//! What the crew's pages are given by the core: a chat's seat in the list of chats, the settings of a chat as rows with who set each and its undo (and the operator's write of a
//! dotted one), and the charters that were signed with where each stands. Each is nothing while the crew is off.

use crate::front_cases::{Fx, fx};
use crate::scripted::{Scripted, end};
use j4n_core::branch::Switch;
use j4n_core::change::Actor;
use j4n_core::clock::Clock;
use j4n_core::crew::{Crew, Writer};
use j4n_core::front::Cfg;
use j4n_core::front::serve::Serve;
use j4n_core::log::New;
use j4n_core::value::Value;
use j4n_proto::{Intent, Up};
use serde_json::json;

fn crewed(on: bool) -> Fx {
    let f = fx(vec![Scripted::new("alpha", vec![vec![end("ok")]])], |c: &mut Cfg| {
        c.lanes = Switch::Orchestrate;
        c.chats.screen = true;
        c.crew = Crew::shipped();
        c.crew.on = on;
        c.doer = Some("alpha".into());
    });
    f.front.say("main", ":new look at the loop");
    f
}

fn seated(f: &Fx, chat: &str, seat: &str) {
    f.rig.log.append(New::of("chat.seated", Actor::Core, json!({ "seat": seat, "role": seat, "reason": "the brief", "narrowed": [] })).thread(chat)).unwrap();
    f.rig.log.sync();
}

fn field(rows: &[Value], id: &str, key: &str) -> Option<String> {
    rows.iter().find_map(|r| match r {
        Value::Map(m) if m.get("id") == Some(&Value::text(id)) => match m.get(key) {
            Some(Value::Str(w)) => Some(w.clone()),
            _ => None,
        },
        _ => None,
    })
}

#[test]
fn a_chat_that_sits_in_a_seat_says_so_in_the_list_before_its_doer_and_with_the_crew_off_it_does_not() {
    for (on, says) in [(true, true), (false, false)] {
        let f = crewed(on);
        seated(&f, "c1", "reviewer");
        assert_eq!(f.front.chat("c1").and_then(|c| c.seat).as_deref(), Some("reviewer"), "the fold keeps the seat the core wrote");
        let row = f.front.chat_list().into_iter().find(|r| r.key == "c1").expect("a row");
        let who = &row.cells[3].text;
        assert_eq!(who.starts_with("reviewer"), says, "{on}: {who:?}");
        assert_eq!(row.data.as_ref().unwrap()["seat"], if says { json!("reviewer") } else { json!(null) });
    }
    // a chat with no seat has the words it had
    let f = crewed(true);
    let row = f.front.chat_list().into_iter().find(|r| r.key == "c1").expect("a row");
    assert!(!row.cells[3].text.contains(" · ") || !row.cells[3].text.starts_with("reviewer"), "{:?}", row.cells[3].text);
    assert_eq!(row.data.as_ref().unwrap()["seat"], json!(null));
}

#[test]
fn a_chats_settings_are_a_row_for_each_key_a_chat_may_hold_with_who_set_it_and_the_undo_of_that_change() {
    let f = crewed(true);
    let held = f.front.chatset_values("c1");
    let keys: Vec<String> = held.iter().filter_map(|r| field(std::slice::from_ref(r), "model", "id").or_else(|| match r { Value::Map(m) => match m.get("id") { Some(Value::Str(s)) => Some(s.clone()), _ => None }, _ => None })).collect();
    assert_eq!(keys, ["model", "effort", "tools.allow", "tools.deny", "personality"]);
    assert!(held.iter().all(|r| matches!(r, Value::Map(m) if m.get("by") == Some(&Value::text("")) && m.get("undo") == Some(&Value::text("")) && m.get("now") == Some(&Value::text("not set")))), "nothing is set yet");
    // the seat wrote its words, the operator an effort: each row names its writer, and the undo is the change that an undo takes back
    f.front.chat_set("c1", "personality", &json!("read, and say where"), &Writer::Seat).unwrap();
    let id = f.front.chat_set("c1", "effort", &json!("high"), &Writer::Operator).unwrap();
    let held = f.front.chatset_values("c1");
    assert_eq!((field(&held, "personality", "by").as_deref(), field(&held, "personality", "now").as_deref()), (Some("seat"), Some("read, and say where")));
    assert_eq!((field(&held, "effort", "by").as_deref(), field(&held, "effort", "now").as_deref(), field(&held, "effort", "undo").as_deref()), (Some("you"), Some("high"), Some(id.as_str())));
    // the editor of a dotted key binds the chat and the key as one, and an effort is a choice
    let edit = held.iter().find_map(|r| match r {
        Value::Map(m) if m.get("id") == Some(&Value::text("tools.deny")) => m.get("edit").cloned(),
        _ => None,
    });
    assert!(matches!(&edit, Some(Value::Map(e)) if e.get("bind") == Some(&Value::text("chat:c1.tools.deny")) && e.get("kind") == Some(&Value::text("str"))), "{edit:?}");
    // a taken-back change leaves the key unset again
    f.front.chat_undo(&id, &Writer::Operator).unwrap();
    assert_eq!(field(&f.front.chatset_values("c1"), "effort", "now").as_deref(), Some("not set"));
}

#[test]
fn a_dotted_setting_is_written_by_the_screen_as_the_chat_and_the_first_word_then_the_last_and_a_refused_one_writes_nothing() {
    let f = crewed(true);
    let mut s = Serve::new(f.front.clone());
    for (id, field, value) in [("c1.tools", "deny", json!("Bash")), ("c1", "effort", json!("max")), ("c1", "route.on", json!(true)), ("c1", "effort", json!("loud"))] {
        s.up(Up::Intent { intent: Intent::Set { kind: "chat".into(), id: id.into(), field: field.into(), value } });
    }
    let held = f.front.chat_layer("c1");
    let word = |k: &str| held.iter().find(|(h, _)| h == k).map(|(_, v)| v.to_string());
    assert_eq!((word("tools.deny").as_deref(), word("effort").as_deref()), (Some("[\"Bash\"]"), Some("\"max\"")), "{held:?}");
    assert_eq!(held.len(), 2, "a machine-wide key and a word that is no effort were refused: {held:?}");
}

#[test]
fn a_chats_settings_page_has_rows_only_for_a_chat_of_its_own_and_only_while_the_crew_is_on() {
    assert!(crewed(false).front.chatset_values("c1").is_empty());
    assert!(crewed(true).front.chatset_values("c9").is_empty(), "no such chat");
    assert!(crewed(true).front.chatset_values("main").is_empty(), "Main is no chat of its own");
    // a branch of a chat is a view of one task and holds no settings of its own
    let f = crewed(true);
    f.rig.log.append(New::of("chat.opened", Actor::Core, json!({ "parent": "c1", "title": "a branch", "task": "t1", "item": "i1" })).thread("c1.1")).unwrap();
    f.rig.log.sync();
    assert!(f.front.chat("c1.1").is_some_and(|c| c.parent.is_some()) && f.front.chatset_values("c1.1").is_empty());
    assert!(!f.front.chatset_values("c1").is_empty());
}

#[test]
fn the_charters_that_were_signed_say_where_each_stands_and_what_its_tree_spent_of_what_it_may() {
    let f = crewed(true);
    let sign = |id: &str, until_ms: Option<u64>| {
        let c = j4n_core::charter::Charter { id: id.into(), goal: format!("goal of {id}"), watch: String::new(), watch_every_min: 0, paths: vec![format!("docs/{id}")], may: vec!["read".into()], doers: vec![], usd: 2.0, until_ms, thread: format!("c{id}"), verbatim: vec![] };
        f.rig.log.append(New::of(j4n_core::charter::SIGNED, Actor::Operator, json!({ "charter": c, "proposal": "p1" }))).unwrap();
    };
    sign("k1", None);
    sign("k2", Some(f.rig.clock.now_ms() + 60_000));
    sign("k3", None);
    f.rig.log.append(New::of(j4n_core::charter::ENDED, Actor::Operator, json!({ "id": "k3" }))).unwrap();
    f.rig.log.sync();
    // k2 is live until its time, and then it is over
    assert_eq!(field(&f.front.charter_values(), "k2", "state").as_deref(), Some("live"));
    f.rig.clock.advance(120_000);
    let rows = f.front.charter_values();
    assert_eq!(rows.len(), 3, "{rows:?}");
    assert_eq!((field(&rows, "k1", "state").as_deref(), field(&rows, "k2", "state").as_deref(), field(&rows, "k3", "state").as_deref()), (Some("live"), Some("over"), Some("ended")));
    assert_eq!(field(&rows, "k1", "spent").as_deref(), Some("$0.00 of $2.00"), "no tasks: nothing spent, and never a minus sign");
    assert_eq!((field(&rows, "k1", "until").as_deref(), field(&rows, "k1", "paths").as_deref()), (Some("until you end it"), Some("docs/k1")));
    assert!(field(&rows, "k3", "why").is_some_and(|w| !w.is_empty()), "an ended charter says why");
}
