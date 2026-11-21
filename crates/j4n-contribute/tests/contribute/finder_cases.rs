//! The finder: what it writes, how often, to whom it says it, and the fixes it carries and retires.

use crate::bed::{Bed, bed};
use j4n_contribute::Finder;
use j4n_contribute::found::Finding;
use j4n_core::change::Actor;
use j4n_core::front::Contribute;
use j4n_core::log::New;
use j4n_core::proposal::{Accepted, Draft, Step, StepOp};
use j4n_core::schema::DataClass;
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn floor(text: &str) -> Finding {
    Finding { source: "floor", id: "turn-cap".into(), row: "model".into(), text: text.into(), title: text.into(), cause: 7 }
}

/// Wait for `n` events of `kind`.
fn waited(b: &Bed, kind: &str, n: usize) -> Vec<serde_json::Value> {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let got = b.events(kind);
        if got.len() >= n || Instant::now() > end {
            return got;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[test]
fn a_finding_is_written_with_its_fingerprint_and_count_as_a_private_event_that_says_where_it_came_from() {
    let b = bed("write");
    let f = b.finder("", None);
    let fp = f.record(&floor("the model went 12 turns without finishing"), None).expect("written");
    assert_eq!(fp.len(), 12);
    let got = b.events("contribute.found");
    assert_eq!(got.len(), 1);
    let g = &got[0];
    assert_eq!((g["key"].as_str(), g["fp"].as_str(), g["source"].as_str(), g["what"].as_str(), g["row"].as_str(), g["n"].as_u64(), g["state"].as_str(), g["mark"].as_str()), (Some(fp.as_str()), Some(fp.as_str()), Some("floor"), Some("turn-cap"), Some("model"), Some(1), Some("kept here"), Some("▲")), "{g}");
    assert_eq!(g["at_ms"].as_u64(), Some(1_000_000_000));
    assert!(g.get("words").is_none(), "only the person's own words have that field: {g}");
    let ev = b.log.read(0).find(|e| e.kind == "contribute.found").unwrap();
    assert_eq!((ev.class, ev.cause), (DataClass::Private, Some(7)), "what is found here is the person's, and says what caused it");
    // the count is a fold of the log, by fingerprint
    let rows = b.folds.read("contribute_found", &[j4n_core::value::Key::Name(fp.clone())], 0);
    assert!(matches!(rows, j4n_core::value::Value::Map(m) if m.get("n") == Some(&j4n_core::value::Value::Num(1.0))));
}

#[test]
fn the_same_finding_again_is_counted_and_written_at_most_every_half_minute_with_the_count_it_missed() {
    let b = bed("gap");
    let f = b.finder("", None);
    let same = |n: u32| floor(&format!("the model went {n} turns without finishing"));
    assert!(f.record(&same(12), None).is_some());
    b.at(1_000_000_000 + 29_999);
    assert!(f.record(&same(13), None).is_none() && f.record(&same(14), None).is_none(), "inside the half minute: counted, not written");
    assert_eq!(b.events("contribute.found").len(), 1);
    b.at(1_000_000_000 + 30_000);
    assert!(f.record(&same(15), None).is_some(), "at the half minute it is written");
    assert_eq!(b.events("contribute.found").iter().map(|g| g["n"].as_u64().unwrap()).collect::<Vec<_>>(), [1, 4], "with the two it missed");
    // a restart: the count goes on from the log
    let again = b.finder("", None);
    b.at(1_000_000_000 + 90_000);
    again.record(&same(16), None);
    assert_eq!(b.events("contribute.found").last().unwrap()["n"].as_u64(), Some(5));
    // a different bug is its own count
    f.record(&Finding { id: "same-reply".into(), ..same(1) }, None);
    assert_eq!(b.events("contribute.found").last().unwrap()["n"].as_u64(), Some(1));
}

#[test]
fn the_finder_reads_the_log_on_its_own_and_writes_what_is_j4ns_and_nothing_that_is_the_providers() {
    let b = bed("read");
    let f = b.finder("", None);
    Finder::start(&f);
    let put = |kind: &str, by: Actor, data: serde_json::Value| {
        b.log.append(New::of(kind, by, data)).unwrap();
    };
    put("run.error", Actor::Doer("claude".into()), json!({ "text": "429 rate limit reached", "class": "rate-limit", "row": "limit" }));
    put("run.error", Actor::Doer("claude".into()), json!({ "text": "not logged in", "class": "permanent", "row": "gone" }));
    put("run.error", Actor::Doer("claude".into()), json!({ "text": "529 overloaded", "class": "transient", "row": null }));
    put("run.unread", Actor::Doer("claude".into()), json!({ "dialect": "claude-stream", "kind": "mystery_frame" }));
    // a frame of its own after them: once its finding is written, the four before it have been read
    put("run.unread", Actor::Doer("claude".into()), json!({ "dialect": "claude-stream", "kind": "last_frame" }));
    let got = waited(&b, "contribute.found", 2);
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(got[1]["title"].as_str(), Some("claude-stream: a frame of kind \"last_frame\" cannot be read"));
    assert_eq!((got[0]["source"].as_str(), got[0]["row"].as_str(), got[0]["title"].as_str()), (Some("dialect"), Some("claude-stream"), Some("claude-stream: a frame of kind \"mystery_frame\" cannot be read")));
    // the same event seen by another version of the tool: the same fingerprint, and nothing in it names a version
    let again = j4n_contribute::fingerprint::fingerprint("dialect", "unread:mystery_frame", "claude-stream", "");
    assert_eq!(got[0]["fp"].as_str(), Some(again.as_str()));
}

#[test]
fn with_the_mode_off_nothing_is_found_and_nothing_is_kept() {
    let b = bed("off");
    let f = b.finder("", Some("off"));
    assert!(!f.on());
    Finder::start(&f);
    b.log.append(New::of("run.unread", Actor::Doer("claude".into()), json!({ "dialect": "claude-stream", "kind": "mystery_frame" }))).unwrap();
    std::thread::sleep(Duration::from_millis(600));
    assert!(b.events("contribute.found").is_empty());
    assert!(f.wrong("main", "this is wrong").is_err_and(|e| e.contains("off")));
    for mode in ["Off", "no", "false", "typo", ""] {
        assert!(!bed("closed").finder("", Some(mode)).on(), "{mode:?}: anything that is not ask or auto is off");
    }
    for mode in ["ask", "auto", "AUTO", " ask "] {
        assert!(bed("on").finder("", Some(mode)).on(), "{mode}");
    }
    assert!(bed("default").finder("", None).on(), "the default is ask");
}

#[test]
fn the_persons_word_is_kept_as_a_finding_whose_words_stay_in_a_field_of_their_own() {
    let b = bed("word");
    let f = b.finder("", None);
    let said = f.wrong("main", "  the lane was picked wrong in /home/ana/secret-project  ").unwrap();
    assert!(said.starts_with("kept as a finding (") && said.contains("stay on this machine"), "{said}");
    let g = &b.events("contribute.found")[0];
    assert_eq!((g["source"].as_str(), g["what"].as_str(), g["n"].as_u64(), g["mark"].as_str()), (Some("word"), Some("wrong"), Some(1), Some("¶")), "{g}");
    assert_eq!(g["words"].as_str(), Some("the lane was picked wrong in /home/ana/secret-project"));
    assert_eq!(g["title"].as_str(), Some("“the lane was picked wrong in /home/ana/secret-project” (your words)"));
    let ev = b.log.read(0).find(|e| e.kind == "contribute.found").unwrap();
    assert_eq!(ev.class, DataClass::Private);
    // the same words are the same finding, and the fingerprint is of the words with paths taken out
    let a = f.wrong("main", "the lane was picked wrong in /tmp/other").unwrap();
    assert!(said.contains(a.split('(').nth(1).unwrap().split(')').next().unwrap()), "{said} / {a}");
    assert!(f.wrong("main", "   ").is_err_and(|e| e.contains(":wrong")), "no words, no finding");
    let long = "x ".repeat(1000);
    f.wrong("main", &long).unwrap();
    assert!(b.events("contribute.found").iter().all(|g| g["words"].as_str().is_none_or(|w| w.chars().count() <= 501)), "words are clipped");
}

#[test]
fn the_persons_words_are_always_written_even_when_they_make_the_same_fingerprint_inside_the_half_minute() {
    let b = bed("words-gap");
    let f = b.finder("", None);
    f.wrong("main", "the build took 3 minutes").unwrap();
    f.wrong("main", "the build took 9 minutes").unwrap();
    let got = b.events("contribute.found");
    assert_eq!(got.iter().map(|g| g["words"].as_str().unwrap()).collect::<Vec<_>>(), ["the build took 3 minutes", "the build took 9 minutes"]);
    assert_eq!(got.iter().map(|g| g["n"].as_u64().unwrap()).collect::<Vec<_>>(), [1, 2]);
}

#[test]
fn a_count_that_went_on_inside_the_half_minute_is_written_when_it_passes_and_when_the_finder_is_let_go() {
    let b = bed("flush");
    let same = |n: u32| floor(&format!("the model went {n} turns without finishing"));
    {
        let f = b.finder("", None);
        f.record(&same(1), None);
        f.record(&same(2), None);
        f.record(&same(3), None);
        assert_eq!(b.events("contribute.found").len(), 1, "inside the half minute");
    }
    let got = b.events("contribute.found");
    assert_eq!(got.iter().map(|g| g["n"].as_u64().unwrap()).collect::<Vec<_>>(), [1, 3], "letting go writes what was counted");
    assert_eq!(got[1]["fp"], got[0]["fp"]);
}

const ROWS: &str = "[[setting]]\nid = \"x.y\"\ndoc = \"a number\"\nty = \"int\"\ndefault = 1\n";

fn fix(key: &str, to: i64) -> Draft {
    let mut fields = toml::Table::new();
    fields.insert("default".into(), toml::Value::Integer(to));
    Draft { key: key.into(), title: format!("x.y is {to}"), why: "tested".into(), evidence: 3, steps: vec![Step { to: j4n_core::change::Where::User, file: Some("fix-x.toml".into()), kind: "setting".into(), id: "x.y".into(), op: StepOp::Set(fields) }], widens: Vec::new() }
}

fn accepted(b: &Bed, d: Draft) {
    let p = b.proposer(ROWS);
    let id = p.make(d, Actor::Core, None).unwrap().expect("proposed");
    assert!(matches!(p.accept(&id).unwrap(), Accepted::Done(_)));
    p.log_sync();
}

#[test]
fn an_accepted_fix_is_seen_once_as_carried_and_retires_once_when_the_pack_says_what_it_said_after_an_update() {
    let b = bed("patch");
    let first = b.finder(ROWS, None);
    first.sweep();
    assert!(b.events("contribute.patch").is_empty(), "nothing accepted, nothing carried");
    accepted(&b, fix("fix:abc", 2));
    accepted(&b, Draft { key: "other:abc".into(), ..fix("x", 3) });
    first.sweep();
    first.sweep();
    let got = b.events("contribute.patch");
    assert_eq!(got.len(), 1, "once, and only a proposal with the key of a fix: {got:?}");
    assert_eq!((got[0]["key"].as_str(), got[0]["state"].as_str(), got[0]["title"].as_str(), got[0]["mark"].as_str()), (Some("fix:abc"), Some("carried"), Some("x.y is 2"), Some("⚑")));
    // in this run the rows loaded do not hold it yet: it is not judged until the next start
    first.sweep();
    assert_eq!(b.events("contribute.patch").len(), 1);
    // the next start, the pack still says 1: the fix is still carried, and nothing is written
    b.at(2_000_000_000);
    let second = b.finder(ROWS, None);
    second.sweep();
    assert_eq!(b.events("contribute.patch").len(), 1, "still carried: said once");
    // the next start after an update: the pack now says 2
    b.at(3_000_000_000);
    let updated = b.finder(&ROWS.replace("default = 1", "default = 2"), None);
    updated.sweep();
    updated.sweep();
    let got = b.events("contribute.patch");
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!((got[1]["state"].as_str(), got[1]["how"].as_str(), got[1]["mark"].as_str()), (Some("retired"), Some("this version already says it"), Some("✓")));
    // a retired fix is not looked at again
    b.at(4_000_000_000);
    b.finder(&ROWS.replace("default = 1", "default = 2"), None).sweep();
    b.finder(ROWS, None).sweep();
    assert_eq!(b.events("contribute.patch").len(), 2);
}

#[test]
fn an_undone_fix_is_withdrawn_once_and_the_same_key_proposed_again_is_a_new_fix_not_a_withdrawn_one() {
    let b = bed("undo");
    let p = b.proposer(ROWS);
    let id = p.make(fix("fix:u", 2), Actor::Core, None).unwrap().unwrap();
    p.accept(&id).unwrap();
    p.log_sync();
    let f = b.finder(ROWS, None);
    f.sweep();
    p.undo(&id).unwrap();
    p.log_sync();
    f.sweep();
    f.sweep();
    let got = b.events("contribute.patch");
    assert_eq!(got.iter().map(|g| g["state"].as_str().unwrap()).collect::<Vec<_>>(), ["carried", "withdrawn"]);
    assert_eq!(got[0]["proposal"].as_str(), Some(id.as_str()));
    // proposed again with more behind it, accepted in a later run: carried again, not judged against the tables of an earlier start
    b.at(2_000_000_000);
    let id2 = p.make(Draft { evidence: 9, ..fix("fix:u", 2) }, Actor::Core, None).unwrap().unwrap();
    p.accept(&id2).unwrap();
    p.log_sync();
    let later = b.finder(ROWS, None);
    later.sweep();
    later.sweep();
    let got = b.events("contribute.patch");
    assert_eq!(got.iter().map(|g| g["state"].as_str().unwrap()).collect::<Vec<_>>(), ["carried", "withdrawn", "carried"]);
    assert_eq!(got[2]["proposal"].as_str(), Some(id2.as_str()));
}

#[test]
fn a_fix_the_operators_folder_no_longer_holds_is_withdrawn_once() {
    let b = bed("withdrawn");
    accepted(&b, fix("fix:gone", 2));
    b.finder(ROWS, None).sweep();
    std::fs::remove_file(b.dir.0.join("home/fix-x.toml")).unwrap();
    b.at(2_000_000_000);
    let next = b.finder(ROWS, None);
    next.sweep();
    next.sweep();
    let got = b.events("contribute.patch");
    assert_eq!(got.iter().map(|g| g["state"].as_str().unwrap()).collect::<Vec<_>>(), ["carried", "withdrawn"]);
    assert_eq!(got[1]["mark"].as_str(), Some("○"));
}

fn two(y: i64, z: i64) -> String {
    format!("[[setting]]\nid = \"x.y\"\ndoc = \"a number\"\nty = \"int\"\ndefault = {y}\n[[setting]]\nid = \"x.z\"\ndoc = \"another\"\nty = \"int\"\ndefault = {z}\n")
}

#[test]
fn a_fix_that_sets_several_rows_retires_only_when_the_pack_has_all_of_them() {
    let b = bed("several");
    let mut d = fix("fix:two", 2);
    let mut second = toml::Table::new();
    second.insert("default".into(), toml::Value::Integer(5));
    d.steps.push(Step { to: j4n_core::change::Where::User, file: Some("fix-z.toml".into()), kind: "setting".into(), id: "x.z".into(), op: StepOp::Set(second) });
    let p = b.proposer(&two(1, 1));
    let id = p.make(d, Actor::Core, None).unwrap().unwrap();
    p.accept(&id).unwrap();
    p.log_sync();
    b.finder(&two(1, 1), None).sweep();
    let states = |b: &Bed| b.events("contribute.patch").iter().map(|g| g["state"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    for (at, rows) in [(2, two(2, 1)), (3, two(1, 5)), (4, two(2, 2))] {
        b.at(at * 1_000_000_000);
        b.finder(&rows, None).sweep();
        assert_eq!(states(&b), ["carried"], "{rows}");
    }
    b.at(5_000_000_000);
    b.finder(&two(2, 5), None).sweep();
    assert_eq!(states(&b), ["carried", "retired"]);
}

#[test]
fn a_count_inside_the_half_minute_is_written_by_the_running_finder_once_the_gap_has_passed_and_the_gap_starts_again() {
    let b = bed("tick");
    let f = b.finder("", None);
    Finder::start(&f);
    let same = |n: u32| floor(&format!("the model went {n} turns without finishing"));
    for n in 1..=3 {
        f.record(&same(n), None);
    }
    assert_eq!(b.events("contribute.found").len(), 1);
    b.at(1_000_000_000 + 31_000);
    let got = waited(&b, "contribute.found", 2);
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!((got[1]["n"].as_u64(), got[1]["at_ms"].as_u64()), (Some(3), Some(1_000_031_000)), "the count and the time of the flush");
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(b.events("contribute.found").len(), 2, "written once, not at every tick");
    b.at(1_000_035_000);
    f.record(&same(4), None);
    assert_eq!(b.events("contribute.found").len(), 2, "the gap starts again at the flush");
    b.at(1_000_080_000);
    assert_eq!(waited(&b, "contribute.found", 3).len(), 3, "the fourth hit is written with the next flush, and only that one");
    b.at(1_000_200_000);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(b.events("contribute.found").len(), 3, "what was written is not written again");
}

#[test]
fn the_running_finder_reads_what_the_engine_stopped_and_does_nothing_with_the_mode_off() {
    let b = bed("stopped");
    let f = b.finder("", None);
    Finder::start(&f);
    let stop = |msg: &str| {
        let mut n = New::of("task.stopped", Actor::Core, json!({ "why": { "kind": "failed", "message": msg } }));
        n.class = Some(DataClass::Private);
        b.log.append(n).unwrap();
    };
    stop("the doer stopped without a report");
    assert_eq!(waited(&b, "contribute.found", 1).len(), 1);
    let off = bed("stopped-off");
    let g = off.finder(ROWS, Some("off"));
    accepted(&off, fix("fix:off", 2));
    Finder::start(&g);
    std::thread::sleep(Duration::from_millis(700));
    assert!(off.events("contribute.patch").is_empty(), "off does not look at fixes");
    drop(g);
}

#[test]
fn the_mode_is_read_without_regard_to_blanks_and_case_and_the_persons_words_have_no_cause() {
    let b = bed("mode");
    assert!(b.finder("", Some(" AUTO ")).on());
    assert!(b.finder("", Some("Ask")).on());
    assert!(!b.finder("", Some(" Off")).on());
    let f = b.finder("", None);
    f.wrong("t", "it hung").unwrap();
    b.log.sync();
    let ev = b.log.read(0).find(|e| e.kind == "contribute.found").unwrap();
    assert_eq!(ev.cause, None);
    f.wrong("t", &"w".repeat(300)).unwrap();
    let long = b.events("contribute.found").pop().unwrap();
    assert!(long["title"].as_str().unwrap().chars().count() < 120, "a title is a line");
    assert!(long["words"].as_str().unwrap().chars().count() <= 501);
}

#[test]
fn a_long_title_of_a_fix_is_clipped_in_what_is_written() {
    let b = bed("long");
    let mut d = fix("fix:long", 2);
    d.title = "x".repeat(300);
    accepted(&b, d);
    b.finder(ROWS, None).sweep();
    assert!(b.events("contribute.patch")[0]["title"].as_str().unwrap().chars().count() <= 101);
}

#[test]
fn one_row_gone_of_two_is_still_carried() {
    // two rows, one of the files gone: carried, not withdrawn
    let b = bed("one-gone");
    let mut d = fix("fix:two", 2);
    let mut second = toml::Table::new();
    second.insert("default".into(), toml::Value::Integer(5));
    d.steps.push(Step { to: j4n_core::change::Where::User, file: Some("fix-z.toml".into()), kind: "setting".into(), id: "x.z".into(), op: StepOp::Set(second) });
    let p = b.proposer(&two(1, 1));
    let id = p.make(d, Actor::Core, None).unwrap().unwrap();
    p.accept(&id).unwrap();
    p.log_sync();
    b.finder(&two(1, 1), None).sweep();
    std::fs::remove_file(b.dir.0.join("home/fix-z.toml")).unwrap();
    b.at(2_000_000_000);
    b.finder(&two(1, 1), None).sweep();
    assert_eq!(b.events("contribute.patch").len(), 1, "one row gone of two says nothing");
}

#[test]
fn a_fix_of_two_fields_retires_only_when_the_pack_has_both() {
    let b = bed("fields");
    let mut d = fix("fix:f", 2);
    if let StepOp::Set(t) = &mut d.steps[0].op {
        t.insert("doc".into(), toml::Value::String("better".into()));
    }
    let p = b.proposer(ROWS);
    let id = p.make(d, Actor::Core, None).unwrap().unwrap();
    p.accept(&id).unwrap();
    p.log_sync();
    b.finder(ROWS, None).sweep();
    b.at(2_000_000_000);
    b.finder(&ROWS.replace("default = 1", "default = 2"), None).sweep();
    assert_eq!(b.events("contribute.patch").len(), 1, "one field of two is not retired");
    b.at(3_000_000_000);
    b.finder(&ROWS.replace("default = 1", "default = 2").replace("a number", "better"), None).sweep();
    assert_eq!(b.events("contribute.patch").len(), 2);
}

#[allow(dead_code)]
fn held(_: Arc<Finder>) {}
