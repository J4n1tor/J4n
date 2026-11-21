//! What is shown belongs to what it is about. The bar names no provider, the
//! orchestrator is told when a chat's turn ended, a lane was merged and a plan moved (three Log kinds written in its own thread, each once), its list
//! marks the chats that are done for a while, and the windows and today's cost stand in `chat:here`. The screen's side (the gauge, the windows row, the
//! fold, the plan row) is `j4n-tui`'s `chat_cases`.

use crate::front_cases::{Fx, fx_deciding, fx_over};
use crate::world::{Scratch, open_log};
use crate::scripted::{Beat, Scripted, end, say};
use j4n_core::front::{Cfg, Said};
use j4n_core::front::compose::{Strip, Work, strip};
use j4n_core::front::serve::Serve;
use j4n_core::log::New;
use j4n_core::change::Actor;
use j4n_proto::{CHAT_HERE, Down, Up};
use serde_json::{Value, json};
use std::sync::Arc;

fn on(c: &mut Cfg) {
    c.chats.on = true;
}

fn doer(turns: usize) -> Arc<Scripted> {
    let one = |n: usize| vec![Beat::Session("s".into()), Beat::Spend(0.5), Beat::Limits(vec![("five_hour", 0.92, 200_000), ("seven_day", 0.4, 200_000)]), say(&format!("answer {n}\nand more")), end(&format!("answer {n}\nand more"))];
    Scripted::new("claude", (0..turns).map(one).collect())
}

fn feed(s: &mut Serve, f: &Fx, from: u64) -> Vec<Down> {
    f.rig.log.sync();
    f.rig.log.read(from).flat_map(|e| s.ev(&e)).collect()
}

fn watch(s: &mut Serve, bind: &str) -> Vec<Down> {
    s.up(Up::Watch { bind: bind.into() }).0
}

fn value(downs: &[Down], bind: &str) -> Option<Value> {
    downs.iter().rev().find_map(|d| match d {
        Down::Value { bind: b, value, .. } if b == bind => Some(value.clone()),
        _ => None,
    })
}

fn rows<'a>(downs: &'a [Down], bind: &str) -> &'a [j4n_proto::Row] {
    downs.iter().rev().find_map(|d| match d {
        Down::Rows { bind: b, rows, .. } if b == bind => Some(rows.as_slice()),
        _ => None,
    }).unwrap_or(&[])
}

#[test]
fn the_bar_names_no_provider_and_the_orchestrators_row_names_each_window_with_when_it_resets() {
    let f = fx_deciding(vec![doer(1)], Some("absent"), on);
    let mut s = Serve::new(f.front.clone());
    f.turn("fix trailing commas in parser.rs");
    f.rig.log.sync();
    let bar = rows(&watch(&mut s, "bar:status"), "bar:status")[0].cells.iter().map(|c| c.text.clone()).collect::<Vec<_>>();
    assert_eq!(bar, ["idle", "5h 92%", "today $0.5000", "Decider off", "proj"], "the bar says the window bare: no harness, no account; `Decider off` is the Decider");
    let here = value(&watch(&mut s, CHAT_HERE), CHAT_HERE).expect("chat:here");
    let windows: Vec<(String, String, f64)> = here["windows"].as_array().expect("windows").iter().map(|w| (w["doer"].as_str().unwrap().into(), w["window"].as_str().unwrap().into(), w["used"].as_f64().unwrap())).collect();
    assert_eq!(windows, [("claude".to_string(), "5h".to_string(), 0.92), ("claude".to_string(), "7d".to_string(), 0.4)], "{here}");
    assert!(here["windows"][0]["resets"].as_str().is_some_and(|t| t.len() == 5), "when it resets, as a time of day: {here}");
    assert_eq!(here["today"], json!("$0.5000"), "what today has cost, said once more in the main chat");
    assert_eq!(here["form"], json!("auto"), "`look.gauge` is auto unless it says line or slice: the screen picks the shape from the width it has");
}

#[test]
fn a_chats_turn_that_ended_is_said_once_in_the_orchestrator_with_the_first_line_of_its_answer() {
    let f = fx_deciding(vec![doer(2)], Some("absent"), on);
    let mut s = Serve::new(f.front.clone());
    watch(&mut s, "chat:main");
    f.turn("fix trailing commas in parser.rs");
    let downs = feed(&mut s, &f, 0);
    assert_eq!(f.count("chat.ended"), 1, "{}", f.rig.dump());
    let ended = f.rig.of_kind("chat.ended").remove(0);
    assert_eq!(ended.thread.as_deref(), Some("main"), "written in the orchestrator's thread");
    assert_eq!((ended.data["chat"].as_str(), ended.data["ok"].as_bool(), ended.data["line"].as_str()), (Some("c1"), Some(true), Some("answer 0 and more")), "{}", ended.data);
    // the settled turn is seen again by a second client, and by the next event: nothing more is written
    feed(&mut s, &f, 0);
    let mut other = Serve::new(f.front.clone());
    feed(&mut other, &f, 0);
    assert_eq!(f.count("chat.ended"), 1, "once for the turn: the Log says so");
    // the row is the orchestrator's, a door into the chat
    let after = feed(&mut s, &f, ended.seq);
    let all: Vec<j4n_proto::Row> = downs.iter().chain(after.iter()).flat_map(|d| match d { Down::Rows { bind, rows, .. } if bind == "chat:main" => rows.clone(), _ => Vec::new() }).collect();
    let row = all.iter().find(|r| r.cells[0].text.starts_with("c1 · fix trailing")).expect("the row of the end");
    assert_eq!((row.cells[0].icon.as_str(), row.data.as_ref().and_then(|d| d["door"].as_str())), ("done", Some("c1")), "{row:?}");
    assert_eq!(row.detail.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["answer 0", "and more"], "the answer is the row's preview, a line to a cell");
    // the next turn of the chat is its own end
    f.turn("also update the docs for parser.rs");
    feed(&mut s, &f, ended.seq);
    assert_eq!(f.count("chat.ended"), 2);
}

#[test]
fn a_merge_and_a_plan_are_one_row_each_the_plan_said_again_in_place() {
    let f = fx_deciding(vec![doer(1)], Some("absent"), on);
    let mut s = Serve::new(f.front.clone());
    watch(&mut s, "chat:main");
    f.turn("fix trailing commas in parser.rs");
    let lane = json!([{ "label": "c1.1", "doer": "omp" }, { "label": "c1.2", "doer": "claude" }, { "label": "c1.3", "doer": "pi" }]);
    let at = f.rig.log.head();
    for ev in [
        New::of("chat.plan", Actor::Core, json!({ "chat": "c1", "done": 1, "total": 3, "lanes": lane, "key": "plan:c1:1/3" })).thread("main"),
        New::of("chat.merged", Actor::Core, json!({ "lane": "c1.1", "chat": "c1", "what": "2 files +4 -4", "ok": true })).thread("main"),
        New::of("chat.merged", Actor::Core, json!({ "lane": "c1.2", "chat": "c1", "what": "", "ok": false })).thread("main"),
        New::of("chat.merged", Actor::Core, json!({ "lane": "c1.3", "chat": "c1", "what": "1 file +2 -0", "ok": false, "still": true })).thread("main"),
        New::of("chat.plan", Actor::Core, json!({ "chat": "c1", "done": 2, "total": 3, "lanes": lane, "key": "plan:c1:2/3" })).thread("main"),
    ] {
        f.rig.log.append(ev).unwrap();
    }
    let downs = feed(&mut s, &f, at + 1);
    let last = rows(&downs, "chat:main");
    let text: Vec<&str> = last.iter().map(|r| r.cells[0].text.as_str()).collect();
    assert!(text.contains(&"c1.1 → c1 merged  2 files +4 -4  gate ✓"), "{text:?}");
    assert!(text.contains(&"c1.2 → c1 merged  unproven"), "a lane whose end no check showed does not say the gate held: {text:?}");
    assert!(text.contains(&"c1.3 → c1 merged  1 file +2 -0  fails as before"), "a lane laid in over failures the folder had before it does not say the gate held: {text:?}");
    let plans: Vec<&j4n_proto::Row> = last.iter().filter(|r| r.data.as_ref().is_some_and(|d| d.get("plan").is_some())).collect();
    assert_eq!(plans.len(), 1, "one row for a chat's plan, said again as it moves: {text:?}");
    assert_eq!(plans[0].cells[0].text, "plan 2/3");
    assert_eq!(plans[0].data.as_ref().unwrap()["plan"]["lanes"], json!("c1.1 omp ▸ c1.2 claude ▸ c1.3 pi"));
}

#[test]
fn a_chat_that_is_done_for_half_an_hour_is_marked_old_for_the_screen_to_fold() {
    let f = fx_deciding(vec![doer(1)], Some("absent"), on);
    f.turn("fix trailing commas in parser.rs");
    let old = |f: &Fx| f.front.chat_list().iter().map(|r| r.data.as_ref().unwrap()["old"].as_bool().unwrap_or(false)).collect::<Vec<_>>();
    assert_eq!(old(&f), [false], "just done: it stays");
    f.rig.clock.advance(29 * 60_000);
    assert_eq!(old(&f), [false], "29 minutes on");
    f.rig.clock.advance(2 * 60_000);
    assert_eq!(old(&f), [true], "31 minutes on: the screen folds it into `N done before`");
}

#[test]
fn the_strip_carries_no_fraction_for_a_task_and_says_the_form_of_its_gauge_unless_it_is_the_line() {
    let s = Strip { route: "claude · sonnet 5.5", mode: "write", work: vec![Work { label: "working".into() }], window: Some(0.42), ..Default::default() };
    let v = strip(&s);
    assert!(v["work"][0].get("frac").is_none() && v.get("form").is_none(), "{v}");
    let v = strip(&Strip { form: "slice", ..s.clone() });
    assert_eq!(v["form"], json!("slice"), "{v}");
    assert_eq!(strip(&Strip { form: "auto", ..s.clone() })["form"], json!("auto"));
    assert!(strip(&Strip { form: "line", ..s })["form"].is_null(), "the thin line is what a strip with no form says");
}

#[test]
fn what_an_older_log_holds_is_not_said_when_the_front_begins_and_what_ends_after_is() {
    // the Log of a front that had no `said` yet (an older build): a chat with a finished turn
    let (p1, h1) = (Scratch::new("proj"), Scratch::new("home"));
    let (log, clock) = open_log(&p1);
    let before = fx_over(p1, h1, log.clone(), clock.clone(), vec![doer(1)], Some("absent"), on);
    before.turn("fix trailing commas in parser.rs");
    assert_eq!(before.count("chat.ended"), 0, "nothing was attached to say it");
    clock.advance(10 * 60_000);
    // a new front over the same Log, with a screen: the old turn is in its book and is not said
    let (p2, h2) = (Scratch::new("proj"), Scratch::new("home"));
    let after = fx_over(p2, h2, log, clock.clone(), vec![doer(1)], Some("absent"), on);
    assert_eq!(after.tasks().len(), 1, "the old turn is in the book: {}", after.rig.dump());
    let mut s = Serve::new(after.front.clone());
    feed(&mut s, &after, 0);
    after.front.plan_step("c1");
    assert_eq!(after.count("chat.ended"), 0, "an old chat's last turn is not a row stamped with now: {}", after.rig.dump());
    // a turn that ends after the front began is said, and the old one still is not
    after.say("also update the docs for parser.rs");
    for t in after.tasks().iter().filter(|r| !r.state.is_terminal()) {
        after.rig.finished(&t.id);
    }
    feed(&mut s, &after, 0);
    let ended: Vec<Value> = after.rig.of_kind("chat.ended").into_iter().map(|e| e.data).collect();
    assert_eq!(ended.len(), 1, "{ended:?}");
}

#[test]
fn a_turn_that_ended_while_a_newer_one_runs_is_said_and_one_that_ended_the_moment_the_front_began_is() {
    // the second turn holds: the chat's newest task is running while the first one has ended unsaid (no screen was attached)
    let two = Scripted::new("claude", vec![
        vec![Beat::Session("s".into()), say("first"), end("first")],
        vec![Beat::Session("s".into()), Beat::Hold, end("second")],
    ]);
    let f = fx_deciding(vec![two], Some("absent"), on);
    let first = f.turn("fix trailing commas in parser.rs");
    assert_eq!(first.ended_ms, Some(1_000), "ended the very moment the front began (the manual clock has not moved): the boundary is inclusive");
    f.say("also update the docs for parser.rs");
    assert!(f.tasks().iter().any(|r| !r.state.is_terminal()), "the newer turn runs");
    f.front.plan_step("c1");
    let ended = f.rig.of_kind("chat.ended");
    assert_eq!(ended.len(), 1, "the newest-only look found nothing to say: {}", f.rig.dump());
    assert_eq!(ended[0].data["task"], json!(first.id));
}

#[test]
fn a_turn_that_ended_one_millisecond_before_the_front_began_is_not_said() {
    let (p1, h1) = (Scratch::new("proj"), Scratch::new("home"));
    let (log, clock) = open_log(&p1);
    let before = fx_over(p1, h1, log.clone(), clock.clone(), vec![doer(1)], Some("absent"), on);
    let old = before.turn("fix trailing commas in parser.rs");
    assert_eq!(old.ended_ms, Some(1_000));
    clock.advance(1);
    let (p2, h2) = (Scratch::new("proj"), Scratch::new("home"));
    let after = fx_over(p2, h2, log, clock, vec![doer(1)], Some("absent"), on);
    after.front.plan_step("c1");
    assert_eq!(after.count("chat.ended"), 0, "{}", after.rig.dump());
}

#[test]
fn a_plan_of_a_chat_nothing_touched_since_the_front_began_is_not_said_until_the_chat_is_touched() {
    let (p1, h1) = (Scratch::new("proj"), Scratch::new("home"));
    let (log, clock) = open_log(&p1);
    let before = fx_over(p1, h1, log.clone(), clock.clone(), vec![doer(1)], Some("absent"), on);
    before.turn("fix trailing commas in parser.rs");
    for id in ["a", "b"] {
        let item = json!({ "op": "add", "id": id, "goal": format!("do {id}"), "done": format!("{id} works"), "check": "", "reads": [], "writes": [], "depends": [], "where": "inline", "main": false, "task": "" });
        log.append(New::of("plan.item", Actor::Core, item).thread("c1")).unwrap();
    }
    clock.advance(10 * 60_000);
    let (p2, h2) = (Scratch::new("proj"), Scratch::new("home"));
    let after = fx_over(p2, h2, log, clock, vec![doer(1)], Some("absent"), on);
    assert_eq!(after.front.plan_counts("c1"), Some((0, 2)), "the old plan is there: {}", after.rig.dump());
    after.front.plan_step("c1");
    assert_eq!(after.count("chat.plan"), 0, "an old plan is not a row stamped with now");
    // a line in the chat touches it: the plan is said as it stands
    let t = after.say("also update the docs for parser.rs");
    let Said::Started { task } = t else { panic!("{t:?}\n{}", after.rig.dump()) };
    after.rig.finished(&task);
    after.front.plan_step("c1");
    let plans = after.rig.of_kind("chat.plan");
    assert_eq!(plans.len(), 1, "{}", after.rig.dump());
    assert_eq!(plans[0].data["total"].as_u64(), Some(2), "said as it stands after the line (the turn took the first item): {}", plans[0].data);
}

/// The rows of the orchestrator's chat as the newest `Rows` says them.
fn all_rows(downs: &[Down]) -> Vec<j4n_proto::Row> {
    rows(downs, "chat:main").to_vec()
}

#[test]
fn a_check_is_no_turn_of_its_chat_its_verdict_is_a_word_on_the_turn_it_checked_and_the_end_carries_the_answer() {
    let not_run = "4-pytest-project: not run — its output says `no module named pytest`; that is no evidence either way";
    let check = Scripted::new("check", vec![vec![say(not_run), end(not_run)]]);
    let f = fx_deciding(vec![doer(1), check], Some("absent"), |c| {
        on(c);
        c.doer = Some("claude".into());
    });
    let mut s = Serve::new(f.front.clone());
    watch(&mut s, "chat:main");
    let first = f.turn("fix trailing commas in parser.rs");
    let mut spec = j4n_core::task::TaskSpec::new(&format!("check what {} changed", first.id), "check");
    spec.thread = first.spec.thread.clone();
    let id = f.rig.tasks.spawn(spec, Actor::Core).expect("the check starts");
    f.rig.finished(&id);
    let downs = feed(&mut s, &f, 0);
    f.front.plan_step("c1");
    let downs: Vec<Down> = downs.into_iter().chain(feed(&mut s, &f, 0)).collect();
    assert_eq!(f.count("chat.ended"), 1, "the check is no turn of the chat: {}", f.rig.dump());
    let ended = f.rig.of_kind("chat.ended").remove(0);
    assert_eq!(ended.data["task"], json!(first.id));
    assert_eq!(ended.data["text"], json!("answer 0\nand more"), "the answer, not its first words: {}", ended.data);
    assert_eq!(ended.data["line"], json!("answer 0 and more"), "the cut line stays for older readers");
    assert_eq!(ended.data["title"], json!("fix trailing commas in parser.rs"));
    let checked = f.rig.of_kind("chat.checked");
    assert_eq!(checked.len(), 1, "{}", f.rig.dump());
    assert_eq!((checked[0].data["of"].as_str(), checked[0].data["verdict"].as_str()), (Some(first.id.as_str()), Some("not run")), "{}", checked[0].data);
    f.front.plan_step("c1");
    assert_eq!((f.count("chat.ended"), f.count("chat.checked")), (1, 1), "each said once");
    // the row of the turn: its preview, the verdict beside it, and no row for the check
    let all = all_rows(&downs);
    let doors: Vec<&j4n_proto::Row> = all.iter().filter(|r| r.data.as_ref().is_some_and(|d| d.get("door").is_some())).collect();
    assert_eq!(doors.len(), 1, "{all:?}");
    let row = doors[0];
    assert_eq!(row.parent, None, "its own row");
    assert_eq!(row.cells[0].text, "c1 · fix trailing commas in parser.rs");
    assert!(row.cells.iter().any(|c| c.text == "not run"), "{row:?}");
    assert_eq!(row.detail.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["answer 0", "and more"]);
    assert!(!all.iter().any(|r| r.cells[0].text.contains("no module named pytest")), "the check's line is nowhere a result of the chat: {all:?}");
}

#[test]
fn a_chats_end_a_merge_and_a_plan_hang_from_no_line_of_the_top_chat() {
    let f = fx_deciding(vec![doer(1)], Some("absent"), on);
    let mut s = Serve::new(f.front.clone());
    watch(&mut s, "chat:main");
    let at = f.rig.log.head();
    for ev in [
        New::of("chat.say", Actor::Operator, json!({ "text": "/usage" })).thread("main"),
        New::of("chat.ended", Actor::Core, json!({ "chat": "c1", "task": "t9", "ok": true, "line": "done", "text": "done", "key": "end:t9" })).thread("main"),
        New::of("chat.merged", Actor::Core, json!({ "lane": "c1.1", "chat": "c1", "what": "", "ok": true })).thread("main"),
        New::of("chat.plan", Actor::Core, json!({ "chat": "c1", "done": 1, "total": 2, "lanes": [], "key": "plan:c1:1/2" })).thread("main"),
        New::of("chat.say", Actor::Operator, json!({ "text": "fix the parser" })).thread("main"),
        New::of("chat.note", Actor::Core, json!({ "text": "→ c1 · same files", "tone": "dim" })).thread("main"),
    ] {
        f.rig.log.append(ev).unwrap();
    }
    let all = all_rows(&feed(&mut s, &f, at + 1));
    let by = |needle: &str| all.iter().find(|r| r.cells[0].text.contains(needle)).unwrap_or_else(|| panic!("no row `{needle}`: {all:?}"));
    let usage = by("/usage");
    assert_eq!(usage.parent, None);
    for needle in ["c1", "merged", "plan 1/2"] {
        assert_eq!(by(needle).parent, None, "`{needle}` is the orchestrator's own row, not an answer to `/usage`");
    }
    let line = by("fix the parser");
    let route = all.iter().find(|r| r.cells[0].text.starts_with('→')).unwrap_or_else(|| panic!("no route row: {all:?}"));
    assert_eq!(route.parent.as_deref(), Some(line.key.as_str()), "the route still answers the line it is for");
}
