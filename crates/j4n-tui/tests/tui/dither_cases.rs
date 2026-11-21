//! The charts as dithered half blocks (`display/dither.rs`). Where the terminal has truecolor, colour is on, the marks are not ascii, the
//! palette tells series apart and `look.chart` is not `plain`, `line` and `bars` are `▀` and `▄` in the colours of their theme roles; every other
//! look gets `plot.rs`'s output, byte for byte (`plot_cases` and the scenes `plot-*-plain` hold that drawer's own words).

use crate::world::hello;
use j4n_proto::{Down, Field, FieldKind, Json, Node, Present, json};
use j4n_tui::App;
use j4n_tui::headless::Headless;
use j4n_tui::theme::palette::Role;
use j4n_tui::theme::{Settings, Theme};

fn chart(look: Settings, kind: FieldKind, props: Json, value: Json) -> App {
    let field = Field { kind, bind: "c:f".into(), props };
    let node = Node { id: "root".into(), present: Present::Stack, children: vec![Node { id: "f".into(), title: "spend".into(), field: Some(field), ..Node::default() }], ..Node::default() };
    let mut a = App::new();
    a.theme = Theme::build(&look, &[]).expect("a theme");
    a.start();
    a.down(hello());
    a.down(Down::Tree { node });
    a.down(Down::Value { bind: "c:f".into(), value, rev: 1 });
    a
}

fn rows(a: &mut App, cols: u16, rows: u16) -> Vec<String> {
    Headless::new(cols, rows).show(a).lines().map(|l| l.trim_end().to_string()).collect()
}

fn plain() -> Settings {
    Settings { chart: "plain".into(), ..Settings::default() }
}

fn blocks(s: &[String]) -> usize {
    s.iter().flat_map(|l| l.chars()).filter(|c| matches!(c, '▀' | '▄')).count()
}

fn braille(s: &[String]) -> usize {
    s.iter().flat_map(|l| l.chars()).filter(|c| ('\u{2801}'..='\u{28ff}').contains(c)).count()
}

/// The SGR of a foreground colour of a role, as the ANSI golden writes it.
fn fg(t: &Theme, r: Role) -> String {
    let c = t.roles.get(r);
    format!("38;2;{};{};{}", c.0, c.1, c.2)
}

fn two_series() -> (Json, Json) {
    let props = json!({ "series": [{ "name": "usd", "unit": "usd" }, { "name": "tokens", "axis": "right", "role": "run", "unit": "n" }] });
    let value = json!({ "x": ["a", "b", "c", "d", "e", "f"], "series": { "usd": [1, 2, 3, 2, 4, 3], "tokens": [100000, 2000000, 300000, 900000, 1200000, 400000] } });
    (props, value)
}

#[test]
fn a_line_is_half_blocks_in_the_colours_of_its_roles_with_an_axis_of_its_own_on_each_side() {
    let (props, value) = two_series();
    let mut a = chart(Settings::default(), FieldKind::Line, props, value);
    let t = a.theme.clone();
    let s = rows(&mut a, 48, 10);
    assert!(blocks(&s) > 20 && braille(&s) == 0, "half blocks and no braille: {s:?}");
    assert!(s[1].starts_with("  $4│") && s[1].ends_with("│2M"), "the dollars on the left and the tokens on the right, as in plot.rs: {s:?}");
    let ansi = Headless::new(48, 10).snap(&mut a).ansi;
    assert!(ansi.contains(&fg(&t, Role::Accent)) && ansi.contains(&fg(&t, Role::Run)), "the strokes carry the exact colours of `accent` and `run`: {ansi:?}");
    // no named dither tone: every colour of the plot is a role's colour, or the role's over the ground
    assert!(s.iter().all(|l| !l.contains("green") && !l.contains("Tone")));
}

#[test]
fn the_half_blocks_are_cells_whose_colours_come_from_the_theme() {
    use ratatui::style::Color;
    let (props, value) = two_series();
    let mut a = chart(Settings::default(), FieldKind::Line, props, value);
    let t = a.theme.clone();
    let mut h = Headless::new(48, 10);
    let _ = h.show(&mut a);
    let roles: Vec<Color> = [Role::Accent, Role::Run].iter().map(|r| { let c = t.roles.get(*r); Color::Rgb(c.0, c.1, c.2) }).collect();
    let cells: Vec<_> = h.buffer().content().iter().filter(|c| matches!(c.symbol(), "▀" | "▄")).collect();
    assert!(!cells.is_empty());
    assert!(cells.iter().any(|c| roles.contains(&c.fg)), "a stroke is exactly a role's colour");
    assert!(cells.iter().all(|c| matches!(c.fg, Color::Rgb(..))), "every half block carries a truecolor foreground");
}

#[test]
fn vertical_and_horizontal_bars_are_half_blocks_too() {
    let v = json!({ "x": ["a", "b", "c"], "series": { "plan": [4, 2, 3], "list": [4, 1, 2] } });
    let props = json!({ "series": [{ "name": "plan", "unit": "n" }, { "name": "list", "role": "run", "unit": "n" }] });
    let mut a = chart(Settings::default(), FieldKind::Bars, props, v);
    let t = a.theme.clone();
    let s = rows(&mut a, 40, 8);
    assert!(blocks(&s) > 10 && !s.iter().any(|l| l.contains('█')), "{s:?}");
    assert!(s[1].contains("4") && s[7].contains('a'), "the labels are J4n's: {s:?}");
    assert!(Headless::new(40, 8).snap(&mut a).ansi.contains(&fg(&t, Role::Accent)));
    // `tasks-lanes`: bars across, as the Tasks page draws them
    let v = json!({ "x": ["l1", "l2"], "series": { "n": [3, 6] } });
    let mut b = chart(Settings::default(), FieldKind::Bars, json!({ "orient": "horizontal", "rows": 6, "series": [{ "name": "n", "unit": "n" }] }), v);
    let h = rows(&mut b, 30, 6);
    assert!(h[1].starts_with("  l1 ") && h[1].ends_with(" 3") && h[2].starts_with("  l2 ") && h[2].ends_with(" 6"), "{h:?}");
    let len = |l: &String| l.chars().filter(|c| matches!(c, '▀' | '▄')).count();
    assert!(len(&h[2]) > len(&h[1]) && len(&h[1]) > 0 && h.iter().all(|l| !l.contains('█')), "the longer bar has more blocks: {h:?}");
}

#[test]
fn plain_no_colour_the_ascii_marks_mono_and_a_terminal_without_truecolor_all_get_the_old_drawer_byte_for_byte() {
    let (props, value) = two_series();
    let draw = |look: Settings| {
        let mut a = chart(look, FieldKind::Line, props.clone(), value.clone());
        let mut h = Headless::new(48, 10);
        (h.show(&mut a), h.snap(&mut a).ansi)
    };
    let (want, want_ansi) = draw(plain());
    assert!(braille(&want.lines().map(String::from).collect::<Vec<_>>()) > 0 && !want.contains('▀'), "plain is braille: {want}");
    assert_eq!(draw(Settings { truecolor: false, ..Settings::default() }), (want.clone(), want_ansi), "a terminal that did not say truecolor");
    // without colour, with the ascii marks and in mono the old drawer has its own words for them: the dither drawer is not used
    for look in [Settings { no_color: true, ..Settings::default() }, Settings { marks: "ascii".into(), ..Settings::default() }, Settings { palette: "mono".into(), ..Settings::default() }] {
        let (text, _) = draw(look.clone());
        let (old, _) = draw(Settings { chart: "plain".into(), ..look });
        assert_eq!(text, old);
        assert!(!text.contains('▀') && !text.contains('▄'), "{text}");
    }
    // `stream` and `wave` are asked for by name and are box lines
    let mut s = chart(Settings::default(), FieldKind::Line, json!({ "style": "stream", "series": [{ "name": "n" }] }), json!({ "x": ["a", "b", "c", "d"], "series": { "n": [0, 10, 10, 0] } }));
    let text = rows(&mut s, 24, 8).join("\n");
    assert!(text.contains('╭') && !text.contains('▀'), "{text}");
}

#[test]
fn a_log_scale_keeps_the_order_of_its_points_and_says_powers_of_ten() {
    let v = json!({ "x": ["a", "b", "c"], "series": { "ms": [10, 1000, 100] } });
    let mut a = chart(Settings::default(), FieldKind::Line, json!({ "scale": "log", "series": [{ "name": "ms" }] }), v);
    let s = rows(&mut a, 40, 8);
    assert!(s[1].contains("1k") && s[6].contains("10"), "labels on powers of ten, as plot.rs has them: {s:?}");
    let axis = s[1].chars().position(|c| c == '│').expect("the axis");
    let (off, pw) = (axis + 1, 40 - axis - 1);
    let top = |i: usize| {
        let col = off + (i * (pw - 1)).div_ceil(2);
        s[1..7].iter().position(|l| l.chars().nth(col).is_some_and(|c| matches!(c, '▀' | '▄'))).expect("a mark in the column")
    };
    assert!(top(1) < top(2) && top(2) < top(0), "1000 above 100 above 10: {} {} {}\n{s:?}", top(1), top(2), top(0));
}

#[test]
fn a_value_not_above_zero_on_a_log_axis_breaks_the_line_as_it_always_did() {
    let v = json!({ "x": ["a", "b", "c", "d", "e"], "series": { "ms": [10, 100, -1, 1000, 100] } });
    let mut a = chart(Settings::default(), FieldKind::Line, json!({ "scale": "log", "series": [{ "name": "ms" }] }), v);
    let s = rows(&mut a, 40, 10);
    let axis = s[1].chars().position(|c| c == '│').expect("the axis");
    let pw = 40 - axis - 1;
    let col = axis + 1 + (2 * (pw - 1) + 2) / 4;
    assert!(s[1..9].iter().all(|l| l.chars().nth(col).is_none_or(|c| c == ' ')), "nothing is drawn where the value is -1: {s:?}");
}

#[test]
fn the_window_is_the_last_points_and_the_axis_is_fitted_to_them() {
    let v = json!({ "x": ["a", "b", "c", "d", "e", "f"], "series": { "n": [900, 5, 6, 7, 8, 9] } });
    let mut a = chart(Settings::default(), FieldKind::Line, json!({ "window": 3, "series": [{ "name": "n" }] }), v.clone());
    let s = rows(&mut a, 40, 8);
    assert!(s[1].contains('9') && !s.join("").contains("900") && s[7].trim().starts_with('d') && s[7].ends_with('f'), "{s:?}");
    let mut b = chart(Settings::default(), FieldKind::Line, json!({ "series": [{ "name": "n" }] }), v);
    let t = rows(&mut b, 40, 8);
    assert!(t[1].contains("900") && t[7].trim().starts_with('a'), "{t:?}");
}

#[test]
fn the_look_the_core_sends_chooses_the_drawer_and_a_terminal_with_no_truecolor_keeps_plot() {
    let (props, value) = two_series();
    let mut a = chart(Settings::default(), FieldKind::Line, props, value);
    let dithered = rows(&mut a, 48, 10);
    assert!(blocks(&dithered) > 0);
    let look = |chart: &str| Down::Theme { look: j4n_proto::Look { palette: "jjj".into(), marks: "icons".into(), frame: "round".into(), corners: "round".into(), title_place: "inside".into(), density: "normal".into(), fill: "match".into(), you: "bracket".into(), motion: true, mouse: "on".into(), read: Default::default(), time: "ago".into(), chart: chart.into() } };
    a.down(look("plain"));
    let plain = rows(&mut a, 48, 10);
    assert!(braille(&plain) > 0 && blocks(&plain) == 0, "{plain:?}");
    a.down(look("dither"));
    assert_eq!(rows(&mut a, 48, 10), dithered, "and back, live");
    a.theme = Theme::build(&Settings::default().with_colorterm(None), &[]).unwrap();
    a.down(look("dither"));
    assert_eq!(rows(&mut a, 48, 10), plain, "no COLORTERM, no half blocks");
}
