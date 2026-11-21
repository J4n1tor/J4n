//! Offload: a light chore goes by rule to a cheaper doer and comes back as a row under the turn. The main
//! lane stays the only planner and the only writer. This is the data and the decisions, with no front and no task in it: which line is a chore
//! (the rows of the kind `chore`, the pack `offload`), and where a model chore goes (the rows' order, the windows the Log shows, the day's
//! spend against the cap). The front starts the task ([`crate::front`] `offload.rs`) and a test reads the same functions.

use crate::table::{Kind, Problem, Tables};
use serde::Deserialize;
use std::sync::Arc;

/// An entry of kind `chore`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chore {
    pub id: String,
    pub doc: Option<String>,
    pub starts: Vec<String>,
    pub how: String,
    #[serde(default)]
    pub check: String,
    pub usd: Option<f64>,
    pub wall: Option<String>,
}

impl Kind for Chore {
    const NAME: &'static str = "chore";
}

impl Chore {
    /// A review asks before it goes: it is the one chore that reads the main lane's own work.
    pub fn asks(&self) -> bool {
        self.how == "review"
    }

    fn check(&self) -> Result<(), String> {
        if !["check", "model", "review"].contains(&self.how.as_str()) || self.starts.is_empty() || self.wall.as_deref().is_some_and(|w| crate::expr::Expr::dur(w).is_none()) {
            return Err(format!("chore `{}`: `how` is check, model or review, `starts` has words, `wall` is a duration such as \"2m\"", self.id));
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    chore: Vec<Chore>,
}

/// What the switch and its settings say (`packs/offload/settings/offload.toml`).
#[derive(Debug, Clone)]
pub struct Offload {
    /// The switch `offload.on`. Off: every line is the main lane's.
    pub on: bool,
    pub cap_usd: f64,
    pub busy_at: f64,
    pub order: Vec<String>,
    pub model: String,
    pub own_model: String,
    pub writes: Vec<String>,
    pub max_words: usize,
    /// `offload.worth`: how many times dearer per token the main model must be than the chore model before a chore on claude is sent away; 0: always.
    pub worth: f64,
    /// `offload.prices`: what each model word costs per token, relative to the others (`haiku=1 sonnet=3`); a model with no word here has no price.
    pub prices: Vec<(String, f64)>,
    pub chores: Arc<Vec<Chore>>,
}

impl Default for Offload {
    /// Off, with no chore: a front nothing wired offload into sends nothing away.
    fn default() -> Offload {
        Offload { on: false, cap_usd: 2.0, busy_at: 0.8, order: Vec::new(), model: "haiku".into(), own_model: String::new(), writes: Vec::new(), max_words: 30, worth: 0.0, prices: Vec::new(), chores: Arc::default() }
    }
}

/// What a line is, if it is a chore: the class, and the words of the line.
#[derive(Debug, Clone, PartialEq)]
pub struct Taken<'a> {
    pub chore: &'a Chore,
    pub line: String,
}

/// Words that may follow a `check` chore's own: it asks for no more than the command.
const FILLER: [&str; 8] = ["please", "now", "again", "quickly", "for", "me", "once", "more"];

fn words(line: &str) -> Vec<String> {
    line.split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_').to_lowercase()).filter(|w| !w.is_empty()).collect()
}

impl Offload {
    /// What a model costs per token, relative to the others: the first word of `offload.prices` that its name holds as a whole word (`claude-sonnet-5-5` holds `sonnet`).
    fn price(&self, model: &str) -> Option<f64> {
        crate::front::tiers::price_of(&self.prices, model)
    }

    /// Why a chore on claude is not worth a second session: the main model is not dearer than the chore model by `offload.worth` times, so
    /// the chore's own session costs more than the main lane's cache saves. None: it is worth it, or a price is not known (then it is sent, as before).
    fn near(&self, main: &str) -> Option<String> {
        if self.worth <= 0.0 {
            return None;
        }
        let (m, c) = (self.price(main)?, self.price(&self.model)?);
        (m < self.worth * c).then(|| format!("{main} is {:.1}× {} per token, under the {}× that makes a second session pay (`offload.worth`)", m / c, self.model, self.worth))
    }

    /// The rows the Table holds (kind `chore`, pack `offload`); a row that does not read is left out, and its fault returned. The pack off: no row.
    pub fn load(tables: &Tables, on: bool, num: impl Fn(&str, f64) -> f64, word: impl Fn(&str, &str) -> String) -> (Offload, Vec<Problem>) {
        let (mut rows, mut problems) = (Vec::new(), Vec::new());
        for id in tables.ids(Chore::NAME) {
            match tables.get::<Chore>(id) {
                Ok(r) => match r.check() {
                    Ok(()) => rows.push(r),
                    Err(e) => problems.push(Problem::refused(format!("chore {id}"), e)),
                },
                Err(p) => problems.push(p),
            }
        }
        let list = |s: String| s.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>();
        let me = Offload {
            on: on && !rows.is_empty(),
            cap_usd: num("offload.cap_usd", 2.0),
            busy_at: num("offload.busy_at", 0.8),
            order: list(word("offload.order", "haiku pro own")),
            model: word("offload.model", "haiku"),
            own_model: word("offload.own_model", ""),
            writes: list(word("offload.writes", "")),
            max_words: num("offload.max_words", 30.0) as usize,
            worth: num("offload.worth", 3.0).max(0.0),
            prices: crate::front::tiers::parse(&word("offload.prices", crate::front::tiers::PRICES)),
            chores: Arc::new(rows),
        };
        (me, problems)
    }

    /// The rows of the pack as it ships, and the words of the setting `offload.writes` as it ships: what a test starts from.
    pub fn shipped() -> Offload {
        let file: File = toml::from_str(include_str!("../../../packs/offload/chores/chores.toml")).unwrap_or_else(|e| panic!("packs/offload/chores/chores.toml: {e}"));
        let settings: toml::Table = toml::from_str(include_str!("../../../packs/offload/settings/offload.toml")).unwrap_or_default();
        let writes = settings.get("setting").and_then(toml::Value::as_array).into_iter().flatten().find(|s| s.get("id").and_then(toml::Value::as_str) == Some("offload.writes")).and_then(|s| s.get("default")?.as_str()).unwrap_or_default();
        Offload { on: true, order: vec!["haiku".into(), "pro".into(), "own".into()], writes: writes.split_whitespace().map(String::from).collect(), worth: 3.0, prices: crate::front::tiers::parse("haiku=1 sonnet=3 opus=5"), chores: Arc::new(file.chore), ..Offload::default() }
    }

    /// Whether a line is a chore: it starts with the words of a class as whole words, it is short, it has no word that says it writes, and a
    /// `check` chore is no more than its command. `None`: the line is the main lane's.
    pub fn take(&self, line: &str) -> Option<Taken<'_>> {
        let w = words(line);
        if !self.on || w.is_empty() || w.len() > self.max_words || line.trim().contains('\n') {
            return None;
        }
        let (chore, n) = self.chores.iter().find_map(|c| {
            let starts = c.starts.iter().map(|s| words(s));
            starts.filter(|s| w.starts_with(s)).map(|s| s.len()).max().map(|n| (c, n))
        })?;
        // what follows the class's own words says nothing that writes (`review my change` is a noun; `run the tests and commit` is not), and a
        // `check` chore is no more than its command
        if w[n..].iter().any(|x| self.writes.contains(x)) || (chore.how == "check" && !w[n..].iter().all(|x| FILLER.contains(&x.as_str()))) {
            return None;
        }
        Some(Taken { chore, line: line.trim().to_string() })
    }
}

/// What the route reads, apart from the settings: the doers, the windows, the spend. The front fills it from the world; a test fills it by hand.
pub struct Facts<'a> {
    /// The doer and the account the main lane runs as (an empty account is the operator's own login).
    pub main: (&'a str, &'a str),
    /// The doers that are installed, signed in and good for the chore's role.
    pub ready: &'a [String],
    /// The accounts whose plan is `pro`.
    pub pro: &'a [String],
    /// How full the tightest window of a doer's account stands now (0 to 1); none: no reading, or it has reset.
    pub window: &'a dyn Fn(&str, &str) -> Option<f64>,
    /// What the chores paid per request took today, and what the running ones may still take, in dollars.
    pub spent: f64,
    /// The model the main lane runs (its model word or name); empty: not known, and then no price is compared.
    pub main_model: &'a str,
    /// What the gateway says is left of the key's credits; none: not known.
    pub credits: &'a dyn Fn() -> Option<f64>,
}

/// Where a model chore goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Route {
    Go { doer: String, model: String, account: Option<String>, paid: bool, why: String },
    /// It stays with the main lane, and why.
    Stay { why: String },
}

/// Where a model chore of up to `usd` dollars goes: the first of the order that is free. `haiku` is claude on the main lane's account while its
/// window is not busy; `pro` is claude on an account of the plan `pro` while that window is not busy (so it is what a busy main window sends
/// there); `own` is the gateway doer, only under the day's cap and while the key has credits. A window is read from what the Log shows
/// (`run.window`), never asked of the provider.
pub fn route(o: &Offload, f: &Facts, usd: f64) -> Route {
    let (mut why, claude) = (Vec::new(), "claude");
    let ready = |d: &str| f.ready.iter().any(|r| r == d);
    let busy = |account: &str| (f.window)(claude, account).filter(|u| *u >= o.busy_at);
    let near = o.near(f.main_model);
    for step in &o.order {
        match step.as_str() {
            "haiku" | "pro" if !ready(claude) => why.push(format!("{step}: claude is not ready")),
            // a second claude session pays only where the main model is dearer per token than the chore's by `offload.worth` times
            "haiku" | "pro" if near.is_some() => why.push(format!("{step}: {}", near.as_deref().unwrap_or_default())),
            "haiku" => {
                let account = if f.main.0 == claude { f.main.1 } else { "" };
                match busy(account) {
                    Some(u) => why.push(format!("haiku: the window is {:.0}% used", u * 100.0)),
                    None => return Route::Go { doer: claude.into(), model: o.model.clone(), account: Some(account.to_string()).filter(|a| !a.is_empty()), paid: false, why: format!("{} on {}, the window has room", o.model, if account.is_empty() { "your login" } else { account }) },
                }
            }
            "pro" => match f.pro.iter().find(|a| busy(a).is_none()) {
                Some(a) if f.main.1 != a.as_str() => return Route::Go { doer: claude.into(), model: o.model.clone(), account: Some(a.clone()), paid: false, why: format!("{} on the pro account {a}, the main window is busy", o.model) },
                _ => why.push(if f.pro.is_empty() { "pro: no account has the plan pro".to_string() } else { "pro: no pro account is free".to_string() }),
            },
            "own" if !ready("own") => why.push("own: no gateway key".into()),
            "own" if f.spent + usd > o.cap_usd => why.push(format!("own: ${:.2} of ${:.2} is spent today", f.spent, o.cap_usd)),
            "own" if (f.credits)().is_some_and(|c| c <= 0.0) => why.push("own: the key has no credits left".into()),
            "own" => return Route::Go { doer: "own".into(), model: o.own_model.clone(), account: None, paid: true, why: format!("the gateway key, ${:.2} of ${:.2} spent today", f.spent, o.cap_usd) },
            other => why.push(format!("{other}: not a place in `offload.order`")),
        }
    }
    Route::Stay { why: if why.is_empty() { "`offload.order` names no place".into() } else { why.join("; ") } }
}
