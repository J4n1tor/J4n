//! The display score: from 10 down by each mark of at least half a chance, by its weight and its
//! chance; held under what the rubrics allow, and under 3 when the project's own check failed. Nothing decides by it. A run that
//! could check nothing has no number, and one that went without its second tier says so beside it.

use crate::scan::Mark;
use j4n_core::table::Kind;
use serde::Deserialize;
use std::collections::BTreeMap;

/// The one entry of kind `qc_score`: what a mark costs.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Weights {
    pub id: String,
    #[serde(default)]
    pub doc: String,
    pub high: f64,
    pub mid: f64,
    pub low: f64,
    /// A mark with less chance than this costs nothing.
    pub min_p: f64,
    /// What a failed project check holds the score under.
    pub failed_cap: f64,
}

impl Kind for Weights {
    const NAME: &'static str = "qc_score";
}

impl Default for Weights {
    fn default() -> Weights {
        Weights { id: "weights".into(), doc: String::new(), high: 2.0, mid: 1.0, low: 0.5, min_p: 0.5, failed_cap: 3.0 }
    }
}

/// `rubrics` holds, for each rubric, its position and its top level.
pub fn score(w: &Weights, marks: &[Mark], rubrics: &BTreeMap<String, (f64, f64)>, check_failed: bool) -> f64 {
    let cost: f64 = marks
        .iter()
        .filter(|m| m.p >= w.min_p)
        .map(|m| {
            m.p * match m.severity {
                crate::pattern::Severity::High => w.high,
                crate::pattern::Severity::Mid => w.mid,
                crate::pattern::Severity::Low => w.low,
            }
        })
        .sum();
    let mut value = 10.0 - cost;
    if !rubrics.is_empty() {
        let mean = rubrics.values().map(|(at, top)| if *top > 0.0 { (at / top).clamp(0.0, 1.0) } else { 1.0 }).sum::<f64>() / rubrics.len() as f64;
        value = value.min(10.0 * mean);
    }
    if check_failed {
        value = value.min(w.failed_cap);
    }
    (value.clamp(0.0, 10.0) * 10.0).round() / 10.0
}
