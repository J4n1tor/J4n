//! The one switch `lanes = single · auto · wide`. Each cell of the table is one
//! assertion; in `single` the Log has no automatic act, and `/plan` and the pin go as they would in a build without the pack. `:lanes`
//! itself, a proposal in the operator's folder, is in `laws/lanes_flow.rs` (it needs the proposer the binary wires).

use crate::front_cases::{Fx, chatty, fx_deciding};
use crate::scripted::Scripted;
use j4n_core::branch::{Class, Ctx, Rules, Stay, Switch, Verdict, decide};
use j4n_core::front::{Cfg, Said};
use j4n_core::keeper::{Item, Plan};
use serde_json::{Value, json};
use std::sync::Arc;

/// The doer pinned and `lanes = auto` said (single is the default).
fn pinned(c: &mut Cfg) {
    c.doer = Some("claude".into());
    c.lanes = Switch::Auto;
}

fn single(c: &mut Cfg) {
    pinned(c);
    c.lanes = Switch::Single;
}

fn three() -> String {
    let items: Vec<Value> = ["read", "change", "check"]
        .iter()
        .enumerate()
        .map(|(n, id)| json!({ "id": id, "goal": format!("do {id}"), "reads": [], "writes": [format!("src/{id}.rs")], "depends": if n == 0 { vec![] } else { vec![["read", "change", "check"][n - 1]] }, "done": format!("{id} works") }))
        .collect();
    json!({ "approach": "Read first, then change, then check.", "items": items }).to_string()
}

fn doer(answers: &[String]) -> Arc<Scripted> {
    let refs: Vec<&str> = answers.iter().map(String::as_str).collect();
    chatty("claude", &refs)
}

fn kinds(f: &Fx, prefix: &str) -> Vec<String> {
    f.rig.events().into_iter().map(|e| e.kind).filter(|k| k.starts_with(prefix)).collect()
}

fn item(mark: &str) -> Item {
    Item { id: "i".into(), goal: "look at the retry loop and say where it gives up and why it does so".into(), done: "it is said".into(), mark: mark.into(), state: "todo".into(), place: "inline".into(), reads: vec!["src/a/1.rs".into(), "src/a/2.rs".into(), "src/a/3.rs".into()], ..Item::default() }
}

fn ctx(switch: Switch) -> Ctx {
    Ctx { switch, ..Ctx::default() }
}

#[test]
fn the_cells_of_the_table_one_assertion_each() {
    let rules = Rules::builtin();
    // branches by rule: none in single, scout and review in auto, and piece too in wide
    for (switch, class, allowed) in [
        (Switch::Single, Class::Scout, false),
        (Switch::Single, Class::Review, false),
        (Switch::Single, Class::Piece, false),
        (Switch::Auto, Class::Scout, true),
        (Switch::Auto, Class::Review, true),
        (Switch::Auto, Class::Piece, false),
        (Switch::Wide, Class::Scout, true),
        (Switch::Wide, Class::Review, true),
        (Switch::Wide, Class::Piece, true),
    ] {
        assert_eq!(rules.allows(class, switch), allowed, "{} under {}", class.word(), switch.word());
    }
    // and the decision follows the rows: a scout branches under auto and wide, never under single
    let plan = Plan { items: vec![item("scout")], approach: "x".into(), ..Plan::default() };
    let at = |s: Switch| decide(&plan.items[0], &plan, &ctx(s));
    assert!(matches!(at(Switch::Single), Verdict::Inline { test: None, .. }));
    assert!(matches!(at(Switch::Auto), Verdict::Branch { class: Class::Scout, .. }));
    assert!(matches!(at(Switch::Wide), Verdict::Branch { class: Class::Scout, .. }));
    // the rows say what a branch is given: a scout and a reviewer only read, and the Gate refuses a write whatever the doer does
    assert_eq!(rules.mode(Class::Scout), j4n_core::task::Mode::Read);
    assert_eq!(rules.mode(Class::Piece), j4n_core::task::Mode::Write);
    assert_eq!(rules.limits(Class::Scout), (Some(0.20), Some(120_000)));
    assert_eq!(rules.limits(Class::Piece), (None, None));
    // a review's time grows with the files it is given: five minutes for one, a minute more for each past it, fifteen at most
    let min = 60_000;
    assert_eq!([0, 1, 2, 10, 40].map(|n| rules.wall(Class::Review, n)), [Some(5 * min), Some(5 * min), Some(6 * min), Some(14 * min), Some(15 * min)]);
    assert_eq!((rules.wall(Class::Scout, 9), rules.wall(Class::Piece, 9)), (Some(2 * min), None), "a row with no `wall_file` keeps its wall");
    // no pack, no rows, no branch: a scout stays inline even under wide, and says why
    let none = decide(&plan.items[0], &plan, &Ctx { rules: Rules::none(), ..ctx(Switch::Wide) });
    assert!(matches!(&none, Verdict::Inline { test: None, why } if why.contains("pack `lanes` is off")), "{none:?}");
    // the stay-inline tests still win over a row
    let main = Item { main: true, ..item("scout") };
    assert_eq!(decide(&main, &plan, &ctx(Switch::Auto)).test(), Some(Stay::Main));
    // the default is single (it was auto before), and a word that is not one is not a switch
    assert_eq!(Cfg::new("p").lanes, Switch::Single);
    assert_eq!(Switch::from_word("narrow"), None);
}

#[test]
fn in_single_a_plan_shaped_line_is_a_task_and_the_log_has_no_automatic_act() {
    let f = fx_deciding(vec![doer(&["added the sqlite log".into()])], Some("intake=flow"), single);
    f.turn("plan how to move the log to sqlite, then build it and check it");
    assert!(kinds(&f, "plan.").is_empty() && !f.keeper.open("main"), "single: no plan, no act: {:?}", kinds(&f, "plan."));
    assert_ne!(f.tasks()[0].spec.role.as_deref(), Some("plan"), "the line went to the doer as a task");
    f.front.plan_step("main");
    assert!(kinds(&f, "plan.").is_empty());
    // the same line in auto begins a plan, and what it does on its own is a note and an event
    let g = fx_deciding(vec![doer(&[three(), "read it".into()])], Some("intake=flow"), pinned);
    let Said::Started { task } = g.say("plan how to move the log to sqlite, then build it and check it") else { panic!("{}", g.rig.dump()) };
    g.rig.finished(&task);
    g.front.plan_collect("main");
    assert!(g.keeper.open("main"));
    g.front.plan_step("main");
    let routed = g.rig.of_kind("plan.routed");
    assert_eq!(routed.len(), 1, "{}", g.rig.dump());
    let note = g.notes().into_iter().find(|n| n.contains("read → inline")).expect("a note for the act");
    assert!(note.contains(routed[0].data["why"].as_str().unwrap()), "{note}");
}

#[test]
fn in_single_the_plan_command_and_the_pin_go_as_they_would_without_the_pack() {
    let f = fx_deciding(vec![doer(&["hello".into(), three(), "added the flag".into()])], Some("intake=flow"), single);
    f.turn("hello");
    let Said::Started { task } = f.say(":plan move the log to sqlite") else { panic!("{}", f.rig.dump()) };
    f.rig.finished(&task);
    f.front.plan_collect("main");
    assert_eq!(f.keeper.plan("main").unwrap().items.len(), 3, "`/plan` still works");
    assert_eq!(f.bound()[0].data["how"], "pinned", "the pin still binds");
    // a line on the open plan is a task as typed: no fold, and nothing starts by itself
    let tasks = f.tasks().len();
    f.turn("also add the flag");
    assert_eq!(f.tasks().len(), tasks + 1);
    assert!(kinds(&f, "plan.folded").is_empty() && kinds(&f, "plan.routed").is_empty());
    f.front.plan_step("main");
    assert_eq!(f.tasks().len(), tasks + 1, "single: the operator's lines drive the plan");
    assert_ne!(f.tasks().last().unwrap().spec.role.as_deref(), Some("plan"));
}

#[test]
fn a_plan_shaped_first_line_begins_a_plan_with_no_model_asked_and_a_plain_line_stays_a_task() {
    let began = |line: &str, tune: fn(&mut Cfg)| {
        let f = fx_deciding(vec![doer(&["ok".into(), three()])], None, tune);
        let said = f.say(line);
        (f.rig.of_kind("plan.asked").len(), f.rig.of_kind("intake.decided").remove(0).data["by"].as_str().unwrap().to_string(), said)
    };
    for line in ["Plan this and do it in order: (1) read the log (2) change the loop (3) check it (4) write it up", "plan how to move the log to sqlite", "Do these:\n1. read\n2. change\n3. check", "- read it\n- change it\n- check it"] {
        let (asked, by, _) = began(line, pinned);
        assert_eq!((asked, by.as_str()), (1, "front:plan-shaped"), "{line}");
    }
    for line in ["fix the typo in the readme", "what is the plan for the log?", "step (1) is to read the log", "explain the planning module"] {
        let (asked, by, _) = began(line, pinned);
        assert!(asked == 0 && by != "front:plan-shaped", "a plain line is a task: {line} ({by})");
    }
    assert_eq!(began("Plan this and do it in order: (1) a (2) b (3) c", single).0, 0, "single: nothing changes");
}
