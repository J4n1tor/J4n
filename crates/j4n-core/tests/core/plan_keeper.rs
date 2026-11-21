//! The plan keeper through the real front, and the shape check of the
//! planner's answer, `rule:fold` and the Brief's line on their own.

use crate::front_cases::{Fx, chatty, fx, fx_deciding};
use crate::scripted::{Beat, Scripted, end};
use j4n_core::answer::{Answerer, Offer, Question};
use j4n_core::ask::{Answer, AskShape};
use j4n_core::doer::Caps;
use j4n_core::fold::Fold;
use j4n_core::front::{Cfg, Said};
use j4n_core::keeper::{How, Keeper, Op, Plan, fold_pick, take};
use j4n_core::task::{Mode, State};
use serde_json::{Value, json};

/// The doer pinned and `lanes = auto` said (single is the default).
fn pinned(c: &mut Cfg) {
    c.doer = Some("claude".into());
    c.lanes = j4n_core::branch::Switch::Auto;
}

/// An answer of a planning turn: `(id, depends)` of each item.
fn plan(approach: &str, items: &[(&str, &[&str])]) -> String {
    let items: Vec<Value> = items
        .iter()
        .map(|(id, deps)| json!({ "id": id, "goal": format!("do {id}"), "reads": [format!("src/{id}.rs")], "writes": [format!("src/{id}.rs")], "depends": deps, "done": format!("{id} works") }))
        .collect();
    json!({ "approach": approach, "items": items }).to_string()
}

fn three() -> String {
    plan("Read first, then change, then check.", &[("read", &[]), ("change", &["read"]), ("check", &["change"])])
}

/// A doer that answers each of its turns with the next text, and keeps a session so that the next turn follows the last.
fn doer(answers: &[String]) -> std::sync::Arc<Scripted> {
    let refs: Vec<&str> = answers.iter().map(String::as_str).collect();
    chatty("claude", &refs)
}

fn ended(f: &Fx, said: Said) -> String {
    let Said::Started { task } = said else { panic!("no task was started: {said:?}\n{}", f.rig.dump()) };
    f.rig.finished(&task);
    f.front.plan_collect("main");
    task
}

fn items(f: &Fx) -> Vec<(String, String)> {
    f.keeper.plan("main").map(|p| p.items.into_iter().map(|i| (i.id, i.state)).collect()).unwrap_or_default()
}

fn kinds(f: &Fx, kind: &str) -> Vec<Value> {
    f.rig.of_kind(kind).into_iter().map(|e| e.data).collect()
}

#[test]
fn a_line_on_an_open_plan_is_folded_into_it_by_a_planning_turn_and_not_run_as_typed() {
    let more = plan("Read first, then change, then check, and say it in the log.", &[("read", &[]), ("change", &["read"]), ("check", &["change"]), ("log", &["check"])]);
    let f = fx(vec![doer(&["hello".into(), three(), more])], pinned);
    f.turn("what does main.rs do?");
    ended(&f, f.say(":plan move the log to sqlite"));
    assert_eq!(items(&f), vec![("read".into(), "todo".into()), ("change".into(), "todo".into()), ("check".into(), "todo".into())]);
    let before = f.keeper.plan("main").unwrap();
    // "also" is the cheap case of rule:fold: the line becomes an item, and no task runs it as typed
    let tasks = f.tasks().len();
    let task = ended(&f, f.say("also write a line in the changelog"));
    assert_eq!(f.tasks().len(), tasks + 1, "one planning turn, and nothing else");
    let folded = kinds(&f, "plan.folded");
    assert_eq!((folded[0]["pick"].as_str(), folded[0]["by"].as_str()), (Some("add"), Some("rule:fold")));
    let after = f.keeper.plan("main").unwrap();
    assert_eq!(after.items.len(), 4);
    assert_eq!(&after.items[..3], &before.items[..], "the items there were are as they were");
    assert_eq!(after.items[3].id, "log");
    assert!(after.reasons.last().unwrap().starts_with("add:"), "{:?}", after.reasons);
    // the Log shows the old items and the new one
    let adds: Vec<_> = kinds(&f, "plan.item").iter().map(|d| d["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(adds, ["read", "change", "check", "log"]);
    assert_eq!(f.rig.tasks.book().get(&task).unwrap().spec.role.as_deref(), Some("plan"));
}

#[test]
fn a_change_is_taken_as_a_change_and_an_answer_runs_as_a_turn_without_touching_the_plan() {
    let changed = plan("Read first, then change, then check.", &[("read", &[]), ("change", &["read"]), ("check", &["change"])]).replace("do change", "do change in main.rs only");
    let f = fx(vec![doer(&["hello".into(), three(), changed, "it prints".into()])], pinned);
    f.turn("hello");
    ended(&f, f.say(":plan move the log"));
    ended(&f, f.say("instead change main.rs only"));
    assert_eq!(kinds(&f, "plan.folded")[0]["pick"], "change");
    let p = f.keeper.plan("main").unwrap();
    assert_eq!((p.items[1].goal.as_str(), p.items[1].rev), ("do change in main.rs only", 1));
    let rev = p.items.iter().map(|i| i.rev).sum::<u32>();
    // a question is an answer: a turn of the doer, and no item moves
    f.turn("what does the check item do?");
    assert_eq!(kinds(&f, "plan.folded")[1]["pick"], "answer");
    assert_eq!(f.keeper.plan("main").unwrap().items.iter().map(|i| i.rev).sum::<u32>(), rev);
    assert!(items(&f).iter().all(|(_, s)| s == "todo"));
}

#[test]
fn a_line_that_would_replace_the_plan_waits_for_the_operators_yes() {
    let again = plan("Move it all to sqlite.", &[("schema", &[]), ("copy", &["schema"]), ("bench", &["copy"])]);
    let f = fx(vec![doer(&["hello".into(), three(), again])], pinned);
    f.turn("hello");
    ended(&f, f.say(":plan fix the flag"));
    let tasks = f.tasks().len();
    assert!(matches!(f.say("start over: move the log to sqlite"), Said::Command(c) if c == "plan"));
    assert_eq!(f.tasks().len(), tasks, "nothing runs before the yes");
    assert_eq!(f.keeper.plan("main").unwrap().held.as_deref(), Some("start over: move the log to sqlite"));
    ended(&f, f.say(":plan replace"));
    let ids: Vec<String> = items(&f).into_iter().map(|(i, _)| i).collect();
    assert_eq!(ids, ["schema", "copy", "bench"]);
    assert_eq!(f.keeper.plan("main").unwrap().held, None);
}

#[test]
fn with_no_open_plan_a_line_is_a_task() {
    let f = fx(vec![doer(&["hello".into(), "added the flag".into()])], pinned);
    f.turn("hello");
    f.turn("also add the flag");
    assert!(f.rig.of_kind("plan.folded").is_empty() && f.rig.of_kind("plan.asked").is_empty(), "no plan, no fold");
    assert!(!f.keeper.open("main"));
}

/// The minimum of three items refused the answer of a planning turn that was paid for (two items, "fewer than 3 is
/// just a task"), after the prompt had told the planner to answer with one or two when the work was small. A short plan is a plan.
#[test]
fn a_plan_of_one_or_two_items_begins_a_plan_and_the_planner_is_not_told_otherwise() {
    let steps: [(&str, &[&str]); 2] = [("read", &[]), ("change", &["read"])];
    for n in 1..=2 {
        let f = fx(vec![doer(&["hello".into(), plan("Few steps.", &steps[..n])])], pinned);
        f.turn("hello");
        ended(&f, f.say(":plan a small thing"));
        let settled = kinds(&f, "plan.settled");
        assert_eq!(settled[0]["ok"], true, "{settled:?}");
        assert!(f.keeper.open("main"), "{n} item(s) are a plan");
        assert_eq!(items(&f).len(), n);
        assert!(!f.notes().iter().any(|m| m.contains("no plan taken")), "{:?}", f.notes());
        let asked = f.rig.of_kind("task.queued").iter().filter_map(|e| e.data["spec"]["goal"].as_str().map(String::from)).find(|g| g.contains("Plan this for the project")).unwrap();
        assert!(!asked.contains("At least three") && asked.contains("one or two"), "{asked}");
    }
}

#[test]
fn a_plan_shaped_first_line_begins_a_plan_where_the_point_intake_says_flow() {
    let f = fx_deciding(vec![doer(&[three()])], Some("intake=flow"), pinned);
    ended(&f, f.say("plan how to move the log to sqlite, then build it and check it"));
    assert_eq!(items(&f).len(), 3);
    assert_eq!(f.tasks()[0].spec.role.as_deref(), Some("plan"));
    // and a first line that is a task is a task
    let g = fx_deciding(vec![doer(&["done".into()])], Some("intake=task"), pinned);
    g.turn("add a --verbose flag");
    assert!(g.rig.of_kind("plan.asked").is_empty());
}

#[test]
fn the_planning_turn_runs_in_the_thread_session_read_only_and_a_bad_answer_leaves_the_old_plan() {
    let f = fx(vec![doer(&["hello".into(), three(), "I would do three things, in order.".into(), plan("x", &[("read", &[]), ("a", &["nope"])])])], pinned);
    let first = f.turn("hello");
    let t = ended(&f, f.say(":plan move the log"));
    let row = f.rig.tasks.book().get(&t).unwrap();
    assert_eq!((row.spec.role.as_deref(), row.spec.tune.mode), (Some("plan"), Mode::Read));
    assert_eq!(row.spec.follows.as_deref(), Some(first.id.as_str()), "the main lane's own session");
    assert!(row.spec.goal.contains("Answer with one JSON object"), "the model is told the shape and given no tool for it");
    let kept = f.keeper.plan("main").unwrap();
    // an answer that is not the shape
    ended(&f, f.say("also one more thing"));
    assert_eq!(f.keeper.plan("main").unwrap().items, kept.items);
    // and one whose reference is not an item
    ended(&f, f.say("also another thing"));
    assert_eq!(f.keeper.plan("main").unwrap().items, kept.items);
    let settled = kinds(&f, "plan.settled");
    assert_eq!(settled.iter().map(|s| s["ok"].as_bool().unwrap()).collect::<Vec<_>>(), [true, false, false]);
    assert!(settled[2]["why"].as_str().unwrap().contains("not an item"), "{settled:?}");
    assert!(f.notes().iter().any(|n| n.contains("the plan stands as it was")));
}

#[test]
fn a_restart_finds_the_plan_the_states_and_the_next_item_as_they_were_and_runs_nothing_merged_again() {
    let f = fx(vec![doer(&["hello".into(), three(), "read it".into()]), ], pinned);
    f.turn("hello");
    ended(&f, f.say(":plan move the log"));
    let done = f.turn("go on");
    f.front.plan_collect("main");
    assert_eq!(done.state, State::Done { ok: true });
    assert_eq!(items(&f)[0], ("read".into(), "merged".into()));
    let now = f.keeper.plan("main").unwrap();
    assert_eq!(now.current().map(|i| i.id.as_str()), Some("change"));
    // a fold started again over the same Log is the same plan
    let again = Keeper::new();
    let mut again_fold = again.clone();
    for ev in f.rig.events() {
        if again_fold.wants(&ev.kind) {
            again_fold.apply(&ev);
        }
    }
    assert_eq!(again.plan("main"), Some(now.clone()));
    // and so is one loaded from its own snapshot
    let mut loaded = Keeper::new();
    loaded.load(again.save()).unwrap();
    assert_eq!(loaded.plan("main"), Some(now));
}

#[test]
fn an_item_whose_turn_was_stopped_is_the_next_one_again() {
    let hold = Scripted::new("claude", vec![
        vec![Beat::Session("s".into()), end("hello")],
        vec![Beat::Session("s".into()), end(&three())],
        vec![Beat::Session("s".into()), Beat::Forever],
    ]);
    let f = fx(vec![hold], pinned);
    f.turn("hello");
    ended(&f, f.say(":plan move the log"));
    let Said::Started { task } = f.say("go on") else { panic!() };
    f.rig.until(&task, "ran", |r| matches!(r.state, State::Running));
    assert_eq!(items(&f)[0].1, "running");
    // a line typed while the item works steers it: it is not folded into the plan
    let folded = f.rig.of_kind("plan.folded").len();
    assert!(matches!(f.say("also mind the tests"), Said::Steered { .. }));
    assert_eq!(f.rig.of_kind("plan.folded").len(), folded);
    f.say(":stop");
    f.rig.finished(&task);
    f.front.plan_collect("main");
    let p = f.keeper.plan("main").unwrap();
    assert_eq!((p.items[0].state.as_str(), p.current().map(|i| i.id.as_str())), ("todo", Some("read")));
    assert!(p.items[0].result.contains("did not finish"), "{:?}", p.items[0].result);
}

#[test]
fn the_brief_has_one_plan_line_of_at_most_sixty_tokens_while_a_plan_is_open_and_none_otherwise() {
    let long = "Read the code before anything is changed, then change it in small steps, then run the checks. ".repeat(6);
    let answers = ["hello".to_string(), "again".into(), plan(long.trim(), &[("read", &[]), ("change", &["read"]), ("check", &["change"])]), "read it".into()];
    let turns = answers.iter().map(|a| vec![Beat::Session("s".into()), end(a)]).collect();
    let f = fx(vec![Scripted::with_caps("claude", Caps { brief: true, resume: true, ..Caps::default() }, turns)], pinned);
    f.turn("hello");
    f.turn("again");
    ended(&f, f.say(":plan move the log"));
    f.turn("go on");
    let briefs = kinds(&f, "brief.rendered");
    let has = |i: usize| briefs[i]["sections"].as_array().unwrap().iter().any(|s| s == "plan");
    assert!(!has(0) && !has(1), "no plan, no section: {briefs:?}");
    assert!(has(3), "{briefs:?}");
    assert!(!has(2), "the planning turn is given the plan in its words, not a line");
    let line = f.keeper.plan("main").unwrap().line().unwrap();
    assert!(j4n_core::brief::tokens(&line) <= 60, "{} tokens", j4n_core::brief::tokens(&line));
    assert!(line.starts_with("now read: do read"), "{line}");
    // a plan that is done has no line
    let mut done = f.keeper.plan("main").unwrap();
    done.items.iter_mut().for_each(|i| i.state = "merged".into());
    assert_eq!(done.line(), None);
}

#[test]
fn what_the_operator_edits_wins_over_the_planners_next_answer() {
    let again = plan("Read first, then change, then check, and log it.", &[("read", &[]), ("change", &["read"]), ("check", &["change"]), ("log", &["check"])]).replace("do change", "do a different change").replace("do check", "do check again");
    let f = fx(vec![doer(&["hello".into(), three(), again])], pinned);
    f.turn("hello");
    ended(&f, f.say(":plan move the log"));
    // the operator rewrites one item and drops another
    f.say(":plan set change goal change main.rs only");
    f.say(":plan drop check");
    let edited = f.keeper.plan("main").unwrap();
    assert_eq!(edited.items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["read", "change"]);
    assert_eq!(edited.items[1].goal, "change main.rs only");
    assert!(edited.items[1].mine && edited.gone.contains("check"));
    assert!(f.rig.of_kind("plan.item").iter().any(|e| e.by == j4n_core::change::Actor::Operator && e.data["op"] == "drop"));
    // the planner answers with the whole plan again: its change and the item it was told is gone are not taken
    ended(&f, f.say("also log it"));
    let after = f.keeper.plan("main").unwrap();
    let ids: Vec<&str> = after.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["read", "change", "log"], "check does not come back");
    assert_eq!(after.items[1].goal, "change main.rs only");
    assert!(after.items[2].depends.is_empty(), "a reference to an item the operator dropped is not carried: {:?}", after.items[2].depends);
}

#[test]
fn the_tasks_page_rows_are_a_pipe_tree_with_the_plan_on_top() {
    let f = fx(vec![doer(&["hello".into(), three()])], pinned);
    f.turn("hello");
    assert!(f.front.plan_rows("main").is_empty());
    ended(&f, f.say(":plan move the log"));
    let rows = f.front.plan_rows("main");
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].key, "plan:main");
    assert!(rows[1..].iter().all(|r| r.parent.as_deref() == Some("plan:main") && r.actions.len() == 2));
    assert_eq!(rows[3].cells[0].text, "└ check");
    assert_eq!(rows[1].cells[0].text, "├ read");
    // the planning turn is counted on the plan
    assert!(f.keeper.plan("main").unwrap().turns == 1);
}

// ---- the shape check, on its own ----

fn old() -> Plan {
    let mut k = Keeper::new();
    let ops = take(&three(), None, How::Begin, "go").unwrap();
    for (n, op) in ops.iter().enumerate() {
        let (kind, data) = op.event("t1");
        let ev = j4n_core::log::Ev { seq: n as u64 + 1, at_ms: 0, kind: kind.into(), task: None, thread: Some("main".into()), by: j4n_core::change::Actor::Core, cause: None, class: Default::default(), data };
        k.apply(&ev);
    }
    k.plan("main").unwrap()
}

#[test]
fn the_planners_answer_is_a_closed_shape_and_every_reference_is_checked() {
    let bad = |text: &str, how: How, want: &str| {
        let got = take(text, Some(&old()), how, "x").expect_err(text);
        assert!(got.contains(want), "{text}: {got}");
    };
    let ok = take(&three(), None, How::Begin, "go").unwrap();
    assert!(matches!(&ok[0], Op::Approach { new: true, .. }) && ok.len() == 4);
    bad("nothing here", How::Add, "no object");
    bad("{\"approach\": \"a\", \"items\": [], \"extra\": 1}", How::Add, "not the shape");
    bad(&plan("a", &[("a", &[]), ("a", &[]), ("b", &[])]), How::Begin, "not an id of its own");
    bad(&plan("A B", &[("Up", &[]), ("b", &[]), ("c", &[])]), How::Begin, "not an id of its own");
    bad(&plan("a", &[("a", &[]), ("b", &["zz"]), ("c", &[])]), How::Begin, "not an item");
    bad(&plan("a", &[("a", &["a"]), ("b", &[]), ("c", &[])]), How::Begin, "depends on itself");
    bad(&plan("a", &[("a", &["c"]), ("b", &["a"]), ("c", &["b"])]), How::Begin, "through the others");
    bad(&plan("a", &[]), How::Begin, "1 to 24 items");
    bad(&plan(" ", &[("a", &[]), ("b", &[]), ("c", &[])]), How::Begin, "one paragraph");
    bad(&three().replace("src/read.rs", "../etc/passwd"), How::Begin, "not a path inside the project");
    bad(&three().replace("src/read.rs", "/etc/passwd"), How::Begin, "not a path inside the project");
    bad(&three().replace("do read", " "), How::Begin, "one line");
    // two items are a plan, begun or changed
    assert!(take(&plan("a", &[("a", &[]), ("b", &[])]), None, How::Begin, "x").is_ok());
    assert!(take(&plan("a", &[("read", &[]), ("change", &["read"]), ("check", &["change"])]), Some(&old()), How::Change, "x").is_ok());
}

#[test]
fn a_long_or_broken_approach_is_cut_to_one_paragraph_and_the_plan_is_still_taken() {
    let approach_of = |text: &str| match take(&plan(text, &[("a", &[]), ("b", &[]), ("c", &[])]), None, How::Begin, "go").unwrap().remove(0) {
        Op::Approach { text, .. } => text,
        other => panic!("{other:?}"),
    };
    assert_eq!(approach_of("one\n\ntwo"), "one two");
    // the goal and the done-when of an item, and the why of a replan, are cut the same way
    let long = plan("a", &[("a", &[]), ("b", &[]), ("c", &[])]).replace("do a", &format!("line one\\nline two {}", "z".repeat(300)));
    let ops = format!("{:?}", take(&long, None, How::Begin, "go").unwrap());
    assert!(ops.contains("line one line two z") && !ops.contains(&"z".repeat(161)) && ops.contains(&"z".repeat(140)), "{ops}");
    let long = format!("{} Last words that run on past the limit {}", "A sentence of some length.".to_string() + &" Another sentence of length.".repeat(21), "x".repeat(100));
    let cut = approach_of(&long);
    assert!(cut.chars().count() <= 600 && cut.ends_with('.') && !cut.contains("Last words"), "{cut}");
    assert_eq!(approach_of(&"y".repeat(700)).chars().count(), 600);
}

#[test]
fn an_item_that_was_started_is_kept_as_it_is_and_cannot_be_left_out() {
    let mut p = old();
    p.items[0].state = "merged".into();
    let reworded = three().replace("do read", "do something else");
    assert!(take(&reworded, Some(&p), How::Change, "x").unwrap_err().contains("kept as it is"));
    let without = plan("a", &[("change", &[]), ("check", &["change"])]);
    assert!(take(&without, Some(&p), How::Change, "x").unwrap_err().contains("it is kept"));
    // an item that waits and is left out is dropped
    let ops = take(&plan("a", &[("read", &[]), ("change", &["read"])]), Some(&old()), How::Change, "x").unwrap();
    assert!(ops.contains(&Op::Drop("check".into())));
}

#[test]
fn rule_fold_takes_the_cheap_cases_and_leaves_the_rest() {
    let ids = ["read".to_string(), "change".to_string()];
    let table = [
        ("also add a changelog line", Some("add")),
        ("add a test for it as well", Some("add")),
        ("instead use sqlite", Some("change")),
        ("stop the change step", Some("change")),
        ("make read look at the tests too", Some("add")),
        ("rewrite change so it does less", Some("change")),
        ("what does the change step do?", Some("answer")),
        ("continue", Some("answer")),
        ("go on", Some("answer")),
        ("start over", Some("replace")),
        ("scrap the plan and plan this: move the log", Some("replace")),
        ("also, what is the plan?", Some("add")),
        ("the next version should log more", None),
        ("add a flag for it", None),
        ("rename the module", None),
    ];
    for (line, want) in table {
        assert_eq!(fold_pick(line, &ids).map(|p| p.0), want, "{line}");
    }
    // the rule reads the plan of the question's thread for the names of its items
    let keeper = Keeper::new();
    let rule = j4n_core::keeper::fold_rule(keeper);
    let q = Question { ask: "a".into(), point: "fold".into(), asker: "front".into(), thread: Some("main".into()), text: "also x".into(), shape: AskShape::Choice { options: vec![] }, goal: "x".into(), act: None };
    assert!(matches!(rule.offer(&q), Offer::Answer { answer: Answer::Pick { id }, .. } if id == "add"));
}

#[test]
fn the_current_item_is_the_one_running_else_the_first_whose_dependencies_are_merged() {
    let item = |id: &str, state: &str, deps: &[&str]| j4n_core::keeper::Item { id: id.into(), state: state.into(), goal: id.into(), depends: deps.iter().map(|d| d.to_string()).collect(), ..Default::default() };
    let mut p = Plan { items: vec![item("a", "todo", &["b"]), item("b", "todo", &[]), item("c", "todo", &[])], approach: "x".into(), ..Plan::default() };
    assert_eq!(p.current().map(|i| i.id.as_str()), Some("b"), "a waits for b");
    p.items[1].state = "merged".into();
    assert_eq!(p.current().map(|i| i.id.as_str()), Some("a"));
    p.items[2].state = "running".into();
    assert_eq!(p.current().map(|i| i.id.as_str()), Some("c"), "what runs is current");
    p.items.iter_mut().for_each(|i| i.state = "merged".into());
    assert!(p.current().is_none() && !p.open());
}

/// The `fold` ask put to a decider said what the line was and nothing of what is being done (its goal was the line), so a decider that
/// passed the probe cases, which carry `goal = "the plan: …"`, was not told the plan live. The goal is the plan's approach and items.
#[test]
fn the_fold_question_a_decider_is_asked_carries_the_plan_as_its_goal() {
    let more = plan("Read first, then change, then check, and rename.", &[("read", &[]), ("change", &["read"]), ("check", &["change"]), ("rename", &["check"])]);
    let f = fx_deciding(vec![doer(&["hello".into(), three(), more])], Some("fold=add"), pinned);
    f.turn("hello");
    ended(&f, f.say(":plan move the log to sqlite"));
    // no cheap rule takes this line, so the chain reaches the decider
    ended(&f, f.say("rename the module"));
    let asked: Vec<Question> = f.asked.lock().unwrap().iter().filter(|q| q.point == "fold").cloned().collect();
    assert_eq!(asked.len(), 1, "the decider was asked `fold` once");
    let q = &asked[0];
    assert_eq!(q.text, "rename the module");
    assert!(q.goal.starts_with("the plan: Read first, then change, then check."), "{}", q.goal);
    for item in ["read [todo] do read", "change [todo] do change", "check [todo] do check"] {
        assert!(q.goal.contains(item), "the goal names `{item}`: {}", q.goal);
    }
    assert_ne!(q.goal, q.text, "the goal is not the line");
}

/// The goal a decider is asked with was nowhere in the Log (`ask.egress` carried the ask, the answerer, the zone and the class), so a live
/// run could not show that the plan reaches the model. The row now carries the goal, clipped, when it is not the question's own words.
#[test]
fn the_egress_row_of_the_fold_question_carries_the_plan_and_one_whose_goal_is_the_line_carries_none() {
    let more = plan("Read first, then change, then check, and rename.", &[("read", &[]), ("change", &["read"]), ("check", &["change"]), ("rename", &["check"])]);
    let f = fx_deciding(vec![doer(&["hello".into(), three(), more])], Some("fold=add"), pinned);
    *f.zone.lock().unwrap() = Some("local".into());
    f.turn("hello");
    ended(&f, f.say(":plan move the log to sqlite"));
    ended(&f, f.say("rename the module"));
    let asked: Vec<Question> = f.asked.lock().unwrap().iter().cloned().collect();
    let fold = asked.iter().find(|q| q.point == "fold").expect("the decider was asked `fold`");
    let egress = f.rig.of_kind("ask.egress");
    let row = egress.iter().find(|e| e.data["ask"] == fold.ask.as_str()).unwrap_or_else(|| panic!("no egress row for {}: {}", fold.ask, f.rig.dump()));
    let goal = row.data["goal"].as_str().unwrap_or_else(|| panic!("the row names no goal: {:?}", row.data));
    assert!(goal.starts_with("the plan: Read first, then change, then check."), "{goal}");
    assert!(goal.contains("read [todo] do read") && goal.chars().count() <= 400 && fold.goal.starts_with(goal), "{goal}");
    // a question whose goal is its own words is not said twice
    for q in asked.iter().filter(|q| q.goal == q.text) {
        let row = egress.iter().find(|e| e.data["ask"] == q.ask.as_str()).unwrap();
        assert!(row.data.get("goal").is_none(), "{:?}", row.data);
    }
    assert!(asked.iter().any(|q| q.goal == q.text), "the first line was asked with its own words, so this saw one");
}

/// The Log redacts a line before it is written, the goal's too.
#[test]
fn a_secret_in_the_goal_is_redacted_in_the_egress_row_like_any_log_text() {
    let secret = "hunter2-hunter2";
    let first = json!({ "approach": "Use the token.", "items": [{ "id": "read", "goal": format!("read with {secret}"), "reads": ["a"], "writes": ["a"], "depends": [], "done": "x" }] }).to_string();
    let f = fx_deciding(vec![doer(&["hello".into(), first])], Some("fold=add"), pinned);
    assert!(f.rig.log.redactor().register("TOKEN", secret));
    *f.zone.lock().unwrap() = Some("local".into());
    f.turn("hello");
    ended(&f, f.say(":plan use the token"));
    ended(&f, f.say("rename the module"));
    let rows: Vec<String> = f.rig.of_kind("ask.egress").into_iter().filter_map(|e| e.data["goal"].as_str().map(String::from)).collect();
    assert!(rows.iter().any(|g| g.starts_with("the plan:")), "{rows:?}");
    assert!(rows.iter().all(|g| !g.contains(secret)) && rows.iter().any(|g| g.contains("«secret:TOKEN»")), "{rows:?}");
}
