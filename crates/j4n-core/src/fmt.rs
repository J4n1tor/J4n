//! Fmt: text with holes, `"{task.cost:usd} of {set.cap:usd}"`. A hole is an [`Expr`] and,
//! after a last `:` outside any quotes, one of a closed set of formatters ([`Format`]); `{{` and `}}` are literal braces.
//! Rendering is total: a hole whose value is missing shows [`DASH`], and a formatter that does not fit the value's kind
//! shows the kind's own words, never an error. (A pointer with a `:` in a key cannot stand in a hole.)

use crate::eval::Env;
use crate::expr::{Expr, ParseError};
use crate::value::{DASH, Value};
use std::fmt;

/// The most characters `short` keeps.
pub const SHORT_MAX: usize = 40;
/// The longest template, prose and holes together (each hole is also an expression of at most
/// [`crate::expr::MAX_LEN`] characters).
pub const MAX_FMT: usize = 20_000;

/// The formatters. Fixed: a pack cannot add one.
/// - `usd` money, or a number counted in dollars: `$12.50`, `$0.0040`, `<$0.0001`
/// - `pct` a number as a share of one: `0.666` is `67%`, `1.04` is `104%`
/// - `n` a number: whole, or two decimals at most
/// - `dur` a duration, or a number counted in seconds: `850ms`, `45s`, `3m05s`, `1h02m`, `2d03h`
/// - `text` the words of the value, as they are (a control character other than a line break or tab shows as `�`)
/// - `short` the first line, whitespace collapsed, at most [`SHORT_MAX`] characters (`…` marks a cut, also of lines
///   left out); a number as `1.2k`, `3M`
/// - `rungs` who is asked, in words: the list `rule:gate, human` is `a rule, then you`; anything else as it is
/// - `runs` a list of keys with each run of three or more that follow each other in short: `1, 2, 3, 4` is `1–4`
/// - `fallback` what stands when no one answers, in words: `pick:add` is `picks add`, `rank` is `the ranking`, `text:…` is the text
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Usd,
    Pct,
    N,
    Dur,
    Text,
    Short,
    Rungs,
    Runs,
    Fallback,
}

impl Format {
    pub const ALL: [Format; 9] = [Format::Usd, Format::Pct, Format::N, Format::Dur, Format::Text, Format::Short, Format::Rungs, Format::Runs, Format::Fallback];

    pub fn word(self) -> &'static str {
        match self {
            Format::Usd => "usd",
            Format::Pct => "pct",
            Format::N => "n",
            Format::Dur => "dur",
            Format::Text => "text",
            Format::Short => "short",
            Format::Rungs => "rungs",
            Format::Runs => "runs",
            Format::Fallback => "fallback",
        }
    }
}

/// A piece of a template.
#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    Lit(String),
    Ref { expr: Expr, format: Option<Format> },
}

/// A parsed template. `Display` prints one that parses back to the same pieces.
#[derive(Debug, Clone, PartialEq)]
pub struct Fmt {
    pieces: Vec<Piece>,
}

/// The index of the `}` that ends a hole starting at `from`; quotes and their escapes are skipped.
fn close(src: &[char], from: usize) -> Option<usize> {
    let (mut quote, mut i) = (None, from);
    while i < src.len() {
        match (quote, src[i]) {
            (Some(_), '\\') => i += 1,
            (Some(q), c) if c == q => quote = None,
            (None, c @ ('\'' | '"')) => quote = Some(c),
            (None, '}') => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

/// The last `:` outside quotes.
fn last_colon(src: &[char]) -> Option<usize> {
    let (mut quote, mut found, mut i) = (None, None, 0);
    while i < src.len() {
        match (quote, src[i]) {
            (Some(_), '\\') => i += 1,
            (Some(q), c) if c == q => quote = None,
            (None, c @ ('\'' | '"')) => quote = Some(c),
            (None, ':') => found = Some(i),
            _ => {}
        }
        i += 1;
    }
    found
}

fn hole(inner: &[char]) -> Result<Piece, ParseError> {
    let (expr_part, format) = match last_colon(inner) {
        Some(k) => {
            let word: String = inner[k + 1..].iter().collect();
            let Some(format) = Format::ALL.into_iter().find(|f| f.word() == word.trim()) else {
                let all: Vec<&str> = Format::ALL.iter().map(|f| f.word()).collect();
                return Err(ParseError::new(
                    k + 1,
                    format!("no formatter `{}` (they are {})", word.trim(), all.join(", ")),
                ));
            };
            (&inner[..k], Some(format))
        }
        None => (inner, None),
    };
    let text: String = expr_part.iter().collect();
    if text.trim().is_empty() {
        return Err(ParseError::new(0, "an empty `{}` — name a value, like `{task.cost:usd}`"));
    }
    Ok(Piece::Ref { expr: Expr::parse(&text)?, format })
}

impl Fmt {
    pub fn parse(text: &str) -> Result<Fmt, ParseError> {
        let src: Vec<char> = text.chars().collect();
        if src.len() > MAX_FMT {
            return Err(ParseError::new(MAX_FMT, format!("longer than {MAX_FMT} characters")));
        }
        let (mut pieces, mut lit, mut i) = (Vec::new(), String::new(), 0);
        while i < src.len() {
            match (src[i], src.get(i + 1)) {
                ('{', Some('{')) | ('}', Some('}')) => {
                    lit.push(src[i]);
                    i += 2;
                }
                ('{', _) => {
                    let Some(end) = close(&src, i + 1) else {
                        return Err(ParseError::new(i, "a `{` is never closed (write `{{` for a literal brace)"));
                    };
                    if !lit.is_empty() {
                        pieces.push(Piece::Lit(std::mem::take(&mut lit)));
                    }
                    pieces.push(hole(&src[i + 1..end]).map_err(|e| e.shift(i + 1))?);
                    i = end + 1;
                }
                ('}', _) => {
                    return Err(ParseError::new(i, "a `}` with no `{` before it (write `}}` for a literal brace)"));
                }
                (c, _) => {
                    lit.push(c);
                    i += 1;
                }
            }
        }
        if !lit.is_empty() {
            pieces.push(Piece::Lit(lit));
        }
        Ok(Fmt { pieces })
    }

    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    /// The text with every hole filled.
    pub fn render(&self, env: &dyn Env) -> String {
        self.pieces
            .iter()
            .map(|p| match p {
                Piece::Lit(l) => l.clone(),
                Piece::Ref { expr, format } => format_value(&expr.eval(env), *format),
            })
            .collect()
    }

    /// The text with every hole filled as a screen shows it: a bare yes or no is `on` or `off`, as the strings of a setting read.
    pub fn render_shown(&self, env: &dyn Env) -> String {
        self.pieces
            .iter()
            .map(|p| match p {
                Piece::Lit(l) => l.clone(),
                Piece::Ref { expr, format: None } if let Value::Bool(b) = expr.eval(env) => (if b { "on" } else { "off" }).to_string(),
                Piece::Ref { expr, format } => format_value(&expr.eval(env), *format),
            })
            .collect()
    }

    /// What [`Expr::unquoted_reads`] finds in the holes.
    pub fn unquoted_reads(&self) -> Vec<Vec<String>> {
        self.pieces.iter().filter_map(|p| if let Piece::Ref { expr, .. } = p { Some(expr.unquoted_reads()) } else { None }).flatten().collect()
    }

    /// What [`Expr::check`] finds in the holes.
    pub fn check(&self, roots: Option<&[&str]>) -> Vec<String> {
        self.pieces
            .iter()
            .filter_map(|p| if let Piece::Ref { expr, .. } = p { Some(expr.check(roots)) } else { None })
            .flatten()
            .collect()
    }
}

impl fmt::Display for Fmt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for piece in &self.pieces {
            match piece {
                Piece::Lit(text) => {
                    for c in text.chars() {
                        match c {
                            '{' | '}' => write!(f, "{c}{c}")?,
                            c => write!(f, "{c}")?,
                        }
                    }
                }
                Piece::Ref { expr, format } => {
                    write!(f, "{{{expr}")?;
                    if let Some(x) = format {
                        write!(f, ":{}", x.word())?;
                    }
                    f.write_str("}")?;
                }
            }
        }
        Ok(())
    }
}

// ── values to words ──────────────────────────────────────────────────────────────────────────

fn number(n: f64) -> String {
    if !n.is_finite() {
        return DASH.to_string();
    }
    if n.abs() >= 1e21 {
        return format!("{n:e}");
    }
    if n.fract() == 0.0 {
        return if n == 0.0 { "0".to_string() } else { format!("{n:.0}") };
    }
    let text = format!("{n:.2}");
    match text.trim_end_matches('0').trim_end_matches('.') {
        "-0" => "0".to_string(),
        trimmed => trimmed.to_string(),
    }
}

fn usd(micros: i64) -> String {
    let (sign, abs) = (if micros < 0 { "-" } else { "" }, micros.unsigned_abs());
    match abs {
        0 => "$0.00".to_string(),
        5_000.. => format!("{sign}${}.{:02}", (abs + 5_000) / 1_000_000, (abs + 5_000) / 10_000 % 100),
        _ => match (abs + 50) / 100 {
            0 => format!("{sign}<$0.0001"),
            n => format!("{sign}$0.{n:04}"),
        },
    }
}

fn pct(n: f64) -> String {
    if n.is_finite() && n.abs() < 1e12 { format!("{}%", (n * 100.0).round() as i64) } else { DASH.to_string() }
}

/// Milliseconds as the two biggest units that matter: `850ms`, `45s`, `3m05s`, `1h02m`, `2d03h`.
pub fn duration(ms: u64) -> String {
    let s = ms / 1000;
    match ms {
        0 => "0s".to_string(),
        1..=999 => format!("{ms}ms"),
        _ => match s {
            0..=59 => format!("{s}s"),
            60..=3599 => format!("{}m{:02}s", s / 60, s % 60),
            3600..=86_399 => format!("{}h{:02}m", s / 3600, s % 3600 / 60),
            _ => format!("{}d{:02}h", s / 86_400, s % 86_400 / 3600),
        },
    }
}

fn plain(v: &Value) -> String {
    match v {
        Value::Missing => DASH.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Num(n) => number(*n),
        Value::Str(s) if s.is_empty() => DASH.to_string(),
        // a control character in a value (a terminal escape, a carriage return) never reaches a screen
        Value::Str(s) => {
            s.chars().map(|c| if c.is_control() && !"\n\r\t".contains(c) { '\u{fffd}' } else { c }).collect()
        }
        Value::Dur(ms) => duration(*ms),
        Value::Usd(m) => usd(*m),
        Value::List(l) if l.is_empty() => DASH.to_string(),
        Value::List(l) => l.iter().map(plain).collect::<Vec<_>>().join(", "),
        Value::Map(m) if m.is_empty() => DASH.to_string(),
        Value::Map(m) => m.iter().map(|(k, v)| format!("{k}={}", plain(v))).collect::<Vec<_>>().join(", "),
    }
}

/// A number as `1.2k`, `3M`, `2B`, `4T`; below a thousand as itself, from 1e15 as its digits.
fn compact(n: f64) -> String {
    const UNITS: [(f64, &str); 4] = [(1e3, "k"), (1e6, "M"), (1e9, "B"), (1e12, "T")];
    let a = n.abs();
    if a >= 1e15 {
        return number(n);
    }
    let Some(mut i) = UNITS.iter().rposition(|(unit, _)| a >= *unit) else { return number(n) };
    let tenths = |i: usize| (a / UNITS[i].0 * 10.0).round() / 10.0;
    let mut v = tenths(i);
    if v >= 1000.0 && i + 1 < UNITS.len() {
        i += 1;
        v = tenths(i);
    }
    let digits = if v.fract() == 0.0 { format!("{}", v as i64) } else { format!("{v:.1}") };
    format!("{}{digits}{}", if n < 0.0 { "-" } else { "" }, UNITS[i].1)
}

/// The rungs of a chain in words, a repeat said once and `you` last: `[rule:fold, decider:jev?min_conf=0.6, human]` is `a rule, a decider, then you`.
fn rungs(l: &[Value]) -> String {
    let mut words: Vec<String> = Vec::new();
    for r in l {
        let rung = plain(r);
        let w = match rung.split(['?', ':']).next().unwrap_or_default() {
            "rule" => "a rule",
            "decider" => "a decider",
            "human" => "you",
            "model" => "a model",
            "rank" => "a ranking",
            other => other,
        };
        if words.last().map(String::as_str) != Some(w) {
            words.push(w.to_string());
        }
    }
    match words.split_last() {
        Some((you, rest)) if you == "you" && !rest.is_empty() => format!("{}, then you", rest.join(", ")),
        _ => words.join(", "),
    }
}

/// A list of keys with each run of three or more single characters that follow each other said as its ends: `1, 2, …, 9` is `1–9`.
fn runs(l: &[Value]) -> String {
    let items: Vec<String> = l.iter().map(plain).collect();
    let one = |at: usize| {
        let mut c = items.get(at)?.chars();
        c.next().filter(|_| c.next().is_none())
    };
    let (mut out, mut i) = (Vec::new(), 0);
    while i < items.len() {
        let mut j = i;
        while one(j).zip(one(j + 1)).is_some_and(|(a, b)| b as u32 == a as u32 + 1) {
            j += 1;
        }
        if j >= i + 2 { out.push(format!("{}–{}", items[i], items[j])) } else { out.extend(items[i..=j].iter().cloned()) }
        i = j + 1;
    }
    out.join(", ")
}

/// What stands when no one answers, in words: `pick:add` is `picks add`, `text:No one…` is the text, `rank` is `the ranking`, `safe` is `the safe answer`.
fn fallback(s: &str) -> String {
    match s.split_once(':') {
        Some(("pick", id)) => format!("picks {id}"),
        Some(("text", words)) => words.to_string(),
        _ => match s {
            "rank" => "the ranking".to_string(),
            "safe" => "the safe answer".to_string(),
            other => other.to_string(),
        },
    }
}

fn short(v: &Value) -> String {
    if let Value::Num(n) = v {
        return compact(*n);
    }
    let text = plain(v);
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let Some(first) = lines.next() else { return DASH.to_string() };
    let one = first.split_whitespace().collect::<Vec<_>>().join(" ");
    let marked = lines.next().is_some() || one.chars().count() > SHORT_MAX;
    one.chars().take(if marked { SHORT_MAX - 1 } else { SHORT_MAX }).chain(marked.then_some('…')).collect()
}

/// A value through one formatter. Missing is [`DASH`]; a formatter that does not fit the value's kind is the kind's own
/// words (`usd`, `pct` and `dur` also take a plain number; `n` and `text` change nothing a plain value does not).
pub fn format_value(v: &Value, format: Option<Format>) -> String {
    match (format, v) {
        (Some(Format::Usd), Value::Num(n)) => match Value::usd(*n) {
            Value::Usd(m) => usd(m),
            _ => DASH.to_string(),
        },
        (Some(Format::Pct), Value::Num(n)) => pct(*n),
        (Some(Format::Dur), Value::Num(n)) if n.is_finite() => duration((n.max(0.0) * 1000.0) as u64),
        (Some(Format::Dur), Value::Num(_)) => DASH.to_string(),
        (Some(Format::Short), _) => short(v),
        (Some(Format::Rungs), Value::List(l)) if !l.is_empty() => rungs(l),
        (Some(Format::Runs), Value::List(l)) if !l.is_empty() => runs(l),
        (Some(Format::Fallback), Value::Str(s)) if !s.is_empty() => fallback(s),
        _ => plain(v),
    }
}
