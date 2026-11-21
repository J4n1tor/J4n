//! A chat's own settings (`crew.on`): which settings it may hold and who may write them, a write that is a `Change` with its writer and an undo, the layer
//! kept by the Log and gone with the archive, and what a chat's turns take from it: its effort, its model, its tool list and its seat's words on every turn.

use crate::branch_validate::{A, Held, caps};
use crate::front_cases::{Fx, chatty, fx_placed};
use crate::scripted::{Beat, Scripted, end};
use crate::split_cases::{LOOK, answer, brief, chat_tasks, events, say, settings, settle};
use j4n_core::change::Actor;
use j4n_core::crew::{self, Crew, Layer, Seat, Writer};
use j4n_core::doer::Caps;
use j4n_core::front::Cfg;
use j4n_core::front::serve::Serve;
use j4n_proto::{Intent, Up};
use serde_json::{Value, json};
use std::sync::Arc;

fn listed() -> Vec<String> {
    crew::DEFAULT_LIST.split_whitespace().map(String::from).collect()
}

fn set(l: &mut Layer, chat: &str, key: &str, v: Value, by: &Writer) -> Result<Option<(String, j4n_core::change::Change)>, String> {
    let value = crew::value_of(key, &v, 1200)?;
    l.set(chat, key, value, by, 7)
}

fn words(l: &[&str]) -> Vec<String> {
    l.iter().map(|s| s.to_string()).collect()
}

// ---- which settings, and who writes ----

#[test]
fn a_machine_wide_a_money_and_a_safety_setting_is_refused_even_on_the_list_and_so_is_one_off_the_list_or_one_a_chat_does_not_act_on() {
    let mut all = listed();
    let hard = ["accounts.default", "orchestrate.writers", "packs.full", "lanes", "chats.on", "crew.on", "plan.usd_line", "orchestrate.chats_usd", "limits.cap", "gate.rows", "guard.mode", "web.local", "grants.all"];
    all.extend(hard.iter().map(|k| k.to_string()));
    for key in hard {
        let why = crew::refusal(key, &all).unwrap_or_else(|| panic!("{key} was taken"));
        assert!(why.contains("machine-wide") || why.contains("money") || why.contains("safety"), "{key}: {why}");
    }
    assert!(crew::refusal("brief.plain_answer", &listed()).unwrap().contains("not on the list"));
    // `profile` is on the list of an operator who added it, and nothing acts on it
    let mut with_profile = listed();
    with_profile.push("profile".into());
    assert!(crew::refusal("profile", &with_profile).unwrap().contains("does not act on it yet"));
    for key in crew::ACTING {
        assert_eq!(crew::refusal(key, &listed()), None, "{key}");
    }
}

#[test]
fn a_value_is_checked_for_its_setting_and_a_null_or_an_empty_text_leaves_it_out() {
    let ok = |k: &str, v: Value| crew::value_of(k, &v, 20);
    assert_eq!(ok("effort", json!("high")).unwrap(), Some(toml::Value::String("high".into())));
    assert!(ok("effort", json!("turbo")).unwrap_err().contains("one of"));
    assert!(ok("model", json!("--yolo")).is_err() && ok("model", json!(3)).is_err());
    assert!(ok("personality", json!("x".repeat(21))).unwrap_err().contains("crew.voice_max"));
    assert_eq!(ok("personality", json!("  ")).unwrap(), None);
    assert_eq!(ok("model", Value::Null).unwrap(), None);
    // a tool list is words, as a text or a list, sorted and without repeats; a name that reads as a flag or holds a separator is refused
    let tools = |v: Value| ok("tools.deny", v).map(|t| t.map(|t| t.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect::<Vec<_>>()));
    assert_eq!(tools(json!("Write, Edit  Edit")).unwrap(), Some(words(&["Edit", "Write"])));
    assert_eq!(tools(json!(["Bash"])).unwrap(), Some(words(&["Bash"])));
    assert_eq!(tools(json!([])).unwrap(), None);
    assert!(tools(json!("--tools")).is_err() && tools(json!(["a b"])).is_err() && tools(json!(7)).is_err());
}

#[test]
fn another_writer_can_only_narrow_a_tool_list_and_only_the_operator_widens_or_leaves_one_out() {
    let mut l = Layer::default();
    let (seat, main, op) = (Writer::Seat, Writer::Main, Writer::Operator);
    set(&mut l, "c1", "tools.deny", json!("Bash"), &seat).unwrap();
    // Main denies more: the two are added, the seat's is not lost
    set(&mut l, "c1", "tools.deny", json!("Edit"), &main).unwrap();
    assert_eq!(l.list("c1", "tools.deny"), words(&["Bash", "Edit"]));
    // Main cannot leave a deny out, nor write a shorter one over it
    let none = l.set("c1", "tools.deny", None, &main, 8);
    assert!(none.unwrap_err().contains("only the operator"));
    set(&mut l, "c1", "tools.deny", json!("Edit"), &main).unwrap();
    assert_eq!(l.list("c1", "tools.deny"), words(&["Bash", "Edit"]), "the deny the seat made is still there");
    // an allow list is cut to what both name, and one that shares nothing is refused (an empty list would allow every tool)
    set(&mut l, "c1", "tools.allow", json!("Read Grep"), &seat).unwrap();
    set(&mut l, "c1", "tools.allow", json!("Read Edit"), &main).unwrap();
    assert_eq!(l.list("c1", "tools.allow"), words(&["Read"]));
    assert!(set(&mut l, "c1", "tools.allow", json!("Write"), &main).unwrap_err().contains("share none"));
    // the operator widens: it replaces, and leaves out
    set(&mut l, "c1", "tools.deny", json!("Bash"), &op).unwrap();
    assert_eq!(l.list("c1", "tools.deny"), words(&["Bash"]));
    l.set("c1", "tools.allow", None, &op, 9).unwrap();
    assert!(l.get("c1", "tools.allow").is_none());
    // the writers are named: what writes is a code path's, and the Change says it
    let (_, c) = set(&mut l, "c1", "effort", json!("low"), &main).unwrap().unwrap();
    assert_eq!(c.by, Actor::Model("main".into()));
    assert_eq!((Writer::Envoy("e1".into()).actor(), Writer::Seat.actor(), Writer::Operator.actor()), (Actor::Model("envoy:e1".into()), Actor::Core, Actor::Operator));
}

#[test]
fn a_write_is_a_change_an_undo_takes_it_back_once_and_the_layer_is_what_the_events_replay() {
    let mut l = Layer::default();
    let mut rows: Vec<(String, Value)> = Vec::new();
    let mut put = |r: Result<Option<(String, j4n_core::change::Change)>, String>| {
        let (id, change) = r.unwrap().unwrap();
        rows.push(("chat.set".into(), json!({ "id": id, "change": change })));
        id
    };
    let first = { let r = set(&mut l, "c1", "effort", json!("low"), &Writer::Main); put(r) };
    let second = { let r = set(&mut l, "c1", "effort", json!("high"), &Writer::Operator); put(r) };
    // the first cannot be taken back while the second stands on it; the second can, and then the first
    assert!(l.undo(&first, &Writer::Operator, 9).unwrap_err().contains("changed after"));
    let r = l.undo(&second, &Writer::Operator, 10).map(Some);
    put(r);
    assert_eq!(l.text("c1", "effort"), Some("low"));
    assert!(l.undo(&second, &Writer::Operator, 11).unwrap_err().contains("already"));
    let r = l.undo(&first, &Writer::Main, 12).map(Some);
    put(r);
    assert!(l.get("c1", "effort").is_none());
    assert!(l.undo("cs99", &Writer::Operator, 13).unwrap_err().contains("no change"));
    // the events say the same layer, with the same numbering, and an archive ends it
    let replay = |extra: Option<(&str, &str)>| {
        let mut ev: Vec<(&str, &str, &Value)> = rows.iter().map(|(k, d)| (k.as_str(), "c1", d)).collect();
        let none = json!({});
        if let Some((k, t)) = extra {
            ev.push((k, t, &none));
        }
        Layer::replay(ev)
    };
    assert!(replay(None).of("c1").is_empty());
    let (mut live, mut r) = (Layer::default(), Vec::new());
    for (v, by) in [("low", Writer::Main), ("max", Writer::Operator)] {
        let (id, c) = set(&mut live, "c2", "effort", json!(v), &by).unwrap().unwrap();
        r.push(json!({ "id": id, "change": c }));
    }
    let again = Layer::replay(r.iter().map(|d| ("chat.set", "c2", d)));
    assert_eq!(again.of("c2"), live.of("c2"));
    assert!(Layer::replay(r.iter().map(|d| ("chat.set", "c2", d)).chain([("chat.archived", "c2", &json!({}))])).of("c2").is_empty());
    assert!(again.of("c1").is_empty() && live.of("c2").len() == 1);
}

// ---- through the front ----

fn tooled(id: &str, answers: &[&str]) -> Arc<Scripted> {
    Scripted::with_caps(id, Caps { tools: true, ..caps() }, answers.iter().map(|a| vec![Beat::Session(format!("{id}-s")), Beat::Say(a.to_string()), end(a)]).collect())
}

fn seat_with_tools(deny: &[&str]) -> Seat {
    let mut s = crate::seat_cases::seat("guarded", None, &[]);
    s.tools_deny = words(deny);
    s.tools_allow = words(&["Read", "Grep", "Bash"]);
    s
}

/// A chat `c1` that reads, with the seat `guarded` (its tools are listed) on a doer that does or does not take a tool list.
fn rig(crew: bool, tools: bool, settings_of_main: Value) -> (Fx, Arc<Held>, String) {
    let held = Held::new();
    let mut b = brief("one", false, &A, &[]);
    (b["doer"], b["seat"], b["settings"]) = (json!("pi"), json!("guarded"), settings_of_main);
    let plan = answer(vec![b]);
    let pi = if tools { tooled("pi", &["chat one", "chat two", "chat three"]) } else { crate::branch_validate::doer("pi", &["chat one", "chat two", "chat three"]) };
    let f = fx_placed(vec![chatty("claude", &[&plan, "main worked"]), pi, chatty("cheap", &[LOOK])], None, vec![held.place()], |c: &mut Cfg| {
        settings(c);
        if crew {
            c.crew = Crew { seats: Arc::new(vec![seat_with_tools(&["Bash", "WebFetch"])]), ..Crew::shipped() };
        }
    });
    say(&f);
    settle(&f);
    let chat = f.front.chats().into_iter().find(|c| c.parent.is_none() && c.id != "main").map(|c| c.id).unwrap_or_default();
    (f, held, chat)
}

fn the_chat_turns(f: &Fx) -> Vec<j4n_core::task::Row> {
    let mut t = chat_tasks(f);
    t.sort_by_key(|r| r.id.clone());
    t
}

fn given(r: &j4n_core::task::Row) -> String {
    format!("{}{}{}", r.spec.input["brief"].as_str().unwrap_or_default(), r.spec.input["reports"].as_str().unwrap_or_default(), r.spec.input["cli_text"].as_str().unwrap_or_default())
}

#[test]
fn the_seat_is_the_first_writer_of_a_chat_and_its_words_and_tool_list_reach_every_turn_not_only_the_first() {
    let (f, _h, chat) = rig(true, true, json!({ "effort": "high" }));
    assert!(!chat.is_empty(), "{}", f.rig.dump());
    let rows = events(&f, "chat.set");
    let by = |key: &str| rows.iter().find(|r| r["key"] == key).map(|r| r["change"]["by"].clone());
    assert_eq!((by("personality"), by("tools.deny"), by("effort")), (Some(json!("core")), Some(json!("core")), Some(json!({ "model": "main" }))), "{rows:?}");
    assert!(rows.iter().all(|r| r["applied"] == true), "{rows:?}");
    // the first turn: the words in its Brief, the list in its input, the effort on its tune
    let first = &the_chat_turns(&f)[0];
    assert!(given(first).contains("words of guarded"), "{}", given(first));
    assert_eq!(first.spec.input["tools"], json!({ "allow": ["Bash", "Grep", "Read"], "deny": ["Bash", "WebFetch"] }));
    assert_eq!(first.spec.tune.effort.as_deref(), Some("high"));
    // a line typed into the chat later is the chat's turn too, and carries all three again
    f.front.say(&chat, "go on with the second part");
    settle(&f);
    let turns = the_chat_turns(&f);
    assert_eq!(turns.len(), 2, "{}", f.rig.dump());
    assert!(given(&turns[1]).contains("words of guarded") && turns[1].spec.input["tools"] == first.spec.input["tools"] && turns[1].spec.tune.effort.as_deref() == Some("high"), "{:?}", turns[1].spec);
}

#[test]
fn a_doer_that_takes_no_tool_list_is_told_so_in_the_row_and_in_the_chat_and_the_lists_are_still_written() {
    let (f, _h, chat) = rig(true, false, json!({}));
    let rows = events(&f, "chat.set");
    let tools: Vec<&Value> = rows.iter().filter(|r| r["key"].as_str().is_some_and(|k| k.starts_with("tools."))).collect();
    assert_eq!(tools.len(), 2);
    assert!(tools.iter().all(|r| r["applied"] == false && r["why"].as_str().unwrap().contains("pi takes no tool list")), "{tools:?}");
    assert!(f.notes().iter().any(|n| n.contains("not applied") && n.contains("pi takes no tool list")), "{:?}", f.notes());
    assert!(rows.iter().filter(|r| r["key"] == "personality").all(|r| r["applied"] == true));
    assert_eq!(f.front.chat_layer(&chat).len(), 3);
}

#[test]
fn main_asks_for_a_setting_that_is_refused_and_the_chat_starts_without_it_with_the_reason_in_the_chat() {
    let (f, _h, chat) = rig(true, true, json!({ "orchestrate.writers": 4, "effort": "turbo", "model": "haiku" }));
    assert_eq!(chat_tasks(&f).len(), 1, "{}", f.rig.dump());
    let notes = f.notes();
    assert!(notes.iter().any(|n| n.contains("`orchestrate.writers` was not set") && n.contains("machine-wide")), "{notes:?}");
    assert!(notes.iter().any(|n| n.contains("`effort` was not set") && n.contains("one of")), "{notes:?}");
    // the one that fits was written as Main's, and the turn takes it
    assert_eq!(f.front.chat_layer(&chat).iter().find(|(k, _)| k == "model").map(|(_, v)| v.as_str().unwrap().to_string()), Some("haiku".into()));
    assert_eq!(the_chat_turns(&f)[0].spec.tune.model.as_deref(), Some("haiku"));
}

#[test]
fn the_operators_set_and_undo_go_through_the_screens_intent_and_the_typed_undo_and_a_bare_undo_leaves_them_alone() {
    let (f, _h, chat) = rig(true, true, json!({}));
    let mut s = Serve::new(f.front.clone());
    let (_, quit) = s.up(Up::Intent { intent: Intent::Set { kind: "chat".into(), id: chat.clone(), field: "effort".into(), value: json!("low") } });
    assert!(!quit);
    let row = events(&f, "chat.set").into_iter().find(|r| r["key"] == "effort").expect("the write is in the Log");
    assert_eq!((row["change"]["by"].clone(), row["id"].as_str()), (json!("operator"), Some("cs4")));
    assert!(f.front.chat_layer(&chat).iter().any(|(k, v)| k == "effort" && v.as_str() == Some("low")));
    // a line that undoes something else (nothing was edited here) does not touch the chat's settings
    f.front.say("main", ":undo");
    assert!(f.front.chat_layer(&chat).iter().any(|(k, _)| k == "effort"));
    f.front.say("main", ":undo cs4");
    assert!(!f.front.chat_layer(&chat).iter().any(|(k, _)| k == "effort"));
    let back = events(&f, "chat.set").into_iter().filter(|r| r["undoes"] == "cs4").count();
    assert_eq!(back, 1);
    // a setting of a chat that is not there, and one that is not a chat's, are said and nothing is written
    for (id, field) in [("c99", "effort"), (chat.as_str(), "lanes")] {
        let before = events(&f, "chat.set").len();
        s.up(Up::Intent { intent: Intent::Set { kind: "chat".into(), id: id.into(), field: field.into(), value: json!("low") } });
        assert_eq!(events(&f, "chat.set").len(), before, "{id} {field}");
    }
}

#[test]
fn the_layer_is_gone_when_the_chat_is_archived_and_a_chore_or_a_check_on_the_thread_is_not_given_it() {
    let (f, _h, chat) = rig(true, true, json!({ "effort": "high" }));
    assert_eq!(f.front.chat_layer(&chat).len(), 4);
    // a task of the chat's thread that is not its work (a branch's) takes none of it
    let mut spec = j4n_core::task::TaskSpec::new("check", "pi");
    spec.role = Some("work".into());
    spec.input = json!({ "branch": { "class": "check" } });
    let mut other = spec.clone();
    other.input = json!({});
    for (s, took) in [(spec, false), (other, true)] {
        let mut s = s;
        let words = f.front.chat_apply(&chat, &mut s);
        assert_eq!((words.is_empty(), s.tune.effort.is_some(), s.input.get("tools").is_some()), (!took, took, took), "{s:?}");
    }
    f.front.say("main", &format!(":archive {chat}"));
    assert!(f.front.chat(&chat).is_some_and(|c| c.archived_ms.is_some()), "{}", f.rig.dump());
    assert!(f.front.chat_layer(&chat).is_empty());
}

#[test]
fn with_the_crew_off_a_chat_has_no_settings_no_row_is_written_and_a_turn_is_given_nothing_of_it() {
    let (f, _h, chat) = rig(false, true, json!({ "effort": "high" }));
    let why = f.front.chat_set(&chat, "effort", &json!("low"), &Writer::Operator).unwrap_err();
    assert!(why.contains("crew is off"), "{why}");
    assert!(f.front.chat_undo("cs1", &Writer::Operator).is_err());
    assert!(events(&f, "chat.set").is_empty() && f.front.chat_layer(&chat).is_empty());
    let t = &the_chat_turns(&f)[0];
    assert!(t.spec.input.get("tools").is_none() && t.spec.tune.effort.is_none() && !given(t).contains("words of"), "{:?}", t.spec);
}

#[test]
fn a_doer_with_no_brief_is_given_the_seats_words_once_a_session_and_not_again_on_the_next_line() {
    let mut b = brief("one", false, &A, &[]);
    (b["doer"], b["seat"]) = (json!("plain"), json!("guarded"));
    let plan = answer(vec![b]);
    let held = Held::new();
    let f = fx_placed(vec![chatty("claude", &[&plan, "main worked"]), chatty("plain", &["one", "two"]), chatty("cheap", &[LOOK])], None, vec![held.place()], |c: &mut Cfg| {
        settings(c);
        c.crew = Crew { seats: Arc::new(vec![seat_with_tools(&[])]), ..Crew::shipped() };
    });
    say(&f);
    settle(&f);
    let chat = f.front.chats().into_iter().find(|c| c.id != "main").map(|c| c.id).unwrap();
    f.front.say(&chat, "and the next part");
    settle(&f);
    let turns = the_chat_turns(&f);
    assert_eq!(turns.len(), 2, "{}", f.rig.dump());
    assert!(given(&turns[0]).contains("words of guarded"), "{}", given(&turns[0]));
    assert!(!given(&turns[1]).contains("words of guarded"), "the session has them: {}", given(&turns[1]));
}
