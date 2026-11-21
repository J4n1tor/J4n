//! What the Log knows that a run needs: the task that did the work (its line, its changed paths, its report, whether the project's own
//! check passed) and what the operator has said about marks (labels, which give a pattern its precision and a mark its dismissal).

use j4n_core::log::Log;
use serde_json::Value as Json;
use std::collections::{BTreeMap, BTreeSet};

/// The labels counted for a pattern's precision.
pub const LABELS: usize = 200;

#[derive(Debug, Default, Clone)]
pub struct Work {
    pub task: String,
    pub thread: Option<String>,
    pub goal: String,
    pub answer: String,
    pub paths: Vec<String>,
    pub deleted: Vec<String>,
    /// `pass`, `fail`, or empty when the project's check was not seen or could not run.
    pub check: String,
}

#[derive(Debug, Default)]
pub struct Facts {
    pub work: Option<Work>,
    /// Per pattern, the last `LABELS` labels: true when it was right (kept, fixed, bench_true).
    pub labels: BTreeMap<String, Vec<bool>>,
    pub dismissed: BTreeSet<String>,
    /// The last `qc.mark` or `qc.mark.state` event data of each mark id.
    pub marks: BTreeMap<String, Json>,
    /// The last `qc.switch`.
    pub switch: Option<bool>,
}

impl Facts {
    /// The precision of a pattern over its labels, and how many there are.
    pub fn precision(&self, pattern: &str) -> (f64, usize) {
        let l = self.labels.get(pattern).map(Vec::as_slice).unwrap_or_default();
        if l.is_empty() { (0.0, 0) } else { (l.iter().filter(|t| **t).count() as f64 / l.len() as f64, l.len()) }
    }
}

fn strings(v: &Json) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(Json::as_str).map(String::from).collect()
}

/// One pass over the Log.
pub fn read(log: &Log, work: Option<&str>) -> Facts {
    log.sync();
    let mut f = Facts::default();
    let mut w = work.map(|t| Work { task: t.to_string(), ..Work::default() });
    let mut check_task: Option<String> = None;
    let mut check_seen: BTreeMap<String, String> = BTreeMap::new();
    for ev in log.read(0) {
        let d = &ev.data;
        match ev.kind.as_str() {
            "qc.switch" => f.switch = d["on"].as_bool(),
            "qc.mark" | "qc.mark.state" => {
                if let Some(id) = d["mark"].as_str() {
                    f.marks.insert(id.to_string(), d.clone());
                }
            }
            "label" if d["subject_kind"] == "qc_mark" => {
                let (pattern, label) = (d["producer"].as_str().unwrap_or_default(), d["label"].as_str().unwrap_or_default());
                if label == "dismissed" {
                    if let Some(id) = d["subject"].as_str() {
                        f.dismissed.insert(id.to_string());
                    }
                } else if label == "kept"
                    && let Some(id) = d["subject"].as_str()
                {
                    f.dismissed.remove(id);
                }
                // a dismissal says what the pattern was wrong about only when its reason is `wrong` (or none was given: the label of before)
                let judged = label != "dismissed" || matches!(d["point"].as_str().unwrap_or_default(), "" | "wrong");
                if judged && matches!(label, "kept" | "fixed" | "bench_true" | "dismissed" | "bench_false") {
                    let v = f.labels.entry(pattern.to_string()).or_default();
                    v.push(matches!(label, "kept" | "fixed" | "bench_true"));
                    if v.len() > LABELS {
                        v.remove(0);
                    }
                }
            }
            "task.queued" => {
                let Some(id) = ev.task.as_deref() else { continue };
                let d = &d["spec"];
                if work == Some(id)
                    && let Some(w) = w.as_mut()
                {
                    w.goal = d["goal"].as_str().unwrap_or_default().to_string();
                    w.thread = ev.thread.clone();
                }
                if let Some(t) = work
                    && d["doer"] == "check"
                    && d["place"].as_str() == Some(&format!("@{t}"))
                {
                    check_task = Some(id.to_string());
                }
            }
            "run.changed" if work.is_some() && ev.task.as_deref() == work => {
                if let Some(w) = w.as_mut() {
                    for p in strings(&d["paths"]) {
                        if !w.paths.contains(&p) {
                            w.paths.push(p);
                        }
                    }
                }
            }
            "task.settled" | "task.stopped" => {
                let Some(id) = ev.task.as_deref() else { continue };
                if work == Some(id)
                    && ev.kind == "task.settled"
                    && let Some(w) = w.as_mut()
                {
                    w.answer = d["answer"].as_str().unwrap_or_default().to_string();
                    let c = &d["changed"];
                    for p in strings(&c["created"]).into_iter().chain(strings(&c["modified"])) {
                        if !w.paths.contains(&p) {
                            w.paths.push(p);
                        }
                    }
                    w.deleted = strings(&c["deleted"]);
                }
                let verdict = if ev.kind == "task.stopped" { "fail" } else if d["answer"].as_str().is_some_and(|a| a.ends_with(": passed")) { "pass" } else { "" };
                check_seen.insert(id.to_string(), verdict.to_string());
            }
            _ => {}
        }
    }
    if let Some(w) = w.as_mut() {
        w.check = check_task.and_then(|t| check_seen.get(&t).cloned()).unwrap_or_default();
    }
    f.work = w;
    f
}
