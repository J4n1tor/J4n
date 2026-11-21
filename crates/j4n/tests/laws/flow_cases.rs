//! Through the binary's own wiring: `/plan` and `/build`, the flows of the pack `code`, run by the own agent against a
//! gateway on the loopback: `/plan` writes nothing and the binding is untouched, the whole of plan → work in a worktree →
//! check → apply on a yes, and the contract a line gets.

use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::front::Said;
use j4n_core::task::{Row, State};
use crate::models_cases::mock::{Mock, Say, Seen};
use serde_json::{Value, json};
use std::fs;
use std::process::Command;
use std::time::{Duration, Instant};

fn open(d: &Dirs, gateway: &Mock) -> World {
    let key = d.dir.join("key");
    crate::support::key_file(&key, "sk-test-own-0001\n");
    fs::create_dir_all(d.j4n_home().join("connectors")).unwrap();
    let mine = format!("api = 1\n[[connector]]\nid = \"own\"\nurl = \"{}/v1\"\nkey = \"file:{}\"\nmodels = [\"vendor/m\"]\n", gateway.url(), key.display());
    fs::write(d.j4n_home().join("connectors/mine.toml"), mine).unwrap();
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    let opts = Opts { doer: Some("own".into()), model: None, approval: Some(Approval::Ask), wall: None, usd: None, mode: j4n_core::task::Mode::Write, place: None, shape: false, anywhere: false };
    wire::open(setup, &opts, true, "main").unwrap_or_else(|e| panic!("opens: {e}"))
}

fn git(d: &Dirs, args: &[&str]) {
    let out = Command::new("git").args(args).current_dir(d.project()).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn repo(d: &Dirs) {
    for a in [&["init", "-q", "-b", "main"][..], &["config", "user.email", "t@t"], &["config", "user.name", "t"]] {
        git(d, a);
    }
    d.put("README.md", "# project\n");
    git(d, &["add", "."]);
    git(d, &["commit", "-qm", "first"]);
}

fn frames(text: &str, calls: &[(&str, &str, &str)], finish: &str) -> Say {
    let mut out = Vec::new();
    if !text.is_empty() {
        out.push(json!({ "choices": [{ "delta": { "content": text } }] }).to_string());
    }
    if !calls.is_empty() {
        let frags: Vec<Value> = calls.iter().enumerate().map(|(i, (id, name, args))| json!({ "index": i, "id": id, "function": { "name": name, "arguments": args } })).collect();
        out.push(json!({ "choices": [{ "delta": { "tool_calls": frags } }] }).to_string());
    }
    out.push(json!({ "choices": [{ "delta": {}, "finish_reason": finish }] }).to_string());
    out.push(json!({ "choices": [], "provider": "Cerebras", "usage": { "prompt_tokens": 100, "completion_tokens": 10 } }).to_string());
    Say::Sse(out)
}

fn answer(text: &str) -> Say {
    frames(text, &[], "stop")
}

fn call(calls: &[(&str, &str, &str)]) -> Say {
    frames("", calls, "tool_calls")
}

fn wait(w: &World, task: &str) -> Row {
    w.front.tasks().book().wait(task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap_or_else(|| panic!("{task} never ended\n{}", dump(w)))
}

fn dump(w: &World) -> String {
    w.front.log().read(0).map(|e| format!("{:>4} {:<18} {} {}", e.seq, e.kind, e.task.as_deref().unwrap_or("-"), e.data)).collect::<Vec<_>>().join("\n")
}

fn kinds(w: &World, kind: &str) -> Vec<Value> {
    w.front.log().read(0).filter(|e| e.kind == kind).map(|e| e.data).collect()
}

fn started(w: &World, line: &str) -> String {
    match w.front.say("main", line) {
        Said::Started { task } => task,
        other => panic!("{line:?} did not start a task: {other:?}\n{}", dump(w)),
    }
}

/// Answer the next question that is open, as the operator would from a button.
fn reply(w: &World, with: j4n_proto::Reply) -> String {
    let end = Instant::now() + crate::support::PATIENCE;
    loop {
        if let Some(a) = w.front.tasks().book().open_asks().into_iter().next() {
            assert!(matches!(w.front.reply("main", &a.ask, with), Said::Answered { .. }), "{}", dump(w));
            return a.question;
        }
        assert!(Instant::now() < end, "no question was asked\n{}", dump(w));
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn system(s: &Seen) -> String {
    s.json()["messages"][0]["content"].as_str().unwrap_or_default().to_string()
}

#[test]
fn a_plan_in_a_bound_thread_writes_nothing_and_leaves_the_binding_as_it_was() {
    let d = Dirs::new("flow-plan-bound-thread");
    repo(&d);
    let gateway = Mock::start(|body, n| match n {
        0 => answer("Hello."),
        // the planner tries to write, as a model that was not told it only reads might
        1 => call(&[("c1", "write", "{\"path\":\"plan-notes.txt\",\"content\":\"I changed things\"}")]),
        2 => answer("1. add the flag in cli.rs\n2. test it"),
        _ => answer(&format!("unexpected request {} of {}", n, body.path)),
    });
    let w = open(&d, &gateway);
    let first = started(&w, "hello there");
    assert!(matches!(wait(&w, &first).state, State::Done { ok: true }));
    let bound = w.front.binding("main").expect("bound by the first line");
    let before = kinds(&w, "thread.bound").len();
    let plan = started(&w, "/plan add a --verbose flag");
    let row = wait(&w, &plan);
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}\n{}", row.state, dump(&w));
    assert_eq!(row.answer.as_deref(), Some("1. add the flag in cli.rs\n2. test it"));
    assert!(!d.project().join("plan-notes.txt").exists(), "a plan writes nothing");
    let blocked = gateway.seen()[2].json()["messages"].as_array().unwrap().iter().filter(|m| m["role"] == "tool").map(|m| m["content"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert!(blocked[0].contains("was not allowed"), "the Gate said no to the write, and the tool stayed on offer: {blocked:?}");
    assert!(gateway.seen()[1].json()["tools"].as_array().unwrap().len() == 9, "a read-only step keeps its tools");
    assert!(events_have(&w, "gate.verdict", |v| v["verb"] == "write" && v["verdict"] == "block"), "{}", dump(&w));
    // the flow's steps are tasks of its own, under the flow's; the thread is bound as it was
    let after = w.front.binding("main").unwrap();
    assert_eq!((&after.doer, &after.model, &after.endpoint, &after.how, &after.answer), (&bound.doer, &bound.model, &bound.endpoint, &bound.how, &bound.answer));
    assert_eq!(after.tasks, bound.tasks, "the flow and its steps are not tasks of the thread's own lines");
    assert_eq!(kinds(&w, "thread.bound").len(), before);
    let step = w.front.tasks().book().children(&plan).into_iter().find(|k| k.spec.doer == "own").expect("the step's task");
    assert_eq!((step.spec.role.as_deref(), step.spec.tune.mode), (Some("plan"), j4n_core::task::Mode::Read));
    assert!(system(&gateway.seen()[1]).contains("This task only reads"), "the Brief says so too");
    // and the thread goes on in its own session afterwards
    let next = started(&w, "thanks, now what is in the README?");
    let again = w.front.tasks().book().get(&next).unwrap();
    assert_eq!(again.spec.follows.as_deref(), Some(first.as_str()), "the bound doer's session carries on past the flow");
    w.close();
}

fn events_have(w: &World, kind: &str, f: impl Fn(&Value) -> bool) -> bool {
    kinds(w, kind).iter().any(f)
}

#[test]
fn a_plan_with_nothing_after_it_or_a_flow_that_is_not_there_is_refused_in_words() {
    let d = Dirs::new("plan_refused");
    let gateway = Mock::start(|_, _| answer("x"));
    let w = open(&d, &gateway);
    assert!(matches!(w.front.say("main", "/plan"), Said::Refused(m) if m.contains("say what it is for")));
    assert!(gateway.seen().is_empty());
    w.close();
}

#[test]
fn a_build_plans_works_in_a_worktree_checks_and_brings_the_work_over_on_a_yes() {
    let d = Dirs::new("build");
    repo(&d);
    let gateway = Mock::start(|_, n| match n {
        0 => answer("1. write feature.txt"),
        1 => call(&[("c1", "write", "{\"path\":\"feature.txt\",\"content\":\"the feature\"}")]),
        2 => answer("wrote feature.txt"),
        _ => answer("unexpected"),
    });
    let w = open(&d, &gateway);
    let flow = started(&w, "/build add the feature");
    // nothing is built without a yes
    let q = reply(&w, j4n_proto::Reply::Yes);
    assert!(q.contains("Build this plan?") && q.contains("1. write feature.txt"), "{q}");
    // the work is done in a worktree, and the project's folder does not have it yet
    let q = reply(&w, j4n_proto::Reply::Yes);
    assert!(q.contains("Bring this work into your folder?") && q.contains("1 file"), "{q}");
    assert!(!d.project().join("feature.txt").exists(), "not in the folder before the yes");
    let row = wait(&w, &flow);
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}\n{}", row.state, dump(&w));
    assert_eq!(fs::read_to_string(d.project().join("feature.txt")).unwrap(), "the feature");
    assert_eq!(row.answer.as_deref(), Some("wrote feature.txt"));
    let steps: Vec<String> = kinds(&w, "flow.step.started").iter().map(|s| s["step"].as_str().unwrap().to_string()).collect();
    assert_eq!(steps, ["plan", "go", "work", "check", "validate", "again", "apply", "done"], "{}", dump(&w));
    // the worker was the only doer there is, so nobody else could validate: that step was left out, and said so
    assert!(kinds(&w, "flow.step.skipped").iter().any(|s| s["step"] == "validate"), "{}", dump(&w));
    // each step was a task of its own: the plan read, the work wrote in a worktree, the check ran where the work was done
    let kids = w.front.tasks().book().children(&flow);
    let own: Vec<_> = kids.iter().filter(|k| k.spec.doer != "human").collect();
    assert_eq!(own.iter().map(|k| (k.spec.doer.as_str(), k.spec.place.as_str())).collect::<Vec<_>>().len(), 3);
    assert_eq!(own[1].spec.place, "worktree");
    assert!(own[2].spec.place.starts_with('@') && own[2].spec.doer == "check", "{:?}", own[2].spec);
    // the apply is the loop's, so `rewind` takes it out again
    assert!(w.front.tasks().book().get(&own[1].id).unwrap().applied.is_some());
    w.close();
}

#[test]
fn a_no_to_the_plan_ends_the_build_before_anything_is_built() {
    let d = Dirs::new("build_no");
    repo(&d);
    let gateway = Mock::start(|_, n| if n == 0 { answer("a plan") } else { answer("unexpected") });
    let w = open(&d, &gateway);
    let flow = started(&w, "/build something");
    reply(&w, j4n_proto::Reply::No);
    let row = wait(&w, &flow);
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}\n{}", row.state, dump(&w));
    assert!(row.answer.as_deref().unwrap().contains("not brought into your folder"));
    assert_eq!(gateway.seen().len(), 1, "only the plan was asked for");
    w.close();
}

#[test]
fn a_short_line_in_a_thread_that_is_going_gets_the_contract_of_work_and_only_the_first_line_may_be_small_talk() {
    let d = Dirs::new("flow-short-line-thread");
    let gateway = Mock::start(|_, _| answer("ok"));
    let w = open(&d, &gateway);
    let mut tasks = Vec::new();
    for line in ["hi", "continue", "yes", "2", "go on", "do it"] {
        let t = started(&w, line);
        assert!(matches!(wait(&w, &t).state, State::Done { ok: true }));
        tasks.push(t);
    }
    let seen = gateway.seen();
    assert!(system(&seen[0]).contains("short conversational line"), "the first line of a thread may be small talk: {}", system(&seen[0]));
    for (i, line) in ["continue", "yes", "2", "go on", "do it"].iter().enumerate() {
        let s = system(&seen[i + 1]);
        assert!(s.contains("Do what the task says") && !s.contains("short conversational line"), "{line}: {s}");
    }
    // what was rendered is an event, with its numbers
    let rendered = kinds(&w, "brief.rendered");
    assert_eq!(rendered.len(), 6);
    assert_eq!(rendered[0]["in_session"], false);
    assert!(rendered[1..].iter().all(|r| r["in_session"] == true && r["sections"] == json!(["contract"])));
    w.close();
}
