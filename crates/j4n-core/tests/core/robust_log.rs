//! The Log against damage and against odd input: random harm to a segment, a model of what a log must hold through
//! crashes and reopening, and data that is awkward to write as a line.

use crate::world::*;
use j4n_core::clock::ManualClock;
use j4n_core::log::{Ev, Log, Opts, Scope};
use j4n_core::redact::Redactor;
use serde_json::{Value as Json, json};
use std::fs::{self, File};
use std::io::Write;
use std::sync::Arc;
use std::time::Instant;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn open_with(s: &Scratch, segment_bytes: u64) -> Log {
    let clock = Arc::new(ManualClock::at(1_000));
    let o = Opts { segment_bytes, ..opts(&clock) };
    match Log::open(&s.join(".j4n"), Scope::Project, o, Redactor::new()) {
        Ok((log, _)) => log,
        Err(p) => panic!("{p}"),
    }
}

fn strictly_increasing(events: &[Ev]) -> bool {
    events.windows(2).all(|w| w[0].seq < w[1].seq)
}

#[test]
fn random_damage_to_a_segment_never_panics_and_the_log_can_be_written_after() {
    let mut rng = Rng(0xDEAD_BEEF_1234_5678);
    for round in 0..200 {
        let s = Scratch::new("robust-damage");
        {
            let log = open_with(&s, 8 << 20);
            for i in 0..30 {
                log.append(ev("a.b", json!({"i": i, "text": "hello world"}))).unwrap();
            }
        }
        let path = s.join(".j4n/log/0001.jsonl");
        let mut bytes = fs::read(&path).unwrap();
        for _ in 0..(1 + rng.below(3)) {
            match rng.below(6) {
                0 => bytes.truncate(rng.below(bytes.len() + 1)),
                1 => {
                    let at = rng.below(bytes.len().max(1));
                    if at < bytes.len() {
                        bytes[at] ^= 1 << rng.below(8);
                    }
                }
                2 => {
                    let at = rng.below(bytes.len() + 1);
                    let junk: Vec<u8> = (0..rng.below(40)).map(|_| rng.next() as u8).collect();
                    bytes.splice(at..at, junk);
                }
                3 => {
                    let start = rng.below(bytes.len().max(1));
                    let end = (start + rng.below(200)).min(bytes.len());
                    bytes[start..end].fill(0);
                }
                4 => {
                    let tail = bytes[bytes.len().saturating_sub(rng.below(100))..].to_vec();
                    bytes.extend(tail);
                }
                _ => {
                    let at = rng.below(bytes.len().max(1));
                    bytes.drain(at..(at + rng.below(60)).min(bytes.len()));
                }
            }
        }
        fs::write(&path, &bytes).unwrap();
        let clock = Arc::new(ManualClock::at(9_000));
        let opened = Log::open(&s.join(".j4n"), Scope::Project, opts(&clock), Redactor::new());
        let Ok((log, _)) = opened else {
            continue; // refused with a reason: allowed, a panic is not
        };
        let mut r = log.read(0);
        let events: Vec<Ev> = r.by_ref().collect();
        let _ = r.into_problems();
        assert!(strictly_increasing(&events), "round {round}");
        let head = log.head();
        let next = log.append(ev("a.b", json!("after"))).unwrap();
        assert!(next.seq > head, "round {round}: numbering goes on past what was there");
        drop(log);
        let clock = Arc::new(ManualClock::at(9_000));
        let (again, problems) = Log::open(&s.join(".j4n"), Scope::Project, opts(&clock), Redactor::new()).unwrap();
        assert!(problems.is_empty(), "round {round}: a repaired log opens clean the next time: {problems:?}");
        let last = again.read(0).last().expect("an event");
        assert_eq!(last.data, json!("after"), "round {round}");
    }
}

/// What a torn write might leave: pieces of a line, never a whole one.
const TORN: &[u8] = b"{\"seq\":99,\"at_ms\":1,\"ki";

#[derive(Debug, Clone)]
enum Op {
    Append(u8),
    Reopen,
    Tear(Vec<u8>),
    Flash,
}

#[test]
fn a_log_through_appends_crashes_and_reopens_holds_exactly_what_was_acknowledged() {
    for seed in [1u64, 7, 99, 12345, 0xABCDEF] {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let s = Scratch::new("robust-model");
        let mut log = Some(open_with(&s, 400));
        let repairs = |l: Option<&Log>| l.map_or(0, |l| all(l).iter().filter(|e| e.kind == "log.repaired").count() as u64);
        let mut model: Vec<(String, Json)> = Vec::new();
        let ops: Vec<Op> = (0..120)
            .map(|_| match rng.below(10) {
                0..=5 => Op::Append(rng.below(250) as u8),
                6 | 7 => Op::Reopen,
                8 => Op::Tear((0..rng.below(80)).map(|_| TORN[rng.below(TORN.len())]).collect()),
                _ => Op::Flash,
            })
            .collect();
        for (n, op) in ops.iter().enumerate() {
            match op {
                Op::Append(len) => {
                    let data = json!({"n": n, "pad": "p".repeat(*len as usize)});
                    log.as_ref().unwrap().append(ev("m.x", data.clone())).unwrap();
                    model.push(("m.x".into(), data));
                }
                Op::Flash => {
                    log.as_ref().unwrap().flash(ev("run.delta", json!(n))).unwrap();
                }
                Op::Reopen => {
                    drop(log.take());
                    log = Some(open_with(&s, 400));
                }
                Op::Tear(bytes) => {
                    let before = repairs(log.as_ref());
                    drop(log.take());
                    let last = fs::read_dir(s.join(".j4n/log")).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "jsonl")).max().unwrap();
                    File::options().append(true).open(last).unwrap().write_all(bytes).unwrap();
                    log = Some(open_with(&s, 400));
                    let said = repairs(log.as_ref()) - before;
                    assert_eq!(said, u64::from(!bytes.is_empty()), "seed {seed}, step {n}: a cut line is said once, and nothing else is");
                }
            }
            let held: Vec<(String, Json)> =
                all(log.as_ref().unwrap()).into_iter().filter(|e| e.kind != "log.repaired").map(|e| (e.kind, e.data)).collect();
            let wanted: Vec<(String, Json)> = model.iter().filter(|(k, _)| k != "log.repaired").cloned().collect();
            assert_eq!(held, wanted, "seed {seed}, after step {n} ({op:?})");
            let events = all(log.as_ref().unwrap());
            assert!(strictly_increasing(&events));
            assert_eq!(events.first().map(|e| e.seq), (!events.is_empty()).then_some(1));
            assert_eq!(events.len() as u64, log.as_ref().unwrap().head(), "no number is skipped");
        }
    }
}

#[test]
fn awkward_data_survives_the_line() {
    let s = Scratch::new("robust-awkward");
    let log = open_with(&s, 8 << 20);
    let tricky = [
        "line\nbreak",
        "tab\tand\rcarriage",
        "nul\u{0}inside",
        "separators \u{2028} \u{2029}",
        "bell\u{7} escape\u{1b}[31m",
        "emoji 🦀 and 🧑‍🧑‍🧒‍🧒",
        "right\u{202e}to-left",
        "quote\" backslash\\ slash/",
        "{\"looks\":\"like json\"}",
        "",
    ];
    let mut sent = Vec::new();
    for t in tricky {
        sent.push(log.append(ev("a.b", json!({ t: t, "v": [t, {"n": t}] }))).unwrap());
    }
    let mut deep = json!("bottom");
    for _ in 0..50 {
        deep = json!({"d": [deep]});
    }
    sent.push(log.append(ev("a.b", deep)).unwrap());
    sent.push(log.append(ev("a.b", Json::Object((0..2000).map(|i| (format!("k{i}"), json!(i))).collect()))).unwrap());
    let read = all(&log);
    assert_eq!(read, sent);
    let text = s.read(".j4n/log/0001.jsonl");
    assert_eq!(text.lines().count(), sent.len(), "every event is one line, whatever it holds");
    drop(log);
    let log = open_with(&s, 8 << 20);
    assert_eq!(all(&log), sent, "and the same after a reopen");
}

#[test]
fn data_nested_too_deep_is_cut_before_it_can_break_anything() {
    let s = Scratch::new("robust-deep");
    let log = open_with(&s, 8 << 20);
    let mut deep = json!("bottom");
    for _ in 0..500 {
        deep = json!([deep]);
    }
    let e = log.append(ev("a.b", deep)).unwrap();
    assert!(e.data.to_string().contains("nested too deep"));
    assert_eq!(all(&log).len(), 1);
}

#[test]
fn a_big_log_writes_and_reads_in_reasonable_time() {
    let s = Scratch::new("robust-speed");
    let log = open_with(&s, 1 << 20);
    let started = Instant::now();
    for i in 0..20_000 {
        log.append(ev("a.b", json!({"i": i, "text": "some ordinary line of text"}))).unwrap();
    }
    let written = started.elapsed();
    let read = log.read(0).count();
    let from_end = log.read(19_990).count();
    assert_eq!((read, from_end), (20_000, 11));
    assert!(started.elapsed().as_secs() < 60, "write {written:?}, then read: {:?}", started.elapsed());
}

#[test]
fn reading_from_a_number_skips_whole_segments_it_does_not_need() {
    let s = Scratch::new("robust-skip");
    let log = open_with(&s, 300);
    for i in 0..60 {
        log.append(ev("a.b", json!({"i": i, "pad": "pppppppppppppppp"}))).unwrap();
    }
    // Make an early segment unreadable: a read that starts later must not touch it.
    let first = s.join(".j4n/log/0001.jsonl");
    fs::write(&first, b"\xff\xfe not events at all\n").unwrap();
    let mut r = log.read(40);
    let seqs: Vec<u64> = r.by_ref().map(|e| e.seq).collect();
    assert_eq!(seqs, (40..=60).collect::<Vec<_>>());
    assert!(r.into_problems().is_empty(), "nothing from the early segments was read");
}
