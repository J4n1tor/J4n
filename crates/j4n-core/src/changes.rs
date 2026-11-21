//! Table writes in the Log: the [`Journal`] that records a [`Change`] before the
//! file is touched, and the fold of those records that is the undo stack.
//!
//! - `change.recorded` holds the change; `change.failed` says one was not applied. An undo is a change like another, whose
//!   cause is `undo`.
//! - A crash between the record and the rename leaves a change recorded and not applied. [`Changes::interrupted`]
//!   finds them by comparing the latest change to each thing with what the tables now hold; the caller writes
//!   `change.failed` for them.
//! - The stack ([`Changes::active`]) is the changes in effect: not failed, not themselves undos, not undone (an undo that is
//!   itself undone does not count). A session-layer change from before this run is gone with its session and not on it.

use crate::change::{Actor, Cause, Change, Where};
use crate::fold::Fold;
use crate::log::{Ev, Log, New};
use crate::table::Tables;
use crate::value::{Key, Value};
use crate::write::Journal;
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

pub const RECORDED: &str = "change.recorded";
pub const FAILED: &str = "change.failed";
/// The most changes the fold keeps: how far back an undo reaches.
pub const KEEP: usize = 1000;

/// Records every change in the Log before the writer touches the file.
pub struct LogJournal<'a> {
    pub log: &'a Log,
}

impl Journal for LogJournal<'_> {
    fn record(&mut self, change: &Change) -> Result<(), String> {
        let data = serde_json::to_value(change).map_err(|e| e.to_string())?;
        let mut ev = New::of(RECORDED, change.by.clone(), data);
        if let Some(Cause::Event(n)) = change.cause {
            ev.cause = Some(n);
        }
        self.log.append(ev).map(|_| ()).map_err(|e| e.to_string())
    }

    fn failed(&mut self, change: &Change, why: &str) {
        let _ = self.log.append(New::of(FAILED, Actor::Core, json!({ "id": change.id, "why": why })));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Rec {
    seq: u64,
    change: Change,
    failed: bool,
}

/// The fold `changes`. A clone is a second handle on the same state: add one to the fold host and keep the other to ask.
#[derive(Debug, Clone, Default)]
pub struct Changes(Arc<Mutex<Vec<Rec>>>);

fn undone(recs: &[Rec], id: &str, depth: usize) -> bool {
    depth < 64
        && recs.iter().any(|r| {
            !r.failed
                && matches!(&r.change.cause, Some(Cause::Undo(x)) if x == id)
                && !undone(recs, &r.change.id, depth + 1)
        })
}

impl Changes {
    pub fn new() -> Changes {
        Changes::default()
    }

    fn recs(&self) -> MutexGuard<'_, Vec<Rec>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The changes in effect, newest first. `live_from` is the first sequence number of this run.
    pub fn active(&self, live_from: u64) -> Vec<Change> {
        let recs = self.recs();
        recs.iter()
            .rev()
            .filter(|r| !r.failed && !matches!(r.change.cause, Some(Cause::Undo(_))))
            .filter(|r| r.change.target.layer != Where::Session || r.seq >= live_from)
            .filter(|r| !undone(&recs, &r.change.id, 0))
            .map(|r| r.change.clone())
            .collect()
    }

    /// The numbers of the operator's edits the stack knows (`e3` is 3), whatever became of them.
    pub fn edits(&self) -> Vec<u64> {
        self.recs().iter().filter_map(|r| match &r.change.cause {
            Some(Cause::Edit(e)) => e.strip_prefix('e').and_then(|n| n.parse().ok()),
            _ => None,
        }).collect()
    }

    /// Ids of the changes recorded and not applied: the latest to a thing, in a layer that is loaded, whose `after` the
    /// tables do not hold and whose `before` they do. One the tables hold in neither form was overtaken by a hand edit and
    /// is left alone.
    pub fn interrupted(&self, tables: &Tables) -> Vec<String> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for r in self.recs().iter().rev().filter(|r| !r.failed) {
            let t = &r.change.target;
            if t.layer == Where::Session || !tables.has_layer(t.layer) {
                continue;
            }
            if !seen.insert((t.layer.word(), t.kind.clone(), t.id.clone())) {
                continue;
            }
            let now = tables.layer_entry(t.layer, &t.kind, &t.id);
            if now != r.change.after.as_ref() && now == r.change.before.as_ref() {
                out.push(r.change.id.clone());
            }
        }
        out
    }
}

impl Fold for Changes {
    fn id(&self) -> &str {
        "changes"
    }

    fn version(&self) -> u64 {
        1
    }

    fn wants(&self, kind: &str) -> bool {
        kind == RECORDED || kind == FAILED
    }

    fn apply(&mut self, ev: &Ev) {
        let mut recs = self.recs();
        if ev.kind == RECORDED {
            if let Ok(change) = serde_json::from_value::<Change>(ev.data.clone())
                && !recs.iter().any(|r| r.change.id == change.id)
            {
                recs.push(Rec { seq: ev.seq, change, failed: false });
                if recs.len() > KEEP {
                    recs.remove(0);
                }
            }
        } else if let Some(id) = ev.data.get("id").and_then(Json::as_str) {
            recs.iter_mut().filter(|r| r.change.id == id).for_each(|r| r.failed = true);
        }
    }

    fn read(&self, path: &[Key], _now_ms: u64) -> Value {
        let row = |c: &Change| {
            let text = |s: &str| Value::text(s);
            Value::Map(BTreeMap::from([
                ("id".to_string(), text(&c.id)),
                ("layer".to_string(), text(c.target.layer.word())),
                ("kind".to_string(), text(&c.target.kind)),
                ("entry".to_string(), text(&c.target.id)),
                ("by".to_string(), text(&crate::log::actor_word(&c.by))),
                ("at_ms".to_string(), Value::num(c.at_ms as f64)),
            ]))
        };
        Value::List(self.active(0).iter().map(row).collect()).at(path)
    }

    fn save(&self) -> Json {
        serde_json::to_value(&*self.recs()).unwrap_or_default()
    }

    fn load(&mut self, state: Json) -> Result<(), String> {
        *self.recs() = serde_json::from_value(state).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn reset(&mut self) {
        self.recs().clear();
    }
}
