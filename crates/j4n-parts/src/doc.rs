//! The reader of the operator's document: its parts are the rows of the first table with a `Gate` column, else
//! its `###` steps, else the rows of its first table. Nothing is read from anything but the file the operator named.

use serde::{Deserialize, Serialize};

/// The longest id of a part (an item id of the plan).
const ID_MAX: usize = 24;
/// The columns that hold a row's words, in the order they are looked for.
const WORDS: [&str; 6] = ["content", "step", "what", "part", "task", "title"];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Part {
    pub id: String,
    /// What the part says to do, as written.
    pub words: String,
    /// The gate cell as written (empty: none).
    pub gate: String,
    /// The gate's command: the first backticked span of the gate cell, as words (empty: the project's own check).
    pub cmd: Vec<String>,
}

impl Part {
    /// The first line of its words, without emphasis, at most 72 characters.
    pub fn title(&self) -> String {
        let line = plain(self.words.lines().next().unwrap_or_default());
        if line.chars().count() <= 72 { line } else { format!("{}…", line.chars().take(71).collect::<String>()) }
    }

    /// The gate in words: its command, else what the project checks with.
    pub fn gate_words(&self) -> String {
        if self.cmd.is_empty() { "the project's check".into() } else { format!("`{}`", self.cmd.join(" ")) }
    }
}

/// Emphasis, backticks and links' brackets out, spaces collapsed.
fn plain(s: &str) -> String {
    s.replace(['*', '`'], "").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A cell as an id: lower case, other characters `-`, at most [`ID_MAX`]; empty when nothing is left.
fn id_of(cell: &str) -> String {
    let raw: String = plain(cell).to_lowercase().chars().map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() { c } else { '-' }).collect();
    let id: String = raw.split('-').filter(|w| !w.is_empty()).collect::<Vec<_>>().join("-").chars().take(ID_MAX).collect();
    id.trim_end_matches('-').to_string()
}

/// The first backticked span of `cell`, as words.
fn command(cell: &str) -> Vec<String> {
    let mut it = cell.split('`');
    it.next();
    match (it.next(), it.next()) {
        (Some(span), Some(_)) => span.split_whitespace().map(String::from).collect(),
        _ => Vec::new(),
    }
}

/// The cells of a table line (`| a | b |`), or none when it is no table line.
fn cells(line: &str) -> Option<Vec<String>> {
    let t = line.trim();
    if !t.starts_with('|') {
        return None;
    }
    let inner = t.trim_start_matches('|');
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    // a `|` inside backticks is part of the cell
    let (mut out, mut cell, mut code) = (Vec::new(), String::new(), false);
    for c in inner.chars() {
        match c {
            '`' => {
                code = !code;
                cell.push(c);
            }
            '|' if !code => out.push(std::mem::take(&mut cell).trim().to_string()),
            _ => cell.push(c),
        }
    }
    out.push(cell.trim().to_string());
    Some(out)
}

fn rule_line(cells: &[String]) -> bool {
    !cells.is_empty() && cells.iter().all(|c| !c.is_empty() && c.chars().all(|x| matches!(x, '-' | ':' | ' ')))
}

/// The tables of a document: each a header and its rows.
fn tables(text: &str) -> Vec<(Vec<String>, Vec<Vec<String>>)> {
    let lines: Vec<&str> = text.lines().collect();
    let (mut all, mut i) = (Vec::new(), 0);
    while i + 1 < lines.len() {
        match (cells(lines[i]), cells(lines[i + 1])) {
            (Some(head), Some(rule)) if rule_line(&rule) => {
                let mut rows = Vec::new();
                i += 2;
                while let Some(row) = lines.get(i).and_then(|l| cells(l)) {
                    rows.push(row);
                    i += 1;
                }
                all.push((head.iter().map(|h| plain(h).to_lowercase()).collect(), rows));
            }
            _ => i += 1,
        }
    }
    all
}

fn from_table(head: &[String], rows: &[Vec<String>]) -> Vec<Part> {
    let gate = head.iter().position(|h| h.split_whitespace().any(|w| w == "gate"));
    let words = WORDS.iter().find_map(|w| head.iter().position(|h| h == w)).unwrap_or(1.min(head.len().saturating_sub(1)));
    let cell = |r: &Vec<String>, i: usize| r.get(i).cloned().unwrap_or_default();
    rows.iter()
        .map(|r| {
            let g = gate.map(|i| cell(r, i)).unwrap_or_default();
            Part { id: id_of(&cell(r, 0)), words: plain(&cell(r, words)), cmd: command(&g), gate: plain(&g) }
        })
        .filter(|p| !p.words.is_empty())
        .collect()
}

fn from_steps(text: &str) -> Vec<Part> {
    let (mut parts, mut code, mut open): (Vec<Part>, bool, bool) = (Vec::new(), false, false);
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            code = !code;
        }
        if !code && let Some(head) = line.strip_prefix("### ") {
            parts.push(Part { id: id_of(head.split_whitespace().next().unwrap_or_default()), words: plain(head), ..Part::default() });
            open = true;
            continue;
        }
        // a heading of another level ends the step
        if !code && line.starts_with('#') {
            open = false;
        }
        let Some(p) = parts.last_mut().filter(|_| open) else { continue };
        let t = line.trim();
        if let Some(g) = t.strip_prefix("Gate:").or_else(|| t.strip_prefix("gate:")) {
            (p.cmd, p.gate) = (command(g), plain(g));
        } else if !t.is_empty() {
            p.words.push('\n');
            p.words.push_str(t);
        }
    }
    parts
}

/// The parts of `text`, with ids made unique (`p<n>` for a part whose first cell gives none, or one taken).
pub fn read(text: &str) -> Vec<Part> {
    let tables = tables(text);
    let gated = tables.iter().find(|(h, _)| h.iter().any(|c| c.split_whitespace().any(|w| w == "gate")));
    let mut parts = match gated {
        Some((h, rows)) => from_table(h, rows),
        None => {
            let steps = from_steps(text);
            if steps.is_empty() { tables.first().map(|(h, r)| from_table(h, r)).unwrap_or_default() } else { steps }
        }
    };
    let mut seen = std::collections::BTreeSet::new();
    for (n, p) in parts.iter_mut().enumerate() {
        if p.id.is_empty() || !p.id.starts_with(|c: char| c.is_ascii_alphanumeric()) || seen.contains(&p.id) {
            p.id = format!("p{}", n + 1);
        }
        seen.insert(p.id.clone());
    }
    parts
}
