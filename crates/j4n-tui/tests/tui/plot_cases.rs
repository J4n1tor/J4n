//! The charts (`display/plot.rs`): what `line` and `bars` draw for the value and props they are given. The sheets are the
//! goldens of the scenes `plot-*`. These cases draw with `look.chart = plain`, the drawer of braille and blocks that stays under the dithered
//! one; `dither_cases` has the dithered charts and the proof that every other look still gets this drawer's output.

use crate::world::{hello, screen};
use j4n_proto::{Down, Field, FieldKind, Json, Node, Present, chart_problem, json};
use j4n_tui::App;
use j4n_tui::display::{self, plot};
use j4n_tui::theme::{Settings, Theme};

fn chart(kind: FieldKind, props: Json, value: Json) -> App {
    let field = Field { kind, bind: "c:f".into(), props };
    let node = Node { id: "root".into(), present: Present::Stack, children: vec![Node { id: "f".into(), title: "spend".into(), field: Some(field), ..Node::default() }], ..Node::default() };
    let mut a = App::new();
    a.theme = Theme::build(&plain(), &[]).expect("a theme");
    a.start();
    a.down(hello());
    a.down(Down::Tree { node });
    a.down(Down::Value { bind: "c:f".into(), value, rev: 1 });
    a
}

/// The look with the plain charts, which the cases of this file are about.
fn plain() -> Settings {
    Settings { chart: "plain".into(), ..Settings::default() }
}

fn lines(a: &mut App, cols: u16, rows: u16) -> Vec<String> {
    screen(a, cols, rows).into_iter().map(|l| l.trim_end().to_string()).collect()
}

fn one(name: &str) -> Json {
    json!([{ "name": name }])
}

fn braille(s: &[String]) -> usize {
    s.iter().flat_map(|l| l.chars()).filter(|c| ('\u{2801}'..='\u{28ff}').contains(c)).count()
}

#[test]
fn a_line_is_braille_dots_that_climb_with_the_numbers_and_the_axis_says_where_they_end() {
    let v = json!({ "x": ["a", "b", "c", "d", "e"], "series": { "usd": [0, 1, 2, 3, 4] } });
    let mut a = chart(FieldKind::Line, json!({ "series": [{ "name": "usd", "unit": "usd" }] }), v);
    let s = lines(&mut a, 40, 8);
    assert!(s[0].contains("spend") && s[0].contains("usd"), "{s:?}");
    assert!(s[1].contains("$4") && s[6].contains("$0"), "the axis names the top and the bottom: {s:?}");
    assert!(braille(&s[1..4]) > 0 && braille(&s[4..7]) > 0, "dots in the upper and the lower rows: {s:?}");
    let first = |row: &String| row.chars().position(|c| ('\u{2801}'..='\u{28ff}').contains(&c));
    assert!(first(&s[1]) > first(&s[6]), "the top row's dots are further to the right than the bottom row's: {s:?}");
    assert!(s[7].trim().starts_with('a') && s[7].ends_with('e'), "the first and the last label stand under the plot: {s:?}");
}

#[test]
fn many_points_are_quadrants_and_the_ascii_tier_has_box_lines_of_plain_signs() {
    let x: Vec<String> = (0..300).map(|i| i.to_string()).collect();
    let y: Vec<f64> = (0..300).map(|i| f64::from(i % 17)).collect();
    let props = json!({ "window": 300, "series": one("n") });
    let mut a = chart(FieldKind::Line, props.clone(), json!({ "x": x, "series": { "n": y } }));
    let s = lines(&mut a, 40, 8);
    assert!(s.iter().any(|l| l.contains(['▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█'])) && braille(&s) == 0, "{s:?}");
    let mut b = chart(FieldKind::Line, json!({ "series": one("n") }), json!({ "x": ["a", "b", "c"], "series": { "n": [1, 5, 2] } }));
    b.theme = Theme::build(&Settings { marks: "ascii".into(), ..Default::default() }, &[]).unwrap();
    let t = lines(&mut b, 40, 8);
    assert!(braille(&t) == 0 && t.iter().any(|l| l.contains('+')) && t.iter().any(|l| l.contains('-')), "{t:?}");
}

#[test]
fn stream_and_wave_are_box_lines_with_corners_where_the_line_turns() {
    let v = json!({ "x": ["a", "b", "c", "d"], "series": { "n": [0, 10, 10, 0] } });
    for style in ["stream", "wave"] {
        let mut a = chart(FieldKind::Line, json!({ "style": style, "series": one("n") }), v.clone());
        let s = lines(&mut a, 24, 8).join("\n");
        assert!(s.contains('─') && (s.contains('╭') || s.contains('╯')) && s.contains('│'), "{style}: {s}");
    }
    let mut a = chart(FieldKind::Line, json!({ "style": "stream", "series": one("n") }), v);
    let s = lines(&mut a, 24, 8).join("\n");
    assert!(s.contains('╭') && s.contains('╮') && s.contains('╰') && s.contains('╯'), "up, across and down: {s}");
}

#[test]
fn a_log_axis_has_powers_of_ten_and_leaves_out_what_is_not_above_zero() {
    let v = json!({ "x": ["a", "b", "c", "d", "e"], "series": { "ms": [-5, 10, 1000, 0, 100] } });
    let mut a = chart(FieldKind::Line, json!({ "scale": "log", "series": [{ "name": "ms" }] }), v.clone());
    let s = lines(&mut a, 40, 8);
    assert!(s[1].contains("1k") && s[6].contains("10"), "labels on powers of ten: {s:?}");
    let mut b = chart(FieldKind::Line, json!({ "series": [{ "name": "ms" }] }), v);
    let t = lines(&mut b, 40, 8);
    assert!(braille(&t) > braille(&s), "on a linear axis the same numbers draw more: {t:?}");
    let mut c = chart(FieldKind::Line, json!({ "scale": "log", "series": one("n") }), json!({ "x": ["a", "b"], "series": { "n": [0, -3] } }));
    let u = lines(&mut c, 40, 8);
    assert!(u[0] == "  spend  —" && u[1..].iter().all(String::is_empty), "nothing above zero, nothing drawn: {u:?}");
}

#[test]
fn a_right_axis_has_its_own_range_and_its_own_words() {
    let v = json!({ "x": ["a", "b", "c"], "series": { "usd": [1, 2, 3], "tokens": [100000, 2000000, 300000] } });
    let props = json!({ "series": [{ "name": "usd", "unit": "usd" }, { "name": "tokens", "axis": "right", "role": "run", "unit": "n" }] });
    let mut a = chart(FieldKind::Line, props, v);
    let s = lines(&mut a, 48, 8);
    assert!(s[1].starts_with("  $3│") && s[1].ends_with("│2M"), "$ on the left and tokens on the right: {s:?}");
    assert!(s[6].starts_with("  $1│") && s[6].ends_with("│100k"), "{s:?}");
}

#[test]
fn the_window_is_the_last_points_and_the_axis_is_fitted_to_what_is_in_it() {
    let v = json!({ "x": ["a", "b", "c", "d", "e", "f"], "series": { "n": [900, 5, 6, 7, 8, 9] } });
    let mut a = chart(FieldKind::Line, json!({ "window": 3, "series": one("n") }), v.clone());
    let s = lines(&mut a, 40, 8);
    assert!(s[1].contains('9') && !s.join("").contains("900") && s[7].trim().starts_with('d'), "{s:?}");
    let mut b = chart(FieldKind::Line, json!({ "series": one("n") }), v);
    assert!(lines(&mut b, 40, 8)[1].contains("900"));
}

#[test]
fn bars_are_columns_in_eighths_of_a_cell_from_zero_and_grouped_by_label() {
    let v = json!({ "x": ["a", "b"], "series": { "plan": [4, 2], "list": [4, 1] } });
    let props = json!({ "series": [{ "name": "plan", "unit": "n" }, { "name": "list", "role": "run", "unit": "n" }] });
    let mut a = chart(FieldKind::Bars, props, v);
    let s = lines(&mut a, 40, 8);
    let full = |col: usize| s.iter().filter(|l| l.chars().nth(col) == Some('█')).count();
    let first = s[1].chars().position(|c| c == '█').expect("a bar reaches the top");
    assert_eq!(full(first), 6, "four of four is every row of the plot: {s:?}");
    assert!(s.iter().any(|l| l.contains('▄')), "the half bar ends in a half block: {s:?}");
    assert!(s[7].contains('a') && s[7].contains('b'), "{s:?}");
}

#[test]
fn horizontal_bars_have_the_label_the_bar_and_the_number() {
    let v = json!({ "x": ["l1", "l2"], "series": { "n": [3, 6] } });
    let mut a = chart(FieldKind::Bars, json!({ "orient": "horizontal", "series": one("n") }), v);
    let s = lines(&mut a, 30, 6);
    assert!(s[1].starts_with("  l1 ") && s[1].ends_with(" 3") && s[2].starts_with("  l2 ") && s[2].ends_with(" 6"), "{s:?}");
    let len = |l: &String| l.chars().filter(|c| *c == '█').count();
    assert!(len(&s[2]) > len(&s[1]) && len(&s[2]) >= 2 * len(&s[1]) - 1, "{s:?}");
}

#[test]
fn more_than_four_series_are_refused_and_the_chart_says_so() {
    let five: Vec<Json> = (0..5).map(|i| json!({ "name": format!("s{i}") })).collect();
    let props = json!({ "series": five });
    assert!(chart_problem(FieldKind::Line, &props).is_some_and(|w| w.contains("one to 4")));
    let mut a = chart(FieldKind::Line, props, json!({ "x": ["a"], "series": {} }));
    let s = lines(&mut a, 60, 8);
    assert!(s[0].contains("one to 4 series, not 5") && s[1..].iter().all(String::is_empty), "{s:?}");
    assert_eq!(plot::rows(&Field { kind: FieldKind::Line, bind: "c".into(), props: json!({ "series": [] }) }, &Json::Null), 1, "a refused chart is one row");
}

#[test]
fn a_chart_with_nothing_to_draw_is_one_row_and_one_series_is_accent_and_two_are_accent_and_run() {
    let mut a = chart(FieldKind::Line, json!({ "series": one("n") }), Json::Null);
    assert_eq!(lines(&mut a, 40, 8)[0], "  spend  —");
    let mut one_series = chart(FieldKind::Line, json!({ "series": [{ "name": "a" }] }), json!({ "x": ["p", "q"], "series": { "a": [1, 2] } }));
    let t = j4n_tui::headless::Headless::new(40, 8).tones(&mut one_series);
    assert!(t[0].contains('c') && !t[0].contains('?'), "one series is accent: {t:?}");
    let two = json!({ "series": [{ "name": "a" }, { "name": "b" }] });
    let mut b = chart(FieldKind::Line, two, json!({ "x": ["p", "q"], "series": { "a": [1, 2], "b": [2, 1] } }));
    let t = j4n_tui::headless::Headless::new(40, 8).tones(&mut b);
    assert!(t[0].contains('c') && t[0].contains('?'), "two are accent and run: {t:?}");
}

#[test]
fn the_kinds_are_drawn_here_and_are_not_values() {
    assert!(display::shows(FieldKind::Line) && display::shows(FieldKind::Bars));
    assert!(!FieldKind::Line.is_value() && !FieldKind::Bars.is_value());
}

#[test]
fn a_chart_over_a_list_leaves_the_keys_with_the_list() {
    let leaf = |id: &str, kind, bind: &str| Node { id: id.into(), field: Some(Field { kind, bind: bind.into(), props: json!({ "series": [{ "name": "a" }] }) }), ..Node::default() };
    let root = Node { id: "root".into(), present: Present::Stack, children: vec![leaf("c", FieldKind::Line, "c:f"), leaf("l", FieldKind::List, "l:f")], ..Node::default() };
    let mut a = App::new();
    a.start();
    a.down(hello());
    a.down(Down::Tree { node: root });
    assert_eq!(a.page().map(|n| n.id.as_str()), Some("l"));
}

#[test]
fn a_line_with_one_point_is_a_dot_and_its_value_with_no_range_made_up_around_it() {
    // a single point is drawn as a dot at its value
    let mut a = chart(FieldKind::Line, json!({ "series": [{ "name": "usd", "unit": "usd" }] }), json!({ "x": ["today"], "series": { "usd": [0.12] } }));
    let s = lines(&mut a, 40, 8);
    assert!(s[0].contains("spend") && s[1].contains('•') && s[1].contains("$0.12"), "{s:?}");
    assert!(s.iter().all(|l| !l.contains("$1.1") && !l.contains('│') && braille(std::slice::from_ref(l)) == 0), "no axis, no range, no braille: {s:?}");
    assert_eq!(plot::rows(&Field { kind: FieldKind::Line, bind: "c:f".into(), props: json!({ "series": [{ "name": "usd", "unit": "usd" }] }) }, &json!({ "x": ["today"], "series": { "usd": [0.12] } })), 2, "the names and the dot");
}

#[test]
fn in_mono_or_without_colour_the_second_series_is_dotted_and_says_so_where_the_colour_would() {
    // in `mono` the two series differ by more than grey
    let v = json!({ "x": ["a", "b", "c", "d", "e", "f"], "series": { "a": [1, 2, 3, 4, 5, 6], "b": [6, 5, 4, 3, 2, 1] } });
    let props = json!({ "series": [{ "name": "a" }, { "name": "b" }] });
    let drawn = |settings: Settings| {
        let mut a = chart(FieldKind::Line, props.clone(), v.clone());
        a.theme = Theme::build(&settings, &[]).unwrap();
        lines(&mut a, 40, 8)
    };
    let coloured = drawn(plain());
    assert!(!coloured[0].contains('⠂'), "colour tells them apart: {coloured:?}");
    for mute in [Settings { palette: "mono".into(), ..plain() }, Settings { no_color: true, ..plain() }] {
        let s = drawn(mute);
        assert!(s[0].contains("⠂⠐ b") && s[0].contains("• a"), "the legend: {s:?}");
        let set = |s: &[String]| s[1..].iter().flat_map(|l| l.chars()).filter(|c| ('\u{2801}'..='\u{28ff}').contains(c)).map(|c| (u32::from(c) - 0x2800).count_ones()).sum::<u32>();
        assert!(set(&s) < set(&coloured), "the dotted series has fewer dots than a solid one: {} against {}", set(&s), set(&coloured));
    }
}
