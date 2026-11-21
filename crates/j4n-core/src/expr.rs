//! Expr: the small, closed expression language behind every `when`, `filter`, gate test and connector map
//! It is **pure** (it reads and answers, nothing else), **total** (every expression has a value:
//! what is not there is [`crate::value::Value::Missing`], nothing panics, nothing loops) and **closed** (no pack can add
//! a function).
//!
//! # The language
//! ```text
//! expr    := or
//! or      := and ("or" and)*
//! and     := not ("and" not)*
//! not     := "not" not | cmp
//! cmp     := post (("==" | "!=" | "<" | "<=" | ">" | ">=" | "in" | "not" "in" | "matches") post)?
//! post    := name step*  |  call step*  |  pointer ("[" expr "]" step*)?  |  literal  |  "(" expr ")"
//! step    := "." name | "[" expr ("," expr)* "]"
//! call    := func "(" (expr ("," expr)*)? ")"
//! literal := 'text' | "text" | 12 | -3 | 0.5 | true | false | 5m | 250ms | 1h30m | $1.50 | [expr, …]
//! ```
//! - **Words**: `and or not in matches true false` cannot be names, except that `in.goal` and `in[…]` are the root `in`
//!   (where a value is due, `in` cannot be the operator). A name is `[A-Za-z_][A-Za-z0-9_]*`; after a `.` any name is
//!   a key, words included, and a key with other characters is written `root['claude-main']`. A comparison is between
//!   two things (`a < b < c` is refused: write `a < b and b < c`); there is no arithmetic, so `+` and `-` are not
//!   operators. A list literal may end with a comma, a call's arguments may not.
//! - **Paths** `event.data.cmd`, `fold.spend[task.id]`: the first name is a *root* that whoever evaluates the expression
//!   defines ([`crate::eval::Env::get`]); `.name` goes into a map, `[expr]` into a map or a list: a text that is a plain whole number
//!   (`'1'`, not `'01'`) is a list position, and a whole number is also a map's key (`m[1]` reads `"1"`); `[a, b]` is
//!   `[a][b]`, a key of two parts. A path that leads nowhere is missing.
//! - **Pointers** `/usage/cost_usd`, `/content/0/text`: an RFC 6901 JSON pointer into the document the expression is
//!   bound to ([`crate::eval::Env::pointer`]); a `~` that is not `~0` or `~1` stands for itself. `/` alone is the whole document. A
//!   pointer is made of letters, digits and `_-.:~@$%+/` (so `.` and `+` are part of it, never operators) and ends at
//!   anything else, so `/type=='text'` is a pointer and a comparison; only `[expr]` continues a pointer
//!   (`/content[0].text`).
//! - **Literals**: text takes `\\ \' \" \n \t \r`. A duration is whole or decimal numbers each with a unit of
//!   `ms s m h d` (`90s`, `1.5h`, `1h30m`); money is `$` and an amount, kept to a millionth, the seventh decimal
//!   rounding half up (`$0.0004`); a number is finite.
//! - **Truth**: missing, `false`, `0`, empty text, `0s`, `$0`, an empty list or map are *no*; everything else is *yes*.
//!   `not`, `and`, `or` read that way and answer `true` or `false`.
//! - **Comparing**: `==` is *yes* only when both sides are there and the same, `!=` only when both are there and surely
//!   not the same; with a missing thing anywhere in the way (on either side, or inside a list or map) both are *no*.
//!   Things of different kinds are not the same (`'1' != 1`). Text compares by character; durations and money by their
//!   own kind; a plain number against money counts in dollars, rounded to a millionth; a plain number against a
//!   duration never compares (give the field a duration type). Lists and maps are the same item by item and have no
//!   order. `<`/`<=`/`>`/`>=` are *no* when the two have no order. Nothing compares with nothing: `missing == missing`,
//!   `missing != 1`, `missing < 1` are all *no*; write `not (x == 1)` for "is not 1, or is not there".
//! - **`in`**: `x in list` (an item `==` x), `key in map`, `part in text` (text contains text). `matches`: text against
//!   a glob where `*` is any run of characters (dots and slashes too) and `?` any one; both sides must be text.
//! - **Functions** (a closed set, [`Func::ALL`]): `len(x)` characters of text, items of a list, keys of a map;
//!   `lower(text)`; `contains(hay, needle)` is `needle in hay` (*no* for a haystack it cannot search); `count(list)`
//!   the items that are there, `count(list, x)` the items `==` x; `sum(list)` the total of the numbers, durations or
//!   money in it (items that are missing are skipped, a mix of kinds or a total too large is missing, nothing to add
//!   is `0`); `last(list)`; `same(list)` *yes* when the list is not empty and every item `==` the first (*no* for
//!   anything else); `since(ts)` the duration from a time in milliseconds since 1970 to now ([`crate::eval::Env::now_ms`]);
//!   `quote(x)` text made inert for a template: one line in double quotes, `\` and `"` escaped, every control or
//!   white-space character a plain space, invisible and direction-changing characters dropped, at most [`QUOTE_MAX`]
//!   characters of the text kept (escapes add to that) and `…` after a cut; `danger(cmd)` what the gate's matcher says
//!   about a shell line ([`crate::eval::Env::danger`]). `count`, `sum`, `last` and `same` read a missing value as an empty list;
//!   `len`, `lower`, `since`, `quote` and `danger` answer missing for a kind they do not take.
//! - **Limits**: [`MAX_LEN`] characters, and [`MAX_DEPTH`] levels of nesting (parentheses, brackets, calls, `not`)
//!   inside the expression, so evaluation is bounded by what was written. `Display` prints an expression that parses
//!   back to the same tree, unless the printed spaces carry it past [`MAX_LEN`]. Error messages show a stray character
//!   escaped, never raw.

use crate::value::{Key, Value};
use std::fmt;
use std::str::FromStr;

/// The longest expression, in characters.
pub const MAX_LEN: usize = 2000;
/// The deepest nesting of parentheses, brackets, calls and `not` inside an expression.
pub const MAX_DEPTH: usize = 24;
/// The most characters of the text `quote` keeps; a cut ends in `…`, and escapes add to the length.
pub const QUOTE_MAX: usize = 200;

const UNITS: [(&str, f64); 5] = [("ms", 1.0), ("s", 1000.0), ("m", 60_000.0), ("h", 3_600_000.0), ("d", 86_400_000.0)];
const MAX_DUR_MS: f64 = 1.0e15;
/// Whole dollars a literal may hold; money is millionths in an `i64`.
const MAX_USD: i64 = 9_223_372_036_853;
const POINTER_CHARS: &str = "_-.:~@$%+/";
const WORDS: [&str; 5] = ["and", "or", "not", "in", "matches"];

/// What is wrong with an expression and where (`at` counts characters from 0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub at: usize,
    pub msg: String,
}

impl ParseError {
    pub(crate) fn new(at: usize, msg: impl Into<String>) -> ParseError {
        ParseError { at, msg: msg.into() }
    }

    /// The same error for text that starts `by` characters later.
    pub(crate) fn shift(mut self, by: usize) -> ParseError {
        self.at += by;
        self
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (character {})", self.msg, self.at + 1)
    }
}

impl std::error::Error for ParseError {}

/// A comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    In,
    Matches,
}

impl Op {
    pub fn word(self) -> &'static str {
        match self {
            Op::Eq => "==",
            Op::Ne => "!=",
            Op::Lt => "<",
            Op::Le => "<=",
            Op::Gt => ">",
            Op::Ge => ">=",
            Op::In => "in",
            Op::Matches => "matches",
        }
    }
}

/// The closed set of functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Func {
    Count,
    Same,
    Last,
    Since,
    Sum,
    Len,
    Lower,
    Contains,
    Danger,
    Quote,
}

impl Func {
    pub const ALL: [Func; 10] = [
        Func::Count,
        Func::Same,
        Func::Last,
        Func::Since,
        Func::Sum,
        Func::Len,
        Func::Lower,
        Func::Contains,
        Func::Danger,
        Func::Quote,
    ];

    pub fn word(self) -> &'static str {
        match self {
            Func::Count => "count",
            Func::Same => "same",
            Func::Last => "last",
            Func::Since => "since",
            Func::Sum => "sum",
            Func::Len => "len",
            Func::Lower => "lower",
            Func::Contains => "contains",
            Func::Danger => "danger",
            Func::Quote => "quote",
        }
    }

    /// The fewest and most arguments.
    pub fn arity(self) -> (usize, usize) {
        match self {
            Func::Count => (1, 2),
            Func::Contains => (2, 2),
            _ => (1, 1),
        }
    }

    /// The kinds (`Value::kind`) argument `i` can be; none listed: anything.
    fn takes(self, i: usize) -> &'static [&'static str] {
        const SIZED: &[&str] = &["text", "a list", "a map"];
        match (self, i) {
            (Func::Count | Func::Same | Func::Last | Func::Sum, 0) => &["a list"],
            (Func::Len | Func::Contains, 0) => SIZED,
            (Func::Lower | Func::Danger, _) => &["text"],
            (Func::Since, _) => &["a number"],
            (Func::Quote, _) => &["text", "a number", "a bool", "a duration", "money"],
            _ => &[],
        }
    }
}

/// One step of a path: `.name` or `[expr]`.
#[derive(Debug, Clone, PartialEq)]
pub enum Seg {
    Key(String),
    Index(Expr),
}

/// A parsed expression. `Display` prints one that parses back to the same tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Lit(Value),
    List(Vec<Expr>),
    Path {
        root: String,
        segs: Vec<Seg>,
    },
    Pointer(String),
    /// Steps taken after a function call or a pointer.
    Get {
        base: Box<Expr>,
        segs: Vec<Seg>,
    },
    Call {
        func: Func,
        args: Vec<Expr>,
    },
    Not(Box<Expr>),
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Cmp {
        op: Op,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
}

// ── reading ──────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Num(f64),
    Dur(u64),
    Usd(i64),
    Str(String),
    Ptr(String),
    Op(Op),
    LParen,
    RParen,
    LBrack,
    RBrack,
    Comma,
    Dot,
    End,
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Ident(w) => format!("`{w}`"),
        Tok::Num(_) => "a number".into(),
        Tok::Dur(_) => "a duration".into(),
        Tok::Usd(_) => "money".into(),
        Tok::Str(_) => "text".into(),
        Tok::Ptr(_) => "a pointer".into(),
        Tok::Op(o) => format!("`{}`", o.word()),
        Tok::LParen => "`(`".into(),
        Tok::RParen => "`)`".into(),
        Tok::LBrack => "`[`".into(),
        Tok::RBrack => "`]`".into(),
        Tok::Comma => "`,`".into(),
        Tok::Dot => "`.`".into(),
        Tok::End => "the end".into(),
    }
}

fn is(src: &[char], i: usize, test: fn(&char) -> bool) -> bool {
    src.get(i).is_some_and(test)
}

/// Digits with an optional fraction, at `*i`.
fn digits(src: &[char], i: &mut usize, at: usize) -> Result<f64, ParseError> {
    let start = *i;
    while is(src, *i, char::is_ascii_digit) {
        *i += 1;
    }
    if src.get(*i) == Some(&'.') && is(src, *i + 1, char::is_ascii_digit) {
        *i += 1;
        while is(src, *i, char::is_ascii_digit) {
            *i += 1;
        }
    }
    let text: String = src[start..*i].iter().collect();
    text.parse::<f64>().ok().filter(|n| n.is_finite()).ok_or_else(|| ParseError::new(at, "expected an amount"))
}

/// `$` and an amount, to a millionth (the seventh decimal rounds half up); `*i` is just past the `$`.
fn money(src: &[char], i: &mut usize, at: usize) -> Result<i64, ParseError> {
    let start = *i;
    while is(src, *i, char::is_ascii_digit) {
        *i += 1;
    }
    if *i == start {
        return Err(ParseError::new(at, "`$` needs an amount: `$1.50`"));
    }
    let whole: String = src[start..*i].iter().collect();
    let mut fraction = Vec::new();
    if src.get(*i) == Some(&'.') && is(src, *i + 1, char::is_ascii_digit) {
        *i += 1;
        while is(src, *i, char::is_ascii_digit) {
            fraction.push(src[*i].to_digit(10).map_or(0, i64::from));
            *i += 1;
        }
    }
    let Some(whole) = whole.parse::<i64>().ok().filter(|w| *w < MAX_USD) else {
        return Err(ParseError::new(at, "that amount is too large"));
    };
    let digit = |k: usize| fraction.get(k).copied().unwrap_or(0);
    let micros = (0..6).fold(0, |acc, k| acc * 10 + digit(k)) + i64::from(digit(6) >= 5);
    Ok(whole * 1_000_000 + micros)
}

fn number(src: &[char], i: &mut usize, at: usize) -> Result<Tok, ParseError> {
    let negative = src[*i] == '-';
    if negative {
        *i += 1;
    }
    let mut n = digits(src, i, at)?;
    if !is(src, *i, char::is_ascii_alphabetic) {
        return Ok(Tok::Num(if negative { -n } else { n }));
    }
    if negative {
        return Err(ParseError::new(at, "a duration is not negative"));
    }
    let mut total = 0.0;
    loop {
        let unit_at = *i;
        let mut unit = String::new();
        while is(src, *i, char::is_ascii_alphabetic) {
            unit.push(src[*i]);
            *i += 1;
        }
        let Some((_, ms)) = UNITS.iter().find(|(u, _)| *u == unit) else {
            return Err(ParseError::new(unit_at, format!("`{unit}` is not a duration unit (ms s m h d)")));
        };
        total += n * ms;
        if total > MAX_DUR_MS {
            return Err(ParseError::new(at, "that duration is too long"));
        }
        if !is(src, *i, char::is_ascii_digit) {
            return Ok(Tok::Dur(total.round() as u64));
        }
        n = digits(src, i, *i)?;
        if !is(src, *i, char::is_ascii_alphabetic) {
            return Err(ParseError::new(*i, "every part of a duration has a unit: `1h30m`"));
        }
    }
}

fn string(src: &[char], i: &mut usize, at: usize) -> Result<String, ParseError> {
    let quote = src[*i];
    let mut s = String::new();
    *i += 1;
    loop {
        let Some(&c) = src.get(*i) else { return Err(ParseError::new(at, "this text is never closed")) };
        *i += 1;
        if c == quote {
            return Ok(s);
        }
        if c != '\\' {
            s.push(c);
            continue;
        }
        let Some(&e) = src.get(*i) else { return Err(ParseError::new(at, "this text is never closed")) };
        *i += 1;
        s.push(match e {
            '\\' | '\'' | '"' => e,
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            _ => {
                return Err(ParseError::new(
                    *i - 2,
                    format!("`\\{}` is not an escape (they are \\\\ \\' \\\" \\n \\t \\r)", e.escape_debug()),
                ));
            }
        });
    }
}

fn lex(src: &[char]) -> Result<Vec<(Tok, usize)>, ParseError> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < src.len() {
        let (c, at) = (src[i], i);
        let tok = match c {
            c if c.is_whitespace() => {
                i += 1;
                continue;
            }
            '(' | ')' | '[' | ']' | ',' | '.' => {
                i += 1;
                match c {
                    '(' => Tok::LParen,
                    ')' => Tok::RParen,
                    '[' => Tok::LBrack,
                    ']' => Tok::RBrack,
                    ',' => Tok::Comma,
                    _ => Tok::Dot,
                }
            }
            '=' | '!' | '<' | '>' => {
                let with_eq = src.get(i + 1) == Some(&'=');
                i += 1 + usize::from(with_eq);
                Tok::Op(match (c, with_eq) {
                    ('=', true) => Op::Eq,
                    ('!', true) => Op::Ne,
                    ('<', true) => Op::Le,
                    ('>', true) => Op::Ge,
                    ('<', false) => Op::Lt,
                    ('>', false) => Op::Gt,
                    ('=', false) => return Err(ParseError::new(at, "use `==` to compare")),
                    _ => return Err(ParseError::new(at, "use `not`, or `!=` to compare")),
                })
            }
            '&' => return Err(ParseError::new(at, "use `and`")),
            '|' => return Err(ParseError::new(at, "use `or`")),
            '\'' | '"' => Tok::Str(string(src, &mut i, at)?),
            '/' => {
                i += 1;
                while is(src, i, |c| c.is_ascii_alphanumeric() || POINTER_CHARS.contains(*c)) {
                    i += 1;
                }
                let text: String = src[at..i].iter().collect();
                Tok::Ptr(if text == "/" { String::new() } else { text })
            }
            '$' => {
                i += 1;
                Tok::Usd(money(src, &mut i, at)?)
            }
            c if c.is_ascii_digit() || (c == '-' && is(src, i + 1, char::is_ascii_digit)) => number(src, &mut i, at)?,
            c if c.is_ascii_alphabetic() || c == '_' => {
                while is(src, i, |c| c.is_ascii_alphanumeric() || *c == '_') {
                    i += 1;
                }
                Tok::Ident(src[at..i].iter().collect())
            }
            _ => return Err(ParseError::new(at, format!("unexpected `{}`", c.escape_debug()))),
        };
        out.push((tok, at));
    }
    out.push((Tok::End, src.len()));
    Ok(out)
}

struct Parser {
    toks: Vec<(Tok, usize)>,
    p: usize,
    depth: usize,
}

type Parsed<T> = Result<T, ParseError>;

/// One item as itself, several as an `and` (or an `or`) of them; a parenthesised group of the same kind joins its
/// neighbours.
fn join(items: Vec<Expr>, and: bool) -> Expr {
    let mut flat = Vec::new();
    for item in items {
        match item {
            Expr::And(inner) if and => flat.extend(inner),
            Expr::Or(inner) if !and => flat.extend(inner),
            other => flat.push(other),
        }
    }
    match (flat.len(), and) {
        (1, _) => flat.remove(0),
        (_, true) => Expr::And(flat),
        (_, false) => Expr::Or(flat),
    }
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.p].0
    }

    fn at(&self) -> usize {
        self.toks[self.p].1
    }

    fn bump(&mut self) {
        self.p = (self.p + 1).min(self.toks.len() - 1);
    }

    fn word(&self, w: &str) -> bool {
        matches!(self.peek(), Tok::Ident(x) if x == w)
    }

    fn eat(&mut self, t: &Tok) -> bool {
        let found = self.peek() == t;
        if found {
            self.bump();
        }
        found
    }

    fn unexpected(&self, wanted: &str) -> ParseError {
        ParseError::new(self.at(), format!("expected {wanted}, found {}", describe(self.peek())))
    }

    fn expect(&mut self, t: &Tok, wanted: &str) -> Parsed<()> {
        if self.eat(t) { Ok(()) } else { Err(self.unexpected(wanted)) }
    }

    fn enter(&mut self) -> Parsed<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH + 1 {
            return Err(ParseError::new(self.at(), format!("nested more than {MAX_DEPTH} levels deep")));
        }
        Ok(())
    }

    fn or(&mut self) -> Parsed<Expr> {
        self.enter()?;
        let mut items = vec![self.and()?];
        while self.word("or") {
            self.bump();
            items.push(self.and()?);
        }
        self.depth -= 1;
        Ok(join(items, false))
    }

    fn and(&mut self) -> Parsed<Expr> {
        let mut items = vec![self.not()?];
        while self.word("and") {
            self.bump();
            items.push(self.not()?);
        }
        Ok(join(items, true))
    }

    fn not(&mut self) -> Parsed<Expr> {
        if !self.word("not") {
            return self.cmp();
        }
        self.bump();
        self.enter()?;
        let inner = self.not()?;
        self.depth -= 1;
        Ok(Expr::Not(Box::new(inner)))
    }

    /// An operator that starts a comparison, and whether the comparison is negated (`not in`).
    fn after(&self) -> &Tok {
        &self.toks[(self.p + 1).min(self.toks.len() - 1)].0
    }

    fn cmp_op(&mut self) -> Option<(Op, bool)> {
        let found = match self.peek() {
            Tok::Op(op) => (*op, false, 1),
            Tok::Ident(w) if w == "in" => (Op::In, false, 1),
            Tok::Ident(w) if w == "matches" => (Op::Matches, false, 1),
            Tok::Ident(w) if w == "not" && matches!(self.after(), Tok::Ident(n) if n == "in") => (Op::In, true, 2),
            _ => return None,
        };
        (0..found.2).for_each(|_| self.bump());
        Some((found.0, found.1))
    }

    fn cmp(&mut self) -> Parsed<Expr> {
        let lhs = self.primary()?;
        let Some((op, negated)) = self.cmp_op() else { return Ok(lhs) };
        let rhs = self.primary()?;
        if self.cmp_op().is_some() {
            return Err(ParseError::new(self.at(), "compare two things at a time: write `a < b and b < c`"));
        }
        let cmp = Expr::Cmp { op, lhs: Box::new(lhs), rhs: Box::new(rhs) };
        Ok(if negated { Expr::Not(Box::new(cmp)) } else { cmp })
    }

    /// `.name` and `[expr]` steps after a name, a call or a pointer (a pointer takes `.name` only after an index, as
    /// the lexer would read a dot straight after it as part of the pointer).
    fn steps(&mut self, base: Expr) -> Parsed<Expr> {
        let mut segs = Vec::new();
        let mut dotted = !matches!(base, Expr::Pointer(_));
        loop {
            if dotted && self.eat(&Tok::Dot) {
                let Tok::Ident(name) = self.peek().clone() else { return Err(self.unexpected("a name after `.`")) };
                self.bump();
                segs.push(Seg::Key(name));
            } else if self.eat(&Tok::LBrack) {
                // `[a, b]` is `[a][b]`: a key of two parts (a fold keyed by a task and a pattern)
                loop {
                    segs.push(Seg::Index(self.or()?));
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                self.expect(&Tok::RBrack, "`,` or `]`")?;
                dotted = true;
            } else {
                break;
            }
        }
        Ok(match base {
            _ if segs.is_empty() => base,
            Expr::Path { root, segs: mut first } => {
                first.extend(segs);
                Expr::Path { root, segs: first }
            }
            base => Expr::Get { base: Box::new(base), segs },
        })
    }

    fn call(&mut self, name: &str, at: usize) -> Parsed<Expr> {
        let Some(func) = Func::ALL.into_iter().find(|f| f.word() == name) else {
            let all: Vec<&str> = Func::ALL.iter().map(|f| f.word()).collect();
            return Err(ParseError::new(at, format!("no function `{name}` (they are {})", all.join(", "))));
        };
        self.bump();
        let mut args = Vec::new();
        if !self.eat(&Tok::RParen) {
            loop {
                args.push(self.or()?);
                if !self.eat(&Tok::Comma) {
                    self.expect(&Tok::RParen, "`,` or `)`")?;
                    break;
                }
            }
        }
        let (lo, hi) = func.arity();
        if args.len() < lo || args.len() > hi {
            let takes = if lo == hi { lo.to_string() } else { format!("{lo} or {hi}") };
            return Err(ParseError::new(at, format!("`{name}` takes {takes} argument(s), got {}", args.len())));
        }
        Ok(Expr::Call { func, args })
    }

    fn primary(&mut self) -> Parsed<Expr> {
        let at = self.at();
        let tok = self.peek().clone();
        let literal = |v: Value| Ok(Expr::Lit(v));
        match tok {
            Tok::Num(n) => {
                self.bump();
                literal(Value::Num(n))
            }
            Tok::Dur(ms) => {
                self.bump();
                literal(Value::Dur(ms))
            }
            Tok::Usd(m) => {
                self.bump();
                literal(Value::Usd(m))
            }
            Tok::Str(s) => {
                self.bump();
                literal(Value::Str(s))
            }
            Tok::Ptr(p) => {
                self.bump();
                self.steps(Expr::Pointer(p))
            }
            Tok::LParen => {
                self.bump();
                let inner = self.or()?;
                self.expect(&Tok::RParen, "`)`")?;
                Ok(inner)
            }
            Tok::LBrack => {
                self.bump();
                let mut items = Vec::new();
                while !self.eat(&Tok::RBrack) {
                    items.push(self.or()?);
                    if !self.eat(&Tok::Comma) {
                        self.expect(&Tok::RBrack, "`,` or `]`")?;
                        break;
                    }
                }
                Ok(Expr::List(items))
            }
            Tok::Ident(w) if w == "true" || w == "false" => {
                self.bump();
                literal(Value::Bool(w == "true"))
            }
            // where a value is due `in` cannot be the operator, so `in.goal` is the root `in` (a flow's inputs)
            Tok::Ident(w) if w == "in" && matches!(self.after(), Tok::Dot | Tok::LBrack) => {
                self.bump();
                self.steps(Expr::Path { root: w, segs: Vec::new() })
            }
            Tok::Ident(w) if WORDS.contains(&w.as_str()) => Err(self.unexpected("a value")),
            Tok::Ident(name) => {
                self.bump();
                if self.peek() == &Tok::LParen {
                    self.call(&name, at).and_then(|call| self.steps(call))
                } else {
                    self.steps(Expr::Path { root: name, segs: Vec::new() })
                }
            }
            _ => Err(self.unexpected("a value")),
        }
    }
}

impl Expr {
    /// Read an expression. Everything wrong is an error here, so what evaluates never fails.
    pub fn parse(text: &str) -> Result<Expr, ParseError> {
        let src: Vec<char> = text.chars().collect();
        if src.len() > MAX_LEN {
            return Err(ParseError::new(MAX_LEN, format!("longer than {MAX_LEN} characters")));
        }
        let mut p = Parser { toks: lex(&src)?, p: 0, depth: 0 };
        if p.peek() == &Tok::End {
            return Err(ParseError::new(0, "an empty expression"));
        }
        let expr = p.or()?;
        if p.peek() != &Tok::End {
            return Err(p.unexpected("an operator or the end"));
        }
        Ok(expr)
    }
}

impl Expr {
    /// A duration as it is written in a table (`"90s"`, `"1h30m"`), in milliseconds; nothing for any other text.
    pub fn dur(text: &str) -> Option<u64> {
        match Expr::parse(text) {
            Ok(Expr::Lit(Value::Dur(ms))) => Some(ms),
            _ => None,
        }
    }

    /// The paths the expression reads, each as its root and then every key written with a dot (a step that is computed
    /// is left out), except what `quote(…)`, `len(…)` and `count(…)` make inert, and the key of a fold read: what a template is not to say bare.
    pub fn unquoted_reads(&self) -> Vec<Vec<String>> {
        fn walk(e: &Expr, out: &mut Vec<Vec<String>>) {
            let steps = |segs: &[Seg], out: &mut Vec<Vec<String>>| {
                for seg in segs {
                    if let Seg::Index(i) = seg {
                        walk(i, out);
                    }
                }
            };
            match e {
                Expr::Lit(_) | Expr::Pointer(_) => {}
                Expr::List(v) | Expr::And(v) | Expr::Or(v) => v.iter().for_each(|x| walk(x, out)),
                Expr::Not(x) => walk(x, out),
                Expr::Cmp { lhs, rhs, .. } => {
                    walk(lhs, out);
                    walk(rhs, out);
                }
                // a number, or what `quote` makes inert: no words of a doer's or an operator's come out of it
                Expr::Call { func: Func::Quote | Func::Len | Func::Count, .. } => {}
                Expr::Call { args, .. } => args.iter().for_each(|x| walk(x, out)),
                Expr::Get { base, segs } => {
                    walk(base, out);
                    steps(segs, out);
                }
                Expr::Path { root, segs } => {
                    let mut name = vec![root.clone()];
                    name.extend(segs.iter().filter_map(|seg| if let Seg::Key(k) = seg { Some(k.clone()) } else { None }));
                    out.push(name);
                    // a fold is read by a key and says what it holds, not the key
                    if root != "fold" {
                        steps(segs, out);
                    }
                }
            }
        }
        let mut out = Vec::new();
        walk(self, &mut out);
        out
    }
}

impl FromStr for Expr {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<Expr, ParseError> {
        Expr::parse(text)
    }
}

// ── printing ─────────────────────────────────────────────────────────────────────────────────

fn dur_text(ms: u64) -> String {
    let mut rest = ms;
    let mut text = String::new();
    for (unit, size) in [("d", 86_400_000), ("h", 3_600_000), ("m", 60_000), ("s", 1000), ("ms", 1)] {
        if rest >= size {
            text += &format!("{}{unit}", rest / size);
            rest %= size;
        }
    }
    if text.is_empty() { "0s".into() } else { text }
}

fn usd_text(micros: i64) -> String {
    let (sign, abs) = (if micros < 0 { "-" } else { "" }, micros.unsigned_abs());
    match abs % 1_000_000 {
        0 => format!("{sign}${}", abs / 1_000_000),
        frac => format!("{sign}${}.{}", abs / 1_000_000, format!("{frac:06}").trim_end_matches('0')),
    }
}

fn write_lit(f: &mut fmt::Formatter<'_>, v: &Value) -> fmt::Result {
    match v {
        Value::Bool(b) => write!(f, "{b}"),
        Value::Num(n) => write!(f, "{n}"),
        Value::Dur(ms) => write!(f, "{}", dur_text(*ms)),
        Value::Usd(m) => write!(f, "{}", usd_text(*m)),
        Value::Str(s) => {
            f.write_str("'")?;
            for c in s.chars() {
                match c {
                    '\\' | '\'' => write!(f, "\\{c}")?,
                    '\n' => f.write_str("\\n")?,
                    '\t' => f.write_str("\\t")?,
                    '\r' => f.write_str("\\r")?,
                    c => write!(f, "{c}")?,
                }
            }
            f.write_str("'")
        }
        Value::List(items) => {
            f.write_str("[")?;
            for (i, item) in items.iter().enumerate() {
                f.write_str(if i == 0 { "" } else { ", " })?;
                write_lit(f, item)?;
            }
            f.write_str("]")
        }
        Value::Missing | Value::Map(_) => f.write_str("[]"),
    }
}

fn write_segs(f: &mut fmt::Formatter<'_>, segs: &[Seg]) -> fmt::Result {
    for seg in segs {
        match seg {
            Seg::Key(k) => write!(f, ".{k}")?,
            Seg::Index(e) => write!(f, "[{e}]")?,
        }
    }
    Ok(())
}

impl Expr {
    fn prec(&self) -> u8 {
        match self {
            Expr::Or(_) => 1,
            Expr::And(_) => 2,
            Expr::Not(_) => 3,
            Expr::Cmp { .. } => 4,
            _ => 9,
        }
    }

    /// A child, in parentheses when it binds no tighter than its parent.
    fn write_under(&self, f: &mut fmt::Formatter<'_>, parent: u8) -> fmt::Result {
        if self.prec() <= parent { write!(f, "({self})") } else { write!(f, "{self}") }
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Lit(v) => write_lit(f, v),
            Expr::List(items) => {
                f.write_str("[")?;
                for (i, item) in items.iter().enumerate() {
                    f.write_str(if i == 0 { "" } else { ", " })?;
                    write!(f, "{item}")?;
                }
                f.write_str("]")
            }
            Expr::Path { root, segs } => {
                f.write_str(root)?;
                write_segs(f, segs)
            }
            Expr::Pointer(p) => f.write_str(if p.is_empty() { "/" } else { p }),
            Expr::Get { base, segs } => {
                write!(f, "{base}")?;
                write_segs(f, segs)
            }
            Expr::Call { func, args } => {
                write!(f, "{}(", func.word())?;
                for (i, a) in args.iter().enumerate() {
                    f.write_str(if i == 0 { "" } else { ", " })?;
                    write!(f, "{a}")?;
                }
                f.write_str(")")
            }
            Expr::Not(inner) => {
                f.write_str("not ")?;
                inner.write_under(f, 2)
            }
            Expr::And(items) | Expr::Or(items) => {
                let (word, prec) = if matches!(self, Expr::And(_)) { (" and ", 2) } else { (" or ", 1) };
                for (i, item) in items.iter().enumerate() {
                    f.write_str(if i == 0 { "" } else { word })?;
                    item.write_under(f, prec)?;
                }
                Ok(())
            }
            Expr::Cmp { op, lhs, rhs } => {
                lhs.write_under(f, 4)?;
                write!(f, " {} ", op.word())?;
                rhs.write_under(f, 4)
            }
        }
    }
}

// ── the lint ─────────────────────────────────────────────────────────────────────────────────

/// What is known of an expression's kind before it runs: only a literal tells.
fn lit_kind(e: &Expr) -> Option<&'static str> {
    match e {
        Expr::Lit(v) => Some(v.kind()),
        Expr::List(_) => Some("a list"),
        _ => None,
    }
}

impl Expr {
    /// Every mistake that can be seen without running it: a root `roots` does not list (none given: any is fine), a
    /// literal of a kind a function or `in`/`matches` cannot take.
    pub fn check(&self, roots: Option<&[&str]>) -> Vec<String> {
        let mut out = Vec::new();
        self.lint(roots, &mut out);
        out
    }

    fn lint(&self, roots: Option<&[&str]>, out: &mut Vec<String>) {
        let steps = |segs: &[Seg], out: &mut Vec<String>| {
            for seg in segs {
                if let Seg::Index(e) = seg {
                    e.lint(roots, out);
                }
            }
        };
        match self {
            Expr::Lit(_) | Expr::Pointer(_) => {}
            Expr::List(items) | Expr::And(items) | Expr::Or(items) => items.iter().for_each(|e| e.lint(roots, out)),
            Expr::Not(e) => e.lint(roots, out),
            Expr::Path { root, segs } => {
                if let Some(known) = roots.filter(|k| !k.contains(&root.as_str())) {
                    let names = if known.is_empty() {
                        "there are none".to_string()
                    } else {
                        format!("they are {}", known.join(", "))
                    };
                    out.push(format!("`{root}` is not a name here ({names})"));
                }
                steps(segs, out);
            }
            Expr::Get { base, segs } => {
                base.lint(roots, out);
                steps(segs, out);
            }
            Expr::Call { func, args } => {
                for (i, arg) in args.iter().enumerate() {
                    let takes = func.takes(i);
                    if let Some(kind) = lit_kind(arg).filter(|k| !takes.is_empty() && !takes.contains(k)) {
                        out.push(format!(
                            "`{self}`: argument {} is {kind}, but `{}` takes {}",
                            i + 1,
                            func.word(),
                            takes.join(" or ")
                        ));
                    }
                    arg.lint(roots, out);
                }
            }
            Expr::Cmp { op, lhs, rhs } => {
                let takes: &[&str] = match op {
                    Op::Matches => &["text"],
                    Op::In => &["text", "a list", "a map"],
                    _ => &[],
                };
                if let Some(kind) = lit_kind(rhs).filter(|k| !takes.is_empty() && !takes.contains(k)) {
                    out.push(format!(
                        "`{self}`: `{}` needs {} on its right, not {kind}",
                        op.word(),
                        takes.join(" or ")
                    ));
                }
                let text_only = *op == Op::Matches || (*op == Op::In && lit_kind(rhs) == Some("text"));
                if let Some(kind) = lit_kind(lhs).filter(|k| text_only && *k != "text") {
                    out.push(format!("`{self}`: `{}` needs text on its left, not {kind}", op.word()));
                }
                lhs.lint(roots, out);
                rhs.lint(roots, out);
            }
        }
    }
}

/// Used by evaluation: the keys a path's steps name, or none when a step cannot name one.
pub(crate) fn key_of(v: &Value) -> Option<Key> {
    match v {
        Value::Str(s) => Some(Key::Name(s.clone())),
        Value::Num(n) if n.fract() == 0.0 && n.abs() < 9.0e15 => Some(Key::Idx(*n as i64)),
        _ => None,
    }
}
