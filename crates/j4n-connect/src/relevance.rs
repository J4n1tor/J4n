//! What a release says that matters to J4n: deterministic, keyless, no model. `uses` is the set of names J4n
//! itself holds for a tool, generated from its connector; a bullet of the release notes is put in a class by the words and names in
//! it. Release notes are untrusted text: here they are only read for words, never followed.

use crate::manifest::Connector;
use std::collections::{BTreeMap, BTreeSet};

/// The most bullets read from one release, and the longest one kept.
const BULLETS: usize = 200;
const CLIP: usize = 300;
/// The most bullets of one release written to the Log (the ones that are not noise).
pub const KEEP: usize = 12;
/// Words of removal or change: a bullet that names something J4n uses and has one of these breaks.
const CHANGED: [&str; 8] = ["removed", "deprecated", "renamed", "no longer", "default", "breaking", "dropped", "replaced"];
/// Words that cannot wait for a digest.
const SECURITY: [&str; 3] = ["cve-", "security advisory", "vulnerability"];

/// How much a bullet matters, the most last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Noise,
    Adds,
    Touches,
    Breaks,
}

impl Class {
    pub fn word(self) -> &'static str {
        match self {
            Class::Noise => "noise",
            Class::Adds => "adds",
            Class::Touches => "touches",
            Class::Breaks => "breaks",
        }
    }
}

/// The names J4n has for a tool.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Uses(BTreeSet<String>);

impl Uses {
    pub fn new<I: IntoIterator<Item = S>, S: Into<String>>(names: I) -> Uses {
        Uses(names.into_iter().map(Into::into).collect())
    }

    /// The names a connector row holds: the flags it passes, the words it passes first, the names of its environment, the models it
    /// routes to and its program.
    pub fn of_connector(c: &Connector) -> Uses {
        let mut names = BTreeSet::new();
        let mut argv = |words: &[String]| {
            let mut after_flag = false;
            for (i, w) in words.iter().enumerate() {
                let is_flag = w.starts_with('-') && w.len() > 2;
                if is_flag {
                    names.insert(w.split('=').next().unwrap_or(w).to_string());
                } else if !after_flag && i > 0 && !w.contains(['{', '/', '~']) && w.len() >= 5 && w.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
                    // a word that follows the program, not the value of a flag
                    names.insert(w.clone());
                }
                after_flag = is_flag && !w.contains('=');
            }
        };
        for words in [&c.start, &c.resume, &c.model, &c.oneshot, &c.argv, &c.probe.argv].into_iter().chain(c.modes.values()) {
            argv(words);
        }
        if let Some(s) = &c.signin {
            argv(&s.argv);
            names.extend(s.env.keys().cloned());
        }
        names.extend(c.env.keys().cloned());
        names.extend(c.models.iter().filter(|m| m.len() >= 4).cloned());
        names.extend(c.program.iter().filter(|p| p.len() >= 3).cloned());
        Uses(names)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }

    /// The names of `uses` that `text` says, each as a whole word (a flag is not found inside a longer one).
    pub fn in_text(&self, text: &str) -> Vec<String> {
        let low = text.to_ascii_lowercase();
        let word = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_');
        self.0
            .iter()
            .filter(|n| {
                let n = n.to_ascii_lowercase();
                low.match_indices(&n).any(|(i, m)| !low[..i].chars().next_back().is_some_and(word) && !low[i + m.len()..].chars().next().is_some_and(word))
            })
            .cloned()
            .collect()
    }
}

/// The text of release notes that come as HTML (an Atom feed's `<content>`) or as Markdown (the REST API's `body`): one line for each
/// bullet, a heading or a paragraph.
pub fn plain(notes: &str) -> String {
    let mut out = String::new();
    let mut rest = notes;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let Some(j) = rest[i..].find('>') else {
            rest = "";
            break;
        };
        let tag = rest[i + 1..i + j].trim_start_matches('/').split([' ', '/']).next().unwrap_or_default().to_ascii_lowercase();
        match tag.as_str() {
            "li" => out.push_str("\n- "),
            "br" | "p" | "h1" | "h2" | "h3" | "h4" | "ul" | "ol" | "div" | "tr" | "pre" => out.push('\n'),
            _ => {}
        }
        rest = &rest[i + j + 1..];
    }
    out.push_str(rest);
    out.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&")
}

/// The bullets of release notes: the lines that start with a bullet mark, else every line that is not a heading.
pub fn bullets(notes: &str) -> Vec<String> {
    let text = plain(notes);
    let marked = |l: &str| l.starts_with(['-', '*', '•']) && !l.starts_with("--") || l.starts_with("- ");
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let pick: Vec<&str> = if lines.iter().any(|l| marked(l)) { lines.iter().copied().filter(|l| marked(l)).collect() } else { lines.iter().copied().filter(|l| !l.starts_with('#')).collect() };
    pick.into_iter()
        .map(|l| l.strip_prefix(['-', '*', '•']).unwrap_or(l).trim())
        .filter(|l| !l.is_empty())
        .take(BULLETS)
        .map(|l| if l.chars().count() > CLIP { format!("{}…", l.chars().take(CLIP).collect::<String>()) } else { l.to_string() })
        .collect()
}

/// A bullet in its class: `breaks` names something J4n uses and says it was removed or changed; `touches` names something J4n uses;
/// `adds` starts "Added", "New" or "Introduced" and names none; the rest is noise.
pub fn classify(bullet: &str, uses: &Uses) -> (Class, Vec<String>) {
    let names = uses.in_text(bullet);
    let low = bullet.to_ascii_lowercase();
    let class = if !names.is_empty() {
        if CHANGED.iter().any(|w| low.contains(w)) { Class::Breaks } else { Class::Touches }
    } else if ["added", "add", "new", "introduced"].contains(&low.split(|c: char| !c.is_ascii_alphabetic()).next().unwrap_or_default()) {
        Class::Adds
    } else {
        Class::Noise
    };
    (class, names)
}

/// What a release's notes come to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Assessed {
    /// The most that any bullet is.
    pub class: Option<Class>,
    /// Bullets that are `breaks`, `touches`, `adds` and noise.
    pub counts: [usize; 4],
    /// The bullets that are not noise, `breaks` first, at most [`KEEP`]: the class, the words, the names of `uses` in them.
    pub items: Vec<(Class, String, Vec<String>)>,
    /// A word that cannot wait for a digest (a CVE, a security advisory).
    pub security: bool,
}

pub fn assess(notes: &str, uses: &Uses) -> Assessed {
    let mut a = Assessed::default();
    let mut items = Vec::new();
    let low = plain(notes).to_ascii_lowercase();
    a.security = SECURITY.iter().any(|w| low.contains(w));
    for b in bullets(notes) {
        let (class, names) = classify(&b, uses);
        a.counts[match class {
            Class::Breaks => 0,
            Class::Touches => 1,
            Class::Adds => 2,
            Class::Noise => 3,
        }] += 1;
        a.class = a.class.max(Some(class));
        if class != Class::Noise {
            items.push((class, b, names));
        }
    }
    items.sort_by_key(|i| std::cmp::Reverse(i.0));
    items.truncate(KEEP);
    a.items = items;
    a
}

/// What a set of names (a server's tools, a provider's models) did between two looks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Gone(String),
    Changed(String),
    Added(String),
}

impl Change {
    /// A name that is gone, or whose words or schema changed, breaks what used it; one that is added only adds.
    pub fn class(&self) -> Class {
        if matches!(self, Change::Added(_)) { Class::Adds } else { Class::Breaks }
    }

    pub fn name(&self) -> &str {
        match self {
            Change::Gone(n) | Change::Changed(n) | Change::Added(n) => n,
        }
    }

    pub fn word(&self) -> &'static str {
        match self {
            Change::Gone(_) => "gone",
            Change::Changed(_) => "changed",
            Change::Added(_) => "added",
        }
    }
}

/// The difference between two sets of names, each with a hash of what it is (a model has `""`).
pub fn diff_set(old: &BTreeMap<String, String>, new: &BTreeMap<String, String>) -> Vec<Change> {
    let mut out: Vec<Change> = old.keys().filter(|k| !new.contains_key(*k)).map(|k| Change::Gone(k.clone())).collect();
    out.extend(new.iter().filter(|(k, h)| old.get(*k).is_some_and(|o| o != *h)).map(|(k, _)| Change::Changed(k.clone())));
    out.extend(new.keys().filter(|k| !old.contains_key(*k)).map(|k| Change::Added(k.clone())));
    out
}
