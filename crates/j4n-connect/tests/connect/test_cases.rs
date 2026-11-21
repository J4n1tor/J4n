//! The `test` doer: a goal is a script, and every step of the script does what `src/test.rs` says. The loop, the Log and the
//! Gate are the real ones; what is asserted is read back from the Log.

use crate::world::Scratch;
use j4n_connect::TestDoer;
use j4n_core::answer::chain::Registry;
use j4n_core::answer::point::points;
use j4n_core::answer::state::Answerers;
use j4n_core::ask::Answer;
use j4n_core::change::Actor;
use j4n_core::clock::ManualClock;
use j4n_core::doer::Doer;
use j4n_core::fold::Folds;
use j4n_core::gate::Gate;
use j4n_core::gate::check::Gates;
use j4n_core::gate::grant::Grants;
use j4n_core::log::{Ev, Log, Opts, Scope};
use j4n_core::redact::Redactor;
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use j4n_core::task::{Asks, Config as Loop, Ctl, Row, SessionRef, State, TaskSpec, Tasks, Why};
use j4n_core::timer::Timers;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

struct Bed {
    tasks: Tasks,
    log: Log,
    project: Scratch,
    _home: Scratch,
    _timers: Arc<Timers>,
}

fn bed() -> Bed {
    let (project, home) = (Scratch::new("proj"), Scratch::new("home"));
    let opts = Opts { segment_bytes: 8 << 20, max_line: 1 << 20, checkpoint_every: 1000, clock: Arc::new(ManualClock::at(1_000)) };
    let (log, problems) = Log::open(&project.join(".j4n"), Scope::Project, opts, Redactor::new()).expect("opens");
    assert!(problems.is_empty(), "{problems:?}");
    let folds = Folds::new(&project.join(".j4n/folds"));
    folds.attach(&log).expect("attaches");
    let timers = Arc::new(Timers::start(&log));
    let files = |list: &[(&str, &str)]| list.iter().map(|(p, t)| SourceFile { path: p.to_string(), text: t.to_string() }).collect::<Vec<_>>();
    let layer = Layer::new(
        LayerKind::Pack,
        "data",
        files(&[
            ("shell.toml", include_str!("../../../../packs/base/gates/shell.toml")),
            ("modes.toml", include_str!("../../../../packs/base/gates/modes.toml")),
            ("paths.toml", include_str!("../../../../packs/base/gates/paths.toml")),
            ("egress.toml", include_str!("../../../../packs/base/gates/egress.toml")),
            ("points.toml", include_str!("../../../../packs/base/asks/points.toml")),
        ]),
    );
    let (tables, problems) = Tables::load(vec![layer]);
    assert!(problems.is_empty(), "{problems:?}");
    let (gates, _) = Gates::load(&tables);
    let (pts, _) = points(&tables);
    let mut cfg = Loop::new(&log, &folds, &timers, project.path(), home.path());
    cfg.doers = vec![Arc::new(TestDoer::new()) as Arc<dyn Doer>];
    cfg.asks = Some(Arc::new(Asks::new(pts, Registry::new(), Answerers::new()).with_gate(Gate::new(gates, Grants::new("s1")))));
    let (tasks, problems) = Tasks::start(cfg).expect("the loop starts");
    assert!(problems.is_empty(), "{problems:?}");
    Bed { tasks, log, project, _home: home, _timers: timers }
}

impl Bed {
    fn start(&self, goal: &str) -> String {
        self.tasks.spawn(TaskSpec::new(goal, "test"), Actor::Operator).expect("spawns")
    }

    fn end(&self, id: &str) -> Row {
        self.tasks.book().wait(id, |r| r.state.is_terminal(), Duration::from_secs(10)).unwrap_or_else(|| panic!("{id} never ended\n{}", self.dump()))
    }

    fn run(&self, goal: &str) -> Row {
        let id = self.start(goal);
        self.end(&id)
    }

    fn events(&self, kind: &str) -> Vec<Ev> {
        self.log.read(0).filter(|e| e.kind == kind).collect()
    }

    fn kinds(&self, id: &str) -> Vec<String> {
        self.log.read(0).filter(|e| e.task.as_deref() == Some(id)).map(|e| e.kind).collect()
    }

    fn dump(&self) -> String {
        self.log.read(0).map(|e| format!("{:>3} {:<16} {} {}", e.seq, e.kind, e.task.as_deref().unwrap_or("-"), e.data)).collect::<Vec<_>>().join("\n")
    }

    fn waiting(&self, id: &str) -> String {
        let row = self.tasks.book().wait(id, |r| matches!(r.state, State::Waiting { .. }), Duration::from_secs(10)).unwrap_or_else(|| panic!("never asked\n{}", self.dump()));
        match row.state {
            State::Waiting { ask } => ask,
            _ => unreachable!(),
        }
    }

    fn answer(&self, ask: &str, a: Answer) {
        self.tasks.ctl(ask, Ctl::Answer(a), Actor::Operator).expect("answers");
    }
}

#[test]
fn it_says_what_it_is_and_that_it_needs_nothing() {
    let d = TestDoer::new();
    assert_eq!(d.id(), "test");
    let p = d.probe();
    assert!(p.installed && p.authed && p.version.as_deref() == Some("scripted"), "{p:?}");
    assert!(d.caps().live && d.caps().resume);
}

#[test]
fn say_ok_is_a_task_that_answers_ok() {
    let b = bed();
    let row = b.run("say ok");
    assert_eq!((row.state, row.answer.as_deref()), (State::Done { ok: true }, Some("ok")));
}

#[test]
fn a_line_that_opens_with_no_verb_is_said_as_it_is() {
    let b = bed();
    assert_eq!(b.run("hello there; sh").answer.as_deref(), Some("hello there"), "a verb with nothing after it still does its step, and says nothing");
    assert_eq!(b.run("  hello   there ").answer.as_deref(), Some("hello   there"));
}

#[test]
fn the_steps_run_in_order_and_the_last_thing_said_is_the_report() {
    let b = bed();
    let row = b.run("say first; tool read src/a.rs; sh make => 2; say second\nsay third");
    assert_eq!(row.answer.as_deref(), Some("third"));
    let kinds = b.kinds(&row.id);
    let at = |k: &str| kinds.iter().position(|x| x == k).unwrap_or_else(|| panic!("no {k} in {kinds:?}"));
    assert!(at("run.tool_call") < at("run.tool_result") && at("run.tool_result") < at("run.shell"), "{kinds:?}");
    let shell = &b.events("run.shell")[0].data;
    assert_eq!((shell["cmd"].as_str(), shell["exit"].as_i64()), (Some("make"), Some(2)), "the exit code is the one asked for, and nothing is run");
    let tool = &b.events("run.tool_call")[0].data;
    assert_eq!((tool["name"].as_str(), tool["detail"].as_str()), (Some("read"), Some("src/a.rs")));
}

#[test]
fn a_command_without_an_exit_exits_clean_and_one_that_is_not_a_number_does_not() {
    let b = bed();
    b.run("sh ls");
    b.run("sh ls => lots");
    let exits: Vec<i64> = b.events("run.shell").iter().map(|e| e.data["exit"].as_i64().unwrap()).collect();
    assert_eq!(exits, [0, 1]);
}

#[test]
fn spend_is_reported_to_the_loop() {
    let b = bed();
    let row = b.run("spend 0.25; say done");
    assert_eq!(row.spent, 250_000);
    assert_eq!(b.run("spend plenty; say done").spent, 0, "a number that is not one is nothing");
}

#[test]
fn silent_ends_with_no_report_and_no_sign_of_work_and_that_is_a_failure() {
    let b = bed();
    let row = b.run("silent");
    assert_eq!(row.state, State::Done { ok: false });
    assert!(row.reason.as_deref().unwrap_or("").contains("no sign of work"), "{row:?}");
}

#[test]
fn fail_stops_the_task_with_its_message() {
    let b = bed();
    let row = b.run("say before; fail it broke");
    assert_eq!(row.state, State::Stopped { why: Why::Failed { message: "it broke".into() } });
    assert_eq!(b.events("run.error")[0].data["text"], "it broke");
}

// ---- sessions ----

#[test]
fn a_session_resumed_keeps_its_child_and_counts_its_turns_and_a_stranger_gets_a_new_child() {
    let b = bed();
    let first = b.run("who");
    assert_eq!(first.answer.as_deref(), Some("child 1 turn 1"));
    let session = first.session.clone().expect("it reported a session");
    let mut spec = TaskSpec::new("who", "test");
    spec.resume = Some(session.clone());
    let id = b.tasks.spawn(spec.clone(), Actor::Operator).unwrap();
    assert_eq!(b.end(&id).answer.as_deref(), Some("child 1 turn 2"));
    assert_eq!(b.run("who").answer.as_deref(), Some("child 2 turn 1"), "a task with no session is a new child");
    spec.resume = Some(SessionRef { doer: "test".into(), id: "test-77".into() });
    let id = b.tasks.spawn(spec.clone(), Actor::Operator).unwrap();
    let row = b.end(&id);
    assert_eq!(row.answer.as_deref(), Some("child 3 turn 1"), "{}", b.dump());
    assert!(b.events("run.notice").iter().any(|e| e.task.as_deref() == Some(id.as_str()) && e.data["text"].as_str().unwrap_or("").contains("session was gone")));
    spec.resume = Some(SessionRef { doer: "claude".into(), id: session.id.clone() });
    let id = b.tasks.spawn(spec, Actor::Operator).unwrap();
    assert_eq!(b.end(&id).answer.as_deref(), Some("child 4 turn 1"), "another doer's session is none of its business");
}

// ---- asking ----

#[test]
fn ask_puts_a_question_to_the_operator_and_says_what_came_back() {
    let b = bed();
    let id = b.start("ask which colour?");
    let ask = b.waiting(&id);
    assert_eq!(b.tasks.book().get(&ask).unwrap().spec.ask().unwrap().question, "which colour?");
    b.answer(&ask, Answer::Text { text: "green".into() });
    assert_eq!(b.end(&id).answer.as_deref(), Some("answered: green"));
}

#[test]
fn write_asks_leave_and_writes_the_file_on_a_yes() {
    let b = bed();
    let id = b.start("write out/notes/a.txt hello world");
    let ask = b.waiting(&id);
    let spec = b.tasks.book().get(&ask).unwrap().spec.ask().unwrap();
    let act = spec.act.expect("the effect is named, so the Gate can have its say");
    assert_eq!((act.verb.word(), act.target.as_str(), act.inside), ("write", "out/notes/a.txt", Some(true)));
    assert!(!b.project.join("out").exists(), "nothing is written before the answer");
    b.answer(&ask, Answer::Yes);
    let row = b.end(&id);
    assert_eq!(b.project.read("out/notes/a.txt"), "hello world");
    assert_eq!(row.state, State::Done { ok: true });
    assert_eq!(b.events("run.changed")[0].data["paths"], serde_json::json!(["out/notes/a.txt"]));
}

#[test]
fn write_on_a_no_writes_nothing_and_says_so() {
    let b = bed();
    let id = b.start("write a.txt x; say after");
    let ask = b.waiting(&id);
    b.answer(&ask, Answer::No);
    let row = b.end(&id);
    assert!(!b.project.join("a.txt").exists());
    assert_eq!(row.answer.as_deref(), Some("after"));
    assert!(b.events("run.notice").iter().any(|e| e.data["text"] == "not allowed: write a.txt"));
}

#[test]
fn write_outside_the_folder_asks_nothing_and_writes_nothing() {
    let b = bed();
    for goal in ["write ../out.txt x", "write /tmp/j4n-test-out.txt x", "write a/../../b.txt x", "write"] {
        let row = b.run(&format!("{goal}; say done"));
        assert_eq!(row.answer.as_deref(), Some("done"), "{goal}");
    }
    assert!(b.events("task.waiting").is_empty(), "no question was put");
    let notes: Vec<String> = b.events("run.notice").iter().map(|e| e.data["text"].as_str().unwrap().to_string()).collect();
    assert_eq!(notes.len(), 4, "{notes:?}");
    assert!(notes.iter().all(|n| n.starts_with("not written:")), "{notes:?}");
    assert!(!std::path::Path::new("/tmp/j4n-test-out.txt").exists());
}

// ---- time, steers, stops ----

#[test]
fn hang_goes_on_until_it_is_stopped_and_a_steer_on_the_way_is_noted() {
    let b = bed();
    let id = b.start("say up; hang");
    b.tasks.book().wait(&id, |r| r.state == State::Running, Duration::from_secs(5)).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    b.tasks.ctl(&id, Ctl::Steer("mind the time".into()), Actor::Operator).unwrap();
    for _ in 0..400 {
        if b.events("run.notice").iter().any(|e| e.data["text"] == "steered: mind the time") {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(b.events("run.notice").iter().any(|e| e.data["text"] == "steered: mind the time"), "{}", b.dump());
    b.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
    let row = b.end(&id);
    assert_eq!((row.state, row.answer.as_deref()), (State::Stopped { why: Why::Aborted }, Some("up")));
}

#[test]
fn wait_lets_time_pass_and_a_stop_ends_it_early() {
    let b = bed();
    let t = std::time::Instant::now();
    assert_eq!(b.run("wait 150; say rested").answer.as_deref(), Some("rested"));
    assert!(t.elapsed() >= Duration::from_millis(140));
    let id = b.start("wait 60000; say never");
    b.tasks.book().wait(&id, |r| r.state == State::Running, Duration::from_secs(5)).unwrap();
    b.tasks.ctl(&id, Ctl::Kill, Actor::Operator).unwrap();
    let t = std::time::Instant::now();
    let row = b.end(&id);
    assert!(t.elapsed() < Duration::from_secs(5) && row.state == State::Stopped { why: Why::Killed }, "{row:?}");
}

#[test]
fn a_stop_between_two_steps_is_seen_before_the_next_one() {
    let b = bed();
    let id = b.start("wait 100; write late.txt x; say never");
    b.tasks.book().wait(&id, |r| r.state == State::Running, Duration::from_secs(5)).unwrap();
    b.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
    b.end(&id);
    assert!(b.events("task.waiting").is_empty() && !b.project.join("late.txt").exists());
}

#[test]
fn what_it_emits_is_json_the_log_keeps_whole() {
    let b = bed();
    let row = b.run("tool grep \"quoted\" and ünïcode");
    let call = b.events("run.tool_call");
    assert_eq!(call[0].data["detail"], Value::String("\"quoted\" and ünïcode".into()));
    assert_eq!(row.answer.as_deref(), None, "a tool call alone is work, not a report");
    assert_eq!(row.state, State::Done { ok: true });
}

#[test]
fn todo_ends_the_run_with_work_left_and_window_reports_how_full_the_context_is() {
    let b = bed();
    let row = b.run("window 0.8; todo 3; say partway");
    assert_eq!((row.todo_left, row.window), (3, Some(0.8)));
    assert_eq!(b.run("say done").todo_left, 0, "nothing left unless it says");
    assert_eq!(b.run("todo; say x").todo_left, 1, "a todo with no number is one");
    assert_eq!(b.run("todo many; say x").todo_left, 1, "and so is one that is not a number");
    assert_eq!(b.run("say x").window, None, "no reading unless it gives one");
}

#[test]
fn a_run_told_to_shrink_the_session_says_so_and_the_words_of_the_task_follow_with_what_is_kept() {
    let b = bed();
    let first = b.run("say one");
    let mut spec = TaskSpec::new("say carried on", "test");
    spec.resume = first.session.clone();
    spec.compact = Some(vec![]);
    let row = b.end(&b.tasks.spawn(spec, Actor::Operator).unwrap());
    assert_eq!(row.state, State::Done { ok: true }, "{}", b.dump());
    assert_eq!(b.events("task.compacted")[0].data["note"], "the session was shrunk");
    let said: Vec<String> = b.events("run.message").iter().filter(|e| e.task.as_deref() == Some(row.id.as_str())).filter_map(|e| e.data["text"].as_str().map(String::from)).collect();
    assert_eq!(said[0], "the session was shrunk");
    assert_eq!(said[1], "carried on", "the task's own words, played as the doer plays any goal");
    assert!(said.iter().any(|l| l.contains("MUST KEEP")), "{said:?}");
}
