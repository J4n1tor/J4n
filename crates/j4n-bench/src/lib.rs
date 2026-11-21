//! The bench (pack `bench`): a case is data — files to start from, lines to say, what each answer must hold and a
//! check of the folder at the end — and a run of it is a doer that makes a fresh folder, has the doer under test say each line in
//! turn, and writes down what came of it. The same cases run offline against a script and live against a real agent; what a run
//! costs and whether it passed are events, and the results page is a fold of them.
//!
//! A case that is a hold-out is scored once: it is not run again for the same doer unless the operator says `again`.

use j4n_connect::connect::{Job, StepDoer, clip};
use j4n_core::ask::{Answer, AskShape};
use j4n_core::change::Actor;
use j4n_core::doer::{Doer, Emit, RunCtl, RunEv};
use j4n_core::eval::Scope;
use j4n_core::expr::Expr;
use j4n_core::front::BriefHook;
use j4n_core::log::{Log, New};
use j4n_core::place::Launch;
use j4n_core::table::{Kind, Problem, Tables};
use j4n_core::task::SessionRef;
use j4n_core::value::Value;
use j4n_connect::proc;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

pub const DOER: &str = "bench-case";
/// How long a turn, a setup and a check are each given.
const TURN_SECS: u64 = 600;
const SCRIPT_SECS: u64 = 60;
/// The most files a case starts from, and the most a case says.
const FILES_MAX: usize = 200;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Turn {
    pub say: String,
    /// An expression over `answer` (the doer's last words of the turn), true when the answer is right.
    #[serde(default)]
    pub expect: Option<String>,
}

/// An entry of kind `case`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub doc: String,
    /// Scored once per doer.
    #[serde(default)]
    pub holdout: bool,
    /// The folder is a git repository with one commit when the case starts.
    #[serde(default)]
    pub git: bool,
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    /// A shell script run in the folder before the first turn.
    #[serde(default)]
    pub setup: Option<String>,
    /// A shell script run in the folder after the last turn: exit 0 is a world as asked.
    #[serde(default)]
    pub check: Option<String>,
    /// The doer is given the Brief of a line of `j4n run`; false: the bare doer, as the line is.
    #[serde(default = "yes")]
    pub brief: bool,
    pub turns: Vec<Turn>,
}

fn yes() -> bool {
    true
}

impl Kind for Case {
    const NAME: &'static str = "case";
}

/// The cases there are, and the doers a case can be run with (known once the front is made).
#[derive(Clone)]
pub struct Bench {
    cases: Arc<Vec<Case>>,
    doers: Arc<OnceLock<Vec<Arc<dyn Doer>>>>,
    /// How a line is given its Brief, once the front is made.
    brief: Arc<OnceLock<BriefHook>>,
    base: PathBuf,
    log: Log,
}

fn plain(path: &str) -> bool {
    !path.is_empty() && path.len() <= 200 && Path::new(path).components().all(|c| matches!(c, Component::Normal(_))) && !path.contains('\0')
}

impl Bench {
    /// The cases of the Table; a case that cannot be run is left out and said why. `base` is where work folders are made.
    pub fn load(tables: &Tables, base: &Path, log: Log) -> (Bench, Vec<Problem>) {
        let (mut cases, mut found) = (Vec::new(), Vec::new());
        for id in tables.ids(Case::NAME) {
            let case = match tables.get::<Case>(id) {
                Ok(c) => c,
                Err(p) => {
                    found.push(p);
                    continue;
                }
            };
            let bad = if case.turns.is_empty() {
                Some("a case says at least one line".to_string())
            } else if case.files.len() > FILES_MAX || case.files.keys().any(|p| !plain(p)) {
                Some("a file of a case is a plain relative path inside its folder".into())
            } else {
                case.turns.iter().filter_map(|t| t.expect.as_deref()).find_map(|e| Expr::parse(e).err().map(|err| format!("`expect = {e}` is not an expression: {err}")))
            };
            match bad {
                Some(why) => found.push(Problem::refused(format!("case {id}"), why)),
                None => cases.push(case),
            }
        }
        (Bench { cases: Arc::new(cases), doers: Arc::default(), brief: Arc::default(), base: base.to_path_buf(), log }, found)
    }

    /// The doers a run may be made with, once there are any.
    pub fn set_doers(&self, doers: Vec<Arc<dyn Doer>>) {
        let _ = self.doers.set(doers);
    }

    /// What gives a line of a case its Brief, once the front is made: without it a line goes to the doer as it is.
    pub fn set_brief(&self, hook: BriefHook) {
        let _ = self.brief.set(hook);
    }

    pub fn doer(&self) -> Arc<dyn Doer> {
        let me = self.clone();
        Arc::new(StepDoer { id: DOER, doc: "runs a case of the bench with a doer", work: Arc::new(move |job| me.run(job)) })
    }

    /// Whether the Log has a result of this case for this doer.
    fn scored(&self, case: &str, doer: &str) -> bool {
        self.log.sync();
        self.log.read(0).any(|e| e.kind == "bench.case" && e.data["case"] == case && e.data["doer"] == doer)
    }

    fn run(&self, job: &Job) -> Result<String, String> {
        let words: Vec<&str> = job.goal.split_whitespace().collect();
        let (names, flags): (Vec<&str>, Vec<&str>) = words.iter().partition(|w| !w.contains('=') && !matches!(**w, "full" | "again" | "bare"));
        let doer = flags.iter().find_map(|w| w.strip_prefix("doer=")).ok_or("name the doer to run it with: `:bench note-saved doer=claude`")?;
        let (full, again, bare) = (flags.contains(&"full"), flags.contains(&"again"), flags.contains(&"bare"));
        let arm = self.doers.get().and_then(|d| d.iter().find(|d| d.id() == doer)).ok_or_else(|| format!("no doer called `{doer}`"))?.clone();
        let chosen: Vec<&Case> = match names.as_slice() {
            ["all"] => self.cases.iter().filter(|c| !c.holdout).collect(),
            [one] => vec![self.cases.iter().find(|c| c.id == *one).ok_or_else(|| format!("no case called `{one}`"))?],
            _ => return Err("name a case, or `all` for those that are not hold-outs: `:bench note-saved doer=claude`".into()),
        };
        let mut lines = Vec::new();
        for case in chosen {
            if case.holdout && !again && self.scored(&case.id, doer) {
                lines.push(format!("{}: a hold-out, scored once for {doer} already (`again` runs it anyway)", case.id));
                continue;
            }
            lines.push(self.one(job, case, &arm, doer, full, case.brief && !bare)?);
        }
        Ok(lines.join("\n"))
    }

    /// One case, start to end: the folder, the turns, the check; the event that says what came of it.
    fn one(&self, job: &Job, case: &Case, arm: &Arc<dyn Doer>, doer: &str, full: bool, brief: bool) -> Result<String, String> {
        let started = Instant::now();
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let dir = self.base.join(format!("{}-{ms}", case.id));
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
        for (path, text) in &case.files {
            let to = dir.join(path);
            std::fs::create_dir_all(to.parent().unwrap_or(&dir)).and_then(|()| std::fs::write(&to, text)).map_err(|e| format!("cannot write {}: {e}", to.display()))?;
        }
        if case.git {
            script(&dir, "git init -q && git add -A && git -c user.name=bench -c user.email=bench@localhost commit -q --allow-empty -m start")?;
        }
        if let Some(s) = &case.setup {
            script(&dir, s)?;
        }
        let (mut turns, mut usd, mut session) = (Vec::new(), 0.0, None::<SessionRef>);
        for t in &case.turns {
            let said = self.turn(job, arm, &dir, &t.say, session.take(), (full, brief));
            usd += said.usd;
            session = said.session;
            let ok = said.error.is_none() && t.expect.as_deref().is_none_or(|e| holds(e, &said.answer));
            turns.push(json!({ "say": clip(&t.say, 200), "answer": clip(&said.answer, 600), "ok": ok, "error": said.error }));
            if said.error.is_some() {
                break;
            }
        }
        let all_ok = turns.iter().all(|t| t["ok"] == true);
        let check = case.check.as_deref().map(|c| script(&dir, c));
        let check_ok = check.as_ref().is_none_or(Result::is_ok);
        let pass = all_ok && check_ok;
        let data = json!({ "case": case.id, "doer": doer, "pass": pass, "holdout": case.holdout, "brief": brief, "turns": turns, "check": check.map(|c| c.err().unwrap_or_default()), "usd": usd, "ms": started.elapsed().as_millis() as u64, "folder": dir.display().to_string() });
        let mut ev = New::of("bench.case", Actor::Doer(DOER.into()), data).task(&job.cx.task);
        if let Some(t) = &job.cx.spec.thread {
            ev = ev.thread(t);
        }
        self.log.append(ev).map_err(|p| p.msg)?;
        Ok(format!("{} with {doer}: {} ({} of {} lines right{}, ${usd:.4})", case.id, if pass { "pass" } else { "FAIL" }, turns.iter().filter(|t| t["ok"] == true).count(), case.turns.len(), if case.check.is_some() { if check_ok { ", folder as asked" } else { ", folder not as asked" } } else { "" }))
    }

    /// One line said to the doer in the case's folder, until it ends. Its questions are answered `yes` only when the run is `full`.
    fn turn(&self, job: &Job, arm: &Arc<dyn Doer>, dir: &Path, say: &str, resume: Option<SessionRef>, (full, brief): (bool, bool)) -> Said {
        let (emit, rx) = Emit::channel();
        let mut cx = job.cx.nested(say, emit);
        cx.spec.doer = arm.id().to_string();
        cx.spec.place = "here".into();
        cx.site.dir = dir.to_path_buf();
        cx.resume = resume;
        // the line is given the Brief a line of `j4n run` gets, so that what a pack puts in it is measured; a case or a run that says `bare` measures the doer alone
        if brief && let Some(hook) = self.brief.get() {
            hook(job.cx.spec.thread.as_deref(), &mut cx.spec, say, cx.resume.is_some());
        }
        let mut said = Said::default();
        let mut run = match arm.start(cx) {
            Ok(r) => r,
            Err(p) => return Said { error: Some(p.msg), ..said },
        };
        let until = Instant::now() + Duration::from_secs(TURN_SECS);
        loop {
            if Instant::now() >= until {
                let _ = run.ctl(RunCtl::Kill);
                return Said { error: Some("timed out".into()), ..said };
            }
            if matches!(job.rx.try_recv(), Ok(RunCtl::Abort | RunCtl::Kill)) {
                let _ = run.ctl(RunCtl::Kill);
                return Said { error: Some("stopped".into()), ..said };
            }
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(RunEv::Text(t)) => said.streamed.push_str(&t),
                Ok(RunEv::Message { text, .. }) => said.streamed = text,
                Ok(RunEv::Metrics(m)) => said.usd += m.usd,
                Ok(RunEv::Session(id)) => said.session = Some(SessionRef { doer: arm.id().to_string(), id }),
                Ok(RunEv::Ask(spec)) => {
                    let answer = match (&spec.shape, full) {
                        (AskShape::Approval { .. }, true) => Answer::Yes,
                        (AskShape::Approval { .. }, false) => Answer::No,
                        _ => Answer::Text { text: String::new() },
                    };
                    let _ = run.ctl(RunCtl::Answer { local: spec.local, answer });
                }
                Ok(RunEv::End { answer, .. }) => {
                    said.answer = answer.unwrap_or_else(|| std::mem::take(&mut said.streamed));
                    return said;
                }
                Ok(RunEv::Error(why)) => return Said { error: Some(why), ..said },
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Said { error: Some("the run went away".into()), ..said },
            }
        }
    }
}

#[derive(Default)]
struct Said {
    answer: String,
    streamed: String,
    usd: f64,
    session: Option<SessionRef>,
    error: Option<String>,
}

/// Whether `expect` holds of `answer`.
fn holds(expect: &str, answer: &str) -> bool {
    let Ok(e) = Expr::parse(expect) else { return false };
    e.holds(&Scope::new().with("answer", Value::text(answer)))
}

/// A shell script run in `dir` with a cleared environment: `Ok` when it exits 0, else its last words.
fn script(dir: &Path, text: &str) -> Result<(), String> {
    let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into());
    let env = BTreeMap::from([("PATH".to_string(), path), ("HOME".into(), dir.display().to_string()), ("LANG".into(), "C.UTF-8".into())]);
    let launch = Launch::new(vec!["sh".into(), "-c".into(), format!("exec 2>&1; {text}")], dir.to_path_buf(), env);
    let (out, exit) = proc::capture(&launch, Duration::from_secs(SCRIPT_SECS)).map_err(|e| format!("cannot run it: {e}"))?;
    if exit.code == Some(0) && !exit.killed {
        Ok(())
    } else {
        Err(format!("{}: {}", if exit.killed { "timed out".to_string() } else { exit.how() }, clip(out.lines().last().unwrap_or_default(), 200)))
    }
}

