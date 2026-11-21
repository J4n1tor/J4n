//! Who does a thing: the election of a doer, a model and a host. The front elects once
//! for a thread (`who:work`), and a flow elects for each of its steps (`who:plan`, `who:validate` …); both walk the ask point of
//! the role the same way — rule, rank over what the Log says each doer has done, a person who is out of time after a minute — and
//! the host of the model is then picked by what it would cost and how long it would take (`front::hosts`).
//!
//! A doer is offered for a role when it is installed and logged in and says it is good for the role (a connector with no `roles`
//! is good for any); the doers a step excludes (the validator is not the one who worked) are not offered.

use super::hosts::{Hosts, Want};
use super::models::{ModelLists, NoLists, Resolved, settle};
use super::thread::AnswerRef;
use crate::answer::chain::{Ledger, Note, Spent, Walk, walk};
use crate::answer::{Kind, Question, described};
use crate::ask::{Answer, AskShape, Opt};
use crate::change::Actor;
use crate::doer::{Doer, Probe};
use crate::flow::run::{Elect, Elected};
use crate::log::{Log, New};
use crate::task::{Asks, Mode};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// How long after the operator's last line a lane still has someone waiting at it.
const WAITING_MS: u64 = 5 * 60_000;

/// How long what a doer's probe found is taken as standing: a line asks who is ready several times, and a program that hangs at its version probe
/// costs the probe's whole limit each time it is asked. A doer that is signed in or installed meanwhile is seen on the first sweep after this.
pub const PROBE_FRESH_MS: u64 = 60_000;

/// What [`Who::ask`] walked.
pub struct Asked {
    pub walk: Walk,
    /// What each step did (`ask.step`, `ask.note`), as written to the Log.
    pub notes: Vec<Note>,
    /// How long the walk took, and how long before it the probe cases were waited for (milliseconds).
    pub ms: u64,
    pub waited: u64,
    /// What the answerers of the walk cost, from its steps: `usd` is `None` when a decider or a model did not say.
    pub spent: Spent,
}

pub struct Who {
    asks: Arc<Asks>,
    doers: Vec<Arc<dyn Doer>>,
    /// What each doer is good for; a doer not here is good for anything.
    roles: BTreeMap<String, Vec<String>>,
    /// What each doer says it is, as a decider is told it when it chooses.
    docs: BTreeMap<String, String>,
    hosts: Arc<dyn Hosts>,
    log: Log,
    /// Numbers the questions it walks.
    walks: AtomicU64,
    /// When the operator last said something (milliseconds).
    said: AtomicU64,
    /// What the launch pinned (`--doer`, `--model`): a flow step that elects (`who:<role>`) is bound by it.
    pin: Pin,
    /// The models each doer serves, to say whether the pinned model is one of an elected doer's.
    lists: Arc<dyn ModelLists>,
    /// The doers whose own usage limit is read (a connector that says where), and what the Log says of every limit: one of them that is shut now is
    /// not elected while another can take the role.
    reads: std::collections::BTreeSet<String>,
    shut: std::sync::OnceLock<Arc<std::sync::Mutex<super::usage::Usage>>>,
    /// Whether the crew works for quality: the host of an election is then picked by its record.
    goal: crate::crew::Goal,
    /// The points the operator picks at by hand, and the pick rows that wait.
    hands: super::hand::Hands,
    /// What each doer's probe last found, and when (the Log's clock): held while a sweep probes, so two sweeps at once probe a doer once.
    probes: std::sync::Mutex<BTreeMap<String, (u64, Probe)>>,
}

/// The pin of a launch: the doer and the model, as `--doer` and `--model` said them (the model in full).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pin {
    pub doer: Option<String>,
    pub model: Option<String>,
}

impl Who {
    /// `doers` are the ones that can be elected: a flow is a doer, but not one that anyone is elected to be.
    pub fn new(asks: Arc<Asks>, doers: &[Arc<dyn Doer>], roles: BTreeMap<String, Vec<String>>, hosts: Arc<dyn Hosts>, log: Log) -> Who {
        let doers = doers.iter().filter(|d| !d.id().starts_with("flow:") && d.id() != crate::task::HUMAN).cloned().collect();
        Who { asks, doers, roles, docs: BTreeMap::new(), hosts, log, walks: AtomicU64::new(1), said: AtomicU64::new(0), pin: Pin::default(), lists: Arc::new(NoLists), reads: Default::default(), shut: Default::default(), goal: Default::default(), hands: Default::default(), probes: Default::default() }
    }

    /// The same, with what each doer says it is: the options a decider is offered carry it, with the roles.
    pub fn with_docs(mut self, docs: BTreeMap<String, String>) -> Who {
        self.docs = docs;
        self
    }

    /// The same, bound by what the launch pinned: an election for a role takes the pinned doer when it is ready and good for the role, and runs
    /// the doer it elects on the pinned model when that doer serves it.
    pub fn with_pin(mut self, pin: Pin, lists: Arc<dyn ModelLists>) -> Who {
        (self.pin, self.lists) = (pin, lists);
        self
    }

    /// The same, told whether the crew works for quality (`crew.goal`).
    pub fn with_goal(mut self, goal: crate::crew::Goal) -> Who {
        self.goal = goal;
        self
    }

    /// The same, told which doers say their own limit: while one is shut, the election goes to another doer that is open.
    pub fn with_limit_readers(mut self, doers: std::collections::BTreeSet<String>) -> Who {
        self.reads = doers;
        self
    }

    /// Where the limits that have been said are kept (the front's): without it nothing is shut.
    pub fn watch_limits(&self, usage: Arc<std::sync::Mutex<super::usage::Usage>>) {
        let _ = self.shut.set(usage);
    }

    /// When `doer` is let in again, if it says its own limit and a window of it is shut now.
    fn shut_until(&self, doer: &str) -> Option<u64> {
        if !self.reads.contains(doer) {
            return None;
        }
        let mut usage = self.shut.get()?.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        usage.catch_up(&self.log);
        usage.limited_until(doer, "", "", self.log.clock().now_ms())
    }

    /// The pinned doer for `role`, or why it cannot be used (`None`: nothing is pinned).
    fn pinned(&self, role: &str, exclude: &[String]) -> Option<Result<String, String>> {
        let d = self.pin.doer.as_deref()?;
        let why = if exclude.iter().any(|e| e == d) {
            "the step leaves it out".to_string()
        } else if !self.doers.iter().any(|x| x.id() == d) {
            "there is no such doer".to_string()
        } else if self.roles.get(d).is_some_and(|r| !r.is_empty() && !r.iter().any(|x| x == role)) {
            format!("it is not good for `{role}`")
        } else if !self.ready(role, exclude).iter().any(|o| o.id == d) {
            "it is not installed or not signed in".to_string()
        } else {
            return Some(Ok(d.to_string()));
        };
        Some(Err(format!("--doer {d} cannot take `{role}`: {why}")))
    }

    /// The pinned model as `doer` names it, or why `doer` does not serve it (`None`: no model is pinned). A doer that is the pinned one and lists no
    /// models is given the word as it was typed, as a bound thread is.
    fn pinned_model(&self, doer: &str, own: bool) -> Option<Result<String, String>> {
        let m = self.pin.model.as_deref()?;
        Some(match settle(self.lists.as_ref(), doer, m) {
            Ok(Some(Resolved::One(name))) => Ok(name),
            Ok(Some(Resolved::Many(all))) => Ok(all.into_iter().next().unwrap_or_else(|| m.to_string())),
            Ok(None) if own => Ok(m.to_string()),
            Ok(_) | Err(_) => Err(format!("--model {m} is not one of {doer}'s models: it keeps its own")),
        })
    }

    /// The operator said a line now.
    pub fn said(&self) {
        self.said.store(self.log.clock().now_ms().max(1), Ordering::SeqCst);
    }

    /// Whether the operator has been at the keyboard lately.
    pub fn waiting(&self) -> bool {
        let at = self.said.load(Ordering::SeqCst);
        at > 0 && self.log.clock().now_ms().saturating_sub(at) < WAITING_MS
    }

    /// Nobody is at the keyboard (a run).
    pub fn headless(&self) -> bool {
        self.asks.headless
    }

    /// Whether signed charters may answer approvals here: where there is an operator to be asked about the rest.
    pub fn charters_answer(&self) -> bool {
        self.asks.charters.is_some()
    }

    /// The Gate the approvals are checked against.
    pub fn gate(&self) -> Option<crate::gate::Gate> {
        self.asks.gate.clone()
    }

    /// `--approval full`: every yes that the Gate does not forbid is given.
    pub fn approves_all(&self) -> bool {
        self.asks.full
    }

    pub fn doers(&self) -> &[Arc<dyn Doer>] {
        &self.doers
    }

    /// The ask point `id`, if there is one.
    pub fn point(&self, id: &str) -> Option<&crate::answer::AskPoint> {
        self.asks.points.get(id)
    }

    /// What each option of each point means.
    pub fn says(&self) -> &crate::answer::Says {
        &self.asks.says
    }

    pub fn hands(&self) -> &super::hand::Hands {
        &self.hands
    }

    pub fn hosts(&self) -> &Arc<dyn Hosts> {
        &self.hosts
    }

    pub fn question(&self, point: &str, thread: Option<&str>, text: &str, options: Vec<Opt>) -> Question {
        let n = self.walks.fetch_add(1, Ordering::SeqCst);
        Question {
            ask: format!("{point}#{}-{n}", self.log.clock().now_ms()),
            point: point.into(),
            asker: "front".into(),
            thread: thread.map(String::from),
            text: text.into(),
            shape: described(AskShape::Choice { options }, &self.asks.says, point),
            goal: text.into(),
            act: None,
        }
    }

    /// Walk a point's chain. Nobody is asked in the middle of a line: what no answerer settles, the point's fallback does. What the
    /// answerers did is written (so the breaker counts it and a machine answer can be labelled); `.3` is the machine answer that
    /// stood, if one did.
    pub fn pick(&self, point: &str, thread: Option<&str>, q: &Question) -> Option<(String, String, String, Option<AnswerRef>)> {
        // a point the operator picks at by hand gets the `human` step, and a person at the keyboard is shown one pick row (`front::hand`)
        let hand = self.hands.has(point) && !self.asks.headless;
        let asked = self.ask_by(point, thread, q, None, hand)?;
        let walked = match asked.walk {
            Walk::ToHuman { after_ms, .. } if hand => {
                let AskShape::Choice { options } = &q.shape else { return None };
                let wait = after_ms.map_or(super::hand::WAIT, Duration::from_millis);
                match self.hands.ask(point, &q.text, options, wait) {
                    Some(id) => {
                        let mut said = New::of("ask.picked", Actor::Operator, json!({ "ask": q.ask, "point": point, "pick": id }));
                        if let Some(t) = thread {
                            said = said.thread(t);
                        }
                        let _ = self.log.append(said);
                        return Some((id, "you".into(), "you picked it".into(), None));
                    }
                    // Esc, or no key: the point's own fallback, with no answerer asked again
                    None => self.ask_by(point, thread, q, Some(""), false)?.walk,
                }
            }
            other => other,
        };
        let (Walk::Answered(o) | Walk::Fallback(o)) = walked else { return None };
        let Answer::Pick { id } = o.answer.clone() else { return None };
        let said = (matches!(o.kind, Kind::Decider | Kind::Model) && o.conf.is_some()).then(|| {
            let model = self.asks.registry.get(&o.by).map(|a| a.model()).unwrap_or_default();
            AnswerRef { ask: q.ask.clone(), answerer: o.by.clone(), model, point: point.into() }
        });
        Some((id, o.by, o.why, said))
    }

    /// The walk of [`Who::pick`], whole: how it ended, what each step did and how long it took, and how long the probes were waited
    /// for. `only` walks one answerer of the chain and no other (`j4n answer --by`): the point's fallback is then not what it says. `None`: there is no
    /// such point.
    pub fn ask(&self, point: &str, thread: Option<&str>, q: &Question, only: Option<&str>) -> Option<Asked> {
        self.ask_by(point, thread, q, only, false)
    }

    /// [`Who::ask`], and with `hand` the chain ends with the `human` step if it has none.
    fn ask_by(&self, point: &str, thread: Option<&str>, q: &Question, only: Option<&str>, hand: bool) -> Option<Asked> {
        let mut p = self.asks.points.get(point)?.clone();
        if hand && !p.chain.iter().any(|s| s.kind == Kind::Human) {
            p.chain.push(crate::answer::Step { kind: Kind::Human, name: String::new(), min_conf: None, after_ms: None });
        }
        if let Some(by) = only {
            p.chain.retain(|s| s.id() == by);
        }
        self.asks.remember(q);
        let write = |kind: &str, by: Actor, data: serde_json::Value| {
            let mut n = New::of(kind, by, data);
            if let Some(t) = thread {
                n = n.thread(t);
            }
            let _ = self.log.append(n);
        };
        // nobody is there to ask later: a decider whose cases for this point are not proved yet is proved now, within a bound, and
        // not skipped for want of a minute (a run with a person at it does not wait: the cases run beside the line); a rule asked
        // for by name needs no proof
        let rule = only.and_then(|by| self.asks.registry.get(by)).is_some_and(|a| !a.kind().is_model());
        let mut waited = Duration::ZERO;
        if let Some(prober) = self.asks.prober.as_ref().filter(|_| self.asks.headless && !rule && !self.asks.probe_wait().is_zero())
            && let Some(w) = prober.wait_for(point, self.asks.probe_wait(), self.log.clock().now_ms())
        {
            waited = w;
            write("ask.note", Actor::Core, json!({ "ask": q.ask, "why": format!("waited {} ms for the probe cases of {point}", w.as_millis()) }));
        }
        let (now, began) = (self.log.clock().now_ms(), Instant::now());
        let (walk, notes) = walk(&p, q, &self.asks.registry, &self.asks.state, &Ledger::new(), now, !hand);
        let ms = began.elapsed().as_millis() as u64;
        let spent = Spent::of(&notes);
        for n in &notes {
            let mut data = n.data.clone();
            // a point outside the breaker says so on each step, and the answerers' fold passes it by
            if !p.breaker && n.kind == "ask.step" {
                data["breaker"] = json!(false);
            }
            write(&n.kind, Actor::Core, data);
        }
        if let Walk::Answered(o) = &walk
            && matches!(o.kind, Kind::Decider | Kind::Model)
            && o.conf.is_some()
        {
            let mut data = json!({ "ask": q.ask, "point": point, "impact": p.impact.word(), "by": o.by, "kind": o.kind.word(), "answer": o.answer.render(), "conf": o.conf, "why": o.why, "via": "chain" });
            // what the walk cost, whole: a number, or `null` when a decider or a model did not say
            data.as_object_mut().into_iter().for_each(|d| d.extend(spent.json().as_object().cloned().unwrap_or_default()));
            write("ask.answered", Actor::Model(o.by.clone()), data);
        }
        Some(Asked { walk, notes, ms, waited: waited.as_millis() as u64, spent })
    }

    /// The doers that can take a role now: installed, logged in, good for it, and not excluded.
    pub fn ready(&self, role: &str, exclude: &[String]) -> Vec<Opt> {
        self.sift(role, exclude).0
    }

    /// [`Who::ready`], and the doers that were left out for want of a login, each with the probe's words.
    fn sift(&self, role: &str, exclude: &[String]) -> (Vec<Opt>, Vec<String>) {
        let (mut ready, mut skipped) = (Vec::new(), Vec::new());
        let good: Vec<&Arc<dyn Doer>> = self.doers.iter().filter(|d| !exclude.iter().any(|e| e == d.id())).filter(|d| self.roles.get(d.id()).is_none_or(|r| r.is_empty() || r.iter().any(|x| x == role))).collect();
        let found = self.probed(&good);
        for (d, p) in good.into_iter().zip(found) {
            if !(p.installed && p.authed) {
                if p.installed {
                    skipped.push(format!("{} ({})", d.id(), p.detail));
                }
                continue;
            }
            let roles = self.roles.get(d.id()).filter(|r| !r.is_empty()).map(|r| format!(" (good for {})", r.join(", "))).unwrap_or_default();
            let label = self.docs.get(d.id()).filter(|t| !t.is_empty()).map_or_else(|| d.id().to_string(), |t| format!("{t}{roles}"));
            ready.push(Opt { id: d.id().into(), label, recommended: false });
        }
        (ready, skipped)
    }

    /// What the probe of `id` found, from the same short-lived probes an election reads; none: no doer has that id.
    pub fn probe_of(&self, id: &str) -> Option<Probe> {
        let d = self.doers.iter().find(|d| d.id() == id)?;
        self.probed(&[d]).pop()
    }

    /// What the probe of each of `doers` found, in their order: a probe younger than [`PROBE_FRESH_MS`] is taken as it stands, the others are made at once,
    /// each on a thread of its own, so a sweep waits for its slowest probe and not for the sum. A probe that panics found nothing installed.
    fn probed(&self, doers: &[&Arc<dyn Doer>]) -> Vec<Probe> {
        let mut seen = self.probes.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = self.log.clock().now_ms();
        let stale: Vec<&Arc<dyn Doer>> = doers.iter().copied().filter(|d| seen.get(d.id()).is_none_or(|(at, _)| now.saturating_sub(*at) >= PROBE_FRESH_MS)).collect();
        let fresh: Vec<(String, Probe)> = std::thread::scope(|s| {
            let jobs: Vec<_> = stale.iter().map(|d| (d.id().to_string(), s.spawn(move || d.probe()))).collect();
            jobs.into_iter().map(|(id, j)| (id, j.join().unwrap_or_default())).collect()
        });
        let at = self.log.clock().now_ms();
        for (id, p) in fresh {
            seen.insert(id, (at, p));
        }
        doers.iter().map(|d| seen.get(d.id()).map(|(_, p)| p.clone()).unwrap_or_default()).collect()
    }

    /// The doer for `role`, from the ask point `who:<role>` (else `who:work`), and the machine answer that picked it. `Err`: nobody
    /// is installed and logged in for it.
    pub fn doer(&self, role: &str, thread: Option<&str>, text: &str, exclude: &[String]) -> Result<(String, Option<AnswerRef>, String), String> {
        let (mut ready, mut skipped) = self.sift(role, exclude);
        let now = self.log.clock().now_ms();
        let shut: Vec<String> = ready.iter().filter_map(|o| self.shut_until(&o.id).map(|at| format!("{} (limited, back in {})", o.id, super::usage::hm(at.saturating_sub(now))))).collect();
        if !shut.is_empty() && shut.len() < ready.len() {
            ready.retain(|o| self.shut_until(&o.id).is_none());
            skipped.extend(shut);
        }
        let left = if skipped.is_empty() { String::new() } else { format!("; left out: {}", skipped.join(", ")) };
        if ready.is_empty() {
            return Err(if exclude.is_empty() { format!("no doer is installed, logged in and good for `{role}`{left}") } else { format!("no doer other than {} is installed, logged in and good for `{role}`{left}", exclude.join(", ")) });
        }
        let point = if self.asks.points.contains_key(&format!("who:{role}")) { format!("who:{role}") } else { "who:work".to_string() };
        let q = self.question(&point, thread, text, ready.clone());
        // the chain only takes a pick from what it offered; with no point to walk, the first that is ready
        Ok(match self.pick(&point, thread, &q) {
            Some((id, _, why, said)) => (id, said, format!("{why}{left}")),
            None => (ready[0].id.clone(), None, format!("the first that is ready{left}")),
        })
    }
}

impl Elect for Who {
    fn elect(&self, role: &str, text: &str, thread: Option<&str>, exclude: &[String], mode: Mode) -> Result<Elected, String> {
        // a pin of the launch binds the election: `--doer` takes the role when it is ready and good for it, else the election runs and says why not
        let (doer, why) = match self.pinned(role, exclude) {
            Some(Ok(d)) => (d.clone(), format!("--doer {d} is pinned, and it takes `{role}`")),
            other => {
                let (d, _, why) = self.doer(role, thread, text, exclude)?;
                (d, match other { Some(Err(note)) => format!("{note}; {why}"), _ => why })
            }
        };
        let (model, why) = match self.pinned_model(&doer, self.pin.doer.as_deref() == Some(doer.as_str())) {
            Some(Ok(m)) => (Some(m.clone()), format!("{why}; --model puts it on {m}")),
            Some(Err(note)) => (None, format!("{why}; {note}")),
            None => (None, why),
        };
        let want = Want { role: role.into(), mode, waiting: self.waiting(), blocked: true, quality: self.goal.quality(), ..Want::default() };
        let host = self.hosts.elect(&doer, model.as_deref(), &want, self.log.clock().now_ms());
        Ok(match host {
            Some(p) => Elected { doer, model: Some(model.unwrap_or(p.model)), endpoint: Some(p.host), why: format!("{why}; {}", p.why) },
            None => Elected { doer, model, endpoint: None, why },
        })
    }
}

/// The standing numbers of a doer for a role, read from the fold of tasks: `reliability`, the smoothed
/// chance a task of that role ended well (a doer with no record is not ranked below one that did badly), and `cost`, the dollars
/// it took to end one. The fold is not there until the loop is: it is given afterwards.
#[derive(Clone, Default)]
pub struct Standing {
    book: Arc<std::sync::OnceLock<crate::task::Book>>,
    /// The crew's goal and the board's scores (the pack `crew`): under `quality` a doer is also given the number the quality pick ranks by.
    quality: Option<(crate::crew::Goal, Arc<Vec<crate::crew::Seed>>)>,
}

impl Standing {
    pub fn new() -> Standing {
        Standing::default()
    }

    /// The same, with the goal and the seeds the quality pick reads.
    pub fn with_quality(mut self, goal: crate::crew::Goal, seeds: Vec<crate::crew::Seed>) -> Standing {
        self.quality = Some((goal, Arc::new(seeds)));
        self
    }

    pub fn set(&self, book: crate::task::Book) {
        let _ = self.book.set(book);
    }
}

impl crate::rank::Columns for Standing {
    fn quality(&self) -> bool {
        self.quality.as_ref().is_some_and(|(g, _)| g.quality())
    }

    fn columns(&self, point: &str, option: &str) -> BTreeMap<String, f64> {
        let (Some(role), Some(book)) = (point.strip_prefix("who:"), self.book.get()) else { return BTreeMap::new() };
        // Main's turns are `work` turns: a doer that has done well at work has done well as Main
        let role = if role == "main" { "work" } else { role };
        let (mut done, mut failed, mut spent) = (0u32, 0u32, 0i64);
        let mut outcomes = Vec::new();
        for r in book.rows().iter().filter(|r| r.spec.doer == option && r.spec.role.as_deref() == Some(role)) {
            match &r.state {
                crate::task::State::Done { ok: true } => {
                    done += 1;
                    spent += r.spent;
                    outcomes.push(true);
                }
                crate::task::State::Done { ok: false } | crate::task::State::Stopped { why: crate::task::Why::Failed { .. } } => {
                    failed += 1;
                    outcomes.push(false);
                }
                _ => {}
            }
        }
        // under quality the number the pick ranks by: the record when it is enough, else the board's score for the doer; the others are shown as they were
        let quality = self.quality.as_ref().filter(|(g, _)| g.quality()).and_then(|(g, seeds)| crate::crew::quality_column(&outcomes, crate::crew::seed_score(seeds, option), g.record_min()));
        if done + failed == 0 {
            return quality.map(|q| BTreeMap::from([("quality".to_string(), q)])).unwrap_or_default();
        }
        let mut cols = BTreeMap::from([("reliability".to_string(), f64::from(done + 1) / f64::from(done + failed + 2))]);
        if done > 0 && spent > 0 {
            cols.insert("cost".into(), spent as f64 / 1e6 / f64::from(done));
        }
        if let Some(q) = quality {
            cols.insert("quality".into(), q);
        }
        cols
    }
}
