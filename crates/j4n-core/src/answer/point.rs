//! The ask points as data (the kind `ask_point`, `packs/base/kinds/ask_point.toml`), their text forms and their lint.
//!
//! A point is a place where a decision is made. Its entry says what is asked (`shape`), what a wrong answer costs
//! (`impact`), how far its data may travel (`data`), who is asked in what order (`chain`) and what stands when no one
//! answers (`fallback`, required: J4n works with no model and no key). `options` lists the fixed option ids of a closed
//! vocabulary (`intake`: answer, command, setting, task, flow); a point whose options come from the machine leaves it out. `says`
//! gives one line for each option on what it means: a decider that reads is shown it beside the id (an id alone, `flow`, tells it
//! nothing).
//! `shape`, `impact`, `data` and `options` are floors: a layer above the one that declared a point cannot loosen them. A
//! project cannot declare a point at all (the kind's default, a project layer is never trusted).
//!
//! # Text forms
//!
//! - A step is `kind[:name][?key=value&…]`: `rule:route`, `rank`, `decider:jev?min_conf=0.6`, `model:light`,
//!   `human?after=60s`. `rule`, `decider` and `model` have a name, `rank` and `human` do not. `min_conf` is a number
//!   and `after` a duration as the Table reads one. Reading is strict (lower case, no spaces, no repeated or unknown
//!   key) but does not judge: whether a value is sensible is the lint's word.
//! - A fallback is `rank`, `safe`, `pick:<id>` or `text:<line>`.
//!
//! # Lint
//!
//! [`lint`] refuses a point whose fallback cannot make a valid answer to its shape (`rank` and `pick` only for a
//! choice, `text:` only for text, `safe` for everything but text); a `human` step that is not the last; a `rank` step on a
//! shape that is not a choice; `min_conf` outside (0, 1]; `after` on a step that is not `human`; an **approval** with a
//! `decider` or `model` step or a fallback but `safe` (approvals never reach a model); a **gate**-impact point whose
//! fallback is not `safe` or whose chain does not end with `human`. A point declared with `options` is also refused
//! when an option is empty, repeated or not one short line, when `pick:<id>` names an id that is not among them, or when
//! `safe` stands for a choice whose options lack `safe`. [`points`] reads the Table, lints, and leaves out a refused point.

use super::{AskPoint, Fallback, Impact, Kind, ShapeKind, Step};
use crate::ask::{Answer, AskShape, OPTIONS_MAX};
use crate::expr::Expr;
use crate::schema::{DataClass, is_id};
use crate::table::{Kind as Declared, Problem, Tables};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// The longest option id or `pick:` id.
const ID_MAX: usize = 64;
/// The longest line that says what an option means.
const SAYS_MAX: usize = 240;

/// Text for an error message: at most 40 characters, control characters shown escaped.
fn shown(s: &str) -> String {
    format!("{:?}", s.chars().take(40).collect::<String>())
}

/// A duration the way the Table reads one (`"60s"`, `"1h30m"`), more than zero.
fn duration(s: &str) -> Result<u64, String> {
    let ok = s.len() <= 16 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '.');
    match Expr::dur(if ok { s } else { "" }) {
        Some(ms) if ms > 0 => Ok(ms),
        _ => Err(format!("{} is not a duration such as \"60s\" or \"1h30m\", more than zero", shown(s))),
    }
}

/// Reads one chain element. See the module doc for the form.
pub fn parse_step(text: &str) -> Result<Step, String> {
    let (head, query) = text.split_once('?').map_or((text, None), |(h, q)| (h, Some(q)));
    let (word, name) = head.split_once(':').map_or((head, None), |(w, n)| (w, Some(n)));
    let Some(kind) = Kind::from_word(word) else {
        return Err(format!("{} is not a kind of answerer (rule, rank, decider, model, human)", shown(word)));
    };
    match (kind, name) {
        (Kind::Rule | Kind::Decider | Kind::Model, Some(n)) if is_id(n) => {}
        (Kind::Rule | Kind::Decider | Kind::Model, _) => {
            return Err(format!("`{word}` needs a name of letters, digits and `_ - . : /`: `{word}:name`"));
        }
        (_, None) => {}
        (_, Some(_)) => return Err(format!("`{word}` has no name")),
    }
    let mut step = Step { kind, name: name.unwrap_or_default().to_string(), min_conf: None, after_ms: None };
    for pair in query.into_iter().flat_map(|q| q.split('&')) {
        match pair.split_once('=') {
            Some(("min_conf", v)) if step.min_conf.is_none() => {
                let conf = v.parse::<f32>().ok().filter(|c| c.is_finite());
                step.min_conf = Some(conf.ok_or_else(|| format!("`min_conf` is a number, not {}", shown(v)))?);
            }
            Some(("after", v)) if step.after_ms.is_none() => step.after_ms = Some(duration(v)?),
            _ => return Err(format!("{} is not `min_conf=<number>` or `after=<duration>`, once each", shown(pair))),
        }
    }
    Ok(step)
}

/// Reads a point's fallback. See the module doc for the form.
pub fn parse_fallback(text: &str) -> Result<Fallback, String> {
    match text.split_once(':') {
        None if text == "rank" => Ok(Fallback::Rank),
        None if text == "safe" => Ok(Fallback::Safe),
        Some(("pick", id))
            if !id.is_empty()
                && id.chars().count() <= ID_MAX
                && !id.contains(|c: char| c.is_whitespace() || c.is_control()) =>
        {
            Ok(Fallback::Pick(id.into()))
        }
        Some(("pick", _)) => Err(format!("`pick:` takes one option id of at most {ID_MAX} characters, no spaces")),
        Some(("text", line)) => {
            let line = line.trim();
            AskShape::Text.check(&Answer::Text { text: line.to_string() })?;
            Ok(Fallback::Text(line.to_string()))
        }
        _ => Err(format!("{} is not a fallback (rank, safe, pick:<id>, text:<line>)", shown(text))),
    }
}

impl fmt::Display for Step {
    /// The text form [`parse_step`] reads back.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let after = self
            .after_ms
            .map(|ms| if ms % 1000 == 0 { format!("after={}s", ms / 1000) } else { format!("after={ms}ms") });
        let params: Vec<String> = self.min_conf.map(|c| format!("min_conf={c}")).into_iter().chain(after).collect();
        write!(f, "{}", self.id())?;
        if params.is_empty() { Ok(()) } else { write!(f, "?{}", params.join("&")) }
    }
}

impl fmt::Display for Fallback {
    /// The text form [`parse_fallback`] reads back.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fallback::Rank => f.write_str("rank"),
            Fallback::Safe => f.write_str("safe"),
            Fallback::Pick(id) => write!(f, "pick:{id}"),
            Fallback::Text(line) => write!(f, "text:{line}"),
        }
    }
}

/// An `ask_point` entry as the Table holds it.
#[derive(Deserialize)]
struct Row {
    id: String,
    #[serde(default)]
    doc: String,
    shape: ShapeKind,
    impact: Impact,
    #[serde(default)]
    data: DataClass,
    #[serde(default)]
    chain: Vec<String>,
    fallback: String,
    rank: Option<String>,
    #[serde(default = "yes")]
    breaker: bool,
    #[serde(default)]
    options: Vec<String>,
    #[serde(default)]
    says: BTreeMap<String, String>,
}

impl Declared for Row {
    const NAME: &'static str = "ask_point";
}

fn yes() -> bool {
    true
}

fn build(r: &Row) -> Result<AskPoint, Vec<String>> {
    let mut errors = Vec::new();
    let mut chain = Vec::new();
    for (i, text) in r.chain.iter().enumerate() {
        match parse_step(text) {
            Ok(step) => chain.push(step),
            Err(e) => errors.push(format!("chain step {}: {e}", i + 1)),
        }
    }
    let fallback = parse_fallback(&r.fallback).map_err(|e| errors.push(format!("fallback: {e}"))).ok();
    match fallback {
        Some(fallback) if errors.is_empty() => Ok(AskPoint {
            id: r.id.clone(),
            doc: r.doc.clone(),
            shape: r.shape,
            impact: r.impact,
            data: r.data,
            chain,
            fallback,
            rank: r.rank.clone(),
            breaker: r.breaker,
        }),
        _ => Err(errors),
    }
}

/// Why `p` (with the fixed `options` it was declared with, if any) is refused; empty when it is clean.
fn refusals(p: &AskPoint, options: &[String]) -> Vec<String> {
    let mut why = Vec::new();
    let choice = p.shape == ShapeKind::Choice;
    let fits = match (&p.fallback, p.shape) {
        (Fallback::Rank | Fallback::Pick(_), _) => choice,
        (Fallback::Text(_), shape) => shape == ShapeKind::Text,
        (Fallback::Safe, shape) => shape != ShapeKind::Text,
    };
    if !fits {
        why.push(format!("the fallback `{}` cannot answer a {} question", p.fallback, p.shape.word()));
    }
    if p.shape == ShapeKind::Approval && p.fallback != Fallback::Safe {
        why.push("an approval's fallback is `safe`".to_string());
    }
    if p.impact == Impact::Gate && (p.fallback != Fallback::Safe || p.chain.last().map(|s| s.kind) != Some(Kind::Human))
    {
        why.push("a gate-impact point has the fallback `safe` and a chain that ends with `human`".to_string());
    }
    for (i, s) in p.chain.iter().enumerate() {
        let at = format!("chain step {} (`{s}`)", i + 1);
        if s.kind == Kind::Human && i + 1 != p.chain.len() {
            why.push(format!("{at}: `human` is the last step; nothing follows it"));
        }
        if s.kind == Kind::Rank && !choice {
            why.push(format!("{at}: `rank` orders the options of a choice"));
        }
        if s.kind.is_model() && p.shape == ShapeKind::Approval {
            why.push(format!("{at}: an approval never reaches a model"));
        }
        if s.min_conf.is_some_and(|c| !(c > 0.0 && c <= 1.0)) {
            why.push(format!("{at}: `min_conf` is above 0 and at most 1"));
        }
        if s.after_ms.is_some() && s.kind != Kind::Human {
            why.push(format!("{at}: only `human` has `after`"));
        }
    }
    if !options.is_empty() {
        let mut seen = BTreeSet::new();
        let clean = |o: &String| !o.is_empty() && o.chars().count() <= ID_MAX && !o.chars().any(char::is_control);
        if !choice || options.len() > OPTIONS_MAX || !options.iter().all(|o| clean(o) && seen.insert(o)) {
            why.push(format!("`options` are for a choice: up to {OPTIONS_MAX} different short lines"));
        }
        match &p.fallback {
            Fallback::Pick(id) if !options.contains(id) => why.push(format!("`pick:{id}` is not one of the options")),
            Fallback::Safe if choice && !options.iter().any(|o| o == "safe") => {
                why.push("`safe` needs an option with the id `safe`".to_string());
            }
            _ => {}
        }
    }
    why
}

/// What is wrong with the lines that say what each option means: each names an option the point declares, in one short line.
fn says_faults(r: &Row) -> Vec<String> {
    let mut why = Vec::new();
    for (id, line) in &r.says {
        if !r.options.contains(id) {
            why.push(format!("`says` describes `{id}`, which is not one of the options"));
        } else if line.trim().is_empty() || line.chars().count() > SAYS_MAX || line.chars().any(char::is_control) {
            why.push(format!("`says.{id}` is one short line of at most {SAYS_MAX} characters"));
        }
    }
    why
}

fn problems(id: &str, why: Vec<String>) -> impl Iterator<Item = Problem> {
    let at = format!("[[ask_point]] {id}");
    why.into_iter().map(move |w| Problem::refused(at.clone(), w))
}

/// What is wrong with these points; empty when all are clean. The `options` rules need the entry, so [`points`] applies
/// them; here only what a built [`AskPoint`] shows is judged.
pub fn lint(points: &[AskPoint]) -> Vec<Problem> {
    points.iter().flat_map(|p| problems(&p.id, refusals(p, &[]))).collect()
}

/// Every `ask_point` of the Table, parsed and linted; a point that does not read or is refused is left out and reported.
pub fn points(tables: &Tables) -> (Vec<AskPoint>, Vec<Problem>) {
    let (mut out, mut found) = (Vec::new(), Vec::new());
    for id in tables.ids("ask_point") {
        let row = match tables.get::<Row>(id) {
            Ok(row) => row,
            Err(p) => {
                found.push(p);
                continue;
            }
        };
        match build(&row) {
            Ok(p) => {
                let mut why = refusals(&p, &row.options);
                why.extend(says_faults(&row));
                if why.is_empty() {
                    out.push(p);
                } else {
                    found.extend(problems(id, why));
                }
            }
            Err(why) => found.extend(problems(id, why)),
        }
    }
    (out, found)
}

/// The fixed option ids a point was declared with (empty for a point whose options come from the machine).
pub fn options(tables: &Tables, point: &str) -> Vec<String> {
    tables.get::<Row>(point).map(|r| r.options).unwrap_or_default()
}

/// What each option of a point means, in one line (`says` of the point), by point: the words an answerer that reads is shown
/// beside the option id, for the options an asker offers as bare ids. Points that say nothing are left out.
pub fn says(tables: &Tables) -> BTreeMap<String, BTreeMap<String, String>> {
    tables.ids("ask_point").into_iter().filter_map(|id| tables.get::<Row>(id).ok().filter(|r| !r.says.is_empty()).map(|r| (r.id, r.says))).collect()
}
