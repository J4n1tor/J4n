//! The reader. One case for each part of the reader: what CommonMark it reads and how
//! it draws each part — then the places that draw prose through it, and the terminal it asks. The goldens are the scenes `read*`.

use j4n_tui::read::{Caps, Opts, RLine, read, spans, xtversion};
use j4n_tui::theme::palette::Role;
use j4n_tui::theme::{Settings, Theme};
use ratatui::style::{Color, Modifier, Style};

fn theme(marks: &str) -> Theme {
    Theme::build(&Settings { marks: marks.into(), ..Settings::default() }, &[]).expect("a theme")
}

fn on() -> Opts {
    Opts { caps: Caps::ALL, ..Opts::default() }
}

fn draw(text: &str, o: &Opts, w: usize) -> Vec<RLine> {
    read(&theme("unicode"), o, Style::default(), text, w)
}

fn said(lines: &[RLine]) -> Vec<String> {
    lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>()).collect()
}

fn rows(text: &str) -> Vec<String> {
    said(&draw(text, &on(), 76))
}

/// The style of the first span whose words hold `word`.
fn style_of(lines: &[RLine], word: &str) -> Style {
    lines.iter().flat_map(|l| &l.spans).find(|s| s.content.contains(word)).unwrap_or_else(|| panic!("`{word}` is not drawn")).style
}

fn role(r: Role) -> Option<Color> {
    let c = theme("unicode").roles.get(r);
    Some(Color::Rgb(c.0, c.1, c.2))
}

#[test]
fn a_paragraph_is_cut_at_the_reading_width_and_a_blank_row_stands_between_two() {
    let long = "word ".repeat(40);
    let got = rows(&format!("{long}\n\nsecond"));
    assert!(got.iter().all(|l| l.chars().count() <= 76), "{got:?}");
    assert_eq!(got.iter().position(String::is_empty), Some(got.len() - 2), "one blank row, between the two paragraphs: {got:?}");
    // the room the screen has is the other limit
    assert!(said(&draw(&long, &on(), 40)).iter().all(|l| l.chars().count() <= 40));
    // `read.width` moves it
    assert!(said(&draw(&long, &Opts { width: 30, ..on() }, 80)).iter().all(|l| l.chars().count() <= 30));
}

#[test]
fn strong_is_bold_text_and_nothing_else_is_bold() {
    let lines = draw("a **strong word** and *em* and `code`", &on(), 76);
    assert_eq!(said(&lines), ["a strong word and em and code"], "no marks are left");
    let s = style_of(&lines, "strong");
    assert!(s.add_modifier.contains(Modifier::BOLD) && s.fg == role(Role::Strong), "{s:?}");
    for plain in ["and", "em", "code"] {
        assert!(!style_of(&lines, plain).add_modifier.contains(Modifier::BOLD), "{plain}");
    }
}

#[test]
fn emphasis_is_italic_where_the_terminal_has_it_and_plain_where_it_does_not() {
    assert!(style_of(&draw("an *aside*", &on(), 76), "aside").add_modifier.contains(Modifier::ITALIC));
    let plain = Opts { caps: Caps::PLAIN, ..Opts::default() };
    assert!(!style_of(&draw("an *aside*", &plain, 76), "aside").add_modifier.contains(Modifier::ITALIC), "an unknown terminal gets the plain subset");
}

#[test]
fn inline_code_is_accent_on_the_raised_band_with_no_backticks() {
    let lines = draw("run `cargo test` now", &on(), 76);
    assert_eq!(said(&lines), ["run cargo test now"]);
    let s = style_of(&lines, "cargo test");
    let t = theme("unicode");
    assert_eq!((s.fg, s.bg), (role(Role::Code), t.back(Role::Raise).bg), "{s:?}");
}

#[test]
fn strike_is_crossed_out() {
    assert!(style_of(&draw("~~gone~~ stays", &on(), 76), "gone").add_modifier.contains(Modifier::CROSSED_OUT));
}

#[test]
fn headings_have_no_hash_a_blank_row_above_and_level_one_a_rule_under() {
    let got = rows("intro\n\n# One\n\n## Two\n\n### Three\n\ntext");
    assert!(!got.iter().any(|l| l.contains('#')), "{got:?}");
    let one = got.iter().position(|l| l == "One").expect("the heading");
    assert_eq!((got[one - 1].as_str(), got[one + 1].as_str()), ("", "────"), "a blank row above, a short rule under: {got:?}");
    let two = got.iter().position(|l| l == "Two").expect("level 2");
    assert!(got[two - 1].is_empty() && !got[two + 1].contains('─'), "levels 2 and 3 have no rule: {got:?}");
    let lines = draw("## Two", &on(), 76);
    let s = style_of(&lines, "Two");
    assert!(s.fg == role(Role::Head) && s.add_modifier.contains(Modifier::BOLD), "{s:?}");
}

#[test]
fn bullets_nest_by_two_and_hang_under_their_words() {
    let got = rows("- one that is long enough to wrap round onto a second line of the answer because it keeps going on and on\n  - nested\n    - deeper\n- two");
    assert_eq!(got[0].chars().take(2).collect::<String>(), "• ");
    assert!(got[1].starts_with("  ") && !got[1].starts_with("  •") && got[1].trim_start().starts_with(|c: char| c.is_alphabetic()), "a hanging indent: {got:?}");
    let nested = got.iter().find(|l| l.contains("nested")).expect("nested");
    let deeper = got.iter().find(|l| l.contains("deeper")).expect("deeper");
    assert_eq!((nested.find('◦'), deeper.find('◦')), (Some(2), Some(4)), "nested by 2 with the second bullet: {got:?}");
    assert!(!got.iter().any(String::is_empty), "a tight list has no blank rows: {got:?}");
}

#[test]
fn numbers_are_right_aligned_with_a_hanging_indent() {
    let got = rows("8. eighth\n9. ninth\n10. tenth is long enough to wrap onto a second line of the answer, which is cut at seventy-six columns here");
    assert_eq!(&got[0][..4], " 8. ", "{got:?}");
    assert_eq!(&got[2][..4], "10. ", "{got:?}");
    assert!(got[3].starts_with("    ") && !got[3].trim().is_empty(), "{got:?}");
}

#[test]
fn a_task_is_a_glyph_in_the_icon_tiers_and_brackets_in_the_plain_ones() {
    let t = |marks: &str| said(&read(&theme(marks), &on(), Style::default(), "- [ ] open\n- [x] done", 76));
    assert_eq!(t("icons"), ["\u{f0131}  open", "\u{f0135}  done"]);
    assert_eq!(t("unicode"), ["◌ open", "▣ done"]);
    assert_eq!(t("ascii"), ["[ ] open", "[x] done"]);
    assert_eq!(t("words"), ["[ ] open", "[x] done"]);
}

#[test]
fn a_quote_has_a_dim_thin_bar_and_dim_italic_words() {
    let lines = draw("> an aside that goes on\n> over two lines", &on(), 76);
    assert_eq!(said(&lines), ["▏ an aside that goes on", "▏ over two lines"], "a line break stays one");
    let s = style_of(&lines, "aside");
    assert!(s.fg == role(Role::Dim) && s.add_modifier.contains(Modifier::ITALIC), "{s:?}");
    assert_eq!(style_of(&lines, "▏").fg, role(Role::Dim), "the bar is dim");
    let two = rows("> one\n>\n> two");
    assert_eq!(two, ["▏ one", "▏", "▏ two"], "a blank row inside a quote keeps the bar");
}

#[test]
fn a_rule_is_a_line_of_the_edge_role() {
    let lines = draw("above\n\n---\n\nbelow", &on(), 76);
    let got = said(&lines);
    let at = got.iter().position(|l| l.starts_with('─')).expect("a rule");
    assert_eq!((got[at - 1].as_str(), got[at + 1].as_str()), ("", ""));
    assert_eq!(style_of(&lines, "─").fg, role(Role::Edge));
}

#[test]
fn a_table_has_a_frame_in_the_edge_role_and_a_bold_header() {
    let lines = draw("| name | role |\n|---|---|\n| ada | first |", &on(), 76);
    assert_eq!(said(&lines), ["╭──────┬───────╮", "│ name │ role  │", "├──────┼───────┤", "│ ada  │ first │", "╰──────┴───────╯"]);
    assert!(style_of(&lines, "name").add_modifier.contains(Modifier::BOLD) && !style_of(&lines, "ada").add_modifier.contains(Modifier::BOLD));
    assert_eq!(style_of(&lines, "╭").fg, role(Role::Edge));
    // the ascii tier has no junctions
    let a = said(&read(&theme("ascii"), &on(), Style::default(), "| a | b |\n|---|---|\n| 1 | 2 |", 76));
    assert_eq!(a[0], "+---+---+", "{a:?}");
}

#[test]
fn a_table_too_wide_for_its_room_is_key_value_lines() {
    let got = said(&draw("| first column | second column | third |\n|---|---|---|\n| one | two | three |\n| four | five | six |", &on(), 30));
    assert_eq!(got, ["first column: one", "second column: two", "third: three", "", "first column: four", "second column: five", "third: six"]);
}

#[test]
fn a_code_block_is_a_raised_band_with_its_language_at_the_top_right_and_folds_after_twelve_lines() {
    let lines = draw("```rust\nfn main() {}\n```", &on(), 40);
    let got = said(&lines);
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(got[0].trim_end().ends_with("rust") && got[0].starts_with(' '), "the tag at the right: {got:?}");
    assert!(got[1].starts_with(" fn main() {}") && got.iter().all(|l| l.chars().count() == 40), "a band of one width: {got:?}");
    let t = theme("unicode");
    assert_eq!(style_of(&lines, "fn main").bg, t.back(Role::Raise).bg);
    let body: String = (1..=15).map(|i| format!("l{i}\n")).collect();
    let fenced = format!("```\n{body}```");
    let folded = said(&draw(&fenced, &on(), 40));
    assert_eq!(folded.len(), 13, "twelve lines and the fold: {folded:?}");
    assert!(folded[12].contains("+3 ▸"));
    assert_eq!(said(&draw(&fenced, &Opts { full: true, ..on() }, 40)).len(), 15, "full does not fold");
    assert_eq!(said(&draw(&fenced, &Opts { fold: 4, ..on() }, 40)).len(), 5, "read.fold_code moves it");
    assert_eq!(said(&draw(&fenced, &Opts { fold: 0, ..on() }, 40)).len(), 15, "0 never folds");
}

#[test]
fn a_link_is_its_underlined_words_and_the_address_opens_where_the_terminal_takes_it() {
    let text = "see [the docs](https://example.com/a) now";
    let lines = draw(text, &on(), 76);
    assert_eq!(said(&lines), ["see the docs now"]);
    let s = style_of(&lines, "docs");
    assert!(s.add_modifier.contains(Modifier::UNDERLINED) && s.fg == role(Role::Link), "{s:?}");
    assert_eq!(lines[0].links.iter().map(|l| (l.col, l.w, l.url.as_str())).collect::<Vec<_>>(), [(4, 8, "https://example.com/a")], "one link over both words");
    // a terminal that does not take it shows the words; `read.links = plain` says the address
    let none = draw(text, &Opts { caps: Caps::PLAIN, ..Opts::default() }, 76);
    assert!(none[0].links.is_empty() && said(&none) == ["see the docs now"]);
    let plain = said(&draw(text, &Opts { osc8: false, ..on() }, 76));
    assert_eq!(plain, ["see the docs (https://example.com/a) now"]);
    // an address that is not a web or file one is never put into an escape sequence
    assert!(draw("[x](javascript:alert(1))", &on(), 76)[0].links.is_empty());
    assert!(draw("[x](https://a.b/\u{1b}]52)", &on(), 76)[0].links.is_empty());
}

#[test]
fn a_path_is_code_and_a_file_address_where_the_terminal_takes_one() {
    let o = Opts { root: Some("/work".into()), ..on() };
    let lines = draw("edit src/main.rs:12, then ./a/b.toml.", &o, 76);
    assert_eq!(said(&lines), ["edit src/main.rs:12, then ./a/b.toml."]);
    assert_eq!(style_of(&lines, "src/main.rs:12").bg, theme("unicode").back(Role::Raise).bg, "drawn as code");
    assert_eq!(lines[0].links.iter().map(|l| l.url.as_str()).collect::<Vec<_>>(), ["file:///work/src/main.rs", "file:///work/a/b.toml"]);
    assert!(draw("edit src/main.rs", &on(), 76)[0].links.is_empty(), "no folder, no address");
    // not a path: no slash, or no extension
    let not = draw("and/or e.g. 1/2 v1.2", &o, 76);
    assert!(not[0].links.is_empty() && style_of(&not, "and/or").bg.is_none());
}

#[test]
fn raw_html_is_shown_as_text() {
    assert_eq!(rows("a <b>bold</b> <script>x</script> end"), ["a <b>bold</b> <script>x</script> end"]);
    assert_eq!(rows("<div>\nblock\n</div>"), ["<div>", "block", "</div>"], "a block of HTML keeps its lines");
}

#[test]
fn the_dense_style_has_no_blank_rows_and_a_description_keeps_the_room_it_has() {
    let md = "# H\n\npara\n\n- a\n- b\n\n> q\n\n```\nc\n```";
    let calm = rows(md);
    let full = said(&draw(md, &Opts { full: true, ..on() }, 76));
    assert!(calm.iter().any(String::is_empty) && !full.iter().any(String::is_empty), "{calm:?} / {full:?}");
    // a description is cut to its room and not to `read.width`
    let long = "word ".repeat(30);
    let o = on();
    let out = spans(&theme("unicode"), &o, Style::default(), long.trim(), 120);
    assert_eq!(out.len(), 2, "{} lines", out.len());
    assert!(out[0].iter().map(|s| s.content.chars().count()).sum::<usize>() > 76);
}

#[test]
fn what_the_terminal_is_comes_from_its_answer_or_its_name_and_an_unknown_one_gets_the_plain_subset() {
    let none = |_: &str| None;
    assert_eq!(Caps::detect(Some("kitty(0.40.1)"), &none), Caps::ALL, "kitty 0.40 has text sizing");
    assert!(!Caps::detect(Some("kitty(0.39.0)"), &none).sizing && Caps::detect(Some("kitty(0.39.0)"), &none).osc8);
    for name in ["WezTerm 20240203", "ghostty 1.1.0", "foot(1.18.0)", "iTerm2 3.5.0"] {
        let c = Caps::detect(Some(name), &none);
        assert!(c.osc8 && c.italic && !c.sizing, "{name}");
    }
    assert_eq!(Caps::detect(Some("tmux 3.4"), &none), Caps::PLAIN);
    assert_eq!(Caps::detect(Some("xterm-ish"), &none), Caps::PLAIN);
    assert_eq!(Caps::detect(None, &none), Caps::PLAIN, "no answer and no name");
    let env = |k: &str| (k == "TERM_PROGRAM").then(|| "ghostty".to_string());
    assert!(Caps::detect(None, &env).osc8, "TERM_PROGRAM names it when it does not answer");
    let term = |k: &str| (k == "TERM").then(|| "xterm-kitty".to_string());
    assert!(Caps::detect(None, &term).osc8);
    assert!(Caps::detect(None, &|k: &str| (k == "TERM").then(|| "foot".to_string())).italic);
    assert_eq!(xtversion(b"\x1b[?62;c"), None);
    assert_eq!(xtversion(b"\x1bP>|kitty(0.40.1)\x1b\\\x1b[?62;c").as_deref(), Some("kitty(0.40.1)"));
}

#[test]
fn the_options_are_the_words_of_read_settings() {
    let r = j4n_proto::Read { style: "full".into(), width: 60, fold_code: 3, links: "plain".into(), big_heads: true, terms: false };
    let o = Opts::of(&r);
    assert_eq!((o.full, o.width, o.fold, o.osc8, o.big_heads), (true, 60, 3, false, true));
    let d = Opts::of(&j4n_proto::Read::default());
    assert_eq!((d.full, d.width, d.fold, d.osc8, d.big_heads), (false, 76, 12, true, false));
}

#[test]
fn the_four_roles_and_the_five_slots_are_in_every_palette_and_tier() {
    use j4n_tui::theme::glyph::{Slot, Tier};
    for p in j4n_tui::theme::palette::builtin() {
        let t = Theme::build(&Settings { palette: p.id.clone(), ..Settings::default() }, &[]).expect("a theme");
        for r in [Role::Strong, Role::Code, Role::Link, Role::Head] {
            let c = t.roles.get(r);
            assert!(c.contrast(t.roles.get(Role::Bg)) >= 3.0, "{} {}: {}", p.id, r.name(), c.hex());
        }
    }
    for tier in Tier::ALL {
        for s in [Slot::Bullet2, Slot::TaskOpen, Slot::TaskDone, Slot::QuoteBar, Slot::Rule] {
            assert!(!s.cell(tier).is_empty(), "{} {}", s.name(), tier.name());
        }
    }
}

// ---- the places that draw prose through it ----

mod places {
    use crate::world::{hello, screen};
    use j4n_proto::{By, Cell, Down, Field, FieldKind, Json, Node, Present, Row, Tone};
    use j4n_tui::App;
    use j4n_tui::headless::{KeyCode, press};

    fn started(root: Node) -> App {
        let mut a = App::new();
        a.start();
        a.down(hello());
        a.down(Down::Tree { node: root });
        a
    }

    fn stack(kind: FieldKind, bind: &str) -> Node {
        let f = Node { id: "f".into(), field: Some(Field { kind, bind: bind.into(), props: Json::Null }), ..Node::default() };
        Node { id: "root".into(), present: Present::Stack, children: vec![f], ..Node::default() }
    }

    fn text(rows: Vec<String>) -> String {
        rows.join("\n")
    }

    #[test]
    fn an_answer_in_the_chat_shows_no_raw_marks() {
        let cell = |t: &str, icon: &str| Cell::with_icon(icon, t, Tone::Plain);
        let row = |key: &str, t: &str, icon: &str, parent: Option<&str>, by: By| Row { key: key.into(), cells: vec![cell(t, icon), Cell::new("", Tone::Dim)], parent: parent.map(String::from), by, ..Row::default() };
        let rows = vec![
            row("t", "go", "", None, By::Core),
            row("l", "A x", "working", Some("t"), By::Core),
            row("p", "The **parser** now eats `,` first.\n\n## What changed\n\n- [x] done in `src/p.rs`\n- a [link](https://a.b/c)", "", Some("l"), By::Core),
            row("n", "limit at **80%**, back at `18:20`", "notice", Some("t"), By::Doer),
        ];
        let mut a = started(stack(FieldKind::Chat, "chat:main"));
        a.down(Down::Rows { bind: "chat:main".into(), rows, total: 4, from: 0, rev: 1 });
        let s = text(screen(&mut a, 80, 30));
        for raw in ["**", "`", "## ", "](", "[x]"] {
            assert!(!s.contains(raw), "`{raw}` is drawn raw:\n{s}");
        }
        assert!(s.contains("What changed") && s.contains("The parser now eats , first.") && s.contains("limit at 80%, back at 18:20"), "{s}");
    }

    #[test]
    fn a_description_in_settings_is_read_in_its_cell_and_in_its_detail() {
        let mut a = started(stack(FieldKind::List, "view:s"));
        let doc = "Whether the **name** of a doer `breathes`: on, it moves; see [the page](https://a.b/c).";
        let rows = vec![Row { key: "s".into(), cells: vec![Cell::new("theme.motion", Tone::Plain), Cell::new("true", Tone::Plain), Cell::new(doc, Tone::Plain)], detail: vec![Cell::new(doc, Tone::Dim)], ..Row::default() }];
        a.down(Down::Rows { bind: "view:s".into(), rows, total: 1, from: 0, rev: 1 });
        let s = text(screen(&mut a, 100, 8));
        assert!(!s.contains('`') && !s.contains("**") && !s.contains("]("), "{s}");
        assert!(s.contains("Whether the name of a doer breathes"), "{s}");
    }

    #[test]
    fn the_detail_of_a_log_row_is_read_too() {
        let mut a = started(stack(FieldKind::Log, "log:all"));
        let row = Row { key: "e".into(), cells: vec![Cell::new("12:00 note", Tone::Plain)], detail: vec![Cell::new("said **why** in `words`", Tone::Plain)], ..Row::default() };
        a.down(Down::Rows { bind: "log:all".into(), rows: vec![row], total: 1, from: 0, rev: 1 });
        a.key(press(KeyCode::Enter));
        let s = text(screen(&mut a, 60, 6));
        assert!(s.contains("said why in words") && !s.contains('`') && !s.contains("**"), "{s}");
    }
}

// ---- the question to the terminal ----

mod probe {
    use crate::world::{app, press};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use j4n_proto::{Intent, Up};
    use j4n_tui::headless::typed;
    use j4n_tui::read::{Caps, Late};
    use std::time::{Duration, Instant};

    /// The keys crossterm makes of terminal bytes: `ESC` and the byte after it is that byte with alt, the rest are characters.
    fn keys(bytes: &str) -> Vec<KeyEvent> {
        let mut out = Vec::new();
        let mut it = bytes.chars();
        while let Some(c) = it.next() {
            if c == '\x1b' {
                let n = it.next().expect("a byte after ESC");
                let shift = if n.is_ascii_uppercase() { KeyModifiers::SHIFT } else { KeyModifiers::NONE };
                out.push(KeyEvent::new(KeyCode::Char(n), KeyModifiers::ALT | shift));
            } else {
                out.push(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
            }
        }
        out
    }

    const REPLY: &str = "\x1bP>|kitty(0.40.1)\x1b\\\x1b[?62;4c";

    fn say(text: &str) -> Up {
        Up::Intent { intent: Intent::Say { text: text.into(), with: Vec::new() } }
    }

    #[test]
    fn a_reply_that_comes_late_is_not_typed_into_the_draft() {
        // without the guard the reply is the operator's draft
        let mut bare = app();
        press(&mut bare, keys(REPLY));
        assert!(!bare.line.buf.is_empty(), "the failure this guards against: {:?}", bare.line.buf);
        // with it, the reply is dropped and what is typed after it is sent as it was typed
        let mut a = app();
        a.late = Late::after(Instant::now());
        assert!(press(&mut a, keys(REPLY)).is_empty());
        assert_eq!(a.line.buf, "", "nothing of the reply is in the draft");
        assert_eq!(press(&mut a, typed("ok", true)), [say("ok")]);
        // a key that is not part of a reply goes through, also the alt chords that start like one
        let mut b = app();
        b.late = Late::after(Instant::now());
        press(&mut b, keys("\x1bPlain"));
        assert_eq!(b.line.buf, "lain", "Alt-P not followed by `>` is lost, what follows is typed");
    }

    #[test]
    fn the_guard_ends_two_seconds_after_the_question() {
        let mut a = app();
        a.late = Late::after(Instant::now() - Duration::from_secs(3));
        press(&mut a, keys("\x1bP>|x\x1b\\"));
        assert!(!a.line.buf.is_empty(), "a long time after the question those are the operator's keys");
    }

    #[test]
    fn over_ssh_without_a_terminal_in_a_test_or_on_request_no_question_is_put() {
        let env = |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
        let quiet = env(&[("TERM", "xterm-kitty")]);
        assert!(Caps::asks(&quiet, true, true), "a terminal on a terminal is asked");
        assert!(!Caps::asks(&quiet, false, true) && !Caps::asks(&quiet, true, false), "stdin or stdout not a terminal");
        assert!(!Caps::asks(&env(&[("SSH_CONNECTION", "10.0.0.1 22 10.0.0.2 51234")]), true, true), "ssh");
        assert!(!Caps::asks(&env(&[("SSH_TTY", "/dev/pts/3")]), true, true), "ssh with a tty");
        assert!(Caps::asks(&env(&[("SSH_TTY", "")]), true, true), "an empty variable is none");
        assert!(!Caps::asks(&env(&[("J4N_HERMETIC", "1")]), true, true) && !Caps::asks(&env(&[("J4N_PROBE", "0")]), true, true) && !Caps::asks(&env(&[("TERM", "dumb")]), true, true));
        // and the name still comes from the environment
        assert!(Caps::detect(None, &env(&[("TERM_PROGRAM", "WezTerm"), ("SSH_TTY", "/dev/pts/3")])).osc8);
    }
}

// ---- the ids of the project's docs ----

fn glossary() -> Opts {
    let mut o = on();
    o.glossary(&j4n_proto::json!({ "X-101": "some row of text", "B12": "a fix of the plan", "K12": "the Tab key" }));
    o
}

#[test]
fn a_known_id_is_drawn_with_its_words_the_first_time_and_alone_after_that() {
    let lines = draw("Open loose ends: X-101. Then X-101 again, and B12.", &glossary(), 76);
    assert_eq!(said(&lines), ["Open loose ends: X-101 · some row of text. Then X-101 again, and B12 · a fix", "of the plan."]);
    // the words are faint, the id is not
    let id = style_of(&lines, "X-101");
    let words = style_of(&lines, "some");
    assert_ne!(id, words, "the words are another style than the id");
    assert_eq!(words.fg, theme("unicode").tone(j4n_proto::Tone::Dim).fg, "faint: the dim tone");
    assert_eq!(lines[0].terms, ["X-101", "B12"], "the line knows its ids, each once, for ⏎");
}

#[test]
fn an_id_the_docs_do_not_define_and_a_word_that_only_looks_like_one_are_left_alone() {
    let got = rows_with("X-999 and X1 and UTF8 and x-101 and X-1010 and (X-101).", &glossary());
    assert_eq!(got, ["X-999 and X1 and UTF8 and x-101 and X-1010 and (X-101 · some row of text)."], "the words stand inside the bracket, before it closes");
}

fn rows_with(text: &str, o: &Opts) -> Vec<String> {
    said(&draw(text, o, 76))
}

#[test]
fn an_id_in_code_in_a_code_block_in_a_link_or_a_table_is_never_annotated() {
    let got = rows_with("a `X-101` here\n\n[see X-101](https://example.com)\n\n```\nX-101 in code\n```\n\n| a |\n|---|\n| X-101 |", &glossary());
    assert!(got.iter().all(|l| !l.contains('·')), "{got:?}");
    let lines = draw("a `X-101` here", &glossary(), 76);
    assert!(lines[0].terms.is_empty(), "code is not an id to open");
}

#[test]
fn the_words_wrap_with_the_sentence_and_are_asked_for_once_in_an_answer() {
    let got = said(&draw("see K12 and then K12 again", &glossary(), 14));
    assert_eq!(got, ["see K12 · the", "Tab key and", "then K12 again"], "{got:?}");
}

#[test]
fn with_read_terms_off_or_no_glossary_nothing_is_added_and_a_description_never_is() {
    let mut o = glossary();
    assert_eq!(rows_with("X-101", &o), ["X-101 · some row of text"]);
    // a description is read with `spans`: ids are as written there
    let described = spans(&theme("unicode"), &o, Style::default(), "see X-101", 40);
    assert_eq!(described[0].iter().map(|s| s.content.as_ref()).collect::<String>(), "see X-101");
    o.terms_on = false;
    o.glossary(&j4n_proto::json!({ "X-101": "x" }));
    assert_eq!(rows_with("X-101", &o), ["X-101"], "read.terms off: the glossary is not kept");
    assert_eq!(rows_with("X-101", &on()), ["X-101"], "none came");
}

// ---- level-1 headings in kitty's text sizing ----

fn big() -> Opts {
    Opts { big_heads: true, ..on() }
}

#[test]
fn a_heading_in_text_sizing_stands_on_one_row_with_a_blank_row_under_it_and_its_words_are_asked_for() {
    let got = draw("# A big one\n\nafter", &big(), 76);
    let said = said(&got);
    assert_eq!(said[0], "A big one", "the words stay in the buffer, in the heading's style: they are what a terminal without the size shows");
    assert_eq!(got[0].big, Some((0, "A big one".to_string())));
    assert_eq!(said[1], "", "the row under the words is blank, the terminal draws into it");
    assert!(got[1].big.is_none() && got[1].spans.is_empty());
    assert_eq!(said[2], "─".repeat(18), "the rule under it is as wide as the words are twice");
    // only the first level, and a line shifted right moves the cell
    assert!(draw("## smaller", &big(), 76)[0].big.is_none());
    let shifted = draw("# A big one", &big(), 76).remove(0).after(&[ratatui::text::Span::raw("│ ")]);
    assert_eq!(shifted.big, Some((2, "A big one".to_string())));
}

#[test]
fn the_words_wrap_at_half_the_room_because_they_are_drawn_twice_as_wide() {
    let got = draw("# aaaa bbbb cccc dddd", &big(), 20);
    let words: Vec<_> = got.iter().filter_map(|l| l.big.as_ref().map(|b| b.1.clone())).collect();
    assert_eq!(words, ["aaaa bbbb", "cccc dddd"], "{:?}", said(&got));
    assert!(got.iter().all(|l| l.width() <= 20));
}

#[test]
fn with_either_the_setting_or_the_terminal_missing_the_lines_are_what_they_were() {
    let text = "# A big one\n\nafter";
    let before = draw(text, &Opts { caps: Caps::PLAIN, ..Opts::default() }, 76);
    for o in [Opts { big_heads: false, ..on() }, Opts { big_heads: true, caps: Caps { sizing: false, ..Caps::ALL }, ..Opts::default() }] {
        let got = draw(text, &o, 76);
        assert_eq!(said(&got), said(&before), "{o:?}");
        assert!(got.iter().all(|l| l.big.is_none()), "{o:?}");
    }
    assert_eq!((said(&before)[0].as_str(), said(&before)[1].clone()), ("A big one", "─".repeat(9)));
}

#[test]
fn the_cell_of_a_heading_carries_the_sequence_of_the_text_sizing_protocol_with_the_style_of_the_words() {
    let mut cell = ratatui::buffer::Cell::default();
    cell.set_symbol("A").set_fg(Color::Red).set_style(Style::default().add_modifier(Modifier::BOLD));
    let c = j4n_tui::term::sized(&cell, "A big\x1b]8;;x\x07 one");
    assert_eq!(c.symbol(), "\x1b]66;s=2:w=0;A big]8;;x one\x1b\\", "no control character stands in the words");
    assert_eq!(c.fg, Color::Red);
    assert!(c.modifier.contains(Modifier::BOLD));
}
