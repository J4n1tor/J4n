//! Quality control: the pack `qc` on, a project in git, the scripted doer making changes and a gateway on the loopback
//! that is Jev, so what is checked is the real rules, the real doer `qc`, the real wire to a decider and the real marks in the Log.

use crate::models_cases::mock::{Mock, Say};
use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::change::Actor;
use j4n_core::front::Said;
use j4n_core::gate::grant::here_dir;
use j4n_core::log::New;
use serde_json::{Value, json};
use std::fs;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What the gateway says to the questions of a check.
#[derive(Clone, Copy, PartialEq)]
enum Reply {
    /// `swallowed-error` is likely, a choice is none, every rubric is at its second-lowest level.
    Normal,
    /// A refusal that is not worth trying again.
    Refuse,
    /// The same, for the part that is in this file.
    RefuseFile(&'static str),
    /// The choice names `parse_numeric`, the helper the part repeats.
    Helper,
}

struct Rig {
    _d: Dirs,
    w: World,
    jev: Mock,
    reply: Arc<Mutex<Reply>>,
}

fn git(d: &Dirs, args: &[&str]) {
    let out = Command::new("git").args(args).current_dir(d.project()).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn answers(qs: &Value, helper: &str) -> Value {
    let mut out = serde_json::Map::new();
    for (id, q) in qs.as_object().into_iter().flatten() {
        let a = match q["type"].as_str().unwrap_or_default() {
            "noul" => json!({ "type": "noul", "noul": if id == "p:swallowed-error" { 0.9 } else { 0.1 } }),
            "choice" => json!({ "type": "choice", "choice": helper, "confidence": 0.9 }),
            _ => json!({ "type": "score", "score": 1.0 }),
        };
        out.insert(id.clone(), a);
    }
    json!({ "model": "jev-test", "answers": out, "usage": { "input_tokens": 100, "output_tokens": 5 } })
}

/// A question of a check, as opposed to the cases the base pack puts to a decider at start.
fn is_check(body: &Value) -> bool {
    body["questions"].as_object().is_some_and(|q| q.keys().any(|k| k.starts_with("p:") || k.starts_with("r:") || k.starts_with("c:")))
}

/// The requests that were checks.
fn checks(g: &Mock) -> Vec<Value> {
    g.seen().into_iter().map(|s| s.json()).filter(is_check).collect()
}

/// `settings` are `id = value` lines of the operator's folder for the project; `jev` adds the decider; `check` is the argv of a
/// check row of the project's own that applies here.
fn rig(tag: &str, settings: &[(&str, &str)], jev: bool, check: Option<&str>) -> Rig {
    rig_with(tag, settings, jev, check, &[])
}

/// `files` are more files of the operator's folder (`gates/mine.toml`).
fn rig_with(tag: &str, settings: &[(&str, &str)], jev: bool, check: Option<&str>, files: &[(&str, &str)]) -> Rig {
    let d = Dirs::new(tag);
    for a in [&["init", "-q", "-b", "main"][..], &["config", "user.email", "t@t"], &["config", "user.name", "t"]] {
        git(&d, a);
    }
    d.put("src/a.rs", "pub fn parse_number(s: &str) -> u32 { s.parse().unwrap() }\n");
    d.put("src/util.rs", "pub fn parse_numeric(s: &str) -> u32 { s.len() as u32 }\n");
    d.put("README.md", "# demo\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-qm", "first"]);
    let reply = Arc::new(Mutex::new(Reply::Normal));
    let r2 = reply.clone();
    let jev_gateway = Mock::start(move |req, _| {
        let body = req.json();
        if !is_check(&body) {
            // the cases the decider is put to at start: whatever, they are not what is tested
            return Say::Json(200, json!({ "model": "jev-test", "answers": { "q": { "type": "noul", "noul": 0.9 } }, "usage": { "input_tokens": 1, "output_tokens": 0 } }));
        }
        match *r2.lock().unwrap() {
            Reply::Normal => Say::Json(200, answers(&body["questions"], "none")),
            Reply::Helper => Say::Json(200, answers(&body["questions"], "parse_numeric")),
            Reply::Refuse => Say::Json(400, json!({ "error": "no" })),
            Reply::RefuseFile(f) if body["state"]["delivered"]["file"] == f => Say::Json(400, json!({ "error": "no" })),
            Reply::RefuseFile(_) => Say::Json(200, answers(&body["questions"], "none")),
        }
    });
    let key = d.dir.join("key");
    crate::support::key_file(&key, "typesafe-test-0001\n");
    fs::create_dir_all(d.j4n_home().join("connectors")).unwrap();
    if jev {
        let row = format!("api = 1\n[[connector]]\nid = \"jev\"\nis = \"decider\"\ndialect = \"systemone\"\nurl = \"{}\"\nkey = \"file:{}\"\nmodels = [\"jev-latest\"]\n", jev_gateway.url(), key.display());
        fs::write(d.j4n_home().join("connectors/mine.toml"), row).unwrap();
    }
    if let Some(argv) = check {
        fs::create_dir_all(d.j4n_home().join("checks")).unwrap();
        fs::write(d.j4n_home().join("checks/mine.toml"), format!("api = 1\n[[check]]\nid = \"mine\"\nfiles = [\"README.md\"]\nargv = {argv}\n")).unwrap();
    }
    for (rel, text) in files {
        let at = d.j4n_home().join(rel);
        fs::create_dir_all(at.parent().unwrap()).unwrap();
        fs::write(at, text).unwrap();
    }
    fs::write(d.j4n_home().join("harness.toml"), "api = 1\n[[enable]]\nid = \"qc\"\n").unwrap();
    let here = here_dir(&d.j4n_home(), &d.project());
    fs::create_dir_all(&here).unwrap();
    // the formatter is not what is tested here (and the code the scripted doer writes is one line)
    let mut rows = String::from("api = 1\n[[setting]]\nid = \"qc.skip\"\nvalue = \"rs-fmt\"\n");
    for (id, value) in settings {
        rows.push_str(&format!("[[setting]]\nid = \"{id}\"\nvalue = {value}\n"));
    }
    fs::write(here.join("settings.toml"), rows).unwrap();
    let w = open(&d);
    Rig { _d: d, w, jev: jev_gateway, reply }
}

fn open(d: &Dirs) -> World {
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Full), wall: None, usd: None, mode: j4n_core::task::Mode::Write, place: None, shape: false, anywhere: false };
    crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"))
}

fn dump(w: &World) -> String {
    w.front.log().sync();
    w.front.log().read(0).map(|e| format!("{:>4} {:<16} {} {}", e.seq, e.kind, e.task.as_deref().unwrap_or("-"), e.data)).collect::<Vec<_>>().join("\n")
}

fn events(w: &World, kind: &str) -> Vec<Value> {
    w.front.log().sync();
    w.front.log().read(0).filter(|e| e.kind == kind).map(|e| e.data).collect()
}

fn until(w: &World, what: &str, mut ready: impl FnMut() -> bool) {
    let end = Instant::now() + crate::support::PATIENCE;
    while !ready() {
        assert!(Instant::now() < end, "never: {what}\n{}", dump(w));
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Say a line and wait for its task.
fn said(w: &World, line: &str) -> j4n_core::task::Row {
    let task = match w.front.say("main", line) {
        Said::Started { task } => task,
        other => panic!("{line:?} did not start a task: {other:?}\n{}", dump(w)),
    };
    w.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap_or_else(|| panic!("{task} never ended\n{}", dump(w)))
}

fn run_of(w: &World, scope: &str) -> Value {
    events(w, "qc.run").into_iter().find(|r| r["scope"] == scope).unwrap_or_else(|| panic!("no qc.run for {scope}\n{}", dump(w)))
}

fn marks_in(w: &World) -> Vec<(String, String, u64)> {
    events(w, "qc.mark").iter().map(|m| (m["pattern"].as_str().unwrap().to_string(), m["file"].as_str().unwrap().to_string(), m["line"].as_u64().unwrap())).collect()
}

#[test]
fn a_path_is_checked_as_it_stands_and_the_report_says_the_mark_the_score_and_that_only_the_first_tier_ran() {
    let r = rig("qc-path-checked-as", &[], false, None);
    let row = said(&r.w, ":qc src/");
    let answer = row.answer.clone().unwrap_or_default();
    assert_eq!(marks_in(&r.w), [("rs-unwrap".to_string(), "src/a.rs".to_string(), 1)], "{}", dump(&r.w));
    assert!(answer.contains("rs-unwrap  src/a.rs:1 mid  1.00"), "{answer}");
    assert!(answer.contains("qc 9.0 0-only · coverage 100% · parts 2"), "{answer}");
    let run = run_of(&r.w, "src/");
    assert_eq!((run["score"].as_f64(), run["status"].as_str(), run["trigger"].as_str()), (Some(9.0), Some("0-only"), Some("ask")));
    assert!(r.jev.seen().iter().all(|s| !is_check(&s.json())), "no check went to Jev");
    r.w.close();
}

#[test]
fn at_settle_a_task_is_checked_for_what_it_added_and_not_for_the_unwrap_that_was_there() {
    let r = rig("qc-at-settle-task", &[("qc.live", "\"off\"")], false, None);
    said(&r.w, ":qc on");
    until(&r.w, "the switch", || !events(&r.w, "qc.switch").is_empty());
    let work = said(&r.w, "write src/b.rs pub fn b(s: &str) -> u32 { s.parse().unwrap() }");
    assert!(matches!(work.state, j4n_core::task::State::Done { ok: true }), "{:?}", work.state);
    until(&r.w, "the check at settle", || !events(&r.w, "qc.run").is_empty());
    let run = events(&r.w, "qc.run").remove(0);
    assert_eq!((run["trigger"].as_str(), run["scope"].as_str(), run["work"].as_str()), (Some("settle"), Some(format!("task {}", work.id).as_str()), Some(work.id.as_str())), "{run}");
    assert_eq!(marks_in(&r.w), [("rs-unwrap".to_string(), "src/b.rs".to_string(), 1)], "only the new file is the task's");
    // the lane did not wait: its task was over before the check ended
    let seq = |kind: &str| r.w.front.log().read(0).find(|e| e.kind == kind && e.task.as_deref() == Some(work.id.as_str())).map(|e| e.seq).unwrap();
    assert!(seq("task.settled") < seq("qc.run"));
    r.w.close();
}

#[test]
fn while_it_is_off_nothing_starts_by_itself_and_a_check_asked_for_still_runs() {
    let r = rig("qc-while-off-nothing", &[], false, None);
    said(&r.w, "write src/b.rs pub fn b(s: &str) -> u32 { s.parse().unwrap() }");
    std::thread::sleep(Duration::from_millis(500));
    assert!(events(&r.w, "qc.run").is_empty() && events(&r.w, "qc.mark").is_empty(), "{}", dump(&r.w));
    said(&r.w, ":qc src/b.rs");
    assert_eq!(events(&r.w, "qc.run").len(), 1);
    said(&r.w, ":qc off");
    until(&r.w, "the switch", || !events(&r.w, "qc.switch").is_empty());
    assert_eq!(events(&r.w, "qc.switch")[0]["on"], false);
    r.w.close();
}

#[test]
fn with_jev_off_no_request_leaves_and_the_score_says_only_the_first_tier_ran() {
    let r = rig("qc-jev-off-no", &[("qc.jev", "false")], true, None);
    said(&r.w, ":qc src/");
    let run = run_of(&r.w, "src/");
    assert_eq!(run["status"], "0-only");
    assert!(checks(&r.jev).is_empty(), "nothing was sent");
    assert!(events(&r.w, "ask.egress").is_empty(), "{}", dump(&r.w));
    r.w.close();
}

#[test]
fn a_jev_that_refuses_is_a_tier_that_did_not_run_and_never_a_clean_number() {
    let r = rig("qc-jev-refuses-tier", &[("qc.jev", "true")], true, None);
    *r.reply.lock().unwrap() = Reply::Refuse;
    let row = said(&r.w, ":qc src/");
    let run = run_of(&r.w, "src/");
    assert_eq!(run["status"], "0-only", "{run}");
    assert_eq!(run["errors"][0]["tier"], 1);
    assert!(run["errors"][0]["reason"].as_str().unwrap().contains("HTTP 400"), "{run}");
    let answer = row.answer.unwrap_or_default();
    assert!(answer.contains("tier 1 did not run") && answer.contains("0-only"), "{answer}");
    // and a Jev nobody connected
    let r2 = rig("qc-jev-refuses-tier-2", &[("qc.jev", "true")], false, None);
    said(&r2.w, ":qc src/");
    let run = run_of(&r2.w, "src/");
    assert_eq!((run["status"].as_str(), run["errors"][0]["tier"].as_u64()), (Some("0-only"), Some(1)));
    assert!(run["errors"][0]["reason"].as_str().unwrap().contains("not connected"), "{run}");
    r.w.close();
    r2.w.close();
}

#[test]
fn a_path_is_put_to_jev_with_one_call_for_each_part_and_the_rubrics_hold_the_score() {
    let r = rig("qc-path-put-jev", &[("qc.jev", "true")], true, None);
    let row = said(&r.w, ":qc src/");
    let calls = checks(&r.jev);
    assert_eq!(calls.len(), 2, "two files, two parts, one call each");
    let a = calls.iter().find(|c| c["state"]["delivered"]["file"] == "src/a.rs").unwrap();
    let qs = a["questions"].as_object().unwrap();
    assert_eq!(qs.keys().filter(|k| k.starts_with("p:")).count(), 12);
    assert_eq!(qs.keys().filter(|k| k.starts_with("r:")).count(), 3);
    assert!(qs["c:duplicate-helper"]["criteria"].as_object().unwrap().contains_key("none"));
    assert_eq!(a["model"], "jev-latest");
    let run = run_of(&r.w, "src/");
    assert_eq!((run["status"].as_str(), run["coverage"].as_u64(), run["tokens"].as_u64()), (Some("full"), Some(100), Some(210)), "{run}");
    // one mark of the first tier, one of the second for each part; rubrics at level 1 of 3, 2 and 2 hold the score to mean(1/3, 1/2, 1/2) of 10
    let by: Vec<(String, u64)> = events(&r.w, "qc.mark").iter().map(|m| (m["pattern"].as_str().unwrap().to_string(), m["tier"].as_u64().unwrap())).collect();
    assert_eq!(by.iter().filter(|(p, t)| p == "swallowed-error" && *t == 1).count(), 2, "{by:?}");
    assert_eq!(run["score"].as_f64(), Some(4.4));
    assert!(row.answer.unwrap_or_default().contains("qc 4.4 · coverage 100%"));
    r.w.close();
}

#[test]
fn the_check_of_the_project_runs_once_the_mark_reads_it_and_a_red_one_holds_the_score_under_three() {
    let r = rig("qc-check-project-runs", &[("check.after", "true"), ("qc.live", "\"off\"")], false, Some("[\"false\"]"));
    said(&r.w, ":qc on");
    until(&r.w, "the switch", || !events(&r.w, "qc.switch").is_empty());
    let work = said(&r.w, "write src/b.rs pub fn b() {}");
    until(&r.w, "the check at settle", || !events(&r.w, "qc.run").is_empty());
    let run = events(&r.w, "qc.run").remove(0);
    assert_eq!((run["trigger"].as_str(), run["check_failed"].as_bool(), run["score"].as_f64()), (Some("settle"), Some(true), Some(3.0)), "{run}");
    let queued: Vec<Value> = events(&r.w, "task.queued");
    assert_eq!(queued.iter().filter(|q| q["spec"]["doer"] == "check").count(), 1, "the project's check was run once, by the rule of the base pack");
    assert_eq!(queued.iter().filter(|q| q["spec"]["goal"].as_str().is_some_and(|g| g.starts_with("@settle"))).count(), 1, "and read once");
    assert_eq!(run["work"], work.id.as_str());
    r.w.close();
}

#[test]
fn a_dismissed_mark_is_labelled_and_lowers_its_pattern_and_is_not_shown_again() {
    let r = rig("qc-dismissed-mark-labelled", &[], false, None);
    said(&r.w, ":qc src/");
    let m = events(&r.w, "qc.mark").remove(0);
    let id = m["mark"].as_str().unwrap().to_string();
    assert_eq!((m["prec"].as_f64(), m["n"].as_u64(), m["state"].as_str()), (Some(0.0), Some(0), Some("open")));
    let row = said(&r.w, &format!(":qc dismiss {id}"));
    assert!(row.answer.unwrap_or_default().starts_with(&format!("dismissed {id}")));
    let labels = events(&r.w, "label");
    assert_eq!(labels.len(), 1);
    assert_eq!((labels[0]["subject_kind"].as_str(), labels[0]["subject"].as_str(), labels[0]["producer"].as_str(), labels[0]["label"].as_str()), (Some("qc_mark"), Some(id.as_str()), Some("rs-unwrap"), Some("dismissed")));
    assert_eq!(events(&r.w, "qc.mark.state")[0]["state"], "dismissed");
    // the same place, checked again: written, but dismissed, so it costs nothing and the pattern's precision is now 0 of 1
    let row = said(&r.w, ":qc src/");
    let again = events(&r.w, "qc.mark").pop().unwrap();
    assert_eq!((again["state"].as_str(), again["n"].as_u64(), again["prec"].as_f64()), (Some("dismissed"), Some(1), Some(0.0)));
    assert!(row.answer.unwrap_or_default().contains("qc 10.0"), "a dismissed mark is not in the score");
    // keeping one is a label too, and no label is ever `fixed` by this version
    let kept = said(&r.w, &format!(":qc keep {id}"));
    assert!(kept.answer.unwrap_or_default().starts_with("kept"));
    said(&r.w, ":qc src/");
    assert_eq!(events(&r.w, "qc.mark").pop().unwrap()["state"], "open", "a place the operator kept is open again");
    assert!(events(&r.w, "label").iter().all(|l| l["label"] != "fixed"));
    r.w.close();
}

/// Labels as the operator's `k` keys would have written them: `n` of the pattern's marks, all of them right when `right`.
fn labelled(w: &World, pattern: &str, n: usize, right: bool) {
    for i in 0..n {
        let data = json!({ "subject_kind": "qc_mark", "subject": format!("m{i:08x}"), "producer": pattern, "model": "", "point": "", "label": if right { "kept" } else { "dismissed" }, "weight": 1.0 });
        w.front.log().append(New::of("label", Actor::Operator, data)).unwrap();
    }
}

/// A task that writes a stub and goes on working long enough for the live check to find it.
fn working(w: &World) -> String {
    match w.front.say("main", "write src/c.rs pub fn c() -> u32 { todo!() }; wait 6500; say done") {
        Said::Started { task } => task,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_high_pattern_with_the_numbers_steers_a_working_lane_once_in_its_own_fixed_words() {
    let r = rig("qc-high-pattern-numbers", &[("qc.live", "\"steer\"")], false, None);
    labelled(&r.w, "stub-left", 41, true);
    said(&r.w, ":qc on");
    until(&r.w, "the switch", || !events(&r.w, "qc.switch").is_empty());
    let task = working(&r.w);
    until(&r.w, "the steer", || !events(&r.w, "task.steered").is_empty());
    let m = events(&r.w, "qc.mark").into_iter().find(|m| m["pattern"] == "stub-left").unwrap();
    assert_eq!((m["steerable"].as_bool(), m["live"].as_bool(), m["n"].as_u64(), m["work"].as_str()), (Some(true), Some(true), Some(41), Some(task.as_str())), "{m}");
    let steered = events(&r.w, "task.steered");
    assert_eq!(steered.len(), 1, "{}", dump(&r.w));
    assert_eq!(steered[0]["text"], "The code at src/c.rs:1 is a stub. Write the implementation the task asks for.");
    r.w.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap();
    r.w.close();
}

#[test]
fn a_pattern_without_the_numbers_only_shows_and_so_does_a_lane_that_may_not_be_steered() {
    // 39 labels are not 40; 40 labels at 0.9 are not 0.95; and `show` never steers
    for (tag, settings, n, right_all) in [("qc-pattern-without-numbers", vec![("qc.live", "\"steer\"")], 39, true), ("qc-pattern-without-numbers-2", vec![("qc.live", "\"show\"")], 41, true), ("qc-pattern-without-numbers-3", vec![("qc.live", "\"steer\""), ("qc.rounds", "0")], 41, true)] {
        let r = rig(tag, &settings, false, None);
        labelled(&r.w, "stub-left", n, right_all);
        said(&r.w, ":qc on");
        until(&r.w, "the switch", || !events(&r.w, "qc.switch").is_empty());
        let task = working(&r.w);
        until(&r.w, "the live check", || events(&r.w, "qc.mark").iter().any(|m| m["pattern"] == "stub-left"));
        assert!(events(&r.w, "task.steered").is_empty(), "{tag}\n{}", dump(&r.w));
        r.w.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap();
        r.w.close();
    }
    let r = rig("qc-pattern-without-numbers-4", &[("qc.live", "\"steer\"")], false, None);
    labelled(&r.w, "stub-left", 36, true);
    labelled(&r.w, "stub-left", 4, false);
    said(&r.w, ":qc on");
    until(&r.w, "the switch", || !events(&r.w, "qc.switch").is_empty());
    let task = working(&r.w);
    until(&r.w, "the live check", || events(&r.w, "qc.mark").iter().any(|m| m["pattern"] == "stub-left"));
    let m = events(&r.w, "qc.mark").into_iter().find(|m| m["pattern"] == "stub-left").unwrap();
    assert_eq!((m["steerable"].as_bool(), m["prec"].as_f64().map(|p| (p * 100.0).round())), (Some(false), Some(90.0)), "{m}");
    assert!(events(&r.w, "task.steered").is_empty());
    r.w.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap();
    r.w.close();
}

#[test]
fn the_screen_has_a_quality_page_with_the_open_marks_and_a_key_to_dismiss_one() {
    use j4n_core::front::serve::Serve;
    use j4n_proto::{Down, Up};
    let r = rig("qcpage", &[], false, None);
    said(&r.w, ":qc src/");
    let mut serve = Serve::new(r.w.front.clone());
    let (downs, _) = serve.up(Up::Watch { bind: "view:qc-marks".into() });
    let rows: Vec<_> = downs.iter().filter_map(|d| if let Down::Rows { rows, .. } = d { Some(rows.clone()) } else { None }).flatten().collect();
    assert_eq!(rows.len(), 1, "{downs:?}");
    assert_eq!(rows[0].cells.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["rs-unwrap", "src/a.rs:1", "mid", "1"]);
    assert!(rows[0].key.starts_with('m'), "{}", rows[0].key);
    r.w.close();
}

#[test]
fn a_request_from_a_rule_while_it_is_off_is_told_so_and_checks_nothing() {
    let r = rig("qcoff", &[], false, None);
    let row = said(&r.w, ":qc @live t1");
    assert_eq!(row.answer.as_deref(), Some("qc is off"));
    assert!(events(&r.w, "qc.run").is_empty() && events(&r.w, "qc.mark").is_empty());
    r.w.close();
}

#[test]
fn a_mark_steers_only_a_lane_at_work_and_only_for_a_high_pattern_that_is_not_jevs() {
    // found at settle: the lane is over, there is nobody to steer
    let r = rig("qc-mark-steers-only", &[("qc.live", "\"steer\"")], false, None);
    labelled(&r.w, "stub-left", 41, true);
    said(&r.w, ":qc on");
    until(&r.w, "the switch", || !events(&r.w, "qc.switch").is_empty());
    said(&r.w, "write src/c.rs pub fn c() -> u32 { todo!() }");
    until(&r.w, "the check at settle", || !events(&r.w, "qc.mark").is_empty());
    let m = events(&r.w, "qc.mark").remove(0);
    assert_eq!((m["pattern"].as_str(), m["live"].as_bool(), m["steerable"].as_bool(), m["prec"].as_f64()), (Some("stub-left"), Some(false), Some(false), Some(1.0)), "{m}");
    assert!(events(&r.w, "task.steered").is_empty());
    r.w.close();
    // found while it works, with every number in order: a mid pattern, and a pattern of Jev's
    let r = rig("qc-mark-steers-only-2", &[("qc.live", "\"steer\""), ("qc.jev", "true")], true, None);
    labelled(&r.w, "rs-unwrap", 41, true);
    labelled(&r.w, "swallowed-error", 41, true);
    said(&r.w, ":qc on");
    until(&r.w, "the switch", || !events(&r.w, "qc.switch").is_empty());
    // the line that switched it on may still be a task: the next line would steer it
    until(&r.w, "nothing at work", || r.w.front.tasks().book().rows().iter().all(|t| t.state.is_terminal()));
    let task = match r.w.front.say("main", "write src/c.rs pub fn c(s: &str) -> u32 { s.parse().unwrap() }; wait 6500; say done") {
        Said::Started { task } => task,
        other => panic!("{other:?}"),
    };
    until(&r.w, "both tiers", || events(&r.w, "qc.mark").iter().any(|m| m["pattern"] == "rs-unwrap") && events(&r.w, "qc.mark").iter().any(|m| m["pattern"] == "swallowed-error"));
    for m in events(&r.w, "qc.mark").iter().filter(|m| m["live"] == true) {
        assert_eq!((m["steerable"].as_bool(), m["n"].as_u64(), m["prec"].as_f64()), (Some(false), Some(41), Some(1.0)), "{m}");
    }
    assert!(events(&r.w, "task.steered").is_empty());
    r.w.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap();
    r.w.close();
}

#[test]
fn writes_a_moment_apart_are_checked_together_once_they_have_stopped() {
    let r = rig("qc-writes-moment-apart", &[("qc.live", "\"show\"")], false, None);
    said(&r.w, ":qc on");
    until(&r.w, "the switch", || !events(&r.w, "qc.switch").is_empty());
    // the line that switched it on may still be a task: the next line would steer it
    until(&r.w, "nothing at work", || r.w.front.tasks().book().rows().iter().all(|t| t.state.is_terminal()));
    let task = match r.w.front.say("main", "write src/c.rs pub fn c() -> u32 { todo!() }; write src/d.rs pub fn d() -> u32 { todo!() }; wait 6500; say done") {
        Said::Started { task } => task,
        other => panic!("{other:?}"),
    };
    until(&r.w, "the live check", || !events(&r.w, "qc.run").is_empty());
    let marks = marks_in(&r.w);
    assert!(marks.contains(&("stub-left".to_string(), "src/c.rs".to_string(), 1)) && marks.contains(&("stub-left".to_string(), "src/d.rs".to_string(), 1)), "{marks:?}");
    assert_eq!(events(&r.w, "qc.run").len(), 1, "one check for the two writes");
    assert_eq!(events(&r.w, "qc.run")[0]["trigger"], "live");
    r.w.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap();
    r.w.close();
}

#[test]
fn a_part_whose_call_failed_makes_the_run_partial() {
    let r = rig("qc-part-whose-call", &[("qc.jev", "true")], true, None);
    *r.reply.lock().unwrap() = Reply::RefuseFile("src/util.rs");
    said(&r.w, ":qc src/");
    let run = run_of(&r.w, "src/");
    assert_eq!((run["status"].as_str(), run["coverage"].as_u64(), run["errors"].as_array().map(Vec::len)), (Some("partial"), Some(50), Some(1)), "{run}");
    r.w.close();
}

#[test]
fn three_runs_whose_every_call_failed_open_the_breaker_until_qc_on() {
    let r = rig("qc-three-runs-whose", &[("qc.jev", "true")], true, None);
    *r.reply.lock().unwrap() = Reply::Refuse;
    for _ in 0..3 {
        said(&r.w, ":qc src/");
    }
    assert_eq!(checks(&r.jev).len(), 6, "two parts a run");
    said(&r.w, ":qc src/");
    assert_eq!(checks(&r.jev).len(), 6, "the fourth run did not ask");
    let run = events(&r.w, "qc.run").pop().unwrap();
    assert!(run["errors"][0]["reason"].as_str().unwrap().contains("3 times in a row"), "{run}");
    *r.reply.lock().unwrap() = Reply::Normal;
    said(&r.w, ":qc on");
    said(&r.w, ":qc src/");
    assert_eq!(checks(&r.jev).len(), 8);
    assert_eq!(events(&r.w, "qc.run").pop().unwrap()["status"], "full");
    r.w.close();
}

#[test]
fn what_the_egress_gate_blocks_is_not_sent_and_a_secret_in_the_code_is_not_in_what_is_sent() {
    let gate = "api = 1\n[[gate]]\nid = \"no-code\"\ndoc = \"No code leaves.\"\non = [\"egress\"]\nwhen = \"action.class == 'project'\"\nverdict = \"block\"\nwhy = \"no code leaves this machine\"\n";
    let r = rig_with("qc-what-egress-gate", &[("qc.jev", "true")], true, None, &[("gates/mine.toml", gate)]);
    said(&r.w, ":qc src/");
    let run = run_of(&r.w, "src/");
    assert_eq!(run["status"], "0-only");
    assert!(run["errors"][0]["reason"].as_str().unwrap().contains("blocked"), "{run}");
    assert!(checks(&r.jev).is_empty());
    r.w.close();
    let r = rig("qc-what-egress-gate-2", &[("qc.jev", "true")], true, None);
    r._d.put("src/a.rs", "// key typesafe-test-0001\npub fn parse_number(s: &str) -> u32 { s.parse().unwrap() }\n");
    said(&r.w, ":qc src/a.rs");
    let calls = checks(&r.jev);
    assert_eq!(calls.len(), 1);
    assert!(!calls[0].to_string().contains("typesafe-test-0001"), "the key went to Jev: {}", calls[0]["state"]["delivered"]);
    r.w.close();
}

#[test]
fn a_check_can_be_asked_to_stay_with_the_first_tier() {
    let r = rig("qc-check-can-be", &[("qc.jev", "true")], true, None);
    said(&r.w, ":qc src/ --tiers 0");
    assert!(checks(&r.jev).is_empty());
    let run = run_of(&r.w, "src/");
    assert_eq!((run["status"].as_str(), run["errors"].as_array().map(Vec::len)), (Some("0-only"), Some(0)));
    r.w.close();
}

#[test]
fn only_the_last_two_hundred_labels_count_for_a_pattern() {
    let r = rig("qc-only-last-two", &[], false, None);
    labelled(&r.w, "rs-unwrap", 60, false);
    labelled(&r.w, "rs-unwrap", 200, true);
    said(&r.w, ":qc src/");
    let m = events(&r.w, "qc.mark").remove(0);
    assert_eq!((m["n"].as_u64(), m["prec"].as_f64()), (Some(200), Some(1.0)), "{m}");
    r.w.close();
}

/// The newest `qc.mark` of a pattern.
fn last_mark(w: &World, pattern: &str) -> Value {
    events(w, "qc.mark").into_iter().rev().find(|m| m["pattern"] == pattern).unwrap_or_else(|| panic!("no mark of {pattern}\n{}", dump(w)))
}

#[test]
fn only_a_dismissal_that_says_wrong_lowers_a_pattern_and_the_others_are_not_counted() {
    let r = rig("qc-only-dismissal-says", &[], false, None);
    said(&r.w, ":qc src/");
    let id = events(&r.w, "qc.mark").remove(0)["mark"].as_str().unwrap().to_string();
    // a word that is not a reason touches nothing
    said(&r.w, &format!(":qc dismiss {id} because whatever"));
    said(&r.w, &format!(":qc dismiss {id} because"));
    assert!(events(&r.w, "label").is_empty() && events(&r.w, "qc.mark.state").is_empty(), "{}", dump(&r.w));
    // three reasons that are about the code, not the pattern
    for reason in ["intended", "pre-existing", "later"] {
        let row = said(&r.w, &format!(":qc dismiss {id} because {reason}"));
        assert!(row.answer.unwrap_or_default().ends_with(&format!("because {reason}")), "{reason}");
        said(&r.w, ":qc src/");
        let m = last_mark(&r.w, "rs-unwrap");
        assert_eq!((m["state"].as_str(), m["n"].as_u64(), m["prec"].as_f64()), (Some("dismissed"), Some(0), Some(0.0)), "{reason}: dismissed, and not counted\n{m}");
    }
    let labels = events(&r.w, "label");
    assert_eq!(labels.iter().map(|l| l["point"].as_str().unwrap()).collect::<Vec<_>>(), ["intended", "pre-existing", "later"]);
    assert_eq!(events(&r.w, "qc.mark.state").last().unwrap()["reason"], "later");
    // `wrong` counts as a miss, and so does a dismissal that says nothing (what it always was)
    for (line, n) in [(format!(":qc dismiss {id} because wrong"), 1), (format!(":qc dismiss {id}"), 2)] {
        said(&r.w, &line);
        said(&r.w, ":qc src/");
        let m = last_mark(&r.w, "rs-unwrap");
        assert_eq!((m["n"].as_u64(), m["prec"].as_f64()), (Some(n), Some(0.0)), "{line}\n{m}");
    }
    r.w.close();
}

#[test]
fn an_uncounted_dismissal_is_not_one_of_the_forty_labels_that_let_a_pattern_steer() {
    let r = rig("qc-uncounted-dismissal-not", &[("qc.live", "\"steer\"")], false, None);
    labelled(&r.w, "stub-left", 39, true);
    // forty dismissals that were about the code: the pattern has 39 labels, so it does not steer
    for i in 0..40 {
        let data = json!({ "subject_kind": "qc_mark", "subject": format!("m{i:08x}"), "producer": "stub-left", "model": "", "point": "intended", "label": "dismissed", "weight": 1.0 });
        r.w.front.log().append(New::of("label", Actor::Operator, data)).unwrap();
    }
    said(&r.w, ":qc on");
    until(&r.w, "the switch", || !events(&r.w, "qc.switch").is_empty());
    let task = working(&r.w);
    until(&r.w, "the live check", || events(&r.w, "qc.mark").iter().any(|m| m["pattern"] == "stub-left"));
    let m = last_mark(&r.w, "stub-left");
    assert_eq!((m["n"].as_u64(), m["steerable"].as_bool()), (Some(39), Some(false)), "{m}");
    assert!(events(&r.w, "task.steered").is_empty());
    r.w.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap();
    r.w.close();
}

/// The flow `refactor` of the pack, with the work step done by the scripted doer (the model that would do it is not here).
fn refactor_flow() -> String {
    let shipped = include_str!("../../../../packs/qc/flows/qc.toml");
    let work = "  { id = \"work\", do = \"test\", mode = \"write\", after = [\"keep\"], goal = \"{in.goal}\" },";
    shipped.lines().map(|l| if l.contains("id = \"work\"") { work } else { l }).collect::<Vec<_>>().join("\n")
}

fn refactored(w: &World, line: &str) -> j4n_core::task::Row {
    let row = said(w, line);
    assert!(matches!(row.state, j4n_core::task::State::Done { ok: true }), "a refactor is never stopped: {:?}\n{}", row.state, dump(w));
    row
}

#[test]
fn a_refactor_that_removes_a_public_name_is_told_as_a_mark_and_is_not_stopped() {
    let flow = refactor_flow();
    let r = rig_with("qc-refactor-removes-public", &[], false, None, &[("flows/refactor.toml", flow.as_str())]);
    // parse_number goes (src/a.rs line 1); parse_numeric and the heading of the README stay
    let row = refactored(&r.w, ":refactor write src/a.rs pub fn parse_one(s: &str) -> u32 { 1 }");
    let answer = row.answer.unwrap_or_default();
    assert!(answer.contains("keep-removed  src/a.rs:1 mid  1.00  removed: fn parse_number") && answer.contains("keep: 1 of "), "{answer}");
    assert!(!answer.contains("parse_numeric") && !answer.contains("heading"), "{answer}");
    let m = last_mark(&r.w, "keep-removed");
    assert_eq!((m["file"].as_str(), m["line"].as_u64(), m["answer"].as_str(), m["state"].as_str(), m["steerable"].as_bool()), (Some("src/a.rs"), Some(1), Some("removed: fn parse_number"), Some("open"), Some(false)), "{m}");
    assert_eq!(events(&r.w, "qc.keep").iter().map(|k| k["step"].as_str().unwrap().to_string()).collect::<Vec<_>>(), ["snapshot", "compare"]);
    assert_eq!(events(&r.w, "qc.keep")[1]["removed"], 1);
    // the mark is on the page of open marks, like any other
    let marks = events(&r.w, "qc.mark").len();
    // a function moved to another file is kept, and one that is only added is not news
    let row = refactored(&r.w, ":refactor write src/b.rs pub fn parse_numeric(s: &str) -> u32 { 2 }; write src/util.rs pub fn util_other() {}");
    assert!(row.answer.unwrap_or_default().contains("keep: nothing is gone"), "moved is not gone");
    assert_eq!(events(&r.w, "qc.mark").len(), marks, "no new mark");
    // a reason of the operator's own: this removal was meant, and the pattern is not lowered by it
    let id = m["mark"].as_str().unwrap();
    said(&r.w, &format!(":qc dismiss {id} because intended"));
    refactored(&r.w, ":refactor write src/util.rs pub fn util_third() {}");
    let again = last_mark(&r.w, "keep-removed");
    assert_eq!((again["answer"].as_str(), again["n"].as_u64(), again["prec"].as_f64()), (Some("removed: fn util_other"), Some(0), Some(0.0)), "{again}");
    // the heading of a document, in its own file
    refactored(&r.w, ":refactor write README.md just words");
    assert_eq!(last_mark(&r.w, "keep-removed")["answer"], "removed: heading demo");
    r.w.close();
}

#[test]
fn the_surface_the_project_chooses_is_the_only_one_kept() {
    let flow = refactor_flow();
    let r = rig_with("qc-surface-project-chooses", &[("qc.keep", "\"md-heading\"")], false, None, &[("flows/refactor.toml", flow.as_str())]);
    let row = refactored(&r.w, ":refactor write src/a.rs pub fn parse_one(s: &str) -> u32 { 1 }");
    assert!(row.answer.unwrap_or_default().contains("keep: nothing is gone of the 1 names"), "only the heading is kept");
    assert!(events(&r.w, "qc.mark").is_empty());
    r.w.close();
}

#[test]
fn with_the_switch_on_the_state_and_the_verdict_say_lexical_and_with_it_off_nothing_is_added() {
    let r = rig("qc-switch-on-state", &[("qc.jev", "true"), ("qc.provenance", "true")], true, None);
    *r.reply.lock().unwrap() = Reply::Helper;
    let row = said(&r.w, ":qc src/a.rs");
    let calls = checks(&r.jev);
    assert_eq!(calls.len(), 1);
    assert_eq!((calls[0]["state"]["basis"]["callers"].as_str(), calls[0]["state"]["basis"]["similar"].as_str()), (Some("~lexical"), Some("~lexical")), "{}", calls[0]["state"]);
    // the verdict that picks among look-alikes rests on word matches alone; the questions about the diff do not
    let m = last_mark(&r.w, "duplicate-helper");
    assert_eq!((m["answer"].as_str(), m["basis"].as_str()), (Some("parse_numeric"), Some("~lexical")), "{m}");
    assert!(last_mark(&r.w, "swallowed-error").get("basis").is_none() && last_mark(&r.w, "rs-unwrap").get("basis").is_none());
    let answer = row.answer.unwrap_or_default();
    assert!(answer.lines().any(|l| l.starts_with("duplicate-helper") && l.ends_with(" ~lexical")), "{answer}");
    assert!(answer.lines().filter(|l| l.contains("~lexical")).count() == 1, "{answer}");
    r.w.close();
    // off: what Jev is sent and what is written are what they were
    let r = rig("qc-switch-on-state-2", &[("qc.jev", "true")], true, None);
    *r.reply.lock().unwrap() = Reply::Helper;
    let row = said(&r.w, ":qc src/a.rs");
    assert!(checks(&r.jev)[0]["state"].get("basis").is_none());
    assert!(last_mark(&r.w, "duplicate-helper").get("basis").is_none());
    assert!(!row.answer.unwrap_or_default().contains("lexical"));
    r.w.close();
}
