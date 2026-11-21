//! A model as a doer (`is = "model"`, dialects `openai` and `anthropic`): a conversation kept in this process, run turn by
//! turn until the model has no more calls to make.
//!
//! - A **session** is a conversation. Its id is announced, a later task carries on in it, and it is also the `session_id` the
//!   gateway keeps one host and its cache by. A restart forgets it, as it forgets a live child.
//! - Each turn is a [`step`]: the reply's words and reasoning are streamed, and what it cost and who served it are said
//!   ([`RunEv::Metrics`], [`RunEv::Served`]). The assistant message is kept with its reasoning and its calls.
//! - **Every call of a reply runs, in order**. A call whose arguments were not JSON is answered with why, and not run. A
//!   call the tools say must be asked about goes to the Gate as an approval first; a no is the call's result, and the model
//!   goes on. The tools are [`Tools`]: none here (a model that only talks), the builtin set for a model that works on its own.
//! - A steer is added to the conversation at the next turn; an abort ends the read at the next frame.
//! - A run fails with the words the failure table reads. The conversation keeps what was completed, so a task that goes on
//!   after a limit starts from there.

use super::offer;
use super::{Call, Client, Delta, Fail, Msg, Req, Tool, Wire, anthropic::Anthropic, openai::Openai, step};
use crate::net::Net;
use j4n_core::ask::{Answer, AskShape, AskSpec, Opt};
use j4n_core::change::Actor;
use j4n_core::doer::{Caps, Doer, Emit, Metrics, Probe, Role, Run, RunCtl, RunEv, Served, StartCx};
use j4n_core::gate::{Action, Gate, ReadOnly};
use j4n_core::place::{Launch, Site};
use j4n_core::table::Problem;
use j4n_core::task::{Mode, TaskSpec};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The most turns one run makes (steps are not the cap, spend is). A task may ask for fewer (`Limits::visits`: a lane that only
/// reads takes ten).
pub const TURNS_MAX: usize = 100;
/// What a model is told on its last turn, when it had no more to give: it answers from what it has, with no tools.
pub const FORCE: &str = "Step budget reached. Answer NOW from the evidence you gathered, in plain text, and say plainly what you could not verify.";
const DETAIL_MAX: usize = 200;
/// The waits before a model call that got no answer at all is asked again: twice, then the task fails.
pub const RIDE: [Duration; 2] = [Duration::from_secs(2), Duration::from_secs(6)];

/// Which wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Openai,
    Anthropic,
}

impl Dialect {
    pub fn wire(self) -> Box<dyn Wire> {
        match self {
            Dialect::Openai => Box::new(Openai),
            Dialect::Anthropic => Box::new(Anthropic),
        }
    }
}

/// A command a call ran, and how it ended: the Log keeps the real exit code and the end of its output.
#[derive(Debug, Clone, Default)]
pub struct ShellOut {
    pub cmd: String,
    pub out: String,
    pub exit: i32,
}

/// What a call came to.
#[derive(Debug, Clone, Default)]
pub struct ToolOut {
    pub text: String,
    pub error: bool,
    /// Paths it changed.
    pub changed: Vec<String>,
    pub shell: Option<ShellOut>,
    /// A cell of the `py` tool, for the Log (`RunEv::Py`).
    pub py: Option<serde_json::Value>,
}

/// What must be asked before a call runs.
pub struct Gated {
    pub action: Action,
    pub question: String,
    pub risk: u8,
}

/// The functions a model may call. Every turn offers the same set.
pub trait Tools: Send + Sync {
    fn specs(&self) -> Vec<Tool>;
    /// What the Gate is asked before `call` runs; `None` for a call that needs no question.
    fn gate(&self, call: &Call, task: &str, site: &Site, read_only: ReadOnly) -> Option<Gated>;
    /// Run the call in the task's folder; `wrap` puts a program behind the place's walls (a box).
    fn run(&self, call: &Call, site: &Site, wrap: &(dyn Fn(Launch) -> Launch + Send + Sync)) -> ToolOut;
    /// Where the words `call` brings back come from when they are not the operator's or the project's (a page, an MCP server's
    /// result, an endpoint's answer): the rest of the run is then held to the taint rules of the pack `guard`.
    fn untrusted(&self, _call: &Call) -> Option<String> {
        None
    }
}

/// No functions at all: the model talks.
pub struct NoTools;

impl Tools for NoTools {
    fn specs(&self) -> Vec<Tool> {
        Vec::new()
    }

    fn gate(&self, _: &Call, _: &str, _: &Site, _: ReadOnly) -> Option<Gated> {
        None
    }

    fn run(&self, call: &Call, _: &Site, _: &(dyn Fn(Launch) -> Launch + Send + Sync)) -> ToolOut {
        ToolOut { text: format!("there is no tool `{}`", call.name), error: true, ..ToolOut::default() }
    }
}

/// How the doer reaches its model.
#[derive(Clone)]
pub struct Config {
    pub id: String,
    pub dialect: Dialect,
    pub base: String,
    pub key: String,
    /// A local model needs no key.
    pub keyless: bool,
    pub net: Net,
    pub model: String,
    pub system: String,
    /// The window in tokens, to say how full it is.
    pub context: Option<u64>,
    pub effort: Option<String>,
}

#[derive(Default)]
struct Convo {
    msgs: Vec<Msg>,
}

pub struct ModelDoer {
    cfg: Config,
    tools: Arc<dyn Tools>,
    /// The Gate that is told what a run read from untrusted sources (the pack `guard`).
    taint: Option<Gate>,
    /// The function `ask` is on offer, and the loop puts its questions.
    asks: bool,
    sessions: Arc<Mutex<HashMap<String, Convo>>>,
    next: AtomicU64,
    /// The waits before a call that got no answer at all is asked again.
    backoff: Vec<Duration>,
    /// Where the key is read at each start; none: `cfg.key`, as it was made.
    fresh: Option<super::KeyFn>,
    /// Why there is no key it can use, when a place holds one that is refused (the task's error says it).
    refused: Option<WhyFn>,
}

/// Why a connector's key is not used: the reader's words for a key that is there and refused, none when there is simply none.
pub type WhyFn = Arc<dyn Fn() -> Option<String> + Send + Sync>;

impl ModelDoer {
    pub fn new(cfg: Config) -> ModelDoer {
        ModelDoer { cfg, tools: Arc::new(NoTools), taint: None, asks: false, sessions: Arc::default(), next: AtomicU64::new(1), backoff: RIDE.to_vec(), fresh: None, refused: None }
    }

    /// Read the key from `fresh` at each probe and each start, so a key typed or removed while J4n runs acts on the next task.
    pub fn keyed(mut self, fresh: Option<super::KeyFn>) -> ModelDoer {
        self.fresh = fresh;
        self
    }

    /// Say why no key is used when one is there and refused.
    pub fn refusing(mut self, why: WhyFn) -> ModelDoer {
        self.refused = Some(why);
        self
    }

    fn key(&self) -> String {
        self.fresh.as_ref().map_or_else(|| self.cfg.key.clone(), |f| f())
    }

    /// Other waits than [`RIDE`] before a call that got no answer is asked again (none: it is not).
    pub fn riding(mut self, backoff: Vec<Duration>) -> ModelDoer {
        self.backoff = backoff;
        self
    }

    pub fn with_tools(mut self, tools: Arc<dyn Tools>) -> ModelDoer {
        self.tools = tools;
        self
    }

    /// A run that reads from an untrusted source tells `gate`, which holds the rest of that run to the taint rules.
    pub fn guarded(mut self, gate: Gate) -> ModelDoer {
        self.taint = Some(gate);
        self
    }

    /// The model may put a closed question to the operator (`ask`, [`offer::AskTool`]).
    pub fn asking(mut self) -> ModelDoer {
        self.asks = true;
        self
    }
}

impl Doer for ModelDoer {
    fn id(&self) -> &str {
        &self.cfg.id
    }

    fn probe(&self) -> Probe {
        let authed = self.cfg.keyless || !self.key().is_empty();
        Probe { installed: true, version: None, authed, detail: if authed { String::new() } else { "no key".into() } }
    }

    fn caps(&self) -> Caps {
        Caps { live: true, resume: true, brief: true, tools: true, ..Caps::default() }
    }

    fn start(&self, cx: StartCx) -> Result<Box<dyn Run>, Problem> {
        let key = self.key();
        if !self.cfg.keyless && key.is_empty() {
            let why = self.refused.as_ref().and_then(|f| f()).map_or_else(|| "there is no key for it".to_string(), |why| format!("its key is not used: {why}"));
            return Err(Problem::refused(&self.cfg.id, format!("{why} · Library › Providers › {}: ⏎ types it", self.cfg.id)));
        }
        let model = cx.spec.tune.model.clone().unwrap_or_else(|| self.cfg.model.clone());
        if model.is_empty() {
            return Err(Problem::refused(&self.cfg.id, "no model is named"));
        }
        let resume = cx.resume.as_ref().filter(|r| r.doer == self.cfg.id).map(|r| r.id.clone());
        let (id, fresh) = match resume.filter(|id| self.sessions.lock().is_ok_and(|s| s.contains_key(id))) {
            Some(id) => (id, false),
            None => (format!("m{}-{}", std::process::id(), self.next.fetch_add(1, Ordering::SeqCst)), true),
        };
        let lost = cx.resume.is_some() && fresh;
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let cx = Arc::new(cx);
        let walls = cx.clone();
        let run = Turns {
            wrap: Arc::new(move |launch| walls.wrap(launch)),
            cfg: Config { key, ..self.cfg.clone() },
            tools: self.tools.clone(),
            limit: tool_limit(&cx.spec),
            taint: self.taint.clone().map(|g| Taint { gate: g, task: cx.task.clone() }),
            asks: self.asks,
            sessions: self.sessions.clone(),
            id,
            model,
            cx_task: cx.task.clone(),
            text: cx.text.clone().unwrap_or_else(|| cx.spec.goal.clone()),
            read_only: if cx.spec.tune.mode == Mode::Read { ReadOnly::Explicit } else { ReadOnly::No },
            effort: cx.spec.tune.effort.clone().or_else(|| self.cfg.effort.clone()),
            host: cx.spec.tune.endpoint.clone(),
            site: cx.site.clone(),
            out: cx.out.clone(),
            rx,
            stop: stop.clone(),
            lost,
            backoff: self.backoff.clone(),
            cap: cx.spec.limits.visits.map_or(TURNS_MAX, |v| (v as usize).clamp(1, TURNS_MAX)),
            brief: cx.spec.input.get("brief").and_then(|b| b.as_str()).unwrap_or_default().to_string(),
        };
        std::thread::spawn(move || run.go());
        Ok(Box::new(ModelRun { tx, stop }))
    }
}

struct ModelRun {
    tx: Sender<RunCtl>,
    stop: Arc<AtomicBool>,
}

impl Run for ModelRun {
    fn ctl(&mut self, ctl: RunCtl) -> Result<(), Problem> {
        if matches!(ctl, RunCtl::Abort | RunCtl::Kill) {
            self.stop.store(true, Ordering::SeqCst);
        }
        self.tx.send(ctl).map_err(|_| Problem::refused("model", "the run is over"))
    }
}

/// What a run tells the Gate it read; when the run is over the Gate forgets it.
struct Taint {
    gate: Gate,
    task: String,
}

impl Drop for Taint {
    fn drop(&mut self) {
        self.gate.untaint(&self.task);
    }
}

/// The tool list of a task as `(allow, deny)`: the names in `input.tools`, none when it has no list.
fn tool_limit(spec: &TaskSpec) -> (Vec<String>, Vec<String>) {
    let list = |k: &str| -> Vec<String> { spec.input.get("tools").and_then(|t| t.get(k)).and_then(|a| a.as_array()).into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect() };
    (list("allow"), list("deny"))
}

struct Turns {
    cfg: Config,
    tools: Arc<dyn Tools>,
    /// The tool list of this task (`input.tools`): `(allow, deny)`; the tools offered are these and a call to another is refused.
    limit: (Vec<String>, Vec<String>),
    taint: Option<Taint>,
    asks: bool,
    sessions: Arc<Mutex<HashMap<String, Convo>>>,
    id: String,
    model: String,
    cx_task: String,
    text: String,
    read_only: ReadOnly,
    effort: Option<String>,
    host: Option<String>,
    site: Site,
    wrap: Arc<dyn Fn(Launch) -> Launch + Send + Sync>,
    out: Emit,
    rx: Receiver<RunCtl>,
    stop: Arc<AtomicBool>,
    lost: bool,
    /// The most turns this run takes.
    cap: usize,
    backoff: Vec<Duration>,
    /// What the front says of this line, beside the system prompt; it is not part of the conversation.
    brief: String,
}

fn clip(s: &str, n: usize) -> String {
    let line: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    if line.chars().count() > n { format!("{}…", line.chars().take(n).collect::<String>()) } else { line }
}

impl Turns {
    fn convo<R>(&self, f: impl FnOnce(&mut Convo) -> R) -> R {
        let mut all = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        f(all.entry(self.id.clone()).or_default())
    }

    fn push(&self, m: Msg) {
        self.convo(|c| c.msgs.push(m));
    }

    /// Steers that arrived, as words for the conversation. False when a stop has come.
    fn drain(&self, steers: &mut Vec<String>) -> bool {
        while let Ok(c) = self.rx.try_recv() {
            match c {
                RunCtl::Abort | RunCtl::Kill => return false,
                RunCtl::Steer(t) => steers.push(t),
                RunCtl::Answer { .. } | RunCtl::Told { .. } => {}
            }
        }
        !self.stop.load(Ordering::SeqCst)
    }

    /// Put a question to the loop and wait for its answer; `None` when the run is stopped.
    fn ask(&self, spec: AskSpec, steers: &mut Vec<String>) -> Option<Answer> {
        let local = spec.local.clone();
        self.out.send(RunEv::Ask(spec));
        loop {
            match self.rx.recv() {
                Ok(RunCtl::Answer { local: l, answer }) if l == local => return Some(answer),
                Ok(RunCtl::Steer(t)) => steers.push(t),
                Ok(RunCtl::Abort | RunCtl::Kill) | Err(_) => return None,
                Ok(RunCtl::Answer { .. } | RunCtl::Told { .. }) => {}
            }
        }
    }

    fn go(self) {
        let notices = self.out.clone();
        let client = Client::new(self.cfg.dialect.wire(), self.cfg.net.clone(), &self.cfg.base, &self.cfg.key).riding(&self.backoff, move |w| {
            let _ = notices.send(RunEv::Notice(w));
        });
        self.out.send(RunEv::Session(self.id.clone()));
        if self.lost {
            self.out.send(RunEv::Notice("the conversation was gone: this one starts again from the goal".into()));
        }
        // a task carried on after a failure finds its last words still the last: they are not said twice
        let said = self.convo(|c| matches!(c.msgs.last(), Some(Msg::User(t)) if *t == self.text));
        if !said {
            self.push(Msg::User(self.text.clone()));
        }
        let specs: Vec<Tool> = self.tools.specs().into_iter().filter(|t| self.offered(&t.name)).collect();
        let system = if self.brief.is_empty() { self.cfg.system.clone() } else { format!("{}\n\n{}", self.cfg.system, self.brief) };
        let mut steers = Vec::new();
        // the last reply's words and calls, to see one that says the same again: its reasoning is not part of it
        let mut last: Option<(String, Vec<(String, String)>)> = None;
        for turn in 0..self.cap {
            if !self.drain(&mut steers) {
                return;
            }
            for s in steers.drain(..) {
                self.push(Msg::User(s));
            }
            // the last turn is for the answer: what has been gathered is all there is
            let last_turn = turn + 1 == self.cap && self.cap > 1;
            if last_turn {
                self.push(Msg::User(FORCE.into()));
            }
            let mut convo = self.convo(|c| c.msgs.clone());
            let (before, sent) = (convo.len(), convo.clone());
            let req = Req { system: &system, tools: if last_turn { &[] } else { &specs }, effort: self.effort.as_deref(), host: self.host.as_deref(), session: Some(&self.id), ..Req::new(&self.model, &sent) };
            let out = self.out.clone();
            let reply = step(&client, &req, &mut convo, &mut |d| {
                out.send(match d {
                    Delta::Text(t) => RunEv::Text(t),
                    Delta::Reasoning(t) => RunEv::Reasoning(t),
                    Delta::Call { id, name, target, bytes } => RunEv::Open { id, name, target, bytes },
                    Delta::Tokens(n) => RunEv::Tokens(n),
                    Delta::Restart => RunEv::Restart,
                });
            }, &self.stop);
            let reply = match reply {
                Ok(r) => r,
                Err(Fail::Cancelled) => return,
                Err(f) => {
                    self.out.send(RunEv::Error(f.words(j4n_core::clock::Clock::now_ms(&j4n_core::clock::SystemClock))));
                    return;
                }
            };
            // what the model was told to get its answer is kept
            self.convo(|c| c.msgs.extend(convo[before..].iter().cloned()));
            self.out.send(RunEv::Metrics(Metrics { usd: reply.usage.usd.unwrap_or(0.0), tokens_in: reply.usage.input, tokens_out: reply.usage.output, ..Metrics::default() }));
            self.out.send(RunEv::Served(Served {
                model: self.model.clone(),
                host: reply.served.clone(),
                input: reply.usage.input,
                cached: reply.usage.cached,
                wrote: reply.usage.wrote,
                output: reply.usage.output,
                ttft_ms: reply.ttft_ms,
                ms: reply.ms,
            }));
            if let Some(w) = self.cfg.context.filter(|c| *c > 0) {
                self.out.send(RunEv::Window((reply.usage.input as f64 / w as f64).min(1.0)));
            }
            self.push(Msg::Assistant { text: reply.text.clone(), think: reply.think.clone(), calls: reply.calls.clone() });
            if reply.calls.is_empty() {
                self.out.send(RunEv::Message { role: Role::Assistant, text: reply.text.clone() });
                self.out.send(RunEv::End { answer: Some(reply.text), todo_left: 0 });
                return;
            }
            let said = (reply.text.clone(), reply.calls.iter().map(|c| (c.name.clone(), c.args.to_string())).collect::<Vec<_>>());
            if last.as_ref() == Some(&said) {
                self.out.send(RunEv::Error("the model sent the same reply twice in a row, so it is not getting anywhere".into()));
                return;
            }
            last = Some(said);
            // what it thought before it called is kept with the call, so the thread can be rebuilt from the Log
            let thought: String = reply.think.iter().map(|t| t.text.as_str()).collect();
            if !thought.trim().is_empty() {
                self.out.send(RunEv::Message { role: Role::Reasoning, text: thought });
            }
            for call in &reply.calls {
                let Some(done) = self.call(call, &mut steers) else { return };
                self.push(Msg::Result { call: call.id.clone(), name: call.name.clone(), text: done.text, error: done.error });
            }
            if turn + 1 == self.cap {
                self.out.send(RunEv::Error(format!("the model went {} turns without finishing", self.cap)));
            }
        }
    }

    /// A call of `ask`: the question goes to the screen as a choice (never an approval) and the answer is the call's result.
    fn question(&self, call: &Call, steers: &mut Vec<String>) -> Option<ToolOut> {
        let (q, options) = match offer::question(call) {
            Ok(x) => x,
            Err(why) => return Some(ToolOut { text: why, error: true, ..ToolOut::default() }),
        };
        let opts = options.iter().enumerate().map(|(i, o)| Opt { id: (i + 1).to_string(), label: o.clone(), recommended: false }).collect();
        let spec = AskSpec { local: format!("{}#{}", self.id, call.id), question: clip(&q, 400), shape: AskShape::Choice { options: opts }, inband: true, point: None, act: None };
        let text = match self.ask(spec, steers)? {
            Answer::Pick { id } => id.parse::<usize>().ok().and_then(|n| options.get(n.wrapping_sub(1))).cloned().unwrap_or(id),
            Answer::Text { text } => text,
            other => other.render(),
        };
        Some(ToolOut { text: format!("the operator answered: {text}"), ..ToolOut::default() })
    }

    /// Whether the task's tool list leaves `name` in: not denied, and allowed when the list names any.
    fn offered(&self, name: &str) -> bool {
        let (allow, deny) = &self.limit;
        !deny.iter().any(|d| d == name) && (allow.is_empty() || allow.iter().any(|a| a == name))
    }

    /// One call: refused, asked about, run. `None` when the run was stopped on the way.
    fn call(&self, call: &Call, steers: &mut Vec<String>) -> Option<ToolOut> {
        let detail = clip(&call.args.to_string(), DETAIL_MAX);
        self.out.send(RunEv::Tool { name: call.name.clone(), detail: detail.clone(), end: false });
        let done = if let Some(why) = &call.bad {
            ToolOut { text: format!("`{}` was not run: {why}", call.name), error: true, ..ToolOut::default() }
        } else if !self.offered(&call.name) {
            ToolOut { text: format!("there is no tool `{}` for this task", call.name), error: true, ..ToolOut::default() }
        } else if self.asks && call.name == offer::AskTool::NAME {
            self.question(call, steers)?
        } else {
            let gated = self.tools.gate(call, &self.cx_task, &self.site, self.read_only);
            let yes = match gated {
                None => true,
                Some(g) => {
                    let spec = AskSpec {
                        local: format!("{}#{}", self.id, call.id),
                        question: clip(&g.question, 400),
                        shape: AskShape::Approval { action: clip(&g.question, 200), risk: g.risk },
                        inband: true,
                        point: None,
                        act: Some(Box::new(g.action)),
                    };
                    matches!(self.ask(spec, steers)?, Answer::Yes)
                }
            };
            if yes {
                let done = self.tools.run(call, &self.site, &*self.wrap);
                // what came back from a page or a server is not the operator's: from here on the run is held to the taint rules
                if let (Some(t), Some(from)) = (&self.taint, self.tools.untrusted(call))
                    && t.gate.taint(&t.task, &from)
                {
                    self.out.send(RunEv::Notice(format!("this run has read from {from}, which is not the operator's: it may not send anything out or write to a sensitive path until the next line")));
                }
                done
            } else {
                ToolOut { text: format!("`{}` was not allowed. Do something else, or say what you need.", call.name), error: true, ..ToolOut::default() }
            }
        };
        if let Some(s) = &done.shell {
            self.out.send(RunEv::Shell { cmd: s.cmd.clone(), out: s.out.clone(), exit: s.exit });
        }
        if let Some(cell) = &done.py {
            self.out.send(RunEv::Py(cell.clone()));
            if let Some(m) = crate::kithost::spend(cell) {
                self.out.send(RunEv::Metrics(m));
            }
        }
        if !done.changed.is_empty() {
            self.out.send(RunEv::Changed(done.changed.clone()));
        }
        self.out.send(RunEv::Tool { name: call.name.clone(), detail: clip(&done.text, DETAIL_MAX), end: true });
        Some(done)
    }
}

/// The action a tool of a doer asks the Gate about.
pub fn action(doer: &str, verb: j4n_core::gate::Verb, target: &str, task: &str, site: &Site, read_only: ReadOnly) -> Action {
    let mut a = Action::new(verb, Actor::Doer(doer.into()), target).task(task);
    a.site = Some(site.dir.display().to_string());
    a.read_only = read_only;
    a
}
