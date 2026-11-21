//! Tiers: which of a doer's models is the cheap one, the standard one and the strong one, found by code from data J4n keeps, never by asking a model
//! First that holds: the list cost of what the doer's models have run (`run.metrics`), when every model that may be picked has run enough to
//! tell; else the price words of the setting `offload.prices` (`haiku=1 sonnet=3 opus=5 …`, the shape of the list prices, the operator's to correct),
//! matched as whole words (`gemini` is not `mini`); else the model has no tier.
//!
//! `cheap` is the lowest price, `standard` the next price up (the cheap one's when no model costs more), `strong` the highest. **A tier never picks a
//! model of the families in [`PINNED_ONLY`] (Opus, Fable): only a pin does**, so with them gone `strong` is for a list's own dearest only and nothing
//! reads it by itself. Models of one price go by the newest. What a token of the window costs (`share`) is not read: no measure of it separates two
//! models of one price.

use super::models::{recency, words};
use super::{Front, Pin};
use std::collections::BTreeMap;

/// The default of `offload.prices`: relative prices per token, a word of a model's name each. The first word a name holds is its price.
pub const PRICES: &str = "haiku=1 sonnet=3 opus=5 fable=8 nano=1 mini=1 flash=1 lite=1 pro=3";
/// The families a tier never picks.
pub const PINNED_ONLY: [&str; 2] = ["opus", "fable"];
/// The tokens a model must have run before its measured cost counts: a few cache-heavy runs say little of its price.
pub const MIN_TOKENS: u64 = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Cheap,
    Standard,
    Strong,
}

impl Tier {
    pub fn word(self) -> &'static str {
        match self {
            Tier::Cheap => "cheapest",
            Tier::Standard => "standard",
            Tier::Strong => "strongest",
        }
    }
}

/// What one model's runs cost at list price, and their tokens.
#[derive(Debug, Clone, Copy, Default)]
pub struct Spent {
    pub usd: f64,
    pub tokens: u64,
}

/// The model a tier names, its price and whether the price was measured.
#[derive(Debug, Clone, PartialEq)]
pub struct Pick {
    pub model: String,
    pub price: f64,
    pub measured: bool,
}

/// `offload.prices`: `word=number` pairs; a pair that does not read is left out.
pub fn parse(text: &str) -> Vec<(String, f64)> {
    text.split_whitespace().filter_map(|p| p.split_once('=')).filter_map(|(w, n)| Some((w.to_lowercase(), n.parse::<f64>().ok().filter(|n| *n > 0.0)?))).collect()
}

/// What a model costs per token relative to the others: the first word of `prices` that its name holds as a whole word.
pub fn price_of(prices: &[(String, f64)], model: &str) -> Option<f64> {
    let name = words(model);
    prices.iter().find(|(w, _)| name.iter().any(|n| n == w)).map(|(_, p)| *p)
}

/// Whether `served` (`claude-haiku-5-5`) is the model `name` (`haiku`) says: it holds every word of it.
fn holds(served: &str, name: &str) -> bool {
    let (s, n) = (words(served), words(name));
    !n.is_empty() && n.iter().all(|w| s.contains(w))
}

/// The model of `list` that `tier` names. `spent` is what each served model has cost.
pub fn pick(list: &[String], prices: &[(String, f64)], spent: &BTreeMap<String, Spent>, tier: Tier) -> Option<Pick> {
    let priced: Vec<(&String, f64)> = list.iter().filter_map(|m| Some((m, price_of(prices, m)?))).collect();
    let free = |m: &str| !words(m).iter().any(|w| PINNED_ONLY.contains(&w.as_str()));
    let order = |a: &(&String, f64), b: &(&String, f64)| a.1.total_cmp(&b.1).then_with(|| recency(b.0).cmp(&recency(a.0))).then_with(|| a.0.cmp(b.0));
    if tier == Tier::Strong {
        return priced.into_iter().max_by(|a, b| a.1.total_cmp(&b.1).then_with(|| recency(a.0).cmp(&recency(b.0)))).map(|(m, p)| Pick { model: m.clone(), price: p, measured: false });
    }
    let mut may: Vec<(&String, f64)> = priced.into_iter().filter(|(m, _)| free(m)).collect();
    // measured only when every model that may be picked has run enough to say; one that has not leaves the price words to all of them
    let cost = |m: &str| {
        let (usd, tokens) = spent.iter().filter(|(s, _)| holds(s, m)).fold((0.0, 0), |a, (_, x)| (a.0 + x.usd, a.1 + x.tokens));
        (tokens >= MIN_TOKENS && usd > 0.0).then(|| usd / tokens as f64)
    };
    let measured: Option<Vec<f64>> = may.iter().map(|(m, _)| cost(m)).collect();
    let measured = measured.filter(|_| may.len() > 1);
    if let Some(m) = &measured {
        may.iter_mut().zip(m).for_each(|(x, c)| x.1 = *c);
    }
    may.sort_by(order);
    let cheap = *may.first()?;
    let chosen = match tier {
        Tier::Standard => may.iter().find(|(_, p)| *p > cheap.1 * 1.000_001).copied().unwrap_or(cheap),
        _ => cheap,
    };
    Some(Pick { model: chosen.0.clone(), price: chosen.1, measured: measured.is_some() })
}

/// The first of `ready` (a doer and its models, in the order to try) that has a model for `tier`.
pub fn find(ready: &[(String, Vec<String>)], prices: &[(String, f64)], spent: &dyn Fn(&str) -> BTreeMap<String, Spent>, tier: Tier) -> Option<(String, Pick)> {
    ready.iter().find_map(|(d, list)| pick(list, prices, &spent(d), tier).map(|p| (d.clone(), p)))
}

impl Front {
    /// What each model that `doer` served has cost, from the tasks the Book holds.
    pub(super) fn spent_by(&self, doer: &str) -> BTreeMap<String, Spent> {
        let mut out: BTreeMap<String, Spent> = BTreeMap::new();
        for r in self.0.tasks.book().rows().iter().filter(|r| r.spec.doer == doer && !r.model.is_empty()) {
            let s = out.entry(r.model.clone()).or_default();
            s.usd += r.listed.max(0) as f64 / 1e6;
            s.tokens += r.tokens_in + r.tokens_out;
        }
        out
    }

    /// The ready doers for a read with the models each has, the thread's doer first.
    pub(super) fn ready_models(&self, prefer: Option<&str>) -> Vec<(String, Vec<String>)> {
        let mut ready: Vec<String> = self.0.who.ready("scout", &[]).into_iter().map(|o| o.id).collect();
        if let Some(i) = prefer.and_then(|p| ready.iter().position(|r| r == p)) {
            let first = ready.remove(i);
            ready.insert(0, first);
        }
        ready.into_iter().map(|d| (self.0.cfg().models.models(&d).unwrap_or_default(), d)).map(|(list, doer)| (doer, list)).collect()
    }

    /// The model `tier` names among the models `doer` has; none when it lists none, or none of its models has a price.
    pub fn tier_model(&self, doer: &str, tier: Tier) -> Option<Pick> {
        let cfg = self.0.cfg();
        let list = cfg.models.models(doer).filter(|l| !l.is_empty())?;
        pick(&list, &cfg.prices, &self.spent_by(doer), tier)
    }

    /// The doer and the model `tier` names: the doers that are signed in for a read, `prefer` first, and the first that has a model for the tier.
    pub fn tier_doer(&self, tier: Tier, prefer: Option<&str>) -> Option<(String, Pick)> {
        find(&self.ready_models(prefer), &self.0.cfg().prices, &|d| self.spent_by(d), tier)
    }

    /// Whether the operator went on without the need `id` (`need.waived`): its tier is not taken by itself.
    pub fn waived(&self, id: &str) -> bool {
        self.0.cfg().waived.iter().any(|w| w == id)
    }

    /// The cheap tier's doer and model for the cheap branches: what `plan.cheap_doer` and `plan.cheap_model` name, and when neither is set the cheapest
    /// model of a ready doer (the thread's own first).
    pub(super) fn cheap_pin(&self, thread: &str) -> Pin {
        let cfg = self.0.cfg();
        if cfg.cheap.doer.is_some() || cfg.cheap.model.is_some() {
            return cfg.cheap.clone();
        }
        self.tier_pin(Tier::Cheap, thread)
    }

    /// The pin a tier names, the thread's own doer first.
    pub(super) fn tier_pin(&self, tier: Tier, thread: &str) -> Pin {
        let own = self.binding(thread).map(|b| b.doer);
        self.tier_doer(tier, own.as_deref()).map(|(doer, p)| Pin { doer: Some(doer), model: Some(p.model) }).unwrap_or_default()
    }
}
