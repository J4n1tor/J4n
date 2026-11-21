//! Who answers a question. This file holds the words the parts share; the parts are:
//!
//! - [`point`]: an *ask point* is data (the kind `ask_point`): the shape of its question, how much a wrong answer costs
//!   ([`Impact`]), how far its data may travel, the **chain** of answerers it walks, and the fallback that stands when no
//!   one answers. A point without a fallback is refused, so J4n always works with no model and no key.
//! - [`chain`]: the walk — each step offers an answer or passes; a pick must be an offered option; an approval never reaches
//!   a model; a gate-impact question needs two machine answers that agree; the operator is the last step.
//! - [`state`]: the state of each answerer (`Active`, `Shadow`, `Off`) as a fold of the Log; `Off` is the breaker.
//!
//! Rank (`crate::rank`) is the answerer that orders what is offered; the binder (`crate::binder`) is how a typed line
//! answers a question that is open.

pub mod chain;
pub mod point;
pub mod probe;
pub mod record;
pub mod state;

use crate::ask::{Answer, AskShape};
use crate::gate::Action;
use crate::schema::DataClass;
use serde::{Deserialize, Serialize};

/// What a wrong answer costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Impact {
    /// Cosmetic: the first confident answer stands.
    Low,
    /// Costly: the first confident answer stands, and a second one may check it off the path.
    Mid,
    /// Unsafe: two distinct active machine answerers must agree, else the operator (headless: the `safe` fallback).
    Gate,
}

impl Impact {
    pub fn word(self) -> &'static str {
        match self {
            Impact::Low => "low",
            Impact::Mid => "mid",
            Impact::Gate => "gate",
        }
    }
}

/// What an answerer is made of. Machine = everything but `Human`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Deterministic: phrase tables, the Gate, a grant, a fact read off the machine.
    Rule,
    /// Orders what is offered by the standing numbers (`crate::rank`).
    Rank,
    /// A decision connector (a small trained model behind a closed vocabulary).
    Decider,
    /// A language model.
    Model,
    /// The operator.
    Human,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Rule => "rule",
            Kind::Rank => "rank",
            Kind::Decider => "decider",
            Kind::Model => "model",
            Kind::Human => "human",
        }
    }

    pub fn from_word(w: &str) -> Option<Kind> {
        [Kind::Rule, Kind::Rank, Kind::Decider, Kind::Model, Kind::Human].into_iter().find(|k| k.word() == w)
    }

    /// Whether a model of any size is behind it: what an approval is never put to.
    pub fn is_model(self) -> bool {
        matches!(self, Kind::Decider | Kind::Model)
    }
}

/// One element of a point's chain, parsed from its text: `rule:route`, `rank`, `decider:jev?min_conf=0.6`, `model:light`,
/// `human?after=60s`.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub kind: Kind,
    /// The part after the colon (empty for `rank` and `human`).
    pub name: String,
    /// An answer below this confidence is not taken; the walk goes on to the next step.
    pub min_conf: Option<f32>,
    /// For `human`: after this long without the operator, the point's fallback stands in.
    pub after_ms: Option<u64>,
}

impl Step {
    /// The answerer's id as the registry knows it: `kind:name`, or `kind` when there is no name.
    pub fn id(&self) -> String {
        if self.name.is_empty() { self.kind.word().to_string() } else { format!("{}:{}", self.kind.word(), self.name) }
    }
}

/// What stands when no one answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fallback {
    /// The head of the Rank of the offered options.
    Rank,
    /// The conservative answer: `no` for a yes/no or an approval, the option whose id is `safe` for a choice.
    Safe,
    /// This option, by id.
    Pick(String),
    /// This line of text.
    Text(String),
}

/// What kind of question a point asks (an [`AskShape`] without its contents).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShapeKind {
    YesNo,
    Choice,
    Approval,
    Text,
}

impl ShapeKind {
    pub fn of(shape: &AskShape) -> ShapeKind {
        match shape {
            AskShape::YesNo => ShapeKind::YesNo,
            AskShape::Choice { .. } => ShapeKind::Choice,
            AskShape::Approval { .. } => ShapeKind::Approval,
            AskShape::Text => ShapeKind::Text,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            ShapeKind::YesNo => "yesno",
            ShapeKind::Choice => "choice",
            ShapeKind::Approval => "approval",
            ShapeKind::Text => "text",
        }
    }
}

/// A place where a decision is made (`ask_point` in the Table).
#[derive(Debug, Clone, PartialEq)]
pub struct AskPoint {
    pub id: String,
    pub doc: String,
    pub shape: ShapeKind,
    pub impact: Impact,
    /// The most sensitive class of data its question carries: what egress checks before a remote answerer sees it.
    pub data: DataClass,
    pub chain: Vec<Step>,
    pub fallback: Fallback,
    /// The id of a Rank spec (`rank` in the Table) for `Fallback::Rank` and the `rank` step, when it has its own.
    pub rank: Option<String>,
    /// Whether its steps count toward each answerer's breaker, which every point shares. A point that is only a nicety (`false`) keeps
    /// its failures to itself, so it can never switch an answerer off for the points that matter.
    pub breaker: bool,
}

/// A question as an answerer sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Question {
    /// The question's own task (the Ask).
    pub ask: String,
    pub point: String,
    /// The task that asked.
    pub asker: String,
    pub thread: Option<String>,
    pub text: String,
    /// The shape, with the options offered (a model never reorders or adds to them).
    pub shape: AskShape,
    /// What the asker was asked to do.
    pub goal: String,
    /// For an approval: the effect it is for.
    pub act: Option<Action>,
}

/// What each option of each point means, in one line (`says` of the ask point): point → option id → line.
pub type Says = std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>;

/// `shape` for `point`, with the options that carry only their id (`flow`) given the line the point says they mean. An id alone tells
/// a decider nothing; a label an asker wrote itself, as the doers' are, stays.
pub fn described(shape: AskShape, says: &Says, point: &str) -> AskShape {
    match (shape, says.get(point)) {
        (AskShape::Choice { mut options }, Some(lines)) => {
            for o in options.iter_mut().filter(|o| o.label == o.id) {
                if let Some(line) = lines.get(&o.id) {
                    o.label = line.clone();
                }
            }
            AskShape::Choice { options }
        }
        (shape, _) => shape,
    }
}

/// What an answerer says.
#[derive(Debug, Clone, PartialEq)]
pub enum Offer {
    Answer { answer: Answer, conf: f32, why: String },
    /// Not answered, and why; the walk goes on.
    Pass { why: String },
    /// It tried and failed (a timeout, an error); counts toward the breaker.
    Failed { why: String },
}

/// What one call to a decider or a model cost, as it said (the list cost; an API answerer has no plan window).
/// `usd` is `None` when the answerer did not report a price: that is unknown, never nothing. The tokens are what it reported, and
/// 0 where it reported none.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Paid {
    pub usd: Option<f64>,
    pub tokens_in: u64,
    pub tokens_out: u64,
}

/// Something that may answer. `offer` may block for as long as the answerer needs; the engine calls it off its own thread.
pub trait Answerer: Send + Sync {
    /// The id the chain names it by: `rule:route`, `rank`, `decider:jev`.
    fn id(&self) -> &str;
    fn kind(&self) -> Kind;
    fn offer(&self, q: &Question) -> Offer;
    /// [`Answerer::offer`], and what the call cost when the answerer says. A decider or a model overrides this and makes `offer`
    /// the first of the pair; for the rest, which cost nothing, it is `None`.
    fn offer_paid(&self, q: &Question) -> (Offer, Option<Paid>) {
        (self.offer(q), None)
    }
    /// What is behind it, as a probe's pass is kept for it: the model and its version. Empty for what is data (a rule, a rank),
    /// which needs no probe and has no rate.
    fn model(&self) -> String {
        String::new()
    }
    /// Where a question goes when it is put: the provider's zone (`local`, `trusted`, `any`), which the egress Gate checks
    /// against the class of the data. `None` for an answerer that keeps the question on this machine.
    fn zone(&self) -> Option<String> {
        None
    }
    /// Whether it can be asked now. A decider or a model that reads its key at each question has none until one is typed (or after it
    /// is removed): the registry then hides it, as if it were not installed, and shows it again once the key is there.
    fn ready(&self) -> bool {
        true
    }
}

/// A code rule of a function of the question: `Ok((answer, why))` answers, sure of itself; `Err(why)` passes.
pub struct FnRule<F>(&'static str, F);

pub fn fn_rule<F: Fn(&Question) -> Result<(Answer, String), String> + Send + Sync>(id: &'static str, f: F) -> FnRule<F> {
    FnRule(id, f)
}

impl<F: Fn(&Question) -> Result<(Answer, String), String> + Send + Sync> Answerer for FnRule<F> {
    fn id(&self) -> &str {
        self.0
    }

    fn kind(&self) -> Kind {
        Kind::Rule
    }

    fn offer(&self, q: &Question) -> Offer {
        match (self.1)(q) {
            Ok((answer, why)) => Offer::Answer { answer, conf: 1.0, why },
            Err(why) => Offer::Pass { why },
        }
    }
}

/// What a rule says when it picks the option `id`.
pub fn picked(id: impl Into<String>, why: impl Into<String>) -> Result<(Answer, String), String> {
    Ok((Answer::Pick { id: id.into() }, why.into()))
}

/// The value of the line `key: value` of a question's text (the lines a probe's slices add, the digest of a reflex): the facts a rule reads, not its sentence.
pub fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix(": "))
}

/// The egress check an answerer with a zone is put through before it is called.
pub trait Egress: Send + Sync {
    fn check(&self, class: DataClass, zone: &str) -> crate::gate::Verdict;
}

/// How far an answerer is trusted now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum State {
    Active,
    /// Its probes have not passed, or its labelled wrong rate is high: it is skipped on the live path.
    Shadow { why: String },
    /// The breaker is open: skipped until this time (milliseconds since 1970).
    Off { until_ms: u64 },
}

/// What the chain reads of the answerers' state.
pub trait StateView: Send + Sync {
    fn state(&self, answerer: &str, now_ms: u64) -> State;

    /// The state for one ask point and the model behind the answerer now: the breaker's, and also whether its probes for that
    /// point have passed for that model and whether its labelled answers for that point are wrong too often.
    /// An answerer with no model is data and has only the first.
    fn state_for(&self, answerer: &str, _point: &str, _model: &str, now_ms: u64) -> State {
        self.state(answerer, now_ms)
    }
}
