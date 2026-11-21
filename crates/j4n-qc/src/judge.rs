//! The second tier: Jev, asked about each part in one call that carries every question that applies —
//! one `noul` for each `jev` pattern whose `lang` matches, one `choice` for the look-alike helpers, and the three rubrics as `score`s.
//! A call that fails is an error of the tier, never a part that came out clean.

use crate::part::Part;
use crate::pattern::{Ready, Way};
use crate::scan::Mark;
use j4n_core::table::Kind;
use serde::Deserialize;
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

/// The most questions in one call, and the most calls at a time.
pub const PER_CALL: usize = 24;
pub const AT_A_TIME: usize = 4;
/// A noul at or over this is a mark.
const YES: f64 = 0.5;

/// An entry of kind `qc_rubric`: a question about the whole part with ordered answers.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rubric {
    pub id: String,
    #[serde(default)]
    pub doc: String,
    pub instructions: String,
    pub levels: Vec<String>,
}

impl Kind for Rubric {
    const NAME: &'static str = "qc_rubric";
}

/// Jev, as far as QC needs it: several questions about one state in one call.
pub trait Jev: Send + Sync {
    fn model(&self) -> String;
    /// The zone its provider is in, for the egress Gate.
    fn zone(&self) -> String;
    /// The reply: `answers` by question id, and `usage`.
    fn batch(&self, state: &Json, questions: &Json) -> Result<Json, String>;
}

/// The parts whose call failed, with why.
pub type Failed = Vec<(usize, String)>;

/// What one part came to.
#[derive(Debug, Default)]
pub struct Judged {
    pub marks: Vec<Mark>,
    /// The position on each rubric, from 0 up to its top level.
    pub rubrics: BTreeMap<String, f64>,
    pub tokens: u64,
    pub calls: usize,
}

/// The questions one part is asked: `p:<pattern>`, `c:<pattern>` for a choice, `r:<rubric>`.
pub fn questions(patterns: &[Ready], rubrics: &[Rubric], part: &Part, state: &Json) -> Vec<(String, Json)> {
    let mut out = Vec::new();
    let similar: Vec<&Json> = state["similar"].as_array().into_iter().flatten().collect();
    for r in patterns.iter().filter(|r| r.p.kind == Way::Jev && r.applies(&part.file)) {
        let instructions = r.p.instructions.clone().unwrap_or_default();
        if r.p.ty.as_deref() == Some("choice") {
            if similar.is_empty() {
                continue;
            }
            let mut criteria = serde_json::Map::new();
            for s in &similar {
                criteria.insert(s["name"].as_str().unwrap_or_default().to_string(), s["sig"].clone());
            }
            criteria.insert("none".into(), json!("none of the listed functions does it"));
            out.push((format!("c:{}", r.p.id), json!({ "type": "choice", "instructions": instructions, "criteria": criteria })));
        } else {
            out.push((format!("p:{}", r.p.id), json!({ "type": "noul", "instructions": instructions })));
        }
    }
    for ru in rubrics {
        out.push((format!("r:{}", ru.id), json!({ "type": "score", "instructions": ru.instructions, "criteria": ru.levels })));
    }
    out
}

/// One call, with a clock on it.
fn call(jev: &dyn Jev, state: &Json, qs: &[(String, Json)], clock: Duration) -> Result<Json, String> {
    let body: serde_json::Map<String, Json> = qs.iter().cloned().collect();
    let (tx, rx) = mpsc::channel();
    std::thread::scope(|s| {
        s.spawn(|| {
            let _ = tx.send(jev.batch(state, &Json::Object(body)));
        });
        rx.recv_timeout(clock).unwrap_or_else(|_| Err(format!("it did not answer in {} ms", clock.as_millis())))
    })
}

fn read(patterns: &[Ready], part: &Part, reply: &Json, into: &mut Judged) {
    let answers = reply["answers"].as_object();
    for (id, a) in answers.into_iter().flatten() {
        let Some((kind, name)) = id.split_once(':') else { continue };
        match kind {
            "p" => {
                let p = a["noul"].as_f64().unwrap_or(0.0).clamp(0.0, 1.0);
                if let (true, Some(r)) = (p >= YES, patterns.iter().find(|r| r.p.id == name)) {
                    into.marks.push(Mark { tier: 1, p, ..Mark::at(r, &part.file, part.line()) });
                }
            }
            "c" => {
                let (pick, conf) = (a["choice"].as_str().unwrap_or("none"), a["confidence"].as_f64().or_else(|| a["probabilities"][a["choice"].as_str().unwrap_or("")].as_f64()).unwrap_or(0.0));
                if let (true, Some(r)) = (pick != "none" && conf >= YES, patterns.iter().find(|r| r.p.id == name)) {
                    into.marks.push(Mark { tier: 1, p: conf, answer: pick.to_string(), ..Mark::at(r, &part.file, part.line()) });
                }
            }
            "r" => {
                if let Some(s) = a["score"].as_f64() {
                    into.rubrics.insert(name.to_string(), s);
                }
            }
            _ => {}
        }
    }
    into.tokens += reply["usage"]["input_tokens"].as_u64().unwrap_or(0) + reply["usage"]["output_tokens"].as_u64().unwrap_or(0);
}

/// The answers for every part: one call per part for each 24 questions, four at a time. A part whose call failed is in `errors`.
pub fn judge(jev: &dyn Jev, patterns: &[Ready], rubrics: &[Rubric], parts: &[(Part, Json)], clock: Duration) -> (Vec<Judged>, Failed) {
    let mut jobs: Vec<(usize, Vec<(String, Json)>)> = Vec::new();
    for (i, (part, state)) in parts.iter().enumerate() {
        for chunk in questions(patterns, rubrics, part, state).chunks(PER_CALL) {
            jobs.push((i, chunk.to_vec()));
        }
    }
    let next = AtomicUsize::new(0);
    let done: Mutex<Vec<(usize, Result<Json, String>)>> = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..AT_A_TIME.min(jobs.len()) {
            s.spawn(|| {
                loop {
                    let n = next.fetch_add(1, Ordering::SeqCst);
                    let Some((i, qs)) = jobs.get(n) else { break };
                    let reply = call(jev, &parts[*i].1, qs, clock);
                    done.lock().unwrap_or_else(|e| e.into_inner()).push((*i, reply));
                }
            });
        }
    });
    let mut judged: Vec<Judged> = parts.iter().map(|_| Judged::default()).collect();
    let mut errors = Vec::new();
    for (i, reply) in done.into_inner().unwrap_or_default() {
        judged[i].calls += 1;
        match reply {
            Ok(r) => read(patterns, &parts[i].0, &r, &mut judged[i]),
            Err(why) => errors.push((i, why)),
        }
    }
    (judged, errors)
}
