//! The tags: the order the rules are tried in is here, the words they read are the rows of the kind `inbox_rule`
//! (`packs/inbox/words/`). Whole words only, lower-cased; the operator's line is never rewritten, only read.

use j4n_core::table::Tables;
use j4n_core::value::Value;
use std::collections::{BTreeMap, BTreeSet};

/// One rule's answer: the tag, the rule that gave it (its row, or one of the three overlap rules) and the fixed words that say why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tagged {
    pub tag: &'static str,
    pub rule: String,
    pub why: String,
}

/// The two numbers the overlap rules read (`inbox.related`, `inbox.unrelated`).
#[derive(Debug, Clone, Copy)]
pub struct Bounds {
    pub related: f64,
    pub unrelated: f64,
}

#[derive(Default)]
pub struct Rules {
    /// Rows of `as = line`: id, tag, words.
    line: Vec<(String, &'static str, BTreeSet<String>)>,
    /// Rows of `as = opens`: id, tag, words.
    opens: Vec<(String, &'static str, Vec<String>)>,
    filler: BTreeSet<String>,
    ignore: BTreeSet<String>,
}

pub fn tag_word(w: &str) -> Option<&'static str> {
    ["stop", "steer", "side", "note", "hold"].into_iter().find(|t| *t == w)
}

fn words_of(row: &std::collections::BTreeMap<String, Value>) -> Vec<String> {
    match row.get("words") {
        Some(Value::List(l)) => l.iter().filter_map(|v| if let Value::Str(s) = v { Some(s.to_lowercase()) } else { None }).collect(),
        _ => Vec::new(),
    }
}

/// The words of a text, lower-cased, cut at everything that is not a letter or a digit.
fn tokens(text: &str) -> Vec<String> {
    text.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(String::from).collect()
}

/// What a path counts for when a line is compared with a task: three words, because naming a file is naming the subject.
const PATH: u32 = 3;

/// A piece of a line that is a file's path: it has a `/`, or ends in a dot and a short extension that has a letter.
fn is_path(w: &str) -> bool {
    w.contains('/') || w.rsplit_once('.').is_some_and(|(a, e)| !a.is_empty() && (1..=4).contains(&e.len()) && e.chars().all(|c| c.is_ascii_alphanumeric()) && e.chars().any(|c| c.is_ascii_alphabetic()))
}

/// A word as it is compared: a plural `s` does not make another word.
fn stem(w: &str) -> String {
    if w.len() > 3 { w.strip_suffix('s').unwrap_or(w) } else { w }.to_string()
}

/// What a text is made of for the comparison: each path whole (with its weight), every other piece cut into words (weight one, the words that
/// do not count left out before a plural is cut).
fn units(text: &str, ignore: &BTreeSet<String>) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    for piece in text.split_whitespace() {
        let whole = piece.trim_matches(|c: char| c.is_ascii_punctuation() && c != '_' && c != '/' && c != '.').trim_end_matches('.').to_lowercase();
        if is_path(&whole) {
            out.push((whole, PATH));
        } else {
            out.extend(tokens(piece).iter().filter(|t| !ignore.contains(*t)).map(|t| (stem(t), 1)));
        }
    }
    out
}

impl Rules {
    pub fn load(tables: &Tables) -> Rules {
        let mut rules = Rules::default();
        for row in tables.rows("inbox_rule") {
            let Value::Map(row) = row else { continue };
            let text = |k: &str| match row.get(k) {
                Some(Value::Str(s)) => s.clone(),
                _ => String::new(),
            };
            let (id, tag, words) = (text("id"), tag_word(&text("tag")), words_of(&row));
            match (text("as").as_str(), tag) {
                ("line", Some(tag)) => rules.line.push((id, tag, words.into_iter().collect())),
                ("opens", Some(tag)) => rules.opens.push((id, tag, words)),
                ("filler", _) => rules.filler.extend(words),
                ("ignore", _) => rules.ignore.extend(words),
                _ => {}
            }
        }
        rules
    }

    /// The tag of a line typed over a task whose words are `context`.
    pub fn tag(&self, line: &str, context: &str, b: Bounds) -> Tagged {
        let low = line.trim().to_lowercase();
        let toks = tokens(&low);
        for (id, tag, words) in &self.line {
            if !toks.is_empty() && toks.iter().all(|t| words.contains(t) || self.filler.contains(t)) && toks.iter().any(|t| words.contains(t)) {
                return Tagged { tag, rule: id.clone(), why: "the line is only a stop word".into() };
            }
        }
        for (id, tag, words) in &self.opens {
            // a word that ends in `:` or is `#` is read as the line's start, any other as its first word
            let hit = words.iter().find(|w| if w.ends_with(':') || *w == "#" { low.starts_with(w.as_str()) } else { toks.first() == Some(w) });
            if let Some(w) = hit {
                return Tagged { tag, rule: id.clone(), why: format!("the line opens with `{w}`") };
            }
        }
        self.overlap(line, context, b)
    }

    fn overlap(&self, line: &str, context: &str, b: Bounds) -> Tagged {
        let ours: BTreeMap<String, u32> = units(line, &self.ignore).into_iter().collect();
        let theirs: BTreeSet<String> = units(context, &BTreeSet::new()).into_iter().map(|(u, _)| u).collect();
        if ours.is_empty() {
            return Tagged { tag: "hold", rule: "no-words".into(), why: "no word of the line says what it is about".into() };
        }
        let (all, shared): (u32, u32) = ours.iter().fold((0, 0), |(all, shared), (u, w)| (all + w, shared + if theirs.contains(u) { *w } else { 0 }));
        let share = f64::from(shared) / f64::from(all);
        let counted = format!("{shared} of its {all} words are the task's");
        if share >= b.related {
            return Tagged { tag: "steer", rule: "overlap-steer".into(), why: counted };
        }
        // a path the task has not touched, or a question: about something else
        let elsewhere = ours.iter().any(|(u, w)| *w == PATH && !theirs.contains(u));
        if share <= b.unrelated && (line.trim_end().ends_with('?') || elsewhere) {
            return Tagged { tag: "side", rule: "overlap-side".into(), why: format!("{counted}, and the line {}", if elsewhere { "names a path the task has not touched" } else { "is a question" }) };
        }
        Tagged { tag: "hold", rule: "overlap-hold".into(), why: format!("{counted}: not enough to say") }
    }
}
