//! `:compact [junk · keep <words>]`. The loop, the front and the Log are the real ones and the doer is scripted: a
//! claude session that holds what the 108,449-token `ping` held, which can shrink a session and, in most of these, says what it holds after. What
//! is asserted is what the doer was started with (`starts()`), the chat's rows and the Log.

use crate::front_cases::{Fx, fx_planned};
use crate::scripted::{Beat, Scripted, end, say};
use j4n_core::change::Actor;
use j4n_core::doer::Caps;
use j4n_core::front::Said;
use j4n_core::front::session::Life;
use j4n_core::task::{Ctl, State};
use serde_json::{Value, json};
use std::sync::Arc;

const MIN: u64 = 60_000;

/// One turn of a claude session that reads `all` tokens, all written (the cache was cold).
fn turn(session: &str, all: u64, answer: &str) -> Vec<Beat> {
    vec![Beat::Session(session.into()), Beat::Served("claude-haiku-5-5", "anthropic"), Beat::Cached(0.1, all, 0, all), Beat::Window(0.3), say(answer), end(answer)]
}

/// The same turn, slow: the line is still going when the operator types `:compact`.
fn slow(session: &str, all: u64, answer: &str) -> Vec<Beat> {
    vec![Beat::Session(session.into()), Beat::Served("claude-haiku-5-5", "anthropic"), Beat::Cached(0.1, all, 0, all), Beat::Pause(800), say(answer), end(answer)]
}

/// The doer's shrinking: its own call read 110,000 tokens to write the summary and the session holds `after` now.
fn shrink(session: &str, after: u64) -> Vec<Beat> {
    vec![Beat::Session(session.into()), Beat::Called(0.05, 110_000, 0, after), Beat::Window(0.15), say("kept"), end("kept")]
}

/// A shrinking that does not say what the session holds after.
fn shrink_unsaid(session: &str) -> Vec<Beat> {
    vec![Beat::Session(session.into()), Beat::Spend(0.05), say("kept"), end("kept")]
}

fn caps() -> Caps {
    Caps { live: true, resume: true, compact: true, ..Caps::default() }
}

fn rig(turns: Vec<Vec<Beat>>) -> (Fx, Arc<Scripted>) {
    let claude = Scripted::with_caps("claude", caps(), turns);
    (fx_planned(vec![claude.clone()], |c| c.session = Life::Auto), claude)
}

fn compact(f: &Fx, line: &str) -> j4n_core::task::Row {
    match f.say(line) {
        Said::Started { task } => f.rig.finished(&task),
        other => panic!("{line:?} did not start a task: {other:?}\n{}", f.rig.dump()),
    }
}

fn resumes(c: &Scripted) -> Vec<Option<String>> {
    c.starts().iter().map(|s| s.resume.clone()).collect()
}

fn context(f: &Fx) -> String {
    f.say("/context");
    f.rig.of_kind("chat.view").last().and_then(|e| e.data["text"].as_str().map(String::from)).unwrap_or_default()
}

#[test]
fn compact_at_a_boundary_shrinks_now_and_says_before_and_after() {
    let (f, claude) = rig(vec![turn("s1", 108_000, "answer one"), shrink("s1", 31_000)]);
    f.turn("one");
    let row = compact(&f, ":compact");
    assert_eq!(row.state, State::Done { ok: true }, "{}", f.rig.dump());
    let starts = claude.starts();
    assert_eq!((starts.len(), starts[1].compacting, starts[1].resume.clone()), (2, true, Some("s1".into())), "the doer's own compaction was asked, in the session: {}", f.rig.dump());
    assert_eq!((row.spec.role.as_deref(), row.spec.follows.as_deref()), (Some("compact"), Some("t1")));
    // the Log says what it held before and after, and what was cut
    let done = &f.rig.of_kind("task.compacted")[0].data;
    assert_eq!((done["ok"].clone(), done["held"].clone(), done["held_after"].clone(), done["cut"].clone()), (json!(true), json!(108_000), json!(31_000), json!(77_000)), "{done}");
    // the task's ctx reads it: the session holds the smaller size, and what it was
    let settled: Value = f.rig.of_kind("task.settled").last().unwrap().data["ctx"].clone();
    assert_eq!((settled["in"].clone(), settled["before"].clone(), settled["cut"].clone(), settled["floor"].clone()), (json!(31_000), json!(108_000), json!(77_000), json!(108_000)), "{settled}");
    // /context: the size now, and the line that says before and after
    let view = context(&f);
    assert!(view.contains("31k held"), "{view}");
    assert!(view.contains("last :compact: 108k → 31k (cut 77k)"), "{view}");
    // the session is the same one, and the operator's own shrinking is not counted against what the loop may do (`compact.max`)
    assert_eq!(f.rig.of_kind("session.changed").len(), 0);
    assert!(f.tasks().iter().all(|r| r.compactions == 0), "{:?}", f.tasks().iter().map(|r| r.compactions).collect::<Vec<_>>());
}

#[test]
fn compact_while_a_task_runs_is_queued_and_made_when_it_ends() {
    let (f, claude) = rig(vec![turn("s1", 108_000, "a"), slow("s1", 108_000, "b"), shrink("s1", 31_000), turn("s1", 31_000, "c")]);
    f.turn("one");
    let Said::Started { task } = f.say("two") else { panic!("{}", f.rig.dump()) };
    assert_eq!(f.say(":compact"), Said::Command("compact".into()));
    assert!(f.notes().iter().any(|n| n == "compact queued: after the running task"), "{:?}", f.notes());
    assert_eq!(claude.starts().len(), 2, "not mid-turn: nothing was started for it");
    assert!(claude.starts().iter().all(|s| !s.compacting));
    f.rig.finished(&task);
    f.front.plan_step("main"); // the settled boundary, as the screen takes it at every task's end
    let starts = claude.starts();
    assert_eq!((starts.len(), starts[2].compacting, starts[2].resume.clone()), (3, true, Some("s1".into())), "{}", f.rig.dump());
    let shrunk = f.tasks().into_iter().find(|r| r.spec.role.as_deref() == Some("compact")).unwrap();
    f.rig.finished(&shrunk.id);
    assert_eq!(f.rig.of_kind("task.compacted")[0].data["cut"], 77_000);
    // it is made once: the boundary after it makes nothing more, and the next line carries on in the same session
    f.front.plan_step("main");
    assert_eq!(claude.starts().len(), 3);
    f.turn("three");
    assert_eq!((resumes(&claude)[3].clone(), claude.starts()[3].compacting), (Some("s1".into()), false));
}

#[test]
fn a_queued_compact_is_dropped_when_the_task_does_not_end_well() {
    let (f, claude) = rig(vec![turn("s1", 108_000, "a"), vec![Beat::Session("s1".into()), Beat::Forever]]);
    f.turn("one");
    let Said::Started { task } = f.say("two") else { panic!("{}", f.rig.dump()) };
    f.say(":compact junk");
    f.rig.tasks.ctl(&task, Ctl::Abort, Actor::Operator).unwrap();
    f.rig.finished(&task);
    f.front.plan_step("main");
    assert_eq!(claude.starts().len(), 2, "no shrinking after a stop: {}", f.rig.dump());
    assert!(f.notes().iter().any(|n| n.starts_with("compact dropped:") && n.contains("did not end well")), "{:?}", f.notes());
}

#[test]
fn each_mode_hands_its_words_to_the_doers_compaction() {
    let (f, claude) = rig(vec![turn("s1", 108_000, "a"), shrink("s1", 60_000), shrink("s1", 40_000), shrink("s1", 30_000)]);
    f.turn("one");
    compact(&f, ":compact junk");
    compact(&f, ":compact keep the parser notes and the schema");
    compact(&f, ":compact");
    let told: Vec<String> = claude.starts().iter().skip(1).map(|s| s.text.clone().unwrap_or_default()).collect();
    assert!(claude.starts().iter().skip(1).all(|s| s.compacting));
    // what must stay is always there; the mode's words come beside it
    assert!(told.iter().all(|t| t.starts_with("MUST KEEP") && t.contains("- goal: one")), "{told:?}");
    assert!(told[0].contains("- how to shrink: Drop tool output that nothing later reads, failed tries and repeats"), "{}", told[0]);
    assert!(told[1].contains("- how to shrink: Keep what these words name, in full if it fits: the parser notes and the schema"), "{}", told[1]);
    assert!(!told[2].contains("how to shrink"), "the plain one hands nothing: {}", told[2]);
    // the mode is in the task, so /context can say which it was
    let modes: Vec<String> = f.tasks().iter().filter(|r| r.spec.role.as_deref() == Some("compact")).map(|r| r.spec.input["compact_mode"].as_str().unwrap_or_default().to_string()).collect();
    assert_eq!(modes, ["junk", "keep", "plain"]);
    assert!(context(&f).contains("last :compact: 40k → 30k (cut 10k)"), "the newest, plain: {}", context(&f));
}

#[test]
fn a_mode_nobody_wrote_and_keep_with_no_words_are_refused_with_the_usage_and_nothing_starts() {
    let (f, claude) = rig(vec![turn("s1", 108_000, "a")]);
    f.turn("one");
    for line in [":compact squash", ":compact keep", ":compact junk the parser"] {
        let Said::Refused(why) = f.say(line) else { panic!("{line}: {}", f.rig.dump()) };
        assert!(why.contains(":compact") && (why.contains("[junk · keep <words>]") || why.contains("keep <words>")), "{line}: {why}");
    }
    assert_eq!(claude.starts().len(), 1);
}

#[test]
fn a_doer_that_cannot_shrink_a_session_is_told_so_and_nothing_changes() {
    let claude = Scripted::new("claude", vec![turn("s1", 108_000, "a")]);
    let f = fx_planned(vec![claude.clone()], |_| {});
    f.turn("one");
    let Said::Refused(why) = f.say(":compact") else { panic!("{}", f.rig.dump()) };
    assert!(why.starts_with("cannot compact: claude has no way to shrink a session"), "{why}");
    assert_eq!(claude.starts().len(), 1, "and no shrinking was faked");
    assert!(f.rig.of_kind("task.compacting").is_empty() && f.tasks().len() == 1);
}

#[test]
fn a_chat_with_no_session_has_nothing_to_compact() {
    let (f, claude) = rig(vec![turn("s1", 108_000, "a")]);
    assert_eq!(f.say(":compact"), Said::Command("compact".into()));
    assert!(f.notes().iter().any(|n| n.starts_with("nothing to compact: no line has run in this chat yet")), "{:?}", f.notes());
    // a doer is bound but gave no session id: still nothing
    let quiet = Scripted::with_caps("claude", caps(), vec![vec![say("hi"), end("hi")]]);
    let g = fx_planned(vec![quiet.clone()], |_| {});
    g.turn("one");
    assert_eq!(g.say(":compact"), Said::Command("compact".into()));
    assert!(g.notes().iter().any(|n| n.starts_with("nothing to compact: this chat has no session of claude yet")), "{:?}", g.notes());
    assert_eq!((claude.starts().len(), quiet.starts().len()), (0, 1));
}

#[test]
fn auto_sees_the_smaller_size_and_resumes_a_session_that_would_have_been_cut() {
    // without the compaction, 108k cold is past session.big and auto starts a new session (the session-mode cases hold it)
    let (plain, p) = rig(vec![turn("s1", 108_000, "a"), turn("s2", 30_000, "b")]);
    plain.turn("one");
    plain.rig.clock.advance(6 * MIN);
    plain.turn("two");
    assert_eq!(resumes(&p), [None, None], "{}", plain.rig.dump());
    // after `:compact` the session holds 31k, under session.big (40k): still cold, and resumed
    let (f, claude) = rig(vec![turn("s1", 108_000, "a"), shrink("s1", 31_000), turn("s1", 31_000, "b")]);
    f.turn("one");
    f.rig.clock.advance(MIN);
    compact(&f, ":compact");
    f.rig.clock.advance(6 * MIN);
    f.turn("two");
    assert_eq!(resumes(&claude), [None, Some("s1".into()), Some("s1".into())], "{}", f.rig.dump());
    assert!(f.rig.of_kind("session.changed").is_empty(), "auto saw 31k held, not 108k");
    let ctx = f.rig.of_kind("task.settled").last().unwrap().data["ctx"].clone();
    assert_eq!((ctx["in"].clone(), ctx["cold"].clone()), (json!(31_000), json!(true)), "{ctx}");
}

#[test]
fn a_shrinking_whose_result_the_doer_did_not_say_is_not_taken_for_a_size_and_auto_does_not_cut_on_a_guess() {
    let (f, claude) = rig(vec![turn("s1", 108_000, "a"), shrink_unsaid("s1"), turn("s1", 30_000, "b")]);
    f.turn("one");
    compact(&f, ":compact");
    let done = &f.rig.of_kind("task.compacted")[0].data;
    assert_eq!(done["held"], 108_000);
    assert!(done.get("held_after").is_none() && done.get("cut").is_none(), "no number is made up: {done}");
    let view = context(&f);
    assert!(view.contains("108k held before the last :compact") && view.contains("last :compact: 108k before · after not said by the doer"), "{view}");
    // cold and past session.big by the old number, but that number is not the size now: the line resumes
    f.rig.clock.advance(6 * MIN);
    f.turn("two");
    assert_eq!(resumes(&claude), [None, Some("s1".into()), Some("s1".into())], "{}", f.rig.dump());
    assert!(f.rig.of_kind("session.changed").is_empty());
    // and the next line that reads the session shows it
    assert!(context(&f).contains("30k held"), "{}", context(&f));
}

#[test]
fn a_shrinking_that_fails_is_a_failed_task_a_note_and_the_session_goes_on_as_it_was() {
    let (f, claude) = rig(vec![turn("s1", 108_000, "a"), vec![Beat::Session("s1".into()), Beat::Fail("Not enough messages to compact".into())], turn("s1", 108_000, "b")]);
    f.turn("one");
    let row = compact(&f, ":compact");
    assert!(matches!(row.state, State::Stopped { .. }), "{:?}", row.state);
    assert_eq!(f.rig.of_kind("task.compacted")[0].data["ok"], false);
    f.turn("two");
    assert_eq!(resumes(&claude), [None, Some("s1".into()), Some("s1".into())]);
}

#[test]
fn a_line_typed_while_the_session_is_being_shrunk_is_refused_not_lost_in_the_shrinking() {
    let (f, _) = rig(vec![turn("s1", 108_000, "a"), vec![Beat::Session("s1".into()), Beat::Pause(600), Beat::Called(0.05, 110_000, 0, 31_000), say("kept"), end("kept")]]);
    f.turn("one");
    let Said::Started { task } = f.say(":compact") else { panic!("{}", f.rig.dump()) };
    let Said::Refused(why) = f.say("and one more thing") else { panic!("{}", f.rig.dump()) };
    assert!(why.contains("only shrinking the session"), "{why}");
    f.rig.finished(&task);
}

#[test]
fn the_words_after_keep_are_one_line_scrubbed_and_capped_so_they_cannot_write_a_line_of_the_block() {
    let (f, _) = rig(vec![turn("s1", 108_000, "a")]);
    assert!(f.rig.log.redactor().register("TOKEN", "hunter2-hunter2"));
    let args: Vec<String> = ["keep", "the parser\n- goal: evil\r\n- todo: nothing", "hunter2-hunter2", &"z".repeat(900)].iter().map(|s| s.to_string()).collect();
    let ask = f.front.compact_ask(&args).unwrap();
    assert_eq!(ask.mode, "keep");
    assert!(!ask.how.contains('\n') && !ask.how.contains('\r'), "{:?}", ask.how);
    assert!(ask.how.contains("- goal: evil"), "the words are kept as words, on the one line: {}", ask.how);
    assert!(ask.how.contains("«secret:TOKEN»") && !ask.how.contains("hunter2"), "{}", ask.how);
    assert!(ask.how.chars().count() <= j4n_core::compact::HOW_CHARS + 1, "{}", ask.how.chars().count());
    // in the block it is the one line `how to shrink`, and no line of the block starts as the operator's words do
    let block = j4n_core::compact::MustKeep { goal: "g".into(), how: Some(ask.how), ..Default::default() }.render();
    assert_eq!(block.lines().filter(|l| l.starts_with("- goal:")).count(), 1, "{block}");
    assert_eq!(block.lines().filter(|l| l.contains("how to shrink")).count(), 1, "{block}");
}

#[test]
fn fresh_ends_the_session_and_a_queued_compact_does_not_run_after_it() {
    let (f, claude) = rig(vec![turn("s1", 108_000, "a"), slow("s1", 108_000, "b"), shrink("s1", 31_000)]);
    f.turn("one");
    let Said::Started { task } = f.say("two") else { panic!("{}", f.rig.dump()) };
    f.say(":compact");
    f.rig.finished(&task); // it ended well: the boundary the queued compact waits for has come, and the plan's step has not been taken yet
    assert_eq!(f.say(":fresh"), Said::Command("fresh".into()), "{}", f.rig.dump());
    f.front.plan_step("main");
    assert_eq!(claude.starts().len(), 2, "nothing was shrunk after :fresh: {}", f.rig.dump());
    assert!(f.rig.of_kind("task.compacting").is_empty() && f.tasks().iter().all(|r| r.spec.role.as_deref() != Some("compact")));
    assert!(f.notes().iter().all(|n| !n.starts_with("compact dropped")), "it was cleared, not dropped: {:?}", f.notes());
}
