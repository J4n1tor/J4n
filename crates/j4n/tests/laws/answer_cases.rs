//! `j4n answer`, the one surface an outside harness asks deciders through. A probe-shaped line in, one JSON
//! line out; the chain of the point is the one a headless run walks, no doer is started and no Change is made. The binary is run
//! as such a harness runs it; no model and no network are in it, so the answerers are the rules and the points' fallbacks.

use crate::run_cases::{Dirs, Out, j4n, kinds, log_events};
use j4n_core::gate::danger::danger;
use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

const INTAKE: [&str; 5] = ["answer", "command", "setting", "task", "flow"];

/// A probe-shaped line of the point `intake`.
fn intake(id: &str, line: &str, offered: &[&str]) -> String {
    json!({ "id": id, "point": "intake", "slices": { "line": line, "thread": "new" }, "offered": offered }).to_string()
}

/// The lines `out` printed, each as JSON.
fn replies(out: &Out) -> Vec<Value> {
    out.out.lines().map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{l:?} is not JSON: {e}"))).collect()
}

fn run(d: &Dirs, args: &[&str], lines: &[String]) -> (Out, Vec<Value>) {
    let mut stdin = lines.join("\n");
    stdin.push('\n');
    let out = j4n(d, args, Some(&stdin), &[]);
    let got = replies(&out);
    (out, got)
}

/// Every file under `dir`, relative, except those under `skip`.
fn files(dir: &Path, skip: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(here) = stack.pop() {
        for e in fs::read_dir(&here).into_iter().flatten().flatten() {
            let rel = e.path().strip_prefix(dir).unwrap().to_string_lossy().into_owned();
            if rel == skip {
                continue;
            }
            if e.path().is_dir() { stack.push(e.path()) } else { found.push(rel) }
        }
    }
    found.sort();
    found
}

/// Agent programs on the PATH that leave a mark in HOME when they are run.
fn agents(d: &Dirs) -> String {
    for name in ["claude", "codex", "gemini", "opencode", "crush", "pi", "omp"] {
        let path = d.dir.join("bin").join(name);
        fs::write(&path, format!("#!/bin/sh\necho ran > \"$HOME/ran-{name}\"\necho \"{name} 1.0.0\"\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    format!("{}:{}", d.dir.join("bin").display(), std::env::var("PATH").unwrap_or_default())
}

fn step<'a>(reply: &'a Value, answerer: &str) -> &'a Value {
    reply["steps"].as_array().unwrap().iter().find(|s| s["answerer"] == answerer).unwrap_or_else(|| panic!("no step of {answerer}: {reply}"))
}

#[test]
fn an_intake_line_gives_one_json_line_with_its_answer_and_starts_no_doer_and_changes_no_file() {
    let d = Dirs::new("answer-intake-line-gives");
    let path = agents(&d);
    let lines = [intake("i-1", ":help", &INTAKE), intake("i-2", "add a --verbose flag to the cli", &INTAKE)];
    let mut stdin = lines.join("\n");
    stdin.push('\n');
    let out = j4n(&d, &["answer"], Some(&stdin), &[("PATH", path.as_str())]);
    assert_eq!(out.rc, 0, "{}", out.err);
    let got = replies(&out);
    assert_eq!(got.len(), 2, "one line out for each line in: {}", out.out);

    // a command is the rule's: it answers, sure of itself, and nothing else is asked
    let a = &got[0];
    assert_eq!((a["id"].as_str(), a["answer"].as_str(), a["by"].as_str()), (Some("i-1"), Some("command"), Some("rule:intake")), "{a}");
    assert_eq!(a["confidence"].as_f64(), Some(1.0), "{a}");
    assert!(a["ms"].is_u64() && a["usd"].as_f64() == Some(0.0), "{a}");
    assert_eq!(step(a, "rule:intake")["outcome"], "answer");
    assert_eq!(a["steps"].as_array().unwrap().len(), 1, "the walk ended at the first answer: {a}");

    // a line of work is nobody's here: the rule passes, the decider is not installed, and the point's fallback stands, said as such
    let b = &got[1];
    assert_eq!((b["id"].as_str(), b["answer"].as_str(), b["by"].as_str()), (Some("i-2"), Some("task"), Some("fallback:pick")), "{b}");
    assert!(b["confidence"].is_null() && b["ms"].is_u64() && b["usd"].as_f64() == Some(0.0), "{b}");
    assert_eq!(step(b, "rule:intake")["outcome"], "pass");
    let jev = step(b, "decider:jev");
    assert_eq!(jev["outcome"], "skipped", "{b}");
    assert!(jev["why"].as_str().unwrap().contains("not installed"), "a quiz can tell never answered from wrong: {b}");

    // no doer: no program was run, no task is in the Log, and the folder holds nothing of its own but `.j4n/`
    let marks: Vec<_> = fs::read_dir(d.home()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("ran-")).collect();
    assert!(marks.is_empty(), "an agent program was run: {marks:?}");
    assert_eq!(files(&d.project(), ".j4n"), Vec::<String>::new(), "the folder is as it was");
    let kinds = kinds(&log_events(&d));
    assert!(kinds.iter().all(|k| !k.starts_with("task.") && !k.starts_with("change.") && !k.starts_with("run.")), "{kinds:?}");
    // the Log keeps what the chain did, as a run's would
    assert!(kinds.iter().any(|k| k == "ask.step"), "{kinds:?}");
}

#[test]
fn by_asks_one_answerer_only_and_an_answer_outside_what_is_offered_cannot_come_back() {
    let d = Dirs::new("answer-asks-one-answerer");
    let lines = [
        intake("a", ":help", &INTAKE),
        intake("b", "add a --verbose flag", &INTAKE),
        // the rule's answer is `command`, which this question does not offer
        intake("c", ":help", &["answer", "task"]),
        intake("d", ":help", &INTAKE),
    ];
    let (out, got) = run(&d, &["answer", "--by", "rule:intake"], &lines[..3]);
    assert_eq!(out.rc, 0, "{}", out.err);
    assert_eq!(got.len(), 3);
    for r in &got {
        assert_eq!(r["steps"].as_array().unwrap().len(), 1, "that answerer only: {r}");
        assert_eq!(r["steps"][0]["answerer"], "rule:intake");
    }
    assert_eq!((got[0]["answer"].as_str(), got[0]["by"].as_str()), (Some("command"), Some("rule:intake")));
    // it passes: no fallback stands in for it, the question was for the answerer
    assert!(got[1]["answer"].is_null() && got[1]["by"].is_null() && got[1]["confidence"].is_null(), "{}", got[1]);
    assert_eq!(got[1]["steps"][0]["outcome"], "pass");
    // an answer that is not an option is not an answer
    assert!(got[2]["answer"].is_null() && got[2]["by"].is_null(), "{}", got[2]);
    assert_eq!(got[2]["steps"][0]["outcome"], "failed");
    assert!(got[2]["steps"][0]["why"].as_str().unwrap().contains("not valid"), "{}", got[2]);

    // an answerer that is not installed is said so; one that is not in the chain of the point is an error, and the run goes on
    let (_, got) = run(&d, &["answer", "--by=decider:jev"], &lines[3..]);
    assert!(got[0]["answer"].is_null() && got[0]["steps"][0]["outcome"] == "skipped", "{}", got[0]);
    let (out, got) = run(&d, &["answer", "--by", "rule:fold"], &lines[3..]);
    assert!(got[0]["error"].as_str().unwrap().contains("rule:fold") && got[0]["id"] == "d", "{}", got[0]);
    assert_eq!(out.rc, 0);

    // whoever answers, the answer is among the options: the point's fallback is held to them too
    let (_, got) = run(&d, &["answer"], &[intake("e", "add a flag", &["answer", "flow"])]);
    assert!(["answer", "flow"].contains(&got[0]["answer"].as_str().unwrap_or("")), "{}", got[0]);
}

#[test]
fn gate_gives_the_class_danger_gives_for_each_command() {
    let d = Dirs::new("answer-gate-gives-class");
    let cmds = [
        ("g-1", "python3 --version", "read"),
        ("g-2", "pip list", "read"),
        ("g-3", "echo hi > /dev/tcp/127.0.0.1/80", "write"),
        ("g-4", "rm -rf /", "wipe"),
        ("g-5", "git push origin main", "push"),
    ];
    let lines: Vec<String> = cmds.iter().map(|(id, cmd, _)| json!({ "id": id, "gate": { "cmd": cmd } }).to_string()).collect();
    let (out, got) = run(&d, &["answer", "--gate"], &lines);
    assert_eq!(out.rc, 0, "{}", out.err);
    assert_eq!(got.len(), cmds.len());
    for ((id, cmd, class), r) in cmds.iter().zip(&got) {
        assert_eq!((r["id"].as_str(), r["class"].as_str()), (Some(*id), Some(*class)), "{cmd}: {r}");
        // the matcher the Gate's rules read, on the same line
        assert_eq!(r["class"], danger(cmd).class, "{cmd}");
        assert_eq!(r["detail"]["program"], danger(cmd).program, "{cmd}");
    }
    assert_ne!(got[2]["class"], "wipe", "a connection to a port is never a wipe");
    assert!(got[3]["detail"]["classes"].as_array().unwrap().iter().any(|c| c == "wipe"));
    // nothing but the matcher ran: no world was opened, no file was made anywhere
    assert_eq!(files(&d.project(), ""), Vec::<String>::new());
    assert_eq!(files(&d.home(), ""), [".j4n/legacy-packs.toml", ".j4n/settings/no-watch.toml", ".j4n/settings/setup-card.toml"]);
}

#[test]
fn a_malformed_line_gives_an_error_line_with_its_id_and_the_run_goes_on() {
    let d = Dirs::new("answer-malformed-line-gives");
    let lines = [
        intake("ok-1", ":help", &INTAKE),
        "this is not json".to_string(),
        json!({ "id": "no-point", "slices": {}, "offered": [] }).to_string(),
        json!({ "id": "bad-point", "point": "no-such-point", "slices": { "line": "x" }, "offered": ["a", "b"] }).to_string(),
        json!({ "id": "bad-offer", "point": "intake", "slices": { "line": "x" }, "offered": "task" }).to_string(),
        json!({ "id": "no-offer", "point": "intake", "slices": { "line": "x" }, "offered": [] }).to_string(),
        "[1, 2]".to_string(),
        intake("ok-2", "add a flag", &INTAKE),
    ];
    let (out, got) = run(&d, &["answer"], &lines);
    assert_eq!(out.rc, 0, "some lines were taken: {}", out.err);
    assert_eq!(got.len(), lines.len(), "one line out for each line in: {}", out.out);
    assert_eq!(got[0]["answer"], "command");
    assert!(got[1]["id"].is_null() && got[1]["error"].is_string(), "{}", got[1]);
    for (n, id) in [(2, "no-point"), (3, "bad-point"), (4, "bad-offer"), (5, "no-offer")] {
        assert_eq!(got[n]["id"], id, "{}", got[n]);
        assert!(got[n]["error"].as_str().is_some_and(|e| !e.is_empty()) && got[n].get("answer").is_none(), "{}", got[n]);
    }
    assert!(got[6]["id"].is_null() && got[6]["error"].is_string(), "{}", got[6]);
    assert_eq!((got[7]["id"].as_str(), got[7]["answer"].as_str()), (Some("ok-2"), Some("task")), "the run went on: {}", got[7]);

    // `--gate` takes the same lines the same way
    let (out, got) = run(&d, &["answer", "--gate"], &["{\"id\": \"g\", \"gate\": {}}".to_string(), "oops".to_string(), "{\"id\": \"h\", \"gate\": {\"cmd\": \"ls\"}}".to_string()]);
    assert_eq!((out.rc, got.len()), (0, 3));
    assert!(got[0]["id"] == "g" && got[0]["error"].is_string() && got[1]["error"].is_string() && got[2]["class"] == "safe", "{got:?}");

    // nothing on stdin is nothing to answer; stdin that is not JSON lines at all is the one failure
    let (out, got) = run(&d, &["answer"], &[]);
    assert_eq!((out.rc, got.len()), (0, 0));
    let (out, got) = run(&d, &["answer"], &["hello".to_string(), "world".to_string()]);
    assert_eq!((out.rc, got.len()), (1, 2), "{}", out.out);
    assert!(got.iter().all(|r| r["error"].is_string()));
    let out = j4n(&d, &["answer"], Some("\u{0}\u{1}\n"), &[]);
    assert_eq!(out.rc, 1);
}

/// A line of the point `plan`, as a harness's `plan` drill writes it: yes/no, `offered` empty.
fn plan(id: &str, line: &str) -> String {
    json!({ "id": id, "point": "plan", "slices": { "line": line }, "offered": [] }).to_string()
}

#[test]
fn the_point_plan_is_answered_yes_or_no_by_rule_plan_through_answer() {
    let d = Dirs::new("answer-point-plan-answered");
    let lines = [
        plan("p-1", "1. move take_token into limit/window.py\n2. cache the quota\n3. run the tests"),
        plan("p-2", "1. move take_token into limit/window.py\n2. cache the quota"),
        plan("p-3", "make a plan for moving the queue into sched/jobs.py"),
        plan("p-4", "did you make a plan for moving the queue?"),
    ];
    let (out, got) = run(&d, &["answer"], &lines);
    assert_eq!(out.rc, 0, "{}", out.err);
    assert_eq!(got.len(), lines.len(), "{}", out.out);
    for (r, (id, want)) in got.iter().zip([("p-1", "yes"), ("p-2", "no"), ("p-3", "yes"), ("p-4", "no")]) {
        assert_eq!((r["id"].as_str(), r["answer"].as_str(), r["by"].as_str()), (Some(id), Some(want), Some("rule:plan")), "{r}");
        assert_eq!((r["confidence"].as_f64(), r["usd"].as_f64()), (Some(1.0), Some(0.0)), "{r}");
        assert_eq!(step(r, "rule:plan")["outcome"], "answer", "{r}");
        assert_eq!(r["steps"].as_array().unwrap().len(), 1, "no model is in its chain: {r}");
    }
    // `--by rule:plan` is the same walk, and the rule always answers: the fallback never stands in for it
    let (_, by) = run(&d, &["answer", "--by", "rule:plan"], &lines[..2]);
    assert_eq!((by[0]["answer"].as_str(), by[1]["answer"].as_str()), (Some("yes"), Some("no")), "{by:?}");
    // the point is a yes/no: options are an error, and the run goes on
    let (_, bad) = run(&d, &["answer"], &[json!({ "id": "p-5", "point": "plan", "slices": { "line": "plan it" }, "offered": ["yes", "no"] }).to_string(), plan("p-6", "plan it")]);
    assert!(bad[0]["error"].is_string() && bad[1]["answer"] == "yes", "{bad:?}");
}

#[test]
fn the_arguments_of_answer_are_by_and_gate_and_nothing_else() {
    let d = Dirs::new("answer-args");
    for args in [&["answer", "--by"][..], &["answer", "--model", "x"], &["answer", "words"], &["answer", "--gate", "--by", "rule:intake"]] {
        let out = j4n(&d, args, Some(""), &[]);
        assert_eq!(out.rc, 2, "{args:?}: {}", out.err);
    }
    let out = j4n(&d, &["answer", "--help"], None, &[]);
    assert!(out.rc == 0 && out.out.contains("--gate") && out.out.contains("--by"), "{}", out.out);
}
