//! The cheap branches of the lanes (scouts and pieces) may run on a doer and model of their own (`plan.cheap_doer`,
//! `plan.cheap_model`), as the planner and the reviewer may. A review, the planning turn and the checker (which runs the item's
//! check command and uses no model) are not touched by it. Unset: today's election and model.

use crate::branch_validate::{A, B, CLEAN, Rig3, checker, doer, item, rig_cfg, wide};
use j4n_core::front::{Cfg, Pin};

const WORK: &str = "sonnet-x";
const CHEAP: &str = "haiku-x";

fn unset(c: &mut Cfg) {
    wide(c);
    c.model = Some(WORK.into());
}

fn pinned(c: &mut Cfg) {
    unset(c);
    c.cheap = Pin { doer: Some("pi".into()), model: Some(CHEAP.into()) };
}

fn items() -> Vec<serde_json::Value> {
    vec![item("s", "scout", &B, &[], &[]), item("p", "piece", &A, &["src/a/1.rs"], &[]), item("end", "inline", &[], &["src/e.rs"], &["s", "p"])]
}

fn run(cfg: fn(&mut Cfg)) -> Rig3 {
    let r = rig_cfg(items(), doer("pi", &["read it", "did p"]), doer("own", &[CLEAN]), checker(2), &["end done"], cfg);
    r.begin();
    r.until("the piece was reviewed", |r| r.f.tasks().iter().any(|t| t.spec.role.as_deref() == Some("review")));
    r
}

fn of(r: &Rig3, role: &str) -> Vec<(String, Option<String>)> {
    r.f.tasks().into_iter().filter(|t| t.spec.role.as_deref() == Some(role)).map(|t| (t.spec.doer, t.spec.tune.model)).collect()
}

#[test]
fn with_the_cheap_pair_set_a_scout_and_a_piece_run_on_it_and_a_review_the_planning_turn_and_the_checker_do_not() {
    let r = run(pinned);
    let cheap = (String::from("pi"), Some(CHEAP.to_string()));
    assert_eq!(of(&r, "scout"), std::slice::from_ref(&cheap), "{}", r.f.rig.dump());
    assert_eq!(of(&r, "piece"), std::slice::from_ref(&cheap), "{}", r.f.rig.dump());
    assert!(!of(&r, "review").is_empty() && of(&r, "review").iter().all(|t| t.1.as_deref() != Some(CHEAP)), "a review is not cheap work: {:?}", of(&r, "review"));
    assert_eq!(of(&r, "plan"), [("claude".to_string(), Some(WORK.to_string()))], "the planning turn keeps the thread's");
    assert!(!of(&r, "check").is_empty() && of(&r, "check").iter().all(|t| t.1.as_deref() != Some(CHEAP)), "the checker uses no model: {:?}", of(&r, "check"));
    let queued: Vec<_> = r.f.rig.of_kind("task.queued").into_iter().filter(|e| e.data["spec"]["tune"]["model"] == CHEAP).collect();
    assert_eq!(queued.len(), 2, "task.queued names the cheap model for the scout and the piece only");
}

#[test]
fn unset_a_scout_and_a_piece_keep_the_elected_doer_and_the_threads_model() {
    let r = run(unset);
    for role in ["scout", "piece"] {
        assert!(!of(&r, role).is_empty());
        assert!(of(&r, role).iter().all(|t| t.0 == "pi" && t.1.as_deref() != Some(CHEAP)), "{role}: {:?}", of(&r, role));
    }
    assert!(r.f.rig.of_kind("task.queued").iter().all(|e| e.data["spec"]["tune"]["model"] != CHEAP));
}

#[test]
fn a_cheap_doer_that_j4n_does_not_have_refuses_the_branch_instead_of_running_it_elsewhere() {
    fn bad(c: &mut Cfg) {
        unset(c);
        c.cheap.doer = Some("nobody".into());
    }
    let r = rig_cfg(items(), doer("pi", &["read it", "did p"]), doer("own", &[CLEAN]), checker(2), &["end done"], bad);
    r.begin();
    assert!(of(&r, "scout").is_empty() && of(&r, "piece").is_empty(), "{}", r.f.rig.dump());
}
