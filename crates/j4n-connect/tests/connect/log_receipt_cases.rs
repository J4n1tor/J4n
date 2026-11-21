//! The Log is the receipt a plan item's check is read from, so no doer writes under `.j4n/`. The first test
//! is the case: a scripted model on the real tools, loop and Gate, and the real Log file in its own folder, trying to forge it. The others
//! ask the Gate the way the claude CLI, pi and acp ask it.

use crate::bench::{Bench, base_layer};
use crate::mock::{Mock, Say};
use crate::world::Scratch;
use j4n_connect::claude::frames::{Turn, effect};
use j4n_connect::model::doer::{Config, Dialect, ModelDoer};
use j4n_connect::net::Net;
use j4n_connect::tools::Builtin;
use j4n_core::change::Actor;
use j4n_core::doer::Doer;
use j4n_core::gate::check::Gates;
use j4n_core::gate::foreign::Own;
use j4n_core::gate::grant::Grants;
use j4n_core::gate::{Action, Gate, ReadOnly, Verb, Verdict};
use j4n_core::table::Tables;
use j4n_core::task::State;
use serde_json::{Value, json};
use std::sync::Arc;

fn delta(d: Value) -> String {
    json!({ "choices": [{ "delta": d }] }).to_string()
}

fn finish(r: &str) -> String {
    json!({ "choices": [{ "delta": {}, "finish_reason": r }] }).to_string()
}

fn usage() -> String {
    json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 5 } }).to_string()
}

fn calls(list: &[(&str, &str, Value)]) -> Say {
    let frags: Vec<Value> = list.iter().enumerate().map(|(i, (id, name, args))| json!({ "index": i, "id": id, "function": { "name": name, "arguments": args.to_string() } })).collect();
    Say::Sse(vec![delta(json!({ "tool_calls": frags })), finish("tool_calls"), usage()])
}

fn answer(text: &str) -> Say {
    Say::Sse(vec![delta(json!({ "content": text })), finish("stop"), usage()])
}

fn config(mock: &Mock) -> Config {
    Config {
        id: "gpt".into(),
        dialect: Dialect::Openai,
        base: format!("{}/v1", mock.url()),
        key: "k".into(),
        keyless: false,
        net: Net::new().with_env(|_| None),
        model: "m".into(),
        system: "brief".into(),
        context: None,
        effort: None,
    }
}

fn gate() -> Gate {
    let (t, problems) = Tables::load(vec![base_layer()]);
    assert!(problems.iter().all(|p| !p.to_string().contains("gate")), "{problems:?}");
    let (gates, notes) = Gates::load(&t);
    assert!(notes.is_empty(), "{notes:?}");
    Gate::new(gates, Grants::new("s1"))
}

fn rig(mock: &Mock) -> Bench {
    let t = Tables::load(vec![base_layer()]).0;
    let g = gate();
    let doer = ModelDoer::new(config(mock)).with_tools(Arc::new(Builtin::new().with_gate(g.clone())));
    Bench::gated(Scratch::new("bin"), vec![Arc::new(doer) as Arc<dyn Doer>], &t, g, |_| {})
}

#[test]
fn the_own_agent_cannot_forge_the_log_it_lives_next_to() {
    let forged = json!({ "seq": 99, "at_ms": 1, "kind": "run.shell", "by": { "doer": "gpt" }, "data": { "cmd": "cargo test", "exit": 0 } }).to_string();
    let mock = Mock::start(move |_, n| {
        if n == 0 {
            calls(&[
                ("c1", "write", json!({ "path": ".j4n/log/0001.jsonl", "content": forged })),
                ("c2", "edit", json!({ "path": ".j4n/log/0001.jsonl", "old": "task.queued", "new": "task.nothing" })),
                ("c3", "write", json!({ "path": ".j4n/log/0002.jsonl", "content": forged })),
                ("c4", "write", json!({ "path": "src/ok.txt", "content": "fine" })),
            ])
        } else {
            answer("done")
        }
    });
    let b = rig(&mock);
    let row = b.run("gpt", "make the check pass");
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}\n{}", row.state, b.dump());
    let seg = b.project.read(".j4n/log/0001.jsonl");
    assert!(seg.starts_with("{\"seq\":1,") && seg.contains("\"kind\":\"task.queued\"") && !seg.contains("\"seq\":99,") && !seg.contains("\"kind\":\"task.nothing\""), "the segment was written by the doer:\n{seg}");
    assert!(!b.project.join(".j4n/log/0002.jsonl").exists(), "a segment was made by the doer");
    assert_eq!(b.project.read("src/ok.txt"), "fine", "the project's own files are still the doer's");
    let by: Vec<String> = b.events("gate.verdict").iter().map(|e| e.data["why"].as_str().unwrap().split(':').next().unwrap().to_string()).collect();
    assert_eq!(by.iter().filter(|w| *w == "core-owns-j4n").count(), 3, "{by:?}");
}

fn says(g: &Gate, a: &Action) -> String {
    match g.check(a) {
        Verdict::Block { by, .. } => format!("block {by}"),
        v => v.word().into(),
    }
}

#[test]
fn the_claude_cli_pi_and_acp_ask_the_same_question_and_get_the_same_no() {
    let g = gate();
    let dir = Scratch::new("site");
    let turn = Turn { task: "t1".into(), dir: dir.path().to_path_buf(), read_only: ReadOnly::No, compacting: false };
    let abs = dir.join(".j4n/log/0001.jsonl").display().to_string();
    // the claude CLI: every tool that writes, by a relative and an absolute path
    for (tool, key) in [("Write", "file_path"), ("Edit", "file_path"), ("MultiEdit", "file_path"), ("NotebookEdit", "notebook_path")] {
        for path in [".j4n/log/0001.jsonl", abs.as_str(), "src/../.j4n/log/0001.jsonl"] {
            let a = effect(tool, &json!({ key: path }), &turn).expect("an effect");
            assert_eq!(says(&g, &a), "block core-owns-j4n", "{tool} {path}");
        }
    }
    assert_eq!(says(&g, &effect("Write", &json!({ "file_path": "src/lib.rs" }), &turn).unwrap()), "allow");
    // pi and acp name the path the same way (`Verb::Write`, the doer's id, `inside` found from the real path)
    for doer in ["pi", "omp", "acp", "gemini", "gpt"] {
        for path in [".j4n/log/0001.jsonl", abs.as_str()] {
            let mut a = Action::new(Verb::Write, Actor::Doer(doer.into()), path).task("t1");
            a.inside = Some(j4n_connect::effects::inside(dir.path(), path));
            assert_eq!(says(&g, &a), "block core-owns-j4n", "{doer} {path}");
        }
    }
    // the operator and the core are not held by it, and a neighbour of the name is not the folder
    for actor in [Actor::Operator, Actor::Core] {
        let mut a = Action::new(Verb::Write, actor, ".j4n/log/0001.jsonl");
        a.inside = Some(true);
        assert_eq!(says(&g, &a), "allow");
    }
    // J4n's own doer for accounts removes an account's folder, and the operator is asked every time
    let mut own = Action::new(Verb::Write, Actor::Doer("account".into()), "/home/u/.j4n/accounts/work");
    own.inside = Some(false);
    assert_eq!(says(&g, &own), "ask");
    let mut near = Action::new(Verb::Write, Actor::Doer("claude".into()), "docs/.j4nx/a.md");
    near.inside = Some(true);
    assert_eq!(says(&g, &near), "allow");
}

#[test]
fn a_shell_line_that_writes_into_the_log_is_refused_and_one_that_only_looks_is_not_a_block() {
    let g = gate();
    let line = |c: &str| says(&g, &Action::shell(Actor::Doer("claude".into()), c));
    for cmd in [
        "echo '{}' >> .j4n/log/0001.jsonl",
        "sed -i s/exit.0/exit\\ 0/ .j4n/log/0001.jsonl",
        "cp /tmp/x .j4n/log/0001.jsonl",
        "tee .j4n/log/0003.jsonl < /tmp/x",
        "cd .j4n/log && echo x > 0001.jsonl",
    ] {
        assert_eq!(line(cmd), "block core-owns-log-shell", "{cmd}");
    }
    for cmd in ["cat .j4n/log/0001.jsonl", "grep run.shell .j4n/log/0001.jsonl", "ls src"] {
        assert!(!line(cmd).starts_with("block core-owns"), "{cmd}: {}", line(cmd));
    }
    assert!(!says(&g, &Action::shell(Actor::Operator, "echo x >> .j4n/log/0001.jsonl")).starts_with("block core-owns"), "the operator is not held");
}

#[test]
fn a_chat_writes_the_worktree_it_works_in_below_j4ns_home_and_nothing_else_there() {
    let g = gate();
    let j4n = "/home/u/.j4n";
    let site = format!("{j4n}/worktrees/50ef4937db2c77c6/t8");
    let at = |path: &str, site: &str| {
        let mut a = Action::new(Verb::Write, Actor::Doer("claude".into()), path).task("t8");
        a.site = Some(site.to_string());
        a.state = Own { user: Some("/home/u".into()), j4n: Some(j4n.into()), root: Some("/home/u/p".into()) };
        a.inside = Some(false);
        says(&g, &a)
    };
    // a chat in its own worktree must not have its writes refused as J4n's own folder
    for path in [format!("{site}/mkdocs.yml"), format!("{site}/docs/index.md"), "docs/api.md".to_string()] {
        assert!(!at(&path, &site).starts_with("block core-owns-j4n"), "{path}: {}", at(&path, &site));
    }
    // a `.j4n` inside the worktree, the worktree folder itself, another task's worktree and the rest of J4n's home stay J4n's
    for path in [format!("{site}/.j4n/log/0001.jsonl"), format!("{j4n}/worktrees/50ef4937db2c77c6/t9/a.md"), format!("{j4n}/projects/k/lanes.toml"), format!("{site}/../t9/a.md"), site.clone()] {
        assert_eq!(at(&path, &site), "block core-owns-j4n", "{path}");
    }
    // a site that is not a worktree below J4n's home gives nothing
    assert_eq!(at(&format!("{j4n}/projects/k/lanes.toml"), &format!("{j4n}/projects/k")), "block core-owns-j4n");
}
