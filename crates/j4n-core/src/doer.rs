//! Doer: who does a task. The trait the loop knows; the connectors that implement it for a
//! program, a protocol or a model live in `j4n-connect`.
//!
//! A doer is started with a [`StartCx`] and answers with a stream of [`RunEv`] through its [`Emit`], from whatever threads
//! it likes, ending with exactly one [`RunEv::End`] or [`RunEv::Error`]. A run that goes away without either (a panic, a
//! dropped handle) is a failure, not a finish. [`Run::ctl`] must not block: it hands the order to the doer's own threads.

use crate::ask::{Answer, AskSpec};
use crate::place::{Launch, Site};
use crate::table::Problem;
use crate::task::{SessionRef, TaskSpec};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Assistant,
    /// The reasoning that came with a tool call, kept so the own agent's thread can be rebuilt.
    Reasoning,
}

/// One limit window as the provider's own stream reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Limit {
    pub window: String,
    /// How much of the window is used: a fraction, 0 to 1, that moves in steps of 0.01 (as claude reports it).
    pub utilization: f64,
    /// When the window resets, in seconds since 1970; a new value is a new window.
    pub resets_at: i64,
    pub status: String,
}

/// What a run spent since it last said: deltas, so a run started again on a session adds to the task.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Metrics {
    pub usd: f64,
    /// Every token the model read: fresh, read from the cache and written to it. The cache parts are in it, not beside it.
    pub tokens_in: u64,
    pub tokens_out: u64,
    /// The part of `tokens_in` read from the cache, and the part written to it; 0 for a doer that says them in `Served` (the model
    /// doers) or not at all.
    pub cached: u64,
    pub wrote: u64,
    /// What the last call of the turn read, when the doer says it (claude does: `tokens_in` adds up every call of a turn): the size the session
    /// holds when the turn ends; and what its first call read, which on a session's first turn is the floor (the prompt, the tools, the folder's
    /// memory) before any tool had answered. 0: not said, and `tokens_in` is taken for each.
    pub last: u64,
    pub first: u64,
}

/// What one call to a model cost and who served it: the endpoint folds read it back.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Served {
    /// The model that was asked for.
    pub model: String,
    /// The host that served it, when the gateway says.
    pub host: Option<String>,
    pub input: u64,
    pub cached: u64,
    pub wrote: u64,
    pub output: u64,
    pub ttft_ms: Option<u64>,
    pub ms: u64,
}

/// Whether `served` is another model than the one `asked` for. Names are compared by their words: every word of three letters or more of
/// the asked name (after the last `/`, so a gateway's vendor prefix does not count) must be one of the served name's, whatever order the
/// versions come in, so `haiku` is `claude-haiku-5-5` and `anthropic/claude-haiku-4.5` is `claude-4.5-haiku-20251001`, and `haiku` is not
/// `claude-sonnet-5-5`. Either name empty, or `default`, is no difference.
pub fn served_other(asked: &str, served: &str) -> bool {
    let words = |t: &str| t.rsplit('/').next().unwrap_or("").to_lowercase().split(|c: char| !c.is_alphabetic()).filter(|w| w.chars().count() >= 3).map(String::from).collect::<Vec<_>>();
    if asked.is_empty() || served.is_empty() || asked == "default" {
        return false;
    }
    let have = words(served);
    words(asked).iter().any(|w| !have.contains(w))
}

/// What a run says. Streamed text and reasoning are flashed and never written; a `Message` is what settled and is.
#[derive(Debug, Clone, PartialEq)]
pub enum RunEv {
    Text(String),
    Reasoning(String),
    Message { role: Role, text: String },
    /// A tool call (`end` false) or its result (`end` true).
    Tool { name: String, detail: String, end: bool },
    /// A tool call that has begun and is still being written (live only): `id` names the call among the message's, `target` is
    /// what it is about once that has parsed, `bytes` how much of its arguments there is. Each one replaces the one before; the
    /// `Tool` that follows replaces the last.
    Open { id: String, name: String, target: String, bytes: u64 },
    /// The output tokens the model has written so far in this call to it (live only); `Metrics` settles them.
    Tokens(u64),
    /// What the turn has used so far, in fresh-input-token equivalents at the provider's ratios (a cached read a tenth, a write a quarter more, an output
    /// five), for a doer whose stream prices a turn only when it ends (the claude CLI: `total_cost_usd` of the result frame, usage on each message).
    /// Live only: the engine prices it from what an earlier turn of the same model cost and holds the hard cap against it; `Metrics` settles the turn.
    Spend(f64),
    /// The reply being streamed starts again: the model's stream broke and the request was asked again, so what it had streamed is dropped.
    Restart,
    /// A command and how it ended, with the real exit code.
    Shell { cmd: String, out: String, exit: i32 },
    /// A cell of the `py` tool: `code`, `risk`, `ok`, `ms`, `out`. The Log keeps the code in a blob.
    Py(serde_json::Value),
    Ask(AskSpec),
    Metrics(Metrics),
    /// One call to a model: who served it, and the tokens it read, cached and wrote.
    Served(Served),
    /// Paths the doer says it changed.
    Changed(Vec<String>),
    Notice(String),
    /// A line the dialect has no meaning for (its `kind`, once for each kind in a run): the stream said something this J4n cannot read.
    Unread { dialect: String, kind: String },
    /// What a run of a connected program's output came to, line by line: `program`, `lines`, `read` (the lines the map made something of), `unread`, `ended`,
    /// `shape` (a digest of the paths of its JSON lines) and `drifted` (it spoke and none of it was understood).
    Health(serde_json::Value),
    /// The run failed. It ends the run.
    Error(String),
    /// The doer's id for the session, so a later task can carry on in it.
    Session(String),
    /// What the provider says of its limit windows (a subscription's five hours, its week): how much of each is used, 0 to 1.
    Limits(Vec<Limit>),
    /// How full the context window is, from 0 to 1, as of the last call the doer made.
    Window(f64),
    /// The doer's own to-do list changed: how many items are open, how many there are, and the words of the one in progress
    /// (empty when none is). The chat keeps one row for it and says it again in place.
    List { open: u32, total: u32, active: String },
    /// The run could not do what it is for, and says why: a check that found no tests to run, or no program to run them with. The task still
    /// ends well, and `task.settled` carries it as `not_run`, so that nothing reads the end as proof.
    NotRun(String),
    /// The run is over. `answer` is the report that settled last; text streamed before a tool call is narration.
    End { answer: Option<String>, todo_left: u32 },
}

/// What the loop can tell a live run.
#[derive(Debug, Clone, PartialEq)]
pub enum RunCtl {
    Steer(String),
    Answer { local: String, answer: Answer },
    /// Why the answer to `local` that follows is a no: the Gate's rule says. A doer that can tell its model hands the reason on, so a refusal
    /// is not just `not allowed`; one that cannot ignores it. Sent before the [`RunCtl::Answer`].
    Told { local: String, why: String },
    /// Stop, keeping the session so another run can carry on.
    Abort,
    /// Stop and take the process group down.
    Kill,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Probe {
    pub installed: bool,
    pub version: Option<String>,
    pub authed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Caps {
    /// A steer reaches the running doer. Without it a steer stops the run and starts it again on its session.
    pub live: bool,
    /// A new run can carry on in a session an earlier one reported.
    pub resume: bool,
    /// A run started with `compacting` shrinks the session instead of working in it.
    pub compact: bool,
    /// It takes a Brief with each line (`TaskSpec::input.brief`): the contract for the line, and what a flow wants said, which it
    /// puts beside its own instructions and does not keep as history.
    pub brief: bool,
    /// It reads a folder of skills of its own for a task (`TaskSpec::input.skills`): the skills J4n picked, which a doer with no Brief is given so.
    pub skills: bool,
    /// It takes a tool list for a task (`TaskSpec::input.tools`: `allow` and `deny`, tool names as the doer knows them) and offers no tool the list leaves out.
    pub tools: bool,
    /// It has no wall of its own that J4n can trust, and is put behind the box when there is one: a task in `here` runs in `box`, one in
    /// `worktree` in `worktree-box`. Where no box is described it runs as it was.
    pub walled: bool,
}

pub(crate) enum Signal {
    Ev(RunEv),
    Gone,
}

struct EmitInner {
    send: Box<dyn Fn(Signal) -> bool + Send + Sync>,
    ended: AtomicBool,
}

impl Drop for EmitInner {
    fn drop(&mut self) {
        if !self.ended.load(Ordering::SeqCst) {
            (self.send)(Signal::Gone);
        }
    }
}

/// The doer's way to speak. Clones share it; when the last is dropped without an `End` or an `Error` having been sent, the
/// loop is told the run vanished.
#[derive(Clone)]
pub struct Emit(Arc<EmitInner>);

impl Emit {
    pub(crate) fn new(send: impl Fn(Signal) -> bool + Send + Sync + 'static) -> Emit {
        Emit(Arc::new(EmitInner { send: Box::new(send), ended: AtomicBool::new(false) }))
    }

    /// An Emit that hands its events to a receiver: for a doer that runs another doer for its own purposes (the canary of the
    /// Connect flow). A run that vanishes without an end comes through as an `Error`, so the reader always sees one.
    pub fn channel() -> (Emit, std::sync::mpsc::Receiver<RunEv>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let emit = Emit::new(move |sig| match sig {
            Signal::Ev(ev) => tx.send(ev).is_ok(),
            Signal::Gone => tx.send(RunEv::Error("the run went away without an end".into())).is_ok(),
        });
        (emit, rx)
    }

    /// Say something. False when nobody listens any more (the run was stopped, or already ended): the doer should stop.
    pub fn send(&self, ev: RunEv) -> bool {
        if self.0.ended.load(Ordering::SeqCst) {
            return false;
        }
        let last = matches!(ev, RunEv::End { .. } | RunEv::Error(_));
        if last {
            self.0.ended.store(true, Ordering::SeqCst);
        }
        (self.0.send)(Signal::Ev(ev))
    }
}

/// What a doer is started with.
pub struct StartCx {
    pub task: String,
    pub spec: TaskSpec,
    /// Where it works.
    pub site: Site,
    /// The session to carry on in, when this is not the first run of the task.
    pub resume: Option<SessionRef>,
    /// Words to start with when carrying on (a steer, an answer); without them, the goal.
    pub text: Option<String>,
    /// This run is to shrink the session, and `text` is what the shrunk session must keep: the run ends with a note of what
    /// was done, as `End`'s answer. Only asked of a doer whose [`Caps::compact`] is set.
    pub compacting: bool,
    pub out: Emit,
    /// The temporary folder the task has of its own, which its programs are given as `TMPDIR`; none behind the box, whose `/tmp` is its own.
    pub tmp: Option<std::path::PathBuf>,
    pub(crate) wrap: Arc<dyn Fn(Launch) -> Launch + Send + Sync>,
}

impl StartCx {
    /// The start of a run a doer makes for its own purposes: the same task, place, walls and folder, another goal, another way to
    /// speak. A run started so is the doer's to read and to answer for.
    pub fn nested(&self, goal: &str, out: Emit) -> StartCx {
        let mut spec = self.spec.clone();
        spec.goal = goal.to_string();
        StartCx { task: self.task.clone(), spec, site: self.site.clone(), resume: None, text: None, compacting: false, out, tmp: self.tmp.clone(), wrap: self.wrap.clone() }
    }

    /// A launch as the place wants it: confined, in its folder.
    pub fn wrap(&self, launch: Launch) -> Launch {
        (self.wrap)(launch)
    }

    /// The same start, with every launch of the doer changed by `f` first (the kit puts `j4n` on its PATH), then walled as before.
    pub fn also(mut self, f: impl Fn(Launch) -> Launch + Send + Sync + 'static) -> StartCx {
        let walls = self.wrap.clone();
        self.wrap = Arc::new(move |launch| walls(f(launch)));
        self
    }
}

pub trait Run: Send {
    fn ctl(&mut self, ctl: RunCtl) -> Result<(), Problem>;
}

pub trait Doer: Send + Sync {
    fn id(&self) -> &str;
    fn probe(&self) -> Probe;
    fn caps(&self) -> Caps;
    fn start(&self, cx: StartCx) -> Result<Box<dyn Run>, Problem>;
    /// The front end is going away: take down any child kept between turns.
    fn shutdown(&self) {}
    /// It runs code and no model (a flow, the code steps of a flow, `check`, `account`): the row of its task names no model for it, and a flow
    /// gives it no Brief, which is the contract of a line said to a model.
    fn code(&self) -> bool {
        false
    }
}
