//! A chat model as an answerer (`model:<id>`): asked one closed question, it answers with one JSON object,
//! `{"answer": "<option id | yes | no>", "confidence": 0–1, "why": "<a short line>"}`, and nothing else. A reply that is not
//! that is a failure (the breaker counts it); an answer that is not one of the offered options is for the chain to refuse.

use super::{render, zone_of};
use crate::model::{Client, Fail, Msg, Req};
use j4n_core::answer::{Answerer, Kind, Offer, Paid, Question};
use j4n_core::ask::{Answer, AskShape};
use serde_json::Value as Json;
use std::sync::atomic::AtomicBool;

/// The most a closed answer may write.
const ANSWER_MAX: u32 = 512;
const SYSTEM: &str = "You answer one closed question for a program. Reply with only a JSON object: \
{\"answer\": \"<one of the option ids, or yes or no>\", \"confidence\": <a number from 0 to 1>, \"why\": \"<one short line>\"}. \
No other words.";

pub struct ModelConfig {
    /// The connector's id: the answerer is `model:<id>`.
    pub id: String,
    pub model: String,
    pub zone: Option<String>,
    pub base: String,
}

pub struct ModelAnswerer {
    cfg: ModelConfig,
    id: String,
    client: Client,
}

impl ModelAnswerer {
    pub fn new(cfg: ModelConfig, client: Client) -> ModelAnswerer {
        let id = format!("model:{}", cfg.id);
        ModelAnswerer { cfg, id, client }
    }
}

/// The first JSON object in a reply, which may be fenced or have words around it.
fn object_in(text: &str) -> Option<Json> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    serde_json::from_str::<Json>(text.get(start..=end)?).ok().filter(Json::is_object)
}

impl Answerer for ModelAnswerer {
    fn id(&self) -> &str {
        &self.id
    }

    fn kind(&self) -> Kind {
        Kind::Model
    }

    fn model(&self) -> String {
        self.cfg.model.clone()
    }

    fn zone(&self) -> Option<String> {
        Some(zone_of(&self.cfg.base, self.cfg.zone.as_deref()))
    }

    fn offer(&self, q: &Question) -> Offer {
        self.offer_paid(q).0
    }

    fn offer_paid(&self, q: &Question) -> (Offer, Option<Paid>) {
        // nothing was sent, or nothing came back: nothing was billed (a reply that came back empty did cost its tokens: price unknown)
        let nothing = Some(Paid { usd: Some(0.0), ..Paid::default() });
        if matches!(q.shape, AskShape::Approval { .. } | AskShape::Text) {
            return (Offer::Pass { why: "a model answers a yes/no or a choice here".into() }, nothing);
        }
        let msgs = [Msg::User(render(q))];
        let req = Req { system: SYSTEM, max: ANSWER_MAX, ..Req::new(&self.cfg.model, &msgs) };
        let reply = match self.client.send(&req, &mut |_| {}, &AtomicBool::new(false)) {
            Ok(r) => r,
            Err(Fail::Cancelled) => return (Offer::Pass { why: "stopped".into() }, nothing),
            Err(f) => {
                let paid = if let Fail::Empty { tokens } = &f { Some(Paid { usd: None, tokens_in: 0, tokens_out: *tokens }) } else { nothing };
                return (Offer::Failed { why: f.words(0) }, paid);
            }
        };
        let u = reply.usage;
        let paid = Some(Paid { usd: u.usd.filter(|x| x.is_finite() && *x >= 0.0), tokens_in: u.input, tokens_out: u.output });
        (self.read(q, &reply.text), paid)
    }
}

impl ModelAnswerer {
    /// The offer a reply's text comes to.
    fn read(&self, q: &Question, text: &str) -> Offer {
        let Some(obj) = object_in(text) else { return Offer::Failed { why: "its reply is not the JSON object asked for".into() } };
        let said = obj.get("answer").and_then(Json::as_str).map(|s| s.trim().trim_matches('`').to_string()).unwrap_or_default();
        let answer = match (&q.shape, said.to_lowercase().as_str()) {
            (AskShape::YesNo, "yes") => Answer::Yes,
            (AskShape::YesNo, "no") => Answer::No,
            (AskShape::Choice { .. }, _) if !said.is_empty() => Answer::Pick { id: said },
            _ => return Offer::Failed { why: "its answer is not one that was offered".into() },
        };
        let Some(conf) = obj.get("confidence").and_then(Json::as_f64) else { return Offer::Failed { why: "its reply has no confidence".into() } };
        let why = obj.get("why").and_then(Json::as_str).unwrap_or("it did not say");
        Offer::Answer { answer, conf: conf as f32, why: why.chars().take(160).collect() }
    }
}
