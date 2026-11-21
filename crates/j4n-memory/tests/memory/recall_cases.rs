//! Remembered claims over a real Log and a fixture folder. The event a report
//! is written as, and the Brief, are `j4n-core`'s `p3_recall`.

use j4n_core::brief::{Brief, tokens};
use j4n_core::change::Actor;
use j4n_core::front::Remembered;
use j4n_core::log::{Log, New, Opts, Scope};
use j4n_core::redact::Redactor;
use j4n_core::report::{Report, goal_key};
use j4n_core::schema::DataClass;
use j4n_core::setting::Settings;
use j4n_core::value::Value;
use j4n_memory::{Index, RECALL_TOKENS, Recall};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!("j4n-recall-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }

    fn put(&self, path: &str, text: &str) {
        let at = self.0.join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
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

fn recall(dir: &Dir, log: &Log, on: Option<bool>) -> Recall {
    let settings = Settings::from_pairs(on.map(|b| ("brief.recall", Value::Bool(b))));
    Recall::new(Arc::new(Index::in_memory().unwrap()), log.clone(), &dir.0, settings)
}

/// A report written as `Lanes::settle` writes it: the answer taken, checked against the folder, with the goal's key and the hash of
/// each file a checked claim names.
fn made(log: &Log, dir: &Dir, goal: &str, answer: serde_json::Value, class: DataClass) {
    let mut report = Report::take(&answer.to_string()).unwrap();
    report.check(&dir.0, &[("cargo test".into(), 0)]);
    let data = json!({ "report": report.json(), "deliver": true, "goal": goal_key(goal), "files": report.files(&dir.0) });
    log.append(New::of("report.made", Actor::Core, data).thread("main").task("t2").class(class)).unwrap();
}

fn fixture() -> Dir {
    let d = Dir::new();
    d.put("src/net.rs", "fn main() {}\nfn fetch() { retry(3) }\n");
    d.put("src/io.rs", "pub fn read_all() {}\n");
    d
}

fn answer() -> serde_json::Value {
    json!({
        "answer": "the retry lives in fetch(); nothing else retries",
        "found": [
            { "claim": "the retry is in fetch()", "at": "src/net.rs:2" },
            { "claim": "read_all is the only reader", "at": "src/io.rs:1" },
            { "claim": "fetch has a backoff", "at": "src/net.rs:99" },
            { "claim": "a claim about nothing", "at": "https://example.com/x" },
        ],
        "changed": "src/net.rs",
        "ran": [{ "cmd": "cargo test", "exit": 0 }],
        "sure": "mid",
        "open": ["is the timeout set"],
    })
}

const GOAL: &str = "Find where the retry of fetch is done";

#[test]
fn a_goal_that_ran_before_gets_its_checked_claims_as_an_old_reports_and_nothing_else_of_it() {
    let d = fixture();
    let log = log_in(&d.0);
    made(&log, &d, GOAL, answer(), DataClass::Project);
    let text = recall(&d, &log, Some(true)).text(GOAL).expect("a hint");
    assert!(text.starts_with("Claims of an earlier report of this same task"), "{text}");
    assert!(text.lines().next().unwrap().contains("not facts"), "{text}");
    assert_eq!(text.lines().skip(1).collect::<Vec<_>>(), ["- the retry is in fetch() @ src/net.rs:2", "- read_all is the only reader @ src/io.rs:1"]);
    // what changes with every task, and what was not checked, is not in it
    for left_out in ["the retry lives", "cargo test", "src/net.rs\n", "is the timeout", "backoff", "example.com", "sure"] {
        assert!(!text.contains(left_out), "{left_out}: {text}");
    }
    // the same words in other letters and spacing are the same goal; other words are another
    assert!(recall(&d, &log, Some(true)).text("  find WHERE the retry\tof fetch is done ").is_some());
    assert_eq!(recall(&d, &log, Some(true)).text("Find where the retry of fetch is not done"), None);
    // the switch: off, or never set, gives nothing
    assert_eq!(recall(&d, &log, Some(false)).text(GOAL), None);
    assert_eq!(recall(&d, &log, None).text(GOAL), None);
    // a report of a log that is not shown to a model is not shown
    let other = fixture();
    let log = log_in(&other.0);
    made(&log, &other, GOAL, answer(), DataClass::Private);
    assert_eq!(recall(&other, &log, Some(true)).text(GOAL), None);
}

#[test]
fn a_claim_whose_file_changed_or_went_is_dropped_and_comes_back_with_the_file() {
    let d = fixture();
    let log = log_in(&d.0);
    made(&log, &d, GOAL, answer(), DataClass::Project);
    let r = recall(&d, &log, Some(true));
    assert_eq!(r.claims(GOAL).len(), 2);
    d.put("src/io.rs", "pub fn read_all() {}\npub fn read_some() {}\n");
    assert_eq!(r.claims(GOAL), ["- the retry is in fetch() @ src/net.rs:2"], "only the claim of the file that changed goes");
    std::fs::remove_file(d.0.join("src/net.rs")).unwrap();
    assert_eq!(r.claims(GOAL), Vec::<String>::new(), "no claim holds: no hint");
    assert_eq!(r.text(GOAL), None);
    d.put("src/net.rs", "fn main() {}\nfn fetch() { retry(3) }\n");
    assert_eq!(r.claims(GOAL).len(), 1, "a file that holds what it held is the same file");
    // a later report of the goal replaces the old one; one whose claims are all stale falls back to an older one that still holds
    d.put("src/io.rs", "pub fn read_all() {}\n");
    made(&log, &d, GOAL, json!({ "answer": "x", "found": [{ "claim": "read_all reads", "at": "src/io.rs:1" }], "sure": "high" }), DataClass::Project);
    assert_eq!(r.claims(GOAL), ["- read_all reads @ src/io.rs:1"]);
    d.put("src/io.rs", "changed\n");
    assert_eq!(r.claims(GOAL), ["- the retry is in fetch() @ src/net.rs:2"]);
}

#[test]
fn the_hint_is_cut_whole_claims_at_a_time_to_its_bound_and_its_section_fits() {
    let d = Dir::new();
    let found: Vec<_> = (0..24).map(|n| json!({ "claim": format!("claim number {n} says a long thing about the module {}", "x".repeat(200)), "at": format!("src/f{n}.rs:1") })).collect();
    for n in 0..24 {
        d.put(&format!("src/f{n}.rs"), &format!("// claim number {n} module\n"));
    }
    let log = log_in(&d.0);
    made(&log, &d, GOAL, json!({ "answer": "y", "found": found, "sure": "low" }), DataClass::Project);
    let text = recall(&d, &log, Some(true)).text(GOAL).unwrap();
    let section = Brief::new().with("remembered", DataClass::Project, &text).render(8000);
    assert_eq!(section.sections, ["remembered"]);
    assert!(section.tokens <= RECALL_TOKENS + 3 && tokens(&text) + 3 <= RECALL_TOKENS, "{} tokens: {text}", section.tokens);
    let n = text.lines().skip(1).count();
    assert!((1..=6).contains(&n), "{n} claims");
    assert!(text.lines().skip(1).all(|l| l.chars().count() < 160 && l.contains('…')), "a claim is cut to a line: {text}");
    // whole claims: the ones shown are the first ones, in order
    assert!(text.lines().nth(1).unwrap().contains("claim number 0"), "{text}");
}

#[test]
fn a_log_with_no_report_of_the_goal_or_a_report_without_the_hashes_gives_nothing() {
    let d = fixture();
    let log = log_in(&d.0);
    // a report written by a build that did not keep the goal and the hashes cannot be shown: nothing says its files are as they were
    let mut report = Report::take(&answer().to_string()).unwrap();
    report.check(&d.0, &[]);
    log.append(New::of("report.made", Actor::Core, json!({ "report": report.json(), "deliver": true })).thread("main").task("t2")).unwrap();
    let r = recall(&d, &log, Some(true));
    assert_eq!(r.text(GOAL), None);
    // and no claim is kept for a file it has no hash of
    log.append(New::of("report.made", Actor::Core, json!({ "report": report.json(), "deliver": true, "goal": goal_key(GOAL), "files": { "src/io.rs": "0000000000000000" } })).thread("main").task("t3")).unwrap();
    assert_eq!(r.claims(GOAL), Vec::<String>::new(), "a hash that is not the file's is a stale claim");
    // the index is made again from the Log: a deleted copy gives the same hint
    made(&log, &d, GOAL, answer(), DataClass::Project);
    assert_eq!(recall(&d, &log, Some(true)).claims(GOAL).len(), 2);
}
