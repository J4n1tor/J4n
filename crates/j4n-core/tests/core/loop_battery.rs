//! The loop battery (law 2): the fifteen scenarios that define "one loop" as running code. Each
//! drives the real loop over a scripted doer and checks what the operator would see — the outcome, whether the same child
//! carried on, and that every stop that can be carried on is a typed menu. A scenario belongs to the group that tests it;
//! the table says which, and a test holds it to that.

use crate::scripted::{Beat, Rig, Scripted, approval, choice, end, nothing, pick, say};
use j4n_core::ask::Answer;
use j4n_core::change::Actor;
use j4n_core::doer::{Caps, RunCtl};
use j4n_core::task::{Act, CONTINUE, Ctl, State, TaskSpec, Why};

/// `(scenario, the group it belongs to)`, in the battery's order. The scenarios of the groups `loop` and `carry` are built here.
const BATTERY: [(&str, &str); 15] = [
    ("ask_waits_and_the_same_child_continues", "loop"),
    ("a_question_takes_a_pick", "loop"),
    ("nobody_answers_and_the_park_is_a_typed_menu", "loop"),
    ("unfinished_work_continues_on_the_same_session", "carry"),
    ("a_rate_limit_cools_and_the_run_resumes_by_itself", "carry"),
    ("the_cap_is_a_menu_and_far_past_it_is_a_kill", "carry"),
    ("a_failing_child_ends_in_a_typed_menu", "loop"),
    ("the_operator_can_stop_a_running_and_a_waiting_child", "loop"),
    ("a_full_window_is_compacted_between_steps", "carry"),
    ("the_lane_log_carries_the_streamed_words", "log"),
    ("a_compact_strategy_adds_to_what_is_kept_and_never_replaces_it", "carry"),
    ("a_child_that_only_streams_still_has_an_answer", "loop"),
    ("an_answerable_question_never_reaches_the_operator", "answer"),
    ("a_question_nothing_can_answer_goes_to_the_operator_unchanged", "answer"),
    ("an_approval_is_never_put_to_the_answerer", "answer"),
];

#[test]
fn the_battery_is_fifteen_and_each_one_not_built_yet_names_its_step() {
    let mut names: Vec<&str> = BATTERY.iter().map(|(n, _)| *n).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), 15);
    let built: Vec<&str> = BATTERY.iter().filter(|(_, step)| ["loop", "carry"].contains(step)).map(|(n, _)| *n).collect();
    assert_eq!(built.len(), 11, "another scenario built changes this table");
    assert!(BATTERY.iter().all(|(_, step)| ["loop", "answer", "log", "carry"].contains(step)));
    // every built scenario is a test, under its own name: those of `loop` and `carry` below, those of `answer` in `ask_engine.rs` (through the chain and the Gate)
    let source = include_str!("loop_battery.rs");
    for name in built {
        assert!(source.contains(&format!("#[test]\nfn {name}()")), "no test called {name}");
    }
    let chain = include_str!("ask_engine.rs");
    let answered: Vec<&str> = BATTERY.iter().filter(|(_, step)| *step == "answer").map(|(n, _)| *n).collect();
    assert_eq!(answered.len(), 3, "another scenario built changes this table");
    for name in answered {
        assert!(chain.contains(&format!("#[test]\nfn {name}()")), "no test called {name}");
    }
}

/// Every stop that can be carried on is a menu that ends with `abort` and recommends at most one thing.
fn typed(rig: &Rig, id: &str) -> Vec<String> {
    let menu = rig.tasks.menu(id);
    assert!(!menu.is_empty(), "{id} stopped with nothing to choose");
    assert_eq!(menu.last().map(|i| i.act.clone()), Some(Act::Abort), "abort comes last: {menu:?}");
    assert!(menu.iter().filter(|i| i.recommended).count() <= 1, "{menu:?}");
    menu.iter().map(|i| i.id.clone()).collect()
}

fn waiting_on(rig: &Rig, id: &str) -> String {
    match rig.until(id, "waited", |r| matches!(r.state, State::Waiting { .. })).state {
        State::Waiting { ask } => ask,
        other => panic!("{other:?}"),
    }
}

// 1 — a question holds the child; the answer reaches the SAME child, which finishes.
#[test]
fn ask_waits_and_the_same_child_continues() {
    let doer = Scripted::new("script", vec![vec![Beat::Ask(approval("q1", "cargo build")), Beat::Hold, say("built"), end("built")]]);
    let rig = Rig::new(vec![doer.clone()]);
    let id = rig.spawn("build it", "script");
    let ask = waiting_on(&rig, &id);
    assert!(doer.ctls().is_empty(), "the child was touched while it waited: {:?}", doer.ctls());
    assert_eq!(rig.tasks.book().get(&ask).unwrap().state, State::Running, "the question is open");
    rig.tasks.ctl(&ask, Ctl::Answer(Answer::Yes), Actor::Operator).unwrap();
    let row = rig.finished(&id);
    assert_eq!(row.state, State::Done { ok: true });
    assert_eq!(row.answer.as_deref(), Some("built"));
    assert_eq!(doer.starts().len(), 1, "one child, not a re-run");
    assert_eq!(doer.ctls(), vec![RunCtl::Answer { local: "q1".into(), answer: Answer::Yes }]);
    let kinds = rig.kinds(&id);
    let at = |k: &str| kinds.iter().position(|x| x == k).unwrap();
    assert!(at("task.waiting") < at("task.resumed") && at("task.resumed") < at("task.settled"), "{kinds:?}");
    let q = rig.tasks.book().get(&ask).unwrap();
    assert_eq!((q.state, q.answer.as_deref()), (State::Done { ok: true }, Some("yes")));
}

// 2 — a real question carries its own options; a pick reaches the same child, a pick that was not offered does not.
#[test]
fn a_question_takes_a_pick() {
    let q = choice("db", "which database?", &[("postgres", true), ("sqlite", false)]);
    let doer = Scripted::new("script", vec![vec![Beat::Ask(q), Beat::Hold, say("done"), end("done")]]);
    let rig = Rig::new(vec![doer.clone()]);
    let id = rig.spawn("set it up", "script");
    let ask = waiting_on(&rig, &id);
    let refused = rig.tasks.ctl(&ask, Ctl::Answer(pick("mysql")), Actor::Operator).unwrap_err();
    assert!(refused.msg.contains("not one of the offered options"), "{refused}");
    assert!(matches!(rig.tasks.book().get(&id).unwrap().state, State::Waiting { .. }), "a refused pick answers nothing");
    rig.tasks.ctl(&ask, Ctl::Answer(pick("sqlite")), Actor::Operator).unwrap();
    assert_eq!(rig.finished(&id).state, State::Done { ok: true });
    assert_eq!(doer.ctls(), vec![RunCtl::Answer { local: "db".into(), answer: pick("sqlite") }]);
    assert_eq!(doer.starts().len(), 1);
}

// 3 — nobody answers: after the wait the task parks, resumably, as a typed menu.
#[test]
fn nobody_answers_and_the_park_is_a_typed_menu() {
    let q = choice("db", "which database?", &[("postgres", true), ("sqlite", false)]);
    let turns = vec![vec![Beat::Session("sess-1".into()), Beat::Ask(q), Beat::Hold, say("never")], vec![say("carried on"), end("carried on")]];
    let doer = Scripted::new("script", turns);
    let rig = Rig::tuned(vec![doer.clone()], |c| c.ask_wait_ms = 60_000);
    let id = rig.spawn("set it up", "script");
    let ask = waiting_on(&rig, &id);
    rig.clock.advance(59_999);
    rig.timers.poke();
    assert!(matches!(rig.tasks.book().get(&id).unwrap().state, State::Waiting { .. }), "not yet");
    rig.clock.advance(1);
    rig.timers.poke();
    let parked = rig.until(&id, "parked", |r| r.state.is_terminal());
    assert_eq!(parked.state, State::Stopped { why: Why::Parked { ask: ask.clone() } });
    assert_eq!(rig.tasks.book().get(&ask).unwrap().state, State::Stopped { why: Why::Deadline });
    assert_eq!(doer.ctls(), vec![RunCtl::Abort], "parked keeps the session; nothing is killed");
    let menu = typed(&rig, &id);
    assert_eq!(menu, ["pick:postgres", "pick:sqlite", "say", "abort"]);
    assert_eq!(rig.tasks.menu(&id).iter().find(|i| i.recommended).map(|i| i.id.as_str()), Some("pick:postgres"));
    // resumable: carrying on starts the doer again on the session it reported
    let next = rig.tasks.resume(&id, Some("use sqlite"), Actor::Operator).unwrap();
    assert_eq!(rig.finished(&next).state, State::Done { ok: true });
    let starts = doer.starts();
    assert_eq!(starts.len(), 2);
    assert_eq!((starts[1].resume.as_deref(), starts[1].goal.as_str()), (Some("sess-1"), "use sqlite"));
    assert_eq!(rig.tasks.book().get(&next).unwrap().spec.follows.as_deref(), Some(id.as_str()));
    let resumed = |task: &str| rig.of_kind("task.started").into_iter().find(|e| e.task.as_deref() == Some(task)).unwrap().data["resumed"].clone();
    assert_eq!((resumed(&id), resumed(&next)), (serde_json::json!(false), serde_json::json!(true)), "the Log says which start carried on");
}

// 7 — a child that fails exhausts nothing quietly: a typed menu, never a silent death.
#[test]
fn a_failing_child_ends_in_a_typed_menu() {
    let rig = Rig::new(vec![Scripted::broken("dead", "transport died"), Scripted::new("flaky", vec![vec![say("half"), Beat::Fail("it fell over".into())]])]);
    let a = rig.spawn("go", "dead");
    let row = rig.finished(&a);
    assert!(matches!(&row.state, State::Stopped { why: Why::Failed { message } } if message.contains("transport died")), "{:?}", row.state);
    assert_eq!(typed(&rig, &a), ["retry", "abort"]);
    assert!(rig.tasks.menu(&a)[0].recommended);
    let b = rig.spawn("go", "flaky");
    let row = rig.finished(&b);
    assert!(matches!(&row.state, State::Stopped { why: Why::Failed { message } } if message == "it fell over"), "{:?}", row.state);
    assert_eq!(row.answer.as_deref(), Some("half"), "what it said before it failed is its report");
    assert_eq!(typed(&rig, &b), ["retry", "abort"]);
}

// 8 — the operator stops a running child and a waiting one: both end, neither as Done.
#[test]
fn the_operator_can_stop_a_running_and_a_waiting_child() {
    let running = Scripted::new("run", vec![vec![Beat::Forever]]);
    let waiting = Scripted::new("wait", vec![vec![Beat::Ask(approval("q", "rm -rf build")), Beat::Hold, say("x")]]);
    let rig = Rig::new(vec![running.clone(), waiting.clone()]);
    let a = rig.spawn("go", "run");
    rig.until(&a, "started", |r| r.state == State::Running);
    while running.starts().is_empty() {
        std::thread::yield_now();
    }
    rig.tasks.ctl(&a, Ctl::Abort, Actor::Operator).unwrap();
    assert_eq!(rig.finished(&a).state, State::Stopped { why: Why::Aborted });
    assert_eq!(running.ctls(), vec![RunCtl::Abort], "an abort keeps the session");
    assert!(rig.tasks.menu(&a).is_empty(), "the operator did it themselves: nothing to choose");

    let b = rig.spawn("go", "wait");
    let ask = waiting_on(&rig, &b);
    rig.tasks.ctl(&b, Ctl::Kill, Actor::Operator).unwrap();
    assert_eq!(rig.finished(&b).state, State::Stopped { why: Why::Killed });
    assert_eq!(waiting.ctls(), vec![RunCtl::Kill]);
    assert_eq!(rig.tasks.book().get(&ask).unwrap().state, State::Stopped { why: Why::Orphaned }, "its question went with it");
    let err = rig.tasks.ctl(&b, Ctl::Abort, Actor::Operator).unwrap_err();
    assert!(err.msg.contains("it is over"), "{err}");
}

// 12 — a child that only streams still has an answer: the streamed words, whole, not "(no message)".
#[test]
fn a_child_that_only_streams_still_has_an_answer() {
    let doer = Scripted::new("script", vec![vec![Beat::Delta("the answer is ".into()), Beat::Delta("{\"n\": 3}".into()), nothing()]]);
    let rig = Rig::new(vec![doer]);
    let id = rig.spawn("count", "script");
    let row = rig.finished(&id);
    assert_eq!(row.state, State::Done { ok: true });
    assert_eq!(row.answer.as_deref(), Some("the answer is {\"n\": 3}"));
    let said: Vec<String> = rig.of_kind("run.message").iter().map(|e| e.data["text"].as_str().unwrap().to_string()).collect();
    assert_eq!(said, ["the answer is {\"n\": 3}"], "the settled words are in the Log, once");
    assert!(rig.of_kind("run.delta").is_empty(), "the deltas were flashed, never written");
}

/// A rig with an extra doer that only answers, so that a test can wait until the loop has handled everything that was ahead of
/// it in its inbox: what the rules said about an event is done by the time a task started after it has finished.
fn barrier(rig: &Rig) {
    let id = rig.spawn("settle", "idle");
    rig.finished(&id);
}

fn idle() -> std::sync::Arc<Scripted> {
    Scripted::new("idle", vec![vec![end("ok")]])
}

fn setting(id: &str, value: &str) -> String {
    format!("[[setting]]\nid = \"{id}\"\nvalue = {value}\n")
}

// 4 — work left over is carried on in the same session, up to the setting, and then the task ends honestly.
#[test]
fn unfinished_work_continues_on_the_same_session() {
    let doer = Scripted::new("script", vec![vec![Beat::Session("sess-battery".into()), Beat::End(Some("turn 1".into()), 2)], vec![Beat::End(Some("turn 2".into()), 0)]]);
    let rig = Rig::based(vec![doer.clone(), idle()], "", |_| {});
    let id = rig.spawn("build it", "script");
    let first = rig.finished(&id);
    assert_eq!((first.state, first.todo_left), (State::Done { ok: true }, 2));
    let second = rig.finished("t2");
    assert_eq!(second.state, State::Done { ok: true });
    assert_eq!(second.answer.as_deref(), Some("turn 2"));
    let starts = doer.starts();
    assert_eq!(starts.len(), 2, "one more run, not a restart of the work");
    assert_eq!(starts[1].resume.as_deref(), Some("sess-battery"), "the same session");
    assert_eq!(starts[1].goal, "2 item(s) are still open. Carry on and finish them.");
    assert_eq!((second.spec.follows.as_deref(), second.auto), (Some("t1"), 1));
    let queued = rig.of_kind("task.queued").into_iter().find(|e| e.task.as_deref() == Some("t2")).unwrap();
    assert_eq!(queued.by, Actor::Rule("followups".into()), "the Log says who carried it on");
    let settled = rig.of_kind("task.settled").into_iter().find(|e| e.task.as_deref() == Some("t1")).unwrap();
    assert_eq!(queued.cause, Some(settled.seq), "and why");
    assert_eq!(rig.of_kind("rule.fired").len(), 1);

    // a child that never finishes is nudged up to the cap and then ends honestly: 1 + the setting
    let stuck = Scripted::new("stuck", vec![vec![Beat::Session("s".into()), Beat::End(Some("again".into()), 1)]]);
    let rig = Rig::based(vec![stuck.clone(), idle()], &setting("loop.followups", "2"), |_| {});
    let id = rig.spawn("go", "stuck");
    rig.finished(&id);
    let last = rig.finished("t3");
    barrier(&rig);
    assert_eq!(stuck.starts().len(), 3, "the first run and two follow-ups");
    assert!(rig.tasks.book().get("t4").is_none_or(|r| r.spec.doer == "idle"), "no fourth run of the child");
    assert_eq!((last.state, last.todo_left, last.auto), (State::Done { ok: true }, 1, 2), "it still says what is left");
}

// 5 — a rate limit is Cooling, not a stop: the run goes on by itself when the limit has passed.
#[test]
fn a_rate_limit_cools_and_the_run_resumes_by_itself() {
    let doer = Scripted::new("script", vec![vec![Beat::Session("s1".into()), Beat::Fail("You've hit your usage limit".into())], vec![say("back"), end("back")]]);
    let rig = Rig::based(vec![doer.clone()], "", |_| {});
    let id = rig.spawn("go", "script");
    let cooling = rig.until(&id, "cooled", |r| matches!(r.state, State::Cooling { .. }));
    let wait = 30 * 60_000;
    assert_eq!(cooling.state, State::Cooling { until_ms: 1_000 + wait }, "the window of the failure row `limit`");
    assert!(rig.tasks.menu(&id).is_empty(), "it is not stopped, so there is nothing to choose");
    let err = rig.tasks.ctl(&id, Ctl::Steer("hurry".into()), Actor::Operator).unwrap_err();
    assert!(err.msg.contains("waiting out a limit"), "{err}");
    rig.clock.advance(wait - 1);
    rig.timers.poke();
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(matches!(rig.tasks.book().get(&id).unwrap().state, State::Cooling { .. }), "not before it is time");
    rig.clock.advance(1);
    rig.timers.poke();
    let row = rig.finished(&id);
    assert_eq!((row.state, row.answer.as_deref()), (State::Done { ok: true }, Some("back")));
    let starts = doer.starts();
    assert_eq!(starts.len(), 2, "the same task, run again");
    assert_eq!((starts[1].resume.as_deref(), starts[1].text.as_deref()), (Some("s1"), Some(CONTINUE)), "in its session, told to carry on");
    let kinds = rig.kinds(&id);
    let at = |k: &str| kinds.iter().position(|x| x == k).unwrap_or_else(|| panic!("{k} in {kinds:?}"));
    assert!(at("run.error") < at("task.cooling") && at("task.cooling") < at("task.resumed") && at("task.resumed") < at("task.settled"), "{kinds:?}");
    assert!(!kinds.contains(&"task.stopped".to_string()), "never stopped: {kinds:?}");
    let error = &rig.of_kind("run.error")[0].data;
    assert_eq!((error["class"].as_str(), error["row"].as_str(), error["wait_ms"].as_u64()), (Some("rate-limit"), Some("limit"), Some(wait)));
}

// 6 — a spend cap is a menu (resumable); only far past it is a kill.
#[test]
fn the_cap_is_a_menu_and_far_past_it_is_a_kill() {
    let spend = |usd: f64| vec![vec![Beat::Session("s".into()), Beat::Spend(usd), Beat::Forever]];
    let capped = |soft: f64| {
        let mut spec = TaskSpec::new("go", "script");
        spec.limits.usd_soft = Some(soft);
        spec
    };
    let doer = Scripted::new("script", spend(0.0042));
    let rig = Rig::based(vec![doer.clone()], "", |_| {});
    let id = rig.tasks.spawn(capped(0.004), Actor::Operator).unwrap();
    let row = rig.finished(&id);
    assert_eq!(row.state, State::Stopped { why: Why::Capped }, "{:?}", row.state);
    assert_eq!(typed(&rig, &id), ["resume", "abort"]);
    assert_eq!(doer.ctls(), vec![RunCtl::Abort], "parked keeps the session; nothing is killed");
    let by = rig.of_kind("task.stopped").remove(0).by;
    assert_eq!(by, Actor::Rule("soft-cap".into()), "the Log says which rule parked it");
    let next = rig.tasks.resume(&id, None, Actor::Operator).unwrap();
    // started, or on past it: the carried-on task spends the same 0.0042 and is parked again as soon as it runs, so it can be `Stopped` before
    // this looks; its row is the same one
    assert_eq!(rig.until(&next, "started", |r| r.state != State::Queued).spec.follows.as_deref(), Some(id.as_str()), "carried on from the menu");

    let hard = Scripted::new("script", spend(0.0042));
    let rig = Rig::based(vec![hard.clone()], "", |_| {});
    let id = rig.tasks.spawn(capped(0.002), Actor::Operator).unwrap();
    assert_eq!(rig.finished(&id).state, State::Stopped { why: Why::HardCap }, "2.1 times the cap");
    assert_eq!(hard.ctls(), vec![RunCtl::Kill]);
    assert!(rig.of_kind("task.stopped").iter().all(|e| e.by == Actor::Core), "the loop did it, whatever any rule says");
}

fn compacting_doer(turns: Vec<Vec<Beat>>) -> std::sync::Arc<Scripted> {
    Scripted::with_caps("script", Caps { live: true, resume: true, compact: true, ..Caps::default() }, turns)
}

fn full_window_turns() -> Vec<Vec<Beat>> {
    vec![
        vec![Beat::Session("sess".into()), Beat::Window(0.8), Beat::Put("out/report.tsv".into(), "a,b".into()), Beat::End(Some("turn 1".into()), 2)],
        vec![say("summarised"), Beat::End(Some("412 tokens kept".into()), 0)],
        vec![Beat::End(Some("turn 2".into()), 0)],
    ]
}

// 9 — a full window is shrunk between steps, never in the middle of one, and what must stay is told again.
#[test]
fn a_full_window_is_compacted_between_steps() {
    let doer = compacting_doer(full_window_turns());
    let rig = Rig::based(vec![doer.clone()], "", |_| {});
    let id = rig.spawn("build it", "script");
    assert_eq!(rig.finished(&id).state, State::Done { ok: true });
    let last = rig.finished("t2");
    assert_eq!((last.state, last.answer.as_deref()), (State::Done { ok: true }, Some("turn 2")));
    let starts = doer.starts();
    assert_eq!(starts.iter().map(|s| s.compacting).collect::<Vec<_>>(), [false, true, false], "work, shrink, work: once");
    assert!(starts.iter().all(|s| s.task == id || s.task == "t2"));
    assert_eq!(starts[1].task, "t2", "at the boundary: the task that carries on does it first");
    let asked = starts[1].text.clone().unwrap();
    assert!(asked.contains("MUST KEEP") && asked.contains("goal: build it") && asked.contains("2 item(s) still open") && asked.contains("out/report.tsv"), "{asked}");
    let next = starts[2].text.clone().unwrap();
    assert!(next.starts_with("2 item(s) are still open. Carry on and finish them."), "{next}");
    assert!(next.contains("MUST KEEP") && next.contains("goal: build it") && next.contains("out/report.tsv"), "the next prompt carries it too: {next}");
    assert_eq!(starts[2].resume.as_deref(), Some("sess"), "in the same session");
    let done = rig.of_kind("task.compacted");
    assert_eq!(done.len(), 1);
    assert_eq!((done[0].data["ok"].clone(), done[0].data["note"].as_str(), done[0].data["before"].as_f64()), (serde_json::json!(true), Some("412 tokens kept"), Some(0.8)));
    assert_eq!(last.compactions, 1);
}

// 11 — a rule's own notes ride the compaction and the next prompt, after what the loop keeps and never instead of it.
#[test]
fn a_compact_strategy_adds_to_what_is_kept_and_never_replaces_it() {
    let mine = "[[rule]]\nid = \"followups-compact\"\nkeep = [\"the goal was: {fold.tasks[task].origin}\", \"never touch vendor/\"]\n";
    let doer = compacting_doer(full_window_turns());
    let rig = Rig::based(vec![doer.clone()], mine, |_| {});
    let id = rig.spawn("build it", "script");
    rig.finished(&id);
    assert_eq!(rig.finished("t2").state, State::Done { ok: true });
    let starts = doer.starts();
    let told = starts[1].text.clone().unwrap();
    assert!(told.contains("out/report.tsv") && told.contains("never touch vendor/") && told.contains("the goal was: build it"), "the child was told: {told}");
    let next = starts[2].text.clone().unwrap();
    assert!(next.contains("2 item(s) are still open") && next.contains("out/report.tsv") && next.contains("never touch vendor/"), "next prompt: {next}");
    assert!(next.find("out/report.tsv") < next.find("never touch vendor/"), "the notes come after what the loop keeps");
    assert!(next.find("goal: build it") < next.find("out/report.tsv"), "and what is kept begins with the goal");
}
