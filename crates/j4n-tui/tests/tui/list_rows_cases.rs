//! The elbow stays on the row the keys are on, the opened row is scrolled into sight with all
//! its detail, the columns share the width of the window, a list has its titles, a row is described once, and Settings is one kind of list.
//! The goldens are the scenes `list-*`.

use crate::world::{banded, hello, screen};
use j4n_proto::{Cell, Down, Field, FieldKind, Json, Node, Present, Row, Tone, json};
use j4n_tui::App;
use j4n_tui::headless::{Headless, KeyCode, press};

fn started(root: Node) -> App {
    let mut a = App::new();
    a.start();
    a.down(hello());
    a.down(Down::Tree { node: root });
    a
}

fn stack(children: Vec<Node>) -> Node {
    Node { id: "root".into(), present: Present::Stack, children, ..Node::default() }
}

fn leaf(id: &str, label: &str, kind: FieldKind, bind: &str, props: Json) -> Node {
    Node { id: id.into(), title: label.into(), field: Some(Field { kind, bind: bind.into(), props }), ..Node::default() }
}

fn hit(a: &mut App, code: KeyCode) {
    a.key(press(code));
}

/// What a setting says of itself: a sentence of 90 columns, so that it is cut at 80 and whole at 120.
fn doc(i: usize) -> String {
    format!("What setting {i:02} is for, said in a sentence of ninety columns, and no more than that, plea")
}

/// A Settings page as the core sends it: the setting, what it is now, what it does; the same said again in full under the open row.
fn settings(n: usize, titles: bool) -> App {
    let props = if titles { json!({ "titles": ["setting", "now", "what it does"] }) } else { Json::Null };
    let mut a = started(stack(vec![leaf("l", "", FieldKind::List, "view:s", props)]));
    let rows = (0..n)
        .map(|i| Row {
            key: format!("s{i:02}"),
            cells: vec![Cell::new(format!("theme.opt{i:02}"), Tone::Plain), Cell::new("round", Tone::Plain), Cell::new(doc(i), Tone::Plain)],
            detail: vec![Cell::new(doc(i), Tone::Dim), Cell::new("default round", Tone::Dim)],
            ..Row::default()
        })
        .collect();
    a.down(Down::Rows { bind: "view:s".into(), rows, total: n as u64, from: 0, rev: 1 });
    a
}

fn lines(a: &mut App, cols: u16, rows: u16) -> Vec<String> {
    screen(a, cols, rows).into_iter().map(|l| l.trim_end().to_string()).collect()
}

// ---- the elbow stays ----

#[test]
fn the_last_row_keeps_its_elbow_when_it_is_selected_and_its_detail_hangs_from_a_blank() {
    let mut a = settings(3, true);
    let first = lines(&mut a, 120, 10);
    assert!(first[1].starts_with("├ theme.opt00"), "{first:?}");
    assert_eq!(first[2], format!("│   {}", doc(0)), "under a row that has others after it the detail hangs from the trunk");
    hit(&mut a, KeyCode::Down);
    hit(&mut a, KeyCode::Down);
    let s = lines(&mut a, 120, 10);
    assert!(s[1].starts_with("├ theme.opt00") && s[2].starts_with("├ theme.opt01") && s[3].starts_with("╰ theme.opt02"), "the elbow is still the elbow: {s:?}");
    assert_eq!(s[4], format!("    {}", doc(2)), "under the last row the trunk is over, so the detail hangs from a blank: {s:?}");
    assert_eq!(s[5], "    default round");
    assert!(banded(&mut a, 120, 10).iter().any(|l| l.starts_with("╰ theme.opt02")), "the selection is the band");
    assert!(!s.iter().any(|l| l.starts_with('┃')), "no heavy bar in front of a row: {s:?}");
}

// ---- the opened part is in sight ----

#[test]
fn moving_down_a_long_list_keeps_the_row_and_all_its_detail_on_the_screen() {
    let mut a = settings(30, true);
    for step in 0..30 {
        let s = lines(&mut a, 100, 8);
        assert!(s.iter().any(|l| l.trim_start_matches(['│', ' ']) == "default round"), "{step}: the end of the detail is in sight: {s:?}");
        assert!(banded(&mut a, 100, 8).iter().any(|l| l.contains(&format!("theme.opt{step:02}"))), "{step}: and so is the row");
        hit(&mut a, KeyCode::Down);
    }
}

#[test]
fn going_back_up_does_not_move_the_view_until_the_row_leaves_it() {
    let mut a = settings(30, true);
    for _ in 0..20 {
        hit(&mut a, KeyCode::Down);
    }
    let low = lines(&mut a, 100, 10);
    hit(&mut a, KeyCode::Up);
    let up = lines(&mut a, 100, 10);
    assert_eq!(low[1], up[1], "the rows above are the same ones: {low:?} {up:?}");
    for _ in 0..19 {
        hit(&mut a, KeyCode::Up);
    }
    assert!(lines(&mut a, 100, 10)[1].starts_with("├ theme.opt00"), "back at the top");
}

#[test]
fn a_stack_of_fields_taller_than_the_screen_scrolls_to_the_one_the_keys_are_on() {
    let fields: Vec<Node> = (0..8).map(|i| leaf(&format!("f{i}"), &format!("field {i}"), FieldKind::Str, &format!("cfg:x.f{i}"), json!({}))).collect();
    let mut a = started(stack(fields));
    assert!(lines(&mut a, 70, 4)[0].starts_with("├ field 0"));
    for i in 1..8 {
        hit(&mut a, KeyCode::Down);
        let s = lines(&mut a, 70, 4);
        assert!(s.iter().any(|l| l.contains(&format!("field {i}"))), "{i}: {s:?}");
        assert!(banded(&mut a, 70, 4).iter().any(|l| l.contains(&format!("field {i}"))), "{i}: the field with the keys is in sight");
    }
    let end = lines(&mut a, 70, 4);
    assert!(end[3].starts_with("╰ field 7"), "the last field ends the pipe: {end:?}");
    for _ in 0..7 {
        hit(&mut a, KeyCode::Up);
    }
    assert!(lines(&mut a, 70, 4)[0].starts_with("├ field 0"), "and back at the top");
}

// ---- the columns share the width ----

#[test]
fn the_description_takes_the_width_that_is_left_and_a_resize_recomputes_it() {
    let mut a = settings(3, true);
    let at = |s: &[String]| s.iter().find(|l| l.starts_with("├ theme.opt01")).cloned().unwrap_or_default();
    let wide = lines(&mut a, 120, 12);
    assert!(at(&wide).ends_with(&doc(1)), "at 120 columns the whole sentence is there: {:?}", at(&wide));
    let mid = lines(&mut a, 80, 12);
    assert!(at(&mid).ends_with('…') && at(&mid).chars().count() == 80, "at 80 it is cut at the edge of the window: {:?}", at(&mid));
    let narrow = lines(&mut a, 60, 12);
    assert!(!at(&narrow).contains("What setting") && at(&narrow) == "├ theme.opt01  round", "under 64 the description goes: {:?}", at(&narrow));
    assert!(narrow.iter().chain(&mid).chain(&wide).all(|l| l.chars().count() <= 120));
    let again = lines(&mut a, 120, 12);
    assert_eq!(again, wide, "and back");
    for w in [60u16, 120] {
        assert!(lines(&mut a, w, 12).iter().all(|l| l.chars().count() <= usize::from(w)), "{w}");
    }
}

#[test]
fn a_long_first_column_gives_up_cells_before_the_description_is_left_with_none() {
    let mut a = started(stack(vec![leaf("l", "", FieldKind::List, "view:s", Json::Null)]));
    let row = |i: usize| Row {
        key: format!("k{i}"),
        cells: vec![Cell::new("x".repeat(60), Tone::Plain), Cell::new("y".repeat(40), Tone::Plain), Cell::new(doc(i), Tone::Plain)],
        detail: vec![Cell::new(doc(i), Tone::Dim)],
        ..Row::default()
    };
    a.down(Down::Rows { bind: "view:s".into(), rows: vec![row(0), row(1)], total: 2, from: 0, rev: 1 });
    let s = lines(&mut a, 90, 6);
    assert!(s[1].contains("What setting"), "the description still shows at 90: {s:?}");
    assert!(s.iter().all(|l| l.chars().count() <= 90));
}

#[test]
fn a_column_the_cap_would_cut_keeps_what_the_last_column_does_not_need() {
    // the proposals page: a title of 41 cells, then a count of one
    let mut a = started(stack(vec![leaf("l", "", FieldKind::List, "view:p", Json::Null)]));
    let title = "Allow \"make build\" here without asking";
    let r = Row { key: "p5".into(), cells: vec![Cell::new("p5", Tone::Plain), Cell::new("open", Tone::Plain), Cell::new(title, Tone::Plain), Cell::new("3", Tone::Plain)], ..Row::default() };
    a.down(Down::Rows { bind: "view:p".into(), rows: vec![r], total: 1, from: 0, rev: 1 });
    assert_eq!(lines(&mut a, 80, 3)[0], format!("╰ p5  open  {title}  3"));
}

#[test]
fn a_head_with_a_hint_stands_beside_its_title_and_the_names_under_it_keep_their_width_at_80_columns() {
    for kind in [FieldKind::List, FieldKind::Tree] {
        let mut a = started(stack(vec![leaf("l", "", kind, "view:c", Json::Null)]));
        let cell = |t: &str| Cell::new(t, Tone::Plain);
        let head = Row { key: "connected".into(), cells: vec![cell("connected"), cell(" "), cell(" "), cell(" "), cell(" "), cell(" "), cell("⏎ opens one · t tests · r learns again · o turns off")], ..Row::default() };
        let one = |name: &str| Row { key: name.into(), cells: vec![cell(name), cell("agent"), cell("ready"), cell("5h 40%"), cell("canary ok"), cell("claude · sonnet 5.5"), cell("signed in, and its login is good for another month")], ..Row::default() };
        a.down(Down::Rows { bind: "view:c".into(), rows: vec![head, one("claude"), one("opencode-acp")], total: 3, from: 0, rev: 1 });
        let s = lines(&mut a, 80, 6);
        let first = s.iter().find(|l| l.contains("connected")).unwrap_or_else(|| panic!("{kind:?}: {s:?}"));
        assert!(first.contains("connected") && first.contains("⏎ opens one"), "{kind:?}: the hint is beside its title, not in a column: {s:?}");
        assert!(s.iter().any(|l| l.contains("opencode-acp")), "{kind:?}: a name is not cut to make room for a note: {s:?}");
        assert!(s.iter().all(|l| !l.contains("connecte ") && l.chars().count() <= 80), "{kind:?}: {s:?}");
    }
}

// ---- one description at a time ----

#[test]
fn the_row_that_is_open_does_not_say_its_description_twice() {
    let mut a = settings(3, true);
    let s = lines(&mut a, 120, 10);
    assert_eq!(s[1], "├ theme.opt00  round", "the open row has no description of its own: {s:?}");
    assert!(s[2].contains(&doc(0)), "it is under the row: {s:?}");
    assert!(s[4].starts_with("├ theme.opt01") && s[4].ends_with(&doc(1)), "the others keep theirs: {s:?}");
}

// ---- titles ----

#[test]
fn a_list_with_titles_has_a_header_row_that_stays_when_the_list_scrolls() {
    let mut a = settings(30, true);
    let s = lines(&mut a, 120, 8);
    assert!(s[0].starts_with("  setting") && s[0].contains("  now  ") && s[0].ends_with("what it does"), "{:?}", s[0]);
    let at = |l: &str, w: &str| l.find(w).map(|b| l[..b].chars().count());
    assert_eq!(at(&s[0], "now"), at(&s[1], "round"), "the titles stand over their columns: {s:?}");
    for _ in 0..25 {
        hit(&mut a, KeyCode::Down);
    }
    assert!(lines(&mut a, 120, 8)[0].starts_with("  setting"), "the titles stay at the top");
}

#[test]
fn a_list_with_no_titles_has_no_header_row() {
    let mut a = settings(3, false);
    assert!(lines(&mut a, 120, 8)[0].starts_with("├ theme.opt00"));
}

#[test]
fn the_titles_are_faint_and_the_row_the_keys_are_on_is_in_the_accent_colour() {
    let mut a = settings(3, true);
    let tones = Headless::new(120, 8).tones(&mut a);
    assert!(tones[0].chars().skip(2).take(7).all(|c| c == '?' || c == 'd'), "{:?}", tones[0]);
    assert!(tones[1].starts_with('c'), "the elbow of the row the keys are on takes the accent: {:?}", tones[1]);
}

// ---- one list style ----

#[test]
fn a_form_of_rows_is_drawn_as_the_same_pipe_list() {
    let mut a = started(stack(vec![leaf("c", "", FieldKind::Form, "form:thread", json!({ "titles": ["setting", "now"] }))]));
    let enter = j4n_proto::Act { key: "enter".into(), label: "change".into(), intent: j4n_proto::Intent::Run { command: "x".into(), args: vec![] }, risk: 0 };
    let row = |k: &str, v: &str, acts: Vec<j4n_proto::Act>| Row { key: k.into(), cells: vec![Cell::new(k, Tone::Dim), Cell::new(v, Tone::Plain)], actions: acts, ..Row::default() };
    a.down(Down::Rows { bind: "form:thread".into(), rows: vec![row("doer", "test", vec![enter.clone()]), row("model", "(the doer's own)", vec![]), row("how", "pinned", vec![]), row("mode", "write", vec![])], total: 4, from: 0, rev: 1 });
    let s = lines(&mut a, 60, 8);
    assert_eq!(s[..5], ["  setting  now", "├ doer     test", "├ model    (the doer's own)", "├ how      pinned", "╰ mode     write"], "{s:?}");
    assert!(!s.iter().any(|l| l.contains("[enter") || l.contains('▸')), "no hint inside the row: {s:?}");
}
