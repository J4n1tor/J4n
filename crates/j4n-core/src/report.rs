//! The report: the one shape a branch answers with, and the check of its claims before the main lane
//! reads them. A branch answers with one JSON object, a closed shape:
//! `{"answer", "found": [{"claim", "at"}], "changed", "ran": [{"cmd", "exit"}], "sure": "low|mid|high", "open": []}`, and a review's `notes`.
//!
//! A claim is checked against what exists, never against the branch's word: an `at` of `path:line` must name a line of a file in the
//! branch's place that holds a word of the claim, and each `ran` must be a command of a `run.shell` event of the branch itself in the Log. A claim
//! that fails, or that cannot be checked here (an `at` that is a URL), is marked `unchecked` and kept: it is never dropped.

use crate::text::clip;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::collections::BTreeMap;
use std::path::Path;

pub const SURE: [&str; 3] = ["low", "mid", "high"];
/// The shape a branch that reads is told to answer in: an answer in any other shape is kept as words, and its `open` is not read.
pub const SHAPE: &str = "When you are done, answer with one JSON object and nothing else: {\"answer\": \"what you found, in a few lines\", \"found\": [{\"claim\": \"…\", \"at\": \"path:line\"}], \"ran\": [{\"cmd\": \"…\", \"exit\": 0}], \"sure\": \"low|mid|high\", \"open\": [\"path:line what goes wrong\"]}.";
const FIELD: usize = 400;
const LIST: usize = 24;
/// The largest file a claim is looked up in.
const FILE_MAX: u64 = 4 << 20;
/// Words of a claim that say nothing about a line.
const STOP: [&str; 12] = ["the", "and", "for", "with", "this", "that", "are", "was", "has", "not", "its", "from"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub claim: String,
    pub at: String,
    #[serde(default)]
    pub unchecked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ran {
    pub cmd: String,
    pub exit: i64,
    #[serde(default)]
    pub unchecked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub answer: String,
    pub found: Vec<Claim>,
    pub changed: String,
    pub ran: Vec<Ran>,
    pub sure: String,
    pub open: Vec<String>,
    /// A review's points that name no case that fails (a risk a later change would make real, style): they inform the main lane and send
    /// nothing back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// What the chat would have the operator's request go by: a pick and its reason. Data: Main reads it and decides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opinion: Option<Opinion>,
}

/// An opinion on a request: a pick and its reason, the reason cut to 400 tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Opinion {
    pub pick: String,
    pub why: String,
}

/// The most characters of an opinion's reason: 400 tokens at four characters each.
pub const OPINION_WHY: usize = 1600;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Said {
    answer: String,
    #[serde(default)]
    found: Vec<Found>,
    #[serde(default)]
    changed: Option<String>,
    #[serde(default)]
    ran: Vec<Cmd>,
    sure: String,
    /// A point is text; one written as an object (`{"at": "a.py:3", "what": "…"}`) is read as its words, so its concern is not lost.
    #[serde(default)]
    open: Vec<Json>,
    #[serde(default)]
    notes: Vec<Json>,
    #[serde(default)]
    opinion: Option<Opinion>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Found {
    claim: String,
    at: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cmd {
    cmd: String,
    exit: i64,
}

/// An open point as text: a string as it is; an object's words and numbers, where it is first (`at`, `file`, `path`, `line`), else its JSON; anything else as JSON.
fn point(o: &Json) -> String {
    let Json::Object(m) = o else { return o.as_str().map_or_else(|| o.to_string(), String::from) };
    let first = |k: &String| !["at", "file", "path", "line"].contains(&k.as_str());
    let mut keys: Vec<&String> = m.keys().collect();
    keys.sort_by_key(|k| first(k));
    let words: Vec<String> = keys.iter().filter_map(|k| m[k.as_str()].as_str().map(|s| s.trim().to_string()).or_else(|| m[k.as_str()].as_number().map(|n| n.to_string()))).filter(|s| !s.is_empty()).collect();
    if words.is_empty() { o.to_string() } else { words.join(" ") }
}

/// The object an answer holds: from its first `{` to its last `}` (a doer may wrap it in words or a fence).
fn object(text: &str) -> Option<&str> {
    let (a, b) = (text.find('{')?, text.rfind('}')?);
    (a < b).then(|| &text[a..=b])
}

impl Report {
    /// Read a branch's answer as a report. Every claim and command comes out `unchecked` until [`check`](Report::check) has been over it
    /// (a branch cannot say it checked itself: the field is not part of its shape).
    pub fn take(text: &str) -> Result<Report, String> {
        let said: Said = serde_json::from_str(object(text).ok_or("the answer holds no object")?).map_err(|e| format!("the answer is not the shape of a report: {e}"))?;
        if !SURE.contains(&said.sure.as_str()) {
            return Err(format!("`sure` is one of {}", SURE.join(", ")));
        }
        if said.found.len() > LIST || said.ran.len() > LIST || said.open.len() > LIST || said.notes.len() > LIST {
            return Err(format!("a report holds up to {LIST} claims, commands and open points each"));
        }
        let ok = |s: &str| !s.trim().is_empty() && !s.chars().any(|c| c.is_control() && c != '\n');
        if !ok(&said.answer) || said.found.iter().any(|f| !ok(&f.claim) || !ok(&f.at)) || said.ran.iter().any(|r| !ok(&r.cmd)) {
            return Err("an answer, a claim, a place and a command are text, and not empty".into());
        }
        Ok(Report {
            answer: clip(&said.answer, FIELD * 4),
            found: said.found.into_iter().map(|f| Claim { claim: clip(&f.claim, FIELD), at: clip(&f.at, 200), unchecked: true }).collect(),
            changed: clip(said.changed.as_deref().map(str::trim).filter(|c| !c.is_empty()).unwrap_or("none"), 200),
            ran: said.ran.into_iter().map(|r| Ran { cmd: clip(&r.cmd, 300), exit: r.exit, unchecked: true }).collect(),
            sure: said.sure,
            open: said.open.iter().map(|o| clip(&point(o), FIELD)).collect(),
            notes: said.notes.iter().map(|o| clip(&point(o), FIELD)).collect(),
            opinion: said.opinion.filter(|o| ok(&o.pick) && ok(&o.why)).map(|o| Opinion { pick: clip(o.pick.trim(), 120), why: clip(o.why.trim(), OPINION_WHY) }),
        })
    }

    /// An answer that is not in the shape, as a report that says so: its words are the answer, nothing is claimed, and it is not sure.
    pub fn plain(answer: &str, why: &str) -> Report {
        Report { answer: clip(answer.trim(), FIELD * 4), found: Vec::new(), changed: "none".into(), ran: Vec::new(), sure: "low".into(), open: vec![clip(why, FIELD)], notes: Vec::new(), opinion: None }
    }

    /// Check every claim: `root` is the folder of the branch's place, `runs` the `(cmd, exit)` of the branch's own `run.shell` events.
    pub fn check(&mut self, root: &Path, runs: &[(String, i64)]) {
        for f in &mut self.found {
            f.unchecked = !holds(root, &f.at, &f.claim);
        }
        for r in &mut self.ran {
            r.unchecked = !runs.iter().any(|(c, e)| ran(c, *e, &r.cmd, r.exit));
        }
    }

    /// How many claims and commands are not checked.
    pub fn unchecked(&self) -> usize {
        self.found.iter().filter(|f| f.unchecked).count() + self.ran.iter().filter(|r| r.unchecked).count()
    }

    /// What the main lane reads in the Brief's `branches` section: short, and each unchecked claim says so.
    pub fn text(&self, task: &str) -> String {
        let mark = |u: bool| if u { " (unchecked)" } else { "" };
        let mut out = format!("{task} · sure {}: {}", self.sure, clip(&self.answer, 300));
        for f in self.found.iter().take(6) {
            out.push_str(&format!("\n- {} @ {}{}", clip(&f.claim, 120), f.at, mark(f.unchecked)));
        }
        for r in self.ran.iter().take(4) {
            out.push_str(&format!("\n- ran {} → {}{}", clip(&r.cmd, 80), r.exit, mark(r.unchecked)));
        }
        if self.changed != "none" {
            out.push_str(&format!("\n- changed {}", self.changed));
        }
        for o in self.open.iter().take(4) {
            out.push_str(&format!("\n- open: {}", clip(o, 120)));
        }
        for o in self.notes.iter().take(2) {
            out.push_str(&format!("\n- note: {}", clip(o, 120)));
        }
        out
    }

    pub fn json(&self) -> Json {
        serde_json::to_value(self).unwrap_or(Json::Null)
    }

    /// The files the checked claims name under `root`, each with its [`file_hash`] now: what a later task must find unchanged before
    /// it is shown the claim again. An unchecked claim names nothing.
    pub fn files(&self, root: &Path) -> BTreeMap<String, String> {
        self.found.iter().filter(|f| !f.unchecked).filter_map(|f| Some((at_path(&f.at)?.to_string(), file_hash(root, at_path(&f.at)?)?))).collect()
    }
}

/// The path an `at` (`path:line`) names, if it may be looked up under a folder: relative, and not out of it.
/// A `ran` claim holds for a run of the branch whose exit it says when each of its commands is one of the run's (a doer runs `cd api && .venv/bin/pytest -q
/// 2>&1 | tail` and reports `api/.venv/bin/pytest -q`): the commands are split at `&&`, `||`, `;`, `|` and lines, redirections are dropped, and a program
/// is its file name. A note after the command, in brackets, and its `cd`s are left out.
fn ran(run: &str, exit: i64, cmd: &str, said: i64) -> bool {
    fn parts(text: &str) -> Vec<Vec<String>> {
        let words = |part: &str| -> Vec<String> {
            let mut w: Vec<String> = part.split_whitespace().filter(|w| !w.contains('>') && !w.starts_with('<')).map(String::from).collect();
            if let Some(first) = w.first_mut() {
                *first = first.rsplit('/').next().unwrap_or(first).to_string();
            }
            w
        };
        text.split(['\n', ';']).flat_map(|l| l.split("&&")).flat_map(|l| l.split("||")).flat_map(|l| l.split('|')).map(words).filter(|w| !w.is_empty()).collect()
    }
    let runs = parts(run);
    let holds = |cmd: &str| {
        // a `cd` says where the rest ran, and the doer names the folder as it likes (`cd docs` for `cd /…/worktrees/t3/docs`)
        let claimed: Vec<Vec<String>> = parts(cmd).into_iter().filter(|c| c[0] != "cd").collect();
        !claimed.is_empty() && claimed.iter().all(|c| runs.contains(c))
    };
    // a note the doer put after the command (`go vet ./... (in cli/)`) is not part of it
    let bare = cmd.trim_end().strip_suffix(')').and_then(|c| c.rfind(" (").map(|at| &cmd[..at]));
    exit == said && (holds(cmd) || bare.is_some_and(holds))
}

pub fn at_path(at: &str) -> Option<&str> {
    let (path, line) = at.rsplit_once(':')?;
    (line.trim().parse::<usize>().is_ok_and(|n| n > 0) && !path.is_empty() && !path.starts_with('/') && !path.split('/').any(|c| c == "..")).then_some(path)
}

/// The first eight bytes, in hex, of the SHA-256 of a file under `root`; none for a file
/// that is not there, or too large.
pub fn file_hash(root: &Path, path: &str) -> Option<String> {
    let file = root.join(path);
    std::fs::metadata(&file).ok().filter(|m| m.is_file() && m.len() <= FILE_MAX)?;
    Some(crate::text::short_hash(std::fs::read(file).ok()?))
}

/// What makes two goals the same task: the words, in lower case, one space apart, and nothing else.
pub fn goal_key(goal: &str) -> String {
    let said = goal.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    crate::text::short_hash(said)
}

/// Whether `at` (`path:line`) names a line of a file under `root` that holds a word of `claim`. A path that leaves the folder, a
/// file too large, a URL: not checkable here, so not held.
fn holds(root: &Path, at: &str, claim: &str) -> bool {
    let Some((path, line)) = at.rsplit_once(':') else { return false };
    let Ok(line) = line.trim().parse::<usize>() else { return false };
    if line == 0 || path.is_empty() || path.starts_with('/') || path.split('/').any(|c| c == "..") {
        return false;
    }
    let file = root.join(path);
    if !std::fs::metadata(&file).is_ok_and(|m| m.len() <= FILE_MAX) {
        return false;
    }
    let Ok(text) = std::fs::read_to_string(&file) else { return false };
    let Some(row) = text.lines().nth(line - 1) else { return false };
    let row = row.to_lowercase();
    claim.to_lowercase().split(|c: char| !(c.is_alphanumeric() || c == '_')).any(|w| w.chars().count() >= 3 && !STOP.contains(&w) && row.contains(w))
}
