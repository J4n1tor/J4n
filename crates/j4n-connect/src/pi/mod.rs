//! Dialect `pi-rpc`: `pi --mode rpc` and `omp --mode rpc` as a doer. One child is kept across turns, so a
//! steer reaches the running turn, an abort leaves the session for the next one, and a follow-up reuses the process; a session
//! whose child is gone is resumed from its transcript (`resume = ["--session", "{session}"]` in the manifest). When the program
//! will not speak RPC, the turn runs once in its JSON print mode (`oneshot`) and says what it lost.
//!
//! - [`frames`] reads what the children print, [`session`] is the child, [`crate::pool`] keeps them between turns.
//! - The context window is read when a turn ends (`get_session_stats`), and a run that is to shrink the session sends the `compact`
//!   command instead of a prompt. Not here: the spend fuse of the program itself (the loop's hard cap kills the run).

pub mod frames;
pub mod session;

use crate::manifest::{Connector, Family};
use crate::pool::{Pool, Reuse, signature};
use crate::proc;
use crate::template::{self, Vars};
use frames::Translator;
use j4n_core::doer::{Caps, Doer, Emit, Probe, Run, RunCtl, RunEv, StartCx};
use j4n_core::gate::ReadOnly;
use j4n_core::place::Launch;
use j4n_core::table::Problem;
use j4n_core::task::Mode;
use session::Session;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a start waits for a session that is still ending its last turn.
const BUSY_WAIT: Duration = Duration::from_secs(5);

pub struct Pi {
    c: Connector,
    family: Family,
    env: BTreeMap<String, String>,
    pool: Arc<Pool<Session>>,
}

impl Pi {
    pub fn new(c: Connector, env: BTreeMap<String, String>) -> Result<Pi, String> {
        let family = c.family.ok_or("`pi-rpc` needs `family`")?;
        Ok(Pi { c, family, env, pool: Arc::new(Pool::new(3, Duration::from_secs(600))) })
    }

    /// The arguments of a child for a task; `resume` adds the transcript to load.
    fn argv(&self, cx: &StartCx, resume: Option<&str>, template: &[String]) -> Result<Vec<String>, String> {
        let mut vars = Vars::new();
        vars.insert("cwd".into(), cx.site.dir.display().to_string());
        vars.insert("prompt".into(), cx.text.clone().unwrap_or_else(|| cx.spec.goal.clone()));
        if let Some(m) = cx.spec.tune.model.as_ref().filter(|m| !m.is_empty()) {
            vars.insert("model".into(), m.clone());
        }
        if let Some(s) = resume {
            vars.insert("session".into(), s.to_string());
        }
        let mut tpl = template.to_vec();
        if vars.contains_key("model") {
            tpl.extend(self.c.model.iter().cloned());
        }
        if resume.is_some() {
            tpl.extend(self.c.resume.iter().cloned());
        }
        tpl.extend(self.c.modes.get(if cx.spec.tune.mode == Mode::Read { "read" } else { "write" }).into_iter().flatten().cloned());
        let mut argv = vec![self.c.program().to_string()];
        argv.extend(template::render(&tpl, &vars)?);
        Ok(argv)
    }
}

/// The turn run once in the print mode, when the child would not speak RPC.
fn once(launch: &Launch, out: Emit, program: &str, why: &str) {
    out.send(RunEv::Notice(format!("{why}: this turn runs once, with no steering and no questions")));
    let seen = Arc::new(Mutex::new((Translator::new(), false)));
    let (lines, ending) = (seen.clone(), seen);
    let (out_end, out_err, program) = (out.clone(), out.clone(), program.to_string());
    let started = proc::spawn(
        launch,
        false,
        move |l| {
            let mut s = lines.lock().unwrap_or_else(|e| e.into_inner());
            for ev in s.0.line(l).evs {
                s.1 |= matches!(ev, RunEv::Error(_));
                out.send(ev);
            }
        },
        move |e| {
            let s = ending.lock().unwrap_or_else(|e| e.into_inner());
            if e.killed || s.1 {
                return;
            }
            match e.code {
                Some(0) => out_end.send(RunEv::End { answer: s.0.said(), todo_left: 0 }),
                _ => out_end.send(RunEv::Error(format!("`{program}` ended with {}: {}", e.how(), e.tail.replace('\n', " | ")))),
            };
        },
    );
    if let Err(e) = started {
        out_err.send(RunEv::Error(format!("could not start `{}`: {e}", launch.argv.first().map_or("", String::as_str))));
    }
}

impl Doer for Pi {
    fn id(&self) -> &str {
        &self.c.id
    }

    fn probe(&self) -> Probe {
        self.c.probe(&self.env)
    }

    fn caps(&self) -> Caps {
        Caps { live: true, resume: true, compact: true, ..Caps::default() }
    }

    fn shutdown(&self) {
        self.pool.kill_all();
    }

    fn start(&self, cx: StartCx) -> Result<Box<dyn Run>, Problem> {
        let id = self.c.id.clone();
        let refuse = |why: String| Problem::refused(&id, why);
        let text = cx.text.clone().unwrap_or_else(|| cx.spec.goal.clone());
        let dir = cx.site.dir.clone();
        let resume = cx.resume.as_ref().filter(|r| r.doer == id).map(|r| r.id.clone());
        let argv = self.argv(&cx, None, &self.c.start).map_err(&refuse)?;
        let want = signature(&argv, &dir, &self.env);
        self.pool.reap(Instant::now());
        let read_only = if cx.spec.tune.mode == Mode::Read { ReadOnly::Explicit } else { ReadOnly::No };
        let mut live: Option<Arc<Session>> = None;
        if let Some(sid) = &resume {
            let until = Instant::now() + BUSY_WAIT;
            loop {
                match self.pool.reusable(sid, &want, &dir) {
                    Reuse::Live(s) => {
                        live = Some(s);
                        break;
                    }
                    Reuse::Busy if Instant::now() < until => std::thread::sleep(Duration::from_millis(20)),
                    Reuse::Busy => return Err(refuse("that session is busy with another task".into())),
                    Reuse::Replaced(_) | Reuse::None => break,
                }
            }
        }
        let site = dir.display().to_string();
        let compacting = cx.compacting;
        let session = match live {
            Some(s) => {
                s.begin(cx.out.clone(), cx.task.clone(), site, read_only);
                if compacting {
                    s.compact(&text);
                } else if let Err(e) = s.prompt(&text) {
                    s.stop();
                    return Err(refuse(e));
                }
                s
            }
            None => {
                let full = self.argv(&cx, resume.as_deref(), &self.c.start).map_err(&refuse)?;
                let launch = cx.wrap(Launch { login: self.c.login.clone(), ..Launch::new(full, dir.clone(), self.env.clone()) });
                let s = Session::spawn(&id, self.family, &launch, want).map_err(|e| refuse(format!("could not start `{}`: {e}", self.c.program())))?;
                self.pool.insert(s.clone());
                s.begin(cx.out.clone(), cx.task.clone(), site, read_only);
                let (driver, out): (Arc<Session>, Emit) = (s.clone(), cx.out.clone());
                let (resumed, doer) = (resume.clone(), id.clone());
                let fallback = (!self.c.oneshot.is_empty()).then(|| self.argv(&cx, None, &self.c.oneshot)).and_then(Result::ok).map(|argv| (cx.wrap(Launch { login: self.c.login.clone(), ..Launch::new(argv, dir.clone(), self.env.clone()) }), self.c.program().to_string()));
                std::thread::spawn(move || match driver.handshake() {
                    Ok(sid) => {
                        if resumed.as_deref().is_some_and(|r| r != sid) {
                            out.send(RunEv::Notice("the session could not be carried on: this is a new one".into()));
                        }
                        out.send(RunEv::Session(sid));
                        if compacting {
                            driver.compact(&text);
                        } else if let Err(e) = driver.prompt(&text) {
                            driver.fail(e);
                            driver.stop();
                        }
                    }
                    Err(e) => {
                        driver.stop();
                        match fallback {
                            // the print mode runs a prompt and shrinks nothing: what it would be told to keep is not a task
                            _ if compacting => drop(out.send(RunEv::Error(format!("{doer}: it could not be started to shrink the session: {e}")))),
                            Some((launch, program)) => once(&launch, out, &program, &e),
                            None => drop(out.send(RunEv::Error(format!("{doer}: {e}")))),
                        }
                    }
                });
                s
            }
        };
        Ok(Box::new(PiRun { id, session }))
    }
}

struct PiRun {
    id: String,
    session: Arc<Session>,
}

impl Run for PiRun {
    fn ctl(&mut self, ctl: RunCtl) -> Result<(), Problem> {
        let said = match ctl {
            RunCtl::Steer(text) => self.session.steer(&text),
            RunCtl::Answer { local, answer } => self.session.answer(&local, &answer),
            RunCtl::Told { .. } => Ok(()),
            RunCtl::Abort => {
                self.session.abort();
                Ok(())
            }
            RunCtl::Kill => {
                self.session.stop();
                Ok(())
            }
        };
        said.map_err(|e| Problem::refused(&self.id, e))
    }
}
