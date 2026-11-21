//! Safe anywhere. Tier 0: the defaults an operator gets with no word said, one test each — a write
//! outside the project asks, `.j4n` and git's internals are blocked, a checkpoint is made before work, claude's writes and commands come
//! to the Gate — then the refusal to start as the root of `$HOME` or of the disk, the place and mode said on the first start, and the list
//! of what is off (the packs and the switches) kept the same as the packs and settings it describes. The binary runs with a fake HOME.

use crate::run_cases::{Dirs, Out, fake_claude, j4n, log_events};
use crate::support::{read, root};
use j4n_core::change::Actor;
use j4n_core::doctor::{DoerFact, Facts, report};
use j4n_core::gate::check::Gates;
use j4n_core::gate::grant::Grants;
use j4n_core::gate::{Action, Gate, Verb, Verdict};
use j4n_core::table::Tables;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

/// The Gate a J4n has when the operator has said nothing: the packs that are on by default, no row of the operator's.
fn default_gate() -> Gate {
    let harness = j4n::pack::catalog().harness(&j4n::pack::ON, &[]);
    let (tables, problems) = Tables::load(harness.layers.clone());
    assert!(problems.iter().all(|p| !p.to_string().contains("gate")), "{problems:?}");
    let (gates, notes) = Gates::load(&tables);
    assert!(notes.is_empty(), "{notes:?}");
    Gate::new(gates, Grants::new("s1"))
}

fn says(g: &Gate, verb: Verb, actor: Actor, path: &str, inside: Option<bool>) -> String {
    let mut a = Action::new(verb, actor, path).task("t1");
    a.inside = inside;
    match g.check(&a) {
        Verdict::Block { by, .. } => format!("block {by}"),
        v => v.word().into(),
    }
}

#[test]
fn a_write_outside_the_project_or_not_known_to_be_inside_asks_by_default() {
    let g = default_gate();
    for actor in [Actor::Doer("claude".into()), Actor::Doer("pi".into()), Actor::Doer("own".into())] {
        assert_eq!(says(&g, Verb::Write, actor.clone(), "/home/u/notes.txt", Some(false)), "ask", "{actor:?}");
        assert_eq!(says(&g, Verb::Write, actor.clone(), "../sibling/a.txt", Some(false)), "ask");
        assert_eq!(says(&g, Verb::Write, actor.clone(), "src/a.rs", None), "ask", "a path not known to be inside asks");
        assert_ne!(says(&g, Verb::Write, actor, "src/a.rs", Some(true)), "ask", "inside the project is not this gate's question");
    }
}

#[test]
fn j4n_s_own_folder_and_git_s_internals_are_blocked_by_default_and_a_grant_does_not_help() {
    let g = default_gate();
    for actor in [Actor::Doer("claude".into()), Actor::Doer("gemini".into())] {
        for verb in [Verb::Write, Verb::Apply] {
            assert_eq!(says(&g, verb, actor.clone(), ".git/config", Some(true)), "block git-internals");
            assert_eq!(says(&g, verb, actor.clone(), "sub/.git/hooks/pre-commit", Some(true)), "block git-internals");
            assert_eq!(says(&g, verb, actor.clone(), ".j4n/log/0001.jsonl", Some(true)), "block core-owns-j4n");
            assert_eq!(says(&g, verb, actor.clone(), "app/.j4n/rules.toml", Some(true)), "block core-owns-j4n");
        }
    }
    // the operator edits their own rules; only a tool of a doer or a model is held
    assert_eq!(says(&g, Verb::Write, Actor::Operator, ".j4n/rules.toml", Some(true)), "allow");
}

/// A fake `claude` that asks for each of these tools, one after the other, and goes on whatever the answer is.
fn claude_asking(d: &Dirs, tools: &[(&str, &str)]) -> Vec<(&'static str, String)> {
    let (program, _, log) = fake_claude(d, "unused");
    let mut frames = String::from(r#"{"type":"system","subtype":"init","cwd":"/work","session_id":"00000000-0000-4000-8000-0000000000d1","model":"claude-haiku-5-5","permissionMode":"default","tools":["Bash","Read","Write"]}"#);
    for (n, (tool, input)) in tools.iter().enumerate() {
        frames.push_str(&format!(
            "\n{{\"type\":\"control_request\",\"request_id\":\"r{n}\",\"request\":{{\"subtype\":\"can_use_tool\",\"tool_name\":\"{tool}\",\"input\":{input},\"tool_use_id\":\"toolu_{n}\"}},\"session_id\":\"s\"}}"
        ));
    }
    frames.push_str("\n{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"done\",\"session_id\":\"s\",\"total_cost_usd\":0,\"usage\":{}}\n");
    let fixture = d.dir.join("asking.jsonl");
    fs::write(&fixture, frames).unwrap();
    vec![("J4N_CLAUDE", program.display().to_string()), ("FAKE_FIXTURE", fixture.display().to_string()), ("FAKE_LOG", log.display().to_string())]
}

/// The requests the fake claude was answered yes to, by their ids.
fn allowed(d: &Dirs) -> Vec<String> {
    read(&d.dir.join("bin/log"))
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["type"] == "control_response" && v["response"]["response"]["behavior"] == "allow")
        .map(|v| v["response"]["request_id"].as_str().unwrap_or_default().to_string())
        .collect()
}

#[test]
fn claude_s_writes_and_commands_come_to_the_gate_and_the_defaults_hold_in_the_real_run() {
    let d = Dirs::new("safe-anywhere-claude");
    d.put("src/a.rs", "fn a() {}\n");
    let tools = [
        ("Read", r#"{"file_path":"src/a.rs"}"#),
        ("Write", r#"{"file_path":"../outside.txt","content":"x"}"#),
        ("Write", r#"{"file_path":".git/config","content":"x"}"#),
        ("Edit", r#"{"file_path":".j4n/log/0001.jsonl","old_string":"a","new_string":"b"}"#),
        ("Bash", r#"{"command":"echo x > .git/hooks/pre-commit"}"#),
        ("Bash", r#"{"command":"rm -rf ~"}"#),
    ];
    let env = claude_asking(&d, &tools);
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let o = j4n(&d, &["run", "go", "--doer", "claude", "--approval", "deny"], None, &env);
    assert_eq!(o.rc, 0, "{}{}", o.out, o.err);
    let verdicts: Vec<(String, String)> =
        log_events(&d).iter().filter(|e| e["kind"] == "gate.verdict").map(|e| (e["data"]["target"].as_str().unwrap_or_default().to_string(), e["data"]["verdict"].as_str().unwrap_or_default().to_string())).collect();
    let of = |needle: &str| verdicts.iter().find(|(t, _)| t.contains(needle)).map(|(_, v)| v.as_str());
    assert!(verdicts.len() >= 4, "every blocked request was said: {verdicts:?}");
    let asked = |needle: &str| log_events(&d).iter().filter(|e| e["kind"] == "task.queued" && e.to_string().contains(needle)).count();
    assert_eq!((asked("outside.txt"), of("outside.txt")), (1, None), "a write outside is a question to the operator, not a block: {verdicts:?}");
    assert_eq!(of(".git/config"), Some("block"), "{verdicts:?}");
    assert_eq!(of("0001.jsonl"), Some("block"), "{verdicts:?}");
    assert_eq!(of("pre-commit"), Some("block"), "{verdicts:?}");
    assert_eq!(of("rm -rf"), Some("block"), "{verdicts:?}");
    assert!(!d.project().parent().unwrap().join("outside.txt").exists(), "nothing was written outside");
    assert_eq!(allowed(&d), Vec::<String>::new(), "nobody was there: every question was answered no\n{}", read(&d.dir.join("bin/log")));
    // `--approval full` is a yes to what the Gate does not block and asks about only inside the project: the read, the question of the operator, and not a
    // block, whoever says yes. A write outside the project is asked at every level: with nobody to ask, the no stands
    let d = Dirs::new("safe-anywhere-claude-full");
    d.put("src/a.rs", "fn a() {}\n");
    let env = claude_asking(&d, &tools);
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let o = j4n(&d, &["run", "go", "--doer", "claude", "--approval", "full"], None, &env);
    assert_eq!(o.rc, 0, "{}{}", o.out, o.err);
    assert_eq!(allowed(&d), ["r0"], "the Gate's blocks, and a write outside the project, stand against a yes to everything\n{}", read(&d.dir.join("bin/log")));
}

#[test]
fn a_task_that_writes_is_checkpointed_first_and_a_read_task_is_not() {
    let d = Dirs::new("safe-anywhere-checkpoint");
    d.put("keep.txt", "mine\n");
    let o = j4n(&d, &["run", "write out/b.txt y", "--doer", "test", "--approval", "full"], None, &[]);
    assert_eq!(o.rc, 0, "{}", o.err);
    assert!(!o.err.contains("no checkpoint"), "{}", o.err);
    let store: Vec<_> = fs::read_dir(d.j4n_home().join("checkpoints")).expect("a store of checkpoints").flatten().collect();
    assert_eq!(store.len(), 1, "one folder's store");
    let tasks: Vec<String> = fs::read_dir(store[0].path()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(tasks.iter().any(|t| t.starts_with('t')), "a checkpoint of the task: {tasks:?}");
    assert!(store[0].path().join(&tasks[0]).join("files/keep.txt").exists(), "the folder as it stood is kept");
    let d = Dirs::new("safe-anywhere-read");
    let o = j4n(&d, &["run", "say hi", "--doer", "test", "--mode", "read"], None, &[]);
    assert_eq!(o.rc, 0, "{}", o.err);
    assert!(!d.j4n_home().join("checkpoints").exists(), "a read task changes nothing, so it is not checkpointed");
}

/// A project of more than the checkpoint's 100,000 files.
fn too_big(tag: &str) -> Dirs {
    let d = Dirs::new(tag);
    d.put("keep.txt", "mine\n");
    let lots = d.project().join("lots");
    fs::create_dir_all(&lots).unwrap();
    for i in 0..100_005 {
        fs::write(lots.join(format!("f{i}")), "").unwrap();
    }
    d
}

#[test]
fn a_write_task_in_a_folder_too_big_to_checkpoint_asks_once_and_deny_stops_it_before_it_writes() {
    let d = too_big("safe-anywhere-big-deny");
    let o = j4n(&d, &["run", "write out/b.txt y", "--doer", "test", "--approval", "deny"], None, &[]);
    assert!(!d.project().join("out/b.txt").exists(), "the answer was no, so nothing was written: {}", o.err);
    let events = log_events(&d);
    let asks: Vec<_> = events.iter().filter(|e| e["kind"] == "ask.answered").collect();
    assert_eq!(asks.len(), 1, "one question for the task");
    assert_eq!(asks[0]["data"]["answer"], "no");
    let text = events.iter().map(|e| e.to_string()).collect::<String>();
    assert!(text.contains("too many to checkpoint") && text.contains("Go on with no checkpoint, or stop?"), "the question says why");
    assert!(text.contains("stopped before it started"), "and the stop says what happened");
    assert!(!d.j4n_home().join("checkpoints").exists() || fs::read_dir(d.j4n_home().join("checkpoints")).unwrap().flatten().all(|e| !e.path().join("t1").exists()));
}

#[test]
fn a_write_task_in_a_folder_too_big_to_checkpoint_goes_on_with_a_yes_and_says_it_has_no_checkpoint() {
    let d = too_big("safe-anywhere-big-full");
    let o = j4n(&d, &["run", "write out/b.txt y", "--doer", "test", "--approval", "full"], None, &[]);
    assert_eq!(o.rc, 0, "{}", o.err);
    assert!(d.project().join("out/b.txt").exists(), "a yes goes on");
    let events = log_events(&d);
    // the place's question names no effect (the write itself is asked about, and answered, as it always was)
    assert_eq!(events.iter().filter(|e| e["kind"] == "ask.answered" && e["data"].get("verb").is_none()).count(), 1, "one question about the missing checkpoint");
    assert!(events.iter().any(|e| e["kind"] == "run.notice" && e.to_string().contains("no checkpoint, so this task cannot be rewound")));
}

#[test]
fn a_read_task_in_a_folder_too_big_to_checkpoint_does_not_ask() {
    let d = too_big("safe-anywhere-big-read");
    let o = j4n(&d, &["run", "say hi", "--doer", "test", "--mode", "read"], None, &[]);
    assert_eq!(o.rc, 0, "{}", o.err);
    assert_eq!(log_events(&d).iter().filter(|e| e["kind"] == "ask.answered").count(), 0);
}

// ---- the refusal ----

#[test]
fn a_path_that_is_the_root_of_the_disk_or_of_home_is_refused_with_how_to_go_on() {
    let home = Dirs::new("safe-anywhere-refuse");
    let h = home.home();
    let why = j4n::refuse_place(&h, Some(&h)).expect("home is refused");
    assert!(why.contains("your home folder") && why.contains("--anywhere") && why.contains("project folder") && !why.contains('\n'), "{why}");
    let why = j4n::refuse_place(Path::new("/"), Some(&h)).expect("the root is refused");
    assert!(why.contains("the root of the disk") && why.contains("--anywhere"), "{why}");
    // a folder in home is a project; so is one when no home is known, and a link to home is home
    assert_eq!(j4n::refuse_place(&home.project(), Some(&h)), None);
    assert_eq!(j4n::refuse_place(&h.join("code/app"), Some(&h)), None);
    assert_eq!(j4n::refuse_place(&home.project(), None), None);
    let link = home.dir.join("link");
    std::os::unix::fs::symlink(&h, &link).unwrap();
    assert!(j4n::refuse_place(&link, Some(&h)).is_some(), "a link to home is home");
    assert!(j4n::refuse_place(&h, Some(&link)).is_some(), "home named by a link is home");
}

/// `j4n args` in `cwd`, with the fake HOME of `d` (never the real one).
fn j4n_in(d: &Dirs, cwd: &Path, args: &[&str]) -> Out {
    let out = Command::new(env!("CARGO_BIN_EXE_j4n"))
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", d.home())
        .env("J4N_HOME", d.j4n_home())
        .env("https_proxy", "http://127.0.0.1:9")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("j4n runs");
    Out { rc: out.status.code().unwrap_or(-1), out: String::from_utf8_lossy(&out.stdout).into_owned(), err: String::from_utf8_lossy(&out.stderr).into_owned(), secs: 0.0 }
}

#[test]
fn j4n_started_in_home_refuses_in_one_line_and_writes_nothing_and_anywhere_goes_on() {
    let d = Dirs::new("safe-anywhere-home");
    let before = fs::read_dir(d.home()).unwrap().flatten().count();
    for args in [&["run", "say hi", "--doer", "test"][..], &["--doer", "test"], &["plan", "--doer", "test", "add a flag"]] {
        let o = j4n_in(&d, &d.home(), args);
        assert_eq!(o.rc, 2, "{args:?}: {}{}", o.out, o.err);
        assert!(o.out.is_empty() && o.err.lines().count() == 1, "one line only: {}", o.err);
        assert!(o.err.starts_with("j4n: this folder is your home folder") && o.err.contains("--anywhere"), "{}", o.err);
    }
    assert_eq!(fs::read_dir(d.home()).unwrap().flatten().count(), before, "nothing was made in home");
    assert!(!d.home().join(".j4n/log").exists());
    let o = j4n_in(&d, &d.home(), &["run", "say hi", "--doer", "test", "--anywhere"]);
    assert_eq!((o.rc, o.out.trim()), (0, "hi"), "{}", o.err);
    // the commands that open nothing are not held to a place
    assert_eq!(j4n_in(&d, &d.home(), &["--version"]).rc, 0);
    assert_eq!(j4n_in(&d, &d.home(), &["--help"]).rc, 0);
}

#[test]
fn j4n_started_in_the_root_of_the_disk_refuses_the_same_way() {
    let d = Dirs::new("safe-anywhere-root");
    let o = j4n_in(&d, Path::new("/"), &["run", "say hi", "--doer", "test"]);
    assert_eq!(o.rc, 2, "{}{}", o.out, o.err);
    assert!(o.err.starts_with("j4n: this folder is the root of the disk") && o.err.contains("--anywhere") && o.err.lines().count() == 1, "{}", o.err);
    assert!(!Path::new("/.j4n").exists());
}

// ---- the first start ----

#[test]
fn the_first_start_says_the_place_and_the_mode_in_the_one_row_the_doctor_already_uses() {
    let d = Dirs::new("safe-anywhere-note");
    let w = crate::doctor_flow::world(&d, false);
    let place = "new folder app · write here · a checkpoint before work";
    let note = j4n::first_start_note(&w.front, true, place).expect("a new folder says its place");
    assert_eq!(note, format!("{place} · pi not found · ⏎ opens Agents › Setup"), "the place, then what is missing, in one row");
    assert_eq!(j4n::first_start_note(&w.front, false, place), None, "a folder J4n has worked in before says nothing");
    w.close();
    let d = Dirs::new("safe-anywhere-note-fine");
    let w = crate::doctor_flow::world(&d, true);
    assert_eq!(j4n::first_start_note(&w.front, true, place).as_deref(), Some(place), "all is fine: the place alone");
    w.close();
}

// ---- what is off ----

fn facts() -> Facts {
    Facts { doers: vec![DoerFact::default()], ..Facts::default() }
}

#[test]
fn the_setup_page_lists_every_pack_that_is_off_and_every_switch_whose_default_is_off_with_its_reason() {
    let rows = report(&facts()).rows;
    let off: Vec<&str> = rows.iter().filter(|r| r.kind == "off").map(|r| r.id.as_str()).collect();
    assert!(rows.iter().filter(|r| r.kind == "off").all(|r| r.what == "off" && r.fix.len() > 20 && r.fix.contains(" · ") && r.tone == "dim"));
    // every pack the binary carries and does not turn on
    let catalog = j4n::pack::catalog();
    let packs: Vec<&str> = catalog.ids().into_iter().filter(|p| !j4n::pack::ON.contains(p)).collect();
    assert!(packs.len() >= 5, "{packs:?}");
    for p in &packs {
        assert!(off.contains(&format!("pack {p}").as_str()), "the pack `{p}` is off and the page does not say why: {off:?}");
    }
    // every setting of a pack whose default is off
    let mut found = Vec::new();
    for dir in fs::read_dir(root().join("packs")).unwrap().flatten() {
        let Ok(files) = fs::read_dir(dir.path().join("settings")) else { continue };
        for f in files.flatten() {
            let table: toml::Table = read(&f.path()).parse().unwrap_or_else(|e| panic!("{}: {e}", f.path().display()));
            for s in table.get("setting").and_then(|s| s.as_array()).into_iter().flatten() {
                let (id, default) = (s["id"].as_str().unwrap(), &s["default"]);
                if default.as_bool() == Some(false) || ["off", "single"].iter().any(|w| default.as_str() == Some(w)) {
                    found.push(id.to_string());
                }
            }
        }
    }
    found.sort();
    // `qc.on` and `shape.on` are the switches inside packs the page lists as packs; `lanes` is the one called `lanes` there
    for id in found.iter().filter(|id| !["qc.on", "shape.on"].contains(&id.as_str())) {
        assert!(off.contains(&id.as_str()), "the setting `{id}` is off by default and the page does not say why: {off:?}");
    }
    // and nothing is listed that is not so
    for id in off.iter().filter(|id| !id.starts_with("pack ")) {
        assert!(found.iter().any(|f| f == id), "`{id}` is listed as off by default and is not: {found:?}");
    }
}
