//! The `test` doer (pack `test`): no program, no model, no network. It plays its goal as a script, so
//! anything that drives J4n from outside — `j4n run "say ok" --doer test` — has a doer whose every move is known.
//!
//! A goal is steps, one per line or after a `;`. A line that opens with no verb is `say`.
//!
//! | step               | what the doer does                                                                    |
//! |--------------------|---------------------------------------------------------------------------------------|
//! | `say TEXT`         | says it; the last thing said is the report                                            |
//! | `tool NAME DETAIL` | a tool call and its result                                                            |
//! | `sh CMD => N`      | reports that CMD ran and exited N (nothing is run)                                    |
//! | `write PATH TEXT`  | asks leave to write the file; writes it on a yes, says so on a no                     |
//! | `run CMD`          | asks leave to run CMD (a shell command); reports it ran on a yes (nothing is run)     |
//! | `ask QUESTION`     | asks the operator, says what was answered                                             |
//! | `spend USD`        | reports that it spent that much                                                       |
//! | `wait MS`          | does nothing for that long                                                            |
//! | `todo N`           | ends with N items left to do (the loop carries it on, up to its setting)              |
//! | `window F`         | reports that its context is that full, from 0 to 1                                    |
//! | `who`              | says which child and which turn this is (a resumed session keeps its child)           |
//! | `hang`             | goes on until it is stopped                                                           |
//! | `silent`           | ends with no report and no sign of work                                               |
//! | `fail TEXT`        | fails                                                                                 |
//!
//! A run that is told to shrink the session says so and ends: the next one is the task going on.

use j4n_core::ask::{Answer, AskShape, AskSpec};
use j4n_core::change::Actor;
use j4n_core::doer::{Caps, Doer, Emit, Limit, Metrics, Probe, Role, Run, RunCtl, RunEv, StartCx};
use j4n_core::gate::{Action, Verb};
use j4n_core::table::Problem;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub const ID: &str = "test";

#[derive(Debug, Clone, PartialEq)]
enum Step {
    Say(String),
    Tool(String, String),
    Sh(String, i32),
    Write(String, String),
    /// `run CMD`: an approval of a shell command, which a grant can cover (`write` is asked about every time).
    Run(String),
    Ask(String),
    Spend(f64),
    Wait(u64),
    Todo(u32),
    Window(f64),
    /// `limits <week> <five_hour>`: the provider's windows, as a subscription's stream says them.
    Limits(f64, f64),
    Who,
    Hang,
    Silent,
    Fail(String),
}

fn parse(goal: &str) -> Vec<Step> {
    let mut steps = Vec::new();
    for raw in goal.split(['\n', ';']).map(str::trim).filter(|l| !l.is_empty()) {
        let (verb, rest) = raw.split_once(char::is_whitespace).map_or((raw, ""), |(v, r)| (v, r.trim()));
        let step = match verb {
            "say" => Step::Say(rest.into()),
            "tool" => {
                let (name, detail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                Step::Tool(name.into(), detail.trim().into())
            }
            "sh" => {
                let (cmd, exit) = rest.rsplit_once("=>").map_or((rest, 0), |(c, n)| (c.trim(), n.trim().parse().unwrap_or(1)));
                Step::Sh(cmd.into(), exit)
            }
            "write" => {
                let (path, text) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                Step::Write(path.into(), text.trim().into())
            }
            "run" => Step::Run(rest.into()),
            "ask" => Step::Ask(rest.into()),
            "spend" => Step::Spend(rest.parse().unwrap_or(0.0)),
            "wait" => Step::Wait(rest.parse().unwrap_or(0)),
            "todo" => Step::Todo(rest.parse().unwrap_or(1)),
            "window" => Step::Window(rest.parse().unwrap_or(0.0)),
            "limits" => {
                let (week, five) = rest.split_once(char::is_whitespace).unwrap_or((rest, "0"));
                Step::Limits(week.parse().unwrap_or(0.0), five.trim().parse().unwrap_or(0.0))
            }
            "who" => Step::Who,
            "hang" => Step::Hang,
            "silent" => Step::Silent,
            "fail" => Step::Fail(rest.into()),
            _ => Step::Say(raw.into()),
        };
        steps.push(step);
    }
    steps
}

/// Whether `path` stays inside the folder it is relative to.
fn inside(path: &str) -> bool {
    let p = Path::new(path);
    !p.as_os_str().is_empty() && p.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

#[derive(Default)]
struct Kids {
    /// Session id → (child, turns so far).
    by_session: HashMap<String, (u32, u32)>,
    children: u32,
}

pub struct TestDoer {
    id: String,
    brief: bool,
    kids: Mutex<Kids>,
}

impl Default for TestDoer {
    fn default() -> TestDoer {
        TestDoer { id: ID.into(), brief: false, kids: Mutex::default() }
    }
}

impl TestDoer {
    pub fn new() -> TestDoer {
        TestDoer::default()
    }

    /// The same doer under another id (a connector row of dialect `test`).
    pub fn named(mut self, id: &str) -> TestDoer {
        self.id = id.to_string();
        self
    }

    /// Whether it takes a Brief with each line (the connector row's `brief`).
    pub fn briefed(mut self, brief: bool) -> TestDoer {
        self.brief = brief;
        self
    }

    fn kids(&self) -> MutexGuard<'_, Kids> {
        self.kids.lock().unwrap_or_else(|e| e.into_inner())
    }
}

struct TestRun(Sender<RunCtl>);

impl Run for TestRun {
    fn ctl(&mut self, ctl: RunCtl) -> Result<(), Problem> {
        self.0.send(ctl).map_err(|_| Problem::refused(ID, "the run is over"))
    }
}

impl Doer for TestDoer {
    fn id(&self) -> &str {
        &self.id
    }

    fn probe(&self) -> Probe {
        Probe { installed: true, version: Some("scripted".into()), authed: true, detail: "plays its goal as a script".into() }
    }

    fn caps(&self) -> Caps {
        Caps { live: true, resume: true, compact: true, brief: self.brief, skills: false, tools: false, walled: false }
    }

    fn start(&self, cx: StartCx) -> Result<Box<dyn Run>, Problem> {
        let steps = if cx.compacting { vec![Step::Say("the session was shrunk".into())] } else { parse(cx.text.as_deref().unwrap_or(&cx.spec.goal)) };
        let asked = cx.resume.as_ref().filter(|r| r.doer == self.id).map(|r| r.id.clone());
        let (session, child, turn, lost) = {
            let mut kids = self.kids();
            match asked.as_ref().and_then(|id| kids.by_session.get_mut(id).map(|k| (id.clone(), k))) {
                Some((id, k)) => {
                    k.1 += 1;
                    (id, k.0, k.1, false)
                }
                None => {
                    kids.children += 1;
                    let (child, id) = (kids.children, format!("test-{}", kids.children));
                    kids.by_session.insert(id.clone(), (child, 1));
                    (id, child, 1, asked.is_some())
                }
            }
        };
        let (tx, rx) = mpsc::channel();
        let play = Play { rx, out: cx.out.clone(), dir: cx.site.dir.clone(), task: cx.task.clone(), who: format!("child {child} turn {turn}"), id: self.id.clone() };
        std::thread::spawn(move || {
            play.out.send(RunEv::Session(session));
            if lost {
                play.out.send(RunEv::Notice("the session was gone: this is a new child".into()));
            }
            play.run(&steps);
        });
        Ok(Box::new(TestRun(tx)))
    }
}

struct Play {
    rx: Receiver<RunCtl>,
    out: Emit,
    dir: PathBuf,
    task: String,
    who: String,
    id: String,
}

enum Wake {
    Answer(Answer),
    Stop,
    Timeout,
}

impl Play {
    /// Wait for an answer (or, with `until`, for the time to pass). A steer on the way is noted.
    fn wait(&self, until: Option<Instant>, want_answer: bool) -> Wake {
        loop {
            let left = until.map_or(Duration::from_secs(3600), |t| t.saturating_duration_since(Instant::now()));
            if until.is_some() && left.is_zero() {
                return Wake::Timeout;
            }
            match self.rx.recv_timeout(left.min(Duration::from_secs(3600))) {
                Ok(RunCtl::Abort | RunCtl::Kill) | Err(mpsc::RecvTimeoutError::Disconnected) => return Wake::Stop,
                Ok(RunCtl::Answer { answer, .. }) if want_answer => return Wake::Answer(answer),
                Ok(RunCtl::Steer(text)) => {
                    self.out.send(RunEv::Notice(format!("steered: {text}")));
                }
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }

    /// Whether a stop has arrived.
    fn stopped(&self) -> bool {
        while let Ok(c) = self.rx.try_recv() {
            match c {
                RunCtl::Abort | RunCtl::Kill => return true,
                RunCtl::Steer(text) => {
                    self.out.send(RunEv::Notice(format!("steered: {text}")));
                }
                RunCtl::Answer { .. } | RunCtl::Told { .. } => {}
            }
        }
        false
    }

    fn ask(&self, spec: AskSpec) -> Option<Answer> {
        self.out.send(RunEv::Ask(spec));
        match self.wait(None, true) {
            Wake::Answer(a) => Some(a),
            _ => None,
        }
    }

    fn run(&self, steps: &[Step]) {
        let (mut said, mut todo) = (None::<String>, 0);
        let say = |text: &str, said: &mut Option<String>| {
            *said = Some(text.to_string());
            self.out.send(RunEv::Message { role: Role::Assistant, text: text.to_string() })
        };
        for (n, step) in steps.iter().enumerate() {
            if self.stopped() {
                return;
            }
            let ok = match step {
                Step::Say(t) => say(t, &mut said),
                Step::Who => say(&self.who, &mut said),
                Step::Tool(name, detail) => {
                    self.out.send(RunEv::Tool { name: name.clone(), detail: detail.clone(), end: false })
                        && self.out.send(RunEv::Tool { name: name.clone(), detail: detail.clone(), end: true })
                }
                Step::Sh(cmd, exit) => {
                    self.out.send(RunEv::Shell { cmd: cmd.clone(), out: String::new(), exit: *exit })
                }
                Step::Spend(usd) => self.out.send(RunEv::Metrics(Metrics { usd: *usd, tokens_in: 10, tokens_out: 5, ..Metrics::default() })),
                Step::Todo(n) => {
                    todo = *n;
                    true
                }
                Step::Window(w) => self.out.send(RunEv::Window(*w)),
                Step::Limits(week, five) => {
                    let win = |window: &str, utilization, resets_at| Limit { window: window.into(), utilization, resets_at, status: "allowed".into() };
                    self.out.send(RunEv::Limits(vec![win("five_hour", *five, 1_000), win("seven_day", *week, 9_000)]))
                }
                Step::Wait(ms) => !matches!(self.wait(Some(Instant::now() + Duration::from_millis(*ms)), false), Wake::Stop),
                Step::Hang => {
                    self.wait(None, false);
                    return;
                }
                Step::Silent => {
                    self.out.send(RunEv::End { answer: None, todo_left: 0 });
                    return;
                }
                Step::Fail(t) => {
                    self.out.send(RunEv::Error(t.clone()));
                    return;
                }
                Step::Ask(q) => {
                    let spec = AskSpec { local: format!("q{n}"), question: q.clone(), shape: AskShape::Text, inband: true, point: None, act: None };
                    match self.ask(spec) {
                        Some(a) => say(&format!("answered: {}", a.render()), &mut said),
                        None => return,
                    }
                }
                Step::Run(cmd) => {
                    let mut act = Action::shell(Actor::Doer(self.id.clone()), cmd).task(&self.task);
                    act.site = Some(self.dir.display().to_string());
                    let shape = AskShape::Approval { action: format!("run {cmd}"), risk: 40 };
                    let spec = AskSpec { local: format!("r{n}"), question: format!("run {cmd}?"), shape, inband: true, point: None, act: Some(Box::new(act)) };
                    match self.ask(spec) {
                        Some(Answer::Yes) => self.out.send(RunEv::Shell { cmd: cmd.clone(), out: String::new(), exit: 0 }),
                        Some(_) => self.out.send(RunEv::Notice(format!("not allowed: run {cmd}"))),
                        None => return,
                    }
                }
                Step::Write(path, text) => {
                    if !inside(path) {
                        self.out.send(RunEv::Notice(format!("not written: {path} is outside the folder")))
                    } else {
                        let mut act = Action::new(Verb::Write, Actor::Doer(self.id.clone()), path).task(&self.task);
                        act.site = Some(self.dir.display().to_string());
                        act.inside = Some(true);
                        let spec = AskSpec {
                            local: format!("w{n}"),
                            question: format!("write {path}?"),
                            shape: AskShape::Approval { action: format!("write {path}"), risk: 40 },
                            inband: true,
                            point: None,
                            act: Some(Box::new(act)),
                        };
                        match self.ask(spec) {
                            Some(Answer::Yes) => {
                                            let file = self.dir.join(path);
                                let made = file.parent().is_none_or(|p| std::fs::create_dir_all(p).is_ok()) && std::fs::write(&file, text).is_ok();
                                if made { self.out.send(RunEv::Changed(vec![path.clone()])) } else { self.out.send(RunEv::Notice(format!("could not write {path}"))) }
                            }
                            Some(_) => self.out.send(RunEv::Notice(format!("not allowed: write {path}"))),
                            None => return,
                        }
                    }
                }
            };
            if !ok {
                return;
            }
        }
        self.out.send(RunEv::End { answer: said, todo_left: todo });
    }
}
