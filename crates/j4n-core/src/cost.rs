//! What a request costs the operator: the list cost is tokens times prices, and the plan cost is the share of a
//! subscription's windows the request took. A plan is a row (`packs/base/kinds/plan.toml`); the money of a window is the price over
//! the windows that fit in a period. What a request took of a window is its list cost over what a full window is worth in list dollars:
//! measured by [`PlanRate`] from the pairs of a window's movement and the list cost spent in it, or, before `cost.plan.min_n` pairs,
//! assumed (`assume` of the plan row, an over-estimate on purpose). The larger share of the windows is the one charged.
//!
//! The price is the operator's: a plan without one has no plan cost, and says so (law 6: no invented numbers).

use crate::change::Actor;
use crate::expr::Expr;
use crate::fold::{Fold, Folds};
use crate::log::Ev;
use crate::table::{Kind, Problem, Tables};
use crate::value::{Key, Value};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, VecDeque};

/// A month, in milliseconds: 30.44 days, so $24 over 4.35 weeks is $5.5 a week.
const MONTH_MS: f64 = 30.44 * 86_400_000.0;
const WEEK_MS: f64 = 7.0 * 86_400_000.0;
/// Pairs a window keeps: the median is over the latest of them.
const KEEP: usize = 30;
/// Less than a step of the windows' 0.01 is no movement.
const EPS: f64 = 1e-9;

/// An entry of kind `plan`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanRow {
    pub id: String,
    pub doc: Option<String>,
    pub connector: String,
    pub account: Option<String>,
    pub price: Option<String>,
    pub period: Option<String>,
    pub assume: Option<String>,
    pub windows: Vec<String>,
}

impl Kind for PlanRow {
    const NAME: &'static str = "plan";
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub id: String,
    pub connector: String,
    pub account: Option<String>,
    /// What it costs for a period; none: the operator has not said.
    pub price: Option<f64>,
    /// What a period is worth in list dollars, taken to be so until a window is measured; none: no plan cost before that.
    pub assume: Option<f64>,
    period_ms: f64,
    /// The windows the provider counts, and how long each is.
    windows: Vec<(String, u64)>,
}

impl Plan {
    /// How long a window of the plan is, in milliseconds (`/usage` places a reading in it).
    pub fn window_ms(&self, window: &str) -> Option<u64> {
        self.windows.iter().find(|(w, _)| w == window).map(|(_, ms)| *ms)
    }

    /// What one full window is worth in money: the price over the windows that fit in a period.
    pub fn money(&self, window: &str) -> Option<f64> {
        let len = self.windows.iter().find(|(w, _)| w == window)?.1 as f64;
        Some(self.price? * len / self.period_ms)
    }

    /// What a request of `usd` list cost took of the plan, in money. A window with `min_n` pairs in `rate` is measured: the request's
    /// share of a full window (its list cost over the median list dollars of one) times what a full window is worth, and the
    /// largest share of the measured windows is the one charged, with its window. When none is measured, the plan's assumed capacity
    /// says it (`price` over `assume`, an over-estimate on purpose). None: no price, or nothing measured and nothing assumed.
    pub fn cost(&self, usd: f64, min_n: usize, rate: impl Fn(&str) -> Option<(usize, f64)>) -> Option<Cost> {
        let price = self.price?;
        let share = |(name, _): &(String, u64)| {
            let (n, per) = rate(name).filter(|(n, per)| *n >= min_n.max(1) && *per > 0.0)?;
            Some(Cost { usd: usd * self.money(name)? / per, window: Some(name.clone()), source: format!("measured n={n}") })
        };
        let measured = self.windows.iter().filter_map(share).max_by(|a, b| a.usd.total_cmp(&b.usd));
        measured.or_else(|| self.assume.map(|a| Cost { usd: usd * price / a, window: None, source: "assumed".into() }))
    }
}

/// What a request took of one window: a fraction of it, and which window (`0.8% of 5h`). It needs no price: the window's worth in
/// list dollars is measured or assumed, and the plan's price is only what turns the share into money.
#[derive(Debug, Clone, PartialEq)]
pub struct Share {
    /// 0 to 1: the request's list cost over what a full window is worth in list dollars.
    pub frac: f64,
    /// The window's length as the operator reads it: `5h`, `7d`.
    pub of: String,
    /// The window's name, when it was measured; none: the plan's assumed capacity said it, of its shortest window.
    pub window: Option<String>,
    /// `measured n=12` or `assumed`.
    pub source: String,
}

/// A window's length as the operator reads it.
fn label(ms: u64) -> String {
    match ms {
        m if m % 86_400_000 == 0 => format!("{}d", m / 86_400_000),
        m if m % 3_600_000 == 0 => format!("{}h", m / 3_600_000),
        m => format!("{}m", m / 60_000),
    }
}

impl Plan {
    /// What a request of `usd` list cost took of a window. A measured window (`min_n` pairs in `rate`) says its share of a full one;
    /// of the measured windows the one that binds is named, by what it takes in money when the plan has a price (the window the plan
    /// cost charges) and by the larger part of a window when it has none. With none measured the assumed capacity says it, of the
    /// shortest window. None: nothing measured and nothing assumed.
    pub fn share(&self, usd: f64, min_n: usize, rate: impl Fn(&str) -> Option<(usize, f64)>) -> Option<Share> {
        let measured = self
            .windows
            .iter()
            .filter_map(|(name, len)| {
                let (n, per) = rate(name).filter(|(n, per)| *n >= min_n.max(1) && *per > 0.0)?;
                let frac = usd / per;
                Some((frac * self.money(name).unwrap_or(1.0), Share { frac, of: label(*len), window: Some(name.clone()), source: format!("measured n={n}") }))
            })
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, s)| s);
        measured.or_else(|| {
            let (_, len) = self.windows.iter().min_by_key(|(_, len)| *len)?;
            let per = self.assume? * *len as f64 / self.period_ms;
            Some(Share { frac: usd / per, of: label(*len), window: None, source: "assumed".into() })
        })
    }

    /// What a request says when its plan has no price: the row of Agents › Plans whose `⏎` sets one.
    pub fn unset(&self) -> String {
        format!("no price: Agents › Plans, ⏎ on {} sets it", self.id)
    }
}

/// The shares of a task or a thread, by window: what the requests took of each, added up.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Shares(BTreeMap<String, f64>);

impl Shares {
    pub fn add(&mut self, of: &str, frac: f64) {
        if frac.is_finite() && frac > 0.0 {
            *self.0.entry(of.to_string()).or_insert(0.0) += frac;
        }
    }

    pub fn merge(&mut self, other: &Shares) {
        for (of, frac) in &other.0 {
            self.add(of, *frac);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The window the most was taken of, as `0.8% of 5h`; none when no request had a share.
    pub fn words(&self) -> Option<String> {
        let (of, frac) = self.0.iter().max_by(|a, b| a.1.total_cmp(b.1))?;
        Some(format!("{} of {of}", percent(*frac)))
    }
}

/// `0.8%`, `12%`, `<0.1%`: a share of a window.
pub fn percent(frac: f64) -> String {
    match frac {
        f if f < 0.0005 => "<0.1%".into(),
        f if f < 0.1 => format!("{:.1}%", f * 100.0),
        f => format!("{:.0}%", f * 100.0),
    }
}

/// How far a window's `utilization` moves at a time (as claude reports it: a fraction of 1 in steps of 0.01).
pub const STEP: f64 = 0.01;

/// What a request that makes a pair for the plan's quickest window has to cost (`:calibrate`).
#[derive(Debug, Clone, PartialEq)]
pub struct Calibration {
    pub plan: String,
    pub window: String,
    /// What a full window is worth in list dollars, and whether that is measured (`measured n=12`) or `assumed`.
    pub per: f64,
    pub source: String,
    /// The list cost of a request that moves the window by two steps, so that one step is certain whatever the reading was inside it.
    pub target: f64,
    /// What `target` is of the window.
    pub share: f64,
    /// What `target` is of the plan, in money; none when the plan has no price.
    pub money: Option<f64>,
    /// A pair needs the window to have moved once before the request; if it never did, two requests are asked for.
    pub requests: u32,
    /// How many numbers the request writes (`cost.calibrate.size`): said by the caller, who knows the setting.
    pub size: u32,
}

impl Plan {
    /// The window that moves in the least list cost is the one that is calibrated: the shortest. None when what a full window is
    /// worth is not known (nothing measured, no `assume`).
    pub fn calibration(&self, rate: impl Fn(&str) -> Option<(usize, f64)>, anchored: impl Fn(&str) -> bool) -> Option<Calibration> {
        let (name, len) = self.windows.iter().min_by_key(|(_, len)| *len)?;
        let (per, source) = match rate(name).filter(|(n, per)| *n >= 1 && *per > 0.0) {
            Some((n, per)) => (per, format!("measured n={n}")),
            None => (self.assume? * *len as f64 / self.period_ms, "assumed".into()),
        };
        let share = 2.0 * STEP;
        Some(Calibration {
            plan: self.id.clone(),
            window: name.clone(),
            per,
            source,
            target: per * share,
            share,
            money: self.money(name).map(|m| m * share),
            requests: if anchored(name) { 1 } else { 2 },
            size: 0,
        })
    }
}

/// The plan cost of a request, and where it comes from.
#[derive(Debug, Clone, PartialEq)]
pub struct Cost {
    pub usd: f64,
    /// The window that binds, when it was measured.
    pub window: Option<String>,
    /// `measured n=12` or `assumed`.
    pub source: String,
}

/// Which of the two numbers a request on a plan is shown with (`cost.show`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Show {
    Plan,
    List,
    #[default]
    Both,
}

impl Show {
    /// The setting's word; one that is not a word of the three is `both`.
    pub fn word(w: &str) -> Show {
        match w {
            "plan" => Show::Plan,
            "list" => Show::List,
            _ => Show::Both,
        }
    }

    /// The money of something that ran on a plan, as `show` says: the plan cost first with the list cost beside it. `cap` is the
    /// cap the plan cost is held to; something that ran on no plan has one number, which is both.
    pub fn money(self, planned: bool, plan: &str, cap: Option<&str>, list: &str) -> String {
        self.money_of(None, planned, plan, cap, list)
    }

    /// The same with the share of the window the requests took (`0.8% of 5h`, [`Shares::words`]) first, as the number that means
    /// something on a plan: the plan cost follows it when the plan has a price, and the list cost is last. A plan with no price has
    /// no plan cost, so its share stands beside the list cost. `list` alone says no share.
    pub fn money_of(self, share: Option<&str>, planned: bool, plan: &str, cap: Option<&str>, list: &str) -> String {
        let Some(share) = share.filter(|_| self != Show::List) else {
            return self.money_plain(planned, plan, cap, list);
        };
        let head = match cap {
            Some(c) => format!("{plan} of {c}"),
            None => plan.to_string(),
        };
        match (planned, self) {
            (true, Show::Plan) => format!("{share} · {head} plan"),
            (true, _) => format!("{share} · {head} plan · {list} list"),
            (false, Show::Plan) => share.to_string(),
            (false, _) => format!("{share} · {head} list"),
        }
    }

    fn money_plain(self, planned: bool, plan: &str, cap: Option<&str>, list: &str) -> String {
        let head = match cap {
            Some(c) => format!("{plan} of {c}"),
            None => plan.to_string(),
        };
        match (planned, self) {
            (false, _) => head,
            (true, Show::List) if cap.is_none() => format!("{list} list"),
            (true, Show::Plan) => format!("{head} plan"),
            (true, _) => format!("{head} plan · {list} list"),
        }
    }
}

/// An entry of kind `cost_source`: how one connector says what a request cost and how full its windows are.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRow {
    pub id: String,
    pub doc: Option<String>,
    pub connector: String,
    pub list: Option<String>,
    pub windows: String,
    /// How long the provider keeps a call's prefix at the cheap read price (`5m`), for `/context`.
    pub cache: Option<String>,
    /// What a token read from the cache, written to it and put out costs, each as a multiple of a fresh input token.
    pub ratio: Option<Ratios>,
}

/// The provider's price of a cached read, a cache write and an output token, each as a multiple of a fresh input token: with the list cost of
/// a turn they give the price of one input token (`task::context`), and so what the next line costs, warm or cold.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ratios {
    pub read: f64,
    pub write: f64,
    pub out: f64,
}

impl Kind for SourceRow {
    const NAME: &'static str = "cost_source";
}

/// The word of `windows` that says a connector shows none.
pub const NO_WINDOWS: &str = "none";

#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub connector: String,
    /// Where the stream keeps the windows; none: it shows none and a request is priced by its list cost.
    pub windows: Option<String>,
    /// How long after a call the provider still reads its prefix from the cache; none: the row does not say, and no turn is called warm or cold.
    pub cache_ms: Option<u64>,
    pub ratios: Option<Ratios>,
}

/// The plans as they are now: an edit of a `plan` or an `account` row puts new ones in. A clone is a second handle.
#[derive(Clone, Default)]
pub struct LivePlans(std::sync::Arc<std::sync::RwLock<std::sync::Arc<Plans>>>);

impl LivePlans {
    pub fn of(plans: std::sync::Arc<Plans>) -> LivePlans {
        LivePlans(std::sync::Arc::new(std::sync::RwLock::new(plans)))
    }

    pub fn now(&self) -> std::sync::Arc<Plans> {
        self.0.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set(&self, plans: Plans) {
        *self.0.write().unwrap_or_else(|e| e.into_inner()) = std::sync::Arc::new(plans);
    }
}

#[derive(Default)]
pub struct Plans {
    plans: Vec<Plan>,
    sources: Vec<Source>,
    /// `(account, plan id)`: the plan an account names (`plan = "pro"` on its row).
    named: Vec<(String, String)>,
}

impl Plans {
    /// The rows of the Table. A row that cannot be used is a problem and no plan.
    pub fn load(tables: &Tables) -> (Plans, Vec<Problem>) {
        let (mut plans, mut sources, mut problems) = (Vec::new(), Vec::new(), Vec::new());
        for id in tables.ids(SourceRow::NAME) {
            match tables.get::<SourceRow>(id) {
                Ok(r) => sources.push(Source { connector: r.connector, windows: Some(r.windows.trim().to_string()).filter(|w| !w.is_empty() && w != NO_WINDOWS), cache_ms: r.cache.as_deref().and_then(crate::expr::Expr::dur).filter(|ms| *ms > 0), ratios: r.ratio.filter(|x| [x.read, x.write, x.out].iter().all(|n| n.is_finite() && *n >= 0.0)) }),
                Err(p) => problems.push(p),
            }
        }
        for id in tables.ids(PlanRow::NAME) {
            let r = match tables.get::<PlanRow>(id) {
                Ok(r) => r,
                Err(p) => {
                    problems.push(p);
                    continue;
                }
            };
            match Plan::from_row(&r) {
                Ok(p) => plans.push(p),
                Err(why) => problems.push(Problem::refused(format!("plan {}", r.id), why)),
            }
        }
        (Plans { plans, sources, named: Vec::new() }, problems)
    }

    /// The plans there are, for the pre-flight and `view:setup` (it has no page of its own): `(id, price, a week or a month)`.
    pub fn list(&self) -> Vec<(String, Option<f64>, &'static str)> {
        self.plans.iter().map(|p| (p.id.clone(), p.price, if p.period_ms < 14.0 * 86_400_000.0 { "week" } else { "month" })).collect()
    }

    /// The connectors whose stream shows windows (`/usage` has a row for each).
    pub fn windowed(&self) -> Vec<&str> {
        self.sources.iter().filter(|s| s.windows.is_some()).map(|s| s.connector.as_str()).collect()
    }

    /// How long the provider of `connector` keeps a call's prefix cached (its `cost_source` row's `cache`); none: not said.
    pub fn cache_ms(&self, connector: &str) -> Option<u64> {
        self.source(connector)?.cache_ms
    }

    /// The price ratios of the provider of `connector` (its `cost_source` row's `ratio`); none: not said.
    pub fn ratios(&self, connector: &str) -> Option<Ratios> {
        self.source(connector)?.ratios
    }

    /// How `connector` says its windows; none: no row.
    pub fn source(&self, connector: &str) -> Option<&Source> {
        self.sources.iter().find(|s| s.connector == connector)
    }

    /// The plan a request of `connector`, run as `account`, is paid by: a plan row for that account, else the plan the account's row
    /// names (`plan = "pro"`), else the connector's own plan (the row of its name), else a plan of it that no account names, else any plan of it.
    pub fn of(&self, connector: &str, account: Option<&str>) -> Option<&Plan> {
        let mine = || self.plans.iter().filter(|p| p.connector == connector);
        let named = |p: &Plan| self.named.iter().any(|(_, id)| *id == p.id);
        mine()
            .find(|p| p.account.is_some() && p.account.as_deref() == account)
            .or_else(|| account.and_then(|a| mine().find(|p| self.named.iter().any(|(acc, id)| acc == a && *id == p.id))))
            .or_else(|| mine().find(|p| p.account.is_none() && p.id == connector && !named(p)))
            .or_else(|| mine().find(|p| p.account.is_none() && !named(p)))
            .or_else(|| mine().find(|p| p.account.is_none()))
    }

    /// The accounts whose plan is `plan` (`plan = "pro"` on their rows): where a chore may run while another window is busy.
    pub fn accounts_on(&self, plan: &str) -> Vec<String> {
        self.named.iter().filter(|(_, id)| id == plan).map(|(a, _)| a.clone()).collect()
    }

    /// The plans of the Table and the plan each `account` row names (`plan = "max5"`): what an edit reads again.
    pub fn with_accounts(tables: &Tables) -> (Plans, Vec<Problem>) {
        let (mut plans, mut problems) = Plans::load(tables);
        let named: Vec<(String, String)> = tables
            .ids("account")
            .into_iter()
            .filter_map(|id| match tables.raw("account", id).and_then(|r| r.fields.get("plan").cloned()) {
                Some(toml::Value::String(p)) => Some((id.to_string(), p)),
                _ => None,
            })
            .collect();
        problems.extend(plans.name(named));
        (plans, problems)
    }

    /// The plans accounts name, `(account, plan id)`: one that no plan row has is a problem, and that account keeps the connector's plan.
    pub fn name(&mut self, accounts: impl IntoIterator<Item = (String, String)>) -> Vec<Problem> {
        let mut problems = Vec::new();
        for (account, plan) in accounts {
            if self.plans.iter().any(|p| p.id == plan) {
                self.named.push((account, plan));
            } else {
                problems.push(Problem::refused(format!("account {account}"), format!("`plan = {plan}` names no plan row; the connector's own plan stands")));
            }
        }
        problems
    }
}

impl Plan {
    fn from_row(r: &PlanRow) -> Result<Plan, String> {
        let price = match r.price.as_deref().map(Expr::parse) {
            None => None,
            Some(Ok(Expr::Lit(Value::Usd(m)))) if m > 0 => Some(m as f64 / 1e6),
            Some(_) => return Err("`price` is money more than zero, such as \"$24\"".into()),
        };
        let mut windows = Vec::new();
        for w in &r.windows {
            match w.split_once('=').and_then(|(name, len)| Some((name.trim(), Expr::dur(len.trim())?))) {
                Some((name, ms)) if !name.is_empty() && ms > 0 => windows.push((name.to_string(), ms)),
                _ => return Err(format!("a window is `name=length`, such as `seven_day=7d`, not `{w}`")),
            }
        }
        if windows.is_empty() {
            return Err("a plan names at least one window".into());
        }
        let period_ms = if r.period.as_deref() == Some("week") { WEEK_MS } else { MONTH_MS };
        let assume = match r.assume.as_deref().map(Expr::parse) {
            None => None,
            Some(Ok(Expr::Lit(Value::Usd(m)))) if m > 0 => Some(m as f64 / 1e6),
            Some(_) => return Err("`assume` is money more than zero, such as \"$1000\"".into()),
        };
        Ok(Plan { id: r.id.clone(), connector: r.connector.clone(), account: r.account.clone(), price, assume, period_ms, windows })
    }
}

/// Where a window stands as the events of a doer say it, and the list cost spent since it last moved.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Track {
    seen: bool,
    util: f64,
    resets: i64,
    /// `util` is where the window moved to, so the cost since is that of whole steps: only then does a move make a pair. The first
    /// reading, or one after a reset, is somewhere inside a step.
    anchored: bool,
    cost: f64,
    /// List dollars of a full window, one per pair, the latest last.
    pairs: VecDeque<f64>,
}

/// What a full window of a plan is worth in list dollars, from the Log: per doer, account and window, the cost
/// spent between two moves of the window over the size of the move. Passive: it reads what runs already wrote (`run.window` and
/// `run.metrics`) and spends nothing.
#[derive(Default)]
pub struct PlanRate(BTreeMap<String, Track>);

fn key(doer: &str, account: &str, window: &str) -> String {
    format!("{doer}|{account}|{window}")
}

impl PlanRate {
    fn said(t: &Track) -> Value {
        let mut sorted: Vec<f64> = t.pairs.iter().copied().collect();
        sorted.sort_by(f64::total_cmp);
        let at = |i: usize| sorted.get(i).copied().unwrap_or(0.0);
        let mid = sorted.len() / 2;
        let median = if sorted.len() % 2 == 1 { at(mid) } else { (at(mid.saturating_sub(1)) + at(mid)) / 2.0 };
        let num = |n: f64| Value::num(n);
        Value::Map(BTreeMap::from([("anchored".into(), Value::Bool(t.anchored)), ("util".into(), num(t.util)), ("resets".into(), num(t.resets as f64)), ("n".into(), num(sorted.len() as f64)), ("median".into(), num(median)), ("lo".into(), num(at(0))), ("hi".into(), num(at(sorted.len().saturating_sub(1))))]))
    }
}

impl Fold for PlanRate {
    fn id(&self) -> &str {
        "plan_rate"
    }

    fn version(&self) -> u64 {
        1
    }

    fn wants(&self, kind: &str) -> bool {
        kind == "run.window" || kind == "run.metrics"
    }

    fn apply(&mut self, ev: &Ev) {
        let Actor::Doer(doer) = &ev.by else { return };
        let d = &ev.data;
        let account = d.get("account").and_then(Json::as_str).unwrap_or("");
        if ev.kind == "run.metrics" {
            let (usd, prefix) = (d.get("usd").and_then(Json::as_f64).unwrap_or(0.0), key(doer, account, ""));
            self.0.range_mut(prefix.clone()..).take_while(|(k, _)| k.starts_with(&prefix)).for_each(|(_, t)| t.cost += usd);
            return;
        }
        let (Some(window), Some(util), Some(resets)) = (d.get("window").and_then(Json::as_str), d.get("utilization").and_then(Json::as_f64), d.get("resets_at").and_then(Json::as_i64)) else { return };
        let t = self.0.entry(key(doer, account, window)).or_default();
        if t.seen && resets == t.resets && util + EPS >= t.util {
            if util > t.util + EPS {
                if t.anchored && t.cost > 0.0 {
                    t.pairs.push_back(t.cost / (util - t.util));
                    while t.pairs.len() > KEEP {
                        t.pairs.pop_front();
                    }
                }
                (t.util, t.anchored, t.cost) = (util, true, 0.0);
            }
        } else {
            (t.seen, t.util, t.resets, t.anchored, t.cost) = (true, util, resets, false, 0.0);
        }
    }

    fn read(&self, path: &[Key], _now_ms: u64) -> Value {
        match path.first() {
            Some(Key::Name(k)) => self.0.get(k).map(Self::said).unwrap_or_default(),
            _ => Value::Map(self.0.iter().map(|(k, t)| (k.clone(), Self::said(t))).collect()),
        }
    }

    fn save(&self) -> Json {
        json!(self.0)
    }

    fn load(&mut self, state: Json) -> Result<(), String> {
        self.0 = serde_json::from_value(state).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn reset(&mut self) {
        self.0.clear();
    }
}

/// What the fold says of a window of `doer` run as `account`: how many pairs it has and their median, in list dollars of a full window.
pub fn rate(folds: &Folds, doer: &str, account: &str, window: &str, now_ms: u64) -> Option<(usize, f64)> {
    let Value::Map(m) = folds.read("plan_rate", &[Key::Name(key(doer, account, window))], now_ms) else { return None };
    match (m.get("n"), m.get("median")) {
        (Some(Value::Num(n)), Some(Value::Num(median))) => Some((*n as usize, *median)),
        _ => None,
    }
}

/// Whether a window of `doer` run as `account` has moved since it was last read or reset: the next move makes a pair.
pub fn anchored(folds: &Folds, doer: &str, account: &str, window: &str, now_ms: u64) -> bool {
    let Value::Map(m) = folds.read("plan_rate", &[Key::Name(key(doer, account, window))], now_ms) else { return false };
    matches!(m.get("anchored"), Some(Value::Bool(true)))
}

/// Where a window of a doer stands, as the last `run.window` said it.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowNow {
    pub doer: String,
    pub account: String,
    /// The provider's name for it: `five_hour`, `seven_day`.
    pub window: String,
    /// How much of it is used, from 0 to 1.
    pub used: f64,
    /// When it resets, seconds since 1970.
    pub resets: i64,
}

impl WindowNow {
    /// The window as the operator reads it: `5h`, `7d`; a name the provider chose is left as it is.
    pub fn short(&self) -> String {
        match self.window.as_str() {
            "five_hour" => "5h".into(),
            "seven_day" => "7d".into(),
            other => other.replace('_', " "),
        }
    }
}

/// Every window the fold has a reading of that has not reset yet at `now_ms`, in the order of its key. The reading is where the window last
/// moved (within a step of 1%), so a window that is read but not moving is as it was.
pub fn windows(folds: &Folds, now_ms: u64) -> Vec<WindowNow> {
    let Value::Map(all) = folds.read("plan_rate", &[], now_ms) else { return Vec::new() };
    all.iter()
        .filter_map(|(k, v)| {
            let (Value::Map(m), mut parts) = (v, k.splitn(3, '|')) else { return None };
            let (doer, account, window) = (parts.next()?, parts.next()?, parts.next()?);
            let (Some(Value::Num(used)), Some(Value::Num(resets))) = (m.get("util"), m.get("resets")) else { return None };
            let resets = *resets as i64;
            (resets.saturating_mul(1000) > now_ms as i64).then(|| WindowNow { doer: doer.into(), account: account.into(), window: window.into(), used: *used, resets })
        })
        .collect()
}

/// What the election weighs for a doer on a plan: the plan cost of a request, measured or assumed as a task's is (the plan of the
/// doer for no account in particular, since an election comes before any account is named). Off with `cost.plan.on`.
pub struct PlanPricing {
    pub plans: LivePlans,
    pub folds: Folds,
    pub settings: crate::setting::Live,
}

impl crate::front::hosts::PlanPrice for PlanPricing {
    fn price(&self, doer: &str, usd: f64, now_ms: u64) -> Option<f64> {
        let plans = self.plans.now();
        if !self.settings.flag("cost.plan.on", true) || plans.source(doer).is_some_and(|s| s.windows.is_none()) {
            return None;
        }
        let min_n = self.settings.num("cost.plan.min_n", 5.0) as usize;
        Some(plans.of(doer, None)?.cost(usd, min_n, |w| rate(&self.folds, doer, "", w, now_ms))?.usd)
    }
}
