//! Attacks on Expr / Fmt / Query through the public API. Every test states what the contract (the module docs of
//! `src/expr.rs`, `src/fmt.rs`, `src/query.rs`, `src/value.rs`) says; a test that FAILS is an attack that worked.
//! Tests named `fN_…` are findings (they fail today); the rest are attempts that held (they pass and stay as a net).

use j4n_core::eval::{Env, Scope, glob_match};
use j4n_core::expr::{Expr, Func, MAX_DEPTH, MAX_LEN, Op, QUOTE_MAX};
use j4n_core::fmt::{Fmt, Format, MAX_FMT, SHORT_MAX, duration, format_value};
use j4n_core::query::{Coll, MAX_LIMIT, MAX_ORDER, Query};
use j4n_core::value::{DASH, Key, Value};
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

// ── helpers ──────────────────────────────────────────────────────────────────────────────────

fn map(pairs: &[(&str, Value)]) -> Value {
    Value::Map(pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect::<BTreeMap<_, _>>())
}

fn ex(src: &str) -> Expr {
    Expr::parse(src).unwrap_or_else(|e| panic!("`{src}` should parse: {e}"))
}

fn world() -> Scope {
    Scope::new()
        .with("a", Value::Num(3.0))
        .with("l", Value::List(vec![Value::Num(1.0), Value::Num(2.0), Value::Num(3.0)]))
        .with("m", map(&[("1", "one".into()), ("k", "kay".into()), ("in", "IN".into())]))
        .with("s", Value::text("hello"))
        .with("hole", Value::List(vec![Value::Num(1.0), Value::Missing, Value::Num(2.0)]))
        .doc(map(&[
            ("a", map(&[("b", Value::Num(7.0))])),
            ("x~y", Value::Num(9.0)),
            ("p/q", Value::Num(10.0)),
            ("a~1b", Value::Num(11.0)),
        ]))
        .at_ms(1_000_000)
        .danger_with(|c| Value::text(c.to_string()))
}

fn val(src: &str) -> Value {
    ex(src).eval(&world())
}

fn val_in(src: &str, sc: &Scope) -> Value {
    ex(src).eval(sc)
}

fn text_of(v: Value) -> String {
    match v {
        Value::Str(s) => s,
        other => panic!("expected text, got {other:?}"),
    }
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

// ── Display claims "prints one that parses back to the same tree" ────────────────────

/// `Display` wraps a `not` under a `not` in parentheses, and every pair of parentheses costs one more level, so a
/// chain of 13..23 `not`s that parses prints as text that is refused (MAX_DEPTH).
#[test]
fn display_of_a_not_chain_parses_back() {
    for n in [13, 23] {
        let src = format!("{}a", "not ".repeat(n));
        let e = ex(&src);
        let shown = e.to_string();
        let back =
            Expr::parse(&shown).unwrap_or_else(|err| panic!("{n} nots print as {shown:?}, which is refused: {err}"));
        assert_eq!(back, e);
    }
}

/// `.` is a pointer character, so `/a .b` is refused (it would print as the different pointer `/a.b`); after an
/// index step a dot step is fine.
#[test]
fn display_of_a_pointer_then_a_key_keeps_its_meaning() {
    assert!(Expr::parse("/a .b").is_err());
    let sc = Scope::new().doc(map(&[("a", Value::List(vec![map(&[("b", Value::Num(7.0))])]))]));
    let e = ex("/a[0].b");
    assert_eq!(e.eval(&sc), Value::Num(7.0));
    assert_eq!(ex(&e.to_string()), e);
}

/// `Display` puts spaces around operators, so an expression that is under MAX_LEN can print longer than MAX_LEN and
/// then not parse back (same for a template: `{a==b}` prints as `{a == b}`).
#[test]
fn display_of_a_long_expression_or_template_parses_back() {
    let src = format!("[{}]", vec!["a==a"; 399].join(","));
    assert!(src.chars().count() <= MAX_LEN);
    let e = ex(&src);
    let shown = e.to_string();
    // documented: the printed spaces can carry an expression near MAX_LEN past it; then it is refused by length alone
    assert!(
        Expr::parse(&shown).is_ok() || shown.chars().count() > MAX_LEN,
        "{} chars in, {} chars printed",
        src.chars().count(),
        shown.chars().count()
    );

    let tpl = "{a==b}".repeat(330);
    assert!(tpl.chars().count() <= MAX_LEN);
    let f = Fmt::parse(&tpl).unwrap();
    let shown = f.to_string();
    assert!(
        Fmt::parse(&shown).is_ok(),
        "template: {} chars in, {} chars printed, refused",
        tpl.chars().count(),
        shown.chars().count()
    );
}

// ── Value::sort_cmp is documented as "a total order"; Query::apply sorts with it ─────

/// A number and money are compared through a rounding to millionths, a number and a number exactly: `0.0000004` is
/// "equal" to `$0` and `$0` is "equal" to `0`, but `0.0000004 > 0`. Equality is not transitive, so a sort is wrong.
#[test]
fn sorting_numbers_and_money_keeps_plain_numbers_in_order() {
    let q = Query::parse("tasks", None, Some("k"), None).unwrap();
    let row = |v: Value| map(&[("k", v)]);
    let sorted = q.apply(vec![row(Value::Num(0.4e-6)), row(Value::Usd(0)), row(Value::Num(0.0))], &Scope::new());
    let k = |r: &Value| r.at(&[Key::Name("k".into())]);
    let pos = |want: Value| sorted.iter().position(|r| k(r) == want).unwrap();
    assert!(
        pos(Value::Num(0.0)) < pos(Value::Num(0.4e-6)),
        "0 must sort before 0.0000004, got {:?}",
        sorted.iter().map(k).collect::<Vec<_>>()
    );
}

/// With enough rows the standard library notices the broken order and PANICS ("user-provided comparison function
/// does not correctly implement a total order"): `Query::apply` is not total. (Needs only finite values.)
#[test]
fn sorting_many_rows_of_numbers_and_money_does_not_panic() {
    let q = Query::parse("tasks", None, Some("k"), None).unwrap();
    let vals: Vec<Value> = (0..12).flat_map(|i| [Value::Num(i as f64 * 0.4e-6), Value::Usd(i)]).collect();
    let mut r = Rng(777);
    for round in 0..40 {
        let n = 30 + r.below(2000);
        let rows: Vec<Value> = (0..n).map(|_| map(&[("k", vals[r.below(vals.len())].clone())])).collect();
        let out = catch_unwind(AssertUnwindSafe(|| q.apply(rows, &Scope::new())));
        assert!(out.is_ok(), "Query::apply panicked in round {round} with {n} rows");
    }
}

/// The same cause at the `Value` level: `sort_cmp` is not transitive over finite values.
#[test]
fn sort_cmp_is_transitive_over_finite_values() {
    use std::cmp::Ordering::Greater;
    let z = [
        Value::Num(0.0),
        Value::Num(0.4e-6),
        Value::Num(0.6e-6),
        Value::Usd(0),
        Value::Usd(1),
        Value::Num(1e-6),
        Value::Usd(-1),
        Value::Num(-0.4e-6),
    ];
    let le = |x: &Value, y: &Value| x.sort_cmp(y) != Greater;
    for a in &z {
        for b in &z {
            for c in &z {
                assert!(!(le(a, b) && le(b, c)) || le(a, c), "{a:?} <= {b:?} <= {c:?} but {a:?} > {c:?}");
            }
        }
    }
}

// ── quote: "text made inert for a template: one line in double quotes" ───────────────

/// U+2028 / U+2029 are line separators for many consumers (JS, Python `splitlines`, editors, some LLM front ends)
/// and `char::is_control` is false for them, so `quote` keeps them: the quoted text is not one line.
#[test]
fn quote_is_one_line_for_every_line_break() {
    for c in ['\u{2028}', '\u{2029}', '\u{85}', '\n', '\r', '\u{b}', '\u{c}'] {
        let sc = Scope::new().with("t", Value::text(format!("a{c}b")));
        let out = text_of(val_in("quote(t)", &sc));
        assert!(!out.contains(c), "{c:?} survived quote: {out:?}");
    }
}

/// Format characters survive `quote` and the 200-char budget counts them: tag characters U+E0000..E007F (the
/// invisible "ASCII smuggling" channel into a model), zero-width and bidi controls, BOM. PLAN §3.2 makes `quote` the ONLY
/// way model text may enter a steer/notice template.
#[test]
fn quote_removes_invisible_characters() {
    let bad = ['\u{e0041}', '\u{200b}', '\u{200d}', '\u{202e}', '\u{2066}', '\u{feff}', '\u{2060}'];
    let mut survivors = Vec::new();
    for c in bad {
        let sc = Scope::new().with("t", Value::text(format!("a{c}b")));
        if text_of(val_in("quote(t)", &sc)).contains(c) {
            survivors.push(format!("U+{:04X}", c as u32));
        }
    }
    assert!(survivors.is_empty(), "invisible characters pass through quote: {survivors:?}");
}

// ── Fmt formatters ──────────────────────────────────────────────────────────────────

/// `short` is "one line, at most 40 characters" but only text and numbers are shortened; a list or a map is printed
/// whole, newlines and all.
#[test]
fn short_is_one_line_and_at_most_40_for_every_kind() {
    let long = Value::text("x".repeat(500));
    let multi = Value::text("a\nb");
    for v in
        [Value::List(vec![long.clone()]), Value::List(vec![multi.clone()]), map(&[("k", long)]), map(&[("k", multi)])]
    {
        let out = format_value(&v, Some(Format::Short));
        assert!(
            !out.contains('\n') && out.chars().count() <= SHORT_MAX,
            "{v:?} as short is {} chars: {out:?}",
            out.chars().count()
        );
    }
}

/// `compact` casts the scaled number to `i64`; from about 9.2e27 on it saturates and prints nonsense.
#[test]
fn short_of_a_huge_number_is_not_saturated_nonsense() {
    for n in [1e30, -1e30, 1e300] {
        let out = format_value(&Value::Num(n), Some(Format::Short));
        assert!(!out.contains("9223372036854775807"), "{n:e} as short is {out:?}");
    }
}

/// `short` keeps the first non-empty line and drops the rest without the `…` that "marks a cut".
#[test]
fn short_marks_dropped_lines() {
    let out = format_value(&Value::text("first\nsecond"), Some(Format::Short));
    assert!(out.ends_with('…'), "later lines were dropped silently: {out:?}");
}

// ── money and durations: "money compares and adds exactly", "kept to a millionth" ──

/// `sum` saturates instead of saying it cannot add: two big sums of money come out as `i64::MAX` (a made-up
/// amount), the same for durations.
#[test]
fn sum_that_overflows_is_missing_not_a_wrong_amount() {
    assert_eq!(val("sum([$8000000000000, $8000000000000])"), Value::Missing, "$16e12 is not $9.22e12");
    let sc = Scope::new().with("d", Value::List(vec![Value::Dur(u64::MAX), Value::Dur(5)]));
    assert_eq!(val_in("sum(d)", &sc), Value::Missing, "u64::MAX ms + 5 ms is not u64::MAX ms");
}

/// A number is turned into money by a saturating `as i64`, so near the top every number above $9.22e12 "equals"
/// the largest money, and `<` is wrong.
#[test]
fn money_against_a_huge_number_compares_by_value() {
    let sc = Scope::new().with("top", Value::Usd(i64::MAX));
    assert!(!val_in("top == 1000000000000000000000000000000", &sc).truthy(), "$9.2e12 is not 1e30");
    assert!(val_in("top < 1000000000000000000000000000000", &sc).truthy());
    assert!(val_in("top < 9300000000000", &sc).truthy(), "$9.223e12 < $9.3e12");
}

/// The amount is read as an `f64` of dollars, so above about $4e9 the last micro is lost: `$8589934592.000001` is
/// kept as 8589934592.000002.
#[test]
fn a_money_literal_is_kept_to_a_millionth() {
    assert_eq!(val("$8589934592.000001"), Value::Usd(8_589_934_592_000_001));
}

/// The same loss makes `Display` of a money literal unstable: `$4394277888.725266` is read as ...725268 and printed
/// as `$4394277888.725267`, which is read as ...725268 again; the printed text is not what was read.
#[test]
fn a_money_literal_prints_and_parses_back() {
    let e = ex("$4394277888.725266");
    assert_eq!(e, Expr::Lit(Value::Usd(4_394_277_888_725_266)), "read as {e:?}");
    assert_eq!(ex(&e.to_string()), e, "printed as {}", e);
}

/// `Value::Num` is a public variant, so a non-finite number can be handed in by an `Env`; every formatter says a
/// dash for it except `short`, which prints `infB`, and `dur`, which prints a saturated 213503982334d14h for infinity.
#[test]
fn formatters_agree_on_a_non_finite_number() {
    for n in [f64::INFINITY, f64::NEG_INFINITY] {
        for f in Format::ALL {
            let out = format_value(&Value::Num(n), Some(f));
            assert!(out == DASH || !out.contains("inf") && !out.contains("213503982334d"), "{n} as {f:?} is {out:?}");
        }
    }
}

// ── the module docs say one thing, the code another (doc-ambiguity: read as written) ──

/// "MAX_DEPTH levels of nesting": 24 levels of parentheses are refused, 23 pass (the outermost `or` counts).
#[test]
fn max_depth_levels_of_nesting_are_allowed() {
    for (what, make) in [
        ("parentheses", (|n: usize| format!("{}a{}", "(".repeat(n), ")".repeat(n))) as fn(usize) -> String),
        ("brackets", |n| format!("{}1{}", "[".repeat(n), "]".repeat(n))),
        ("calls", |n| format!("{}a{}", "len(".repeat(n), ")".repeat(n))),
        ("not", |n| format!("{}a", "not ".repeat(n))),
    ] {
        assert!(Expr::parse(&make(MAX_DEPTH)).is_ok(), "{MAX_DEPTH} levels of {what} should be the deepest allowed");
        assert!(Expr::parse(&make(MAX_DEPTH + 1)).is_err(), "{} levels of {what} should be refused", MAX_DEPTH + 1);
    }
}

/// "Every mistake is reported at once": only the first bad `order` key is reported.
#[test]
fn query_parse_reports_every_mistake_in_order() {
    let problems = Query::parse("tasks", None, Some("a ==, b =="), None).unwrap_err();
    assert_eq!(problems.len(), 2, "two bad keys, got {problems:?}");
}

/// A word is a key after a dot (`a.in`, `m.and`): JSON keys named like words are common.
#[test]
fn words_cannot_be_names_after_a_dot_either() {
    for w in ["and", "or", "not", "in", "matches", "true", "false"] {
        assert!(Expr::parse(&format!("a.{w}")).is_ok(), "`a.{w}` should parse: the word is a key after a dot");
    }
}

/// A whole number reads a map's key written with its digits, and a text that is a plain whole number reads a list
/// position.
#[test]
fn index_kinds_are_as_documented() {
    assert_eq!(val("m[1]"), Value::text("one"), "a number into a map");
    assert_eq!(val("l['1']"), Value::Num(2.0), "text into a list");
    assert_eq!(val("l['01']"), Value::Missing, "not a plain number");
}

/// Lists take a trailing comma, calls do not.
#[test]
fn trailing_comma_is_refused_the_same_everywhere() {
    assert!(Expr::parse("count(l,)").is_err());
    assert!(Expr::parse("[1,]").is_ok(), "a list takes a trailing comma, a call does not");
}

/// "Nothing compares with nothing" and "`!=` is yes only when both sides are there": a list holding a missing item
/// is neither `==` nor `!=` itself... it is `!=` itself. A rule `when = "task.tags != prev.tags"` fires on identical data.
#[test]
fn a_list_with_a_missing_item_is_not_unequal_to_itself() {
    assert!(!val("hole == hole").truthy());
    assert!(!val("hole != hole").truthy(), "`hole == hole` is false and `hole != hole` is true");
    assert!(!val("[1, nope] != [1, nope]").truthy());
}

/// `$.5` is money but `.5` is not a number, and `/a/b+1` looks like arithmetic ("there is no arithmetic") but is a
/// pointer to the key `b+1`.
#[test]
fn odd_literals_follow_the_grammar() {
    assert!(Expr::parse("$.5").is_err(), "`$.5`: the grammar says `$1.50`");
    assert!(Expr::parse("/a/b+1").is_ok(), "`+` and `-` are pointer characters: this is a pointer to the key \"b+1\"");
}

// ── robustness / spec gaps ─────────────────────────────────────────────────────────

/// Error messages echo the offending character raw, so a pack file can put an ESC or a bidi override into text a
/// TUI prints.
#[test]
fn parse_errors_do_not_echo_control_characters() {
    for src in ["a\u{1b}[2J", "a\u{202e}", "'x\\\u{1b}'", "\u{7}"] {
        let err = Expr::parse(src).unwrap_err();
        let shown = err.to_string();
        assert!(!shown.chars().any(|c| c.is_control() || c == '\u{202e}'), "{src:?} -> {shown:?}");
    }
}

/// `.` and `..` are valid collection names (the character set allows `.`), so `table:..` / `fold:..` / `thread:..`
/// parse; a resolver that maps a name to a path walks out of its directory.
#[test]
fn collection_names_cannot_be_dot_or_dot_dot() {
    for from in ["table:.", "table:..", "fold:..", "thread:..", "log:.."] {
        assert!(Coll::parse(from).is_err(), "`{from}` is accepted");
    }
}

/// "a name is first a field of the row, then a root": a row (data) can shadow any outer root, so a row with its own
/// `fold` field answers `fold.spend` instead of the real fold, and the real one cannot be reached.
#[test]
fn a_row_field_cannot_shadow_an_outer_root() {
    let q = Query::parse("tasks", Some("fold.spend > 1"), None, None).unwrap();
    let outer = Scope::new().with("fold", map(&[("spend", Value::Num(0.0))]));
    let liar = map(&[("fold", map(&[("spend", Value::Num(100.0))]))]);
    assert!(q.apply(vec![liar], &outer).is_empty(), "the row's own `fold` decided the filter");
}

/// `matches` has no way to write a literal `*` (documented: `?` matches it).
#[test]
fn glob_has_an_escape_for_star() {
    assert!(!glob_match("a\\*b", "a*b"), "a backslash is itself");
    assert!(glob_match("a?b", "a*b"), "`?` matches the character `*`");
}

/// Worst-case glob work is pattern x text (documented): 100k characters of data and a 500-character pattern finish
/// well inside a second.
#[test]
fn glob_work_is_bounded_by_the_pattern() {
    let text = "a".repeat(100_000);
    let pat = format!("*{}b", "a".repeat(500));
    let t = crate::cpu::start();
    assert!(!glob_match(&pat, &text));
    crate::cpu::assert_within(std::time::Duration::from_secs(20), "a glob that backtracks", || t.elapsed());
}

/// The module docs of `expr.rs` link `Env::get`, `Env::pointer`, `Env::now_ms`, `Env::danger` but `Env` is not in
/// scope there (`cargo doc` warns four times). Nothing to run: the check is `scripts/t doc -p j4n-core --no-deps`.
#[test]
fn doc_links_resolve() {
    let src = include_str!("../../src/expr.rs");
    assert!(
        src.contains("use crate::eval::Env") || !src.contains("[`Env::"),
        "expr.rs links [`Env::…`] without importing Env"
    );
}

// ═════════════════════════════════════════════════════════════════════════════════════════════
// Attempts that held. They stay as a net.
// ═════════════════════════════════════════════════════════════════════════════════════════════

#[test]
fn numbers_followed_by_letters_are_refused() {
    for src in ["5mm", "1e3", "0x10", "1_000", "5é", "5m3", "5m 3s", "1.5.5h", "-5m", "$1e3", "5.", "1.e3", "1d1"] {
        assert!(Expr::parse(src).is_err(), "`{src}` parsed");
    }
    assert_eq!(val("1h1h"), Value::Dur(7_200_000));
    assert_eq!(val("5ms5s"), Value::Dur(5005));
}

#[test]
fn dollar_forms() {
    for src in ["$", "$ 5", "$a", "$-5", "$5.", "$5m", "$$5"] {
        assert!(Expr::parse(src).is_err(), "`{src}` parsed");
    }
    assert_eq!(val("$9000000000000"), Value::Usd(9_000_000_000_000_000_000));
    assert!(Expr::parse("$9223372036854").is_err());
    assert!(Expr::parse(&format!("${}", "9".repeat(400))).is_err(), "inf dollars");
    assert_eq!(val("$0.0000004"), Value::Usd(0));
}

#[test]
fn unicode_identifiers_and_digits_are_refused() {
    for src in ["é", "naïve", "x\u{200b}y", "ａ", "１", "٣", "a.é", "m['k'].é", "\u{feff}a", "a\u{0}", "/ä", "日本"]
    {
        assert!(Expr::parse(src).is_err(), "`{src:?}` parsed");
    }
}

/// A text literal for `body`, written the way the language wants it.
fn lit_expr(body: &str) -> Expr {
    let mut s = String::from("'");
    for c in body.chars() {
        if c == '\\' || c == '\'' {
            s.push('\\');
        }
        s.push(c);
    }
    s.push('\'');
    ex(&s)
}

#[test]
fn strings_hold_odd_characters_and_roundtrip() {
    for body in
        ["\u{0}", "\u{feff}", "\u{2028}", "\u{85}", "é", "\u{1b}[0m", "{x}", "}}", "it's", "back\\slash", "a\nb\tc\rd"]
    {
        let e = lit_expr(body);
        assert_eq!(e.eval(&Scope::new()), Value::text(body));
        let shown = e.to_string();
        assert_eq!(ex(&shown), e, "{body:?} printed as {shown:?}");
    }
    assert!(Expr::parse("'a\\0'").is_err() && Expr::parse("'a\\u'").is_err() && Expr::parse("'a\\").is_err());
}

#[test]
fn length_limits_count_characters() {
    let ok = format!("'{}'", "é".repeat(MAX_LEN - 2));
    assert_eq!(ok.chars().count(), MAX_LEN);
    assert!(Expr::parse(&ok).is_ok());
    assert!(Expr::parse(&format!("{ok} ")).is_err());
    assert!(Fmt::parse(&"é".repeat(MAX_FMT)).is_ok());
    assert!(Fmt::parse(&"é".repeat(MAX_FMT + 1)).is_err());
}

#[test]
fn list_literals() {
    assert_eq!(val("[[],[]]"), Value::List(vec![Value::List(vec![]), Value::List(vec![])]));
    assert_eq!(val("len([[],[]])"), Value::Num(2.0));
    for src in ["[,]", "[1,,2]", "[", "[1", "[1 2]", "[1;2]"] {
        assert!(Expr::parse(src).is_err(), "`{src}` parsed");
    }
}

#[test]
fn indexes_that_are_huge_fractional_negative_or_missing_lead_nowhere() {
    for src in [
        "l[1.5]",
        "l[-1]",
        "l[9000000000000000]",
        "l[9007199254740991]",
        "l[nope]",
        "l[true]",
        "l[1h]",
        "l[[0]]",
        "m[-1]",
        "m[1.5]",
        "l[0].x",
        "l[3]",
        "a.b",
        "s.length",
        "m['nope'].x",
    ] {
        assert_eq!(val(src), Value::Missing, "`{src}`");
    }
    assert_eq!(val("l[nope] == l[nope]"), Value::Bool(false));
    assert!(val("not l[nope]").truthy());
    // the env is not even asked for the root when an index is missing
    struct Spy(std::cell::Cell<u32>);
    impl Env for Spy {
        fn get(&self, root: &str, _: &[Key]) -> Value {
            if root == "r" {
                self.0.set(self.0.get() + 1);
                return Value::Num(1.0);
            }
            Value::Missing
        }
    }
    let spy = Spy(Default::default());
    assert_eq!(ex("r[nope]").eval(&spy), Value::Missing);
    assert_eq!(spy.0.get(), 0);
    assert_eq!(ex("r[r]").eval(&spy), Value::Num(1.0), "this spy answers 1 to every path");
    assert_eq!(spy.0.get(), 2, "r[r] asks for the index, then for the value");
}

#[test]
fn pointer_corners() {
    let sc = world();
    let p = |ptr: &str| sc.pointer(ptr);
    assert_eq!(p("/a/b"), Value::Num(7.0));
    assert_eq!(p("/x~0y"), Value::Num(9.0));
    assert_eq!(p("/p~1q"), Value::Num(10.0));
    assert_eq!(p("/a~01b"), Value::Num(11.0), "~01 is ~1, not /");
    for bad in ["/a/", "//", "/a//b", "a/b", "/x~", "/x~2", "/%61", "/nope", "/a/b/c"] {
        assert_eq!(p(bad), Value::Missing, "{bad:?}");
    }
    assert_eq!(val("/a/b"), Value::Num(7.0));
    assert_eq!(val("/a/b == 7 and /a/b >= 7"), Value::Bool(true));
    assert!(matches!(val("/"), Value::Map(_)));
    assert_eq!(val("/ == /"), Value::Bool(true));
    for bad in ["/ä", "/a b", "a/b", "5/2"] {
        assert!(Expr::parse(bad).is_err(), "{bad:?}");
    }
    let list = Value::List(vec!["z".into()]);
    assert_eq!(list.pointer("/0"), "z".into());
    for bad in ["/00", "/-", "/+0", "/-1", "/18446744073709551616", "/٣"] {
        assert_eq!(list.pointer(bad), Value::Missing, "{bad:?}");
    }
}

#[test]
fn functions_over_the_wrong_kind_answer_missing() {
    for src in [
        "count(a)",
        "count(s)",
        "sum(s)",
        "sum(a)",
        "sum(nested)",
        "sum([1, true])",
        "sum([$1, 2])",
        "sum([1, 1s])",
        "len(a)",
        "len(true)",
        "len(nope)",
        "lower(a)",
        "last(a)",
        "since(s)",
        "since(5s)",
        "danger(a)",
        "quote(l)",
        "quote(nope)",
        "quote(m)",
    ] {
        let sc = world().with("nested", Value::List(vec![Value::List(vec![]), Value::Num(1.0)]));
        assert_eq!(val_in(src, &sc), Value::Missing, "`{src}`");
    }
    assert_eq!(val("count(nope)"), Value::Num(0.0));
    assert_eq!(val("sum(nope)"), Value::Num(0.0));
    assert_eq!(val("sum(hole)"), Value::Num(3.0));
    assert_eq!(val("count(hole)"), Value::Num(2.0));
    assert_eq!(val("same([])"), Value::Bool(false));
    assert_eq!(val("last([])"), Value::Missing);
    assert_eq!(val("len('é')"), Value::Num(1.0));
    // a float that overflows is missing, not inf
    let sc = Scope::new().with("big", Value::List(vec![Value::Num(f64::MAX), Value::Num(f64::MAX)]));
    assert_eq!(val_in("sum(big)", &sc), Value::Missing);
}

#[test]
fn since_edge_cases_are_clamped_not_wrong_kinds() {
    assert_eq!(val("since(1000)"), Value::Dur(999_000));
    assert_eq!(val("since(-5)"), val("since(0)"), "before 1970 counts from 1970");
    assert_eq!(val("since(1000.9)"), Value::Dur(999_000), "a fraction is cut");
    assert_eq!(val("since(99999999999)"), Value::Dur(0), "a time in the future is 0s");
    assert_eq!(ex("since(1)").eval(&Scope::new()), Value::Missing, "no clock, no answer");
}

#[test]
fn quote_boundaries_and_kinds() {
    for (n, ellipsis) in [(199, false), (200, false), (201, true), (5000, true)] {
        let sc = Scope::new().with("t", Value::text("x".repeat(n)));
        let out = text_of(val_in("quote(t)", &sc));
        assert!(out.starts_with('"') && out.ends_with('"'));
        assert_eq!(out.contains('…'), ellipsis, "{n} chars");
        let inner = out.trim_matches('"').trim_end_matches('…');
        assert!(inner.chars().count() <= QUOTE_MAX, "{n} chars kept {}", inner.chars().count());
    }
    let sc = Scope::new().with("t", Value::text("\u{1b}\u{7}\u{0}\n\r\t\u{7f}\u{9b}"));
    assert_eq!(text_of(val_in("quote(t)", &sc)), format!("\"{}\"", " ".repeat(8)));
    let sc = Scope::new().with("t", Value::text("\\\"\\\\\"")).with("e", Value::text(""));
    let out = text_of(val_in("quote(t)", &sc));
    assert_eq!(
        out,
        format!("\"{}\"", ["\\\\", "\\\"", "\\\\", "\\\\", "\\\""].concat()),
        "each backslash and quote gets a backslash"
    );
    assert_eq!(text_of(val_in("quote(e)", &sc)), "\"\"");
    for (src, want) in
        [("quote(a)", "\"3\""), ("quote(true)", "\"true\""), ("quote(5m)", "\"5m00s\""), ("quote($1)", "\"$1.00\"")]
    {
        assert_eq!(text_of(val(src)), want);
    }
    // a hundred quotes escape to two hundred characters but are cut by the INPUT count
    let sc = Scope::new().with("t", Value::text("\"".repeat(300)));
    assert_eq!(text_of(val_in("quote(t)", &sc)).chars().count(), 2 + 2 * QUOTE_MAX + 1);
}

#[test]
fn caps_of_durations_and_money_do_not_overflow() {
    assert_eq!(val("1000000000000000ms"), Value::Dur(1_000_000_000_000_000));
    for src in [
        "1000000000000001ms".to_string(),
        "11574075d".into(),
        "11574074d23h".into(),
        format!("{}1ms", "1000000ms".repeat(1000)),
        "1d".repeat(900),
    ] {
        let _ = Expr::parse(&src); // either way, no panic
    }
    assert!(Expr::parse(&"11574074d".repeat(2)).is_err());
    assert!(Expr::parse(&format!("{}d", "9".repeat(300))).is_err());
    assert!(Expr::parse(&format!("{}d", "9".repeat(309))).is_err(), "inf days");
    assert!(Expr::parse("$8999999999999").is_ok());
    assert_eq!(val("$9000000000000"), Value::Usd(9_000_000_000_000_000_000));
    assert!(Expr::parse("$9223372036854").is_err());
    assert_eq!(Value::usd(f64::MAX), Value::Missing);
    assert_eq!(Value::usd(f64::NAN), Value::Missing);
    assert_eq!(Value::usd(9.0e12), Value::Usd(9_000_000_000_000_000_000));
    assert_eq!(Value::usd(9.3e12), Value::Missing);
    assert_eq!(Value::usd(-8.9e12), Value::Usd(-8_900_000_000_000_000_000));
    assert_eq!(Value::num(f64::INFINITY), Value::Missing);
    assert_eq!(Value::from(f64::NAN), Value::Missing);
    // printing the extremes
    assert_eq!(format_value(&Value::Usd(i64::MIN), None), "-$9223372036854.78");
    assert_eq!(format_value(&Value::Usd(i64::MAX), Some(Format::Usd)), "$9223372036854.78");
    assert_eq!(format_value(&Value::Dur(u64::MAX), None), "213503982334d14h");
    assert_eq!(duration(u64::MAX), "213503982334d14h");
    assert_eq!(format_value(&Value::Num(f64::MAX), Some(Format::Dur)), "213503982334d14h");
    assert_eq!(format_value(&Value::Num(f64::MAX), Some(Format::Usd)), DASH);
    assert_eq!(format_value(&Value::Num(f64::MAX), Some(Format::Pct)), DASH);
    let shown = Expr::parse("$0.0004").unwrap().to_string();
    assert_eq!(shown, "$0.0004");
    assert_eq!(val("1h30m5s250ms").kind(), "a duration");
}

#[test]
fn keywords_as_call_names_and_names_as_roots() {
    for src in ["in(x)", "and(x)", "matches(x)", "true(x)", "nofunc(x)", "COUNT(l)", "l.count(x)", "count(l)(x)"] {
        assert!(Expr::parse(src).is_err(), "`{src}` parsed");
    }
    // function names are fine as roots, and calls work with spaces and the arity is enforced
    let sc = Scope::new().with("count", map(&[("x", Value::Num(1.0))])).with("l", Value::List(vec![Value::Num(1.0)]));
    assert_eq!(val_in("count.x", &sc), Value::Num(1.0));
    assert_eq!(val_in("count (l)", &sc), Value::Num(1.0));
    for f in Func::ALL {
        let (lo, hi) = f.arity();
        let call = |n: usize| format!("{}({})", f.word(), vec!["a"; n].join(","));
        for n in 0..=hi + 1 {
            assert_eq!(Expr::parse(&call(n)).is_ok(), (lo..=hi).contains(&n), "{}", call(n));
        }
    }
}

#[test]
fn comparison_corners() {
    for src in [
        "1 < 2 < 3",
        "a == b == c",
        "a == b in c",
        "a not b",
        "not in",
        "a in not b",
        "== 3",
        "a ==",
        "a = 1",
        "a != = 1",
        "a ! b",
        "a & b",
        "a | b",
        "a <> b",
        "a === b",
        "a !== b",
    ] {
        assert!(Expr::parse(src).is_err(), "`{src}` parsed");
    }
    for (src, want) in [
        ("1 == 1.0", true),
        ("'1' != 1", true),
        ("'1' == 1", false),
        ("true == 1", false),
        ("true != 1", true),
        ("nope == nope", false),
        ("nope != 1", false),
        ("nope < 1", false),
        ("5s < 6s", true),
        ("5 < 6s", false),
        ("$1 < $2", true),
        ("$1 == 1", true),
        ("[1,2] == [1,2]", true),
        ("[1,2] == [2,1]", false),
        ("[1] < [2]", false),
        ("'' in 'abc'", true),
        ("'b' in 'abc'", true),
        ("'z' in 'abc'", false),
        ("1 in 5", false),
        ("'k' in m", true),
        ("1 in m", false),
        ("2.0 in l", true),
        ("'é' < 'f'", false),
        ("'a' < 'b'", true),
        ("true < false", false),
        ("5 matches '5'", false),
        ("'ab' matches 'a?'", true),
        ("'a/b.c' matches 'a*c'", true),
        ("'Ab' matches 'a*'", false),
        ("s not in l", true),
        ("not (1 == 2)", true),
        ("0s", false),
        ("$0", false),
        ("[]", false),
        ("'0'", true),
        ("-0", false),
    ] {
        assert_eq!(val(src).truthy(), want, "`{src}`");
    }
}

#[test]
fn eval_is_pure_and_repeatable() {
    let sc = world();
    for src in ["since(1000)", "danger('x')", "l[0]", "quote(s)", "m.in", "/a/b"] {
        let e = ex(src);
        assert_eq!(e.eval(&sc), e.eval(&sc), "{src}");
    }
    // evaluation does not change the scope it reads
    let before = format!("{sc:?}");
    for src in ["l", "m", "hole", "sum(l)"] {
        let _ = val_in(src, &sc);
    }
    assert_eq!(before, format!("{sc:?}"));
}

#[test]
fn fmt_corners() {
    let sc = world();
    let render = |t: &str| Fmt::parse(t).unwrap_or_else(|e| panic!("{t:?}: {e}")).render(&sc);
    assert_eq!(render("{{}}"), "{}");
    assert_eq!(render("{{{a}}}"), "{3}");
    assert_eq!(render("{a}}}"), "3}");
    assert_eq!(render("{lower('{a}')}"), "{a}");
    assert_eq!(render("{lower('}')}"), "}");
    assert_eq!(render("{ 'a:b' }"), "a:b");
    assert_eq!(render("{'a:b':text}"), "a:b");
    assert_eq!(render("{/a/b:n}"), "7");
    assert_eq!(render("{ a : usd }"), "$3.00");
    assert_eq!(render("{\"}\"}"), "}");
    assert_eq!(render("{'it\\'s }'}"), "it's }");
    assert_eq!(render("x\u{0}y\u{feff}{a}\u{2028}"), "x\u{0}y\u{feff}3\u{2028}");
    assert_eq!(render(""), "");
    for bad in [
        "{",
        "}",
        "{{a}",
        "{a",
        "a}",
        "{}",
        "{ }",
        "{:usd}",
        "{a:}",
        "{a:USD}",
        "{a:usd:pct}",
        "{ {a} }",
        "{a b}",
        "{é}",
        "{/a:b}",
        "{'}",
        "{a == '}",
        "{{a}}}",
    ] {
        assert!(Fmt::parse(bad).is_err(), "{bad:?} parsed");
    }
    // the error position is a character count (1-based in the message)
    let e = Fmt::parse("é{a:zz}").unwrap_err();
    assert_eq!(e.at, 4, "{e}");
    // holes only in holes
    let f = Fmt::parse("{a}{b}{a}").unwrap();
    assert_eq!(f.pieces().len(), 3);
    assert_eq!(f.check(Some(&["a"])).len(), 1, "one unknown root");
    assert!(Fmt::parse("{a:usd}").unwrap().check(None).is_empty());
}

#[test]
fn formatters_over_every_kind_never_panic_and_give_words() {
    let z = [
        Value::Missing,
        Value::Bool(true),
        Value::Num(0.0),
        Value::Num(-0.0),
        Value::Num(0.0049),
        Value::Num(-0.0049),
        Value::Num(999.999),
        Value::Num(1e15),
        Value::Num(f64::MAX),
        Value::Num(f64::MIN),
        Value::Num(f64::MIN_POSITIVE),
        Value::text("é".repeat(100)),
        Value::text("   "),
        Value::Dur(0),
        Value::Dur(999),
        Value::Dur(1000),
        Value::Dur(86_399_999),
        Value::Dur(u64::MAX),
        Value::Usd(0),
        Value::Usd(-1),
        Value::Usd(49),
        Value::Usd(50),
        Value::Usd(4_999),
        Value::Usd(5_000),
        Value::Usd(994_999),
        Value::Usd(995_000),
        Value::Usd(i64::MAX),
        Value::Usd(i64::MIN),
        Value::List(vec![]),
        Value::List(vec![Value::Missing, Value::Usd(1)]),
        map(&[("k", Value::Dur(5))]),
    ];
    for v in &z {
        for f in std::iter::once(None).chain(Format::ALL.into_iter().map(Some)) {
            let out = format_value(v, f);
            assert!(!out.is_empty(), "{v:?} as {f:?} is empty");
        }
    }
    assert_eq!(format_value(&Value::Usd(995_000), None), "$1.00");
    assert_eq!(format_value(&Value::Usd(994_999), None), "$0.99");
    assert_eq!(format_value(&Value::Usd(4_999), None), "$0.0050");
    assert_eq!(format_value(&Value::Usd(49), None), "<$0.0001");
    assert_eq!(format_value(&Value::Dur(999), None), "999ms");
    assert_eq!(format_value(&Value::Dur(86_399_999), None), "23h59m");
    assert_eq!(format_value(&Value::Num(-0.0049), Some(Format::N)), "0");
    assert_eq!(format_value(&Value::Num(999_999.0), Some(Format::Short)), "1M");
    let forty = format_value(&Value::text("x".repeat(41)), Some(Format::Short));
    assert_eq!(forty.chars().count(), SHORT_MAX);
    assert_eq!(format_value(&Value::text("x".repeat(40)), Some(Format::Short)).chars().count(), SHORT_MAX);
}

#[test]
fn money_formatting_is_cosmetic_only_for_negatives() {
    // "-<$0.0001" is odd but a number: recorded, not a contract break
    assert_eq!(format_value(&Value::Usd(-1), None), "-<$0.0001");
}

#[test]
fn coll_parse_corners() {
    for (from, ok) in [
        ("tasks", true),
        (" tasks ", false),
        ("asks", true),
        ("changes", true),
        ("log:*", true),
        ("log:a*b?", true),
        ("log : x", false),
        ("table:x", true),
        ("fold:x.y-z_1", true),
        ("thread:x", true),
        ("tasks:", false),
        ("tasks:x", false),
        ("log:", false),
        ("log", false),
        ("Log:x", false),
        ("Tasks", false),
        ("", false),
        (":x", false),
        ("log:a:b", false),
        ("log:a b", false),
        ("log:ä", false),
        ("log:a\u{0}", false),
        ("table:*", false),
        ("fold:?", false),
        ("thread:a/b", false),
        ("log:a/b", false),
        ("table:a,b", false),
    ] {
        assert_eq!(Coll::parse(from).is_ok(), ok, "{from:?}");
    }
    for from in ["tasks", "log:a*", "table:x", "fold:y", "thread:z", "asks", "changes"] {
        assert_eq!(Coll::parse(from).unwrap().to_string(), from);
    }
}

#[test]
fn query_order_and_limit_corners() {
    let order = |o: &str| {
        Query::parse("tasks", None, Some(o), None)
            .map(|q| q.order.iter().map(|k| (k.key.to_string(), k.desc)).collect::<Vec<_>>())
    };
    assert_eq!(order("-a, -b").unwrap(), vec![("a".into(), true), ("b".into(), true)]);
    assert_eq!(order("- a").unwrap(), vec![("a".into(), true)]);
    assert_eq!(order("[a,b]").unwrap().len(), 1, "a comma inside brackets is not a key break");
    assert_eq!(order("'a,b'").unwrap().len(), 1);
    assert_eq!(order("a, b, c, d").unwrap().len(), MAX_ORDER);
    for bad in ["-", ",", "a,,b", "a,", ",a", "--a", "", "a,b,c,d,e", "a),b", "(a,b", "f(a,b)", "a ==", "-,-"] {
        assert!(order(bad).is_err(), "{bad:?} parsed");
    }
    let lim = |l: u64| Query::parse("tasks", None, None, Some(l)).map(|q| q.limit);
    assert_eq!(lim(1).unwrap(), Some(1));
    assert_eq!(lim(MAX_LIMIT).unwrap(), Some(MAX_LIMIT as usize));
    assert!(lim(0).is_err() && lim(MAX_LIMIT + 1).is_err() && lim(u64::MAX).is_err());
    assert!(Query::parse("tasks", Some(""), None, None).is_err());
    assert!(Query::parse("tasks", Some(&"a".repeat(MAX_LEN + 1)), None, None).is_err());
}

#[test]
fn query_apply_orders_filters_and_limits() {
    let row = |id: &str, k: Value| map(&[("id", id.into()), ("k", k)]);
    let rows = vec![
        row("m1", Value::Missing),
        row("b", Value::Num(2.0)),
        row("a", Value::Num(1.0)),
        row("c", Value::Num(2.0)),
        row("s", "text".into()),
        row("d", Value::Usd(3_000_000)),
        row("m2", Value::Missing),
    ];
    let ids = |q: &Query| {
        q.apply(rows.clone(), &Scope::new())
            .iter()
            .map(|r| text_of(r.at(&[Key::Name("id".into())])))
            .collect::<Vec<_>>()
    };
    let q = |filter: Option<&str>, order: Option<&str>, limit: Option<u64>| {
        Query::parse("tasks", filter, order, limit).unwrap()
    };
    assert_eq!(
        ids(&q(None, Some("k"), None)),
        ["a", "b", "c", "d", "s", "m1", "m2"],
        "ties keep their place, missing last, kinds in rank"
    );
    assert_eq!(
        ids(&q(None, Some("-k"), None)),
        ["s", "d", "b", "c", "a", "m1", "m2"],
        "desc: missing still last, ties keep their place"
    );
    assert_eq!(ids(&q(None, Some("k, -id"), None))[..4], ["a", "c", "b", "d"]);
    assert_eq!(ids(&q(Some("k >= 2"), Some("id"), Some(2))), ["b", "c"]);
    assert_eq!(ids(&q(Some("not k"), None, None)).len(), 2, "missing is not truthy");
    assert_eq!(ids(&q(Some("row.id == 'a' or /id == 'b'"), None, None)), ["b", "a"]);
    assert_eq!(ids(&q(Some("id in ['a', 'c'] and since(5) > 0s"), None, None)).len(), 0, "no clock: since is missing");
    assert!(q(None, None, None).apply(vec![], &Scope::new()).is_empty());
    // a row that is not a map, or is missing, still goes through
    let odd = vec![Value::Num(1.0), Value::Missing, Value::text("x"), Value::List(vec![])];
    assert_eq!(q(Some("row"), None, None).apply(odd.clone(), &Scope::new()).len(), 2);
    assert_eq!(q(None, Some("row"), None).apply(odd, &Scope::new()).len(), 4);
    // outer roots, the clock and `danger` are visible to a row
    let outer =
        Scope::new().with("fold", map(&[("cap", Value::Num(2.0))])).at_ms(10_000).danger_with(|_| Value::Bool(true));
    assert_eq!(
        q(Some("k <= fold.cap and since(0) == 10s and danger('x')"), None, None).apply(rows.clone(), &outer).len(),
        3,
        "a, b, c"
    );
}

#[test]
fn docs_names_exist() {
    let funcs: Vec<&str> = Func::ALL.iter().map(|f| f.word()).collect();
    let mut want = vec!["count", "same", "last", "since", "sum", "len", "lower", "contains", "danger", "quote"];
    let mut got = funcs.clone();
    want.sort_unstable();
    got.sort_unstable();
    assert_eq!(got, want, "PLAN §3.2: count same last since sum len lower contains danger quote");
    let formats: Vec<&str> = Format::ALL.iter().map(|f| f.word()).collect();
    assert_eq!(formats, ["usd", "pct", "n", "dur", "text", "short", "rungs", "runs", "fallback"]);
    let ops: Vec<&str> =
        [Op::Eq, Op::Ne, Op::Lt, Op::Le, Op::Gt, Op::Ge, Op::In, Op::Matches].iter().map(|o| o.word()).collect();
    assert_eq!(ops, ["==", "!=", "<", "<=", ">", ">=", "in", "matches"]);
    let _ = (
        Coll::Tasks,
        Coll::Asks,
        Coll::Changes,
        Coll::Log(String::new()),
        Coll::Table(String::new()),
        Coll::Fold(String::new()),
        Coll::Thread(String::new()),
    );
    let q = Query { from: Coll::Tasks, filter: None, order: vec![], limit: None };
    assert!(q.check(None).is_empty());
    assert_eq!("a.b".parse::<Expr>().unwrap(), ex("a.b"));
    // Env defaults know nothing
    struct Bare;
    impl Env for Bare {
        fn get(&self, _: &str, _: &[Key]) -> Value {
            Value::Missing
        }
    }
    assert_eq!(ex("/a").eval(&Bare), Value::Missing);
    assert_eq!(ex("since(1)").eval(&Bare), Value::Missing);
    assert_eq!(ex("danger('x')").eval(&Bare), Value::Missing);
    assert!(!ex("a").holds(&Bare));
    // check(): unknown roots and literal kinds
    assert_eq!(ex("a.b and c").check(Some(&["a"])).len(), 1);
    assert_eq!(ex("a").check(Some(&[])).len(), 1);
    assert_eq!(ex("len(5)").check(None).len(), 1);
    assert_eq!(ex("'a' in 5").check(None).len(), 1);
    assert_eq!(ex("5 matches 'a'").check(None).len(), 1);
    assert_eq!(ex("x[len(5)]").check(None).len(), 1, "a lint inside an index");
    assert!(ex("/a == 1").check(Some(&[])).is_empty(), "pointers have no root");
    assert_eq!(Value::Dur(0).kind(), "a duration");
    assert_eq!(DASH, "—");
}

#[test]
fn value_laws() {
    assert!(
        !Value::Missing.truthy() && !Value::Dur(0).truthy() && !Value::Usd(0).truthy() && !Value::text("").truthy()
    );
    assert!(Value::Dur(1).truthy() && Value::Usd(-1).truthy() && Value::List(vec![Value::Missing]).truthy());
    assert_eq!(Value::Missing.at(&[]), Value::Missing);
    assert_eq!(Value::Num(1.0).at(&[Key::Name("x".into())]), Value::Missing);
    assert_eq!(Value::Missing.pointer(""), Value::Missing);
    // equality is symmetric over a zoo; Missing equals nothing, itself included
    let z = [
        Value::Missing,
        Value::Bool(true),
        Value::Num(1.0),
        Value::Usd(1_000_000),
        Value::text("1"),
        Value::Dur(1000),
        Value::List(vec![Value::Num(1.0)]),
        map(&[("a", Value::Num(1.0))]),
    ];
    for a in &z {
        for b in &z {
            assert_eq!(a.equals(b), b.equals(a), "{a:?} {b:?}");
            assert_eq!(a.order(b), b.order(a).map(|o| o.reverse()), "{a:?} {b:?}");
        }
        assert_eq!(a.equals(a), *a != Value::Missing, "{a:?}");
    }
}

// ── fuzzers (fixed seeds) ────────────────────────────────────────────────────────────────────

fn toxic_zoo() -> Vec<Value> {
    vec![
        Value::Missing,
        Value::Bool(true),
        Value::Num(0.0),
        Value::Num(-0.0),
        Value::Num(1e15),
        Value::Num(9e15),
        Value::Num(f64::MAX),
        Value::Num(f64::MIN_POSITIVE),
        Value::Num(f64::NAN),
        Value::Num(f64::INFINITY),
        Value::Num(f64::NEG_INFINITY),
        Value::text(""),
        Value::text("é".repeat(500)),
        Value::text("  \n x \r\n "),
        Value::Dur(0),
        Value::Dur(u64::MAX),
        Value::Usd(0),
        Value::Usd(i64::MAX),
        Value::Usd(i64::MIN),
        Value::List(vec![]),
        Value::List(vec![Value::Num(1.0), Value::Missing]),
        Value::List(vec![Value::Usd(i64::MAX), Value::Usd(i64::MAX)]),
        Value::List(vec![Value::Dur(u64::MAX), Value::Dur(u64::MAX)]),
        Value::List(vec![Value::Num(f64::MAX), Value::Num(f64::MAX)]),
        Value::List(vec![Value::Num(f64::INFINITY), Value::Num(f64::NEG_INFINITY)]),
        map(&[("a", Value::Num(1.0)), ("1", Value::Missing), ("", Value::text("e"))]),
    ]
}

/// Every function, operator, index and formatter over extreme values, in a debug build (overflow checks on): no panic.
#[test]
fn extreme_values_never_panic() {
    let exprs = [
        "len(x)",
        "lower(x)",
        "count(x)",
        "count(x, y)",
        "sum(x)",
        "last(x)",
        "same(x)",
        "since(x)",
        "quote(x)",
        "danger(x)",
        "contains(x, y)",
        "x == y",
        "x != y",
        "x < y",
        "x <= y",
        "x > y",
        "x >= y",
        "x in y",
        "x matches y",
        "x[y]",
        "x.a",
        "not x",
        "x and y",
        "x or y",
        "[x, y] == [y, x]",
        "sum([x, y])",
        "count([x, y], x)",
        "same([x, y])",
        "last([x, y])",
        "x[y][y]",
        "/a",
        "/a/0",
        "x not in y",
    ];
    let z = toxic_zoo();
    let mut panics = Vec::new();
    for x in &z {
        for y in &z {
            let sc = Scope::new()
                .with("x", x.clone())
                .with("y", y.clone())
                .doc(x.clone())
                .at_ms(u64::MAX)
                .danger_with(|c| Value::text(c.to_string()));
            for src in exprs {
                let e = ex(src);
                let r = catch_unwind(AssertUnwindSafe(|| {
                    let v = e.eval(&sc);
                    for f in Format::ALL {
                        let _ = format_value(&v, Some(f));
                    }
                }));
                if r.is_err() && panics.len() < 5 {
                    panics.push(format!("{src} with x={x:?} y={y:?}"));
                }
            }
        }
    }
    assert!(panics.is_empty(), "{panics:#?}");
}

const TOKENS: &[&str] = &[
    "a", "b", "x", "x.y", "m", "l", "[", "]", "(", ")", ",", ".", "==", "!=", "<", "<=", ">", ">=", "in", "not", "and",
    "or", "matches", "true", "false", "1", "0.5", "-3", "5m", "1h30m", "$1.50", "'s'", "\"t\"", "/p", "/p/q", "count",
    "sum", "len", "lower", "contains", "last", "same", "since", "quote", "danger", " ", "  ", "$", "'", "\\", "0",
    "250ms", "$0.0004", "'a\\'b'", "/p/0", "[0]", "[a]", "(a)", "count(l)", "len(a)", "not a", "a == b", "a in l",
    "a.b", "l[0]", "'{x}'", "'\n'", "'é'", "/a b",
];

/// Random token soup: whatever parses evaluates, checks and prints without a panic; Fmt too; Query::parse too.
#[test]
fn random_sources_never_panic() {
    let mut r = Rng(0x9E3779B97F4A7C15);
    let sc = world().with("x", map(&[("y", Value::Usd(5))])).with("b", Value::text("s"));
    for _ in 0..60_000 {
        let n = 1 + r.below(9);
        let src: String = (0..n)
            .map(|_| format!("{}{}", TOKENS[r.below(TOKENS.len())], if r.below(3) > 0 { " " } else { "" }))
            .collect();
        let res = catch_unwind(AssertUnwindSafe(|| {
            if let Ok(e) = Expr::parse(&src) {
                let _ = (e.eval(&sc), e.check(Some(&["a"])), e.to_string());
            }
            if let Ok(f) = Fmt::parse(&format!("{{{src}}}")) {
                let _ = (f.render(&sc), f.to_string());
            }
            let _ = Query::parse(&src, Some(&src), Some(&src), Some(3));
        }));
        assert!(res.is_ok(), "panic on {src:?}");
    }
}

fn sp(r: &mut Rng) -> &'static str {
    ["", "", " ", "  "][r.below(4)]
}

fn g_primary(r: &mut Rng, d: usize) -> String {
    let k = if d == 0 { r.below(8) } else { r.below(16) };
    match k {
        0 => ["a", "b", "x.y", "l", "m", "count", "len"][r.below(7)].to_string(),
        1 => [
            "1", "0.5", "-3", "007", "5m", "1h30m", "250ms", "$1.50", "$0.0004", "'s'", "\"t\"", "'a\\'b'", "true",
            "false", "'{x}'", "'\\n'",
        ][r.below(16)]
        .to_string(),
        2 => ["/", "/p", "/p/q", "/p/0", "//"][r.below(5)].to_string(),
        3 => format!("{}{}{}.{}", ["a", "m", "x"][r.below(3)], sp(r), sp(r), ["k", "in", "and", "z"][r.below(4)]),
        4 => format!("a{}.{}{}", sp(r), ["in", "k", "not", "matches", "true"][r.below(5)], sp(r)),
        5 => format!("l[{}]", ["0", "1", "a", "'1'", "-1", "1.5"][r.below(6)]),
        6 => format!("{}[0]", ["/p", "m", "x"][r.below(3)]),
        7 => "[]".to_string(),
        8 => format!("({})", g_expr(r, d - 1)),
        9 => format!("[{}{}]", g_expr(r, d - 1), [",", "", ", 1"][r.below(3)]),
        10 => {
            let (f, n) = [
                ("len", 1),
                ("lower", 1),
                ("count", 2),
                ("sum", 1),
                ("last", 1),
                ("same", 1),
                ("since", 1),
                ("quote", 1),
                ("danger", 1),
                ("contains", 2),
            ][r.below(10)];
            let args: Vec<String> =
                (0..if f == "count" { 1 + r.below(2) } else { n }).map(|_| g_expr(r, d - 1)).collect();
            format!("{f}{}({})", sp(r), args.join(","))
        }
        11 => format!("{}({}).{}", ["last", "lower", "quote"][r.below(3)], g_expr(r, d - 1), ["k", "in"][r.below(2)]),
        12 => format!("x[{}]", g_expr(r, d - 1)),
        13 => format!("{}(l)[{}]", ["last", "count"][r.below(2)], g_expr(r, d - 1)),
        14 => format!("not {}", g_primary(r, d - 1)),
        _ => format!("m[{}]", g_expr(r, d - 1)),
    }
}

fn g_cmp(r: &mut Rng, d: usize) -> String {
    let lhs = g_primary(r, d);
    match r.below(4) {
        0 | 1 => lhs,
        _ => format!(
            "{lhs} {} {}",
            ["==", "!=", "<", "<=", ">", ">=", "in", "not in", "matches"][r.below(9)],
            g_primary(r, d)
        ),
    }
}

fn g_not(r: &mut Rng, d: usize) -> String {
    if r.below(5) == 0 { format!("not {}", g_not(r, d)) } else { g_cmp(r, d) }
}

fn g_expr(r: &mut Rng, d: usize) -> String {
    let mut s = g_not(r, d);
    for _ in 0..r.below(3) {
        s.push_str([" and ", " or "][r.below(2)]);
        s.push_str(&g_not(r, d));
    }
    s
}

/// Valid random expressions: print, parse back, same tree, same value. The generator keeps clear of the three
/// the findings on long `not` chains, a pointer then `.key`, and printing longer than MAX_LEN, so this stays a net.
#[test]
fn random_valid_expressions_print_and_parse_back() {
    let mut r = Rng(42);
    let sc = Scope::new()
        .with("a", Value::Num(1.0))
        .with("b", Value::text("s"))
        .with("l", Value::List(vec![Value::Num(1.0), Value::text("s")]))
        .with("m", map(&[("y", Value::Num(2.0)), ("k", Value::Num(2.0))]))
        .with("x", map(&[("y", Value::Usd(5)), ("1", Value::Bool(true))]))
        .doc(map(&[("p", map(&[("q", Value::Num(3.0)), ("k", Value::Num(4.0))])), ("", Value::Num(9.0))]))
        .at_ms(10);
    let (mut parsed, mut skipped) = (0, 0);
    for _ in 0..60_000 {
        let src = g_expr(&mut r, 3);
        let Ok(e) = Expr::parse(&src) else { continue };
        let shown = e.to_string();
        if shown.chars().count() > MAX_LEN {
            skipped += 1;
            continue;
        }
        parsed += 1;
        let back = match Expr::parse(&shown) {
            Ok(b) => b,
            Err(err) => {
                assert!(
                    err.msg.starts_with("nested more than") && src.contains("not not"),
                    "{src:?} printed as {shown:?}: {err}"
                );
                continue; // the long `not` chains finding
            }
        };
        assert_eq!(back, e, "{src:?} printed as {shown:?}");
        assert_eq!(back.eval(&sc), e.eval(&sc), "{src:?}");
    }
    assert!(parsed > 20_000, "the generator is too weak: {parsed} parsed, {skipped} too long");
}
