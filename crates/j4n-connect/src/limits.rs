//! A program's own usage limit, read into the readings claude's windows give: how much is used, and when it ends.
//!
//! A connector's `[[connector.limits]]` rows say where the program tells it. A row has one of four readers:
//! - `line`: the shape of a line of its output, with holes (`[{used}/{cap} · resets in {reset}]`); the holes are `{used}`, `{left}`,
//!   `{cap}`, `{percent}`, `{reset}` (a time to wait, `4m10s`), `{unit}`, `{window}`, `{n}` (any number) and `{*}` (anything);
//! - the names of keys (`used`, `cap`, `reset_s` …) or `/pointers`, looked up in any JSON object a line holds, whatever frames it;
//! - `command`: a subcommand run after each turn (`usage`), its output read as the two above;
//! - `path`: a file the program keeps its count in, relative to the project, read after each turn as JSON.
//!
//! A program that refuses for its limit is read the same way with no row at all: a run that fails with the words of a limit and says
//! how long to wait is a refusal, and the wait is the program's own. What is learned of a row from the program's output is
//! [`learn`]; where a limit is read from a run is [`Gauge`].

use crate::decode;
use crate::proc;
use j4n_core::doer::Limit;
use j4n_core::place::Launch;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long a subcommand that reports the limit is given.
const ASK_SECS: u64 = 8;
/// What a refusal that says no time waits: as long as the failure table's own default.
const WAIT_S: i64 = 1800;
/// What a reading that says no time is taken to last.
const HOUR_S: i64 = 3600;

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    /// The shape of a line that says it, with holes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    /// The key (or `/pointer`) in a JSON object that holds each number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cap: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<String>,
    /// Seconds (or a time like `4m10s`) until the window ends, and the time it ends at (seconds since 1970).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// The name of the window as the gauge says it; left out, the unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<String>,
    /// A subcommand of the program that says it, run after each turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub command: Vec<String>,
    /// A file in the project that says it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl Row {
    fn keyed(&self) -> bool {
        [&self.used, &self.left, &self.cap, &self.percent, &self.reset, &self.reset_at].iter().any(|k| k.is_some())
    }

    /// Why a row cannot be used, if it cannot.
    pub fn check(&self) -> Result<(), String> {
        if self.line.is_none() && !self.keyed() {
            return Err("a row of `limits` says a `line` or the keys (`used`, `left`, `cap`, `percent`, `reset`, `reset_at`)".into());
        }
        if let Some(line) = &self.line {
            Pattern::parse(line)?;
        }
        if self.command.iter().any(|w| w.is_empty() || w.chars().any(char::is_whitespace)) {
            return Err("`command` is the words of a subcommand, one to a word".into());
        }
        if let Some(p) = &self.path
            && (p.is_empty() || Path::new(p).is_absolute() || Path::new(p).components().any(|c| matches!(c, std::path::Component::ParentDir)))
        {
            return Err("`path` is a file inside the project: relative, with no `..`".into());
        }
        Ok(())
    }
}

/// What the limit rows of a connector say, or why one cannot be used.
pub fn check(rows: &[Row]) -> Result<(), String> {
    rows.iter().enumerate().try_for_each(|(i, r)| r.check().map_err(|e| format!("limits {}: {e}", i + 1)))
}

// ---- what a reading is ----

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reading {
    pub used: Option<f64>,
    pub left: Option<f64>,
    pub cap: Option<f64>,
    /// 0 to 100.
    pub percent: Option<f64>,
    pub reset_s: Option<f64>,
    pub reset_at: Option<f64>,
    pub unit: Option<String>,
    pub window: Option<String>,
    /// The program said, in words a refusal has, that it refused.
    pub refused: bool,
}

impl Reading {
    fn gauge(&self) -> bool {
        self.percent.is_some() || (self.cap.is_some() && (self.used.is_some() || self.left.is_some())) || (self.used.is_some() && self.left.is_some())
    }

    fn any(&self) -> bool {
        self.gauge() || self.reset_s.is_some() || self.reset_at.is_some()
    }

    fn fill(&mut self, other: &Reading) {
        macro_rules! take {
            ($($f:ident),*) => {$( if self.$f.is_none() { self.$f = other.$f.clone(); } )*};
        }
        take!(used, left, cap, percent, reset_s, reset_at, unit, window);
        self.refused |= other.refused;
    }

    /// How much is used, 0 to 1.
    fn fraction(&self) -> Option<f64> {
        let f = match (self.percent, self.used, self.left, self.cap) {
            (Some(p), ..) => p / 100.0,
            (_, Some(u), _, Some(c)) if c > 0.0 => u / c,
            (_, _, Some(l), Some(c)) if c > 0.0 => 1.0 - l / c,
            (_, Some(u), Some(l), _) if u + l > 0.0 => u / (u + l),
            _ => return None,
        };
        Some(f.clamp(0.0, 1.0))
    }

    /// When the window ends, seconds since 1970.
    fn resets_at(&self, now_s: i64) -> Option<i64> {
        match (self.reset_at, self.reset_s) {
            (Some(at), _) if at > 1e12 => Some((at / 1000.0) as i64),
            (Some(at), _) if at > 1e9 => Some(at as i64),
            (_, Some(s)) if s >= 0.0 => Some(now_s + s.ceil() as i64),
            _ => None,
        }
    }
}

// ---- numbers and times in words ----

/// A time in a program's words, in seconds: `4m10s`, `1h 2m`, `58s`, `2 minutes`, `90` (seconds).
pub fn time(text: &str) -> Option<f64> {
    let text = text.trim();
    let (secs, used) = time_at(text, true)?;
    (used == text.len()).then_some(secs)
}

fn scale(word: &str) -> Option<f64> {
    Some(match word {
        "ms" => 0.001,
        "s" | "sec" | "secs" | "second" | "seconds" => 1.0,
        "m" | "min" | "mins" | "minute" | "minutes" => 60.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3600.0,
        "d" | "day" | "days" => 86400.0,
        _ => return None,
    })
}

/// The time at the start of `text`: its seconds and the bytes it takes. A number with no unit is seconds only where `bare` says so.
fn time_at(text: &str, bare: bool) -> Option<(f64, usize)> {
    let (mut total, mut at, mut any) = (0.0, 0, false);
    while let Some((n, used)) = number_at(&text[at..]) {
        let after = &text[at + used..];
        let gap = after.len() - after.trim_start_matches(' ').len();
        let word: String = after[gap..].chars().take_while(char::is_ascii_alphabetic).collect();
        let Some(scale) = scale(&word) else {
            let plain = !after[gap..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '%' || c == '/');
            return if any { Some((total, at)) } else { (bare && plain).then_some((n, used)) };
        };
        total += n * scale;
        at += used + gap + word.len();
        any = true;
        // a part follows at once (`4m10s`) or after a space (`1h 2m`)
        let next = text[at..].trim_start_matches(' ');
        if !next.starts_with(|c: char| c.is_ascii_digit()) {
            break;
        }
        at = text.len() - next.len();
    }
    any.then_some((total, at))
}

/// The number at the start of `text`: its value and the bytes it takes.
fn number_at(text: &str) -> Option<(f64, usize)> {
    let b = text.as_bytes();
    let mut i = usize::from(b.first() == Some(&b'-'));
    let from = i;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    if i == from {
        return None;
    }
    if b.get(i) == Some(&b'.') && b.get(i + 1).is_some_and(u8::is_ascii_digit) {
        i += 1;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
    }
    text[..i].parse().ok().map(|n| (n, i))
}

// ---- the shape of a line ----

#[derive(Debug, Clone, PartialEq)]
enum Piece {
    Lit(String),
    Hole(String),
}

/// A line shape: words that must be there and holes for what varies. Spaces match any run of spaces.
#[derive(Debug, Clone, PartialEq)]
pub struct Pattern(Vec<Piece>);

const HOLES: [&str; 9] = ["used", "left", "cap", "percent", "reset", "unit", "window", "n", "*"];

fn squeeze(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Pattern {
    pub fn parse(text: &str) -> Result<Pattern, String> {
        let (mut pieces, mut lit, mut rest) = (Vec::new(), String::new(), text);
        while let Some(open) = rest.find('{') {
            let Some(close) = rest[open..].find('}') else { return Err(format!("`line` has a `{{` that is not closed: {text}")) };
            let hole = &rest[open + 1..open + close];
            if !HOLES.contains(&hole) {
                return Err(format!("`line` has the hole `{{{hole}}}`, and the holes are {}", HOLES.map(|h| format!("{{{h}}}")).join(" ")));
            }
            lit.push_str(&rest[..open]);
            if !lit.is_empty() {
                pieces.push(Piece::Lit(squeeze_edges(&std::mem::take(&mut lit))));
            }
            pieces.push(Piece::Hole(hole.into()));
            rest = &rest[open + close + 1..];
        }
        lit.push_str(rest);
        if !lit.is_empty() {
            pieces.push(Piece::Lit(squeeze_edges(&lit)));
        }
        if !pieces.iter().any(|p| matches!(p, Piece::Hole(h) if h != "n" && h != "*")) {
            return Err("`line` has no hole that takes a number".into());
        }
        Ok(Pattern(pieces))
    }

    /// What the first place `text` fits the shape says.
    pub fn find(&self, text: &str) -> Option<Reading> {
        let text = squeeze(text);
        (0..=text.len()).filter(|i| text.is_char_boundary(*i)).find_map(|start| self.at(&text[start..], &self.0, Reading::default()))
    }

    fn at(&self, text: &str, pieces: &[Piece], mut got: Reading) -> Option<Reading> {
        let Some((first, rest)) = pieces.split_first() else { return Some(got) };
        match first {
            Piece::Lit(l) => self.at(text.strip_prefix(l.as_str())?, rest, got),
            Piece::Hole(h) if h == "*" => {
                (0..=text.len()).filter(|i| text.is_char_boundary(*i)).find_map(|i| self.at(&text[i..], rest, got.clone()))
            }
            Piece::Hole(h) => {
                let used = match h.as_str() {
                    "reset" => {
                        let (s, n) = time_at(text, true)?;
                        got.reset_s = Some(s);
                        n
                    }
                    "unit" | "window" => {
                        let word: String = text.chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-').collect();
                        if word.is_empty() {
                            return None;
                        }
                        match h.as_str() {
                            "unit" => got.unit = Some(word.clone()),
                            _ => got.window = Some(word.clone()),
                        }
                        word.len()
                    }
                    _ => {
                        let (n, used) = number_at(text)?;
                        match h.as_str() {
                            "used" => got.used = Some(n),
                            "left" => got.left = Some(n),
                            "cap" => got.cap = Some(n),
                            "percent" => got.percent = Some(n),
                            _ => {}
                        }
                        used
                    }
                };
                self.at(&text[used..], rest, got)
            }
        }
    }
}

fn squeeze_edges(lit: &str) -> String {
    let words = squeeze(lit);
    if words.is_empty() {
        return " ".into();
    }
    let lead = if lit.starts_with(char::is_whitespace) { " " } else { "" };
    let trail = if lit.ends_with(char::is_whitespace) { " " } else { "" };
    format!("{lead}{words}{trail}")
}

// ---- JSON wherever a line holds it ----

/// The JSON objects a line holds, whatever it is wrapped in: the line itself, a payload after a prefix (`data: {…}`, a length), a value of
/// `k=v` pairs, or text with its quotes escaped.
pub fn objects(line: &str) -> Vec<Json> {
    let mut found = Vec::new();
    for text in [line.to_string(), line.replace("&quot;", "\"").replace("\\\"", "\"")] {
        let mut from = 0;
        while let Some(i) = text[from..].find('{') {
            let at = from + i;
            let mut stream = serde_json::Deserializer::from_str(&text[at..]).into_iter::<Json>();
            match stream.next() {
                Some(Ok(v @ Json::Object(_))) => {
                    from = at + stream.byte_offset();
                    found.push(v);
                }
                _ => from = at + 1,
            }
        }
        if !found.is_empty() {
            break;
        }
    }
    found
}

/// The number under a key, anywhere in `doc` (a text that is itself JSON is looked into): a number, or a text that is one.
fn find_key(doc: &Json, key: &str, depth: u8) -> Option<f64> {
    let as_number = |v: &Json| v.as_f64().or_else(|| v.as_str().and_then(time));
    if let Some(p) = key.strip_prefix('/') {
        return doc.pointer(&format!("/{p}")).and_then(as_number);
    }
    match doc {
        Json::Object(m) => {
            if let Some(n) = m.get(key).and_then(as_number) {
                return Some(n);
            }
            m.values().find_map(|v| find_key(v, key, depth))
        }
        Json::Array(a) => a.iter().find_map(|v| find_key(v, key, depth)),
        Json::String(s) if depth < 3 && s.trim_start().starts_with('{') => objects(s).iter().find_map(|o| find_key(o, key, depth + 1)),
        _ => None,
    }
}

/// Keys whose value, true, says a program refused for its limit.
const REFUSAL_KEYS: [&str; 5] = ["exceeded", "limited", "rate_limited", "over_limit", "quota_exceeded"];

fn refused_in(doc: &Json) -> bool {
    match doc {
        Json::Object(m) => m.iter().any(|(k, v)| (REFUSAL_KEYS.contains(&k.as_str()) && v.as_bool() == Some(true)) || refused_in(v)),
        Json::Array(a) => a.iter().any(refused_in),
        _ => false,
    }
}

impl Row {
    fn keys(&self, doc: &Json) -> Reading {
        let get = |k: &Option<String>| k.as_deref().and_then(|k| find_key(doc, k, 0));
        Reading { used: get(&self.used), left: get(&self.left), cap: get(&self.cap), percent: get(&self.percent), reset_s: get(&self.reset), reset_at: get(&self.reset_at), refused: refused_in(doc), ..Reading::default() }
    }

    /// What one line says, by this row.
    fn read(&self, line: &str) -> Option<Reading> {
        let mut r = match &self.line {
            Some(l) => Pattern::parse(l).ok()?.find(line)?,
            None => objects(line).iter().map(|o| self.keys(o)).find(Reading::any)?,
        };
        r.unit = r.unit.or_else(|| self.unit.clone());
        r.window = r.window.or_else(|| self.window.clone());
        Some(r)
    }
}

// ---- a run ----

/// Words that say a program refused for its limit.
const REFUSAL_WORDS: [&str; 12] = ["rate limit", "rate-limit", "ratelimit", "rate_limit", "quota", "usage limit", "limit reached", "limit exceeded", "too many requests", "429", "exhausted", "retry in"];

/// Whether `text` says a refusal: the words of one.
pub fn refusal_words(text: &str) -> bool {
    let low = text.to_lowercase();
    REFUSAL_WORDS.iter().any(|w| low.contains(w)) || low.contains("limit") && (low.contains("exceed") || low.contains("reached") || low.contains("retry"))
}

/// The time a refusal says to wait, from its words: the time after `reset(s) in`, `retry in`, `retry after`, `try again in`, `wait`, and a time
/// with its unit after `in` or `after`.
fn waits(text: &str) -> Option<f64> {
    let low = text.to_lowercase();
    let after = |words: &[&str], bare: bool| {
        words.iter().find_map(|w| low.match_indices(w).find_map(|(i, _)| time_at(text[i + w.len()..].trim_start_matches([' ', ':', '=']), bare).map(|(s, _)| s)))
    };
    after(&["resets in", "reset in", "retry in", "retry after", "retry-after", "try again in", "again in", "wait ", "resets after", "reset_s", "retry_after"], true).or_else(|| after(&[" in ", " after "], false))
}

/// The numbers `used/cap` in words: two numbers around a `/`, the first no more than the second.
fn pair(text: &str) -> Option<(f64, f64)> {
    text.match_indices('/').find_map(|(i, _)| {
        let before = &text[..i];
        let start = before.rfind(|c: char| !(c.is_ascii_digit() || c == '.')).map_or(0, |p| p + 1);
        let used: f64 = before[start..].parse().ok()?;
        let (cap, _) = number_at(&text[i + 1..])?;
        (cap > 0.0 && used <= cap).then_some((used, cap))
    })
}

/// What a refusal's words say with no row: the pair `used/cap`, the keys of an object, and the time to wait.
fn refusal_reading(text: &str) -> Reading {
    let mut r = Reading { reset_s: waits(text), refused: true, ..Reading::default() };
    if let Some((used, cap)) = pair(text) {
        (r.used, r.cap) = (Some(used), Some(cap));
    }
    let keys = Row { used: Some("used".into()), cap: Some("cap".into()), left: Some("left".into()), reset: Some("reset_s".into()), ..Row::default() };
    objects(text).iter().for_each(|o| r.fill(&keys.keys(o)));
    r
}

/// What the run said of the limit, to be told to the loop.
#[derive(Debug, Default)]
pub struct Told {
    pub limits: Vec<Limit>,
    /// The seconds since 1970 a refusal says the program works again.
    pub refused_until: Option<i64>,
    /// What the program said when it refused, when the failure does not say it.
    pub said: Option<String>,
}

/// The place a limit is read from in a run of a connector: the rows, and what a run of it has said so far. One is made for each run;
/// `last` is shared by the runs of one connector, so a refusal that says no number keeps the fraction the turn before it said.
pub struct Gauge {
    rows: Arc<Vec<Row>>,
    got: Reading,
    last: Arc<Mutex<Option<f64>>>,
    /// The subcommands and files read after the run: where they run, and the folder the files are in.
    asks: Vec<(Row, Option<Launch>)>,
    site: Option<PathBuf>,
    /// The lines of the output that said words of a refusal: a program that is read as JSON leaves the words of its refusal out of the failure.
    refusals: Vec<String>,
}

impl Gauge {
    pub fn new(rows: Arc<Vec<Row>>, last: Arc<Mutex<Option<f64>>>) -> Gauge {
        Gauge { rows, got: Reading::default(), last, asks: Vec::new(), site: None, refusals: Vec::new() }
    }

    /// A subcommand row is run with `launch`; a file row is read in `site`.
    pub fn ask_with(&mut self, launches: Vec<Option<Launch>>, site: PathBuf) {
        let rows = self.rows.clone();
        self.asks = rows.iter().zip(launches).filter(|(r, _)| !r.command.is_empty() || r.path.is_some()).map(|(r, l)| (r.clone(), l)).collect();
        self.site = Some(site);
    }

    /// One line of the output.
    pub fn line(&mut self, line: &str) {
        let line = decode::clean(line);
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        if line.contains('{') {
            self.got.refused |= objects(line).iter().any(refused_in);
        }
        if refusal_words(line) && self.refusals.len() < 4 {
            self.refusals.push(line.chars().take(200).collect());
        }
        for row in self.rows.iter().filter(|r| r.command.is_empty() && r.path.is_none()) {
            if let Some(r) = row.read(line) {
                self.got.fill(&r);
                return;
            }
        }
    }

    /// What was written to stderr.
    pub fn stderr(&mut self, text: &str) {
        text.lines().for_each(|l| self.line(l));
    }

    /// The subcommands and files that say the limit, after a run.
    pub fn after(&mut self) {
        for (row, launch) in self.asks.clone() {
            let text = match (&row.path, launch) {
                (Some(p), _) => self.site.as_ref().and_then(|s| std::fs::read_to_string(s.join(p)).ok()),
                (None, Some(l)) => ask(&l),
                _ => None,
            };
            for line in text.iter().flat_map(|t| std::iter::once(t.as_str()).chain(t.lines())) {
                if let Some(r) = row.read(decode::clean(line).trim()) {
                    self.got.fill(&r);
                    break;
                }
            }
        }
    }

    /// The limit as the run left it. `failed` is how the run ended, when it failed: with the words of a limit it is a refusal.
    pub fn told(&mut self, failed: Option<&str>, now_s: i64) -> Told {
        let mut got = std::mem::take(&mut self.got);
        let said = self.refusals.drain(..).next_back().filter(|_| failed.is_some());
        let refused = got.refused || failed.is_some_and(refusal_words) || said.is_some();
        if let Some(text) = failed.filter(|_| refused) {
            got.fill(&refusal_reading(&format!("{text} {}", said.as_deref().unwrap_or_default())));
        }
        let mut last = self.last.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let fraction = got.fraction().or(if refused { Some(last.unwrap_or(1.0)) } else { None });
        let Some(fraction) = fraction else { return Told::default() };
        let until = got.resets_at(now_s);
        let rejected = refused || fraction >= 1.0;
        *last = Some(fraction);
        let window = got.window.clone().or(got.unit.clone()).map(|w| w.to_lowercase().replace(|c: char| !c.is_ascii_alphanumeric(), "_")).unwrap_or_else(|| "usage".into());
        let resets_at = until.unwrap_or(now_s + if refused { WAIT_S } else { HOUR_S });
        let limit = Limit { window, utilization: (fraction * 100.0).round() / 100.0, resets_at, status: if rejected { "rejected".into() } else { "allowed".into() } };
        Told { limits: vec![limit], refused_until: refused.then_some(resets_at), said: said.filter(|s| !failed.is_some_and(|f| f.contains(s.as_str()))) }
    }
}

/// The failure a refusal is, in the words the failure table reads: what the program said, and when it works again.
pub fn refused_message(said: &str, until: i64) -> String {
    let said = if refusal_words(said) { said.to_string() } else { format!("usage limit reached: {said}") };
    format!("{said} (resets at {until})")
}

/// Run a subcommand that reports the limit and give what it printed, within a short time.
fn ask(launch: &Launch) -> Option<String> {
    let (text, exit) = proc::capture(launch, Duration::from_secs(ASK_SECS)).ok()?;
    (!exit.killed).then(|| format!("{text}{}", exit.tail))
}

// ---- learning a row ----

const USED: [&str; 4] = ["used", "spent", "consumed", "usage"];
const LEFT: [&str; 6] = ["left", "remaining", "remain", "available", "avail", "balance"];
const CAP: [&str; 7] = ["cap", "limit", "quota", "max", "total", "allowance", "capacity"];
const RESET: [&str; 8] = ["reset_s", "reset", "resets_in", "reset_in", "retry_after", "retry_in", "reset_secs", "ttl"];
const RESET_AT: [&str; 4] = ["resets_at", "reset_at", "reset_time", "expires_at"];
const PERCENT: [&str; 3] = ["percent", "pct", "percentage"];

/// Every key of `doc` that holds a number, at any depth (a text that is JSON is looked into).
fn numeric_keys(doc: &Json, depth: u8, out: &mut Vec<String>) {
    match doc {
        Json::Object(m) => {
            for (k, v) in m {
                if v.is_number() || v.as_str().is_some_and(|s| time(s).is_some()) {
                    out.push(k.clone());
                }
                numeric_keys(v, depth, out);
            }
        }
        Json::Array(a) => a.iter().for_each(|v| numeric_keys(v, depth, out)),
        Json::String(s) if depth < 3 && s.trim_start().starts_with('{') => objects(s).iter().for_each(|o| numeric_keys(o, depth + 1, out)),
        _ => {}
    }
}

/// The names a JSON object that reports a limit uses, as a row: a count of the whole and what is used or left of it, or a percent.
fn row_of_keys(obj: &Json) -> Option<Row> {
    let mut keys = Vec::new();
    numeric_keys(obj, 0, &mut keys);
    let pick = |names: &[&str]| names.iter().find(|n| keys.iter().any(|k| k == *n)).map(|n| (*n).to_string());
    let row = Row { used: pick(&USED), left: pick(&LEFT), cap: pick(&CAP), percent: pick(&PERCENT), reset: pick(&RESET), reset_at: pick(&RESET_AT), unit: find_text(obj, "unit"), ..Row::default() };
    let has = |k: &Option<String>| k.is_some();
    ((has(&row.cap) && (has(&row.used) || has(&row.left))) || has(&row.percent)).then_some(row)
}

fn find_text(doc: &Json, key: &str) -> Option<String> {
    match doc {
        Json::Object(m) => m.get(key).and_then(Json::as_str).map(String::from).or_else(|| m.values().find_map(|v| find_text(v, key))),
        Json::Array(a) => a.iter().find_map(|v| find_text(v, key)),
        _ => None,
    }
}

/// Words that are not a unit.
const NOT_UNITS: [&str; 14] = ["of", "used", "left", "remaining", "in", "quota", "usage", "limit", "window", "resets", "reset", "the", "is", "out"];

/// A line that says a gauge in words, as a shape: `[req 3/5 · resets in 6m58s]` is `[req {used}/{cap} · resets in {reset}]`.
fn row_of_line(line: &str) -> Option<Row> {
    if line.contains(['{', '}']) {
        return None;
    }
    // the numbers and the times of the line: where each is, and whether it is a time to wait
    let mut spans: Vec<(usize, usize, bool)> = Vec::new();
    let mut at = 0;
    while at < line.len() {
        let rest = &line[at..];
        let starts = rest.starts_with(|c: char| c.is_ascii_digit()) && !line[..at].ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_');
        if starts {
            let near = line[..at].to_lowercase();
            let near = near.rsplit([',', ';']).next().unwrap_or_default().to_string();
            let waiting = ["in ", "reset", "retry", "after", "until", "wait"].iter().any(|w| near.contains(w)) && !near.trim_end().ends_with("of");
            if let Some((_, n)) = time_at(rest, false).filter(|_| waiting) {
                spans.push((at, at + n, true));
                at += n;
                continue;
            }
            if let Some((_, n)) = number_at(rest) {
                spans.push((at, at + n, false));
                at += n;
                continue;
            }
        }
        at += rest.chars().next().map_or(1, char::len_utf8);
    }
    let nums: Vec<usize> = spans.iter().enumerate().filter(|(_, s)| !s.2).map(|(i, _)| i).collect();
    let between = |a: usize, b: usize| squeeze(&line[spans[a].1..spans[b].0]);
    let mut roles: Vec<(usize, &str)> = Vec::new();
    if let Some(w) = nums.windows(2).find(|w| matches!(between(w[0], w[1]).as_str(), "/" | "of" | "out of")) {
        roles = vec![(w[0], "used"), (w[1], "cap")];
        let after = line[spans[w[0]].1..].trim_start().to_lowercase();
        if ["left", "remaining", "available"].iter().any(|k| after.starts_with(k)) {
            roles[0].1 = "left";
        }
    } else if let Some(&i) = nums.iter().find(|&&i| line[spans[i].1..].starts_with('%')) {
        roles = vec![(i, "percent")];
    }
    let (first, _) = *roles.first()?;
    // a pair of numbers is a limit's only when the line says a time to wait or names what it is
    const SAYS: [&str; 12] = ["quota", "limit", "usage", "used", "remaining", "left", "credit", "window", "rate", "budget", "cap", "allowance"];
    if !spans.iter().any(|s| s.2) && !SAYS.iter().any(|w| line.to_lowercase().contains(w)) {
        return None;
    }
    let mut shape = String::new();
    let mut cursor = 0;
    for (i, s) in spans.iter().enumerate() {
        shape.push_str(&line[cursor..s.0]);
        let hole = if s.2 { "reset" } else { roles.iter().find(|(j, _)| *j == i).map_or("n", |(_, r)| *r) };
        shape.push_str(&format!("{{{hole}}}"));
        cursor = s.1;
    }
    shape.push_str(&line[cursor..]);
    let word = |s: &str| s.split(|c: char| !c.is_alphabetic()).find(|w| !w.is_empty() && w.len() <= 12 && !NOT_UNITS.contains(&w.to_lowercase().as_str())).map(String::from);
    let after_cap = roles.last().map(|(i, _)| &line[spans[*i].1..]).and_then(|s| word(s.split([',', ';', '(', '[', ']']).next().unwrap_or_default().split_whitespace().take(1).collect::<Vec<_>>().join(" ").as_str()));
    let before = line[..spans[first].0].rsplit(|c: char| !c.is_alphabetic()).find(|w| !w.is_empty() && w.len() <= 12 && !NOT_UNITS.contains(&w.to_lowercase().as_str())).map(String::from);
    let row = Row { line: Some(squeeze(&shape)), unit: after_cap.or(before), ..Row::default() };
    row.check().ok()?;
    Some(row)
}

/// What a program's output says of its limit, as rows: the shapes its objects and its lines have. Each text is the whole output of one
/// run; a row is kept when it reads every text that holds a line of the same kind.
pub fn learn(outputs: &[String]) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for text in outputs {
        for raw in text.lines() {
            let line = decode::clean(raw);
            let line = line.trim();
            let found = objects(line).iter().find_map(row_of_keys).or_else(|| if line.contains('{') { None } else { row_of_line(line) });
            if let Some(row) = found.filter(|r| !rows.iter().any(|k| k == r)) {
                // a shape that does not read its own line is not kept
                if row.read(line).is_some_and(|r| r.gauge()) {
                    rows.push(row);
                }
            }
        }
    }
    // a row that every line it should read reads, and no more than the first of each kind
    rows.dedup_by(|a, b| a.line.is_some() == b.line.is_some() && a.line == b.line);
    rows.truncate(2);
    rows
}

/// The subcommands the program's help says report the limit: a word of the kinds a program names it, with words that say what it shows.
pub fn commands(help: &str) -> Vec<Vec<String>> {
    const NAMES: [&str; 6] = ["usage", "quota", "limits", "limit", "credits", "balance"];
    const SAYS: [&str; 7] = ["used", "quota", "limit", "remaining", "credit", "balance", "usage"];
    let mut found: Vec<Vec<String>> = Vec::new();
    for line in help.lines() {
        let mut words = line.split_whitespace();
        let (Some(name), Some(first)) = (words.next(), words.next()) else { continue };
        let about = format!("{first} {}", words.collect::<Vec<_>>().join(" ")).to_lowercase();
        if line.starts_with(char::is_whitespace) && NAMES.contains(&name) && SAYS.iter().any(|w| about.contains(w)) && !found.iter().any(|c| c[0] == name) {
            found.push(vec![name.to_string()]);
        }
    }
    found.truncate(2);
    found
}
