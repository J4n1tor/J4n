//! The doer `check`: which command applies to a folder, how an exit is judged, and what the doer says — through the real loop.

use crate::world::{Scratch, script};
use j4n_connect::check::{Check, CheckRow, Verdict, judge, timeout_ms};
use j4n_core::change::Actor;
use j4n_core::clock::ManualClock;
use j4n_core::doer::Doer;
use j4n_core::fold::Folds;
use j4n_core::log::{Ev, Log, Opts, Scope};
use j4n_core::redact::Redactor;
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use j4n_core::task::{Config as Loop, Ctl, Row, State, TaskSpec, Tasks, Why};
use j4n_core::timer::Timers;
use std::sync::Arc;
use std::time::Duration;

const SHIPPED: &str = include_str!("../../../../packs/base/checks/detect.toml");

fn tables(text: &str) -> Tables {
    let layer = Layer::new(LayerKind::Pack, "checks", vec![SourceFile { path: "checks/c.toml".into(), text: text.into() }]);
    let (tables, problems) = Tables::load(vec![layer]);
    assert!(problems.is_empty(), "{problems:?}");
    tables
}

fn row(unrunnable: &[&str]) -> CheckRow {
    CheckRow {
        id: "t".into(),
        doc: None,
        files: vec!["f".into()],
        also: vec![],
        needs: None,
        argv: vec!["x".into()],
        timeout: Some("2m".into()),
        pass_env: vec![],
        env: Default::default(),
        unrunnable: unrunnable.iter().map(ToString::to_string).collect(),
        named: false,
        net: None,
        if_found: false,
        top: false,
    }
}

#[test]
fn an_exit_is_a_pass_a_failure_or_no_evidence_either_way() {
    let r = row(&["no module named pytest", "Read-only file system"]);
    assert_eq!(judge(&r, Some(0), false, "all good"), Verdict::Pass);
    assert_eq!(judge(&r, Some(0), false, "Read-only file system, but it passed"), Verdict::Pass, "words matter only to a failure");
    assert_eq!(judge(&r, Some(1), false, "assertion failed"), Verdict::Fail(1));
    assert_eq!(judge(&r, Some(2), false, "ModuleNotFoundError: No module named PYTEST"), Verdict::Unrunnable("its output says `no module named pytest`".into()), "without regard to case");
    assert_eq!(judge(&r, Some(127), false, ""), Verdict::Unrunnable("the program could not be run (exit 127)".into()));
    assert_eq!(judge(&r, Some(126), false, ""), Verdict::Unrunnable("the program could not be run (exit 126)".into()));
    assert_eq!(judge(&r, None, false, ""), Verdict::Unrunnable("it was ended by a signal".into()));
    assert_eq!(judge(&r, Some(1), true, "partial"), Verdict::Unrunnable("it did not finish in 2m".into()), "the clock ran out: nothing was learned");
    assert_eq!(judge(&CheckRow { timeout: None, ..r }, None, true, ""), Verdict::Unrunnable("it did not finish in 10m".into()));
}

#[test]
fn the_shipped_rows_load_clean_and_the_first_whose_files_exist_is_the_one() {
    let (check, problems) = Check::load(&tables(SHIPPED));
    assert!(problems.is_empty(), "{problems:?}");
    let dir = Scratch::new("check-rows");
    assert!(check.row_for(dir.path()).is_none());
    dir.put("Makefile", "build:\n\ttrue\n");
    assert!(check.row_for(dir.path()).is_none(), "a Makefile with no test target has no check");
    dir.put("Makefile", "test:\n\ttrue\n");
    assert_eq!(check.row_for(dir.path()).unwrap().id, "7-make");
    dir.put("package.json", "{\"scripts\": {}}");
    assert_eq!(check.row_for(dir.path()).unwrap().id, "7-make", "no test script, no npm check");
    dir.put("package.json", "{\"scripts\": {\"test\": \"jest\"}}");
    assert_eq!(check.row_for(dir.path()).unwrap().id, "3-npm", "a lower id wins");
    dir.put("Cargo.toml", "[package]\n");
    assert_eq!(check.row_for(dir.path()).unwrap().id, "1-cargo");
    let py = Scratch::new("check-py");
    py.put("pyproject.toml", "[tool.black]\n");
    assert!(check.row_for(py.path()).is_none(), "a pyproject that does not name pytest is not a pytest project");
    py.put("pyproject.toml", "[tool.pytest.ini_options]\n");
    assert_eq!(check.row_for(py.path()).unwrap().id, "4-pytest-project");
    py.put("tests/test_a.py", "");
    assert_eq!(check.row_for(py.path()).unwrap().id, "4-pytest-project");
}

#[test]
fn a_row_that_cannot_be_used_is_a_problem_and_no_row() {
    let text = "api = 1\n[[check]]\nid = \"a\"\nfiles = [\"f\"]\nargv = [\"true\"]\ntimeout = \"0s\"\n[[check]]\nid = \"b\"\nfiles = [\"f\"]\nargv = []\n[[check]]\nid = \"c\"\nfiles = []\nargv = [\"true\"]\n[[check]]\nid = \"d\"\nfiles = [\"f\"]\nargv = [\"true\"]\n";
    let (check, problems) = Check::load(&tables(text));
    assert_eq!(problems.len(), 3, "{problems:?}");
    assert!(problems.iter().any(|p| p.msg.contains("`timeout` is a duration more than zero")) && problems.iter().filter(|p| p.msg.contains("have words")).count() == 2);
    let dir = Scratch::new("check-bad");
    dir.put("f", "");
    assert_eq!(check.row_for(dir.path()).unwrap().id, "d");
}

// ---- through the loop ----

struct Loopy {
    tasks: Tasks,
    log: Log,
    project: Scratch,
    _timers: Arc<Timers>,
    _home: Scratch,
}

impl Loopy {
    fn new(rows: &str) -> Loopy {
        let (project, home) = (Scratch::new("check-p"), Scratch::new("check-h"));
        let clock = Arc::new(ManualClock::at(1_000));
        let opts = Opts { segment_bytes: 8 << 20, max_line: 1 << 20, checkpoint_every: 1000, clock };
        let (log, problems) = Log::open(&project.join(".j4n"), Scope::Project, opts, Redactor::new()).expect("opens");
        assert!(problems.is_empty(), "{problems:?}");
        let folds = Folds::new(&project.join(".j4n/folds"));
        folds.attach(&log).expect("attaches");
        let timers = Arc::new(Timers::start(&log));
        let (check, problems) = Check::load(&tables(rows));
        assert!(problems.is_empty(), "{problems:?}");
        let mut lp = Loop::new(&log, &folds, &timers, project.path(), home.path());
        lp.doers = vec![Arc::new(check) as Arc<dyn Doer>];
        let (tasks, problems) = Tasks::start(lp).expect("starts");
        assert!(problems.is_empty(), "{problems:?}");
        Loopy { tasks, log, project, _timers: timers, _home: home }
    }

    fn run(&self) -> Row {
        let id = self.tasks.spawn(TaskSpec::new("check", "check"), Actor::Operator).unwrap();
        self.tasks.book().wait(&id, |r| r.state.is_terminal(), Duration::from_secs(20)).unwrap_or_else(|| panic!("never finished\n{}", self.dump()))
    }

    fn events(&self) -> Vec<Ev> {
        self.log.read(0).collect()
    }

    fn dump(&self) -> String {
        self.events().iter().map(|e| format!("{} {} {}", e.seq, e.kind, e.data)).collect::<Vec<_>>().join("\n")
    }
}

fn rows(body: &str) -> String {
    format!("api = 1\n[[check]]\nid = \"c\"\nfiles = [\"go\"]\n{body}\n")
}

#[test]
fn a_check_that_passes_ends_well_and_a_shell_event_carries_its_real_exit() {
    let l = Loopy::new(&rows("argv = [\"sh\", \"-c\", \"echo fine; exit 0\"]"));
    l.project.put("go", "");
    let row = l.run();
    assert_eq!((row.state, row.answer.as_deref()), (State::Done { ok: true }, Some("c: passed")), "{}", l.dump());
    let shell: Vec<Ev> = l.events().into_iter().filter(|e| e.kind == "run.shell").collect();
    assert_eq!((shell[0].data["cmd"].as_str(), shell[0].data["exit"].as_i64(), shell[0].data["out"].as_str()), (Some("sh -c echo fine; exit 0"), Some(0), Some("fine")));
}

#[test]
fn a_check_that_fails_is_a_failed_task_with_the_end_of_its_output() {
    let l = Loopy::new(&rows("argv = [\"sh\", \"-c\", \"echo one; echo bad line >&2; exit 4\"]"));
    l.project.put("go", "");
    let row = l.run();
    match &row.state {
        State::Stopped { why: Why::Failed { message } } => assert!(message.starts_with("c check failed (exit 4):") && message.contains("one") && message.contains("bad line"), "{message}"),
        other => panic!("{other:?}\n{}", l.dump()),
    }
}

#[test]
fn a_check_that_cannot_run_here_ends_well_and_says_it_is_no_evidence() {
    let l = Loopy::new(&rows("argv = [\"sh\", \"-c\", \"echo cannot reach the registry; exit 1\"]\nunrunnable = [\"the registry\"]"));
    l.project.put("go", "");
    let row = l.run();
    assert_eq!((row.state.clone(), row.answer.clone().unwrap_or_default()), (State::Done { ok: true }, "c: not run — its output says `the registry`; that is no evidence either way".to_string()), "{}", l.dump());
    let l = Loopy::new(&rows("argv = [\"/no/such/program\"]"));
    l.project.put("go", "");
    assert!(l.run().answer.unwrap().contains("not run — the program could not be run (exit 127)"), "{}", l.dump());
}

#[test]
fn a_check_that_outlives_its_clock_is_killed_and_is_no_evidence() {
    let l = Loopy::new(&rows("argv = [\"sleep\", \"30\"]\ntimeout = \"1s\""));
    l.project.put("go", "");
    let t = std::time::Instant::now();
    let row = l.run();
    assert!(t.elapsed() < Duration::from_secs(10), "{:?}", t.elapsed());
    assert_eq!(row.answer.as_deref(), Some("c: not run — it did not finish in 1s; that is no evidence either way"), "{}", l.dump());
}

#[test]
fn the_check_sees_the_folder_and_only_the_environment_the_row_lets_through() {
    // cargo sets CARGO_* for every test: the row lets one through, and the rest of the machine's own stays out
    let l = Loopy::new(&rows("argv = [\"sh\", \"-c\", \"pwd; env | sort\"]\npass_env = [\"CARGO_MANIFEST_DIR\", \"NOT_SET_ANYWHERE\"]\nenv = { FIXED = \"yes\" }"));
    l.project.put("go", "");
    let row = l.run();
    assert_eq!(row.state, State::Done { ok: true }, "{}", l.dump());
    let out = l.events().into_iter().find(|e| e.kind == "run.shell").unwrap().data["out"].as_str().unwrap().to_string();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], l.project.path().canonicalize().unwrap().display().to_string(), "in the folder it was started in");
    assert!(lines.contains(&"FIXED=yes") && lines.iter().any(|l| l.starts_with("PATH=")) && lines.contains(&"TERM=dumb"), "{out}");
    assert!(lines.iter().any(|l| l.starts_with("CARGO_MANIFEST_DIR=")), "what the row names comes through: {out}");
    assert!(!lines.iter().any(|l| l.starts_with("NOT_SET_ANYWHERE") || l.starts_with("CARGO_PKG_") || l.starts_with("RUST") || l.starts_with("SSH_") || l.starts_with("J4N_")), "nothing else of the machine's own: {out}");
}

#[test]
fn a_folder_no_row_applies_to_gets_an_answer_and_a_stop_takes_the_check_down() {
    let l = Loopy::new(&rows("argv = [\"sleep\", \"30\"]"));
    let row = l.run();
    assert!(row.answer.unwrap().starts_with("no check applies in "), "{}", l.dump());
    l.project.put("go", "");
    let id = l.tasks.spawn(TaskSpec::new("check", "check"), Actor::Operator).unwrap();
    l.tasks.book().wait(&id, |r| r.state == State::Running, Duration::from_secs(10)).unwrap();
    std::thread::sleep(Duration::from_millis(150));
    l.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
    let row = l.tasks.book().wait(&id, |r| r.state.is_terminal(), Duration::from_secs(10)).unwrap();
    assert_eq!(row.state, State::Stopped { why: Why::Aborted });
}

#[test]
fn the_output_shown_is_its_end_and_a_very_long_one_does_not_fill_memory() {
    let l = Loopy::new(&rows("argv = [\"sh\", \"-c\", \"i=0; while [ $i -lt 6000 ]; do echo line-$i-padding-padding-padding; i=$((i+1)); done; echo THE-END; exit 5\"]"));
    l.project.put("go", "");
    let row = l.run();
    let State::Stopped { why: Why::Failed { message } } = row.state else { panic!("{:?}", row.state) };
    assert!(message.ends_with("THE-END"), "the end of it is what is said: …{}", &message[message.len().saturating_sub(80)..]);
    assert!(message.len() < 4200, "{} bytes", message.len());
    let shell = l.events().into_iter().find(|e| e.kind == "run.shell").unwrap();
    assert_eq!(shell.data["exit"], 5);
    assert!(shell.data["out"].as_str().unwrap().len() <= 4010);
}

#[test]
fn the_clock_of_a_row_is_its_timeout_or_ten_minutes() {
    let with = |t: Option<&str>| CheckRow { timeout: t.map(String::from), ..row(&[]) };
    assert_eq!(timeout_ms(&with(Some("2m"))), 120_000);
    assert_eq!(timeout_ms(&with(Some("1s"))), 1_000);
    for none in [None, Some("0s"), Some("soon")] {
        assert_eq!(timeout_ms(&with(none)), 600_000, "{none:?}");
    }
}

#[test]
fn rows_are_tried_in_the_order_of_their_ids_whatever_the_order_in_the_file() {
    let text = "api = 1\n[[check]]\nid = \"b-second\"\nfiles = [\"go\"]\nargv = [\"true\"]\n[[check]]\nid = \"a-first\"\nfiles = [\"go\"]\nargv = [\"true\"]\n";
    let (check, problems) = Check::load(&tables(text));
    assert!(problems.is_empty());
    let dir = Scratch::new("check-order");
    dir.put("go", "");
    assert_eq!(check.row_for(dir.path()).unwrap().id, "a-first");
}

#[test]
fn a_python_tests_folder_whose_tests_need_a_pytest_that_is_not_there_is_not_run_and_a_failing_test_still_fails() {
    // each part's tests import pytest, none is installed, and unittest's import error must not be read as the part failing
    let (check, _) = Check::load(&tables(SHIPPED));
    let dir = Scratch::new("check-unittest");
    dir.put("tests/test_board.py", "import pytest\n");
    let row = check.row_for(dir.path()).expect("a tests folder has a check");
    assert_eq!(row.id, "6-unittest");
    let out = "E\n======\nERROR: test_board (unittest.loader._FailedTest.test_board)\nImportError: Failed to import test module: test_board\n  File \"tests/test_board.py\", line 9, in <module>\n    import pytest\nModuleNotFoundError: No module named 'pytest'\n\nRan 1 test in 0.000s\n\nFAILED (errors=1)";
    assert_eq!(judge(row, Some(1), false, out), Verdict::Unrunnable("its output says `no module named 'pytest'`".into()));
    assert_eq!(judge(row, Some(1), false, "FAIL: test_score (test_board.TestBoard)\nAssertionError: 1 != 2\n\nRan 3 tests\n\nFAILED (failures=1)"), Verdict::Fail(1));
}

#[test]
fn python_test_files_at_the_top_of_a_flat_folder_have_unittest_as_their_check_and_it_runs_there() {
    // a split's parts each wrote `test_<part>.py` beside their module, with no `tests` folder: no row applied, and each part ended unproven
    let (check, problems) = Check::load(&tables(SHIPPED));
    assert!(problems.is_empty(), "{problems:?}");
    let id = |files: &[&str]| {
        let dir = Scratch::new("check-flat");
        for f in files {
            dir.put(f, "");
        }
        check.row_for(dir.path()).map(|r| r.id.clone())
    };
    assert_eq!(id(&["engine.py", "test_engine.py"]).as_deref(), Some("65-unittest"));
    let row: CheckRow = tables(SHIPPED).get("65-unittest").expect("the row");
    assert_eq!(row.argv, ["python3", "-m", "unittest", "-q"]);
    assert!(row.top && ["ran 0 tests", "no module named 'pytest'", "read-only file system"].iter().all(|w| row.unrunnable.iter().any(|u| u == w)), "{row:?}");
    // pytest's own files come first, a `tests` folder keeps its row, and a module with no test file has none
    assert_eq!(id(&["test_engine.py", "pytest.ini"]).as_deref(), Some("5-pytest"));
    assert_eq!(id(&["test_engine.py", "tests/test_board.py"]).as_deref(), Some("6-unittest"));
    assert_eq!(id(&["engine.py", "engine_test.py", "testing.md"]), None);
    // through the loop, as a part's check names it in the part's folder: its tests run and pass
    let l = Loopy::new(SHIPPED);
    l.project.put("engine.py", "def add(a, b):\n    return a + b\n");
    l.project.put("test_engine.py", "import unittest\nimport engine\n\nclass T(unittest.TestCase):\n    def test_add(self):\n        self.assertEqual(engine.add(1, 2), 3)\n");
    let row = l.run_with(serde_json::json!({ "check": "unittest", "writes": ["engine.py", "test_engine.py"] }));
    assert_eq!((row.state, row.answer.as_deref()), (State::Done { ok: true }, Some("65-unittest: passed")), "{}", l.dump());
}

#[test]
fn a_row_for_the_top_of_a_folder_is_not_found_in_a_folder_above_a_write() {
    // a project with its own check has a package whose test files sit beside its modules: the project's check runs, not a top row in the package
    let text = |top: bool| format!("api = 1\n[[check]]\nid = \"1-flat\"\nfiles = [\"test_*.py\"]\ntop = {top}\nargv = [\"sh\", \"-c\", \"exit 3\"]\n[[check]]\nid = \"2-project\"\nfiles = [\"project.toml\"]\nargv = [\"true\"]\n");
    let l = Loopy::new(&text(true));
    l.project.put("project.toml", "");
    l.project.put("src/pkg/mod.py", "");
    l.project.put("src/pkg/test_mod.py", "");
    let row = l.run_with(serde_json::json!({ "check": "", "writes": ["src/pkg/mod.py"] }));
    assert_eq!(row.answer.as_deref(), Some("2-project: passed"), "{}", l.dump());
    // without `top` the package's folder is the nearest that a row applies in
    let l = Loopy::new(&text(false));
    l.project.put("project.toml", "");
    l.project.put("src/pkg/mod.py", "");
    l.project.put("src/pkg/test_mod.py", "");
    let row = l.run_with(serde_json::json!({ "check": "", "writes": ["src/pkg/mod.py"] }));
    assert!(matches!(row.state, State::Stopped { .. }), "{}", l.dump());
    // a `*` is for the last name of a path only
    let (_, problems) = Check::load(&tables("api = 1\n[[check]]\nid = \"x\"\nfiles = [\"*/test_a.py\"]\nargv = [\"true\"]\n"));
    assert!(problems.iter().any(|p| p.to_string().contains("the last name of a path")), "{problems:?}");
}

#[test]
fn a_row_applies_when_any_one_of_its_files_is_there() {
    let (check, _) = Check::load(&tables(SHIPPED));
    for file in ["pytest.ini", "tox.ini", "conftest.py"] {
        let dir = Scratch::new("check-any");
        dir.put(file, "");
        assert_eq!(check.row_for(dir.path()).map(|r| r.id.clone()), Some("5-pytest".to_string()), "{file}");
    }
}

#[test]
fn a_check_ended_by_a_signal_has_no_exit_to_report_and_says_so() {
    let l = Loopy::new(&rows("argv = [\"sh\", \"-c\", \"kill -9 $$\"]"));
    l.project.put("go", "");
    let row = l.run();
    assert!(row.answer.unwrap().contains("not run — it was ended by a signal"), "{}", l.dump());
    let shell = l.events().into_iter().find(|e| e.kind == "run.shell").unwrap();
    assert_eq!(shell.data["exit"], -1, "not 0, which would read as a pass");
}

#[test]
fn a_stop_takes_the_check_and_what_it_started_down() {
    let l = Loopy::new(&rows("argv = [\"sh\", \"-c\", \"echo $$ > pid; sleep 30\"]"));
    l.project.put("go", "");
    let id = l.tasks.spawn(TaskSpec::new("check", "check"), Actor::Operator).unwrap();
    for _ in 0..2000 {
        if l.project.join("pid").exists() && !l.project.read("pid").trim().is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let pid = l.project.read("pid").trim().to_string();
    assert!(std::path::Path::new(&format!("/proc/{pid}")).exists(), "it is running");
    l.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
    l.tasks.book().wait(&id, |r| r.state.is_terminal(), Duration::from_secs(10)).unwrap();
    for _ in 0..400 {
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the check {pid} is still running after the stop");
}

#[test]
fn a_plan_items_check_is_a_row_name_without_its_order_digits_and_the_row_of_that_name_is_the_one_that_runs() {
    let (check, _) = Check::load(&tables(SHIPPED));
    assert_eq!(check.names(), ["cargo", "go", "npm", "pytest-project", "pytest", "unittest", "make"]);
    // the command lines a shell approval for a check reads as: one for each default row, never a row only a chore names
    let lines = check.lines();
    assert!(lines.iter().any(|l| l.starts_with("cargo test")) && lines.iter().all(|l| !l.is_empty() && !check.names().contains(l)), "{lines:?}");
    // two rows apply here; the item names the second
    let text = "api = 1\n[[check]]\nid = \"1-first\"\nfiles = [\"go\"]\nargv = [\"sh\", \"-c\", \"exit 3\"]\n[[check]]\nid = \"2-second\"\nfiles = [\"go\"]\nargv = [\"sh\", \"-c\", \"exit 0\"]\n";
    let l = Loopy::new(text);
    l.project.put("go", "");
    assert!(matches!(l.run().state, State::Stopped { why: Why::Failed { .. } }), "without a name the first row runs: {}", l.dump());
    let mut spec = TaskSpec::new("check", "check");
    spec.input = serde_json::json!({ "check": "second" });
    let id = l.tasks.spawn(spec, Actor::Operator).unwrap();
    let row = l.tasks.book().wait(&id, |r| r.state.is_terminal(), Duration::from_secs(20)).unwrap();
    assert_eq!(row.answer.as_deref(), Some("2-second: passed"), "{}", l.dump());
    let mut spec = TaskSpec::new("check", "check");
    spec.input = serde_json::json!({ "check": "nothing-of-that-name" });
    let id = l.tasks.spawn(spec, Actor::Operator).unwrap();
    let row = l.tasks.book().wait(&id, |r| r.state.is_terminal(), Duration::from_secs(20)).unwrap();
    assert!(matches!(row.state, State::Stopped { .. }), "a name that is no row falls back to the first that applies: {}", l.dump());
}

/// A check of the item `writes`, run through the loop; the answer of the task.
fn run_for(l: &Loopy, writes: &[&str]) -> Row {
    let mut spec = TaskSpec::new("check", "check");
    spec.input = serde_json::json!({ "check": "c", "writes": writes });
    let id = l.tasks.spawn(spec, Actor::Operator).unwrap();
    l.tasks.book().wait(&id, |r| r.state.is_terminal(), Duration::from_secs(20)).unwrap_or_else(|| panic!("never finished\n{}", l.dump()))
}

#[test]
fn a_plan_items_check_runs_in_the_nearest_folder_of_its_writes_that_holds_a_project_then_the_root() {
    // the combo's projects sit in `p1/`, `p2/`; the check looked at the folder root only and said "none of the described projects is there"
    let l = Loopy::new(&rows("argv = [\"sh\", \"-c\", \"grep -q ok go\"]"));
    l.project.put("p1/go", "ok");
    l.project.put("p2/go", "bad");
    l.project.put("p1/ridge/parse.py", "");
    for (writes, ok) in [(vec!["p1/ridge/parse.py"], true), (vec!["p2/answer.json"], false), (vec!["CHANGELOG.md", "p1/ridge/"], true), (vec!["p1/not/yet/made.py"], true), (vec!["p1"], true), (vec!["p2"], false)] {
        let row = run_for(&l, &writes);
        assert_eq!(matches!(row.state, State::Done { ok: true }), ok, "{writes:?}: {:?} {}\n{}", row.state, row.answer.clone().unwrap_or_default(), l.dump());
        assert!(!row.answer.clone().unwrap_or_default().contains("no check applies"), "{writes:?}: {}", l.dump());
    }
    // nothing under the writes holds a project and the root does not either: as before
    let row = run_for(&l, &["CHANGELOG.md"]);
    assert!(row.answer.unwrap_or_default().contains("no check applies"), "{}", l.dump());
    // the root, when it holds the project; a write that tries to leave the folder is the root's
    l.project.put("go", "ok");
    assert!(matches!(run_for(&l, &["src/a.rs", "../p2/x.py"]).state, State::Done { ok: true }), "{}", l.dump());
    assert!(matches!(run_for(&l, &[]).state, State::Done { ok: true }), "no writes: the root");
}

// ---- the rows a chore names ----

const OFFLOAD_CHECKS: &str = include_str!("../../../../packs/base/checks/named.toml");

impl Loopy {
    /// A check task given `input` (what a chore says: the name of its row, and `strict`).
    fn run_with(&self, input: serde_json::Value) -> Row {
        let mut spec = TaskSpec::new("check", "check");
        spec.input = input;
        let id = self.tasks.spawn(spec, Actor::Operator).unwrap();
        self.tasks.book().wait(&id, |r| r.state.is_terminal(), Duration::from_secs(20)).unwrap_or_else(|| panic!("never finished\n{}", self.dump()))
    }
}

#[test]
fn a_row_that_only_a_name_runs_is_not_the_default_and_not_one_a_plan_item_may_name() {
    let both = "api = 1\n[[check]]\nid = \"1-t\"\nfiles = [\"go\"]\nargv = [\"true\"]\n[[check]]\nid = \"8-lint-x\"\nfiles = [\"go\"]\nargv = [\"true\"]\nnamed = true\n";
    let (check, problems) = Check::load(&tables(both));
    assert!(problems.is_empty(), "{problems:?}");
    let dir = Scratch::new("check-named");
    dir.put("go", "");
    assert_eq!(check.row_for(dir.path()).unwrap().id, "1-t");
    assert_eq!(check.names(), ["t"], "the planner is not offered it");
    // the rows of `named.toml` load clean beside the shipped ones, and change neither the default nor the names
    let all = format!("{SHIPPED}\n{}", OFFLOAD_CHECKS.trim_start_matches("api = 1\n"));
    let (shipped, problems) = Check::load(&tables(&all));
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(shipped.names(), Check::load(&tables(SHIPPED)).0.names());
    let rust = Scratch::new("check-rust");
    rust.put("Cargo.toml", "[package]\n");
    assert_eq!(shipped.row_for(rust.path()).unwrap().id, "1-cargo");
}

#[test]
fn a_chore_runs_the_check_it_names_and_a_strict_one_never_falls_back_to_the_tests() {
    let l = Loopy::new("api = 1\n[[check]]\nid = \"1-t\"\nfiles = [\"go\"]\nargv = [\"sh\", \"-c\", \"exit 0\"]\n[[check]]\nid = \"8-lint-x\"\nfiles = [\"go\"]\nargv = [\"sh\", \"-c\", \"echo lint says no >&2; exit 2\"]\nnamed = true\n");
    l.project.put("go", "");
    // `lint` is `lint-x`: the first words of a name that a chore uses
    let row = l.run_with(serde_json::json!({ "check": "lint", "strict": true }));
    match &row.state {
        State::Stopped { why: Why::Failed { message } } => assert!(message.contains("lint-x check failed (exit 2)") && message.contains("lint says no"), "{message}"),
        other => panic!("{other:?}\n{}", l.dump()),
    }
    // no `build` here: strict says so and runs nothing, where a plan item that names a check that is not there gets the tests
    let row = l.run_with(serde_json::json!({ "check": "build", "strict": true }));
    assert_eq!((row.state.clone(), row.answer.clone().unwrap_or_default()), (State::Done { ok: true }, "no `build` check applies in ".to_string() + &l.project.path().display().to_string()), "{}", l.dump());
    let row = l.run_with(serde_json::json!({ "check": "build" }));
    assert_eq!(row.answer.as_deref(), Some("1-t: passed"), "not strict: the old behaviour");
    // the tests chore names none, and the default is the tests, never a named row
    let row = l.run_with(serde_json::json!({ "check": "", "strict": false }));
    assert_eq!(row.answer.as_deref(), Some("1-t: passed"));
}

// ---- the project's own interpreter, and a check that did not run is not a pass ----

fn settled(l: &Loopy) -> Ev {
    l.events().into_iter().rev().find(|e| e.kind == "task.settled").unwrap_or_else(|| panic!("never settled\n{}", l.dump()))
}

#[test]
fn the_cleared_environment_never_sees_the_projects_venv_so_the_machines_python_is_not_its_python() {
    // what the old row did: `python3` found on the `PATH` the check is given, which holds nothing of the project's
    let l = Loopy::new(&rows("argv = [\"sh\", \"-c\", \"echo PATH=$PATH; echo VENV=${VIRTUAL_ENV-unset}\"]"));
    l.project.put("go", "");
    script(&l.project, ".venv/bin/python", "echo the project's own");
    let row = l.run();
    assert_eq!(row.state, State::Done { ok: true }, "{}", l.dump());
    let out = l.events().into_iter().find(|e| e.kind == "run.shell").unwrap().data["out"].as_str().unwrap().to_string();
    assert!(!out.contains(".venv") && out.contains("VENV=unset"), "{out}");
}

#[test]
fn a_project_with_its_own_venv_runs_that_interpreter_and_the_answer_is_passed() {
    let l = Loopy::new(SHIPPED);
    l.project.put("pyproject.toml", "[tool.pytest.ini_options]\n");
    // a venv's python: it is run as `.venv/bin/python -m pytest -q` from the folder, with nothing of the operator's environment
    script(&l.project, ".venv/bin/python", "echo \"venv python: $*, VIRTUAL_ENV=${VIRTUAL_ENV-unset}\"; exit 0");
    let row = l.run();
    assert_eq!((row.state.clone(), row.answer.as_deref()), (State::Done { ok: true }, Some("3-pytest-project: passed")), "{}", l.dump());
    let shell = l.events().into_iter().find(|e| e.kind == "run.shell").unwrap();
    assert_eq!((shell.data["cmd"].as_str(), shell.data["exit"].as_i64()), (Some(".venv/bin/python -m pytest -q"), Some(0)));
    assert!(shell.data["out"].as_str().unwrap().contains("venv python: -m pytest -q, VIRTUAL_ENV=unset"), "{}", shell.data["out"]);
    assert!(settled(&l).data.get("not_run").is_none(), "a pass is not a check that did not run");
    assert!(row.not_run.is_none(), "nor does the book say so");
}

#[test]
fn the_projects_own_rows_sort_before_the_machines_python_and_a_missing_uv_gives_way_to_the_next_row() {
    let (check, problems) = Check::load(&tables(SHIPPED));
    assert!(problems.is_empty(), "{problems:?}");
    let py = Scratch::new("check-venv-order");
    py.put("pyproject.toml", "[tool.pytest.ini_options]\n");
    assert_eq!(check.row_for(py.path()).unwrap().id, "4-pytest-project", "no venv, no lock: the machine's python3");
    py.put("uv.lock", "");
    // `uv` is on the PATH of a developer's machine and not of every CI box: either way a row applies, and never a row whose program is not there
    let id = check.row_for(py.path()).unwrap().id.clone();
    assert!(id == "35-pytest-project" || id == "4-pytest-project", "{id}");
    script(&py, ".venv/bin/python", "exit 0");
    assert_eq!(check.row_for(py.path()).unwrap().id, "3-pytest-project", "the venv comes before uv");
    let own = Scratch::new("check-venv-pytest-ini");
    own.put("pytest.ini", "");
    assert_eq!(check.row_for(own.path()).unwrap().id, "5-pytest");
    script(&own, ".venv/bin/python", "exit 0");
    assert_eq!(check.row_for(own.path()).unwrap().id, "45-pytest");
    assert_eq!(check.names(), ["cargo", "go", "npm", "pytest-project", "pytest", "unittest", "make"], "a plan item names `pytest-project` and the first row that applies is run");
    let text = "api = 1\n[[check]]\nid = \"1-first\"\nfiles = [\"go\"]\nif_found = true\nargv = [\"no-such-program-x\"]\n[[check]]\nid = \"2-second\"\nfiles = [\"go\"]\nalso = [\"also\"]\nargv = [\"sh\"]\nif_found = true\n[[check]]\nid = \"3-third\"\nfiles = [\"go\"]\nargv = [\"true\"]\n";
    let (check, _) = Check::load(&tables(text));
    let dir = Scratch::new("check-found");
    dir.put("go", "");
    assert_eq!(check.row_for(dir.path()).unwrap().id, "3-third", "no such program, and `also` is not there");
    dir.put("also", "");
    assert_eq!(check.row_for(dir.path()).unwrap().id, "2-second", "`sh` is on the PATH and `also` is there");
}

#[test]
fn a_check_that_could_not_run_ends_well_with_a_state_of_its_own_and_a_pass_has_none() {
    // the project's own python has no pytest: the venv row cannot run, which is no evidence either way
    let l = Loopy::new(SHIPPED);
    l.project.put("pyproject.toml", "[tool.pytest.ini_options]\n");
    script(&l.project, ".venv/bin/python", "echo 'No module named pytest' >&2; exit 1");
    let row = l.run();
    assert_eq!(row.state, State::Done { ok: true }, "{}", l.dump());
    assert_eq!(row.answer.as_deref(), Some("3-pytest-project: not run — its output says `no module named pytest`; that is no evidence either way"));
    assert_eq!(row.not_run.as_deref(), Some("its output says `no module named pytest`"), "the book keeps it for the gate: a plan item's check that did not run is not `checked` by it");
    let end = settled(&l);
    assert_eq!((end.data["ok"].clone(), end.data["not_run"].as_str()), (true.into(), Some("its output says `no module named pytest`")), "{}", l.dump());
    // a project no row applies to, and a program that could not be started, say it the same way
    let l = Loopy::new(&rows("argv = [\"true\"]"));
    l.run();
    assert!(settled(&l).data["not_run"].as_str().unwrap().starts_with("no check applies in "), "{}", l.dump());
    let l = Loopy::new(&rows("argv = [\"/no/such/program\"]"));
    l.project.put("go", "");
    l.run();
    assert!(settled(&l).data["not_run"].is_string(), "{}", l.dump());
    // a failure is a failure, not a check that did not run
    let l = Loopy::new(&rows("argv = [\"sh\", \"-c\", \"exit 4\"]"));
    l.project.put("go", "");
    assert!(matches!(l.run().state, State::Stopped { why: Why::Failed { .. } }));
}

#[test]
fn a_package_whose_tests_folder_is_found_above_a_write_is_imported_by_its_own_name() {
    // a part's `chess/tests/test_screen.py` does `from chess import pieces`; a check run in `chess/` would not find the package itself
    for init in [false, true] {
        let l = Loopy::new(SHIPPED);
        l.project.put("chess/pieces.py", "KINDS = ['queen']\n");
        l.project.put("chess/screen.py", "from chess import pieces\n\ndef first():\n    return pieces.KINDS[0]\n");
        l.project.put("chess/tests/test_screen.py", "import unittest\nfrom chess import screen\n\nclass T(unittest.TestCase):\n    def test_first(self):\n        self.assertEqual(screen.first(), 'queen')\n");
        if init {
            l.project.put("chess/__init__.py", "");
            l.project.put("chess/tests/__init__.py", "");
        }
        let row = l.run_with(serde_json::json!({ "check": "unittest", "writes": ["chess/screen.py", "chess/tests/test_screen.py"] }));
        assert_eq!((row.state, row.answer.as_deref()), (State::Done { ok: true }, Some("6-unittest: passed")), "with __init__.py: {init}\n{}", l.dump());
    }
    // the parent goes on the path only below the folder the check is given: at its top, nothing outside it does
    let text = "api = 1\n[[check]]\nid = \"1-py\"\nfiles = [\"tests\"]\nenv = { PYTHONPATH = \".\" }\nargv = [\"sh\", \"-c\", \"echo \\\"[$PYTHONPATH]\\\"; exit 3\"]\n";
    let said = |writes: serde_json::Value, folder: &str| {
        let l = Loopy::new(text);
        l.project.put(&format!("{folder}tests/test_a.py"), "");
        l.run_with(serde_json::json!({ "check": "", "writes": writes }));
        l.events().into_iter().find(|e| e.kind == "run.shell").map(|e| e.data["out"].as_str().unwrap_or_default().to_string()).unwrap_or_default()
    };
    assert_eq!(said(serde_json::json!(["pkg/mod.py"]), "pkg/"), "[.:..]");
    assert_eq!(said(serde_json::json!(["mod.py"]), ""), "[.]");
}
