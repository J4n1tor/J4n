//! What `scripts/t` promises about the environment, one test each. They pass only under that runner and are run
//! by `scripts/selftest-t`, by exact name: `writes_into_home` passes by itself and is the one the runner must turn
//! into a failed run, and it would dirty the home `home_starts_empty` looks at; `leaves_a_read_only_directory_in_tmpdir`
//! passes by itself too and is the one the run directory must not survive.

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

const AGENT_CLIS: [&str; 8] = ["claude", "omp", "codex", "opencode", "pi", "gemini", "msb", "j4n"];
/// Variables `scripts/selftest-t` exports into the runner; none may reach the run.
const CANARIES: [&str; 6] = [
    "SSH_AUTH_SOCK",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "ANTHROPIC_API_KEY",
    "CLAUDE_CONFIG_DIR",
    "OPENROUTER_API_KEY",
];
const CAP_BYTES: u64 = 5 << 30;
const DEAD_PROXY: &str = "http://127.0.0.1:9";
const PROXY_VARS: [&str; 6] = ["https_proxy", "http_proxy", "all_proxy", "HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY"];

fn dir_var(name: &str) -> PathBuf {
    PathBuf::from(env::var_os(name).unwrap_or_else(|| panic!("{name} is not set")))
}

#[test]
fn writes_into_home() {
    fs::write(dir_var("HOME").join("stray"), "x").expect("write into HOME");
}

#[test]
fn writes_only_into_tmpdir() {
    let file = dir_var("TMPDIR").join("hermetic-fixture");
    fs::write(&file, "x").expect("write into TMPDIR");
    fs::remove_file(&file).expect("remove it again");
}

#[test]
fn leaves_a_read_only_directory_in_tmpdir() {
    let dir = dir_var("TMPDIR").join("read-only");
    fs::create_dir(&dir).expect("make a directory in TMPDIR");
    fs::write(dir.join("stray"), "x").expect("write into it");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).expect("make it read-only");
}

#[test]
fn tmpdir_sits_next_to_home() {
    let (home, tmp) = (dir_var("HOME"), dir_var("TMPDIR"));
    assert!(
        home.parent().is_some() && home.parent() == tmp.parent() && home != tmp,
        "TMPDIR {} is not a sibling of HOME {}",
        tmp.display(),
        home.display()
    );
}

#[test]
fn inherited_variables_are_gone() {
    let found: Vec<&str> = CANARIES.into_iter().filter(|name| env::var_os(name).is_some()).collect();
    assert!(found.is_empty(), "variables of the caller reached the run: {}", found.join(", "));
}

#[test]
fn the_run_says_it_is_hermetic() {
    assert_eq!(env::var("J4N_HERMETIC").ok().as_deref(), Some("1"));
}

#[test]
fn a_capped_run_has_at_most_5_gib() {
    match env::var("J4N_CAP").ok().as_deref() {
        Some("off") => return,
        Some("on") => {}
        other => panic!("J4N_CAP is {other:?}, not on or off"),
    }
    let cgroups = fs::read_to_string("/proc/self/cgroup").expect("read /proc/self/cgroup");
    let path = cgroups.lines().find_map(|line| line.strip_prefix("0::")).expect("a cgroup v2 line");
    let file = format!("/sys/fs/cgroup{path}/memory.max");
    let max = fs::read_to_string(&file).unwrap_or_else(|e| panic!("read {file}: {e}"));
    let bytes: u64 = max.trim().parse().unwrap_or_else(|_| panic!("{file} says {:?}: no cap", max.trim()));
    assert!(bytes <= CAP_BYTES, "{file} says {bytes}, more than {CAP_BYTES}");
}

#[test]
fn home_starts_empty() {
    let home = dir_var("HOME");
    let entries: Vec<_> = fs::read_dir(&home).expect("read HOME").flatten().map(|entry| entry.file_name()).collect();
    assert!(entries.is_empty(), "HOME {} is not empty: {entries:?}", home.display());
}

#[test]
fn agent_clis_are_not_on_the_path() {
    let path = env::var_os("PATH").expect("PATH is not set");
    let found: Vec<String> = env::split_paths(&path)
        .flat_map(|dir| {
            AGENT_CLIS
                .iter()
                .filter(|cli| fs::symlink_metadata(dir.join(cli)).is_ok())
                .map(|cli| format!("{cli} in {}", dir.display()))
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(found.is_empty(), "agent CLIs on PATH: {}", found.join(", "));
}

#[test]
fn the_proxy_is_dead() {
    for name in PROXY_VARS {
        assert_eq!(env::var(name).ok().as_deref(), Some(DEAD_PROXY), "{name}");
    }
}
