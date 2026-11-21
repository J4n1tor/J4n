//! J4n's own steps in Main under `orchestrate`: a line typed while only the check or the review of Main's change works is an order
//! to Main, which ends them; `skip` ends the step that runs and lets the next come, `stop` ends it and the steps to come; ended work is marked unchecked,
//! never passed; a line typed while Main's own turn works still steers it.

use crate::branch_validate::caps;
use crate::front_cases::{Fx, fx};
use crate::scripted::{Beat, Scripted, end};
use j4n_core::branch::Switch;
use j4n_core::front::Said;
use j4n_core::front::serve::Serve;
use j4n_core::task::{Row, State};
use j4n_proto::{COMPOSER_STRIP, Down, Up};
use serde_json::{Value, json};
use std::sync::Arc;

const LINE: &str = "add a name row to the configs, off by default";

/// Main's turns: the first writes `feed.py`, the others write nothing; a turn of `None` holds until it is steered or stopped.
fn main_doer(turns: &[Option<&str>]) -> Arc<Scripted> {
    let script = turns
        .iter()
        .enumerate()
        .map(|(n, said)| {
            let mut t = vec![Beat::Session("claude-s".into())];
            if n == 0 {
                t.push(Beat::Put("feed.py".into(), "# the name row\n".into()));
            }
            match said {
                Some(s) => t.push(end(s)),
                None => t.push(Beat::Hold),
            }
            t
        })
        .collect();
    Scripted::with_caps("claude", caps(), script)
}

/// A doer that works until it is stopped: the slow check, or the review that runs to its wall.
fn slow(id: &str) -> Arc<Scripted> {
    Scripted::with_caps(id, if id == "check" { j4n_core::doer::Caps::default() } else { caps() }, vec![vec![Beat::Forever]; 2])
}

fn rig(main: Arc<Scripted>, others: Vec<Arc<Scripted>>) -> Fx {
    let mut doers = vec![main];
    doers.extend(others);
    fx(doers, |c| {
        c.doer = Some("claude".into());
        c.lanes = Switch::Orchestrate;
        c.chats.screen = true;
    })
}

fn events(f: &Fx, kind: &str) -> Vec<Value> {
    f.rig.of_kind(kind).into_iter().map(|e| e.data).collect()
}

/// Main's first turn, ended: the change the steps are about.
fn turn(f: &Fx) -> Row {
    f.front.say("main", LINE);
    let turn = f.tasks().into_iter().find(|t| t.spec.doer == "claude").expect("Main's turn");
    f.rig.finished(&turn.id)
}

/// The project's check of `turn`, as the rule `lane-check` starts it, working.
fn check(f: &Fx, turn: &Row) -> String {
    let mut spec = j4n_core::task::TaskSpec::new(&format!("check what {} changed", turn.id), "check");
    spec.thread = turn.spec.thread.clone();
    f.rig.tasks.spawn(spec, j4n_core::change::Actor::Core).expect("the check starts")
}

fn reviews(f: &Fx) -> Vec<Row> {
    f.tasks().into_iter().filter(|r| r.spec.role.as_deref() == Some("review")).collect()
}

#[test]
fn a_line_typed_while_the_check_runs_ends_it_unchecked_goes_to_main_and_no_review_runs_behind_it() {
    // `deploy it` typed while the check of Main's change runs must not be taken as a steer of the check, which takes none, and lost; typed
    // again, Main deploys while the review of the change ran on to its wall
    let f = rig(main_doer(&[Some("built it"), Some("deployed")]), vec![slow("check"), slow("pi")]);
    let t = turn(&f);
    let c = check(&f, &t);
    let said = f.front.say("main", "deploy it");
    assert!(matches!(said, Said::Started { .. }), "the line went to Main: {said:?}\n{}", f.rig.dump());
    let row = f.rig.finished(&c);
    assert!(!matches!(row.state, State::Done { ok: true }), "an ended check never reads as passed: {row:?}");
    let skipped = events(&f, "check.skipped");
    assert_eq!(skipped.len(), 1, "{}", f.rig.dump());
    assert_eq!((skipped[0]["task"].clone(), skipped[0]["of"].clone(), skipped[0]["by"].clone(), skipped[0]["unchecked"].clone(), skipped[0]["line"].clone()), (json!(c), json!(t.id), json!("line"), json!(true), json!("deploy it")));
    let deploy = f.tasks().into_iter().filter(|r| r.spec.doer == "claude").nth(1).expect("Main's turn on the line");
    assert!(deploy.spec.goal.contains("deploy it"), "{}", deploy.spec.goal);
    assert!(f.notes().iter().any(|n| n == &format!("ended check {c}: {}'s change is unchecked · the line goes to Main", t.id)), "{:?}", f.notes());
    assert!(!f.notes().iter().any(|n| n.contains("cannot be changed while it works")), "the line is no steer of the check: {:?}", f.notes());
    // the deploy ends, the steps run: the change the operator moved past is not reviewed behind it
    f.rig.finished(&deploy.id);
    for _ in 0..3 {
        f.front.plan_step("main");
    }
    assert!(reviews(&f).is_empty() && events(&f, "second.started").is_empty(), "{}", f.rig.dump());
    assert_eq!(events(&f, "second.ended").first().map(|e| e["outcome"].clone()), Some(json!("skipped")), "{}", f.rig.dump());
}

#[test]
fn a_line_typed_while_the_review_runs_ends_it_and_nothing_of_it_goes_back_to_main() {
    let f = rig(main_doer(&[Some("built it"), Some("deployed")]), vec![slow("pi")]);
    let t = turn(&f);
    f.front.plan_step("main");
    let review = reviews(&f).pop().expect("the review runs");
    assert_eq!(review.spec.parent.as_deref(), Some(t.id.as_str()));
    let said = f.front.say("main", "deploy it");
    assert!(matches!(said, Said::Started { .. }), "{said:?}\n{}", f.rig.dump());
    f.rig.finished(&review.id);
    let skipped = events(&f, "review.skipped");
    assert_eq!((skipped.len(), skipped[0]["by"].clone()), (1, json!("line")), "{}", f.rig.dump());
    let deploy = f.tasks().into_iter().filter(|r| r.spec.doer == "claude").nth(1).expect("Main's turn on the line");
    f.rig.finished(&deploy.id);
    f.front.plan_step("main");
    let ended = events(&f, "second.ended");
    assert_eq!(ended.iter().map(|e| e["outcome"].as_str().unwrap_or_default()).collect::<Vec<_>>(), ["skipped"], "{}", f.rig.dump());
    assert!(!f.notes().iter().any(|n| n.contains("did not finish")), "an ended review is no failed one: {:?}", f.notes());
    assert_eq!(f.tasks().iter().filter(|r| r.spec.doer == "claude").count(), 2, "nothing went back to Main");
}

#[test]
fn skip_typed_in_full_ends_the_check_and_the_review_still_comes_and_stop_ends_both() {
    // skip: this step only
    let f = rig(main_doer(&[Some("built it")]), vec![slow("check"), slow("pi")]);
    let t = turn(&f);
    let c = check(&f, &t);
    assert_eq!(f.front.say("main", "skip"), Said::Command("skip".into()), "{}", f.rig.dump());
    f.rig.finished(&c);
    assert_eq!(events(&f, "check.skipped")[0]["follows"], json!(true));
    f.front.plan_step("main");
    let review = reviews(&f).pop().expect("the review comes after a skipped check");
    // and `skip` again ends the review: the review is the step that runs now
    assert_eq!(f.front.say("main", "Skip "), Said::Command("skip".into()));
    f.rig.finished(&review.id);
    f.front.plan_step("main");
    assert_eq!(events(&f, "second.ended").last().map(|e| e["outcome"].clone()), Some(json!("skipped")), "{}", f.rig.dump());
    assert_eq!(f.tasks().iter().filter(|r| r.spec.doer == "claude").count(), 1, "skip sends nothing to Main");
    // stop: this step and the ones to come; `:stop` is the same
    for word in ["stop", ":stop"] {
        let f = rig(main_doer(&[Some("built it")]), vec![slow("check"), slow("pi")]);
        let t = turn(&f);
        let c = check(&f, &t);
        assert_eq!(f.front.say("main", word), Said::Command("stop".into()), "{word}: {}", f.rig.dump());
        f.rig.finished(&c);
        f.front.plan_step("main");
        assert!(reviews(&f).is_empty(), "{word}: no review after stop: {}", f.rig.dump());
        assert!(f.notes().iter().any(|n| n.ends_with("· no step follows")), "{word}: {:?}", f.notes());
    }
}

#[test]
fn a_line_typed_while_main_s_own_turn_works_still_steers_it_and_skip_is_a_plain_line_with_no_step() {
    let f = rig(main_doer(&[None]), vec![slow("check")]);
    f.front.say("main", LINE);
    let turn = f.tasks().into_iter().find(|t| t.spec.doer == "claude").expect("Main's turn");
    f.rig.until(&turn.id, "running", |r| r.state == State::Running);
    let said = f.front.say("main", "also keep the old row");
    assert_eq!(said, Said::Steered { task: turn.id.clone() }, "{}", f.rig.dump());
    assert!(f.rig.of_kind("check.skipped").is_empty() && f.rig.of_kind("review.skipped").is_empty());
}

#[test]
fn the_composer_strip_carries_the_words_while_a_step_can_be_ended_and_not_after() {
    let f = rig(main_doer(&[Some("built it")]), vec![slow("check")]);
    let mut s = Serve::new(f.front.clone());
    let strip = |s: &mut Serve| {
        let (downs, _) = s.up(Up::Watch { bind: COMPOSER_STRIP.into() });
        downs.iter().rev().find_map(|d| match d {
            Down::Value { bind, value, .. } if bind == COMPOSER_STRIP => Some(value.clone()),
            _ => None,
        })
    };
    let t = turn(&f);
    assert!(strip(&mut s).is_none_or(|v| v.get("skip").is_none()), "no step runs");
    let c = check(&f, &t);
    let v = strip(&mut s).expect("a strip");
    assert_eq!(v["skip"], json!({ "words": ["skip", "stop"], "what": format!("check {c}") }), "{v}");
    f.front.say("main", "skip");
    f.rig.finished(&c);
    assert!(strip(&mut s).is_none_or(|v| v.get("skip").is_none()), "nothing to end");
}
