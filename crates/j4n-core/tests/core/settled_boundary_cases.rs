//! Law 9's settled boundary is said once, in `Front::settled`, and each hook of `plan_step` asks it when it runs.
//! What is pinned here: a hook does nothing while the thread's newest task runs, and the plan's two hooks keep their order (what a finished turn brought is taken
//! before the next item starts). The loop, the front and the Log are the real ones; the doer is scripted.

use crate::front_cases::{Fx, fx_planned, fx_trusting};
use crate::scripted::{Beat, Scripted, end, say};
use j4n_core::branch::Switch;
use j4n_core::doer::Caps;
use j4n_core::front::session::Life;
use j4n_core::front::{Cfg, Said};
use serde_json::json;
use std::sync::Arc;

fn turn(answer: &str) -> Vec<Beat> {
    vec![Beat::Session("s1".into()), Beat::Window(0.3), say(answer), end(answer)]
}

fn slow(answer: &str) -> Vec<Beat> {
    vec![Beat::Session("s1".into()), Beat::Pause(800), say(answer), end(answer)]
}

fn caps() -> Caps {
    Caps { live: true, resume: true, compact: true, ..Caps::default() }
}

#[test]
fn a_queued_compact_waits_while_the_newest_task_runs_and_is_made_at_its_end() {
    let claude = Scripted::with_caps("claude", caps(), vec![turn("a"), slow("b"), vec![say("kept"), end("kept")]]);
    let f = fx_planned(vec![claude.clone()], |c| c.session = Life::Auto);
    f.turn("one");
    let Said::Started { task } = f.say("two") else { panic!("{}", f.rig.dump()) };
    f.say(":compact");
    // not settled: the step makes nothing, and the queue is kept (not dropped as after a failure)
    f.front.plan_step("main");
    assert_eq!(claude.starts().len(), 2, "{}", f.rig.dump());
    assert!(f.notes().iter().all(|n| !n.starts_with("compact dropped")), "{:?}", f.notes());
    f.rig.finished(&task);
    f.front.plan_step("main");
    assert_eq!(claude.starts().len(), 3, "settled: the queued compact is made now: {}", f.rig.dump());
}

fn three() -> String {
    let items: Vec<_> = ["a", "b"].iter().map(|id| json!({ "id": id, "goal": format!("do {id}"), "reads": [], "writes": [], "depends": [], "done": format!("{id} works"), "where": "inline" })).collect();
    json!({ "approach": "Do a, then b.", "items": items }).to_string()
}

fn planned(turns: Vec<Vec<Beat>>) -> (Fx, Arc<Scripted>) {
    let doer = Scripted::with_caps("claude", Caps { live: true, resume: true, ..Caps::default() }, turns);
    let f = fx_trusting(vec![doer.clone()], Some("intake=flow"), |c: &mut Cfg| {
        c.doer = Some("claude".into());
        c.lanes = Switch::Auto;
    });
    (f, doer)
}

#[test]
fn one_step_takes_the_planning_turn_and_then_starts_the_first_item_and_no_second_while_it_runs() {
    let planning = vec![Beat::Session("sess".into()), say(&three()), end(&three())];
    let (f, _) = planned(vec![planning, vec![Beat::Session("sess".into()), Beat::Pause(800), say("did a"), end("did a")], vec![say("did b"), end("did b")]]);
    let Said::Started { task } = f.say("plan how to do a and b, then do them") else { panic!("{}", f.rig.dump()) };
    f.rig.finished(&task);
    // plan_collect takes the plan, plan_drive starts its item: collect first, in the one step
    f.front.plan_step("main");
    let items = f.keeper.plan("main").unwrap().items;
    assert_eq!((f.tasks().len(), items.iter().map(|i| i.state.as_str()).collect::<Vec<_>>()), (2, vec!["running", "todo"]), "{}", f.rig.dump());
    // the newest task runs: the boundary is not there, and the step starts nothing
    f.front.plan_step("main");
    assert_eq!(f.tasks().len(), 2, "{}", f.rig.dump());
}
