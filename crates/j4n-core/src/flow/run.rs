//! The flow runner: a flow as a doer (`flow:<id>`). Its run walks the steps in the order the lint found, on a thread of its own,
//! and every step that does work is a **task of its own**, a child of the flow's: its place, its Gate, its checkpoint, its cost
//! and its questions are the loop's, as for any task. The flow itself only decides what comes next.
//!
//! - `do` renders the goal, finds who does it (a connector, `who:<role>` by an election, `flow:<id>`, `model:<connector>`), starts
//!   the child, and waits; what the child answered is the step's `answer` and its declared ports. A child that did not finish well
//!   stops the flow, with its reason.
//! - `ask` puts the question from the flow's own run (the same child continues: the flow waits for the answer); `yes`/`no` may go
//!   to another step, and `apply` brings the work a step kept to the project's folder on a yes.
//! - `branch` tests an expression. A step entered more than its `max_visits` takes `on_max`, or stops the flow.
//! - `map` runs a doer over a list, a few at a time. `end` is the answer; a flow that runs out of steps answers with the last one.
//! - A stop or a steer of the flow's task reaches the child that works.

use crate::text::clip;
use super::{Flow, Flows, Shape, StepKind, StepRow, DEPTH};
use crate::ask::{Answer, AskShape, AskSpec, Opt};
use crate::brief::{self, Brief};
use crate::schema::DataClass;
use crate::change::Actor;
use crate::doer::{Caps, Doer, Emit, Probe, Run, RunCtl, RunEv, StartCx};
use crate::eval::Scope;
use crate::expr::Expr;
use crate::fmt::Fmt;
use crate::gate::{Action, Verb};
use crate::lanes::Lanes;
use crate::log::{Log, New};
use crate::report::Report;
use crate::table::Problem;
use crate::task::{Ctl, Mode, Row, State, TaskSpec, Tasks, WeakTasks, Why};
use crate::value::Value;
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// How many members of a `map` work at once, and how many items a `map` takes.
pub const MAP_AT_ONCE: usize = 4;
/// The setting that says how many items of a `map` run at once (default [`MAP_AT_ONCE`]).
pub const MAP_WIDTH: &str = "flow.map_width";
const MAP_ITEMS: usize = 20;
const POLL: Duration = Duration::from_millis(250);
/// The window a step's Brief is cut to (tokens); the contract is never cut.
const BRIEF_WINDOW: usize = 128_000;
/// The most characters of a question a flow raises.
const ASK_CHARS: usize = 3800;

/// Who was elected for a role, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Elected {
    pub doer: String,
    pub model: Option<String>,
    pub endpoint: Option<String>,
    pub why: String,
}

/// What a flow's run needs of the loop and of whoever elects.
pub trait Host: Send + Sync {
    fn spawn(&self, spec: TaskSpec) -> Result<String, String>;
    /// The task's row once it is over, waiting up to `limit`; `None` when it is not over yet.
    fn wait(&self, task: &str, limit: Duration) -> Option<Row>;
    fn row(&self, task: &str) -> Option<Row>;
    fn ctl(&self, task: &str, ctl: Ctl);
    /// Bring the work a task kept in a place of its own to the project's folder: the words that say what was done.
    fn take(&self, task: &str) -> Result<String, String>;
    /// Who does `role`, for `text`, excluding these doers.
    fn elect(&self, role: &str, text: &str, thread: Option<&str>, exclude: &[String], mode: Mode) -> Result<Elected, String>;
    fn event(&self, task: &str, doer: &str, kind: &str, data: Json);
    /// What a finished child answered, as a checked report, written to the Log for the flow to read.
    fn report(&self, task: &str, answer: &str) -> Report;
    /// Whether the doer runs code and no model: it is given no Brief.
    fn code(&self, _doer: &str) -> bool {
        false
    }
    /// How many items of a `map` run at once (the setting `flow.map_width`).
    fn map_width(&self) -> usize {
        MAP_AT_ONCE
    }
}

/// Who elects: the front's election, as a flow needs it.
pub trait Elect: Send + Sync {
    fn elect(&self, role: &str, text: &str, thread: Option<&str>, exclude: &[String], mode: Mode) -> Result<Elected, String>;
}

/// The loop, as a flow's host: it holds the loop weakly, because the loop holds the flow's doer.
pub struct TaskHost {
    pub tasks: WeakTasks,
    pub who: Arc<dyn Elect>,
    pub log: Log,
    /// The fold `lanes` (what the branch ran), and the project's folder (where a claim is looked up unless the branch kept its work).
    pub lanes: Lanes,
    pub root: PathBuf,
    /// The settings, for `flow.map_width`.
    pub settings: crate::setting::Live,
}

impl TaskHost {
    fn tasks(&self) -> Result<Tasks, String> {
        self.tasks.upgrade().ok_or_else(|| "the loop is gone".to_string())
    }
}

impl Host for TaskHost {
    fn code(&self, doer: &str) -> bool {
        self.tasks().is_ok_and(|t| t.code(doer))
    }

    fn map_width(&self) -> usize {
        (self.settings.num(MAP_WIDTH, MAP_AT_ONCE as f64) as usize).clamp(1, 16)
    }

    fn spawn(&self, spec: TaskSpec) -> Result<String, String> {
        let by = Actor::Doer(spec.parent.as_ref().and_then(|p| self.tasks().ok().and_then(|t| t.book().get(p)).map(|r| r.spec.doer)).unwrap_or_else(|| "flow".into()));
        self.tasks()?.spawn(spec, by).map_err(|p| p.msg)
    }

    fn wait(&self, task: &str, limit: Duration) -> Option<Row> {
        self.tasks().ok()?.book().wait(task, |r| r.state.is_terminal(), limit)
    }

    fn row(&self, task: &str) -> Option<Row> {
        self.tasks().ok()?.book().get(task)
    }

    fn ctl(&self, task: &str, ctl: Ctl) {
        if let Ok(t) = self.tasks() {
            let _ = t.ctl(task, ctl, Actor::Core);
        }
    }

    fn take(&self, task: &str) -> Result<String, String> {
        use crate::place::{Take, Taken};
        let tasks = self.tasks()?;
        // a task that mended the work of another worked in its place: the work is kept under the first
        let mut task = task.to_string();
        for _ in 0..8 {
            match tasks.book().get(&task).and_then(|r| r.spec.place.strip_prefix('@').map(String::from)) {
                Some(origin) => task = origin,
                None => break,
            }
        }
        match tasks.take(&task, Take::Apply).map_err(|p| p.msg)? {
            Taken::Nothing => Ok("there was nothing to bring over".into()),
            Taken::Clean(work, notes) => Ok(format!("brought over {}{}", work.counts(), if notes.is_empty() { String::new() } else { format!(" ({})", notes.join("; ")) })),
            Taken::Blocked { why, .. } => Err(format!("it does not apply: {why}")),
        }
    }

    fn elect(&self, role: &str, text: &str, thread: Option<&str>, exclude: &[String], mode: Mode) -> Result<Elected, String> {
        self.who.elect(role, text, thread, exclude, mode)
    }

    fn event(&self, task: &str, doer: &str, kind: &str, data: Json) {
        let _ = self.log.append(New::of(kind, Actor::Doer(doer.into()), data).task(task));
    }

    fn report(&self, task: &str, answer: &str) -> Report {
        match self.tasks().ok().and_then(|t| t.book().get(task)) {
            Some(row) => self.lanes.settle(&self.log, &row, &self.root, answer, false),
            None => Report::plain(answer, "the task is gone"),
        }
    }
}

/// Where a flow doer finds its host: set once the loop is up, because the loop is made of the doers.
#[derive(Clone, Default)]
pub struct Link(Arc<OnceLock<Arc<dyn Host>>>);

impl Link {
    pub fn set(&self, host: Arc<dyn Host>) {
        let _ = self.0.set(host);
    }
}

/// The doers of the flows, one for each, `flow:<id>`.
pub fn doers(flows: &Flows, link: &Link) -> Vec<Arc<dyn Doer>> {
    flows.all().iter().map(|f| Arc::new(FlowDoer { id: format!("flow:{}", f.id()), flow: Arc::new(f.clone()), link: link.clone() }) as Arc<dyn Doer>).collect()
}

pub struct FlowDoer {
    id: String,
    flow: Arc<Flow>,
    link: Link,
}

impl Doer for FlowDoer {
    fn id(&self) -> &str {
        &self.id
    }

    fn probe(&self) -> Probe {
        Probe { installed: true, version: None, authed: true, detail: String::new() }
    }

    fn caps(&self) -> Caps {
        Caps { live: true, resume: false, compact: false, ..Caps::default() }
    }

    fn code(&self) -> bool {
        true
    }

    fn start(&self, cx: StartCx) -> Result<Box<dyn Run>, Problem> {
        let host = self.link.0.get().cloned().ok_or_else(|| Problem::refused(&self.id, "flows are not wired to the loop"))?;
        let depth = cx.spec.input.pointer("/flow/depth").and_then(Json::as_u64).unwrap_or(0) as u32;
        // the least of the chain counts
        let cap = self.flow.row.depth.unwrap_or(DEPTH).min(cx.spec.input.pointer("/flow/cap").and_then(Json::as_u64).map_or(u32::MAX, |c| c as u32));
        if depth >= cap {
            return Err(Problem::refused(&self.id, format!("flows are running flows {depth} deep: that is as deep as the chain allows ({cap})")));
        }
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let ins = {
            let mut m = cx.spec.input.get("in").and_then(Json::as_object).cloned().unwrap_or_default();
            if self.flow.row.input.contains_key("goal") && !m.contains_key("goal") {
                m.insert("goal".into(), Json::String(cx.spec.goal.clone()));
            }
            Json::Object(m)
        };
        let go = Go {
            flow: self.flow.clone(),
            host,
            task: cx.task.clone(),
            me: self.id.clone(),
            thread: cx.spec.thread.clone(),
            mode: cx.spec.tune.mode,
            place: cx.spec.place.clone(),
            depth,
            cap,
            ins,
            out: cx.out.clone(),
            rx: Mutex::new(rx),
            stop: stop.clone(),
            child: Mutex::new(None),
            outs: Mutex::new(BTreeMap::new()),
            visits: Mutex::new(BTreeMap::new()),
        };
        std::thread::spawn(move || go.run());
        Ok(Box::new(FlowRun { tx, stop }))
    }
}

struct FlowRun {
    tx: Sender<RunCtl>,
    stop: Arc<AtomicBool>,
}

impl Run for FlowRun {
    fn ctl(&mut self, ctl: RunCtl) -> Result<(), Problem> {
        if matches!(ctl, RunCtl::Abort | RunCtl::Kill) {
            self.stop.store(true, Ordering::SeqCst);
        }
        self.tx.send(ctl).map_err(|_| Problem::refused("flow", "the run is over"))
    }
}

/// What a step came to.
#[derive(Debug, Clone, Default)]
struct StepOut {
    ports: BTreeMap<String, Json>,
    doer: String,
    task: String,
    answer: String,
    ok: bool,
    place: Json,
    skipped: bool,
}

/// Where the walk goes after a step.
enum Next {
    On,
    To(usize),
    End(String),
}

struct Go {
    flow: Arc<Flow>,
    host: Arc<dyn Host>,
    task: String,
    me: String,
    thread: Option<String>,
    mode: Mode,
    place: String,
    depth: u32,
    cap: u32,
    ins: Json,
    out: Emit,
    rx: Mutex<Receiver<RunCtl>>,
    stop: Arc<AtomicBool>,
    /// The child that works now, so a steer or a stop reaches it.
    child: Mutex<Option<String>>,
    outs: Mutex<BTreeMap<String, StepOut>>,
    visits: Mutex<BTreeMap<String, u32>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The last JSON object of a text that holds a `pass`, else a line that starts with PASS or FAIL.
fn verdict(text: &str) -> Json {
    let mut found = None;
    for (i, _) in text.match_indices('{') {
        if let Some(end) = text[i..].rfind('}')
            && let Ok(v) = serde_json::from_str::<Json>(&text[i..=i + end])
            && v.get("pass").and_then(Json::as_bool).is_some()
        {
            found = Some(v);
        }
    }
    if let Some(v) = found {
        return json!({ "pass": v["pass"], "concerns": v.get("concerns").cloned().unwrap_or_else(|| json!([])) });
    }
    for line in text.lines().rev() {
        let l = line.trim();
        let up = l.to_uppercase();
        if up.starts_with("PASS") || up.starts_with("FAIL") {
            let rest = l[4..].trim_start_matches([':', '-', ' ']).trim();
            return json!({ "pass": up.starts_with("PASS"), "concerns": if rest.is_empty() { json!([]) } else { json!([rest]) } });
        }
    }
    json!({ "pass": false, "concerns": ["the answer holds no verdict (a JSON object with `pass`, or a line PASS or FAIL)"] })
}

fn stopped_words(row: &Row) -> String {
    match &row.state {
        State::Done { ok: false } => row.reason.clone().unwrap_or_else(|| "it did not finish".into()),
        State::Stopped { why: Why::Failed { message } } => message.clone(),
        State::Stopped { why } => match &row.reason {
            Some(r) => format!("{}: {r}", why.word()),
            None => why.word().to_string(),
        },
        other => other.word().to_string(),
    }
}

impl Go {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    fn fail(&self, msg: String) {
        self.out.send(RunEv::Error(msg));
    }

    fn event(&self, kind: &str, data: Json) {
        self.host.event(&self.task, &self.me, kind, data);
    }

    fn scope(&self, item: Option<&Value>) -> Scope {
        let steps: BTreeMap<String, Value> = lock(&self.outs)
            .iter()
            .map(|(id, o)| {
                let ports = o.ports.iter().map(|(k, v)| (k.clone(), Value::from_json(v))).collect();
                let m = BTreeMap::from([
                    ("out".to_string(), Value::Map(ports)),
                    ("doer".to_string(), Value::text(&o.doer)),
                    ("task".to_string(), Value::text(&o.task)),
                    ("answer".to_string(), Value::text(&o.answer)),
                    ("ok".to_string(), Value::Bool(o.ok)),
                    ("skipped".to_string(), Value::Bool(o.skipped)),
                    ("place".to_string(), Value::from_json(&o.place)),
                ]);
                (id.clone(), Value::Map(m))
            })
            .collect();
        let mut scope = Scope::new().with("in", Value::from_json(&self.ins)).with("steps", Value::Map(steps));
        if let Some(i) = item {
            scope = scope.with("item", i.clone());
        }
        scope
    }

    fn render(&self, template: &str, item: Option<&Value>) -> String {
        Fmt::parse(template).map_or_else(|_| template.to_string(), |f| f.render(&self.scope(item)))
    }

    fn holds(&self, expr: &str, item: Option<&Value>) -> bool {
        Expr::parse(expr).is_ok_and(|e| e.holds(&self.scope(item)))
    }

    /// Steers go to the child that works; a stop is the run's `stop`, set when it was ordered.
    fn listen(&self) {
        let rx = lock(&self.rx);
        while let Ok(c) = rx.try_recv() {
            if let (RunCtl::Steer(t), Some(child)) = (c, lock(&self.child).clone()) {
                self.host.ctl(&child, Ctl::Steer(t));
            }
        }
    }

    fn run(self) {
        self.event("flow.started", json!({ "flow": self.flow.id(), "depth": self.depth }));
        let flow = self.flow.clone();
        let mut pos = 0;
        let mut last = String::new();
        while pos < flow.order.len() {
            if self.stopped() {
                return;
            }
            let i = flow.order[pos];
            let s = &flow.row.steps[i];
            if s.when.as_deref().is_some_and(|w| !self.holds(w, None)) {
                self.event("flow.step.skipped", json!({ "step": s.id }));
                pos += 1;
                continue;
            }
            let visits = {
                let mut v = lock(&self.visits);
                let n = v.entry(s.id.clone()).or_insert(0);
                *n += 1;
                *n
            };
            self.event("flow.step.started", json!({ "step": s.id, "kind": format!("{:?}", s.kind()).to_lowercase(), "visits": visits }));
            let next = match s.kind() {
                StepKind::Do => match self.do_step(s, None) {
                    Err(Stop::Skip(why)) => {
                        self.event("flow.step.skipped", json!({ "step": s.id, "why": why }));
                        self.keep(s, StepOut { skipped: true, ..StepOut::default() });
                        Ok(Next::On)
                    }
                    other => other.map(|o| {
                        last = o.answer.clone();
                        self.keep(s, o);
                        Next::On
                    }),
                },
                StepKind::Ask => self.ask_step(s),
                StepKind::Branch => Ok(Next::To(self.place_of(if self.holds(s.on.as_deref().unwrap_or_default(), None) { s.yes.as_deref() } else { s.no.as_deref() }.unwrap_or_default()))),
                StepKind::Map => self.map_step(s).map(|o| {
                    last = o.answer.clone();
                    self.keep(s, o);
                    Next::On
                }),
                StepKind::End => Ok(Next::End(s.answer.as_deref().map_or_else(|| last.clone(), |a| self.render(a, None)))),
            };
            match next {
                Err(Stop::Gone) => return,
                Err(Stop::Failed(m)) => return self.fail(m),
                Err(Stop::Skip(m)) => return self.fail(format!("flow step `{}` cannot be skipped here: {m}", s.id)),
                Ok(Next::On) => pos += 1,
                Ok(Next::To(p)) => {
                    // a step may go back while it has been entered fewer times than `max_visits`; at the last it may not
                    match s.max_visits.filter(|m| p <= pos && visits >= *m) {
                        None => pos = p,
                        Some(max) => match s.on_max.as_deref() {
                            Some("fail") | None => return self.fail(format!("flow step `{}` went back {} times and it is still not right; that is as often as it may", s.id, max - 1)),
                            Some(to) => pos = self.place_of(to),
                        },
                    }
                }
                Ok(Next::End(answer)) => return self.finish(answer),
            }
        }
        self.finish(last);
    }

    fn finish(&self, answer: String) {
        self.event("flow.ended", json!({ "flow": self.flow.id() }));
        self.out.send(RunEv::End { answer: Some(answer), todo_left: 0 });
    }

    /// Where the walk is when it is at the step `id`.
    fn place_of(&self, id: &str) -> usize {
        self.flow.step(id).and_then(|i| self.flow.order.iter().position(|o| *o == i)).unwrap_or(self.flow.order.len())
    }

    fn keep(&self, s: &StepRow, o: StepOut) {
        self.event("flow.step.ended", json!({ "step": s.id, "task": o.task, "doer": o.doer, "ok": o.ok }));
        lock(&self.outs).insert(s.id.clone(), o);
    }

    /// Who does a `do`: the doer, the model, the endpoint, and what to say about the choice.
    fn who(&self, s: &StepRow, goal: &str, item: Option<&Value>) -> Result<Who, Stop> {
        let reference = s.doer.as_deref().map(|d| if d.contains('{') { self.render(d, item) } else { d.to_string() }).unwrap_or_default();
        let model = s.model.clone();
        match reference.split_once(':') {
            Some(("who", role)) => {
                let exclude: Vec<String> = s.exclude.iter().map(|e| self.render(e, item)).filter(|e| !e.is_empty() && e != crate::value::DASH).collect();
                let mode = s.mode.unwrap_or(self.mode);
                let e = self.host.elect(role, goal, self.thread.as_deref(), &exclude, mode).map_err(|m| if s.optional { Stop::Skip(m) } else { Stop::Failed(format!("flow step `{}`: nobody can be elected for `{role}`: {m}", s.id)) })?;
                self.event("flow.elected", json!({ "step": s.id, "role": role, "doer": e.doer, "model": e.model, "endpoint": e.endpoint, "why": e.why }));
                // a model the step names itself wins over the pin and over the election's, and the host elected for another model does not go with it
                let endpoint = if model.as_ref().is_some_and(|m| e.model.as_ref() != Some(m)) { None } else { e.endpoint };
                Ok(Who { doer: e.doer, model: model.or(e.model), endpoint, role: Some(role.to_string()) })
            }
            Some(("model", connector)) => Ok(Who { doer: connector.to_string(), model, endpoint: None, role: s.role.clone() }),
            _ if reference == "checks" => Ok(Who { doer: "check".into(), model: None, endpoint: None, role: s.role.clone() }),
            _ if reference.is_empty() || reference == crate::value::DASH => Err(Stop::Failed(format!("flow step `{}`: there is nobody to do it (`{}` gave nothing)", s.id, s.doer.as_deref().unwrap_or_default()))),
            _ => Ok(Who { doer: reference, model, endpoint: None, role: s.role.clone() }),
        }
    }

    fn do_step(&self, s: &StepRow, item: Option<&Value>) -> Result<StepOut, Stop> {
        let again = lock(&self.visits).get(&s.id).copied().unwrap_or(1) > 1;
        let goal = self.render(s.redo.as_deref().filter(|_| again).or(s.goal.as_deref()).unwrap_or_default(), item);
        let who = self.who(s, &goal, item)?;
        self.run_child(s, &goal, who, item)
    }

    fn run_child(&self, s: &StepRow, goal: &str, who: Who, item: Option<&Value>) -> Result<StepOut, Stop> {
        let Who { doer, model, endpoint, role } = who;
        let mut spec = TaskSpec::new(goal, &doer);
        spec.parent = Some(self.task.clone());
        spec.thread = self.thread.clone();
        spec.role = role;
        spec.tune.mode = s.mode.unwrap_or(self.mode);
        spec.tune.model = model;
        spec.tune.endpoint = endpoint;
        spec.limits.visits = s.turns;
        spec.limits.usd_soft = s.usd;
        // the paths a step lists in `reads` (the others name the kinds that other steps write) are what a box binds read-only
        spec.reads = s.reads.iter().filter(|r| r.starts_with('/')).cloned().collect();
        // a held step that is entered again carries on in the place it was in: the work is mended, it is not done over
        let before = lock(&self.outs).get(&s.id).map(|o| o.task.clone()).filter(|t| s.hold && !t.is_empty());
        spec.place = match s.place.as_deref() {
            _ if before.is_some() => format!("@{}", before.unwrap_or_default()),
            None => self.place.clone(),
            Some(p) => match p.strip_prefix('@') {
                Some(step) => {
                    let task = lock(&self.outs).get(step).map(|o| o.task.clone()).filter(|t| !t.is_empty());
                    format!("@{}", task.ok_or_else(|| Stop::Failed(format!("flow step `{}`: its place `{p}` is where step `{step}` worked, and that step has not run", s.id)))?)
                }
                None => p.to_string(),
            },
        };
        let mut input = serde_json::Map::new();
        if s.hold {
            input.insert("hold".into(), json!(true));
        }
        if !doer.starts_with("flow:") && !self.host.code(&doer) {
            // a doer that takes a Brief is told what kind of line this is, and that a step that only reads may not change anything
            let contract = brief::contract(goal, false, spec.tune.mode == Mode::Read);
            input.insert("brief".into(), json!(Brief::new().with("contract", DataClass::Public, &contract).render(BRIEF_WINDOW).text));
        }
        // what a step is given besides its goal: a doer that takes structured words (the Connect flow's) finds them in `with`
        if !doer.starts_with("flow:") && !s.with.is_empty() {
            let with: serde_json::Map<String, Json> = s.with.iter().map(|(k, t)| (k.clone(), Json::String(self.render(t, item)))).collect();
            input.insert("with".into(), Json::Object(with));
        }
        if !input.is_empty() {
            spec.input = Json::Object(input);
        }
        if doer.starts_with("flow:") {
            let with: serde_json::Map<String, Json> = s.with.iter().map(|(k, t)| (k.clone(), Json::String(self.render(t, item)))).collect();
            let with = if with.is_empty() { json!({ "goal": goal }) } else { Json::Object(with) };
            spec.input = json!({ "in": with, "flow": { "depth": self.depth + 1, "cap": self.cap } });
        }
        let id = self.host.spawn(spec).map_err(|m| Stop::Failed(format!("flow step `{}` could not start {doer}: {m}", s.id)))?;
        *lock(&self.child) = Some(id.clone());
        self.event("flow.child", json!({ "step": s.id, "task": id, "doer": doer }));
        let row = loop {
            self.listen();
            if self.stopped() {
                self.host.ctl(&id, Ctl::Abort);
                return Err(Stop::Gone);
            }
            if let Some(r) = self.host.wait(&id, POLL) {
                break r;
            }
        };
        *lock(&self.child) = None;
        let finished = matches!(row.state, State::Done { ok: true });
        if !finished && !s.soft {
            return Err(Stop::Failed(format!("flow step `{}` ({doer}, {id}) did not finish: {}", s.id, stopped_words(&row))));
        }
        let answer = if finished { row.answer.clone().unwrap_or_default() } else { stopped_words(&row) };
        let place = match &row.kept {
            Some(k) => json!({ "dir": k.dir, "diff": k.work.counts(), "files": k.work.files, "ignored": k.work.ignored }),
            None => json!({ "dir": "", "diff": format!("{} created, {} modified, {} deleted", row.changed.created.len(), row.changed.modified.len(), row.changed.deleted.len()), "files": row.changed.created.iter().chain(&row.changed.modified).collect::<Vec<_>>() }),
        };
        let mut ports = BTreeMap::new();
        for (name, kind) in &s.out {
            let value = match kind.as_str() {
                "json" => serde_json::from_str(&answer).unwrap_or_else(|_| Json::String(answer.clone())),
                // a check says how it went by how it ended; any other doer says it in its answer
                "verdict" if doer == "check" => json!({ "pass": finished, "concerns": if finished { json!([]) } else { json!([answer]) } }),
                "verdict" => verdict(&answer),
                "report" => self.host.report(&id, &answer).json(),
                "diff" => place["diff"].clone(),
                "files" => place["files"].clone(),
                _ => Json::String(answer.clone()),
            };
            ports.insert(name.clone(), value);
        }
        Ok(StepOut { ports, doer, task: id, answer, ok: finished, place, skipped: false })
    }

    fn ask_step(&self, s: &StepRow) -> Result<Next, Stop> {
        let mut question = self.render(s.question.as_deref().unwrap_or_default(), None);
        if let Some(show) = &s.show {
            question = format!("{question}\n\n{}", self.render(show, None));
        }
        let change = s.change.as_deref().map(|c| self.render(c, None));
        if let Some(c) = &change {
            question = format!("{question}\n\n{c}");
        }
        let visits = lock(&self.visits).get(&s.id).copied().unwrap_or(1);
        let shape = match s.shape.unwrap_or(Shape::Text) {
            Shape::Yesno => AskShape::YesNo,
            Shape::Text => AskShape::Text,
            Shape::Choice => AskShape::Choice { options: s.options.iter().map(|o| Opt { id: o.clone(), label: o.clone(), recommended: false }).collect() },
            Shape::Approval => AskShape::Approval { action: clip(change.as_deref().unwrap_or(&question), 200), risk: 40 },
        };
        let act = (s.shape == Some(Shape::Approval)).then(|| {
            let mut a = Action::new(Verb::Apply, Actor::Doer(self.me.clone()), &clip(change.as_deref().unwrap_or(&s.id), 200));
            a.task = Some(self.task.clone());
            Box::new(a)
        });
        let local = format!("{}#{}-{visits}", self.task, s.id);
        self.out.send(RunEv::Ask(AskSpec { local: local.clone(), question: clip(&question, ASK_CHARS), shape, inband: true, point: None, act }));
        let answer = loop {
            if self.stopped() {
                return Err(Stop::Gone);
            }
            let got = lock(&self.rx).recv_timeout(POLL);
            match got {
                Ok(RunCtl::Answer { local: l, answer }) if l == local => break answer,
                Ok(RunCtl::Steer(t)) => {
                    if let Some(child) = lock(&self.child).clone() {
                        self.host.ctl(&child, Ctl::Steer(t));
                    }
                }
                Ok(RunCtl::Abort | RunCtl::Kill) | Err(mpsc::RecvTimeoutError::Disconnected) => return Err(Stop::Gone),
                _ => {}
            }
        };
        let yes = matches!(answer, Answer::Yes | Answer::Pick { .. } | Answer::Text { .. });
        let said = answer.render();
        let mut note = said.clone();
        if yes && let Some(from) = s.apply.as_deref() {
            let task = lock(&self.outs).get(from).map(|o| o.task.clone()).unwrap_or_default();
            note = self.host.take(&task).map_err(|m| Stop::Failed(format!("flow step `{}`: the work of `{from}` was not applied: {m}", s.id)))?;
        }
        let ports = BTreeMap::from([("answer".to_string(), json!(said)), ("yes".to_string(), json!(yes))]);
        self.keep(s, StepOut { ports, doer: "operator".into(), task: String::new(), answer: note, ok: yes, place: Json::Null, skipped: false });
        let target = if matches!(answer, Answer::No) { s.no.as_deref() } else { s.yes.as_deref() };
        Ok(match (target, answer) {
            (Some(t), _) => Next::To(self.place_of(t)),
            (None, Answer::No) => Next::End(format!("stopped at `{}`: the answer was no", s.id)),
            _ => Next::On,
        })
    }

    fn map_step(&self, s: &StepRow) -> Result<StepOut, Stop> {
        let items = match Expr::parse(s.over.as_deref().unwrap_or_default()).map(|e| e.eval(&self.scope(None))) {
            Ok(Value::List(l)) => l,
            _ => return Err(Stop::Failed(format!("flow step `{}`: `over` did not give a list", s.id))),
        };
        let items: Vec<Value> = items.into_iter().take(MAP_ITEMS).collect();
        let each = s.each.clone().unwrap_or_default();
        let mut results: Vec<Result<StepOut, Stop>> = Vec::new();
        for chunk in items.chunks(self.host.map_width()) {
            let found = std::thread::scope(|sc| {
                let jobs: Vec<_> = chunk
                    .iter()
                    .map(|item| {
                        sc.spawn(|| {
                            let goal = self.render(s.goal.as_deref().unwrap_or_default(), Some(item));
                            self.run_child(s, &goal, Who { doer: each.clone(), model: s.model.clone(), endpoint: None, role: s.role.clone() }, Some(item))
                        })
                    })
                    .collect();
                jobs.into_iter().map(|j| j.join().unwrap_or_else(|_| Err(Stop::Failed("a member of the map panicked".into())))).collect::<Vec<_>>()
            });
            results.extend(found);
        }
        // a map gives a list of reports, each checked, and not the answers joined as text
        let mut reports = Vec::new();
        for r in results {
            let out = r?;
            reports.push(self.host.report(&out.task, &out.answer).json());
        }
        let ports = BTreeMap::from([("results".to_string(), Json::Array(reports.clone()))]);
        Ok(StepOut { ports, doer: each, task: String::new(), answer: Json::Array(reports).to_string(), ok: true, place: Json::Null, skipped: false })
    }
}

/// Who does a step: the doer, and the model, the host and the role it was elected with.
struct Who {
    doer: String,
    model: Option<String>,
    endpoint: Option<String>,
    role: Option<String>,
}

/// Why a step did not go on.
enum Stop {
    /// The run was stopped: nothing more to say.
    Gone,
    /// Nobody can do an optional step: it is left out, and why.
    Skip(String),
    Failed(String),
}
