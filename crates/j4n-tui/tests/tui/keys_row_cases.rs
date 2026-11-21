//! The keys row, the last row of the screen. It shows the keys of the page that is open from the hints the core
//! sent (`keys:row`), glyph first and a few words; the keys of the tabs while their numbers show; it drops from the right when narrow; `?`
//! opens the sheet; and with the part left out of the tree (`look.keys_row` off) the page has the row back.

use crate::world::*;
use crossterm::event::KeyCode;
use j4n_proto::{Act, Cell, Down, FieldKind, Intent, Node, Present, Row, Shape, Tone};
use j4n_tui::App;
use j4n_tui::headless::{Headless, press as key, typed};
use std::time::Instant;

fn group(id: &str, kids: Vec<Node>) -> Node {
    Node { id: id.into(), title: id.into(), present: Present::Tabs, children: kids, ..Node::default() }
}

/// Chat · Work (sub-tabs: Work, Asks) · Log · Data (a table) · Settings, then the questions, the status bar and the keys row.
fn tree(keys_row: bool) -> Node {
    let pages = Node {
        id: "pages".into(),
        present: Present::Tabs,
        children: vec![
            leaf("chat", "Chat", FieldKind::Chat, "chat:main"),
            group("work", vec![leaf("work.own", "Work", FieldKind::List, "w:own"), leaf("asks", "Asks", FieldKind::List, "w:asks")]),
            leaf("log", "Log", FieldKind::Log, "log:all"),
            leaf("data", "Data", FieldKind::Table, "t:data"),
            leaf("settings", "Settings", FieldKind::Form, "form:thread"),
        ],
        ..Node::default()
    };
    let mut kids = vec![pages, leaf("asks", "Questions", FieldKind::Ask, "asks:open"), leaf("bar", "", FieldKind::Bar, "bar:status")];
    if keys_row {
        kids.push(leaf("keys", "", FieldKind::Bar, "keys:row"));
    }
    Node { id: "root".into(), children: kids, ..Node::default() }
}

/// The hints as the core sends them: the id, the keys, the words, what it does.
fn hint(id: &str, keys: &str, word: &str, doc: &str) -> Row {
    Row { key: id.into(), cells: vec![Cell::new(keys, Tone::Plain), Cell::new(word, Tone::Dim)], detail: vec![Cell::new(doc, Tone::Dim)], ..Row::default() }
}

fn hints() -> Vec<Row> {
    vec![
        hint("global/tabs", "⌥1–9", "tabs", "Tab n."),
        hint("ask/pick", "1–9", "answer", "Answer."),
        hint("chat/send", "⏎", "send", "Send the line."),
        hint("list/move", "↑↓", "move", "Row above or below."),
        hint("table/move", "↑↓", "move", "Row above or below."),
        hint("list/open", "⏎", "open", "Open the row."),
        hint("table/open", "⏎", "open", "Open the row."),
        hint("chat/history", "↑", "history", "Older line."),
        hint("table/find", "/", "find", "Find."),
        hint("page:work.own/pin", "p", "pin", "Pin the work."),
        hint("log/follow", "G", "follow", "Follow the end."),
        hint("log/raw", "r", "raw", "The raw JSON."),
        hint("page:asks/open", "⏎", "", "Hidden by the page."),
        hint("global/keys", "?", "keys", "All the keys."),
        hint("ask/keys", "?", "", "?  is why while a question is open."),
    ]
}

fn on_with(keys_row: bool) -> App {
    let mut a = App::new();
    a.start();
    a.down(hello());
    a.down(Down::Tree { node: tree(keys_row) });
    rows(&mut a, "keys:row", hints());
    a.set_clock(Instant::now());
    a
}

fn on() -> App {
    on_with(true)
}

/// The last row of the screen at this width.
fn last(a: &mut App, w: u16) -> String {
    let s = Headless::new(w, 12).show(a);
    s.lines().last().unwrap_or_default().to_string()
}

fn go(a: &mut App, code: KeyCode) {
    a.key(key(code));
}

#[test]
fn the_chat_page_ends_in_the_keys_of_the_chat_and_the_screen() {
    let mut a = on();
    assert_eq!(last(&mut a, 80), " ⌥1–9 tabs  ⏎ send  ↑ history  ? keys");
}

#[test]
fn a_list_page_has_the_keys_of_a_list_and_a_table_those_of_a_table() {
    let mut a = on();
    a.tab = 1;
    assert_eq!(last(&mut a, 80), " ⌥1–9 tabs  ↑↓ move  ⏎ open  p pin  ? keys", "the page's own key, after the field's, in the order of the core");
    a.tab = 3;
    assert_eq!(last(&mut a, 80), " ⌥1–9 tabs  ↑↓ move  ⏎ open  / find  ? keys");
    a.tab = 2;
    assert_eq!(last(&mut a, 80), " ⌥1–9 tabs  G follow  r raw  ? keys", "the Log's own, and none of a list's");
}

#[test]
fn a_page_with_sub_tabs_shows_the_keys_of_the_sub_tab_that_is_open() {
    let mut a = on();
    a.tab = 1;
    assert!(last(&mut a, 80).contains("p pin"), "the first sub-tab has its key");
    a.sub.insert("work".into(), 1);
    let row = last(&mut a, 80);
    assert!(!row.contains("p pin") && row.contains("↑↓ move"), "{row}");
}

#[test]
fn a_page_hides_a_hint_of_its_field_with_an_empty_word_and_the_nearest_place_wins() {
    let mut a = on();
    a.tab = 1;
    a.sub.insert("work".into(), 1);
    // `page:asks/open` has no word: the list's `open` is hidden on that page
    assert_eq!(last(&mut a, 80), " ⌥1–9 tabs  ↑↓ move  ? keys");
}

#[test]
fn while_the_numbers_show_the_row_is_the_keys_of_the_tabs() {
    let mut a = on();
    a.key(j4n_tui::headless::alt('2'));
    assert_eq!(a.tab, 1);
    assert_eq!(last(&mut a, 80), " ⌥1–9 tab  ⌥n again sub-tab  ⌥←→ sub-tabs  F1–F9");
    // the window passes: the keys of the page again
    a.tick(Instant::now() + std::time::Duration::from_secs(5));
    a.tick(Instant::now() + std::time::Duration::from_secs(10));
    assert_eq!(last(&mut a, 80), " ⌥1–9 tabs  ↑↓ move  ⏎ open  p pin  ? keys");
}

#[test]
fn a_narrow_screen_drops_hints_from_the_right_whole() {
    let mut a = on();
    a.tab = 3;
    assert_eq!(last(&mut a, 60), " ⌥1–9 tabs  ↑↓ move  ⏎ open  / find  ? keys");
    assert_eq!(last(&mut a, 43), " ⌥1–9 tabs  ↑↓ move  ⏎ open  / find  ? keys");
    assert_eq!(last(&mut a, 35), " ⌥1–9 tabs  ↑↓ move  ⏎ open  / find");
    assert_eq!(last(&mut a, 34), " ⌥1–9 tabs  ↑↓ move  ⏎ open", "the last to stand goes first, never half a hint");
    assert_eq!(last(&mut a, 12), " ⌥1–9 tabs", "one at least, cut by the width of the screen");
    a.key(j4n_tui::headless::alt('4'));
    assert_eq!(last(&mut a, 42), " ⌥1–9 tab  ⌥n again sub-tab  ⌥←→ sub-tabs", "the keys of the tabs drop from the right too");
}

#[test]
fn with_the_part_left_out_of_the_tree_there_is_no_keys_row() {
    let mut a = on_with(false);
    let s = Headless::new(80, 12).show(&mut a);
    assert!(!s.contains("⌥1–9 tabs"), "{s}");
    assert!(!s.lines().last().unwrap_or_default().contains("keys"), "{s}");
}

#[test]
fn the_status_bar_is_the_row_above_and_the_keys_row_shows_no_status() {
    let mut a = on();
    rows(&mut a, "bar:status", vec![row("s", &["idle", "tokens 12"])]);
    let s = Headless::new(80, 12).show(&mut a);
    let lines: Vec<&str> = s.lines().collect();
    assert!(lines[lines.len() - 2].contains("tokens 12"), "{s}");
    assert!(!lines[lines.len() - 1].contains("tokens") && !lines[lines.len() - 1].contains("idle"), "{s}");
}

#[test]
fn question_mark_opens_the_sheet_of_the_page_and_any_key_shuts_it() {
    let mut a = on();
    a.tab = 3;
    go(&mut a, KeyCode::Char('?'));
    assert!(a.sheet);
    let s = Headless::new(80, 24).show(&mut a);
    for want in ["keys · Data", "↑↓", "move", "Row above or below.", "Find.", "any key closes", "⌥n again", "F1–F9"] {
        assert!(s.contains(want), "{want}: {s}");
    }
    go(&mut a, KeyCode::Char('j'));
    assert!(!a.sheet, "any key shuts it, and is not read again");
    assert!(!Headless::new(80, 24).show(&mut a).contains("any key closes"));
}

#[test]
fn question_mark_in_the_chat_opens_the_sheet_only_for_an_empty_draft_and_no_open_question() {
    let mut a = on();
    go(&mut a, KeyCode::Char('?'));
    assert!(a.sheet, "an empty draft");
    go(&mut a, KeyCode::Esc);
    for k in typed("what", false) {
        a.key(k);
    }
    go(&mut a, KeyCode::Char('?'));
    assert!(!a.sheet);
    assert_eq!(a.line.buf, "what?", "a question mark in a line is text");
}

#[test]
fn an_open_question_changes_the_row_to_its_keys_and_leaves_question_mark_to_why() {
    let mut a = on();
    a.down(Down::Asks { open: vec![ask(Shape::Choice)] });
    let row = last(&mut a, 80);
    assert!(row.contains("1–9 answer") && row.contains("⏎ send"), "{row}");
    assert!(!row.contains("? keys"), "the global `?` is hidden by the question's empty word: {row}");
    go(&mut a, KeyCode::Char('?'));
    assert!(!a.sheet);
}

#[test]
fn question_mark_does_not_open_the_sheet_while_a_find_is_being_typed() {
    let mut a = on();
    a.tab = 3;
    rows(&mut a, "t:data", vec![row("r1", &["a"])]);
    go(&mut a, KeyCode::Char('/'));
    go(&mut a, KeyCode::Char('?'));
    assert!(!a.sheet);
}

// ---- the keys that go into the list of chats and back ----

fn chat_hints(a: &mut App) {
    let mut all = hints();
    // the core sends them in the order of `order`: `← main` (25) stands after send and before the history
    let at = all.iter().position(|r| r.key == "chat/history").expect("the history's hint");
    all.insert(at, hint("sub/main", "←", "main", "Back to the orchestrator."));
    all.extend([
        hint("pick/move", "↑↓", "move", "The chat above or below."),
        hint("pick/open", "⏎ ⇥", "open", "Go into the chat."),
        hint("pick/back", "esc", "back", "Give the keys back."),
    ]);
    rows(a, "keys:row", all);
}

fn here(a: &mut App, top: bool) {
    let v = j4n_proto::json!({"top": top, "id": if top { "" } else { "c1" }, "crumb": [], "chats": 1, "running": 0, "waits": 0, "state": "", "icon": "", "today": "", "windows": []});
    a.down(Down::Value { bind: "chat:here".into(), value: v, rev: 1 });
}

#[test]
fn a_chat_under_the_orchestrator_says_left_goes_back_while_the_line_is_empty() {
    let mut a = on();
    chat_hints(&mut a);
    here(&mut a, true);
    assert_eq!(last(&mut a, 80), " ⌥1–9 tabs  ⏎ send  ↑ history  ? keys", "the orchestrator has nowhere to go back to");
    here(&mut a, false);
    assert_eq!(last(&mut a, 80), " ⌥1–9 tabs  ⏎ send  ← main  ↑ history  ? keys");
    a.key(key(KeyCode::Char('x')));
    assert_eq!(last(&mut a, 80), " ⌥1–9 tabs  ⏎ send  ↑ history  ? keys", "with text ← is the cursor's, and the row does not say it is not");
}

#[test]
fn while_the_list_of_the_chats_has_the_keys_the_row_shows_its_keys_and_only_those() {
    let mut a = on();
    chat_hints(&mut a);
    here(&mut a, true);
    rows(&mut a, "chats:list", vec![row("c1", &["", "c1", "a chat"])]);
    go(&mut a, KeyCode::Down);
    assert_eq!(last(&mut a, 80), " ↑↓ move  ⏎ ⇥ open  esc back");
    go(&mut a, KeyCode::Esc);
    assert_eq!(last(&mut a, 80), " ⌥1–9 tabs  ⏎ send  ↑ history  ? keys");
}

#[test]
fn a_chat_with_a_lane_that_is_over_or_a_chats_end_says_how_to_select_and_open_it() {
    let mut a = on();
    let mut all = hints();
    let at = all.iter().position(|r| r.key == "chat/history").expect("the history's hint");
    all.insert(at + 1, hint("lanes/open", "⌃↑", "select · ⏎ open", "Put the keys on a lane or a chat's end."));
    rows(&mut a, "keys:row", all);
    let lane = |key: &str, icon: &str| Row { key: key.into(), cells: vec![Cell::with_icon(icon, "A omp", Tone::Plain)], parent: Some("t".into()), data: Some(j4n_proto::json!({"lane": {"doer": "omp", "own": true}})), ..Row::default() };
    rows(&mut a, "chat:main", vec![lane("a", "working"), lane("b", "queued")]);
    assert_eq!(last(&mut a, 80), " ⌥1–9 tabs  ⏎ send  ↑ history  ? keys", "no lane is over: nothing to open");
    rows(&mut a, "chat:main", vec![lane("a", "done")]);
    assert_eq!(last(&mut a, 80), " ⌥1–9 tabs  ⏎ send  ↑ history  ? keys", "a turn with one lane has nothing folded");
    rows(&mut a, "chat:main", vec![lane("a", "done"), lane("b", "working")]);
    assert_eq!(last(&mut a, 80), " ⌥1–9 tabs  ⏎ send  ↑ history  ⌃↑ select · ⏎ open  ? keys");
    let end = Row { key: "e".into(), cells: vec![Cell::with_icon("done", "c1", Tone::Plain)], data: Some(j4n_proto::json!({"door": "c1"})), ..Row::default() };
    rows(&mut a, "chat:main", vec![end]);
    assert!(last(&mut a, 80).contains("⌃↑ select · ⏎ open"), "a chat's end opens too");
}

// ---- the keys that are live only now are in the row, first and marked ----

fn act(key: &str, label: &str) -> Act {
    Act { key: key.into(), label: label.into(), intent: Intent::Run { command: label.into(), args: Vec::new() }, risk: 0 }
}

fn row_with(key: &str, text: &str, acts: Vec<Act>) -> Row {
    Row { key: key.into(), cells: vec![Cell::new(text, Tone::Plain)], actions: acts, ..Row::default() }
}

#[test]
fn the_newest_notes_act_is_the_first_hint_marked_as_temporary_and_a_later_row_hides_it() {
    let mut a = on();
    rows(&mut a, "chat:main", vec![row_with("n1", "chats.on is true (⟳ ⏎ restarts now)", vec![act("enter", "restart now")])]);
    let h = a.hints();
    assert_eq!((h[0].keys.as_str(), h[0].word.as_str(), h[0].temp), ("⏎", "restart now", true), "{h:?}");
    assert!(h.iter().skip(1).all(|h| !h.temp && h.keys != "⏎"), "the chat's own `⏎ send` gives way: a bare ⏎ does the act {h:?}");
    assert_eq!(last(&mut a, 80), " • ⏎ restart now  ⌥1–9 tabs  ↑ history  ? keys");
    // the mark and the tone are one part of the theme, the accent
    let (mark, style) = a.theme.temp_hint();
    assert_eq!((mark, style), ("•", a.theme.fg(j4n_tui::theme::palette::Role::Accent)));
    // typing a line: the next ⏎ sends it, so the act is not offered
    a.line = j4n_tui::parts::line::Typed::new("hello");
    assert!(a.hints().iter().all(|h| !h.temp), "{:?}", a.hints());
    a.line = j4n_tui::parts::line::Typed::new("");
    assert!(a.hints()[0].temp);
    // a newer row (it carries no act) hides it
    rows(&mut a, "chat:main", vec![row_with("n1", "old", vec![act("enter", "restart now")]), row_with("n2", "newer", Vec::new())]);
    assert_eq!(last(&mut a, 80), " ⌥1–9 tabs  ⏎ send  ↑ history  ? keys");
}

#[test]
fn the_selected_rows_actions_are_in_the_row_and_a_row_without_actions_shows_none() {
    let mut a = on();
    a.tab = 1;
    rows(&mut a, "w:own", vec![row_with("w1", "one", vec![act("t", "test"), act("r", "again")]), row_with("w2", "two", Vec::new())]);
    assert_eq!(last(&mut a, 80), " • t test  • r again  ⌥1–9 tabs  ↑↓ move  ⏎ open  p pin  ? keys");
    go(&mut a, KeyCode::Down);
    assert_eq!(last(&mut a, 80), " ⌥1–9 tabs  ↑↓ move  ⏎ open  p pin  ? keys", "no actions on this row, no temporary keys");
    // a row key of the page's own name takes the place of its hint (`⏎ open` becomes the row's `⏎`), and a Log has none
    rows(&mut a, "w:own", vec![row_with("w1", "one", vec![act("enter", "turn on"), act(" ", "flip")])]);
    go(&mut a, KeyCode::Up);
    assert_eq!(last(&mut a, 80), " • ⏎ turn on  • space flip  ⌥1–9 tabs  ↑↓ move  p pin  ? keys");
    a.tab = 2;
    assert!(a.hints().iter().all(|h| !h.temp));
}

// ---- the keys of the completion ----

/// The hints with the completion's own places, as the core sends them (their orders stand first), and a palette of four commands.
fn complete_hints(a: &mut App) {
    let mut all = vec![
        hint("ghost/take", "⇥", "take", "Take the suggestion whole; ⇥ again puts the next in the line."),
        hint("ghost/word", "⌃→", "word", "Take one word of it."),
        hint("step/next", "⇥", "next", "The next suggestion in the line."),
        hint("step/back", "⇧⇥", "back", "The one before."),
        hint("step/send", "⏎", "keep", "Keep the one in the line."),
        hint("step/undo", "esc", "undo", "Put back what was typed."),
        hint("step/history", "↑", "", "Hidden while Tab steps."),
    ];
    all.extend(hints());
    rows(a, "keys:row", all);
    let palette: Vec<Row> = ["switch", "status", "stop", "setup"].iter().map(|w| row(w, &[&format!(":{w}")])).collect();
    rows(a, j4n_proto::PALETTE, palette);
}

#[test]
fn the_row_says_take_while_a_ghost_shows_and_next_while_tab_steps() {
    let mut a = on();
    complete_hints(&mut a);
    for k in typed(":sw", false) {
        a.key(k);
    }
    assert_eq!(last(&mut a, 80), " ⇥ take  ⌃→ word  ⌥1–9 tabs  ⏎ send  ↑ history  ? keys", "a ghost shows");
    a.key(j4n_tui::headless::ctrl('u'));
    for k in typed(":s", false) {
        a.key(k);
    }
    go(&mut a, KeyCode::Tab);
    assert_eq!(last(&mut a, 80), " ⇥ next  ⇧⇥ back  ⏎ keep  esc undo  ⌥1–9 tabs  ? keys", "Tab steps: ⏎ keeps, and the history is hidden");
    go(&mut a, KeyCode::Esc);
    a.key(j4n_tui::headless::ctrl('u'));
    assert_eq!(last(&mut a, 80), " ⌥1–9 tabs  ⏎ send  ↑ history  ? keys", "nothing to complete, the row as it was");
}

#[test]
fn the_sheet_of_the_chat_lists_the_keys_of_the_completion_each_key_once() {
    let mut a = on();
    complete_hints(&mut a);
    go(&mut a, KeyCode::Char('?'));
    assert!(a.sheet);
    let s = Headless::new(80, 30).show(&mut a);
    for want in ["Take the suggestion whole", "⌃→", "Take one word of it.", "⇧⇥", "The one before.", "Put back what was typed."] {
        assert!(s.contains(want), "{want}: {s}");
    }
    assert!(!s.contains("The next suggestion") && !s.contains("Keep the one in the line"), "⇥ and ⏎ are listed once, by their first word: {s}");
}
