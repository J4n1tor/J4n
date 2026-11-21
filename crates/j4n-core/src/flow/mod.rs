//! Flow: a declared graph of steps, the one runner of work that has more than one part. The rows are the
//! kind `flow` (`packs/base/kinds/flow.toml`); [`Flows::load`] reads them and checks each one the way a program is checked before
//! it runs, and [`run`] makes a doer of every flow that passed (`flow:<id>`), so a step can run a flow.
//!
//! - **Five kinds of step** (closed): `do` runs a doer as a task of its own, `ask` puts a question, `branch` goes one way or the
//!   other on a test, `map` runs a doer over a list, `end` is the answer. A step is one thing; nothing here is a language.
//! - **The order is the ports'.** A step runs after the steps it names in `after`, after the steps whose ports its templates read
//!   (`{steps.plan.out.plan}`), and after the steps that write a kind it `reads` (a reader of `diff` waits for the writers of
//!   `diff`: flows plug together by what they pass). The steps run in that order, ties in the order they were written; a branch,
//!   or an `ask` with `yes`/`no`, may go back, and says how often (`max_visits`).
//! - **Lint at load**: ids, kinds, every reference, ports against what a step declares, a cycle of dependencies, a back-edge with
//!   no `max_visits`, a flow that runs itself. A flow that does not pass is left out, and says why.

pub mod run;

use crate::expr::Expr;
use crate::fmt::Fmt;
use crate::table::{Kind, Problem, Tables};
use serde::Deserialize;
use std::collections::BTreeMap;

/// How many flows may run flows, one inside the other, when the flow does not say.
pub const DEPTH: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StepKind {
    Do,
    Ask,
    Branch,
    Map,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    Yesno,
    Approval,
    Choice,
    Text,
}

/// One step as it is written.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepRow {
    pub id: String,
    #[serde(default)]
    pub kind: Option<StepKind>,
    #[serde(default, rename = "do")]
    pub doer: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub mode: Option<crate::task::Mode>,
    #[serde(default)]
    pub place: Option<String>,
    #[serde(default)]
    pub goal: Option<String>,
    /// The goal when the step is entered again (a loop came back to it); it may read what came after, which `goal` may not.
    #[serde(default)]
    pub redo: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    /// For a model that works on its own: the most turns it takes (`Limits::visits`).
    #[serde(default)]
    pub turns: Option<u32>,
    /// The most this step may spend, in dollars (`Limits::usd_soft`); the tree's own cap leaves it the less of the two.
    #[serde(default)]
    pub usd: Option<f64>,
    /// Keep the work of this step in its place until it is said yes to (`apply`), whatever `work.apply` says: for a step whose work
    /// is checked, and shown to the operator, before it reaches the folder.
    #[serde(default)]
    pub hold: bool,
    /// The step that did not end well is an answer and not the end of the flow: a loop can go back on it.
    #[serde(default)]
    pub soft: bool,
    /// Nobody to do it (an election with nobody left to offer) skips the step; `steps.<id>.skipped` says so.
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub with: BTreeMap<String, String>,
    #[serde(default)]
    pub after: Vec<String>,
    /// The kinds this step reads (it runs after the steps that write them), and the absolute paths it reads, which a box binds read-only.
    #[serde(default)]
    pub reads: Vec<String>,
    #[serde(default)]
    pub out: BTreeMap<String, String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub when: Option<String>,
    #[serde(default)]
    pub shape: Option<Shape>,
    #[serde(default)]
    pub question: Option<String>,
    #[serde(default)]
    pub show: Option<String>,
    #[serde(default)]
    pub change: Option<String>,
    #[serde(default)]
    pub apply: Option<String>,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub on: Option<String>,
    #[serde(default)]
    pub yes: Option<String>,
    #[serde(default)]
    pub no: Option<String>,
    #[serde(default)]
    pub max_visits: Option<u32>,
    #[serde(default)]
    pub on_max: Option<String>,
    #[serde(default)]
    pub over: Option<String>,
    #[serde(default)]
    pub each: Option<String>,
    #[serde(default)]
    pub answer: Option<String>,
}

impl StepRow {
    pub fn kind(&self) -> StepKind {
        self.kind.unwrap_or(StepKind::Do)
    }
}

/// A flow as it is written.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowRow {
    pub id: String,
    #[serde(default)]
    pub doc: String,
    #[serde(default, rename = "in")]
    pub input: BTreeMap<String, String>,
    #[serde(default)]
    pub depth: Option<u32>,
    pub steps: Vec<StepRow>,
}

impl Kind for FlowRow {
    const NAME: &'static str = "flow";
}

/// A flow that passed its checks: the rows, and the order they run in.
#[derive(Debug, Clone)]
pub struct Flow {
    pub row: FlowRow,
    /// Indices into `row.steps`, in the order the steps run.
    pub order: Vec<usize>,
}

impl Flow {
    pub fn id(&self) -> &str {
        &self.row.id
    }

    pub fn step(&self, id: &str) -> Option<usize> {
        self.row.steps.iter().position(|s| s.id == id)
    }
}

/// The flows of a set of tables.
#[derive(Debug, Clone, Default)]
pub struct Flows(Vec<Flow>);

fn word(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// A port kind: the closed list, or an artifact (`artifact:<kind>@<major>`).
fn port_kind(k: &str) -> bool {
    matches!(k, "text" | "json" | "files" | "diff" | "verdict" | "report") || k.strip_prefix("artifact:").is_some_and(|a| a.split_once('@').is_some_and(|(n, v)| word(n) && !v.is_empty() && v.chars().all(|c| c.is_ascii_digit())))
}

/// The templates of a step, with the field each one is in.
fn templates(s: &StepRow) -> Vec<(&'static str, &str)> {
    fn one<'a>(name: &'static str, v: &'a Option<String>) -> Option<(&'static str, &'a str)> {
        v.as_deref().map(|t| (name, t))
    }
    let mut out: Vec<(&'static str, &str)> = [one("goal", &s.goal), one("redo", &s.redo), one("question", &s.question), one("show", &s.show), one("change", &s.change), one("answer", &s.answer), one("do", &s.doer)]
        .into_iter()
        .flatten()
        .collect();
    out.extend(s.with.values().map(|t| ("with", t.as_str())));
    out.extend(s.exclude.iter().map(|t| ("exclude", t.as_str())));
    out
}

/// The expressions of a step.
fn exprs(s: &StepRow) -> Vec<(&'static str, &str)> {
    [("when", &s.when), ("on", &s.on), ("over", &s.over)].into_iter().filter_map(|(n, v)| v.as_deref().map(|e| (n, e))).collect()
}

/// What a flow reads: each path read by a template or an expression.
fn reads_of(s: &StepRow) -> Result<Vec<Vec<String>>, String> {
    let mut paths = Vec::new();
    for (field, t) in templates(s) {
        let f = Fmt::parse(t).map_err(|e| format!("`{field}` is not a template: {e}"))?;
        paths.extend(f.unquoted_reads());
    }
    for (field, e) in exprs(s) {
        let x = Expr::parse(e).map_err(|e| format!("`{field}` is not an expression: {e}"))?;
        paths.extend(x.unquoted_reads());
    }
    Ok(paths)
}

/// The problems of one flow, and its order when it has none.
fn lint(row: &FlowRow, known: &[&str]) -> Result<Vec<usize>, Vec<String>> {
    let mut bad: Vec<String> = Vec::new();
    if !word(&row.id) {
        bad.push("the id is one word of letters, digits, `-` and `_`".into());
    }
    for (name, kind) in &row.input {
        if !word(name) || !port_kind(kind) {
            bad.push(format!("`in.{name} = {kind}`: a port has a word for a name and a kind of text, json, files, diff, verdict, report or artifact:<kind>@<major>"));
        }
    }
    if row.steps.is_empty() {
        bad.push("a flow has steps".into());
    }
    let ids: Vec<&str> = row.steps.iter().map(|s| s.id.as_str()).collect();
    let n = ids.len();
    let mut deps: Vec<Vec<usize>> = vec![Vec::new(); n];
    let at = |id: &str| ids.iter().position(|i| *i == id);
    for (i, s) in row.steps.iter().enumerate() {
        let here = |m: String| format!("step `{}`: {m}", s.id);
        if !word(&s.id) {
            bad.push(here("the id is one word of letters, digits, `-` and `_`".into()));
        } else if ids[..i].contains(&s.id.as_str()) {
            bad.push(here("the id is used twice".into()));
        }
        match s.kind() {
            StepKind::Do if s.doer.is_none() => bad.push(here("a `do` step says what does it".into())),
            StepKind::Do if s.goal.is_none() => bad.push(here("a `do` step has a `goal`".into())),
            StepKind::Ask => {
                if s.question.is_none() {
                    bad.push(here("an `ask` step has a `question`".into()));
                }
                match s.shape {
                    None => bad.push(here("an `ask` step has a `shape`".into())),
                    Some(Shape::Choice) if s.options.is_empty() => bad.push(here("a `choice` offers `options`".into())),
                    _ => {}
                }
            }
            StepKind::Branch if s.on.is_none() || s.yes.is_none() || s.no.is_none() => bad.push(here("a `branch` has `on`, `yes` and `no`".into())),
            StepKind::Map if s.over.is_none() || s.each.is_none() || s.goal.is_none() => bad.push(here("a `map` has `over`, `each` and a `goal` for each item".into())),
            _ => {}
        }
        if !matches!(s.kind(), StepKind::Do) && s.doer.is_some() {
            bad.push(here("`do` belongs to a `do` step".into()));
        }
        for (name, kind) in &s.out {
            if !word(name) || !port_kind(kind) {
                bad.push(here(format!("`out.{name} = {kind}`: a port has a word for a name and a kind of text, json, files, diff, verdict, report or artifact:<kind>@<major>")));
            }
        }
        for r in [&s.yes, &s.no, &s.apply].into_iter().flatten().chain(s.on_max.iter().filter(|r| *r != "fail")) {
            if at(r).is_none() {
                bad.push(here(format!("`{r}` is not a step of this flow")));
            }
        }
        if let Some(d) = s.doer.as_deref().filter(|d| !d.contains('{')) {
            match d.split_once(':') {
                Some(("who", role)) if !word(role) => bad.push(here(format!("`{d}`: a role is one word"))),
                Some(("flow", f)) if !known.contains(&f) => bad.push(here(format!("`{d}`: there is no flow called `{f}`"))),
                Some(("who" | "flow" | "model", _)) | None => {}
                Some((other, _)) => bad.push(here(format!("`{d}`: `{other}:` is not a way to say who does it (who:<role>, flow:<id>, model:<id>, or a connector's id)"))),
            }
        }
        match reads_of(s) {
            Err(m) => bad.push(here(m)),
            Ok(paths) => {
                let late = s.redo.as_deref().and_then(|r| Fmt::parse(r).ok()).map(|f| f.unquoted_reads()).unwrap_or_default();
                for p in paths {
                    // what a `redo` reads came after: it is checked, and it makes no step wait
                    let came_after = late.contains(&p) && !s.goal.as_deref().and_then(|g| Fmt::parse(g).ok()).is_some_and(|f| f.unquoted_reads().contains(&p));
                    match p.first().map(String::as_str) {
                        Some("in") => {
                            if let Some(name) = p.get(1).filter(|m| !row.input.contains_key(*m)) {
                                bad.push(here(format!("`in.{name}` is not an input of this flow")));
                            }
                        }
                        Some("steps") => {
                            let Some(id) = p.get(1) else { continue };
                            match at(id) {
                                None => bad.push(here(format!("`steps.{id}` is not a step of this flow"))),
                                Some(j) => {
                                    if p.get(2).map(String::as_str) == Some("out")
                                        && let Some(port) = p.get(3).filter(|port| !row.steps[j].out.contains_key(*port))
                                    {
                                        bad.push(here(format!("`steps.{id}.out.{port}`: step `{id}` declares no such port")));
                                    }
                                    if j != i && !came_after && !deps[i].contains(&j) {
                                        deps[i].push(j);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        for a in &s.after {
            match at(a) {
                None => bad.push(here(format!("`after` names `{a}`, which is not a step"))),
                Some(j) if j == i => bad.push(here("it comes after itself".into())),
                Some(j) => {
                    if !deps[i].contains(&j) {
                        deps[i].push(j);
                    }
                }
            }
        }
        // an entry that starts with `/` is a path the step reads, which a box shows it; the others are kinds that steps write
        for kind in s.reads.iter().filter(|r| !r.starts_with('/')) {
            let writers: Vec<usize> = row.steps.iter().enumerate().filter(|(j, w)| *j != i && w.out.values().any(|k| k == kind)).map(|(j, _)| j).collect();
            if writers.is_empty() {
                bad.push(here(format!("it `reads` {kind}, and no step writes it")));
            }
            for j in writers {
                if !deps[i].contains(&j) {
                    deps[i].push(j);
                }
            }
        }
    }
    if !bad.is_empty() {
        return Err(bad);
    }
    // the order: the dependencies first, ties in the order written; a cycle is named
    let (mut order, mut done) = (Vec::new(), vec![false; n]);
    while order.len() < n {
        match (0..n).find(|i| !done[*i] && deps[*i].iter().all(|d| done[*d])) {
            Some(i) => {
                done[i] = true;
                order.push(i);
            }
            None => {
                let stuck: Vec<&str> = (0..n).filter(|i| !done[*i]).map(|i| ids[i]).collect();
                return Err(vec![format!("the steps {} wait for each other: a loop goes back with a `branch` and `max_visits`, not with `after`", stuck.join(", "))]);
            }
        }
    }
    // a jump back needs a limit on it
    let place = |i: usize| order.iter().position(|o| *o == i).unwrap_or(0);
    for (i, s) in row.steps.iter().enumerate() {
        for target in [&s.yes, &s.no].into_iter().flatten().filter_map(|t| at(t)) {
            if place(target) <= place(i) && s.max_visits.is_none() {
                bad.push(format!("step `{}`: it goes back to `{}`, so it needs `max_visits`", s.id, ids[target]));
            }
        }
    }
    if bad.is_empty() { Ok(order) } else { Err(bad) }
}

impl Flows {
    pub fn load(tables: &Tables) -> (Flows, Vec<Problem>) {
        let (mut rows, mut problems) = (Vec::new(), Vec::new());
        match tables.list::<FlowRow>() {
            Ok(list) => rows = list,
            Err(p) => problems.push(p),
        }
        let known: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
        let mut passed: Vec<Flow> = Vec::new();
        for row in rows {
            let names: Vec<&str> = known.iter().map(String::as_str).collect();
            match lint(&row, &names) {
                Ok(order) => passed.push(Flow { row, order }),
                Err(bad) => problems.extend(bad.into_iter().map(|m| Problem::refused(format!("[[flow]] {}", row.id), m))),
            }
        }
        // a flow that runs itself, however many flows later, never ends
        let calls = |f: &Flow| -> Vec<String> { f.row.steps.iter().flat_map(|s| s.doer.iter().chain(s.each.iter())).filter_map(|d| d.strip_prefix("flow:").map(String::from)).collect() };
        let mut loops: Vec<String> = Vec::new();
        for f in &passed {
            let (mut seen, mut stack) = (vec![f.id().to_string()], calls(f));
            while let Some(next) = stack.pop() {
                if next == f.id() {
                    loops.push(f.id().to_string());
                    break;
                }
                if !seen.contains(&next) {
                    seen.push(next.clone());
                    stack.extend(passed.iter().find(|g| g.id() == next).map(&calls).unwrap_or_default());
                }
            }
        }
        for id in &loops {
            problems.push(Problem::refused(format!("[[flow]] {id}"), "it runs itself, through the flows it runs"));
        }
        passed.retain(|f| !loops.iter().any(|l| l == f.id()));
        (Flows(passed), problems)
    }

    pub fn all(&self) -> &[Flow] {
        &self.0
    }

    pub fn get(&self, id: &str) -> Option<&Flow> {
        self.0.iter().find(|f| f.id() == id)
    }
}
