//! A row of the Log that carries its event is one readable line and opens into a tree;
//! `r` shows its raw JSON, `c` goes to its cause, `k` and `t` keep only its kind or task, `y` copies it, `z` opens them all.

use crate::world::{hello, screen};
use j4n_proto::{Cell, Down, Field, FieldKind, Json, Node, Present, Row, Tone, Up, json};
use j4n_tui::App;
use j4n_tui::display::event::{self, Look};
use std::sync::OnceLock;
use std::time::Duration;
use j4n_tui::headless::{Headless, KeyCode, press};
use j4n_tui::theme::palette::Role;
use j4n_tui::theme::{Settings, Theme};

/// 2025-11-06 08:12:04.120 UTC.
const AT: u64 = 1_762_416_724_120;

fn ev(seq: u64, kind: &str, task: Option<&str>, data: Json) -> Json {
    let mut e = json!({ "seq": seq, "at_ms": AT + seq * 1000, "kind": kind, "by": "core", "class": "project", "data": data });
    if let Some(t) = task {
        e["task"] = t.into();
    }
    e
}

fn row_of(e: Json) -> Row {
    let seq = e["seq"].as_u64().unwrap();
    Row { key: format!("e{seq}"), cells: vec![Cell::new(seq.to_string(), Tone::Dim), Cell::new(format!("{} {}", e["kind"].as_str().unwrap(), e["data"]), Tone::Plain)], data: Some(e), ..Row::default() }
}

/// The built-in look file as the core sends it (`fixtures/look.json`, which a core test keeps equal to `packs/base/look/look.toml`).
fn look_file() -> Json {
    include_str!("../../../../fixtures/look.json").parse().expect("the look file is JSON")
}

fn look() -> &'static Look {
    static LOOK: OnceLock<Look> = OnceLock::new();
    LOOK.get_or_init(|| Look::of(&look_file()))
}

fn long() -> String {
    (1..=60).map(|i| format!("word{i}")).collect::<Vec<_>>().join(" ")
}

/// Events of very different shapes, `e1`–`e6`.
fn shapes() -> Vec<Json> {
    vec![
        ev(1, "task.started", Some("t3"), json!({ "doer": "claude", "resumed": false })),
        ev(2, "run.shell", Some("t3"), json!({ "cmd": "python3 -m unittest discover", "exit": 1, "out": "line one\nline two\nFAILED (errors=1)", "took_ms": 1234 })),
        ev(3, "gate.verdict", None, json!({ "ok": true, "suite": { "name": "core", "tests": { "passed": 3000, "failed": 0 } }, "cost_usd": 0.012, "bytes": 2_500_000 })),
        ev(4, "chat.note", None, json!({ "text": long(), "tone": "dim" })),
        ev(5, "plan.item", Some("t4"), json!({ "steps": ["read", "write", { "check": "gate" }] })),
        json!({ "seq": 6, "at_ms": AT + 6000, "kind": "zz.never_seen", "by": "doer:pi", "cause": 2, "class": "private", "thread": "main", "data": "a bare string" }),
    ]
}

fn app_with(rows: Vec<Row>, total: u64, from: u64) -> App {
    let mut a = App::new();
    a.start();
    a.down(hello());
    let field = Field { kind: FieldKind::Log, bind: "log:all".into(), props: Json::Null };
    let leaf = Node { id: "l".into(), field: Some(field), ..Node::default() };
    a.down(Down::Tree { node: Node { id: "root".into(), present: Present::Stack, children: vec![leaf], ..Node::default() } });
    a.down(Down::Value { bind: j4n_proto::LOOK_FILE.into(), value: look_file(), rev: 1 });
    a.down(Down::Rows { bind: "log:all".into(), rows, total, from, rev: 1 });
    a
}

fn shapes_app() -> App {
    let rows: Vec<Row> = shapes().into_iter().map(row_of).collect();
    let n = rows.len() as u64;
    app_with(rows, n, 0)
}

fn hit(a: &mut App, code: KeyCode) -> Vec<Up> {
    a.key(press(code))
}

/// The lines of the screen, without the scroll thumb at their end.
fn lines(a: &mut App, cols: u16, rows: u16) -> Vec<String> {
    screen(a, cols, rows).into_iter().map(|l| l.trim_end().trim_end_matches('┃').trim_end().to_string()).collect()
}

/// To the row `i` from the top.
fn to(a: &mut App, i: usize) {
    hit(a, KeyCode::Home);
    for _ in 0..i {
        hit(a, KeyCode::Down);
    }
}

#[test]
fn events_of_every_shape_are_one_readable_line_with_no_json() {
    let mut a = shapes_app();
    let s = lines(&mut a, 100, 8);
    for (l, (time, kind)) in s.iter().zip([("08:12:05", "task.started"), ("08:12:06", "run.shell"), ("08:12:07", "gate.verdict"), ("08:12:08", "chat.note"), ("08:12:09", "plan.item"), ("08:12:10", "zz.never_seen")]) {
        assert!(l.chars().take(12).collect::<String>().contains(time) && l.contains(kind), "the time, not the seq, and the kind: {l}");
        assert!(!l.contains('"') && !l.contains("{\""), "no JSON on the line: {l}");
        assert!(l.ends_with('▸'), "every event opens: {l}");
        assert!(l.chars().count() <= 100, "{l}");
    }
    assert!(s[0].contains("task.started   claude ") && s[0].ends_with("t3 ▸"), "{}", s[0]);
    assert!(s[1].contains("python3 -m unittest discover"), "the kind's summary: {}", s[1]);
    assert!(s[2].contains("bytes 2.4 MB") && s[2].contains("cost_usd $0.0120") && s[2].contains("ok \u{f012c}") && !s[2].contains("true"), "the first short fields, in their look: {}", s[2]);
    assert!(s[3].contains("word1 word2") && s[3].contains('…') && !s[3].contains("word60"), "a long text is cut on the line: {}", s[3]);
    assert!(s[4].contains("plan.item") && s[4].contains("{1}"), "nothing short: how many fields: {}", s[4]);
    assert!(s[5].contains("a bare string"), "{}", s[5]);
}

#[test]
fn errors_and_refusals_take_the_bad_tone_and_a_kind_takes_its_familys_colour() {
    let a = App::new();
    let l = look();
    let [started, shell, gate, ..] = &shapes()[..] else { unreachable!() };
    assert_eq!((l.tone(started), l.tone(shell), l.tone(gate)), (Tone::Plain, Tone::Bad, Tone::Plain), "a shell that exited 1 is bad");
    assert_eq!(l.tone(&ev(9, "grant.revoked", None, json!({}))), Tone::Bad);
    assert_eq!(l.tone(&ev(9, "task.waiting", None, json!({ "error": "no route" }))), Tone::Bad);
    assert_eq!(l.tone(&ev(9, "chat.note", None, json!({ "tone": "warn" }))), Tone::Warn);
    // a queued task's line says its goal, not the count of its data's keys (`{1}`: the data is `{ "spec": … }`)
    let queued = ev(9, "task.queued", Some("t5"), json!({ "spec": { "goal": "fix the parser", "doer": "claude", "input": {} } }));
    let text: String = event::summary(&a.theme, l, &queued, l.tone(&queued)).iter().map(|s| s.content.to_string()).collect();
    assert_eq!(text, "fix the parser");
    // a broken hash chain is a tamper alarm, not routine log chatter
    let broken = ev(9, "log.broken", None, json!({ "after": 41, "at": 42, "lines": 3 }));
    assert_eq!(l.tone(&broken), Tone::Bad, "log.broken is bad");
    let spans = event::line(&a.theme, l, &broken, 12, 80);
    assert!(spans.iter().any(|s| s.content.contains("after 41, 3 lines") && s.style == a.theme.fg(Role::Error)), "{spans:?}");
    let spans = event::line(&a.theme, l, shell, 12, 80);
    assert!(spans.iter().any(|s| s.content.contains("python3") && s.style == a.theme.fg(Role::Error)), "{spans:?}");
    let spans = event::line(&a.theme, l, gate, 12, 80);
    assert!(spans.iter().any(|s| s.content.starts_with("gate.verdict") && s.style == a.theme.fg(Role::Ok)), "{spans:?}");
    assert_eq!(l.family("zz.never_seen"), l.other, "an unknown kind still has a glyph");
    assert_eq!(l.family("run.tool_call").0, j4n_tui::theme::glyph::Slot::Tool, "the longest prefix wins");
}

#[test]
fn an_open_event_is_the_whole_event_as_a_pipe_tree_with_values_in_their_look() {
    let mut a = shapes_app();
    to(&mut a, 2);
    hit(&mut a, KeyCode::Enter);
    let s = lines(&mut a, 80, 20);
    let tree: Vec<&str> = s[3..].iter().take_while(|l| !l.contains("08:12:08")).map(String::as_str).collect();
    let want = [
        "  ├ bytes     2.4 MB",
        "  ├ cost_usd  $0.0120",
        "  ├ ok        \u{f012c}",
        "  ├ suite",
        "  │ ├ name   core",
        "  │ ╰ tests",
        "  │   ├ failed  0",
        "  │   ╰ passed  3000",
        "  ├ seq       3",
        "  ├ by        core",
        "  ├ class     project",
        "  ╰ at_ms     2025-11-06 08:12:07.120",
    ];
    assert_eq!(tree, want, "{s:#?}");
    assert!(s[2].ends_with('▾'), "{}", s[2]);
    hit(&mut a, KeyCode::Char(' '));
    assert!(lines(&mut a, 80, 20)[3].contains("08:12:08"), "space shuts it again");
}

#[test]
fn a_long_string_is_shown_whole_when_open_and_lines_and_lists_keep_their_shape() {
    let mut a = shapes_app();
    to(&mut a, 3);
    hit(&mut a, KeyCode::Enter);
    let s = lines(&mut a, 60, 30);
    let text: String = s.iter().skip(4).take_while(|l| !l.contains("tone")).map(|l| l.trim_start_matches([' ', '├', '│', '╰']).trim_start_matches("text").trim()).collect::<Vec<_>>().join(" ");
    assert_eq!(text, long(), "every word, wrapped, none cut: {s:#?}");
    assert!(s.iter().all(|l| l.chars().count() <= 60));
    to(&mut a, 1);
    hit(&mut a, KeyCode::Enter);
    let s = lines(&mut a, 60, 30);
    let at = s.iter().position(|l| l.contains("├ out")).expect("out");
    assert!(s[at].ends_with("line one") && s[at + 1].ends_with("line two") && s[at + 2].ends_with("FAILED (errors=1)"), "a text of lines stays lines: {s:#?}");
    assert!(s.iter().any(|l| l.contains("took_ms") && l.ends_with("1.2 s")), "milliseconds as a duration");
    to(&mut a, 4);
    hit(&mut a, KeyCode::Enter);
    let s = lines(&mut a, 60, 30);
    let at = s.iter().position(|l| l.contains("steps")).expect("steps");
    assert_eq!(&s[at + 1..at + 5], ["  │ ├ 1  read", "  │ ├ 2  write", "  │ ╰ 3", "  │   ╰ check  gate"], "a list is numbered");
}

#[test]
fn right_and_left_open_and_shut_one_level_and_z_opens_every_event() {
    let mut a = shapes_app();
    to(&mut a, 2);
    hit(&mut a, KeyCode::Right);
    let s = lines(&mut a, 80, 20);
    assert!(s.contains(&"  ├ suite     {2} ▸".to_string()), "one level: a map deeper is its size: {s:#?}");
    hit(&mut a, KeyCode::Right);
    let s = lines(&mut a, 80, 20);
    assert!(s.contains(&"  │ ╰ tests  {2} ▸".to_string()), "{s:#?}");
    for _ in 0..5 {
        hit(&mut a, KeyCode::Right);
    }
    assert!(lines(&mut a, 80, 30).iter().any(|l| l.ends_with("passed  3000")), "no deeper than the event goes");
    hit(&mut a, KeyCode::Left);
    assert!(lines(&mut a, 80, 20).contains(&"  │ ╰ tests  {2} ▸".to_string()), "back one level from the deepest");
    hit(&mut a, KeyCode::Left);
    hit(&mut a, KeyCode::Left);
    assert!(lines(&mut a, 80, 20)[2].ends_with('▸'), "shut");
    hit(&mut a, KeyCode::Char('z'));
    let s = lines(&mut a, 80, 80);
    assert!(s.iter().filter(|l| l.ends_with('▾')).count() == 6, "all of them open: {s:#?}");
    hit(&mut a, KeyCode::Char('z'));
    assert!(lines(&mut a, 80, 80).iter().filter(|l| l.ends_with('▸')).count() == 6, "and shut");
}

#[test]
fn r_shows_the_raw_json_of_the_event_and_y_copies_it() {
    let mut a = shapes_app();
    to(&mut a, 5);
    hit(&mut a, KeyCode::Char('r'));
    let s = lines(&mut a, 80, 30);
    let raw: Vec<&str> = s[6..].iter().map(|l| l.trim_start_matches([' ', '▎']).trim_start()).collect();
    assert!(raw.contains(&"\"kind\": \"zz.never_seen\",") && raw.contains(&"\"data\": \"a bare string\","), "{s:#?}");
    hit(&mut a, KeyCode::Char('r'));
    assert_eq!(lines(&mut a, 80, 30).len(), 6 + lines(&mut a, 80, 30)[6..].len());
    assert!(!lines(&mut a, 80, 30).iter().any(|l| l.contains("\"kind\"")), "r again: shut");
    hit(&mut a, KeyCode::Char('y'));
    let copied = a.wins["log:all"].copy.clone().expect("copied");
    assert_eq!(serde_like(&copied), shapes()[5], "the event, whole");
    assert_eq!(event::osc52("hi"), "\x1b]52;c;aGk=\x07");
    assert_eq!(event::osc52("hello"), "\x1b]52;c;aGVsbG8=\x07");
    assert_eq!(event::osc52("abc"), "\x1b]52;c;YWJj\x07");
}

fn serde_like(text: &str) -> Json {
    text.parse().expect("JSON")
}

#[test]
fn c_goes_to_the_cause_of_the_event_held_or_not() {
    let mut a = shapes_app();
    to(&mut a, 5);
    hit(&mut a, KeyCode::Char('c'));
    assert_eq!(a.selected(a.field().unwrap()), 1, "e6's cause is e2, held");
    hit(&mut a, KeyCode::Char('c'));
    assert_eq!(a.selected(a.field().unwrap()), 1, "e2 has no cause: the keys stay");
    // a million events, the end held; the cause is far up
    let total = 1_000_000u64;
    let held: Vec<Row> = (total - 100..total).map(|p| row_of(ev(p + 1, "task.kept", None, json!({ "n": p })))).collect();
    let mut a = app_with(held, total, total - 100);
    let mut last = ev(total, "task.settled", None, json!({}));
    last["cause"] = 12.into();
    a.down(Down::Rows { bind: "log:all".into(), rows: (total - 100..total - 1).map(|p| row_of(ev(p + 1, "task.kept", None, json!({})))).chain([row_of(last)]).collect(), total, from: total - 100, rev: 2 });
    a.windows();
    let ask: Vec<Up> = hit(&mut a, KeyCode::Char('c')).into_iter().chain(a.windows()).collect();
    assert_eq!(a.selected(a.field().unwrap()), 11, "the place it would have");
    let Some(Up::Window { from, count, .. }) = ask.first() else { panic!("{ask:?}") };
    assert!(*from <= 11 && from + count > 11);
    a.down(Down::Rows { bind: "log:all".into(), rows: (*from..from + count).map(|p| row_of(ev(p + 1, "task.kept", None, json!({})))).collect(), total, from: *from, rev: 3 });
    a.windows();
    let (sel, from) = (a.selected(a.field().unwrap()), a.span["log:all"].0);
    assert_eq!(a.rows["log:all"].get((sel - from) as usize).map(|r| r.key.as_str()), Some("e12"));
    assert!(a.wins["log:all"].goto.is_none(), "arrived");
}

#[test]
fn k_and_t_keep_only_this_kind_or_this_task_and_take_it_off_again() {
    let mut a = shapes_app();
    to(&mut a, 1);
    let ask = hit(&mut a, KeyCode::Char('k'));
    let ask: Vec<Up> = ask.into_iter().chain(a.windows()).collect();
    assert!(ask.iter().any(|u| matches!(u, Up::Window { find, .. } if find == "kind:run.shell")), "{ask:?}");
    to(&mut a, 1);
    hit(&mut a, KeyCode::Char('t'));
    assert_eq!(a.wins["log:all"].find, "task:t3 kind:run.shell");
    to(&mut a, 1);
    hit(&mut a, KeyCode::Char('k'));
    assert_eq!(a.wins["log:all"].find, "task:t3", "k again takes the kind off");
    to(&mut a, 5);
    hit(&mut a, KeyCode::Char('t'));
    assert_eq!(a.wins["log:all"].find, "", "t on an event of no task takes the task off");
}

#[test]
fn values_are_shown_by_what_they_are() {
    assert_eq!([event::duration(340.0), event::duration(1234.0), event::duration(125_000.0), event::duration(3_720_000.0)], ["340 ms", "1.2 s", "2 m 05 s", "1 h 02 m"]);
    assert_eq!([event::time(0), event::time(951_782_400_000), event::time(4_107_542_399_999), event::time(AT)], ["1970-01-01 00:00:00.000", "2000-02-29 00:00:00.000", "2100-02-28 23:59:59.999", "2025-11-06 08:12:04.120"]);
    assert_eq!([event::bytes(512.0), event::bytes(4096.0), event::bytes(2_500_000.0)], ["512 B", "4.0 kB", "2.4 MB"]);
    let l = look();
    use event::Show;
    assert_eq!([l.show("at_ms"), l.show("took_ms"), l.show("ms"), l.show("_ms"), l.show("cost"), l.show("in_usd"), l.show("cwd"), l.show("name")], [Show::Time, Show::Duration, Show::Duration, Show::Text, Show::Money, Show::Money, Show::Path, Show::Text]);
    // a result is a tick and a switch is on or off; any other yes or no is as the event has it
    assert_eq!([l.show("ok"), l.show("pass"), l.show("on"), l.show("live"), l.show("told")], [Show::Tick, Show::Tick, Show::OnOff, Show::OnOff, Show::Text]);
    // the yes or no fields the Log shows are in the look, each as a result, a switch or as it is
    assert_eq!([l.show("passed"), l.show("steerable"), l.show("pinned"), l.show("grantable"), l.show("auto")], [Show::Tick, Show::OnOff, Show::OnOff, Show::OnOff, Show::Text]);
}

#[test]
fn tab_does_nothing_on_the_log() {
    let mut a = shapes_app();
    to(&mut a, 2);
    hit(&mut a, KeyCode::Tab);
    assert!(lines(&mut a, 80, 10)[2].ends_with('▸'), "Tab is never a Log key");
}

#[test]
fn a_million_events_scroll_open_and_shut_without_a_wait() {
    let total: u64 = 1_000_000;
    let mut a = app_with(Vec::new(), total, 0);
    let mut screen = Headless::new(160, 48);
    let make = |p: u64| row_of(ev(p + 1, ["task.kept", "run.shell", "gate.verdict"][(p % 3) as usize], Some("t1"), json!({ "n": p, "cmd": "cargo test", "nested": { "a": [1, 2, 3], "b": "x".repeat(300) } })));
    let serve = |a: &mut App, ups: Vec<Up>| {
        for u in ups {
            if let Up::Window { bind, from, count, .. } = u {
                let count = count.min(total - from);
                a.down(Down::Rows { bind, rows: (from..from + count).map(make).collect(), total, from, rev: 1 });
            }
        }
    };
    let ups = a.windows();
    serve(&mut a, ups);
    let (_, first) = crate::cpu::of(|| screen.show(&mut a));
    let t = crate::cpu::start();
    let mut keys = 0u32;
    let mut go = |a: &mut App, code: KeyCode| {
        let ups = a.key(press(code));
        let ups: Vec<Up> = ups.into_iter().chain(a.windows()).collect();
        serve(a, ups);
        let _ = screen.show(a);
        keys += 1;
    };
    for _ in 0..30 {
        go(&mut a, KeyCode::PageUp);
    }
    for _ in 0..100 {
        go(&mut a, KeyCode::Up);
        go(&mut a, KeyCode::Enter);
    }
    go(&mut a, KeyCode::Char('z'));
    for _ in 0..50 {
        go(&mut a, KeyCode::Down);
    }
    go(&mut a, KeyCode::Char('G'));
    let per = t.elapsed() / keys;
    // generous for a debug build, as the log of rows (display_cases); the CPU of this thread, held as a quiet machine would take it
    crate::cpu::assert_within(Duration::from_millis(500), "first frame", || first);
    crate::cpu::assert_within(Duration::from_millis(50), "key and frame", || per);
    assert!(a.rows["log:all"].len() < 400);
    eprintln!("1M-event log: first frame {first:?}, key+frame {per:?} over {keys} keys");
}

#[test]
fn an_open_event_taller_than_the_screen_is_scrolled_through_before_the_keys_move_on() {
    let mut a = shapes_app();
    to(&mut a, 3);
    hit(&mut a, KeyCode::Enter);
    let s = lines(&mut a, 60, 8);
    assert!(s[0].contains("chat.note") && !s.iter().any(|l| l.contains("at_ms")), "the top of it: {s:#?}");
    let mut downs = 0;
    while !lines(&mut a, 60, 8).iter().any(|l| l.contains("at_ms")) {
        hit(&mut a, KeyCode::Down);
        downs += 1;
        assert!(downs < 20, "never reaches its end");
    }
    let s = lines(&mut a, 60, 8);
    assert!(s.iter().any(|l| l.contains("word60")) && a.selected(a.field().unwrap()) == 3, "every word is reached, on the same row: {s:#?}");
    hit(&mut a, KeyCode::Down);
    assert_eq!(a.selected(a.field().unwrap()), 4, "past its end the keys move on");
    hit(&mut a, KeyCode::Up);
    assert!(lines(&mut a, 60, 8).iter().any(|l| l.starts_with("┃ ") && l.contains("chat.note")));
    hit(&mut a, KeyCode::PageDown);
    assert!(!lines(&mut a, 60, 8).iter().any(|l| l.contains("chat.note")), "a page into it");
}

#[test]
fn a_log_that_follows_shows_the_end_of_a_tall_open_event_above_the_last() {
    let mut a = shapes_app();
    to(&mut a, 3);
    hit(&mut a, KeyCode::Enter);
    hit(&mut a, KeyCode::Char('G'));
    let s = lines(&mut a, 60, 8);
    assert!(s[7].contains("zz.never_seen") && s[6].contains("plan.item"), "the end at the bottom: {s:#?}");
    assert!(s[5].contains("at_ms") && s[0].starts_with("  │"), "and the end of the open event above it, not a blank screen: {s:#?}");
}

#[test]
fn the_look_files_words_are_the_themes_own_and_a_kind_no_family_has_takes_the_bare_look() {
    // the kind `look` names the slots, roles and renderers an entry may say: each is the theme's, and every role a line can
    // be drawn in and every renderer is there, so a role or a renderer added to the theme fails here until the look file knows it
    let kind = include_str!("../../../../packs/base/kinds/look.toml");
    let words = |field: &str| -> Vec<&str> {
        let line = kind.lines().find(|l| l.starts_with(&format!("{field} = "))).expect(field);
        line.split("enum:").nth(1).and_then(|r| r.split('"').next()).expect("an enum").split('|').collect()
    };
    assert!(words("glyph").iter().all(|g| j4n_tui::theme::glyph::Slot::by_name(g).is_some()), "{:?}", words("glyph"));
    let roles: Vec<&str> = Role::ALL.iter().map(|r| r.name()).filter(|r| !["bg", "raise", "sel", "edge"].contains(r)).collect();
    assert_eq!(words("role"), roles);
    assert_eq!(words("show"), ["text", "duration", "time", "money", "bytes", "path", "cmd", "diff", "prose", "onoff", "tick"]);
    assert!(words("show").iter().all(|s| event::Show::parse(s).is_some()));
    let bare = Look::default();
    assert_eq!((bare.family("task.started"), bare.summaries.len(), bare.fields.len()), (bare.other, 0, 0), "before the core sends the look file, every kind is bare");
    let mine = Look::of(&json!({ "family": { "": { "glyph": "tool", "role": "ok" }, "zz.": { "glyph": "rocket", "role": "ok" } }, "field": { "_secs": "duration", "x": "sparkle" } }));
    assert_eq!(mine.other, (j4n_tui::theme::glyph::Slot::Tool, Role::Ok), "the empty prefix is the look of a kind no family has");
    assert!(mine.families.is_empty() && mine.fields.len() == 1, "a name the theme does not know is left out");
    assert_eq!((mine.show("wait_secs"), mine.show("_secs")), (event::Show::Duration, event::Show::Text));
}

#[test]
fn the_log_is_a_receipt_a_string_keeps_every_character_unless_the_look_file_calls_it_prose() {
    let said = "glob *.rs matched nothing in src/* and `cargo test` failed with _no_ files";
    let e = ev(1, "run.error", Some("t1"), json!({ "error": said, "reason": "the **parser** ate `,` first and did not stop here", "hint": "see [the page](https://a.b/c) for more words" }));
    let mut a = app_with(vec![row_of(e)], 1, 0);
    hit(&mut a, KeyCode::Enter);
    let s = lines(&mut a, 100, 16).join("\n");
    assert!(s.contains(said), "an error string is shown as written, `*` and backticks kept:\n{s}");
    assert!(s.contains("see [the page](https://a.b/c) for more words"), "a field the look file does not call prose is not read:\n{s}");
    assert!(s.contains("the parser ate , first") && !s.contains("**parser**"), "`reason` is prose in the look file, so it is read:\n{s}");
    let l = look();
    assert_eq!([l.show("reason"), l.show("why"), l.show("summary"), l.show("text"), l.show("note"), l.show("error")], [event::Show::Prose, event::Show::Prose, event::Show::Prose, event::Show::Prose, event::Show::Prose, event::Show::Text]);
}

#[test]
fn theme_frame_reaches_the_tree_of_an_event() {
    let open = |frame: &str| {
        let mut a = shapes_app();
        a.theme = Theme::build(&Settings { frame: frame.into(), marks: "unicode".into(), ..Settings::default() }, &[]).expect("a theme");
        to(&mut a, 2);
        hit(&mut a, KeyCode::Right);
        hit(&mut a, KeyCode::Right);
        lines(&mut a, 80, 20)
    };
    let pipes = open("pipes");
    assert!(pipes.iter().any(|l| l.contains("├ suite")), "{pipes:#?}");
    assert!(pipes.iter().any(|l| l.contains("│ └ tests")), "{pipes:#?}");
    // airy: no line; what nests is an arrow, a row of the event itself a dot
    let airy = open("airy");
    assert!(!airy.iter().any(|l| l.contains(['├', '╰', '└', '│', '┆'])), "{airy:#?}");
    assert!(airy.iter().any(|l| l.contains("· suite")) && airy.iter().any(|l| l.contains("› tests")), "{airy:#?}");
    // hang: nothing at the top, a rail where a row goes on, an elbow at the end
    let hang = open("hang");
    assert!(hang.iter().any(|l| l.contains("╰ tests")) && !hang.iter().any(|l| l.contains('├')), "{hang:#?}");
}
