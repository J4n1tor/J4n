//! The kind `pattern`: what an agent leaves behind that the project's own checks do not catch. A row
//! says where it applies (`lang`, `not`), how it is found (`regex` over added lines, `cmd` over the changed files, `jev` by a question
//! to Jev), how bad it is, and the fixed words that steer a lane away from it. Its numbers (precision, labels) live in the Log, never
//! here: the file is the operator's.

use j4n_core::table::{Kind, Tables};
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Way {
    Regex,
    Cmd,
    Jev,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Matches a hunk adds that its removed lines did not already hold.
    #[default]
    Added,
    /// Matches a hunk removes that it did not add back.
    Removed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Low,
    Mid,
    High,
}

impl Severity {
    pub fn word(self) -> &'static str {
        match self {
            Severity::Low => "low",
            Severity::Mid => "mid",
            Severity::High => "high",
        }
    }
}

/// An entry of kind `pattern`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pattern {
    pub id: String,
    #[serde(default)]
    pub doc: String,
    pub title: String,
    pub lang: Vec<String>,
    #[serde(default)]
    pub not: Vec<String>,
    pub kind: Way,
    /// `choice`: the question names the existing helpers it may repeat, and `none`.
    #[serde(default, rename = "type")]
    pub ty: Option<String>,
    #[serde(default)]
    pub scope: Scope,
    #[serde(default)]
    pub multiline: bool,
    #[serde(default, rename = "match")]
    pub pattern: Option<String>,
    #[serde(default)]
    pub argv: Vec<String>,
    #[serde(default)]
    pub instructions: Option<String>,
    pub severity: Severity,
    pub steer: String,
    pub bad: String,
    pub good: String,
    #[serde(default)]
    pub stability: Option<String>,
}

impl Kind for Pattern {
    const NAME: &'static str = "pattern";
}

/// A glob over paths, as a regex: `*.rs` is any file of that name in any folder, `tests/**` anything under `tests`, `**/x.toml` the
/// file in any folder (the root's too). A glob with no slash is matched against the file's name.
pub fn glob(text: &str) -> Option<Regex> {
    let mut out = String::from("^");
    if !text.contains('/') {
        out.push_str("(?:.*/)?");
    }
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' if chars.get(i + 1) == Some(&'*') => {
                if chars.get(i + 2) == Some(&'/') {
                    out.push_str("(?:.*/)?");
                    i += 3;
                } else {
                    out.push_str(".*");
                    i += 2;
                }
                continue;
            }
            '*' => out.push_str("[^/]*"),
            '?' => out.push_str("[^/]"),
            c => out.push_str(&regex::escape(&c.to_string())),
        }
        i += 1;
    }
    out.push('$');
    Regex::new(&out).ok()
}

/// A pattern ready to be used: its globs and its expression compiled once.
#[derive(Clone)]
pub struct Ready {
    pub p: Pattern,
    lang: Vec<Regex>,
    not: Vec<Regex>,
    pub re: Option<Regex>,
}

impl Ready {
    /// Why a pattern cannot be used, or the pattern made ready.
    pub fn new(p: Pattern) -> Result<Ready, String> {
        let globs = |list: &[String]| list.iter().map(|g| glob(g).ok_or_else(|| format!("`{g}` is not a glob"))).collect::<Result<Vec<_>, _>>();
        let (lang, not) = (globs(&p.lang)?, globs(&p.not)?);
        let re = match (p.kind, &p.pattern) {
            (Way::Regex, Some(m)) => {
                let src = if p.multiline { format!("(?m){m}") } else { m.clone() };
                Some(Regex::new(&src).map_err(|e| format!("`match` is not a regular expression: {e}"))?)
            }
            (Way::Regex, None) => return Err("a regex pattern needs `match`".into()),
            (Way::Cmd, _) if p.argv.is_empty() => return Err("a cmd pattern needs `argv`".into()),
            (Way::Jev, _) if p.instructions.as_deref().is_none_or(|i| i.trim().is_empty()) => return Err("a jev pattern needs `instructions`".into()),
            _ => None,
        };
        Ok(Ready { p, lang, not, re })
    }

    /// Whether the pattern looks at this file.
    pub fn applies(&self, path: &str) -> bool {
        self.lang.iter().any(|g| g.is_match(path)) && !self.not.iter().any(|g| g.is_match(path))
    }

    /// The steering words with the place filled in.
    pub fn steer_at(&self, file: &str, line: u32, answer: &str) -> String {
        self.p.steer.replace("{file}", file).replace("{line}", &line.to_string()).replace("{answer}", answer)
    }
}

/// The patterns of the Table that can be used, in the order of their ids, and what was wrong with the others.
pub fn load(tables: &Tables) -> (Vec<Ready>, Vec<String>) {
    let (mut ready, mut found) = (Vec::new(), Vec::new());
    for id in tables.ids(Pattern::NAME) {
        match tables.get::<Pattern>(id) {
            Ok(p) => match Ready::new(p) {
                Ok(r) => ready.push(r),
                Err(why) => found.push(format!("pattern {id}: {why}")),
            },
            Err(p) => found.push(p.to_string()),
        }
    }
    (ready, found)
}
