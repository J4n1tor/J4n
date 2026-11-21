//! The step `keep`: before a refactor, a snapshot of a surface the project chose — public names, test names,
//! doc headings and links, found by the expressions of `qc_keep` rows — and after it, what the snapshot had and the folder no longer has,
//! shown as marks (`removed: fn parse_x`). It reads files and compares sets; it asks no model and it never stops a flow.

use crate::change::{Change, text_of};
use crate::facts;
use crate::pattern::glob;
use crate::run::{Quality, short};
use j4n_connect::connect::{Job, StepDoer};
use j4n_core::doer::Doer;
use j4n_core::table::{Kind, Tables};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

pub const DOER: &str = "keep";
/// The pattern a removed item is marked as: it has no row, so it never steers.
pub const PATTERN: &str = "keep-removed";
/// A snapshot holds at most this many names from at most this many files; it says so when it was cut.
const MAX_ITEMS: usize = 20_000;
const MAX_FILES: usize = 5_000;

/// An entry of kind `qc_keep`: one kind of name to guard in the files it looks at.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Surface {
    pub id: String,
    #[serde(default)]
    pub doc: String,
    pub lang: Vec<String>,
    #[serde(default)]
    pub not: Vec<String>,
    /// The word in the mark: `fn`, `type`, `test`, `heading`, `link`.
    pub what: String,
    /// Run over a whole file with `(?m)`; the first group is the name (the whole match when there is none).
    #[serde(rename = "match")]
    pub pattern: String,
    /// The item is this name in this file, not the name anywhere: a heading moved to another file is gone.
    #[serde(default)]
    pub same_file: bool,
    #[serde(default)]
    pub stability: Option<String>,
}

impl Kind for Surface {
    const NAME: &'static str = "qc_keep";
}

/// A surface ready to be used.
#[derive(Clone)]
pub struct Rule {
    s: Surface,
    lang: Vec<Regex>,
    not: Vec<Regex>,
    re: Regex,
}

impl Rule {
    pub fn new(s: Surface) -> Result<Rule, String> {
        let globs = |list: &[String]| list.iter().map(|g| glob(g).ok_or_else(|| format!("`{g}` is not a glob"))).collect::<Result<Vec<_>, _>>();
        let (lang, not) = (globs(&s.lang)?, globs(&s.not)?);
        let re = Regex::new(&format!("(?m){}", s.pattern)).map_err(|e| format!("`match` is not a regular expression: {e}"))?;
        Ok(Rule { s, lang, not, re })
    }

    fn applies(&self, path: &str) -> bool {
        self.lang.iter().any(|g| g.is_match(path)) && !self.not.iter().any(|g| g.is_match(path))
    }
}

/// The surfaces of the Table (those `only` names, separated by commas; all when it is empty), and what was wrong with the others.
pub fn load(tables: &Tables, only: &str) -> (Vec<Rule>, Vec<String>) {
    let (mut ready, mut found) = (Vec::new(), Vec::new());
    let only: Vec<&str> = only.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    let ids = tables.ids(Surface::NAME);
    found.extend(only.iter().filter(|o| !ids.iter().any(|i| i == *o)).map(|o| format!("qc.keep names `{o}`, and there is no such surface")));
    for id in ids.iter().filter(|i| only.is_empty() || only.iter().any(|o| *o == **i)) {
        match tables.get::<Surface>(id).map_err(|p| p.to_string()).and_then(Rule::new) {
            Ok(r) => ready.push(r),
            Err(why) => found.push(format!("qc_keep {id}: {why}")),
        }
    }
    (ready, found)
}

/// One name: what it is, the name, the file, the line, and whether it belongs to the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item(pub String, pub String, pub String, pub u32, pub bool);

impl Item {
    fn key(&self) -> String {
        if self.4 { format!("{} {} @{}", self.0, self.1, self.2) } else { format!("{} {}", self.0, self.1) }
    }

    fn text(&self) -> String {
        format!("removed: {} {}", self.0, self.1)
    }
}

#[derive(Debug, Default)]
pub struct Snap {
    pub items: Vec<Item>,
    pub files: usize,
    pub cut: bool,
}

/// The surface of a folder as its files stand.
pub fn take(rules: &[Rule], dir: &Path) -> Snap {
    let mut snap = Snap::default();
    for rel in Change::listing(dir) {
        let hit: Vec<&Rule> = rules.iter().filter(|r| r.applies(&rel)).collect();
        if hit.is_empty() {
            continue;
        }
        let Some(text) = text_of(dir, &rel) else { continue };
        if snap.files >= MAX_FILES {
            snap.cut = true;
            break;
        }
        snap.files += 1;
        for r in hit {
            let (mut at, mut line) = (0usize, 1u32);
            for c in r.re.captures_iter(&text) {
                let Some(m) = c.get(1).or_else(|| c.get(0)).filter(|m| !m.as_str().trim().is_empty()) else { continue };
                line += text[at..m.start()].matches('\n').count() as u32;
                at = m.start();
                if snap.items.len() >= MAX_ITEMS {
                    snap.cut = true;
                    return snap;
                }
                snap.items.push(Item(r.s.what.clone(), m.as_str().trim().chars().take(120).collect(), rel.clone(), line, r.s.same_file));
            }
        }
    }
    snap
}

/// What `before` has that `now` no longer has; a name found twice before and once now is one gone.
pub fn lost<'a>(before: &'a [Item], now: &[Item]) -> Vec<&'a Item> {
    let mut have: BTreeMap<String, usize> = BTreeMap::new();
    for i in now {
        *have.entry(i.key()).or_default() += 1;
    }
    before
        .iter()
        .filter(|i| match have.get_mut(&i.key()) {
            Some(n) if *n > 0 => {
                *n -= 1;
                false
            }
            _ => true,
        })
        .collect()
}

fn is_id(w: &str) -> bool {
    w.len() == 9 && w.starts_with('k') && w[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

impl Quality {
    /// The doer `keep`, a step of a flow: `snapshot`, then `compare <snapshot>`.
    pub fn keeper(&self) -> Arc<dyn Doer> {
        let me = self.clone();
        Arc::new(StepDoer { id: DOER, doc: "keeps a surface across a refactor: the names, tests and headings before, and what is gone after, as marks", work: Arc::new(move |job| me.keep(job)) })
    }

    fn keep(&self, job: &Job) -> Result<String, String> {
        if self.cfg.keeps.is_empty() {
            return Err("there is no surface to keep: no `qc_keep` row is left (see `qc.keep`)".into());
        }
        let (dir, thread) = (&job.cx.site.dir, job.cx.spec.thread.as_deref());
        let words: Vec<&str> = job.goal.split_whitespace().collect();
        let (cut, now) = match words.first() {
            Some(&"snapshot") => (None, take(&self.cfg.keeps, dir)),
            Some(&"compare") => {
                let id = words.iter().map(|w| w.trim_end_matches(':')).find(|w| is_id(w)).ok_or("`compare` names a snapshot: `compare k1a2b3c4`")?;
                self.log.sync();
                let was = self.log.read(0).filter(|e| e.kind == "qc.keep" && e.data["keep"] == id && e.data["step"] == "snapshot").last().ok_or_else(|| format!("there is no snapshot `{id}`"))?;
                let items: Vec<Item> = serde_json::from_value(was.data["items"].clone()).map_err(|e| format!("snapshot `{id}` does not read: {e}"))?;
                (Some((id.to_string(), items, was.data["cut"] == true)), take(&self.cfg.keeps, dir))
            }
            _ => return Err("`keep` takes `snapshot`, or `compare <snapshot>`".into()),
        };
        let size = format!("{} names in {} files{}", now.items.len(), now.files, if now.cut { ", cut" } else { "" });
        let Some((id, before, was_cut)) = cut else {
            let id = short('k', &[&serde_json::to_string(&now.items).unwrap_or_default()]);
            self.put("qc.keep", json!({ "keep": id, "step": "snapshot", "files": now.files, "n": now.items.len(), "cut": now.cut, "items": now.items }), Some(&job.cx.task), thread);
            return Ok(format!("kept {id}: {size}"));
        };
        let facts = facts::read(&self.log, None);
        let (prec, n) = facts.precision(PATTERN);
        let (mut shown, mut seen) = (Vec::new(), Vec::<(String, u32)>::new());
        for i in lost(&before, &now.items) {
            if seen.contains(&(i.2.clone(), i.3)) {
                continue;
            }
            seen.push((i.2.clone(), i.3));
            let mark = short('m', &[PATTERN, &i.2, &i.3.to_string()]);
            let dismissed = facts.dismissed.contains(&mark);
            let data = json!({ "mark": mark, "run": job.cx.task, "live": false, "pattern": PATTERN, "title": i.text(), "file": i.2, "line": i.3, "p": 1.0, "severity": "mid", "tier": 0, "answer": i.text(), "steer": "", "steerable": false, "prec": prec, "n": n, "state": if dismissed { "dismissed" } else { "open" } });
            self.put("qc.mark", data, Some(&job.cx.task), thread);
            if !dismissed {
                shown.push(format!("{PATTERN}  {}:{} mid  1.00  {}", i.2, i.3, i.text()));
            }
        }
        self.put("qc.keep", json!({ "keep": id, "step": "compare", "files": now.files, "n": now.items.len(), "cut": now.cut, "removed": seen.len() }), Some(&job.cx.task), thread);
        let note = if was_cut || now.cut { " · the snapshot was cut, so a name past the cut is not guarded" } else { "" };
        shown.push(if seen.is_empty() { format!("keep: nothing is gone of the {} names kept ({size}){note}", before.len()) } else { format!("keep: {} of {} names gone ({size}){note}", seen.len(), before.len()) });
        Ok(shown.join("\n"))
    }
}
