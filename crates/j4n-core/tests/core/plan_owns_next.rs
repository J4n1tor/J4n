//! When a task is a plan item's turn, the plan owns what comes next. The rule `followups` does
//! not fire for it; a turn that ends with the doer's own list open is evidence that the item is not done, so the item is sent
//! again with the open count and the words in hand; a turn with nothing left moves the plan on. A plain task is carried on as ever.
//! Through the real front, the base pack's loop rules and a scripted doer that keeps a session.

use crate::front_cases::{Fx, fx_trusting};
use crate::scripted::{Beat, Scripted, end, say};
use j4n_core::branch::Switch;
use j4n_core::change::Actor;
use j4n_core::doer::Caps;
use j4n_core::front::{Cfg, Said};
use serde_json::json;
use std::time::Duration;

fn three() -> String {
    let items: Vec<_> = ["a", "b", "c"].iter().map(|id| json!({ "id": id, "goal": format!("do {id}"), "reads": [], "writes": [], "depends": [], "done": format!("{id} works"), "where": "inline" })).collect();
    json!({ "approach": "Do a, b, then c.", "items": items }).to_string()
}

fn fx(turns: Vec<Vec<Beat>>) -> (Fx, std::sync::Arc<Scripted>) {
    let doer = Scripted::with_caps("claude", Caps { live: true, resume: true, ..Caps::default() }, turns);
    let f = fx_trusting(vec![doer.clone()], Some("intake=flow"), |c: &mut Cfg| {
        c.doer = Some("claude".into());
        c.lanes = Switch::Auto;
    });
    (f, doer)
}

/// Every task is over, and the rules have had their say about the last one.
fn settle(f: &Fx) {
    for r in f.tasks() {
        f.rig.finished(&r.id);
    }
    std::thread::sleep(Duration::from_millis(200));
}

fn carried_on(f: &Fx) -> usize {
    f.rig.of_kind("task.queued").iter().filter(|e| e.by == Actor::Rule("followups".into())).count()
}

fn state_of(f: &Fx, id: &str) -> String {
    f.keeper.plan("main").unwrap().items.iter().find(|i| i.id == id).unwrap().state.clone()
}

#[test]
fn a_plan_items_turn_that_ends_with_its_list_open_is_not_carried_on_the_item_is_sent_again_with_the_reason() {
    let planning = vec![Beat::Session("sess".into()), say(&three()), end(&three())];
    let open = vec![Beat::Session("sess".into()), Beat::List(2, 3, "writing tests"), Beat::Pause(100), say("half of a"), Beat::End(Some("half of a".into()), 2)];
    let (f, _) = fx(vec![planning, open, vec![Beat::List(0, 3, ""), say("did a"), end("did a")], vec![say("did b"), end("did b")], vec![say("did c"), end("did c")]]);
    let Said::Started { task } = f.say("plan how to do a, b and c, then do them") else { panic!("{}", f.rig.dump()) };
    f.rig.finished(&task);
    f.front.plan_collect("main");
    f.front.plan_step("main");
    settle(&f);
    assert_eq!(carried_on(&f), 0, "the plan owns what comes next: {}", f.rig.dump());
    assert_eq!(f.tasks().len(), 2, "one planning turn and one turn of a: {}", f.rig.dump());
    // the plan takes the turn: not done, sent back with the count and the words in hand, and started again
    f.front.plan_step("main");
    let a = f.keeper.plan("main").unwrap().items.into_iter().find(|i| i.id == "a").unwrap();
    assert_eq!((a.state.as_str(), a.sends), ("running", 1), "{}", f.rig.dump());
    let again = f.tasks().pop().unwrap();
    assert!(again.spec.goal.contains("Sent back (1 of 3)") && again.spec.goal.contains("2 item(s) open") && again.spec.goal.contains("in hand: writing tests"), "the reason goes with the item: {}", again.spec.goal);
    assert_eq!(again.spec.follows.as_deref(), Some("t2"), "in the doer's one session");
    // the second turn closes the list: the plan moves on
    settle(&f);
    f.front.plan_step("main");
    assert_eq!(state_of(&f, "a"), "merged", "{}", f.rig.dump());
    settle(&f);
    f.front.plan_step("main");
    assert_eq!(state_of(&f, "b"), "merged", "{}", f.rig.dump());
    settle(&f);
    f.front.plan_step("main");
    assert_eq!((state_of(&f, "c"), carried_on(&f)), ("merged".to_string(), 0), "{}", f.rig.dump());
}

#[test]
fn a_plain_task_with_its_list_open_is_carried_on_as_ever() {
    let (f, _) = fx(vec![vec![Beat::Session("sess".into()), say("half"), Beat::End(Some("half".into()), 2)], vec![say("done"), end("done")]]);
    let Said::Started { task } = f.say("fix the typo in the readme") else { panic!("{}", f.rig.dump()) };
    f.rig.finished(&task);
    f.rig.until_event("the rule carried it on", |e| e.kind == "task.queued" && e.by == Actor::Rule("followups".into()));
    assert_eq!(carried_on(&f), 1, "{}", f.rig.dump());
}
