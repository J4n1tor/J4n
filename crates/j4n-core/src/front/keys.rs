//! The keymap as data (the kind `key`, `packs/base/kinds/key.toml`): what the screen is told about its keys. The base pack lists the keys
//! the TUI's own code reads; a layer above says other keys do them, or none; a page's own key sends an intent. The screen reads no file:
//! it is sent the result, every key that is bound or freed, by context (`j4n_proto::Down::Keys`).

use crate::table::{Kind, Problem, Tables};
use j4n_proto::keys::{self, Key};
use j4n_proto::{Bind, Intent};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

/// Where a key is read: the screen as a whole, the line under a chat, the keys on a transcript, a list or form, an open question. A key of
/// one page is `page:<id>`.
const CONTEXTS: [&str; 5] = ["global", "composer", "transcript", "list", "ask"];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyRow {
    pub id: String,
    #[serde(default)]
    pub doc: String,
    /// For a key of a page: the few words it is shown with on the keys row (`front::hints`); none, and it is not shown there.
    #[serde(default)]
    pub word: String,
    /// For a key of a page that has a `word`: where it stands on the row.
    #[serde(default)]
    pub order: Option<i64>,
    #[serde(default)]
    pub native: Vec<String>,
    #[serde(default)]
    pub keys: Option<Vec<String>>,
    #[serde(default)]
    pub run: Option<toml::Table>,
}

impl Kind for KeyRow {
    const NAME: &'static str = "key";
}

/// What a key of a row does.
struct Row {
    context: String,
    native: Vec<Key>,
    run: Option<Intent>,
    keys: Vec<Key>,
    /// A layer of the operator's said `keys`: its say outranks a pack's.
    mine: bool,
}

fn row(tables: &Tables, r: &KeyRow, page: &dyn Fn(&str) -> bool, found: &mut Vec<Problem>) -> Option<Row> {
    let at = format!("[[key]] {}", r.id);
    let mut refuse = |why: String| {
        found.push(Problem::refused(at.clone(), why));
        None
    };
    let Some((context, name)) = r.id.split_once('/').filter(|(c, n)| !c.is_empty() && !n.is_empty()) else {
        return refuse("an id is `<context>/<name>`".into());
    };
    let page_id = context.strip_prefix("page:");
    if !CONTEXTS.contains(&context) && !page_id.is_some_and(page) {
        return refuse(format!("`{context}` is not a place for keys: {}, or page:<id> of a page that is there", CONTEXTS.join(", ")));
    }
    let native: Result<Vec<Key>, String> = r.native.iter().map(|k| k.parse()).collect();
    let native = match native {
        Ok(n) => n,
        Err(why) => return refuse(why),
    };
    let run = match &r.run {
        Some(t) => match serde_json::to_value(t).ok().as_ref().and_then(Intent::from_json) {
            Some(i) => Some(i),
            None => return refuse("`run` is not an intent".into()),
        },
        None => None,
    };
    match (native.is_empty(), run.is_some(), page_id.is_some()) {
        (false, true, _) => return refuse("a key reads the screen's own keys (`native`) or sends an intent (`run`), not both".into()),
        (true, false, _) => return refuse(format!("`{name}` needs `native`, the keys the screen reads for it, or `run`")),
        (false, false, true) => return refuse("the screen has no keys of its own for a page: give `run`".into()),
        _ => {}
    }
    let mut keys = Vec::new();
    for word in r.keys.as_ref().unwrap_or(&r.native) {
        match word.parse::<Key>() {
            // a key the screen's own code reads is its own business, whatever a terminal or tmux does with it
            Ok(k) => match keys::refused(&k).filter(|_| !native.contains(&k)) {
                Some(why) => found.push(Problem::refused(at.clone(), format!("`{word}` is not bound: {why}"))),
                None => keys.push(k),
            },
            Err(why) => found.push(Problem::refused(at.clone(), why)),
        }
    }
    let mine = tables.raw(KeyRow::NAME, &r.id).and_then(|m| m.from.get("keys")).is_some_and(|l| matches!(l.as_str(), "home" | "project" | "session"));
    Some(Row { context: context.to_string(), native, run, keys, mine })
}

/// The binds the Table's rows make: each key of a row, read as the screen's own key (itself where the row lists it, else the first) or
/// as its intent; and every key a row reads that no row binds now, freed. Two rows that bind a key in one place: the operator's wins
/// over a pack's, else the first by id, and it is said.
pub fn compile(tables: &Tables, page: &dyn Fn(&str) -> bool) -> (Vec<Bind>, Vec<Problem>) {
    let mut found = Vec::new();
    let rows: BTreeMap<String, Row> = tables
        .ids(KeyRow::NAME)
        .into_iter()
        .filter_map(|id| match tables.get::<KeyRow>(id) {
            Ok(r) => row(tables, &r, page, &mut found).map(|row| (r.id, row)),
            Err(p) => {
                found.push(p);
                None
            }
        })
        .collect();
    let mut bound: BTreeMap<(&str, Key), (&str, &Row)> = BTreeMap::new();
    for (id, r) in &rows {
        for k in &r.keys {
            match bound.get(&(r.context.as_str(), *k)) {
                Some((first, old)) if old.mine == r.mine => {
                    found.push(Problem::warn(format!("[[key]] {id}"), format!("`{k}` is also `{first}`'s in {}: it keeps it", r.context)))
                }
                Some((_, old)) if old.mine => {}
                _ => {
                    bound.insert((r.context.as_str(), *k), (id.as_str(), r));
                }
            }
        }
    }
    let mut binds: Vec<Bind> = bound
        .iter()
        .map(|((context, k), (_, r))| {
            let to = if r.native.contains(k) { Some(*k) } else { r.native.first().copied() };
            Bind { context: context.to_string(), key: k.to_string(), to: to.map(|k| k.to_string()).unwrap_or_default(), run: r.run.clone() }
        })
        .collect();
    let free: BTreeSet<(&str, Key)> = rows.values().flat_map(|r| r.native.iter().map(|k| (r.context.as_str(), *k))).filter(|c| !bound.contains_key(c)).collect();
    binds.extend(free.into_iter().map(|(context, k)| Bind { context: context.to_string(), key: k.to_string(), to: String::new(), run: None }));
    binds.sort_by(|a, b| (&a.context, &a.key).cmp(&(&b.context, &b.key)));
    (binds, found)
}
