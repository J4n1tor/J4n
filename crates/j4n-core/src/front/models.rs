//! Model words: what the operator says for a model (`deepseek 4.1 latest`, `sonnet`, `gpt-4o`) is
//! matched against the models the doer has, before anything starts. A word that matches none is refused; it is never passed to
//! the program as typed.
//!
//! The lists are kept by whatever knows the doers ([`ModelLists`]; `j4n-connect` holds them in `ModelsOf`) and are never read on
//! the turn path. [`resolve`] is pure: the word and a list in, one of three answers out.
//!
//! How a word matches a name:
//! - both are lowercased and cut into words at every run of characters that are not letters or digits, and again where a letter
//!   is followed by a digit (`v4` is `v 4`, `deepseek4.1` is `deepseek 4 1`; `4o` and `235b` stay whole);
//! - every word of the model word must be a word of the name, in any order. The name's own words are a set, so a provider
//!   prefix (`deepseek/deepseek-…`) is no extra word;
//! - the numbers that follow each other at the start of the first run of numbers are the version (`4.1`, `4-1` and `v4.1` are
//!   all `4 1`); a number of three digits or more is a size or a date, never part of a version. The version in the model word
//!   must be a start of the name's version, and the name whose version is exactly it is preferred (`gpt 4` is `gpt-4`, not
//!   `gpt-4.1`);
//! - `latest` and `newest` ask for no version: the newest of what matches is taken, by the version in the name and then by the
//!   date it carries (`…-flash-1017` over `…-flash`). That is also what a word with no version gets;
//! - of what still matches, the names with the fewest words the model word did not say are kept (`gpt 4.1` is `gpt-4.1`, not
//!   `gpt-4.1-mini`). Names that are left and differ in more than a date are the choice the operator is asked.

use std::collections::BTreeSet;

/// What knows the models a doer has. The front asks it when a model word is said, never when a turn starts.
pub trait ModelLists: Send + Sync {
    /// The models of `doer` as kept now; `None` when this front is not wired to any list (a bare front under test), in which case
    /// a model word is taken as typed.
    fn models(&self, doer: &str) -> Option<Vec<String>>;

    /// The models of `doer` read again, because a word did not match the list as kept (a model new since it was read).
    fn refresh(&self, doer: &str) -> Option<Vec<String>> {
        self.models(doer)
    }

    /// The models of `doer` as already kept, read from nothing: the library page asks this every time it is drawn.
    fn kept(&self, _doer: &str) -> Vec<String> {
        Vec::new()
    }
}

pub struct NoLists;

impl ModelLists for NoLists {
    fn models(&self, _doer: &str) -> Option<Vec<String>> {
        None
    }
}

/// What a model word came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// The model it names, in full.
    One(String),
    /// More than one model, in the order to offer them: the first is the one to recommend (the newest).
    Many(Vec<String>),
    /// None; the nearest names by edit distance (at most three).
    None(Vec<String>),
}

const FILLER: [&str; 2] = ["latest", "newest"];

/// The words of `s`: see the module's account of how a name is cut.
pub(super) fn words(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut prev_alpha = false;
    for c in s.chars().flat_map(char::to_lowercase) {
        if !c.is_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            prev_alpha = false;
            continue;
        }
        // a letter then a digit starts a new word; a digit then a letter does not (`4o`, `235b`)
        if prev_alpha && c.is_ascii_digit() && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        prev_alpha = c.is_alphabetic();
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn number(w: &str) -> Option<u64> {
    (!w.is_empty() && w.len() <= 18 && w.bytes().all(|b| b.is_ascii_digit())).then(|| w.parse().unwrap_or(u64::MAX))
}

/// Where the version is in `ws`: the start and the length of the parts of at most two digits at the start of the first run of
/// numbers.
fn span(ws: &[String]) -> (usize, usize) {
    let at = ws.iter().position(|w| number(w).is_some()).unwrap_or(ws.len());
    let len = ws[at..].iter().take_while(|w| number(w).is_some() && w.len() <= 2).count();
    (at, len)
}

fn version(ws: &[String]) -> Vec<u64> {
    let (at, len) = span(ws);
    ws[at..at + len].iter().filter_map(|w| number(w)).collect()
}

/// One name of the list, cut up.
struct Name<'a> {
    full: &'a str,
    set: BTreeSet<String>,
    version: Vec<u64>,
    /// The numbers that are not the version, in order: the date.
    date: Vec<u64>,
    /// The words that are not numbers: what makes two names the same model at two dates.
    stem: BTreeSet<String>,
}

impl<'a> Name<'a> {
    fn of(full: &'a str) -> Name<'a> {
        let ws = words(full);
        let (at, len) = span(&ws);
        let date = ws.iter().enumerate().filter(|(i, _)| *i < at || *i >= at + len).filter_map(|(_, w)| number(w)).collect();
        let stem = ws.iter().filter(|w| number(w).is_none()).cloned().collect();
        Name { full, version: version(&ws), date, stem, set: ws.into_iter().collect() }
    }
}

/// What `word` says: the words it needs, the version it asks for (none for `latest` or no version).
struct Ask {
    need: BTreeSet<String>,
    version: Vec<u64>,
}

fn ask_of(word: &str) -> Ask {
    let mut ws = words(word);
    ws.retain(|w| !FILLER.contains(&w.as_str()));
    // a lone `v` before a number is the way a version is written (`v4.1`), not a word the name must have
    let lone_v: Vec<usize> = (0..ws.len().saturating_sub(1)).filter(|&i| ws[i] == "v" && number(&ws[i + 1]).is_some()).collect();
    for i in lone_v.into_iter().rev() {
        ws.remove(i);
    }
    let (at, len) = span(&ws);
    let need = ws.iter().enumerate().filter(|(i, _)| *i < at || *i >= at + len).map(|(_, w)| w.clone()).collect();
    Ask { need, version: version(&ws) }
}

/// The model `word` names among `list`. Pure: no list is read, nothing is started.
pub fn resolve(word: &str, list: &[String]) -> Resolved {
    let key = words(word).join(" ");
    if key.is_empty() {
        return Resolved::None(nearest(word, list));
    }
    // the name itself, however it is spelled, is the model it names
    let bare = |m: &str| words(m.rsplit('/').next().unwrap_or(m)).join(" ");
    let exact: Vec<&String> = list.iter().filter(|m| words(m).join(" ") == key || bare(m) == key || m.eq_ignore_ascii_case(word.trim())).collect();
    if let [one] = exact.as_slice() {
        return Resolved::One((*one).clone());
    }
    let ask = ask_of(word);
    let mut names: Vec<Name> = list.iter().map(|m| Name::of(m)).filter(|n| ask.need.is_subset(&n.set) && n.version.starts_with(&ask.version)).collect();
    if names.is_empty() {
        return Resolved::None(nearest(word, list));
    }
    if ask.version.is_empty() {
        let newest = names.iter().map(|n| n.version.clone()).max().unwrap_or_default();
        names.retain(|n| n.version == newest);
    } else if names.iter().any(|n| n.version == ask.version) {
        names.retain(|n| n.version == ask.version);
    } else {
        let newest = names.iter().map(|n| n.version.clone()).max().unwrap_or_default();
        names.retain(|n| n.version == newest);
    }
    // the same model at several dates: the newest date
    let mut kept: Vec<Name> = Vec::new();
    for n in names {
        match kept.iter().position(|k| k.stem == n.stem && k.version == n.version) {
            Some(i) if kept[i].date < n.date => kept[i] = n,
            Some(_) => {}
            None => kept.push(n),
        }
    }
    let extra = |n: &Name| n.stem.difference(&ask.need).count();
    if let Some(least) = kept.iter().map(extra).min() {
        kept.retain(|n| extra(n) == least);
    }
    kept.sort_by(|a, b| b.version.cmp(&a.version).then(extra(a).cmp(&extra(b))).then(b.date.cmp(&a.date)).then(a.full.cmp(b.full)));
    match kept.as_slice() {
        [one] => Resolved::One(one.full.to_string()),
        _ => Resolved::Many(kept.iter().map(|n| n.full.to_string()).collect()),
    }
}

/// How new a name is: its version, then its date (`…-flash-1017` over `…-flash`); what a tier breaks a tie by.
pub(super) fn recency(name: &str) -> (Vec<u64>, Vec<u64>) {
    let n = Name::of(name);
    (n.version, n.date)
}

/// The three names of `list` nearest to `word` by edit distance, nearest first.
fn nearest(word: &str, list: &[String]) -> Vec<String> {
    let w = words(word).join(" ");
    let mut scored: Vec<(usize, &String)> = list
        .iter()
        .map(|m| {
            let whole = words(m).join(" ");
            // the part after the provider (`deepseek/`) is what is usually typed
            let bare = m.rsplit('/').next().map(|b| words(b).join(" ")).unwrap_or_default();
            (edit_distance(&w, &whole).min(edit_distance(&w, &bare)), m)
        })
        .collect();
    scored.sort();
    scored.into_iter().take(3).map(|(_, m)| m.clone()).collect()
}

pub(super) fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut diag = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let up = row[j + 1];
            row[j + 1] = (up + 1).min(row[j] + 1).min(diag + usize::from(ca != cb));
            diag = up;
        }
    }
    row[b.len()]
}

/// Whether `word` is one of the words that stand for "no model asked" before the model: `to`, `on`, `with`.
pub fn is_filler(word: &str) -> bool {
    matches!(word, "to" | "on" | "with")
}

/// The doer's own full id of a family on its list: `<doer>-<family>-<version>` (`claude-haiku-5-5`, `claude-haiku-4-5-20251001`).
/// It is passed on as typed, so the version asked for is the version run; the doer's program says whether it knows it.
fn full_id(doer: &str, word: &str, list: &[String]) -> Option<String> {
    let id = word.trim().to_lowercase();
    let rest = id.strip_prefix(doer)?.strip_prefix('-')?;
    let version = list.iter().find_map(|m| rest.strip_prefix(m.as_str()).and_then(|v| v.strip_prefix('-')))?;
    (!version.is_empty() && version.split('-').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))).then_some(id)
}

/// Settle a model word against `lists` for `doer`: the list as kept first, then once more from a fresh read when the word matched
/// none. `Ok(None)` when the doer has no list (the word is then its own: only a doer that lists no models at all gets that).
/// This is the one way a model word becomes a model, for `:switch`, `--model` and a line that names one.
pub fn settle(lists: &dyn ModelLists, doer: &str, word: &str) -> Result<Option<Resolved>, String> {
    // a doer that lists nothing has nothing to match against (a scripted doer, a connector with no `models`): the word is its own
    let Some(list) = lists.models(doer).filter(|l| !l.is_empty()) else { return Ok(None) };
    let mut got = resolve(word, &list);
    if matches!(got, Resolved::None(_))
        && let Some(fresh) = lists.refresh(doer).filter(|f| *f != list)
    {
        got = resolve(word, &fresh);
    }
    if let (Resolved::None(_), Some(id)) = (&got, full_id(doer, word, &list)) {
        return Ok(Some(Resolved::One(id)));
    }
    if let Resolved::None(near) = &got {
        return Err(format!("{doer} has no model like `{word}`; nearest: {}", near.join(", ")));
    }
    Ok(Some(got))
}
