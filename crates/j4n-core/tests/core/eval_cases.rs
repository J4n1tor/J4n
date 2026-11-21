//! Evaluation semantics of Expr (`Expr::parse(..).eval(&Scope)` and `holds`), written from the language reference in
//! `src/expr.rs` and the doc comments of `value.rs`, `eval.rs`, `fmt.rs`, `query.rs` only.
//!
//! One test per theme; each theme is a table of rows, every failing row is collected and the theme asserts once at
//! the end, so one run shows every failing row. A row the contract leaves open is written with the `*_amb` helpers
//! (its message starts with `[DECIDED]`) and carries the most natural reading.

use j4n_core::eval::{Env, Scope};
use j4n_core::expr::{Expr, QUOTE_MAX};
use j4n_core::value::{Key, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::mpsc;
use std::time::Duration;

// ── value shorthands ─────────────────────────────────────────────────────────────────────────

fn t() -> Value {
    Value::Bool(true)
}
fn f() -> Value {
    Value::Bool(false)
}
fn x() -> Value {
    Value::Missing
}
fn n(v: f64) -> Value {
    Value::Num(v)
}
fn s(v: &str) -> Value {
    Value::Str(v.to_string())
}
fn d(ms: u64) -> Value {
    Value::Dur(ms)
}
fn u(micros: i64) -> Value {
    Value::Usd(micros)
}
fn l(v: Vec<Value>) -> Value {
    Value::List(v)
}
fn m(pairs: &[(&str, Value)]) -> Value {
    Value::Map(pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect::<BTreeMap<_, _>>())
}

// ── the rows machinery ───────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct Rows {
    n: usize,
    bad: Vec<String>,
}

fn short(v: &Value) -> String {
    let s = format!("{v:?}");
    let len = s.chars().count();
    if len > 220 {
        let head: String = s.chars().take(110).collect();
        let tail: String = s.chars().skip(len - 70).collect();
        format!("{head} …[{len} chars]… {tail}")
    } else {
        s
    }
}

/// Evaluate `src` over `env`; a refusal to parse or a panic is an `Err` with the words for the report.
fn run(env: &dyn Env, src: &str) -> Result<Value, String> {
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Expr::parse(src).map(|ex| ex.eval(env))));
    match res {
        Err(_) => Err("PANICKED".to_string()),
        Ok(Err(e)) => Err(format!("does not parse: {e}")),
        Ok(Ok(v)) => Ok(v),
    }
}

fn run_holds(env: &dyn Env, src: &str) -> Result<bool, String> {
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Expr::parse(src).map(|ex| ex.holds(env))));
    match res {
        Err(_) => Err("PANICKED".to_string()),
        Ok(Err(e)) => Err(format!("does not parse: {e}")),
        Ok(Ok(v)) => Ok(v),
    }
}

fn tagged(tag: &str) -> String {
    if tag.is_empty() { String::new() } else { format!("[{tag}] ") }
}

impl Rows {
    fn val_t(&mut self, tag: &str, env: &dyn Env, src: &str, want: Value) {
        self.n += 1;
        let tag = tagged(tag);
        match run(env, src) {
            Err(e) => self.bad.push(format!("{tag}{src}\n      {e}\n      want {}", short(&want))),
            Ok(got) if got != want => {
                self.bad.push(format!("{tag}{src}\n      want {}\n      got  {}", short(&want), short(&got)));
            }
            Ok(_) => {}
        }
    }

    /// `src` evaluates to exactly `want`.
    fn val(&mut self, env: &dyn Env, src: &str, want: Value) {
        self.val_t("", env, src, want);
    }

    /// The same, for a row whose answer the contract leaves open.
    fn amb(&mut self, env: &dyn Env, src: &str, want: Value) {
        self.val_t("DECIDED", env, src, want);
    }

    /// `src` evaluates to the bool `want`.
    fn yes(&mut self, env: &dyn Env, src: &str, want: bool) {
        self.val_t("", env, src, Value::Bool(want));
    }

    fn yes_amb(&mut self, env: &dyn Env, src: &str, want: bool) {
        self.val_t("DECIDED", env, src, Value::Bool(want));
    }

    fn hold_t(&mut self, tag: &str, env: &dyn Env, src: &str, want: bool) {
        self.n += 1;
        let tag = tagged(tag);
        match run_holds(env, src) {
            Err(e) => self.bad.push(format!("{tag}holds({src})\n      {e}\n      want {want}")),
            Ok(got) if got != want => self.bad.push(format!("{tag}holds({src})\n      want {want}\n      got  {got}")),
            Ok(_) => {}
        }
    }

    /// `src` holds (or does not).
    fn hold(&mut self, env: &dyn Env, src: &str, want: bool) {
        self.hold_t("", env, src, want);
    }

    fn prop_t(&mut self, tag: &str, env: &dyn Env, src: &str, what: &str, ok: impl Fn(&Value) -> bool) {
        self.n += 1;
        let tag = tagged(tag);
        match run(env, src) {
            Err(e) => self.bad.push(format!("{tag}{src}\n      {e}\n      want {what}")),
            Ok(got) if !ok(&got) => {
                self.bad.push(format!("{tag}{src}\n      want {what}\n      got  {}", short(&got)));
            }
            Ok(_) => {}
        }
    }

    /// `src` evaluates to something with the property `what`.
    fn prop(&mut self, env: &dyn Env, src: &str, what: &str, ok: impl Fn(&Value) -> bool) {
        self.prop_t("", env, src, what, ok);
    }

    fn prop_amb(&mut self, env: &dyn Env, src: &str, what: &str, ok: impl Fn(&Value) -> bool) {
        self.prop_t("DECIDED", env, src, what, ok);
    }

    /// `src` answers, whatever it answers (nothing panics, nothing fails to parse).
    fn answers(&mut self, env: &dyn Env, src: &str) {
        self.prop_t("", env, src, "an answer", |_| true);
    }

    fn done(self, theme: &str) {
        assert!(self.bad.is_empty(), "{theme}: {} of {} rows fail:\n{}", self.bad.len(), self.n, self.bad.join("\n"));
    }
}

// ── the world the rows read ──────────────────────────────────────────────────────────────────

fn doc() -> Value {
    m(&[
        ("type", s("text")),
        ("usage", m(&[("cost_usd", n(0.5)), ("n", n(3.0))])),
        ("content", l(vec![m(&[("type", s("text")), ("text", s("hello"))]), m(&[("type", s("image"))])])),
        ("a/b", n(1.0)),
        ("m~n", n(2.0)),
        ("x~1y", n(3.0)),
        ("", n(4.0)),
        ("meta.v", n(7.0)),
        ("meta", m(&[("v", n(8.0))])),
        ("a-b", n(9.0)),
        ("a:b", n(10.0)),
        ("a@b", n(11.0)),
        ("a$b", n(12.0)),
        ("a%b", n(13.0)),
        ("a+b", n(14.0)),
        ("k_1", n(15.0)),
        ("a b", n(16.0)),
        ("a%20b", n(17.0)),
        ("nums", l(vec![n(1.0), n(2.0), n(3.0)])),
        ("grid", l(vec![l(vec![n(1.0), n(2.0)]), l(vec![n(3.0), n(4.0)])])),
        ("obj", m(&[("0", s("zero-key"))])),
        ("flag", t()),
        ("dur", d(90_000)),
        ("cost", u(1_500_000)),
    ])
}

/// The matcher `danger` asks: the class of a push or of a delete, `true` for sudo, nothing for the rest.
fn gate(cmd: &str) -> Value {
    let low = cmd.to_lowercase();
    if low.contains("git push") {
        m(&[("class", s("push")), ("echo", s(cmd))])
    } else if low.contains("rm -rf") {
        m(&[("class", s("destroy")), ("echo", s(cmd))])
    } else if low.contains("sudo") {
        t()
    } else {
        x()
    }
}

fn world() -> Scope {
    Scope::new()
        .with(
            "task",
            m(&[
                ("id", s("t1")),
                ("n", n(1.0)),
                ("name", s("Plant")),
                ("key", s("a")),
                ("cost", u(1_500_000)),
                ("took", d(90_000)),
                ("flag", t()),
                ("off", f()),
                ("zero", n(0.0)),
                ("blank", s("")),
                ("frac", n(1.5)),
                ("neg", n(-1.0)),
                ("started", n(1000.0)),
                ("cmd", s("git push origin main")),
            ]),
        )
        .with("list", l(vec![n(10.0), n(20.0), n(30.0)]))
        .with("nums", l(vec![n(1.0), n(2.0), n(3.0)]))
        .with("ones", l(vec![n(1.0), n(1.0), n(1.0)]))
        .with("strs", l(vec![s("a"), s("b")]))
        .with("dups", l(vec![s("a"), s("b"), s("a")]))
        .with("holes", l(vec![n(1.0), x(), n(3.0)]))
        .with("onlymissing", l(vec![x()]))
        .with("tailmissing", l(vec![n(1.0), x()]))
        .with("emptylist", l(vec![]))
        .with("emptymap", m(&[]))
        .with("mix", l(vec![n(1.0), s("1"), u(1_000_000), n(1.0), t()]))
        .with("durs", l(vec![d(60_000), d(30_000)]))
        .with("costs", l(vec![u(1_500_000), u(400)]))
        .with("costsh", l(vec![u(2_500_000), x(), u(500_000)]))
        .with("numhole", l(vec![n(1.0), x(), n(2.0)]))
        .with("tenth", l(vec![u(100_000), u(200_000)]))
        .with("mixnu", l(vec![n(1.0), u(1_000_000)]))
        .with("mixnd", l(vec![n(1.0), d(1000)]))
        .with("mixdu", l(vec![d(1000), u(1_000_000)]))
        .with("txts", l(vec![s("a"), s("b")]))
        .with("bools", l(vec![t(), t()]))
        .with("lists", l(vec![l(vec![n(1.0)]), l(vec![n(2.0)])]))
        .with("events", l(vec![m(&[("kind", s("a")), ("n", n(1.0))]), m(&[("kind", s("b")), ("n", n(2.0))])]))
        .with(
            "m",
            m(&[
                ("a", m(&[("b", n(5.0))])),
                ("k", s("v")),
                ("1", s("one")),
                ("x y", s("sp")),
                ("", s("empty-key")),
                ("lst", l(vec![m(&[("name", s("p"))]), m(&[("name", s("q"))])])),
            ]),
        )
        .with("mapa", m(&[("x", n(1.0)), ("y", n(2.0))]))
        .with("mapb", m(&[("y", n(2.0)), ("x", n(1.0))]))
        .with("mapc", m(&[("x", n(1.0))]))
        .with("mapd", m(&[("x", n(1.0)), ("y", n(3.0))]))
        .with("mape", m(&[("x", n(1.0)), ("z", n(2.0))]))
        .with("nested1", m(&[("l", l(vec![n(1.0), n(2.0)]))]))
        .with("nested2", m(&[("l", l(vec![n(1.0), n(2.0)]))]))
        .with("nested3", m(&[("l", l(vec![n(1.0), n(3.0)]))]))
        .with("zero", n(0.0))
        .with("mapmiss", m(&[("k", x())]))
        .with("maxusd", u(i64::MAX))
        .with("minusd", u(i64::MIN))
        .with("maxdur", d(u64::MAX))
        .with("bigdur", l(vec![d(u64::MAX), d(1)]))
        .with("bigdur2", l(vec![d(u64::MAX), d(u64::MAX)]))
        .with("bigusd", l(vec![u(i64::MAX), u(1)]))
        .with("bigusd2", l(vec![u(i64::MAX), u(i64::MAX)]))
        .with("bigneg", l(vec![u(i64::MIN), u(-1)]))
        .with("event", m(&[("data", m(&[("cmd", s("git push origin main"))]))]))
        // text fixtures for quote
        .with("dq", s("say \"x\""))
        .with("bs", s("a\\b"))
        .with("txt200", s(&"a".repeat(200)))
        .with("txt201", s(&"a".repeat(201)))
        .with("longtxt", s(&"a".repeat(300)))
        .with("quoty", s(&"\"".repeat(150)))
        .with("edge", s(&format!("{}\"b", "a".repeat(199))))
        .with("edgeb", s(&format!("{}\\b", "a".repeat(199))))
        .with("e200", s(&"é".repeat(200)))
        .with("e201", s(&"é".repeat(201)))
        .with("ctrl", s("a\nb\r\tc\u{0}d\u{1b}e\u{7f}f"))
        .with("crlf", s("a\r\nb"))
        .with("uni", s("héllo wörld"))
        .doc(doc())
        .at_ms(100_000)
        .danger_with(gate)
}

/// What the world has when there is no clock, no matcher and no document.
fn bare() -> Scope {
    Scope::new().with("task", m(&[("started", n(1000.0)), ("cmd", s("git push"))]))
}

// ── 1. literals ──────────────────────────────────────────────────────────────────────────────

fn rows_literals() -> Rows {
    let mut r = Rows::default();
    let sc = world();
    let table: Vec<(&str, Value)> = vec![
        // text and its escapes, both quote styles
        ("'hello'", s("hello")),
        (r#""hello""#, s("hello")),
        ("''", s("")),
        (r#""""#, s("")),
        (r"'it\'s'", s("it's")),
        (r#""it's""#, s("it's")),
        (r#"'say "hi"'"#, s("say \"hi\"")),
        (r#""say \"hi\"""#, s("say \"hi\"")),
        (r"'a\\b'", s("a\\b")),
        (r"'a\nb'", s("a\nb")),
        (r"'a\tb'", s("a\tb")),
        (r"'a\rb'", s("a\rb")),
        (r#"'\"'"#, s("\"")),
        (r#""\'""#, s("'")),
        (r"'\\n'", s("\\n")), // a backslash and an n, not a newline
        (r"'\\'", s("\\")),
        ("'héllo ✓'", s("héllo ✓")),
        ("'  spaces  '", s("  spaces  ")),
        ("'1'", s("1")),
        ("'true'", s("true")), // text, not a bool
        // numbers: whole, decimal, negative
        ("12", n(12.0)),
        ("0", n(0.0)),
        ("-3", n(-3.0)),
        ("0.5", n(0.5)),
        ("-0.5", n(-0.5)),
        ("1.50", n(1.5)),
        ("100", n(100.0)),
        ("3.25", n(3.25)),
        // bools
        ("true", t()),
        ("false", f()),
        // durations: every unit, decimals, combinations
        ("250ms", d(250)),
        ("90s", d(90_000)),
        ("1s", d(1000)),
        ("5m", d(300_000)),
        ("30m", d(1_800_000)),
        ("2h", d(7_200_000)),
        ("2d", d(172_800_000)),
        ("0s", d(0)),
        ("0ms", d(0)),
        ("1.5h", d(5_400_000)),
        ("1.5m", d(90_000)),
        ("0.5s", d(500)),
        ("0.25s", d(250)),
        ("1.5d", d(129_600_000)),
        ("1h30m", d(5_400_000)),
        ("1m30s", d(90_000)),
        ("1d12h", d(129_600_000)),
        ("1h1s", d(3_601_000)),
        ("1.5h30m", d(7_200_000)),
        ("1d2h3m4s5ms", d(86_400_000 + 7_200_000 + 180_000 + 4_000 + 5)),
        // money, kept to a millionth
        ("$1.50", u(1_500_000)),
        ("$0.0004", u(400)),
        ("$0", u(0)),
        ("$5", u(5_000_000)),
        ("$0.000001", u(1)),
        ("$12.345678", u(12_345_678)),
        ("$0.1", u(100_000)),
        ("$0.50", u(500_000)),
        ("$100", u(100_000_000)),
        // lists
        ("[]", l(vec![])),
        ("[1, 'a', true]", l(vec![n(1.0), s("a"), t()])),
        ("[1, [2, 3]]", l(vec![n(1.0), l(vec![n(2.0), n(3.0)])])),
        ("[task.n, task.id]", l(vec![n(1.0), s("t1")])),
        ("[5s, $1, 2]", l(vec![d(5000), u(1_000_000), n(2.0)])),
        ("[ 1 , 2 ]", l(vec![n(1.0), n(2.0)])),
        ("[[]]", l(vec![l(vec![])])),
        ("count([1, nothing, 3])", n(2.0)),
    ];
    for (src, want) in table {
        r.val(&sc, src, want);
    }
    // DECIDED: money beyond a millionth: "kept to a millionth" does not say round or cut.
    r.amb(&sc, "$0.0000006", u(1));
    r.amb(&sc, "$0.0000004", u(0));
    // DECIDED: does a list literal keep an item that is missing, or drop it?
    r.amb(&sc, "[1, nothing, 3]", l(vec![n(1.0), x(), n(3.0)]));
    r.amb(&sc, "len([1, nothing, 3])", n(3.0));
    r
}

#[test]
fn literals() {
    rows_literals().done("literals");
}

// ── 2. truthiness of every kind ──────────────────────────────────────────────────────────────

fn rows_truthiness() -> Rows {
    let mut r = Rows::default();
    let sc = world();
    // (expression, is it yes)
    let table: Vec<(&str, bool)> = vec![
        ("nothing", false), // a root nobody defined: missing
        ("task.nope", false),
        ("list[9]", false),
        ("true", true),
        ("false", false),
        ("0", false),
        ("0.0", false),
        ("-0", false),
        ("1", true),
        ("-1", true),
        ("0.5", true),
        ("''", false),
        ("' '", true),
        ("'0'", true),
        ("'false'", true),
        ("0s", false),
        ("0ms", false),
        ("1ms", true),
        ("1h", true),
        ("$0", false),
        ("$0.0", false),
        ("$0.000001", true),
        ("$5", true),
        ("[]", false),
        ("[0]", true),
        ("[1, 2]", true),
        ("emptylist", false),
        ("emptymap", false),
        ("holes", true),       // a list with an item missing is not an empty list
        ("onlymissing", true), // nor is a list of one missing item
        ("mapmiss", true),     // nor is a map with one key whose value is missing
        ("mapc", true),
        ("m", true),
        ("list", true),
        ("task.zero", false),
        ("task.blank", false),
        ("task.flag", true),
        ("task.off", false),
        ("task.cost", true),
        ("task.took", true),
        ("task.n", true),
        ("task.name", true),
        ("sum(emptylist)", false),
        ("count(nothing)", false),
        ("count(list)", true),
        ("last(emptylist)", false),
        ("len('')", false),
        ("len('a')", true),
        ("quote('')", true), // two double quotes: not empty
    ];
    for (e, yes) in &table {
        r.hold(&sc, e, *yes);
        r.val(&sc, &format!("not ({e})"), Value::Bool(!yes));
        r.val(&sc, &format!("({e}) and true"), Value::Bool(*yes));
        r.val(&sc, &format!("({e}) or false"), Value::Bool(*yes));
        r.val(&sc, &format!("true and ({e})"), Value::Bool(*yes));
        r.val(&sc, &format!("false or ({e})"), Value::Bool(*yes));
    }
    r
}

#[test]
fn truthiness() {
    rows_truthiness().done("truthiness");
}

// ── 3. and / or / not, precedence ────────────────────────────────────────────────────────────

fn rows_logic() -> Rows {
    let mut r = Rows::default();
    let sc = world();
    let table: Vec<(&str, bool)> = vec![
        // they answer true or false, never the operand
        ("1 and 'x'", true),
        ("1 and 0", false),
        ("0 and 1", false),
        ("0 or 0s", false),
        ("'' or $0", false),
        ("'a' or 0", true),
        ("nothing or 'x'", true),
        ("1 or nothing", true),
        ("nothing and 1", false),
        ("1 and nothing", false),
        ("[] or emptymap", false),
        ("[1] and mapc", true),
        ("true and true and true", true),
        ("true and true and false", false),
        ("false or false or true", true),
        ("false or false or false", false),
        ("not 0", true),
        ("not 'a'", false),
        ("not nothing", true),
        ("not []", true),
        ("not [0]", false),
        ("not not 0", false),
        ("not not 'a'", true),
        ("not not not 1", false),
        ("not task.flag", false),
        ("not task.nope", true),
        // precedence: not > and > or; a comparison binds tighter than not
        ("true or false and false", true),
        ("false and false or true", true),
        ("false and (false or true)", false),
        ("not false and false", false),
        ("not true or true", true),
        ("not (true or true)", false),
        ("(true or false) and false", false),
        ("not 1 == 1", false),
        ("not 1 == 2", true),
        ("not nothing == 1", true),
        ("not (nothing == 1)", true),
        ("1 == 1 and 2 == 2", true),
        ("1 == 2 or 2 == 2", true),
        ("1 < 2 and 2 < 3", true),
        ("1 < 2 and 3 < 2", false),
        ("not (1 == 1 and 2 == 3)", true),
        ("1 == 1 and not 2 == 2", false),
        ("1 == 1 and not 2 == 3", true),
        ("nothing == 1 or nothing != 1", false),
        ("not (nothing == 1) and not (nothing != 1)", true),
        ("(1 == 1) == (2 == 2)", true),
        ("(1 == 1) == true", true),
        ("(1 == 2) == false", true),
        ("(1 == 1) != (1 == 2)", true),
        ("'a' in strs and 'b' in strs", true),
        ("'a' in strs and 'c' in strs", false),
        ("'c' in strs or 'b' in strs", true),
        ("not 'c' in strs", true),
        ("not 'a' in strs", false),
    ];
    for (e, want) in &table {
        r.yes(&sc, e, *want);
        r.hold(&sc, e, *want);
    }
    r
}

#[test]
fn logic() {
    rows_logic().done("logic");
}

// ── 4. paths ─────────────────────────────────────────────────────────────────────────────────

fn rows_paths() -> Rows {
    let mut r = Rows::default();
    let sc = world();
    let table: Vec<(&str, Value)> = vec![
        // maps
        ("task.id", s("t1")),
        ("task.n", n(1.0)),
        ("task['id']", s("t1")),
        (r#"task["id"]"#, s("t1")),
        ("task.cost", u(1_500_000)),
        ("task.took", d(90_000)),
        ("event.data.cmd", s("git push origin main")),
        ("event['data']['cmd']", s("git push origin main")),
        ("event.data['cmd']", s("git push origin main")),
        ("event['data'].cmd", s("git push origin main")),
        ("m.a.b", n(5.0)),
        ("m['a']['b']", n(5.0)),
        ("m.a['b']", n(5.0)),
        ("m['a'].b", n(5.0)),
        ("m[task.key].b", n(5.0)), // the key from a text in the world
        ("m[task.key]", m(&[("b", n(5.0))])),
        ("m.k", s("v")),
        ("m['1']", s("one")),
        ("m['x y']", s("sp")),
        ("m['']", s("empty-key")),
        ("m.lst[1].name", s("q")),
        ("m.lst[0]['name']", s("p")),
        ("m.lst[task.zero].name", s("p")),
        // missing: nowhere to go
        ("nothing", x()),
        ("nothing.a", x()),
        ("nothing.a.b.c", x()),
        ("nothing[0]", x()),
        ("nothing['a'].b", x()),
        ("task.nope", x()),
        ("task.nope.deeper", x()),
        ("task.nope.deeper.deepest", x()),
        ("task.nope[0]", x()),
        ("task.nope['k']", x()),
        ("m.nope", x()),
        ("m.nope.deeper.deepest", x()),
        ("m.a.zz.b", x()),
        ("m['zz']", x()),
        // a step into something that is not a map or a list
        ("m.a.b.c", x()),
        ("m.k.x", x()),
        ("task.name.first", x()),
        ("task.n.x", x()),
        ("task.flag.x", x()),
        ("zero.x", x()),
        ("zero[0]", x()),
        ("task.name[0]", x()),
        // lists by a position
        ("list[0]", n(10.0)),
        ("list[1]", n(20.0)),
        ("list[2]", n(30.0)),
        ("list[1.0]", n(20.0)), // 1.0 is a whole number
        ("list[2.0]", n(30.0)),
        ("list[task.n]", n(20.0)),
        ("list[task.zero]", n(10.0)),
        ("list[len(strs)]", n(30.0)),
        ("list[count(strs)]", n(30.0)),
        ("events[0].kind", s("a")),
        ("events[1].n", n(2.0)),
        ("events[1]['kind']", s("b")),
        ("events[2].kind", x()),
        ("last(events).kind", s("b")),
        ("last(events)['n']", n(2.0)),
        ("list[0][0]", x()),
        // out of range, not whole, not a number
        ("list[3]", x()),
        ("list[99]", x()),
        ("list[-1]", x()),
        ("list[1.5]", x()),
        ("list[0.5]", x()),
        ("list[task.frac]", x()),
        ("list[task.neg]", x()),
        ("list[task.nope]", x()),
        ("list[true]", x()),
        ("list[1s]", x()),
        ("list[$1]", x()),
        ("list[[1]]", x()),
        ("m[true]", x()),
        ("m[5s]", x()),
        ("m[$1]", x()),
        ("list[nothing]", x()),
        ("list[list[0]]", x()),
        ("list[1000000000000]", x()),
        ("list[100000000000000000000]", x()),
        ("list[18446744073709551616]", x()),
        ("list[-100000000000000000000]", x()),
        ("list.x", x()),
        ("emptylist[0]", x()),
        ("holes[0]", n(1.0)),
        ("holes[1]", x()),
        ("holes[2]", n(3.0)),
        ("ghost", x()),
        ("ghost.a", x()),
        ("ghost[0]", x()),
        ("ghost['a'].b", x()),
    ];
    for (src, want) in table {
        r.val(&sc, src, want);
    }
    // DECIDED: a text that looks like a position, against a list: `[expr]` says "by whole number" for a list.
    r.amb(&sc, "list['1']", n(20.0));
    r.amb(&sc, "list['01']", x());
    r.amb(&sc, "list[-0]", n(10.0)); // -0 is a whole number from 0, or is it "negative"?
    // DECIDED: a number against a map: `[expr]` says "by text" for a map; is 1 the key "1"?
    r.amb(&sc, "m[1]", s("one"));
    r.amb(&sc, "m[task.n]", s("one"));
    r
}

#[test]
fn paths() {
    rows_paths().done("paths");
}

// ── 5. pointers ──────────────────────────────────────────────────────────────────────────────

fn rows_pointers() -> Rows {
    let mut r = Rows::default();
    let sc = world();
    let keys = match doc() {
        Value::Map(mm) => mm.len() as f64,
        _ => unreachable!(),
    };
    let table: Vec<(&str, Value)> = vec![
        ("/type", s("text")),
        ("/usage/cost_usd", n(0.5)),
        ("/usage/n", n(3.0)),
        ("/content/0/text", s("hello")),
        ("/content/0/type", s("text")),
        ("/content/1/type", s("image")),
        ("/content/1", m(&[("type", s("image"))])),
        ("/nums/0", n(1.0)),
        ("/nums/2", n(3.0)),
        ("/obj/0", s("zero-key")), // a digit name on a map is a key
        ("/grid/1/0", n(3.0)),
        ("/grid/0", l(vec![n(1.0), n(2.0)])),
        ("/grid/2/0", x()),
        ("/grid/1/2", x()),
        ("/grid[1][1]", n(4.0)),
        ("len(/grid)", n(2.0)),
        ("last(/grid)", l(vec![n(3.0), n(4.0)])),
        ("/flag", t()),
        ("/dur", d(90_000)),
        ("/cost", u(1_500_000)),
        // the whole document
        ("/", doc()),
        ("len(/)", n(keys)),
        // nowhere to go
        ("/nope", x()),
        ("/nope/deeper", x()),
        ("/type/x", x()),
        ("/usage/n/x", x()),
        ("/usage/", x()), // the empty name inside usage: none there
        ("/content/2", x()),
        ("/content/2/type", x()),
        ("/content/-1", x()),
        ("/content/01", x()),
        ("/content/x", x()),
        ("/content/1.0", x()),
        ("/nums/3", x()),
        ("/nums/-", x()),
        // ~1 is /, ~0 is ~ (RFC 6901), and the order of undoing them
        ("/a~1b", n(1.0)),
        ("/m~0n", n(2.0)),
        ("/x~01y", n(3.0)), // ~01 is the two characters ~1
        ("/x~1y", x()),     // the key x/y is not there
        // names with the characters a pointer may hold
        ("/a-b", n(9.0)),
        ("/a:b", n(10.0)),
        ("/a@b", n(11.0)),
        ("/a$b", n(12.0)),
        ("/a%b", n(13.0)),
        ("/a+b", n(14.0)),
        ("/k_1", n(15.0)),
        ("/meta/v", n(8.0)),
        // steps after a pointer
        ("/content[0].type", s("text")),
        ("/content[1]['type']", s("image")),
        ("/content[5].type", x()),
        ("/nums[2]", n(3.0)),
        ("/usage['n']", n(3.0)),
        ("/content[task.zero].text", s("hello")),
        // a pointer ends at anything else
        ("[/type, /usage/n]", l(vec![s("text"), n(3.0)])),
        ("(/type)", s("text")),
        ("len(/content)", n(2.0)),
        ("len(/type)", n(4.0)),
        ("count(/nums)", n(3.0)),
        ("sum(/nums)", n(6.0)),
        ("last(/nums)", n(3.0)),
        ("lower(/type)", s("text")),
    ];
    for (src, want) in table {
        r.val(&sc, src, want);
    }
    // pointers in comparisons and conditions
    let bools: Vec<(&str, bool)> = vec![
        (r#"/type=="text""#, true),
        ("/type=='text'", true),
        ("/type == 'text'", true),
        ("/type=='image'", false),
        ("/type != 'text'", false),
        ("/type!='image'", true),
        ("/usage/n>2", true),
        ("/usage/n>=3", true),
        ("/usage/n<3", false),
        ("/usage/n<=3", true),
        ("/usage/n==3", true),
        ("/usage/cost_usd<$1", true),
        ("/usage/cost_usd==$0.5", true),
        ("/nope=='x'", false),
        ("/nope != 'x'", false),
        ("/nope==/nope2", false),
        ("/nope!=/nope2", false),
        ("/type==/content/0/type", true),
        ("/content/0/type=='text' and /content/1/type=='image'", true),
        ("/content/0/type=='image' or /content/1/type=='image'", true),
        ("/type in ['text','image']", true),
        ("/type in ['audio','image']", false),
        ("'tex' in /type", true),
        ("/flag", true),
        ("not /flag", false),
        ("not /nope", true),
        ("/dur > 1m", true),
        ("/dur == 90s", true),
        ("/dur < 1m", false),
        ("/cost == $1.5", true),
        ("/cost > 1", true),
        ("/type matches 't*'", true),
        ("/type matches 'x*'", false),
        ("2 in /nums", true),
        ("5 in /nums", false),
        ("'a' in /obj", false),
        ("'0' in /obj", true),
        ("same(/nums)", false),
    ];
    for (src, want) in bools {
        r.yes(&sc, src, want);
    }
    r.hold(&sc, "/type=='text'", true);
    r.hold(&sc, "/nope", false);
    r.hold(&sc, "/usage/n", true);
    // no document bound: every pointer is missing
    let nodoc = Scope::new();
    for src in ["/type", "/", "/usage/n", "/content/0", "/a~1b"] {
        r.val(&nodoc, src, x());
    }
    r.yes(&nodoc, "/type=='text'", false);
    r.yes(&nodoc, "not /type", true);
    // DECIDED: `.` is a pointer character, yet the grammar lets `.name` follow a pointer. The reading taken:
    // the pointer swallows the dot (so the key "meta.v" is read).
    r.amb(&sc, "/meta.v", n(7.0));
    // DECIDED: `%` is a pointer character; is `%20` decoded (URI fragment form) or kept (JSON string form)?
    r.amb(&sc, "/a%20b", n(17.0));
    // DECIDED: a `~` that is not `~0` or `~1` is an invalid pointer in RFC 6901.
    r.amb(&sc, "/m~n", n(2.0));
    r.amb(&sc, "/a~2b", x());
    r
}

#[test]
fn pointers() {
    rows_pointers().done("pointers");
}

// ── 6. comparisons, every operator against the pairs of kinds that matter ────────────────────

/// (a, b, pattern): the pattern says what `a == b`, `a != b`, `a < b`, `a <= b`, `a > b`, `a >= b` are. `T` yes,
/// `F` no, anything else: the cell is not a row here (it is a row of its own, marked DECIDED).
const MATRIX: &[(&str, &str, &str)] = &[
    // numbers
    ("1", "1", "TFFTFT"),
    ("1", "2", "FTTTFF"),
    ("2", "1", "FTFFTT"),
    ("-1", "0", "FTTTFF"),
    ("0", "-0.5", "FTFFTT"),
    ("1", "1.0", "TFFTFT"),
    ("0.5", "0.25", "FTFFTT"),
    ("10", "9", "FTFFTT"), // numbers by value
    ("task.n", "1", "TFFTFT"),
    ("task.frac", "task.n", "FTFFTT"),
    // text, by character
    ("'a'", "'a'", "TFFTFT"),
    ("'a'", "'b'", "FTTTFF"),
    ("'b'", "'a'", "FTFFTT"),
    ("'a'", "'B'", "FTFFTT"), // 'a' is after 'B' by character
    ("'Z'", "'a'", "FTTTFF"),
    ("'a'", "'A'", "FTFFTT"),
    ("'ab'", "'abc'", "FTTTFF"),
    ("'abc'", "'abd'", "FTTTFF"),
    ("''", "'a'", "FTTTFF"),
    ("''", "''", "TFFTFT"),
    ("'é'", "'z'", "FTFFTT"),
    ("'10'", "'9'", "FTTTFF"),       // text is not read as a number
    ("'é'", "'e\u{301}'", "FTFFTT"), // by character: the precomposed é is not e and an accent
    ("task.name", "'Plant'", "TFFTFT"),
    // bools: equality only (their order is a row of its own)
    ("true", "true", "TF----"),
    ("true", "false", "FT----"),
    ("false", "false", "TF----"),
    ("task.flag", "true", "TF----"),
    ("task.off", "false", "TF----"),
    // durations
    ("5m", "300s", "TFFTFT"),
    ("1m", "90s", "FTTTFF"),
    ("2h", "90m", "FTFFTT"),
    ("250ms", "0.25s", "TFFTFT"),
    ("1d", "24h", "TFFTFT"),
    ("0s", "0ms", "TFFTFT"),
    ("1h", "59m", "FTFFTT"),
    ("1h30m", "90m", "TFFTFT"),
    ("task.took", "90s", "TFFTFT"),
    ("task.took", "1m", "FTFFTT"),
    // money
    ("$1.50", "$1.5", "TFFTFT"),
    ("$0.0004", "$0.001", "FTTTFF"),
    ("$2", "$1.99", "FTFFTT"),
    ("$0", "$0", "TFFTFT"),
    ("$0.000001", "$0", "FTFFTT"),
    ("$1", "$1.000000", "TFFTFT"),
    ("task.cost", "$1.50", "TFFTFT"),
    ("task.cost", "$2", "FTTTFF"),
    // a plain number against money counts in dollars
    ("1", "$1", "T~FTFT"),
    ("2", "$1.50", "FTFFTT"),
    ("$1", "2", "FTTTFF"),
    ("0.0004", "$0.0004", "T~FTFT"),
    ("1", "$2", "FTTTFF"),
    ("0", "$0", "T~FTFT"),
    ("$1.50", "1.5", "T~FTFT"),
    ("100", "$99.99", "FTFFTT"),
    ("task.cost", "1", "FTFFTT"),
    ("task.cost", "2", "FTTTFF"),
    // a plain number against a duration does not compare
    ("5", "5s", "FTFFFF"),
    ("5s", "5", "FTFFFF"),
    ("1000", "1s", "FTFFFF"),
    ("1s", "1000", "FTFFFF"),
    ("0", "0s", "FTFFFF"),
    ("1", "1ms", "FTFFFF"),
    ("task.n", "task.took", "FTFFFF"),
    // other pairs of different kinds: not equal, no order
    ("'1'", "1", "FTFFFF"),
    ("1", "'1'", "FTFFFF"),
    ("'a'", "1", "FTFFFF"),
    ("'1'", "$1", "FTFFFF"),
    ("'5s'", "5s", "FTFFFF"),
    ("'true'", "true", "FTFFFF"),
    ("true", "1", "FTFFFF"),
    ("1", "true", "FTFFFF"),
    ("false", "0", "FTFFFF"),
    ("false", "''", "FTFFFF"),
    ("5s", "$5", "FTFFFF"),
    ("$5", "5s", "FTFFFF"),
    ("[1]", "1", "FTFFFF"),
    ("[1]", "'1'", "FTFFFF"),
    ("[1]", "true", "FTFFFF"),
    ("[]", "0", "FTFFFF"),
    ("[]", "''", "FTFFFF"),
    ("[]", "false", "FTFFFF"),
    ("mapa", "1", "FTFFFF"),
    ("mapa", "'x'", "FTFFFF"),
    ("mapa", "[1, 2]", "FTFFFF"),
    ("emptymap", "[]", "FTFFFF"),
    ("[]", "emptymap", "FTFFFF"),
    ("emptymap", "0", "FTFFFF"),
    // lists: item by item, no order
    ("[1, 2]", "[1, 2]", "TFFFFF"),
    ("[1, 2]", "[2, 1]", "FTFFFF"),
    ("[1]", "[1, 2]", "FTFFFF"),
    ("[1, 2]", "[1]", "FTFFFF"),
    ("[]", "[]", "TFFFFF"),
    ("[]", "emptylist", "TFFFFF"),
    ("[1, [2]]", "[1, [2]]", "TFFFFF"),
    ("[1, [2]]", "[1, [3]]", "FTFFFF"),
    ("[1, 2]", "[1.0, 2.0]", "TFFFFF"),
    ("[1, 'a']", "[1, 'a']", "TFFFFF"),
    ("[1, 'a']", "['a', 1]", "FTFFFF"),
    ("['1']", "[1]", "FTFFFF"),
    ("[1]", "[$1]", "TFFFFF"), // the items compare as `==` does: a number against money in dollars
    ("list", "[10, 20, 30]", "TFFFFF"),
    ("list", "list", "TFFFFF"),
    ("list", "nums", "FTFFFF"),
    ("[5s]", "[5000ms]", "TFFFFF"),
    // maps: key and value, no order
    ("mapa", "mapb", "TFFFFF"),
    ("mapa", "mapa", "TFFFFF"),
    ("mapa", "mapc", "FTFFFF"),
    ("mapc", "mapa", "FTFFFF"),
    ("mapa", "mapd", "FTFFFF"),
    ("mapa", "mape", "FTFFFF"),
    ("emptymap", "emptymap", "TFFFFF"),
    ("nested1", "nested2", "TFFFFF"),
    ("nested1", "nested3", "FTFFFF"),
    ("m", "m", "TFFFFF"),
    ("task", "task", "TFFFFF"),
    ("task", "mapa", "FTFFFF"),
    // nothing compares with nothing
    ("nothing", "nothing", "FFFFFF"),
    ("nothing", "1", "FFFFFF"),
    ("1", "nothing", "FFFFFF"),
    ("nothing", "0", "FFFFFF"),
    ("0", "nothing", "FFFFFF"),
    ("nothing", "''", "FFFFFF"),
    ("''", "nothing", "FFFFFF"),
    ("nothing", "'x'", "FFFFFF"),
    ("nothing", "true", "FFFFFF"),
    ("nothing", "false", "FFFFFF"),
    ("false", "nothing", "FFFFFF"),
    ("nothing", "[]", "FFFFFF"),
    ("[]", "nothing", "FFFFFF"),
    ("nothing", "emptymap", "FFFFFF"),
    ("nothing", "5s", "FFFFFF"),
    ("nothing", "$0", "FFFFFF"),
    ("$0", "nothing", "FFFFFF"),
    ("task.nope", "0", "FFFFFF"),
    ("task.nope", "task.nope2", "FFFFFF"),
    ("list[9]", "list[9]", "FFFFFF"),
    ("m.a.zz", "'x'", "FFFFFF"),
    ("task.n", "task.nope", "FFFFFF"),
    ("/nope", "/nope", "FFFFFF"),
    ("last(emptylist)", "last(emptylist)", "FFFFFF"),
];

fn rows_comparisons() -> Rows {
    let mut r = Rows::default();
    let sc = world();
    const OPS: [&str; 6] = ["==", "!=", "<", "<=", ">", ">="];
    for (a, b, pat) in MATRIX {
        assert_eq!(pat.chars().count(), 6, "bad pattern for {a} / {b}");
        for (op, ch) in OPS.iter().zip(pat.chars()) {
            match ch {
                'T' => r.yes(&sc, &format!("{a} {op} {b}"), true),
                'F' => r.yes(&sc, &format!("{a} {op} {b}"), false),
                _ => {}
            }
        }
    }
    // `not (x == 1)` is "is not 1, or is not there"
    r.yes(&sc, "not (nothing == 1)", true);
    r.yes(&sc, "not (task.n == 1)", false);
    r.yes(&sc, "not (task.nope == 1)", true);
    // the answer of a comparison is a bool, also with nothing on a side (not missing)
    r.val(&sc, "nothing < 1", f());
    r.val(&sc, "nothing == nothing", f());
    r.val(&sc, "nothing != 1", f());
    r.val(&sc, "1 != nothing", f());
    // a comparison reads paths and calls on both sides
    r.yes(&sc, "task.n == list[0] and 1 == 1", false);
    r.yes(&sc, "list[1] == 20", true);
    r.yes(&sc, "list[1] > list[0]", true);
    r.yes(&sc, "last(list) >= len(list)", true);
    r.yes(&sc, "len(list) == 3", true);
    // money against a number in dollars where the float is not exact
    for (a, b) in [
        ("1.1", "$1.1"),
        ("0.3", "$0.3"),
        ("2.675", "$2.675"),
        ("0.07", "$0.07"),
        ("0.000001", "$0.000001"),
        ("123456.789", "$123456.789"),
    ] {
        r.yes(&sc, &format!("{a} == {b}"), true);
        r.yes(&sc, &format!("{b} == {a}"), true);
        r.yes(&sc, &format!("{a} <= {b}"), true);
        r.yes(&sc, &format!("{b} >= {a}"), true);
        r.yes(&sc, &format!("{a} < {b}"), false);
        r.yes(&sc, &format!("{b} > {a}"), false);
    }
    // the extremes of money and of durations compare, and are yes
    r.yes(&sc, "maxusd > 1", true);
    r.yes(&sc, "minusd < 1", true);
    r.yes(&sc, "minusd < 0", true);
    r.yes(&sc, "maxusd == maxusd", true);
    r.yes(&sc, "minusd == minusd", true);
    r.yes(&sc, "maxusd > minusd", true);
    r.yes(&sc, "minusd < maxusd", true);
    r.yes(&sc, "maxusd > $1000000", true);
    r.yes(&sc, "maxdur > 1d", true);
    r.yes(&sc, "maxdur == maxdur", true);
    r.yes(&sc, "maxdur >= maxdur", true);
    r.yes(&sc, "maxdur < 1d", false);
    r.hold(&sc, "maxdur", true);
    r.hold(&sc, "maxusd", true);
    r.hold(&sc, "minusd", true);
    // DECIDED: a number with more places than money has, against money: "counts in dollars" (exact), or rounded to a
    // millionth first?
    r.yes_amb(&sc, "0.0000004 == $0", true);
    r.yes_amb(&sc, "0.0000004 > $0", false);
    r.yes_amb(&sc, "$0 < 0.0000004", false);
    // DECIDED: a number against money is equal in dollars, yet `!=` is "true for two things of different kinds".
    r.yes_amb(&sc, "1 != $1", false);
    r.yes_amb(&sc, "$1 != 1", false);
    r.yes_amb(&sc, "0.0004 != $0.0004", false);
    r.yes_amb(&sc, "task.cost != 1.5", false);
    // DECIDED: bools are scalars of one kind, but the contract names an order only for text, numbers, durations,
    // money; the reading taken is "no order".
    r.yes_amb(&sc, "false < true", false);
    r.yes_amb(&sc, "true > false", false);
    r.yes_amb(&sc, "true >= true", false);
    r.yes_amb(&sc, "true <= true", false);
    // DECIDED: a list with a missing item against itself: "nothing compares with nothing", so an item that is
    // missing is not equal to itself, so the list is not either.
    r.yes_amb(&sc, "holes == holes", false);
    r.yes_amb(&sc, "mapmiss == mapmiss", false);
    r.yes_amb(&sc, "onlymissing == onlymissing", false);
    r.yes_amb(&sc, "holes != holes", false);
    r
}

#[test]
fn comparisons() {
    rows_comparisons().done("comparisons");
}

// ── 7. in / not in ───────────────────────────────────────────────────────────────────────────

fn rows_membership() -> Rows {
    let mut r = Rows::default();
    let sc = world();
    let table: Vec<(&str, bool)> = vec![
        // x in list: an item == x
        ("1 in [1, 2]", true),
        ("3 in [1, 2]", false),
        ("'1' in [1, 2]", false),
        ("1 in ['1']", false),
        ("'a' in strs", true),
        ("'c' in strs", false),
        ("1 in []", false),
        ("1 in emptylist", false),
        ("1.0 in [1]", true),
        ("[1] in [[1], [2]]", true),
        ("[3] in [[1], [2]]", false),
        ("[1] in lists", true),
        ("[2] in lists", true),
        ("[3] in lists", false),
        ("1 in [$1]", true), // == counts a number against money in dollars
        ("$1 in [1]", true),
        ("$1.50 in [1, 2]", false),
        ("5s in [5000]", false), // a number does not compare with a duration
        ("5s in [5s]", true),
        ("90s in [1m30s]", true),
        ("true in [true]", true),
        ("true in [1]", false),
        ("false in [0]", false),
        ("'' in ['']", true),
        ("'' in []", false),
        ("task.id in ['t1', 't2']", true),
        ("task.id in strs", false),
        ("task.n in nums", true),
        ("nothing in [1]", false),
        ("task.nope in [1]", false),
        ("nothing in [nothing]", false),
        ("1 in [nothing, 1]", true),
        ("1 in holes", true),
        ("2 in holes", false),
        ("3 in holes", true),
        ("nothing in holes", false),
        ("1 in nothing", false),
        ("1 in task.nope", false),
        ("nothing in nothing", false),
        // not a list, a map or a text on the right
        ("1 in 5", false),
        ("1 in true", false),
        ("'a' in 5", false),
        ("1 in 1", false),
        ("5s in 5s", false),
        // key in map
        ("'a' in m", true),
        ("'k' in m", true),
        ("'zz' in m", false),
        ("'' in m", true),
        ("'1' in m", true),
        ("'x y' in m", true),
        ("'a' in emptymap", false),
        ("'x' in mapa", true),
        ("'z' in mapa", false),
        ("'v' in m", false), // a value is not a key
        ("nothing in m", false),
        ("'b' in m.a", true),
        ("'c' in m.a", false),
        ("'name' in m.lst[0]", true),
        ("'id' in task", true),
        ("'nope' in task", false),
        // part in text
        ("'ell' in 'hello'", true),
        ("'hello' in 'hello'", true),
        ("'z' in 'hello'", false),
        ("'x' in ''", false),
        ("'HELLO' in 'hello'", false),
        ("'hello world' in 'hello'", false),
        ("'llo w' in 'hello world'", true),
        ("'é' in 'café'", true),
        ("'afé' in 'café'", true),
        ("'fa' in 'café'", false),
        ("task.id in 't1-extra'", true),
        ("task.nope in 'abc'", false),
        ("'a' in task.nope", false),
        ("1 in 'a1'", false),
        ("5s in '5s'", false),
        ("true in 'true'", false),
        ("[1] in 'a1'", false),
        ("'git' in task.cmd", true),
        ("'rm' in task.cmd", false),
        ("'push' in event.data.cmd", true),
        ("lower('ABC') in 'xabcx'", true),
        // not in
        ("3 not in [1, 2]", true),
        ("1 not in [1, 2]", false),
        ("'a' not in 'abc'", false),
        ("'z' not in 'abc'", true),
        ("'a' not in m", false),
        ("'zz' not in m", true),
        ("'c' not in strs", true),
        ("'a' not in strs", false),
        ("1 not in 5", true),
        ("1 in [1] and 2 not in [1]", true),
        ("1 not in [1] or 2 not in [1]", true),
        ("1 not in [1] or 2 not in [2]", false),
        ("not 1 in [1]", false),
        ("not 3 in [1, 2]", true),
        ("not (1 in [1])", false),
    ];
    for (src, want) in &table {
        r.yes(&sc, src, *want);
    }
    // DECIDED: the empty text is part of every text.
    r.yes_amb(&sc, "'' in 'hello'", true);
    r.yes_amb(&sc, "'' in ''", true);
    // DECIDED: the key of a map is a text; is the number 1 the key "1"?
    r.yes_amb(&sc, "1 in m", false);
    // DECIDED: `not in` of nothing: the negation of `in` (true), or "nothing compares with nothing" (false)?
    r.yes_amb(&sc, "nothing not in [1]", true);
    r.yes_amb(&sc, "task.nope not in strs", true);
    r.yes_amb(&sc, "1 not in nothing", true);
    r.yes_amb(&sc, "nothing not in nothing", true);
    r
}

#[test]
fn membership() {
    rows_membership().done("membership");
}

// ── 8. matches ───────────────────────────────────────────────────────────────────────────────

fn rows_matches() -> Rows {
    let mut r = Rows::default();
    let sc = world();
    // (text, pattern, does it match)
    let table: Vec<(&str, &str, bool)> = vec![
        // `*` runs, dots and slashes too
        ("src/main.rs", "*.rs", true),
        ("src/main.rs", "src/*", true),
        ("src/main.rs", "*", true),
        ("src/main.rs", "src/*.rs", true),
        ("src/main.rs", "*.py", false),
        ("src/main.rs", "main.rs", false), // the whole text, not a part
        ("a/b/c/d.txt", "a/*/d.txt", true),
        ("a/b/c/d.txt", "a/*/*/d.txt", true),
        ("a.b.c", "a*c", true),
        ("a.b.c", "a.*", true),
        ("a.b.c", "*.c", true),
        ("path/to/file", "*/file", true),
        ("file", "*/file", false),
        ("Make file", "* *", true),
        ("abc", "*", true),
        ("abc", "**", true),
        ("abc", "a*", true),
        ("abc", "*c", true),
        ("abc", "a*c", true),
        ("ac", "a*c", true), // `*` may be empty
        ("abc", "*b*", true),
        ("abc", "*b", false),
        ("abc", "b*", false),
        ("x", "*x*", true),
        ("aaab", "*a*b", true),
        ("ab", "a*b*", true),
        ("aXbXc", "a*b*c", true),
        ("abab", "*ab", true),
        ("ababab", "*abab", true),
        ("abcabc", "*abc", true),
        ("abcabd", "*abc", false),
        ("abcabd", "*abd", true),
        // `?` one character
        ("abc", "a?c", true),
        ("ac", "a?c", false),
        ("abbc", "a?c", false),
        ("abc", "?", false),
        ("a", "?", true),
        ("abc", "???", true),
        ("abc", "??", false),
        ("abc", "????", false),
        ("a/b", "a?b", true), // `?` is any one character: a slash too
        ("a.b", "a?b", true),
        ("xx", "x?", true),
        ("x", "x?", false),
        ("é", "?", true),
        ("café", "caf?", true),
        ("café", "caf??", false),
        ("日本", "??", true),
        ("日本", "?", false),
        ("abc", "?*", true),
        ("a", "?*", true),
        ("", "?*", false),
        ("abc", "*?", true),
        ("", "*?", false),
        // the empty pattern and the empty text
        ("", "", true),
        ("a", "", false),
        ("", "*", true),
        ("", "?", false),
        ("", "**", true),
        ("", "a", false),
        // everything else is itself
        ("abc", "abc", true),
        ("abc", "ab", false),
        ("abc", "bc", false),
        ("abc", "b", false),
        ("abc", "abcd", false),
        ("a.c", "a.c", true),
        ("abc", "a.c", false), // a dot is a dot
        ("aac", "a+c", false),
        ("a+c", "a+c", true),
        ("a[b]", "a[b]", true),
        ("ab", "a[b]", false),
        ("(x)", "(x)", true),
        ("a^b", "a^b", true),
        ("a$b", "a$b", true),
        ("ABC", "abc", false),
        ("abc", "ABC", false),
        ("Abc", "A*", true),
        ("abc", "A*", false),
        ("a*c", "a*c", true),
        ("a?c", "a?c", true),
        ("abc", "a|c", false),
        ("a|c", "a|c", true),
    ];
    for (text, pat, want) in &table {
        r.yes(&sc, &format!("'{text}' matches '{pat}'"), *want);
    }
    // both sides must be text
    let kinds: Vec<(&str, bool)> = vec![
        ("5 matches '5'", false),
        ("5 matches '*'", false),
        ("'5' matches 5", false),
        ("'*' matches 5", false),
        ("nothing matches '*'", false),
        ("'a' matches nothing", false),
        ("nothing matches nothing", false),
        ("task.nope matches '*'", false),
        ("'' matches nothing", false),
        ("[1] matches '*'", false),
        ("'*' matches [1]", false),
        ("true matches 'true'", false),
        ("true matches '*'", false),
        ("5s matches '*'", false),
        ("$1 matches '*'", false),
        ("mapa matches '*'", false),
        ("task.n matches '1'", false),
        // text from the world
        ("task.id matches 't?'", true),
        ("task.name matches 'P*'", true),
        ("task.name matches 'p*'", false),
        ("lower(task.name) matches 'p*'", true),
        ("task.cmd matches 'git push*'", true),
        ("task.cmd matches '*--force*'", false),
        ("event.data.cmd matches 'git *'", true),
        ("task.cmd matches task.cmd", true),
        ("'git push origin main' matches task.cmd", true),
        ("not task.cmd matches 'rm*'", true),
        ("not nothing matches '*'", true),
        ("not (task.cmd matches 'git*')", false),
        ("task.cmd matches 'git*' and task.id matches 't*'", true),
        ("task.cmd matches 'rm*' or task.id matches 'x*'", false),
    ];
    for (src, want) in &kinds {
        r.yes(&sc, src, *want);
    }
    r.yes(&sc, "/type matches 't*'", true);
    r.yes(&sc, "/type matches '*xt'", true);
    r.yes(&sc, "/content/0/text matches 'h?llo'", true);
    r
}

#[test]
fn matches() {
    rows_matches().done("matches");
}

// ── 9. len, lower, contains ──────────────────────────────────────────────────────────────────

fn rows_len_lower_contains() -> Rows {
    let mut r = Rows::default();
    let sc = world();
    let table: Vec<(&str, Value)> = vec![
        // len: characters, items, keys
        ("len('hello')", n(5.0)),
        ("len('héllo')", n(5.0)), // characters, not bytes
        ("len('日本語')", n(3.0)),
        ("len('😀')", n(1.0)),
        ("len('')", n(0.0)),
        ("len('a b')", n(3.0)),
        ("len(task.name)", n(5.0)),
        ("len(uni)", n(11.0)),
        ("len(list)", n(3.0)),
        ("len(strs)", n(2.0)),
        ("len([])", n(0.0)),
        ("len(emptylist)", n(0.0)),
        ("len([[1, 2], [3]])", n(2.0)),
        ("len(holes)", n(3.0)), // the items of the list, those that are missing too
        ("len(mapa)", n(2.0)),
        ("len(emptymap)", n(0.0)),
        ("len(m)", n(6.0)),
        ("len(last(events))", n(2.0)),
        ("len(nothing)", x()),
        ("len(task.nope)", x()),
        ("len(5)", x()),
        ("len(0)", x()),
        ("len(true)", x()),
        ("len(5s)", x()),
        ("len($1)", x()),
        ("len(last(emptylist))", x()),
        // lower
        ("lower('ABC')", s("abc")),
        ("lower('abc')", s("abc")),
        ("lower('MiXed 1É')", s("mixed 1é")),
        ("lower('ÀÉÎ')", s("àéî")),
        ("lower('')", s("")),
        ("lower('A-B_C 9')", s("a-b_c 9")),
        ("lower(task.name)", s("plant")),
        ("lower(lower('AB'))", s("ab")),
        ("lower(nothing)", x()),
        ("lower(task.nope)", x()),
        ("lower(5)", x()),
        ("lower(true)", x()),
        ("lower(5s)", x()),
        ("lower($1)", x()),
        ("lower(['A'])", x()),
        ("lower(mapa)", x()),
        ("lower(last(emptylist))", x()),
        // contains(hay, needle) is `needle in hay`
        ("contains('hello', 'ell')", t()),
        ("contains('hello', 'z')", f()),
        ("contains('hello', 'HELLO')", f()),
        ("contains('hello', 'hello')", t()),
        ("contains('', 'x')", f()),
        ("contains('héllo', 'é')", t()),
        ("contains([1, 2], 2)", t()),
        ("contains([1, 2], 3)", f()),
        ("contains([1, 2], '2')", f()),
        ("contains(list, task.zero)", f()),
        ("contains(nums, task.n)", t()),
        ("contains(strs, 'a')", t()),
        ("contains([], 1)", f()),
        ("contains(m, 'a')", t()),
        ("contains(m, 'zz')", f()),
        ("contains(mapa, 'x')", t()),
        ("contains(emptymap, 'x')", f()),
        ("contains(task.cmd, 'push')", t()),
        ("contains(task.cmd, 'pull')", f()),
        ("contains(lower('ABC'), 'b')", t()),
        ("contains(event.data.cmd, 'git')", t()),
        ("contains(/type, 'ex')", t()),
        ("contains(holes, 3)", t()),
        ("contains(holes, nothing)", f()),
        ("contains([1], nothing)", f()),
    ];
    for (src, want) in table {
        r.val(&sc, src, want);
    }
    // DECIDED: `contains(hay, needle)` "is `needle in hay`" (false for anything else) yet functions "answer missing
    // for a kind they do not take". The reading taken: a hay of a wrong kind (or no hay) is missing.
    r.amb(&sc, "contains(task.nope, 'a')", Value::Bool(false));
    r.amb(&sc, "contains(nothing, 'a')", Value::Bool(false));
    r.amb(&sc, "contains(5, 5)", Value::Bool(false));
    r.amb(&sc, "contains(true, true)", Value::Bool(false));
    r.amb(&sc, "contains(5s, 5s)", Value::Bool(false));
    // DECIDED: a needle of a kind the hay cannot hold: `in` says false; does the function say false or missing?
    r.amb(&sc, "contains('hello', 5)", f());
    r.amb(&sc, "contains('hello', nothing)", f());
    r.amb(&sc, "contains(m, 1)", f());
    r.amb(&sc, "contains('hello', '')", t());
    r
}

#[test]
fn len_lower_contains() {
    rows_len_lower_contains().done("len_lower_contains");
}

// ── 10. count, same, last ────────────────────────────────────────────────────────────────────

fn rows_count_same_last() -> Rows {
    let mut r = Rows::default();
    let sc = world();
    let table: Vec<(&str, Value)> = vec![
        // count(list): the items that are there
        ("count(list)", n(3.0)),
        ("count(emptylist)", n(0.0)),
        ("count([])", n(0.0)),
        ("count(holes)", n(2.0)),
        ("count(onlymissing)", n(0.0)),
        ("count(tailmissing)", n(1.0)),
        ("count([1, 2, 3, 4])", n(4.0)),
        ("count([[], []])", n(2.0)), // empty items are there
        ("count(nothing)", n(0.0)),  // a missing value reads as an empty list
        ("count(task.nope)", n(0.0)),
        ("count(last(emptylist))", n(0.0)),
        ("count(/nums)", n(3.0)),
        ("count(/nope)", n(0.0)),
        // count(list, x): the items == x
        ("count(list, 20)", n(1.0)),
        ("count(list, 21)", n(0.0)),
        ("count(list, '20')", n(0.0)),
        ("count(dups, 'a')", n(2.0)),
        ("count(dups, 'b')", n(1.0)),
        ("count(dups, 'c')", n(0.0)),
        ("count(dups, 'A')", n(0.0)),
        ("count(ones, 1)", n(3.0)),
        ("count(ones, 1.0)", n(3.0)),
        ("count(ones, $1)", n(3.0)), // 1 == $1 in dollars
        ("count(mix, 1)", n(3.0)),   // 1, $1 and 1.0
        ("count(mix, '1')", n(1.0)),
        ("count(mix, true)", n(1.0)),
        ("count(mix, $1)", n(3.0)),
        ("count(mix, 1s)", n(0.0)),
        ("count(holes, 1)", n(1.0)),
        ("count(holes, 3)", n(1.0)),
        ("count(holes, nothing)", n(0.0)), // nothing is not == anything, not even the items that are missing
        ("count(list, nothing)", n(0.0)),
        ("count(list, task.nope)", n(0.0)),
        ("count(nothing, 1)", n(0.0)),
        ("count(emptylist, 1)", n(0.0)),
        ("count(nums, task.n)", n(1.0)),
        ("count(list, task.n)", n(0.0)),
        ("count(lists, [1])", n(1.0)),
        ("count([[1], [1], [2]], [1])", n(2.0)),
        ("count(durs, 60s)", n(1.0)),
        ("count(durs, 1m)", n(1.0)),
        ("count(durs, 60)", n(0.0)),
        ("count(costs, $0.0004)", n(1.0)),
        ("count(bools, true)", n(2.0)),
        ("count(bools, false)", n(0.0)),
        ("count(bools, 1)", n(0.0)),
        ("count(events, mapa)", n(0.0)),
        // same(list): not empty and every item == the first
        ("same(ones)", t()),
        ("same(nums)", f()),
        ("same(strs)", f()),
        ("same(['a'])", t()),
        ("same(['a', 'a'])", t()),
        ("same(['a', 'A'])", f()),
        ("same([1])", t()),
        ("same([1, 1.0])", t()),
        ("same([1, '1'])", f()),
        ("same([1, $1, 1.0])", t()),
        ("same([5s, 5000ms])", t()),
        ("same([5s, 5])", f()),
        ("same([0, 0s])", f()),
        ("same([[1], [1]])", t()),
        ("same([[1], [2]])", f()),
        ("same([mapa, mapb])", t()),
        ("same([mapa, mapc])", f()),
        ("same(bools)", t()),
        ("same([true, false])", f()),
        ("same(mix)", f()),
        ("same([])", f()),
        ("same(emptylist)", f()),
        ("same(nothing)", f()), // a missing value reads as an empty list
        ("same(task.nope)", f()),
        ("same(holes)", f()),
        ("same(tailmissing)", f()),
        ("same(last(emptylist))", f()),
        ("same(/nums)", f()),
        ("same(/nope)", f()),
        // last(list)
        ("last(list)", n(30.0)),
        ("last(strs)", s("b")),
        ("last([1])", n(1.0)),
        ("last([[1], [2, 3]])", l(vec![n(2.0), n(3.0)])),
        ("last(events)", m(&[("kind", s("b")), ("n", n(2.0))])),
        ("last(lists)", l(vec![n(2.0)])),
        ("last(durs)", d(30_000)),
        ("last(costs)", u(400)),
        ("last(bools)", t()),
        ("last(holes)", n(3.0)),
        ("last(emptylist)", x()),
        ("last([])", x()),
        ("last(nothing)", x()),
        ("last(task.nope)", x()),
        ("last(last(emptylist))", x()),
        ("last(/nums)", n(3.0)),
        ("last(/nope)", x()),
        ("last(last(lists))", n(2.0)),
    ];
    for (src, want) in table {
        r.val(&sc, src, want);
    }
    // DECIDED: `count`, `sum`, `last` and `same` read a *missing* value as an empty list; the contract does not
    // say what they answer for a value of another kind. The reading taken (as for every other function): missing.
    r.amb(&sc, "count(5)", x());
    r.amb(&sc, "count('abc')", x());
    r.amb(&sc, "count(true)", x());
    r.amb(&sc, "count(mapa)", x());
    r.amb(&sc, "count(5, 1)", x());
    r.amb(&sc, "same(5)", Value::Bool(false));
    r.amb(&sc, "same('aa')", Value::Bool(false));
    r.amb(&sc, "same(mapc)", Value::Bool(false));
    r.amb(&sc, "last(5)", x());
    r.amb(&sc, "last('abc')", x());
    r.amb(&sc, "last(mapa)", x());
    // DECIDED: a list whose only/first item is missing: `same` needs every item == the first, and the first is
    // not == itself.
    r.yes_amb(&sc, "same(onlymissing)", false);
    // DECIDED: the last item of a list that ends with a missing item: that item (missing), or the last one that is
    // there (1)?
    r.amb(&sc, "last(tailmissing)", x());
    r.amb(&sc, "last(onlymissing)", x());
    r
}

#[test]
fn count_same_last() {
    rows_count_same_last().done("count_same_last");
}

// ── 11. sum ──────────────────────────────────────────────────────────────────────────────────

fn rows_sum() -> Rows {
    let mut r = Rows::default();
    let sc = world();
    let table: Vec<(&str, Value)> = vec![
        // numbers
        ("sum(list)", n(60.0)),
        ("sum(nums)", n(6.0)),
        ("sum([1.5, 2.5])", n(4.0)),
        ("sum([-1, 1])", n(0.0)),
        ("sum([-5, 2])", n(-3.0)),
        ("sum([7])", n(7.0)),
        ("sum(ones)", n(3.0)),
        ("sum(numhole)", n(3.0)), // an item that is missing is skipped
        ("sum(holes)", n(4.0)),
        ("sum([1, nothing, nothing, 2])", n(3.0)),
        ("sum([task.n, task.n])", n(2.0)),
        // durations
        ("sum(durs)", d(90_000)),
        ("sum([1m, 30s])", d(90_000)),
        ("sum([5s])", d(5_000)),
        ("sum([1d, 1h, 1m, 1s, 1ms])", d(86_400_000 + 3_600_000 + 60_000 + 1_000 + 1)),
        ("sum([1m, nothing, 1m])", d(120_000)),
        // money, exact
        ("sum(costs)", u(1_500_400)),
        ("sum([$1.50, $0.0004])", u(1_500_400)),
        ("sum(tenth)", u(300_000)), // $0.1 + $0.2 is exactly $0.3
        ("sum(costsh)", u(3_000_000)),
        ("sum([$1])", u(1_000_000)),
        ("sum([$0.000001, $0.000001])", u(2)),
        ("sum([$0.1, $0.1, $0.1])", u(300_000)),
        // nothing to add is 0 (a plain number)
        ("sum([])", n(0.0)),
        ("sum(emptylist)", n(0.0)),
        ("sum(nothing)", n(0.0)), // a missing value reads as an empty list
        ("sum(task.nope)", n(0.0)),
        ("sum(onlymissing)", n(0.0)),
        ("sum([nothing, nothing])", n(0.0)),
        ("sum(last(emptylist))", n(0.0)),
        ("sum(/nope)", n(0.0)),
        ("sum(/nums)", n(6.0)),
        // a mix of kinds is missing
        ("sum(mixnu)", x()),
        ("sum(mixnd)", x()),
        ("sum(mixdu)", x()),
        ("sum([1, 5s])", x()),
        ("sum([1, $1])", x()),
        ("sum([5s, $1])", x()),
        ("sum([1, 'a'])", x()),
        ("sum([1, true])", x()),
        ("sum([1, 2, 5s])", x()),
        ("sum([1, nothing, $1])", x()),
        ("sum(mix)", x()),
        // in a comparison
        ("sum(list) == 60", t()),
        ("sum(list) > 50", t()),
        ("sum(durs) == 90s", t()),
        ("sum(durs) > 1m", t()),
        ("sum(durs) < 2m", t()),
        ("sum(costs) == $1.5004", t()),
        ("sum(costs) > 1", t()), // money against a plain number: dollars
        ("sum(costs) < 2", t()),
        ("sum(tenth) == $0.3", t()),
        ("sum(mixnu) == 2", f()), // missing compares with nothing
        ("sum(mixnu) != 2", f()),
        ("sum(nothing) == 0", t()),
        ("sum(emptylist) == 0", t()),
        ("sum(emptylist) == 0s", f()),
        ("sum(emptylist) == $0", t()), // the 0 is a number: it equals $0 in dollars
    ];
    for (src, want) in table {
        r.val(&sc, src, want);
    }
    // DECIDED: kinds the contract does not list as summable ("numbers, durations or money"), on their own.
    r.amb(&sc, "sum(txts)", x());
    r.amb(&sc, "sum(['a', 'b'])", x());
    r.amb(&sc, "sum(bools)", x());
    r.amb(&sc, "sum(lists)", x());
    r.amb(&sc, "sum([mapa])", x());
    // DECIDED: `sum` of a value that is not a list.
    r.amb(&sc, "sum(5)", x());
    r.amb(&sc, "sum('abc')", x());
    r.amb(&sc, "sum(mapa)", x());
    r.amb(&sc, "sum(5s)", x());
    r
}

#[test]
fn sum() {
    rows_sum().done("sum");
}

// ── 12. since ────────────────────────────────────────────────────────────────────────────────

fn rows_since() -> Rows {
    let mut r = Rows::default();
    let sc = world(); // the clock says 100 000 ms
    let table: Vec<(&str, Value)> = vec![
        ("since(1000)", d(99_000)),
        ("since(0)", d(100_000)),
        ("since(100000)", d(0)),
        ("since(99000)", d(1000)),
        ("since(99999)", d(1)),
        ("since(task.started)", d(99_000)),
        ("since(/usage/n)", d(99_997)),
        ("since(list[0])", d(99_990)),
        ("since(last(list))", d(99_970)),
        ("since(99000) == 1s", t()),
        ("since(99000) == 1000", f()), // a duration does not compare with a plain number
        ("since(99000) != 1000", t()),
        ("since(task.started) > 1m", t()),
        ("since(task.started) < 2m", t()),
        ("since(task.started) == 99s", t()),
        ("since(task.started) >= 99s", t()),
        ("since(task.started) > 99s", f()),
        ("since(90000) > 5s", t()),
        ("since(99000) > 5s", f()),
        ("since(99000) < 5s", t()),
        ("since(nothing)", x()),
        ("since(task.nope)", x()),
        ("since('1000')", x()),
        ("since(true)", x()),
        ("since([1000])", x()),
        ("since($1)", x()),
        ("since(mapa)", x()),
        ("since(last(emptylist))", x()),
        ("since(nothing) > 1s", f()),
        ("since(nothing) < 1s", f()),
        ("not (since(nothing) > 1s)", t()),
    ];
    for (src, want) in table {
        r.val(&sc, src, want);
    }
    r.hold(&sc, "since(1000)", true);
    r.hold(&sc, "since(100000)", false); // Dur(0) is no
    r.hold(&sc, "since(nothing)", false);
    r.hold(&sc, "since(1000) > 1m", true);
    // another clock
    let sc5 = Scope::new().at_ms(5000);
    r.val(&sc5, "since(1000)", d(4000));
    r.val(&sc5, "since(5000)", d(0));
    r.val(&sc5, "since(0)", d(5000));
    r.val(&sc5, "since(4999)", d(1));
    let now = 1_700_000_000_000u64;
    let big = Scope::new().at_ms(now).with("ts", n((now - 10_000) as f64));
    r.val(&big, "since(ts)", d(10_000));
    r.val(&big, "since(1699999990000)", d(10_000));
    r.val(&big, "since(ts) > 5s and since(ts) < 15s", t());
    let zero = Scope::new().at_ms(0);
    r.val(&zero, "since(0)", d(0));
    // no clock: missing, whatever the argument
    let noclock = bare();
    r.val(&noclock, "since(1000)", x());
    r.val(&noclock, "since(task.started)", x());
    r.val(&noclock, "since(0)", x());
    r.val(&noclock, "since(nothing)", x());
    r.val(&noclock, "since(1000) > 1s", f());
    r.val(&noclock, "since(1000) < 1s", f());
    r.hold(&noclock, "since(1000)", false);
    r.hold(&noclock, "since(1000) > 1s", false);
    r.yes(&noclock, "not (since(1000) > 1s)", true);
    // DECIDED: a time in the future (no negative duration), a duration given as the time, a fraction of a ms.
    r.amb(&sc, "since(200000)", d(0));
    r.amb(&sc, "since(100001)", d(0));
    r.amb(&sc, "since(5s)", x());
    r.amb(&sc, "since(task.took)", x());
    r
}

#[test]
fn since() {
    rows_since().done("since");
}

// ── 13. quote ────────────────────────────────────────────────────────────────────────────────

/// The text inside the double quotes of `v`, undone: `Err` when it is not text, not wrapped in double quotes, has a
/// bare `"`, a dangling `\` or a raw line break. Returns (the unescaped text, the characters between the quotes).
fn unquote(v: &Value) -> Result<(String, usize), String> {
    let Value::Str(text) = v else { return Err(format!("not text: {v:?}")) };
    let cs: Vec<char> = text.chars().collect();
    if cs.len() < 2 || cs[0] != '"' || cs[cs.len() - 1] != '"' {
        return Err("not wrapped in double quotes".into());
    }
    let inner = &cs[1..cs.len() - 1];
    let mut out = String::new();
    let mut i = 0;
    while i < inner.len() {
        match inner[i] {
            '\\' => {
                if i + 1 >= inner.len() {
                    return Err("a dangling backslash before the closing quote".into());
                }
                out.push(inner[i + 1]);
                i += 2;
            }
            '"' => return Err(format!("a bare double quote at inner position {i}")),
            '\n' | '\r' => return Err(format!("a raw line break at inner position {i}")),
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Ok((out, inner.len()))
}

fn rows_quote() -> Rows {
    let mut r = Rows::default();
    let sc = world();
    let table: Vec<(&str, Value)> = vec![
        ("quote('hi')", s("\"hi\"")),
        ("quote('')", s("\"\"")),
        ("quote('a b')", s("\"a b\"")),
        (r#"quote('say "x"')"#, s(r#""say \"x\"""#)),
        ("quote(dq)", s(r#""say \"x\"""#)),
        (r"quote('a\\b')", s(r#""a\\b""#)),
        ("quote(bs)", s(r#""a\\b""#)),
        (r#"quote('\\"')"#, s(r#""\\\"""#)), // the text \" : both escaped
        ("quote(task.name)", s("\"Plant\"")),
        ("quote(task.cmd)", s("\"git push origin main\"")),
        ("quote(uni)", s("\"héllo wörld\"")),
        ("quote('日本語')", s("\"日本語\"")),
        ("quote(quote('a'))", s(r#""\"a\"""#)),
        ("quote('it\\'s')", s("\"it's\"")),
        ("quote(txt200)", s(&format!("\"{}\"", "a".repeat(200)))), // exactly at the cap: nothing cut
        ("quote(e200)", s(&format!("\"{}\"", "é".repeat(200)))),
        ("len(quote('abc'))", n(5.0)),
        ("len(quote(txt200))", n(202.0)),
        ("len(quote(''))", n(2.0)),
        ("quote('x') == '\"x\"'", t()),
        ("quote(nothing)", x()),
        ("quote(task.nope)", x()),
        ("quote(last(emptylist))", x()),
        ("quote(/nope)", x()),
        ("quote(/type)", s("\"text\"")),
        ("lower(quote('AB'))", s("\"ab\"")),
    ];
    for (src, want) in table {
        r.val(&sc, src, want);
    }
    // one line: no raw line break, ever; a well-formed quoted text
    for src in ["quote('a\\nb')", "quote('a\\rb')", "quote(crlf)", "quote(ctrl)", "quote('\\n')", "quote('x\\n')"] {
        r.prop(&sc, src, "a quoted text without a raw line break", |v| unquote(v).is_ok());
    }
    // `\` and `"` stay recoverable: what was inside comes back after undoing the escapes
    r.prop(&sc, "quote(dq)", "unquotes to the original", |v| {
        unquote(v).map(|(t, _)| t == "say \"x\"").unwrap_or(false)
    });
    r.prop(&sc, "quote(bs)", "unquotes to the original", |v| unquote(v).map(|(t, _)| t == "a\\b").unwrap_or(false));
    r.prop(&sc, "quote(uni)", "unquotes to the original", |v| {
        unquote(v).map(|(t, _)| t == "héllo wörld").unwrap_or(false)
    });
    // over the cap: cut, still well-formed (no bare quote, no half an escape), the head kept, shorter than the input
    for (src, head, given) in [
        ("quote(txt201)", "a".repeat(150), "a".repeat(201)),
        ("quote(longtxt)", "a".repeat(150), "a".repeat(300)),
        ("quote(e201)", "é".repeat(150), "é".repeat(201)),
        ("quote(edge)", "a".repeat(199), format!("{}\"b", "a".repeat(199))),
        ("quote(edgeb)", "a".repeat(199), format!("{}\\b", "a".repeat(199))),
    ] {
        r.prop(&sc, src, "well-formed, the head kept", |v| matches!(unquote(v), Ok((t, _)) if t.starts_with(&head)));
        r.prop(
            &sc,
            src,
            "not the whole text it was given (a cut text shows that it was cut)",
            |v| matches!(unquote(v), Ok((t, _)) if t != given),
        );
    }
    // a text of 150 double quotes: well-formed whatever the cap does with the escapes, and only quotes (and a mark)
    r.prop(&sc, "quote(quoty)", "well-formed, only double quotes (and a possible mark)", |v| {
        matches!(unquote(v), Ok((t, _)) if t.starts_with(&"\"".repeat(100)) && t.chars().all(|c| c == '"' || c == '…' || c == '.'))
    });
    // DECIDED: "at most QUOTE_MAX characters" and "(the text inside the quotes)": do the escapes count, and the mark
    // of a cut? The strict reading taken: no more than QUOTE_MAX characters between the quotes, whatever they are.
    for src in ["quote(txt201)", "quote(longtxt)", "quote(e201)", "quote(quoty)", "quote(edge)", "quote(edgeb)"] {
        r.prop_amb(
            &sc,
            src,
            &format!("at most {QUOTE_MAX} characters of the text, then the mark"),
            |v| matches!(unquote(v), Ok((t, _)) if t.chars().count() <= QUOTE_MAX + 1),
        );
    }
    // exactly at the cap: nothing cut, nothing marked
    r.prop(
        &sc,
        "quote(txt200)",
        "all 200 characters, no mark",
        |v| matches!(unquote(v), Ok((t, inner)) if t == "a".repeat(200) && inner == 200),
    );
    // DECIDED: the contract names no mark for a cut text; the reading taken is an ellipsis that counts in the cap.
    r.amb(&sc, "quote(txt201)", s(&format!("\"{}…\"", "a".repeat(QUOTE_MAX))));
    r.amb(&sc, "quote(longtxt)", s(&format!("\"{}…\"", "a".repeat(QUOTE_MAX))));
    r.amb(&sc, "quote(e201)", s(&format!("\"{}…\"", "é".repeat(QUOTE_MAX))));
    r.prop_amb(
        &sc,
        "quote(longtxt)",
        "the cut text ends with an ellipsis",
        |v| matches!(unquote(v), Ok((t, _)) if t.ends_with('…')),
    );
    // DECIDED: how a line break is made "one line": escaped as \n, or a space?
    r.amb(&sc, "quote('a\\nb')", s(r#""a b""#));
    r.amb(&sc, "quote('a\\r\\nb')", s(r#""a  b""#));
    // DECIDED: control characters other than a line break (tab, NUL, ESC, DEL): kept, escaped or dropped? The
    // reading taken: none stays raw ("made inert").
    for src in ["quote('a\\tb')", "quote(ctrl)"] {
        r.prop_amb(&sc, src, "no raw control character inside", |v| match v {
            Value::Str(text) => !text.chars().any(|c| c.is_control()),
            _ => false,
        });
    }
    // DECIDED: which kinds `quote` takes besides text is not said.
    r.amb(&sc, "quote(5)", s("\"5\""));
    r.amb(&sc, "quote(1.5)", s("\"1.5\""));
    r.amb(&sc, "quote(true)", s("\"true\""));
    r.amb(&sc, "quote(list)", x());
    r.amb(&sc, "quote(mapa)", x());
    r
}

#[test]
fn quote() {
    rows_quote().done("quote");
}

// ── 14. danger ───────────────────────────────────────────────────────────────────────────────

fn rows_danger() -> Rows {
    let mut r = Rows::default();
    let sc = world();
    let table: Vec<(&str, Value)> = vec![
        // what the matcher answers is what the expression is
        ("danger('git push origin main').class", s("push")),
        ("danger('git push')['class']", s("push")),
        ("danger('rm -rf /tmp/x').class", s("destroy")),
        ("danger('sudo reboot')", t()),
        ("danger('ls -la')", x()),
        ("danger('ls').class", x()),
        ("danger('')", x()),
        ("danger('git push')", m(&[("class", s("push")), ("echo", s("git push"))])),
        ("danger(task.cmd).class", s("push")),
        ("danger(event.data.cmd).class", s("push")),
        ("danger(task.cmd).echo", s("git push origin main")),
        ("danger('  Git PUSH ').class", s("push")),
        ("danger('  Git PUSH ').echo", s("  Git PUSH ")), // the line goes in as it is
        (r"danger('git push\n').echo", s("git push\n")),
        ("danger(lower('GIT PUSH')).class", s("push")),
        ("danger('git push').nope", x()),
        ("danger('git push').class.x", x()),
        ("len(danger('git push'))", n(2.0)),
        // comparisons on the answer
        ("danger('git push origin main').class == 'push'", t()),
        ("danger('git push origin main').class == 'destroy'", f()),
        ("danger('git push origin main').class != 'destroy'", t()),
        ("danger('ls').class == 'push'", f()),
        ("danger('ls').class != 'push'", f()), // nothing compares with nothing
        ("not (danger('ls').class == 'push')", t()),
        ("danger(task.cmd).class in ['push', 'destroy']", t()),
        ("danger('rm -rf /').class in ['push', 'destroy']", t()),
        ("danger('ls').class in ['push', 'destroy']", f()),
        ("danger(task.cmd).class matches 'pu*'", t()),
        ("danger(task.cmd).class matches 'de*'", f()),
        ("lower(danger('git push').class) == 'push'", t()),
        ("len(danger('git push').class)", n(4.0)),
        ("'class' in danger('git push')", t()), // a key of the answer, if it is a map
        ("'push' in danger('git push')", f()),  // a value is not a key
        ("'nope' in danger('git push')", f()),
        ("danger('git push').class == 'push' and danger('rm -rf x').class == 'destroy'", t()),
        ("danger('git push').class == 'destroy' or danger('rm -rf x').class == 'destroy'", t()),
        ("not danger('ls').class", t()),
        ("not danger('git push').class", f()),
        // the argument must be text
        ("danger(nothing)", x()),
        ("danger(task.nope)", x()),
        ("danger(5)", x()),
        ("danger(true)", x()),
        ("danger(5s)", x()),
        ("danger($1)", x()),
        ("danger(['git push'])", x()),
        ("danger(mapa)", x()),
        ("danger(task.n)", x()),
        ("danger(last(emptylist))", x()),
        ("danger(nothing).class == 'push'", f()),
    ];
    for (src, want) in table {
        r.val(&sc, src, want);
    }
    r.hold(&sc, "danger('git push')", true);
    r.hold(&sc, "danger('sudo reboot')", true); // the matcher said `true`
    r.hold(&sc, "danger('ls')", false);
    r.hold(&sc, "danger(5)", false);
    r.hold(&sc, "danger('git push').class == 'push'", true);
    r.hold(&sc, "danger('ls').class == 'push'", false);
    // no matcher: everything is missing
    let nomatcher = bare();
    for src in ["danger('git push')", "danger('git push').class", "danger(task.cmd)", "danger('')", "danger(nothing)"] {
        r.val(&nomatcher, src, x());
    }
    r.yes(&nomatcher, "danger('git push').class == 'push'", false);
    r.yes(&nomatcher, "danger(task.cmd).class != 'push'", false);
    r.yes(&nomatcher, "not danger('git push').class", true);
    r.hold(&nomatcher, "danger('git push')", false);
    r
}

#[test]
fn danger() {
    rows_danger().done("danger");
}

// ── 15. purity and totality ──────────────────────────────────────────────────────────────────

fn rows_total() -> Rows {
    let mut r = Rows::default();
    let sc = world();
    // the same expression over the same world is the same answer
    let sources = [
        "task.n",
        "list[1]",
        "since(1000)",
        "danger('git push').class",
        "quote(task.cmd)",
        "sum(list) > 50 and count(dups, 'a') == 2",
        "/content/0/text",
        "last(events)",
        "holes",
    ];
    for src in sources {
        r.n += 1;
        match Expr::parse(src) {
            Ok(ex) => {
                let a = ex.eval(&sc);
                let b = ex.eval(&sc);
                let c = Expr::parse(src).unwrap().eval(&sc);
                if a != b || a != c {
                    r.bad.push(format!(
                        "{src}\n      two evaluations differ: {} / {} / {}",
                        short(&a),
                        short(&b),
                        short(&c)
                    ));
                }
            }
            Err(e) => r.bad.push(format!("{src}\n      does not parse: {e}")),
        }
    }
    // an expression with a missing thing in every place answers (nothing panics)
    let table: Vec<(&str, Value)> = vec![
        ("len(last(nothing))", x()),
        ("lower(last(nothing))", x()),
        ("count(last(nothing))", n(0.0)),
        ("sum(last(nothing))", n(0.0)),
        ("same(last(nothing))", f()),
        ("last(last(nothing))", x()),
        ("quote(last(nothing))", x()),
        ("danger(last(nothing))", x()),
        ("since(last(nothing))", x()),
        ("nothing in nothing", f()),
        ("nothing matches nothing", f()),
        ("not nothing", t()),
        ("nothing and nothing", f()),
        ("nothing or nothing", f()),
        ("[nothing] == [nothing]", f()),
        ("nothing[nothing]", x()),
        ("nothing.a[nothing].b", x()),
        ("list[list]", x()),
        ("list[mapa]", x()),
        ("m[list]", x()),
        ("m[mapa]", x()),
        ("m[nothing]", x()),
        ("m[true]", x()),
        ("task[task]", x()),
    ];
    for (src, want) in table {
        r.val(&sc, src, want);
    }
    // sums and clocks at the ends of the integers answer (what they answer is not said, but nothing panics)
    for src in
        ["sum(bigdur)", "sum(bigdur2)", "sum(bigusd)", "sum(bigusd2)", "sum(bigneg)", "sum([maxusd, maxusd, maxusd])"]
    {
        r.answers(&sc, src);
    }
    for src in [
        "since(1000000000000000000000000000000)",
        "since(-1000000000000000000000000000000)",
        "since(-1)",
        "since(0)",
        "since(18446744073709551616)",
        "since(18446744073709551615)",
    ] {
        r.answers(&sc, src);
    }
    let latest = Scope::new().at_ms(u64::MAX);
    for src in
        ["since(0)", "since(-1)", "since(-1000000000000000000000000000000)", "since(1)", "since(18446744073709551615)"]
    {
        r.answers(&latest, src);
    }
    // DECIDED: contains with nothing to look in (see len_lower_contains)
    r.amb(&sc, "contains(last(nothing), last(nothing))", Value::Bool(false));
    // an empty scope: every root is missing, every function still answers
    let empty = Scope::new();
    r.val(&empty, "task.id", x());
    r.val(&empty, "count(list)", n(0.0));
    r.val(&empty, "sum(list)", n(0.0));
    r.val(&empty, "len('abc')", n(3.0));
    r
}

#[test]
fn total() {
    rows_total().done("total");
}

// ── 16. the Env: what the evaluator asks of whoever defines the world ────────────────────────

/// An Env that answers only for whole paths, so an evaluator that walks step by step on its own gets nothing.
struct Lazy;

impl Env for Lazy {
    fn get(&self, root: &str, path: &[Key]) -> Value {
        match (root, path) {
            ("fold", [Key::Name(a), Key::Name(b)]) if a == "spend" && b == "t1" => n(5.0),
            ("fold", [Key::Name(a), Key::Name(b)]) if a == "spend" && b == "t2" => n(7.0),
            ("task", [Key::Name(a)]) if a == "id" => s("t1"),
            ("task", [Key::Name(a)]) if a == "n" => n(2.0),
            ("rows", [Key::Idx(i)]) if *i >= 0 => n(*i as f64 * 10.0),
            ("rows", [Key::Idx(i), Key::Name(a)]) if *i >= 0 && a == "v" => n(*i as f64),
            ("deep", [Key::Name(a), Key::Name(b), Key::Name(c)])
                if (a.as_str(), b.as_str(), c.as_str()) == ("a", "b", "c") =>
            {
                t()
            }
            _ => Value::Missing,
        }
    }
}

/// An Env that knows only `get`: the defaults for the rest.
struct OnlyGet;

impl Env for OnlyGet {
    fn get(&self, root: &str, path: &[Key]) -> Value {
        if root == "task" { m(&[("cmd", s("git push"))]).at(path) } else { Value::Missing }
    }
}

/// An Env that writes down what it is asked.
#[derive(Default)]
struct Spy {
    gets: RefCell<Vec<(String, Vec<Key>)>>,
    pointers: RefCell<Vec<String>>,
    dangers: RefCell<Vec<String>>,
}

impl Env for Spy {
    fn get(&self, root: &str, path: &[Key]) -> Value {
        self.gets.borrow_mut().push((root.to_string(), path.to_vec()));
        match root {
            "task" => m(&[("id", s("t1")), ("n", n(1.0))]).at(path),
            _ => Value::Missing,
        }
    }
    fn pointer(&self, ptr: &str) -> Value {
        self.pointers.borrow_mut().push(ptr.to_string());
        s(ptr)
    }
    fn now_ms(&self) -> Option<u64> {
        Some(50)
    }
    fn danger(&self, cmd: &str) -> Value {
        self.dangers.borrow_mut().push(cmd.to_string());
        s("seen")
    }
}

fn name(k: &str) -> Key {
    Key::Name(k.to_string())
}

fn rows_env() -> Rows {
    let mut r = Rows::default();
    // a path is handed over whole: `fold.spend[task.id]` is the root `fold` and the steps spend, t1
    let lazy = Lazy;
    for (src, want) in [
        ("fold.spend[task.id]", n(5.0)),
        ("fold.spend.t1", n(5.0)),
        ("fold.spend['t1']", n(5.0)),
        ("fold['spend']['t1']", n(5.0)),
        ("fold.spend.t2", n(7.0)),
        ("fold.spend", x()),
        ("fold", x()),
        ("fold.spend.t3", x()),
        ("fold.spend[task.nope]", x()),
        ("rows[3]", n(30.0)),
        ("rows[0]", n(0.0)),
        ("rows[task.n]", n(20.0)),
        ("rows[1.0]", n(10.0)),
        ("rows[2].v", n(2.0)),
        ("rows[1.5]", x()),
        ("rows[true]", x()),
        ("deep.a.b.c", t()),
        ("deep.a.b", x()),
        ("deep.a.b.c.d", x()),
        ("task.id", s("t1")),
        ("task.id == 't1'", t()),
        ("fold.spend[task.id] > 4 and fold.spend.t2 < 8", t()),
    ] {
        r.val(&lazy, src, want);
    }
    // an Env that knows only `get`: no document, no clock, no matcher
    let only = OnlyGet;
    for src in [
        "/type",
        "/",
        "/a/0/b",
        "since(1)",
        "since(task.nope)",
        "danger('git push')",
        "danger(task.cmd)",
        "danger(task.cmd).class",
    ] {
        r.val(&only, src, x());
    }
    r.val(&only, "task.cmd", s("git push"));
    r.yes(&only, "danger(task.cmd).class == 'push'", false);
    r.yes(&only, "since(1) > 1s", false);
    r.hold(&only, "/type=='text'", false);
    r.hold(&only, "danger('git push')", false);
    r.hold(&only, "since(1)", false);
    r.hold(&only, "task.cmd", true);
    // what the evaluator hands to the Env
    let spy = Spy::default();
    let check = |r: &mut Rows, label: &str, ok: bool, saw: String| {
        r.n += 1;
        if !ok {
            r.bad.push(format!("{label}\n      the Env saw {saw}"));
        }
    };
    let _ = Expr::parse("a.b.c.d").map(|e| e.eval(&spy));
    let g = spy.gets.borrow().clone();
    check(
        &mut r,
        "a.b.c.d: one get, the root a and the steps b, c, d",
        g == vec![("a".to_string(), vec![name("b"), name("c"), name("d")])],
        format!("{g:?}"),
    );
    spy.gets.borrow_mut().clear();
    let _ = Expr::parse("list[1]").map(|e| e.eval(&spy));
    let g = spy.gets.borrow().clone();
    check(
        &mut r,
        "list[1]: one get, the root list and the position 1",
        g == vec![("list".to_string(), vec![Key::Idx(1)])],
        format!("{g:?}"),
    );
    spy.gets.borrow_mut().clear();
    let _ = Expr::parse("m['a'].b").map(|e| e.eval(&spy));
    let g = spy.gets.borrow().clone();
    check(
        &mut r,
        "m['a'].b: one get, the root m and the names a, b",
        g == vec![("m".to_string(), vec![name("a"), name("b")])],
        format!("{g:?}"),
    );
    spy.gets.borrow_mut().clear();
    let _ = Expr::parse("list[task.n]").map(|e| e.eval(&spy));
    let g = spy.gets.borrow().clone();
    check(
        &mut r,
        "list[task.n]: the index is read (task, n) and the list is read at the position 1",
        g.contains(&("task".to_string(), vec![name("n")])) && g.contains(&("list".to_string(), vec![Key::Idx(1)])),
        format!("{g:?}"),
    );
    spy.gets.borrow_mut().clear();
    let _ = Expr::parse("m[task.id].x").map(|e| e.eval(&spy));
    let g = spy.gets.borrow().clone();
    check(
        &mut r,
        "m[task.id].x: the index is read, then m with the names t1, x",
        g.contains(&("m".to_string(), vec![name("t1"), name("x")])),
        format!("{g:?}"),
    );
    spy.gets.borrow_mut().clear();
    let _ = Expr::parse("nothing").map(|e| e.eval(&spy));
    let g = spy.gets.borrow().clone();
    check(&mut r, "a bare root: one get with no steps", g == vec![("nothing".to_string(), vec![])], format!("{g:?}"));
    // pointers go to Env::pointer as written
    for (src, want) in
        [("/usage/cost_usd", "/usage/cost_usd"), ("/a/0/b", "/a/0/b"), ("/a~1b/m~0n", "/a~1b/m~0n"), ("/type", "/type")]
    {
        r.val(&spy, src, s(want));
    }
    // the steps after a pointer are taken on what the Env answered (here a text: it has no steps)
    r.val(&spy, "/content[0]", x());
    // DECIDED: `.` is a pointer character, so `.type` after a pointer is part of the pointer (see pointers)
    r.amb(&spy, "/content.type", s("/content.type"));
    let p = spy.pointers.borrow().clone();
    check(
        &mut r,
        "/content[0]: the Env is asked for the pointer /content only",
        p.iter().any(|q| q == "/content"),
        format!("{p:?}"),
    );
    r.yes(&spy, "/type == '/type'", true);
    r.val(&spy, "since(20)", d(30));
    r.val(&spy, "danger('x y')", s("seen"));
    let dg = spy.dangers.borrow().clone();
    check(
        &mut r,
        "danger('x y'): the matcher is asked about the line as it is",
        dg == vec!["x y".to_string()],
        format!("{dg:?}"),
    );
    // DECIDED: what Env::pointer is handed for `/` alone ("the whole document"): "/" as written, or ""?
    spy.pointers.borrow_mut().clear();
    let _ = Expr::parse("/").map(|e| e.eval(&spy));
    let p = spy.pointers.borrow().clone();
    r.n += 1;
    if p != vec![String::new()] {
        r.bad.push(format!("[DECIDED] `/` alone: Env::pointer is handed the empty pointer\n      the Env saw {p:?}"));
    }
    r
}

#[test]
fn env() {
    rows_env().done("env");
}

// ── 17. a glob is bounded by what was written ────────────────────────────────────────────────

fn rows_glob_bounded() -> Rows {
    let mut r = Rows::default();
    // "evaluation is bounded by what was written": many stars against a long text that cannot match must not take
    // exponential time. Each case runs on its own thread with 5 s to answer.
    let cases: Vec<(String, bool)> = vec![
        (format!("'{}' matches '{}b'", "a".repeat(60), "*a".repeat(12)), false),
        (format!("'{}' matches '{}'", "a".repeat(60), "*a".repeat(12)), true),
        (format!("'{}b' matches '{}b'", "a".repeat(60), "*a".repeat(12)), true),
        (format!("'{}' matches '{}c'", "ab".repeat(40), "*a*b".repeat(10)), false),
        (format!("'{}' matches '{}'", "a".repeat(60), "?*".repeat(30)), true),
        (format!("'{}' matches '{}'", "a".repeat(60), "*?".repeat(61)), false), // 61 `?` need 61 characters
    ];
    for (src, want) in cases {
        r.n += 1;
        let (tx, rx) = mpsc::channel();
        let job = src.clone();
        std::thread::spawn(move || {
            let got = Expr::parse(&job).map(|e| e.eval(&Scope::new()));
            let _ = tx.send(got);
        });
        let short_src = format!("{}…[{} chars]", src.chars().take(60).collect::<String>(), src.chars().count());
        match rx.recv_timeout(Duration::from_secs(30)) {
            Ok(Ok(v)) if v == Value::Bool(want) => {}
            Ok(Ok(v)) => r.bad.push(format!("{short_src}\n      want Bool({want})\n      got  {}", short(&v))),
            Ok(Err(e)) => r.bad.push(format!("{short_src}\n      does not parse: {e}")),
            Err(_) => r
                .bad
                .push(format!("{short_src}\n      no answer in 5 s (exponential backtracking?); want Bool({want})")),
        }
    }
    r
}

#[test]
fn glob_bounded() {
    rows_glob_bounded().done("glob_bounded");
}

// ── the census: how many rows ────────────────────────────────────────────────────────────────

#[test]
fn row_census() {
    let all = [
        ("literals", rows_literals().n),
        ("truthiness", rows_truthiness().n),
        ("logic", rows_logic().n),
        ("paths", rows_paths().n),
        ("pointers", rows_pointers().n),
        ("comparisons", rows_comparisons().n),
        ("membership", rows_membership().n),
        ("matches", rows_matches().n),
        ("len_lower_contains", rows_len_lower_contains().n),
        ("count_same_last", rows_count_same_last().n),
        ("sum", rows_sum().n),
        ("since", rows_since().n),
        ("quote", rows_quote().n),
        ("danger", rows_danger().n),
        ("total", rows_total().n),
        ("env", rows_env().n),
        ("glob_bounded", rows_glob_bounded().n),
    ];
    let total: usize = all.iter().map(|(_, k)| k).sum();
    assert!(total >= 140, "only {total} rows: {all:?}");
}
