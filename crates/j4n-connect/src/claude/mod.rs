//! Claude Code as a doer (dialect `claude-stream`): a live session kept between turns, so a steer
//! reaches the running turn and a follow-up reuses the same process and session. The questions it asks come back in-band
//! (`--permission-prompt-tool stdio`): the loop's Gate answers the ones it can, and the rest are the operator's. The operator's own allow rules
//! in claude's settings never answer first: the child is started with J4n's `--settings` that ask for every tool the Gate judges (`door`).
//!
//! - [`frames`] reads its stream, [`session`] is the child, [`crate::pool`] keeps the children between turns.
//! - A turn that carries on in a session finds the child in the pool when it was launched the same way (same model, effort,
//!   mode, folder and environment); changing any of them (`Ctl::Retune`) takes the old child out and resumes the
//!   transcript in a new one. A session whose child is gone is resumed from its transcript (`--resume`).
//! - Not here: a one-shot `-p` fallback (a child that cannot stream fails the turn, with what it wrote to stderr), claude's
//!   own spend fuse (the loop's hard cap kills the run), the window meter and `/compact` (with a way to ask the run).

pub mod frames;
pub mod profile;
pub mod session;
pub mod skills;

use crate::pool::{Pool, RELAUNCHED, Reuse, signature};
use crate::proc;
use frames::Turn;
use j4n_core::doer::{Caps, Doer, Probe, Run, RunCtl, RunEv, StartCx};
use j4n_core::gate::{ReadOnly, door};
use j4n_core::place::Launch;
use j4n_core::table::Problem;
use j4n_core::task::{Mode, TaskSpec};
use serde_json::Value;
use session::Session;
use std::collections::BTreeMap;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

/// Said first to a turn that writes in a conversation resumed under new settings.
const MAY_WRITE: &str = "(J4n: this turn may change files and run commands, whatever an earlier turn of this conversation was told.)";

/// The version the flags, the frames and the in-band questions were last checked against, live, on this machine.
pub const VERIFIED_VERSION: &str = "2.1.309";

/// The doer's id, and the name its sessions carry in the Log.
pub const ID: &str = "claude";

/// How long the CLI is given to answer `--version` or `auth status`.
const PROBE_LIMIT: Duration = Duration::from_secs(15);

/// The setting that gives a print-mode child its list tools (`TaskCreate`, `TaskUpdate`, `TaskList`): claude 2.1.310 leaves them to the
/// interactive program and to some models, so the doer's open list (`todo_left`) is empty in a headless run without it.
const TODO_TOOLS: &str = "CLAUDE_CODE_ENABLE_TODO_TOOLS";

/// The tools of the CLI that change files, denied by name in a read task (one comma-separated value: the flag takes a list, and the
/// next flag ends it). `MultiEdit` is an `Edit` to the CLI. Commands are the Gate's: a read task's shell line that writes is refused there.
const READ_DENIES: &str = "Edit,Write,NotebookEdit";

/// Which profile a task is started with: `full` always (what the tool loads by itself; the default until `lean` is
/// measured), `auto` by the task's role (the row's `lean_for` take `lean`, the rest `full`), `lean` always.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pick {
    #[default]
    Full,
    Auto,
    Lean,
}

impl Pick {
    /// The setting's word; a word that is none is told, and `full` stands.
    pub fn parse(w: &str) -> Result<Pick, String> {
        match w {
            "full" => Ok(Pick::Full),
            "auto" => Ok(Pick::Auto),
            "lean" => Ok(Pick::Lean),
            _ => Err(format!("the setting `profile` is `{w}`, which is not full, auto or lean: full stands")),
        }
    }
}

const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// How claude is started. The defaults are built in; a manifest row will say the same in data.
#[derive(Debug, Clone)]
pub struct Config {
    pub program: String,
    /// Added to the environment the child inherits (an account's token, `CLAUDE_CODE_OAUTH_TOKEN`).
    pub env: BTreeMap<String, String>,
    /// The flags of a read task and of a task that may change things. Writing is `default`, not `acceptEdits`, and `argv` gives the child J4n's own
    /// `--settings` ([`door`]): the tools that change things or reach out are *asked* whatever `permissions.allow` the operator's own files hold (an ask
    /// beats an allow in the CLI), and the credential paths are denied for reading. So every write, every command and every web call comes to the Gate as a
    /// question and its floors bind claude's own tools too. What stays out of its reach: a tool the CLI never asks about (a read in the project that
    /// is no credential), a managed policy of the machine, and a hook of the operator's that answers for the CLI.
    /// Reading is `default` too, with the tools that write denied by name, never the CLI's `plan` permission mode: under `plan` the
    /// CLI runs another model than the one asked for the settings `haiku` and `opusplan` (haiku was asked, sonnet was served and billed).
    pub read: Vec<String>,
    pub write: Vec<String>,
    /// Where its login and its own state live (`~/.claude`): a box binds them.
    pub login: Vec<String>,
    /// J4n's own program, which has `relay`: a remote shell of a lane is run inside it. None when this is not J4n (a test): remote lines run as they are.
    pub relay: Option<String>,
    /// What a `full` and a `lean` child are started with besides the above, and the roles that take `lean` under `auto`.
    pub full: crate::manifest::Profile,
    pub lean: crate::manifest::Profile,
    pub lean_for: Vec<String>,
    pub pick: Pick,
    /// The setting `doer.scope` is `project`: every child is also started with `project`.
    pub scoped: bool,
    pub project: crate::manifest::Profile,
    /// Children kept at once, and how long an idle one is kept (the plan's numbers).
    pub live_max: usize,
    pub idle: Duration,
    /// How long a child may stay mute after it has been sent its message.
    pub init_timeout: Duration,
}

impl Default for Config {
    fn default() -> Config {
        let mode = |m: &str| vec!["--permission-mode".to_string(), m.to_string()];
        Config {
            program: "claude".into(),
            env: BTreeMap::new(),
            read: [mode("default"), vec!["--disallowedTools".to_string(), READ_DENIES.to_string()]].concat(),
            write: mode("default"),
            login: Vec::new(),
            relay: std::env::current_exe().ok().filter(|p| p.file_stem().is_some_and(|n| n == "j4n")).map(|p| p.display().to_string()),
            full: Default::default(),
            lean: Default::default(),
            lean_for: Vec::new(),
            pick: Pick::Full,
            scoped: false,
            project: Default::default(),
            live_max: 3,
            idle: Duration::from_secs(600),
            init_timeout: Duration::from_secs(30),
        }
    }
}

/// The tool list of a chat (`input.tools`) as the CLI takes it: `--tools` limits its built-in set to the allowed names, and the denied names are added to the
/// ones the mode already denies (a second `--disallowedTools` would replace the first, and a read chat would lose its own). No list: the arguments are untouched.
fn tool_args(argv: &mut Vec<String>, spec: &TaskSpec) {
    let list = |k: &str| -> Vec<String> { spec.input.get("tools").and_then(|t| t.get(k)).and_then(|a| a.as_array()).into_iter().flatten().filter_map(|v| v.as_str()).filter(|t| !t.starts_with('-') && !t.contains([',', ' '])).map(String::from).collect() };
    let (allow, deny) = (list("allow"), list("deny"));
    if !allow.is_empty() {
        argv.extend(["--tools".to_string(), allow.join(",")]);
    }
    if deny.is_empty() {
        return;
    }
    match argv.iter().position(|a| a == "--disallowedTools") {
        Some(at) if at + 1 < argv.len() => argv[at + 1] = format!("{},{}", argv[at + 1], deny.join(",")),
        _ => argv.extend(["--disallowedTools".to_string(), deny.join(",")]),
    }
}

impl Config {
    /// The row `claude` of the connectors, with the environment its secrets made.
    pub fn from_connector(c: &crate::manifest::Connector, env: BTreeMap<String, String>) -> Config {
        let mut cfg = Config { program: c.program().to_string(), env, login: c.login.clone(), ..Config::default() };
        if let Some(read) = c.modes.get("read") {
            cfg.read = read.clone();
        }
        if let Some(write) = c.modes.get("write") {
            cfg.write = write.clone();
        }
        (cfg.full, cfg.lean, cfg.lean_for, cfg.pick) = (c.profile.full.clone(), c.profile.lean.clone(), c.profile.lean_for.clone(), c.pick);
        (cfg.scoped, cfg.project) = (c.scoped, c.profile.project.clone());
        cfg
    }
}

pub struct Claude {
    cfg: Config,
    pool: Arc<Pool<Session>>,
}

impl Claude {
    pub fn new(mut cfg: Config) -> Claude {
        cfg.env.entry(TODO_TOOLS.into()).or_insert_with(|| "1".into());
        if let Some(relay) = cfg.relay.clone() {
            cfg.env.entry(frames::RELAY_ENV.into()).or_insert(relay);
        }
        let pool = Arc::new(Pool::new(cfg.live_max, cfg.idle));
        Claude { cfg, pool }
    }

    /// The profile a task begins with: `lean` when the operator said so, or under `auto` when its role is one the row names.
    fn begins_with(&self, spec: &TaskSpec) -> profile::Which {
        let lean = match self.cfg.pick {
            Pick::Full => false,
            Pick::Lean => true,
            Pick::Auto => spec.role.as_ref().is_some_and(|r| self.cfg.lean_for.contains(r)),
        };
        if lean { profile::Which::Lean } else { profile::Which::Full }
    }

    /// The profile a child is started with. A session is started with the one it begins with and keeps it: a line that carries on in a
    /// session takes the profile the session was begun with, whatever the setting and the line's role say now, because the CLI records
    /// the system prompt on a session's first request and reuses it. A session no record knows began before profiles.
    fn which(&self, spec: &TaskSpec, place: &std::path::Path, resume: Option<&str>) -> profile::Which {
        match resume {
            Some(id) => profile::recorded(place, id).unwrap_or(profile::Which::Full),
            None => self.begins_with(spec),
        }
    }

    fn profile(&self, which: profile::Which) -> &crate::manifest::Profile {
        if which == profile::Which::Lean { &self.cfg.lean } else { &self.cfg.full }
    }

    /// The environment of a child: the connector's, then its profile's.
    fn env(&self, which: profile::Which) -> BTreeMap<String, String> {
        let mut env = self.cfg.env.clone();
        env.extend(self.profile(which).env.clone());
        if self.cfg.scoped {
            env.extend(self.cfg.project.env.clone());
        }
        env
    }

    /// The arguments of a child for a task: streaming in and out, the questions in-band, then the task's settings.
    fn argv(&self, spec: &TaskSpec, skills: Option<&std::path::Path>, which: profile::Which) -> Vec<String> {
        let mut argv = vec![self.cfg.program.clone()];
        argv.extend(
            ["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose", "--permission-prompt-tool", "stdio", "--include-partial-messages"]
                .map(String::from),
        );
        argv.extend(if spec.tune.mode == Mode::Read { self.cfg.read.clone() } else { self.cfg.write.clone() });
        tool_args(&mut argv, spec);
        // the operator's own allow rules (user, project, local settings) answer before the prompt tool does; these settings ask for what the Gate judges
        // and deny the credentials, and a deny or an ask beats an allow. The sources are kept (CLAUDE.md, hooks, model, plugins): only the answer is taken back.
        // the child's own config folder (an operator's `CLAUDE_CONFIG_DIR`, or the process's) is asked about for its projects, as `~/.claude` is
        let config = self.env(which).get("CLAUDE_CONFIG_DIR").cloned().or_else(|| std::env::var("CLAUDE_CONFIG_DIR").ok());
        argv.extend(["--settings".to_string(), door::claude_settings_in(config.as_deref())]);
        if let Some(model) = spec.tune.model.as_ref().filter(|m| !m.is_empty()) {
            argv.extend(["--model".into(), model.clone()]);
        }
        if let Some(effort) = spec.tune.effort.as_ref().filter(|e| EFFORTS.contains(&e.as_str())) {
            argv.extend(["--effort".into(), effort.clone()]);
        }
        if let Some(dir) = skills {
            argv.extend(["--plugin-dir".into(), dir.to_string_lossy().into_owned()]);
        }
        argv.extend(self.profile(which).args.clone());
        if self.cfg.scoped {
            argv.extend(self.cfg.project.args.clone());
        }
        argv
    }

    /// Kill every child the pool holds (the front end is going away).
    pub fn shutdown(&self) {
        self.pool.kill_all();
    }

    /// How many children are alive between and during turns.
    pub fn live(&self) -> usize {
        self.pool.alive()
    }

    /// What the CLI prints to stdout for `args`, if it ends well within a few seconds.
    fn ask(&self, args: &[&str]) -> Option<String> {
        let argv = std::iter::once(self.cfg.program.as_str()).chain(args.iter().copied()).map(String::from).collect();
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir());
        let (text, exit) = proc::capture(&Launch::new(argv, cwd, self.cfg.env.clone()), PROBE_LIMIT).ok()?;
        (exit.code == Some(0)).then_some(text)
    }
}

impl Drop for Claude {
    fn drop(&mut self) {
        self.pool.kill_all();
    }
}

/// `(logged in, words)` from `claude auth status`'s JSON, whether an API key is in the environment, and the CLI's version.
/// Three things the operator should know before a task spends anything: how it is logged in, that an exported
/// `ANTHROPIC_API_KEY` makes `claude -p` bill per token instead of the subscription, and that the CLI has moved past the
/// version this was checked against.
pub fn describe(auth: Option<&str>, api_key_in_env: bool, version: Option<&str>) -> (bool, String) {
    let parsed = auth.and_then(|t| serde_json::from_str::<Value>(t).ok());
    if !parsed.as_ref().and_then(|v| v.get("loggedIn")).and_then(Value::as_bool).unwrap_or(false) {
        return (false, "not logged in — Agents › Setup › Agent › claude: s signs it in".into());
    }
    let method = parsed.as_ref().and_then(|v| v.get("authMethod")).and_then(Value::as_str).unwrap_or("unknown");
    let mut detail = format!("logged in ({method})");
    if api_key_in_env {
        detail.push_str(" · WARNING: ANTHROPIC_API_KEY is set — claude -p may bill that key per token, not your subscription");
    }
    if let Some(v) = version.and_then(|v| v.split_whitespace().next()).filter(|v| *v != VERIFIED_VERSION) {
        detail.push_str(&format!(" · version {v} differs from the verified {VERIFIED_VERSION}"));
    }
    (true, detail)
}

impl Doer for Claude {
    fn id(&self) -> &str {
        ID
    }

    fn probe(&self) -> Probe {
        let version = self.ask(&["--version"]);
        let auth = self.ask(&["auth", "status"]);
        let key = self.cfg.env.get("ANTHROPIC_API_KEY").cloned().or_else(|| std::env::var("ANTHROPIC_API_KEY").ok()).is_some_and(|v| !v.is_empty());
        let (authed, detail) = describe(auth.as_deref(), key, version.as_deref());
        Probe { installed: version.is_some(), version, authed, detail }
    }

    fn caps(&self) -> Caps {
        Caps { live: true, resume: true, compact: true, skills: true, tools: true, ..Caps::default() }
    }

    fn shutdown(&self) {
        self.pool.kill_all();
    }

    fn start(&self, cx: StartCx) -> Result<Box<dyn Run>, Problem> {
        let text = cx.text.clone().unwrap_or_else(|| cx.spec.goal.clone());
        // shrinking the session is a command the CLI runs as a turn of its own, told what the summary must keep
        let text = if cx.compacting { format!("/compact {text}") } else { text };
        // a value that looks like a flag would be read by the CLI as one
        if [&cx.spec.tune.model, &cx.spec.tune.effort].into_iter().flatten().any(|v| v.starts_with('-') || v.chars().any(char::is_control)) {
            return Err(Problem::refused(ID, "a model or an effort cannot start with a dash or hold control characters"));
        }
        let dir = cx.site.dir.clone();
        // the skills J4n picked for the task are claude's own for it: a child started with another set is not shared, so the signature names the set, not the folder
        let folder = skills::folder(&dir, &cx.task, &cx.spec).map_err(|e| Problem::refused(ID, format!("the skills of the task could not be put in place: {e}")))?;
        let resume = cx.resume.as_ref().filter(|r| r.doer == ID).map(|r| r.id.clone());
        let which = self.which(&cx.spec, &dir, resume.as_deref());
        let argv = self.argv(&cx.spec, folder.as_deref(), which);
        let mut kept = self.argv(&cx.spec, None, which);
        kept.extend(skills::mark(&cx.spec));
        let env = self.env(which);
        let want = signature(&kept, &dir, &env);
        let turn = Turn { task: cx.task.clone(), dir: dir.clone(), read_only: if cx.spec.tune.mode == Mode::Read { ReadOnly::Explicit } else { ReadOnly::No }, compacting: cx.compacting };
        self.pool.reap(Instant::now());
        let mut note = None;
        let mut session = None;
        match resume.as_deref().map(|id| self.pool.reusable(id, &want, &dir)) {
            Some(Reuse::Live(s)) => {
                s.begin(turn.clone(), cx.out.clone());
                if s.prompt(&text).is_ok() {
                    session = Some(s);
                } else {
                    // it died between the check and the line: the transcript is on disk, a new child resumes it
                    s.stop();
                    note = Some("the live session had just ended");
                }
            }
            Some(Reuse::Busy) => return Err(Problem::refused(ID, "that session is busy with another task")),
            Some(Reuse::Replaced(why)) => note = Some(why),
            Some(Reuse::None) | None => {}
        }
        let session = match session {
            Some(s) => s,
            None => {
                if let Some(why) = note.filter(|_| resume.is_some()) {
                    cx.out.send(RunEv::Notice(format!("the live session was {why}: its conversation is resumed in a new one")));
                }
                // a conversation resumed under new settings remembers what its earlier turns were told (a planning turn: change nothing); a turn that
                // writes says so, or the model goes on reading only
                let text = if note == Some(RELAUNCHED) && resume.is_some() && cx.spec.tune.mode == Mode::Write && !cx.compacting {
                    format!("{MAY_WRITE}\n\n{text}")
                } else {
                    text
                };
                let mut full = argv.clone();
                full.extend(resume.iter().flat_map(|id| ["--resume".to_string(), id.clone()]));
                let launch = cx.wrap(Launch { login: self.cfg.login.clone(), ..Launch::new(full, dir.clone(), env.clone()) });
                let s = Session::spawn(&launch, want, &env, (resume.is_none() && which == profile::Which::Lean).then_some(dir.clone()))
                    .map_err(|e| Problem::refused(ID, format!("could not start `{}`: {e}", self.cfg.program)))?;
                self.pool.insert(s.clone());
                s.begin(turn, cx.out.clone());
                if let Err(e) = s.prompt(&text) {
                    s.stop();
                    return Err(Problem::refused(ID, e));
                }
                watch(Arc::downgrade(&s), self.cfg.init_timeout);
                s
            }
        };
        Ok(Box::new(ClaudeRun { session }))
    }
}

/// A child that stays mute after it was sent its message is dead weight: fail the turn and take it down.
fn watch(session: Weak<Session>, timeout: Duration) {
    std::thread::spawn(move || {
        let end = Instant::now() + timeout;
        while Instant::now() < end {
            match session.upgrade() {
                Some(s) if !s.is_ready() && s.in_turn() => {}
                _ => return,
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if let Some(s) = session.upgrade().filter(|s| !s.is_ready() && s.in_turn()) {
            s.give_up(format!("claude said nothing for {} seconds after it was asked", timeout.as_secs()));
        }
    });
}

struct ClaudeRun {
    session: Arc<Session>,
}

impl Run for ClaudeRun {
    fn ctl(&mut self, ctl: RunCtl) -> Result<(), Problem> {
        let said = match ctl {
            RunCtl::Steer(text) => self.session.steer(&text),
            RunCtl::Answer { local, answer } => self.session.answer(&local, &answer),
            RunCtl::Told { local, why } => {
                self.session.told(&local, why);
                Ok(())
            }
            // claude has no abort: the child goes (the pool drops it at the next start), the transcript stays, and a new run
            // resumes it
            RunCtl::Abort | RunCtl::Kill => {
                self.session.stop();
                Ok(())
            }
        };
        said.map_err(|e| Problem::refused(ID, e))
    }
}
