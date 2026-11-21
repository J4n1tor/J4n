//! The project's glossary: every Markdown table row of the project's docs whose first cell is an id
//! (`ADR-12`, `BUG-7`, `UX3`) with the first sentence of its meaning. The screen draws a known id in an answer with those words, and `:term`
//! opens the row. Read once, kept, and read again only when a doc changed (its time or size); nothing here is per frame.

use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// How many columns the words beside an id take at most.
pub const WIDTH: usize = 40;
const FILES: usize = 300;
const BYTES: u64 = 2_000_000;
const DEPTH: usize = 3;
const TERMS: usize = 6_000;
/// How much of a row `:term` shows.
const CELL: usize = 600;
/// A column of a table that says who or when and not what: the words come from the next one.
const NOT_WORDS: [&str; 7] = ["found by", "found", "date", "when", "who", "by", "status"];

/// What a doc says an id is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Term {
    /// The first sentence of the cell, without marks, cut to [`WIDTH`] columns.
    pub words: String,
    /// The doc and the line of the row (`docs/GUIDE.md:120`).
    pub at: String,
    /// The cell as written, cut: what `:term` shows.
    pub cell: String,
}

#[derive(Default)]
pub struct Terms {
    map: BTreeMap<String, Term>,
    /// The docs the map was made from, with their time and size.
    stamp: Vec<(PathBuf, Option<SystemTime>, u64)>,
    loaded: bool,
}

/// Whether a word is an id of the project's docs: one to three capitals, an optional dash, digits.
pub fn is_id(w: &str) -> bool {
    let caps = w.bytes().take_while(u8::is_ascii_uppercase).count();
    let rest = w[caps..].strip_prefix('-').unwrap_or(&w[caps..]);
    (1..=3).contains(&caps) && !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()) && w.len() <= 12
}

/// The cells of a table row; a bar inside a code span or after a backslash is not a cell's end.
fn cells(row: &str) -> Vec<String> {
    let (mut out, mut cur, mut code, mut esc) = (Vec::new(), String::new(), false, false);
    for ch in row.trim().trim_start_matches('|').chars() {
        match ch {
            _ if esc => {
                cur.push(ch);
                esc = false;
            }
            '\\' => esc = true,
            '`' => {
                code = !code;
                cur.push(ch);
            }
            '|' if !code => out.push(std::mem::take(&mut cur).trim().to_string()),
            _ => cur.push(ch),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// A cell as the words of one line, the marks of Markdown taken out: `**` and backticks, a link as its words, a line break as a blank.
fn plain(cell: &str) -> String {
    let (mut out, mut rest) = (String::new(), cell.replace("<br>", " ").replace("<br/>", " "));
    while let Some(open) = rest.find('[') {
        let linked = rest[open..].find("](").and_then(|m| rest[open + m..].find(')').map(|e| (m, e)));
        let Some((m, e)) = linked else { break };
        out.push_str(&rest[..open]);
        out.push_str(&rest[open + 1..open + m]);
        rest = rest[open + m + e + 1..].to_string();
    }
    out.push_str(&rest);
    out.replace("**", "").replace('`', "").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The first sentence of a cell, no longer than `WIDTH` columns (the last one an ellipsis when it was cut). A cell that opens with a bold
/// phrase is a title: the phrase is the sentence.
pub fn sentence(cell: &str) -> String {
    let cell = cell.trim();
    let bold = cell.strip_prefix("**").and_then(|r| r.find("**").map(|e| &r[..e])).filter(|b| !b.trim().is_empty());
    let text = plain(bold.unwrap_or(cell));
    let mut end = text.len();
    for (i, _) in text.match_indices(". ") {
        let word = text[..i].rsplit(' ').next().unwrap_or("");
        if !["e.g", "i.e", "vs", "etc", "cf"].contains(&word) {
            end = i;
            break;
        }
    }
    let first = text[..end].trim().trim_end_matches(['.', ':', ';', ',']);
    if first.chars().count() <= WIDTH {
        return first.to_string();
    }
    let cut: String = first.chars().take(WIDTH - 1).collect();
    format!("{}…", cut.trim_end())
}

/// The ids a doc defines, in its order: a row of a table whose first cell is an id, and the first sentence of the first cell after it that
/// is words (a column that says who found it or when is passed over). `file` is how the doc is named in `at`.
pub fn scan(file: &str, text: &str) -> Vec<(String, Term)> {
    let (mut out, mut head, mut fence) = (Vec::new(), None::<Vec<String>>, false);
    for (n, line) in text.lines().enumerate() {
        let t = line.trim();
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = !fence;
        }
        if fence || !t.starts_with('|') {
            head = None;
            continue;
        }
        let row = cells(t);
        // the first row of a table is its header; the second is the rule under it
        let Some(titles) = head.as_ref() else {
            head = Some(row.iter().map(|c| plain(c).to_lowercase()).collect());
            continue;
        };
        let id = row.first().map(|c| plain(c)).filter(|c| is_id(c));
        let (Some(id), true) = (id, !row.iter().all(|c| c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))) else { continue };
        let pick = (1..row.len()).filter(|i| titles.get(*i).is_none_or(|h| !NOT_WORDS.contains(&h.as_str()))).find_map(|i| Some((i, sentence(&row[i]))).filter(|(_, s)| !s.is_empty()));
        if let Some((i, words)) = pick {
            out.push((id, Term { words, at: format!("{file}:{}", n + 1), cell: row[i].chars().take(CELL).collect() }));
        }
    }
    out
}

/// The docs that say what an id is before any other doc does: a project's decision log and bug log are where `D-` and `J` ids are
/// made; plans and test tables only mention them.
const FIRST: [&str; 2] = ["docs/dev/DECISIONS.md", "docs/dev/BUGS.md"];

/// The docs of a project folder: its Markdown files, and those under `docs/` to a few levels, [`FIRST`] first and then in the order of their names.
fn docs(root: &Path) -> Vec<(PathBuf, String)> {
    fn walk(dir: &Path, name: &str, depth: usize, out: &mut Vec<(PathBuf, String)>) {
        let Ok(list) = std::fs::read_dir(dir) else { return };
        for e in list.flatten() {
            let (path, file) = (e.path(), e.file_name().to_string_lossy().to_string());
            let shown = if name.is_empty() { file.clone() } else { format!("{name}/{file}") };
            let Ok(kind) = e.file_type() else { continue };
            if file.starts_with('.') || kind.is_symlink() {
                continue;
            }
            if kind.is_dir() && depth < DEPTH && name.starts_with("docs") && file != "target" && file != "node_modules" {
                walk(&path, &shown, depth + 1, out);
            } else if kind.is_file() && file.ends_with(".md") {
                out.push((path, shown));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, "", 0, &mut out);
    walk(&root.join("docs"), "docs", 1, &mut out);
    let rank = |name: &str| FIRST.iter().position(|f| *f == name).unwrap_or(FIRST.len());
    out.sort_by(|a, b| (rank(&a.1), &a.1).cmp(&(rank(&b.1), &b.1)));
    out.dedup_by(|a, b| a.1 == b.1);
    out.truncate(FILES);
    out
}

impl Terms {
    /// Read the docs of `root` again if any changed (or none was read yet). Whether the glossary is not what it was.
    pub fn refresh(&mut self, root: &Path) -> bool {
        let root = if root.as_os_str().is_empty() { Path::new(".") } else { root };
        let files = docs(root);
        let stamp: Vec<_> = files.iter().filter_map(|(p, _)| std::fs::metadata(p).ok().map(|m| (p.clone(), m.modified().ok(), m.len()))).collect();
        if self.loaded && stamp == self.stamp {
            return false;
        }
        let mut map = BTreeMap::new();
        for (path, shown) in &files {
            if std::fs::metadata(path).is_ok_and(|m| m.len() > BYTES) {
                continue;
            }
            for (id, term) in scan(shown, &std::fs::read_to_string(path).unwrap_or_default()) {
                if map.len() < TERMS {
                    map.entry(id).or_insert(term);
                }
            }
        }
        let changed = !self.loaded || map != self.map;
        (self.map, self.stamp, self.loaded) = (map, stamp, true);
        changed
    }

    /// `{id: words}`, what the screen is sent.
    pub fn json(&self) -> Json {
        Json::Object(self.map.iter().map(|(k, t)| (k.clone(), json!(t.words))).collect())
    }

    pub fn get(&self, id: &str) -> Option<&Term> {
        self.map.get(id)
    }
}

/// What `:term` says of ids: each with its words, where it stands, and its row as written.
pub fn said(terms: &Terms, ids: &[String]) -> String {
    ids.iter()
        .map(|id| match terms.get(id) {
            Some(t) => format!("**{id}** · {}\n{}\n{}", t.words, t.at, t.cell),
            None => format!("{id}: no table row of the project's docs defines it"),
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

impl super::Front {
    /// The glossary as the screen is sent it: `null` when `read.terms` is off, so that nothing is read.
    pub fn terms_value(&self) -> Json {
        if !self.look_spoken().read.terms {
            return Json::Null;
        }
        let mut terms = self.0.terms.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        terms.refresh(&self.0.cfg().root);
        terms.json()
    }

    /// Whether a doc changed since the glossary was read (the screen is sent it again if so).
    pub fn terms_changed(&self) -> bool {
        self.look_spoken().read.terms && self.0.terms.lock().unwrap_or_else(std::sync::PoisonError::into_inner).refresh(&self.0.cfg().root)
    }

    /// `:term ADR-12 BUG-7`: the rows those ids stand for.
    pub(super) fn term_said(&self, ids: &[String]) -> Result<String, String> {
        if ids.is_empty() {
            return Err("`:term ADR-12`: say which id".into());
        }
        let mut terms = self.0.terms.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        terms.refresh(&self.0.cfg().root);
        Ok(said(&terms, ids))
    }
}
