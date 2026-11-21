//! Machines that answer a question: the **deciders** `systemone` and `decisions`, small trained models
//! behind a closed vocabulary, and the **model** answerer, a chat model asked for one closed JSON answer. All three are
//! [`Answerer`]s of the chain; none is ever called for an approval, and every one is put through the egress Gate by the walk
//! (it names its [`Answerer::zone`]) before a question leaves.
//!
//! What they share is here: the question as text, and the zone a URL is in.

mod jev;
mod model;

pub use jev::{Decider, DeciderConfig, Wire as DeciderWire, paid_of};
pub use model::{ModelAnswerer, ModelConfig};

use j4n_core::answer::Question;
use j4n_core::ask::AskShape;

/// `local` for a URL that is this machine, else the connector's own word (`any` unless it says `trusted`).
pub fn zone_of(url: &str, said: Option<&str>) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = match host.strip_prefix('[') {
        Some(inner) => inner.split(']').next().unwrap_or_default(),
        None => host.rsplit_once(':').map_or(host, |(h, _)| h),
    };
    if matches!(host, "localhost" | "127.0.0.1" | "::1") {
        return "local".into();
    }
    said.filter(|z| !z.is_empty()).unwrap_or("any").to_string()
}

/// The question in words for a model that reads: what is asked, what was being done, and the options with their ids.
pub fn render(q: &Question) -> String {
    let mut out = format!("Question: {}\n", q.text.trim());
    if !q.goal.trim().is_empty() && q.goal.trim() != q.text.trim() {
        out.push_str(&format!("What is being done: {}\n", q.goal.trim()));
    }
    match &q.shape {
        AskShape::YesNo => out.push_str("Answer yes or no.\n"),
        AskShape::Choice { options } => {
            out.push_str("Options:\n");
            for o in options {
                out.push_str(&format!("- {}: {}\n", o.id, o.label));
            }
        }
        AskShape::Approval { .. } | AskShape::Text => {}
    }
    out
}
