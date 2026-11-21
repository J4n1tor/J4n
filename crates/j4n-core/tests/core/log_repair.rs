//! What a crash leaves, and what a reader meets: the torn last line, junk, gaps, and lines a newer binary wrote.

use crate::world::*;
use j4n_core::clock::ManualClock;
use j4n_core::log::{Log, Opts, Scope};
use j4n_core::redact::Redactor;
use j4n_core::table::{Level, Problem};
use serde_json::json;
use std::fs::{self, File};
use std::io::Write;
use std::sync::Arc;

const SEG: &str = ".j4n/log/0001.jsonl";

fn seed(s: &Scratch, n: u64) -> String {
    let (log, _) = open_log(s);
    for i in 0..n {
        log.append(ev("a.b", json!({"i": i}))).unwrap();
    }
    drop(log);
    s.read(SEG)
}

fn reopen(s: &Scratch) -> (Log, Vec<Problem>) {
    let clock = Arc::new(ManualClock::at(5_000));
    Log::open(&s.join(".j4n"), Scope::Project, opts(&clock), Redactor::new()).expect("opens")
}

fn append_raw(s: &Scratch, bytes: &[u8]) {
    File::options().append(true).open(s.join(SEG)).unwrap().write_all(bytes).unwrap();
}

fn kinds(log: &Log) -> Vec<String> {
    all(log).into_iter().map(|e| format!("{}:{}", e.seq, e.kind)).collect()
}

#[test]
fn a_torn_last_line_is_cut_off_said_so_and_leaves_everything_before_it() {
    let s = Scratch::new("repair-torn");
    let before = seed(&s, 3);
    let torn = br#"{"seq":4,"at_ms":9,"kind":"a.b","by":"cor"#;
    append_raw(&s, torn);
    let (log, problems) = reopen(&s);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].level, Level::Warn);
    assert!(problems[0].msg.contains("torn last line") && problems[0].msg.contains("0001.jsonl"), "{}", problems[0]);
    assert_eq!(kinds(&log), ["1:a.b", "2:a.b", "3:a.b", "4:log.repaired"]);
    let repaired = &all(&log)[3];
    assert_eq!(repaired.data["segment"], "0001.jsonl");
    assert_eq!(repaired.data["bytes"], torn.len());
    assert!(repaired.data["text"].as_str().unwrap().starts_with(r#"{"seq":4"#));
    assert!(s.read(SEG).starts_with(&before), "the lines before it are byte for byte as they were");
    assert_eq!(log.append(ev("a.b", json!(0))).unwrap().seq, 5);
}

#[test]
fn every_way_of_cutting_the_last_line_is_recovered() {
    let s = Scratch::new("repair-every");
    let text = seed(&s, 3);
    let last_start = text[..text.len() - 1].rfind('\n').unwrap() + 1;
    let bytes = text.as_bytes();
    for cut in last_start..=bytes.len() {
        let t = Scratch::new("repair-every-case");
        t.put(SEG, "");
        fs::write(t.join(SEG), &bytes[..cut]).unwrap();
        let (log, problems) = reopen(&t);
        let whole_but_newline = cut == bytes.len() - 1;
        let clean = cut == bytes.len() || cut == last_start;
        let expected_head = if cut == bytes.len() || whole_but_newline { 3 } else { 2 };
        let said = !(clean || whole_but_newline);
        let first_new = if said { expected_head + 2 } else { expected_head + 1 };
        assert_eq!(problems.is_empty(), !said, "cut at {cut}: {problems:?}");
        let n = log.append(ev("a.b", json!(0))).unwrap().seq;
        assert_eq!(n, first_new, "cut at {cut}");
        drop(log);
        let read = t.read(SEG);
        assert!(read.ends_with('\n'), "cut at {cut}: the file ends on a line");
        assert!(read.lines().all(|l| serde_json::from_str::<serde_json::Value>(l).is_ok()), "cut at {cut}: only whole lines");
        let seqs: Vec<u64> = read.lines().map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap()["seq"].as_u64().unwrap()).collect();
        assert_eq!(seqs, (1..=*seqs.last().unwrap()).collect::<Vec<_>>(), "cut at {cut}: numbers follow one another");
    }
}

#[test]
fn a_tail_of_zeros_after_a_power_cut_is_cut_off() {
    let s = Scratch::new("repair-zeros");
    let before = seed(&s, 2);
    append_raw(&s, &[0u8; 4096]);
    let (log, problems) = reopen(&s);
    assert!(problems[0].msg.contains("4096 bytes"), "{}", problems[0]);
    assert_eq!(kinds(&log), ["1:a.b", "2:a.b", "3:log.repaired"]);
    assert!(s.read(SEG).starts_with(&before));
}

#[test]
fn whole_lines_at_the_end_that_are_not_events_are_cut_too_but_nothing_in_the_middle() {
    let s = Scratch::new("repair-junk");
    let before = seed(&s, 2);
    append_raw(&s, b"garbage\n\n{\"no_seq\":1}\n{\"seq\":0,\"at_ms\":1,\"kind\":\"a.b\",\"by\":\"core\"}\n");
    let (log, problems) = reopen(&s);
    assert_eq!(problems.len(), 1);
    assert_eq!(kinds(&log), ["1:a.b", "2:a.b", "3:log.repaired"]);
    assert!(s.read(SEG).starts_with(&before));
}

#[test]
fn a_segment_with_nothing_readable_is_emptied_and_numbering_starts_again() {
    let s = Scratch::new("repair-nothing");
    s.put(SEG, "this is\nnot a log\n");
    let (log, problems) = reopen(&s);
    assert_eq!(problems.len(), 1);
    assert_eq!(kinds(&log), ["1:log.repaired"]);
}

#[test]
fn a_torn_line_in_the_last_segment_is_cut_and_the_earlier_segments_are_not_touched() {
    let s = Scratch::new("repair-two");
    let clock = Arc::new(ManualClock::at(1));
    let o = Opts { segment_bytes: 260, ..opts(&clock) };
    let (log, _) = Log::open(&s.join(".j4n"), Scope::Project, o, Redactor::new()).unwrap();
    for i in 0..12 {
        log.append(ev("a.b", json!({"i": i, "pad": "pppppppppp"}))).unwrap();
    }
    drop(log);
    let segs: Vec<String> = (1..=9).map(|i| format!(".j4n/log/{i:04}.jsonl")).filter(|p| s.exists(p)).collect();
    assert!(segs.len() >= 3);
    let earlier: Vec<String> = segs[..segs.len() - 1].iter().map(|p| s.read(p)).collect();
    File::options().append(true).open(s.join(segs.last().unwrap())).unwrap().write_all(b"{\"seq\":13,\"at").unwrap();
    let (log, problems) = reopen(&s);
    assert!(problems[0].msg.contains("torn"), "{}", problems[0]);
    for (p, text) in segs.iter().zip(&earlier) {
        assert_eq!(&s.read(p), text);
    }
    assert_eq!(log.head(), 13, "12 events and then the note of the repair");
    assert_eq!(all(&log).len(), 13);
}

#[test]
fn an_empty_last_segment_takes_its_numbering_from_the_one_before() {
    let s = Scratch::new("repair-empty-last");
    seed(&s, 3);
    s.put(".j4n/log/0002.jsonl", "");
    let (log, problems) = reopen(&s);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(log.append(ev("a.b", json!(0))).unwrap().seq, 4);
    assert_eq!(s.read(".j4n/log/0002.jsonl").lines().count(), 1);
}

#[test]
fn a_line_a_newer_binary_wrote_is_read_when_it_can_be_and_never_cut_when_it_cannot() {
    let s = Scratch::new("repair-newer");
    let text = seed(&s, 2);
    // Extra fields, a kind nobody here knows, and fields left out; each line links to the one before it.
    let link = |line: &str| j4n_core::log::link(line.trim_end().as_bytes());
    let third = format!("{{\"seq\":3,\"at_ms\":50,\"kind\":\"future.thing\",\"by\":\"core\",\"colour\":\"red\",\"prev\":\"{}\",\"data\":{{\"deep\":[1,2]}}}}\n", link(text.lines().last().unwrap()));
    append_raw(&s, third.as_bytes());
    // An actor kind this version does not have: not readable, but whole.
    append_raw(&s, format!("{{\"seq\":4,\"at_ms\":60,\"kind\":\"future.thing\",\"by\":{{\"hologram\":\"h\"}},\"prev\":\"{}\"}}\n", link(&third)).as_bytes());
    let before = s.read(SEG);
    let (log, problems) = reopen(&s);
    assert!(problems.is_empty(), "nothing was cut: {problems:?}");
    assert_eq!(s.read(SEG), before, "the file is exactly as it was");
    assert_eq!(log.head(), 4);
    let mut reader = log.read(0);
    let events: Vec<_> = reader.by_ref().collect();
    assert_eq!(events.iter().map(|e| e.seq).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(events[2].kind, "future.thing");
    assert_eq!(events[2].data["deep"], json!([1, 2]));
    assert_eq!(events[2].class, j4n_core::schema::DataClass::Project, "a field left out is its default");
    let p = reader.into_problems();
    assert_eq!(p.len(), 1);
    assert!(p[0].at.contains("0001.jsonl:4") && p[0].msg.contains("not an event this version reads"), "{}", p[0]);
}

#[test]
fn a_bad_line_in_the_middle_is_skipped_and_said_and_the_rest_is_read() {
    let s = Scratch::new("repair-middle");
    seed(&s, 4);
    let text = s.read(SEG);
    let mut lines: Vec<&str> = text.lines().collect();
    lines[1] = "{ this is not json";
    fs::write(s.join(SEG), lines.join("\n") + "\n").unwrap();
    let (log, problems) = reopen(&s);
    assert!(problems.is_empty());
    let mut r = log.read(0);
    let seqs: Vec<u64> = r.by_ref().map(|e| e.seq).collect();
    assert_eq!(seqs, [1, 3, 4]);
    let p = r.into_problems();
    assert!(p.iter().any(|p| p.at.ends_with("0001.jsonl:2") && p.msg.contains("skipped")), "{p:?}");
    assert!(p.iter().any(|p| p.msg.contains("events are missing")), "a gap is said: {p:?}");
}

#[test]
fn a_seq_that_goes_backwards_or_stands_still_is_skipped() {
    for nth in [1usize, 2] {
        let s = Scratch::new("repair-order");
        let text = seed(&s, 3);
        let dup = text.lines().nth(nth).unwrap();
        append_raw(&s, format!("{dup}\n").as_bytes());
        let (log, _) = reopen(&s);
        let mut r = log.read(0);
        assert_eq!(r.by_ref().count(), 3, "line {nth} again");
        assert!(r.into_problems().iter().any(|p| p.msg.contains("out of order")), "line {nth} again");
    }
}

#[test]
fn a_line_being_written_is_no_problem_and_a_torn_one_in_a_closed_segment_is() {
    let s = Scratch::new("repair-inflight");
    let clock = Arc::new(ManualClock::at(1));
    let o = Opts { segment_bytes: 200, ..opts(&clock) };
    let (log, _) = Log::open(&s.join(".j4n"), Scope::Project, o, Redactor::new()).unwrap();
    for i in 0..6 {
        log.append(ev("a.b", json!({"i": i, "pad": "pppppppppp"}))).unwrap();
    }
    // A write in flight: the end of the last segment is half a line, as a reader can see it.
    let last = (1..=9).map(|i| format!(".j4n/log/{i:04}.jsonl")).rfind(|p| s.exists(p)).unwrap();
    File::options().append(true).open(s.join(&last)).unwrap().write_all(b"{\"seq\":7,\"at").unwrap();
    let mut r = log.read(0);
    assert_eq!(r.by_ref().count(), 6);
    assert!(r.into_problems().is_empty(), "not a problem: the writer is not done");
    // The same half line in a segment that is closed is one.
    let first = s.join(".j4n/log/0001.jsonl");
    File::options().append(true).open(&first).unwrap().write_all(b"{\"seq\":9,\"at").unwrap();
    let mut r = log.read(0);
    let _ = r.by_ref().count();
    let p = r.into_problems();
    assert!(p.iter().any(|p| p.at == "0001.jsonl" || p.at.starts_with("0001.jsonl")) && p.iter().any(|p| p.msg.contains("torn line in a closed segment")), "{p:?}");
}

#[test]
fn reading_from_the_first_number_of_a_segment_does_not_touch_the_one_before() {
    let s = Scratch::new("repair-boundary");
    let clock = Arc::new(ManualClock::at(1));
    let o = Opts { segment_bytes: 300, ..opts(&clock) };
    let (log, _) = Log::open(&s.join(".j4n"), Scope::Project, o, Redactor::new()).unwrap();
    for i in 0..30 {
        log.append(ev("a.b", json!({"i": i, "pad": "pppppppppppppppp"}))).unwrap();
    }
    let second_first: u64 = serde_json::from_str::<serde_json::Value>(s.read(".j4n/log/0002.jsonl").lines().next().unwrap()).unwrap()["seq"].as_u64().unwrap();
    fs::write(s.join(".j4n/log/0001.jsonl"), b"\xff not events\n").unwrap();
    let mut r = log.read(second_first);
    assert_eq!(r.by_ref().next().map(|e| e.seq), Some(second_first));
    let _ = r.by_ref().count();
    assert!(r.into_problems().is_empty(), "segment one was not read for a number that starts segment two");
}

#[test]
fn a_segment_made_of_a_link_is_not_one() {
    let s = Scratch::new("repair-link");
    seed(&s, 2);
    s.put("elsewhere.jsonl", "{\"seq\":3,\"at_ms\":1,\"kind\":\"a.b\",\"by\":\"core\"}\n");
    std::os::unix::fs::symlink(s.join("elsewhere.jsonl"), s.join(".j4n/log/0002.jsonl")).unwrap();
    let (log, _) = reopen(&s);
    assert_eq!(log.head(), 2, "the link is not a segment: its event is not in the log");
    assert_eq!(all(&log).len(), 2);
}

#[test]
fn a_last_line_exactly_as_long_as_a_line_may_be_is_torn_not_too_long() {
    let s = Scratch::new("repair-linemax");
    let clock = Arc::new(ManualClock::at(1));
    let o = Opts { segment_bytes: 200, ..opts(&clock) };
    let (log, _) = Log::open(&s.join(".j4n"), Scope::Project, o, Redactor::new()).unwrap();
    for i in 0..8 {
        log.append(ev("a.b", json!({"i": i, "pad": "pppppppppp"}))).unwrap();
    }
    drop(log);
    // Close segment one with a final line of exactly the longest length, and no newline after it.
    File::options().append(true).open(s.join(".j4n/log/0001.jsonl")).unwrap().write_all(&vec![b'x'; j4n_core::log::LINE_MAX]).unwrap();
    let (log, _) = reopen(&s);
    let mut r = log.read(0);
    let _ = r.by_ref().count();
    let p = r.into_problems();
    assert!(p.iter().any(|p| p.msg.contains("torn line in a closed segment")), "{p:?}");
    assert!(!p.iter().any(|p| p.msg.contains("bytes — skipped")), "{p:?}");
}

#[test]
fn a_very_long_line_is_skipped_without_reading_all_of_it_into_the_event_stream() {
    let s = Scratch::new("repair-long");
    let text = seed(&s, 2);
    let mut long = vec![b'x'; 9 << 20];
    long.push(b'\n');
    let mut all_bytes = text.into_bytes();
    all_bytes.extend(long);
    all_bytes.extend(b"{\"seq\":3,\"at_ms\":9,\"kind\":\"a.b\",\"by\":\"core\"}\n");
    fs::write(s.join(SEG), all_bytes).unwrap();
    let (log, problems) = reopen(&s);
    assert!(problems.is_empty());
    let mut r = log.read(0);
    assert_eq!(r.by_ref().map(|e| e.seq).collect::<Vec<_>>(), [1, 2, 3]);
    assert!(r.into_problems().iter().any(|p| p.msg.contains("bytes — skipped")));
}

#[test]
fn many_bad_lines_are_summed_not_listed() {
    let s = Scratch::new("repair-many");
    let text = seed(&s, 1);
    let junk = "junk\n".repeat(100);
    fs::write(s.join(SEG), format!("{}{junk}{}", text, text.replace("\"seq\":1", "\"seq\":2"))).unwrap();
    let (log, _) = reopen(&s);
    let mut r = log.read(0);
    assert_eq!(r.by_ref().count(), 2);
    let p = r.into_problems();
    assert!(p.len() <= 22, "{}", p.len());
    assert!(p.last().unwrap().msg.contains("more lines"), "{:?}", p.last());
}

#[test]
fn files_that_are_not_segments_are_not_read() {
    let s = Scratch::new("repair-names");
    seed(&s, 1);
    s.put(".j4n/log/notes.jsonl", "{\"seq\":9,\"at_ms\":1,\"kind\":\"a.b\",\"by\":\"core\"}\n");
    s.put(".j4n/log/12.jsonl", "{\"seq\":9,\"at_ms\":1,\"kind\":\"a.b\",\"by\":\"core\"}\n");
    s.put(".j4n/log/0003.jsonl.bak", "{\"seq\":9,\"at_ms\":1,\"kind\":\"a.b\",\"by\":\"core\"}\n");
    std::os::unix::fs::symlink("0001.jsonl", s.join(".j4n/log/0009.jsonl")).unwrap();
    let (log, _) = reopen(&s);
    assert_eq!(all(&log).len(), 1);
    assert_eq!(log.head(), 1);
}

#[test]
fn a_tail_cut_at_any_byte_of_a_long_log_never_loses_a_whole_line() {
    let s = Scratch::new("repair-sweep");
    let text = seed(&s, 6);
    let lines: Vec<usize> = text.match_indices('\n').map(|(i, _)| i + 1).collect();
    for cut in (0..=text.len()).step_by(7) {
        let t = Scratch::new("repair-sweep-case");
        fs::create_dir_all(t.join(".j4n/log")).unwrap();
        fs::write(t.join(SEG), &text.as_bytes()[..cut]).unwrap();
        let (log, _) = reopen(&t);
        let whole = lines.iter().filter(|&&end| end <= cut).count() as u64;
        let whole_plus = whole + u64::from(lines.contains(&(cut + 1)));
        let kept = all(&log).into_iter().filter(|e| e.kind == "a.b").count() as u64;
        assert!(kept == whole || kept == whole_plus, "cut {cut}: kept {kept} of {whole} whole lines");
    }
}

#[test]
fn a_tail_with_no_line_in_it_at_all_is_refused_and_left_alone() {
    let s = Scratch::new("repair-window");
    seed(&s, 2);
    let before = fs::metadata(s.join(SEG)).unwrap().len();
    // 70 MiB with no newline: nothing in the last 64 MiB says where a line begins.
    let mut f = File::options().append(true).open(s.join(SEG)).unwrap();
    let chunk = vec![b'x'; 1 << 20];
    for _ in 0..70 {
        f.write_all(&chunk).unwrap();
    }
    drop(f);
    let clock = Arc::new(ManualClock::at(1));
    let refused = Log::open(&s.join(".j4n"), Scope::Project, opts(&clock), Redactor::new());
    let err = refused.err().expect("refused").to_string();
    assert!(err.contains("0001.jsonl") && err.contains("no whole line"), "{err}");
    assert_eq!(fs::metadata(s.join(SEG)).unwrap().len(), before + (70 << 20), "and nothing was cut");
}
