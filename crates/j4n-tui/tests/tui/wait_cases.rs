//! A part of a page is always in one of three states and says which: waiting for the core (the loading sign, after a short delay so a fast
//! page never flickers, and `the core is busy` once the wait has gone on), empty (`nothing here`), or filled. The clock is the one the screen
//! is told (`set_clock`, `tick`); nothing here sleeps.

use crate::world::{hello, leaf, row, show, still};
use j4n_proto::{Down, FieldKind, Json, Node, Present, Row, json};
use j4n_tui::App;
use j4n_tui::wait::{BUSY, DELAY};
use std::time::{Duration, Instant};

const SIGN: [char; 2] = ['━', '─'];

fn page(children: Vec<Node>) -> Node {
    Node { id: "root".into(), present: Present::Stack, children, ..Node::default() }
}

/// A screen told the time `t0` before its page came, so every watch it sends is stamped with it.
fn at(t0: Instant, root: Node) -> App {
    let mut a = App::new();
    a.set_clock(t0);
    a.start();
    a.down(hello());
    a.down(Down::Tree { node: root });
    a
}

/// The rows of `bind` come; a windowed field may ask for its window after them, which is not what these cases are about.
fn came(a: &mut App, bind: &str, list: Vec<Row>) {
    let total = list.len() as u64;
    a.down(Down::Rows { bind: bind.into(), rows: list, total, from: 0, rev: 1 });
}

fn signed(s: &str) -> bool {
    s.contains("loading") && s.chars().any(|c| SIGN.contains(&c))
}

#[test]
fn a_list_whose_rows_have_not_come_shows_nothing_at_first_then_the_sign_then_that_the_core_is_busy() {
    let t0 = Instant::now();
    let mut a = at(t0, page(vec![leaf("l", "Rows", FieldKind::List, "w:rows")]));
    let s = show(&mut a, 60, 6);
    assert!(!s.contains("loading") && !s.contains("nothing here"), "a page that may fill at once draws no sign yet:\n{s}");
    let wake = a.wake(t0).expect("the screen wakes for the moment the sign is due");
    assert!(wake <= DELAY, "it wakes within the delay, not later: {wake:?}");
    a.tick(t0 + DELAY);
    assert!(a.take_redraw(), "the moment the sign is due draws the screen again");
    let s = show(&mut a, 60, 6);
    assert!(signed(&s) && !s.contains("busy"), "the sign and the word, once the wait is long enough to see:\n{s}");
    assert!(a.signs, "a sign on the screen wakes it for its next frame");
    a.tick(t0 + BUSY);
    let s = show(&mut a, 60, 6);
    assert!(signed(&s) && s.contains("loading · the core is busy"), "a long wait says why:\n{s}");
    came(&mut a, "w:rows", Vec::new());
    let s = show(&mut a, 60, 6);
    assert!(s.contains("nothing here") && !s.contains("loading"), "rows that came and are none say so:\n{s}");
    assert!(!a.signs, "no sign, no wake for it");
    came(&mut a, "w:rows", vec![row("a", &["alpha"])]);
    let s = show(&mut a, 60, 6);
    assert!(s.contains("alpha") && !s.contains("nothing here") && !s.contains("loading"), "rows that came are shown:\n{s}");
}

#[test]
fn the_rows_that_come_before_the_delay_never_show_the_sign_and_a_screen_without_a_clock_shows_none() {
    let t0 = Instant::now();
    let mut a = at(t0, page(vec![leaf("l", "Rows", FieldKind::List, "w:rows")]));
    a.tick(t0 + DELAY / 2);
    came(&mut a, "w:rows", vec![row("a", &["alpha"])]);
    a.tick(t0 + DELAY * 2);
    let s = show(&mut a, 60, 6);
    assert!(s.contains("alpha") && !s.contains("loading"), "{s}");
    assert_eq!(a.wake(t0 + DELAY * 2), None, "nothing waits, so the clock is no reason to wake");
    // a screen that was never told the time cannot say how long it waited, and draws as it did
    let mut b = App::new();
    b.start();
    b.down(hello());
    b.down(Down::Tree { node: page(vec![leaf("l", "Rows", FieldKind::List, "w:rows")]) });
    assert!(!show(&mut b, 60, 6).contains("loading"));
}

#[test]
fn with_motion_off_the_sign_stands_still_and_the_screen_still_wakes_for_the_busy_words() {
    let t0 = Instant::now();
    let mut a = at(t0, page(vec![leaf("l", "Rows", FieldKind::List, "w:rows")]));
    still(&mut a);
    a.tick(t0 + DELAY);
    let s = show(&mut a, 60, 6);
    assert!(signed(&s), "{s}");
    let wake = a.wake(t0 + DELAY).expect("the busy words are a reason to wake");
    assert!(wake <= BUSY - DELAY && wake > Duration::from_millis(500), "it wakes for the words, not for frames of a sign that stands still: {wake:?}");
    a.tick(t0 + BUSY);
    assert!(a.take_redraw());
    assert!(show(&mut a, 60, 6).contains("the core is busy"));
}

#[test]
fn a_chat_drawn_after_a_part_that_waits_keeps_its_sign_moving() {
    let t0 = Instant::now();
    let mut a = at(t0, page(vec![leaf("l", "Rows", FieldKind::List, "w:rows"), leaf("c", "Chat", FieldKind::Chat, "chat:main")]));
    a.tick(t0 + DELAY);
    let s = show(&mut a, 60, 12);
    assert!(signed(&s), "{s}");
    assert!(a.signs, "the chat, which has no sign of its own, leaves the list's sign waking the screen");
}

#[test]
fn a_table_a_form_a_tree_and_cards_wait_and_empty_the_same_way() {
    let t0 = Instant::now();
    let table = Node { field: Some(j4n_proto::Field { kind: FieldKind::Table, bind: "t:rows".into(), props: json!({ "columns": ["name"] }) }), ..leaf("t", "T", FieldKind::Table, "t:rows") };
    let form = leaf("f", "F", FieldKind::Form, "f:rows");
    let tree = leaf("tr", "Tr", FieldKind::Tree, "tr:rows");
    let cards = leaf("c", "C", FieldKind::Cards, "c:rows");
    for (node, bind) in [(table, "t:rows"), (form, "f:rows"), (tree, "tr:rows"), (cards, "c:rows")] {
        let mut a = at(t0, page(vec![node]));
        a.tick(t0 + DELAY);
        assert!(signed(&show(&mut a, 60, 6)), "{bind} waits with the sign");
        came(&mut a, bind, Vec::new());
        let s = show(&mut a, 60, 6);
        assert!(s.contains("nothing here") && !s.contains("loading"), "{bind}:\n{s}");
    }
}

#[test]
fn a_log_waits_then_says_it_is_empty_then_shows_its_rows() {
    let t0 = Instant::now();
    let mut a = at(t0, page(vec![leaf("log", "Log", FieldKind::Log, "log:rows")]));
    let s = show(&mut a, 60, 6);
    assert!(!s.contains("loading") && !s.contains("of 0"), "{s}");
    a.tick(t0 + DELAY);
    let s = show(&mut a, 60, 6);
    assert!(signed(&s) && !s.contains("of 0"), "a log that waits counts no rows:\n{s}");
    came(&mut a, "log:rows", Vec::new());
    let s = show(&mut a, 60, 6);
    assert!(s.contains("nothing here") && !s.contains("loading"), "{s}");
    came(&mut a, "log:rows", vec![row("1", &["12:00", "a line was said"])]);
    let s = show(&mut a, 60, 6);
    assert!(s.contains("a line was said") && !s.contains("nothing here"), "{s}");
}

#[test]
fn a_chart_whose_value_has_not_come_shows_its_label_and_the_sign_and_one_with_no_data_a_dash() {
    let t0 = Instant::now();
    let chart = Node { field: Some(j4n_proto::Field { kind: FieldKind::Line, bind: "chart:x".into(), props: json!({ "series": [{ "name": "usd", "unit": "usd" }] }) }), ..leaf("c", "a day", FieldKind::Line, "chart:x") };
    let mut a = at(t0, page(vec![chart]));
    let s = show(&mut a, 60, 10);
    assert!(s.contains("a day") && !s.contains('—') && !s.contains("loading"), "its label alone, not the dash of no data:\n{s}");
    a.tick(t0 + DELAY);
    let s = show(&mut a, 60, 10);
    assert!(s.contains("a day") && signed(&s) && !s.contains('—'), "{s}");
    a.down(Down::Value { bind: "chart:x".into(), value: Json::Null, rev: 1 });
    let s = show(&mut a, 60, 10);
    assert!(s.contains("a day  —") && !s.contains("loading"), "no data is a dash:\n{s}");
    a.down(Down::Value { bind: "chart:x".into(), value: json!({ "x": ["a", "b", "c"], "series": { "usd": [1, 3, 2] } }), rev: 2 });
    let s = show(&mut a, 60, 10);
    assert!(!s.contains('—') && !s.contains("loading") && s.contains("usd"), "a chart with data draws it:\n{s}");
}

#[test]
fn a_page_whose_parts_were_asked_for_shows_the_sign_until_they_come() {
    let t0 = Instant::now();
    let lazy = Node { id: "later".into(), title: "Later".into(), present: Present::Stack, lazy: true, ..Node::default() };
    let mut a = at(t0, Node { id: "root".into(), present: Present::Tabs, children: vec![lazy], ..Node::default() });
    assert!(a.asked.contains("later"), "the page was asked for");
    assert!(!show(&mut a, 60, 8).contains("loading"));
    a.tick(t0 + DELAY);
    assert!(signed(&show(&mut a, 60, 8)));
    let full = Node { id: "later".into(), title: "Later".into(), present: Present::Stack, children: vec![leaf("l", "Rows", FieldKind::List, "w:rows")], ..Node::default() };
    a.down(Down::Tree { node: full });
    came(&mut a, "w:rows", vec![row("a", &["alpha"])]);
    let s = show(&mut a, 60, 8);
    assert!(s.contains("alpha") && !s.contains("loading"), "{s}");
}
