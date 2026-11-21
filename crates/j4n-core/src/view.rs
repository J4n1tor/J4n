//! Views: rows for a page, from data. A `[[view]]` is a [`Query`] over a collection the core keeps and the
//! way a row of it reads — cells made of [`Fmt`]s, a tone from an [`Expr`], lines of detail, and the keys that send a line as the
//! operator would type it. Where the rows of a collection come from is the caller's ([`Sources`]); what the screen is sent is
//! [`j4n_proto::Row`]s, so the screen is not changed for a view a pack adds.

use crate::eval::{Env, Scope};
use crate::expr::Expr;
use crate::fmt::{Fmt, Piece};
use crate::query::{Coll, Query, RowEnv};
use crate::table::{Kind, Problem, Tables};
use crate::value::{DASH, Value};
use j4n_proto::{Act, Cell, Fold, Intent, Row, Tone};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActRow {
    pub key: String,
    pub label: String,
    /// The line the key sends, as the operator would type it; or, with `set`, none.
    #[serde(default)]
    pub run: String,
    /// The operator's edit the key makes: `<kind>:<id>.<field>` (`setting:{id}.value`), sent as `Intent::Set` with `value`;
    /// with no `value` the screen opens the row's editor and sends what is kept.
    #[serde(default)]
    pub set: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub risk: u8,
    /// The key is offered only for a row where this holds (an expression over the row's fields).
    #[serde(default)]
    pub when: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewRow {
    pub id: String,
    #[serde(default)]
    pub doc: String,
    pub from: toml::Table,
    #[serde(default)]
    pub key: Option<String>,
    pub cells: Vec<String>,
    /// The title of each cell, drawn as a dim header row over a list; none leaves the list without one.
    #[serde(default)]
    pub titles: Vec<String>,
    #[serde(default)]
    pub tone: Option<String>,
    #[serde(default)]
    pub detail: Vec<String>,
    /// One hole, `{field}`, that holds a list of `[depth, text]`: rows that hang from the row (depth 1 from the row, 2 from the depth 1 above it; a depth 0
    /// is the row's own words and is left out), so a page that draws a tree shows them on pipes.
    #[serde(default)]
    pub children: Option<String>,
    /// An expression over the row's fields and `place` (its place in the rows, 0 for the first): the row starts shut where it holds, for a screen
    /// that folds rows with rows under them.
    #[serde(default)]
    pub folded: Option<String>,
    /// One hole, `{field}`: the key of the row this one hangs from, a row of the same view before it (empty: a row at the top), so a row
    /// that hangs has keys of its own, which a row of `children` has not.
    #[serde(default)]
    pub parent: Option<String>,
    /// One hole, `{field}`: the name of the glyph slot the first cell starts with (`yes`, `queued`); empty or unknown, none.
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub acts: Vec<ActRow>,
    /// One hole, `{field}`: its value goes to the screen as the row's `data` under the field's name, as structure (`{edit}`: what the row's
    /// editor needs).
    #[serde(default)]
    pub data: Option<String>,
    #[serde(default)]
    pub when: Option<String>,
}

impl Kind for ViewRow {
    const NAME: &'static str = "view";
}

/// Where the rows of a collection come from.
pub trait Sources {
    /// The rows of `coll`, each a map of fields, in the order the collection keeps.
    fn rows(&self, coll: &Coll) -> Vec<Value>;
}

pub struct View {
    pub id: String,
    pub doc: String,
    query: Query,
    key: Option<Fmt>,
    cells: Vec<Fmt>,
    titles: Vec<String>,
    tone: Option<Expr>,
    detail: Vec<Fmt>,
    children: Option<Expr>,
    folded: Option<Expr>,
    parent: Option<Expr>,
    icon: Option<Expr>,
    acts: Vec<ActFmt>,
    data: Option<(String, Expr)>,
    when: Option<Expr>,
}

/// A key of a row: the line it sends, or the edit it makes.
struct ActFmt {
    key: String,
    label: String,
    run: Fmt,
    set: Option<(Fmt, Option<Fmt>)>,
    risk: u8,
    when: Option<Expr>,
}

impl ActFmt {
    fn intent(&self, env: &dyn Env) -> Intent {
        let Some((to, value)) = &self.set else { return Intent::Say { text: self.run.render(env), with: Vec::new() } };
        let to = to.render(env);
        let (kind, rest) = to.split_once(':').unwrap_or(("", to.as_str()));
        let (id, field) = rest.rsplit_once('.').unwrap_or((rest, ""));
        let value = value.as_ref().map_or(serde_json::Value::Null, |v| serde_json::Value::String(v.render(env)));
        Intent::Set { kind: kind.into(), id: id.into(), field: field.into(), value }
    }
}

/// The field a `{field}` hole names, and its expression; anything else is refused.
fn one_hole(what: &str, text: &str) -> Result<Expr, String> {
    match Fmt::parse(text).map_err(|e| format!("`{text}`: {e}"))?.pieces() {
        [Piece::Ref { expr, format: None }] => Ok(expr.clone()),
        _ => Err(format!("`{what}` is one hole, `{{field}}`, not `{text}`")),
    }
}

fn tone_of(word: &str) -> Tone {
    match word {
        "dim" => Tone::Dim,
        "good" => Tone::Good,
        "warn" => Tone::Warn,
        "bad" => Tone::Bad,
        "accent" => Tone::Accent,
        _ => Tone::Plain,
    }
}

fn text(v: Value) -> String {
    match v {
        Value::Str(s) => s,
        Value::Missing => String::new(),
        other => crate::fmt::format_value(&other, None),
    }
}

impl View {
    fn read(r: ViewRow) -> Result<View, String> {
        let s = |k: &str| r.from.get(k).and_then(|v| v.as_str()).map(str::to_string);
        let limit = r.from.get("limit").and_then(|v| v.as_integer()).map(|n| n as u64);
        let query = Query::parse(&s("from").unwrap_or_default(), s("filter").as_deref(), s("order").as_deref(), limit).map_err(|e| e.join("; "))?;
        let fmts = |list: &[String]| list.iter().map(|t| Fmt::parse(t).map_err(|e| format!("`{t}`: {e}"))).collect::<Result<Vec<_>, _>>();
        let fmt = |t: &str| Fmt::parse(t).map_err(|e| format!("`{t}`: {e}"));
        let acts = r
            .acts
            .iter()
            .map(|a| {
                let when = a.when.as_deref().map(Expr::parse).transpose().map_err(|e| format!("`{}`: {e}", a.key))?;
                if a.set.is_none() == a.run.is_empty() {
                    return Err(format!("`{}` has `run` or `set`, one of them", a.key));
                }
                // a key is typed in the row's secret editor and goes by its own intent: never a value the pack writes, never a plain line
                if a.set.as_deref().is_some_and(|s| s.starts_with("key:")) && (a.value.is_some() || r.data.is_none()) {
                    return Err(format!("`{}` sets a `key:`: the row's secret editor types it (`data = \"{{edit}}\"`), with no `value`", a.key));
                }
                let set = a.set.as_deref().map(|s| Ok::<_, String>((fmt(s)?, a.value.as_deref().map(fmt).transpose()?))).transpose()?;
                Ok(ActFmt { key: a.key.clone(), label: a.label.clone(), run: fmt(&a.run)?, set, risk: a.risk, when })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let children = r.children.as_deref().map(|c| one_hole("children", c));
        let data = r.data.as_deref().map(|d| one_hole("data", d).map(|e| (d.trim_matches(['{', '}']).trim().to_string(), e))).transpose()?;
        Ok(View {
            id: r.id,
            doc: r.doc,
            query,
            key: r.key.as_deref().map(Fmt::parse).transpose().map_err(|e| e.to_string())?,
            cells: fmts(&r.cells)?,
            titles: r.titles,
            tone: r.tone.as_deref().map(Expr::parse).transpose().map_err(|e| e.to_string())?,
            detail: fmts(&r.detail)?,
            children: children.transpose()?,
            folded: r.folded.as_deref().map(Expr::parse).transpose().map_err(|e| format!("`folded`: {e}"))?,
            parent: r.parent.as_deref().map(|p| one_hole("parent", p)).transpose()?,
            icon: r.icon.as_deref().map(|i| one_hole("icon", i)).transpose()?,
            acts,
            data,
            when: r.when.as_deref().map(Expr::parse).transpose().map_err(|e| e.to_string())?,
        })
    }

    /// The titles of the cells, one each, or none.
    pub fn titles(&self) -> &[String] {
        &self.titles
    }

    pub fn from(&self) -> &Coll {
        &self.query.from
    }

    /// Whether the view is shown now (`set` is the settings tree).
    pub fn shown(&self, outer: &dyn Env) -> bool {
        self.when.as_ref().is_none_or(|w| w.holds(outer))
    }

    /// The rows, as the screen is sent them. A row with no `key` is named by its `id`, else its `key` field, else its place.
    pub fn rows(&self, src: &dyn Sources, outer: &dyn Env) -> Vec<Row> {
        if !self.shown(outer) {
            return Vec::new();
        }
        let rows = self.query.apply(src.rows(&self.query.from), outer);
        rows.iter()
            .enumerate()
            .flat_map(|(i, v)| {
                // `place` is a field of the row only for `folded`, which is the one expression that wants it
                let shut = self.folded.as_ref().is_some_and(|f| {
                    let mut with = v.clone();
                    if let Value::Map(m) = &mut with {
                        m.insert("place".into(), Value::Num(i as f64));
                    }
                    f.holds(&RowEnv::new(&with, outer))
                });
                let env = RowEnv::new(v, outer);
                let name = |f: &str| match v {
                    Value::Map(m) => m.get(f).map(|x| text(x.clone())).filter(|s| !s.is_empty()),
                    _ => None,
                };
                let key = self.key.as_ref().map(|k| k.render(&env)).or_else(|| name("id")).or_else(|| name("key")).unwrap_or_else(|| i.to_string());
                let tone = self.tone.as_ref().map_or(Tone::Plain, |t| tone_of(&text(t.eval(&env))));
                let kids = self.children.as_ref().map(|c| hang(&key, &c.eval(&env))).unwrap_or_default();
                let hole = |e: &Option<Expr>| e.as_ref().map(|e| text(e.eval(&env))).filter(|s| !s.is_empty());
                let mut cells: Vec<Cell> = self.cells.iter().map(|c| Cell::new(c.render_shown(&env), Tone::Plain)).collect();
                if let (Some(icon), Some(first)) = (hole(&self.icon), cells.first_mut()) {
                    first.icon = icon;
                }
                let row = Row {
                    key,
                    cells,
                    tone,
                    detail: self.detail.iter().map(|d| Cell::new(d.render_shown(&env), Tone::Dim)).filter(|c| !c.text.is_empty() && c.text != DASH).collect(),
                    actions: self
                        .acts
                        .iter()
                        .filter(|a| a.when.as_ref().is_none_or(|w| w.holds(&env)))
                        .map(|a| Act { key: a.key.clone(), label: a.label.clone(), intent: a.intent(&env), risk: a.risk })
                        .collect(),
                    parent: hole(&self.parent),
                    fold: shut.then_some(Fold::Folded),
                    data: self.data.as_ref().and_then(|(name, e)| Some(e.eval(&env).to_json()).filter(|j| !j.is_null()).map(|j| serde_json::json!({ name.as_str(): j }))),
                    ..Row::default()
                };
                std::iter::once(row).chain(kids)
            })
            .collect()
    }
}

/// The rows a list of `[depth, text]` makes under the row `key`: depth 1 hangs from the row, a deeper one from the nearest row above it that is one level up.
fn hang(key: &str, list: &Value) -> Vec<Row> {
    let Value::List(items) = list else { return Vec::new() };
    let mut up: Vec<String> = vec![key.to_string()];
    let mut out = Vec::new();
    for (n, item) in items.iter().enumerate() {
        let Value::List(pair) = item else { continue };
        let (Some(Value::Num(depth)), Some(words)) = (pair.first(), pair.get(1)) else { continue };
        let depth = (*depth as usize).min(up.len());
        if depth == 0 {
            continue;
        }
        let own = format!("{key}/{n}");
        up.truncate(depth);
        out.push(Row { key: own.clone(), cells: vec![Cell::new(text(words.clone()), Tone::Plain)], parent: Some(up[depth - 1].clone()), ..Row::default() });
        up.push(own);
    }
    out
}

#[derive(Default)]
pub struct Views {
    by_id: BTreeMap<String, View>,
}

impl Views {
    /// The views the Table holds. One that does not read is left out and said.
    pub fn load(tables: &Tables) -> (Views, Vec<Problem>) {
        let (mut out, mut found) = (Views::default(), Vec::new());
        for id in tables.ids(ViewRow::NAME) {
            match tables.get::<ViewRow>(id).map_err(|p| p.msg).and_then(View::read) {
                Ok(v) => drop(out.by_id.insert(v.id.clone(), v)),
                Err(why) => found.push(Problem::refused(format!("[[view]] {id}"), why)),
            }
        }
        (out, found)
    }

    pub fn get(&self, id: &str) -> Option<&View> {
        self.by_id.get(id)
    }

    pub fn ids(&self) -> Vec<&str> {
        self.by_id.keys().map(String::as_str).collect()
    }
}

/// An outer `Env` that knows the settings (`set`).
pub fn outer(settings: Value) -> Scope {
    Scope::new().with("set", settings)
}
