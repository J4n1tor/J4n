//! Needs: a feature says what it lacks (the kind `need`, `packs/base/kinds/need.toml`). A need names the setting that turns a feature on, and
//! for each of its parts (a cheap model for the look, a model for Main) the settings that hold the doer and the model and how a value is found
//! (`tier:cheap`, `tier:standard`, `doer:ready`: [`super::tiers`], by price, never by asking a model). A need is **met** when a part's model setting holds
//! a value, or the operator went on without it (`need.waived`); while it is unmet the feature still works on the tier's pick, so nothing stops.
//!
//! When a feature is switched on, or is about to act, and lacks what it needs, **one card** says what is missing, what was found (*claude is signed in; its
//! cheapest model is haiku*) and four answers: **use** it, **pick another** (a card for each part with the doer's models, the cheapest first), **go on without
//! it** (the need is added to `need.waived`; what that means is on the card) and **turn the feature off**. Each is a setting edit of the operator's
//! (`:undo` takes it back), and the card is not shown again for that need this run; it is shown again at the next start only while the need is unmet.
//! No card is made for a run with no one to ask.
//!
//! Beyond a setting a need is of a **kind**: `key` (a connector's key is there), `login` (a doer is installed and signed in), `git` (the folder has a commit a worktree is cut from) or `decider` (an ask point has a Decider
//! or a model to answer it, or the operator picks by hand). A feature that is on and lacks one says so before it acts, with the answers that fit: a key is
//! set where keys are typed (J4n never shows or reads a key), a login by the doer's own sign-in, a Decider by setting one up or **picking by hand**
//! (`pick.hand`: the point's chain gets the `human` step, and each ask shows a pick row, see [`super::hand`]), or going on without it, or turning the feature off.
//! A need that is `optional` never asks: it says what an empty setting means, so every setting that names a model, a doer or a Decider is covered.

use super::tiers::{Spent, Tier, find, price_of};
use super::{Front, Said};
use crate::change::Actor;
use crate::table::{Kind, Problem, Tables};
use j4n_proto::{AskView, By, Cell, Choice, Reply, Shape, Tone};
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

/// The need of the orchestrate mode: its id is what `need.waived` holds, and what a waived need stops the tiers for.
pub const ORCHESTRATE: &str = "orchestrate";
/// The models one card of `pick another` offers.
const OFFERED: usize = 11;

/// An entry of kind `need`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NeedRow {
    pub id: String,
    #[serde(default)]
    pub doc: String,
    pub feature: String,
    #[serde(default = "yes")]
    pub when: String,
    #[serde(default = "no")]
    pub off: String,
    #[serde(default)]
    pub parts: Vec<String>,
    #[serde(default)]
    pub doers: Vec<String>,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub suggest: Vec<String>,
    #[serde(default)]
    pub without: String,
    /// `setting` (the default), `key`, `login`, `decider` or `git` (the project's folder has a commit to cut a worktree from).
    #[serde(default = "setting")]
    pub kind: String,
    /// `key`: the connector whose key is needed. `login`: the doer that must be signed in. `decider`: the ask point.
    #[serde(default)]
    pub connector: String,
    #[serde(default)]
    pub doer: String,
    #[serde(default)]
    pub point: String,
    /// The settings the row speaks for besides `doers` and `models`: every setting that names a model, a doer or a Decider is in one row.
    #[serde(default)]
    pub covers: Vec<String>,
    /// Never asks: the row says what the feature does with the setting empty.
    #[serde(default)]
    pub optional: bool,
}

fn setting() -> String {
    "setting".into()
}

fn yes() -> String {
    "true".into()
}

fn no() -> String {
    "false".into()
}

impl Kind for NeedRow {
    const NAME: &'static str = "need";
}

impl NeedRow {
    /// Whether the row reads: its lists are one entry per part, and a way to find a value is one of the three.
    pub fn check(&self) -> Result<(), String> {
        let (key, what) = match self.kind.as_str() {
            "setting" => (None, ""),
            "key" => (Some(&self.connector), "connector"),
            "login" => (Some(&self.doer), "doer"),
            "decider" => (Some(&self.point), "point"),
            "git" => return Ok(()),
            k => return Err(format!("need `{}`: `{k}` is not a kind of need (setting, key, login, decider, git)", self.id)),
        };
        if let Some(v) = key {
            return if v.is_empty() { Err(format!("need `{}`: a {} need names its `{what}`", self.id, self.kind)) } else { Ok(()) };
        }
        let n = self.parts.len();
        if n == 0 || self.doers.len() != n || self.models.len() != n || (self.suggest.len() != n && !self.optional) {
            return Err(format!("need `{}`: `parts`, `doers`, `models` and `suggest` are lists of one entry for each part", self.id));
        }
        match self.suggest.iter().find(|s| !matches!(s.as_str(), "tier:cheap" | "tier:standard" | "doer:ready")) {
            Some(s) => Err(format!("need `{}`: `{s}` is not a way to find a value (`tier:cheap`, `tier:standard`, `doer:ready`)", self.id)),
            None => Ok(()),
        }
    }
}

/// Whether a setting's id names a model, a doer or a Decider: its last word is `doer`, `model`, `judge`, `answer` or `jev`, or it ends in `_doer` or `_model`.
pub fn names_a_model(id: &str) -> bool {
    let last = id.rsplit('.').next().unwrap_or(id);
    matches!(last, "doer" | "model" | "judge" | "answer" | "jev") || last.ends_with("_doer") || last.ends_with("_model")
}

/// The settings among `settings` that name a model, a doer or a Decider and that no row of `rows` speaks for (`doers`, `models` or `covers`): what the law over
/// every pack holds to nothing.
pub fn uncovered<'a>(settings: &'a [String], rows: &[NeedRow]) -> Vec<&'a String> {
    let spoken = |id: &str| rows.iter().any(|r| r.doers.iter().chain(&r.models).chain(&r.covers).any(|c| c == id));
    settings.iter().filter(|id| names_a_model(id) && !spoken(id)).collect()
}

/// What a way of finding a value found for a part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub doer: String,
    pub model: Option<String>,
    /// In words: `claude is signed in; its cheapest model is haiku`.
    pub says: String,
}

/// One part of a need that holds no value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub label: String,
    pub doer_setting: String,
    pub model_setting: String,
    pub found: Option<Found>,
}

/// A need whose feature is on and one of whose parts holds no value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unmet {
    pub need: NeedRow,
    pub parts: Vec<Part>,
    /// For a key, a login or a Decider: what is missing, in words (`the key of light is not set`).
    pub why: String,
}

/// The way `how` finds a value among `ready` (the doers that are signed in and their models, the one to try first first).
pub fn suggestion(how: &str, ready: &[(String, Vec<String>)], prices: &[(String, f64)], spent: &dyn Fn(&str) -> BTreeMap<String, Spent>) -> Option<Found> {
    let tier = match how {
        "tier:cheap" => Tier::Cheap,
        "tier:standard" => Tier::Standard,
        _ => return ready.first().map(|(d, _)| Found { doer: d.clone(), model: None, says: format!("{d} is signed in") }),
    };
    find(ready, prices, spent, tier).map(|(d, p)| Found { says: format!("{d} is signed in; its {} model is {}", tier.word(), p.model), doer: d, model: Some(p.model) })
}

/// The needs of `tables` whose feature is on (`value` says a setting's words) and that hold no value, each with what was found. A need the operator went on
/// without (`need.waived`) is not one.
pub fn unmet(tables: &Tables, value: &dyn Fn(&str) -> String, ready: &dyn Fn() -> Vec<(String, Vec<String>)>, prices: &[(String, f64)], spent: &dyn Fn(&str) -> BTreeMap<String, Spent>, have: &dyn Fn(&NeedRow) -> Option<String>) -> (Vec<Unmet>, Vec<Problem>) {
    let (mut out, mut found) = (Vec::new(), Vec::new());
    let waived = value("need.waived");
    // the doers are asked who is ready only when a feature that is on lacks something: a folder that does not use the feature probes nothing
    let ready_now = std::cell::OnceCell::new();
    for id in tables.ids(NeedRow::NAME) {
        let row = match tables.get::<NeedRow>(id) {
            Ok(r) => r,
            Err(p) => {
                found.push(p);
                continue;
            }
        };
        if let Err(e) = row.check() {
            found.push(Problem::refused(format!("need {id}"), e));
            continue;
        }
        let now = value(&row.feature);
        let on = match row.when.strip_prefix('!') {
            Some(not) => !now.is_empty() && now != not,
            None => now == row.when,
        };
        if !on || row.optional || waived.split_whitespace().any(|w| w == row.id) {
            continue;
        }
        if row.kind != "setting" {
            if let Some(why) = have(&row) {
                out.push(Unmet { need: row, parts: Vec::new(), why });
            }
            continue;
        }
        let parts: Vec<Part> = (0..row.parts.len())
            .filter(|i| value(&row.models[*i]).trim().is_empty())
            .map(|i| Part { label: row.parts[i].clone(), doer_setting: row.doers[i].clone(), model_setting: row.models[i].clone(), found: suggestion(&row.suggest[i], ready_now.get_or_init(ready), prices, spent) })
            .collect();
        if !parts.is_empty() {
            out.push(Unmet { need: row, parts, why: String::new() });
        }
    }
    (out, found)
}

/// The card while it waits, and the needs asked this run.
#[derive(Default)]
pub struct Held(Mutex<State>);

#[derive(Default)]
struct State {
    open: Option<Open>,
    closed: BTreeSet<String>,
    n: u64,
}

struct Open {
    ask: String,
    unmet: Unmet,
    /// What each part is given, as the answers make it: a doer and a model.
    picks: Vec<Option<(String, Option<String>)>>,
    /// The doer whose models `pick another` offers for a part nothing was found for: the first ready one that lists some.
    alt: Option<String>,
    /// The part whose model is being picked (`pick another`); none: the first card.
    at: Option<usize>,
}

impl Front {
    fn held(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.needs.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A setting as words, for a need to read: `lanes` is the switch as it stands now.
    fn need_value(&self, id: &str) -> String {
        match id {
            "lanes" => self.lanes_now().word().to_string(),
            "need.waived" => self.0.cfg().waived.join(" "),
            _ => match self.screen().set.pointer(&format!("/{}", id.replace('.', "/"))) {
                crate::value::Value::Str(s) => s,
                crate::value::Value::Bool(b) => b.to_string(),
                _ => String::new(),
            },
        }
    }

    /// What a key, a login or a Decider need lacks now, in words; none when it holds. Nothing here reads a key: only whether the connector says it has one.
    fn need_have(&self, row: &NeedRow) -> Option<String> {
        match row.kind.as_str() {
            "key" | "login" => {
                let id = if row.kind == "key" { &row.connector } else { &row.doer };
                let Some(d) = self.0.who.doers().iter().find(|d| d.id() == id) else { return Some(format!("{id} is not there: its pack is off")) };
                let p = d.probe();
                match (row.kind.as_str(), p.installed, p.authed) {
                    ("key", _, false) => Some(format!("the key of {id} is not set")),
                    ("login", false, _) => Some(format!("{id} is not installed")),
                    ("login", _, false) => Some(format!("{id} is not signed in")),
                    _ => None,
                }
            }
            "git" => {
                let root = &self.0.cfg().root;
                (!crate::place::checkpoint::has_commit(root)).then(|| "this folder has no git commit, and a chat that writes works in a worktree cut from one".to_string())
            }
            _ => {
                let asks = self.asks();
                let answers = asks.points.get(&row.point).is_some_and(|p| p.chain.iter().any(|s| s.kind.is_model() && asks.registry.get(&s.id()).is_some()));
                (!answers && !self.0.cfg().hand.contains(&row.point)).then(|| format!("no Decider is set up to answer `{}`", row.point))
            }
        }
    }

    /// How many needs are unmet now: what a page may show beside a pack.
    pub fn needs_count(&self, _thread: &str) -> usize {
        self.unmet_quiet().len()
    }

    /// The needs that are unmet now, found without asking a doer who is signed in (a page is drawn often and must not run a program): what lacks is the same, the
    /// suggestions of each part are left out.
    fn unmet_quiet(&self) -> Vec<Unmet> {
        unmet(&self.screen().tables, &|id| self.need_value(id), &Vec::new, &self.0.cfg().prices, &|d| self.spent_by(d), &|row| self.need_have(row)).0
    }

    /// The needs that are unmet now, for each pack that declared them: the words of each (what the need says, else why it is unmet).
    pub fn needs_by_pack(&self, _thread: &str) -> std::collections::BTreeMap<String, Vec<String>> {
        let mut by: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
        for u in self.unmet_quiet() {
            let pack = self.screen().tables.raw(NeedRow::NAME, &u.need.id).and_then(|m| m.from.get("feature")).cloned().unwrap_or_default();
            let words = if u.why.is_empty() { u.need.doc.clone() } else { u.why.clone() };
            by.entry(pack).or_default().push(if words.is_empty() { u.need.id.clone() } else { words });
        }
        by
    }

    /// The needs that are unmet now.
    pub fn needs_unmet(&self, thread: &str) -> Vec<Unmet> {
        let own = self.binding(thread).map(|b| b.doer);
        unmet(&self.screen().tables, &|id| self.need_value(id), &|| self.ready_models(own.as_deref()), &self.0.cfg().prices, &|d| self.spent_by(d), &|row| self.need_have(row)).0
    }

    /// A feature that was switched on, or is about to act, and lacks what it needs says so, once: one card. Called when the settings change, when the
    /// screen starts and before the look reads a line. It holds nothing across what it calls, so an edit the answer makes may call it again.
    pub fn need_check(&self, thread: &str) {
        if self.0.who.headless() || self.held().open.is_some() {
            return;
        }
        let all = self.needs_unmet(thread);
        // a need that holds again is asked again if it goes missing again
        self.held().closed.retain(|id| all.iter().any(|u| u.need.id == *id));
        let Some(u) = all.into_iter().find(|u| !self.held().closed.contains(&u.need.id)) else { return };
        let ask = {
            let mut h = self.held();
            if h.open.is_some() {
                return;
            }
            h.n += 1;
            let ask = format!("need:{}", h.n);
            let picks = u.parts.iter().map(|p| p.found.as_ref().map(|f| (f.doer.clone(), f.model.clone()))).collect();
            let alt = self.ready_models(None).into_iter().find(|(_, l)| !l.is_empty()).map(|(d, _)| d);
            h.open = Some(Open { ask: ask.clone(), unmet: u.clone(), picks, alt, at: None });
            ask
        };
        let mut found: Vec<String> = u.parts.iter().map(|p| format!("{}: {}", p.label, p.found.as_ref().map_or("nothing found", |f| f.says.as_str()))).collect();
        found.extend(Some(u.why.clone()).filter(|w| !w.is_empty()));
        self.write(thread, "need.asked", Actor::Core, json!({ "ask": ask, "need": u.need.id, "kind": u.need.kind, "parts": u.parts.iter().map(|p| &p.label).collect::<Vec<_>>(), "found": found }));
    }

    /// The card, while it waits (`Down::Asks`): what is missing, what was found, and the four answers; or, after `pick another`, the models of a part.
    pub fn need_asks(&self) -> Vec<AskView> {
        let h = self.held();
        let Some(o) = h.open.as_ref() else { return Vec::new() };
        let (n, core) = (&o.unmet.need, |id: &str, label: String, recommended: bool| Choice { id: id.into(), label, recommended, by: By::Core, ..Choice::default() });
        if n.kind != "setting" {
            let (first, label) = match n.kind.as_str() {
                "key" if o.unmet.why.ends_with("its pack is off") => ("key", format!("turn its pack on: Grow › Packs › {}: ⏎", crate::doctor::pack_of(&n.connector))),
                "key" => ("key", format!("set the key: Library › Providers › {}: ⏎ types it", n.connector)),
                "login" => ("login", format!("log in: {}", self.sign_in_words(&n.doer))),
                "git" => ("commit", "make a first commit: `git init` and an empty one".to_string()),
                _ => ("setup", "set up a Decider: Connect › Deciders".to_string()),
            };
            let mut options = vec![core(first, label, true)];
            if n.kind == "decider" {
                options.push(core("hand", "pick yourself: you choose at each ask, one key".into(), false));
            }
            options.push(core("without", format!("go on without it: {}", n.without), false));
            options.push(core("off", format!("turn {} off", n.feature), false));
            let what = match n.kind.as_str() {
                "key" => format!("{} needs the key of {}", n.feature, n.connector),
                "login" => format!("{} needs {} signed in", n.feature, n.doer),
                "git" => format!("{} needs a git commit in this folder", n.feature),
                _ => format!("{} needs a Decider for {}", n.feature, n.point),
            };
            return vec![AskView { ask: o.ask.clone(), asker: "j4n".into(), question: what, shape: Shape::Choice, options, facts: vec![Cell::new(o.unmet.why.clone(), Tone::Dim)], why: n.doc.clone(), ..AskView::default() }];
        }
        if let Some(i) = o.at {
            let part = &o.unmet.parts[i];
            let doer = o.picks[i].as_ref().map(|p| p.0.clone()).or_else(|| o.alt.clone());
            let options = doer.map(|d| self.models_to_pick(&d)).unwrap_or_default().into_iter().map(|(m, price)| Choice { note: price, ..core(&m, m.clone(), false) }).collect();
            return vec![AskView { ask: o.ask.clone(), asker: "j4n".into(), question: format!("Which model for {}?", part.label), shape: Shape::Choice, options, why: "the cheapest first, with its price".into(), ..AskView::default() }];
        }
        let labels: Vec<&str> = o.unmet.parts.iter().map(|p| p.label.as_str()).collect();
        let found: Vec<String> = o.unmet.parts.iter().map(|p| format!("{}: {}", p.label, p.found.as_ref().map_or("nothing found", |f| f.says.as_str()))).collect();
        let mut options = Vec::new();
        let used: Vec<String> = o.unmet.parts.iter().zip(&o.picks).filter_map(|(p, f)| f.as_ref().map(|(d, m)| format!("{} {d}{}", p.label, m.as_ref().map(|m| format!(" · {m}")).unwrap_or_default()))).collect();
        if !used.is_empty() {
            options.push(core("use", format!("use the suggestion: {}", used.join(", ")), true));
        }
        if !used.is_empty() || o.alt.is_some() {
            options.push(core("pick", "pick another".into(), used.is_empty()));
        }
        options.push(core("without", format!("go on without it: {}", n.without), false));
        options.push(core("off", format!("turn {} off", n.feature), false));
        vec![AskView {
            ask: o.ask.clone(),
            asker: "j4n".into(),
            question: format!("{} needs {}", n.feature, labels.join(" and ")),
            shape: Shape::Choice,
            options,
            facts: found.iter().map(|f| Cell::new(f.clone(), Tone::Dim)).collect(),
            why: n.doc.clone(),
            ..AskView::default()
        }]
    }

    /// The doer's own way to sign in, as the pre-flight words it; where it says none, the tab that does.
    fn sign_in_words(&self, doer: &str) -> String {
        let rows = self.cfg().setup.as_ref().map(|c| c.report_for(self.main_pick()).rows).unwrap_or_default();
        rows.into_iter().find(|r| r.kind == "doer" && r.id == doer).map(|r| r.fix).filter(|f| !f.is_empty()).unwrap_or_else(|| "Connect shows how".into())
    }

    /// The models of `doer`, the cheapest first, with the price word: what `pick another` offers.
    fn models_to_pick(&self, doer: &str) -> Vec<(String, String)> {
        let cfg = self.0.cfg();
        let mut list: Vec<(f64, String)> = cfg.models.models(doer).unwrap_or_default().into_iter().map(|m| (price_of(&cfg.prices, &m).unwrap_or(f64::MAX), m)).collect();
        list.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        list.into_iter().take(OFFERED).map(|(p, m)| (m, if p == f64::MAX { String::new() } else { format!("price {p}") })).collect()
    }

    /// The reply of the operator to the card: `None` when `ask` is not it.
    pub(super) fn need_reply(&self, thread: &str, ask: &str, reply: &Reply) -> Option<Said> {
        if !ask.starts_with("need:") {
            return None;
        }
        let mut h = self.held();
        if h.open.as_ref().is_none_or(|o| o.ask != ask) {
            return Some(Said::Refused("that question is not open any more".into()));
        }
        // the first card, or the models of a part
        if let Some(i) = h.open.as_ref().and_then(|o| o.at) {
            let Reply::Pick { id } = reply else { return Some(self.refuse(thread, "answer with one of the models".into())) };
            let o = h.open.as_mut().expect("open was checked");
            let doer = o.picks[i].as_ref().map(|p| p.0.clone()).or_else(|| o.alt.clone()).unwrap_or_default();
            o.picks[i] = Some((doer, Some(id.clone())));
            o.at = (i + 1 < o.picks.len() && (o.alt.is_some() || o.picks[i + 1].is_some())).then_some(i + 1);
            if o.at.is_some() {
                return Some(Said::Nothing);
            }
            let o = h.open.take().expect("open was checked");
            h.closed.insert(o.unmet.need.id.clone());
            drop(h);
            return Some(self.need_use(thread, &o, "pick"));
        }
        let answer = match reply {
            Reply::Pick { id } => id.as_str(),
            // no: not now, and not again this run
            Reply::No => "later",
            _ => return Some(self.refuse(thread, "answer with one of the options".into())),
        };
        if answer == "pick" {
            let o = h.open.as_mut().expect("open was checked");
            if o.alt.is_none() && o.picks.iter().all(Option::is_none) {
                return Some(self.refuse(thread, "no doer that is signed in has a model to pick from: Connect shows what to mend".into()));
            }
            o.at = Some(0);
            return Some(Said::Nothing);
        }
        let Some(o) = h.open.take() else { return Some(Said::Nothing) };
        if !matches!(answer, "use" | "without" | "off" | "later" | "key" | "login" | "setup" | "hand" | "commit") {
            h.open = Some(o);
            return Some(self.refuse(thread, "answer with one of the options".into()));
        }
        // closed before anything is written: an edit calls `need_check` again, and must not find it open
        if answer != "off" {
            h.closed.insert(o.unmet.need.id.clone());
        }
        drop(h);
        Some(match answer {
            "use" => self.need_use(thread, &o, "use"),
            "without" => self.need_without(thread, &o),
            "off" => self.need_off(thread, &o),
            "key" | "login" | "setup" => self.need_lead(thread, &o, answer),
            "hand" => self.need_hand(thread, &o),
            "commit" => self.need_commit(thread, &o),
            _ => {
                self.write(thread, "need.answered", Actor::Operator, json!({ "ask": ask, "need": o.unmet.need.id, "answer": "later" }));
                self.note(thread, &format!("{} still lacks {}: asked again at the next start", o.unmet.need.feature, if o.unmet.why.is_empty() { o.unmet.parts.iter().map(|p| p.label.as_str()).collect::<Vec<_>>().join(" and ") } else { o.unmet.why.clone() }), "dim");
                Said::Nothing
            }
        })
    }

    /// A setting written as the operator's edit, in the file named after its first word; the words that take it back.
    fn need_write(&self, id: &str, value: &str) -> Result<String, String> {
        let file = format!("{}.toml", id.split('.').next().unwrap_or(id));
        self.edit_setting(id, &file, toml::Value::String(value.into()), &format!("{id}: {value}"))
    }

    /// The suggestion, or the models picked, are written: the doer and the model of each part.
    fn need_use(&self, thread: &str, o: &Open, answer: &str) -> Said {
        let mut said = Vec::new();
        let mut back = String::new();
        for (part, pick) in o.unmet.parts.iter().zip(&o.picks) {
            let Some((doer, model)) = pick else { continue };
            let sets = [(&part.doer_setting, Some(doer)), (&part.model_setting, model.as_ref())];
            for (id, value) in sets.into_iter().filter(|(id, _)| !id.is_empty()) {
                let Some(value) = value else { continue };
                match self.need_write(id, value) {
                    Ok(b) => back.push_str(&b),
                    Err(why) => return self.refuse(thread, format!("{id} was not written: {why}")),
                }
            }
            said.push(format!("{} on {doer}{}", part.label, model.as_ref().map(|m| format!(" · {m}")).unwrap_or_default()));
        }
        self.write(thread, "need.answered", Actor::Operator, json!({ "ask": o.ask, "need": o.unmet.need.id, "answer": answer, "set": said }));
        self.note(thread, &format!("{}: {}{back}", o.unmet.need.feature, said.join(" · ")), "accent");
        Said::Nothing
    }

    /// A key, a login or a Decider is the operator's to set: the row says where, with the key that goes there. Nothing is written, and the card waits until the
    /// need holds again.
    fn need_lead(&self, thread: &str, o: &Open, answer: &str) -> Said {
        let n = &o.unmet.need;
        self.write(thread, "need.answered", Actor::Operator, json!({ "ask": o.ask, "need": n.id, "answer": answer }));
        let (text, lead) = match answer {
            "key" if o.unmet.why.ends_with("its pack is off") => (format!("{}: {} is in the pack {}, which is off · Grow › Packs › {}: ⏎ turns it on", n.feature, n.connector, crate::doctor::pack_of(&n.connector), crate::doctor::pack_of(&n.connector)), super::Lead::open("open Packs", "packs")),
            "key" => (format!("{}: type the key at Library › Providers › {} · ⏎ opens it", n.feature, n.connector), super::Lead::open("type the key", super::keyfile::PAGE)),
            "login" => (format!("{}: sign {} in: {} · Connect shows the state", n.feature, n.doer, self.sign_in_words(&n.doer)), super::Lead::open("open Connect", "connectors")),
            _ => (format!("{}: set up a Decider at Connect › Deciders · ⏎ opens it", n.feature), super::Lead::open("open the deciders", "deciders")),
        };
        self.note_to(thread, &text, "accent", lead);
        Said::Nothing
    }

    /// A first commit is made in the project's folder (`git init` first where it is none), so a chat that writes has a commit to cut its worktree from.
    fn need_commit(&self, thread: &str, o: &Open) -> Said {
        let n = &o.unmet.need;
        let root = self.0.cfg().root.clone();
        let root = if root.as_os_str().is_empty() { std::env::current_dir().unwrap_or_default() } else { root };
        match crate::place::checkpoint::first_commit(&root) {
            Ok(()) => {
                self.write(thread, "need.answered", Actor::Operator, json!({ "ask": o.ask, "need": n.id, "answer": "commit" }));
                self.note(thread, &format!("{}: a first commit is made in {} · a chat that writes now works in a worktree, and sees what is committed · `git log` shows it", n.feature, root.display()), "accent");
                Said::Nothing
            }
            Err(why) => self.refuse(thread, why),
        }
    }

    /// Pick by hand: the point goes into `pick.hand`, so its chain gets the `human` step and each ask shows the pick row.
    fn need_hand(&self, thread: &str, o: &Open) -> Said {
        let n = &o.unmet.need;
        let mut list = self.0.cfg().hand.clone();
        list.push(n.point.clone());
        match self.need_write("pick.hand", &list.join(" ")) {
            Ok(back) => {
                self.write(thread, "need.answered", Actor::Operator, json!({ "ask": o.ask, "need": n.id, "answer": "hand", "point": n.point }));
                self.note(thread, &format!("{}: you answer `{}` yourself: one key at each ask · Esc takes the fallback{back}", n.feature, n.point), "accent");
                Said::Nothing
            }
            Err(why) => self.refuse(thread, why),
        }
    }

    /// Go on without: the need is added to `need.waived`, so no tier picks for it and the card is not asked again.
    fn need_without(&self, thread: &str, o: &Open) -> Said {
        let mut list = self.0.cfg().waived.clone();
        list.push(o.unmet.need.id.clone());
        match self.need_write("need.waived", &list.join(" ")) {
            Ok(back) => {
                self.write(thread, "need.answered", Actor::Operator, json!({ "ask": o.ask, "need": o.unmet.need.id, "answer": "without" }));
                self.note(thread, &format!("{}: going on without it · {}{back}", o.unmet.need.feature, o.unmet.need.without), "accent");
                Said::Nothing
            }
            Err(why) => self.refuse(thread, why),
        }
    }

    /// Turn the feature off: its switch goes to the row's `off` word.
    fn need_off(&self, thread: &str, o: &Open) -> Said {
        let n = &o.unmet.need;
        self.write(thread, "need.answered", Actor::Operator, json!({ "ask": o.ask, "need": n.id, "answer": "off" }));
        if n.feature == "lanes" {
            return self.lanes_said(thread, std::slice::from_ref(&n.off));
        }
        let off = match n.off.as_str() {
            "true" | "false" => toml::Value::Boolean(n.off == "true"),
            w => toml::Value::String(w.into()),
        };
        let file = format!("{}.toml", n.feature.split('.').next().unwrap_or(&n.feature));
        match self.edit_setting(&n.feature, &file, off, &format!("{}: {}", n.feature, n.off)) {
            Ok(back) => {
                self.note(thread, &format!("{} is {}{back}", n.feature, n.off), "accent");
                Said::Nothing
            }
            Err(why) => self.refuse(thread, why),
        }
    }
}
