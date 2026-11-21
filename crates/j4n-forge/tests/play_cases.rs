//! The forge makes the same harness from the same seed, every axis has its values, and a harness does what its spec says.

use j4n_forge::spec::{draw, truth};
use std::path::Path;
use std::process::Command;

fn forge() -> Command {
    Command::new(env!("CARGO_BIN_EXE_j4n-forge"))
}

fn scratch(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("j4n-forge-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_seed_makes_the_same_harness_every_time_and_two_seeds_make_two() {
    let (a, b) = (draw(7), draw(7));
    assert_eq!(toml::to_string(&a).unwrap(), toml::to_string(&b).unwrap());
    assert_ne!(toml::to_string(&draw(8)).unwrap(), toml::to_string(&a).unwrap());
    assert_eq!(draw(7).name, a.name);
}

#[test]
fn two_hundred_seeds_reach_every_value_of_every_axis_and_half_carry_a_held_out_value() {
    let mut seen = std::collections::BTreeSet::new();
    let mut held_seeds = 0;
    for seed in 0..400 {
        let s = draw(seed);
        let t = truth(&s);
        held_seeds += usize::from(!t.held.is_empty());
        for (axis, value) in &t.axes {
            seen.insert(format!("{axis}:{value}"));
        }
    }
    // 7 + 10 + 5 + 5 + 12 + 6 + 9 values
    assert_eq!(seen.len(), 54, "{seen:?}");
    assert!((120..=300).contains(&held_seeds), "{held_seeds} of 400 seeds carry a held-out value");
}

#[test]
fn some_seeds_are_seen_families_only_and_say_so() {
    let clean = (0..60).map(|s| truth(&draw(s))).filter(|t| t.held.is_empty()).count();
    assert!(clean >= 15, "{clean}");
}

fn make(seed: u64, dir: &Path) -> Command {
    let out = forge().args(["make", "--agent", "--seed", &seed.to_string(), "--out"]).arg(dir).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let mut c = Command::new(dir.join(&draw(seed).name));
    c.current_dir(dir);
    c
}

/// The first seed whose truth holds (the test names what it needs, not a number).
fn seed_where(ok: impl Fn(&j4n_forge::spec::Truth) -> bool) -> u64 {
    (0..400_000).find(|n| ok(&truth(&draw(*n)))).expect("a seed like that")
}

/// The words that carry `text` to the harness of this seed, for the ways a command line can (flag, positional, subcommand).
fn say(seed: u64, text: &str) -> Vec<String> {
    let s = draw(seed);
    match s.a.kind.as_str() {
        "flag" => vec![s.a.get("flag").to_string(), text.to_string()],
        "subcommand" => s.a.get("sub").split(' ').map(String::from).chain([text.to_string()]).collect(),
        _ => vec![text.to_string()],
    }
}

fn by_line(t: &j4n_forge::spec::Truth) -> bool {
    matches!(t.axes["a"].as_str(), "positional" | "flag" | "subcommand") && t.axes["b"] != "http" && t.axes["g"] == "clean"
}

#[test]
fn a_harness_says_the_word_back_prints_its_version_and_wants_a_prompt() {
    let seed = seed_where(|t| by_line(t) && t.axes["e"] == "none");
    let dir = scratch("word");
    let out = make(seed, &dir).args(say(seed, "Reply with the single word: plum")).output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("plum"), "{}", String::from_utf8_lossy(&out.stdout));
    let v = make(seed, &dir).arg("--version").output().unwrap();
    assert!(String::from_utf8_lossy(&v.stdout).contains(&draw(seed).name));
    // no prompt: a usage error, not a hang
    let e = make(seed, &dir).output().unwrap();
    assert_eq!(e.status.code(), Some(2));
}

#[test]
fn a_harness_with_tools_writes_and_reads_files_and_one_that_asks_waits_for_its_yes() {
    let seed = seed_where(|t| by_line(t) && t.axes["e"] == "none" && t.axes["d"] == "events");
    let dir = scratch("tools");
    make(seed, &dir).args(say(seed, "write hello to a.txt")).output().unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "hello");
    let out = make(seed, &dir).args(say(seed, "read a.txt")).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("hello"));

    let seed = seed_where(|t| by_line(t) && t.axes["e"] == "none" && t.axes["d"] == "ask_stdin" && t.axes["a"] != "stdin");
    let dir = scratch("asks");
    let mut c = make(seed, &dir);
    c.args(say(seed, "write secret to b.txt")).stdin(std::process::Stdio::null());
    let out = c.output().unwrap();
    assert!(!dir.join("b.txt").exists(), "no answer is no: {}", String::from_utf8_lossy(&out.stdout));
    assert!(String::from_utf8_lossy(&out.stdout).contains("[y/n]"));
}

#[test]
fn a_limit_is_spent_and_the_harness_keeps_the_numbers_it_really_has() {
    let seed = seed_where(|t| by_line(t) && t.axes["e"] == "requests");
    let dir = scratch("limit");
    let cap = draw(seed).e.num("cap") as usize;
    for i in 0..cap {
        let out = make(seed, &dir).args(say(seed, &format!("Reply with the single word: w{i}"))).output().unwrap();
        assert!(out.status.success(), "turn {i}");
    }
    let out = make(seed, &dir).args(say(seed, "Reply with the single word: over")).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stdout).contains("limit reached"));
    let state: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join(format!(".{}/limit.json", draw(seed).name))).unwrap()).unwrap();
    assert_eq!(state["limited"], true);
    assert_eq!(state["left"].as_f64().unwrap().round() as usize, 0);
}

#[test]
fn each_way_of_printing_names_the_word_and_a_login_that_is_needed_exits_3() {
    for kind in ["plain", "ansi", "json", "jsonl", "sse", "frames", "xml", "logfmt", "jsonrpc"] {
        let seed = seed_where(|t| by_line(t) && t.axes["e"] == "none" && t.axes["b"] == kind);
        let dir = scratch(kind);
        let out = make(seed, &dir).args(say(seed, "Reply with the single word: plum")).output().unwrap();
        assert!(String::from_utf8_lossy(&out.stdout).contains("plum"), "{kind}: {:?}", String::from_utf8_lossy(&out.stdout));
    }
    let seed = seed_where(|t| by_line_g(t, "login"));
    let dir = scratch("login");
    let out = make(seed, &dir).args(say(seed, "hello")).output().unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stdout).contains("sign in"));
}

/// A harness that crashes half way through its answer dies by a signal, as an out-of-memory kill does: its `kill` is started with no
/// environment but its PATH, so no key of the operator's reaches it, and it still finds the program.
#[test]
fn a_harness_that_crashes_half_way_is_killed_by_a_signal_its_kill_found_with_no_environment_but_its_path() {
    use std::os::unix::process::ExitStatusExt as _;
    let seed = seed_where(|t| by_line_g(t, "crash_half") && t.axes["e"] == "none");
    let dir = scratch("crash-half");
    let out = make(seed, &dir).args(say(seed, "trigger failure")).env("SOME_SERVICE_TOKEN", "forge7Qx2Lm9Vb4Kt").output().unwrap();
    assert_eq!(out.status.signal(), Some(9), "killed, not exited: {:?} {}", out.status, String::from_utf8_lossy(&out.stderr));
}

fn by_line_g(t: &j4n_forge::spec::Truth, g: &str) -> bool {
    matches!(t.axes["a"].as_str(), "positional" | "flag" | "subcommand") && t.axes["b"] != "http" && t.axes["g"] == g
}

// ---- the tool family ----

use j4n_forge::tool::{TOOL_BASE, draw as tool_draw, truth as tool_truth};
use std::path::PathBuf;


fn make_tool(seed: u64, name: &str) -> (PathBuf, PathBuf, String) {
    let d = std::env::temp_dir().join(format!("j4n-forge-tool-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let out = d.join("h");
    let ok = Command::new(env!("CARGO_BIN_EXE_j4n-forge")).args(["make", "--seed", &seed.to_string(), "--out"]).arg(&out).status().unwrap();
    assert!(ok.success());
    let spec: toml::Table = std::fs::read_to_string(out.join("spec.toml")).unwrap().parse().unwrap();
    let work = d.join("work");
    std::fs::create_dir_all(&work).unwrap();
    (out.clone(), work, spec["name"].as_str().unwrap().to_string())
}

fn run_tool(h: &Path, work: &Path, name: &str, args: &[&str], stdin: Option<&str>) -> (String, i32) {
    use std::io::Write;
    let mut c = Command::new(h.join(name));
    c.args(args).current_dir(work).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let mut child = c.spawn().unwrap();
    if let Some(t) = stdin {
        let _ = child.stdin.take().unwrap().write_all(t.as_bytes());
    }
    let o = child.wait_with_output().unwrap();
    (format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)), o.status.code().unwrap_or(-1))
}

#[test]
fn a_tool_seed_makes_the_same_tool_every_time_and_the_agent_seeds_are_as_they_were() {
    assert_eq!(toml::to_string(&tool_draw(TOOL_BASE + 7)).unwrap(), toml::to_string(&tool_draw(TOOL_BASE + 7)).unwrap());
    assert_ne!(toml::to_string(&tool_draw(TOOL_BASE + 8)).unwrap(), toml::to_string(&tool_draw(TOOL_BASE + 7)).unwrap());
    // seeds below the base are the agents, whose harness did not change
    let (_, _, agent) = make_tool(7, "agent");
    assert_eq!(agent, j4n_forge::spec::draw(7).name);
}

#[test]
fn two_hundred_tool_seeds_reach_every_value_of_every_axis_and_half_carry_a_held_out_value() {
    let mut seen = std::collections::BTreeSet::new();
    let mut held = 0;
    for seed in TOOL_BASE..TOOL_BASE + 400 {
        let t = tool_truth(&tool_draw(seed));
        held += usize::from(!t.held.is_empty());
        for (k, v) in &t.axes {
            seen.insert(format!("{k}:{v}"));
        }
    }
    // 6 + 6 + 6 + 5 + 6 values
    assert_eq!(seen.len(), 29, "{seen:?}");
    assert!((120..=300).contains(&held), "{held} of 400 seeds carry a held-out value");
}

#[test]
fn a_tool_with_subcommands_shows_counts_writes_and_deletes_files_and_says_what_each_does() {
    let seed = (TOOL_BASE..TOOL_BASE + 400).find(|s| tool_draw(*s).axes["t1"] == "subs" && tool_draw(*s).axes["t2"] == "full_help" && tool_draw(*s).axes["t3"] == "text" && tool_draw(*s).axes["t5"] == "wrong_flag").expect("a seed");
    let (h, work, name) = make_tool(seed, "subs");
    let t = tool_draw(seed);
    let cmd = |op: &str| t.cmds.iter().find(|c| c.op == op).unwrap().name.clone();
    std::fs::write(work.join("notes.txt"), "alpha\nbeta\n").unwrap();
    assert_eq!(run_tool(&h, &work, &name, &[&cmd("show"), "notes.txt"], None), ("alpha\nbeta\n".to_string(), 0));
    assert_eq!(run_tool(&h, &work, &name, &[&cmd("count"), "notes.txt"], None).0, "2\n");
    assert_eq!(run_tool(&h, &work, &name, &[&cmd("put"), "hi", "made.txt"], None).1, 0);
    assert_eq!(std::fs::read_to_string(work.join("made.txt")).unwrap(), "hi");
    assert_eq!(run_tool(&h, &work, &name, &[&cmd("rm"), "made.txt"], None).1, 0);
    assert!(!work.join("made.txt").exists());
    let help = run_tool(&h, &work, &name, &["--help"], None).0;
    assert!(help.contains("deletes the file") && help.contains("writes the text"), "{help}");
    let (bad, code) = run_tool(&h, &work, &name, &["--nope"], None);
    assert_eq!(code, 2);
    assert!(bad.contains("usage:"), "{bad}");
    let (missing, code) = run_tool(&h, &work, &name, &[&cmd("show"), "absent.txt"], None);
    assert_eq!(code, 1, "{missing}");
}

#[test]
fn a_filter_reads_standard_input_and_a_tool_that_offers_json_says_it_in_its_help_and_prints_it() {
    let seed = (TOOL_BASE..TOOL_BASE + 400).find(|s| tool_draw(*s).axes["t1"] == "filter" && tool_draw(*s).axes["t2"] == "full_help" && tool_draw(*s).axes["t3"] == "json" && tool_draw(*s).axes["t5"] != "hang").expect("a seed");
    let (h, work, name) = make_tool(seed, "filter");
    assert!(run_tool(&h, &work, &name, &["--help"], None).0.contains("standard input"));
    let flag: Vec<String> = tool_draw(seed).json_flag.split(' ').map(String::from).collect();
    let flag: Vec<&str> = flag.iter().map(String::as_str).collect();
    let (out, code) = run_tool(&h, &work, &name, &flag, Some("abc\n"));
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("{\"text\":\"ABC\"}"), "{out}");
}
