//! The first tier, with no model: the `regex` patterns over the lines a change adds. A hunk that also
//! removes a line holding the same match has not added it (a rename or a move rewrites every line it touches), so only the
//! net-new matches of each hunk are marks; a pattern with `scope = "removed"` counts what a hunk removes and does not add back.

use crate::change::Change;
use crate::diff::{FileDiff, Hunk, Line};
use crate::pattern::{Ready, Scope, Severity, Way};
use std::collections::BTreeMap;

/// A place where a pattern found something.
#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    pub pattern: String,
    pub file: String,
    pub line: u32,
    pub p: f64,
    pub severity: Severity,
    pub tier: u8,
    /// For a choice: the helper it repeats.
    pub answer: String,
}

impl Mark {
    pub fn at(r: &Ready, file: &str, line: u32) -> Mark {
        Mark { pattern: r.p.id.clone(), file: file.to_string(), line, p: 1.0, severity: r.p.severity, tier: 0, answer: String::new() }
    }
}

/// Every match of `r` in `lines`, with the line it starts on.
fn matches(r: &Ready, lines: &[Line], whole: bool) -> Vec<(String, u32)> {
    let Some(re) = &r.re else { return Vec::new() };
    if !whole {
        return lines.iter().flat_map(|l| re.find_iter(&l.text).map(|m| (m.as_str().to_string(), l.no))).collect();
    }
    let text = lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n");
    re.find_iter(&text)
        .map(|m| {
            let at = text[..m.start()].matches('\n').count();
            (m.as_str().to_string(), lines.get(at).map_or(0, |l| l.no))
        })
        .collect()
}

/// The matches of `r` in one hunk that are new: those of the side it counts, less as many of the same text as the other side has.
fn net_new(r: &Ready, h: &Hunk) -> Vec<u32> {
    let (here, there) = match r.p.scope {
        Scope::Added => (&h.added, &h.removed),
        Scope::Removed => (&h.removed, &h.added),
    };
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for (text, _) in matches(r, there, r.p.multiline) {
        *seen.entry(text).or_default() += 1;
    }
    let mut lines = Vec::new();
    for (text, line) in matches(r, here, r.p.multiline) {
        match seen.get_mut(&text) {
            Some(n) if *n > 0 => *n -= 1,
            _ => lines.push(line),
        }
    }
    lines.dedup();
    lines
}

/// The marks of the `regex` patterns over a change.
pub fn regex(patterns: &[Ready], change: &Change) -> Vec<Mark> {
    let mut marks = Vec::new();
    for r in patterns.iter().filter(|r| r.p.kind == Way::Regex) {
        for f in change.files.iter().filter(|f| r.applies(&f.path)) {
            for h in &f.hunks {
                marks.extend(net_new(r, h).into_iter().map(|line| Mark::at(r, &f.path, line)));
            }
        }
    }
    marks
}

/// The files of a change that a pattern looks at.
pub fn files_of<'a>(r: &Ready, change: &'a Change) -> Vec<&'a FileDiff> {
    change.files.iter().filter(|f| r.applies(&f.path) && !f.deleted).collect()
}
