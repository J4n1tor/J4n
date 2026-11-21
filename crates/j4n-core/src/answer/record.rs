//! The machine's record of passed probes: what a project's Log cannot give a new project. One file in J4n's own folder
//! (`<home>/cache/probe-passes.json`), a list of `{answerer, point, model, cases_hash, at_ms}` and nothing else — no case text, no
//! answers. A pass the Log has not seen is seeded from it ([`super::probe::Prober::due`]) when the same model passed the same cases
//! less than [`super::probe::STALE_MS`] ago; a failure takes the entry away. The fold stays the truth for the task: this only seeds it.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Pass {
    answerer: String,
    point: String,
    model: String,
    cases_hash: String,
    at_ms: u64,
}

impl Pass {
    fn same_pair(&self, answerer: &str, point: &str, model: &str) -> bool {
        self.answerer == answerer && self.point == point && self.model == model
    }
}

/// The file; a missing or unreadable one is an empty record, and a write that fails is lost (it is a cache of a check already done).
#[derive(Debug, Clone)]
pub struct Record {
    path: PathBuf,
}

impl Record {
    pub fn at(home: &std::path::Path) -> Record {
        Record { path: home.join("cache/probe-passes.json") }
    }

    fn read(&self) -> Vec<Pass> {
        std::fs::read(&self.path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn write(&self, all: &[Pass]) {
        let tmp = self.path.with_extension(format!("tmp{}", std::process::id()));
        let kept = self.path.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok())
            && serde_json::to_vec(all).is_ok_and(|b| std::fs::write(&tmp, b).is_ok() && std::fs::rename(&tmp, &self.path).is_ok());
        if !kept {
            let _ = std::fs::remove_file(&tmp);
        }
    }

    /// When this model passed exactly these cases, if that was less than `max_age_ms` before `now_ms`.
    pub fn passed(&self, answerer: &str, point: &str, model: &str, cases_hash: &str, now_ms: u64, max_age_ms: u64) -> Option<u64> {
        let found = self.read().into_iter().find(|p| p.same_pair(answerer, point, model) && p.cases_hash == cases_hash);
        found.map(|p| p.at_ms).filter(|at| now_ms.saturating_sub(*at) < max_age_ms)
    }

    /// A pass: it replaces what the model had for the point, and entries older than `max_age_ms` go.
    pub fn keep(&self, answerer: &str, point: &str, model: &str, cases_hash: &str, now_ms: u64, max_age_ms: u64) {
        let mut all: Vec<Pass> = self.read().into_iter().filter(|p| !p.same_pair(answerer, point, model) && now_ms.saturating_sub(p.at_ms) < max_age_ms).collect();
        all.push(Pass { answerer: answerer.into(), point: point.into(), model: model.into(), cases_hash: cases_hash.into(), at_ms: now_ms });
        self.write(&all);
    }

    /// A failure: the model's entries for the point go, whatever cases they were for.
    pub fn drop(&self, answerer: &str, point: &str, model: &str) {
        let all = self.read();
        if all.iter().any(|p| p.same_pair(answerer, point, model)) {
            self.write(&all.into_iter().filter(|p| !p.same_pair(answerer, point, model)).collect::<Vec<_>>());
        }
    }
}
