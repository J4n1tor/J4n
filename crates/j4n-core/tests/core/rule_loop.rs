//! Rules in the loop (the kinds `rule`, `setting` and `failure`, and what the loop does with them): a task carried on, parked at
//! its cap, waiting out a limit, shrunk between steps, watched — and what the rules cannot do. The five scenarios of the loop
//! battery that need them are in `loop_battery.rs`; these are the rest.

use crate::scripted::{Beat, Rig, Scripted, base_tables, end, say};
use j4n_core::change::Actor;
use j4n_core::doer::{Caps, RunCtl};
use j4n_core::task::{CONTINUE, Ctl, State, TaskSpec, Why};
use std::sync::Arc;
use std::time::Duration;

fn idle() -> Arc<Scripted> {
    Scripted::new("idle", vec![vec![end("ok")]])
}

/// Wait until the loop has handled everything that was ahead of a task that only answers.
fn barrier(rig: &Rig) {
    let id = rig.spawn("settle", "idle");
    rig.finished(&id);
}

fn setting(id: &str, value: &str) -> String {
    format!("[[setting]]\nid = \"{id}\"\nvalue = {value}\n")
}

fn shrinking(turns: Vec<Vec<Beat>>) -> Arc<Scripted> {
    Scripted::with_caps("script", Caps { live: true, resume: true, compact: true, ..Caps::default() }, turns)
}

fn pause() {
    std::thread::sleep(Duration::from_millis(60));
}

/// A turn that ends with `left` items open in session `s`, the window `w` full.
fn leaving(left: u32, w: f64) -> Vec<Beat> {
    vec![Beat::Session("s".into()), Beat::Window(w), Beat::End(Some("turn".into()), left)]
}

// ---- carrying on ----

#[test]
fn a_task_is_carried_on_only_when_it_finished_well_with_work_left_and_a_session_to_go_on_in() {
    let cases: [(&str, Vec<Beat>); 4] = [
        ("nothing left", vec![Beat::Session("s".into()), Beat::End(Some("done".into()), 0)]),
        ("no session", vec![Beat::End(Some("half".into()), 2)]),
        ("it failed", vec![Beat::Session("s".into()), Beat::Fail("it fell over".into())]),
        ("did nothing", vec![Beat::Session("s".into()), Beat::End(None, 3)]),
    ];
    for (why, turn) in cases {
        let rig = Rig::based(vec![Scripted::new("script", vec![turn]), idle()], "", |_| {});
        let id = rig.spawn("go", "script");
        rig.finished(&id);
        barrier(&rig);
        let others: Vec<String> = rig.tasks.book().rows().into_iter().filter(|r| r.spec.doer == "script").map(|r| r.id).collect();
        assert_eq!(others, [id], "{why}: no second run");
        assert!(rig.of_kind("rule.fired").is_empty(), "{why}");
    }
}

#[test]
fn a_chain_of_follow_ups_ends_at_the_depth_of_a_chain_of_rules_whatever_the_setting_says() {
    let stuck = Scripted::new("script", vec![leaving(1, 0.0)]);
    let rig = Rig::based(vec![stuck.clone(), idle()], &setting("loop.followups", "20"), |_| {});
    let id = rig.spawn("go", "script");
    let last = rig.until_event("the chain is cut", |e| e.kind == "rule.dropped" && e.data["rule"] == "followups");
    barrier(&rig);
    assert!(last.data["why"].as_str().unwrap().contains("a chain of 8 reactions"), "{}", last.data);
    assert_eq!(stuck.starts().len(), 9, "the first run and eight follow-ups");
    let rows: Vec<_> = rig.tasks.book().rows().into_iter().filter(|r| r.spec.doer == "script").collect();
    assert_eq!(rows.last().unwrap().auto, 8);
    let said: Vec<String> = rig.of_kind("rule.dropped").iter().map(|e| e.data["rule"].as_str().unwrap().to_string()).collect();
    assert_eq!(said.iter().filter(|r| *r == "followups").count(), 1, "said once, whatever else the deep task goes on to say: {said:?}");
    assert_eq!(rows[0].id, id);
}

#[test]
fn what_a_rule_wrote_says_which_rule_and_which_event_it_answered() {
    let doer = Scripted::new("script", vec![leaving(2, 0.0), vec![end("again")]]);
    let rig = Rig::based(vec![doer, idle()], "", |_| {});
    let id = rig.spawn("go", "script");
    rig.finished("t2");
    let settled = rig.of_kind("task.settled").into_iter().find(|e| e.task.as_deref() == Some(id.as_str())).unwrap();
    let fired = rig.of_kind("rule.fired");
    assert_eq!(fired.len(), 1);
    assert_eq!((fired[0].by.clone(), fired[0].cause, fired[0].task.as_deref()), (Actor::Rule("followups".into()), Some(settled.seq), Some(id.as_str())));
    assert_eq!(fired[0].data, serde_json::json!({ "rule": "followups", "then": "start" }));
    // what the action wrote follows from the same event, and the chain is one deeper than the event
    for kind in ["task.queued", "task.started"] {
        let e = rig.of_kind(kind).into_iter().find(|e| e.task.as_deref() == Some("t2")).unwrap();
        assert_eq!(e.cause, Some(settled.seq), "{kind}");
    }
    let run = rig.of_kind("run.message").into_iter().find(|e| e.task.as_deref() == Some("t2")).unwrap();
    assert_eq!(run.cause, None, "what the doer said later is the doer's");
}

#[test]
fn an_operator_who_carries_a_task_on_starts_the_count_again_and_the_origin_stays() {
    let doer = Scripted::new("script", vec![leaving(1, 0.0), vec![Beat::Session("s".into()), Beat::Fail("it fell over".into())], vec![end("fine")]]);
    let rig = Rig::based(vec![doer, idle()], &setting("loop.followups", "1"), |_| {});
    let id = rig.spawn("write the report", "script");
    let second = rig.finished("t2");
    assert!(matches!(second.state, State::Stopped { why: Why::Failed { .. } }), "{:?}", second.state);
    assert_eq!((second.auto, second.origin.as_str()), (1, "write the report"), "the origin is the first words, not the follow-up's");
    let next = rig.tasks.resume("t2", Some("try the other way"), Actor::Operator).unwrap();
    let third = rig.finished(&next);
    assert_eq!((third.auto, third.origin.as_str(), third.spec.goal.as_str()), (0, "write the report", "try the other way"));
    assert_eq!(id, "t1");
}

#[test]
fn without_rules_the_loop_does_what_no_rule_can_undo_and_nothing_more() {
    let doer = Scripted::new("script", vec![vec![Beat::Session("s".into()), Beat::Spend(0.005), Beat::End(Some("done".into()), 2)]]);
    let rig = Rig::new(vec![doer.clone(), idle()]);
    let mut spec = TaskSpec::new("go", "script");
    spec.limits.usd_soft = Some(0.004);
    let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
    let row = rig.finished(&id);
    barrier(&rig);
    assert_eq!(row.state, State::Done { ok: true }, "past the soft cap and not parked: it is written, and nothing acts on it");
    assert!(row.capped);
    assert_eq!(doer.starts().len(), 1, "work left, and not carried on");
    assert_eq!(rig.of_kind("task.cap_reached").len(), 1);
}

// ---- the cap ----

#[test]
fn the_soft_cap_is_written_once_and_a_rule_in_the_users_own_file_answers_it_too() {
    let doer = Scripted::new("script", vec![vec![Beat::Session("s".into()), Beat::Spend(0.0045), Beat::Forever]]);
    // the operator's rule answers the same event as the base pack's, which still parks
    let rig = Rig::based(vec![doer.clone(), idle()], "[[rule]]\nid = \"tell-cap\"\non = \"task.cap_reached\"\nthen = \"notice\"\ntext = \"past the cap\"\n", |_| {});
    let mut spec = TaskSpec::new("go", "script");
    spec.limits.usd_soft = Some(0.004);
    let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
    let row = rig.finished(&id);
    assert_eq!(row.state, State::Stopped { why: Why::Capped }, "the base rule still parks");
    assert_eq!(rig.of_kind("task.cap_reached").len(), 1, "once");
    rig.until_event("the operator's rule", |e| e.kind == "run.notice" && e.by == Actor::Rule("tell-cap".into()));
    let notices: Vec<String> = rig.of_kind("run.notice").iter().filter(|e| e.by == Actor::Rule("tell-cap".into())).map(|e| e.data["text"].as_str().unwrap().into()).collect();
    assert_eq!(notices, ["past the cap"]);
}

// ---- waiting out a limit ----

#[test]
fn a_limit_that_names_its_end_is_waited_out_to_that_time() {
    let doer = Scripted::new("script", vec![vec![Beat::Session("s".into()), Beat::Fail("usage limit reached (resets at 1700000600)".into())], vec![end("back")]]);
    let rig = Rig::based(vec![doer], "", |_| {});
    rig.clock.set(1_700_000_000_000);
    let id = rig.spawn("go", "script");
    let cooling = rig.until(&id, "cooled", |r| matches!(r.state, State::Cooling { .. }));
    assert_eq!(cooling.state, State::Cooling { until_ms: 1_700_000_000_000 + 602_000 }, "the time it named, and a little after");
}

#[test]
fn what_the_doer_had_said_when_the_limit_came_is_kept_and_the_run_that_failed_is_let_go_once() {
    let doer = Scripted::new("script", vec![vec![Beat::Session("s".into()), Beat::Delta("so far so good".into()), Beat::Fail("429".into())], vec![end("back")]]);
    let rig = Rig::based(vec![doer.clone()], "", |_| {});
    let id = rig.spawn("go", "script");
    rig.until(&id, "cooled", |r| matches!(r.state, State::Cooling { .. }));
    let kinds = rig.kinds(&id);
    let said = rig.of_kind("run.message").into_iter().find(|e| e.data["narration"] == true).expect("the words so far are kept as they were said");
    assert_eq!(said.data["text"], "so far so good");
    assert!(said.seq < rig.of_kind("task.cooling")[0].seq, "before the wait: {kinds:?}");
    rig.clock.advance(30 * 60_000);
    rig.timers.poke();
    rig.finished(&id);
    assert_eq!(doer.ctls(), vec![RunCtl::Abort], "the failed run was let go once, when the wait began, and not again when it ended");
}

#[test]
fn a_limit_that_comes_while_the_task_waits_for_an_answer_fails_it_and_is_not_waited_out() {
    let doer = Scripted::new("script", vec![vec![Beat::Session("s".into()), Beat::Ask(crate::scripted::approval("q", "make")), Beat::Fail("429".into())]]);
    let rig = Rig::based(vec![doer], "", |_| {});
    let id = rig.spawn("go", "script");
    let row = rig.finished(&id);
    assert!(matches!(row.state, State::Stopped { why: Why::Failed { .. } }), "{:?}\n{}", row.state, rig.dump());
    assert_eq!(row.class.as_deref(), Some("rate-limit"));
    assert!(rig.of_kind("task.cooling").is_empty());
    let dropped = rig.of_kind("rule.dropped");
    assert!(dropped[0].data["why"].as_str().unwrap().contains("only a task that is running"), "{}", dropped[0].data);
}

#[test]
fn the_operator_can_end_a_wait_and_the_task_does_not_wake_after_it() {
    let doer = Scripted::new("script", vec![vec![Beat::Session("s".into()), Beat::Fail("429".into())], vec![end("back")]]);
    let rig = Rig::based(vec![doer.clone()], "", |_| {});
    let id = rig.spawn("go", "script");
    rig.until(&id, "cooled", |r| matches!(r.state, State::Cooling { .. }));
    rig.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
    assert_eq!(rig.finished(&id).state, State::Stopped { why: Why::Aborted });
    assert_eq!(rig.timers.pending(), 0, "the timer is let go with the task, not left to find nobody");
    rig.clock.advance(31 * 60_000);
    rig.timers.poke();
    pause();
    assert_eq!(doer.starts().len(), 1, "no run after the time has passed");
    assert_eq!(rig.timers.pending(), 0, "and the timer is gone");
}

#[test]
fn a_wait_counts_against_the_tasks_own_wall() {
    let doer = Scripted::new("script", vec![vec![Beat::Session("s".into()), Beat::Fail("429".into())], vec![end("back")]]);
    let rig = Rig::based(vec![doer], "", |_| {});
    let mut spec = TaskSpec::new("go", "script");
    spec.limits.wall_ms = Some(10 * 60_000);
    let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
    rig.until(&id, "cooled", |r| matches!(r.state, State::Cooling { .. }));
    rig.clock.advance(10 * 60_000);
    rig.timers.poke();
    assert_eq!(rig.finished(&id).state, State::Stopped { why: Why::Wall }, "the wall passed before the limit did");
}

#[test]
fn a_limit_that_ends_later_than_a_task_may_wait_stops_it_and_the_menu_offers_to_carry_on() {
    let doer = Scripted::new("script", vec![vec![Beat::Session("s".into()), Beat::Fail("usage limit".into())]]);
    let rig = Rig::based(vec![doer], &setting("cool.max", "\"10m\""), |_| {});
    let id = rig.spawn("go", "script");
    let row = rig.finished(&id);
    assert!(matches!(&row.state, State::Stopped { why: Why::Failed { message } } if message == "usage limit"), "{:?}", row.state);
    assert_eq!(row.class.as_deref(), Some("rate-limit"));
    let dropped = rig.of_kind("rule.dropped");
    assert_eq!(dropped.len(), 1);
    assert!(dropped[0].data["why"].as_str().unwrap().contains("longer than the 10m"), "{}", dropped[0].data);
    let menu = rig.tasks.menu(&id);
    assert_eq!(menu.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["resume", "abort"], "a limit passes: carry on, not try again");
    assert!(menu[0].recommended && menu[0].label.contains("once the limit is over"));
}

#[test]
fn a_task_that_cannot_carry_on_in_a_session_does_not_wait() {
    let mut turns = vec![vec![Beat::Fail("429".into())]];
    turns.push(vec![end("never")]);
    let no_session = Rig::based(vec![Scripted::new("script", turns.clone())], "", |_| {});
    let id = no_session.spawn("go", "script");
    let row = no_session.finished(&id);
    assert!(matches!(row.state, State::Stopped { why: Why::Failed { .. } }), "{:?}", row.state);
    assert!(no_session.of_kind("rule.dropped")[0].data["why"].as_str().unwrap().contains("cannot carry on in a session"));
    let fixed = Scripted::with_caps("script", Caps { live: true, resume: false, ..Caps::default() }, vec![vec![Beat::Session("s".into()), Beat::Fail("429".into())]]);
    let rig = Rig::based(vec![fixed], "", |_| {});
    let id = rig.spawn("go", "script");
    assert!(matches!(rig.finished(&id).state, State::Stopped { why: Why::Failed { .. } }));
}

#[test]
fn a_task_waits_out_a_limit_three_times_and_the_fourth_fails_it() {
    let doer = Scripted::new("script", vec![vec![Beat::Session("s".into()), Beat::Fail("429".into())]]);
    let rig = Rig::based(vec![doer.clone()], "", |_| {});
    let id = rig.spawn("go", "script");
    for n in 1..=3 {
        rig.until(&id, "cooled", |r| matches!(r.state, State::Cooling { .. }));
        rig.clock.advance(30 * 60_000);
        rig.timers.poke();
        rig.until_event("woke", |e| e.kind == "task.resumed" && e.task.as_deref() == Some(id.as_str()) && e.data["cooled"] == true && rig.of_kind("task.resumed").len() >= n);
    }
    let row = rig.finished(&id);
    assert!(matches!(row.state, State::Stopped { why: Why::Failed { .. } }), "{:?}", row.state);
    assert_eq!(doer.starts().len(), 4, "the first run and three after a wait");
    assert_eq!(rig.of_kind("task.cooling").len(), 3);
}

#[test]
fn a_permanent_failure_is_not_waited_out_and_is_not_recommended_to_try_again() {
    let doer = Scripted::new("script", vec![vec![Beat::Session("s".into()), Beat::Fail("Not logged in · Please run /login".into())]]);
    let rig = Rig::based(vec![doer.clone()], "", |_| {});
    let id = rig.spawn("go", "script");
    let row = rig.finished(&id);
    assert_eq!(row.class.as_deref(), Some("permanent"));
    assert_eq!(doer.starts().len(), 1);
    let menu = rig.tasks.menu(&id);
    assert_eq!(menu.iter().map(|i| (i.id.as_str(), i.recommended)).collect::<Vec<_>>(), [("retry", false), ("abort", false)], "it is there, and nothing says to");
    let stopped = &rig.of_kind("task.stopped")[0];
    assert_eq!(stopped.data["class"], "permanent");
    let transient = Scripted::new("flaky", vec![vec![Beat::Fail("connection reset by peer".into())]]);
    let rig = Rig::based(vec![transient], "", |_| {});
    let id = rig.spawn("go", "flaky");
    assert_eq!(rig.finished(&id).class.as_deref(), Some("transient"));
    assert!(rig.tasks.menu(&id)[0].recommended, "try again is the likely thing for what nothing knows");
}

#[test]
fn the_operators_file_changes_how_long_a_limit_is_waited_by_changing_one_field_of_a_rule() {
    let doer = Scripted::new("script", vec![vec![Beat::Session("s".into()), Beat::Fail("429".into())], vec![end("back")]]);
    let rig = Rig::based(vec![doer], "[[rule]]\nid = \"cool-on-limit\"\ncool_for = \"5m\"\n", |_| {});
    let id = rig.spawn("go", "script");
    let cooling = rig.until(&id, "cooled", |r| matches!(r.state, State::Cooling { .. }));
    assert_eq!(cooling.state, State::Cooling { until_ms: 1_000 + 300_000 });
    assert_eq!(base_tables("").ids("rule").len(), 7, "and the base pack's own rule is as it was");
    let _ = CONTINUE;
}

// ---- shrinking ----

#[test]
fn a_doer_that_cannot_shrink_a_session_goes_on_without_and_says_so() {
    let doer = Scripted::new("script", vec![leaving(2, 0.9), vec![end("turn 2")]]);
    let rig = Rig::based(vec![doer.clone()], "", |_| {});
    let id = rig.spawn("go", "script");
    assert_eq!(rig.finished("t2").state, State::Done { ok: true });
    let starts = doer.starts();
    assert_eq!(starts.iter().map(|s| s.compacting).collect::<Vec<_>>(), [false, false], "no shrinking asked of it");
    assert_eq!(starts[1].text, None, "its own words, plain");
    let said: Vec<String> = rig.of_kind("run.notice").iter().filter(|e| e.task.as_deref() == Some("t2")).map(|e| e.data["text"].as_str().unwrap().into()).collect();
    assert_eq!(said, ["the session was not shrunk: its doer cannot shrink a session"]);
    assert!(rig.of_kind("task.compacted").is_empty());
    let _ = id;
}

#[test]
fn nothing_is_shrunk_while_the_window_has_room_or_is_not_reported_or_the_chain_has_had_its_share() {
    let run = |mine: &str, first: Vec<Beat>| {
        let doer = shrinking(vec![first, vec![end("turn 2")]]);
        let rig = Rig::based(vec![doer.clone()], mine, |_| {});
        let id = rig.spawn("go", "script");
        rig.finished(&id);
        rig.finished("t2");
        doer.starts().iter().map(|s| s.compacting).collect::<Vec<_>>()
    };
    assert_eq!(run("", leaving(1, 0.69)), [false, false], "below the line");
    assert_eq!(run("", vec![Beat::Session("s".into()), Beat::End(Some("t".into()), 1)]), [false, false], "no reading");
    assert_eq!(run("", leaving(1, 0.71)).len(), 3, "above it");
    assert_eq!(run(&setting("compact.at", "0.5"), leaving(1, 0.6)).len(), 3, "the line is a setting");
    assert_eq!(run(&setting("compact.max", "0"), leaving(1, 0.99)), [false, false], "this chain has had its share");
}

#[test]
fn a_chain_is_shrunk_as_often_as_the_setting_says_and_no_more() {
    let doer = shrinking(vec![leaving(1, 0.9), vec![end("shrunk")], leaving(1, 0.9), vec![end("shrunk")], leaving(1, 0.9), vec![end("not asked")]]);
    let rig = Rig::based(vec![doer.clone()], &(setting("compact.max", "2") + &setting("loop.followups", "5")), |_| {});
    let id = rig.spawn("go", "script");
    rig.finished(&id);
    rig.until_event("the third follow-up ends", |e| e.kind == "task.settled" && e.task.as_deref() == Some("t4"));
    barrier_after(&rig);
    let starts = doer.starts();
    assert_eq!(starts.iter().filter(|s| s.compacting).count(), 2, "twice, then plain follow-ups");
    let rows = rig.tasks.book().rows();
    assert_eq!(rows.iter().find(|r| r.id == "t4").map(|r| r.compactions), Some(2));
}

fn barrier_after(rig: &Rig) {
    rig.log.sync();
    pause();
}

#[test]
fn a_shrinking_that_fails_is_a_note_and_the_task_goes_on_with_its_words() {
    let doer = shrinking(vec![leaving(2, 0.8), vec![Beat::Fail("the model is gone".into())], vec![end("turn 2")]]);
    let rig = Rig::based(vec![doer.clone()], "", |_| {});
    let id = rig.spawn("go", "script");
    rig.finished(&id);
    let last = rig.finished("t2");
    assert_eq!((last.state, last.answer.as_deref()), (State::Done { ok: true }, Some("turn 2")), "it did not stop");
    let starts = doer.starts();
    assert_eq!(starts.iter().map(|s| s.compacting).collect::<Vec<_>>(), [false, true, false]);
    assert!(starts[2].text.as_deref().unwrap().contains("MUST KEEP"), "and still carries what must stay");
    let done = rig.of_kind("task.compacted");
    assert_eq!((done[0].data["ok"].clone(), done[0].data["note"].clone()), (serde_json::json!(false), serde_json::Value::Null));
    let said: Vec<String> = rig.of_kind("run.notice").iter().map(|e| e.data["text"].as_str().unwrap().into()).collect();
    assert!(said.iter().any(|t| t.contains("could not be shrunk") && t.contains("the model is gone")), "{said:?}");
    assert!(rig.of_kind("run.error").is_empty() || rig.kinds("t2").iter().all(|k| k != "task.stopped"), "it is a note, not a failure");
}

#[test]
fn what_the_operator_steers_while_a_session_is_shrunk_is_not_lost() {
    let doer = shrinking(vec![leaving(2, 0.8), vec![Beat::Pause(400), end("shrunk")], vec![end("turn 2")]]);
    let rig = Rig::based(vec![doer.clone()], "", |_| {});
    let id = rig.spawn("go", "script");
    rig.finished(&id);
    rig.until_event("shrinking", |e| e.kind == "task.compacting");
    pause();
    rig.tasks.ctl("t2", Ctl::Steer("also keep the tests green".into()), Actor::Operator).unwrap();
    assert_eq!(rig.finished("t2").state, State::Done { ok: true });
    assert!(doer.ctls().iter().all(|c| !matches!(c, RunCtl::Steer(_))), "the shrinking was not interrupted: {:?}", doer.ctls());
    let starts = doer.starts();
    let next = starts[2].text.clone().unwrap();
    assert!(next.contains("also keep the tests green"), "{next}");
    assert!(next.find("MUST KEEP") < next.find("also keep the tests green"), "after what is kept, as the newest word");
    let steered = rig.of_kind("task.steered");
    assert_eq!(steered[0].data["queued"], true);
    // and it is kept for the next shrinking, as one of the operator's words
    assert_eq!(rig.tasks.book().get("t2").unwrap().prompts, ["also keep the tests green"]);
}

#[test]
fn the_window_a_shrinking_leaves_is_said_and_is_not_taken_for_the_next_turns() {
    let doer = shrinking(vec![leaving(2, 0.8), vec![Beat::Window(0.3), end("shrunk")], vec![end("turn 2")]]);
    let rig = Rig::based(vec![doer.clone()], "", |_| {});
    let id = rig.spawn("go", "script");
    rig.finished(&id);
    rig.finished("t2");
    let done = rig.of_kind("task.compacted");
    assert_eq!((done[0].data["before"].clone(), done[0].data["after"].clone()), (serde_json::json!(0.8), serde_json::json!(0.3)));
    let settled: Vec<_> = rig.of_kind("task.settled").into_iter().filter(|e| e.task.as_deref() == Some("t2")).collect();
    assert_eq!(settled[0].data["window"], serde_json::Value::Null, "turn 2 reported none; the shrinking's 0.3 is not its");
}

#[test]
fn a_window_that_is_not_a_fraction_is_made_one() {
    let run = |w: f64| {
        let doer = Scripted::new("script", vec![vec![Beat::Window(w), end("ok")]]);
        let rig = Rig::based(vec![doer], "", |_| {});
        let id = rig.spawn("go", "script");
        rig.finished(&id);
        rig.of_kind("task.settled").remove(0).data["window"].clone()
    };
    assert_eq!(run(1.7), serde_json::json!(1.0));
    assert_eq!(run(-0.2), serde_json::json!(0.0));
    assert_eq!(run(f64::NAN), serde_json::json!(0.0));
}

#[test]
fn a_file_changed_in_two_turns_is_kept_once() {
    let first = vec![Beat::Session("s".into()), Beat::Put("a.rs".into(), "one".into()), Beat::End(Some("t".into()), 1)];
    let second = vec![Beat::Put("a.rs".into(), "two".into()), Beat::Put("b.rs".into(), "x".into()), Beat::Window(0.9), Beat::End(Some("t".into()), 1)];
    let doer = shrinking(vec![first, second, vec![end("shrunk")], vec![end("last")]]);
    let rig = Rig::based(vec![doer.clone()], "", |_| {});
    let id = rig.spawn("go", "script");
    rig.finished(&id);
    rig.finished("t3");
    let told = doer.starts()[2].text.clone().unwrap();
    assert!(told.contains("changed files: a.rs, b.rs\n"), "{told}");
}

#[test]
fn the_operator_can_stop_a_task_while_its_session_is_shrunk() {
    let doer = shrinking(vec![leaving(2, 0.8), vec![Beat::Forever]]);
    let rig = Rig::based(vec![doer.clone()], "", |_| {});
    let id = rig.spawn("go", "script");
    rig.finished(&id);
    rig.until_event("shrinking", |e| e.kind == "task.compacting");
    rig.tasks.ctl("t2", Ctl::Abort, Actor::Operator).unwrap();
    assert_eq!(rig.finished("t2").state, State::Stopped { why: Why::Aborted });
    assert_eq!(doer.starts().len(), 2, "and it was not started again");
    assert!(rig.of_kind("task.compacted").is_empty());
}

#[test]
fn the_decisions_the_operator_made_and_the_files_changed_ride_a_shrinking() {
    use j4n_core::ask::Answer;
    let q = crate::scripted::choice("db", "which database?", &[("postgres", true), ("sqlite", false)]);
    let first = vec![Beat::Session("s".into()), Beat::Ask(q), Beat::Hold, Beat::Put("db/schema.sql".into(), "x".into()), Beat::Window(0.8), Beat::End(Some("turn".into()), 1)];
    let doer = shrinking(vec![first, vec![end("shrunk")], vec![end("turn 2")]]);
    let rig = Rig::based(vec![doer.clone()], "", |_| {});
    let id = rig.spawn("go", "script");
    let ask = match rig.until(&id, "waited", |r| matches!(r.state, State::Waiting { .. })).state {
        State::Waiting { ask } => ask,
        other => panic!("{other:?}"),
    };
    rig.tasks.ctl(&ask, Ctl::Answer(Answer::Pick { id: "sqlite".into() }), Actor::Operator).unwrap();
    rig.finished(&id);
    rig.finished("t3");
    let told = doer.starts()[1].text.clone().unwrap();
    assert!(told.contains("which database? → sqlite") && told.contains("db/schema.sql"), "{told}");
}

// ---- watching ----

fn reads(n: usize, then: Beat) -> Vec<Beat> {
    let mut turn = vec![Beat::Session("s".into())];
    turn.extend((0..n).map(|i| Beat::Tool("Read".into(), format!("file{i}"))));
    turn.push(then);
    turn
}

fn steers(doer: &Scripted) -> Vec<String> {
    doer.ctls().into_iter().filter_map(|c| if let RunCtl::Steer(t) = c { Some(t) } else { None }).collect()
}

#[test]
fn a_task_that_only_reads_is_asked_once_to_say_what_it_found() {
    let doer = Scripted::new("script", vec![reads(40, Beat::Forever)]);
    let rig = Rig::based(vec![doer.clone()], "", |_| {});
    let id = rig.spawn("why is the build slow", "script");
    rig.until_event("steered", |e| e.kind == "task.steered");
    pause();
    let said = steers(&doer);
    assert_eq!(said.len(), 1, "once, however long it goes on: {said:?}");
    // the rule reads the world as it is when it gets to the event, and the doer had already read on
    let n: usize = said[0].strip_prefix("You have read ").and_then(|t| t.split(' ').next()).and_then(|n| n.parse().ok()).expect(&said[0]);
    assert!((31..=40).contains(&n) && said[0].contains(" places and changed nothing."), "{}", said[0]);
    assert!(said[0].contains("The task was: \"why is the build slow\""), "the operator's words, quoted: {}", said[0]);
    let e = &rig.of_kind("task.steered")[0];
    assert_eq!(e.by, Actor::Rule("rabbit".into()));
    rig.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
}

#[test]
fn a_task_that_changed_something_or_read_few_places_is_left_alone() {
    let doer = Scripted::new("script", vec![reads(40, Beat::Forever)]);
    let mut turn = vec![Beat::Session("s".into()), Beat::Changed(vec!["a.rs".into()])];
    turn.extend((0..40).map(|i| Beat::Tool("Grep".into(), format!("q{i}"))));
    turn.push(Beat::Forever);
    let writer = Scripted::new("writer", vec![turn]);
    let few = Scripted::new("few", vec![reads(30, Beat::Forever)]);
    let rig = Rig::based(vec![doer, writer.clone(), few.clone(), idle()], "", |_| {});
    let a = rig.spawn("go", "writer");
    let b = rig.spawn("go", "few");
    barrier(&rig);
    pause();
    assert!(steers(&writer).is_empty() && steers(&few).is_empty(), "{:?} {:?}", steers(&writer), steers(&few));
    for id in [a, b] {
        rig.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
    }
}

#[test]
fn a_task_whose_commands_keep_failing_is_asked_to_step_back_once_and_a_success_starts_the_count_again() {
    let fail = |n: usize| (0..n).map(|i| Beat::Shell(format!("make {i}"), "error".into(), 2)).collect::<Vec<_>>();
    let mut turn = vec![Beat::Session("s".into())];
    turn.extend(fail(3));
    turn.push(Beat::Shell("make ok".into(), "".into(), 0));
    turn.extend(fail(3));
    turn.push(Beat::Forever);
    let steady = Scripted::new("steady", vec![turn]);
    let rig = Rig::based(vec![steady.clone(), Scripted::new("stuck", vec![[vec![Beat::Session("s".into())], fail(6), vec![Beat::Forever]].concat()]), idle()], "", |_| {});
    let a = rig.spawn("go", "steady");
    let b = rig.spawn("go", "stuck");
    rig.until_event("steered", |e| e.kind == "task.steered");
    barrier(&rig);
    pause();
    assert!(steers(&steady).is_empty(), "never four in a row: {:?}", steers(&steady));
    let all = rig.of_kind("task.steered");
    assert_eq!(all.len(), 1, "{}", rig.dump());
    assert_eq!((all[0].task.as_deref(), all[0].by.clone()), (Some(b.as_str()), Actor::Rule("stuck".into())));
    let text = all[0].data["text"].as_str().unwrap();
    let n: usize = text.strip_prefix("The last ").and_then(|t| t.split(' ').next()).and_then(|n| n.parse().ok()).expect(text);
    assert!((4..=6).contains(&n) && text.ends_with(" commands have failed. Step back: say what the failure is, then change the approach."), "{text}");
    for id in [a, b] {
        rig.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
    }
}

#[test]
fn a_task_that_goes_quiet_is_noted_after_the_quiet_has_lasted_and_not_before() {
    let doer = Scripted::new("script", vec![vec![Beat::Session("s".into()), say("working"), Beat::Forever]]);
    let rig = Rig::based(vec![doer], "", |_| {});
    let id = rig.spawn("go", "script");
    rig.until_event("it spoke", |e| e.kind == "run.message");
    pause();
    rig.clock.advance(5 * 60_000 - 1);
    rig.timers.poke();
    pause();
    assert!(rig.of_kind("run.notice").is_empty(), "not yet");
    rig.clock.advance(1);
    rig.timers.poke();
    let notice = rig.until_event("the note", |e| e.kind == "run.notice");
    assert_eq!((notice.by.clone(), notice.task.as_deref()), (Actor::Rule("quiet".into()), Some(id.as_str())));
    assert_eq!(notice.data["text"], "Nothing has been heard from this task for 5 minutes.");
    assert_eq!(rig.timers.pending(), 0, "its own note did not start another wait");
    rig.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
}

#[test]
fn something_heard_in_the_meantime_starts_the_quiet_again_and_a_task_that_ends_takes_the_wait_with_it() {
    let doer = Scripted::new("script", vec![vec![Beat::Session("s".into()), say("one"), Beat::Pause(300), say("two"), Beat::Forever], vec![say("a"), end("done")]]);
    let rig = Rig::based(vec![doer, idle()], "", |_| {});
    let id = rig.spawn("go", "script");
    rig.until_event("the second word", |e| e.kind == "run.message" && e.data["text"] == "two");
    pause();
    assert_eq!(rig.timers.pending(), 1, "one wait, the newer");
    rig.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
    rig.finished(&id);
    pause();
    assert_eq!(rig.timers.pending(), 0, "stopped: the wait is gone");
    rig.clock.advance(6 * 60_000);
    rig.timers.poke();
    pause();
    assert!(rig.of_kind("run.notice").is_empty());
}

#[test]
fn the_quiet_is_counted_from_the_last_word_even_when_the_timer_that_ends_it_was_set_by_the_first() {
    let doer = Scripted::new("script", vec![vec![Beat::Session("s".into()), say("one"), Beat::Pause(400), say("two"), Beat::Forever]]);
    let rig = Rig::based(vec![doer], "", |_| {});
    let id = rig.spawn("go", "script");
    rig.until_event("the first word", |e| e.kind == "run.message" && e.data["text"] == "one");
    rig.clock.advance(4 * 60_000);
    rig.until_event("the second word", |e| e.kind == "run.message" && e.data["text"] == "two");
    pause();
    rig.clock.advance(60_000);
    rig.timers.poke();
    pause();
    assert!(rig.of_kind("run.notice").is_empty(), "five minutes since the first word and one since the second: not quiet yet");
    assert_eq!(rig.timers.pending(), 1, "and the wait went on under a new timer");
    rig.clock.advance(4 * 60_000 - 1);
    rig.timers.poke();
    pause();
    assert!(rig.of_kind("run.notice").is_empty(), "a millisecond short");
    rig.clock.advance(1);
    rig.timers.poke();
    let notice = rig.until_event("the note", |e| e.kind == "run.notice");
    assert_eq!(notice.by, Actor::Rule("quiet".into()));
    rig.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
}

#[test]
fn a_task_waiting_for_the_operator_is_not_noted_as_quiet() {
    let q = crate::scripted::approval("q", "cargo build");
    let doer = Scripted::new("script", vec![vec![Beat::Session("s".into()), say("asking"), Beat::Ask(q), Beat::Hold, end("done")]]);
    let rig = Rig::based(vec![doer], "", |_| {});
    let id = rig.spawn("go", "script");
    rig.until(&id, "waited", |r| matches!(r.state, State::Waiting { .. }));
    pause();
    rig.clock.advance(6 * 60_000);
    rig.timers.poke();
    pause();
    assert!(rig.of_kind("run.notice").is_empty(), "{}", rig.dump());
}

/// A task speaks, asks an approval, waits 4 m 20 s for the operator and resumes.
/// The quiet it waited must not be counted, or the notice comes 40 s after the answer. The wait for an answer is not quiet: the quiet counts from the
/// resume, and the notice is said only when five minutes have passed since then with nothing heard.
#[test]
fn the_quiet_counts_from_the_answer_not_from_before_the_ask() {
    let q = crate::scripted::approval("q", "cargo build");
    // it works a while before it asks, so the quiet is waited for when the ask comes (seq 900 → 903)
    let doer = Scripted::new("script", vec![vec![Beat::Session("s".into()), say("working"), Beat::Pause(300), Beat::Ask(q), Beat::Hold, Beat::Forever]]);
    let rig = Rig::based(vec![doer], "", |_| {});
    let id = rig.spawn("go", "script");
    rig.until(&id, "waited", |r| matches!(r.state, State::Waiting { .. }));
    pause();
    rig.clock.advance(4 * 60_000 + 20_000);
    rig.timers.poke();
    pause();
    let ask = rig.tasks.book().open_asks().first().map(|a| a.ask.clone()).expect("the approval is open");
    rig.tasks.ctl(&ask, Ctl::Answer(j4n_core::ask::Answer::Yes), Actor::Operator).unwrap();
    rig.until_event("resumed", |e| e.kind == "task.resumed" && e.task.as_deref() == Some(id.as_str()));
    pause();
    assert_eq!(rig.timers.pending(), 1, "the quiet is waited for, the ask's own timer is gone\n{}", rig.dump());
    rig.clock.advance(40_000);
    rig.timers.poke();
    pause();
    assert!(rig.of_kind("run.notice").is_empty(), "five minutes since it spoke, 40 s since the answer: not quiet\n{}", rig.dump());
    assert_eq!(rig.timers.pending(), 1, "the wait goes on, counted from the resume\n{}", rig.dump());
    rig.clock.advance(5 * 60_000 - 40_000 - 1);
    rig.timers.poke();
    pause();
    assert!(rig.of_kind("run.notice").is_empty(), "a millisecond short of five minutes since the resume");
    rig.clock.advance(1);
    rig.timers.poke();
    let notice = rig.until_event("the note", |e| e.kind == "run.notice");
    assert_eq!((notice.by.clone(), notice.task.as_deref()), (Actor::Rule("quiet".into()), Some(id.as_str())));
    rig.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
}

#[test]
fn an_answer_that_takes_longer_than_the_quiet_says_nothing_and_the_quiet_counts_from_the_resume() {
    let q = crate::scripted::approval("q", "cargo build");
    let doer = Scripted::new("script", vec![vec![Beat::Session("s".into()), say("working"), Beat::Pause(300), Beat::Ask(q), Beat::Hold, Beat::Forever]]);
    let rig = Rig::based(vec![doer], "", |_| {});
    let id = rig.spawn("go", "script");
    rig.until(&id, "waited", |r| matches!(r.state, State::Waiting { .. }));
    pause();
    rig.clock.advance(20 * 60_000);
    rig.timers.poke();
    pause();
    assert!(rig.of_kind("run.notice").is_empty(), "twenty minutes waiting for the operator is not quiet\n{}", rig.dump());
    let ask = rig.tasks.book().open_asks().first().map(|a| a.ask.clone()).expect("the approval is open");
    rig.tasks.ctl(&ask, Ctl::Answer(j4n_core::ask::Answer::Yes), Actor::Operator).unwrap();
    rig.until_event("resumed", |e| e.kind == "task.resumed" && e.task.as_deref() == Some(id.as_str()));
    pause();
    assert_eq!(rig.timers.pending(), 1, "the held wait has a timer again\n{}", rig.dump());
    rig.clock.advance(5 * 60_000 - 1);
    rig.timers.poke();
    pause();
    assert!(rig.of_kind("run.notice").is_empty(), "a millisecond short of five minutes since the resume");
    rig.clock.advance(1);
    rig.timers.poke();
    let notice = rig.until_event("the note", |e| e.kind == "run.notice");
    assert_eq!((notice.by.clone(), notice.task.as_deref()), (Actor::Rule("quiet".into()), Some(id.as_str())));
    rig.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
}

// ---- what a rule cannot do ----

#[test]
fn an_action_that_cannot_be_carried_out_is_a_dropped_one_and_says_why() {
    let mine = "[[rule]]\nid = \"nag\"\non = \"task.settled\"\nthen = \"steer\"\ntext = \"are you there\"\n[[rule]]\nid = \"hear\"\non = \"rule.dropped\"\nthen = \"notice\"\ntext = \"heard\"\n";
    let rig = Rig::based(vec![Scripted::new("script", vec![vec![end("done")]]), idle()], mine, |_| {});
    let id = rig.spawn("go", "script");
    let dropped = rig.until_event("the refusal", |e| e.kind == "rule.dropped" && e.data["rule"] == "nag");
    assert_eq!((dropped.task.as_deref(), dropped.by.clone()), (Some(id.as_str()), Actor::Core));
    assert!(dropped.data["why"].as_str().unwrap().contains("it is over"), "{}", dropped.data);
    assert_eq!(rig.tasks.book().get(&id).unwrap().state, State::Done { ok: true }, "it did not change what happened");
    barrier(&rig);
    assert!(rig.of_kind("run.notice").iter().all(|e| e.by != Actor::Rule("hear".into())), "what a rule says of its own refusals is not answered: that is how a storm feeds itself");
}

#[test]
fn a_rule_that_answers_its_own_words_is_cut_off_and_the_cut_is_said_once() {
    let mine = "[[rule]]\nid = \"echo\"\non = \"rule.fired\"\nthen = \"notice\"\ntext = \"again\"\n";
    let rig = Rig::based(vec![Scripted::new("script", vec![vec![Beat::Session("s".into()), end("done")]]), idle()], &format!("{mine}[[rule]]\nid = \"first\"\non = \"task.settled\"\nthen = \"notice\"\ntext = \"start\"\n"), |_| {});
    let id = rig.spawn("go", "script");
    rig.finished(&id);
    rig.until_event("the cut", |e| e.kind == "rule.dropped");
    barrier(&rig);
    // the barrier task is a task too, and its end starts a chain of its own: this is about the first
    let of_first = |kind: &str| rig.of_kind(kind).into_iter().filter(|e| e.task.as_deref() == Some(id.as_str())).collect::<Vec<_>>();
    let fired = of_first("rule.fired").len();
    assert!((8..=18).contains(&fired), "{fired} sayings before the chain ended");
    assert_eq!(of_first("rule.dropped").len(), 1, "once: {}", rig.dump());
    assert!(of_first("rule.dropped")[0].data["why"].as_str().unwrap().contains("a chain of 8 reactions"));
}

#[test]
fn a_flashed_event_is_answered_too() {
    let mine = "[[rule]]\nid = \"talking\"\non = \"run.delta\"\nthen = \"notice\"\ntext = \"it is talking\"\nlimit = { max = 1 }\n";
    let doer = Scripted::new("script", vec![vec![Beat::Delta("hel".into()), Beat::Delta("lo".into()), end("hello")]]);
    let rig = Rig::based(vec![doer], mine, |_| {});
    let id = rig.spawn("go", "script");
    rig.finished(&id);
    let notice = rig.until_event("the note", |e| e.kind == "run.notice" && e.by == Actor::Rule("talking".into()));
    assert_eq!(notice.data["text"], "it is talking");
    assert!(rig.of_kind("run.delta").is_empty(), "and the flash was never written");
    pause();
    assert_eq!(rig.of_kind("run.notice").len(), 1, "once");
}

#[test]
fn the_cap_rule_does_not_park_a_task_that_is_already_over() {
    let doer = Scripted::new("script", vec![vec![Beat::Spend(0.005), end("done")]]);
    let rig = Rig::based(vec![doer, idle()], "", |_| {});
    let mut spec = TaskSpec::new("go", "script");
    spec.limits.usd_soft = Some(0.004);
    let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
    let row = rig.finished(&id);
    barrier(&rig);
    assert!(matches!(row.state, State::Done { .. } | State::Stopped { why: Why::Capped }), "one or the other, never both: {:?}", row.state);
    let ends = rig.kinds(&id).iter().filter(|k| *k == "task.settled" || *k == "task.stopped").count();
    assert_eq!(ends, 1);
}

#[test]
fn a_rule_can_answer_an_event_that_is_about_no_task_by_saying_something_or_starting_a_task_of_its_own() {
    use j4n_core::log::New;
    let mine = r#"
[[rule]]
id = "tell"
on = "probe.hit"
then = "notice"
text = "saw {data.what}"
[[rule]]
id = "begin"
on = "probe.hit"
then = "start"
doer = "idle"
text = "check it"
[[rule]]
id = "nameless"
on = "probe.hit"
then = "start"
text = "check it"
[[rule]]
id = "nag"
on = "probe.hit"
then = "steer"
text = "hurry"
"#;
    let rig = Rig::based(vec![idle()], mine, |_| {});
    let hit = New::of("probe.hit", Actor::Operator, serde_json::json!({ "what": "a lamp" })).thread("main");
    rig.log.append(hit).unwrap();
    let began = rig.until_event("the task it began", |e| e.kind == "task.queued");
    assert_eq!((began.by.clone(), began.thread.as_deref(), began.task.as_deref()), (Actor::Rule("begin".into()), Some("main"), Some("t1")));
    assert_eq!(rig.finished("t1").spec.goal, "check it");
    let notice = rig.until_event("the note", |e| e.kind == "run.notice");
    assert_eq!((notice.task, notice.thread.as_deref(), notice.data["text"].as_str()), (None, Some("main"), Some("saw a lamp")));
    rig.until_event("two refusals", |e| e.kind == "rule.dropped" && e.data["rule"] == "nag");
    let said: std::collections::BTreeMap<String, String> = rig.of_kind("rule.dropped").iter().map(|e| (e.data["rule"].as_str().unwrap().to_string(), e.data["why"].as_str().unwrap().to_string())).collect();
    assert!(said["nameless"].contains("names no doer"), "{said:?}");
    assert!(said["nag"].contains("not about a task"), "{said:?}");
    assert!(rig.of_kind("rule.fired").iter().all(|e| e.task.is_none() && e.thread.as_deref() == Some("main")));
}

#[test]
fn a_session_that_is_being_shrunk_is_not_retuned_in_the_middle() {
    let doer = shrinking(vec![leaving(2, 0.8), vec![Beat::Pause(300), end("shrunk")], vec![end("turn 2")]]);
    let rig = Rig::based(vec![doer.clone()], "", |_| {});
    let id = rig.spawn("go", "script");
    rig.finished(&id);
    rig.until_event("shrinking", |e| e.kind == "task.compacting");
    let err = rig.tasks.ctl("t2", Ctl::Retune(j4n_core::task::Tune { model: Some("other".into()), ..Default::default() }), Actor::Operator).unwrap_err();
    assert!(err.msg.contains("is being shrunk"), "{err}");
    assert_eq!(rig.finished("t2").state, State::Done { ok: true }, "and it went on as it was");
    assert_eq!(doer.starts().len(), 3);
}

// ---- what the row of a task says ----

#[test]
fn a_row_says_what_its_chain_and_its_end_were_and_a_task_that_ends_in_any_way_keeps_them() {
    use j4n_core::value::{Key, Value};
    let doer = shrinking(vec![leaving(2, 0.8), vec![end("shrunk")], vec![Beat::Session("s".into()), Beat::Fail("429".into())], vec![end("back")]]);
    let rig = Rig::based(vec![doer], "", |_| {});
    let id = rig.spawn("write the report", "script");
    rig.finished(&id);
    rig.until_event("t2 is cooling", |e| e.kind == "task.cooling");
    let read = |task: &str, field: &str| rig.folds.read("tasks", &[Key::Name(task.into()), Key::Name(field.into())], 0);
    // the fold has its own thread: the event is in the Log a moment before the fold has taken it, so the reads wait for the fold
    let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while read("t2", "state") != Value::text("cooling") && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(read("t1", "window"), Value::num(0.8), "what the doer said of its window");
    assert_eq!((read("t2", "auto"), read("t2", "compactions"), read("t2", "origin")), (Value::num(1.0), Value::num(1.0), Value::text("write the report")));
    assert_eq!(read("t1", "class"), Value::Missing);
    assert_eq!(read("t2", "state"), Value::text("cooling"));
    assert_eq!(read("t2", "until_ms"), Value::num(1_800_000.0 + 1_000.0));
    let children = rig.folds.read("tasks", &[Key::Name("t1".into()), Key::Name("children".into())], 0);
    assert_eq!(children, Value::List(vec![]), "a task that asked nothing has no children, and it is still read");
}

// ---- the same piece of work ----

#[test]
fn a_task_carried_on_by_a_rule_has_what_the_one_before_left_of_its_time_and_its_money() {
    let doer = Scripted::new("script", vec![vec![Beat::Session("s".into()), Beat::Spend(0.003), Beat::Pause(100), Beat::End(Some("turn".into()), 1)], vec![Beat::Session("s".into()), Beat::Spend(0.0005), Beat::End(Some("turn".into()), 0)]]);
    let rig = Rig::based(vec![doer.clone()], "", |_| {});
    let mut spec = TaskSpec::new("go", "script");
    (spec.limits.wall_ms, spec.limits.usd_soft) = (Some(100_000), Some(0.004));
    let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
    rig.until(&id, "has run a while", |r| r.spent > 0);
    rig.clock.advance(30_000);
    rig.finished(&id);
    let second = rig.finished("t2");
    assert_eq!(second.state, State::Done { ok: true }, "{}", rig.dump());
    assert_eq!(second.spec.limits.wall_ms, Some(70_000), "100 s, less the 30 it used");
    assert!((second.spec.limits.usd_soft.unwrap() - 0.001).abs() < 1e-9, "$0.004, less the $0.003 it spent: {:?}", second.spec.limits.usd_soft);
}

#[test]
fn a_task_whose_time_or_money_has_run_out_is_not_carried_on() {
    for (what, wall, soft, spent) in [("time", Some(10_000u64), None, 0.0), ("money", None, Some(0.004), 0.004)] {
        let doer = Scripted::new("script", vec![vec![Beat::Session("s".into()), Beat::Spend(spent), Beat::Pause(400), Beat::End(Some("turn".into()), 1)]]);
        let rig = Rig::based(vec![doer.clone(), idle()], "", |_| {});
        let mut spec = TaskSpec::new("go", "script");
        (spec.limits.wall_ms, spec.limits.usd_soft) = (wall, soft);
        let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
        rig.until_event("it spent", |e| e.kind == "run.metrics");
        pause();
        // the clock moves and nothing wakes the timer thread before the task ends on its own, having used all its time
        rig.clock.advance(10_000);
        let row = rig.finished(&id);
        assert_eq!(row.state, State::Done { ok: true }, "{what}: {}", rig.dump());
        let dropped = rig.until_event("the refusal", |e| e.kind == "rule.dropped");
        assert_eq!(dropped.data["rule"], "followups");
        assert!(dropped.data["why"].as_str().unwrap().contains(&format!("the {what} it was given has run out")), "{what}: {}", dropped.data);
        barrier(&rig);
        assert_eq!(doer.starts().len(), 1, "{what}: no second run");
    }
}
