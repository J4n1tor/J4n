//! Query: which rows, in what order, how many. `from` names a collection; `filter` and `order`
//! are [`Expr`]s read once per row; `limit` is the most rows. This module reads and runs the *shape*: where a
//! collection's rows come from is the resolver's, which hands them in as [`Value`]s.
//!
//! In a `filter` or an `order` key a name is first a root of the outer [`Env`] (`fold.spend`, `set.loop`), which no
//! field of a row can shadow, and otherwise a field of the row (`state`, `data.cmd`); `row` is the whole row, and a
//! pointer reads into it.

use crate::eval::Env;
use crate::expr::Expr;
use crate::value::{Key, Value};
use std::cmp::Ordering;
use std::fmt;

/// The most rows a `limit` may ask for.
pub const MAX_LIMIT: u64 = 10_000;
/// The most keys an `order` may have.
pub const MAX_ORDER: usize = 4;

/// What a query reads from. Written `tasks`, `asks`, `changes`, `log:<kind glob>`, `table:<kind>`, `fold:<id>`,
/// `thread:<id>`; names are letters, digits and `_ . -` (a glob may also hold `*` and `?`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Coll {
    Tasks,
    Asks,
    Changes,
    Log(String),
    Table(String),
    Fold(String),
    Thread(String),
}

/// Letters, digits and `_ . -` (and `extra`), starting with a letter, a digit, `_` or `extra`: never `.`, `..` or `-x`.
fn name_ok(s: &str, extra: &str) -> bool {
    let start = |c: char| c.is_ascii_alphanumeric() || c == '_' || extra.contains(c);
    s.chars().next().is_some_and(start) && s.chars().all(|c| start(c) || ".-".contains(c))
}

impl Coll {
    pub fn parse(text: &str) -> Result<Coll, String> {
        let (head, arg) = match text.split_once(':') {
            Some((h, a)) => (h, Some(a)),
            None => (text, None),
        };
        let arg_for = |extra: &str| match arg {
            Some(a) if name_ok(a, extra) => Ok(a.to_string()),
            Some(_) => Err(format!(
                "{text:?}: a name after `{head}:` is letters, digits and `_ . -`{}, starting with a letter, a digit or `_`",
                if extra.is_empty() { "" } else { " (and `*` `?`)" }
            )),
            None => Err(format!("{text:?}: `{head}` needs a name after it: `{head}:<name>`")),
        };
        match (head, arg) {
            ("tasks", None) => Ok(Coll::Tasks),
            ("asks", None) => Ok(Coll::Asks),
            ("changes", None) => Ok(Coll::Changes),
            ("tasks" | "asks" | "changes", Some(_)) => Err(format!("{text:?}: `{head}` takes nothing after it")),
            ("log", _) => arg_for("*?").map(Coll::Log),
            ("table", _) => arg_for("").map(Coll::Table),
            ("fold", _) => arg_for("").map(Coll::Fold),
            ("thread", _) => arg_for("").map(Coll::Thread),
            _ => Err(format!(
                "{text:?} is not a collection (tasks, asks, changes, log:<glob>, table:<kind>, fold:<id>, thread:<id>)"
            )),
        }
    }
}

impl fmt::Display for Coll {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Coll::Tasks => f.write_str("tasks"),
            Coll::Asks => f.write_str("asks"),
            Coll::Changes => f.write_str("changes"),
            Coll::Log(g) => write!(f, "log:{g}"),
            Coll::Table(k) => write!(f, "table:{k}"),
            Coll::Fold(id) => write!(f, "fold:{id}"),
            Coll::Thread(id) => write!(f, "thread:{id}"),
        }
    }
}

/// One sort key: an expression over the row, ascending or (`-` before it) descending. A row where it is missing sorts
/// last either way.
#[derive(Debug, Clone, PartialEq)]
pub struct Order {
    pub key: Expr,
    pub desc: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub from: Coll,
    pub filter: Option<Expr>,
    pub order: Vec<Order>,
    pub limit: Option<usize>,
}

/// Split at the commas that are not inside quotes, parentheses or brackets.
fn split_top(text: &str) -> Vec<String> {
    let (mut parts, mut cur, mut depth, mut quote, mut escaped) = (Vec::new(), String::new(), 0i32, None, false);
    for c in text.chars() {
        match (quote, c) {
            (Some(_), _) if escaped => escaped = false,
            (Some(_), '\\') => escaped = true,
            (Some(q), c) if c == q => quote = None,
            (None, '\'' | '"') => quote = Some(c),
            (None, '(' | '[') => depth += 1,
            (None, ')' | ']') => depth -= 1,
            (None, ',') if depth == 0 => {
                parts.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    parts.push(cur);
    parts
}

fn parse_order(text: &str) -> Result<Vec<Order>, Vec<String>> {
    let parts = split_top(text);
    if parts.len() > MAX_ORDER {
        return Err(vec![format!("{} keys — at most {MAX_ORDER}", parts.len())]);
    }
    let (mut keys, mut problems) = (Vec::new(), Vec::new());
    for part in parts.iter().map(|p| p.trim()) {
        let (desc, body) = part.strip_prefix('-').map_or((false, part), |rest| (true, rest));
        match Expr::parse(body) {
            Ok(key) => keys.push(Order { key, desc }),
            Err(e) => problems.push(format!("`{part}`: {e}")),
        }
    }
    if problems.is_empty() { Ok(keys) } else { Err(problems) }
}

/// An [`Env`] that reads a name from `outer` when `outer` has it as a root, else as a field of the row (`row` is the
/// whole row): what a [`Query`] filters with, and what a template for the row should render with, so the two read the
/// same things.
pub struct RowEnv<'a> {
    row: &'a Value,
    outer: &'a dyn Env,
}

impl<'a> RowEnv<'a> {
    pub fn new(row: &'a Value, outer: &'a dyn Env) -> RowEnv<'a> {
        RowEnv { row, outer }
    }
}

impl Env for RowEnv<'_> {
    fn get(&self, root: &str, path: &[Key]) -> Value {
        let field = match self.row {
            Value::Map(fields) if root != "row" && self.outer.get(root, &[]) == Value::Missing => fields.get(root),
            _ => None,
        };
        match (root, field) {
            ("row", _) => self.row.at(path),
            (_, Some(value)) => value.at(path),
            _ => self.outer.get(root, path),
        }
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

fn by_keys(order: &[Order], a: &[Value], b: &[Value]) -> Ordering {
    for ((o, x), y) in order.iter().zip(a).zip(b) {
        let ord = match (x, y) {
            (Value::Missing, Value::Missing) => Ordering::Equal,
            (Value::Missing, _) => return Ordering::Greater,
            (_, Value::Missing) => return Ordering::Less,
            _ if o.desc => x.sort_cmp(y).reverse(),
            _ => x.sort_cmp(y),
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    Ordering::Equal
}

impl Query {
    /// Read the four parts as a pack writes them. Every mistake is reported at once.
    pub fn parse(
        from: &str,
        filter: Option<&str>,
        order: Option<&str>,
        limit: Option<u64>,
    ) -> Result<Query, Vec<String>> {
        let mut problems = Vec::new();
        let mut note = |what: &str, e: String| problems.push(format!("{what}: {e}"));
        let from = Coll::parse(from).map_err(|e| note("from", e)).ok();
        let filter = filter.and_then(|f| Expr::parse(f).map_err(|e| note("filter", e.to_string())).ok());
        let order = order.map(|o| {
            parse_order(o).unwrap_or_else(|all| {
                all.into_iter().for_each(|e| note("order", e));
                Vec::new()
            })
        });
        match limit {
            Some(0) => note("limit", "at least 1".into()),
            Some(n) if n > MAX_LIMIT => note("limit", format!("{n} rows — at most {MAX_LIMIT}")),
            _ => {}
        }
        match (from, problems.is_empty()) {
            (Some(from), true) => {
                Ok(Query { from, filter, order: order.unwrap_or_default(), limit: limit.map(|n| n as usize) })
            }
            _ => Err(problems),
        }
    }

    /// What [`Expr::check`] finds in the filter and the keys. `names` are the roots the outer [`Env`] has (`row` is
    /// always one); none given: any name is fine.
    pub fn check(&self, names: Option<&[&str]>) -> Vec<String> {
        let known: Option<Vec<&str>> = names.map(|n| n.iter().copied().chain(["row"]).collect());
        let keys = self.order.iter().map(|o| ("order", &o.key));
        let all = self.filter.iter().map(|f| ("filter", f)).chain(keys);
        all.flat_map(|(what, e)| e.check(known.as_deref()).into_iter().map(move |p| format!("{what}: {p}"))).collect()
    }

    /// Keep the rows the filter holds for, in the order asked (rows that tie keep their place), at most `limit`.
    pub fn apply(&self, rows: Vec<Value>, outer: &dyn Env) -> Vec<Value> {
        let mut kept: Vec<(Vec<Value>, Value)> = rows
            .into_iter()
            .filter(|row| self.filter.as_ref().is_none_or(|f| f.holds(&RowEnv::new(row, outer))))
            .map(|row| (self.order.iter().map(|o| o.key.eval(&RowEnv::new(&row, outer))).collect(), row))
            .collect();
        kept.sort_by(|(a, _), (b, _)| by_keys(&self.order, a, b));
        kept.into_iter().map(|(_, row)| row).take(self.limit.unwrap_or(usize::MAX)).collect()
    }
}
