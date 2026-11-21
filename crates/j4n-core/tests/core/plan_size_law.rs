//! A plan is as big as the request and the money left allow. The shape is the one `lanes = auto` can get from the own agent for
//! a three-part request: a read for each part feeding its write, a review, a changelog
//! line and a final check, 9 items, each a task that read the files again.

use crate::front_cases::{chatty, fx};
use j4n_core::branch::Switch;
use j4n_core::front::Said;
use j4n_core::keeper::{How, Item, Op, Plan, plan_bound, request_parts, take_with, take_within};
use serde_json::{Value, json};

const COMBO: &str = "This folder holds several small projects. Do all of these:\n\nIn `p1/` (a project of its own): there is a bug in `wrap_words`. Add tests that fail on the code as it is.\n\nIn `p2/`: review this package and write the bugs to `answer.json`.\n\nIn `p3/`: answer these questions, writing `answer.json`.";

fn item(id: &str, reads: &[&str], writes: &[&str], deps: &[&str], place: &str) -> Value {
    json!({ "id": id, "goal": format!("do {id}"), "reads": reads, "writes": writes, "depends": deps, "done": format!("{id} is done"), "where": place })
}

/// What the planner gave for `COMBO`.
fn nine() -> String {
    let items = vec![
        item("p1-bug", &["p1/SPEC.md", "p1/ferrule/rules.py"], &[], &[], "scout"),
        item("p1-tests", &["p1/SPEC.md", "p1/tests/test_rules.py"], &["p1/tests/test_wrap.py"], &["p1-bug"], "piece"),
        item("p2-bugs", &["p2/README.md", "p2/timeclock/api.py"], &[], &[], "scout"),
        item("p2-answer", &["p2/README.md"], &["p2/answer.json"], &["p2-bugs"], "piece"),
        item("p3-graph", &["p3/main.py", "p3/graphite/app.py"], &[], &[], "scout"),
        item("p3-answer", &["p3/main.py"], &["p3/answer.json"], &["p3-graph"], "piece"),
        item("review", &["p1/tests/test_wrap.py", "p2/answer.json", "p3/answer.json"], &[], &["p1-tests", "p2-answer", "p3-answer"], "review"),
        item("changelog", &[], &["CHANGELOG.md"], &["review"], "inline"),
        item("final-check", &["CHANGELOG.md"], &[], &["changelog"], "inline"),
    ];
    json!({ "approach": "one pass over each part, then check", "items": items }).to_string()
}

/// The items a plan that begins is given, and what was folded to get them.
fn taken(text: &str, line: &str, bound: usize) -> (Vec<Item>, Vec<String>) {
    let (ops, shift) = take_within(text, None, How::Begin, line, &[], bound).unwrap();
    (ops.into_iter().filter_map(|o| if let Op::Add(i) = o { Some(i) } else { None }).collect(), shift.folded)
}

#[test]
fn the_bound_is_the_requests_parts_and_the_money_left() {
    assert_eq!(request_parts(COMBO), 3);
    assert_eq!(plan_bound(COMBO, None, 0.0), 3);
    assert_eq!(plan_bound(&format!("{COMBO}\n\n- a\n- b\n\nRules for the whole job: add a line to the changelog."), None, 0.0), 4, "bullets are a part's, a paragraph of rules is one more");
    assert_eq!(plan_bound("fix the bug in main.rs", None, 0.0), 3, "never below three");
    assert_eq!(plan_bound("do these:\n1. a\n2. b\n3. c\n4. d\n5. e\n6. f", None, 0.0), 6);
    assert_eq!(plan_bound("do these: 1. a 2. b 3. c 4. d", None, 0.0), 4, "a line typed to `:plan` has no line breaks");
    // an item costs about what the planning turn did: $0.04 left at $0.012 a turn pays for 3
    assert_eq!(plan_bound(&format!("{COMBO}\n\nRules: a changelog line."), Some(0.04), 0.012), 3);
    assert_eq!(plan_bound(COMBO, Some(0.01), 0.012), 1, "one task at least");
    assert_eq!(plan_bound(&format!("{COMBO}\n\nRules: a changelog line."), Some(5.0), 0.012), 4, "plenty left: the request's bound");
    assert_eq!(plan_bound(&format!("{COMBO}\n\nRules: a changelog line."), Some(0.04), 0.0), 4, "a turn with no price: no bound by money");
}

#[test]
fn a_combo_plan_of_nine_items_is_folded_to_the_bound_and_every_part_keeps_its_write() {
    // the planner gave nine items, and the law's only part in `take_with` is the feeders
    assert_eq!(serde_json::from_str::<Value>(&nine()).unwrap()["items"].as_array().unwrap().len(), 9);
    let (ops, _) = take_with(&nine(), None, How::Begin, COMBO, &[]).unwrap();
    assert_eq!(ops.len(), 1 + 6, "an approach and six items: the three reads went into their writes, the rest stands");
    let (items, folded) = taken(&nine(), COMBO, plan_bound(COMBO, None, 0.0));
    let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
    assert!(items.len() <= 3, "{ids:?}");
    assert!(!folded.is_empty());
    // each scout is the first step of the write it feeds; no read is lost
    let all_writes: Vec<&str> = items.iter().flat_map(|i| i.writes.iter().map(String::as_str)).collect();
    for w in ["p1/tests/test_wrap.py", "p2/answer.json", "p3/answer.json", "CHANGELOG.md"] {
        assert!(all_writes.contains(&w), "{w} in {all_writes:?}");
    }
    let p2 = items.iter().find(|i| i.writes.contains(&"p2/answer.json".to_string())).unwrap();
    assert!(p2.reads.contains(&"p2/README.md".to_string()), "{:?}", p2.reads);
    assert!(p2.goal.contains("do p2-bugs"), "{}", p2.goal);
    // every dependency is an item of the plan, and nothing waits for itself
    for i in &items {
        assert!(i.depends.iter().all(|d| ids.contains(&d.as_str()) && *d != i.id), "{}: {:?}", i.id, i.depends);
    }
}

#[test]
fn money_left_bounds_the_plan_below_the_requests_bound() {
    let (items, _) = taken(&nine(), COMBO, 2);
    assert_eq!(items.len(), 2, "{:?}", items.iter().map(|i| &i.id).collect::<Vec<_>>());
}

#[test]
fn a_read_that_feeds_two_items_stays_and_a_review_stays_inside_the_bound() {
    let items = vec![
        item("map", &["a.rs"], &[], &[], "scout"),
        item("one", &[], &["b.rs"], &["map"], "inline"),
        item("two", &[], &["c.rs"], &["map"], "inline"),
        item("review", &["b.rs", "c.rs"], &[], &["one", "two"], "review"),
    ];
    let text = json!({ "approach": "a", "items": items }).to_string();
    let (items, folded) = taken(&text, "do these:\n1. one\n2. two\n3. three", 4);
    assert!(folded.is_empty(), "{folded:?}");
    assert_eq!(items.len(), 4);
}

#[test]
fn an_item_that_reads_more_than_twelve_paths_is_cut_not_refused() {
    let many: Vec<String> = (0..20).map(|n| format!("src/f{n}.rs")).collect();
    let items = vec![json!({ "id": "all", "goal": "read all", "reads": many, "writes": ["out.md"], "depends": [], "done": "written" })];
    let text = json!({ "approach": "a", "items": items }).to_string();
    let (items, _) = taken(&text, "summarise", 3);
    assert_eq!(items[0].reads.len(), 12);
}

#[test]
fn an_open_plan_is_not_folded_by_a_change() {
    let (mut items, _) = taken(&nine(), COMBO, 9);
    items.iter_mut().for_each(|i| i.state = "todo".into());
    let open = Plan { items, approach: "a".into(), ..Plan::default() };
    assert!(open.open());
    let (ops, shift) = take_within(&nine(), Some(&open), How::Change, COMBO, &[], 3).unwrap();
    assert!(shift.folded.is_empty(), "{:?}", shift.folded);
    assert!(!ops.is_empty());
}

#[test]
fn a_planning_turn_through_the_front_begins_a_plan_within_the_bound() {
    let f = fx(vec![chatty("claude", &["hello", &nine()])], |c| {
        c.doer = Some("claude".into());
        c.lanes = Switch::Auto;
    });
    f.turn("hello");
    let Said::Started { task } = f.say(&format!(":plan {COMBO}")) else { panic!("{}", f.rig.dump()) };
    f.rig.finished(&task);
    f.front.plan_collect("main");
    let plan = f.keeper.plan("main").unwrap();
    assert!(plan.items.len() <= 3, "{:?}", plan.items.iter().map(|i| &i.id).collect::<Vec<_>>());
    let settled = f.rig.of_kind("plan.settled");
    assert!(!settled[0].data["folded"].as_array().unwrap().is_empty(), "{:?}", settled[0].data);
}
