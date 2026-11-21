//! What the app keeps and what a key does: the handshake, the draft, the questions, the lists and forms.

use crate::world::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use j4n_proto::{Act, Control, Down, Intent, Json, Reply, Role, Shape, Tone, Up};
use j4n_tui::headless::{alt, ctrl, press as key, typed};

fn say(text: &str) -> Up {
    Up::Intent { intent: Intent::Say { text: text.into(), with: Vec::new() } }
}

fn watch(bind: &str) -> Up {
    Up::Watch { bind: bind.into() }
}

fn unwatch(bind: &str) -> Up {
    Up::Unwatch { bind: bind.into() }
}

fn answer(reply: Reply) -> Up {
    Up::Intent { intent: Intent::Answer { ask: "t2".into(), reply } }
}

// ---- the handshake ----

#[test]
fn the_first_thing_said_is_hello_and_the_tree_makes_it_watch_what_it_shows() {
    let (app, sent) = started();
    assert_eq!(sent[0], Up::Hello { api: j4n_proto::API });
    let mut watched: Vec<Up> = sent[1..].to_vec();
    watched.sort_by_key(|u| format!("{u:?}"));
    assert_eq!(watched, [watch("asks:open"), watch("bar:status"), watch("chat:here"), watch("chat:main"), watch("chats:list"), watch("commands:all"), watch("complete:"), watch("composer:bind"), watch("composer:strip"), watch("shape:card"), watch("shape:on"), watch("terms:")], "the open tab's field only, besides what is outside the tabs, the chat, the palette of commands, the card, switch, strip and binder's guess of its composer, what Tab completes, the glossary of the docs, and where the screen is among the chats and their list");
    assert_eq!((app.name.as_str(), app.version.as_str(), app.project.as_str()), ("j4n", "0.0.0", "demo"));
}

#[test]
fn another_tab_is_watched_when_it_opens_and_let_go_when_it_closes_but_the_chat_stays() {
    let mut app = app();
    assert_eq!(app.key(alt('2')), [watch("tasks:all")]);
    assert_eq!(app.key(alt('3')), [unwatch("tasks:all"), watch("log:all"), watch("look:file")], "a Log, and the look file it draws its events in");
    assert_eq!(app.key(alt('4')), [unwatch("log:all"), unwatch("look:file"), watch("form:thread")]);
    assert_eq!(app.key(alt('1')), [unwatch("form:thread")], "back at the chat, which was never let go");
    assert_eq!(app.key(alt('4')), [watch("form:thread")]);
    assert_eq!(app.key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE)), [unwatch("form:thread"), watch("tasks:all")]);
    assert_eq!(app.key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE)), [unwatch("tasks:all")]);
    assert!(app.key(KeyEvent::new(KeyCode::F(9), KeyModifiers::NONE)).is_empty(), "there is no ninth tab");
    assert!(app.key(KeyEvent::new(KeyCode::F(0), KeyModifiers::NONE)).is_empty());
}

#[test]
fn a_core_that_speaks_a_newer_protocol_ends_the_screen_with_a_message() {
    let mut app = j4n_tui::App::new();
    app.down(Down::Hello { api: j4n_proto::API + 1, name: "j4n".into(), version: "9".into(), project: "p".into() });
    assert!(app.quit);
    assert!(app.fatal.as_deref().unwrap().contains("update J4n"));
    assert!(show(&mut app, 80, 5).contains("update J4n"));
}

#[test]
fn a_new_tree_replaces_the_old_and_a_value_or_a_focus_on_the_open_page_changes_nothing() {
    let mut app = app();
    assert!(app.down(Down::Value { bind: "x".into(), value: Json::Null, rev: 1 }).is_empty());
    assert!(app.down(Down::Focus { node: "chat".into() }).is_empty());
    let again = app.down(Down::Tree { node: tree() });
    assert!(again.is_empty(), "nothing new to watch: {again:?}");
}

// ---- the draft ----

#[test]
fn enter_sends_the_line_and_empties_the_draft_and_an_empty_draft_sends_nothing() {
    let mut app = app();
    assert!(press(&mut app, typed("   ", true)).is_empty(), "blank is not a line");
    // typing plain words asks the completion for whole lines on the way; what ⏎ sends is the line
    let sent: Vec<Up> = press(&mut app, typed("  hello there ", true)).into_iter().filter(|u| !matches!(u, Up::Window { .. })).collect();
    assert_eq!(sent, [say("hello there")]);
    assert_eq!((app.line.buf.as_str(), app.line.at), ("", 0));
}

#[test]
fn escape_keeps_the_draft_and_never_sends_it_and_ctrl_u_empties_it() {
    let mut app = app();
    type_in(&mut app, "half a thought");
    assert!(app.key(key(KeyCode::Esc)).is_empty());
    assert_eq!(app.line.buf, "half a thought", "esc never drops a typed line");
    app.key(ctrl('u'));
    assert_eq!(app.line.buf, "");
    assert!(app.key(key(KeyCode::Enter)).is_empty());
}

#[test]
fn the_cursor_moves_and_edits_by_characters_not_bytes() {
    let mut app = app();
    type_in(&mut app, "añb");
    assert_eq!(app.line.at, 3);
    app.key(key(KeyCode::Left));
    app.key(key(KeyCode::Left));
    assert_eq!(app.line.at, 1);
    app.key(key(KeyCode::Delete));
    assert_eq!(app.line.buf, "ab");
    app.key(key(KeyCode::Backspace));
    assert_eq!((app.line.buf.as_str(), app.line.at), ("b", 0));
    app.key(key(KeyCode::Backspace));
    assert_eq!(app.line.buf, "b", "nothing before the cursor");
    type_in(&mut app, "é世");
    assert_eq!(app.line.buf, "é世b");
    app.key(key(KeyCode::End));
    app.key(key(KeyCode::Delete));
    assert_eq!(app.line.buf, "é世b", "nothing after it");
    app.key(key(KeyCode::Home));
    assert_eq!(app.line.at, 0);
    app.key(key(KeyCode::Right));
    app.key(key(KeyCode::Right));
    type_in(&mut app, "-");
    assert_eq!(app.line.buf, "é世-b");
    for _ in 0..9 {
        app.key(key(KeyCode::Right));
    }
    assert_eq!(app.line.at, 4);
}

#[test]
fn control_keys_edit_the_draft_and_do_not_type_themselves() {
    let mut app = app();
    type_in(&mut app, "abc");
    app.key(ctrl('a'));
    assert_eq!(app.line.at, 0);
    app.key(ctrl('e'));
    assert_eq!(app.line.at, 3);
    app.key(ctrl('u'));
    assert_eq!((app.line.buf.as_str(), app.line.at), ("", 0));
    app.key(ctrl('x'));
    app.key(alt('x'));
    assert_eq!(app.line.buf, "", "a chord that means nothing types nothing");
}

#[test]
fn the_lines_sent_are_recalled_with_up_and_down_and_the_same_line_twice_is_kept_once() {
    let mut app = app();
    for l in ["one", "two", "two", "three"] {
        press(&mut app, typed(l, true));
    }
    assert_eq!(app.history, ["one", "two", "three"]);
    app.key(key(KeyCode::Up));
    assert_eq!(app.line.buf, "three");
    app.key(key(KeyCode::Up));
    app.key(key(KeyCode::Up));
    app.key(key(KeyCode::Up));
    assert_eq!(app.line.buf, "one", "the oldest is the end of the road");
    app.key(key(KeyCode::Down));
    assert_eq!(app.line.buf, "two");
    app.key(key(KeyCode::Down));
    app.key(key(KeyCode::Down));
    assert_eq!((app.line.buf.as_str(), app.line.at), ("", 0), "past the newest is a clean draft");
    assert!(app.key(key(KeyCode::Down)).is_empty());
}

#[test]
fn there_is_nothing_to_recall_before_anything_was_sent_and_the_history_has_a_limit() {
    let mut app = app();
    app.key(key(KeyCode::Up));
    assert_eq!(app.line.buf, "");
    for i in 0..130 {
        press(&mut app, typed(&format!("line {i}"), true));
    }
    assert_eq!(app.history.len(), 100);
    assert_eq!(app.history.last().map(String::as_str), Some("line 129"));
}

#[test]
fn control_c_and_control_d_leave() {
    for c in ['c', 'd'] {
        let mut app = app();
        assert_eq!(app.key(ctrl(c)), [Up::Quit]);
        assert!(app.quit);
    }
}

// ---- what is shown and scrolled ----

#[test]
fn a_stream_is_nothing_to_this_screen_the_core_sends_rows() {
    let mut app = app();
    let items = vec![j4n_proto::Item::Put { line: j4n_proto::Line { key: "a".into(), role: Role::Doer, text: "one".into(), task: None, partial: false, tone: Tone::Plain } }];
    assert!(app.down(Down::Stream { bind: "chat:main".into(), reset: true, items }).is_empty());
    assert!(!app.rows.contains_key("chat:main"), "a stream makes no rows");
    assert!(!show(&mut app, 80, 24).contains("one"), "and nothing is drawn from it");
}

#[test]
fn page_up_and_down_scroll_the_open_field_and_a_reset_or_a_send_brings_back_the_end() {
    let mut app = app();
    chat(&mut app, &[(Role::Doer, "x")]);
    still(&mut app);
    app.size = (80, 24);
    app.key(key(KeyCode::PageUp));
    assert_eq!(app.scroll["chat:main"], 9, "half of the height less six");
    app.key(key(KeyCode::PageUp));
    assert_eq!(app.scroll["chat:main"], 18);
    app.key(key(KeyCode::PageDown));
    assert_eq!(app.scroll["chat:main"], 9);
    app.key(key(KeyCode::PageUp));
    type_in(&mut app, "x");
    app.key(key(KeyCode::Enter));
    assert!(!app.scroll.contains_key("chat:main"), "saying something goes to the end");
    app.key(key(KeyCode::PageDown));
    assert_eq!(app.scroll.get("chat:main").copied().unwrap_or(0), 0, "no further down than the end");
}

#[test]
fn a_key_clears_the_toast() {
    let mut app = app();
    app.down(Down::Toast { text: "careful".into(), tone: Tone::Warn });
    assert!(app.toast.is_some());
    app.key(key(KeyCode::Char('x')));
    assert!(app.toast.is_none());
}

// ---- questions ----

#[test]
fn alt_y_and_alt_n_answer_an_approval_and_a_plain_y_is_a_letter_for_the_draft() {
    let mut app = app();
    app.down(Down::Asks { open: vec![ask(Shape::Approval)] });
    assert_eq!(app.key(alt('y')), [answer(Reply::Yes)]);
    assert_eq!(app.key(alt('n')), [answer(Reply::No)]);
    assert!(app.key(key(KeyCode::Char('y'))).is_empty());
    assert_eq!(app.line.buf, "y");
    assert!(app.key(alt('q')).is_empty(), "a chord that answers nothing is nothing");
}

#[test]
fn alt_s_and_alt_h_grant_only_what_the_question_offers() {
    let mut app = app();
    let mut a = ask(Shape::Approval);
    app.down(Down::Asks { open: vec![a.clone()] });
    assert!(app.key(alt('s')).is_empty() && app.key(alt('h')).is_empty(), "no grants offered, none given");
    a.grants = vec!["session".into()];
    app.down(Down::Asks { open: vec![a.clone()] });
    assert_eq!(app.key(alt('s')), [answer(Reply::Grant { scope: "session".into() })]);
    assert!(app.key(alt('h')).is_empty());
    a.grants = vec!["session".into(), "here".into()];
    app.down(Down::Asks { open: vec![a] });
    assert_eq!(app.key(alt('h')), [answer(Reply::Grant { scope: "here".into() })]);
}

#[test]
fn a_yes_no_question_takes_alt_y_and_alt_n_but_not_a_grant() {
    let mut app = app();
    app.down(Down::Asks { open: vec![ask(Shape::YesNo)] });
    assert_eq!(app.key(alt('y')), [answer(Reply::Yes)]);
    assert!(app.key(alt('s')).is_empty());
}

#[test]
fn a_choice_is_picked_by_number_or_by_enter_on_the_recommended_one() {
    let mut app = app();
    let mut a = ask(Shape::Choice);
    a.options = options(&[("red", false), ("green", true), ("blue", false)]);
    app.down(Down::Asks { open: vec![a] });
    assert_eq!(app.key(key(KeyCode::Char('3'))), [answer(Reply::Pick { id: "blue".into() })]);
    assert!(app.key(alt('y')).is_empty(), "yes is no pick");
    assert_eq!(app.key(key(KeyCode::Enter)), [answer(Reply::Pick { id: "green".into() })]);
    type_in(&mut app, "something else");
    assert_eq!(app.key(key(KeyCode::Enter)), [answer(Reply::Text { text: "something else".into() })], "the card is up: a typed line is the answer of its row `custom`, and the core decides what it names");
}

#[test]
fn enter_on_a_question_with_no_recommended_option_or_not_a_choice_sends_nothing() {
    let mut app = app();
    let mut a = ask(Shape::Choice);
    a.options = options(&[("red", false)]);
    app.down(Down::Asks { open: vec![a] });
    assert!(app.key(key(KeyCode::Enter)).is_empty());
    app.down(Down::Asks { open: vec![ask(Shape::Text)] });
    assert!(app.key(key(KeyCode::Enter)).is_empty());
    assert!(app.key(alt('y')).is_empty());
    let mut approval = ask(Shape::Approval);
    approval.options = options(&[("red", true)]);
    app.down(Down::Asks { open: vec![approval] });
    assert!(app.key(key(KeyCode::Enter)).is_empty(), "enter picks for a choice only");
}

#[test]
fn the_keys_are_for_the_oldest_question() {
    let mut app = app();
    let mut second = ask(Shape::Approval);
    second.ask = "t9".into();
    app.down(Down::Asks { open: vec![ask(Shape::Approval), second] });
    assert_eq!(app.key(alt('y')), [answer(Reply::Yes)], "t2, not t9");
}

// ---- lists and forms ----

fn act(key: &str, intent: Intent) -> Act {
    Act { key: key.into(), label: key.into(), intent, risk: 0 }
}

fn abort(task: &str) -> Intent {
    Intent::Control { task: task.into(), control: Control::Abort }
}

fn on_tab(app: &mut j4n_tui::App, n: u8) {
    app.key(key(KeyCode::F(n + 1)));
}

#[test]
fn up_and_down_select_a_row_and_its_action_key_or_enter_runs_it() {
    let mut app = app();
    on_tab(&mut app, 1);
    let mut r1 = row("t1", &["t1", "running"]);
    r1.actions = vec![act("a", abort("t1"))];
    let mut r2 = row("t2", &["t2", "running"]);
    r2.actions = vec![act("a", abort("t2"))];
    rows(&mut app, "tasks:all", vec![r1, r2, row("t3", &["t3", "done"])]);
    assert_eq!(app.key(key(KeyCode::Char('a'))), [Up::Intent { intent: abort("t1") }]);
    app.key(key(KeyCode::Down));
    assert_eq!(app.key(key(KeyCode::Enter)), [Up::Intent { intent: abort("t2") }]);
    app.key(key(KeyCode::Down));
    app.key(key(KeyCode::Down));
    assert_eq!(app.place("tasks:all"), 2, "no further than the last");
    assert!(app.key(key(KeyCode::Char('a'))).is_empty() && app.key(key(KeyCode::Enter)).is_empty(), "a row with no actions has none to run");
    app.key(key(KeyCode::Up));
    app.key(key(KeyCode::Up));
    app.key(key(KeyCode::Up));
    assert_eq!(app.place("tasks:all"), 0);
    assert!(app.key(key(KeyCode::Char('z'))).is_empty(), "a key no action has");
    assert!(app.key(ctrl('a')).is_empty());
}

#[test]
fn the_selection_never_points_past_the_rows_that_are_left() {
    let mut app = app();
    on_tab(&mut app, 1);
    rows(&mut app, "tasks:all", (0..5).map(|i| row(&format!("t{i}"), &["x"])).collect());
    for _ in 0..4 {
        app.key(key(KeyCode::Down));
    }
    rows(&mut app, "tasks:all", vec![row("t0", &["x"])]);
    assert_eq!(app.place("tasks:all"), 0);
    rows(&mut app, "tasks:all", vec![]);
    assert_eq!(app.place("tasks:all"), 0);
    assert!(app.key(key(KeyCode::Enter)).is_empty());
}

#[test]
fn a_field_of_a_form_is_changed_by_typing_its_new_value_and_escape_gives_it_up() {
    let mut app = app();
    on_tab(&mut app, 3);
    let set = Intent::Set { kind: "thread".into(), id: "main".into(), field: "doer".into(), value: Json::Null };
    let mut r = row("doer", &["doer", "alpha"]);
    r.actions = vec![act("enter", set)];
    rows(&mut app, "form:thread", vec![r]);
    assert!(app.key(key(KeyCode::Enter)).is_empty(), "it asks for the value first");
    assert!(app.edit.is_some());
    press(&mut app, typed("beta", false));
    app.key(key(KeyCode::Backspace));
    press(&mut app, typed("ta ", false));
    app.key(ctrl('x'));
    assert_eq!(app.edit.as_ref().map(|e| e.line.buf.as_str()), Some("betta "));
    let sent = app.key(key(KeyCode::Enter));
    assert_eq!(sent, [Up::Intent { intent: Intent::Set { kind: "thread".into(), id: "main".into(), field: "doer".into(), value: Json::String("betta".into()) } }]);
    assert!(app.edit.is_none());
    app.key(key(KeyCode::Enter));
    type_in(&mut app, "x");
    assert!(app.key(key(KeyCode::Esc)).is_empty());
    assert!(app.edit.is_none(), "given up: nothing sent, and the form is as it was");
}

#[test]
fn keys_on_the_log_page_do_nothing_but_scroll_and_switch() {
    let mut app = app();
    on_tab(&mut app, 2);
    assert!(app.key(key(KeyCode::Enter)).is_empty());
    assert!(app.key(key(KeyCode::Char('x'))).is_empty());
    assert_eq!(app.line.buf, "", "a letter on the log page is not for the chat's draft");
    app.key(key(KeyCode::PageUp));
    assert!(app.scroll.contains_key("log:all"));
}

#[test]
fn an_app_with_no_tree_ignores_keys_without_complaint() {
    let mut app = j4n_tui::App::new();
    assert!(app.key(key(KeyCode::Tab)).is_empty());
    assert!(app.key(alt('1')).is_empty());
    assert!(app.key(key(KeyCode::Char('x'))).is_empty());
    assert!(app.key(key(KeyCode::PageUp)).is_empty());
    assert!(app.key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE)).is_empty());
    assert_eq!(app.key(ctrl('c')), [Up::Quit]);
}

#[test]
fn a_toast_and_the_asks_are_state_the_next_message_replaces() {
    let mut app = app();
    app.down(Down::Asks { open: vec![ask(Shape::Approval)] });
    app.down(Down::Asks { open: vec![] });
    assert!(app.asks.is_empty());
    assert!(app.key(alt('y')).is_empty());
}

// ---- paste ----

#[test]
fn a_paste_goes_into_the_draft_with_its_line_breaks_and_sends_nothing() {
    let mut app = app();
    app.paste("fn main() {\r\n\tprintln!(\"hi\");\r}\n");
    assert_eq!(app.line.buf, "fn main() {\n println!(\"hi\");\n}\n");
    assert_eq!(app.line.at, app.line.buf.chars().count());
    let sent = app.key(key(KeyCode::Enter));
    assert_eq!(sent, [say("fn main() {\n println!(\"hi\");\n}")], "sent whole, the breaks inside it kept");
}

#[test]
fn a_paste_lands_where_the_cursor_is_and_control_characters_in_it_are_dropped() {
    let mut app = app();
    type_in(&mut app, "ab");
    app.key(key(KeyCode::Left));
    app.paste("X\u{7}Y\u{1b}[31m");
    assert_eq!(app.line.buf, "aX Y[31mb".replace(' ', ""));
}

#[test]
fn a_paste_into_a_value_being_typed_is_one_line_and_a_paste_on_another_page_is_nothing() {
    let mut app = app();
    on_tab(&mut app, 3);
    let set = Intent::Set { kind: "thread".into(), id: "main".into(), field: "model".into(), value: Json::Null };
    let mut r = row("model", &["model", "m"]);
    r.actions = vec![act("enter", set)];
    rows(&mut app, "form:thread", vec![r]);
    app.paste("ignored: no value is being typed");
    assert!(app.edit.is_none() && app.line.buf.is_empty());
    app.key(key(KeyCode::Enter));
    app.paste("claude-\nhaiku");
    assert_eq!(app.edit.as_ref().map(|e| e.line.buf.as_str()), Some("claude-haiku"));
}
