//! The desk of the front (`j4n_core::front::shape::Desk`): the switch, the state of the draft in the composer, the card that shows
//! what the shaper added, and the rows the operator kept. It holds what the screen shows and nothing the Log does not also say:
//! the switch is `shape.switch` events, a result is `shape.result`, what was sent with a line is `shape.kept`.

use crate::check::{Dropped, Item};
use crate::render::{self, Template};
use j4n_core::change::Actor;
use j4n_core::front::shape::{Draft, Desk as FrontDesk, hash_of};
use j4n_core::log::{Log, New};
use j4n_proto::{Cell, Row, Tone};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// What the pack and the settings give the desk.
#[derive(Clone, Default)]
pub struct Cfg {
    /// The switch's value when the Log has never been told.
    pub on: bool,
    /// Lines (whole, case folded) that are never shaped, besides the short replies the front knows.
    pub skip: Vec<String>,
    pub templates: BTreeMap<String, Template>,
    /// `list`, `plain`, `sections`, `xml`, or `auto`.
    pub format: String,
    /// What `auto` reads: the format each doer likes (`[[shape_format]]`).
    pub formats: BTreeMap<String, String>,
}

/// What the shaper came to for one draft.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outcome {
    pub hash: String,
    /// `pass`, `add` or `ask`.
    pub verdict: String,
    pub items: Vec<Item>,
    pub dropped: Vec<Dropped>,
    pub rounds: u32,
    pub ms: u64,
    pub tokens: u64,
    pub usd: f64,
    /// Who: `shaper model / judge model`.
    pub by: String,
    /// What the judge said, in a line, when there was one.
    pub judge: String,
}

#[derive(Debug, Clone)]
enum Phase {
    Skipped(String),
    Working,
    Done(Arc<Outcome>),
    /// It did not run, and why.
    NotRun(String),
}

struct Slot {
    draft: Draft,
    phase: Phase,
    kept: BTreeSet<String>,
    answer: Option<usize>,
    dropped: bool,
}

pub struct Desk {
    cfg: Cfg,
    log: Log,
    on: AtomicBool,
    /// Failures of the shaper in a row; the breaker opens at three and `:shape on` closes it.
    pub fails: AtomicU32,
    slots: Mutex<BTreeMap<String, Slot>>,
    /// Results by draft hash, so a line typed again is not shaped again.
    cache: Mutex<BTreeMap<String, Result<Arc<Outcome>, String>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn cell(text: impl Into<String>, tone: Tone) -> Cell {
    Cell::new(text, tone)
}

impl Desk {
    pub fn new(cfg: Cfg, log: Log) -> Arc<Desk> {
        log.sync();
        let on = log.read(0).filter(|e| e.kind == "shape.switch").last().and_then(|e| e.data.get("on").and_then(|v| v.as_bool())).unwrap_or(cfg.on);
        Arc::new(Desk { cfg, log, on: AtomicBool::new(on), fails: AtomicU32::new(0), slots: Mutex::default(), cache: Mutex::default() })
    }

    fn changed(&self, thread: &str) {
        let _ = self.log.flash(New::of("shape.card", Actor::Core, json!({})).thread(thread));
    }

    /// Switch on for this process only, without a word in the Log (`j4n run --shape`).
    pub fn force_on(&self) {
        self.on.store(true, Ordering::SeqCst);
    }

    /// The draft the shaper is to work on (`hash`), or `None` when there is nothing to do: the draft is not the composer's now, or its
    /// result is known already.
    pub fn claim(&self, hash: &str) -> Option<Draft> {
        let mut slots = lock(&self.slots);
        let slot = slots.values_mut().find(|s| s.draft.hash == hash && matches!(s.phase, Phase::Working))?;
        Some(slot.draft.clone())
    }

    /// The shaper is done: the result goes to the slot that waits for it, if the composer still holds that draft.
    pub fn finish(&self, hash: &str, result: Result<Outcome, String>) {
        let result = result.map(Arc::new);
        // a failure is not remembered: the same line may be tried again
        if result.is_ok() {
            lock(&self.cache).insert(hash.to_string(), result.clone());
        }
        let mut thread = None;
        for (t, slot) in lock(&self.slots).iter_mut().filter(|(_, s)| s.draft.hash == hash) {
            fill(slot, &result);
            thread = Some(t.clone());
        }
        if let Some(t) = thread {
            self.changed(&t);
        }
    }

    /// The template for the format the target likes.
    fn template(&self, target: &str) -> Result<&Template, String> {
        let name = match self.cfg.format.as_str() {
            "" | "auto" => self.cfg.formats.get(target).map_or("list", String::as_str),
            other => other,
        };
        self.cfg.templates.get(name).ok_or_else(|| format!("there is no format called `{name}` (shape_template)"))
    }
}

fn fill(slot: &mut Slot, result: &Result<Arc<Outcome>, String>) {
    match result {
        Ok(o) => {
            slot.kept = o.items.iter().map(|i| i.key.clone()).collect();
            slot.phase = Phase::Done(o.clone());
        }
        Err(why) => slot.phase = Phase::NotRun(why.clone()),
    }
}

impl FrontDesk for Desk {
    fn on(&self) -> bool {
        self.on.load(Ordering::SeqCst)
    }

    fn drafted(&self, draft: &Draft, skip: Option<&str>) {
        let mut slot = Slot { draft: draft.clone(), phase: Phase::Working, kept: BTreeSet::new(), answer: None, dropped: false };
        match skip {
            Some(why) => slot.phase = Phase::Skipped(why.into()),
            None => {
                if let Some(known) = lock(&self.cache).get(&draft.hash) {
                    fill(&mut slot, known);
                }
            }
        }
        lock(&self.slots).insert(draft.thread.clone(), slot);
    }

    fn skips(&self, line: &str) -> bool {
        let line = crate::check::fold(line);
        self.cfg.skip.contains(&line)
    }

    fn command(&self, thread: &str, args: &[String]) -> Result<String, String> {
        let word = |n: usize| args.get(n).map(String::as_str).unwrap_or_default();
        match word(0) {
            "on" | "off" => {
                let on = word(0) == "on";
                self.log.append(New::of("shape.switch", Actor::Operator, json!({ "on": on }))).map_err(|p| p.msg)?;
                self.on.store(on, Ordering::SeqCst);
                self.fails.store(0, Ordering::SeqCst);
                lock(&self.slots).remove(thread);
                Ok(if on { "shape is on: lines the shaper may help with get a card of notes first (experimental)".into() } else { "shape is off".to_string() })
            }
            "" | "status" => Ok(format!("shape is {}", if self.on() { "on" } else { "off" })),
            "drop" => {
                let mut slots = lock(&self.slots);
                let slot = slots.get_mut(thread).ok_or("there are no notes to drop")?;
                slot.dropped = true;
                drop(slots);
                self.changed(thread);
                Ok("notes dropped: the next Enter sends the line as typed".into())
            }
            "toggle" => {
                let mut slots = lock(&self.slots);
                let slot = slots.get_mut(thread).ok_or("there are no notes")?;
                let Phase::Done(o) = &slot.phase else { return Err("there are no notes".into()) };
                let key = word(1);
                if !o.items.iter().any(|i| i.key == key) {
                    return Err(format!("`{key}` is not a row of the notes"));
                }
                if !slot.kept.remove(key) {
                    slot.kept.insert(key.to_string());
                }
                drop(slots);
                self.changed(thread);
                Ok(String::new())
            }
            "pick" => {
                let mut slots = lock(&self.slots);
                let slot = slots.get_mut(thread).ok_or("there is no question")?;
                let Phase::Done(o) = &slot.phase else { return Err("there is no question".into()) };
                let q = o.items.iter().find(|i| i.kind == "question").ok_or("there is no question")?;
                let n: usize = word(1).parse().ok().filter(|n| (1..=q.options.len()).contains(n)).ok_or_else(|| format!("pick 1 to {}", q.options.len()))?;
                slot.answer = Some(n - 1);
                slot.kept.insert("question".into());
                drop(slots);
                self.changed(thread);
                Ok(String::new())
            }
            other => Err(format!("`{other}` is not something shape does: on, off, drop")),
        }
    }

    fn keep(&self, thread: &str, line: &str, keys: &[String]) -> Result<String, String> {
        let (outcome, answer, target) = {
            let slots = lock(&self.slots);
            let slot = slots.get(thread).ok_or("there are no notes for this line")?;
            let Phase::Done(o) = &slot.phase else { return Err("there are no notes for this line".into()) };
            if slot.draft.hash != hash_of(&slot.draft.target, line) {
                return Err("those notes are for another draft".into());
            }
            if slot.dropped {
                return Err("the notes were dropped".into());
            }
            (o.clone(), slot.answer, slot.draft.target.clone())
        };
        if let Some(k) = keys.iter().find(|k| !outcome.items.iter().any(|i| &i.key == *k)) {
            return Err(format!("`{k}` is not a row of the latest notes for this line"));
        }
        let kept: Vec<&Item> = outcome.items.iter().filter(|i| keys.contains(&i.key)).collect();
        let t = self.template(&target)?;
        let mut parts = Vec::new();
        if let (Some(r), Some(rt)) = (kept.iter().find(|i| i.kind == "rewrite"), self.cfg.templates.get("rewrite")) {
            parts.push(render::fill(&rt.head, &[("line", line), ("rewrite", &r.text)]));
        }
        parts.extend(render::section(t, &kept, answer, line));
        let text = parts.join("\n");
        if text.trim().is_empty() {
            return Err("nothing was kept".into());
        }
        let _ = self.log.append(New::of("shape.kept", Actor::Operator, json!({ "result": outcome.hash, "keys": keys })).thread(thread));
        Ok(text)
    }

    fn card(&self, thread: &str) -> Vec<Row> {
        let slots = lock(&self.slots);
        let Some(slot) = slots.get(thread) else { return Vec::new() };
        let head = |state: &str, detail: String, tone: Tone| Row { key: "~draft".into(), cells: vec![cell(slot.draft.text.clone(), Tone::Plain), cell(state, tone), cell(detail, Tone::Dim)], ..Row::default() };
        let mut rows = Vec::new();
        match &slot.phase {
            Phase::Skipped(why) => rows.push(head("skip", why.clone(), Tone::Dim)),
            Phase::Working => rows.push(head("working", String::new(), Tone::Accent)),
            Phase::NotRun(why) => rows.push(head("failed", why.clone(), Tone::Warn)),
            Phase::Done(o) => {
                let state = if slot.dropped { "dropped" } else if o.items.is_empty() { "pass" } else { "ready" };
                rows.push(head(state, format!("{} rounds · {:.1}s · ${:.4}", o.rounds, o.ms as f64 / 1000.0, o.usd), Tone::Good));
                for i in &o.items {
                    let on = slot.kept.contains(&i.key) && (i.kind != "question" || slot.answer.is_some());
                    let tone = if on { Tone::Good } else if slot.kept.contains(&i.key) { Tone::Warn } else { Tone::Dim };
                    let mut row = Row { key: i.key.clone(), cells: vec![cell(i.kind, Tone::Dim), cell(i.text.clone(), Tone::Plain), cell(i.proof, Tone::Dim)], tone, ..Row::default() };
                    if i.kind == "question" {
                        row.detail = i.options.iter().enumerate().map(|(n, o)| cell(format!("{} {o}", n + 1), if slot.answer == Some(n) { Tone::Good } else { Tone::Plain })).collect();
                    }
                    rows.push(row);
                }
                for (n, d) in o.dropped.iter().enumerate() {
                    rows.push(Row { key: format!("x:{}", n + 1), cells: vec![cell("dropped", Tone::Warn), cell(d.text.clone(), Tone::Dim), cell(d.reason.clone(), Tone::Warn)], tone: Tone::Warn, ..Row::default() });
                }
                if !o.judge.is_empty() {
                    rows.push(Row { key: "~judge".into(), cells: vec![cell("judge", Tone::Dim), cell(o.judge.clone(), Tone::Dim)], ..Row::default() });
                }
            }
        }
        rows
    }
}
