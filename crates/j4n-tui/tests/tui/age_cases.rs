//! The screen says how long ago a row's event was, and counts a window's reset down, from its own clock. The core sends the event's time
//! (`data.at_ms`, an entry's `resets_ms`) with the text it made; the screen wakes for the soonest word that changes and for nothing else.

use crate::world::{hello, screen};
use j4n_proto::{Cell, Down, Field, FieldKind, Json, Node, Present, Row, Tone, json};
use j4n_tui::App;
use j4n_tui::age::{ago, ago_wait, until, until_wait};
use j4n_tui::theme::{Settings, Theme};
use std::time::{Duration, Instant};

const NOON: u64 = 1_700_000_000_000;

fn started() -> App {
    let mut a = App::new();
    a.start();
    a.down(hello());
    let chat = Node { id: "chat".into(), field: Some(Field { kind: FieldKind::Chat, bind: "chat:main".into(), props: Json::Null }), ..Node::default() };
    a.down(Down::Tree { node: Node { id: "root".into(), present: Present::Stack, children: vec![chat], ..Node::default() } });
    a.theme = Theme::build(&Settings { marks: "unicode".into(), you: "mark".into(), ..Settings::default() }, &[]).expect("a theme");
    a
}

fn send(a: &mut App, rows: Vec<Row>) {
    let total = rows.len() as u64;
    a.down(Down::Rows { bind: "chat:main".into(), rows, total, from: 0, rev: 1 });
}

/// A turn that ends in `12:10`, the clock text the core made, and `at_ms`, when it was.
fn turn(at_ms: u64) -> Row {
    Row { key: "t1".into(), cells: vec![Cell::new("fix the parser", Tone::Plain), Cell::new("", Tone::Dim), Cell::new("12:10", Tone::Dim)], data: Some(json!({ "at_ms": at_ms })), ..Row::default() }
}

fn look(time: &str) -> Down {
    Down::Theme { look: j4n_proto::Look { palette: "jjj".into(), marks: "unicode".into(), frame: "pipes".into(), corners: "round".into(), title_place: "inside".into(), density: "normal".into(), fill: "match".into(), you: "mark".into(), motion: true, mouse: "on".into(), read: Default::default(), time: time.into(), chart: "dither".into() } }
}

#[test]
fn the_age_is_said_in_the_width_of_a_clock() {
    for (secs, word) in [(-5, "now"), (0, "now"), (1, "1s"), (5, "5s"), (59, "59s"), (60, "1m"), (179, "2m"), (3599, "59m"), (3600, "1h"), (3661, "1h1m"), (7260, "2h1m"), (86_399, "23h59m"), (90_000, "1d1h"), (97_200, "1d3h"), (86_400, "1d"), (864_000, "10d"), (4_000_000, "46d")] {
        assert_eq!(ago(secs), word, "{secs} s ago");
        assert!(word.chars().count() <= 6, "`{word}` is no wider than a clock and a little");
    }
    assert_eq!(until(2_160_000), "0:36", "the countdown is the core's words: hours and minutes");
    assert_eq!(until(2 * 86_400_000 + 4 * 3_600_000), "2d 4h");
}

#[test]
fn a_word_is_looked_at_again_when_it_changes_and_not_before() {
    assert_eq!(ago_wait(0), 1000, "under a minute: each second");
    assert_eq!(ago_wait(1500), 500);
    assert_eq!(ago_wait(61_000), 59_000, "from a minute: each minute, at the minute");
    assert_eq!(ago_wait(3_660_000 + 20_000), 40_000);
    assert_eq!(ago_wait(90_000_000), 3_600_000, "from a day: each hour (25 h is on the hour, so a whole hour waits)");
    assert_eq!(ago_wait(-2000), 2001, "a time still to come says `now` until it comes");
    assert_eq!(until_wait(0), None, "a reset that has come has nothing to count");
    assert_eq!(until_wait(2_160_000), Some(1), "36:00 left says 0:36 until it is 0:35");
    assert_eq!(until_wait(2_159_999), Some(60_000), "35:59.999 left says 0:35 until it is under 35:00");
}

#[test]
fn a_row_two_hours_old_says_two_hours() {
    let mut a = started();
    let t0 = Instant::now();
    a.set_wall(NOON, t0);
    send(&mut a, vec![turn(NOON - 2 * 3_600_000 - 60_000)]);
    let s = screen(&mut a, 80, 12).join("\n");
    assert!(s.contains("2h1m"), "the row says how long ago it was: {s}");
    assert!(!s.contains("12:10"), "and not the clock text the core made: {s}");
}

#[test]
fn a_screen_without_a_clock_says_the_time_of_day_the_core_sent() {
    let mut a = started();
    send(&mut a, vec![turn(NOON - 7_200_000)]);
    let s = screen(&mut a, 80, 12).join("\n");
    assert!(s.contains("12:10"), "a replay or a pipe reads the text of the row: {s}");
}

#[test]
fn the_age_moves_on_by_itself() {
    let mut a = started();
    let t0 = Instant::now();
    a.set_wall(NOON, t0);
    send(&mut a, vec![turn(NOON - 1000)]);
    assert!(screen(&mut a, 80, 12).join("\n").contains(" 1s"), "a second old");
    // no event came: the clock moves, the screen says it is to be drawn again, and the word is a second older
    let wait = a.wake(t0).expect("a row with an age in sight wakes the screen");
    assert!(wait <= Duration::from_secs(1), "within the second: {wait:?}");
    a.tick(t0 + Duration::from_secs(1));
    assert!(a.take_redraw(), "the word changed");
    assert!(screen(&mut a, 80, 12).join("\n").contains(" 2s"), "two seconds old");
    // a quarter of a second later it is still two seconds: nothing to draw
    a.tick(t0 + Duration::from_millis(1250));
    assert!(!a.take_redraw(), "no word changed");
}

#[test]
fn an_old_row_wakes_the_screen_once_a_minute_and_none_wakes_it_for_nothing() {
    let mut a = started();
    let t0 = Instant::now();
    a.set_wall(NOON, t0);
    send(&mut a, vec![turn(NOON - 3_600_000 * 3)]);
    screen(&mut a, 80, 12);
    let wait = a.wake(t0).expect("an age is in sight");
    assert!(wait > Duration::from_secs(1) && wait <= Duration::from_secs(60), "a word of minutes is looked at each minute: {wait:?}");
    // the same screen with a row that has no age: the clock is no reason to wake
    let mut b = started();
    b.set_wall(NOON, t0);
    send(&mut b, vec![Row { data: None, ..turn(0) }]);
    screen(&mut b, 80, 12);
    assert_eq!(b.wake(t0), None, "nothing on the screen ages");
}

#[test]
fn look_time_clock_keeps_the_time_of_day() {
    let mut a = started();
    a.set_wall(NOON, Instant::now());
    a.down(look("clock"));
    send(&mut a, vec![turn(NOON - 7_200_000)]);
    let s = screen(&mut a, 80, 12).join("\n");
    assert!(s.contains("12:10") && !s.contains("2h"), "`look.time = clock` is the time of day: {s}");
    assert_eq!(a.wake(Instant::now()), None, "and nothing ages");
    a.down(look("ago"));
    assert!(screen(&mut a, 80, 12).join("\n").contains("2h"), "back to ago");
}

fn usage(at: u64, left: &str) -> Row {
    let entries = json!([{ "label": "5h", "f": 0.5, "note": format!("resets in {left} (13:50 UTC) · 50%/h"), "resets_ms": at, "left": left }]);
    Row { key: "v1".into(), cells: vec![Cell::new("usage", Tone::Dim), Cell::new("", Tone::Dim)], data: Some(json!({ "view": { "id": "usage", "field": "meter", "title": "usage", "value": entries } })), ..Row { parent: Some("t1".into()), ..Row::default() } }
}

#[test]
fn the_usage_countdown_moves_without_an_event() {
    let mut a = started();
    let t0 = Instant::now();
    a.set_wall(NOON, t0);
    let mut head = turn(NOON);
    head.data = None;
    send(&mut a, vec![head, usage(NOON + 36 * 60_000, "0:36")]);
    let s = screen(&mut a, 100, 14).join("\n");
    assert!(s.contains("resets in 0:36 (13:50 UTC)"), "as the core made it: {s}");
    let wait = a.wake(t0).expect("a countdown in sight wakes the screen");
    assert!(wait <= Duration::from_secs(60), "{wait:?}");
    a.tick(t0 + Duration::from_secs(61));
    assert!(a.take_redraw(), "a minute went by");
    let s = screen(&mut a, 100, 14).join("\n");
    assert!(s.contains("resets in 0:34 (13:50 UTC)"), "the screen counted down by itself: {s}");
    assert!(!s.contains("resets in 0:36"), "{s}");
}
