//! Task: all work, recursively. A chat turn, a lane, an orchestrated member and every Ask is a
//! task, and a task is in exactly one of six states; nothing else ends it.
//!
//! ```text
//!   Queued ─► Running ─► Waiting{ask} ─► Running …          Cooling{until} (a limit waited out)
//!                │                             │
//!                └──────────────┬──────────────┘
//!                        Done{ok}  |  Stopped{why}
//! ```
//!
//! - [`TaskSpec`] says what to do and under what limits; it is the data of the `task.queued` event, so a task is rebuilt
//!   from the Log (the [`Book`] is the fold) and an older binary can read a newer spec.
//! - [`Tasks`] is the loop: one thread that starts a doer in a place, turns what the doer says into events, and enforces
//!   what no pack may remove — the hard spend cap, the wall, the open-ask rules, "did nothing is a failure" (law 9).
//! - Every stop that can be carried on has a [`menu`]: what the operator may do next, ending with `abort`, at most one
//!   recommended. Carrying on is a new task that starts the doer on the same session ([`Tasks::resume`]).

mod asks;
mod book;
pub mod context;
mod engine;
mod ran;
pub mod spot;

pub use asks::{Asks, FULL, GATE, GateAnswerer, NOT_ALLOWED};
pub use book::{Book, KEEP, Row, model_name, model_word, who};
pub use engine::{CONTINUE, Config, Launcher, Tasks, WeakTasks};

use crate::ask::{Answer, AskShape, AskSpec};
use crate::gate::grant::Scope;
use crate::place::Settled;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

/// A task's name: `t` and a number, made by the loop (`t1`, `t2` …), so it is safe as a file name.
pub fn id_number(id: &str) -> Option<u64> {
    let digits = id.strip_prefix('t')?;
    (!digits.is_empty() && digits.len() <= 18 && digits.bytes().all(|b| b.is_ascii_digit()) && !digits.starts_with('0'))
        .then(|| digits.parse().ok())
        .flatten()
}

/// Dollars as millionths, the unit money is kept in; what is not a plain amount is nothing.
pub fn micros(usd: f64) -> i64 {
    if usd.is_finite() && usd > 0.0 { (usd * 1e6).round() as i64 } else { 0 }
}

/// A doer's session, so a later task can carry on in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRef {
    pub doer: String,
    pub id: String,
}

/// Whether the work may change things: a read task is never checkpointed, and the Gate refuses its writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Read,
    #[default]
    Write,
}

/// The settings a run can be changed to while it goes on (`Ctl::Retune`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Tune {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The host that serves the model, when one was elected: part of the thread's binding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default)]
    pub mode: Mode,
}

/// The bounds of a task. The loop enforces the hard spend cap and the wall; the soft cap is written once and a rule parks the
/// task; `followups` and `visits` are for the flow runner (the follow-ups of a task are the setting `loop.followups`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Limits {
    /// Dollars. Past it a `task.cap_reached` event is written once; past [`HARD_FACTOR`] times it the task is killed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usd_soft: Option<f64>,
    /// Milliseconds from the start: the task stops itself, with its report, when they have passed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub followups: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visits: Option<u32>,
}

/// The paths a task changed, as its settling event and its row keep them: the first [`PATHS_KEPT`] of each kind, and how
/// many more there were.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Changed {
    pub created: Vec<String>,
    pub modified: Vec<String>,
    pub deleted: Vec<String>,
    pub not_restorable: Vec<String>,
    pub more: u64,
    /// The lists are empty because the place could not look (no checkpoint), not because nothing changed; written only when it is so.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub unknown: bool,
}

pub const PATHS_KEPT: usize = 200;

impl Changed {
    pub fn of(s: &Settled) -> Changed {
        let mut more = 0;
        let mut keep = |v: &[String]| {
            more += v.len().saturating_sub(PATHS_KEPT) as u64;
            v.iter().take(PATHS_KEPT).cloned().collect::<Vec<_>>()
        };
        let (created, modified, deleted, not_restorable) =
            (keep(&s.created), keep(&s.modified), keep(&s.deleted), keep(&s.not_restorable));
        Changed { created, modified, deleted, not_restorable, more, unknown: s.unknown }
    }

    pub fn is_empty(&self) -> bool {
        self.created.is_empty() && self.modified.is_empty() && self.deleted.is_empty() && self.more == 0
    }
}

/// The hard spend cap is this many times the soft one.
pub const HARD_FACTOR: f64 = 1.5;

/// The doer that is the operator, for the child task an Ask is.
pub const HUMAN: &str = "human";

fn here() -> String {
    "here".into()
}

/// What to do, by whom, where, and under which limits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskSpec {
    pub goal: String,
    /// The task this one is part of (an Ask's asker); it must be running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// The task this one carries on from (its session, its thread).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follows: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    /// A doer's id (`claude`), `human`, or `flow:<id>` for a flow.
    pub doer: String,
    /// What the task is for (`plan`, `work`, `review`, `validate`): what the standing of its doer is counted by.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// What the task is given besides its goal (an Ask's question lives here).
    #[serde(default, skip_serializing_if = "Json::is_null")]
    pub input: Json,
    /// The place's name; `here` works in the project's own folder.
    #[serde(default = "here")]
    pub place: String,
    #[serde(default)]
    pub limits: Limits,
    #[serde(default)]
    pub tune: Tune,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<SessionRef>,
    /// The account it runs as: its login, its config folder and its proxy are the child's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// Have the session shrunk before the doer works in it again, keeping what the loop knows must stay and these notes
    /// besides. Only where a doer can, and only at the start of a task that carries on in a session: a boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact: Option<Vec<String>>,
    /// Absolute paths the task may read that a wall would hide (a step's `reads` that start with `/`): a box binds them read-only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reads: Vec<String>,
}

/// The role of a task that only shrinks the session it follows (`:compact`): it ends when the shrinking does, and says nothing of its own.
pub const COMPACT_ROLE: &str = "compact";

impl TaskSpec {
    /// Whether the task is a shrinking and nothing else.
    pub fn shrinks_only(&self) -> bool {
        self.role.as_deref() == Some(COMPACT_ROLE)
    }

    pub fn new(goal: &str, doer: &str) -> TaskSpec {
        TaskSpec {
            goal: goal.to_string(),
            parent: None,
            follows: None,
            thread: None,
            doer: doer.to_string(),
            role: None,
            input: Json::Null,
            place: here(),
            limits: Limits::default(),
            tune: Tune::default(),
            resume: None,
            account: None,
            compact: None,
            reads: Vec::new(),
        }
    }

    pub fn check(&self) -> Result<(), String> {
        let word = |s: &str| !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control);
        if self.goal.trim().is_empty() || self.goal.len() > 1 << 20 {
            return Err("a task needs a goal of up to 1 MiB".into());
        }
        if !word(&self.doer) || !word(&self.place) {
            return Err("a task names its doer and its place".into());
        }
        if self.thread.as_deref().is_some_and(|t| !word(t)) || self.account.as_deref().is_some_and(|a| !word(a)) {
            return Err("the thread's and the account's names are one short line".into());
        }
        for id in [&self.parent, &self.follows].into_iter().flatten() {
            if id_number(id).is_none() {
                return Err(format!("`{id}` is not a task"));
            }
        }
        if self.limits.wall_ms == Some(0) || self.limits.usd_soft.is_some_and(|u| !(u.is_finite() && u > 0.0)) {
            return Err("a wall or a spend limit is above zero".into());
        }
        Ok(())
    }

    /// The question of an Ask task.
    pub fn ask(&self) -> Option<AskSpec> {
        serde_json::from_value(self.input.get("ask")?.clone()).ok()
    }
}

/// Why a task stopped without finishing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Why {
    /// The operator stopped it; the doer's session is kept.
    Aborted,
    /// The operator killed it; the doer's process group is gone.
    Killed,
    /// It asked a question and nobody answered in time.
    Parked { ask: String },
    /// An Ask nobody answered in time.
    Deadline,
    /// Its `wall_ms` passed.
    Wall,
    /// It spent past the soft cap and a rule parked it.
    Capped,
    /// It spent past the hard cap.
    HardCap,
    Failed { message: String },
    /// The process ended while it ran.
    Interrupted,
    /// The task that asked it stopped.
    Orphaned,
}

impl Why {
    pub fn word(&self) -> &'static str {
        match self {
            Why::Aborted => "aborted",
            Why::Killed => "killed",
            Why::Parked { .. } => "parked",
            Why::Deadline => "deadline",
            Why::Wall => "wall",
            Why::Capped => "capped",
            Why::HardCap => "hard_cap",
            Why::Failed { .. } => "failed",
            Why::Interrupted => "interrupted",
            Why::Orphaned => "orphaned",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum State {
    Queued,
    Running,
    Cooling { until_ms: u64 },
    Waiting { ask: String },
    Done { ok: bool },
    Stopped { why: Why },
}

impl State {
    pub fn word(&self) -> &'static str {
        match self {
            State::Queued => "queued",
            State::Running => "running",
            State::Cooling { .. } => "cooling",
            State::Waiting { .. } => "waiting",
            State::Done { .. } => "done",
            State::Stopped { .. } => "stopped",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, State::Done { .. } | State::Stopped { .. })
    }
}

/// What a menu entry does when chosen. The caller carries it out: an answer goes to [`Tasks::ctl`], `Resume` and `Retry` to
/// [`Tasks::resume`], `Say` needs a line of the operator's own words first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    Answer(Answer),
    Say,
    Resume,
    Retry,
    Abort,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub label: String,
    pub act: Act,
    pub recommended: bool,
}

/// What the operator may do about a stop. Empty when there is nothing to decide (they did it themselves, or it was an Ask
/// that ran out of time); otherwise the entries end with `abort`, and at most one is recommended.
pub fn menu(why: &Why, ask: Option<&AskSpec>) -> Vec<Item> {
    let item = |id: &str, label: &str, act: Act, recommended: bool| Item { id: id.into(), label: label.into(), act, recommended };
    let mut items = match why {
        Why::Parked { .. } => match ask.map(|a| &a.shape) {
            Some(AskShape::YesNo) => vec![item("yes", "yes", Act::Answer(Answer::Yes), false), item("no", "no", Act::Answer(Answer::No), false)],
            Some(AskShape::Approval { .. }) => {
                vec![item("yes", "allow once", Act::Answer(Answer::Yes), false), item("no", "deny", Act::Answer(Answer::No), true)]
            }
            Some(AskShape::Choice { options }) => {
                let mut seen = false;
                let mut v: Vec<Item> = options
                    .iter()
                    .map(|o| {
                        let rec = o.recommended && !seen;
                        seen |= rec;
                        item(&format!("pick:{}", o.id), &o.label, Act::Answer(Answer::Pick { id: o.id.clone() }), rec)
                    })
                    .collect();
                v.push(item("say", "answer in your own words", Act::Say, false));
                v
            }
            Some(AskShape::Text) => vec![item("say", "answer in your own words", Act::Say, true)],
            None => vec![item("resume", "carry on", Act::Resume, true)],
        },
        Why::Failed { .. } => vec![item("retry", "try again", Act::Retry, true)],
        Why::Wall | Why::Interrupted => vec![item("resume", "carry on", Act::Resume, true)],
        Why::Capped | Why::HardCap => vec![item("resume", "carry on", Act::Resume, false)],
        Why::Aborted | Why::Killed | Why::Deadline | Why::Orphaned => return Vec::new(),
    };
    items.push(item("abort", "leave it stopped", Act::Abort, false));
    items
}

/// What can be told to a task from outside. Shrinking a session is not among them: it happens at the
/// start of a task that carries on in one (`TaskSpec::compact`), which is a boundary, and a rule asks for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ctl {
    /// Words to a running doer.
    Steer(String),
    /// The answer to an Ask task (the task is the Ask).
    Answer(Answer),
    /// Yes to an approval, and remember it at this scope. Only the operator.
    Grant(Scope),
    /// Stop it and keep the doer's session.
    Abort,
    /// Stop it and kill the doer's process group.
    Kill,
    /// Change model, effort or mode: the run is stopped and started again on its session.
    Retune(Tune),
    /// A task that waits out a limit goes on as another login (`None`: the doer's own), in a new session; `why` is said in the Log.
    Move { account: Option<String>, why: String },
}
