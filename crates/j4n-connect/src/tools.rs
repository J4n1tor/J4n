//! The builtin tools (`dialect = "builtin"`): `list`, `read`, `grep`, `edit`,
//! `write`, `run` and `git`, the same set on every turn whatever the mode — a read-only task keeps its tools, and the Gate
//! blocks or asks when one of them would change something.
//!
//! - **Paths** are the project's own: a relative one, or an absolute one that lies inside the folder the task works in (task
//!   instructions name files that way). `..`, `.git` and `.env*` are refused whatever the call, and so is a path that a
//!   link leads out of the folder. The folder is the task's [`Site`], so a worktree's tools work in the worktree.
//! - **Every effect goes by the Gate.** [`Builtin::gate`] asks it first; a call it allows runs at once (no question is raised for
//!   a read), and one it asks about or blocks goes to the loop as an approval, whose answer is the call's result.
//! - **A command** (`run`) goes through the place's wrap (a box puts it behind its walls), has a time limit, and comes back with
//!   its real exit code and the end of what it wrote: the model sees them, and so does the Log (`RunEv::Shell`).
//! - Nothing that can be long is unbounded: lines, entries, hits, bytes and a command's output are all cut, and the cut is said.

use j4n_core::text::clip;
use crate::model::Tool;
use crate::model::doer::{Gated, ShellOut, ToolOut, Tools, action};
use crate::proc;
use j4n_core::gate::{Action, Gate, ReadOnly, Verb, Verdict};
use j4n_core::place::{Launch, Site};
use serde_json::{Value as Json, json};
use std::fs;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const READ_LINES: usize = 400;
pub const GOES_ON: &str = "(the file goes on after line ";
const OUT_CHARS: usize = 12_000;
const KEEP_BYTES: usize = 64 << 10;
const GREP_HITS: usize = 200;
const LIST_ENTRIES: usize = 500;
const LIST_DEPTH: u64 = 3;
const WALK_FILES: usize = 20_000;
const SCAN_BYTES: u64 = 2 << 20;
const WRITE_BYTES: usize = 200_000;
const RUN_SECS: u64 = 120;
const RUN_SECS_MAX: u64 = 600;
const LINE_CHARS: usize = 300;
const SKIPPED: [&str; 7] = [".git", ".j4n", "target", "node_modules", ".venv", "__pycache__", ".hg"];

/// The tools of `dialect = "builtin"`. With a Gate, a call it allows runs without a question; without one, everything that is
/// not a plain read is asked about.
#[derive(Clone, Default)]
pub struct Builtin {
    gate: Option<Gate>,
}

impl Builtin {
    pub fn new() -> Builtin {
        Builtin::default()
    }

    pub fn with_gate(mut self, gate: Gate) -> Builtin {
        self.gate = Some(gate);
        self
    }
}

fn tool(name: &str, doc: &str, props: Json, required: &[&str]) -> Tool {
    Tool { name: name.into(), doc: doc.into(), schema: json!({ "type": "object", "properties": props, "required": required }) }
}

/// The paths a call names, which the Gate is asked about.
fn arg<'a>(call: &'a crate::model::Call, name: &str) -> Option<&'a str> {
    call.args.get(name).and_then(Json::as_str)
}

/// The path of a path, lexically: `.` dropped, a `..` refused.
fn clean(rel: &str) -> Result<PathBuf, String> {
    let mut out = PathBuf::new();
    for c in Path::new(rel).components() {
        match c {
            Component::Normal(n) => out.push(n),
            Component::CurDir | Component::RootDir => {}
            Component::ParentDir => return Err(format!("`{rel}` goes up with `..`: name the path from the project's folder")),
            Component::Prefix(_) => return Err(format!("`{rel}` is not a path in this project")),
        }
    }
    Ok(out)
}

fn secret_name(n: &str) -> bool {
    n == ".git" || n.starts_with(".env")
}

/// Where `name` is, inside `root`: the path relative to it (to be shown and asked about) and the real one. A path that is
/// absolute must lie inside `root`. `.git` and `.env*` are never reached, and nothing is reached through a link that
/// leads out of the folder. `create` allows the last parts to be missing.
pub fn confine(root: &Path, name: &str, create: bool) -> Result<(String, PathBuf), String> {
    if name.is_empty() || name.contains('\0') {
        return Err("a path is needed".into());
    }
    let real_root = root.canonicalize().map_err(|e| format!("the project's folder is not there: {e}"))?;
    let rel = if Path::new(name).is_absolute() {
        let p = Path::new(name);
        let inside = p.strip_prefix(root).or_else(|_| p.strip_prefix(&real_root)).map_err(|_| format!("`{name}` is outside the project ({})", root.display()))?;
        clean(&inside.to_string_lossy())?
    } else {
        clean(name)?
    };
    if let Some(bad) = rel.components().find(|c| matches!(c, Component::Normal(n) if secret_name(&n.to_string_lossy()))) {
        return Err(format!("`{}` is not reached by a tool (git's own files and environment files are the operator's)", bad.as_os_str().to_string_lossy()));
    }
    let joined = real_root.join(&rel);
    // the part that exists must resolve to somewhere inside the folder
    let mut probe = joined.as_path();
    let existing = loop {
        if probe.symlink_metadata().is_ok() {
            break probe;
        }
        match probe.parent() {
            Some(up) if create && up.starts_with(&real_root) => probe = up,
            _ => return Err(format!("`{}` is not there", rel.display())),
        }
    };
    let resolved = existing.canonicalize().map_err(|e| format!("`{}`: {e}", rel.display()))?;
    if !resolved.starts_with(&real_root) {
        return Err(format!("`{}` leads outside the project", rel.display()));
    }
    Ok((if rel.as_os_str().is_empty() { ".".into() } else { rel.display().to_string() }, joined))
}

fn tail(s: &str, n: usize) -> String {
    let count = s.chars().count();
    if count <= n { s.to_string() } else { format!("…{}", s.chars().skip(count - n).collect::<String>()) }
}

fn ok(text: String) -> ToolOut {
    ToolOut { text, ..ToolOut::default() }
}

fn bad(text: impl Into<String>) -> ToolOut {
    ToolOut { text: text.into(), error: true, ..ToolOut::default() }
}

fn text_of(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("cannot read it: {e}"))?;
    if bytes.iter().take(8192).any(|b| *b == 0) {
        return Err("it is a binary file".into());
    }
    String::from_utf8(bytes).map_err(|_| "it is not text (not UTF-8)".into())
}

/// Write a file whole, so no reader sees half of it; the mode of a file that was there stays.
fn put(target: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = target.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("cannot make the folder: {e}"))?;
    }
    let mode = fs::metadata(target).ok().map(|m| m.permissions());
    let tmp = target.with_extension(format!("j4n-{}.tmp", std::process::id()));
    let mut f = fs::File::create(&tmp).map_err(|e| format!("cannot write: {e}"))?;
    f.write_all(bytes).and_then(|()| f.sync_all()).map_err(|e| format!("cannot write: {e}"))?;
    drop(f);
    if let Some(m) = mode {
        let _ = fs::set_permissions(&tmp, m);
    }
    fs::rename(&tmp, target).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("cannot write: {e}")
    })
}

/// The most files one `read` call takes, and the most lines it gives back in all: small files in one call, not a way round the 400 lines of one.
const READ_FILES: usize = 8;
const READ_TOTAL: usize = 800;

/// The paths a `read` is asked for: `path`, then `paths`.
fn read_names(call: &crate::model::Call) -> Vec<&str> {
    arg(call, "path").into_iter().chain(call.args.get("paths").and_then(Json::as_array).into_iter().flatten().filter_map(Json::as_str)).collect()
}

fn read(root: &Path, call: &crate::model::Call) -> ToolOut {
    let names = read_names(call);
    if names.is_empty() {
        return bad("`read` needs a `path` or `paths`");
    }
    if call.args.get("paths").is_none() {
        return read_one(root, names[0], call);
    }
    // several files: each under its own header, each as one file is read; a file that cannot be read says so under its header and the call goes on
    let (mut out, mut left, mut failed) = (String::new(), READ_TOTAL, 0);
    for (i, name) in names.iter().enumerate() {
        if i >= READ_FILES || left == 0 {
            out.push_str(&format!("=== {name} ===\nnot read: one call reads {READ_FILES} files and {READ_TOTAL} lines at most; ask again for this one\n"));
            continue;
        }
        let one = read_one(root, name, call);
        failed += usize::from(one.error);
        let text: Vec<&str> = one.text.lines().collect();
        let shown = if text.len() > left { format!("{}\n{GOES_ON}{left}; read this file again with offset {})\n", text[..left].join("\n"), left + 1) } else { format!("{}\n", one.text.trim_end()) };
        left = left.saturating_sub(text.len());
        out.push_str(&format!("=== {name} ===\n{shown}"));
    }
    ToolOut { text: out, error: failed == names.len(), ..ToolOut::default() }
}

fn read_one(root: &Path, name: &str, call: &crate::model::Call) -> ToolOut {
    let (rel, path) = match confine(root, name, false) {
        Ok(p) => p,
        Err(e) => return bad(e),
    };
    if path.is_dir() {
        return bad(format!("`{rel}` is a folder: use `list`"));
    }
    let text = match text_of(&path) {
        Ok(t) => t,
        Err(e) => return bad(format!("`{rel}`: {e}")),
    };
    let from = call.args.get("offset").and_then(Json::as_u64).unwrap_or(1).max(1) as usize;
    let count = call.args.get("limit").and_then(Json::as_u64).map_or(READ_LINES, |n| (n as usize).clamp(1, READ_LINES));
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return ok(format!("`{rel}` is empty"));
    }
    if from > lines.len() {
        return bad(format!("`{rel}` has {} lines; there is nothing at line {from}", lines.len()));
    }
    let end = (from - 1 + count).min(lines.len());
    let mut out: String = lines[from - 1..end].iter().enumerate().map(|(i, l)| format!("{:>5}  {}\n", from + i, clip(l, 2000))).collect();
    if end < lines.len() {
        out.push_str(&format!("{GOES_ON}{end}; read again with offset {})\n", end + 1));
    }
    ok(out)
}

fn skipped(n: &str) -> bool {
    SKIPPED.contains(&n) || secret_name(n)
}

/// The files and folders under `dir`, `depth` deep, as paths relative to `root`; a folder ends in `/`.
fn walk(root: &Path, dir: &Path, depth: u64, out: &mut Vec<String>, files_only: bool, budget: &mut usize) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        if *budget == 0 {
            return;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if skipped(&name) {
            continue;
        }
        let path = e.path();
        let Ok(kind) = e.file_type() else { continue };
        let rel = path.strip_prefix(root).unwrap_or(&path).display().to_string();
        if kind.is_dir() {
            if !files_only {
                out.push(format!("{rel}/"));
            }
            *budget -= 1;
            if depth > 1 {
                walk(root, &path, depth - 1, out, files_only, budget);
            }
        } else if kind.is_file() {
            out.push(rel);
            *budget -= 1;
        }
    }
}

fn list(root: &Path, call: &crate::model::Call) -> ToolOut {
    let name = arg(call, "path").unwrap_or(".");
    let (rel, path) = match confine(root, if name.is_empty() { "." } else { name }, false) {
        Ok(p) => p,
        Err(e) => return bad(e),
    };
    if !path.is_dir() {
        return bad(format!("`{rel}` is not a folder"));
    }
    let depth = call.args.get("depth").and_then(Json::as_u64).unwrap_or(1).clamp(1, LIST_DEPTH);
    let (mut out, mut budget) = (Vec::new(), WALK_FILES);
    let real = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    walk(&real, &path, depth, &mut out, false, &mut budget);
    if out.is_empty() {
        return ok(format!("`{rel}` is empty"));
    }
    let more = out.len().saturating_sub(LIST_ENTRIES);
    out.truncate(LIST_ENTRIES);
    let mut text = out.join("\n");
    if more > 0 {
        text.push_str(&format!("\n({more} more: name a deeper folder)"));
    }
    ok(text)
}

fn grep(root: &Path, call: &crate::model::Call) -> ToolOut {
    let Some(pattern) = arg(call, "pattern").filter(|p| !p.is_empty()) else { return bad("`grep` needs a `pattern`") };
    let fold = call.args.get("ignore_case").and_then(Json::as_bool).unwrap_or(false);
    let needle = if fold { pattern.to_lowercase() } else { pattern.to_string() };
    let (rel, start) = match confine(root, arg(call, "path").filter(|p| !p.is_empty()).unwrap_or("."), false) {
        Ok(p) => p,
        Err(e) => return bad(e),
    };
    let real = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut files = Vec::new();
    if start.is_dir() {
        let mut budget = WALK_FILES;
        walk(&real, &start, 64, &mut files, true, &mut budget);
    } else {
        files.push(start.strip_prefix(&real).unwrap_or(&start).display().to_string());
    }
    let (mut hits, mut more) = (Vec::new(), 0usize);
    for f in files {
        let path = real.join(&f);
        if fs::metadata(&path).is_ok_and(|m| m.len() > SCAN_BYTES) {
            continue;
        }
        let Ok(text) = text_of(&path) else { continue };
        for (i, line) in text.lines().enumerate() {
            let hit = if fold { line.to_lowercase().contains(&needle) } else { line.contains(&needle) };
            if hit {
                if hits.len() < GREP_HITS {
                    hits.push(format!("{f}:{}:{}", i + 1, clip(line.trim(), LINE_CHARS)));
                } else {
                    more += 1;
                }
            }
        }
    }
    if hits.is_empty() {
        return ok(format!("no line in `{rel}` holds `{pattern}`"));
    }
    let mut text = hits.join("\n");
    if more > 0 {
        text.push_str(&format!("\n({more} more hits: narrow the path or the pattern)"));
    }
    ok(text)
}

fn edit(root: &Path, call: &crate::model::Call) -> ToolOut {
    let (Some(name), Some(old), Some(new)) = (arg(call, "path"), arg(call, "old"), arg(call, "new")) else {
        return bad("`edit` needs `path`, `old` (the exact text to replace) and `new`");
    };
    let (rel, path) = match confine(root, name, false) {
        Ok(p) => p,
        Err(e) => return bad(e),
    };
    if old.is_empty() {
        return bad("`old` is empty: use `write` to make a file");
    }
    if fs::metadata(&path).is_ok_and(|m| m.len() > SCAN_BYTES) {
        return bad(format!("`{rel}` is too large to edit"));
    }
    let text = match text_of(&path) {
        Ok(t) => t,
        Err(e) => return bad(format!("`{rel}`: {e}")),
    };
    let all = call.args.get("all").and_then(Json::as_bool).unwrap_or(false);
    let n = text.matches(old).count();
    if n == 0 {
        return bad(format!("`old` is not in `{rel}`: read the file again and copy the text exactly"));
    }
    if n > 1 && !all {
        return bad(format!("`old` is in `{rel}` {n} times: add the lines around it so it is one place, or set `all`"));
    }
    let line = text[..text.find(old).unwrap_or(0)].matches('\n').count() + 1;
    let changed = if all { text.replace(old, new) } else { text.replacen(old, new, 1) };
    if let Err(e) = put(&path, changed.as_bytes()) {
        return bad(format!("`{rel}`: {e}"));
    }
    ToolOut { text: format!("edited `{rel}` at line {line}{}", if all && n > 1 { format!(" ({n} places)") } else { String::new() }), error: false, changed: vec![rel], shell: None, py: None }
}

fn write(root: &Path, call: &crate::model::Call) -> ToolOut {
    let (Some(name), Some(content)) = (arg(call, "path"), arg(call, "content")) else { return bad("`write` needs `path` and `content`") };
    if content.len() > WRITE_BYTES {
        return bad(format!("`content` is {} bytes; write a file this long in pieces (at most {WRITE_BYTES} at once)", content.len()));
    }
    let (rel, path) = match confine(root, name, true) {
        Ok(p) => p,
        Err(e) => return bad(e),
    };
    if path.is_dir() {
        return bad(format!("`{rel}` is a folder"));
    }
    let existed = path.exists();
    if let Err(e) = put(&path, content.as_bytes()) {
        return bad(format!("`{rel}`: {e}"));
    }
    ToolOut { text: format!("{} `{rel}` ({} bytes)", if existed { "replaced" } else { "made" }, content.len()), error: false, changed: vec![rel], shell: None, py: None }
}

/// `sh -c` over `cmd`, both streams in one, in `launch`'s folder: what it wrote at the end, and how it ended.
fn shell(launch: Launch, cmd: &str, secs: u64) -> ToolOut {
    let kept = Arc::new(Mutex::new(String::new()));
    let (tx, rx) = mpsc::channel();
    let sink = kept.clone();
    let spawned = proc::spawn(
        &launch,
        false,
        move |l| {
            let mut s = sink.lock().unwrap_or_else(|e| e.into_inner());
            s.push_str(l);
            s.push('\n');
            if s.len() > 2 * KEEP_BYTES {
                let cut = s.len() - KEEP_BYTES;
                let at = (cut..s.len()).find(|i| s.is_char_boundary(*i)).unwrap_or(s.len());
                s.drain(..at);
            }
        },
        move |e| drop(tx.send(e)),
    );
    let (child, _) = match spawned {
        Ok(c) => c,
        Err(e) => return bad(format!("could not start the command: {e}")),
    };
    let (exit, timed_out) = match rx.recv_timeout(Duration::from_secs(secs)) {
        Ok(e) => (e, false),
        Err(_) => {
            child.kill();
            match rx.recv() {
                Ok(e) => (e, true),
                Err(_) => return bad("the command's end was lost"),
            }
        }
    };
    let out = tail(kept.lock().unwrap_or_else(|e| e.into_inner()).trim_end(), OUT_CHARS);
    let code = exit.code.unwrap_or(-1);
    let how = if timed_out { format!("it was stopped after {secs} s") } else { exit.how() };
    let mut text = format!("{how}\n{out}");
    if timed_out {
        text.push_str("\n(a command is given up to 600 s with `timeout`; run a long one in the background and look at its output)");
    }
    ToolOut { text, error: timed_out, changed: Vec::new(), shell: Some(ShellOut { cmd: cmd.to_string(), out, exit: code }), py: None }
}

/// How long a command is given: what it asked for, between a second and ten minutes.
pub fn run_limit(asked: Option<u64>) -> u64 {
    asked.unwrap_or(RUN_SECS).clamp(1, RUN_SECS_MAX)
}

fn run(site: &Site, wrap: &(dyn Fn(Launch) -> Launch + Send + Sync), call: &crate::model::Call) -> ToolOut {
    let Some(cmd) = arg(call, "cmd").filter(|c| !c.trim().is_empty()) else { return bad("`run` needs a `cmd`") };
    let secs = run_limit(call.args.get("timeout").and_then(Json::as_u64));
    let argv = vec!["sh".to_string(), "-c".into(), format!("exec 2>&1\n{cmd}")];
    // what it inherits is J4n's environment as the scrub leaves it: a copy of it here would put back what the scrub takes out
    shell(wrap(Launch::new(argv, site.dir.clone(), Default::default())), cmd, secs)
}

/// The fixed command lines of `git`: what a model may look at, and nothing that writes.
fn git_argv(call: &crate::model::Call) -> Result<Vec<String>, String> {
    let word = |s: &str| !s.is_empty() && !s.starts_with('-') && s.chars().all(|c| c.is_ascii_alphanumeric() || "_./~^@-".contains(c));
    let mut v: Vec<String> = vec!["git".into(), "--no-pager".into()];
    match arg(call, "op").unwrap_or_default() {
        "status" => v.extend(["status", "--short", "--branch"].map(String::from)),
        "diff" => v.extend(["diff", "--stat", "-p"].map(String::from)),
        "log" => v.extend(["log", "--oneline", "-n", "20"].map(String::from)),
        "show" => {
            let rev = arg(call, "rev").filter(|r| word(r)).ok_or("`git show` needs a `rev` (a commit, a branch, a tag)")?;
            v.extend(["show", "--stat", "-p", rev].map(String::from));
        }
        other => return Err(format!("`git` takes `op` of status, diff, log or show, not `{other}`; use `run` for anything else")),
    }
    if matches!(arg(call, "op"), Some("diff" | "log"))
        && let Some(p) = arg(call, "path").filter(|p| !p.is_empty())
    {
        v.extend(["--".into(), p.to_string()]);
    }
    Ok(v)
}

fn git(site: &Site, wrap: &(dyn Fn(Launch) -> Launch + Send + Sync), call: &crate::model::Call) -> ToolOut {
    let argv = match git_argv(call) {
        Ok(a) => a,
        Err(e) => return bad(e),
    };
    let cmd = argv[2..].join(" ");
    let mut launch = Launch::new(argv, site.dir.clone(), Default::default());
    launch.env.insert("GIT_TERMINAL_PROMPT".into(), "0".into());
    launch.env.insert("GIT_OPTIONAL_LOCKS".into(), "0".into());
    let mut out = shell_plain(wrap(launch));
    if out.text.trim().is_empty() {
        out.text = format!("git {cmd}: nothing to show");
    }
    out
}

/// A program run to its end, its output kept like a command's, without the `exec 2>&1` of a shell line. `shown` is the command as
/// it is said in the Log; the program's own words are the answer and its exit is only worth saying when it failed.
pub(crate) fn plain(launch: Launch, shown: &str, secs: u64) -> ToolOut {
    let mut argv = vec!["sh".to_string(), "-c".into(), "exec 2>&1; exec \"$@\"".into(), "sh".into()];
    argv.extend(launch.argv.iter().cloned());
    let mut out = shell(Launch { argv, ..launch }, shown, secs);
    if let Some(s) = out.shell.take() {
        out.text = if s.exit == 0 { s.out } else { format!("exit {}\n{}", s.exit, s.out) };
        out.error = s.exit != 0 || out.error;
        out.shell = Some(ShellOut { exit: s.exit, ..ShellOut { cmd: shown.to_string(), out: out.text.clone(), exit: 0 } });
    }
    out
}

fn shell_plain(launch: Launch) -> ToolOut {
    let shown = format!("git {}", launch.argv.get(2..).map(|a| a.join(" ")).unwrap_or_default());
    let mut out = plain(launch, &shown, 30);
    out.shell = None;
    out
}

/// Programs a `run` line reads the outside world with: what they print is somebody else's words.
const READS_OUTSIDE: [&str; 3] = ["curl", "wget", "gh"];

impl Tools for Builtin {
    fn untrusted(&self, call: &crate::model::Call) -> Option<String> {
        let line = arg(call, "cmd").filter(|_| call.name == "run")?;
        let word = line.split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_')).find(|w| READS_OUTSIDE.contains(w))?;
        Some(format!("shell:{word}"))
    }

    fn specs(&self) -> Vec<Tool> {
        vec![
            tool("list", "List a folder. depth 1-3.", json!({ "path": { "type": "string" }, "depth": { "type": "integer" } }), &[]),
            tool("read", "Read a text file with line numbers (400 lines at most). `paths`: several small files in one call.", json!({ "path": { "type": "string" }, "paths": { "type": "array", "items": { "type": "string" } }, "offset": { "type": "integer" }, "limit": { "type": "integer" } }), &[]),
            tool("grep", "Find lines holding a text, in a file or folder.", json!({ "pattern": { "type": "string" }, "path": { "type": "string" }, "ignore_case": { "type": "boolean" } }), &["pattern"]),
            tool("edit", "Replace one exact text in a file.", json!({ "path": { "type": "string" }, "old": { "type": "string" }, "new": { "type": "string" }, "all": { "type": "boolean" } }), &["path", "old", "new"]),
            tool("write", "Make or replace a file; folders are made.", json!({ "path": { "type": "string" }, "content": { "type": "string" } }), &["path", "content"]),
            tool("run", "Run a shell command; returns its exit code and output.", json!({ "cmd": { "type": "string" }, "timeout": { "type": "integer" } }), &["cmd"]),
            tool("git", "Look at git: status, diff, log, show.", json!({ "op": { "type": "string", "enum": ["status", "diff", "log", "show"] }, "path": { "type": "string" }, "rev": { "type": "string" } }), &["op"]),
        ]
    }

    fn gate(&self, call: &crate::model::Call, task: &str, site: &Site, read_only: ReadOnly) -> Option<Gated> {
        let (verb, target, question) = match call.name.as_str() {
            "run" => (Verb::Shell, arg(call, "cmd")?.to_string(), format!("run `{}`", arg(call, "cmd")?)),
            "git" => {
                let argv = git_argv(call).ok()?;
                (Verb::Shell, argv[2..].join(" "), format!("run `git {}`", argv[2..].join(" ")))
            }
            "write" | "edit" => {
                let name = arg(call, "path")?;
                let (rel, _) = confine(&site.dir, name, true).ok()?;
                (Verb::Write, rel.clone(), format!("{} `{rel}`", call.name))
            }
            // every file of a `read` is put to the Gate as a read of its own, the strongest answer is the call's; a path that is no file of the project is refused
            // by the tool itself, file by file
            "read" if call.args.get("paths").is_some() => {
                let mut worst: Option<Gated> = None;
                for name in read_names(call).into_iter().take(READ_FILES) {
                    let one = crate::model::Call { args: json!({ "path": name }), ..call.clone() };
                    if let Some(g) = self.gate(&one, task, site, read_only)
                        && worst.as_ref().is_none_or(|w| g.risk > w.risk)
                    {
                        worst = Some(g);
                    }
                }
                return worst;
            }
            "list" | "read" | "grep" => {
                let name = arg(call, "path").filter(|p| !p.is_empty()).unwrap_or(".");
                let (rel, _) = confine(&site.dir, name, false).ok()?;
                (Verb::Read, rel.clone(), format!("{} `{rel}`", call.name))
            }
            _ => return None,
        };
        let mut a: Action = action("builtin", verb, &target, task, site, read_only);
        a.inside = Some(true);
        let verdict = self.gate.as_ref().map_or_else(
            || if verb == Verb::Read { Verdict::Allow { by: "no gate".into() } } else { Verdict::Ask { risk: 40, why: "no rules are loaded".into(), by: "no gate".into() } },
            |g| g.check(&a),
        );
        match verdict {
            Verdict::Allow { .. } => None,
            Verdict::Ask { risk, .. } => Some(Gated { action: a, question, risk }),
            Verdict::Block { .. } => Some(Gated { action: a, question, risk: 100 }),
        }
    }

    fn run(&self, call: &crate::model::Call, site: &Site, wrap: &(dyn Fn(Launch) -> Launch + Send + Sync)) -> ToolOut {
        let root = site.dir.as_path();
        match call.name.as_str() {
            "list" => list(root, call),
            "read" => read(root, call),
            "grep" => grep(root, call),
            "edit" => edit(root, call),
            "write" => write(root, call),
            "run" => run(site, wrap, call),
            "git" => git(site, wrap, call),
            other => bad(format!("there is no tool `{other}`; the tools are list, read, grep, edit, write, run and git")),
        }
    }
}
