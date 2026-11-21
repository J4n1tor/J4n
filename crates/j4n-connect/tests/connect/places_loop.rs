//! The worktree place through the real loop: a task's change kept, looked at, applied, rewound; what a restart finds.

use crate::world::Scratch;
use j4n_connect::place::{Apply, Worktree};
use j4n_core::change::Actor;
use j4n_core::clock::ManualClock;
use j4n_core::doer::{Caps, Doer, Probe, Run, RunCtl, RunEv, StartCx};
use j4n_core::fold::Folds;
use j4n_core::log::{Log, New, Opts, Scope};
use j4n_core::place::checkpoint::Cfg;
use j4n_core::place::{Take, Taken};
use j4n_core::redact::Redactor;
use j4n_core::table::Problem;
use j4n_core::task::{Config as Loop, Row, State, TaskSpec, Tasks};
use j4n_core::timer::Timers;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

/// Writes the files its goal names (`a.txt=two;ledger/x.json={}`) in the folder it is given, then ends.
struct Writer;

struct Idle;

impl Run for Idle {
    fn ctl(&mut self, _: RunCtl) -> Result<(), Problem> {
        Ok(())
    }
}

impl Doer for Writer {
    fn id(&self) -> &str {
        "writer"
    }

    fn probe(&self) -> Probe {
        Probe { installed: true, version: None, authed: true, detail: String::new() }
    }

    fn caps(&self) -> Caps {
        Caps::default()
    }

    fn start(&self, cx: StartCx) -> Result<Box<dyn Run>, Problem> {
        for item in cx.spec.goal.split(';').filter(|i| !i.is_empty()) {
            let (path, text) = item.split_once('=').unwrap_or((item, ""));
            let file = cx.site.dir.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
        cx.out.send(RunEv::End { answer: Some("written".into()), todo_left: 0 });
        Ok(Box::new(Idle))
    }
}

/// Runs until it is stopped.
struct Hanger;

impl Doer for Hanger {
    fn id(&self) -> &str {
        "hanger"
    }

    fn probe(&self) -> Probe {
        Probe { installed: true, version: None, authed: true, detail: String::new() }
    }

    fn caps(&self) -> Caps {
        Caps::default()
    }

    fn start(&self, cx: StartCx) -> Result<Box<dyn Run>, Problem> {
        std::mem::forget(cx.out);
        Ok(Box::new(Idle))
    }
}

fn git(root: &std::path::Path, args: &[&str]) -> String {
    let out = Command::new("git").arg("-C").arg(root).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

struct World {
    project: Scratch,
    home: Scratch,
    log: Log,
    folds: Folds,
    timers: Arc<Timers>,
}

impl World {
    fn new(tag: &str) -> World {
        let (project, home) = (Scratch::new(&format!("{tag}-p")), Scratch::new(&format!("{tag}-h")));
        git(project.path(), &["init", "-q", "-b", "main"]);
        project.put("a.txt", "one\n");
        project.put(".gitignore", "ledger/\n.j4n/\n");
        git(project.path(), &["add", "-A"]);
        git(project.path(), &["commit", "-qm", "base"]);
        let opts = Opts { segment_bytes: 8 << 20, max_line: 1 << 20, checkpoint_every: 1000, clock: Arc::new(ManualClock::at(1_000)) };
        let (log, problems) = Log::open(&project.join(".j4n"), Scope::Project, opts, Redactor::new()).expect("opens");
        assert!(problems.is_empty(), "{problems:?}");
        let folds = Folds::new(&project.join(".j4n/folds"));
        folds.attach(&log).expect("attaches");
        let timers = Arc::new(Timers::start(&log));
        World { project, home, log, folds, timers }
    }

    fn worktree(&self, apply: Apply) -> Arc<Worktree> {
        Arc::new(Worktree::new(self.home.path(), self.project.path(), apply, Cfg::default()))
    }

    fn start(&self, place: &Arc<Worktree>) -> (Tasks, Vec<Problem>) {
        let mut lp = Loop::new(&self.log, &self.folds, &self.timers, self.project.path(), self.home.path());
        lp.doers = vec![Arc::new(Writer) as Arc<dyn Doer>, Arc::new(Hanger) as Arc<dyn Doer>];
        lp.places.push(place.clone());
        Tasks::start(lp).expect("starts")
    }
}

fn run(tasks: &Tasks, goal: &str) -> Row {
    let mut spec = TaskSpec::new(goal, "writer");
    spec.place = "worktree".into();
    let id = tasks.spawn(spec, Actor::Operator).unwrap();
    tasks.book().wait(&id, |r| r.state.is_terminal(), Duration::from_secs(20)).expect("finishes")
}

#[test]
fn a_kept_change_is_looked_at_applied_and_rewound() {
    let w = World::new("take");
    let (tasks, _) = w.start(&w.worktree(Apply::Off));
    let row = run(&tasks, "a.txt=two\n;new.txt=n\n;ledger/x.json={}");
    assert_eq!(row.state, State::Done { ok: true });
    let kept = row.kept.clone().expect("kept");
    assert_eq!((kept.work.files.clone(), kept.work.ignored.clone()), (vec!["a.txt".to_string(), "new.txt".to_string()], vec!["ledger/x.json".to_string()]));
    assert_eq!(row.changed.created, ["new.txt", "ledger/x.json"], "what it did is in the row though it is not in the folder");
    assert_eq!(std::fs::read_to_string(w.project.join("a.txt")).unwrap(), "one\n");
    let looked = tasks.take(&row.id, Take::Look).unwrap();
    assert!(matches!(&looked, Taken::Clean(work, notes) if work.files.len() == 2 && notes.is_empty()), "{looked:?}");
    assert!(!w.project.join("new.txt").exists());
    let done = tasks.take(&row.id, Take::Apply).unwrap();
    assert!(matches!(done, Taken::Clean(..)));
    tasks.sync().unwrap();
    let after = tasks.book().get(&row.id).unwrap();
    assert!(after.kept.is_none() && after.applied.is_some(), "{after:?}");
    assert_eq!(std::fs::read_to_string(w.project.join("a.txt")).unwrap(), "two\n");
    assert!(w.project.join("new.txt").exists() && w.project.join("ledger/x.json").exists());
    // and the apply is undone
    let report = tasks.rewind(Some(&row.id), false).unwrap();
    assert_eq!((report.restored.len(), report.removed.len()), (1, 2), "{report:?}");
    assert_eq!(std::fs::read_to_string(w.project.join("a.txt")).unwrap(), "one\n");
    assert!(!w.project.join("new.txt").exists() && !w.project.join("ledger").exists());
    assert_eq!(git(w.project.path(), &["status", "--porcelain"]), "");
    assert_eq!(w.log.read(0).filter(|e| e.kind == "task.rewound").count(), 1);
}

/// A lay-in rewound holds its work again, and the row names the copy that holds it: that copy is there as soon as the rewind is done,
/// not only once the work is taken again, so the words that name it ("its work is held at …") name a folder that exists.
#[test]
fn a_lay_in_rewound_is_held_again_in_the_copy_its_row_names() {
    let w = World::new("held-again");
    let (tasks, _) = w.start(&w.worktree(Apply::Off));
    let row = run(&tasks, "a.txt=two\n;new.txt=n\n");
    assert!(matches!(tasks.take(&row.id, Take::Apply).unwrap(), Taken::Clean(..)));
    // the row says it was laid in before it is rewound, as it does in the loop, where a check runs in between
    tasks.sync().unwrap();
    tasks.rewind(Some(&row.id), false).unwrap();
    tasks.sync().unwrap();
    let after = tasks.book().get(&row.id).unwrap();
    let kept = after.kept.expect("held again once its lay-in is rewound");
    let dir = std::path::Path::new(&kept.dir);
    assert!(dir.is_dir(), "the row names {}, which is not there", kept.dir);
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "two\n");
    assert!(dir.join("new.txt").exists() && !w.project.join("new.txt").exists(), "the work is in the copy, not in the folder");
    // and it is laid in again from there
    assert!(matches!(tasks.take(&row.id, Take::Apply).unwrap(), Taken::Clean(..)));
    assert_eq!(std::fs::read_to_string(w.project.join("a.txt")).unwrap(), "two\n");
}

#[test]
fn with_apply_auto_it_lands_as_the_task_ends_and_a_clash_keeps_it() {
    let w = World::new("auto");
    let (tasks, _) = w.start(&w.worktree(Apply::Auto));
    let row = run(&tasks, "a.txt=two\n");
    assert!(row.kept.is_none() && row.applied.as_ref().is_some_and(|a| a.files == ["a.txt"]), "{row:?}");
    assert_eq!(std::fs::read_to_string(w.project.join("a.txt")).unwrap(), "two\n");
    git(w.project.path(), &["checkout", "--", "a.txt"]);
    // the operator edits the same file while the task works: the task's change waits
    w.project.put("a.txt", "mine\n");
    let row = run(&tasks, "a.txt=theirs\n");
    let kept = row.kept.expect("kept");
    assert!(kept.why.starts_with("not applied: ") && kept.why.contains("a.txt"), "{}", kept.why);
    assert_eq!(std::fs::read_to_string(w.project.join("a.txt")).unwrap(), "mine\n");
    git(w.project.path(), &["checkout", "--", "a.txt"]);
    let again = tasks.take(&row.id, Take::Apply).unwrap();
    assert!(matches!(again, Taken::Clean(..)));
    assert_eq!(std::fs::read_to_string(w.project.join("a.txt")).unwrap(), "theirs\n");
}

#[test]
fn a_task_that_changed_nothing_leaves_nothing_and_a_still_running_task_cannot_be_taken_from() {
    let w = World::new("none");
    let (tasks, _) = w.start(&w.worktree(Apply::Off));
    let row = run(&tasks, ";");
    assert!(row.kept.is_none() && row.applied.is_none());
    assert!(tasks.take(&row.id, Take::Apply).unwrap_err().msg.contains("left no work"));
    assert!(w.home.path().join("worktrees").read_dir().unwrap().flatten().all(|e| e.path().read_dir().unwrap().count() == 0));
}

#[test]
fn work_a_restart_finds_in_a_worktree_is_said_on_the_row_of_the_task_that_made_it() {
    let w = World::new("restart");
    let place = w.worktree(Apply::Off);
    // J4n stopped while t1 ran: its worktree has a change and its row is open
    let spec = {
        let mut s = TaskSpec::new("a.txt=two", "writer");
        s.place = "worktree".into();
        s
    };
    w.log.append(New::of("task.queued", Actor::Operator, serde_json::json!({ "spec": spec })).task("t1")).unwrap();
    w.log.append(New::of("task.started", Actor::Core, serde_json::json!({ "doer": "writer", "place": "worktree" })).task("t1")).unwrap();
    let site = {
        use j4n_core::place::{Place, Prepare};
        place.prepare(&Prepare { task: "t1", spec: &spec, root: w.project.path() }).unwrap()
    };
    std::fs::write(site.dir.join("a.txt"), "two\n").unwrap();
    let after = w.worktree(Apply::Off);
    let (tasks, problems) = w.start(&after);
    assert!(problems.iter().any(|p| p.msg.contains("was running when J4n stopped")), "{problems:?}");
    let found = after.recover();
    assert_eq!(found.len(), 1);
    let (task, kept) = found.into_iter().next().unwrap();
    assert!(tasks.keep(&task, kept.clone()), "an interrupted task is over, and has nothing kept yet");
    tasks.sync().unwrap();
    assert!(!tasks.keep(&task, kept), "and it is said once");
    let row = tasks.book().get("t1").unwrap();
    assert_eq!(row.kept.as_ref().map(|k| k.work.files.clone()), Some(vec!["a.txt".to_string()]));
    assert!(matches!(tasks.take("t1", Take::Apply).unwrap(), Taken::Clean(..)));
    assert_eq!(std::fs::read_to_string(w.project.join("a.txt")).unwrap(), "two\n");
    assert!(!tasks.keep("t9", j4n_core::place::Kept { dir: "x".into(), work: Default::default(), why: "x".into() }), "no such task");
    // work that was applied is not kept again, and a task that is still going has not left anything yet
    let some = j4n_core::place::Kept { dir: "x".into(), work: Default::default(), why: "x".into() };
    assert!(!tasks.keep("t1", some.clone()), "applied");
    let mut hang = TaskSpec::new("x", "hanger");
    hang.place = "worktree".into();
    let id = tasks.spawn(hang, Actor::Operator).unwrap();
    tasks.book().wait(&id, |r| r.state == State::Running, Duration::from_secs(10)).unwrap();
    assert!(!tasks.keep(&id, some), "still going");
    tasks.ctl(&id, j4n_core::task::Ctl::Abort, Actor::Operator).unwrap();
}
