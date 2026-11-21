//! What the other kinds of source say: a list of models (models.dev), a docs page (`llms.txt` or any
//! page, as a set of lines) and one issue of GitHub's REST. Each is read here into plain values and nothing else; the asking, the
//! validators and the budget are the watcher's. What an answer says is untrusted text: it is clipped to a line and nothing in it is run.

use crate::digest::one_line;
use crate::relevance::Uses;
use serde_json::Value as Json;
use std::collections::BTreeMap;

/// The most lines of a page that are kept, and the longest of them.
pub const PAGE_LINES: usize = 2000;
const LINE: usize = 200;

/// A short hash of a text: what changes when the text does.
pub fn short(text: &str) -> String {
    j4n_core::text::short_hash(text)
}

/// The providers a `watch` of `ids:a,b` names.
pub fn providers(watch: &str) -> Vec<String> {
    watch.strip_prefix("ids:").map(|r| r.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()).unwrap_or_default()
}

/// The number a `watch` of `issue:<n>` names.
pub fn issue_number(watch: &str) -> Option<u64> {
    watch.strip_prefix("issue:")?.trim().parse().ok().filter(|n| *n > 0)
}

/// The models of these providers that J4n holds, each with a hash of what matters in them (what they cost, their limits, what they take and
/// give, whether they reason or call tools, their status); `None` when the answer is not a list of providers. A model is held when `uses` has its id, or a word of its id after the first (the
/// maker's name is no more a model than `claude` is): `opus` holds `claude-opus-5-5`. A name is `<provider>/<id>`.
pub fn models(text: &str, providers: &[String], uses: &Uses) -> Option<BTreeMap<String, String>> {
    let Ok(Json::Object(all)) = serde_json::from_str::<Json>(text) else { return None };
    let mut out = BTreeMap::new();
    for p in providers {
        let Some(Json::Object(models)) = all.get(p).and_then(|p| p.get("models")) else { continue };
        for (id, m) in models {
            if !held(id, uses) {
                continue;
            }
            let facts: Vec<String> = ["cost", "limit", "modalities", "reasoning", "tool_call", "structured_output", "status"].iter().map(|k| format!("{k}={}", m.get(*k).map_or_else(String::new, Json::to_string))).collect();
            out.insert(format!("{p}/{id}"), short(&facts.join("\n")));
        }
    }
    Some(out)
}

fn held(id: &str, uses: &Uses) -> bool {
    let id = id.to_ascii_lowercase();
    let after_first: Vec<&str> = id.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).skip(1).collect();
    uses.names().map(str::to_ascii_lowercase).any(|n| n == id || after_first.contains(&n.as_str()))
}

/// A page as a set of lines. A Markdown link (`- [Title](url): words`, the way `llms.txt` lists pages) is named by its address and hashed
/// with its words, so a page that is added, gone or described differently shows; any other line is its own name. `None` for a page with
/// nothing on it: an empty answer is a failure to say, not a page that lost every line.
pub fn lines(text: &str) -> Option<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()).take(PAGE_LINES) {
        let link = line.split_once("](").and_then(|(_, r)| r.split_once(')')).map(|(url, _)| url.trim()).filter(|u| !u.is_empty());
        match link {
            Some(url) => out.insert(one_line(url, LINE), short(line)),
            None => out.insert(one_line(line, LINE), String::new()),
        };
    }
    (!out.is_empty()).then_some(out)
}

/// One issue as GitHub's REST says it.
#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub number: u64,
    pub state: String,
    pub title: String,
    pub comments: u64,
}

pub fn issue(text: &str) -> Option<Issue> {
    let j: Json = serde_json::from_str(text).ok()?;
    let state = j.get("state")?.as_str()?;
    Some(Issue { number: j.get("number")?.as_u64()?, state: one_line(state, 20), title: one_line(j.get("title").and_then(Json::as_str).unwrap_or_default(), 160), comments: j.get("comments").and_then(Json::as_u64).unwrap_or(0) })
}
