//! Through the binary's own wiring: a model, a decider and a gateway written as connector files, a gateway on the
//! loopback that answers as a script says. What is checked is J4n: the key never reaches a file, the proxy of an account is the
//! one used, a point answers by its fallback when no key is there, and a decider acts only once it has passed its cases.

use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
#[allow(dead_code)]
#[path = "../../../j4n-connect/tests/connect/mock.rs"]
pub mod mock;

use mock::{Mock, Proxy as Fixture, Say};
use serde_json::{Value, json};
use std::fs;
use std::time::{Duration, Instant};

fn open(d: &Dirs, connectors: &str, opts: Opts) -> World {
    open_as(d, connectors, opts, false)
}

/// [`open`] for a screen (`interactive`) or not (a run, a pipe).
fn open_as(d: &Dirs, connectors: &str, opts: Opts, interactive: bool) -> World {
    fs::create_dir_all(d.j4n_home().join("connectors")).unwrap();
    fs::write(d.j4n_home().join("connectors/mine.toml"), format!("api = 1\n{connectors}")).unwrap();
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    wire::open(setup, &opts, interactive, "main").unwrap_or_else(|e| panic!("opens: {e}"))
}

fn opts(doer: Option<&str>) -> Opts {
    Opts { doer: doer.map(String::from), model: None, approval: Some(Approval::Deny), wall: None, usd: None, mode: j4n_core::task::Mode::Write, place: None, shape: false, anywhere: false }
}

fn key(d: &Dirs, text: &str) -> String {
    let p = d.dir.join("key");
    crate::support::key_file(&p, &format!("{text}\n"));
    format!("file:{}", p.display())
}

fn chat(text: &str) -> Say {
    Say::Sse(vec![
        json!({ "choices": [{ "delta": { "content": text } }] }).to_string(),
        json!({ "choices": [{ "delta": {}, "finish_reason": "stop" }] }).to_string(),
        json!({ "choices": [], "provider": "Cerebras", "usage": { "prompt_tokens": 12, "completion_tokens": 3 } }).to_string(),
    ])
}

fn model(url: &str, key: &str, extra: &str) -> String {
    format!("[[connector]]\nid = \"gpt\"\nis = \"model\"\ndialect = \"openai\"\nurl = \"{url}\"\nkey = \"{key}\"\nmodels = [\"vendor/m\"]\n{extra}\n")
}

fn say_and_wait(w: &World, line: &str) -> j4n_core::task::Row {
    let j4n_core::front::Said::Started { task } = w.front.say("main", line) else { panic!("{line:?} did not start a task") };
    w.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap_or_else(|| panic!("{task} never ended"))
}

fn files_under(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() { files_under(&p, out) } else { out.push(p) }
    }
}

#[test]
fn a_model_is_one_file_and_answers_through_the_front_and_its_key_is_in_no_file() {
    let d = Dirs::new("model");
    let gateway = Mock::start(|_, _| chat("Four."));
    let k = key(&d, "sk-live-1234567890abcdef");
    let w = open(&d, &model(&format!("{}/v1", gateway.url()), &k, ""), opts(Some("gpt")));
    let row = say_and_wait(&w, "what is 2+2?");
    assert_eq!(row.answer.as_deref(), Some("Four."));
    let req = &gateway.seen()[0];
    assert_eq!(req.headers.get("authorization").map(String::as_str), Some("Bearer sk-live-1234567890abcdef"));
    let b = req.json();
    assert_eq!(b["model"], "vendor/m");
    assert!(b["session_id"].as_str().is_some() && b.get("provider").is_none(), "{b}");
    w.close();
    let mut all = Vec::new();
    files_under(&d.project().join(".j4n"), &mut all);
    files_under(&d.j4n_home(), &mut all);
    for f in all.iter().filter(|f| f.file_name().is_some_and(|n| n != "key")) {
        assert!(!fs::read_to_string(f).unwrap_or_default().contains("sk-live-1234567890abcdef"), "the key is in {}", f.display());
    }
}

#[test]
fn a_connector_with_an_accounts_proxy_leaves_by_it_and_the_system_proxy_sees_nothing() {
    let d = Dirs::new("proxy");
    let gateway = Mock::start(|_, _| chat("via the account"));
    let account = Fixture::start(gateway.addr);
    let system = Fixture::start(gateway.addr);
    let k = key(&d, "k-account-0001");
    let url = format!("http://gateway.test:{}/v1", gateway.addr.port());
    let w = open(&d, &model(&url, &k, &format!("proxy = \"{}\"", account.url())), opts(Some("gpt")));
    let _ = system;
    let row = say_and_wait(&w, "hello");
    assert_eq!(row.answer.as_deref(), Some("via the account"));
    assert_eq!(account.calls(), [format!("http://gateway.test:{}/v1/chat/completions", gateway.addr.port())]);
    assert_eq!(gateway.seen().len(), 1);
    // the system proxy of these tests is a dead address: a call that had used it would have failed
    w.close();
}

#[test]
fn a_model_without_its_key_is_a_doer_that_is_not_logged_in_and_nothing_is_sent() {
    let d = Dirs::new("nokey");
    let gateway = Mock::start(|_, _| chat("never"));
    let w = open(&d, &model(&format!("{}/v1", gateway.url()), "file:/no/such/key", ""), opts(Some("gpt")));
    let row = say_and_wait(&w, "hello");
    assert!(!matches!(row.state, j4n_core::task::State::Done { ok: true }), "{:?}", row.state);
    assert!(gateway.seen().is_empty());
    assert!(format!("{:?}", row.state).contains("there is no key"), "{:?}", row.state);
    w.close();
}

// ---- the deciders ----

/// A Jev that decides by the words of the question, as a person would for the cases of the base pack.
fn jev(n: &mut u32, body: &Value) -> Value {
    *n += 1;
    let q = &body["questions"]["q"];
    let text = q["instructions"].as_str().unwrap_or_default().to_lowercase();
    let keys: Vec<&str> = q["criteria"].as_object().map(|m| m.keys().map(String::as_str).collect()).unwrap_or_default();
    let pick = if keys.contains(&"replace") {
        if text.contains("sqlite") {
            "replace"
        } else if text.contains("which file does") {
            "answer"
        } else if text.contains("not in cli.rs") || text.contains("forget the test") {
            "change"
        } else {
            "add"
        }
    } else if keys.contains(&"new") {
        // `chats`: which chat a line belongs to, or a new one; only the line is read, the options below it name the chats
        let line = text.lines().next().unwrap_or_default();
        if line.contains("parser") {
            "c1"
        } else if line.contains("helper") {
            "c2"
        } else {
            "new"
        }
    } else if keys.contains(&"none") {
        // `librarian`: which of the skills that fit alike; only the line is read, the slice below it names both skills of each pair
        let line = text.lines().next().unwrap_or_default();
        if line.contains("cargo") {
            "rust-tests"
        } else if line.contains("pytest") {
            "python-tests"
        } else if line.contains("release notes") {
            "release-notes"
        } else if line.contains("changelog") {
            "changelog"
        } else {
            "none"
        }
    } else if keys.contains(&"1") && keys.contains(&"2") {
        // `complete`: which candidate the operator means to type; the chat's last turn says what was just done, and so which twin it is
        if ["rebuild the docs", "tag the release when", "tests live"].iter().any(|w| text.contains(w)) { "2" } else { "1" }
    } else if keys.contains(&"pi") {
        if text.contains("rename") { "pi" } else { "claude" }
    } else if keys.contains(&"task") && keys.contains(&"light") {
        // `weigh`: work is a task, a question about the project is light, any other is an answer; only the line is read, the options
        // below it say "this project" themselves
        let line = text.lines().next().unwrap_or_default().trim_start_matches("question: ");
        if ["fix", "add ", "rename", "port", "move "].iter().any(|w| line.contains(w)) || line.starts_with("yes") || line.starts_with("and ") {
            "task"
        } else if line.contains("this project") || line.contains("in src/main.rs") {
            "light"
        } else {
            "answer"
        }
    } else if keys.contains(&"task") {
        if text.contains("plan how") {
            "flow"
        } else if ["fix", "add ", "rename", "port", "move "].iter().any(|w| text.contains(w)) {
            "task"
        } else {
            "answer"
        }
    } else if keys.contains(&"light") {
        if text.contains("rename") { "light" } else { "heavy" }
    } else if text.contains("connection reset") {
        "retry"
    } else {
        "stop"
    };
    json!({ "model": "jev-latest", "answers": { "q": { "type": "choice", "choice": pick, "confidence": 0.95, "probabilities": { pick: 0.95 } } }, "usage": { "input_tokens": 5, "output_tokens": 0 } })
}

fn jev_gateway() -> Mock {
    let n = std::sync::Mutex::new(0u32);
    Mock::start(move |s, _| Say::Json(200, jev(&mut n.lock().unwrap(), &s.json())))
}

fn decider(url: &str, key: &str) -> String {
    format!("[[connector]]\nid = \"jev\"\nis = \"decider\"\ndialect = \"systemone\"\nurl = \"{url}\"\nkey = \"{key}\"\nmodels = [\"jev-latest\"]\n")
}

fn events(w: &World, kind: &str) -> Vec<Value> {
    w.front.log().sync();
    w.front.log().read(0).filter(|e| e.kind == kind).map(|e| e.data).collect()
}

fn until(what: &str, mut ready: impl FnMut() -> bool) {
    let end = Instant::now() + crate::support::PATIENCE;
    while !ready() {
        assert!(Instant::now() < end, "never: {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn keyless_the_points_answer_by_their_fallback_and_no_question_leaves() {
    let d = Dirs::new("keyless");
    let gateway = jev_gateway();
    let w = open(&d, &decider(&gateway.url(), "file:/no/such/key"), opts(Some("test")));
    assert!(w.front.asks().registry.get("decider:jev").is_none(), "no key, no decider");
    let row = say_and_wait(&w, "say: fix the build");
    assert!(matches!(row.state, j4n_core::task::State::Done { .. }));
    let decided = events(&w, "intake.decided");
    assert_eq!(decided.len(), 1);
    assert!(decided[0]["by"].as_str().unwrap().starts_with("fallback:"), "{decided:?}");
    assert!(gateway.seen().is_empty(), "nothing was asked of any decider");
    assert!(events(&w, "ask.egress").is_empty() && events(&w, "probe.result").is_empty());
    // and the Packs page says the key is missing, with the pack on
    let packs = w.front.screen().pack_rows();
    let jev = packs.iter().find(|r| r.key == "jev").expect("the pack jev is there");
    assert_eq!(jev.cells[1].text, "on");
    assert!(jev.detail.iter().any(|c| c.text.contains("the key is missing for jev")), "{:?}", jev.detail);
    w.close();
}

#[test]
fn a_decider_proves_itself_at_start_and_answers_only_after_it_has() {
    let d = Dirs::new("decider");
    let gateway = jev_gateway();
    let k = key(&d, "typesafe-key-0001");
    let w = open(&d, &decider(&gateway.url(), &k), opts(Some("test")));
    // at start its cases are put to it, off any line's path
    until("the cases of complete, fold, intake, who:work and retry were put", || events(&w, "probe.result").len() == 5);
    let results = events(&w, "probe.result");
    for r in &results {
        assert_eq!((r["answerer"].as_str(), r["passed"].as_bool(), r["model"].as_str()), (Some("decider:jev"), Some(true), Some("jev-latest@systemone")), "{r}");
    }
    let mut points: Vec<&str> = results.iter().map(|r| r["point"].as_str().unwrap()).collect();
    points.sort();
    assert_eq!(points, ["complete", "fold", "intake", "retry", "who:work"]);
    assert_eq!(gateway.seen().len(), 6 + 6 + 6 + 2 + 2, "every case, once: the point `weigh` is off, and its cases are not put");
    // now a first line is decided by it, and the Log says the question left, and for where
    let row = say_and_wait(&w, "say: why is the sky blue?");
    assert!(matches!(row.state, j4n_core::task::State::Done { .. }));
    let decided = events(&w, "intake.decided");
    assert_eq!((decided[0]["by"].as_str(), decided[0]["pick"].as_str()), (Some("decider:jev"), Some("answer")), "{decided:?}");
    assert_eq!(events(&w, "ask.egress").last().map(|e| (e["zone"].clone(), e["answerer"].clone())), Some((json!("local"), json!("decider:jev"))));
    // and `:probe` puts them again, and says how it went
    assert_eq!(w.front.say("main", ":probe"), j4n_core::front::Said::Command("probe".into()));
    until("the second round", || events(&w, "probe.result").len() == 10);
    until("its notes", || w.front.log().read(0).filter(|e| e.kind == "chat.note").any(|e| e.data["text"].as_str().is_some_and(|t| t.contains("passed its cases for retry"))));
    w.close();
}

#[test]
fn the_cases_of_weigh_are_put_to_jev_only_when_its_switch_is_on() {
    // off (the default): nothing is proved for the point and no call is spent on it, however many starts
    let d = Dirs::new("weigh-off");
    let gateway = jev_gateway();
    let k = key(&d, "typesafe-key-0003");
    let w = open(&d, &decider(&gateway.url(), &k), opts(Some("test")));
    until("the five points were put", || events(&w, "probe.result").len() == 5);
    std::thread::sleep(Duration::from_millis(300));
    assert!(events(&w, "probe.result").iter().all(|r| r["point"] != "weigh"), "{:?}", events(&w, "probe.result"));
    assert_eq!(gateway.seen().len(), 22, "no case of `weigh` was sent");
    assert!(!w.front.asks().points.contains_key("weigh"), "the point is not walked while its switch is off");
    w.close();
    // on: its ten cases are put (and the two of `read:goes_on`, which it asks), and the fixture Jev passes them
    let d = Dirs::new("weigh-on");
    fs::create_dir_all(d.j4n_home().join("settings")).unwrap();
    fs::write(d.j4n_home().join("settings/weigh.toml"), "api = 1\n[[setting]]\nid = \"weigh.on\"\nvalue = true\n").unwrap();
    let gateway = jev_gateway();
    let k = key(&d, "typesafe-key-0004");
    let w = open(&d, &decider(&gateway.url(), &k), opts(Some("test")));
    until("seven points were put", || events(&w, "probe.result").len() == 7);
    let weigh = events(&w, "probe.result").into_iter().find(|r| r["point"] == "weigh").expect("weigh was probed");
    assert_eq!(weigh["passed"], true, "{weigh}");
    assert_eq!(gateway.seen().len(), 22 + 10 + 2, "the ten cases of weigh and the two of read:goes_on, which weigh asks");
    w.close();
}

#[test]
fn the_cases_of_complete_are_not_put_to_jev_while_complete_decider_is_off() {
    let d = Dirs::new("complete-off");
    fs::create_dir_all(d.j4n_home().join("settings")).unwrap();
    fs::write(d.j4n_home().join("settings/complete.toml"), "api = 1\n[[setting]]\nid = \"complete.decider\"\nvalue = false\n").unwrap();
    let gateway = jev_gateway();
    let k = key(&d, "typesafe-key-0043");
    let w = open(&d, &decider(&gateway.url(), &k), opts(Some("test")));
    until("the four points were put", || events(&w, "probe.result").len() == 4);
    std::thread::sleep(Duration::from_millis(300));
    assert!(events(&w, "probe.result").iter().all(|r| r["point"] != "complete"), "{:?}", events(&w, "probe.result"));
    assert_eq!(gateway.seen().len(), 16, "no case of `complete` was sent");
    assert!(!w.front.asks().points.contains_key("complete"), "the point is not walked while its switch is off");
    w.close();
}

#[test]
fn the_cases_of_librarian_are_put_to_jev_only_when_its_switch_is_on() {
    // off (the default): nothing is proved for the point and no call is spent on it, however many starts
    let d = Dirs::new("librarian-off");
    let gateway = jev_gateway();
    let k = key(&d, "typesafe-key-0005");
    let w = open(&d, &decider(&gateway.url(), &k), opts(Some("test")));
    until("the five points were put", || events(&w, "probe.result").len() == 5);
    std::thread::sleep(Duration::from_millis(300));
    assert!(events(&w, "probe.result").iter().all(|r| r["point"] != "librarian"), "{:?}", events(&w, "probe.result"));
    assert_eq!(gateway.seen().len(), 22, "no case of `librarian` was sent");
    assert!(!w.front.asks().points.contains_key("librarian"), "the point is not walked while its switch is off");
    w.close();
    // on: its four cases are put, and the fixture Jev passes them
    let d = Dirs::new("librarian-on");
    fs::create_dir_all(d.j4n_home().join("settings")).unwrap();
    fs::write(d.j4n_home().join("settings/librarian.toml"), "api = 1\n[[setting]]\nid = \"librarian.on\"\nvalue = true\n").unwrap();
    let gateway = jev_gateway();
    let k = key(&d, "typesafe-key-0006");
    let w = open(&d, &decider(&gateway.url(), &k), opts(Some("test")));
    until("six points were put", || events(&w, "probe.result").len() == 6);
    let librarian = events(&w, "probe.result").into_iter().find(|r| r["point"] == "librarian").expect("librarian was probed");
    assert_eq!(librarian["passed"], true, "{librarian}");
    assert_eq!(gateway.seen().len(), 22 + 4);
    assert!(w.front.asks().points.contains_key("librarian"));
    w.close();
}

#[test]
fn the_cases_of_chats_are_put_to_jev_only_when_its_switch_is_on_and_a_screen_is_there_to_use_it() {
    let on = |d: &Dirs| {
        fs::create_dir_all(d.j4n_home().join("settings")).unwrap();
        fs::write(d.j4n_home().join("settings/chats.toml"), "api = 1\n[[setting]]\nid = \"chats.on\"\nvalue = true\n").unwrap();
    };
    // off (the default), on a screen: nothing is proved for the point and no call is spent on it, however many starts
    let d = Dirs::new("chats-off");
    let gateway = jev_gateway();
    let k = key(&d, "typesafe-key-0007");
    let w = open_as(&d, &decider(&gateway.url(), &k), opts(Some("test")), true);
    until("the five points were put", || events(&w, "probe.result").len() == 5);
    std::thread::sleep(Duration::from_millis(300));
    assert!(events(&w, "probe.result").iter().all(|r| r["point"] != "chats"), "{:?}", events(&w, "probe.result"));
    assert_eq!(gateway.seen().len(), 22, "no case of `chats` was sent");
    assert!(!w.front.asks().points.contains_key("chats"), "the point is not walked while its switch is off");
    assert!(!w.front.chats_on());
    w.close();
    // on, in a run or a pipe: there is no orchestrator to route for, so the point is not registered and no case is sent for it
    let d = Dirs::new("chats-run");
    on(&d);
    let gateway = jev_gateway();
    let k = key(&d, "typesafe-key-0009");
    let w = open_as(&d, &decider(&gateway.url(), &k), opts(Some("test")), false);
    until("the five points were put", || events(&w, "probe.result").len() == 5);
    std::thread::sleep(Duration::from_millis(300));
    assert!(events(&w, "probe.result").iter().all(|r| r["point"] != "chats"), "{:?}", events(&w, "probe.result"));
    assert_eq!(gateway.seen().len(), 22, "no case of `chats` was sent");
    assert!(!w.front.asks().points.contains_key("chats") && !w.front.chats_on());
    w.close();
    // on, on a screen: its four cases are put, and the fixture Jev passes them
    let d = Dirs::new("chats-on");
    on(&d);
    let gateway = jev_gateway();
    let k = key(&d, "typesafe-key-0008");
    let w = open_as(&d, &decider(&gateway.url(), &k), opts(Some("test")), true);
    until("six points were put", || events(&w, "probe.result").len() == 6);
    let chats = events(&w, "probe.result").into_iter().find(|r| r["point"] == "chats").expect("chats was probed");
    assert_eq!(chats["passed"], true, "{chats}");
    assert_eq!(gateway.seen().len(), 22 + 4);
    assert!(w.front.asks().points.contains_key("chats") && w.front.chats_on());
    w.close();
}

#[test]
fn a_decider_that_gets_a_twin_wrong_does_not_answer_and_the_fallback_stands() {
    let d = Dirs::new("wrong");
    // it says "task" to everything
    let gateway = Mock::start(|_, _| Say::Json(200, json!({ "answers": { "q": { "type": "choice", "choice": "task", "confidence": 0.99 } } })));
    let k = key(&d, "typesafe-key-0002");
    let w = open(&d, &decider(&gateway.url(), &k), opts(Some("test")));
    until("the cases were put", || events(&w, "probe.result").len() == 5);
    let intake = events(&w, "probe.result").into_iter().find(|r| r["point"] == "intake").unwrap();
    assert_eq!(intake["passed"], false);
    assert!(!intake["failed"].as_array().unwrap().is_empty());
    let before = gateway.seen().len();
    let row = say_and_wait(&w, "say: tell me about it");
    assert!(matches!(row.state, j4n_core::task::State::Done { .. }));
    let decided = events(&w, "intake.decided");
    assert!(decided[0]["by"].as_str().unwrap().starts_with("fallback:") || decided[0]["by"] == "rule:intake", "{decided:?}");
    assert_eq!(gateway.seen().len(), before, "it was not asked: it has not proved itself");
    let steps = events(&w, "ask.step");
    assert!(steps.iter().any(|s| s["answerer"] == "decider:jev" && s["outcome"] == "skipped" && s["why"].as_str().unwrap().starts_with("shadow")), "{steps:?}");
    w.close();
}

/// A Jev that takes its time, so a first line meets cases that are still being put.
fn slow_jev_gateway() -> Mock {
    let n = std::sync::Mutex::new(0u32);
    Mock::start(move |s, _| {
        std::thread::sleep(Duration::from_millis(80));
        Say::Json(200, jev(&mut n.lock().unwrap(), &s.json()))
    })
}

#[test]
fn a_first_line_with_no_operator_waits_for_the_cases_of_its_point_and_the_decider_answers_it() {
    let d = Dirs::new("coldwait");
    let gateway = slow_jev_gateway();
    let k = key(&d, "typesafe-key-0040");
    let w = open(&d, &decider(&gateway.url(), &k), opts(Some("test")));
    // a fresh home: nothing is proved, and the line is said at once
    let row = say_and_wait(&w, "say: fix the build");
    assert!(matches!(row.state, j4n_core::task::State::Done { .. }));
    let decided = events(&w, "intake.decided");
    assert_eq!((decided[0]["by"].as_str(), decided[0]["pick"].as_str()), (Some("decider:jev"), Some("task")), "{decided:?}");
    let notes = events(&w, "ask.note");
    assert!(notes.iter().any(|n| n["why"].as_str().is_some_and(|t| t.starts_with("waited ") && t.ends_with("for the probe cases of intake"))), "{notes:?}");
    let intake = events(&w, "probe.result").into_iter().find(|r| r["point"] == "intake").expect("its cases were written before the walk");
    assert_eq!(intake["passed"], true);
    assert!(intake["missed"].as_array().unwrap().is_empty());
    w.close();
}

#[test]
fn a_line_does_not_queue_behind_the_cases_of_another_point_that_the_start_up_run_is_still_putting() {
    let d = Dirs::new("behind");
    let held = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let (n, hold) = (std::sync::Mutex::new(0u32), held.clone());
    // the cases of `fold` (the first point in the order of the table) are not answered until the test says
    let gateway = Mock::start(move |s, _| {
        let body = s.json();
        let fold = body["questions"]["q"]["criteria"].as_object().is_some_and(|m| m.contains_key("replace"));
        let end = Instant::now() + Duration::from_secs(25);
        while fold && hold.load(std::sync::atomic::Ordering::SeqCst) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        Say::Json(200, jev(&mut n.lock().unwrap(), &body))
    });
    let k = key(&d, "typesafe-key-0042");
    let w = open(&d, &decider(&gateway.url(), &k), opts(Some("test")));
    let row = say_and_wait(&w, "say: fix the build");
    assert!(matches!(row.state, j4n_core::task::State::Done { .. }));
    let decided = events(&w, "intake.decided");
    assert_eq!(decided[0]["by"], "decider:jev", "{decided:?}");
    assert!(events(&w, "probe.result").iter().all(|r| r["point"] != "fold"), "fold was still held");
    held.store(false, std::sync::atomic::Ordering::SeqCst);
    w.close();
}

#[test]
fn with_the_wait_switched_off_the_same_first_line_is_not_decided_by_the_decider() {
    let d = Dirs::new("nowait");
    fs::create_dir_all(d.j4n_home().join("settings")).unwrap();
    fs::write(d.j4n_home().join("settings/mine.toml"), "api = 1\n[[setting]]\nid = \"probe.wait\"\nvalue = 0\n").unwrap();
    let gateway = slow_jev_gateway();
    let k = key(&d, "typesafe-key-0041");
    let w = open(&d, &decider(&gateway.url(), &k), opts(Some("test")));
    let row = say_and_wait(&w, "say: fix the build");
    assert!(matches!(row.state, j4n_core::task::State::Done { .. }));
    let decided = events(&w, "intake.decided");
    assert!(decided[0]["by"].as_str().unwrap().starts_with("fallback:"), "{decided:?}");
    assert!(events(&w, "ask.note").iter().all(|n| !n["why"].as_str().unwrap_or_default().starts_with("waited ")));
    w.close();
}

#[test]
fn a_decider_that_gets_a_twin_wrong_leaves_the_case_its_answer_and_its_confidence_in_the_log() {
    let d = Dirs::new("missed");
    let gateway = Mock::start(|_, _| Say::Json(200, json!({ "answers": { "q": { "type": "choice", "choice": "task", "confidence": 0.99 } } })));
    let k = key(&d, "typesafe-key-0039");
    let w = open(&d, &decider(&gateway.url(), &k), opts(Some("test")));
    until("the cases were put", || events(&w, "probe.result").len() == 5);
    let intake = events(&w, "probe.result").into_iter().find(|r| r["point"] == "intake").unwrap();
    let missed = intake["missed"].as_array().unwrap();
    assert!(missed.iter().any(|m| m["case"] == "intake-short-question" && m["wanted"] == "answer" && m["got"] == "task" && (m["conf"].as_f64().unwrap() - 0.99).abs() < 1e-6), "{missed:?}");
    assert_eq!(missed.len(), intake["failed"].as_array().unwrap().len());
    w.close();
}

// ---- hosts ----

const HOSTS: &str = r#"
[[model]]
id = "oss"
connector = "gpt"
name = "vendor/m"
default = true
endpoints = [
  { host = "Dear", zone = "any", p_in = 0.5, p_out = 1.0 },
  { host = "Cheap", zone = "any", p_in = 0.05, p_out = 0.2 },
]
"#;

fn route_on(d: &Dirs) {
    fs::create_dir_all(d.j4n_home().join("settings")).unwrap();
    fs::write(d.j4n_home().join("settings/mine.toml"), "api = 1\n[[setting]]\nid = \"route.hosts\"\nvalue = true\n").unwrap();
}

#[test]
fn with_the_switch_on_the_cheapest_host_is_elected_once_asked_for_by_name_and_kept() {
    let d = Dirs::new("hosts");
    let gateway = Mock::start(|_, _| chat("ok"));
    let k = key(&d, "k-hosts-0001");
    route_on(&d);
    let w = open(&d, &format!("{}{HOSTS}", model(&format!("{}/v1", gateway.url()), &k, "")), opts(Some("gpt")));
    say_and_wait(&w, "one");
    say_and_wait(&w, "two");
    let seen = gateway.seen();
    assert_eq!(seen.len(), 2);
    for s in &seen {
        assert_eq!(s.json()["provider"], json!({ "order": ["Cheap"], "allow_fallbacks": false }), "{}", s.body);
    }
    assert_eq!(w.front.binding("main").and_then(|b| b.endpoint), Some("Cheap".into()));
    assert_eq!(events(&w, "thread.bound").len(), 1);
    assert_eq!(events(&w, "run.served")[0]["host"], "Cerebras", "who served is what the gateway said, not what was asked");
    w.close();
}

/// The dollars the election said it weighed for the host it chose: `…for a `work` task: $0.0123 (…`.
fn weighed(d: &Dirs, plan: bool, off: bool) -> f64 {
    let gateway = Mock::start(|_, _| chat("ok"));
    let k = key(d, "k-hosts-0003");
    route_on(d);
    if plan {
        fs::write(d.j4n_home().join("plan.toml"), "api = 1\n[[plan]]\nid = \"gpt\"\nconnector = \"gpt\"\nwindows = [\"seven_day=7d\"]\nprice = \"$24\"\nassume = \"$1000\"\n").unwrap();
    }
    if off {
        fs::write(d.j4n_home().join("settings/off.toml"), "api = 1\n[[setting]]\nid = \"cost.plan.on\"\nvalue = false\n").unwrap();
    }
    let w = open(d, &format!("{}{HOSTS}", model(&format!("{}/v1", gateway.url()), &k, "")), opts(Some("gpt")));
    say_and_wait(&w, "one");
    let route = events(&w, "thread.bound")[0]["route"].as_str().unwrap().to_string();
    w.close();
    let after = route.split("task: $").nth(1).unwrap_or_else(|| panic!("{route}"));
    after.split(|c: char| !(c.is_ascii_digit() || c == '.')).next().unwrap().parse().unwrap()
}

#[test]
fn the_election_weighs_what_a_request_takes_of_the_plan_when_the_doer_is_on_one() {
    let (list, planned, off) = (weighed(&Dirs::new("hostslist"), false, false), weighed(&Dirs::new("hostsplan"), true, false), weighed(&Dirs::new("hostsoffplan"), true, true));
    assert!(list > 0.0, "{list}");
    assert!((planned - list * 0.024).abs() < list * 0.024 * 0.05 + 1e-4, "$24 over $1000 of list cost: {planned} of {list}");
    assert!(planned < list / 10.0 && (off - list).abs() < 1e-4, "with the switch off it is the list cost: {off} of {list}");
}

#[test]
fn with_the_switch_off_no_host_is_named_though_the_rows_are_there() {
    let d = Dirs::new("hostsoff");
    let gateway = Mock::start(|_, _| chat("ok"));
    let k = key(&d, "k-hosts-0002");
    let w = open(&d, &format!("{}{HOSTS}", model(&format!("{}/v1", gateway.url()), &k, "")), opts(Some("gpt")));
    say_and_wait(&w, "one");
    let b = gateway.seen()[0].json();
    assert!(b.get("provider").is_none() && b["session_id"].is_string(), "{b}");
    assert_eq!(w.front.binding("main").and_then(|b| b.endpoint), None);
    w.close();
}

#[test]
fn a_model_connector_is_also_an_answerer_for_the_points_that_name_it_and_min_conf_holds() {
    use j4n_core::answer::chain::{Ledger, Walk, walk};
    use j4n_core::answer::Question;
    use j4n_core::ask::{AskShape, Opt};
    let d = Dirs::new("answerer");
    let said = std::sync::Arc::new(std::sync::Mutex::new(0.8));
    let gateway = {
        let said = said.clone();
        Mock::start(move |_, _| chat(&format!("{{\"answer\": \"b\", \"confidence\": {}, \"why\": \"the second is safer\"}}", said.lock().unwrap())))
    };
    let k = key(&d, "k-answerer-0001");
    let conn = model(&format!("{}/v1", gateway.url()), &k, "").replace("id = \"gpt\"", "id = \"light\"");
    let w = open(&d, &conn, opts(Some("test")));
    let asks = w.front.asks();
    assert!(asks.registry.get("model:light").is_some() && asks.registry.get("decider:jev").is_none());
    let p = &asks.points["pick"];
    let q = Question {
        ask: "pick#1".into(),
        point: "pick".into(),
        asker: "front".into(),
        thread: None,
        text: "which one?".into(),
        shape: AskShape::Choice { options: ["a", "b"].iter().map(|id| Opt { id: (*id).into(), label: (*id).into(), recommended: false }).collect() },
        goal: "g".into(),
        act: None,
    };
    let (walked, notes) = walk(p, &q, &asks.registry, &asks.state, &Ledger::new(), 1_000, true);
    let Walk::Answered(o) = walked else { panic!("{walked:?}") };
    assert_eq!((o.by.as_str(), o.answer.clone()), ("model:light", j4n_core::ask::Answer::Pick { id: "b".into() }));
    assert!(notes.iter().any(|n| n.kind == "ask.egress" && n.data["zone"] == "local"));
    // not proved for this point? `pick` has no cases, so it needs none; a confidence under the step's 0.7 is passed over
    *said.lock().unwrap() = 0.6;
    let (walked, notes) = walk(p, &Question { text: "again?".into(), ..q }, &asks.registry, &asks.state, &Ledger::new(), 1_000, true);
    assert!(matches!(walked, Walk::Fallback(_)), "{walked:?}");
    assert!(notes.iter().any(|n| n.data["answerer"] == "model:light" && n.data["outcome"] == "pass" && n.data["why"].as_str().unwrap().contains("below 0.70")), "{notes:?}");
    w.close();
}
