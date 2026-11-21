//! What the composer's strip is sent. The orchestrator has none while the chats are on, and the
//! strip of a chat carries the plan's count for the screen's thin line. The screen's side (hidden when empty, one mark with a count, the group at the right edge) is
//! `j4n-tui`'s `chat_cases`.

use crate::front_cases::{fx, fx_trusting};
use crate::scripted::{Beat, Scripted, end, say};
use j4n_core::branch::Switch;
use j4n_core::doer::Caps;
use j4n_core::front::Said;
use j4n_core::front::serve::Serve;
use j4n_proto::{COMPOSER_STRIP, Down, Up};
use serde_json::{Value, json};

fn strip_of(downs: &[Down]) -> Option<Value> {
    downs.iter().rev().find_map(|d| match d {
        Down::Value { bind, value, .. } if bind == COMPOSER_STRIP => Some(value.clone()),
        _ => None,
    })
}

#[test]
fn the_orchestrator_has_no_strip_while_the_chats_are_on_and_the_one_thread_has_it_while_they_are_off() {
    let one = || vec![Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Served("claude-sonnet-5-5", "anthropic"), say("a"), end("a")]])];
    let on = fx(one(), |c| {
        c.model = Some("sonnet".into());
        c.chats.on = true;
    });
    let mut s = Serve::new(on.front.clone());
    let (downs, _) = s.up(Up::Watch { bind: COMPOSER_STRIP.into() });
    assert_eq!(strip_of(&downs), Some(Value::Null), "the orchestrator before its first line: {downs:?}");
    on.turn("hello");
    let (downs, _) = s.up(Up::Watch { bind: COMPOSER_STRIP.into() });
    assert_eq!(strip_of(&downs), Some(Value::Null), "and after it, however much work or cost there is: {downs:?}");
    // with the switch off the one thread is the chat, and its strip is its own
    let off = fx(one(), |c| c.model = Some("sonnet".into()));
    let mut s = Serve::new(off.front.clone());
    let (downs, _) = s.up(Up::Watch { bind: COMPOSER_STRIP.into() });
    assert_eq!(strip_of(&downs).expect("a strip")["route"], json!("no doer yet"), "{downs:?}");
}

fn three() -> String {
    let items: Vec<_> = ["a", "b", "c"].iter().map(|id| json!({ "id": id, "goal": format!("do {id}"), "reads": [], "writes": [], "depends": [], "done": format!("{id} works"), "where": "inline" })).collect();
    json!({ "approach": "Do a, b, then c.", "items": items }).to_string()
}

#[test]
fn the_strip_counts_the_plans_items_done_and_all_for_the_thin_line() {
    let planning = vec![Beat::Session("sess".into()), say(&three()), end(&three())];
    let doer = Scripted::with_caps("claude", Caps { live: true, resume: true, ..Caps::default() }, vec![planning]);
    let f = fx_trusting(vec![doer], Some("intake=flow"), |c| {
        c.doer = Some("claude".into());
        c.lanes = Switch::Auto;
    });
    let mut s = Serve::new(f.front.clone());
    let (downs, _) = s.up(Up::Watch { bind: COMPOSER_STRIP.into() });
    assert!(strip_of(&downs).is_none_or(|v| v.get("plan").is_none()), "no plan yet: {downs:?}");
    let Said::Started { task } = f.say("plan how to do a, b and c, then do them") else { panic!("{}", f.rig.dump()) };
    f.rig.finished(&task);
    f.front.plan_collect("main");
    let (downs, _) = s.up(Up::Watch { bind: COMPOSER_STRIP.into() });
    let strip = strip_of(&downs).expect("a strip");
    assert_eq!(strip["plan"], json!({ "done": 0, "total": 3 }), "{strip}");
}
