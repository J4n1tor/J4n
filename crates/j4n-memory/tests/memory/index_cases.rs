use j4n_core::change::Actor;
use j4n_core::log::{Log, New, Opts, Scope};
use j4n_core::redact::Redactor;
use j4n_core::schema::DataClass;
use j4n_memory::{HITS_MAX, Index};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!("j4n-mem-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn log_in(dir: &Path) -> Log {
    Log::open(&dir.join(".j4n"), Scope::Project, Opts::default(), Redactor::new()).unwrap().0
}

fn put(log: &Log, kind: &str, thread: &str, data: serde_json::Value) -> u64 {
    log.append(New::of(kind, Actor::Operator, data).thread(thread)).unwrap().seq
}

#[test]
fn notes_lines_and_good_reports_are_kept_and_nothing_else() {
    let d = Dir::new("kept");
    let log = log_in(&d.0);
    let n = put(&log, "note", "main", json!({ "text": "the build needs --release", "tags": ["build"] }));
    let l = put(&log, "chat.say", "main", json!({ "text": "please fix the parser" }));
    let r = put(&log, "task.settled", "main", json!({ "ok": true, "answer": "fixed the parser in two places" }));
    put(&log, "task.settled", "main", json!({ "ok": false, "answer": "parser still broken", "reason": "x" }));
    put(&log, "task.settled", "main", json!({ "ok": true, "answer": null }));
    put(&log, "chat.note", "main", json!({ "text": "parser noise" }));
    // a command is not about the work, and would find itself
    put(&log, "chat.say", "main", json!({ "text": ":recall parser" }));
    put(&log, "chat.say", "main", json!({ "text": "/plan the parser" }));
    put(&log, "note", "main", json!({ "text": "   " }));
    let index = Index::in_memory().unwrap();
    assert_eq!(index.catch_up(&log).unwrap(), 3);
    assert_eq!(index.len(), 3);
    let hits = index.search("parser", None, DataClass::Private, 10).unwrap();
    let seqs: Vec<u64> = hits.iter().map(|h| h.seq).collect();
    assert!(seqs.contains(&l) && seqs.contains(&r) && !seqs.contains(&n), "{seqs:?}");
    assert_eq!(hits.iter().find(|h| h.seq == l).unwrap().kind, "chat.say");
    assert!(hits[0].text.contains('«'), "the words that matched are marked: {}", hits[0].text);
    // a tag is words too
    assert_eq!(index.search("build", None, DataClass::Private, 10).unwrap()[0].seq, n);
}

#[test]
fn it_goes_on_from_the_last_event_it_saw_and_is_made_again_for_a_log_that_is_not_the_one_it_saw() {
    let d = Dir::new("again");
    let log = log_in(&d.0);
    put(&log, "note", "main", json!({ "text": "alpha one" }));
    let index = Index::open(&d.0.join("idx/memory.db")).unwrap();
    assert_eq!(index.catch_up(&log).unwrap(), 1);
    assert_eq!(index.catch_up(&log).unwrap(), 0, "nothing new");
    put(&log, "note", "main", json!({ "text": "alpha two" }));
    assert_eq!(index.catch_up(&log).unwrap(), 1, "only the new one");
    assert_eq!(index.search("alpha", None, DataClass::Private, 10).unwrap().len(), 2);
    // closed and opened again: it has kept what it saw
    drop(index);
    let index = Index::open(&d.0.join("idx/memory.db")).unwrap();
    assert_eq!(index.len(), 2);
    assert_eq!(index.seen(), 2);
    // another Log, shorter than what the index saw: the index is made again from it
    let other = Dir::new("other");
    let log2 = log_in(&other.0);
    put(&log2, "note", "main", json!({ "text": "beta only" }));
    assert_eq!(index.catch_up(&log2).unwrap(), 1);
    assert!(index.search("alpha", None, DataClass::Private, 10).unwrap().is_empty());
    assert_eq!(index.search("beta", None, DataClass::Private, 10).unwrap().len(), 1);
}

#[test]
fn what_is_asked_is_words_and_never_the_syntax_of_the_search() {
    let d = Dir::new("syntax");
    let log = log_in(&d.0);
    put(&log, "note", "main", json!({ "text": "Releases are built by running cargo build" }));
    put(&log, "note", "main", json!({ "text": "unrelated gardening" }));
    let index = Index::in_memory().unwrap();
    index.catch_up(&log).unwrap();
    let find = |q: &str| index.search(q, None, DataClass::Private, 10).unwrap().len();
    assert_eq!(find("release"), 1, "a word is found by its stem");
    assert_eq!(find("cargo bui"), 1, "the last word is the start of one");
    assert_eq!(find("cargo AND gardening"), 0, "AND is a word asked for, not an operator");
    for q in ["\"", "*", "NEAR(", "cargo OR", "a:b", "-x", "'; DROP TABLE item; --", "   ", "", "!!!"] {
        let _ = find(q);
    }
    assert_eq!(index.len(), 2, "no query changed anything");
    assert_eq!(find("   "), 0);
}

#[test]
fn a_class_above_what_is_asked_for_is_never_given_and_a_secret_is_never_kept() {
    let d = Dir::new("class");
    let log = log_in(&d.0);
    for (class, text) in [(DataClass::Public, "plum public"), (DataClass::Project, "plum project"), (DataClass::Private, "plum private"), (DataClass::Secret, "plum secret")] {
        log.append(New::of("note", Actor::Operator, json!({ "text": text })).class(class)).unwrap();
    }
    let index = Index::in_memory().unwrap();
    assert_eq!(index.catch_up(&log).unwrap(), 3, "the secret is not kept");
    let words = |up_to| {
        let mut w: Vec<String> = index.search("plum", None, up_to, 10).unwrap().iter().map(|h| h.text.replace(['«', '»'], "")).collect();
        w.sort();
        w
    };
    assert_eq!(words(DataClass::Public), ["plum public"]);
    assert_eq!(words(DataClass::Project), ["plum project", "plum public"]);
    assert_eq!(words(DataClass::Private).len(), 3);
}

#[test]
fn a_thread_narrows_the_search_and_the_most_asked_for_is_bounded() {
    let d = Dir::new("narrow");
    let log = log_in(&d.0);
    put(&log, "note", "a", json!({ "text": "shared word in a" }));
    put(&log, "note", "b", json!({ "text": "shared word in b" }));
    for n in 0..(HITS_MAX + 20) {
        put(&log, "note", "c", json!({ "text": format!("many {n}") }));
    }
    let index = Index::in_memory().unwrap();
    index.catch_up(&log).unwrap();
    let hits = index.search("shared", Some("b"), DataClass::Private, 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].thread, "b");
    assert_eq!(index.search("many", None, DataClass::Private, 10_000).unwrap().len(), HITS_MAX);
    assert_eq!(index.search("many", None, DataClass::Private, 0).unwrap().len(), 1, "at least one is asked for");
}

#[test]
fn an_index_of_another_format_is_made_again_and_a_long_report_is_cut() {
    let d = Dir::new("format");
    let log = log_in(&d.0);
    put(&log, "task.settled", "main", json!({ "ok": true, "answer": format!("start {}", "x ".repeat(10_000)) }));
    let path = d.0.join("memory.db");
    let index = Index::open(&path).unwrap();
    index.catch_up(&log).unwrap();
    let hit = &index.search("start", None, DataClass::Private, 1).unwrap()[0];
    assert!(hit.text.chars().count() < 400, "a snippet, not the report");
    drop(index);
    // the file says another format: whatever it held is not trusted
    let conn = rusqlite_path(&path);
    conn.execute("UPDATE meta SET v = '0' WHERE k = 'format'", []).unwrap();
    drop(conn);
    let index = Index::open(&path).unwrap();
    assert_eq!(index.len(), 0);
    assert_eq!(index.seen(), 0);
    assert_eq!(index.catch_up(&log).unwrap(), 1);
}

fn rusqlite_path(p: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open(p).unwrap()
}

#[test]
fn the_best_match_comes_first_and_not_the_oldest() {
    let d = Dir::new("rank");
    let log = log_in(&d.0);
    put(&log, "note", "main", json!({ "text": "a long note about many things in which a plum is named once among a great many other words" }));
    let best = put(&log, "note", "main", json!({ "text": "plum plum plum" }));
    let index = Index::in_memory().unwrap();
    index.catch_up(&log).unwrap();
    assert_eq!(index.search("plum", None, DataClass::Private, 10).unwrap()[0].seq, best);
}
