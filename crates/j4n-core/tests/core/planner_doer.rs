//! The planning turn and its replans, and a review branch, may run on a doer and model of their own (`plan.doer`,
//! `plan.model`, `plan.review_doer`, `plan.review_model`) while the items' work keeps the thread's binding. Empty: the thread's own,
//! as before. The tier policy is Opus plans, Sonnet works; here `sonnet-x` is the thread's and `opus-x` the planner's.

use crate::branch_validate::{A, Rig3, checker, doer, item, planned, rig_cfg, wide};
use j4n_core::front::{Cfg, Pin};
use serde_json::{Value, json};

const WORK: &str = "sonnet-x";
const PLANNER: &str = "opus-x";
const WEAK: &str = r#"{"answer":"looked","sure":"low","open":["is the retry loop in a.rs?"]}"#;

fn unset(c: &mut Cfg) {
    wide(c);
    c.model = Some(WORK.into());
}

fn tiered(c: &mut Cfg) {
    unset(c);
    c.planner = Pin { doer: None, model: Some(PLANNER.into()) };
    c.reviewer = Pin { doer: None, model: Some(PLANNER.into()) };
}

fn inline() -> Vec<Value> {
    vec![item("m", "inline", &[], &["src/m.rs"], &[]), item("n", "inline", &[], &["src/n.rs"], &["m"])]
}

fn models(r: &Rig3, role: Option<&str>) -> Vec<Option<String>> {
    r.f.tasks().iter().filter(|t| t.spec.role.as_deref() == role).map(|t| t.spec.tune.model.clone()).collect()
}

fn work_of(r: &Rig3, id: &str) -> Option<String> {
    let task = r.item(id).task;
    r.f.tasks().into_iter().find(|t| t.id == task).unwrap().spec.tune.model
}

#[test]
fn with_plan_model_set_the_planning_turn_runs_on_it_and_the_items_keep_the_threads_model() {
    let r = rig_cfg(inline(), doer("pi", &[]), doer("own", &[]), checker(1), &["m done"], tiered);
    r.begin();
    assert_eq!(models(&r, Some("plan")), [Some(PLANNER.to_string())], "the planning turn is the planner's");
    assert_eq!(r.item("m").state, "running");
    assert_eq!(work_of(&r, "m").as_deref(), Some(WORK), "the item's work is the thread's");
    assert_eq!(r.f.tasks()[0].spec.tune.model.as_deref(), Some(WORK), "and so is the line before the plan");
}

#[test]
fn unset_the_planning_turn_and_the_item_both_carry_the_threads_model() {
    let r = rig_cfg(inline(), doer("pi", &[]), doer("own", &[]), checker(1), &["m done"], unset);
    r.begin();
    assert_eq!(models(&r, Some("plan")), [Some(WORK.to_string())]);
    assert_eq!(work_of(&r, "m").as_deref(), Some(WORK));
}

#[test]
fn a_replan_runs_on_the_planners_model_too() {
    let scouted = || vec![item("s", "scout", &A, &[], &[]), item("m", "inline", &[], &["src/m.rs"], &["s"]), item("n", "inline", &[], &["src/n.rs"], &["m"])];
    let mut more = scouted();
    more.push(item("x", "inline", &[], &["src/x.rs"], &["s"]));
    let mut again: Value = serde_json::from_str(&planned(more)).unwrap();
    again["why"] = json!("the retry loop is unclear, look at it again");
    let r = rig_cfg(scouted(), doer("pi", &[WEAK]), doer("own", &[]), checker(1), &[&again.to_string(), "m done", "n done", "x done"], tiered);
    r.begin();
    r.until("the approach was changed", |r| r.f.keeper.plan("main").unwrap().revs == 1);
    assert_eq!(models(&r, Some("plan")), [Some(PLANNER.to_string()), Some(PLANNER.to_string())], "the plan and its replan");
    assert!(r.f.rig.of_kind("plan.asked").iter().any(|e| e.data["how"] == "replan"));
    r.until("the plan was done", |r| r.item("x").state == "merged");
    assert!(r.f.tasks().iter().filter(|t| t.spec.role.as_deref() == Some("work")).all(|t| t.spec.tune.model.as_deref() == Some(WORK)), "the items stay on the thread's model");
}

#[test]
fn a_review_runs_on_the_reviewers_model_when_it_is_pinned_and_on_the_threads_when_not() {
    let items = || vec![item("m", "inline", &[], &["src/m.rs"], &[]), item("rv", "review", &A, &[], &["m"])];
    for (cfg, want) in [(tiered as fn(&mut Cfg), PLANNER), (unset, WORK)] {
        let r = rig_cfg(items(), doer("pi", &[]), doer("own", &[]), checker(2), &["m done", "fine"], cfg);
        r.begin();
        r.until("the review began", |r| r.f.tasks().iter().any(|t| t.spec.role.as_deref() == Some("review")));
        let seen = models(&r, Some("review"));
        assert_eq!(seen.len(), 1, "{}", r.f.rig.dump());
        if want == PLANNER {
            assert_eq!(seen, [Some(PLANNER.to_string())], "pinned");
        } else {
            assert_ne!(seen, [Some(PLANNER.to_string())], "not pinned");
        }
    }
}

#[test]
fn a_planner_word_that_matches_no_model_refuses_the_planning_turn_and_a_planner_doer_not_here_does_too() {
    fn bad_doer(c: &mut Cfg) {
        unset(c);
        c.planner.doer = Some("nobody".into());
    }
    let r = rig_cfg(inline(), doer("pi", &[]), doer("own", &[]), checker(1), &["m done"], bad_doer);
    r.f.turn("hello");
    let said = r.f.say(":plan do these: 1. look 2. look again 3. look more");
    assert!(matches!(&said, j4n_core::front::Said::Refused(why) if why.contains("`nobody` is not one J4n has")), "{said:?}");
    assert!(models(&r, Some("plan")).is_empty());
}
