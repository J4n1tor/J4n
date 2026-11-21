//! Crew: a seat is what a chat is beside its task: its own words, whether it only reads, the paths and tools it may use. Main's brief names one; the
//! seat fills what the brief leaves empty, and where both say something about access the narrower wins, field by field. This is the data and the
//! decisions, with no front and no task in it: the rows of the kind `seat` (the pack `crew`) and [`narrow`], which a test reads as the front does.

use crate::change::{Actor, Cause, Change, Target, Where};
use crate::setting::{Settings, Tune};
use crate::table::{Kind, Problem, Tables};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use toml::{Table, Value as Toml};

/// The seat the operator opens for a talk with them. A brief of Main's does not name it.
pub const ENVOY: &str = "envoy";

/// An entry of kind `seat`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seat {
    pub id: String,
    pub doc: Option<String>,
    pub role: String,
    #[serde(default)]
    pub personality: String,
    pub mode: Option<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub tools_allow: Vec<String>,
    #[serde(default)]
    pub tools_deny: Vec<String>,
    #[serde(default)]
    pub doer: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub reports: String,
}

impl Kind for Seat {
    const NAME: &'static str = "seat";
}

fn word(w: &str) -> bool {
    !w.is_empty() && w.len() <= 40 && w.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.' | ':'))
}

/// A tool's name as a doer knows it (`Edit`, `read`, `mcp__x__y`): letters, digits and `_ . : -`, starting with a letter, so it is never read as a flag or a list.
pub fn tool_word(w: &str) -> bool {
    w.len() <= 60 && w.starts_with(|c: char| c.is_ascii_alphabetic()) && w.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'))
}

impl Seat {
    /// Whether the seat reads: the role is a word, the words are no longer than `voice_max`, the paths are inside the project, a tool is not both allowed
    /// and denied.
    pub fn check(&self, voice_max: usize) -> Result<(), String> {
        let bad = |what: String| Err(format!("seat `{}`: {what}", self.id));
        if !word(&self.role) {
            return bad("the role is one lowercase word".into());
        }
        if self.personality.chars().count() > voice_max {
            return bad(format!("its own words are more than the {voice_max} characters `crew.voice_max` allows"));
        }
        if let Some(p) = self.paths.iter().find(|p| !crate::front::split::inside(p)) {
            return bad(format!("`{p}` is not a path inside the project"));
        }
        if let Some(t) = self.tools_allow.iter().chain(&self.tools_deny).find(|t| !tool_word(t)) {
            return bad(format!("`{t}` is not the name of a tool"));
        }
        if let Some(t) = self.tools_allow.iter().find(|t| self.tools_deny.contains(t)) {
            return bad(format!("`{t}` is both allowed and denied"));
        }
        Ok(())
    }
}

impl Seat {
    /// What the chat is given of this seat, with what it narrowed in the brief.
    pub fn seated(&self, narrowed: Vec<String>) -> Seated {
        Seated { id: self.id.clone(), role: self.role.clone(), voice: self.personality.clone(), reports: self.reports.clone(), tools_allow: self.tools_allow.clone(), tools_deny: self.tools_deny.clone(), narrowed }
    }
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    seat: Vec<Seat>,
}

/// The pack's switch and the seats the Table holds.
#[derive(Debug, Clone, Default)]
pub struct Crew {
    /// The switch `crew.on`. Off: a brief's `seat` is not read and a chat has no settings of its own.
    pub on: bool,
    /// The switch `crew.envoys`: the operator may open an envoy, and a charter is signed in its chat.
    pub envoys: bool,
    pub seats: Arc<Vec<Seat>>,
    /// `crew.voice_max`, and the words of `crew.chat_settings`: the settings a chat's own layer may hold.
    pub voice_max: usize,
    pub listed: Vec<String>,
    /// `crew.goal` (`cost` or `quality`) and `crew.checkers`.
    pub goal: String,
    pub checkers: usize,
    /// `crew.links`: who may talk to whom (the star, or the rows declared).
    pub links: Arc<crate::links::Links>,
}

impl Crew {
    /// The rows the Table holds (kind `seat`, pack `crew`); a row that does not read is left out, and its fault returned.
    pub fn load(tables: &Tables, settings: &Settings) -> (Crew, Vec<Problem>) {
        let voice_max = settings.num("crew.voice_max", 1200.0) as usize;
        let (mut rows, mut problems) = (Vec::new(), Vec::new());
        for id in tables.ids(Seat::NAME) {
            match tables.get::<Seat>(id) {
                Ok(r) => match r.check(voice_max) {
                    Ok(()) => rows.push(r),
                    Err(e) => problems.push(Problem::refused(format!("seat {id}"), e)),
                },
                Err(p) => problems.push(p),
            }
        }
        let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
        let (links, found) = crate::links::Links::load(tables, settings.word("crew.links", "star") == "declared", &ids);
        problems.extend(found);
        let crew = Crew {
            on: settings.flag("crew.on", false),
            envoys: settings.flag("crew.envoys", false),
            seats: Arc::new(rows),
            voice_max,
            listed: settings.word("crew.chat_settings", DEFAULT_LIST).split_whitespace().map(String::from).collect(),
            goal: settings.word("crew.goal", "cost").to_string(),
            checkers: settings.num("crew.checkers", 2.0).max(1.0) as usize,
            links: Arc::new(links),
        };
        (crew, problems)
    }

    /// Whether the crew works for quality: on, and the goal says so.
    pub fn quality(&self) -> bool {
        self.on && self.goal == "quality"
    }

    /// The seats a brief may name: none while the switch is off (`None`, and the field is not read).
    pub fn named(&self) -> Option<&[Seat]> {
        self.on.then(|| &self.seats[..])
    }

    /// The rows of the pack as it ships: what a test starts from.
    pub fn shipped() -> Crew {
        let file: File = toml::from_str(include_str!("../../../packs/crew/seats/seats.toml")).unwrap_or_else(|e| panic!("packs/crew/seats/seats.toml: {e}"));
        Crew { on: true, envoys: false, seats: Arc::new(file.seat), voice_max: 1200, listed: DEFAULT_LIST.split_whitespace().map(String::from).collect(), goal: "cost".into(), checkers: 2, links: Default::default() }
    }
}

/// A brief's access: whether it writes, the paths it reads and the paths it writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Access {
    pub write: bool,
    pub reads: Vec<String>,
    pub writes: Vec<String>,
}

/// What a seat gave a chat, as it is written to the Log and handed to the chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seated {
    pub id: String,
    pub role: String,
    /// The seat's own words, given to the chat as a section of its own.
    pub voice: String,
    pub reports: String,
    pub tools_allow: Vec<String>,
    pub tools_deny: Vec<String>,
    /// What the seat narrowed in the brief.
    pub narrowed: Vec<String>,
}

fn under(p: &str, s: &str) -> bool {
    let (p, s) = (p.trim_end_matches('/'), s.trim_end_matches('/'));
    p == s || p.strip_prefix(s).is_some_and(|rest| rest.starts_with('/'))
}

/// `paths` cut to `seat`: a path inside a seat path stays, a path that holds a seat path becomes that seat path, any other goes.
fn cut(paths: &[String], seat: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (p, s) in paths.iter().flat_map(|p| seat.iter().map(move |s| (p, s))) {
        let kept = if under(p, s) { p } else if under(s, p) { s } else { continue };
        let kept = kept.trim_end_matches('/').to_string();
        if !out.contains(&kept) {
            out.push(kept);
        }
    }
    out
}

/// The access of a brief under `seat`, and what was narrowed. A seat that only reads makes a chat that only reads, and a chat that writes loses its
/// write paths with it; a seat's paths cut the brief's reads and writes to what lies inside them. An empty list of the brief stays empty (the seat
/// does not fill it), and a list that is cut to nothing refuses the brief. Never wider than `access`.
pub fn narrow(seat: &Seat, access: Access) -> Result<(Access, Vec<String>), String> {
    let (mut a, mut notes) = (access, Vec::new());
    if seat.mode.as_deref() == Some("read") && a.write {
        a.write = false;
        a.writes.clear();
        notes.push("mode: it only reads".to_string());
    }
    if !seat.paths.is_empty() {
        for (name, list) in [("paths", &mut a.reads), ("writes", &mut a.writes)] {
            if list.is_empty() {
                continue;
            }
            let kept = cut(list, &seat.paths);
            if kept.is_empty() {
                return Err(format!("none of its {name} is inside the paths of the seat `{}`", seat.id));
            }
            if kept != *list {
                notes.push(format!("{name}: cut to the paths of the seat"));
            }
            *list = kept;
        }
    }
    Ok((a, notes))
}

// ---- a chat's own settings ----

/// The settings a chat's layer holds by default (`crew.chat_settings`).
pub const DEFAULT_LIST: &str = "model effort tools.allow tools.deny personality";
/// The settings the layer acts on: a key that is not here is refused, because a value nothing reads would only look as if it held.
pub const ACTING: [&str; 5] = ["model", "effort", "tools.allow", "tools.deny", "personality"];
/// The efforts a doer takes.
pub const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
/// What the layer refuses whatever `crew.chat_settings` says: the first segment of a key is one of these words (machine-wide, or about safety), or a word of the
/// key says money.
const MACHINE: [&str; 13] = ["accounts", "account", "packs", "lanes", "route", "watch", "chats", "crew", "orchestrate", "connect", "keys", "doer", "loop"];
const SAFETY: [&str; 9] = ["gate", "guard", "grants", "web", "box", "redact", "taint", "approval", "permissions"];
const MONEY: [&str; 7] = ["usd", "cap", "cost", "limit", "spend", "budget", "price"];

/// Why `key` is not a setting a chat's layer holds, or `None`. The classes of the plan come first and no list overrides them; then the list; then whether the
/// layer acts on it.
pub fn refusal(key: &str, listed: &[String]) -> Option<String> {
    let first = key.split('.').next().unwrap_or_default();
    if MACHINE.contains(&first) {
        return Some(format!("`{key}` is machine-wide, not a setting of one chat"));
    }
    if SAFETY.contains(&first) {
        return Some(format!("`{key}` is about safety: the Gate and the guard hold it for every chat alike"));
    }
    if MONEY.iter().any(|m| key.contains(m)) {
        return Some(format!("`{key}` is about money: a chat's spend is bounded where it starts, not in its own settings"));
    }
    if !listed.iter().any(|l| l == key) {
        return Some(format!("`{key}` is not on the list `crew.chat_settings` ({})", listed.join(", ")));
    }
    if !ACTING.contains(&key) {
        return Some(format!("`{key}` is on the list, but a chat does not act on it yet: the layer would hold a value nothing reads"));
    }
    None
}

/// The value of a chat setting as the layer keeps it, or why it does not fit. A null or an empty text is `None`: the setting is left out.
pub fn value_of(key: &str, v: &Json, voice_max: usize) -> Result<Option<Toml>, String> {
    match key {
        "model" | "effort" | "personality" => {
            let Some(t) = v.as_str().map(|s| s.trim().to_string()).or_else(|| v.is_null().then(String::new)) else { return Err(format!("`{key}` is words")) };
            if t.is_empty() {
                return Ok(None);
            }
            match key {
                "effort" if !EFFORTS.contains(&t.as_str()) => Err(format!("an effort is one of {}", EFFORTS.join(", "))),
                "model" if t.len() > 80 || t.starts_with('-') || t.chars().any(char::is_control) => Err("a model is one short word of its doer, not starting with a dash".into()),
                "personality" if t.chars().count() > voice_max => Err(format!("the words are more than the {voice_max} characters `crew.voice_max` allows")),
                _ => Ok(Some(Toml::String(t))),
            }
        }
        "tools.allow" | "tools.deny" => {
            let mut list: Vec<String> = match v {
                Json::Null => Vec::new(),
                Json::String(s) => s.split(|c: char| c == ',' || c.is_whitespace()).filter(|w| !w.is_empty()).map(String::from).collect(),
                Json::Array(a) => a.iter().filter_map(Json::as_str).map(|w| w.trim().to_string()).filter(|w| !w.is_empty()).collect(),
                _ => return Err(format!("`{key}` is a list of tool names")),
            };
            if let Some(w) = list.iter().find(|w| !tool_word(w)) {
                return Err(format!("`{w}` is not the name of a tool"));
            }
            list.sort();
            list.dedup();
            Ok(Some(Toml::Array(list.into_iter().map(Toml::String).collect())).filter(|a| a.as_array().is_some_and(|l| !l.is_empty())))
        }
        other => Err(format!("`{other}` has no value to check here")),
    }
}

/// Who writes a chat's settings. The front gives it from the code path that writes, never from text a model wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Writer {
    Operator,
    /// Main, from the brief it wrote.
    Main,
    /// An envoy (its chat's id), acting as the operator inside its charter.
    Envoy(String),
    /// The seat of the chat, when it starts.
    Seat,
}

impl Writer {
    pub fn actor(&self) -> Actor {
        match self {
            Writer::Operator => Actor::Operator,
            Writer::Main => Actor::Model("main".into()),
            Writer::Envoy(e) => Actor::Model(format!("envoy:{e}")),
            Writer::Seat => Actor::Core,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Rec {
    id: String,
    change: Change,
    undone: bool,
}

/// The settings of each chat, one entry per `(chat, key)`, and the changes that made them: a [`Change`] each, naming its writer, with an inverse.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Layer {
    rows: BTreeMap<String, BTreeMap<String, Toml>>,
    recs: Vec<Rec>,
}

fn row(v: &Toml) -> Table {
    Table::from_iter([("value".to_string(), v.clone())])
}

fn list_of(v: Option<&Toml>) -> Vec<String> {
    v.and_then(Toml::as_array).into_iter().flatten().filter_map(Toml::as_str).map(String::from).collect()
}

impl Layer {
    pub fn get(&self, chat: &str, key: &str) -> Option<&Toml> {
        self.rows.get(chat)?.get(key)
    }

    pub fn text(&self, chat: &str, key: &str) -> Option<&str> {
        self.get(chat, key).and_then(Toml::as_str)
    }

    pub fn list(&self, chat: &str, key: &str) -> Vec<String> {
        list_of(self.get(chat, key))
    }

    /// What a chat holds, `(key, value)`.
    pub fn of(&self, chat: &str) -> Vec<(String, Toml)> {
        self.rows.get(chat).map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default()
    }

    fn next_id(&self) -> String {
        format!("cs{}", self.recs.iter().filter_map(|r| r.id.strip_prefix("cs").and_then(|n| n.parse::<u64>().ok())).max().unwrap_or(0) + 1)
    }

    /// Set (or, with `None`, leave out) `key` of `chat`. Only the operator widens: another writer's tool lists can only narrow what the chat holds (its denies
    /// are added to, its allowed tools cut to what both name) and it cannot leave one out. `Ok(None)`: the layer already says so.
    pub fn set(&mut self, chat: &str, key: &str, value: Option<Toml>, by: &Writer, at_ms: u64) -> Result<Option<(String, Change)>, String> {
        let before = self.get(chat, key).cloned();
        let value = if *by == Writer::Operator || !key.starts_with("tools.") {
            value
        } else {
            let Some(new) = value else { return Err(format!("only the operator leaves `{key}` out: another writer may only narrow it")) };
            let (old, new) = (list_of(before.as_ref()), list_of(Some(&new)));
            let merged: Vec<String> = if key == "tools.deny" {
                old.iter().chain(&new).cloned().collect::<std::collections::BTreeSet<_>>().into_iter().collect()
            } else if old.is_empty() {
                new
            } else {
                old.iter().filter(|t| new.contains(t)).cloned().collect()
            };
            if merged.is_empty() {
                return Err("the tools it allows and the tools the chat already allows share none: only the operator widens".into());
            }
            Some(Toml::Array(merged.into_iter().map(Toml::String).collect()))
        };
        if value == before {
            return Ok(None);
        }
        let target = Target { layer: Where::Session, file: String::new(), kind: "chat_setting".into(), id: format!("{chat}:{key}") };
        let change = Change::sealed(target, before.as_ref().map(row), value.as_ref().map(row), by.actor(), None, at_ms);
        let id = self.next_id();
        self.apply(&id, &change);
        Ok(Some((id, change)))
    }

    /// Take back change `id`: the inverse, when the chat still holds what that change left. The undo is a change of its own and is returned with its id.
    pub fn undo(&mut self, id: &str, by: &Writer, at_ms: u64) -> Result<(String, Change), String> {
        let rec = self.recs.iter().find(|r| r.id == id).ok_or_else(|| format!("there is no change `{id}` of a chat's settings"))?;
        if rec.undone {
            return Err(format!("`{id}` was taken back already"));
        }
        let (chat, key) = rec.change.target.id.split_once(':').map(|(c, k)| (c.to_string(), k.to_string())).unwrap_or_default();
        if self.get(&chat, &key).map(row) != rec.change.after {
            return Err(format!("`{key}` of {chat} was changed after `{id}`: undo the later change first"));
        }
        let mut inverse = rec.change.inverse(by.actor(), at_ms);
        inverse.cause = Some(Cause::Undo(rec.change.id.clone()));
        let new = self.next_id();
        self.apply(&new, &inverse);
        Ok((new, inverse))
    }

    /// Put `change` in: the entry it leaves, or none. A change that takes another back marks that one undone.
    fn apply(&mut self, id: &str, change: &Change) {
        let Some((chat, key)) = change.target.id.split_once(':') else { return };
        let rows = self.rows.entry(chat.to_string()).or_default();
        match change.after.as_ref().and_then(|t| t.get("value")) {
            Some(v) => rows.insert(key.to_string(), v.clone()),
            None => rows.remove(key),
        };
        if rows.is_empty() {
            self.rows.remove(chat);
        }
        if let Some(Cause::Undo(of)) = &change.cause {
            for r in self.recs.iter_mut().filter(|r| r.change.id == *of) {
                r.undone = true;
            }
        }
        self.recs.push(Rec { id: id.to_string(), change: change.clone(), undone: false });
    }

    /// For each key `chat` holds: the id of the change that an undo takes back, and who made it in a word (`you`, `seat`, or the name of the model).
    pub fn held_by(&self, chat: &str) -> BTreeMap<String, (String, String)> {
        let mut out = BTreeMap::new();
        for (key, now) in self.rows.get(chat).into_iter().flatten() {
            let (target, current) = (format!("{chat}:{key}"), Some(row(now)));
            let found = self.recs.iter().rev().find(|r| r.change.target.id == target && !r.undone && !matches!(r.change.cause, Some(Cause::Undo(_))) && r.change.after == current);
            if let Some(r) = found {
                let by = match &r.change.by {
                    Actor::Operator => "you".to_string(),
                    Actor::Core => "seat".to_string(),
                    Actor::Rule(m) | Actor::Doer(m) | Actor::Model(m) | Actor::Pack(m) => m.clone(),
                };
                out.insert(key.clone(), (r.id.clone(), by));
            }
        }
        out
    }

    /// The chat is archived: its layer is gone.
    pub fn drop_chat(&mut self, chat: &str) -> bool {
        self.rows.remove(chat).is_some()
    }

    /// One event of the Log: a `chat.set` row (it holds its change; an undo's change names the one it takes back) puts that change in, and the archive of a chat ends
    /// its layer. A row that does not read, and any other event, leaves it as it is.
    pub fn take(&mut self, kind: &str, thread: &str, data: &Json) {
        match kind {
            "chat.set" => {
                if let (Some(id), Some(Ok(change))) = (data.get("id").and_then(Json::as_str), data.get("change").map(|c| serde_json::from_value::<Change>(c.clone()))) {
                    self.apply(id, &change);
                }
            }
            "chat.archived" | "chat.removed" => {
                self.drop_chat(thread);
            }
            _ => {}
        }
    }

    /// The layer the events say, in the order they were written.
    pub fn replay<'a>(events: impl IntoIterator<Item = (&'a str, &'a str, &'a Json)>) -> Layer {
        let mut l = Layer::default();
        for (kind, thread, data) in events {
            l.take(kind, thread, data);
        }
        l
    }
}

// ---- quality first ----

/// How much of a doer's pass rate its unsteadiness takes away at most: a doer whose results alternate, pass and fail, at every turn keeps this much less of it.
pub const FLIP_PENALTY: f64 = 0.3;
/// How many of the latest results the record is read from.
const WINDOW: usize = 12;

/// An entry of kind `seed`: a public board's score of a harness with a model.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seed {
    pub id: String,
    pub doc: Option<String>,
    #[serde(default)]
    pub doer: String,
    pub harness: String,
    pub model: String,
    pub score: f64,
    #[serde(default)]
    pub pm: f64,
    pub board: String,
    pub version: String,
    pub dated: String,
}

impl Kind for Seed {
    const NAME: &'static str = "seed";
}

impl Seed {
    fn check(&self) -> Result<(), String> {
        let day = self.dated.split('-').map(|p| p.parse::<u32>().ok()).collect::<Vec<_>>();
        if !(self.score.is_finite() && self.pm.is_finite()) || day.len() != 3 || day.iter().any(Option::is_none) || self.dated.len() != 10 {
            return Err(format!("seed `{}`: the score is a number and `dated` is a day, YYYY-MM-DD", self.id));
        }
        Ok(())
    }
}

/// The seeds the Table holds (kind `seed`, pack `crew`); a row that does not read is left out, and its fault returned.
pub fn seeds(tables: &Tables) -> (Vec<Seed>, Vec<Problem>) {
    let (mut rows, mut problems) = (Vec::new(), Vec::new());
    for id in tables.ids(Seed::NAME) {
        match tables.get::<Seed>(id) {
            Ok(r) => match r.check() {
                Ok(()) => rows.push(r),
                Err(e) => problems.push(Problem::refused(format!("seed {id}"), e)),
            },
            Err(p) => problems.push(p),
        }
    }
    (rows, problems)
}

/// The seeds of the pack as it ships: what a test starts from.
pub fn shipped_seeds() -> Vec<Seed> {
    #[derive(Deserialize)]
    struct File {
        seed: Vec<Seed>,
    }
    toml::from_str::<File>(include_str!("../../../packs/crew/seeds/terminal-bench.toml")).unwrap_or_else(|e| panic!("packs/crew/seeds/terminal-bench.toml: {e}")).seed
}

/// What the board says of `doer`, from 0 to 1: the lowest of its rows, what it can be counted on for whichever model it runs; none when no row is its.
pub fn seed_score(seeds: &[Seed], doer: &str) -> Option<f64> {
    seeds.iter().filter(|s| !s.doer.is_empty() && s.doer == doer).map(|s| s.score / 100.0).reduce(f64::min)
}

/// The smoothed pass rate of the latest results (oldest first; `true` is a pass) and how often a result differs from the one before it. Unsteadiness is a proxy:
/// the tasks fold has no "same request", so a doer that passes and fails by turns is the one that is not steady.
pub fn steadiness(outcomes: &[bool]) -> (f64, f64) {
    let latest = &outcomes[outcomes.len().saturating_sub(WINDOW)..];
    let passed = latest.iter().filter(|o| **o).count() as f64;
    let flips = latest.windows(2).filter(|w| w[0] != w[1]).count() as f64;
    ((passed + 1.0) / (latest.len() as f64 + 2.0), if latest.len() > 1 { flips / (latest.len() - 1) as f64 } else { 0.0 })
}

/// The pass rate with its unsteadiness taken off: a doer that fails every time is steady and scores low, one that alternates scores less than its rate.
pub fn quality_of(outcomes: &[bool]) -> f64 {
    let (rate, flips) = steadiness(outcomes);
    rate * (1.0 - FLIP_PENALTY * flips)
}

/// The number the quality pick ranks a doer by: its own record when it has `record_min` results or more; else the board's score, if the seeds have one for
/// it; else what little record there is; none when there is nothing at all.
pub fn quality_column(outcomes: &[bool], seed: Option<f64>, record_min: usize) -> Option<f64> {
    if outcomes.len() >= record_min.max(1) {
        return Some(quality_of(outcomes));
    }
    seed.or_else(|| (!outcomes.is_empty()).then(|| quality_of(outcomes)))
}

/// Whether another review may start: fewer than `cap` are running.
pub fn checkers_room(running: usize, cap: usize) -> bool {
    running < cap.max(1)
}

#[derive(Default)]
struct GoalState {
    quality: AtomicBool,
    record_min: AtomicUsize,
}

/// Whether the crew works for quality now, and how many results a record needs: one handle the election of a doer and of a host share, kept by the settings
/// (`crew.on`, `crew.goal`, `crew.record_min`) after every edit.
#[derive(Clone, Default)]
pub struct Goal(Arc<GoalState>);

impl Goal {
    pub fn quality(&self) -> bool {
        self.0.quality.load(Ordering::SeqCst)
    }

    pub fn record_min(&self) -> usize {
        self.0.record_min.load(Ordering::SeqCst).max(1)
    }

    pub fn set(&self, quality: bool, record_min: usize) {
        self.0.quality.store(quality, Ordering::SeqCst);
        self.0.record_min.store(record_min, Ordering::SeqCst);
    }
}

impl Tune for Goal {
    fn tune(&self, settings: &Settings) {
        self.set(settings.flag("crew.on", false) && settings.word("crew.goal", "cost") == "quality", settings.num("crew.record_min", 5.0).max(1.0) as usize);
    }
}
