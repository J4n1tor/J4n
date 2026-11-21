//! The dialects that run a program once for a task and read what it prints: `text` (stdout is the
//! answer), `json-last` (the last line is JSON, and `"ok": false` is a failure whatever the exit code) and `jsonl-map`
//! (a stream of JSON lines mapped to events by data, [`crate::jsonmap`]). A `tool` is asked of the Gate before it runs; an
//! `agent` is the operator's choice already. The real exit code always reaches the chat.

use j4n_core::text::line as clip;
use crate::jsonmap::Mapper;
use crate::limits::{self, Gauge, Row as Limits};
use crate::permit;
use crate::manifest::{Connector, Dialect, Is, Risk};
use crate::proc::{self, Child, Exit};
use crate::template::{self, Vars};
use j4n_core::ask::{Answer, AskShape, AskSpec};
use j4n_core::change::Actor;
use j4n_core::doer::{Caps, Doer, Emit, Probe, Role, Run, RunCtl, RunEv, StartCx};
use j4n_core::gate::{Action, ReadOnly, Verb};
use j4n_core::place::Launch;
use j4n_core::table::Problem;
use j4n_core::task::Mode;
use serde_json::Value as Json;
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard};

fn now_s() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// How much of a program's output is kept.
const KEEP: usize = 1 << 20;
/// How much of its output goes into a message about a failure.
const SHOWN: usize = 400;

pub struct OneShot {
    c: Connector,
    env: BTreeMap<String, String>,
    mapper: Option<Arc<Mapper>>,
    /// The rows of the connector's `limits`, and the fraction the last run said (for a refusal that says no number).
    limits: Arc<Vec<Limits>>,
    last: Arc<Mutex<Option<f64>>>,
}

enum Msg {
    Ctl(RunCtl),
    Exit,
}

struct OneRun {
    id: String,
    tx: Sender<Msg>,
}

impl Run for OneRun {
    fn ctl(&mut self, ctl: RunCtl) -> Result<(), Problem> {
        self.tx.send(Msg::Ctl(ctl)).map_err(|_| Problem::refused(&self.id, "the run is over"))
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl OneShot {
    pub fn new(c: Connector, env: BTreeMap<String, String>) -> Result<OneShot, String> {
        let mapper = match (c.dialect, &c.map) {
            (Dialect::JsonlMap, Some(m)) => Some(Arc::new(Mapper::new(m)?)),
            _ => None,
        };
        let limits = Arc::new(c.limits.clone());
        Ok(OneShot { c, env, mapper, limits, last: Arc::new(Mutex::new(None)) })
    }

    /// What the task brings, by name; for a tool, its arguments from the goal.
    fn vars(&self, cx: &StartCx, text: &str) -> Result<Vars, String> {
        let mut vars = Vars::new();
        vars.insert("prompt".into(), text.to_string());
        // a program that reads its prompt from a file is given the path of one that holds it: in the task's own temporary folder, which goes with the task
        if self.c.argv.iter().chain(&self.c.start).any(|a| a.contains("{promptfile}")) {
            // behind the box its `/tmp` is its own, so a task with no temporary folder of its own gets the file beside the work, taken away when the run ends
            let dir = cx.tmp.clone().unwrap_or_else(|| cx.site.dir.clone());
            let path = dir.join(format!(".j4n-prompt-{}.txt", cx.task));
            let _ = std::fs::create_dir_all(&dir);
            std::fs::write(&path, text).map_err(|e| format!("the prompt could not be put in {}: {e}", path.display()))?;
            vars.insert("promptfile".into(), path.display().to_string());
        }
        vars.insert("cwd".into(), cx.site.dir.display().to_string());
        if let Some(m) = cx.spec.tune.model.as_ref().filter(|m| !m.is_empty()) {
            vars.insert("model".into(), m.clone());
        }
        if let Some(e) = cx.spec.tune.effort.as_ref().filter(|e| !e.is_empty()) {
            vars.insert("effort".into(), e.clone());
        }
        if let Some(r) = cx.resume.as_ref().filter(|r| r.doer == self.c.id && !self.c.resume.is_empty()) {
            vars.insert("session".into(), r.id.clone());
        }
        if self.c.args.is_empty() {
            return Ok(vars);
        }
        let given: Option<serde_json::Map<String, Json>> = serde_json::from_str::<Json>(text).ok().and_then(|j| if let Json::Object(m) = j { Some(m) } else { None });
        for (i, a) in self.c.args.iter().enumerate() {
            let value = match &given {
                Some(m) => m.get(&a.name).map(|v| v.as_str().map_or_else(|| v.to_string(), String::from)),
                // a goal that is not a JSON object is the first argument
                None => (i == 0).then(|| text.to_string()),
            };
            match value.or_else(|| a.default.clone()) {
                Some(v) => {
                    vars.insert(a.name.clone(), v);
                }
                None if a.required => return Err(format!("the argument `{}` is needed (a goal of the form {{\"{}\": …}})", a.name, a.name)),
                None => {}
            }
        }
        Ok(vars)
    }

    /// Where this run's limit is read: its output, and the subcommands and files the rows name.
    fn gauge(&self, cx: &StartCx, env: &BTreeMap<String, String>) -> Gauge {
        let mut gauge = Gauge::new(self.limits.clone(), self.last.clone());
        let launches = self.limits.iter().map(|r| {
            (!r.command.is_empty()).then(|| {
                let mut argv = vec![self.c.program().to_string()];
                argv.extend(r.command.iter().cloned());
                cx.wrap(Launch { login: self.c.login.clone(), ..Launch::new(argv, cx.site.dir.clone(), env.clone()) })
            })
        });
        gauge.ask_with(launches.collect(), cx.site.dir.clone());
        gauge
    }

    /// The command line of a run.
    fn argv(&self, cx: &StartCx, vars: &Vars) -> Result<Vec<String>, String> {
        if !self.c.argv.is_empty() {
            return template::render(&self.c.argv, vars);
        }
        // what a run adds goes in front of the prompt, which stands last; without a `{prompt}` it goes at the end
        let mut extra: Vec<String> = Vec::new();
        if vars.contains_key("session") {
            extra.extend(self.c.resume.iter().cloned());
        }
        if vars.contains_key("model") {
            extra.extend(self.c.model.iter().cloned());
        }
        // `--auto` only with the config that denies what the gate does not call safe: it answers what the config leaves to ask
        extra.extend(self.c.guard.iter().flat_map(|g| g.args.iter().cloned()));
        let mode = if cx.spec.tune.mode == Mode::Read { "read" } else { "write" };
        extra.extend(self.c.modes.get(mode).into_iter().flatten().cloned());
        let at = self.c.start.iter().position(|a| a.contains("{prompt}")).unwrap_or(self.c.start.len());
        let mut tpl = self.c.start.clone();
        tpl.splice(at..at, extra);
        let mut argv = vec![self.c.program().to_string()];
        argv.extend(template::render(&tpl, vars)?);
        Ok(argv)
    }

    /// The question put to the Gate before a tool runs: a command line like any other, so `danger()` and the read-only modes
    /// have their say; `risk` is how much the operator is warned.
    fn gate(&self, cx: &StartCx, argv: &[String]) -> AskSpec {
        let line = argv.join(" ");
        let mut act = Action::new(Verb::Shell, Actor::Doer(self.c.id.clone()), &line).task(&cx.task);
        act.site = Some(cx.site.dir.display().to_string());
        act.read_only = if cx.spec.tune.mode == Mode::Read { ReadOnly::Explicit } else { ReadOnly::No };
        let level = |r: Risk| match r {
            Risk::Read => 10,
            Risk::Write => 40,
            Risk::Net => 40,
            Risk::Exec => 60,
        };
        let risk = level(self.c.risk).max(self.c.says.map_or(0, level));
        AskSpec { local: "run".into(), question: format!("run `{}`?", clip(&line, 200)), shape: AskShape::Approval { action: format!("{}: {}", self.c.id, clip(&line, 200)), risk }, inband: true, point: None, act: Some(Box::new(act)) }
    }
}

impl OneShot {
    /// A program that is a local server: it is started, the prompt is posted, and the reply is read as the program's lines.
    fn serve_run(&self, cx: &StartCx, launch: Launch, sv: crate::manifest::ServeSpec, vars: &Vars, gate: Option<AskSpec>) -> Result<Box<dyn Run>, Problem> {
        let id = self.c.id.clone();
        let tpl = sv.resume.as_deref().filter(|_| vars.contains_key("session")).unwrap_or(&sv.body);
        let body = template::fill_with(tpl, vars, &template::json_part).map_err(|e| Problem::refused(&id, e))?;
        let mut rd = Reader::new(self.c.dialect, cx.out.clone(), self.mapper.clone(), format!("{} (a server)", self.c.program()), self.c.program().to_string());
        rd.decode = crate::decode::steps(&self.c.decode).map_err(|e| Problem::refused(&id, e))?;
        rd.gauge = self.gauge(cx, &self.env);
        let (tx, rx) = mpsc::channel::<Msg>();
        let (out, program) = (cx.out.clone(), self.c.program().to_string());
        std::thread::spawn(move || {
            let said = |why: String| {
                out.send(RunEv::Error(why));
            };
            if !pass_gate(&rx, &out, gate) {
                return;
            }
            let server = match crate::serve::start(&launch, &sv.ready, 20) {
                Ok(s) => s,
                Err(e) => return said(format!("`{program}`: {e}")),
            };
            if matches!(rx.try_recv(), Ok(Msg::Ctl(RunCtl::Abort | RunCtl::Kill))) {
                return;
            }
            match server.post(sv.path.as_deref().unwrap_or("/"), &body, 120) {
                Err(e) => said(format!("`{program}`: {e}")),
                Ok(r) if r.status == 429 => {
                    let message = rd.refused(&format!("`{program}` is rate limited: HTTP 429, retry in {}s", r.retry.unwrap_or(60)));
                    said(message)
                }
                Ok(r) if !(200..300).contains(&r.status) => said(format!("`{program}` answered HTTP {}: {}", r.status, clip(&r.body, SHOWN))),
                Ok(r) => {
                    r.body.lines().for_each(|l| rd.line(l));
                    rd.done(&Exit { code: Some(0), signal: None, tail: String::new(), killed: false, broken: false });
                }
            }
        });
        Ok(Box::new(OneRun { id, tx }))
    }
}

impl Doer for OneShot {
    fn id(&self) -> &str {
        &self.c.id
    }

    fn probe(&self) -> Probe {
        self.c.probe(&self.env)
    }

    fn caps(&self) -> Caps {
        Caps { live: false, resume: !self.c.resume.is_empty() && self.c.dialect == Dialect::JsonlMap, walled: self.c.guard.is_some(), ..Caps::default() }
    }

    fn start(&self, cx: StartCx) -> Result<Box<dyn Run>, Problem> {
        let id = self.c.id.clone();
        let text = cx.text.clone().unwrap_or_else(|| cx.spec.goal.clone());
        let vars = self.vars(&cx, &text).map_err(|e| Problem::refused(&id, e))?;
        let argv = self.argv(&cx, &vars).map_err(|e| Problem::refused(&id, e))?;
        // a program with permission rules of its own is given the config J4n made for the task, by a variable that outranks every file
        let mut env = self.env.clone();
        // a variable that carries the prompt: `FOO_PROMPT = "{prompt}"`
        for v in env.values_mut().filter(|v| v.contains('{')) {
            if let Ok(t) = template::fill_with(v, &vars, &|x| x.to_string()) {
                *v = t;
            }
        }
        if let Some(g) = &self.c.guard {
            // the program's own scratch folder is under the `TMPDIR` it will run with
            let tmp = cx.tmp.as_ref().map(|t| t.display().to_string()).or_else(|| env.get("TMPDIR").cloned()).or_else(|| std::env::var("TMPDIR").ok());
            let scratch = permit::opencode_scratch(tmp.as_deref());
            env.insert(g.env.clone(), permit::opencode_walled(g, self.c.permit_full, cx.spec.tune.mode == Mode::Read || cx.site.read_only, &cx.site.dir, &cx.site.also, &scratch, cx.site.notes.iter().any(|n| n.starts_with(crate::place::boxed::BEHIND))));
        }
        let launch = cx.wrap(Launch { login: self.c.login.clone(), ..Launch::new(argv.clone(), cx.site.dir.clone(), env.clone()) });
        let stdin = match (&self.c.stdin, self.c.dialect) {
            (Some(t), _) => template::fill_with(t, &vars, &|x| x.to_string()).ok(),
            (None, Dialect::JsonLast) => Some(match serde_json::from_str::<Json>(&text) {
                Ok(j @ Json::Object(_)) => j.to_string(),
                _ => serde_json::json!({ "prompt": text }).to_string(),
            }),
            _ => None,
        };
        let gate = (self.c.is == Is::Tool || (self.c.gated && cx.spec.tune.mode != Mode::Read)).then(|| self.gate(&cx, &argv));
        if let Some(sv) = self.c.serve.clone() {
            return self.serve_run(&cx, launch, sv, &vars, gate);
        }
        let (tx, rx) = mpsc::channel::<Msg>();
        let promptfile = vars.get("promptfile").cloned();
        let mut rd = Reader::new(self.c.dialect, cx.out.clone(), self.mapper.clone(), argv.join(" "), self.c.program().to_string());
        rd.decode = crate::decode::steps(&self.c.decode).map_err(|e| Problem::refused(&id, e))?;
        rd.gauge = self.gauge(&cx, &env);
        rd.tool = self.c.is == Is::Tool;
        let reader = Arc::new(Mutex::new(rd));
        let out = cx.out.clone();
        let exit_tx = tx.clone();
        std::thread::spawn(move || {
            if !pass_gate(&rx, &out, gate) {
                return;
            }
            let on_line = reader.clone();
            let finish = reader.clone();
            let spawned = proc::spawn(&launch, stdin.is_some(), move |l| lock(&on_line).line(l), move |e| {
                if let Some(f) = &promptfile {
                    let _ = std::fs::remove_file(f);
                }
                lock(&finish).done(&e);
                let _ = exit_tx.send(Msg::Exit);
            });
            let (child, pipe) = match spawned {
                Ok(c) => c,
                Err(e) => {
                    out.send(RunEv::Error(format!("could not start `{}`: {e}", launch.argv.first().map_or("", String::as_str))));
                    return;
                }
            };
            if let (Some(mut pipe), Some(body)) = (pipe, stdin) {
                let _ = writeln!(pipe, "{body}").and_then(|_| pipe.flush());
            }
            wait(&rx, &child, &out);
        });
        Ok(Box::new(OneRun { id, tx }))
    }
}

/// The question of the Gate before a run, if there is one: whether the run goes on (a no says why, a stop says nothing).
fn pass_gate(rx: &mpsc::Receiver<Msg>, out: &Emit, gate: Option<AskSpec>) -> bool {
    let Some(spec) = gate else { return true };
    out.send(RunEv::Ask(spec));
    loop {
        match rx.recv() {
            Ok(Msg::Ctl(RunCtl::Answer { answer: Answer::Yes, .. })) => return true,
            Ok(Msg::Ctl(RunCtl::Answer { .. })) => {
                out.send(RunEv::Error("this was not allowed".into()));
                return false;
            }
            Ok(Msg::Ctl(RunCtl::Abort | RunCtl::Kill)) | Err(_) => return false,
            Ok(_) => {}
        }
    }
}

/// Until the program is over: a stop takes it down, a steer is said to be of no use.
fn wait(rx: &mpsc::Receiver<Msg>, child: &Child, out: &Emit) {
    while let Ok(msg) = rx.recv() {
        match msg {
            Msg::Exit => return,
            Msg::Ctl(RunCtl::Abort | RunCtl::Kill) => {
                child.kill();
                return;
            }
            Msg::Ctl(RunCtl::Steer(_)) => {
                out.send(RunEv::Notice("this program takes its words once, when it starts: the line was not delivered".into()));
            }
            Msg::Ctl(RunCtl::Answer { .. } | RunCtl::Told { .. }) => {}
        }
    }
}

/// What the output of one run adds up to, line by line.
struct Reader {
    dialect: Dialect,
    out: Emit,
    mapper: Option<Arc<Mapper>>,
    cmd: String,
    program: String,
    /// The output so far (text, json-last), kept to [`KEEP`].
    all: String,
    /// The words since the last tool: the answer if nothing follows.
    para: String,
    session: Option<String>,
    said: Option<String>,
    ended: bool,
    failed: Option<String>,
    answer: Option<String>,
    /// The error of the last tool, when the last tool that came back had one.
    tool_error: Option<String>,
    /// What a line is before it is JSON (`decode`); none: the line is JSON as it stands.
    decode: Vec<crate::decode::Step>,
    /// A tool: a run that ends 0 with nothing printed is a finished run.
    tool: bool,
    /// What the run says of the program's usage limit.
    gauge: Gauge,
    /// The program's health in this run: the lines it said, the ones the map made something of, the ones it did not, and the shape of its JSON.
    lines: u32,
    read: u32,
    unread: u32,
    shape: crate::canary::Shape,
}

impl Reader {
    fn new(dialect: Dialect, out: Emit, mapper: Option<Arc<Mapper>>, cmd: String, program: String) -> Reader {
        Reader { dialect, out, mapper, cmd, program, all: String::new(), para: String::new(), session: None, said: None, ended: false, failed: None, answer: None, tool_error: None, decode: Vec::new(), tool: false, gauge: Gauge::new(Arc::default(), Arc::default()), lines: 0, read: 0, unread: 0, shape: Default::default() }
    }

    fn keep(&mut self, line: &str) {
        if self.all.len() + line.len() < KEEP {
            self.all.push_str(line);
            self.all.push('\n');
        }
    }

    /// Settle the words said so far as a message of their own.
    fn flush(&mut self) {
        let words = std::mem::take(&mut self.para);
        if !words.trim().is_empty() {
            self.said = Some(words.trim().to_string());
            self.out.send(RunEv::Message { role: Role::Assistant, text: words.trim().to_string() });
        }
    }

    fn line(&mut self, line: &str) {
        self.gauge.line(line);
        match self.dialect {
            Dialect::Text => {
                self.keep(line);
                self.out.send(RunEv::Text(format!("{line}\n")));
            }
            Dialect::JsonLast => self.keep(line),
            _ => self.mapped(line),
        }
    }

    fn mapped(&mut self, line: &str) {
        let Some(mapper) = self.mapper.clone() else { return };
        if line.trim().is_empty() {
            return;
        }
        let doc = if self.decode.is_empty() {
            let Ok(doc) = serde_json::from_str::<Json>(line) else {
                (self.lines, self.unread) = (self.lines + 1, self.unread + 1);
                self.out.send(RunEv::Notice(format!("log: {}", clip(line, 120))));
                return;
            };
            doc
        } else {
            // a line the steps drop (a spinner, an `event:` line, a blank) is no event
            let Some(doc) = crate::decode::line(&self.decode, line) else { return };
            doc
        };
        let m = mapper.line(&doc);
        self.lines += 1;
        crate::canary::note(&mut self.shape, &doc);
        if m.evs.is_empty() && m.session.is_none() && !m.end && m.answer.is_none() && m.cost.is_none() && m.error.is_none() && m.metrics().is_none() && !m.tool_ok && m.tool_failed.is_none() {
            self.unread += 1;
        } else {
            self.read += 1;
        }
        if let Some(id) = m.session.clone().filter(|id| self.session.as_ref() != Some(id)) {
            self.session = Some(id.clone());
            self.out.send(RunEv::Session(id));
        }
        for ev in m.evs.iter().cloned() {
            match &ev {
                RunEv::Text(t) => self.para.push_str(t),
                RunEv::Message { text, .. } => {
                    self.para.clear();
                    self.said = Some(text.clone());
                }
                RunEv::Tool { .. } | RunEv::Shell { .. } => self.flush(),
                RunEv::Error(e) => self.failed = Some(e.clone()),
                _ => {}
            }
            self.out.send(ev);
        }
        if let Some(metrics) = m.metrics() {
            self.out.send(RunEv::Metrics(metrics));
        }
        if let Some(e) = m.error {
            self.failed = Some(e);
        }
        // the last tool decides: one that came back with an error and no tool after it that did not
        if m.tool_ok {
            self.tool_error = None;
        }
        if m.tool_failed.is_some() {
            self.tool_error = m.tool_failed;
        }
        if m.answer.is_some() {
            self.answer = m.answer;
        }
        self.ended |= m.end;
    }

    /// A refusal for the limit that came outside the program's output (an HTTP status): the reading is told, and the failure that says it is given
    /// with when the program works again.
    fn refused(&mut self, said: &str) -> String {
        let told = self.gauge.told(Some(said), now_s());
        if !told.limits.is_empty() {
            self.out.send(RunEv::Limits(told.limits));
        }
        told.refused_until.map_or_else(|| said.to_string(), |at| limits::refused_message(said, at))
    }

    /// The program is over: say how the run ended. Our own kill says nothing.
    fn done(&mut self, exit: &Exit) {
        if exit.killed {
            return;
        }
        self.gauge.stderr(&exit.tail);
        self.gauge.after();
        let failure = |why: String| RunEv::Error(why);
        let exited = if exit.code == Some(0) {
            None
        } else {
            let tail = if exit.tail.is_empty() { clip(&self.all, SHOWN) } else { clip(&exit.tail, SHOWN) };
            Some(format!("`{}` ended with {}{}", self.program, exit.how(), if tail.is_empty() { String::new() } else { format!(": {tail}") }))
        };
        if let Some(code) = exit.code.filter(|c| *c != 0) {
            // the real exit code is in the chat whatever else is said
            self.out.send(RunEv::Shell { cmd: self.cmd.clone(), out: clip(&self.all, SHOWN), exit: code });
        }
        let unfinished = if self.failed.is_none() && exited.is_none() { self.unfinished(&exit.tail) } else { None };
        if self.mapper.is_some() && self.lines > 0 {
            // a program that spoke and none of it was understood has changed what it says (or never said it): the run says so, and a rule may learn it again
            let drifted = self.read == 0 && !self.ended;
            let crashed = exit.signal.is_some() || exit.broken;
            self.out.send(RunEv::Health(serde_json::json!({ "program": self.program, "lines": self.lines, "read": self.read, "unread": self.unread, "ended": self.ended, "crashed": crashed, "shape": crate::canary::digest(&self.shape), "drifted": drifted })));
        }
        let ev = match self.dialect {
            Dialect::Text => match exited {
                Some(why) => failure(why),
                None => {
                    let mut said = self.all.trim().to_string();
                    // a command that prints nothing and ends 0 has done what it was asked: its answer is that it ended well (`test -f`, `grep -q`, a formatter)
                    if said.is_empty() && self.tool {
                        said = "(no output; it ended with exit 0)".into();
                    }
                    if !said.is_empty() {
                        self.out.send(RunEv::Message { role: Role::Assistant, text: said.clone() });
                    }
                    RunEv::End { answer: (!said.is_empty()).then_some(said), todo_left: 0 }
                }
            },
            Dialect::JsonLast => self.json_last(exited),
            _ => match (self.failed.take(), exited, unfinished) {
                (Some(why), ..) | (None, Some(why), _) | (None, None, Some(why)) => failure(why),
                (None, None, None) => {
                    let tail = std::mem::take(&mut self.para);
                    if !tail.trim().is_empty() {
                        self.said = Some(tail.trim().to_string());
                        self.out.send(RunEv::Message { role: Role::Assistant, text: tail.trim().to_string() });
                    }
                    RunEv::End { answer: self.answer.take().or_else(|| self.said.clone()), todo_left: 0 }
                }
            },
        };
        // the limit as the run left it goes first: a refusal is then a failure that says when the program works again
        let failed = if let RunEv::Error(m) = &ev { Some(m.clone()) } else { None };
        let told = self.gauge.told(failed.as_deref(), now_s());
        if !told.limits.is_empty() {
            self.out.send(RunEv::Limits(told.limits));
        }
        let ev = match (ev, told.refused_until) {
            (RunEv::Error(m), Some(at)) => RunEv::Error(limits::refused_message(&told.said.map_or(m.clone(), |s| format!("{m}: {s}")), at)),
            (ev, _) => ev,
        };
        self.out.send(ev);
    }

    /// Why a run that exited with 0 and said no error did not finish well: its last tool came back with an error, or the map says how a run ends and
    /// this one never did. The line of stderr that says it refused a tool (`map.rejects`) goes with the reason: the permission and its path.
    fn unfinished(&mut self, stderr: &str) -> Option<String> {
        let mapper = self.mapper.as_ref()?;
        let cause = match self.tool_error.take() {
            Some(why) => format!("the run ended on a tool that failed: {}", clip(&why, SHOWN)),
            None if mapper.expects_end() && !self.ended => format!("`{}` stopped without ending the run, and its last words are not an answer", self.program),
            None => return None,
        };
        Some(match mapper.rejection(stderr) {
            Some(line) => format!("{cause} · {}", clip(&line, SHOWN)),
            None => cause,
        })
    }

    /// The last line is JSON; `"ok": false` fails the run whatever the exit code said.
    fn json_last(&mut self, exited: Option<String>) -> RunEv {
        let last = self.all.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or_default();
        let Ok(doc) = serde_json::from_str::<Json>(last) else {
            return RunEv::Error(exited.unwrap_or_else(|| format!("`{}` did not end with a line of JSON: {}", self.program, clip(last, 120))));
        };
        let field = |names: &[&str]| names.iter().find_map(|n| doc.get(*n).and_then(Json::as_str)).map(String::from);
        if doc.get("ok").and_then(Json::as_bool) == Some(false) {
            let why = field(&["error", "message", "reason"]).unwrap_or_else(|| "it said `ok: false`".into());
            return RunEv::Error(format!("`{}` reported failure: {why}", self.program));
        }
        if let Some(why) = exited {
            return RunEv::Error(why);
        }
        let answer = field(&["answer", "text", "result", "message"]).unwrap_or_else(|| doc.to_string());
        self.out.send(RunEv::Message { role: Role::Assistant, text: answer.clone() });
        RunEv::End { answer: Some(answer), todo_left: 0 }
    }
}
