use j4n_core::change::Actor;
use j4n_core::log::{Log, New, Opts, Scope};
use j4n_core::redact::Redactor;
use j4n_shape::check::{Facts, Turn};
use j4n_shape::state::{self, STATE_TOKENS};
use serde_json::{Value, json};
use std::path::PathBuf;

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("j4n-shape-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn log(d: &std::path::Path) -> Log {
    Log::open(&d.join(".j4n"), Scope::Project, Opts::default(), Redactor::new()).unwrap().0
}

fn put(log: &Log, kind: &str, thread: &str, task: Option<&str>, data: Value) {
    let mut n = New::of(kind, Actor::Operator, data).thread(thread);
    if let Some(t) = task {
        n = n.task(t);
    }
    log.append(n).unwrap();
}

#[test]
fn the_last_three_turns_of_the_thread_with_the_answer_head_the_files_and_the_commands_that_passed() {
    let d = dir("turns");
    std::fs::write(d.join("a.txt"), "x").unwrap();
    let log = log(&d);
    for n in 1..=4 {
        put(&log, "chat.say", "main", None, json!({ "text": format!("line number {n}") }));
        put(&log, "task.settled", "main", Some(&format!("t{n}")), json!({ "ok": true, "answer": format!("answer {n} {}", "word ".repeat(100)) }));
        put(&log, "run.changed", "main", Some(&format!("t{n}")), json!({ "paths": [format!("f{n}.rs")] }));
    }
    put(&log, "chat.say", "main", None, json!({ "text": ":status" }));
    put(&log, "chat.say", "other", None, json!({ "text": "a line of another thread" }));
    put(&log, "run.shell", "main", Some("t4"), json!({ "cmd": "make test", "exit": 0 }));
    put(&log, "run.shell", "main", Some("t4"), json!({ "cmd": "make broken", "exit": 2 }));
    let f = state::facts(&log, "main", "fix it", &d, &["cargo test".to_string()]);
    assert_eq!(f.turns.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["t2", "t3", "t4"], "the last three; a command is no turn");
    assert_eq!(f.turns[2].you, "line number 4");
    assert!(f.turns[2].answer_head.starts_with("answer 4 word") && f.turns[2].answer_head.chars().count() <= 300, "{}", f.turns[2].answer_head);
    assert_eq!(f.turns[2].files, ["f4.rs"]);
    assert_eq!(f.files_changed, ["f2.rs", "f3.rs", "f4.rs"]);
    assert_eq!(f.checks, ["cargo test", "make test"], "the project's checks and what passed in the thread, not what failed");
    assert_eq!(f.repo[..3], ["f2.rs", "f3.rs", "f4.rs"], "what changed is first");
    assert!(f.repo.contains(&"a.txt".to_string()), "outside git the folder is walked");
}

#[test]
fn a_folder_walk_leaves_out_hidden_folders_and_build_output() {
    let d = dir("walk");
    for p in ["src/main.rs", "target/debug/x", ".git/config", "node_modules/m/i.js", "README.md"] {
        std::fs::create_dir_all(d.join(p).parent().unwrap()).unwrap();
        std::fs::write(d.join(p), "x").unwrap();
    }
    let log = log(&d);
    let mut f = state::facts(&log, "main", "x", &d, &[]).repo;
    f.sort();
    assert_eq!(f, ["README.md", "src/main.rs"]);
}

#[test]
fn the_state_is_json_and_over_two_thousand_tokens_the_file_list_is_cut_first_then_the_answer_heads() {
    let big = Facts {
        line: "fix it".into(),
        turns: vec![Turn { id: "t1".into(), you: "a line".into(), answer_head: "head ".repeat(60), files: vec![] }],
        repo: (0..200).map(|i| format!("crates/some/long/path/number/{i}/file.rs")).collect(),
        ..Facts::default()
    };
    let (text, tokens) = state::render(&big, "claude", true, "", "notes", "keep", "list");
    assert!(tokens <= STATE_TOKENS, "{tokens}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["line"], "fix it");
    assert_eq!(v["target"], json!({ "doer": "claude", "received_turns": true }));
    assert!(v["repo_map"].as_array().unwrap().len() < 200 && !v["repo_map"].as_array().unwrap().is_empty());
    assert_eq!(v["turns"][0]["answer_head"], big.turns[0].answer_head, "the answer heads stay while the file list can be cut");
    let heavy = Facts { turns: vec![Turn { id: "t1".into(), you: "a line".into(), answer_head: "head ".repeat(2_000), files: vec![] }], ..big.clone() };
    let (text, tokens) = state::render(&heavy, "claude", false, "name the tests", "rewrite", "plain", "xml");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert!(v["repo_map"].as_array().unwrap().is_empty(), "the file list went first");
    assert_eq!(v["turns"][0]["answer_head"], "", "then the answer heads, until it fits or nothing is left to cut ({tokens})");
    assert_eq!((v["target"]["received_turns"].clone(), v["words"].clone(), v["style"].clone(), v["tone"].clone(), v["format"].clone()), (json!(false), json!("name the tests"), json!("rewrite"), json!("plain"), json!("xml")));
}
