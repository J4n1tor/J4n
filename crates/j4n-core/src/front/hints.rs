//! The keys row as data (the kind `hint`, `packs/base/kinds/hint.toml`): the keys of a page, of a field kind and
//! of the whole screen, each with the few words it is shown with. The screen is sent them as the rows of `keys:row`, in the order of
//! their `order`, and picks the ones of the page that is open; no word of a key stands in its code.

use crate::table::{Kind, Problem, Tables};
use j4n_proto::keys::Key;
use j4n_proto::{Cell, Row, Tone};
use serde::Deserialize;

/// Where a page's own key without an `order` stands on the row.
const PAGE_ORDER: i64 = 35;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HintRow {
    pub id: String,
    #[serde(default)]
    pub doc: String,
    pub keys: String,
    #[serde(default)]
    pub word: String,
    #[serde(default = "fifty")]
    pub order: i64,
}

fn fifty() -> i64 {
    50
}

impl Kind for HintRow {
    const NAME: &'static str = "hint";
}

/// A key as the row writes it: `C-p` is `⌃p`.
fn glyphs(k: &Key) -> String {
    k.to_string().replace("C-", "⌃").replace("M-", "⌥").replace("S-", "⇧")
}

/// The hints the Table holds, as rows of `keys:row`: the id as the key, then the keys, the words and the doc; in the order of `order`, a tie
/// by id. A key of one page in the Keys table that has a `word` is a hint of that page, written with the keys that do it now.
pub fn rows(tables: &Tables) -> (Vec<Row>, Vec<Problem>) {
    let mut found = Vec::new();
    let mut list: Vec<(i64, String, String, String, String)> = Vec::new();
    for id in tables.ids(HintRow::NAME) {
        match tables.get::<HintRow>(id) {
            Ok(h) if h.id.split_once('/').is_some_and(|(p, n)| !p.is_empty() && !n.is_empty()) => list.push((h.order, h.id, h.keys, h.word, h.doc)),
            Ok(h) => found.push(Problem::refused(format!("[[hint]] {}", h.id), "an id is `<place>/<name>`")),
            Err(p) => found.push(p),
        }
    }
    for id in tables.ids(super::keys::KeyRow::NAME) {
        let Ok(r) = tables.get::<super::keys::KeyRow>(id) else { continue };
        if !r.id.starts_with("page:") || r.word.is_empty() {
            continue;
        }
        let keys: Vec<String> = r.keys.as_ref().unwrap_or(&r.native).iter().filter_map(|k| k.parse::<Key>().ok()).map(|k| glyphs(&k)).collect();
        if let Some(first) = keys.first() {
            list.push((r.order.unwrap_or(PAGE_ORDER), r.id.clone(), first.clone(), r.word.clone(), r.doc.clone()));
        }
    }
    list.sort();
    let rows = list
        .into_iter()
        .map(|(_, id, keys, word, doc)| Row {
            key: id,
            cells: vec![Cell::new(keys, Tone::Plain), Cell::new(word, Tone::Dim)],
            detail: vec![Cell::new(doc, Tone::Dim)],
            ..Row::default()
        })
        .collect();
    (rows, found)
}
