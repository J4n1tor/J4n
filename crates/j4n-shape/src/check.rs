//! The checks without a model: every item the shaper gives needs a source that J4n can look at, and
//! what does not hold is dropped with its reason. Nothing here asks a model, so a shaper that invents a file, a turn, a limit the
//! operator never set or a shout in capitals gets none of it through.

use serde_json::Value as Json;
use std::path::{Component, Path, PathBuf};

/// The most of each kind of item.
const MAX: [(&str, usize); 5] = [("ref", 3), ("file", 5), ("done", 1), ("note", 3), ("question", 1)];
/// A line of an item is cut at this many characters.
const TEXT_MAX: usize = 200;
const SHOUTS: [&str; 3] = ["MUST", "CRITICAL", "IMPORTANT"];
/// Words of a budget, and claims that something cannot be done (law 14): the model is not told either.
const BAD_WORDS: [&str; 14] = ["budget", "quota", "cost", "cheap", "expensive", "spend", "cannot", "can't", "unable", "no access", "not allowed", "not able", "don't have access", "do not have access"];
/// A note that limits the doer says one of these; hints are allowed, limits come only from the operator.
const LIMIT_WORDS: [&str; 8] = ["only", "never", "don't", "do not", "must not", "mustn't", "unchanged", "without"];

/// One of the last turns of the thread, as the shaper is shown it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub id: String,
    pub you: String,
    pub answer_head: String,
    pub files: Vec<String>,
}

/// What the checks look at: the state the shaper was given, and the folder.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub line: String,
    pub turns: Vec<Turn>,
    pub files_changed: Vec<String>,
    pub repo: Vec<String>,
    pub checks: Vec<String>,
    pub root: PathBuf,
}

/// A thing the shaper added that held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// `ref:1`, `file:2`, `done`, `note:1`, `question`, `rewrite`: what a kept row is called.
    pub key: String,
    pub kind: &'static str,
    pub text: String,
    /// The turn, check or file that backs it.
    pub source: String,
    /// What was looked at: `quote`, `exist`, `check`, `turn`, `operator`.
    pub proof: &'static str,
    pub options: Vec<String>,
    /// The words the templates fill in (`phrase`, `turn`, `quote`, `path`, `cmd`).
    pub parts: Vec<(&'static str, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    pub text: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Checked {
    pub items: Vec<Item>,
    pub dropped: Vec<Dropped>,
}

/// What the answer may add up to.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_tokens: usize,
    pub rewrite: bool,
}

/// Lower case, white space as one space: how a quote is compared.
pub fn fold(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// The closed JSON out of an answer: the first object in it, with a verdict that is one of the three.
pub fn parse(answer: &str) -> Result<Json, String> {
    let v: Json = serde_json::from_str(object_in(answer).ok_or("the answer is not JSON")?).map_err(|e| format!("the answer is not the closed JSON: {e}"))?;
    match v.get("verdict").and_then(Json::as_str) {
        Some("pass" | "add" | "ask") => Ok(v),
        other => Err(format!("the verdict is {}, not pass, add or ask", other.map_or("missing".to_string(), |o| format!("`{o}`")))),
    }
}

fn word<'a>(v: &'a Json, k: &str) -> &'a str {
    v.get(k).and_then(Json::as_str).map(str::trim).unwrap_or_default()
}

fn list<'a>(v: &'a Json, k: &str) -> impl Iterator<Item = &'a Json> {
    v.get(k).and_then(Json::as_array).into_iter().flatten()
}

fn shout(w: &str) -> bool {
    let w = w.trim_matches(|c: char| !c.is_alphanumeric());
    w.chars().count() >= 3 && w.chars().all(|c| c.is_ascii_uppercase())
}

/// Why `text` may not be shown: more than a line, control characters, emphasis the operator did not type, a budget word or a
/// claim that something cannot be done. `typed` is everything the operator wrote that the text may repeat.
fn unclean(text: &str, typed: &str) -> Option<String> {
    if text.is_empty() {
        return Some("empty".into());
    }
    if text.chars().any(char::is_control) || text.chars().count() > TEXT_MAX {
        return Some("not one short line".into());
    }
    if text.contains("!!") || text.split_whitespace().any(|w| (shout(w) || SHOUTS.contains(&w)) && !typed.contains(w.trim_matches(|c: char| !c.is_alphanumeric()))) {
        return Some("emphasis the operator did not type".into());
    }
    let low = text.to_lowercase();
    BAD_WORDS.iter().find(|b| low.contains(*b)).map(|b| format!("words about `{b}` are not for a model"))
}

fn inside(root: &Path, path: &str) -> bool {
    !path.is_empty() && Path::new(path).components().all(|c| matches!(c, Component::Normal(_))) && root.join(path).exists()
}

/// Things in `line` that a rewrite must keep exactly: paths and file names, identifiers, numbers, quoted strings and code spans.
pub fn keepables(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for open in ['`', '"'] {
        let parts: Vec<&str> = line.split(open).collect();
        out.extend(parts.iter().skip(1).step_by(2).filter(|_| parts.len() % 2 == 1).map(|s| s.to_string()));
    }
    let bare: String = line.replace(['`', '"'], " ");
    for tok in bare.split_whitespace() {
        let t = tok.trim_matches(|c: char| ",;:!?()[]{}'".contains(c)).trim_end_matches('.');
        let alnum = |c: char| c.is_alphanumeric();
        let dotted = t.split_once('.').is_some_and(|(a, b)| !a.is_empty() && !b.is_empty() && a.chars().all(|c| alnum(c) || "_-/".contains(c)) && b.chars().all(|c| alnum(c) || "_-./".contains(c)));
        let camel = t.chars().zip(t.chars().skip(1)).any(|(a, b)| a.is_lowercase() && b.is_uppercase()) && t.chars().all(alnum);
        let chess = t.contains('_') && t.chars().any(alnum) && t.chars().all(|c| alnum(c) || c == '_');
        let number = !t.is_empty() && t.chars().all(|c| c.is_ascii_digit() || c == '.') && t.chars().any(|c| c.is_ascii_digit());
        if t.contains('/') || dotted || camel || chess || number {
            out.push(t.to_string());
        }
    }
    out.sort();
    out.dedup();
    out
}

fn tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

/// What of `reply` holds. `limits.rewrite` says whether a rewrite is wanted at all.
pub fn check(reply: &Json, facts: &Facts, limits: Limits) -> Checked {
    let mut out = Checked::default();
    let typed = std::iter::once(facts.line.as_str()).chain(facts.turns.iter().map(|t| t.you.as_str())).collect::<Vec<_>>().join("\n");
    let drop = |out: &mut Checked, text: String, reason: &str| out.dropped.push(Dropped { text, reason: reason.into() });
    let count = |kind: &str| MAX.iter().find(|(k, _)| *k == kind).map(|(_, n)| *n).unwrap_or(1);
    let turn = |id: &str| facts.turns.iter().find(|t| t.id == id);
    let n = |out: &Checked, kind: &str| out.items.iter().filter(|i| i.kind == kind).count();
    let push = |out: &mut Checked, kind: &'static str, text: String, source: String, proof: &'static str, parts: Vec<(&'static str, String)>| {
        if n(out, kind) >= count(kind) {
            out.dropped.push(Dropped { text, reason: format!("more than {} {kind}s", count(kind)) });
            return;
        }
        let key = if matches!(kind, "done" | "question" | "rewrite") { kind.to_string() } else { format!("{kind}:{}", n(out, kind) + 1) };
        out.items.push(Item { key, kind, text, source, proof, options: Vec::new(), parts });
    };
    for r in list(reply, "refs") {
        let (phrase, id, quote) = (word(r, "phrase"), word(r, "turn"), word(r, "quote"));
        let text = format!("\"{phrase}\" = {id} › {quote}");
        match turn(id) {
            None => drop(&mut out, text, "no such turn"),
            Some(t) if quote.is_empty() || !(fold(&t.you).contains(&fold(quote)) || fold(&t.answer_head).contains(&fold(quote))) => drop(&mut out, text, "the quote is not in that turn"),
            // the quote is a copy of what was said and is not the shaper's words; the phrase is
            Some(_) => match unclean(phrase, &typed) {
                Some(why) => drop(&mut out, text, &why),
                None => push(&mut out, "ref", text, id.into(), "quote", vec![("phrase", phrase.into()), ("turn", id.into()), ("quote", quote.into())]),
            },
        }
    }
    for f in list(reply, "files") {
        let (path, why) = (word(f, "path"), word(f, "why"));
        if !(facts.repo.iter().any(|p| p == path) || inside(&facts.root, path)) {
            drop(&mut out, path.into(), "no such file");
        } else if let Some(bad) = unclean(path, &typed) {
            drop(&mut out, path.into(), &bad);
        } else {
            push(&mut out, "file", path.into(), why.into(), "exist", vec![("path", path.into())]);
        }
    }
    if let Some(d) = reply.get("done").filter(|d| d.is_object()) {
        let (cmd, why) = (word(d, "cmd"), word(d, "why"));
        if cmd.is_empty() || !facts.checks.iter().any(|c| c == cmd) {
            drop(&mut out, cmd.into(), "not a check of the project, nor a command of the thread that exited 0");
        } else {
            push(&mut out, "done", cmd.into(), why.into(), "check", vec![("cmd", cmd.into())]);
        }
    }
    for note in list(reply, "notes") {
        let (text, why, quote) = (word(note, "text"), word(note, "why"), word(note, "quote"));
        let cited = turn(why).is_some_and(|t| !quote.is_empty() && (fold(&t.you).contains(&fold(quote)) || fold(&t.answer_head).contains(&fold(quote)))) || facts.files_changed.iter().chain(&facts.repo).any(|p| p == why);
        let limits = |t: &str| {
            let low = t.to_lowercase();
            LIMIT_WORDS.iter().any(|w| low.split(|c: char| !c.is_alphanumeric() && c != '\'').any(|x| x == *w) || (w.contains(' ') && low.contains(w)))
        };
        if !cited {
            drop(&mut out, text.into(), "no source");
        // a limit is the operator's own words: the quote has to say it, in a line the operator typed
        } else if limits(text) && !(limits(quote) && fold(&typed).contains(&fold(quote))) {
            drop(&mut out, text.into(), "a limit the operator did not state");
        } else if let Some(bad) = unclean(text, &typed) {
            drop(&mut out, text.into(), &bad);
        } else {
            push(&mut out, "note", text.into(), why.into(), "quote", Vec::new());
        }
    }
    if let Some(q) = reply.get("question").filter(|q| q.is_object()) {
        let text = word(q, "text");
        let options: Vec<String> = list(q, "options").filter_map(Json::as_str).map(|s| s.trim().to_string()).collect();
        let distinct = options.iter().enumerate().all(|(i, o)| !o.is_empty() && o.chars().count() <= 80 && !options[..i].contains(o));
        if !(2..=4).contains(&options.len()) || !distinct {
            drop(&mut out, text.into(), "a question has 2 to 4 different options of up to 80 characters");
        } else if let Some(bad) = unclean(text, &typed).or_else(|| options.iter().find_map(|o| unclean(o, &typed))) {
            drop(&mut out, text.into(), &bad);
        } else {
            push(&mut out, "question", text.into(), "operator".into(), "turn", Vec::new());
            if let Some(i) = out.items.iter_mut().find(|i| i.kind == "question") {
                i.options = options;
            }
        }
    }
    if let Some(r) = reply.get("rewrite").and_then(Json::as_str).map(str::trim).filter(|r| !r.is_empty()) {
        let lost: Vec<String> = keepables(&facts.line).into_iter().filter(|k| !r.contains(k.as_str())).collect();
        if !limits.rewrite {
            drop(&mut out, r.into(), "a rewrite was not asked for");
        } else if !lost.is_empty() {
            drop(&mut out, r.into(), &format!("the rewrite lost {}", lost.join(", ")));
        } else if let Some(bad) = unclean(r, &typed) {
            drop(&mut out, r.into(), &bad);
        } else {
            out.items.push(Item { key: "rewrite".into(), kind: "rewrite", text: r.into(), source: "operator".into(), proof: "turn", options: Vec::new(), parts: Vec::new() });
        }
    }
    // the total: items go from the end, notes first and files last
    for kind in ["note", "question", "done", "ref", "file"] {
        while out.items.iter().map(|i| tokens(&i.text)).sum::<usize>() > limits.max_tokens {
            let Some(at) = out.items.iter().rposition(|i| i.kind == kind) else { break };
            let gone = out.items.remove(at);
            drop(&mut out, gone.text, "over the size of the notes");
        }
    }
    out
}

/// The first JSON object in an answer, whatever it holds (the judge's verdict has no `verdict` word).
pub fn parse_any(answer: &str) -> Result<Json, String> {
    serde_json::from_str(object_in(answer).ok_or("the judge's answer is not JSON")?).map_err(|e| format!("the judge's answer is not JSON: {e}"))
}

/// From the first `{` to the last `}`: the object, with the fence or the talk round it left off.
fn object_in(answer: &str) -> Option<&str> {
    let (a, b) = (answer.find('{')?, answer.rfind('}')?);
    (a < b).then(|| &answer[a..=b])
}
