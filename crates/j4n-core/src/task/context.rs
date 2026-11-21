//! What a doer's session holds: from the tasks the Book folds out of the Log (`run.session`, `run.metrics`,
//! `run.served`, `task.settled.window`) and nothing else. A **session** is the turns of one doer under one session id in one chat. Its
//! **floor** is what its first turn read before any word of the operator's (the first call of the first turn), its **held** size is what its
//! newest turn read, and the **history** is the part of that the session has grown by. The **next line** is warm while the last turn is no
//! older than the provider's cache life (a `cost_source` row's `cache`) and cold after it: all of `held` is then written again.

use super::{Book, Row, State, id_number};
use crate::cost::{Plans, Ratios};
use crate::setting::Settings;
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;

/// One doer's session in one chat, as the Log's tasks say it.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub thread: String,
    pub doer: String,
    pub id: String,
    /// `doer · model` of the newest turn, and the model's id as the Log has it.
    pub who: String,
    pub model: String,
    /// How many of its turns read anything (a turn that read nothing is not one).
    pub turns: u32,
    /// What its first turn read, when the Log holds that turn: the one task of the session that was not told to resume it. A session whose
    /// first turn is not in the Log (it began before) has none.
    pub floor: Option<u64>,
    /// What the newest turn read, in tokens: the size the next line starts from.
    pub held: u64,
    /// How full the doer said its window was after the newest turn that said, 0 to 1.
    pub window: Option<f64>,
    /// When the last turn ended; a turn still running is as old as `now`.
    pub last_ms: u64,
    /// A turn of it runs now.
    pub busy: bool,
    /// The newest `:compact` of it: what it held before and, when the doer said it, after.
    pub shrunk: Option<Shrunk>,
    /// The newest row is a `:compact` whose result the doer did not say: `held` is what the session held before it, and is not known now until
    /// the next line reads it.
    pub unsure: bool,
}

/// One `:compact` as the Log's tasks say it.
#[derive(Debug, Clone, PartialEq)]
pub struct Shrunk {
    /// What the session held before, in tokens: what the turn before it left.
    pub before: u64,
    /// What it holds after, when the doer says it. None: the next line that reads the session shows it.
    pub after: Option<u64>,
    /// The mode that was asked for (`plain`, `junk`, `keep`), and when it ended.
    pub mode: String,
    pub at_ms: u64,
}

impl Session {
    /// What the session has grown by since its first turn; none when the floor is not known.
    pub fn history(&self) -> Option<u64> {
        self.floor.map(|f| self.held.saturating_sub(f))
    }

    /// Whether the next line finds the cache cold at `now_ms`: the last turn is older than the cache life. None: the provider's cache life is
    /// not said. A turn that runs now keeps the cache warm.
    pub fn cold(&self, now_ms: u64, cache_ms: Option<u64>) -> Option<bool> {
        cache_ms.map(|c| !self.busy && now_ms.saturating_sub(self.last_ms) > c)
    }
}

/// What a turn left the session holding: what its last call read when the doer said it, else what the turn read (`tokens_in` adds the calls up).
/// A `:compact` task read the whole session to write its summary, so what it read is not what the session holds: it holds what the doer
/// said it holds after (`last`), and nothing known when it did not say.
fn size(r: &Row) -> u64 {
    if r.last > 0 || r.spec.shrinks_only() { r.last } else { r.tokens_in }
}

/// What a `:compact` task of `rows` (oldest first) took the session from and to: the size the turn before it left, and the size it says it
/// left; `(None, None)` for a task that is not one or has no turn before it.
pub fn shrink(rows: &[Row], task: &str) -> (Option<u64>, Option<u64>) {
    shrink_in(&rows.iter().collect::<Vec<_>>(), task)
}

fn shrink_in(rows: &[&Row], task: &str) -> (Option<u64>, Option<u64>) {
    let Some(at) = rows.iter().position(|r| r.id == task && r.spec.shrinks_only()) else { return (None, None) };
    let before = rows[..at].iter().rev().map(|r| size(r)).find(|n| *n > 0);
    (before, Some(size(rows[at])).filter(|n| *n > 0))
}

/// The newest `:compact` of a session that ended well.
fn shrunk_of(rows: &[&Row]) -> Option<Shrunk> {
    let at = rows.iter().rposition(|r| r.spec.shrinks_only() && r.state == State::Done { ok: true })?;
    let (before, after) = shrink_in(&rows[..=at], &rows[at].id);
    Some(Shrunk { before: before?, after, mode: rows[at].spec.input.get("compact_mode").and_then(Json::as_str).unwrap_or("plain").to_string(), at_ms: rows[at].ended_ms.unwrap_or(rows[at].queued_ms) })
}

/// What a session's first turn read before any word: its first call when the doer said it, else the turn.
fn start(r: &Row) -> u64 {
    if r.first > 0 { r.first } else { size(r) }
}

fn session_of(rows: &[&Row]) -> Option<Session> {
    let (first, last) = (rows.first()?, rows.last()?);
    let turns: Vec<&&Row> = rows.iter().filter(|r| size(r) > 0).collect();
    let newest = turns.last()?;
    let end = |r: &Row| r.ended_ms.or(r.started_ms).unwrap_or(r.queued_ms);
    Some(Session {
        thread: first.spec.thread.clone().unwrap_or_default(),
        doer: first.session.as_ref().map(|s| s.doer.clone()).unwrap_or_default(),
        id: first.session.as_ref().map(|s| s.id.clone()).unwrap_or_default(),
        who: newest.who(),
        model: newest.model.clone(),
        turns: turns.len().min(u32::MAX as usize) as u32,
        floor: floor(rows),
        held: size(newest),
        window: rows.iter().rev().find_map(|r| r.window),
        last_ms: rows.iter().map(|r| end(r)).max().unwrap_or(end(last)),
        busy: rows.iter().any(|r| !r.state.is_terminal()),
        shrunk: shrunk_of(rows),
        unsure: last.spec.shrinks_only() && last.state == State::Done { ok: true } && size(last) == 0,
    })
}

/// The floor of a session from its rows, oldest first: its first turn, when that task began the session.
fn floor(rows: &[&Row]) -> Option<u64> {
    rows.first().filter(|r| r.spec.resume.is_none() && size(r) > 0).map(|r| start(r))
}

/// The live session of each doer in each chat: of the rows that have a session (the tasks that are not a question), the newest session of a
/// doer in a thread. The caller leaves out the rows a `:fresh` ended. Ordered by thread, then by doer.
pub fn sessions(rows: &[Row]) -> Vec<Session> {
    let mut by: BTreeMap<(String, String, String), Vec<&Row>> = BTreeMap::new();
    for r in rows.iter().filter(|r| r.spec.doer != super::HUMAN) {
        if let (Some(s), Some(t)) = (&r.session, &r.spec.thread) {
            by.entry((t.clone(), s.doer.clone(), s.id.clone())).or_default().push(r);
        }
    }
    // the newest of a doer's sessions in a thread is the one whose last task is the newest
    let mut newest: BTreeMap<(String, String), (u64, Session)> = BTreeMap::new();
    for ((thread, doer, _), rows) in by {
        let (Some(last), Some(s)) = (rows.last().and_then(|r| id_number(&r.id)), session_of(&rows)) else { continue };
        let slot = newest.entry((thread, doer)).or_insert_with(|| (0, s.clone()));
        if last >= slot.0 {
            *slot = (last, s);
        }
    }
    newest.into_values().map(|(_, s)| s).collect()
}

/// What `task.settled` says of the turn that just ended: `{"in", "floor", "hist", "cold", "warm_usd", "cold_usd"}`, each left out when it is not known.
/// `rows` are the tasks of the turn's session, oldest first, `task` among them. `cold` is whether the turn found the cache cold when it began:
/// the first turn of a session always writes what it reads; a later one is cold when the turn before it ended more than the cache life
/// before it started. None: the turn read nothing.
pub fn turn(rows: &[Row], task: &str, cache_ms: Option<u64>, price: Option<&Price>) -> Option<Json> {
    let at = rows.iter().position(|r| r.id == task)?;
    let r = &rows[at];
    let held = size(r);
    if held == 0 {
        return None;
    }
    let floor = floor(&rows.iter().collect::<Vec<_>>());
    let cold = match rows[..at].iter().rev().find(|p| size(p) > 0) {
        None => Some(true),
        Some(p) => p.ended_ms.zip(cache_ms).map(|(end, c)| r.started_ms.unwrap_or(r.queued_ms).saturating_sub(end) > c),
    };
    let mut out = json!({ "in": held });
    // a `:compact`: what the session held before it, beside what it holds now
    if let Some(b) = shrink(rows, task).0.filter(|_| r.spec.shrinks_only()) {
        (out["before"], out["cut"]) = (json!(b), json!(b.saturating_sub(held)));
    }
    if let Some(f) = floor {
        (out["floor"], out["hist"]) = (json!(f), json!(held.saturating_sub(f)));
    }
    if let Some(c) = cold {
        out["cold"] = json!(c);
    }
    // what the line after this turn costs, warm and cold, when the price is known
    if let Some((warm, cold)) = price.map(|p| p.next(held)) {
        (out["warm_usd"], out["cold_usd"]) = (json!((warm * 1e6).round() / 1e6), json!((cold * 1e6).round() / 1e6));
    }
    Some(out)
}

/// The price of one fresh input token as one task says it: its list cost over what it read and wrote, each at the provider's ratio. None: it has
/// no cost or no tokens.
pub fn unit_of(r: &Row, x: &Ratios) -> Option<f64> {
    let (cached, wrote) = (r.cached as f64, r.wrote as f64);
    let fresh = (r.tokens_in as f64 - cached - wrote).max(0.0);
    let equiv = fresh + x.read * cached + x.write * wrote + x.out * r.tokens_out as f64;
    (r.listed > 0 && equiv > 0.0).then(|| r.listed as f64 / 1e6 / equiv)
}

/// What a token costs a doer on a model, and the size of a typical small line: what the next line of a session costs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    pub unit: f64,
    pub ratios: Ratios,
    /// A typical small line, in fresh input tokens and in output tokens.
    pub line: (f64, f64),
}

impl Price {
    /// The price of `doer` on `model` from the provider's ratios (its `cost_source` row) and a task of that model the Log has priced, with the
    /// size of a line from the settings `context.line.in` and `context.line.out`. None when any of them is not there: the price is not known.
    pub fn of(book: &Book, doer: &str, model: &str, plans: &Plans, settings: &Settings) -> Option<Price> {
        let ratios = plans.ratios(doer)?;
        let unit = book.unit(doer, model, &ratios).filter(|_| !model.is_empty())?;
        Some(Price { unit, ratios, line: (settings.num("context.line.in", 0.0), settings.num("context.line.out", 0.0)) })
    }

    /// What the next line costs in dollars when a session holds `held` tokens, `(warm, cold)`: the session read from the cache or written to it
    /// again, and the line and its answer on top.
    pub fn next(&self, held: u64) -> (f64, f64) {
        let line = self.unit * (self.line.0 + self.line.1 * self.ratios.out);
        (self.unit * held as f64 * self.ratios.read + line, self.unit * held as f64 * self.ratios.write + line)
    }
}

/// `108k`, `30k`, `950`: a token count the way the view says it.
pub fn k(n: u64) -> String {
    if n >= 1000 { format!("{}k", (n + 500) / 1000) } else { n.to_string() }
}
