//! The `cmd` patterns: a command over the changed files whose output says where it found something. A
//! program that is not installed is skipped with the reason, never counted as clean. `qc-deps` is J4n's own: the dependencies a
//! manifest has now and did not have before.

use crate::change::Change;
use crate::pattern::Ready;
use crate::scan::{Mark, files_of};
use regex::Regex;
use std::collections::BTreeSet;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CLOCK: Duration = Duration::from_secs(30);

/// The marks of one `cmd` pattern, or why it was skipped.
pub fn run(r: &Ready, change: &Change) -> Result<Vec<Mark>, String> {
    let files = files_of(r, change);
    if files.is_empty() {
        return Ok(Vec::new());
    }
    if r.p.argv.first().map(String::as_str) == Some("qc-deps") {
        return Ok(deps(r, change));
    }
    let mut argv: Vec<String> = Vec::new();
    for a in &r.p.argv {
        if a == "{files}" {
            argv.extend(files.iter().map(|f| f.path.clone()));
        } else {
            argv.push(a.clone());
        }
    }
    let (program, args) = argv.split_first().ok_or("it has no command")?;
    let mut cmd = Command::new(program);
    j4n_connect::proc::scrub(&mut cmd);
    cmd.args(args).current_dir(&change.dir).env_clear().stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for key in ["PATH", "HOME", "LANG", "RUSTUP_HOME", "CARGO_HOME"] {
        if let Ok(v) = std::env::var(key) {
            cmd.env(key, v);
        }
    }
    let mut child = j4n_connect::proc::start(&mut cmd).map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { format!("`{program}` is not installed") } else { format!("`{program}` could not be run: {e}") })?;
    let (mut out, mut err) = (child.stdout.take(), child.stderr.take());
    let reader = std::thread::spawn(move || {
        let (mut a, mut b) = (String::new(), String::new());
        if let Some(o) = out.as_mut() {
            let _ = o.read_to_string(&mut a);
        }
        if let Some(e) = err.as_mut() {
            let _ = e.read_to_string(&mut b);
        }
        (a, b)
    });
    let until = Instant::now() + CLOCK;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("`{program}` did not finish in {} s", CLOCK.as_secs()));
            }
        }
    };
    let (stdout, stderr) = reader.join().unwrap_or_default();
    if status.success() {
        return Ok(Vec::new());
    }
    let said = format!("{stdout}\n{stderr}");
    let mut marks: Vec<Mark> = Vec::new();
    for d in differences(&said, &change.dir.to_string_lossy()) {
        let Some(f) = files.iter().find(|f| f.path == d.path) else { continue };
        let Some(line) = d.lines.iter().copied().find(|n| f.added().any(|l| l.no == *n)) else { continue };
        if !marks.iter().any(|m| m.file == f.path && m.line == line) {
            marks.push(Mark::at(r, &f.path, line));
        }
    }
    if marks.is_empty() && !said.contains("Diff in") {
        return Err(format!("`{program}` failed and did not say where: {}", said.trim().lines().next().unwrap_or("no output")));
    }
    Ok(marks)
}

/// One hunk of a formatter's diff: the file, and the lines of it the hunk changes, first to last.
struct Difference {
    path: String,
    lines: Vec<u32>,
}

/// The hunks of rustfmt's `--check` output (`Diff in <file>:<n>:` and the diff lines under it, `n` being the first line shown, which
/// is context: rustfmt shows three lines before the first difference). The lines a hunk changes are the ones it takes out (`-`), and
/// for a run of lines only put in (`+`) the line they go in front of. A hunk with nothing under it names the line it says.
fn differences(said: &str, dir: &str) -> Vec<Difference> {
    let Ok(at) = Regex::new(r"^Diff in (.+?)(?::| at line )(\d+):?\s*$") else { return Vec::new() };
    let mut found: Vec<(Difference, u32)> = Vec::new();
    let (mut old, mut open, mut after_change) = (0u32, false, false);
    for text in said.lines() {
        if let Some(c) = at.captures(text) {
            let path = c[1].trim().trim_start_matches("./");
            let path = path.strip_prefix(&format!("{dir}/")).unwrap_or(path);
            old = c[2].parse().unwrap_or(1);
            found.push((Difference { path: path.to_string(), lines: Vec::new() }, old));
            (open, after_change) = (true, false);
            continue;
        }
        let Some((d, _)) = found.last_mut().filter(|_| open) else { continue };
        match text.chars().next() {
            Some('-') => {
                d.lines.push(old);
                old += 1;
                after_change = true;
            }
            Some('+') => {
                if !after_change {
                    d.lines.push(old);
                }
                after_change = true;
            }
            Some(' ') | None => {
                old += 1;
                after_change = false;
            }
            Some(_) => open = false,
        }
    }
    found
        .into_iter()
        .map(|(mut d, start)| {
            if d.lines.is_empty() {
                d.lines.push(start);
            }
            d
        })
        .collect()
}

// ---- qc-deps ----

/// The names of the dependencies a manifest declares.
fn names(path: &str, text: &str) -> BTreeSet<String> {
    let base = path.rsplit('/').next().unwrap_or(path);
    let mut set = BTreeSet::new();
    let word = |req: &str| req.trim().split(|c: char| "=<>!~[;@ ".contains(c)).next().unwrap_or_default().to_string();
    match base {
        "Cargo.toml" | "pyproject.toml" => {
            let Ok(doc) = text.parse::<toml::Table>() else { return set };
            let tables = dependency_tables(&doc);
            for t in tables {
                set.extend(t.keys().filter(|k| k.as_str() != "python").cloned());
            }
            let project = doc.get("project");
            for v in project.and_then(|p| p.get("dependencies")).and_then(toml::Value::as_array).into_iter().flatten().filter_map(toml::Value::as_str) {
                set.insert(word(v));
            }
            if let Some(extra) = project.and_then(|p| p.get("optional-dependencies")).and_then(toml::Value::as_table) {
                for v in extra.values().filter_map(toml::Value::as_array).flatten().filter_map(toml::Value::as_str) {
                    set.insert(word(v));
                }
            }
        }
        "package.json" => {
            let Ok(doc) = serde_json::from_str::<serde_json::Value>(text) else { return set };
            for k in ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"] {
                if let Some(o) = doc.get(k).and_then(serde_json::Value::as_object) {
                    set.extend(o.keys().cloned());
                }
            }
        }
        "go.mod" => {
            let mut block = false;
            for line in text.lines().map(str::trim) {
                if line.starts_with("require (") {
                    block = true;
                } else if line == ")" {
                    block = false;
                } else if let Some(rest) = line.strip_prefix("require ") {
                    set.insert(word(rest));
                } else if block && !line.is_empty() && !line.starts_with("//") {
                    set.insert(word(line));
                }
            }
        }
        _ if base.starts_with("requirements") => {
            for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('-')) {
                set.insert(word(line));
            }
        }
        _ => {}
    }
    set.retain(|n| !n.is_empty());
    set
}

/// The tables of a manifest that list dependencies, wherever they stand.
fn dependency_tables(doc: &toml::Table) -> Vec<&toml::Table> {
    const KINDS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];
    fn add<'a>(found: &mut Vec<&'a toml::Table>, v: Option<&'a toml::Value>) {
        found.extend(v.and_then(toml::Value::as_table));
    }
    let mut found: Vec<&toml::Table> = Vec::new();
    for k in KINDS {
        add(&mut found, doc.get(k));
    }
    add(&mut found, doc.get("workspace").and_then(|w| w.get("dependencies")));
    let poetry = doc.get("tool").and_then(|t| t.get("poetry"));
    add(&mut found, poetry.and_then(|p| p.get("dependencies")));
    add(&mut found, poetry.and_then(|p| p.get("dev-dependencies")));
    for t in doc.get("target").and_then(toml::Value::as_table).into_iter().flat_map(toml::map::Map::values) {
        for k in KINDS {
            add(&mut found, t.get(k));
        }
    }
    found
}

/// A mark for each dependency a manifest has now and did not have before, at the line that names it.
fn deps(r: &Ready, change: &Change) -> Vec<Mark> {
    let mut marks = Vec::new();
    for f in files_of(r, change) {
        let Some(now) = change.after(&f.path) else { continue };
        let was = change.before(&f.path).map(|t| names(&f.path, &t)).unwrap_or_default();
        for name in names(&f.path, &now).difference(&was) {
            let line = now.lines().position(|l| l.contains(name.as_str())).map_or(1, |i| i as u32 + 1);
            marks.push(Mark::at(r, &f.path, line));
        }
    }
    marks
}
