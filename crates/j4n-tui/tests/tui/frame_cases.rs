//! The frame. The tabs stand a row down and a cell in from the sides; the `?` sheet, the
//! `⌃k` jump and the toast are boxes drawn from the theme's corner slots; the sheet wraps a long description and lists a key once;
//! the jump opens under the tab rows and names a parent once. The background is `ground_cases`.

use crate::world::*;
use crossterm::event::KeyCode;
use j4n_proto::{Cell, Down, FieldKind, Node, Present, Row, Tone};
use j4n_tui::App;
use j4n_tui::headless::press as key;
use j4n_tui::theme::{Settings, Theme};

fn hint(id: &str, keys: &str, word: &str, doc: &str) -> Row {
    Row { key: id.into(), cells: vec![Cell::new(keys, Tone::Plain), Cell::new(word, Tone::Dim)], detail: vec![Cell::new(doc, Tone::Dim)], ..Row::default() }
}

const LONG: &str = "The next tab, and then 1–9 for a tab or the same number again for its sub-tabs.";

fn on(settings: Settings) -> App {
    let mut a = app();
    a.theme = Theme::build(&settings, &[]).expect("a theme");
    rows(&mut a, "keys:row", vec![hint("global/tabs", "⌥1–9", "tabs", LONG), hint("chat/send", "⏎", "send", "Send the line."), hint("global/find", "⌃k", "find", "Find anything.")]);
    a
}

/// A tree with a tab that holds tabs, for a jump that has a parent to name.
fn nested() -> Node {
    let group = |id: &str, title: &str, kids: Vec<Node>| Node { id: id.into(), title: title.into(), present: Present::Subtabs, children: kids, ..Node::default() };
    let top = vec![leaf("chat", "Chat", FieldKind::Chat, "chat:main"), group("models", "Models", vec![leaf("trust", "Trust", FieldKind::List, "t:rows"), leaf("hosts", "Hosts", FieldKind::List, "h:rows")]), leaf("tasks", "Tasks", FieldKind::List, "tasks:all")];
    Node { id: "root".into(), present: Present::Tabs, children: top, ..Node::default() }
}

/// The lines of the box that starts with `top` and ends at the first line that closes it.
fn the_box(s: &[String], top: char, bottom: char) -> Vec<String> {
    let at = s.iter().position(|l| l.contains(top)).unwrap_or_else(|| panic!("no `{top}` in {s:#?}"));
    let end = s[at..].iter().position(|l| l.contains(bottom)).unwrap_or_else(|| panic!("no `{bottom}` in {s:#?}")) + at;
    s[at..=end].to_vec()
}

// ---- room above the tabs ----

#[test]
fn the_tabs_stand_one_row_down_and_a_cell_in_from_the_sides() {
    let mut a = on(Settings::default());
    let s = screen(&mut a, 60, 12);
    assert!(s[0].is_empty(), "a blank row above the tabs: {s:#?}");
    assert!(s[1].starts_with("  Chat ") && s[1].ends_with("j4n 0.0.0"), "{s:#?}");
    assert!(s[2].starts_with(" ━") && s[2].chars().count() == 59, "the rule stands in by the margin, one cell each side: {:?}", s[2]);
}

#[test]
fn compact_density_gives_the_row_back_and_keeps_the_margin() {
    let mut a = on(Settings { density: "compact".into(), ..Settings::default() });
    let s = screen(&mut a, 60, 12);
    assert!(s[0].starts_with("  Chat "), "{s:#?}");
    assert!(s[1].starts_with(" ━"), "{s:#?}");
}

#[test]
fn a_screen_too_short_to_spare_the_row_does_not() {
    let mut a = on(Settings::default());
    let s = screen(&mut a, 60, 5);
    assert!(s[0].starts_with("  Chat "), "{s:#?}");
}

// ---- boxes with corners ----

fn open_sheet(a: &mut App) {
    a.key(key(KeyCode::Char('?')));
    assert!(a.sheet);
}

#[test]
fn the_sheet_is_a_round_box_with_its_title_in_the_edge() {
    let mut a = on(Settings::default());
    open_sheet(&mut a);
    let b = the_box(&screen(&mut a, 80, 30), '╭', '╯');
    assert!(b[0].contains("╭─ keys · ") && b[0].trim_end().ends_with('╮'), "{b:#?}");
    assert!(b[1..b.len() - 1].iter().all(|l| l.contains('│')) && b[b.len() - 1].contains('╰'), "{b:#?}");
    assert!(b[1].trim().trim_matches('│').trim().is_empty(), "and a cell of padding inside the line: {b:#?}");
}

#[test]
fn the_corners_of_the_sheet_the_jump_and_the_toast_follow_the_theme() {
    for (corners, tl, br) in [("round", '╭', '╯'), ("square", '┌', '┘')] {
        let mut a = on(Settings { corners: corners.into(), ..Settings::default() });
        open_sheet(&mut a);
        let s = screen(&mut a, 80, 30).join("\n");
        assert!(s.contains(tl) && s.contains(br), "the sheet, {corners}: {s}");
        a.key(key(KeyCode::Esc));
        a.key(j4n_tui::headless::ctrl('k'));
        assert!(a.jump.is_some());
        let s = screen(&mut a, 80, 30).join("\n");
        assert!(s.contains(tl) && s.contains(br), "the jump, {corners}: {s}");
        a.key(key(KeyCode::Esc));
        a.down(Down::Toast { text: "nothing is bound as `x`".into(), tone: Tone::Warn });
        let s = screen(&mut a, 80, 30);
        let at = s.iter().position(|l| l.contains("nothing is bound")).expect("the toast");
        assert!(s[at - 1].contains(tl) && s[at + 1].contains(br), "the toast, {corners}: {s:#?}");
    }
}

// ---- the sheet ----

#[test]
fn the_sheet_wraps_a_long_description_under_its_key_instead_of_cutting_it() {
    for w in [60, 80, 120] {
        let mut a = on(Settings::default());
        open_sheet(&mut a);
        let b = the_box(&screen(&mut a, w, 30), '╭', '╯');
        let inside: Vec<String> = b.iter().map(|l| l.chars().filter(|c| !"│╭╮╰╯─".contains(*c)).collect::<String>()).collect();
        let text = inside.iter().flat_map(|l| l.split_whitespace()).collect::<Vec<_>>().join(" ");
        assert!(text.contains(LONG), "{w}: the whole description is there: {b:#?}");
        assert!(b.iter().all(|l| !l.contains('…') || l.contains("keys ·")), "{w}: and nothing is cut: {b:#?}");
        assert!(b.iter().any(|l| l.contains("sub-tabs.")) && b.iter().filter(|l| l.contains("next tab") || l.contains("The next")).count() <= 2, "{b:#?}");
    }
}

#[test]
fn a_key_is_listed_once_and_the_pages_word_wins() {
    let mut a = on(Settings::default());
    open_sheet(&mut a);
    let b = the_box(&screen(&mut a, 80, 30), '╭', '╯');
    // the keys column is the first eight cells inside the padding
    let lines_of = |keys: &str| b.iter().filter_map(|l| l.trim_start().strip_prefix("│ ")).filter(|l| l.chars().take(8).collect::<String>().trim() == keys).count();
    assert_eq!(lines_of("⌥1–9"), 1, "{b:#?}");
    assert!(b.iter().any(|l| l.contains("⌥1–9 ") && l.contains("tabs")), "the page's word, not the table's 'tab': {b:#?}");
    assert_eq!(lines_of("⌥0"), 1, "a key of the tabs the page does not list is still there: {b:#?}");
}

// ---- the jump ----

#[test]
fn the_jump_opens_under_the_tab_rows_not_over_them() {
    let mut a = on(Settings::default());
    a.down(Down::Tree { node: nested() });
    a.key(j4n_tui::headless::ctrl('k'));
    let s = screen(&mut a, 80, 24);
    assert!(s[1].contains("Chat") && s[1].contains("Models") && s[1].contains("Tasks"), "the tabs are still all there: {s:#?}");
    assert!(s[2].contains('━'), "and their rule: {s:#?}");
    let top = s.iter().position(|l| l.contains("╭─ find")).expect("the box");
    assert_eq!(top, 3, "right under the rule: {s:#?}");
}

#[test]
fn a_parent_is_named_once_as_a_head_and_its_children_stand_under_it() {
    let mut a = on(Settings::default());
    a.down(Down::Tree { node: nested() });
    a.key(j4n_tui::headless::ctrl('k'));
    let titles: Vec<&str> = a.jump.as_ref().expect("open").hits.iter().map(|h| h.title.as_str()).collect();
    assert_eq!(titles, ["Chat", "Models", "Tasks", "Trust", "Hosts"], "the hits are in the order they are drawn, a parent's children together");
    let b = the_box(&screen(&mut a, 80, 24), '╭', '╯');
    // what is inside the line and its padding; a head starts at the padding, a hit two cells in
    let cells: Vec<String> = b.iter().map(|l| l.trim().trim_start_matches('│').trim_end_matches('│').strip_prefix(' ').unwrap_or("").trim_end().to_string()).collect();
    assert_eq!(cells.iter().filter(|l| l.as_str() == "Models").count(), 1, "the head, once: {b:#?}");
    let at = cells.iter().position(|l| l == "Models").expect("the head");
    assert!(cells[at + 1] == "  Trust" && cells[at + 2] == "  Hosts", "{b:#?}");
    assert!(!b.iter().any(|l| l.contains("Trust  Models") || l.contains("Hosts  Models") || l.contains("Tasks  Tasks")), "no title is printed twice on a row: {b:#?}");
}

// ---- the strip when it is short: the title of the core goes first, then the side cell, then the names ----

/// The eight tabs of the real screen, each with its icon, and the page under them.
fn real_tabs() -> Node {
    let tabs = [("chat", "Chat", "chat"), ("tasks", "Tasks", "tasks"), ("connectors", "Connectors", "connector"), ("models", "Models", "models"), ("spend", "Spend", "spend"), ("grow", "Grow", "grow"), ("log", "Log", "log"), ("settings", "Settings", "settings")];
    let kids = tabs.iter().map(|(id, title, icon)| Node { icon: icon.to_string(), ..leaf(id, title, FieldKind::List, &format!("{id}:rows")) }).collect();
    Node { id: "root".into(), present: Present::Tabs, children: kids, ..Node::default() }
}

const NAMES: [&str; 8] = ["Chat", "Tasks", "Connectors", "Models", "Spend", "Grow", "Log", "Settings"];

#[test]
fn the_strip_at_80_columns_shows_every_name_and_gives_up_the_title_of_the_core_first() {
    let mut a = on(Settings::default());
    a.down(Down::Tree { node: real_tabs() });
    let s = screen(&mut a, 80, 24);
    for name in NAMES {
        assert!(s[1].contains(name), "{name} is in the strip at 80 columns: {:?}", s[1]);
    }
    assert!(!s[1].contains("j4n 0.0.0"), "the title is what was given up: {:?}", s[1]);
    assert!(s[1].starts_with(' ') && s[2].starts_with(" ━"), "and the side cell stays: {s:#?}");
    // with the room for it the title stands in its place
    let s = screen(&mut a, 120, 24);
    assert!(NAMES.iter().all(|n| s[1].contains(n)) && s[1].ends_with("j4n 0.0.0"), "{:?}", s[1]);
}

#[test]
fn the_strip_gives_up_the_title_then_the_side_cell_and_the_names_last() {
    let mut a = on(Settings::default());
    a.down(Down::Tree { node: real_tabs() });
    let mut seen = Vec::new();
    for w in 60..=130u16 {
        let s = screen(&mut a, w, 24);
        let (names, title, side) = (NAMES.iter().all(|n| s[1].contains(n)), s[1].contains("j4n 0.0.0"), s[2].starts_with(' '));
        // the title is never kept when a cell at the side is gone, and the names are kept while anything else can go instead
        assert!(!title || side, "{w}: the title goes before the side cell: {s:#?}");
        assert!(names || title && side, "{w}: the names go only when the title and the side cell are gone already... {s:#?}");
        seen.push((w, names, title, side));
    }
    assert!(seen.windows(2).all(|p| !p[0].1 || p[1].1), "once the names fit they fit on every wider screen: {seen:?}");
    assert!(seen.iter().any(|(_, names, title, side)| *names && !*title && *side), "a width with the names, no title and the side cell: {seen:?}");
    assert!(seen.iter().any(|(_, names, title, side)| *names && !*title && !*side), "and one where the side cell is gone too: {seen:?}");
    let narrow = screen(&mut a, 58, 24);
    assert!(!narrow[1].contains("Connectors") && narrow[1].contains('󰚥'), "under 60 columns the icons stand for the names: {:?}", narrow[1]);
}
