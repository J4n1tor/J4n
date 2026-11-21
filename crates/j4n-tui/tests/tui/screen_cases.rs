//! What is drawn, character for character, into a screen in memory.

use crate::world::*;
use j4n_proto::{Cell, Down, Intent, Role, Row, Shape, Tone};
use j4n_tui::headless::{Headless, typed};

#[test]
fn before_the_core_has_sent_the_tree_the_screen_says_it_is_waiting() {
    let mut app = j4n_tui::App::new();
    assert_eq!(show(&mut app, 30, 3).lines().next(), Some("waiting for the core…"));
}

#[test]
fn the_chat_page_has_the_tabs_the_transcript_the_composer_and_the_bar() {
    let mut app = app();
    chat(&mut app, &[(Role::You, "what does main.rs do?"), (Role::Doer, "It prints a greeting.")]);
    rows(&mut app, "bar:status", vec![Row { key: "status".into(), cells: vec![Cell::new("alpha · haiku", Tone::Accent), Cell::new("idle", Tone::Dim), Cell::new("", Tone::Plain), Cell::new("$0.0123", Tone::Dim)], ..Row::default() }]);
    let s = screen(&mut app, 50, 10);
    assert_eq!(s.len(), 10);
    assert!(s[0].is_empty(), "a blank row of room above the tabs: {:?}", s[0]);
    assert!(s[1].starts_with("  Chat ") && s[1].contains("Tasks") && s[1].contains("Log") && s[1].contains("Settings") && s[1].ends_with("j4n 0.0.0"), "{:?}", s[1]);
    let at = |what: &str| s.iter().position(|l| l.contains(what)).unwrap_or_else(|| panic!("{what:?} not in {s:?}"));
    let frame = s.iter().position(|l| l.starts_with('╭')).expect("the composer's frame");
    assert!(at("what does main.rs do?") < at("It prints a greeting.") && at("It prints a greeting.") < frame, "the turn, its answer, the composer: {s:?}");
    assert!(s[frame + 2].starts_with('╰') && frame + 3 < 10, "the composer is a frame of one line");
    assert!(s[9].starts_with(" alpha · haiku  idle  $0.0123"), "and the bar is the last of all: {s:?}");
}

#[test]
fn the_transcript_sticks_to_the_bottom_and_older_turns_scroll_off_the_top() {
    let mut app = app();
    let lines: Vec<(Role, String)> = (1..=30).map(|i| (Role::You, format!("line {i}"))).collect();
    let refs: Vec<(Role, &str)> = lines.iter().map(|(r, t)| (*r, t.as_str())).collect();
    chat(&mut app, &refs);
    still(&mut app);
    let s = screen(&mut app, 30, 12).join("\n");
    assert!(s.contains("line 30") && s.contains("line 28") && !s.contains("line 1 ") && !s.contains("line 20"), "{s}");
    app.key(j4n_tui::headless::press(crossterm::event::KeyCode::PageUp));
    let up = screen(&mut app, 30, 12).join("\n");
    assert!(!up.contains("line 30") && up.contains("line 27"), "half a page is half the height less six: {up}");
}

#[test]
fn a_short_transcript_stands_at_the_bottom_not_the_top() {
    let mut app = app();
    chat(&mut app, &[(Role::You, "only line")]);
    let s = screen(&mut app, 30, 10);
    let (at, frame) = (s.iter().position(|l| l.contains("only line")).expect("the line"), s.iter().position(|l| l.starts_with('╭')).expect("the frame"));
    assert_eq!(at + 1, frame, "it stands on the composer: {s:?}");
    assert!(s[3..at].iter().all(|l| l.is_empty()), "and nothing is above it: {s:?}");
}

#[test]
fn the_draft_is_shown_with_the_cursor_where_the_next_letter_goes() {
    let mut app = app();
    type_in(&mut app, "世ab");
    let mut h = Headless::new(30, 6);
    let s = h.show(&mut app);
    assert!(s.lines().nth(3).is_some_and(|l| l.starts_with("│ 世ab")), "{s}");
    assert_eq!(h.cursor(), (2 + 2 + 2, 3), "the frame and a space, a wide letter, two narrow ones");
    app.key(j4n_tui::headless::press(crossterm::event::KeyCode::Left));
    h.show(&mut app);
    assert_eq!(h.cursor().0, 5);
}

#[test]
fn an_open_approval_is_shown_in_a_box_with_its_effect_its_risk_and_its_answers() {
    let mut app = app();
    let mut a = ask(Shape::Approval);
    a.effect = "shell: make build".into();
    a.risk = 50;
    a.grants = vec!["session".into(), "here".into()];
    app.down(Down::Asks { open: vec![a] });
    let s = screen(&mut app, 80, 30);
    let at = s.iter().position(|l| l.starts_with('╭') && l.ends_with('╮')).unwrap_or_else(|| panic!("a box: {s:?}"));
    assert!(s[at + 1].starts_with("│ "), "{s:?}");
    let body = s[at..].join("\n");
    for want in ["t1", "may I run `make build`?", "shell: make build", "yes · this session", "yes · always here", "widens", "esc later"] {
        assert!(body.contains(want), "{want:?} not in\n{body}");
    }
    assert!(s.iter().any(|l| l.starts_with('╰') && l.ends_with('╯')), "{s:?}");
}

#[test]
fn two_questions_are_a_tray_with_a_row_for_each() {
    let mut app = app();
    let mut a = ask(Shape::Choice);
    a.question = "which colour?".into();
    a.options = options(&[("red", false), ("green", true)]);
    let mut b = a.clone();
    b.ask = "t3".into();
    b.question = "which size?".into();
    app.down(Down::Asks { open: vec![a, b] });
    let s = screen(&mut app, 60, 24).join("\n");
    assert!(s.contains("├ t1  which colour?") && !s.contains("├ t1  which size?") && s.contains("╰ t1  which size?"), "{s}");
    assert!(banded(&mut app, 60, 24).iter().any(|l| l.contains("├ t1  which colour?")), "the one the keys are on has the band, and keeps its elbow");
    assert!(s.contains("the red") && s.contains("the green") && !s.contains("~"), "{s}");
}

#[test]
fn a_text_question_says_to_type_and_the_prompt_changes_while_any_question_is_open() {
    let mut app = app();
    let mut a = ask(Shape::Text);
    a.question = "what name?".into();
    app.down(Down::Asks { open: vec![a] });
    let s = show(&mut app, 50, 12);
    assert!(s.contains("type the answer and send it") && s.contains("what name?"), "{s}");
    assert!(s.lines().any(|l| l.starts_with('╭') || l.starts_with('┌')) && s.contains('→'), "the composer is aimed at the question: {s}");
}

#[test]
fn a_toast_shows_just_above_the_bar_in_its_tone() {
    let mut app = app();
    app.down(Down::Toast { text: "nothing is bound as `x`".into(), tone: Tone::Warn });
    let s = screen(&mut app, 40, 10);
    let at = s.iter().position(|l| l.contains("nothing is bound as `x`")).unwrap_or_else(|| panic!("the toast: {s:?}"));
    assert!(s[at].starts_with("│ ") && s[at - 1].starts_with("╭") && s[at + 1].starts_with("╰"), "a round box of its own, padded by a cell: {s:?}");
    assert_eq!(at, 7, "the box ends just above the bar: {s:?}");
    // a screen too short for a box has the line alone
    let s = screen(&mut app, 40, 5);
    assert!(s[3].starts_with("nothing is bound as `x`"), "{s:?}");
}

#[test]
fn the_task_list_has_aligned_columns_a_marker_on_the_selected_row_and_its_keys() {
    let mut app = app();
    app.key(j4n_tui::headless::press(crossterm::event::KeyCode::F(2)));
    let mut r = row("t2", &["t2", "running", "claude", "fix the thing"]);
    r.actions = vec![j4n_proto::Act { key: "a".into(), label: "abort".into(), intent: Intent::Control { task: "t2".into(), control: j4n_proto::Control::Abort }, risk: 10 }];
    r.detail = vec![Cell::new("the answer so far", Tone::Dim)];
    rows(&mut app, "tasks:all", vec![r, row("t1", &["t1", "done", "claude", "look around"])]);
    let s = screen(&mut app, 60, 8);
    assert!(s[3].starts_with("├ t2  running  claude  fix the thing") && s[3].ends_with("⏎ abort"), "{s:?}");
    assert!(banded(&mut app, 60, 8).iter().any(|l| l.starts_with("├ t2")), "the band marks the row the keys are on");
    assert_eq!(s[4], "│   the answer so far");
    assert_eq!(s[5], "╰ t1  done     claude  look around");
}

#[test]
fn a_long_list_follows_the_selection_down_the_screen() {
    let mut app = app();
    app.key(j4n_tui::headless::press(crossterm::event::KeyCode::F(2)));
    rows(&mut app, "tasks:all", (0..20).map(|i| row(&format!("t{i}"), &[&format!("task {i}")])).collect());
    for _ in 0..12 {
        app.key(j4n_tui::headless::press(crossterm::event::KeyCode::Down));
    }
    let s = show(&mut app, 30, 8);
    assert!(banded(&mut app, 30, 8).iter().any(|l| l.contains("task 12")), "{s}");
    assert!(!s.contains("task 0\n"), "{s}");
}

#[test]
fn the_log_page_shows_its_lines_from_the_bottom() {
    let mut app = app();
    app.key(j4n_tui::headless::press(crossterm::event::KeyCode::F(3)));
    rows(&mut app, "log:all", (1..=3).map(|i| row(&format!("e{i}"), &[&i.to_string(), "task.settled t1"])).collect());
    let s = screen(&mut app, 40, 6);
    let shown: Vec<&str> = s[2..5].iter().map(|l| l.trim_start_matches(['┃', ' '])).collect();
    assert_eq!(shown, ["1 task.settled t1", "2 task.settled t1", "3 task.settled t1"], "{s:?}");
}

#[test]
fn the_form_shows_its_value_being_typed() {
    let mut app = app();
    app.key(j4n_tui::headless::press(crossterm::event::KeyCode::F(4)));
    let mut r = row("doer", &["doer", "alpha"]);
    r.actions = vec![j4n_proto::Act { key: "enter".into(), label: "change".into(), intent: Intent::Set { kind: "thread".into(), id: "main".into(), field: "doer".into(), value: j4n_proto::Json::Null }, risk: 0 }];
    rows(&mut app, "form:thread", vec![r]);
    app.key(j4n_tui::headless::press(crossterm::event::KeyCode::Enter));
    for k in typed("be", false) {
        app.key(k);
    }
    let s = screen(&mut app, 50, 8);
    assert_eq!(s[3], "╰ doer  alpha", "a list like the others; ⏎ is in the keys row, not in the row");
    assert_eq!(s[4], "doer: be▏");
}

#[test]
fn the_active_tab_is_marked_and_the_screen_survives_any_size() {
    let mut app = app();
    chat(&mut app, &[(Role::Doer, "a long line of text that cannot possibly fit in a very narrow screen")]);
    app.down(Down::Asks { open: vec![ask(Shape::Approval)] });
    app.down(Down::Toast { text: "t".repeat(200), tone: Tone::Bad });
    for (c, r) in [(1, 1), (2, 2), (10, 3), (5, 40), (200, 2), (80, 24), (3, 100)] {
        let s = show(&mut app, c, r);
        assert_eq!(s.split('\n').count(), r as usize, "{c}x{r}");
    }
    for n in [2, 3, 4, 1] {
        app.key(j4n_tui::headless::press(crossterm::event::KeyCode::F(n)));
        let _ = show(&mut app, 20, 5);
    }
}

#[test]
fn a_page_with_sections_titles_each_section_and_a_node_without_a_field_or_children_is_empty() {
    use j4n_proto::{FieldKind, Node, Present};
    let mut app = app();
    let sections = Node {
        id: "root".into(),
        present: Present::Sections,
        children: vec![leaf("a", "Alpha", FieldKind::Log, "log:a"), Node { id: "empty".into(), title: "Empty".into(), ..Node::default() }, leaf("b", "", FieldKind::Log, "log:b")],
        ..Node::default()
    };
    app.down(Down::Tree { node: sections });
    rows(&mut app, "log:a", vec![row("1", &["1", "in alpha"])]);
    let s = show(&mut app, 30, 9);
    assert!(s.contains("Alpha") && s.contains("in alpha") && s.contains("Empty"), "{s}");
}

#[test]
fn a_draft_of_several_lines_is_a_row_each_in_the_composer_with_its_keys_at_the_end_of_the_last() {
    let mut app = app();
    app.paste("one\ntwo");
    let s = screen(&mut app, 40, 9);
    let at = s.iter().position(|l| l.starts_with("│ one")).unwrap_or_else(|| panic!("{s:?}"));
    assert!(s[at + 1].starts_with("│ two") && s[at + 1].contains("⏎ send"), "{s:?}");
}

#[test]
fn a_draft_longer_than_the_line_wraps_in_the_composer_so_the_cursor_is_always_in_sight() {
    let mut app = app();
    type_in(&mut app, "0123456789abcdefghij");
    let mut h = Headless::new(12, 10);
    let s = h.show(&mut app);
    let inside: Vec<&str> = s.lines().filter(|l| l.starts_with("│ ")).collect();
    assert!(inside.len() >= 3 && inside.concat().contains("hij"), "{s}");
    let end = h.cursor();
    assert!(end.0 < 12 && usize::from(end.1) < 10, "{end:?}");
    for _ in 0..20 {
        app.key(j4n_tui::headless::press(crossterm::event::KeyCode::Left));
    }
    let s = h.show(&mut app);
    let first = s.lines().position(|l| l.starts_with("│ 0123456")).expect("the start of the draft");
    assert_eq!(h.cursor(), (2, first as u16), "the cursor comes back to the start with the keys: {s}");
}

#[test]
fn a_chat_of_thousands_of_turns_is_drawn_as_fast_as_a_screenful_and_scrolls_to_its_start() {
    let mut app = app();
    let all: Vec<(Role, String)> = (0..20_000).map(|i| (Role::You, format!("line {i} with a few more words so that it is long enough to wrap on a narrow screen"))).collect();
    let refs: Vec<(Role, &str)> = all.iter().map(|(r, t)| (*r, t.as_str())).collect();
    chat(&mut app, &refs);
    still(&mut app);
    let mut h = Headless::new(40, 20);
    let ((), cpu) = crate::cpu::of(|| {
        for _ in 0..20 {
            h.show(&mut app);
        }
    });
    let each = cpu / 20;
    crate::cpu::assert_within(std::time::Duration::from_millis(50), "a frame (the whole chat is not laid out)", || each);
    let s = h.show(&mut app);
    assert!(s.contains("line 19999"), "the end of the chat is shown: {s}");
    for _ in 0..16_000 {
        app.key(j4n_tui::headless::press(crossterm::event::KeyCode::PageUp));
    }
    let top = h.show(&mut app);
    assert!(top.contains("line 0 with") && !top.contains("line 19999"), "scrolling stops at the first turn: {top}");
}

#[test]
fn a_risk_of_fifty_or_more_is_in_warning_colour_and_a_lower_one_is_in_the_accent() {
    for (risk, tone) in [(49, 'c'), (50, 'y'), (80, 'r')] {
        let mut app = app();
        let mut a = ask(Shape::Approval);
        a.risk = risk;
        app.down(Down::Asks { open: vec![a] });
        let text = screen(&mut app, 80, 30);
        let tones = Headless::new(80, 30).tones(&mut app);
        let (row, line) = text.iter().enumerate().find(|(_, l)| l.contains('▕')).expect("the meter");
        // the first cell of the fill is just after the edge of the bar
        let col = line.chars().position(|c| c == '▕').expect("its edge") + 1;
        assert_eq!(tones[row].chars().nth(col), Some(tone), "risk {risk}: {line} {:?}", tones[row]);
    }
}

#[test]
fn only_the_selected_row_shows_its_keys_and_its_detail() {
    let mut app = app();
    app.key(j4n_tui::headless::press(crossterm::event::KeyCode::F(2)));
    let with = |key: &str| {
        let mut r = row(key, &[key, "running"]);
        r.actions = vec![j4n_proto::Act { key: "a".into(), label: "abort".into(), intent: Intent::Control { task: key.into(), control: j4n_proto::Control::Abort }, risk: 10 }];
        r.detail = vec![Cell::new(format!("detail of {key}"), Tone::Dim)];
        r
    };
    rows(&mut app, "tasks:all", vec![with("t2"), with("t1")]);
    let s = screen(&mut app, 60, 8);
    assert_eq!((s[3].contains("⏎ abort"), s[5].contains("⏎ abort")), (true, false), "{s:?}");
    assert!(s.iter().any(|l| l.contains("detail of t2")) && !s.iter().any(|l| l.contains("detail of t1")), "{s:?}");
}

#[test]
fn a_transcript_scrolled_past_its_start_stops_at_the_start_and_one_page_down_moves_it() {
    let mut app = app();
    let lines: Vec<(Role, String)> = (0..30).map(|i| (Role::You, format!("line {i}"))).collect();
    let refs: Vec<(Role, &str)> = lines.iter().map(|(r, t)| (*r, t.as_str())).collect();
    chat(&mut app, &refs);
    still(&mut app);
    app.size = (40, 10);
    for _ in 0..40 {
        app.key(j4n_tui::headless::press(crossterm::event::KeyCode::PageUp));
    }
    let top = show(&mut app, 40, 10);
    assert!(top.contains("line 0"), "{top}");
    app.key(j4n_tui::headless::press(crossterm::event::KeyCode::PageDown));
    let down = show(&mut app, 40, 10);
    assert_ne!(top, down, "the page that was asked for is a page, not the ones that were over the start");
}

fn nested() -> j4n_proto::Node {
    use j4n_proto::{FieldKind, Node, Present};
    let models = Node { id: "models".into(), title: "Models".into(), present: Present::Tabs, children: vec![leaf("trust", "Trust", FieldKind::List, "view:trust"), leaf("hosts", "Hosts", FieldKind::List, "view:hosts")], ..Node::default() };
    let pages = Node { id: "pages".into(), present: Present::Tabs, children: vec![leaf("chat", "Chat", FieldKind::Chat, "chat:main"), models, leaf("packs", "Packs", FieldKind::List, "packs:all")], ..Node::default() };
    Node { id: "root".into(), children: vec![pages, leaf("bar", "", FieldKind::Bar, "bar:status")], ..Node::default() }
}

fn watched(ups: &[j4n_proto::Up]) -> Vec<String> {
    ups.iter().filter_map(|u| if let j4n_proto::Up::Watch { bind } = u { Some(bind.clone()) } else { None }).collect()
}

#[test]
fn a_tab_that_holds_tabs_shows_its_own_row_of_them_and_the_keys_move_between_them_and_only_the_open_one_is_watched() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = app();
    app.down(Down::Tree { node: nested() });
    let key = |code, mods| KeyEvent::new(code, mods);
    app.key(key(KeyCode::F(2), KeyModifiers::NONE));
    let s = show(&mut app, 60, 8);
    let lines: Vec<&str> = s.lines().collect();
    assert!(lines[1].contains("Chat") && lines[1].contains("Models") && lines[1].contains("j4n"), "{s}");
    assert!(lines[2].contains("━") && lines[3].contains("Trust") && lines[3].contains("Hosts") && !lines[3].contains("j4n"), "the sub-tabs are a row of their own, under the rule of the tabs, without the name: {s}");
    assert_eq!(app.page().map(|p| p.id.as_str()), Some("trust"));
    let ups = app.key(key(KeyCode::Right, KeyModifiers::ALT));
    assert_eq!(app.page().map(|p| p.id.as_str()), Some("hosts"));
    assert_eq!(watched(&ups), ["view:hosts"], "{ups:?}");
    app.key(key(KeyCode::Right, KeyModifiers::ALT));
    assert_eq!(app.page().map(|p| p.id.as_str()), Some("trust"), "round the corner");
    app.key(key(KeyCode::Left, KeyModifiers::ALT));
    assert_eq!(app.page().map(|p| p.id.as_str()), Some("hosts"));
    // the arrows alone are not the sub-tabs' (a draft is edited with them), and a tab with no tabs in it ignores the keys
    app.key(key(KeyCode::F(3), KeyModifiers::NONE));
    assert_eq!(app.page().map(|p| p.id.as_str()), Some("packs"));
    assert!(app.key(key(KeyCode::Right, KeyModifiers::ALT)).is_empty());
}

#[test]
fn the_core_can_send_the_screen_to_a_page_even_one_in_a_tab_of_tabs() {
    let mut app = app();
    app.down(Down::Tree { node: nested() });
    let ups = app.down(Down::Focus { node: "hosts".into() });
    assert_eq!(app.page().map(|p| p.id.as_str()), Some("hosts"));
    assert_eq!(watched(&ups), ["view:hosts"]);
    app.down(Down::Focus { node: "packs".into() });
    assert_eq!(app.page().map(|p| p.id.as_str()), Some("packs"));
    let ups = app.down(Down::Focus { node: "nowhere".into() });
    assert!(ups.is_empty() && app.page().map(|p| p.id.as_str()) == Some("packs"), "a page that is not there changes nothing");
    app.down(Down::Focus { node: "models".into() });
    assert_eq!(app.page().map(|p| p.id.as_str()), Some("hosts"), "the sub-tab that was open stays open");
}

// ---- the icon of the Decider, with no name ----

#[test]
fn the_decider_are_an_icon_dim_with_off_after_it_when_none_can_be_asked_and_plain_when_they_can_and_no_cell_says_the_pack() {
    use j4n_tui::theme::glyph::{Slot, Tier};
    for tier in ["icons", "unicode", "ascii", "words"] {
        for (cells, off) in [(vec![Cell::with_icon("decide-off", "Decider off", Tone::Dim)], true), (vec![Cell::with_icon("decide", "", Tone::Plain)], false)] {
            let mut app = app();
            app.theme = j4n_tui::theme::Theme::build(&j4n_tui::theme::Settings { marks: tier.into(), you: "mark".into(), ..Default::default() }, &[]).expect("a theme");
            let mut all = vec![Cell::new("idle", Tone::Dim)];
            all.extend(cells);
            all.push(Cell::new("proj", Tone::Dim));
            rows(&mut app, "bar:status", vec![Row { key: "status".into(), cells: all, ..Row::default() }]);
            let bar = screen(&mut app, 60, 8).pop().unwrap();
            let glyph = Slot::Decide.cell(Tier::parse(tier).unwrap());
            assert!(bar.contains(glyph), "{tier}: the icon is in the bar: {bar:?}");
            assert_eq!(bar.contains(" off"), off, "{tier}: `off` only while none can be asked: {bar:?}");
            let low = bar.to_ascii_lowercase();
            assert!(!low.contains("jev") && !low.contains("decision model"), "{tier}: the name of the pack is on no screen: {bar:?}");
        }
    }
    // the two states are two hues of one glyph: dim (faint) when off, the accent when on
    assert_eq!((Slot::Decide.role(), Slot::DecideOff.role()), (j4n_tui::theme::palette::Role::Accent, j4n_tui::theme::palette::Role::Faint));
    assert_eq!(Slot::Decide.cell(Tier::Icons), Slot::DecideOff.cell(Tier::Icons));
}
