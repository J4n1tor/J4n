//! The model library and the providers: Library › Models is one row per model J4n has used, is using or can
//! use, whatever way it is had, and Library › Providers one row per provider. Both are built here from what J4n already knows, never from a
//! program or the net: the connectors and their `models`, the lists a doer gave and J4n kept, the `[[model]]` rows, the accounts and their
//! plans, the pins of the settings, and the tasks the Log folded (the model that served and how it went). A choice of J4n's own (the planner's
//! pick, offload, the host) reads [`Library`] rather than a page.
//!
//! One row a model: a name has many forms (`claude-sonnet-5-5`, `sonnet-5.5`, `anthropic/claude-sonnet-5.5`) and they are one, by
//! [`model_name`]; a word with no version (`sonnet`) is the newest of its family that was seen, so `sonnet` and `sonnet 5.5` are one row and a
//! family nothing was seen of stays as the word. The ways to reach a model are a field of its row, not rows.

use super::Front;
use crate::task::{Row as Task, model_name};
use crate::value::Value;
use std::collections::{BTreeMap, BTreeSet};

/// A window this full is a limit: the plan is said to be limited.
const LIMITED: f64 = 0.95;

/// One way a model is had: through a connector, as an account of it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Route {
    pub connector: String,
    /// The name the connector sends or serves (`openai/gpt-oss-120b`), as it was said.
    pub name: String,
    /// The accounts and the plan the route is paid by, in words (`claude-1 max5`).
    pub account: String,
    /// `usable`, `limited`, `no key`, `not signed in`, `not checked` (a program nothing probes), `checking…` (its probe has not answered yet), `not installed`, or `seen` (used, and no connector now says it).
    pub state: String,
    /// Where the route comes from: `declared`, `listed`, `model row`, `pinned`, `used`.
    pub from: String,
}

/// One use of a model, from a task of the Log.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Use {
    pub doer: String,
    pub model: String,
    pub ok: Option<bool>,
    pub at_ms: u64,
    pub wall_ms: u64,
    pub out: u64,
}

/// What the library is built from.
#[derive(Debug, Clone, Default)]
pub struct Src {
    pub connectors: Vec<Value>,
    pub models: Vec<Value>,
    pub plans: Vec<Value>,
    pub accounts: Vec<Value>,
    /// A doer as the pre-flight found it: `(what, tone)`.
    pub doers: BTreeMap<String, (String, String)>,
    /// What a connector's key is now: `set · env`, `not set`; never a value.
    pub keys: BTreeMap<String, String>,
    /// What a doer's list said, as kept; no program is asked here.
    pub kept: BTreeMap<String, Vec<String>>,
    /// A provider's list no connector of it takes (OpenRouter's, in J4n basic): `(provider, models)`, read from the cache.
    pub loose: Vec<(String, Vec<String>)>,
    /// `(doer, model)` the settings pin.
    pub pins: Vec<(String, String)>,
    pub uses: Vec<Use>,
    /// Every model a run was ever served by, over the whole Log and not only the Book's last tasks: `(times, last at_ms)` by model as the event said it.
    pub served: BTreeMap<String, (u64, u64)>,
    /// The doers a window of is full.
    pub limited: BTreeSet<String>,
    pub now_ms: u64,
}

/// One model of the library.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Entry {
    pub id: String,
    pub aliases: Vec<String>,
    pub routes: Vec<Route>,
    pub context: Option<u64>,
    /// Per million tokens, dollars: the cheapest `(in, out)` a `[[model]]` row says.
    pub price: Option<(f64, f64)>,
    pub seen: usize,
    pub last_ms: Option<u64>,
    pub done: usize,
    pub ok: usize,
    pub tps: Option<f64>,
}

fn text(m: &BTreeMap<String, Value>, k: &str) -> String {
    match m.get(k) {
        Some(Value::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

fn words(m: &BTreeMap<String, Value>, k: &str) -> Vec<String> {
    match m.get(k) {
        Some(Value::List(l)) => l.iter().filter_map(|v| if let Value::Str(s) = v { Some(s.clone()) } else { None }).collect(),
        _ => Vec::new(),
    }
}

fn number(m: &BTreeMap<String, Value>, k: &str) -> Option<f64> {
    match m.get(k) {
        Some(Value::Num(n)) => Some(*n),
        _ => None,
    }
}

fn maps(rows: &[Value]) -> impl Iterator<Item = &BTreeMap<String, Value>> {
    rows.iter().filter_map(|v| if let Value::Map(m) = v { Some(m) } else { None })
}

/// The key a model is held under: its short name without a `[1m]` note of a larger window.
pub fn canon(raw: &str) -> String {
    let name = model_name(raw);
    match name.find('[') {
        Some(at) => name[..at].trim().to_string(),
        None => name,
    }
}

/// The version of a short name (`sonnet 5.5` is `[5, 5]`): the numbers after its first word.
fn version(name: &str) -> Vec<u32> {
    name.split(|c: char| !c.is_ascii_digit()).filter_map(|n| n.parse().ok()).collect()
}

/// A word with no digit and no blank: a family (`sonnet`), which stands for its newest.
fn is_family(name: &str) -> bool {
    !name.is_empty() && !name.contains(' ') && !name.bytes().any(|b| b.is_ascii_digit() || b == b'-' || b == b'.')
}

fn rank(state: &str) -> u8 {
    match state {
        "usable" => 0,
        "limited" => 1,
        "no key" => 2,
        "not signed in" => 3,
        "not checked" | crate::doctor::CHECKING => 4,
        "not installed" => 5,
        _ => 6,
    }
}

/// How a connector stands now: signed in or keyed, found or not, limited or not.
fn standing(src: &Src, c: &BTreeMap<String, Value>) -> String {
    let id = text(c, "id");
    let keyed = !text(c, "key").is_empty();
    let base = if keyed {
        match src.keys.get(&id) {
            Some(k) if k.starts_with("set") => "usable",
            _ => "no key",
        }
    } else {
        let accounts: Vec<_> = maps(&src.accounts).filter(|a| text(a, "connector") == id).collect();
        let signed = accounts.iter().any(|a| matches!(text(a, "state").as_str(), "signed in" | "yours"));
        match src.doers.get(&id) {
            Some((what, _)) if what.contains("not signed in") && !signed => "not signed in",
            Some((what, _)) if what == "not found" || what == "not declared" => "not installed",
            Some((what, _)) if what == crate::doctor::CHECKING => crate::doctor::CHECKING,
            _ if !accounts.is_empty() && !signed => "not signed in",
            // the pre-flight names the doers it probes; a program it does not name is not claimed to work
            None if accounts.is_empty() => "not checked",
            _ => "usable",
        }
    };
    if base == "usable" && src.limited.contains(&id) { "limited".into() } else { base.into() }
}

/// The accounts of a connector and their plans, in words: `claude-1 max5, claude-2 max5`.
fn account_words(src: &Src, connector: &str) -> String {
    let mut out: Vec<String> = maps(&src.accounts)
        .filter(|a| text(a, "connector") == connector)
        .map(|a| match text(a, "plan") {
            p if p.is_empty() => text(a, "id"),
            p => format!("{} {p}", text(a, "id")),
        })
        .collect();
    if out.is_empty()
        && let Some(p) = maps(&src.plans).find(|p| text(p, "connector") == connector && text(p, "account").is_empty())
    {
        out.push(format!("plan {}", text(p, "id")));
    }
    out.join(", ")
}

/// The library: every model there is a reason to name, once.
pub fn build(src: &Src) -> Vec<Entry> {
    let mut all: BTreeMap<String, Entry> = BTreeMap::new();
    let mut add = |raw: &str, route: Route| {
        let id = canon(raw);
        if id.is_empty() {
            return;
        }
        let e = all.entry(id.clone()).or_insert_with(|| Entry { id, ..Entry::default() });
        match e.routes.iter_mut().find(|r| r.connector == route.connector) {
            // the same connector said it twice (declared and listed, then used): the live route wins over `seen`
            Some(r) if rank(&route.state) < rank(&r.state) || r.from == "used" => *r = route,
            Some(_) => {}
            None => e.routes.push(route),
        }
    };
    let by_id: BTreeMap<String, &BTreeMap<String, Value>> = maps(&src.connectors).map(|c| (text(c, "id"), c)).collect();
    for c in by_id.values().filter(|c| matches!(text(c, "is").as_str(), "agent" | "model" | "decider")) {
        let id = text(c, "id");
        let (state, account) = (standing(src, c), account_words(src, &id));
        let kept = src.kept.get(&id).cloned().unwrap_or_default();
        for (name, from) in words(c, "models").into_iter().map(|m| (m, "declared")).chain(kept.into_iter().map(|m| (m, "listed"))) {
            add(&name.clone(), Route { connector: id.clone(), name, account: account.clone(), state: state.clone(), from: from.into() });
        }
    }
    for (provider, list) in &src.loose {
        for name in list {
            add(name, Route { connector: provider.clone(), name: name.clone(), account: String::new(), state: "no connector".into(), from: "listed".into() });
        }
    }
    for m in maps(&src.models) {
        let (id, connector) = (text(m, "id"), text(m, "connector"));
        let name = Some(text(m, "name")).filter(|n| !n.is_empty()).unwrap_or(id);
        let state = by_id.get(&connector).map_or_else(|| "not installed".to_string(), |c| standing(src, c));
        add(&name, Route { connector, name: name.clone(), account: String::new(), state, from: "model row".into() });
    }
    for (doer, model) in &src.pins {
        let state = by_id.get(doer).map_or_else(|| "not installed".to_string(), |c| standing(src, c));
        add(model, Route { connector: doer.clone(), name: model.clone(), account: account_words(src, doer), state, from: "pinned".into() });
    }
    for u in &src.uses {
        let state = by_id.get(&u.doer).map_or_else(|| "seen".to_string(), |c| standing(src, c));
        add(&u.model, Route { connector: u.doer.clone(), name: u.model.clone(), account: account_words(src, &u.doer), state, from: "used".into() });
    }
    // a model the whole Log served is a row, so a family word of the Book folds into it below
    for raw in src.served.keys() {
        let id = canon(raw);
        if !id.is_empty() {
            all.entry(id.clone()).or_insert_with(|| Entry { id, ..Entry::default() });
        }
    }
    // a family word is the newest of its family that was seen; a family nothing was seen of stays a row of its own
    let families: Vec<String> = all.keys().filter(|k| is_family(k)).cloned().collect();
    for f in families {
        let newest = all.keys().filter(|k| k.starts_with(&format!("{f} ")) && !version(k).is_empty()).max_by_key(|k| version(k)).cloned();
        let Some(into) = newest else { continue };
        let Some(alias) = all.remove(&f) else { continue };
        let e = all.get_mut(&into).expect("the newest was just found");
        e.aliases.push(f);
        for r in alias.routes {
            match e.routes.iter_mut().find(|x| x.connector == r.connector) {
                Some(x) if rank(&r.state) < rank(&x.state) => *x = r,
                Some(_) => {}
                None => e.routes.push(r),
            }
        }
    }
    // what the model rows say of a model, and what the Log says of its use
    for m in maps(&src.models) {
        let raw = Some(text(m, "name")).filter(|n| !n.is_empty()).unwrap_or_else(|| text(m, "id"));
        let Some(e) = all.get_mut(&canon(&raw)) else { continue };
        e.context = e.context.or(number(m, "context").map(|n| n as u64));
        if let Some(Value::List(hosts)) = m.get("endpoints") {
            for h in hosts.iter().filter_map(|h| if let Value::Map(h) = h { Some(h) } else { None }) {
                if let (Some(i), Some(o)) = (number(h, "p_in"), number(h, "p_out"))
                    && e.price.is_none_or(|(pi, po)| i + o < pi + po)
                {
                    e.price = Some((i, o));
                }
            }
        }
    }
    for c in by_id.values() {
        if let Some(ctx) = number(c, "context") {
            let names = words(c, "models");
            if names.len() == 1
                && let Some(e) = all.get_mut(&canon(&names[0]))
            {
                e.context = e.context.or(Some(ctx as u64));
            }
        }
    }
    let mut speeds: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for u in &src.uses {
        let id = canon(&u.model);
        let id = if all.contains_key(&id) { id } else { all.iter().find(|(_, e)| e.aliases.contains(&id)).map(|(k, _)| k.clone()).unwrap_or(id) };
        let Some(e) = all.get_mut(&id) else { continue };
        e.seen += 1;
        e.last_ms = e.last_ms.max(Some(u.at_ms));
        if let Some(ok) = u.ok {
            e.done += 1;
            e.ok += usize::from(ok);
        }
        if u.out > 0 && u.wall_ms >= 1000 {
            speeds.entry(id).or_default().push(u.out as f64 * 1000.0 / u.wall_ms as f64);
        }
    }
    // a model the Book no longer holds a task of is seen all the same, by what the whole Log says of it
    let mut ever: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for (raw, (n, at)) in &src.served {
        let id = canon(raw);
        let id = if all.contains_key(&id) { id } else { all.iter().find(|(_, e)| e.aliases.contains(&id)).map(|(k, _)| k.clone()).unwrap_or(id) };
        let sum = ever.entry(id).or_default();
        *sum = (sum.0 + n, sum.1.max(*at));
    }
    for (id, (n, at)) in ever {
        let Some(e) = all.get_mut(&id) else { continue };
        if e.seen == 0 {
            e.seen = n as usize;
        }
        e.last_ms = e.last_ms.max(Some(at));
    }
    for (id, mut s) in speeds {
        s.sort_by(f64::total_cmp);
        if let Some(e) = all.get_mut(&id) {
            e.tps = Some(s[s.len() / 2]);
        }
    }
    for e in all.values_mut() {
        e.routes.sort_by(|a, b| rank(&a.state).cmp(&rank(&b.state)).then_with(|| a.connector.cmp(&b.connector)));
    }
    all.into_values().collect()
}

impl Entry {
    /// The best state of its routes.
    pub fn state(&self) -> &str {
        self.routes.first().map_or("seen", |r| r.state.as_str())
    }

    fn tone(&self) -> &'static str {
        match self.state() {
            "usable" => "good",
            "limited" | "no key" | "not signed in" => "warn",
            _ => "dim",
        }
    }
}

fn price_words(e: &Entry, plans: &[Value]) -> String {
    // a route on a plan is paid by the plan; else the cheapest host's list price
    let plan = e.routes.iter().filter(|r| r.state == "usable" || r.state == "limited").find_map(|r| {
        let p = maps(plans).find(|p| text(p, "connector") == r.connector && text(p, "account").is_empty())?;
        Some((text(p, "id"), match p.get("price") {
            Some(Value::Usd(u)) => Some(*u as f64 / 1e6),
            _ => None,
        }))
    });
    match (plan, e.price) {
        (Some((id, Some(usd))), _) => format!("plan {id} ${usd:.0}/mo"),
        (Some((id, None)), _) => format!("plan {id}, no price set"),
        (None, Some((i, o))) => format!("${i}/${o} per M"),
        _ => "not priced".into(),
    }
}

/// The rows of `fold:library`.
pub fn rows(src: &Src) -> Vec<Value> {
    build(src)
        .into_iter()
        .map(|e| {
            let t = |s: &str| Value::text(s);
            let via: Vec<String> = e.routes.iter().map(|r| r.connector.clone()).collect();
            let routes: Vec<String> = e.routes.iter().map(|r| format!("{} · {}{}{}", r.connector, r.state, if r.account.is_empty() { String::new() } else { format!(" · {}", r.account) }, if r.name != e.id { format!(" · sends {}", r.name) } else { String::new() })).collect();
            let mut used = vec![match e.seen {
                0 => "not used yet".to_string(),
                n => format!("used {n} time{}{}", if n == 1 { "" } else { "s" }, if e.done > 0 { format!(", {} of {} done well", e.ok, e.done) } else { String::new() }),
            }];
            if let Some(s) = e.tps {
                used.push(format!("{s:.0} tok/s"));
            }
            if !e.aliases.is_empty() {
                used.push(format!("also called {}", e.aliases.join(", ")));
            }
            let used = used.join(" · ");
            let mut m = BTreeMap::from([
                ("id".to_string(), t(&e.id)),
                ("via".into(), t(&via.join(", "))),
                ("routes".into(), t(&routes.join(" · "))),
                ("route".into(), t(via.first().map_or("", String::as_str))),
                ("state".into(), t(e.state())),
                ("tone".into(), t(e.tone())),
                ("price".into(), t(&price_words(&e, &src.plans))),
                ("seen".into(), Value::Num(e.seen as f64)),
                ("used".into(), t(&used)),
                ("pass".into(), t(&if e.done > 0 { format!("{}/{}", e.ok, e.done) } else { String::new() })),
                ("doc".into(), t(&routes.first().cloned().unwrap_or_default())),
            ]);
            if let Some(c) = e.context {
                m.insert("context".into(), Value::Num(c as f64));
            }
            if let Some(at) = e.last_ms {
                m.insert("ago".into(), Value::Dur(src.now_ms.saturating_sub(at)));
            }
            if let Some(s) = e.tps {
                m.insert("tps".into(), Value::Num(s.round()));
            }
            Value::Map(m)
        })
        .collect()
}

/// A provider's host, to match a connector's url against: `openrouter.ai` of `https://openrouter.ai/api/v1`.
fn host(url: &str) -> String {
    url.split("://").last().unwrap_or(url).split(['/', ':']).next().unwrap_or_default().to_ascii_lowercase()
}

/// Whether the provider row `p` is the one connector `c` is of: it names it, or its url is at a host of the provider's.
fn takes(p: &BTreeMap<String, Value>, c: &BTreeMap<String, Value>) -> bool {
    words(p, "connectors").contains(&text(c, "id")) || words(p, "hosts").contains(&host(&text(c, "url"))) && !text(c, "url").is_empty()
}

/// The rows of `fold:providers`: each `provider` row, then a row for every connector no provider row takes. `keys` is what a provider's key is
/// now (never the value), by provider id.
pub fn providers(src: &Src, provider_rows: &[Value], keys: &BTreeMap<String, String>) -> Vec<Value> {
    let library = build(src);
    let models_of = |connectors: &[String]| library.iter().filter(|e| e.routes.iter().any(|r| connectors.contains(&r.connector))).count();
    let take = takes;
    let conns: Vec<&BTreeMap<String, Value>> = maps(&src.connectors).filter(|c| matches!(text(c, "is").as_str(), "agent" | "model" | "decider") && text(c, "dialect") != "test").collect();
    let mut taken: BTreeSet<String> = BTreeSet::new();
    let mut out = Vec::new();
    let row = |id: &str, kind: &str, p: Option<&BTreeMap<String, Value>>, mine: Vec<&BTreeMap<String, Value>>, key: String| {
        let t = |s: &str| Value::text(s);
        let ids: Vec<String> = mine.iter().map(|c| text(c, "id")).collect();
        let accounts: Vec<String> = ids.iter().map(|c| account_words(src, c)).filter(|a| !a.is_empty()).collect();
        let signed = maps(&src.accounts).filter(|a| ids.contains(&text(a, "connector"))).filter(|a| matches!(text(a, "state").as_str(), "signed in" | "yours")).count();
        // the connector that takes the key typed on this row: the first of its connectors that has a key reference
        // a provider with no connector at all that keeps a key takes it as its own (one whose connectors take none, `claude`, has nothing that would read it): the key is kept for the provider and every connector of it reads it
        // so does one whose program is given the provider's kept key (`kept:<provider>` in its `secrets`, `claude` with an API key)
        let gives = |p: &BTreeMap<String, Value>| mine.iter().any(|c| matches!(c.get("secrets"), Some(Value::Map(m)) if m.values().any(|v| matches!(v, Value::Str(r) if *r == format!("kept:{}", text(p, "id"))))));
        let keyid = mine.iter().find(|c| !text(c, "key").is_empty()).map(|c| text(c, "id")).or_else(|| p.filter(|p| (mine.is_empty() || gives(p)) && !text(p, "key").is_empty()).map(|p| text(p, "id"))).unwrap_or_default();
        let best = mine.iter().map(|c| standing(src, c)).min_by_key(|s| rank(s)).unwrap_or_else(|| "not installed".into());
        let state = match (key.as_str(), best.as_str()) {
            (_, "usable" | "limited") => best.clone(),
            ("set · env" | "set · J4n file" | "set · file", _) => "usable".to_string(),
            _ if mine.is_empty() => "no connector".to_string(),
            _ => best.clone(),
        };
        let tone = match state.as_str() {
            "usable" => "good",
            "no connector" | "not installed" | "not checked" | crate::doctor::CHECKING => "dim",
            _ => "warn",
        };
        let doc = p.map(|p| text(p, "doc")).filter(|d| !d.is_empty()).or_else(|| mine.first().map(|c| text(c, "doc"))).unwrap_or_default();
        let url = p.map(|p| text(p, "url")).filter(|u| !u.is_empty()).or_else(|| mine.iter().map(|c| text(c, "url")).find(|u| !u.is_empty())).unwrap_or_default();
        let mut m = BTreeMap::from([
            ("id".to_string(), t(id)),
            ("kind".into(), t(kind)),
            ("url".into(), t(&url)),
            ("key".into(), t(&key)),
            ("state".into(), t(&state)),
            ("tone".into(), t(tone)),
            ("through".into(), t(&ids.join(", "))),
            ("accounts".into(), t(&if accounts.is_empty() { if signed == 0 { String::new() } else { signed.to_string() } } else { accounts.join(", ") })),
            ("models".into(), Value::Num(models_of(&if ids.is_empty() { vec![id.to_string()] } else { ids.clone() }) as f64)),
            ("keyid".into(), t(&keyid)),
            ("doc".into(), t(&doc)),
        ]);
        if let Some(c) = mine.iter().find(|c| text(c, "is") == "model") {
            m.insert("switch".into(), t(&text(c, "id")));
        }
        Value::Map(m)
    };
    for p in maps(provider_rows) {
        let id = text(p, "id");
        let mine: Vec<&BTreeMap<String, Value>> = conns.iter().copied().filter(|c| take(p, c)).collect();
        taken.extend(mine.iter().map(|c| text(c, "id")));
        let key = if mine.iter().any(|c| !text(c, "key").is_empty()) {
            mine.iter().filter(|c| !text(c, "key").is_empty()).map(|c| src.keys.get(&text(c, "id")).cloned().unwrap_or_else(|| "not set".into())).find(|k| k != "not set").unwrap_or_else(|| "not set".into())
        } else {
            keys.get(&id).cloned().unwrap_or_else(|| if text(p, "key").is_empty() { String::new() } else { "not set".into() })
        };
        out.push(row(&id, &text(p, "family"), Some(p), mine, key));
    }
    for c in conns.iter().filter(|c| !taken.contains(&text(c, "id"))) {
        let key = if text(c, "key").is_empty() { String::new() } else { src.keys.get(&text(c, "id")).cloned().unwrap_or_else(|| "not set".into()) };
        let kind = if text(c, "is") == "decider" { "decider" } else if text(c, "dialect") == "openai" || text(c, "dialect") == "anthropic" { "api" } else { "harness" };
        out.push(row(&text(c, "id"), kind, None, vec![c], key));
    }
    out
}

impl Front {
    /// What the library is built from now, read from what the front already holds.
    pub fn library_src(&self) -> Src {
        let screen = self.screen();
        let cfg = self.cfg();
        let now_ms = self.log().clock().now_ms();
        let connectors = screen.tables.rows("connector");
        let state: BTreeMap<String, BTreeMap<String, Value>> = screen.fold_rows("accounts", now_ms).into_iter().filter_map(|r| if let Value::Map(m) = r { Some((text(&m, "id"), m)) } else { None }).collect();
        let accounts = screen
            .tables
            .rows("account")
            .into_iter()
            .map(|r| match r {
                Value::Map(mut m) => {
                    let word = state.get(&text(&m, "id")).map(|s| text(s, "state")).filter(|s| !s.is_empty()).unwrap_or_else(|| if text(&m, "keeps") == "yours" { "yours".into() } else { "needs sign-in".into() });
                    m.insert("state".into(), Value::text(word));
                    Value::Map(m)
                }
                other => other,
            })
            .collect();
        let doers = cfg.setup.as_ref().map(|c| c.report_for(self.main_pick()).rows).unwrap_or_default().into_iter().filter(|r| r.kind == "doer").map(|r| (r.id, (r.what, r.tone.to_string()))).collect();
        let mut keys = BTreeMap::new();
        let mut kept = BTreeMap::new();
        let cache = cfg.listing.as_ref().map(|c| c.read()).unwrap_or_default();
        for c in maps(&connectors) {
            let id = text(c, "id");
            let reference = text(c, "key");
            if !reference.is_empty() {
                keys.insert(id.clone(), self.key_state(&id, &reference));
            }
            let mut list = cfg.models.kept(&id);
            list.extend(cache.get(&id).map(|l| l.models.clone()).unwrap_or_default());
            list.dedup();
            if !list.is_empty() {
                kept.insert(id, list);
            }
        }
        // a provider's list (OpenRouter's) is every model its connectors can send, or, with no connector, the provider's own
        let mut loose = Vec::new();
        let providers = screen.tables.rows("provider");
        for (source, listed) in cache.iter().filter(|(k, _)| k.starts_with("provider:")) {
            let Some(p) = maps(&providers).find(|p| format!("provider:{}", text(p, "id")) == *source) else { continue };
            let mine: Vec<String> = maps(&connectors).filter(|c| matches!(text(c, "is").as_str(), "agent" | "model" | "decider") && takes(p, c)).map(|c| text(c, "id")).collect();
            if mine.is_empty() {
                loose.push((text(p, "id"), listed.models.clone()));
            }
            for id in mine {
                kept.entry(id).or_insert_with(Vec::new).extend(listed.models.clone());
            }
        }
        let served = self.served_models(now_ms);
        let pins = [&cfg.planner, &cfg.reviewer, &cfg.cheap, &cfg.main].into_iter().filter_map(|p| Some((p.doer.clone()?, p.model.clone()?))).collect();
        let uses = self.tasks().book().rows().iter().filter_map(use_of).collect();
        let limited = self.tasks().windows().into_iter().filter(|w| w.used >= LIMITED).map(|w| w.doer).collect();
        Src { connectors, models: screen.tables.rows("model"), plans: screen.tables.rows("plan"), accounts, doers, keys, kept, loose, pins, uses, served, limited, now_ms }
    }

    /// Every model `run.served` ever named, over the whole Log: `(times, last at_ms)` by model (the folds `models_served` and `models_last`).
    fn served_models(&self, now_ms: u64) -> BTreeMap<String, (u64, u64)> {
        let screen = self.screen();
        let num = |r: Value| match r {
            Value::Map(m) => (text(&m, "id"), number(&m, "value").unwrap_or(0.0) as u64),
            _ => (String::new(), 0),
        };
        let last: BTreeMap<String, u64> = screen.fold_rows("models_last", now_ms).into_iter().map(num).collect();
        screen.fold_rows("models_served", now_ms).into_iter().map(num).filter(|(m, _)| !m.is_empty()).map(|(m, n)| { let at = last.get(&m).copied().unwrap_or(0); (m, (n, at)) }).collect()
    }

    /// The rows of `fold:library`.
    pub fn library_rows(&self) -> Vec<Value> {
        rows(&self.library_src())
    }

    /// The rows of `fold:providers`: the key of each is read as the row of its connector reads it (`set · env`, never a value).
    pub fn provider_rows(&self) -> Vec<Value> {
        let src = self.library_src();
        let screen = self.screen();
        let provider_rows = screen.tables.rows("provider");
        let keys: BTreeMap<String, String> = maps(&provider_rows).filter(|p| !text(p, "key").is_empty()).map(|p| (text(p, "id"), self.key_state(&text(p, "id"), &text(p, "key")))).collect();
        let mut out = providers(&src, &provider_rows, &keys);
        for r in &mut out {
            let Value::Map(m) = r else { continue };
            let keyid = text(m, "keyid");
            m.insert("kept".into(), Value::Bool(!keyid.is_empty() && self.key_kept_any(&keyid)));
            if !keyid.is_empty() {
                // a Decider or model is made at the start with no key and is asked once its key reads: the row waits for no start
                m.insert("edit".into(), Value::Map(BTreeMap::from([("kind".to_string(), Value::text("secret")), ("bind".into(), Value::text(format!("key:{keyid}.value")))])));
            }
        }
        out
    }
}

/// A task as a use of a model; none for the human's, or one that named no model.
fn use_of(r: &Task) -> Option<Use> {
    if r.spec.doer == crate::task::HUMAN {
        return None;
    }
    let model = Some(r.model.as_str()).filter(|m| !m.is_empty()).or(r.spec.tune.model.as_deref())?.to_string();
    let ok = match r.state {
        crate::task::State::Done { ok } => Some(ok),
        _ => None,
    };
    let at_ms = r.ended_ms.or(r.started_ms).unwrap_or(r.queued_ms);
    let wall_ms = r.started_ms.zip(r.ended_ms).map_or(0, |(a, b)| b.saturating_sub(a));
    Some(Use { doer: r.spec.doer.clone(), model, ok, at_ms, wall_ms, out: r.tokens_out })
}
