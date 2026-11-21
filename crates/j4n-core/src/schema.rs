//! Schema: what the fields of a declared thing may hold (the TUI's field components
//! are exactly these types). A kind is a table of named fields; each has a type and a few flags. A schema is data
//! — it is written in TOML inside a `[[kind]]` entry — and the same code checks any entry against it.
//!
//! # The written form
//!
//! A field is a type word, or a table that holds the type and the flags:
//!
//! ```toml
//! [kind.fields]
//! name   = "str"                                       # the short form: an optional field of that type
//! size   = { ty = "int", min = 1, max = 9, default = 3 }
//! lamps  = { ty = "list<ref:lamp>", required = true, doc = "the lamps it carries" }
//! shape  = { ty = "obj", fields = { w = "int", h = "int" } }
//! ```
//!
//! | type | holds |
//! |---|---|
//! | `str` · `text` · `markdown` | a line (≤ 4,096 chars) · text (≤ 100,000) · markdown; control characters other than tab and, in `text`, line ends are refused |
//! | `path` · `pattern` | a path (not empty, not starting with `-`, as it may land on a command line) · a glob (not empty) |
//! | `argv` | a list of text, the first not empty |
//! | `int` · `num` | a whole number · a finite number, within `min` and `max` when given |
//! | `bool` | yes or no |
//! | `dur` · `usd` | a duration (`"800ms"`, `"1h30m"`) · money (`"$1.50"`), written as the text, read exactly |
//! | `enum:a\|b` · `multi:a\|b` | one of the words · a list of them, none twice (table form: `ty = "enum", options = [...]`) |
//! | `list<T>` · `map<T>` | a list of T (≤ 10,000) · a table of text keys to T (table form: `ty = "list", of = T`) |
//! | `obj` | a table with exactly the declared `fields`; one not declared is refused |
//! | `ref:kind` | the id of an entry of that kind; one that is not there is a warning, not a refusal |
//! | `secret` | a reference — `env:NAME` or `file:/path` — never the secret itself |
//! | `expr` · `fmt` · `query` | text that parses as an [`Expr`], a [`Fmt`] template, a [`Query`] (a table of `from`, `filter`, `order`, `limit`); `roots = [...]` lists the names it may read |
//! | `any` | any value (no dates, no infinite numbers, nested at most 12 deep) |
//! | `fields` | a table of field specs: what the `fields` of a `[[kind]]` entry is |
//!
//! Flags: `required`, `default` (it must fit the type; not with `required`), `doc`, `class` (`public project private
//! secret`; a `secret` is always the last), `floor` (a layer above the first that sets it cannot change it),
//! `who_may` (the actors that may write it: `operator core rule doer model pack`; default operator and core),
//! `widens` (`true`, or a list of the enum words that do: writing one is a widening, which a trust-less project layer
//! may not do and the Gate arms with two presses).
//!
//! Types nest at most 4 deep. The field name `id` is every entry's own and cannot be declared.

use crate::change::ActorKind;
use crate::expr::Expr;
use crate::fmt::Fmt;
use crate::query::Query;
use crate::value::Value;
use std::collections::{BTreeMap, BTreeSet};
use toml::{Table, Value as Toml};

/// How deep types nest in a schema.
pub const MAX_TYPE_DEPTH: usize = 4;
/// How deep a value nests in an entry.
pub const MAX_VALUE_DEPTH: usize = 12;
/// The most items in a list or a table.
pub const MAX_ITEMS: usize = 10_000;
/// The longest `str`, `path` and `pattern`; and the longest `text` and `markdown`.
pub const MAX_LINE: usize = 4_096;
pub const MAX_TEXT: usize = 100_000;
/// The most fields in a kind.
pub const MAX_FIELDS: usize = 200;

/// Where data may go: the egress Gate checks classes against provider zones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DataClass {
    Public,
    #[default]
    Project,
    Private,
    Secret,
}

impl DataClass {
    fn from_word(w: &str) -> Option<DataClass> {
        [DataClass::Public, DataClass::Project, DataClass::Private, DataClass::Secret]
            .into_iter()
            .find(|c| c.word() == w)
    }

    pub fn word(self) -> &'static str {
        match self {
            DataClass::Public => "public",
            DataClass::Project => "project",
            DataClass::Private => "private",
            DataClass::Secret => "secret",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    Str,
    Text,
    Markdown,
    Path,
    Pattern,
    Argv,
    Bool,
    Dur,
    Usd,
    Secret,
    Any,
    Fields,
    Int { min: Option<i64>, max: Option<i64> },
    Num { min: Option<f64>, max: Option<f64> },
    Enum(Vec<String>),
    Multi(Vec<String>),
    List(Box<Ty>),
    Map(Box<Ty>),
    Obj(BTreeMap<String, Field>),
    Ref(String),
    Expr(Option<Vec<String>>),
    Fmt(Option<Vec<String>>),
    Query(Option<Vec<String>>),
}

/// What writing a field does to what J4n may do.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Widens {
    #[default]
    No,
    /// Any value is a widening.
    Any,
    /// These enum words are.
    Options(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub ty: Ty,
    pub required: bool,
    pub default: Option<Toml>,
    pub doc: String,
    pub class: DataClass,
    pub floor: bool,
    pub who_may: Option<Vec<ActorKind>>,
    pub widens: Widens,
}

impl Field {
    fn of(ty: Ty) -> Field {
        let class = if ty == Ty::Secret { DataClass::Secret } else { DataClass::default() };
        Field {
            ty,
            required: false,
            default: None,
            doc: String::new(),
            class,
            floor: false,
            who_may: None,
            widens: Widens::No,
        }
    }

    /// May `actor` write this field? The default is the operator and the core.
    pub fn may_write(&self, actor: ActorKind) -> bool {
        match &self.who_may {
            Some(list) => list.contains(&actor),
            None => matches!(actor, ActorKind::Operator | ActorKind::Core),
        }
    }
}

/// The words of a reference to a thing that is not there; [`is_dangling`] reads them back (`j4n pack check` fails a pack on one).
pub fn dangling(at: &str, kind: &str, id: &str) -> String {
    format!("{at}: no {kind} `{id}`")
}

/// Whether `msg` is what [`dangling`] says.
pub fn is_dangling(msg: &str) -> bool {
    msg.rsplit_once(": no ").is_some_and(|(_, rest)| rest.strip_suffix('`').and_then(|r| r.split_once(" `")).is_some_and(|(kind, id)| is_id(kind) && is_id(id)))
}

/// What [`Schema::check`] found: refusals, and references to things that are not there (warnings).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Checked {
    pub errors: Vec<String>,
    pub dangling: Vec<String>,
}

/// Does a partial entry (one layer's say) have to hold the required fields? `Whole` is the merged entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Partial,
    Whole,
}

/// The fields of one kind.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Schema {
    pub fields: BTreeMap<String, Field>,
}

const FLAGS: [&str; 7] = ["required", "default", "doc", "class", "floor", "who_may", "widens"];
const PARAMS: [&str; 7] = ["ty", "min", "max", "options", "of", "fields", "to"];

/// `a-z`, digits and `_`, starting with a letter, at most 48 characters: a field name, and a kind name up to 32.
pub fn is_name(s: &str) -> bool {
    s.len() <= 48
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// An id: ASCII letters, digits and `_ - . : /`, starting with a letter or digit, at most 96 characters.
pub fn is_id(s: &str) -> bool {
    s.len() <= 96
        && s.starts_with(|c: char| c.is_ascii_alphanumeric())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || "_-.:/".contains(c))
}

fn is_word(s: &str) -> bool {
    s.len() <= 48
        && s.starts_with(|c: char| c.is_ascii_alphanumeric())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || "_-.:".contains(c))
}

fn words(list: &[String], at: &str) -> Result<Vec<String>, String> {
    let mut seen = BTreeSet::new();
    for w in list {
        if !is_word(w) {
            return Err(format!("{at}: `{w}` is not an option word (letters, digits, `_ - . :`)"));
        }
        if !seen.insert(w) {
            return Err(format!("{at}: `{w}` is listed twice"));
        }
    }
    if list.is_empty() {
        return Err(format!("{at}: options are empty"));
    }
    Ok(list.to_vec())
}

fn ty_word(s: &str, at: &str, depth: usize) -> Result<Ty, String> {
    if depth > MAX_TYPE_DEPTH {
        return Err(format!("{at}: types nest at most {MAX_TYPE_DEPTH} deep"));
    }
    let inner = |open: &str| s.strip_prefix(open).and_then(|r| r.strip_suffix('>'));
    if let Some(i) = inner("list<") {
        return Ok(Ty::List(Box::new(ty_word(i, at, depth + 1)?)));
    }
    if let Some(i) = inner("map<") {
        return Ok(Ty::Map(Box::new(ty_word(i, at, depth + 1)?)));
    }
    if let Some(kind) = s.strip_prefix("ref:") {
        return if is_name(kind) {
            Ok(Ty::Ref(kind.to_string()))
        } else {
            Err(format!("{at}: `{kind}` is not a kind name"))
        };
    }
    for (prefix, make) in [("enum:", Ty::Enum as fn(Vec<String>) -> Ty), ("multi:", Ty::Multi as fn(Vec<String>) -> Ty)]
    {
        if let Some(options) = s.strip_prefix(prefix) {
            let list: Vec<String> = options.split('|').map(str::to_string).collect();
            return words(&list, at).map(make);
        }
    }
    Ok(match s {
        "str" => Ty::Str,
        "text" => Ty::Text,
        "markdown" => Ty::Markdown,
        "path" => Ty::Path,
        "pattern" => Ty::Pattern,
        "argv" => Ty::Argv,
        "bool" => Ty::Bool,
        "dur" => Ty::Dur,
        "usd" => Ty::Usd,
        "secret" => Ty::Secret,
        "any" => Ty::Any,
        "fields" => Ty::Fields,
        "int" => Ty::Int { min: None, max: None },
        "num" => Ty::Num { min: None, max: None },
        "expr" => Ty::Expr(None),
        "fmt" => Ty::Fmt(None),
        "query" => Ty::Query(None),
        "enum" | "multi" => return Err(format!("{at}: `{s}` needs `options` (or write `{s}:a|b`)")),
        "list" | "map" => return Err(format!("{at}: `{s}` needs `of` (or write `{s}<T>`)")),
        "obj" => return Err(format!("{at}: `obj` needs `fields`")),
        "ref" => return Err(format!("{at}: `ref` needs `to` (or write `ref:kind`)")),
        other => return Err(format!("{at}: `{other}` is not a type")),
    })
}

fn strings(v: &Toml, key: &str, at: &str) -> Result<Vec<String>, String> {
    let items = v.as_array().ok_or_else(|| format!("{at}: `{key}` is a list of text"))?;
    items
        .iter()
        .map(|i| i.as_str().map(str::to_string).ok_or_else(|| format!("{at}: `{key}` holds {i}, which is not text")))
        .collect()
}

fn number(t: &Table, key: &str, at: &str, whole: bool) -> Result<Option<f64>, String> {
    match t.get(key) {
        None => Ok(None),
        Some(Toml::Integer(i)) => Ok(Some(*i as f64)),
        Some(Toml::Float(f)) if !whole && f.is_finite() => Ok(Some(*f)),
        Some(v) => Err(format!("{at}: `{key}` is {}, not a {}", v, if whole { "whole number" } else { "number" })),
    }
}

/// The type a table spells out; the parameters it used are the ones that apply.
fn ty_table(t: &Table, at: &str, depth: usize) -> Result<Ty, String> {
    let name = t.get("ty").and_then(Toml::as_str).ok_or_else(|| format!("{at}: needs `ty`, the type"))?;
    let mut used = vec!["ty"];
    let mut ty = match name {
        "enum" | "multi" => {
            let list =
                strings(t.get("options").ok_or_else(|| format!("{at}: `{name}` needs `options`"))?, "options", at)?;
            used.push("options");
            let list = words(&list, at)?;
            if name == "enum" { Ty::Enum(list) } else { Ty::Multi(list) }
        }
        "list" | "map" => {
            let of = t.get("of").ok_or_else(|| format!("{at}: `{name}` needs `of`, the type of its items"))?;
            used.push("of");
            let inner = Box::new(nested(of, at, depth + 1)?);
            if name == "list" { Ty::List(inner) } else { Ty::Map(inner) }
        }
        "obj" => {
            let spec = t
                .get("fields")
                .and_then(Toml::as_table)
                .ok_or_else(|| format!("{at}: `obj` needs `fields`, a table"))?;
            used.push("fields");
            Ty::Obj(fields(spec, at, depth + 1).map_err(|e| e.join("; "))?)
        }
        "ref" => {
            let to = t.get("to").and_then(Toml::as_str).ok_or_else(|| format!("{at}: `ref` needs `to`, a kind"))?;
            used.push("to");
            ty_word(&format!("ref:{to}"), at, depth)?
        }
        other => ty_word(other, at, depth)?,
    };
    match &mut ty {
        Ty::Int { min, max } => {
            let (lo, hi) = (number(t, "min", at, true)?, number(t, "max", at, true)?);
            (*min, *max) = (lo.map(|n| n as i64), hi.map(|n| n as i64));
            used.extend(["min", "max"]);
            if min.zip(*max).is_some_and(|(a, b)| a > b) {
                return Err(format!("{at}: `min` is above `max`"));
            }
        }
        Ty::Num { min, max } => {
            (*min, *max) = (number(t, "min", at, false)?, number(t, "max", at, false)?);
            used.extend(["min", "max"]);
            if min.zip(*max).is_some_and(|(a, b)| a > b) {
                return Err(format!("{at}: `min` is above `max`"));
            }
        }
        Ty::Expr(r) | Ty::Fmt(r) | Ty::Query(r) => {
            if let Some(list) = t.get("roots") {
                *r = Some(strings(list, "roots", at)?);
            }
            used.push("roots");
        }
        _ => {}
    }
    if let Some(stray) = PARAMS.iter().chain(["roots"].iter()).find(|k| t.contains_key(**k) && !used.contains(k)) {
        return Err(format!("{at}: `{stray}` does not apply to `{name}`"));
    }
    Ok(ty)
}

/// A type inside a type (`of`): a word, or a table of `ty` and its parameters but no flags.
fn nested(v: &Toml, at: &str, depth: usize) -> Result<Ty, String> {
    if depth > MAX_TYPE_DEPTH {
        return Err(format!("{at}: types nest at most {MAX_TYPE_DEPTH} deep"));
    }
    match v {
        Toml::String(s) => ty_word(s, at, depth),
        Toml::Table(t) => match FLAGS.iter().find(|f| t.contains_key(**f)) {
            Some(flag) => Err(format!("{at}: `{flag}` belongs to a field, not to the type of its items")),
            None => ty_table(t, at, depth),
        },
        _ => Err(format!("{at}: a type is a word or a table with `ty`")),
    }
}

fn field(v: &Toml, at: &str, depth: usize) -> Result<Field, String> {
    let Toml::Table(t) = v else {
        return nested(v, at, depth).map(Field::of);
    };
    if let Some(stray) =
        t.keys().find(|k| !FLAGS.contains(&k.as_str()) && !PARAMS.contains(&k.as_str()) && *k != "roots")
    {
        return Err(format!(
            "{at}: `{stray}` is not a field key (a field holds {})",
            [FLAGS.as_slice(), PARAMS.as_slice(), &["roots"]].concat().join(", ")
        ));
    }
    let mut f = Field::of(ty_table(t, at, depth)?);
    let flag = |key: &str, what: &str| -> Result<Option<bool>, String> {
        match t.get(key) {
            None => Ok(None),
            Some(Toml::Boolean(b)) => Ok(Some(*b)),
            Some(_) => Err(format!("{at}: `{key}` is {what}")),
        }
    };
    f.required = flag("required", "true or false")?.unwrap_or(false);
    f.floor = flag("floor", "true or false")?.unwrap_or(false);
    if let Some(doc) = t.get("doc") {
        f.doc = doc.as_str().ok_or_else(|| format!("{at}: `doc` is text"))?.to_string();
    }
    if let Some(class) = t.get("class") {
        let word = class.as_str().and_then(DataClass::from_word);
        f.class = word.ok_or_else(|| format!("{at}: `class` is public, project, private or secret"))?;
        if f.ty == Ty::Secret && f.class != DataClass::Secret {
            return Err(format!("{at}: a secret is always class secret"));
        }
    }
    if let Some(who) = t.get("who_may") {
        let list = strings(who, "who_may", at)?;
        let kinds: Option<Vec<ActorKind>> = list.iter().map(|w| ActorKind::from_word(w)).collect();
        f.who_may = Some(
            kinds
                .filter(|k| !k.is_empty())
                .ok_or_else(|| format!("{at}: `who_may` lists operator, core, rule, doer, model, pack"))?,
        );
    }
    match t.get("widens") {
        None | Some(Toml::Boolean(false)) => {}
        Some(Toml::Boolean(true)) => f.widens = Widens::Any,
        Some(list) => {
            let options = match &f.ty {
                Ty::Enum(o) | Ty::Multi(o) => o,
                _ => return Err(format!("{at}: only an enum or multi names the words that widen")),
            };
            let named = strings(list, "widens", at)?;
            if let Some(w) = named.iter().find(|w| !options.contains(w)) {
                return Err(format!("{at}: `widens` names `{w}`, which is not an option"));
            }
            f.widens = Widens::Options(named);
        }
    }
    if let Some(d) = t.get("default") {
        if f.required {
            return Err(format!("{at}: a required field has no default"));
        }
        let mut found = Checked::default();
        f.ty.check(d, at, &|_, _| true, &mut found);
        if let Some(e) = found.errors.first() {
            return Err(format!("the default of {e}"));
        }
        f.default = Some(d.clone());
    }
    Ok(f)
}

fn fields(spec: &Table, at: &str, depth: usize) -> Result<BTreeMap<String, Field>, Vec<String>> {
    let mut out = BTreeMap::new();
    let mut problems = Vec::new();
    if spec.len() > MAX_FIELDS {
        problems.push(format!("{at}: {} fields — at most {MAX_FIELDS}", spec.len()));
    }
    for (name, v) in spec {
        let here = if at.is_empty() { name.clone() } else { format!("{at}.{name}") };
        if !is_name(name) {
            problems.push(format!("{here}: not a field name (a-z, digits, `_`, starting with a letter, ≤ 48)"));
        } else if name == "id" && at.is_empty() {
            problems.push("id: every entry has an `id`; it is not declared".to_string());
        } else {
            match field(v, &here, depth) {
                Ok(f) => {
                    out.insert(name.clone(), f);
                }
                Err(e) => problems.push(e),
            }
        }
    }
    if problems.is_empty() { Ok(out) } else { Err(problems) }
}

/// A value that is not a line or not clean text is refused.
fn clean(s: &str, lines: bool) -> bool {
    s.chars().all(|c| !c.is_control() || c == '\t' || (lines && (c == '\n' || c == '\r')))
}

/// A duration or money written as text: the value it is, read the way an expression reads it.
fn literal(s: &str) -> Option<Value> {
    if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '$' || c == '.') {
        return None;
    }
    match Expr::parse(s) {
        Ok(Expr::Lit(v @ (Value::Dur(_) | Value::Usd(_)))) => Some(v),
        _ => None,
    }
}

/// `env:NAME` or `file:/path` (or `file:~/path`), or `kept:<provider>`: the key J4n keeps for a provider, by a name that can be a file's.
pub fn is_secret_ref(s: &str) -> bool {
    match s.split_once(':') {
        Some(("kept", id)) => crate::keys::fits(id),
        Some(("env", name)) => {
            name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        Some(("file", path)) => {
            (path.starts_with('/') || path.starts_with("~/")) && clean(path, false) && path.len() <= MAX_LINE
        }
        _ => false,
    }
}

fn got(v: &Toml) -> &'static str {
    match v {
        Toml::String(_) => "text",
        Toml::Integer(_) => "a whole number",
        Toml::Float(_) => "a number",
        Toml::Boolean(_) => "a yes/no",
        Toml::Datetime(_) => "a date",
        Toml::Array(_) => "a list",
        Toml::Table(_) => "a table",
    }
}

/// Whether `v` is safe to hold: no dates (the Log is JSON), no infinite numbers, nested at most [`MAX_VALUE_DEPTH`].
pub fn sane(v: &Toml, depth: usize) -> Result<(), String> {
    if depth > MAX_VALUE_DEPTH {
        return Err(format!("nested deeper than {MAX_VALUE_DEPTH} levels"));
    }
    match v {
        Toml::Datetime(_) => Err("a date is not a value here — write text, or milliseconds".to_string()),
        Toml::Float(f) if !f.is_finite() => Err("a number that is not finite".to_string()),
        Toml::Array(a) => a.iter().try_for_each(|i| sane(i, depth + 1)),
        Toml::Table(t) => t.values().try_for_each(|i| sane(i, depth + 1)),
        _ => Ok(()),
    }
}

impl Ty {
    fn check(&self, v: &Toml, at: &str, known: &dyn Fn(&str, &str) -> bool, out: &mut Checked) {
        let mut bad = |what: String| out.errors.push(format!("{at}: {what}"));
        let want = |w: &str| format!("expected {w}, got {}", got(v));
        let text = |max: usize, lines: bool| -> Result<&str, String> {
            let s = v.as_str().ok_or_else(|| want("text"))?;
            if s.chars().count() > max {
                return Err(format!("longer than {max} characters"));
            }
            if !clean(s, lines) {
                return Err(if lines {
                    "holds a control character".to_string()
                } else {
                    "is more than one line, or holds a control character".to_string()
                });
            }
            Ok(s)
        };
        match self {
            Ty::Str => {
                if let Err(e) = text(MAX_LINE, false) {
                    bad(e);
                }
            }
            Ty::Text | Ty::Markdown => {
                if let Err(e) = text(MAX_TEXT, true) {
                    bad(e);
                }
            }
            Ty::Path => match text(MAX_LINE, false) {
                Ok(s) if s.is_empty() || s.starts_with('-') => {
                    bad(format!("{s:?} is not a path (not empty, not starting with `-`)"))
                }
                Ok(_) => {}
                Err(e) => bad(e),
            },
            Ty::Pattern => match text(MAX_LINE, false) {
                Ok("") => bad("a pattern is not empty".to_string()),
                Ok(_) => {}
                Err(e) => bad(e),
            },
            Ty::Argv => match v.as_array() {
                None => bad(want("a list of text")),
                Some(a) if a.is_empty() || a.len() > 1000 => bad("an argv holds 1 to 1000 items".to_string()),
                Some(a) => {
                    for (i, item) in a.iter().enumerate() {
                        match item.as_str() {
                            Some(s) if clean(s, false) && s.len() <= MAX_LINE && !(i == 0 && s.is_empty()) => {}
                            _ => bad(format!("[{i}]: not text, or empty as the program, or more than a line")),
                        }
                    }
                }
            },
            Ty::Bool => {
                if !v.is_bool() {
                    bad(want("true or false"));
                }
            }
            Ty::Dur | Ty::Usd => match v.as_str() {
                Some(s)
                    if literal(s)
                        .is_some_and(|l| matches!((self, l), (Ty::Dur, Value::Dur(_)) | (Ty::Usd, Value::Usd(_)))) => {}
                _ => bad(if *self == Ty::Dur {
                    want("a duration such as \"800ms\" or \"1h30m\"")
                } else {
                    want("money such as \"$1.50\"")
                }),
            },
            Ty::Secret => {
                if !v.as_str().is_some_and(is_secret_ref) {
                    bad("a secret is a reference, `env:NAME`, `file:/path` or `kept:<provider>` — never the secret itself".to_string());
                }
            }
            Ty::Any => {}
            Ty::Fields => match v.as_table() {
                None => bad(want("a table of fields")),
                Some(t) => {
                    if let Err(problems) = fields(t, "", 0) {
                        out.errors.extend(problems.into_iter().map(|p| format!("{at}.{p}")));
                    }
                }
            },
            Ty::Int { min, max } => match v.as_integer() {
                None => bad(want("a whole number")),
                Some(i) if min.is_some_and(|m| i < m) || max.is_some_and(|m| i > m) => {
                    bad(format!(
                        "{i} is outside {}..{}",
                        min.map_or("any".to_string(), |m| m.to_string()),
                        max.map_or("any".to_string(), |m| m.to_string())
                    ));
                }
                Some(_) => {}
            },
            Ty::Num { min, max } => {
                let n = v.as_float().or_else(|| v.as_integer().map(|i| i as f64));
                match n {
                    None => bad(want("a number")),
                    Some(n) if !n.is_finite() || min.is_some_and(|m| n < m) || max.is_some_and(|m| n > m) => {
                        bad(format!(
                            "{n} is outside {}..{}",
                            min.map_or("any".to_string(), |m| m.to_string()),
                            max.map_or("any".to_string(), |m| m.to_string())
                        ));
                    }
                    Some(_) => {}
                }
            }
            Ty::Enum(options) => match v.as_str() {
                Some(s) if options.iter().any(|o| o == s) => {}
                Some(s) => bad(format!("`{s}` is not one of {}", options.join(", "))),
                None => bad(want("one of the options")),
            },
            Ty::Multi(options) => match strings(v, "", at) {
                Err(_) => bad(want("a list of options")),
                Ok(list) => {
                    let mut seen = BTreeSet::new();
                    for w in &list {
                        if !options.contains(w) {
                            bad(format!("`{w}` is not one of {}", options.join(", ")));
                        } else if !seen.insert(w) {
                            bad(format!("`{w}` is listed twice"));
                        }
                    }
                }
            },
            Ty::List(inner) => match v.as_array() {
                None => bad(want("a list")),
                Some(a) if a.len() > MAX_ITEMS => bad(format!("{} items — at most {MAX_ITEMS}", a.len())),
                Some(a) => {
                    a.iter().enumerate().for_each(|(i, item)| inner.check(item, &format!("{at}[{i}]"), known, out))
                }
            },
            Ty::Map(inner) => match v.as_table() {
                None => bad(want("a table")),
                Some(t) if t.len() > MAX_ITEMS => bad(format!("{} entries — at most {MAX_ITEMS}", t.len())),
                Some(t) => {
                    for (key, item) in t {
                        if key.is_empty() || key.len() > 128 || !clean(key, false) {
                            out.errors.push(format!(
                                "{at}: the key {key:?} is empty, longer than 128 or holds a control character"
                            ));
                        }
                        inner.check(item, &format!("{at}.{key}"), known, out);
                    }
                }
            },
            Ty::Obj(spec) => match v.as_table() {
                None => bad(want("a table")),
                Some(t) => check_fields(spec, t, Mode::Whole, &format!("{at}."), known, out),
            },
            Ty::Ref(kind) => match v.as_str() {
                Some(id) if is_id(id) => {
                    if !known(kind, id) {
                        out.dangling.push(dangling(at, kind, id));
                    }
                }
                Some(id) => bad(format!("{id:?} is not an id (letters, digits, `_ - . : /`)")),
                None => bad(want("the id of a thing")),
            },
            Ty::Expr(roots) | Ty::Fmt(roots) | Ty::Query(roots) => {
                let names: Option<Vec<&str>> = roots.as_ref().map(|r| r.iter().map(String::as_str).collect());
                for problem in lint(self, v, names.as_deref()) {
                    out.errors.push(format!("{at}: {problem}"));
                }
            }
        }
    }

    /// The value in the language of expressions: a duration is a duration and money is money.
    fn value(&self, v: &Toml) -> Value {
        match (self, v) {
            (Ty::Dur | Ty::Usd, Toml::String(s)) => literal(s).unwrap_or_default(),
            (Ty::List(inner), Toml::Array(a)) => Value::List(a.iter().map(|i| inner.value(i)).collect()),
            (Ty::Map(inner), Toml::Table(t)) => {
                Value::Map(t.iter().map(|(k, i)| (k.clone(), inner.value(i))).collect())
            }
            (Ty::Obj(spec), Toml::Table(t)) => Value::Map(typed(spec, t)),
            _ => plain(v),
        }
    }
}

/// What is wrong with the text of an `expr`, `fmt` or `query` value.
fn lint(ty: &Ty, v: &Toml, roots: Option<&[&str]>) -> Vec<String> {
    let mut out = Vec::new();
    match (ty, v) {
        (Ty::Expr(_), Toml::String(s)) => match Expr::parse(s) {
            Ok(e) => out.extend(e.check(roots)),
            Err(e) => out.push(e.to_string()),
        },
        (Ty::Fmt(_), Toml::String(s)) => match Fmt::parse(s) {
            Ok(f) => out.extend(f.check(roots)),
            Err(e) => out.push(e.to_string()),
        },
        (Ty::Query(_), Toml::Table(t)) => {
            let text = |key: &str| {
                t.get(key).map(|v| v.as_str().map(str::to_string).ok_or_else(|| format!("`{key}` is text")))
            };
            let from = text("from").unwrap_or_else(|| Err("`from` is needed".to_string()));
            let (filter, order) = (text("filter").transpose(), text("order").transpose());
            let limit = match t.get("limit") {
                None => Ok(None),
                Some(Toml::Integer(n)) if *n >= 0 => Ok(Some(*n as u64)),
                Some(_) => Err("`limit` is a whole number, 0 or more".to_string()),
            };
            if let Some(stray) = t.keys().find(|k| !["from", "filter", "order", "limit"].contains(&k.as_str())) {
                out.push(format!("`{stray}` is not part of a query (from, filter, order, limit)"));
            }
            match (from, filter, order, limit) {
                (Ok(from), Ok(filter), Ok(order), Ok(limit)) => {
                    match Query::parse(&from, filter.as_deref(), order.as_deref(), limit) {
                        Ok(q) => out.extend(q.check(roots)),
                        Err(all) => out.extend(all),
                    }
                }
                (a, b, c, d) => out.extend([a.err(), b.err(), c.err(), d.err()].into_iter().flatten()),
            }
        }
        (Ty::Query(_), _) => out.push(format!("expected a table of from, filter, order, limit, got {}", got(v))),
        _ => out.push(format!("expected text, got {}", got(v))),
    }
    out
}

fn plain(v: &Toml) -> Value {
    match v {
        Toml::String(s) => Value::Str(s.clone()),
        Toml::Integer(i) => Value::num(*i as f64),
        Toml::Float(f) => Value::num(*f),
        Toml::Boolean(b) => Value::Bool(*b),
        Toml::Datetime(_) => Value::Missing,
        Toml::Array(a) => Value::List(a.iter().map(plain).collect()),
        Toml::Table(t) => Value::Map(t.iter().map(|(k, i)| (k.clone(), plain(i))).collect()),
    }
}

fn typed(spec: &BTreeMap<String, Field>, t: &Table) -> BTreeMap<String, Value> {
    t.iter().map(|(k, v)| (k.clone(), spec.get(k).map_or_else(|| plain(v), |f| f.ty.value(v)))).collect()
}

fn check_fields(
    spec: &BTreeMap<String, Field>,
    t: &Table,
    mode: Mode,
    prefix: &str,
    known: &dyn Fn(&str, &str) -> bool,
    out: &mut Checked,
) {
    for (name, v) in t {
        match spec.get(name) {
            Some(f) => f.ty.check(v, &format!("{prefix}{name}"), known, out),
            None => {
                let names: Vec<&str> = spec.keys().map(String::as_str).collect();
                let list = if names.is_empty() {
                    "it has no fields".to_string()
                } else {
                    format!("its fields are {}", names.join(", "))
                };
                out.errors.push(format!("{prefix}{name}: not a field of this ({list})"));
            }
        }
    }
    if mode == Mode::Whole {
        out.errors.extend(
            spec.iter()
                .filter(|(n, f)| f.required && !t.contains_key(*n))
                .map(|(n, _)| format!("{prefix}{n}: required")),
        );
    }
}

fn fill_fields(spec: &BTreeMap<String, Field>, t: &mut Table) {
    for (name, f) in spec {
        if !t.contains_key(name)
            && let Some(d) = &f.default
        {
            t.insert(name.clone(), d.clone());
        }
        if let (Ty::Obj(inner), Some(Toml::Table(sub))) = (&f.ty, t.get_mut(name)) {
            fill_fields(inner, sub);
        }
    }
}

fn widening(spec: &BTreeMap<String, Field>, t: &Table, prefix: &str, out: &mut Vec<String>) {
    for (name, v) in t {
        let Some(f) = spec.get(name) else { continue };
        let named = match (&f.widens, v) {
            (Widens::No, _) => false,
            (Widens::Any, _) => true,
            (Widens::Options(o), Toml::String(s)) => o.contains(s),
            (Widens::Options(o), Toml::Array(a)) => {
                a.iter().any(|i| i.as_str().is_some_and(|s| o.contains(&s.to_string())))
            }
            _ => false,
        };
        if named {
            out.push(format!("{prefix}{name}"));
        }
        if let (Ty::Obj(inner), Toml::Table(sub)) = (&f.ty, v) {
            widening(inner, sub, &format!("{prefix}{name}."), out);
        }
    }
}

impl Schema {
    /// Read the `fields` of a `[[kind]]` entry. Every field's first problem is reported.
    pub fn parse(spec: &Table) -> Result<Schema, Vec<String>> {
        fields(spec, "", 0).map(|fields| Schema { fields })
    }

    /// What is wrong with `entry` (without its `id`): a field the schema does not have, a value of the wrong type or
    /// out of range, and, in [`Mode::Whole`], a required field that is missing. `known(kind, id)` says whether a
    /// `ref` points at something.
    pub fn check(&self, entry: &Table, mode: Mode, known: &dyn Fn(&str, &str) -> bool) -> Checked {
        let mut out = Checked::default();
        check_fields(&self.fields, entry, mode, "", known, &mut out);
        out
    }

    /// Put the default of every field the entry does not hold (also inside a table it holds).
    pub fn fill(&self, entry: &mut Table) {
        fill_fields(&self.fields, entry);
    }

    /// The entry in the language of expressions, so a query over it compares durations as durations.
    pub fn value(&self, entry: &Table) -> Value {
        Value::Map(typed(&self.fields, entry))
    }

    /// The fields `entry` holds at a value that widens what J4n may do (`a.b` inside a table).
    pub fn widening(&self, entry: &Table) -> Vec<String> {
        let mut out = Vec::new();
        widening(&self.fields, entry, "", &mut out);
        out
    }

    /// The kinds the fields point at with `ref`.
    pub fn ref_kinds(&self, out: &mut BTreeSet<String>) {
        fn walk(ty: &Ty, out: &mut BTreeSet<String>) {
            match ty {
                Ty::Ref(k) => {
                    out.insert(k.clone());
                }
                Ty::List(t) | Ty::Map(t) => walk(t, out),
                Ty::Obj(fields) => fields.values().for_each(|f| walk(&f.ty, out)),
                _ => {}
            }
        }
        self.fields.values().for_each(|f| walk(&f.ty, out));
    }

    /// May `actor` write field `name` (its top-level name)? A name the schema does not have: no.
    pub fn may_write(&self, name: &str, actor: ActorKind) -> bool {
        self.fields.get(name).is_some_and(|f| f.may_write(actor))
    }
}
