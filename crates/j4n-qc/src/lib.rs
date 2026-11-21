//! Quality control of code (pack `qc`): what an agent leaves behind that the project's own checks do not
//! catch, found in three tiers — patterns over the lines a change adds (no model), Jev asked about each part, and a reviewer — and
//! reported as marks (pattern, place, chance, severity) and a score for display. It informs and never decides: a mark steers a lane
//! only when its pattern has earned it in numbers, with the pattern's own fixed words.
//!
//! - [`pattern`]: the kind `pattern`, with its globs and expressions.
//! - [`change`], [`diff`]: what is checked (a task's work, a path as it stands, a git range) as hunks of lines.
//! - [`scan`], [`cmd`]: the first tier. [`part`], [`judge`]: the second. [`score`]: the number for display. [`keep`]: the step that
//!   guards a surface (names, tests, headings) across a refactor.
//! - [`Quality`]: the doer `qc` that the rules `qc-live` and `qc-settle` and the command `:qc` start.

pub mod change;
pub mod cmd;
pub mod diff;
pub mod facts;
pub mod judge;
pub mod keep;
pub mod part;
pub mod pattern;
pub mod run;
pub mod scan;
pub mod score;

pub use judge::{Jev, Rubric};
pub use run::{Cfg, DOER, Quality};
pub use score::Weights;

use j4n_core::log::Log;
use j4n_core::redact::Redactor;
use j4n_core::setting::Settings;
use j4n_core::table::{Kind, Tables};
use j4n_core::value::Value;
use std::path::Path;

/// The checker the Table and the settings describe, and what was wrong with the rows (a row that does not read is left out, and says so).
pub fn load(tables: &Tables, settings: &Settings, root: &Path, home: &Path, log: &Log, red: &Redactor) -> (Quality, Vec<String>) {
    let (mut patterns, mut found) = pattern::load(tables);
    let skip: Vec<String> = settings.word("qc.skip", "").split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    for s in skip.iter().filter(|s| !patterns.iter().any(|p| &p.p.id == *s)) {
        found.push(format!("qc.skip names `{s}`, and there is no such pattern"));
    }
    patterns.retain(|p| !skip.contains(&p.p.id));
    let mut rubrics = Vec::new();
    for id in tables.ids(Rubric::NAME) {
        match tables.get::<Rubric>(id) {
            Ok(r) if r.levels.len() >= 2 => rubrics.push(r),
            Ok(r) => found.push(format!("qc_rubric {}: a rubric has at least two levels", r.id)),
            Err(p) => found.push(p.to_string()),
        }
    }
    let (keeps, bad) = keep::load(tables, settings.word("qc.keep", ""));
    found.extend(bad);
    let weights = tables.ids(Weights::NAME).into_iter().next().and_then(|id| match tables.get::<Weights>(id) {
        Ok(w) => Some(w),
        Err(p) => {
            found.push(p.to_string());
            None
        }
    });
    let cfg = Cfg {
        on: matches!(settings.get("qc.on"), Some(Value::Bool(true))),
        jev: matches!(settings.get("qc.jev"), Some(Value::Bool(true))),
        precision: settings.num("qc.steer_precision", 0.95),
        steer_n: settings.num("qc.steer_labels", 40.0) as usize,
        clock_ms: settings.dur_ms("qc.timeout", 5_000),
        patterns,
        rubrics,
        weights: weights.unwrap_or_default(),
        provenance: matches!(settings.get("qc.provenance"), Some(Value::Bool(true))),
        keeps,
        root: root.to_path_buf(),
        home: home.to_path_buf(),
    };
    (Quality::new(cfg, log.clone(), red.clone()), found)
}

impl Jev for j4n_connect::decide::Decider {
    fn model(&self) -> String {
        j4n_core::answer::Answerer::model(self)
    }

    fn zone(&self) -> String {
        self.zone_word()
    }

    fn batch(&self, state: &serde_json::Value, questions: &serde_json::Value) -> Result<serde_json::Value, String> {
        j4n_connect::decide::Decider::batch(self, state, questions)
    }
}
