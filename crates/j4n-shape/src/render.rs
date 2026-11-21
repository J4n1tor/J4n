//! What the doer receives: the rows the operator kept, worded by a template of the pack (`shape_template`,
//! so a new format is a file and law 14 reads its words). The line itself is never wrapped or changed: it is the goal of the turn, and
//! this is the section `shape` of the Brief beside it.

use crate::check::Item;
use j4n_core::table::Kind;
use serde::Deserialize;

/// An entry of kind `shape_template`: the words round the kept rows. A hole is `{name}`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Template {
    pub id: String,
    #[serde(default)]
    pub doc: String,
    /// Before the rows (`{line}` is what was typed; in the template `rewrite`, `{rewrite}` is the clean version).
    #[serde(default)]
    pub head: String,
    /// Round all the rows: `{rows}` is where they go.
    #[serde(default)]
    pub frame: String,
    #[serde(default, rename = "ref")]
    pub reference: String,
    #[serde(default)]
    pub files: String,
    #[serde(default)]
    pub done: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub question: String,
}

impl Kind for Template {
    const NAME: &'static str = "shape_template";
}

pub fn fill(template: &str, holes: &[(&str, &str)]) -> String {
    holes.iter().fold(template.to_string(), |t, (k, v)| t.replace(&format!("{{{k}}}"), v))
}

/// The section for the rows `items` that were kept (a rewrite is worded by the template `rewrite`, see `Desk::keep`); `answer` is the
/// option picked for a question. `line` is what was typed. `None` when nothing is left to say.
pub fn section(t: &Template, items: &[&Item], answer: Option<usize>, line: &str) -> Option<String> {
    let mut rows: Vec<String> = Vec::new();
    for i in items.iter().filter(|i| i.kind == "ref") {
        let part = |k: &str| i.parts.iter().find(|(n, _)| *n == k).map_or("", |(_, v)| v.as_str());
        rows.push(fill(&t.reference, &[("phrase", part("phrase")), ("turn", part("turn")), ("quote", part("quote"))]));
    }
    let paths: Vec<&str> = items.iter().filter(|i| i.kind == "file").map(|i| i.text.as_str()).collect();
    if !paths.is_empty() {
        rows.push(fill(&t.files, &[("paths", &paths.join(", "))]));
    }
    if let Some(i) = items.iter().find(|i| i.kind == "done") {
        rows.push(fill(&t.done, &[("cmd", &i.text)]));
    }
    for i in items.iter().filter(|i| i.kind == "note") {
        rows.push(fill(&t.note, &[("text", &i.text)]));
    }
    if let (Some(i), Some(n)) = (items.iter().find(|i| i.kind == "question"), answer)
        && let Some(option) = i.options.get(n)
    {
        rows.push(fill(&t.question, &[("text", &i.text), ("answer", option)]));
    }
    if rows.is_empty() {
        return None;
    }
    let head = fill(&t.head, &[("line", line)]);
    let body = if t.frame.is_empty() { rows.join("\n") } else { fill(&t.frame, &[("rows", &rows.join("\n"))]) };
    Some([head, body].into_iter().filter(|s| !s.trim().is_empty()).collect::<Vec<_>>().join("\n"))
}

/// An entry of kind `persona`: what a model is told it is for.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Persona {
    pub id: String,
    #[serde(default)]
    pub doc: String,
    pub text: String,
}

impl Kind for Persona {
    const NAME: &'static str = "persona";
}

/// An entry of kind `shape_format`: the format a doer reads best. The pack ships none.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Format {
    pub id: String,
    #[serde(default)]
    pub doc: String,
    pub doer: String,
    pub format: String,
}

impl Kind for Format {
    const NAME: &'static str = "shape_format";
}
