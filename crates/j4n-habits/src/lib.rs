//! The label of a habit row. The detectors are pack data (`packs/habits/`): each says a `run.notice` from a
//! rule `habit-…`, ending in `h<seq>`, the event that set it off. What needs code is the operator's word on a row, because it writes a `label`
//! event: `:habit keep h4521` / `:habit wrong h4521` (the first word on a row counts), and `:habit list` says each rule's labels and its
//! precision `kept / (kept + wrong)` over the last thirty days. Nothing here reads the switch: a row exists only when a rule said it.

use j4n_connect::connect::{Job, StepDoer};
use j4n_core::change::Actor;
use j4n_core::doer::Doer;
use j4n_core::log::{Ev, Log, New};
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::sync::Arc;

/// The doer the flow `habit` runs.
pub const DOER: &str = "habit";
/// The window a precision is counted over: thirty days.
const WITHIN_MS: u64 = 30 * 24 * 3_600_000;

fn s<'a>(d: &'a Json, k: &str) -> &'a str {
    d.get(k).and_then(Json::as_str).unwrap_or_default()
}

/// The doer of `:habit`.
pub fn doer(log: &Log) -> Arc<dyn Doer> {
    let log = log.clone();
    Arc::new(StepDoer { id: DOER, doc: "labels a habit row kept or wrong and says how each detector has done", work: Arc::new(move |job| run(&log, job)) })
}

/// A row of a detector: the notice a rule `habit-…` said.
fn is_row(e: &Ev) -> bool {
    e.kind == "run.notice" && s(&e.data, "rule").starts_with("habit-")
}

fn run(log: &Log, job: &Job) -> Result<String, String> {
    log.sync();
    let words: Vec<&str> = job.goal.split_whitespace().collect();
    match words.as_slice() {
        [] | ["list"] => Ok(precision(log)),
        [word @ ("keep" | "wrong"), id] => label(log, word, id),
        _ => Err("`:habit list` says how each detector has done; `:habit keep h4521` or `:habit wrong h4521` says what a row was".into()),
    }
}

fn label(log: &Log, word: &str, id: &str) -> Result<String, String> {
    let seq: u64 = id.strip_prefix('h').and_then(|n| n.parse().ok()).ok_or_else(|| format!("`{id}` is not a row: a row ends in its id, such as h4521"))?;
    let row = log.read(0).find(|e| is_row(e) && e.cause == Some(seq)).ok_or_else(|| format!("there is no habit row {id}"))?;
    let rule = s(&row.data, "rule").to_string();
    // only the first word of the operator on a row is a label of its rule
    if let Some(l) = log.read(0).find(|e| e.kind == "label" && s(&e.data, "subject_kind") == "habit" && s(&e.data, "subject") == id) {
        return Err(format!("{id} is already {}: the first word on a row is the one that counts", s(&l.data, "label")));
    }
    let label = if word == "keep" { "kept" } else { "wrong" };
    let mut n = New::of("label", Actor::Operator, json!({ "subject_kind": "habit", "subject": id, "producer": rule, "model": "", "point": "", "label": label, "weight": 1.0 }));
    if let Some(t) = &row.thread {
        n = n.thread(t);
    }
    log.append(n).map_err(|p| p.msg)?;
    Ok(format!("{id} is {label} ({rule})"))
}

/// Each rule's rows and labels, and `kept / (kept + wrong)` over thirty days.
fn precision(log: &Log) -> String {
    let now = log.clock().now_ms();
    let mut rules: BTreeMap<String, [u64; 3]> = BTreeMap::new();
    for e in log.read(0) {
        if is_row(&e) {
            rules.entry(s(&e.data, "rule").to_string()).or_default()[0] += 1;
        } else if e.kind == "label" && s(&e.data, "subject_kind") == "habit" && now.saturating_sub(e.at_ms) <= WITHIN_MS {
            let slot = rules.entry(s(&e.data, "producer").to_string()).or_default();
            slot[if s(&e.data, "label") == "kept" { 1 } else { 2 }] += 1;
        }
    }
    if rules.is_empty() {
        return "no habit has been seen yet (the switch is the setting habits.mode)".into();
    }
    rules
        .iter()
        .map(|(rule, [rows, kept, wrong])| match kept + wrong {
            0 => format!("{rule} · {rows} rows · not labelled yet"),
            n => format!("{rule} · {rows} rows · kept {kept} · wrong {wrong} · precision {:.2} of {n}", *kept as f64 / n as f64),
        })
        .collect::<Vec<_>>()
        .join("\n")
}
