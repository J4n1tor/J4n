//! The doer `parts`: the gate of a part and its commit, each line put to the Gate before it runs, as a `tool`
//! connector's is. A commit is made only in a folder that is the top of a git work tree itself, on the branch checked out, of the paths it is
//! given and no others (`git add -- <paths>`, `git commit -m … -- <paths>`); it never pushes, and a push is blocked by `push-is-yours`.

use j4n_connect::connect::Job;
use j4n_connect::proc;
use j4n_core::gate::Verb;
use j4n_core::place::Launch;
use serde_json::Value as Json;
use std::collections::BTreeMap;
use std::path::{Component, Path};
use std::time::Duration;

pub const DOER: &str = "parts";
/// The clock of a gate, and of one git command.
const GATE_SECS: u64 = 15 * 60;
const GIT_SECS: u64 = 120;
/// How much of a gate's output says why it failed.
const TAIL: usize = 1500;

/// A word as the shell would read it back as one word: the Gate reads the line, so the line must say what runs.
pub fn quote(w: &str) -> String {
    if !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || "-_./=:@+,".contains(c)) { w.to_string() } else { format!("'{}'", w.replace('\'', "'\\''")) }
}

/// The line the Gate is asked about.
pub fn line(argv: &[String]) -> String {
    argv.iter().map(|w| quote(w)).collect::<Vec<_>>().join(" ")
}

fn tail(text: &str) -> String {
    let n = text.chars().count();
    if n <= TAIL { text.trim().to_string() } else { format!("…{}", text.chars().skip(n - TAIL).collect::<String>().trim()) }
}

/// `argv` run in the folder with a cleared environment (`PATH`, `HOME`, `LANG` only: no `GIT_DIR`, no keys), both streams in one.
fn run(job: &Job, argv: &[String], secs: u64) -> Result<(String, i32), String> {
    let mut words: Vec<String> = vec!["env".into(), "-i".into()];
    for (k, fallback) in [("PATH", "/usr/local/bin:/usr/bin:/bin"), ("HOME", "/"), ("LANG", "C.UTF-8")] {
        words.push(format!("{k}={}", std::env::var(k).unwrap_or_else(|_| fallback.into())));
    }
    words.extend(["TERM=dumb".into(), "sh".into(), "-c".into(), "exec 2>&1; exec \"$@\"".into(), "sh".into()]);
    words.extend(argv.iter().cloned());
    let launch = job.cx.wrap(Launch::new(words, job.cx.site.dir.clone(), BTreeMap::new()));
    let (text, exit) = proc::capture(&launch, Duration::from_secs(secs)).map_err(|e| format!("`{}` could not be started: {e}", line(argv)))?;
    if exit.killed {
        return Err(format!("`{}` did not end in {secs}s", line(argv)));
    }
    Ok((text, exit.code.unwrap_or(-1)))
}

/// Ask the Gate, then run; an exit other than 0 is an error with the end of the output.
fn gated(job: &Job, local: &str, argv: &[String], secs: u64) -> Result<String, String> {
    let shown = line(argv);
    job.approve_as(local, format!("run `{shown}`?"), &shown, Verb::Shell, None)?;
    match run(job, argv, secs)? {
        (text, 0) => Ok(text),
        (text, code) => Err(format!("`{shown}` failed (exit {code}):\n{}", tail(&text))),
    }
}

fn git(job: &Job, args: &[&str]) -> Result<String, String> {
    let argv: Vec<String> = std::iter::once("git").chain(args.iter().copied()).map(String::from).collect();
    match run(job, &argv, GIT_SECS)? {
        (text, 0) => Ok(text.trim().to_string()),
        (text, code) => Err(format!("`{}` exited {code}: {}", line(&argv), tail(&text))),
    }
}

/// A path the commit may name: relative, inside the folder, not git's own, not an option.
pub fn path_ok(p: &str) -> bool {
    let path = Path::new(p);
    !p.is_empty() && !p.starts_with('-') && path.components().all(|c| matches!(c, Component::Normal(_))) && !path.components().any(|c| c.as_os_str() == ".git")
}

/// The job the task was given (`input.job`: `gate` or `commit`).
pub fn work(job: &Job) -> Result<String, String> {
    let input = &job.cx.spec.input;
    let words = |k: &str| input.get(k).and_then(Json::as_array).map(|a| a.iter().filter_map(Json::as_str).map(String::from).collect::<Vec<_>>()).unwrap_or_default();
    match input.get("job").and_then(Json::as_str) {
        Some("gate") => {
            let argv = words("argv");
            if argv.is_empty() {
                return Err("a gate with no command".into());
            }
            gated(job, "gate", &argv, GATE_SECS)?;
            Ok(format!("gate `{}`: passed", line(&argv)))
        }
        Some("commit") => commit(job, &words("paths"), &words("message")),
        other => Err(format!("the doer `parts` has no job {other:?}")),
    }
}

fn commit(job: &Job, paths: &[String], message: &[String]) -> Result<String, String> {
    let dir = job.cx.site.dir.canonicalize().map_err(|e| format!("the folder cannot be read: {e}"))?;
    let top = git(job, &["rev-parse", "--show-toplevel"]).map_err(|e| format!("no commit: the folder is not in a git work tree ({e})"))?;
    if Path::new(&top).canonicalize().ok().as_deref() != Some(dir.as_path()) {
        return Err(format!("no commit: the folder is not the top of its own git work tree (that is {top}); J4n commits only the project folder's own repository"));
    }
    let branch = git(job, &["symbolic-ref", "-q", "--short", "HEAD"]).map_err(|_| "no commit: HEAD is not on a branch".to_string())?;
    if let Some(bad) = paths.iter().find(|p| !path_ok(p)) {
        return Err(format!("no commit: `{bad}` is not a path inside the folder"));
    }
    if paths.is_empty() || message.is_empty() {
        return Err("no commit: no paths, or no message".into());
    }
    // a path git ignores is not the part's to commit; `check-ignore` exits 1 when none is
    let mut ask: Vec<String> = ["git", "check-ignore", "--"].map(String::from).to_vec();
    ask.extend(paths.iter().cloned());
    let ignored: Vec<String> = match run(job, &ask, GIT_SECS)? {
        (text, 0) => text.lines().map(|l| l.trim().to_string()).collect(),
        _ => Vec::new(),
    };
    let paths: Vec<String> = paths.iter().filter(|p| !ignored.contains(p)).cloned().collect();
    let mut status: Vec<&str> = vec!["status", "--porcelain", "--"];
    status.extend(paths.iter().map(String::as_str));
    if paths.is_empty() || git(job, &status)?.is_empty() {
        return Ok(format!("nothing to commit on {branch}: the part's paths are as HEAD has them"));
    }
    let mut add: Vec<String> = ["git", "add", "--"].map(String::from).to_vec();
    add.extend(paths.iter().cloned());
    gated(job, "add", &add, GIT_SECS)?;
    let mut argv: Vec<String> = ["git", "commit"].map(String::from).to_vec();
    for m in message {
        argv.extend(["-m".to_string(), m.clone()]);
    }
    argv.push("--".into());
    argv.extend(paths.iter().cloned());
    gated(job, "commit", &argv, GIT_SECS)?;
    let sha = git(job, &["rev-parse", "HEAD"])?;
    Ok(format!("committed {sha} on {branch}: {}", paths.join(" ")))
}
