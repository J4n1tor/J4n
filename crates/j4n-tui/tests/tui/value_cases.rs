//! The value fields. What each type of value shows, what a key does to it at rest and while it is edited, what it sends, and
//! what it says is wrong with a value. The sheets of each type in each state are the goldens of the scenes `value-*`.

use crate::world::{banded, hello, screen, show};
use j4n_proto::{Act, Cell, Down, Field, FieldKind, Intent, Json, Node, Present, Row, Tone, Up};
use j4n_tui::App;
use j4n_tui::fields::{Spec, draft, parse, problem};
use crossterm::event::{KeyEvent, KeyModifiers};
use j4n_tui::headless::{KeyCode, ctrl, press, typed};
use j4n_proto::json;

fn field(id: &str, label: &str, kind: FieldKind, props: Json) -> Node {
    Node { id: id.into(), title: label.into(), field: Some(Field { kind, bind: format!("cfg:x.{id}"), props }), ..Node::default() }
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

/// An app with one value field called `f` of this kind, holding this value.
fn one(kind: FieldKind, props: Json, value: Json) -> App {
    let mut a = started(stack(vec![field("f", "f", kind, props)]));
    a.down(Down::Value { bind: "cfg:x.f".into(), value, rev: 1 });
    a
}

fn keys(a: &mut App, text: &str) -> Vec<Up> {
    typed(text, false).into_iter().flat_map(|k| a.key(k)).collect()
}

fn key(a: &mut App, code: KeyCode) -> Vec<Up> {
    a.key(press(code))
}

/// What was set: the field and the value of each `Set`.
fn sets(ups: &[Up]) -> Vec<(String, String, String, Json)> {
    ups.iter()
        .filter_map(|u| if let Up::Intent { intent: Intent::Set { kind, id, field, value } } = u { Some((kind.clone(), id.clone(), field.clone(), value.clone())) } else { None })
        .collect()
}

fn set_to(ups: &[Up]) -> Vec<Json> {
    sets(ups).into_iter().map(|s| s.3).collect()
}

fn spec(kind: FieldKind, props: &Json) -> Field {
    Field { kind, bind: "cfg:x.f".into(), props: props.clone() }
}

fn bad(kind: FieldKind, props: Json, value: Json) -> Option<String> {
    let f = spec(kind, &props);
    problem(&Spec::new("f", &f), &value)
}

// ---- reading what is typed ----

#[test]
fn a_duration_is_numbers_with_units_and_words_say_it_back() {
    assert_eq!(parse::dur("90s"), Some(90));
    assert_eq!(parse::dur("5m"), Some(300));
    assert_eq!(parse::dur("1h30m"), Some(5400));
    assert_eq!(parse::dur("1m 30s"), Some(90));
    assert_eq!(parse::dur("2d"), Some(172_800));
    for no in ["5x", "90", "", "m", "1m5", "s30"] {
        assert_eq!(parse::dur(no), None, "`{no}` is not a duration");
    }
    assert_eq!(parse::dur_words(90), "1m 30s");
    assert_eq!(parse::dur_words(7200), "2h");
    assert_eq!(parse::dur_words(0), "0s");
    assert_eq!(parse::dur_words(86_461), "1d 1m 1s");
    assert_eq!((parse::dur_short(300), parse::dur_short(90), parse::dur_short(7200), parse::dur_short(86_400), parse::dur_short(61)), ("5m".into(), "90s".into(), "2h".into(), "1d".into(), "61s".into()));
}

#[test]
fn dollars_are_read_with_or_without_the_sign_and_shown_with_cents() {
    assert_eq!((parse::usd("$5"), parse::usd("2.5"), parse::usd(" $ 0.25 "), parse::usd("-1")), (Some(5.0), Some(2.5), Some(0.25), Some(-1.0)));
    assert_eq!((parse::usd("five"), parse::usd(""), parse::usd("$")), (None, None, None));
    assert_eq!((parse::money(5.0), parse::money(3.75), parse::money(0.0125), parse::money(-1.0), parse::money(0.0)), ("$5.00".into(), "$3.75".into(), "$0.0125".into(), "-$1.00".into(), "$0.00".into()));
}

#[test]
fn numbers_lose_the_zeros_they_do_not_need() {
    assert_eq!((parse::num(3.0), parse::num(0.7), parse::num(2.50), parse::num(-0.1), parse::num(0.0)), ("3".into(), "0.7".into(), "2.5".into(), "-0.1".into(), "0".into()));
    assert_eq!((parse::range(Some(1.0), Some(8.0)), parse::range(None, Some(8.0)), parse::range(Some(1.0), None), parse::range(None, None)), ("1–8".into(), "≤ 8".into(), "≥ 1".into(), String::new()));
}

#[test]
fn a_glob_and_an_expression_say_where_they_are_not_closed() {
    assert_eq!(parse::glob("src/**/*.rs"), None);
    assert_eq!(parse::glob("[a-"), Some("open [ at 1".into()));
    assert_eq!(parse::glob("x{a,b"), Some("open { at 2".into()));
    assert_eq!(parse::glob("[a-z]{x,y}"), None);
    assert_eq!(parse::glob("\\[a"), None, "an escaped bracket is a letter");
    assert_eq!(parse::expr("danger(cmd).class == 'push'"), None);
    assert_eq!(parse::expr("dangr("), Some("( at 6 not closed".into()));
    assert_eq!(parse::expr("a == 'push"), Some("' at 6 not closed".into()));
    assert_eq!(parse::expr("a)"), Some(") at 2 has no (".into()));
    assert_eq!(parse::expr("(a]"), Some("] at 3 has no [".into()));
    assert_eq!(parse::expr("f(')')"), None, "a bracket in a quote is text");
}

#[test]
fn names_and_holes_are_found_in_text() {
    assert!(parse::name("RUST_LOG") && parse::name("a-b.c") && !parse::name("a b") && !parse::name("") && !parse::name("a=b"));
    assert_eq!(parse::holes("{pat}"), vec!["pat"]);
    assert_eq!(parse::holes("x{a}y{b}"), vec!["a", "b"]);
    assert!(parse::holes("-n").is_empty() && parse::holes("{open").is_empty());
}

// ---- what is wrong with a value ----

#[test]
fn each_type_says_what_is_wrong_with_its_value() {
    let none = Json::Null;
    assert_eq!(bad(FieldKind::Str, json!({ "required": true }), json!("")), Some("needed".into()));
    assert_eq!(bad(FieldKind::Str, json!({ "max": 3 }), json!("abcd")), Some("≤ 3 chars".into()));
    assert_eq!(bad(FieldKind::Str, json!({ "max": 4 }), json!("abcd")), None, "as long as it may be is fine");
    assert_eq!(bad(FieldKind::Str, none.clone(), Json::Null), None, "a value that was never set is not wrong unless it is required");
    assert_eq!(bad(FieldKind::Text, none.clone(), json!("x".repeat(241))), Some("≤ 240 chars".into()));
    assert_eq!(bad(FieldKind::Text, none.clone(), json!("x".repeat(240))), None);
    assert_eq!(bad(FieldKind::Int, json!({ "min": 1, "max": 8 }), json!(12)), Some("1–8".into()));
    assert_eq!(bad(FieldKind::Int, json!({ "max": 8 }), json!(9)), Some("≤ 8".into()));
    assert_eq!(bad(FieldKind::Int, json!({ "max": 8 }), json!(8)), None);
    assert_eq!(bad(FieldKind::Int, json!({ "min": 1 }), json!(0)), Some("≥ 1".into()));
    assert_eq!(bad(FieldKind::Num, json!({ "min": 0, "max": 2 }), json!(2.5)), Some("0–2".into()));
    assert_eq!(bad(FieldKind::Num, json!({ "min": 0, "max": 2 }), json!(2)), None);
    assert_eq!(bad(FieldKind::Enum, json!({ "options": ["a", "b"] }), json!("c")), Some("not offered".into()));
    assert_eq!(bad(FieldKind::Enum, json!({ "options": ["a", "b"] }), json!("b")), None);
    assert_eq!(bad(FieldKind::Enum, none.clone(), json!("whatever")), None, "with no options given, any value stands");
    assert_eq!(bad(FieldKind::Multi, json!({ "options": ["a", "b"], "min": 1 }), json!([])), Some("pick one or more".into()));
    assert_eq!(bad(FieldKind::Multi, json!({ "options": ["a", "b", "c"], "min": 2 }), json!(["a"])), Some("pick at least 2".into()));
    assert_eq!(bad(FieldKind::Multi, json!({ "options": ["a", "b"] }), json!(["a", "z"])), Some("not offered".into()));
    assert_eq!(bad(FieldKind::Multi, json!({ "options": ["a", "b"], "min": 1 }), json!(["a"])), None);
    assert_eq!(bad(FieldKind::Ref, json!({ "options": ["omp"] }), json!("codex")), Some("not found".into()));
    assert_eq!(bad(FieldKind::Ref, json!({ "options": ["omp"], "error": "not connected" }), json!("codex")), Some("not connected".into()), "the core's words come first");
    assert_eq!(bad(FieldKind::Ref, json!({ "options": ["omp"] }), json!("omp")), None);
    assert_eq!(bad(FieldKind::Ref, json!({ "options": ["omp"] }), json!("")), None, "none picked yet is not wrong");
    assert_eq!(bad(FieldKind::Seq, json!({ "min": 1 }), json!([])), Some("at least 1".into()));
    assert_eq!(bad(FieldKind::Seq, json!({ "min": 2 }), json!(["a"])), Some("at least 2".into()));
    assert_eq!(bad(FieldKind::Map, none.clone(), json!({ "a b": "1" })), Some("bad name \"a b\"".into()));
    assert_eq!(bad(FieldKind::Map, none.clone(), json!({ "A_b-1.x": "1" })), None);
    assert_eq!(bad(FieldKind::Dur, none.clone(), json!("5x")), Some("like 90s · 5m · 1h".into()));
    assert_eq!(bad(FieldKind::Dur, none.clone(), json!("5m")), None);
    assert_eq!(bad(FieldKind::Dur, none.clone(), json!(300)), None, "a duration may arrive as seconds");
    assert_eq!(bad(FieldKind::Usd, none.clone(), json!(-1.0)), Some("≥ $0".into()));
    assert_eq!(bad(FieldKind::Usd, none.clone(), json!("lots")), Some("like $5 or 2.50".into()));
    assert_eq!(bad(FieldKind::Usd, none.clone(), json!(0)), None);
    assert_eq!(bad(FieldKind::Pattern, none.clone(), json!("[a-")), Some("open [ at 1".into()));
    assert_eq!(bad(FieldKind::Expr, none.clone(), json!("f(")), Some("( at 2 not closed".into()));
    assert_eq!(bad(FieldKind::Argv, json!({ "args": ["pat"] }), json!(["rg", "{pt}"])), Some("no arg \"pt\"".into()));
    assert_eq!(bad(FieldKind::Argv, json!({ "args": ["pat"] }), json!(["rg", "{pat}"])), None);
    assert_eq!(bad(FieldKind::Argv, none.clone(), json!(["rg", "{anything}"])), None, "with no arguments given, any hole stands");
    assert_eq!(bad(FieldKind::Markdown, json!({ "required": true }), json!("")), Some("empty".into()));
    assert_eq!(bad(FieldKind::Path, json!({ "required": true }), json!("")), Some("needed".into()));
    for kind in [FieldKind::Bool, FieldKind::Secret, FieldKind::Str, FieldKind::Int] {
        assert_eq!(bad(kind, json!({ "error": "the core says no" }), json!(1)), Some("the core says no".into()), "{kind:?}: whatever the core says is shown");
    }
    assert_eq!(bad(FieldKind::Str, json!({ "error": "" }), json!("x")), None, "an empty message is none");
}

#[test]
fn a_draft_is_read_as_a_value_of_its_kind_or_refused() {
    let d = |kind, props: Json, text: &str| {
        let f = spec(kind, &props);
        draft(&Spec::new("f", &f), text, &[])
    };
    assert_eq!(d(FieldKind::Str, json!({}), "  claude "), Ok(json!("claude")), "the edges are trimmed");
    assert_eq!(d(FieldKind::Text, json!({}), "a\nb"), Ok(json!("a\nb")), "a text keeps its lines");
    assert_eq!(d(FieldKind::Int, json!({}), " 5 "), Ok(json!(5)));
    assert_eq!(d(FieldKind::Int, json!({}), "5.5"), Err("a whole number".into()));
    assert_eq!(d(FieldKind::Int, json!({}), ""), Err("a whole number".into()));
    assert_eq!(d(FieldKind::Num, json!({}), "0.4"), Ok(json!(0.4)));
    assert_eq!(d(FieldKind::Num, json!({}), "abc"), Err("a number".into()));
    assert_eq!(d(FieldKind::Num, json!({}), "NaN"), Err("a number".into()));
    assert_eq!(d(FieldKind::Dur, json!({}), "90s"), Ok(json!("90s")));
    assert_eq!(d(FieldKind::Usd, json!({}), "$2.5"), Ok(json!(2.5)));
    assert_eq!(d(FieldKind::Usd, json!({}), "-2"), Err("≥ $0".into()));
    assert_eq!(d(FieldKind::Secret, json!({}), ""), Err("nothing typed".into()));
    assert_eq!(d(FieldKind::Secret, json!({}), " x "), Ok(json!(true)), "what a key would be is never made into a value");
    assert_eq!(d(FieldKind::Secret, json!({}), "  "), Err("nothing typed".into()));
    assert_eq!(d(FieldKind::Ref, json!({ "options": ["omp", "pi"] }), "pi"), Ok(json!("pi")));
    assert_eq!(d(FieldKind::Ref, json!({ "options": ["omp", "pi"] }), "p"), Err("not found".into()));
    assert_eq!(d(FieldKind::Ref, json!({}), "anything"), Ok(json!("anything")), "with no entries given, what is typed stands");
    assert_eq!(d(FieldKind::Bool, json!({}), "x"), Err("not a typed value".into()));
}

// ---- keys at rest ----

#[test]
fn a_string_is_opened_typed_in_and_kept_with_enter() {
    let mut a = one(FieldKind::Str, json!({ "max": 8 }), json!("claude"));
    assert!(key(&mut a, KeyCode::Enter).is_empty());
    let shown = screen(&mut a, 64, 6);
    assert!(shown[0].contains("claude▏") && shown[1].contains("esc"), "being edited, with the caret and the keys: {shown:?}");
    assert!(keys(&mut a, "-2").is_empty(), "typing sends nothing");
    let sent = key(&mut a, KeyCode::Enter);
    assert_eq!(sets(&sent), vec![("cfg".into(), "x".into(), "f".into(), json!("claude-2"))], "the bind is `kind:id.field`");
    assert_eq!(a.value("cfg:x.f"), &json!("claude-2"), "the new value stands at once");
    assert!(show(&mut a, 64, 6).lines().next().is_some_and(|l| l.starts_with("╰ f") && !l.contains('▏')), "the edit is over");
}

#[test]
fn escape_gives_up_the_edit_and_keeps_the_value() {
    let mut a = one(FieldKind::Str, json!({}), json!("claude"));
    key(&mut a, KeyCode::Enter);
    keys(&mut a, "zzz");
    assert!(key(&mut a, KeyCode::Esc).is_empty());
    assert_eq!(a.value("cfg:x.f"), &json!("claude"));
    assert!(!show(&mut a, 64, 6).contains("zzz"));
    assert!(key(&mut a, KeyCode::Enter).is_empty() && screen(&mut a, 64, 6)[0].contains("claude▏"), "opened again, it starts from the value");
}

#[test]
fn keeping_the_same_value_sends_nothing() {
    let mut a = one(FieldKind::Str, json!({}), json!("claude"));
    key(&mut a, KeyCode::Enter);
    assert!(key(&mut a, KeyCode::Enter).is_empty());
    let mut a = one(FieldKind::Int, json!({}), json!(3));
    key(&mut a, KeyCode::Enter);
    assert!(key(&mut a, KeyCode::Enter).is_empty(), "3 kept is not a change");
}

#[test]
fn a_draft_that_is_no_good_stays_open_with_the_reason_and_sends_nothing() {
    let mut a = one(FieldKind::Dur, json!({}), json!("5m"));
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Backspace);
    keys(&mut a, "x");
    assert!(key(&mut a, KeyCode::Enter).is_empty(), "5x is not sent");
    let shown = screen(&mut a, 64, 6);
    assert!(shown[0].contains("5x▏") && shown[1].contains("like 90s · 5m · 1h"), "still open, and why: {shown:?}");
    key(&mut a, KeyCode::Backspace);
    keys(&mut a, "h");
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!("5h")]);
}

#[test]
fn the_cursor_moves_inside_the_text() {
    let mut a = one(FieldKind::Str, json!({}), json!("abc"));
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Left);
    key(&mut a, KeyCode::Left);
    keys(&mut a, "X");
    assert!(screen(&mut a, 64, 6)[0].contains("aX▏bc"), "typed where the caret is");
    key(&mut a, KeyCode::Home);
    key(&mut a, KeyCode::Delete);
    key(&mut a, KeyCode::End);
    keys(&mut a, "!");
    key(&mut a, KeyCode::Backspace);
    key(&mut a, KeyCode::Backspace);
    a.key(ctrl('a'));
    keys(&mut a, "_");
    a.key(ctrl('e'));
    keys(&mut a, "?");
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!("_Xb?")]);
    // deleting past either end does nothing
    let mut a = one(FieldKind::Str, json!({}), json!("a"));
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Delete);
    key(&mut a, KeyCode::Right);
    key(&mut a, KeyCode::Backspace);
    key(&mut a, KeyCode::Backspace);
    key(&mut a, KeyCode::Left);
    assert!(screen(&mut a, 64, 6)[0].contains("▏"));
    a.key(ctrl('u'));
    keys(&mut a, "z");
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!("z")], "ctrl-u empties the text");
}

#[test]
fn a_paste_goes_into_the_text_and_keeps_its_lines_only_where_the_field_has_lines() {
    let mut a = one(FieldKind::Str, json!({}), json!(""));
    key(&mut a, KeyCode::Enter);
    a.paste("one\ntwo\tthree");
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!("onetwothree")], "a line break is not a character of a string");
    let mut a = one(FieldKind::Text, json!({}), json!(""));
    key(&mut a, KeyCode::Enter);
    a.paste("one\r\ntwo");
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!("one\ntwo")], "a text has its lines");
    let mut a = one(FieldKind::Str, json!({}), json!("x"));
    a.paste("ignored");
    assert!(sets(&key(&mut a, KeyCode::Enter)).is_empty(), "a paste at rest goes nowhere");
}

#[test]
fn a_text_takes_lines_with_ctrl_j_or_shift_enter_and_keeps_with_enter() {
    let mut a = one(FieldKind::Text, json!({ "rows": 3 }), json!("a"));
    key(&mut a, KeyCode::Enter);
    a.key(ctrl('j'));
    keys(&mut a, "b");
    a.key(j4n_tui::headless::KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::SHIFT));
    keys(&mut a, "c");
    assert_eq!(screen(&mut a, 64, 8)[1..4], ["  a".to_string(), "  b".to_string(), "  c▏".to_string()].map(|s| s.trim_end().to_string()), "three rows, hanging from a blank under the last field");
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!("a\nb\nc")]);
}

#[test]
fn a_markdown_takes_enter_as_a_line_and_ctrl_s_as_keeping() {
    let mut a = one(FieldKind::Markdown, json!({ "rows": 2 }), json!("# Title\nbody"));
    key(&mut a, KeyCode::Enter);
    assert!(screen(&mut a, 64, 8)[1].contains("▏# Title"), "it opens at the top");
    key(&mut a, KeyCode::Enter);
    assert!(key(&mut a, KeyCode::Enter).is_empty(), "Enter is a line break here");
    assert_eq!(set_to(&a.key(ctrl('s'))), vec![json!("\n\n# Title\nbody")]);
    let mut a = one(FieldKind::Markdown, json!({}), json!("x"));
    key(&mut a, KeyCode::Enter);
    keys(&mut a, "y");
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.value("cfg:x.f"), &json!("x"), "escape gives it up");
}

#[test]
fn a_number_starts_at_a_digit_and_steps_with_the_arrows() {
    let mut a = one(FieldKind::Int, json!({ "min": 1, "max": 8 }), json!(3));
    assert!(keys(&mut a, "5").is_empty());
    assert!(screen(&mut a, 64, 4)[0].contains("5▏"), "a digit starts a new number");
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!(5)]);
    let mut a = one(FieldKind::Int, json!({ "min": 1, "max": 8 }), json!(3));
    key(&mut a, KeyCode::Right);
    key(&mut a, KeyCode::Right);
    assert!(screen(&mut a, 64, 4)[0].contains("5▏"), "the arrow opens it and steps");
    for _ in 0..9 {
        key(&mut a, KeyCode::Right);
    }
    assert!(screen(&mut a, 64, 4)[0].contains("8▏"), "held to the most");
    for _ in 0..9 {
        key(&mut a, KeyCode::Left);
    }
    assert!(screen(&mut a, 64, 4)[0].contains("1▏"), "held to the least");
    let mut a = one(FieldKind::Int, json!({ "max": 8 }), json!(3));
    keys(&mut a, "-");
    keys(&mut a, "x7.");
    assert!(screen(&mut a, 64, 4)[0].contains("-7▏"), "an integer takes digits and a minus: {:?}", screen(&mut a, 64, 4));
    let mut a = one(FieldKind::Int, json!({ "max": 8 }), json!(3));
    keys(&mut a, "12");
    assert!(key(&mut a, KeyCode::Enter).is_empty() && screen(&mut a, 64, 4)[1].contains("≤ 8"), "12 is over the most and is not sent");
}

#[test]
fn a_number_with_decimals_steps_by_its_step_without_drift() {
    let mut a = one(FieldKind::Num, json!({ "min": 0, "max": 2, "step": 0.1 }), json!(0.7));
    for _ in 0..3 {
        key(&mut a, KeyCode::Left);
    }
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!(0.4)]);
    let mut a = one(FieldKind::Num, json!({ "step": 0.25 }), json!(1));
    key(&mut a, KeyCode::Right);
    key(&mut a, KeyCode::Right);
    key(&mut a, KeyCode::Right);
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!(1.75)]);
    let mut a = one(FieldKind::Num, json!({}), Json::Null);
    key(&mut a, KeyCode::Right);
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!(1.0)], "from nothing, from 0, by 1");
    let mut a = one(FieldKind::Num, json!({}), json!(1));
    keys(&mut a, "-.5x");
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!(-0.5)]);
}

#[test]
fn a_switch_flips_with_space_and_is_sent_at_once() {
    let mut a = one(FieldKind::Bool, json!({}), json!(true));
    let sent = keys(&mut a, " ");
    assert_eq!(set_to(&sent), vec![json!(false)]);
    assert_eq!(set_to(&keys(&mut a, " ")), vec![json!(true)]);
    let mut a = one(FieldKind::Bool, json!({}), Json::Null);
    assert_eq!(set_to(&keys(&mut a, " ")), vec![json!(true)], "a switch never set is off");
}

#[test]
fn enter_on_a_switch_holds_the_change_until_it_is_kept() {
    let mut a = one(FieldKind::Bool, json!({}), json!(true));
    assert!(key(&mut a, KeyCode::Enter).is_empty());
    assert!(screen(&mut a, 64, 4)[1].contains("space flips"), "pending, flipped");
    assert!(keys(&mut a, "  ").is_empty(), "flipping the pending value sends nothing");
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!(false)], "two flips of the flipped value is the flipped value");
    let mut a = one(FieldKind::Bool, json!({}), json!(true));
    key(&mut a, KeyCode::Enter);
    keys(&mut a, " ");
    assert!(key(&mut a, KeyCode::Enter).is_empty(), "flipped back to what it was is no change");
    let mut a = one(FieldKind::Bool, json!({}), json!(true));
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.value("cfg:x.f"), &json!(true));
    key(&mut a, KeyCode::Enter);
    assert!(keys(&mut a, "x").is_empty(), "other keys do nothing to it");
    assert!(screen(&mut a, 64, 4)[1].contains("space flips"));
}

#[test]
fn a_choice_moves_with_the_arrows_and_is_kept_with_enter() {
    for (style, key_next) in [("radio", KeyCode::Down), ("segmented", KeyCode::Right), ("dropdown", KeyCode::Down)] {
        let mut a = one(FieldKind::Enum, json!({ "options": ["a", "b", "c"], "style": style }), json!("a"));
        key(&mut a, KeyCode::Enter);
        key(&mut a, key_next);
        key(&mut a, key_next);
        key(&mut a, key_next);
        assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!("c")], "{style}: held to the last option");
        key(&mut a, KeyCode::Enter);
        key(&mut a, KeyCode::Up);
        assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!("b")], "{style}: up goes back one");
    }
    let mut a = one(FieldKind::Enum, json!({ "options": ["a", "b"] }), json!("b"));
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Left);
    key(&mut a, KeyCode::Left);
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!("a")]);
    let mut a = one(FieldKind::Enum, json!({ "options": ["a", "b"] }), json!("a"));
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Right);
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.value("cfg:x.f"), &json!("a"));
    let mut a = one(FieldKind::Enum, json!({ "options": ["a", "b"] }), json!("a"));
    key(&mut a, KeyCode::Enter);
    assert!(key(&mut a, KeyCode::Enter).is_empty(), "keeping the one it has is no change");
}

#[test]
fn an_arrow_at_rest_opens_a_choice_one_step_on() {
    let mut a = one(FieldKind::Enum, json!({ "options": ["a", "b", "c"] }), json!("a"));
    key(&mut a, KeyCode::Right);
    assert!(screen(&mut a, 64, 4)[0].contains(" b "), "the segment after the value is the one chosen");
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!("b")]);
    let mut a = one(FieldKind::Enum, json!({ "options": ["a", "b", "c"] }), json!("a"));
    key(&mut a, KeyCode::Left);
    assert!(key(&mut a, KeyCode::Enter).is_empty(), "left of the first is the first");
}

#[test]
fn the_style_of_a_choice_follows_how_many_options_there_are() {
    let style = |n: usize, p: Json| {
        let opts: Vec<String> = (0..n).map(|i| format!("o{i}")).collect();
        let mut props = json!({ "options": opts });
        if let (Some(m), Some(extra)) = (props.as_object_mut(), p.as_object()) {
            m.extend(extra.clone());
        }
        let f = spec(FieldKind::Enum, &props);
        Spec::new("f", &f).style()
    };
    use j4n_tui::fields::Style::{Dropdown, Radio, Segmented};
    assert_eq!((style(0, json!({})), style(3, json!({})), style(4, json!({})), style(7, json!({})), style(8, json!({}))), (Segmented, Segmented, Radio, Radio, Dropdown));
    assert_eq!((style(3, json!({ "style": "radio" })), style(3, json!({ "style": "dropdown" })), style(9, json!({ "style": "segmented" })), style(3, json!({ "style": "nonsense" }))), (Radio, Dropdown, Segmented, Segmented));
}

#[test]
fn several_are_ticked_with_space_and_each_tick_is_sent_in_the_order_of_the_options() {
    let mut a = one(FieldKind::Multi, json!({ "options": ["read", "write", "net", "exec"] }), json!(["write"]));
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Right);
    assert_eq!(set_to(&keys(&mut a, " ")), vec![json!([])], "`write` unticked");
    assert_eq!(set_to(&keys(&mut a, " ")), vec![json!(["write"])], "and ticked again");
    key(&mut a, KeyCode::Left);
    assert_eq!(set_to(&keys(&mut a, " ")), vec![json!(["read", "write"])], "in the order the options have, not the order they were ticked");
    key(&mut a, KeyCode::End);
    key(&mut a, KeyCode::Right);
    key(&mut a, KeyCode::Right);
    key(&mut a, KeyCode::Right);
    key(&mut a, KeyCode::Right);
    assert_eq!(set_to(&keys(&mut a, " ")), vec![json!(["read", "write", "exec"])], "held to the last option");
    assert!(key(&mut a, KeyCode::Enter).is_empty());
    assert!(show(&mut a, 64, 6).lines().next().is_some_and(|l| !l.contains('▏')));
}

#[test]
fn the_options_to_tick_are_in_columns_that_fit_and_the_arrows_go_by_row() {
    let opts: Vec<String> = (0..6).map(|i| format!("opt{i}")).collect();
    let mut a = one(FieldKind::Multi, json!({ "options": opts }), json!([]));
    a.size = (30, 10);
    key(&mut a, KeyCode::Enter);
    // 30 columns: a cell is the name (4) and a mark and two blanks (4)... three to a row
    key(&mut a, KeyCode::Down);
    assert_eq!(set_to(&keys(&mut a, " ")), vec![json!(["opt3"])], "down is a row of three");
    key(&mut a, KeyCode::Up);
    assert_eq!(set_to(&keys(&mut a, " ")), vec![json!(["opt0", "opt3"])], "up is back");
    key(&mut a, KeyCode::Up);
    assert_eq!(set_to(&keys(&mut a, " ")), vec![json!(["opt3"])], "and no further up than the first row");
}

#[test]
fn a_ref_is_picked_by_typing_part_of_a_name() {
    let props = json!({ "options": ["claude", "omp", "pi", "clef"] });
    let mut a = one(FieldKind::Ref, props.clone(), json!("omp"));
    key(&mut a, KeyCode::Enter);
    keys(&mut a, "cl");
    let shown = screen(&mut a, 64, 8);
    assert!(shown[1].contains("clef") && shown[2].contains("claude"), "the entries the letters are in, the better first: {shown:?}");
    key(&mut a, KeyCode::Down);
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!("claude")], "the one the cursor is on");
    let mut a = one(FieldKind::Ref, props.clone(), json!("omp"));
    key(&mut a, KeyCode::Enter);
    keys(&mut a, "zz");
    assert!(key(&mut a, KeyCode::Enter).is_empty(), "nothing matches, nothing is picked");
    assert!(screen(&mut a, 64, 8)[1].contains("not found"));
    key(&mut a, KeyCode::Backspace);
    key(&mut a, KeyCode::Backspace);
    keys(&mut a, "pi");
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!("pi")]);
    let mut a = one(FieldKind::Ref, props, json!("pi"));
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Up);
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Down);
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!("clef")], "held to the last entry; with nothing typed they are all there, in order");
}

#[test]
fn a_path_is_completed_with_tab_and_tab_again_steps_through_the_names_it_could_be() {
    let props = json!({ "options": ["~/.j4n/packs/", "~/.j4n/palettes/", "~/other/"] });
    let mut a = one(FieldKind::Path, props, json!("~/.j4n/packs"));
    key(&mut a, KeyCode::Enter);
    a.key(ctrl('u'));
    keys(&mut a, "~/.j4n/pa");
    let line = |a: &App| a.val.as_ref().map(|e| e.line.buf.clone()).unwrap_or_default();
    key(&mut a, KeyCode::Tab);
    assert_eq!(line(&a), "~/.j4n/packs/", "⇥ takes the first name");
    key(&mut a, KeyCode::Tab);
    assert_eq!(line(&a), "~/.j4n/palettes/", "⇥ again the next");
    a.key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(line(&a), "~/.j4n/packs/", "⇧⇥ the one before");
    key(&mut a, KeyCode::Esc);
    assert_eq!(line(&a), "~/.j4n/pa", "esc puts back what was typed, and the field is still being edited");
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Tab);
    assert_eq!(line(&a), "~/.j4n/palettes/", "the first ⇥ takes the name the cursor is on, as → does");
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!("~/.j4n/palettes/")]);
}

#[test]
fn a_ref_keeps_the_name_tab_put_in_its_line() {
    let mut a = one(FieldKind::Ref, json!({ "options": ["sonnet", "sonnet-5.5", "opus"] }), json!("opus"));
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Tab);
    assert_eq!(a.val.as_ref().map(|e| e.line.buf.as_str()), Some("opus"), "the first ⇥ takes the one the cursor is on");
    key(&mut a, KeyCode::Tab);
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!("sonnet")], "⏎ keeps what ⇥ put there");
}

#[test]
fn a_path_is_completed_with_right_at_the_end_or_control_right_from_the_names_it_could_be() {
    let props = json!({ "options": ["~/.j4n/packs/", "~/.j4n/palettes/", "~/other/"] });
    let mut a = one(FieldKind::Path, props.clone(), json!("~/.j4n/packs"));
    key(&mut a, KeyCode::Enter);
    a.key(ctrl('u'));
    keys(&mut a, "~/.j4n/pa");
    let shown = screen(&mut a, 64, 6);
    assert!(shown[1].contains("~/.j4n/packs/") && shown[2].contains("~/.j4n/palettes/") && !shown.iter().any(|l| l.contains("~/other/")), "{shown:?}");
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Right);
    assert!(screen(&mut a, 64, 6)[0].contains("~/.j4n/palettes/▏"), "→ at the end takes the name");
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!("~/.j4n/palettes/")]);
    // ⌃→ completes wherever the cursor is
    let mut a = one(FieldKind::Path, props.clone(), json!("~/.j4n/packs"));
    key(&mut a, KeyCode::Enter);
    a.key(ctrl('u'));
    keys(&mut a, "~/.j4n/pa");
    key(&mut a, KeyCode::Left);
    key(&mut a, KeyCode::Right);
    assert!(a.val.as_ref().is_some_and(|e| e.line.buf == "~/.j4n/pa"), "→ before the end only moves");
    key(&mut a, KeyCode::Left);
    a.key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL));
    assert!(screen(&mut a, 64, 6)[0].contains("~/.j4n/packs/▏"), "⌃→ takes it from the middle");
    let mut a = one(FieldKind::Path, props, json!("x"));
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Tab);
    key(&mut a, KeyCode::Right);
    key(&mut a, KeyCode::Up);
    keys(&mut a, "yz");
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!("xyz")], "nothing to complete does nothing; what is typed is what is kept");
}

#[test]
fn a_list_adds_edits_removes_and_moves_its_rows_and_sends_each_change() {
    let mut a = one(FieldKind::Seq, json!({ "min": 1 }), json!(["haiku", "sonnet"]));
    key(&mut a, KeyCode::Enter);
    // to the row that adds, then type
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Enter);
    keys(&mut a, "opus");
    assert!(screen(&mut a, 64, 8).iter().any(|l| l.contains("opus▏")), "typed on a new row");
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!(["haiku", "sonnet", "opus"])]);
    // edit the second
    key(&mut a, KeyCode::Up);
    key(&mut a, KeyCode::Up);
    key(&mut a, KeyCode::Up);
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Enter);
    keys(&mut a, "-2");
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!(["haiku", "sonnet-2", "opus"])]);
    // move it down, then up twice
    assert_eq!(set_to(&a.key(shift(KeyCode::Down))), vec![json!(["haiku", "opus", "sonnet-2"])]);
    assert_eq!(set_to(&a.key(shift(KeyCode::Up))), vec![json!(["haiku", "sonnet-2", "opus"])]);
    assert_eq!(set_to(&a.key(shift(KeyCode::Up))), vec![json!(["sonnet-2", "haiku", "opus"])]);
    assert!(a.key(shift(KeyCode::Up)).is_empty(), "the first goes no higher");
    // remove it
    assert_eq!(set_to(&keys(&mut a, "d")), vec![json!(["haiku", "opus"])]);
    // an item emptied is removed
    key(&mut a, KeyCode::Enter);
    for _ in 0..4 {
        key(&mut a, KeyCode::Backspace);
    }
    key(&mut a, KeyCode::Backspace);
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!(["opus"])]);
    // `a` adds from anywhere; escape gives up the row, and then the edit
    keys(&mut a, "a");
    keys(&mut a, "zzz");
    key(&mut a, KeyCode::Esc);
    assert_eq!(a.value("cfg:x.f"), &json!(["opus"]));
    key(&mut a, KeyCode::Esc);
    assert!(screen(&mut a, 64, 8)[0].starts_with("╰ f"), "closed, and still the one with the keys");
    assert!(banded(&mut a, 64, 8).iter().any(|l| l.starts_with("╰ f")), "the band is what says so");
    assert!(!screen(&mut a, 64, 8).iter().skip(1).any(|l| l.contains('╰')));
}

fn shift(code: KeyCode) -> j4n_tui::headless::KeyEvent {
    j4n_tui::headless::KeyEvent::new(code, crossterm::event::KeyModifiers::SHIFT)
}

#[test]
fn a_list_cannot_remove_the_row_that_adds_or_move_past_its_ends() {
    let mut a = one(FieldKind::Seq, json!({}), json!(["a", "b"]));
    key(&mut a, KeyCode::Enter);
    assert!(keys(&mut a, "d").iter().any(|u| matches!(u, Up::Intent { .. })), "on the first row, `d` removes it");
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Down);
    assert!(keys(&mut a, "d").is_empty(), "on the row that adds there is nothing to remove");
    assert!(a.key(shift(KeyCode::Down)).is_empty(), "nor to move");
    key(&mut a, KeyCode::Up);
    assert!(a.key(shift(KeyCode::Down)).is_empty(), "the last row goes no lower");
    assert!(keys(&mut a, "+x").is_empty(), "plus adds a row too, and typing is not sending");
    assert!(key(&mut a, KeyCode::Enter).iter().any(|u| matches!(u, Up::Intent { .. })));
    // editing keys inside a row
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Left);
    key(&mut a, KeyCode::Right);
    key(&mut a, KeyCode::Delete);
    keys(&mut a, "ab");
    key(&mut a, KeyCode::Left);
    key(&mut a, KeyCode::Delete);
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!(["ba", "x"])], "the caret moves inside a row, and delete takes the letter after it");
}

#[test]
fn a_map_edits_a_value_and_adds_a_pair_typed_as_name_equals_value() {
    let mut a = one(FieldKind::Map, json!({}), json!({ "RUST_LOG": "info" }));
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Backspace);
    key(&mut a, KeyCode::Backspace);
    key(&mut a, KeyCode::Backspace);
    key(&mut a, KeyCode::Backspace);
    keys(&mut a, "debug");
    assert!(screen(&mut a, 64, 8).iter().any(|l| l.contains("RUST_LOG = debug▏")));
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!({ "RUST_LOG": "debug" })]);
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Enter);
    keys(&mut a, "TOKEN = x y");
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!({ "RUST_LOG": "debug", "TOKEN": "x y" })]);
    // a name with a space is no name: the row stays, nothing is sent
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Enter);
    keys(&mut a, "a b=1");
    assert!(key(&mut a, KeyCode::Enter).is_empty());
    assert!(screen(&mut a, 64, 9).iter().any(|l| l.contains("a b=1▏")), "still being typed");
    // a line without `=` adds nothing, and the same name replaces its value
    a.key(ctrl('u'));
    keys(&mut a, "TOKEN=z");
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!({ "RUST_LOG": "debug", "TOKEN": "z" })]);
    keys(&mut a, "a");
    keys(&mut a, "nonsense");
    assert!(sets(&key(&mut a, KeyCode::Enter)).is_empty(), "no name, no pair");
    key(&mut a, KeyCode::Esc);
    // remove
    key(&mut a, KeyCode::Up);
    key(&mut a, KeyCode::Up);
    key(&mut a, KeyCode::Up);
    assert_eq!(set_to(&keys(&mut a, "d")), vec![json!({ "TOKEN": "z" })]);
    assert!(a.key(shift(KeyCode::Down)).is_empty(), "a map has no order to move in");
}

#[test]
fn an_argument_list_is_edited_a_word_at_a_time() {
    let mut a = one(FieldKind::Argv, json!({ "args": ["pat"] }), json!(["rg", "-n"]));
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Char(' '));
    keys(&mut a, "{pat}");
    assert!(screen(&mut a, 64, 6)[0].contains("rg") && screen(&mut a, 64, 6)[0].contains("{pat}▏"));
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!(["rg", "-n", "{pat}"])]);
    // backspace on an empty word takes the one before it back
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Char(' '));
    key(&mut a, KeyCode::Backspace);
    key(&mut a, KeyCode::Backspace);
    keys(&mut a, "x");
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!(["rg", "-n", "{patx"])]);
}

// ---- the other types ----

#[test]
fn a_dollar_amount_says_the_hard_cap_it_makes() {
    let mut a = one(FieldKind::Usd, json!({ "hard": 1.5 }), json!(5.0));
    key(&mut a, KeyCode::Enter);
    a.key(ctrl('u'));
    keys(&mut a, "2.5");
    assert!(screen(&mut a, 64, 4)[1].contains("hard = $3.75"), "{:?}", screen(&mut a, 64, 4));
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!(2.5)]);
    let mut a = one(FieldKind::Usd, json!({}), json!(5.0));
    key(&mut a, KeyCode::Enter);
    a.key(ctrl('u'));
    keys(&mut a, "-1");
    assert!(screen(&mut a, 64, 4)[1].contains("≥ $0"), "a negative amount is refused while it is typed");
    assert!(key(&mut a, KeyCode::Enter).is_empty());
}

#[test]
fn a_duration_says_what_it_comes_to_while_it_is_typed() {
    let mut a = one(FieldKind::Dur, json!({}), json!("5m"));
    key(&mut a, KeyCode::Enter);
    a.key(ctrl('u'));
    keys(&mut a, "90s");
    assert!(screen(&mut a, 64, 4)[1].contains("= 1m 30s"));
    let mut a = one(FieldKind::Dur, json!({}), json!(300));
    key(&mut a, KeyCode::Enter);
    assert!(screen(&mut a, 64, 4)[0].contains("5m▏"), "seconds are edited as a duration");
    assert!(show(&mut one(FieldKind::Dur, json!({}), json!(90)), 64, 3).contains("90s"));
}

#[test]
fn a_pattern_shows_how_many_it_matches_when_the_core_says_and_what_is_open_when_it_is_not_closed() {
    let mut a = one(FieldKind::Pattern, json!({ "matches": 12 }), json!("src/*.rs"));
    key(&mut a, KeyCode::Enter);
    assert!(screen(&mut a, 64, 4)[1].contains("12 match"));
    keys(&mut a, "[");
    assert!(screen(&mut a, 64, 4)[1].contains("open [ at 9"), "{:?}", screen(&mut a, 64, 4));
    assert!(key(&mut a, KeyCode::Enter).is_empty());
    let mut a = one(FieldKind::Pattern, json!({}), json!("a"));
    key(&mut a, KeyCode::Enter);
    assert!(screen(&mut a, 64, 4)[1].contains("esc"), "with no count the keys are shown");
}

#[test]
fn an_expression_is_checked_as_it_is_typed_and_the_core_says_what_it_made_of_it() {
    let mut a = one(FieldKind::Expr, json!({ "check": "3 of 40 last cmds" }), json!("danger(cmd)"));
    key(&mut a, KeyCode::Enter);
    assert!(screen(&mut a, 64, 6).iter().any(|l| l.contains("3 of 40 last cmds")));
    keys(&mut a, " == (");
    assert!(screen(&mut a, 64, 6).iter().any(|l| l.contains("( at 16 not closed")), "{:?}", screen(&mut a, 64, 6));
    assert!(key(&mut a, KeyCode::Enter).is_empty());
    keys(&mut a, ")");
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!("danger(cmd) == ()")]);
}

#[test]
fn a_secret_is_never_shown_and_never_kept_by_the_screen() {
    let mut a = key_app(json!(true));
    assert!(show(&mut a, 64, 3).contains("set"));
    key(&mut a, KeyCode::Enter);
    assert!(!screen(&mut a, 64, 4)[0].contains('▏') || screen(&mut a, 64, 4)[0].trim_end().ends_with('▏'), "it starts empty: the old one is not shown");
    keys(&mut a, "hunter2-typed");
    let shown = show(&mut a, 64, 4);
    assert!(!shown.contains("hunter") && shown.contains("••••••••▏ typed"), "masked: {shown}");
    // a paste is taken whole, in place of what was typed
    a.paste("zz-paste-pasted");
    assert!(show(&mut a, 64, 4).contains("••••••••▏ pasted · 1 line"), "{}", show(&mut a, 64, 4));
    let sent = key(&mut a, KeyCode::Enter);
    assert_eq!(kept(&sent), vec![("own".to_string(), "zz-paste-pasted".to_string())], "the core gets it once, by its own intent");
    assert!(sets(&sent).is_empty(), "never as a Set");
    assert_eq!(a.value("key:own.value"), &json!(true), "the screen keeps only that it is set");
    assert!(!show(&mut a, 64, 4).contains("zz-paste"));
    key(&mut a, KeyCode::Enter);
    assert!(key(&mut a, KeyCode::Enter).is_empty(), "nothing typed is refused");
    assert!(screen(&mut a, 64, 4)[1].contains("nothing typed"));
    let mut a = key_app(Json::Null);
    assert!(show(&mut a, 64, 3).contains("not set"));
    // a secret bound to anything but a provider's key sends nothing at all
    let mut a = one(FieldKind::Secret, json!({}), Json::Null);
    key(&mut a, KeyCode::Enter);
    keys(&mut a, "hunter2-typed");
    assert!(key(&mut a, KeyCode::Enter).is_empty() && a.toast.as_ref().is_some_and(|t| t.0.contains("nothing was sent")), "{:?}", a.toast);
}

/// The key's field as Agents › Providers binds it.
fn key_app(value: Json) -> App {
    let node = Node { id: "f".into(), title: "f".into(), field: Some(Field { kind: FieldKind::Secret, bind: "key:own.value".into(), props: json!({}) }), ..Node::default() };
    let mut a = started(stack(vec![node]));
    a.down(Down::Value { bind: "key:own.value".into(), value, rev: 1 });
    a
}

/// What each `Intent::Key` sent: the id and the key.
fn kept(ups: &[Up]) -> Vec<(String, String)> {
    ups.iter().filter_map(|u| if let Up::Intent { intent: Intent::Key { id, value } } = u { Some((id.clone(), value.expose().to_string())) } else { None }).collect()
}

#[test]
fn a_key_of_seven_and_of_seventy_characters_is_drawn_in_the_same_cells() {
    let mut a = key_app(Json::Null);
    key(&mut a, KeyCode::Enter);
    keys(&mut a, "abcdefg");
    let (short, short_narrow) = (screen(&mut a, 64, 4), screen(&mut a, 40, 6));
    key(&mut a, KeyCode::Esc);
    key(&mut a, KeyCode::Enter);
    keys(&mut a, &"abcdefghij".repeat(7));
    assert_eq!(screen(&mut a, 64, 4), short, "the mask says nothing of the length");
    assert_eq!(screen(&mut a, 40, 6), short_narrow);
    assert!(!short.join("").contains("abc"));
    // the cursor stays at the end: ← and a letter add to the end, as nothing is drawn to move in
    key(&mut a, KeyCode::Left);
    keys(&mut a, "Z");
    let sent = key(&mut a, KeyCode::Enter);
    assert_eq!(kept(&sent), vec![("own".to_string(), format!("{}Z", "abcdefghij".repeat(7)))]);
}

/// Agents › Providers as the core sends it: a row that carries the secret editor, and ⏎ that opens it.
fn providers_app() -> App {
    let list = Node { id: "providers".into(), title: "Providers".into(), field: Some(Field { kind: FieldKind::List, bind: "view:providers".into(), props: Json::Null }), ..Node::default() };
    let pages = Node { id: "pages".into(), present: Present::Tabs, children: vec![list], ..Node::default() };
    let mut a = App::new();
    a.start();
    a.down(hello());
    a.down(Down::Tree { node: Node { id: "root".into(), children: vec![pages], ..Node::default() } });
    let open = Act { key: "enter".into(), label: "type the key".into(), intent: Intent::Set { kind: "key".into(), id: "own".into(), field: "value".into(), value: Json::Null }, risk: 0 };
    let row = |key: &str, data: Option<Json>| Row { key: key.into(), cells: vec![Cell::new(key, Tone::Plain), Cell::new("not set", Tone::Plain)], actions: vec![open.clone()], data, ..Row::default() };
    let rows = vec![row("own", Some(json!({ "edit": { "kind": "secret", "bind": "key:own.value" } }))), row("bare", None), row("plain", Some(json!({ "edit": { "kind": "str", "bind": "key:own.value" } })))];
    crate::world::rows(&mut a, "view:providers", rows);
    a
}

#[test]
fn a_paste_into_a_key_of_two_lines_or_a_blank_is_refused_and_keeps_nothing_and_name_equals_value_keeps_the_value() {
    let mut a = providers_app();
    assert!(key(&mut a, KeyCode::Enter).is_empty() && a.row_ed.is_some(), "⏎ opens the row's secret editor");
    a.paste("sk-test-line-one-4417\nsk-test-line-two-4417\n");
    let shown = show(&mut a, 80, 10);
    assert!(shown.contains("the paste had 2 lines · paste only the key") && !shown.contains("line-one"), "{shown}");
    assert!(key(&mut a, KeyCode::Enter).is_empty(), "nothing of the paste was kept");
    assert!(show(&mut a, 80, 10).contains("nothing typed"));
    a.paste("export OPENROUTER_API_KEY=sk-test-pasted-4417");
    assert!(show(&mut a, 80, 10).contains("the paste had a blank in it"));
    // `NAME=value`: the value, the name said
    a.paste("  OPENROUTER_API_KEY=\"sk-test-pasted-4417\"\r\n");
    let shown = show(&mut a, 100, 10);
    assert!(shown.contains("pasted OPENROUTER_API_KEY=… · the name is left out") && !shown.contains("sk-test"), "{shown}");
    assert_eq!(kept(&key(&mut a, KeyCode::Enter)), vec![("own".to_string(), "sk-test-pasted-4417".to_string())]);
    assert!(a.row_ed.is_none(), "kept: the row is a row again");
    // a key's own `=` is not a name: a padded key is kept whole
    key(&mut a, KeyCode::Enter);
    a.paste("QUJDREVGR0hJSktMTU5PUA==");
    assert_eq!(kept(&key(&mut a, KeyCode::Enter)), vec![("own".to_string(), "QUJDREVGR0hJSktMTU5PUA==".to_string())]);
}

#[test]
fn a_key_row_without_the_secret_editor_opens_no_editor_and_sends_nothing() {
    let mut a = providers_app();
    for _ in 0..2 {
        key(&mut a, KeyCode::Down);
        let ups = key(&mut a, KeyCode::Enter);
        assert!(ups.is_empty() && a.edit.is_none() && a.row_ed.is_none() && a.val.is_none(), "the plain line does not mask: no editor opens");
        assert!(a.toast.as_ref().is_some_and(|t| t.0.contains("masked editor")), "{:?}", a.toast);
    }
}

#[test]
fn the_keys_that_come_with_the_enter_that_kept_a_key_are_dropped() {
    let mut a = providers_app();
    let t0 = std::time::Instant::now();
    let at = |ms: u64| t0 + std::time::Duration::from_millis(ms);
    a.set_clock(at(0));
    key(&mut a, KeyCode::Enter);
    keys(&mut a, "sk-test-typed-out-4417");
    assert_eq!(kept(&key(&mut a, KeyCode::Enter)).len(), 1);
    let before = (show(&mut a, 80, 10), banded(&mut a, 80, 10));
    // a terminal without bracketed paste types the rest of a paste out: five keys, a few ms apart, one of them ⏎
    for (i, k) in [KeyCode::Down, KeyCode::Char('j'), KeyCode::Enter, KeyCode::Char('x'), KeyCode::Down].into_iter().enumerate() {
        a.set_clock(at(5 + 10 * i as u64));
        assert!(key(&mut a, k).is_empty(), "key {i} of the burst is dropped");
    }
    assert_eq!((show(&mut a, 80, 10), banded(&mut a, 80, 10)), before, "no cursor moved, no editor opened");
    assert!(a.row_ed.is_none() && a.val.is_none());
    // a key a hand types after it is a key again
    a.set_clock(at(400));
    key(&mut a, KeyCode::Enter);
    assert!(a.row_ed.is_some(), "⏎ opens the editor again");
}

// ---- the keys go to a field ----

fn form() -> App {
    let kids = vec![
        field("name", "name", FieldKind::Str, json!({})),
        field("locked", "locked", FieldKind::Str, json!({ "disabled": "read only" })),
        field("count", "count", FieldKind::Int, json!({ "max": 9 })),
        field("on", "on", FieldKind::Bool, json!({})),
    ];
    let mut a = started(stack(kids));
    for (id, v) in [("name", json!("claude")), ("locked", json!("x")), ("count", json!(3)), ("on", json!(false))] {
        a.down(Down::Value { bind: format!("cfg:x.{id}"), value: v, rev: 1 });
    }
    a
}

fn at(a: &App) -> String {
    a.page().map(|p| p.id.clone()).unwrap_or_default()
}

#[test]
fn the_arrows_go_from_field_to_field_and_skip_one_that_may_not_be_changed() {
    let mut a = form();
    assert_eq!(at(&a), "name", "the first field has the keys");
    key(&mut a, KeyCode::Down);
    assert_eq!(at(&a), "count", "the locked one is passed over");
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Down);
    assert_eq!(at(&a), "on", "held to the last");
    key(&mut a, KeyCode::Up);
    key(&mut a, KeyCode::Up);
    key(&mut a, KeyCode::Up);
    assert_eq!(at(&a), "name", "and to the first");
    let rows = screen(&mut a, 64, 6);
    assert!(rows[0].starts_with("├ name") && rows[1].starts_with("├ locked") && rows[2].starts_with("├ count") && rows[3].starts_with("╰ on"), "{rows:?}");
    assert!(banded(&mut a, 64, 6).iter().all(|l| l.starts_with("├ name")) && !banded(&mut a, 64, 6).is_empty(), "the band is on the first field only");
    assert!(rows[1].trim_end().ends_with('󰍁'), "the locked one shows it");
}

#[test]
fn a_field_that_may_not_be_changed_takes_no_edit() {
    let mut a = started(stack(vec![field("locked", "locked", FieldKind::Str, json!({ "disabled": true }))]));
    a.down(Down::Value { bind: "cfg:x.locked".into(), value: json!("x"), rev: 1 });
    assert!(key(&mut a, KeyCode::Enter).is_empty());
    keys(&mut a, "abc");
    assert!(!show(&mut a, 64, 4).contains('▏'));
    assert_eq!(at(&a), "root", "no field has the keys: nothing in the tree can be changed");
    // a disabled flag of `false` or an empty reason is not disabled
    for props in [json!({ "disabled": false }), json!({ "disabled": "" }), json!({ "disabled": 0 })] {
        let mut a = one(FieldKind::Str, props, json!("x"));
        key(&mut a, KeyCode::Enter);
        assert!(show(&mut a, 64, 3).contains('▏'));
    }
}

#[test]
fn a_digit_is_a_number_in_a_number_field_and_a_tab_anywhere_else() {
    let tabs = Node {
        id: "root".into(),
        present: Present::Tabs,
        children: vec![
            Node { id: "form".into(), title: "Form".into(), present: Present::Stack, children: vec![field("name", "name", FieldKind::Str, json!({})), field("count", "count", FieldKind::Int, json!({}))], ..Node::default() },
            field("other", "Other", FieldKind::List, Json::Null),
        ],
        ..Node::default()
    };
    let mut a = started(tabs);
    keys(&mut a, "2");
    assert_eq!(at(&a), "other", "on a string a digit goes to the tab");
    keys(&mut a, "1");
    key(&mut a, KeyCode::Down);
    assert_eq!(at(&a), "count");
    keys(&mut a, "2");
    assert_eq!(at(&a), "count", "on a number a digit is typed");
    assert!(show(&mut a, 64, 6).contains("2▏"));
    // while it is being typed even the tab keys are letters of the text
    keys(&mut a, "3");
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!(23)]);
}

#[test]
fn an_edit_that_is_open_when_the_keys_go_elsewhere_is_given_up() {
    let mut a = form();
    key(&mut a, KeyCode::Enter);
    keys(&mut a, "zz");
    a.key(ctrl('k'));
    keys(&mut a, "on");
    key(&mut a, KeyCode::Enter);
    assert_eq!(at(&a), "on", "the jump went to another field");
    key(&mut a, KeyCode::Char('x'));
    assert_eq!(a.value("cfg:x.name"), &json!("claude"));
    assert!(!show(&mut a, 64, 6).contains("zz"), "the half-typed text is gone");
}

#[test]
fn the_value_the_core_sends_replaces_the_one_the_screen_took_and_a_field_asks_for_its_binding() {
    let mut a = started(stack(vec![field("f", "f", FieldKind::Str, json!({}))]));
    let mut a2 = App::new();
    a2.start();
    a2.down(hello());
    let sent = a2.down(Down::Tree { node: stack(vec![field("f", "f", FieldKind::Str, json!({}))]) });
    assert!(sent.contains(&Up::Watch { bind: "cfg:x.f".into() }), "the screen watches the value it shows: {sent:?}");
    a.down(Down::Value { bind: "cfg:x.f".into(), value: json!("one"), rev: 1 });
    key(&mut a, KeyCode::Enter);
    keys(&mut a, "2");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.value("cfg:x.f"), &json!("one2"));
    a.down(Down::Value { bind: "cfg:x.f".into(), value: json!("one"), rev: 2 });
    assert_eq!(a.value("cfg:x.f"), &json!("one"), "the core turned the change down");
    assert_eq!(a.value("cfg:nothing"), &Json::Null);
}

#[test]
fn a_bind_without_a_dot_or_a_colon_still_names_what_is_set() {
    let mk = |bind: &str| {
        let mut a = started(stack(vec![Node { id: "f".into(), title: "f".into(), field: Some(Field { kind: FieldKind::Str, bind: bind.into(), props: Json::Null }), ..Node::default() }]));
        key(&mut a, KeyCode::Enter);
        keys(&mut a, "x");
        sets(&key(&mut a, KeyCode::Enter))
    };
    assert_eq!(mk("connector:claude.model"), vec![("connector".into(), "claude".into(), "model".into(), json!("x"))]);
    assert_eq!(mk("connector:a.b.c"), vec![("connector".into(), "a.b".into(), "c".into(), json!("x"))], "the field is after the last dot");
    assert_eq!(mk("plain"), vec![("".into(), "plain".into(), "".into(), json!("x"))]);
    assert_eq!(mk("kind:only"), vec![("kind".into(), "only".into(), "".into(), json!("x"))]);
}

// ---- inside the sections of a page ----

fn settings() -> App {
    let look = Node { id: "look".into(), title: "look".into(), present: Present::Stack, children: vec![field("palette", "palette", FieldKind::Str, json!({})), field("frame", "frame", FieldKind::Str, json!({}))], ..Node::default() };
    let chat = Node { id: "chat".into(), title: "chat".into(), present: Present::Stack, children: vec![field("width", "width", FieldKind::Int, json!({}))], ..Node::default() };
    let lone = field("lone", "lone", FieldKind::Str, json!({}));
    let sections = Node { id: "sections".into(), present: Present::Sections, children: vec![look, chat, lone], ..Node::default() };
    let mut a = started(Node { id: "root".into(), present: Present::Tabs, children: vec![Node { id: "settings".into(), title: "Settings".into(), ..sections }], ..Node::default() });
    for (id, v) in [("palette", json!("jjj")), ("frame", json!("pipes")), ("width", json!(88)), ("lone", json!("l"))] {
        a.down(Down::Value { bind: format!("cfg:x.{id}"), value: v, rev: 1 });
    }
    a
}

#[test]
fn right_goes_into_the_open_section_and_up_from_its_first_field_comes_back() {
    let mut a = settings();
    assert_eq!(at(&a), "settings", "the headers have the keys");
    key(&mut a, KeyCode::Right);
    assert_eq!(at(&a), "palette", "into the first field of the section");
    key(&mut a, KeyCode::Down);
    assert_eq!(at(&a), "frame");
    key(&mut a, KeyCode::Down);
    assert_eq!(at(&a), "frame", "the end of the section");
    key(&mut a, KeyCode::Up);
    key(&mut a, KeyCode::Up);
    assert_eq!(at(&a), "settings", "up from the first field is the header again");
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Right);
    assert_eq!(at(&a), "width", "the next section");
    key(&mut a, KeyCode::Esc);
    assert_eq!(at(&a), "settings", "escape also comes back");
    key(&mut a, KeyCode::Down);
    key(&mut a, KeyCode::Right);
    assert_eq!(at(&a), "lone", "a section that is one field");
    assert!(keys(&mut a, "x").is_empty());
    key(&mut a, KeyCode::Down);
    assert_eq!(at(&a), "lone");
    key(&mut a, KeyCode::Up);
    assert_eq!(at(&a), "settings");
}

#[test]
fn a_folded_section_is_not_entered() {
    let mut a = settings();
    key(&mut a, KeyCode::Enter);
    assert!(a.shut.contains("look"));
    key(&mut a, KeyCode::Right);
    assert_eq!(at(&a), "settings", "a folded section has no field to go to");
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Right);
    key(&mut a, KeyCode::Enter);
    keys(&mut a, "-x");
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!("jjj-x")], "the edit of a field in a section");
    let rows = screen(&mut a, 64, 12);
    assert!(rows.iter().any(|l| l.contains("look")) && rows.iter().any(|l| l.contains("palette")), "{rows:?}");
}

#[test]
fn the_jump_to_a_field_in_a_section_puts_the_keys_in_it() {
    let mut a = settings();
    a.key(ctrl('k'));
    keys(&mut a, "wid");
    key(&mut a, KeyCode::Enter);
    assert_eq!(at(&a), "width");
    a.key(ctrl('k'));
    keys(&mut a, "look");
    key(&mut a, KeyCode::Enter);
    assert_eq!(at(&a), "settings", "the jump to the section itself is to its header");
}

// ---- what is drawn ----

#[test]
fn an_edit_takes_more_rows_and_the_fields_under_it_move_down() {
    let mut a = form();
    let before = screen(&mut a, 64, 12);
    assert!(before[2].starts_with("├ count"));
    key(&mut a, KeyCode::Enter);
    let after = screen(&mut a, 64, 12);
    assert!(after[0].starts_with("├ name") && after[1].starts_with("│ ⏎") && after[2].starts_with("├ locked") && after[3].starts_with("├ count"), "{after:?}");
    key(&mut a, KeyCode::Esc);
    assert_eq!(screen(&mut a, 64, 12), before);
}

#[test]
fn a_label_that_does_not_fit_is_cut_and_the_value_is_not() {
    let label = "a very long name for a field to have, and then some more words after it";
    let mut a = started(stack(vec![field("f", label, FieldKind::Str, json!({}))]));
    a.down(Down::Value { bind: "cfg:x.f".into(), value: json!("claude"), rev: 1 });
    let row = &screen(&mut a, 60, 3)[0];
    assert!(row.contains('…') && row.trim_end().ends_with(" claude"), "{row:?}");
    assert_eq!(row.chars().count(), 60, "the row is the width of the screen");
}

#[test]
fn under_sixty_columns_the_label_has_its_own_row() {
    let mut a = one(FieldKind::Str, json!({}), json!("claude"));
    let wide = screen(&mut a, 60, 4);
    let narrow = screen(&mut a, 59, 4);
    assert!(wide[0].contains("claude") && wide[0].contains(" f ") && narrow[0].trim_end() == "╰ f" && narrow[1].trim_end() == "  claude", "{wide:?} {narrow:?}");
}

#[test]
fn the_kinds_that_are_values_are_the_ones_the_operator_can_change() {
    use FieldKind::*;
    for k in [Chat, Bar, Ask, Log, List, Form] {
        assert!(!k.is_value(), "{k:?}");
    }
    for k in [Str, Text, Int, Num, Bool, Enum, Multi, Ref, Seq, Map, Dur, Usd, Path, Argv, Pattern, Expr, Secret, Markdown] {
        assert!(k.is_value(), "{k:?}");
    }
}

#[test]
fn what_a_field_is_given_is_read_for_what_it_is() {
    let f = Field { kind: FieldKind::Enum, bind: "k:i.f".into(), props: json!({ "options": ["a", 2, "b"], "max": 3.5, "required": true, "disabled": "why", "error": "bad", "style": "radio", "name": "n" }) };
    let s = Spec::new("label", &f);
    assert_eq!((s.label, s.bind, s.kind), ("label", "k:i.f", FieldKind::Enum));
    assert_eq!(s.words("options"), vec!["a", "b"], "only the words");
    assert_eq!((s.num("max"), s.num("name"), s.num("absent")), (Some(3.5), None, None));
    assert_eq!((s.flag("required"), s.flag("max"), s.flag("absent")), (true, false, false));
    assert_eq!((s.text("name"), s.text("max")), (Some("n"), None));
    assert!(s.disabled() && s.told() == Some("bad") && s.get("absent").is_null());
    assert!(s.words("absent").is_empty());
    let none = Field { kind: FieldKind::Str, bind: "x".into(), props: Json::Null };
    let s = Spec::new("l", &none);
    assert!(!s.disabled() && s.told().is_none() && s.words("options").is_empty());
}

// ---- edge cases ----

#[test]
fn the_edges_of_a_glob_an_expression_and_a_range() {
    assert_eq!(parse::glob("[[]x"), None, "a bracket inside a class is a letter of it");
    assert_eq!(parse::expr("f(\")\")"), None, "a double quote holds text too");
    assert_eq!(parse::expr("f(\")"), Some("\" at 3 not closed".into()));
    assert_eq!(bad(FieldKind::Int, json!({ "min": 1, "max": 8 }), json!(1)), None, "the least is allowed");
    assert_eq!(bad(FieldKind::Int, json!({ "min": 1, "max": 8 }), json!(8)), None, "and the most");
    assert_eq!(bad(FieldKind::Int, json!({ "min": 1, "max": 8 }), json!(0)), Some("1–8".into()));
    assert_eq!(bad(FieldKind::Seq, json!({ "min": 1 }), json!(["a"])), None, "as many rows as the least is enough");
    for kind in [FieldKind::Int, FieldKind::Num, FieldKind::Dur, FieldKind::Usd, FieldKind::Pattern, FieldKind::Expr, FieldKind::Text] {
        assert_eq!(bad(kind, json!({}), Json::Null), None, "{kind:?}: a value never set is not wrong");
    }
    assert!(bad(FieldKind::Int, json!({ "required": true }), Json::Null).is_some(), "unless it is required");
}

#[test]
fn the_last_word_of_an_argument_list_is_edited_in_place() {
    let mut a = one(FieldKind::Argv, json!({}), json!(["rg", "-n"]));
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Backspace);
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!(["rg", "-"])]);
}

#[test]
fn an_arrow_at_the_last_option_stays_on_it() {
    let mut a = one(FieldKind::Enum, json!({ "options": ["a", "b", "c"] }), json!("c"));
    key(&mut a, KeyCode::Right);
    let row = &screen(&mut a, 64, 4)[0];
    assert!(row.contains("│ c"), "the last is still the one chosen: {row:?}");
    key(&mut a, KeyCode::Left);
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!("b")], "one left of the last, though the arrow right was pressed at the last");
}

#[test]
fn a_list_in_a_choice_holds_its_ends_so_one_arrow_back_is_one_row_back() {
    let mut a = one(FieldKind::Ref, json!({ "options": ["claude", "omp", "pi", "clef"] }), json!("omp"));
    key(&mut a, KeyCode::Enter);
    for _ in 0..6 {
        key(&mut a, KeyCode::Down);
    }
    key(&mut a, KeyCode::Up);
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!("pi")]);
    let mut a = one(FieldKind::Path, json!({ "options": ["a/", "a/b", "a/c"] }), json!(""));
    key(&mut a, KeyCode::Enter);
    keys(&mut a, "a");
    let rows = screen(&mut a, 64, 6);
    assert!(rows[1].contains("a/") && rows[2].contains("a/b") && rows[3].contains("a/c"), "{rows:?}");
    keys(&mut a, "/");
    let rows = screen(&mut a, 64, 6);
    assert!(rows[1].contains("a/b") && rows[2].contains("a/c") && !rows.iter().any(|l| l.trim_end().ends_with("a/") && l.contains('󰉖')), "what is typed is not offered again: {rows:?}");
}

#[test]
fn a_map_keeps_the_value_trimmed_and_does_not_move_its_rows() {
    let mut a = one(FieldKind::Map, json!({}), json!({ "A": "1", "B": "2" }));
    key(&mut a, KeyCode::Enter);
    assert!(a.key(shift(KeyCode::Down)).is_empty(), "a map has no order to move in");
    key(&mut a, KeyCode::Enter);
    keys(&mut a, "  ");
    assert_eq!(set_to(&key(&mut a, KeyCode::Enter)), vec![json!({ "A": "1", "B": "2" })]);
}

#[test]
fn what_a_value_shows_with_a_unit_a_secret_and_a_long_text() {
    let mut a = one(FieldKind::Int, json!({ "unit": "turns" }), json!(3));
    assert!(screen(&mut a, 64, 2)[0].contains("3 turns"));
    let mut a = one(FieldKind::Int, json!({ "unit": "turns" }), Json::Null);
    assert!(screen(&mut a, 64, 2)[0].contains('—') && !screen(&mut a, 64, 2)[0].contains("turns"), "no value, no unit");
    let mut a = one(FieldKind::Secret, json!({}), json!("set"));
    assert!(show(&mut a, 64, 2).contains("set") && !show(&mut a, 64, 2).contains("not set"), "a text that is there is a secret that is set");
    let mut a = one(FieldKind::Secret, json!({}), json!(""));
    assert!(show(&mut a, 64, 2).contains("not set"));
    let mut a = one(FieldKind::Text, json!({}), json!("one\ntwo"));
    assert!(screen(&mut a, 64, 2)[0].trim_end().ends_with("one…"), "a second line is a dots");
    let mut a = one(FieldKind::Text, json!({}), json!("one"));
    assert!(screen(&mut a, 64, 2)[0].trim_end().ends_with("one"), "and one line is not");
    assert!(!screen(&mut a, 64, 2)[0].contains('…'));
}

#[test]
fn a_box_scrolls_to_the_caret_and_a_markdown_is_six_rows_tall_unless_told() {
    let mut a = one(FieldKind::Text, json!({ "rows": 2 }), json!("one\ntwo\nthree"));
    key(&mut a, KeyCode::Enter);
    let rows = screen(&mut a, 64, 6);
    assert!(rows[1].contains("two") && rows[2].contains("three▏") && !rows.iter().any(|l| l.contains("one")), "the last two rows, where the caret is: {rows:?}");
    let body: Vec<String> = (1..=12).map(|i| format!("line {i}")).collect();
    let mut a = one(FieldKind::Markdown, json!({}), json!(body.join("\n")));
    key(&mut a, KeyCode::Enter);
    let rows = screen(&mut a, 64, 12);
    assert!(rows[1].contains("line 1") && rows[6].contains("line 6") && !rows[7].contains("line 7"), "six rows: {rows:?}");
}

#[test]
fn the_options_to_tick_fit_the_columns_the_width_leaves() {
    let opts: Vec<String> = (0..4).map(|i| format!("opt{i}")).collect();
    // a cell is the name (4), the mark (2) and two blanks: 8 columns, after the pipe
    let mut a = one(FieldKind::Multi, json!({ "options": opts.clone() }), json!([]));
    key(&mut a, KeyCode::Enter);
    let narrow = screen(&mut a, 17, 8);
    assert!(narrow[1].contains("opt0") && !narrow[1].contains("opt1") && narrow[2].contains("opt1"), "one to a row at 17: {narrow:?}");
    let wider = screen(&mut a, 18, 8);
    assert!(wider[1].contains("opt0") && wider[1].contains("opt1"), "two to a row at 18: {wider:?}");
}

#[test]
fn a_form_in_a_screen_too_short_for_it_is_cut_and_does_not_fail() {
    let mut a = settings();
    for h in 1..8 {
        let rows = screen(&mut a, 60, h);
        assert_eq!(rows.len(), usize::from(h).min(rows.len()), "{h} rows");
    }
    key(&mut a, KeyCode::Right);
    key(&mut a, KeyCode::Enter);
    let _ = screen(&mut a, 60, 3);
}
