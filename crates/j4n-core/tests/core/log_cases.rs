//! The Log: what is written, in what order, how it is read back, and what is refused.

use crate::world::*;
use j4n_core::change::Actor;
use j4n_core::clock::ManualClock;
use j4n_core::log::{Log, New, Opts, Scope, Sink};
use j4n_core::redact::Redactor;
use j4n_core::schema::DataClass;
use serde_json::json;
use std::fs;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[test]
fn an_event_is_numbered_timed_and_in_the_file_when_append_returns() {
    let s = Scratch::new("log-basic");
    let (log, clock) = open_log(&s);
    let a = log.append(ev("task.started", json!({"n": 1})).task("t1").thread("th")).unwrap();
    clock.advance(5);
    let b = log.append(New::of("task.settled", Actor::Rule("r".into()), json!({"ok": true})).cause(a.seq)).unwrap();
    assert_eq!((a.seq, a.at_ms, b.seq, b.at_ms), (1, 1_000, 2, 1_005));
    assert_eq!(log.head(), 2);
    let read = all(&log);
    assert_eq!(read, vec![a.clone(), b.clone()]);
    assert_eq!(read[0].task.as_deref(), Some("t1"));
    assert_eq!(read[1].cause, Some(1));
    assert_eq!(read[1].by, Actor::Rule("r".into()));
    assert_eq!(read[0].class, DataClass::Project);
    // The file holds exactly those two lines.
    let text = s.read(".j4n/log/0001.jsonl");
    assert_eq!(text.lines().count(), 2);
    assert!(text.ends_with('\n'));
}

#[test]
fn time_never_goes_backwards_even_when_the_clock_does() {
    let s = Scratch::new("log-time");
    let (log, clock) = open_log(&s);
    let a = log.append(ev("x.y", json!(null))).unwrap();
    clock.set(10);
    let b = log.append(ev("x.y", json!(null))).unwrap();
    assert_eq!((a.at_ms, b.at_ms), (1_000, 1_000));
}

#[test]
fn numbering_goes_on_after_the_log_is_closed_and_opened_again() {
    let s = Scratch::new("log-reopen");
    {
        let (log, _) = open_log(&s);
        log.append(ev("a.b", json!(1))).unwrap();
        log.append(ev("a.b", json!(2))).unwrap();
    }
    let (log, _) = open_log(&s);
    assert_eq!(log.head(), 2);
    let c = log.append(ev("a.b", json!(3))).unwrap();
    assert_eq!(c.seq, 3);
    assert_eq!(all(&log).iter().map(|e| e.seq).collect::<Vec<_>>(), [1, 2, 3]);
}

#[test]
fn a_second_open_of_the_same_log_is_refused_and_says_why() {
    let s = Scratch::new("log-lock");
    let (_log, clock) = open_log(&s);
    let again = Log::open(&s.join(".j4n"), Scope::Project, opts(&clock), Redactor::new());
    let err = again.err().expect("refused").to_string();
    assert!(err.contains("another J4n has this log open"), "{err}");
}

#[test]
fn the_lock_is_let_go_when_the_last_handle_is_dropped() {
    let s = Scratch::new("log-unlock");
    let (log, clock) = open_log(&s);
    let clone = log.clone();
    drop(log);
    assert!(Log::open(&s.join(".j4n"), Scope::Project, opts(&clock), Redactor::new()).is_err(), "a clone still holds it");
    drop(clone);
    assert!(Log::open(&s.join(".j4n"), Scope::Project, opts(&clock), Redactor::new()).is_ok());
}

#[test]
fn segments_rotate_by_size_and_read_as_one_log() {
    let s = Scratch::new("log-rotate");
    let clock = Arc::new(ManualClock::at(1));
    let o = Opts { segment_bytes: 300, ..opts(&clock) };
    let (log, _) = Log::open(&s.join(".j4n"), Scope::Project, o, Redactor::new()).unwrap();
    for i in 0..30 {
        log.append(ev("a.b", json!({"i": i, "pad": "xxxxxxxxxxxxxxxxxxxx"}))).unwrap();
    }
    let names: Vec<String> = fs::read_dir(s.join(".j4n/log"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".jsonl"))
        .collect();
    assert!(names.len() >= 5, "{names:?}");
    assert!(names.contains(&"0001.jsonl".to_string()) && names.contains(&"0002.jsonl".to_string()));
    for n in &names {
        assert!(s.read(&format!(".j4n/log/{n}")).len() <= 300 + 120, "{n} is a segment, not a pile");
    }
    let read = all(&log);
    assert_eq!(read.iter().map(|e| e.seq).collect::<Vec<_>>(), (1..=30).collect::<Vec<_>>());
    assert_eq!(log.read(17).next().map(|e| e.seq), Some(17), "reading from a number starts there");
    assert_eq!(log.read(31).count(), 0);
    assert_eq!(log.read(30).count(), 1);
    drop(log);
    let (log, _) = open_log(&s);
    assert_eq!(log.head(), 30);
    assert_eq!(log.append(ev("a.b", json!(0))).unwrap().seq, 31, "the next number follows the last segment's");
}

#[test]
fn a_segment_is_never_empty_until_it_is_the_last_and_one_event_larger_than_a_segment_still_fits() {
    let s = Scratch::new("log-bigger");
    let clock = Arc::new(ManualClock::at(1));
    let o = Opts { segment_bytes: 50, ..opts(&clock) };
    let (log, _) = Log::open(&s.join(".j4n"), Scope::Project, o, Redactor::new()).unwrap();
    for _ in 0..4 {
        log.append(ev("a.b", json!({"pad": "x".repeat(200)}))).unwrap();
    }
    for i in 1..=4 {
        assert_eq!(s.read(&format!(".j4n/log/{i:04}.jsonl")).lines().count(), 1);
    }
}

#[test]
fn many_threads_appending_give_one_unbroken_order() {
    let s = Scratch::new("log-threads");
    let (log, _) = open_log(&s);
    let handles: Vec<_> = (0..8)
        .map(|t| {
            let log = log.clone();
            std::thread::spawn(move || (0..50).map(|i| log.append(ev("a.b", json!({"t": t, "i": i}))).unwrap().seq).collect::<Vec<_>>())
        })
        .collect();
    let mut seen: Vec<u64> = handles.into_iter().flat_map(|h| h.join().unwrap()).collect();
    seen.sort();
    assert_eq!(seen, (1..=400).collect::<Vec<_>>());
    let read = all(&log);
    assert_eq!(read.len(), 400);
    for t in 0..8 {
        let mine: Vec<i64> = read.iter().filter(|e| e.data["t"] == t).map(|e| e.data["i"].as_i64().unwrap()).collect();
        assert_eq!(mine, (0..50).collect::<Vec<_>>(), "one thread's events keep their order");
    }
}

#[test]
fn a_kind_is_lowercase_words_joined_by_dots() {
    let s = Scratch::new("log-kinds");
    let (log, _) = open_log(&s);
    for bad in ["", "Run.message", "run..message", ".run", "run.", "run message", "1run", &"a".repeat(65), "run/x"] {
        assert!(log.append(ev(bad, json!(null))).is_err(), "{bad:?}");
    }
    for good in ["run.message", "task", "shape.result_2", &"a".repeat(64)] {
        assert!(log.append(ev(good, json!(null))).is_ok(), "{good:?}");
    }
}

#[test]
fn a_flash_reaches_subscribers_and_is_never_written() {
    let s = Scratch::new("log-flash");
    let (log, _) = open_log(&s);
    let sub = log.subscribe("run.*", 16);
    let other = log.subscribe("task.*", 16);
    let f = log.flash(ev("run.delta", json!({"text": "he"}))).unwrap();
    assert_eq!(f.seq, 0);
    assert!(f.is_flash());
    let got = sub.try_recv().expect("delivered");
    assert_eq!(got.kind, "run.delta");
    assert!(other.try_recv().is_none(), "the glob decides");
    assert_eq!(log.head(), 0);
    assert!(all(&log).is_empty());
    assert!(fs::read_to_string(s.join(".j4n/log/0001.jsonl")).unwrap().is_empty());
}

#[test]
fn a_written_event_reaches_subscribers_with_its_number() {
    let s = Scratch::new("log-sub");
    let (log, _) = open_log(&s);
    let sub = log.subscribe("*", 16);
    let e = log.append(ev("task.started", json!(1))).unwrap();
    let got = sub.recv_timeout(Duration::from_secs(2)).expect("delivered");
    assert_eq!(*got, e);
}

#[test]
fn a_slow_subscriber_loses_events_and_counts_them_without_slowing_the_log() {
    let s = Scratch::new("log-lag");
    let (log, _) = open_log(&s);
    let sub = log.subscribe("*", 2);
    for i in 0..10 {
        log.append(ev("a.b", json!(i))).unwrap();
    }
    assert_eq!(sub.lagged(), 8);
    assert_eq!(sub.try_recv().map(|e| e.seq), Some(1));
    assert_eq!(sub.try_recv().map(|e| e.seq), Some(2));
    assert!(sub.try_recv().is_none());
    assert_eq!(all(&log).len(), 10, "the log has them all");
}

#[test]
fn a_dropped_subscriber_is_forgotten() {
    let s = Scratch::new("log-gone");
    let (log, _) = open_log(&s);
    let sub = log.subscribe("*", 4);
    drop(sub);
    assert_eq!(log.subscribers(), 1, "forgotten at the next event");
    for _ in 0..3 {
        log.append(ev("a.b", json!(1))).unwrap();
    }
    assert_eq!(log.subscribers(), 0, "and it was");
    let kept = log.subscribe("*", 4);
    assert_eq!(log.subscribers(), 1);
    log.append(ev("a.b", json!(1))).unwrap();
    assert_eq!(log.subscribers(), 1, "the one that is listening stays");
    assert!(kept.try_recv().is_some());
}

#[test]
fn deltas_are_flashed_never_written_and_settled_things_are_written_never_flashed() {
    let s = Scratch::new("log-split");
    let (log, _) = open_log(&s);
    for k in j4n_core::log::FLASH_KINDS {
        assert!(log.append(ev(k, json!({}))).is_err(), "{k} is a flash");
        assert!(log.flash(ev(k, json!({}))).is_ok());
    }
    for k in j4n_core::log::SETTLED_KINDS {
        assert!(log.flash(ev(k, json!({}))).is_err(), "{k} is settled");
        assert!(log.append(ev(k, json!({}))).is_ok());
    }
    assert_eq!(log.head(), j4n_core::log::SETTLED_KINDS.len() as u64);
}

struct Tap(Arc<Mutex<Vec<String>>>);

impl Sink for Tap {
    fn event(&mut self, ev: &j4n_core::log::Ev) {
        self.0.lock().unwrap().push(format!("e{}", ev.seq));
    }

    fn checkpoint(&mut self, seq: u64) {
        self.0.lock().unwrap().push(format!("c{seq}"));
    }
}

#[test]
fn a_sink_gets_the_past_then_the_future_with_none_missed_and_none_twice() {
    let s = Scratch::new("log-attach");
    let (log, _) = open_log(&s);
    for i in 0..5 {
        log.append(ev("a.b", json!(i))).unwrap();
    }
    let seen = Arc::new(Mutex::new(Vec::new()));
    log.attach(Box::new(Tap(seen.clone())), 2).unwrap();
    for i in 0..3 {
        log.append(ev("a.b", json!(i))).unwrap();
    }
    assert_eq!(*seen.lock().unwrap(), ["e3", "e4", "e5", "e6", "e7", "e8"]);
}

#[test]
fn a_sink_attached_while_others_append_misses_nothing() {
    let s = Scratch::new("log-attach-race");
    let (log, _) = open_log(&s);
    let writer = {
        let log = log.clone();
        std::thread::spawn(move || (0..300).for_each(|i| drop(log.append(ev("a.b", json!(i))).unwrap())))
    };
    std::thread::sleep(Duration::from_millis(2));
    let seen = Arc::new(Mutex::new(Vec::new()));
    log.attach(Box::new(Tap(seen.clone())), 0).unwrap();
    writer.join().unwrap();
    let got: Vec<String> = seen.lock().unwrap().clone();
    assert_eq!(got, (1..=300).map(|i| format!("e{i}")).collect::<Vec<_>>());
}

#[test]
fn checkpoints_come_every_n_events_after_a_sync_and_at_close() {
    let s = Scratch::new("log-checkpoint");
    let clock = Arc::new(ManualClock::at(1));
    let o = Opts { checkpoint_every: 3, ..opts(&clock) };
    let (log, _) = Log::open(&s.join(".j4n"), Scope::Project, o, Redactor::new()).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    log.attach(Box::new(Tap(seen.clone())), 0).unwrap();
    for _ in 0..7 {
        log.append(ev("a.b", json!(1))).unwrap();
    }
    assert_eq!(*seen.lock().unwrap(), ["e1", "e2", "e3", "c3", "e4", "e5", "e6", "c6", "e7"]);
    log.sync();
    assert_eq!(seen.lock().unwrap().last().unwrap(), "c7");
    log.append(ev("a.b", json!(1))).unwrap();
    drop(log);
    assert_eq!(seen.lock().unwrap().last().unwrap(), "c8", "closing is a checkpoint");
}

struct Greedy(Log);

impl Sink for Greedy {
    fn event(&mut self, _: &j4n_core::log::Ev) {
        assert!(self.0.append(ev("x.y", json!(1))).is_err(), "refused, not waited for");
        assert!(self.0.flash(ev("x.y", json!(1))).is_err());
        assert!(self.0.attach(Box::new(Greedy(self.0.clone())), 0).is_err());
    }
}

#[test]
fn a_sink_may_not_call_back_into_the_log() {
    let s = Scratch::new("log-reenter");
    let (log, _) = open_log(&s);
    log.attach(Box::new(Greedy(log.clone())), 0).unwrap();
    log.append(ev("a.b", json!(1))).unwrap();
    assert!(log.notes().is_empty(), "the sink's own asserts held: {:?}", log.notes());
}

struct Bomb;

impl Sink for Bomb {
    fn event(&mut self, _: &j4n_core::log::Ev) {
        panic!("a sink that cannot cope");
    }
}

#[test]
fn a_sink_that_panics_is_dropped_and_the_log_goes_on() {
    let s = Scratch::new("log-bomb");
    let (log, _) = open_log(&s);
    let seen = Arc::new(Mutex::new(Vec::new()));
    log.attach(Box::new(Bomb), 0).unwrap();
    log.attach(Box::new(Tap(seen.clone())), 0).unwrap();
    assert!(log.append(ev("a.b", json!(1))).is_ok());
    assert!(log.append(ev("a.b", json!(1))).is_ok());
    assert_eq!(*seen.lock().unwrap(), ["e1", "e2"]);
    assert!(log.notes().iter().any(|n| n.msg.contains("panicked")), "{:?}", log.notes());
}

#[test]
fn a_big_event_becomes_a_blob_and_the_line_keeps_its_id() {
    let s = Scratch::new("log-spill");
    let clock = Arc::new(ManualClock::at(1));
    let o = Opts { max_line: 1000, ..opts(&clock) };
    let (log, _) = Log::open(&s.join(".j4n"), Scope::Project, o, Redactor::new()).unwrap();
    let big = "z".repeat(5000);
    let e = log.append(ev("run.shell", json!({"out": big}))).unwrap();
    let id = e.data["spilled"].as_str().expect("spilled").to_string();
    assert_eq!(e.data["bytes"], json!(serde_json::to_vec(&json!({"out": big})).unwrap().len()));
    let stored: serde_json::Value = serde_json::from_slice(&log.blobs().get(&id).unwrap()).unwrap();
    assert_eq!(stored, json!({"out": big}));
    assert!(s.read(".j4n/log/0001.jsonl").len() < 600, "the line stayed small");
    let small = log.append(ev("run.shell", json!({"out": "ok"}))).unwrap();
    assert_eq!(small.data, json!({"out": "ok"}));
}

#[test]
fn a_project_log_makes_its_gitignore_and_a_home_log_does_not() {
    let p = Scratch::new("log-ignore-p");
    let h = Scratch::new("log-ignore-h");
    let clock = Arc::new(ManualClock::at(1));
    drop(Log::open(&p.join(".j4n"), Scope::Project, opts(&clock), Redactor::new()).unwrap());
    drop(Log::open(&h.join(".j4n"), Scope::Home, opts(&clock), Redactor::new()).unwrap());
    assert_eq!(p.read(".j4n/.gitignore"), "*\n");
    assert!(!h.exists(".j4n/.gitignore"));
}

#[test]
fn a_directory_that_was_there_keeps_what_it_ignores() {
    let p = Scratch::new("log-ignore-kept");
    p.put(".j4n/.gitignore", "log/\n");
    let clock = Arc::new(ManualClock::at(1));
    drop(Log::open(&p.join(".j4n"), Scope::Project, opts(&clock), Redactor::new()).unwrap());
    assert_eq!(p.read(".j4n/.gitignore"), "log/\n");
}

#[test]
fn state_files_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new("log-modes");
    let (log, _) = open_log(&s);
    log.append(ev("a.b", json!(1))).unwrap();
    let id = log.blobs().put(b"hello").unwrap();
    let mode = |p: &str| fs::metadata(s.join(p)).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(".j4n/log"), 0o700);
    assert_eq!(mode(".j4n/log/0001.jsonl"), 0o600);
    assert_eq!(mode(&format!(".j4n/blobs/{}/{}", &id[..2], &id[2..])), 0o600);
}

#[test]
fn a_root_that_cannot_be_made_is_refused_with_a_reason() {
    let s = Scratch::new("log-noroot");
    s.put("file", "in the way");
    let clock = Arc::new(ManualClock::at(1));
    let e = Log::open(&s.join("file"), Scope::Project, opts(&clock), Redactor::new()).err().expect("refused").to_string();
    assert!(e.contains("cannot make the directory"), "{e}");
    s.put("root/log", "a file where the directory should be");
    let e = Log::open(&s.join("root"), Scope::Home, opts(&clock), Redactor::new()).err().expect("refused").to_string();
    assert!(e.contains("cannot make the directory"), "{e}");
}

#[test]
fn reading_a_directory_that_is_not_there_gives_nothing_and_says_so() {
    let s = Scratch::new("log-noread");
    let mut r = j4n_core::log::Reader::open(&s.join("nothing/here"), 0);
    assert_eq!(r.by_ref().count(), 0);
    let p = r.into_problems();
    assert_eq!(p.len(), 1);
    assert!(p[0].msg.contains("cannot list the segments"), "{}", p[0]);
}

#[test]
fn the_settled_and_the_flashed_kinds_are_the_ones_the_plan_names() {
    assert_eq!(j4n_core::log::SETTLED_KINDS, ["run.message", "run.reasoning", "run.tool_call", "run.tool_result", "run.shell"]);
    assert_eq!(j4n_core::log::FLASH_KINDS, ["run.delta", "run.reasoning_delta", "run.busy", "run.open", "run.tokens"]);
}

#[test]
fn a_segment_is_full_when_the_next_line_would_pass_its_size_and_not_before() {
    let s = Scratch::new("log-exact");
    let clock = Arc::new(ManualClock::at(1_000));
    // Lines of one length: measure one, then allow exactly two to a segment.
    let probe = Scratch::new("log-exact-probe");
    let (plog, _) = open_log(&probe);
    plog.append(ev("a.b", json!({"k": "same"}))).unwrap();
    plog.append(ev("a.b", json!({"k": "same"}))).unwrap();
    // the second line: it carries the link to the first, as every line but the very first does
    let one = probe.read(".j4n/log/0001.jsonl").lines().nth(1).unwrap().len() as u64 + 1;
    drop(plog);
    let o = Opts { segment_bytes: 2 * one, ..opts(&clock) };
    let (log, _) = Log::open(&s.join(".j4n"), Scope::Project, o, Redactor::new()).unwrap();
    for _ in 0..5 {
        log.append(ev("a.b", json!({"k": "same"}))).unwrap();
    }
    assert_eq!(s.read(".j4n/log/0001.jsonl").lines().count(), 2, "two fit exactly");
    assert_eq!(s.read(".j4n/log/0002.jsonl").lines().count(), 2);
    assert_eq!(s.read(".j4n/log/0003.jsonl").lines().count(), 1);
}

#[test]
fn an_event_is_kept_whole_up_to_the_line_limit_and_a_blob_beyond_it() {
    let s = Scratch::new("log-limit");
    let clock = Arc::new(ManualClock::at(1));
    let o = Opts { max_line: 100, ..opts(&clock) };
    let (log, _) = Log::open(&s.join(".j4n"), Scope::Project, o, Redactor::new()).unwrap();
    // The data `"xxxx…"` serializes to its text plus two quotes.
    let at = |n: usize| json!("x".repeat(n - 2));
    assert_eq!(log.append(ev("a.b", at(99))).unwrap().data, at(99));
    assert_eq!(log.append(ev("a.b", at(100))).unwrap().data, at(100), "exactly the limit stays in the line");
    assert!(log.append(ev("a.b", at(101))).unwrap().data["spilled"].is_string(), "one more is a blob");
}

// ---- the operator's clock for a time the doer names ----

#[test]
fn a_time_is_said_on_the_operators_clock_and_with_its_weekday_when_it_is_not_today() {
    use j4n_core::clock::when;
    let (at, noon) = (1_794_066_000_i64, 1_794_066_000_i64 - 3 * 3600);
    // 2026-11-07 15:40 UTC is a Saturday
    assert_eq!(when(at, noon, |_| 0), "15:40");
    assert_eq!(when(at, noon, |_| 3600 * 5 / 2), "18:10", "a zone east of UTC");
    assert_eq!(when(at, noon, |_| -3600 * 8), "07:40", "and one west of it");
    // the next day, and the day it would be there: the weekday comes in front
    assert_eq!(when(at + 86_400, noon, |_| 0), "Sun 15:40");
    assert_eq!(when(at + 6 * 86_400, noon, |_| 0), "Fri 15:40");
    // the day is the operator's: 15:40 UTC + 9h is already tomorrow where they are
    assert_eq!(when(at, noon, |_| 9 * 3600), "Sun 00:40");
    assert_eq!(when(0, 0, |_| 0), "00:00");
    // the real zone is some zone: an offset of less than a day, and the same for the same instant
    let off = j4n_core::clock::offset_at(at);
    assert!(off.abs() < 86_400 && off == j4n_core::clock::offset_at(at));
}
