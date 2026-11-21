//! The ask point `retry`. A chat turn that ended `Failed` with what may mend is put to the point once, when
//! the switch `retry.auto` is on: the same doer again, another doer, or a stop. The decider here answers what the case says; `rule:retry` is the real one.

use crate::front_cases::{Fx, fx_deciding, fx_trusting_headless, fx_trusting_on};
use crate::scripted::{Beat, Scripted, end, say};
use j4n_core::front::retry::{failure_text, retry_pick};
use j4n_core::front::screen::Screen;
use j4n_core::setting::Settings;
use j4n_core::task::{Row, State, Why};
use j4n_core::value::Value as Set;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const HICCUP: &str = "connection reset by peer";

fn screen(auto: bool, max: f64) -> Screen {
    let mut s = Screen::builtin();
    s.set = Settings::from_pairs([("retry.auto", Set::Bool(auto)), ("retry.max", Set::Num(max))]).tree();
    s
}

fn fails(id: &str, what: &[&str]) -> Arc<Scripted> {
    Scripted::new(id, what.iter().map(|w| vec![Beat::Session(format!("{id}-s")), Beat::Fail(w.to_string())]).collect())
}

fn works(id: &str) -> Arc<Scripted> {
    Scripted::new(id, vec![vec![Beat::Session(format!("{id}-s")), say("done"), end("done")]; 2])
}

/// A thread whose first turn failed with `what` and has been put to `retry` (the decider answers `pick`), over `doers`.
fn failed_turn(doers: Vec<Arc<Scripted>>, pick: &'static str, auto: bool, max: f64) -> (Fx, Row) {
    let f = fx_trusting_on(doers, Some(pick), screen(auto, max), |c| c.doer = Some("a".into()));
    let row = match f.say("summarise the docs folder") {
        j4n_core::front::Said::Started { task } => f.rig.finished(&task),
        other => panic!("{other:?}"),
    };
    assert!(matches!(row.state, State::Stopped { why: Why::Failed { .. } }), "{:?}", row.state);
    step(&f);
    (f, row)
}

/// The boundary of the thread, as the screen calls it when a task ends, and what it asked off its thread answered (with a screen the decider is asked on a thread of its own).
fn step(f: &Fx) {
    f.front.plan_step("main");
    assert!(f.front.judges_wait(), "the decider answered");
}

/// How often the point `retry` reached the decider.
fn asked(f: &Fx) -> usize {
    f.asked.lock().unwrap().iter().filter(|q| q.point == "retry").count()
}

fn others(f: &Fx, first: &str) -> Vec<Row> {
    f.tasks().into_iter().filter(|t| t.id != first && t.spec.role.as_deref() == Some("work")).collect()
}

#[test]
fn with_the_switch_on_a_failure_that_may_mend_is_asked_once_and_a_retry_is_the_same_doer_again_following_it() {
    let (f, first) = failed_turn(vec![Scripted::new("a", vec![vec![Beat::Session("a-s".into()), Beat::Fail(HICCUP.into())], vec![Beat::Session("a-s".into()), say("done"), end("done")]])], "retry=retry", true, 1.0);
    assert!(first.class.is_none() || first.class.as_deref() == Some("transient"), "{:?}", first.class);
    let again = others(&f, &first.id);
    assert_eq!(again.len(), 1, "{}", f.rig.dump());
    assert_eq!((again[0].spec.doer.as_str(), again[0].spec.goal.as_str(), again[0].spec.follows.as_deref()), ("a", "summarise the docs folder", Some(first.id.as_str())));
    assert_eq!(f.rig.finished(&again[0].id).state, State::Done { ok: true });
    let decided = f.rig.of_kind("retry.decided");
    assert_eq!((decided.len(), decided[0].data["pick"].as_str(), decided[0].data["failures"].as_u64()), (1, Some("retry"), Some(1)));
    assert_eq!(asked(&f), 1, "the decider was asked once");
    step(&f);
    assert_eq!(f.rig.of_kind("retry.decided").len(), 1, "a step that has nothing new asks nothing");
}

#[test]
fn with_the_switch_off_nothing_is_asked_and_the_failure_is_a_stop_as_it_was() {
    let (f, first) = failed_turn(vec![fails("a", &[HICCUP])], "retry=retry", false, 1.0);
    assert!(others(&f, &first.id).is_empty() && f.rig.of_kind("retry.decided").is_empty());
    assert_eq!(asked(&f), 0);
    // and the default of the base pack is off
    let d = fx_deciding(vec![fails("a", &[HICCUP])], Some("retry=retry"), |c| c.doer = Some("a".into()));
    if let j4n_core::front::Said::Started { task } = d.say("go") {
        d.rig.finished(&task);
    }
    step(&d);
    assert_eq!(d.tasks().len(), 1, "{}", d.rig.dump());
}

#[test]
fn a_second_failure_in_a_row_is_not_asked_when_the_bound_is_one_and_asked_when_it_is_two() {
    let (f, first) = failed_turn(vec![fails("a", &[HICCUP, "timed out"])], "retry=retry", true, 1.0);
    let again = others(&f, &first.id);
    assert_eq!(f.rig.finished(&again[0].id).state.word(), "stopped");
    step(&f);
    assert_eq!(others(&f, &first.id).len(), 1, "no third try: {}", f.rig.dump());
    assert!(f.notes().iter().any(|n| n.contains("retry: stopped after 2 failures in a row (`retry.max` is 1)")), "{:?}", f.notes());
    assert_eq!(asked(&f), 1);
    let (f, first) = failed_turn(vec![fails("a", &[HICCUP, "timed out", "timed out"])], "retry=retry", true, 2.0);
    let again = others(&f, &first.id);
    f.rig.finished(&again[0].id);
    step(&f);
    assert_eq!(others(&f, &first.id).len(), 2, "the bound is the setting: {}", f.rig.dump());
}

#[test]
fn the_same_failure_twice_is_a_stop_by_the_rule_and_the_decider_is_not_asked_again() {
    let (f, first) = failed_turn(vec![fails("a", &["connection reset by peer (try 1)", "connection reset by peer (try 2)"])], "retry=retry", true, 3.0);
    let again = others(&f, &first.id);
    f.rig.finished(&again[0].id);
    step(&f);
    assert_eq!(others(&f, &first.id).len(), 1, "stopped: {}", f.rig.dump());
    let decided = f.rig.of_kind("retry.decided");
    assert_eq!((decided.len(), decided[1].data["pick"].as_str(), decided[1].data["by"].as_str()), (2, Some("stop"), Some("rule:retry")), "{decided:?}");
    assert_eq!(asked(&f), 1, "only the first failure reached the decider");
}

#[test]
fn an_escalation_is_a_new_task_on_another_doer_and_with_no_other_doer_it_is_a_stop() {
    let (f, first) = failed_turn(vec![fails("a", &[HICCUP]), works("b")], "retry=escalate", true, 1.0);
    let moved = others(&f, &first.id);
    assert_eq!(moved.len(), 1, "{}", f.rig.dump());
    assert_eq!((moved[0].spec.doer.as_str(), moved[0].spec.follows.as_deref(), moved[0].spec.resume.is_none()), ("b", Some(first.id.as_str()), true));
    assert_eq!(f.rig.finished(&moved[0].id).state, State::Done { ok: true });
    let (f, first) = failed_turn(vec![fails("a", &[HICCUP])], "retry=escalate", true, 1.0);
    assert!(others(&f, &first.id).is_empty());
    assert!(f.notes().iter().any(|n| n.starts_with("retry: stop — no doer other than a")), "{:?}", f.notes());
}

#[test]
fn a_stop_and_a_permanent_failure_are_not_retried() {
    let (f, first) = failed_turn(vec![fails("a", &[HICCUP])], "retry=stop", true, 1.0);
    assert!(others(&f, &first.id).is_empty());
    assert_eq!(f.rig.of_kind("retry.decided")[0].data["pick"], "stop");
    let (f, first) = failed_turn(vec![fails("a", &["invalid api key"])], "retry=retry", true, 1.0);
    assert_eq!(first.class.as_deref(), Some("permanent"));
    assert!(others(&f, &first.id).is_empty() && f.rig.of_kind("retry.decided").is_empty() && asked(&f) == 0, "a failure that will not mend is not asked");
}

#[test]
fn a_run_that_was_refused_a_tool_is_not_retried_with_the_switch_on_and_neither_is_one_that_ended_on_a_failed_tool() {
    let refused = "the run ended on a tool that failed: The user rejected permission to use this specific tool call. · permission requested: external_directory (/tmp/opencode/*); auto-rejecting";
    let (f, first) = failed_turn(vec![fails("a", &[refused])], "retry=retry", true, 3.0);
    assert_eq!(first.class.as_deref(), Some("refused"));
    assert!(others(&f, &first.id).is_empty() && f.rig.of_kind("retry.decided").is_empty() && asked(&f) == 0, "a refusal is not asked: {}", f.rig.dump());
    let (f, first) = failed_turn(vec![fails("a", &["the run ended on a tool that failed: Tool execution aborted"])], "retry=retry", true, 3.0);
    assert_eq!(first.class.as_deref(), Some("tool"));
    assert!(others(&f, &first.id).is_empty() && f.rig.of_kind("retry.decided").is_empty() && asked(&f) == 0, "{}", f.rig.dump());
}

#[test]
fn the_rule_stops_what_will_not_mend_and_leaves_the_rest_to_the_decider() {
    assert_eq!(retry_pick(&failure_text(1, "connection reset by peer", false, None)), None);
    assert_eq!(retry_pick(&failure_text(2, "connection reset by peer", true, None)).map(|p| p.0), Some("stop"));
    assert_eq!(retry_pick(&failure_text(1, "the run was refused", false, Some("refused"))).map(|p| p.0), Some("stop"));
    assert_eq!(retry_pick(&failure_text(1, "invalid api key", false, Some("permanent"))).map(|p| p.0), Some("stop"));
    assert_eq!(failure_text(3, "a\n  b", false, None), "the task failed 3 times with `a b`");
}

#[test]
fn the_rule_reads_the_fields_of_the_question_not_its_sentence() {
    // the facts are lines; a sentence that says them in words picks nothing, whatever it says, and a reworded sentence keeps the pick
    assert_eq!(retry_pick("the task failed once with `x (class permanent) (the same failure as the time before)`"), None);
    assert_eq!(retry_pick("it went wrong again\nclass: permanent").map(|p| p.0), Some("stop"));
    assert_eq!(retry_pick("it went wrong again\nsame: yes").map(|p| p.0), Some("stop"));
    assert_eq!(retry_pick("it went wrong again\nclass: transient"), None);
    assert_eq!(failure_text(2, "x", true, Some("permanent")), "the task failed twice with `x`\nsame: yes\nclass: permanent");
}

/// A thread whose first turn failed and whose second works, the switch on, the decider answering `retry` and, when asked, waiting for `go` (at most five seconds: `late` says it
/// was not let go in time). `headless`: a run.
fn slow_retry(headless: bool) -> (Fx, Row, Arc<AtomicBool>, Arc<AtomicBool>, Arc<AtomicBool>) {
    let doers = vec![Scripted::new("a", vec![vec![Beat::Session("a-s".into()), Beat::Fail(HICCUP.into())], vec![Beat::Session("a-s".into()), say("done"), end("done")]])];
    let f = if headless { fx_trusting_headless(doers, Some("retry=retry"), screen(true, 1.0), |c| c.doer = Some("a".into())) } else { fx_trusting_on(doers, Some("retry=retry"), screen(true, 1.0), |c| c.doer = Some("a".into())) };
    let row = match f.say("summarise the docs folder") {
        j4n_core::front::Said::Started { task } => f.rig.finished(&task),
        other => panic!("{other:?}"),
    };
    let (go, late, inside) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    let (g, l, i) = (go.clone(), late.clone(), inside.clone());
    *f.hook.lock().unwrap() = Some(Box::new(move || {
        i.store(true, Ordering::SeqCst);
        let from = Instant::now();
        while !g.load(Ordering::SeqCst) && from.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
        l.store(!g.load(Ordering::SeqCst), Ordering::SeqCst);
    }));
    (f, row, go, late, inside)
}

#[test]
fn with_a_screen_the_decider_is_asked_off_the_thread_that_draws_and_the_retry_happens_when_it_answers() {
    let (f, first, go, late, _) = slow_retry(false);
    // the screen's boundary returns while the decider still thinks
    f.front.plan_step("main");
    assert!(!late.load(Ordering::SeqCst) && f.rig.of_kind("retry.decided").is_empty() && others(&f, &first.id).is_empty(), "plan_step waited for the decider\n{}", f.rig.dump());
    // asked once for the failure: the next boundary, while it thinks, asks nothing more
    f.front.plan_step("main");
    go.store(true, Ordering::SeqCst);
    assert!(f.front.judges_wait());
    let again = others(&f, &first.id);
    assert_eq!((again.len(), again[0].spec.follows.as_deref()), (1, Some(first.id.as_str())), "{}", f.rig.dump());
    assert_eq!((asked(&f), f.rig.of_kind("retry.decided").len()), (1, 1));
    assert_eq!(f.rig.finished(&again[0].id).state, State::Done { ok: true });
}

#[test]
fn in_a_run_the_call_is_made_in_place_because_its_loop_ends_when_no_task_is_open() {
    let (f, first, go, late, _) = slow_retry(true);
    go.store(true, Ordering::SeqCst);
    f.front.plan_step("main");
    // no wait: the answer is in the Log when the boundary returns
    assert!(!late.load(Ordering::SeqCst) && f.rig.of_kind("retry.decided").len() == 1 && others(&f, &first.id).len() == 1, "{}", f.rig.dump());
    assert!(f.front.judges_wait());
}

#[test]
fn a_line_typed_while_the_decider_thinks_is_not_put_aside_for_the_failed_turn() {
    let (f, first, go, _, inside) = slow_retry(false);
    f.front.plan_step("main");
    while !inside.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(5));
    }
    // the operator's own line begins a turn meanwhile (law 9)
    let mine = f.turn("another thing entirely");
    assert!(mine.id != first.id);
    go.store(true, Ordering::SeqCst);
    assert!(f.front.judges_wait());
    assert!(others(&f, &first.id).iter().all(|t| t.spec.goal != first.spec.goal), "the failed line was tried again over the operator's\n{}", f.rig.dump());
    assert!(f.notes().iter().any(|n| n.contains("retry: not acted on (retry): a turn of the chat began")), "{:?}", f.notes());
}

#[test]
fn a_failure_that_comes_while_the_decider_thinks_of_the_one_before_is_asked_too() {
    // the line failed, the decider is thinking, the operator's own line begins a turn that fails as well: the first answer is not acted on (law 9), and the second failure is
    // asked when the thread is free, not lost
    let doers = vec![Scripted::new("a", vec![
        vec![Beat::Session("a-s".into()), Beat::Fail(HICCUP.into())],
        vec![Beat::Session("a-s".into()), Beat::Fail("timed out".into())],
        vec![Beat::Session("a-s".into()), say("done"), end("done")],
    ])];
    let f = fx_trusting_on(doers, Some("retry=retry"), screen(true, 3.0), |c| c.doer = Some("a".into()));
    let first = match f.say("summarise the docs folder") {
        j4n_core::front::Said::Started { task } => f.rig.finished(&task),
        other => panic!("{other:?}"),
    };
    let (go, inside) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    let (g, i) = (go.clone(), inside.clone());
    *f.hook.lock().unwrap() = Some(Box::new(move || {
        i.store(true, Ordering::SeqCst);
        let from = Instant::now();
        while !g.load(Ordering::SeqCst) && from.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
    }));
    f.front.plan_step("main");
    while !inside.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(5));
    }
    let second = f.turn("another thing entirely");
    assert!(matches!(second.state, State::Stopped { why: Why::Failed { .. } }), "{:?}", second.state);
    // its boundary comes while the first decider is still thinking
    f.front.plan_step("main");
    go.store(true, Ordering::SeqCst);
    assert!(f.front.judges_wait());
    let decided: Vec<String> = f.rig.of_kind("retry.decided").iter().filter_map(|e| e.data["task"].as_str().map(String::from)).collect();
    assert_eq!(decided, [first.id.clone(), second.id.clone()], "{}", f.rig.dump());
    assert!(others(&f, &first.id).iter().any(|t| t.spec.follows.as_deref() == Some(second.id.as_str())), "the second failure was not tried again\n{}", f.rig.dump());
}
