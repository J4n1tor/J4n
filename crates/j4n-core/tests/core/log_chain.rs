//! Each line of the Log carries `prev`, the link of the line before it, and a reader says where the
//! chain breaks. A break is told in the Log once (`log.broken`), and what the Lanes fold holds as the commands a task ran (the evidence
//! of the items' checks) from the lines between the break and that note is dropped.

use crate::world::*;
use j4n_core::change::Actor;
use j4n_core::clock::ManualClock;
use j4n_core::fold::Fold;
use j4n_core::lanes::Lanes;
use j4n_core::log::{Ev, Log, Opts, Scope, link};
use j4n_core::redact::Redactor;
use j4n_core::table::Problem;
use serde_json::{Value, json};
use std::fs::{self, File};
use std::io::Write;
use std::sync::Arc;

const SEG: &str = ".j4n/log/0001.jsonl";

fn lines(s: &Scratch, seg: &str) -> Vec<String> {
    s.read(seg).lines().map(String::from).collect()
}

fn reopen(s: &Scratch, segment_bytes: u64) -> (Log, Vec<Problem>) {
    let clock = Arc::new(ManualClock::at(5_000));
    Log::open(&s.join(".j4n"), Scope::Project, Opts { segment_bytes, ..opts(&clock) }, Redactor::new()).expect("opens")
}

/// A log of `n` events, written and closed.
fn seed(tag: &str, n: u64) -> Scratch {
    let s = Scratch::new(tag);
    let (log, _) = open_log(&s);
    for i in 0..n {
        log.append(ev("run.shell", json!({ "i": i, "cmd": "cargo test", "exit": 0 }))).unwrap();
    }
    drop(log);
    s
}

fn problems_of(log: &Log) -> Vec<String> {
    let mut r = log.read(0);
    r.by_ref().for_each(drop);
    r.into_problems().iter().map(|p| p.to_string()).collect()
}

fn told(log: &Log) -> Vec<Ev> {
    all(log).into_iter().filter(|e| e.kind == "log.broken").collect()
}

#[test]
fn every_line_carries_the_link_of_the_one_before_across_segments_and_restarts() {
    let s = Scratch::new("i9-links");
    let clock = Arc::new(ManualClock::at(1_000));
    let (log, _) = Log::open(&s.join(".j4n"), Scope::Project, Opts { segment_bytes: 600, ..opts(&clock) }, Redactor::new()).unwrap();
    for i in 0..6 {
        log.append(ev("a.b", json!({ "i": i }))).unwrap();
    }
    drop(log);
    let (log, found) = reopen(&s, 600);
    assert!(found.is_empty(), "{found:?}");
    log.append(ev("a.b", json!({ "i": 6 }))).unwrap();
    drop(log);
    let segs: Vec<String> = (1..=9).map(|i| format!(".j4n/log/{i:04}.jsonl")).filter(|p| s.exists(p)).collect();
    assert!(segs.len() > 1, "the log rotated: {segs:?}");
    let all_lines: Vec<String> = segs.iter().flat_map(|p| lines(&s, p)).collect();
    assert_eq!(all_lines.len(), 7);
    assert!(!all_lines[0].contains("\"prev\""), "the first line has nothing before it");
    for pair in all_lines.windows(2) {
        let prev: Value = serde_json::from_str(&pair[1]).unwrap();
        assert_eq!(prev["prev"], link(pair[0].as_bytes()), "{}", pair[1]);
    }
    let (log, found) = reopen(&s, 600);
    assert!(found.is_empty() && problems_of(&log).is_empty() && told(&log).is_empty());
}

#[test]
fn an_edited_line_breaks_the_chain_and_the_read_says_where() {
    let s = seed("i9-edit", 5);
    let text = s.read(SEG);
    // the doer's edit: a failed command made to look green, in the third event
    let forged = text.lines().map(|l| if l.contains("\"i\":2") { l.replace("\"exit\":0", "\"exit\":1").replace("cargo test", "cargo tes_") } else { l.to_string() }).collect::<Vec<_>>().join("\n") + "\n";
    assert_ne!(forged, text);
    fs::write(s.join(SEG), forged).unwrap();
    let (log, found) = reopen(&s, 8 << 20);
    let said = problems_of(&log);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("0001.jsonl:4") && said[0].contains("chain of the log is broken"), "{said:?}");
    assert!(found.is_empty(), "the open itself reads only the tail: {found:?}");
    let mut r = log.read(0);
    r.by_ref().for_each(drop);
    assert_eq!(r.broken(), [(2, "0001.jsonl:4".to_string())], "events up to 2 still fit");
}

#[test]
fn a_line_added_while_the_log_is_open_is_found_at_the_core_s_next_line() {
    let s = Scratch::new("i9-add");
    let (log, _) = open_log(&s);
    log.append(ev("run.shell", json!({ "cmd": "cargo test", "exit": 1 }))).unwrap();
    // a doer that read the last line makes a line that follows it perfectly, and says its command passed
    let last = lines(&s, SEG).pop().unwrap();
    let fake = json!({ "seq": 2, "at_ms": 1, "kind": "run.shell", "task": "t1", "by": "core", "prev": link(last.as_bytes()), "data": { "cmd": "cargo test", "exit": 0 } });
    File::options().append(true).open(s.join(SEG)).unwrap().write_all(format!("{fake}\n").as_bytes()).unwrap();
    assert_eq!(problems_of(&log), Vec::<String>::new(), "a line that fits is no break until something follows it");
    log.append(ev("task.settled", json!({}))).unwrap();
    let said = problems_of(&log);
    assert!(said.iter().any(|m| m.contains("0001.jsonl:3") && m.contains("broken")), "{said:?}");
}

#[test]
fn a_break_is_told_in_the_log_once_to_the_sinks_and_not_again_at_the_next_start() {
    let s = seed("i9-told", 4);
    let text = s.read(SEG);
    let mut v: Vec<String> = text.lines().map(String::from).collect();
    v.remove(1); // an event cut out: seq 3 now follows seq 1 (line 2 does not fit line 1)
    fs::write(s.join(SEG), v.join("\n") + "\n").unwrap();
    let (log, _) = reopen(&s, 8 << 20);
    struct Spy(Arc<std::sync::Mutex<Vec<String>>>);
    impl j4n_core::log::Sink for Spy {
        fn event(&mut self, ev: &Ev) {
            self.0.lock().unwrap().push(format!("{}:{}", ev.seq, ev.kind));
        }
    }
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let attached = log.attach(Box::new(Spy(seen.clone())), 0).unwrap();
    assert!(attached.iter().any(|p| p.to_string().contains("broken")), "{attached:?}");
    let tell = told(&log);
    assert_eq!(tell.len(), 1, "{tell:?}");
    assert_eq!((tell[0].by.clone(), tell[0].data["after"].as_u64(), tell[0].data["at"].as_str()), (Actor::Core, Some(0), Some("0001.jsonl:2")), "nothing before the cut was ever matched by a successor, so none of it is trusted");
    assert_eq!(seen.lock().unwrap().last().map(String::as_str), Some("5:log.broken"), "the sink that replayed is told too");
    // a second attach in the same run, and a new start: the break is still in the file, and no second note is made
    log.attach(Box::new(Spy(seen.clone())), log.head()).unwrap();
    drop(log);
    let (log, _) = reopen(&s, 8 << 20);
    log.attach(Box::new(Spy(seen.clone())), 0).unwrap();
    assert_eq!(told(&log).len(), 1);
    // the events the core writes after the note chain from it
    log.append(ev("a.b", json!({}))).unwrap();
    let said = problems_of(&log);
    assert_eq!(said.iter().filter(|m| m.contains("broken")).count(), 1, "only the one old break: {said:?}");
}

#[test]
fn a_log_from_before_the_chain_reads_clean_and_the_chain_starts_at_the_first_new_line() {
    let s = Scratch::new("i9-old");
    let plain: Vec<String> = (1..=3).map(|i| json!({ "seq": i, "at_ms": i, "kind": "a.b", "by": "core", "class": "project", "data": { "i": i } }).to_string()).collect();
    s.put(SEG, &(plain.join("\n") + "\n"));
    let (log, found) = reopen(&s, 8 << 20);
    assert!(found.is_empty(), "{found:?}");
    log.append(ev("a.b", json!({}))).unwrap();
    let all_lines = lines(&s, SEG);
    let fourth: Value = serde_json::from_str(&all_lines[3]).unwrap();
    assert_eq!(fourth["prev"], link(plain[2].as_bytes()));
    assert!(problems_of(&log).is_empty());
    // a line without a link after linked ones is a break: a doer cannot slip a bare line in
    File::options().append(true).open(s.join(SEG)).unwrap().write_all(format!("{}\n", plain[0].replace("\"seq\":1", "\"seq\":5")).as_bytes()).unwrap();
    assert!(problems_of(&log).iter().any(|m| m.contains("0001.jsonl:5")));
}

fn shell(seq: u64, task: &str, exit: i64) -> Ev {
    Ev { seq, at_ms: seq, kind: "run.shell".into(), task: Some(task.into()), thread: None, by: Actor::Doer("claude".into()), cause: None, class: Default::default(), data: json!({ "cmd": "cargo test", "exit": exit }) }
}

fn queued(seq: u64, task: &str) -> Ev {
    Ev { seq, at_ms: seq, kind: "task.queued".into(), task: Some(task.into()), thread: Some("main".into()), by: Actor::Core, cause: None, class: Default::default(), data: json!({ "spec": { "goal": "g", "doer": "d", "parent": "t1", "thread": "main", "role": "check", "tune": {} } }) }
}

#[test]
fn what_a_task_ran_between_the_break_and_the_note_is_no_evidence() {
    let mut lanes = Lanes::new();
    for e in [queued(1, "t2"), shell(2, "t2", 1), shell(4, "t2", 0), queued(5, "t3"), shell(6, "t3", 0)] {
        lanes.apply(&e);
    }
    assert_eq!(lanes.ran("t2").len(), 2);
    // the chain broke after event 2; the note is event 8; t3's check ran again at 9, under J4n
    lanes.apply(&Ev { kind: "log.broken".into(), by: Actor::Core, data: json!({ "after": 2, "at": "0001.jsonl:3", "lines": 1 }), task: None, ..shell(8, "", 0) });
    lanes.apply(&shell(9, "t3", 0));
    assert_eq!(lanes.ran("t2"), [("cargo test".to_string(), 1)], "the green exit at 4 is gone and the red one at 2 stays");
    assert_eq!(lanes.ran("t3"), [("cargo test".to_string(), 0)], "the line at 6 is gone, the one at 9 is the core's own");
}
