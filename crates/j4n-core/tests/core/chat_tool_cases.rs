//! Main's tool `chat`, through the real front and the post's mailbox: a call made while Main's turn is at work starts a chat that only reads, at once, and its digest comes
//! with Main's next turn; a call that asks to write, one that comes from a chat, one past the cap, one the money does not reach and one outside the word are refused with
//! the reason and start nothing. A doer that is offered the tool is told so on the first line of its session, and no other is.

use crate::branch_validate::{A, CLEAN, Held, Rig3, caps, checker, doer};
use crate::front_cases::{Fx, chatty, fx_placed};
use crate::scripted::{Beat, Scripted, end, say};
use j4n_core::branch::Switch;
use j4n_core::front::messages::Mailbox;
use j4n_core::front::scout::{TOOL_CLI, Want};
use j4n_core::front::Cfg;
use serde_json::Value;
use std::sync::Arc;

const ONE: &str = r#"{"decision":"one","chats":[],"features":[],"why":"a question"}"#;
const LINE: &str = "find out how the lexer handles a nested comment";
const REPORT: &str = r#"{"answer":"it counts the depth","found":[],"changed":"","ran":[],"sure":"high","open":[]}"#;

fn settings(c: &mut Cfg) {
    c.doer = Some("claude".into());
    c.lanes = Switch::Orchestrate;
    c.chats.screen = true;
    c.orchestrate.look = true;
    c.orchestrate.doer = "zcheap".into();
    c.orchestrate.scouts = vec!["claude".into()];
}

/// Main works a long turn (so a call is made while it is at work); `pi` answers a chat after a short pause, so a chat is still at work for a moment.
fn rig(tune: impl FnOnce(&mut Cfg)) -> (Rig3, Arc<Mailbox>, Arc<Scripted>) {
    let held = Held::new();
    let main = Scripted::new("claude", vec![vec![Beat::Session("claude-s".into()), Beat::Pause(1500), end("main worked")]; 4]);
    let pi = Scripted::with_caps("pi", caps(), vec![vec![Beat::Session("pi-s".into()), Beat::Pause(1200), say(REPORT), end(REPORT)]; 4]);
    let doers = vec![main.clone(), pi, doer("own", &[CLEAN; 4]), checker(2), chatty("zcheap", &[ONE; 4])];
    let f: Fx = fx_placed(doers, Some("pi"), vec![held.place()], |c| {
        settings(c);
        tune(c);
    });
    let mailbox = Arc::new(Mailbox::default());
    f.front.clone().with_post(&mailbox);
    (Rig3 { f, held }, mailbox, main)
}

fn want(goal: &str) -> Want {
    Want { goal: goal.into(), paths: A.iter().map(|p| p.to_string()).collect(), doer: "pi".into(), model: String::new(), write: false }
}

/// Main's turn, started and still at work.
fn main_turn(r: &Rig3) -> String {
    main_turn_of(r, LINE)
}

fn main_turn_of(r: &Rig3, line: &str) -> String {
    match r.f.front.say("main", line) {
        j4n_core::front::Said::Started { task } => task,
        other => panic!("{other:?}\n{}", r.f.rig.dump()),
    }
}

fn scouts(r: &Rig3) -> Vec<Value> {
    r.f.rig.of_kind("orchestrate.scout").into_iter().map(|e| e.data).collect()
}

fn chats_started(r: &Rig3) -> usize {
    r.f.rig.of_kind("chat.briefed").len()
}

#[test]
fn a_scout_started_in_a_turn_of_main_returns_at_once_and_its_digest_comes_with_the_next_turn() {
    let (r, mail, main) = rig(|_| {});
    let task = main_turn(&r);
    let said = mail.get().expect("the front put its post").scout(&task, &want("how does the lexer count a nested comment")).unwrap_or_else(|e| panic!("{e}\n{}", r.f.rig.dump()));
    assert!(said.starts_with("c1 is started on pi") && said.contains("do not wait"), "{said}");
    assert!(!r.f.rig.tasks.book().get(&task).unwrap().state.is_terminal(), "the call returned while Main's turn was still at work");
    let log = scouts(&r);
    assert_eq!((log.len(), log[0]["chat"].as_str(), log[0]["doer"].as_str()), (1, Some("c1"), Some("pi")), "{log:?}");
    // the chat is a scout of Main's plan, read-only, and it is carried to its end as a split's chats are
    let briefed = r.f.rig.of_kind("chat.briefed");
    assert_eq!((briefed[0].data["mode"].as_str(), briefed[0].data["writes"].as_array().map(Vec::len)), (Some("read"), Some(0)), "{}", briefed[0].data);
    r.until("the scout ended", |r| r.f.keeper.plan("main").and_then(|p| p.items.into_iter().find(|i| i.id == "c1")).is_some_and(|i| i.done()));
    assert_eq!(r.item("c1").state, "reviewed", "a scout has nothing to merge");
    let digest = r.f.rig.of_kind("orchestrate.digest");
    assert_eq!(digest.len(), 1, "{}", r.f.rig.dump());
    assert!(digest[0].data["text"].as_str().unwrap().starts_with("c1 "), "{}", digest[0].data);
    // Main is not woken for it; its next turn is shown the digest
    assert!(r.f.tasks().iter().all(|t| !t.spec.goal.starts_with("J4n:")), "no turn of Main was started for a finished scout");
    let next = main_turn_of(&r, "and what did the scout find?");
    r.f.rig.finished(&next);
    let told = main.starts().last().unwrap().text.clone().unwrap_or_default();
    assert!(told.contains("c1 ") && told.contains("scout reviewed"), "Main's next turn carries the digest: {told}");
}

#[test]
fn a_call_that_asks_to_write_is_refused_and_starts_no_chat() {
    let (r, mail, _) = rig(|_| {});
    let task = main_turn(&r);
    let post = mail.get().unwrap();
    let asked = Want { write: true, ..want("rewrite the lexer") };
    let why = post.scout(&task, &asked).expect_err("a write is refused");
    assert!(why.contains("only reads") && why.contains("plan"), "{why}");
    assert_eq!(chats_started(&r), 0, "{}", r.f.rig.dump());
    assert_eq!(scouts(&r)[0]["refused"].as_str(), Some(why.as_str()), "the Log says it was refused, and why");
    r.f.rig.finished(&task);
}

#[test]
fn a_chat_does_not_start_a_chat_and_the_cap_counts_the_chats_at_work() {
    let (r, mail, _) = rig(|c| c.orchestrate.chats = 2);
    let task = main_turn(&r);
    let post = mail.get().unwrap();
    post.scout(&task, &want("the first read")).expect("the first is started");
    // the chat's own turn is a turn of a chat: it may not start another, whatever the cap
    let own = r.f.tasks().into_iter().find(|t| t.spec.thread.as_deref() == Some("c1") && t.spec.role.as_deref() == Some("work")).expect("the chat's turn");
    let why = post.scout(&own.id, &want("a read of its own")).expect_err("depth is one");
    assert!(why.contains("a chat does not start another"), "{why}");
    post.scout(&task, &want("the second read")).expect("the second is started");
    let why = post.scout(&task, &want("a third read")).expect_err("the cap");
    assert!(why.contains("2 chats are at work already") && why.contains("orchestrate.chats"), "{why}");
    assert_eq!(chats_started(&r), 2, "{}", r.f.rig.dump());
    for t in r.f.tasks() {
        r.f.rig.finished(&t.id);
    }
    // a chat that is over leaves room
    post.scout(&task, &want("a third read, now")).expect("room again");
    assert_eq!(chats_started(&r), 3);
}

#[test]
fn the_money_of_the_line_is_given_once_across_calls() {
    let (r, mail, _) = rig(|c| c.usd_line = 0.05);
    let task = main_turn(&r);
    let post = mail.get().unwrap();
    post.scout(&task, &want("the first read")).expect("the first is given what the line may spend");
    let why = post.scout(&task, &want("the second read")).expect_err("what is left is too little");
    assert!(why.contains("too little"), "{why}");
    assert_eq!(chats_started(&r), 1);
    for t in r.f.tasks() {
        r.f.rig.finished(&t.id);
    }
}

#[test]
fn a_call_outside_the_word_or_for_a_doer_j4n_lacks_or_under_a_card_for_every_chat_is_refused() {
    let (r, mail, _) = rig(|_| {});
    let task = main_turn(&r);
    let post = mail.get().unwrap();
    let why = post.scout(&task, &Want { doer: "nobody".into(), ..want("a read") }).expect_err("an unknown doer");
    assert!(why.contains("`nobody` is not a doer J4n has"), "{why}");
    let why = post.scout(&task, &Want { paths: vec!["src/a/1.rs".into()], ..want("a read") }).expect_err("one small file is a read for Main, not a chat");
    assert!(why.contains("yourself") && why.contains("less than the"), "{why}");
    let why = post.scout(&task, &Want { paths: vec!["/etc/passwd".into()], ..want("a read") }).expect_err("a path outside the project");
    assert!(why.contains("not a path inside the project"), "{why}");
    let why = post.scout(&task, &Want { goal: "delete all the old logs with rm -rf".into(), ..want("") }).expect_err("destruction the line did not say");
    assert!(why.contains("rm -rf"), "{why}");
    r.f.rig.finished(&task);
    let (r, mail, _) = rig(|c| c.orchestrate.ask = "always".into());
    let task = main_turn(&r);
    let why = mail.get().unwrap().scout(&task, &want("a read")).expect_err("every chat waits for a yes");
    assert!(why.contains("orchestrate.ask"), "{why}");
    r.f.rig.finished(&task);
    let (r, mail, _) = rig(|c| c.lanes = Switch::Single);
    let task = main_turn(&r);
    let why = mail.get().unwrap().scout(&task, &want("a read")).expect_err("not under the word");
    assert!(why.contains("`orchestrate`"), "{why}");
    assert_eq!(chats_started(&r), 0);
    r.f.rig.finished(&task);
}

#[test]
fn a_doer_that_is_offered_the_tool_is_told_so_on_the_first_line_and_no_other_is() {
    let (r, _, claude) = rig(|_| {});
    let task = main_turn(&r);
    r.f.rig.finished(&task);
    let first = claude.starts()[0].text.clone().unwrap_or_default();
    assert!(first.contains(TOOL_CLI), "Main's doer is told: {first}");
    // not told again in the session it carries on in, and not told where the tool is not offered
    let next = main_turn_of(&r, "and what about the parser?");
    r.f.rig.finished(&next);
    assert!(!claude.starts().last().unwrap().text.clone().unwrap_or_default().contains(TOOL_CLI), "once, on the first line");
    let (r, _, claude) = rig(|c| c.orchestrate.scouts = Vec::new());
    let task = main_turn(&r);
    r.f.rig.finished(&task);
    assert!(!claude.starts()[0].text.clone().unwrap_or_default().contains("j4n.call('chat'"), "a doer the tool is not offered to is not told of it");
    let (r, _, claude) = rig(|c| c.lanes = Switch::Single);
    let task = main_turn(&r);
    r.f.rig.finished(&task);
    assert!(!claude.starts()[0].text.clone().unwrap_or_default().contains("j4n.call('chat'"), "under the other words nothing of it is said");
}
