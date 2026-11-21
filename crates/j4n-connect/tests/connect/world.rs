//! What the tests share: scratch folders, the recorded frames, a child that plays them, and the loop over a project.

use j4n_connect::Claude;
use j4n_connect::claude::Config;
use j4n_core::change::Actor;
use j4n_core::clock::ManualClock;
use j4n_core::doer::Doer;
use j4n_core::fold::Folds;
use j4n_core::log::{Ev, Log, Opts, Scope};
use j4n_core::redact::Redactor;
use j4n_core::task::{Config as Loop, Row, TaskSpec, Tasks};
use j4n_core::timer::Timers;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// A directory under the run's temporary directory, removed when dropped.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new(tag: &str) -> Scratch {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("j4n-connect-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn join(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }

    pub fn put(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }

    pub fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.0.join(rel)).unwrap_or_default()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/claude-frames")
}

/// The lines of a recorded stream, without its comments.
pub fn frames(name: &str) -> Vec<String> {
    let text = fs::read_to_string(fixtures().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()).map(String::from).collect()
}

/// A program: `body` as a shell script, executable.
pub fn script(dir: &Scratch, name: &str, body: &str) -> PathBuf {
    let path = dir.put(name, &format!("#!/bin/sh\n{body}\n"));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// A `claude` that plays a recorded stream (`fixtures/claude-frames/fake-claude.sh` says how). `$FAKE_FIXTURE` names the
/// recording and everything the child is sent is appended to `$FAKE_LOG`.
pub fn fake(dir: &Scratch) -> PathBuf {
    let path = dir.put("claude", include_str!("../../../../fixtures/claude-frames/fake-claude.sh"));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// The loop over a project, a home and a Log, with Claude as its doer (the program and the environment are the test's).
pub struct Rig {
    pub tasks: Tasks,
    _timers: Arc<Timers>,
    pub claude: Arc<Claude>,
    pub log: Log,
    _project: Scratch,
    _home: Scratch,
    pub bin: Scratch,
}

pub fn config(program: &Path, env: &[(&str, &str)]) -> Config {
    let mut cfg = Config { program: program.display().to_string(), ..Config::default() };
    cfg.env = env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    cfg
}

impl Rig {
    /// The project folder the loop works in.
    pub fn project_dir(&self) -> PathBuf {
        self._project.path().to_path_buf()
    }

    /// A fake `claude` playing `fixture`, its log in `bin/log`.
    pub fn playing(fixture: &str) -> Rig {
        Rig::with(fixture, |_| {})
    }

    /// [`Rig::playing`], with the doer's configuration changed first.
    pub fn playing_with(fixture: &str, tune: impl FnOnce(&mut Config)) -> Rig {
        let bin = Scratch::new("bin");
        let program = fake(&bin);
        let path = fixtures().join(fixture);
        let log = bin.join("log");
        let mut cfg = config(&program, &[("FAKE_FIXTURE", path.to_str().unwrap()), ("FAKE_LOG", log.to_str().unwrap())]);
        tune(&mut cfg);
        Rig::over(bin, cfg, |_| {})
    }

    /// A `claude` that is the script `body` (it logs to `$FAKE_LOG`).
    pub fn scripted(body: &str) -> Rig {
        let bin = Scratch::new("bin");
        let log = bin.join("log");
        let program = script(&bin, "claude", body);
        Rig::over(bin, config(&program, &[("FAKE_LOG", log.to_str().unwrap())]), |_| {})
    }

    pub fn with(fixture: &str, tune: impl FnOnce(&mut Loop)) -> Rig {
        let bin = Scratch::new("bin");
        let program = fake(&bin);
        let path = fixtures().join(fixture);
        let log = bin.join("log");
        let cfg = config(&program, &[("FAKE_FIXTURE", path.to_str().unwrap()), ("FAKE_LOG", log.to_str().unwrap())]);
        Rig::over(bin, cfg, tune)
    }

    pub fn over(bin: Scratch, cfg: Config, tune: impl FnOnce(&mut Loop)) -> Rig {
        let (project, home) = (Scratch::new("proj"), Scratch::new("home"));
        let clock = Arc::new(ManualClock::at(1_000));
        let opts = Opts { segment_bytes: 8 << 20, max_line: 1 << 20, checkpoint_every: 1000, clock: clock.clone() };
        let (log, problems) = Log::open(&project.join(".j4n"), Scope::Project, opts, Redactor::new()).expect("opens");
        assert!(problems.is_empty(), "{problems:?}");
        let folds = Folds::new(&project.join(".j4n/folds"));
        folds.attach(&log).expect("attaches");
        let timers = Arc::new(Timers::start(&log));
        let claude = Arc::new(Claude::new(cfg));
        let mut lp = Loop::new(&log, &folds, &timers, project.path(), home.path());
        lp.doers = vec![claude.clone() as Arc<dyn Doer>];
        tune(&mut lp);
        let (tasks, problems) = Tasks::start(lp).expect("the loop starts");
        assert!(problems.iter().all(|p| p.msg.contains("left out")), "{problems:?}");
        Rig { tasks, _timers: timers, claude, log, _project: project, _home: home, bin }
    }

    pub fn spawn(&self, goal: &str) -> String {
        self.tasks.spawn(TaskSpec::new(goal, "claude"), Actor::Operator).expect("spawns")
    }

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

    pub fn dump(&self) -> String {
        let events = self.events();
        let lines: Vec<String> = events.iter().map(|e| format!("{:>4} {:<18} {} {}", e.seq, e.kind, e.task.as_deref().unwrap_or("-"), e.data)).collect();
        format!("{}\n-- what the child was sent:\n{}", lines.join("\n"), self.bin.read("log"))
    }

    /// The open question's task, once there is one.
    pub fn waiting_on(&self, id: &str) -> String {
        let row = self.until(id, "waited on a question", |r| matches!(r.state, j4n_core::task::State::Waiting { .. }));
        match row.state {
            j4n_core::task::State::Waiting { ask } => ask,
            _ => unreachable!(),
        }
    }

    /// What the child was sent, line by line.
    pub fn sent(&self) -> Vec<String> {
        self.bin.read("log").lines().map(String::from).collect()
    }
}

impl Rig {
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
}

/// Each example of the kind `T` is a row of a pack with no problem, and with a key its schema does not know it is refused and is no
/// row: the struct and the schema read the examples alike (Law 4 at the edge).
pub fn examples_are_rows<T: j4n_core::table::Kind>(count: usize) {
    use j4n_core::table::{KindDecl, Layer, LayerKind, SourceFile, Tables};
    let (all, problems) = Tables::load(vec![]);
    assert!(problems.is_empty(), "{problems:?}");
    let decl: KindDecl = all.get(T::NAME).unwrap();
    assert_eq!(decl.examples.len(), count, "the examples of {}", T::NAME);
    let base = all.list::<T>().unwrap().len();
    let load = |row: toml::Value| {
        let mut doc = toml::Table::new();
        doc.insert("api".into(), toml::Value::Integer(1));
        doc.insert(T::NAME.into(), toml::Value::Array(vec![row]));
        let file = SourceFile { path: format!("{}/mine.toml", T::NAME), text: toml::to_string(&doc).unwrap() };
        Tables::load(vec![Layer::new(LayerKind::Pack, "mine", vec![file])])
    };
    for (n, example) in decl.examples.iter().enumerate() {
        let (t, said) = load(example.clone());
        assert!(said.is_empty(), "{} example {n}: {said:?}", T::NAME);
        assert_eq!(t.list::<T>().unwrap().len(), base + 1, "{} example {n} is a row", T::NAME);
        let mut stray = example.as_table().unwrap().clone();
        stray.insert("zz_unknown".into(), toml::Value::Integer(1));
        let (t, said) = load(toml::Value::Table(stray));
        assert!(!said.is_empty() && t.list::<T>().unwrap().len() == base, "{} example {n} with a stray key: {said:?}", T::NAME);
    }
}
