//! SYNTAX of Expr: parsing, errors, printing, the lint. Blind tests written from the contract only (the language
//! reference at the top of `src/expr.rs` and the `///` of the public items). Every test fn is one theme: a table of
//! rows, every failing row is collected and the theme asserts once at the end, so one run shows all of them.
//!
//! Row classes, by the comment on the row (or on the table):
//!   plain      the contract says it.
//!   DECIDED  the contract is silent or allows two readings; the row holds the most natural reading and its
//!              failure message starts with `[DECIDED]`.
//!   BUG:       the contract clearly says otherwise (added after a run, with the observed behaviour).

use j4n_core::expr::{Expr, Func, MAX_DEPTH, MAX_LEN, Op, ParseError, Seg};
use j4n_core::value::Value;
use std::panic::{AssertUnwindSafe, catch_unwind};

// ── harness ──────────────────────────────────────────────────────────────────────────────────

struct Rows {
    theme: &'static str,
    rows: usize,
    fails: Vec<String>,
    tag: &'static str,
}

impl Rows {
    fn new(theme: &'static str) -> Rows {
        Rows { theme, rows: 0, fails: Vec::new(), tag: "" }
    }
    /// The rows that follow are DECIDED ones.
    fn ambiguous(&mut self) {
        self.tag = "[DECIDED] ";
    }
    fn firm(&mut self) {
        self.tag = "";
    }
    fn row(&mut self, ok: bool, msg: impl FnOnce() -> String) {
        self.rows += 1;
        if !ok {
            let m = msg();
            self.fails.push(format!("{}{}", self.tag, m));
        }
    }
    fn finish(self) {
        eprintln!("SYNTAX-ROWS {} rows={} failing={}", self.theme, self.rows, self.fails.len());
        let shown: Vec<&String> = self.fails.iter().take(60).collect();
        let more = self.fails.len().saturating_sub(shown.len());
        assert!(
            self.fails.is_empty(),
            "{}: {} of {} rows fail:\n  {}\n{}",
            self.theme,
            self.fails.len(),
            self.rows,
            shown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  "),
            if more > 0 { format!("  … and {more} more") } else { String::new() }
        );
    }
}

fn guarded<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|p| {
        if let Some(s) = p.downcast_ref::<&str>() {
            format!("PANIC: {s}")
        } else if let Some(s) = p.downcast_ref::<String>() {
            format!("PANIC: {s}")
        } else {
            "PANIC".to_string()
        }
    })
}

fn try_parse(s: &str) -> Result<Result<Expr, ParseError>, String> {
    guarded(|| Expr::parse(s))
}

/// An input for a message: quoted, long ones shortened.
fn show(s: &str) -> String {
    let n = s.chars().count();
    if n > 70 {
        let head: String = s.chars().take(40).collect();
        let tail: String = s.chars().skip(n - 12).collect();
        format!("{head:?}…[{n} chars]…{tail:?}")
    } else {
        format!("{s:?}")
    }
}

fn at_ok(e: &ParseError, s: &str) -> bool {
    e.at <= s.chars().count() && !e.msg.trim().is_empty()
}

/// Row: the input is refused, with a message and an `at` that is not past the end.
fn err(t: &mut Rows, s: &str) {
    match try_parse(s) {
        Err(p) => t.row(false, || format!("{}: {p}", show(s))),
        Ok(Ok(e)) => t.row(false, || format!("{}: expected Err, got Ok({e}) = {e:?}", show(s))),
        Ok(Err(pe)) => t.row(at_ok(&pe, s), || {
            format!(
                "{}: Err has at={} msg={:?} (input has {} chars; an `at` past the end or an empty message is wrong)",
                show(s),
                pe.at,
                pe.msg,
                s.chars().count()
            )
        }),
    }
}

/// Row: the input is accepted.
fn ok(t: &mut Rows, s: &str) -> Option<Expr> {
    match try_parse(s) {
        Err(p) => {
            t.row(false, || format!("{}: {p}", show(s)));
            None
        }
        Ok(Err(pe)) => {
            t.row(false, || format!("{}: expected Ok, got Err(at={}, {:?})", show(s), pe.at, pe.msg));
            None
        }
        Ok(Ok(e)) => {
            t.row(true, String::new);
            Some(e)
        }
    }
}

/// A list of literals may be kept as `Lit(Value::List)` or as `Expr::List` of literals: the module doc names both
/// kinds of node without saying which a literal list reads as. Trees are compared through this, except in the rows
/// that ask that question on purpose (`tree_exact`).
fn norm(e: Expr) -> Expr {
    fn lit_list(vs: Vec<Value>) -> Expr {
        Expr::List(
            vs.into_iter()
                .map(|v| match v {
                    Value::List(inner) => lit_list(inner),
                    other => Expr::Lit(other),
                })
                .collect(),
        )
    }
    fn seg(s: Seg) -> Seg {
        match s {
            Seg::Index(e) => Seg::Index(norm(e)),
            k => k,
        }
    }
    match e {
        Expr::Lit(Value::List(vs)) => lit_list(vs),
        Expr::List(items) => Expr::List(items.into_iter().map(norm).collect()),
        Expr::Path { root, segs } => Expr::Path { root, segs: segs.into_iter().map(seg).collect() },
        Expr::Get { base, segs } => {
            Expr::Get { base: Box::new(norm(*base)), segs: segs.into_iter().map(seg).collect() }
        }
        Expr::Call { func, args } => Expr::Call { func, args: args.into_iter().map(norm).collect() },
        Expr::Not(x) => Expr::Not(Box::new(norm(*x))),
        Expr::And(v) => Expr::And(v.into_iter().map(norm).collect()),
        Expr::Or(v) => Expr::Or(v.into_iter().map(norm).collect()),
        Expr::Cmp { op, lhs, rhs } => Expr::Cmp { op, lhs: Box::new(norm(*lhs)), rhs: Box::new(norm(*rhs)) },
        other => other,
    }
}

/// Row: the input reads as this tree (a literal list may be either kind of node).
fn tree(t: &mut Rows, s: &str, want: Expr) {
    match try_parse(s) {
        Err(p) => t.row(false, || format!("{}: {p}", show(s))),
        Ok(Err(pe)) => t.row(false, || format!("{}: expected {want:?}, got Err(at={}, {:?})", show(s), pe.at, pe.msg)),
        Ok(Ok(e)) => t.row(norm(e.clone()) == norm(want.clone()), || {
            format!("{}: expected {want:?}\n      got      {e:?}", show(s))
        }),
    }
}

/// Row: the input reads as exactly this tree, node for node.
fn tree_exact(t: &mut Rows, s: &str, want: Expr) {
    match try_parse(s) {
        Err(p) => t.row(false, || format!("{}: {p}", show(s))),
        Ok(Err(pe)) => t.row(false, || format!("{}: expected {want:?}, got Err(at={}, {:?})", show(s), pe.at, pe.msg)),
        Ok(Ok(e)) => t.row(e == want, || format!("{}: expected {want:?}\n      got      {e:?}", show(s))),
    }
}

/// Row: both read, as the same tree.
fn same(t: &mut Rows, a: &str, b: &str) {
    match (try_parse(a), try_parse(b)) {
        (Ok(Ok(x)), Ok(Ok(y))) => t.row(norm(x.clone()) == norm(y.clone()), || {
            format!("{} vs {}: trees differ\n      {x:?}\n      {y:?}", show(a), show(b))
        }),
        (ra, rb) => t.row(false, || {
            format!("{} vs {}: both must parse, got {} / {}", show(a), show(b), describe(&ra), describe(&rb))
        }),
    }
}

/// Row: both read, as different trees.
fn differ(t: &mut Rows, a: &str, b: &str) {
    match (try_parse(a), try_parse(b)) {
        (Ok(Ok(x)), Ok(Ok(y))) => t.row(norm(x.clone()) != norm(y.clone()), || {
            format!("{} vs {}: both read as {x:?}, they should differ", show(a), show(b))
        }),
        (ra, rb) => t.row(false, || {
            format!("{} vs {}: both must parse, got {} / {}", show(a), show(b), describe(&ra), describe(&rb))
        }),
    }
}

fn describe(r: &Result<Result<Expr, ParseError>, String>) -> String {
    match r {
        Err(p) => p.clone(),
        Ok(Err(pe)) => format!("Err(at={}, {:?})", pe.at, pe.msg),
        Ok(Ok(e)) => format!("Ok({e})"),
    }
}

/// parse(display(parse(s))) == parse(s), and printing is stable.
fn rt(s: &str) -> Result<(), String> {
    let e = Expr::parse(s).map_err(|pe| format!("input does not parse: at={} msg={:?}", pe.at, pe.msg))?;
    let printed = e.to_string();
    let e2 = Expr::parse(&printed)
        .map_err(|pe| format!("prints as {printed:?}, which does not parse: at={} msg={:?}", pe.at, pe.msg))?;
    if e2 != e {
        return Err(format!(
            "prints as {printed:?}, which reads back as a different tree\n      {e2:?}\n      was {e:?}"
        ));
    }
    let again = e2.to_string();
    if again != printed {
        return Err(format!("printing is not stable: {printed:?} then {again:?}"));
    }
    Ok(())
}

fn round_trip(t: &mut Rows, s: &str) {
    match guarded(|| rt(s)) {
        Ok(Ok(())) => t.row(true, String::new),
        Ok(Err(m)) => t.row(false, || format!("{}: {m}", show(s))),
        Err(p) => t.row(false, || format!("{}: {p}", show(s))),
    }
}

// tree builders
fn nm(root: &str) -> Expr {
    Expr::Path { root: root.to_string(), segs: vec![] }
}
fn path(root: &str, segs: Vec<Seg>) -> Expr {
    Expr::Path { root: root.to_string(), segs }
}
fn key(k: &str) -> Seg {
    Seg::Key(k.to_string())
}
fn idx(e: Expr) -> Seg {
    Seg::Index(e)
}
fn num(x: f64) -> Expr {
    Expr::Lit(Value::Num(x))
}
fn txt(s: &str) -> Expr {
    Expr::Lit(Value::Str(s.to_string()))
}
fn dur(ms: u64) -> Expr {
    Expr::Lit(Value::Dur(ms))
}
fn usd(micro: i64) -> Expr {
    Expr::Lit(Value::Usd(micro))
}
fn boo(b: bool) -> Expr {
    Expr::Lit(Value::Bool(b))
}
fn ptr(s: &str) -> Expr {
    Expr::Pointer(s.to_string())
}
fn cmp(op: Op, l: Expr, r: Expr) -> Expr {
    Expr::Cmp { op, lhs: Box::new(l), rhs: Box::new(r) }
}
fn nt(e: Expr) -> Expr {
    Expr::Not(Box::new(e))
}
fn call(func: Func, args: Vec<Expr>) -> Expr {
    Expr::Call { func, args }
}
fn get(base: Expr, segs: Vec<Seg>) -> Expr {
    Expr::Get { base: Box::new(base), segs }
}

// nesting builders: `n` levels of one construct around a leaf
fn parens(n: usize) -> String {
    format!("{}a{}", "(".repeat(n), ")".repeat(n))
}
fn brackets(n: usize) -> String {
    format!("{}1{}", "[".repeat(n), "]".repeat(n))
}
fn calls(n: usize) -> String {
    format!("{}x{}", "len(".repeat(n), ")".repeat(n))
}
fn indexes(n: usize) -> String {
    let mut s = "0".to_string();
    for _ in 0..n {
        s = format!("a[{s}]");
    }
    s
}
fn nots(n: usize) -> String {
    format!("{}a", "not ".repeat(n))
}
fn mixed(n: usize) -> String {
    let mut s = "a".to_string();
    for i in 0..n {
        s = match i % 4 {
            0 => format!("({s})"),
            1 => format!("[{s}]"),
            2 => format!("len({s})"),
            _ => format!("not {s}"),
        };
    }
    s
}

type Shape = (&'static str, fn(usize) -> String);
const SHAPES: [Shape; 6] = [
    ("parens", parens),
    ("brackets", brackets),
    ("calls", calls),
    ("indexes", indexes),
    ("nots", nots),
    ("mixed", mixed),
];

// ── 1. precedence and grouping ───────────────────────────────────────────────────────────────

#[test]
fn precedence_and_grouping() {
    let mut t = Rows::new("precedence_and_grouping");

    // parse(a) == parse(b)
    let same_rows: &[(&str, &str)] = &[
        // or < and < not < comparison
        ("a or b and c", "a or (b and c)"),
        ("a and b or c", "(a and b) or c"),
        ("a or b and c or d", "a or (b and c) or d"),
        ("a and b or c and d", "(a and b) or (c and d)"),
        ("a or b and c and d or e", "a or (b and c and d) or e"),
        ("not a == b", "not (a == b)"),
        ("not a != b", "not (a != b)"),
        ("not a < b", "not (a < b)"),
        ("not a <= b", "not (a <= b)"),
        ("not a > b", "not (a > b)"),
        ("not a >= b", "not (a >= b)"),
        ("not a in b", "not (a in b)"),
        ("not a matches b", "not (a matches b)"),
        ("not a and b", "(not a) and b"),
        ("not a or b", "(not a) or b"),
        ("a and not b", "a and (not b)"),
        ("a or not b and c", "a or ((not b) and c)"),
        ("not a or not b and not c", "(not a) or ((not b) and (not c))"),
        ("not not a", "not (not a)"),
        ("not not not a", "not (not (not a))"),
        ("not not not a == b", "not (not (not (a == b)))"),
        ("x not in y", "not (x in y)"),
        ("x not in y and z", "not (x in y) and z"),
        ("a not in b or c not in d", "not (a in b) or not (c in d)"),
        ("a == b and c == d", "(a == b) and (c == d)"),
        ("a == b or c != d and e", "(a == b) or ((c != d) and e)"),
        ("a in b or c", "(a in b) or c"),
        ("a in b and c in d", "(a in b) and (c in d)"),
        ("a matches 'x*' and b", "(a matches 'x*') and b"),
        ("a < b and b < c", "(a < b) and (b < c)"),
        ("a < b or b < c and c < d", "(a < b) or ((b < c) and (c < d))"),
        // spacing around words and operators
        ("a not  in b", "a not in b"),
        ("a not\tin b", "a not in b"),
        ("a not\nin b", "a not in b"),
        ("a==b", "a == b"),
        ("a!=b", "a != b"),
        ("a<b", "a < b"),
        ("a<=b", "a <= b"),
        ("a>b", "a > b"),
        ("a>=b", "a >= b"),
        ("a ==b", "a == b"),
        ("a== b", "a == b"),
        ("a\t==\nb", "a == b"),
        ("a\tand\nb", "a and b"),
        ("a\r\nor b", "a or b"),
        ("\n a \n", "a"),
        ("  (  a  )  ", "a"),
        ("len( x )", "len(x)"),
        ("count( xs , 1 )", "count(xs,1)"),
        ("[ 1 , 2 ]", "[1,2]"),
        ("a[ 0 ]", "a[0]"),
        ("a[\n0\n]", "a[0]"),
        ("not(a)", "not (a)"),
        ("a and(b)", "a and (b)"),
        ("a or(b)", "a or (b)"),
        ("(a)and(b)", "(a) and (b)"),
        ("(a)or(b)", "(a) or (b)"),
        ("a in'x'", "a in 'x'"),
        ("a in[1]", "a in [1]"),
        ("a matches'x*'", "a matches 'x*'"),
        ("not'a'", "not 'a'"),
        ("a and'b'", "a and 'b'"),
        ("a and\"b\"", "a and \"b\""),
        ("1<2", "1 < 2"),
        ("-3<-2", "-3 < -2"),
        ("a>=-1", "a >= -1"),
        ("a==-1", "a == -1"),
        ("5m<10m", "5m < 10m"),
        ("$1<$2", "$1 < $2"),
        ("len(a)>3", "len(a) > 3"),
        ("a.b==c.d", "a.b == c.d"),
        // redundant parentheses make no node
        ("(a)", "a"),
        ("((a))", "a"),
        ("(((a)))", "a"),
        ("(a == b)", "a == b"),
        ("((a == b))", "a == b"),
        ("(a and b)", "a and b"),
        ("(a) and (b)", "a and b"),
        ("(a) and (b) and (c)", "a and b and c"),
        ("(a) or (b)", "a or b"),
        ("not ((a))", "not a"),
        ("(not a)", "not a"),
        ("(1)", "1"),
        ("('x')", "'x'"),
        ("(a.b)", "a.b"),
        ("([1])", "[1]"),
        ("(len(x))", "len(x)"),
        ("((a and b))", "a and b"),
        ("(a or b) and (c)", "(a or b) and c"),
        // literals are values, not spellings
        ("'a'", "\"a\""),
        (r#"'it\'s'"#, r#""it's""#),
        (r#""say \"hi\"""#, r#"'say "hi"'"#),
        (r#"'\"'"#, r#""\"""#),
        ("0.50", "0.5"),
        ("1.0", "1"),
        ("-3.0", "-3"),
        ("90s", "1m30s"),
        ("1.5h", "1h30m"),
        ("1000ms", "1s"),
        ("60s", "1m"),
        ("24h", "1d"),
        ("0.5m", "30s"),
        ("1h30m", "90m"),
        ("1500ms", "1.5s"),
        ("0.1s", "100ms"),
        ("0.001s", "1ms"),
        ("2.5s", "2500ms"),
        ("0.25m", "15s"),
        ("1.1h", "66m"),
        ("$1.5", "$1.50"),
        ("$1", "$1.0"),
        ("$0.5", "$0.500000"),
        ("$2", "$2.000"),
        // pointers end at anything that is not a pointer character
        ("/type=='text'", "/type == 'text'"),
        ("/a==/b", "/a == /b"),
        ("/a<=/b", "/a <= /b"),
        ("/a!=/b", "/a != /b"),
        ("/a>/b", "/a > /b"),
        ("(/a)", "/a"),
        ("[/a,/b]", "[/a, /b]"),
        ("len(/a)", "len( /a )"),
        ("/a/b[0]", "/a/b[ 0 ]"),
        ("/x in[1]", "/x in [1]"),
        ("a and/x", "a and /x"),
    ];
    for (a, b) in same_rows {
        same(&mut t, a, b);
    }

    // parse(a) != parse(b), both read
    let differ_rows: &[(&str, &str)] = &[
        ("a or b and c", "(a or b) and c"),
        ("a and b or c", "a and (b or c)"),
        ("not a and b", "not (a and b)"),
        ("not a or b", "not (a or b)"),
        ("not a == b", "not a == c"),
        ("a and b", "a or b"),
        ("a == b", "a != b"),
        ("a < b", "a <= b"),
        ("a > b", "a >= b"),
        ("a in b", "a not in b"),
        ("a in b", "b in a"),
        ("x not in y", "x in y"),
        ("a.b", "a[b]"),
        ("a.b", "a['b']"),
        ("'1'", "1"),
        ("5m", "5"),
        ("$1", "1"),
        ("true", "'true'"),
        ("[1,2]", "[2,1]"),
        ("a and b and c", "a and b"),
        ("a or b", "b or a"),
        ("5m", "5s"),
        ("60s", "61s"),
        ("$1.50", "$1.51"),
        ("/a/b", "/a/c"),
        ("/a", "a"),
        ("len(x)", "lower(x)"),
        ("count(xs)", "count(xs, 1)"),
        ("a[0]", "a['0']"),
        ("a[b]", "a[c]"),
        ("last(a).b", "last(a)[b]"),
        ("not not a", "a"),
    ];
    for (a, b) in differ_rows {
        differ(&mut t, a, b);
    }

    // DECIDED: the grammar has no production for parentheses at all, and says nothing about n-ary
    // and/or with nested groups, parentheses as a comparison operand, or space before `.`, `[`, `(`.
    t.ambiguous();
    let ambiguous_same: &[(&str, &str)] = &[
        ("a or b or c", "(a or b) or c"), // flattened or nested?
        ("a or b or c", "a or (b or c)"),
        ("a and b and c", "(a and b) and c"),
        ("a and b and c", "a and (b and c)"),
        ("(a) == (b)", "a == b"), // a group as an operand of a comparison
        ("(a) < 3", "a < 3"),
        ("a == (b)", "a == b"),
        ("(a.b) == c", "a.b == c"),
        ("(not a) == b", "(not a) == b"),
        ("a . b", "a.b"), // space before `.`, `[`, `(`
        ("a. b", "a.b"),
        ("a [0]", "a[0]"),
        ("len (x)", "len(x)"),
        ("last(x) .a", "last(x).a"),
        ("last(x) [0]", "last(x)[0]"),
    ];
    for (a, b) in ambiguous_same {
        same(&mut t, a, b);
    }
    differ(&mut t, "(not a) == b", "not a == b"); // only if `(not a) == b` parses at all
    t.finish();
}

// ── 2. the shape of the tree ─────────────────────────────────────────────────────────────────

#[test]
fn tree_shapes() {
    let mut t = Rows::new("tree_shapes");
    let rows: Vec<(&str, Expr)> = vec![
        ("a", nm("a")),
        ("event.data.cmd", path("event", vec![key("data"), key("cmd")])),
        ("fold.spend[task.id]", path("fold", vec![key("spend"), idx(path("task", vec![key("id")]))])),
        ("a[0]", path("a", vec![idx(num(0.0))])),
        ("a['k']", path("a", vec![idx(txt("k"))])),
        ("a[\"k\"]", path("a", vec![idx(txt("k"))])),
        ("a.b[c].d", path("a", vec![key("b"), idx(nm("c")), key("d")])),
        ("a[0][1]", path("a", vec![idx(num(0.0)), idx(num(1.0))])),
        ("a[b[c]]", path("a", vec![idx(path("b", vec![idx(nm("c"))]))])),
        ("/usage/cost_usd", ptr("/usage/cost_usd")),
        ("/content/0/text", ptr("/content/0/text")),
        ("/type=='text'", cmp(Op::Eq, ptr("/type"), txt("text"))),
        ("/a[0]", get(ptr("/a"), vec![idx(num(0.0))])),
        ("/a[0].b", get(ptr("/a"), vec![idx(num(0.0)), key("b")])),
        ("true", boo(true)),
        ("false", boo(false)),
        ("12", num(12.0)),
        ("-3", num(-3.0)),
        ("0.5", num(0.5)),
        ("'a'", txt("a")),
        ("5m", dur(300_000)),
        ("250ms", dur(250)),
        ("1h30m", dur(5_400_000)),
        ("$1.50", usd(1_500_000)),
        ("$0.0004", usd(400)),
        ("not a", nt(nm("a"))),
        ("not not a", nt(nt(nm("a")))),
        ("a and b", Expr::And(vec![nm("a"), nm("b")])),
        ("a or b", Expr::Or(vec![nm("a"), nm("b")])),
        ("a and b and c", Expr::And(vec![nm("a"), nm("b"), nm("c")])),
        ("a or b or c", Expr::Or(vec![nm("a"), nm("b"), nm("c")])),
        ("a or b and c", Expr::Or(vec![nm("a"), Expr::And(vec![nm("b"), nm("c")])])),
        ("a and b or c", Expr::Or(vec![Expr::And(vec![nm("a"), nm("b")]), nm("c")])),
        ("not a and b", Expr::And(vec![nt(nm("a")), nm("b")])),
        ("not a == b", nt(cmp(Op::Eq, nm("a"), nm("b")))),
        ("a == b and c", Expr::And(vec![cmp(Op::Eq, nm("a"), nm("b")), nm("c")])),
        ("len(x)", call(Func::Len, vec![nm("x")])),
        ("lower(x)", call(Func::Lower, vec![nm("x")])),
        ("contains(h, n)", call(Func::Contains, vec![nm("h"), nm("n")])),
        ("count(xs)", call(Func::Count, vec![nm("xs")])),
        ("count(xs, 1)", call(Func::Count, vec![nm("xs"), num(1.0)])),
        ("sum(xs)", call(Func::Sum, vec![nm("xs")])),
        ("last(xs)", call(Func::Last, vec![nm("xs")])),
        ("same(xs)", call(Func::Same, vec![nm("xs")])),
        ("since(ts)", call(Func::Since, vec![nm("ts")])),
        ("quote(x)", call(Func::Quote, vec![nm("x")])),
        ("danger(cmd)", call(Func::Danger, vec![nm("cmd")])),
        ("last(xs).cmd", get(call(Func::Last, vec![nm("xs")]), vec![key("cmd")])),
        ("last(xs)[0]", get(call(Func::Last, vec![nm("xs")]), vec![idx(num(0.0))])),
        ("last(xs).a[0].b", get(call(Func::Last, vec![nm("xs")]), vec![key("a"), idx(num(0.0)), key("b")])),
        ("len(len(x))", call(Func::Len, vec![call(Func::Len, vec![nm("x")])])),
        ("count(xs, a == 1)", call(Func::Count, vec![nm("xs"), cmp(Op::Eq, nm("a"), num(1.0))])),
        ("count(xs, not a)", call(Func::Count, vec![nm("xs"), nt(nm("a"))])),
        ("len(x) > 3", cmp(Op::Gt, call(Func::Len, vec![nm("x")]), num(3.0))),
        ("a in [1, 2]", cmp(Op::In, nm("a"), Expr::List(vec![num(1.0), num(2.0)]))),
        ("a matches 'x*'", cmp(Op::Matches, nm("a"), txt("x*"))),
        ("a in b", cmp(Op::In, nm("a"), nm("b"))),
        ("1 == a", cmp(Op::Eq, num(1.0), nm("a"))),
        ("'a' in b", cmp(Op::In, txt("a"), nm("b"))),
        ("x not in y", nt(cmp(Op::In, nm("x"), nm("y")))),
        ("-3 in xs", cmp(Op::In, num(-3.0), nm("xs"))),
    ];
    for (s, want) in rows {
        tree(&mut t, s, want);
    }
    // every comparison word and symbol
    let ops = [
        ("==", Op::Eq),
        ("!=", Op::Ne),
        ("<", Op::Lt),
        ("<=", Op::Le),
        (">", Op::Gt),
        (">=", Op::Ge),
        ("in", Op::In),
        ("matches", Op::Matches),
    ];
    for (w, op) in ops {
        tree(&mut t, &format!("a {w} b"), cmp(op, nm("a"), nm("b")));
        tree(&mut t, &format!("x.y {w} 'z'"), cmp(op, path("x", vec![key("y")]), txt("z")));
    }
    for (w, op) in ops.iter().take(6) {
        tree(&mut t, &format!("a{w}b"), cmp(*op, nm("a"), nm("b")));
    }

    // DECIDED: is a list literal an `Expr::List` of literals, or folded into one `Lit(Value::List)`? Does
    // `/` alone keep its slash? Is the empty text key `""` of `/` kept?
    t.ambiguous();
    tree_exact(&mut t, "[]", Expr::List(vec![]));
    tree_exact(&mut t, "[1, 2]", Expr::List(vec![num(1.0), num(2.0)]));
    tree_exact(&mut t, "[a, 'x']", Expr::List(vec![nm("a"), txt("x")]));
    tree_exact(&mut t, "/", ptr(""));
    t.finish();
}

// ── 3. names and reserved words ──────────────────────────────────────────────────────────────

#[test]
fn names_and_reserved_words() {
    let mut t = Rows::new("names_and_reserved_words");
    // names: [A-Za-z_][A-Za-z0-9_]*; the reserved words are lower case and only whole words
    for name in [
        "a", "abc", "A", "_", "_x", "x1", "x_1", "a1b2", "Z9", "__", "andy", "android", "order", "orb", "notes",
        "nothing", "not_a", "input", "inside", "in_x", "matchesx", "matches_", "trueish", "true1", "falsey", "_true",
        "True", "FALSE", "None", "null", "AND", "OR", "NOT", "IN", "Matches", "ainb", "aorb", "xnot",
    ] {
        tree(&mut t, name, nm(name));
    }
    tree(&mut t, "a_b.c_d[e_f]", path("a_b", vec![key("c_d"), idx(nm("e_f"))]));
    tree(&mut t, "x1.y2", path("x1", vec![key("y2")]));
    tree(&mut t, "_a._b", path("_a", vec![key("_b")]));
    tree(&mut t, "a['and']", path("a", vec![idx(txt("and"))])); // a reserved word as text is fine

    // the reserved words cannot be names
    for w in ["and", "or", "not", "in", "matches"] {
        err(&mut t, w);
        if w == "in" {
            // `in` is also the root of a flow's inputs where a value is due
            tree(&mut t, "in.x", path("in", vec![key("x")]));
        } else {
            err(&mut t, &format!("{w}.x"));
        }
        err(&mut t, &format!("{w} == 1"));
        err(&mut t, &format!("1 == {w}"));
        err(&mut t, &format!("[{w}]"));
        err(&mut t, &format!("len({w})"));
        if w != "not" {
            // `not [0]` and `not(x)` are a `not` of a list and of a group
            if w != "in" {
                err(&mut t, &format!("{w}[0]"));
            }
            err(&mut t, &format!("{w}(x)"));
        }
    }
    for s in [
        "a and",
        "and a",
        "a or",
        "or a",
        "not",
        "a not",
        "not not",
        "a in",
        "in a",
        "a matches",
        "matches a",
        "a and and b",
        "a or or b",
        "a and or b",
        "a or and b",
        "not and a",
        "a andb",
        "a inb",
        "a notin b",
        "a orb",
        "a matchesb",
        "aand b",
        "a not matches 'x'",
        "a not == b",
        "a not < b",
        "a not not in b",
        "a not in",
        "not in a",
        "a in b not",
        "true(1)",
        "false()",
        "1.and",
    ] {
        err(&mut t, s);
    }
    // names with characters outside [A-Za-z0-9_]
    for s in [
        "1a",
        "9",
        "a-b",
        "a b",
        "é",
        "naïve",
        "ñ",
        "日本",
        "a.é",
        "a$",
        "a$b",
        "a@b",
        "a:b",
        "a'b",
        "a\"b",
        "a#b",
        "a`b",
        "a\\b",
        "a?b",
        "a~b",
        "a^b",
        "a*b",
        "a+b",
        "a%b",
        "a;b",
        "a{b}",
        "😀",
        "a\u{0}b",
        "\u{0}",
        "x²",
        "a\u{661}",
        "\u{661}\u{662}",
        "²",
        "½",
        "５",
        "ａ",
        "ⅷ",
        "a\u{ff11}",
        "Ω",
        "ß",
        "a_é",
        "_é",
        "x.ñ",
        "a[é]",
        "len(é)",
        "[é]",
    ] {
        if s == "9" {
            ok(&mut t, s); // a number, not a name
        } else {
            err(&mut t, s);
        }
    }

    // DECIDED: `name` is the same production in `a.name`, so the reserved words are refused after a dot too; JSON
    // keys named `in`, `or`, `not` are common, so the other reading is plausible. And the function words are not
    // reserved, so a bare `len` is a root.
    t.ambiguous();
    for w in ["and", "or", "not", "in", "matches", "true", "false"] {
        ok(&mut t, &format!("x.{w}"));
        ok(&mut t, &format!("x.y.{w}"));
    }
    // and a literal word is a literal: nothing follows it
    for s in ["true.x", "false[0]", "true[0]", "false.x.y"] {
        err(&mut t, s);
    }
    for f in ["len", "count", "last", "sum", "same", "since", "lower", "contains", "quote", "danger"] {
        tree(&mut t, f, nm(f));
        tree(&mut t, &format!("{f}.x"), path(f, vec![key("x")]));
    }
    t.finish();
}

// ── 4. pointers ──────────────────────────────────────────────────────────────────────────────

#[test]
fn pointers() {
    let mut t = Rows::new("pointers");
    // letters, digits and `_-.:~@$%+/` make a pointer; it ends at anything else
    for p in [
        "/usage/cost_usd",
        "/content/0/text",
        "/a",
        "/a/b-c",
        "/a.b",
        "/a:b",
        "/a~0b",
        "/a~1b",
        "/a@b",
        "/a$b",
        "/a%20b",
        "/a+b",
        "/A_9",
        "/a/b/",
        "//",
        "/a//b",
        "/0",
        "/-",
        "/_",
        "/a.b.c.d",
        "/a-b_c.d:e~f@g$h%i+j",
    ] {
        tree(&mut t, p, ptr(p));
    }
    ok(&mut t, "/");
    ok(&mut t, "/ == 1");
    ok(&mut t, "( / )");
    ok(&mut t, "[/]");
    ok(&mut t, "len(/)");
    ok(&mut t, "not /");
    let rows: Vec<(&str, Expr)> = vec![
        ("/a == 1", cmp(Op::Eq, ptr("/a"), num(1.0))),
        ("/a==1", cmp(Op::Eq, ptr("/a"), num(1.0))),
        ("/a!=1", cmp(Op::Ne, ptr("/a"), num(1.0))),
        ("/a<1", cmp(Op::Lt, ptr("/a"), num(1.0))),
        ("/a<=1", cmp(Op::Le, ptr("/a"), num(1.0))),
        ("/a>1", cmp(Op::Gt, ptr("/a"), num(1.0))),
        ("/a>=1", cmp(Op::Ge, ptr("/a"), num(1.0))),
        ("/a in [1]", cmp(Op::In, ptr("/a"), Expr::List(vec![num(1.0)]))),
        ("/a in'x'", cmp(Op::In, ptr("/a"), txt("x"))),
        ("/x==/y", cmp(Op::Eq, ptr("/x"), ptr("/y"))),
        ("/x/y=='a b'", cmp(Op::Eq, ptr("/x/y"), txt("a b"))),
        ("not /a", nt(ptr("/a"))),
        ("/a and /b", Expr::And(vec![ptr("/a"), ptr("/b")])),
        ("/a or /b", Expr::Or(vec![ptr("/a"), ptr("/b")])),
        ("len(/a)", call(Func::Len, vec![ptr("/a")])),
        ("count(/items, 1)", call(Func::Count, vec![ptr("/items"), num(1.0)])),
        ("(/a)", ptr("/a")),
        ("/a/b[c]", get(ptr("/a/b"), vec![idx(nm("c"))])),
        ("/a[0][1]", get(ptr("/a"), vec![idx(num(0.0)), idx(num(1.0))])),
        ("/a['k'].b", get(ptr("/a"), vec![idx(txt("k")), key("b")])),
        ("a[/p]", path("a", vec![idx(ptr("/p"))])),
        ("a[/p/q].b", path("a", vec![idx(ptr("/p/q")), key("b")])),
    ];
    for (s, want) in rows {
        tree(&mut t, s, want);
    }
    for s in [
        "/a#", "/a*b", "/a!", "/a b", "/a /b", "a /b", "a/b", "1/a", "/a'x'", "/a\"", "/a?b", "/a{b}", "/a(b)", "/a|b",
        "/a&b", "/a;b", "/a,b", "/a\\b", "/a=b", "/a=", "/a<>b", "x /y", "x / y", "x /", "/ a",
        "/a ", // trailing space is fine
    ] {
        if s == "/a " {
            ok(&mut t, s);
        } else {
            err(&mut t, s);
        }
    }
    t.finish();
}

// ── 5. lexical errors ────────────────────────────────────────────────────────────────────────

#[test]
fn lexical_errors() {
    let mut t = Rows::new("lexical_errors");
    let rows: Vec<&str> = vec![
        // a single `=`
        "a = b",
        "=",
        "a =",
        "= a",
        "a == b = c",
        "a=b",
        "a === b",
        "a =< b",
        "a => b",
        "a< =b",
        "a=!b",
        "a! =b",
        "a<==b",
        // `!`
        "!",
        "!a",
        "a ! b",
        "a !",
        "!(a)",
        "a !! b",
        "a !== b",
        "!true",
        "a!b",
        "a !b",
        // `&`
        "&",
        "a & b",
        "a && b",
        "a&b",
        "&&",
        "a &",
        // `|`
        "|",
        "a | b",
        "a || b",
        "a|b",
        "||",
        "a |",
        // other characters that are not part of the language
        "#",
        "@",
        "`",
        "\\",
        ";",
        "?",
        ":",
        "^",
        "*",
        "+",
        "%",
        "{",
        "}",
        "{x}",
        "~",
        "§",
        "→",
        "a ~ b",
        "a ; b",
        "a ? b : c",
        "a + b",
        "a * b",
        "a % b",
        "a ^ b",
        "a - b",
        "a <> b",
        "a # comment",
        "a // b",
        "a /* b */",
        "a, b",
        ", a",
        "a: b",
        // unknown escapes in text
        r#"'\q'"#,
        r#"'\0'"#,
        r#"'\x41'"#,
        r#"'\u0041'"#,
        r#""\a""#,
        r#"'\/'"#,
        r#"'\ '"#,
        r#"'a\sb'"#,
        r#"'\b'"#,
        r#"'\f'"#,
        r#"'\v'"#,
        r#"'\`'"#,
        r#"'\é'"#,
        r#""\w""#,
        r#"'ok \d'"#,
        r#"'\1'"#,
        // unterminated text
        "'abc",
        "\"abc",
        "'",
        "\"",
        r#"'abc""#,
        r#""abc'"#,
        "a == 'x",
        "len('x)",
        "['a, 'b']",
        r#"'abc\"#,
        r#"'\'"#,
        r#""\""#,
        r#"'a\'b"#,
        "'''",
        "\"\"\"",
        "a == \"",
        "'a' == 'b",
        "'é",
        "\"é",
        // durations: unit missing, unknown, in the wrong case, or the second part without one
        "5x",
        "5X",
        "5sec",
        "5min",
        "5hr",
        "5w",
        "5y",
        "5us",
        "5ns",
        "5M",
        "5S",
        "5H",
        "5D",
        "5Ms",
        "5mss",
        "5mh",
        "5ds",
        "1h30",
        "1h30x",
        "1h30m15",
        "1m30",
        "5m5",
        "5s5",
        "1d12",
        "1h30m5",
        "1h-30m",
        "1h30m-5s",
        "5.s",
        "5.5.5s",
        "1h5",
        "2d5",
        "1h30mx",
        "5msec",
        "5 s",
        // negative durations
        "-5s",
        "-1h30m",
        "-250ms",
        "-1d",
        "-0.5s",
        "a == -5s",
        "[-5m]",
        // a bare $, and money without an amount
        "$",
        "$x",
        "$ 1",
        "$$1",
        "$.",
        "$1.2.3",
        "$1e3",
        "$1x",
        "$5m",
        "a$",
        "a $",
        "$ ",
        "1$",
        "$1$2",
        "$a",
        "$,",
        "$)",
        "($)",
        "$1,000",
        "$1_000",
        // numbers
        "1.2.3",
        "1..2",
        "1_000",
        "0x10",
        "1e3",
        "1.5e3",
        "1e-3",
        "1E3",
        "--3",
        "+3",
        "3-",
        "-",
        "- 3",
        "-a",
        "-(3)",
        "-.5",
        "1 + 2",
        "1-2",
        "a-1",
        "a -1",
        "1 -1",
        "3 - 2",
    ];
    for s in &rows {
        err(&mut t, s);
    }
    // numbers that are not finite
    let big = |digits: usize| format!("1{}", "0".repeat(digits));
    for s in [
        big(400),
        big(309),
        format!("9{}", "0".repeat(308)),
        "9".repeat(400),
        format!("-{}", big(400)),
        format!("[{}]", big(500)),
        format!("a == {}", "9".repeat(350)),
    ] {
        err(&mut t, &s);
    }
    // finite ones are fine
    for s in [
        big(307),
        big(308),
        format!("0.{}", "9".repeat(400)),
        format!("-{}", big(307)),
        format!("0.{}1", "0".repeat(300)),
    ] {
        ok(&mut t, &s);
    }
    // durations and money that cannot be held
    for s in [
        "99999999999999999999999d".to_string(),
        format!("1{}ms", "0".repeat(30)),
        "18446744073709551616ms".to_string(),
        "1000000000000000d".to_string(),
        format!("{}d", "9".repeat(400)),
        format!("${}", "9".repeat(30)),
        format!("${}", "9".repeat(400)),
        "$9223372036855".to_string(),
        "$99999999999999999999".to_string(),
    ] {
        err(&mut t, &s);
    }
    // DECIDED: the grammar gives numbers as `12 | -3 | 0.5`, nothing on `5.`, `.5`, `$.5`, `$1.`, `$-1`, `-$1`;
    // a duration above ~31 years and the largest money are not stated either.
    t.ambiguous();
    for s in ["5.", ".5", "$.5", "$1.", "$-1", "-$1", "$ 1.5", "5.m", "1h.5m"] {
        err(&mut t, s);
    }
    err(&mut t, "10000000000000000ms"); // durations stop at 1e15 ms
    err(&mut t, "100000000d");
    err(&mut t, "$9223372036854"); // money stops below 9223372036854 whole dollars
    t.finish();
}

// ── 6. literals: numbers and text ────────────────────────────────────────────────────────────

#[test]
fn literals_numbers_and_text() {
    let mut t = Rows::new("literals_numbers_and_text");
    for (s, v) in [
        ("0", 0.0),
        ("12", 12.0),
        ("007", 7.0),
        ("0.5", 0.5),
        ("-3", -3.0),
        ("-0.5", -0.5),
        ("0.0", 0.0),
        ("0.1", 0.1),
        ("100", 100.0),
        ("12345678901234567890", 12345678901234567890.0),
        ("7.14159", 7.14159),
        ("-12", -12.0),
        ("1.50", 1.5),
        ("1000000", 1000000.0),
    ] {
        if s == "007" {
            continue; // leading zeros: not stated; see the ambiguous block
        }
        tree(&mut t, s, num(v));
    }
    tree(&mut t, "[-1, -2.5]", Expr::List(vec![num(-1.0), num(-2.5)]));
    let tail = "9".repeat(400);
    tree(&mut t, &format!("0.{tail}"), num(1.0)); // 0.999… rounds to 1.0 in f64
    let text_rows: Vec<(&str, &str)> = vec![
        ("'a'", "a"),
        ("\"a\"", "a"),
        ("''", ""),
        ("\"\"", ""),
        (r#"'it\'s'"#, "it's"),
        (r#""say \"hi\"""#, "say \"hi\""),
        (r#"'a\\b'"#, "a\\b"),
        (r#"'\n'"#, "\n"),
        (r#"'\t'"#, "\t"),
        (r#"'\r'"#, "\r"),
        (r#"'\\'"#, "\\"),
        (r#"'\\n'"#, "\\n"),
        (r#"'\"'"#, "\""),
        (r#""\'""#, "'"),
        (r#"'a"b'"#, "a\"b"),
        (r#""a'b""#, "a'b"),
        ("'héllo ✓'", "héllo ✓"),
        ("'😀'", "😀"),
        ("'日本語'", "日本語"),
        ("'{x}'", "{x}"),
        ("'{{'", "{{"),
        ("'}'", "}"),
        ("'${x}'", "${x}"),
        ("'$1.50'", "$1.50"),
        ("'/a/b'", "/a/b"),
        ("'a and b'", "a and b"),
        ("'not'", "not"),
        ("'5m'", "5m"),
        ("'true'", "true"),
        ("' '", " "),
        ("'  a  '", "  a  "),
        (r#"'a\tb\nc'"#, "a\tb\nc"),
        (r#"'\\\\'"#, "\\\\"),
        (r#"'\'\''"#, "''"),
        (r#"'\n\n'"#, "\n\n"),
        (r#"'=='"#, "=="),
        ("'#'", "#"),
        ("'@'", "@"),
        ("'(' ", "("),
        ("'['", "["),
        ("'a,b'", "a,b"),
    ];
    for (s, want) in text_rows {
        tree(&mut t, s, txt(want));
    }
    for s in ["'a' 'b'", "'a''b'", "'a'b", "a'b'", "'a' 1", "1 'a'", "\"a\" \"b\""] {
        err(&mut t, s);
    }
    // text that ends where it should
    tree(&mut t, "'a' == 'b'", cmp(Op::Eq, txt("a"), txt("b")));
    tree(&mut t, "'a'=='b'", cmp(Op::Eq, txt("a"), txt("b")));
    tree(&mut t, "['a','b']", Expr::List(vec![txt("a"), txt("b")]));
    tree(&mut t, "'a' in b", cmp(Op::In, txt("a"), nm("b")));
    tree(&mut t, "a in 'abc'", cmp(Op::In, nm("a"), txt("abc")));
    tree(&mut t, "a matches 'x*'", cmp(Op::Matches, nm("a"), txt("x*")));

    // DECIDED: a raw newline or tab inside quotes; leading zeros; a literal followed by `.name` or `[expr]`
    // (the grammar's `post` lists no literal, yet `x == 5` and `'a' in y` must parse)
    t.ambiguous();
    err(&mut t, "'a'.b");
    err(&mut t, "'a'[0]");
    err(&mut t, "5.b");
    err(&mut t, "[1,2][0]");
    err(&mut t, "[1,2].a");
    tree(&mut t, "'a\nb'", txt("a\nb"));
    tree(&mut t, "'a\tb'", txt("a\tb"));
    tree(&mut t, "007", num(7.0));
    tree(&mut t, "00.5", num(0.5));
    t.finish();
}

// ── 7. literals: durations and money ─────────────────────────────────────────────────────────

#[test]
fn literals_durations_and_money() {
    let mut t = Rows::new("literals_durations_and_money");
    for (s, ms) in [
        ("5m", 300_000u64),
        ("250ms", 250),
        ("1h30m", 5_400_000),
        ("90s", 90_000),
        ("1.5h", 5_400_000),
        ("0s", 0),
        ("0ms", 0),
        ("0m", 0),
        ("1d", 86_400_000),
        ("2d12h", 216_000_000),
        ("1m30s500ms", 90_500),
        ("0.5s", 500),
        ("1500ms", 1500),
        ("1s", 1000),
        ("1h", 3_600_000),
        ("100m", 6_000_000),
        ("1d1h1m1s1ms", 90_061_001),
        ("0.25h", 900_000),
        ("1.25m", 75_000),
        ("2.5s", 2500),
        ("0.001s", 1),
        ("1000d", 86_400_000_000),
        ("1ms", 1),
        ("999ms", 999),
        ("60m", 3_600_000),
        ("1h0m", 3_600_000),
        ("0h30m", 1_800_000),
        ("1.5m30s", 120_000),
        ("12h", 43_200_000),
        ("7d", 604_800_000),
        ("1m1ms", 60_001),
    ] {
        tree(&mut t, s, dur(ms));
    }
    for (s, micro) in [
        ("$1.50", 1_500_000i64),
        ("$0.0004", 400),
        ("$0", 0),
        ("$1", 1_000_000),
        ("$100", 100_000_000),
        ("$0.000001", 1),
        ("$0.5", 500_000),
        ("$12.345678", 12_345_678),
        ("$1000000", 1_000_000_000_000),
        ("$0.10", 100_000),
        ("$0.00", 0),
        ("$0.0", 0),
        ("$3", 3_000_000),
        ("$0.25", 250_000),
        ("$99.99", 99_990_000),
        ("$0.000100", 100),
    ] {
        tree(&mut t, s, usd(micro));
    }
    // inside larger expressions
    tree(&mut t, "since(ts) > 5m", cmp(Op::Gt, call(Func::Since, vec![nm("ts")]), dur(300_000)));
    tree(&mut t, "cost >= $1.50", cmp(Op::Ge, nm("cost"), usd(1_500_000)));
    tree(&mut t, "[5m, $1, 2]", Expr::List(vec![dur(300_000), usd(1_000_000), num(2.0)]));
    tree(&mut t, "5m<10m", cmp(Op::Lt, dur(300_000), dur(600_000)));
    tree(&mut t, "$1<$2", cmp(Op::Lt, usd(1_000_000), usd(2_000_000)));
    tree(&mut t, "a == 0s", cmp(Op::Eq, nm("a"), dur(0)));
    tree(&mut t, "a == $0", cmp(Op::Eq, nm("a"), usd(0)));
    // a duration or money followed by a word or digits is not a duration
    for s in ["5m 5m", "$1 $2", "1h 30m", "1h  30m", "5m x", "5mx", "5m_", "$1.50 5m", "5m5m5", "5 m", "5\tm"] {
        err(&mut t, s);
    }
    // DECIDED: parts in any order, a unit twice, below a millisecond, below a millionth, more than six decimals
    t.ambiguous();
    tree(&mut t, "30m1h", dur(5_400_000));
    tree(&mut t, "1h1h", dur(7_200_000));
    tree(&mut t, "1s1m", dur(61_000));
    ok(&mut t, "0.4ms");
    ok(&mut t, "1.5ms");
    tree(&mut t, "$0.1234567", usd(123_457)); // kept to a millionth: rounded
    tree(&mut t, "$0.1234564", usd(123_456));
    tree(&mut t, "$0.00000049", usd(0));
    t.finish();
}

// ── 8. chained comparison ────────────────────────────────────────────────────────────────────

#[test]
fn comparison_chain_refused() {
    let mut t = Rows::new("comparison_chain_refused");
    for s in [
        "a < b < c",
        "a == b == c",
        "a != b != c",
        "a < b == c",
        "a == b < c",
        "a <= b >= c",
        "a in b in c",
        "a in b == c",
        "a == b in c",
        "a not in b not in c",
        "a not in b in c",
        "a in b not in c",
        "a matches 'x' == b",
        "a == b matches 'x'",
        "a < b < c < d",
        "1 < 2 < 3",
        "a == b == c == d",
        "x in y matches z",
        "a < b != c",
        "a > b > c",
        "a >= b <= c",
        "len(a) < 3 < 5",
        "a.b < c.d < e.f",
        "/a < /b < /c",
        "true == false == true",
        "a < b < c and d",
        "d and a < b < c",
        "not a < b < c",
        "d or a < b < c",
        "[a < b < c]",
        "len(a < b < c)",
        "count(xs, a < b < c)",
        "a[b < c < d]",
        "a < b <c",
        "a<b<c",
        "a==b==c",
        "'a' < 'b' < 'c'",
        "5m < 10m < 15m",
        "$1 < $2 < $3",
        "x == 1 == true",
        "x == y != z in w",
        "a < b and c < d < e",
        "a not in b == c",
        "a == b not in c",
        "a matches b matches c",
    ] {
        err(&mut t, s);
    }
    for s in [
        "a < b and b < c",
        "a < b or b < c",
        "(a < b) and (b < c)",
        "not (a < b)",
        "a < b and b < c and c < d",
        "[a < b, b < c]",
        "count(xs, x < 3)",
        "a[b < c]",
        "a == b and b == c",
        "a in b and b in c",
        "a not in b and b not in c",
        "x < 1 or x > 5",
        "a == 'x' or a == 'y' or a == 'z'",
        "len(a) < len(b)",
        "len(a) < 3 and len(b) < 3",
        "since(ts) > 5m and since(ts) < 1h",
        "not a == b",
        "not a in b",
    ] {
        ok(&mut t, s);
    }
    // DECIDED: the grammar has no group production; a group as the operand of a comparison
    t.ambiguous();
    for s in ["(a < b) < c", "a < (b < c)", "(a == b) == c", "(a < b) == (c < d)", "(a) < (b) and (b) < (c)"] {
        ok(&mut t, s);
    }
    t.finish();
}

// ── 9. structural parse errors ───────────────────────────────────────────────────────────────

#[test]
fn structural_parse_errors() {
    let mut t = Rows::new("structural_parse_errors");
    let empty_like = ["", " ", "\t", "\n", "  \n  ", "\r\n", "   \t  ", "\n\n\n"];
    let parens_brackets = [
        "()", "( )", "(", ")", "(a", "a)", "((a)", "(a))", ")(", "(()", "())", "(a))(", "(a)(b)", "((", "))", "[", "]",
        "[1", "1]", "[1,", "[,", "a[", "a[]", "a[ ]", "a[0", "a[0]]", "a]", "a[[0]", "a[0][", "a[,]", "[1)", "(1]",
        "[(1])", "([1)]", "len(x", "len(x))", "len((x)", "len(x]", "len(", "len)", "len(]", "a[(0])", "a[0)", "(a[0)]",
        "[]]", "[[]", "[()]", "(,)", "(a,b)", "(a, b)",
    ];
    let garbage = [
        "a b",
        "a b c",
        "a 1",
        "1 2",
        "1 a",
        "a 'b'",
        "'a' b",
        "true false",
        "a.b c.d",
        "a == b c",
        "a and b c",
        "(a) b",
        "a (b)",
        "[1] [2]",
        "a [1] 2",
        "a ,",
        "a;",
        "a)",
        "a ]",
        "5m 5m",
        "a.b.",
        "a.b..c",
        "a..b",
        ".a",
        ".",
        "..",
        "a.1",
        "a.0",
        "a.-1",
        "a.'k'",
        "a.[0]",
        "a.(b)",
        "a.5m",
        "a.",
        "a. ",
        "a.\n",
        "a.\"k\"",
        "a.$1",
        "a./b",
        "a(b)",
        "a(",
        "a()",
        "(a)(b)",
        "a b.c",
        "a.b[0] c",
        "[1]a",
        "(a)b",
        "a(b)c",
        "1(a)",
        "'x'(a)",
        "[1](a)",
        "a(b)(c)",
        "len(x)(y)",
        "last(xs)(0)",
        "last(xs).",
        "last(xs).1",
        "last(xs)[",
        "last(xs)[]",
        "last(xs)..a",
        "a.len(x)",
        "len.x(1)",
    ];
    let missing_operands = [
        "a ==",
        "== a",
        "a !=",
        "!= a",
        "a <",
        "< a",
        "a <=",
        "<= a",
        "a >",
        "> a",
        "a >=",
        ">= a",
        "a in",
        "in a",
        "a not in",
        "a not",
        "a matches",
        "matches a",
        "not",
        "not not",
        "a and",
        "and a",
        "a and and b",
        "a or",
        "or a",
        "a or or b",
        "a and or b",
        "a or and b",
        "a == == b",
        "a == and b",
        "a and == b",
        "not and a",
        "not == a",
        "(and)",
        "[or]",
        "len(and)",
        "not in a",
        "a == )",
        "( == a)",
        "a and )",
        "(a and)",
        "(and b)",
        "[a and]",
        "[and a]",
        "a == ]",
        "a < , b",
        "a == b ==",
        "a in not",
        "a in and b",
        "a == not",
        "a and not",
        "a or not",
        "not a and",
        "a and not and b",
        "a ==\n",
    ];
    for group in [&empty_like[..], &parens_brackets[..], &garbage[..], &missing_operands[..]] {
        for s in group {
            err(&mut t, s);
        }
    }
    // the valid neighbours
    for s in [
        "(a)",
        "((a))",
        "[]",
        "[ ]",
        "[[]]",
        "a[0]",
        "a[b]",
        "a[b][c]",
        "[1]",
        "(a) and (b)",
        "len(x)",
        "last(x)[0]",
        "a.b[0].c",
        "not (a)",
        "a  \n ",
        "\t a",
        "((a) and (b))",
        "[(a), (b)]",
        "a[(b)]",
        "len((x))",
        "(len(x))",
        "(a)\n",
    ] {
        ok(&mut t, s);
    }
    t.finish();
}

// ── 10. the closed set of functions ──────────────────────────────────────────────────────────

#[test]
fn functions_closed_set_and_arity() {
    let mut t = Rows::new("functions_closed_set_and_arity");
    // (name, fewest, most) as the module doc gives them
    let arity: [(&str, usize, usize); 10] = [
        ("len", 1, 1),
        ("lower", 1, 1),
        ("contains", 2, 2),
        ("count", 1, 2),
        ("sum", 1, 1),
        ("last", 1, 1),
        ("same", 1, 1),
        ("since", 1, 1),
        ("quote", 1, 1),
        ("danger", 1, 1),
    ];
    let args = ["a", "b", "c", "d", "e"];
    for (name, lo, hi) in arity {
        for n in 0..=4usize {
            let s = format!("{name}({})", args[..n].join(", "));
            if n >= lo && n <= hi {
                if let Some(e) = ok(&mut t, &s) {
                    t.row(matches!(&e, Expr::Call { func, args } if func.word() == name && args.len() == n), || {
                        format!("{s}: should be a call of `{name}` with {n} arguments, got {e:?}")
                    });
                }
            } else {
                err(&mut t, &s);
            }
            // tight spacing reads the same
            let tight = format!("{name}({})", args[..n].join(","));
            if n >= lo && n <= hi {
                same(&mut t, &tight, &s);
            } else {
                err(&mut t, &tight);
            }
        }
    }
    // Func::ALL is the documented set, in the doc's terms
    let mut words: Vec<&str> = Func::ALL.iter().map(|f| f.word()).collect();
    words.sort();
    let mut doc: Vec<&str> = arity.iter().map(|(n, _, _)| *n).collect();
    doc.sort();
    t.row(words == doc, || format!("Func::ALL words {words:?} differ from the documented set {doc:?}"));
    for f in Func::ALL {
        let (lo, hi) = f.arity();
        let want = arity.iter().find(|(n, _, _)| *n == f.word()).map(|(_, l, h)| (*l, *h));
        t.row(want == Some((lo, hi)), || format!("Func::{f:?}.arity() is ({lo}, {hi}), the doc gives {want:?}"));
    }
    // an unknown function is refused, and the message lists the real ones
    for s in [
        "foo(x)",
        "min(a, b)",
        "max(a)",
        "Len(x)",
        "LEN(x)",
        "length(x)",
        "upper(x)",
        "now()",
        "int(x)",
        "str(x)",
        "abs(x)",
        "round(x)",
        "exists(x)",
        "startswith(a, b)",
        "starts_with(a,b)",
        "matches(a, b)",
        "in(a)",
        "and(a, b)",
        "or(a)",
        "x(1)",
        "_(1)",
        "a.b(1)",
        "len2(x)",
        "xlen(x)",
        "lenx(x)",
        "Count(xs)",
        "SUM(xs)",
        "Quote(x)",
        "first(xs)",
        "keys(m)",
        "values(m)",
        "join(a, b)",
        "split(a, b)",
        "replace(a, b, c)",
        "trim(a)",
        "upper (x)",
        "toString(x)",
        "number(x)",
        "bool(x)",
        "any(xs)",
        "all(xs)",
        "max()",
    ] {
        match try_parse(s) {
            Ok(Err(pe)) => {
                t.row(at_ok(&pe, s), || format!("{}: bad at/msg: at={} msg={:?}", show(s), pe.at, pe.msg));
                // Task statement (not the module doc): "unknown function error lists the real ones".
                let missing: Vec<&str> = Func::ALL.iter().map(|f| f.word()).filter(|w| !pe.msg.contains(w)).collect();
                if s == "foo(x)" || s == "min(a, b)" || s == "Len(x)" || s == "now()" || s == "x(1)" || s == "len2(x)" {
                    t.row(missing.is_empty(), || {
                        format!(
                            "{}: the error message {:?} should list every real function, missing {missing:?}",
                            show(s),
                            pe.msg
                        )
                    });
                }
            }
            other => t.row(false, || format!("{}: expected Err, got {}", show(s), describe(&other))),
        }
    }
    // arguments are whole expressions
    for s in [
        "count(xs, a and b)",
        "count(xs, a or b)",
        "count(xs, not a)",
        "count(xs, a == 1)",
        "count(xs, a in b)",
        "count(xs, a matches 'x*')",
        "count(xs, a not in b)",
        "len(a) > 3",
        "contains(a, 'x') and not contains(b, 'y')",
        "sum(xs) >= $1.50",
        "since(ts) > 5m",
        "danger(cmd)",
        "quote(a.b[c])",
        "last(/a/b)",
        "count(/items)",
        "same(/a)",
        "len([a, b, c])",
        "lower(quote(x))",
        "len(lower(x))",
        "count(xs, len(x) > 1)",
        "contains(a.b, c[d])",
        "last(xs).cmd == 'x'",
        "last(xs)[0]",
        "same(xs) and not x",
        "len('abc')",
        "len([1,2])",
        "lower('A')",
        "count([1,2,3], 2)",
        "sum([1,2,3])",
        "last([1,2,3])",
        "count(xs, 5m)",
        "sum([$1, $2])",
        "len( \n x \n )",
        "last(last(xs))",
        "last(last(xs)).a",
        "last(xs)[last(ys)]",
        "contains(last(xs), last(ys))",
        "danger(last(xs).cmd)",
        "quote(danger(x))",
        "len(count(xs))",
        "since(last(xs))",
        "count(xs, count(ys))",
        "[len(x), count(y)]",
        "len(x) in [1, 2]",
        "count(xs, x) >= 2 and count(xs, y) == 0",
    ] {
        ok(&mut t, s);
    }
    // a path after a call or a pointer keeps going
    tree(&mut t, "last(xs).a.b", get(call(Func::Last, vec![nm("xs")]), vec![key("a"), key("b")]));
    tree(&mut t, "last(xs)[0][1]", get(call(Func::Last, vec![nm("xs")]), vec![idx(num(0.0)), idx(num(1.0))]));
    // DECIDED: space between a function name and its parenthesis; a function name followed by `.name` on a call
    t.ambiguous();
    tree(&mut t, "len (x)", call(Func::Len, vec![nm("x")]));
    tree(&mut t, "count (xs, 1)", call(Func::Count, vec![nm("xs"), num(1.0)]));
    t.finish();
}

// ── 11. lists and trailing commas ────────────────────────────────────────────────────────────

#[test]
fn lists_and_trailing_commas() {
    let mut t = Rows::new("lists_and_trailing_commas");
    for s in [
        "[]",
        "[ ]",
        "[1]",
        "[1,2]",
        "[ 1 , 2 ]",
        "[[]]",
        "[[1],[2,3]]",
        "[[[[[]]]]]",
        "[\n1,\n2\n]",
        "[a, b.c, d[0], /p, len(x), 'x', 5m, $1, true, -3, [1]]",
        "[a == 1, b and c, not d]",
        "[(a or b), (c and d)]",
        "x in [1, 2, 3]",
        "[1,2] == [1,2]",
        "[1,2]!=[3]",
        "[a]",
        "['a']",
        "[$1.50]",
        "[5m, 1h30m]",
        "[/a, /b]",
        "[last(xs), len(ys)]",
    ] {
        ok(&mut t, s);
    }
    // the call grammar is `(expr ("," expr)*)?`: no trailing comma, no empty slot
    for s in [
        "len(x,)",
        "len(,)",
        "count(a,)",
        "count(,a)",
        "count(a,,b)",
        "contains(a,)",
        "contains(,b)",
        "count(a, 1,)",
        "count(,)",
        "last(a,)",
        "sum(a,)",
        "same(a,)",
        "since(a,)",
        "quote(a,)",
        "danger(a,)",
        "lower(a,)",
        "len(,x)",
        "count(a b)",
        "count(a;b)",
        "contains(a b)",
        "contains(a, b,)",
        "contains(a,, b)",
        "count(a, 1 2)",
    ] {
        err(&mut t, s);
    }
    // a list needs a comma between items and none before the first
    for s in
        ["[,]", "[,1]", "[1,,2]", "[1 2]", "[1,2 3]", "[a b]", "['a' 'b']", "[1;2]", "[1:2]", "[,,]", "[1,,]", "[ , ]"]
    {
        err(&mut t, s);
    }
    // DECIDED: the list literal is `[expr, …]`; the call grammar forbids a trailing comma and the list grammar
    // does not say. These rows hold "no trailing comma anywhere".
    t.ambiguous();
    for s in ["[1,]", "[1,2,]", "[ 1 , ]", "[a,]", "[[1,],]", "x in [1, 2,]"] {
        ok(&mut t, s);
    }
    for s in ["len(a,)", "count(a,)", "contains(a, b,)"] {
        err(&mut t, s);
    }
    t.finish();
}

// ── 12. where the error is ───────────────────────────────────────────────────────────────────

#[test]
fn error_positions() {
    let mut t = Rows::new("error_positions");
    // (input, lowest ok `at`, highest ok `at`): `at` counts characters from 0, not bytes
    // DECIDED in general (the contract only says "where"): the rows take it to be near the first offending
    // character; single-character lexical errors are pinned exactly, the others get a window.
    let rows: &[(&str, usize, usize)] = &[
        ("a = b", 2, 2),
        ("a ! b", 2, 2),
        ("a & b", 2, 2),
        ("a | b", 2, 2),
        ("a # b", 2, 2),
        ("ab #", 3, 3),
        ("'éé' #", 5, 5), // `at` counts characters, not bytes
        ("'é é é é é' #", 12, 12),
        ("'é' @", 4, 4),
        ("'日本' #", 5, 5),
        ("'😀' #", 4, 4),
        ("éé #", 0, 0), // a letter outside [A-Za-z_] is itself the first offending character
        ("日本", 0, 0),
        ("a b", 2, 2),
        ("a)", 1, 1),
        ("a ]", 2, 2),
        ("a 1", 2, 2),
        ("a == == b", 5, 5),
        ("(a", 0, 2),
        ("[1, 2", 0, 5),
        ("a and", 3, 5),
        ("a ==", 2, 4),
        ("a <", 2, 3),
        ("a < b < c", 4, 9),
        ("count(a,)", 7, 9),
        ("count(a, b, c)", 0, 14),
        ("1h30", 0, 4),
        ("$", 0, 1),
        ("'abc", 0, 4),
        ("\"abc", 0, 4),
        ("a.", 1, 2),
        ("a[0", 1, 3),
        ("a[]", 1, 3),
        ("foo(1)", 0, 6),
        ("a and foo(1)", 6, 12),
        (r#"x == 'a\q'"#, 5, 10),
        ("", 0, 0),
        ("   ", 0, 3),
        ("a\n\n b c", 4, 4),
        ("'é é'\n(", 6, 7),
    ];
    for (s, lo, hi) in rows {
        match try_parse(s) {
            Ok(Err(pe)) => {
                t.row(pe.at >= *lo && pe.at <= *hi && at_ok(&pe, s), || {
                    format!(
                        "{}: at={} (wanted {lo}..={hi}, input has {} chars) msg={:?}",
                        show(s),
                        pe.at,
                        s.chars().count(),
                        pe.msg
                    )
                });
            }
            other => t.row(false, || format!("{}: expected Err, got {}", show(s), describe(&other))),
        }
    }
    // every error row of the other themes obeys `at <= chars` through `err`; here the multi-byte ones in bulk, the
    // single-character mistakes pinned to the character after the (valid, multi-byte) prefix
    for prefix in ["'é' and ", "'日本' == ", "'😀😀' == ", "['ñandú', ", "'é\\n' in ", "len('日本語') > "]
    {
        for bad in ["#", "=", "&", "|", "!", "@"] {
            let s = format!("{prefix}{bad}");
            let want = prefix.chars().count();
            match try_parse(&s) {
                Ok(Err(pe)) => t.row(pe.at == want && at_ok(&pe, &s), || {
                    format!("{}: at={} but the offending character is char {want}", show(&s), pe.at)
                }),
                other => t.row(false, || format!("{}: expected Err, got {}", show(&s), describe(&other))),
            }
        }
        for bad in ["'x", "$", "1h30"] {
            let s = format!("{prefix}{bad}");
            err(&mut t, &s);
        }
    }
    t.finish();
}

// ── 13. the length limit ─────────────────────────────────────────────────────────────────────

fn or_chain_exact(n: usize) -> String {
    let mut s = String::from("a");
    while s.len() + 5 <= n {
        s.push_str(" or a");
    }
    let pad = n - s.len();
    s.push_str(&"a".repeat(pad));
    s
}

#[test]
fn length_limit() {
    let mut t = Rows::new("length_limit");
    let name = |n: usize| "a".repeat(n);
    // exactly MAX_LEN characters is fine, one more is an error
    let exact = or_chain_exact(MAX_LEN);
    let more = or_chain_exact(MAX_LEN + 1);
    t.row(exact.chars().count() == MAX_LEN && more.chars().count() == MAX_LEN + 1, || {
        "test mistake: builder".to_string()
    });
    ok(&mut t, &name(MAX_LEN));
    err(&mut t, &name(MAX_LEN + 1));
    ok(&mut t, &exact);
    err(&mut t, &more);
    err(&mut t, &or_chain_exact(MAX_LEN + 5));
    err(&mut t, &or_chain_exact(MAX_LEN + 100));
    err(&mut t, &or_chain_exact(MAX_LEN * 3));
    err(&mut t, &or_chain_exact(100_000));
    err(&mut t, &name(1_000_000));
    ok(&mut t, &or_chain_exact(MAX_LEN - 1));
    ok(&mut t, &or_chain_exact(MAX_LEN - 7));
    ok(&mut t, &or_chain_exact(MAX_LEN / 2));
    // a text of é: MAX_LEN characters but 2 × MAX_LEN bytes
    let accents = |n: usize| format!("'{}'", "é".repeat(n));
    ok(&mut t, &accents(MAX_LEN - 2));
    err(&mut t, &accents(MAX_LEN - 1));
    err(&mut t, &accents(MAX_LEN + 100));
    ok(&mut t, &accents(MAX_LEN / 2));
    let emoji = |n: usize| format!("\"{}\"", "😀".repeat(n));
    ok(&mut t, &emoji(MAX_LEN - 2));
    err(&mut t, &emoji(MAX_LEN - 1));
    ok(&mut t, &emoji(MAX_LEN - 3));
    let kanji = |n: usize| format!("'{}'", "日".repeat(n));
    ok(&mut t, &kanji(MAX_LEN - 2));
    err(&mut t, &kanji(MAX_LEN - 1));
    // a long fraction
    ok(&mut t, &format!("0.{}", "1".repeat(MAX_LEN - 2)));
    err(&mut t, &format!("0.{}", "1".repeat(MAX_LEN - 1)));
    // a long list
    let list = |k: usize, last: &str| format!("[{}{}]", "1,".repeat(k), last);
    ok(&mut t, &list(998, "11")); // 2000 characters
    err(&mut t, &list(998, "111")); // 2001
    ok(&mut t, &list(998, "1"));
    err(&mut t, &list(5000, "1"));
    // a long path, a long chain of .name and [0] steps (long is not deep)
    ok(&mut t, &format!("a{}", ".b".repeat(999))); // 1999 characters
    err(&mut t, &format!("a{}", ".b".repeat(1000))); // 2001
    ok(&mut t, &format!("a{}", "[0]".repeat(600))); // 1801
    err(&mut t, &format!("a{}", "[0]".repeat(700))); // 2101
    ok(&mut t, &format!("a{}", ".b".repeat(900)));
    // a long and-chain
    ok(&mut t, &format!("a{}", " and a".repeat(300))); // 1801
    err(&mut t, &format!("a{}", " and a".repeat(400))); // 2401
    // errors in short and long inputs keep `at` in range (err() checks at <= chars)
    err(&mut t, &format!("{} #", name(MAX_LEN)));
    err(&mut t, &format!("{}#", "é".repeat(MAX_LEN + 10)));
    // DECIDED: do spaces count? Exactly MAX_LEN characters including padding is fine either way; MAX_LEN + 1
    // characters of which the last is a space is an error only if the whole text is measured.
    t.ambiguous();
    ok(&mut t, &format!("a{}", " ".repeat(MAX_LEN - 1)));
    err(&mut t, &format!("a{}", " ".repeat(MAX_LEN)));
    err(&mut t, &format!("{}a", " ".repeat(MAX_LEN)));
    err(&mut t, &format!("a{}\n", " ".repeat(MAX_LEN - 1)));
    t.finish();
}

/// `unit` joined by `sep` as many times as fit in MAX_LEN characters.
fn siblings(unit: &str, sep: &str) -> String {
    let mut s = unit.to_string();
    while s.chars().count() + sep.chars().count() + unit.chars().count() <= MAX_LEN {
        s.push_str(sep);
        s.push_str(unit);
    }
    s
}

// ── 14. the nesting limit ────────────────────────────────────────────────────────────────────

#[test]
fn nesting_limit() {
    let mut t = Rows::new("nesting_limit");
    // far below MAX_DEPTH
    for (label, build) in SHAPES {
        for n in [1usize, 2, 3, 6, MAX_DEPTH / 2] {
            let s = build(n);
            if s.chars().count() > MAX_LEN {
                t.row(false, || format!("test mistake: {label}({n}) is longer than MAX_LEN"));
                continue;
            }
            if ok(&mut t, &s).is_none() {
                eprintln!("  (shape {label}, depth {n})");
            }
        }
    }
    // far above MAX_DEPTH, and short enough that only the nesting can be what is wrong
    let far: [(&str, usize); 6] =
        [("parens", 500), ("brackets", 500), ("calls", 300), ("indexes", 500), ("nots", 400), ("mixed", 400)];
    for (label, n) in far {
        let build = SHAPES.iter().find(|(l, _)| *l == label).unwrap().1;
        for depth in [n, n / 2, MAX_DEPTH * 3, MAX_DEPTH * 2, 100] {
            let s = build(depth);
            if s.chars().count() > MAX_LEN {
                t.row(false, || format!("test mistake: {label}({depth}) is longer than MAX_LEN"));
                continue;
            }
            err(&mut t, &s);
        }
    }
    // nesting is a depth, not a count: siblings, long chains and wide lists are fine
    for (unit, sep) in [
        ("(a)", " or "),
        ("((((a))))", " and "),
        ("len(a)", " and "),
        ("not a", " and "),
        ("a[b[c]]", " or "),
        ("[[[1]]]", " or "),
    ] {
        let s = siblings(unit, sep);
        t.row(s.chars().count() > 1500, || format!("test mistake: {unit:?} siblings are too short"));
        ok(&mut t, &s);
    }
    for build in [calls(10), indexes(10), mixed(10), parens(10), brackets(10), nots(10)] {
        ok(&mut t, &siblings(&build, " or "));
    }
    ok(&mut t, &format!("a{}", ".b".repeat(900)));
    ok(&mut t, &format!("a{}", "[0]".repeat(600)));
    ok(&mut t, &format!("last(a){}", "[0]".repeat(600)));

    // DECIDED: where exactly MAX_DEPTH is. "The deepest nesting … MAX_DEPTH levels": the rows hold that MAX_DEPTH
    // levels are fine and MAX_DEPTH + 1 is not. A call may count as one level (the call) or two (call and its
    // parentheses); `not` too.
    t.ambiguous();
    for (label, build) in SHAPES {
        for (n, want_ok) in [(MAX_DEPTH - 1, true), (MAX_DEPTH, true), (MAX_DEPTH + 1, false)] {
            let s = build(n);
            if want_ok {
                if ok(&mut t, &s).is_none() {
                    eprintln!("  (boundary shape {label}, depth {n})");
                }
            } else {
                err(&mut t, &s);
            }
        }
    }
    // two shapes in one: 12 + 13 levels
    let s = format!("{}{}a{}{}", "(".repeat(12), "[".repeat(13), "]".repeat(13), ")".repeat(12));
    err(&mut t, &s);
    t.finish();
}

/// Far-too-deep input must be refused by a counter, not by running out of stack: parse it on a small stack.
#[test]
fn nesting_never_overflows_a_small_stack() {
    let h = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            let mut t = Rows::new("nesting_small_stack");
            for (_label, build) in SHAPES {
                for n in [50usize, 100, 200, 300, 400] {
                    let s = build(n);
                    if s.chars().count() <= MAX_LEN {
                        err(&mut t, &s);
                    }
                }
                // a legal one, too
                ok(&mut t, &build(10));
                round_trip(&mut t, &build(10));
            }
            // and printing a tree that is as deep as allowed
            for (_label, build) in SHAPES {
                round_trip(&mut t, &build(MAX_DEPTH / 2));
            }
            t.finish();
        })
        .unwrap();
    if let Err(p) = h.join() {
        std::panic::resume_unwind(p);
    }
}

// ── 15. Display round trips ──────────────────────────────────────────────────────────────────

const RT: &[&str] = &[
    // names, paths, pointers
    "a",
    "event.data.cmd",
    "fold.spend[task.id]",
    "a[0]",
    "a['k']",
    "a[\"k\"]",
    "a[0][1][2]",
    "a.b[c.d].e",
    "a[b[c[d]]]",
    "/",
    "/usage/cost_usd",
    "/content/0/text",
    "/type=='text'",
    "/a.b:c~0d@e$f%g+h-i_j",
    "/a[0]",
    "/a/b[x].y",
    "[/a, /b]",
    "/a == /b",
    // numbers
    "true",
    "false",
    "0",
    "12",
    "-3",
    "0.5",
    "-0.5",
    "0.1",
    "3.14159265358979",
    "123456789012345678",
    "0.000001",
    "0.0000000001",
    "0.00000000000000000001",
    "100000000000000000000",
    // durations and money
    "5m",
    "250ms",
    "1h30m",
    "1.5h",
    "90s",
    "0s",
    "1d",
    "2d12h",
    "0.5s",
    "1m30s500ms",
    "1500ms",
    "100d",
    "1ms",
    "$1.50",
    "$0.0004",
    "$0",
    "$12",
    "$1000000",
    "$0.000001",
    "$123456.789012",
    // text
    "''",
    "\"\"",
    r#"'it\'s'"#,
    r#""say \"hi\"""#,
    r#"'a\\b'"#,
    r#"'\\'"#,
    r#"'\\n'"#,
    r#"'line1\nline2'"#,
    r#"'tab\there'"#,
    r#"'cr\rhere'"#,
    r#"'{x}'"#,
    r#"'{{x}}'"#,
    r#"'}{'"#,
    r#"'${x}'"#,
    r#"'a"b'"#,
    r#""a'b""#,
    r#"'\'"'"#,
    r#"'\\\\'"#,
    r#"'\\\''"#,
    r#"'\n\n'"#,
    r#"'\"\"\'\''"#,
    "'héllo ✓'",
    "'😀'",
    "'a and b'",
    "'/a/b'",
    "'$1.50'",
    "'5m'",
    "'true'",
    "'not'",
    "' '",
    // operators and grouping
    "a == b",
    "a != b",
    "a < b",
    "a <= b",
    "a > b",
    "a >= b",
    "x in y",
    "x not in y",
    "a matches 'x*'",
    "a matches b.c",
    "not x in y",
    "not (a == b)",
    "(a or b) and c",
    "a and (b or c)",
    "(a or b) or c",
    "a or (b or c)",
    "(a and b) and c",
    "a and (b and c)",
    "not (a and b)",
    "not (a or b)",
    "not not a",
    "not (not (not a))",
    "(not a) and b",
    "not a and b",
    "a and not b",
    "a or not b and c",
    "not (a == 1) or (b == 2 and c == 3)",
    "a == 1 and b == 2 or c == 3 and d == 4",
    "(a or b) and (c or d)",
    "((a or b) and c) or d",
    "not (a or b) and not (c and d)",
    "x in y and z in w",
    "x not in y or z not in w",
    "(x not in y) and z",
    "not (x not in y)",
    "not (x in y)",
    "a or b or c",
    "a and b and c and d",
    "(a or b or c) and (d or e)",
    "not (a or b or c)",
    "(not (a and b)) or c",
    // lists
    "[]",
    "[[]]",
    "[1, 2, 3]",
    "[1, 'a', true, 5m, $1, -3, 0.5]",
    "[[1, 2], [3]]",
    "[a, b.c, d[0]]",
    "x in [1, 2, 3]",
    "[1,2] == [1,2]",
    "[a == 1, b and c, not d, e or f]",
    "[(a or b), (c and d)]",
    // calls
    "len(x) > 3",
    "count(xs)",
    "count(xs, 'a')",
    "count(xs, x == 1)",
    "count(xs, not x)",
    "contains(a, 'b')",
    "sum(xs) >= $1.50",
    "since(ts) > 5m",
    "quote(x)",
    "danger(cmd) == 'deny'",
    "last(xs).cmd",
    "last(xs)[0].a",
    "same(xs) and not x",
    "lower(x) == 'a'",
    "len(len(x))",
    "len(last(xs).items)",
    "count(xs, a or b)",
    "count(xs, (a or b) and c)",
    "last(/a/b)",
    "len([a, b])",
    "contains(a.b, c[d])",
    "lower(quote(x))",
    // mixed
    "event.data.cmd == 'x' and len(event.data.args) > 2",
    "/usage/cost_usd > $0.5",
    "/content/0/text matches '*err*'",
    "a == true",
    "a != 'x'",
    "a <= 5m",
    "a >= 0.5",
    "-3 in xs",
    "-0.5 < x",
    "'a' in b",
    "5m < since(ts)",
    "a.b.c.d.e.f",
];

#[test]
fn display_round_trip() {
    let mut t = Rows::new("display_round_trip");
    t.row(RT.len() >= 60, || "test mistake: fewer than 60 inputs".to_string());
    for s in RT {
        round_trip(&mut t, s);
    }
    // DECIDED: inputs whose reading is not fixed by the contract (parenthesised comparisons, trailing comma, raw
    // control characters in text, space before a dot): if they parse, they must also print and read back
    t.ambiguous();
    for s in [
        "(a < b) < c",
        "a < (b < c)",
        "(a == b) == c",
        "(a) == (b)",
        "[1,]",
        "'a\nb'",
        "'a\tb'",
        "a . b",
        "a [0]",
        "len (x)",
        "x.and",
        "x.or",
        "x.not",
        "x.in",
        "x.matches",
        "x.true",
        "x.false",
        "a.in.or.not[true]",
        "007",
        "30m1h",
        "1h1h",
        "0.4ms",
        "$0.0000004",
        "$0.1234567",
        "-0",
        "not [0]",
        "not(a)",
        "/ == 1",
        "'x' in /",
    ] {
        round_trip(&mut t, s);
    }
    t.finish();
}

#[test]
fn display_round_trip_generated() {
    let mut t = Rows::new("display_round_trip_generated");
    // each construct at several depths, and the extremes of the number grammar
    for (label, build) in SHAPES {
        // (`not` chains deeper than 12 print beyond MAX_DEPTH: see display_round_trip_limits_and_pointer_steps)
        for n in [1usize, 2, 3, 5, 8, 12, 20] {
            if label == "nots" && n > 12 {
                continue;
            }
            round_trip(&mut t, &build(n));
        }
    }
    let nines = "9".repeat(60);
    for s in [
        format!("1{}", "0".repeat(300)),
        format!("1{}", "0".repeat(307)),
        format!("17976931348623157{}", "0".repeat(292)),
        format!("0.{}1", "0".repeat(30)),
        format!("0.{}1", "0".repeat(300)),
        format!("0.{nines}"),
        format!("{nines}.{nines}"),
        format!("-{}", "1".repeat(30)),
        format!("[{}, 0.{}1]", "1".repeat(20), "0".repeat(20)),
        format!("${}", "9".repeat(12)),
        format!("{}ms", "9".repeat(12)),
        format!("{}d", "9".repeat(5)),
        format!("'{}'", "\\\\".repeat(100)),
        format!("'{}'", "\\'".repeat(100)),
        format!("'{}'", "\\n".repeat(100)),
        format!("'{}'", "{x}".repeat(100)),
        format!("'{}'", "é".repeat(300)),
    ] {
        round_trip(&mut t, &s);
    }
    // random expressions, valid by construction of the grammar in the module doc
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..4000 {
        let s = g_expr(&mut r, 3);
        if s.chars().count() > SHORT {
            continue; // printing adds parentheses; stay far from MAX_LEN (see display_round_trip_limits)
        }
        round_trip(&mut t, &s);
    }
    t.finish();
}

/// What Display prints must be inside the limits `parse` enforces and must keep the steps of a path after a pointer.
/// Every row here is a plain reading of "`Display` prints one that parses back to the same tree" for an input that
/// `parse` itself accepted.
#[test]
fn display_round_trip_limits_and_pointer_steps() {
    let mut t = Rows::new("display_round_trip_limits_and_pointer_steps");

    // BUG: `not not a` prints as `not (not a)`: the grammar says `not := "not" not`, so the parentheses are not needed,
    // and they double the depth: 13 nested nots (accepted by `parse`) print as 25 levels, which `parse` refuses.
    for n in 1..MAX_DEPTH {
        round_trip(&mut t, &nots(n));
    }
    // BUG (same cause, seen through the length limit): 150 × `not not a` is 1946 characters and prints as 2246.
    let long = siblings("not not a", " or ");
    t.row(long.chars().count() <= MAX_LEN && long.chars().count() > 1900, || "test mistake: builder".to_string());
    round_trip(&mut t, &long);
    // the same through the other construct that gains parentheses when printed: a group inside a group of one kind
    round_trip(&mut t, &siblings("(a or b) or c", " and "));
    round_trip(&mut t, &siblings("(a and b) and c", " or "));
    // nesting that only the printed form deepens: `not` inside `not` inside a list inside a call, four times
    round_trip(&mut t, &calls(1).replace('x', &nots(11)));
    round_trip(&mut t, &format!("[{}]", nots(11)));
    round_trip(&mut t, &format!("a[{}]", nots(11)));
    round_trip(&mut t, &format!("({})", nots(12)));

    // `.` is a pointer character, so `/p/q.false` is one pointer and a `.name` step may only follow an index step:
    // a dot after a pointer and a space is refused (it would print as part of the pointer).
    for src in [
        "/p/q .false",
        "/a .b",
        "/a/b .c.d",
        "/a .b[0]",
        "/ .a",
        "/a .b == /c .d",
        "[/a .b, /c]",
        "len(/a .b)",
        "not /a .b",
        "/a .b and /c",
        "/a .b in x",
        "x in /a .b",
        "/a .b matches 'x*'",
    ] {
        err(&mut t, src);
    }
    for src in ["/a[0] .b", "/a[0].b", "/a[b] .c"] {
        round_trip(&mut t, src);
    }
    t.finish();
}

// ── random input ─────────────────────────────────────────────────────────────────────────────

/// Inputs up to this many characters cannot print beyond MAX_LEN even if every `not` gains a pair of parentheses.
const SHORT: usize = 1000;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[self.below(xs.len())]
    }
}

const GEN_NAMES: [&str; 8] = ["a", "b", "c", "x", "y1", "_z", "task", "event"];
const GEN_TEXT: [&str; 9] =
    ["'a'", "\"b\"", r#"'it\'s'"#, "'x y'", "''", "'{x}'", r#"'a\\b'"#, r#"'say "hi"'"#, r#"'l1\nl2'"#];
const GEN_NUM: [&str; 7] = ["0", "12", "-3", "0.5", "100", "-0.25", "7"];
const GEN_DUR: [&str; 5] = ["5m", "250ms", "1h30m", "90s", "1.5h"];
const GEN_USD: [&str; 4] = ["$1.50", "$0.0004", "$0", "$12"];
const GEN_PTR: [&str; 5] = ["/a/b", "/usage/cost_usd", "/", "/content/0/text", "/x-y.z"];

/// An atom: what may stand on either side of a comparison.
fn g_atom(r: &mut Rng, d: usize) -> String {
    match r.below(if d == 0 { 9 } else { 17 }) {
        0 => r.pick(&GEN_NAMES).to_string(),
        1 => format!("{}.{}.{}", r.pick(&GEN_NAMES), r.pick(&GEN_NAMES), r.pick(&GEN_NAMES)),
        2 => format!("{}.{}", r.pick(&GEN_NAMES), r.pick(&GEN_NAMES)),
        3 => r.pick(&GEN_PTR).to_string(),
        4 => r.pick(&GEN_NUM).to_string(),
        5 => r.pick(&GEN_TEXT).to_string(),
        6 => r.pick(&["true", "false"]).to_string(),
        7 => r.pick(&GEN_DUR).to_string(),
        8 => r.pick(&GEN_USD).to_string(),
        9 => format!("{}[{}]", r.pick(&GEN_NAMES), g_expr(r, d - 1)),
        10 => {
            let k = r.below(4);
            let items: Vec<String> = (0..k).map(|_| g_expr(r, d - 1)).collect();
            format!("[{}]", items.join(", "))
        }
        11 => format!(
            "{}({})",
            r.pick(&["len", "lower", "sum", "last", "same", "since", "quote", "danger", "count"]),
            g_expr(r, d - 1)
        ),
        12 => format!("{}({}, {})", r.pick(&["contains", "count"]), g_expr(r, d - 1), g_expr(r, d - 1)),
        13 => format!("last({}).{}", g_expr(r, d - 1), r.pick(&GEN_NAMES)),
        14 => format!("last({})[{}]", g_expr(r, d - 1), g_expr(r, d - 1)),
        15 => format!("{}[{}]", r.pick(&GEN_PTR), g_expr(r, d - 1)),
        _ => format!("{}.{}[{}].{}", r.pick(&GEN_NAMES), r.pick(&GEN_NAMES), g_expr(r, d - 1), r.pick(&GEN_NAMES)),
    }
}

fn g_cmp(r: &mut Rng, d: usize) -> String {
    let l = g_atom(r, d);
    if r.below(2) == 0 {
        return l;
    }
    let op = r.pick(&["==", "!=", "<", "<=", ">", ">=", "in", "not in", "matches"]);
    format!("{l} {op} {}", g_atom(r, d))
}

fn g_not(r: &mut Rng, d: usize, nots: usize) -> String {
    match r.below(6) {
        0 if nots < 2 => format!("not {}", g_not(r, d, nots + 1)),
        1 if d > 0 => format!("({})", g_expr(r, d - 1)),
        2 if d > 0 => format!("not ({})", g_expr(r, d - 1)),
        _ => g_cmp(r, d),
    }
}

fn g_and(r: &mut Rng, d: usize) -> String {
    let k = 1 + r.below(3);
    (0..k).map(|_| g_not(r, d, 0)).collect::<Vec<_>>().join(" and ")
}

fn g_expr(r: &mut Rng, d: usize) -> String {
    let k = 1 + r.below(3);
    (0..k).map(|_| g_and(r, d)).collect::<Vec<_>>().join(" or ")
}

// ── 16. FromStr and ParseError ───────────────────────────────────────────────────────────────

#[test]
fn from_str_and_parse_error() {
    let mut t = Rows::new("from_str_and_parse_error");
    let inputs = [
        "a == 1",
        "a",
        "",
        "  ",
        "a = 1",
        "a < b < c",
        "len(x)",
        "len()",
        "foo(1)",
        "'abc",
        "5m",
        "1h30",
        "$1.50",
        "[1, 2]",
        "[1,]",
        "not (a and b)",
        "/usage/cost_usd",
        "a b",
        "a and",
        "(a",
        "a)",
        "x not in y",
        "a matches 'x*'",
        "1e5",
        "-3",
        "a.b[c]",
        "and",
        "true",
        "é",
        "'\\q'",
        "last(xs).cmd",
        "((((a))))",
        "count(a, b, c)",
        "contains(a)",
    ];
    for s in inputs {
        let via_parse = guarded(|| format!("{:?}", Expr::parse(s)));
        let via_from_str = guarded(|| format!("{:?}", s.parse::<Expr>()));
        match (via_parse, via_from_str) {
            (Ok(p), Ok(f)) => {
                t.row(p == f, || format!("{}: Expr::parse gives {p} but str::parse::<Expr> gives {f}", show(s)))
            }
            (a, b) => t.row(false, || format!("{}: {a:?} / {b:?}", show(s))),
        }
    }
    // an Ok from str::parse is an Expr that Displays and parses back
    for s in ["a == 1", "not (a and b)", "last(xs).cmd", "[1, 'a']"] {
        match s.parse::<Expr>() {
            Ok(e) => round_trip(&mut t, &e.to_string()),
            Err(pe) => t.row(false, || format!("{}: {pe:?}", show(s))),
        }
    }
    // ParseError: `at` and `msg` are public, it prints, it is an Error, it compares
    match Expr::parse("a = b") {
        Err(pe) => {
            t.row(!pe.to_string().trim().is_empty(), || "ParseError prints as nothing".to_string());
            t.row(pe.clone() == pe, || "ParseError != its clone".to_string());
            let boxed: Box<dyn std::error::Error> = Box::new(pe.clone());
            t.row(!boxed.to_string().is_empty(), || "ParseError as Box<dyn Error> prints as nothing".to_string());
            // DECIDED: what a ParseError prints as is not documented; the natural reading carries the message
            t.ambiguous();
            t.row(pe.to_string().contains(&pe.msg), || {
                format!("ParseError's Display {:?} should carry its message {:?}", pe.to_string(), pe.msg)
            });
            t.firm();
        }
        Ok(e) => t.row(false, || format!("`a = b` should be refused, got {e:?}")),
    }
    // the same input twice gives the same answer
    for s in ["a = b", "a == b", "len(x", "(((a)))"] {
        let x = format!("{:?}", Expr::parse(s));
        let y = format!("{:?}", Expr::parse(s));
        t.row(x == y, || format!("{}: two parses differ: {x} / {y}", show(s)));
    }
    // two different errors differ (the message names the problem, so these are not one generic message)
    let msgs: Vec<(String, String)> =
        ["a = b", "a ! b", "a & b", "'abc", "5x", "a < b < c", "foo(1)", "len()", "(a", "a b", ""]
            .iter()
            .filter_map(|s| Expr::parse(s).err().map(|e| (s.to_string(), e.msg)))
            .collect();
    let mut uniq: Vec<&String> = msgs.iter().map(|(_, m)| m).collect();
    uniq.sort();
    uniq.dedup();
    t.row(uniq.len() >= 6, || {
        format!("eleven different mistakes gave only {} different messages: {msgs:?}", uniq.len())
    });
    t.finish();
}

// ── 17. Expr::check, the lint ────────────────────────────────────────────────────────────────

enum Want {
    /// Nothing to report.
    Clean,
    /// At least one message.
    Any,
    /// At least this many messages.
    AtLeast(usize),
    /// Messages, and every one of these words is in some message.
    Names(&'static [&'static str]),
}

fn lint(s: &str, roots: Option<&[&str]>) -> Result<Vec<String>, String> {
    let e = Expr::parse(s).map_err(|pe| format!("does not parse: at={} msg={:?}", pe.at, pe.msg))?;
    guarded(|| e.check(roots))
}

fn lint_row(t: &mut Rows, s: &str, roots: Option<&[&str]>, want: Want) {
    let shown = format!("{} with roots {:?}", show(s), roots);
    match lint(s, roots) {
        Err(m) => t.row(false, || format!("{shown}: {m}")),
        Ok(msgs) => {
            let ok = match &want {
                Want::Clean => msgs.is_empty(),
                Want::Any => !msgs.is_empty() && msgs.iter().all(|m| !m.trim().is_empty()),
                Want::AtLeast(n) => msgs.len() >= *n && msgs.iter().all(|m| !m.trim().is_empty()),
                Want::Names(names) => !msgs.is_empty() && names.iter().all(|n| msgs.iter().any(|m| m.contains(n))),
            };
            t.row(ok, || {
                let w = match &want {
                    Want::Clean => "nothing".to_string(),
                    Want::Any => "a message".to_string(),
                    Want::AtLeast(n) => format!("at least {n} messages"),
                    Want::Names(n) => format!("messages naming {n:?}"),
                };
                format!("{shown}: wanted {w}, got {msgs:?}")
            });
        }
    }
}

#[test]
fn check_lint() {
    let mut t = Rows::new("check_lint");

    // None: any root is fine
    for s in [
        "ghost.x == nope[zork]",
        "ghost",
        "a.b.c",
        "len(ghost) > 1",
        "count(ghost, wraith)",
        "a in ghost",
        "[ghost, nope]",
        "not ghost",
        "ghost and nope or zork",
        "last(ghost).x",
        "ghost[nope[zork]]",
        "/a/b",
        "/a[ghost]",
        "fold.spend[task.id]",
    ] {
        lint_row(&mut t, s, None, Want::Clean);
    }

    // clean expressions, with every root they use listed
    let clean: &[(&str, &[&str])] = &[
        ("event.data.cmd == 'x'", &["event"]),
        ("len(a) > 3", &["a"]),
        ("count(xs, 1) >= 2", &["xs"]),
        ("sum(costs) > $1", &["costs"]),
        ("since(ts) > 5m", &["ts"]),
        ("last(xs).cmd matches 'rm*'", &["xs"]),
        ("danger(cmd)", &["cmd"]),
        ("/a/b > 1", &[]),
        ("/a == /b", &[]),
        ("a in [1, 2]", &["a"]),
        ("'x' in a", &["a"]),
        ("k in m", &["k", "m"]),
        ("contains(a, 'x')", &["a"]),
        ("quote(a)", &["a"]),
        ("lower(a) == 'b'", &["a"]),
        ("same(xs)", &["xs"]),
        ("a matches 'x*'", &["a"]),
        ("not a", &["a"]),
        ("a and b or c", &["a", "b", "c"]),
        ("[a, b]", &["a", "b"]),
        ("a[b]", &["a", "b"]),
        ("fold.spend[task.id]", &["fold", "task"]),
        ("true", &[]),
        ("'text'", &[]),
        ("5m", &[]),
        ("$1.50", &[]),
        ("12", &[]),
        ("len([1,2,3])", &[]),
        ("lower('ABC')", &[]),
        ("sum([1,2])", &[]),
        ("sum([5m, 1m])", &[]),
        ("sum([$1, $2])", &[]),
        ("contains('abc', 'b')", &[]),
        ("contains([1], 1)", &[]),
        ("contains(x, 5)", &["x"]),
        ("count([1,2])", &[]),
        ("count([1,2], 1)", &[]),
        ("count(x, 5)", &["x"]),
        ("last([1])", &[]),
        ("same([1,1])", &[]),
        ("since(5)", &[]),
        ("since(1700000000000)", &[]),
        ("since(x)", &["x"]),
        ("danger('rm -rf /')", &[]),
        ("x in [1]", &["x"]),
        ("x in 'abc'", &["x"]),
        ("1 in [1]", &[]),
        ("'a' in 'abc'", &[]),
        ("x in y", &["x", "y"]),
        ("x matches 'a*'", &["x"]),
        ("'a' matches x", &["x"]),
        ("'a' matches 'a*'", &[]),
        ("x not in 'abc'", &["x"]),
        ("x not in [1, 2]", &["x"]),
        ("a[len('x')]", &["a"]),
        ("a[last([1])]", &["a"]),
        ("last(xs)[0].a == 1", &["xs"]),
        ("count(xs, x == 1)", &["xs", "x"]),
        ("a.b.c.d", &["a"]),
        ("a['k']", &["a"]),
        ("a == b and c in d or not e", &["a", "b", "c", "d", "e"]),
        ("event.data.args[0] == '-f'", &["event"]),
    ];
    for (s, roots) in clean {
        lint_row(&mut t, s, None, Want::Clean);
        lint_row(&mut t, s, Some(*roots), Want::Clean);
    }

    // unknown roots: reported, and named
    const R_A: &[&str] = &["a"];
    const R_AB: &[&str] = &["a", "b"];
    const R_ABC: &[&str] = &["a", "b", "c"];
    const R_FOLD: &[&str] = &["fold"];
    const R_TASK: &[&str] = &["task"];
    const R_UPPER: &[&str] = &["A"];
    const R_PREFIX: &[&str] = &["ghos", "ghostx", "hos"];
    const R_EMPTY: &[&str] = &[];
    let a: Option<&[&str]> = Some(R_A);
    let ab: Option<&[&str]> = Some(R_AB);
    let none: Option<&[&str]> = Some(R_EMPTY);
    let unknown: Vec<(&str, Option<&[&str]>, Want)> = vec![
        ("ghost", none, Want::Names(&["ghost"])),
        ("ghost.x == 1", a, Want::Names(&["ghost"])),
        ("a == ghost", a, Want::Names(&["ghost"])),
        ("ghost == a", a, Want::Names(&["ghost"])),
        ("ghost == nope", a, Want::Names(&["ghost", "nope"])),
        ("not ghost", a, Want::Names(&["ghost"])),
        ("a and ghost", a, Want::Names(&["ghost"])),
        ("ghost or a", a, Want::Names(&["ghost"])),
        ("a in ghost", a, Want::Names(&["ghost"])),
        ("ghost in a", a, Want::Names(&["ghost"])),
        ("ghost not in a", a, Want::Names(&["ghost"])),
        ("a not in ghost", a, Want::Names(&["ghost"])),
        ("a matches ghost", a, Want::Names(&["ghost"])),
        ("ghost matches 'x'", a, Want::Names(&["ghost"])),
        ("[a, ghost]", a, Want::Names(&["ghost"])),
        ("[[ghost]]", a, Want::Names(&["ghost"])),
        ("len(ghost)", a, Want::Names(&["ghost"])),
        ("count(a, ghost)", a, Want::Names(&["ghost"])),
        ("count(ghost, a)", a, Want::Names(&["ghost"])),
        ("contains(a, ghost)", a, Want::Names(&["ghost"])),
        ("contains(ghost, a)", a, Want::Names(&["ghost"])),
        ("lower(ghost)", a, Want::Names(&["ghost"])),
        ("since(ghost)", a, Want::Names(&["ghost"])),
        ("sum(ghost)", a, Want::Names(&["ghost"])),
        ("quote(ghost)", a, Want::Names(&["ghost"])),
        ("danger(ghost)", a, Want::Names(&["ghost"])),
        ("same(ghost)", a, Want::Names(&["ghost"])),
        ("last(ghost).x", a, Want::Names(&["ghost"])),
        ("last(a)[ghost]", a, Want::Names(&["ghost"])),
        ("a[ghost]", a, Want::Names(&["ghost"])),
        ("a[ghost.x]", a, Want::Names(&["ghost"])),
        ("a.b[ghost].c", a, Want::Names(&["ghost"])),
        ("a[b[ghost]]", ab, Want::Names(&["ghost"])),
        ("/a[ghost]", none, Want::Names(&["ghost"])),
        ("/a/b[0][ghost]", none, Want::Names(&["ghost"])),
        ("last(a)[len(ghost)]", a, Want::Names(&["ghost"])),
        ("count(a, len(ghost) > 1)", a, Want::Names(&["ghost"])),
        ("a[last(ghost)]", a, Want::Names(&["ghost"])),
        ("a[count(b, ghost)]", ab, Want::Names(&["ghost"])),
        ("(ghost)", a, Want::Names(&["ghost"])),
        ("not (a and ghost)", a, Want::Names(&["ghost"])),
        ("fold.spend[task.id]", Some(R_FOLD), Want::Names(&["task"])),
        ("fold.spend[task.id]", Some(R_TASK), Want::Names(&["fold"])),
        ("A.x", a, Want::Names(&["A"])), // roots are case-sensitive
        ("a.x", Some(R_UPPER), Want::Names(&["a"])),
        ("zork.a.b.c", a, Want::Names(&["zork"])),
        ("wraith[0]", a, Want::Names(&["wraith"])),
        ("ghost.a == nope.b and wraith.c", a, Want::Names(&["ghost", "nope", "wraith"])),
        ("ghost", Some(R_ABC), Want::Names(&["ghost"])),
        ("ghost", Some(R_PREFIX), Want::Names(&["ghost"])), // whole names, not prefixes
    ];
    for (s, roots, want) in unknown {
        lint_row(&mut t, s, roots, want);
    }
    // keys are not roots
    for (s, roots) in [
        ("a.ghost", a),
        ("a.ghost.nope", a),
        ("a['ghost']", a),
        ("/ghost/x", none),
        ("a == 'ghost'", a),
        ("'ghost' in a", a),
    ] {
        lint_row(&mut t, s, roots, Want::Clean);
    }
    // nothing but literals and pointers: no root to list
    for s in ["1 == 1", "'a' in ['a']", "5m < 1h", "$1 < $2", "[1, 2]", "/a/b", "true and false", "len([1]) == 1"] {
        lint_row(&mut t, s, none, Want::Clean);
    }
    // more than one mistake: more than one message (every mistake that can be seen)
    lint_row(&mut t, "ghost == nope", a, Want::AtLeast(2));
    lint_row(&mut t, "len(5) and len(6)", None, Want::AtLeast(2));
    lint_row(&mut t, "len(5) and ghost", a, Want::AtLeast(2));
    lint_row(&mut t, "ghost and nope and wraith", a, Want::AtLeast(3));
    lint_row(&mut t, "lower(5) == lower(true)", None, Want::AtLeast(2));
    lint_row(&mut t, "x in 5 or y in true", None, Want::AtLeast(2));

    // literal kinds a function or in/matches cannot take
    const R_ALL: &[&str] = &["a", "b", "x", "y", "p"];
    for s in [
        "len(5)",
        "len(true)",
        "len(5m)",
        "len($1)",
        "lower(5)",
        "lower([1])",
        "lower(true)",
        "lower(5m)",
        "contains(5, a)",
        "contains(true, a)",
        "contains(5m, a)",
        "contains($1, a)",
        "count(5)",
        "count('abc')",
        "count(true)",
        "count(5, 1)",
        "sum(5)",
        "sum('a')",
        "sum(true)",
        "last(5)",
        "last('a')",
        "same(5)",
        "same('a')",
        "since('x')",
        "since([1])",
        "since(true)",
        "danger(5)",
        "danger([1])",
        "danger(true)",
        "x in 5",
        "x in true",
        "x in 5m",
        "x in $1",
        "x not in 5",
        "5 matches 'a'",
        "'a' matches 5",
        "x matches 5",
        "5 matches x",
        "true matches x",
        "[1] matches 'a'",
        "x matches [1]",
        "x matches true",
        "x matches 5m",
        "x matches $1",
        // nested: the mistake is found wherever it sits
        "a and len(5)",
        "len(5) or a",
        "not len(5)",
        "len(5) == 1",
        "1 == len(5)",
        "[len(5)]",
        "a[len(5)]",
        "last(a)[len(5)]",
        "count(a, len(5))",
        "len(lower(5))",
        "(len(5))",
        "x in [len(5)]",
        "/p[len(5)]",
        "lower(a) == lower(5)",
        "contains(a, len(5))",
        "a.b[lower(5)].c",
        "last(a)[sum(5)].x",
        "len(5).x",
        "not (a and (b or len(5)))",
        "[[[len(5)]]]",
        "count(a, x in 5)",
        "count(a, x matches 5)",
        "a[b in 5]",
        "len(x in 5)",
        "x in 5 and y",
    ] {
        lint_row(&mut t, s, None, Want::Any);
        // and with every root known: still found
        lint_row(&mut t, s, Some(R_ALL), Want::Any);
    }

    // DECIDED: literals whose fit depends on the other side, and quote's input
    t.ambiguous();
    for s in ["5 in 'abc'", "'a' in 5", "since(5m)"] {
        // two readings: a static refusal (a number can never be inside a text, a duration is no time of day) or
        // silence (it is only a missing at run time); the rows hold "report"
        lint_row(&mut t, s, None, Want::Any);
    }
    // a duplicated unknown root: one message or two
    lint_row(&mut t, "ghost == ghost", a, Want::Any);
    t.finish();
}

// ── 18. Func and Op words ────────────────────────────────────────────────────────────────────

#[test]
fn func_and_op_words() {
    let mut t = Rows::new("func_and_op_words");
    t.row(Func::ALL.len() == 10, || format!("Func::ALL has {} entries, the doc names ten", Func::ALL.len()));
    // each function's word parses as a call, and the call carries that function
    for f in Func::ALL {
        let (lo, _hi) = f.arity();
        let args = ["a", "b"][..lo.min(2)].join(", ");
        let s = format!("{}({args})", f.word());
        match try_parse(&s) {
            Ok(Ok(Expr::Call { func, args: got })) => {
                t.row(func == f && got.len() == lo, || format!("{s}: read as {func:?} with {} args", got.len()))
            }
            other => t.row(false, || format!("{s}: expected a call of {f:?}, got {}", describe(&other))),
        }
    }
    let mut seen: Vec<&str> = Func::ALL.iter().map(|f| f.word()).collect();
    seen.sort();
    seen.dedup();
    t.row(seen.len() == Func::ALL.len(), || "two functions share a word".to_string());
    // the operator words, as the text of the comparison (undocumented: DECIDED)
    t.ambiguous();
    for (op, w) in [
        (Op::Eq, "=="),
        (Op::Ne, "!="),
        (Op::Lt, "<"),
        (Op::Le, "<="),
        (Op::Gt, ">"),
        (Op::Ge, ">="),
        (Op::In, "in"),
        (Op::Matches, "matches"),
    ] {
        t.row(op.word() == w, || format!("Op::{op:?}.word() is {:?}, the operator is written {w:?}", op.word()));
    }
    t.finish();
}

// ── 19. never panics, whatever it is given ───────────────────────────────────────────────────

fn robust(t: &mut Rows, s: &str) {
    match try_parse(s) {
        Err(p) => t.row(false, || format!("{}: {p}", show(s))),
        Ok(Err(pe)) => t.row(at_ok(&pe, s), || {
            format!("{}: Err(at={}, {:?}) with {} chars", show(s), pe.at, pe.msg, s.chars().count())
        }),
        Ok(Ok(e)) => {
            // anything that reads must print, read back, and lint without panic
            match guarded(|| {
                let a = e.check(None);
                let b = e.check(Some(&[]));
                let c = e.check(Some(&["a", "b", "x"]));
                (a.len(), b.len(), c.len())
            }) {
                Ok(_) => {}
                Err(p) => t.row(false, || format!("{}: check panicked: {p}", show(s))),
            }
            match guarded(|| rt(s)) {
                Ok(Ok(())) => t.row(true, String::new),
                Ok(Err(m)) => t.row(false, || format!("{}: parses, but {m}", show(s))),
                Err(p) => t.row(false, || format!("{}: {p}", show(s))),
            }
        }
    }
}

#[test]
fn robustness_never_panics() {
    let mut t = Rows::new("robustness_never_panics");
    let mut nasty: Vec<String> = vec![
        "\0".into(), "a\0b".into(), "\u{7f}".into(), "\u{feff}a".into(), "a\u{feff}".into(), "\u{200b}a".into(), "a\u{2028}b".into(),
        "a\u{a0}b".into(), "\u{301}".into(), "e\u{301}".into(), "😀".into(), "'😀".into(), "a 😀".into(), "\u{10ffff}".into(), "'\u{10ffff}'".into(),
        "/\u{e9}".into(), "/😀".into(), "$😀".into(), "5😀".into(), "'\\😀'".into(),
        "'".repeat(1000), "\"".repeat(1000), "\\".repeat(1000), "(".repeat(1999), ")".repeat(1000), "[".repeat(1999), "]".repeat(1000),
        "not ".repeat(499), "not".repeat(600), "$".repeat(100), "/".repeat(1999), "//".repeat(999), ".".repeat(1000), ",".repeat(1000),
        "=".repeat(1000), "==".repeat(500), "!".repeat(1000), "&".repeat(1000), "|".repeat(1000), "<".repeat(1000), "<=".repeat(500),
        "a".repeat(5000), "1".repeat(5000), "9".repeat(2000), "0".repeat(2000), "0.".repeat(500), "5m".repeat(500), "1h30m".repeat(300),
        "$1".repeat(500), "'a'".repeat(600), "a.".repeat(900), "a[".repeat(900), "a]".repeat(900), "len(".repeat(400), ") ".repeat(900),
        "a and ".repeat(300), "a or ".repeat(300), "not in ".repeat(300), "in ".repeat(600), "matches ".repeat(200), " ".repeat(5000),
        "\n".repeat(3000), "\t".repeat(2000), "\r\n".repeat(900), "é".repeat(2500), "日".repeat(2500), "😀".repeat(2500),
        "'é".repeat(900), "\"😀".repeat(900), "-".repeat(1999), "-1".repeat(900), "--".repeat(900), "+".repeat(1999), "1e".repeat(900),
        "1h".repeat(900), "1h1".repeat(600), "1.".repeat(900), ".1".repeat(900), "$.".repeat(900), "$-".repeat(900), "/-".repeat(900),
        "@".repeat(1000), "#".repeat(1000), "~".repeat(1000), "%".repeat(1000), "`".repeat(1000), "{".repeat(1000), "}".repeat(1000),
        "a.b.c.d.e.f.g.h.i.j.k.l.m.n.o.p.q.r.s.t.u.v.w.x.y.z".to_string(), "a[0][0][0][0][0][0][0][0][0][0][0][0]".to_string(),
        "18446744073709551615ms".into(), "18446744073709551616ms".into(), "9223372036854775807ms".into(), "$9223372036854.775807".into(),
        "$9223372036854.775808".into(), "$9223372036855".into(), "$18446744073709551616".into(), "1.7976931348623157e308".into(),
        "179769313486231570000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000".into(),
        "0.0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001".into(),
    ];
    // every prefix and every suffix of a rich valid expression
    let rich = r#"event.data.cmd == 'rm \'x\'' and (len(event.args) > 2 or not /usage/cost_usd >= $0.0004) and last(xs)[0].a in [1, 5m, 1h30m, "q"]"#;
    let chars: Vec<char> = rich.chars().collect();
    for i in 0..=chars.len() {
        nasty.push(chars[..i].iter().collect());
        nasty.push(chars[i..].iter().collect());
    }
    // every single deletion and every single replacement of a character
    for i in 0..chars.len() {
        let mut c = chars.clone();
        c.remove(i);
        nasty.push(c.iter().collect());
        for sub in ['(', ')', '[', ']', '\'', '"', '\\', '=', '!', '$', '/', ',', '.', ' ', 'é'] {
            let mut c = chars.clone();
            c[i] = sub;
            nasty.push(c.iter().collect());
        }
    }
    for s in &nasty {
        robust(&mut t, s);
    }
    t.finish();
}

// BUG (found here first): one random string, `/p/q  . false> x.y`, reads as a `Get` of a pointer and prints back as the
// single pointer `/p/q.false` (see display_round_trip_limits_and_pointer_steps).
#[test]
fn robustness_token_soup() {
    let mut t = Rows::new("robustness_token_soup");
    let toks: &[&str] = &[
        "a",
        "b",
        "x.y",
        "a[0]",
        "len(",
        "count(",
        "last(",
        "contains(",
        ")",
        "(",
        "[",
        "]",
        ",",
        "and",
        "or",
        "not",
        "in",
        "matches",
        "==",
        "!=",
        "<",
        "<=",
        ">",
        ">=",
        "1",
        "2.5",
        "'t'",
        "\"u\"",
        "5m",
        "1h30m",
        "250ms",
        "$1.50",
        "/p/q",
        "true",
        "false",
        " ",
        ".",
        "=",
        "!",
        "&",
        "|",
        "$",
        "'",
        "\\",
        "é",
        "-3",
        "not in",
        "since(",
        "sum(",
        "same(",
        "quote(",
        "danger(",
        "lower(",
        "0",
        "-",
        "+",
        "/",
        "1h",
        "5x",
    ];
    let mut r = Rng(0xD1B5_4A32_D192_ED03);
    let mut accepted = 0usize;
    for _ in 0..30_000 {
        let n = 1 + r.below(10);
        let mut s = String::new();
        for _ in 0..n {
            s.push_str(toks[r.below(toks.len())]);
            if r.below(3) != 0 {
                s.push(' ');
            }
        }
        if matches!(try_parse(&s), Ok(Ok(_))) {
            accepted += 1;
        }
        robust(&mut t, &s);
    }
    t.row(accepted > 100, || format!("test mistake: only {accepted} of 30000 random strings were accepted"));
    t.finish();
}

#[test]
fn robustness_valid_by_construction() {
    let mut t = Rows::new("robustness_valid_by_construction");
    let mut r = Rng(0xA076_1D64_78BD_642F);
    for _ in 0..6000 {
        let d = r.below(4);
        let s = g_expr(&mut r, d);
        if s.chars().count() > SHORT {
            continue;
        }
        // every generated expression follows the grammar of the module doc, so it must be read
        if ok(&mut t, &s).is_some() {
            // and it is clean when every root is allowed and has no kind problem we know of
            round_trip(&mut t, &s);
        }
    }
    t.finish();
}

/// `[a, b]` is `[a][b]`, a key of two parts (a fold keyed by two parts is written `fold.label_rate['qc_mark', ev.pattern].p`).
#[test]
fn a_key_of_two_parts_is_two_steps() {
    let same = |a: &str, b: &str| assert_eq!(Expr::parse(a).unwrap(), Expr::parse(b).unwrap(), "{a} vs {b}");
    same("a[1, 2]", "a[1][2]");
    same("fold.rate['x', ev.p].n", "fold.rate['x'][ev.p].n");
    same("a[b, c, d]", "a[b][c][d]");
    same("/items[0, 1].name", "/items[0][1].name");
    assert_eq!(Expr::parse("a[1, 2]").unwrap().to_string(), "a[1][2]");
    for bad in ["a[1,]", "a[,1]", "a[,]", "a[1 2]", "a[1,, 2]"] {
        assert!(Expr::parse(bad).is_err(), "{bad}");
    }
}

/// The seventh decimal of an amount rounds half up (`$0.0000005` is a millionth, `$0.0000004` is none).
#[test]
fn money_rounds_at_the_seventh_decimal() {
    let micros = |t: &str| match Expr::parse(t).unwrap() {
        Expr::Lit(Value::Usd(m)) => m,
        other => panic!("{t}: {other:?}"),
    };
    for (text, want) in [
        ("$0.0000004", 0),
        ("$0.0000005", 1),
        ("$0.0000006", 1),
        ("$0.0000015", 2),
        ("$1.0000005", 1_000_001),
        ("$1.9999995", 2_000_000),
        ("$0.0000009999", 1),
        ("$0.1234565", 123_457),
        ("$0.1234564", 123_456),
    ] {
        assert_eq!(micros(text), want, "{text}");
    }
}
