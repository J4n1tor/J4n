//! The consumers' view: every expression, template and query the design's examples show (the language reference, the
//! screen's field kinds and the boards), written the way the example writes it,
//! asked in a hand-made world, with the names the place it sits in would offer. A row passes when the text
//! (1) parses, (2) passes `check` with those names, (3) answers what the example means and (4) prints back to the same tree.
//! `docs/TESTING.md` holds no expression or template (it names rules in prose only).
//!
//! What a doc writes and the language cannot express is not a row. It is a `gap_*` test that quotes the doc line and
//! pins today's answer, so a change to the language shows up here. Rows marked "derived" are the plausible spelling of
//! something the doc names without writing it (stall, soft cap, compaction, the `.env` floor).

use j4n_core::eval::{Env, Scope, glob_match};
use j4n_core::expr::{Expr, Func, Op, Seg};
use j4n_core::fmt::{Fmt, Format, MAX_FMT, Piece, SHORT_MAX, format_value};
use j4n_core::query::{Coll, Query, RowEnv};
use j4n_core::value::{DASH, Key, Value};
use std::collections::BTreeMap;

// ── small builders ───────────────────────────────────────────────────────────────────────────

fn m(pairs: &[(&str, Value)]) -> Value {
    Value::Map(pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect::<BTreeMap<_, _>>())
}

fn s(t: &str) -> Value {
    Value::text(t)
}

fn n(x: f64) -> Value {
    Value::Num(x)
}

fn l(items: Vec<Value>) -> Value {
    Value::List(items)
}

fn usd(d: f64) -> Value {
    Value::usd(d)
}

fn secs(x: u64) -> Value {
    Value::Dur(x * 1000)
}

fn yes() -> Value {
    Value::Bool(true)
}

fn no() -> Value {
    Value::Bool(false)
}

/// What `danger(cmd)` answers in these worlds: a map with the matcher's class (the real matcher is the Gate's).
fn danger(cmd: &str) -> Value {
    let class = if cmd.contains("git push") {
        "push"
    } else if cmd.starts_with("rm ") {
        "delete"
    } else {
        "read"
    };
    m(&[("class", s(class))])
}

/// A connector frame, the document a `jsonl-map` pointer reads.
fn frame(kind: &str) -> Value {
    match kind {
        "text" => m(&[("type", s("text")), ("text", s("hello"))]),
        "tool" => m(&[("type", s("tool_use")), ("name", s("Read")), ("input", m(&[("path", s("src/a.rs"))]))]),
        "error" => m(&[("type", s("error")), ("message", s("boom"))]),
        _ => m(&[("type", s("result")), ("session_id", s("s-1")), ("usage", m(&[("cost_usd", n(0.0123))]))]),
    }
}

fn on(doc: Value) -> Scope {
    Scope::new().doc(doc)
}

/// The world of a Rule or a Gate over one event.
fn w_event() -> Scope {
    let data = m(&[
        ("cmd", s("git push origin main")),
        ("files", l(vec![s("a.rs"), s("b.rs"), s("c.rs")])),
        ("exits", l(vec![n(0.0), n(0.0), n(0.0)])),
        ("name", s("README.md")),
        ("text", s("say \"hi\"\nnow")),
    ]);
    Scope::new()
        .with("event", m(&[("at_ms", n(1_000_000.0)), ("data", data)]))
        .with("task", m(&[("state", s("running"))]))
        .with("fold", m(&[("spend", m(&[("today", usd(1.24)), ("by_task", l(vec![usd(0.5), usd(0.25)]))]))]))
        .at_ms(1_090_000)
        .danger_with(danger)
}

fn w_gate(cmd: &str) -> Scope {
    Scope::new().with("cmd", s(cmd)).danger_with(danger)
}

/// Continue-until-done: the event, the follow-up fold keyed by the task's id, the setting.
fn w_rule(todo_left: f64, followups: f64) -> Scope {
    Scope::new()
        .with("ev", m(&[("todo_left", n(todo_left))]))
        .with("task", s("t1"))
        .with("fold", m(&[("followups", m(&[("t1", n(followups))]))]))
        .with("set", m(&[("loop", m(&[("followups", n(2.0))]))]))
}

fn w_flow(pass: bool) -> Scope {
    Scope::new().with(
        "steps",
        m(&[
            ("plan", m(&[("out", m(&[("plan", s("1. read the parser\n2. fix commas"))]))])),
            ("work", m(&[("doer", s("omp")), ("place", m(&[("diff", s("--- a/p.rs\n+++ b/p.rs"))]))])),
            ("validate", m(&[("out", m(&[("verdict", m(&[("pass", Value::Bool(pass))]))]))])),
        ]),
    )
}

// ── the runners ──────────────────────────────────────────────────────────────────────────────

/// One expression example: where the doc writes it, the text as written, the names its place offers, the world it is
/// Asked in, and what the doc means it to answer.
struct Ex {
    at: &'static str,
    src: &'static str,
    roots: &'static [&'static str],
    world: fn() -> Scope,
    want: Value,
    /// The place only asks whether it holds (a Rule's `when`, a node's `when`): compare `holds`, not the value.
    cond: bool,
}

fn ex(at: &'static str, src: &'static str, roots: &'static [&'static str], world: fn() -> Scope, want: Value) -> Ex {
    Ex { at, src, roots, world, want, cond: false }
}

/// A row for a `when`: the answer is yes or no, whatever the expression's own value is.
fn cond(at: &'static str, src: &'static str, roots: &'static [&'static str], world: fn() -> Scope, holds: bool) -> Ex {
    Ex { at, src, roots, world, want: Value::Bool(holds), cond: true }
}

/// One template example.
struct Tx {
    at: &'static str,
    src: &'static str,
    roots: &'static [&'static str],
    world: fn() -> Scope,
    want: String,
}

fn tx(at: &'static str, src: &'static str, roots: &'static [&'static str], world: fn() -> Scope, want: &str) -> Tx {
    Tx { at, src, roots, world, want: want.to_string() }
}

fn run(rows: Vec<Ex>) {
    let mut bad = Vec::new();
    for r in &rows {
        let e = match Expr::parse(r.src) {
            Ok(e) => e,
            Err(err) => {
                bad.push(format!("{}  `{}`  does not parse: {err}", r.at, r.src));
                continue;
            }
        };
        let seen = e.check(Some(r.roots));
        if !seen.is_empty() {
            bad.push(format!("{}  `{}`  check says {seen:?}", r.at, r.src));
        }
        let env = (r.world)();
        let got = if r.cond { Value::Bool(e.holds(&env)) } else { e.eval(&env) };
        if got != r.want {
            bad.push(format!("{}  `{}`  is {got:?}, the doc means {:?}", r.at, r.src, r.want));
        }
        if !matches!(Expr::parse(&e.to_string()), Ok(ref back) if *back == e) {
            bad.push(format!("{}  `{}`  prints back as `{e}`, which does not read the same", r.at, r.src));
        }
    }
    assert!(bad.is_empty(), "{} of {} rows:\n{}", bad.len(), rows.len(), bad.join("\n"));
}

fn run_fmt(rows: Vec<Tx>) {
    let mut bad = Vec::new();
    for r in &rows {
        let f = match Fmt::parse(r.src) {
            Ok(f) => f,
            Err(err) => {
                bad.push(format!("{}  `{}`  does not parse: {err}", r.at, r.src));
                continue;
            }
        };
        let seen = f.check(Some(r.roots));
        if !seen.is_empty() {
            bad.push(format!("{}  `{}`  check says {seen:?}", r.at, r.src));
        }
        let got = f.render(&(r.world)());
        if got != r.want {
            bad.push(format!("{}  `{}`  shows {got:?}, the doc means {:?}", r.at, r.src, r.want));
        }
        if !matches!(Fmt::parse(&f.to_string()), Ok(ref back) if *back == f) {
            bad.push(format!("{}  `{}`  prints back as `{f}`, which does not read the same", r.at, r.src));
        }
    }
    assert!(bad.is_empty(), "{} of {} rows:\n{}", bad.len(), rows.len(), bad.join("\n"));
}

// ── the rows ─────────────────────────────────────────────────────────────────────────────────

/// "paths (`event.data.cmd`, `task.state`, `fold.spend.today`, `/json/pointer`
/// Inside connector maps), literals (string, number, bool, duration `5m`, money `$1.50`)".
fn paths_and_literals() -> Vec<Ex> {
    let lit = |at, src, want| ex(at, src, &[], Scope::new, want);
    vec![
        ex("path", "event.data.cmd", &["event"], w_event, s("git push origin main")),
        ex("path", "task.state", &["task"], w_event, s("running")),
        ex("path", "fold.spend.today", &["fold"], w_event, usd(1.24)),
        ex("pointer", "/json/pointer", &[], || on(m(&[("json", m(&[("pointer", n(1.0))]))])), n(1.0)),
        lit("string", "'text'", s("text")),
        lit("string", "\"text\"", s("text")),
        lit("number", "12", n(12.0)),
        lit("number", "-3", n(-3.0)),
        lit("number", "0.5", n(0.5)),
        lit("bool", "true", yes()),
        lit("bool", "false", no()),
        lit("duration `5m`", "5m", Value::Dur(300_000)),
        lit("duration", "250ms", Value::Dur(250)),
        lit("duration", "1h30m", Value::Dur(5_400_000)),
        lit("money `$1.50`", "$1.50", Value::Usd(1_500_000)),
        lit("list", "['a', 2]", l(vec![s("a"), n(2.0)])),
    ]
}

/// "`== != < <= > >= and or not in matches(glob)`", one row each (and `not in`).
fn operators() -> Vec<Ex> {
    let at = "operators";
    vec![
        ex(at, "task.state == 'running'", &["task"], w_event, yes()),
        ex(at, "task.state != 'done'", &["task"], w_event, yes()),
        ex(at, "fold.spend.today < $5", &["fold"], w_event, yes()),
        ex(at, "fold.spend.today <= $1.24", &["fold"], w_event, yes()),
        ex(at, "fold.spend.today > $1", &["fold"], w_event, yes()),
        ex(at, "fold.spend.today >= $2", &["fold"], w_event, no()),
        ex(at, "task.state == 'running' and fold.spend.today < $5", &["task", "fold"], w_event, yes()),
        ex(at, "task.state == 'done' or fold.spend.today > $1", &["task", "fold"], w_event, yes()),
        ex(at, "not task.state == 'done'", &["task"], w_event, yes()),
        ex(at, "task.state in ['running', 'cooling']", &["task"], w_event, yes()),
        ex(at, "task.state not in ['done', 'stopped']", &["task"], w_event, yes()),
        ex(at, "'push' in event.data.cmd", &["event"], w_event, yes()),
        ex(at, "event.data.cmd matches 'git push*'", &["event"], w_event, yes()),
        ex(at, "event.data.cmd matches 'git pull*'", &["event"], w_event, no()),
    ]
}

/// "closed functions (`count same last since sum len lower contains danger quote`)", one row each.
fn functions() -> Vec<Ex> {
    let at = "functions";
    vec![
        ex(at, "count(event.data.files)", &["event"], w_event, n(3.0)),
        ex(at, "count(event.data.files, 'b.rs')", &["event"], w_event, n(1.0)),
        ex(at, "same(event.data.exits)", &["event"], w_event, yes()),
        ex(at, "last(event.data.files)", &["event"], w_event, s("c.rs")),
        ex(at, "since(event.at_ms)", &["event"], w_event, Value::Dur(90_000)),
        ex(at, "sum(fold.spend.by_task)", &["fold"], w_event, usd(0.75)),
        ex(at, "len(event.data.cmd)", &["event"], w_event, n(20.0)),
        ex(at, "lower(event.data.name)", &["event"], w_event, s("readme.md")),
        ex(at, "contains(event.data.cmd, 'push')", &["event"], w_event, yes()),
        ex(at, "danger(event.data.cmd).class", &["event"], w_event, s("push")),
        ex(at, "quote(event.data.text)", &["event"], w_event, s("\"say \\\"hi\\\" now\"")),
    ]
}

/// The `jsonl-map` of a never-seen agent.
fn connector_maps() -> Vec<Ex> {
    vec![
        ex("session", "/session_id", &[], || on(frame("result")), s("s-1")),
        ex("end", "/type == 'result'", &[], || on(frame("result")), yes()),
        ex("end, a text frame", "/type == 'result'", &[], || on(frame("text")), no()),
        ex("cost", "/usage/cost_usd", &[], || on(frame("result")), n(0.0123)),
        ex("cost, a frame without usage", "/usage/cost_usd", &[], || on(frame("text")), Value::Missing),
        ex("when", "/type == 'text'", &[], || on(frame("text")), yes()),
        ex("when, a frame without type", "/type == 'text'", &[], || on(m(&[])), no()),
        ex("text", "/text", &[], || on(frame("text")), s("hello")),
        ex("when", "/type == 'tool_use'", &[], || on(frame("tool")), yes()),
        ex("name", "/name", &[], || on(frame("tool")), s("Read")),
        ex("detail", "/input", &[], || on(frame("tool")), m(&[("path", s("src/a.rs"))])),
        ex("when", "/type == 'error'", &[], || on(frame("error")), yes()),
        ex("text", "/message", &[], || on(frame("error")), s("boom")),
    ]
}

/// The gate `when`; plus the `.env` / `.git/` floor (derived).
fn gates() -> Vec<Ex> {
    let at = "gate";
    vec![
        ex(at, "danger(cmd).class == 'push'", &["cmd"], || w_gate("git push origin main"), yes()),
        ex(at, "danger(cmd).class == 'push'", &["cmd"], || w_gate("ls -la"), no()),
        ex(at, "danger(cmd).class == 'push'", &["cmd"], || w_gate("rm -rf build"), no()),
        // no matcher behind `danger`: nothing is known of the line, so nothing is said of it
        ex(at, "danger(cmd).class == 'push'", &["cmd"], || Scope::new().with("cmd", s("git push")), no()),
        ex(
            "derived",
            "target matches '*/.env*'",
            &["target"],
            || Scope::new().with("target", s("/w/p/.env.local")),
            yes(),
        ),
        ex(
            "derived",
            "target matches '*/.env*'",
            &["target"],
            || Scope::new().with("target", s("/w/p/src/main.rs")),
            no(),
        ),
        ex(
            "derived",
            "'/.git/' in target",
            &["target"],
            || Scope::new().with("target", s("/w/p/.git/config")),
            yes(),
        ),
    ]
}

/// The Rule `when`s, and the ones the plan only names (derived).
fn rules() -> Vec<Ex> {
    let at = "continue-until-done";
    let cont = "ev.todo_left > 0 and fold.followups[task] < set.loop.followups";
    let names = &["ev", "fold", "task", "set"];
    vec![
        ex(at, cont, names, || w_rule(3.0, 1.0), yes()),
        ex(at, cont, names, || w_rule(3.0, 2.0), no()),
        ex(at, cont, names, || w_rule(0.0, 1.0), no()),
        // no fold yet for this task: nothing is smaller than nothing
        ex(at, cont, names, || Scope::new().with("ev", m(&[("todo_left", n(3.0))])).with("task", s("t9")), no()),
        cond(
            "lane_check",
            "changed_files",
            &["changed_files"],
            || Scope::new().with("changed_files", l(vec![s("a.rs")])),
            true,
        ),
        cond(
            "lane_check, none",
            "changed_files",
            &["changed_files"],
            || Scope::new().with("changed_files", l(vec![])),
            false,
        ),
        cond("lane_check, unknown", "changed_files", &["changed_files"], Scope::new, false),
        // "context > 70 %" cannot be written (gap_percent_is_not_a_literal); as a fraction it can
        ex("derived", "context > 0.7", &["context"], || Scope::new().with("context", n(0.82)), yes()),
        ex("derived", "context > 0.7", &["context"], || Scope::new().with("context", n(0.45)), no()),
        ex(
            "stall, derived",
            "since(fold.last_ms[task]) > 5m",
            &["fold", "task"],
            || {
                Scope::new()
                    .with("task", s("t1"))
                    .with("fold", m(&[("last_ms", m(&[("t1", n(600_000.0))]))]))
                    .at_ms(1_000_000)
            },
            yes(),
        ),
        ex(
            "stall, derived",
            "since(fold.last_ms[task]) > 5m",
            &["fold", "task"],
            || {
                Scope::new()
                    .with("task", s("t1"))
                    .with("fold", m(&[("last_ms", m(&[("t1", n(900_000.0))]))]))
                    .at_ms(1_000_000)
            },
            no(),
        ),
        ex(
            "soft cap, derived",
            "fold.spend[task] >= limits.usd_soft",
            &["fold", "task", "limits"],
            || {
                Scope::new()
                    .with("task", s("t1"))
                    .with("fold", m(&[("spend", m(&[("t1", usd(5.2))]))]))
                    .with("limits", m(&[("usd_soft", usd(5.0))]))
            },
            yes(),
        ),
        ex(
            "soft cap, derived",
            "fold.spend[task] >= limits.usd_soft",
            &["fold", "task", "limits"],
            || {
                Scope::new()
                    .with("task", s("t1"))
                    .with("fold", m(&[("spend", m(&[("t1", usd(4.0))]))]))
                    .with("limits", m(&[("usd_soft", usd(5.0))]))
            },
            no(),
        ),
    ]
}

/// The branch step's `on`, an Expr whose answer picks `yes` or `no`.
fn flows() -> Vec<Ex> {
    let at = "branch";
    let src = "steps.validate.out.verdict.pass";
    vec![
        ex(at, src, &["steps"], || w_flow(true), yes()),
        ex(at, src, &["steps"], || w_flow(false), no()),
        ex(at, src, &["steps"], Scope::new, Value::Missing),
    ]
}

/// Durations and money the docs type into a field or a setting, and the
/// Value-fields board ("like 90s · 5m · 1h").
fn typed_values() -> Vec<Ex> {
    let lit = |at, src, want| ex(at, src, &[], Scope::new, want);
    vec![
        lit("`after = 60s`", "60s", Value::Dur(60_000)),
        lit("Value-fields board `wall`", "90s", Value::Dur(90_000)),
        lit("Value-fields board `wall`", "1h", Value::Dur(3_600_000)),
        lit("`$5.00`", "$5.00", Value::Usd(5_000_000)),
        lit("Value-fields board `soft cap`", "$2.5", Value::Usd(2_500_000)),
        lit("Value-fields board `soft cap`", "$5", Value::Usd(5_000_000)),
    ]
}

fn templates() -> Vec<Tx> {
    let argv = "argv";
    let a = |src: &'static str, root: &'static [&'static str], v: &'static str| -> Tx {
        // each hole is its own argv element; the value is whatever the operator or a model typed
        let world: fn() -> Scope = match root[0] {
            "model" => || Scope::new().with("model", s("haiku")),
            "session" => || Scope::new().with("session", s("s-1")),
            "prompt" => || Scope::new().with("prompt", s("fix the parser")),
            "pattern" => || Scope::new().with("pattern", s("fn main")),
            _ => || Scope::new().with("path", s(".")),
        };
        tx(argv, src, root, world, v)
    };
    vec![
        a("{model}", &["model"], "haiku"),
        a("{session}", &["session"], "s-1"),
        a("{prompt}", &["prompt"], "fix the parser"),
        a("{pattern}", &["pattern"], "fn main"),
        a("{path}", &["path"], "."),
        // a value that holds braces is text, not another template
        tx(
            argv,
            "{prompt}",
            &["prompt"],
            || Scope::new().with("prompt", s("use {model} }} {{x}")),
            "use {model} }} {{x}",
        ),
        tx("fold key", "{task}", &["task"], || Scope::new().with("task", s("t-42")), "t-42"),
        tx(
            "show",
            "{steps.plan.out.plan}",
            &["steps"],
            || w_flow(true),
            "1. read the parser\n2. fix commas",
        ),
        tx(
            "goal",
            "{steps.plan.out.plan}",
            &["steps"],
            || w_flow(true),
            "1. read the parser\n2. fix commas",
        ),
        tx("exclude", "{steps.work.doer}", &["steps"], || w_flow(true), "omp"),
        tx("change", "{steps.work.place.diff}", &["steps"], || w_flow(true), "--- a/p.rs\n+++ b/p.rs"),
        tx(
            "ports `{steps.X.out.Y}`",
            "{steps.plan.out.plan}",
            &["steps"],
            || w_flow(true),
            "1. read the parser\n2. fix commas",
        ),
        // a connector map's pointer in a hole, through a formatter
        tx("cost", "{/usage/cost_usd:usd}", &[], || on(frame("result")), "$0.01"),
        tx("cost, no usage", "{/usage/cost_usd:usd}", &[], || on(frame("text")), DASH),
        tx(
            "bind",
            "table:connector/{selected}",
            &["selected"],
            || Scope::new().with("selected", s("claude")),
            "table:connector/claude",
        ),
        tx("`show`", "{id}", &["id"], || Scope::new().with("id", s("claude")), "claude"),
        // right-aligned stats (`wall cost` on lanes, `+ −` on edits, counts on merged rows)
        tx(
            "`21s  $0.03`",
            "{lane.wall:dur}  {lane.cost:usd}",
            &["lane"],
            || Scope::new().with("lane", m(&[("wall", secs(21)), ("cost", usd(0.03))])),
            "21s  $0.03",
        ),
        tx(
            "`+12 −3`",
            "+{edit.added} −{edit.removed}",
            &["edit"],
            || Scope::new().with("edit", m(&[("added", n(12.0)), ("removed", n(3.0))])),
            "+12 −3",
        ),
        tx(
            "`3 files`",
            "{lane.files} files",
            &["lane"],
            || Scope::new().with("lane", m(&[("files", n(3.0))])),
            "3 files",
        ),
        tx(
            "`exit 0`",
            "exit {step.exit}",
            &["step"],
            || Scope::new().with("step", m(&[("exit", n(0.0))])),
            "exit 0",
        ),
        tx(
            "Composer board tag pill `~88%`",
            "~{tag.conf:pct}",
            &["tag"],
            || Scope::new().with("tag", m(&[("conf", n(0.88))])),
            "~88%",
        ),
        tx("Composer board gauge `41%`", "{ctx:pct}", &["ctx"], || Scope::new().with("ctx", n(0.41)), "41%"),
        tx(
            "`▲ 12%`",
            "▲ {stat.delta:pct}",
            &["stat"],
            || Scope::new().with("stat", m(&[("delta", n(0.12))])),
            "▲ 12%",
        ),
        tx(
            "`$5.00`",
            "{limits.usd_soft:usd}",
            &["limits"],
            || Scope::new().with("limits", m(&[("usd_soft", usd(5.0))])),
            "$5.00",
        ),
        tx("FieldsDisplay board stat `$1.24`", "{fold.spend.today:usd}", &["fold"], w_event, "$1.24"),
        tx(
            "multi `2/4`",
            "{count(picked)}/{len(options)}",
            &["picked", "options"],
            || {
                Scope::new()
                    .with("picked", l(vec![s("read"), s("write")]))
                    .with("options", l(vec![s("read"), s("write"), s("net"), s("exec")]))
            },
            "2/4",
        ),
        tx(
            "map `2 keys`",
            "{len(env)} keys",
            &["env"],
            || Scope::new().with("env", m(&[("RUST_LOG", s("info")), ("TOKEN", s("x"))])),
            "2 keys",
        ),
        tx(
            "text field `first line + …`",
            "{task.goal:short}",
            &["task"],
            || {
                Scope::new().with(
                    "task",
                    m(&[("goal", s("Fix trailing commas in the parser\nand add a regression test for the lexer"))]),
                )
            },
            "Fix trailing commas in the parser…",
        ),
        // the closed formatters, each once (`usd pct n dur text short`)
        tx(
            "formatters",
            "{a:usd} {b:pct} {c:n} {d:dur} {e:text} {f:short}",
            &["a", "b", "c", "d", "e", "f"],
            || {
                Scope::new()
                    .with("a", usd(12.5))
                    .with("b", n(0.666))
                    .with("c", n(2.345))
                    .with("d", secs(185))
                    .with("e", s("plant"))
                    .with("f", n(1234.0))
            },
            "$12.50 67% 2.35 3m05s plant 1.2k",
        ),
        // the seventh, the words of a chain
        tx(
            "rungs",
            "{chain:rungs}",
            &["chain"],
            || Scope::new().with("chain", Value::List(vec![s("rule:fold"), s("decider:jev?min_conf=0.6"), s("human")])),
            "a rule, a decider, then you",
        ),
        // the eighth and the ninth, the keys of a run and the words of a fallback
        tx("runs", "{keys:runs}", &["keys"], || Scope::new().with("keys", Value::List(vec![s("1"), s("2"), s("3"), s("4")])), "1–4"),
        tx("fallback", "{fb:fallback}", &["fb"], || Scope::new().with("fb", s("pick:add")), "picks add"),
    ]
}

// ── the tests over the rows ──────────────────────────────────────────────────────────────────

#[test]
fn paths_and_literals_of_the_value_types_section() {
    run(paths_and_literals());
}

#[test]
fn every_operator_of_the_value_types_section() {
    run(operators());
}

#[test]
fn every_function_of_the_value_types_section() {
    run(functions());
}

#[test]
fn a_jsonl_map_reads_frames_by_pointer() {
    run(connector_maps());
}

#[test]
fn a_frame_matches_one_event_row_of_the_map_and_the_end_row_apart() {
    // `end`, then `events = [text, tool_use, error]`; each frame is exactly one of them.
    let whens = ["/type == 'text'", "/type == 'tool_use'", "/type == 'error'"].map(|w| Expr::parse(w).unwrap());
    let end = Expr::parse("/type == 'result'").unwrap();
    for (kind, row) in [("text", Some(0)), ("tool", Some(1)), ("error", Some(2)), ("result", None)] {
        let env = on(frame(kind));
        let hit: Vec<usize> = (0..3).filter(|i| whens[*i].holds(&env)).collect();
        assert_eq!(hit, row.into_iter().collect::<Vec<_>>(), "a `{kind}` frame");
        assert_eq!(end.holds(&env), kind == "result", "`end` on a `{kind}` frame");
    }
}

#[test]
fn the_gate_asks_what_danger_says() {
    run(gates());
}

#[test]
fn the_rules_the_plan_names() {
    run(rules());
}

#[test]
fn a_branch_on_a_verdict() {
    run(flows());
}

#[test]
fn values_typed_into_a_field() {
    run(typed_values());
}

#[test]
fn templates_of_connectors_flows_folds_and_the_tui() {
    run_fmt(templates());
}

#[test]
fn every_function_operator_and_formatter_the_plan_names_has_a_row() {
    // risk: "a new step kind/reducer/function needs two consumers" - a function added to `Func::ALL` comes here first.
    let exprs: Vec<Ex> =
        [paths_and_literals(), operators(), functions(), connector_maps(), gates(), rules(), flows(), typed_values()]
            .into_iter()
            .flatten()
            .collect();
    let fmts = templates();
    let text: String = exprs.iter().map(|r| r.src).chain(fmts.iter().map(|r| r.src)).collect::<Vec<_>>().join(" ");
    for f in Func::ALL {
        assert!(text.contains(&format!("{}(", f.word())), "no row calls `{}`", f.word());
    }
    for f in Format::ALL {
        assert!(
            fmts.iter().any(|r| r.src.contains(&format!(":{}", f.word()))),
            "no template uses the formatter `{}`",
            f.word()
        );
    }
    let ops = [Op::Eq, Op::Ne, Op::Lt, Op::Le, Op::Gt, Op::Ge, Op::In, Op::Matches];
    for op in ops {
        let w = format!(" {} ", op.word());
        assert!(text.contains(&w), "no row uses `{}`", op.word());
    }
    for w in [" and ", " or ", "not ", " not in "] {
        assert!(text.contains(w), "no row uses `{}`", w.trim());
    }
}

// ── queries ────────────────────────────────────────

fn tasks() -> Vec<Value> {
    vec![
        m(&[("id", s("t1")), ("state", s("running")), ("doer", s("claude")), ("usd", usd(0.9))]),
        m(&[("id", s("t2")), ("state", s("queued")), ("doer", s("pi")), ("usd", usd(0.1))]),
        m(&[("id", s("t3")), ("state", s("running")), ("doer", s("omp")), ("usd", usd(0.6))]),
        m(&[("id", s("t4")), ("state", s("done")), ("doer", s("claude"))]),
    ]
}

fn ids(rows: &[Value]) -> Vec<String> {
    rows.iter()
        .map(|r| match r.at(&[Key::Name("id".into())]) {
            Value::Str(t) => t,
            other => format!("{other:?}"),
        })
        .collect()
}

#[test]
fn every_collection_of_the_plan_parses_and_prints_back() {
    // `Coll = Tasks | Asks | Changes | Log(kind glob) | Table(kind) | Fold(id) | Thread(id)`
    let rows = [
        ("tasks", Coll::Tasks),
        ("asks", Coll::Asks),
        ("changes", Coll::Changes),
        ("log:run.*", Coll::Log("run.*".into())),
        ("table:connector", Coll::Table("connector".into())),
        ("fold:spend", Coll::Fold("spend".into())),
        ("thread:current", Coll::Thread("current".into())),
    ];
    for (text, want) in rows {
        let got = Coll::parse(text).unwrap_or_else(|e| panic!("`{text}`: {e}"));
        assert_eq!(got, want, "`{text}`");
        assert_eq!(got.to_string(), text);
    }
}

#[test]
fn tasks_in_a_state_by_spend_most_first_at_most_two() {
    // `Query { from, filter, order, limit }`, over the Tasks page of the F1 tree.
    let q = Query::parse("tasks", Some("state in ['running', 'queued']"), Some("-usd, id"), Some(2)).unwrap();
    assert!(q.check(None).is_empty(), "{:?}", q.check(None));
    assert_eq!(ids(&q.apply(tasks(), &Scope::new())), ["t1", "t3"]);
    // a task that was never priced has no spend: it sorts after every priced one, either way round
    let all = Query::parse("tasks", None, Some("-usd"), None).unwrap();
    assert_eq!(ids(&all.apply(tasks(), &Scope::new())), ["t1", "t3", "t2", "t4"]);
    let up = Query::parse("tasks", None, Some("usd"), None).unwrap();
    assert_eq!(ids(&up.apply(tasks(), &Scope::new())), ["t2", "t3", "t1", "t4"]);
}

#[test]
fn a_filter_reads_the_row_first_and_the_fold_behind_it() {
    // `fold.followups[task]` is spelled so; in a Query the row's `id` is the key into the outer fold.
    let spend = m(&[("spend", m(&[("t1", usd(0.9)), ("t2", usd(0.1)), ("t3", usd(0.6))]))]);
    let outer = Scope::new().with("fold", spend);
    let q = Query::parse("tasks", Some("fold.spend[id] >= $0.5"), Some("-fold.spend[id]"), None).unwrap();
    assert_eq!(ids(&q.apply(tasks(), &outer)), ["t1", "t3"]);
    // `row` is the whole row
    let q = Query::parse("tasks", Some("row.doer == 'claude' and row.state != 'done'"), None, None).unwrap();
    assert_eq!(ids(&q.apply(tasks(), &outer)), ["t1"]);
}

#[test]
fn the_shell_events_that_failed() {
    // "a persisted `Shell` event with `exit: 3`" - `Log(kind glob)`, then a filter on `data`.
    let ev = |seq: f64, kind: &str, exit: Option<f64>| {
        let data = exit.map_or_else(|| m(&[]), |x| m(&[("cmd", s("sh -c 'exit 3'")), ("exit", n(x))]));
        m(&[("seq", n(seq)), ("kind", s(kind)), ("data", data)])
    };
    let q = Query::parse("log:run.shell", Some("data.exit != 0"), Some("-seq"), None).unwrap();
    let Coll::Log(glob) = &q.from else { panic!("a log") };
    let log = vec![
        ev(1.0, "run.shell", Some(0.0)),
        ev(2.0, "run.message", None),
        ev(3.0, "run.shell", Some(3.0)),
        ev(4.0, "run.shell", Some(1.0)),
        ev(5.0, "run.shell", None),
    ];
    // the resolver picks the kinds the glob names; the query keeps the rows that failed
    let named: Vec<Value> = log
        .into_iter()
        .filter(|e| matches!(e.at(&[Key::Name("kind".into())]), Value::Str(k) if glob_match(glob, &k)))
        .collect();
    let seqs: Vec<Value> = q.apply(named, &Scope::new()).iter().map(|e| e.at(&[Key::Name("seq".into())])).collect();
    assert_eq!(seqs, [n(4.0), n(3.0)], "an event with no exit code is not a failed one");
}

#[test]
fn the_connectors_page_lists_the_agents() {
    // `bind = "view:connectors?is=agent"`, written as the Query it stands for.
    let connectors = vec![
        m(&[("id", s("claude")), ("is", s("agent")), ("roles", l(vec![s("plan"), s("work")]))]),
        m(&[("id", s("ripgrep")), ("is", s("tool"))]),
        m(&[("id", s("pi")), ("is", s("agent")), ("roles", l(vec![s("work")]))]),
        m(&[("id", s("newagent")), ("is", s("agent")), ("roles", l(vec![s("review")]))]),
    ];
    let q = Query::parse("table:connector", Some("is == 'agent'"), Some("id"), None).unwrap();
    assert_eq!(ids(&q.apply(connectors.clone(), &Scope::new())), ["claude", "newagent", "pi"]);
    // `ref:doer` with `filter` and `show = "{id}"`: the picker lists who can work
    let pick = Query::parse("table:connector", Some("'work' in roles"), Some("id"), None).unwrap();
    let shown: Vec<String> = pick
        .apply(connectors, &Scope::new())
        .iter()
        .map(|row| {
            let Value::Map(fields) = row else { panic!("a map") };
            let scope = fields.iter().fold(Scope::new(), |sc, (k, v)| sc.with(k, v.clone()));
            Fmt::parse("{id}").unwrap().render(&scope)
        })
        .collect();
    assert_eq!(shown, ["claude", "pi"]);
}

#[test]
fn a_node_shows_when_something_is_selected() {
    // `{ field = "card", bind = "log:selected", when = "selected" }`: a path alone.
    let when = Expr::parse("selected").unwrap();
    assert!(when.check(Some(&["selected"])).is_empty());
    assert!(when.holds(&Scope::new().with("selected", s("c-1"))));
    assert!(!when.holds(&Scope::new().with("selected", s(""))));
    assert!(!when.holds(&Scope::new()));
}

#[test]
fn kind_globs_of_rules_folds_and_logs() {
    // `on (kind glob)`, `on = "<event kind glob>"`, `Log(kind glob)`
    for (glob, kind, hit) in [
        ("task.settled", "task.settled", true),
        ("task.*", "task.settled", true),
        ("task.*", "run.message", false),
        ("run.*", "run", false),
        ("*", "task.settled", true),
        ("*.settled", "task.settled", true),
        ("run.t?ol", "run.tool", true),
    ] {
        assert_eq!(glob_match(glob, kind), hit, "`{glob}` over `{kind}`");
    }
}

// ── the expr field of the TUI (and the Value-fields board) ────────────────

#[test]
fn the_expr_field_counts_the_last_commands_it_holds_for() {
    // "multi-line + live check 󰄬 3 of 40": the board shows `when danger(cmd).class == 'push'`, "3 of 40 last cmds".
    let cmds: Vec<String> =
        (0..40).map(|i| if i % 14 == 0 { format!("git push origin b{i}") } else { format!("ls dir{i}") }).collect();
    let e = Expr::parse("danger(cmd).class == 'push'").unwrap();
    assert!(e.check(Some(&["cmd"])).is_empty());
    let held = cmds.iter().filter(|c| e.holds(&w_gate(c))).count();
    assert_eq!((held, cmds.len()), (3, 40));
}

#[test]
fn the_expr_field_names_what_is_wrong_where_it_stands() {
    // the board's error row: `dangr(` with the caret under the name, "no fn dangr"
    let err = Expr::parse("dangr(").unwrap_err();
    assert_eq!(err.at, 0, "the caret goes under `dangr`");
    assert!(err.msg.starts_with("no function `dangr`") && err.msg.contains("danger"), "{err}");
    // and a name no place offers (the argv board: "no arg \"pt\"")
    let hole = Fmt::parse("rg -n {pt}").unwrap();
    let seen = hole.check(Some(&["pat"]));
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert!(seen[0].contains("`pt`") && seen[0].contains("pat"), "{seen:?}");
}

// ── what the docs write and the language cannot express (each pins today's answer) ──────────

#[test]
fn in_is_the_root_of_a_flows_inputs_where_a_value_is_due() {
    // `in = { goal = "text" }` ... `goal = "{in.goal}"`: `in` is also an operator word, but not where a value is due.
    let f = Fmt::parse("{in.goal}").unwrap();
    let world = Scope::new().with("in", m(&[("goal", s("ship it"))]));
    assert_eq!(f.render(&world), "ship it");
    assert_eq!(Expr::parse("in.goal").unwrap().eval(&world), s("ship it"));
    assert!(Expr::parse("x in in.goals").is_ok());
    assert!(Expr::parse("in").is_err(), "alone it is still the operator");
    assert!(Fmt::parse("{input.goal}").is_ok());
    assert!(Expr::parse("steps.in.goal").is_ok());
}

#[test]
fn gap_a_secret_reference_in_a_template_reads_as_a_formatter() {
    // `env = { CLAUDE_CODE_OAUTH_TOKEN = "{secret:claude-main}" }`; the Value-fields board: `TOKEN = {secret:x}`.
    let err = Fmt::parse("{secret:claude-main}").unwrap_err();
    assert!(err.msg.contains("no formatter `claude-main`"), "{err}");
    // worse: a secret named like a formatter parses, and means the formatter
    assert!(matches!(Fmt::parse("{secret:n}").unwrap().pieces(), [Piece::Ref { format: Some(Format::N), .. }]));
    assert!(matches!(Fmt::parse("{secret:text}").unwrap().pieces(), [Piece::Ref { format: Some(Format::Text), .. }]));
}

#[test]
fn gap_percent_is_not_a_literal() {
    // "compaction at a boundary when context > 70 %"
    assert!(Expr::parse("context > 70 %").unwrap_err().msg.contains("unexpected `%`"));
    assert!(Expr::parse("context > 70%").is_err());
    assert!(Expr::parse("context > 0.7").is_ok());
}

#[test]
fn gap_matches_is_an_operator_not_a_call() {
    // "`== != < <= > >= and or not in matches(glob)`"
    assert!(Expr::parse("matches(event.data.cmd, 'git*')").is_err());
    assert!(Expr::parse("event.data.cmd matches 'git*'").is_ok());
}

#[test]
fn gap_task_is_a_map_in_one_line_and_an_id_in_two_others() {
    // `task.state` (a map); `fold.followups[task]` and `key = "{task}"` (an id).
    let as_map = w_rule(3.0, 1.0).with("task", m(&[("id", s("t1")), ("state", s("running"))]));
    let rule = Expr::parse("ev.todo_left > 0 and fold.followups[task] < set.loop.followups").unwrap();
    assert!(!rule.holds(&as_map), "a map is no key: the rule is quietly never true");
    assert_eq!(Fmt::parse("{task}").unwrap().render(&as_map), "id=t1, state=running");
    let as_id = w_rule(3.0, 1.0);
    assert!(rule.holds(&as_id));
    assert_eq!(Expr::parse("task.state").unwrap().eval(&as_id), Value::Missing);
}

#[test]
fn gap_the_event_is_named_event_in_one_line_and_ev_in_another() {
    // `event.data.cmd` and "Expr over event + folds" against `ev.todo_left`; the envelope
    // puts `todo_left` under `data`. Whoever evaluates fixes the root, so the doc has to say which.
    let e = Expr::parse("ev.todo_left > 0").unwrap();
    assert_eq!(e.check(Some(&["event", "fold", "set", "task"])).len(), 1);
}

#[test]
fn gap_a_pointer_takes_the_dot_name_that_follows_it() {
    // expr.rs grammar: `post := … | pointer ("." name | "[" expr "]")*`. `.` is also a pointer character, so only `[` can
    // continue a pointer: `/a.b` is the key "a.b".
    let doc = m(&[("a", m(&[("b", n(1.0))])), ("a.b", n(7.0))]);
    assert_eq!(Expr::parse("/a.b").unwrap().eval(&on(doc.clone())), n(7.0));
    assert_eq!(Expr::parse("/a['b']").unwrap().eval(&on(doc)), n(1.0));
    // after an index the next step is `.name`, not `/name`
    assert!(Expr::parse("/content[0]/text").is_err());
    assert!(Expr::parse("/content[0].text").is_ok());
}

#[test]
fn gap_a_number_named_ms_does_not_compare_with_a_duration() {
    // "stall (`quiet_ms`)" and "stall Rule on `quiet_ms` fold": a field named for milliseconds holds a number
    let quiet = Scope::new().with("fold", m(&[("quiet_ms", n(400_000.0))]));
    assert!(!Expr::parse("fold.quiet_ms > 5m").unwrap().holds(&quiet), "a plain number is not a duration (expr.rs)");
    assert!(Expr::parse("fold.quiet_ms > 300000").unwrap().holds(&quiet));
    // the same field typed as a duration compares
    let typed = Scope::new().with("fold", m(&[("quiet_ms", Value::Dur(400_000))]));
    assert!(Expr::parse("fold.quiet_ms > 5m").unwrap().holds(&typed));
}

#[test]
fn gap_a_bare_word_after_a_comparison_is_a_name_and_nothing_says_so() {
    // a `when = "rig.standing != done"` reads here as a comparison with a name `done`.
    let e = Expr::parse("rig.standing != done").unwrap();
    let running = Scope::new().with("rig", m(&[("standing", s("running"))]));
    assert!(!e.holds(&running), "never true, whatever the standing is");
    assert!(e.check(None).is_empty());
    assert_eq!(e.check(Some(&["rig"])).len(), 1, "only a list of names catches it");
    // a Query has no names to give (a name may be a row's field), so the same slip in a filter passes
    let q = Query::parse("tasks", Some("state == done"), None, None).unwrap();
    assert!(q.check(None).is_empty());
    assert!(q.apply(tasks(), &Scope::new()).is_empty());
}

#[test]
fn gap_a_name_with_a_hyphen_is_written_as_an_index() {
    // a path `rig.<rig-id>.<field>` with slug ids; J4n ids such as `claude-main`, `plan-work-validate`, `b-claude`.
    assert!(Expr::parse("rig.contest-run.spent_usd").is_err());
    let rigs = Scope::new().with("rig", m(&[("contest-run", m(&[("spent_usd", usd(0.5))]))]));
    assert_eq!(Expr::parse("rig['contest-run'].spent_usd").unwrap().eval(&rigs), usd(0.5));
}

#[test]
fn gap_the_boards_show_what_the_closed_formatters_do_not_say() {
    // (template, world, what the formatter gives, what the board shows)
    type Case = (&'static str, fn() -> Scope, &'static str, &'static str);
    let cases: [Case; 8] = [
        (
            "{x:dur}",
            || Scope::new().with("x", Value::Dur(64_000)),
            "1m04s",
            "1:04 (the Main and Lanes boards)",
        ),
        ("{x:dur}", || Scope::new().with("x", Value::Dur(9_200)), "9s", "9.2s (Log board, `3.1s`)"),
        ("{x:dur}", || Scope::new().with("x", Value::Dur(400)), "400ms", "0.4s (the route row)"),
        (
            "{x:pct}",
            || Scope::new().with("x", n(1.04)),
            "104%",
            "104% (FieldsDisplay board; \"error >100 %\")",
        ),
        ("{x:usd}", || Scope::new().with("x", usd(0.004)), "$0.0040", "$0.004 (FieldsDisplay table, `$/run`)"),
        ("{x:usd}", || Scope::new().with("x", usd(0.031)), "$0.03", "$0.031 (FieldsDisplay table, `$/run`)"),
        ("{x:usd}", || Scope::new().with("x", usd(0.21)), "$0.21", "$0.210 (FieldsDisplay table, `$/run`)"),
        ("{x:n}", || Scope::new().with("x", n(1204.0)), "1204", "1,204 (FieldsDisplay table footer)"),
    ];
    for (template, world, formatter, board) in cases {
        let shown = Fmt::parse(template).unwrap().render(&world());
        assert_eq!(shown, formatter, "{template}: the board shows {board}");
    }
}

#[test]
fn gap_a_bind_is_not_a_collection() {
    // the screen's binds, "resolved by the core with Expr/Query", against the seven collections of the query
    for bind in [
        "asks:open",
        "tasks:state=running",
        "task:t1",
        "view:connectors?is=agent",
        "table:connector/{selected}",
        "intent:probe",
    ] {
        assert!(Coll::parse(bind).is_err(), "`{bind}`");
    }
    // while these happen to read as collections
    assert_eq!(Coll::parse("fold:spend.today").unwrap(), Coll::Fold("spend.today".into()));
    assert_eq!(Coll::parse("log:selected").unwrap(), Coll::Log("selected".into()));
}

#[test]
fn a_label_template_reads_a_query_row_the_way_the_filter_does() {
    // `row = { label = Fmt, … }` over the rows of `from`: `RowEnv` is what the filter reads, and a label renders over it.
    let row = m(&[("id", s("t1")), ("state", s("running"))]);
    let q = Query::parse("tasks", Some("row.id == 't1' and state == 'running'"), None, None).unwrap();
    assert_eq!(q.apply(vec![row.clone()], &Scope::new()).len(), 1);
    let outer = Scope::new().with("fold", m(&[("spend", Value::usd(0.5))]));
    let env = RowEnv::new(&row, &outer);
    assert_eq!(Fmt::parse("{row.id} {state} {fold.spend:usd} {/id}").unwrap().render(&env), "t1 running $0.50 t1");
}

#[test]
fn short_is_one_line_of_at_most_forty_characters_whatever_the_kind() {
    // fmt.rs `Format::Short`: "one line, at most SHORT_MAX characters (`…` marks a cut)". Lists and maps go by `plain`.
    let files: Vec<Value> = (0..30).map(|i| s(&format!("file_{i}.rs"))).collect();
    for (what, v) in [
        ("a long list", l(files)),
        ("a list with a line break in an item", l(vec![s("a\nb"), s("c")])),
        ("a map with a line break in a value", m(&[("k", s("x\ny"))])),
    ] {
        let shown = format_value(&v, Some(Format::Short));
        assert!(!shown.contains('\n') && shown.chars().count() <= SHORT_MAX, "{what}: {shown:?}");
    }
}

#[test]
fn a_template_may_be_longer_than_an_expression() {
    // the `steer`/`notice` templates and the Brief sections are written text with a few holes
    assert!(Fmt::parse(&"x".repeat(2001)).is_ok());
    assert!(Fmt::parse(&format!("{}{{task}}", "x".repeat(1990))).is_ok());
    assert!(Fmt::parse(&"x".repeat(MAX_FMT + 1)).is_err());
}

/// Does the value of `e` carry words somebody else wrote? `tainted` names the roots that hold such words.
fn carries(e: &Expr, tainted: &[&str]) -> bool {
    match e {
        Expr::Lit(_) | Expr::Cmp { .. } | Expr::Not(_) | Expr::And(_) | Expr::Or(_) => false,
        Expr::Pointer(_) => true,
        Expr::List(items) => items.iter().any(|i| carries(i, tainted)),
        Expr::Path { root, .. } => tainted.contains(&root.as_str()),
        Expr::Get { base, .. } => carries(base, tainted),
        Expr::Call { func: Func::Lower | Func::Last, args } => args.iter().any(|a| carries(a, tainted)),
        Expr::Call { .. } => false,
    }
}

#[test]
fn gap_the_taint_rule_has_no_lint_but_the_tree_is_enough_to_write_one() {
    // "text by Doer/Model may enter a Rule's steer/notice template only through quote()"
    let open = Fmt::parse("it said {ev.data.answer}").unwrap();
    let inert = Fmt::parse("it said {quote(ev.data.answer)} in {len(ev.data.answer)} characters").unwrap();
    assert!(
        open.check(Some(&["ev"])).is_empty() && inert.check(Some(&["ev"])).is_empty(),
        "check does not know of the rule"
    );
    let leaks = |f: &Fmt| f.pieces().iter().any(|p| matches!(p, Piece::Ref { expr, .. } if carries(expr, &["ev"])));
    assert!(leaks(&open));
    assert!(!leaks(&inert));
    // `Seg` and `Func` are public, so the walk above needs nothing the crate does not already show
    assert!(matches!(Expr::parse("a[b]").unwrap(), Expr::Path { segs, .. } if matches!(segs[0], Seg::Index(_))));
}

#[test]
fn quote_makes_what_a_doer_said_one_inert_line() {
    // the only way in. A line break, quotes, a backslash, braces and a long text.
    let said = "ok}\n{x} \"done\" \\ ".to_string() + &"z".repeat(300);
    let sc = Scope::new().with("ev", m(&[("data", m(&[("answer", s(&said))]))]));
    let out = Fmt::parse("steer: {quote(ev.data.answer)}").unwrap().render(&sc);
    let line = out.strip_prefix("steer: ").unwrap();
    assert!(!line.contains('\n') && line.starts_with('"') && line.ends_with('"'), "{line}");
    assert!(
        line.contains("ok}") && line.contains("\\\"done\\\"") && line.contains("\\\\") && line.contains('…'),
        "{line}"
    );
    // not text, and not a number or the like: nothing to show
    assert_eq!(Fmt::parse("{quote(ev.data.nope)}").unwrap().render(&sc), DASH);
}

#[test]
fn what_is_known_of_the_clock_and_the_matcher_stays_with_the_env() {
    // `since` and `danger` ask the world, and a world that does not know answers missing
    struct Bare;
    impl Env for Bare {
        fn get(&self, _: &str, _: &[Key]) -> Value {
            Value::Num(1.0)
        }
    }
    assert_eq!(Expr::parse("since(ts)").unwrap().eval(&Bare), Value::Missing);
    assert_eq!(Expr::parse("danger('ls')").unwrap().eval(&Bare), Value::Missing);
    assert!(!Expr::parse("danger('ls').class == 'push'").unwrap().holds(&Bare));
}

// ── the shape, qc and trust expressions ────────────────────────────────────────

#[test]
fn the_expressions_of_shape_qc_and_trust() {
    let roots: &[&str] = &["set", "ev", "fold", "task", "steps"];
    for (at, src) in [
        ("§2.5 has-judge", "set.shape.judge != 'off'"),
        ("§2.5 flow end", "steps.check.out.verdict.pass and steps.judge.out.verdict.pass"),
        ("§3.2 qc-live", "set.qc == 'on' and set.qc.live != 'off' and ev.family in ['edit', 'write']"),
        ("§3.2 check row", "set.qc == 'on'"),
        (
            "§4.2 steer",
            "set.qc.live == 'steer' and ev.severity == 'high' and fold.label_rate['qc_mark', ev.pattern].p >= set.qc.steer_precision and fold.label_rate['qc_mark', ev.pattern].n >= 40 and fold.qc_steers[ev.task] < set.qc.rounds",
        ),
        ("§5 model answers", "ev.by_kind == 'model' and ev.impact == 'mid'"),
    ] {
        let e = Expr::parse(src).unwrap_or_else(|e| panic!("{at}: {e}"));
        assert!(e.check(Some(roots)).is_empty(), "{at}: {:?}", e.check(Some(roots)));
        assert_eq!(Expr::parse(&e.to_string()).as_ref(), Ok(&e), "{at}: prints back");
    }
    let steer = Expr::parse("set.qc.live == 'steer' and ev.severity == 'high' and fold.label_rate['qc_mark', ev.pattern].p >= set.qc.steer_precision and fold.label_rate['qc_mark', ev.pattern].n >= 40 and fold.qc_steers[ev.task] < set.qc.rounds").unwrap();
    let world = |evidence: f64, steers: f64| {
        Scope::new()
            .with("set", m(&[("qc", m(&[("live", s("steer")), ("steer_precision", n(0.8)), ("rounds", n(3.0))]))]))
            .with("ev", m(&[("severity", s("high")), ("pattern", s("fallback")), ("task", s("t1"))]))
            .with(
                "fold",
                m(&[
                    ("label_rate", m(&[("qc_mark", m(&[("fallback", m(&[("p", n(0.9)), ("n", n(evidence))]))]))])),
                    ("qc_steers", m(&[("t1", n(steers))])),
                ]),
            )
    };
    assert!(steer.holds(&world(50.0, 1.0)), "a precise pattern with evidence steers");
    assert!(!steer.holds(&world(39.0, 1.0)), "too little evidence");
    assert!(!steer.holds(&world(50.0, 3.0)), "the rounds are used up");
    // the flow template of §2.5 and the done-when line of §2.7
    let model = Fmt::parse("model:{set.shape.model}").unwrap();
    assert_eq!(model.render(&Scope::new().with("set", m(&[("shape", m(&[("model", s("haiku"))]))]))), "model:haiku");
    assert_eq!(
        Fmt::parse("- Done when {done.cmd} passes")
            .unwrap()
            .render(&Scope::new().with("done", m(&[("cmd", s("cargo test"))]))),
        "- Done when cargo test passes"
    );
}
