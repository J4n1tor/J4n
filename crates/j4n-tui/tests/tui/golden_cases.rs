//! The golden of a screen (`golden`): the text, the same with its style as ANSI, the comparison and the spacing check, on
//! buffers and strings made here.

use j4n_tui::golden;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

#[test]
fn a_golden_that_changed_is_found_and_says_where() {
    assert_eq!(golden::differ("a\nb", "a\nb"), None);
    let got = golden::differ("a\nb\nc", "a\nX\nc").expect("a difference");
    assert!(got.starts_with("row 2:") && got.contains("\"b\"") && got.contains("\"X\""), "{got}");
    assert!(golden::differ("a", "a\nb").expect("a longer screen").starts_with("row 2:"), "an added row is found");
    assert!(golden::differ("a\n", "a").is_some(), "a final line break is a difference");
    let styled = golden::differ("\x1b[31mred\x1b[0m", "red").expect("a style gone");
    assert!(styled.contains("\\e[31m"), "an escape is shown, not sent to the terminal: {styled}");
}

#[test]
fn an_icon_must_be_followed_by_a_blank_cell() {
    assert!(golden::spacing("\u{f0001} one\n\u{e0b0} two\nplain ✓ text").is_empty());
    assert!(golden::spacing("an icon at the end \u{f0001}").is_empty(), "the end of a row has room");
    let got = golden::spacing("ok\n\u{f0001}x\nab \u{e0b0}\u{e0b0} c");
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(got[0].starts_with("row 2, column 1:"), "{got:?}");
    assert!(got[1].starts_with("row 3, column 4:"), "{got:?}");
    assert!(golden::spacing("世\u{f0001}x").iter().any(|d| d.contains("column 3")), "a wide letter takes two columns");
}

#[test]
fn the_ansi_golden_holds_colour_and_modifiers() {
    let mut b = Buffer::empty(Rect::new(0, 0, 12, 3));
    b.set_string(0, 0, "ab", Style::default().fg(Color::Red));
    b.set_string(2, 0, "cd", Style::default().fg(Color::Rgb(1, 2, 3)).bg(Color::Indexed(9)).add_modifier(Modifier::BOLD | Modifier::ITALIC));
    b.set_string(0, 1, "x世y", Style::default().fg(Color::DarkGray).bg(Color::LightBlue));
    b.set_string(0, 2, "tail  ", Style::default());
    let got = golden::ansi(&b);
    let rows: Vec<&str> = got.split('\n').collect();
    assert_eq!(rows[0], "\x1b[0;31mab\x1b[0;1;3;38;2;1;2;3;48;5;9mcd\x1b[0m", "{got:?}");
    assert_eq!(rows[1], "\x1b[0;90;104mx世y\x1b[0m");
    assert_eq!(rows[2], "tail", "blanks in the terminal's own colours at the end of a row are cut off");
    assert_eq!(golden::text(&b), "abcd\nx世y\ntail");
    let mut cut = Buffer::empty(Rect::new(0, 0, 6, 1));
    cut.set_string(0, 0, "ab", Style::default());
    cut.set_string(2, 0, "  ", Style::default().bg(Color::Blue));
    assert_eq!(golden::ansi(&cut), "ab\x1b[0;44m  \x1b[0m", "blanks with a background are part of the picture");
}
