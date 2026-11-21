//! The speed of the screen: the frame, the key and the first frame, headless, in a release build, each held to a
//! ceiling in `fixtures/tui-bench.toml`. The ceiling is a ratchet: it only goes down. `-- --ratchet` lowers each ceiling to three times
//! what is measured now (where that is lower); a ceiling is raised by hand, in a step that says why.
//! Run: `scripts/t bench -p j4n --bench tui_speed`. Each number is the best of several rounds. A machine that other builds share is slower
//! for the whole run, so a ceiling is held as a quiet machine would take it: it is stretched by how much slower than a quiet machine a fixed
//! piece of work is, timed in this process just before and just after the benches.

use j4n_core::front::window::{Ask, LogRows};
use j4n_core::log::Ev;
use j4n_proto::{Cell, Down, Field, FieldKind, Json, Node, Present, Row, Tone, Up};
use j4n_tui::App;
use j4n_tui::headless::{Headless, KeyCode, press, ctrl, typed};
use std::collections::BTreeMap;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// What the screen is held to, in microseconds; a ceiling above it is refused. `None` where it asks for "no visible delay".
const SPEC_US: [(&str, u64); 14] = [
    ("first_frame_main", 100_000),
    ("frame_main_160x48", 1_000),
    ("key_main_160x48", 16_000),
    ("frame_idle_main", 1_000),
    ("nodes_1m_open", 100_000),
    ("nodes_1m_key", 16_000),
    ("nodes_1m_find", 50_000),
    ("log_1m_first", 100_000),
    ("log_1m_key", 16_000),
    ("log_1m_window", 16_000),
    ("log_1m_find", 100_000),
    ("log_1m_sort", 250_000),
    ("flood_frame", 50_000),
    ("resize_main", 16_000),
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn hello() -> Down {
    Down::Hello { api: j4n_proto::API, name: "j4n".into(), version: "0.0.0".into(), project: "demo".into() }
}

/// The best of `rounds` rounds of `ops` runs of `f`, per run.
fn best(rounds: u32, ops: u32, mut f: impl FnMut()) -> Duration {
    (0..rounds)
        .map(|_| {
            let t = Instant::now();
            for _ in 0..ops {
                f();
            }
            t.elapsed() / ops
        })
        .min()
        .unwrap_or_default()
}

/// What the reference below takes on a quiet machine in a release build, in microseconds (the best of many goes, on a machine that was not busy).
const QUIET_REFERENCE_US: u64 = 60_000;

/// A fixed piece of work of the kind the benches do (small allocations, text made and sorted, a walk over it), big enough that it does
/// not fit in a cache: what slows a walk over a million titles on a busy machine is the memory that other work is using too.
fn reference() -> u64 {
    let mut v: Vec<String> = (0..400_000u32).map(|n| format!("item-{}", n.wrapping_mul(2_654_435_761) % 2_000_003)).collect();
    v.sort();
    v.iter().fold(0u64, |h, s| h.wrapping_mul(31).wrapping_add(s.len() as u64 + u64::from(s.as_bytes()[5])))
}

/// How many times slower than a quiet machine this one is now, never below one: the reference, timed the way the benches are, against
/// what it takes on a quiet one.
fn slowness() -> f64 {
    let took = best(9, 1, || {
        black_box(reference());
    });
    (took.as_micros() as f64 / QUIET_REFERENCE_US as f64).max(1.0)
}

/// The steps of the scene `main` that the screen is given (messages, keys, text); no pictures.
fn main_scene() -> Vec<toml::Table> {
    let text = std::fs::read_to_string(root().join("crates/j4n-tui/tests/scenes/main.toml")).expect("the scene main");
    let t: toml::Table = text.parse().expect("the scene is TOML");
    t["step"].as_array().expect("steps").iter().filter_map(|s| s.as_table().cloned()).collect()
}

fn play(app: &mut App, steps: &[toml::Table]) {
    app.start();
    let start = Instant::now();
    app.tick(start);
    for s in steps {
        if s.contains_key("down") {
            app.down(toml::Value::Table(s.clone()).try_into::<Down>().expect("a message"));
        } else if let Some(k) = s.get("key").and_then(toml::Value::as_str) {
            app.key(j4n_tui::scene::key(k).expect("a key"));
        } else if let Some(t) = s.get("text").and_then(toml::Value::as_str) {
            typed(t, false).into_iter().for_each(|k| drop(app.key(k)));
        }
    }
}

fn node(id: &str, title: &str, present: Present, children: Vec<Node>) -> Node {
    Node { id: id.into(), title: title.into(), present, children, ..Node::default() }
}

/// 10 tabs × 10 sub-tabs × 100 sections × 100 fields: a million nodes.
fn million() -> Node {
    let tab = |t: usize| {
        let subs = (0..10).map(|s| {
            let sections = (0..100).map(|c| {
                let fields = (0..100).map(|f| node(&format!("f{t}.{s}.{c}.{f}"), &format!("field {f}"), Present::default(), Vec::new())).collect();
                node(&format!("c{t}.{s}.{c}"), &format!("section {c}"), Present::Stack, fields)
            });
            node(&format!("s{t}.{s}"), &format!("sub {s}"), Present::Sections, sections.collect())
        });
        node(&format!("t{t}"), &format!("tab {t}"), Present::Subtabs, subs.collect())
    };
    node("root", "", Present::Tabs, (0..10).map(tab).collect())
}

fn row(i: u64) -> Row {
    let cell = |t: String| Cell::new(t, Tone::Plain);
    Row { key: format!("e{i}"), cells: vec![cell(format!("12:{:02}:{:02}", (i / 60) % 60, i % 60)), cell(format!("event number {i}"))], ..Row::default() }
}

/// An event of the Log: kinds and tasks that come round, and what each carried.
fn event(i: u64) -> Ev {
    let kind = ["task.settled", "run.message", "gate.verdict", "run.tool_call", "chat.note"][(i % 5) as usize];
    Ev { seq: i, at_ms: i, kind: kind.into(), task: Some(format!("t{}", i % 97)), thread: None, by: j4n_core::change::Actor::Core, cause: None, class: Default::default(), data: j4n_proto::json!({ "n": i, "word": format!("w{i}") }) }
}

/// The core of a log of a million events, as the front holds it: each window the screen asks for is answered from the index.
fn serve(a: &mut App, ups: Vec<Up>, log: &mut LogRows) {
    for u in ups {
        if let Up::Window { bind, from, count, sort, find } = u {
            let (rows, total) = log.window(&Ask { from, count, sort, find });
            a.down(Down::Rows { bind, rows, total, from: from.min(total), rev: 1 });
        }
    }
}

fn measure() -> BTreeMap<&'static str, u64> {
    let mut got = BTreeMap::new();
    let mut put = |name: &'static str, d: Duration| {
        got.insert(name, d.as_micros().max(1) as u64);
    };
    let steps = main_scene();

    // the first frame: the hello and the tree, then the frame; the rest of the scene streams in after it
    let first = steps.iter().take(2).cloned().collect::<Vec<_>>();
    put("first_frame_main", best(5, 20, || {
        let mut a = App::new();
        let mut screen = Headless::new(160, 48);
        play(&mut a, &first);
        screen.draw(&mut a);
        black_box(&a);
    }));

    // the Main scene at 160×48: a frame of a screen nothing happens to, a frame after a key, a frame after a resize
    let mut a = App::new();
    play(&mut a, &steps);
    let mut screen = Headless::new(160, 48);
    screen.draw(&mut a);
    put("frame_main_160x48", best(7, 200, || screen.draw(&mut a)));
    put("frame_idle_main", best(7, 200, || {
        // the clock ticks and nothing is in motion: the loop asks whether to draw, and does not
        black_box(a.tick(Instant::now()));
        black_box(a.take_redraw());
    }));
    let mut flip = false;
    put("key_main_160x48", best(7, 100, || {
        flip = !flip;
        a.key(ctrl(if flip { 'u' } else { 't' }));
        a.key(press(if flip { KeyCode::Up } else { KeyCode::Down }));
        screen.draw(&mut a);
    }) / 2);
    let mut wide = false;
    put("resize_main", best(5, 50, || {
        wide = !wide;
        let (c, r) = if wide { (200, 60) } else { (160, 48) };
        screen = Headless::new(c, r);
        a.down(Down::Tree { node: node("x", "", Present::Stack, Vec::new()) }).clear();
        screen.draw(&mut a);
    }));

    // a million nodes
    // (the tree is made by the core, so making it is not timed; taking it and drawing it is)
    let tree = million();
    let mut n = App::new();
    n.start();
    n.down(hello());
    let t = Instant::now();
    n.down(Down::Tree { node: tree });
    let mut screen = Headless::new(160, 48);
    screen.draw(&mut n);
    put("nodes_1m_open", t.elapsed());
    let mut moves = 0;
    put("nodes_1m_key", best(5, 40, || {
        moves += 1;
        n.key(press(if moves % 2 == 0 { KeyCode::Down } else { KeyCode::Char('}') }));
        screen.draw(&mut n);
    }));
    // the jump over a million titles: the slowest of five letters, each drawn
    n.key(ctrl('k'));
    let slowest = typed("sec 9", false)
        .into_iter()
        .map(|k| {
            let t = Instant::now();
            n.key(k);
            screen.draw(&mut n);
            t.elapsed()
        })
        .max()
        .unwrap_or_default();
    put("nodes_1m_find", slowest);

    // a log of a million events, held by the core as the front holds it (making the index is not timed: it is read once, over ticks)
    let total = 1_000_000u64;
    let mut index = LogRows::of((1..=total).map(event));
    let log = || Node { id: "l".into(), field: Some(Field { kind: FieldKind::Log, bind: "log:all".into(), props: Json::Null }), ..Node::default() };
    let open_log = |index: &mut LogRows| {
        let mut l = App::new();
        l.start();
        l.down(hello());
        l.down(Down::Tree { node: node("root", "", Present::Stack, vec![log()]) });
        let mut screen = Headless::new(160, 48);
        let ups = l.down(Down::Rows { bind: "log:all".into(), rows: Vec::new(), total, from: 0, rev: 1 });
        serve(&mut l, ups, index);
        screen.draw(&mut l);
        let ups = l.windows();
        serve(&mut l, ups, index);
        screen.draw(&mut l);
        (l, screen)
    };
    put("log_1m_first", best(5, 1, || drop(black_box(open_log(&mut index)))));
    let (mut l, mut screen) = open_log(&mut index);
    let mut at = 0;
    put("log_1m_key", best(5, 100, || {
        at += 1;
        let ups = l.key(press(if at % 4 == 0 { KeyCode::PageDown } else { KeyCode::Up }));
        serve(&mut l, ups, &mut index);
        screen.draw(&mut l);
    }));
    // what the core makes of one window of the million, in the order they were written
    let mut place = 0;
    put("log_1m_window", best(5, 100, || {
        place = (place + 7_919) % (total - 100);
        black_box(index.window(&Ask { from: place, count: 100, ..Ask::default() }));
    }));
    // a word looked for in a million events, typed to the find of the log and sent with ⏎ (the time of the ⏎ alone): each of three
    // rounds looks for a word it has not looked for
    let found = (1..=3)
        .map(|word| {
            l.key(press(KeyCode::Char('/')));
            typed(&format!("w99999{word}"), false).into_iter().for_each(|k| drop(l.key(k)));
            let t = Instant::now();
            let ups = l.key(press(KeyCode::Enter));
            serve(&mut l, ups, &mut index);
            screen.draw(&mut l);
            let took = t.elapsed();
            let ups = l.key(press(KeyCode::Esc));
            serve(&mut l, ups, &mut index);
            took
        })
        .min()
        .unwrap_or_default();
    put("log_1m_find", found);
    // the million put in the order of a kind, and of a task: a round sorts by the one the last did not
    let mut by = false;
    put("log_1m_sort", best(3, 1, || {
        by = !by;
        black_box(index.window(&Ask { from: 0, count: 100, sort: (if by { "kind" } else { "task" }).into(), find: String::new() }));
    }));

    // a core that sends faster than the screen draws: the rows of a log, one message each, then one frame
    let mut f = App::new();
    f.start();
    f.down(hello());
    f.down(Down::Tree { node: node("root", "", Present::Stack, vec![log()]) });
    let mut screen = Headless::new(160, 48);
    let mut rev = 0;
    put("flood_frame", best(5, 20, || {
        for i in 0..200 {
            rev += 1;
            f.down(Down::Rows { bind: "log:all".into(), rows: vec![row(rev * 1000 + i)], total: 1_000_000, from: 999_999, rev });
        }
        screen.draw(&mut f);
    }));
    got
}

fn ceilings(path: &PathBuf) -> BTreeMap<String, u64> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let t: toml::Table = text.parse().unwrap_or_default();
    t.get("ceiling_us").and_then(toml::Value::as_table).map(|c| c.iter().filter_map(|(k, v)| v.as_integer().map(|n| (k.clone(), n as u64))).collect()).unwrap_or_default()
}

fn main() {
    let path = root().join("fixtures/tui-bench.toml");
    let before = slowness();
    let got = measure();
    let slow = before.max(slowness());
    println!("bench: this machine is {slow:.2} times slower than a quiet one (the reference, before and after); each ceiling is stretched by it");
    let have = ceilings(&path);
    let ratchet = std::env::args().any(|a| a == "--ratchet");
    let mut next = BTreeMap::new();
    let mut failed = Vec::new();
    for (name, spec) in SPEC_US {
        let us = got[name];
        let ceiling = have.get(name).copied().unwrap_or(spec);
        // a ceiling is three times what was measured, never above the spec, never lower than 50 µs (a clock reads that much)
        next.insert(name, ceiling.min(((us as f64 / slow) as u64 * 3).max(50)).min(spec));
        let allowed = (ceiling as f64 * slow) as u64;
        println!("bench {name:<20} {us:>9} µs   ceiling {ceiling:>9} µs   spec {spec:>9} µs");
        if ceiling > spec {
            failed.push(format!("{name}: the ceiling {ceiling} µs is above the spec {spec} µs"));
        }
        if us > allowed {
            failed.push(format!("{name}: {us} µs is above its ceiling of {ceiling} µs ({allowed} µs on a machine {slow:.2} times slower than a quiet one)"));
        }
    }
    if ratchet {
        let body: String = next.iter().map(|(k, v)| format!("{k} = {v}\n")).collect();
        let head = "# The speed ratchet of the screen (crates/j4n/benches/tui_speed.rs): the most a bench may take, in microseconds.\n# A ceiling only goes down (scripts/t bench -p j4n --bench tui_speed -- --ratchet); raising one is done by hand, with the reason in the commit.\n[ceiling_us]\n";
        std::fs::write(&path, format!("{head}{body}")).expect("write the ceilings");
        println!("bench: ceilings written to fixtures/tui-bench.toml");
    }
    if !failed.is_empty() {
        eprintln!("bench: too slow:\n  {}", failed.join("\n  "));
        std::process::exit(1);
    }
}
