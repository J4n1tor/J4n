//! Every J4n finds what is wrong with J4n and keeps the fixes it makes. This is the local half:
//! findings are written to the Log with a fingerprint and a count, the person's word that something was wrong is kept as one, a fix of the
//! person's own to a pack row is carried and says so when the pack has it. **Nothing here reaches the network** — no sign-in, no report, no
//! push; those come later, and `contribute.mode` has no effect on what leaves because nothing does.

pub mod fingerprint;
pub mod found;
pub mod patch;

use found::{Finding, clip, judge, mark};
use j4n_core::change::Actor;
use j4n_core::fold::Folds;
use j4n_core::front::Contribute;
use j4n_core::log::{Ev, Log, New, Sub};
use j4n_core::proposal::{Fate, Proposals};
use j4n_core::schema::DataClass;
use j4n_core::table::Tables;
use j4n_core::value::{Key, Value};
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A finding of one fingerprint is written at most this often (its count goes on in memory and is written with the next).
const GAP_MS: u64 = 30_000;
/// How often the log is read for findings, and how often the proposals are looked at.
const TICK: Duration = Duration::from_millis(150);
const SWEEP_MS: u64 = 5_000;

pub struct Finder {
    log: Log,
    folds: Folds,
    /// The settings as they are now.
    settings: j4n_core::setting::Live,
    tables: Tables,
    proposals: Proposals,
    started_ms: u64,
    /// For each fingerprint: its count, the count last written, and when that was.
    tally: Mutex<BTreeMap<String, (u64, u64, u64)>>,
    swept: Mutex<u64>,
}

impl Finder {
    pub fn new(log: &Log, folds: &Folds, settings: impl Into<j4n_core::setting::Live>, tables: &Tables, proposals: &Proposals) -> Finder {
        Finder {
            log: log.clone(),
            folds: folds.clone(),
            settings: settings.into(),
            tables: tables.clone(),
            proposals: proposals.clone(),
            started_ms: log.clock().now_ms(),
            tally: Mutex::default(),
            swept: Mutex::new(0),
        }
    }

    /// `contribute.mode` is `ask` (the default) or `auto`; `off` finds nothing.
    pub fn on(&self) -> bool {
        matches!(self.settings.word("contribute.mode", "ask").trim().to_ascii_lowercase().as_str(), "ask" | "auto")
    }

    fn now(&self) -> u64 {
        self.log.clock().now_ms()
    }

    /// Write `data` as a private event of `kind`: what is found here is the person's, and stays so.
    fn put(&self, kind: &str, data: Json, cause: Option<u64>) {
        let mut n = New::of(kind, Actor::Core, data);
        (n.class, n.cause) = (Some(DataClass::Private), cause);
        let _ = self.log.append(n);
    }

    fn prior(&self, fp: &str) -> u64 {
        match self.folds.read("contribute_found", &[Key::Name(fp.into())], self.now()) {
            Value::Map(m) => match m.get("n") {
                Some(Value::Num(n)) => *n as u64,
                _ => 0,
            },
            _ => 0,
        }
    }

    /// Count a finding and, the first time and then at most every [`GAP_MS`], write it. The fingerprint, once written.
    pub fn record(&self, f: &Finding, words: Option<&str>) -> Option<String> {
        let fp = fingerprint::fingerprint(f.source, &f.id, &f.row, &f.text);
        let now = self.now();
        let (n, write) = {
            let mut t = self.tally.lock().unwrap_or_else(|e| e.into_inner());
            let known = t.get(&fp).copied();
            let n = known.map_or_else(|| self.prior(&fp), |(n, ..)| n) + 1;
            // the person's own words are always kept; a machine finding repeats at most every half minute
            let write = words.is_some() || known.is_none_or(|(_, _, at)| now.saturating_sub(at) >= GAP_MS);
            t.insert(fp.clone(), (n, if write { n } else { known.map_or(0, |(_, w, _)| w) }, if write { now } else { known.map_or(now, |(.., at)| at) }));
            (n, write)
        };
        if !write {
            return None;
        }
        let mut data = json!({ "key": fp, "fp": fp, "source": f.source, "what": f.id, "row": f.row, "title": f.title, "n": n, "mark": mark(f.source), "state": "kept here", "at_ms": now });
        if let Some(w) = words {
            data["words"] = json!(w);
        }
        self.put("contribute.found", data, Some(f.cause).filter(|c| *c > 0));
        Some(fp)
    }

    /// Write the count of a finding that went on inside the half minute and has had no hit since: the tab and a restart read it from the Log.
    fn flush(&self, force: bool) {
        let now = self.now();
        let due: Vec<(String, u64)> = {
            let mut t = self.tally.lock().unwrap_or_else(|e| e.into_inner());
            t.iter_mut().filter(|(_, (n, w, at))| n > w && (force || now.saturating_sub(*at) >= GAP_MS)).map(|(fp, e)| { *e = (e.0, e.0, now); (fp.clone(), e.0) }).collect()
        };
        for (fp, n) in due {
            let row = self.folds.read("contribute_found", &[Key::Name(fp.clone())], now);
            if let Value::Map(mut m) = row {
                m.insert("n".into(), Value::Num(n as f64));
                m.insert("at_ms".into(), Value::Num(now as f64));
                let data: serde_json::Map<String, Json> = m.into_iter().map(|(k, v)| (k, match v { Value::Num(x) => json!(x as u64), Value::Str(s) => json!(s), _ => Json::Null })).filter(|(_, v)| !v.is_null()).collect();
                self.put("contribute.found", Json::Object(data), None);
            }
        }
    }

    fn see(&self, ev: &Ev) {
        if self.on() && let Some(f) = judge(ev) {
            self.record(&f, None);
        }
    }

    /// Look at the accepted fixes: a new one is written as carried; one that was carried before this run started is compared with the rows
    /// that are loaded, and written once if the pack now says what it said, or the person's layer no longer holds it.
    pub fn sweep(&self) {
        let now = self.now();
        for p in self.proposals.list().into_iter().filter(|p| matches!(p.fate, Fate::Accepted(_) | Fate::Undone) && patch::is_patch(p)) {
            let key = p.draft.key.clone();
            let text = |m: &BTreeMap<String, Value>, k: &str| m.get(k).and_then(|s| if let Value::Str(s) = s { Some(s.clone()) } else { None });
            let (was, at, of) = match self.folds.read("contribute_patch", &[Key::Name(key.clone())], now) {
                Value::Map(m) => (text(&m, "state"), m.get("at_ms").and_then(|n| if let Value::Num(n) = n { Some(*n as u64) } else { None }).unwrap_or(0), text(&m, "proposal")),
                _ => (None, 0, None),
            };
            let undone = matches!(p.fate, Fate::Undone);
            // a key proposed again after an undo is a new fix, whatever was said of the old one
            let same = of.as_deref() == Some(p.id.as_str());
            let state = match was.as_deref() {
                None | Some(_) if !same && !undone => patch::State::Carried,
                Some("carried") if undone && same => patch::State::Withdrawn,
                Some("carried") if at < self.started_ms && !undone => patch::state(&p, &self.tables),
                _ => continue,
            };
            if !same && undone {
                continue;
            }
            if same && state == patch::State::Carried {
                continue;
            }
            let (word, mark) = match state {
                patch::State::Carried => ("carried", "⚑"),
                patch::State::Retired => ("retired", "✓"),
                patch::State::Withdrawn => ("withdrawn", "○"),
            };
            let how = if state == patch::State::Retired { "this version already says it" } else { "" };
            self.put("contribute.patch", json!({ "key": key, "proposal": p.id, "title": clip(&p.draft.title, 100), "state": word, "how": how, "mark": mark, "at_ms": now }), None);
        }
        *self.swept.lock().unwrap_or_else(|e| e.into_inner()) = now;
    }

    /// Read the log for findings on a thread of its own, and look at the fixes now and then, for as long as the finder is held.
    pub fn start(this: &Arc<Finder>) {
        let me = Arc::downgrade(this);
        let subs: Vec<Sub> = ["run.error", "task.stopped", "run.unread"].iter().map(|k| this.log.subscribe(k, 256)).collect();
        std::thread::spawn(move || {
            loop {
                let Some(f) = me.upgrade() else { return };
                for s in &subs {
                    while let Some(ev) = s.try_recv() {
                        f.see(&ev);
                    }
                }
                let (now, last) = (f.now(), *f.swept.lock().unwrap_or_else(|e| e.into_inner()));
                if f.on() && (last == 0 || now.saturating_sub(last) >= SWEEP_MS) {
                    f.sweep();
                }
                f.flush(false);
                drop(f);
                std::thread::sleep(TICK);
            }
        });
    }
}

impl Contribute for Finder {
    fn wrong(&self, _thread: &str, words: &str) -> Result<String, String> {
        if !self.on() {
            return Err("`contribute.mode` is off, so nothing is kept".into());
        }
        let words = words.trim();
        if words.is_empty() {
            return Err("`:wrong <what was wrong>`: a line of your own words; they stay on this machine".into());
        }
        let title = format!("“{}” (your words)", clip(words, 90));
        let f = Finding { source: "word", id: "wrong".into(), row: String::new(), text: words.into(), title, cause: 0 };
        let fp = self.record(&f, Some(&clip(words, 500))).unwrap_or_else(|| fingerprint::fingerprint(f.source, &f.id, &f.row, &f.text));
        Ok(format!("kept as a finding ({fp}); it and your words stay on this machine"))
    }
}

impl Drop for Finder {
    fn drop(&mut self) {
        self.flush(true);
    }
}
