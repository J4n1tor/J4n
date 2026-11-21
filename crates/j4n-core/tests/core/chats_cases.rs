//! Chats. The loop, the front, the Log, the ask point `chats` and the screen's server are the
//! real ones (the base pack's table is read from disk) and the doers are scripted; Jev is the fixture's decider, which answers the pick it was given, or is
//! not there. What is asserted is which chat a line went to and why (the Log's `chat.routed`, the route row), what the screen is sent for entering and leaving,
//! and what the words did to the chat in hand.

use crate::front_cases::{Fx, chatty, fx_deciding};
use crate::scripted::{Beat, Scripted, choice, end, say};
use j4n_core::branch::Switch;
use j4n_core::front::serve::Serve;
use j4n_core::front::{Cfg, Said};
use j4n_core::task::Row;
use j4n_proto::{By, CHAT_HERE, CHATS, Down, Intent, Reply, Up};
use serde_json::{Value, json};
use std::sync::Arc;

fn on(c: &mut Cfg) {
    c.chats.on = true;
}

fn serve(f: &Fx) -> Serve {
    Serve::new(f.front.clone())
}

fn up(s: &mut Serve, u: Up) -> Vec<Down> {
    let (downs, quit) = s.up(u);
    assert!(!quit, "{downs:?}");
    downs
}

fn watch(s: &mut Serve, bind: &str) -> Vec<Down> {
    up(s, Up::Watch { bind: bind.into() })
}

fn say_up(s: &mut Serve, text: &str) -> Vec<Down> {
    up(s, Up::Intent { intent: Intent::Say { text: text.into(), with: Vec::new() } })
}

/// Feed the events the Log holds from `from` on to the client, as the server thread would.
fn feed(s: &mut Serve, f: &Fx, from: u64) -> Vec<Down> {
    f.rig.log.read(from).flat_map(|e| s.ev(&e)).collect()
}

fn rows<'a>(downs: &'a [Down], bind: &str) -> &'a [j4n_proto::Row] {
    downs.iter().rev().find_map(|d| match d {
        Down::Rows { bind: b, rows, .. } if b == bind => Some(rows.as_slice()),
        _ => None,
    }).unwrap_or(&[])
}

fn here(downs: &[Down]) -> Option<Value> {
    downs.iter().rev().find_map(|d| match d {
        Down::Value { bind, value, .. } if bind == CHAT_HERE => Some(value.clone()),
        _ => None,
    })
}

/// The words of the first cell of each row of the chat.
fn said(downs: &[Down]) -> Vec<String> {
    rows(downs, "chat:main").iter().filter_map(|r| r.cells.first().map(|c| c.text.clone())).collect()
}

fn routed(f: &Fx) -> Vec<Value> {
    f.rig.of_kind("chat.routed").into_iter().map(|e| e.data).collect()
}

fn of_chat(f: &Fx, chat: &str) -> Vec<Row> {
    f.tasks().into_iter().filter(|r| r.spec.thread.as_deref() == Some(chat) && r.spec.doer != "human").collect()
}

fn claude(n: usize) -> Arc<Scripted> {
    chatty("claude", &(0..n).map(|i| format!("answer {i}")).collect::<Vec<_>>().iter().map(String::as_str).collect::<Vec<_>>())
}

const PARSER: &str = "fix trailing commas in parser.rs";
const LEXER: &str = "rename the helper in lexer.rs";
const BOTH: &str = "check that parser.rs and lexer.rs agree";

// ---- the switch ----

#[test]
fn with_the_switch_off_a_line_is_the_threads_nothing_is_routed_asked_or_written_and_new_is_fresh() {
    let f = fx_deciding(vec![claude(3)], Some("chats=new"), |_| {});
    let t = f.turn(PARSER);
    assert_eq!(t.spec.thread.as_deref(), Some("main"), "the line is the thread's own: {}", f.rig.dump());
    f.turn("and the lexer");
    for k in ["chat.opened", "chat.routed", "chat.route.asked"] {
        assert_eq!(f.count(k), 0, "{k}: {}", f.rig.dump());
    }
    assert!(f.asked.lock().unwrap().iter().all(|q| q.point != "chats"), "the point was not asked");
    assert!(f.front.chats().is_empty() && f.front.chat_list().is_empty() && f.front.route_asks().is_empty());
    // `:new` is `:fresh`, as it was; the other words say the chats are off and change nothing
    assert!(f.front.binding("main").is_some());
    assert_eq!(f.say(":new"), Said::Command("fresh".into()));
    assert!(f.front.binding("main").is_none());
    assert_eq!(f.count("thread.fresh"), 1);
    for word in [":clear", ":history", ":chats", ":top"] {
        assert!(matches!(f.say(word), Said::Refused(_)), "{word}");
    }
    assert!(f.notes().iter().any(|n| n.starts_with("the chats are off")), "{:?}", f.notes());
    // the screen is not given anything of the chats
    let mut s = serve(&f);
    assert_eq!(here(&watch(&mut s, CHAT_HERE)), Some(Value::Null));
    assert!(rows(&watch(&mut s, CHATS), CHATS).is_empty());
    assert!(matches!(&up(&mut s, Up::Intent { intent: Intent::Enter { chat: "c1".into() } })[0], Down::Toast { .. }));
    assert_eq!(s.here(), "");
}

// ---- routing ----

#[test]
fn a_line_opens_c1_and_a_line_that_names_its_file_goes_on_there_and_the_orchestrator_has_no_doer() {
    let f = fx_deciding(vec![claude(3)], Some("absent"), on);
    let first = f.turn(PARSER);
    assert_eq!(first.spec.thread.as_deref(), Some("c1"), "{}", f.rig.dump());
    assert_eq!(f.front.chat("c1").map(|c| c.title), Some(PARSER.to_string()));
    let second = f.turn("also update the docs for parser.rs");
    assert_eq!(second.spec.thread.as_deref(), Some("c1"), "the line went to the chat it matches: {}", f.rig.dump());
    assert_eq!(second.spec.follows.as_deref(), Some(first.id.as_str()), "in the chat's own session");
    assert_eq!(f.count("chat.opened"), 1, "no chat was opened for the second line");
    let r = routed(&f);
    assert_eq!((r[0]["to"].as_str(), r[0]["new"].clone(), r[0]["why"].as_str(), r[0]["by"].as_str()), (Some("c1"), json!(true), Some("new chat · none matched"), Some("code")));
    assert_eq!((r[1]["to"].as_str(), r[1]["new"].clone(), r[1]["why"].as_str(), r[1]["detail"].as_str(), r[1]["by"].as_str()), (Some("c1"), json!(false), Some("same files"), Some("parser.rs"), Some("code")));
    // the route row comes after the send, in the orchestrator's turn, and says why
    assert_eq!(f.notes().iter().filter(|n| n.starts_with("→")).collect::<Vec<_>>(), ["→ c1 · new chat · none matched", "→ c1 · same files · parser.rs"]);
    assert_eq!(f.front.binding("main"), None, "the orchestrator has no doer");
    assert!(f.front.binding("c1").is_some());
}

#[test]
fn a_line_about_something_else_is_asked_when_jev_is_absent_and_the_chat_it_opens_has_a_session_of_its_own() {
    let f = fx_deciding(vec![claude(3)], Some("absent"), on);
    let first = f.turn(PARSER);
    let Said::Asking { ask } = f.say("what does the budget gate check") else { panic!("{}", f.rig.dump()) };
    let Said::Started { task } = f.front.reply("main", &ask, Reply::Pick { id: "new".into() }) else { panic!("{}", f.rig.dump()) };
    let other = f.rig.finished(&task);
    assert_eq!(other.spec.thread.as_deref(), Some("c2"), "{}", f.rig.dump());
    assert_eq!(other.spec.follows, None, "a session of its own");
    assert_eq!(f.tasks().iter().filter(|r| r.spec.thread.as_deref() == Some("c1")).count(), 1);
    assert!(f.front.binding("c1").is_some() && f.front.binding("c2").is_some());
    let r = routed(&f);
    assert_eq!((r[1]["to"].as_str(), r[1]["new"].clone(), r[1]["by"].as_str(), r[1]["why"].as_str()), (Some("c2"), json!(true), Some("you"), Some("new chat · Decider off")));
    assert_eq!(r[1]["detail"].as_str(), Some("Decider off"), "the Log says why: {}", r[1]);
    assert!(f.asked.lock().unwrap().iter().all(|q| q.point != "chats"), "nobody was asked");
    assert_ne!(first.id, other.id);
}

#[test]
fn with_no_chat_yet_jev_is_not_asked_and_with_chats_it_routes_a_line_code_cannot_by_itself() {
    let f = fx_deciding(vec![claude(3)], Some("chats=c1"), on);
    f.turn(PARSER);
    assert!(f.asked.lock().unwrap().iter().all(|q| q.point != "chats"), "there was nothing to choose among");
    // no word or file in common: code finds none, and Jev says it is c1's
    let t = f.turn("make arrays tolerate that dangling punctuation");
    assert_eq!(t.spec.thread.as_deref(), Some("c1"), "{}", f.rig.dump());
    let asked: Vec<_> = f.asked.lock().unwrap().iter().filter(|q| q.point == "chats").cloned().collect();
    assert_eq!(asked.len(), 1);
    let r = routed(&f);
    assert_eq!((r[1]["by"].as_str(), r[1]["why"].as_str(), r[1]["new"].clone()), (Some("decider:jev"), Some("same subject"), json!(false)));
    // and Jev saying `new` opens one
    let f = fx_deciding(vec![claude(3)], Some("chats=new"), on);
    f.turn(PARSER);
    let t = f.turn("make arrays tolerate that dangling punctuation");
    assert_eq!(t.spec.thread.as_deref(), Some("c2"));
}

#[test]
fn two_chats_that_match_are_one_ask_answered_from_the_orchestrator_with_jevs_pick_as_the_hint() {
    let f = fx_deciding(vec![claude(3)], Some("chats=c2"), on);
    f.turn(PARSER);
    f.turn(LEXER);
    let before = f.tasks().len();
    let Said::Asking { ask } = f.say(BOTH) else { panic!("{}", f.rig.dump()) };
    // `route:1` was the question of the second line: Jev named a chat that is not there (`c2`), so the operator was asked
    assert_eq!(ask, "route:2");
    assert_eq!(f.tasks().len(), before, "nothing was sent while it is not answered");
    assert_eq!(f.count("chat.route.asked"), 2);
    let mut s = serve(&f);
    let Down::Asks { open } = watch(&mut s, "asks:open").remove(0) else { panic!() };
    assert_eq!((open.len(), open[0].ask.as_str(), open[0].question.as_str()), (1, "route:2", "which chat takes this?"));
    let ids: Vec<&str> = open[0].options.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["c1", "c2", "new"], "an option for each match and a new chat");
    let hint = open[0].options.iter().find(|o| o.recommended).expect("Jev's pick");
    assert_eq!((hint.id.as_str(), hint.by), ("c2", By::Model), "marked as the model's");
    assert!(open[0].options[0].note.contains("claude"), "its doer at the right: {:?}", open[0].options[0]);
    // answered from here, without going in
    up(&mut s, Up::Intent { intent: Intent::Answer { ask: "route:2".into(), reply: Reply::Pick { id: "c1".into() } } });
    let sent = f.tasks().into_iter().last().unwrap();
    assert_eq!((sent.spec.thread.as_deref(), sent.spec.goal.as_str()), (Some("c1"), BOTH), "{}", f.rig.dump());
    f.rig.finished(&sent.id);
    let r = routed(&f);
    assert_eq!((r[2]["to"].as_str(), r[2]["by"].as_str(), r[2]["why"].as_str()), (Some("c1"), Some("you"), Some("you chose it")));
    assert!(f.front.route_asks().is_empty());
    assert!(watch(&mut s, "asks:open").iter().all(|d| matches!(d, Down::Asks { open } if open.is_empty())), "the question is gone");
}

#[test]
fn the_number_typed_in_the_orchestrator_answers_it_too_and_a_new_line_drops_a_question_that_waits() {
    let f = fx_deciding(vec![claude(4)], Some("absent"), on);
    f.turn(PARSER);
    f.turn(LEXER);
    assert!(matches!(f.say(BOTH), Said::Asking { .. }));
    let said = f.say("3");
    let Said::Started { task } = said else { panic!("{said:?}\n{}", f.rig.dump()) };
    f.rig.finished(&task);
    assert_eq!(f.tasks().last().unwrap().spec.thread.as_deref(), Some("c3"), "the third option is a new chat");
    assert_eq!(routed(&f).last().unwrap()["why"], "new chat · you chose it");
    // a question that waits is dropped by a line that is not its answer
    assert!(matches!(f.say("check parser.rs and lexer.rs once more"), Said::Asking { .. }));
    assert_eq!(f.front.route_asks().len(), 1);
    let said = f.say_new("what does the budget gate check");
    assert!(matches!(said, Said::Started { .. }), "{said:?}");
    assert!(f.front.route_asks().is_empty());
    assert!(f.notes().iter().any(|n| n.contains("is dropped")), "{:?}", f.notes());
}

#[test]
fn one_chat_that_matches_and_is_cooling_or_failed_is_a_problem_and_asks() {
    let failing = Scripted::new("claude", vec![vec![Beat::Session("s".into()), Beat::Fail("boom".into())], vec![Beat::Session("s2".into()), say("ok"), end("ok")]]);
    let f = fx_deciding(vec![failing], Some("absent"), on);
    let Said::Started { task } = f.say(PARSER) else { panic!() };
    f.rig.finished(&task);
    let Said::Asking { .. } = f.say("again with parser.rs") else { panic!("{}", f.rig.dump()) };
    let q = &f.front.route_asks()[0];
    assert!(q.question.contains("c1's last turn failed") && q.options.iter().map(|o| o.id.as_str()).eq(["c1", "new"]), "{q:?}");
}

// ---- entering and leaving ----

#[test]
fn the_screen_enters_a_chat_and_leaves_it_and_is_sent_that_chats_rows_and_where_it_is() {
    let f = fx_deciding(vec![claude(3)], Some("absent"), on);
    f.turn(PARSER);
    f.turn("what does the budget gate check");
    let mut s = serve(&f);
    let start = watch(&mut s, CHAT_HERE);
    let top = here(&start).unwrap();
    assert_eq!((top["top"].clone(), top["chats"].clone(), top["id"].clone()), (json!(true), json!(2), json!("")));
    let list = watch(&mut s, CHATS);
    assert_eq!(rows(&list, CHATS).iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), ["c2", "c1"], "newest first when none waits or runs: {:?}", rows(&list, CHATS));
    let chat = said(&watch(&mut s, "chat:main"));
    assert!(chat.contains(&PARSER.to_string()) && chat.contains(&"→ c1 · new chat · none matched".to_string()), "the orchestrator's rows: {chat:?}");
    // in
    let downs = up(&mut s, Up::Intent { intent: Intent::Enter { chat: "c1".into() } });
    let h = here(&downs).expect("where it is is sent");
    assert_eq!((h["top"].clone(), h["id"].clone(), h["crumb"][0]["id"].clone(), h["crumb"][0]["title"].clone()), (json!(false), json!("c1"), json!("c1"), json!(PARSER)));
    assert_eq!((s.here(), s.thread()), ("c1", "c1"));
    let inside = said(&downs);
    assert_eq!(inside.first().map(String::as_str), Some(PARSER), "c1's own rows: {inside:?}");
    assert!(inside.iter().all(|t| !t.starts_with("→") && !t.contains("budget")), "not the orchestrator's, nor another chat's: {inside:?}");
    // a line typed inside goes to that chat and is not routed
    let n = f.count("chat.routed");
    say_up(&mut s, "now the docs please");
    assert_eq!(f.tasks().last().unwrap().spec.thread.as_deref(), Some("c1"));
    assert_eq!(f.count("chat.routed"), n, "nothing is routed inside a chat");
    f.rig.finished(&f.tasks().last().unwrap().id);
    // up
    let downs = up(&mut s, Up::Intent { intent: Intent::Leave });
    assert_eq!(here(&downs).unwrap()["top"], json!(true));
    assert_eq!((s.here(), s.thread()), ("", "main"));
    assert!(said(&downs).contains(&"→ c1 · new chat · none matched".to_string()));
    assert!(up(&mut s, Up::Intent { intent: Intent::Leave }).is_empty(), "at the top there is nothing above");
    // by name, and a chat that is not there
    assert_eq!(here(&up(&mut s, Up::Intent { intent: Intent::Enter { chat: "c2".into() } })).unwrap()["id"], "c2");
    assert!(matches!(up(&mut s, Up::Intent { intent: Intent::Enter { chat: "c9".into() } })[0], Down::Toast { .. }));
    assert_eq!(s.here(), "c2");
    assert!(here(&say_up(&mut s, ":top")).is_some_and(|h| h["top"] == json!(true)), ":top goes to the top");
    assert!(here(&say_up(&mut s, ":main")).is_none(), "`:main` is a way up too, and at the top nothing changes");
}

#[test]
fn next_steps_to_the_chat_that_waits_before_the_one_that_runs() {
    let ask = Scripted::new("claude", vec![vec![Beat::Session("s".into()), say("a"), end("a")], vec![Beat::Session("s".into()), Beat::Ask(choice("q", "which one?", &[("a", true), ("b", false)])), Beat::Hold, end("x")], vec![Beat::Session("s".into()), Beat::Hold, end("y")]]);
    let f = fx_deciding(vec![ask], Some("absent"), on);
    f.turn(PARSER);
    let Said::Started { task: waits } = f.say_new("what does the budget gate check") else { panic!() };
    f.rig.until(&waits, "waiting", |r| matches!(r.state, j4n_core::task::State::Waiting { .. }));
    let Said::Started { task: runs } = f.say_new("summarize the log since friday") else { panic!() };
    f.rig.until(&runs, "running", |r| matches!(r.state, j4n_core::task::State::Running));
    let list = f.front.chat_list();
    assert_eq!(list.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), ["c2", "c3", "c1"], "the one that waits first, then the one that runs");
    assert_eq!(f.front.next_chat_after(""), Some("c2".into()));
    assert_eq!(f.front.next_chat_after("c2"), Some("c3".into()));
    let mut s = serve(&f);
    watch(&mut s, CHAT_HERE);
    assert_eq!(here(&up(&mut s, Up::Intent { intent: Intent::Enter { chat: "@next".into() } })).unwrap()["id"], "c2");
    f.rig.tasks.ctl(&waits, j4n_core::task::Ctl::Abort, j4n_core::change::Actor::Operator).unwrap();
    f.rig.tasks.ctl(&runs, j4n_core::task::Ctl::Abort, j4n_core::change::Actor::Operator).unwrap();
}

// ---- the words ----

#[test]
fn new_with_words_opens_a_chat_sends_them_and_leaves_you_where_you_are_and_new_alone_goes_in() {
    let f = fx_deciding(vec![claude(3)], Some("absent"), on);
    let mut s = serve(&f);
    watch(&mut s, CHAT_HERE);
    watch(&mut s, "chat:main");
    let downs = say_up(&mut s, ":new summarize the log since friday");
    assert_eq!(s.here(), "", "you stay where you are");
    let c1 = f.front.chat("c1").expect("a chat was opened");
    assert_eq!(c1.title, "summarize the log since friday");
    let t = of_chat(&f, "c1");
    assert_eq!((t.len(), t[0].spec.goal.as_str()), (1, "summarize the log since friday"), "the words were sent: {}", f.rig.dump());
    assert!(f.notes().contains(&"→ c1 new chat · you asked for it".to_string()), "{:?}", f.notes());
    assert_eq!(routed(&f)[0]["by"], "you");
    f.rig.finished(&t[0].id);
    assert!(here(&downs).is_none(), "the screen was not moved");
    // alone: a chat with no line yet, and in
    let downs = say_up(&mut s, ":new");
    assert_eq!(s.here(), "c2");
    assert_eq!(here(&downs).unwrap()["id"], "c2");
    assert_eq!(f.front.chat("c2").map(|c| c.title), Some(String::new()));
    assert!(of_chat(&f, "c2").is_empty());
    // its first line names it, and is its own (nothing is routed inside a chat)
    say_up(&mut s, "check parser.rs for tabs");
    assert_eq!(f.front.chat("c2").map(|c| c.title), Some("check parser.rs for tabs".into()));
    assert_eq!(of_chat(&f, "c2").len(), 1);
}

#[test]
fn fresh_and_clear_act_on_the_chat_in_hand_and_history_brings_the_rows_back() {
    let f = fx_deciding(vec![claude(4)], Some("absent"), on);
    f.turn(PARSER);
    f.turn("what does the budget gate check");
    let mut s = serve(&f);
    watch(&mut s, "chat:main");
    up(&mut s, Up::Intent { intent: Intent::Enter { chat: "c1".into() } });
    say_up(&mut s, ":fresh");
    assert!(f.front.binding("c1").is_none() && f.front.binding("c2").is_some(), "only the chat in hand starts again");
    assert!(f.notes().contains(&"new session by :fresh · the next line starts cold".to_string()), "{:?}", f.notes());
    // clear: this chat's rows, nothing else
    let before = f.rig.log.head();
    say_up(&mut s, ":clear");
    let after = said(&feed(&mut s, &f, before));
    assert!(after.iter().any(|t| t.starts_with("cleared ") && t.contains("rows · the Log keeps them · ⏎ brings them back (`:history`)")), "{after:?}");
    assert!(!after.contains(&PARSER.to_string()), "the rows are gone from the screen: {after:?}");
    let kept = f.rig.log.read(0).filter(|e| e.thread.as_deref() == Some("c1")).count();
    assert!(kept > 5, "the Log keeps them");
    let c2 = said(&up(&mut s, Up::Intent { intent: Intent::Enter { chat: "c2".into() } }));
    assert_eq!(c2.first().map(String::as_str), Some("what does the budget gate check"), "another chat is not touched: {c2:?}");
    let back = said(&up(&mut s, Up::Intent { intent: Intent::Enter { chat: "c1".into() } }));
    assert!(back.iter().any(|t| t.starts_with("cleared ")) && !back.contains(&PARSER.to_string()), "it is cleared when it is entered again: {back:?}");
    let before = f.rig.log.head();
    say_up(&mut s, ":history");
    let again = said(&feed(&mut s, &f, before));
    assert!(again.contains(&PARSER.to_string()), ":history brings the rows back: {again:?}");
    let whole = said(&up(&mut s, Up::Intent { intent: Intent::Enter { chat: "c2".into() } }));
    assert!(whole.len() > 1);
    let whole = said(&up(&mut s, Up::Intent { intent: Intent::Enter { chat: "c1".into() } }));
    assert!(whole.contains(&PARSER.to_string()), "and it stays: {whole:?}");
    // the orchestrator has no session to start again
    up(&mut s, Up::Intent { intent: Intent::Leave });
    let bound = f.front.binding("c2");
    say_up(&mut s, ":fresh");
    assert!(f.notes().contains(&"the orchestrator has no session of its own · :fresh is for a chat".to_string()));
    assert_eq!(f.front.binding("c2"), bound);
    assert_eq!(f.count("thread.fresh"), 1);
}

// ---- plans and asks ----

fn item(id: &str, place: &str, reads: &[&str], writes: &[&str], deps: &[&str]) -> Value {
    json!({ "id": id, "goal": format!("do {id}"), "reads": reads, "writes": writes, "depends": deps, "done": format!("{id} works"), "where": place })
}

const BIG: [&str; 3] = ["src/a/1.rs", "src/a/2.rs", "src/a/3.rs"];

#[test]
fn a_plans_branch_becomes_c1_1_under_its_chat_and_the_lane_row_is_its_door() {
    let plan = json!({ "approach": "Look, then change.", "items": [
        item("a", "scout", &BIG, &[], &[]),
        item("b", "scout", &["src/b/1.rs", "src/b/2.rs", "src/b/3.rs"], &[], &[]),
        item("go", "inline", &["src/a/1.rs"], &["src/out.rs"], &["a", "b"]),
    ] }).to_string();
    let main_lane = chatty("claude", &["hello", &plan, "went on"]);
    let branch = Scripted::with_caps("pi", j4n_core::doer::Caps { live: true, resume: true, brief: true, ..Default::default() }, vec![vec![Beat::Session("pi-s".into()), say("the retry is in fetch()"), end("the retry is in fetch()")], vec![Beat::Session("pi-s".into()), say("nothing"), end("nothing")]]);
    let f = fx_deciding(vec![main_lane, branch], Some("pi"), |c| {
        on(c);
        c.doer = Some("claude".into());
        c.lanes = Switch::Auto;
    });
    f.turn("hello");
    let Said::Started { task } = f.front.say("c1", ":plan do these: 1. look 2. look again 3. change") else { panic!("{}", f.rig.dump()) };
    f.rig.finished(&task);
    f.front.plan_collect("c1");
    f.front.plan_step("c1");
    let chats: Vec<(String, Option<String>)> = f.front.chats().into_iter().map(|c| (c.id, c.parent)).collect();
    assert_eq!(chats, [("c1".to_string(), None), ("c1.1".into(), Some("c1".into())), ("c1.2".into(), Some("c1".into()))], "{}", f.rig.dump());
    let b = f.front.chat("c1.1").unwrap();
    let branches: Vec<Row> = f.tasks().into_iter().filter(|r| r.spec.input.get("branch").is_some()).collect();
    assert_eq!((b.task.as_deref(), b.item.as_str(), b.title.as_str()), (Some(branches[0].id.as_str()), "a", "do a"));
    assert_eq!(f.front.chat_thread("c1.1").as_deref(), Some("c1"), "it has no thread of its own");
    // the list nests it under its chat
    let list = f.front.chat_list();
    assert_eq!(list.iter().map(|r| (r.key.as_str(), r.parent.as_deref())).collect::<Vec<_>>(), [("c1", None), ("c1.1", Some("c1")), ("c1.2", Some("c1"))]);
    // in the chat the lane says its door, and inside the branch chat the first row says where it came from, then the lane
    let mut s = serve(&f);
    let top = rows(&watch(&mut s, "chat:main"), "chat:main").to_vec();
    assert!(top.iter().all(|r| !branches.iter().any(|b| b.id == r.key)), "the orchestrator holds none of the lanes");
    let downs = up(&mut s, Up::Intent { intent: Intent::Enter { chat: "c1".into() } });
    let lane = rows(&downs, "chat:main").iter().find(|r| r.key == branches[0].id).expect("the lane");
    assert_eq!(lane.data.as_ref().unwrap()["lane"]["chat"], "c1.1");
    let downs = up(&mut s, Up::Intent { intent: Intent::Enter { chat: "c1.1".into() } });
    let h = here(&downs).unwrap();
    assert_eq!((h["crumb"][0]["id"].clone(), h["crumb"][1]["id"].clone()), (json!("c1"), json!("c1.1")), "the crumb is two deep");
    let words = said(&downs);
    assert_eq!(words.first().map(String::as_str), Some("from c1 · do a"), "{words:?}");
    assert!(rows(&downs, "chat:main").iter().any(|r| r.key == branches[0].id), "the lane as it is, and no other branch's");
    assert!(rows(&downs, "chat:main").iter().all(|r| r.key != branches[1].id));
    // the words that act on a chat are not a lane's: in a branch chat they are refused and the parent is not touched
    let before = f.rig.log.head();
    say_up(&mut s, ":clear");
    say_up(&mut s, ":fresh");
    say_up(&mut s, ":history");
    assert_eq!((f.count("chat.cleared"), f.count("thread.fresh"), f.count("chat.history")), (0, 0, 0), "{}", f.rig.dump());
    assert!(f.front.binding("c1").is_some(), "the parent's session is not ended");
    let refused = said(&feed(&mut s, &f, before));
    assert!(refused.contains(&"this is a lane of c1: say :clear in c1".to_string()) && refused.contains(&"this is a lane of c1: say :fresh in c1".to_string()), "{refused:?}");
    // out of a branch is its chat
    assert_eq!(here(&up(&mut s, Up::Intent { intent: Intent::Leave })).unwrap()["id"], "c1");
    for r in f.tasks() {
        f.rig.finished(&r.id);
    }
}

#[test]
fn a_question_raised_in_c2_shows_in_the_orchestrator_with_its_chat_and_is_answered_from_there() {
    let doer = Scripted::new("claude", vec![vec![Beat::Session("s".into()), say("a"), end("a")], vec![Beat::Session("s2".into()), Beat::Ask(choice("q", "keep the tabs?", &[("yes please", true), ("no", false)])), Beat::Hold, end("done")]]);
    let f = fx_deciding(vec![doer], Some("absent"), on);
    f.turn(PARSER);
    let Said::Started { task } = f.say_new("what does the budget gate check") else { panic!() };
    f.rig.until(&task, "waiting", |r| matches!(r.state, j4n_core::task::State::Waiting { .. }));
    assert_eq!(f.tasks().last().unwrap().spec.thread.as_deref(), Some("c2"));
    let mut s = serve(&f);
    watch(&mut s, CHAT_HERE);
    let Down::Asks { open } = watch(&mut s, "asks:open").remove(0) else { panic!() };
    assert_eq!((open.len(), open[0].question.as_str()), (1, "keep the tabs?"));
    assert_eq!(open[0].facts.first().map(|c| c.text.as_str()), Some("c2"), "it says whose it is: {:?}", open[0].facts);
    let list = watch(&mut s, CHATS);
    let waits = rows(&list, CHATS).iter().find(|r| r.key == "c2").unwrap();
    assert_eq!((waits.data.as_ref().unwrap()["state"].clone(), waits.cells[0].icon.as_str()), (json!("waits"), "ask"), "its mark turns to waits");
    assert_eq!(rows(&list, CHATS)[0].key, "c2", "the one that waits comes first");
    assert_eq!(here(&watch(&mut s, CHAT_HERE)).unwrap()["waits"], 1);
    // answered without going in
    up(&mut s, Up::Intent { intent: Intent::Answer { ask: open[0].ask.clone(), reply: Reply::Pick { id: "no".into() } } });
    f.rig.finished(&task);
    assert_eq!(f.rig.tasks.book().get(&task).unwrap().answer.as_deref(), Some("done"));
    assert_eq!(s.here(), "");
}

#[test]
fn a_bare_yes_in_the_orchestrator_answers_the_one_question_of_a_chat() {
    let doer = Scripted::new("claude", vec![vec![Beat::Session("s".into()), Beat::Ask(choice("q", "keep the tabs?", &[("yes", true), ("no", false)])), Beat::Hold, end("done")]]);
    let f = fx_deciding(vec![doer], Some("absent"), on);
    let Said::Started { task } = f.say(PARSER) else { panic!() };
    f.rig.until(&task, "waiting", |r| matches!(r.state, j4n_core::task::State::Waiting { .. }));
    let said = f.say("yes");
    assert!(matches!(said, Said::Answered { .. }), "{said:?}\n{}", f.rig.dump());
    f.rig.finished(&task);
    assert_eq!(f.count("chat.routed"), 1, "the yes was an answer, not a line to route");
}

#[test]
fn a_bare_yes_is_no_word_to_route_by_and_goes_through_jev_and_then_the_operator_without_the_switch() {
    // no word of a line routes it (`yes` included): with nothing found by files, Jev is asked, and with Jev off the operator is
    let f = fx_deciding(vec![claude(3)], Some("absent"), on);
    f.turn(PARSER);
    assert_eq!(f.say(":new"), Said::Enter("c2".into()), "an empty chat, newer than c1");
    assert!(matches!(f.say("yes"), Said::Asking { .. }), "the operator is asked which chat takes it: {}", f.rig.dump());
    assert!(of_chat(&f, "c2").is_empty());
}

#[test]
fn a_line_that_only_starts_with_a_word_of_a_reply_is_a_line_and_goes_through_jev_and_may_open_a_chat() {
    let f = fx_deciding(vec![claude(4)], Some("chats=new"), on);
    f.turn(PARSER);
    for (line, chat) in [("more tests for the gate", "c2"), ("go through the CI config", "c3"), ("ok, go on", "c4")] {
        let t = f.turn(line);
        assert_eq!(t.spec.thread.as_deref(), Some(chat), "{line}: {}", f.rig.dump());
        assert_eq!(routed(&f).last().unwrap()["by"], "decider:jev", "{line}");
    }
    assert_eq!(f.asked.lock().unwrap().iter().filter(|q| q.point == "chats").count(), 3, "Jev was asked for each: no word of a line is a reply");
}

// ---- the review of chats ----

#[test]
fn a_dropped_question_is_in_the_log_and_one_lost_with_a_restart_is_dropped_and_said_with_its_line() {
    let f = fx_deciding(vec![claude(4)], Some("absent"), on);
    f.turn(PARSER);
    f.turn(LEXER);
    assert!(matches!(f.say(BOTH), Said::Asking { .. }));
    assert_eq!(f.count("chat.route.dropped"), 0);
    // a new line drops it: the Log says so, with the question and the line
    assert!(matches!(f.say_new("what does the budget gate check"), Said::Started { .. }));
    let dropped = f.rig.of_kind("chat.route.dropped");
    assert_eq!((dropped.len(), dropped[0].data["ask"].as_str(), dropped[0].data["why"].as_str(), dropped[0].data["line"].as_str()), (1, Some("route:2"), Some("a new line came"), Some(BOTH)));
    // `no` drops it too
    assert!(matches!(f.say(BOTH), Said::Asking { .. }));
    let mut s = serve(&f);
    up(&mut s, Up::Intent { intent: Intent::Answer { ask: "route:4".into(), reply: Reply::No } });
    assert_eq!(f.rig.of_kind("chat.route.dropped").last().unwrap().data["why"], "you said no");
    // answered, it is not dropped
    assert!(matches!(f.say(BOTH), Said::Asking { .. }));
    let said = f.say("1");
    assert!(matches!(said, Said::Started { .. }), "{said:?}");
    assert_eq!(f.count("chat.route.dropped"), 2);
    // a question that waits when the process stops is not in the next one's memory: the Log says it was lost, once, and the line is said
    assert!(matches!(f.say(BOTH), Said::Asking { .. }));
    let g = crate::front_cases::fx_reflexes_again(&f, vec![claude(2)], Some("absent"), j4n_core::front::screen::Screen::builtin(), on);
    assert!(g.front.route_asks().is_empty(), "nothing holds it in memory");
    g.front.route_resume();
    g.front.route_resume();
    let dropped = g.rig.of_kind("chat.route.dropped");
    assert_eq!((dropped.len(), dropped[2].data["why"].as_str(), dropped[2].data["line"].as_str()), (3, Some("lost with a restart"), Some(BOTH)));
    assert!(g.notes().iter().any(|n| n.contains("was lost with the restart: type the line again") && n.contains("check that parser.rs")), "{:?}", g.notes());
    // a question that is held here is not taken for a lost one (a second screen starting)
    assert!(matches!(g.say(BOTH), Said::Asking { .. }));
    g.front.route_resume();
    assert_eq!(g.rig.of_kind("chat.route.dropped").len(), 3);
}

#[test]
fn clear_twice_says_nothing_to_clear_and_a_row_is_one_row() {
    let f = fx_deciding(vec![claude(2)], Some("absent"), on);
    f.turn(PARSER);
    f.front.say("c1", ":clear");
    // the four rows of the turn: the `:clear` line that said it is not one of them
    let cleared: Vec<String> = f.notes().into_iter().filter(|n| n.starts_with("cleared ")).collect();
    assert_eq!(cleared, ["cleared 4 rows · the Log keeps them · ⏎ brings them back (`:history`)"], "{:?}", f.notes());
    // what the first one said is not a row to clear
    f.front.say("c1", ":clear");
    f.front.say("c1", ":clear");
    let notes = f.notes();
    assert_eq!(notes.iter().filter(|n| n.starts_with("cleared ")).count(), 1, "{notes:?}");
    assert_eq!(notes.iter().filter(|n| *n == "nothing to clear").count(), 2, "{notes:?}");
    assert_eq!(f.count("chat.cleared"), 1);
    // one row is `1 row`
    f.rig.log.append(j4n_core::log::New::of("chat.say", j4n_core::change::Actor::Operator, json!({ "text": "a line that was said" })).thread("c1")).unwrap();
    f.front.say("c1", ":clear");
    assert!(f.notes().last().unwrap().starts_with("cleared 1 row · the Log keeps them"), "{:?}", f.notes());
}

#[test]
fn a_number_typed_with_a_route_question_and_a_chats_own_question_open_answers_the_route_first() {
    let ask = vec![Beat::Session("s3".into()), Beat::Ask(choice("q", "keep the tabs?", &[("yes", true), ("no", false)])), Beat::Hold, end("done")];
    let one = |s: &str| vec![Beat::Session(s.into()), say("ok"), end("ok")];
    let f = fx_deciding(vec![Scripted::new("claude", vec![one("s1"), one("s2"), ask, one("s4")])], Some("absent"), on);
    f.turn(PARSER);
    f.turn(LEXER);
    let Said::Started { task: waiting } = f.say("rename more in lexer.rs") else { panic!("{}", f.rig.dump()) };
    f.rig.until(&waiting, "waiting", |r| matches!(r.state, j4n_core::task::State::Waiting { .. }));
    assert_eq!(f.tasks().last().unwrap().spec.thread.as_deref(), Some("c2"));
    assert!(matches!(f.say(BOTH), Said::Asking { .. }), "{}", f.rig.dump());
    // `1` is the first option of the route question (c1), not the first answer of c2's question
    let said = f.say("1");
    let Said::Started { task } = said else { panic!("{said:?}\n{}", f.rig.dump()) };
    assert_eq!(f.rig.tasks.book().get(&task).unwrap().spec.thread.as_deref(), Some("c1"));
    assert_eq!(f.rig.tasks.book().open_asks().len(), 1, "c2's question is still open");
    f.rig.finished(&task);
    // with none waiting, the number answers the chat's question as it did
    assert!(matches!(f.say("1"), Said::Answered { .. }));
    f.rig.finished(&waiting);
}

// ---- Jev is off or not sure, a line that names a chat is asked, never opened silently as a new chat ----

/// Two chats with the Decider off: the second one was chosen at the question, which the first line of a second subject now is.
fn two_chats_models_off() -> Fx {
    let f = fx_deciding(vec![claude(4)], Some("absent"), on);
    f.turn(PARSER);
    let Said::Asking { ask } = f.say(LEXER) else { panic!("{}", f.rig.dump()) };
    let Said::Started { task } = f.front.reply("main", &ask, Reply::Pick { id: "new".into() }) else { panic!("{}", f.rig.dump()) };
    f.rig.finished(&task);
    assert_eq!(f.count("chat.opened"), 2);
    f
}

#[test]
fn with_the_decider_off_a_line_naming_c1_is_a_routing_question_with_c1_recommended_and_the_log_says_why() {
    let f = two_chats_models_off();
    assert!(!f.front.decider_on());
    let Said::Asking { ask } = f.say("finish the left over parts in c1") else { panic!("a line that names a chat opened one: {}", f.rig.dump()) };
    assert_eq!(f.count("chat.opened"), 2, "no chat was opened behind the operator's back");
    let asked = f.rig.of_kind("chat.route.asked").pop().expect("the question is in the Log").data;
    assert_eq!((asked["why"].as_str(), asked["hint"].as_str()), (Some("Decider off"), Some("c1")));
    assert_eq!(asked["options"], json!(["c1", "c2", "new"]));
    let view = f.front.route_asks().remove(0);
    let marked: Vec<(&str, bool, By)> = view.options.iter().map(|o| (o.id.as_str(), o.recommended, o.by)).collect();
    assert_eq!(marked, [("c1", true, By::Rule), ("c2", false, By::Operator), ("new", false, By::Operator)]);
    assert!(view.options[0].note.contains("claude"), "each chat says its doer: {:?}", view.options[0]);
    // the answer is the operator's, and the Log keeps what Jev could not say
    let said = f.front.reply("main", &ask, Reply::Pick { id: "c1".into() });
    assert!(matches!(said, Said::Started { .. }), "{said:?}");
    let r = routed(&f).pop().unwrap();
    assert_eq!((r["to"].as_str(), r["by"].as_str(), r["detail"].as_str()), (Some("c1"), Some("you"), Some("Decider off")));
}

#[test]
fn a_new_chat_chosen_at_the_question_says_decider_off_and_the_row_leads_to_the_deciders() {
    let f = two_chats_models_off();
    let Said::Asking { ask } = f.say("make arrays tolerate dangling punctuation") else { panic!() };
    f.front.reply("main", &ask, Reply::Pick { id: "new".into() });
    let r = routed(&f).pop().unwrap();
    assert_eq!((r["why"].as_str(), r["detail"].as_str(), r["new"].clone()), (Some("new chat · Decider off"), Some("Decider off"), json!(true)));
    assert!(!r["detail"].as_str().unwrap().contains("no operator"), "{r}");
    let row = f.notes().into_iter().rfind(|n| n.starts_with("→")).unwrap();
    assert_eq!(row, "→ c3 · new chat · Decider off · ⏎ opens Agents › Deciders");
}

#[test]
fn jev_saying_new_opens_a_chat_without_a_question_and_jev_that_is_off_with_no_chat_does_too() {
    let f = fx_deciding(vec![claude(3)], Some("chats=new"), on);
    f.turn(PARSER);
    assert!(f.front.decider_on());
    let t = f.turn("make arrays tolerate that dangling punctuation");
    assert_eq!(t.spec.thread.as_deref(), Some("c2"), "Jev was sure it is new");
    let f = fx_deciding(vec![claude(1)], Some("absent"), on);
    assert_eq!(f.turn("finish the rest of c1").spec.thread.as_deref(), Some("c1"), "no chat to join: a new one");
}
