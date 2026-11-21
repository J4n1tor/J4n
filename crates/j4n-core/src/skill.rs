//! Skills: a folder with a `SKILL.md`, whose frontmatter says its `name`, `when` it applies and what it `cost`s
//! in tokens. They live in the J4n home (`<home>/skills/<name>/SKILL.md`) and in packs (`<pack>/skills/…`); a pack's are carried in the binary
//! or read from where the pack is installed. J4n **picks** the few that fit a task, locally and with no model: by the words of the line, the
//! files it names and the kind of task, up to a count and a token cap, so that only those go into that doer's Brief (or, for a doer that takes
//! none, into a folder of its own for the task).
//!
//! `where` (optional) is a path that must be in the project's folder: a skill with one is picked only there (the shipped skills are
//! about J4n's own repository and say `where: crates/j4n-core/Cargo.toml`). `when` is a list, split at `,` and `;`: a **word** (`tests`), a **file** (`*.rs`, `Cargo.toml`, `docs/`) or `kind:<word>` (`kind:write`, `kind:read`,
//! `kind:review`). A word or a file must hit for a skill to be picked at all; the kind only ranks one that hit. A short line in a going thread
//! picks none ([`crate::brief::cards`]).

use crate::brief::{self, Card};
use crate::value::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Mutex;

/// The most a skill's file may hold (bytes): longer is cut at a line.
const MAX_BYTES: usize = 16_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub id: String,
    pub when: String,
    /// What it takes of a doer's context, in tokens (the frontmatter's `cost`, else measured).
    pub cost: usize,
    pub body: String,
    /// `home`, or `pack:<id>`.
    pub from: String,
    /// The frontmatter's `where`: a path that must be in the project's folder for the skill to be picked there (`crates/j4n-core/Cargo.toml`
    /// for the skills about J4n's own repository). None: it fits every project.
    pub here: Option<String>,
    /// The frontmatter's `description`: one plain sentence saying what the skill is for and when to read it, which a doer that lists skills by their
    /// description (claude) is shown. Empty: the list of `when` words is all it has, which reads as a keyword soup and was never opened.
    pub description: String,
}

/// A `SKILL.md`: `---`, `key: value` lines (`name`, `when`, `cost`, and optionally `where` and `description`), `---`, then the words. `folder` is the name when the frontmatter has none.
pub fn parse(from: &str, folder: &str, text: &str) -> Result<Skill, String> {
    let rest = text.strip_prefix("---\n").ok_or("it starts with `---` and the frontmatter")?;
    let (head, body) = rest.split_once("\n---").ok_or("the frontmatter ends with `---`")?;
    let body = body.trim_start_matches('-').trim();
    let get = |k: &str| head.lines().find_map(|l| l.strip_prefix(k).and_then(|r| r.strip_prefix(':')).map(|v| v.trim().trim_matches('"').to_string()));
    let id = get("name").unwrap_or_else(|| folder.to_string());
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return Err(format!("`{id}` is not a skill name (lowercase letters, digits, `-`)"));
    }
    let when = get("when").filter(|w| !w.is_empty()).ok_or("`when` says what it is for")?;
    let body: String = if body.len() > MAX_BYTES { body[..body.floor_char_boundary(MAX_BYTES)].rsplit_once('\n').map_or("", |(a, _)| a).into() } else { body.into() };
    let cost = get("cost").and_then(|c| c.parse().ok()).unwrap_or_else(|| brief::tokens(&body));
    let here = get("where").filter(|w| !w.is_empty());
    if let Some(w) = &here
        && (w.starts_with(['/', '~']) || w.split('/').any(|c| c == ".."))
    {
        return Err(format!("`where: {w}` is a path inside the project"));
    }
    let description = get("description").unwrap_or_default();
    Ok(Skill { id, when, cost, body, from: from.into(), here, description })
}

/// The skill of a connection, made from its connector row and nothing else: what it is for, how J4n speaks to it, what the Gate does, what to expect
/// of it. `None` for a row that is not an agent or a tool that runs a program (`test`, `builtin` and the model dialects have none). The words are those
/// of the row, so a row the operator accepted and the skill it has cannot disagree; an operator's own skill of the same name replaces it.
pub fn of_connector(id: &str, f: &toml::Table) -> Option<Skill> {
    let text = |k: &str| f.get(k).and_then(toml::Value::as_str).unwrap_or_default();
    let (is, dialect, program) = (text("is"), text("dialect"), text("program"));
    if !matches!(is, "agent" | "tool") || program.is_empty() || matches!(dialect, "test" | "builtin") {
        return None;
    }
    let base = Path::new(program).file_name().map_or(program.to_string(), |n| n.to_string_lossy().into_owned());
    let name: String = format!("connector-{id}").chars().map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' { c } else if c.is_ascii_uppercase() { c.to_ascii_lowercase() } else { '-' }).collect();
    let has = |k: &str| f.get(k).is_some_and(|v| v.as_array().is_none_or(|a| !a.is_empty()) && v.as_table().is_none_or(|t| !t.is_empty()));
    let doc = text("doc");
    let mut lines: Vec<String> = Vec::new();
    let description;
    if is == "agent" {
        description = format!("Read before a task that is to run on the agent `{id}` (`{base}`) or that names it: how J4n speaks to it and what to expect of it.");
        let prompt = if has("serve") {
            "posted to a local server it starts"
        } else if has("stdin") {
            "on its standard input"
        } else if f.get("env").and_then(toml::Value::as_table).is_some_and(|t| t.values().any(|v| v.as_str().is_some_and(|s| s.contains("{prompt}")))) {
            "in an environment variable"
        } else {
            "as an argument"
        };
        // the live protocols keep a session open between lines; the others are given `resume` to carry on
        let session = matches!(dialect, "acp" | "pi-rpc" | "claude-stream") || has("resume") || f.get("serve").and_then(toml::Value::as_table).is_some_and(|t| t.contains_key("resume"));
        let tools = match dialect {
            "acp" | "pi-rpc" | "claude-stream" => true,
            _ => f.get("map").and_then(toml::Value::as_table).and_then(|m| m.get("events")).and_then(toml::Value::as_array).is_some_and(|a| !a.is_empty()),
        };
        lines.push(format!("- use: `--doer {id}`, or name it in the line (`use {id} …`); it is offered for {}", f.get("roles").and_then(toml::Value::as_array).filter(|r| !r.is_empty()).map_or("every role".to_string(), |r| format!("the role {}", r.iter().filter_map(toml::Value::as_str).collect::<Vec<_>>().join(", ")))));
        lines.push(format!("- prompt: {prompt}"));
        lines.push(format!("- session: {}", if session { "a later line carries on in its session" } else { "it keeps no session between lines" }));
        lines.push(format!("- tools: {}", if tools { "its tool calls are shown as tool rows" } else { "its tool calls are not shown; what it says is the answer" }));
        lines.push(format!("- gate: {}", if f.get("gated").and_then(toml::Value::as_bool) != Some(true) { "what it asks to write goes to the Gate first, as for any doer" } else { "each run goes to the Gate first in write mode, and it may write only where the Gate says yes" }));
    } else {
        let argv = f.get("argv").and_then(toml::Value::as_array).map(|a| a.iter().filter_map(toml::Value::as_str).skip(1).collect::<Vec<_>>().join(" ")).unwrap_or_default();
        description = format!("Read before a task that needs `{base}`{}: what the tool `{id}` takes, what running it can do, and how its answer is read.", if argv.is_empty() { String::new() } else { format!(" (`{base} {argv}`)") });
        lines.push(format!("- use: it is a tool; a model calls `{id}` with {}", {
            let names: Vec<String> = f.get("args").and_then(toml::Value::as_array).map(|a| a.iter().filter_map(|x| x.get("name").and_then(toml::Value::as_str)).map(|n| format!("`{n}`")).collect()).unwrap_or_default();
            if names.is_empty() { "no arguments".to_string() } else { names.join(", ") }
        }));
        let risk = if text("risk").is_empty() { "exec" } else { text("risk") };
        lines.push(format!("- risk: {risk}{}", if text("says").is_empty() || text("says") == risk { String::new() } else { format!(" (its own words say {})", text("says")) }));
        lines.push(format!("- gate: every run goes to the Gate first; a `{risk}` run {}", if matches!(risk, "read") { "is allowed where the Gate allows a read" } else { "asks before it runs" }));
        if f.get("net").and_then(toml::Value::as_bool) == Some(false) {
            lines.push("- network: cut where a box can cut it".into());
        }
    }
    if let Some(words) = f.get("decode").and_then(toml::Value::as_array).map(|a| a.iter().filter_map(toml::Value::as_str).collect::<Vec<_>>()).filter(|w| !w.is_empty()) {
        lines.push(format!("- output: each line is read as {} first", words.join(", then ")));
    }
    lines.push(if has("limits") { "- limit: its own usage limit is read, and a turn that meets it ends with a line that says when it lifts".into() } else { "- limit: its usage limit is not read; when it says it is out, the turn ends with its words".into() });
    // the first line is what a list of skills shows: the id and what the row says it is
    let body = format!("# {id}: {}\n\n{}\n", if doc.is_empty() { format!("`{program}`, connected as {} {is}.", if is == "agent" { "an" } else { "a" }) } else { doc.to_string() }, lines.join("\n"));
    let mut when = vec![id.to_lowercase()];
    if base.to_lowercase() != id.to_lowercase() {
        when.push(base.to_lowercase());
    }
    Some(Skill { id: name, when: when.join(", "), cost: brief::tokens(&body), body, from: format!("connector:{id}"), here: None, description })
}

/// What `when` says, in three lists.
fn triggers(when: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
    let (mut words, mut files, mut kinds) = (Vec::new(), Vec::new(), Vec::new());
    for t in when.split([',', ';']).map(|t| t.trim().to_lowercase()).filter(|t| !t.is_empty()) {
        if let Some(k) = t.strip_prefix("kind:") {
            kinds.push(k.trim().to_string());
        } else if !t.contains(' ') && t.contains(['.', '/', '*']) {
            files.push(t);
        } else {
            words.extend(t.split_whitespace().map(String::from));
        }
    }
    (words, files, kinds)
}

impl Skill {
    /// The skill as a card of the Brief: its words, the plural of each too.
    pub fn card(&self) -> Card {
        let words = triggers(&self.when).0.into_iter().flat_map(|w| [format!("{w}s"), w]).collect();
        Card { id: self.id.clone(), words, text: self.body.clone() }
    }

    /// The first line of its words: what it is, for a list.
    pub fn about(&self) -> &str {
        self.body.lines().map(|l| l.trim_start_matches(['#', ' '])).find(|l| !l.trim().is_empty()).unwrap_or_default()
    }
}

/// A file pattern against a path: `*.rs` the ending, `docs/` a folder, else the file's name.
fn file_hit(pat: &str, path: &str) -> bool {
    let path = path.trim_start_matches("./").to_lowercase();
    match pat.strip_prefix('*') {
        Some(end) => path.ends_with(end),
        None if pat.ends_with('/') => path.starts_with(pat) || path.contains(&format!("/{pat}")),
        None => path == pat || path.ends_with(&format!("/{pat}")),
    }
}

/// What chooses among skills that tie: given their ids and first lines, the id to take first, if any.
pub type Chooser<'a> = dyn FnMut(&[(String, String)]) -> Option<String> + 'a;

/// What a pick is asked for: the line, the files the task names or touches, its kind (`write`, `read`, `plan`…), and the caps.
pub struct Ask<'a> {
    pub line: &'a str,
    pub files: Vec<String>,
    pub kinds: Vec<String>,
    pub in_session: bool,
    pub max: usize,
    pub tokens: usize,
}

/// A skill that was picked, and why in words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picked {
    pub id: String,
    pub why: String,
    pub tokens: usize,
}

/// The skills there are, and which the operator switched off.
pub struct Library {
    skills: Vec<Skill>,
    off: Mutex<BTreeSet<String>>,
    /// The project's folder: what a skill's `where` is looked for in. None: a skill with a `where` is never picked.
    root: Option<std::path::PathBuf>,
}

fn ids(list: &str) -> BTreeSet<String> {
    list.split([',', ' ']).map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect()
}

impl Library {
    /// `skills` in the order of the layers below to above: a later one of the same name replaces an earlier one. `off` is the setting `skills.off`.
    pub fn new(skills: Vec<Skill>, off: &str) -> Library {
        let mut by: BTreeMap<String, Skill> = BTreeMap::new();
        for s in skills {
            by.insert(s.id.clone(), s);
        }
        Library { skills: by.into_values().collect(), off: Mutex::new(ids(off)), root: None }
    }

    /// The library of a project in `root`: a skill whose `where` is not in it is not picked there.
    pub fn in_project(mut self, root: &Path) -> Library {
        self.root = Some(root.to_path_buf());
        self
    }

    fn fits(&self, s: &Skill) -> bool {
        s.here.as_ref().is_none_or(|w| self.root.as_ref().is_some_and(|r| r.join(w).exists()))
    }

    /// The skills under `dir` (`<dir>/<name>/SKILL.md`), and what was wrong with the ones that did not read.
    pub fn read_dir(from: &str, dir: &Path) -> (Vec<Skill>, Vec<String>) {
        let (mut found, mut bad) = (Vec::new(), Vec::new());
        let mut names: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().filter_map(Result::ok).filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).collect();
        names.sort_by_key(|e| e.file_name());
        for e in names {
            let (name, file) = (e.file_name().to_string_lossy().into_owned(), e.path().join("SKILL.md"));
            let Ok(text) = std::fs::read_to_string(&file) else { continue };
            match parse(from, &name, &text) {
                Ok(s) => found.push(s),
                Err(why) => bad.push(format!("{}: {why}", file.display())),
            }
        }
        (found, bad)
    }

    pub fn all(&self) -> &[Skill] {
        &self.skills
    }

    pub fn get(&self, id: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.id == id)
    }

    fn off(&self) -> std::sync::MutexGuard<'_, BTreeSet<String>> {
        self.off.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn is_on(&self, id: &str) -> bool {
        !self.off().contains(id)
    }

    /// The setting `skills.off` as it stands again (a change was taken back).
    pub fn set_off(&self, list: &str) {
        *self.off() = ids(list);
    }

    /// Switch one on or off; the setting's text to keep, else what is wrong.
    pub fn switch(&self, id: &str, on: bool) -> Result<String, String> {
        if self.get(id).is_none() {
            return Err(format!("there is no skill `{id}` (`:skill` lists them)"));
        }
        let mut off = self.off();
        if on { off.remove(id) } else { off.insert(id.into()) };
        Ok(off.iter().cloned().collect::<Vec<_>>().join(","))
    }

    /// The skills that match `words`, by their name, their `when` and their first line.
    pub fn search(&self, words: &str) -> Vec<&Skill> {
        let said = brief::said(words);
        self.skills.iter().filter(|s| said.iter().all(|w| format!("{} {} {}", s.id, s.when, s.about()).to_lowercase().contains(w.as_str()))).collect()
    }

    /// The few skills that fit: those the line's words or the task's files hit, the best first, no more than `max` and no more than `tokens` of them.
    pub fn pick(&self, a: &Ask) -> Vec<Picked> {
        self.pick_by(a, &mut |_| None)
    }

    /// [`Library::pick`], where code cannot choose: when the skills that tie at the last place that fits are more than the places left, `choose` is
    /// given them (the id and the first line of each) and may name the one to take first; the rest follow in the order code gave them.
    /// A name that is not one of them, or none, leaves the pick as code made it. `choose` is not called when code can choose.
    pub fn pick_by(&self, a: &Ask, choose: &mut Chooser) -> Vec<Picked> {
        if a.in_session && brief::is_short(a.line) {
            return Vec::new();
        }
        let on: Vec<&Skill> = self.skills.iter().filter(|s| self.is_on(&s.id) && self.fits(s)).collect();
        let (cards, said) = (on.iter().map(|s| s.card()).collect::<Vec<_>>(), brief::said(a.line));
        let by_word: Vec<&Card> = brief::cards(a.line, false, &cards);
        let mut scored: Vec<(usize, &Skill, String)> = Vec::new();
        for s in on {
            let (words, files, kinds) = triggers(&s.when);
            let word_ok = by_word.iter().any(|c| c.id == s.id);
            let hit_w: Vec<&String> = words.iter().filter(|w| word_ok && (said.contains(w) || said.contains(&format!("{w}s")))).collect();
            let hit_f: Vec<&String> = files.iter().filter(|p| a.files.iter().any(|f| file_hit(p, f))).collect();
            if hit_w.is_empty() && hit_f.is_empty() {
                continue;
            }
            let hit_k: Vec<&String> = kinds.iter().filter(|k| a.kinds.contains(k)).collect();
            let list = |name: &str, v: &[&String]| (!v.is_empty()).then(|| format!("{name} {}", v.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
            let why = [list("said", &hit_w), list("file", &hit_f), list("kind", &hit_k)].into_iter().flatten().collect::<Vec<_>>().join("; ");
            scored.push((2 * hit_w.len().min(3) + 3 * hit_f.len().min(2) + hit_k.len(), s, why));
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cost.cmp(&b.1.cost)).then(a.1.id.cmp(&b.1.id)));
        // the skills that tie at the place the cut falls: code ordered them by cost and name, which is no reason to take one and leave another
        if let Some(edge) = scored.get(a.max.saturating_sub(1)).map(|s| s.0).filter(|_| a.max > 0 && scored.len() > a.max) {
            let first = scored.iter().position(|s| s.0 == edge).unwrap_or(0);
            let tied = scored.iter().filter(|s| s.0 == edge).count();
            if tied > 1 && first + tied > a.max {
                let group: Vec<(String, String)> = scored[first..first + tied].iter().map(|s| (s.1.id.clone(), s.1.about().to_string())).collect();
                if let Some(at) = choose(&group).and_then(|id| scored[first..first + tied].iter().position(|s| s.1.id == id)) {
                    scored[first..first + tied].rotate_left(at);
                    // only the chosen one moves up; the others keep code's order
                    scored[first..first + tied][1..].sort_by(|a, b| a.1.cost.cmp(&b.1.cost).then(a.1.id.cmp(&b.1.id)));
                }
            }
        }
        let (mut out, mut used) = (Vec::new(), 0);
        for (_, s, why) in scored {
            if out.len() < a.max && used + s.cost <= a.tokens {
                used += s.cost;
                out.push(Picked { id: s.id.clone(), why, tokens: s.cost });
            }
        }
        out
    }

    /// The rows of the Skills page.
    pub fn rows(&self) -> Vec<Value> {
        self.skills
            .iter()
            .map(|s| {
                let t = |v: &str| Value::text(v);
                Value::Map(BTreeMap::from([
                    ("id".to_string(), t(&s.id)),
                    ("state".into(), t(if self.is_on(&s.id) { "on" } else { "off" })),
                    ("flip".into(), t(if self.is_on(&s.id) { "off" } else { "on" })),
                    ("from".into(), t(&s.from)),
                    ("when".into(), t(&s.when)),
                    ("about".into(), t(s.about())),
                    ("tokens".into(), Value::Num(s.cost as f64)),
                    ("body".into(), t(&s.body)),
                ]))
            })
            .collect()
    }
}
