//! The offer line: when a goal names a list of 8 or more items and a per-item word, the first text of
//! the session gets one line after the goal that offers the loop from code. It is decided here, by rules, with no model; it sits beside the
//! card (`kitsock::Kitted`), so `j4n-core` has no line for it. It names the tool whose spec shares the most word stems with the goal (none
//! when two tie or none shares one), and a recipe when one fits: `checks` for the check on every changed file, `each` for a file of items.
//! What it prints runs as written: `each` is named only for a tool that has one argument for the item, and a tool that takes a
//! `url` is not picked for items that are not URLs.
//!
//! **Counting**: the items are the largest of a file named in the goal (its non-blank lines), a folder named in it (its files), a
//! marked list in the goal (`-`, `*`, `1.` lines) and a run of short items separated by commas or `and` in one line of the goal.

use crate::model::Tool;
use std::io::Read;
use std::path::Path;

/// Items below this get no line.
const LEAST: usize = 8;
/// The words that say "one by one": a goal needs one.
const PER_ITEM: [&str; 8] = ["each", "every", "for all", "which of", "how many", "all of", "one by one", "one at a time"];
/// Words of any goal that tell nothing of the tool; they are not matched to a spec.
const SKIP: &str = "this that with from what which whether them they have each every item items list lists line lines file files text name path into then than when where also";
const CHECK: [&str; 5] = ["check", "test", "lint", "verify", "compile"];
const CHANGED: [&str; 5] = ["changed", "edited", "modified", "touched", "wrote"];
/// File endings of source code: the lines of such a file are code, not a list of items; a goal that names `tests/test_rules.py` and says "each of these"
/// is not a loop over its 20 lines.
const CODE: [&str; 20] = ["py", "rs", "js", "ts", "tsx", "jsx", "go", "java", "c", "h", "cc", "cpp", "hpp", "rb", "sh", "kt", "swift", "php", "cs", "lua"];
/// Bytes of a file read to count its lines, and files seen in a folder.
const READ: u64 = 1 << 20;
const WALK: usize = 5000;

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
}

fn has(goal: &[String], phrases: &[&str]) -> bool {
    let padded = format!(" {} ", goal.join(" "));
    phrases.iter().any(|p| padded.contains(&format!(" {p} ")) || padded.contains(&format!(" {p}s ")))
}

fn is_url(text: &str) -> bool {
    let t = text.trim();
    t.starts_with("http://") || t.starts_with("https://")
}

/// The non-blank lines of a file (the first megabyte of it; a file that is not text has none), and whether they are all URLs.
fn lines_of(path: &Path) -> (usize, bool) {
    let mut text = String::new();
    if let Ok(f) = std::fs::File::open(path) {
        let _ = f.take(READ).read_to_string(&mut text);
    }
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    (lines.len(), !lines.is_empty() && lines.iter().take(8).all(|l| is_url(l)))
}

/// The files under `dir` (not those of a dot folder), up to [`WALK`].
fn files_in(dir: &Path) -> usize {
    let (mut todo, mut n) = (vec![dir.to_path_buf()], 0);
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            if e.file_name().to_string_lossy().starts_with('.') || n >= WALK {
                continue;
            }
            match e.file_type() {
                Ok(t) if t.is_dir() => todo.push(e.path()),
                Ok(t) if t.is_file() => n += 1,
                _ => {}
            }
        }
    }
    n
}

/// A marked list in the goal, or the longest run of pieces of six words or fewer, split by commas, `;` or `and`, in one line of it.
fn listed(goal: &str) -> usize {
    let marked = goal.lines().filter(|l| {
        let t = l.trim_start();
        t.starts_with("- ") || t.starts_with("* ") || t.split_once(['.', ')']).is_some_and(|(n, _)| (1..=3).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_digit()))
    });
    let runs = goal.lines().map(|l| {
        let (mut best, mut run) = (0, 0);
        for piece in l.replace(" and ", ",").split([',', ';']) {
            run = if (1..=6).contains(&piece.split_whitespace().count()) { run + 1 } else { 0 };
            best = best.max(run);
        }
        best
    });
    marked.count().max(runs.max().unwrap_or(0))
}

/// What the goal's items are: how many, where (a file or folder named in the goal, as the goal spells it), whether they are a file's lines,
/// and whether they are URLs (the lines of a file all are; a list in the goal is when the goal holds a link).
fn items(goal: &str, dir: &Path) -> Option<(usize, String, bool, bool)> {
    let mut best = (listed(goal), "the goal".to_string(), false, goal.contains("http://") || goal.contains("https://"));
    for tok in goal.split_whitespace().map(|t| t.trim_matches(|c: char| ",;:()[]'\"`".contains(c)).trim_end_matches('.')) {
        if tok.is_empty() || tok.starts_with('/') || tok.contains("..") {
            continue;
        }
        let path = dir.join(tok);
        let (n, file, urls) = if path.is_file() {
            if path.extension().and_then(|e| e.to_str()).is_some_and(|e| CODE.contains(&e.to_lowercase().as_str())) {
                continue;
            }
            let (n, urls) = lines_of(&path);
            (n, true, urls)
        } else if path.is_dir() && !tok.trim_matches(['.', '/']).is_empty() {
            (files_in(&path), false, false)
        } else {
            continue;
        };
        if n > best.0 {
            best = (n, tok.to_string(), file, urls);
        }
    }
    (best.0 >= LEAST).then_some(best)
}

fn stems(text: &str, min: usize) -> Vec<String> {
    words(text).into_iter().filter(|w| w.chars().count() >= min && !SKIP.split(' ').any(|s| s == w)).map(|w| w.chars().take(4).collect()).collect()
}

/// The names of the arguments a call must give, as `packs/kit/recipes/each.py` reads them: the schema's `required`, else all it lists.
fn needs(t: &Tool) -> Vec<&str> {
    let named = |k: &str| t.schema.get(k).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|n| n.as_str()).collect::<Vec<_>>());
    let props = || t.schema.get("properties").and_then(|p| p.as_object()).map(|p| p.keys().map(String::as_str).collect::<Vec<_>>());
    named("required").filter(|r| !r.is_empty()).or_else(props).unwrap_or_default()
}

/// The tool whose spec (name and words) shares the most stems with the goal; none when two share the most or none shares any. A tool
/// that takes a `url` is left out when the items are not URLs: "pages" in a goal over local files is no reason to fetch them.
fn tool_for<'a>(goal: &str, specs: &'a [Tool], urls: bool) -> Option<&'a Tool> {
    let fetches = |t: &Tool| t.schema.get("properties").is_some_and(|p| p.get("url").is_some());
    let specs: Vec<&Tool> = specs.iter().filter(|t| urls || !fetches(t)).collect();
    let want = stems(goal, 3);
    let score = |t: &Tool| {
        let mut have = stems(&t.doc, 4);
        have.extend(words(&t.name).iter().map(|w| w.chars().take(4).collect::<String>()));
        want.iter().filter(|w| have.contains(w)).count()
    };
    let best = specs.iter().map(|t| score(t)).max()?;
    let mut top = specs.iter().filter(|t| score(t) == best);
    top.next().copied().filter(|_| best > 0 && top.next().is_none())
}

/// The one line for `goal` in the task's folder `dir`, given the tools code may call; none when the goal is not a loop's shape.
pub fn line(goal: &str, dir: &Path, specs: &[Tool]) -> Option<String> {
    line_with(goal, dir, specs, &[])
}

/// [`line`], without what the operator turned off (`kit.off`): a recipe that is off is not named, and with `many` off there is no line.
pub fn line_with(goal: &str, dir: &Path, specs: &[Tool], off: &[String]) -> Option<String> {
    let w = words(goal);
    let off = |part: &str| off.iter().any(|o| o == part);
    if !has(&w, &PER_ITEM) {
        return None;
    }
    let recipe = |name: &str| !off("recipe") && !off(&format!("recipe:{name}"));
    let (n, src, file, urls) = items(goal, dir)?;
    let pick = tool_for(goal, specs, urls);
    let counted = format!("J4n: this task has {n} items in {src}");
    Some(if has(&w, &CHECK) && has(&w, &CHANGED) && recipe("checks") {
        format!("{counted}. The check on every changed file is one call: `j4n.recipe('checks', cmd='<the check> {{}}')`, which counts the ok and the failed.")
    } else if let (true, Some(t)) = (file && recipe("each"), pick.filter(|t| needs(t).len() == 1).map(|t| t.name.as_str())) {
        format!("{counted}, one `{t}` call each. One `py` cell can do them all: `j4n.recipe('each', tool='{t}', items='{src}')`, one `j4n.many` that counts the ok and the failed.")
    } else if off("many") {
        return None;
    } else {
        let (what, call) = pick.map(|t| t.name.as_str()).map_or((String::new(), "tool".to_string()), |t| (format!(", one `{t}` call each"), format!("'{t}'")));
        format!("{counted}{what}. One `py` cell can do them all: `j4n.many({call}, [{{...}} for x in items])`.")
    })
}
