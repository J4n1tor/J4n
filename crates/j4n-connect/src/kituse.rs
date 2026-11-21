//! The use fold of the kit: what each part of the kit's library did in the cells of the Log,
//! and which part has not been used for a long while. The library reports the parts a cell used (`used` on the `py.cell` event); this counts them
//! by part: how often, in how many tasks, in how many failed cells, and when last. It reads nothing but `py.cell` and `proposal.made`, so with the
//! kit off, where no cell is written, it holds nothing. The pack rule `kit-part-unused` (`packs/kit/rules/`) reads [`next`](KitUse) from it and
//! makes the Proposal that turns the part off (the setting `kit.off`); this fold never writes.

use j4n_core::fold::Fold;
use j4n_core::log::Ev;
use j4n_core::value::{Key, Value};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

/// The parts of the library that count (`_PARTS` in `packs/kit/py/j4n.py`; a case holds the two together), and the recipes as `recipe:<name>`.
pub const PARTS: [&str; 18] = ["chunks", "table", "grep", "dedup", "heads", "recipes", "recipe", "judge", "choose", "score", "keep", "pick", "model", "tools", "call", "many", "fetch", "need"];
/// A part is unused when none of the last this many cells (of the Log) used it — default, not confirmed.
pub const WINDOW: u64 = 100;
/// … and those cells were in at least this many tasks.
pub const TASKS: usize = 5;
/// The most tasks and the most tasks per part the fold keeps.
const KEPT: usize = 1000;
/// The most one part's count in one cell is taken as.
const TIMES: u64 = 100_000;
/// The key a Proposal of this fold has, before the part.
pub const KEY: &str = "kit-off:";

#[derive(Default, Serialize, Deserialize)]
struct Part {
    uses: u64,
    cells: u64,
    fails: u64,
    /// The ordinal of the last cell that used it.
    last: u64,
    tasks: BTreeSet<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct Book {
    /// The cells the Log holds that say what they used.
    cells: u64,
    /// By task: the ordinal of its last such cell.
    tasks: BTreeMap<String, u64>,
    parts: BTreeMap<String, Part>,
    /// The parts a Proposal was made for, by the proposal's id (`p<seq>`): asked once, whatever the answer (the operator's word on it is theirs).
    asked: BTreeMap<String, String>,
    /// The parts whose Proposal was accepted and not undone: in `kit.off` from the next start.
    accepted: BTreeSet<String>,
}

/// The fold `kit_use`. A clone is a second handle on the same state.
#[derive(Clone)]
pub struct KitUse {
    book: Arc<Mutex<Book>>,
    /// The parts that are off already (`kit.off`, read at start) and the recipes there are.
    off: Arc<Vec<String>>,
    all: Arc<Vec<String>>,
}

impl KitUse {
    pub fn new(off: Vec<String>, recipes: &[&str]) -> KitUse {
        let all = PARTS.iter().map(|p| p.to_string()).chain(recipes.iter().map(|r| format!("recipe:{r}"))).collect();
        KitUse { book: Arc::default(), off: Arc::new(off), all: Arc::new(all) }
    }

    fn book(&self) -> MutexGuard<'_, Book> {
        self.book.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn num(n: u64) -> Value {
    Value::num(n as f64)
}

impl Fold for KitUse {
    fn id(&self) -> &str {
        "kit_use"
    }

    fn version(&self) -> u64 {
        1
    }

    fn wants(&self, kind: &str) -> bool {
        matches!(kind, "py.cell" | "proposal.made" | "proposal.accepted" | "proposal.undone")
    }

    fn apply(&mut self, ev: &Ev) {
        let mut b = self.book();
        if ev.kind.starts_with("proposal.") {
            let id = ev.data["id"].as_str().map(String::from).unwrap_or_else(|| format!("p{}", ev.seq));
            match ev.kind.as_str() {
                "proposal.made" => {
                    if let Some(part) = ev.data["draft"]["key"].as_str().and_then(|k| k.strip_prefix(KEY)) {
                        b.asked.insert(id, part.to_string());
                    }
                }
                kind => {
                    if let Some(part) = b.asked.get(&id).cloned() {
                        if kind == "proposal.accepted" { b.accepted.insert(part) } else { b.accepted.remove(&part) };
                    }
                }
            }
            return;
        }
        // an older cell does not say what it used: it is no evidence of anything
        let Some(used) = ev.data.get("used").and_then(Json::as_object) else { return };
        let (task, failed) = (ev.task.clone().unwrap_or_default(), ev.data.get("ok") == Some(&Json::Bool(false)));
        b.cells += 1;
        let at = b.cells;
        if b.tasks.len() < KEPT || b.tasks.contains_key(&task) {
            b.tasks.insert(task.clone(), at);
        }
        for (name, times) in used.iter().filter(|(n, _)| self.all.contains(n)) {
            let p = b.parts.entry(name.clone()).or_default();
            (p.uses, p.cells, p.last) = (p.uses.saturating_add(times.as_u64().unwrap_or(1).min(TIMES)), p.cells + 1, at);
            p.fails += u64::from(failed);
            if p.tasks.len() < KEPT {
                p.tasks.insert(task.clone());
            }
        }
    }

    /// `cells`, `tasks` (with a cell in the window), `parts.<name>.{uses,cells,tasks,fails,idle}` for the parts used, and `next`: the first part that
    /// is unused over the window, not off, not asked about yet, as `{part, idle, tasks, uses, in_tasks, fails}`; absent when there is none.
    fn read(&self, path: &[Key], _now_ms: u64) -> Value {
        let b = self.book();
        let idle = |p: Option<&Part>| b.cells - p.map_or(0, |p| p.last);
        let seen = b.tasks.values().filter(|&&at| b.cells - at < WINDOW).count();
        let part = |name: &str| {
            let p = b.parts.get(name);
            let row = [("uses", p.map_or(0, |p| p.uses)), ("cells", p.map_or(0, |p| p.cells)), ("tasks", p.map_or(0, |p| p.tasks.len() as u64)), ("fails", p.map_or(0, |p| p.fails)), ("idle", idle(p))];
            Value::Map(row.into_iter().map(|(k, v)| (k.to_string(), num(v))).collect())
        };
        let mut top = BTreeMap::from([("cells".to_string(), num(b.cells)), ("tasks".to_string(), num(seen as u64)), ("parts".to_string(), Value::Map(self.all.iter().map(|n| (n.clone(), part(n))).collect()))]);
        let due = self.all.iter().find(|n| !self.off.contains(n) && !b.asked.values().any(|a| a == *n) && b.cells >= WINDOW && seen >= TASKS && idle(b.parts.get(*n)) >= WINDOW);
        if let Some(name) = due {
            let Value::Map(mut row) = part(name) else { return Value::Missing };
            row.insert("part".into(), Value::Str(name.clone()));
            // the whole new value of `kit.off`: what it says now, what was accepted since, and this part (a template shows an empty setting as a dash)
            let value: BTreeSet<&String> = self.off.iter().chain(&b.accepted).chain([name]).collect();
            row.insert("value".into(), Value::Str(value.into_iter().cloned().collect::<Vec<_>>().join(" ")));
            row.insert("window".into(), num(WINDOW));
            row.insert("seen".into(), num(seen as u64));
            top.insert("next".into(), Value::Map(row));
        }
        Value::Map(top).at(path)
    }

    fn save(&self) -> Json {
        serde_json::to_value(&*self.book()).unwrap_or_default()
    }

    fn load(&mut self, state: Json) -> Result<(), String> {
        *self.book() = serde_json::from_value(state).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn reset(&mut self) {
        *self.book() = Book::default();
    }
}

/// What a cell said it used, as the Log may keep it: the cell is arbitrary Python and can write anything into the library's account, so only the
/// known parts stay (`PARTS` and `recipe:<name>` for the recipes there are), each count clamped. `None` for what is not a map.
pub fn clean_used(raw: &Json, recipes: &[&str]) -> Option<Json> {
    let kept = raw.as_object()?.iter().filter(|(n, _)| PARTS.contains(&n.as_str()) || n.strip_prefix("recipe:").is_some_and(|r| recipes.contains(&r)));
    Some(Json::Object(kept.map(|(n, t)| (n.clone(), Json::from(t.as_u64().unwrap_or(1).min(TIMES)))).collect()))
}

/// The card without its sentences about the judge (`j4n.judge`, `choose`, `score`, `keep` and the line that they need `kit.judge`): from "For yes/no" to
/// "the error says so." A card that has no such span is returned as it was.
pub fn without_judge(card: &str) -> String {
    const FROM: &str = "For yes/no";
    const TO: &str = "the error says so.";
    match card.find(FROM).and_then(|a| card[a..].find(TO).map(|b| (a, a + b + TO.len()))) {
        Some((a, b)) => format!("{}{}", &card[..a], card[b..].trim_start()),
        None => card.to_string(),
    }
}

/// The card of the kit with a last sentence naming what is turned off (`kit.off`); the card as it was measured when nothing is. The words of a
/// part are not cut out of the card's prose: a cell that calls one is refused and the list says so.
pub fn card_with(card: &str, off: &[String]) -> String {
    let names: Vec<String> = off.iter().map(|n| n.strip_prefix("recipe:").map_or_else(|| format!("`j4n.{n}`"), |r| format!("the recipe `{r}`"))).collect();
    if names.is_empty() { card.trim().to_string() } else { format!("{} Turned off in this project, so a cell that calls them is refused: {}.", card.trim(), names.join(", ")) }
}
