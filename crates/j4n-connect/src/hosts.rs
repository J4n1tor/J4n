//! Which host serves a model. The rows are kind `model`: a model, the connector that speaks to it, and the
//! hosts it can be had from. The fold [`HostBook`] (`endpoints`) is what the Log says of each host — how fast it answered, what it
//! cached, which limits it hit — and of each thread's cache; [`Election`] is what the front asks, once, when a thread binds.
//!
//! What it decides:
//!
//! - **the switch** `route.hosts` (off): when off, every model is served at its default endpoint, and the gateway is told a
//!   session id so it keeps the session on one host and its cache with it;
//! - **the filters**: a host whose zone the egress Gate does not allow for project data, whose precision `route.quant`
//!   refuses, whose uptime is under `route.uptime_floor`, or that is cooling, is never offered;
//! - **cooling**: `route.cooling` limits or server errors in five minutes make a host cooling; what a thread bound to it does
//!   is `route.failover`, and a move says what the cold cache costs;
//! - **the pick** among those that remain is the cheapest by `cost_est + λ·time_est` for the role's own mix of tokens:
//!   the cost of that mix at the host's prices, plus what the minutes it takes are worth — λ, which depends on who waits and is a
//!   pinned `route.lambda` when the operator set one. Unpriced ranks last; a speed J4n has not measured yet is the listed one, shown
//!   `~`, and a host with no speed at all counts as the slowest of the others. A host whose own record for the role is clearly under
//!   the best host's is not offered, and one with too little of a record gets tasks that only read first.

use j4n_core::answer::Egress;
use j4n_core::fold::Fold;
use j4n_core::front::hosts::{Hosts, Line, NoPlan, NoRecord, Pick, PlanPrice, Record, Standing, Want};
use j4n_core::gate::Verdict;
use j4n_core::log::Ev;
use j4n_core::schema::DataClass;
use j4n_core::table::{Kind, Problem, Tables};
use j4n_core::value::{Key, Value};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

/// How far back errors count toward cooling.
pub const COOLING_WINDOW_MS: u64 = 5 * 60 * 1000;
/// How long a host's cache is assumed to last when its row does not say.
pub const TTL_MS: u64 = 5 * 60 * 1000;
const ERRORS_KEPT: usize = 32;
const THREADS_KEPT: usize = 256;

/// One host in a `model` row. Prices are dollars per million tokens.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub host: String,
    #[serde(default)]
    pub quant: Option<String>,
    #[serde(default)]
    pub zone: Option<String>,
    #[serde(default)]
    pub tier: Option<String>,
    #[serde(default)]
    pub uptime: Option<f64>,
    #[serde(default)]
    pub p_in: Option<f64>,
    #[serde(default)]
    pub p_out: Option<f64>,
    #[serde(default)]
    pub p_cache_read: Option<f64>,
    #[serde(default)]
    pub p_cache_write: Option<f64>,
    #[serde(default)]
    pub ttl: Option<String>,
    #[serde(default)]
    pub tps: Option<f64>,
    #[serde(default)]
    pub ttft_ms: Option<u64>,
}

impl Endpoint {
    fn ttl_ms(&self) -> u64 {
        self.ttl.as_deref().and_then(j4n_core::expr::Expr::dur).unwrap_or(TTL_MS)
    }
}

/// An entry of kind `model`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRow {
    pub id: String,
    pub doc: Option<String>,
    pub connector: String,
    pub name: Option<String>,
    #[serde(default)]
    pub default: bool,
    pub context: Option<u64>,
    #[serde(default)]
    pub endpoints: Vec<Endpoint>,
}

impl Kind for ModelRow {
    const NAME: &'static str = "model";
}

impl ModelRow {
    /// The name the connector sends.
    pub fn sent(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }
}

/// The model rows that loaded.
#[derive(Debug, Clone, Default)]
pub struct Models(Vec<ModelRow>);

impl Models {
    pub fn load(tables: &Tables) -> (Models, Vec<Problem>) {
        let (mut rows, mut problems) = (Vec::new(), Vec::new());
        for id in tables.ids(ModelRow::NAME) {
            match tables.get::<ModelRow>(id) {
                Ok(r) => {
                    let twice = r.endpoints.iter().enumerate().find(|(i, e)| r.endpoints[..*i].iter().any(|p| p.host == e.host));
                    if let Some((_, e)) = twice {
                        problems.push(Problem::refused(format!("model {id}"), format!("the host `{}` is listed twice", e.host)));
                    } else if r.endpoints.iter().any(|e| e.host.is_empty() || e.host.chars().any(char::is_control)) {
                        problems.push(Problem::refused(format!("model {id}"), "a host has a name of one line"));
                    } else {
                        rows.push(r);
                    }
                }
                Err(p) => problems.push(p),
            }
        }
        (Models(rows), problems)
    }

    pub fn rows(&self) -> &[ModelRow] {
        &self.0
    }

    /// The row for `model` (its id or the name it is sent as) of the connector `doer`; with no model named, the connector's default.
    pub fn find(&self, doer: &str, model: Option<&str>) -> Option<&ModelRow> {
        self.0.iter().filter(|r| r.connector == doer).find(|r| match model {
            Some(m) => r.id == m || r.sent() == m,
            None => r.default,
        })
    }

    /// The name of the connector's default model, as it is sent.
    pub fn default_of(&self, doer: &str) -> Option<&str> {
        self.0.iter().find(|r| r.connector == doer && r.default).map(ModelRow::sent)
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct Host {
    served: u64,
    /// Milliseconds to the first word, and tokens per second, as running averages of the calls seen.
    ttft_ms: f64,
    tps: f64,
    errors: VecDeque<u64>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct Warm {
    at_ms: u64,
    cached: u64,
    input: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct Book {
    /// By `model|host`.
    hosts: BTreeMap<String, Host>,
    /// By `thread|host`: what that host last read from the cache for the thread.
    cache: BTreeMap<String, Warm>,
}

fn key(model: &str, host: &str) -> String {
    format!("{model}|{host}")
}

/// The fold `endpoints`. A clone is a second handle on the same state.
#[derive(Clone, Default)]
pub struct HostBook(Arc<Mutex<Book>>);

impl HostBook {
    pub fn new() -> HostBook {
        HostBook::default()
    }

    fn lock(&self) -> MutexGuard<'_, Book> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// How many limits or server errors the host has had in the last five minutes.
    pub fn recent_errors(&self, model: &str, host: &str, now_ms: u64) -> usize {
        self.lock().hosts.get(&key(model, host)).map_or(0, |h| h.errors.iter().filter(|at| now_ms.saturating_sub(**at) < COOLING_WINDOW_MS).count())
    }

    /// What the host last cached for the thread, if it still would hold it at `now_ms` (its `ttl_ms`).
    pub fn warm(&self, thread: &str, host: &str, ttl_ms: u64, now_ms: u64) -> Option<u64> {
        self.lock().cache.get(&key(thread, host)).filter(|w| w.cached > 0 && now_ms.saturating_sub(w.at_ms) < ttl_ms).map(|w| w.cached)
    }

    /// How fast J4n has found the host to be: milliseconds to the first word, and tokens a second; `None` before it served a call.
    pub fn speed(&self, model: &str, host: &str) -> Option<(f64, f64)> {
        self.lock().hosts.get(&key(model, host)).filter(|h| h.served > 0 && h.tps > 0.0).map(|h| (h.ttft_ms, h.tps))
    }

    pub fn served(&self, model: &str, host: &str) -> u64 {
        self.lock().hosts.get(&key(model, host)).map_or(0, |h| h.served)
    }
}

impl Fold for HostBook {
    fn id(&self) -> &str {
        "endpoints"
    }

    fn version(&self) -> u64 {
        1
    }

    fn wants(&self, kind: &str) -> bool {
        matches!(kind, "run.served" | "run.error")
    }

    fn apply(&mut self, ev: &Ev) {
        let text = |k: &str| ev.data.get(k).and_then(Json::as_str).filter(|s| !s.is_empty());
        let num = |k: &str| ev.data.get(k).and_then(Json::as_u64).unwrap_or(0);
        let (Some(model), Some(host)) = (text("model"), text(if ev.kind == "run.served" { "host" } else { "endpoint" })) else { return };
        let mut book = self.lock();
        let h = book.hosts.entry(key(model, host)).or_default();
        if ev.kind == "run.error" {
            // a limit or a server that failed counts; a model that was wrong, a key that was refused, does not
            if matches!(text("class"), Some("rate-limit" | "transient")) {
                h.errors.push_back(ev.at_ms);
                while h.errors.len() > ERRORS_KEPT {
                    h.errors.pop_front();
                }
            }
            return;
        }
        let n = h.served as f64;
        let mean = |old: f64, new: f64| (old * n + new) / (n + 1.0);
        let ttft = ev.data.get("ttft_ms").and_then(Json::as_f64);
        if let Some(t) = ttft {
            h.ttft_ms = mean(h.ttft_ms, t);
        }
        let (out, ms) = (num("output") as f64, num("ms") as f64);
        let ttft = ttft.unwrap_or(0.0);
        if out > 0.0 && ms > ttft {
            h.tps = mean(h.tps, out / ((ms - ttft) / 1000.0));
        }
        h.served += 1;
        if let Some(thread) = ev.thread.as_deref() {
            let w = key(thread, host);
            if book.cache.len() >= THREADS_KEPT
                && !book.cache.contains_key(&w)
                && let Some(oldest) = book.cache.iter().min_by_key(|(_, w)| w.at_ms).map(|(k, _)| k.clone())
            {
                book.cache.remove(&oldest);
            }
            book.cache.insert(w, Warm { at_ms: ev.at_ms, cached: num("cached"), input: num("input") });
        }
    }

    fn read(&self, path: &[Key], now_ms: u64) -> Value {
        let book = self.lock();
        let hosts: BTreeMap<String, Value> = book
            .hosts
            .iter()
            .map(|(k, h)| {
                let recent = h.errors.iter().filter(|at| now_ms.saturating_sub(**at) < COOLING_WINDOW_MS).count();
                let m = BTreeMap::from([
                    ("served".to_string(), Value::num(h.served as f64)),
                    ("ttft_ms".to_string(), Value::num(h.ttft_ms)),
                    ("tps".to_string(), Value::num(h.tps)),
                    ("errors".to_string(), Value::num(recent as f64)),
                ]);
                (k.clone(), Value::Map(m))
            })
            .collect();
        let cache: BTreeMap<String, Value> = book
            .cache
            .iter()
            .map(|(k, w)| {
                let m = BTreeMap::from([
                    ("cached".to_string(), Value::num(w.cached as f64)),
                    ("input".to_string(), Value::num(w.input as f64)),
                    ("at_ms".to_string(), Value::num(w.at_ms as f64)),
                ]);
                (k.clone(), Value::Map(m))
            })
            .collect();
        Value::Map(BTreeMap::from([("hosts".to_string(), Value::Map(hosts)), ("cache".to_string(), Value::Map(cache))])).at(path)
    }

    fn save(&self) -> Json {
        serde_json::to_value(&*self.lock()).unwrap_or_default()
    }

    fn load(&mut self, state: Json) -> Result<(), String> {
        *self.lock() = serde_json::from_value(state).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn reset(&mut self) {
        *self.lock() = Book::default();
    }
}

/// What the settings say about hosts.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub hosts: bool,
    pub cooling: usize,
    pub quant: String,
    pub uptime_floor: f64,
    /// A time price pinned, in dollars a minute; else the rule below works one out for each election.
    pub lambda: Option<f64>,
    /// What a minute is worth when nobody waits and nothing hangs on the result, and how much more when someone does.
    pub lambda_base: f64,
    pub lambda_waiting: f64,
    pub lambda_blocked: f64,
    pub lambda_tight: f64,
    /// Tasks a host needs to have run for a role before its record counts, and how far under the best host's rate is "clearly".
    pub min_n: u32,
    pub floor_gap: f64,
}

impl Default for Route {
    fn default() -> Route {
        Route { hosts: false, cooling: 3, quant: "any".into(), uptime_floor: 95.0, lambda: None, lambda_base: 0.003, lambda_waiting: 3.0, lambda_blocked: 2.0, lambda_tight: 2.0, min_n: 5, floor_gap: 0.2 }
    }
}

/// The tokens a role usually needs when the Log has not said yet: `(in, out)`. Writing code is output-heavy; reviewing is not.
fn default_mix(role: &str) -> (f64, f64) {
    match role {
        "plan" => (20_000.0, 2_500.0),
        "review" => (23_000.0, 400.0),
        "validate" => (15_000.0, 400.0),
        _ => (40_000.0, 8_000.0),
    }
}

impl Route {
    /// The route the settings say (`route.*`): the wiring at the start, and an edit while J4n runs.
    pub fn of(settings: &j4n_core::setting::Settings) -> Route {
        Route {
            hosts: matches!(settings.get("route.hosts"), Some(Value::Bool(true))),
            cooling: settings.num("route.cooling", 3.0) as usize,
            quant: settings.word("route.quant", "any").to_string(),
            uptime_floor: settings.num("route.uptime_floor", 95.0),
            lambda: match settings.get("route.lambda") {
                Some(Value::Num(n)) if *n >= 0.0 => Some(*n),
                _ => None,
            },
            lambda_base: settings.num("route.lambda_base", 0.003),
            lambda_waiting: settings.num("route.lambda_waiting", 3.0),
            lambda_blocked: settings.num("route.lambda_blocked", 2.0),
            lambda_tight: settings.num("route.lambda_tight", 2.0),
            min_n: settings.num("route.min_n", 5.0) as u32,
            floor_gap: settings.num("route.floor_gap", 0.2),
        }
    }

    /// Whether the precision is one the setting forbids (`no-fp4,no-int4`).
    fn refuses(&self, quant: Option<&str>) -> bool {
        quant.is_some_and(|q| self.quant.split(',').any(|w| w.trim().to_ascii_lowercase().strip_prefix("no-").is_some_and(|n| n.eq_ignore_ascii_case(q))))
    }
}

/// The front's view: the model rows, what the Log says of them, the settings, and the egress Gate.
pub struct Election {
    pub models: Models,
    pub book: HostBook,
    pub route: Route,
    pub egress: Option<Arc<dyn Egress>>,
    /// What each host has done for each role.
    pub record: Arc<dyn Record>,
    /// What a request costs the plan of a doer that is on one: the election weighs it for the list cost.
    pub plan: Arc<dyn PlanPrice>,
    /// `route.*` as an edit left them while J4n runs; none: `route` as it was made.
    tuned: Mutex<Option<Route>>,
}

impl Election {
    pub fn new(models: Models, book: HostBook, route: Route, egress: Option<Arc<dyn Egress>>) -> Election {
        Election { models, book, route, egress, record: Arc::new(NoRecord), plan: Arc::new(NoPlan), tuned: Mutex::new(None) }
    }

    /// The route as the settings say now.
    fn route(&self) -> Route {
        let tuned = self.tuned.lock().unwrap_or_else(|e| e.into_inner()).clone();
        tuned.unwrap_or_else(|| self.route.clone())
    }

    /// Why `e` is not offered, if it is not.
    fn refused(&self, row: &ModelRow, e: &Endpoint, now_ms: u64) -> Option<String> {
        if let Some(zone) = e.zone.as_deref() {
            let verdict = self.egress.as_ref().map(|g| g.check(DataClass::Project, zone));
            if !matches!(verdict, None | Some(Verdict::Allow { .. })) {
                return Some(format!("its zone `{zone}` is not allowed for this data"));
            }
        }
        if self.route().refuses(e.quant.as_deref()) {
            return Some(format!("`route.quant` refuses {}", e.quant.as_deref().unwrap_or_default()));
        }
        if let Some(u) = e.uptime.filter(|u| *u < self.route().uptime_floor) {
            return Some(format!("its uptime {u:.1}% is under {:.0}%", self.route().uptime_floor));
        }
        self.cooling_why(row, e, now_ms)
    }

    fn cooling_why(&self, row: &ModelRow, e: &Endpoint, now_ms: u64) -> Option<String> {
        let n = self.book.recent_errors(row.sent(), &e.host, now_ms);
        (n >= self.route().cooling).then(|| format!("{n} limits or errors in the last 5 minutes"))
    }

    /// What a minute of waiting is worth now, in dollars, and why. A pinned time price wins; otherwise the base is raised by
    /// each thing that makes the wait count — and it is never nothing while someone waits.
    fn lambda(&self, want: &Want) -> (f64, String) {
        if let Some(l) = want.lambda.or(self.route().lambda).filter(|l| l.is_finite() && *l >= 0.0) {
            return (l, format!("λ ${l:.4}/min (pinned)"));
        }
        let r = self.route();
        let (mut l, mut why) = (r.lambda_base.max(1e-6), Vec::new());
        for (on, factor, words) in [(want.waiting, r.lambda_waiting, "you are waiting"), (want.blocked, r.lambda_blocked, "a flow waits on it"), (want.tight, r.lambda_tight, "a deadline is near")] {
            if on {
                l *= factor.max(1.0);
                why.push(words);
            }
        }
        let level = if why.is_empty() { "λ low: nobody waits".to_string() } else { format!("λ high: {}", why.join(", ")) };
        (l, format!("{level} (${l:.4}/min)"))
    }

    /// The hosts of the row that hold the filters, as lines: priced, timed and scored for the role's mix of tokens, the best first.
    fn offered(&self, row: &ModelRow, want: &Want, now_ms: u64) -> (Vec<Line>, f64, String) {
        let (min_in, min_out) = self.record.mix(&want.role).map_or_else(|| default_mix(&want.role), |(i, o)| (i as f64, o as f64));
        let (lambda, lambda_why) = self.lambda(want);
        let hosts: Vec<&Endpoint> = row.endpoints.iter().filter(|e| self.refused(row, e, now_ms).is_none()).collect();
        let rate = |e: &Endpoint| {
            let (ok, bad) = self.record.results(row.sent(), &e.host, &want.role);
            (ok + bad >= self.route().min_n).then(|| f64::from(ok) / f64::from(ok + bad))
        };
        let best = hosts.iter().filter_map(|e| rate(e)).fold(None, |a: Option<f64>, r| Some(a.map_or(r, |a| a.max(r))));
        let mut lines: Vec<Line> = hosts
            .iter()
            .map(|e| {
                let standing = match (rate(e), best) {
                    (None, _) => Standing::Unknown,
                    (Some(r), Some(b)) if r < b - self.route().floor_gap => Standing::Poor,
                    _ => Standing::Good,
                };
                let cost = e.p_in.zip(e.p_out).map(|(i, o)| (min_in * i + min_out * o) / 1e6).map(|list| self.plan.price(&row.connector, list, now_ms).unwrap_or(list));
                let (speed, measured) = match self.book.speed(row.sent(), &e.host) {
                    Some((ttft, tps)) => (Some((ttft, tps)), true),
                    None => (e.tps.map(|t| (e.ttft_ms.map_or(0.0, |t| t as f64), t)), false),
                };
                let secs = speed.map(|(ttft, tps)| ttft / 1000.0 + min_out / tps.max(1e-9));
                Line { host: e.host.clone(), cost, secs, measured, standing, score: None }
            })
            .collect();
        // a host with a poor record is not offered, and one with none gets only reading until a good host is not to be had
        let good = lines.iter().any(|l| l.standing == Standing::Good);
        // (the best host of the row is never poor, so something is always left)
        lines.retain(|l| l.standing != Standing::Poor && !(want.mode == j4n_core::task::Mode::Write && good && l.standing == Standing::Unknown));
        // a speed nobody knows is as slow as the slowest that is known: it is never free
        let slowest = lines.iter().filter_map(|l| l.secs).fold(None, |a: Option<f64>, s| Some(a.map_or(s, |a| a.max(s))));
        for l in &mut lines {
            l.score = l.cost.map(|c| c + lambda * l.secs.or(slowest).unwrap_or(0.0) / 60.0);
        }
        // for quality the best record comes first (a host with none after those that have one), and cost and time only order hosts that are level
        let record: std::collections::BTreeMap<String, Option<f64>> = if want.quality {
            lines.iter().map(|l| (l.host.clone(), Some(self.record.outcomes(row.sent(), &l.host, &want.role)).filter(|o| !o.is_empty()).map(|o| j4n_core::crew::quality_of(&o)))).collect()
        } else {
            Default::default()
        };
        lines.sort_by(|a, b| {
            let by_record = if want.quality { record[&b.host].partial_cmp(&record[&a.host]).unwrap_or(std::cmp::Ordering::Equal) } else { std::cmp::Ordering::Equal };
            by_record.then_with(|| match (a.score, b.score) {
                (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            })
        });
        (lines, lambda, lambda_why)
    }
}

impl Hosts for Election {
    fn tune(&self, settings: &j4n_core::setting::Settings) {
        *self.tuned.lock().unwrap_or_else(|e| e.into_inner()) = Some(Route::of(settings));
    }

    fn elect(&self, doer: &str, model: Option<&str>, want: &Want, now_ms: u64) -> Option<Pick> {
        if !self.route().hosts {
            return None;
        }
        let row = self.models.find(doer, model)?;
        let (lines, lambda, lambda_why) = self.offered(row, want, now_ms);
        let first = lines.first()?;
        let how = match first.score {
            _ if want.quality => format!("the best record of {} host(s) for a `{}` task (the crew works for quality; cost and time only order hosts that are level)", lines.len(), want.role),
            Some(s) => format!("the cheapest of {} host(s) by cost + time for a `{}` task: ${s:.4} ({}; {})", lines.len(), want.role, first.speed(), lambda_why),
            None => format!("the only one of {} host(s) with a price known ({lambda_why})", lines.len()),
        };
        Some(Pick { host: first.host.clone(), model: row.sent().to_string(), why: how, alts: lines.iter().skip(1).map(|l| l.host.clone()).collect(), lines: lines.clone(), lambda })
    }

    fn cooling(&self, doer: &str, model: Option<&str>, host: &str, now_ms: u64) -> Option<String> {
        let row = self.models.find(doer, model)?;
        let e = row.endpoints.iter().find(|e| e.host == host)?;
        self.cooling_why(row, e, now_ms)
    }

    fn another(&self, doer: &str, model: Option<&str>, thread: &str, from: &str, now_ms: u64) -> Option<(Pick, String)> {
        let row = self.models.find(doer, model)?;
        let (lines, lambda, lambda_why) = self.offered(row, &Want::default(), now_ms);
        let lines: Vec<Line> = lines.into_iter().filter(|l| l.host != from).collect();
        let first = lines.first()?;
        let first_e = row.endpoints.iter().find(|e| e.host == first.host);
        let old = row.endpoints.iter().find(|e| e.host == from);
        let cost = match self.book.warm(thread, from, old.map_or(TTL_MS, Endpoint::ttl_ms), now_ms) {
            Some(cached) => {
                let usd = first_e.and_then(|f| f.p_in).zip(old.and_then(|o| o.p_cache_read)).map(|(fresh, read)| cached as f64 * (fresh - read).max(0.0) / 1e6);
                match usd {
                    Some(usd) => format!("the cache on {from} (~{cached} tokens) is left behind: about ${usd:.4} to read it again"),
                    None => format!("the cache on {from} (~{cached} tokens) is left behind"),
                }
            }
            None => format!("{from} holds no cache for this thread"),
        };
        let pick = Pick { host: first.host.clone(), model: row.sent().to_string(), why: format!("the cheapest host left of {} ({lambda_why})", lines.len()), alts: lines.iter().skip(1).map(|l| l.host.clone()).collect(), lines: lines.clone(), lambda };
        Some((pick, cost))
    }
}
