//! A lane Main worked itself under `orchestrate` is reviewed on another doer: the review reads what the turn changed, its concerns go back to Main as a
//! turn of their own, which is reviewed again, at most `sends` rounds; a turn that changed nothing, the setting off or another word of `lanes` start nothing.

use crate::branch_validate::{caps, checker};
use crate::front_cases::{Fx, fx};
use crate::scripted::{Beat, Scripted, end};
use j4n_core::branch::Switch;
use j4n_core::front::Cfg;
use serde_json::{Value, json};
use std::sync::Arc;

const LINE: &str = "build a chess game in the terminal with pieces and castles";

/// Main's turns: each writes `game.py` (or nothing) and says what it did.
fn main_doer(turns: &[(&str, bool)]) -> Arc<Scripted> {
    let script = turns
        .iter()
        .enumerate()
        .map(|(n, (said, writes))| {
            let mut t = vec![Beat::Session("claude-s".into())];
            if *writes {
                t.push(Beat::Put("game.py".into(), format!("# round {n}\n")));
            }
            t.push(end(said));
            t
        })
        .collect();
    Scripted::with_caps("claude", caps(), script)
}

fn review(open: &[&str]) -> String {
    json!({ "answer": "read game.py", "sure": "mid", "found": [], "ran": [], "open": open }).to_string()
}

fn reviewer(reports: &[String]) -> Arc<Scripted> {
    Scripted::with_caps("pi", caps(), reports.iter().map(|r| vec![Beat::Session("pi-s".into()), end(r)]).collect())
}

fn orchestrating(c: &mut Cfg) {
    c.doer = Some("claude".into());
    c.lanes = Switch::Orchestrate;
}

fn rig(main: Arc<Scripted>, pi: Arc<Scripted>, tune: impl FnOnce(&mut Cfg)) -> Fx {
    fx(vec![main, pi], |c| {
        orchestrating(c);
        tune(c);
    })
}

/// Every task ends, then the plan step runs; `n` times.
fn steps(f: &Fx, n: usize) {
    for _ in 0..n {
        for r in f.tasks() {
            f.rig.finished(&r.id);
        }
        f.front.plan_step("main");
    }
}

fn events(f: &Fx, kind: &str) -> Vec<Value> {
    f.rig.of_kind(kind).into_iter().map(|e| e.data).collect()
}

fn reviews(f: &Fx) -> Vec<j4n_core::task::Row> {
    f.tasks().into_iter().filter(|r| r.spec.role.as_deref() == Some("review")).collect()
}

#[test]
fn a_lane_main_worked_that_changed_files_is_reviewed_on_another_doer_and_its_concerns_go_back_to_main_until_none_is_left() {
    let f = rig(main_doer(&[("built it", true), ("fixed the score", true)]), reviewer(&[review(&["game.py:3 the score never resets"]), review(&[])]), |_| {});
    f.front.say("main", LINE);
    steps(&f, 6);
    let r = reviews(&f);
    assert_eq!(r.len(), 2, "one review per round: {}", f.rig.dump());
    assert!(r.iter().all(|t| t.spec.doer == "pi" && t.spec.tune.mode == j4n_core::task::Mode::Read), "on another doer than the writer, reading only: {r:?}");
    assert!(r[0].spec.goal.contains(LINE) && r[0].spec.goal.contains("game.py"), "the line and the files it changed: {}", r[0].spec.goal);
    let ended = events(&f, "second.ended");
    assert_eq!(ended.iter().map(|e| e["outcome"].as_str().unwrap()).collect::<Vec<_>>(), ["sent-back", "clean"], "{ended:?}");
    // the concern went back to Main as a turn of its own, which carries the line, and the second review was of that turn
    let back = f.tasks().into_iter().find(|t| t.spec.goal.starts_with("A review of your last change")).expect("Main's turn on the concerns");
    assert!(back.spec.goal.contains("- game.py:3 the score never resets") && back.spec.doer == "claude", "{}", back.spec.goal);
    assert_eq!((back.spec.input["second"]["round"].clone(), back.spec.input["second"]["line"].clone()), (json!(1), json!(LINE)));
    assert_eq!(r[1].spec.parent.as_deref(), Some(back.id.as_str()));
    assert!(r[1].spec.goal.contains(LINE), "the second review is told the operator's line, not the concerns: {}", r[1].spec.goal);
    let notes = f.notes();
    assert!(notes.iter().any(|n| n.contains("back to Main (1 of 3)")) && notes.iter().any(|n| n == "reviewed on pi: no concern"), "{notes:?}");
}

#[test]
fn after_the_rounds_the_setting_allows_the_concerns_left_are_the_operators_and_nothing_more_is_started() {
    let open = review(&["game.py:9 a castle can drop the chess inside a wall"]);
    let f = rig(main_doer(&[("built", true), ("fixed", true)]), reviewer(&[open.clone(), open.clone()]), |c| c.sends = 1);
    f.front.say("main", LINE);
    steps(&f, 8);
    let ended = events(&f, "second.ended");
    assert_eq!(ended.iter().map(|e| e["outcome"].as_str().unwrap()).collect::<Vec<_>>(), ["sent-back", "left"], "{ended:?}");
    assert_eq!(reviews(&f).len(), 2);
    assert!(f.notes().iter().any(|n| n.starts_with("after 1 rounds the review on pi still names 1 concern, yours to decide: game.py:9")), "{:?}", f.notes());
}

#[test]
fn a_turn_that_changed_nothing_the_setting_off_or_another_word_of_lanes_starts_no_review() {
    // changed nothing: an answer
    let f = rig(main_doer(&[("it is a chess game", false)]), reviewer(&[review(&[])]), |_| {});
    f.front.say("main", "what does this project do?");
    steps(&f, 3);
    assert!(reviews(&f).is_empty() && events(&f, "second.started").is_empty(), "{}", f.rig.dump());
    // the setting off
    let f = rig(main_doer(&[("built", true)]), reviewer(&[review(&[])]), |c| c.orchestrate.review = false);
    f.front.say("main", LINE);
    steps(&f, 3);
    assert!(reviews(&f).is_empty(), "{}", f.rig.dump());
    // under `auto` the line is the plan's, as before
    let f = rig(main_doer(&[("built", true)]), reviewer(&[review(&[])]), |c| c.lanes = Switch::Single);
    f.front.say("main", LINE);
    steps(&f, 3);
    assert!(reviews(&f).is_empty(), "{}", f.rig.dump());
}

#[test]
fn a_review_that_did_not_finish_sends_nothing_back_and_the_step_acts_once() {
    let fails = Scripted::with_caps("pi", caps(), vec![vec![Beat::Fail("no login".into())]]);
    let f = rig(main_doer(&[("built", true)]), fails, |_| {});
    f.front.say("main", LINE);
    steps(&f, 5);
    let ended = events(&f, "second.ended");
    assert_eq!(ended.len(), 1, "{ended:?}");
    assert_eq!(ended[0]["outcome"], "failed");
    assert_eq!(f.tasks().iter().filter(|t| t.spec.doer == "claude").count(), 1, "nothing went back to Main");
}

#[test]
fn main_s_turn_is_reviewed_when_a_check_of_it_came_after_it_on_the_thread() {
    // the lane check of Main's turn is the newest task of the thread; the step must look at Main's turn, not at the check
    let f = fx(vec![main_doer(&[("built it", true)]), reviewer(&[review(&[])]), checker(2)], orchestrating);
    f.front.say("main", LINE);
    let turn = f.tasks().into_iter().find(|t| t.spec.doer == "claude").expect("Main's turn");
    f.rig.finished(&turn.id);
    let mut spec = j4n_core::task::TaskSpec::new(&format!("check what {} changed", turn.id), "check");
    spec.thread = turn.spec.thread.clone();
    let check = f.rig.tasks.spawn(spec, j4n_core::change::Actor::Core).expect("the check starts");
    f.rig.finished(&check);
    f.front.plan_step("main");
    let started = events(&f, "second.started");
    assert_eq!(started.len(), 1, "{}", f.rig.dump());
    assert_eq!(started[0]["of"], json!(turn.id));
}

#[test]
fn a_review_started_after_a_check_of_main_s_turn_hangs_from_that_turn_and_its_concerns_go_back() {
    // a review queued from the thread's newest task, the check of Main's turn, must still have its end matched to the turn
    let f = fx(vec![main_doer(&[("built it", true), ("fixed it", true)]), reviewer(&[review(&["chess/game.py:40 q after p does not quit"])]), checker(2)], orchestrating);
    f.front.say("main", LINE);
    let turn = f.tasks().into_iter().find(|t| t.spec.doer == "claude").expect("Main's turn");
    f.rig.finished(&turn.id);
    let mut spec = j4n_core::task::TaskSpec::new(&format!("check what {} changed", turn.id), "check");
    spec.thread = turn.spec.thread.clone();
    let check = f.rig.tasks.spawn(spec, j4n_core::change::Actor::Core).expect("the check starts");
    f.rig.finished(&check);
    steps(&f, 4);
    let review = f.tasks().into_iter().find(|t| t.spec.role.as_deref() == Some("review")).expect("the review");
    assert_eq!(review.spec.parent.as_deref(), Some(turn.id.as_str()), "{}", f.rig.dump());
    let ended = events(&f, "second.ended");
    assert_eq!(ended.first().map(|e| e["outcome"].clone()), Some(json!("sent-back")), "{}", f.rig.dump());
    let back = f.tasks().into_iter().find(|t| t.spec.goal.starts_with("A review of your last change")).expect("Main's turn on the concerns");
    assert!(back.spec.goal.contains("chess/game.py:40"), "{}", back.spec.goal);
}

#[test]
fn a_turn_of_main_that_changed_nothing_while_the_review_ran_does_not_hide_the_review_s_end() {
    // a decision is asked of Main while the review of its join runs; Main answers in words, and the review's concerns must still be read
    let f = fx(vec![main_doer(&[("built it", true), ("both chats stay blocked", false), ("fixed it", true)]), reviewer(&[review(&["game.py:2 the keys are dead"])])], orchestrating);
    f.front.say("main", LINE);
    let turn = f.tasks().into_iter().find(|t| t.spec.doer == "claude").expect("Main's turn");
    f.rig.finished(&turn.id);
    f.front.plan_step("main");
    let review = reviews(&f).pop().expect("the review started");
    let mut spec = j4n_core::task::TaskSpec::new("J4n: 1 of your chats need a decision", "claude");
    (spec.thread, spec.role) = (turn.spec.thread.clone(), Some("work".into()));
    let answered = f.rig.tasks.spawn(spec, j4n_core::change::Actor::Core).expect("Main's answer starts");
    f.rig.finished(&answered);
    f.rig.finished(&review.id);
    f.front.plan_step("main");
    let ended = events(&f, "second.ended");
    assert_eq!(ended.first().map(|e| e["outcome"].clone()), Some(json!("sent-back")), "{}", f.rig.dump());
    assert_eq!(ended[0]["of"], json!(turn.id));
}

#[test]
fn a_newer_turn_of_main_that_changed_files_does_not_hide_an_older_review_s_end() {
    // the review of Main's turn settles with a finding, the join of the split starts at once and changes files, and the review's end must still be acted on
    let f = fx(vec![main_doer(&[("built it", true), ("joined the parts", true), ("fixed the keys", true), ("fixed more", true)]), reviewer(&[review(&["game.py:2 the keys are dead"]), review(&[]), review(&[])])], orchestrating);
    f.front.say("main", LINE);
    let turn = f.tasks().into_iter().find(|t| t.spec.doer == "claude").expect("Main's turn");
    f.rig.finished(&turn.id);
    f.front.plan_step("main");
    let first = reviews(&f).pop().expect("the review started");
    // the join begins as the review ends, before the step sees the review's end
    let mut spec = j4n_core::task::TaskSpec::new("Now join what is here.", "claude");
    (spec.thread, spec.role) = (turn.spec.thread.clone(), Some("work".into()));
    let join = f.rig.tasks.spawn(spec, j4n_core::change::Actor::Core).expect("the join starts");
    f.rig.finished(&first.id);
    f.front.plan_step("main");
    f.rig.finished(&join);
    let joined = f.tasks().into_iter().find(|t| t.id == join).expect("the join");
    assert!(!joined.changed.modified.is_empty() || !joined.changed.created.is_empty(), "the join changed files: {:?}", joined.changed);
    steps(&f, 6);
    // the newer turn is still reviewed, and the older review's concerns reach Main once, as of an earlier change
    let started = events(&f, "second.started");
    assert!(started.iter().any(|e| e["of"] == json!(join)), "{}", f.rig.dump());
    let ended = events(&f, "second.ended");
    let of_first: Vec<&Value> = ended.iter().filter(|e| e["review"] == json!(first.id)).collect();
    assert_eq!(of_first.len(), 1, "{}", f.rig.dump());
    assert_eq!((of_first[0]["outcome"].clone(), of_first[0]["of"].clone()), (json!("sent-back"), json!(turn.id)));
    let back = f.tasks().into_iter().find(|t| t.spec.goal.starts_with("A review of an earlier change of yours")).expect("Main's turn on the older concerns");
    assert!(back.spec.goal.contains("- game.py:2 the keys are dead") && back.spec.goal.contains("you have changed more since"), "{}", back.spec.goal);
}

#[test]
fn a_review_stopped_at_its_limit_after_it_named_concerns_sends_them_back_and_one_that_named_none_sends_nothing() {
    // the review reads all ten files and names ten concerns, then its five-minute wall stops it: the concerns must still go back
    let named = review(&["chess/board.py:19 a small terminal raises a raw ValueError"]);
    let stopped = Scripted::with_caps("pi", caps(), vec![vec![Beat::Session("pi-s".into()), Beat::Say(named.clone()), Beat::Fail("the time it was given ran out".into())]]);
    let f = rig(main_doer(&[("built it", true), ("fixed it", true)]), stopped, |_| {});
    f.front.say("main", LINE);
    steps(&f, 4);
    let ended = events(&f, "second.ended");
    assert_eq!(ended.first().map(|e| e["outcome"].clone()), Some(json!("sent-back")), "{}", f.rig.dump());
    let back = f.tasks().into_iter().find(|t| t.spec.goal.starts_with("A review of your last change")).expect("Main's turn on the concerns");
    assert!(back.spec.goal.contains("chess/board.py:19"), "{}", back.spec.goal);
    assert!(f.notes().iter().any(|n| n.contains("after it named 1 concern; they stand")), "{:?}", f.notes());
}

#[test]
fn a_review_that_answers_in_words_and_not_in_the_report_s_shape_sends_its_words_back_and_every_review_is_told_the_shape() {
    // the review of the join names a crash under a `## open` heading in prose; it must not be read as no concern
    let prose = "Read all files. The assembled product does not run clean.\n\n## open\n\n- items.py:97 `store.add_item` does not exist on store.Store: adding a timed item raises AttributeError".to_string();
    let f = rig(main_doer(&[("built it", true), ("fixed it", true)]), reviewer(&[prose, review(&[])]), |_| {});
    f.front.say("main", LINE);
    steps(&f, 6);
    let r = reviews(&f);
    assert!(r[0].spec.goal.contains("answer with one JSON object") && r[0].spec.goal.contains("\"open\""), "{}", r[0].spec.goal);
    let ended = events(&f, "second.ended");
    assert_eq!(ended.first().map(|e| e["outcome"].clone()), Some(json!("sent-back")), "{}", f.rig.dump());
    let back = f.tasks().into_iter().find(|t| t.spec.goal.starts_with("A review of your last change")).expect("Main's turn on the review's words");
    assert!(back.spec.goal.contains("items.py:97 `store.add_item` does not exist"), "{}", back.spec.goal);
}

#[test]
fn a_review_is_given_more_time_for_each_file_it_reads_and_is_told_it() {
    // the review of a whole program stopped at the same five minutes as the review of one file, with nothing said
    let mut wrote = vec![Beat::Session("claude-s".into())];
    wrote.extend(["game.py", "board.py", "pieces.py", "app.py"].map(|p| Beat::Put(p.into(), "x = 1\n".into())));
    wrote.push(end("built it"));
    let f = rig(Scripted::with_caps("claude", caps(), vec![wrote]), reviewer(&[review(&[])]), |_| {});
    f.front.say("main", LINE);
    steps(&f, 3);
    let r = reviews(&f);
    assert_eq!(r.len(), 1, "{}", f.rig.dump());
    assert_eq!(r[0].spec.limits.wall_ms, Some(8 * 60_000), "five minutes and a minute for each file past the first");
    assert!(r[0].spec.goal.contains("You have 8m00s"), "{}", r[0].spec.goal);
}

#[test]
fn neither_main_s_own_turn_nor_its_review_is_told_it_may_tell_a_chat() {
    // the tool `tell` is offered per doer, but only a top-level chat's turn may send with it: Main's own thread is no chat, and a review reports
    use j4n_core::front::messages::{TELL, TELL_CLI};
    let f = rig(main_doer(&[("built it", true)]), reviewer(&[review(&[])]), |c| {
        c.chats.screen = true;
        c.chats.tells = vec!["claude".into(), "pi".into()];
    });
    f.front.say("main", LINE);
    steps(&f, 3);
    let all = f.tasks();
    assert!(all.iter().any(|t| t.spec.role.as_deref() == Some("review")), "{}", f.rig.dump());
    for t in &all {
        let said = format!("{} {}", t.spec.goal, t.spec.input);
        assert!(!said.contains(TELL) && !said.contains(TELL_CLI), "{}: {said}", t.id);
    }
}
