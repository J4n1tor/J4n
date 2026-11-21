//! The result ledger through the binary's own wiring and the scripted `test` doer. When nothing of
//! a thread is open, one report is written from the Log: the facts, a receipt for every line (the inbox's items among them), the answers merged
//! under the line they answer; `mixed` adds a paragraph of a mock model that the facts must hold; `:report` and `j4n report` rebuild it.

use crate::models_cases::mock::{Mock, Say};
use crate::run_cases::{Dirs, j4n};
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::change::Actor;
use j4n_core::front::Said;
use j4n_core::log::New;
use j4n_core::task::{Mode, State};
use j4n_ledger::merge::{Mark, merge, trim_opener};
use serde_json::{Value, json};
use std::fs;
use std::time::{Duration, Instant};

const ASK: &str = "You write one short paragraph for the operator of J4n";

fn open(d: &Dirs) -> World {
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Full), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"))
}

/// A world whose operator folder holds `settings` (`id = value` lines) and, with `extra`, more connector rows.
fn rig(d: &Dirs, settings: &[(&str, &str)], extra: &str) -> World {
    let mut rows = String::from("api = 1\n");
    for (id, value) in settings {
        rows.push_str(&format!("[[setting]]\nid = \"{id}\"\nvalue = {value}\n"));
    }
    fs::write(d.j4n_home().join("settings/ledger.toml"), rows).unwrap();
    if !extra.is_empty() {
        fs::create_dir_all(d.j4n_home().join("connectors")).unwrap();
        fs::write(d.j4n_home().join("connectors/mine.toml"), format!("api = 1\n{extra}")).unwrap();
    }
    let w = open(d);
    assert!(w.notices.is_empty(), "{:?}", w.notices);
    w
}

fn dump(w: &World) -> String {
    w.front.log().sync();
    w.front.log().read(0).map(|e| format!("{:>4} {:<16} {} {} {}", e.seq, e.kind, e.task.as_deref().unwrap_or("-"), e.thread.as_deref().unwrap_or("-"), e.data)).collect::<Vec<_>>().join("\n")
}

fn events(w: &World, kind: &str) -> Vec<Value> {
    w.front.log().sync();
    w.front.log().read(0).filter(|e| e.kind == kind).map(|e| e.data).collect()
}

fn notes(w: &World) -> Vec<String> {
    events(w, "chat.note").iter().map(|n| n["text"].as_str().unwrap_or_default().to_string()).collect()
}

fn until(w: &World, what: &str, mut ready: impl FnMut() -> bool) {
    let end = Instant::now() + crate::support::PATIENCE;
    while !ready() {
        assert!(Instant::now() < end, "never: {what}\n{}", dump(w));
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn start(w: &World, script: &str) -> String {
    let Said::Started { task } = w.front.say("main", script) else { panic!("{script:?} did not start a task\n{}", dump(w)) };
    until(w, "it runs", || w.front.tasks().book().get(&task).is_some_and(|r| matches!(r.state, State::Running) || r.state.is_terminal()));
    task
}

fn ended(w: &World, task: &str) {
    w.front.tasks().book().wait(task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap_or_else(|| panic!("{task} never ended\n{}", dump(w)));
}

/// The settled boundary, as the screen makes it when a task ends.
fn boundary(w: &World, task: &str) {
    ended(w, task);
    w.front.plan_step("main");
}

fn ledger_events(w: &World) -> usize {
    w.front.log().sync();
    w.front.log().read(0).filter(|e| e.kind.starts_with("ledger.")).count()
}

/// The goal of the first task: it writes a file, runs a command, waits for the lines, and answers.
const WORK: &str = "refactor the config parser module and its error messages; wait 2500; write src/parser.rs fn parse() {}; sh cargo test parser_cases => 0; sh cargo test lexer_cases => 1; say Sure, the config parser is refactored. Its error messages name the line. The lexer test still fails.";
const STEER: &str = "The config parser Errors  should name the line";
const NOTE: &str = "note: bump the docs later";

/// Three lines and two tasks: the goal, a `steer` line delivered to the next turn at the boundary, and a `note` that is kept.
fn three_lines(w: &World) -> (String, String) {
    let first = start(w, WORK);
    assert!(matches!(w.front.say("main", STEER), Said::Kept { tag: "steer", .. }), "{}", dump(w));
    assert!(matches!(w.front.say("main", NOTE), Said::Kept { tag: "note", .. }), "{}", dump(w));
    boundary(w, &first);
    // the next turn carries the steer line; while it works the thread is not settled, so no report yet
    let second = w.front.tasks().book().rows().into_iter().filter(|r| r.spec.doer == "test").nth(1).unwrap_or_else(|| panic!("no second turn\n{}", dump(w))).id;
    boundary(w, &second);
    // that is read in the Log's order, not as the state between two steps: the turn is quick, and a starved test thread looks after it
    // ended and the report is made. A report waits for nothing of the thread to be open: none before the second turn settled.
    w.front.log().sync();
    let log: Vec<_> = w.front.log().read(0).collect();
    let settled = log.iter().find(|e| e.kind == "task.settled" && e.task.as_deref() == Some(second.as_str())).unwrap_or_else(|| panic!("{second} never settled\n{}", dump(w))).seq;
    let early: Vec<u64> = log.iter().filter(|e| e.kind == "ledger.report" && e.seq < settled).map(|e| e.seq).collect();
    assert!(early.is_empty(), "a report waits for nothing of the thread to be open: {early:?} before {second} settled at {settled}\n{}", dump(w));
    (first, second)
}

#[test]
fn the_ledger_of_three_lines_and_two_tasks_is_facts_from_events_and_a_receipt_for_every_line() {
    let d = Dirs::new("ledger-three-lines-two");
    let w = rig(&d, &[("ledger.mode", "\"on\""), ("inbox.mode", "\"show\"")], "");
    let (first, second) = three_lines(&w);
    let reports = events(&w, "ledger.report");
    assert_eq!(reports.len(), 1, "one report for the settled thread\n{}", dump(&w));
    let r = &reports[0];
    let card = r["card"].as_str().unwrap().to_string();
    assert_eq!((r["thread"].as_str(), r["tasks"].as_u64(), r["lines"].as_u64(), r["prose"].as_str()), (Some("main"), Some(2), Some(3), Some("facts")), "{card}");
    // the facts are the events': the file the task wrote, the two commands and the one that failed, the cost
    assert!(card.starts_with("◆ ledger · main · 2 tasks · ✓ 2"), "{card}");
    assert!(card.contains("± 1 files: src/parser.rs · ❱ 2 ran, 1 failed · $0.0000"), "{card}");
    // every line has a receipt, numbered in the order typed; none is merged away
    assert!(card.contains("├─ ❯ #1 “refactor the config parser module and its error messages; wa…” → t1 ✓") && first == "t1", "{card}");
    // the second turn is t3: the write's approval was the thread's t2
    assert!(card.contains(&format!("├─ → #2 “{STEER}” delivered to {second} ✓")) && second == "t3", "{card}");
    assert!(card.contains(&format!("¶ #3 “{NOTE}” kept")), "{card}");
    assert_eq!(card.matches(" #").count(), 3, "{card}");
    // the answer stands under the line it answers, its opener cut, two sentences and `more`
    assert!(card.contains(&format!("│  ╰─ {first}  the config parser is refactored. Its error messages name the line. … more")), "{card}");
    assert!(card.ends_with("╰─ ¶ #3 “note: bump the docs later” kept"), "{card}");
    assert!(!card.contains("Sure,"), "{card}");
    // the same as rows with a depth, and as lines indented with no pipes in them, for the screen to hang
    let tree = r["tree"].as_array().unwrap();
    assert_eq!(tree[0][0], 0, "the head is depth 0");
    assert!(tree.iter().any(|t| t[0] == 2 && t[1].as_str().unwrap().starts_with(&first)), "the answer is a row under its line: {tree:?}");
    assert!(tree.iter().all(|t| !t[1].as_str().unwrap().contains(['├', '╰'])), "{tree:?}");
    // the card is what the chat said, once; a second boundary says nothing
    assert_eq!(notes(&w).iter().filter(|n| **n == card).count(), 1, "{}", dump(&w));
    w.front.plan_step("main");
    assert_eq!(events(&w, "ledger.report").len(), 1, "nothing ended since: no second report\n{}", dump(&w));
    // `:report` rebuilds it from the Log; `all` says the answer whole
    w.front.say("main", ":report");
    assert_eq!(notes(&w).last(), Some(&card));
    w.front.say("main", ":report all");
    assert!(notes(&w).last().unwrap().contains("The lexer test still fails."), "{:?}", notes(&w).last());
    assert!(notes(&w).last().unwrap().contains("`sh cargo test lexer_cases` → 1") || notes(&w).last().unwrap().contains("`cargo test lexer_cases` → 1"), "{:?}", notes(&w).last());
    w.close();
}

#[test]
fn with_prose_off_the_report_is_facts_and_receipts_only() {
    let d = Dirs::new("ledger-off");
    let w = rig(&d, &[("ledger.mode", "\"on\""), ("ledger.prose", "\"off\""), ("inbox.mode", "\"show\"")], "");
    three_lines(&w);
    let card = events(&w, "ledger.report")[0]["card"].as_str().unwrap().to_string();
    assert!(card.contains("#1") && card.contains("#2") && card.contains("#3"), "{card}");
    assert!(!card.contains("refactored"), "no doer's words: {card}");
    w.close();
}

#[test]
fn with_the_switch_off_nothing_is_reported_and_the_log_has_nothing_of_the_ledger() {
    for (tag, settings, pack_off) in [("ledger-switch-off-nothing", vec![("inbox.mode", "\"show\""), ("ledger.mode", "\"off\"")], false), ("ledger-switch-off-nothing-2", vec![("inbox.mode", "\"show\""), ("ledger.mode", "\"no\"")], false), ("ledger-switch-off-nothing-3", vec![("inbox.mode", "\"show\"")], true)] {
        let d = Dirs::new(tag);
        if pack_off {
            d.pack_off("ledger");
        }
        let w = rig(&d, &settings, "");
        let before = notes(&w).len();
        three_lines(&w);
        assert_eq!(ledger_events(&w), 0, "{tag}\n{}", dump(&w));
        assert!(notes(&w)[before..].iter().all(|n| !n.contains("◆ ledger")), "{tag}");
        let said = w.front.say("main", ":report");
        assert!(matches!(said, Said::Refused(_)), "{tag}: {said:?}");
        w.close();
    }
}

#[test]
fn the_plan_report_is_the_ledger_plan_row_not_a_second_report() {
    let d = Dirs::new("ledger-plan");
    let w = rig(&d, &[("ledger.mode", "\"on\"")], "");
    let task = start(&w, "say the plan's last item is done");
    // the plan keeper writes `plan.reported` at the boundary; the ledger reads it as its plan row and its decisions
    let data = json!({ "sig": "s", "outcome": "blocked", "merged": ["p1"], "blocked": ["p2"], "left": ["p3"], "decisions": ["p2: the cap was hit"], "micros": 0, "wall_ms": 0 });
    w.front.log().append(New::of("plan.reported", Actor::Core, data).thread("main")).unwrap();
    boundary(&w, &task);
    let card = events(&w, "ledger.report")[0]["card"].as_str().unwrap().to_string();
    assert!(card.contains("◆ plan blocked: 1/3 done (p1) · blocked p2"), "{card}");
    assert!(card.contains("decide — p2: the cap was hit") && card.contains("plan left p3"), "{card}");
    w.close();
}

fn chat(text: &str) -> Say {
    Say::Sse(vec![
        json!({ "choices": [{ "delta": { "content": text } }] }).to_string(),
        json!({ "choices": [{ "delta": {}, "finish_reason": "stop" }] }).to_string(),
        json!({ "choices": [], "usage": { "prompt_tokens": 12, "completion_tokens": 3 } }).to_string(),
    ])
}

fn mixed(tag: &str, paragraph: &'static str) -> (Dirs, World, Mock) {
    let d = Dirs::new(tag);
    let gateway = Mock::start(move |_, _| chat(paragraph));
    let key = d.dir.join("key");
    crate::support::key_file(&key, "sk-test-ledger-0001\n");
    let row = format!("[[connector]]\nid = \"proser\"\nis = \"model\"\ndialect = \"openai\"\nurl = \"{}/v1\"\nkey = \"file:{}\"\nmodels = [\"vendor/m\"]\n", gateway.url(), key.display());
    let w = rig(&d, &[("ledger.mode", "\"on\""), ("ledger.prose", "\"mixed\""), ("ledger.model", "\"proser:vendor/m\""), ("inbox.mode", "\"show\"")], &row);
    (d, w, gateway)
}

#[test]
fn a_paragraph_with_a_made_up_path_or_number_is_dropped_and_the_facts_stand() {
    let (_d, w, gateway) = mixed("ledger-paragraph-made-up", "Two tasks ran: the parser in src/parser.rs and src/lexer.rs was refactored and 42 tests pass.");
    three_lines(&w);
    until(&w, "the paragraph", || !events(&w, "ledger.prose").is_empty());
    let prose = &events(&w, "ledger.prose")[0];
    assert_eq!((prose["kept"].as_bool(), prose["prose"].as_str()), (Some(false), Some("")), "{prose}");
    assert_eq!(prose["unknown"], json!(["src/lexer.rs", "42"]), "the path and the number the facts do not hold: {prose}");
    // the model was asked once, with the facts and the answers quoted as data
    let seen = gateway.seen();
    assert_eq!(seen.len(), 1);
    let body = seen[0].json().to_string();
    assert!(body.contains(ASK) && body.contains("src/parser.rs") && body.contains("> Sure, the config parser is refactored."), "{body}");
    // the facts stand: the report is as it was, and the chat says the paragraph was dropped
    let card = events(&w, "ledger.report")[0]["card"].as_str().unwrap().to_string();
    assert!(card.contains("± 1 files: src/parser.rs"), "{card}");
    assert!(notes(&w).iter().any(|n| n.starts_with("~ the paragraph was dropped (it names src/lexer.rs, 42, which the facts do not hold)")), "{:?}", notes(&w));
    assert!(!notes(&w).iter().any(|n| n.contains("42 tests pass")));
    w.close();
}

#[test]
fn a_paragraph_the_facts_hold_is_kept_with_its_mark_and_read_back_by_report() {
    let paragraph = "The thread ran 2 tasks: t1 changed src/parser.rs, ran 2 commands of which 1 failed, and cost $0.00.";
    let (_d, w, _gateway) = mixed("ledger-paragraph-facts-hold", paragraph);
    three_lines(&w);
    until(&w, "the paragraph", || !events(&w, "ledger.prose").is_empty());
    let prose = &events(&w, "ledger.prose")[0];
    assert_eq!((prose["kept"].as_bool(), prose["prose"].as_str()), (Some(true), Some(paragraph)), "{prose}");
    assert!(notes(&w).contains(&format!("~ {paragraph}")), "{:?}", notes(&w));
    w.front.say("main", ":report");
    assert!(notes(&w).last().unwrap().ends_with(&format!("\n~ {paragraph}")), "{:?}", notes(&w).last());
    w.close();
}

fn ev(seq: u64, kind: &str, task: Option<&str>, by: Actor, data: Value) -> j4n_core::log::Ev {
    j4n_core::log::Ev { seq, at_ms: seq, kind: kind.into(), task: task.map(String::from), thread: Some("main".into()), by, cause: None, class: Default::default(), data }
}

#[test]
fn a_doers_answer_is_quoted_to_the_model_and_is_no_fact_a_claim_can_rest_on() {
    // the scripted doer's answer is its own goal, which the operator typed; a doer that says something of its own is written by hand
    let events = vec![
        ev(1, "chat.say", None, Actor::Operator, json!({ "text": "fix the parser" })),
        ev(2, "task.queued", Some("t1"), Actor::Operator, json!({ "spec": { "doer": "pi", "goal": "fix the parser", "thread": "main" } })),
        ev(3, "task.settled", Some("t1"), Actor::Core, json!({ "ok": true, "answer": "Fixed in src/lexer.rs; 7 tests pass.", "changed": { "modified": ["src/parser.rs"] } })),
    ];
    let report = j4n_ledger::build::Report::of("main", events.into_iter());
    let facts = report.facts_all();
    assert!(facts.contains("src/parser.rs") && facts.contains("fix the parser") && !facts.contains("src/lexer.rs"), "{facts}");
    assert!(report.quoted().contains("> Fixed in src/lexer.rs; 7 tests pass."), "{}", report.quoted());
    assert_eq!(j4n_ledger::check::unknown("t1 fixed src/lexer.rs and 7 tests pass; src/parser.rs changed", &facts), ["src/lexer.rs", "7"]);
}

#[test]
fn the_claim_check_reads_paths_shas_numbers_and_test_names_against_the_facts() {
    let facts = "± 2 files: src/a.rs docs/B.md · ❱ 3 ran · `cargo test parser_cases` → 0 · ● commit 0123abc · $0.0420 · #1 t1";
    let cases: [(&str, &[&str]); 9] = [
        ("src/a.rs and docs/B.md changed", &[]),
        ("src/c.rs changed", &["src/c.rs"]),
        ("commit 0123abc landed; abc1234f did not", &["abc1234f"]),
        ("it cost $0.04, or 0.042", &[]),
        ("it cost $0.05", &["$0.05"]),
        ("3 ran, #1 by t1", &[]),
        ("4 ran", &["4"]),
        ("parser_cases passed, lexer_cases too", &["lexer_cases"]),
        ("two of three things, e.g. the work", &[]),
    ];
    for (paragraph, want) in cases {
        assert_eq!(j4n_ledger::check::unknown(paragraph, facts), want.iter().map(|w| w.to_string()).collect::<Vec<_>>(), "{paragraph}");
    }
}

fn answers(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(t, a)| (t.to_string(), a.to_string())).collect()
}

#[test]
fn two_answers_that_differ_are_kept_side_by_side_and_one_that_repeats_is_same_as() {
    // a repeat is shown to have been there; the two that each say something are both kept, marked; the merge decides nothing
    let m = merge(&answers(&[("t1", "Sure! The tests pass. The parser is fixed."), ("t2", "The tests pass."), ("t3", "Certainly, the tests fail. Three are red. One is flaky.")]));
    assert_eq!(m.len(), 3);
    assert_eq!((m[0].task.as_str(), m[0].text.as_str(), &m[0].mark, m[0].more), ("t1", "The tests pass. The parser is fixed.", &Mark::Differs, false));
    assert_eq!(m[1].mark, Mark::Same("t1".into()));
    assert_eq!((m[2].text.as_str(), &m[2].mark, m[2].more, m[2].full.as_str()), ("the tests fail. Three are red.", &Mark::Differs, true, "the tests fail. Three are red. One is flaky."));
    // one answer alone is not marked; a sentence twice in one answer is said once; case and spaces do not make another sentence
    let one = merge(&answers(&[("t1", "Done. It works.  it  WORKS. Fine")]));
    assert_eq!((one[0].text.as_str(), &one[0].mark, one[0].more), ("It works. Fine", &Mark::One, false));
    // an answer that is only an opener is shown as it is; the openers of the fixed list are cut, others are words
    assert_eq!(merge(&answers(&[("t1", "Done.")]))[0].text, "Done.");
    for (said, left) in [("Sure, here", "here"), ("Of course! Okay, x", "x"), ("I've done that. Then y", "Then y"), ("Surely not", "Surely not"), ("Okay", "Okay"), ("Great! z", "z")] {
        assert_eq!(trim_opener(said), left, "{said}");
    }
}

#[test]
fn two_answers_to_one_line_are_both_on_the_card_and_a_restart_rebuilds_the_same_report() {
    let d = Dirs::new("ledger-two-answers-one");
    let w = rig(&d, &[("ledger.mode", "\"on\"")], "");
    // the first turn leaves an item to do; the loop carries it on in its own session, and that turn answers the same line otherwise
    let first = start(&w, "say Alpha is done. Beta is next.; todo 1");
    ended(&w, &first);
    until(&w, "the follow-up", || w.front.tasks().book().rows().iter().any(|r| r.spec.follows.as_deref() == Some(first.as_str())));
    let next = w.front.tasks().book().rows().into_iter().find(|r| r.spec.follows.as_deref() == Some(first.as_str())).unwrap().id;
    w.front.plan_step("main");
    boundary(&w, &next);
    let reports = events(&w, "ledger.report");
    assert_eq!(reports.len(), 1, "{}", dump(&w));
    let card = reports[0]["card"].as_str().unwrap().to_string();
    assert!(card.contains(&format!("≠ {first}  Alpha is done. Beta is next.")), "{card}");
    assert!(card.contains(&format!("≠ {next}  ")), "{card}");
    assert_eq!(card.matches(" #").count(), 1, "one line, two answers under it: {card}");
    drop(w);
    // a J4n opened again on the same Log says the same report, and so does `j4n report`
    let w = open(&d);
    w.front.say("main", ":report");
    assert_eq!(notes(&w).last(), Some(&card), "{}", dump(&w));
    drop(w);
    // a command word says its answer on stdout, where `> file` takes it
    let out = j4n(&d, &["report", "main"], None, &[]);
    assert_eq!(out.rc, 0, "{}", out.err);
    assert!(out.out.contains(&card) && !out.err.contains(&card), "{}\n{}", out.out, out.err);
}
