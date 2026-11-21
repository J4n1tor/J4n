//! The one value type Expr, Fmt and Query work on. Nothing here can fail: a thing that is not
//! there is [`Value::Missing`], and every comparison with nothing is false.

use std::cmp::Ordering;
use std::collections::BTreeMap;

/// What a missing value is shown as.
pub const DASH: &str = "—";

const MICROS: f64 = 1_000_000.0;
/// 2^63 as a float: money cannot reach it.
const MONEY_END: f64 = 9.223_372_036_854_776e18;

/// A value. `Num` is finite (build it with [`Value::num`]); `Dur` is milliseconds; `Usd` is millionths of a dollar, so
/// money compares and adds exactly. A value is as deep as whatever built it (the JSON and TOML readers cap their depth);
/// cloning, comparing and formatting one recurse that deep.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Value {
    #[default]
    Missing,
    Bool(bool),
    Num(f64),
    Str(String),
    Dur(u64),
    Usd(i64),
    List(Vec<Value>),
    Map(BTreeMap<String, Value>),
}

/// One step into a value: a name (`.cmd`, `["cmd"]`, a pointer segment) or a position (`[0]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    Name(String),
    Idx(i64),
}

fn micros(dollars: f64) -> i64 {
    (dollars * MICROS).round() as i64
}

/// Money against a plain number, in dollars, the number rounded to a millionth; a number past the range of money is
/// past all of it.
fn money_cmp(m: i64, n: f64) -> Ordering {
    let x = (n * MICROS).round();
    if x >= MONEY_END {
        Ordering::Less
    } else if x <= -MONEY_END {
        Ordering::Greater
    } else {
        m.cmp(&(x as i64))
    }
}

/// `"7"` is the position 7; `"07"`, `"+7"` and `"-1"` are not positions.
fn position(name: &str) -> Option<usize> {
    name.parse::<usize>().ok().filter(|n| n.to_string() == name)
}

impl Value {
    /// A number; one that is not finite is missing.
    pub fn num(n: f64) -> Value {
        if n.is_finite() { Value::Num(n) } else { Value::Missing }
    }

    /// Dollars as money, rounded to a millionth; one out of range is missing.
    pub fn usd(dollars: f64) -> Value {
        if (dollars * MICROS).abs() < MONEY_END { Value::Usd(micros(dollars)) } else { Value::Missing }
    }

    pub fn text(s: impl Into<String>) -> Value {
        Value::Str(s.into())
    }

    /// The kind as the word messages use.
    pub fn kind(&self) -> &'static str {
        match self {
            Value::Missing => "nothing",
            Value::Bool(_) => "a bool",
            Value::Num(_) => "a number",
            Value::Str(_) => "text",
            Value::Dur(_) => "a duration",
            Value::Usd(_) => "money",
            Value::List(_) => "a list",
            Value::Map(_) => "a map",
        }
    }

    /// Does it count as yes? Missing, `false`, zero, empty text, an empty list or map do not.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Missing => false,
            Value::Bool(b) => *b,
            Value::Num(n) => *n != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::Dur(d) => *d != 0,
            Value::Usd(m) => *m != 0,
            Value::List(l) => !l.is_empty(),
            Value::Map(m) => !m.is_empty(),
        }
    }

    fn descend(&self, path: &[Key]) -> Option<&Value> {
        let Some((key, rest)) = path.split_first() else { return Some(self) };
        let next = match (self, key) {
            (Value::Map(m), Key::Name(k)) => m.get(k),
            (Value::Map(m), Key::Idx(i)) => m.get(&i.to_string()),
            (Value::List(l), Key::Idx(i)) => usize::try_from(*i).ok().and_then(|i| l.get(i)),
            (Value::List(l), Key::Name(k)) => position(k).and_then(|i| l.get(i)),
            _ => None,
        };
        next?.descend(rest)
    }

    /// The value at `path`, or missing. Only the value found is copied.
    pub fn at(&self, path: &[Key]) -> Value {
        self.descend(path).cloned().unwrap_or_default()
    }

    /// An RFC 6901 JSON pointer read: `""` is the whole value, `/a/0/b` goes down by names, `~1` is `/` and `~0` is
    /// `~`. A name on a list is a position when it is a plain number.
    pub fn pointer(&self, ptr: &str) -> Value {
        if ptr.is_empty() {
            return self.clone();
        }
        let Some(rest) = ptr.strip_prefix('/') else { return Value::Missing };
        let path: Vec<Key> = rest.split('/').map(|s| Key::Name(s.replace("~1", "/").replace("~0", "~"))).collect();
        self.at(&path)
    }

    /// `Some(true)` when the two are the same, `Some(false)` when they surely are not, `None` when a missing thing is in
    /// the way (on either side, or inside a list or map). Scalars of one kind are the same by value, money and a plain
    /// number by dollars, lists by item, maps by key and value; different kinds are not the same.
    fn same_as(&self, other: &Value) -> Option<bool> {
        let all = |pairs: &mut dyn Iterator<Item = (&Value, &Value)>| {
            let mut hole = false;
            for (x, y) in pairs {
                match x.same_as(y) {
                    Some(false) => return Some(false),
                    None => hole = true,
                    Some(true) => {}
                }
            }
            if hole { None } else { Some(true) }
        };
        match (self, other) {
            (Value::Missing, _) | (_, Value::Missing) => None,
            (Value::Bool(a), Value::Bool(b)) => Some(a == b),
            (Value::List(a), Value::List(b)) if a.len() == b.len() => all(&mut a.iter().zip(b)),
            (Value::Map(a), Value::Map(b)) if a.len() == b.len() && a.keys().eq(b.keys()) => {
                all(&mut a.values().zip(b.values()))
            }
            _ => Some(self.order(other) == Some(Ordering::Equal)),
        }
    }

    /// Both are there and the same.
    pub fn equals(&self, other: &Value) -> bool {
        self.same_as(other) == Some(true)
    }

    /// Both are there and surely not the same: unlike `!equals`, nothing missing is in the way.
    pub fn differs(&self, other: &Value) -> bool {
        self.same_as(other) == Some(false)
    }

    /// The order of two scalars of one kind (text by character, a number against money by dollars); none for the rest.
    pub fn order(&self, other: &Value) -> Option<Ordering> {
        match (self, other) {
            (Value::Num(a), Value::Num(b)) => a.partial_cmp(b),
            (Value::Str(a), Value::Str(b)) => Some(a.cmp(b)),
            (Value::Dur(a), Value::Dur(b)) => Some(a.cmp(b)),
            (Value::Usd(a), Value::Usd(b)) => Some(a.cmp(b)),
            (Value::Usd(a), Value::Num(b)) if b.is_finite() => Some(money_cmp(*a, *b)),
            (Value::Num(a), Value::Usd(b)) if a.is_finite() => Some(money_cmp(*b, *a).reverse()),
            _ => None,
        }
    }

    fn rank(&self) -> u8 {
        match self {
            Value::Bool(_) => 0,
            Value::Num(_) => 1,
            Value::Usd(_) => 2,
            Value::Dur(_) => 3,
            Value::Str(_) => 4,
            Value::List(_) => 5,
            Value::Map(_) => 6,
            Value::Missing => 7,
        }
    }

    /// A total order for sorting: kinds in a fixed order (a number before money), then [`Value::order`] within one (lists
    /// and maps keep their place); missing is after everything.
    pub fn sort_cmp(&self, other: &Value) -> Ordering {
        match (self, other) {
            (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
            (Value::Num(a), Value::Num(b)) => a.partial_cmp(b).unwrap_or_else(|| a.is_nan().cmp(&b.is_nan())),
            _ => self.rank().cmp(&other.rank()).then_with(|| self.order(other).unwrap_or(Ordering::Equal)),
        }
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Value {
        Value::Bool(b)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Value {
        Value::Str(s.to_string())
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Value {
        Value::num(n)
    }
}

impl Value {
    /// Plain JSON read as a value, which is how an event's `data` reaches Expr: `null` is missing, numbers are numbers
    /// (money and durations have no JSON form of their own), objects are maps. Nothing here can fail.
    pub fn from_json(j: &serde_json::Value) -> Value {
        use serde_json::Value as J;
        match j {
            J::Null => Value::Missing,
            J::Bool(b) => Value::Bool(*b),
            J::Number(n) => n.as_f64().map(Value::num).unwrap_or_default(),
            J::String(s) => Value::Str(s.clone()),
            J::Array(items) => Value::List(items.iter().map(Value::from_json).collect()),
            J::Object(map) => Value::Map(map.iter().map(|(k, v)| (k.clone(), Value::from_json(v))).collect()),
        }
    }

    /// The value as plain JSON, for a screen to read (a view's `data`): money is dollars, a duration milliseconds, missing is `null`.
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::Value as J;
        match self {
            Value::Missing => J::Null,
            Value::Bool(b) => J::Bool(*b),
            Value::Num(n) => serde_json::Number::from_f64(*n).map_or(J::Null, J::Number),
            Value::Str(s) => J::String(s.clone()),
            Value::Dur(ms) => J::from(*ms),
            Value::Usd(m) => serde_json::Number::from_f64(*m as f64 / MICROS).map_or(J::Null, J::Number),
            Value::List(l) => J::Array(l.iter().map(Value::to_json).collect()),
            Value::Map(m) => J::Object(m.iter().map(|(k, v)| (k.clone(), v.to_json())).collect()),
        }
    }
}
