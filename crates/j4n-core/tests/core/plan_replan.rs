//! Through the real front and scripted doers: a trigger starts a replan, a
//! planning turn of the main lane told the reason and the evidence by Log id; the answer changes the items not yet started and says why;
//! done work is kept (dropping it is the operator's), a replan that adds more than `plan.grow` items is the operator's, the fourth
//! revision asks and stops, and the branches of a dropped item are stopped with their reports kept. The triggers are checked as data too.

use crate::branch_validate::{A, B, CLEAN, Rig3, caps, checker, doer, item, planned, rig};
use crate::scripted::{Beat, Scripted, end};
use j4n_core::change::Actor;
use j4n_core::keeper::{Item, Plan};
use j4n_core::log::New;
use j4n_core::replan::{RULES, triggers};
use serde_json::{Value, json};

const WEAK: &str = r#"{"answer":"looked","sure":"low","open":["is the retry loop in a.rs?"]}"#;

fn replanned(why: &str, items: Vec<Value>) -> String {
    let mut v: Value = serde_json::from_str(&planned(items)).unwrap();
    v["why"] = json!(why);
    v.to_string()
}

fn scouted() -> Vec<Value> {
    vec![item("s", "scout", &A, &[], &[]), item("m", "inline", &[], &["src/m.rs"], &["s"]), item("n", "inline", &[], &["src/n.rs"], &["m"])]
}

/// A plan whose scout `s` reported unsure: the main lane writes the plan, then each `replans` answer in turn.
fn weak_rig(replans: &[String]) -> Rig3 {
    let tail: Vec<&str> = replans.iter().map(String::as_str).chain(["m done", "n done", "x done", "y done", "z done", "w done"]).collect();
    rig(scouted(), doer("pi", &[WEAK]), doer("own", &[]), checker(1), &tail)
}

fn revs(r: &Rig3) -> u32 {
    r.f.keeper.plan("main").unwrap().revs
}

fn plan_turns(r: &Rig3) -> usize {
    r.f.tasks().iter().filter(|t| t.spec.role.as_deref() == Some("plan")).count()
}

fn plan_of(r: &Rig3) -> Plan {
    r.f.keeper.plan("main").unwrap()
}

// ---- the triggers ----

#[test]
fn each_trigger_is_a_row_of_data_and_a_reported_trigger_is_answered_once() {
    let mut plan = Plan { items: vec![Item { id: "i".into(), state: "todo".into(), ..Item::default() }], ..Plan::default() };
    let found = |p: &Plan| triggers(p).into_iter().map(|t| t.rule).collect::<Vec<_>>();
    assert!(found(&plan).is_empty());
    // (kind of `went`, the count that is not enough, the rule it makes)
    for (went, short, rule) in [("check", 1, "check-twice"), ("blocked", 0, "blocked"), ("weak", 0, "unsure"), ("unchecked", 1, "unchecked-twice"), ("repeat", 0, "concern-repeats"), ("stuck", 0, "stuck")] {
        let mut p = plan.clone();
        p.items[0].state = if went == "blocked" { "blocked".into() } else { "todo".into() };
        p.items[0].went.insert(went.into(), short);
        assert!(found(&p).is_empty(), "{went} x{short} is not enough");
        p.items[0].went.insert(went.into(), short + 1);
        assert_eq!(found(&p), [rule], "{went}");
        let key = triggers(&p)[0].key.clone();
        p.handled.insert(key);
        assert!(found(&p).is_empty(), "a trigger a replan was started for does not start another");
        p.items.push(Item { id: "done".into(), state: "merged".into(), ..Item::default() });
        p.items[0].state = "merged".into();
        assert!(found(&p).is_empty() || p.open(), "a plan with nothing left to do has no trigger");
    }
    assert_eq!(RULES.len(), 6, "the six triggers of §5");
    plan.items[0].went.insert("blocked".into(), 1);
    assert!(found(&plan).is_empty(), "`blocked` holds only while the item is");
}

#[test]
fn a_report_that_is_unsure_starts_a_replan_told_the_old_plan_the_reason_and_the_evidence_by_log_id() {
    let again = replanned("the retry loop is unclear, so look at it first", { let mut v = scouted(); v.push(item("x", "inline", &[], &["src/x.rs"], &["s"])); v });
    let r = weak_rig(&[again]);
    r.begin();
    r.until("the approach was changed", |r| revs(r) == 1);
    let asked = r.f.rig.of_kind("plan.asked").into_iter().find(|e| e.data["how"] == "replan").expect("a replan was asked");
    let turn = r.f.tasks().into_iter().find(|t| t.id == asked.data["task"].as_str().unwrap()).unwrap();
    let goal = &turn.spec.goal;
    assert!(goal.contains("The approach is not working") && goal.contains("do m") && goal.contains("approach: Look, change, check."), "the old plan: {goal}");
    assert!(goal.contains("s: its report is unsure or leaves a point open (unsure)"), "the reason: {goal}");
    assert!(goal.contains("Evidence, by Log id") && goal.contains(" weak: ") && goal.contains("is the retry loop in a.rs?"), "the evidence: {goal}");
    let id = goal.split("\ne").nth(1).and_then(|t| t.split_whitespace().next()).and_then(|n| n.parse::<u64>().ok()).expect("a Log id");
    assert!(r.f.rig.events().iter().any(|e| e.seq == id && e.kind == "plan.item"), "e{id} is an event of the Log");
    assert_eq!((turn.spec.tune.mode, asked.data["evidence"].as_array().is_some_and(|a| !a.is_empty())), (j4n_core::task::Mode::Read, true));
    let approach = r.f.rig.of_kind("plan.approach").into_iter().find(|e| e.data["rev"] == 1).expect("plan.approach with a rev");
    assert_eq!(approach.data["why"], "the retry loop is unclear, so look at it first");
    assert!(approach.data["evidence"].as_array().unwrap().iter().any(|e| e.as_str().unwrap().contains(" weak: ")));
    assert!(r.f.notes().iter().any(|n| n == "changed approach: the retry loop is unclear, so look at it first"), "{:?}", r.f.notes());
    let p = plan_of(&r);
    assert_eq!((p.revs, p.why.as_str(), p.items.len()), (1, "the retry loop is unclear, so look at it first", 4), "the added item is in the plan");
    assert!(p.pending.is_none() && r.f.tasks().iter().all(|t| t.spec.doer != "human"), "nothing was asked: one item added, nothing dropped");
    r.step();
    let asks: Vec<String> = r.f.rig.of_kind("plan.asked").iter().map(|e| format!("{} {}", e.data["how"], e.data["line"])).collect();
    assert_eq!(plan_turns(&r), 2, "the plan, and one replan: the same report does not start another: {asks:?}");
}

#[test]
fn a_blocked_item_is_a_trigger_and_the_replan_gives_it_another_go() {
    let items = vec![item("p", "piece", &A, &["src/a/1.rs"], &[]), item("x", "inline", &[], &["src/x.rs"], &["p"]), item("y", "inline", &[], &["src/y.rs"], &["x"])];
    let again = replanned("p never lays beside the others; try it once more", items.clone());
    let r = rig(items, doer("pi", &["did"; 8]), doer("own", &[CLEAN; 8]), checker(16), &[&again, "x done", "y done"]);
    *r.held.refuse_all.lock().unwrap() = Some("src/a/1.rs changed in the folder".into());
    r.begin();
    r.until("p was blocked", |r| r.item("p").state == "blocked");
    let asked = r.f.rig.of_kind("plan.asked").into_iter().find(|e| e.data["how"] == "replan").expect("the block starts a replan");
    assert!(asked.data["line"].as_str().unwrap().contains("p: it is blocked (blocked)"), "{asked:?}");
    assert!(r.f.rig.of_kind("plan.reported").is_empty(), "the replan speaks before the plan reports a block");
    *r.held.refuse_all.lock().unwrap() = None;
    r.until("the plan was done", |r| r.item("y").state == "merged");
    assert_eq!(revs(&r), 1);
    let p = r.item("p");
    assert_eq!((p.state.as_str(), p.sends, p.tries), ("merged", 0, 1), "its counts began again: {}", r.f.rig.dump());
    assert_eq!(r.f.rig.of_kind("plan.reported").iter().map(|e| e.data["outcome"].as_str().unwrap().to_string()).collect::<Vec<_>>(), ["done"]);
}

#[test]
fn the_same_check_failing_twice_is_a_trigger() {
    let items = vec![item("p", "piece", &A, &["src/a/1.rs"], &[]), item("x", "inline", &[], &["src/x.rs"], &["p"]), item("y", "inline", &[], &["src/y.rs"], &["x"])];
    let again = replanned("the checks keep failing on p; keep it and look again", items.clone());
    let check = Scripted::new("check", vec![vec![Beat::Fail("tests broke".into())], vec![Beat::Fail("tests broke".into())], vec![end("green")], vec![end("green")], vec![end("green")], vec![end("green")]]);
    let r = rig(items, doer("pi", &["did"; 6]), doer("own", &[CLEAN; 6]), check, &[&again, "x done", "y done"]);
    r.begin();
    r.until("the approach was changed", |r| revs(r) == 1);
    let asked = r.f.rig.of_kind("plan.asked").into_iter().find(|e| e.data["how"] == "replan").unwrap();
    assert!(asked.data["line"].as_str().unwrap().contains("p: the same check failed twice (check-twice)"), "{asked:?}");
    let seen = asked.data["evidence"].as_array().unwrap();
    assert_eq!(seen.iter().filter(|e| e.as_str().unwrap().contains(" check: ")).count(), 2, "both failures are in the evidence: {seen:?}");
    assert_eq!(r.item("p").sends, 2, "sent back twice, not blocked: the replan came before a third go");
    r.until("the plan was done", |r| r.item("y").state == "merged");
}

#[test]
fn the_main_lane_told_to_step_back_is_a_trigger() {
    let again = replanned("stepping back: m needs a look first", { let mut v = vec![item("m0", "scout", &A, &[], &[])]; v.extend([item("m", "inline", &[], &["src/m.rs"], &["m0"]), item("n", "inline", &[], &["src/n.rs"], &["m"])]); v });
    let r = rig(vec![item("m", "inline", &[], &["src/m.rs"], &[]), item("n", "inline", &[], &["src/n.rs"], &["m"]), item("o", "inline", &[], &["src/o.rs"], &["n"])], doer("pi", &["looked"]), doer("own", &[]), checker(1), &["m done", &again, "m again", "n done", "o done"]);
    r.begin();
    let m = r.item("m");
    assert_eq!(m.state, "running");
    let fired = New::of("rule.fired", Actor::Rule("stuck".into()), json!({ "rule": "stuck", "then": "steer" })).task(&m.task).thread("main");
    r.f.rig.log.append(fired).unwrap();
    assert_eq!(r.item("m").went.get("stuck"), Some(&1), "{:?}", r.item("m"));
    r.f.rig.finished(&m.task);
    r.f.front.plan_step("main");
    let asked = r.f.rig.of_kind("plan.asked").into_iter().find(|e| e.data["how"] == "replan").expect("a replan after the turn ended");
    assert!(asked.data["line"].as_str().unwrap().contains("the main lane was told it is stuck (stuck)"), "{asked:?}");
}

// ---- what goes ahead and what is the operator's ----

#[test]
fn a_replan_that_adds_at_most_grow_items_and_keeps_all_done_work_goes_ahead_without_asking() {
    let add = |n: usize| (0..n).map(|k| item(&format!("add{k}"), "inline", &[], &[&format!("src/add{k}.rs")], &["s"])).collect::<Vec<_>>();
    let r = weak_rig(&[replanned("three more steps are needed", scouted().into_iter().chain(add(3)).collect())]);
    r.begin();
    r.until("the approach was changed", |r| revs(r) == 1);
    let p = plan_of(&r);
    assert_eq!((p.items.len(), p.pending.is_none()), (6, true), "three added is the most that goes ahead");
    assert_eq!(r.item("s").state, "reviewed", "and the done work is kept");
    assert!(r.f.rig.of_kind("plan.pending").is_empty());
}

#[test]
fn a_replan_that_adds_more_than_grow_items_is_held_for_the_operator_who_may_take_it_or_not() {
    let add = |n: usize| (0..n).map(|k| item(&format!("add{k}"), "inline", &[], &[&format!("src/add{k}.rs")], &["s"])).collect::<Vec<_>>();
    let r = weak_rig(&[replanned("a bigger plan", scouted().into_iter().chain(add(4)).collect())]);
    r.begin();
    r.until("a replan was held", |r| plan_of(r).pending.is_some());
    let p = plan_of(&r);
    let pending = p.pending.clone().unwrap();
    assert_eq!((pending.kind.as_str(), p.items.len(), p.revs), ("grow", 3, 0), "the plan stands as it was");
    assert!(pending.why.contains("adds 4 items, more than plan.grow (3)"), "{}", pending.why);
    let started = r.f.tasks().len();
    r.step();
    r.step();
    assert_eq!(r.f.tasks().len(), started, "the plan waits for the answer: nothing starts");
    let said = r.f.rig.of_kind("plan.reported");
    assert_eq!(said.len(), 1, "{:?}", r.f.notes());
    assert!(said[0].data["decisions"][0].as_str().unwrap().starts_with("grow:"), "a big replan is a real decision: {:?}", said[0].data);
    assert!(matches!(r.f.say(":plan accept"), j4n_core::front::Said::Command(_)));
    let p = plan_of(&r);
    assert_eq!((p.items.len(), p.revs, p.pending.is_none()), (7, 1, true), "{:?}", r.f.notes());
    assert!(r.f.notes().iter().any(|n| n == "changed approach: a bigger plan"));
}

#[test]
fn a_replan_that_drops_merged_work_is_a_real_decision_and_rejecting_it_keeps_the_plan() {
    let without_s = vec![item("m", "inline", &[], &["src/m.rs"], &[]), item("n", "inline", &[], &["src/n.rs"], &["m"]), item("k", "inline", &[], &["src/k.rs"], &["n"])];
    let r = weak_rig(&[replanned("s is not needed any more", without_s)]);
    r.begin();
    r.until("a replan was held", |r| plan_of(r).pending.is_some());
    let p = plan_of(&r);
    assert_eq!(p.pending.as_ref().unwrap().kind, "drops-merged");
    assert!(p.items.iter().any(|i| i.id == "s" && i.done()), "the merged item is not dropped silently");
    assert!(r.f.notes().iter().any(|n| n.contains("drops merged work (s)")), "{:?}", r.f.notes());
    r.f.say(":plan reject");
    let p = plan_of(&r);
    assert_eq!((p.items.len(), p.revs, p.pending.is_none()), (3, 0, true));
    r.until("m was merged", |r| r.item("m").state == "merged");
    assert_eq!(plan_turns(&r), 2, "the old plan goes on, and the same report does not ask again");
}


#[test]
fn the_fourth_revision_asks_and_stops_and_does_not_loop() {
    let chain = vec![item("s1", "scout", &A, &[], &[]), item("s2", "scout", &B, &[], &["s1"]), item("s3", "scout", &A, &[], &["s2"]), item("s4", "scout", &B, &[], &["s3"]), item("end", "inline", &[], &["src/e.rs"], &["s4"])];
    let again: Vec<String> = ["one", "two", "three", "four"].iter().map(|w| replanned(&format!("revision {w}"), chain.clone())).collect();
    let tail: Vec<&str> = again.iter().map(String::as_str).collect();
    let r = rig(chain, doer("pi", &[WEAK; 6]), doer("own", &[]), checker(1), &tail);
    r.begin();
    r.until("the fourth was held", |r| plan_of(r).pending.is_some());
    let p = plan_of(&r);
    assert_eq!((p.revs, plan_turns(&r)), (3, 4), "the plan and three replans; the fourth is not started");
    let pending = p.pending.unwrap();
    assert_eq!(pending.kind, "revisions");
    assert!(pending.why.contains("changed 3 times (plan.revisions 3)") && pending.why.contains("yours to decide"), "{}", pending.why);
    for _ in 0..3 {
        r.step();
    }
    assert_eq!(plan_turns(&r), 4, "and it does not loop");
    assert_eq!(r.item("s4").state, "reviewed");
    let said = r.f.rig.of_kind("plan.reported");
    assert_eq!((said.len(), said[0].data["outcome"].as_str()), (1, Some("blocked")), "one report that asks");
    assert!(said[0].data["decisions"][0].as_str().unwrap().starts_with("revisions:"), "{:?}", said[0].data);
    r.f.say(":plan accept");
    assert_eq!(plan_turns(&r), 5, "the operator's yes is the fourth revision");
    r.until("the fourth revision was taken", |r| revs(r) == 4);
    assert_eq!(r.f.keeper.plan("main").unwrap().why, "revision four");
}

#[test]
fn the_bounds_are_the_settings_of_the_plan_and_are_three_by_default() {
    let c = j4n_core::front::Cfg::new("p");
    assert_eq!((c.revisions, c.grow, c.sends), (3, 3, 3));
}


#[test]
fn a_replan_never_undoes_merged_work_and_stops_the_running_branches_of_a_dropped_item_with_their_reports_kept() {
    let items = vec![item("a", "scout", &A, &[], &[]), item("b", "scout", &B, &[], &[]), item("c", "inline", &[], &["src/c.rs"], &["a", "b"]), item("d", "inline", &[], &["src/d.rs"], &["c"])];
    let without_b = replanned("b is not needed after all", vec![items[0].clone(), item("c", "inline", &[], &["src/c.rs"], &["a"]), items[3].clone()]);
    let pi = Scripted::with_caps("pi", caps(), vec![vec![Beat::Session("pi-s".into()), Beat::Say(WEAK.into()), end(WEAK)], vec![Beat::Session("pi-s2".into()), Beat::Forever]]);
    let r = rig(items, pi, doer("own", &[]), checker(1), &[&without_b, "c done", "d done"]);
    r.begin();
    let task_of = |id: &str| r.f.tasks().into_iter().find(|t| t.spec.input["item"] == id && t.spec.role.as_deref() == Some("scout")).map(|t| t.id).unwrap();
    let (a, b) = (task_of("a"), task_of("b"));
    r.f.rig.until(&b, "running", |t| t.state == j4n_core::task::State::Running);
    r.f.rig.finished(&a);
    r.f.front.plan_step("main");
    let turn = r.f.tasks().into_iter().rev().find(|t| t.spec.role.as_deref() == Some("plan")).unwrap();
    r.f.rig.finished(&turn.id);
    r.f.front.plan_step("main");
    assert_eq!(revs(&r), 1, "{}", r.f.rig.dump());
    let ids: Vec<String> = plan_of(&r).items.into_iter().map(|i| i.id).collect();
    assert_eq!(ids, ["a", "c", "d"].map(String::from), "b is gone from the plan");
    assert_eq!(r.item("a").state, "reviewed", "the done work is kept as it was");
    let row = r.f.rig.until(&b, "stopped", |t| t.state.is_terminal());
    assert!(!matches!(row.state, j4n_core::task::State::Done { ok: true }), "b was stopped: {:?}", row.state);
    assert_eq!(r.f.rig.of_kind("report.made").iter().filter(|e| e.task.as_deref() == Some(b.as_str())).count(), 1, "its report is kept");
    // the main lane here takes no Brief, so the next turn of its item is told the report in its text, once; it waited until then
    let sent: Vec<_> = r.f.rig.of_kind("report.sent").into_iter().filter(|e| e.data["tasks"].as_array().is_some_and(|t| t.iter().any(|x| x == b.as_str()))).collect();
    assert_eq!(sent.len(), 1, "and told to the main lane's next turn: {}", r.f.rig.dump());
    assert!(r.f.lanes.waiting("main").iter().all(|(t, _)| *t != b), "once");
    assert!(r.f.notes().iter().any(|n| n.starts_with("changed approach: b is not needed after all") && n.contains("stopped b")), "{:?}", r.f.notes());
    assert!(r.f.rig.of_kind("plan.pending").is_empty(), "dropping an item that was not done is not a decision");
}
