//! Which host serves a model: the front asks, once, when a thread elects its doer, and keeps the answer in the
//! binding. What knows the hosts — the model rows, what each host cost and how fast it was, which are cooling — is in
//! `j4n-connect`; the front sees only this.
//!
//! What an election weighs: the cost of the task's own mix of tokens at each host, the price of waiting for it (λ, from who
//! waits now), the host's own record for this kind of work, and whether it holds the floor of the role. The record is read from
//! the tasks fold ([`BookRecord`]): a task is for a role, ran on a model at a host, and ended well or not.

use crate::task::{Book, Mode, State};

/// What the election is for.
#[derive(Debug, Clone, PartialEq)]
pub struct Want {
    /// `plan`, `work`, `review`, `validate` — what a host's record is counted by, and the mix of tokens it is likely to need.
    pub role: String,
    /// A host with too little of a record gets tasks that only read first.
    pub mode: Mode,
    /// The operator is on this lane now (a line or a steer in the last minutes, an open question).
    pub waiting: bool,
    /// A flow or a task is held up by this result.
    pub blocked: bool,
    /// A deadline is near.
    pub tight: bool,
    /// A time price pinned for this task (dollars a minute); left out, the rule works one out.
    pub lambda: Option<f64>,
    /// The crew works for quality (`crew.goal`): the host with the best record for the role comes first, and cost and time only order hosts that are level.
    pub quality: bool,
}

impl Default for Want {
    fn default() -> Want {
        Want { role: "work".into(), mode: Mode::Write, waiting: false, blocked: false, tight: false, lambda: None, quality: false }
    }
}

/// What a host's own results for a role say of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// Too few results to say.
    Unknown,
    Good,
    /// Clearly below the best host of the model for this role: not offered.
    Poor,
}

/// One host as the election saw it.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub host: String,
    /// Dollars for the task's mix of tokens; `None`: the host is unpriced, and ranks last.
    pub cost: Option<f64>,
    /// Seconds for the same mix; `None`: no speed is known at all.
    pub secs: Option<f64>,
    /// The speed is J4n's own measurement; else it is the listed one, and shows with a `~`.
    pub measured: bool,
    pub standing: Standing,
    /// `cost + λ · minutes`: lower is better.
    pub score: Option<f64>,
}

impl Line {
    /// The time as it is shown: `12s`, `~12s` when it is the listed speed and not a measured one, `—` when nothing is known.
    pub fn speed(&self) -> String {
        match (self.secs, self.measured) {
            (Some(s), true) => format!("{s:.1}s"),
            (Some(s), false) => format!("~{s:.1}s"),
            (None, _) => crate::value::DASH.into(),
        }
    }
}

/// A host that was chosen, and the others that would do, in the order they would be tried.
#[derive(Debug, Clone, PartialEq)]
pub struct Pick {
    pub host: String,
    /// The model as the host is asked for it (the row's `name`).
    pub model: String,
    pub why: String,
    pub alts: Vec<String>,
    /// Every host that was offered, in the order of the pick.
    pub lines: Vec<Line>,
    /// Dollars a minute of waiting was worth.
    pub lambda: f64,
}

/// What each host has done, by role.
pub trait Record: Send + Sync {
    /// How many tasks of `role` that ran `model` at `host` ended well, and how many did not.
    fn results(&self, model: &str, host: &str, role: &str) -> (u32, u32);

    /// The usual tokens in and out of a task of `role`, when there have been enough to say.
    fn mix(&self, role: &str) -> Option<(u64, u64)>;

    /// How the tasks of `role` that ran `model` at `host` ended, oldest first (`true`: well); none by default.
    fn outcomes(&self, _model: &str, _host: &str, _role: &str) -> Vec<bool> {
        Vec::new()
    }
}

/// What a request of `usd` list cost takes of the plan that pays for `doer`, for the election to weigh.
pub trait PlanPrice: Send + Sync {
    /// `None`: the doer is on no plan, or the plan has no price; the list cost is what the election weighs.
    fn price(&self, doer: &str, usd: f64, now_ms: u64) -> Option<f64>;
}

/// No plan at all: the list cost.
pub struct NoPlan;

impl PlanPrice for NoPlan {
    fn price(&self, _: &str, _: f64, _: u64) -> Option<f64> {
        None
    }
}

/// No record at all: every host is unknown, and the mix is the role's default.
pub struct NoRecord;

impl Record for NoRecord {
    fn results(&self, _: &str, _: &str, _: &str) -> (u32, u32) {
        (0, 0)
    }

    fn mix(&self, _: &str) -> Option<(u64, u64)> {
        None
    }
}

/// The record read from the fold of tasks.
pub struct BookRecord(pub Book);

/// Tasks it takes to say what a role usually needs.
const MIX_AT_LEAST: usize = 3;

impl Record for BookRecord {
    fn results(&self, model: &str, host: &str, role: &str) -> (u32, u32) {
        self.0.rows().iter().filter(|r| r.spec.role.as_deref() == Some(role) && r.spec.tune.endpoint.as_deref() == Some(host) && r.spec.tune.model.as_deref() == Some(model)).fold((0, 0), |(ok, bad), r| match &r.state {
            State::Done { ok: true } => (ok + 1, bad),
            State::Done { ok: false } | State::Stopped { why: crate::task::Why::Failed { .. } } => (ok, bad + 1),
            _ => (ok, bad),
        })
    }

    fn outcomes(&self, model: &str, host: &str, role: &str) -> Vec<bool> {
        self.0
            .rows()
            .iter()
            .filter(|r| r.spec.role.as_deref() == Some(role) && r.spec.tune.endpoint.as_deref() == Some(host) && r.spec.tune.model.as_deref() == Some(model))
            .filter_map(|r| match &r.state {
                State::Done { ok } => Some(*ok),
                State::Stopped { why: crate::task::Why::Failed { .. } } => Some(false),
                _ => None,
            })
            .collect()
    }

    fn mix(&self, role: &str) -> Option<(u64, u64)> {
        let median = |mut v: Vec<u64>| {
            v.sort_unstable();
            v[v.len() / 2]
        };
        let rows: Vec<_> = self.0.rows().into_iter().filter(|r| r.spec.role.as_deref() == Some(role) && matches!(r.state, State::Done { ok: true }) && r.tokens_in > 0).collect();
        (rows.len() >= MIX_AT_LEAST).then(|| (median(rows.iter().map(|r| r.tokens_in).collect()), median(rows.iter().map(|r| r.tokens_out).collect())))
    }
}

pub trait Hosts: Send + Sync {
    /// The host for `model` of `doer` for a thread being bound, or a step being elected; `None`: the model's default endpoint (the
    /// switch `route.hosts` is off, nothing is known of the model, or no host passes the filters).
    fn elect(&self, doer: &str, model: Option<&str>, want: &Want, now_ms: u64) -> Option<Pick>;

    /// Why `host` is not to be used now (it is cooling after a burst of limits), if that is so.
    fn cooling(&self, doer: &str, model: Option<&str>, host: &str, now_ms: u64) -> Option<String>;

    /// Another host for the same model, chosen as `elect` does and not cooling, with what leaving the cache costs in words.
    fn another(&self, doer: &str, model: Option<&str>, thread: &str, from: &str, now_ms: u64) -> Option<(Pick, String)>;

    /// Take the settings as an edit left them: the numbers it keeps of them; by default it keeps none.
    fn tune(&self, _settings: &crate::setting::Settings) {}
}

/// No hosts known: every model is served where it is by default.
pub struct NoHosts;

impl Hosts for NoHosts {
    fn elect(&self, _: &str, _: Option<&str>, _: &Want, _: u64) -> Option<Pick> {
        None
    }

    fn cooling(&self, _: &str, _: Option<&str>, _: &str, _: u64) -> Option<String> {
        None
    }

    fn another(&self, _: &str, _: Option<&str>, _: &str, _: &str, _: u64) -> Option<(Pick, String)> {
        None
    }
}
