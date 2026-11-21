//! Dialect `jsonl-map`'s reader: each JSON line of a program's output becomes what J4n shows, by the rows of the manifest's
//! `[connector.map]`. Every expression reads the line as its document (`/type == 'text'`), so the
//! program is described, not coded for. The expressions are [`Expr`]s: pure and total, so a line that lacks a field is no
//! event and no error.

use crate::manifest::MapSpec;
use j4n_core::doer::{Metrics, Role, RunEv};
use j4n_core::eval::Scope;
use j4n_core::expr::Expr;
use j4n_core::value::Value;
use serde_json::Value as Json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Emit {
    Text,
    Reasoning,
    Message,
    Tool,
    ToolEnd,
    Shell,
    Error,
    Notice,
    Changed,
    /// A tool that came back with an error: the run is failed if it is the last tool, whatever else is said after it.
    ToolError,
}

impl Emit {
    fn from_word(word: &str) -> Option<Emit> {
        Some(match word {
            "text" => Emit::Text,
            "reasoning" => Emit::Reasoning,
            "message" => Emit::Message,
            "tool" => Emit::Tool,
            "tool_end" => Emit::ToolEnd,
            "shell" => Emit::Shell,
            "error" => Emit::Error,
            "notice" => Emit::Notice,
            "changed" => Emit::Changed,
            "tool_error" => Emit::ToolError,
            _ => return None,
        })
    }
}

struct Row {
    when: Expr,
    emit: Emit,
    text: Option<Expr>,
    name: Option<Expr>,
    detail: Option<Expr>,
    cmd: Option<Expr>,
    out: Option<Expr>,
    exit: Option<Expr>,
}

pub struct Mapper {
    session: Option<Expr>,
    end: Option<Expr>,
    answer: Option<Expr>,
    cost: Option<Expr>,
    error: Option<Expr>,
    /// What the line says it used, for the metrics of the run: `tokens_in` without the cache parts, which are added to it.
    tokens_in: Option<Expr>,
    tokens_out: Option<Expr>,
    cached: Option<Expr>,
    wrote: Option<Expr>,
    /// Words of the program's stderr that say it refused a tool: the line that holds one is the reason a run that ended on a tool is failed.
    rejects: Vec<String>,
    /// Words of a tool's error that say the program's rules refused it.
    refused: Vec<String>,
    rows: Vec<Row>,
}

/// What one line meant.
#[derive(Debug, Default, PartialEq)]
pub struct Mapped {
    pub evs: Vec<RunEv>,
    pub session: Option<String>,
    /// The line is the program's own end of the run.
    pub end: bool,
    pub answer: Option<String>,
    pub cost: Option<f64>,
    /// The line says the run failed.
    pub error: Option<String>,
    /// What the line says it used.
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cached: u64,
    pub wrote: u64,
    /// A tool of the line came back with an error (the words of it), or one that did not: the last of the run decides.
    pub tool_failed: Option<String>,
    pub tool_ok: bool,
}

fn parse(text: &Option<String>, at: &str) -> Result<Option<Expr>, String> {
    let Some(t) = text else { return Ok(None) };
    let e = Expr::parse(t).map_err(|e| format!("{at}: `{t}` is not an expression: {e}"))?;
    match e.check(Some(&[])).first() {
        Some(problem) => Err(format!("{at}: `{t}`: {problem}")),
        None => Ok(Some(e)),
    }
}

/// A value as the words it is shown with.
pub fn show(v: &Value) -> String {
    match v {
        Value::Missing => String::new(),
        Value::Str(s) => s.clone(),
        Value::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => format!("{}", *n as i64),
        other => json_of(other).to_string(),
    }
}

fn json_of(v: &Value) -> Json {
    match v {
        Value::Missing => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::Num(n) => serde_json::Number::from_f64(*n).map_or(Json::Null, Json::Number),
        Value::Str(s) => Json::String(s.clone()),
        Value::Dur(ms) => Json::from(*ms),
        Value::Usd(micros) => Json::from(*micros as f64 / 1e6),
        Value::List(items) => Json::Array(items.iter().map(json_of).collect()),
        Value::Map(m) => Json::Object(m.iter().map(|(k, v)| (k.clone(), json_of(v))).collect()),
    }
}

impl Mapper {
    pub fn new(spec: &MapSpec) -> Result<Mapper, String> {
        let mut rows = Vec::new();
        for (i, e) in spec.events.iter().enumerate() {
            let at = format!("map.events[{i}]");
            let emit = Emit::from_word(&e.emit).ok_or_else(|| format!("{at}: `{}` is not something a line can emit", e.emit))?;
            let when = parse(&Some(e.when.clone()), &at)?.ok_or("a row needs `when`")?;
            rows.push(Row {
                when,
                emit,
                text: parse(&e.text, &at)?,
                name: parse(&e.name, &at)?,
                detail: parse(&e.detail, &at)?,
                cmd: parse(&e.cmd, &at)?,
                out: parse(&e.out, &at)?,
                exit: parse(&e.exit, &at)?,
            });
        }
        Ok(Mapper {
            session: parse(&spec.session, "map.session")?,
            end: parse(&spec.end, "map.end")?,
            answer: parse(&spec.answer, "map.answer")?,
            cost: parse(&spec.cost, "map.cost")?,
            error: parse(&spec.error, "map.error")?,
            tokens_in: parse(&spec.tokens_in, "map.tokens_in")?,
            tokens_out: parse(&spec.tokens_out, "map.tokens_out")?,
            cached: parse(&spec.cached, "map.cached")?,
            wrote: parse(&spec.wrote, "map.wrote")?,
            rejects: spec.rejects.iter().map(|r| r.to_lowercase()).collect(),
            refused: spec.refused.iter().map(|r| r.to_lowercase()).collect(),
            rows,
        })
    }

    /// The map says how a run ends (`end`): a run that stops without it did not finish.
    pub fn expects_end(&self) -> bool {
        self.end.is_some()
    }

    /// The line of `stderr` that says the program refused a tool, if one does.
    pub fn rejection(&self, stderr: &str) -> Option<String> {
        stderr.lines().find(|l| {
            let l = l.to_lowercase();
            self.rejects.iter().any(|r| l.contains(r.as_str()))
        }).map(|l| l.trim().to_string())
    }

    /// Is `error` the program saying its rules refused a tool call?
    fn refusal(&self, error: &str) -> bool {
        let low = error.to_lowercase();
        self.refused.iter().any(|w| low.contains(w.as_str()))
    }

    pub fn line(&self, doc: &Json) -> Mapped {
        let scope = Scope::new().doc(Value::from_json(doc));
        let get = |e: &Option<Expr>| e.as_ref().map(|e| e.eval(&scope)).unwrap_or_default();
        let text = |e: &Option<Expr>| show(&get(e));
        let mut out = Mapped::default();
        for row in self.rows.iter().filter(|r| r.when.holds(&scope)) {
            let t = text(&row.text);
            let ev = match row.emit {
                Emit::Text if !t.is_empty() => Some(RunEv::Text(t)),
                Emit::Reasoning if !t.is_empty() => Some(RunEv::Reasoning(t)),
                Emit::Message if !t.is_empty() => Some(RunEv::Message { role: Role::Assistant, text: t }),
                Emit::Tool | Emit::ToolEnd => {
                    let name = text(&row.name);
                    let detail = text(&row.detail);
                    let detail = if row.emit == Emit::ToolEnd && self.refusal(&detail) { crate::permit::refusal(&format!("the `{name}` tool")) } else { detail };
                    Some(RunEv::Tool { name: if name.is_empty() { "tool".into() } else { name }, detail, end: row.emit == Emit::ToolEnd })
                }
                Emit::Shell => {
                    let exit = match get(&row.exit) {
                        Value::Num(n) => n as i32,
                        Value::Str(s) => s.trim().parse().unwrap_or(1),
                        Value::Bool(false) => 1,
                        _ => 0,
                    };
                    let (cmd, out) = (text(&row.cmd), text(&row.out));
                    // a line the program's rules refused comes back with its whole ruleset as the error: one sentence says it
                    let out = if exit != 0 && self.refusal(&out) { crate::permit::refusal(&cmd) } else { out };
                    Some(RunEv::Shell { cmd, out, exit })
                }
                Emit::Error if !t.is_empty() => Some(RunEv::Error(t)),
                Emit::Notice if !t.is_empty() => Some(RunEv::Notice(t)),
                Emit::Changed => {
                    let paths = match get(&row.text) {
                        Value::List(items) => items.iter().map(show).filter(|p| !p.is_empty()).collect(),
                        Value::Missing => Vec::new(),
                        one => vec![show(&one)],
                    };
                    (!paths.is_empty()).then_some(RunEv::Changed(paths))
                }
                _ => None,
            };
            match (&ev, row.emit) {
                (Some(RunEv::Tool { .. } | RunEv::Shell { .. }), _) => out.tool_ok = true,
                (_, Emit::ToolError) => {
                    let mut why = text(&row.text);
                    if self.refusal(&why) {
                        let cmd = text(&row.cmd);
                        why = crate::permit::refusal(if cmd.is_empty() { "a tool call" } else { &cmd });
                    }
                    out.tool_failed = Some(if why.is_empty() { "it came back with an error".to_string() } else { why });
                }
                _ => {}
            }
            out.evs.extend(ev);
        }
        let session = text(&self.session);
        out.session = (!session.is_empty()).then_some(session);
        out.end = self.end.as_ref().is_some_and(|e| e.holds(&scope));
        let answer = text(&self.answer);
        out.answer = (!answer.is_empty()).then_some(answer);
        out.cost = match get(&self.cost) {
            Value::Num(n) => Some(n),
            Value::Str(s) => s.trim().trim_start_matches('$').parse().ok(),
            _ => None,
        };
        let count = |e: &Option<Expr>| match get(e) {
            Value::Num(n) if n.is_finite() && n > 0.0 => n as u64,
            _ => 0,
        };
        (out.tokens_in, out.tokens_out, out.cached, out.wrote) = (count(&self.tokens_in), count(&self.tokens_out), count(&self.cached), count(&self.wrote));
        let error = text(&self.error);
        out.error = (!error.is_empty()).then_some(error);
        out
    }
}

impl Mapped {
    /// The cost and the tokens as the loop counts them: `tokens_in` is every token read, the cache parts in it.
    pub fn metrics(&self) -> Option<Metrics> {
        let usd = self.cost.filter(|c| *c > 0.0).unwrap_or(0.0);
        let tokens_in = self.tokens_in.saturating_add(self.cached).saturating_add(self.wrote);
        (usd > 0.0 || tokens_in > 0 || self.tokens_out > 0).then_some(Metrics { usd, tokens_in, tokens_out: self.tokens_out, cached: self.cached, wrote: self.wrote, ..Metrics::default() })
    }
}
