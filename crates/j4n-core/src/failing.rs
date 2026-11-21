//! What a failed check names as failing, read from its output by code: the tests that failed, and the test modules that could not be
//! loaded. A part laid in is judged by what it breaks, not by what the folder had broken before it ([`caused`]).
//!
//! Two runners are read: Python's `unittest` (a block `ERROR: name (id)` or `FAIL: name (id)` under a line of `=`, and `FAILED (…)` at
//! the end) and `pytest` (the lines `FAILED id` and `ERROR id` of its short summary, and the counts at the end). Any other output is
//! not read. An output is read only when the entries found are as many as its own counts say: the check keeps the end of a long output,
//! and an entry cut off with the start of it must not pass for a test that did not fail.

use std::collections::BTreeSet;

/// What one failed run names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Failing {
    /// Each failing entry's id (`test_engine.Moves.test_turn`, `tests/test_a.py::test_x`), as often as the runner counts it.
    pub ids: Vec<String>,
    /// The test modules that could not be loaded (`test_screen`, `tests/test_b.py`): every test in one is failing as long as it is.
    pub unloaded: BTreeSet<String>,
}

impl Failing {
    /// The ids, each once, in their order.
    pub fn distinct(&self) -> Vec<String> {
        let mut seen = BTreeSet::new();
        self.ids.iter().filter(|i| seen.insert(i.as_str())).cloned().collect()
    }

    /// `id` failed here: the same id, or a test of a module that could not be loaded.
    fn has(&self, id: &str) -> bool {
        self.ids.iter().any(|i| i == id) || self.unloaded.iter().any(|m| id == m || id.starts_with(&format!("{m}.")) || id.starts_with(&format!("{m}::")))
    }
}

/// What a failed run of a check names, or why it cannot be told.
pub fn read(out: &str) -> Result<Failing, String> {
    let lines: Vec<&str> = out.lines().map(str::trim_end).collect();
    if lines.iter().any(|l| l.starts_with("Ran ") && l.contains(" test")) {
        return unittest(&lines);
    }
    if lines.iter().any(|l| l.contains("short test summary info")) || lines.iter().rev().find(|l| !l.trim().is_empty()).is_some_and(|l| counts(l).is_some()) {
        return pytest(&lines);
    }
    Err("its output is not one J4n reads (Python's unittest or pytest)".into())
}

fn unittest(lines: &[&str]) -> Result<Failing, String> {
    let mut f = Failing::default();
    for (i, l) in lines.iter().enumerate().skip(1) {
        let rule = lines[i - 1];
        if rule.len() < 20 || !rule.chars().all(|c| c == '=') {
            continue;
        }
        let Some(head) = ["ERROR: ", "FAIL: ", "UNEXPECTED SUCCESS: "].iter().find_map(|w| l.strip_prefix(w)) else { continue };
        let (name, rest) = head.split_once(" (").unwrap_or((head, ""));
        let inside = rest.split_once(')').map_or("", |(i, _)| i);
        // Python 3.11 on names the test in the brackets (`mod.Class.test_x`); before, the class only (`mod.Class`)
        let id = if inside.is_empty() {
            name.to_string()
        } else if inside == name || inside.ends_with(&format!(".{name}")) {
            inside.to_string()
        } else {
            format!("{inside}.{name}")
        };
        // a module that could not be imported is a test of the loader's, named after the module
        if inside.starts_with("unittest.loader.") {
            f.unloaded.insert(name.to_string());
        }
        f.ids.push(id);
    }
    let Some(summary) = lines.iter().rev().find_map(|l| l.trim().strip_prefix("FAILED (")) else { return Err("it failed without unittest's `FAILED (…)` line".into()) };
    let want: usize = summary.trim_end_matches(')').split(',').filter_map(|kv| kv.trim().split_once('=')).filter(|(k, _)| matches!(*k, "failures" | "errors" | "unexpected successes")).filter_map(|(_, v)| v.trim().parse::<usize>().ok()).sum();
    whole(f, want)
}

/// The counts at the end of a pytest run (`1 failed, 3 passed, 1 error in 0.12s`): failed and errors together, `None` for any other line.
fn counts(line: &str) -> Option<usize> {
    let line = line.trim().trim_matches('=').trim();
    let (said, _) = line.rsplit_once(" in ")?;
    let mut bad = 0;
    for part in said.split(',') {
        let (n, word) = part.trim().split_once(' ')?;
        let n: usize = n.parse().ok()?;
        match word.trim() {
            "failed" | "error" | "errors" => bad += n,
            "passed" | "skipped" | "deselected" | "xfailed" | "xpassed" | "warning" | "warnings" | "rerun" | "reruns" => {}
            _ => return None,
        }
    }
    Some(bad)
}

fn pytest(lines: &[&str]) -> Result<Failing, String> {
    // a collection error stops the run before any test: what did not run says nothing either way
    if lines.iter().any(|l| l.contains("Interrupted:")) {
        return Err("pytest was interrupted before its tests ran".into());
    }
    let mut f = Failing::default();
    let from = lines.iter().position(|l| l.contains("short test summary info")).map_or(lines.len(), |p| p + 1);
    for l in &lines[from..] {
        let Some((word, rest)) = l.split_once(' ').filter(|(w, _)| matches!(*w, "FAILED" | "ERROR")) else { continue };
        let id = rest.split_once(" - ").map_or(rest, |(id, _)| id).trim().to_string();
        if word == "ERROR" && !id.contains("::") {
            f.unloaded.insert(id.clone());
        }
        f.ids.push(id);
    }
    let want = lines.iter().rev().find(|l| !l.trim().is_empty()).and_then(|l| counts(l)).ok_or("it failed without pytest's counts at the end")?;
    whole(f, want)
}

/// `f`, when it holds as many entries as the runner counted, and some.
fn whole(f: Failing, want: usize) -> Result<Failing, String> {
    if f.ids.is_empty() {
        return Err("it failed and named no test".into());
    }
    if f.ids.len() != want {
        return Err(format!("{} failing named of the {want} it counts (the start of the output was cut)", f.ids.len()));
    }
    Ok(f)
}

/// What came of a part laid in, against the folder's check just before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caused {
    /// Every failure was there before (the ids that still fail).
    Old(Vec<String>),
    /// These failures are new: a test that passed, or was not there, before (empty: which is not known, the check before was green).
    New(Vec<String>),
    /// One of the two outputs could not be read: why.
    Unread(String),
}

/// What a part broke: `before` is the check of the folder just before it was laid in (`None`: it passed), `after` the one after.
pub fn caused(before: Option<&str>, after: &str) -> Caused {
    // a green folder needs no reading: whatever fails now is the part's
    let Some(before) = before else { return Caused::New(read(after).map(|a| a.distinct()).unwrap_or_default()) };
    let before = match read(before) {
        Ok(b) => b,
        Err(why) => return Caused::Unread(format!("the check before it: {why}")),
    };
    let after = match read(after) {
        Ok(a) => a,
        Err(why) => return Caused::Unread(format!("the check after it: {why}")),
    };
    let new: Vec<String> = after.distinct().into_iter().filter(|id| !before.has(id)).collect();
    if new.is_empty() { Caused::Old(after.distinct()) } else { Caused::New(new) }
}
