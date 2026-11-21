//! Rank: order what is offered by the standing numbers, with a reason for each place. It is a pure
//! function over [`Candidate`]s and a [`Spec`] (the kind `rank`); [`RankAnswerer`] puts it in the answerer chain.
//!
//! - `score` = Σ weight · column. A weight below zero marks a column where less is better (cost). **Unpriced is not free:** a
//!   candidate that lacks a column the spec weighs (or whose value is NaN or infinite) takes the *worst finite value* any
//!   remaining candidate has for it; if none has one, the column counts for nobody.
//! - Order: score (to a billionth), then `recommended`, then id — deterministic whatever the input order.
//! - `exclude` removes candidates. `pins` then hard-filter what is left: if at least one pin is offered only the pins stay
//!   (an excluded pin counts as not offered); a pin that is not offered is ignored with a note.
//! - `cost_low` applies only when `election` is true (a thread elects its model once and keeps it): the `cost` column
//!   then weighs at least -1.
//! - Nothing left to order is `Err(Empty)`: the caller skips with a notice, or asks the operator at gate impact.

use crate::answer::{AskPoint, Answerer, Kind as AnswerKind, Offer, Question};
use crate::ask::{Answer, AskShape};
use crate::table::{Kind, Problem, Tables};
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};

/// One thing that may be picked. `cols` are the standing numbers the asker knows of it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Candidate {
    pub id: String,
    pub cols: BTreeMap<String, f64>,
    pub recommended: bool,
}

/// How to order (the kind `rank`).
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct Spec {
    #[serde(default)]
    pub weights: BTreeMap<String, f64>,
    #[serde(default)]
    pub pins: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub cost_low: bool,
    /// The least lead of the head over the second that makes it a clear pick.
    #[serde(default)]
    pub min_gap: f64,
}

impl Kind for Spec {
    const NAME: &'static str = "rank";
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ranked {
    pub id: String,
    pub score: f64,
    /// A short reason, e.g. `weights: reliability 0.9 · cost low`.
    pub why: String,
}

/// The order, best first, and what was ignored on the way.
#[derive(Debug, Clone, PartialEq)]
pub struct Ranking {
    pub order: Vec<Ranked>,
    pub notes: Vec<String>,
}

impl Ranking {
    /// Whether the head leads the second by at least `min_gap` (and by anything at all): a lone candidate always does.
    pub fn clear(&self, min_gap: f64) -> bool {
        match self.order.as_slice() {
            [_] => true,
            [a, b, ..] => grid(a.score) - grid(b.score) >= grid(min_gap.max(0.0)).max(1e-9),
            [] => false,
        }
    }
}

/// Nothing is left to order, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Empty {
    pub why: String,
}

fn grid(x: f64) -> f64 {
    (x * 1e9).round() / 1e9 + 0.0
}

fn num(x: f64) -> String {
    format!("{x:.3}").trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Order `candidates` by `spec`. `election`: a thread is choosing its model now (see the module doc).
pub fn rank(candidates: &[Candidate], spec: &Spec, election: bool) -> Result<Ranking, Empty> {
    let mut notes = Vec::new();
    let excluded: HashSet<&str> = spec.exclude.iter().map(String::as_str).collect();
    let (mut seen, mut skipped) = (HashSet::new(), 0);
    let mut pool: Vec<&Candidate> = Vec::new();
    for c in candidates {
        if c.id.is_empty() || !seen.insert(c.id.as_str()) {
            skipped += 1;
        } else if !excluded.contains(c.id.as_str()) {
            pool.push(c);
        }
    }
    if skipped > 0 {
        notes.push(format!("{skipped} candidate(s) offered twice or without an id — ignored"));
    }
    if pool.is_empty() {
        let why = if candidates.is_empty() { "nothing is offered" } else { "everything offered is excluded" };
        return Err(Empty { why: why.into() });
    }
    let pins: HashSet<&str> = spec.pins.iter().map(String::as_str).collect();
    if pool.iter().any(|c| pins.contains(c.id.as_str())) {
        pool.retain(|c| pins.contains(c.id.as_str()));
        let kept: HashSet<&str> = pool.iter().map(|c| c.id.as_str()).collect();
        let gone = pins.iter().filter(|p| !kept.contains(**p)).count();
        if gone > 0 {
            notes.push(format!("{gone} pin(s) not offered — ignored"));
        }
    } else if !pins.is_empty() {
        notes.push("no pin is offered — all candidates are ranked".into());
    }
    let low = election && spec.cost_low;
    let mut weights: BTreeMap<&str, f64> =
        spec.weights.iter().filter(|(_, w)| w.is_finite() && **w != 0.0).map(|(c, w)| (c.as_str(), *w)).collect();
    if low {
        let w = weights.get("cost").map_or(1.0, |w| w.abs().max(1.0));
        weights.insert("cost", -w);
    }
    // the worst value any remaining candidate has, per weighted column
    let worst: BTreeMap<&str, f64> = weights
        .iter()
        .filter_map(|(col, w)| {
            let known = pool.iter().filter_map(|c| c.cols.get(*col).copied().filter(|v| v.is_finite()));
            known.min_by(|a, b| (a * w).total_cmp(&(b * w))).map(|v| (*col, v))
        })
        .collect();
    let basis: Vec<String> =
        weights.iter().map(|(c, w)| if low && *c == "cost" { "cost low".to_string() } else { format!("{c} {}", num(*w)) }).collect();
    let mut order: Vec<(f64, bool, Ranked)> = pool
        .iter()
        .map(|c| {
            let (mut score, mut why) = (0.0, vec![if basis.is_empty() { "no weights".to_string() } else { format!("weights: {}", basis.join(" · ")) }]);
            for (col, w) in &weights {
                let own = c.cols.get(*col).copied().filter(|v| v.is_finite());
                if own.is_none() && worst.contains_key(col) {
                    why.push(format!("{col} unknown, worst taken"));
                }
                let term = w * own.or(worst.get(col).copied()).unwrap_or(0.0);
                score += if term.is_finite() { term } else { 0.0 };
            }
            if pins.contains(c.id.as_str()) {
                why.insert(0, "pinned".into());
            }
            if c.recommended {
                why.push("recommended".into());
            }
            let score = score.clamp(-f64::MAX, f64::MAX);
            (grid(score), c.recommended, Ranked { id: c.id.clone(), score, why: why.join(" · ") })
        })
        .collect();
    order.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)).then_with(|| a.2.id.cmp(&b.2.id)));
    Ok(Ranking { order: order.into_iter().map(|o| o.2).collect(), notes })
}

/// Every `rank` row of the Table as a [`Spec`], by id; a row that cannot be read is a problem and is left out. A spec that
/// pins and excludes the same id is read, with a warning (the exclusion wins).
pub fn specs(tables: &Tables) -> (BTreeMap<String, Spec>, Vec<Problem>) {
    let (mut out, mut problems) = (BTreeMap::new(), Vec::new());
    for id in tables.ids("rank") {
        match tables.get::<Spec>(id) {
            Ok(spec) => {
                if spec.pins.iter().any(|p| spec.exclude.contains(p)) {
                    problems.push(Problem::warn(format!("[[rank]] {id}"), "a pinned id is also excluded — the exclusion wins"));
                }
                out.insert(id.to_string(), spec);
            }
            Err(p) => problems.push(p),
        }
    }
    (out, problems)
}

/// Where the standing numbers come from: the columns known of one option of one ask point.
pub trait Columns: Send + Sync {
    fn columns(&self, point: &str, option: &str) -> BTreeMap<String, f64>;

    /// The crew works for quality: the spec `quality` orders the options when there is one.
    fn quality(&self) -> bool {
        false
    }
}

/// No numbers known: Rank falls back to the recommended option, then the smallest id.
pub struct NoColumns;

impl Columns for NoColumns {
    fn columns(&self, _: &str, _: &str) -> BTreeMap<String, f64> {
        BTreeMap::new()
    }
}

/// The answerer `rank`: picks the head of the ranked options of a choice. It uses the spec its point names (`rank` in the
/// point), else the spec `default`, else none. A question that reaches an answerer is always an election (a bound thread
/// asks nothing), so `election` starts true.
pub struct RankAnswerer {
    specs: BTreeMap<String, Spec>,
    by_point: BTreeMap<String, String>,
    columns: Box<dyn Columns>,
    pub election: bool,
}

impl RankAnswerer {
    pub fn new(specs: BTreeMap<String, Spec>, points: &[AskPoint], columns: Box<dyn Columns>) -> RankAnswerer {
        let by_point = points.iter().filter_map(|p| Some((p.id.clone(), p.rank.clone()?))).collect();
        RankAnswerer { specs, by_point, columns, election: true }
    }
}

impl Answerer for RankAnswerer {
    fn id(&self) -> &str {
        "rank"
    }

    fn kind(&self) -> AnswerKind {
        AnswerKind::Rank
    }

    fn offer(&self, q: &Question) -> Offer {
        let AskShape::Choice { options } = &q.shape else {
            return Offer::Pass { why: "rank orders the options of a choice".into() };
        };
        let name = if self.columns.quality() && self.specs.contains_key("quality") { "quality" } else { self.by_point.get(&q.point).map_or("default", String::as_str) };
        let spec = self.specs.get(name).or_else(|| self.specs.get("default")).cloned().unwrap_or_default();
        let cands: Vec<Candidate> = options
            .iter()
            .map(|o| Candidate { id: o.id.clone(), cols: self.columns.columns(&q.point, &o.id), recommended: o.recommended })
            .collect();
        match rank(&cands, &spec, self.election) {
            Err(e) => Offer::Pass { why: e.why },
            Ok(r) => {
                let (conf, tie) = if r.clear(spec.min_gap) { (0.9, "") } else { (0.3, " · no clear lead") };
                Offer::Answer { answer: Answer::Pick { id: r.order[0].id.clone() }, conf, why: format!("{}{tie}", r.order[0].why) }
            }
        }
    }
}
