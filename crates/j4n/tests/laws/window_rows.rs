//! The windows the core answers are the rows the screen was built for. A Log of events goes into the core's index; the
//! screen asks for what it shows (`Up::Window`), is answered from the index, and draws; the same events written out by hand as rows,
//! cut by hand to the same asks, go to the same screen; the two screens are the same at every size. A page with many pages under it is
//! sent lazy, opened when the screen shows it, and is the same screen as the page sent whole.

use j4n_core::change::Actor;
use j4n_core::eval::Scope;
use j4n_core::front::page::Pages;
use j4n_core::front::window::{Ask, LogRows};
use j4n_core::log::Ev;
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use j4n_proto::{Cell, Down, Field, FieldKind, Intent, Json, Node, Present, Row, Tone, Up};
use j4n_tui::App;
use j4n_tui::headless::{Headless, KeyCode, press, typed};
use j4n_tui::theme::{Settings, Theme};
use serde_json::json;
use std::collections::BTreeMap;

const SIZES: [(u16, u16); 3] = [(120, 40), (80, 24), (60, 20)];

/// 2,500 events of five kinds and eleven tasks; one carries a long word, one nothing.
fn events() -> Vec<Ev> {
    (1..=2_500u64)
        .map(|i| {
            let kind = ["task.queued", "run.message", "gate.verdict", "run.tool_call", "task.settled"][(i % 5) as usize];
            let data = match i {
                7 => Json::Null,
                9 => json!({ "text": "long ".repeat(80) }),
                _ => json!({ "n": i, "word": format!("w{i}") }),
            };
            Ev { seq: i, at_ms: i * 1000, kind: kind.into(), task: (i % 11 != 0).then(|| format!("t{}", i % 11)), thread: None, by: Actor::Core, cause: None, class: Default::default(), data }
        })
        .collect()
}

/// A row of the Log as the screen shows it, written out from the event: the number, the kind and what it carried, the task.
fn by_hand(e: &Ev) -> Row {
    let one = if e.data.is_null() { String::new() } else { e.data.to_string().split_whitespace().collect::<Vec<_>>().join(" ") };
    let data = if one.chars().count() <= 200 { one } else { format!("{}…", one.chars().take(199).collect::<String>()) };
    Row {
        key: format!("e{}", e.seq),
        cells: vec![Cell::new(e.seq.to_string(), Tone::Dim), Cell::new(format!("{:<16} {data}", e.kind), Tone::Plain), Cell::new(e.task.clone().unwrap_or_default(), Tone::Dim)],
        ..Row::default()
    }
}

/// The rows the ask names, cut out of the events by hand: those that hold the word anywhere, in the order written or the reverse.
fn cut_by_hand(all: &[Ev], ask: &Ask) -> (Vec<Row>, u64) {
    let needle = ask.find.trim().to_lowercase();
    let mut hits: Vec<&Ev> = all.iter().filter(|e| needle.is_empty() || by_hand(e).cells.iter().any(|c| c.text.to_lowercase().contains(&needle)) || e.kind.contains(&needle)).collect();
    if ask.sort == "-seq" {
        hits.reverse();
    }
    let total = hits.len() as u64;
    let rows = hits.into_iter().skip(ask.from.min(total) as usize).take(ask.count as usize).map(by_hand).collect();
    (rows, total)
}

/// The screen's side: the app on a Log field, answered by `serve` for every window it asks, after it was sent the tree and the first
/// window, and then given the keys.
fn screen(total: u64, serve: &mut dyn FnMut(&Ask) -> (Vec<Row>, u64), keys: &[Vec<j4n_tui::headless::KeyEvent>], cols: u16, high: u16) -> String {
    let mut app = App::new();
    app.start();
    app.down(Down::Hello { api: j4n_proto::API, name: "j4n".into(), version: "0.0.0".into(), project: "demo".into() });
    let log = Node { id: "log".into(), field: Some(Field { kind: FieldKind::Log, bind: "log:all".into(), props: Json::Null }), ..Node::default() };
    app.down(Down::Tree { node: Node { id: "root".into(), present: Present::Stack, children: vec![log], ..Node::default() } });
    app.theme = Theme::build(&Settings { marks: "unicode".into(), ..Settings::default() }, &[]).expect("a theme");
    let mut answer = |app: &mut App, mut ups: Vec<Up>| {
        let mut rounds = 0;
        while !ups.is_empty() {
            rounds += 1;
            assert!(rounds < 10, "the screen asks and asks: {ups:?}");
            let mut more = Vec::new();
            for u in ups {
                if let Up::Window { bind, from, count, sort, find } = u {
                    let (rows, found) = serve(&Ask { from, count, sort, find });
                    more.extend(app.down(Down::Rows { bind, rows, total: found, from: from.min(found), rev: 1 }));
                }
            }
            ups = more;
        }
    };
    let ups = app.down(Down::Rows { bind: "log:all".into(), rows: Vec::new(), total, from: 0, rev: 1 });
    answer(&mut app, ups);
    // the screen knows how large it is once it has been drawn, and asks again for the rows that fill it
    let mut screen = Headless::new(cols, high);
    screen.draw(&mut app);
    let ups = app.windows();
    answer(&mut app, ups);
    for step in keys {
        for k in step.clone() {
            let ups = app.key(k);
            answer(&mut app, ups);
        }
        screen.draw(&mut app);
        let ups = app.windows();
        answer(&mut app, ups);
    }
    screen.show(&mut app)
}

/// What each side shows after the same keys, at every size: the end of the Log, the beginning, a page on, a find, a find cleared.
#[test]
fn a_log_the_core_windows_is_the_same_screen_as_the_rows_cut_by_hand_at_every_size() {
    let all = events();
    let mut index = LogRows::of(all.clone());
    let total = all.len() as u64;
    let steps: Vec<(&str, Vec<Vec<j4n_tui::headless::KeyEvent>>)> = vec![
        ("the end", vec![]),
        ("the beginning", vec![vec![press(KeyCode::Char('g'))]]),
        ("a page on", vec![vec![press(KeyCode::Char('g'))], vec![press(KeyCode::PageDown)], vec![press(KeyCode::PageDown)]]),
        ("a find", vec![vec![press(KeyCode::Char('/'))], typed("w1234", false), vec![press(KeyCode::Enter)]]),
        ("a find by a kind", vec![vec![press(KeyCode::Char('/'))], typed("GATE", false), vec![press(KeyCode::Enter)]]),
        ("a find cleared", vec![vec![press(KeyCode::Char('/'))], typed("w1234", false), vec![press(KeyCode::Enter)], vec![press(KeyCode::Esc)]]),
    ];
    for (name, keys) in &steps {
        for (cols, high) in SIZES {
            let core = screen(total, &mut |a| index.window(a), keys, cols, high);
            let hand = screen(total, &mut |a| cut_by_hand(&all, a), keys, cols, high);
            assert_eq!(core, hand, "{name}, at {cols}×{high}: the screen of the core's window is not the screen of the hand-cut one\n{core}\n----\n{hand}");
            assert!(keys.is_empty() || name.ends_with("cleared") || core.contains(" of "), "{name}: the place among all is on the screen: {core}");
            assert!(!core.contains('…') || name.contains("find"), "{name}: every row on the screen was sent: {core}");
        }
    }
    // the screens differ from one another: the checks above are of different pictures
    let (end, start) = (screen(total, &mut |a| index.window(a), &[], 120, 40), screen(total, &mut |a| index.window(a), &steps[1].1, 120, 40));
    assert!(end != start && start.contains("1 of 2,500"), "{end}\n----\n{start}");
    let found = screen(total, &mut |a| index.window(a), &steps[3].1, 120, 40);
    assert!(found.contains("w1234") && found.contains(" of 1"), "{found}");
}

/// A pack's page with `n` pages under it, on top of the pages of the base pack.
fn pages(n: usize) -> Pages {
    const FILES: [(&str, &str); 2] = [("pages/main.toml", include_str!("../../../../packs/base/pages/main.toml")), ("templates/main.toml", include_str!("../../../../packs/base/templates/main.toml"))];
    let mut shelf = String::from("api = 1\n[[page]]\nid = \"shelf\"\nparent = \"home\"\ntitle = \"Shelf\"\norder = 900\npresent = \"tabs\"\n");
    for i in 0..n {
        shelf.push_str(&format!("[[page]]\nid = \"s{i}\"\nparent = \"shelf\"\ntitle = \"Shelf {i}\"\norder = {i}\nfield = \"list\"\nbind = \"tasks:all\"\n"));
    }
    let base = FILES.iter().map(|(p, t)| SourceFile { path: (*p).into(), text: (*t).into() }).collect();
    let layers = vec![Layer::new(LayerKind::Pack, "base", base), Layer::new(LayerKind::Pack, "shelf", vec![SourceFile { path: "pages/shelf.toml".into(), text: shelf }])];
    Pages::load(&Tables::load(layers).0, &BTreeMap::new()).0
}

fn drawn(app: &mut App, cols: u16, high: u16) -> String {
    app.theme = Theme::build(&Settings { marks: "unicode".into(), ..Settings::default() }, &[]).expect("a theme");
    Headless::new(cols, high).show(app)
}

fn shelf_of(n: &Node) -> Option<&Node> {
    if n.id == "shelf" { Some(n) } else { n.children.iter().find_map(shelf_of) }
}

/// A screen sent the page whole, and one sent it lazy that then asks for its children once and is sent them, show the same.
#[test]
fn a_page_sent_lazy_and_opened_is_the_same_screen_as_the_page_sent_whole() {
    let pages = pages(450);
    let scope = Scope::new();
    let (whole, lazy) = (pages.tree("main", &scope), pages.tree_lazy("main", &scope, 400));
    let left_out = shelf_of(&lazy).expect("the page");
    assert_eq!((left_out.lazy, left_out.count, left_out.children.len()), (true, Some(450), 0));
    let opened = pages.subtree("shelf", "main", &scope, 400).expect("its children");
    // by hand: the page with its pages, as a pack that wrote them would have them
    let by_hand = Node {
        id: "shelf".into(),
        title: "Shelf".into(),
        present: Present::Tabs,
        children: (0..450)
            .map(|i| Node { id: format!("s{i}"), title: format!("Shelf {i}"), field: Some(Field { kind: FieldKind::List, bind: "tasks:all".into(), props: Json::Null }), ..Node::default() })
            .collect(),
        ..Node::default()
    };
    assert_eq!(opened.children, by_hand.children, "the core made the pages the pack wrote");
    let to_shelf = |app: &mut App| {
        // the jump goes to the tab
        let mut ups = Vec::new();
        ups.extend(app.key(j4n_tui::headless::ctrl('k')));
        typed("Shelf", false).into_iter().for_each(|k| ups.extend(app.key(k)));
        ups.extend(app.key(press(KeyCode::Enter)));
        ups
    };
    for (cols, high) in SIZES {
        let hello = Down::Hello { api: j4n_proto::API, name: "j4n".into(), version: "0.0.0".into(), project: "demo".into() };
        let mut a = App::new();
        a.start();
        a.down(hello.clone());
        a.down(Down::Tree { node: whole.clone() });
        to_shelf(&mut a);
        let mut b = App::new();
        b.start();
        b.down(hello);
        let ups = b.down(Down::Tree { node: lazy.clone() });
        assert!(!ups.iter().any(|u| matches!(u, Up::Intent { intent: Intent::Open { node } } if node == "shelf")), "a page that is not shown is not asked for: {ups:?}");
        let ups = to_shelf(&mut b);
        let asked: Vec<_> = ups.iter().filter(|u| matches!(u, Up::Intent { intent: Intent::Open { node } } if node == "shelf")).collect();
        assert_eq!(asked.len(), 1, "asked for once when it is shown: {ups:?}");
        let ups = b.down(Down::Tree { node: opened.clone() });
        assert!(!ups.iter().any(|u| matches!(u, Up::Intent { intent: Intent::Open { .. } })), "and not again once it has them: {ups:?}");
        let (x, y) = (drawn(&mut a, cols, high), drawn(&mut b, cols, high));
        assert_eq!(x, y, "at {cols}×{high} the lazy page is not the whole page\n{x}\n----\n{y}");
        assert!(x.contains("Shelf"), "{x}");
    }
}

// ---- a line seen again is one row with `×n`, and `c` finds the row of a run that holds the cause ----

/// `groups` runs of `each` events that are alike, then one event, caused by the event `cause`.
fn runs_then_one(groups: u64, each: u64, cause: u64) -> Vec<Ev> {
    let alike = (1..=groups * each).map(|i| Ev { seq: i, at_ms: 1_700_000_000_000 + i * 1000, kind: "run.tool_call".into(), task: Some("t1".into()), thread: None, by: Actor::Core, cause: None, class: Default::default(), data: json!({ "tool": "Read", "path": format!("f{}.rs", (i - 1) / each) }) });
    let last = Ev { seq: groups * each + 1, at_ms: 1_700_000_000_000 + groups * each * 1000 + 1000, kind: "task.settled".into(), task: Some("t1".into()), thread: None, by: Actor::Core, cause: Some(cause), class: Default::default(), data: json!({ "state": "done" }) };
    alike.chain([last]).collect()
}

/// The screen of a Log the core folds and fills, after the keys.
fn folded_screen(index: &mut LogRows, keys: &[Vec<j4n_tui::headless::KeyEvent>], cols: u16, high: u16) -> String {
    let total = index.rows();
    screen(total, &mut |a| {
        let (mut rows, found) = index.window(a);
        index.fill(None, &mut rows);
        (rows, found)
    }, keys, cols, high)
}

#[test]
fn a_line_seen_again_is_one_line_with_its_count_on_the_screen_and_opens_into_the_first_and_last_time() {
    let mut index = LogRows::of(runs_then_one(40, 3, 5));
    assert_eq!(index.rows(), 41);
    for (cols, high) in SIZES {
        let end = folded_screen(&mut index, &[], cols, high);
        assert!(end.contains("×3") && end.contains("f39.rs"), "{cols}×{high}: {end}");
        // the third event of the run in the middle is the cause of the last event: `c` goes to the row of its run, and it is open
        let keys = vec![vec![press(KeyCode::Char('c'))], vec![press(KeyCode::Enter)]];
        let at = folded_screen(&mut index, &keys, cols, high);
        let line = at.lines().find(|l| l.starts_with('┃') && l.contains("f1.rs")).unwrap_or_else(|| panic!("{cols}×{high}: the keys are not on the run that holds the cause (event 5)\n{at}"));
        assert!(line.contains("×3"), "{line}");
        if high >= 20 {
            assert!(at.contains("repeats") && at.contains("last_seq") && at.contains("last_at_ms"), "{cols}×{high}: the opened run says how many and where it ends\n{at}");
        }
    }
}

#[test]
fn c_lands_on_the_run_that_holds_the_cause_however_far_the_log_is_folded() {
    // 4,000 runs of five: the cause is far from the end, in the middle of a run, and the rows are fewer than the events a guess counts
    let mut index = LogRows::of(runs_then_one(4_000, 5, 1_203));
    assert_eq!(index.rows(), 4_001);
    let at = folded_screen(&mut index, &[vec![press(KeyCode::Char('c'))]], 120, 40);
    // event 1,203 is in the run of events 1,201 to 1,205: the 241st, whose path is f240.rs
    assert!(at.lines().any(|l| l.starts_with('┃') && l.contains("f240.rs") && l.contains("×5")), "{at}");
}

#[test]
fn c_from_a_find_goes_to_the_run_that_holds_the_cause_among_all_the_events() {
    let mut index = LogRows::of(runs_then_one(4_000, 5, 1_203));
    // the find shows the one event that was caused; `c` leaves the find and goes to the run of the event that caused it
    let keys = vec![vec![press(KeyCode::Char('/'))], typed("settled", false), vec![press(KeyCode::Enter)], vec![press(KeyCode::Char('c'))]];
    let at = folded_screen(&mut index, &keys, 120, 40);
    assert!(at.lines().any(|l| l.starts_with('┃') && l.contains("f240.rs") && l.contains("×5")), "{at}");
    assert!(!at.contains("/ settled"), "the find is cleared: {at}");
}

#[test]
fn c_from_a_find_goes_to_the_cause_among_all_the_events_when_no_line_is_seen_again() {
    // every event is its own line: nothing folds, and the rows held under the find are not the rows of the Log in the order written
    let mut index = LogRows::of(runs_then_one(900, 1, 300));
    assert_eq!(index.rows(), 901);
    let keys = vec![vec![press(KeyCode::Char('/'))], typed("settled", false), vec![press(KeyCode::Enter)], vec![press(KeyCode::Char('c'))]];
    let at = folded_screen(&mut index, &keys, 120, 40);
    assert!(at.lines().any(|l| l.starts_with('┃') && l.contains("f299.rs")), "{at}");
}
