//! Probes: cases a machine answerer must get right before it acts for an ask point. The kind is
//! `probe` (`packs/base/kinds/probe.toml`); a case has a twin whose right answer is another, so an answerer that always says one
//! thing fails one of the two.
//!
//! [`ProbeSet`] is the cases that loaded and made sense: a case whose point is not there, whose point cannot be probed (a text,
//! an approval), whose right answer is not one that is offered, or whose twin is missing, is the same shape, does not name it
//! back or has the same right answer, is a problem and no case — and so is its twin. The hash of a point's cases says which
//! cases a pass was for ([`ProbeSet::hashes`] is what the state fold is told).
//!
//! [`Prober`] runs them, off the path of any line: for each machine answerer that is in the chain of a point with cases it checks
//! where the question goes (the egress Gate, as the walk does), puts every case, and writes one `probe.result` for the answerer,
//! the point and the model behind it. A pass is for that model and those cases only; the fold turns it into `Active`, and
//! the lack of it into `Shadow`. With a [`Record`] (the machine's, in J4n's folder) a pass the Log has not seen is seeded from it when
//! the same model passed the same cases less than a week ago; no case is put then.

use super::chain::Registry;
use super::record::Record;
use super::state::Answerers;
use super::{Answerer, AskPoint, Offer, Question, Says, ShapeKind, described};
use crate::ask::{Answer, AskShape, Opt};
use crate::change::Actor;
use crate::log::{Log, New};
use crate::table::{Kind, Problem, Tables};
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// How old a pass may be before the start-up check asks again.
pub const STALE_MS: u64 = 7 * 24 * 3600 * 1000;

/// An entry of kind `probe`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeRow {
    pub id: String,
    pub doc: Option<String>,
    pub point: String,
    #[serde(default)]
    pub slices: BTreeMap<String, String>,
    #[serde(default)]
    pub offered: Vec<String>,
    pub expect: String,
    pub pair: String,
}

impl Kind for ProbeRow {
    const NAME: &'static str = "probe";
}

impl ProbeRow {
    /// The question the case puts: `line` (or `question`) is its text, `goal` what is being done, any other slice an added line.
    pub fn question(&self) -> Question {
        let text = self.slices.get("line").or_else(|| self.slices.get("question")).cloned().unwrap_or_default();
        let mut text = text;
        for (k, v) in self.slices.iter().filter(|(k, _)| !["line", "question", "goal"].contains(&k.as_str())) {
            text.push_str(&format!("\n{k}: {v}"));
        }
        let shape = if self.offered.is_empty() {
            AskShape::YesNo
        } else {
            AskShape::Choice { options: self.offered.iter().map(|id| Opt { id: id.clone(), label: id.clone(), recommended: false }).collect() }
        };
        let goal = self.slices.get("goal").cloned().unwrap_or_else(|| text.clone());
        Question { ask: format!("probe:{}", self.id), point: self.point.clone(), asker: "probe".into(), thread: None, text, shape, goal, act: None }
    }

    /// The answer the case wants.
    pub fn wanted(&self) -> Answer {
        match (self.offered.is_empty(), self.expect.as_str()) {
            (true, "yes") => Answer::Yes,
            (true, _) => Answer::No,
            (false, id) => Answer::Pick { id: id.into() },
        }
    }
}

/// The cases that made sense, by point.
#[derive(Debug, Clone, Default)]
pub struct ProbeSet {
    by_point: BTreeMap<String, Vec<ProbeRow>>,
}

impl ProbeSet {
    /// The rows of the Table that hold together (see the module doc). `points` are the ask points that loaded.
    pub fn load(tables: &Tables, points: &BTreeMap<String, AskPoint>) -> (ProbeSet, Vec<Problem>) {
        let (mut rows, mut problems) = (Vec::new(), Vec::new());
        for id in tables.ids(ProbeRow::NAME) {
            match tables.get::<ProbeRow>(id) {
                Ok(r) => rows.push(r),
                Err(p) => problems.push(p),
            }
        }
        let by_id: BTreeMap<&str, &ProbeRow> = rows.iter().map(|r| (r.id.as_str(), r)).collect();
        let mut bad: BTreeMap<String, String> = BTreeMap::new();
        for r in &rows {
            let fault = match points.get(&r.point) {
                None => Some(format!("there is no ask point `{}`", r.point)),
                Some(p) if !matches!(p.shape, ShapeKind::YesNo | ShapeKind::Choice) => Some(format!("`{}` takes a {}, which is not probed", r.point, p.shape.word())),
                Some(p) if p.shape == ShapeKind::YesNo && (!r.offered.is_empty() || !["yes", "no"].contains(&r.expect.as_str())) => Some("a yes/no point offers nothing, and the right answer is yes or no".to_string()),
                Some(p) if p.shape == ShapeKind::Choice && !r.offered.contains(&r.expect) => Some(format!("the right answer `{}` is not among what is offered", r.expect)),
                Some(_) => match by_id.get(r.pair.as_str()) {
                    None => Some(format!("its twin `{}` is not there", r.pair)),
                    Some(t) if t.point != r.point || t.offered != r.offered => Some(format!("its twin `{}` is not a case of the same question", r.pair)),
                    Some(t) if t.pair != r.id => Some(format!("its twin `{}` does not name it back", r.pair)),
                    Some(t) if t.expect == r.expect => Some(format!("its twin `{}` has the same right answer, so the pair tells nothing", r.pair)),
                    Some(_) => None,
                },
            };
            if let Some(f) = fault {
                bad.insert(r.id.clone(), f);
            }
        }
        // a case without its twin is no case: the fault of one is the fault of both
        for r in &rows {
            if let Some(t) = bad.get(&r.pair).cloned().filter(|_| !bad.contains_key(&r.id)) {
                bad.insert(r.id.clone(), format!("its twin `{}` is refused ({t})", r.pair));
            }
        }
        for (id, why) in &bad {
            problems.push(Problem::refused(format!("probe {id}"), why.clone()));
        }
        let mut by_point: BTreeMap<String, Vec<ProbeRow>> = BTreeMap::new();
        for r in rows.into_iter().filter(|r| !bad.contains_key(&r.id)) {
            by_point.entry(r.point.clone()).or_default().push(r);
        }
        by_point.values_mut().for_each(|v| v.sort_by(|a, b| a.id.cmp(&b.id)));
        (ProbeSet { by_point }, problems)
    }

    pub fn cases(&self, point: &str) -> &[ProbeRow] {
        self.by_point.get(point).map_or(&[], Vec::as_slice)
    }

    /// The hash of the cases of each point that has any: what a pass is a pass for.
    pub fn hashes(&self) -> BTreeMap<String, String> {
        self.by_point
            .iter()
            .map(|(p, rows)| {
                let text = serde_json::to_string(&rows.iter().map(|r| json!([r.id, r.slices, r.offered, r.expect, r.pair])).collect::<Vec<_>>()).unwrap_or_default();
                (p.clone(), crate::text::short_hash(text))
            })
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.by_point.is_empty()
    }

    /// The set without the cases of `point`: a point whose switch is off is not asked, so nothing is proved for it and no call is spent on it.
    pub fn without(mut self, point: &str) -> ProbeSet {
        self.by_point.remove(point);
        self
    }
}

/// What one answerer did with the cases of one point.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub answerer: String,
    pub point: String,
    pub model: String,
    pub hash: String,
    pub passed: bool,
    /// The cases it got wrong.
    pub failed: Vec<String>,
    /// What it said to each of them, and how sure it was: why a pass did not come.
    pub missed: Vec<Miss>,
    pub ms: u64,
    /// What the cases cost, when the answerer said it for each of them; none when one call did not say.
    pub usd: Option<f64>,
    /// Why it was not put to them at all (the question may not go where it goes).
    pub why: Option<String>,
}

/// A case an answerer got wrong: what the case wanted, and what it gave instead (an answer, or why it gave none).
#[derive(Debug, Clone, PartialEq)]
pub struct Miss {
    pub case: String,
    pub wanted: String,
    pub got: String,
    pub conf: Option<f32>,
}

impl Miss {
    /// The words of the event and of the `:probe` notice: `intake-flow-plan: wanted flow, got task (0.81)`.
    pub fn line(&self) -> String {
        let conf = self.conf.map(|c| format!(" ({c:.2})")).unwrap_or_default();
        format!("{}: wanted {}, got {}{conf}", self.case, self.wanted, self.got)
    }
}

/// Runs the cases; off the path of any line, and blocking: the caller gives it a thread of its own.
pub struct Prober {
    log: Log,
    registry: Registry,
    set: ProbeSet,
    points: BTreeMap<String, AskPoint>,
    state: Answerers,
    says: Says,
    record: Option<Record>,
    /// The pairs being run now, and the pairs run since this process began (a failed run is not run again by every line that asks).
    running: Mutex<Running>,
    done: Condvar,
}

#[derive(Default)]
struct Running {
    now: BTreeSet<(String, String)>,
    tried: BTreeSet<(String, String)>,
}

impl Prober {
    pub fn new(log: Log, registry: Registry, set: ProbeSet, points: BTreeMap<String, AskPoint>, state: Answerers) -> Prober {
        Prober { log, registry, set, points, state, says: Says::new(), record: None, running: Mutex::default(), done: Condvar::new() }
    }

    /// The same, putting each case with the words its point says its options mean, as the live question is put.
    pub fn with_says(mut self, says: Says) -> Prober {
        self.says = says;
        self
    }

    /// The same, keeping passes in the machine's record and seeding the Log from it.
    pub fn with_record(mut self, record: Record) -> Prober {
        self.record = Some(record);
        self
    }

    fn running(&self) -> MutexGuard<'_, Running> {
        self.running.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set(&self) -> &ProbeSet {
        &self.set
    }

    /// The (answerer, point) pairs there is something to prove for: an installed machine answerer with a model, named in the chain
    /// of a point that has cases. `only` limits it to one answerer.
    pub fn targets(&self, only: Option<&str>) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for (point, p) in self.points.iter().filter(|(id, _)| !self.set.cases(id).is_empty()) {
            for step in p.chain.iter().filter(|s| s.kind.is_model()) {
                let id = step.id();
                let installed = self.registry.get(&id).is_some_and(|a| a.kind() == step.kind && !a.model().is_empty());
                if installed && only.is_none_or(|o| o == id) && !out.contains(&(id.clone(), point.clone())) {
                    out.push((id, point.clone()));
                }
            }
        }
        out
    }

    /// What has not been proved, or was proved a week ago or more, or for another model or other cases.
    pub fn due(&self, now_ms: u64) -> Vec<(String, String)> {
        let hashes = self.set.hashes();
        self.targets(None)
            .into_iter()
            .filter(|(a, p)| {
                let model = self.registry.get(a).map(|a| a.model()).unwrap_or_default();
                let hash = hashes.get(p);
                let proved = |at: u64| now_ms.saturating_sub(at) < STALE_MS;
                match self.state.passed(a, p) {
                    Some((m, h, at)) if m == model && Some(&h) == hash && proved(at) => false,
                    _ => !self.seed(a, p, &model, hash.map_or("", String::as_str), now_ms),
                }
            })
            .collect()
    }

    /// The Log has no fresh pass for this pair: take the machine's record of one for this model and these cases into it, as a
    /// `probe.result` that says `from: record` and when the pass was. True when it did.
    fn seed(&self, answerer: &str, point: &str, model: &str, hash: &str, now_ms: u64) -> bool {
        let Some(at) = self.record.as_ref().and_then(|r| r.passed(answerer, point, model, hash, now_ms, STALE_MS)) else { return false };
        let data = json!({ "point": point, "answerer": answerer, "model": model, "cases_hash": hash, "passed": true, "failed": [], "missed": [], "ms": 0, "why": null, "from": "record", "passed_at": at });
        self.log.append(New::of("probe.result", Actor::Core, data)).is_ok()
    }

    /// Put the cases to each answerer for each point, and write what came of it. A pass needs every case right. A pair that another
    /// run has in hand is left to it.
    pub fn run(&self, targets: &[(String, String)]) -> Vec<Report> {
        let mine = self.claim(targets, false);
        self.run_claimed(&mine)
    }

    /// What is due, one pair at a time, each taken only when it is reached and nobody has it or has tried it: the run at start, which a
    /// line that waits for its own point may get ahead of.
    pub fn run_due(&self, now_ms: u64) -> Vec<Report> {
        let mut out = Vec::new();
        for t in self.due(now_ms) {
            let mine = self.claim(std::slice::from_ref(&t), true);
            out.extend(self.run_claimed(&mine));
        }
        out
    }

    /// Take the pairs nobody is running (and, with `fresh`, that this process has not tried).
    fn claim(&self, targets: &[(String, String)], fresh: bool) -> Vec<(String, String)> {
        let mut running = self.running();
        targets.iter().filter(|t| !(fresh && running.tried.contains(*t)) && running.now.insert((*t).clone())).cloned().collect()
    }

    fn run_claimed(&self, targets: &[(String, String)]) -> Vec<Report> {
        let hashes = self.set.hashes();
        let mut reports = Vec::new();
        for (id, point) in targets {
            let report = self.one(id, point, &hashes);
            if let Some(r) = &report {
                let missed: Vec<_> = r.missed.iter().map(|m| json!({ "case": m.case, "wanted": m.wanted, "got": m.got, "conf": m.conf })).collect();
                let data = json!({ "point": r.point, "answerer": r.answerer, "model": r.model, "cases_hash": r.hash, "passed": r.passed, "failed": r.failed, "missed": missed, "ms": r.ms, "usd": r.usd, "why": r.why });
                let _ = self.log.append(New::of("probe.result", Actor::Core, data));
                // a pass is kept for the next project, a failure takes it away; a question that was not put proved nothing
                let now = self.log.clock().now_ms();
                match (&self.record, r.why.is_none()) {
                    (Some(rec), true) if r.passed => rec.keep(&r.answerer, &r.point, &r.model, &r.hash, now, STALE_MS),
                    (Some(rec), true) => rec.drop(&r.answerer, &r.point, &r.model),
                    _ => {}
                }
            }
            // released after the result is in the Log, so whoever waits for the pair finds the pass
            let mut running = self.running();
            running.now.remove(&(id.clone(), point.clone()));
            running.tried.insert((id.clone(), point.clone()));
            drop(running);
            self.done.notify_all();
            reports.extend(report);
        }
        self.log.sync();
        reports
    }

    fn one(&self, id: &str, point: &str, hashes: &BTreeMap<String, String>) -> Option<Report> {
        let (a, p) = (self.registry.get(id)?, self.points.get(point)?);
        let hash = hashes.get(point).cloned().unwrap_or_default();
        let mut r = Report { answerer: id.into(), point: point.into(), model: a.model(), hash, passed: false, failed: Vec::new(), missed: Vec::new(), ms: 0, usd: Some(0.0), why: None };
        let started = Instant::now();
        match a.zone().map_or(Ok(()), |z| self.registry.allows(p.data, &z)) {
            Err(why) => r.why = Some(why),
            Ok(()) => {
                for c in self.set.cases(point) {
                    let (missed, paid) = miss(&*a, c, &self.says, self.log.redactor());
                    r.usd = r.usd.zip(paid.and_then(|p| p.usd)).map(|(a, b)| a + b);
                    r.missed.extend(missed);
                }
                r.failed = r.missed.iter().map(|m| m.case.clone()).collect();
                r.passed = r.missed.is_empty();
            }
        }
        r.ms = started.elapsed().as_millis() as u64;
        Some(r)
    }

    /// A line is about to be asked at `point`, and nobody is there to wait for an answer that is not coming: put the cases of the
    /// answerers in its chain that have not been proved to them now, on a thread of their own, and wait until they are written or `max`
    /// is gone. A run already begun (the one at start) is waited for, not begun again; a pair already tried in this process is not
    /// tried again, so a failed run costs the first line only. Returns how long it waited, none when there was nothing to wait for.
    pub fn wait_for(self: &Arc<Prober>, point: &str, max: Duration, now_ms: u64) -> Option<Duration> {
        let pending: Vec<_> = {
            let tried = self.running().tried.clone();
            self.due(now_ms).into_iter().filter(|(a, p)| p == point && !tried.contains(&(a.clone(), p.clone()))).collect()
        };
        if pending.is_empty() {
            return None;
        }
        let began = Instant::now();
        let mine = self.claim(&pending, true);
        if !mine.is_empty() {
            let prober = self.clone();
            std::thread::spawn(move || drop(prober.run_claimed(&mine)));
        }
        let mut running = self.running();
        while let Some(left) = max.checked_sub(began.elapsed()).filter(|_| pending.iter().any(|t| running.now.contains(t))) {
            running = self.done.wait_timeout(running, left).unwrap_or_else(|e| e.into_inner()).0;
        }
        Some(began.elapsed())
    }
}

/// The case an answerer got wrong, with what it said; none when it gave the answer the case wants. Why it failed is scrubbed before it is
/// cut short, so a secret it says back (a gateway that echoes the key) is never cut into a piece the redactor no longer knows.
/// The case put to the answerer: what it missed, none when it was right, and what the call cost when it said.
fn miss(a: &dyn Answerer, case: &ProbeRow, says: &Says, red: &crate::redact::Redactor) -> (Option<Miss>, Option<super::Paid>) {
    let mut q = case.question();
    q.shape = described(q.shape, says, &case.point);
    let (offer, paid) = a.offer_paid(&q);
    let (got, conf) = match offer {
        Offer::Answer { answer, .. } if answer == case.wanted() && q.shape.check(&answer).is_ok() => return (None, paid),
        Offer::Answer { answer, conf, .. } => (if q.shape.check(&answer).is_ok() { answer.render() } else { format!("{} (not an option)", answer.render()) }, Some(conf)),
        Offer::Pass { why } => (format!("no answer: {}", one_line(&red.scrub(&why))), None),
        Offer::Failed { why } => (format!("failed: {}", one_line(&red.scrub(&why))), None),
    };
    (Some(Miss { case: case.id.clone(), wanted: case.expect.clone(), got, conf }), paid)
}

/// One line of at most 120 characters: what an answerer says about itself goes into events as this.
fn one_line(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).take(120).collect()
}
