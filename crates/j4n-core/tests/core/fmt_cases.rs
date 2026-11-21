//! Fmt through the public API only. Blind tests: every expectation comes from the doc comments of `fmt.rs`, `value.rs`,
//! `eval.rs` and the language reference in `expr.rs`. A row marked `DECIDED` is one where the contract is silent or
//! allows two readings; the row states the reading it chose. Each theme collects every failing row and asserts once.

use j4n_core::eval::{Env, Scope};
use j4n_core::expr::{Expr, MAX_DEPTH, MAX_LEN};
use j4n_core::fmt::{Fmt, Format, MAX_FMT, Piece, SHORT_MAX, duration, format_value};
use j4n_core::value::{DASH, Key, Value};
use std::cell::RefCell;
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

fn sv(x: &str) -> Value {
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

fn scope(v: Value) -> Scope {
    Scope::new().with("v", v)
}

/// Parse and render; a parse error shows as text so a row can say what it expected.
fn show(tpl: &str, env: &Scope) -> String {
    match Fmt::parse(tpl) {
        Ok(f) => f.render(env),
        Err(e) => format!("<<PARSE ERROR at {}: {}>>", e.at, e.msg),
    }
}

/// `{v:<format>}` over the value; `None` is `{v}`.
fn hole(format: Option<Format>, v: &Value) -> String {
    let tpl = match format {
        Some(f) => format!("{{v:{}}}", f.word()),
        None => "{v}".to_string(),
    };
    show(&tpl, &scope(v.clone()))
}

fn fname(f: Option<Format>) -> &'static str {
    f.map(Format::word).unwrap_or("(none)")
}

fn world() -> Scope {
    Scope::new()
        .with("a", map(&[("b", list(vec![n(10.0), n(20.0)])), ("s", sv("hi")), ("t", usd(1.5))]))
        .with("empty", sv(""))
        .with("zero", n(0.0))
        .with("no", Value::Bool(false))
        .with("l", list(vec![n(1.0), n(2.0)]))
        .with("m", map(&[("k", n(1.0))]))
        .with("t0", n(4000.0))
        .at_ms(10_000)
        .doc(map(&[
            ("usage", map(&[("cost_usd", usd(0.5)), ("tokens", n(1234.0))])),
            ("content", list(vec![map(&[("text", sv("hello"))])])),
            ("type", sv("text")),
        ]))
}

#[test]
fn literal_text_and_brace_escapes() {
    let mut t = Tally::new();
    // (template, want, note); no holes, so the scope is empty
    let rows: &[(&str, &str, &str)] = &[
        ("", "", ""),
        ("hello", "hello", ""),
        ("{{", "{", ""),
        ("}}", "}", ""),
        ("{{}}", "{}", ""),
        ("}}{{", "}{", ""),
        ("{{{{", "{{", ""),
        ("}}}}", "}}", ""),
        ("{{}}{{}}", "{}{}", ""),
        ("a {{x}} b", "a {x} b", ""),
        ("a}}b{{c", "a}b{c", ""),
        ("x{{y}}z", "x{y}z", ""),
        ("{{x:usd}}", "{x:usd}", "escaped braces make no hole, so the colon is literal"),
        ("100%", "100%", ""),
        ("a:b", "a:b", "a colon outside a hole is literal"),
        ("[a] (b) <c> $1.50 :usd", "[a] (b) <c> $1.50 :usd", ""),
        ("a\\b", "a\\b", "a backslash outside a hole is literal"),
        ("tab\there", "tab\there", ""),
        ("line1\nline2", "line1\nline2", ""),
        ("  padded  ", "  padded  ", "whitespace is kept"),
        ("héllo — ✓ 日本", "héllo — ✓ 日本", ""),
        ("it's \"quoted\"", "it's \"quoted\"", "quotes outside a hole are literal"),
        ("`code` <b>&amp;</b>", "`code` <b>&amp;</b>", ""),
        ("{{{{}}}}", "{{}}", ""),
    ];
    for &(tpl, want, note) in rows {
        t.eq(format!("render({tpl:?})"), note, want.to_string(), || show(tpl, &Scope::new()));
    }
    let long = "x".repeat(MAX_FMT);
    t.holds(
        "a template of MAX_FMT characters is kept whole".into(),
        "prose is not bound by the length of an expression",
        || {
            let got = show(&long, &Scope::new());
            if got == long { Ok(()) } else { Err(format!("got {} characters", got.chars().count())) }
        },
    );
    t.holds("one more character is refused".into(), "", || {
        if Fmt::parse(&"x".repeat(MAX_FMT + 1)).is_err() { Ok(()) } else { Err("parsed".into()) }
    });
    t.finish("literal_text_and_brace_escapes");
}

#[test]
fn usd_formatter() {
    let mut t = Tally::new();
    let u = Some(Format::Usd);
    // (value, want, note)
    let rows: Vec<(Value, &str, &str)> = vec![
        (Value::Missing, DASH, ""),
        (Value::Usd(0), "$0.00", ""),
        (usd(0.0), "$0.00", ""),
        (usd(-0.0), "$0.00", "no negative zero"),
        (usd(12.5), "$12.50", "doc example"),
        (usd(0.5), "$0.50", ""),
        (usd(1.0), "$1.00", ""),
        (usd(100.0), "$100.00", ""),
        (usd(0.01), "$0.01", ""),
        (usd(0.1), "$0.10", ""),
        (usd(0.03), "$0.03", ""),
        (usd(1.994999), "$1.99", "below half a cent over: down"),
        (usd(1.995001), "$2.00", "above half a cent over: up"),
        (usd(0.994999), "$0.99", ""),
        (usd(0.125), "$0.13", "DECIDED: exactly half a cent, the direction is not given; reading: half up"),
        (usd(12.345), "$12.35", "DECIDED: exactly half a cent; reading: half up"),
        (usd(2.675), "$2.68", "DECIDED: exactly half a cent; reading: half up (money is exact, so no float surprise)"),
        (usd(1.005), "$1.01", "DECIDED: exactly half a cent; reading: half up"),
        (usd(0.015), "$0.02", "DECIDED: exactly half a cent; reading: half up"),
        (usd(0.995), "$1.00", "DECIDED: exactly half a cent; reading: half up, carrying into the dollars"),
        (usd(19.995), "$20.00", "DECIDED: exactly half a cent; reading: half up, carrying"),
        (usd(0.004), "$0.0040", "doc example: four places"),
        (usd(0.0004), "$0.0004", "four places"),
        (usd(0.0001), "$0.0001", "the smallest shown as itself"),
        (Value::Usd(100), "$0.0001", "the smallest shown as itself"),
        (
            Value::Usd(99),
            "$0.0001",
            "DECIDED: below $0.0001 by a little: strictly below the smallest shown (or rounds to $0.0001)",
        ),
        (usd(0.00009), "$0.0001", "DECIDED: below $0.0001 by a little: strictly below (or rounds to $0.0001)"),
        (usd(0.00005), "$0.0001", "DECIDED: half the smallest: strictly below (or rounds up to $0.0001)"),
        (Value::Usd(1), "<$0.0001", "one millionth"),
        (Value::Usd(10), "<$0.0001", "ten millionths"),
        (Value::Usd(49), "<$0.0001", "well below the smallest"),
        (usd(0.0049), "$0.0049", "under half a cent: four places"),
        (usd(0.003), "$0.0030", ""),
        (usd(0.001), "$0.0010", ""),
        (usd(0.004999), "$0.0050", "DECIDED: just under half a cent: four places"),
        (usd(0.005), "$0.01", "DECIDED: exactly half a cent: where four places stop"),
        (usd(0.0051), "$0.01", "DECIDED: where four places stop"),
        (usd(0.0075), "$0.01", "DECIDED: where four places stop (half a cent or a whole cent)"),
        (usd(0.009), "$0.01", "DECIDED: where four places stop (half a cent or a whole cent)"),
        (usd(0.0099), "$0.01", "DECIDED: where four places stop (half a cent or a whole cent)"),
        (usd(-12.5), "-$12.50", "DECIDED: where the sign goes"),
        (usd(-0.5), "-$0.50", "DECIDED: where the sign goes"),
        (usd(-0.004), "-$0.0040", "DECIDED: negative tiny money"),
        (usd(-0.0004), "-$0.0004", "DECIDED: negative tiny money"),
        (usd(-0.00004), "-<$0.0001", "DECIDED: negative money below the smallest"),
        (Value::Usd(-1), "-<$0.0001", "DECIDED: negative money below the smallest"),
        (usd(-0.125), "-$0.13", "DECIDED: negative half cent: away from zero"),
        (usd(-1.995001), "-$2.00", "DECIDED: sign placement"),
        (n(3.0), "$3.00", "a number counts in dollars"),
        (n(0.0), "$0.00", ""),
        (n(0.5), "$0.50", ""),
        (n(12.5), "$12.50", ""),
        (n(0.004), "$0.0040", "a number is money here too"),
        (n(0.00004), "<$0.0001", ""),
        (n(-3.0), "-$3.00", "DECIDED: where the sign goes"),
        (n(1234.5), "$1234.50", "DECIDED: no thousands separator is mentioned"),
        (usd(1234567.891), "$1234567.89", "DECIDED: no thousands separator is mentioned"),
        (n(1e6), "$1000000.00", "DECIDED: no thousands separator is mentioned"),
    ];
    for (v, want, note) in rows {
        t.eq(format!("{{v:usd}} with v={v:?}"), note, want.to_string(), || hole(u, &v));
    }
    t.finish("usd_formatter");
}

#[test]
fn pct_formatter() {
    let mut t = Tally::new();
    let p = Some(Format::Pct);
    // (value, want, note); "a number as a share of one, held to 0-1: 0.666 is 67%"
    let rows: Vec<(Value, &str, &str)> = vec![
        (Value::Missing, DASH, ""),
        (n(0.666), "67%", "doc example"),
        (n(0.0), "0%", ""),
        (n(1.0), "100%", ""),
        (n(0.5), "50%", ""),
        (n(0.25), "25%", ""),
        (n(0.01), "1%", ""),
        (n(0.1), "10%", ""),
        (n(0.004), "0%", ""),
        (n(0.994), "99%", ""),
        (n(0.996), "100%", ""),
        (n(0.999), "100%", ""),
        (n(0.333), "33%", ""),
        (n(0.125), "13%", "DECIDED: exactly half a percent; reading: half up"),
        (n(0.375), "38%", "DECIDED: exactly half a percent; reading: half up"),
        (n(0.005), "1%", "DECIDED: exactly half a percent; reading: half up"),
        (n(1.5), "150%", "over one"),
        (n(2.0), "200%", "over one"),
        (n(1.0001), "100%", "over one"),
        (n(100.0), "10000%", "over one"),
        (n(1e9), "100000000000%", "over one"),
        (n(-0.5), "-50%", "below zero"),
        (n(-1.0), "-100%", "below zero"),
        (n(-0.001), "0%", "held to 0-1, not -0%"),
        (n(-0.0), "0%", "not -0%"),
        (n(-1e9), "-100000000000%", "below zero"),
        (n(f64::MAX), "—", "too large to show"),
        (n(f64::MIN), "—", "too large to show"),
    ];
    for (v, want, note) in rows {
        t.eq(format!("{{v:pct}} with v={v:?}"), note, want.to_string(), || hole(p, &v));
    }
    t.finish("pct_formatter");
}

#[test]
fn n_formatter() {
    let mut t = Tally::new();
    let f = Some(Format::N);
    // (value, want, note); "a number: whole, or two decimals at most"
    let rows: Vec<(Value, &str, &str)> = vec![
        (Value::Missing, DASH, ""),
        (n(0.0), "0", ""),
        (n(3.0), "3", "whole"),
        (n(-2.0), "-2", ""),
        (n(999.0), "999", ""),
        (n(1000.0), "1000", "DECIDED: no thousands separator is mentioned"),
        (n(1234.0), "1234", "DECIDED: no thousands separator is mentioned"),
        (n(12345.0), "12345", "DECIDED: no thousands separator is mentioned"),
        (n(999_999.0), "999999", "DECIDED: no thousands separator is mentioned"),
        (n(2e9), "2000000000", "not scientific notation, not compacted"),
        (n(1e15), "1000000000000000", "not scientific notation"),
        (n(3.5), "3.5", "two decimals at most: one is fine"),
        (n(3.25), "3.25", ""),
        (n(0.1), "0.1", ""),
        (n(0.5), "0.5", ""),
        (n(0.05), "0.05", ""),
        (n(1.1), "1.1", ""),
        (n(2.5), "2.5", ""),
        (n(7.14159), "7.14", "rounded to two decimals"),
        (n(2.567), "2.57", "rounded to two decimals"),
        (n(-2.567), "-2.57", ""),
        (n(-0.5), "-0.5", ""),
        (n(2.999), "3", "DECIDED: rounds up to a whole number; reading: shown whole"),
        (n(0.001), "0", "DECIDED: rounds to zero; reading: shown as 0"),
        (n(-0.001), "0", "DECIDED: rounds to zero; reading: 0, not -0"),
        (n(1e-9), "0", "DECIDED: rounds to zero"),
        (n(-0.0), "0", "not -0"),
        (n(100.0), "100", ""),
        (n(10.0), "10", "no trailing zeros chopped off a whole number"),
        (n(1.0), "1", ""),
        (n(20.0), "20", "no trailing zeros chopped off a whole number"),
        (n(100.5), "100.5", ""),
    ];
    for (v, want, note) in rows {
        t.eq(format!("{{v:n}} with v={v:?}"), note, want.to_string(), || hole(f, &v));
    }
    t.finish("n_formatter");
}

#[test]
fn dur_formatter() {
    let mut t = Tally::new();
    let f = Some(Format::Dur);
    // (value, want, note); "850ms, 45s, 3m05s, 1h02m, 2d03h": the two biggest units that matter
    let rows: Vec<(Value, &str, &str)> = vec![
        (Value::Missing, DASH, ""),
        (Value::Dur(1), "1ms", ""),
        (Value::Dur(850), "850ms", "doc example"),
        (Value::Dur(999), "999ms", ""),
        (Value::Dur(1000), "1s", ""),
        (Value::Dur(45_000), "45s", "doc example"),
        (Value::Dur(59_000), "59s", ""),
        (Value::Dur(60_000), "1m00s", ""),
        (Value::Dur(65_000), "1m05s", ""),
        (Value::Dur(90_000), "1m30s", ""),
        (Value::Dur(185_000), "3m05s", "doc example"),
        (Value::Dur(3_599_000), "59m59s", ""),
        (Value::Dur(3_600_000), "1h00m", ""),
        (Value::Dur(3_720_000), "1h02m", "doc example"),
        (Value::Dur(5_400_000), "1h30m", ""),
        (Value::Dur(3_659_000), "1h00m", "only the two biggest units: the seconds fall off"),
        (Value::Dur(86_399_000), "23h59m", ""),
        (Value::Dur(86_400_000), "1d00h", ""),
        (Value::Dur(90_000_000), "1d01h", ""),
        (Value::Dur(183_600_000), "2d03h", "doc example"),
        (Value::Dur(864_000_000), "10d00h", ""),
        (Value::Dur(0), "0s", "zero"),
        (Value::Dur(1500), "1s", "DECIDED: 1.5 seconds; reading: cut to the unit (not rounded, not 1.5s)"),
        (Value::Dur(59_999), "59s", "DECIDED: just under a minute; reading: cut, not rounded up to 60s"),
        (Value::Dur(60_999), "1m00s", "DECIDED: reading: cut"),
        // a number counts in seconds
        (n(0.85), "850ms", "a number counts in seconds"),
        (n(0.001), "1ms", ""),
        (n(0.5), "500ms", ""),
        (n(45.0), "45s", ""),
        (n(59.0), "59s", ""),
        (n(60.0), "1m00s", ""),
        (n(185.0), "3m05s", ""),
        (n(3599.0), "59m59s", ""),
        (n(3600.0), "1h00m", ""),
        (n(86400.0), "1d00h", ""),
        (n(183600.0), "2d03h", ""),
        (n(1e9), "11574d01h", "a very long time"),
        (n(0.0), "0s", "zero"),
        (n(1.5), "1s", "DECIDED: 1.5 seconds; reading: cut"),
        (n(-5.0), "0s", "a negative number of seconds is none"),
    ];
    for (v, want, note) in rows {
        t.eq(format!("{{v:dur}} with v={v:?}"), note, want.to_string(), || hole(f, &v));
    }
    // the same, through the public helper
    let ms_rows: &[(u64, &str, &str)] = &[
        (1, "1ms", ""),
        (850, "850ms", ""),
        (999, "999ms", ""),
        (1000, "1s", ""),
        (45_000, "45s", ""),
        (59_000, "59s", ""),
        (60_000, "1m00s", ""),
        (185_000, "3m05s", ""),
        (3_599_000, "59m59s", ""),
        (3_600_000, "1h00m", ""),
        (3_720_000, "1h02m", ""),
        (86_399_000, "23h59m", ""),
        (86_400_000, "1d00h", ""),
        (183_600_000, "2d03h", ""),
        (0, "0s", "zero"),
    ];
    for &(ms, want, note) in ms_rows {
        t.eq(format!("duration({ms})"), note, want.to_string(), || duration(ms));
    }
    t.holds("duration(u64::MAX) neither panics nor is empty".into(), "", || {
        let s = duration(u64::MAX);
        if s.is_empty() { Err("empty".into()) } else { Ok(()) }
    });
    t.holds("duration is never longer than two units (at most 6 characters up to 99 days)".into(), "", || {
        for ms in [1u64, 999, 1000, 59_999, 60_000, 3_599_999, 3_600_000, 86_399_999, 86_400_000, 8_639_999_999] {
            let s = duration(ms);
            if s.chars().count() > 6 || s.is_empty() {
                return Err(format!("duration({ms}) = {s:?}"));
            }
        }
        Ok(())
    });
    t.finish("dur_formatter");
}

#[test]
fn text_formatter() {
    let mut t = Tally::new();
    let f = Some(Format::Text);
    // (value, want, note); "the words of the value, as they are"
    let rows: Vec<(Value, String, &str)> = vec![
        (Value::Missing, DASH.to_string(), ""),
        (sv("hello"), "hello".into(), ""),
        (sv("a\nb"), "a\nb".into(), "line breaks stay"),
        (sv("a\r\nb"), "a\r\nb".into(), ""),
        (sv("  x  "), "  x  ".into(), "spaces stay"),
        (sv("a   b"), "a   b".into(), "whitespace runs stay"),
        (sv("a\tb"), "a\tb".into(), ""),
        (sv("{x}"), "{x}".into(), "braces in a value are not a template"),
        (sv("{{x}}"), "{{x}}".into(), "braces in a value are not an escape"),
        (sv("日本語 — ✓"), "日本語 — ✓".into(), ""),
        (sv(&"w".repeat(100)), "w".repeat(100), "text never cuts"),
        (sv(&"w".repeat(SHORT_MAX + 1)), "w".repeat(SHORT_MAX + 1), "text never cuts"),
        (sv("x:usd"), "x:usd".into(), "a colon in a value is not a formatter"),
        (n(3.0), "3".into(), ""),
        (n(2.5), "2.5".into(), ""),
        (n(-7.0), "-7".into(), ""),
        (n(0.0), "0".into(), "zero is there"),
        (Value::Bool(true), "true".into(), ""),
        (Value::Bool(false), "false".into(), ""),
        (Value::Dur(45_000), "45s".into(), ""),
        (Value::Usd(1_500_000), "$1.50".into(), ""),
        (sv(""), DASH.to_string(), "DECIDED: empty text; the brief says a dash, the doc comments do not"),
    ];
    for (v, want, note) in rows {
        t.eq(format!("{{v:text}} with v={v:?}"), note, want, || hole(f, &v));
    }
    t.finish("text_formatter");
}

#[test]
fn short_formatter_on_text() {
    let mut t = Tally::new();
    let f = Some(Format::Short);
    let fill = |c: char, k: usize| c.to_string().repeat(k);
    let exactly_max = fill('a', SHORT_MAX);
    let spaced = format!("{}     {}", fill('a', 20), fill('b', SHORT_MAX - 21));
    let spaced_want = format!("{} {}", fill('a', 20), fill('b', SHORT_MAX - 21));
    // (text, want, note)
    let rows: Vec<(String, String, &str)> = vec![
        ("hello".into(), "hello".into(), ""),
        ("a b".into(), "a b".into(), ""),
        ("a   b".into(), "a b".into(), "DECIDED: a whitespace run; reading: one space (one line)"),
        ("a\tb".into(), "a b".into(), "DECIDED: a tab; reading: one space"),
        ("a\nb".into(), "a…".into(), "only the first line, and the cut is marked"),
        ("a\r\nb".into(), "a…".into(), "only the first line, and the cut is marked"),
        ("a\n\n\nb".into(), "a…".into(), "only the first line, and the cut is marked"),
        ("line one\nline two".into(), "line one…".into(), "only the first line, and the cut is marked"),
        ("  lead".into(), "lead".into(), "DECIDED: leading whitespace; reading: trimmed"),
        ("trail  ".into(), "trail".into(), "DECIDED: trailing whitespace; reading: trimmed"),
        ("\n  both \n".into(), "both".into(), "DECIDED: reading: trimmed"),
        ("x{y}".into(), "x{y}".into(), ""),
        ("日本語".into(), "日本語".into(), ""),
        ("with — dash ✓".into(), "with — dash ✓".into(), ""),
        (exactly_max.clone(), exactly_max.clone(), "exactly SHORT_MAX characters is not cut"),
        (fill('é', SHORT_MAX), fill('é', SHORT_MAX), "SHORT_MAX characters, not bytes"),
        (fill('日', SHORT_MAX), fill('日', SHORT_MAX), "SHORT_MAX characters, not bytes"),
        (spaced, spaced_want, "DECIDED: the length is of the one-line result: 45 characters in, 40 out, no cut"),
        ("   ".into(), DASH.to_string(), "DECIDED: nothing but whitespace; reading: no words, so the dash"),
        ("\n".into(), DASH.to_string(), "DECIDED: nothing but whitespace; reading: no words, so the dash"),
        (String::new(), DASH.to_string(), "DECIDED: empty text; the brief says a dash, the doc comments do not"),
    ];
    for (text, want, note) in rows {
        let v = sv(&text);
        t.eq(format!("{{v:short}} with v={v:?}"), note, want, || hole(f, &v));
    }
    // one over, and well over: the result is one line of at most SHORT_MAX characters, ends in the cut mark, and is a
    // start of the one-line text
    let long_inputs: Vec<(String, &str)> = vec![
        (fill('a', SHORT_MAX + 1), "one more than SHORT_MAX"),
        (fill('a', SHORT_MAX + 2), "two more than SHORT_MAX"),
        (fill('a', 100), "100 characters"),
        (fill('é', SHORT_MAX + 1), "one more, two bytes each"),
        (fill('日', 1000), "1000 characters, three bytes each"),
        (fill('😀', SHORT_MAX + 5), "emoji, four bytes each"),
        ("word ".repeat(30), "many words"),
        (format!("{}     {}", fill('p', 25), fill('q', 25)), "a whitespace run in the middle"),
        (format!("{} {}", fill('a', SHORT_MAX - 1), "bbbb"), "a space after 39 characters"),
    ];
    for (text, what) in long_inputs {
        let v = sv(&text);
        t.holds(format!("{{v:short}} over {what}"), "", || {
            let got = hole(f, &v);
            let count = got.chars().count();
            if count > SHORT_MAX {
                return Err(format!("{count} characters (more than SHORT_MAX={SHORT_MAX}): {got:?}"));
            }
            if got.contains('\n') || got.contains('\r') {
                return Err(format!("not one line: {got:?}"));
            }
            if !got.ends_with('…') {
                return Err(format!("no cut mark: {got:?}"));
            }
            let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
            let kept = got.trim_end_matches('…');
            if !flat.starts_with(kept) {
                return Err(format!("{got:?} is not the start of {flat:?}"));
            }
            Ok(())
        });
        t.holds(
            format!("{{v:short}} over {what} fills the budget"),
            "DECIDED: the cut keeps as much as fits, the cut mark included",
            || {
                let got = hole(f, &v);
                let count = got.chars().count();
                if count == SHORT_MAX || got.trim_end_matches('…').ends_with(' ') && count + 1 == SHORT_MAX {
                    Ok(())
                } else {
                    Err(format!("{count} characters: {got:?}"))
                }
            },
        );
    }
    // a second line that does not fit is either shown or marked as cut: it does not vanish without a trace
    for (text, what) in [
        (format!("{}\n{}", fill('x', 30), fill('y', 30)), "two lines of 30"),
        ("first line\nsecond line".to_string(), "two short lines"),
        ("only one\n".to_string(), "a trailing line break"),
    ] {
        let v = sv(&text);
        t.holds(format!("{{v:short}} over {what}: nothing is dropped unmarked"), "DECIDED: multi-line text; reading: one line, and a dropped line is a cut, so it is marked with the cut mark", || {
            let got = hole(f, &v);
            let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if got == flat || got.ends_with('…') { Ok(()) } else { Err(format!("got {got:?}, the text was {text:?}")) }
        });
    }
    t.finish("short_formatter_on_text");
}

#[test]
fn short_formatter_on_numbers() {
    let mut t = Tally::new();
    let f = Some(Format::Short);
    // (number, want, note); "a number as 1.2k, 3M; below a thousand as itself"
    let rows: Vec<(f64, &str, &str)> = vec![
        (0.0, "0", ""),
        (7.0, "7", ""),
        (42.0, "42", ""),
        (999.0, "999", "below a thousand as itself"),
        (1.5, "1.5", "below a thousand as itself"),
        (0.5, "0.5", "below a thousand as itself"),
        (1000.0, "1k", ""),
        (1100.0, "1.1k", ""),
        (1234.0, "1.2k", "doc example"),
        (1500.0, "1.5k", ""),
        (1999.0, "2k", "DECIDED: rounds up to the next whole k"),
        (9999.0, "10k", "DECIDED: rounds up"),
        (12345.0, "12.3k", "one decimal"),
        (99999.0, "100k", "DECIDED: rounds up"),
        (100_000.0, "100k", ""),
        (999_999.0, "1M", "DECIDED: rounds up into the next unit, not 1000k"),
        (1_000_000.0, "1M", ""),
        (1_500_000.0, "1.5M", ""),
        (3_000_000.0, "3M", "doc example"),
        (12_345_678.0, "12.3M", "one decimal"),
        (999_999_999.0, "1B", "DECIDED: rounds up into the next unit, not 1000M"),
        (1e9, "1B", ""),
        (1.5e9, "1.5B", ""),
        (2e9, "2B", "doc example"),
        (-1.0, "-1", ""),
        (-999.0, "-999", ""),
        (-1000.0, "-1k", ""),
        (-1234.0, "-1.2k", ""),
        (-1_500_000.0, "-1.5M", ""),
        (-2e9, "-2B", ""),
        (1e12, "1T", "a trillion"),
        (-0.0, "0", "not -0"),
    ];
    for (x, want, note) in rows {
        t.eq(format!("{{v:short}} with v=Num({x:?})"), note, want.to_string(), || hole(f, &n(x)));
    }
    // the other kinds under short are what they are
    let other: Vec<(Value, &str, &str)> = vec![
        (Value::Missing, DASH, ""),
        (Value::Dur(45_000), "45s", "DECIDED: a duration under short; reading: its own words"),
        (Value::Usd(1_500_000), "$1.50", "DECIDED: money under short; reading: its own words"),
        (Value::Bool(true), "true", ""),
        (Value::Bool(false), "false", ""),
    ];
    for (v, want, note) in other {
        t.eq(format!("{{v:short}} with v={v:?}"), note, want.to_string(), || hole(f, &v));
    }
    t.finish("short_formatter_on_numbers");
}

#[test]
fn no_formatter_and_formatters_that_do_not_fit() {
    let mut t = Tally::new();
    // "a formatter that does not fit the value's kind is the kind's own words (usd, pct and dur also take a plain number;
    // n and text change nothing a plain value does not)". Reading: the kind's own words are how the value shows with
    // no formatter at all; a formatter that does not fit changes nothing.
    let plain: Vec<(Value, &str, &str)> = vec![
        (
            sv("hi"),
            "hi",
            "DECIDED: 'the kind's own words' could be the kind's name (text) instead; reading: the plain value",
        ),
        (Value::Bool(true), "true", ""),
        (Value::Bool(false), "false", ""),
        (list(vec![n(1.0), n(2.0)]), "1, 2", "a list shows its items"),
        (map(&[("k", n(1.0))]), "k=1", "a map shows its keys and values"),
        (usd(1.5), "$1.50", "DECIDED: money under a formatter that does not fit; reading: as under usd"),
        (Value::Dur(45_000), "45s", "DECIDED: a duration under a formatter that does not fit; reading: as under dur"),
    ];
    let formats: Vec<Option<Format>> = vec![None].into_iter().chain(Format::ALL.iter().map(|f| Some(*f))).collect();
    for (v, want, note) in &plain {
        for f in &formats {
            if matches!(v, Value::List(_) | Value::Map(_))
                && !matches!(f, None | Some(Format::Usd) | Some(Format::Short))
            {
                continue;
            }
            t.eq(format!("{{v:{}}} with v={v:?}", fname(*f)), note, want.to_string(), || hole(*f, v));
        }
    }
    // a plain number without a formatter
    let nums: Vec<(f64, &str, &str)> = vec![
        (3.0, "3", ""),
        (2.5, "2.5", ""),
        (-7.0, "-7", ""),
        (0.0, "0", "zero is there, not a dash"),
        (1234.0, "1234", "DECIDED: no thousands separator is mentioned"),
        (1e9, "1000000000", "DECIDED: reading: whole"),
        (1.0 / 3.0, "0.33", "DECIDED: a plain number; reading: two decimals at most, like n"),
    ];
    for (x, want, note) in nums {
        t.eq(format!("{{v}} with v=Num({x:?})"), note, want.to_string(), || hole(None, &n(x)));
    }
    t.eq("{v} with v=Missing".into(), "", DASH.to_string(), || hole(None, &Value::Missing));
    t.finish("no_formatter_and_formatters_that_do_not_fit");
}

#[test]
fn missing_is_a_dash_and_empty_text_too() {
    let mut t = Tally::new();
    let w = world();
    for f in Format::ALL {
        t.eq(format!("{{nope:{}}}", f.word()), "", DASH.to_string(), || show(&format!("{{nope:{}}}", f.word()), &w));
    }
    // (template, want, note)
    let rows: &[(&str, &str, &str)] = &[
        ("{nope}", DASH, "a root nobody defines"),
        ("x{nope}y", "x—y", "in place"),
        ("{nope}{nope}", "——", ""),
        ("[{nope}]", "[—]", ""),
        ("{a.nope}", DASH, "a path that leads nowhere"),
        ("{a.b[5]}", DASH, ""),
        ("{a.s.x}", DASH, "a name into text"),
        ("{l[9]:usd}", DASH, ""),
        ("{l[0].x}", DASH, ""),
        ("{/nope}", DASH, "a pointer that leads nowhere"),
        ("{/nope:n}", DASH, ""),
        ("{/content/5/text:short}", DASH, ""),
        ("{/usage/nope/x}", DASH, ""),
        ("{since(nope):dur}", DASH, "since of nothing is nothing"),
        ("{len(nope)}", DASH, "len of nothing is nothing"),
        ("{last(nope)}", DASH, "last of an empty list is nothing"),
        ("{lower(nope)}", DASH, ""),
        ("{sum(nope)}", "0", "sum reads missing as an empty list: nothing to add is 0"),
        ("{count(nope)}", "0", "count reads missing as an empty list"),
        ("{same(nope)}", "false", "same of an empty list is no"),
        ("{zero}", "0", "zero is there, not missing"),
        ("{zero:usd}", "$0.00", ""),
        ("{zero:pct}", "0%", ""),
        ("{zero:n}", "0", ""),
        ("{zero:short}", "0", ""),
        ("{zero:dur}", "0s", "zero"),
        ("{no}", "false", "false is there, not missing"),
        ("{no:text}", "false", ""),
        ("{nope == 1}", "false", "nothing compares with nothing: the answer false is there"),
        ("{nope != 1}", "false", ""),
        ("{not nope}", "true", ""),
        ("{empty}", DASH, "DECIDED: empty text; the brief says a dash, the doc comments do not"),
        ("{empty:text}", DASH, "DECIDED: empty text; the brief says a dash, the doc comments do not"),
        ("{empty:short}", DASH, "DECIDED: empty text; the brief says a dash, the doc comments do not"),
        ("[{empty}]", "[—]", "DECIDED: empty text; the brief says a dash, the doc comments do not"),
        ("{'':text}", DASH, "DECIDED: empty text literal"),
        ("{'x'}{empty}{'y'}", "x—y", "DECIDED: empty text in the middle"),
    ];
    for &(tpl, want, note) in rows {
        t.eq(format!("render({tpl:?})"), note, want.to_string(), || show(tpl, &w));
    }
    t.eq("explicit Missing root".into(), "", DASH.to_string(), || show("{v:usd}", &scope(Value::Missing)));
    t.eq("no root at all, empty Scope".into(), "", DASH.to_string(), || show("{v}", &Scope::new()));
    t.finish("missing_is_a_dash_and_empty_text_too");
}

#[test]
fn quoted_colons_and_braces_in_holes() {
    let mut t = Tally::new();
    let w = world();
    // (template, want, note); only the last colon outside quotes is a formatter, and a quoted brace does not end a hole
    let rows: &[(&str, &str, &str)] = &[
        ("{'a:b'}", "a:b", ""),
        ("{\"a:b\"}", "a:b", ""),
        ("{'}'}", "}", ""),
        ("{'{'}", "{", ""),
        ("{\"}\"}", "}", ""),
        ("{\"{\"}", "{", ""),
        ("{'a:b':text}", "a:b", "colon in the quotes, then the real formatter"),
        ("{'a:b':short}", "a:b", ""),
        ("{\"a:b\":text}", "a:b", ""),
        ("{'x}y'}", "x}y", ""),
        ("{'x}y':text}", "x}y", ""),
        ("{'x{y'}", "x{y", ""),
        ("{'}}'}", "}}", "braces in quotes are not escapes"),
        ("{'{{'}", "{{", "braces in quotes are not escapes"),
        ("{'}:{'}", "}:{", ""),
        ("{'a\\'}b'}", "a'}b", "an escaped quote does not end the text"),
        ("{\"a\\\"}b\"}", "a\"}b", "an escaped double quote does not end the text"),
        ("{'a\\\\'}", "a\\", "an escaped backslash, then the quote ends the text and the brace ends the hole"),
        ("{'a\\nb'}", "a\nb", "text escape \\n"),
        ("{'a\\tb'}", "a\tb", "text escape \\t"),
        ("{\"a'b:c\"}", "a'b:c", "a single quote inside double quotes"),
        ("{'a\"b:c'}", "a\"b:c", "a double quote inside single quotes"),
        ("x {'a:b'} y {'}'} z", "x a:b y } z", ""),
        ("{'a:b'}{'}'}", "a:b}", ""),
        ("{'a:b'} {{ {'}}'} }}", "a:b { }} }", "escapes outside, quotes inside"),
        ("{'x'}:{'y'}", "x:y", "a colon between holes is literal"),
        ("{lower('A:B')}", "a:b", ""),
        ("{lower('A:B'):text}", "a:b", ""),
        ("{len('}:{')}", "3", ""),
        ("{ 'a:b' }", "a:b", "DECIDED: spaces around an expression"),
        ("{'a:b' :text}", "a:b", "DECIDED: space before the formatter colon"),
        ("{'a b'}", "a b", ""),
        ("{'a:b':usd}", "a:b", "DECIDED: a formatter that does not fit text"),
        ("{/usage/tokens:short}", "1.2k", "a pointer, then the formatter"),
        ("{/usage/cost_usd:usd}", "$0.50", ""),
        ("{/content/0/text}", "hello", ""),
        ("{/type=='text'}", "true", "a pointer ends at '=' and the rest is a comparison"),
    ];
    for &(tpl, want, note) in rows {
        t.eq(format!("render({tpl:?})"), note, want.to_string(), || show(tpl, &w));
    }
    // a value holding colons and braces is just text
    for text in ["a:b", "x:usd", "}", "{", "{v}", "}}", "{{", "a}b{c"] {
        let v = sv(text);
        t.eq(format!("{{v}} with v={text:?}"), "", text.to_string(), || hole(None, &v));
        t.eq(format!("{{v:text}} with v={text:?}"), "", text.to_string(), || hole(Some(Format::Text), &v));
    }
    t.finish("quoted_colons_and_braces_in_holes");
}

#[test]
fn holes_hold_expressions_paths_and_pointers() {
    let mut t = Tally::new();
    let w = world();
    // (template, want, note)
    let rows: &[(&str, &str, &str)] = &[
        ("{a.s}", "hi", ""),
        ("{a.b[0]}", "10", ""),
        ("{a.b[1]}", "20", ""),
        ("{a['s']}", "hi", ""),
        ("{a.t}", "$1.50", ""),
        ("{a.t:usd}", "$1.50", ""),
        ("{a.b[0]:usd}", "$10.00", "a number counted in dollars"),
        ("{a.b[1]:dur}", "20s", "a number counted in seconds"),
        ("{a.b[0]:pct}", "1000%", "over one"),
        ("{a.b[1]:short}", "20", ""),
        ("{len(a.b)}", "2", ""),
        ("{len(a.s)}", "2", ""),
        ("{lower('HeLLo')}", "hello", ""),
        ("{count(a.b)}", "2", ""),
        ("{sum(a.b)}", "30", ""),
        ("{last(a.b)}", "20", ""),
        ("{a.s == 'hi'}", "true", ""),
        ("{a.s != 'hi'}", "false", ""),
        ("{not a.s}", "false", ""),
        ("{a.s in ['hi', 'yo']}", "true", ""),
        ("{'h' in a.s}", "true", ""),
        ("{a.s matches 'h*'}", "true", ""),
        ("{a.s matches 'x*'}", "false", ""),
        ("{a.b[0] < a.b[1]}", "true", ""),
        ("{a.b[0] < a.b[1] and a.s == 'hi'}", "true", ""),
        ("{1h30m:dur}", "1h30m", "a duration literal"),
        ("{90s}", "1m30s", "DECIDED: a duration with no formatter; reading: as under dur"),
        ("{$1.50}", "$1.50", ""),
        ("{$0.0004}", "$0.0004", ""),
        ("{'text'}", "text", ""),
        ("{12}", "12", ""),
        ("{0.5}", "0.5", ""),
        ("{-3}", "-3", ""),
        ("{true}", "true", ""),
        ("{count([1, 2, 3])}", "3", ""),
        ("{sum([$1.50, $2.50]):usd}", "$4.00", ""),
        ("{sum([1h, 30m]):dur}", "1h30m", ""),
        ("{since(t0):dur}", "6s", "10000 now, 4000 then"),
        ("{quote('hi')}", "\"hi\"", ""),
        ("{/usage/cost_usd}", "$0.50", ""),
        ("{/usage/tokens}", "1234", "DECIDED: a plain number is shown whole"),
        ("{/usage/tokens:n}", "1234", "DECIDED: no thousands separator"),
        ("{/content/0/text:short}", "hello", ""),
        ("{/usage/cost_usd} of {/usage/tokens:short}", "$0.50 of 1.2k", ""),
        ("{/type == 'text'}", "true", ""),
        ("{{{a.s}}}", "{hi}", "an escape, a hole, an escape"),
        ("{{{{{a.s}}}}}", "{{hi}}", ""),
        ("{a.s}{a.s}", "hihi", "holes side by side"),
        ("{a.s}-{a.s}", "hi-hi", ""),
        ("{a.s:text}{a.t:usd}{a.b[0]:n}", "hi$1.5010", ""),
        ("{a.s} and {a.t:usd} / {a.b[0]:n}!", "hi and $1.50 / 10!", ""),
        ("total {sum(a.b):n} in {since(t0):dur}", "total 30 in 6s", ""),
        ("{ a.s }", "hi", "DECIDED: spaces around an expression"),
        ("{  a.s  :text}", "hi", "DECIDED: spaces around an expression and before the colon"),
        ("{a.s: text}", "hi", "DECIDED: space after the colon"),
        ("{a.s:text }", "hi", "DECIDED: space after the formatter"),
    ];
    for &(tpl, want, note) in rows {
        t.eq(format!("render({tpl:?})"), note, want.to_string(), || show(tpl, &w));
    }
    // the same template renders the same twice
    t.holds("render is repeatable".into(), "", || {
        let f = Fmt::parse("{a.s} {a.t:usd} {since(t0):dur} {/usage/tokens:short}").map_err(|e| format!("{e:?}"))?;
        let (x, y) = (f.render(&w), f.render(&w));
        if x == y { Ok(()) } else { Err(format!("{x:?} then {y:?}")) }
    });
    t.holds("one parsed Fmt renders over different worlds".into(), "", || {
        let f = Fmt::parse("{v:usd}").map_err(|e| format!("{e:?}"))?;
        let (x, y) = (f.render(&scope(n(1.0))), f.render(&scope(n(2.5))));
        if (x.as_str(), y.as_str()) == ("$1.00", "$2.50") { Ok(()) } else { Err(format!("{x:?} and {y:?}")) }
    });
    t.finish("holes_hold_expressions_paths_and_pointers");
}

struct Spy {
    seen: RefCell<Vec<(String, Vec<Key>)>>,
}

impl Env for Spy {
    fn get(&self, root: &str, path: &[Key]) -> Value {
        self.seen.borrow_mut().push((root.to_string(), path.to_vec()));
        Value::Missing
    }
}

#[test]
fn holes_ask_the_env_for_root_and_steps() {
    let mut t = Tally::new();
    let k = |x: &str| Key::Name(x.to_string());
    // (template, root, path asked); `.name` and `["name"]` are names, `[0]` is a position
    let rows: Vec<(&str, &str, Vec<Key>, &str)> = vec![
        ("{a}", "a", vec![], ""),
        ("{a:usd}", "a", vec![], "the formatter is not a path step"),
        ("{a.b}", "a", vec![k("b")], ""),
        ("{a.b.c}", "a", vec![k("b"), k("c")], ""),
        ("{a[0]}", "a", vec![Key::Idx(0)], ""),
        ("{a[3]}", "a", vec![Key::Idx(3)], ""),
        ("{a['k']}", "a", vec![k("k")], ""),
        ("{a[\"k\"]}", "a", vec![k("k")], ""),
        ("{a.b[1].c}", "a", vec![k("b"), Key::Idx(1), k("c")], ""),
        ("{a.b[1].c:short}", "a", vec![k("b"), Key::Idx(1), k("c")], ""),
        ("x {a.b} y {a.b}", "a", vec![k("b")], "asked at least once"),
        ("{len(a.b)}", "a", vec![k("b")], "a path inside a call"),
        ("{a.b == 1}", "a", vec![k("b")], "a path inside a comparison"),
        ("{count(r.xs, a.b)}", "a", vec![k("b")], "a path inside a call argument"),
        ("{count(r.xs, a.b)}", "r", vec![k("xs")], ""),
        ("{l[i]}", "i", vec![], "an index expression is evaluated: its root is asked"),
    ];
    for (tpl, root, path, note) in rows {
        t.holds(format!("{tpl} asks {root:?} for {path:?}"), note, || {
            let f = Fmt::parse(tpl).map_err(|e| format!("parse: {e:?}"))?;
            let spy = Spy { seen: RefCell::new(Vec::new()) };
            if !spy.seen.borrow().is_empty() {
                return Err("asked before render".into());
            }
            let out = f.render(&spy);
            if !spy.seen.borrow().iter().any(|(r, p)| r == root && *p == path) {
                return Err(format!("asked {:?}", spy.seen.borrow()));
            }
            if out.contains('{') || out.contains('}') {
                return Err(format!("odd output {out:?}"));
            }
            Ok(())
        });
    }
    t.holds("parsing asks the world nothing; a literal-only template asks nothing at all".into(), "", || {
        let f = Fmt::parse("just text {{ok}}").map_err(|e| format!("parse: {e:?}"))?;
        let spy = Spy { seen: RefCell::new(Vec::new()) };
        let out = f.render(&spy);
        if out != "just text {ok}" || !spy.seen.borrow().is_empty() {
            return Err(format!("out {out:?} asked {:?}", spy.seen.borrow()));
        }
        Ok(())
    });
    t.finish("holes_ask_the_env_for_root_and_steps");
}

#[test]
fn parse_errors_say_where() {
    let mut t = Tally::new();
    // (template, lowest and highest sensible `at`, note); `at` counts characters from 0
    let rows: &[(&str, usize, usize, &str)] = &[
        ("{", 0, 0, "unclosed: at the brace"),
        ("{x", 0, 0, "unclosed: at the brace"),
        ("abc {x", 4, 4, "unclosed: at the brace"),
        ("{x:usd", 0, 0, "unclosed: at the brace"),
        ("a {x} b {y", 8, 8, "the second hole is the unclosed one"),
        ("{x}{", 3, 3, "unclosed: at the brace"),
        ("{'a'", 0, 0, "quotes closed, brace not"),
        ("{'a}", 0, 1, "an unclosed quote swallows the brace: at the hole or at the quote"),
        ("{\"a}", 0, 1, "an unclosed quote swallows the brace: at the hole or at the quote"),
        ("{'a:b}", 0, 1, "an unclosed quote swallows the brace: at the hole or at the quote"),
        ("}", 0, 0, "stray close brace"),
        ("a}b", 1, 1, "stray close brace"),
        ("{{x}", 3, 3, "an escape, then x, then a lone close brace"),
        ("x }}} y", 4, 4, "an escape, then a lone close brace"),
        ("}}}", 2, 2, "an escape, then a lone close brace"),
        ("{x}}", 3, 3, "a hole, then a lone close brace"),
        ("{x}}}}", 5, 5, "a hole, an escape, then a lone close brace: at the last one"),
        ("é }", 2, 2, "counts characters, not bytes"),
        ("日本 {x", 3, 3, "counts characters, not bytes"),
        ("😀😀}", 2, 2, "counts characters, not bytes"),
        ("{}", 0, 1, "empty hole"),
        ("{ }", 0, 2, "empty hole"),
        ("{   }", 0, 4, "empty hole"),
        ("{:usd}", 0, 1, "empty expression before a formatter"),
        ("a {} b", 2, 3, "empty hole"),
        ("{x}{}", 3, 4, "the second hole is empty"),
        ("{x:}", 0, 3, "a colon and no formatter"),
        ("{x:nope}", 0, 3, "unknown formatter"),
        ("ab {x:nope} cd", 3, 6, "unknown formatter"),
        ("{x:USD}", 0, 3, "formatters are lowercase words"),
        ("{x:Usd}", 0, 3, "formatters are lowercase words"),
        ("{x:usdd}", 0, 3, "unknown formatter"),
        ("{x:us}", 0, 3, "unknown formatter"),
        ("{x:1}", 0, 3, "unknown formatter"),
        ("{/a:b}", 0, 4, "a pointer ends at the colon: b is an unknown formatter"),
        ("{x:usd:n}", 0, 7, "the last colon splits: the expression x:usd is not one"),
        ("{x:usd:nope}", 0, 8, "the last colon splits"),
        ("ab {x +} cd", 4, 8, "an error inside a hole is placed in the template, not in the hole"),
        ("pre {1 <} post", 5, 8, "an error inside a hole is placed in the template, not in the hole"),
        ("12345678 {x +} z", 12, 14, "an error inside a hole is placed in the template, not in the hole"),
        ("{a b}", 1, 4, "two names"),
        ("{foo(1)}", 1, 7, "no such function"),
        ("{1 < 2 < 3}", 1, 10, "a comparison is between two things"),
        ("{and}", 1, 3, "a word cannot be a name"),
        ("{x.}", 1, 3, "a dot and no name"),
        ("{x[}", 1, 3, "unclosed bracket"),
        ("{(x}", 1, 3, "unclosed parenthesis"),
        ("{len(}", 1, 5, "a call and no argument close"),
        ("{x:text} {y:bad}", 9, 12, "the second hole has the unknown formatter"),
    ];
    for &(tpl, lo, hi, note) in rows {
        t.holds(format!("Fmt::parse({tpl:?}) fails with at in {lo}..={hi}"), note, || match Fmt::parse(tpl) {
            Ok(f) => Err(format!("parsed fine: {:?}", f.pieces())),
            Err(e) if e.msg.is_empty() => Err(format!("empty message, at={}", e.at)),
            Err(e) if e.at < lo || e.at > hi => Err(format!("at={} msg={:?}", e.at, e.msg)),
            Err(_) => Ok(()),
        });
    }
    // the ones that must parse, though they look odd
    let fine: &[(&str, &str)] = &[
        ("{{", "doubled braces"),
        ("}}", "doubled braces"),
        ("{x:usd}", "plain hole"),
        ("{ x }", "DECIDED: spaces around the expression"),
        ("{a.b[0]:short}", "path hole"),
        ("{/a/b}", "pointer hole"),
        ("{'}'}", "quoted brace"),
        ("{'a:b'}", "quoted colon"),
        ("}}{x}{{", "escapes around a hole"),
        ("{{{x}}}", "escape, hole, escape"),
    ];
    for &(tpl, note) in fine {
        t.holds(format!("Fmt::parse({tpl:?}) succeeds"), note, || match Fmt::parse(tpl) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("at={} msg={:?}", e.at, e.msg)),
        });
    }
    t.finish("parse_errors_say_where");
}

#[test]
fn expression_limits_inside_holes() {
    let mut t = Tally::new();
    t.holds(format!("Expr::parse of a name of exactly MAX_LEN={MAX_LEN} characters works"), "", || {
        match Expr::parse(&"a".repeat(MAX_LEN)) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("at={} msg={:?}", e.at, e.msg)),
        }
    });
    t.holds("Expr::parse of a name of MAX_LEN+1 characters is refused".to_string(), "", || {
        match Expr::parse(&"a".repeat(MAX_LEN + 1)) {
            Ok(_) => Err("parsed fine".into()),
            Err(_) => Ok(()),
        }
    });
    let nest = |depth: usize| format!("{}1{}", "(".repeat(depth), ")".repeat(depth));
    t.holds(
        format!("Expr::parse nested exactly MAX_DEPTH={MAX_DEPTH} parentheses deep works"),
        "DECIDED: does the whole expression count as a level?",
        || match Expr::parse(&nest(MAX_DEPTH)) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("at={} msg={:?}", e.at, e.msg)),
        },
    );
    t.holds("Expr::parse nested MAX_DEPTH+1 parentheses deep is refused".to_string(), "", || {
        match Expr::parse(&nest(MAX_DEPTH + 1)) {
            Ok(_) => Err("parsed fine".into()),
            Err(_) => Ok(()),
        }
    });
    t.holds(
        format!("a template of exactly MAX_LEN={MAX_LEN} literal characters parses"),
        "DECIDED: the doc names a limit for an expression only",
        || match Fmt::parse(&"x".repeat(MAX_LEN)) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("at={} msg={:?}", e.at, e.msg)),
        },
    );
    t.holds(
        "a template of MAX_LEN+1 literal characters parses".to_string(),
        "DECIDED: the doc names a limit for an expression only",
        || match Fmt::parse(&"x".repeat(MAX_LEN + 1)) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("at={} msg={:?}", e.at, e.msg)),
        },
    );
    t.holds(
        "a hole whose braces and expression together are MAX_LEN characters parses".to_string(),
        "DECIDED: the doc names a limit for an expression only",
        || match Fmt::parse(&format!("{{{}}}", "a".repeat(MAX_LEN - 2))) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("at={} msg={:?}", e.at, e.msg)),
        },
    );
    let name_ok = format!("{{{}}}", "a".repeat(MAX_LEN));
    let name_long = format!("{{{}}}", "a".repeat(MAX_LEN + 1));
    t.holds(
        format!("a hole of exactly MAX_LEN={MAX_LEN} characters parses"),
        "DECIDED: the limit is of the expression in a hole (Expr::parse takes this one), not of the template",
        || match Fmt::parse(&name_ok) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("at={} msg={:?}", e.at, e.msg)),
        },
    );
    t.holds(
        "a hole of MAX_LEN+1 characters is refused".to_string(),
        "DECIDED: the limit counts the expression of a hole",
        || match Fmt::parse(&name_long) {
            Ok(_) => Err("parsed fine".into()),
            Err(e) if e.at > MAX_LEN + 2 => Err(format!("at={} is past the end", e.at)),
            Err(_) => Ok(()),
        },
    );
    let two_holes = format!("{{{0}}}{{{0}}}", "a".repeat(MAX_LEN));
    t.holds(
        "two holes of MAX_LEN characters each parse".into(),
        "DECIDED: the limit is per expression, not for the template",
        || match Fmt::parse(&two_holes) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("at={} msg={:?}", e.at, e.msg)),
        },
    );
    let nested = |depth: usize| format!("{{{}1{}}}", "(".repeat(depth), ")".repeat(depth));
    t.eq(
        format!("a hole nested {MAX_DEPTH} deep renders"),
        "DECIDED: does the whole expression count as a level?",
        "1".to_string(),
        || show(&nested(MAX_DEPTH), &Scope::new()),
    );
    t.holds(format!("a hole nested {} deep is refused", MAX_DEPTH + 1), "", || {
        match Fmt::parse(&nested(MAX_DEPTH + 1)) {
            Ok(_) => Err("parsed fine".into()),
            Err(_) => Ok(()),
        }
    });
    t.holds(
        "a thousand holes in one template".into(),
        "DECIDED: no limit on a template is documented, only on an expression",
        || {
            let tpl = "{a.s}".repeat(1000);
            let out = show(&tpl, &world());
            if out == "hi".repeat(1000) { Ok(()) } else { Err(format!("{} characters", out.chars().count())) }
        },
    );
    t.holds("a very deeply nested hole is refused, not a stack overflow".into(), "", || {
        let tpl = nested(5000);
        match Fmt::parse(&tpl) {
            Ok(_) => Err("parsed fine".into()),
            Err(_) => Ok(()),
        }
    });
    t.finish("expression_limits_inside_holes");
}

#[test]
fn display_round_trips() {
    let mut t = Tally::new();
    let templates: &[&str] = &[
        "",
        "plain",
        "a {{ b }} c",
        "{{}}",
        "}}{{",
        "{{{x}}}",
        "a}}b",
        "{x}",
        "{x:usd}",
        "{x:pct}",
        "{x:n}",
        "{x:dur}",
        "{x:text}",
        "{x:short}",
        "{a.b[0]:short}",
        "{a['k'].c}",
        "{'a:b'}",
        "{'}'}",
        "{'{'}",
        "{\"x}y\":text}",
        "{'it\\'s'}",
        "{'back\\\\slash'}",
        "{'line\\nbreak'}",
        "{/a/b:n}",
        "{/a/b}",
        "{/type=='text'}",
        "{x == 1}",
        "{x != 'a'}",
        "{not x}",
        "{x and y or z}",
        "{(x or y) and z}",
        "{count(xs, 'a'):n}",
        "{sum([$1.50, $2.50]):usd}",
        "{1h30m:dur}",
        "{250ms:dur}",
        "{$0.0004:usd}",
        "{-3}",
        "{0.5:pct}",
        "{count([1, 2])}",
        "{x in [1, 2]}",
        "{'a' in s}",
        "{s matches 'a*'}",
        "{x}{y}",
        "{x:dur} of {y:usd} ({z:pct})",
        "é{x}—✓",
        "line1\nline2 {x}\n",
        "tab\t{x}\t",
        "lots of {{braces}} and {x} and }}{{",
        "{since(t):dur} ago",
        "{lower(quote(x))}",
    ];
    let env = Scope::new()
        .with("x", n(3.0))
        .with("y", usd(2.5))
        .with("z", n(0.5))
        .with("s", sv("abc"))
        .with("a", map(&[("b", list(vec![n(1.0)])), ("k", map(&[("c", sv("deep"))]))]))
        .with("xs", list(vec![sv("a"), sv("b"), sv("a")]))
        .with("t", n(1000.0))
        .at_ms(61_000)
        .doc(map(&[("a", map(&[("b", n(1234.0))])), ("type", sv("text"))]));
    for tpl in templates {
        t.holds(format!("Fmt::parse({tpl:?}) printed and parsed again"), "", || {
            let f = Fmt::parse(tpl).map_err(|e| format!("first parse: at={} {:?}", e.at, e.msg))?;
            let printed = f.to_string();
            let g = Fmt::parse(&printed).map_err(|e| format!("re-parse of {printed:?}: at={} {:?}", e.at, e.msg))?;
            if g != f {
                return Err(format!("printed {printed:?}; pieces differ: {:?} vs {:?}", f.pieces(), g.pieces()));
            }
            if g.to_string() != printed {
                return Err(format!("printing again differs: {printed:?} vs {:?}", g.to_string()));
            }
            let (x, y) = (f.render(&env), g.render(&env));
            if x != y {
                return Err(format!("renders differ: {x:?} vs {y:?}"));
            }
            Ok(())
        });
    }
    t.holds(
        "a template with no holes prints as the same text".into(),
        "DECIDED: the doc promises a round trip, not the same characters; reading: plain text is printed as written",
        || {
            let f = Fmt::parse("just some words").map_err(|e| format!("{e:?}"))?;
            if f.to_string() == "just some words" { Ok(()) } else { Err(format!("{:?}", f.to_string())) }
        },
    );
    t.holds("literal braces print so they parse back (escaped)".into(), "", || {
        let f = Fmt::parse("a{{b}}c").map_err(|e| format!("{e:?}"))?;
        let printed = f.to_string();
        if printed.contains('{') && Fmt::parse(&printed).map(|g| g == f).unwrap_or(false) {
            Ok(())
        } else {
            Err(format!("printed {printed:?}"))
        }
    });
    t.finish("display_round_trips");
}

#[test]
fn pieces_of_a_parsed_template() {
    let mut t = Tally::new();
    let ex = |s: &str| Expr::parse(s).unwrap_or_else(|e| panic!("test expression {s:?}: {e:?}"));
    let lit = |s: &str| Piece::Lit(s.to_string());
    let hole = |e: &str, f: Option<Format>| Piece::Ref { expr: ex(e), format: f };
    // (template, pieces, note)
    let rows: Vec<(&str, Vec<Piece>, &str)> = vec![
        ("a{x:usd}b", vec![lit("a"), hole("x", Some(Format::Usd)), lit("b")], ""),
        ("{x}", vec![hole("x", None)], ""),
        ("{x:pct}", vec![hole("x", Some(Format::Pct))], ""),
        ("{x:n}", vec![hole("x", Some(Format::N))], ""),
        ("{x:dur}", vec![hole("x", Some(Format::Dur))], ""),
        ("{x:text}", vec![hole("x", Some(Format::Text))], ""),
        ("{x:short}", vec![hole("x", Some(Format::Short))], ""),
        ("{x}{y}", vec![hole("x", None), hole("y", None)], "no empty literal between"),
        ("{x} {y}", vec![hole("x", None), lit(" "), hole("y", None)], ""),
        ("{x == 1}", vec![hole("x == 1", None)], ""),
        ("{a.b[0]:short}", vec![hole("a.b[0]", Some(Format::Short))], ""),
        ("{/a/b:n}", vec![hole("/a/b", Some(Format::N))], ""),
        ("{'a:b'}", vec![hole("'a:b'", None)], ""),
        ("{'a:b':text}", vec![hole("'a:b'", Some(Format::Text))], ""),
        ("{'}'}", vec![hole("'}'", None)], ""),
        ("{{}}", vec![lit("{}")], "escapes become literal braces"),
        ("a{{b", vec![lit("a{b")], "DECIDED: an escape inside a run of text; reading: one literal piece"),
        ("a{{b}}c", vec![lit("a{b}c")], "DECIDED: reading: one literal piece"),
        ("{{{x}}}", vec![lit("{"), hole("x", None), lit("}")], ""),
        ("plain", vec![lit("plain")], ""),
        ("", vec![], "DECIDED: empty template; reading: no pieces (not one empty literal)"),
        ("a\nb", vec![lit("a\nb")], ""),
    ];
    for (tpl, want, note) in rows {
        t.eq(format!("Fmt::parse({tpl:?}).pieces()"), note, want, || match Fmt::parse(tpl) {
            Ok(f) => f.pieces().to_vec(),
            Err(e) => vec![Piece::Lit(format!("PARSE ERROR at {}: {}", e.at, e.msg))],
        });
    }
    t.finish("pieces_of_a_parsed_template");
}

#[test]
fn the_closed_set_of_formatters() {
    let mut t = Tally::new();
    let words: Vec<&str> = Format::ALL.iter().map(|f| f.word()).collect();
    t.eq(
        "Format::ALL words, in the order the doc lists them".into(),
        "",
        vec!["usd", "pct", "n", "dur", "text", "short", "rungs", "runs", "fallback"],
        || words.clone(),
    );
    for f in Format::ALL {
        t.holds(format!("{{x:{}}} parses to {f:?}", f.word()), "", || {
            let parsed = Fmt::parse(&format!("{{x:{}}}", f.word())).map_err(|e| format!("at={} {:?}", e.at, e.msg))?;
            match parsed.pieces() {
                [Piece::Ref { format: Some(got), .. }] if *got == f => Ok(()),
                other => Err(format!("pieces {other:?}")),
            }
        });
    }
    t.holds("every word of Format::ALL is distinct".into(), "", || {
        for (i, a) in words.iter().enumerate() {
            if words.iter().skip(i + 1).any(|b| a == b) {
                return Err(format!("{a:?} twice"));
            }
        }
        Ok(())
    });
    for bad in [
        "USD", "Usd", "percent", "money", "num", "number", "int", "float", "date", "json", "upper", "lower", "len",
        "x", "1", "usd2", "_usd", "u", "shor", "shortt",
    ] {
        t.holds(format!("{{x:{bad}}} is not a formatter"), "", || match Fmt::parse(&format!("{{x:{bad}}}")) {
            Ok(f) => Err(format!("parsed as {:?}", f.pieces())),
            Err(_) => Ok(()),
        });
    }
    t.finish("the_closed_set_of_formatters");
}

#[test]
fn format_value_direct() {
    let mut t = Tally::new();
    // (value, formatter, want, note)
    let rows: Vec<(Value, Option<Format>, &str, &str)> = vec![
        (Value::Missing, None, DASH, ""),
        (Value::Missing, Some(Format::Usd), DASH, ""),
        (Value::Missing, Some(Format::Pct), DASH, ""),
        (Value::Missing, Some(Format::N), DASH, ""),
        (Value::Missing, Some(Format::Dur), DASH, ""),
        (Value::Missing, Some(Format::Text), DASH, ""),
        (Value::Missing, Some(Format::Short), DASH, ""),
        (n(3.0), Some(Format::Usd), "$3.00", ""),
        (usd(12.5), Some(Format::Usd), "$12.50", ""),
        (usd(0.004), Some(Format::Usd), "$0.0040", ""),
        (usd(0.00004), Some(Format::Usd), "<$0.0001", ""),
        (n(0.666), Some(Format::Pct), "67%", ""),
        (n(5.0), Some(Format::Pct), "500%", ""),
        (n(7.14159), Some(Format::N), "7.14", ""),
        (Value::Dur(850), Some(Format::Dur), "850ms", ""),
        (n(185.0), Some(Format::Dur), "3m05s", ""),
        (sv("x"), Some(Format::Text), "x", ""),
        (n(1234.0), Some(Format::Short), "1.2k", ""),
        (n(2e9), Some(Format::Short), "2B", ""),
        (Value::List(vec![sv("rule:gate"), sv("human")]), Some(Format::Rungs), "a rule, then you", "rungs"),
        (Value::List(vec![sv("human")]), Some(Format::Rungs), "you", "rungs"),
        (Value::List(vec![sv("rank"), sv("human?after=30s")]), Some(Format::Rungs), "a ranking, then you", "rungs"),
        (Value::List(vec![sv("decider:jev?min_conf=0.5")]), Some(Format::Rungs), "a decider", "rungs"),
        (Value::List(vec![sv("rule:a"), sv("rule:b"), sv("human")]), Some(Format::Rungs), "a rule, then you", "a repeat is said once"),
        (sv("not a list"), Some(Format::Rungs), "not a list", "rungs"),
        (usd(1.5), None, "$1.50", "DECIDED: money with no formatter"),
        (Value::Dur(45_000), None, "45s", "DECIDED: a duration with no formatter"),
        (n(2.5), None, "2.5", ""),
        (sv("hi"), None, "hi", ""),
        (Value::Bool(true), None, "true", ""),
    ];
    for (v, f, want, note) in rows {
        t.eq(format!("format_value({v:?}, {})", fname(f)), note, want.to_string(), || format_value(&v, f));
        let via_template =
            catch_unwind(AssertUnwindSafe(|| hole(f, &v))).unwrap_or_else(|_| "PANIC IN TEMPLATE".to_string());
        t.eq(
            format!("format_value({v:?}, {}) agrees with the same hole in a template", fname(f)),
            note,
            via_template,
            || format_value(&v, f),
        );
    }
    t.finish("format_value_direct");
}

#[test]
fn rendering_is_total_even_for_odd_values() {
    let mut t = Tally::new();
    let deep = (0..200).fold(n(1.0), |acc, _| list(vec![acc]));
    let odd: Vec<(&str, Value)> = vec![
        ("NaN", Value::Num(f64::NAN)),
        ("+inf", Value::Num(f64::INFINITY)),
        ("-inf", Value::Num(f64::NEG_INFINITY)),
        ("f64::MAX", Value::Num(f64::MAX)),
        ("f64::MIN", Value::Num(f64::MIN)),
        ("f64::MIN_POSITIVE", Value::Num(f64::MIN_POSITIVE)),
        ("subnormal", Value::Num(5e-324)),
        ("-0.0", Value::Num(-0.0)),
        ("Dur(u64::MAX)", Value::Dur(u64::MAX)),
        ("Dur(u64::MAX-1)", Value::Dur(u64::MAX - 1)),
        ("Usd(i64::MAX)", Value::Usd(i64::MAX)),
        ("Usd(i64::MIN)", Value::Usd(i64::MIN)),
        ("Usd(i64::MIN+1)", Value::Usd(i64::MIN + 1)),
        ("100000-character text", sv(&"é".repeat(100_000))),
        ("text of only spaces and breaks", sv(" \n\t \r\n ")),
        ("text with a nul", sv("a\0b")),
        ("text with a right-to-left mark", sv("a\u{202e}b")),
        ("list nested 200 deep", deep),
        ("a map with 1000 keys", Value::Map((0..1000).map(|i| (format!("k{i}"), n(i as f64))).collect())),
    ];
    let formats: Vec<Option<Format>> = vec![None].into_iter().chain(Format::ALL.iter().map(|f| Some(*f))).collect();
    for (what, v) in &odd {
        t.holds(format!("every formatter over {what} gives some text and does not panic"), "", || {
            for f in &formats {
                let out = hole(*f, v);
                if out.is_empty() && !matches!(v, Value::Str(_)) {
                    return Err(format!("{{v:{}}} gave nothing", fname(*f)));
                }
                if out.starts_with("<<PARSE ERROR") {
                    return Err(format!("{{v:{}}} gave {out}", fname(*f)));
                }
            }
            Ok(())
        });
    }
    t.holds("duration(0), duration(1) and duration(u64::MAX) never panic".into(), "", || {
        for ms in [0u64, 1, u64::MAX] {
            let _ = duration(ms);
        }
        Ok(())
    });
    t.finish("rendering_is_total_even_for_odd_values");
}

#[test]
fn check_finds_what_expr_check_finds_in_the_holes() {
    let mut t = Tally::new();
    let known: &[&str] = &["a", "b"];
    let a_only: &[&str] = &["a"];
    let xs_only: &[&str] = &["xs"];
    let nothing: &[&str] = &[];
    // (template, roots, fewest messages, most messages, a word every message set must mention, note)
    #[allow(clippy::type_complexity)]
    let rows: Vec<(&str, Option<&[&str]>, usize, usize, &str, &str)> = vec![
        ("{a} {b}", Some(a_only), 1, 1, "b", "one root not listed"),
        ("{a} {b}", Some(known), 0, 0, "", "both listed"),
        ("{a} {b}", None, 0, 0, "", "none given: any is fine"),
        ("{a.b.c}", Some(a_only), 0, 0, "", "the root is what counts, not the path"),
        ("{a[0].x:usd}", Some(a_only), 0, 0, "", ""),
        ("{xs[i]}", Some(xs_only), 1, 1, "i", "a root inside an index expression"),
        ("{a == c}", Some(a_only), 1, 1, "c", "a root inside a comparison"),
        ("{len(c)}", Some(a_only), 1, 1, "c", "a root inside a call"),
        ("{c}{c}", Some(a_only), 1, 2, "c", "DECIDED: the same unlisted root twice: one message or two"),
        ("{c} and {d}", Some(a_only), 2, 2, "d", "two different unlisted roots"),
        ("{a}", Some(nothing), 1, 1, "a", "no roots at all are listed"),
        ("plain text", Some(nothing), 0, 0, "", "no holes"),
        ("plain {{text}}", Some(a_only), 0, 0, "", "escaped braces are no holes"),
        ("", Some(nothing), 0, 0, "", "empty template"),
        ("{/p/q}", Some(nothing), 0, 0, "", "a pointer has no root"),
        ("{/p/q:usd} {a}", Some(a_only), 0, 0, "", ""),
        ("{len(1)}", None, 1, 5, "", "a number where len takes text, a list or a map"),
        ("{lower(1)}", None, 1, 5, "", "a number where lower takes text"),
        ("{5 matches 'x'}", None, 1, 5, "", "matches takes text on both sides"),
        ("{'a' matches 5}", None, 1, 5, "", "matches takes text on both sides"),
        ("{len('abc')}", None, 0, 0, "", "a fitting literal"),
        ("{len(a)}", Some(a_only), 0, 0, "", "a path: unknown until it runs"),
        ("{lower('A')}", None, 0, 0, "", ""),
        ("{'x' in 'xyz'}", None, 0, 0, "", ""),
        ("{a:usd} {b:short} {a.c:n}", Some(known), 0, 0, "", "formatters are no roots"),
    ];
    for (tpl, roots, lo, hi, mention, note) in rows {
        t.holds(format!("Fmt::parse({tpl:?}).check({roots:?}) gives {lo}..={hi} messages"), note, || {
            let f = Fmt::parse(tpl).map_err(|e| format!("parse: at={} {:?}", e.at, e.msg))?;
            let msgs = f.check(roots);
            if msgs.len() < lo || msgs.len() > hi {
                return Err(format!("{} messages: {msgs:?}", msgs.len()));
            }
            if msgs.iter().any(|m| m.is_empty()) {
                return Err(format!("an empty message in {msgs:?}"));
            }
            if !mention.is_empty() && !msgs.iter().any(|m| m.contains(mention)) {
                return Err(format!("no message mentions {mention:?}: {msgs:?}"));
            }
            Ok(())
        });
    }
    t.holds("check agrees with Expr::check on a single-hole template".into(), "", || {
        for (tpl, e) in
            [("{c + }", ""), ("{len(1)}", "len(1)"), ("{lower(1)}", "lower(1)"), ("{c}", "c"), ("{a.b}", "a.b")]
        {
            if e.is_empty() {
                continue;
            }
            let f = Fmt::parse(tpl).map_err(|er| format!("parse {tpl:?}: {er:?}"))?;
            let expr = Expr::parse(e).map_err(|er| format!("parse {e:?}: {er:?}"))?;
            for roots in [None, Some(&["a"][..]), Some(&[][..])] {
                let (x, y) = (f.check(roots), expr.check(roots));
                if x != y {
                    return Err(format!("{tpl:?} with roots {roots:?}: Fmt {x:?} vs Expr {y:?}"));
                }
            }
        }
        Ok(())
    });
    t.finish("check_finds_what_expr_check_finds_in_the_holes");
}

/// The formatter is after the *last* colon outside quotes; a pointer may hold colons of its own.
#[test]
fn the_last_colon_names_the_formatter() {
    let pieces = |t: &str| Fmt::parse(t).map(|f| f.pieces().to_vec());
    assert_eq!(
        pieces("{/x:y:usd}"),
        Ok(vec![Piece::Ref { expr: Expr::parse("/x:y").unwrap(), format: Some(Format::Usd) }])
    );
    assert_eq!(
        pieces("{'a:b':short}"),
        Ok(vec![Piece::Ref { expr: Expr::parse("'a:b'").unwrap(), format: Some(Format::Short) }])
    );
    assert_eq!(
        pieces("{/x:y}").map_err(|e| e.msg.contains("no formatter `y`")),
        Err(true),
        "the one colon is the formatter's, and `y` is none"
    );
}
