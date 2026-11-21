//! A row of Settings carries its editor (`data.edit`): `⏎` on a switch flips it, `space` sends the next
//! word, `⏎` on anything else opens the editor of its kind in the row's place; a change that acts from the next start brings up the
//! restart card; `/` finds in a list.

use crate::world::{hello, rows, screen};
use j4n_proto::{Act, Cell, Down, Field, FieldKind, Intent, Json, Node, Present, Row, Tone, Up, json};
use j4n_tui::App;
use j4n_tui::headless::{KeyCode, press, typed};

const BIND: &str = "view:settings-features";

fn app() -> App {
    let page = Node { id: "features".into(), title: "Features".into(), field: Some(Field { kind: FieldKind::List, bind: BIND.into(), props: Json::Null }), ..Node::default() };
    let pages = Node { id: "pages".into(), present: Present::Tabs, children: vec![page], ..Node::default() };
    let mut a = App::new();
    a.start();
    a.down(hello());
    a.down(Down::Tree { node: Node { id: "root".into(), children: vec![pages], ..Node::default() } });
    a
}

fn set(id: &str, value: Json) -> Intent {
    Intent::Set { kind: "setting".into(), id: id.into(), field: "value".into(), value }
}

/// A setting's row as the view sends it: ⏎ (no value) and, for a switch of words, `space` with the next one.
fn setting(id: &str, now: &str, edit: Json, flip: Option<&str>) -> Row {
    let mut actions = vec![Act { key: "enter".into(), label: "change".into(), intent: set(id, Json::Null), risk: 0 }];
    if let Some(f) = flip {
        actions.push(Act { key: " ".into(), label: "flip".into(), intent: set(id, json!(f)), risk: 0 });
    }
    let mut edit = edit;
    edit["bind"] = json!(format!("setting:{id}.value"));
    Row { key: id.into(), cells: vec![Cell::new(id, Tone::Plain), Cell::new(now, Tone::Plain), Cell::new("what it does", Tone::Plain)], actions, data: Some(json!({ "edit": edit })), ..Row::default() }
}

fn put(a: &mut App) {
    let all = vec![
        setting("chats.on", "off", json!({ "kind": "bool", "value": false, "start": false, "next_start": true, "waits": false }), Some("true")),
        setting("look.keys_row", "on", json!({ "kind": "bool", "value": true, "start": true, "next_start": false, "waits": false }), Some("false")),
        setting("session.mode", "auto", json!({ "kind": "enum", "value": "auto", "options": ["resume", "auto", "each"], "start": "auto" }), Some("each")),
        setting("loop.followups", "3", json!({ "kind": "int", "value": 3, "min": 0, "max": 9, "start": 3 }), None),
    ];
    rows(a, BIND, all);
}

fn sent_set(ups: &[Up]) -> Option<Intent> {
    ups.iter().find_map(|u| if let Up::Intent { intent: i @ Intent::Set { .. } } = u { Some(i.clone()) } else { None })
}

#[test]
fn enter_flips_a_switch_and_one_of_the_next_start_brings_up_the_restart_card() {
    let mut a = app();
    put(&mut a);
    let ups = a.key(press(KeyCode::Enter));
    assert_eq!(sent_set(&ups), Some(set("chats.on", json!(true))), "⏎ on a switch flips it at once");
    assert_eq!(a.restart.as_deref(), Some("chats.on"), "it acts from the next start: the card asks");
    let shown = screen(&mut a, 80, 20).join("\n");
    assert!(shown.contains("restart now? ⏎ / later esc") && shown.contains("chats.on"), "{shown}");
    let ups = a.key(press(KeyCode::Enter));
    assert_eq!(ups, vec![Up::Intent { intent: Intent::Run { command: "restart".into(), args: Vec::new() } }]);
    assert!(a.restart.is_none());
    // a live one brings up no card; `esc` puts a card off
    a.key(press(KeyCode::Down));
    let ups = a.key(press(KeyCode::Char(' ')));
    assert_eq!(sent_set(&ups), Some(set("look.keys_row", json!("false"))), "space sends the row's own value");
    assert!(a.restart.is_none(), "a live setting needs no restart");
    a.restart = Some("chats.on".into());
    assert!(a.key(press(KeyCode::Esc)).is_empty() && a.restart.is_none(), "esc: later");
}

#[test]
fn enter_opens_the_editor_of_the_rows_kind_in_its_place_and_keeps_what_is_typed() {
    let mut a = app();
    put(&mut a);
    for _ in 0..3 {
        a.key(press(KeyCode::Down));
    }
    assert!(a.key(press(KeyCode::Enter)).is_empty(), "a number opens its editor, nothing is sent yet");
    assert!(a.row_ed.is_some() && a.val.is_some());
    for k in [press(KeyCode::Backspace)].into_iter().chain(typed("7", false)) {
        assert!(a.key(k).is_empty());
    }
    let shown = screen(&mut a, 80, 20).join("\n");
    assert!(shown.contains("loop.followups") && shown.contains('7'), "the editor stands in the row's place:\n{shown}");
    let ups = a.key(press(KeyCode::Enter));
    assert_eq!(sent_set(&ups), Some(set("loop.followups", json!(7))));
    assert!(a.row_ed.is_none() && a.val.is_none(), "kept: the row is a row again");
    // a number past its bounds stays open, and esc gives it up
    a.key(press(KeyCode::Enter));
    for k in [press(KeyCode::Backspace)].into_iter().chain(typed("12", false)) {
        a.key(k);
    }
    assert!(sent_set(&a.key(press(KeyCode::Enter))).is_none() && a.row_ed.is_some(), "12 is past 9: it stays open");
    assert!(a.key(press(KeyCode::Esc)).is_empty() && a.row_ed.is_none());
    // words: ⏎ opens the picker, → and ⏎ take the next one
    a.key(press(KeyCode::Up));
    a.key(press(KeyCode::Enter));
    a.key(press(KeyCode::Right));
    assert_eq!(sent_set(&a.key(press(KeyCode::Enter))), Some(set("session.mode", json!("each"))));
}

#[test]
fn a_switch_has_its_mark_a_waiting_one_says_so_and_slash_finds_in_the_list() {
    let mut a = app();
    let mut all = vec![setting("chats.on", "on", json!({ "kind": "bool", "value": true, "start": false, "next_start": true, "waits": true }), Some("false"))];
    all.push(setting("look.keys_row", "on", json!({ "kind": "bool", "value": true, "start": true }), Some("false")));
    rows(&mut a, BIND, all);
    let shown = screen(&mut a, 80, 12).join("\n");
    assert!(shown.lines().any(|l| l.contains("chats.on") && l.contains("next start")), "{shown}");
    assert!(!shown.lines().any(|l| l.contains("look.keys_row") && l.contains("next start")), "{shown}");
    for k in [press(KeyCode::Char('/'))].into_iter().chain(typed("keys", true)) {
        a.key(k);
    }
    let shown = screen(&mut a, 80, 12).join("\n");
    assert!(!shown.contains("chats.on") && shown.contains("look.keys_row") && shown.contains("1 found"), "{shown}");
    let ups = a.key(press(KeyCode::Enter));
    assert_eq!(sent_set(&ups), Some(set("look.keys_row", json!(false))), "the keys are on the row found");
    a.key(press(KeyCode::Esc));
    assert!(screen(&mut a, 80, 12).join("\n").contains("chats.on"), "esc clears the find");
}

/// The keys row shows a row's key only on a row that has it: `u` undo on a row with an edit to take back, none on one without.
#[test]
fn the_keys_row_shows_undo_only_on_a_row_with_an_edit_to_take_back() {
    let mut a = app();
    let mut all = vec![
        setting("chats.on", "off", json!({ "kind": "bool", "value": false, "start": false }), Some("true")),
        setting("look.keys_row", "off", json!({ "kind": "bool", "value": false, "start": true }), Some("true")),
    ];
    all[1].actions.push(Act { key: "u".into(), label: "undo".into(), intent: Intent::Say { text: ":undo e1".into(), with: Vec::new() }, risk: 0 });
    rows(&mut a, BIND, all);
    let hint = |key: &str, keys: &str, word: &str| Row { key: key.into(), cells: vec![Cell::new(keys, Tone::Plain), Cell::new(word, Tone::Plain)], ..Row::default() };
    rows(&mut a, j4n_proto::KEYS_ROW, vec![hint("page:features/undo", "u", "undo"), hint("page:features/change", "⏎", "change"), hint("global/quit", "⌃c", "quit")]);
    let words = |a: &App| a.hints().into_iter().map(|h| h.word).collect::<Vec<_>>();
    // the row's own keys stand first, marked, and a page hint of the same keys gives way to them: `⏎ change` is the row's now
    assert_eq!(words(&a), ["change", "flip", "quit"], "the first row has nothing to take back");
    a.key(press(KeyCode::Down));
    assert_eq!(words(&a), ["change", "flip", "undo", "quit"], "the second has");
}

/// A key right after `/ text ⏎`, before the list is drawn again or the core's found rows come, and with all the rows sent again, acts on
/// the row the find left, not on the first of all the rows the screen hides.
#[test]
fn a_key_right_after_a_find_acts_on_the_row_found_not_a_hidden_one() {
    let mut a = app();
    let all = vec![
        setting("chats.on", "on", json!({ "kind": "bool", "value": true, "start": true }), Some("false")),
        setting("look.keys_row", "on", json!({ "kind": "bool", "value": true, "start": true }), Some("false")),
    ];
    rows(&mut a, BIND, all.clone());
    screen(&mut a, 80, 12);
    for k in [press(KeyCode::Char('/'))].into_iter().chain(typed("keys", true)) {
        a.key(k);
    }
    assert_eq!(sent_set(&a.key(press(KeyCode::Enter))), Some(set("look.keys_row", json!(false))), "not drawn yet: the keys are on the row found");
    screen(&mut a, 80, 12);
    rows(&mut a, BIND, all);
    assert_eq!(sent_set(&a.key(press(KeyCode::Enter))), Some(set("look.keys_row", json!(false))), "all the rows again: still the row found");
}
