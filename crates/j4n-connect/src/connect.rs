//! The doers of the Connect flow (pack `connect`): what J4n does to connect a program it has never seen,
//! with no change to its code. The flow is data (`packs/connect/flows/connect.toml`); these are the four steps that are not a
//! model's:
//!
//! - **`connect-probe`**: finds the program, asks it `--version` and `--help` (and the manual, when there is one) behind the
//!   place's walls, and puts what it said through the `sniff` rows, the rules that say how a program speaks (`--acp` is `acp`,
//!   `--json` is `jsonl-map`, anything else is `text`). Where the rows cannot tell what it *is* (an agent you talk with, or a
//!   command run for its output) it asks, at the point `connect:is`.
//! - **`connect-check`**: reads the draft a model wrote, into the closed schema: one connector, the program that was probed and no
//!   other, a row its dialect accepts, a canary to run, and a write the Table would take. What is wrong is the answer, so the
//!   drafter can mend it.
//! - **`connect-canary`**: starts the draft as a doer of its own, in the same place, and has it say a word (an agent) or run a
//!   harmless example (a tool). Its questions to the Gate come out as the canary's own.
//! - **`connect-propose`**: makes the Proposal that shows the manifest and the canary's transcript; a yes writes the connector.
//!
//! Each step is a doer, so each has its own task, its place and its Gate like any other.

pub use j4n_core::text::clip;
use crate::manifest::{Connector, Is, Reach};
use crate::{manifest, proc};
use j4n_core::ask::{Answer, AskShape, AskSpec, Opt};
use j4n_core::change::{Actor, Where};
use j4n_core::doer::{Caps, Doer, Emit, Probe, Run, RunCtl, RunEv, StartCx};
use j4n_core::eval::Scope;
use j4n_core::expr::Expr;
use j4n_core::gate::{Action, ReadOnly, Verb};
use j4n_core::place::Launch;
use j4n_core::proposal::{Draft, Proposer, Step, StepOp};
use j4n_core::table::{Kind, Problem, Tables};
use j4n_core::task::Mode;
use j4n_core::value::Value;
use serde::Deserialize;
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

pub const PROBE: &str = "connect-probe";
pub const LEARN: &str = "connect-learn";
pub const CHECK: &str = "connect-check";
pub const CANARY: &str = "connect-canary";
pub const PROPOSE: &str = "connect-propose";

/// How long the program is given for each of its answers.
const ASK_SECS: u64 = 8;
/// How long the canary is given to say its word.
const CANARY_SECS: u64 = 90;
/// How much of what a program says is kept.
const HELP_CHARS: usize = 24_000;
const MAN_CHARS: usize = 8_000;
const TRANSCRIPT_CHARS: usize = 3_000;
/// The word the canary of an agent is asked to say, and looks for.
const READY: &str = "ready";

/// An entry of kind `sniff`: one rule for how a program speaks, read from what it says about itself.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sniff {
    pub id: String,
    #[serde(default)]
    pub doc: String,
    pub when: String,
    pub is: String,
    pub dialect: String,
    #[serde(default)]
    pub rank: i64,
    #[serde(default)]
    pub ask: bool,
    #[serde(default)]
    pub start: Vec<String>,
    #[serde(default)]
    pub note: String,
}

impl Kind for Sniff {
    const NAME: &'static str = "sniff";
}

/// Where the Proposer is found once the front has been made, because the front is made of the doers; and who looks at an `mcp`
/// server for the canary.
#[derive(Clone, Default)]
pub struct Link(Arc<OnceLock<Arc<Proposer>>>, Arc<OnceLock<Arc<dyn Reach>>>, Arc<AtomicBool>);

impl Link {
    pub fn set(&self, proposer: Arc<Proposer>) {
        let _ = self.0.set(proposer);
    }

    pub fn set_reach(&self, reach: Arc<dyn Reach>) {
        let _ = self.1.set(reach);
    }

    /// The setting `connect.auto`: the probe takes the best guess of the rows where it would ask what a program is.
    pub fn set_auto(&self, on: bool) {
        self.2.store(on, Ordering::SeqCst);
    }

    pub fn get(&self) -> Result<&Arc<Proposer>, String> {
        self.0.get().ok_or_else(|| "proposals are not wired to the Connect flow".to_string())
    }
}

/// The rows, in the order they are tried: by `rank`, then by id.
fn load(tables: &Tables) -> (Vec<Sniff>, Vec<Problem>) {
    let (mut rows, mut found) = (Vec::new(), Vec::new());
    for id in tables.ids(Sniff::NAME) {
        match tables.get::<Sniff>(id) {
            Ok(row) if Expr::parse(&row.when).is_err() => found.push(Problem::refused(format!("sniff {id}"), format!("`when = {}` is not an expression", row.when))),
            Ok(row) => rows.push(row),
            Err(p) => found.push(p),
        }
    }
    rows.sort_by(|a, b| (a.rank, &a.id).cmp(&(b.rank, &b.id)));
    (rows, found)
}

/// The doers of the flow, and the rows that were not usable.
pub fn doers(tables: &Tables, link: &Link) -> (Vec<Arc<dyn Doer>>, Vec<Problem>) {
    let (rows, found) = load(tables);
    let rows = Arc::new(rows);
    let link = link.clone();
    let step = |id: &'static str, doc: &'static str, work: Work| Arc::new(StepDoer { id, doc, work }) as Arc<dyn Doer>;
    let (a, b, c, d, auto, e) = (rows.clone(), link.clone(), link.clone(), link.clone(), link.clone(), link.clone());
    let doers = vec![
        step(PROBE, "finds a program and learns how it speaks", Arc::new(move |job| probe(job, &a, &auto))),
        step(LEARN, "learns how a program is given a prompt and how it answers, by trying it", Arc::new(move |job| crate::learn::learn(job, &e))),
        step(CHECK, "reads a drafted connector into the closed schema", Arc::new(move |job| check(job, &b))),
        step(CANARY, "runs a drafted connector once", Arc::new(move |job| canary(job, &d))),
        step(PROPOSE, "proposes a checked connector", Arc::new(move |job| propose(job, &c))),
    ];
    (doers, found)
}

// ---- the shape of a step ----

/// What a step is given: the task's words, its place's walls, and the way the loop talks to it.
pub struct Job {
    pub cx: StartCx,
    pub goal: String,
    pub with: BTreeMap<String, String>,
    pub rx: Receiver<RunCtl>,
}

impl Job {
    pub fn out(&self) -> &Emit {
        &self.cx.out
    }

    pub fn with(&self, name: &str) -> &str {
        self.with.get(name).map_or("", String::as_str)
    }

    /// Put the connecting of `line` to the Gate and wait for the yes; an `Err` when it was refused or the run was stopped. The
    /// verb is `connect`, not `shell`: a program J4n has not met cannot be told to read only, and what it may do is what the walls
    /// of the place say.
    pub fn approve(&self, local: &str, question: String, line: &str) -> Result<(), String> {
        self.approve_as(local, question, line, Verb::Connect, None)
    }

    /// The same for another verb, and for a path, whether it lies inside the project.
    pub fn approve_as(&self, local: &str, question: String, line: &str, verb: Verb, inside: Option<bool>) -> Result<(), String> {
        let mut act = Action::new(verb, Actor::Doer(self.cx.spec.doer.clone()), line).task(&self.cx.task);
        act.site = Some(self.cx.site.dir.display().to_string());
        act.inside = inside;
        act.read_only = if self.cx.spec.tune.mode == Mode::Read { ReadOnly::Explicit } else { ReadOnly::No };
        let spec = AskSpec { local: local.into(), question: clip(&question, 400), shape: AskShape::Approval { action: clip(line, 200), risk: 40 }, inband: true, point: None, act: Some(Box::new(act)) };
        match self.ask(spec)? {
            Answer::Yes => Ok(()),
            _ => Err("this was not allowed".into()),
        }
    }

    /// Raise a question and wait for its answer.
    pub fn ask(&self, spec: AskSpec) -> Result<Answer, String> {
        let local = spec.local.clone();
        self.out().send(RunEv::Ask(spec));
        loop {
            match self.rx.recv() {
                Ok(RunCtl::Answer { local: l, answer }) if l == local => return Ok(answer),
                Ok(RunCtl::Abort | RunCtl::Kill) | Err(_) => return Err("stopped".into()),
                Ok(_) => {}
            }
        }
    }

    /// A command line of a try: the program with a cleared environment, both streams in one, behind the place's walls; with `fresh`, in a
    /// folder of its own that is removed after (see [`Job::trial`]).
    pub(crate) fn launch(&self, argv: &[String], env: &[(String, String)], fresh: bool) -> Launch {
        let mut line: Vec<String> = vec!["env".into(), "-i".into()];
        for (k, fallback) in [("PATH", "/usr/local/bin:/usr/bin:/bin"), ("HOME", "/"), ("LANG", "C.UTF-8")] {
            line.push(format!("{k}={}", std::env::var(k).unwrap_or_else(|_| fallback.into())));
        }
        line.extend(["TERM=dumb".into(), "NO_COLOR=1".into()]);
        line.extend(env.iter().map(|(k, v)| format!("{k}={v}")));
        let script = if fresh { "exec 2>&1; d=$(mktemp -d) || exec \"$@\"; cd \"$d\" || exec \"$@\"; \"$@\"; c=$?; cd /; rm -rf \"$d\"; exit $c" } else { "exec 2>&1; exec \"$@\"" };
        line.extend(["sh".into(), "-c".into(), script.into(), "sh".into()]);
        line.extend(argv.iter().cloned());
        self.cx.wrap(Launch::new(line, self.cx.site.dir.clone(), BTreeMap::new()))
    }

    /// The program run behind the place's walls with a cleared environment, both streams in one, until `secs` are over.
    fn capture(&self, argv: &[&str], secs: u64) -> Option<(String, Option<i32>)> {
        self.trial(&argv.iter().map(|a| a.to_string()).collect::<Vec<_>>(), None, &[], secs, false)
    }

    /// The same, with words on its standard input and variables of its own (`learn`: the ways a program may be given a prompt).
    /// With `fresh` the program runs in a folder made for the one run and removed after it: what it keeps there (a session, a count of its
    /// requests) is not the project's, and a try does not spend what the next run of the program would have.
    /// A run with nothing given, for at most `secs`: what it printed (so far, when it ran over), whether it ran over, and how it ended.
    pub(crate) fn trial_out(&self, argv: &[String], secs: u64) -> Option<(String, bool, Option<i32>)> {
        let (text, exit) = proc::capture(&self.launch(argv, &[], true), Duration::from_secs(secs)).ok()?;
        Some((text, exit.killed, exit.code))
    }

    pub(crate) fn trial(&self, argv: &[String], stdin: Option<&str>, env: &[(String, String)], secs: u64, fresh: bool) -> Option<(String, Option<i32>)> {
        let launch = self.launch(argv, env, fresh);
        let Some(words) = stdin else {
            let (text, exit) = proc::capture(&launch, Duration::from_secs(secs)).ok()?;
            // a program that fails with nothing on its standard output says why on its standard error (a sign-in it wants, a usage)
            let text = if text.trim().is_empty() && exit.code != Some(0) { exit.tail.trim().to_string() } else { text };
            return (!exit.killed).then_some((text, exit.code));
        };
        let out = Arc::new(std::sync::Mutex::new(String::new()));
        let (tx, rx) = mpsc::channel();
        let sink = out.clone();
        let (child, pipe) = proc::spawn(&launch, true, move |l| {
            let mut s = sink.lock().unwrap_or_else(|e| e.into_inner());
            s.push_str(l);
            s.push('\n');
        }, move |e| drop(tx.send(e))).ok()?;
        if let Some(mut pipe) = pipe {
            use std::io::Write;
            let _ = writeln!(pipe, "{words}").and_then(|_| pipe.flush());
        }
        let exit = match rx.recv_timeout(Duration::from_secs(secs)) {
            Ok(e) => e,
            Err(_) => {
                child.kill();
                rx.recv().ok()?
            }
        };
        let text = out.lock().unwrap_or_else(|e| e.into_inner()).trim().to_string();
        (!exit.killed).then_some((text, exit.code))
    }
}

pub type Work = Arc<dyn Fn(&Job) -> Result<String, String> + Send + Sync>;

/// A step of the flow as a doer: its work runs on a thread of its own and ends the run with its answer, or with why not.
pub struct StepDoer {
    pub id: &'static str,
    pub doc: &'static str,
    pub work: Work,
}

struct StepRun {
    tx: Sender<RunCtl>,
}

impl Run for StepRun {
    fn ctl(&mut self, ctl: RunCtl) -> Result<(), Problem> {
        self.tx.send(ctl).map_err(|_| Problem::refused("connect", "the run is over"))
    }
}

impl Doer for StepDoer {
    fn id(&self) -> &str {
        self.id
    }

    fn probe(&self) -> Probe {
        Probe { installed: true, version: None, authed: true, detail: self.doc.into() }
    }

    fn caps(&self) -> Caps {
        Caps::default()
    }

    fn code(&self) -> bool {
        true
    }

    fn start(&self, cx: StartCx) -> Result<Box<dyn Run>, Problem> {
        let (tx, rx) = mpsc::channel();
        let goal = cx.text.clone().unwrap_or_else(|| cx.spec.goal.clone());
        let with = cx.spec.input.get("with").and_then(Json::as_object).map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect()).unwrap_or_default();
        let job = Job { cx, goal, with, rx };
        let work = self.work.clone();
        std::thread::spawn(move || {
            let out = job.out().clone();
            match work(&job) {
                Ok(answer) => out.send(RunEv::End { answer: Some(answer), todo_left: 0 }),
                Err(why) => out.send(RunEv::Error(why)),
            };
        });
        Ok(Box::new(StepRun { tx }))
    }
}

// ---- probe ----

/// The program a goal names, as a path: a name is looked for on the PATH, a path (`./bin/agent`) as it stands.
pub fn find(name: &str, cwd: &Path) -> Result<PathBuf, String> {
    if name.is_empty() || name.len() > 200 || name.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("name the program as one word: `connect gemini`, or a path".into());
    }
    let runnable = |p: &Path| {
        use std::os::unix::fs::PermissionsExt;
        p.is_file() && p.metadata().is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
    };
    if name.contains('/') {
        let p = if Path::new(name).is_absolute() { PathBuf::from(name) } else { cwd.join(name) };
        return if runnable(&p) { Ok(p.canonicalize().unwrap_or(p)) } else { Err(format!("`{name}` is not a program that can be run")) };
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| runnable(p)).or_else(|| crate::found::named(name, cwd)).ok_or_else(|| format!("`{name}` is not on the PATH or in the folders programs are put in (~/.local/bin, ~/.cargo/bin, ~/go/bin, ~/.bun/bin, this project's node_modules/.bin, .venv/bin and build folders): give its path"))
}

fn probe(job: &Job, rows: &[Sniff], link: &Link) -> Result<String, String> {
    let name = job.goal.split_whitespace().next().unwrap_or_default();
    let path = find(name, &job.cx.site.dir)?;
    let shown = path.display().to_string();
    job.approve("probe", format!("ask `{name}` what it is (`--version`, `--help`)?"), &format!("{shown} --version --help"))?;
    let first = |attempts: &[&[&str]], good: &dyn Fn(&str, Option<i32>) -> bool| -> String {
        for a in attempts {
            let mut argv = vec![shown.as_str()];
            argv.extend(a.iter().copied());
            if let Some((text, code)) = job.capture(&argv, ASK_SECS)
                && good(&text, code)
            {
                return clip(text.trim(), HELP_CHARS);
            }
        }
        String::new()
    };
    let version = first(&[&["--version"], &["-V"], &["version"]], &|t, c| c == Some(0) && !t.trim().is_empty());
    let help = first(&[&["--help"], &["-h"], &["help"]], &|t, c| {
        let low = t.to_lowercase();
        !t.trim().is_empty() && (c == Some(0) || low.contains("usage") || low.contains("options"))
    });
    // the manual of a name; a path is not a name, and `man` would read the file it points at
    let man = Some(name).filter(|n| !n.contains('/')).and_then(|n| job.capture(&["man", "-P", "cat", n], ASK_SECS)).filter(|(t, c)| *c == Some(0) && !t.trim().is_empty()).map(|(t, _)| clip(t.trim(), MAN_CHARS)).unwrap_or_default();
    let version_line = version.lines().next().unwrap_or_default().to_string();
    let text = format!("{help}\n{man}\n{version}").to_lowercase();
    let scope = Scope::new().with("program", Value::text(name)).with("help", Value::text(&help)).with("version", Value::text(&version)).with("man", Value::text(&man)).with("text", Value::text(&text));
    let matched: Vec<&Sniff> = rows.iter().filter(|r| Expr::parse(&r.when).is_ok_and(|e| e.holds(&scope))).collect();
    let top = matched.first().ok_or("none of the `sniff` rows fits what it says: write its connector by hand")?;
    let mut is = top.is.clone();
    let mut by = "rule";
    if top.ask && link.2.load(Ordering::SeqCst) {
        by = "auto"; // `connect.auto`: the best guess of the rows stands, and the facts say so
    } else if top.ask {
        let options = ["agent", "tool"].map(|id| Opt { id: id.into(), label: id.into(), recommended: id == top.is });
        let short = Path::new(name).file_name().map_or(name, |n| n.to_str().unwrap_or(name));
        let question = format!("`{short}` speaks `{}`. Is it an agent you talk with, or a command run for its output?", top.dialect);
        let spec = AskSpec { local: "is".into(), question, shape: AskShape::Choice { options: options.to_vec() }, inband: true, point: Some("connect:is".into()), act: None };
        match job.ask(spec)? {
            Answer::Pick { id } if id == "agent" || id == "tool" => {
                is = id;
                by = "ask";
            }
            _ => return Err("it was not said whether it is an agent or a tool".into()),
        }
    }
    let facts = json!({
        "program": name, "path": shown, "found": true, "version": version_line, "help": help, "man": man,
        "class": { "id": top.id, "is": is, "dialect": top.dialect, "why": top.doc, "start": top.start, "note": top.note, "by": by },
        "candidates": matched.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
    });
    Ok(facts.to_string())
}

// ---- check ----

/// The connector's TOML out of what a drafter said: a fenced block when there is one, else the text from its first `[[connector]]`
/// or its first `key = value` line.
pub fn toml_of(text: &str) -> String {
    let mut fenced: Option<String> = None;
    let mut open = false;
    for line in text.lines() {
        let l = line.trim_start();
        if l.starts_with("```") {
            if open {
                break;
            }
            open = l.trim_start_matches('`').trim().eq_ignore_ascii_case("toml") || l.trim_start_matches('`').trim().is_empty();
            if open {
                fenced = Some(String::new());
            }
            continue;
        }
        if open && let Some(f) = fenced.as_mut() {
            f.push_str(line);
            f.push('\n');
        }
    }
    if let Some(f) = fenced.filter(|f| !f.trim().is_empty()) {
        return f;
    }
    let from = text.lines().position(|l| l.trim_start().starts_with("[[connector]]") || (l.contains(" = ") && !l.trim_start().starts_with('#'))).unwrap_or(0);
    text.lines().skip(from).collect::<Vec<_>>().join("\n")
}

/// The one connector entry of a draft, as a table.
fn entry_of(text: &str) -> Result<toml::Table, String> {
    let mut table: toml::Table = text.parse().map_err(|e: toml::de::Error| format!("it is not TOML: {}", e.message()))?;
    match table.remove("connector") {
        Some(toml::Value::Array(rows)) => match <[toml::Value; 1]>::try_from(rows) {
            Ok([toml::Value::Table(t)]) if table.is_empty() => Ok(t),
            _ => Err("it holds one `[[connector]]` and nothing else".into()),
        },
        Some(_) => Err("`connector` is written `[[connector]]`".into()),
        None => Ok(table),
    }
}

pub(crate) fn id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.starts_with(|c: char| c.is_ascii_lowercase()) && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// The login folders a draft may name: a hidden file or folder directly in the home, and none of those that hold credentials or
/// the shell's own startup.
const LOGIN_DENIED: [&str; 18] = [".ssh", ".gnupg", ".aws", ".azure", ".kube", ".docker", ".netrc", ".git-credentials", ".config", ".local", ".cache", ".j4n", ".bashrc", ".zshrc", ".profile", ".bash_profile", ".zprofile", ".npmrc"];

fn login_ok(entry: &str) -> bool {
    entry.strip_prefix("~/").is_some_and(|rest| rest.len() > 1 && rest.starts_with('.') && !rest.contains('/') && !rest.contains("..") && !LOGIN_DENIED.contains(&rest))
}

/// The variables that change what a program loads or where it looks: a draft does not set them.
fn env_denied(name: &str) -> bool {
    name.starts_with("LD_") || name.starts_with("DYLD_") || ["PATH", "HOME", "NODE_OPTIONS", "BASH_ENV", "ENV", "PYTHONSTARTUP", "PYTHONPATH", "PERL5OPT", "RUBYOPT"].contains(&name)
}

/// What is wrong with a draft, in words a drafter can act on, before the Table is asked; the entry when it can be read at all.
pub fn concerns(draft: &str, program: &str, chosen: (&str, &str)) -> (Vec<String>, Option<toml::Table>) {
    let mut bad = Vec::new();
    let mut entry = match entry_of(&toml_of(draft)) {
        Ok(e) => e,
        Err(why) => return (vec![why], None),
    };
    let id = entry.get("id").and_then(toml::Value::as_str).unwrap_or_default().to_string();
    if !id_ok(&id) {
        bad.push("`id` is lower-case letters, digits, `-` and `_`, and begins with a letter".into());
    }
    match entry.get("program").and_then(toml::Value::as_str) {
        None => {
            entry.insert("program".into(), toml::Value::String(program.into()));
        }
        Some(p) if p != program => bad.push(format!("`program` is `{p}`, and the program that was looked at is `{program}`: a connector made here runs that program and no other")),
        Some(_) => {}
    }
    for forbidden in ["secrets", "key", "url", "proxy", "signin"] {
        if entry.contains_key(forbidden) {
            bad.push(format!("`{forbidden}` is not for a program that is run: leave it out"));
        }
    }
    let conn: Result<Connector, String> = serde_json::to_value(&entry).map_err(|e| e.to_string()).and_then(|j| serde_json::from_value(j).map_err(|e| e.to_string()));
    match conn {
        Err(why) => bad.push(format!("a field is not one the connector has, or has the wrong shape: {why}")),
        Ok(c) => {
            if let Err(why) = c.check() {
                bad.push(why);
            }
            if c.is.as_str() != chosen.0 || c.dialect.word() != chosen.1 {
                bad.push(format!("it is `is = \"{}\"` with the dialect `{}`, and it was found to be `{}` with `{}`: say that", c.is.as_str(), c.dialect.word(), chosen.0, chosen.1));
            }
            if c.is == Is::Tool && c.canary.as_ref().is_none_or(|k| k.goal.trim().is_empty() && k.argv.is_empty()) {
                bad.push("a tool needs `canary = { goal = \"…\" }`: a harmless example of its arguments to run once".into());
            }
            // a command line made here is the program's, whatever else its words say
            for (what, argv) in [("argv", &c.argv), ("probe.argv", &c.probe.argv), ("probe.auth", &c.probe.auth)] {
                if let Some(first) = argv.first().filter(|f| f.as_str() != program) {
                    bad.push(format!("`{what}` begins with `{first}`, and the program that was looked at is `{program}`: a command line made here runs that program and no other"));
                }
            }
            if let Some(l) = c.login.iter().find(|l| !login_ok(l)) {
                bad.push(format!("`login` names `{l}`: only a hidden file or folder directly in the home that holds the program's own login (`~/.gemini`) is opened for writing, never one that holds credentials or startup files"));
            }
            if let Some(k) = c.env.keys().find(|k| env_denied(k)) {
                bad.push(format!("`env` sets `{k}`, which changes what a program loads or where it looks: leave it out"));
            }
        }
    }
    (bad, Some(entry))
}

/// [`concerns`], and whether the Table would take a write of the draft.
pub(crate) fn judge(draft: &str, program: &str, chosen: (&str, &str), link: &Link) -> (Vec<String>, Option<toml::Table>) {
    let (mut bad, entry) = concerns(draft, program, chosen);
    if let (true, Some(entry)) = (bad.is_empty(), entry.as_ref()) {
        let id = entry.get("id").and_then(toml::Value::as_str).unwrap_or_default();
        if let Err(why) = link.get().and_then(|p| p.vet(&draft_of(id, entry, "", "")).map_err(|p| p.msg)) {
            bad.push(why);
        }
    }
    (bad, entry)
}

fn draft_of(id: &str, entry: &toml::Table, why: &str, transcript: &str) -> Draft {
    let program = entry.get("program").and_then(toml::Value::as_str).unwrap_or_default();
    let dialect = entry.get("dialect").and_then(toml::Value::as_str).unwrap_or_default();
    // the id names the entry; it is not one of its fields
    let mut fields = entry.clone();
    fields.remove("id");
    let why = if why.is_empty() { format!("a connector for `{program}`") } else { format!("{why}{}", if transcript.is_empty() { String::new() } else { format!("\n\nThe canary:\n{transcript}") }) };
    Draft {
        // a connector learned again has the same id and another row: it is another proposal, and the same row again is the one that was made
        key: format!("connect:{id}:{}", &crate::canary::word_of(&toml::to_string(&fields).unwrap_or_default())[..8]),
        title: format!("Connect {id} ({dialect})"),
        why,
        evidence: 1,
        steps: vec![Step { to: Where::User, file: Some(format!("connectors/{id}.toml")), kind: "connector".into(), id: id.to_string(), op: StepOp::Set(fields) }],
        widens: vec![format!("connector {id}: runs `{program}`")],
    }
}

fn check(job: &Job, link: &Link) -> Result<String, String> {
    let (program, draft) = (job.with("program"), job.with("draft"));
    let chosen = (job.with("is"), job.with("dialect"));
    let (concerns, entry) = judge(draft, program, chosen, link);
    let manifest = entry.as_ref().filter(|_| concerns.is_empty()).and_then(|e| toml::to_string(e).ok()).unwrap_or_default();
    Ok(json!({ "pass": concerns.is_empty(), "concerns": concerns, "manifest": manifest }).to_string())
}

// ---- canary ----

/// The doer a draft makes, to be started by the canary: the draft's secrets, if it had any, are refused.
fn doer_of(c: &Connector) -> Result<Arc<dyn Doer>, String> {
    manifest::build(c, &|name, _| Err(format!("{name}: a connector made here carries no secret")))
}

fn canary(job: &Job, link: &Link) -> Result<String, String> {
    canary_of(job, link, job.with("manifest"))
}

/// The canary of a manifest: it is run once, in the walls of the step, and the verdict is said.
pub(crate) fn canary_of(job: &Job, link: &Link, manifest: &str) -> Result<String, String> {
    let entry: toml::Table = manifest.parse().map_err(|e: toml::de::Error| format!("the manifest is not TOML: {}", e.message()))?;
    let c: Connector = serde_json::to_value(&entry).map_err(|e| e.to_string()).and_then(|j| serde_json::from_value(j).map_err(|e| e.to_string()))?;
    if c.is == Is::Mcp {
        return survey(job, link, &c, entry, manifest);
    }
    // a tool learned by trying is tried with a run that has no effect (`--help`): what it learned of its commands is not run for a canary
    if let Some(k) = c.canary.as_ref().filter(|k| c.is == Is::Tool && !k.argv.is_empty()) {
        let mut argv = vec![c.program().to_string()];
        argv.extend(k.argv.iter().cloned());
        let line = argv.join(" ");
        job.approve("canary", format!("run `{}` once, to see that it answers?", clip(&line, 160)), &line)?;
        let (text, code) = job.trial(&argv, None, &[], 15, true).ok_or_else(|| "it did not end in its clock".to_string())?;
        let ok = code == Some(0) && !text.trim().is_empty();
        let transcript = format!("$ {line} (exit {})\n{}", code.map_or("?".into(), |c| c.to_string()), clip(text.trim(), 600));
        return Ok(verdict(ok, if ok { "" } else { "it did not answer with words and exit 0" }, &transcript, manifest));
    }
    let (goal, expect) = match c.canary.as_ref() {
        Some(k) if !k.goal.trim().is_empty() => (k.goal.clone(), k.expect.clone()),
        _ if c.is == Is::Agent => (format!("Reply with the single word: {READY}"), Some(READY.to_string())),
        _ => return Ok(verdict(false, "there is no canary to run", "", manifest)),
    };
    let doer = doer_of(&c)?;
    let line = std::iter::once(c.program().to_string()).chain(c.start.iter().cloned()).chain(c.argv.iter().cloned()).collect::<Vec<_>>().join(" ");
    job.approve("canary", format!("run `{}` once, to see that it works?", clip(&line, 160)), &line)?;
    let (emit, events) = Emit::channel();
    let mut inner = job.cx.nested(&goal, emit);
    // a command cannot be shown to only read, so the step's read-only rule would refuse it whatever it is: it is asked about like
    // any command, and the walls of the place (read-only for this step) are what hold it
    if c.is == Is::Tool {
        inner.spec.tune.mode = Mode::Write;
    }
    let mut run = doer.start(inner).map_err(|p| p.msg)?;
    let mut transcript = String::new();
    let mut said = String::new();
    let mut failure: Option<String> = None;
    let mut ended = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(CANARY_SECS);
    while !ended {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match events.recv_timeout(left.min(Duration::from_millis(200))) {
            Ok(RunEv::Ask(spec)) => {
                // the program's own question is the canary's: the loop's Gate answers it, or the operator does
                let local = spec.local.clone();
                let answer = job.ask(spec)?;
                let _ = run.ctl(RunCtl::Answer { local, answer });
            }
            Ok(RunEv::Text(t)) => said.push_str(&t),
            Ok(RunEv::Message { text, .. }) => {
                transcript.push_str(&format!("{text}\n"));
                said = text;
            }
            Ok(RunEv::Tool { name, detail, end }) => transcript.push_str(&format!("{} {name} {}\n", if end { "←" } else { "→" }, clip(&detail, 200))),
            Ok(RunEv::Shell { cmd, out, exit }) => transcript.push_str(&format!("$ {cmd} (exit {exit})\n{}\n", clip(&out, 400))),
            Ok(RunEv::Notice(n)) => transcript.push_str(&format!("({n})\n")),
            Ok(RunEv::Error(e)) => {
                failure = Some(e);
                ended = true;
            }
            Ok(RunEv::End { answer, .. }) => {
                if let Some(a) = answer.filter(|a| !a.trim().is_empty()) {
                    said = a;
                }
                ended = true;
            }
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => {
                if std::time::Instant::now() >= deadline {
                    failure = Some(format!("it did not finish in {CANARY_SECS} s"));
                    let _ = run.ctl(RunCtl::Kill);
                    ended = true;
                } else if matches!(job.rx.try_recv(), Ok(RunCtl::Abort | RunCtl::Kill)) {
                    let _ = run.ctl(RunCtl::Kill);
                    return Err("stopped".into());
                }
            }
            Err(RecvTimeoutError::Disconnected) => ended = true,
        }
    }
    if transcript.is_empty() || !said.is_empty() && !transcript.contains(&said) {
        transcript.push_str(&format!("{}\n", said.trim()));
    }
    let transcript = clip(transcript.trim(), TRANSCRIPT_CHARS);
    let expected = expect.as_deref().filter(|e| !e.is_empty());
    let (pass, why) = match (&failure, expected) {
        (Some(e), _) => (false, format!("it failed: {}", clip(e, 300))),
        (None, Some(e)) if !said.to_lowercase().contains(&e.to_lowercase()) && !transcript.to_lowercase().contains(&e.to_lowercase()) => (false, format!("it did not say `{e}`")),
        (None, None) if said.trim().is_empty() && transcript.trim().is_empty() => (false, "it said nothing".into()),
        (None, _) => (true, String::new()),
    };
    Ok(verdict(pass, &why, &transcript, manifest))
}

fn verdict(pass: bool, why: &str, transcript: &str, manifest: &str) -> String {
    json!({ "pass": pass, "concerns": if why.is_empty() { json!([]) } else { json!([why]) }, "transcript": transcript, "manifest": manifest }).to_string()
}

/// The canary of an `mcp` server: it is started behind the place's walls, its tools are listed, and the manifest comes back with
/// the pin of each, so that one yes adds the server and offers its tools.
fn survey(job: &Job, link: &Link, c: &Connector, mut entry: toml::Table, manifest: &str) -> Result<String, String> {
    let reach = link.1.get().ok_or("the tools that reach out are not wired to the Connect flow")?;
    let line = std::iter::once(c.program().to_string()).chain(c.start.iter().cloned()).collect::<Vec<_>>().join(" ");
    job.approve("canary", format!("start `{}` and list its tools?", clip(&line, 160)), &line)?;
    let found = match reach.survey(c, &|l| job.cx.wrap(l), &job.cx.site.dir) {
        Ok(f) => f,
        Err(why) => return Ok(verdict(false, &format!("it failed: {}", clip(&why, 300)), "", manifest)),
    };
    let pins: toml::Table = found.tools.iter().map(|(name, hash, _)| (name.clone(), toml::Value::String(hash.clone()))).collect();
    entry.insert("pins".into(), toml::Value::Table(pins));
    let manifest = toml::to_string(&entry).map_err(|e| e.to_string())?;
    Ok(json!({ "pass": true, "concerns": [], "transcript": clip(&found.text, TRANSCRIPT_CHARS), "manifest": manifest }).to_string())
}

// ---- propose ----

/// The arm of an agent that is proposed: the roles the choosers may offer it for. A connector with no `roles` is offered for every role, and this one
/// has proved only that it answers and that its writes go through the Gate, so it is offered for `work` and the operator (or the standing it earns
/// there) adds the rest. The row is what the operator reads before a yes, so the arm is in it.
fn arm(entry: &mut toml::Table) {
    if entry.get("is").and_then(toml::Value::as_str) == Some("agent") && !entry.contains_key("roles") {
        entry.insert("roles".into(), toml::Value::Array(vec![toml::Value::String("work".into())]));
    }
}

fn propose(job: &Job, link: &Link) -> Result<String, String> {
    let (program, facts) = (job.with("program"), job.with("version"));
    let auto = link.2.load(Ordering::SeqCst);
    // a tool with commands is one connector for each, written by one Proposal: the manifests are told apart by a line `---`
    let mut entries: Vec<toml::Table> = Vec::new();
    for text in job.with("manifest").split("\n---\n") {
        let mut entry: toml::Table = text.parse().map_err(|e: toml::de::Error| format!("the manifest is not TOML: {}", e.message()))?;
        // `connect.auto` makes no connector that writes by itself: a command it could not place as an agent is a tool of `risk = "read"`, and each run of
        // a tool is asked of the Gate as ever
        if auto && entry.get("is").and_then(toml::Value::as_str) == Some("tool") {
            entry.insert("risk".into(), toml::Value::String("read".into()));
        }
        // the flow's steps can be changed by the operator's own files: what is proposed is checked here as well
        let (bad, _) = judge(text, program, (job.with("is"), job.with("dialect")), link);
        if !bad.is_empty() {
            return Err(format!("the manifest is not one that is proposed: {}", bad.join("; ")));
        }
        arm(&mut entry);
        entries.push(entry);
    }
    let entry = entries.first().cloned().ok_or("there is no manifest")?;
    let id = entry.get("id").and_then(toml::Value::as_str).filter(|i| id_ok(i)).ok_or("the manifest has no usable `id`")?.to_string();
    let dialect = entry.get("dialect").and_then(toml::Value::as_str).unwrap_or_default();
    let is = entry.get("is").and_then(toml::Value::as_str).unwrap_or_default();
    let many = if entries.len() > 1 { format!(" ({} commands)", entries.len()) } else { String::new() };
    let why = format!("Runs `{program}` as {} over `{dialect}`{many}{}. It was asked once and answered; read the manifest before you say yes.", if is == "agent" { "an agent" } else { "a tool" }, if facts.is_empty() { String::new() } else { format!(" (version {facts})") });
    let mut draft = draft_of(&id, &entry, &why, job.with("transcript"));
    for e in entries.iter().skip(1) {
        let more = draft_of(e.get("id").and_then(toml::Value::as_str).filter(|i| id_ok(i)).ok_or("a manifest has no usable `id`")?, e, &why, "");
        draft.steps.extend(more.steps);
        draft.widens.extend(more.widens);
    }
    let made = link.get()?.make(draft, Actor::Core, None).map_err(|p| p.msg)?;
    // `connect.auto`: the Proposal is skipped. The connector was run in the box and in read mode and its canary passed; its first
    // write still asks the Gate, and a wider trust is still asked for. The second `accept` is the confirmation a widening wants.
    if let (Some(p), true) = (made.as_ref(), link.2.load(Ordering::SeqCst)) {
        let proposer = link.get()?;
        let mut done = false;
        for _ in 0..2 {
            if matches!(proposer.accept(p).map_err(|e| e.msg)?, j4n_core::proposal::Accepted::Done(_)) {
                done = true;
                break;
            }
        }
        if done {
            return Ok(format!("connected {id} ({program}, {dialect}): its canary passed in the box, and `connect.auto` took the proposal {p}."));
        }
    }
    Ok(match made {
        Some(p) => format!("proposed {p}: connect {id} ({program}, {dialect}). `:accept {p}` twice to add it, `:decline {p}` to leave it."),
        None => format!("`{id}` was proposed before and left; nothing new was proposed."),
    })
}
