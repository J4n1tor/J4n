//! A start that needs no doer runs no doer's program. The binary is run as an outside test harness runs it:
//! no terminal, nothing on standard input, an empty home, the watch switch at its default (on). Every agent program on the
//! PATH is ours and leaves a mark in HOME when it is run; none may be, because codex (and the others) write their own home on any call.

use crate::run_cases::{Dirs, j4n};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

const AGENTS: [&str; 7] = ["claude", "codex", "gemini", "opencode", "crush", "pi", "omp"];

/// An agent program that writes `ran-<name>` into HOME whatever it is asked, and answers like a program that is there.
fn agents(d: &Dirs) -> String {
    for name in AGENTS {
        let path = d.dir.join("bin").join(name);
        fs::write(&path, format!("#!/bin/sh\necho \"{name} 1.0.0\" > \"$HOME/ran-{name}\"\necho \"{name} 1.0.0\"\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    format!("{}:{}", d.dir.join("bin").display(), std::env::var("PATH").unwrap_or_default())
}

fn marks(d: &Dirs) -> Vec<String> {
    let mut found: Vec<String> = fs::read_dir(d.home()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("ran-")).collect();
    found.sort();
    found
}

#[test]
fn a_start_with_no_terminal_and_nothing_to_say_runs_no_agent_program() {
    // the first look of a start is the quick part: a late child is waited for, and a few starts make a race a miss
    for round in 0..4 {
        let d = Dirs::new("lazy-probe-start-no-terminal");
        // the watch switch is at its default here: `Dirs` turns it off for the tests that do not mean to have it
        fs::remove_file(d.j4n_home().join("settings/no-watch.toml")).unwrap();
        let path = agents(&d);
        let out = j4n(&d, &[], Some(""), &[("PATH", path.as_str())]);
        assert_eq!(out.rc, 0, "round {round}: {}", out.err);
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(marks(&d), Vec::<String>::new(), "round {round}: a program was run for a start that asked for none");
    }
}
