//! The approval card over any tab. An approval, or the confirm of a risky key of a row, is a
//! card over the open tab and takes `y`, `n`, a digit, `?` and `esc` there; a plain question stays in the Chat, and the Chat tab counts what
//! waits while another tab is open. The sheets are the goldens of the scene `approval-card`.

use crate::world::*;
use crossterm::event::KeyCode;
use j4n_proto::{Act, AskView, Cell, Choice, Down, Intent, Reply, Role, Row, Shape, Tone, Up};
use j4n_tui::App;
use j4n_tui::headless::{ctrl, press as key};

fn act(k: &str, label: &str, text: &str, risk: u8) -> Act {
    Act { key: k.into(), label: label.into(), intent: Intent::Say { text: text.into(), with: Vec::new() }, risk }
}

/// The Tasks tab with two rows: `p1` widens and asks first on `a`, and takes `d` at once; `p2` has a key under the line.
fn world() -> App {
    let mut a = app();
    chat(&mut a, &[(Role::You, "hello")]);
    let widening = Row {
        key: "p1".into(),
        cells: vec![Cell::new("p1", Tone::Plain), Cell::new("open", Tone::Warn), Cell::new("let tests run cargo", Tone::Plain)],
        detail: vec![Cell::new("tests ran cargo by hand", Tone::Dim), Cell::new("widens runs a shell command", Tone::Dim)],
        actions: vec![act("a", "accept", ":accept p1", 60), act("d", "decline", ":decline p1", 0)],
        ..Row::default()
    };
    let plain = Row { key: "p2".into(), cells: vec![Cell::new("p2", Tone::Plain), Cell::new("open", Tone::Warn), Cell::new("a palette", Tone::Plain)], detail: vec![Cell::new("widens —", Tone::Dim)], actions: vec![act("a", "accept", ":accept p2", 60), act("o", "open", ":open p2", 40)], ..Row::default() };
    rows(&mut a, "tasks:all", vec![widening, plain]);
    a.key(key(KeyCode::F(2)));
    a
}

fn approval() -> AskView {
    AskView {
        ask: "a1".into(),
        asker: "b".into(),
        question: "git push origin main?".into(),
        shape: Shape::Approval,
        risk: 65,
        grants: vec!["task".into()],
        why: "A push sends your commits out of this machine.".into(),
        options: vec![Choice { id: "task".into(), widens: true, ..Choice::default() }],
        ..AskView::default()
    }
}

fn asks(a: &mut App, open: Vec<AskView>) {
    a.down(Down::Asks { open });
}

fn sent(ups: &[Up]) -> Vec<Intent> {
    ups.iter().filter_map(|u| if let Up::Intent { intent } = u { Some(intent.clone()) } else { None }).collect()
}

fn key_esc(a: &mut App) {
    a.key(key(KeyCode::Esc));
}

fn text(a: &mut App) -> String {
    show(a, 100, 28)
}

#[test]
fn an_approval_is_a_card_over_the_open_tab_and_y_n_and_the_digits_answer_it() {
    let mut a = world();
    asks(&mut a, vec![approval()]);
    let s = text(&mut a);
    assert!(s.contains("git push origin main?") && s.contains("y yes · n no") && s.contains("╭─"), "{s}");
    assert!(s.contains("p1"), "the tab is still under the card");
    assert_eq!(sent(&type_in(&mut a, "y")), [Intent::Answer { ask: "a1".into(), reply: Reply::Yes }]);
    assert_eq!(sent(&type_in(&mut a, "n")), [Intent::Answer { ask: "a1".into(), reply: Reply::No }]);
    assert_eq!(sent(&type_in(&mut a, "2")), [Intent::Answer { ask: "a1".into(), reply: Reply::No }]);
    assert!(sent(&type_in(&mut a, "x")).is_empty(), "no other key reaches the page behind the card");
    assert!(!text(&mut a).contains("A push sends your commits"), "the reason waits for `?`");
    type_in(&mut a, "?");
    assert!(text(&mut a).contains("A push sends your commits"));
}

#[test]
fn an_option_that_widens_keeps_its_second_press_on_the_card() {
    let mut a = world();
    asks(&mut a, vec![approval()]);
    assert!(sent(&type_in(&mut a, "3")).is_empty());
    assert!(text(&mut a).contains("3 again"));
    assert_eq!(sent(&type_in(&mut a, "3")), [Intent::Answer { ask: "a1".into(), reply: Reply::Grant { scope: "task".into() } }]);
    // another key lets go of the first press
    type_in(&mut a, "3");
    type_in(&mut a, "x");
    assert!(sent(&type_in(&mut a, "3")).is_empty());
}

#[test]
fn esc_puts_the_card_off_and_the_chat_tab_counts_what_waits() {
    let mut a = world();
    assert!(!text(&mut a).contains("Chat 1"));
    asks(&mut a, vec![approval()]);
    type_in(&mut a, "x");
    key_esc(&mut a);
    let s = text(&mut a);
    assert!(!s.contains("git push") && s.contains("Chat 1"), "{s}");
    // the page has its keys again
    assert!(sent(&type_in(&mut a, "a")).is_empty());
    assert!(text(&mut a).contains("accept p1?"), "the key of the row is the page's now");
}

#[test]
fn a_plain_question_is_not_a_card_it_waits_in_the_chat() {
    let mut a = world();
    let pick = AskView { ask: "q1".into(), asker: "b".into(), question: "which palette?".into(), shape: Shape::Choice, options: options(&[("jjj", false), ("dusk", false)]), ..AskView::default() };
    asks(&mut a, vec![pick]);
    let s = text(&mut a);
    assert!(!s.contains("which palette?") && s.contains("Chat 1"), "{s}");
    assert!(sent(&type_in(&mut a, "1")).is_empty(), "a digit is not an answer here: the page takes it for its tab");
    a.key(key(KeyCode::F(2)));
    // with the plain question and an approval, the card is the approval and the count the question
    let mut ask = approval();
    ask.ask = "a2".into();
    asks(&mut a, vec![AskView { ask: "q1".into(), asker: "b".into(), question: "which palette?".into(), shape: Shape::Choice, options: options(&[("jjj", false)]), ..AskView::default() }, ask]);
    let s = text(&mut a);
    assert!(s.contains("git push origin main?") && s.contains("Chat 1") && !s.contains("which palette?"), "{s}");
    assert_eq!(sent(&type_in(&mut a, "y")), [Intent::Answer { ask: "a2".into(), reply: Reply::Yes }]);
}

#[test]
fn the_chat_tab_draws_its_own_approval_and_no_card() {
    let mut a = world();
    asks(&mut a, vec![approval()]);
    a.key(key(KeyCode::F(1)));
    let s = text(&mut a);
    assert_eq!(s.matches("git push origin main?").count(), 1, "{s}");
    assert!(!s.contains("Chat 1"), "the tab that is open shows no count");
}

#[test]
fn a_risky_key_of_a_row_asks_on_the_card_and_a_key_under_the_line_runs_at_once() {
    let mut a = world();
    assert!(sent(&type_in(&mut a, "a")).is_empty(), "nothing runs on the first press");
    let s = text(&mut a);
    assert!(s.contains("accept p1?") && s.contains("tests ran cargo by hand") && s.contains("not now"), "{s}");
    // `n` and `esc` leave it
    assert!(sent(&type_in(&mut a, "n")).is_empty());
    assert!(!text(&mut a).contains("accept p1?"));
    type_in(&mut a, "a");
    key_esc(&mut a);
    assert!(!text(&mut a).contains("accept p1?"));
    // `y` sends the command of the key, once
    type_in(&mut a, "a");
    assert_eq!(sent(&type_in(&mut a, "y")), vec![Intent::Say { text: ":accept p1".into(), with: Vec::new() }]);
    a.key(key(KeyCode::Down));
    type_in(&mut a, "a");
    assert_eq!(sent(&type_in(&mut a, "1")), vec![Intent::Say { text: ":accept p2".into(), with: Vec::new() }]);
    // a key under the line has no card, `⏎` on the first key of a risky row has
    assert_eq!(sent(&type_in(&mut a, "o")), vec![Intent::Say { text: ":open p2".into(), with: Vec::new() }]);
    assert!(sent(&a.key(key(KeyCode::Enter))).is_empty());
    assert!(text(&mut a).contains("accept p2?"));
}

#[test]
fn the_card_is_modal_for_the_page_but_tabs_and_chords_go_through() {
    let mut a = world();
    type_in(&mut a, "a");
    assert!(sent(&type_in(&mut a, "d")).is_empty(), "the page does not decline behind the card");
    assert!(text(&mut a).contains("accept p1?"));
    a.key(key(KeyCode::F(3)));
    assert!(!text(&mut a).contains("accept p1?"), "a confirm goes with its page");
    a.key(key(KeyCode::F(2)));
    assert!(!text(&mut a).contains("accept p1?"), "and does not come back");
    asks(&mut a, vec![approval()]);
    assert_eq!(a.key(ctrl('c')), vec![Up::Quit]);
}

/// The Setup card is an approval J4n asks itself (asker `j4n`): the core's recommended answer, `open Setup`, is `⏎`, `esc` is its no, `later`,
/// and it has no task to stop.
#[test]
fn the_setup_card_takes_enter_for_open_setup_and_esc_for_later_and_has_no_stop() {
    let mut a = world();
    let card = AskView {
        ask: "setup:card".into(),
        asker: "j4n".into(),
        question: "Set up J4n?".into(),
        shape: Shape::Approval,
        effect: "Agent: none works; Decider: not set; Test: not run".into(),
        options: vec![Choice { id: "yes".into(), label: "open Setup".into(), recommended: true, by: j4n_proto::By::Core, ..Choice::default() }, Choice { id: "no".into(), label: "later".into(), by: j4n_proto::By::Core, ..Choice::default() }],
        ..AskView::default()
    };
    asks(&mut a, vec![card.clone()]);
    let s = text(&mut a);
    assert!(s.contains("Set up J4n?") && s.contains("⏎ open Setup") && s.contains("esc not now") && !s.contains("stop j4n"), "{s}");
    assert_eq!(sent(&a.key(key(KeyCode::Enter))), [Intent::Answer { ask: "setup:card".into(), reply: Reply::Yes }]);
    assert_eq!(sent(&a.key(key(KeyCode::Esc))), [Intent::Answer { ask: "setup:card".into(), reply: Reply::No }]);
    // an approval of a task keeps `esc later` and has no recommended answer for `⏎`
    asks(&mut a, vec![approval()]);
    assert!(text(&mut a).contains("esc later"));
    assert!(sent(&a.key(key(KeyCode::Enter))).is_empty());
}
