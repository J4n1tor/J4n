//! The composer's card: the draft is sent after a pause and only when it changed, an Enter waits for
//! the shaper, a card needs one more Enter, Esc drops the notes and never sends, and with the switch off none of it happens.

use crate::world::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use j4n_proto::{Cell, Down, Intent, Json, Row, Tone, Up};
use j4n_tui::App;
use j4n_tui::headless::{ctrl, press, typed};
use j4n_tui::shape::PAUSE;
use std::time::{Duration, Instant};

fn row(key: &str, cells: &[&str], tone: Tone) -> Row {
    Row { key: key.into(), cells: cells.iter().map(|c| Cell::new(*c, Tone::Plain)).collect(), tone, ..Row::default() }
}

fn card(app: &mut App, rows: Vec<Row>) -> Vec<Up> {
    let total = rows.len() as u64;
    app.down(Down::Rows { bind: "shape:card".into(), rows, total, from: 0, rev: 1 })
}

fn on(app: &mut App) {
    app.down(Down::Value { bind: "shape:on".into(), value: Json::Bool(true), rev: 1 });
}

fn head(text: &str, state: &str) -> Row {
    row("~draft", &[text, state, ""], Tone::Plain)
}

fn type_in(app: &mut App, text: &str) {
    for k in typed(text, false) {
        app.key(k);
    }
}

fn enter(app: &mut App) -> Vec<Up> {
    app.key(press(KeyCode::Enter))
}

fn say(text: &str, with: &[&str]) -> Up {
    Up::Intent { intent: Intent::Say { text: text.into(), with: with.iter().map(|k| k.to_string()).collect() } }
}

fn run(args: &[&str]) -> Up {
    Up::Intent { intent: Intent::Run { command: "shape".into(), args: args.iter().map(|a| a.to_string()).collect() } }
}

#[test]
fn the_draft_is_sent_once_it_has_stood_still_and_only_when_it_changed() {
    let mut app = app();
    on(&mut app);
    let t0 = Instant::now();
    type_in(&mut app, "fix the parser thing");
    assert!(app.tick(t0).is_empty(), "the first look only starts the wait");
    assert!(app.tick(t0 + Duration::from_millis(700)).is_empty());
    assert_eq!(app.tick(t0 + Duration::from_millis(900)), [Up::Draft { text: "fix the parser thing".into(), target: String::new() }]);
    assert!(app.tick(t0 + Duration::from_secs(5)).is_empty(), "it is not sent again");
    type_in(&mut app, "!");
    assert!(app.tick(t0 + Duration::from_secs(6)).is_empty(), "a change starts the wait again");
    assert_eq!(app.tick(t0 + Duration::from_secs(7)).len(), 1);
}

#[test]
fn with_the_switch_off_nothing_is_sent_and_enter_sends_the_line() {
    let mut app = app();
    let t0 = Instant::now();
    type_in(&mut app, "fix the parser thing");
    app.tick(t0);
    assert!(app.tick(t0 + Duration::from_secs(5)).is_empty());
    assert_eq!(enter(&mut app), [say("fix the parser thing", &[])]);
}

#[test]
fn a_command_is_never_sent_as_a_draft_and_never_waited_on() {
    let mut app = app();
    on(&mut app);
    let t0 = Instant::now();
    type_in(&mut app, ":fresh");
    app.tick(t0);
    assert!(app.tick(t0 + Duration::from_secs(5)).is_empty());
    assert_eq!(enter(&mut app), [say(":fresh", &[])]);
}

#[test]
fn a_card_that_is_ready_needs_an_enter_and_sends_the_kept_rows_with_the_line() {
    let mut app = app();
    on(&mut app);
    type_in(&mut app, "fix the parser thing");
    card(&mut app, vec![head("fix the parser thing", "ready"), row("file:1", &["file", "crates/cli/src/args.rs", "exist"], Tone::Good), row("file:2", &["file", "crates/cli/src/lexer.rs", "exist"], Tone::Dim), row("x:1", &["dropped", "a note", "no source"], Tone::Warn)]);
    let shown = show(&mut app, 100, 20);
    assert!(shown.contains("crates/cli/src/args.rs") && shown.contains("crates/cli/src/lexer.rs") && shown.contains("no source") && shown.contains("send with notes"), "{shown}");
    assert_eq!(enter(&mut app), [say("fix the parser thing", &["file:1"])], "only the rows shown as kept go with it");
    assert!(app.line.buf.is_empty());
}

#[test]
fn an_enter_before_the_result_waits_and_a_pass_then_sends_at_once() {
    let mut app = app();
    on(&mut app);
    type_in(&mut app, "add a flag");
    // typed and sent before the pause: ask now
    assert_eq!(enter(&mut app), [Up::Draft { text: "add a flag".into(), target: String::new() }]);
    assert_eq!(app.line.buf, "add a flag", "the line stays while it waits");
    assert!(card(&mut app, vec![head("add a flag", "working")]).is_empty());
    assert!(enter(&mut app).is_empty(), "a second Enter does not send either");
    assert_eq!(card(&mut app, vec![head("add a flag", "pass")]), [say("add a flag", &[])]);
    assert!(app.line.buf.is_empty());
}

#[test]
fn an_enter_before_the_result_that_comes_with_notes_waits_for_one_more_enter() {
    let mut app = app();
    on(&mut app);
    type_in(&mut app, "fix the parser thing");
    app.tick(Instant::now());
    card(&mut app, vec![head("fix the parser thing", "working")]);
    assert!(enter(&mut app).is_empty());
    let sent = card(&mut app, vec![head("fix the parser thing", "ready"), row("file:1", &["file", "a.rs", "exist"], Tone::Good)]);
    assert!(sent.is_empty(), "nothing goes out with notes the operator has not seen");
    assert_eq!(enter(&mut app), [say("fix the parser thing", &["file:1"])]);
}

#[test]
fn a_line_the_shaper_did_not_run_for_goes_as_typed_and_does_not_wait() {
    let mut app = app();
    on(&mut app);
    type_in(&mut app, "do it");
    app.tick(Instant::now());
    card(&mut app, vec![row("~draft", &["do it", "skip", "a short reply to the doer that has the thread"], Tone::Plain)]);
    assert_eq!(enter(&mut app), [say("do it", &[])]);
    type_in(&mut app, "fix it");
    card(&mut app, vec![row("~draft", &["fix it", "failed", "the shaper did not answer"], Tone::Plain)]);
    assert!(show(&mut app, 100, 12).contains("shape did not run: the shaper did not answer"));
    assert_eq!(enter(&mut app), [say("fix it", &[])]);
}

#[test]
fn escape_drops_the_notes_and_never_sends_and_with_no_card_it_keeps_the_draft() {
    let mut app = app();
    on(&mut app);
    type_in(&mut app, "fix the parser thing");
    card(&mut app, vec![head("fix the parser thing", "ready"), row("file:1", &["file", "a.rs", "exist"], Tone::Good)]);
    assert_eq!(app.key(press(KeyCode::Esc)), [run(&["drop"])]);
    assert_eq!(app.line.buf, "fix the parser thing", "the draft stays as typed");
    card(&mut app, vec![head("fix the parser thing", "dropped")]);
    assert_eq!(enter(&mut app), [say("fix the parser thing", &[])], "the next Enter sends it as typed");
    type_in(&mut app, "something");
    assert!(app.key(press(KeyCode::Esc)).is_empty());
    assert_eq!(app.line.buf, "something", "no card: Escape keeps the draft too");
}

#[test]
fn control_o_gives_the_card_the_keys_to_move_keep_and_pick() {
    let mut app = app();
    on(&mut app);
    type_in(&mut app, "fix the parser thing");
    card(
        &mut app,
        vec![head("fix the parser thing", "ready"), row("file:1", &["file", "a.rs", "exist"], Tone::Good), row("file:2", &["file", "b.rs", "exist"], Tone::Good), row("question", &["question", "Which one?", "turn"], Tone::Warn)],
    );
    assert!(app.key(ctrl('o')).is_empty());
    assert!(show(&mut app, 100, 20).contains("space keep/drop"));
    app.key(press(KeyCode::Down));
    assert_eq!(app.key(press(KeyCode::Char(' '))), [run(&["toggle", "file:2"])]);
    assert_eq!(app.key(press(KeyCode::Char('2'))), [run(&["pick", "2"])]);
    assert!(app.line.buf == "fix the parser thing", "the keys went to the card, not the draft");
    assert!(app.key(press(KeyCode::Enter)).is_empty(), "Enter gives the keys back");
    assert_eq!(enter(&mut app).len(), 1, "and the next one sends");
    let _ = KeyEvent::new(KeyCode::Null, KeyModifiers::NONE);
}

#[test]
fn a_card_for_another_text_is_not_shown_and_a_pass_is_a_mark_at_the_edge() {
    let mut app = app();
    on(&mut app);
    type_in(&mut app, "something new");
    card(&mut app, vec![head("an older line", "ready"), row("file:1", &["file", "a.rs", "exist"], Tone::Good)]);
    assert!(!show(&mut app, 100, 12).contains("a.rs"));
    app.line.buf.clear();
    type_in(&mut app, "add a flag");
    card(&mut app, vec![head("add a flag", "pass")]);
    assert!(show(&mut app, 100, 12).lines().any(|l| l.starts_with("│ add a flag") && l.trim_end().trim_end_matches('│').trim_end().ends_with('\u{F012C}')), "{}", show(&mut app, 100, 12));
}

#[test]
fn an_enter_that_waited_is_forgotten_when_the_line_was_changed_meanwhile() {
    let mut app = app();
    on(&mut app);
    type_in(&mut app, "add a flag");
    assert_eq!(enter(&mut app).len(), 1, "the draft is asked for now");
    type_in(&mut app, " to the cli");
    // the answer is for the line as it was: the Enter that waited is not let go with the new text
    assert!(card(&mut app, vec![head("add a flag", "pass")]).is_empty());
    assert_eq!(app.line.buf, "add a flag to the cli");
}

#[test]
fn an_enter_for_a_draft_already_asked_and_still_quiet_sends_the_line() {
    let mut app = app();
    on(&mut app);
    type_in(&mut app, "add a flag");
    app.tick(Instant::now());
    app.tick(Instant::now() + Duration::from_secs(2));
    // the draft went out and nothing came back (the desk has no card): a second look is not asked for, the line goes as typed
    assert_eq!(enter(&mut app), [say("add a flag", &[])]);
}

#[test]
fn the_clock_wakes_the_loop_for_the_end_of_the_pause_and_for_nothing_else() {
    let mut app = app();
    let t0 = Instant::now();
    assert_eq!(app.wake(t0), None, "an empty line: nothing waits");
    on(&mut app);
    type_in(&mut app, "fix the parser thing");
    assert_eq!(app.wake(t0), Some(Duration::ZERO), "a line the clock has not looked at is looked at now");
    app.tick(t0);
    assert_eq!(app.wake(t0), Some(PAUSE), "then the pause is waited for");
    assert_eq!(app.wake(t0 + Duration::from_millis(300)), Some(PAUSE - Duration::from_millis(300)));
    assert_eq!(app.wake(t0 + Duration::from_secs(2)), Some(Duration::ZERO), "a pause that is over is not waited for");
    app.tick(t0 + Duration::from_secs(2));
    assert_eq!(app.wake(t0 + Duration::from_secs(2)), None, "the draft is sent: nothing waits");
    let mut off = self::app();
    type_in(&mut off, "fix the parser thing");
    off.tick(t0);
    assert_eq!(off.wake(t0), None, "with the switch off the draft is not sent, so the clock is no reason to wake");
    let mut cmd = self::app();
    on(&mut cmd);
    type_in(&mut cmd, ":quit");
    cmd.tick(t0);
    assert_eq!(cmd.wake(t0), None, "a command is not a draft");
}

#[test]
fn a_line_that_was_sent_and_then_cleared_is_no_reason_to_wake() {
    let mut app = app();
    on(&mut app);
    let t0 = Instant::now();
    type_in(&mut app, "fix the parser thing");
    app.tick(t0);
    assert_eq!(app.tick(t0 + Duration::from_secs(1)).len(), 1, "sent");
    app.key(ctrl('u'));
    app.tick(t0 + Duration::from_secs(2));
    assert_eq!(app.wake(t0 + Duration::from_secs(2)), None, "an empty line is not sent, so the loop would spin on a pause it can never end");
    assert_eq!(app.wake(t0 + Duration::from_secs(9)), None);
}
