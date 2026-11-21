//! The screen answers the operator at once. A line sent is a turn of the chat in the frame of the ⏎, a key that answers a
//! question is marked on its option, and while a line is being taken a sign of J4n's own (the track) says how far it has come. These tests give
//! the screen no message from the core after the key, which is what a core that is slow looks like from here. The operator's line is no longer
//! drawn with a person mark: on the pipes it starts at the left edge.

use crate::world::{hello, screen};
use j4n_proto::{AskView, By, Cell, Choice, Down, Field, FieldKind, Intent, Json, Node, Present, Reply, Row, Shape, Tone, Up};
use j4n_tui::App;
use j4n_tui::headless::{KeyCode, press, typed};
use j4n_tui::parts::track::{BEAT_MS, CELLS, FRAMES, NARROW, cycle, track};
use j4n_tui::theme::{Settings, Theme};

fn look(a: &mut App, s: Settings) {
    a.theme = Theme::build(&s, &[]).expect("a theme");
}

/// The operator's look: the mark mode of `theme.you`, on the pipes, in the unicode tier so that a test can read the marks.
fn mark() -> Settings {
    Settings { marks: "unicode".into(), you: "mark".into(), ..Settings::default() }
}

fn started() -> App {
    let mut a = App::new();
    a.start();
    a.down(hello());
    let chat = Node { id: "chat".into(), field: Some(Field { kind: FieldKind::Chat, bind: "chat:main".into(), props: Json::Null }), ..Node::default() };
    a.down(Down::Tree { node: Node { id: "root".into(), present: Present::Stack, children: vec![chat], ..Node::default() } });
    look(&mut a, mark());
    a
}

fn turn(key: &str, what: &str) -> Row {
    Row { key: key.into(), cells: vec![Cell::new(what, Tone::Plain), Cell::new("", Tone::Dim), Cell::new("12:29", Tone::Dim)], ..Row::default() }
}

fn kid(key: &str, what: &str, icon: &str, parent: &str) -> Row {
    Row { key: key.into(), cells: vec![Cell::with_icon(icon, what, Tone::Plain), Cell::new("", Tone::Dim)], parent: Some(parent.into()), by: By::Core, ..Row::default() }
}

fn rows(a: &mut App, bind: &str, rows: Vec<Row>) {
    let total = rows.len() as u64;
    a.down(Down::Rows { bind: bind.into(), rows, total, from: 0, rev: 1 });
}

fn say(a: &mut App, text: &str) -> Vec<Up> {
    let mut sent = Vec::new();
    for k in typed(text, true) {
        sent.extend(a.key(k));
    }
    // what Tab would complete is asked as the line is typed (a file's name here): not what is said
    sent.retain(|u| !matches!(u, Up::Window { bind, .. } if bind == j4n_proto::COMPLETE));
    sent
}

fn lines(a: &mut App) -> Vec<String> {
    screen(a, 80, 24).into_iter().map(|l| l.trim_end().to_string()).collect()
}

fn has(s: &[String], what: &str) -> bool {
    s.iter().any(|l| l.contains(what))
}

// ---- the line is on the screen at once ----

#[test]
fn a_line_that_is_sent_is_a_turn_of_the_chat_in_the_same_frame_with_no_word_from_the_core() {
    let mut a = started();
    rows(&mut a, "chat:main", vec![turn("t1", "an earlier line")]);
    let sent = say(&mut a, "fix trailing commas in parser.rs");
    assert!(matches!(sent.as_slice(), [Up::Intent { intent: Intent::Say { text, .. } }] if text == "fix trailing commas in parser.rs"), "{sent:?}");
    // nothing came back: the core may be weighing the line, routing it or waiting on a lock
    let s = lines(&mut a);
    let at = s.iter().position(|l| l.starts_with("fix trailing commas in parser.rs")).unwrap_or_else(|| panic!("the line is not on the screen: {s:#?}"));
    assert!(s[at + 1].starts_with("╰─ ━━──── sending") && s[at + 1].contains("sending"), "the track is under it: {s:#?}");
    assert!(s.iter().position(|l| l.starts_with("an earlier line")) < Some(at), "after the turns the core sent: {s:#?}");
    // the composer took the keys the whole time: it is empty and takes the next line
    typed("and then", false).into_iter().for_each(|k| drop(a.key(k)));
    assert!(has(&lines(&mut a), "and then"));
}

#[test]
fn the_composer_keeps_taking_keys_while_a_line_waits_and_a_second_line_waits_behind_the_first() {
    let mut a = started();
    say(&mut a, "one");
    say(&mut a, "two");
    let s = lines(&mut a);
    let (one, two) = (s.iter().position(|l| l.starts_with("one")), s.iter().position(|l| l.starts_with("two")));
    assert!(one.is_some() && two > one, "{s:#?}");
}

#[test]
fn the_cores_row_takes_the_place_of_the_pending_one_and_the_track_goes_on_from_what_the_rows_say() {
    let mut a = started();
    say(&mut a, "fix it");
    // the core's row of the line, with nothing under it yet: it was taken, and is being weighed
    rows(&mut a, "chat:main", vec![turn("t1", "fix it")]);
    let s = lines(&mut a);
    assert_eq!(s.iter().filter(|l| l.starts_with("fix it")).count(), 1, "one row, not two: {s:#?}");
    assert!(has(&s, "routing"), "{s:#?}");
    // it was routed to a new chat: the orchestrator's route row; the chat is not running yet
    rows(&mut a, "chat:main", vec![turn("t1", "fix it"), kid("r1", "→ c3 · new chat · none matched", "", "t1")]);
    let s = lines(&mut a);
    assert!(has(&s, "opening · a chat for “fix it”"), "{s:#?}");
    // the chat has started: nothing is being taken any more
    let chat = |state: &str| Row { key: "c3".into(), cells: vec![Cell::with_icon(state, "", Tone::Dim)], data: Some(j4n_proto::json!({ "state": state })), ..Row::default() };
    rows(&mut a, "chats:list", vec![chat("queued")]);
    assert!(has(&lines(&mut a), "starting"), "{:#?}", lines(&mut a));
    rows(&mut a, "chats:list", vec![chat("running")]);
    let s = lines(&mut a);
    assert!(!has(&s, "starting") && !has(&s, "opening") && !has(&s, "sending"), "{s:#?}");
}

#[test]
fn the_first_line_of_a_chat_is_said_in_the_turn_the_core_already_had_for_what_came_before_it() {
    // a chat that begins with notes has a turn with no words, and the first line is said in it (`rows::new_turn`)
    let mut a = started();
    rows(&mut a, "chat:main", vec![turn("s1", ""), kid("n1", "new folder · a checkpoint before work", "", "s1")]);
    say(&mut a, "who");
    assert!(lines(&mut a).iter().filter(|l| l.starts_with("who")).count() == 1 && has(&lines(&mut a), "sending"));
    rows(&mut a, "chat:main", vec![turn("s1", "who"), kid("n1", "new folder · a checkpoint before work", "", "s1"), kid("c1", "ledger · 1 tasks", "done", "s1")]);
    let s = lines(&mut a);
    assert_eq!(s.iter().filter(|l| l.starts_with("who")).count(), 1, "the turn of the core, and no second one: {s:#?}");
    assert!(!has(&s, "sending") && !has(&s, "routing"), "{s:#?}");
}

#[test]
fn a_command_has_its_line_at_once_and_no_track_after_the_core_has_answered() {
    let mut a = started();
    say(&mut a, ":look jjj");
    assert!(has(&lines(&mut a), ":look jjj"));
    rows(&mut a, "chat:main", vec![turn("t1", ":look jjj"), kid("n1", "the look is jjj", "", "t1")]);
    let s = lines(&mut a);
    assert!(has(&s, "the look is jjj") && !has(&s, "sending") && !has(&s, "routing"), "{s:#?}");
}

#[test]
fn a_line_the_core_never_took_does_not_stay_for_ever() {
    let mut a = started();
    let t0 = std::time::Instant::now();
    a.set_clock(t0);
    say(&mut a, "lost");
    assert!(has(&lines(&mut a), "lost"));
    a.set_clock(t0 + std::time::Duration::from_secs(61));
    a.tick(t0 + std::time::Duration::from_secs(61));
    assert!(!has(&lines(&mut a), "lost"));
}

// ---- a key on a question ----

fn route_question() -> AskView {
    AskView {
        ask: "route:1".into(),
        asker: "main".into(),
        question: "which chat takes this?".into(),
        shape: Shape::Choice,
        options: ["c1", "c2", "new"].iter().map(|id| Choice { id: id.to_string(), label: format!("{id} · files"), recommended: *id == "new", ..Choice::default() }).collect(),
        ..AskView::default()
    }
}

#[test]
fn a_key_on_the_route_question_is_marked_on_its_option_at_once_and_goes_with_the_question() {
    let mut a = started();
    rows(&mut a, "chat:main", vec![turn("t0", "an earlier line")]);
    a.down(Down::Asks { open: vec![route_question()] });
    let sent = a.key(press(KeyCode::Char('2')));
    assert_eq!(sent, vec![Up::Intent { intent: Intent::Answer { ask: "route:1".into(), reply: Reply::Pick { id: "c2".into() } } }]);
    // no word from the core yet: the option that was pressed says it is being sent, the others do not
    let s = lines(&mut a);
    let marked: Vec<&String> = s.iter().filter(|l| l.contains("sending")).collect();
    assert!(marked.len() == 1 && marked[0].contains("c2 · files"), "{s:#?}");
    // the core took the answer: the question is gone with its mark
    a.down(Down::Asks { open: Vec::new() });
    assert!(!has(&lines(&mut a), "sending"));
}

#[test]
fn the_digit_typed_into_the_composer_and_sent_is_a_line_at_once_too() {
    let mut a = started();
    rows(&mut a, "chat:main", vec![turn("t0", "an earlier line")]);
    a.down(Down::Asks { open: vec![route_question()] });
    // typed after a letter, so that it is a line and not the key of the question
    typed("c2", true).into_iter().for_each(|k| drop(a.key(k)));
    assert!(lines(&mut a).iter().any(|l| l.starts_with("c2")));
}

// ---- the track ----

fn text(spans: &[ratatui::text::Span]) -> String {
    spans.iter().map(|s| s.content.as_ref()).collect()
}

fn built(marks: &str, motion: bool) -> Theme {
    Theme::build(&Settings { marks: marks.into(), motion, ..Settings::default() }, &[]).expect("a theme")
}

/// The frames of one full cycle of a sign `cells` wide, as the counter `App.sign` runs through them (`0..FRAMES`, round again).
fn cycle_of(th: &Theme, cells: usize) -> Vec<String> {
    (0..cycle(cells) as u8).map(|f| text(&track(th, f, cells))).collect()
}

#[test]
fn the_track_is_a_bright_segment_of_two_cells_that_runs_right_and_back_on_a_dim_track_of_six() {
    let th = built("unicode", true);
    assert_eq!(cycle_of(&th, CELLS), ["━━────", "─━━───", "──━━──", "───━━─", "────━━", "───━━─", "──━━──", "─━━───"]);
    // a narrow row has four cells and a shorter run
    assert_eq!(cycle_of(&th, NARROW), ["━━──", "─━━─", "──━━", "─━━─"]);
}

#[test]
fn each_frame_of_a_cycle_differs_from_its_neighbours_so_the_sign_is_never_still_for_two_frames() {
    let th = built("unicode", true);
    for cells in [CELLS, NARROW] {
        let frames = cycle_of(&th, cells);
        for (i, f) in frames.iter().enumerate() {
            assert_ne!(*f, frames[(i + 1) % frames.len()], "{cells} cells, frame {i} is as the next: {frames:?}");
        }
        // the counter of the screen runs on past one cycle and round, and the sign goes on from where it was
        let counter: Vec<String> = (0..FRAMES).map(|f| text(&track(&th, f, cells))).collect();
        assert!(counter.windows(2).all(|w| w[0] != w[1]) && counter[0] != counter[usize::from(FRAMES) - 1], "{cells} cells: {counter:?}");
    }
}

#[test]
fn no_frame_is_a_complete_one_and_the_sign_is_only_the_thin_line() {
    for tier in ["unicode", "words", "icons"] {
        let th = built(tier, true);
        for cells in [CELLS, NARROW] {
            for f in cycle_of(&th, cells) {
                assert_eq!(f.chars().filter(|c| *c == '━').count(), 2, "{tier} {cells}: a segment of two cells and no more: {f}");
                assert!(f.contains('─'), "{tier} {cells}: a part of the track is always dim: {f}");
                assert!(f.chars().all(|c| matches!(c, '━' | '─')), "{tier}: no mark, no half cell, no dots: {f}");
            }
        }
    }
}

#[test]
fn the_width_is_at_most_six_cells_and_four_on_a_narrow_row_whatever_is_asked() {
    let th = built("unicode", true);
    for asked in [0, 1, 2, 3, 4, 6, 8, 12, 40] {
        let w = text(&track(&th, 3, asked)).chars().count();
        assert!(w <= CELLS, "asked {asked}: {w} cells");
        if asked <= NARROW {
            assert!(w <= NARROW, "asked {asked}: {w} cells");
        }
    }
    assert_eq!((CELLS, NARROW), (6, 4));
}

#[test]
fn with_motion_off_the_sign_is_one_fixed_frame_the_dim_track_in_every_tier() {
    for (tier, still) in [("unicode", "──────"), ("words", "──────"), ("icons", "──────"), ("ascii", "------")] {
        let th = built(tier, false);
        let every: Vec<String> = (0..FRAMES).map(|f| text(&track(&th, f, CELLS))).collect();
        assert!(every.iter().all(|f| *f == still), "{tier}: {every:?}");
        assert_eq!(text(&track(&th, 5, NARROW)), still[..4 * still.len() / 6], "{tier}: four cells on a narrow row");
    }
    // and with motion on the ascii tier is `=` over `-`, the track `--==--`
    assert_eq!(text(&track(&built("ascii", true), 2, CELLS)), "--==--");
}

#[test]
fn the_sign_wakes_the_screen_every_120_ms_while_a_sign_is_drawn_and_for_nothing_else() {
    let t0 = std::time::Instant::now();
    // nothing sent: no sign on the screen, the screen sleeps
    let mut idle = started();
    idle.set_clock(t0);
    idle.tick(t0);
    lines(&mut idle);
    assert_eq!(idle.wake(t0), None, "no sign, no wake");
    // a line is being taken: a sign is drawn, and the next frame is a beat away
    let mut a = started();
    say(&mut a, "go");
    a.set_clock(t0);
    a.tick(t0);
    lines(&mut a);
    let wait = a.wake(t0).expect("a sign is drawn");
    assert_eq!(wait.as_millis(), BEAT_MS, "{wait:?}");
    assert_eq!(a.wake(t0 + std::time::Duration::from_millis(50)).map(|d| d.as_millis()), Some(70), "aligned to the clock, not to the last wake");
    // with motion off the sign stands still and nothing wakes the screen for it
    look(&mut a, Settings { motion: false, ..mark() });
    lines(&mut a);
    assert_eq!(a.wake(t0), None, "motion off: one fixed frame, no wake");
}

#[test]
fn the_frame_steps_with_the_clock_and_is_drawn_again_only_while_a_sign_is_in_sight() {
    let mut a = started();
    say(&mut a, "go");
    let t0 = std::time::Instant::now();
    a.set_clock(t0);
    a.tick(t0);
    let first = lines(&mut a);
    a.take_redraw();
    let later = t0 + std::time::Duration::from_millis(BEAT_MS as u64);
    a.set_clock(later);
    a.tick(later);
    assert!(a.take_redraw(), "one beat on a sign is drawn again");
    let then = lines(&mut a);
    assert_ne!(first, then, "the segment has moved");
    assert!(then.iter().any(|l| l.contains("─━━─── sending")), "one cell on: {then:#?}");
    // no sign, no redraw for the beat
    let mut idle = started();
    idle.set_clock(t0);
    idle.tick(t0);
    lines(&mut idle);
    idle.take_redraw();
    idle.set_clock(later);
    idle.tick(later);
    assert!(!idle.take_redraw(), "nothing on the screen moves");
}

// ---- the operator's line ----

#[test]
fn the_operators_line_has_no_mark_and_starts_at_the_left_edge_with_the_rows_under_it_on_their_pipes() {
    let mut a = started();
    rows(&mut a, "chat:main", vec![turn("t1", "fix the parser so trailing commas work"), kid("k1", "A claude sonnet", "working", "t1")]);
    let s = lines(&mut a);
    let at = s.iter().position(|l| l.starts_with("fix the parser")).expect("the line starts at column 0");
    assert!(s[at].contains("12:29"), "{s:#?}");
    assert!(s[at + 1].starts_with("╰─ ▷ A claude"), "the tree under it starts where it does: {s:#?}");
    assert!(s.iter().all(|l| !l.contains('❯')), "no mark: {s:#?}");
}

#[test]
fn in_every_tier_the_line_has_no_person_mark_and_a_wrapped_line_stays_at_the_edge() {
    for tier in ["icons", "unicode", "ascii", "words"] {
        let mut a = started();
        look(&mut a, Settings { marks: tier.into(), you: "mark".into(), ..Settings::default() });
        rows(&mut a, "chat:main", vec![turn("t1", "fix the parser so trailing commas work in every place the lexer reads a list, and say what changed")]);
        let s = lines(&mut a);
        let at = s.iter().position(|l| l.starts_with("fix the parser")).unwrap_or_else(|| panic!("{tier}: {s:#?}"));
        assert!(s[at + 1].starts_with(|c: char| c.is_alphabetic()), "{tier}: the next row of the line starts at the edge too: {s:#?}");
        assert!(s.iter().all(|l| !l.contains('\u{F0013}')), "{tier}: the person mark is gone");
    }
}
