//! The theme. The colour arithmetic, the twenty roles and their floors, the four tiers of glyphs, `NO_COLOR`, and what the
//! settings refuse.

use crate::world::{app, chat};
use j4n_proto::{Role as Who, Tone};
use j4n_tui::headless::Headless;
use j4n_tui::theme::color::{Rgb, pass, reach};
use j4n_tui::theme::glyph::{Slot, Tier};
use j4n_tui::theme::palette::{self, Inputs, Palette, Role};
use j4n_tui::theme::{Corners, Settings, Theme, You, sheet};
use ratatui::style::{Color, Modifier};

fn rgb(s: &str) -> Rgb {
    Rgb::parse(s).expect("a colour")
}

fn theme(f: impl FnOnce(&mut Settings)) -> Theme {
    let mut s = Settings::default();
    f(&mut s);
    Theme::build(&s, &[]).expect("a theme")
}

#[test]
fn colours_are_read_mixed_and_compared_as_the_design_computes_them() {
    assert_eq!(rgb("#161a20"), Rgb(22, 26, 32));
    assert_eq!(Rgb(22, 26, 32).hex(), "#161a20");
    for bad in ["161a20", "#16a", "#gggggg", "#161a2000", "#ééé"] {
        assert_eq!(Rgb::parse(bad), None, "{bad}");
    }
    let (black, white) = (Rgb(0, 0, 0), Rgb(255, 255, 255));
    assert_eq!(black.mix(white, 0.0), black);
    assert_eq!(black.mix(white, 1.0), white);
    assert_eq!(black.mix(white, 0.5), Rgb(128, 128, 128), "a half is rounded up");
    assert_eq!(Rgb(10, 20, 30).mix(Rgb(20, 40, 60), 0.25), Rgb(13, 25, 38));
    assert!((black.contrast(white) - 21.0).abs() < 1e-9);
    assert!((white.contrast(white) - 1.0).abs() < 1e-9);
    assert!((white.contrast(black) - black.contrast(white)).abs() < 1e-12, "the ratio does not care which is which");
    assert!((Rgb(0, 0, 0).luminance()).abs() < 1e-12 && (white.luminance() - 1.0).abs() < 1e-9);
    // the branch of the transfer curve at the foot (0.03928) and above it
    assert!((Rgb(10, 10, 10).luminance() - 10.0 / 255.0 / 12.92).abs() < 1e-9);
    assert!((Rgb(128, 128, 128).luminance() - 0.2159).abs() < 1e-3);
}

#[test]
fn a_hue_is_turned_and_a_grey_is_left_alone() {
    assert_eq!(Rgb(255, 0, 0).rotate(120.0), Rgb(0, 255, 0));
    assert_eq!(Rgb(255, 0, 0).rotate(240.0), Rgb(0, 0, 255));
    assert_eq!(Rgb(255, 0, 0).rotate(60.0), Rgb(255, 255, 0));
    assert_eq!(Rgb(255, 0, 0).rotate(-60.0), Rgb(255, 0, 255));
    assert_eq!(Rgb(0, 255, 0).rotate(-120.0), Rgb(255, 0, 0));
    assert_eq!(Rgb(0, 0, 255).rotate(120.0), Rgb(255, 0, 0));
    assert_eq!(Rgb(100, 100, 100).rotate(90.0), Rgb(100, 100, 100));
    // red above blue: the hue is on the far side of red, between magenta and red
    assert_eq!(Rgb(255, 0, 128).rotate(0.0), Rgb(255, 0, 128));
    assert_eq!(Rgb(255, 0, 128).rotate(30.0), Rgb(255, 0, 0));
    assert_eq!(Rgb(255, 0, 128).rotate(-30.0), Rgb(255, 0, 255));
    let c = rgb("#7caede");
    let back = c.rotate(60.0).rotate(-60.0);
    assert!(c.0.abs_diff(back.0) <= 1 && c.1.abs_diff(back.1) <= 1 && c.2.abs_diff(back.2) <= 1, "{c:?} {back:?}");
}

#[test]
fn the_pass_pulls_a_colour_back_until_it_can_be_read_and_stops_at_the_colour() {
    let (text, bg) = (rgb("#2a2e35"), rgb("#f3f0e8"));
    assert!(text.mix(bg, 0.41).contrast(bg) < 4.5, "Paper's dim is 3.6:1 without the pass");
    assert!(pass(text, &[bg], 0.41, 4.5).contrast(bg) >= 4.5);
    assert_eq!(pass(text, &[bg], 0.0, 4.5), text, "no distance, no change");
    let (accent, dark) = (rgb("#7caede"), rgb("#161a20"));
    assert!(accent.mix(dark, 0.26).contrast(dark) < 4.7, "JJJ's soft is 4.68:1 without the pass");
    assert!(pass(accent, &[dark], 0.26, 4.7).contrast(dark) >= 4.7);
    assert_eq!(pass(rgb("#555555"), &[rgb("#4a4a4a")], 0.5, 21.0), rgb("#555555"), "a colour that cannot reach the floor is itself");
}

#[test]
fn the_roles_of_jjj_are_the_ones_the_board_computes() {
    let r = palette::builtin().remove(0).roles();
    let hex = |role: Role| r.get(role).hex();
    assert_eq!(hex(Role::Bg), "#161a20");
    assert_eq!(hex(Role::Raise), "#1d242d");
    assert_eq!(hex(Role::Sel), "#26323e");
    assert_eq!(hex(Role::Edge), "#494e54");
    assert_eq!(hex(Role::You), hex(Role::Accent));
    assert_eq!(hex(Role::Focus), hex(Role::Accent));
    assert_eq!(hex(Role::Made), hex(Role::Faint));
    assert_eq!(hex(Role::Add), hex(Role::Ok));
    assert_eq!(hex(Role::Del), hex(Role::Error));
    assert_eq!(hex(Role::Route), "#b199d7");
    assert_eq!(Role::ALL.len(), 24);
    assert_eq!(Role::ALL.map(Role::name).iter().collect::<std::collections::BTreeSet<_>>().len(), 24);
    for role in Role::ALL {
        assert_eq!(Role::parse(role.name()), Some(role));
    }
}

#[test]
fn every_palette_of_the_design_meets_its_floors() {
    let all = palette::builtin();
    assert_eq!(all.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["jjj", "dusk", "paper", "ember", "mono", "contrast"]);
    for p in &all {
        assert_eq!(p.short(), Vec::<String>::new(), "{}", p.id);
        let (r, bg) = (p.roles(), p.roles().get(Role::Bg));
        assert!(r.get(Role::Dim).contrast(bg) >= 4.5, "{}: dim", p.id);
        assert!(r.get(Role::Faint).contrast(bg) >= 3.0, "{}: faint", p.id);
        assert!(r.get(Role::Soft).contrast(bg) >= 4.7, "{}: soft", p.id);
        assert!(r.get(Role::Text).contrast(bg) >= 7.0, "{}: text", p.id);
        // the floors hold on the raised composer and the selected row too, not on the background only
        for (role, floor) in [(Role::Dim, 4.5), (Role::Faint, 3.0), (Role::Soft, 4.7), (Role::Accent, 4.5), (Role::Ok, 4.5), (Role::Warn, 4.5), (Role::Error, 4.5), (Role::Route, 4.5), (Role::Run, 4.5), (Role::Fetch, 4.5)] {
            let (got, on) = r.worst(role);
            assert!(got >= floor, "{}: {} is {got:.2}:1 on {on}, the floor is {floor}", p.id, role.name());
        }
        assert!(Theme::build(&Settings { palette: p.id.clone(), ..Settings::default() }, &[]).is_ok(), "{}", p.id);
    }
}

#[test]
fn a_palette_names_three_colours_less_and_the_rest_is_turned_out_of_them() {
    let inputs = Inputs { bg: rgb("#101010"), text: rgb("#e0e0e0"), accent: rgb("#4488cc"), ok: rgb("#66bb66"), warn: rgb("#ccaa44"), error: rgb("#cc5555"), route: None, run: None, fetch: None };
    let p = Palette::new("mine", inputs.clone());
    let r = p.roles();
    // turned out of the others, then lifted to the floor on bg, raise and sel when the turn leaves them too dim
    let lift = |c: Rgb| reach(c, inputs.text, &[inputs.bg, inputs.bg.mix(inputs.accent, 0.07), inputs.bg.mix(inputs.accent, 0.16)], 4.5);
    assert_eq!(r.get(Role::Route), lift(inputs.accent.rotate(60.0)));
    assert_eq!(r.get(Role::Run), lift(inputs.warn.rotate(-20.0)));
    assert_eq!(r.get(Role::Fetch), lift(inputs.accent.mix(inputs.ok, 0.5)));
    assert_ne!(r.get(Role::Route), inputs.accent.rotate(60.0), "the turned hue was too dark on sel");
    let given = Palette::new("given", Inputs { route: Some(rgb("#a0b0c0")), run: Some(rgb("#c0b0a0")), fetch: Some(rgb("#a0c0b0")), ..inputs.clone() }).roles();
    assert_eq!((given.get(Role::Route).hex(), given.get(Role::Run).hex(), given.get(Role::Fetch).hex()), ("#a0b0c0".into(), "#c0b0a0".into(), "#a0c0b0".into()));
    let mut pinned = Palette::new("pinned", inputs);
    pinned.pins.insert(Role::Sel, rgb("#253040"));
    pinned.derive.raise = 0.5;
    assert_eq!(pinned.roles().get(Role::Sel).hex(), "#253040", "a role set by hand wins");
    assert_ne!(pinned.roles().get(Role::Raise), p.roles().get(Role::Raise), "a ratio is data");
}

#[test]
fn a_palette_that_cannot_be_read_is_refused_with_the_roles_that_fall_short() {
    let weak = Palette::new("weak", Inputs { bg: rgb("#777777"), text: rgb("#808080"), accent: rgb("#858585"), ok: rgb("#888888"), warn: rgb("#888888"), error: rgb("#888888"), route: None, run: None, fetch: None });
    let short = weak.short();
    assert_eq!(short.len(), 10, "{short:?}");
    assert!(short[0].starts_with("dim: ") && short[1].starts_with("faint: ") && short[2].starts_with("soft: ") && short[3].starts_with("accent: "), "{short:?}");
    let got = Theme::build(&Settings { palette: "weak".into(), ..Settings::default() }, &[weak]).expect_err("refused");
    assert!(got.starts_with("palette `weak` cannot be read: dim: "), "{got}");
    let mine = palette::builtin().remove(0);
    let own = Palette { id: "jjj".into(), ..Palette::new("x", Inputs { accent: rgb("#ff00ff"), ..mine.inputs.clone() }) };
    let built = Theme::build(&Settings::default(), &[own]).expect("a palette of the operator's");
    assert_eq!(built.roles.get(Role::Accent).hex(), "#ff00ff", "the operator's palette of the same id comes first");
}

#[test]
fn a_setting_that_is_not_one_of_the_words_is_refused_and_the_words_are_named() {
    let bad = |f: fn(&mut Settings, String), word: &str| {
        let mut s = Settings::default();
        f(&mut s, word.into());
        Theme::build(&s, &[]).expect_err(word)
    };
    let got = bad(|s, w| s.palette = w, "neon");
    assert!(got.starts_with("theme.palette is `neon`; it is one of jjj, dusk, paper, ember, mono, contrast"), "{got}");
    assert!(bad(|s, w| s.marks = w, "emoji").ends_with("one of icons, unicode, ascii, words"));
    assert!(bad(|s, w| s.frame = w, "box").ends_with("one of pipes, gutter, hang, round, airy"));
    assert!(bad(|s, w| s.corners = w, "oval").ends_with("one of round, square"));
    assert!(bad(|s, w| s.title_place = w, "top").ends_with("one of inside, border, none"));
    assert!(bad(|s, w| s.density = w, "tight").ends_with("one of compact, normal, roomy"));
    let t = theme(|s| {
        s.frame = "hang".into();
        s.title_place = "none".into();
        s.density = "roomy".into();
        s.motion = false;
    });
    assert_eq!((t.frame.word(), t.title_place.word(), t.density.word(), t.motion), ("hang", "none", "roomy", false));
    assert_eq!(Theme::default().palette, "jjj");
    assert_eq!(Theme::default().tier, Tier::Icons);
}

#[test]
fn every_slot_has_a_glyph_in_every_tier() {
    assert_eq!(Slot::ALL.len(), 130);
    let names: std::collections::BTreeSet<_> = Slot::ALL.iter().map(|s| s.name()).collect();
    assert_eq!(names.len(), Slot::ALL.len(), "a slot name is used once");
    for slot in Slot::ALL {
        for tier in Tier::ALL {
            let g = slot.cell(tier);
            assert!(!g.is_empty(), "{} has no glyph in {}", slot.name(), tier.name());
            match tier {
                Tier::Ascii => assert!(g.is_ascii(), "{} in ascii is `{g}`", slot.name()),
                Tier::Unicode | Tier::Words => assert!(g.chars().all(|c| c < '\u{E000}'), "{} in {} is a font icon", slot.name(), tier.name()),
                Tier::Icons => assert!(g.chars().all(|c| c < '\u{E000}' || ('\u{F0000}'..='\u{FFFFD}').contains(&c)), "{} in icons is `{g}`", slot.name()),
            }
        }
    }
    for tier in Tier::ALL {
        assert_eq!(Tier::parse(tier.name()), Some(tier));
    }
    assert_eq!(Tier::parse("emoji"), None);
}

#[test]
fn the_tiers_hold_what_the_marks_board_shows() {
    let cell = |s: Slot| Tier::ALL.map(|t| s.cell(t));
    assert_eq!(cell(Slot::You), ["❯", "❯", ">", "you"], "no person mark");
    assert_eq!(cell(Slot::Done), ["\u{F05E1}", "✓", "v", "✓"]);
    assert_eq!(cell(Slot::Failed), ["\u{F015A}", "✗", "x", "✗"]);
    assert_eq!(cell(Slot::Output), ["\u{F09ED}", "¶", "|", "out"]);
    assert_eq!(cell(Slot::Message), ["\u{F01EE}", "◫", "m", "msg"], "the envelope, m3");
    assert_eq!(cell(Slot::Spark), ["▁▃▅█", "▁▃▅█", "_.-#", "▁▃▅█"]);
    assert_eq!(cell(Slot::PipeElbow)[2], "`-");
    assert_eq!(cell(Slot::HangBar), ["┆", "┆", ":", "┆"]);
    assert_eq!(cell(Slot::Working)[3], "●", "the words tier says a lane that works by its name breathing; the bar and the cells keep the dot");
    for slot in [Slot::PipeBar, Slot::Focus, Slot::BoxTl, Slot::FillFull, Slot::KeyCtrl, Slot::KeyArrows] {
        assert_eq!(slot.cell(Tier::Words), slot.cell(Tier::Unicode), "{}: the words tier takes the unicode bricks and keys", slot.name());
    }
    assert_eq!((Slot::Done.role(), Slot::Failed.role(), Slot::Run.role(), Slot::Made.role()), (Role::Ok, Role::Error, Role::Run, Role::Made));
}

#[test]
fn the_numbers_of_the_tabs_have_four_twins_one_cell_a_number() {
    let cell = |t| Slot::TabNumber.cell(t);
    assert_eq!(Tier::ALL.map(cell), ["¹²³⁴⁵⁶⁷⁸⁹", "¹²³⁴⁵⁶⁷⁸⁹", "123456789", "123456789"], "small where a tier has them, plain digits in ascii and words");
    for tier in Tier::ALL {
        assert!(cell(tier).chars().count() == 9 && cell(tier).chars().all(|c| unicode_width::UnicodeWidthChar::width(c) == Some(1)), "{}", tier.name());
    }
    assert_eq!((Slot::by_name("tab-number"), Slot::TabNumber.role()), (Some(Slot::TabNumber), Role::Dim));
}

#[test]
fn in_the_unicode_tier_a_tick_has_a_pair_of_its_own_and_the_stopped_the_queued_and_the_done_marks_keep_one_meaning() {
    // `■` was Stopped and a checked tick, `○` Queued and an empty tick
    let (on, off) = (Slot::TickOn.cell(Tier::Unicode), Slot::TickOff.cell(Tier::Unicode));
    assert_eq!((on, off), ("☑", "☐"));
    let uses = |g: &str| Slot::ALL.iter().filter(|s| s.cell(Tier::Unicode) == g).map(|s| s.name()).collect::<Vec<_>>();
    assert_eq!(uses("■"), ["stopped"]);
    assert_eq!(uses("☑"), ["tick-on"]);
    assert_eq!(uses("☐"), ["tick-off"]);
    assert!(!uses("○").iter().any(|n| n.starts_with("tick")), "an empty tick is not the queued circle");
    assert_eq!(unicode_width::UnicodeWidthStr::width(on).max(unicode_width::UnicodeWidthStr::width(off)), 1, "a tick is one cell wide");
}

#[test]
fn in_the_unicode_tier_a_todo_is_not_a_box_so_it_is_never_taken_for_an_empty_tick() {
    // `□` (a todo, an open task) and `☐` (an empty tick) were one hair apart in most fonts
    let uses = |g: &str| Slot::ALL.iter().filter(|s| s.cell(Tier::Unicode) == g).map(|s| s.name()).collect::<Vec<_>>();
    assert_eq!(uses("◌"), ["todo", "task-open"], "the open item: one shape, no other meaning");
    assert_eq!(uses("☐"), ["tick-off"]);
    assert!(uses("□").is_empty(), "no slot is the box that looks like the tick");
    assert!(!uses("○").iter().any(|n| n.starts_with("todo") || n.starts_with("task")), "a todo is not the queued circle");
    assert_eq!(unicode_width::UnicodeWidthStr::width("◌"), 1, "one cell wide");
}

#[test]
fn in_the_unicode_tier_a_glyph_stands_for_one_thing_where_two_meanings_share_a_screen() {
    // `▣` done task and Packs tab, `≡` Read, a choice and the Log tab, `⊙` Search and a score, `¶` Output, a text question and the Chat tab,
    // `◈` Tool and Models, `⊕` New and Add, `●` Commit and an on switch, `▲` Error and a rise; `○` Queued, an off switch and an empty
    // radio, `◉` Connector and a radio on, `▸` a fold shut and a folder, `▾` a fold open and a sort down
    let mut by: std::collections::BTreeMap<&str, Vec<&str>> = std::collections::BTreeMap::new();
    for s in Slot::ALL {
        by.entry(s.cell(Tier::Unicode)).or_default().push(s.name());
    }
    // what is left on purpose: one meaning in two slots (a tick, a failure, an open item, a bar, a rule); a queued row, an empty radio, a
    // connector, a folder and a sort arrow have a glyph of their own
    let same: &[&[&str]] = &[
        &["done", "todo-done", "yes"],
        // the Decider is one icon, told on from off by its hue and the word `off`
        &["decide", "decide-off"],
        &["failed", "no"],
        &["todo", "task-open"],
        // the arrow of what nests in the airy look is the chevron; the left cap of your line is the half block the work bar fills with
        &["airy-child", "step-right", "chevron"],
        &["you-cap-l", "fill-half"],
        &["mask", "bullet"],
        &["hint", "key-tab"],
        &["pipe-bar", "box-v"],
        &["branch-elbow", "box-bl"],
        // the square end of the pipes look is the square corner of a box
        &["branch-square", "pop-bl"],
        &["focus", "thumb"],
        &["box-h", "delta-flat", "rule"],
        &["fill-right", "caret", "quote-bar"],
    ];
    for (g, names) in &by {
        assert!(names.len() == 1 || same.contains(&names.as_slice()), "{g} stands for {names:?}");
    }
    for name in ["task-done", "packs", "read", "log", "ask-choice", "search", "ask-score", "output", "ask-text", "chat", "tool", "models", "new", "add", "commit", "bool-on", "error", "delta-up", "queued", "bool-off", "radio-off", "radio-on", "connector", "fold-closed", "folder", "fold-open", "sort-up", "sort-down"] {
        let g = Slot::by_name(name).expect("a slot").cell(Tier::Unicode);
        assert_eq!(by[g], [name], "{name}");
        assert_eq!(unicode_width::UnicodeWidthStr::width(g), 1, "{name}: one cell wide");
    }
    // the icons tier draws a fold and a sort arrow as letters too; the two are not one drawing there either
    let icons = |n: &str| Slot::by_name(n).expect("a slot").cell(Tier::Icons);
    assert_ne!(icons("fold-open"), icons("sort-down"));
    assert_ne!(icons("fold-closed"), icons("folder"));
    for name in ["sort-up", "sort-down", "folder", "connector", "queued", "radio-off"] {
        let g = Slot::by_name(name).expect("a slot").cell(Tier::Unicode);
        assert!(!["○", "●", "◉", "▸", "▾", "↓", "↑", "◌", "☐", "☑", "⊙", "…"].contains(&g), "{name} is not another slot's glyph: {g}");
    }
}

#[test]
fn corners_round_or_square_and_an_icon_takes_two_cells() {
    let round = theme(|_| {});
    let square = theme(|s| s.corners = "square".into());
    assert_eq!((round.glyph(Slot::BoxTl), round.glyph(Slot::BoxBr)), ("╭", "╯"));
    assert_eq!((square.glyph(Slot::BoxTl), square.glyph(Slot::BoxTr), square.glyph(Slot::BoxBl), square.glyph(Slot::BoxBr)), ("┌", "┐", "└", "┘"));
    assert_eq!(square.corners, Corners::Square);
    assert_eq!(square.glyph(Slot::PopTl), "┌", "a popup is square either way");
    assert_eq!(round.glyph(Slot::PopTl), "┌");
    assert_eq!(round.glyph(Slot::Done), "\u{F05E1}");
    assert_eq!(round.mark(Slot::Done).0, "\u{F05E1} ", "an icon is a slot of two cells");
    assert_eq!(round.mark(Slot::FoldOpen).0, "▾", "a letter of the font is one");
    let ascii = theme(|s| s.marks = "ascii".into());
    assert_eq!(ascii.mark(Slot::Done).0, "v");
}

#[test]
fn without_colour_the_roles_are_attributes_and_the_glyphs_stay() {
    let c = theme(|_| {});
    let n = theme(|s| s.no_color = true);
    let error = c.roles.get(Role::Error);
    assert_eq!(c.tone(Tone::Bad).fg, Some(Color::Rgb(error.0, error.1, error.2)));
    assert_eq!(n.tone(Tone::Bad).fg, None);
    assert!(n.tone(Tone::Bad).add_modifier.contains(Modifier::BOLD | Modifier::UNDERLINED));
    assert!(n.tone(Tone::Warn).add_modifier.contains(Modifier::UNDERLINED) && !n.tone(Tone::Warn).add_modifier.contains(Modifier::BOLD));
    assert!(n.tone(Tone::Dim).add_modifier.contains(Modifier::DIM));
    assert!(n.tone(Tone::Accent).add_modifier.contains(Modifier::BOLD));
    assert_eq!(n.tone(Tone::Good).add_modifier, Modifier::empty());
    assert_eq!(n.tone(Tone::Plain), ratatui::style::Style::default());
    assert!(n.back(Role::Sel).add_modifier.contains(Modifier::REVERSED) && n.back(Role::Sel).bg.is_none());
    assert_eq!(n.back(Role::Raise), ratatui::style::Style::default());
    assert!(c.back(Role::Sel).bg.is_some() && !c.back(Role::Sel).add_modifier.contains(Modifier::REVERSED));
    assert_eq!(n.base(), ratatui::style::Style::default(), "the terminal's own colours");
    assert_eq!(c.base().bg, Some(Color::Rgb(22, 26, 32)));
    assert!(c.fg(Role::You).add_modifier.contains(Modifier::BOLD) && c.fg(Role::Focus).add_modifier.contains(Modifier::BOLD));
    assert_eq!(c.fg(Role::Accent).add_modifier, Modifier::empty());
    assert_eq!(n.glyph(Slot::Done), c.glyph(Slot::Done), "NO_COLOR keeps the glyphs");
}

#[test]
fn the_screen_is_painted_with_the_theme_and_not_at_all_without_colour() {
    let mut a = app();
    // the operator's line is what is bold without colour: `theme.you = mark` has no icon on the pipes and the band has none
    a.theme = theme(|s| s.you = "mark".into());
    chat(&mut a, &[(Who::You, "hello"), (Who::Doer, "hi"), (Who::Error, "no")]);
    let c = Headless::new(60, 12).snap(&mut a);
    assert!(c.ansi.contains("48;2;22;26;32"), "the background of jjj is under every cell");
    assert!(c.ansi.contains("38;2;205;210;216"), "and the text colour on it");
    a.theme = theme(|s| (s.no_color, s.you) = (true, "mark".into()));
    let n = Headless::new(60, 12).snap(&mut a);
    assert!(!n.ansi.contains("38;2") && !n.ansi.contains("48;2") && !n.ansi.contains("38;5"), "no colour at all: {:?}", n.ansi);
    assert!(n.ansi.contains("\x1b[0;1mhello") && n.ansi.contains("\x1b[0;2m"), "your line is bold and the marks of the others dim: {:?}", n.ansi);
    assert_eq!(c.text, n.text, "the same words in the same places");
    a.theme = theme(|s| s.palette = "paper".into());
    assert!(Headless::new(60, 12).snap(&mut a).ansi.contains("48;2;243;240;232"), "another palette, another ground");
}

#[test]
fn the_roles_page_and_the_glyphs_page_show_everything() {
    let t = theme(|_| {});
    let roles = sheet::roles(&t);
    assert_eq!(roles.len(), 25, "a title and a line a role");
    let text = |l: &ratatui::text::Line| l.spans.iter().map(|s| s.content.to_string()).collect::<String>();
    assert!(text(&roles[0]).contains("roles · jjj"));
    assert!(text(&roles[5]).contains("dim") && text(&roles[5]).contains("✓"), "{}", text(&roles[5]));
    let n = theme(|s| s.no_color = true);
    assert!(sheet::roles(&n).iter().any(|l| text(l).contains("reverse")), "the attribute of each role is said");
    for tier in Tier::ALL {
        let t = theme(|s| s.marks = tier.name().into());
        let page: String = sheet::glyphs(&t, 100).iter().map(text).collect::<Vec<_>>().join("\n");
        for slot in Slot::ALL {
            assert!(page.contains(slot.name()), "{} is missing from {}", slot.name(), tier.name());
        }
        assert!(sheet::glyphs(&t, 30).len() > sheet::glyphs(&t, 100).len(), "a narrow screen has fewer in a row");
    }
}

#[test]
fn no_color_in_the_environment_is_set_and_not_empty() {
    use std::ffi::OsStr;
    let off = |v: Option<&str>| Settings::with_no_color_var(v.map(OsStr::new)).no_color;
    assert_eq!((off(None), off(Some("")), off(Some("1")), off(Some("0")), off(Some("anything"))), (false, false, true, true, true));
    assert_eq!(Settings::with_no_color_var(None), Settings::default());
}

#[test]
fn colorterm_says_whether_the_terminal_draws_any_colour_and_the_charts_follow_the_setting() {
    use std::ffi::OsStr;
    let on = |v: Option<&str>| Settings::default().with_colorterm(v.map(OsStr::new)).truecolor;
    assert_eq!((on(Some("truecolor")), on(Some("24bit")), on(Some("")), on(Some("yes")), on(None)), (true, true, false, false, false));
    assert!(Settings::default().truecolor, "the default screen, as the goldens have it, has truecolor");
    let theme = |chart: &str| Theme::build(&Settings { chart: chart.into(), ..Settings::default() }, &[]).unwrap();
    assert!(theme("dither").dither && !theme("plain").dither);
}

#[test]
fn one_role_short_of_its_floor_is_enough_to_refuse_a_palette_and_a_pin_does_not_move_a_tone() {
    let mut p = palette::builtin().remove(0);
    p.id = "strict".into();
    p.floors.dim = 21.0;
    let got = Theme::build(&Settings { palette: "strict".into(), ..Settings::default() }, &[p]).expect_err("dim cannot reach 21:1");
    assert!(got.starts_with("palette `strict` cannot be read: dim: ") && !got.contains("faint"), "{got}");
    // add and del are ok and error until a hand says otherwise; a tone of the protocol is ok and error, not add and del
    let mut q = palette::builtin().remove(0);
    q.id = "pinned".into();
    q.pins.insert(Role::Add, rgb("#010101"));
    q.pins.insert(Role::Del, rgb("#020202"));
    let t = Theme::build(&Settings { palette: "pinned".into(), ..Settings::default() }, &[q]).expect("a palette");
    let (ok, error) = (t.roles.get(Role::Ok), t.roles.get(Role::Error));
    assert_eq!(t.tone(Tone::Good).fg, Some(Color::Rgb(ok.0, ok.1, ok.2)));
    assert_eq!(t.tone(Tone::Bad).fg, Some(Color::Rgb(error.0, error.1, error.2)));
}

// ---- the look the core names (`Down::Theme`) ----

fn look(frame: &str, marks: &str) -> j4n_proto::Look {
    j4n_proto::Look { palette: "jjj".into(), marks: marks.into(), frame: frame.into(), corners: "round".into(), title_place: "inside".into(), density: "normal".into(), fill: "match".into(), you: "band".into(), motion: true, mouse: "on".into(), read: Default::default(), time: "ago".into(), chart: "dither".into() }
}

#[test]
fn the_look_the_core_names_is_the_look_the_screen_draws() {
    let mut a = app();
    assert_eq!((a.theme.frame, a.theme.tier), (j4n_tui::theme::Frame::Round, Tier::Icons), "until it comes the screen has the design's");
    a.down(j4n_proto::Down::Theme { look: look("gutter", "words") });
    assert_eq!((a.theme.frame, a.theme.tier), (j4n_tui::theme::Frame::Gutter, Tier::Words));
    assert!(a.take_redraw(), "a new look is drawn at once");
    chat(&mut a, &[(Who::You, "hi"), (Who::Doer, "hello")]);
    let drawn = j4n_tui::headless::Headless::new(60, 12).show(&mut a);
    assert!(drawn.lines().any(|l| l.trim_start().starts_with("you") && l.contains("hi")), "the turn's word stands in the gutter:\n{drawn}");
}

#[test]
fn a_look_the_screen_cannot_read_leaves_the_look_as_it_was_and_says_why() {
    let mut a = app();
    a.down(j4n_proto::Down::Theme { look: look("gutter", "unicode") });
    let mut bad = look("tree", "unicode");
    bad.palette = "jjj".into();
    a.down(j4n_proto::Down::Theme { look: bad });
    assert_eq!(a.theme.frame, j4n_tui::theme::Frame::Gutter, "the look it had stands");
    let (text, tone) = a.toast.clone().expect("a toast");
    assert!(text.contains("theme.frame is `tree`") && text.contains("pipes, gutter, hang"), "{text}");
    assert_eq!(tone, Tone::Bad);
    let mut bad = look("pipes", "unicode");
    bad.palette = "neon".into();
    a.down(j4n_proto::Down::Theme { look: bad });
    assert!(a.toast.as_ref().is_some_and(|(t, _)| t.contains("theme.palette is `neon`")), "{:?}", a.toast);
}

#[test]
fn the_colour_the_terminal_asked_for_is_not_the_cores_to_give_back() {
    let mut a = app();
    a.theme = Theme::build(&Settings { no_color: true, ..Settings::default() }, &[]).expect("a theme");
    a.down(j4n_proto::Down::Theme { look: look("hang", "ascii") });
    assert!(a.theme.no_color, "NO_COLOR stays on");
    assert_eq!((a.theme.frame, a.theme.tier), (j4n_tui::theme::Frame::Hang, Tier::Ascii));
    let mut still = look("pipes", "icons");
    still.motion = false;
    a.down(j4n_proto::Down::Theme { look: still });
    assert!(!a.theme.motion, "a look without motion is a screen that stands still");
}

/// What the core checks a `:look` against and what the screen reads are one list: every word the protocol names builds a theme, and every
/// word the screen knows is a word the protocol names (so a word the core lets through is never one the screen refuses).
#[test]
fn the_words_of_the_look_the_protocol_names_are_the_words_the_screen_reads() {
    use j4n_tui::theme::{Density, Fill, Frame, TitlePlace};
    let screen: [(&str, Vec<String>); 9] = [
        ("palette", palette::builtin().into_iter().map(|p| p.id).collect()),
        ("marks", Tier::ALL.iter().map(|t| t.name().to_string()).collect()),
        ("frame", Frame::WORDS.iter().map(|w| w.to_string()).collect()),
        ("corners", Corners::WORDS.iter().map(|w| w.to_string()).collect()),
        ("title_place", TitlePlace::WORDS.iter().map(|w| w.to_string()).collect()),
        ("density", Density::WORDS.iter().map(|w| w.to_string()).collect()),
        ("fill", Fill::WORDS.iter().map(|w| w.to_string()).collect()),
        ("you", You::WORDS.iter().map(|w| w.to_string()).collect()),
        ("motion", vec!["true".into(), "false".into()]),
    ];
    for ((part, words), (named, known)) in j4n_proto::Look::PARTS.iter().zip(screen) {
        assert_eq!(*part, named);
        assert_eq!(words.iter().map(|w| w.to_string()).collect::<Vec<_>>(), known, "{part}");
        for word in *words {
            let look = look("pipes", "icons").with(part, word);
            assert_eq!(look.word(part).as_deref(), Some(*word), "{part} {word}: the look says it");
            let mut a = app();
            a.down(j4n_proto::Down::Theme { look });
            assert!(a.toast.is_none(), "{part} {word}: the screen refused a word the protocol names: {:?}", a.toast);
        }
    }
}

#[test]
fn a_part_or_a_word_that_is_not_one_leaves_the_look_as_it_was() {
    let was = look("gutter", "words");
    assert_eq!(was.clone().with("motion", "maybe"), was, "a word that is not a switch");
    assert_eq!(was.clone().with("hue", "red"), was, "a part that is not one");
    assert!(!was.clone().with("motion", "false").motion);
    assert_eq!(was.word("hue"), None);
}
