//! The builtin tools on their own: what each one does to a folder, and what it refuses.

use crate::bench::base_layer;
use crate::world::Scratch;
use j4n_connect::model::Call;
use j4n_connect::model::doer::{ToolOut, Tools};
use j4n_connect::tools::{Builtin, GOES_ON, READ_LINES, confine};
use j4n_core::gate::check::Gates;
use j4n_core::gate::grant::Grants;
use j4n_core::gate::{Gate, ReadOnly};
use j4n_core::place::Site;
use j4n_core::table::Tables;
use serde_json::{Value, json};

fn call(name: &str, args: Value) -> Call {
    Call { id: "c1".into(), name: name.into(), args, bad: None }
}

fn run(dir: &Scratch, name: &str, args: Value) -> ToolOut {
    Builtin::new().run(&call(name, args), &Site::new("t1", dir.path()), &|l| l)
}

fn gate() -> Gate {
    let (tables, _) = Tables::load(vec![base_layer()]);
    Gate::new(Gates::load(&tables).0, Grants::new("s1"))
}

#[test]
fn write_makes_the_folders_it_needs() {
    let dir = Scratch::new("tools");
    let out = run(&dir, "write", json!({ "path": "out/reports/s.md", "content": "# report\n" }));
    assert!(!out.error, "{}", out.text);
    assert_eq!(dir.read("out/reports/s.md"), "# report\n");
    assert_eq!(out.changed, vec!["out/reports/s.md".to_string()]);
    assert!(out.text.contains("made"), "{}", out.text);
    // writing it again replaces it, and says so
    let again = run(&dir, "write", json!({ "path": "./out/reports/s.md", "content": "two\n" }));
    assert!(again.text.contains("replaced"), "{}", again.text);
    assert_eq!(dir.read("out/reports/s.md"), "two\n");
}

#[test]
fn an_absolute_path_inside_the_root_is_that_path_and_the_escapes_stay_refused() {
    let dir = Scratch::new("tools");
    let abs = dir.join("deep/x.txt");
    let out = run(&dir, "write", json!({ "path": abs.display().to_string(), "content": "same" }));
    assert!(!out.error, "{}", out.text);
    assert_eq!(dir.read("deep/x.txt"), "same");
    assert_eq!(out.changed, vec!["deep/x.txt".to_string()], "the same path as the relative one");
    for bad in ["../outside.txt", "a/../../outside.txt", ".git/hooks/pre-commit", "sub/.git/config", ".env", "cfg/.env.local", "/etc/passwd"] {
        let out = run(&dir, "write", json!({ "path": bad, "content": "x" }));
        assert!(out.error, "{bad} was written: {}", out.text);
    }
    assert!(!dir.join("../outside.txt").exists());
    // reading is as shut
    dir.put(".env", "KEY=1\n");
    assert!(run(&dir, "read", json!({ "path": ".env" })).error);
}

#[test]
fn a_link_that_leads_out_of_the_folder_is_not_followed() {
    let dir = Scratch::new("tools");
    let outside = Scratch::new("outside");
    outside.put("secret.txt", "hidden");
    std::os::unix::fs::symlink(outside.path(), dir.join("door")).unwrap();
    assert!(confine(dir.path(), "door/secret.txt", false).is_err());
    assert!(run(&dir, "read", json!({ "path": "door/secret.txt" })).error);
    assert!(run(&dir, "write", json!({ "path": "door/new.txt", "content": "x" })).error);
    assert!(!outside.join("new.txt").exists());
}

#[test]
fn read_numbers_its_lines_and_says_where_the_rest_is() {
    let dir = Scratch::new("tools");
    let text: String = (1..=450).map(|n| format!("line {n}\n")).collect();
    dir.put("big.txt", &text);
    let out = run(&dir, "read", json!({ "path": "big.txt" }));
    assert!(out.text.starts_with("    1  line 1\n"), "{}", out.text);
    assert!(out.text.contains(&format!("{READ_LINES}  line {READ_LINES}\n")), "{}", out.text);
    assert!(out.text.contains(&format!("{GOES_ON}{READ_LINES}; read again with offset {})", READ_LINES + 1)), "{}", out.text);
    let rest = run(&dir, "read", json!({ "path": "big.txt", "offset": 401 }));
    assert!(rest.text.contains("  450  line 450") && !rest.text.contains(GOES_ON), "{}", rest.text);
    assert!(run(&dir, "read", json!({ "path": "big.txt", "offset": 999 })).error);
    dir.put("blob.bin", "ab\0cd");
    assert!(run(&dir, "read", json!({ "path": "blob.bin" })).text.contains("binary"));
}

#[test]
fn list_and_grep_leave_git_and_secrets_alone() {
    let dir = Scratch::new("tools");
    dir.put("src/a.rs", "fn needle() {}\n");
    dir.put("src/deep/b.rs", "let Needle = 1;\n");
    dir.put(".git/config", "needle\n");
    dir.put(".env", "needle=1\n");
    dir.put("target/x.rs", "needle\n");
    let out = run(&dir, "list", json!({}));
    assert_eq!(out.text.lines().collect::<Vec<_>>(), vec!["src/"], "{}", out.text);
    let deep = run(&dir, "list", json!({ "path": "src", "depth": 2 }));
    assert_eq!(deep.text.lines().collect::<Vec<_>>(), vec!["src/a.rs", "src/deep/", "src/deep/b.rs"], "{}", deep.text);
    let hits = run(&dir, "grep", json!({ "pattern": "needle" }));
    assert_eq!(hits.text, "src/a.rs:1:fn needle() {}", "{}", hits.text);
    let folded = run(&dir, "grep", json!({ "pattern": "needle", "ignore_case": true, "path": "src" }));
    assert_eq!(folded.text.lines().count(), 2, "{}", folded.text);
    assert!(run(&dir, "grep", json!({ "pattern": "absent" })).text.contains("no line"));
}

#[test]
fn edit_replaces_one_exact_place_and_says_which() {
    let dir = Scratch::new("tools");
    dir.put("a.rs", "fn a() {}\nfn b() {}\nfn b() {}\n");
    let two = run(&dir, "edit", json!({ "path": "a.rs", "old": "fn b() {}", "new": "fn c() {}" }));
    assert!(two.error && two.text.contains("2 times"), "{}", two.text);
    assert_eq!(dir.read("a.rs"), "fn a() {}\nfn b() {}\nfn b() {}\n", "a refused edit leaves the file alone");
    let one = run(&dir, "edit", json!({ "path": "a.rs", "old": "fn a() {}", "new": "fn a() -> u8 { 1 }" }));
    assert!(!one.error && one.text.contains("line 1"), "{}", one.text);
    let all = run(&dir, "edit", json!({ "path": "a.rs", "old": "fn b() {}", "new": "fn c() {}", "all": true }));
    assert!(all.text.contains("2 places"), "{}", all.text);
    assert_eq!(dir.read("a.rs"), "fn a() -> u8 { 1 }\nfn c() {}\nfn c() {}\n");
    assert!(run(&dir, "edit", json!({ "path": "a.rs", "old": "nowhere", "new": "x" })).error);
    assert!(run(&dir, "edit", json!({ "path": "missing.rs", "old": "a", "new": "b" })).error);
}

#[test]
fn a_command_comes_back_with_its_real_exit_code_and_the_end_of_its_output_l11() {
    let dir = Scratch::new("tools");
    let out = run(&dir, "run", json!({ "cmd": "echo to-out; echo to-err >&2; exit 3" }));
    let shell = out.shell.expect("the exit is recorded");
    assert_eq!(shell.exit, 3);
    assert!(shell.out.contains("to-out") && shell.out.contains("to-err"), "{}", shell.out);
    assert!(out.text.starts_with("exit 3\n") && out.text.contains("to-err"), "{}", out.text);
    let ok = run(&dir, "run", json!({ "cmd": "pwd" }));
    assert_eq!(ok.shell.expect("shell").exit, 0);
    assert!(ok.text.contains(&dir.path().file_name().unwrap().to_string_lossy().to_string()), "it runs in the task's folder: {}", ok.text);
    // a command that does not end is given up on, and the output so far is kept
    let slow = run(&dir, "run", json!({ "cmd": "echo started; sleep 30", "timeout": 1 }));
    assert!(slow.error && slow.text.contains("stopped after 1 s") && slow.text.contains("started"), "{}", slow.text);
}

#[test]
fn the_output_of_a_command_is_cut_to_its_end() {
    let dir = Scratch::new("tools");
    let out = run(&dir, "run", json!({ "cmd": "seq 1 60000" }));
    assert!(out.text.ends_with("60000"), "the tail is kept");
    assert!(out.text.chars().count() < 13_000, "{}", out.text.len());
}

#[test]
fn git_looks_and_does_not_write() {
    let dir = Scratch::new("tools");
    for c in ["git init -q .", "git config user.email a@b.c", "git config user.name n"] {
        assert_eq!(run(&dir, "run", json!({ "cmd": c })).shell.unwrap().exit, 0);
    }
    dir.put("f.txt", "one\n");
    let status = run(&dir, "git", json!({ "op": "status" }));
    assert!(status.text.contains("f.txt"), "{}", status.text);
    run(&dir, "run", json!({ "cmd": "git add . && git commit -qm first" }));
    let log = run(&dir, "git", json!({ "op": "log" }));
    assert!(log.text.contains("first"), "{}", log.text);
    assert!(run(&dir, "git", json!({ "op": "show", "rev": "HEAD" })).text.contains("f.txt"));
    // nothing else is a git op, and a revision cannot be an option
    assert!(run(&dir, "git", json!({ "op": "push" })).error);
    assert!(run(&dir, "git", json!({ "op": "show", "rev": "--output=x" })).error);
}

#[test]
fn the_gate_decides_what_is_asked_and_a_read_only_step_keeps_its_tools() {
    let dir = Scratch::new("tools");
    let site = Site::new("t1", dir.path());
    let tools = Builtin::new().with_gate(gate());
    let ask = |name: &str, args: Value, ro: ReadOnly| tools.gate(&call(name, args), "t1", &site, ro);
    assert!(ask("read", json!({ "path": "a.rs" }), ReadOnly::Explicit).is_none(), "a read asks nobody");
    assert!(ask("grep", json!({ "pattern": "x" }), ReadOnly::No).is_none());
    // a write by a task that may write goes to the Gate's rules: inside the project it needs no question
    assert!(ask("write", json!({ "path": "a.rs", "content": "" }), ReadOnly::No).is_none());
    // a read-only step is blocked from writing; the Gate says so through the question it raises
    let blocked = ask("write", json!({ "path": "a.rs", "content": "" }), ReadOnly::Explicit).expect("a question is raised");
    assert_eq!(blocked.risk, 100);
    let danger = ask("run", json!({ "cmd": "rm -rf /" }), ReadOnly::No).expect("a wipe is never run");
    assert_eq!(danger.risk, 100);
    assert!(ask("run", json!({ "cmd": "git status" }), ReadOnly::Explicit).is_none(), "a line that only reads is allowed to a read-only step");
    // and the tools are the same in both modes
    assert_eq!(tools.specs().iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["list", "read", "grep", "edit", "write", "run", "git"]);
}

/// With the project in the home folder, `.j4n` is J4n's own: the builtin `read` and `list` of its keys and its salt are blocked, its Log is read.
#[test]
fn the_builtin_read_of_j4ns_keys_and_salt_is_blocked_where_the_project_is_the_home_folder() {
    let dir = Scratch::new("tools-home");
    for f in [".j4n/keys/own", ".j4n/guard.salt", ".j4n/log/0001.jsonl"] {
        dir.put(f, "x");
    }
    let site = Site::new("t1", dir.path());
    let tools = Builtin::new().with_gate(gate());
    let ask = |name: &str, path: &str| tools.gate(&call(name, json!({ "path": path })), "t1", &site, ReadOnly::No);
    for (name, path) in [("read", ".j4n/keys/own"), ("list", ".j4n/keys"), ("read", ".j4n/guard.salt")] {
        assert_eq!(ask(name, path).map(|g| g.risk), Some(100), "{name} {path}");
    }
    assert!(ask("read", ".j4n/log/0001.jsonl").is_none(), "the Log is the doer's own state");
}

#[test]
fn without_a_gate_everything_but_a_read_is_asked() {
    let dir = Scratch::new("tools");
    let site = Site::new("t1", dir.path());
    let tools = Builtin::new();
    assert!(tools.gate(&call("read", json!({ "path": "a" })), "t1", &site, ReadOnly::No).is_none());
    assert!(tools.gate(&call("write", json!({ "path": "a", "content": "" })), "t1", &site, ReadOnly::No).is_some());
    assert!(tools.gate(&call("run", json!({ "cmd": "ls" })), "t1", &site, ReadOnly::No).is_some());
}

#[test]
fn an_unknown_tool_says_which_there_are() {
    let dir = Scratch::new("tools");
    let out = run(&dir, "teleport", json!({}));
    assert!(out.error && out.text.contains("list, read, grep, edit, write, run and git"), "{}", out.text);
}

// ---- the bounds, each one on its own ----

#[test]
fn a_read_never_gives_more_than_four_hundred_lines_whatever_the_limit_asked_for() {
    let dir = Scratch::new("tools");
    dir.put("big.txt", &(1..=900).map(|n| format!("l{n}\n")).collect::<String>());
    let out = run(&dir, "read", json!({ "path": "big.txt", "limit": 100_000 }));
    assert_eq!(out.text.lines().filter(|l| l.contains("  l")).count(), READ_LINES, "{}", out.text.lines().count());
    assert!(out.text.contains(GOES_ON));
    let small = run(&dir, "read", json!({ "path": "big.txt", "limit": 3 }));
    assert_eq!(small.text.lines().filter(|l| l.contains("  l")).count(), 3);
}

#[test]
fn a_list_goes_three_deep_and_five_hundred_entries_and_says_how_many_more() {
    let dir = Scratch::new("tools");
    dir.put("a/b/c/d/e/deep.txt", "x");
    let out = run(&dir, "list", json!({ "depth": 99 }));
    assert!(out.text.contains("a/b/c/") && !out.text.contains("a/b/c/d"), "{}", out.text);
    let many = Scratch::new("tools");
    for n in 0..600 {
        many.put(&format!("f{n:04}.txt"), "x");
    }
    let out = run(&many, "list", json!({}));
    assert_eq!(out.text.lines().count(), 501, "500 entries and the note");
    assert!(out.text.contains("(100 more"), "{}", out.text.lines().last().unwrap());
}

#[test]
fn a_grep_gives_two_hundred_hits_and_says_how_many_more() {
    let dir = Scratch::new("tools");
    dir.put("many.txt", &"needle\n".repeat(250));
    let out = run(&dir, "grep", json!({ "pattern": "needle" }));
    assert_eq!(out.text.lines().count(), 201, "200 hits and the note");
    assert!(out.text.contains("(50 more hits"), "{}", out.text.lines().last().unwrap());
}

#[test]
fn a_write_is_cut_at_two_hundred_thousand_bytes_and_a_file_that_is_edited_keeps_its_mode() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Scratch::new("tools");
    let big = "x".repeat(200_001);
    let out = run(&dir, "write", json!({ "path": "big.txt", "content": big }));
    assert!(out.error && out.text.contains("in pieces"), "{}", out.text);
    assert!(!dir.join("big.txt").exists());
    assert!(!run(&dir, "write", json!({ "path": "ok.txt", "content": "x".repeat(200_000) })).error);
    let script = dir.put("run.sh", "#!/bin/sh\necho one\n");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!run(&dir, "edit", json!({ "path": "run.sh", "old": "one", "new": "two" })).error);
    assert_eq!(std::fs::metadata(&script).unwrap().permissions().mode() & 0o777, 0o755);
}

#[test]
fn a_command_is_given_a_second_to_ten_minutes_and_one_that_does_not_end_is_stopped_at_its_time() {
    use j4n_connect::tools::run_limit;
    assert_eq!((run_limit(None), run_limit(Some(0)), run_limit(Some(5)), run_limit(Some(99_999))), (120, 1, 5, 600));
    let dir = Scratch::new("tools");
    let started = std::time::Instant::now();
    let out = run(&dir, "run", json!({ "cmd": "sleep 30", "timeout": 1 }));
    assert!(out.error && started.elapsed() < std::time::Duration::from_secs(10), "{:?}", started.elapsed());
}

#[test]
fn git_diff_and_log_can_be_asked_about_one_path() {
    let dir = Scratch::new("tools");
    for c in ["git init -q .", "git config user.email a@b.c", "git config user.name n"] {
        run(&dir, "run", json!({ "cmd": c }));
    }
    dir.put("a.txt", "one\n");
    dir.put("b.txt", "one\n");
    run(&dir, "run", json!({ "cmd": "git add . && git commit -qm first" }));
    dir.put("a.txt", "two\n");
    dir.put("b.txt", "two\n");
    let diff = run(&dir, "git", json!({ "op": "diff", "path": "a.txt" }));
    assert!(diff.text.contains("a.txt") && !diff.text.contains("b.txt"), "{}", diff.text);
    let all = run(&dir, "git", json!({ "op": "diff" }));
    assert!(all.text.contains("a.txt") && all.text.contains("b.txt"));
}

#[test]
fn a_path_given_to_git_cannot_be_an_option() {
    let dir = Scratch::new("tools");
    for c in ["git init -q .", "git config user.email a@b.c", "git config user.name n"] {
        run(&dir, "run", json!({ "cmd": c }));
    }
    dir.put("a.txt", "one\n");
    run(&dir, "run", json!({ "cmd": "git add . && git commit -qm first" }));
    dir.put("a.txt", "two\n");
    let out = run(&dir, "git", json!({ "op": "diff", "path": "--output=planted.txt" }));
    assert!(!dir.join("planted.txt").exists(), "{}", out.text);
}

// ---- several files in one read ----

#[test]
fn a_read_of_several_paths_gives_each_file_under_its_header_with_the_limits_of_one() {
    let dir = Scratch::new("tools");
    dir.put("a.txt", "alpha\nbeta\n");
    dir.put("b.txt", &(1..=450).map(|n| format!("b{n}\n")).collect::<String>());
    let one = run(&dir, "read", json!({ "path": "a.txt" }));
    assert_eq!(one.text, "    1  alpha\n    2  beta\n", "one path reads as it did");
    let out = run(&dir, "read", json!({ "paths": ["a.txt", "b.txt"] }));
    assert!(!out.error, "{}", out.text);
    assert!(out.text.starts_with("=== a.txt ===\n    1  alpha\n    2  beta\n=== b.txt ===\n    1  b1\n"), "{}", out.text);
    // a file that goes on says where, as one file does
    assert!(out.text.contains(&format!("{GOES_ON}{READ_LINES}; read again with offset {})", READ_LINES + 1)), "{}", out.text);
    // the offset and the limit are each file's own
    let sliced = run(&dir, "read", json!({ "paths": ["a.txt", "b.txt"], "offset": 2, "limit": 1 }));
    assert!(sliced.text.contains("=== a.txt ===\n    2  beta\n=== b.txt ===\n    2  b2\n"), "{}", sliced.text);
    // `path` and `paths` together are both read, and neither is a call that fails
    let both = run(&dir, "read", json!({ "path": "a.txt", "paths": ["b.txt"], "limit": 1 }));
    assert!(both.text.contains("=== a.txt ===") && both.text.contains("=== b.txt ==="), "{}", both.text);
    assert!(run(&dir, "read", json!({})).error);
}

#[test]
fn a_secret_among_the_paths_is_refused_alone_and_the_call_does_not_fail() {
    let dir = Scratch::new("tools");
    dir.put("a.txt", "alpha\n");
    dir.put(".env", "KEY=1\n");
    let out = run(&dir, "read", json!({ "paths": ["a.txt", ".env", "missing.txt", "../outside.txt"] }));
    assert!(!out.error, "one file was read: {}", out.text);
    assert!(out.text.contains("=== a.txt ===\n    1  alpha\n"), "{}", out.text);
    assert!(out.text.contains("=== .env ===\n`.env` is not reached by a tool"), "{}", out.text);
    assert!(!out.text.contains("KEY=1"), "{}", out.text);
    assert!(out.text.contains("=== missing.txt ===") && out.text.contains("=== ../outside.txt ==="), "{}", out.text);
    // when nothing could be read the call says it failed
    assert!(run(&dir, "read", json!({ "paths": [".env", "nothing"] })).error);
    // the Gate is asked about every path of the call: a read asks nobody, as one does, and a path that is no file of the project is the tool's to refuse
    let site = Site::new("t1", dir.path());
    let tools = Builtin::new().with_gate(gate());
    assert!(tools.gate(&call("read", json!({ "paths": ["a.txt", ".env", "../x"] })), "t1", &site, ReadOnly::Explicit).is_none());
}

#[test]
fn one_call_reads_eight_files_and_eight_hundred_lines_and_says_what_it_left() {
    let dir = Scratch::new("tools");
    for n in 0..10 {
        dir.put(&format!("f{n}.txt"), &(1..=300).map(|i| format!("f{n}-{i}\n")).collect::<String>());
    }
    let names: Vec<String> = (0..10).map(|n| format!("f{n}.txt")).collect();
    let out = run(&dir, "read", json!({ "paths": names }));
    let lines = out.text.lines().filter(|l| l.contains("  f")).count();
    assert_eq!(lines, 800, "the lines of all the files together: {lines}");
    // two files whole (600), a third cut at the 200 that were left, the rest not read
    assert!(out.text.contains("=== f2.txt ===") && out.text.contains(&format!("{GOES_ON}200; read this file again with offset 201)")), "{}", out.text);
    assert!(out.text.contains("=== f3.txt ===\nnot read: one call reads 8 files and 800 lines at most"), "{}", out.text);
    assert!(out.text.contains("=== f9.txt ===\nnot read"), "{}", out.text);
    // the tool says so in one plain sentence, and the call needs neither argument by schema
    let spec = Builtin::new().specs().into_iter().find(|t| t.name == "read").unwrap();
    assert!(spec.doc.contains("`paths`: several small files in one call") && spec.doc.len() < 120, "{}", spec.doc);
}
