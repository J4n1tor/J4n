//! Folds: every ledger is a fold of the Log (law 3). A [`Fold`] reads events in order and keeps
//! some state; the [`Folds`] host feeds it, snapshots it, and starts it again from the snapshot.
//!
//! - **Replay**: a fold is a function of the events and nothing else — never of the clock or of when it was started
//!   (`since(...)` in a fold reads the event's own time). So a fold started from a snapshot at `seq` and fed the events
//!   after it ends in exactly the state a fold fed every event ends in. That is the property the host exists for, and its
//!   tests state it.
//! - **Snapshots** go to `<dir>/<id>.v<version>.json` as `{fold, version, seq, state}`, written whenever the Log
//!   checkpoints (after it has synced, so a snapshot is never ahead of the disk). A snapshot whose version is not the
//!   fold's, whose `seq` is past the log's head, or that cannot be loaded is not used: the fold replays from the start,
//!   and the host says so.
//! - **Forward compatibility**: a fold skips the event kinds it does not `want` and reads the fields it knows;
//!   it never fails on an event it does not understand.
//! - A fold that panics is dropped from the host and noted; the Log and the other folds go on.

mod decl;

pub use decl::{DeclFold, FoldDecl, Reduce, Unit, declared};

use crate::log::{Ev, Log, Sink};
use crate::state::private_dir;
use crate::table::Problem;
use crate::value::{Key, Value};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

/// A reduction of the Log. `apply` is called for the events the fold `wants`, in order, once each.
pub trait Fold: Send {
    fn id(&self) -> &str;

    /// Any change to what the fold computes changes its version, so a snapshot of the old one is not used.
    fn version(&self) -> u64;

    fn wants(&self, kind: &str) -> bool;

    fn apply(&mut self, ev: &Ev);

    /// What the fold says at `path` (empty: all of it), as of `now_ms` (for windows that end at the present).
    fn read(&self, path: &[Key], now_ms: u64) -> Value;

    /// The state, to be written to a snapshot.
    fn save(&self) -> Json;

    /// Replace the state with a saved one. On an error the fold is [`reset`](Fold::reset) by the host.
    fn load(&mut self, state: Json) -> Result<(), String>;

    fn reset(&mut self);

    /// Things a reader should know about what the fold says (a cap that was hit).
    fn notes(&self) -> Vec<String> {
        Vec::new()
    }
}

#[derive(Serialize, Deserialize)]
struct Snapshot {
    fold: String,
    version: u64,
    seq: u64,
    state: Json,
}

struct Slot {
    fold: Box<dyn Fold>,
    /// The last event that went through the host while this fold was in it.
    applied: u64,
    /// Put in after the host had gone through events it has not: live events pass it by until [`catch_up`](Folds::catch_up)
    /// has fed it those. One it took would carry `applied` past them, and the catch-up, which starts after `applied`, would
    /// not give them.
    behind: bool,
}

struct Host {
    dir: PathBuf,
    /// The last event that went through the host.
    last: u64,
    slots: Vec<Slot>,
    problems: Vec<Problem>,
}

/// The fold host. Clones share it. It is a [`Sink`] of the Log; reading it is cheap and does not wait for the writer.
#[derive(Clone)]
pub struct Folds(Arc<Mutex<Host>>);

/// A fold's id as a file name: letters, digits, `_ - .` as they are, everything else `%XX`.
fn file_id(id: &str) -> String {
    id.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.') { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

fn snapshot_path(dir: &Path, id: &str, version: u64) -> PathBuf {
    dir.join(format!("{}.v{version}.json", file_id(id)))
}

impl Host {
    fn note(&mut self, p: Problem) {
        if self.problems.len() < 100 {
            self.problems.push(p);
        }
    }

    fn write_snapshot(&self, slot: &Slot) -> Result<(), String> {
        let (id, version) = (slot.fold.id().to_string(), slot.fold.version());
        private_dir(&self.dir).map_err(|e| e.to_string())?;
        let snap = Snapshot { fold: id.clone(), version, seq: slot.applied, state: slot.fold.save() };
        let text = serde_json::to_vec(&snap).map_err(|e| e.to_string())?;
        let path = snapshot_path(&self.dir, &id, version);
        let tmp = path.with_extension("json.tmp");
        let mut f = fs::File::create(&tmp).map_err(|e| e.to_string())?;
        f.write_all(&text).and_then(|()| f.sync_all()).map_err(|e| e.to_string())?;
        fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
        // Older versions of this fold are of no use now.
        let prefix = format!("{}.v", file_id(&id));
        for entry in fs::read_dir(&self.dir).map_err(|e| e.to_string())?.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_old = name.strip_prefix(&prefix).is_some_and(|rest| {
                rest.strip_suffix(".json").is_some_and(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
            });
            if is_old && entry.path() != path {
                let _ = fs::remove_file(entry.path());
            }
        }
        Ok(())
    }
}

impl Folds {
    /// A host that keeps its snapshots under `dir`.
    pub fn new(dir: &Path) -> Folds {
        Folds(Arc::new(Mutex::new(Host { dir: dir.to_path_buf(), last: 0, slots: Vec::new(), problems: Vec::new() })))
    }

    fn host(&self) -> MutexGuard<'_, Host> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Put `fold` in the host, from its snapshot when there is a usable one (`head` is the log's last sequence number).
    /// A fold with the same id is replaced. It has seen nothing past its snapshot until [`attach`](Folds::attach) or
    /// [`catch_up`](Folds::catch_up) feeds it.
    pub fn add(&self, mut fold: Box<dyn Fold>, head: u64) -> Vec<Problem> {
        let mut host = self.host();
        let (id, version) = (fold.id().to_string(), fold.version());
        let path = snapshot_path(&host.dir, &id, version);
        let mut problems = Vec::new();
        let mut applied = 0;
        if let Ok(text) = fs::read(&path) {
            let at = format!("fold {id}");
            let snap: Result<Snapshot, _> = serde_json::from_slice(&text);
            match snap {
                Ok(s) if s.fold == id && s.version == version && s.seq <= head => match fold.load(s.state) {
                    Ok(()) => applied = s.seq,
                    Err(e) => {
                        fold.reset();
                        problems.push(Problem::warn(at, format!("its snapshot could not be loaded ({e}) — replaying from the start")));
                    }
                },
                Ok(s) if s.seq > head => problems.push(Problem::warn(
                    at,
                    format!("its snapshot is at {} and the log ends at {head} — replaying from the start", s.seq),
                )),
                Ok(_) => problems.push(Problem::warn(at, "its snapshot is for another fold or version — replaying from the start")),
                Err(e) => problems.push(Problem::warn(at, format!("its snapshot is unreadable ({e}) — replaying from the start"))),
            }
        }
        host.slots.retain(|s| s.fold.id() != id);
        let behind = host.last > applied;
        host.slots.push(Slot { fold, applied, behind });
        problems
    }

    pub fn remove(&self, id: &str) -> bool {
        let mut host = self.host();
        let before = host.slots.len();
        host.slots.retain(|s| s.fold.id() != id);
        host.slots.len() != before
    }

    /// Register as a sink of `log`: replay every event the folds have not seen, then follow the log.
    pub fn attach(&self, log: &Log) -> Result<Vec<Problem>, Problem> {
        let after = self.host().slots.iter().map(|s| s.applied).min().unwrap_or(0);
        log.attach(Box::new(self.clone()), after)
    }

    /// Feed the folds the events of `log` they have not seen. For a fold added to a host that is already attached: it takes
    /// no live event before this. The host is held for the reading, so an event written meanwhile waits, and one the reading
    /// did not reach (a line half written) comes after it: none is missed or counted twice.
    pub fn catch_up(&self, log: &Log) -> Vec<Problem> {
        let mut host = self.host();
        let from = host.slots.iter().map(|s| s.applied).min().unwrap_or(0) + 1;
        let mut reader = log.read(from);
        for ev in reader.by_ref() {
            Folds::feed(&mut host, &ev, true);
        }
        host.slots.iter_mut().for_each(|s| s.behind = false);
        reader.into_problems()
    }

    fn feed(host: &mut Host, ev: &Ev, replay: bool) {
        host.last = host.last.max(ev.seq);
        let mut broken = Vec::new();
        for (i, slot) in host.slots.iter_mut().enumerate() {
            if ev.seq <= slot.applied || (slot.behind && !replay) {
                continue;
            }
            if slot.fold.wants(&ev.kind) && catch_unwind(AssertUnwindSafe(|| slot.fold.apply(ev))).is_err() {
                broken.push(i);
                continue;
            }
            slot.applied = ev.seq;
        }
        for i in broken.into_iter().rev() {
            let slot = host.slots.remove(i);
            host.note(Problem::warn(format!("fold {}", slot.fold.id()), "panicked on an event and was dropped"));
        }
    }

    /// What fold `id` says at `path`, as of `now_ms`. Missing when there is no such fold.
    pub fn read(&self, id: &str, path: &[Key], now_ms: u64) -> Value {
        let host = self.host();
        host.slots.iter().find(|s| s.fold.id() == id).map(|s| s.fold.read(path, now_ms)).unwrap_or_default()
    }

    /// `fold.<id>…` for Expr: an environment whose `fold` root reads the folds as of `now_ms`.
    pub fn scope(&self, now_ms: u64) -> crate::eval::Scope {
        let host = self.host();
        let all: BTreeMap<String, Value> =
            host.slots.iter().map(|s| (s.fold.id().to_string(), s.fold.read(&[], now_ms))).collect();
        crate::eval::Scope::new().with("fold", Value::Map(all)).at_ms(now_ms)
    }

    /// The last event fold `id` has gone through.
    pub fn applied(&self, id: &str) -> Option<u64> {
        self.host().slots.iter().find(|s| s.fold.id() == id).map(|s| s.applied)
    }

    pub fn ids(&self) -> Vec<String> {
        self.host().slots.iter().map(|s| s.fold.id().to_string()).collect()
    }

    /// The state of fold `id`, as it would be written to a snapshot.
    pub fn state(&self, id: &str) -> Option<Json> {
        self.host().slots.iter().find(|s| s.fold.id() == id).map(|s| s.fold.save())
    }

    /// Write a snapshot of every fold now. The Log does this at its checkpoints, after it has synced; a caller that does it
    /// by hand should call [`Log::sync`] first, or the snapshot may be ahead of the disk.
    pub fn snapshot(&self) -> Vec<Problem> {
        let host = self.host();
        let mut problems = Vec::new();
        for slot in &host.slots {
            if let Err(e) = host.write_snapshot(slot) {
                problems.push(Problem::warn(format!("fold {}", slot.fold.id()), format!("snapshot not written: {e}")));
            }
        }
        problems
    }

    /// What went wrong on the writer thread (snapshots not written, a fold dropped) and what folds say about themselves.
    pub fn problems(&self) -> Vec<Problem> {
        let host = self.host();
        let mut out = host.problems.clone();
        for slot in &host.slots {
            out.extend(slot.fold.notes().into_iter().map(|n| Problem::warn(format!("fold {}", slot.fold.id()), n)));
        }
        out
    }
}

impl Sink for Folds {
    fn event(&mut self, ev: &Ev) {
        let mut host = self.host();
        Folds::feed(&mut host, ev, false);
    }

    fn checkpoint(&mut self, _seq: u64) {
        let mut host = self.host();
        let mut failed = Vec::new();
        for slot in &host.slots {
            if let Err(e) = host.write_snapshot(slot) {
                failed.push(Problem::warn(format!("fold {}", slot.fold.id()), format!("snapshot not written: {e}")));
            }
        }
        failed.into_iter().for_each(|p| host.note(p));
    }
}

/// A value as JSON that keeps money and durations apart from numbers: `{"$usd": micros}`, `{"$dur": ms}`, and a map as
/// `{"$map": {…}}` so no map can be taken for one of those.
pub fn to_tagged(v: &Value) -> Json {
    match v {
        Value::Missing => Json::Null,
        Value::Bool(b) => json!(b),
        Value::Num(n) => json!(n),
        Value::Str(s) => json!(s),
        Value::Dur(ms) => json!({ "$dur": ms }),
        Value::Usd(m) => json!({ "$usd": m }),
        Value::List(items) => Json::Array(items.iter().map(to_tagged).collect()),
        Value::Map(m) => json!({ "$map": m.iter().map(|(k, v)| (k.clone(), to_tagged(v))).collect::<serde_json::Map<_, _>>() }),
    }
}

pub fn from_tagged(j: &Json) -> Result<Value, String> {
    Ok(match j {
        Json::Null => Value::Missing,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => Value::num(n.as_f64().ok_or("a number out of range")?),
        Json::String(s) => Value::Str(s.clone()),
        Json::Array(items) => Value::List(items.iter().map(from_tagged).collect::<Result<_, _>>()?),
        Json::Object(o) if o.len() == 1 => match o.iter().next() {
            Some((k, v)) if k == "$dur" => Value::Dur(v.as_u64().ok_or("a duration out of range")?),
            Some((k, v)) if k == "$usd" => Value::Usd(v.as_i64().ok_or("money out of range")?),
            Some((k, Json::Object(m))) if k == "$map" => {
                Value::Map(m.iter().map(|(k, v)| from_tagged(v).map(|v| (k.clone(), v))).collect::<Result<_, _>>()?)
            }
            _ => return Err("an object that is not a tagged value".into()),
        },
        Json::Object(_) => return Err("an object that is not a tagged value".into()),
    })
}
