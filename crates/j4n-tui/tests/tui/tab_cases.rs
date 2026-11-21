//! The tabs: `⌥1`–`⌥9` go to a tab from anywhere, the number of the open tab again steps its sub-tabs (on Chat, the next chat), `⌥0`
//! shows the numbers, and `F1`–`F9` do what they always did. The numbers are drawn small over a cell that is blank already, so no tab
//! moves, and only for a moment after a key of the tabs. Tab moves no tab and never types: in no field does it put anything in the draft
//! but a name the field could be.

use crate::world::*;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, ModifierKeyCode};
use j4n_proto::json;
use j4n_proto::{Down, Field, FieldKind, Intent, Json, Node, Present, Up};
use j4n_tui::headless::{alt, ctrl, press as key};
use j4n_tui::leader::{HOLD, KEYS, Nav, WINDOW, read, reads, typed};
use j4n_tui::App;
use std::time::{Duration, Instant};

fn group(id: &str, kids: Vec<Node>) -> Node {
    Node { id: id.into(), title: id.into(), present: Present::Tabs, children: kids, ..Node::default() }
}

fn with_tabs(kids: Vec<Node>) -> App {
    let pages = Node { id: "pages".into(), present: Present::Tabs, children: kids, ..Node::default() };
    let root = Node { id: "root".into(), children: vec![pages, leaf("bar", "", FieldKind::Bar, "bar:status")], ..Node::default() };
    let mut a = App::new();
    a.start();
    a.down(hello());
    a.down(Down::Tree { node: root });
    a.set_clock(Instant::now());
    a
}

/// Chat · Work (sub-tabs: own, asks, inbox) · Models (a, b) · Log · Settings.
fn on() -> App {
    with_tabs(vec![
        leaf("chat", "Chat", FieldKind::Chat, "chat:main"),
        group("work", vec![leaf("work.own", "Work", FieldKind::List, "w:own"), leaf("asks", "Asks", FieldKind::List, "w:asks"), leaf("inbox", "Inbox", FieldKind::List, "w:inbox")]),
        group("models", vec![leaf("a", "A", FieldKind::List, "m:a"), leaf("b", "B", FieldKind::List, "m:b")]),
        leaf("log", "Log", FieldKind::Log, "log:all"),
        leaf("settings", "Settings", FieldKind::Form, "form:thread"),
    ])
}

/// [`on`] with the rows of Work already come: a page that waits for its rows wakes the screen for its loading sign, which a case about the
/// wakes of the numbers is not about.
fn filled() -> App {
    let mut a = on();
    for bind in ["w:own", "w:asks", "w:inbox"] {
        rows(&mut a, bind, Vec::new());
    }
    a
}

/// Nine tabs with their icons, as the base packs and the quality pack make them.
fn nine() -> App {
    let tabs = [("chat", "Chat", "chat"), ("tasks", "Tasks", "tasks"), ("library", "Library", "models"), ("connect", "Connect", "connector"), ("spend", "Spend", "spend"), ("quality", "Quality", "done"), ("grow", "Grow", "grow"), ("log", "Log", "log"), ("settings", "Settings", "settings")];
    with_tabs(tabs.iter().map(|(id, title, icon)| Node { icon: (*icon).into(), ..leaf(id, title, FieldKind::List, &format!("{id}:all")) }).collect())
}

fn tab(a: &App) -> &str {
    &a.top().expect("tabs").children[a.tab].id
}

fn sub(a: &App) -> usize {
    a.sub.get(tab(a)).copied().unwrap_or(0)
}

fn press_key(a: &mut App, code: KeyCode) {
    a.key(key(code));
}

const SMALL: &str = "¹²³⁴⁵⁶⁷⁸⁹";

/// The small numbers on a row of the strip, and the plain digits on the rule under it.
fn numbers(a: &mut App, cols: u16) -> (String, String) {
    let s = screen(a, cols, 8);
    (s[1].chars().filter(|c| SMALL.contains(*c)).collect(), s[2].chars().filter(char::is_ascii_digit).collect())
}

#[test]
fn alt_digit_goes_to_that_tab_from_any_page() {
    let mut a = on();
    a.key(alt('4'));
    assert_eq!(tab(&a), "log");
    a.key(alt('2'));
    assert_eq!((tab(&a), sub(&a)), ("work", 0), "from a page of rows too");
    a.key(alt('5'));
    assert_eq!(tab(&a), "settings", "and from a form");
    a.key(alt('9'));
    assert_eq!(tab(&a), "settings", "no ninth tab: nothing");
    a.key(alt('1'));
    type_line(&mut a, "ab");
    a.key(alt('3'));
    assert_eq!(tab(&a), "models", "a draft in the line does not keep the keys");
    a.key(alt('1'));
    assert_eq!(a.line.buf, "ab", "and it is there when the Chat is back");
}

fn type_line(a: &mut App, text: &str) {
    for k in j4n_tui::headless::typed(text, false) {
        a.key(k);
    }
}

#[test]
fn alt_digit_of_the_open_tab_steps_its_sub_tabs_and_on_chat_goes_to_the_next_chat() {
    let mut a = on();
    a.key(alt('2'));
    assert_eq!((tab(&a), sub(&a)), ("work", 0), "⌥2 from Chat goes to Work");
    a.key(alt('2'));
    assert_eq!((tab(&a), sub(&a)), ("work", 1), "its own number, again: the next sub-tab");
    a.key(alt('2'));
    a.key(alt('2'));
    assert_eq!((tab(&a), sub(&a)), ("work", 0), "round and round");
    a.key(alt('1'));
    assert_eq!(tab(&a), "chat");
    a.down(Down::Value { bind: "chat:here".into(), value: json!({ "id": "c1", "top": true }), rev: 1 });
    let ups = a.key(alt('1'));
    assert!(ups.contains(&Up::Intent { intent: Intent::Enter { chat: "@next".into() } }), "on Chat its own number is the next chat: {ups:?}");
    assert_eq!(tab(&a), "chat");
}

#[test]
fn tab_no_longer_changes_the_tab() {
    let mut a = on();
    // on an empty line Tab asks the completion for the next line, and nothing more
    for k in [key(KeyCode::Tab), KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT), key(KeyCode::Char('\t'))] {
        assert!(a.key(k).iter().all(|u| matches!(u, Up::Window { bind, .. } if bind == j4n_proto::COMPLETE)), "{k:?}");
        assert_eq!(tab(&a), "chat", "{k:?}");
    }
    press_key(&mut a, KeyCode::F(4));
    a.key(key(KeyCode::Tab));
    press_key(&mut a, KeyCode::Char('2'));
    assert_eq!(tab(&a), "work", "after a Tab a digit is what it always is: on a page of rows, the tab of that number");
    assert_eq!(numbers(&mut a, 100), (String::new(), String::new()), "Tab draws no numbers");
    let mut a = on();
    a.key(key(KeyCode::Tab));
    press_key(&mut a, KeyCode::Char('4'));
    assert_eq!((tab(&a), a.line.buf.as_str()), ("chat", "4"), "on the Chat the digit is typed");
}

/// Every cell of the strip with the numbers is the cell without them, or a number over a blank (beside a tab) or over the rule (under one).
fn moved_nothing(without: &[String], with: &[String], cols: u16) {
    for (row, blank) in [(1, " "), (2, "─━-=")] {
        let (b, w): (Vec<char>, Vec<char>) = (without[row].chars().collect(), with[row].chars().collect());
        assert_eq!(b.len(), w.len(), "{cols}: row {row}\n{}\n{}", without[row], with[row]);
        for (x, (c0, c1)) in b.iter().zip(&w).enumerate() {
            let number = SMALL.contains(*c1) || c1.is_ascii_digit();
            assert!(c0 == c1 || (number && blank.contains(*c0)), "{cols}: row {row} cell {x} was `{c0}` and is `{c1}`\n{}\n{}", without[row], with[row]);
        }
    }
}

#[test]
fn the_numbers_move_no_tab() {
    // with nine tabs the strip is spaced names at 160 and 120, close names at 100 and 80, spaced icons at 60, close icons at 44
    for (cols, beside) in [(160, true), (120, true), (100, false), (80, false), (60, true), (44, false)] {
        let mut a = nine();
        a.key(alt('3'));
        let t0 = Instant::now();
        a.tick(t0);
        let with = screen(&mut a, cols, 8);
        let shown = numbers(&mut a, cols);
        a.tick(t0 + WINDOW);
        let without = screen(&mut a, cols, 8);
        moved_nothing(&without, &with, cols);
        let all = if beside { (SMALL.to_string(), String::new()) } else { (String::new(), "123456789".to_string()) };
        assert_eq!(shown, all, "{cols}: {}\n{}", with[1], with[2]);
    }
}

#[test]
fn in_a_tier_of_plain_digits_the_numbers_stand_under_the_tabs_first_glyph() {
    for marks in ["ascii", "words"] {
        let mut a = nine();
        a.theme = j4n_tui::theme::Theme::build(&j4n_tui::theme::Settings { marks: marks.into(), ..Default::default() }, &[]).expect("a theme");
        a.key(alt('3'));
        let t0 = Instant::now();
        a.tick(t0);
        let with = screen(&mut a, 160, 8);
        a.tick(t0 + WINDOW);
        let without = screen(&mut a, 160, 8);
        moved_nothing(&without, &with, 160);
        // the strip is spaced at 160, and a plain digit beside a letter would read as one word: every number is in the rule, under the
        // first glyph of its tab, the blank before it left blank
        let (titles, rule): (Vec<char>, Vec<char>) = (with[1].chars().collect(), with[2].chars().collect());
        let under: Vec<char> = rule.iter().enumerate().filter(|(_, c)| c.is_ascii_digit()).map(|(x, _)| titles[x]).collect();
        assert!(rule.iter().filter(|c| c.is_ascii_digit()).copied().eq("123456789".chars()) && under.iter().all(|c| *c != ' '), "{marks}\n{}\n{}", with[1], with[2]);
    }
}

#[test]
fn a_cut_strip_keeps_the_numbers_of_the_tree() {
    let mut a = with_tabs((1..=9).map(|i| leaf(&format!("t{i}"), &format!("tab-{i}-of-nine"), FieldKind::List, &format!("t{i}:all"))).collect());
    a.key(alt('7'));
    let s = screen(&mut a, 50, 8);
    let titles: String = (1..=9).filter(|i| s[1].contains(&format!("tab-{i}-of-nine"))).map(|i| i.to_string()).collect();
    assert!(titles.contains('7') && !titles.starts_with('1'), "the strip is cut from the start: {}", s[1]);
    assert_eq!(numbers(&mut a, 50).1, titles, "the tabs shown keep their own numbers and a cut one has none\n{}\n{}", s[1], s[2]);
}

#[test]
fn the_numbers_show_for_a_moment_after_an_alt_digit_and_on_alt_zero() {
    let mut a = filled();
    let t0 = Instant::now();
    a.tick(t0);
    assert_eq!(numbers(&mut a, 100), (String::new(), String::new()), "none until a key of the tabs");
    a.key(alt('2'));
    assert_eq!(numbers(&mut a, 100).0, "¹²³⁴⁵", "beside each tab of a spaced strip");
    assert_eq!(a.wake(t0), Some(WINDOW), "the screen wakes when the numbers are due to go");
    a.tick(t0 + WINDOW - Duration::from_millis(1));
    assert_eq!(numbers(&mut a, 100).0, "¹²³⁴⁵");
    a.tick(t0 + WINDOW);
    assert!(a.take_redraw(), "the strip is drawn again without them");
    assert_eq!(numbers(&mut a, 100), (String::new(), String::new()));
    a.key(alt('0'));
    let shown = numbers(&mut a, 100).0;
    assert_eq!((tab(&a), shown.as_str()), ("work", "¹²³⁴⁵"), "⌥0 shows them and moves nothing");
    press_key(&mut a, KeyCode::Down);
    assert_eq!(numbers(&mut a, 100).0, "", "another key ends them");
    // while they show a key is read as it always is: on the Chat a digit is typed
    a.key(alt('1'));
    press_key(&mut a, KeyCode::Char('4'));
    assert_eq!((tab(&a), a.line.buf.as_str()), ("chat", "4"));
}

#[test]
fn f1_to_f9_still_pick_a_tab_and_the_f_key_of_the_open_tab_steps_its_sub_tabs() {
    let mut a = on();
    press_key(&mut a, KeyCode::F(3));
    assert_eq!(tab(&a), "models");
    press_key(&mut a, KeyCode::F(3));
    assert_eq!((tab(&a), sub(&a)), ("models", 1));
    press_key(&mut a, KeyCode::F(9));
    assert_eq!(tab(&a), "models", "no ninth tab");
    press_key(&mut a, KeyCode::F(10));
    assert_eq!(tab(&a), "models", "F10 is not a tab key");
}

#[test]
fn the_keys_of_the_tabs_are_one_table_and_it_reads_each_of_them() {
    let k = |c| KeyEvent::new(c, KeyModifiers::NONE);
    assert_eq!(read(&alt('4')), Some(Nav::Alt(4)));
    assert_eq!(read(&alt('0')), Some(Nav::Numbers));
    assert_eq!(read(&k(KeyCode::F(4))), Some(Nav::Fkey(4)));
    assert_eq!(read(&k(KeyCode::Char('4'))), None, "a plain digit is not a key of the tabs");
    assert_eq!(read(&KeyEvent::new(KeyCode::Char('4'), KeyModifiers::ALT | KeyModifiers::CONTROL)), None);
    for not in [k(KeyCode::Tab), KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT), k(KeyCode::Esc), k(KeyCode::Right)] {
        assert_eq!(read(&not), None, "{not:?}");
    }
    assert!(KEYS.iter().all(|r| !r.keys.is_empty() && !r.doc.is_empty()), "each row has its keys and its line for the sheet");
}

// ---- ⌥ held, where the terminal reports every key ----

/// ⌥ alone, pressed or let go, as the kitty protocol reports it with every key as a code.
fn alt_alone(kind: KeyEventKind) -> KeyEvent {
    KeyEvent::new_with_kind(KeyCode::Modifier(ModifierKeyCode::LeftAlt), KeyModifiers::ALT, kind)
}

#[test]
fn a_held_alt_shows_the_numbers_after_the_delay_and_its_release_hides_them() {
    let mut a = on();
    let t0 = Instant::now();
    a.set_clock(t0);
    assert!(a.key(alt_alone(KeyEventKind::Press)).is_empty());
    assert!(a.alt_held());
    assert_eq!(a.wake(t0), Some(HOLD), "the screen wakes when the hold's time is up");
    a.tick(t0 + HOLD - Duration::from_millis(1));
    assert_eq!(numbers(&mut a, 100), (String::new(), String::new()), "not before the delay: an ⌥← does not flash them");
    a.key(alt_alone(KeyEventKind::Repeat));
    a.tick(t0 + HOLD);
    assert!(a.take_redraw(), "the strip is drawn again with them");
    assert_eq!(numbers(&mut a, 100).0, "¹²³⁴⁵", "beside each tab, and the tab is where it was");
    a.tick(t0 + HOLD + WINDOW * 3);
    assert_eq!(numbers(&mut a, 100).0, "¹²³⁴⁵", "they stay while ⌥ is held");
    // ⌥ held, then 3 and 5: two hops, and the numbers stay
    a.key(alt('3'));
    a.key(alt('5'));
    assert_eq!(tab(&a), "settings");
    a.tick(t0 + HOLD + WINDOW * 5);
    assert_eq!(numbers(&mut a, 100).0, "¹²³⁴⁵", "an ⌥digit while held keeps them");
    assert!(a.key(alt_alone(KeyEventKind::Release)).is_empty());
    assert!(!a.alt_held());
    assert_eq!(numbers(&mut a, 100), (String::new(), String::new()), "the release hides them");
    assert_eq!(tab(&a), "settings", "and moves nothing");
    // esc takes them down too, and the hold does not bring them back
    let t1 = t0 + Duration::from_secs(10);
    a.set_clock(t1);
    a.key(alt_alone(KeyEventKind::Press));
    a.tick(t1 + HOLD);
    assert_eq!(numbers(&mut a, 100).0, "¹²³⁴⁵");
    a.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::ALT));
    a.tick(t1 + HOLD * 3);
    assert_eq!(numbers(&mut a, 100).0, "", "esc takes them down");
    // a release the terminal never sent: the next key without ⌥ ends the hold
    a.key(key(KeyCode::Down));
    assert!(!a.alt_held(), "a key without ⌥ means ⌥ is up");
}

#[test]
fn an_alt_arrow_shows_no_numbers() {
    let mut a = filled();
    a.key(alt('2'));
    let t0 = Instant::now() + WINDOW * 2;
    a.tick(t0);
    assert_eq!(numbers(&mut a, 100).0, "");
    a.set_clock(t0);
    a.key(alt_alone(KeyEventKind::Press));
    a.tick(t0 + Duration::from_millis(40));
    a.key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT));
    assert_eq!((tab(&a), sub(&a)), ("work", 1), "⌥→ steps the sub-tab");
    assert_eq!(a.wake(t0 + Duration::from_millis(40)).filter(|w| *w <= HOLD), None, "nothing waits to show the numbers");
    a.tick(t0 + HOLD * 4);
    assert_eq!(numbers(&mut a, 100), (String::new(), String::new()), "⌥ still held, but a key came with it: no numbers");
    a.key(alt_alone(KeyEventKind::Release));
    assert_eq!(numbers(&mut a, 100), (String::new(), String::new()));
}

#[test]
fn a_release_of_any_other_key_is_still_nothing() {
    let mut a = on();
    type_line(&mut a, "ab");
    let release = |code, mods| KeyEvent::new_with_kind(code, mods, KeyEventKind::Release);
    let others = [
        release(KeyCode::Char('c'), KeyModifiers::NONE),
        release(KeyCode::Char('3'), KeyModifiers::ALT),
        release(KeyCode::F(3), KeyModifiers::NONE),
        release(KeyCode::Esc, KeyModifiers::NONE),
        release(KeyCode::Enter, KeyModifiers::NONE),
        release(KeyCode::Backspace, KeyModifiers::NONE),
        release(KeyCode::Modifier(ModifierKeyCode::LeftShift), KeyModifiers::SHIFT),
        KeyEvent::new(KeyCode::Modifier(ModifierKeyCode::LeftShift), KeyModifiers::SHIFT),
        KeyEvent::new(KeyCode::Modifier(ModifierKeyCode::LeftControl), KeyModifiers::CONTROL),
        KeyEvent::new(KeyCode::Modifier(ModifierKeyCode::IsoLevel3Shift), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::CapsLock, KeyModifiers::NONE),
        KeyEvent::new_with_kind(KeyCode::Tab, KeyModifiers::NONE, KeyEventKind::Repeat),
    ];
    for k in others {
        assert!(!reads(&k), "{k:?}");
        assert!(a.key(k).is_empty(), "{k:?}");
        assert_eq!((tab(&a), a.line.buf.as_str(), a.alt_held()), ("chat", "ab", false), "{k:?}");
    }
    assert_eq!(numbers(&mut a, 100), (String::new(), String::new()));
    // a repeat of a letter is still typed, as a press is
    a.key(KeyEvent::new_with_kind(KeyCode::Char('c'), KeyModifiers::NONE, KeyEventKind::Repeat));
    assert_eq!(a.line.buf, "abc");
}

#[test]
fn under_caps_lock_a_letter_reported_as_a_code_types_its_capital() {
    let caps = |c, mods| KeyEvent { state: KeyEventState::CAPS_LOCK, ..KeyEvent::new(KeyCode::Char(c), mods) };
    assert_eq!(typed(caps('a', KeyModifiers::NONE)).code, KeyCode::Char('A'));
    assert_eq!(typed(caps('é', KeyModifiers::NONE)).code, KeyCode::Char('É'));
    assert_eq!(typed(caps('a', KeyModifiers::SHIFT)).code, KeyCode::Char('a'), "with ⇧ as well the small one is what the terminal sent");
    assert_eq!(typed(caps('a', KeyModifiers::CONTROL)).code, KeyCode::Char('a'), "⌃a is still ⌃a");
    assert_eq!(typed(caps('1', KeyModifiers::NONE)).code, KeyCode::Char('1'));
    assert_eq!(typed(caps('ß', KeyModifiers::NONE)).code, KeyCode::Char('ß'), "a letter whose capital is two is left as it is");
    assert_eq!(typed(key(KeyCode::Char('a'))).code, KeyCode::Char('a'), "no lock: as it came");
    let num = KeyEvent { state: KeyEventState::NUM_LOCK, ..key(KeyCode::Enter) };
    assert_eq!(typed(num), key(KeyCode::Enter), "a key with Num Lock on is the key");
    let mut a = app();
    a.key(caps('a', KeyModifiers::NONE));
    assert_eq!(a.line.buf, "A");
}

// ---- Tab never types ----

fn value_app(kind: FieldKind, props: Json) -> App {
    let f = Node { id: "f".into(), title: "f".into(), field: Some(Field { kind, bind: "cfg:x.f".into(), props }), ..Node::default() };
    let mut a = App::new();
    a.start();
    a.down(hello());
    a.down(Down::Tree { node: Node { id: "root".into(), present: Present::Stack, children: vec![f], ..Node::default() } });
    a.down(Down::Value { bind: "cfg:x.f".into(), value: Json::Null, rev: 1 });
    a
}

#[test]
fn tab_in_no_kind_of_value_field_puts_anything_in_the_draft_but_a_name_it_could_be() {
    let kinds = [
        (FieldKind::Str, "ab"),
        (FieldKind::Text, "ab"),
        (FieldKind::Int, "12"),
        (FieldKind::Num, "1.5"),
        (FieldKind::Dur, "5m"),
        (FieldKind::Usd, "1.5"),
        (FieldKind::Path, "~/a"),
        (FieldKind::Argv, "ab"),
        (FieldKind::Pattern, "a*"),
        (FieldKind::Expr, "a"),
        (FieldKind::Secret, "ab"),
        (FieldKind::Markdown, "ab"),
        (FieldKind::Ref, "ab"),
        (FieldKind::Seq, "ab"),
        (FieldKind::Map, "a=b"),
    ];
    for (kind, text) in kinds {
        for tab in [key(KeyCode::Tab), KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT), key(KeyCode::Char('\t'))] {
            let mut a = value_app(kind, json!({ "options": ["ab", "abc"] }));
            a.key(key(KeyCode::Enter));
            keys_of(&mut a, text);
            let draft = |a: &App| a.val.as_ref().map(|e| (e.line.buf.clone(), e.line.at, e.items.clone()));
            let before = draft(&a);
            assert!(before.is_some(), "{kind:?} is being edited");
            assert!(a.key(tab).is_empty());
            if matches!(kind, FieldKind::Ref | FieldKind::Path) {
                // a name the field could be is what Tab puts there, and nothing else
                let now = draft(&a).map(|d| d.0).unwrap_or_default();
                assert!(now == text || ["ab", "abc"].contains(&now.as_str()), "{kind:?}: Tab put `{now}` there");
            } else {
                assert_eq!(draft(&a), before, "{kind:?}: Tab is not a character and not an edit");
            }
        }
    }
    for kind in [FieldKind::Bool, FieldKind::Enum, FieldKind::Multi] {
        let mut a = value_app(kind, json!({ "options": ["x", "y"] }));
        a.key(key(KeyCode::Enter));
        let before = a.val.as_ref().map(|e| e.pick);
        a.key(key(KeyCode::Tab));
        assert_eq!(a.val.as_ref().map(|e| e.pick), before, "{kind:?}");
    }
}

fn keys_of(a: &mut App, text: &str) {
    for k in j4n_tui::headless::typed(text, false) {
        a.key(k);
    }
}

#[test]
fn tab_never_types_in_the_composer_a_terminal_that_reports_it_as_a_character_is_read_as_the_key() {
    let mut a = app();
    type_in(&mut a, "ab");
    a.key(key(KeyCode::Tab));
    assert_eq!(a.line.buf, "ab", "the draft is as it was");
    let mut a = app();
    type_in(&mut a, "ab");
    a.key(key(KeyCode::Char('\t')));
    assert_eq!((a.line.buf.as_str(), a.tab), ("ab", 0), "⇥ sent as a character is read as the key: no tab in the draft, and no other tab");
    // a tab inside a paste is text, and is one space
    let mut a = app();
    a.paste("a\tb");
    assert_eq!(a.line.buf, "a b");
}

#[test]
fn the_keys_of_the_tabs_wait_while_a_value_a_find_or_the_jump_is_being_typed() {
    let mut a = on();
    press_key(&mut a, KeyCode::F(3));
    a.key(ctrl('k'));
    assert!(a.jump.is_some());
    a.key(key(KeyCode::Tab));
    a.key(alt('4'));
    assert_eq!(tab(&a), "models", "the jump keeps its keys");
    assert!(a.jump.is_some());
    a.key(key(KeyCode::Esc));
    a.key(alt('4'));
    assert_eq!(tab(&a), "log");
}
