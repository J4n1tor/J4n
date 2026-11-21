//! A headless `lanes = auto` run on a combo-shaped request (three parts, each in a
//! folder of its own) stopped at the first item with no check, because `unproven` was made a stop; the check looked at the folder
//! root only (`j4n-connect` `check_cases`); and the planner chained the independent parts.

use crate::front_cases::{Fx, fx_deciding};
use crate::scripted::{Beat, Scripted, end, say};
use j4n_core::branch::Switch;
use j4n_core::front::{Cfg, Said};
use j4n_core::keeper::{How, Item, Op, take_within};
use serde_json::{Value, json};

fn item(id: &str, reads: &[&str], writes: &[&str], deps: &[&str], place: &str, check: &str) -> Value {
    json!({ "id": id, "goal": format!("do {id}"), "reads": reads, "writes": writes, "depends": deps, "done": format!("{id} is done"), "where": place, "check": check })
}

fn taken(items: Vec<Value>, line: &str, bound: usize) -> (Vec<Item>, Vec<String>) {
    let text = json!({ "approach": "each part in its folder", "items": items }).to_string();
    let (ops, shift) = take_within(&text, None, How::Begin, line, &[], bound).unwrap();
    (ops.into_iter().filter_map(|o| if let Op::Add(i) = o { Some(i) } else { None }).collect(), shift.freed)
}

fn edits(path: &str) -> Vec<Beat> {
    vec![Beat::Session("s".into()), Beat::Put(path.into(), "x = 1\n".into()), say("done"), end("done")]
}

/// The planner's answer, then a turn for each item, in a headless run.
fn rig(items: Vec<Value>, turns: Vec<Vec<Beat>>) -> (Fx, std::sync::Arc<Scripted>) {
    let plan = json!({ "approach": "in order", "items": items }).to_string();
    let mut all = vec![vec![Beat::Session("s".into()), say(&plan), end(&plan)]];
    all.extend(turns);
    let checker = Scripted::new("check", vec![]);
    let f = fx_deciding(vec![Scripted::new("claude", all), checker.clone()], None, |c: &mut Cfg| {
        c.doer = Some("claude".into());
        c.lanes = Switch::Auto;
        c.checks = vec!["unittest".into()];
    });
    let Said::Started { task } = f.say("plan how to do these three parts, then do them") else { panic!("{}", f.rig.dump()) };
    f.rig.finished(&task);
    f.front.plan_collect("main");
    f.front.plan_step("main");
    (f, checker)
}

fn drive(f: &Fx) -> Vec<Item> {
    for _ in 0..40 {
        let items = f.keeper.plan("main").unwrap().items;
        if items.iter().all(Item::done) || items.iter().any(|i| i.state == "blocked") {
            return items;
        }
        for r in f.tasks() {
            f.rig.finished(&r.id);
        }
        f.front.plan_step("main");
    }
    panic!("the plan never ended: {:?}\n{}", f.keeper.plan("main").unwrap().items.iter().map(|i| (&i.id, &i.state)).collect::<Vec<_>>(), f.rig.dump());
}

#[test]
fn unproven_is_a_mark_not_a_stop_the_dependents_start_and_the_report_says_how_many() {
    // each part reads what the one before wrote, so the law keeps the order; none names a check
    let items = vec![
        item("p1", &["p1/SPEC.md"], &["p1/a.py"], &[], "inline", "none"),
        item("p2", &["p1/a.py"], &["p2/b.py"], &["p1"], "inline", "none"),
        item("p3", &["p2/b.py"], &["p3/c.py"], &["p2"], "inline", "none"),
    ];
    let (f, checker) = rig(items, vec![edits("p1/a.py"), edits("p2/b.py"), edits("p3/c.py")]);
    let items = drive(&f);
    assert!(items.iter().all(|i| i.state == "merged"), "{:?}\n{}", items.iter().map(|i| (&i.id, &i.state, &i.result)).collect::<Vec<_>>(), f.rig.dump());
    assert!(items.iter().all(|i| i.unproven && i.result.starts_with("unproven:")), "each carries the mark: {items:?}");
    let states: Vec<String> = f.rig.of_kind("plan.item").into_iter().filter(|e| e.data["op"] == "state").map(|e| e.data["state"].as_str().unwrap().to_string()).collect();
    assert!(!states.iter().any(|s| s == "checked"), "an item no check ran for is never `checked`: {states:?}");
    assert_eq!(checker.starts().len(), 0);
    let said = f.rig.of_kind("plan.reported");
    assert_eq!(said.len(), 1, "{}", f.rig.dump());
    assert_eq!((said[0].data["outcome"].as_str(), said[0].data["unproven"].as_array().map(Vec::len)), (Some("done"), Some(3)), "{:?}", said[0].data);
    assert_eq!(said[0].data["decisions"].as_array().unwrap().len(), 3, "each is still listed under yours to decide");
    assert!(f.rig.of_kind("plan.pending").is_empty(), "an unproven item does not start a replan");
}

#[test]
fn the_operator_accepting_an_unproven_item_takes_the_mark_off() {
    let (f, _) = rig(vec![item("p1", &[], &["p1/a.py"], &[], "inline", "none")], vec![edits("p1/a.py")]);
    assert!(drive(&f)[0].unproven);
    f.say(":plan state p1 merged");
    let p1 = f.keeper.plan("main").unwrap().items.remove(0);
    assert!(!p1.unproven && p1.state == "merged", "{p1:?}");
}

#[test]
fn a_combo_plan_of_three_independent_parts_has_no_depends_between_them() {
    // p3 waited for p1 and p2 in 9 of 13 plans, and each part sits in a folder of its own
    let items = vec![
        item("p1-read-kv", &["p1/SPEC.md", "p1/ridge/parse.py"], &["p1/ridge/parse.py"], &[], "piece", "unittest"),
        item("p2-bug-review", &["p2/README.md", "p2/jobwheel/core.py"], &["p2/answer.json"], &[], "piece", "none"),
        item("p3-call-graph", &["p3/main.py", "CHANGELOG.md"], &["p3/answer.json", "CHANGELOG.md"], &["p1-read-kv", "p2-bug-review"], "piece", "none"),
    ];
    let (got, freed) = taken(items, "do these", 3);
    assert!(got.iter().all(|i| i.depends.is_empty()), "{:?}", got.iter().map(|i| (&i.id, &i.depends)).collect::<Vec<_>>());
    assert_eq!(freed, ["p3-call-graph after p1-read-kv", "p3-call-graph after p2-bug-review"]);
}

#[test]
fn a_dependency_stays_when_the_paths_meet_or_cannot_be_told() {
    let items = vec![
        item("lib", &[], &["src/lib.rs"], &[], "inline", "none"),
        item("scout", &["docs/a.md"], &[], &[], "scout", "none"),
        item("api", &["src/lib.rs"], &["src/api.rs"], &["lib"], "inline", "none"),
        item("later", &["README.md"], &["README.md"], &["api"], "inline", "none"),
        item("note", &["README.md"], &["notes/n.md"], &["scout"], "inline", "none"),
        item("bare", &[], &[], &["lib"], "inline", "none"),
        item("end", &[], &["src/e.rs"], &["lib"], "inline", "none"),
        item("under", &["src/"], &["out/x"], &["lib"], "inline", "none"),
        item("rewrites", &[], &["src/lib.rs"], &["api"], "inline", "none"),
    ];
    let (got, freed) = taken(items, "do these", 9);
    let deps = |id: &str| got.iter().find(|i| i.id == id).unwrap().depends.clone();
    assert_eq!(deps("api"), ["lib"], "it reads what lib writes");
    assert_eq!(deps("note"), ["scout"], "a read-only item hands over what it found, not a file");
    assert_eq!(deps("bare"), ["lib"], "an item that names no path is not told apart");
    assert_eq!(deps("end"), ["lib"], "nor one that reads nothing: it may sum up what the others did");
    assert_eq!(deps("under"), ["lib"], "a folder meets the files under it");
    assert_eq!(deps("rewrites"), ["api"], "it writes what api reads");
    assert!(deps("later").is_empty(), "README.md is nobody's before it: {freed:?}");
}

#[test]
fn a_chain_is_kept_when_a_later_item_reads_what_an_earlier_one_wrote() {
    // c reads a's file and depends on b only, which depends on a: dropping c after b would lose a
    let items = vec![
        item("a", &[], &["a.txt"], &[], "inline", "none"),
        item("b", &["a.txt"], &["b.txt"], &["a"], "inline", "none"),
        item("c", &["a.txt"], &["c.txt"], &["b"], "inline", "none"),
    ];
    let (got, freed) = taken(items, "do these", 3);
    assert_eq!(got.iter().find(|i| i.id == "c").unwrap().depends, ["b"], "{freed:?}");
}

#[test]
fn the_planning_prompt_says_parts_that_share_no_file_have_no_depends() {
    let (f, _) = rig(vec![item("p1", &[], &["p1/a.py"], &[], "inline", "none")], vec![edits("p1/a.py")]);
    let plan = f.tasks().into_iter().find(|t| t.spec.role.as_deref() == Some("plan")).unwrap();
    assert!(plan.spec.goal.contains("share no file have no `depends`"), "{}", plan.spec.goal);
}
