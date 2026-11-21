//! The deciders (dialects `systemone` and `decisions`): typed questions answered with probabilities, in about a second and for
//! next to nothing.
//!
//! - `systemone`: `POST {base}/v1/systemone` with `{state, model, questions: {q: {type, instructions, criteria}}}`; a `noul`
//!   question gets a probability, a `choice` the option and its probabilities.
//! - `decisions`: `POST {base}/api/alpha/decisions`, which takes `noul` questions only. A choice becomes one `noul` per option
//!   ("is `<id>` the pick?") and the answer is the option with the highest probability, which is also its confidence.
//!
//! A yes/no answer is yes at half or more, with the confidence of whichever side is likelier. Only yes/no and choice are
//! put to a decider; a text, an approval is a `Pass`. Any status but 200, a body that is not the contract, and a transport
//! failure after the retries (429 and 5xx only, with a pause between) are `Failed`, which the breaker counts: a decision
//! never silently reads as a default.

use super::{render, zone_of};
use crate::net::Net;
use j4n_core::answer::{Answerer, Kind, Offer, Paid, Question};
use j4n_core::ask::{Answer, AskShape};
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::time::Duration;

const RETRYABLE: [u16; 5] = [429, 500, 502, 503, 504];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wire {
    Systemone,
    Decisions,
}

#[derive(Clone)]
pub struct DeciderConfig {
    /// The connector's id: the decider is `decider:<id>`.
    pub id: String,
    pub wire: Wire,
    pub base: String,
    pub key: String,
    pub model: String,
    pub zone: Option<String>,
    pub net: Net,
    /// The pauses before the second and the third try.
    pub backoff: Vec<Duration>,
}

pub struct Decider {
    cfg: DeciderConfig,
    id: String,
    /// Where the key is read at each question; none: `cfg.key`.
    fresh: Option<crate::model::KeyFn>,
}

impl Decider {
    pub fn new(cfg: DeciderConfig) -> Decider {
        let id = format!("decider:{}", cfg.id);
        Decider { cfg, id, fresh: None }
    }

    /// Read the key from `fresh` at each question, so a key replaced or removed while J4n runs acts at once.
    pub fn keyed(mut self, fresh: Option<crate::model::KeyFn>) -> Decider {
        self.fresh = fresh;
        self
    }

    fn url(&self) -> String {
        let base = self.cfg.base.trim_end_matches('/');
        match self.cfg.wire {
            Wire::Systemone => format!("{base}/v1/systemone"),
            Wire::Decisions => format!("{base}/api/alpha/decisions"),
        }
    }

    /// The questions for `q`, and for `decisions` the options each fan stands for. `None` for what a decider does not take.
    fn ask(&self, q: &Question) -> Option<(Json, Vec<String>)> {
        let state = json!({ "point": q.point, "question": q.text, "goal": q.goal });
        let (questions, keys) = match &q.shape {
            AskShape::YesNo => (json!({ "q": { "type": "noul", "instructions": render(q) } }), Vec::new()),
            AskShape::Choice { options } if !options.is_empty() => match self.cfg.wire {
                Wire::Systemone => {
                    let criteria: serde_json::Map<String, Json> = options.iter().map(|o| (o.id.clone(), Json::String(o.label.clone()))).collect();
                    (json!({ "q": { "type": "choice", "instructions": render(q), "criteria": criteria } }), Vec::new())
                }
                Wire::Decisions => {
                    let fans: serde_json::Map<String, Json> = options
                        .iter()
                        .enumerate()
                        .map(|(i, o)| (format!("q__c{i}"), json!({ "type": "noul", "instructions": format!("{}\nThe candidate pick under test is: {}", render(q), o.id), "criteria": { "true": o.label, "false": format!("`{}` is not the pick this state calls for", o.id) } })))
                        .collect();
                    (Json::Object(fans), options.iter().map(|o| o.id.clone()).collect())
                }
            },
            _ => return None,
        };
        let state = match self.cfg.wire {
            // this contract takes the state as a plain string
            Wire::Decisions => Json::String(state.to_string()),
            Wire::Systemone => state,
        };
        Some((json!({ "state": state, "model": self.cfg.model, "questions": questions }), keys))
    }

    /// One call with several questions about one state, of any type the wire takes (`noul`, `choice`, `score`): the `answers` and
    /// the `usage` of its reply. Only `systemone` takes more than one question; any other failure is an error, never a default.
    pub fn batch(&self, state: &Json, questions: &Json) -> Result<Json, String> {
        if self.cfg.wire != Wire::Systemone {
            return Err("this decider takes one yes/no question at a time".into());
        }
        let reply = self.post(&json!({ "state": state, "model": self.cfg.model, "questions": questions }))?;
        reply.get("answers").and_then(Json::as_object).ok_or("its reply has no answers")?;
        Ok(reply)
    }

    /// The zone its provider is in, for the egress Gate.
    pub fn zone_word(&self) -> String {
        zone_of(&self.cfg.base, self.cfg.zone.as_deref())
    }

    /// The body of a 200, after the tries 429 and 5xx earn.
    fn post(&self, body: &Json) -> Result<Json, String> {
        let url = self.url();
        let key = self.fresh.as_ref().map_or_else(|| self.cfg.key.clone(), |f| f());
        if key.is_empty() {
            return Err("there is no key for it".into());
        }
        let auth = format!("Bearer {key}");
        let mut last = String::new();
        for attempt in 0..=self.cfg.backoff.len() {
            if attempt > 0 {
                std::thread::sleep(self.cfg.backoff[attempt - 1]);
            }
            match self.cfg.net.post_json(&url, &[("Authorization", &auth)], body) {
                Err(e) => last = format!("could not reach it: {e}"),
                Ok(r) if r.status == 200 => return serde_json::from_str(&r.text().map_err(|e| e.to_string())?).map_err(|e| format!("its reply is not JSON: {e}")),
                Ok(r) => {
                    let status = r.status;
                    last = format!("HTTP {status}: {}", crate::model::excerpt(&r.text().unwrap_or_default()));
                    if !RETRYABLE.contains(&status) {
                        break;
                    }
                }
            }
        }
        Err(last)
    }
}

/// What a reply says it cost: the tokens of its `usage` (`input_tokens`, `output_tokens`), and the dollars when the usage
/// names them (`cost_usd`, `usd` or `cost`). A reply with no `usage`, or none with a price, leaves `usd` unknown: never a guess.
pub fn paid_of(reply: &Json) -> Paid {
    let u = reply.get("usage");
    let n = |k: &str| u.and_then(|u| u.get(k)).and_then(Json::as_u64).unwrap_or(0);
    let usd = ["cost_usd", "usd", "cost"].iter().find_map(|k| u.and_then(|u| u.get(*k)).and_then(Json::as_f64)).filter(|x| x.is_finite() && *x >= 0.0);
    Paid { usd, tokens_in: n("input_tokens"), tokens_out: n("output_tokens") }
}

/// The answer to the question `qid`: its object.
fn answer_of<'a>(reply: &'a Json, qid: &str) -> Result<&'a Json, String> {
    reply.pointer(&format!("/answers/{qid}")).ok_or_else(|| format!("its reply has no answer for `{qid}`"))
}

fn noul(a: &Json, qid: &str) -> Result<f64, String> {
    a.get("noul").and_then(Json::as_f64).map(|p| p.clamp(0.0, 1.0)).ok_or_else(|| format!("its answer for `{qid}` has no probability"))
}

impl Answerer for Decider {
    fn id(&self) -> &str {
        &self.id
    }

    fn kind(&self) -> Kind {
        Kind::Decider
    }

    fn model(&self) -> String {
        format!("{}@{}", self.cfg.model, match self.cfg.wire {
            Wire::Systemone => "systemone",
            Wire::Decisions => "decisions",
        })
    }

    fn zone(&self) -> Option<String> {
        Some(zone_of(&self.cfg.base, self.cfg.zone.as_deref()))
    }

    fn offer(&self, q: &Question) -> Offer {
        self.offer_paid(q).0
    }

    fn offer_paid(&self, q: &Question) -> (Offer, Option<Paid>) {
        // nothing was sent, or nothing came back: nothing was billed
        let nothing = Some(Paid { usd: Some(0.0), ..Paid::default() });
        let Some((body, keys)) = self.ask(q) else { return (Offer::Pass { why: "a decider takes a yes/no or a choice".into() }, nothing) };
        let reply = match self.post(&body) {
            Ok(r) => r,
            Err(why) => return (Offer::Failed { why }, nothing),
        };
        let paid = Some(paid_of(&reply));
        let read = || -> Result<(Answer, f32, String), String> {
            match &q.shape {
                AskShape::YesNo => {
                    let p = noul(answer_of(&reply, "q")?, "q")?;
                    Ok((if p >= 0.5 { Answer::Yes } else { Answer::No }, p.max(1.0 - p) as f32, format!("p(yes) {p:.2}")))
                }
                _ if self.cfg.wire == Wire::Decisions => {
                    let probs: Vec<f64> = (0..keys.len()).map(|i| noul(answer_of(&reply, &format!("q__c{i}"))?, &format!("q__c{i}"))).collect::<Result<_, _>>()?;
                    let best = (0..keys.len()).fold(0, |b, i| if probs[i] > probs[b] { i } else { b });
                    Ok((Answer::Pick { id: keys[best].clone() }, probs[best] as f32, format!("p {:.2} of {}", probs[best], keys.len())))
                }
                _ => {
                    let a = answer_of(&reply, "q")?;
                    let id = a.get("choice").and_then(Json::as_str).ok_or("its answer names no choice")?;
                    let probs: BTreeMap<String, f64> = a.get("probabilities").and_then(Json::as_object).map(|m| m.iter().filter_map(|(k, v)| v.as_f64().map(|f| (k.clone(), f))).collect()).unwrap_or_default();
                    let conf = a.get("confidence").and_then(Json::as_f64).or_else(|| probs.get(id).copied()).ok_or("its answer has no confidence")?;
                    Ok((Answer::Pick { id: id.to_string() }, conf as f32, format!("chose {id}")))
                }
            }
        };
        match read() {
            Ok((answer, conf, why)) => (Offer::Answer { answer, conf, why }, paid),
            Err(why) => (Offer::Failed { why }, paid),
        }
    }
}
