//! The display fields. What each shows for the value or the rows it is given, what a key does to it, and which rows of a
//! binding the screen asks the core for. The sheets are the goldens of the scenes `display-*`.

use crate::world::{banded, hello, screen};
use j4n_proto::{Act, Cell, Down, Field, FieldKind, Intent, Json, Node, Present, Row, Tone, Up, json};
use j4n_tui::App;
use j4n_tui::display::{self, group};
use j4n_tui::headless::{Headless, KeyCode, ctrl, press, typed};
use j4n_tui::theme::{Settings, Theme};
use std::time::Instant;

fn node(id: &str, label: &str, kind: FieldKind, bind: &str, props: Json) -> Node {
    Node { id: id.into(), title: label.into(), field: Some(Field { kind, bind: bind.into(), props }), ..Node::default() }
}

fn stack(children: Vec<Node>) -> Node {
    Node { id: "root".into(), present: Present::Stack, children, ..Node::default() }
}

fn started(root: Node) -> App {
    let mut a = App::new();
    a.start();
    a.down(hello());
    a.down(Down::Tree { node: root });
    a
}

/// An app with one display field on the page, given a value.
fn one(kind: FieldKind, label: &str, props: Json, value: Json) -> App {
    let mut a = started(stack(vec![node("f", label, kind, "d:f", props)]));
    a.down(Down::Value { bind: "d:f".into(), value, rev: 1 });
    a
}

fn cell(text: &str) -> Cell {
    Cell::new(text, Tone::Plain)
}

fn row(key: &str, cells: &[&str]) -> Row {
    Row { key: key.into(), cells: cells.iter().map(|c| cell(c)).collect(), ..Row::default() }
}

fn under(key: &str, parent: &str, cells: &[&str]) -> Row {
    Row { parent: Some(parent.into()), ..row(key, cells) }
}

fn probe(key: &str, label: &str) -> Act {
    Act { key: key.into(), label: label.into(), intent: Intent::Run { command: label.into(), args: vec![] }, risk: 0 }
}

/// The rows of a binding as the core sends them: a window of `total` from `from`.
fn send(a: &mut App, bind: &str, rows: Vec<Row>, total: u64, from: u64) -> Vec<Up> {
    a.down(Down::Rows { bind: bind.into(), rows, total, from, rev: 1 })
}

fn hit(a: &mut App, code: KeyCode) -> Vec<Up> {
    a.key(press(code))
}

fn typing(a: &mut App, text: &str) -> Vec<Up> {
    typed(text, false).into_iter().flat_map(|k| a.key(k)).collect()
}

fn windows(ups: &[Up]) -> Vec<(u64, u64, String, String)> {
    ups.iter().filter_map(|u| if let Up::Window { from, count, sort, find, .. } = u { Some((*from, *count, sort.clone(), find.clone())) } else { None }).collect()
}

fn lines(a: &mut App, cols: u16, rows: u16) -> Vec<String> {
    screen(a, cols, rows).into_iter().map(|l| l.trim_end().to_string()).collect()
}

// ---- what the fields share ----

#[test]
fn a_count_has_a_comma_between_the_thousands() {
    let seen: Vec<String> = [0, 7, 999, 1000, 1204, 12345, 1000000, 1234567890].iter().map(|n| group(*n)).collect();
    assert_eq!(seen, ["0", "7", "999", "1,000", "1,204", "12,345", "1,000,000", "1,234,567,890"]);
}

#[test]
fn which_kinds_are_shown_and_which_are_asked_for_in_windows_follows_the_kind() {
    let shown = [FieldKind::Stat, FieldKind::Meter, FieldKind::Spark, FieldKind::Chip, FieldKind::Action, FieldKind::Table, FieldKind::Tree, FieldKind::Card, FieldKind::Cards];
    assert!(shown.iter().all(|k| display::shows(*k) && !k.is_value()));
    assert!(![FieldKind::Log, FieldKind::List, FieldKind::Bar, FieldKind::Str, FieldKind::Chat].iter().any(|k| display::shows(*k)));
    assert!([FieldKind::Table, FieldKind::Log].iter().all(|k| display::windowed(*k)));
    assert!(![FieldKind::Tree, FieldKind::Cards, FieldKind::List].iter().any(|k| display::windowed(*k)));
    assert!([FieldKind::Table, FieldKind::Tree, FieldKind::Card, FieldKind::Cards, FieldKind::Log, FieldKind::Action].iter().all(|k| display::keyed(*k)));
    assert!(![FieldKind::Stat, FieldKind::Meter, FieldKind::List, FieldKind::Chat].iter().any(|k| display::keyed(*k)));
}

// ---- stat ----

#[test]
fn a_stat_has_its_number_over_its_label_over_what_changed() {
    let mut a = one(FieldKind::Stat, "", Json::Null, json!([{"v": "$1.24", "label": "today", "delta": 12}, {"v": 38, "label": "tasks", "delta": -3}, {"v": "4.1s", "label": "p50", "delta": 0}]));
    let s = lines(&mut a, 60, 6);
    assert_eq!(s[0], "  $1.24    38       4.1s");
    assert_eq!(s[1], "  today    tasks    p50");
    assert_eq!(s[2], "  ▲ 12%    ▼ 3%     ─");
    assert_eq!(s[3], "", "three rows and no more");
}

#[test]
fn a_stat_with_no_change_is_two_rows_and_one_that_does_not_fit_is_one_row_of_numbers_with_the_mark_of_each_change() {
    let mut a = one(FieldKind::Stat, "", Json::Null, json!([{"v": "$1.24", "label": "today"}, {"v": "38", "label": "tasks"}]));
    let s = lines(&mut a, 60, 6);
    assert_eq!((s[0].as_str(), s[1].as_str(), s[2].as_str()), ("  $1.24    38", "  today    tasks", ""));
    let mut b = one(FieldKind::Stat, "", Json::Null, json!([{"v": "$1.24", "label": "spent today", "delta": 12}, {"v": "38", "label": "tasks run", "delta": -3}, {"v": "4.1s", "label": "median wall", "delta": 0}]));
    let n = lines(&mut b, 30, 6);
    assert_eq!((n[0].as_str(), n[1].as_str()), ("  $1.24 ▲  38 ▼  4.1s ─", ""));
    assert_eq!(lines(&mut b, 41, 6)[1], "  spent today    tasks run    median wall", "and in columns when it does");
}

#[test]
fn a_stat_says_its_change_in_the_unit_it_was_given_and_the_colour_of_its_sign_unless_told() {
    let mut a = one(FieldKind::Stat, "", Json::Null, json!({"v": "12", "label": "fails", "delta": 4, "unit": " runs", "tone": "bad"}));
    let s = lines(&mut a, 60, 6);
    assert_eq!(s[2], "  ▲ 4 runs");
    let tones = Headless::new(60, 6).tones(&mut a);
    assert_eq!(&tones[2][2..4], "rr", "the colour the core gave, not the one the sign would have");
    let mut b = one(FieldKind::Stat, "", Json::Null, json!({"v": "12", "label": "fails", "delta": 4}));
    assert_eq!(&Headless::new(60, 6).tones(&mut b)[2][2..4], "gg", "a rise is good unless said otherwise");
    let mut c = one(FieldKind::Stat, "", Json::Null, json!({"v": "12", "label": "fails", "delta": -4}));
    assert_eq!(&Headless::new(60, 6).tones(&mut c)[2][2..4], "rr");
}

#[test]
fn a_stat_with_nothing_to_show_is_blank() {
    let mut a = one(FieldKind::Stat, "", Json::Null, Json::Null);
    assert!(lines(&mut a, 40, 4).iter().all(String::is_empty));
}

// ---- meter ----

#[test]
fn a_meter_is_the_label_at_the_left_and_the_bar_with_its_percentage_at_the_right() {
    let mut a = one(FieldKind::Meter, "context", Json::Null, json!(0.45));
    let s = lines(&mut a, 64, 4);
    // the thin line, a heavy line over a light one in the track's own colour, no frame, at most eight cells: 45% of 8 cells is 7 halves
    assert_eq!(s[0], format!("├ context{}{}╸{} 45%", " ".repeat(42), "━".repeat(3), "─".repeat(4)));
    assert_eq!(s[1], "");
}

#[test]
fn a_bar_is_filled_in_halves_of_a_cell_and_never_says_more_than_there_is() {
    let bar = |f: f64| {
        let mut a = one(FieldKind::Meter, "m", Json::Null, json!(f));
        lines(&mut a, 64, 2)[0].clone()
    };
    let (heavy, light) = ("━", "─");
    assert!(bar(0.0).contains(&format!("{} 0%", light.repeat(8))), "{}", bar(0.0));
    assert!(bar(1.0).contains(&format!("{} 100%", heavy.repeat(8))), "{}", bar(1.0));
    // 8 cells: half a cell is 0.5/8, and less than that is none
    assert!(bar(0.5 / 8.0).contains(&format!("╸{}", light.repeat(7))), "{}", bar(0.5 / 8.0));
    assert!(bar(1.0 / 8.0).contains(&format!("━{}", light.repeat(7))), "{}", bar(1.0 / 8.0));
    assert!(bar(1.0 / 8.0 / 8.0).contains(&light.repeat(8)), "{}", bar(1.0 / 8.0 / 8.0));
}

#[test]
fn a_meter_is_warm_from_its_warn_line_and_red_over_all_of_it() {
    let tones = |f: f64, props: Json| {
        let mut a = one(FieldKind::Meter, "m", props, json!(f));
        let t = Headless::new(64, 2).tones(&mut a);
        (t[0].chars().nth(52).unwrap(), t[0].chars().nth(61).unwrap())
    };
    assert_eq!(tones(0.45, Json::Null), ('c', '.'), "the bar in the accent colour, the number plain");
    assert_eq!(tones(0.80, Json::Null), ('y', 'y'), "from the line, not above it");
    assert_eq!(tones(0.79, Json::Null).0, 'c');
    assert_eq!(tones(1.0, Json::Null).0, 'y', "all of it is not over it");
    assert_eq!(tones(1.04, Json::Null), ('r', 'r'));
    assert_eq!(tones(0.5, json!({"warn": 0.4})).0, 'y', "the line is the prop's");
}

#[test]
fn a_meter_over_all_of_it_shows_the_whole_bar_and_the_percentage() {
    let mut a = one(FieldKind::Meter, "limit 5h", Json::Null, json!(1.04));
    assert!(lines(&mut a, 64, 2)[0].ends_with(&format!("{} 104%", "━".repeat(8))));
}

#[test]
fn a_meter_under_sixty_columns_has_its_bar_on_the_row_under_its_label() {
    let mut a = one(FieldKind::Meter, "context", Json::Null, json!({"v": 0.45}));
    let s = lines(&mut a, 40, 4);
    assert_eq!(s[0], "├ context");
    assert_eq!(s[1], format!("│ {}╸{} 45%", "━".repeat(3), "─".repeat(4)));
}

#[test]
fn a_meter_with_no_value_says_so_and_an_ascii_bar_is_whole_cells() {
    let mut a = started(stack(vec![node("f", "context", FieldKind::Meter, "d:f", Json::Null)]));
    assert_eq!(lines(&mut a, 64, 2)[0], "├ context  —");
    let mut b = one(FieldKind::Meter, "context", Json::Null, json!(0.5));
    b.theme = j4n_tui::theme::Theme::build(&j4n_tui::theme::Settings { marks: "ascii".into(), ..Default::default() }, &[]).unwrap();
    let s = lines(&mut b, 64, 2);
    assert!(s[0].ends_with("====---- 50%"), "{}", s[0]);
}

#[test]
fn a_bar_that_does_not_fit_beside_a_long_label_goes_under_it() {
    let mut a = one(FieldKind::Meter, "a label so long that no bar can stand beside it at all", Json::Null, json!(0.5));
    let s = lines(&mut a, 64, 4);
    assert_eq!(s[0], "├ a label so long that no bar can stand beside it at all");
    assert!(s[1].starts_with("│ ━") && s[1].ends_with("─ 50%"), "{s:?}");
}

// ---- spark ----

#[test]
fn a_spark_is_the_series_in_eight_heights_and_the_last_value() {
    let mut a = one(FieldKind::Spark, "$ / day", json!({"window": 40}), json!({"points": [1, 2, 4, 8, 4, 2, 1], "last": "$1.24"}));
    let s = lines(&mut a, 64, 3);
    assert!(s[0].starts_with("├ $ / day") && s[0].ends_with("▁▂▄█▄▂▁   $1.24"), "{s:?}");
}

#[test]
fn a_spark_goes_from_the_lowest_to_the_highest_and_a_flat_series_is_the_lowest() {
    let chart = |pts: Json| {
        let mut a = one(FieldKind::Spark, "s", Json::Null, pts);
        let l = lines(&mut a, 64, 2)[0].clone();
        l.trim_start_matches("├ s").trim().chars().filter(|c| ('▁'..='█').contains(c)).collect::<String>()
    };
    assert_eq!(chart(json!([0, 1, 2, 3, 4, 5, 6, 7])), "▁▂▃▄▅▆▇█");
    assert_eq!(chart(json!([10, 20, 30, 40])), "▁▃▆█", "the lowest is the lowest of these, not zero");
    assert_eq!(chart(json!([5, 5, 5])), "▁▁▁");
    assert_eq!(chart(json!([3])), "▁");
}

#[test]
fn a_spark_shows_the_last_window_of_the_series_and_the_last_number_with_its_unit_when_the_core_gave_no_text() {
    let mut a = one(FieldKind::Spark, "s", json!({"window": 4, "unit": "s"}), json!([9, 1, 2, 3, 4, 5]));
    let l = lines(&mut a, 64, 2)[0].clone();
    assert!(l.ends_with("▁▃▆█      5s"), "{l}");
    let mut b = one(FieldKind::Spark, "s", json!({"window": 100}), json!([1, 2, 3]));
    assert!(lines(&mut b, 64, 2)[0].ends_with("▁▅█       3"));
}

#[test]
fn the_last_values_of_sparks_end_in_one_column() {
    let mut a = started(stack(vec![node("a", "one", FieldKind::Spark, "s:a", Json::Null), node("b", "two", FieldKind::Spark, "s:b", Json::Null)]));
    a.down(Down::Value { bind: "s:a".into(), value: json!({"points": [1, 2], "last": "$1.24"}), rev: 1 });
    a.down(Down::Value { bind: "s:b".into(), value: json!({"points": [1, 2], "last": "41s"}), rev: 1 });
    let s = lines(&mut a, 64, 4);
    assert_eq!(s[0].chars().count(), s[1].chars().count(), "{s:?}");
}

#[test]
fn a_spark_under_sixty_columns_has_its_chart_under_its_label_and_a_tiny_chart_does_too() {
    let mut a = one(FieldKind::Spark, "$ / day", Json::Null, json!({"points": [1, 4, 2], "last": "$1"}));
    let s = lines(&mut a, 40, 3);
    assert_eq!((s[0].as_str(), s[1].as_str()), ("├ $ / day", "│ ▁█▃      $1"));
    let mut b = one(FieldKind::Spark, "a rather long label for it", Json::Null, json!({"points": [1, 4, 2], "last": "$1"}));
    let t = lines(&mut b, 34, 3);
    assert!(t[1].starts_with("│ "), "no room beside the label: {t:?}");
}

// ---- chip ----

#[test]
fn a_chip_is_a_mark_and_a_word_in_the_colour_of_what_it_says() {
    let mut a = one(FieldKind::Chip, "", Json::Null, json!([{"icon": "done", "text": "ready", "tone": "good"}, {"icon": "failed", "text": "failed", "tone": "bad"}, {"text": "plain"}, {"icon": "no-such-slot", "text": "x", "tone": "nonsense"}]));
    let s = lines(&mut a, 60, 2);
    assert_eq!(s[0], "  \u{F05E1} ready  \u{F015A} failed  plain  x");
    let t = Headless::new(60, 2).tones(&mut a);
    assert_eq!(&t[0][4..9], "ggggg", "{t:?}");
}

#[test]
fn one_chip_given_as_an_object_is_shown_and_text_with_no_mark_is_the_word_alone() {
    let mut a = one(FieldKind::Chip, "", Json::Null, json!({"icon": "ask", "text": 3, "tone": "warn"}));
    assert_eq!(lines(&mut a, 40, 2)[0], "  \u{F0625} 3");
}

// ---- action ----

fn buttons() -> App {
    let a = |id: &str, label: &str, props: Json| node(id, label, FieldKind::Action, &format!("act:{id}"), props);
    started(stack(vec![
        a("probe", "probe", json!({"intent": {"intent": "run", "command": "probe", "args": ["omp"]}})),
        a("off", "locked", json!({"intent": {"intent": "run", "command": "x"}, "disabled": "read only"})),
        a("widen", "widen", json!({"intent": {"intent": "run", "command": "widen"}, "risk": 70})),
        a("pin", "pin", json!({"intent": {"intent": "run", "command": "pin"}, "undo": {"intent": "run", "command": "unpin"}})),
    ]))
}

fn ran(ups: &[Up]) -> Vec<String> {
    ups.iter().filter_map(|u| if let Up::Intent { intent: Intent::Run { command, .. } } = u { Some(command.clone()) } else { None }).collect()
}

#[test]
fn a_button_is_a_key_and_a_word_and_a_risky_one_has_a_mark_of_its_own() {
    let mut a = buttons();
    a.down(Down::Value { bind: "act:pin".into(), value: json!("done"), rev: 1 });
    let s = lines(&mut a, 40, 6);
    assert_eq!(s[0], "  ⏎ probe", "the first has the keys: it is a box with a blank each side, so its text starts where the others do");
    assert_eq!(s[1], "  ⏎ locked \u{F0341}");
    assert_eq!(s[2], "  \u{F0CC8} widen");
    assert_eq!(s[3], "  ⏎ pin  \u{F05E1} done  u undo");
}

#[test]
fn a_press_sends_the_intent_and_a_button_that_may_not_be_pressed_is_skipped() {
    let mut a = buttons();
    assert_eq!(ran(&hit(&mut a, KeyCode::Enter)), ["probe"]);
    hit(&mut a, KeyCode::Down);
    let seen = a.page().map(|p| p.id.clone());
    assert_eq!(seen.as_deref(), Some("widen"), "the disabled one is stepped over");
    assert_eq!(ran(&a.key(press(KeyCode::Char(' ')))), Vec::<String>::new(), "space is the first press of a risky one");
}

#[test]
fn a_risky_button_wants_two_presses_and_any_other_key_gives_up_the_first() {
    let mut a = buttons();
    hit(&mut a, KeyCode::Down);
    assert_eq!(ran(&hit(&mut a, KeyCode::Enter)), Vec::<String>::new());
    assert_eq!(a.armed.as_deref(), Some("act:widen"));
    assert!(lines(&mut a, 40, 6)[2].contains("again ⏎"), "{:?}", lines(&mut a, 40, 6));
    assert_eq!(ran(&hit(&mut a, KeyCode::Enter)), ["widen"]);
    assert_eq!(a.armed, None);
    hit(&mut a, KeyCode::Enter);
    hit(&mut a, KeyCode::Esc);
    assert_eq!(a.armed, None, "esc gives it up");
    assert_eq!(ran(&hit(&mut a, KeyCode::Enter)), Vec::<String>::new(), "and then it is a first press again");
    hit(&mut a, KeyCode::Down);
    assert_eq!(a.armed, None, "moving to another button gives it up too");
    hit(&mut a, KeyCode::Up);
    assert_eq!(ran(&hit(&mut a, KeyCode::Enter)), Vec::<String>::new());
}

#[test]
fn what_makes_a_button_need_two_presses_is_its_risk_or_its_word() {
    let press_once = |props: Json| {
        let mut a = started(stack(vec![node("b", "go", FieldKind::Action, "act:b", props)]));
        ran(&hit(&mut a, KeyCode::Enter))
    };
    let go = json!({"command": "go", "intent": "run"});
    assert_eq!(press_once(json!({"intent": go, "risk": 49})), ["go"]);
    assert_eq!(press_once(json!({"intent": go, "risk": 50})), Vec::<String>::new());
    assert_eq!(press_once(json!({"intent": go, "confirm": "always"})), Vec::<String>::new());
    assert_eq!(press_once(json!({"intent": go, "risk": 90, "confirm": "never"})), ["go"]);
    assert_eq!(press_once(json!({"intent": go, "risk": 90, "confirm": "auto"})), Vec::<String>::new());
    assert_eq!(press_once(json!({"risk": 0})), Vec::<String>::new(), "no intent, nothing to send");
}

#[test]
fn an_applied_button_offers_its_undo_and_u_sends_it() {
    let mut a = buttons();
    hit(&mut a, KeyCode::Down);
    hit(&mut a, KeyCode::Down);
    assert_eq!(ran(&typing(&mut a, "u")), Vec::<String>::new(), "nothing to undo before it is applied");
    a.down(Down::Value { bind: "act:pin".into(), value: json!({"state": "done"}), rev: 2 });
    assert_eq!(ran(&typing(&mut a, "u")), ["unpin"]);
    assert!(lines(&mut a, 40, 6)[3].ends_with("done  u undo"));
    let mut b = started(stack(vec![node("b", "pin", FieldKind::Action, "act:b", json!({"intent": {"intent": "run", "command": "pin"}}))]));
    b.down(Down::Value { bind: "act:b".into(), value: json!("done"), rev: 1 });
    assert!(lines(&mut b, 40, 2)[0].ends_with("done"), "no undo when the core gave none");
    assert!(!lines(&mut b, 40, 2)[0].contains("undo"));
}

#[test]
fn a_button_is_in_a_form_with_the_fields_and_the_arrows_go_to_it_and_from_it() {
    let mut a = started(stack(vec![
        node("n", "name", FieldKind::Str, "cfg:x.n", Json::Null),
        node("go", "save", FieldKind::Action, "act:go", json!({"intent": {"intent": "run", "command": "save"}})),
    ]));
    hit(&mut a, KeyCode::Down);
    assert_eq!(a.page().map(|p| p.id.as_str()), Some("go"));
    assert_eq!(ran(&hit(&mut a, KeyCode::Enter)), ["save"]);
    hit(&mut a, KeyCode::Up);
    assert_eq!(a.page().map(|p| p.id.as_str()), Some("n"));
}

// ---- bar ----

fn bar_app(props: Json, cells: &[&str]) -> App {
    let mut a = started(stack(vec![node("bar", "", FieldKind::Bar, "bar:status", props)]));
    send(&mut a, "bar:status", vec![row("status", cells)], 1, 0);
    a
}

#[test]
fn a_bar_is_chips_at_the_left_and_the_last_ones_at_the_right() {
    let mut a = bar_app(json!({"right": 2}), &["claude", "$0.33", "B", "F1"]);
    let s = lines(&mut a, 40, 2);
    assert_eq!(s[0], format!(" claude  $0.33{}B  F1", " ".repeat(20)), "the right ones end one cell short of the edge");
}

#[test]
fn what_does_not_fit_goes_last_first_and_the_first_never() {
    let mut a = bar_app(json!({"right": 2}), &["claude", "$0.33", "B", "F1"]);
    assert_eq!(lines(&mut a, 21, 2)[0], " claude  $0.33 B  F1", "all of it fits in 21");
    assert_eq!(lines(&mut a, 20, 2)[0], format!(" claude  $0.33{}B", " ".repeat(4)), "the last goes first");
    assert_eq!(lines(&mut a, 16, 2)[0], " claude  $0.33");
    assert_eq!(lines(&mut a, 10, 2)[0], " claude");
    assert_eq!(lines(&mut a, 3, 2)[0], " cl", "the first is what is left, cut");
}

#[test]
fn a_bar_names_the_order_things_go_in() {
    let mut a = bar_app(json!({"right": 2, "drop": [1, 3]}), &["claude", "$0.33", "B", "F1"]);
    assert_eq!(lines(&mut a, 21, 2)[0], " claude  $0.33 B  F1");
    assert_eq!(lines(&mut a, 20, 2)[0], format!(" claude{}B  F1", " ".repeat(7)), "the one named first goes first");
    assert_eq!(lines(&mut a, 13, 2)[0], format!(" claude{}B", " ".repeat(4)), "then the next named");
}

#[test]
fn an_empty_cell_of_a_bar_is_not_a_chip_and_a_bar_with_no_row_is_blank() {
    let mut a = bar_app(Json::Null, &["alpha", "", "$1"]);
    assert_eq!(lines(&mut a, 30, 2)[0], " alpha  $1");
    let mut b = started(stack(vec![node("bar", "", FieldKind::Bar, "bar:status", Json::Null)]));
    assert!(lines(&mut b, 30, 2).iter().all(String::is_empty));
}

// ---- windows of rows ----

fn models(n: usize) -> Vec<Row> {
    (0..n).map(|i| row(&format!("m{i}"), &[&format!("model {i}"), &format!("{}", i * 10)])).collect()
}

fn table_app(total: u64, held: usize) -> (App, Vec<Up>) {
    let cols = json!({"columns": [{"name": "model", "title": "model"}, {"name": "uses", "title": "uses", "right": true}]});
    let mut a = App::new();
    a.start();
    a.down(hello());
    a.down(Down::Tree { node: stack(vec![node("t", "", FieldKind::Table, "view:m", cols)]) });
    let ups = send(&mut a, "view:m", models(held), total, 0);
    (a, ups)
}

#[test]
fn a_table_has_a_header_the_rows_a_thumb_and_a_footer_that_counts() {
    let (mut a, _) = table_app(40, 40);
    let s = lines(&mut a, 40, 8);
    assert_eq!(s[0], "  model    uses");
    assert!(s[1].starts_with("┃ model 0     0") && s[1].ends_with('┃') && s[1].chars().count() == 40, "{s:?}");
    assert_eq!(s[2], "  model 1    10");
    assert_eq!(s[6], "  model 5    50");
    assert_eq!(s[7], " 6 of 40 · s sort · / find");
}

#[test]
fn the_thumb_of_a_table_is_as_long_as_the_screen_is_of_the_rows_and_sits_where_they_are() {
    let (mut a, _) = table_app(60, 60);
    let thumb = |a: &mut App| screen(a, 30, 12).iter().map(|l| l.chars().last()).map(|c| c == Some('┃')).collect::<Vec<_>>();
    let start = thumb(&mut a);
    assert_eq!(start.iter().filter(|t| **t).count(), 1, "{start:?}");
    assert!(start[1], "at the top first: {start:?}");
    for _ in 0..59 {
        hit(&mut a, KeyCode::Down);
    }
    let end = thumb(&mut a);
    assert!(end[10], "and at the bottom at the end (the rows, then the footer): {end:?}");
    let (mut few, _) = table_app(3, 3);
    assert!(!screen(&mut few, 30, 12).iter().any(|l| l.ends_with('┃') && !l.starts_with('┃')), "no thumb when all the rows are in sight");
}

#[test]
fn rows_the_screen_does_not_hold_yet_are_dots_and_the_footer_counts_the_ones_it_shows() {
    let (mut a, _) = table_app(1204, 3);
    let s = lines(&mut a, 40, 8);
    assert_eq!(s[3], "  model 2    20");
    assert_eq!(s[4], "  …");
    assert_eq!(s[7], " 3 of 1,204 · s sort · / find");
}

#[test]
fn a_table_drops_the_columns_that_do_not_fit_from_the_right_and_aligns_a_right_column_at_its_edge() {
    let (mut a, _) = table_app(5, 5);
    assert!(lines(&mut a, 40, 6)[1].contains("model 0     0"));
    let s = lines(&mut a, 14, 6);
    assert_eq!(s[0], "  model", "{s:?}");
    assert!(s[2].starts_with("  model 1") && !s[2].contains("10"), "{s:?}");
}

#[test]
fn a_table_names_its_sort_on_the_header_with_the_way_it_goes() {
    let cols = json!({"columns": ["model", {"name": "uses", "right": true}], "sort": "-uses"});
    let mut a = started(stack(vec![node("t", "", FieldKind::Table, "view:m", cols)]));
    send(&mut a, "view:m", models(3), 3, 0);
    assert_eq!(lines(&mut a, 40, 6)[0], "  model    uses ⇓");
    hit(&mut a, KeyCode::Char('S'));
    assert_eq!(lines(&mut a, 40, 6)[0], "  model    uses ⇑");
}

#[test]
fn s_sorts_by_the_next_column_and_after_the_last_by_none_and_the_core_is_asked() {
    let (mut a, _) = table_app(1204, 40);
    let first = windows(&hit(&mut a, KeyCode::Char('s')));
    assert_eq!(first.len(), 1, "{first:?}");
    assert_eq!((first[0].0, first[0].2.as_str(), first[0].3.as_str()), (0, "model", ""));
    assert_eq!(windows(&hit(&mut a, KeyCode::Char('s')))[0].2, "uses");
    assert_eq!(windows(&hit(&mut a, KeyCode::Char('s')))[0].2, "", "after the last column the core's own order");
    assert_eq!(windows(&hit(&mut a, KeyCode::Char('s')))[0].2, "model", "and round again");
    // the other way
    hit(&mut a, KeyCode::Char('S'));
    assert_eq!(a.wins["view:m"].sort, "-model");
    let up = windows(&hit(&mut a, KeyCode::Char('s')));
    assert_eq!(up[0].2, "-uses", "the way stays when the column changes");
    hit(&mut a, KeyCode::Char('s'));
    assert_eq!(a.wins["view:m"].sort, "", "and none is none either way");
    assert!(windows(&hit(&mut a, KeyCode::Char('S'))).is_empty(), "nothing to turn round");
}

#[test]
fn a_new_sort_starts_from_the_first_row() {
    let (mut a, _) = table_app(1204, 40);
    for _ in 0..30 {
        hit(&mut a, KeyCode::Down);
    }
    assert_eq!(a.wins["view:m"].cur.at, 30);
    hit(&mut a, KeyCode::Char('s'));
    let w = &a.wins["view:m"];
    assert_eq!((w.cur.at, w.cur.top), (0, 0));
}

#[test]
fn a_find_is_typed_after_a_slash_sent_with_enter_and_taken_off_with_esc() {
    let (mut a, _) = table_app(1204, 40);
    assert!(windows(&hit(&mut a, KeyCode::Char('/'))).is_empty());
    assert!(lines(&mut a, 40, 8)[7].starts_with(" / ▏"), "{:?}", lines(&mut a, 40, 8));
    assert!(windows(&typing(&mut a, "ha")).is_empty(), "nothing is sent while it is typed");
    assert!(lines(&mut a, 40, 8)[7].starts_with(" / ha▏"));
    hit(&mut a, KeyCode::Backspace);
    typing(&mut a, "ik");
    let sent = windows(&hit(&mut a, KeyCode::Enter));
    assert_eq!((sent[0].0, sent[0].3.as_str()), (0, "hik"));
    assert_eq!(lines(&mut a, 60, 8)[7], " 6 of 1,204 · / hik · esc clear");
    let cleared = windows(&hit(&mut a, KeyCode::Esc));
    assert_eq!(cleared[0].3, "");
    // esc while typing gives up the typing and keeps what was found
    hit(&mut a, KeyCode::Char('/'));
    typing(&mut a, "x");
    assert!(windows(&hit(&mut a, KeyCode::Esc)).is_empty());
    assert!(a.wins["view:m"].typing.is_none() && a.wins["view:m"].find.is_empty());
    // a find that is typed again starts from the one there is
    hit(&mut a, KeyCode::Char('/'));
    typing(&mut a, "ab");
    hit(&mut a, KeyCode::Enter);
    hit(&mut a, KeyCode::Char('/'));
    assert_eq!(a.wins["view:m"].typing.as_ref().map(|t| t.buf.as_str()), Some("ab"));
    // control keys are not text
    let q = hit(&mut a, KeyCode::Char('x'));
    assert!(q.is_empty());
    assert_eq!(a.wins["view:m"].typing.as_ref().map(|t| t.buf.as_str()), Some("abx"));
    assert!(a.key(ctrl('x')).is_empty());
    assert_eq!(a.wins["view:m"].typing.as_ref().map(|t| t.buf.as_str()), Some("abx"));
}

#[test]
fn the_rows_around_the_row_the_keys_are_on_are_asked_for_once_when_the_screen_does_not_hold_them() {
    let (mut a, first) = table_app(1_000, 3);
    let first = windows(&first);
    assert_eq!(first.len(), 1, "three rows of a thousand are not a screen's worth");
    assert_eq!((first[0].0, first[0].1), (0, 61), "three screens around the row, from the top");
    let mut asked = Vec::new();
    for _ in 0..40 {
        asked.extend(windows(&hit(&mut a, KeyCode::Down)));
    }
    assert!(asked.is_empty(), "the window that was asked for covers the rows wanted until the row is 40 down: {asked:?}");
    asked.extend(windows(&hit(&mut a, KeyCode::Down)));
    assert_eq!(asked.len(), 1, "and once it does not, it is asked for again, once: {asked:?}");
    assert!(asked[0].0 > 0 && asked[0].0 + asked[0].1 > 61, "{asked:?}");
    // what the core then sends covers it, and nothing more is asked
    let ups = send(&mut a, "view:m", models(100), 1_000, asked[0].0);
    assert!(windows(&ups).is_empty());
    assert!(windows(&hit(&mut a, KeyCode::Down)).is_empty());
}

#[test]
fn a_jump_to_the_end_asks_for_the_end_and_a_core_that_sent_all_the_rows_is_never_asked() {
    let (mut a, _) = table_app(10_000, 100);
    let ups = windows(&hit(&mut a, KeyCode::End));
    assert_eq!(ups.len(), 1);
    assert!(ups[0].0 + ups[0].1 > 9_999 && ups[0].0 > 9_000, "{ups:?}");
    assert_eq!(a.wins["view:m"].cur.at, 9_999);
    let (mut whole, _) = table_app(50, 50);
    let mut asked = Vec::new();
    for _ in 0..50 {
        asked.extend(windows(&hit(&mut whole, KeyCode::Down)));
    }
    asked.extend(windows(&hit(&mut whole, KeyCode::End)));
    assert!(asked.is_empty(), "{asked:?}");
}

#[test]
fn the_rows_of_a_window_that_starts_further_down_are_the_ones_at_their_places() {
    let (mut a, _) = table_app(1_000, 3);
    hit(&mut a, KeyCode::End);
    send(&mut a, "view:m", models(60).into_iter().enumerate().map(|(i, mut r)| { r.cells[0].text = format!("row {}", 940 + i); r }).collect(), 1_000, 940);
    let s = lines(&mut a, 40, 8);
    assert!(s.iter().any(|l| l.starts_with("┃ row 999")), "{s:?}");
    assert!(s.iter().any(|l| l.starts_with("  row 994")), "{s:?}");
    assert_eq!(s[7], " 6 of 1,000 · s sort · / find");
}

#[test]
fn the_keys_move_the_row_by_one_by_a_page_and_to_the_ends_and_stop_there() {
    let (mut a, _) = table_app(100, 100);
    let at = |a: &App| a.wins["view:m"].cur.at;
    hit(&mut a, KeyCode::Down);
    hit(&mut a, KeyCode::Down);
    assert_eq!(at(&a), 2);
    hit(&mut a, KeyCode::Up);
    assert_eq!(at(&a), 1);
    hit(&mut a, KeyCode::PageDown);
    assert_eq!(at(&a), 1 + a.screenful());
    hit(&mut a, KeyCode::PageUp);
    assert_eq!(at(&a), 1);
    hit(&mut a, KeyCode::Char('G'));
    assert_eq!(at(&a), 99);
    hit(&mut a, KeyCode::Down);
    assert_eq!(at(&a), 99, "no further than the last");
    hit(&mut a, KeyCode::Char('g'));
    assert_eq!(at(&a), 0);
    hit(&mut a, KeyCode::Up);
    assert_eq!(at(&a), 0);
    hit(&mut a, KeyCode::Down);
    hit(&mut a, KeyCode::Home);
    assert_eq!(at(&a), 0);
    hit(&mut a, KeyCode::End);
    assert_eq!(at(&a), 99);
    assert_eq!(a.screenful(), 20, "a screen's worth is the rows of the screen less four");
}

#[test]
fn the_selected_row_is_kept_in_sight_and_the_keys_of_its_actions_run_them() {
    let mut rows = models(30);
    rows[7].actions = vec![probe("p", "probe")];
    let cols = json!({"columns": ["model", "uses"]});
    let mut a = started(stack(vec![node("t", "", FieldKind::Table, "view:m", cols)]));
    send(&mut a, "view:m", rows, 30, 0);
    for _ in 0..7 {
        hit(&mut a, KeyCode::Down);
    }
    let s = lines(&mut a, 40, 6);
    assert!(s.iter().any(|l| l.starts_with("┃ model 7")), "{s:?}");
    assert!(a.wins["view:m"].cur.top >= 4, "{:?}", a.wins["view:m"]);
    assert_eq!(ran(&typing(&mut a, "p")), ["probe"]);
    assert_eq!(ran(&hit(&mut a, KeyCode::Enter)), ["probe"]);
    assert!(typing(&mut a, "z").is_empty(), "a key no action has does nothing");
    hit(&mut a, KeyCode::Down);
    assert!(hit(&mut a, KeyCode::Enter).is_empty(), "and a row with no action does nothing on enter");
}

// ---- list ----

fn list_app() -> App {
    let mut a = started(stack(vec![node("l", "", FieldKind::List, "view:agents", Json::Null)]));
    let mut omp = under("omp", "ready", &["omp", "opus"]);
    omp.actions = vec![probe("p", "probe")];
    omp.detail = vec![Cell::new("last probe 4m ago", Tone::Dim)];
    send(&mut a, "view:agents", vec![row("ready", &["ready", "2"]), under("claude", "ready", &["claude", "sonnet"]), omp, row("off", &["off"]), under("clef", "off", &["clef", "no key"])], 5, 0);
    a
}

#[test]
fn a_list_has_sections_with_their_counts_and_rows_on_pipes() {
    let mut a = list_app();
    let s = lines(&mut a, 40, 8);
    assert_eq!(s[0], "▾ ready  2");
    assert_eq!(s[1], "├ claude  sonnet");
    assert_eq!(s[2], "╰ omp     opus");
    assert_eq!(s[3], "▾ off  1", "a count the core did not give is the rows under it");
    assert_eq!(s[4], "╰ clef    no key");
}

#[test]
fn the_selected_row_of_a_list_has_a_bar_its_hint_and_its_detail() {
    let mut a = list_app();
    hit(&mut a, KeyCode::Down);
    hit(&mut a, KeyCode::Down);
    let s = lines(&mut a, 40, 8);
    assert!(s[2].starts_with("╰ omp     opus") && s[2].ends_with("⏎ probe"), "the elbow stays on the last row: {s:?}");
    assert!(banded(&mut a, 40, 8).iter().any(|l| l.starts_with("╰ omp")), "the band is what marks it");
    assert_eq!(s[3], "    last probe 4m ago", "under the last row the detail hangs from a blank");
    assert_eq!(s[4], "▾ off  1");
}

#[test]
fn enter_on_the_head_of_a_section_folds_it_and_the_arrows_step_over_what_is_hidden() {
    let mut a = list_app();
    hit(&mut a, KeyCode::Enter);
    let s = lines(&mut a, 40, 8);
    assert_eq!((s[0].as_str(), s[1].as_str()), ("▸ ready  2", "▾ off  1"), "{s:?}");
    hit(&mut a, KeyCode::Down);
    assert_eq!(a.place("view:agents"), 3, "from the shut section to the next");
    hit(&mut a, KeyCode::Up);
    assert_eq!(a.place("view:agents"), 0);
    hit(&mut a, KeyCode::Char(' '));
    assert_eq!(lines(&mut a, 40, 8)[1], "├ claude  sonnet", "space unfolds it again");
    // a row that is not a head does what ⏎ does for it
    hit(&mut a, KeyCode::Down);
    hit(&mut a, KeyCode::Down);
    assert_eq!(ran(&hit(&mut a, KeyCode::Enter)), ["probe"]);
}

#[test]
fn a_list_lines_its_cells_up_under_one_another_and_cuts_a_long_one() {
    let mut a = started(stack(vec![node("l", "", FieldKind::List, "view:x", Json::Null)]));
    send(&mut a, "view:x", vec![row("a", &["a", "running", "x"]), row("b", &["bbbb", "ok", "y"]), row("c", &["c".repeat(70).as_str(), "-", "z"])], 3, 0);
    let s = lines(&mut a, 140, 6);
    assert!(s[1].starts_with("├ bbbb") && s[1].contains("  ok       y"), "{:?}", s[1]);
    assert_eq!(s[1].find("ok"), s[0].find("running"), "{s:?}");
    assert!(s[2].starts_with(&format!("╰ {}  -", "c".repeat(70))), "the last column needs little, so the first keeps all it holds: {:?}", s[2]);
    let s = lines(&mut a, 60, 6);
    assert!(s[2].starts_with(&format!("╰ {}…  -", "c".repeat(45))) && s[2].chars().count() <= 60, "and is cut where the window ends: {:?}", s[2]);
}

// ---- tree ----

fn tree_app() -> App {
    let mut a = started(stack(vec![node("t", "", FieldKind::Tree, "tasks:open", Json::Null)]));
    let mut commit = under("commit", "work", &["commit?", "you"]);
    commit.actions = vec![probe("y", "approve")];
    send(
        &mut a,
        "tasks:open",
        vec![
            row("fix", &["fix trailing commas", "3:12"]),
            under("plan", "fix", &["plan", "omp", "21s"]),
            under("work", "fix", &["work", "claude", "1:04"]),
            commit,
            under("check", "fix", &["check", "pi"]),
            under("lint", "check", &["lint"]),
        ],
        6,
        0,
    );
    a
}

#[test]
fn a_tree_has_pipes_that_follow_who_is_last_and_columns_that_line_up() {
    let mut a = tree_app();
    let s = lines(&mut a, 40, 8);
    assert_eq!(s[0], "┃ fix trailing commas  3:12");
    assert_eq!(s[1], "  ├─ plan              omp     21s");
    assert_eq!(s[2], "  ├─ work              claude  1:04");
    assert_eq!(s[3], "  │ ╰─ commit?         you");
    assert_eq!(s[4], "  ╰─ check             pi");
    assert_eq!(s[5], "    ╰─ lint");
    let col = |l: &str, w: &str| l.find(w).map(|b| l[..b].chars().count());
    assert_eq!((col(&s[1], "omp"), col(&s[2], "claude"), col(&s[3], "you"), col(&s[4], "pi")), (Some(23), Some(23), Some(23), Some(23)), "the cells after the first start in one column");
}

#[test]
fn the_arrows_fold_and_unfold_a_tree_and_left_on_a_leaf_goes_to_its_parent() {
    let mut a = tree_app();
    hit(&mut a, KeyCode::Down);
    hit(&mut a, KeyCode::Down);
    hit(&mut a, KeyCode::Left);
    let s = lines(&mut a, 40, 8);
    assert_eq!(s[2], "┃ ├─ work              claude  1:04   ▸", "{s:?}");
    assert_eq!(s[2].chars().position(|c| c == '▸'), Some(38), "at the right edge, one cell in");
    assert_eq!(s[3], "  ╰─ check             pi");
    hit(&mut a, KeyCode::Left);
    assert_eq!(a.wins["tasks:open"].cur.at, 0, "left on a row already shut goes to its parent");
    hit(&mut a, KeyCode::Down);
    hit(&mut a, KeyCode::Down);
    hit(&mut a, KeyCode::Right);
    assert!(lines(&mut a, 40, 8)[3].contains("commit?"), "right unfolds");
    hit(&mut a, KeyCode::Enter);
    assert!(!lines(&mut a, 40, 8)[3].contains("commit?"), "enter folds a row that has rows under it");
    hit(&mut a, KeyCode::Char(' '));
    assert!(lines(&mut a, 40, 8)[3].contains("commit?"), "and space unfolds it");
}

#[test]
fn a_row_under_a_row_that_is_shut_cannot_be_stepped_on_and_a_leaf_runs_its_action() {
    let mut a = tree_app();
    hit(&mut a, KeyCode::Down);
    hit(&mut a, KeyCode::Down);
    hit(&mut a, KeyCode::Enter);
    hit(&mut a, KeyCode::Down);
    assert_eq!(a.wins["tasks:open"].cur.at, 4, "from work to check, over commit");
    hit(&mut a, KeyCode::Up);
    hit(&mut a, KeyCode::Enter);
    hit(&mut a, KeyCode::Down);
    assert_eq!(a.wins["tasks:open"].cur.at, 3);
    assert_eq!(ran(&typing(&mut a, "y")), ["approve"]);
    assert_eq!(ran(&hit(&mut a, KeyCode::Enter)), ["approve"]);
    hit(&mut a, KeyCode::End);
    assert_eq!(a.wins["tasks:open"].cur.at, 5);
    hit(&mut a, KeyCode::Home);
    assert_eq!(a.wins["tasks:open"].cur.at, 0);
}

#[test]
fn a_row_the_core_folds_away_under_the_cursor_leaves_the_cursor_on_the_row_that_holds_it_and_the_keys_still_move_it() {
    // the cursor on `commit`; the core then sends `work` shut (its step is done): the row the cursor is on is out of sight
    let mut a = tree_app();
    for _ in 0..3 {
        hit(&mut a, KeyCode::Down);
    }
    assert_eq!(a.wins["tasks:open"].cur.at, 3);
    let mut work = under("work", "fix", &["work", "claude", "1:04"]);
    work.fold = Some(j4n_proto::Fold::Folded);
    let mut commit = under("commit", "work", &["commit?", "you"]);
    commit.actions = vec![probe("y", "approve")];
    send(&mut a, "tasks:open", vec![row("fix", &["fix trailing commas", "3:12"]), under("plan", "fix", &["plan", "omp", "21s"]), work, commit, under("check", "fix", &["check", "pi"]), under("lint", "check", &["lint"])], 6, 0);
    let s = lines(&mut a, 40, 8);
    assert!(s[2].starts_with("┃ ├─ work") && !s.iter().any(|l| l.contains("commit?")), "the focus is on the row that holds it: {s:?}");
    hit(&mut a, KeyCode::Down);
    assert_eq!(a.wins["tasks:open"].cur.at, 4, "down goes on from there, to check");
    assert!(lines(&mut a, 40, 8)[3].starts_with("┃ ╰─ check"));
}

#[test]
fn a_row_whose_parent_is_not_among_the_rows_is_at_the_top() {
    let mut a = started(stack(vec![node("t", "", FieldKind::Tree, "t:x", Json::Null)]));
    send(&mut a, "t:x", vec![under("a", "ghost", &["a"]), under("b", "a", &["b"])], 2, 0);
    let s = lines(&mut a, 20, 3);
    assert_eq!((s[0].as_str(), s[1].as_str()), ("┃ a", "  ╰─ b"));
}

// ---- card and cards ----

#[test]
fn a_card_is_its_title_and_state_what_is_known_on_pipes_and_the_keys_of_what_can_be_done() {
    let mut head = row("claude", &["claude", "ready"]);
    head.cells[0].icon = "done".into();
    head.cells[1].tone = Tone::Good;
    head.actions = vec![probe("r", "probe"), probe("p", "pin")];
    let mut a = started(stack(vec![node("c", "claude", FieldKind::Card, "agent:claude", Json::Null)]));
    send(&mut a, "agent:claude", vec![head, row("dialect", &["dialect", "claude-stream"]), row("can", &["can", "live", "resume"])], 3, 0);
    let s = lines(&mut a, 50, 6);
    assert_eq!(s[0], "  \u{F05E1} claude  ready");
    assert_eq!(s[1], "├ dialect  claude-stream");
    assert_eq!(s[2], "╰ can      live  resume");
    assert_eq!(s[3], "  ⏎ probe   p pin", "the first is in a box with a blank each side, so its word starts where the others do");
    assert_eq!(ran(&hit(&mut a, KeyCode::Enter)), ["probe"]);
    assert_eq!(ran(&typing(&mut a, "p")), ["pin"]);
    assert!(hit(&mut a, KeyCode::Right).is_empty());
}

#[test]
fn a_card_with_no_rows_is_its_name_and_the_arrows_still_go_to_the_next_field() {
    let mut a = started(stack(vec![node("c", "claude", FieldKind::Card, "agent:claude", Json::Null), node("n", "name", FieldKind::Str, "cfg:x.n", Json::Null)]));
    assert_eq!(lines(&mut a, 30, 3)[0], "  claude");
}

fn cards_app() -> App {
    let mut a = started(stack(vec![node("c", "", FieldKind::Cards, "view:c", json!({"width": 14}))]));
    let mut first = row("claude", &["claude", "$0.19"]);
    first.actions = vec![probe("p", "probe")];
    send(&mut a, "view:c", vec![first, row("omp", &["omp", "$0.03"]), row("gemini", &["gemini", "0:40"]), row("pi", &["pi", "$0.00"])], 4, 0);
    a
}

#[test]
fn cards_are_in_a_grid_that_fills_the_width_and_the_selected_one_has_a_heavy_frame() {
    let mut a = cards_app();
    let s = lines(&mut a, 32, 10);
    assert_eq!(s[0], "┏━━━━━━━━━━━━┓ ╭────────────╮");
    assert_eq!(s[1], "┃ claude     ┃ │ omp        │");
    assert_eq!(s[2], "┃ $0.19      ┃ │ $0.03      │");
    assert_eq!(s[3], "┗━━━━━━━━━━━━┛ ╰────────────╯");
    assert_eq!(s[4], "╭────────────╮ ╭────────────╮");
    assert_eq!(s[5], "│ gemini     │ │ pi         │");
}

#[test]
fn the_arrows_move_between_cards_by_one_and_by_a_row() {
    let mut a = cards_app();
    lines(&mut a, 32, 10);
    let at = |a: &App| a.wins["view:c"].cur.at;
    hit(&mut a, KeyCode::Right);
    assert_eq!(at(&a), 1);
    hit(&mut a, KeyCode::Down);
    assert_eq!(at(&a), 3);
    hit(&mut a, KeyCode::Left);
    assert_eq!(at(&a), 2);
    hit(&mut a, KeyCode::Up);
    assert_eq!(at(&a), 0);
    hit(&mut a, KeyCode::Left);
    assert_eq!(at(&a), 0);
    hit(&mut a, KeyCode::End);
    assert_eq!(at(&a), 3);
    hit(&mut a, KeyCode::Right);
    assert_eq!(at(&a), 3);
    hit(&mut a, KeyCode::Home);
    assert_eq!(at(&a), 0);
    assert_eq!(ran(&hit(&mut a, KeyCode::Enter)), ["probe"]);
}

#[test]
fn cards_under_the_width_of_two_are_a_column_and_a_grid_taller_than_the_screen_follows_the_card() {
    let mut a = cards_app();
    let s = lines(&mut a, 20, 5);
    assert_eq!(s[1], "┃ claude     ┃");
    for _ in 0..3 {
        hit(&mut a, KeyCode::Down);
    }
    let s = lines(&mut a, 20, 5);
    assert!(s.iter().any(|l| l.starts_with("┃ pi")), "{s:?}");
    assert!(!s.iter().any(|l| l.contains("claude")), "the first card scrolled off: {s:?}");
}

// ---- log ----

fn log_rows(n: usize) -> Vec<Row> {
    (0..n)
        .map(|i| {
            let mut r = row(&format!("e{i}"), &[&format!("12:{:02}:00", i % 60), &format!("event {i}")]);
            if i % 5 == 4 {
                r.detail = vec![Cell::new(format!("detail of {i}"), Tone::Dim)];
            }
            r
        })
        .collect()
}

fn log_app(total: u64, held: usize) -> (App, Vec<Up>) {
    let mut a = App::new();
    a.start();
    a.down(hello());
    a.down(Down::Tree { node: stack(vec![node("l", "", FieldKind::Log, "log:all", Json::Null)]) });
    let ups = send(&mut a, "log:all", log_rows(held), total, total - held as u64);
    (a, ups)
}

#[test]
fn a_log_follows_its_end_until_the_operator_moves_and_follows_again_on_the_last_row() {
    let (mut a, _) = log_app(1_000, 100);
    let s = lines(&mut a, 40, 5);
    assert!(s[4].starts_with("┃ 12:39:00 event 99"), "{s:?}");
    hit(&mut a, KeyCode::Up);
    let s = lines(&mut a, 40, 6);
    assert!(s[3].starts_with("┃ 12:38:00 event 98"), "the view stays and the mark moves up: {s:?}");
    assert!(s[4].starts_with("  12:39:00 event 99"), "{s:?}");
    assert_eq!(s[5], " 999 of 1,000 · G follow", "and the log says where it is");
    assert!(a.wins["log:all"].moved);
    // new rows come and the view stays
    send(&mut a, "log:all", log_rows(101), 1_001, 900);
    assert!(lines(&mut a, 40, 6).iter().any(|l| l.starts_with("┃ 12:38:00 event 98")), "the row the keys are on stays the same row");
    hit(&mut a, KeyCode::Down);
    assert!(a.wins["log:all"].moved, "one row from the end is not the end");
    hit(&mut a, KeyCode::Down);
    assert!(!a.wins["log:all"].moved, "on the last row it follows again");
    hit(&mut a, KeyCode::Up);
    assert!(a.wins["log:all"].moved);
    hit(&mut a, KeyCode::Char('G'));
    assert!(!a.wins["log:all"].moved);
    assert_eq!(a.selected(a.field().unwrap()), 1_001 - 1);
}

#[test]
fn the_wheel_on_the_log_is_three_presses_of_up_or_down() {
    // the wheel did nothing on a page that is not the chat
    let (mut a, _) = log_app(1_000, 100);
    let end = a.selected(a.field().unwrap());
    a.wheel(true);
    assert_eq!(a.selected(a.field().unwrap()), end - 3, "three rows up");
    assert!(a.wins["log:all"].moved);
    a.wheel(false);
    assert_eq!(a.selected(a.field().unwrap()), end, "and three down, to the end again");
}

#[test]
fn a_log_that_is_told_not_to_follow_stays_where_it_is() {
    let mut a = started(stack(vec![node("l", "", FieldKind::Log, "log:all", json!({"follow": false}))]));
    send(&mut a, "log:all", log_rows(50), 50, 0);
    assert_eq!(a.selected(a.field().unwrap()), 0);
    assert!(lines(&mut a, 40, 5)[0].starts_with("┃ 12:00:00 event 0"));
}

#[test]
fn the_end_of_a_log_is_at_the_bottom_whatever_the_rows_above_it_take() {
    let (mut a, _) = log_app(100, 100);
    // a row that is open takes its detail's lines, and the end is still the last line
    for _ in 0..1 {
        hit(&mut a, KeyCode::Up);
    }
    hit(&mut a, KeyCode::Char('G'));
    let s = lines(&mut a, 40, 6);
    assert!(s[5].starts_with("┃ 12:39:00 event 99"), "{s:?}");
    assert_eq!(s[4], "  12:38:00 event 98");
    assert!(s[0].starts_with("  12:34:00 event 94") && s[0].ends_with("▸"), "{s:?}");
}

#[test]
fn a_row_with_detail_opens_under_itself_with_enter_and_shuts_again() {
    let (mut a, _) = log_app(10, 10);
    hit(&mut a, KeyCode::Char('g'));
    for _ in 0..4 {
        hit(&mut a, KeyCode::Down);
    }
    let s = lines(&mut a, 40, 8);
    assert!(s[4].ends_with("▸"), "{s:?}");
    hit(&mut a, KeyCode::Enter);
    let s = lines(&mut a, 40, 8);
    assert!(s[4].ends_with("▾") && s[5] == "  ╰ detail of 4", "{s:?}");
    hit(&mut a, KeyCode::Char(' '));
    assert!(lines(&mut a, 40, 8)[4].ends_with("▸"));
    // a row with no detail does what its action does
    hit(&mut a, KeyCode::Up);
    assert!(hit(&mut a, KeyCode::Enter).is_empty());
}

#[test]
fn a_log_that_is_scrolled_says_where_it_is_and_the_find_is_the_same_as_the_tables() {
    let (mut a, _) = log_app(1_000_000, 100);
    assert_eq!(lines(&mut a, 60, 5).len(), 5);
    hit(&mut a, KeyCode::Up);
    assert_eq!(lines(&mut a, 60, 5)[4], " 999,999 of 1,000,000 · G follow · / find");
    assert_eq!(lines(&mut a, 40, 5)[4], " 999,999 of 1,000,000 · G follow");
    hit(&mut a, KeyCode::Char('/'));
    typing(&mut a, "err");
    assert!(lines(&mut a, 60, 5)[4].starts_with(" / err▏"));
    let sent = windows(&hit(&mut a, KeyCode::Enter));
    assert_eq!(sent[0].3, "err");
    assert!(lines(&mut a, 60, 5)[4].ends_with("· / err · G follow · esc clear"), "{:?}", lines(&mut a, 60, 5));
    let sent = windows(&hit(&mut a, KeyCode::Esc));
    assert_eq!(sent[0].3, "");
    assert!(!a.wins["log:all"].moved || a.wins["log:all"].cur.at == 0);
}

#[test]
fn a_log_asks_for_the_rows_around_the_end_it_shows_and_around_the_row_it_goes_to() {
    let mut a = App::new();
    a.start();
    a.down(hello());
    a.down(Down::Tree { node: stack(vec![node("l", "", FieldKind::Log, "log:all", Json::Null)]) });
    let ups = send(&mut a, "log:all", log_rows(3), 1_000_000, 999_997);
    let ask = windows(&ups);
    assert_eq!(ask.len(), 1, "the end is shown and only three of its rows are held: {ups:?}");
    assert!(ask[0].0 > 999_900 && ask[0].0 + ask[0].1 > 999_999, "{ask:?}");
    // the same rows again: asked already
    assert!(windows(&send(&mut a, "log:all", log_rows(3), 1_000_000, 999_997)).is_empty());
    let top = windows(&hit(&mut a, KeyCode::Char('g')));
    assert_eq!(top.len(), 1);
    assert_eq!(top[0].0, 0);
}

#[test]
fn a_log_the_core_has_sent_no_rows_for_is_drawn_empty_and_its_keys_ask_for_nothing() {
    let mut a = started(stack(vec![node("l", "", FieldKind::Log, "log:all", Json::Null)]));
    assert!(windows(&hit(&mut a, KeyCode::PageUp)).is_empty());
    assert!(a.wins.get("log:all").is_none_or(|w| w.find.is_empty()));
}

// ---- the page ----

#[test]
fn a_page_of_gauges_and_buttons_stacks_them_with_the_height_each_asks_for() {
    let page = Node {
        id: "root".into(),
        present: Present::Tabs,
        children: vec![Node {
            id: "spend".into(),
            title: "Spend".into(),
            present: Present::Sections,
            children: vec![
                Node { id: "today".into(), title: "today".into(), present: Present::Stack, children: vec![node("stats", "", FieldKind::Stat, "s:s", Json::Null), node("ctx", "context", FieldKind::Meter, "s:m", Json::Null), node("chips", "", FieldKind::Chip, "s:c", Json::Null)], ..Node::default() },
                Node { id: "do".into(), title: "do".into(), present: Present::Stack, children: vec![node("go", "probe", FieldKind::Action, "s:go", json!({"intent": {"intent": "run", "command": "probe"}}))], ..Node::default() },
            ],
            ..Node::default()
        }],
        ..Node::default()
    };
    let mut a = started(page);
    a.down(Down::Value { bind: "s:s".into(), value: json!([{"v": "1", "label": "a", "delta": 1}]), rev: 1 });
    a.down(Down::Value { bind: "s:m".into(), value: json!(0.5), rev: 1 });
    a.down(Down::Value { bind: "s:c".into(), value: json!({"text": "ok"}), rev: 1 });
    let s = lines(&mut a, 60, 12);
    let at = |t: &str| s.iter().position(|l| l.contains(t)).unwrap_or_else(|| panic!("{t}: {s:?}"));
    assert_eq!(at("today") + 4, at("context"), "three rows of stat between the header and the meter: {s:?}");
    assert_eq!(at("context") + 1, at("ok"));
    assert_eq!(at("ok") + 1, at("do"));
    assert_eq!(at("do") + 1, at("probe"));
}

// ---- 1,000,000 rows ----

#[test]
fn a_million_rows_of_a_log_scroll_without_a_wait() {
    let total: u64 = 1_000_000;
    let mut a = App::new();
    a.start();
    a.down(hello());
    a.down(Down::Tree { node: stack(vec![node("l", "", FieldKind::Log, "log:all", Json::Null)]) });
    let mut screen = Headless::new(160, 48);
    // the core: it answers each window asked for with the rows of it
    let serve = |a: &mut App, ups: Vec<Up>| {
        for u in ups {
            if let Up::Window { bind, from, count, .. } = u {
                let count = count.min(total - from);
                let rows = (from..from + count).map(|i| row(&format!("e{i}"), &[&format!("12:{:02}:{:02}", (i / 60) % 60, i % 60), &format!("event number {i}")])).collect();
                a.down(Down::Rows { bind, rows, total, from, rev: 1 });
            }
        }
    };
    let first = Instant::now();
    let ups = a.down(Down::Rows { bind: "log:all".into(), rows: Vec::new(), total, from: 0, rev: 1 });
    serve(&mut a, ups);
    let _ = screen.show(&mut a);
    let first = first.elapsed();
    let ups = a.windows();
    serve(&mut a, ups);
    let _ = screen.show(&mut a);
    // a page up, a page down, to the top, to the end, and a few hundred lines one by one
    let t = Instant::now();
    let mut keys = 0u32;
    let mut go = |a: &mut App, code: KeyCode| {
        let ups = a.key(press(code));
        serve(a, ups);
        let _ = screen.show(a);
        keys += 1;
    };
    for _ in 0..50 {
        go(&mut a, KeyCode::PageUp);
    }
    for _ in 0..200 {
        go(&mut a, KeyCode::Up);
    }
    go(&mut a, KeyCode::Char('g'));
    for _ in 0..50 {
        go(&mut a, KeyCode::PageDown);
    }
    go(&mut a, KeyCode::Char('G'));
    for _ in 0..200 {
        go(&mut a, KeyCode::Down);
    }
    let per = t.elapsed() / keys;
    let held = a.rows["log:all"].len();
    // generous for a debug build: the numbers that matter are those of the release bench
    assert!(first.as_millis() < 500, "first frame {first:?}");
    assert!(per.as_millis() < 50, "key and frame {per:?}");
    assert!(held < 400, "the screen holds three screens of a million rows, not more: {held}");
    assert_eq!(a.selected(a.field().unwrap()), total - 1);
    eprintln!("1M-row log: first frame {first:?}, key+frame {per:?} over {keys} keys, {held} rows held");
}

// ---- edge cases ----

#[test]
fn the_stat_columns_fit_to_the_cell_and_not_a_cell_less() {
    let mut a = one(FieldKind::Stat, "", Json::Null, json!([{"v": "$1.24", "label": "today"}, {"v": "38", "label": "tasks"}, {"v": "4.1s", "label": "p50"}]));
    // 2 + (5 + 5 + 4) + 4 * 2 = 24
    assert_eq!(lines(&mut a, 24, 4)[1], "  today    tasks    p50", "wide at exactly the width it needs");
    assert_eq!(lines(&mut a, 23, 4)[1], "", "and one row of numbers a cell less");
    assert_eq!(lines(&mut a, 23, 4)[0], "  $1.24  38  4.1s");
}

#[test]
fn a_stat_row_for_the_change_is_there_when_one_of_them_has_one_and_the_narrow_mark_has_its_colour() {
    let mut a = one(FieldKind::Stat, "", Json::Null, json!([{"v": "$1.24", "label": "today", "delta": 12}, {"v": "38", "label": "tasks"}]));
    let s = lines(&mut a, 60, 5);
    assert_eq!(s[2], "  ▲ 12%", "{s:?}");
    let mut b = one(FieldKind::Stat, "", Json::Null, json!([{"v": "$1.24", "label": "a long label here", "delta": 12}, {"v": "38", "label": "another long one", "delta": -3}]));
    let tones = Headless::new(30, 4).tones(&mut b);
    assert_eq!(lines(&mut b, 30, 4)[0], "  $1.24 ▲  38 ▼");
    assert_eq!((tones[0].chars().nth(8), tones[0].chars().nth(14)), (Some('g'), Some('r')), "the mark of a rise is good and of a fall bad: {tones:?}");
}

#[test]
fn a_meter_goes_beside_its_label_only_when_four_cells_are_left_for_the_bar() {
    // width 60: 2 + label + 2 + 7 + room
    let beside = |label_w: usize, width: u16| {
        let mut a = one(FieldKind::Meter, &"l".repeat(label_w), Json::Null, json!(0.5));
        lines(&mut a, width, 4)
    };
    let s = beside(45, 60);
    assert_eq!(s[1], "", "one row with a bar of four cells: {s:?}");
    assert!(s[0].ends_with("━━── 50%"), "{s:?}");
    let s = beside(46, 60);
    assert!(s[1].starts_with("│ ━") && s[0] == format!("├ {}", "l".repeat(46)), "three cells left, so the bar goes under the label: {s:?}");
    let s = beside(2, 60);
    assert_eq!(s[1], "", "at 60 columns there is room beside the label");
    assert_eq!(beside(2, 59)[0], "├ ll", "and at 59 there is not");
}

#[test]
fn the_bar_under_a_label_is_as_wide_as_the_screen_lets_it_be_up_to_8_cells() {
    let mut a = one(FieldKind::Meter, "m", Json::Null, json!(1.0));
    assert_eq!(lines(&mut a, 20, 3)[1], format!("│ {} 100%", "━".repeat(8)), "20 columns less the pipe, the blank and four for the number is eleven cells, and a meter is at most eight");
    assert_eq!(lines(&mut a, 50, 3)[1], format!("│ {} 100%", "━".repeat(8)), "wherever there is room, no more than 8");
    let mut b = one(FieldKind::Meter, "m", Json::Null, json!(0.5));
    assert!(lines(&mut b, 8, 3)[1].starts_with("│ ━━── "), "and not fewer than four");
}

#[test]
fn a_spark_without_a_window_shows_the_last_forty_points() {
    let pts: Vec<u32> = (0..50).collect();
    let mut a = one(FieldKind::Spark, "s", Json::Null, json!(pts));
    let l = lines(&mut a, 90, 2)[0].clone();
    assert_eq!(l.chars().filter(|c| ('▁'..='█').contains(c)).count(), 40, "{l}");
}

#[test]
fn a_spark_goes_beside_its_label_when_four_cells_are_left_and_a_tiny_one_keeps_four() {
    // 2 + label + 2 + 2 + 6 + room
    let rows = |label_w: usize, width: u16| {
        let mut a = one(FieldKind::Spark, &"l".repeat(label_w), Json::Null, json!({"points": [1, 2, 3, 4, 5, 6], "last": "x"}));
        lines(&mut a, width, 3)
    };
    let s = rows(48, 64);
    assert_eq!(s[1], "", "room for four cells beside the label: {s:?}");
    let s = rows(49, 64);
    assert!(s[1].starts_with("│ "), "three is not enough: {s:?}");
    assert_eq!(rows(2, 60)[1], "");
    assert_eq!(rows(2, 59)[0], "├ ll");
    let tiny = rows(2, 10);
    assert_eq!(tiny[1].chars().filter(|c| ('▁'..='█').contains(c)).count(), 4, "{tiny:?}");
}

#[test]
fn esc_on_a_button_waiting_for_its_second_press_gives_that_up_and_stays_in_the_section() {
    let mut page = stack(vec![node("w", "widen", FieldKind::Action, "act:w", json!({"intent": {"intent": "run", "command": "widen"}, "risk": 90}))]);
    page.id = "body".into();
    let root = Node { id: "root".into(), present: Present::Sections, children: vec![Node { title: "do".into(), ..page }], ..Node::default() };
    let mut a = started(root);
    hit(&mut a, KeyCode::Right);
    assert!(a.inside.contains("root"), "the keys went into the section");
    hit(&mut a, KeyCode::Enter);
    assert!(a.armed.is_some());
    hit(&mut a, KeyCode::Esc);
    assert!(a.armed.is_none() && a.inside.contains("root"), "the first esc only gives up the press");
    hit(&mut a, KeyCode::Esc);
    assert!(!a.inside.contains("root"), "the next leaves the section");
}

#[test]
fn a_control_key_is_not_a_press_and_a_button_that_may_not_be_pressed_never_looks_pressed() {
    let mut a = buttons();
    let up = a.key(j4n_tui::headless::ctrl(' '));
    assert!(ran(&up).is_empty());
    let enter = crossterm::event::KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::CONTROL);
    assert!(ran(&a.key(enter)).is_empty(), "ctrl and enter is not enter");
    // a disabled button that is the whole of a tab has the keys and is not drawn as if it could be pressed
    let tabs = Node { id: "root".into(), present: Present::Tabs, children: vec![node("off", "locked", FieldKind::Action, "act:off", json!({"intent": {"intent": "run", "command": "x"}, "disabled": true}))], ..Node::default() };
    let mut b = started(tabs);
    assert_eq!(b.page().map(|p| p.id.as_str()), Some("off"));
    assert_eq!(lines(&mut b, 30, 4)[2], "  ⏎ locked \u{F0341}", "not in a box");
    let accent = b.theme.roles.get(j4n_tui::theme::palette::Role::Accent);
    let ansi = Headless::new(30, 4).snap(&mut b).ansi;
    assert!(!ansi.contains(&format!("48;2;{};{};{}", accent.0, accent.1, accent.2)), "no inverse box on a button that is off: {ansi:?}");
}

#[test]
fn a_bar_may_be_told_to_drop_its_first_cell_and_still_keeps_one() {
    let mut a = bar_app(json!({"drop": [1, 0]}), &["claude", "$0.33"]);
    assert_eq!(lines(&mut a, 8, 2)[0], " claude", "the cell named first goes and the one named next stays because it is the last");
    let mut b = bar_app(json!({"drop": [0]}), &["claude", "$0.33"]);
    assert_eq!(lines(&mut b, 8, 2)[0], " $0.33", "named, the first goes");
}

#[test]
fn a_bar_without_colour_is_reversed_and_with_colour_it_is_raised() {
    let mut a = bar_app(Json::Null, &["claude"]);
    a.theme = j4n_tui::theme::Theme::build(&j4n_tui::theme::Settings { no_color: true, ..Default::default() }, &[]).unwrap();
    let plain = Headless::new(20, 2).snap(&mut a).ansi;
    assert!(plain.lines().next().unwrap().contains(";7m") || plain.lines().next().unwrap().contains("[7m"), "{plain:?}");
    let mut b = bar_app(Json::Null, &["claude"]);
    let raise = b.theme.roles.get(j4n_tui::theme::palette::Role::Raise);
    let ansi = Headless::new(20, 2).snap(&mut b).ansi;
    assert!(ansi.contains(&format!("48;2;{};{};{}", raise.0, raise.1, raise.2)), "{ansi:?}");
}

#[test]
fn a_table_column_grows_to_28_cells_and_an_icon_is_two_of_them() {
    let mut a = started(stack(vec![node("t", "", FieldKind::Table, "view:m", json!({"columns": ["name", "n"]}))]));
    let mut r = row("a", &[&"w".repeat(40), "1"]);
    r.cells[1].icon = "done".into();
    send(&mut a, "view:m", vec![r], 1, 0);
    let s = lines(&mut a, 60, 5);
    assert_eq!(s[1], format!("┃ {}…  \u{F05E1} 1", "w".repeat(27)), "{s:?}");
}

#[test]
fn a_table_column_with_a_width_has_it_whatever_it_holds() {
    let mut a = started(stack(vec![node("t", "", FieldKind::Table, "view:m", json!({"columns": [{"name": "a", "width": 12}, "b"]}))]));
    send(&mut a, "view:m", vec![row("a", &["x", "y"])], 1, 0);
    let s = lines(&mut a, 40, 5);
    assert_eq!(s[1], "┃ x             y", "{s:?}");
    assert_eq!(s[0], "  a             b");
}

#[test]
fn a_table_four_rows_high_has_a_footer_and_three_rows_high_has_not() {
    let (mut a, _) = table_app(40, 40);
    let s = lines(&mut a, 40, 4);
    assert_eq!(s[3], " 2 of 40 · s sort · / find", "{s:?}");
    let t = lines(&mut a, 40, 3);
    assert!(!t.iter().any(|l| l.contains(" of 40")), "{t:?}");
}

#[test]
fn the_row_the_keys_are_on_is_in_sight_after_every_key() {
    let (mut a, _) = table_app(30, 30);
    for i in 1..30 {
        hit(&mut a, KeyCode::Down);
        let s = lines(&mut a, 30, 6);
        assert!(s.iter().any(|l| l.starts_with(&format!("┃ model {i} "))) || s.iter().any(|l| l == &format!("┃ model {i}")), "row {i}: {s:?}");
    }
    for i in (0..29).rev() {
        hit(&mut a, KeyCode::Up);
        let s = lines(&mut a, 30, 6);
        assert!(s.iter().any(|l| l.starts_with(&format!("┃ model {i} ")) || l == &format!("┃ model {i}")), "row {i}: {s:?}");
    }
}

#[test]
fn a_table_keeps_its_columns_while_they_fit_to_the_cell() {
    let (mut a, _) = table_app(5, 5);
    // 2 for the mark, 7 + 2 + 4 for the columns, 2 for the thumb
    assert!(lines(&mut a, 17, 6)[0].ends_with("uses"), "{:?}", lines(&mut a, 17, 6));
    assert!(!lines(&mut a, 16, 6)[0].ends_with("uses"), "{:?}", lines(&mut a, 16, 6));
}

#[test]
fn a_row_before_a_section_is_the_last_of_its_own() {
    let mut a = started(stack(vec![node("l", "", FieldKind::List, "view:x", Json::Null)]));
    send(&mut a, "view:x", vec![row("flat", &["flat"]), row("head", &["head", "1"]), under("kid", "head", &["kid"])], 3, 0);
    hit(&mut a, KeyCode::Down);
    let s = lines(&mut a, 30, 5);
    assert_eq!((s[0].as_str(), s[1].as_str(), s[2].as_str()), ("╰ flat", "▾ head  1", "╰ kid"), "{s:?}");
}

#[test]
fn the_hint_of_a_list_row_is_never_closer_than_two_cells_to_the_text() {
    let mut a = started(stack(vec![node("l", "", FieldKind::List, "view:x", Json::Null)]));
    let mut r = row("a", &[&"a".repeat(18)]);
    r.actions = vec![probe("p", "probe")];
    send(&mut a, "view:x", vec![r], 1, 0);
    let s = lines(&mut a, 28, 3);
    assert!(s[0].contains("…  ⏎"), "the text gives way, so the hint is two cells from it: {s:?}");
}

#[test]
fn a_screenful_is_the_screen_less_four_rows_and_never_less_than_eight() {
    let mut a = App::new();
    a.size = (80, 40);
    assert_eq!(a.screenful(), 36);
    a.size = (80, 12);
    assert_eq!(a.screenful(), 8);
    a.size = (80, 10);
    assert_eq!(a.screenful(), 8, "six less four is two, and eight is the least");
    a.size = (80, 11);
    assert_eq!(a.screenful(), 8);
}

#[test]
fn the_window_asked_for_is_three_screens_around_the_row_and_nothing_when_there_is_nothing() {
    let (mut a, _) = table_app(1_000, 3);
    let ask = windows(&hit(&mut a, KeyCode::End));
    assert_eq!(ask.len(), 1);
    assert_eq!((ask[0].0, ask[0].1), (999 - 20 - 10, 61), "a screen and half a screen before the row, and three screens in all");
    // no rows, no total: nothing to ask for
    let (_, none) = table_app(0, 0);
    assert!(windows(&none).is_empty());
}

#[test]
fn rows_that_start_where_the_screen_needs_them_and_reach_the_row_after_the_last_wanted_are_enough() {
    // sel 0, a screen's worth of 20 below it: rows 0..=20 are wanted
    let (_, ups) = table_app(1_000, 21);
    assert!(windows(&ups).is_empty(), "21 rows cover 0..=20");
    let (_, ups) = table_app(1_000, 20);
    assert_eq!(windows(&ups).len(), 1, "20 rows are one short");
    let (mut a, _) = table_app(10_000, 100);
    assert!(windows(&hit(&mut a, KeyCode::Down)).is_empty());
    // a window that starts exactly at the first row wanted is enough
    let (mut b, _) = table_app(1_000, 3);
    for _ in 0..30 {
        hit(&mut b, KeyCode::Down);
    }
    let ups = send(&mut b, "view:m", models(41), 1_000, 10);
    assert!(windows(&ups).is_empty(), "rows 10..=50 are what a row at 30 wants");
}

#[test]
fn a_find_is_trimmed_and_esc_with_no_find_asks_for_nothing() {
    let (mut a, _) = table_app(1_000, 40);
    assert!(windows(&hit(&mut a, KeyCode::Esc)).is_empty(), "there is no find to take off");
    hit(&mut a, KeyCode::Char('/'));
    typing(&mut a, " ha ");
    let sent = windows(&hit(&mut a, KeyCode::Enter));
    assert_eq!(sent[0].3, "ha");
}

#[test]
fn a_new_find_in_a_log_follows_the_end_of_what_was_found() {
    let (mut a, _) = log_app(1_000, 100);
    hit(&mut a, KeyCode::Up);
    assert!(a.wins["log:all"].moved);
    hit(&mut a, KeyCode::Char('/'));
    typing(&mut a, "x");
    hit(&mut a, KeyCode::Enter);
    assert!(!a.wins["log:all"].moved);
}

#[test]
fn rows_under_a_shut_row_are_hidden_all_the_way_down() {
    let mut a = tree_app();
    hit(&mut a, KeyCode::Left);
    let s = lines(&mut a, 40, 8);
    assert_eq!(s[0].trim_end_matches(['▸', ' ']), "┃ fix trailing commas  3:12", "{s:?}");
    assert!(s[1].is_empty(), "no child and no grandchild: {s:?}");
}

#[test]
fn a_row_whose_parent_comes_after_it_is_at_the_top() {
    let mut a = started(stack(vec![node("t", "", FieldKind::Tree, "t:x", Json::Null)]));
    send(&mut a, "t:x", vec![under("a", "b", &["a"]), row("b", &["b"])], 2, 0);
    let s = lines(&mut a, 20, 3);
    assert_eq!((s[0].as_str(), s[1].as_str()), ("┃ a", "  b"), "{s:?}");
}

#[test]
fn a_tree_goes_to_its_first_row_with_g_and_by_a_page_and_keeps_the_row_in_sight() {
    let mut a = started(stack(vec![node("t", "", FieldKind::Tree, "t:x", Json::Null)]));
    let mut rows = vec![row("root", &["root"])];
    rows.extend((0..40).map(|i| under(&format!("k{i}"), "root", &[&format!("kid {i}")])));
    send(&mut a, "t:x", rows, 41, 0);
    hit(&mut a, KeyCode::PageDown);
    let sel = a.wins["t:x"].cur.at;
    assert_eq!(sel, a.screenful(), "a page down is a screen's worth");
    hit(&mut a, KeyCode::PageUp);
    assert_eq!(a.wins["t:x"].cur.at, 0);
    hit(&mut a, KeyCode::End);
    hit(&mut a, KeyCode::Char('g'));
    assert_eq!(a.wins["t:x"].cur.at, 0, "g is the first row");
    for i in 1..=30usize {
        hit(&mut a, KeyCode::Down);
        let s = lines(&mut a, 30, 6);
        assert!(s.iter().any(|l| l.starts_with("┃ ") && l.contains(&format!("kid {}", i - 1))), "row {i}: {s:?}");
    }
}

#[test]
fn cards_are_as_wide_as_their_longest_title_asks_up_to_28_and_a_row_of_them_fits_to_the_cell() {
    let mut a = started(stack(vec![node("c", "", FieldKind::Cards, "view:c", Json::Null)]));
    send(&mut a, "view:c", vec![row("a", &[&"t".repeat(60)])], 1, 0);
    let s = lines(&mut a, 60, 5);
    assert_eq!(s[0].chars().count(), 28, "{s:?}");
    // two cards of 14 and the blank between are 29 columns
    let mut b = cards_app();
    let s = lines(&mut b, 29, 6);
    assert_eq!(s[0], "┏━━━━━━━━━━━━┓ ╭────────────╮", "{s:?}");
    let s = lines(&mut b, 28, 9);
    assert_eq!(s[0], "┏━━━━━━━━━━━━┓", "one card in a row at 28");
    assert_eq!(s[4], "╭────────────╮");
}

#[test]
fn a_card_shows_at_most_three_lines_under_its_title_and_a_short_screen_shows_part_of_it() {
    let mut a = started(stack(vec![node("c", "", FieldKind::Cards, "view:c", json!({"width": 14}))]));
    send(&mut a, "view:c", vec![row("a", &["t", "one", "two", "three", "four", "five"])], 1, 0);
    let s = lines(&mut a, 20, 8);
    assert_eq!((s[2].as_str(), s[3].as_str(), s[4].as_str(), s[5].as_str()), ("┃ one        ┃", "┃ two        ┃", "┃ three      ┃", "┗━━━━━━━━━━━━┛"), "{s:?}");
    let short = lines(&mut a, 20, 2);
    assert_eq!(short[0], "┏━━━━━━━━━━━━┓", "a screen shorter than a card draws the top of it: {short:?}");
}

#[test]
fn the_card_the_keys_are_on_is_in_sight_after_every_key() {
    let mut a = cards_app();
    for i in 1..4 {
        hit(&mut a, KeyCode::Down);
        let s = lines(&mut a, 20, 5);
        let name = ["claude", "omp", "gemini", "pi"][i];
        assert!(s.iter().any(|l| l.starts_with("┃ ") && l.contains(name)), "card {i}: {s:?}");
    }
}

#[test]
fn the_arrows_of_one_card_are_not_taken_and_the_end_of_the_cards_is_the_last_card() {
    let mut a = started(stack(vec![node("c", "claude", FieldKind::Card, "agent:x", Json::Null)]));
    send(&mut a, "agent:x", vec![row("claude", &["claude", "ready"])], 1, 0);
    let f = a.field().cloned().expect("the card has the keys");
    for code in [KeyCode::Down, KeyCode::Up, KeyCode::Left, KeyCode::Right, KeyCode::Home, KeyCode::End] {
        assert!(display::key(&mut a, &f, press(code)).is_none(), "{code:?} is for whatever is around the card");
    }
    let mut b = cards_app();
    hit(&mut b, KeyCode::End);
    assert_eq!(b.wins["view:c"].cur.at, 3);
}

#[test]
fn a_log_keeps_a_blank_between_its_text_and_its_meta_and_the_time_column_is_as_wide_as_the_widest() {
    let mut a = started(stack(vec![node("l", "", FieldKind::Log, "log:all", Json::Null)]));
    let r = |t: &str, text: &str, meta: &str| Row { key: t.into(), cells: vec![cell(t), cell(text), cell(meta)], ..Row::default() };
    send(&mut a, "log:all", vec![r("9:00", "abc", "m"), r("12:00:00", "def", "n")], 2, 0);
    // 2 for the mark, 8 for the time and a blank, 3 of text, 1 of meta, 2 for the thumb's place: the text and the meta would touch
    let s = lines(&mut a, 17, 4);
    assert_eq!(s[0], "  9:00     abc m", "the times start the text in one column: {s:?}");
    assert!(s[1].ends_with("def n"), "{s:?}");
}

#[test]
fn the_footer_of_a_log_names_the_find_key_from_48_columns() {
    let (mut a, _) = log_app(1_000, 100);
    hit(&mut a, KeyCode::Up);
    assert!(lines(&mut a, 48, 5)[4].ends_with("· G follow · / find"), "{:?}", lines(&mut a, 48, 5));
    assert!(lines(&mut a, 47, 5)[4].ends_with("· G follow"), "{:?}", lines(&mut a, 47, 5));
}

#[test]
fn enter_on_a_log_row_with_no_detail_opens_nothing() {
    let (mut a, _) = log_app(10, 10);
    hit(&mut a, KeyCode::Char('g'));
    hit(&mut a, KeyCode::Enter);
    assert!(a.wins["log:all"].flip.is_empty());
}

#[test]
fn a_thumb_is_there_when_there_are_more_rows_than_the_screen_shows_and_not_when_all_are_in_sight() {
    // 10 rows in a body of 10: the header, ten rows, the footer
    let (mut a, _) = table_app(10, 10);
    let s = screen(&mut a, 30, 12);
    assert!(!s[1..11].iter().any(|l| l.ends_with('┃') && !l.starts_with("┃ model 0")), "{s:?}");
    assert!(!s[1].ends_with('┃'), "not on the row that has the keys either: {s:?}");
    let (mut b, _) = table_app(11, 11);
    assert!(screen(&mut b, 30, 12)[1].ends_with('┃'), "one more and there is");
}

#[test]
fn the_tone_words_are_the_ones_of_the_protocol_and_the_two_older_names_for_good_and_bad() {
    let mut a = one(FieldKind::Chip, "", Json::Null, json!([{"text": "aaa", "tone": "ok"}, {"text": "bbb", "tone": "error"}, {"text": "ccc", "tone": "good"}, {"text": "ddd", "tone": "bad"}, {"text": "eee", "tone": "dim"}, {"text": "fff", "tone": "warn"}, {"text": "ggg", "tone": "accent"}, {"text": "hhh", "tone": "plain"}]));
    let t = Headless::new(60, 2).tones(&mut a);
    let line = lines(&mut a, 60, 2)[0].clone();
    let col = |w: &str| line.find(w).map(|b| line[..b].chars().count()).unwrap();
    let tone = |w: &str| t[0].chars().nth(col(w)).unwrap();
    assert_eq!(["aaa", "bbb", "ccc", "ddd", "eee", "fff", "ggg", "hhh"].map(tone), ['g', 'r', 'g', 'r', 'd', 'y', 'c', '.']);
}

// ---- theme.frame reaches the Tasks tree and the list pages ----

fn framed(mut a: App, frame: &str) -> App {
    a.theme = Theme::build(&Settings { frame: frame.into(), marks: "unicode".into(), ..Settings::default() }, &[]).expect("a theme");
    a
}

fn has_pipe(s: &[String]) -> bool {
    s.iter().any(|l| l.contains(['├', '╰', '└', '│', '┆']))
}

#[test]
fn theme_frame_airy_changes_the_tasks_tree_and_a_list_page_and_hang_has_no_tee() {
    // the pipes: the tree has them, and so does the page, with square ends
    let s = lines(&mut framed(tree_app(), "pipes"), 40, 8);
    assert!(s[1].contains("├─ plan") && s[4].contains("└─ check"), "{s:#?}");
    let s = lines(&mut framed(list_app(), "pipes"), 40, 8);
    assert_eq!((s[1].as_str(), s[2].as_str()), ("├ claude  sonnet", "└ omp     opus"), "{s:#?}");
    let s = lines(&mut framed(list_app(), "round"), 40, 8);
    assert_eq!((s[1].as_str(), s[2].as_str()), ("├ claude  sonnet", "╰ omp     opus"), "round, the default, ends rounded: {s:#?}");
    // airy: no line anywhere; an arrow for what hangs from the head of the tree, a dot for a row of a page
    let s = lines(&mut framed(tree_app(), "airy"), 40, 8);
    assert!(!has_pipe(&s) && s[1].contains("› plan") && s[3].contains("› commit?") && s[5].contains("› lint"), "{s:#?}");
    let s = lines(&mut framed(list_app(), "airy"), 40, 8);
    assert!(!has_pipe(&s) && s[1].starts_with("· claude  sonnet") && s[2].starts_with("· omp     opus"), "{s:#?}");
    // hang: a rail only where a row goes on, an elbow at the end, and a page has no tree at its top
    let s = lines(&mut framed(tree_app(), "hang"), 40, 8);
    assert!(s[1].contains("┆ plan") && s[4].contains("╰ check") && !s.iter().any(|l| l.contains('├')), "{s:#?}");
    let s = lines(&mut framed(list_app(), "hang"), 40, 8);
    assert!(s[1].starts_with("  claude  sonnet") && !has_pipe(&s), "{s:#?}");
    // the same words, whatever the look
    let words = |s: Vec<String>| s.iter().map(|l| l.split_whitespace().filter(|w| !w.contains(['├', '╰', '└', '│', '┆', '·', '›'])).collect::<Vec<_>>().join(" ")).collect::<Vec<_>>();
    assert_eq!(words(lines(&mut framed(tree_app(), "airy"), 40, 8)), words(lines(&mut framed(tree_app(), "pipes"), 40, 8)));
}

#[test]
fn a_bar_stays_in_every_column_of_a_tasks_tree_however_deep() {
    // a: [b: [c: [d]], e]: `d` is three levels in, and `b` has `e` after it, so the column of `b` is a bar, not a blank
    let mut a = started(stack(vec![node("t", "", FieldKind::Tree, "tasks:deep", Json::Null)]));
    send(&mut a, "tasks:deep", vec![row("a", &["a"]), under("b", "a", &["b"]), under("c", "b", &["c"]), under("d", "c", &["d"]), under("e", "a", &["e"])], 5, 0);
    let s = lines(&mut a, 40, 8);
    assert_eq!(&s[1..5], ["  ├─ b", "  │ ╰─ c", "  │   ╰─ d", "  ╰─ e"], "{s:#?}");
}
