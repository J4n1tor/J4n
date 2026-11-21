//! Through the real front and scripted doers: the items of an open plan
//! run inline or as branches of a class, the stay-inline tests win over the planner's mark and the Log names the test, a branch is
//! linked to its item and given a `brief`, `clean` or `verbatim` context, and a branch starts no branch. `branch::decide` is checked
//! on its own as well.

use crate::front_cases::{Fx, chatty, fx, fx_deciding, fx_headless};
use crate::scripted::{Beat, Rig, Scripted, end, say};
use j4n_core::branch::{Class, Context, Ctx, Rules, Stay, Switch, Verdict, decide, estimate, via};
use j4n_core::change::Actor;
use j4n_core::doer::Caps;
use j4n_core::front::{Cfg, Said};
use j4n_core::keeper::{Item, Plan};
use j4n_core::task::{Mode, State, TaskSpec};
use serde_json::{Value, json};
use std::collections::BTreeSet;

const BIG: [&str; 3] = ["src/a/1.rs", "src/a/2.rs", "src/a/3.rs"];

fn item(id: &str, place: &str, reads: &[&str], writes: &[&str], deps: &[&str]) -> Value {
    json!({ "id": id, "goal": format!("do {id}"), "reads": reads, "writes": writes, "depends": deps, "done": format!("{id} works"), "where": place })
}

fn planned(items: Vec<Value>) -> String {
    json!({ "approach": "Look, change, check.", "items": items }).to_string()
}

fn auto(c: &mut Cfg) {
    c.doer = Some("claude".into());
    c.lanes = Switch::Auto;
}

/// A doer that takes a Brief, answers each turn with the next text and keeps a session.
fn branching(id: &str, answers: &[&str]) -> std::sync::Arc<Scripted> {
    let turns = answers.iter().map(|a| vec![Beat::Session(format!("{id}-s")), say(a), end(a)]).collect();
    Scripted::with_caps(id, Caps { live: true, resume: true, brief: true, ..Caps::default() }, turns)
}

fn main_lane(answers: &[String]) -> std::sync::Arc<Scripted> {
    let refs: Vec<&str> = answers.iter().map(String::as_str).collect();
    chatty("claude", &refs)
}

/// Every task is over, then the plan takes what came of them and starts what is next.
fn step(f: &Fx) {
    for r in f.tasks() {
        f.rig.finished(&r.id);
    }
    f.front.plan_step("main");
}

fn begin(f: &Fx) {
    f.turn("hello");
    match f.say(":plan do these: 1. look 2. look again 3. look more 4. change 5. change more 6. check 7. check again") {
        Said::Started { task } => {
            f.rig.finished(&task);
        }
        other => panic!("{other:?}\n{}", f.rig.dump()),
    }
    f.front.plan_collect("main");
}

fn routed(f: &Fx) -> Vec<Value> {
    f.rig.of_kind("plan.routed").into_iter().map(|e| e.data).collect()
}

fn states(f: &Fx) -> Vec<(String, String, String)> {
    f.keeper.plan("main").map(|p| p.items.into_iter().map(|i| (i.id, i.state, i.place)).collect()).unwrap_or_default()
}

fn branches(f: &Fx) -> Vec<j4n_core::task::Row> {
    f.tasks().into_iter().filter(|r| r.spec.input.get("branch").is_some()).collect()
}

// ---- the decision on its own ----

fn one(id: &str, mark: &str, reads: &[&str], writes: &[&str]) -> Item {
    Item { id: id.into(), goal: format!("do {id} in a way that reads and writes"), done: format!("{id} works"), mark: mark.into(), state: "todo".into(), place: "inline".into(), reads: reads.iter().map(|s| s.to_string()).collect(), writes: writes.iter().map(|s| s.to_string()).collect(), ..Item::default() }
}

fn of(items: Vec<Item>) -> Plan {
    Plan { items, approach: "x".into(), ..Plan::default() }
}

fn ctx(switch: Switch, written: &[&str]) -> Ctx {
    Ctx { switch, rules: Default::default(), written: written.iter().map(|s| s.to_string()).collect::<BTreeSet<_>>(), asked: false }
}

#[test]
fn the_stay_inline_tests_win_over_the_mark_in_the_order_main_coupled_cheap() {
    let scout = one("s", "scout", &BIG, &[]);
    let plan = of(vec![scout.clone()]);
    assert!(estimate(&scout) >= j4n_core::branch::START_TOKENS);
    assert_eq!(decide(&scout, &plan, &ctx(Switch::Auto, &[])), Verdict::Branch { class: Class::Scout, why: decide(&scout, &plan, &ctx(Switch::Auto, &[])).why().into() });
    // the mark says inline: it stays, and no test is the reason
    let inline = one("i", "inline", &BIG, &[]);
    assert_eq!(decide(&inline, &of(vec![inline.clone()]), &ctx(Switch::Auto, &[])).test(), None);
    // the item reads what this line wrote
    assert_eq!(decide(&scout, &plan, &ctx(Switch::Auto, &["src/a/2.rs"])).test(), Some(Stay::Coupled));
    // a review is of the work this line did: that is not a coupling
    let review = one("v", "review", &BIG, &[]);
    assert!(matches!(decide(&review, &of(vec![review.clone()]), &ctx(Switch::Auto, &["src/a/2.rs"])), Verdict::Branch { class: Class::Review, .. }));
    // cheaper than a branch
    let small = one("t", "scout", &[], &[]);
    let small = Item { goal: "x".into(), done: "y".into(), ..small };
    assert_eq!(decide(&small, &of(vec![small.clone()]), &ctx(Switch::Auto, &[])).test(), Some(Stay::Cheap));
    // needs the main lane: the plan's mark, or an Ask that is open
    let main = Item { main: true, ..scout.clone() };
    assert_eq!(decide(&main, &of(vec![main.clone()]), &ctx(Switch::Auto, &["src/a/2.rs"])).test(), Some(Stay::Main), "main is tried before the others");
    let asked = Ctx { asked: true, ..ctx(Switch::Auto, &[]) };
    assert_eq!(decide(&scout, &plan, &asked).test(), Some(Stay::Main));
    // single branches nothing
    assert_eq!(decide(&scout, &plan, &ctx(Switch::Single, &[])), Verdict::Inline { test: None, why: "lanes = single: nothing branches".into() });
    // a read-only class does not take an item that writes
    let writes = one("w", "scout", &BIG, &["src/out.rs"]);
    assert!(matches!(decide(&writes, &of(vec![writes.clone()]), &ctx(Switch::Auto, &[])), Verdict::Inline { test: None, .. }));
}

#[test]
fn a_piece_runs_only_under_wide_and_only_apart_from_every_other_item_that_is_not_done() {
    let a = one("a", "piece", &["src/a.rs", "src/b.rs", "src/c.rs"], &["src/a.rs"]);
    let b = one("b", "piece", &["src/x.rs", "src/y.rs", "src/z.rs"], &["src/x.rs"]);
    let plan = of(vec![a.clone(), b.clone()]);
    assert!(matches!(decide(&a, &plan, &ctx(Switch::Auto, &[])), Verdict::Inline { test: None, .. }), "auto starts no piece");
    assert!(matches!(decide(&a, &plan, &ctx(Switch::Wide, &[])), Verdict::Branch { class: Class::Piece, .. }));
    // the second one reads what the first writes: not disjoint, so it stays with the main lane
    let b2 = Item { reads: vec!["src/a.rs".into(), "src/y.rs".into(), "src/z.rs".into()], ..b.clone() };
    let plan = of(vec![a.clone(), b2.clone()]);
    let d = decide(&a, &plan, &ctx(Switch::Wide, &[]));
    assert!(matches!(d, Verdict::Inline { test: None, .. }) && d.why().contains("not disjoint"), "{d:?}");
    // an item that is done is no clash
    let done = Item { state: "merged".into(), ..b2 };
    assert!(matches!(decide(&a, &of(vec![a.clone(), done]), &ctx(Switch::Wide, &[])), Verdict::Branch { .. }));
    // an item that waits for it, as a closing review does, runs after it: no clash, however many items lie between
    let review = Item { depends: vec!["a".into()], ..one("v", "review", &["src/a.rs", "src/x.rs", "src/y.rs"], &[]) };
    let last = Item { depends: vec!["v".into()], ..one("l", "scout", &["src/a.rs", "src/b.rs"], &[]) };
    assert!(matches!(decide(&a, &of(vec![a.clone(), review.clone(), last.clone()]), &ctx(Switch::Wide, &[])), Verdict::Branch { class: Class::Piece, .. }), "the dependents are after it");
    // a review that does not wait for it reads the same files at the same time
    let alone = Item { depends: vec![], ..review };
    assert!(decide(&a, &of(vec![a.clone(), alone]), &ctx(Switch::Wide, &[])).why().contains("not disjoint"));
    // no paths to write: nothing says it is disjoint
    let none = one("n", "piece", &BIG, &[]);
    assert!(matches!(decide(&none, &of(vec![none.clone()]), &ctx(Switch::Wide, &[])), Verdict::Inline { .. }));
}

#[test]
fn the_log_names_the_test_that_kept_an_item_inline_and_a_branch_mark_is_overridden() {
    let answers = ["hello".to_string(), planned(vec![
        item("w", "inline", &[], &["src/w.rs"], &[]),
        item("s", "scout", &BIG, &[], &[]),
        item("r", "scout", &["src/w.rs", "src/a/2.rs", "src/a/3.rs"], &[], &["w"]),
        item("c", "scout", &[], &[], &[]),
        item("m", "review", &BIG, &[], &[]),
    ])]
    .into_iter()
    .chain(["w done", "r done", "c done", "m done"].map(String::from))
    .collect::<Vec<_>>();
    let f = fx_deciding(vec![main_lane(&answers), branching("pi", &["looked"])], Some("pi"), auto);
    begin(&f);
    // an `m` that needs the main lane: set by the operator, it wins over the planner as every edit does
    f.say(":plan set m main yes");
    assert_eq!(states(&f).iter().map(|(_, s, _)| s.as_str()).collect::<Vec<_>>(), ["todo"; 5]);
    f.front.plan_step("main");
    for _ in 0..4 {
        step(&f);
    }
    let seen: Vec<(String, String, Option<String>)> = routed(&f).iter().map(|d| (d["id"].as_str().unwrap().into(), d["place"].as_str().unwrap().into(), d["test"].as_str().map(String::from))).collect();
    assert_eq!(
        seen,
        [
            ("w".into(), "inline".into(), None),
            ("s".into(), "scout".into(), None),
            ("r".into(), "inline".into(), Some("coupled".into())),
            ("c".into(), "inline".into(), Some("cheap".into())),
            ("m".into(), "inline".into(), Some("main".into())),
        ],
        "{}",
        f.rig.dump()
    );
    let why = routed(&f)[2]["why"].as_str().unwrap().to_string();
    assert!(why.contains("src/w.rs"), "the Log says what it read: {why}");
    let p = f.keeper.plan("main").unwrap();
    // a scout has nothing to merge: it ends at `reviewed`; an inline turn is the main lane writing the folder itself
    assert!(p.items.iter().all(|i| i.done()), "{:?}\n{}", states(&f), f.rig.dump());
    assert_eq!(p.items[1].state, "reviewed");
    assert_eq!(p.items[2].mark, "scout", "the planner's mark is kept beside what was decided");
    assert_eq!(p.items[2].place, "inline");
}


#[test]
fn a_scout_and_a_review_may_take_what_a_claude_session_costs() {
    // a claude scout on haiku can cost $0.107 to $0.159 (list price, as the claude doer reports it), and a cap of
    // $0.05 would kill them all at the hard line; a branch of the class must be able to take the dearest of them without a word
    const DEAREST_SCOUT: f64 = 0.159;
    let rules = Rules::builtin();
    for class in [Class::Scout, Class::Review] {
        let usd = rules.limits(class).0.unwrap();
        assert!(usd >= DEAREST_SCOUT, "{}: ${usd} is under what a claude session takes", class.word());
    }
}


#[test]
fn independent_reads_start_scouts_by_plan_each_on_an_elected_doer_with_a_brief_and_the_binding_stays() {
    let answers = ["hello".to_string(), planned(vec![
        item("a", "scout", &BIG, &[], &[]),
        item("b", "scout", &["src/b/1.rs", "src/b/2.rs", "src/b/3.rs"], &[], &[]),
        item("go", "inline", &["src/a/1.rs"], &["src/out.rs"], &["a", "b"]),
    ]), "went on".to_string()];
    let pi = branching("pi", &["the retry is in fetch()", "nothing there"]);
    let f = fx_deciding(vec![main_lane(&answers), pi.clone()], Some("pi"), auto);
    begin(&f);
    f.front.plan_step("main");
    let got = branches(&f);
    assert_eq!(got.len(), 2, "{}", f.rig.dump());
    let main = f.tasks().into_iter().find(|r| r.spec.role.as_deref() == Some("plan")).unwrap();
    for (r, id) in got.iter().zip(["a", "b"]) {
        assert_eq!((r.spec.doer.as_str(), r.spec.role.as_deref(), r.spec.tune.mode), ("pi", Some("scout"), Mode::Read));
        assert_eq!(r.spec.parent.as_deref(), Some(main.id.as_str()), "a branch of the line the main lane is on");
        assert_eq!(r.spec.input["item"], id, "linked back to its item");
        assert_eq!((r.spec.input["branch"]["class"].as_str(), r.spec.input["branch"]["context"].as_str(), r.spec.input["branch"]["via"].as_str()), (Some("scout"), Some("brief"), Some("delegate")));
        assert!(r.spec.input["brief"].as_str().unwrap().contains("only reads"), "the contract of a read task");
        assert!(r.spec.limits.usd_soft.is_some_and(|u| u == Rules::builtin().limits(Class::Scout).0.unwrap()) && r.spec.limits.wall_ms.is_some_and(|w| w <= 120_000), "the limits of the class");
    }
    let p = f.keeper.plan("main").unwrap();
    assert_eq!((p.items[0].state.as_str(), p.items[0].task.as_str()), ("running", got[0].id.as_str()), "todo goes to running, and the item knows its branch");
    assert_eq!(p.items[1].task, got[1].id);
    assert_eq!(p.items[2].state, "todo", "it waits for both");
    assert_eq!(f.bound().len(), 1, "the thread's binding is not changed by a branch");
    // they end; each reports to the main lane once, and the item that waited for them goes
    step(&f);
    let p = f.keeper.plan("main").unwrap();
    assert_eq!((p.items[0].state.as_str(), p.items[1].state.as_str(), p.items[2].state.as_str()), ("reviewed", "reviewed", "running"), "{}", f.rig.dump());
    assert_eq!(f.rig.of_kind("report.made").len(), 2);
    let rows = f.front.lane_rows("main");
    assert!(rows.iter().any(|r| r.cells.iter().any(|c| c.text.contains("scout"))), "the pipe tree shows the class");
    let told = f.tasks().into_iter().rev().find(|r| r.spec.doer == "claude").unwrap();
    assert!(told.spec.input["brief"].as_str().is_none_or(|b| b.contains("branches") || b.contains("## ")), "the main lane's turn");
}

#[test]
fn a_scout_on_the_main_lanes_own_doer_keeps_the_threads_pinned_model() {
    let answers = ["hello".to_string(), planned(vec![
        item("a", "scout", &BIG, &[], &[]),
        item("b", "scout", &["src/b/1.rs", "src/b/2.rs", "src/b/3.rs"], &[], &[]),
        item("go", "inline", &["src/a/1.rs"], &["src/out.rs"], &["a", "b"]),
    ]), "went on".to_string()];
    let f = fx_deciding(vec![main_lane(&answers)], Some("claude"), |c| {
        auto(c);
        c.model = Some("haiku".into());
    });
    begin(&f);
    f.front.plan_step("main");
    let got = branches(&f);
    assert_eq!(got.len(), 2, "{}", f.rig.dump());
    for r in &got {
        assert_eq!((r.spec.doer.as_str(), r.spec.tune.model.as_deref()), ("claude", Some("haiku")));
    }
}

/// A run that named its doer has no operator to ask; the decider here would pick `pi` for every branch.
fn pinned_plan(a_goal: &str) -> Vec<String> {
    let mut a = item("a", "scout", &BIG, &[], &[]);
    a["goal"] = json!(a_goal);
    vec!["hello".to_string(), planned(vec![
        a,
        item("b", "scout", &["src/b/1.rs", "src/b/2.rs", "src/b/3.rs"], &[], &[]),
        item("go", "inline", &["src/a/1.rs"], &["src/out.rs"], &["a", "b"]),
    ])]
}

#[test]
fn in_a_run_the_items_of_a_plan_keep_the_runs_doer_and_not_the_one_a_ranking_would_pick() {
    let f = fx_headless(vec![main_lane(&pinned_plan("do a")), branching("pi", &["x", "y"])], Some("pi"), auto);
    begin(&f);
    f.front.plan_step("main");
    let got = branches(&f);
    assert_eq!(got.len(), 2, "{}", f.rig.dump());
    assert!(got.iter().all(|r| r.spec.doer == "claude"), "{:?}", got.iter().map(|r| r.spec.doer.clone()).collect::<Vec<_>>());
    assert!(routed(&f).iter().skip(1).all(|r| r["doer"] == "claude"), "{:?}", routed(&f));
}

#[test]
fn an_item_whose_own_words_name_another_doer_goes_to_it() {
    let f = fx_headless(vec![main_lane(&pinned_plan("@pi to read the a files")), branching("pi", &["x"])], Some("pi"), auto);
    begin(&f);
    f.front.plan_step("main");
    let by: Vec<(String, String)> = branches(&f).into_iter().map(|r| (r.spec.input["item"].as_str().unwrap().to_string(), r.spec.doer)).collect();
    assert_eq!(by, [("a".to_string(), "pi".to_string()), ("b".to_string(), "claude".to_string())], "{}", f.rig.dump());
}

#[test]
fn no_more_than_the_map_limit_read_at_once_and_a_rest_waits() {
    let many: Vec<Value> = (0..6).map(|n| item(&format!("s{n}"), "scout", &BIG, &[], &[])).chain([item("end", "inline", &[], &["src/o.rs"], &["s0"])]).collect();
    let answers = ["hello".to_string(), planned(many)];
    let pi = branching("pi", &["ok"]);
    let f = fx_deciding(vec![main_lane(&answers), pi], Some("pi"), auto);
    begin(&f);
    f.front.plan_step("main");
    assert_eq!(branches(&f).len(), j4n_core::flow::run::MAP_AT_ONCE, "four at once");
    step(&f);
    assert_eq!(branches(&f).len(), 6, "the other two once there is room");
}


#[test]
fn a_review_is_given_a_clean_brief_on_another_doer_and_the_doer_decides_native_or_delegate() {
    let answers = ["hello".to_string(), planned(vec![
        item("work", "inline", &BIG, &["src/w.rs"], &[]),
        item("check", "review", &["src/w.rs", "src/a/1.rs", "src/a/2.rs"], &[], &["work"]),
        item("end", "inline", &[], &["src/e.rs"], &["check"]),
    ]), "worked".to_string(), "ended".to_string()];
    let own = branching("own", &["looks right"]);
    let f = fx_deciding(vec![main_lane(&answers), branching("pi", &["x"]), own.clone()], Some("own"), auto);
    begin(&f);
    f.front.plan_step("main");
    step(&f);
    let r = &branches(&f)[0];
    assert_eq!((r.spec.role.as_deref(), r.spec.doer.as_str(), r.spec.tune.mode), (Some("review"), "own", Mode::Read), "{}", f.rig.dump());
    assert_ne!(r.spec.doer, "claude", "not the doer of the work");
    assert_eq!(r.spec.input["branch"]["context"], "clean");
    assert_eq!(r.spec.input["branch"]["via"], "native", "a branch on the own agent");
    let brief = r.spec.input["brief"].as_str().unwrap();
    assert!(!brief.contains("## plan"), "a clean brief has nothing of the plan: {brief}");
    assert!(brief.contains("## changed") && brief.contains("src/w.rs"), "the goal and what the work changed: {brief}");
    let routed = routed(&f);
    assert_eq!((routed[1]["via"].as_str(), routed[1]["context"].as_str(), routed[1]["doer"].as_str()), (Some("native"), Some("clean"), Some("own")));
    // an external harness is a delegate
    let answers = ["hello".to_string(), planned(vec![item("look", "scout", &BIG, &[], &[]), item("two", "inline", &[], &["src/e.rs"], &["look"]), item("three", "inline", &[], &["src/f.rs"], &["two"])])];
    let g = fx_deciding(vec![main_lane(&answers), branching("pi", &["x"])], Some("pi"), auto);
    begin(&g);
    g.front.plan_step("main");
    assert_eq!(branches(&g)[0].spec.input["branch"]["via"], "delegate");
    assert_eq!(via("own", Context::Brief), "native");
    assert_eq!(via("claude", Context::Clean), "delegate");
    assert_eq!(via("claude", Context::Verbatim), "passthrough");
}

#[test]
fn a_verbatim_branch_gets_the_operators_words_and_no_brief_and_stays_under_the_gate_and_the_caps() {
    let words = "read every log under var/ and tell me which one lies";
    let answers = ["hello".to_string(), planned(vec![item("logs", "scout", &BIG, &[], &[]), item("two", "inline", &[], &["src/e.rs"], &["logs"]), item("three", "inline", &[], &["src/f.rs"], &["two"])])];
    let pi = branching("pi", &["the third one"]);
    let f = fx_deciding(vec![main_lane(&answers), pi.clone()], Some("pi"), auto);
    begin(&f);
    // only the operator sets it: the words are theirs
    f.say(&format!(":plan set logs goal {words}"));
    f.say(":plan set logs verbatim yes");
    f.front.plan_step("main");
    let r = &branches(&f)[0];
    assert_eq!(r.spec.goal, words, "the operator's words, with no `Done when`");
    assert!(r.spec.input.get("brief").is_none(), "no Brief");
    assert_eq!((r.spec.input["branch"]["context"].as_str(), r.spec.input["branch"]["via"].as_str()), (Some("verbatim"), Some("passthrough")));
    assert_eq!(r.spec.tune.mode, Mode::Read, "the mode of the class: the Gate still refuses a write");
    assert!(r.spec.limits.usd_soft.is_some() && r.spec.limits.wall_ms.is_some(), "the caps of the class stand");
    assert!(f.rig.of_kind("task.queued").iter().any(|e| e.task.as_deref() == Some(r.id.as_str())), "and the Log has it");
    // the planner cannot set it
    let cannot = j4n_core::keeper::take(&planned(vec![json!({ "id": "x", "goal": "g", "done": "d", "verbatim": true })]), None, j4n_core::keeper::How::Begin, "");
    assert!(cannot.is_err(), "{cannot:?}");
}

#[test]
fn the_planners_where_is_one_of_four_words() {
    for place in ["inline", "scout", "review", "piece"] {
        let ok = planned(vec![item("a", place, &[], &[], &[]), item("b", "inline", &[], &[], &[]), item("c", "inline", &[], &[], &[])]);
        assert!(j4n_core::keeper::take(&ok, None, j4n_core::keeper::How::Begin, "x").is_ok(), "{place}");
    }
    let bad = planned(vec![item("a", "chore", &[], &[], &[]), item("b", "inline", &[], &[], &[]), item("c", "inline", &[], &[], &[])]);
    let why = j4n_core::keeper::take(&bad, None, j4n_core::keeper::How::Begin, "x").unwrap_err();
    assert!(why.contains("not a place"), "{why}");
}


#[test]
fn a_branch_has_no_way_to_start_a_branch() {
    let answers = ["hello".to_string(), planned(vec![item("a", "scout", &BIG, &[], &[]), item("b", "inline", &[], &["src/b.rs"], &["a"]), item("c", "inline", &[], &["src/c.rs"], &["b"])])];
    let f = fx_deciding(vec![main_lane(&answers), branching("pi", &["ok"])], Some("pi"), auto);
    begin(&f);
    f.front.plan_step("main");
    let branch = branches(&f)[0].id.clone();
    let mut spec = TaskSpec::new("look further", "pi");
    spec.parent = Some(branch.clone());
    spec.thread = Some("main".into());
    spec.input = json!({ "branch": { "class": "scout" } });
    let why = f.rig.tasks.spawn(spec, Actor::Core).unwrap_err().msg;
    assert!(why.contains("depth one"), "{why}");
    let mut plain = TaskSpec::new("look further", "pi");
    plain.parent = Some(branch.clone());
    let why = f.rig.tasks.spawn(plain, Actor::Core).unwrap_err().msg;
    assert!(why.contains("depth one"), "{why}");
    // a question of its own is still its to ask
    assert_eq!(f.tasks().iter().filter(|r| r.spec.parent.as_deref() == Some(branch.as_str())).count(), 0);
}

#[test]
fn a_rule_that_would_carry_a_branch_on_is_refused() {
    let leaving = vec![Beat::Session("s".into()), Beat::End(Some("half".into()), 2)];
    let pi = Scripted::new("pi", vec![leaving, vec![end("again")]]);
    let boss = Scripted::new("boss", vec![vec![end("main")]]);
    let rig = Rig::based(vec![boss, pi.clone()], "", |_| {});
    let main = rig.spawn("main", "boss");
    rig.finished(&main);
    let mut spec = TaskSpec::new("look", "pi");
    spec.parent = Some(main.clone());
    spec.input = json!({ "branch": { "class": "scout" } });
    let branch = rig.tasks.spawn(spec, Actor::Core).expect("a branch may be a part of a line whose turn is over");
    rig.finished(&branch);
    let dropped = rig.until_event("the rule is refused", |e| e.kind == "rule.dropped" && e.task.as_deref() == Some(branch.as_str()));
    assert!(dropped.data["why"].as_str().unwrap().contains("depth one"), "{}", dropped.data);
    assert_eq!(pi.starts().len(), 1, "no second run of it");
    assert!(matches!(rig.tasks.book().get(&branch).unwrap().state, State::Done { ok: true }));
}


#[test]
fn with_lanes_single_no_item_starts_a_branch_whatever_its_mark_and_nothing_drives_itself() {
    let answers = ["hello".to_string(), planned(vec![item("a", "scout", &BIG, &[], &[]), item("b", "review", &BIG, &[], &["a"]), item("c", "piece", &BIG, &["src/c.rs"], &["b"])]), "a done".to_string()];
    let f = fx(vec![main_lane(&answers), branching("pi", &["ok"])], |c| {
        c.doer = Some("claude".into());
        c.lanes = Switch::Single;
    });
    begin(&f);
    let before = f.tasks().len();
    f.front.plan_step("main");
    assert_eq!(f.tasks().len(), before, "single: the operator's lines drive the plan");
    assert!(routed(&f).is_empty(), "and the Log has no routing");
    // the operator's line works the item inline, as before
    f.turn("go on");
    f.front.plan_collect("main");
    assert_eq!(states(&f)[0].1, "merged");
    assert!(branches(&f).is_empty() && routed(&f).is_empty());
}

#[test]
fn inline_items_run_one_at_a_time_in_plan_order_without_the_operator_typing() {
    let answers = ["hello".to_string(), planned(vec![item("one", "inline", &[], &["src/1.rs"], &[]), item("two", "inline", &[], &["src/2.rs"], &["one"]), item("three", "inline", &[], &["src/3.rs"], &["two"])]), "1".into(), "2".into(), "3".into()];
    let claude = main_lane(&answers);
    let f = fx(vec![claude.clone()], auto);
    begin(&f);
    for _ in 0..4 {
        f.front.plan_step("main");
        let running = f.keeper.plan("main").unwrap().items.into_iter().filter(|i| i.state == "running").count();
        assert!(running <= 1, "one at a time: {running}");
        step(&f);
    }
    let order: Vec<String> = routed(&f).iter().map(|d| d["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(order, ["one", "two", "three"]);
    assert!(f.keeper.plan("main").unwrap().items.iter().all(|i| i.state == "merged"), "{}", f.rig.dump());
    // and the turns carried on in the main lane's own session
    let turns: Vec<_> = f.tasks().into_iter().filter(|r| r.spec.doer == "claude").collect();
    assert!(turns[2..].iter().all(|r| r.spec.follows.is_some()));
}

#[test]
fn an_item_that_keeps_failing_is_blocked_after_three_starts_and_does_not_loop() {
    let boom = |s: &str| vec![Beat::Session("s".into()), Beat::Fail(s.into())];
    let claude = Scripted::new("claude", vec![
        vec![Beat::Session("s".into()), end("hello")],
        vec![Beat::Session("s".into()), end(&planned(vec![item("a", "inline", &[], &["src/a.rs"], &[]), item("b", "inline", &[], &["src/b.rs"], &["a"]), item("c", "inline", &[], &["src/c.rs"], &["b"])]))],
        boom("one"),
        boom("two"),
        boom("three"),
    ]);
    let f = fx(vec![claude], auto);
    begin(&f);
    for _ in 0..5 {
        step(&f);
    }
    let a = &f.keeper.plan("main").unwrap().items[0];
    assert_eq!((a.state.as_str(), a.tries), ("blocked", 3), "{}", f.rig.dump());
    assert_eq!(f.tasks().iter().filter(|r| r.spec.goal.starts_with("do a")).count(), 3);
}

#[test]
fn a_branch_that_failed_is_back_to_todo_and_its_item_is_started_again_not_reviewed() {
    let answers = ["hello".to_string(), planned(vec![item("a", "scout", &BIG, &[], &[]), item("b", "inline", &[], &["src/b.rs"], &["a"]), item("c", "inline", &[], &["src/c.rs"], &["b"])])];
    let pi = Scripted::with_caps("pi", Caps { live: true, resume: true, brief: true, ..Caps::default() }, vec![vec![Beat::Fail("no network".into())], vec![end("fine")]]);
    let f = fx_deciding(vec![main_lane(&answers), pi], Some("pi"), auto);
    begin(&f);
    f.front.plan_step("main");
    step(&f);
    let a = f.keeper.plan("main").unwrap().items.remove(0);
    assert_eq!((a.state.as_str(), a.tries), ("running", 2), "started again: {}", f.rig.dump());
    step(&f);
    assert_eq!(f.keeper.plan("main").unwrap().items[0].state, "reviewed");
    assert_eq!(branches(&f).len(), 2);
}

#[test]
fn a_plan_that_was_saved_before_or2_loads_and_the_fold_replays_the_same() {
    let answers = ["hello".to_string(), planned(vec![item("a", "scout", &BIG, &[], &[]), item("b", "inline", &[], &["src/b.rs"], &["a"]), item("c", "inline", &[], &["src/c.rs"], &["b"])])];
    let f = fx_deciding(vec![main_lane(&answers), branching("pi", &["ok"])], Some("pi"), auto);
    begin(&f);
    f.front.plan_step("main");
    let now = f.keeper.plan("main").unwrap();
    let mut again = j4n_core::keeper::Keeper::new();
    let mut fold = again.clone();
    use j4n_core::fold::Fold;
    for ev in f.rig.events() {
        if fold.wants(&ev.kind) {
            fold.apply(&ev);
        }
    }
    assert_eq!(again.plan("main"), Some(now.clone()));
    let mut loaded = j4n_core::keeper::Keeper::new();
    loaded.load(again.save()).unwrap();
    // a snapshot from before the fields existed has none of them
    let old = json!({ "main": { "items": [{ "id": "a", "goal": "g", "state": "todo", "place": "inline", "reads": [], "writes": [], "depends": [], "done": "d", "result": "", "rev": 0, "task": "", "mine": false }], "approach": "x", "reasons": [], "asked": null, "held": null, "gone": [], "micros": 0, "turns": 0 } });
    again.load(old).unwrap();
    assert_eq!(again.plan("main").unwrap().items[0].mark, "");
}

#[test]
fn a_doer_that_fails_its_first_start_is_out_of_the_plans_elections_and_the_item_is_elected_again() {
    let answers = ["hello".to_string(), planned(vec![
        item("work", "inline", &BIG, &["src/w.rs"], &[]),
        item("check", "review", &["src/w.rs", "src/a/1.rs", "src/a/2.rs"], &[], &["work"]),
        item("end", "inline", &[], &["src/e.rs"], &["check"]),
    ]), "worked".to_string(), "ended".to_string()];
    let no_login = Scripted::with_caps("own", Caps { live: true, resume: true, brief: true, ..Caps::default() }, vec![vec![Beat::Fail("unexpected status 401 Unauthorized: Missing bearer or basic authentication in header".into())]]);
    let f = fx_deciding(vec![main_lane(&answers), branching("pi", &["looks right"]), no_login], Some("own"), auto);
    begin(&f);
    f.front.plan_step("main");
    step(&f);
    assert_eq!(branches(&f)[0].spec.doer, "own", "elected while its program is found: {}", f.rig.dump());
    step(&f);
    let out = f.rig.of_kind("plan.doer_out");
    assert_eq!(out.len(), 1, "{}", f.rig.dump());
    assert_eq!((out[0].data["doer"].as_str(), out[0].data["id"].as_str()), (Some("own"), Some("check")));
    step(&f);
    let tried: Vec<String> = branches(&f).iter().map(|r| r.spec.doer.clone()).collect();
    assert_eq!(tried, ["own", "pi"], "the item goes to another doer, and not to `own` again: {}", f.rig.dump());
}

// ---- the sentence of `brief.plain_answer` is for the answer turn alone ----

#[test]
fn the_plain_answer_sentence_is_in_neither_a_planning_turn_nor_a_branch_and_marks_the_turns_that_answer() {
    let answers = ["hello".to_string(), planned(vec![
        item("a", "scout", &BIG, &[], &[]),
        item("b", "scout", &["src/b/1.rs", "src/b/2.rs", "src/b/3.rs"], &[], &[]),
        item("go", "inline", &["src/a/1.rs"], &["src/out.rs"], &["a", "b"]),
    ]), "went on".to_string()];
    let pi = branching("pi", &["the retry is in fetch()", "nothing there"]);
    let mut screen = j4n_core::front::screen::Screen::builtin();
    screen.set = j4n_core::setting::Settings::from_pairs([("brief.plain_answer", j4n_core::value::Value::Bool(true))]).tree();
    let f = crate::front_cases::fx_placed_on(vec![main_lane(&answers), pi], Some("pi"), Vec::new(), screen, auto);
    begin(&f);
    f.front.plan_step("main");
    step(&f);
    let tasks = f.tasks();
    assert!(tasks.iter().any(|r| r.spec.role.as_deref() == Some("plan")) && tasks.iter().any(|r| r.spec.input.get("branch").is_some()), "{}", f.rig.dump());
    for r in &tasks {
        let answers = r.spec.role.as_deref() == Some("work") && r.spec.input.get("branch").is_none();
        assert_eq!(r.spec.input.get("plain_answer").is_some(), answers, "{:?} {:?}", r.spec.role, r.spec.input);
        assert_eq!(r.spec.input["brief"].as_str().unwrap_or("").contains(j4n_core::brief::PLAIN), answers && r.spec.input.get("brief").is_some(), "{:?}", r.spec.input);
    }
    assert!(tasks.iter().any(|r| r.spec.input.get("plain_answer").is_some()), "the turns that answer are marked");
}
