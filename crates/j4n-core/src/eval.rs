//! Running an expression over the world it is asked in (`Env`). Evaluation never fails and never changes anything: the
//! same expression over the same `Env` is the same answer (the language in [`crate::expr`]).

use crate::expr::{Expr, Func, Op, QUOTE_MAX, Seg, key_of};
use crate::value::{Key, Value};
use std::cmp::Ordering;
use std::collections::BTreeMap;

static MISSING: Value = Value::Missing;

/// The world an expression reads. Whoever evaluates implements it; every method answers missing for what it does not
/// know, and the defaults know nothing.
pub trait Env {
    /// The value of the root `root` followed by `path` (empty: the root itself, which is how a caller asks whether
    /// there is such a root).
    fn get(&self, root: &str, path: &[Key]) -> Value;

    /// A JSON pointer read against the document the expression is bound to. `ptr` is as written, escapes (`~0`, `~1`)
    /// not decoded, and `""` for a lone `/` (the whole document); [`Value::pointer`] reads one.
    fn pointer(&self, _ptr: &str) -> Value {
        Value::Missing
    }

    /// Now, in milliseconds since 1970 (for `since`).
    fn now_ms(&self) -> Option<u64> {
        None
    }

    /// What the gate's matcher says of a shell line (for `danger`).
    fn danger(&self, _cmd: &str) -> Value {
        Value::Missing
    }
}

/// An `Env` held in plain data: named roots, one bound document, a clock and a `danger` function.
#[derive(Debug, Clone, Default)]
pub struct Scope {
    roots: BTreeMap<String, Value>,
    doc: Value,
    now: Option<u64>,
    danger: Option<fn(&str) -> Value>,
}

impl Scope {
    pub fn new() -> Scope {
        Scope::default()
    }

    pub fn with(mut self, root: &str, value: Value) -> Scope {
        self.roots.insert(root.to_string(), value);
        self
    }

    pub fn doc(mut self, value: Value) -> Scope {
        self.doc = value;
        self
    }

    pub fn at_ms(mut self, now: u64) -> Scope {
        self.now = Some(now);
        self
    }

    pub fn danger_with(mut self, f: fn(&str) -> Value) -> Scope {
        self.danger = Some(f);
        self
    }
}

impl Env for Scope {
    fn get(&self, root: &str, path: &[Key]) -> Value {
        self.roots.get(root).map(|v| v.at(path)).unwrap_or_default()
    }

    fn pointer(&self, ptr: &str) -> Value {
        self.doc.pointer(ptr)
    }

    fn now_ms(&self) -> Option<u64> {
        self.now
    }

    fn danger(&self, cmd: &str) -> Value {
        self.danger.map(|f| f(cmd)).unwrap_or_default()
    }
}

/// `*` matches any run of characters (dots and slashes too), `?` any one character; everything else is itself.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if p.get(pi) == Some(&'*') {
            (star, mark) = (Some(pi), ti);
            pi += 1;
        } else if p.get(pi).is_some_and(|c| *c == '?' || *c == t[ti]) {
            (pi, ti) = (pi + 1, ti + 1);
        } else if let Some(s) = star {
            (pi, mark) = (s + 1, mark + 1);
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

fn keys(segs: &[Seg], env: &dyn Env) -> Option<Vec<Key>> {
    segs.iter()
        .map(|seg| match seg {
            Seg::Key(k) => Some(Key::Name(k.clone())),
            Seg::Index(e) => key_of(&e.eval(env)),
        })
        .collect()
}

fn within(needle: &Value, hay: &Value) -> bool {
    match (needle, hay) {
        (_, Value::List(items)) => items.iter().any(|item| needle.equals(item)),
        (Value::Str(k), Value::Map(m)) => m.contains_key(k),
        (Value::Str(part), Value::Str(whole)) => whole.contains(part.as_str()),
        _ => false,
    }
}

fn compare(op: Op, a: &Value, b: &Value) -> bool {
    let order = a.order(b);
    match op {
        Op::Eq => a.equals(b),
        Op::Ne => a.differs(b),
        Op::Lt => order == Some(Ordering::Less),
        Op::Le => matches!(order, Some(Ordering::Less | Ordering::Equal)),
        Op::Gt => order == Some(Ordering::Greater),
        Op::Ge => matches!(order, Some(Ordering::Greater | Ordering::Equal)),
        Op::In => within(a, b),
        Op::Matches => matches!((a, b), (Value::Str(s), Value::Str(p)) if glob_match(p, s)),
    }
}

/// A list's items; a missing value is an empty list; anything else is not a list.
fn items(v: &Value) -> Option<&[Value]> {
    match v {
        Value::List(l) => Some(l),
        Value::Missing => Some(&[]),
        _ => None,
    }
}

fn total(list: &[Value]) -> Value {
    let (mut num, mut dur, mut usd, mut seen) = (0.0, 0u64, 0i64, [false; 3]);
    for v in list.iter().filter(|v| **v != Value::Missing) {
        match v {
            Value::Num(n) => (num, seen[0]) = (num + n, true),
            Value::Dur(d) => match dur.checked_add(*d) {
                Some(sum) => (dur, seen[1]) = (sum, true),
                None => return Value::Missing,
            },
            Value::Usd(m) => match usd.checked_add(*m) {
                Some(sum) => (usd, seen[2]) = (sum, true),
                None => return Value::Missing,
            },
            _ => return Value::Missing,
        }
    }
    match seen {
        [false, false, false] => Value::Num(0.0),
        [true, false, false] => Value::num(num),
        [false, true, false] => Value::Dur(dur),
        [false, false, true] => Value::Usd(usd),
        _ => Value::Missing,
    }
}

/// A character that shows nothing and can hide or reorder what is around it.
fn invisible(c: char) -> bool {
    matches!(c, '\u{ad}' | '\u{61c}' | '\u{180e}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}'
        | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206f}' | '\u{feff}' | '\u{fff9}'..='\u{fffb}' | '\u{e0000}'..='\u{e007f}')
}

fn quote(v: &Value) -> Value {
    let text = match v {
        Value::Str(s) => s.clone(),
        Value::Num(_) | Value::Bool(_) | Value::Dur(_) | Value::Usd(_) => crate::fmt::format_value(v, None),
        _ => return Value::Missing,
    };
    let kept: Vec<char> = text.chars().filter(|c| !invisible(*c)).collect();
    let mut out = String::from("\"");
    for &c in kept.iter().take(QUOTE_MAX) {
        match c {
            '\\' | '"' => out.extend(['\\', c]),
            c if c.is_control() || c.is_whitespace() => out.push(' '),
            c => out.push(c),
        }
    }
    if kept.len() > QUOTE_MAX {
        out.push('…');
    }
    out.push('"');
    Value::Str(out)
}

fn call(func: Func, args: &[Expr], env: &dyn Env) -> Value {
    let a: Vec<Value> = args.iter().map(|e| e.eval(env)).collect();
    let arg = |i: usize| a.get(i).unwrap_or(&MISSING);
    let count = |n: usize| Value::Num(n as f64);
    match func {
        Func::Len => match arg(0) {
            Value::Str(s) => count(s.chars().count()),
            Value::List(l) => count(l.len()),
            Value::Map(m) => count(m.len()),
            _ => Value::Missing,
        },
        Func::Lower => match arg(0) {
            Value::Str(s) => Value::Str(s.to_lowercase()),
            _ => Value::Missing,
        },
        Func::Contains => Value::Bool(compare(Op::In, arg(1), arg(0))),
        Func::Count => match items(arg(0)) {
            Some(list) if a.len() == 2 => count(list.iter().filter(|v| v.equals(arg(1))).count()),
            Some(list) => count(list.iter().filter(|v| **v != Value::Missing).count()),
            None => Value::Missing,
        },
        Func::Sum => items(arg(0)).map_or(Value::Missing, total),
        Func::Last => items(arg(0)).and_then(|l| l.last().cloned()).unwrap_or_default(),
        Func::Same => {
            Value::Bool(items(arg(0)).is_some_and(|l| l.first().is_some_and(|f| l.iter().all(|v| v.equals(f)))))
        }
        Func::Since => match (arg(0), env.now_ms()) {
            (Value::Num(ts), Some(now)) => Value::Dur(now.saturating_sub(ts.max(0.0) as u64)),
            _ => Value::Missing,
        },
        Func::Quote => quote(arg(0)),
        Func::Danger => match arg(0) {
            Value::Str(cmd) => env.danger(cmd),
            _ => Value::Missing,
        },
    }
}

impl Expr {
    /// The value. Total: a thing that is not there, or of a kind that does not fit, is [`Value::Missing`] (or `false`).
    pub fn eval(&self, env: &dyn Env) -> Value {
        match self {
            Expr::Lit(v) => v.clone(),
            Expr::List(items) => Value::List(items.iter().map(|e| e.eval(env)).collect()),
            Expr::Path { root, segs } => keys(segs, env).map_or(Value::Missing, |k| env.get(root, &k)),
            Expr::Pointer(p) => env.pointer(p),
            Expr::Get { base, segs } => keys(segs, env).map_or(Value::Missing, |k| base.eval(env).at(&k)),
            Expr::Call { func, args } => call(*func, args, env),
            Expr::Not(e) => Value::Bool(!e.eval(env).truthy()),
            Expr::And(items) => Value::Bool(items.iter().all(|e| e.eval(env).truthy())),
            Expr::Or(items) => Value::Bool(items.iter().any(|e| e.eval(env).truthy())),
            Expr::Cmp { op, lhs, rhs } => Value::Bool(compare(*op, &lhs.eval(env), &rhs.eval(env))),
        }
    }

    /// Does it come out *yes*?
    pub fn holds(&self, env: &dyn Env) -> bool {
        self.eval(env).truthy()
    }
}
