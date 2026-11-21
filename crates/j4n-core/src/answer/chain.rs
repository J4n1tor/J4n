//! The walk of a point's chain. [`walk`] is a function of the point,
//! the question, the answerers that are installed and what the Log says of them; it writes nothing. It returns what stands
//! ([`Walk`]) and the events the loop should write ([`Note`]); the loop calls it where the answerers may block, never on
//! its own thread, and calls it **once per ask**.
//!
//! What the walk keeps, whatever an answerer says:
//!
//! - steps run in order; a machine step whose answerer is not installed, is `Off` (the breaker) or `Shadow`, or is not of
//!   the kind the chain names, is skipped and said once per asker task; `human` ends the walk (`ToHuman`), unless there is
//!   no operator (`headless`), where the point's [`fallback`] stands;
//! - an answer is an answer only if the question's own shape accepts it (a pick is offered, a text is one short line) and
//!   its confidence is a number from 0 to 1 and reaches the step's `min_conf`; anything else is `failed` or a `pass`;
//! - an approval is never put to a decider or a model, and such an answerer is not called: the question's shape decides,
//!   not only the point's;
//! - one asker task gets at most [`MAX_MACHINE`] answers from deciders and models, and the same question (point, text,
//!   options) is put to them once; after that those steps are skipped, so the operator or the fallback answers. A rule (the
//!   Gate, a grant, a phrase table) is deterministic and is not counted: an approval asked twice is answered twice;
//! - at `Impact::Gate` (an approval excepted: it has no machine but rules) two distinct machine answerers (rule, decider or model; a rank is not one) must give the same
//!   answer: agreement stands with `pair`; disagreement, or fewer than two, goes to the operator (headless: the fallback)
//!   and keeps what was said in `pair`, so a Rule can label it `agreed` or `disagreed`. With only one rule in the chain a
//!   gate point always goes to the operator;
//! - an answerer that panics has `failed`.
//!
//! Events: `ask.step {ask, answerer, kind, outcome: answer|pass|failed|skipped, conf?, why, ms, usd?, tokens_in?, tokens_out?}` for
//! every step that was tried or skipped by name (a decider's or a model's step that was called says `usd`: a number, or `null` when it
//! did not report a price)), and `ask.note {ask, why}` for what is true of the whole walk. The `human` step makes none.

use super::{Answerer, AskPoint, Egress, Fallback, Impact, Kind, Offer, Paid, Question, ShapeKind, State, StateView};
use crate::gate::Verdict;
use crate::ask::{Answer, AskShape};
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// How much of the goal an `ask.egress` row keeps.
const GOAL_LOGGED: usize = 400;

/// The most machine answers one asker task gets.
pub const MAX_MACHINE: usize = 6;
/// The most things a ledger remembers (question hashes and notices); past it every question counts as asked before.
const LEDGER_MAX: usize = 4096;

/// The installed answerers by id (`rule:route`, `rank`, `decider:jev`).
#[derive(Clone, Default)]
pub struct Registry(BTreeMap<String, Arc<dyn Answerer>>, Option<Arc<dyn Egress>>);

impl Registry {
    /// Put the egress check that answerers with a zone are put through. Without one such an answerer is never called.
    pub fn with_egress(mut self, egress: Arc<dyn Egress>) -> Registry {
        self.1 = Some(egress);
        self
    }

    pub fn new() -> Registry {
        Registry::default()
    }

    /// Install `a` under its own id, replacing one with that id. The operator is not an answerer, and a nameless one is not
    /// installed; both return false.
    pub fn register(&mut self, a: Arc<dyn Answerer>) -> bool {
        if a.kind() == Kind::Human || a.id().is_empty() {
            return false;
        }
        self.0.insert(a.id().to_string(), a);
        true
    }

    /// Whether a question carrying data of this class may go to this zone, and if not, why.
    pub fn allows(&self, class: crate::schema::DataClass, zone: &str) -> Result<(), String> {
        match self.1.as_ref().map(|e| e.check(class, zone)) {
            Some(Verdict::Allow { .. }) => Ok(()),
            Some(Verdict::Ask { why, .. }) => Err(format!("egress to `{zone}` needs a yes: {why}")),
            Some(Verdict::Block { why, .. }) => Err(format!("egress to `{zone}` is blocked: {why}")),
            None => Err("nothing checks where its question goes".to_string()),
        }
    }

    /// The answerer `id`, while it can be asked ([`Answerer::ready`]): one whose key is not there now is none, live.
    pub fn get(&self, id: &str) -> Option<Arc<dyn Answerer>> {
        self.0.get(id).filter(|a| a.ready()).cloned()
    }

    /// Whether `id` is installed, ready or not.
    pub fn installed(&self, id: &str) -> bool {
        self.0.contains_key(id)
    }

    pub fn ids(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }
}

#[derive(Default)]
struct Seen {
    given: usize,
    once: HashSet<String>,
}

/// What one asker task has had from machines: how many answers, which questions, which notices. Shared by every walk of the
/// task; a walk holds it only to read and write these, never while an answerer is called.
#[derive(Default)]
pub struct Ledger(Mutex<Seen>);

impl Ledger {
    pub fn new() -> Ledger {
        Ledger::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Seen> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The machine answers taken so far.
    pub fn given(&self) -> usize {
        self.lock().given
    }

    /// True the first time `key` is seen (and while there is room to remember it).
    fn first(&self, key: String) -> bool {
        let mut s = self.lock();
        s.once.len() < LEDGER_MAX && s.once.insert(key)
    }
}

/// An event for the loop to write.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub kind: String,
    pub data: Json,
}

/// What stands as the answer, and who gave it.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub answer: Answer,
    /// The answerer's id; for a gate pair the first of the two; `fallback:<word>` for a fallback made here.
    pub by: String,
    pub kind: Kind,
    /// For a pair, the lower of the two. `None` for a fallback made here.
    pub conf: Option<f32>,
    pub why: String,
    /// The two machine answers of a gate pair (or what was said before a fallback), by answerer.
    pub pair: Vec<(String, Answer)>,
}

/// How a walk ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Walk {
    /// A machine answered (or two did, at gate impact).
    Answered(Outcome),
    /// The operator answers; after `after_ms` the point's fallback stands in (`None`: the loop's own deadline). `pair` holds
    /// the machine answers given at gate impact (two that disagreed, or one).
    ToHuman { after_ms: Option<u64>, why: String, pair: Vec<(String, Answer)> },
    /// No operator (or no human step): the point's fallback.
    Fallback(Outcome),
    /// Nobody can answer and nothing may wait for it: the asker goes on without.
    Skip { why: String },
}

/// `[{by, answer}]` for an event.
pub fn pair_json(pair: &[(String, Answer)]) -> Json {
    pair.iter().map(|(by, answer)| json!({ "by": by, "answer": answer })).collect()
}

/// One line, no control characters, at most 200 characters: what an answerer says about itself goes into events as this.
fn tidy(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).take(200).collect()
}

/// Call an answerer; a panic is a failure. Returns the offer, what the call cost when the answerer says, and the milliseconds it took.
fn call(a: &dyn Answerer, q: &Question) -> (Offer, Option<Paid>, u64) {
    let t = Instant::now();
    let (offer, paid) = catch_unwind(AssertUnwindSafe(|| a.offer_paid(q))).unwrap_or_else(|_| (Offer::Failed { why: "it panicked".into() }, None));
    (offer, paid, t.elapsed().as_millis() as u64)
}

/// What the answerers of a walk cost: the sum of the `usd` of its `ask.step` notes. A step of a rule, a rank or the operator
/// costs nothing and a step that was skipped was not called, so neither says; a decider's or a model's step says `usd` as a number,
/// or as `null` when it did not report one — and then the sum is unknown (`usd: None`), never the part that was known.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spent {
    pub usd: Option<f64>,
    pub tokens_in: u64,
    pub tokens_out: u64,
}

impl Default for Spent {
    fn default() -> Spent {
        Spent { usd: Some(0.0), tokens_in: 0, tokens_out: 0 }
    }
}

impl Spent {
    /// The walk whose steps are `notes`.
    pub fn of(notes: &[Note]) -> Spent {
        let mut sum = Spent::default();
        for n in notes.iter().filter(|n| n.kind == "ask.step") {
            sum.add(&Spent {
                usd: n.data.get("usd").map_or(Some(0.0), Json::as_f64),
                tokens_in: n.data.get("tokens_in").and_then(Json::as_u64).unwrap_or(0),
                tokens_out: n.data.get("tokens_out").and_then(Json::as_u64).unwrap_or(0),
            });
        }
        sum
    }

    pub fn add(&mut self, other: &Spent) {
        // to a nanodollar, so float noise does not make a sum differ from itself once it has been through the Log
        self.usd = self.usd.zip(other.usd).map(|(a, b)| ((a + b) * 1e9).round() / 1e9);
        self.tokens_in += other.tokens_in;
        self.tokens_out += other.tokens_out;
    }

    /// The fields an event says it by: `usd` is a number, or `null` for unknown; the tokens as reported.
    pub fn json(&self) -> Json {
        json!({ "usd": self.usd, "tokens_in": self.tokens_in, "tokens_out": self.tokens_out })
    }
}

/// The same question is the same point, text and options (and, for an approval, the action).
fn question_hash(point: &AskPoint, q: &Question) -> String {
    let text = serde_json::to_vec(&(&point.id, &q.text, &q.shape)).unwrap_or_default();
    crate::text::hex_hash(text)
}

struct Notes<'a> {
    ask: &'a str,
    list: Vec<Note>,
}

impl Notes<'_> {
    fn step(&mut self, id: &str, kind: Kind, outcome: &str, conf: Option<f32>, why: &str, ms: u64) {
        self.step_paid(id, kind, outcome, conf, why, ms, None);
    }

    /// A step that was called: a decider's or a model's says what the call cost (`usd` is `null` when it did not say).
    #[allow(clippy::too_many_arguments)]
    fn step_paid(&mut self, id: &str, kind: Kind, outcome: &str, conf: Option<f32>, why: &str, ms: u64, paid: Option<Paid>) {
        let mut data = json!({ "ask": self.ask, "answerer": id, "kind": kind.word(), "outcome": outcome, "why": tidy(why), "ms": ms });
        if kind.is_model() && outcome != "skipped" {
            data["usd"] = json!(paid.and_then(|p| p.usd));
            if let Some(p) = paid {
                (data["tokens_in"], data["tokens_out"]) = (json!(p.tokens_in), json!(p.tokens_out));
            }
        }
        if let Some(c) = conf {
            data["conf"] = json!((f64::from(c) * 1000.0).round() / 1000.0);
        }
        self.list.push(Note { kind: "ask.step".into(), data });
    }

    fn say(&mut self, why: &str) {
        self.list.push(Note { kind: "ask.note".into(), data: json!({ "ask": self.ask, "why": tidy(why) }) });
    }
}

/// The answer the point's fallback stands for, or `None` when it cannot make a valid one (the question then stays open for
/// the operator). `Rank` asks the installed `rank` answerer, else takes the recommended option, else the first offered;
/// `Safe` is `no` (yes/no, approval) or the option `safe`; `Pick` must be offered, else the first offered; `Text` is the line.
/// Whatever it makes passes the question's shape check.
pub fn fallback(point: &AskPoint, q: &Question, reg: &Registry) -> Option<Outcome> {
    let options = match &q.shape {
        AskShape::Choice { options } => options.as_slice(),
        _ => &[],
    };
    let made = |answer: Answer, by: &str, kind: Kind, why: String| {
        q.shape.check(&answer).ok().map(|()| Outcome { answer, by: by.into(), kind, conf: None, why, pair: Vec::new() })
    };
    let pick = |id: &str| Answer::Pick { id: id.into() };
    match &point.fallback {
        Fallback::Rank => {
            if let Some(r) = reg.get("rank").filter(|r| r.kind() == Kind::Rank)
                && let (Offer::Answer { answer, why, .. }, ..) = call(&*r, q)
                && let Some(o) = made(answer, "rank", Kind::Rank, tidy(&why))
            {
                return Some(o);
            }
            let o = options.iter().find(|o| o.recommended);
            let why = if o.is_some() { "the recommended option" } else { "the first option offered" };
            made(pick(&o.or(options.first())?.id), "fallback:rank", Kind::Rule, why.into())
        }
        Fallback::Safe => {
            let safe = if matches!(q.shape, AskShape::YesNo | AskShape::Approval { .. }) { Answer::No } else { pick("safe") };
            made(safe, "fallback:safe", Kind::Rule, "the safe answer".into())
        }
        Fallback::Pick(id) => {
            let id = if options.iter().any(|o| &o.id == id) { id } else { &options.first()?.id };
            made(pick(id), "fallback:pick", Kind::Rule, format!("the point's pick, `{id}`"))
        }
        Fallback::Text(line) => made(Answer::Text { text: line.clone() }, "fallback:text", Kind::Rule, "the point's line".into()),
    }
}

/// How a walk ends when no one answered: the operator if there is one and a human step to reach them, else the fallback.
fn end(point: &AskPoint, q: &Question, reg: &Registry, headless: bool, why: &str, pair: Vec<(String, Answer)>) -> Walk {
    let human = point.chain.iter().find(|s| s.kind == Kind::Human);
    let stand = |lead: &str, pair| {
        fallback(point, q, reg).map(|mut o| {
            o.why = format!("{lead}: {}", o.why);
            o.pair = pair;
            Walk::Fallback(o)
        })
    };
    if headless {
        return stand("no operator", pair).unwrap_or(Walk::Skip { why: "no operator, and the fallback has no answer".into() });
    }
    match human {
        Some(h) => Walk::ToHuman { after_ms: h.after_ms, why: why.into(), pair },
        None => stand("no one answered", pair.clone()).unwrap_or(Walk::ToHuman { after_ms: None, why: why.into(), pair }),
    }
}

/// A machine answer that was taken.
struct Given {
    id: String,
    kind: Kind,
    answer: Answer,
    conf: f32,
    why: String,
}

/// Walk `point`'s chain for `q`. See the module doc for the rules; `now_ms` is only for the breaker's `until`.
pub fn walk(
    point: &AskPoint,
    q: &Question,
    reg: &Registry,
    state: &dyn StateView,
    ledger: &Ledger,
    now_ms: u64,
    headless: bool,
) -> (Walk, Vec<Note>) {
    let mut n = Notes { ask: &q.ask, list: Vec::new() };
    let walked = run(point, q, reg, state, ledger, now_ms, headless, &mut n);
    (walked, n.list)
}

#[allow(clippy::too_many_arguments)]
fn run(
    point: &AskPoint,
    q: &Question,
    reg: &Registry,
    state: &dyn StateView,
    ledger: &Ledger,
    now_ms: u64,
    headless: bool,
    n: &mut Notes,
) -> Walk {
    let approval = point.shape == ShapeKind::Approval || ShapeKind::of(&q.shape) == ShapeKind::Approval;
    // an approval is answered by one rule, a grant or the operator; the pair is for the other questions that can do harm
    let gate = point.impact == Impact::Gate && !approval;
    if matches!(&q.shape, AskShape::Choice { options } if options.is_empty()) {
        n.say("nothing is offered to choose from");
        let why = "nothing is offered to choose from";
        return if gate {
            end(point, q, reg, headless, why, Vec::new())
        } else {
            fallback(point, q, reg).map_or(Walk::Skip { why: why.into() }, Walk::Fallback)
        };
    }
    let repeat = !ledger.first(format!("q:{}", question_hash(point, q)));
    let (mut got, mut tried, mut closed, mut held): (Vec<Given>, HashSet<String>, bool, bool) = Default::default();
    let pair = |got: &[Given]| got.iter().map(|g| (g.id.clone(), g.answer.clone())).collect::<Vec<_>>();
    for step in &point.chain {
        if step.kind == Kind::Human {
            let why = if closed { "the two answerers disagreed" } else if gate { "fewer than two answerers were available" } else { "no machine answered" };
            return end(point, q, reg, headless, why, pair(&got));
        }
        let id = step.id();
        if closed {
            continue;
        }
        let skip = |n: &mut Notes, why: &str| n.step(&id, step.kind, "skipped", None, why, 0);
        if !tried.insert(id.clone()) {
            skip(n, "it is in the chain twice");
            continue;
        }
        if approval && step.kind.is_model() {
            skip(n, "an approval is never put to a model");
            continue;
        }
        if gate && step.kind == Kind::Rank {
            skip(n, "a rank does not make a pair");
            continue;
        }
        let Some(a) = reg.get(&id) else {
            if ledger.first(format!("n:{id}:missing")) {
                skip(n, if reg.installed(&id) { "not installed until its key is typed (Library › Providers)" } else { "not installed" });
            }
            continue;
        };
        if a.kind() != step.kind {
            if ledger.first(format!("n:{id}:kind")) {
                skip(n, &format!("it is a {}, the chain says {}", a.kind().word(), step.kind.word()));
            }
            continue;
        }
        match state.state_for(&id, &point.id, &a.model(), now_ms) {
            State::Active => {}
            State::Off { until_ms } => {
                if ledger.first(format!("n:{id}:off")) {
                    skip(n, &format!("off until {until_ms}: it failed {} times in a row", super::state::BREAKER_AFTER));
                }
                continue;
            }
            State::Shadow { why } => {
                if ledger.first(format!("n:{id}:shadow")) {
                    skip(n, &format!("shadow: {why}"));
                }
                continue;
            }
        }
        if step.kind.is_model() && (repeat || ledger.given() >= MAX_MACHINE) {
            if !std::mem::replace(&mut held, true) {
                n.say(if repeat { "this question was put to machines before" } else { "this task has had its machine answers" });
            }
            continue;
        }
        if let Some(zone) = a.zone() {
            if let Err(why) = reg.allows(point.data, &zone) {
                if ledger.first(format!("n:{id}:egress")) {
                    skip(n, &why);
                }
                continue;
            }
            let mut data = json!({ "ask": q.ask, "answerer": id, "zone": zone, "class": point.data.word() });
            // What the answerer is told the asker is doing, when that is not the question's own words (the Log redacts it like any text)
            if !q.goal.is_empty() && q.goal != q.text {
                data["goal"] = q.goal.chars().take(GOAL_LOGGED).collect::<String>().into();
            }
            n.list.push(Note { kind: "ask.egress".into(), data });
        }
        let (offer, paid, ms) = call(&*a, q);
        let mut taken = None;
        let (outcome, conf, why) = match offer {
            Offer::Pass { why } => ("pass", None, why),
            Offer::Failed { why } => ("failed", None, why),
            Offer::Answer { answer, conf, why } => {
                if !(0.0..=1.0).contains(&conf) {
                    ("failed", None, "its confidence is not a number from 0 to 1".into())
                } else if let Err(e) = q.shape.check(&answer) {
                    ("failed", Some(conf), format!("its answer is not valid: {e}"))
                } else if let Some(m) = step.min_conf.filter(|m| conf < *m) {
                    ("pass", Some(conf), format!("confidence {conf:.2} is below {m:.2}"))
                } else {
                    taken = Some(Given { id: id.clone(), kind: step.kind, answer, conf, why: why.clone() });
                    ("answer", Some(conf), why)
                }
            }
        };
        n.step_paid(&id, step.kind, outcome, conf, &why, ms, paid);
        let Some(g) = taken else { continue };
        if step.kind.is_model() {
            ledger.lock().given += 1;
        }
        if !gate {
            return Walk::Answered(Outcome { answer: g.answer, by: g.id, kind: g.kind, conf: Some(g.conf), why: tidy(&g.why), pair: Vec::new() });
        }
        got.push(g);
        if let [a, b] = got.as_slice() {
            if a.answer == b.answer {
                let why = format!("{} and {} agree: {}", a.id, b.id, tidy(&a.why));
                let o = Outcome { answer: a.answer.clone(), by: a.id.clone(), kind: a.kind, conf: Some(a.conf.min(b.conf)), why, pair: pair(&got) };
                return Walk::Answered(o);
            }
            closed = true;
            n.say("the two answerers disagreed");
        }
    }
    end(point, q, reg, headless, if gate { "fewer than two answerers were available" } else { "no machine answered" }, pair(&got))
}
