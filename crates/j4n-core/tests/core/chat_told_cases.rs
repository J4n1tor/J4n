//! A doer is told that it is chat `cN` of J4n in this project and which the other chats are, on the first line of its session and on
//! a line that asks about chats. A line carried on in a session can have an empty brief (`sections: []`), and the model then looks for
//! the chats in the program that runs it.

use crate::front_cases::{Fx, fx_deciding};
use crate::scripted::{Beat, Scripted, end, say};
use j4n_core::doer::Caps;
use j4n_core::front::{Cfg, Said};
use j4n_core::task::Row;
use serde_json::Value;
use std::sync::Arc;

fn on(c: &mut Cfg) {
    c.chats.on = true;
}

fn doer() -> Arc<Scripted> {
    let turns = (0..6).map(|i| vec![Beat::Session("claude-s".into()), say(&format!("answer {i}")), end(&format!("answer {i}"))]).collect();
    Scripted::with_caps("claude", Caps { live: true, resume: true, brief: true, ..Caps::default() }, turns)
}

fn brief(r: &Row) -> String {
    r.spec.input.get("brief").and_then(Value::as_str).unwrap_or_default().to_string()
}

/// The sections of the newest brief written in `chat` and whether its line carried on in a session.
fn sections(f: &Fx, chat: &str) -> (Vec<String>, bool) {
    let seen = f.rig.of_kind("brief.rendered").into_iter().rfind(|e| e.thread.as_deref() == Some(chat)).expect("a brief was rendered");
    (seen.data["sections"].as_array().unwrap().iter().filter_map(|s| s.as_str().map(String::from)).collect(), seen.data["in_session"] == true)
}

fn said(s: Said) -> String {
    match s {
        Said::Started { task } => task,
        other => panic!("not started: {other:?}"),
    }
}

#[test]
fn the_first_line_of_a_chat_says_which_chat_it_is_and_which_the_others_are_and_a_going_line_says_it_only_when_it_asks_about_chats() {
    let f = fx_deciding(vec![doer()], Some("absent"), on);
    let c1 = f.turn("fix trailing commas in parser.rs");
    assert_eq!(c1.spec.thread.as_deref(), Some("c1"));
    let b = brief(&c1);
    assert!(b.contains("You are chat c1 of J4n in this project") && b.contains("There is no other chat yet."), "{b}");
    let c2 = f.turn("what does the budget gate check");
    assert_eq!(c2.spec.thread.as_deref(), Some("c2"), "{}", f.rig.dump());
    let b = brief(&c2);
    assert!(b.contains("You are chat c2 of J4n in this project") && b.contains("The other chats here: c1 \"fix trailing commas in parser.rs\"."), "{b}");
    assert!(b.contains("not J4n's chats"), "it says the sessions of the program that runs it are no chats of J4n: {b}");
    assert!(!b.contains("c2 \""), "a chat is not listed among its own others: {b}");
    // a going line (the session is carried on) that is no question about chats has no such line, as before
    let plain = said(f.front.say("c2", "run the tests"));
    f.rig.finished(&plain);
    let (got, going) = sections(&f, "c2");
    assert!(going && !got.contains(&"chats".to_string()), "{got:?}");
    // a going line that asks about chats has it, whatever words: `chats`, `sub chats`, an id
    for line in ["can you see other sub chats?", "what did c1 do", "list my chats"] {
        let task = said(f.front.say("c2", line));
        let row = f.rig.finished(&task);
        let (got, going) = sections(&f, "c2");
        assert!(going && got.contains(&"chats".to_string()), "{line}: {got:?}");
        assert!(brief(&row).contains("You are chat c2 of J4n") && brief(&row).contains("c1 \"fix trailing commas"), "{line}: {}", brief(&row));
    }
}

#[test]
fn the_line_is_within_the_project_and_small() {
    let f = fx_deciding(vec![doer()], Some("absent"), on);
    f.turn("fix trailing commas in parser.rs");
    let b = brief(&f.turn("what does the budget gate check"));
    assert!(j4n_core::brief::tokens(&b) < 120, "{} tokens: {b}", j4n_core::brief::tokens(&b));
    assert!(j4n_core::brief::about_chats("sub chats?") && j4n_core::brief::about_chats("ask c12") && !j4n_core::brief::about_chats("run the tests in crates/c4"));
}
