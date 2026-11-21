//! The fuzzer's file: deterministic property tests that try to break Expr, Fmt, Query and the values under them.
//! No dependencies: a xorshift PRNG, a generator of valid expression text, a generator of values and scopes.
//!
//! A test whose comment says `BUG:` is a property the code does not keep today; it stays here and stays red until the
//! code or the documentation changes. Everything else is expected green. Every loop prints no noise: a violation
//! panics at the end with the first few repros (the input as a Rust string literal, ready to paste into a test).

use j4n_core::eval::{Env, Scope, glob_match};
use j4n_core::expr::{Expr, Func, MAX_DEPTH, MAX_LEN, ParseError, QUOTE_MAX};
use j4n_core::fmt::{Fmt, Format, MAX_FMT, Piece, SHORT_MAX, duration, format_value};
use j4n_core::query::{Coll, MAX_LIMIT, MAX_ORDER, Query};
use j4n_core::value::{DASH, Key, Value};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

// ── iteration counts (each at least 20000 unless the work per iteration is large) ──────────────

const N_SOUP: usize = 40_000;
const N_SOUP_QUERY: usize = 30_000;
const N_MUTATED: usize = 30_000;
const N_VALID: usize = 20_000;
const N_ALGEBRA: usize = 20_000;
const N_FMT: usize = 20_000;
const N_QUERY: usize = 20_000;
const N_GLOB: usize = 60_000;
/// A loop stops once this many violations are seen: the first few are the repros.
const STOP: usize = 40;

// ── the PRNG ─────────────────────────────────────────────────────────────────────────────────

/// xorshift64*: deterministic, no dependencies.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03);
        if r.0 == 0 {
            r.0 = 0x1234_5678_9ABC_DEF1;
        }
        for _ in 0..8 {
            r.next();
        }
        r
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// 0..n (n > 0).
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    /// lo..=hi.
    fn between(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo + 1) as u64) as i64
    }

    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }

    fn weighted(&mut self, w: &[u64]) -> usize {
        let total: u64 = w.iter().sum();
        let mut x = self.next() % total;
        for (i, &k) in w.iter().enumerate() {
            if x < k {
                return i;
            }
            x -= k;
        }
        w.len() - 1
    }
}

// ── collecting violations and catching panics ───────────────────────────────────────────────

struct Sink {
    what: &'static str,
    bad: usize,
    first: Vec<String>,
}

impl Sink {
    fn new(what: &'static str) -> Sink {
        Sink { what, bad: 0, first: Vec::new() }
    }

    fn fail(&mut self, msg: String) {
        self.bad += 1;
        if self.first.len() < 5 {
            self.first.push(clip(msg, 1500));
        }
    }

    fn done(&self) -> bool {
        self.bad >= STOP
    }

    fn finish(self, iterations: usize) {
        if self.bad > 0 {
            panic!(
                "{}: {} violation(s) (a loop stops at {STOP}) within {iterations} iterations; the first {}:\n  - {}",
                self.what,
                self.bad,
                self.first.len(),
                self.first.join("\n  - ")
            );
        }
    }
}

fn caught<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|p| {
        p.downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| p.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "a panic".into())
    })
}

/// Run `f`; a panic is a violation naming `label` and `input`.
fn guarded<R>(sink: &mut Sink, label: &str, input: &str, f: impl FnOnce() -> R) -> Option<R> {
    match caught(f) {
        Ok(r) => Some(r),
        Err(msg) => {
            sink.fail(format!("PANIC in {label} on {input:?}: {msg}"));
            None
        }
    }
}

/// At most `max` characters of `s`, with a count of what was cut.
fn clip(s: String, max: usize) -> String {
    if chars(&s) <= max {
        return s;
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}… ({} more characters)", chars(&s) - max)
}

fn chars(s: &str) -> usize {
    s.chars().count()
}

fn pos_ok(e: &ParseError, input: &str) -> bool {
    e.at <= chars(input)
}

// ── what the language is made of, for the soup ───────────────────────────────────────────────

const TOKENS: &[&str] = &[
    "(",
    ")",
    "[",
    "]",
    ",",
    ".",
    "==",
    "!=",
    "<",
    "<=",
    ">",
    ">=",
    "=",
    "!",
    "&",
    "&&",
    "|",
    "||",
    "and",
    "or",
    "not",
    "in",
    "not in",
    "matches",
    "true",
    "false",
    "len",
    "count",
    "sum",
    "since",
    "same",
    "last",
    "lower",
    "contains",
    "danger",
    "quote",
    "(x)",
    "a",
    "a.b",
    "_id",
    "x1",
    "'",
    "\"",
    "'abc'",
    "\"x\"",
    "'it\\'s'",
    "\\",
    "\\n",
    "\\'",
    "{",
    "}",
    "{{",
    "}}",
    "{x}",
    "{a:usd}",
    ":",
    ":usd",
    ":pct",
    ":n",
    ":dur",
    ":text",
    ":short",
    ":bogus",
    "$",
    "$1.50",
    "$0.0004",
    "$0.5",
    "0",
    "1",
    "12",
    "-",
    "-3",
    "0.5",
    "1.",
    ".5",
    "1e5",
    "5m",
    "250ms",
    "1h30m",
    "90s",
    "1.5h",
    "5d",
    "5x",
    "1m30",
    "ms",
    "s",
    "m",
    "h",
    "d",
    "99999999999999999999999999",
    "/",
    "//",
    "/a/b",
    "/content/0/text",
    "/a~1b",
    "~0",
    "~1",
    "%",
    "@",
    "+",
    "*",
    "?",
    " ",
    "  ",
    "\t",
    "\n",
    "\r\n",
    "é",
    "e\u{301}",
    "\u{200b}",
    "\u{202e}",
    "\u{0}",
    "\u{feff}",
    "😀",
    "👨‍👩‍👧",
    "\u{ffff}",
    "日本語",
    "\u{3000}",
    "\u{85}",
    "\u{2028}",
    "tasks",
    "asks",
    "changes",
    "log:",
    "table:",
    "fold:",
    "thread:",
    "log:a*",
    "x*y?",
];

fn rand_char(rng: &mut Rng) -> char {
    loop {
        let cp = match rng.below(8) {
            0..=2 => rng.below(0x80) as u32,
            3 => 0x80 + rng.below(0x780) as u32,
            4 => 0x800 + rng.below(0xF800) as u32,
            5 => 0x300 + rng.below(0x70) as u32,
            6 => 0x1F300 + rng.below(0x400) as u32,
            _ => rng.below(0x11_0000) as u32,
        };
        if let Some(c) = char::from_u32(cp) {
            return c;
        }
    }
}

fn soup(rng: &mut Rng, max_tokens: usize) -> String {
    let mut s = String::new();
    for _ in 0..rng.below(max_tokens + 1) {
        if rng.chance(10) {
            s.push(rand_char(rng));
        } else {
            s.push_str(rng.pick(TOKENS));
        }
    }
    s
}

fn mutate(rng: &mut Rng, text: &str) -> String {
    let mut cs: Vec<char> = text.chars().collect();
    for _ in 0..1 + rng.below(3) {
        let n = cs.len();
        match rng.below(8) {
            0 if n > 0 => {
                cs.remove(rng.below(n));
            }
            1 => {
                let at = rng.below(n + 1);
                cs.insert(at, rand_char(rng));
            }
            2 => {
                let at = rng.below(n + 1);
                for (k, c) in rng.pick(TOKENS).chars().enumerate() {
                    cs.insert((at + k).min(cs.len()), c);
                }
            }
            3 if n > 1 => {
                let (a, b) = (rng.below(n), rng.below(n));
                cs.swap(a, b);
            }
            4 if n > 0 => cs.truncate(rng.below(n)),
            5 if n > 0 => {
                let a = rng.below(n);
                let b = (a + rng.below(20)).min(n);
                let dup: Vec<char> = cs[a..b].to_vec();
                let at = rng.below(n + 1);
                for (k, c) in dup.into_iter().enumerate() {
                    cs.insert((at + k).min(cs.len()), c);
                }
            }
            6 if n > 0 => {
                let at = rng.below(n);
                cs[at] = rand_char(rng);
            }
            _ if n > 0 => {
                let a = rng.below(n);
                let b = (a + 1 + rng.below(10)).min(n);
                cs.drain(a..b);
            }
            _ => {}
        }
    }
    cs.into_iter().collect()
}

// ── values and scopes ────────────────────────────────────────────────────────────────────────

const ROOTS: &[&str] =
    &["a", "b", "row", "fold", "set", "event", "task", "zz", "_x", "orx", "notx", "inx", "count2", "trueish", "A1"];
const SCOPE_ROOTS: [&str; 7] = ["a", "b", "row", "fold", "set", "event", "task"];
const KEYS: &[&str] = &[
    "id", "cmd", "data", "state", "n", "s", "d", "m", "items", "text", "and", "or", "not", "in", "true", "matches",
    "len", "_0",
];
const VALUE_KEYS: &[&str] = &[
    "id", "cmd", "data", "state", "n", "s", "d", "m", "items", "text", "and", "or", "not", "in", "true", "matches",
    "len", "_0", "content", "usage", "cost_usd", "a", "b", "0", "1", "2", "a/b", "~", "", "a b", "é",
];
const QROOTS: &[&str] = &["v", "n", "s", "t", "d", "m", "w", "data", "row", "id", "fold", "set", "zz"];

fn num(rng: &mut Rng, fine: bool) -> f64 {
    if fine {
        return *rng.pick(&[-3.0, -2.0, -1.5, -1.0, -0.5, 0.0, -0.0, 0.5, 1.0, 1.5, 2.0, 3.0]);
    }
    match rng.below(20) {
        0 => -0.0,
        1 => 0.0,
        2 => 1e15,
        3 => 9e15,
        4 => 1e300,
        5 => -1e300,
        6 => 4e-7,
        7 => 1e-7,
        8 => f64::MAX,
        9 => f64::MIN_POSITIVE,
        10 => 5e-324,
        11 => 123456.789,
        12..=15 => rng.between(-1000, 1000) as f64,
        16 => 0.1,
        17 => 2.5,
        18 => 8_999_999_999_999_999.0,
        _ => rng.next() as f64 / 1e3,
    }
}

fn dur(rng: &mut Rng, fine: bool) -> u64 {
    if fine {
        return rng.below(5) as u64 * 1000;
    }
    match rng.below(8) {
        0 => 0,
        1 => 999,
        2 => 1000,
        3 => u64::MAX,
        4 => u64::MAX - 1,
        5 => 86_400_000,
        6 => 59_999,
        _ => rng.next() >> rng.below(60),
    }
}

fn usd(rng: &mut Rng, fine: bool) -> i64 {
    if fine {
        return rng.between(-6, 6) * 500_000;
    }
    match rng.below(10) {
        0 => 0,
        1 => 1,
        2 => -1,
        3 => 49,
        4 => 5000,
        5 => -5000,
        6 => i64::MAX,
        7 => i64::MIN,
        8 => 1_500_000,
        _ => (rng.next() as i64) >> rng.below(60),
    }
}

fn string(rng: &mut Rng) -> String {
    const POOL: &[&str] = &[
        "",
        "a",
        "abc",
        "ABC",
        "Hello World",
        "rm -rf /",
        "git push origin main",
        "a\nb",
        "  padded  ",
        "\t",
        "é",
        "e\u{301}",
        "😀",
        "👨‍👩‍👧",
        "\u{202e}rtl",
        "\u{200b}",
        "\u{0}",
        "{x}",
        "'q'",
        "\"dq\"",
        "back\\slash",
        "日本語テキスト",
        "İ",
        "ΑΣ",
        "ß",
        "a\r\nb",
        "\u{2028}x",
        "42",
        "3.5",
        "running",
        "done",
        "plant",
        "text:with:colons",
        "x*y",
        "a?b",
        "tasks",
        "/a/b",
    ];
    match rng.below(10) {
        0 => "x".repeat(300),
        1 => (0..rng.below(12)).map(|_| rand_char(rng)).collect(),
        _ => (*rng.pick(POOL)).to_string(),
    }
}

/// A random value. `fine` keeps numbers, durations and money coarse, so that every comparison of two of them is
/// consistent (see `bug_query_order_is_not_a_total_order_for_numbers_against_money`).
fn value(rng: &mut Rng, depth: u32, fine: bool) -> Value {
    let nest = depth < 3;
    match rng.weighted(&[4, 7, 18, 16, 8, 8, if nest { 16 } else { 0 }, if nest { 20 } else { 0 }]) {
        0 => Value::Missing,
        1 => Value::Bool(rng.chance(50)),
        2 => Value::num(num(rng, fine)),
        3 => Value::Str(string(rng)),
        4 => Value::Dur(dur(rng, fine)),
        5 => Value::Usd(usd(rng, fine)),
        6 => {
            let n = rng.below(5);
            if rng.chance(40) {
                match rng.below(3) {
                    0 => Value::List((0..n).map(|_| Value::num(num(rng, fine))).collect()),
                    1 => Value::List((0..n).map(|_| Value::Dur(dur(rng, fine))).collect()),
                    _ => Value::List((0..n).map(|_| Value::Usd(usd(rng, fine))).collect()),
                }
            } else {
                Value::List((0..n).map(|_| value(rng, depth + 1, fine)).collect())
            }
        }
        _ => {
            let mut m = BTreeMap::new();
            for _ in 0..rng.below(6) {
                m.insert((*rng.pick(VALUE_KEYS)).to_string(), value(rng, depth + 1, fine));
            }
            Value::Map(m)
        }
    }
}

fn map(pairs: Vec<(&str, Value)>) -> Value {
    Value::Map(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn doc_value(rng: &mut Rng) -> Value {
    if rng.chance(50) {
        return value(rng, 0, false);
    }
    map(vec![
        ("content", Value::List(vec![map(vec![("text", Value::text("hello"))]), map(vec![("text", Value::Missing)])])),
        ("usage", map(vec![("cost_usd", Value::Usd(1_500_000))])),
        ("a", value(rng, 1, false)),
        ("b", Value::List(vec![Value::Num(1.0), Value::Num(2.0)])),
        ("a/b", Value::text("slash")),
        ("~", Value::text("tilde")),
    ])
}

fn danger_fn(cmd: &str) -> Value {
    if cmd.contains("rm") {
        Value::text("rm")
    } else if cmd.is_empty() {
        Value::Missing
    } else {
        Value::Bool(false)
    }
}

fn random_scope(rng: &mut Rng) -> Scope {
    let mut s = Scope::new();
    for root in SCOPE_ROOTS {
        if rng.chance(85) {
            s = s.with(root, value(rng, 0, false));
        }
    }
    s = s.doc(doc_value(rng));
    if rng.chance(70) {
        s = s.at_ms(*rng.pick(&[0, 1, 1_700_000_000_000, u64::MAX, u64::MAX / 2, 5000]));
    }
    if rng.chance(50) {
        s = s.danger_with(danger_fn);
    }
    s
}

// ── the generator of valid expression text ───────────────────────────────────────────────────

struct Gen {
    rng: Rng,
    used: usize,
    cap: usize,
    /// May a pointer hold a `:` (not inside a template hole: the last `:` there starts the formatter).
    colon_ok: bool,
    /// Every name a path may start with, and the ones the scope really defines (picked more often).
    roots: &'static [&'static str],
    known: &'static [&'static str],
}

impl Gen {
    fn new(seed: u64) -> Gen {
        Gen { rng: Rng::new(seed), used: 0, cap: 330, colon_ok: true, roots: ROOTS, known: &SCOPE_ROOTS }
    }

    /// Start a new expression: its size budget is drawn so that short ones and long ones are both common.
    fn fresh(&mut self) {
        self.used = 0;
        self.cap = *self.rng.pick(&[0, 0, 25, 60, 120, 330]);
    }

    /// Whitespace that may be left out.
    fn ws(&mut self) -> &'static str {
        self.rng.pick(&["", "", "", " ", " ", "  ", "\t", "\n", "\u{3000}", "\u{a0}", "\r\n"])
    }

    /// Whitespace that has to be there.
    fn sp(&mut self) -> &'static str {
        self.rng.pick(&[" ", " ", " ", "  ", "\t", "\n", "\u{3000}", "\u{2028}", "\u{85}"])
    }

    fn expr(&mut self, depth: u32) -> String {
        let n = if self.rng.chance(75) { 1 } else { 2 + self.rng.below(2) };
        let mut out = String::new();
        for i in 0..n {
            if i > 0 {
                let (a, b) = (self.sp(), self.sp());
                out += a;
                out += "or";
                out += b;
            }
            out += &self.and(depth);
        }
        out
    }

    fn and(&mut self, depth: u32) -> String {
        let n = if self.rng.chance(75) { 1 } else { 2 + self.rng.below(2) };
        let mut out = String::new();
        for i in 0..n {
            if i > 0 {
                let (a, b) = (self.sp(), self.sp());
                out += a;
                out += "and";
                out += b;
            }
            out += &self.not(depth);
        }
        out
    }

    fn not(&mut self, depth: u32) -> String {
        let most = if depth == 0 { 2 } else { 1 };
        let mut out = String::new();
        let mut k = 0;
        while k < most && self.rng.chance(18) {
            out += "not";
            out += self.sp();
            k += 1;
        }
        out + &self.cmp(depth)
    }

    fn cmp(&mut self, depth: u32) -> String {
        let lhs = self.primary(depth);
        if !self.rng.chance(40) {
            return lhs;
        }
        let (op, word) = *self.rng.pick(&[
            ("==", false),
            ("!=", false),
            ("<", false),
            ("<=", false),
            (">", false),
            (">=", false),
            ("in", true),
            ("not in", true),
            ("matches", true),
        ]);
        let op = if op == "not in" { format!("not{}in", self.sp()) } else { op.to_string() };
        let rhs = self.primary(depth);
        let (a, b) = if word { (self.sp(), self.sp()) } else { (self.ws(), self.ws()) };
        format!("{lhs}{a}{op}{b}{rhs}")
    }

    fn primary(&mut self, depth: u32) -> String {
        let nest = depth < 4 && self.used < self.cap;
        let n = |k: u64| if nest { k } else { 0 };
        match self.rng.weighted(&[28, 30, 8, n(14), n(8), n(8)]) {
            0 => self.literal(),
            1 => self.path(depth),
            2 => self.pointer(depth),
            3 => self.call(depth),
            4 => self.list(depth),
            _ => {
                let (a, b) = (self.ws(), self.ws());
                let inner = self.expr(depth + 1);
                format!("({a}{inner}{b})")
            }
        }
    }

    fn literal(&mut self) -> String {
        let s = match self.rng.below(9) {
            0 | 1 | 8 => self.number(),
            2 => self.duration(),
            3 => self.money(),
            4 | 5 => self.string_lit(),
            6 => "true".into(),
            _ => "false".into(),
        };
        self.used += s.len();
        s
    }

    fn number(&mut self) -> String {
        match self.rng.below(12) {
            0 => "0".into(),
            1 => "-0".into(),
            2 => format!("{}", self.rng.below(1000)),
            3 => format!("-{}", 1 + self.rng.below(1000)),
            4 => format!("{}.{}", self.rng.below(100), self.rng.below(1000)),
            5 => format!("-{}.{:03}", self.rng.below(100), self.rng.below(1000)),
            6 => "0.5".into(),
            7 => format!("{}", self.rng.next() % 1_000_000_007),
            8 => "9000000000000000".into(),
            9 => format!("{}", self.rng.next()),
            10 => format!("0.{}7", "0".repeat(self.rng.below(12))),
            _ => "12345678901234567890123456789".into(),
        }
    }

    fn duration(&mut self) -> String {
        let mut out = String::new();
        for _ in 0..1 + self.rng.below(3) {
            let amount = match self.rng.below(4) {
                0 => format!("{}", self.rng.below(100)),
                1 => format!("{}.{}", self.rng.below(10), self.rng.below(100)),
                2 => "0".into(),
                _ => format!("{}", self.rng.below(100_000)),
            };
            out += &amount;
            out += self.rng.pick(&["ms", "s", "m", "h", "d"]);
        }
        out
    }

    /// Amounts stay below a billion dollars: f64 still holds every millionth there (see the money bug test).
    fn money(&mut self) -> String {
        match self.rng.below(8) {
            0 => "$0".into(),
            1 => "$1.50".into(),
            2 => "$0.0004".into(),
            3 => format!("${}", self.rng.below(100_000)),
            4 => format!("${}.{:06}", self.rng.below(1000), self.rng.below(1_000_000)),
            5 => format!("${}.{}", self.rng.below(100), self.rng.below(100)),
            6 => "$0.5".into(),
            _ => format!("${}.{:06}", self.rng.next() % 1_000_000_000, self.rng.below(1_000_000)),
        }
    }

    fn string_lit(&mut self) -> String {
        let q = if self.rng.chance(50) { '\'' } else { '"' };
        let mut s = String::from(q);
        for _ in 0..self.rng.below(8) {
            match self.rng.below(14) {
                0 => s += "\\\\",
                1 => s += "\\'",
                2 => s += "\\\"",
                3 => s += "\\n",
                4 => s += "\\t",
                5 => s += "\\r",
                6 => s.push(if q == '\'' { '"' } else { '\'' }),
                7 => {
                    s += self.rng.pick(&[
                        "{", "}", ":", "{{", "}}", ",", " ", "/", "$", "-", "é", "e\u{301}", "😀", "\u{200b}",
                        "\u{202e}", "\u{0}", "\n", "\t", "\u{2028}", "日本",
                    ])
                }
                _ => s.push(*self.rng.pick(&['a', 'b', 'c', 'x', 'y', 'Z', '0', '1', ' ', '_', '*', '?', '.'])),
            }
        }
        s.push(q);
        s
    }

    fn path(&mut self, depth: u32) -> String {
        let root = if self.rng.chance(75) { *self.rng.pick(self.known) } else { *self.rng.pick(self.roots) };
        self.used += root.len();
        let steps = self.steps(depth, true);
        format!("{root}{steps}")
    }

    /// `.key` and `[expr]` steps; a pointer takes a `[expr]` first (a `.` would be part of the pointer).
    fn steps(&mut self, depth: u32, mut dot_ok: bool) -> String {
        let mut out = String::new();
        for _ in 0..self.rng.below(4) {
            if dot_ok && self.rng.chance(55) {
                out.push('.');
                out += self.rng.pick(KEYS);
                self.used += 4;
            } else if depth < 5 && self.used < self.cap {
                let (a, b) = (self.ws(), self.ws());
                let inner = if self.rng.chance(60) {
                    match self.rng.below(3) {
                        0 => format!("{}", self.rng.below(4)),
                        1 => self.string_lit(),
                        _ => self.path(depth + 1),
                    }
                } else {
                    self.expr(depth + 1)
                };
                out += &format!("[{a}{inner}{b}]");
                dot_ok = true;
            } else {
                break;
            }
        }
        out
    }

    fn pointer(&mut self, depth: u32) -> String {
        let mut base = match self.rng.below(10) {
            0 => "/".to_string(),
            1 => "/a".into(),
            2 => "/content/0/text".into(),
            3 => "/usage/cost_usd".into(),
            4 => "//".into(),
            5 => "/a~1b/~0".into(),
            _ => {
                let mut p = String::new();
                for _ in 0..1 + self.rng.below(3) {
                    p.push('/');
                    for _ in 0..self.rng.below(5) {
                        let alphabet: &[char] = if self.colon_ok {
                            &[
                                'a', 'b', 'c', 'x', 'z', '0', '1', '8', '9', '_', '-', '.', ':', '@', '$', '%', '+',
                                '~', 'Q',
                            ]
                        } else {
                            &['a', 'b', 'c', 'x', 'z', '0', '1', '8', '9', '_', '-', '.', '@', '$', '%', '+', '~', 'Q']
                        };
                        p.push(*self.rng.pick(alphabet));
                    }
                }
                p
            }
        };
        self.used += base.len();
        base += &self.steps(depth, false);
        base
    }

    fn call(&mut self, depth: u32) -> String {
        let func = *self.rng.pick(&Func::ALL);
        let (lo, hi) = func.arity();
        let n = lo + self.rng.below(hi - lo + 1);
        let mut args = Vec::new();
        for _ in 0..n {
            let (a, b) = (self.ws(), self.ws());
            let e = self.expr(depth + 1);
            args.push(format!("{a}{e}{b}"));
        }
        let gap = self.ws();
        self.used += 6;
        let steps = self.steps(depth, true);
        format!("{}{gap}({}){steps}", func.word(), args.join(","))
    }

    fn list(&mut self, depth: u32) -> String {
        let mut items = Vec::new();
        for _ in 0..self.rng.below(4) {
            let (a, b) = (self.ws(), self.ws());
            let e = self.expr(depth + 1);
            items.push(format!("{a}{e}{b}"));
        }
        let mut body = items.join(",");
        if !items.is_empty() && self.rng.chance(15) {
            body.push(',');
        }
        let (a, b) = (self.ws(), self.ws());
        format!("[{a}{body}{b}]")
    }
}

// ── 1. token soup ────────────────────────────────────────────────────────────────────────────

fn roundtrip_expr(sink: &mut Sink, input: &str, expr: &Expr) {
    let shown = expr.to_string();
    match Expr::parse(&shown) {
        Ok(back) if back == *expr => {}
        Ok(back) => sink.fail(format!("Display changed the tree: {input:?} -> {shown:?} -> {back:?} (was {expr:?})")),
        Err(e) => sink.fail(format!("Display of {input:?} is {shown:?}, which does not parse: {e}")),
    }
}

fn roundtrip_fmt(sink: &mut Sink, input: &str, f: &Fmt) {
    let shown = f.to_string();
    match Fmt::parse(&shown) {
        Ok(back) if back == *f => {}
        Ok(back) => sink.fail(format!("Display changed the template: {input:?} -> {shown:?} -> {back:?} (was {f:?})")),
        Err(e) => sink.fail(format!("Display of the template {input:?} is {shown:?}, which does not parse: {e}")),
    }
}

/// The `(character N)` a Query message ends with, N-1 being a position inside the text it came from.
fn query_positions(sink: &mut Sink, msgs: &[String], filter: Option<&str>, order: Option<&str>) {
    for m in msgs {
        let Some(k) = m.rfind("(character ") else { continue };
        let digits: String = m[k + 11..].chars().take_while(char::is_ascii_digit).collect();
        let src = if m.starts_with("filter: ") {
            filter
        } else if m.starts_with("order: ") {
            order
        } else {
            continue;
        };
        let len = src.map_or(0, chars);
        match digits.parse::<usize>() {
            Ok(n) if n >= 1 && n - 1 <= len => {}
            _ => sink
                .fail(format!("position outside the input ({len} chars) in {m:?}; filter {filter:?}, order {order:?}")),
        }
    }
}

#[test]
fn token_soup_never_panics_and_error_positions_stay_inside_the_input() {
    let mut rng = Rng::new(0x5EED_0001);
    let mut sink = Sink::new("token soup (Expr, Fmt, Coll)");
    let scope = random_scope(&mut rng);
    let (mut ok_expr, mut ok_fmt, mut ok_coll) = (0usize, 0usize, 0usize);
    for _ in 0..N_SOUP {
        if sink.done() {
            break;
        }
        let input = soup(&mut rng, 24);
        if let Some(r) = guarded(&mut sink, "Expr::parse", &input, || Expr::parse(&input)) {
            match r {
                Err(e) if !pos_ok(&e, &input) => sink.fail(format!(
                    "Expr error at {} past the end ({} chars) of {input:?}: {e}",
                    e.at,
                    chars(&input)
                )),
                Err(e) => {
                    let _ = e.to_string();
                }
                Ok(expr) => {
                    ok_expr += 1;
                    roundtrip_expr(&mut sink, &input, &expr);
                    guarded(&mut sink, "Expr::eval", &input, || expr.eval(&scope));
                    guarded(&mut sink, "Expr::check", &input, || (expr.check(None), expr.check(Some(&["a", "row"]))));
                }
            }
        }
        if let Some(r) = guarded(&mut sink, "Fmt::parse", &input, || Fmt::parse(&input)) {
            match r {
                Err(e) if !pos_ok(&e, &input) => {
                    sink.fail(format!("Fmt error at {} past the end ({} chars) of {input:?}: {e}", e.at, chars(&input)))
                }
                Err(_) => {}
                Ok(f) => {
                    ok_fmt += 1;
                    roundtrip_fmt(&mut sink, &input, &f);
                    guarded(&mut sink, "Fmt::render", &input, || f.render(&scope));
                    guarded(&mut sink, "Fmt::check", &input, || f.check(None));
                }
            }
        }
        if let Some(Ok(c)) = guarded(&mut sink, "Coll::parse", &input, || Coll::parse(&input)) {
            ok_coll += 1;
            if Coll::parse(&c.to_string()) != Ok(c.clone()) {
                sink.fail(format!("Coll Display does not round trip: {input:?} -> {c} "));
            }
        }
    }
    assert!(
        ok_expr > 300 && ok_fmt > 300 && ok_coll > 5,
        "the soup is too hostile to test anything: {ok_expr} {ok_fmt} {ok_coll}"
    );
    sink.finish(N_SOUP);
}

#[test]
fn token_soup_through_query_parse_never_panics() {
    let mut rng = Rng::new(0x5EED_0002);
    let mut sink = Sink::new("token soup (Query::parse)");
    let scope = random_scope(&mut rng);
    let mut ok = 0usize;
    for _ in 0..N_SOUP_QUERY {
        if sink.done() {
            break;
        }
        let from = match rng.below(4) {
            0 => soup(&mut rng, 6),
            1 => format!(
                "{}{}",
                rng.pick(&["tasks", "asks", "changes", "log:", "table:", "fold:", "thread:", "log:a*", "table:x"]),
                soup(&mut rng, 2)
            ),
            2 => (*rng.pick(&["tasks", "asks", "changes", "log:*", "table:t", "fold:f", "thread:th"])).to_string(),
            _ => String::new(),
        };
        let filter = if rng.chance(70) { Some(soup(&mut rng, 16)) } else { None };
        let order = if rng.chance(70) { Some(soup(&mut rng, 16)) } else { None };
        let limit = match rng.below(6) {
            0 => None,
            1 => Some(0),
            2 => Some(1),
            3 => Some(MAX_LIMIT),
            4 => Some(MAX_LIMIT + 1),
            _ => Some(rng.next()),
        };
        let label = format!("{from:?} {filter:?} {order:?} {limit:?}");
        let Some(r) = guarded(&mut sink, "Query::parse", &label, || {
            Query::parse(&from, filter.as_deref(), order.as_deref(), limit)
        }) else {
            continue;
        };
        match r {
            Err(msgs) => {
                if msgs.is_empty() {
                    sink.fail(format!("Err with no message: {label}"));
                }
                query_positions(&mut sink, &msgs, filter.as_deref(), order.as_deref());
            }
            Ok(q) => {
                ok += 1;
                if q.limit == Some(0) || q.limit.is_some_and(|n| n as u64 > MAX_LIMIT) || q.order.len() > MAX_ORDER {
                    sink.fail(format!("Query::parse let through a limit or an order out of range: {label} -> {q:?}"));
                }
                let rows: Vec<Value> = (0..5).map(|_| value(&mut rng, 1, false)).collect();
                guarded(&mut sink, "Query::apply", &label, || q.apply(rows, &scope));
                guarded(&mut sink, "Query::check", &label, || q.check(None));
            }
        }
    }
    assert!(ok > 20, "no soup query parsed: {ok}");
    sink.finish(N_SOUP_QUERY);
}

#[test]
fn mutated_valid_text_never_panics_and_errors_stay_inside_the_input() {
    let mut g = Gen::new(0x5EED_0003);
    let mut sink = Sink::new("mutated valid text");
    let scope = random_scope(&mut g.rng);
    let (mut ok, mut err) = (0usize, 0usize);
    for _ in 0..N_MUTATED {
        if sink.done() {
            break;
        }
        g.fresh();
        let valid = g.expr(0);
        let bad = mutate(&mut g.rng, &valid);
        // the same text as an expression, as a hole of a template, as a template, as a filter and as an order key
        let hole = format!("{{{bad}}}");
        let colon = format!("{{{bad}:usd}}");
        for (what, input) in [("expression", &bad), ("hole", &hole), ("hole with formatter", &colon)] {
            let r = guarded(&mut sink, what, input, || (Expr::parse(input), Fmt::parse(input)));
            let Some((e, f)) = r else { continue };
            match e {
                Ok(expr) => {
                    ok += 1;
                    roundtrip_expr(&mut sink, input, &expr);
                    guarded(&mut sink, "eval", input, || expr.eval(&scope));
                }
                Err(e) => {
                    err += 1;
                    if !pos_ok(&e, input) {
                        sink.fail(format!(
                            "Expr error at {} past the end ({} chars) of {input:?}: {e}",
                            e.at,
                            chars(input)
                        ));
                    }
                }
            }
            match f {
                Ok(f) => {
                    roundtrip_fmt(&mut sink, input, &f);
                    guarded(&mut sink, "render", input, || f.render(&scope));
                }
                Err(e) if !pos_ok(&e, input) => {
                    sink.fail(format!("Fmt error at {} past the end ({} chars) of {input:?}: {e}", e.at, chars(input)))
                }
                Err(_) => {}
            }
        }
        let label = format!("filter and order {bad:?}");
        if let Some(Err(msgs)) =
            guarded(&mut sink, "Query::parse", &label, || Query::parse("tasks", Some(&bad), Some(&bad), None))
        {
            query_positions(&mut sink, &msgs, Some(&bad), Some(&bad));
        }
    }
    assert!(ok > 1000 && err > 1000, "mutations should leave both valid and broken text: {ok} {err}");
    sink.finish(N_MUTATED);
}

// ── 2. valid expressions: round trip, totality, determinism ─────────────────────────────────

#[test]
fn valid_expressions_round_trip_and_evaluate_totally() {
    let mut g = Gen::new(0x5EED_0004);
    let mut sink = Sink::new("valid expressions");
    let (mut yes, mut present, mut longest) = (0usize, 0usize, 0usize);
    for i in 0..N_VALID {
        if sink.done() {
            break;
        }
        g.fresh();
        let text = g.expr(0);
        longest = longest.max(chars(&text));
        let scope = random_scope(&mut g.rng);
        let parsed = match Expr::parse(&text) {
            Ok(e) => e,
            Err(e) => {
                sink.fail(format!(
                    "[{i}] the generator wrote text the parser refuses (a test mistake?): {text:?}: {e}"
                ));
                continue;
            }
        };
        // parse(display(parse(text))) == parse(text), and display is stable from then on
        let shown = parsed.to_string();
        let again = match Expr::parse(&shown) {
            Ok(b) => b,
            Err(e) => {
                sink.fail(format!("[{i}] Display of {text:?} is {shown:?}, which does not parse: {e}"));
                continue;
            }
        };
        if again != parsed {
            sink.fail(format!(
                "[{i}] parse(display(parse(t))) != parse(t) for {text:?}: {shown:?} -> {again:?} vs {parsed:?}"
            ));
            continue;
        }
        if again.to_string() != shown {
            sink.fail(format!("[{i}] Display is not stable: {text:?}: {shown:?} then {:?}", again.to_string()));
        }
        if text.parse::<Expr>().as_ref() != Ok(&parsed) {
            sink.fail(format!("[{i}] FromStr differs from parse for {text:?}"));
        }
        // evaluation: total, deterministic, holds == truthy, the same on the round-tripped tree
        let first = guarded(&mut sink, "Expr::eval", &text, || parsed.eval(&scope));
        let second = guarded(&mut sink, "Expr::eval (2nd)", &text, || parsed.eval(&scope));
        let held = guarded(&mut sink, "Expr::holds", &text, || parsed.holds(&scope));
        let (Some(first), Some(second), Some(held)) = (first, second, held) else {
            sink.fail(format!("[{i}] scope: {:.1500}", format!("{scope:?}")));
            continue;
        };
        if first != second {
            sink.fail(format!("[{i}] eval is not deterministic for {text:?}: {first:?} then {second:?}"));
        }
        if held != first.truthy() {
            sink.fail(format!("[{i}] holds() != eval().truthy() for {text:?}: {held} vs {first:?}"));
        }
        let back = again.eval(&scope);
        if back != first {
            sink.fail(format!(
                "[{i}] the round-tripped tree evaluates differently for {text:?}: {first:?} vs {back:?}"
            ));
        }
        guarded(&mut sink, "Expr::check", &text, || {
            (parsed.check(None), parsed.check(Some(&[])), parsed.check(Some(&["a", "b", "row"])))
        });
        yes += usize::from(held);
        present += usize::from(first != Value::Missing);
    }
    // the generator must reach real values, not only nothing
    assert!(
        present * 4 > N_VALID && yes * 10 > N_VALID,
        "the generator is too weak: {present} present, {yes} yes of {N_VALID}"
    );
    assert!(longest > 150, "texts are too short: {longest}");
    sink.finish(N_VALID);
}

// ── 2a. the generator itself ─────────────────────────────────────────────────────────────────

fn walk(e: &Expr, f: &mut impl FnMut(&Expr)) {
    f(e);
    let segs = |segs: &[j4n_core::expr::Seg], f: &mut dyn FnMut(&Expr)| {
        for s in segs {
            if let j4n_core::expr::Seg::Index(i) = s {
                f(i);
            }
        }
    };
    match e {
        Expr::Lit(_) | Expr::Pointer(_) => {}
        Expr::List(items) | Expr::And(items) | Expr::Or(items) => items.iter().for_each(|i| walk(i, f)),
        Expr::Call { args, .. } => args.iter().for_each(|i| walk(i, f)),
        Expr::Not(i) => walk(i, f),
        Expr::Cmp { lhs, rhs, .. } => {
            walk(lhs, f);
            walk(rhs, f);
        }
        Expr::Path { segs: s, .. } => {
            let mut inner = Vec::new();
            segs(s, &mut |i| inner.push(i.clone()));
            inner.iter().for_each(|i| walk(i, f));
        }
        Expr::Get { base, segs: s } => {
            walk(base, f);
            let mut inner = Vec::new();
            segs(s, &mut |i| inner.push(i.clone()));
            inner.iter().for_each(|i| walk(i, f));
        }
    }
}

/// A fuzzer that never writes `not in`, a pointer with steps or `since` proves nothing about them.
#[test]
fn the_generator_reaches_every_construct_of_the_language() {
    use std::collections::BTreeMap as Counts;
    let mut g = Gen::new(0x5EED_00F0);
    let mut seen: Counts<String, usize> = Counts::new();
    let mut bump = |k: String| *seen.entry(k).or_insert(0) += 1;
    let mut max_depth = 0usize;
    for _ in 0..4000 {
        g.fresh();
        let text = g.expr(0);
        let e = Expr::parse(&text).expect("the generator writes valid text");
        walk(&e, &mut |n| match n {
            Expr::Lit(v) => bump(format!("lit {}", v.kind())),
            Expr::List(_) => bump("list".into()),
            Expr::Path { segs, .. } => bump(format!("path{}", if segs.is_empty() { "" } else { " with steps" })),
            Expr::Pointer(p) => bump(format!("pointer{}", if p.contains(':') { " with colon" } else { "" })),
            Expr::Get { base, .. } => {
                bump(format!("get on {}", if matches!(**base, Expr::Pointer(_)) { "pointer" } else { "call" }))
            }
            Expr::Call { func, .. } => bump(format!("call {}", func.word())),
            Expr::Not(i) => bump(if matches!(**i, Expr::Cmp { op: j4n_core::expr::Op::In, .. }) {
                "not in".into()
            } else {
                "not".into()
            }),
            Expr::And(_) => bump("and".into()),
            Expr::Or(_) => bump("or".into()),
            Expr::Cmp { op, .. } => bump(format!("cmp {}", op.word())),
        });
        max_depth = max_depth.max(text.matches(['(', '[']).count());
    }
    let must = [
        "lit nothing",
        "lit a bool",
        "lit a number",
        "lit text",
        "lit a duration",
        "lit money",
        "list",
        "path",
        "path with steps",
        "pointer",
        "pointer with colon",
        "get on pointer",
        "get on call",
        "not",
        "not in",
        "and",
        "or",
        "cmp ==",
        "cmp !=",
        "cmp <",
        "cmp <=",
        "cmp >",
        "cmp >=",
        "cmp in",
        "cmp matches",
        "call count",
        "call same",
        "call last",
        "call since",
        "call sum",
        "call len",
        "call lower",
        "call contains",
        "call danger",
        "call quote",
    ];
    let missing: Vec<&str> =
        must.iter().copied().filter(|k| seen.get(*k).copied().unwrap_or(0) < 50 && *k != "lit nothing").collect();
    assert!(missing.is_empty(), "the generator hardly writes {missing:?}; saw {seen:?}");
    assert!(max_depth >= 6, "the generator never nests: {max_depth}");
}

// ── 2b. algebra: what must hold whatever the values are ──────────────────────────────────────

fn truth(text: &str, scope: &Scope) -> Result<bool, String> {
    Expr::parse(text).map(|e| e.holds(scope)).map_err(|e| format!("{text:?} does not parse: {e}"))
}

fn val(text: &str, scope: &Scope) -> Result<Value, String> {
    Expr::parse(text).map(|e| e.eval(scope)).map_err(|e| format!("{text:?} does not parse: {e}"))
}

fn ensure(ok: bool, what: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(what.to_string()) }
}

/// A `quote` result: one line in double quotes, `\` and `"` escaped, nothing a terminal would act on.
fn quote_shape(s: &str) -> Result<(), String> {
    ensure(s.len() >= 2 && s.starts_with('"') && s.ends_with('"'), "quote: not in double quotes")?;
    let inner = &s[1..s.len() - 1];
    let mut it = inner.chars();
    while let Some(c) = it.next() {
        match c {
            '\\' => ensure(matches!(it.next(), Some('\\' | '"')), "quote: a backslash that escapes nothing")?,
            '"' => return Err("quote: an unescaped double quote".into()),
            c if c.is_control() => return Err(format!("quote: a control character {c:?}")),
            _ => {}
        }
    }
    ensure(chars(inner) <= 2 * QUOTE_MAX + 1, "quote: longer than QUOTE_MAX")
}

fn algebra_case(l: &str, r: &str, t: &str, scope: &Scope) -> Result<(), String> {
    let h = |s: &str| truth(s, scope);
    let ht = h(t)?;
    ensure(h(&format!("not ({t})"))? == !ht, "not (T) is the opposite of T")?;
    ensure(h(&format!("({t}) and ({t})"))? == ht, "T and T is T")?;
    ensure(h(&format!("({t}) or false"))? == ht, "T or false is T")?;
    ensure(h(&format!("({t}) or not ({t})"))?, "T or not T")?;
    ensure(!h(&format!("({t}) and not ({t})"))?, "T and not T")?;
    ensure(h(&format!("not not ({t})"))? == ht, "not not T is T")?;
    // comparisons
    let (lv, rv) = (val(l, scope)?, val(r, scope)?);
    let both = lv != Value::Missing && rv != Value::Missing;
    let eq = h(&format!("({l}) == ({r})"))?;
    let ne = h(&format!("({l}) != ({r})"))?;
    let lt = h(&format!("({l}) < ({r})"))?;
    let le = h(&format!("({l}) <= ({r})"))?;
    let gt = h(&format!("({l}) > ({r})"))?;
    let ge = h(&format!("({l}) >= ({r})"))?;
    if both {
        ensure(eq != ne, "== and != are opposite when both sides are there")?;
    } else {
        ensure(!(eq || ne || lt || le || gt || ge), "nothing compares with nothing")?;
    }
    ensure(lt == h(&format!("({r}) > ({l})"))?, "a < b is b > a")?;
    ensure(le == h(&format!("({r}) >= ({l})"))?, "a <= b is b >= a")?;
    ensure(eq == h(&format!("({r}) == ({l})"))?, "== is symmetric")?;
    ensure(ne == h(&format!("({r}) != ({l})"))?, "!= is symmetric")?;
    ensure(!(lt && gt) && !(lt && eq) && !(gt && eq), "an order is one of <, ==, >")?;
    ensure(!lt || (le && ne), "< implies <= and !=")?;
    ensure(!gt || (ge && ne), "> implies >= and !=")?;
    ensure(!le || !gt, "<= excludes >")?;
    // membership
    let within = h(&format!("({l}) in ({r})"))?;
    ensure(h(&format!("contains(({r}), ({l}))"))? == within, "contains(hay, x) is x in hay")?;
    ensure(h(&format!("({l}) not in ({r})"))? == !within, "not in is not (in)")?;
    ensure(h(&format!("({l}) in [({r})]"))? == eq, "x in [y] is x == y")?;
    ensure(h(&format!("same([({l}), ({r})])"))? == eq, "same([x, y]) is x == y")?;
    ensure(val(&format!("len([({l}), ({r})])"), scope)? == Value::Num(2.0), "len of a two-item list")?;
    // text
    let lower = val(&format!("lower(({t}))"), scope)?;
    ensure(val(&format!("lower(lower(({t})))"), scope)? == lower, "lower is idempotent")?;
    if let Value::Str(s) = val(&format!("quote(({t}))"), scope)? {
        quote_shape(&s)?;
    }
    Ok(())
}

#[test]
fn expression_algebra_holds_whatever_the_values_are() {
    let mut g = Gen::new(0x5EED_0005);
    let mut sink = Sink::new("expression algebra");
    for i in 0..N_ALGEBRA {
        if sink.done() {
            break;
        }
        g.fresh();
        let (l, r, t) = (g.cmp(2), g.cmp(2), g.expr(1));
        let scope = random_scope(&mut g.rng);
        let label = format!("L={l:?} R={r:?} T={t:?}");
        match caught(|| algebra_case(&l, &r, &t, &scope)) {
            Ok(Ok(())) => {}
            Ok(Err(why)) => sink.fail(format!("[{i}] {why}: {label}; scope {:.800}", format!("{scope:?}"))),
            Err(p) => sink.fail(format!("[{i}] PANIC {p}: {label}")),
        }
    }
    sink.finish(N_ALGEBRA);
}

// ── 2c. scalar semantics against a model written from the documentation ─────────────────────

/// A literal the way the documentation talks about it. Numbers are halves, durations milliseconds, money millionths.
#[derive(Clone, Debug, PartialEq)]
enum M {
    B(bool),
    N(i64),
    S(String),
    D(u64),
    U(i64),
}

fn m_value(m: &M) -> Value {
    match m {
        M::B(b) => Value::Bool(*b),
        M::N(h) => Value::Num(*h as f64 / 2.0),
        M::S(s) => Value::Str(s.clone()),
        M::D(d) => Value::Dur(*d),
        M::U(u) => Value::Usd(*u),
    }
}

fn m_text(rng: &mut Rng, m: &M) -> String {
    match m {
        M::B(b) => b.to_string(),
        M::N(h) => {
            let (sign, a) = (if *h < 0 || (*h == 0 && rng.chance(20)) { "-" } else { "" }, h.unsigned_abs());
            if a % 2 == 0 { format!("{sign}{}", a / 2) } else { format!("{sign}{}.5", a / 2) }
        }
        M::S(s) => {
            let q = if rng.chance(50) { '\'' } else { '"' };
            let mut out = String::from(q);
            for c in s.chars() {
                match c {
                    '\\' => out += "\\\\",
                    '\n' => out += "\\n",
                    c if c == q => {
                        out.push('\\');
                        out.push(c);
                    }
                    c => out.push(c),
                }
            }
            out.push(q);
            out
        }
        M::D(ms) if ms % 1000 == 0 && rng.chance(50) => format!("{}s", ms / 1000),
        M::D(ms) => format!("{ms}ms"),
        M::U(u) => format!("${}.{:06}", u / 1_000_000, u % 1_000_000),
    }
}

fn m_random(rng: &mut Rng) -> M {
    match rng.below(10) {
        0 | 1 => M::B(rng.chance(50)),
        2..=4 => M::N(rng.between(-6, 6)),
        5 | 6 => M::S(
            (*rng.pick(&[
                "", "a", "b", "ab", "ba", "A", "é", "e\u{301}", "a'b", "a\"b", "x\ny", "a\\b", "😀", "ab*", "a?",
            ]))
            .to_string(),
        ),
        7 => M::D(*rng.pick(&[0, 1, 999, 1000, 1001, 60_000, 90_000, 3_600_000, 86_400_000, 1_000_000_000_000_000])),
        _ => {
            M::U(*rng.pick(&[
                0, 1, 7, 500_000, 999_999, 1_000_000, 1_500_000, 3_000_000, -0, 2_000_000, 4_000_000, 3_500_000,
            ]))
        }
    }
}

fn m_cmp(a: &M, b: &M) -> Option<Ordering> {
    match (a, b) {
        (M::N(x), M::N(y)) => Some(x.cmp(y)),
        (M::S(x), M::S(y)) => Some(x.chars().cmp(y.chars())),
        (M::D(x), M::D(y)) => Some(x.cmp(y)),
        (M::U(x), M::U(y)) => Some(x.cmp(y)),
        (M::N(h), M::U(u)) => Some((h * 500_000).cmp(u)),
        (M::U(u), M::N(h)) => Some(u.cmp(&(h * 500_000))),
        _ => None,
    }
}

fn m_eq(a: &M, b: &M) -> bool {
    match (a, b) {
        (M::B(x), M::B(y)) => x == y,
        _ => m_cmp(a, b) == Some(Ordering::Equal),
    }
}

fn m_glob(p: &str, t: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (p.chars().collect(), t.chars().collect());
    glob_reference(&p, &t)
}

#[test]
fn scalar_semantics_match_a_model_written_from_the_documentation() {
    let mut rng = Rng::new(0x5EED_00A0);
    let scope = Scope::new();
    let mut sink = Sink::new("scalar semantics");
    for i in 0..N_ALGEBRA {
        if sink.done() {
            break;
        }
        let (a, b, c) = (m_random(&mut rng), m_random(&mut rng), m_random(&mut rng));
        let (ta, tb, tc) = (m_text(&mut rng, &a), m_text(&mut rng, &b), m_text(&mut rng, &c));
        let ev = |text: &str| Expr::parse(text).map(|e| e.eval(&scope)).map_err(|e| format!("{text:?}: {e}"));
        let mut check = |text: String, want: Value| match ev(&text) {
            Ok(got) if got == want => {}
            Ok(got) => sink.fail(format!("[{i}] {text:?} is {got:?}, the documentation says {want:?}")),
            Err(e) => sink.fail(format!("[{i}] {e}")),
        };
        let ord = m_cmp(&a, &b);
        let yes = |ok: bool| Value::Bool(ok);
        check(format!("{ta} == {tb}"), yes(m_eq(&a, &b)));
        check(format!("{ta} != {tb}"), yes(!m_eq(&a, &b)));
        check(format!("{ta} < {tb}"), yes(ord == Some(Ordering::Less)));
        check(format!("{ta} <= {tb}"), yes(matches!(ord, Some(Ordering::Less | Ordering::Equal))));
        check(format!("{ta} > {tb}"), yes(ord == Some(Ordering::Greater)));
        check(format!("{ta} >= {tb}"), yes(matches!(ord, Some(Ordering::Greater | Ordering::Equal))));
        // membership
        let in_list = [&b, &c].into_iter().any(|x| m_eq(&a, x));
        check(format!("{ta} in [{tb}, {tc}]"), yes(in_list));
        check(format!("{ta} not in [{tb}, {tc}]"), yes(!in_list));
        check(format!("contains([{tb}, {tc}], {ta})"), yes(in_list));
        if let (M::S(x), M::S(y)) = (&a, &b) {
            check(format!("{ta} in {tb}"), yes(y.contains(x.as_str())));
            check(format!("{tb} matches {ta}"), yes(m_glob(x, y)));
            check(format!("lower({ta})"), Value::Str(x.to_lowercase()));
            check(format!("len({ta})"), Value::Num(x.chars().count() as f64));
        } else {
            check(format!("{tb} matches {ta}"), yes(false));
        }
        // lists: items that are missing (`zz.q`) are skipped by `count` and `sum`, never equal to anything
        let items: Vec<Option<&M>> =
            vec![Some(&a), None, Some(&b), Some(&c)].into_iter().filter(|_| rng.chance(80)).collect();
        let list = format!(
            "[{}]",
            items
                .iter()
                .zip([&ta, &tb, &tc, &ta])
                .map(|(it, t)| if it.is_some() { t.clone() } else { "zz.q".to_string() })
                .collect::<Vec<_>>()
                .join(", ")
        );
        let list = if items.is_empty() { "[]".to_string() } else { list };
        let _ = list;
        let given: Vec<(Option<M>, String)> = {
            let mut v = Vec::new();
            for (k, (m, t)) in [(&a, &ta), (&b, &tb), (&c, &tc)].into_iter().enumerate() {
                if rng.chance(20) {
                    v.push((None, "zz.q".to_string()));
                }
                if k < 3 && rng.chance(85) {
                    v.push((Some(m.clone()), t.clone()));
                }
            }
            v
        };
        let text = format!("[{}]", given.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join(", "));
        let present: Vec<&M> = given.iter().filter_map(|(m, _)| m.as_ref()).collect();
        check(format!("len({text})"), Value::Num(given.len() as f64));
        check(format!("count({text})"), Value::Num(present.len() as f64));
        check(format!("count({text}, {ta})"), Value::Num(present.iter().filter(|x| m_eq(x, &a)).count() as f64));
        let last = given.last().and_then(|(m, _)| m.as_ref()).map_or(Value::Missing, m_value);
        check(format!("last({text})"), last);
        let same = given.first().is_some_and(|(f, _)| {
            f.as_ref().is_some_and(|f| given.iter().all(|(m, _)| m.as_ref().is_some_and(|m| m_eq(m, f))))
        });
        check(format!("same({text})"), yes(same));
        let sum = if present.is_empty() {
            Value::Num(0.0)
        } else if present.iter().all(|m| matches!(m, M::N(_))) {
            Value::Num(present.iter().map(|m| if let M::N(h) = m { *h } else { 0 }).sum::<i64>() as f64 / 2.0)
        } else if present.iter().all(|m| matches!(m, M::D(_))) {
            Value::Dur(present.iter().map(|m| if let M::D(d) = m { *d } else { 0 }).fold(0u64, u64::saturating_add))
        } else if present.iter().all(|m| matches!(m, M::U(_))) {
            Value::Usd(present.iter().map(|m| if let M::U(u) = m { *u } else { 0 }).sum())
        } else {
            Value::Missing
        };
        check(format!("sum({text})"), sum);
    }
    sink.finish(N_ALGEBRA);
}

// ── 2d. the laws of Value ────────────────────────────────────────────────────────────────────

fn has_missing(v: &Value) -> bool {
    match v {
        Value::Missing => true,
        Value::List(l) => l.iter().any(has_missing),
        Value::Map(m) => m.values().any(has_missing),
        _ => false,
    }
}

/// On coarse values (`fine`) `equals` is symmetric, `sort_cmp` is a total preorder that agrees with `equals`, and
/// `equals` is reflexive for everything that has no missing part.
#[test]
fn value_laws_hold_on_coarse_values() {
    let mut rng = Rng::new(0x5EED_00A1);
    let mut sink = Sink::new("value laws");
    for i in 0..N_ALGEBRA {
        if sink.done() {
            break;
        }
        let (a, b, c) = (value(&mut rng, 1, true), value(&mut rng, 1, true), value(&mut rng, 1, true));
        let label = format!("{a:?} / {b:?} / {c:?}");
        if a.equals(&b) != b.equals(&a) {
            sink.fail(format!("[{i}] equals is not symmetric: {label}"));
        }
        if !has_missing(&a) && !a.equals(&a) {
            sink.fail(format!("[{i}] equals is not reflexive: {a:?}"));
        }
        let number_and_money = matches!((&a, &b), (Value::Num(_), Value::Usd(_)) | (Value::Usd(_), Value::Num(_)));
        if a.equals(&b) && !number_and_money && a.sort_cmp(&b) != Ordering::Equal {
            sink.fail(format!("[{i}] equal values sort apart: {label}"));
        }
        if a.sort_cmp(&b) != b.sort_cmp(&a).reverse() {
            sink.fail(format!("[{i}] sort_cmp is not antisymmetric: {label}"));
        }
        if a.sort_cmp(&b) != Ordering::Greater
            && b.sort_cmp(&c) != Ordering::Greater
            && a.sort_cmp(&c) == Ordering::Greater
        {
            sink.fail(format!("[{i}] sort_cmp is not transitive: {label}"));
        }
        if a.sort_cmp(&b) == Ordering::Equal && b.sort_cmp(&c) == Ordering::Equal && a.sort_cmp(&c) != Ordering::Equal {
            sink.fail(format!("[{i}] sort_cmp ties are not transitive: {label}"));
        }
        if let (Some(x), Some(y)) = (a.order(&b), b.order(&a))
            && x != y.reverse()
        {
            sink.fail(format!("[{i}] order is not antisymmetric: {label}"));
        }
        if let Some(Ordering::Less | Ordering::Greater) = a.order(&b)
            && (a.equals(&b) || a.truthy() != a.truthy())
        {
            sink.fail(format!("[{i}] values that order apart are equal: {label}"));
        }
    }
    sink.finish(N_ALGEBRA);
}

/// `Value::pointer` reads what `at` reads, for every key including ones with `~` and `/` in them.
#[test]
fn pointer_reads_what_a_walk_reads_whatever_the_keys_look_like() {
    let mut rng = Rng::new(0x5EED_00A2);
    let keys =
        ["a", "b", "a/b", "~", "~0", "~1", "~01", "~~1", "a~b", "", "01", "1", "-1", "+1", " ", "é", "x.y", "%41"];
    fn build(rng: &mut Rng, keys: &[&str], depth: u32) -> Value {
        if depth == 0 || rng.chance(25) {
            return Value::text(format!("leaf{}", rng.below(1000)));
        }
        if rng.chance(30) {
            Value::List((0..1 + rng.below(4)).map(|_| build(rng, keys, depth - 1)).collect())
        } else {
            Value::Map(
                (0..1 + rng.below(4)).map(|_| ((*rng.pick(keys)).to_string(), build(rng, keys, depth - 1))).collect(),
            )
        }
    }
    for i in 0..N_ALGEBRA {
        let doc = build(&mut rng, &keys, 4);
        // walk down at random, remembering the keys and what was found
        let (mut node, mut ptr, mut path) = (&doc, String::new(), Vec::new());
        loop {
            let step: Option<(String, Key, &Value)> = match node {
                Value::Map(m) if !m.is_empty() => {
                    let (k, v) = m.iter().nth(rng.below(m.len())).unwrap();
                    Some((k.clone(), Key::Name(k.clone()), v))
                }
                Value::List(l) if !l.is_empty() => {
                    let k = rng.below(l.len());
                    Some((k.to_string(), Key::Idx(k as i64), &l[k]))
                }
                _ => None,
            };
            let Some((name, key, next)) = step else { break };
            ptr += &format!("/{}", name.replace('~', "~0").replace('/', "~1"));
            path.push(key);
            node = next;
            assert_eq!(&doc.pointer(&ptr), node, "[{i}] pointer {ptr:?} of {doc:?}");
            assert_eq!(&doc.at(&path), node, "[{i}] path {path:?} of {doc:?}");
            if rng.chance(25) {
                break;
            }
        }
        // a pointer that goes past a leaf, or that does not start with `/`, is missing
        assert_eq!(
            doc.pointer(&format!("{ptr}/zz")).kind(),
            if matches!(node, Value::Map(_) | Value::List(_)) { Value::Missing.kind() } else { "nothing" }
        );
        assert_eq!(doc.pointer("x"), Value::Missing);
    }
}

// ── 3. templates ─────────────────────────────────────────────────────────────────────────────

enum Model {
    Lit(String),
    Hole(String, Option<Format>),
}

impl Gen {
    /// A literal run of a template: (as written, as meant).
    fn fmt_lit(&mut self) -> (String, String) {
        let (mut src, mut mean) = (String::new(), String::new());
        for _ in 0..1 + self.rng.below(6) {
            match self.rng.below(12) {
                0 => {
                    src += "{{";
                    mean.push('{');
                }
                1 => {
                    src += "}}";
                    mean.push('}');
                }
                _ => {
                    let w = *self.rng.pick(&[
                        "abc", " ", "x", ":", ",", "'", "\"", "\\", "é", "—", "\n", "😀", "0", "usd", " of ", "/", "[",
                        "]", "(", ")", "$", "%", "e\u{301}", "\u{200b}", "\t", "\u{0}",
                    ]);
                    src += w;
                    mean += w;
                }
            }
        }
        (src, mean)
    }

    fn template(&mut self) -> (String, Vec<Model>) {
        let (mut text, mut model) = (String::new(), Vec::new());
        for _ in 0..self.rng.below(6) {
            if self.rng.chance(50) {
                let (src, mean) = self.fmt_lit();
                text += &src;
                match model.last_mut() {
                    Some(Model::Lit(l)) => l.push_str(&mean),
                    _ => model.push(Model::Lit(mean)),
                }
            } else {
                self.fresh();
                let e = self.expr(0);
                let (a, b) = (self.ws(), self.ws());
                let format = if self.rng.chance(60) { Some(*self.rng.pick(&Format::ALL)) } else { None };
                text += &format!("{{{a}{e}{b}");
                if let Some(f) = format {
                    let (c, d) = (self.ws(), self.ws());
                    text += &format!(":{c}{}{d}", f.word());
                }
                text.push('}');
                model.push(Model::Hole(e, format));
            }
        }
        (text, model)
    }
}

#[test]
fn valid_templates_round_trip_and_render_totally() {
    let mut g = Gen::new(0x5EED_0006);
    g.colon_ok = false;
    let mut sink = Sink::new("valid templates");
    let mut done = 0usize;
    for i in 0..N_FMT {
        if sink.done() {
            break;
        }
        let (text, model) = g.template();
        if chars(&text) > 1400 {
            continue;
        }
        done += 1;
        let scope = random_scope(&mut g.rng);
        let f = match Fmt::parse(&text) {
            Ok(f) => f,
            Err(e) => {
                sink.fail(format!(
                    "[{i}] the generator wrote a template the parser refuses (a test mistake?): {text:?}: {e}"
                ));
                continue;
            }
        };
        // the pieces are what was written
        if f.pieces().len() != model.len() {
            sink.fail(format!(
                "[{i}] {} pieces, {} expected for {text:?}: {:?}",
                f.pieces().len(),
                model.len(),
                f.pieces()
            ));
            continue;
        }
        let mut oracle = String::new();
        for (piece, want) in f.pieces().iter().zip(&model) {
            match (piece, want) {
                (Piece::Lit(got), Model::Lit(want)) if got == want => oracle += got,
                (Piece::Ref { expr, format }, Model::Hole(src, want))
                    if format == want && Expr::parse(src).as_ref() == Ok(expr) =>
                {
                    oracle += &format_value(&expr.eval(&scope), *format);
                }
                _ => sink.fail(format!("[{i}] a piece is not what was written in {text:?}: {piece:?}")),
            }
        }
        roundtrip_fmt(&mut sink, &text, &f);
        let first = guarded(&mut sink, "Fmt::render", &text, || f.render(&scope));
        let second = guarded(&mut sink, "Fmt::render (2nd)", &text, || f.render(&scope));
        if let (Some(first), Some(second)) = (first, second) {
            if first != second {
                sink.fail(format!("[{i}] render is not deterministic for {text:?}: {first:?} then {second:?}"));
            }
            if first != oracle {
                sink.fail(format!(
                    "[{i}] render != the pieces rendered one by one for {text:?}: {first:?} vs {oracle:?}"
                ));
            }
        } else {
            sink.fail(format!("[{i}] scope: {:.1200}", format!("{scope:?}")));
        }
        guarded(&mut sink, "Fmt::check", &text, || (f.check(None), f.check(Some(&["a"]))));
    }
    assert!(done > N_FMT / 2, "too many templates were skipped: {done}");
    sink.finish(N_FMT);
}

/// `format_value` over random values through every formatter: total, and the formatters keep their word.
#[test]
fn every_formatter_over_random_values_is_total() {
    let mut rng = Rng::new(0x5EED_0007);
    let mut sink = Sink::new("format_value");
    for i in 0..N_FMT {
        if sink.done() {
            break;
        }
        let v = value(&mut rng, 0, false);
        for f in std::iter::once(None).chain(Format::ALL.into_iter().map(Some)) {
            let label = format!("{v:?} as {f:?}");
            let Some(s) = guarded(&mut sink, "format_value", &label, || format_value(&v, f)) else { continue };
            if s.is_empty() {
                sink.fail(format!("[{i}] {label} is the empty string (a missing value is {DASH:?})"));
            }
            if v == Value::Missing && s != DASH {
                sink.fail(format!("[{i}] {label} is {s:?}, not the dash"));
            }
            // short on a scalar: one line, at most SHORT_MAX characters
            if f == Some(Format::Short)
                && matches!(v, Value::Str(_) | Value::Num(_))
                && (chars(&s) > SHORT_MAX || s.contains(['\n', '\r']))
            {
                sink.fail(format!("[{i}] {label} is not one short line: {s:?}"));
            }
        }
    }
    sink.finish(N_FMT);
}

// ── 4. Query ─────────────────────────────────────────────────────────────────────────────────

/// What the documentation says a row's `Env` is: a root of the outer `Env` first, then a field of the row; `row` is
/// the whole row; a pointer reads into the row.
struct RowOracle<'a> {
    row: &'a Value,
    outer: &'a Scope,
}

impl Env for RowOracle<'_> {
    fn get(&self, root: &str, path: &[Key]) -> Value {
        if root == "row" {
            return self.row.at(path);
        }
        if self.outer.get(root, &[]) == Value::Missing
            && let Value::Map(m) = self.row
            && let Some(v) = m.get(root)
        {
            return v.at(path);
        }
        self.outer.get(root, path)
    }

    fn pointer(&self, ptr: &str) -> Value {
        self.row.pointer(ptr)
    }

    fn now_ms(&self) -> Option<u64> {
        self.outer.now_ms()
    }

    fn danger(&self, cmd: &str) -> Value {
        self.outer.danger(cmd)
    }
}

fn row(rng: &mut Rng, id: usize, fine: bool) -> Value {
    if rng.chance(3) {
        return value(rng, 2, fine);
    }
    let mut m = BTreeMap::new();
    m.insert("id".to_string(), Value::Num(id as f64));
    let mut put = |k: &str, v: Value, rng: &mut Rng| {
        if rng.chance(85) {
            m.insert(k.to_string(), v);
        }
    };
    put("v", value(rng, 1, fine), rng);
    put("n", Value::num(rng.between(-3, 3) as f64), rng);
    put("s", Value::Str((*rng.pick(&["", "a", "b", "abc", "Abc", "é", "z", "running", "done"])).to_string()), rng);
    put("t", Value::text(*rng.pick(&["a", "b", "c"])), rng);
    put("d", Value::Dur(dur(rng, true)), rng);
    put("m", Value::Usd(usd(rng, true)), rng);
    put("w", value(rng, 1, fine), rng);
    put("data", map(vec![("cmd", Value::Str((*rng.pick(&["ls", "rm -rf /", "git push", ""])).to_string()))]), rng);
    Value::Map(m)
}

fn outer_scope() -> Scope {
    // `n` is also a field of most rows: the row's own must win
    Scope::new()
        .with("n", Value::Num(100.0))
        .with("fold", map(vec![("spend", Value::Usd(2_000_000))]))
        .with("set", map(vec![("loop", Value::text("a"))]))
}

fn cmp_keys(desc: &[bool], a: &[Value], b: &[Value]) -> Ordering {
    for ((d, x), y) in desc.iter().zip(a).zip(b) {
        let ord = match (x, y) {
            (Value::Missing, Value::Missing) => Ordering::Equal,
            (Value::Missing, _) => return Ordering::Greater,
            (_, Value::Missing) => return Ordering::Less,
            _ if *d => y.sort_cmp(x),
            _ => x.sort_cmp(y),
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    Ordering::Equal
}

fn multiset(rows: &[Value]) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for r in rows {
        *m.entry(format!("{r:?}")).or_insert(0) += 1;
    }
    m
}

fn sub_multiset(small: &[Value], big: &[Value]) -> bool {
    let big = multiset(big);
    multiset(small).iter().all(|(k, n)| big.get(k).is_some_and(|m| m >= n))
}

#[test]
fn query_apply_filters_orders_and_limits_as_the_documentation_says() {
    let mut g = Gen::new(0x5EED_0008);
    g.roots = QROOTS;
    g.known = QROOTS;
    let mut sink = Sink::new("Query::apply");
    let outer = outer_scope();
    let key_pool = [
        "v",
        "n",
        "s",
        "t",
        "d",
        "m",
        "id",
        "data.cmd",
        "len(s)",
        "w",
        "row",
        "/v",
        "-n",
        "-v",
        "-s",
        "-t",
        "-d",
        "-m",
        "-w",
        "-row",
        "n in [1, 2]",
    ];
    let (mut total_rows, mut total_out, mut sorted_cases) = (0usize, 0usize, 0usize);
    for i in 0..N_QUERY {
        if sink.done() {
            break;
        }
        let nrows = if i % 50 == 0 { 21 + g.rng.below(150) } else { g.rng.below(12) };
        let rows: Vec<Value> = (0..nrows).map(|k| row(&mut g.rng, k, true)).collect();
        g.fresh();
        let filter = if g.rng.chance(60) { Some(g.expr(0)) } else { None };
        let keys: Vec<String> = (0..g.rng.below(5))
            .map(|_| {
                if g.rng.chance(30) {
                    g.fresh();
                    g.expr(1)
                } else {
                    (*g.rng.pick(&key_pool)).to_string()
                }
            })
            .collect();
        let order = if keys.is_empty() { None } else { Some(keys.join(", ")) };
        let limit = if g.rng.chance(40) {
            Some(1 + g.rng.below(30) as u64)
        } else if g.rng.chance(10) {
            Some(MAX_LIMIT)
        } else {
            None
        };
        let label = format!("filter {filter:?} order {order:?} limit {limit:?}");
        let q = match Query::parse("tasks", filter.as_deref(), order.as_deref(), limit) {
            Ok(q) => q,
            Err(e) => {
                sink.fail(format!(
                    "[{i}] the generator wrote a query the parser refuses (a test mistake?): {label}: {e:?}"
                ));
                continue;
            }
        };
        let Some(out) = guarded(&mut sink, "Query::apply", &label, || q.apply(rows.clone(), &outer)) else {
            sink.fail(format!("[{i}] rows: {:.1500}", format!("{rows:?}")));
            continue;
        };
        total_rows += rows.len();
        total_out += out.len();
        // the filter, read by the documented rules, decides which rows are in
        let keep = |r: &Value| q.filter.as_ref().is_none_or(|f| f.holds(&RowOracle { row: r, outer: &outer }));
        let kept: Vec<Value> = rows.iter().filter(|r| keep(r)).cloned().collect();
        // limit, and a sub-multiset of the rows that passed the filter
        if let Some(n) = limit
            && out.len() as u64 > n
        {
            sink.fail(format!("[{i}] {} rows come out of a limit of {n}: {label}", out.len()));
        }
        if out.len() > kept.len() || !sub_multiset(&out, &kept) {
            sink.fail(format!("[{i}] the result is not a sub-multiset of the rows that pass the filter: {label}; in {rows:?} out {out:?}"));
            continue;
        }
        // deterministic
        if q.apply(rows.clone(), &outer) != out {
            sink.fail(format!("[{i}] apply is not deterministic: {label}"));
        }
        // limit takes the head of the unlimited answer, and the filter alone keeps the input order
        let unlimited = Query { limit: None, ..q.clone() }.apply(rows.clone(), &outer);
        let head: Vec<Value> = unlimited.iter().take(limit.map_or(usize::MAX, |n| n as usize)).cloned().collect();
        if head != out {
            sink.fail(format!("[{i}] limit is not the head of the unlimited answer: {label}"));
        }
        if unlimited.len() != kept.len() || !sub_multiset(&kept, &unlimited) {
            sink.fail(format!("[{i}] the rows that pass the filter are not the rows that come out: {label}"));
        }
        let unordered = Query { order: vec![], limit: None, ..q.clone() }.apply(rows.clone(), &outer);
        if unordered != kept {
            sink.fail(format!("[{i}] a query with no order must keep the input order of the rows that pass: {label}"));
        }
        // the order: the keys read by the documented rules, missing last either way, ties in their place (stable)
        let desc: Vec<bool> = q.order.iter().map(|o| o.desc).collect();
        let mut expected: Vec<(Vec<Value>, Value)> = kept
            .iter()
            .map(|r| (q.order.iter().map(|o| o.key.eval(&RowOracle { row: r, outer: &outer })).collect(), r.clone()))
            .collect();
        expected.sort_by(|a, b| cmp_keys(&desc, &a.0, &b.0));
        let expected_rows: Vec<Value> = expected.iter().map(|(_, r)| r.clone()).collect();
        if unlimited != expected_rows {
            sink.fail(format!("[{i}] the order is not the documented one: {label}; in {rows:?} out {unlimited:?} expected {expected_rows:?}"));
        }
        // and the output really is sorted: every earlier row is not after a later one
        let out_keys: Vec<Vec<Value>> = unlimited
            .iter()
            .map(|r| q.order.iter().map(|o| o.key.eval(&RowOracle { row: r, outer: &outer })).collect())
            .collect();
        'pairs: for a in 0..out_keys.len() {
            for b in a + 1..out_keys.len() {
                if cmp_keys(&desc, &out_keys[a], &out_keys[b]) == Ordering::Greater {
                    sink.fail(format!(
                        "[{i}] rows {a} and {b} are out of order: {label}; keys {:?} then {:?}",
                        out_keys[a], out_keys[b]
                    ));
                    break 'pairs;
                }
            }
        }
        sorted_cases += usize::from(!q.order.is_empty() && unlimited.len() > 1);
    }
    assert!(
        total_rows > 20_000 && total_out > 5_000 && sorted_cases > 2_000,
        "the queries are too thin: {total_rows} rows, {total_out} out, {sorted_cases} sorted"
    );
    sink.finish(N_QUERY);
}

#[test]
fn query_parse_enforces_its_limits() {
    for n in [1, 2, 10_000] {
        assert!(Query::parse("tasks", None, None, Some(n)).is_ok(), "limit {n}");
    }
    for n in [0, 10_001, u64::MAX] {
        assert!(Query::parse("tasks", None, None, Some(n)).is_err(), "limit {n}");
    }
    let keys = |k: usize| vec!["a"; k].join(", ");
    assert!(Query::parse("tasks", None, Some(&keys(MAX_ORDER)), None).is_ok());
    assert!(Query::parse("tasks", None, Some(&keys(MAX_ORDER + 1)), None).is_err());
    assert!(Query::parse("tasks", None, Some(&keys(1000)), None).is_err());
    // a comma inside quotes, brackets and parentheses is not a key separator
    let q = Query::parse("tasks", None, Some("len('a,b'), -count([1, 2], 1), last(['x,y'])"), None).unwrap();
    assert_eq!(q.order.len(), 3);
    assert!(q.order[1].desc);
    // every mistake at once
    let errs = Query::parse("nope", Some("a ="), Some(""), Some(0)).unwrap_err();
    assert!(errs.len() >= 4, "{errs:?}");
}

/// BUG: `Value::order` compares a number against money after rounding the number to a millionth, and two numbers
/// exactly, so "equal to" is not transitive: `Num(4e-7) == Usd(0) == Num(1e-7)` but `Num(4e-7) > Num(1e-7)`. That is
/// not a total order, and `Query::apply` sorts with it: the result is not sorted (and `sort_by` may panic on a
/// comparison that is not a total order, since Rust 1.81).
#[test]
fn bug_query_order_is_not_a_total_order_for_numbers_against_money() {
    let rows = vec![row_v(Value::Num(4e-7)), row_v(Value::Usd(0)), row_v(Value::Num(1e-7))];
    let q = Query::parse("tasks", None, Some("v"), None).unwrap();
    let out = q.apply(rows, &Scope::new());
    let keys: Vec<Value> = out.iter().map(|r| r.at(&[Key::Name("v".into())])).collect();
    assert!(
        keys.iter().enumerate().all(|(i, a)| keys[i + 1..].iter().all(|b| a.sort_cmp(b) != Ordering::Greater)),
        "ascending by `v` left a bigger value before a smaller one: {keys:?}"
    );
}

fn row_v(v: Value) -> Value {
    map(vec![("v", v)])
}

/// BUG (same cause): a long mixed list, many times, must neither panic nor come out unsorted.
#[test]
fn bug_query_order_over_mixed_numbers_and_money_neither_panics_nor_misorders() {
    let mut rng = Rng::new(0x5EED_0009);
    let mut sink = Sink::new("Query::apply over Num and Usd");
    let q = Query::parse("tasks", None, Some("v"), None).unwrap();
    for i in 0..3000 {
        if sink.done() {
            break;
        }
        let n = 3 + rng.below(if i % 10 == 0 { 400 } else { 30 });
        let rows: Vec<Value> = (0..n)
            .map(|_| {
                row_v(match rng.below(4) {
                    0 => Value::Num(rng.between(0, 6) as f64 * 1e-7),
                    1 => Value::Usd(rng.between(0, 3)),
                    2 => Value::Num(rng.between(0, 3) as f64 * 1e-6),
                    _ => Value::Usd(rng.between(0, 3) * 1_000_000),
                })
            })
            .collect();
        let label = format!("{n} rows");
        let Some(out) = guarded(&mut sink, "Query::apply", &label, || q.apply(rows.clone(), &Scope::new())) else {
            sink.fail(format!("[{i}] rows {rows:?}"));
            continue;
        };
        let keys: Vec<Value> = out.iter().map(|r| r.at(&[Key::Name("v".into())])).collect();
        'pairs: for a in 0..keys.len() {
            for b in a + 1..keys.len() {
                if keys[a].sort_cmp(&keys[b]) == Ordering::Greater {
                    sink.fail(format!("[{i}] {:?} sorted before {:?} ({n} rows)", keys[a], keys[b]));
                    break 'pairs;
                }
            }
        }
    }
    sink.finish(3000);
}

/// BUG: 21 rows (the length where the stable sort stops using insertion sort) with `Usd(0)` among small numbers
/// make `sort_by` panic ("user-provided comparison function does not correctly implement a total order"), so
/// `Query::apply` panics with values inside the contract.
#[test]
fn bug_query_apply_panics_on_21_rows_of_small_numbers_and_one_zero_dollars() {
    let palette = [Value::Num(4e-7), Value::Usd(0), Value::Num(1e-7), Value::Num(2e-7), Value::Num(0.0)];
    let rows: Vec<Value> = [3, 0, 4, 2, 2, 0, 1, 4, 0, 4, 3, 2, 3, 0, 3, 4, 0, 0, 3, 4, 4]
        .iter()
        .map(|&i| row_v(palette[i].clone()))
        .collect();
    let q = Query::parse("tasks", None, Some("v"), None).unwrap();
    let r = caught(|| q.apply(rows, &Scope::new()));
    assert!(r.is_ok(), "Query::apply panicked: {r:?}");
}

/// BUG: the same panic with a number that is not finite (`Value::Num` is documented finite, but its field is public):
/// 21 rows with a NaN key.
#[test]
fn bug_query_apply_panics_on_21_rows_with_a_nan_key() {
    let keys = [
        1.0,
        1.0,
        1.0,
        3.0,
        3.0,
        f64::NAN,
        2.0,
        3.0,
        3.0,
        2.0,
        f64::NAN,
        2.0,
        f64::NAN,
        3.0,
        2.0,
        2.0,
        2.0,
        1.0,
        1.0,
        2.0,
        f64::NAN,
    ];
    let rows: Vec<Value> = keys.iter().map(|&k| row_v(Value::Num(k))).collect();
    let q = Query::parse("tasks", None, Some("v"), None).unwrap();
    let r = caught(|| q.apply(rows, &Scope::new()));
    assert!(r.is_ok(), "Query::apply panicked: {r:?}");
}

/// Values that break the documented contract (`Num` is finite) must still not panic the sort.
#[test]
fn query_order_over_non_finite_numbers_does_not_panic() {
    let mut rng = Rng::new(0x5EED_000A);
    let q = Query::parse("tasks", None, Some("v, -v"), None).unwrap();
    for i in 0..2000 {
        let n = 2 + rng.below(if i % 5 == 0 { 300 } else { 25 });
        let rows: Vec<Value> = (0..n)
            .map(|_| {
                row_v(match rng.below(5) {
                    0 => Value::Num(f64::NAN),
                    1 => Value::Num(f64::INFINITY),
                    2 => Value::Num(f64::NEG_INFINITY),
                    3 => Value::Num(rng.between(-3, 3) as f64),
                    _ => Value::Missing,
                })
            })
            .collect();
        let r = caught(|| q.apply(rows.clone(), &Scope::new()));
        assert!(r.is_ok(), "panic over NaN/inf keys ({n} rows): {r:?}");
        assert!(r.unwrap().len() == n);
    }
}

/// A `Value` has no depth bound of its own (the readers that build them cap theirs: JSON stops at 128 levels), and
/// `clone`, `equals` and `format_value` recurse as deep as it is: 200 levels are far inside any stack.
#[test]
fn values_a_few_hundred_levels_deep_are_fine() {
    let mut v = Value::Num(1.0);
    for _ in 0..200 {
        v = Value::List(vec![v]);
    }
    assert!(v.clone().equals(&v));
    assert_eq!(format_value(&v, None), "1");
}

// ── 5. deep nesting ──────────────────────────────────────────────────────────────────────────

fn on_small_stack<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> R {
    std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap_or_else(|_| panic!("the thread panicked"))
}

#[test]
fn deep_nesting_is_refused_quickly_without_overflowing_the_stack() {
    const SHAPES: &[(&str, &str, &str)] = &[
        ("(", "1", ")"),
        ("[", "1", "]"),
        ("not ", "x", ""),
        ("len(", "x", ")"),
        ("count(", "x", ")"),
        ("[(", "1", ")]"),
        ("not (", "x", ")"),
        ("(not ", "x", ")"),
        ("a[", "1", "]"),
        ("/a[", "1", "]"),
        ("last([", "x", "])"),
        ("(a and ", "b", ")"),
        ("(a or not ", "b", ")"),
        ("contains(a, (", "b", "))"),
    ];
    let start = Instant::now();
    let failures = on_small_stack(|| {
        let mut bad = Vec::new();
        for &(open, mid, close) in SHAPES {
            // 5000 levels, the longest that still fits MAX_LEN, and a few just over the depth limit
            for n in [5000, 1999, 1000, 500, 100, 30, 26, 25] {
                let text = format!("{}{mid}{}", open.repeat(n), close.repeat(n));
                let hole = format!("{{{text}}}");
                let hole_fmt = format!("{{{text}:usd}}");
                let r = (
                    Expr::parse(&text),
                    Fmt::parse(&hole),
                    Fmt::parse(&hole_fmt),
                    Query::parse("tasks", Some(&text), Some(&text), None),
                );
                match &r.0 {
                    Err(e) if e.at > chars(&text) => {
                        bad.push(format!("{open:?}{mid}{close:?} x{n}: position {} past the end", e.at))
                    }
                    Err(_) => {}
                    Ok(_) => bad.push(format!("{open:?}{mid}{close:?} x{n} was accepted as an expression")),
                }
                if r.1.is_ok() || r.2.is_ok() {
                    bad.push(format!("{open:?}{mid}{close:?} x{n} was accepted as a template hole"));
                }
                if r.3.is_ok() {
                    bad.push(format!("{open:?}{mid}{close:?} x{n} was accepted as a query"));
                }
            }
        }
        bad
    });
    assert!(failures.is_empty(), "{failures:#?}");
    assert!(start.elapsed().as_secs_f64() < 30.0, "refusing deep nesting took {:?}", start.elapsed());
}

/// Templates whose braces and holes nest or repeat 5000 times.
#[test]
fn braces_and_holes_repeated_thousands_of_times_are_fine() {
    let start = Instant::now();
    on_small_stack(|| {
        assert!(Fmt::parse(&"{{".repeat(1000)).is_ok());
        assert!(Fmt::parse(&"}}".repeat(1000)).is_ok());
        assert!(Fmt::parse(&"{".repeat(5001)).is_err());
        assert!(Fmt::parse(&"{".repeat(1801)).is_err());
        assert!(Fmt::parse(&"{".repeat(1800)).is_ok());
        assert!(Fmt::parse(&"}".repeat(5001)).is_err());
        assert!(Fmt::parse(&"{a}".repeat(600)).is_ok());
        assert!(Fmt::parse(&"{a}".repeat(7000)).is_err()); // 21000 characters, over MAX_FMT
        assert!(Fmt::parse(&"{'}'".repeat(500)).is_err());
        assert!(Fmt::parse(&format!("{{{}}}", "'".repeat(1990))).is_err());
        assert!(Expr::parse(&"a ".repeat(1000)).is_err());
        assert!(Expr::parse(&"a and ".repeat(300)).is_err());
        assert!(Expr::parse(&format!("a{}", " and a".repeat(300))).is_ok());
        assert!(Expr::parse(&format!("a{}", ".b".repeat(900))).is_ok());
        assert!(Expr::parse(&format!("a{}", "[0]".repeat(600))).is_ok());
        assert!(Expr::parse(&format!("[{}1]", "1,".repeat(990))).is_ok());
        assert!(Query::parse("tasks", Some(&"a or ".repeat(5000)), Some(&"a, ".repeat(5000)), None).is_err());
    });
    assert!(start.elapsed().as_secs_f64() < 30.0, "took {:?}", start.elapsed());
}

/// The limits count characters, not bytes, and a position counts characters too.
#[test]
fn limits_and_positions_count_characters() {
    let one_line_of = |n: usize| format!("'{}'", "é".repeat(n - 2));
    assert!(Expr::parse(&one_line_of(MAX_LEN)).is_ok(), "2000 characters (3998 bytes) are within MAX_LEN");
    let e = Expr::parse(&one_line_of(MAX_LEN + 1)).unwrap_err();
    assert_eq!(e.at, MAX_LEN);
    assert!(Expr::parse(&format!("1{}", " ".repeat(MAX_LEN - 1))).is_ok());
    assert!(Expr::parse(&format!("1{}", " ".repeat(MAX_LEN))).is_err());
    assert!(Fmt::parse(&"é".repeat(MAX_FMT)).is_ok());
    assert!(Fmt::parse(&"é".repeat(MAX_FMT + 1)).is_err());
    assert_eq!(Expr::parse("'日本' @").unwrap_err().at, 5);
    assert_eq!(Expr::parse("😀").unwrap_err().at, 0);
    assert_eq!(Expr::parse("a 😀").unwrap_err().at, 2);
    assert_eq!(Fmt::parse("日本 {a} {").unwrap_err().at, 7);
    assert_eq!(Fmt::parse("日本 {a @}").unwrap_err().at, 6);
    assert_eq!(Fmt::parse("😀😀 }").unwrap_err().at, 3);
}

/// BUG: the documentation says `MAX_DEPTH` levels of nesting are allowed and the error says "nested more than 24 levels
/// deep", but the top level counts as one, so 24 nested parentheses (and 24 nested `not`) are refused: the deepest
/// accepted are 23.
#[test]
fn bug_nesting_limit_is_one_less_than_max_depth() {
    let parens = format!("{}1{}", "(".repeat(MAX_DEPTH), ")".repeat(MAX_DEPTH));
    assert!(Expr::parse(&parens).is_ok(), "{MAX_DEPTH} nested parentheses are refused: {:?}", Expr::parse(&parens));
    let nots = format!("{}x", "not ".repeat(MAX_DEPTH));
    assert!(Expr::parse(&nots).is_ok(), "{MAX_DEPTH} nested `not` are refused: {:?}", Expr::parse(&nots));
    // and one more is refused
    assert!(Expr::parse(&format!("{}1{}", "(".repeat(MAX_DEPTH + 1), ")".repeat(MAX_DEPTH + 1))).is_err());
}

/// BUG: `Display` prints `not not x` as `not (not x)`, which nests twice as deep, so an expression within MAX_DEPTH
/// (13 `not` in a row) prints as text the parser refuses ("nested more than 24 levels deep"):
/// `parse(display(parse("not not … not x"))) != parse(…)`.
#[test]
fn bug_display_of_repeated_not_exceeds_the_depth_limit() {
    let mut failures = Vec::new();
    for k in 1..MAX_DEPTH {
        let text = format!("{}x", "not ".repeat(k));
        let Ok(e) = Expr::parse(&text) else { continue };
        if Expr::parse(&e.to_string()).as_ref() != Ok(&e) {
            failures.push(k);
        }
    }
    assert!(
        failures.is_empty(),
        "`not` x{failures:?} does not survive display: 13 × \"not \" + \"x\" prints as {:?}",
        Expr::parse(&format!("{}x", "not ".repeat(13))).map(|e| e.to_string())
    );
}

/// BUG: `Display` spaces what the source did not (`[1,1]` prints `[1, 1]`, `a==b` prints `a == b`), so an expression
/// near MAX_LEN prints longer than MAX_LEN and the printed text is refused: 700 items `[1,1,…]` is 1401 characters
/// and prints as 2101.
#[test]
fn bug_display_can_exceed_max_len() {
    let list = format!("[{}1]", "1,".repeat(700));
    let e = Expr::parse(&list).expect("1402 characters parse");
    let shown = e.to_string();
    assert!(chars(&shown) > MAX_LEN, "the printed list is {} characters", chars(&shown));
    // documented: only the length refuses it
    let back = Expr::parse(&shown);
    assert!(
        back.as_ref() == Ok(&e) || back.as_ref().is_err_and(|err| err.msg.contains("longer than")),
        "{:?}",
        back.err()
    );
    let template = "{a==b}".repeat(333);
    let f = Fmt::parse(&template).expect("1998 characters parse");
    assert!(
        Fmt::parse(&f.to_string()).as_ref() == Ok(&f),
        "the display of a 1998-character template is {} characters: {:?}",
        chars(&f.to_string()),
        Fmt::parse(&f.to_string()).err()
    );
}

/// `Display` of a tree the parser never makes (negative money, a duration over 1e15 ms, a non-finite number, a missing
/// value or a map) does not parse back, and is documented not to: only parsed trees round trip. It must still print.
#[test]
fn display_of_a_hand_built_literal_never_panics() {
    let lits = [
        Value::Usd(-5),
        Value::Usd(i64::MAX),
        Value::Dur(u64::MAX),
        Value::Num(f64::NAN),
        Value::Num(f64::INFINITY),
        Value::Missing,
        Value::Map(BTreeMap::new()),
    ];
    for v in lits {
        assert!(!Expr::Lit(v.clone()).to_string().is_empty(), "{v:?}");
    }
}

// ── 6. glob ──────────────────────────────────────────────────────────────────────────────────

fn glob_reference(p: &[char], t: &[char]) -> bool {
    let mut dp = vec![vec![false; t.len() + 1]; p.len() + 1];
    dp[0][0] = true;
    for i in 1..=p.len() {
        dp[i][0] = p[i - 1] == '*' && dp[i - 1][0];
    }
    for i in 1..=p.len() {
        for j in 1..=t.len() {
            dp[i][j] = match p[i - 1] {
                '*' => dp[i - 1][j] || dp[i][j - 1],
                '?' => dp[i - 1][j - 1],
                c => dp[i - 1][j - 1] && c == t[j - 1],
            };
        }
    }
    dp[p.len()][t.len()]
}

#[test]
fn glob_agrees_with_a_reference_matcher() {
    let mut rng = Rng::new(0x5EED_000B);
    let alphabet = ['a', 'b', 'é', '*', '?', 'a', 'b', '.', '/'];
    for i in 0..N_GLOB {
        let p: Vec<char> = (0..rng.below(9)).map(|_| *rng.pick(&alphabet)).collect();
        let t: Vec<char> =
            (0..rng.below(11)).map(|_| *rng.pick(&['a', 'b', 'é', 'a', 'b', '.', '/', '*', '?'])).collect();
        let (ps, ts): (String, String) = (p.iter().collect(), t.iter().collect());
        assert_eq!(glob_match(&ps, &ts), glob_reference(&p, &t), "[{i}] glob_match({ps:?}, {ts:?})");
        // and through the language: `text matches pattern` is the same
        if i % 20 == 0 {
            let scope = Scope::new().with("t", Value::Str(ts.clone())).with("p", Value::Str(ps.clone()));
            let want = glob_reference(&p, &t);
            assert_eq!(Expr::parse("t matches p").unwrap().holds(&scope), want, "[{i}] {ts:?} matches {ps:?}");
        }
    }
}

fn timed(what: &str, want: bool, f: impl FnOnce() -> bool) -> f64 {
    let start = Instant::now();
    let r = f();
    let secs = start.elapsed().as_secs_f64();
    assert_eq!(r, want, "{what}");
    secs
}

#[test]
fn glob_with_forty_stars_against_a_long_near_miss_is_fast() {
    let n = 100_000;
    let cases: Vec<(&str, bool, String, String)> = vec![
        ("stars then a b", false, "*a".repeat(40) + "b", "a".repeat(n)),
        ("a stars", false, "a*".repeat(40) + "b", "a".repeat(n)),
        ("star question", false, "*?".repeat(40) + "b", "a".repeat(n)),
        ("stars between letters", false, format!("{}c", "*a*b".repeat(20)), "ab".repeat(n / 2)),
        ("end miss", false, format!("{}x", "*".repeat(40)), "y".repeat(n)),
        ("start miss", false, format!("x{}", "*".repeat(40)), "y".repeat(n)),
        ("all stars", true, "*".repeat(40), "y".repeat(n)),
        ("multibyte", false, "*é".repeat(40) + "b", "é".repeat(n)),
        ("zero width", false, "*\u{200b}".repeat(40) + "b", "\u{200b}".repeat(n)),
        ("question marks", false, "?".repeat(40) + "*b", "a".repeat(n)),
    ];
    let mut slow = Vec::new();
    for (name, want, pattern, text) in &cases {
        let secs = timed(name, *want, || glob_match(pattern, text));
        if secs >= 20.0 {
            slow.push(format!("{name}: {secs:.2}s"));
        }
        // and through the language, in a template and in a query filter
        let scope = Scope::new().with("t", Value::Str(text.clone()));
        let start = Instant::now();
        let e = Expr::parse(&format!("t matches {}", Expr::Lit(Value::Str(pattern.clone())))).unwrap();
        assert_eq!(e.holds(&scope), *want, "{name} via Expr");
        if start.elapsed().as_secs_f64() >= 20.0 {
            slow.push(format!("{name} via Expr: {:?}", start.elapsed()));
        }
    }
    assert!(slow.is_empty(), "{slow:?}");
}

/// The simple two-pointer matcher is linear in the text for one star and quadratic when the last literal run after
/// the last star is long and nearly matches everywhere. A pattern is at most MAX_LEN characters; a text is not
/// bounded (a tool's output): 1900 characters against 200000 must still finish.
#[test]
fn glob_with_a_long_last_run_against_a_long_text_is_bounded() {
    let pattern = format!("*{}b", "a".repeat(400));
    let text = "a".repeat(100_000);
    let start = Instant::now();
    let r = glob_match(&pattern, &text);
    let secs = start.elapsed().as_secs_f64();
    assert!(!r);
    assert!(secs < 20.0, "`*` + 400 × a + b against 100000 × a took {secs:.2}s");
}

// ── 7. numeric extremes ──────────────────────────────────────────────────────────────────────

fn extremes() -> Vec<Value> {
    let mut v: Vec<Value> = [
        -0.0,
        0.0,
        1e308,
        -1e308,
        f64::MAX,
        f64::MIN,
        f64::MIN_POSITIVE,
        5e-324,
        1e15,
        999_999_999_999_999.0,
        9e15,
        1e19,
        1e27,
        1e28,
        1e300,
        0.1,
        0.005,
        -0.005,
        0.004999999,
        0.994999,
        0.995,
        999.95,
        999_950.0,
        999_999_999.95,
    ]
    .into_iter()
    .map(Value::Num)
    .collect();
    v.extend(
        [
            i64::MIN,
            i64::MAX,
            i64::MIN + 1,
            0,
            1,
            -1,
            49,
            50,
            4999,
            5000,
            -4999,
            -5000,
            999_999,
            1_000_000,
            9_000_000_000_000_000_000,
        ]
        .map(Value::Usd),
    );
    v.extend(
        [
            u64::MAX,
            u64::MAX - 1,
            0,
            1,
            999,
            1000,
            59_999,
            60_000,
            3_599_999,
            3_600_000,
            86_399_999,
            86_400_000,
            1_000_000_000_000_000,
        ]
        .map(Value::Dur),
    );
    v.push(Value::Missing);
    v
}

fn non_finite() -> Vec<Value> {
    // `Num` is documented finite; a resolver can still build these by hand
    vec![Value::Num(f64::NAN), Value::Num(f64::INFINITY), Value::Num(f64::NEG_INFINITY)]
}

fn all_formats() -> Vec<Option<Format>> {
    std::iter::once(None).chain(Format::ALL.into_iter().map(Some)).collect()
}

#[test]
fn extremes_through_every_formatter_neither_panic_nor_print_nothing() {
    let mut bad = Vec::new();
    for v in extremes().into_iter().chain(non_finite()) {
        for f in all_formats() {
            match caught(|| format_value(&v, f)) {
                Ok(s) if s.is_empty() || s.contains('\n') => bad.push(format!("{v:?} as {f:?} is {s:?}")),
                Ok(_) => {}
                Err(p) => bad.push(format!("{v:?} as {f:?} panics: {p}")),
            }
        }
        // the same through a template hole, whose scope holds the value
        let scope = Scope::new().with("x", v.clone());
        let t = Fmt::parse("{x}|{x:usd}|{x:pct}|{x:n}|{x:dur}|{x:text}|{x:short}|{[x, x]:short}|{x == x}|{quote(x)}")
            .unwrap();
        if let Err(p) = caught(|| t.render(&scope)) {
            bad.push(format!("rendering {v:?} panics: {p}"));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn extremes_through_the_expression_functions_neither_panic_nor_wrap() {
    let exprs = [
        "x",
        "x == x",
        "x != x",
        "x < x",
        "x <= x",
        "x > x",
        "x >= x",
        "[x] == [x]",
        "x in [x]",
        "[x, x] == [x, x]",
        "count([x, x])",
        "count([x, x], x)",
        "sum([x, x])",
        "sum([x, x, x])",
        "sum([x])",
        "same([x, x])",
        "last([x, x])",
        "len([x, x])",
        "len(x)",
        "lower(x)",
        "quote(x)",
        "since(x)",
        "danger(x)",
        "contains(x, x)",
        "x matches x",
        "x[x]",
        "x.y",
        "y[x]",
        "x not in [x]",
        "not x",
        "x and x",
        "x or x",
        "x > 0",
        "x < 0",
        "x == 0",
        "x > 1h",
        "x < $0",
        "x == $1",
        "x > 1000000000000000000",
        "x > 0.5",
        "y[x]",
        "y[0] == x",
    ];
    let mut bad = Vec::new();
    for v in extremes().into_iter().chain(non_finite()) {
        let scope =
            Scope::new().with("x", v.clone()).with("y", Value::List(vec![v.clone(), v.clone()])).at_ms(u64::MAX);
        for text in exprs {
            let e = Expr::parse(text).unwrap();
            match caught(|| (e.eval(&scope), e.eval(&scope))) {
                Ok((a, b)) if a != b && !matches!(v, Value::Num(n) if n.is_nan()) => {
                    bad.push(format!("{text} with {v:?} is not deterministic: {a:?} vs {b:?}"))
                }
                Ok(_) => {}
                Err(p) => bad.push(format!("{text} with {v:?} panics: {p}")),
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn negative_zero_is_zero_everywhere() {
    let neg = Expr::parse("-0").unwrap();
    assert_eq!(neg, Expr::parse("0").unwrap(), "-0 and 0 are the same literal");
    let s = Scope::new().with("z", Value::Num(-0.0)).with("l", Value::List(vec!["a".into(), "b".into()]));
    for (text, want) in [
        ("-0 == 0", true),
        ("z == 0", true),
        ("z", false),
        ("not z", true),
        ("z < 0", false),
        ("z > 0", false),
        ("l[z] == 'a'", true),
        ("l[-0] == 'a'", true),
        ("[z] == [0]", true),
        ("sum([z, z]) == 0", true),
        ("z in [0]", true),
    ] {
        assert_eq!(Expr::parse(text).unwrap().holds(&s), want, "{text}");
    }
    for f in all_formats() {
        let shown = format_value(&Value::Num(-0.0), f);
        assert!(!shown.contains('-'), "-0.0 as {f:?} shows {shown:?}");
    }
    assert_eq!(Fmt::parse("{-0}").unwrap().render(&s), "0");
    assert_eq!(Fmt::parse("{z:pct} {z:dur} {z:usd} {z:short} {z:n}").unwrap().render(&s), "0% 0s $0.00 0 0");
    // the smallest negative numbers also show as 0, never -0
    for n in [-0.001, -0.004, -1e-300, -5e-324] {
        assert_eq!(format_value(&Value::Num(n), None), "0", "{n}");
    }
}

#[test]
fn a_309_digit_number_is_one_e308_and_a_310_digit_one_is_refused() {
    let big = format!("1{}", "0".repeat(308));
    let e = Expr::parse(&big).expect("1e308 written out");
    assert_eq!(e, Expr::Lit(Value::Num(1e308)));
    assert_eq!(Expr::parse(&e.to_string()).as_ref(), Ok(&e));
    let too_big = format!("1{}", "0".repeat(309));
    let err = Expr::parse(&too_big).unwrap_err();
    assert!(err.at <= chars(&too_big));
    // it is a number: it compares, it formats, and a sum that overflows is missing, not infinite
    let s = Scope::new();
    assert!(Expr::parse(&format!("{big} > 1")).unwrap().holds(&s));
    assert!(Expr::parse(&format!("{big} == {big}")).unwrap().holds(&s));
    assert!(Expr::parse(&format!("{big} > 9007199254740993")).unwrap().holds(&s));
    assert_eq!(Expr::parse(&format!("sum([{big}, {big}])")).unwrap().eval(&s), Value::Missing);
    assert_eq!(Expr::parse(&format!("sum([{big}, 1])")).unwrap().eval(&s), Value::Num(1e308));
    for f in all_formats() {
        let out = format_value(&Value::Num(1e308), f);
        assert!(!out.is_empty());
    }
    // 1e308 in a template is the digits, not an exponent or a crash
    let shown = Fmt::parse(&format!("{{{big}}}")).unwrap().render(&s);
    assert_eq!(shown, "1e308");
    // as dollars and as seconds it is too big to show or to count
    assert_eq!(Fmt::parse(&format!("{{{big}:usd}}")).unwrap().render(&s), DASH);
    assert!(Fmt::parse(&format!("{{{big}:dur}}")).unwrap().render(&s).ends_with('h'));
    assert_eq!(Fmt::parse(&format!("{{{big}:pct}}")).unwrap().render(&s), DASH);
}

#[test]
fn nine_e15_as_an_index_is_missing_and_just_below_it_is_a_key() {
    let m = map(vec![
        ("8999999999999999", "below".into()),
        ("9000000000000000", "at".into()),
        ("0", "zero".into()),
        ("-1", "minus".into()),
    ]);
    let s = Scope::new().with("m", m).with("l", Value::List(vec!["a".into(), "b".into()]));
    let ev = |t: &str| Expr::parse(t).unwrap().eval(&s);
    assert_eq!(ev("m[8999999999999999]"), Value::text("below"));
    assert_eq!(ev("m[9000000000000000]"), Value::Missing);
    assert_eq!(ev("m['9000000000000000']"), Value::text("at"));
    assert_eq!(ev("m[-1]"), Value::text("minus"));
    assert_eq!(ev("l[9000000000000000]"), Value::Missing);
    assert_eq!(ev("l[8999999999999999]"), Value::Missing);
    assert_eq!(ev("l[-1]"), Value::Missing);
    assert_eq!(ev("l[1.5]"), Value::Missing);
}

#[test]
fn money_at_the_edges_of_i64_through_formats_sum_and_comparison() {
    let (lo, hi) = (Value::Usd(i64::MIN), Value::Usd(i64::MAX));
    assert_eq!(format_value(&hi, None), "$9223372036854.78");
    assert_eq!(format_value(&lo, None), "-$9223372036854.78");
    assert_eq!(format_value(&hi, Some(Format::Short)), "$9223372036854.78");
    let s = Scope::new().with("lo", lo.clone()).with("hi", hi.clone());
    let ev = |t: &str| Expr::parse(t).unwrap().eval(&s);
    assert_eq!(ev("sum([hi, hi])"), Value::Missing, "a sum too large is missing, never a wrong amount");
    assert_eq!(ev("sum([lo, lo])"), Value::Missing, "a sum too large is missing, never a wrong amount");
    assert_eq!(ev("sum([lo, hi])"), Value::Usd(-1));
    assert_eq!(ev("lo < hi"), Value::Bool(true));
    assert_eq!(ev("lo < $0"), Value::Bool(true));
    assert_eq!(ev("hi > $9000000"), Value::Bool(true));
    assert_eq!(ev("hi > 1000000000000"), Value::Bool(true));
    assert_eq!(ev("lo < -1000000000000"), Value::Bool(true));
    // Value::usd refuses what does not fit
    assert_eq!(Value::usd(9.3e12), Value::Missing);
    assert_eq!(Value::usd(-9.3e12), Value::Missing);
    assert_eq!(Value::usd(f64::NAN), Value::Missing);
    assert_eq!(Value::usd(f64::INFINITY), Value::Missing);
    assert!(matches!(Value::usd(8.9e12), Value::Usd(_)));
}

#[test]
fn durations_at_the_edges_of_u64_through_formats_sum_and_since() {
    let max = Value::Dur(u64::MAX);
    let s = Scope::new().with("d", max.clone()).with("zero", Value::Num(0.0)).at_ms(u64::MAX);
    let ev = |t: &str| Expr::parse(t).unwrap().eval(&s);
    assert_eq!(ev("sum([d, d])"), Value::Missing, "a sum too large is missing, never a wrong amount");
    assert_eq!(ev("since(zero)"), max);
    assert_eq!(ev(&format!("since(1{})", "0".repeat(300))), Value::Dur(0));
    assert_eq!(ev(&format!("since(-1{})", "0".repeat(300))), max);
    assert_eq!(ev("since(18446744073709551615)"), Value::Dur(0));
    assert_eq!(ev("d > 1d"), Value::Bool(true));
    assert_eq!(ev("d == d"), Value::Bool(true));
    assert_eq!(ev("d < 1000000000000000ms"), Value::Bool(false));
    assert_eq!(duration(u64::MAX), "213503982334d14h");
    assert_eq!(format_value(&max, Some(Format::Dur)), "213503982334d14h");
    assert_eq!(format_value(&Value::Num(1e300), Some(Format::Dur)), "213503982334d14h");
    assert_eq!(format_value(&Value::Num(-1e300), Some(Format::Dur)), "0s");
    // 1e15 ms is the longest literal; one more is refused
    assert!(Expr::parse("1000000000000000ms").is_ok());
    assert!(Expr::parse("1000000000000001ms").is_err());
}

/// `since(ts)` for a timestamp that is whole milliseconds in 0..2^53 is exactly `now - ts`, or nothing when it is later.
#[test]
fn since_is_now_minus_the_time_for_any_clock_and_any_timestamp() {
    let mut rng = Rng::new(0x5EED_000C);
    let e = Expr::parse("since(t)").unwrap();
    for i in 0..N_SOUP {
        let ts: u64 = match rng.below(6) {
            0 => 0,
            1 => 1_700_000_000_000,
            2 => rng.next() >> 11,
            3 => 1 << 53,
            4 => (1 << 53) - 1,
            _ => rng.next() >> rng.between(11, 63),
        } & ((1 << 53) - 1);
        let now: u64 = *rng.pick(&[0, 1, 1_700_000_000_000, u64::MAX, u64::MAX / 2, 1 << 53]);
        let now = if rng.chance(50) { now } else { rng.next() >> rng.below(64) };
        let s = Scope::new().with("t", Value::Num(ts as f64)).at_ms(now);
        assert_eq!(e.eval(&s), Value::Dur(now.saturating_sub(ts)), "[{i}] since({ts}) at {now}");
    }
    // no clock, or not a number: nothing
    assert_eq!(e.eval(&Scope::new().with("t", Value::Num(1.0))), Value::Missing);
    assert_eq!(e.eval(&Scope::new().with("t", Value::text("1")).at_ms(5)), Value::Missing);
    assert_eq!(e.eval(&Scope::new().at_ms(5)), Value::Missing);
}

/// `short` writes a number as `1.2k`, `3M`, `2B`, `4T` and, from 1e15 up, as its digits (an exponent from 1e21), never
/// as a saturated amount.
#[test]
fn short_does_not_saturate_huge_numbers() {
    let mut bad = Vec::new();
    for exp in 3..=300 {
        for mant in [1.0, 1.234, 5.5, 9.99] {
            let n = mant * 10f64.powi(exp);
            for sign in [1.0, -1.0] {
                let shown = format_value(&Value::Num(n * sign), Some(Format::Short));
                let body = shown.trim_start_matches('-');
                let (digits, scale) = match body.chars().last() {
                    Some('k') => (&body[..body.len() - 1], 1e3),
                    Some('M') => (&body[..body.len() - 1], 1e6),
                    Some('B') => (&body[..body.len() - 1], 1e9),
                    Some('T') => (&body[..body.len() - 1], 1e12),
                    _ => (body, 1.0),
                };
                let back: f64 = digits.parse().unwrap_or(f64::NAN) * scale;
                if !(back > 0.0 && (back / n - 1.0).abs() < 0.06) {
                    bad.push(format!("{} shows as {shown:?}", n * sign));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} numbers are shown wrongly, e.g. {:?}", bad.len(), &bad[..bad.len().min(4)]);
}

/// BUG: `short` is documented as "one line, at most SHORT_MAX characters", but only text and numbers are cut; a list
/// or a map is joined without a limit and keeps the newlines of its items.
#[test]
fn bug_short_of_a_list_or_map_is_not_one_short_line() {
    let list = Value::List(vec![Value::text("first\nsecond"); 30]);
    let m = map(vec![("k", Value::text("a\nb")), ("long", Value::text("x".repeat(100)))]);
    for v in [list, m] {
        let shown = format_value(&v, Some(Format::Short));
        assert!(
            chars(&shown) <= SHORT_MAX && !shown.contains('\n'),
            "short of {v:?} is {} characters: {shown:?}",
            chars(&shown)
        );
    }
}

/// BUG: `short` of a number that is not finite (`Value::Num` is documented finite, the formatter tests pass one to
/// `usd`) is `infB` / `-infB` while the other formatters show the dash.
#[test]
fn bug_short_of_infinity_is_infb() {
    for n in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        assert_eq!(format_value(&Value::Num(n), Some(Format::Short)), DASH, "{n}");
    }
}

/// BUG: `quote` makes text "one line", but only control characters (Cc) are replaced: U+2028 and U+2029, which many
/// parsers and viewers treat as line ends, pass through.
#[test]
fn bug_quote_lets_unicode_line_separators_through() {
    for sep in ['\u{2028}', '\u{2029}'] {
        let s = Scope::new().with("x", Value::Str(format!("a{sep}b")));
        let Value::Str(q) = Expr::parse("quote(x)").unwrap().eval(&s) else { panic!("quote of text is text") };
        assert!(!q.contains(sep), "quote(\"a{}b\") = {q:?} still holds a line separator", sep.escape_unicode());
    }
}

/// BUG: a template shows text as it is. An escape character (and every C0 control but the ones `split_whitespace`
/// eats) passes through `plain` and `short`, so `{x}` of a tool's output can carry a terminal escape sequence into a
/// row of the interface. `quote` already replaces them.
#[test]
fn bug_templates_pass_terminal_escapes_through() {
    let s = Scope::new().with("x", Value::text("\u{1b}[2Jcleared\u{7}"));
    for t in ["{x}", "{x:short}", "{x:text}"] {
        let shown = Fmt::parse(t).unwrap().render(&s);
        assert!(!shown.chars().any(char::is_control), "{t} shows {shown:?}");
    }
}

/// BUG: money above 2^53 millionths (about $9.0e9) cannot be written exactly: `$` and an amount go through f64, so
/// `$9007199254.740993` (9007199254740993 millionths) reads as ...992, and `==` of two amounts a millionth apart is true.
#[test]
fn bug_money_above_nine_billion_dollars_is_not_exact() {
    let lit = |t: &str| match Expr::parse(t) {
        Ok(Expr::Lit(Value::Usd(m))) => m,
        other => panic!("{t}: {other:?}"),
    };
    assert_eq!(lit("$9007199254.740993"), 9_007_199_254_740_993, "the literal is not what was written");
    assert_eq!(lit("$9007199254.740992"), 9_007_199_254_740_992);
    assert!(
        !Expr::parse("$9007199254.740993 == $9007199254.740992").unwrap().holds(&Scope::new()),
        "two amounts a millionth apart are equal"
    );
    // and printing then reading it again is the identity for every money value a literal can hold
    for m in [9_007_199_254_740_993i64, 123_456_789_012_123_457, 8_999_999_999_999_999_999, 999_999_999_999_999_999] {
        let e = Expr::Lit(Value::Usd(m));
        assert_eq!(Expr::parse(&e.to_string()).as_ref(), Ok(&e), "{m} prints as {e}");
    }
}

/// Money below a billion dollars is exact, millionth by millionth, through print and read.
#[test]
fn money_below_a_billion_dollars_is_exact_through_print_and_read() {
    let mut rng = Rng::new(0x5EED_000D);
    for i in 0..N_VALID {
        let m = (rng.next() % 1_000_000_000_000_000) as i64;
        let text = format!("${}.{:06}", m / 1_000_000, m % 1_000_000);
        let e = Expr::parse(&text).unwrap();
        assert_eq!(e, Expr::Lit(Value::Usd(m)), "[{i}] {text}");
        assert_eq!(Expr::parse(&e.to_string()).as_ref(), Ok(&e), "[{i}] {text} prints as {e}");
    }
}

/// Durations written in any unit mix survive print and read, and the printed text is the largest-unit-first form.
#[test]
fn durations_survive_print_and_read() {
    let mut rng = Rng::new(0x5EED_000E);
    for i in 0..N_VALID {
        let ms = rng.next() % 1_000_000_000_000_000;
        let ms = if i % 3 == 0 { ms >> rng.below(50) } else { ms };
        let e = Expr::Lit(Value::Dur(ms));
        let shown = e.to_string();
        assert_eq!(Expr::parse(&shown).as_ref(), Ok(&e), "[{i}] {ms} prints as {shown}");
    }
}
