//! Before the front starts the next inline plan item, a window past `compact.at` and
//! a chain under `compact.max` have the item start with a compaction that keeps the plan line; at `compact.max` it starts as it is and a
//! note says so; under `compact.at` nothing compacts. Through the real front, the real loop and a doer that can shrink a session.

use crate::front_cases::{Fx, fx_deciding};
use crate::scripted::{Beat, Scripted, end, say};
use j4n_core::branch::Switch;
use j4n_core::doer::Caps;
use j4n_core::front::{Cfg, Said};
use serde_json::json;

fn three() -> String {
    let items: Vec<_> = ["a", "b", "c"].iter().map(|id| json!({ "id": id, "goal": format!("do {id}"), "reads": [], "writes": [], "depends": [], "done": format!("{id} works"), "where": "inline" })).collect();
    json!({ "approach": "Do a, b, then c.", "items": items }).to_string()
}

/// A turn that ends with the window at `w`; a `shrink` turn is the compaction a start with `compact` asks for first.
fn turn(w: f64, answer: &str) -> Vec<Beat> {
    vec![Beat::Session("sess".into()), Beat::Window(w), say(answer), end(answer)]
}

fn shrink() -> Vec<Beat> {
    vec![say("kept"), end("kept")]
}

fn run(turns: Vec<Vec<Beat>>, tune: impl FnOnce(&mut Cfg)) -> (Fx, std::sync::Arc<Scripted>) {
    let doer = Scripted::with_caps("claude", Caps { live: true, resume: true, compact: true, ..Caps::default() }, turns);
    let f = fx_deciding(vec![doer.clone()], Some("intake=flow"), |c| {
        c.doer = Some("claude".into());
        c.lanes = Switch::Auto;
        tune(c);
    });
    let Said::Started { task } = f.say("plan how to do a, b and c, then do them") else { panic!("{}", f.rig.dump()) };
    f.rig.finished(&task);
    f.front.plan_collect("main");
    for _ in 0..3 {
        for r in f.tasks() {
            f.rig.finished(&r.id);
        }
        f.front.plan_step("main");
    }
    for r in f.tasks() {
        f.rig.finished(&r.id);
    }
    (f, doer)
}

#[test]
fn a_full_window_has_the_next_item_start_with_a_compaction_that_keeps_the_plan_line() {
    let turns = vec![turn(0.2, &three()), turn(0.8, "did a"), shrink(), turn(0.8, "did b"), shrink(), turn(0.3, "did c")];
    let (f, doer) = run(turns, |_| {});
    let starts = doer.starts();
    assert_eq!(starts.iter().map(|s| s.compacting).collect::<Vec<_>>(), [false, false, true, false, true, false], "{}", f.rig.dump());
    let kept = f.rig.of_kind("task.compacting");
    assert_eq!(kept.len(), 2);
    let block = kept[0].data["keep"].as_str().unwrap();
    assert!(block.contains("MUST KEEP") && block.contains("Do a, b, then c."), "the plan line rides the block: {block}");
    assert!(f.tasks().iter().all(|r| r.compactions <= 2));
    assert_eq!(f.tasks().iter().map(|r| r.compactions).max(), Some(2));
}

#[test]
fn at_compact_max_the_item_starts_as_it_is_and_a_note_says_so_and_under_compact_at_nothing_compacts() {
    let turns = vec![turn(0.2, &three()), turn(0.8, "did a"), shrink(), turn(0.8, "did b"), turn(0.8, "did c")];
    let (f, doer) = run(turns, |c| c.compact_max = 1);
    assert_eq!(doer.starts().iter().map(|s| s.compacting).collect::<Vec<_>>(), [false, false, true, false, false], "{}", f.rig.dump());
    assert_eq!(f.rig.of_kind("task.compacting").len(), 1);
    assert!(f.notes().iter().any(|n| n.contains("shrunk 1 times already") && n.contains("starts as it is")), "{:?}", f.notes());
    let (g, quiet) = run(vec![turn(0.2, &three()), turn(0.5, "did a"), turn(0.7, "did b"), turn(0.1, "did c")], |_| {});
    assert!(quiet.starts().iter().all(|s| !s.compacting) && g.rig.of_kind("task.compacting").is_empty(), "{}", g.rig.dump());
}

#[test]
fn the_block_keeps_the_operators_last_line_as_the_goal_not_the_planning_turns_prompt() {
    let turns = vec![turn(0.2, &three()), turn(0.8, "did a"), shrink(), turn(0.8, "did b"), shrink(), turn(0.3, "did c")];
    let (f, _) = run(turns, |_| {});
    for k in f.rig.of_kind("task.compacting") {
        let block = k.data["keep"].as_str().unwrap();
        assert!(block.contains("- goal: plan how to do a, b and c, then do them"), "the operator's line is the goal: {block}");
        assert!(!block.contains("Plan this for the project"), "the planning prompt is not the goal: {block}");
    }
}
