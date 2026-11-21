//! The core answers `Up::Window` for the rows it sends — any rows, cut by a sort and a find; the Log as an index that is
//! windowed, found and sorted without being read again — and a page with many nodes under it is sent without them until it is shown.

use crate::front_cases::{Fx, fx};
use j4n_core::change::Actor;
use j4n_core::front::page::Pages;
use j4n_core::front::screen::Screen;
use j4n_core::front::serve::{LAZY, Serve};
use j4n_core::front::window::{Ask, FIRST, LogRows, MOST, cut, has};
use j4n_core::log::Ev;
use j4n_core::setting::Settings;
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use j4n_proto::{Cell, Down, Intent, Node, Row, Tone, Up};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;

fn ask(from: u64, count: u64, sort: &str, find: &str) -> Ask {
    Ask { from, count, sort: sort.into(), find: find.into() }
}

fn row(key: &str, cells: &[&str]) -> Row {
    Row { key: key.into(), cells: cells.iter().map(|c| Cell::new(*c, Tone::Plain)).collect(), ..Row::default() }
}

fn keys(rows: &[Row]) -> Vec<&str> {
    rows.iter().map(|r| r.key.as_str()).collect()
}

fn cols(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| n.to_string()).collect()
}

// ---- any rows ----

fn money() -> Vec<Row> {
    vec![row("a", &["alpha", "$9.50"]), row("b", &["Beta", "$10.25"]), row("c", &["gamma", "$0.75"]), row("d", &["delta", "$10.25"])]
}

#[test]
fn rows_are_put_in_the_order_of_a_column_as_numbers_when_both_cells_are_and_the_other_way_with_a_minus() {
    let c = cols(&["name", "cost"]);
    let (rows, total) = cut(&money(), &c, &ask(0, 10, "cost", ""));
    assert_eq!((keys(&rows), total), (vec!["c", "a", "b", "d"], 4), "$10.25 is more than $9.50, and rows that are equal keep their order");
    assert_eq!(keys(&cut(&money(), &c, &ask(0, 10, "-cost", "")).0), ["b", "d", "a", "c"]);
    assert_eq!(keys(&cut(&money(), &c, &ask(0, 10, "name", "")).0), ["a", "b", "d", "c"], "words are put in order whatever their case");
    assert_eq!(keys(&cut(&money(), &c, &ask(0, 10, "nothing", "")).0), ["a", "b", "c", "d"], "a column nobody has leaves the order");
}

#[test]
fn a_word_is_looked_for_in_every_cell_in_whatever_case_and_the_rows_that_hold_it_are_counted() {
    let (rows, total) = cut(&money(), &cols(&["name", "cost"]), &ask(0, 10, "", "BETA"));
    assert_eq!((keys(&rows), total), (vec!["b"], 1));
    let (rows, total) = cut(&money(), &[], &ask(0, 10, "", " 10.25 "));
    assert_eq!((keys(&rows), total), (vec!["b", "d"], 2), "the word is trimmed");
    assert_eq!(cut(&money(), &[], &ask(0, 10, "", "zzz")), (Vec::new(), 0));
}

#[test]
fn a_window_starts_where_asked_holds_no_more_than_asked_and_is_never_larger_than_the_most() {
    let many: Vec<Row> = (0..MOST + 500).map(|i| row(&format!("r{i}"), &[&i.to_string()])).collect();
    let (rows, total) = cut(&many, &[], &ask(10, 3, "", ""));
    assert_eq!((keys(&rows), total), (vec!["r10", "r11", "r12"], MOST + 500));
    assert_eq!(cut(&many, &[], &ask(0, 1_000_000, "", "")).0.len() as u64, MOST, "no more than the most");
    let (rows, total) = cut(&many, &[], &ask(5_000, 10, "", ""));
    assert!(rows.is_empty() && total == MOST + 500, "past the end there is nothing, and the count is still told");
}

#[test]
fn a_word_is_found_whatever_the_case_of_its_letters() {
    assert!(has("Task.Settled", "settled") && has("abc", "") && has("abc", "c") && has("ABC", "abc"));
    assert!(!has("ab", "abc") && !has("abc", "bd") && !has("", "a"));
}

// ---- the Log as rows ----

fn event(i: u64) -> Ev {
    let kind = ["task.settled", "run.message", "gate.verdict"][(i % 3) as usize];
    Ev { seq: i, at_ms: i, kind: kind.into(), task: Some(format!("t{}", i % 7)), thread: None, by: Actor::Core, cause: None, class: Default::default(), data: json!({ "n": i, "word": format!("w{i}") }) }
}

fn log_of(n: u64) -> LogRows {
    LogRows::of((1..=n).map(event))
}

fn seqs(rows: &[Row]) -> Vec<u64> {
    rows.iter().map(|r| r.cells[0].text.parse().unwrap()).collect()
}

#[test]
fn the_log_is_windowed_in_the_order_it_was_written_or_the_other_way() {
    let mut log = log_of(1_000);
    let (rows, total) = log.window(&ask(0, 3, "", ""));
    assert_eq!((seqs(&rows), total), (vec![1, 2, 3], 1_000));
    assert_eq!(rows[0].key, "e1");
    assert!(rows[0].cells[1].text.starts_with("run.message") && rows[0].cells[1].text.contains("\"word\":\"w1\""), "{:?}", rows[0]);
    assert_eq!(rows[0].cells[2].text, "t1");
    assert_eq!(seqs(&log.window(&ask(0, 3, "-seq", "")).0), [1_000, 999, 998]);
    assert_eq!(seqs(&log.window(&ask(997, 10, "", "")).0), [998, 999, 1_000], "a window at the end holds what is left");
    let (rows, total) = log.window(&ask(5_000, 10, "", ""));
    assert!(rows.is_empty() && total == 1_000);
}

#[test]
fn a_word_is_found_in_the_kind_the_task_the_data_and_the_number_of_an_event() {
    let mut log = log_of(1_000);
    let (rows, total) = log.window(&ask(0, 4, "", "SETTLED"));
    assert_eq!((seqs(&rows), total), (vec![3, 6, 9, 12], 333), "a kind");
    let (rows, total) = log.window(&ask(0, 3, "", "t3"));
    assert_eq!((seqs(&rows), total), (vec![3, 10, 17], 143), "a task");
    let (rows, total) = log.window(&ask(0, 10, "", "w999"));
    assert_eq!((seqs(&rows), total), (vec![999], 1), "what the event carried");
    assert_eq!(log.window(&ask(0, 10, "", "nothing like it")), (Vec::new(), 0));
    let (rows, total) = log.window(&ask(0, 3, "-seq", "w9"));
    assert_eq!((seqs(&rows), total), (vec![999, 998, 997], 111), "found, then the other way");
}

#[test]
fn the_log_is_put_in_the_order_of_a_kind_or_a_task_and_what_is_equal_keeps_the_order_it_was_written_in() {
    let mut log = log_of(30);
    let (rows, total) = log.window(&ask(0, 5, "kind", ""));
    assert_eq!((seqs(&rows), total), (vec![2, 5, 8, 11, 14], 30), "gate.verdict comes first: it is every third event");
    assert_eq!(seqs(&log.window(&ask(0, 3, "-kind", "")).0), [30, 27, 24], "the last kind is task.settled, the newest first");
    assert_eq!(seqs(&log.window(&ask(0, 6, "task", "")).0), [7, 14, 21, 28, 1, 8], "t0 is every seventh, then t1");
    assert_eq!(seqs(&log.window(&ask(0, 3, "kind", "t1")).0), [8, 29, 1], "a find and a sort together");
}

#[test]
fn an_event_that_is_written_is_held_at_once_and_a_find_that_was_made_looks_only_at_what_is_new() {
    let mut log = log_of(10);
    assert_eq!(log.window(&ask(0, 10, "", "w11")).1, 0);
    log.push(&event(11));
    assert!(log.built() && log.len() == 11);
    assert_eq!(seqs(&log.window(&ask(0, 10, "", "w11")).0), [11], "the find of before sees the new event");
    log.push(&event(13));
    assert!(!log.built() && log.len() == 11, "an event that does not follow the last is left to be read: {}", log.len());
    let flash = Ev { seq: 0, ..event(1) };
    log.push(&flash);
    assert_eq!(log.len(), 11, "a flash is not an event");
}

// ---- the Log from the files ----

#[test]
fn the_log_is_read_from_the_files_a_part_at_a_time_and_is_built_when_there_is_no_more() {
    let f = fx(vec![], |_| {});
    for i in 0..2_500 {
        f.front.note("main", &format!("note {i}"), "dim");
    }
    let (log, head) = (&f.rig.log, f.rig.log.head());
    let mut rows = LogRows::new();
    rows.hear(head);
    let mut calls = 0;
    while !rows.built() {
        rows.catch_up(log, 1_000);
        calls += 1;
        assert!(calls < 20, "it never ends");
        assert!(rows.len() <= 1_000 * calls);
    }
    assert!(calls >= 3 && rows.len() == head, "{calls} calls for {head} events: {}", rows.len());
    let (last, _) = rows.window(&ask(head - 1, 1, "", ""));
    assert_eq!(last[0].key, format!("e{head}"));
    // an event that was written and never pushed is read the next time
    f.front.note("main", "one more", "dim");
    rows.hear(log.head());
    assert!(!rows.built());
    rows.catch_up(log, 1_000);
    assert!(rows.built() && rows.len() == head + 1);
}

// ---- what a client is sent ----

fn serve(f: &Fx) -> Serve {
    Serve::new(f.front.clone())
}

fn watch(s: &mut Serve, bind: &str) -> Vec<Down> {
    s.up(Up::Watch { bind: bind.into() }).0
}

fn window(s: &mut Serve, bind: &str, from: u64, count: u64, sort: &str, find: &str) -> Vec<Down> {
    s.up(Up::Window { bind: bind.into(), from, count, sort: sort.into(), find: find.into() }).0
}

/// The last rows sent for `bind`, with the place of the first and how many there are in all.
fn last_rows(downs: &[Down], bind: &str) -> Option<(Vec<Row>, u64, u64)> {
    downs.iter().rev().find_map(|d| match d {
        Down::Rows { bind: b, rows, total, from, .. } if b == bind => Some((rows.clone(), *from, *total)),
        _ => None,
    })
}

#[test]
fn a_window_of_rows_that_are_watched_is_answered_with_the_rows_the_place_of_the_first_and_how_many_there_are() {
    let f = fx(vec![], |_| {});
    let mut s = serve(&f);
    assert_eq!(last_rows(&watch(&mut s, "form:thread"), "form:thread").map(|r| (r.0.len(), r.1, r.2)), Some((4, 0, 4)), "until it asks, all of them");
    let (rows, from, total) = last_rows(&window(&mut s, "form:thread", 1, 2, "", ""), "form:thread").expect("an answer");
    assert_eq!((keys(&rows), from, total), (vec!["model", "how"], 1, 4));
    let (rows, from, total) = last_rows(&window(&mut s, "form:thread", 0, 10, "", "HOW"), "form:thread").expect("an answer");
    assert_eq!((keys(&rows), from, total), (vec!["how"], 0, 1), "the rows that hold the word, and how many");
}

#[test]
fn a_window_of_a_binding_that_is_not_watched_is_not_answered() {
    let f = fx(vec![], |_| {});
    let mut s = serve(&f);
    assert!(window(&mut s, "form:thread", 0, 2, "", "").is_empty());
}

#[test]
fn the_window_asked_for_is_kept_when_the_rows_are_sent_again_and_forgotten_when_they_are_no_longer_watched() {
    let f = fx(vec![], |_| {});
    let mut s = serve(&f);
    watch(&mut s, "form:thread");
    window(&mut s, "form:thread", 1, 2, "", "");
    let (rows, from, total) = last_rows(&s.resync(), "form:thread").expect("sent again");
    assert_eq!((keys(&rows), from, total), (vec!["model", "how"], 1, 4), "the same window");
    s.up(Up::Unwatch { bind: "form:thread".into() });
    let (rows, from, total) = last_rows(&watch(&mut s, "form:thread"), "form:thread").expect("sent");
    assert_eq!((rows.len(), from, total), (4, 0, 4), "it asks again");
}

fn notes(f: &Fx, n: usize) {
    for i in 0..n {
        f.front.note("main", &format!("note {i}"), "dim");
    }
}

#[test]
fn the_window_of_the_log_a_client_asks_for_holds_what_it_asks_and_finds_what_it_asks_to_find() {
    let f = fx(vec![], |_| {});
    notes(&f, 620);
    let mut s = serve(&f);
    watch(&mut s, "log:all");
    let (rows, from, total) = last_rows(&window(&mut s, "log:all", 10, 5, "", ""), "log:all").expect("an answer");
    assert_eq!((rows.len(), from, total >= 620), (5, 10, true));
    assert_eq!(rows[0].key, format!("e{}", 11), "the tenth after the first");
    let (rows, from, total) = last_rows(&window(&mut s, "log:all", 0, 50, "", "note 61"), "log:all").expect("an answer");
    assert_eq!((rows.len(), from, total), (11, 0, 11), "note 61 and note 610 to note 619");
    let (rows, _, _) = last_rows(&window(&mut s, "log:all", 0, 2, "-seq", "note 6"), "log:all").expect("an answer");
    assert!(rows[0].cells[1].text.contains("note 619") && rows[1].cells[1].text.contains("note 618"), "{rows:?}");
    let (rows, _, total) = last_rows(&window(&mut s, "log:all", 0, 2, "kind", "note 6"), "log:all").expect("an answer");
    assert!(rows.iter().all(|r| r.cells[1].text.starts_with("chat.note")) && total == 31, "note 6, note 60 to 69 and note 600 to 619: {total} {rows:?}");
}

fn written(f: &Fx, s: &mut Serve, text: &str) -> Vec<Down> {
    let before = f.rig.log.head();
    f.front.note("main", text, "dim");
    let mut downs: Vec<Down> = f.rig.log.read(before + 1).flat_map(|e| s.ev(&e)).collect();
    downs.extend(s.tick());
    downs
}

#[test]
fn a_window_that_holds_the_end_of_the_log_holds_it_still_as_events_are_written_and_one_in_the_middle_stays() {
    let f = fx(vec![], |_| {});
    notes(&f, 300);
    let mut s = serve(&f);
    let total = last_rows(&watch(&mut s, "log:all"), "log:all").expect("sent").2;
    window(&mut s, "log:all", total - 50, 50, "", "");
    let (rows, from, now) = last_rows(&written(&f, &mut s, "fresh"), "log:all").expect("the window moved");
    assert!(rows[rows.len() - 1].cells[1].text.contains("fresh"), "{:?}", rows.last());
    assert_eq!((rows.len(), from + 50, now), (50, now, total + 1), "the same size, one on");
    window(&mut s, "log:all", 10, 20, "", "");
    let (rows, from, now) = last_rows(&written(&f, &mut s, "newer"), "log:all").expect("the total is told");
    assert_eq!((rows.len(), from, now), (20, 10, total + 2), "the window stays and the total grows");
    let found = window(&mut s, "log:all", 0, 10, "", "fresh");
    assert_eq!(last_rows(&found, "log:all").map(|r| r.2), Some(1));
    let (_, _, now) = last_rows(&written(&f, &mut s, "fresh again"), "log:all").expect("the find holds");
    assert_eq!(now, 2, "a window that is found is not moved to the end, and the new event is counted if it matches");
}

#[test]
fn events_that_come_close_together_wait_for_the_tick_and_are_sent_in_one_window() {
    let f = fx(vec![], |_| {});
    let mut s = serve(&f).spaced(std::time::Duration::from_secs(3600));
    watch(&mut s, "log:all");
    let before = f.rig.log.head();
    f.front.note("main", "one", "dim");
    f.front.note("main", "two", "dim");
    let sent: Vec<Down> = f.rig.log.read(before + 1).flat_map(|e| s.ev(&e)).collect();
    assert!(last_rows(&sent, "log:all").is_none(), "{sent:?}");
    let (rows, _, _) = last_rows(&s.tick(), "log:all").expect("the tick sends what waited");
    assert!(rows.iter().any(|r| r.cells[1].text.contains("one")) && rows.iter().any(|r| r.cells[1].text.contains("two")));
    assert!(s.tick().is_empty(), "and then there is nothing");
}

#[test]
fn an_event_that_comes_when_the_last_window_was_sent_long_ago_is_sent_at_once() {
    let f = fx(vec![], |_| {});
    let mut s = serve(&f).spaced(std::time::Duration::ZERO);
    watch(&mut s, "log:all");
    f.front.note("main", "one", "dim");
    let sent: Vec<Down> = f.rig.log.read(f.rig.log.head()).flat_map(|e| s.ev(&e)).collect();
    let (rows, _, _) = last_rows(&sent, "log:all").expect("sent with the event");
    assert!(rows.last().is_some_and(|r| r.cells[1].text.contains("one")), "{rows:?}");
}

#[test]
fn a_log_that_is_built_and_not_written_to_has_nothing_to_send_on_a_tick() {
    let f = fx(vec![], |_| {});
    notes(&f, 300);
    let mut s = serve(&f);
    let first = last_rows(&watch(&mut s, "log:all"), "log:all").expect("sent");
    assert_eq!(first.0.len() as u64, FIRST);
    assert!(f.front.logrows().built());
    assert!(s.tick().is_empty());
}

// ---- lazy children ----

fn big_screen(n: usize) -> Screen {
    let mut shelf = String::from("api = 1\n[[page]]\nid = \"shelf\"\nparent = \"home\"\ntitle = \"Shelf\"\norder = 900\npresent = \"tabs\"\n");
    for i in 0..n {
        shelf.push_str(&format!("[[page]]\nid = \"s{i}\"\nparent = \"shelf\"\ntitle = \"Shelf {i}\"\norder = {i}\nfield = \"list\"\nbind = \"tasks:all\"\n"));
    }
    screen_of(shelf)
}

/// The screen of the base pack and a pack of one file.
fn screen_of(pack: String) -> Screen {
    const FILES: [(&str, &str); 5] = [
        ("keys/main.toml", include_str!("../../../../packs/base/keys/main.toml")),
        ("pages/main.toml", include_str!("../../../../packs/base/pages/main.toml")),
        ("templates/main.toml", include_str!("../../../../packs/base/templates/main.toml")),
        ("views/main.toml", include_str!("../../../../packs/base/views/main.toml")),
        ("commands/main.toml", include_str!("../../../../packs/base/commands/main.toml")),
    ];
    let files = FILES.iter().map(|(p, t)| SourceFile { path: (*p).into(), text: (*t).into() }).collect();
    let shelf = vec![SourceFile { path: "pages/shelf.toml".into(), text: pack }];
    let (tables, _) = Tables::load(vec![Layer::new(LayerKind::Pack, "base", files), Layer::new(LayerKind::Pack, "shelf", shelf)]);
    let (settings, _) = Settings::load(&tables);
    Screen::load(tables, &settings, Vec::new(), &BTreeMap::new()).0
}

fn find<'a>(n: &'a Node, id: &str) -> Option<&'a Node> {
    if n.id == id { Some(n) } else { n.children.iter().find_map(|c| find(c, id)) }
}

fn tree_of(pages: &Pages, lazy: usize) -> Node {
    pages.tree_lazy("main", &j4n_core::eval::Scope::new(), lazy)
}

#[test]
fn a_page_with_more_nodes_under_it_than_the_limit_is_sent_without_them_and_with_their_number() {
    let screen = big_screen(500);
    let whole = tree_of(&screen.pages, 0);
    assert_eq!(find(&whole, "shelf").map(|n| (n.children.len(), n.lazy, n.count)), Some((500, false, None)), "no limit, no lazy page");
    let cut = tree_of(&screen.pages, LAZY);
    let shelf = find(&cut, "shelf").expect("the page");
    assert_eq!((shelf.children.len(), shelf.lazy, shelf.count), (0, true, Some(500)));
    assert!(find(&cut, "chat").is_some_and(|n| !n.lazy), "a page with nothing under it is whole");
    assert!(find(&tree_of(&big_screen(100).pages, LAZY), "shelf").is_some_and(|n| !n.lazy && n.children.len() == 100), "under the limit it is whole");
}

#[test]
fn the_children_of_a_lazy_page_are_made_when_asked_for_and_only_for_a_page_that_is_lazy() {
    let screen = big_screen(500);
    let set = j4n_core::eval::Scope::new();
    let sub = screen.pages.subtree("shelf", "main", &set, LAZY).expect("the shelf is lazy");
    assert_eq!((sub.id.as_str(), sub.children.len(), sub.lazy), ("shelf", 500, false));
    assert_eq!(sub.children[7].title, "Shelf 7");
    assert!(screen.pages.subtree("chat", "main", &set, LAZY).is_none(), "a page that is whole is not lazy");
    assert!(screen.pages.subtree("nothing", "main", &set, LAZY).is_none());
    assert!(big_screen(100).pages.subtree("shelf", "main", &set, LAZY).is_none());
}

fn serving_big(n: usize) -> (Serve, Fx) {
    let f = fx(vec![], |_| {});
    let Fx { front, rig, decided, asked, keeper, lanes, pays, zone, hook, boom, lag } = f;
    let front = front.with_screen(Arc::new(big_screen(n)));
    let s = Serve::new(front.clone());
    (s, Fx { front, rig, decided, asked, keeper, lanes, pays, zone, hook, boom, lag })
}

#[test]
fn a_client_is_sent_the_page_without_its_children_and_is_sent_them_when_it_opens_the_page() {
    let (mut s, _f) = serving_big(500);
    let hello = s.up(Up::Hello { api: j4n_proto::API }).0;
    let tree = hello.iter().find_map(|d| if let Down::Tree { node } = d { Some(node) } else { None }).expect("the tree");
    assert_eq!(find(tree, "shelf").map(|n| (n.lazy, n.count, n.children.len())), Some((true, Some(500), 0)));
    let opened = s.up(Up::Intent { intent: Intent::Open { node: "shelf".into() } }).0;
    let [Down::Tree { node }] = &opened[..] else { panic!("{opened:?}") };
    assert_eq!((node.id.as_str(), node.children.len(), node.lazy), ("shelf", 500, false));
    let focus = s.up(Up::Intent { intent: Intent::Open { node: "log".into() } }).0;
    assert_eq!(focus, [Down::Focus { node: "log".into() }], "a page that is whole is gone to");
}

#[test]
fn a_page_is_lazy_past_the_limit_and_not_at_it_and_the_nodes_under_the_nodes_under_it_count() {
    let set = j4n_core::eval::Scope::new();
    let lazy_at = |n| find(&tree_of(&big_screen(n).pages, LAZY), "shelf").map(|n| n.lazy);
    assert_eq!((lazy_at(LAZY), lazy_at(LAZY + 1)), (Some(false), Some(true)), "the limit itself is not past it");
    // three pages with a hundred and fifty under each: three children, four hundred and fifty three nodes
    let mut pack = String::from("api = 1\n[[page]]\nid = \"shelf\"\nparent = \"home\"\ntitle = \"Shelf\"\norder = 900\npresent = \"tabs\"\n");
    for i in 0..3 {
        pack.push_str(&format!("[[page]]\nid = \"g{i}\"\nparent = \"shelf\"\ntitle = \"G{i}\"\norder = {i}\npresent = \"tabs\"\n"));
        for j in 0..150 {
            pack.push_str(&format!("[[page]]\nid = \"g{i}x{j}\"\nparent = \"g{i}\"\ntitle = \"X\"\norder = {j}\nfield = \"list\"\nbind = \"tasks:all\"\n"));
        }
    }
    let pages = screen_of(pack).pages;
    let shelf = pages.tree_lazy("main", &set, LAZY);
    let shelf = find(&shelf, "shelf").expect("the page");
    assert_eq!((shelf.lazy, shelf.count), (true, Some(3)), "its children are few and what is under them is many");
    let opened = pages.subtree("shelf", "main", &set, LAZY).expect("opened");
    assert!(opened.children.iter().all(|g| g.lazy == (g.children.is_empty())) && opened.children.len() == 3, "the pages under it are whole: 150 are not past the limit");
}

fn table_screen() -> Screen {
    screen_of("api = 1\n[[page]]\nid = \"grid\"\nparent = \"home\"\ntitle = \"Grid\"\norder = 900\nfield = \"table\"\nbind = \"form:thread\"\nprops = { columns = [\"name\", \"value\"] }\n".into())
}

fn serving_table(whole: usize, first: u64) -> (Serve, Fx) {
    let f = fx(vec![], |_| {});
    let Fx { front, rig, decided, asked, keeper, lanes, pays, zone, hook, boom, lag } = f;
    let front = front.with_screen(Arc::new(table_screen()));
    let mut s = Serve::new(front.clone()).windowed_over(whole, first);
    s.up(Up::Hello { api: j4n_proto::API });
    (s, Fx { front, rig, decided, asked, keeper, lanes, pays, zone, hook, boom, lag })
}

#[test]
fn rows_of_a_table_past_the_size_are_sent_as_a_window_and_the_rest_whole() {
    let (mut s, _f) = serving_table(2, 3);
    let (rows, from, total) = last_rows(&watch(&mut s, "form:thread"), "form:thread").expect("sent");
    assert_eq!((keys(&rows), from, total), (vec!["doer", "model", "how"], 0, 4), "four rows are past two: the first three");
    let (mut s, _f) = serving_table(4, 3);
    let (rows, _, total) = last_rows(&watch(&mut s, "form:thread"), "form:thread").expect("sent");
    assert_eq!((rows.len(), total), (4, 4), "four rows are not past four");
    // a list of the same rows is not a field that shows a window, and is sent whole whatever its size
    let f = fx(vec![], |_| {});
    let mut s = serve(&f).windowed_over(2, 3);
    s.up(Up::Hello { api: j4n_proto::API });
    assert_eq!(last_rows(&watch(&mut s, "form:thread"), "form:thread").map(|r| r.0.len()), Some(4));
}

#[test]
fn a_table_is_put_in_the_order_of_the_column_it_names_by_the_core() {
    let (mut s, _f) = serving_table(2, 3);
    watch(&mut s, "form:thread");
    let (rows, _, total) = last_rows(&window(&mut s, "form:thread", 0, 10, "-name", ""), "form:thread").expect("an answer");
    assert_eq!((keys(&rows), total), (vec!["model", "mode", "how", "doer"], 4));
    let (rows, _, _) = last_rows(&window(&mut s, "form:thread", 0, 10, "name", ""), "form:thread").expect("an answer");
    assert_eq!(keys(&rows), ["doer", "how", "mode", "model"]);
}

#[test]
fn what_was_heard_of_the_log_only_grows_and_a_number_is_found_by_the_event_that_has_it() {
    let mut log = log_of(10);
    log.push(&event(13));
    log.push(&event(5));
    log.hear(3);
    assert!(!log.built(), "an event that is old, and a head that is old, tell nothing of what was heard");
    let bare = |i: u64| Ev { data: serde_json::Value::Null, task: None, kind: "x.y".into(), ..event(i) };
    let mut log = LogRows::of((1..=100).map(bare));
    assert_eq!(seqs(&log.window(&ask(0, 10, "", "77")).0), [77], "by the number alone");
}

#[test]
fn a_log_put_in_the_order_of_a_kind_is_put_again_when_events_are_written() {
    let mut log = log_of(30);
    assert_eq!(log.window(&ask(0, 1, "kind", "")).1, 30);
    log.push(&event(31));
    assert_eq!(log.window(&ask(0, 40, "kind", "")).1, 31);
    assert!(seqs(&log.window(&ask(0, 40, "kind", "")).0).contains(&31), "the new event is in the order");
}

#[test]
fn events_written_without_being_pushed_are_read_on_the_tick() {
    let f = fx(vec![], |_| {});
    notes(&f, 5);
    let mut s = serve(&f);
    watch(&mut s, "log:all");
    f.front.note("main", "unheard", "dim");
    let (rows, _, _) = last_rows(&s.tick(), "log:all").expect("the tick read them");
    assert!(rows.last().is_some_and(|r| r.cells[1].text.contains("unheard")), "{rows:?}");
}

#[test]
fn a_window_asked_for_past_the_end_is_told_where_the_end_is() {
    let f = fx(vec![], |_| {});
    let mut s = serve(&f);
    watch(&mut s, "form:thread");
    let (rows, from, total) = last_rows(&window(&mut s, "form:thread", 99, 5, "", ""), "form:thread").expect("an answer");
    assert_eq!((rows.len(), from, total), (0, 4, 4));
}

fn shelf_with_a_table(when: &str) -> Screen {
    let mut pack = format!("api = 1\n[[page]]\nid = \"shelf\"\nparent = \"home\"\ntitle = \"Shelf\"\norder = 900\npresent = \"tabs\"\n{when}");
    pack.push_str("[[page]]\nid = \"grid\"\nparent = \"shelf\"\ntitle = \"Grid\"\norder = 0\nfield = \"table\"\nbind = \"form:thread\"\nprops = { columns = [\"name\", \"value\"] }\n");
    for i in 1..450 {
        pack.push_str(&format!("[[page]]\nid = \"s{i}\"\nparent = \"shelf\"\ntitle = \"S{i}\"\norder = {i}\nfield = \"list\"\nbind = \"tasks:all\"\n"));
    }
    screen_of(pack)
}

#[test]
fn a_table_that_was_under_a_lazy_page_shows_a_window_once_the_page_is_opened() {
    let f = fx(vec![], |_| {});
    let Fx { front, .. } = f;
    let mut s = Serve::new(front.with_screen(Arc::new(shelf_with_a_table("")))).windowed_over(2, 3);
    s.up(Up::Hello { api: j4n_proto::API });
    let opened = s.up(Up::Intent { intent: Intent::Open { node: "shelf".into() } }).0;
    assert!(matches!(&opened[..], [Down::Tree { .. }]), "{opened:?}");
    let (rows, _, total) = last_rows(&watch(&mut s, "form:thread"), "form:thread").expect("sent");
    assert_eq!((rows.len(), total), (3, 4));
}

#[test]
fn a_page_that_is_not_shown_is_not_opened() {
    let screen = shelf_with_a_table("when = \"set.shown\"\n");
    let set = j4n_core::eval::Scope::new();
    assert!(find(&tree_of(&screen.pages, LAZY), "shelf").is_none(), "it is not in the tree");
    assert!(screen.pages.subtree("shelf", "main", &set, LAZY).is_none());
}

// ---- the events of a window, whole, and views over the Log ----

#[test]
fn the_rows_of_a_window_carry_their_event_whole_and_only_they_do() {
    let mut log = log_of(10_000);
    let (mut rows, _) = log.window(&ask(9_990, 3, "", ""));
    assert!(rows.iter().all(|r| r.data.is_none()), "a window is cut without its events");
    log.fill(None, &mut rows);
    let ev = rows[0].data.as_ref().expect("the newest events are kept whole");
    assert_eq!(ev["seq"], 9_991);
    assert_eq!((ev["kind"].as_str(), ev["task"].as_str(), ev["by"].as_str(), ev["class"].as_str()), (Some("run.message"), Some("t2"), Some("core"), Some("project")));
    assert_eq!(ev["data"], json!({ "n": 9_991, "word": "w9991" }), "the data as structure, not a string");
    // the oldest are no longer kept, and with no Log to read them from they are sent as their cells only
    let (mut old, _) = log.window(&ask(0, 2, "", ""));
    log.fill(None, &mut old);
    assert!(old.iter().all(|r| r.data.is_none()));
}

#[test]
fn an_old_event_of_a_window_is_read_from_the_files_with_its_long_text_whole() {
    let f = fx(vec![], |_| {});
    let long = "a word ".repeat(200);
    f.front.note("main", &long, "dim");
    notes(&f, 5_000);
    let (log, head) = (&f.rig.log, f.rig.log.head());
    let mut rows = LogRows::new();
    rows.hear(head);
    while !rows.built() {
        rows.catch_up(log, 2_000);
    }
    let first = log.read(0).find(|e| e.data["text"] == long).expect("written").seq;
    let (mut win, _) = rows.window(&ask(first - 1, 2, "", ""));
    assert_eq!(win[0].key, format!("e{first}"));
    assert!(win[0].cells[1].text.chars().count() < 260, "the cell is cut");
    rows.fill(Some(log), &mut win);
    assert_eq!(win[0].data.as_ref().expect("read from the files")["data"]["text"], long.as_str(), "the event is not cut");
    assert_eq!(win[1].data.as_ref().expect("and the next")["seq"], first + 1);
    // two far apart, in one window sorted the other way
    let (mut far, _) = rows.window(&ask(0, 1, "", "kind:chat.note"));
    far.extend(rows.window(&ask(0, 1, "-seq", "")).0);
    rows.fill(Some(log), &mut far);
    assert!(far.iter().all(|r| r.data.is_some()), "{far:?}");
}

#[test]
fn a_find_of_kind_and_task_keeps_only_those_and_a_word_after_them_is_looked_for_among_them() {
    let mut log = log_of(1_000);
    let (rows, total) = log.window(&ask(0, 3, "", "kind:gate.verdict"));
    assert_eq!((seqs(&rows), total), (vec![2, 5, 8], 333));
    let (rows, total) = log.window(&ask(0, 3, "", "task:t3"));
    assert_eq!((seqs(&rows), total), (vec![3, 10, 17], 143));
    assert_eq!(log.window(&ask(0, 3, "", "task:t3 kind:gate.verdict")).1, 47, "both: every 21st");
    let (rows, total) = log.window(&ask(0, 3, "", "kind:gate.verdict w99"));
    assert_eq!((seqs(&rows), total), (vec![992, 995, 998], 3), "the word among the kind");
    assert_eq!(log.window(&ask(0, 3, "", "kind:gate")).1, 0, "a kind is the whole kind, not a part of it");
    assert_eq!(log.window(&ask(0, 3, "", "kind: x")).1, 0, "an empty kind is a word like any other");
}

fn view_over_the_log() -> Screen {
    screen_of("api = 1\n[[view]]\nid = \"notes\"\nfrom = { from = \"log:chat.*\" }\ncells = [\"{kind}\", \"{data.text}\", \"{data.tone}\"]\n".into())
}

#[test]
fn a_view_over_the_log_has_what_each_event_said() {
    let f = fx(vec![], |_| {});
    let Fx { front, .. } = f;
    let front = front.with_screen(Arc::new(view_over_the_log()));
    front.note("main", "hello there", "warn");
    let mut s = Serve::new(front);
    let (rows, _, _) = last_rows(&watch(&mut s, "view:notes"), "view:notes").expect("sent");
    let row = rows.iter().find(|r| r.cells.first().is_some_and(|c| c.text == "chat.note")).unwrap_or_else(|| panic!("{rows:?}"));
    assert_eq!((row.cells[1].text.as_str(), row.cells[2].text.as_str()), ("hello there", "warn"), "the data's fields, not empty cells");
}

// ---- a line that is seen again is one row with `×n` ----

/// The event `seq`, of `kind`, in the task `t1`, that carries the word `w`; the time is its own.
fn said(seq: u64, kind: &str, w: &str) -> Ev {
    Ev { seq, at_ms: seq * 10, kind: kind.into(), task: Some("t1".into()), thread: None, by: Actor::Core, cause: None, class: Default::default(), data: json!({ "w": w }) }
}

/// Events of the kinds and words given, numbered from 1.
fn said_all(spec: &[(&str, &str)]) -> LogRows {
    LogRows::of(spec.iter().enumerate().map(|(i, (k, w))| said(i as u64 + 1, k, w)))
}

/// What a row says of a run: its first event, and the `×n` of its fourth cell (1: none).
fn run_of(r: &Row) -> (String, usize) {
    (r.key.clone(), r.cells.get(3).map_or(1, |c| c.text.trim_start_matches('×').parse().expect("×n")))
}

fn runs(rows: &[Row]) -> Vec<(String, usize)> {
    rows.iter().map(run_of).collect()
}

fn run(key: &str, n: usize) -> (String, usize) {
    (key.into(), n)
}

#[test]
fn a_line_seen_again_straight_after_is_one_row_with_how_many_in_its_meta() {
    let mut log = said_all(&[("a.x", "1"), ("a.x", "1"), ("a.x", "1"), ("a.x", "2"), ("b.x", "2"), ("a.x", "2"), ("a.x", "2")]);
    let (rows, total) = log.window(&ask(0, 10, "", ""));
    assert_eq!((total, log.rows()), (4, 4), "seven events, four rows");
    assert_eq!(runs(&rows), [run("e1", 3), run("e4", 1), run("e5", 1), run("e6", 2)]);
    assert_eq!((rows[0].cells[3].text.as_str(), rows[0].cells.len(), rows[1].cells.len()), ("×3", 4, 3), "a line seen once has no count");
    let (rows, total) = log.window(&ask(1, 2, "", ""));
    assert_eq!((runs(&rows), total), (vec![run("e4", 1), run("e5", 1)], 4), "a window is of rows, not of events");
    let (rows, _) = log.window(&ask(0, 10, "-seq", ""));
    assert_eq!(runs(&rows), [run("e6", 2), run("e5", 1), run("e4", 1), run("e1", 3)], "the other way: the same runs, each led by its first event");
}

#[test]
fn a_line_that_differs_in_anything_but_its_time_and_number_is_another_row() {
    let base = said(1, "a.x", "w");
    let others = [
        Ev { kind: "a.y".into(), ..said(2, "a.x", "w") },
        Ev { task: Some("t2".into()), ..said(2, "a.x", "w") },
        Ev { task: None, ..said(2, "a.x", "w") },
        Ev { thread: Some("main".into()), ..said(2, "a.x", "w") },
        Ev { by: Actor::Operator, ..said(2, "a.x", "w") },
        Ev { cause: Some(1), ..said(2, "a.x", "w") },
        Ev { class: j4n_core::schema::DataClass::Secret, ..said(2, "a.x", "w") },
        Ev { data: json!({ "w": "w", "more": 1 }), ..said(2, "a.x", "w") },
        Ev { data: json!({ "w": "W" }), ..said(2, "a.x", "w") },
        Ev { data: json!(null), ..said(2, "a.x", "w") },
    ];
    for (i, other) in others.into_iter().enumerate() {
        assert_eq!(LogRows::of([base.clone(), other]).window(&ask(0, 9, "", "")).1, 2, "case {i} is a row of its own");
    }
    let same = Ev { at_ms: 99_999, ..said(2, "a.x", "w") };
    assert_eq!(LogRows::of([base, same]).window(&ask(0, 9, "", "")).1, 1, "the time and the number are not part of the line");
    // a line is not the same as one that is long and alike in its first 200 characters
    let long = |tail: &str| Ev { data: json!({ "w": format!("{}{tail}", "x".repeat(400)) }), ..said(1, "a.x", "") };
    let (a, b) = (long("a"), Ev { seq: 2, ..long("b") });
    assert_eq!(LogRows::of([a, b]).window(&ask(0, 9, "", "")).1, 2, "what is cut from the line is still told apart");
    // one that comes again later, with another between, is not folded
    let mut log = said_all(&[("a.x", "1"), ("a.x", "2"), ("a.x", "1")]);
    assert_eq!(log.window(&ask(0, 9, "", "")).1, 3);
}

#[test]
fn a_find_finds_inside_a_run_and_a_run_is_of_the_events_that_were_found() {
    let mut log = said_all(&[("a.x", "one"), ("a.x", "one"), ("a.x", "one"), ("a.x", "one"), ("b.x", "two"), ("a.x", "one"), ("a.x", "one")]);
    // the word is in every event of the runs: the rows are the runs, and the events between that were not found are not there
    let (rows, total) = log.window(&ask(0, 9, "", "one"));
    assert_eq!((runs(&rows), total), (vec![run("e1", 6)], 1), "kind a.x, six events, the b between is not a hit");
    // a number is in one event, and it is not inside the run of the events around it
    let (rows, total) = log.window(&ask(0, 9, "", "3"));
    assert_eq!((runs(&rows), total), (vec![run("e3", 1)], 1), "the hit in the middle of a run of four is a row");
    let (rows, _) = log.window(&ask(0, 9, "", "kind:b.x"));
    assert_eq!(runs(&rows), [run("e5", 1)]);
    let (rows, total) = log.window(&ask(0, 9, "", "kind:a.x"));
    assert_eq!((runs(&rows), total), (vec![run("e1", 6)], 1), "a kind filter keeps the events of that kind, and they are alike");
    // sorted and found together
    let (rows, total) = log.window(&ask(0, 9, "kind", "one"));
    assert_eq!((runs(&rows), total), (vec![run("e1", 6)], 1));
    let (rows, total) = log.window(&ask(0, 9, "task", "3"));
    assert_eq!((runs(&rows), total), (vec![run("e3", 1)], 1));
    // every event that holds the word is in the runs that are shown: nothing a find hits is hidden
    let counted: usize = log.window(&ask(0, 9, "", "a.x")).0.iter().map(|r| run_of(r).1).sum();
    assert_eq!(counted, 6);
}

#[test]
fn a_log_in_the_order_of_a_kind_folds_what_is_next_to_each_other_in_that_order() {
    let mut log = said_all(&[("a.x", "1"), ("b.x", "2"), ("a.x", "1"), ("b.x", "2"), ("a.x", "3")]);
    assert_eq!(log.window(&ask(0, 9, "", "")).1, 5, "in the order written nothing is next to its like");
    let (rows, total) = log.window(&ask(0, 9, "kind", ""));
    assert_eq!((runs(&rows), total), (vec![run("e1", 2), run("e5", 1), run("e2", 2)], 3));
    let (rows, _) = log.window(&ask(0, 9, "-kind", ""));
    assert_eq!(runs(&rows), [run("e2", 2), run("e5", 1), run("e1", 2)]);
}

#[test]
fn the_end_of_the_log_grows_the_count_of_its_last_row_and_adds_none_while_the_line_is_the_same() {
    let mut log = said_all(&[("a.x", "1"), ("b.x", "2"), ("b.x", "2")]);
    assert_eq!((log.rows(), log.len()), (2, 3));
    for seq in 4..=6 {
        log.push(&said(seq, "b.x", "2"));
    }
    let (rows, total) = log.window(&ask(0, 9, "", ""));
    assert_eq!((runs(&rows), total, log.rows()), (vec![run("e1", 1), run("e2", 5)], 2, 2), "five events of the line, still one row, still e2");
    log.push(&said(7, "a.x", "1"));
    assert_eq!(runs(&log.window(&ask(0, 9, "", "")).0), [run("e1", 1), run("e2", 5), run("e7", 1)], "another line is a new row");
    // a find that was made counts the new events into its run
    assert_eq!(runs(&log.window(&ask(0, 9, "", "b.x")).0), [run("e2", 5)]);
    log.push(&said(8, "b.x", "2"));
    log.push(&said(9, "b.x", "2"));
    assert_eq!(runs(&log.window(&ask(0, 9, "", "b.x")).0), [run("e2", 7)], "the find that was made counts the new events into its run: the a.x between is no hit, so the rows next to each other are alike");
    // sorted
    assert_eq!(runs(&log.window(&ask(0, 9, "kind", "")).0), [run("e1", 2), run("e2", 7)], "in the order of the kind the two runs of b.x are next to each other, and alike");
}

#[test]
fn the_event_of_a_run_is_sent_whole_with_how_many_and_where_the_run_ends() {
    let mut log = said_all(&[("a.x", "1"), ("b.x", "2"), ("b.x", "2"), ("b.x", "2"), ("a.x", "1")]);
    let (mut rows, _) = log.window(&ask(0, 9, "", ""));
    log.fill(None, &mut rows);
    let ev = rows[1].data.as_ref().expect("the first of the run");
    assert_eq!((ev["seq"].as_u64(), ev["at_ms"].as_u64()), (Some(2), Some(20)), "the first event of the run");
    assert_eq!((ev["repeats"].as_u64(), ev["last_seq"].as_u64(), ev["last_at_ms"].as_u64()), (Some(3), Some(4), Some(40)));
    assert_eq!(ev["data"], json!({ "w": "2" }));
    assert!(rows[0].data.as_ref().is_some_and(|e| e.get("repeats").is_none()), "a line seen once says nothing of repeats");
    // the window after: the table of the last window is of that window
    let (mut again, _) = log.window(&ask(0, 1, "", ""));
    log.fill(None, &mut again);
    assert!(again[0].data.as_ref().is_some_and(|e| e.get("repeats").is_none()));
}

#[test]
fn the_last_event_of_a_run_that_is_no_longer_kept_is_read_from_the_files() {
    let f = fx(vec![], |_| {});
    for _ in 0..3 {
        f.front.note("main", "the same", "dim");
    }
    notes(&f, 5_000);
    let (log, head) = (&f.rig.log, f.rig.log.head());
    let mut rows = LogRows::new();
    rows.hear(head);
    while !rows.built() {
        rows.catch_up(log, 2_000);
    }
    let first = log.read(0).find(|e| e.data["text"] == "the same").expect("written").seq;
    let (mut win, _) = rows.window(&ask(0, 400, "", "the same"));
    assert_eq!(runs(&win), [run(&format!("e{first}"), 3)]);
    rows.fill(None, &mut win);
    assert!(win[0].data.is_none(), "with no files the old event is not sent");
    rows.fill(Some(log), &mut win);
    let ev = win[0].data.as_ref().expect("read from the files");
    let last = log.read(first).take(3).last().expect("three");
    assert_eq!((ev["repeats"].as_u64(), ev["last_seq"].as_u64(), ev["last_at_ms"].as_u64()), (Some(3), Some(last.seq), Some(last.at_ms)));
}

#[test]
fn a_client_is_sent_the_end_of_the_log_as_runs_and_the_end_grows_in_place() {
    let f = fx(vec![], |_| {});
    notes(&f, 300);
    let mut s = serve(&f);
    let total = last_rows(&watch(&mut s, "log:all"), "log:all").expect("sent").2;
    let (rows, _, now) = last_rows(&written(&f, &mut s, "same"), "log:all").expect("the end moved");
    assert_eq!((now, run_of(&rows[rows.len() - 1]).1), (total + 1, 1));
    let (rows, from, now) = last_rows(&written(&f, &mut s, "same"), "log:all").expect("the end changed");
    assert_eq!(now, total + 1, "the line is the same: the count grows and the rows do not");
    assert_eq!(run_of(&rows[rows.len() - 1]).1, 2);
    assert_eq!(from + rows.len() as u64, now, "and the window holds the end still");
    let (rows, _, now) = last_rows(&written(&f, &mut s, "same"), "log:all").expect("the end changed");
    assert_eq!((now, run_of(&rows[rows.len() - 1]).1), (total + 1, 3));
    let (rows, _, now) = last_rows(&written(&f, &mut s, "other"), "log:all").expect("a new row");
    assert_eq!((now, run_of(&rows[rows.len() - 1]).1), (total + 2, 1));
}

#[test]
fn a_log_of_a_great_many_events_in_runs_is_windowed_found_and_sorted_as_quickly_as_one_without() {
    let n = 400_000u64;
    let kinds = ["task.queued", "run.message", "gate.verdict"];
    let events = |run: u64| (1..=n).map(move |i| said(i, kinds[((i - 1) / run % 3) as usize], &format!("w{}", (i - 1) / run)));
    let (mut plain, mut folded) = (LogRows::of(events(1)), LogRows::of(events(40)));
    assert_eq!((plain.rows(), folded.rows()), (n, n / 40));
    for log in [&mut plain, &mut folded] {
        let t = std::time::Instant::now();
        for p in 0..200 {
            let (rows, total) = log.window(&ask((p * 7_919) % (log.rows() - 100), 100, if p % 2 == 0 { "" } else { "-seq" }, ""));
            assert_eq!(rows.len(), 100);
            assert!(total > 0);
        }
        // a word never looked for, then the next, then the same with an order
        assert!(log.window(&ask(0, 5, "", "w9999")).1 > 0);
        assert!(log.window(&ask(0, 5, "kind", "w9998")).1 > 0);
        assert!(log.window(&ask(0, 5, "task", "gate")).1 > 0);
        assert!(t.elapsed().as_secs_f64() < 20.0, "slow: {:?}", t.elapsed());
    }
    assert_eq!(runs(&folded.window(&ask(0, 5, "", "w9999")).0), [run("e399961", 40)], "the last run of the forty");
    // all the events are in some row of the Log and of each find, whichever the order
    let (all, total) = folded.window(&ask(0, MOST, "", ""));
    assert_eq!((total, all.iter().map(|r| run_of(r).1).sum::<usize>()), (n / 40, 2_000 * 40));
    let rest: usize = (0..total.div_ceil(MOST)).flat_map(|p| folded.window(&ask(p * MOST, MOST, "kind", "")).0).map(|r| run_of(&r).1).sum();
    assert_eq!(rest as u64, n);
}
