//! The keymap. The core says what each key does where; the screen reads a pressed key as the key its own code knows, or as
//! nothing, or sends the intent of a page's key. Typing into a line or a value is never read through it.

use crate::world::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use j4n_proto::keys::Key;
use j4n_proto::{Act, AskView, Bind, Cell, Choice, Down, Field, FieldKind, Intent, Json, Node, Present, Reply, Row, Shape, Tone, Up, json};
use j4n_tui::headless::{alt, ctrl, press as key};
use j4n_tui::keymap::key_of;
use j4n_tui::App;

fn bind(context: &str, key: &str, to: &str) -> Bind {
    Bind { context: context.into(), key: key.into(), to: to.into(), run: None }
}

fn run(context: &str, key: &str, command: &str) -> Bind {
    Bind { run: Some(Intent::Run { command: command.into(), args: Vec::new() }), ..bind(context, key, "") }
}

fn mapped(binds: Vec<Bind>) -> App {
    let mut a = app();
    a.down(Down::Keys { binds });
    a
}

fn rows(a: &mut App, bind: &str, n: usize) {
    let rows: Vec<Row> = (0..n).map(|i| Row { key: format!("r{i}"), ..Row::default() }).collect();
    a.down(Down::Rows { bind: bind.into(), rows, total: n as u64, from: 0, rev: 1 });
}

fn probe() -> Up {
    Up::Intent { intent: Intent::Run { command: "probe".into(), args: Vec::new() } }
}

#[test]
fn a_key_that_is_bound_is_read_as_the_key_the_screen_knows_and_the_one_it_had_still_is_when_it_is_bound_too() {
    let mut a = mapped(vec![bind("global", "C-p", "C-k"), bind("global", "C-k", "C-k")]);
    a.key(ctrl('p'));
    assert!(a.jump.is_some(), "⌃p opens the jump");
    a.key(key(KeyCode::Esc));
    assert!(a.jump.is_none());
    a.key(ctrl('k'));
    assert!(a.jump.is_some(), "⌃k still does");
}

#[test]
fn a_key_that_is_freed_does_nothing_and_the_key_that_is_bound_in_its_place_does_it() {
    let mut a = mapped(vec![bind("composer", "C-u", ""), bind("composer", "C-g", "C-u")]);
    type_in(&mut a, "abc");
    assert!(a.key(ctrl('u')).is_empty());
    assert_eq!(a.line.buf, "abc", "⌃u is freed");
    a.key(ctrl('g'));
    assert_eq!(a.line.buf, "", "⌃g clears the line");
}

#[test]
fn a_key_of_a_context_is_read_there_and_only_there() {
    let mut a = mapped(vec![bind("list", "j", "Down")]);
    a.key(key(KeyCode::F(2)));
    rows(&mut a, "tasks:all", 4);
    a.key(key(KeyCode::Char('j')));
    assert_eq!(a.place("tasks:all"), 1, "on a list `j` is ↓");
    a.key(key(KeyCode::F(1)));
    assert_eq!(a.key(key(KeyCode::Char('j'))), [], "the composer types it");
    assert_eq!(a.line.buf, "j");
}

#[test]
fn the_key_of_a_page_sends_its_intent_but_a_character_is_not_taken_from_a_line_being_typed() {
    let mut a = mapped(vec![run("page:chat", "r", "probe"), run("page:chat", "C-r", "probe"), run("page:tasks", "r", "probe")]);
    assert!(a.key(key(KeyCode::Char('r'))).is_empty());
    assert_eq!(a.line.buf, "r", "the composer has the letter");
    assert_eq!(a.key(ctrl('r')), [probe()], "a chord is the page's");
    a.key(key(KeyCode::F(2)));
    assert_eq!(a.key(key(KeyCode::Char('r'))), [probe()], "on a list the letter is the page's");
}

#[test]
fn a_page_key_is_read_before_the_screens_and_the_screens_before_a_freed_one() {
    let mut a = mapped(vec![run("page:chat", "C-p", "probe"), bind("global", "C-p", "C-k"), bind("composer", "C-p", "")]);
    assert_eq!(a.key(ctrl('p')), [probe()]);
    assert!(a.jump.is_none());
    let mut b = mapped(vec![bind("global", "C-p", "C-k"), bind("composer", "C-p", "")]);
    b.key(ctrl('p'));
    assert!(b.jump.is_some(), "a context that frees a key does not hold it against one that binds it");
}

#[test]
fn nothing_is_read_through_the_map_while_a_line_or_a_value_is_being_typed() {
    let mut a = mapped(vec![bind("global", "q", "Esc"), bind("list", "q", "Esc")]);
    a.key(ctrl('k'));
    a.key(key(KeyCode::Char('q')));
    assert_eq!(a.jump.as_ref().map(|j| j.line.buf.as_str()), Some("q"), "the jump is text");
}

#[test]
fn a_key_is_read_as_a_key_with_its_modifiers_and_back() {
    let name = |k: KeyEvent| key_of(&k).map(|k| k.to_string());
    assert_eq!(name(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT)).as_deref(), Some("G"));
    assert_eq!(name(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)).as_deref(), Some("S-Tab"));
    assert_eq!(name(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)).as_deref(), Some("S-Enter"));
    assert_eq!(name(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT)).as_deref(), Some("M-Left"));
    assert_eq!(name(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL)).as_deref(), Some("C-j"));
    assert_eq!(name(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE)).as_deref(), Some("PgDn"));
    assert_eq!(name(KeyEvent::new(KeyCode::F(3), KeyModifiers::NONE)).as_deref(), Some("F3"));
    assert_eq!(name(KeyEvent::new(KeyCode::Null, KeyModifiers::NONE)), None);
    assert_eq!("S-Tab".parse::<Key>().map(|k| k.shift), Ok(true));
}

#[test]
fn shift_enter_where_a_terminal_tells_it_apart_is_a_new_line_and_a_key_can_stand_for_it() {
    let mut a = app();
    type_in(&mut a, "one");
    a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
    type_in(&mut a, "two");
    assert_eq!(a.line.buf, "one\ntwo", "⇧⏎ is a line break, not a send");
    let mut b = mapped(vec![bind("composer", "M-n", "S-Enter")]);
    type_in(&mut b, "x");
    b.key(alt('n'));
    type_in(&mut b, "y");
    assert_eq!(b.line.buf, "x\ny");
    assert_eq!(b.key(key(KeyCode::Enter)).len(), 1, "and Enter still sends");
}

#[test]
fn a_key_the_map_does_not_name_is_read_as_it_is() {
    let mut a = mapped(vec![bind("global", "C-p", "C-k")]);
    type_in(&mut a, "hello");
    assert_eq!(a.line.buf, "hello");
    assert_eq!(a.key(key(KeyCode::F(2))).len(), 1, "F2 still opens the second tab");
}

#[test]
fn the_keys_on_the_transcript_are_a_context_of_their_own() {
    let mut a = mapped(vec![bind("transcript", "k", "C-Up")]);
    let turn = |k: &str| Row { key: k.into(), cells: vec![Cell::new(k, Tone::Plain)], ..Row::default() };
    a.down(Down::Rows { bind: "chat:main".into(), rows: vec![turn("t1"), turn("t2")], total: 2, from: 0, rev: 1 });
    a.key(key(KeyCode::Char('k')));
    assert_eq!(a.line.buf, "k", "until the keys are on the transcript `k` is a letter");
    a.key(ctrl('u'));
    a.key(KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL));
    let at = a.wins.get("chat:main").and_then(|w| w.at.clone());
    assert!(at.is_some(), "⌃↑ puts the keys on the newest turn");
    a.key(key(KeyCode::Char('k')));
    let before = a.wins.get("chat:main").and_then(|w| w.at.clone());
    assert!(before.is_some() && before != at, "`k` is the turn before: {at:?} then {before:?}");
}

#[test]
fn a_question_that_is_open_has_keys_of_its_own_and_one_that_is_not_has_none() {
    let ask = AskView {
        ask: "q1".into(),
        asker: "b".into(),
        question: "which colour?".into(),
        shape: Shape::Choice,
        options: ["red", "green"].iter().map(|l| Choice { id: (*l).into(), label: (*l).into(), ..Choice::default() }).collect(),
        ..AskView::default()
    };
    let mut a = mapped(vec![bind("ask", "o", "1")]);
    a.key(key(KeyCode::Char('o')));
    assert_eq!(a.line.buf, "o", "no question: a letter");
    a.key(ctrl('u'));
    a.down(Down::Asks { open: vec![ask] });
    let sent = a.key(key(KeyCode::Char('o')));
    assert_eq!(sent, [Up::Intent { intent: Intent::Answer { ask: "q1".into(), reply: Reply::Pick { id: "red".into() } } }], "`o` is the first answer");
}

#[test]
fn a_find_being_typed_and_a_value_being_edited_are_text_for_the_map_too() {
    let mut a = mapped(vec![bind("list", "q", "Esc")]);
    a.key(key(KeyCode::F(3)));
    rows(&mut a, "log:all", 3);
    a.key(key(KeyCode::Char('/')));
    a.key(key(KeyCode::Char('q')));
    assert_eq!(a.wins.get("log:all").and_then(|w| w.typing.as_ref().map(|t| t.buf.clone())), Some("q".into()), "a letter of the find, not esc");
    let field = Node { id: "f".into(), title: "f".into(), field: Some(Field { kind: FieldKind::Str, bind: "cfg:x.f".into(), props: Json::Null }), ..Node::default() };
    let mut b = App::new();
    b.start();
    b.down(hello());
    b.down(Down::Tree { node: Node { id: "root".into(), present: Present::Stack, children: vec![field], ..Node::default() } });
    b.down(Down::Keys { binds: vec![bind("list", "q", "Esc")] });
    b.down(Down::Value { bind: "cfg:x.f".into(), value: Json::String("a".into()), rev: 1 });
    b.key(key(KeyCode::Enter));
    assert!(b.val.is_some(), "the value is being edited");
    b.key(key(KeyCode::Char('q')));
    assert!(b.val.is_some(), "and `q` was a letter of it, not esc");
}

#[test]
fn a_key_can_stand_for_a_key_of_the_tabs() {
    let mut a = mapped(vec![bind("global", "C-n", "M-2")]);
    a.key(ctrl('n'));
    assert_eq!(a.tab, 1, "⌃n is ⌥2, the second tab");
    assert_eq!(key_of(&KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE)).map(|k| k.to_string()).as_deref(), Some("S-Tab"), "a BackTab that carries no shift is still one");
}

#[test]
fn the_card_that_has_the_keys_and_the_value_a_row_asks_for_are_text_for_the_map_too() {
    let mut a = mapped(vec![bind("composer", "z", "Esc"), bind("list", "z", "Esc")]);
    a.down(Down::Value { bind: "shape:on".into(), value: Json::Bool(true), rev: 1 });
    type_in(&mut a, "fix the parser thing");
    let card = |key: &str, cells: &[&str]| Row { key: key.into(), cells: cells.iter().map(|c| Cell::new(*c, Tone::Plain)).collect(), ..Row::default() };
    let rows = vec![card("~draft", &["fix the parser thing", "ready", ""]), card("file:1", &["file", "a.rs", "exist"])];
    a.down(Down::Rows { bind: "shape:card".into(), total: 2, rows, from: 0, rev: 1 });
    a.key(ctrl('o'));
    assert!(a.composer.focus, "the card has the keys");
    a.key(key(KeyCode::Char('z')));
    assert!(a.composer.focus, "and `z` is not esc to it");
    // a row that asks for a word: the letters are the word's
    let set = Intent::Set { kind: "thread".into(), id: "main".into(), field: "name".into(), value: Json::Null };
    let row = Row { key: "r0".into(), actions: vec![Act { key: "e".into(), label: "rename".into(), intent: set, risk: 0 }], ..Row::default() };
    a.down(Down::Rows { bind: "tasks:all".into(), rows: vec![row], total: 1, from: 0, rev: 1 });
    a.key(key(KeyCode::Esc));
    a.key(key(KeyCode::F(2)));
    a.key(key(KeyCode::Char('e')));
    assert!(a.edit.is_some(), "the row's key began an edit");
    a.key(key(KeyCode::Char('z')));
    assert_eq!(a.edit.as_ref().map(|e| e.line.buf.as_str()), Some("z"), "`z` is a letter of it");
}

#[test]
fn the_page_nearest_the_keys_is_read_before_the_ones_around_it() {
    let mut a = mapped(vec![run("page:chat", "C-r", "probe"), run("page:root", "C-r", "other")]);
    assert_eq!(a.key(ctrl('r')), [probe()], "the chat's key, not the root's");
    let mut b = mapped(vec![run("page:root", "C-r", "other")]);
    assert_eq!(b.key(ctrl('r')).len(), 1, "and the root's where nothing nearer has it");
}

// ---- Tab completes: the ghost, the steps, and the keys around them ----

/// A chat whose palette has four commands: `:s` matches each at its start, so the shorter go first.
fn commands() -> App {
    let mut a = app();
    let rows: Vec<Row> = ["switch", "status", "stop", "setup"].iter().map(|w| Row { key: (*w).into(), cells: vec![Cell::new(format!(":{w}"), Tone::Accent)], ..Row::default() }).collect();
    a.down(Down::Rows { bind: j4n_proto::PALETTE.into(), total: rows.len() as u64, rows, from: 0, rev: 1 });
    a
}

fn said(ups: &[Up]) -> Vec<String> {
    ups.iter().filter_map(|u| if let Up::Intent { intent: Intent::Say { text, .. } } = u { Some(text.clone()) } else { None }).collect()
}

/// The core's answer to `find`: `~for`, then the candidates.
fn answer(a: &mut App, find: &str, slot: &str, words: &[&str]) {
    let head = Row { key: "~for".into(), cells: vec![Cell::new(find, Tone::Plain), Cell::new(slot, Tone::Dim), Cell::new("", Tone::Dim)], data: Some(json!({ "find": find, "slot": slot, "on": true })), ..Row::default() };
    let rows: Vec<Row> = std::iter::once(head).chain(words.iter().map(|w| Row { key: format!("{slot}:{w}"), cells: vec![Cell::new(*w, Tone::Plain), Cell::new("", Tone::Dim)], ..Row::default() })).collect();
    a.down(Down::Rows { bind: j4n_proto::COMPLETE.into(), total: rows.len() as u64, rows, from: 0, rev: 1 });
}

#[test]
fn tab_takes_the_ghost() {
    let mut a = commands();
    type_in(&mut a, ":sw");
    assert!(show(&mut a, 60, 12).contains(":switch"), "the rest of `:switch` stands after the cursor");
    assert!(a.key(key(KeyCode::Tab)).is_empty(), "nothing is sent");
    assert_eq!((a.line.buf.as_str(), a.line.at), (":switch", 7), "the candidate whole, the cursor after it");
}

#[test]
fn tab_again_puts_the_next_in_the_line_and_shift_tab_the_one_before() {
    let mut a = commands();
    type_in(&mut a, ":s");
    let mut seen = Vec::new();
    for _ in 0..5 {
        a.key(key(KeyCode::Tab));
        seen.push(a.line.buf.clone());
    }
    assert_eq!(seen, [":stop", ":setup", ":switch", ":status", ":stop"], "the shorter first, then as listed; round the end");
    a.key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(a.line.buf, ":status", "⇧⇥ the one before");
    let mut b = commands();
    type_in(&mut b, ":s");
    b.key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(b.line.buf, ":status", "⇧⇥ steps in from the last");
}

#[test]
fn esc_puts_back_what_was_typed() {
    let mut a = commands();
    type_in(&mut a, ":s");
    a.key(key(KeyCode::Tab));
    a.key(key(KeyCode::Tab));
    assert!(a.key(key(KeyCode::Esc)).is_empty());
    assert_eq!((a.line.buf.as_str(), a.line.at), (":s", 2), "back to what was typed before the first ⇥");
    // with a ghost and nothing stepped, `esc` hides the ghost and keeps the line
    let mut b = commands();
    type_in(&mut b, ":sw");
    b.key(key(KeyCode::Esc));
    assert_eq!(b.line.buf, ":sw", "the line stays");
    assert!(!show(&mut b, 60, 12).contains(":switch"), "the ghost is hidden for this token");
    b.key(key(KeyCode::Char('i')));
    assert!(show(&mut b, 60, 12).contains(":switch"), "and back once the token changes");
}

#[test]
fn enter_keeps_the_pick_and_sends_nothing() {
    let mut a = commands();
    type_in(&mut a, ":s");
    a.key(key(KeyCode::Tab));
    assert!(said(&a.key(key(KeyCode::Enter))).is_empty(), "⏎ keeps the one in the line");
    assert_eq!(a.line.buf, ":stop");
    assert_eq!(said(&a.key(key(KeyCode::Enter))), [":stop"], "the next ⏎ sends it");
    // a key typed while stepping keeps the line and goes on from it
    let mut b = commands();
    type_in(&mut b, ":s");
    b.key(key(KeyCode::Tab));
    type_in(&mut b, " x");
    assert_eq!(b.line.buf, ":stop x");
}

#[test]
fn right_at_the_end_takes_the_ghost_and_ctrl_right_one_word() {
    let mut a = commands();
    type_in(&mut a, ":sw");
    a.key(key(KeyCode::Right));
    assert_eq!(a.line.buf, ":switch", "→ at the end takes it whole");
    let mut b = app();
    type_in(&mut b, ":say dep");
    answer(&mut b, "line :say dep", "word:say", &["deploy the fix"]);
    b.key(ctrl_right());
    assert_eq!(b.line.buf, ":say deploy", "⌃→ takes one word of it");
    answer(&mut b, "line :say deploy", "word:say", &["deploy the fix"]);
    b.key(ctrl_right());
    assert_eq!(b.line.buf, ":say deploy the", "and the next, with the blank before it");
    answer(&mut b, "line :say deploy the", "word:say", &["the fix"]);
    b.key(key(KeyCode::Right));
    assert_eq!(b.line.buf, ":say deploy the fix", "→ the rest");
}

fn ctrl_right() -> KeyEvent {
    KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL)
}

#[test]
fn enter_with_a_ghost_sends_the_line_as_typed() {
    let mut a = commands();
    type_in(&mut a, ":sw");
    assert!(show(&mut a, 60, 12).contains(":switch"), "a ghost stands there");
    assert_eq!(said(&a.key(key(KeyCode::Enter))), [":sw"], "the ghost is never part of the line");
}

#[test]
fn the_core_is_asked_for_a_commands_words_and_an_answer_for_another_draft_is_dropped() {
    let (mut a, sent) = started();
    assert!(sent.contains(&Up::Watch { bind: j4n_proto::COMPLETE.into() }), "watched wherever a chat is");
    let ups = type_in(&mut a, ":pack o");
    let windows: Vec<&str> = ups.iter().filter_map(|u| if let Up::Window { bind, find, .. } = u { (bind == j4n_proto::COMPLETE).then_some(find.as_str()) } else { None }).collect();
    assert_eq!(windows, ["line :", "line :p", "line :pa", "line :pac", "line :pack", "line :pack ", "line :pack o"], "a window when the find changes, the command's name too");
    // the answer for `:pack ` came late: it is for another draft, and nothing is offered from it
    answer(&mut a, "line :pack ", "on|off|ready", &["on", "off", "ready"]);
    a.key(key(KeyCode::Tab));
    assert_eq!(a.line.buf, ":pack o", "an older answer is dropped");
    answer(&mut a, "line :pack o", "on|off|ready", &["on", "off"]);
    a.key(key(KeyCode::Tab));
    assert_eq!(a.line.buf, ":pack on");
    // while Tab steps the list holds still: no window is asked, and a new answer changes nothing
    assert!(a.key(key(KeyCode::Tab)).is_empty(), "no window while stepping");
    assert_eq!(a.line.buf, ":pack off");
    answer(&mut a, "line :pack o", "on|off|ready", &["only"]);
    a.key(key(KeyCode::Tab));
    assert_eq!(a.line.buf, ":pack on", "the order never changes under ⇥");
}

#[test]
fn a_command_the_palette_lacks_is_completed_from_the_cores_answer() {
    // the phrase commands are not rows of the palette: the core answers the slot of a command too, and the palette's stay at once
    let mut a = commands();
    a.down(Down::Rows { bind: j4n_proto::PALETTE.into(), total: 1, rows: vec![Row { key: "pack".into(), cells: vec![Cell::new(":pack", Tone::Accent)], ..Row::default() }], from: 0, rev: 1 });
    let ups = type_in(&mut a, ":sw");
    let windows: Vec<&str> = ups.iter().filter_map(|u| if let Up::Window { bind, find, .. } = u { (bind == j4n_proto::COMPLETE).then_some(find.as_str()) } else { None }).collect();
    assert_eq!(windows, ["line :", "line :s", "line :sw"]);
    a.key(key(KeyCode::Tab));
    assert_eq!(a.line.buf, ":sw", "nothing yet: the palette has no `:switch`");
    answer(&mut a, "line :sw", "command", &[":switch", ":swap"]);
    assert_eq!(a.ghost().as_deref(), Some("ap"), "the core's words, ranked with the palette's");
    a.key(key(KeyCode::Tab));
    a.key(key(KeyCode::Tab));
    assert_eq!(a.line.buf, ":switch");
    a.key(ctrl('u'));
    type_in(&mut a, ":pa");
    answer(&mut a, "line :pa", "command", &[":pack"]);
    assert_eq!(a.candidates().1.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), [":pack"], "a word both hold is one candidate");
}

/// The core's answer to `find` for files: each name, and what the Decider's pick adds to it.
fn files_answer(a: &mut App, find: &str, files: &[(&str, Option<i64>)]) {
    let head = Row { key: "~for".into(), cells: vec![Cell::new(find, Tone::Plain), Cell::new("file", Tone::Dim), Cell::new("", Tone::Dim)], data: Some(json!({ "find": find, "slot": "file", "on": true })), ..Row::default() };
    let row = |(f, lift): &(&str, Option<i64>)| {
        let mut data = json!({ "slot": "file", "text": f, "score": 310 });
        if let Some(n) = lift {
            data["decider"] = json!(n);
            data["made"] = json!(true);
        }
        Row { key: format!("file:{f}"), cells: vec![Cell::new(*f, Tone::Plain), Cell::new("", Tone::Dim)], data: Some(data), ..Row::default() }
    };
    let rows: Vec<Row> = std::iter::once(head).chain(files.iter().map(row)).collect();
    a.down(Down::Rows { bind: j4n_proto::COMPLETE.into(), total: rows.len() as u64, rows, from: 0, rev: 1 });
}

#[test]
fn the_deciders_pick_comes_first_with_its_mark_and_one_that_does_not_come_first_has_none() {
    let mut a = app();
    type_in(&mut a, "open src/m");
    let order = |a: &App| a.candidates().1.iter().map(|c| (c.text.clone(), c.made)).collect::<Vec<_>>();
    // the shorter first by code; the Decider's pick adds its confidence × 60 and comes first, marked
    files_answer(&mut a, "file src/m", &[("src/main.rs", None), ("src/mod_cases.rs", Some(54))]);
    assert_eq!(order(&a), [("src/mod_cases.rs".to_string(), true), ("src/main.rs".to_string(), false)]);
    assert_eq!(a.ghost().as_deref(), Some("od_cases.rs"));
    // a pick that its lift does not bring past a better match is not first, and carries no mark
    files_answer(&mut a, "file src/m", &[("src/main.rs", None), ("lib/src/m.rs", Some(54))]);
    assert_eq!(order(&a), [("src/main.rs".to_string(), false), ("lib/src/m.rs".to_string(), false)]);
}

#[test]
fn with_complete_off_tab_completes_nothing_and_the_core_is_asked_nothing() {
    let mut a = commands();
    let head = Row { key: "~for".into(), cells: vec![Cell::new("", Tone::Plain)], data: Some(json!({ "find": "", "on": false })), ..Row::default() };
    a.down(Down::Rows { bind: j4n_proto::COMPLETE.into(), rows: vec![head], total: 1, from: 0, rev: 1 });
    let ups = type_in(&mut a, ":sw");
    a.key(key(KeyCode::Tab));
    assert_eq!(a.line.buf, ":sw", "nothing completed");
    assert!(!show(&mut a, 60, 12).contains(":switch"), "no ghost");
    assert!(type_in(&mut a, "itch o").iter().chain(&ups).all(|u| !matches!(u, Up::Window { .. })), "no window");
}

#[test]
fn a_chat_is_completed_after_an_at_and_where_the_core_names_the_slot_chat() {
    let mut a = app();
    let chats: Vec<Row> = [("c3", "lexer"), ("c4", "parser")].iter().map(|(id, t)| Row { key: (*id).into(), cells: vec![Cell::new("", Tone::Dim), Cell::new(*id, Tone::Plain), Cell::new(*t, Tone::Plain)], ..Row::default() }).collect();
    a.down(Down::Rows { bind: j4n_proto::CHATS.into(), total: 2, rows: chats, from: 0, rev: 1 });
    type_in(&mut a, "ask @c");
    a.key(key(KeyCode::Tab));
    assert_eq!(a.line.buf, "ask @c3", "the chats the screen holds");
    a.key(ctrl('u'));
    type_in(&mut a, ":archive c");
    answer(&mut a, "line :archive c", "chat", &[]);
    a.key(key(KeyCode::Tab));
    a.key(key(KeyCode::Tab));
    assert_eq!(a.line.buf, ":archive c4", "the core names the slot and the screen fills it");
}

/// The core's answer to `find` when it is whole lines: each with the score the core gave it.
fn lines_answer(a: &mut App, find: &str, slot: &str, lines: &[(&str, i64)]) {
    let head = Row { key: "~for".into(), cells: vec![Cell::new(find, Tone::Plain), Cell::new(slot, Tone::Dim), Cell::new("", Tone::Dim)], data: Some(json!({ "find": find, "slot": slot, "on": true })), ..Row::default() };
    let rows = lines.iter().map(|(t, n)| Row { key: format!("{slot}:{t}"), cells: vec![Cell::new(*t, Tone::Plain), Cell::new("", Tone::Dim)], data: Some(json!({ "slot": slot, "text": t, "score": n })), ..Row::default() });
    let rows: Vec<Row> = std::iter::once(head).chain(rows).collect();
    a.down(Down::Rows { bind: j4n_proto::COMPLETE.into(), total: rows.len() as u64, rows, from: 0, rev: 1 });
}

fn windows(ups: &[Up]) -> Vec<String> {
    ups.iter().filter_map(|u| if let Up::Window { bind, find, .. } = u { (bind == j4n_proto::COMPLETE).then(|| find.clone()) } else { None }).collect()
}

fn question(labels: &[(&str, bool)]) -> AskView {
    let options = labels.iter().map(|(l, rec)| Choice { id: (*l).into(), label: (*l).into(), recommended: *rec, ..Choice::default() }).collect();
    AskView { ask: "q1".into(), asker: "b".into(), question: "which?".into(), shape: Shape::Choice, options, ..AskView::default() }
}

#[test]
fn an_empty_line_and_tab_offer_the_next_line() {
    let mut a = app();
    let ups = a.key(key(KeyCode::Tab));
    assert_eq!(windows(&ups), ["line "], "⇥ on an empty line asks the core for the next line");
    assert!(a.line.buf.is_empty(), "nothing is in the line before the core answers");
    let next = [("deploy the fix to the login page", 324), ("add a test for the empty password", 323), ("fix the parser", 18)];
    lines_answer(&mut a, "line ", "next", &next);
    assert_eq!((a.line.buf.as_str(), a.line.at), ("deploy the fix to the login page", 32), "the first, whole, as the answer comes");
    a.key(key(KeyCode::Tab));
    assert_eq!(a.line.buf, "add a test for the empty password", "⇥ again: the next, in the core's order");
    a.key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(a.line.buf, "deploy the fix to the login page", "⇧⇥: the one before");
    a.key(key(KeyCode::Esc));
    assert!(a.line.buf.is_empty(), "esc puts back the empty line");
    // ⏎ keeps the one in the line and sends nothing; the next ⏎ sends it
    a.key(key(KeyCode::Tab));
    lines_answer(&mut a, "line ", "next", &next);
    assert!(said(&a.key(key(KeyCode::Enter))).is_empty());
    assert_eq!(said(&a.key(key(KeyCode::Enter))), ["deploy the fix to the login page"]);
    // an answer that comes after another key puts nothing in the line
    a.key(key(KeyCode::Tab));
    type_in(&mut a, "x");
    lines_answer(&mut a, "line ", "next", &next);
    assert_eq!(a.line.buf, "x");
    // with an open question, ⇥ on an empty line takes its recommended option, and the core is not asked
    let mut b = app();
    b.down(Down::Asks { open: vec![question(&[("ship it", true), ("wait", false)])] });
    let ups = b.key(key(KeyCode::Tab));
    assert!(windows(&ups).is_empty() && ups.iter().any(|u| matches!(u, Up::Intent { intent: Intent::Answer { .. } })), "{ups:?}");
}

#[test]
fn plain_words_are_completed_by_a_whole_line_and_one_that_may_be_a_key_never_is() {
    let mut a = app();
    assert!(windows(&type_in(&mut a, "de")).is_empty(), "two letters: nothing is asked yet");
    assert_eq!(windows(&type_in(&mut a, "p")), ["line dep"], "three: the line is asked");
    // the core never sends a key; were one there, the screen would offer it nowhere
    lines_answer(&mut a, "line dep", "sentence", &[("Dp7kQ2mW9xR4tY8vB3nL6jH1sF5gA0cE2uI7oP4z", 330), (":key …", 329), ("deploy the fix to the login page", 325), ("deploy the docs", 310)]);
    let (_, found) = a.candidates();
    assert_eq!(found.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["deploy the fix to the login page", "deploy the docs"], "the core's order, nothing that may be a key");
    assert_eq!(a.ghost().as_deref(), Some("loy the fix to the login page"));
    a.key(key(KeyCode::Tab));
    assert_eq!(a.line.buf, "deploy the fix to the login page", "the whole line takes the place of what was typed");
    // the start of a key typed alone is never asked: a one-word line goes only while it reads as a word
    let mut b = app();
    assert!(windows(&type_in(&mut b, "Qm4tH7vR2kX9")).is_empty());
    assert!(windows(&type_in(&mut app(), "dePloy")).is_empty(), "nor a word with a capital inside");
    let mut c = app();
    assert_eq!(windows(&type_in(&mut c, "Deploy")).last().map(String::as_str), Some("line Deploy"), "a capital first is a word");
    // the cursor inside the line: plain words are not asked
    c.key(key(KeyCode::Left));
    assert!(windows(&type_in(&mut c, "e")).is_empty());
    // an open question's options are whole lines too, before the core says anything
    let mut d = app();
    d.down(Down::Asks { open: vec![question(&[("ship it now", false), ("wait for the tests", false)])] });
    type_in(&mut d, "wait f");
    assert_eq!(d.ghost().as_deref(), Some("or the tests"));
    d.key(key(KeyCode::Left));
    assert!(d.candidates().1.is_empty(), "with the cursor inside the line, no whole line");
}

#[test]
fn a_paste_or_a_new_line_ends_the_steps_and_keeps_what_is_in_the_line() {
    let mut a = commands();
    type_in(&mut a, ":s");
    a.key(key(KeyCode::Tab));
    a.paste("x");
    a.key(key(KeyCode::Tab));
    assert_eq!(a.line.buf, ":stopx", "the paste is kept: Tab starts over from it");
}

// ---- Esc: the first thing open takes it; with nothing to close, cancel or give back it goes to the first tab, Chat ----

/// Chat · Work (sub-tabs Own, Inbox) · Log · Settings (a form) · Prefs (a value field), with the questions and a chat with one turn.
fn homes() -> App {
    let work = Node { id: "work".into(), title: "Work".into(), present: Present::Tabs, children: vec![leaf("own", "Own", FieldKind::List, "w:own"), leaf("inbox", "Inbox", FieldKind::List, "w:inbox")], ..Node::default() };
    let name = Node { id: "name".into(), title: "name".into(), field: Some(Field { kind: FieldKind::Text, bind: "cfg:x.name".into(), props: Json::Null }), ..Node::default() };
    let prefs = Node { id: "prefs".into(), title: "Prefs".into(), present: Present::Stack, children: vec![name], ..Node::default() };
    let pages = Node {
        id: "pages".into(),
        present: Present::Tabs,
        children: vec![leaf("chat", "Chat", FieldKind::Chat, "chat:main"), work, leaf("log", "Log", FieldKind::Log, "log:all"), leaf("settings", "Settings", FieldKind::Form, "form:thread"), prefs],
        ..Node::default()
    };
    let root = Node { id: "root".into(), children: vec![pages, leaf("asks", "Questions", FieldKind::Ask, "asks:open"), leaf("bar", "", FieldKind::Bar, "bar:status")], ..Node::default() };
    let mut a = App::new();
    a.start();
    a.down(hello());
    a.down(Down::Tree { node: root });
    a.down(Down::Value { bind: "cfg:x.name".into(), value: json!("ada"), rev: 1 });
    chat(&mut a, &[(j4n_proto::Role::You, "hello")]);
    a
}

fn tab(a: &App) -> String {
    a.top().expect("tabs").children[a.tab].id.clone()
}

fn esc(a: &mut App) -> Vec<Up> {
    a.key(key(KeyCode::Esc))
}

fn intents(ups: &[Up]) -> Vec<Intent> {
    ups.iter().filter_map(|u| if let Up::Intent { intent } = u { Some(intent.clone()) } else { None }).collect()
}

fn act(k: &str, intent: Intent, risk: u8) -> Act {
    Act { key: k.into(), label: k.into(), intent, risk }
}

/// Work › Own with one running task: `x` stops it (asks first), `e` renames it in place.
fn on_work() -> App {
    let mut a = homes();
    let stop = Intent::Say { text: ":stop t1".into(), with: Vec::new() };
    let rename = Intent::Set { kind: "task".into(), id: "t1".into(), field: "name".into(), value: Json::Null };
    let task = Row { key: "t1".into(), cells: vec![Cell::new("t1", Tone::Plain), Cell::new("running", Tone::Accent)], actions: vec![act("x", stop, 60), act("e", rename, 0)], ..Row::default() };
    a.down(Down::Rows { bind: "w:own".into(), rows: vec![task], total: 1, from: 0, rev: 1 });
    a.key(key(KeyCode::F(2)));
    a
}

#[test]
fn esc_on_a_confirm_leaves_it_and_on_an_approval_puts_it_off_and_the_tab_stays() {
    let mut a = on_work();
    a.key(key(KeyCode::Char('x')));
    assert!(a.confirm.is_some(), "the stop asks first");
    assert!(intents(&esc(&mut a)).is_empty(), "esc is the confirm's no: nothing is sent");
    assert!(a.confirm.is_none());
    assert_eq!(tab(&a), "work");
    let push = AskView { ask: "a1".into(), asker: "b".into(), question: "git push?".into(), shape: Shape::Approval, risk: 65, ..AskView::default() };
    a.down(Down::Asks { open: vec![push] });
    assert!(intents(&esc(&mut a)).is_empty(), "never a yes");
    assert!(a.ui.later.contains("a1"), "the approval is put off");
    assert_eq!(tab(&a), "work");
}

#[test]
fn tab_on_a_card_is_the_cards_and_never_lets_a_confirm_go() {
    let mut a = on_work();
    a.key(key(KeyCode::Char('x')));
    assert!(a.key(key(KeyCode::Tab)).is_empty());
    assert!(a.confirm.is_some(), "Tab completes; it does not reach under the card, nor dismiss it");
    assert!(intents(&esc(&mut a)).is_empty());
    assert!(a.confirm.is_none() && tab(&a) == "work", "and the card's esc is still the first");
}

#[test]
fn esc_on_the_restart_card_an_edit_in_place_or_a_value_editor_closes_it_and_the_tab_stays() {
    let mut a = on_work();
    a.restart = Some("look.x".into());
    esc(&mut a);
    assert!(a.restart.is_none(), "the restart card closed");
    assert_eq!(tab(&a), "work");
    a.key(key(KeyCode::Char('e')));
    assert!(a.edit.is_some(), "the rename is typed in place");
    type_in(&mut a, "zz");
    assert!(intents(&esc(&mut a)).is_empty(), "the edit is dropped");
    assert!(a.edit.is_none());
    assert_eq!(tab(&a), "work");
    a.key(key(KeyCode::F(5)));
    a.key(key(KeyCode::Enter));
    type_in(&mut a, "x");
    assert!(a.val.is_some(), "the value is being edited");
    assert!(esc(&mut a).is_empty(), "nothing is set");
    assert!(a.val.is_none());
    assert_eq!(tab(&a), "prefs");
}

#[test]
fn esc_on_the_jump_or_the_sheet_closes_it_and_the_tab_stays() {
    let mut a = on_work();
    a.key(ctrl('k'));
    esc(&mut a);
    assert!(a.jump.is_none());
    assert_eq!(tab(&a), "work");
    a.sheet = true;
    esc(&mut a);
    assert!(!a.sheet);
    assert_eq!(tab(&a), "work");
}

#[test]
fn esc_on_a_find_typed_or_kept_ends_it_and_the_tab_stays() {
    let mut a = homes();
    a.down(Down::Rows { bind: "log:all".into(), rows: vec![Row { key: "e1".into(), cells: vec![Cell::new("ab", Tone::Plain)], ..Row::default() }], total: 1, from: 0, rev: 1 });
    a.key(key(KeyCode::F(3)));
    type_in(&mut a, "/a");
    esc(&mut a);
    assert!(a.wins["log:all"].typing.is_none(), "the find being typed ends");
    assert_eq!(tab(&a), "log");
    type_in(&mut a, "/ab");
    a.key(key(KeyCode::Enter));
    assert_eq!(a.wins["log:all"].find, "ab");
    esc(&mut a);
    assert_eq!(a.wins["log:all"].find, "", "the find kept is cleared");
    assert_eq!(tab(&a), "log");
    esc(&mut a);
    assert_eq!(tab(&a), "chat", "then esc goes home");
}

#[test]
fn esc_with_the_keys_on_the_transcript_or_the_chats_list_gives_them_back_to_the_line() {
    let mut a = homes();
    a.key(KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL));
    assert!(a.wins["chat:main"].at.is_some(), "the keys are on the newest turn");
    assert!(intents(&esc(&mut a)).is_empty());
    assert!(a.wins["chat:main"].at.is_none(), "back on the line");
    let mut b = homes();
    b.down(Down::Value { bind: "chat:here".into(), value: json!({ "id": "", "top": true, "chats": 1 }), rev: 1 });
    let c1 = Row { key: "c1".into(), cells: vec![Cell::new("", Tone::Plain), Cell::new("c1", Tone::Dim), Cell::new("lexer", Tone::Plain)], data: Some(json!({ "chat": "c1" })), ..Row::default() };
    b.down(Down::Rows { bind: j4n_proto::CHATS.into(), rows: vec![c1], total: 1, from: 0, rev: 1 });
    b.key(key(KeyCode::Down));
    assert!(intents(&esc(&mut b)).is_empty());
    assert!(!intents(&b.key(key(KeyCode::Enter))).iter().any(|i| matches!(i, Intent::Enter { .. })), "the list no longer has the keys: ⏎ goes into no chat");
}

#[test]
fn esc_on_a_question_with_an_empty_line_puts_it_off_and_on_the_shapers_card_drops_the_notes_and_keeps_the_line() {
    let mut a = homes();
    let q = AskView { ask: "q1".into(), asker: "b".into(), question: "which?".into(), shape: Shape::Choice, options: vec![Choice { id: "a".into(), label: "a".into(), ..Choice::default() }, Choice { id: "b".into(), label: "b".into(), ..Choice::default() }], ..AskView::default() };
    a.down(Down::Asks { open: vec![q] });
    assert!(intents(&esc(&mut a)).is_empty());
    assert!(a.ui.later.contains("q1"), "put off");
    let mut b = homes();
    b.down(Down::Value { bind: "shape:on".into(), value: Json::Bool(true), rev: 1 });
    type_in(&mut b, "fix the parser thing");
    let card = |key: &str, cells: &[&str]| Row { key: key.into(), cells: cells.iter().map(|c| Cell::new(*c, Tone::Plain)).collect(), ..Row::default() };
    b.down(Down::Rows { bind: "shape:card".into(), total: 2, rows: vec![card("~draft", &["fix the parser thing", "ready", ""]), card("file:1", &["file", "a.rs", "exist"])], from: 0, rev: 1 });
    let ups = esc(&mut b);
    assert_eq!(intents(&ups), [Intent::Run { command: "shape".into(), args: vec!["drop".into()] }], "the notes are dropped");
    assert_eq!(b.line.buf, "fix the parser thing", "the line stays");
}

#[test]
fn esc_in_a_sub_chat_on_an_empty_line_goes_up_one_level_and_a_typed_line_stays_there() {
    let mut a = homes();
    a.down(Down::Value { bind: "chat:here".into(), value: json!({ "id": "c1", "top": false, "crumb": [{ "id": "c1", "title": "lexer" }] }), rev: 1 });
    type_in(&mut a, "half");
    assert!(intents(&esc(&mut a)).is_empty(), "a typed line keeps the chat");
    assert_eq!(a.line.buf, "half");
    a.key(ctrl('u'));
    assert_eq!(intents(&esc(&mut a)), [Intent::Leave], "up one level");
}

#[test]
fn esc_keeps_a_typed_line() {
    let mut a = commands();
    type_in(&mut a, ":sw");
    esc(&mut a);
    assert_eq!(a.line.buf, ":sw", "the first esc hides the ghost");
    assert!(esc(&mut a).is_empty(), "the second does nothing");
    assert_eq!((a.line.buf.as_str(), a.line.at), (":sw", 3), "the line is kept, cursor and all");
    a.key(ctrl('u'));
    assert_eq!(a.line.buf, "", "⌃u empties it");
}

#[test]
fn down_past_the_newest_line_gives_back_the_draft_typed_before_up() {
    let mut a = homes();
    for line in ["one", "two"] {
        type_in(&mut a, line);
        a.key(key(KeyCode::Enter));
    }
    type_in(&mut a, "dra");
    a.key(key(KeyCode::Up));
    a.key(key(KeyCode::Up));
    assert_eq!(a.line.buf, "one");
    a.key(key(KeyCode::Down));
    assert_eq!(a.line.buf, "two");
    a.key(key(KeyCode::Down));
    assert_eq!(a.line.buf, "dra", "past the newest: the draft typed before ↑");
    // esc in a walk gives it back too; a recalled line that was changed is kept
    a.key(key(KeyCode::Up));
    assert_eq!(a.line.buf, "two");
    esc(&mut a);
    assert_eq!(a.line.buf, "dra");
    a.key(key(KeyCode::Up));
    type_in(&mut a, "!");
    esc(&mut a);
    assert_eq!(a.line.buf, "two!", "what was typed is never dropped");
    a.key(key(KeyCode::Down));
    a.key(key(KeyCode::Down));
    assert_eq!(a.line.buf, "dra", "the walk ends on the draft");
    a.key(key(KeyCode::Down));
    assert_eq!(a.line.buf, "dra", "↓ with no walk leaves the line as it is");
}

#[test]
fn esc_with_nothing_open_goes_to_chat_and_the_tab_keeps_its_sub_tab() {
    let mut a = on_work();
    a.key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT));
    assert_eq!(a.sub.get("work"), Some(&1), "Work › Inbox");
    esc(&mut a);
    assert_eq!(tab(&a), "chat", "from a list");
    a.key(key(KeyCode::F(2)));
    assert_eq!((tab(&a).as_str(), a.sub.get("work")), ("work", Some(&1)), "Work kept its sub-tab");
    for f in [3, 4, 5] {
        a.key(key(KeyCode::F(f)));
        assert_ne!(tab(&a), "chat");
        esc(&mut a);
        assert_eq!(tab(&a), "chat", "from F{f}: the Log, a form, a value field");
    }
    assert!(esc(&mut a).is_empty(), "on Chat, at the top, with nothing open: nothing");
    assert_eq!(tab(&a), "chat");
}

#[test]
fn esc_never_stops_a_running_task() {
    let mut a = on_work();
    a.down(Down::Value { bind: "chat:here".into(), value: json!({ "id": "", "top": true, "running": 1 }), rev: 1 });
    assert!(intents(&esc(&mut a)).is_empty(), "nothing is asked of the core");
    assert_eq!(tab(&a), "chat", "only the screen moved");
    assert!(intents(&esc(&mut a)).is_empty());
}

#[test]
fn a_digit_within_40_ms_of_a_bare_esc_is_alt_digit_and_the_esc_does_nothing() {
    use std::time::{Duration, Instant};
    let mut a = homes();
    let q = AskView { ask: "q1".into(), asker: "b".into(), question: "which?".into(), shape: Shape::Choice, options: vec![Choice { id: "a".into(), label: "a".into(), ..Choice::default() }, Choice { id: "b".into(), label: "b".into(), ..Choice::default() }], ..AskView::default() };
    a.down(Down::Asks { open: vec![q] });
    a.esc_waits = true;
    let t0 = Instant::now();
    a.set_clock(t0);
    assert!(esc(&mut a).is_empty(), "a bare Esc waits");
    assert!(a.ui.later.is_empty(), "and has done nothing yet");
    assert!(a.wake(t0).is_some_and(|w| w <= Duration::from_millis(40)), "the screen wakes for it");
    a.set_clock(t0 + Duration::from_millis(10));
    let ups = a.key(key(KeyCode::Char('2')));
    assert_eq!(tab(&a), "work", "Esc and 2 in two reads are ⌥2");
    assert!(a.ui.later.is_empty() && intents(&ups).is_empty(), "the question was not put off, nor answered");
    assert_eq!(a.line.buf, "", "and the 2 typed nothing");
    // a lone Esc acts once its time is up, and the screen is drawn again
    let t1 = t0 + Duration::from_millis(100);
    a.set_clock(t1);
    esc(&mut a);
    assert_eq!(tab(&a), "work");
    a.take_redraw();
    a.tick(t1 + Duration::from_millis(41));
    assert_eq!(tab(&a), "chat", "home");
    assert!(a.take_redraw(), "drawn again");
    // any other key lets the Esc act first, in order
    a.key(key(KeyCode::F(3)));
    let t2 = t1 + Duration::from_millis(500);
    a.set_clock(t2);
    esc(&mut a);
    a.set_clock(t2 + Duration::from_millis(5));
    a.key(key(KeyCode::Char('x')));
    assert_eq!((tab(&a).as_str(), a.line.buf.as_str()), ("chat", "x"), "the Esc went home, then the x was typed there");
    // where the terminal speaks the kitty protocol, Esc is its own code and acts at once
    a.esc_waits = false;
    a.key(key(KeyCode::F(3)));
    esc(&mut a);
    assert_eq!(tab(&a), "chat");
}
