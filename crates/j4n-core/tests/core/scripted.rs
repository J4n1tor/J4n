//! What the task tests share: a doer that plays a script (an echo adapter), and a rig — a project folder, a
//! home, a Log on a clock the test moves, the fold host, the timer thread and the loop. The doer's script is what a real
//! doer would say; the doer's record is what the loop did to it.

use crate::world::{Scratch, open_log};
use j4n_core::ask::{Answer, AskShape, AskSpec, Opt};
use j4n_core::change::Actor;
use j4n_core::clock::ManualClock;
use j4n_core::doer::{Caps, Doer, Emit, Metrics, Probe, Role, Run, RunCtl, RunEv, StartCx};
use j4n_core::failure::Failures;
use j4n_core::fold::{Folds, declared};
use j4n_core::log::{Ev, Log, Scope};
use j4n_core::place::Launch;
use j4n_core::rule::Rules;
use j4n_core::setting::Settings;
use j4n_core::table::{Layer, LayerKind, Problem, SourceFile, Tables};
use j4n_core::task::{Config, Row, Tasks, TaskSpec};
use j4n_core::timer::Timers;
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum Beat {
    Say(String),
    Delta(String),
    Think(String),
    /// A call being written: `(id, name, target, bytes)`.
    Open(&'static str, &'static str, &'static str, u64),
    /// The output tokens so far.
    Tokens(u64),
    Reasoned(String),
    Tool(String, String),
    ToolEnd(String, String),
    Shell(String, String, i32),
    Changed(Vec<String>),
    Notice(String),
    /// A frame the dialect cannot read: `(dialect, kind)`.
    Unread(&'static str, &'static str),
    Spend(f64),
    /// What the turn in hand has used so far, in fresh-input-token equivalents: live, before the turn's cost is reported.
    Running(f64),
    /// A request that read through the cache: list cost, tokens in (all of them), the part read from the cache and the part written.
    Cached(f64, u64, u64, u64),
    /// A turn of several calls: list cost, tokens in (every call added up), what the first call read and what the last call read.
    Called(f64, u64, u64, u64),
    /// A call that a model served: `(model, host)`.
    Served(&'static str, &'static str),
    /// A report that spends nothing.
    Idle,
    Session(String),
    /// How full the context is.
    Window(f64),
    /// What the provider says of its limit windows: `(name, used, resets_at)`.
    Limits(Vec<(&'static str, f64, i64)>),
    /// A window that says `rejected` and when it resets (seconds since 1970): the provider has said not now.
    Shut(&'static str, i64),
    Ask(AskSpec),
    /// Wait for an answer (a steer in the meantime is recorded and the wait goes on).
    Hold,
    /// Wait for an abort or a kill.
    Forever,
    /// Do nothing for this many milliseconds (orders that arrive are recorded).
    Pause(u64),
    /// The doer's own list: `(open, total, the item in hand)`.
    List(u32, u32, &'static str),
    /// Write a file in the folder the task works in.
    Put(String, String),
    Remove(String),
    Fail(String),
    End(Option<String>, u32),
    /// The run could not do its job (`RunEv::NotRun`): no evidence either way.
    NotRun(String),
    /// Panic on the doer's thread.
    Panic,
    /// Return without saying the run is over.
    Vanish,
}

pub fn end(answer: &str) -> Beat {
    Beat::End(Some(answer.to_string()), 0)
}

pub fn nothing() -> Beat {
    Beat::End(None, 0)
}

pub fn say(text: &str) -> Beat {
    Beat::Say(text.to_string())
}

#[derive(Debug, Clone)]
pub struct Started {
    pub task: String,
    pub goal: String,
    pub resume: Option<String>,
    pub text: Option<String>,
    pub compacting: bool,
    pub dir: PathBuf,
    /// The login it was started as (none: the doer's own).
    pub account: Option<String>,
}

#[derive(Default)]
pub struct Record {
    pub starts: Vec<Started>,
    pub ctls: Vec<RunCtl>,
    /// The way a leaking doer speaks after its run is over.
    pub emit: Option<Emit>,
    /// What each start made of its launch once the place had wrapped it.
    pub launches: Vec<Launch>,
}

pub struct Scripted {
    id: String,
    caps: Caps,
    turns: Vec<Vec<Beat>>,
    refuse: Option<String>,
    leak: bool,
    panics: bool,
    hostile: bool,
    probe: Probe,
    record: Arc<Mutex<Record>>,
    /// The turns of each login of a doer that is run as several: a start plays the next turn of the account it is for, and its last again.
    routes: Arc<Mutex<BTreeMap<String, Played>>>,
    /// What to play, from the folder a start is given and its spec: `None` plays the next turn.
    judge: Option<Judge>,
}

/// A doer's turn made at its start, from where it is and what it was given.
pub type Judge = Arc<dyn Fn(&std::path::Path, &TaskSpec) -> Option<Vec<Beat>> + Send + Sync>;

/// The turns of one login and how many were played.
type Played = (Vec<Vec<Beat>>, usize);

fn ready() -> Probe {
    Probe { installed: true, version: Some("scripted".into()), authed: true, detail: "plays a script".into() }
}

impl Scripted {
    /// A live, resumable doer: every start plays the next turn (the last one again, when the turns run out).
    pub fn new(id: &str, turns: Vec<Vec<Beat>>) -> Arc<Scripted> {
        Arc::new(Scripted { id: id.into(), caps: Caps { live: true, resume: true, ..Caps::default() }, turns, refuse: None, leak: false, panics: false, hostile: false, probe: ready(), record: Arc::default(), routes: Arc::default(), judge: None })
    }

    /// A live, resumable doer that plays what `judge` makes of each start, and its turns where that is `None`.
    pub fn judging(id: &str, judge: Judge, turns: Vec<Vec<Beat>>) -> Arc<Scripted> {
        let mut d = Scripted::new(id, turns);
        Arc::get_mut(&mut d).expect("just made").judge = Some(judge);
        d
    }

    /// A live, resumable doer run as several logins: `(account, turns)`, the account empty for the doer's own login.
    pub fn by_account(id: &str, logins: Vec<(&str, Vec<Vec<Beat>>)>) -> Arc<Scripted> {
        let d = Scripted::new(id, Vec::new());
        *d.routes.lock().unwrap() = logins.into_iter().map(|(a, t)| (a.to_string(), (t, 0))).collect();
        d
    }

    pub fn with_caps(id: &str, caps: Caps, turns: Vec<Vec<Beat>>) -> Arc<Scripted> {
        Arc::new(Scripted { id: id.into(), caps, turns, refuse: None, leak: false, panics: false, hostile: false, probe: ready(), record: Arc::default(), routes: Arc::default(), judge: None })
    }

    /// A doer whose probe says what is given: installed, logged in.
    pub fn probed(id: &str, installed: bool, authed: bool, turns: Vec<Vec<Beat>>) -> Arc<Scripted> {
        let mut d = Scripted::new(id, turns);
        Arc::get_mut(&mut d).expect("just made").probe = Probe { installed, authed, ..ready() };
        d
    }

    /// A doer that cannot be started.
    pub fn broken(id: &str, why: &str) -> Arc<Scripted> {
        Arc::new(Scripted {
            id: id.into(),
            caps: Caps::default(),
            turns: Vec::new(),
            refuse: Some(why.into()),
            leak: false,
            panics: false,
            hostile: false,
            probe: ready(),
            record: Arc::default(),
            routes: Arc::default(),
            judge: None,
        })
    }

    /// A doer that keeps a way to speak after its run is over, so a test can make it.
    pub fn leaking(id: &str, turns: Vec<Vec<Beat>>) -> Arc<Scripted> {
        Arc::new(Scripted { id: id.into(), caps: Caps { live: true, resume: true, ..Caps::default() }, turns, refuse: None, leak: true, panics: false, hostile: false, probe: ready(), record: Arc::default(), routes: Arc::default(), judge: None })
    }

    /// A doer that panics when it is started.
    pub fn panicking(id: &str) -> Arc<Scripted> {
        Arc::new(Scripted { id: id.into(), caps: Caps::default(), turns: Vec::new(), refuse: None, leak: false, panics: true, hostile: false, probe: ready(), record: Arc::default(), routes: Arc::default(), judge: None })
    }

    /// A doer that panics when asked what it can do and when told anything.
    pub fn hostile(id: &str, turns: Vec<Vec<Beat>>) -> Arc<Scripted> {
        Arc::new(Scripted { id: id.into(), caps: Caps::default(), turns, refuse: None, leak: false, panics: false, hostile: true, probe: ready(), record: Arc::default(), routes: Arc::default(), judge: None })
    }

    pub fn launches(&self) -> Vec<Launch> {
        self.record.lock().unwrap().launches.clone()
    }

    pub fn emit(&self) -> Emit {
        self.record.lock().unwrap().emit.clone().expect("a leaking doer that has started")
    }

    pub fn starts(&self) -> Vec<Started> {
        self.record.lock().unwrap().starts.clone()
    }

    pub fn ctls(&self) -> Vec<RunCtl> {
        self.record.lock().unwrap().ctls.clone()
    }
}

struct ScriptedRun {
    tx: Sender<RunCtl>,
    record: Arc<Mutex<Record>>,
    hostile: bool,
}

impl Run for ScriptedRun {
    fn ctl(&mut self, ctl: RunCtl) -> Result<(), Problem> {
        self.record.lock().unwrap().ctls.push(ctl.clone());
        assert!(!self.hostile, "the scripted run was told to panic when told something");
        self.tx.send(ctl).map_err(|_| Problem::refused("scripted", "the run is over"))
    }
}

impl Doer for Scripted {
    fn id(&self) -> &str {
        &self.id
    }

    fn probe(&self) -> Probe {
        self.probe.clone()
    }

    fn caps(&self) -> Caps {
        assert!(!self.hostile, "the scripted doer was told to panic when asked its caps");
        self.caps
    }

    fn start(&self, cx: StartCx) -> Result<Box<dyn Run>, Problem> {
        if let Some(why) = &self.refuse {
            return Err(Problem::refused("scripted", why));
        }
        assert!(!self.panics, "the scripted doer was told to panic when started");
        let launch = cx.wrap(Launch::new(vec!["prog".into()], cx.site.dir.clone(), BTreeMap::new()));
        self.record.lock().unwrap().launches.push(launch);
        let n = {
            let mut rec = self.record.lock().unwrap();
            rec.starts.push(Started {
                task: cx.task.clone(),
                goal: cx.spec.goal.clone(),
                resume: cx.resume.as_ref().map(|s| s.id.clone()),
                text: cx.text.clone(),
                compacting: cx.compacting,
                dir: cx.site.dir.clone(),
                account: cx.spec.account.clone(),
            });
            rec.starts.len() - 1
        };
        let mut routes = self.routes.lock().unwrap();
        let judged = self.judge.as_ref().and_then(|j| j(&cx.site.dir, &cx.spec));
        let beats = match (judged, routes.get_mut(&cx.spec.account.clone().unwrap_or_default())) {
            (Some(beats), _) => beats,
            (None, Some((turns, played))) => {
                let beats = turns.get((*played).min(turns.len().saturating_sub(1))).cloned().unwrap_or_default();
                *played += 1;
                beats
            }
            (None, None) => self.turns.get(n.min(self.turns.len().saturating_sub(1))).cloned().unwrap_or_default(),
        };
        drop(routes);
        let (tx, rx) = mpsc::channel();
        let (dir, out) = (cx.site.dir.clone(), cx.out.clone());
        if self.leak {
            self.record.lock().unwrap().emit = Some(cx.out.clone());
        }
        drop(cx);
        std::thread::spawn(move || play(beats, rx, out, dir));
        Ok(Box::new(ScriptedRun { tx, record: self.record.clone(), hostile: self.hostile }))
    }
}

/// Block for the next order that matters; `None` when the run was stopped (or nobody holds it any more). An answer that came
/// before the script asked to wait for it is kept in `late` and is the first thing a wait finds.
fn wait(rx: &Receiver<RunCtl>, late: &mut VecDeque<RunCtl>, want_answer: bool) -> Option<RunCtl> {
    if want_answer && let Some(at) = late.iter().position(|c| matches!(c, RunCtl::Answer { .. })) {
        return late.remove(at);
    }
    loop {
        match rx.recv().ok()? {
            RunCtl::Abort | RunCtl::Kill => return None,
            c @ RunCtl::Answer { .. } if want_answer => return Some(c),
            _ => {}
        }
    }
}

fn play(beats: Vec<Beat>, rx: Receiver<RunCtl>, out: Emit, dir: PathBuf) {
    let mut late = VecDeque::new();
    for beat in beats {
        // a stop that has arrived ends the run, whatever is next in the script
        while let Ok(c) = rx.try_recv() {
            match c {
                RunCtl::Abort | RunCtl::Kill => return,
                c @ RunCtl::Answer { .. } => late.push_back(c),
                _ => {}
            }
        }
        let ev = match beat {
            Beat::Say(t) => RunEv::Message { role: Role::Assistant, text: t },
            Beat::Delta(t) => RunEv::Text(t),
            Beat::Think(t) => RunEv::Reasoning(t),
            Beat::Open(id, name, target, bytes) => RunEv::Open { id: id.into(), name: name.into(), target: target.into(), bytes },
            Beat::Tokens(n) => RunEv::Tokens(n),
            Beat::Reasoned(t) => RunEv::Message { role: Role::Reasoning, text: t },
            Beat::Tool(name, detail) => RunEv::Tool { name, detail, end: false },
            Beat::ToolEnd(name, detail) => RunEv::Tool { name, detail, end: true },
            Beat::Shell(cmd, o, exit) => RunEv::Shell { cmd, out: o, exit },
            Beat::Changed(p) => RunEv::Changed(p),
            Beat::Notice(t) => RunEv::Notice(t),
            Beat::Unread(dialect, kind) => RunEv::Unread { dialect: dialect.into(), kind: kind.into() },
            Beat::Served(model, host) => RunEv::Served(j4n_core::doer::Served { model: model.into(), host: Some(host.into()), ..Default::default() }),
            Beat::Running(units) => RunEv::Spend(units),
            Beat::Idle => RunEv::Metrics(Metrics::default()),
            Beat::Spend(usd) => RunEv::Metrics(Metrics { usd, tokens_in: 10, tokens_out: 5, ..Metrics::default() }),
            Beat::Cached(usd, tokens_in, cached, wrote) => RunEv::Metrics(Metrics { usd, tokens_in, tokens_out: 5, cached, wrote, ..Metrics::default() }),
            Beat::Called(usd, tokens_in, first, last) => RunEv::Metrics(Metrics { usd, tokens_in, tokens_out: 5, first, last, ..Metrics::default() }),
            Beat::Session(id) => RunEv::Session(id),
            Beat::Window(w) => RunEv::Window(w),
            Beat::List(open, total, active) => RunEv::List { open, total, active: active.into() },
            Beat::Limits(l) => RunEv::Limits(l.into_iter().map(|(window, utilization, resets_at)| j4n_core::doer::Limit { window: window.into(), utilization, resets_at, status: "allowed".into() }).collect()),
            Beat::Shut(window, resets_at) => RunEv::Limits(vec![j4n_core::doer::Limit { window: window.into(), utilization: 1.0, resets_at, status: "rejected".into() }]),
            Beat::Ask(spec) => RunEv::Ask(spec),
            Beat::Hold => {
                if wait(&rx, &mut late, true).is_none() {
                    return;
                }
                continue;
            }
            Beat::Forever => {
                wait(&rx, &mut late, false);
                return;
            }
            Beat::Pause(ms) => {
                let end = std::time::Instant::now() + Duration::from_millis(ms);
                while let Some(left) = end.checked_duration_since(std::time::Instant::now()) {
                    match rx.recv_timeout(left) {
                        Ok(RunCtl::Abort | RunCtl::Kill) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                        Ok(c @ RunCtl::Answer { .. }) => late.push_back(c),
                        Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                }
                continue;
            }
            Beat::Put(path, text) => {
                // the file system stamps files in coarse ticks; a change within one is not a change a checkpoint can see
                std::thread::sleep(Duration::from_millis(20));
                let p = dir.join(path);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(p, text).unwrap();
                continue;
            }
            Beat::Remove(path) => {
                std::thread::sleep(Duration::from_millis(20));
                std::fs::remove_file(dir.join(path)).unwrap();
                continue;
            }
            Beat::Fail(t) => RunEv::Error(t),
            Beat::NotRun(t) => RunEv::NotRun(t),
            Beat::End(answer, todo_left) => RunEv::End { answer, todo_left },
            Beat::Panic => panic!("the scripted doer was told to panic"),
            Beat::Vanish => return,
        };
        if !out.send(ev) {
            return;
        }
    }
}

/// A project, a home, a Log and the loop, all in scratch directories.
pub struct Rig {
    // the loop goes first: it stops its tasks, and writes to the Log and the folders, on the way out
    pub tasks: Tasks,
    pub timers: Arc<Timers>,
    pub folds: Folds,
    pub log: Log,
    pub clock: Arc<ManualClock>,
    pub project: Scratch,
    pub home: Scratch,
}

/// The base pack's data that the loop's rules need, exactly as the binary has it.
const BASE: [(&str, &str); 10] = [
    ("failures/table.toml", include_str!("../../../../packs/base/failures/table.toml")),
    ("folds/juards.toml", include_str!("../../../../packs/base/folds/juards.toml")),
    ("folds/spend.toml", include_str!("../../../../packs/base/folds/spend.toml")),
    ("folds/served.toml", include_str!("../../../../packs/base/folds/served.toml")),
    ("plans/claude.toml", include_str!("../../../../packs/base/plans/claude.toml")),
    ("plans/sources.toml", include_str!("../../../../packs/base/plans/sources.toml")),
    ("rules/juards.toml", include_str!("../../../../packs/base/rules/juards.toml")),
    ("rules/loop.toml", include_str!("../../../../packs/base/rules/loop.toml")),
    ("settings/cost.toml", include_str!("../../../../packs/base/settings/cost.toml")),
    ("settings/loop.toml", include_str!("../../../../packs/base/settings/loop.toml")),
];

/// The base pack's rules, failures, settings and folds, and the operator's own file on top of them (`mine`: more rules, or a
/// `value` for a setting).
pub fn base_tables(mine: &str) -> Tables {
    let file = |(path, text): &(&str, &str)| SourceFile { path: path.to_string(), text: text.to_string() };
    let mut layers = vec![Layer::new(LayerKind::Pack, "base-test", BASE.iter().map(file).collect())];
    if !mine.is_empty() {
        layers.push(Layer::new(LayerKind::User, "home", vec![SourceFile { path: "mine.toml".into(), text: format!("api = 1\n{mine}") }]));
    }
    let (tables, problems) = Tables::load(layers);
    assert!(problems.is_empty(), "{problems:?}");
    tables
}

/// Give the loop the base pack's reactions (see [`base_tables`]).
pub fn load_base(cfg: &mut Config, mine: &str) {
    let tables = base_tables(mine);
    let (rules, problems) = Rules::load(&tables);
    assert!(problems.is_empty(), "{problems:?}");
    let (failures, problems) = Failures::load(&tables);
    assert!(problems.is_empty(), "{problems:?}");
    let (settings, problems) = Settings::load(&tables);
    assert!(problems.is_empty(), "{problems:?}");
    let (counts, problems) = declared(&tables, Scope::Project);
    assert!(problems.is_empty(), "{problems:?}");
    for fold in counts {
        assert!(cfg.folds.add(fold, cfg.log.head()).is_empty());
    }
    (cfg.rules, cfg.failures, cfg.settings) = (Arc::new(rules), Arc::new(failures), settings);
}

impl Rig {
    /// The loop with the base pack's rules (and `mine` on top of them) and the loop's own config changed by `tune`.
    pub fn based(doers: Vec<Arc<Scripted>>, mine: &str, tune: impl FnOnce(&mut Config)) -> Rig {
        Rig::tuned(doers, |c| {
            load_base(c, mine);
            tune(c);
        })
    }

    pub fn new(doers: Vec<Arc<Scripted>>) -> Rig {
        Rig::tuned(doers, |_| {})
    }

    /// `tune` changes the loop's config before it starts.
    pub fn tuned(doers: Vec<Arc<Scripted>>, tune: impl FnOnce(&mut Config)) -> Rig {
        let project = Scratch::new("proj");
        let home = Scratch::new("home");
        let (log, clock) = open_log(&project);
        Rig::over(project, home, log, clock, doers, tune)
    }

    /// The loop over a log that is already open (a restart).
    pub fn over(
        project: Scratch,
        home: Scratch,
        log: Log,
        clock: Arc<ManualClock>,
        doers: Vec<Arc<Scripted>>,
        tune: impl FnOnce(&mut Config),
    ) -> Rig {
        let folds = Folds::new(&project.join(".j4n/folds"));
        folds.attach(&log).expect("attaches");
        let timers = Arc::new(Timers::start(&log));
        let mut cfg = Config::new(&log, &folds, &timers, project.path(), home.path());
        cfg.doers = doers.into_iter().map(|d| d as Arc<dyn Doer>).collect();
        tune(&mut cfg);
        let (tasks, problems) = Tasks::start(cfg).expect("the loop starts");
        assert!(problems.iter().all(|p| p.msg.contains("left out")), "{problems:?}");
        Rig { tasks, timers, folds, log, clock, project, home }
    }

    pub fn spawn(&self, goal: &str, doer: &str) -> String {
        self.tasks.spawn(TaskSpec::new(goal, doer), Actor::Operator).expect("spawns")
    }

    /// The row of `id` once `ready` holds; a task that never gets there fails the test with what the Log holds.
    pub fn until(&self, id: &str, what: &str, ready: impl Fn(&Row) -> bool) -> Row {
        match self.tasks.book().wait(id, ready, Duration::from_secs(20)) {
            Some(row) => row,
            None => panic!("{id} never {what}; it is {:?}\n{}", self.tasks.book().get(id).map(|r| r.state), self.dump()),
        }
    }

    pub fn finished(&self, id: &str) -> Row {
        self.until(id, "finished", |r| r.state.is_terminal())
    }

    pub fn events(&self) -> Vec<Ev> {
        self.log.read(0).collect()
    }

    /// The kinds of the events of one task, in order.
    pub fn kinds(&self, id: &str) -> Vec<String> {
        self.events().into_iter().filter(|e| e.task.as_deref() == Some(id)).map(|e| e.kind).collect()
    }

    pub fn of_kind(&self, kind: &str) -> Vec<Ev> {
        self.events().into_iter().filter(|e| e.kind == kind).collect()
    }

    /// Wait for an event that `ready` accepts; fails the test with the Log when none comes.
    pub fn until_event(&self, what: &str, ready: impl Fn(&Ev) -> bool) -> Ev {
        for _ in 0..4000 {
            if let Some(e) = self.events().into_iter().find(|e| ready(e)) {
                return e;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("no event: {what}\n{}", self.dump());
    }

    pub fn dump(&self) -> String {
        self.events()
            .iter()
            .map(|e| format!("{:>4} {:<18} {} {}", e.seq, e.kind, e.task.as_deref().unwrap_or("-"), e.data))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// A question that asks leave to run `action`.
pub fn approval(local: &str, action: &str) -> AskSpec {
    AskSpec {
        local: local.into(),
        question: format!("may I run `{action}`?"),
        shape: AskShape::Approval { action: action.into(), risk: 40 },
        inband: true,
        point: None,
        act: None,
    }
}

/// A question that offers options; `(id, recommended)`.
pub fn choice(local: &str, question: &str, options: &[(&str, bool)]) -> AskSpec {
    let options = options.iter().map(|(id, recommended)| Opt { id: id.to_string(), label: id.to_string(), recommended: *recommended });
    AskSpec { local: local.into(), question: question.into(), shape: AskShape::Choice { options: options.collect() }, inband: true, point: None, act: None }
}

pub fn pick(id: &str) -> Answer {
    Answer::Pick { id: id.into() }
}
