//! What the shaper sees, made without a model: the last turns of the thread from the Log, the
//! files in the project, and the commands that are known to pass. At most 2,000 tokens; over it the file list is cut first, then the
//! answer heads.

use j4n_core::text::line as clip;
use crate::check::{Facts, Turn};
use j4n_core::log::Log;
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Stdio};

/// How many turns of the thread the shaper is shown, and how much of an answer.
const TURNS: usize = 3;
const HEAD: usize = 300;
/// The most paths in the file list, and the most a walk outside git looks at.
const PATHS: usize = 200;
const WALK: usize = 5_000;
/// The state is cut to this many tokens.
pub const STATE_TOKENS: usize = 2_000;
/// Commands that exited 0 in the thread, at most.
const PASSED: usize = 10;

/// The files of the project: `git ls-files`, or outside git a walk with a skip list.
fn files(root: &Path) -> Vec<String> {
    let out = j4n_core::host::keyless(&mut Command::new("git")).args(["ls-files", "-z"]).current_dir(root).stdin(Stdio::null()).stderr(Stdio::null()).output();
    if let Ok(o) = out.as_ref().map_err(|_| ()).and_then(|o| o.status.success().then_some(o).ok_or(())) {
        return String::from_utf8_lossy(&o.stdout).split('\0').filter(|p| !p.is_empty()).map(String::from).collect();
    }
    let (mut found, mut stack) = (Vec::new(), vec![root.to_path_buf()]);
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || matches!(name.as_str(), "target" | "node_modules" | "__pycache__" | "dist" | "build") {
                continue;
            }
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(rel) = path.strip_prefix(root) {
                found.push(rel.to_string_lossy().to_string());
            }
            if found.len() >= WALK {
                return found;
            }
        }
    }
    found.sort();
    found
}

/// The facts about a line in `thread`: its last turns, the project's files and the commands that are known to pass.
pub fn facts(log: &Log, thread: &str, line: &str, root: &Path, checks: &[String]) -> Facts {
    log.sync();
    let mut turns: Vec<Turn> = Vec::new();
    let mut shell: Vec<String> = Vec::new();
    let mut n = 0;
    for ev in log.read(0).filter(|e| e.thread.as_deref() == Some(thread)) {
        let text = ev.data.get("text").and_then(Json::as_str).unwrap_or_default();
        match ev.kind.as_str() {
            "chat.say" if !text.starts_with(':') && !text.starts_with('/') => {
                n += 1;
                turns.push(Turn { id: format!("t{n}"), you: clip(text, 400), answer_head: String::new(), files: Vec::new() });
            }
            "task.settled" if ev.data.get("ok").and_then(Json::as_bool) == Some(true) => {
                if let Some(t) = turns.last_mut() {
                    t.answer_head = clip(ev.data.get("answer").and_then(Json::as_str).unwrap_or_default(), HEAD);
                }
            }
            "run.changed" => {
                if let Some(t) = turns.last_mut() {
                    for p in ev.data.get("paths").and_then(Json::as_array).into_iter().flatten().filter_map(Json::as_str) {
                        if !t.files.iter().any(|f| f == p) {
                            t.files.push(p.to_string());
                        }
                    }
                }
            }
            "run.shell" if ev.data.get("exit").and_then(Json::as_i64) == Some(0) => {
                if let Some(c) = ev.data.get("cmd").and_then(Json::as_str) {
                    shell.retain(|s| s != c);
                    shell.push(c.to_string());
                }
            }
            _ => {}
        }
    }
    let from = turns.len().saturating_sub(TURNS);
    let turns = turns.split_off(from);
    let files_changed: Vec<String> = turns.iter().flat_map(|t| t.files.iter().cloned()).collect::<BTreeSet<_>>().into_iter().collect();
    // recently changed first, then the rest
    let mut repo = files_changed.clone();
    repo.extend(files(root).into_iter().filter(|p| !files_changed.contains(p)));
    repo.truncate(PATHS);
    let mut passed: Vec<String> = checks.to_vec();
    passed.extend(shell.into_iter().rev().take(PASSED).filter(|c| !checks.contains(c)));
    Facts { line: line.to_string(), turns, files_changed, repo, checks: passed, root: root.to_path_buf() }
}

/// The state as JSON text for the shaper, and how many tokens it came to.
pub fn render(f: &Facts, target: &str, received: bool, words: &str, style: &str, tone: &str, format: &str) -> (String, usize) {
    let mut facts = f.clone();
    loop {
        let turns: Vec<Json> = facts.turns.iter().map(|t| json!({ "id": t.id, "you": t.you, "answer_head": t.answer_head, "files": t.files })).collect();
        let state = json!({
            "line": facts.line,
            "target": { "doer": target, "received_turns": received },
            "turns": turns,
            "files_changed": facts.files_changed,
            "repo_map": facts.repo,
            "checks": facts.checks,
            "words": words, "style": style, "tone": tone, "format": format,
        });
        let text = state.to_string();
        let tokens = text.chars().count().div_ceil(4);
        if tokens <= STATE_TOKENS || (facts.repo.is_empty() && facts.turns.iter().all(|t| t.answer_head.is_empty())) {
            return (text, tokens);
        }
        if !facts.repo.is_empty() {
            let keep = facts.repo.len() / 2;
            facts.repo.truncate(keep);
        } else if let Some(t) = facts.turns.iter_mut().find(|t| !t.answer_head.is_empty()) {
            t.answer_head.clear();
        }
    }
}
