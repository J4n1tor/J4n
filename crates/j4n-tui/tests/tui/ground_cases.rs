//! `theme.fill`: what paints the background. `match` asks the terminal to take the palette's background (the bytes
//! are `speed_cases`), `terminal` paints none, `paint` paints every cell as before.

use crate::world::*;
use j4n_tui::headless::Headless;
use j4n_tui::theme::{Fill, Settings, Theme};

fn theme(fill: &str, no_color: bool) -> Theme {
    Theme::build(&Settings { fill: fill.into(), no_color, ..Settings::default() }, &[]).expect("a theme")
}

#[test]
fn only_fill_match_asks_the_terminal_for_the_palettes_background() {
    let bg = theme("match", false).roles.get(j4n_tui::theme::palette::Role::Bg);
    assert_eq!(theme("match", false).ground(), Some((bg.0, bg.1, bg.2)));
    assert_eq!(theme("terminal", false).ground(), None);
    assert_eq!(theme("paint", false).ground(), None);
    assert_eq!(theme("match", true).ground(), None, "NO_COLOR leaves the terminal as it is");
    assert_eq!(Theme::default().fill, Fill::Match, "and `match` is the default");
}

#[test]
fn fill_terminal_paints_no_background_and_the_others_paint_every_cell() {
    let bg = |t: &str| {
        let mut a = app();
        a.theme = theme(t, false);
        let ansi = Headless::new(40, 12).snap(&mut a).ansi;
        let c = a.theme.roles.get(j4n_tui::theme::palette::Role::Bg);
        (ansi.contains(&format!("48;2;{};{};{}", c.0, c.1, c.2)), ansi.lines().filter(|l| l.contains("48;2;")).count())
    };
    assert_eq!(bg("terminal"), (false, 0), "not a cell has a background of its own");
    assert!(bg("match").0 && bg("paint").0, "the others paint the palette's");
}

#[test]
fn a_theme_that_names_a_word_the_screen_cannot_read_for_the_fill_is_refused() {
    let err = Theme::build(&Settings { fill: "grey".into(), ..Settings::default() }, &[]).expect_err("refused");
    assert!(err.ends_with("one of match, terminal, paint"), "{err}");
}
