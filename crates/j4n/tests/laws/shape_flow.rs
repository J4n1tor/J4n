//! Prompt shaping: the shaper and the judge are models behind a gateway on the loopback that says what each test
//! needs, and the doer is a model behind the same one, so what is checked is the real wire: the persona in the request, the state as
//! its goal, the checks without a model on what comes back, the card, the keys, and what the doer is finally sent.

use crate::models_cases::mock::{Mock, Say, Seen};
use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::front::Said;
use j4n_core::gate::grant::here_dir;
use j4n_proto::Row;
use serde_json::{Value, json};
use std::fs;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SHAPER: &str = "You read a line that someone is typing for a coding agent";
const JUDGE: &str = "You read notes that were added to a line";

/// What the gateway answers to the shaper, to the judge, and to the doer.
#[derive(Clone)]
struct Says {
    shaper: Arc<Mutex<String>>,
    judge: Arc<Mutex<String>>,
}

fn chat(text: &str) -> Say {
    Say::Sse(vec![
        json!({ "choices": [{ "delta": { "content": text } }] }).to_string(),
        json!({ "choices": [{ "delta": {}, "finish_reason": "stop" }] }).to_string(),
        json!({ "choices": [], "provider": "Cerebras", "usage": { "prompt_tokens": 12, "completion_tokens": 3 } }).to_string(),
    ])
}

fn system(s: &Seen) -> String {
    s.json()["messages"][0]["content"].as_str().unwrap_or_default().to_string()
}

fn user(s: &Seen) -> String {
    let m = s.json()["messages"].as_array().cloned().unwrap_or_default();
    m.iter().rev().find(|m| m["role"] == "user").and_then(|m| m["content"].as_str()).unwrap_or_default().to_string()
}

/// The requests the doer was sent, and those the shaper and the judge were.
fn doer_calls(g: &Mock) -> Vec<Seen> {
    g.seen().into_iter().filter(|s| !system(s).contains(SHAPER) && !system(s).contains(JUDGE)).collect()
}

fn shaper_calls(g: &Mock) -> Vec<Seen> {
    g.seen().into_iter().filter(|s| system(s).contains(SHAPER)).collect()
}

struct Rig {
    d: Dirs,
    w: World,
    gateway: Mock,
    says: Says,
}

fn git(d: &Dirs, args: &[&str]) {
    let out = Command::new("git").args(args).current_dir(d.project()).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// A project with a few files, a gateway, three model connectors on it (`gpt` does the work, `shaper` and `judge` shape), and the
/// pack `shape` on. `settings` are `id = value` lines for the operator's folder; `doer` is who the thread is pinned to.
fn rig(tag: &str, settings: &[(&str, &str)], doer: &str, judge: bool) -> Rig {
    rig_with(tag, settings, doer, judge, Approval::Deny)
}

fn rig_with(tag: &str, settings: &[(&str, &str)], doer: &str, judge: bool, approval: Approval) -> Rig {
    let d = Dirs::new(tag);
    for a in [&["init", "-q", "-b", "main"][..], &["config", "user.email", "t@t"], &["config", "user.name", "t"]] {
        git(&d, a);
    }
    for f in ["crates/cli/src/args.rs", "crates/cli/src/lexer.rs", "README.md"] {
        d.put(f, "// file\n");
    }
    git(&d, &["add", "."]);
    git(&d, &["commit", "-qm", "first"]);
    let says = Says { shaper: Arc::new(Mutex::new("{\"verdict\":\"pass\"}".into())), judge: Arc::new(Mutex::new("{\"items\":[],\"changes_ask\":false}".into())) };
    let (s2, j2) = (says.shaper.clone(), says.judge.clone());
    let gateway = Mock::start(move |req, _| {
        let sys = system(req);
        if sys.contains(SHAPER) {
            chat(&s2.lock().unwrap())
        } else if sys.contains(JUDGE) {
            chat(&j2.lock().unwrap())
        } else {
            chat("ok")
        }
    });
    let key = d.dir.join("key");
    crate::support::key_file(&key, "sk-test-shape-0001\n");
    fs::create_dir_all(d.j4n_home().join("connectors")).unwrap();
    let model = |id: &str| format!("[[connector]]\nid = \"{id}\"\nis = \"model\"\ndialect = \"openai\"\nurl = \"{}/v1\"\nkey = \"file:{}\"\nmodels = [\"vendor/m\"]\n", gateway.url(), key.display());
    fs::write(d.j4n_home().join("connectors/mine.toml"), format!("api = 1\n{}{}{}", model("gpt"), model("shaper"), model("judge"))).unwrap();
    fs::write(d.j4n_home().join("harness.toml"), "api = 1\n[[enable]]\nid = \"shape\"\n").unwrap();
    let here = here_dir(&d.j4n_home(), &d.project());
    fs::create_dir_all(&here).unwrap();
    let mut rows = String::from("api = 1\n[[setting]]\nid = \"shape.model\"\nvalue = \"shaper:vendor/m\"\n[[setting]]\nid = \"shape.judge\"\nvalue = \"");
    rows.push_str(if judge { "judge:vendor/m" } else { "off" });
    rows.push_str("\"\n");
    for (id, value) in settings {
        rows.push_str(&format!("[[setting]]\nid = \"{id}\"\nvalue = {value}\n"));
    }
    fs::write(here.join("settings.toml"), rows).unwrap();
    let w = open_with(&d, doer, false, approval);
    Rig { d, w, gateway, says }
}

fn open(d: &Dirs, doer: &str, shape: bool) -> World {
    open_with(d, doer, shape, Approval::Deny)
}

fn open_with(d: &Dirs, doer: &str, shape: bool, approval: Approval) -> World {
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    let opts = Opts { doer: Some(doer.into()), model: None, approval: Some(approval), wall: None, usd: None, mode: j4n_core::task::Mode::Write, place: None, shape, anywhere: false };
    crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"))
}

fn dump(w: &World) -> String {
    w.front.log().read(0).map(|e| format!("{:>4} {:<18} {} {}", e.seq, e.kind, e.task.as_deref().unwrap_or("-"), e.data)).collect::<Vec<_>>().join("\n")
}

fn kinds(w: &World, kind: &str) -> Vec<Value> {
    w.front.log().read(0).filter(|e| e.kind == kind).map(|e| e.data).collect()
}

fn started(w: &World, line: &str) -> String {
    let task = match w.front.say("main", line) {
        Said::Started { task } => task,
        other => panic!("{line:?} did not start a task: {other:?}\n{}", dump(w)),
    };
    w.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap_or_else(|| panic!("{task} never ended\n{}", dump(w)));
    task
}

fn state_of(rows: &[Row]) -> String {
    rows.first().and_then(|r| r.cells.get(1)).map(|c| c.text.clone()).unwrap_or_default()
}

/// Put a draft to the front and wait until the card says something other than `working`.
fn drafted(w: &World, text: &str) -> Vec<Row> {
    w.front.draft("main", text, "");
    let desk = w.front.desk().expect("the pack shape is on");
    let end = Instant::now() + crate::support::PATIENCE;
    loop {
        let rows = desk.card("main");
        if !rows.is_empty() && state_of(&rows) != "working" {
            return rows;
        }
        assert!(Instant::now() < end, "the card never settled: {rows:?}\n{}", dump(w));
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn texts(rows: &[Row]) -> Vec<String> {
    rows.iter().map(|r| r.cells.iter().map(|c| c.text.clone()).collect::<Vec<_>>().join(" | ")).collect()
}

fn on(w: &World) {
    w.front.say("main", ":shape on");
}

#[test]
fn with_the_switch_off_the_doer_gets_the_line_byte_for_byte_and_nothing_is_written() {
    let r = rig("shape-switch-off-doer", &[], "gpt", true);
    r.w.front.draft("main", "fix the parser thing", "");
    started(&r.w, "fix the parser thing");
    assert!(shaper_calls(&r.gateway).is_empty(), "the shaper was never asked");
    let doer = doer_calls(&r.gateway);
    assert_eq!(doer.len(), 1);
    assert_eq!(user(&doer[0]), "fix the parser thing");
    assert!(!system(&doer[0]).contains("## shape"), "{}", system(&doer[0]));
    assert!(r.w.front.log().read(0).all(|e| !e.kind.starts_with("shape.")), "no shape event\n{}", dump(&r.w));
    assert_eq!(kinds(&r.w, "brief.rendered")[0]["sections"], json!(["contract", "project"]), "the project card is on by default");
    r.w.close();
}

#[test]
fn a_shaper_that_says_pass_leaves_the_line_alone_and_one_result_is_written() {
    let r = rig("shape-shaper-says-pass", &[], "gpt", true);
    on(&r.w);
    let rows = drafted(&r.w, "add a --verbose flag to the cli");
    assert_eq!(state_of(&rows), "pass", "{:?}", texts(&rows));
    assert_eq!(rows.len(), 1, "no card rows for a pass");
    started(&r.w, "add a --verbose flag to the cli");
    assert!(!dump(&r.w).contains("goes as typed"), "a line with no rows kept asks the desk for nothing\n{}", dump(&r.w));
    assert_eq!(user(&doer_calls(&r.gateway)[0]), "add a --verbose flag to the cli");
    assert_eq!(kinds(&r.w, "brief.rendered")[0]["sections"], json!(["contract", "project"]), "the project card is on by default");
    let results = kinds(&r.w, "shape.result");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["verdict"], "pass");
    // the persona rode in the Brief of the request, and the state was its goal
    let asked = &shaper_calls(&r.gateway)[0];
    assert!(user(asked).contains("\"repo_map\"") && user(asked).contains("crates/cli/src/args.rs"), "{}", user(asked));
    assert!(system(asked).contains(SHAPER) && !system(asked).contains("For the rewrite"), "the tone is for a rewrite only: {}", system(asked));
    r.w.close();
}

#[test]
fn what_does_not_hold_is_dropped_with_its_reason_and_the_one_good_file_is_shown() {
    let r = rig("shape-what-does-not", &[], "gpt", false);
    on(&r.w);
    started(&r.w, "trailing commas in args break the parser");
    *r.says.shaper.lock().unwrap() = json!({
        "verdict": "add",
        "refs": [{ "phrase": "the parser thing", "turn": "t1", "quote": "nested quotes" }],
        "files": [{ "path": "crates/cli/src/missing.rs", "why": "t1" }, { "path": "crates/cli/src/args.rs", "why": "t1" }],
        "notes": [
            { "text": "MAKE SURE THE TESTS PASS", "why": "t1", "quote": "trailing commas" },
            { "text": "only touch args.rs", "why": "t1", "quote": "trailing commas in args" }
        ]
    })
    .to_string();
    let rows = drafted(&r.w, "fix the parser thing and make tests pass");
    assert_eq!(state_of(&rows), "ready", "{:?}", texts(&rows));
    let kept: Vec<&str> = rows.iter().filter(|r| !r.key.starts_with('~') && !r.key.starts_with("x:")).map(|r| r.key.as_str()).collect();
    assert_eq!(kept, ["file:1"], "{:?}", texts(&rows));
    let why: Vec<String> = rows.iter().filter(|r| r.key.starts_with("x:")).map(|r| r.cells[2].text.clone()).collect();
    for want in ["the quote is not in that turn", "no such file", "emphasis the operator did not type", "a limit the operator did not state"] {
        assert!(why.iter().any(|w| w == want), "{want} is not among {why:?}");
    }
    assert_eq!(why.len(), 4);
    r.w.close();
}

#[test]
fn the_operator_keeps_one_of_two_rows_and_the_doer_gets_exactly_that_one_and_an_old_key_is_refused() {
    let r = rig("shape-operator-keeps-one", &[], "gpt", false);
    on(&r.w);
    *r.says.shaper.lock().unwrap() = json!({ "verdict": "add", "files": [{ "path": "crates/cli/src/args.rs", "why": "t1" }, { "path": "crates/cli/src/lexer.rs", "why": "t1" }] }).to_string();
    let rows = drafted(&r.w, "fix the parser thing");
    assert_eq!(rows.iter().filter(|r| r.key.starts_with("file:")).count(), 2);
    let said = r.w.front.say_with("main", "fix the parser thing", &["file:2".to_string()]);
    let Said::Started { task } = said else { panic!("{said:?}\n{}", dump(&r.w)) };
    r.w.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap();
    let req = &doer_calls(&r.gateway)[0];
    assert_eq!(user(req), "fix the parser thing", "the line is what was typed");
    let sys = system(req);
    assert!(sys.contains("## shape") && sys.contains("Likely relevant: crates/cli/src/lexer.rs"), "{sys}");
    assert!(!sys.contains("args.rs"), "only the row that was kept: {sys}");
    let said: Vec<String> = kinds(&r.w, "chat.say").iter().map(|d| d["text"].as_str().unwrap().to_string()).collect();
    assert!(said.contains(&"fix the parser thing".to_string()), "{said:?}");
    assert_eq!(kinds(&r.w, "shape.kept")[0]["keys"], json!(["file:2"]));
    // a key of the result for another line is refused with a notice, and the line goes as typed
    let task = match r.w.front.say_with("main", "something else entirely", &["file:1".to_string()]) {
        Said::Started { task } => task,
        other => panic!("{other:?}"),
    };
    r.w.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap();
    let last = doer_calls(&r.gateway).pop().unwrap();
    assert!(!system(&last).contains("## shape") && user(&last) == "something else entirely");
    let notes: Vec<String> = kinds(&r.w, "chat.note").iter().map(|d| d["text"].as_str().unwrap_or_default().to_string()).collect();
    assert!(notes.iter().any(|n| n.contains("another draft") && n.contains("as typed")), "{notes:?}");
    r.w.close();
}

#[test]
fn an_answer_a_command_a_short_reply_and_a_paste_are_never_shaped() {
    let r = rig_with("shape-answer-command-short", &[], "test", false, Approval::Ask);
    on(&r.w);
    // the doer asks a question: the next line would be its answer
    let ask = match r.w.front.say("main", "ask Shall I go on?") {
        Said::Started { task } => task,
        other => panic!("{other:?}"),
    };
    let end = Instant::now() + crate::support::PATIENCE;
    while r.w.front.tasks().book().open_asks().is_empty() {
        assert!(Instant::now() < end, "no question\n{}", dump(&r.w));
        std::thread::sleep(Duration::from_millis(10));
    }
    let rows = drafted(&r.w, "use the first approach");
    assert_eq!(state_of(&rows), "skip");
    assert!(rows[0].cells[2].text.contains("answer"), "{:?}", texts(&rows));
    let a = r.w.front.tasks().book().open_asks().remove(0);
    r.w.front.reply("main", &a.ask, j4n_proto::Reply::Text { text: "the first approach".into() });
    r.w.front.tasks().book().wait(&ask, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap();
    for (line, why) in [("do it", "short reply"), (":fresh", "command"), ("/plan add a flag", "command"), (&"word ".repeat(1000), "paste"), ("!ls -la", "shell"), ("$ ls -la", "shell")] {
        let rows = drafted(&r.w, line);
        assert_eq!(state_of(&rows), "skip", "{line:?}: {:?}", texts(&rows));
        assert!(rows[0].cells[2].text.contains(why), "{line:?}: {:?}", texts(&rows));
    }
    assert!(r.gateway.seen().is_empty(), "the shaper was asked about none of them");
    assert!(kinds(&r.w, "shape.result").is_empty());
    r.w.close();
}

#[test]
fn a_line_to_a_doer_that_missed_the_turns_is_shaped_with_them_and_the_same_line_to_the_one_that_has_them_is_not() {
    let r = rig("shape-line-doer-missed", &[], "test", false);
    on(&r.w);
    started(&r.w, "say the fix goes in lexer.rs");
    // the doer that has the thread: a short reply to it is not shaped
    let rows = drafted(&r.w, "do it");
    assert_eq!(state_of(&rows), "skip", "{:?}", texts(&rows));
    // another doer takes over: it never received the turn
    r.w.front.say("main", ":switch gpt vendor/m");
    *r.says.shaper.lock().unwrap() = json!({ "verdict": "add", "refs": [{ "phrase": "it", "turn": "t1", "quote": "the fix goes in lexer.rs" }] }).to_string();
    let rows = drafted(&r.w, "do it");
    assert_eq!(state_of(&rows), "ready", "{:?}", texts(&rows));
    assert!(rows.iter().any(|r| r.key == "ref:1" && r.cells[1].text.contains("t1")), "{:?}", texts(&rows));
    let asked = user(&shaper_calls(&r.gateway)[0]);
    assert!(asked.contains("the fix goes in lexer.rs") && asked.contains("\"received_turns\":false"), "{asked}");
    // it takes a turn, and the thread is back with a doer that has a session of its own: "do it" is a short reply again
    started(&r.w, "add a --keep flag");
    r.w.front.say("main", ":switch test");
    started(&r.w, "say the flag is --keep");
    let n = shaper_calls(&r.gateway).len();
    let rows = drafted(&r.w, "do it");
    assert_eq!(state_of(&rows), "skip", "{:?}", texts(&rows));
    assert_eq!(shaper_calls(&r.gateway).len(), n);
    r.w.close();
}

#[test]
fn a_rewrite_that_loses_a_path_of_the_line_is_dropped_and_the_notes_stay() {
    let r = rig("shape-rewrite-loses-path", &[("shape.style", "\"rewrite\""), ("shape.words", "\"name the test command\"")], "gpt", false);
    on(&r.w);
    let line = "fix crates/cli/src/args.rs parse errors";
    *r.says.shaper.lock().unwrap() = json!({ "verdict": "add", "files": [{ "path": "crates/cli/src/args.rs", "why": "t1" }], "rewrite": "Fix the parse errors in the argument parser." }).to_string();
    let rows = drafted(&r.w, line);
    assert!(!rows.iter().any(|r| r.key == "rewrite"), "{:?}", texts(&rows));
    let persona = system(&shaper_calls(&r.gateway)[0]);
    assert!(persona.contains("For the rewrite, fix typos and grammar") && persona.contains("The operator's own instruction: name the test command"), "{persona}");
    assert!(rows.iter().any(|r| r.key == "file:1"), "the notes stay: {:?}", texts(&rows));
    assert!(rows.iter().any(|r| r.key.starts_with("x:") && r.cells[2].text.contains("lost crates/cli/src/args.rs")), "{:?}", texts(&rows));
    started(&r.w, line);
    assert_eq!(user(&doer_calls(&r.gateway)[0]), line, "the line goes as typed");
    // one that keeps the path is offered, and the typed line follows it
    *r.says.shaper.lock().unwrap() = json!({ "verdict": "add", "rewrite": "Fix the parse errors in crates/cli/src/args.rs." }).to_string();
    let line2 = "fix crates/cli/src/args.rs parse errors please";
    let rows = drafted(&r.w, line2);
    assert!(rows.iter().any(|r| r.key == "rewrite"), "{:?}", texts(&rows));
    let said = r.w.front.say_with("main", line2, &["rewrite".to_string()]);
    let Said::Started { task } = said else { panic!("{said:?}") };
    r.w.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap();
    let req = doer_calls(&r.gateway).pop().unwrap();
    assert!(system(&req).contains("Clean version of the line: Fix the parse errors in crates/cli/src/args.rs.") && system(&req).contains(&format!("As typed: \"{line2}\"")), "{}", system(&req));
    assert_eq!(user(&req), line2);
    r.w.close();
}

#[test]
fn a_run_shapes_only_when_asked_to_and_prints_what_was_added_to_stderr_while_stdout_stays_the_answer() {
    let r = rig("shape-run-shapes-only", &[], "gpt", false);
    *r.says.shaper.lock().unwrap() = json!({
        "verdict": "ask",
        "files": [{ "path": "crates/cli/src/args.rs", "why": "t1" }],
        "question": { "text": "Which one?", "options": ["the commas", "the quotes"] }
    })
    .to_string();
    // without the flag: the shaper is never asked
    let opts = Opts { doer: Some("gpt".into()), model: None, approval: Some(Approval::Deny), wall: None, usd: None, mode: j4n_core::task::Mode::Write, place: None, shape: false, anywhere: false };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    j4n::lines::drive(&r.w, &opts, &mut "fix the parser\n".as_bytes(), &mut out, &mut err, false);
    assert!(shaper_calls(&r.gateway).is_empty() && !String::from_utf8_lossy(&err).contains("shape:"), "{}", String::from_utf8_lossy(&err));
    r.w.close();
    drop(r.w);
    // with it: the checked items are kept, the question is dropped, the report lists them
    let w = open(&r.d, "gpt", true);
    let opts = Opts { shape: true, ..opts };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    j4n::lines::drive(&w, &opts, &mut "fix the parser\n".as_bytes(), &mut out, &mut err, false);
    let (out, err) = (String::from_utf8_lossy(&out).to_string(), String::from_utf8_lossy(&err).to_string());
    assert!(err.contains("shape: add · kept 1 · dropped 1") && err.contains("+ file") && err.contains("crates/cli/src/args.rs") && err.contains("nobody is there to answer it"), "{err}");
    assert!(out.contains("ok") && !out.contains("shape:"), "stdout is the answer: {out}");
    let req = doer_calls(&r.gateway).pop().unwrap();
    assert!(system(&req).contains("Likely relevant: crates/cli/src/args.rs") && !system(&req).contains("Which one?"), "{}", system(&req));
    w.close();
}

#[test]
fn a_shaper_that_answers_text_leaves_the_line_as_typed_and_a_third_failure_in_a_row_opens_the_breaker() {
    let r = rig("shape-shaper-answers-text", &[], "gpt", false);
    on(&r.w);
    *r.says.shaper.lock().unwrap() = "I think this line is fine as it is.".into();
    for n in 1..=3 {
        let rows = drafted(&r.w, &format!("fix the parser thing {n}"));
        assert_eq!(state_of(&rows), "failed", "{:?}", texts(&rows));
        assert!(rows[0].cells[2].text.contains("not JSON"), "{:?}", texts(&rows));
    }
    started(&r.w, "fix the parser thing 3");
    let last = doer_calls(&r.gateway).pop().unwrap();
    assert_eq!(user(&last), "fix the parser thing 3");
    assert_eq!(shaper_calls(&r.gateway).len(), 3);
    let rows = drafted(&r.w, "fix the parser thing 4");
    assert_eq!(state_of(&rows), "failed");
    assert!(rows[0].cells[2].text.contains("off until `:shape on`"), "{:?}", texts(&rows));
    assert_eq!(shaper_calls(&r.gateway).len(), 3, "the breaker is open: the shaper is not asked");
    // the operator says so again, and it is tried
    *r.says.shaper.lock().unwrap() = "{\"verdict\":\"pass\"}".into();
    on(&r.w);
    assert_eq!(state_of(&drafted(&r.w, "fix the parser thing 4")), "pass", "a failure is not remembered: the same line is tried again");
    assert_eq!(shaper_calls(&r.gateway).len(), 4);
    r.w.close();
}

#[test]
fn the_switch_is_in_the_log_and_is_still_on_when_j4n_is_started_again() {
    let r = rig("shape-switch-log-still", &[], "gpt", false);
    assert!(!r.w.front.desk().unwrap().on());
    on(&r.w);
    assert!(r.w.front.desk().unwrap().on());
    assert_eq!(kinds(&r.w, "shape.switch"), vec![json!({ "on": true })]);
    r.w.close();
    drop(r.w);
    let w = open(&r.d, "gpt", false);
    assert!(w.front.desk().unwrap().on(), "{}", dump(&w));
    w.front.say("main", ":shape off");
    assert!(!w.front.desk().unwrap().on());
    w.close();
}

#[test]
fn the_screen_types_a_line_sees_the_card_and_one_enter_sends_it_with_the_rows() {
    use crate::screen_drive::Operator;
    use j4n_tui::headless::{press, typed};
    let r = rig("shape-screen-types-line", &[], "gpt", false);
    on(&r.w);
    *r.says.shaper.lock().unwrap() = json!({ "verdict": "add", "files": [{ "path": "crates/cli/src/args.rs", "why": "t1" }, { "path": "crates/cli/src/lexer.rs", "why": "t1" }] }).to_string();
    let mut op = Operator::sit(&r.w);
    // the core has the switch on first and tells the screen after: the pause is the screen's, and it counts only once the screen knows
    op.until("the switch", |_| op_on(&r.w));
    op.until_app("the screen told of the switch", |app| app.composer.on);
    op.press(typed("fix the parser thing", false));
    // the pause: the draft stands still for 800 ms, and then it is sent
    let t0 = Instant::now();
    for at in [0, 1_000] {
        for u in op.app.tick(t0 + Duration::from_millis(at)) {
            op.up.send(u).unwrap();
        }
    }
    let shown = op.until("the card", |s| s.contains("crates/cli/src/args.rs") && s.contains("send with notes"));
    assert!(shown.contains("crates/cli/src/lexer.rs"), "{shown}");
    // the operator drops the second row with ⌃o, down and space, and sends with one Enter
    op.press(vec![j4n_tui::headless::ctrl('o'), press(j4n_tui::headless::KeyCode::Down), press(j4n_tui::headless::KeyCode::Char(' ')), press(j4n_tui::headless::KeyCode::Enter)]);
    op.until("the second row dropped", |s| s.contains("· file") && s.contains("lexer.rs"));
    op.press(vec![press(j4n_tui::headless::KeyCode::Enter)]);
    let end = Instant::now() + crate::support::PATIENCE;
    while doer_calls(&r.gateway).is_empty() {
        assert!(Instant::now() < end, "the line never went\n{}", dump(&r.w));
        std::thread::sleep(Duration::from_millis(20));
    }
    let req = &doer_calls(&r.gateway)[0];
    assert_eq!(user(req), "fix the parser thing");
    assert!(system(req).contains("Likely relevant: crates/cli/src/args.rs") && !system(req).contains("lexer.rs"), "{}", system(req));
    r.w.close();
}

fn op_on(w: &World) -> bool {
    w.front.desk().is_some_and(|d| d.on())
}

#[test]
fn a_judge_drops_the_item_it_finds_unfaithful_and_a_draft_that_changes_the_ask_goes_back_once_and_then_loses_its_rows() {
    let r = rig("shape-judge-drops-item", &[], "gpt", true);
    on(&r.w);
    started(&r.w, "trailing commas in args break the parser");
    *r.says.shaper.lock().unwrap() = json!({
        "verdict": "add",
        "refs": [{ "phrase": "the parser thing", "turn": "t1", "quote": "trailing commas" }],
        "files": [{ "path": "crates/cli/src/args.rs", "why": "t1" }]
    })
    .to_string();
    *r.says.judge.lock().unwrap() = json!({ "items": [{ "id": "ref:1", "faithful": false, "why": "the turn is about something else" }, { "id": "file:1", "faithful": true, "why": "" }], "changes_ask": false }).to_string();
    let rows = drafted(&r.w, "fix the parser thing");
    assert_eq!(rows.iter().filter(|r| !r.key.starts_with('~') && !r.key.starts_with("x:")).map(|r| r.key.as_str()).collect::<Vec<_>>(), ["file:1"], "{:?}", texts(&rows));
    assert!(rows.iter().any(|r| r.key.starts_with("x:") && r.cells[2].text == "the judge: the turn is about something else"), "{:?}", texts(&rows));
    assert!(rows.iter().any(|r| r.key == "~judge" && r.cells[1].text.contains("some not faithful")), "{:?}", texts(&rows));
    // a judge that says the notes change the ask sends the draft back once; a second time the rows are lost
    *r.says.judge.lock().unwrap() = json!({ "items": [], "changes_ask": true }).to_string();
    let rows = drafted(&r.w, "fix the other parser thing");
    assert_eq!(state_of(&rows), "pass", "{:?}", texts(&rows));
    assert_eq!(shaper_calls(&r.gateway).len(), 3, "the draft was sent back once: two rounds");
    assert!(rows.iter().filter(|r| r.key.starts_with("x:")).all(|r| r.cells[2].text == "the judge: it changes the ask"), "{:?}", texts(&rows));
    let back = user(&shaper_calls(&r.gateway)[2]);
    assert!(back.contains("The judge sent the draft back"), "{back}");
    assert_eq!(kinds(&r.w, "shape.result").last().unwrap()["rounds"], 2);
    r.w.close();
}

#[test]
fn a_line_the_operator_listed_in_shape_skip_is_never_shaped_whatever_its_case_and_spacing() {
    let r = rig("shape-line-operator-listed", &[("shape.skip", "\"ship it, Looks Good, please go ahead with it now\"")], "gpt", false);
    on(&r.w);
    started(&r.w, "add a flag");
    for line in ["SHIP   it", "looks good", "Please  go ahead   with it NOW"] {
        let rows = drafted(&r.w, line);
        assert_eq!(state_of(&rows), "skip", "{line}: {:?}", texts(&rows));
    }
    assert!(shaper_calls(&r.gateway).is_empty());
    // a line that only holds one of them is shaped
    assert_ne!(state_of(&drafted(&r.w, "ship it to the staging box")), "skip");
    // a paste is more than 4,000 characters, not 4,000
    let line = |n: usize| format!("fix {}", "x".repeat(n - 4));
    assert_ne!(state_of(&drafted(&r.w, &line(4_000))), "skip");
    assert_eq!(state_of(&drafted(&r.w, &line(4_001))), "skip");
    r.w.close();
}

#[test]
fn a_question_is_kept_only_once_an_option_is_picked_and_dropped_notes_are_not_sent() {
    let r = rig("shape-question-kept-only", &[], "gpt", false);
    on(&r.w);
    *r.says.shaper.lock().unwrap() = json!({ "verdict": "ask", "files": [{ "path": "crates/cli/src/args.rs", "why": "t1" }], "question": { "text": "Which one?", "options": ["the commas", "the quotes"] } }).to_string();
    let rows = drafted(&r.w, "fix the parser thing");
    let tone = |rows: &[Row], key: &str| rows.iter().find(|r| r.key == key).unwrap().tone;
    let desk = r.w.front.desk().unwrap().clone();
    assert_eq!((tone(&rows, "file:1"), tone(&rows, "question")), (j4n_proto::Tone::Good, j4n_proto::Tone::Warn), "an unanswered question is not kept");
    assert!(r.w.front.say("main", ":shape pick 3") != Said::Nothing);
    assert_eq!(tone(&desk.card("main"), "question"), j4n_proto::Tone::Warn, "an option that is not there picks nothing");
    r.w.front.say("main", ":shape pick 2");
    let rows = desk.card("main");
    assert_eq!(tone(&rows, "question"), j4n_proto::Tone::Good);
    assert_eq!(rows.iter().find(|r| r.key == "question").unwrap().detail.iter().map(|c| c.tone).collect::<Vec<_>>(), [j4n_proto::Tone::Plain, j4n_proto::Tone::Good]);
    // `toggle` drops a row and takes it back
    r.w.front.say("main", ":shape toggle file:1");
    assert_eq!(tone(&desk.card("main"), "file:1"), j4n_proto::Tone::Dim);
    r.w.front.say("main", ":shape toggle file:1");
    assert_eq!(tone(&desk.card("main"), "file:1"), j4n_proto::Tone::Good);
    let said = r.w.front.say_with("main", "fix the parser thing", &["file:1".to_string(), "question".to_string()]);
    let Said::Started { task } = said else { panic!("{said:?}") };
    r.w.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap();
    let sys = system(&doer_calls(&r.gateway)[0]);
    assert!(sys.contains("Which one? the quotes") && sys.contains("crates/cli/src/args.rs"), "{sys}");
    // notes that were dropped are not sent, whatever keys come with the line
    drafted(&r.w, "fix the other parser thing");
    r.w.front.say("main", ":shape drop");
    assert_eq!(state_of(&desk.card("main")), "dropped");
    let said = r.w.front.say_with("main", "fix the other parser thing", &["file:1".to_string()]);
    let Said::Started { task } = said else { panic!("{said:?}") };
    r.w.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap();
    let last = doer_calls(&r.gateway).pop().unwrap();
    assert!(!system(&last).contains("## shape"), "{}", system(&last));
    r.w.close();
}

/// A project and a gateway, the pack `shape` on (and the pack `jev`, which declares the doer `light`, when `jev`), and **no** row of `shape.model` or `shape.judge`: the
/// defaults are what runs. The home's own row for `light` points it at the gateway, as the key of the operator would.
fn defaults_rig(tag: &str, jev: bool) -> Rig {
    let d = Dirs::new(tag);
    if !jev {
        d.pack_off("jev");
    }
    for a in [&["init", "-q", "-b", "main"][..], &["config", "user.email", "t@t"], &["config", "user.name", "t"]] {
        git(&d, a);
    }
    d.put("crates/cli/src/args.rs", "// file\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-qm", "first"]);
    let says = Says { shaper: Arc::new(Mutex::new("{\"verdict\":\"pass\"}".into())), judge: Arc::new(Mutex::new("{\"items\":[],\"changes_ask\":false}".into())) };
    let (s2, j2) = (says.shaper.clone(), says.judge.clone());
    let gateway = Mock::start(move |req, _| {
        let sys = system(req);
        if sys.contains(SHAPER) {
            chat(&s2.lock().unwrap())
        } else if sys.contains(JUDGE) {
            chat(&j2.lock().unwrap())
        } else {
            chat("ok")
        }
    });
    let key = d.dir.join("key");
    crate::support::key_file(&key, "sk-test-shape-0001\n");
    fs::create_dir_all(d.j4n_home().join("connectors")).unwrap();
    let light = if jev { format!("[[connector]]\nid = \"light\"\nurl = \"{}/v1\"\nkey = \"file:{}\"\nmodels = [\"vendor/m\", \"anthropic/claude-haiku-5.5\", \"deepseek/deepseek-v4.1-flash\"]\n", gateway.url(), key.display()) } else { String::new() };
    fs::write(d.j4n_home().join("connectors/mine.toml"), format!("api = 1\n{light}[[connector]]\nid = \"gpt\"\nis = \"model\"\ndialect = \"openai\"\nurl = \"{}/v1\"\nkey = \"file:{}\"\nmodels = [\"vendor/m\"]\n", gateway.url(), key.display())).unwrap();
    fs::write(d.j4n_home().join("harness.toml"), format!("api = 1\n[[enable]]\nid = \"shape\"\n{}", if jev { "[[enable]]\nid = \"jev\"\n" } else { "" })).unwrap();
    let w = open_with(&d, "gpt", false, Approval::Deny);
    Rig { d, w, gateway, says }
}

#[test]
fn the_defaults_of_shape_name_a_doer_that_exists_so_the_shaper_runs_with_no_row_of_the_operators() {
    let r = defaults_rig("shape-defaults-name-doer", true);
    on(&r.w);
    let rows = drafted(&r.w, "add a --verbose flag to the cli");
    assert_eq!(state_of(&rows), "pass", "{:?}\n{}", texts(&rows), dump(&r.w));
    let asked = shaper_calls(&r.gateway);
    assert_eq!(asked.len(), 1, "the shaper was asked once, through `light`");
    assert_eq!(asked[0].json()["model"], "anthropic/claude-haiku-5.5", "the default model of `shape.model` went to the doer");
    r.w.close();
}

#[test]
fn with_the_pack_that_declares_the_default_doer_off_the_line_says_which_pack_and_where_to_turn_it_on() {
    let r = defaults_rig("shape-off", false);
    on(&r.w);
    let rows = drafted(&r.w, "add a --verbose flag to the cli");
    let said = texts(&rows).join("\n");
    assert_eq!(state_of(&rows), "failed", "{said}");
    assert!(said.contains("`light`") && said.contains("pack jev") && said.contains("Settings › Packs › jev"), "the reason names the doer, its pack and the way to turn it on: {said}\n{}", dump(&r.w));
    assert!(shaper_calls(&r.gateway).is_empty() && doer_calls(&r.gateway).is_empty(), "nothing was sent");
    r.w.close();
}
