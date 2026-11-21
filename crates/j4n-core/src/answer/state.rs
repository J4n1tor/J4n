//! The state of each answerer, as a fold of the Log: `Active`, `Shadow` or `Off`. `Off` is the
//! breaker: an answerer whose last [`BREAKER_AFTER`] steps all `failed` is skipped for a day and sends nothing meanwhile.
//!
//! The fold reads these events and nothing else (a fold is a function of the Log; the clock is never read, so a fold started
//! from a snapshot ends where one fed every event ends):
//!
//! | event | data | effect |
//! |---|---|---|
//! | `ask.step` | `answerer`, `outcome`, `breaker`? | `failed` adds one to the streak; the third opens the breaker until the event's time + 24 h; `answer` and `pass` end the streak; `skipped` is not read; a step with `breaker` = `false` (a point that keeps its failures to itself) is not read at all |
//! | `answerer.shadow` | `answerer`, `why` | the answerer is skipped on the live path |
//! | `answerer.active` | `answerer` | ends a shadow (it does not touch the breaker: only time closes that) |
//! | `answerer.off` | `answerer` | the notice of an opening has been written: [`Answerers::just_opened`] is empty again |
//! | `probe.result` | `answerer`, `point`, `model`, `cases_hash`, `passed` | a pass is kept for that point, that model and those cases; a failure drops it. The event also carries `failed` (the ids of the cases it got wrong) and `missed` (for each: `case`, `wanted`, `got` — the answer it gave, or `failed: …` / `no answer: …` and why — and `conf`, how sure it was), which the fold does not read: they say why a pass did not come. A pass may say `from` = `record` (seeded from the machine's record, `answerer/record.rs`) and `passed_at`, when the pass it records was given: the fold keeps that time, never a later one |
//! | `ask.answered` | `by`, `ask`, `conf` | a decider's or a model's confidence in the answer it gave, kept for the last [`CONFS_KEPT`] asks, so a label can say how sure the answer was |
//! | `label` | `subject_kind` = `answer`, `producer`, `point`, `weight` | the last [`LABELS_KEPT`] labels of the answerer for the point (within [`LABELS_DAYS`]); with [`Limits::min_n`] of them and a wrong rate over [`Limits::max_wrong`] the answerer is shadowed for that point |
//! | `answerer.active` | `answerer`, `point`? | with a `point`, ends a shadow by rate for it |
//!
//! After the day the answerer is `Active` again and gets one call; its streak was kept, so one more failure opens the
//! breaker again, and an answer or a pass starts the count from nothing. A failure that arrives while the breaker is open
//! (a call that began before it opened) neither extends it nor counts as a second opening. While it is open an answer
//! ends the streak but does not close it early.

use super::{State, StateView};
use crate::fold::Fold;
use crate::log::Ev;
use crate::value::{Key, Value};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

/// How many failures in a row open the breaker.
pub const BREAKER_AFTER: u32 = 3;
/// How long the breaker stays open (milliseconds).
pub const OFF_MS: u64 = 24 * 60 * 60 * 1000;
/// How many labels of an answerer for a point are counted, and how old the oldest may be.
pub const LABELS_KEPT: usize = 200;
/// How many answers' confidences an answerer keeps, to join to the labels that come later.
pub const CONFS_KEPT: usize = 2000;
/// The band above an answerer's `min_conf` whose answers are measured against the answers above it.
pub const BAND: f64 = 0.15;
/// How much a proposed `min_conf` moves, and how far.
pub const STEP: f64 = 0.05;
pub const CONF_MAX: f64 = 0.9;
pub const CONF_MIN: f64 = 0.3;
/// Labelled answers in the band before the numbers are worth a proposal: to raise `min_conf`, and to lower it.
pub const RAISE_N: usize = 40;
pub const LOWER_N: usize = 80;
pub const LABELS_DAYS: u64 = 30;

/// When an answerer's wrong answers shadow it: more than `max_wrong` of its labelled answers for a point, once there are
/// `min_n` of them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    pub max_wrong: f64,
    pub min_n: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { max_wrong: 0.25, min_n: 20 }
    }
}

/// One labelled answer: when, which ask, how wrong it counts, and how sure the answerer was (when it said).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Lab {
    at: u64,
    ask: String,
    w: f64,
    #[serde(default)]
    conf: Option<f64>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    #[serde(default)]
    failures: u32,
    #[serde(default)]
    shadow: Option<String>,
    /// The breaker is open while the time is before this; 0 is never opened (or closed by an answer after its day).
    #[serde(default)]
    off_until: u64,
    /// The breaker opened and its notice (`answerer.off`) is not in the Log yet.
    #[serde(default)]
    pending: bool,
    /// The last probe that passed, by point: (model, hash of the cases, when).
    #[serde(default)]
    passes: BTreeMap<String, (String, String, u64)>,
    /// The labels by point.
    #[serde(default)]
    labels: BTreeMap<String, VecDeque<Lab>>,
    /// How sure it was of its last answers: (ask, confidence).
    #[serde(default)]
    confs: VecDeque<(String, f64)>,
    /// Shadowed by its wrong rate, by point, and why.
    #[serde(default)]
    rate: BTreeMap<String, String>,
}

impl Entry {
    fn state(&self, now_ms: u64) -> State {
        if self.off_until > now_ms {
            State::Off { until_ms: self.off_until }
        } else if let Some(why) = &self.shadow {
            State::Shadow { why: why.clone() }
        } else {
            State::Active
        }
    }
}

/// A breaker that has opened and not yet been told to the operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Opened {
    pub until_ms: u64,
    pub failures: u32,
}

/// The fold `answerers`. A clone is a second handle on the same state: add one to the fold host and give the other to the
/// chain walk (it is a [`StateView`]).
#[derive(Clone, Default)]
pub struct Answerers {
    map: Arc<Mutex<BTreeMap<String, Entry>>>,
    /// The hash of the probe cases of each point that has any: an answerer with a model may act there only when it has passed
    /// them. Not part of the fold: it is what the loaded tables say now.
    cases: Arc<Mutex<BTreeMap<String, String>>>,
    /// The chain of each ask point as the loaded tables say it (the texts of its steps): what a `min_conf` proposal changes.
    chains: Arc<Mutex<BTreeMap<String, Vec<String>>>>,
    /// `trust.max_wrong` and `trust.min_n`, shared by the clones, so an edit tunes them all.
    limits: Arc<Mutex<Limits>>,
}

impl Answerers {
    pub fn new() -> Answerers {
        Answerers::default()
    }

    pub fn with_limits(self, limits: Limits) -> Answerers {
        *self.limits.lock().unwrap_or_else(|e| e.into_inner()) = limits;
        self
    }

    /// Say which points have probe cases, by the hash of the cases (`point → hash`). Replaces what was said before.
    pub fn require(&self, cases: BTreeMap<String, String>) {
        *self.cases.lock().unwrap_or_else(|e| e.into_inner()) = cases;
    }

    /// Say what the chain of each point is (`point → the text of each step`). Replaces what was said before.
    pub fn chains(&self, chains: BTreeMap<String, Vec<String>>) {
        *self.chains.lock().unwrap_or_else(|e| e.into_inner()) = chains;
    }

    /// How many answers of the answerer for the point are counted, and how wrong they add up to be.
    pub fn labelled(&self, answerer: &str, point: &str) -> (usize, f64) {
        self.lock().get(answerer).and_then(|e| e.labels.get(point)).map_or((0, 0.0), |q| (q.len(), q.iter().map(|l| l.w).sum()))
    }

    /// The pass last recorded for the answerer at the point: (model, hash of the cases, when).
    pub fn passed(&self, answerer: &str, point: &str) -> Option<(String, String, u64)> {
        self.lock().get(answerer).and_then(|e| e.passes.get(point).cloned())
    }

    fn lock(&self) -> MutexGuard<'_, BTreeMap<String, Entry>> {
        self.map.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// One row for each answerer and point it has a record at (and one with no point for an answerer with none): how far it is
    /// trusted there — its state, the model whose probes it passed, how many of its answers are labelled and how wrong they were.
    /// What the trust page shows.
    pub fn rows(&self, now_ms: u64) -> Vec<Value> {
        let map = self.lock();
        let mut out = Vec::new();
        for (id, e) in map.iter() {
            let mut points: Vec<&String> = e.passes.keys().chain(e.labels.keys()).chain(e.rate.keys()).collect();
            points.sort();
            points.dedup();
            let state = e.state(now_ms);
            let (word, tone) = match &state {
                State::Active => ("active", "good"),
                State::Shadow { .. } => ("shadow", "warn"),
                State::Off { .. } => ("off", "bad"),
            };
            let row = |point: &str| {
                let (n, wrong) = e.labels.get(point).map_or((0, 0.0), |q| (q.len(), q.iter().map(|l| l.w).sum::<f64>()));
                let why = e.rate.get(point).cloned().or(match &state {
                    State::Shadow { why } => Some(why.clone()),
                    _ => None,
                });
                let m = BTreeMap::from([
                    ("id".to_string(), Value::text(id)),
                    ("point".into(), Value::text(point)),
                    ("state".into(), Value::text(if e.rate.contains_key(point) { "shadow" } else { word })),
                    ("tone".into(), Value::text(if e.rate.contains_key(point) { "warn" } else { tone })),
                    ("model".into(), Value::text(e.passes.get(point).map(|p| p.0.as_str()).unwrap_or_default())),
                    ("n".into(), Value::num(n as f64)),
                    ("wrong".into(), Value::num(if n == 0 { 0.0 } else { wrong / n as f64 })),
                    ("why".into(), Value::text(why.unwrap_or_default())),
                    ("failures".into(), Value::num(f64::from(e.failures))),
                ]);
                Value::Map(m)
            };
            if points.is_empty() {
                out.push(row(""));
            }
            out.extend(points.iter().map(|p| row(p)));
        }
        out
    }

    /// The breaker of `answerer` opened and the loop has not yet written its `answerer.off` notice: write it once, with
    /// these numbers, and this is empty again.
    pub fn just_opened(&self, answerer: &str) -> Option<Opened> {
        self.lock().get(answerer).filter(|e| e.pending).map(|e| Opened { until_ms: e.off_until, failures: e.failures })
    }
}

impl StateView for Answerers {
    fn state(&self, answerer: &str, now_ms: u64) -> State {
        self.lock().get(answerer).map_or(State::Active, |e| e.state(now_ms))
    }

    fn state_for(&self, answerer: &str, point: &str, model: &str, now_ms: u64) -> State {
        let base = self.state(answerer, now_ms);
        if base != State::Active || model.is_empty() {
            return base;
        }
        let need = self.cases.lock().unwrap_or_else(|e| e.into_inner()).get(point).cloned();
        let map = self.lock();
        let e = map.get(answerer);
        if let Some(hash) = need {
            match e.and_then(|e| e.passes.get(point)) {
                Some((m, h, _)) if m == model && *h == hash => {}
                Some((m, _, _)) if m != model => return State::Shadow { why: format!("its probes for {point} passed for {m}, not for {model}") },
                Some(_) => return State::Shadow { why: format!("the probe cases for {point} changed since it passed them") },
                None => return State::Shadow { why: format!("its probes for {point} have not passed") },
            }
        }
        match e.and_then(|e| e.rate.get(point)) {
            Some(why) => State::Shadow { why: why.clone() },
            None => State::Active,
        }
    }
}

/// How the answers in the band above `min_conf` fared against those above it, and the chain with `min_conf` moved when the numbers
/// say so. Per point whose chain gives this answerer a `min_conf`.
fn tune(id: &str, e: &Entry, chains: &BTreeMap<String, Vec<String>>) -> Value {
    let mut out = BTreeMap::new();
    for (point, steps) in chains {
        let found = steps.iter().enumerate().find_map(|(i, t)| super::point::parse_step(t).ok().filter(|s| s.id() == id && s.min_conf.is_some()).map(|s| (i, s)));
        let Some((at, step)) = found else { continue };
        // as it is written (`0.6`), not as a 32-bit float holds it (0.6000000238…): an answer that is exactly as sure as the line is in the band
        let min = step.min_conf.map_or(0.0, |c| c.to_string().parse().unwrap_or(f64::from(c)));
        let (mut band, mut above) = ((0usize, 0.0f64), (0usize, 0.0f64));
        for l in e.labels.get(point).into_iter().flatten() {
            match l.conf {
                Some(c) if c >= min && c < min + BAND => (band.0, band.1) = (band.0 + 1, band.1 + l.w),
                Some(c) if c >= min + BAND => (above.0, above.1) = (above.0 + 1, above.1 + l.w),
                _ => {}
            }
        }
        let rate = |(n, w): (usize, f64)| if n == 0 { 0.0 } else { w / n as f64 };
        let moved = |to: f64| -> Value {
            let mut s = step.clone();
            s.min_conf = Some(((to * 100.0).round() / 100.0) as f32);
            Value::List(steps.iter().enumerate().map(|(i, t)| Value::text(if i == at { s.to_string() } else { t.clone() })).collect())
        };
        let mut m = BTreeMap::from([
            ("min_conf".to_string(), Value::num(min)),
            ("n_band".into(), Value::num(band.0 as f64)),
            ("wrong_band".into(), Value::num(rate(band))),
            ("n_above".into(), Value::num(above.0 as f64)),
            ("wrong_above".into(), Value::num(rate(above))),
        ]);
        if band.0 >= RAISE_N && above.0 >= RAISE_N / 4 && rate(band) > 0.0 && rate(band) >= 2.0 * rate(above) && min + STEP <= CONF_MAX + 1e-9 {
            m.insert("raise".into(), moved(min + STEP));
        }
        if band.0 >= LOWER_N && band.1 == 0.0 && min - STEP >= CONF_MIN - 1e-9 {
            m.insert("lower".into(), moved(min - STEP));
        }
        out.insert(point.clone(), Value::Map(m));
    }
    Value::Map(out)
}

fn entry_value(id: &str, e: &Entry, chains: &BTreeMap<String, Vec<String>>, now_ms: u64) -> Value {
    let mut m = BTreeMap::from([("failures".to_string(), Value::num(f64::from(e.failures)))]);
    m.insert("tune".into(), tune(id, e, chains));
    m.insert("rate".into(), Value::Map(e.rate.iter().map(|(p, w)| (p.clone(), Value::text(w))).collect()));
    m.insert("passed".into(), Value::Map(e.passes.iter().map(|(p, (model, _, _))| (p.clone(), Value::text(model))).collect()));
    let word = match e.state(now_ms) {
        State::Active => "active",
        State::Shadow { why } => {
            m.insert("why".into(), Value::text(why));
            "shadow"
        }
        State::Off { until_ms } => {
            m.insert("until_ms".into(), Value::num(until_ms as f64));
            "off"
        }
    };
    m.insert("state".into(), Value::text(word));
    Value::Map(m)
}

impl Fold for Answerers {
    fn id(&self) -> &str {
        "answerers"
    }

    fn version(&self) -> u64 {
        3
    }

    fn wants(&self, kind: &str) -> bool {
        matches!(kind, "ask.step" | "answerer.shadow" | "answerer.active" | "answerer.off" | "probe.result" | "label" | "ask.answered")
    }

    fn apply(&mut self, ev: &Ev) {
        let key = match ev.kind.as_str() {
            "label" => "producer",
            "ask.answered" => "by",
            _ => "answerer",
        };
        let Some(id) = ev.data.get(key).and_then(Json::as_str).filter(|s| !s.is_empty()) else { return };
        let limits = *self.limits.lock().unwrap_or_else(|e| e.into_inner());
        let mut map = self.lock();
        let e = map.entry(id.to_string()).or_default();
        match ev.kind.as_str() {
            "ask.step" if ev.data.get("breaker").and_then(Json::as_bool) == Some(false) => {}
            "ask.step" => match ev.data.get("outcome").and_then(Json::as_str) {
                Some("failed") => {
                    e.failures = e.failures.saturating_add(1);
                    if e.failures >= BREAKER_AFTER && e.off_until <= ev.at_ms {
                        e.off_until = ev.at_ms.saturating_add(OFF_MS);
                        e.pending = true;
                    }
                }
                Some("answer" | "pass") => {
                    e.failures = 0;
                    if e.off_until <= ev.at_ms {
                        e.off_until = 0;
                    }
                }
                _ => {}
            },
            "answerer.shadow" => e.shadow = Some(ev.data.get("why").and_then(Json::as_str).unwrap_or_default().to_string()),
            "answerer.active" => match ev.data.get("point").and_then(Json::as_str) {
                Some(point) => {
                    e.rate.remove(point);
                }
                None => {
                    e.shadow = None;
                    e.rate.clear();
                }
            },
            "probe.result" => {
                let s = |k: &str| ev.data.get(k).and_then(Json::as_str).unwrap_or_default().to_string();
                let point = s("point");
                if ev.data.get("passed").and_then(Json::as_bool) == Some(true) {
                    // a pass seeded from the machine's record is as old as the pass it records, not as new as the seeding
                    let at = ev.data.get("passed_at").and_then(Json::as_u64).map_or(ev.at_ms, |t| t.min(ev.at_ms));
                    e.passes.insert(point, (s("model"), s("cases_hash"), at));
                } else {
                    e.passes.remove(&point);
                }
            }
            "label" if ev.data.get("subject_kind").and_then(Json::as_str) == Some("answer") => {
                let point = ev.data.get("point").and_then(Json::as_str).unwrap_or_default().to_string();
                let weight = ev.data.get("weight").and_then(Json::as_f64).unwrap_or(0.0).clamp(0.0, 1.0);
                let subject = ev.data.get("subject").and_then(Json::as_str).unwrap_or_default().to_string();
                let conf = e.confs.iter().rev().find(|(a, _)| !subject.is_empty() && *a == subject).map(|(_, c)| *c);
                let q = e.labels.entry(point.clone()).or_default();
                // an answer is counted once; later evidence replaces what was said of it only by being worse
                match q.iter_mut().find(|l| !subject.is_empty() && l.ask == subject) {
                    Some(old) => old.w = old.w.max(weight),
                    None => q.push_back(Lab { at: ev.at_ms, ask: subject, w: weight, conf }),
                }
                let oldest = ev.at_ms.saturating_sub(LABELS_DAYS * 24 * 3600 * 1000);
                while q.len() > LABELS_KEPT || q.front().is_some_and(|l| l.at < oldest) {
                    q.pop_front();
                }
                let (n, wrong) = (q.len(), q.iter().map(|l| l.w).sum::<f64>());
                if n >= limits.min_n && wrong / n as f64 > limits.max_wrong && !e.rate.contains_key(&point) {
                    e.rate.insert(point.clone(), format!("{:.0}% of its {n} labelled answers for {point} were wrong", wrong / n as f64 * 100.0));
                }
            }
            "ask.answered" => {
                let machine = matches!(ev.data.get("kind").and_then(Json::as_str), Some("decider" | "model"));
                let ask = ev.data.get("ask").and_then(Json::as_str).unwrap_or_default();
                if let (true, false, Some(conf)) = (machine, ask.is_empty(), ev.data.get("conf").and_then(Json::as_f64).filter(|c| c.is_finite())) {
                    e.confs.push_back((ask.to_string(), conf));
                    while e.confs.len() > CONFS_KEPT {
                        e.confs.pop_front();
                    }
                }
            }
            "answerer.off" => e.pending = false,
            _ => {}
        }
        if *e == Entry::default() {
            map.remove(id);
        }
    }

    fn read(&self, path: &[Key], now_ms: u64) -> Value {
        let map = self.lock();
        let chains = self.chains.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(Key::Name(id)) = path.first() {
            return entry_value(id, map.get(id).unwrap_or(&Entry::default()), &chains, now_ms).at(&path[1..]);
        }
        Value::Map(map.iter().map(|(id, e)| (id.clone(), entry_value(id, e, &chains, now_ms))).collect()).at(path)
    }

    fn save(&self) -> Json {
        serde_json::to_value(&*self.lock()).unwrap_or_default()
    }

    fn load(&mut self, state: Json) -> Result<(), String> {
        *self.lock() = serde_json::from_value(state).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn reset(&mut self) {
        self.lock().clear();
    }
}

impl crate::setting::Tune for Answerers {
    fn tune(&self, settings: &crate::setting::Settings) {
        *self.limits.lock().unwrap_or_else(|e| e.into_inner()) = Limits { max_wrong: settings.num("trust.max_wrong", 0.25), min_n: settings.num("trust.min_n", 20.0) as usize };
    }
}
