//! The report and the ledger of branches through the real
//! front and the test doer, and the claim check, the fold and the Brief section on their own.

use crate::front_cases::{Fx, fx};
use crate::scripted::{Beat, Scripted, end};
use crate::world::Scratch;
use j4n_core::change::Actor;
use j4n_core::doer::Caps;
use j4n_core::fold::Fold;
use j4n_core::lanes::Lanes;
use j4n_core::log::Ev;
use j4n_core::report::Report;
use j4n_core::task::{Ctl, TaskSpec};
use serde_json::{Value, json};
use std::path::Path;

const NET: &str = "use std::io;\n// the retry lives here\nfn fetch() { retry(3) }\n";

fn report(extra: Value) -> String {
    let mut r = json!({ "answer": "the retry is in fetch()", "sure": "mid", "found": [], "ran": [], "open": [] });
    r.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    r.to_string()
}

/// A task of `doer` on the main thread, under `parent` when it has one.
fn spawn(f: &Fx, doer: &str, role: &str, parent: Option<&str>) -> String {
    let mut spec = TaskSpec::new(&format!("{role} work"), doer);
    spec.thread = Some("main".into());
    spec.role = Some(role.into());
    spec.parent = parent.map(String::from);
    f.rig.tasks.spawn(spec, Actor::Operator).expect("spawns")
}

fn parts() -> (std::sync::Arc<Scripted>, Scratch) {
    let src = Scratch::new("net");
    src.put("src/net.rs", NET);
    (Scripted::new("boss", vec![vec![Beat::Forever]]), src)
}

// ---- the claim check ----

#[test]
fn a_report_has_a_closed_shape_and_a_branch_cannot_say_it_checked_itself() {
    assert!(Report::take(&report(json!({}))).is_ok());
    let wrapped = format!("Here you are:\n```json\n{}\n```", report(json!({})));
    assert!(Report::take(&wrapped).is_ok(), "a doer may wrap it in words");
    for bad in [
        report(json!({ "sure": "certain" })),
        report(json!({ "extra": 1 })),
        report(json!({ "found": [{ "claim": "a", "at": "b", "unchecked": false }] })),
        report(json!({ "ran": [{ "cmd": "ls", "exit": "0" }] })),
        report(json!({ "answer": "  " })),
        "no object here".to_string(),
    ] {
        assert!(Report::take(&bad).is_err(), "{bad}");
    }
    let r = Report::take(&report(json!({ "found": [{ "claim": "x", "at": "src/net.rs:3" }], "ran": [{ "cmd": "ls", "exit": 0 }] }))).unwrap();
    assert!(r.found[0].unchecked && r.ran[0].unchecked, "nothing is checked until the check has been over it");
    assert_eq!((r.changed.as_str(), r.unchecked()), ("none", 2));
}

#[test]
fn an_open_point_written_as_an_object_is_read_as_its_words_and_not_lost() {
    // a review wrote its points as objects: the report was refused as no report, and the piece was taken as having no concern
    let said = report(json!({ "open": [{ "issue": "moving a pinned piece raises AttributeError", "at": "pieces.py:97" }, { "file": "app.py", "line": 91, "what": "a stale size after resize" }, "game.py:3 the score never resets", { "severity": 2 }] }));
    let r = Report::take(&said).expect("a report");
    assert_eq!(r.open, ["pieces.py:97 moving a pinned piece raises AttributeError", "app.py 91 a stale size after resize", "game.py:3 the score never resets", "2"]);
}

#[test]
fn a_claim_holds_only_when_the_line_exists_and_holds_a_word_of_it() {
    let (_, src) = parts();
    let at = |at: &str, claim: &str| {
        let mut r = Report::take(&report(json!({ "found": [{ "claim": claim, "at": at }] }))).unwrap();
        r.check(src.path(), &[]);
        !r.found[0].unchecked
    };
    assert!(at("src/net.rs:3", "the retry is in fetch()"));
    assert!(at("src/net.rs:2", "where RETRY happens"), "the case of a word does not matter");
    assert!(!at("src/net.rs:1", "the retry is in fetch()"), "a line with none of its words");
    assert!(!at("src/net.rs:4", "the retry is in fetch()"), "past the last line");
    assert!(!at("src/net.rs:0", "the retry is in fetch()"), "lines count from one");
    assert!(!at("src/gone.rs:3", "the retry is in fetch()"), "a file that is not there");
    assert!(!at("src/net.rs:2", "the and for"), "words that say nothing do not make a claim hold");
    assert!(!at("src/net.rs:1", "io"), "nor a word of two letters");
    assert!(!at("src/net.rs", "retry"), "no line");
    assert!(!at("src/../src/net.rs:3", "retry"), "a path that leaves the folder is not looked up, even where it comes back");
    assert!(!at(&format!("{}/src/net.rs:3", src.path().display()), "retry") && !at("https://x.dev/src/net.rs:3", "retry"), "nor an absolute one, nor a URL");
    src.put("big.txt", &format!("retry{}", " ".repeat(5 << 20)));
    assert!(!at("big.txt:1", "retry"), "a file past the size a claim is looked up in");
    assert!(!at("src:1", "retry"), "a folder is not a file");
}

#[test]
fn a_ran_holds_for_a_command_inside_a_line_the_branch_ran_and_its_program_is_its_file_name() {
    // the doer ran compound lines and reported the commands in them, as a chat may
    let said = json!({ "ran": [{ "cmd": "api/.venv/bin/pytest -q", "exit": 0 }, { "cmd": "go vet ./...", "exit": 0 }, { "cmd": "gofmt -w . && gofmt -l .", "exit": 0 }, { "cmd": "go vet ./...", "exit": 2 }, { "cmd": "pytest -x", "exit": 0 }, { "cmd": "go", "exit": 0 }, { "cmd": "go vet ./... (in cli/, first run)", "exit": 0 }, { "cmd": "go vet ./... (cli)x", "exit": 0 }, { "cmd": "cd docs && make build", "exit": 0 }, { "cmd": "cd docs", "exit": 0 }] });
    let mut r = Report::take(&report(said)).unwrap();
    let runs = [("cd api && .venv/bin/pytest -q 2>&1 | tail -1; echo $?".to_string(), 0), ("cd cli && go vet ./... && go build -o /tmp/x .\ngofmt -w . && gofmt -l .".to_string(), 0), ("cd /w/t3/docs && make build; echo $?".to_string(), 0)];
    r.check(Path::new("."), &runs);
    assert_eq!(r.ran.iter().map(|x| x.unchecked).collect::<Vec<_>>(), [false, false, false, true, true, true, false, true, false, true], "a command of a run holds, with a note after it or a `cd` before it too; another exit, other words or a part of a command do not");
}

#[test]
fn a_ran_holds_only_for_a_command_and_an_exit_the_branch_itself_ran() {
    let mut r = Report::take(&report(json!({ "ran": [{ "cmd": "cargo test -p net", "exit": 0 }, { "cmd": "cargo test -p net", "exit": 1 }, { "cmd": "cargo build", "exit": 0 }] }))).unwrap();
    r.check(Path::new("."), &[("cargo test -p net ".into(), 0)]);
    assert_eq!(r.ran.iter().map(|x| x.unchecked).collect::<Vec<_>>(), [false, true, true]);
    assert_eq!(r.ran.len(), 3, "a claim that fails is marked, never dropped");
}


#[test]
fn a_report_whose_claims_are_not_in_the_files_or_the_log_is_marked_unchecked_before_the_main_lane_sees_it() {
    let (boss, src) = parts();
    let answer = report(json!({
        "found": [
            { "claim": "the retry is in fetch()", "at": "src/net.rs:3" },
            { "claim": "the retry is in fetch()", "at": "src/net.rs:99" },
            { "claim": "the retry is in fetch()", "at": "src/gone.rs:3" },
        ],
        "ran": [{ "cmd": "cargo test -p net", "exit": 0 }, { "cmd": "cargo build", "exit": 0 }],
    }));
    let scout = Scripted::new("scout", vec![vec![Beat::Shell("cargo test -p net".into(), "ok".into(), 0), end(&answer)]]);
    let f = fx(vec![boss, scout], |c| c.root = src.path().to_path_buf());
    let main = spawn(&f, "boss", "work", None);
    let branch = spawn(&f, "scout", "scout", Some(&main));
    f.rig.finished(&branch);
    let r = f.front.report(&branch).expect("a report");
    assert_eq!(r.found.iter().map(|c| c.unchecked).collect::<Vec<_>>(), [false, true, true]);
    assert_eq!(r.ran.iter().map(|c| c.unchecked).collect::<Vec<_>>(), [false, true]);
    assert_eq!((r.found.len(), r.ran.len()), (3, 2), "none is dropped");
    // what the Log says is what the main lane will read
    let made = f.rig.of_kind("report.made");
    assert_eq!(made.len(), 1);
    assert_eq!((made[0].task.as_deref(), made[0].thread.as_deref()), (Some(branch.as_str()), Some("main")));
    assert_eq!(made[0].data["report"]["found"][1]["unchecked"], true);
    assert!(r.text(&branch).contains("src/net.rs:99 (unchecked)") && r.text(&branch).contains("ran cargo build → 0 (unchecked)"), "{}", r.text(&branch));
    assert!(!r.text(&branch).contains("src/net.rs:3 (unchecked)"));
    f.rig.tasks.ctl(&main, Ctl::Abort, Actor::Operator).unwrap();
}

#[test]
fn an_answer_that_is_not_a_report_is_one_that_says_so_and_a_command_of_another_branch_does_not_count() {
    let (boss, src) = parts();
    let a = Scripted::new("a", vec![vec![Beat::Shell("make".into(), "".into(), 0), end("just some words")]]);
    let b = Scripted::new("b", vec![vec![end(&report(json!({ "ran": [{ "cmd": "make", "exit": 0 }] })))]]);
    let f = fx(vec![boss, a, b], |c| c.root = src.path().to_path_buf());
    let main = spawn(&f, "boss", "work", None);
    let (one, two) = (spawn(&f, "a", "scout", Some(&main)), spawn(&f, "b", "scout", Some(&main)));
    f.rig.finished(&one);
    f.rig.finished(&two);
    let plain = f.front.report(&one).unwrap();
    assert_eq!((plain.answer.as_str(), plain.sure.as_str(), plain.open.len()), ("just some words", "low", 1));
    assert!(plain.open[0].contains("not a report"));
    // `b` says it ran `make`, and only `a` did
    assert!(f.front.report(&two).unwrap().ran[0].unchecked);
    f.rig.tasks.ctl(&main, Ctl::Abort, Actor::Operator).unwrap();
}


#[test]
fn a_report_reaches_the_main_lane_once_in_the_brief_and_is_not_carried_again() {
    let caps = Caps { brief: true, resume: true, ..Caps::default() };
    let turns = (0..4).map(|n| vec![Beat::Session("s".into()), end(&format!("turn {n}"))]).collect();
    let claude = Scripted::with_caps("claude", caps, turns);
    let (boss, src) = parts();
    let scout = Scripted::new("scout", vec![vec![end(&report(json!({ "answer": "found it in fetch()", "found": [{ "claim": "retry", "at": "src/net.rs:3" }] })))]]);
    let f = fx(vec![claude, boss, scout], |c| {
        c.doer = Some("claude".into());
        c.root = src.path().to_path_buf();
    });
    f.turn("hello");
    let main = spawn(&f, "boss", "work", None);
    let branch = spawn(&f, "scout", "scout", Some(&main));
    f.rig.finished(&branch);
    f.front.report(&branch).unwrap();
    assert_eq!(f.lanes.waiting("main").len(), 1);
    // a line goes to the task that works, so the stand-in for the lane's other work is stopped first
    f.rig.tasks.ctl(&main, Ctl::Abort, Actor::Operator).unwrap();
    f.rig.finished(&main);
    f.turn("go on");
    f.turn("and then?");
    let briefs: Vec<Value> = f.rig.of_kind("brief.rendered").into_iter().map(|e| e.data).collect();
    let has = |i: usize| briefs[i]["sections"].as_array().unwrap().iter().any(|s| s == "branches");
    assert!((!has(0), has(1), !has(2)) == (true, true, true), "{briefs:?}");
    let told: Vec<String> = f.tasks().into_iter().filter(|t| t.spec.doer == "claude").filter_map(|t| t.spec.input["brief"].as_str().map(String::from)).collect();
    assert!(told[1].contains("## branches") && told[1].contains("found it in fetch()") && told[1].contains("sure mid"), "{}", told[1]);
    assert!(!told[0].contains("branches") && !told[2].contains("branches") && !told[2].contains("found it"), "once, and not on the next turn");
    assert_eq!(f.rig.of_kind("report.sent").len(), 1);
    assert!(f.lanes.waiting("main").is_empty());
}

#[test]
fn the_planning_turn_is_not_told_the_reports_and_they_wait_for_the_lane() {
    let caps = Caps { brief: true, resume: true, ..Caps::default() };
    let turns = (0..3).map(|n| vec![Beat::Session("s".into()), end(&format!("turn {n}"))]).collect();
    let (boss, src) = parts();
    let scout = Scripted::new("scout", vec![vec![end(&report(json!({})))]]);
    let f = fx(vec![Scripted::with_caps("claude", caps, turns), boss, scout], |c| {
        c.doer = Some("claude".into());
        c.root = src.path().to_path_buf();
    });
    f.turn("hello");
    let main = spawn(&f, "boss", "work", None);
    let branch = spawn(&f, "scout", "scout", Some(&main));
    f.rig.finished(&branch);
    f.front.report(&branch).unwrap();
    f.rig.tasks.ctl(&main, Ctl::Abort, Actor::Operator).unwrap();
    f.rig.finished(&main);
    let planning = match f.say(":plan move the log") {
        j4n_core::front::Said::Started { task } => f.rig.finished(&task),
        other => panic!("{other:?}"),
    };
    assert_eq!(planning.spec.role.as_deref(), Some("plan"));
    assert!(!planning.spec.input["brief"].as_str().unwrap().contains("branches"));
    assert_eq!(f.lanes.waiting("main").len(), 1);
}

#[test]
fn with_no_report_waiting_the_brief_is_what_it_was() {
    let caps = Caps { brief: true, resume: true, ..Caps::default() };
    let f = fx(vec![Scripted::with_caps("claude", caps, vec![vec![end("hi")]])], |c| c.doer = Some("claude".into()));
    f.turn("hello");
    let briefs = f.rig.of_kind("brief.rendered");
    assert_eq!(briefs[0].data["sections"], json!(["contract"]));
    assert!(f.rig.of_kind("report.sent").is_empty() && f.rig.of_kind("report.made").is_empty());
}

#[test]
fn a_report_waits_in_the_fold_until_an_event_says_it_was_told() {
    let (_, src) = parts();
    let lanes = Lanes::new();
    let mut fold = lanes.clone();
    fold.apply(&ev(1, "report.made", Some("t2"), json!({ "report": Report::plain("x", "y").json(), "deliver": true })));
    assert_eq!(lanes.waiting("main").len(), 1, "{}", src.path().display());
    fold.apply(&ev(2, "report.sent", None, json!({ "tasks": ["t2"] })));
    assert!(lanes.waiting("main").is_empty());
}


fn ev(seq: u64, kind: &str, task: Option<&str>, data: Value) -> Ev {
    Ev { seq, at_ms: seq * 1000, kind: kind.into(), task: task.map(String::from), thread: Some("main".into()), by: Actor::Core, cause: None, class: Default::default(), data }
}

fn queued(seq: u64, task: &str, parent: Option<&str>, role: &str) -> Ev {
    ev(seq, "task.queued", Some(task), json!({ "spec": { "goal": format!("{task} goal"), "doer": "d", "parent": parent, "thread": "main", "role": role, "tune": { "model": "m", "endpoint": "h" } } }))
}

fn spent(seq: u64, task: &str, usd: f64, i: u64, o: u64) -> Ev {
    ev(seq, "run.metrics", Some(task), json!({ "usd": usd, "tokens_in": i, "tokens_out": o }))
}

/// A line t1 with a flow t2 under it, the flow with two members t3 and t4, and t5 beside the flow; a second line t6 with t7.
fn tree() -> Lanes {
    let mut lanes = Lanes::new();
    let evs = vec![
        queued(1, "t1", None, "work"),
        queued(2, "t2", Some("t1"), "flow"),
        queued(3, "t3", Some("t2"), "scout"),
        queued(4, "t4", Some("t2"), "review"),
        queued(5, "t5", Some("t1"), "scout"),
        spent(6, "t1", 0.5, 1000, 100),
        spent(7, "t3", 0.01, 10, 5),
        spent(8, "t3", 0.02, 20, 7),
        spent(9, "t4", 0.04, 100, 9),
        spent(10, "t5", 0.1, 7, 3),
        ev(11, "run.served", Some("t3"), json!({ "model": "served", "host": "gw", "cached": 40 })),
        ev(12, "run.served", Some("t3"), json!({ "cached": 2 })),
        ev(13, "run.served", Some("t5"), json!({ "cached": 5 })),
        queued(14, "t6", None, "work"),
        queued(15, "t7", Some("t6"), "chore"),
        spent(16, "t7", 0.003, 1, 1),
        ev(17, "task.started", Some("t3"), json!({})),
        ev(18, "task.settled", Some("t3"), json!({ "ok": true })),
        ev(19, "task.settled", Some("t4"), json!({ "ok": true })),
        ev(20, "task.settled", Some("t5"), json!({ "ok": false })),
        ev(21, "task.stopped", Some("t7"), json!({ "why": { "kind": "wall" } })),
        ev(22, "task.settled", Some("t2"), json!({ "ok": true })),
        ev(23, "task.started", Some("t1"), json!({})),
        ev(24, "task.settled", Some("t1"), json!({ "ok": true })),
    ];
    evs.iter().for_each(|e| lanes.apply(e));
    lanes
}

#[test]
fn the_totals_of_a_line_and_a_thread_are_the_sum_of_their_branches_rows() {
    let lanes = tree();
    let lane = lanes.lane("main");
    assert_eq!(lane.lines.len(), 2);
    let (one, two) = (&lane.lines[0], &lane.lines[1]);
    assert_eq!((one.root.id.as_str(), two.root.id.as_str()), ("t1", "t6"));
    assert_eq!(one.rows.iter().map(|r| (r.id.as_str(), r.depth)).collect::<Vec<_>>(), [("t2", 1), ("t3", 2), ("t4", 2), ("t5", 1)]);
    // the line's total is the sum of its branches, and the line's own cost is not one of them
    let sum = |f: fn(&j4n_core::lanes::Row) -> i64| one.rows.iter().map(f).sum::<i64>();
    assert_eq!(one.total.micros, sum(|r| r.micros));
    assert_eq!(one.total.micros, 170_000);
    assert_eq!((one.total.branches, one.total.tokens_in, one.total.tokens_out, one.total.cached), (4, 137, 24, 47));
    assert_eq!(one.root.micros, 500_000, "the main lane's own row is its own");
    assert_eq!(one.root.wall_ms, 1000, "and so is its time");
    assert_eq!(one.total.wall_ms, one.rows.iter().map(|r| r.wall_ms).sum::<u64>());
    assert_eq!(one.total.wall_ms, 20_000 + 1000 + 15_000 + 15_000, "a branch's time runs from its start, else from when it was queued");
    // the thread is the sum of its lines
    assert_eq!(lane.total.micros, one.total.micros + two.total.micros);
    assert_eq!((lane.total.micros, lane.total.branches, lane.total.tokens_in, lane.total.tokens_out), (173_000, 5, 138, 25));
    // a flow's own row has the total of its children 
    let flow = &one.rows[0];
    assert_eq!((flow.micros, flow.sub.micros, flow.sub.branches), (0, 70_000, 3));
    assert_eq!(flow.sub.micros, one.rows[1].micros + one.rows[2].micros);
    // and the fold says the same to an Expr
    let read = lanes.read(&[], 0);
    assert_eq!(read.at(&[j4n_core::value::Key::Name("main".into()), j4n_core::value::Key::Name("total".into()), j4n_core::value::Key::Name("usd".into())]), j4n_core::value::Value::Usd(173_000));
}

#[test]
fn what_came_of_a_branch_is_one_of_five_words_and_a_model_a_host_and_a_class() {
    let mut lanes = tree();
    let rows = |l: &Lanes| l.lane("main").lines.iter().flat_map(|x| x.rows.iter().map(|r| (r.id.clone(), r.outcome))).collect::<std::collections::BTreeMap<_, _>>();
    let got = rows(&lanes);
    assert_eq!((got["t3"], got["t4"], got["t5"], got["t7"]), ("ignored", "ignored", "blocked", "killed"));
    // a flow that went on with a member's answer used it; work laid into the folder is merged; a report that was read is used
    for e in [
        ev(30, "flow.step.ended", Some("t2"), json!({ "task": "t3", "ok": true })),
        ev(31, "task.applied", Some("t4"), json!({ "work": {} })),
        ev(32, "report.made", Some("t5"), json!({ "report": Report::plain("x", "y").json(), "deliver": false })),
    ] {
        lanes.apply(&e);
    }
    let got = rows(&lanes);
    assert_eq!((got["t3"], got["t4"], got["t5"]), ("used", "merged", "blocked"), "a failed branch is blocked whatever was said of it");
    let t3 = &lanes.lane("main").lines[0].rows[1];
    assert_eq!((t3.class.as_str(), t3.doer.as_str(), t3.model.as_str(), t3.host.as_str(), t3.wall_ms), ("scout", "d", "served", "gw", 1000));
    // work that was laid in and read is merged, not only used
    lanes.apply(&ev(33, "report.made", Some("t4"), json!({ "report": Report::plain("x", "y").json(), "deliver": false })));
    assert_eq!(rows(&lanes)["t4"], "merged");
    // not over yet: no outcome
    lanes.apply(&queued(40, "t9", Some("t1"), "scout"));
    assert_eq!(rows(&lanes)["t9"], "");
    // a report that was told is used; a branch that failed or waited for an answer nobody gave is blocked, any other stop is killed
    lanes.apply(&ev(41, "task.settled", Some("t9"), json!({ "ok": true })));
    lanes.apply(&ev(42, "report.made", Some("t9"), json!({ "report": Report::plain("x", "y").json(), "deliver": true })));
    assert_eq!(rows(&lanes)["t9"], "ignored", "waiting is not yet read");
    lanes.apply(&ev(43, "report.sent", None, json!({ "tasks": ["t9"] })));
    assert_eq!(rows(&lanes)["t9"], "used");
    for (n, why, outcome) in [(50, "failed", "blocked"), (51, "deadline", "blocked"), (52, "parked", "blocked"), (53, "aborted", "killed"), (54, "hardcap", "killed")] {
        let id = format!("t{n}");
        lanes.apply(&queued(n, &id, Some("t1"), "scout"));
        lanes.apply(&ev(n + 100, "task.stopped", Some(&id), json!({ "why": { "kind": why } })));
        assert_eq!(rows(&lanes)[&id], outcome, "{why}");
    }
}

#[test]
fn what_a_branch_took_is_the_plan_cost_when_there_is_one_and_at_most_sixteen_reports_wait() {
    let mut lanes = Lanes::new();
    for e in [queued(1, "t1", None, "work"), queued(2, "t2", Some("t1"), "scout"), ev(3, "run.metrics", Some("t2"), json!({ "usd": 0.5, "plan_usd": 0.1, "tokens_in": 4, "tokens_out": 2 }))] {
        lanes.apply(&e);
    }
    assert_eq!(lanes.lane("main").total.micros, 100_000, "what it took of the plan, as a task's row has it");
    for n in 0..20 {
        lanes.apply(&ev(10 + n, "report.made", Some("t2"), json!({ "report": Report::plain(&format!("r{n}"), "y").json(), "deliver": true })));
    }
    let waiting = lanes.waiting("main");
    assert_eq!((waiting.len(), waiting[0].1.answer.as_str()), (16, "r4"), "the oldest are the ones left out");
    assert!(lanes.notes()[0].contains("left out"));
}

#[test]
fn the_cached_tokens_of_a_branch_come_from_either_row_that_says_them() {
    let mut lanes = Lanes::new();
    for e in [
        queued(1, "t1", None, "work"),
        queued(2, "t2", Some("t1"), "scout"),
        ev(3, "run.served", Some("t2"), json!({ "model": "m", "cached": 40 })),
        ev(4, "run.metrics", Some("t2"), json!({ "usd": 0.5, "tokens_in": 150, "tokens_out": 2, "cached": 100, "wrote": 10 })),
        ev(5, "run.metrics", Some("t2"), json!({ "usd": 0.5, "tokens_in": 4, "tokens_out": 2 })),
    ] {
        lanes.apply(&e);
    }
    assert_eq!(lanes.lane("main").total.cached, 140, "the cached of an old row is read as it was, a missing key is 0");
}

#[test]
fn an_ask_is_not_a_branch_and_a_fold_restarted_from_its_snapshot_says_the_same() {
    let mut lanes = tree();
    lanes.apply(&ev(50, "task.queued", Some("t10"), json!({ "spec": { "goal": "q", "doer": "human", "parent": "t1", "thread": "main" } })));
    assert_eq!(lanes.lane("main").lines[0].total.branches, 4);
    let mut again = Lanes::new();
    again.load(lanes.save()).unwrap();
    assert_eq!(again.lane("main"), lanes.lane("main"));
}

#[test]
fn the_real_loop_gives_the_same_rows_a_branch_that_is_aborted_is_killed_and_the_pipe_tree_draws_them() {
    let (boss, _) = parts();
    let work = Scripted::new("worker", vec![vec![Beat::Spend(0.25), end("done")]]);
    let slow = Scripted::new("slow", vec![vec![Beat::Spend(0.5), Beat::Forever]]);
    let f = fx(vec![boss, work, slow], |_| {});
    let main = spawn(&f, "boss", "work", None);
    let (a, b) = (spawn(&f, "worker", "scout", Some(&main)), spawn(&f, "slow", "review", Some(&main)));
    f.rig.finished(&a);
    f.rig.until(&b, "spent", |r| r.spent > 0);
    f.rig.tasks.ctl(&b, Ctl::Abort, Actor::Operator).unwrap();
    f.rig.finished(&b);
    let lane = f.lanes.lane("main");
    assert_eq!(lane.lines.len(), 1);
    let line = &lane.lines[0];
    assert_eq!(line.rows.iter().map(|r| (r.id.as_str(), r.outcome)).collect::<Vec<_>>(), [(a.as_str(), "ignored"), (b.as_str(), "killed")]);
    assert_eq!((line.total.micros, line.total.tokens_in, line.total.tokens_out), (750_000, 20, 10));
    let rows = f.front.lane_rows("main");
    let cells: Vec<Vec<String>> = rows.iter().map(|r| r.cells.iter().map(|c| c.text.clone()).collect()).collect();
    assert_eq!(cells[0][..2], ["lanes", "2 branches"]);
    assert!(cells[0][4].starts_with("$0.7500"), "{cells:?}");
    assert_eq!(cells[1][0], main);
    assert_eq!((cells[2][0].as_str(), cells[3][0].as_str()), (format!("├─ {a}").as_str(), format!("└─ {b}").as_str()));
    assert_eq!((cells[2][1].as_str(), cells[3][1].as_str()), ("ignored", "killed"));
    assert!(cells[2][2].contains("scout") && cells[2][2].contains("worker"));
    assert!(f.front.lane_rows("elsewhere").is_empty());
    f.rig.tasks.ctl(&main, Ctl::Abort, Actor::Operator).unwrap();
}

// ---- a doer that takes no Brief is told the reports in the text of its turn ----

/// A main lane that takes no Brief (the claude CLI's caps) and a scout branch whose report waits for it; the lane's turns are "hello", "go on"
/// and "and then?".
fn no_brief_lane() -> (Fx, std::sync::Arc<Scripted>) {
    no_brief_lane_saying("found it in fetch()")
}

fn no_brief_lane_saying(answer: &str) -> (Fx, std::sync::Arc<Scripted>) {
    let caps = Caps { brief: false, resume: true, ..Caps::default() };
    let turns = (0..4).map(|n| vec![Beat::Session("s".into()), end(&format!("turn {n}"))]).collect();
    let claude = Scripted::with_caps("claude", caps, turns);
    let (boss, src) = parts();
    let scout = Scripted::new("scout", vec![vec![end(&report(json!({ "answer": answer, "found": [{ "claim": "retry", "at": "src/net.rs:3" }] })))]]);
    let f = fx(vec![claude.clone(), boss, scout], |c| {
        c.doer = Some("claude".into());
        c.root = src.path().to_path_buf();
    });
    f.turn("hello");
    let main = spawn(&f, "boss", "work", None);
    let branch = spawn(&f, "scout", "scout", Some(&main));
    f.rig.finished(&branch);
    f.front.report(&branch).unwrap();
    assert_eq!(f.lanes.waiting("main").len(), 1);
    f.rig.tasks.ctl(&main, Ctl::Abort, Actor::Operator).unwrap();
    f.rig.finished(&main);
    (f, claude)
}

#[test]
fn the_next_turn_of_a_doer_with_no_brief_carries_the_waiting_report_in_its_text_once_and_it_is_marked_sent() {
    let (f, claude) = no_brief_lane();
    f.turn("go on");
    f.turn("and then?");
    let starts = claude.starts();
    let texts: Vec<Option<String>> = starts.iter().map(|s| s.text.clone()).collect();
    assert_eq!(texts[0], None, "the first turn had nothing waiting");
    let told = texts[1].as_deref().unwrap_or_else(|| panic!("the turn after the report was given no words but its goal: {starts:?}"));
    assert!(told.starts_with("go on\n\n## branches\n") && told.contains("found it in fetch()") && told.contains("sure mid"), "{told}");
    assert_eq!(texts[2], None, "told once, and not on the turn after");
    assert_eq!(f.rig.of_kind("report.sent").len(), 1);
    assert!(f.lanes.waiting("main").is_empty());
    // the goal of the task is still what the operator said, and no Brief was made for a doer that takes none
    assert_eq!(starts[1].goal, "go on");
    assert!(f.rig.of_kind("brief.rendered").is_empty());
}

/// The pack `shape`'s desk as the front sees it: what the operator kept under a line is one text.
struct Notes(&'static str);

impl j4n_core::front::shape::Desk for Notes {
    fn on(&self) -> bool {
        true
    }
    fn drafted(&self, _: &j4n_core::front::shape::Draft, _: Option<&str>) {}
    fn command(&self, _: &str, _: &[String]) -> Result<String, String> {
        Ok(String::new())
    }
    fn keep(&self, _: &str, _: &str, keys: &[String]) -> Result<String, String> {
        if keys.is_empty() { Err("nothing kept".into()) } else { Ok(self.0.into()) }
    }
    fn card(&self, _: &str) -> Vec<j4n_proto::Row> {
        Vec::new()
    }
    fn skips(&self, _: &str) -> bool {
        false
    }
}

#[test]
fn the_notes_kept_under_a_line_reach_a_doer_with_no_brief_in_the_text_of_its_turn_and_only_that_turn() {
    let caps = Caps { brief: false, resume: true, ..Caps::default() };
    let claude = Scripted::with_caps("claude", caps, (0..2).map(|n| vec![Beat::Session("s".into()), end(&format!("turn {n}"))]).collect());
    let mut f = fx(vec![claude.clone()], |c| c.doer = Some("claude".into()));
    f.front = f.front.with_desk(std::sync::Arc::new(Notes("- the empty input is an error, not a panic\n- keep the public names")));
    assert!(f.front.desk().is_some(), "the desk is on the front");
    let said = f.front.say_with("main", "fix the parser", &["k1".into()]);
    let j4n_core::front::Said::Started { task } = said else { panic!("{said:?}") };
    f.rig.finished(&task);
    f.turn("and the lexer");
    let starts = claude.starts();
    let told = starts[0].text.as_deref().unwrap_or_else(|| panic!("the notes were not in the words of the turn: {starts:?}"));
    assert!(told.starts_with("fix the parser\n\n## shape\n- the empty input is an error"), "{told}");
    assert!(told.contains("keep the public names"), "{told}");
    assert_eq!(starts[0].goal, "fix the parser", "the goal of the task is still what was typed");
    assert_eq!(starts[1].text, None, "a line with nothing kept is sent as it is");
    assert!(f.rig.of_kind("brief.rendered").is_empty(), "no Brief was made for a doer that takes none");
}

#[test]
fn a_planning_turn_of_a_doer_with_no_brief_is_not_told_the_reports_and_they_wait() {
    let (f, claude) = no_brief_lane();
    let planning = match f.say(":plan move the log") {
        j4n_core::front::Said::Started { task } => f.rig.finished(&task),
        other => panic!("{other:?}"),
    };
    assert_eq!(planning.spec.role.as_deref(), Some("plan"));
    assert!(claude.starts().iter().all(|s| s.text.as_deref().is_none_or(|t| !t.contains("branches"))), "{:?}", claude.starts());
    assert_eq!(f.lanes.waiting("main").len(), 1);
    assert!(f.rig.of_kind("report.sent").is_empty());
}

#[test]
fn a_secret_in_a_report_does_not_reach_the_doers_text() {
    let key = format!("sk-{}", "a".repeat(30));
    let (f, claude) = no_brief_lane_saying(&format!("found it in fetch(), the key is {key}"));
    f.turn("go on");
    let told = claude.starts()[1].text.clone().unwrap_or_else(|| panic!("no text: {:?}", claude.starts()));
    assert!(told.contains("found it in fetch()") && !told.contains(&key), "{told}");
}

