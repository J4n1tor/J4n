//! A connected program's own usage limit, through the real binary: what its output says of the limit becomes a window of the Log (the gauge the
//! router reads), and a refusal waits for the time the program named, not the failure table's default.

use crate::run_cases::{Dirs, j4n, kinds, log_events};
use std::fs;
use std::os::unix::fs::PermissionsExt;

/// A program that keeps a count in the folder it runs in and says it in a footer, as each turn ends.
const COUNTER: &str = "n=$(cat .count 2>/dev/null || echo 0); n=$((n+1)); echo $n > .count\necho ready\necho \"[req $n/5 · resets in 4m10s]\"\n";
/// A program that refuses: it names the time it works again, on stderr, and ends with its own code.
const REFUSER: &str = "echo \"rate limited: retry in 4m10s\" >&2\nexit 75\n";

fn connect(d: &Dirs, id: &str, body: &str, row: &str) {
    let path = d.dir.join("bin").join(id);
    fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    let home = d.j4n_home();
    fs::create_dir_all(home.join("connectors")).unwrap();
    fs::create_dir_all(home.join("settings")).unwrap();
    fs::write(home.join("settings/box.toml"), "api = 1\n[[setting]]\nid = \"box.unavailable\"\nvalue = \"here\"\n").unwrap();
    fs::write(home.join(format!("connectors/{id}.toml")), format!("api = 1\n[[connector]]\nid = \"{id}\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"{}\"\nstart = [\"{{prompt}}\"]\n{row}", path.display())).unwrap();
}

fn windows(d: &Dirs, doer: &str) -> Vec<serde_json::Value> {
    log_events(d).into_iter().filter(|e| e["kind"] == "run.window" && e["by"]["doer"] == doer).collect()
}

#[test]
fn the_footer_a_program_prints_is_read_into_the_windows_the_router_reads() {
    let d = Dirs::new("limit-footer");
    connect(&d, "counter", COUNTER, "limits = [{ line = \"[{unit} {used}/{cap} · resets in {reset}]\" }]\n");
    for _ in 0..2 {
        let o = j4n(&d, &["run", "hi", "--doer", "counter"], None, &[]);
        assert_eq!(o.rc, 0, "{}", o.err);
    }
    let w = windows(&d, "counter");
    let seen: Vec<(f64, &str, &str)> = w.iter().map(|e| (e["data"]["utilization"].as_f64().unwrap(), e["data"]["window"].as_str().unwrap(), e["data"]["status"].as_str().unwrap())).collect();
    assert_eq!(seen, [(0.2, "req", "allowed"), (0.4, "req", "allowed")]);
    let (at, ends) = (w[1]["at_ms"].as_f64().unwrap() / 1000.0, w[1]["data"]["resets_at"].as_f64().unwrap());
    assert!((ends - at - 250.0).abs() < 5.0, "the window ends in the 4m10s the footer says: {}", ends - at);
}

#[test]
fn a_program_with_no_row_that_refuses_for_its_limit_says_when_it_works_again_in_its_own_time() {
    let d = Dirs::new("limit-refuse");
    connect(&d, "refuser", REFUSER, "");
    let o = j4n(&d, &["run", "hi", "--doer", "refuser", "--wall", "20s"], None, &[]);
    assert_ne!(o.rc, 0, "{}", o.out);
    let events = log_events(&d);
    let e = events.iter().find(|e| e["kind"] == "run.error").unwrap_or_else(|| panic!("no failure in {:?}", kinds(&events)));
    let wait = e["data"]["wait_ms"].as_u64().unwrap();
    assert_eq!(e["data"]["class"], "rate-limit");
    assert!((250_000..260_000).contains(&wait), "the program's own 4m10s (and a margin), not the table's 30m: {wait}");
    let w = windows(&d, "refuser");
    assert_eq!((w.len(), w[0]["data"]["status"].as_str(), w[0]["data"]["utilization"].as_f64()), (1, Some("rejected"), Some(1.0)));
    // a program that cannot carry on in a session cannot wait, so the task ends and says when the program works again
    assert!(o.err.contains("(resets at "), "{}", o.err);
}
