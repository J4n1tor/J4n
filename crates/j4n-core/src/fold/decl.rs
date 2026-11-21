//! Declarative folds: the kind `fold` (`packs/base/kinds/fold.toml`), a reduction written as data.
//!
//! ```toml
//! [[fold]]
//! id = "spend"
//! on = "run.metrics"          # event kinds, a glob
//! key = "{task}"             # one value per key; leave out for one value in all
//! reduce = "sum"             # count · sum · last · set · max · streak · distinct
//! of = "data.usd"            # what is reduced (not for count)
//! unit = "usd"               # n (default) · usd · dur: how a number in `of` is read
//! when = "data.ok"           # only events for which this holds
//! window = 40                # only the last 40 events of a key …
//! within = "7d"              # … and only those not older than this (as of the reading)
//! scope = "project"          # which log: home · project (default)
//! ```
//!
//! The expressions read the event: `ev` (or `event`), and its fields by name — `seq at_ms kind task thread by cause class
//! data` — with `by` one word like `rule:followups`. Time is the event's own, so a replay gives the same answer.
//!
//! - `count` counts events; `sum` adds (numbers, money, or durations, not a mix — a mix makes the sum missing); `last`
//!   and `max` keep one value; `set` lists the distinct values, `distinct` counts them (up to 10,000 a key, then the
//!   rest are not kept and the fold says so); `streak` counts the events in a row for which `of` holds (one for which it
//!   does not starts again from nothing; a `when` that fails skips the event altogether).
//! - An event whose `of`, or a hole of `key`, is missing is skipped. `window` and `within` make the answer a function of
//!   the events in them, computed at the reading; a key keeps at most 10,000 of them.
//! - At most 100,000 keys; the rest are not kept and the fold says so.
//! - Read as `fold.<id>` (a map by key, or the one value), `fold.<id>[task]`, `fold.<id>.<key>`.

use super::{Fold, from_tagged, to_tagged};
use crate::eval::{Env, glob_match};
use crate::expr::Expr;
use crate::fmt::{Fmt, Piece};
use crate::log::{Ev, Scope};
use crate::table::{Kind, Problem, Tables};
use crate::value::{Key, Value};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value as Json;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::{BTreeMap, VecDeque};

const MAX_KEYS: usize = 100_000;
const MAX_SET: usize = 10_000;
const MAX_WINDOW: usize = 10_000;
/// Bumped when what a snapshot means changes, so old snapshots are not read as new.
const FORMAT: u32 = 1;
/// What an expression of a fold may name.
const ROOTS: [&str; 11] = ["ev", "event", "seq", "at_ms", "kind", "task", "thread", "by", "cause", "class", "data"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reduce {
    Count,
    Sum,
    Last,
    Set,
    Max,
    Streak,
    Distinct,
}

/// How a plain number in `of` is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    #[default]
    N,
    /// Dollars, kept exactly as millionths.
    Usd,
    /// Milliseconds.
    Dur,
}

/// An entry of kind `fold`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoldDecl {
    pub id: String,
    pub doc: Option<String>,
    pub on: String,
    pub reduce: Reduce,
    pub of: Option<String>,
    pub key: Option<String>,
    pub when: Option<String>,
    #[serde(default)]
    pub unit: Unit,
    pub window: Option<u64>,
    pub within: Option<String>,
    #[serde(default)]
    pub scope: Scope,
}

impl Kind for FoldDecl {
    const NAME: &'static str = "fold";
}

/// A value that serializes with money and durations kept apart from numbers.
#[derive(Debug, Clone, PartialEq)]
struct V(Value);

impl Serialize for V {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        to_tagged(&self.0).serialize(s)
    }
}

impl<'de> Deserialize<'de> for V {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<V, D::Error> {
        from_tagged(&Json::deserialize(d)?).map(V).map_err(serde::de::Error::custom)
    }
}

/// One key's running answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
enum Acc {
    Count(u64),
    /// The sum so far (missing: nothing yet), and whether the kinds mixed.
    Sum(V, bool),
    Last(V),
    Max(V),
    Set(BTreeMap<String, V>, bool),
    Streak(u64),
}

impl Acc {
    fn new(r: Reduce) -> Acc {
        match r {
            Reduce::Count => Acc::Count(0),
            Reduce::Sum => Acc::Sum(V(Value::Missing), false),
            Reduce::Last => Acc::Last(V(Value::Missing)),
            Reduce::Max => Acc::Max(V(Value::Missing)),
            Reduce::Set | Reduce::Distinct => Acc::Set(BTreeMap::new(), false),
            Reduce::Streak => Acc::Streak(0),
        }
    }

    fn push(&mut self, v: &Value) {
        match self {
            Acc::Count(n) => *n += 1,
            Acc::Sum(total, poisoned) => {
                if *poisoned {
                    return;
                }
                let next = match (&total.0, v) {
                    (Value::Missing, Value::Num(_) | Value::Usd(_) | Value::Dur(_)) => v.clone(),
                    (Value::Num(a), Value::Num(b)) => Value::num(a + b),
                    (Value::Usd(a), Value::Usd(b)) => a.checked_add(*b).map(Value::Usd).unwrap_or_default(),
                    (Value::Dur(a), Value::Dur(b)) => a.checked_add(*b).map(Value::Dur).unwrap_or_default(),
                    _ => Value::Missing,
                };
                if matches!(next, Value::Missing) {
                    *poisoned = true;
                }
                total.0 = next;
            }
            Acc::Last(cur) => cur.0 = v.clone(),
            Acc::Max(cur) => {
                if matches!(cur.0, Value::Missing) || v.order(&cur.0) == Some(Ordering::Greater) {
                    cur.0 = v.clone();
                }
            }
            Acc::Set(items, capped) => {
                let key = to_tagged(v).to_string();
                if items.contains_key(&key) {
                    return;
                }
                if items.len() >= MAX_SET {
                    *capped = true;
                } else {
                    items.insert(key, V(v.clone()));
                }
            }
            Acc::Streak(n) => *n = if v.truthy() { *n + 1 } else { 0 },
        }
    }

    fn out(&self, r: Reduce, unit: Unit) -> Value {
        match self {
            Acc::Count(n) | Acc::Streak(n) => Value::num(*n as f64),
            Acc::Sum(_, true) => Value::Missing,
            Acc::Sum(t, false) if matches!(t.0, Value::Missing) => match unit {
                Unit::N => Value::num(0.0),
                Unit::Usd => Value::Usd(0),
                Unit::Dur => Value::Dur(0),
            },
            Acc::Sum(t, false) => t.0.clone(),
            Acc::Last(v) | Acc::Max(v) => v.0.clone(),
            Acc::Set(items, _) if r == Reduce::Distinct => Value::num(items.len() as f64),
            Acc::Set(items, _) => Value::List(items.values().map(|v| v.0.clone()).collect()),
        }
    }

    fn capped(&self) -> bool {
        matches!(self, Acc::Set(_, true))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum Cell {
    Live(Acc),
    Window(VecDeque<(u64, V)>),
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    cells: BTreeMap<String, Cell>,
    capped_keys: bool,
    capped_window: bool,
}

/// A fold built from an entry of kind `fold`.
pub struct DeclFold {
    id: String,
    version: u64,
    on: String,
    reduce: Reduce,
    unit: Unit,
    of: Option<Expr>,
    key: Option<Fmt>,
    when: Option<Expr>,
    window: Option<usize>,
    within: Option<u64>,
    state: State,
}

/// The event as an expression reads it. The clock is the event's, never the wall's.
struct EvEnv<'a>(&'a Value, u64);

impl Env for EvEnv<'_> {
    fn get(&self, root: &str, path: &[Key]) -> Value {
        match (root, self.0) {
            ("ev" | "event", row) => row.at(path),
            (name, Value::Map(m)) => m.get(name).map(|v| v.at(path)).unwrap_or_default(),
            _ => Value::Missing,
        }
    }

    fn now_ms(&self) -> Option<u64> {
        Some(self.1)
    }
}

impl FoldDecl {
    /// Check the entry's expressions and combinations, and build the fold. Every fault is reported, not only the first.
    pub fn compile(&self) -> Result<DeclFold, Vec<String>> {
        let mut errs = Vec::new();
        let mut expr = |what: &str, src: &Option<String>| -> Option<Expr> {
            let e = match Expr::parse(src.as_deref()?) {
                Ok(e) => e,
                Err(e) => {
                    errs.push(format!("`{what}`: {e}"));
                    return None;
                }
            };
            errs.extend(e.check(Some(&ROOTS)).into_iter().map(|m| format!("`{what}`: {m}")));
            Some(e)
        };
        let (of, when) = (expr("of", &self.of), expr("when", &self.when));
        let key = match self.key.as_deref().map(Fmt::parse) {
            None => None,
            Some(Ok(f)) => {
                errs.extend(f.check(Some(&ROOTS)).into_iter().map(|m| format!("`key`: {m}")));
                Some(f)
            }
            Some(Err(e)) => {
                errs.push(format!("`key`: {e}"));
                None
            }
        };
        let within = match self.within.as_deref().map(Expr::parse) {
            None => None,
            Some(Ok(Expr::Lit(Value::Dur(ms)))) => Some(ms),
            Some(_) => {
                errs.push("`within` is a duration such as `5m` or `7d`".into());
                None
            }
        };
        if self.on.is_empty() {
            errs.push("`on` names the event kinds to read".into());
        }
        match (self.reduce, &self.of) {
            (Reduce::Count, Some(_)) => errs.push("`count` counts events and takes no `of`".into()),
            (Reduce::Count, None) => {}
            (_, None) => errs.push(format!("`{:?}` needs an `of`", self.reduce).to_lowercase()),
            _ => {}
        }
        if self.unit != Unit::N && !matches!(self.reduce, Reduce::Sum | Reduce::Last | Reduce::Max | Reduce::Set) {
            errs.push("`unit` has no meaning for this `reduce`".into());
        }
        if self.window == Some(0) {
            errs.push("`window` is at least 1".into());
        }
        if !errs.is_empty() {
            return Err(errs);
        }
        let seed = format!("{FORMAT}|{self:?}");
        let digest = Sha256::digest(seed.as_bytes());
        let version = u64::from_be_bytes(digest[..8].try_into().unwrap_or([0; 8]));
        Ok(DeclFold {
            id: self.id.clone(),
            version,
            on: self.on.clone(),
            reduce: self.reduce,
            unit: self.unit,
            of,
            key,
            when,
            window: self.window.map(|w| w as usize),
            within,
            state: State::default(),
        })
    }
}

/// The folds the tables declare for one log. A declaration that does not compile is a problem and no fold.
pub fn declared(tables: &Tables, scope: Scope) -> (Vec<Box<dyn Fold>>, Vec<Problem>) {
    let (mut folds, mut problems) = (Vec::<Box<dyn Fold>>::new(), Vec::new());
    for id in tables.ids(FoldDecl::NAME) {
        match tables.get::<FoldDecl>(id) {
            Ok(d) if d.scope == scope => match d.compile() {
                Ok(f) => folds.push(Box::new(f)),
                Err(e) => problems.push(Problem::refused(format!("fold {id}"), e.join("; "))),
            },
            Ok(_) => {}
            Err(p) => problems.push(p),
        }
    }
    (folds, problems)
}

impl DeclFold {
    fn windowed(&self) -> bool {
        self.window.is_some() || self.within.is_some()
    }

    fn value_of(&self, cell: &Cell, now: u64) -> Value {
        match cell {
            Cell::Live(acc) => acc.out(self.reduce, self.unit),
            Cell::Window(items) => {
                let mut acc = Acc::new(self.reduce);
                for (at, v) in items.iter().filter(|(at, _)| self.within.is_none_or(|w| at.saturating_add(w) >= now)) {
                    let _ = at;
                    acc.push(&v.0);
                }
                acc.out(self.reduce, self.unit)
            }
        }
    }

    fn typed(&self, v: Value) -> Value {
        match (self.unit, &v) {
            (Unit::Usd, Value::Num(d)) => Value::usd(*d),
            (Unit::Dur, Value::Num(ms)) if *ms >= 0.0 => Value::Dur(ms.round() as u64),
            _ => v,
        }
    }
}

impl Fold for DeclFold {
    fn id(&self) -> &str {
        &self.id
    }

    fn version(&self) -> u64 {
        self.version
    }

    fn wants(&self, kind: &str) -> bool {
        glob_match(&self.on, kind)
    }

    fn apply(&mut self, ev: &Ev) {
        let row = ev.row();
        let env = EvEnv(&row, ev.at_ms);
        if self.when.as_ref().is_some_and(|w| !w.holds(&env)) {
            return;
        }
        let key = match &self.key {
            None => String::new(),
            Some(f) => {
                let missing = f.pieces().iter().any(|p| matches!(p, Piece::Ref { expr, .. } if matches!(expr.eval(&env), Value::Missing)));
                if missing {
                    return;
                }
                f.render(&env)
            }
        };
        let item = match (&self.of, self.reduce) {
            (_, Reduce::Count) | (None, _) => Value::Bool(true),
            (Some(of), _) => match of.eval(&env) {
                Value::Missing => return,
                v if self.reduce == Reduce::Streak => Value::Bool(v.truthy()),
                v => self.typed(v),
            },
        };
        if !self.state.cells.contains_key(&key) && self.state.cells.len() >= MAX_KEYS {
            self.state.capped_keys = true;
            return;
        }
        let (reduce, windowed) = (self.reduce, self.windowed());
        let cell = self.state.cells.entry(key).or_insert_with(|| {
            if windowed { Cell::Window(VecDeque::new()) } else { Cell::Live(Acc::new(reduce)) }
        });
        match cell {
            Cell::Live(acc) => acc.push(&item),
            Cell::Window(items) => {
                items.push_back((ev.at_ms, V(item)));
                if let Some(w) = self.window {
                    while items.len() > w {
                        items.pop_front();
                    }
                }
                if let Some(ms) = self.within {
                    while items.front().is_some_and(|(at, _)| at.saturating_add(ms) < ev.at_ms) {
                        items.pop_front();
                    }
                }
                if items.len() > MAX_WINDOW {
                    items.pop_front();
                    self.state.capped_window = true;
                }
            }
        }
    }

    fn read(&self, path: &[Key], now_ms: u64) -> Value {
        let one = |cell: &Cell| self.value_of(cell, now_ms);
        if self.key.is_none() {
            let v = match self.state.cells.get("") {
                Some(cell) => one(cell),
                None => one(&if self.windowed() { Cell::Window(VecDeque::new()) } else { Cell::Live(Acc::new(self.reduce)) }),
            };
            return v.at(path);
        }
        match path.split_first() {
            None => Value::Map(self.state.cells.iter().map(|(k, c)| (k.clone(), one(c))).collect()),
            Some((first, rest)) => {
                let key = match first {
                    Key::Name(n) => n.clone(),
                    Key::Idx(i) => i.to_string(),
                };
                self.state.cells.get(&key).map(|c| one(c).at(rest)).unwrap_or_default()
            }
        }
    }

    fn save(&self) -> Json {
        serde_json::to_value(&self.state).unwrap_or_default()
    }

    fn load(&mut self, state: Json) -> Result<(), String> {
        self.state = serde_json::from_value(state).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn reset(&mut self) {
        self.state = State::default();
    }

    fn notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        if self.state.capped_keys {
            notes.push(format!("more than {MAX_KEYS} keys: the later ones are not kept"));
        }
        if self.state.capped_window {
            notes.push(format!("a key had more than {MAX_WINDOW} events in its window: the oldest were dropped"));
        }
        if self.state.cells.values().any(|c| matches!(c, Cell::Live(a) if a.capped())) {
            notes.push(format!("a set holds {MAX_SET} values: the later ones are not kept"));
        }
        notes
    }
}
