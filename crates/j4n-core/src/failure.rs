//! Failure words (the kind `failure`, `packs/base/kinds/failure.toml`): what kind of failure a run ended with, read once from
//! the words it gave and by no model.
//!
//! - `transient` is what a failure is when no row knows it: work should go on, so that is the answer that fails open.
//! - `rate-limit` is the provider saying "not now". Trying at once makes it worse, so the task waits the limit out: until the
//!   time the failure names (`(resets at <unix seconds>)`), else the row's `wait`.
//! - `permanent` is a failure that trying again cannot cure: a login, a model that is gone, a program that is not there.
//! - `refused` is a run that ended on a tool the program was not allowed to use; `tool` is one that ended on a tool that failed. Neither is the
//!   provider's or the network's: nothing keys on `transient` for them (`retry.auto`, the host's error count), so they are not tried again by themselves.
//!
//! The first row by `order` whose words occur wins; an entry of digits only is a status code and must stand alone.

use crate::table::{Kind, Problem, Tables};
use serde::Deserialize;

/// Added to the time a limit says it ends, so the window has opened when the task goes on.
const MARGIN_MS: u64 = 2_000;
/// The longest word of a row.
const WORD_MAX: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Class {
    #[serde(rename = "transient")]
    Transient,
    #[serde(rename = "rate-limit")]
    RateLimit,
    #[serde(rename = "permanent")]
    Permanent,
    #[serde(rename = "refused")]
    Refused,
    #[serde(rename = "tool")]
    Tool,
}

impl Class {
    pub fn word(self) -> &'static str {
        match self {
            Class::Transient => "transient",
            Class::RateLimit => "rate-limit",
            Class::Permanent => "permanent",
            Class::Refused => "refused",
            Class::Tool => "tool",
        }
    }
}

/// An entry of kind `failure`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailureRow {
    pub id: String,
    pub doc: Option<String>,
    pub class: Class,
    pub any: Vec<String>,
    #[serde(default = "hundred")]
    pub order: i64,
    pub wait: Option<String>,
}

fn hundred() -> i64 {
    100
}

impl Kind for FailureRow {
    const NAME: &'static str = "failure";
}

struct Row {
    id: String,
    class: Class,
    any: Vec<String>,
    wait_ms: Option<u64>,
}

/// What the words of a failure said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    /// The row that knew them (none: nothing did, and it is `transient`).
    pub row: Option<String>,
    pub class: Class,
    /// For a rate limit: how long until it ends.
    pub wait_ms: Option<u64>,
}

#[derive(Default)]
pub struct Failures(Vec<Row>);

/// Does `m` hold `word`? A word of digits (a status code) must stand alone, not sit inside a longer number or word.
fn has(m: &str, word: &str) -> bool {
    if !word.bytes().all(|b| b.is_ascii_digit()) {
        return m.contains(word);
    }
    m.match_indices(word).any(|(at, _)| {
        let before = m[..at].chars().next_back();
        let after = m[at + word.len()..].chars().next();
        !before.is_some_and(|c| c.is_ascii_alphanumeric()) && !after.is_some_and(|c| c.is_ascii_alphanumeric())
    })
}

/// The time a failure says its limit ends: `(resets at <unix seconds>)`, in milliseconds since 1970.
fn resets_at(message: &str) -> Option<u64> {
    let rest = &message[message.rfind("resets at ")? + "resets at ".len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let secs: u64 = digits.parse().ok().filter(|s| (1_000_000_000..100_000_000_000).contains(s))?;
    Some(secs * 1000)
}

impl Failures {
    /// The rows of the Table. A row that cannot be used is a problem and no row.
    pub fn load(tables: &Tables) -> (Failures, Vec<Problem>) {
        let (mut rows, mut problems) = (Vec::new(), Vec::new());
        let mut list = Vec::new();
        for id in tables.ids(FailureRow::NAME) {
            match tables.get::<FailureRow>(id) {
                Ok(r) => list.push(r),
                Err(p) => problems.push(p),
            }
        }
        for r in list {
            let at = format!("failure {}", r.id);
            let any: Vec<String> = r.any.iter().map(|w| w.trim().to_lowercase()).collect();
            if any.is_empty() || any.iter().any(|w| w.is_empty() || w.len() > WORD_MAX) {
                problems.push(Problem::refused(at, format!("`any` lists words of 1 to {WORD_MAX} characters, at least one")));
                continue;
            }
            let wait_ms = match r.wait.as_deref().map(|w| (w, crate::expr::Expr::dur(w))) {
                None => None,
                Some((_, Some(ms))) if ms > 0 && r.class == Class::RateLimit => Some(ms),
                Some((w, _)) => {
                    problems.push(Problem::refused(at, format!("`wait` is a duration, more than zero, and only a rate limit has one, not `{w}`")));
                    continue;
                }
            };
            rows.push((r.order, Row { id: r.id, class: r.class, any, wait_ms }));
        }
        rows.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.id.cmp(&b.1.id)));
        (Failures(rows.into_iter().map(|(_, r)| r).collect()), problems)
    }

    /// What `message` says, as of `now_ms`.
    pub fn classify(&self, message: &str, now_ms: u64) -> Seen {
        let m = message.to_lowercase();
        let Some(row) = self.0.iter().find(|r| r.any.iter().any(|w| has(&m, w))) else {
            return Seen { row: None, class: Class::Transient, wait_ms: None };
        };
        let wait_ms = (row.class == Class::RateLimit)
            .then(|| resets_at(message).map(|at| at.saturating_sub(now_ms).saturating_add(MARGIN_MS)).or(row.wait_ms))
            .flatten();
        Seen { row: Some(row.id.clone()), class: row.class, wait_ms }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
