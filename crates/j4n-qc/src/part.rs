//! A change cut into parts: the unit Jev is asked about. A part is the hunks of one file that
//! together change at most 60 lines; a hunk of more is cut into windows of 60. Each part is made into a state without a model:
//! what was asked, what was delivered, the code around it, the helpers that look alike, and how the file is written.

use crate::change::{Change, git};
use crate::diff::{Hunk, Line};
use crate::scan::Mark;
use regex::Regex;
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;
use std::path::Path;

pub const PART_LINES: usize = 60;
/// A part's state is at most this many bytes (Jev answered a 50 KB state in 1.5 s).
pub const STATE_BYTES: usize = 24 * 1024;
const AROUND: usize = 60;
const AROUND_TRIM: usize = 20;
const CALLERS: usize = 20;
const SIMILAR: usize = 8;
const MAP: usize = 100;

#[derive(Debug, Clone)]
pub struct Part {
    pub file: String,
    /// The part as unified diff text.
    pub diff: String,
    pub added: Vec<Line>,
}

impl Part {
    /// The first added line, where a mark about the whole part stands.
    pub fn line(&self) -> u32 {
        self.added.first().map_or(1, |l| l.no)
    }
}

fn windows(h: &Hunk) -> Vec<Hunk> {
    if h.changed() <= PART_LINES {
        return vec![h.clone()];
    }
    let mut out: Vec<Hunk> = h.added.chunks(PART_LINES).map(|c| Hunk { header: h.header.clone(), added: c.to_vec(), removed: Vec::new(), text: c.iter().map(|l| format!("+{}", l.text)).collect::<Vec<_>>().join("\n") }).collect();
    if let Some(first) = out.first_mut() {
        first.removed = h.removed.iter().take(PART_LINES).cloned().collect();
        let gone: Vec<String> = first.removed.iter().map(|l| format!("-{}", l.text)).collect();
        first.text = format!("{}\n{}", gone.join("\n"), first.text).trim_start().to_string();
    }
    out
}

/// The parts of a change: files that were deleted have none (there is nothing left to judge).
pub fn cut(change: &Change) -> Vec<Part> {
    let mut parts = Vec::new();
    for f in change.files.iter().filter(|f| !f.deleted && f.added().next().is_some()) {
        let mut open: Option<(Part, usize)> = None;
        for h in f.hunks.iter().flat_map(windows) {
            let n = h.changed();
            if let Some((p, used)) = open.as_mut()
                && *used + n <= PART_LINES
            {
                p.diff.push('\n');
                p.diff.push_str(&h.text);
                p.added.extend(h.added.iter().cloned());
                *used += n;
                continue;
            }
            parts.extend(open.take().map(|(p, _)| p));
            open = Some((Part { file: f.path.clone(), diff: h.text.clone(), added: h.added.clone() }, n));
        }
        parts.extend(open.map(|(p, _)| p));
    }
    parts
}

/// What the task is about, for the state: the line the operator typed, the notes they kept, and the head of the doer's report.
#[derive(Debug, Clone, Default)]
pub struct Goal {
    pub line: String,
    pub notes: Vec<String>,
    pub done: String,
    pub answer_head: String,
    /// `pass`, `fail`, or empty when the project's check was not seen.
    pub check: String,
}

fn lang_of(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default() {
        "rs" => "rust",
        "py" => "python",
        "js" | "jsx" => "javascript",
        "ts" | "tsx" => "typescript",
        "go" => "go",
        "toml" => "toml",
        "json" => "json",
        "md" => "markdown",
        "sh" => "shell",
        _ => "text",
    }
}

fn clip(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max { t.to_string() } else { format!("{}…", t.chars().take(max.saturating_sub(1)).collect::<String>()) }
}

fn defs() -> Regex {
    Regex::new(r"\b(?:fn|def|function|class|struct|enum|trait|func|interface|type)\s+([A-Za-z_][A-Za-z0-9_]*)|\bconst\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(?:async\s*)?\(").unwrap_or_else(|_| Regex::new("^$").expect("an empty pattern is a pattern"))
}

/// The names a part defines.
fn symbols(part: &Part) -> Vec<String> {
    let re = defs();
    let mut out: Vec<String> = Vec::new();
    for l in &part.added {
        for c in re.captures_iter(&l.text) {
            if let Some(m) = c.get(1).or_else(|| c.get(2))
                && !out.iter().any(|o| o == m.as_str())
            {
                out.push(m.as_str().to_string());
            }
        }
    }
    out
}

fn words(name: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for c in name.chars() {
        if c == '_' || c == '-' || (c.is_uppercase() && prev_lower) {
            if cur.chars().count() >= 4 {
                out.push(cur.to_lowercase());
            }
            cur.clear();
        }
        if c != '_' && c != '-' {
            cur.push(c);
        }
        prev_lower = c.is_lowercase();
    }
    if cur.chars().count() >= 4 {
        out.push(cur.to_lowercase());
    }
    out
}

/// Where the project's files are, nearest first.
fn map(dir: &Path, file: &str) -> Vec<String> {
    let Some(all) = git(dir, &["ls-files", "-z"]) else { return Vec::new() };
    let near = file.rsplit_once('/').map_or("", |(d, _)| d);
    let mut paths: Vec<&str> = all.split('\0').filter(|p| !p.is_empty()).collect();
    paths.sort_by_key(|p| (!p.starts_with(near) as u8, p.matches('/').count(), *p));
    paths.into_iter().take(MAP).map(String::from).collect()
}

fn style(text: &str) -> Json {
    let (mut tab, mut two, mut four) = (0, 0, 0);
    for l in text.lines() {
        if l.starts_with('\t') {
            tab += 1;
        } else if l.starts_with("    ") {
            four += 1;
        } else if l.starts_with("  ") {
            two += 1;
        }
    }
    let indent = if tab > two.max(four) { "tabs" } else if two > four { "2 spaces" } else { "4 spaces" };
    let (chess, camel) = (text.matches("_").count(), Regex::new(r"[a-z][A-Z]").map_or(0, |r| r.find_iter(text).count()));
    json!({ "indent": indent, "naming": if chess >= camel { "snake_case" } else { "camelCase" } })
}

/// How sure an edge is that a word search found: it names the same word, nothing parsed it (`qc.provenance`).
pub const LEXICAL: &str = "~lexical";

/// Say, in the state, that the callers, tests, look-alikes and file map are word matches.
pub fn tag(state: &mut Json) {
    state["basis"] = json!({
        "callers": LEXICAL, "tests": LEXICAL, "similar": LEXICAL, "map": LEXICAL,
        "note": "found by searching for the same word, not by reading the code: a caller may only share a name, and one that is missing may be called another way",
    });
}

/// The state of one part, with the cuts the size asks for made in the stated order (the file map, the look-alikes, the code round).
pub fn state(change: &Change, part: &Part, goal: &Goal, marks: &[Mark]) -> Json {
    let text = change.after(&part.file).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let (first, last) = (part.added.first().map_or(1, |l| l.no) as usize, part.added.last().map_or(1, |l| l.no) as usize);
    let around = |pad: usize| lines[first.saturating_sub(pad + 1).min(lines.len())..(last + pad).min(lines.len())].join("\n");
    let syms = symbols(part);
    let (mut callers, mut tests) = (Vec::new(), BTreeSet::new());
    for s in &syms {
        let Some(hits) = git(&change.dir, &["grep", "-n", "-w", "-I", "-e", s]) else { continue };
        for h in hits.lines() {
            let mut p = h.splitn(3, ':');
            let (path, no, rest) = (p.next().unwrap_or_default(), p.next().unwrap_or_default(), p.next().unwrap_or_default());
            if path == part.file && no.parse::<usize>().is_ok_and(|n| (first..=last).contains(&n)) {
                continue;
            }
            if path.contains("test") {
                tests.insert(format!("{path}:{no}"));
            } else if callers.len() < CALLERS {
                callers.push(format!("{path}:{no}: {}", clip(rest, 120)));
            }
        }
    }
    let mut similar: Vec<Json> = Vec::new();
    let w: BTreeSet<String> = syms.iter().flat_map(|s| words(s)).collect();
    if !w.is_empty() {
        let any = w.iter().map(|x| regex::escape(x)).collect::<Vec<_>>().join("|");
        let pat = format!(r"(fn|def|function|class|func)\s+[A-Za-z_0-9]*({any})[A-Za-z_0-9]*");
        if let Some(hits) = git(&change.dir, &["grep", "-n", "-I", "-i", "-E", "-e", &pat]) {
            let re = defs();
            for h in hits.lines() {
                let mut p = h.splitn(3, ':');
                let (path, no, rest) = (p.next().unwrap_or_default(), p.next().unwrap_or_default(), p.next().unwrap_or_default());
                let Some(name) = re.captures(rest).and_then(|c| c.get(1).or_else(|| c.get(2))).map(|m| m.as_str().to_string()) else { continue };
                if syms.contains(&name) || similar.iter().any(|s| s["name"] == name) {
                    continue;
                }
                similar.push(json!({ "name": name, "sig": clip(rest, 120), "at": format!("{path}:{no}") }));
                if similar.len() >= SIMILAR {
                    break;
                }
            }
        }
    }
    let tier0: Vec<Json> = marks.iter().filter(|m| m.file == part.file).map(|m| json!({ "pattern": m.pattern, "line": m.line })).collect();
    let mut state = json!({
        "goal": { "line": goal.line, "notes": goal.notes, "done": goal.done },
        "delivered": { "file": part.file, "lang": lang_of(&part.file), "diff": part.diff, "answer_head": clip(&goal.answer_head, 400) },
        "around": { "function": around(AROUND), "callers": callers, "tests": tests.into_iter().take(CALLERS).collect::<Vec<_>>() },
        "similar": similar,
        "repo": { "map": map(&change.dir, &part.file), "style": style(&text) },
        "checks": { "lane_check": if goal.check.is_empty() { "none" } else { goal.check.as_str() }, "tier0": tier0 },
    });
    // over the size: the file map goes first, then the look-alikes, then the code round is cut to the changed lines and 20 more
    let size = |s: &Json| s.to_string().len();
    if size(&state) > STATE_BYTES {
        state["repo"]["map"] = json!([]);
    }
    if size(&state) > STATE_BYTES {
        state["similar"] = json!([]);
    }
    if size(&state) > STATE_BYTES {
        state["around"]["function"] = json!(around(AROUND_TRIM));
    }
    state
}
