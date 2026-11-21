//! Value through the public API only. Blind tests: every expectation comes from the doc comments of `value.rs` and the
//! language reference in `expr.rs`. A row marked `DECIDED` is one where the contract is silent or allows two readings;
//! the row states the reading it chose. Each theme collects every failing row and asserts once at the end.

use j4n_core::eval::Scope;
use j4n_core::expr::Expr;
use j4n_core::value::{DASH, Key, Value};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Flip to `true` for one run to see "rows / failed" of every theme, including the passing ones.
const REPORT: bool = false;

struct Tally {
    rows: usize,
    fails: Vec<String>,
}

impl Tally {
    fn new() -> Tally {
        Tally { rows: 0, fails: Vec::new() }
    }

    fn note(note: &str) -> String {
        if note.is_empty() { String::new() } else { format!("  [{note}]") }
    }

    fn eq<T: PartialEq + Debug>(&mut self, input: String, note: &str, want: T, got: impl FnOnce() -> T) {
        self.rows += 1;
        let note = Tally::note(note);
        match catch_unwind(AssertUnwindSafe(got)) {
            Ok(g) if g == want => {}
            Ok(g) => self.fails.push(format!("{input}{note}\n      want {want:?}\n      got  {g:?}")),
            Err(_) => self.fails.push(format!("{input}{note}\n      want {want:?}\n      got  PANIC")),
        }
    }

    /// `Ok(())` passes, `Err(why)` fails with the reason.
    fn holds(&mut self, input: String, note: &str, f: impl FnOnce() -> Result<(), String>) {
        self.rows += 1;
        let note = Tally::note(note);
        match catch_unwind(AssertUnwindSafe(f)) {
            Ok(Ok(())) => {}
            Ok(Err(why)) => self.fails.push(format!("{input}{note}\n      {why}")),
            Err(_) => self.fails.push(format!("{input}{note}\n      PANIC")),
        }
    }

    fn finish(self, theme: &str) {
        let (rows, n) = (self.rows, self.fails.len());
        if REPORT || n > 0 {
            panic!("[{theme}] {n} of {rows} rows failed\n  {}", self.fails.join("\n  "));
        }
    }
}

fn n(x: f64) -> Value {
    Value::Num(x)
}

fn s(x: &str) -> Value {
    Value::Str(x.to_string())
}

fn usd(d: f64) -> Value {
    Value::usd(d)
}

fn list(items: Vec<Value>) -> Value {
    Value::List(items)
}

fn map(pairs: &[(&str, Value)]) -> Value {
    Value::Map(pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect::<BTreeMap<_, _>>())
}

fn name(x: &str) -> Key {
    Key::Name(x.to_string())
}

/// A document with the awkward keys in it.
fn doc() -> Value {
    map(&[
        ("", s("empty-key")),
        ("/", n(12.0)),
        ("~1", n(11.0)),
        ("0", s("zero-key")),
        ("a", map(&[("b", list(vec![n(10.0), map(&[("c", s("deep"))])])), ("", n(99.0))])),
        ("a/b", n(7.0)),
        ("m~n", n(8.0)),
        ("l", list(vec![n(1.0), n(2.0), n(3.0)])),
        ("s", s("str")),
        ("z", n(0.0)),
        ("ключ", s("cyrillic")),
        ("nested", list(vec![list(vec![s("x"), s("y")]), list(vec![])])),
    ])
}

/// One of everything that matters, no NaN, no infinity.
fn sample() -> Vec<Value> {
    vec![
        Value::Missing,
        Value::Bool(false),
        Value::Bool(true),
        n(-1.0),
        n(0.0),
        n(1.5),
        n(2.0),
        n(1e9),
        s(""),
        s("a"),
        s("B"),
        s("ab"),
        Value::Dur(0),
        Value::Dur(1500),
        Value::Dur(60_000),
        Value::Usd(0),
        Value::Usd(1_500_000),
        Value::Usd(-3),
        Value::Usd(2_000_000),
        list(vec![]),
        list(vec![n(1.0)]),
        list(vec![n(2.0), s("x")]),
        map(&[]),
        map(&[("a", n(1.0))]),
    ]
}

fn is_scalar(v: &Value) -> bool {
    matches!(v, Value::Bool(_) | Value::Num(_) | Value::Str(_) | Value::Dur(_) | Value::Usd(_))
}

#[test]
fn constants_and_defaults() {
    let mut t = Tally::new();
    t.eq("DASH".into(), "", "—", || DASH);
    t.eq("Value::default()".into(), "", Value::Missing, Value::default);
    t.eq("Value::from(true)".into(), "no doc on From: natural reading", Value::Bool(true), || Value::from(true));
    t.eq("Value::from(false)".into(), "no doc on From: natural reading", Value::Bool(false), || Value::from(false));
    t.eq("Value::from(\"x\")".into(), "no doc on From: natural reading", s("x"), || Value::from("x"));
    t.eq("Value::from(\"\")".into(), "no doc on From: natural reading", s(""), || Value::from(""));
    t.eq("Value::from(2.5)".into(), "no doc on From: natural reading", n(2.5), || Value::from(2.5f64));
    t.eq(
        "Value::from(f64::NAN)".into(),
        "DECIDED: From<f64> has no doc; reading: goes through Value::num, so a non-finite number is missing",
        Value::Missing,
        || Value::from(f64::NAN),
    );
    t.eq(
        "Value::from(f64::INFINITY)".into(),
        "DECIDED: From<f64> has no doc; reading: goes through Value::num, so a non-finite number is missing",
        Value::Missing,
        || Value::from(f64::INFINITY),
    );
    t.eq("Value::text(\"hi\")".into(), "", s("hi"), || Value::text("hi"));
    t.eq("Value::text(String::from(\"hi\"))".into(), "", s("hi"), || Value::text(String::from("hi")));
    t.eq("Value::text(\"\")".into(), "", s(""), || Value::text(""));
    t.eq("Value::text(\"line\\nbreak {x}\")".into(), "", s("line\nbreak {x}"), || Value::text("line\nbreak {x}"));
    t.finish("constants_and_defaults");
}

#[test]
fn num_constructor() {
    let mut t = Tally::new();
    let rows: Vec<(f64, Value, &str)> = vec![
        (0.0, n(0.0), ""),
        (1.0, n(1.0), ""),
        (-1.0, n(-1.0), ""),
        (2.5, n(2.5), ""),
        (-0.5, n(-0.5), ""),
        (1e300, n(1e300), ""),
        (f64::MAX, n(f64::MAX), ""),
        (f64::MIN, n(f64::MIN), ""),
        (f64::MIN_POSITIVE, n(f64::MIN_POSITIVE), ""),
        (f64::NAN, Value::Missing, ""),
        (f64::INFINITY, Value::Missing, ""),
        (f64::NEG_INFINITY, Value::Missing, ""),
    ];
    for (input, want, note) in rows {
        t.eq(format!("Value::num({input:?})"), note, want, || Value::num(input));
    }
    t.holds("Value::num(-0.0) is a zero".into(), "", || match Value::num(-0.0) {
        Value::Num(0.0) => Ok(()),
        other => Err(format!("got {other:?}")),
    });
    t.finish("num_constructor");
}

#[test]
fn usd_constructor() {
    let mut t = Tally::new();
    // (dollars, want, note); money is kept to a millionth, one out of range is missing
    let rows: Vec<(f64, Value, &str)> = vec![
        (0.0, Value::Usd(0), ""),
        (-0.0, Value::Usd(0), ""),
        (1.0, Value::Usd(1_000_000), ""),
        (1.5, Value::Usd(1_500_000), ""),
        (-1.5, Value::Usd(-1_500_000), ""),
        (0.25, Value::Usd(250_000), ""),
        (0.000001, Value::Usd(1), ""),
        (0.0004, Value::Usd(400), ""),
        (0.0000004, Value::Usd(0), "rounded to the nearest millionth: below half a millionth is zero"),
        (0.0000006, Value::Usd(1), "rounded to the nearest millionth"),
        (-0.0000006, Value::Usd(-1), "rounded to the nearest millionth"),
        (-0.0000004, Value::Usd(0), "rounded to the nearest millionth"),
        (0.1 + 0.2, Value::Usd(300_000), "float noise (0.30000000000000004) is rounded away"),
        (1234.567891, Value::Usd(1_234_567_891), ""),
        (19.99, Value::Usd(19_990_000), ""),
        (1e12, Value::Usd(1_000_000_000_000_000_000), ""),
        (8e12, Value::Usd(8_000_000_000_000_000_000), ""),
        (-8e12, Value::Usd(-8_000_000_000_000_000_000), ""),
        (
            9e12,
            Value::Usd(9_000_000_000_000_000_000),
            "DECIDED: the range is never given; reading: what an i64 of millionths holds (about 9.22e12 dollars)",
        ),
        (
            -9e12,
            Value::Usd(-9_000_000_000_000_000_000),
            "DECIDED: the range is never given; reading: what an i64 of millionths holds (about 9.22e12 dollars)",
        ),
        (
            9.2e12,
            Value::Usd(9_200_000_000_000_000_000),
            "DECIDED: the range is never given; reading: what an i64 of millionths holds (about 9.22e12 dollars)",
        ),
        (9.3e12, Value::Missing, "out of range of i64 millionths"),
        (-9.3e12, Value::Missing, "out of range of i64 millionths"),
        (1e13, Value::Missing, "out of range"),
        (1e300, Value::Missing, "out of range"),
        (-1e300, Value::Missing, "out of range"),
        (f64::MAX, Value::Missing, "out of range"),
        (f64::MIN, Value::Missing, "out of range"),
        (f64::NAN, Value::Missing, "not finite"),
        (f64::INFINITY, Value::Missing, "not finite"),
        (f64::NEG_INFINITY, Value::Missing, "not finite"),
    ];
    for (input, want, note) in rows {
        t.eq(format!("Value::usd({input:?})"), note, want, || Value::usd(input));
    }
    t.finish("usd_constructor");
}

#[test]
fn kind_words() {
    let mut t = Tally::new();
    // The doc says only "the kind as the word messages use": the words themselves are not in any public doc.
    // DECIDED (spec-gap): reading: the words an expression error uses ("text", "a list", "a map", "a number", "a bool",
    // "a duration", "money"); the word for Missing is not guessable, so it is only checked to be non-empty and distinct.
    let rows: Vec<(Value, &str)> = vec![
        (Value::Bool(true), "a bool"),
        (n(1.0), "a number"),
        (s("x"), "text"),
        (Value::Dur(1), "a duration"),
        (Value::Usd(1), "money"),
        (list(vec![]), "a list"),
        (map(&[]), "a map"),
    ];
    for (v, want) in &rows {
        t.eq(
            format!("{v:?}.kind()"),
            "DECIDED: kind words are not enumerated in the public docs",
            want.to_string(),
            || v.kind().to_string(),
        );
    }
    t.holds("all eight kinds have a distinct, non-empty word".into(), "", || {
        let mut all: Vec<(Value, &str)> = rows.clone();
        all.push((Value::Missing, ""));
        let rows = all;
        let words: Vec<&str> = rows.iter().map(|(v, _)| v.kind()).collect();
        for (i, a) in words.iter().enumerate() {
            if a.is_empty() {
                return Err(format!("empty word for {:?}", rows[i].0));
            }
            for (j, b) in words.iter().enumerate() {
                if i != j && a == b {
                    return Err(format!("{:?} and {:?} share the word {a:?}", rows[i].0, rows[j].0));
                }
            }
        }
        Ok(())
    });
    t.holds("kind does not depend on the content".into(), "", || {
        let pairs = [
            (n(1.0), n(-7.5)),
            (s("a"), s("")),
            (list(vec![]), list(vec![n(1.0)])),
            (map(&[]), map(&[("a", n(1.0))])),
            (Value::Dur(1), Value::Dur(2)),
            (Value::Usd(1), Value::Usd(-9)),
            (Value::Bool(true), Value::Bool(false)),
        ];
        for (a, b) in pairs {
            if a.kind() != b.kind() {
                return Err(format!("{a:?} is {:?} but {b:?} is {:?}", a.kind(), b.kind()));
            }
        }
        Ok(())
    });
    t.finish("kind_words");
}

#[test]
fn truthy_table() {
    let mut t = Tally::new();
    // "Missing, `false`, `0`, empty text, `0s`, `$0`, an empty list or map are no; everything else is yes."
    let rows: Vec<(Value, bool, &str)> = vec![
        (Value::Missing, false, ""),
        (Value::Bool(false), false, ""),
        (Value::Bool(true), true, ""),
        (n(0.0), false, ""),
        (n(-0.0), false, "negative zero is zero"),
        (n(1.0), true, ""),
        (n(-1.0), true, ""),
        (n(0.001), true, ""),
        (n(1e-300), true, ""),
        (n(1e300), true, ""),
        (s(""), false, ""),
        (s(" "), true, "a space is not empty"),
        (s("0"), true, "the text 0 is not the number 0"),
        (s("false"), true, "the text false is not false"),
        (s("a"), true, ""),
        (Value::Dur(0), false, ""),
        (Value::Dur(1), true, ""),
        (Value::Dur(u64::MAX), true, ""),
        (Value::Usd(0), false, ""),
        (Value::Usd(1), true, ""),
        (Value::Usd(-1), true, ""),
        (usd(0.0), false, ""),
        (usd(0.0000004), false, "rounds to $0"),
        (list(vec![]), false, ""),
        (list(vec![Value::Missing]), true, "non-empty even if the item is missing"),
        (list(vec![Value::Bool(false)]), true, "non-empty even if the item is no"),
        (list(vec![n(0.0)]), true, ""),
        (map(&[]), false, ""),
        (map(&[("a", Value::Missing)]), true, "non-empty even if the value is missing"),
        (map(&[("a", n(0.0))]), true, ""),
    ];
    for (v, want, note) in rows {
        t.eq(format!("{v:?}.truthy()"), note, want, || v.truthy());
    }
    t.finish("truthy_table");
}

#[test]
fn at_paths() {
    let mut t = Tally::new();
    let d = doc();
    let missing = Value::Missing;
    // (path, want, note)
    let rows: Vec<(Vec<Key>, Value, &str)> = vec![
        (vec![], d.clone(), "the empty path is the whole value"),
        (vec![name("a"), name("b"), Key::Idx(1), name("c")], s("deep"), ""),
        (vec![name("a"), name("b"), Key::Idx(0)], n(10.0), ""),
        (vec![name("a"), name("b"), Key::Idx(2)], missing.clone(), "past the end"),
        (vec![name("a"), name("b"), Key::Idx(-1)], missing.clone(), "positions count from 0, never from the end"),
        (vec![name("a"), name("b"), Key::Idx(i64::MAX)], missing.clone(), "no overflow"),
        (vec![name("a"), name("b"), Key::Idx(i64::MIN)], missing.clone(), "no overflow"),
        (vec![name("a"), name("nope")], missing.clone(), ""),
        (vec![name("nope"), name("x")], missing.clone(), ""),
        (vec![name("nope"), Key::Idx(0)], missing.clone(), ""),
        (vec![name("s"), name("x")], missing.clone(), "a name into text"),
        (vec![name("z"), name("x")], missing.clone(), "a name into a number"),
        (vec![name("z"), Key::Idx(0)], missing.clone(), "a position into a number"),
        (vec![name("s"), Key::Idx(0)], missing.clone(), "DECIDED: a position into text (no character indexing)"),
        (vec![name("l"), Key::Idx(0)], n(1.0), ""),
        (vec![name("l"), Key::Idx(2)], n(3.0), ""),
        (vec![name("l"), Key::Idx(3)], missing.clone(), ""),
        (
            vec![name("l"), name("1")],
            n(2.0),
            "DECIDED: a plain-number name on a list is a position (the pointer doc says so; `at` is silent)",
        ),
        (vec![name("l"), name("01")], missing.clone(), "07 is not a position"),
        (vec![name("l"), name("+1")], missing.clone(), "+7 is not a position"),
        (vec![name("l"), name("-1")], missing.clone(), "-1 is not a position"),
        (vec![name("l"), name("x")], missing.clone(), ""),
        (vec![name("l"), name("")], missing.clone(), "an empty name is not a position"),
        (vec![name("0")], s("zero-key"), "a name \"0\" on a map is the key \"0\""),
        (vec![Key::Idx(0)], s("zero-key"), "a whole number on a map is the key written with its digits"),
        (vec![name("a/b")], n(7.0), "a name is not split at slashes"),
        (vec![name("m~n")], n(8.0), "a name is not unescaped"),
        (vec![name("~1")], n(11.0), "a name is not unescaped"),
        (vec![name("/")], n(12.0), "a name is not unescaped"),
        (vec![name("")], s("empty-key"), "the empty name is a key"),
        (vec![name("a"), name("")], n(99.0), "the empty name is a key"),
        (vec![name("ключ")], s("cyrillic"), ""),
        (vec![name("nested"), Key::Idx(0), Key::Idx(1)], s("y"), ""),
        (vec![name("nested"), Key::Idx(1), Key::Idx(0)], missing.clone(), "an empty inner list"),
        (vec![name("nested"), name("0"), name("1")], s("y"), "DECIDED: plain-number names on lists are positions"),
    ];
    for (path, want, note) in rows {
        t.eq(format!("doc.at({path:?})"), note, want, || d.at(&path));
    }
    t.eq("Missing.at(&[])".into(), "", Value::Missing, || Value::Missing.at(&[]));
    t.eq("Missing.at(&[a])".into(), "", Value::Missing, || Value::Missing.at(&[name("a")]));
    t.eq("Missing.at(&[0])".into(), "", Value::Missing, || Value::Missing.at(&[Key::Idx(0)]));
    t.eq("Num(5).at(&[])".into(), "", n(5.0), || n(5.0).at(&[]));
    t.eq("List.at(&[Idx(1)])".into(), "", s("b"), || list(vec![s("a"), s("b")]).at(&[Key::Idx(1)]));
    t.eq("Usd.at(&[a])".into(), "", Value::Missing, || Value::Usd(5).at(&[name("a")]));
    t.eq("Dur.at(&[a])".into(), "", Value::Missing, || Value::Dur(5).at(&[name("a")]));
    t.eq("Bool.at(&[a])".into(), "", Value::Missing, || Value::Bool(true).at(&[name("a")]));
    t.finish("at_paths");
}

#[test]
fn pointer_reads() {
    let mut t = Tally::new();
    let d = doc();
    let missing = Value::Missing;
    // (pointer, want, note)
    let rows: Vec<(&str, Value, &str)> = vec![
        ("", d.clone(), "\"\" is the whole value"),
        ("/a/b/0", n(10.0), ""),
        ("/a/b/1/c", s("deep"), ""),
        ("/a/b/2", missing.clone(), ""),
        ("/a/b/-1", missing.clone(), "-1 is not a position"),
        ("/a/b/01", missing.clone(), "07 is not a position"),
        ("/a/b/+0", missing.clone(), "+7 is not a position"),
        ("/a/b/1.0", missing.clone(), "not a plain whole number"),
        ("/a/b/-", missing.clone(), "RFC 6901 end-of-array marker has no value to read"),
        ("/a/b/x", missing.clone(), ""),
        ("/a/b/", missing.clone(), "the empty name on a list"),
        ("/l/0", n(1.0), ""),
        ("/l/2", n(3.0), ""),
        ("/l/3", missing.clone(), ""),
        ("/l/10", missing.clone(), ""),
        ("/l/99999999999999999999", missing.clone(), "an enormous position does not overflow"),
        ("/s", s("str"), ""),
        ("/s/0", missing.clone(), "into text"),
        ("/s/x", missing.clone(), "into text"),
        ("/z", n(0.0), "a zero is there"),
        ("/z/x", missing.clone(), "into a number"),
        ("/0", s("zero-key"), "a name on a map, even a number-like one"),
        ("/a~1b", n(7.0), "~1 is /"),
        ("/m~0n", n(8.0), "~0 is ~"),
        ("/~01", n(11.0), "~01 is the key ~1 (~1 first, then ~0, is the wrong order)"),
        ("/~1", n(12.0), "~1 alone is the key /"),
        ("/a/", n(99.0), "a trailing slash is the empty name"),
        (
            "/",
            s("empty-key"),
            "DECIDED: RFC 6901 says `/` is the key \"\"; the expression doc says `/` alone is the whole document",
        ),
        ("/nope", missing.clone(), ""),
        ("/nope/x/y", missing.clone(), ""),
        ("a", missing.clone(), "no leading slash is not a pointer"),
        ("a/b", missing.clone(), "no leading slash is not a pointer"),
        ("/~2", missing.clone(), "DECIDED: a bad escape reads nothing (or the literal key ~2)"),
        ("/ключ", s("cyrillic"), ""),
        ("/nested/0/1", s("y"), ""),
        ("/nested/1/0", missing.clone(), ""),
    ];
    for (ptr, want, note) in rows {
        t.eq(format!("doc.pointer({ptr:?})"), note, want, || d.pointer(ptr));
    }
    t.eq("Missing.pointer(\"\")".into(), "", Value::Missing, || Value::Missing.pointer(""));
    t.eq("Missing.pointer(\"/a\")".into(), "", Value::Missing, || Value::Missing.pointer("/a"));
    t.eq("Num(5).pointer(\"\")".into(), "", n(5.0), || n(5.0).pointer(""));
    t.eq("Num(5).pointer(\"/a\")".into(), "", Value::Missing, || n(5.0).pointer("/a"));
    t.eq("List.pointer(\"/1\")".into(), "", s("b"), || list(vec![s("a"), s("b")]).pointer("/1"));
    t.eq("List.pointer(\"/a\")".into(), "", Value::Missing, || list(vec![s("a")]).pointer("/a"));
    t.eq("List.pointer(\"/0/0\") of lists".into(), "", n(1.0), || list(vec![list(vec![n(1.0)])]).pointer("/0/0"));
    t.finish("pointer_reads");
}

#[test]
fn equals_table() {
    let mut t = Tally::new();
    let m = Value::Missing;
    // (a, b, want, note)
    let rows: Vec<(Value, Value, bool, &str)> = vec![
        (m.clone(), m.clone(), false, "nothing equals nothing"),
        (m.clone(), n(1.0), false, ""),
        (n(1.0), m.clone(), false, ""),
        (m.clone(), s(""), false, ""),
        (m.clone(), Value::Bool(false), false, ""),
        (n(1.0), n(1.0), true, ""),
        (n(1.0), n(2.0), false, ""),
        (n(0.0), n(-0.0), true, "by value"),
        (n(0.1 + 0.2), n(0.3), false, "plain float compare"),
        (s("a"), s("a"), true, ""),
        (s("a"), s("A"), false, "case matters"),
        (s(""), s(""), true, ""),
        (s("a"), s("a "), false, ""),
        (s("1"), n(1.0), false, "different kinds"),
        (n(1.0), s("1"), false, "different kinds"),
        (Value::Bool(true), Value::Bool(true), true, ""),
        (Value::Bool(true), Value::Bool(false), false, ""),
        (Value::Bool(true), n(1.0), false, "different kinds"),
        (Value::Bool(false), n(0.0), false, "different kinds"),
        (Value::Bool(false), s(""), false, "different kinds"),
        (Value::Dur(1000), Value::Dur(1000), true, ""),
        (Value::Dur(1000), Value::Dur(1001), false, ""),
        (Value::Dur(0), Value::Dur(0), true, ""),
        (Value::Dur(1000), n(1.0), false, "a plain number against a duration does not compare"),
        (n(1000.0), Value::Dur(1000), false, "a plain number against a duration does not compare"),
        (Value::Dur(0), n(0.0), false, "a plain number against a duration does not compare"),
        (Value::Dur(1000), Value::Usd(1000), false, "different kinds"),
        (Value::Usd(1_500_000), Value::Usd(1_500_000), true, ""),
        (Value::Usd(1_500_000), Value::Usd(1_500_001), false, "exact to a millionth"),
        (usd(1.5), n(1.5), true, "money against a number counts in dollars"),
        (n(1.5), usd(1.5), true, "money against a number counts in dollars"),
        (Value::Usd(1_500_000), n(1.5000001), true, "a number against money is rounded to a millionth"),
        (n(1.5000001), Value::Usd(1_500_000), true, "the other way round"),
        (n(1.5000001), n(1.5), false, "plain numbers differ"),
        (Value::Usd(0), n(0.0), true, ""),
        (Value::Usd(-2_000_000), n(-2.0), true, ""),
        (Value::Usd(1_000_000), n(1.0), true, ""),
        (Value::Usd(1_000_000), n(2.0), false, ""),
        (Value::Usd(1_000_000), s("$1"), false, ""),
        (list(vec![]), list(vec![]), true, ""),
        (list(vec![n(1.0), n(2.0)]), list(vec![n(1.0), n(2.0)]), true, ""),
        (list(vec![n(1.0), n(2.0)]), list(vec![n(2.0), n(1.0)]), false, "by item, in order"),
        (list(vec![n(1.0)]), list(vec![n(1.0), n(2.0)]), false, "different length"),
        (list(vec![n(1.0), n(2.0)]), list(vec![n(1.0)]), false, "different length"),
        (list(vec![s("a"), s("b")]), list(vec![s("a"), s("b")]), true, ""),
        (list(vec![list(vec![n(1.0)])]), list(vec![list(vec![n(1.0)])]), true, "nested"),
        (list(vec![list(vec![n(1.0)])]), list(vec![list(vec![n(2.0)])]), false, "nested"),
        (
            list(vec![n(1.0), usd(1.0)]),
            list(vec![usd(1.0), n(1.0)]),
            true,
            "items are compared by equals: number against money by dollars",
        ),
        (list(vec![n(1.0)]), list(vec![s("1")]), false, ""),
        (list(vec![n(1.0)]), n(1.0), false, "list against scalar"),
        (list(vec![]), map(&[]), false, "list against map"),
        (list(vec![]), m.clone(), false, ""),
        (
            list(vec![m.clone()]),
            list(vec![m.clone()]),
            false,
            "DECIDED: items are compared item by item and nothing equals nothing, so a list holding a missing item is not equal even to itself",
        ),
        (map(&[]), map(&[]), true, ""),
        (map(&[("a", n(1.0)), ("b", n(2.0))]), map(&[("b", n(2.0)), ("a", n(1.0))]), true, "by key and value"),
        (map(&[("a", n(1.0))]), map(&[("a", n(2.0))]), false, ""),
        (map(&[("a", n(1.0))]), map(&[("b", n(1.0))]), false, ""),
        (map(&[("a", n(1.0))]), map(&[("a", n(1.0)), ("b", n(1.0))]), false, "extra key"),
        (map(&[("a", n(1.0)), ("b", n(1.0))]), map(&[("a", n(1.0))]), false, "missing key"),
        (
            map(&[("a", n(1.5))]),
            map(&[("a", usd(1.5))]),
            true,
            "values are compared by equals: number against money by dollars",
        ),
        (map(&[("a", list(vec![n(1.0)]))]), map(&[("a", list(vec![n(1.0)]))]), true, "nested"),
        (map(&[("a", list(vec![n(1.0)]))]), map(&[("a", list(vec![n(2.0)]))]), false, "nested"),
        (map(&[("a", n(1.0))]), list(vec![n(1.0)]), false, "map against list"),
        (map(&[]), m.clone(), false, ""),
        (
            map(&[("a", m.clone())]),
            map(&[("a", m.clone())]),
            false,
            "DECIDED: a missing value inside is not equal (see lists)",
        ),
    ];
    for (a, b, want, note) in rows {
        t.eq(format!("{a:?}.equals({b:?})"), note, want, || a.equals(&b));
    }
    t.holds("equals is symmetric over the sample".into(), "", || {
        let v = sample();
        for a in &v {
            for b in &v {
                if a.equals(b) != b.equals(a) {
                    return Err(format!(
                        "{a:?}.equals({b:?}) = {} but the other way round it is {}",
                        a.equals(b),
                        b.equals(a)
                    ));
                }
            }
        }
        Ok(())
    });
    t.holds("a value without missing inside equals itself".into(), "", || {
        for a in sample().iter().filter(|a| !matches!(a, Value::Missing)) {
            if !a.equals(a) {
                return Err(format!("{a:?} is not equal to itself"));
            }
        }
        Ok(())
    });
    t.holds(
        "equals agrees with order over text, numbers, durations and money: equal exactly when the order is Equal"
            .into(),
        "",
        || {
            let v = sample();
            let plain = |x: &&Value| is_scalar(x) && !matches!(x, Value::Bool(_));
            for a in v.iter().filter(plain) {
                for b in v.iter().filter(plain) {
                    let by_order = a.order(b) == Some(Ordering::Equal);
                    if a.equals(b) != by_order {
                        return Err(format!("{a:?} vs {b:?}: equals={} order={:?}", a.equals(b), a.order(b)));
                    }
                }
            }
            Ok(())
        },
    );
    t.finish("equals_table");
}

#[test]
fn order_table() {
    let mut t = Tally::new();
    use Ordering::*;
    let m = Value::Missing;
    // (a, b, want, note)
    let rows: Vec<(Value, Value, Option<Ordering>, &str)> = vec![
        (n(1.0), n(2.0), Some(Less), ""),
        (n(2.0), n(1.0), Some(Greater), ""),
        (n(1.0), n(1.0), Some(Equal), ""),
        (n(-5.0), n(-1.0), Some(Less), ""),
        (n(-1.0), n(0.0), Some(Less), ""),
        (n(0.0), n(-0.0), Some(Equal), "by value"),
        (n(9.0), n(10.0), Some(Less), "numbers, not text"),
        (n(1e-9), n(0.0), Some(Greater), ""),
        (s("a"), s("b"), Some(Less), ""),
        (s("b"), s("a"), Some(Greater), ""),
        (s("a"), s("a"), Some(Equal), ""),
        (s(""), s("a"), Some(Less), "empty text first"),
        (s(""), s(""), Some(Equal), ""),
        (s("ab"), s("abc"), Some(Less), "a prefix is smaller"),
        (s("abc"), s("abd"), Some(Less), ""),
        (s("a"), s("B"), Some(Greater), "by character: B (66) is before a (97)"),
        (s("B"), s("a"), Some(Less), "by character"),
        (s("10"), s("9"), Some(Less), "text compares by character, not as numbers"),
        (s("é"), s("z"), Some(Greater), "by character (U+00E9 is after z)"),
        (s("日"), s("本"), Some(Less), "by character"),
        (Value::Bool(false), Value::Bool(true), None, "a bool has no order"),
        (Value::Bool(true), Value::Bool(true), None, "a bool has no order, not even against itself"),
        (Value::Dur(1), Value::Dur(2), Some(Less), ""),
        (Value::Dur(2000), Value::Dur(2000), Some(Equal), ""),
        (Value::Dur(60_000), Value::Dur(1_000), Some(Greater), ""),
        (Value::Usd(1), Value::Usd(2), Some(Less), ""),
        (Value::Usd(-5), Value::Usd(-4), Some(Less), ""),
        (Value::Usd(-5), Value::Usd(0), Some(Less), ""),
        (Value::Usd(i64::MIN), Value::Usd(i64::MAX), Some(Less), "no overflow"),
        (n(1.5), usd(1.5), Some(Equal), "a number against money counts in dollars"),
        (usd(1.5), n(1.5), Some(Equal), "a number against money counts in dollars"),
        (n(2.0), usd(1.5), Some(Greater), "dollars"),
        (usd(0.5), n(1.0), Some(Less), "dollars"),
        (usd(2.0), n(1.99), Some(Greater), "dollars"),
        (n(-1.0), usd(0.0), Some(Less), "dollars"),
        (n(1.0), Value::Dur(1000), None, "a plain number against a duration does not compare"),
        (Value::Dur(1000), n(1.0), None, "a plain number against a duration does not compare"),
        (n(1.0), s("1"), None, "different kinds"),
        (s("1"), n(1.0), None, "different kinds"),
        (Value::Bool(true), n(1.0), None, "different kinds"),
        (Value::Dur(1), Value::Usd(1), None, "different kinds"),
        (s("a"), Value::Dur(1), None, "different kinds"),
        (m.clone(), n(1.0), None, "missing has no order"),
        (n(1.0), m.clone(), None, "missing has no order"),
        (m.clone(), m.clone(), None, "missing has no order"),
        (m.clone(), s("a"), None, ""),
        (list(vec![n(1.0)]), list(vec![n(2.0)]), None, "lists have no order"),
        (list(vec![]), list(vec![]), None, "lists have no order"),
        (list(vec![n(1.0)]), n(1.0), None, ""),
        (map(&[]), map(&[]), None, "maps have no order"),
        (map(&[("a", n(1.0))]), map(&[("a", n(2.0))]), None, "maps have no order"),
        (map(&[]), list(vec![]), None, ""),
    ];
    for (a, b, want, note) in rows {
        t.eq(format!("{a:?}.order({b:?})"), note, want, || a.order(&b));
    }
    t.holds("order is antisymmetric over the sample".into(), "", || {
        let v = sample();
        for a in &v {
            for b in &v {
                if a.order(b) != b.order(a).map(Ordering::reverse) {
                    return Err(format!(
                        "{a:?}.order({b:?}) = {:?} but the other way round {:?}",
                        a.order(b),
                        b.order(a)
                    ));
                }
            }
        }
        Ok(())
    });
    t.holds("order is transitive over the sample".into(), "", || {
        let v = sample();
        for a in &v {
            for b in &v {
                for c in &v {
                    if let (Some(x), Some(y), Some(z)) = (a.order(b), b.order(c), a.order(c))
                        && x != Greater
                        && y != Greater
                        && z == Greater
                    {
                        return Err(format!("{a:?} <= {b:?} <= {c:?} but {a:?} > {c:?}"));
                    }
                }
            }
        }
        Ok(())
    });
    t.holds("bools have no order, though they are equal to themselves".into(), "", || {
        let (f, t2) = (Value::Bool(false), Value::Bool(true));
        if !(f.equals(&f) && t2.equals(&t2)) || f.order(&f).is_some() || f.order(&t2).is_some() {
            return Err("a bool is equal to itself and has no order".into());
        }
        Ok(())
    });
    t.holds("a value that is not a scalar never has an order".into(), "", || {
        let v = sample();
        for a in &v {
            for b in &v {
                if (!is_scalar(a) || !is_scalar(b)) && a.order(b).is_some() {
                    return Err(format!("{a:?}.order({b:?}) = {:?}", a.order(b)));
                }
            }
        }
        Ok(())
    });
    t.finish("order_table");
}

#[test]
fn sort_cmp_table() {
    let mut t = Tally::new();
    use Ordering::*;
    let m = Value::Missing;
    // (a, b, want, note)
    let rows: Vec<(Value, Value, Ordering, &str)> = vec![
        (n(1.0), n(2.0), Less, "within one kind, Value::order"),
        (n(2.0), n(1.0), Greater, ""),
        (n(1.0), n(1.0), Equal, ""),
        (s("a"), s("b"), Less, ""),
        (s("b"), s("a"), Greater, ""),
        (s("a"), s("B"), Greater, "by character"),
        (s("10"), s("9"), Less, "by character"),
        (Value::Dur(1), Value::Dur(2), Less, ""),
        (Value::Usd(5), Value::Usd(-5), Greater, ""),
        (Value::Bool(false), Value::Bool(true), Less, "DECIDED: false before true"),
        (m.clone(), m.clone(), Equal, "missing against missing keeps its place"),
        (m.clone(), n(1.0), Greater, "missing is after everything"),
        (n(1.0), m.clone(), Less, "missing is after everything"),
        (m.clone(), s(""), Greater, "missing is after everything"),
        (s(""), m.clone(), Less, "missing is after everything"),
        (m.clone(), Value::Bool(false), Greater, "missing is after everything"),
        (m.clone(), Value::Dur(0), Greater, "missing is after everything"),
        (m.clone(), Value::Usd(i64::MAX), Greater, "missing is after everything"),
        (m.clone(), list(vec![]), Greater, "missing is after everything"),
        (list(vec![]), m.clone(), Less, "missing is after everything"),
        (m.clone(), map(&[]), Greater, "missing is after everything"),
        (map(&[]), m.clone(), Less, "missing is after everything"),
        (list(vec![n(1.0)]), list(vec![n(2.0)]), Equal, "the rest keep their place"),
        (list(vec![n(2.0)]), list(vec![n(1.0)]), Equal, "the rest keep their place"),
        (list(vec![]), list(vec![n(1.0), n(2.0)]), Equal, "the rest keep their place"),
        (map(&[("a", n(1.0))]), map(&[("a", n(2.0))]), Equal, "the rest keep their place"),
        (map(&[]), map(&[("z", s("q"))]), Equal, "the rest keep their place"),
        (n(1.5), usd(1.5), Less, "sorting puts a number before money, whatever the dollars"),
        (n(2.0), usd(1.5), Less, "sorting puts a number before money, whatever the dollars"),
        (usd(0.5), n(1.0), Greater, "sorting puts a number before money, whatever the dollars"),
    ];
    for (a, b, want, note) in rows {
        t.eq(format!("{a:?}.sort_cmp({b:?})"), note, want, || a.sort_cmp(&b));
    }
    t.holds("sort_cmp is reflexive over the sample".into(), "", || {
        for a in &sample() {
            if a.sort_cmp(a) != Equal {
                return Err(format!("{a:?}.sort_cmp(itself) = {:?}", a.sort_cmp(a)));
            }
        }
        Ok(())
    });
    t.holds("sort_cmp is antisymmetric over the sample".into(), "", || {
        let v = sample();
        for a in &v {
            for b in &v {
                if a.sort_cmp(b) != b.sort_cmp(a).reverse() {
                    return Err(format!(
                        "{a:?}.sort_cmp({b:?}) = {:?} but the other way round {:?}",
                        a.sort_cmp(b),
                        b.sort_cmp(a)
                    ));
                }
            }
        }
        Ok(())
    });
    t.holds("sort_cmp is transitive over the sample (a total preorder)".into(), "", || {
        let v = sample();
        for a in &v {
            for b in &v {
                for c in &v {
                    if a.sort_cmp(b) != Greater && b.sort_cmp(c) != Greater && a.sort_cmp(c) == Greater {
                        return Err(format!("{a:?} <= {b:?} <= {c:?} but {a:?} > {c:?}"));
                    }
                }
            }
        }
        Ok(())
    });
    t.holds("sort_cmp equals order wherever order has an answer".into(), "", || {
        let v = sample();
        for a in &v {
            for b in &v {
                let number_and_money =
                    matches!((a, b), (Value::Num(_), Value::Usd(_)) | (Value::Usd(_), Value::Num(_)));
                if let Some(o) = a.order(b).filter(|_| !number_and_money)
                    && a.sort_cmp(b) != o
                {
                    return Err(format!("{a:?} vs {b:?}: order {o:?}, sort_cmp {:?}", a.sort_cmp(b)));
                }
            }
        }
        Ok(())
    });
    t.holds("a missing value is after every other value, and only missing ties with missing".into(), "", || {
        for a in sample().iter().filter(|a| !matches!(a, Value::Missing)) {
            if Value::Missing.sort_cmp(a) != Greater || a.sort_cmp(&Value::Missing) != Less {
                return Err(format!("Missing vs {a:?}"));
            }
        }
        Ok(())
    });
    t.holds(
        "two values of different kinds that are scalars never tie unless order says Equal".into(),
        "DECIDED: kinds in a fixed order (which one is not documented)",
        || {
            let v = sample();
            for a in v.iter().filter(|a| is_scalar(a)) {
                for b in v.iter().filter(|b| is_scalar(b)) {
                    if a.kind() != b.kind() && a.sort_cmp(b) == Equal && a.order(b) != Some(Equal) {
                        return Err(format!("{a:?} and {b:?} tie in sort_cmp but order says {:?}", a.order(b)));
                    }
                }
            }
            Ok(())
        },
    );
    t.holds(
        "kinds sort as contiguous groups: sorting a shuffled sample puts each kind in one run".into(),
        "DECIDED: number and money may share a group (ordered by dollars) or be two groups",
        || {
            let mut v = sample();
            v.reverse();
            v.sort_by(|a, b| a.sort_cmp(b));
            // non-decreasing under sort_cmp
            for w in v.windows(2) {
                if w[0].sort_cmp(&w[1]) == Greater {
                    return Err(format!("not sorted: {:?} then {:?}", w[0], w[1]));
                }
            }
            // a group is a run of one kind word (number and money are merged: they can interleave by dollars)
            let word = |x: &Value| match x {
                Value::Num(_) | Value::Usd(_) => "num-or-money".to_string(),
                Value::Missing => "MISSING".to_string(),
                other => other.kind().to_string(),
            };
            let mut seen: Vec<String> = Vec::new();
            for x in &v {
                let w = word(x);
                if seen.last() != Some(&w) {
                    if seen.contains(&w) {
                        return Err(format!(
                            "kind {w:?} is split into several runs: {:?}",
                            v.iter().map(word).collect::<Vec<_>>()
                        ));
                    }
                    seen.push(w);
                }
            }
            if seen.last().map(String::as_str) != Some("MISSING") {
                return Err(format!("missing is not last: {seen:?}"));
            }
            Ok(())
        },
    );
    t.holds("sorting with sort_cmp does not panic from every start order".into(), "", || {
        let base = sample();
        for k in 0..base.len() {
            let mut v = base.clone();
            v.rotate_left(k);
            v.sort_by(|a, b| a.sort_cmp(b));
            let mut w = base.clone();
            w.rotate_right(k);
            w.sort_by(|a, b| a.sort_cmp(b));
            for (x, y) in v.iter().zip(w.iter()) {
                if x.sort_cmp(y) != Equal {
                    return Err(format!("start order {k}: {x:?} vs {y:?} at the same place"));
                }
            }
        }
        Ok(())
    });
    t.finish("sort_cmp_table");
}

#[test]
fn index_steps_through_expressions() {
    // `[expr]` goes "into a map (by text) or a list (by whole number from 0)": the same steps `Value::at` takes.
    let mut t = Tally::new();
    let m = map(&[("0", s("zero")), ("a", s("A")), ("", s("empty"))]);
    let l = list(vec![s("p"), s("q")]);
    // (expression, want, note)
    let rows: Vec<(&str, Value, &str)> = vec![
        ("m['0']", s("zero"), ""),
        ("m['a']", s("A"), ""),
        ("m['']", s("empty"), ""),
        ("m['nope']", Value::Missing, ""),
        ("m[0]", s("zero"), "a whole number on a map is the key written with its digits"),
        ("m[1 == 1]", Value::Missing, "a bool is not text"),
        ("m.a", s("A"), ""),
        ("l[0]", s("p"), ""),
        ("l[1]", s("q"), ""),
        ("l[2]", Value::Missing, "past the end"),
        ("l[-1]", Value::Missing, "from 0, not from the end"),
        ("l[1.0]", s("q"), "1.0 is a whole number"),
        ("l[0.5]", Value::Missing, "not a whole number"),
        ("l[1.5]", Value::Missing, "not a whole number"),
        ("l['1']", s("q"), "a text that is a plain whole number is a position"),
        ("l[nope]", Value::Missing, "a missing index"),
        ("l[true]", Value::Missing, "a bool is not a number"),
        ("m[nope]", Value::Missing, "a missing key"),
        ("l[0][0]", Value::Missing, "text is not indexed"),
        ("m.a.b", Value::Missing, "a name into text"),
        ("l.p", Value::Missing, "a name into a list"),
    ];
    let env = Scope::new().with("m", m).with("l", l);
    for (src, want, note) in rows {
        t.eq(format!("{src} over m={{\"0\",\"a\",\"\"}}, l=[p,q]"), note, want, || match Expr::parse(src) {
            Ok(e) => e.eval(&env),
            Err(e) => s(&format!("PARSE ERROR {e}")),
        });
    }
    t.finish("index_steps_through_expressions");
}
