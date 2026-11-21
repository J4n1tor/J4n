//! `/context`: what each chat's doer holds and what the next line costs, from the Log's tasks and
//! nothing else: no doer, no model, no task. For each chat and each `doer · model` there, one group: how full its window is, the session's
//! floor and history, and the line that matters, warm or cold. It is held in the chat as a row, as `/usage` is, and drawn again as
//! tasks end.

use super::Front;
use super::usage::{hm, plain};
use crate::change::Actor;
use crate::task::context::{Price, Session, k, sessions};
use crate::task::id_number;
use serde_json::{Value as Json, json};

/// The id of this view in a `chat.view` event and in the row that holds it.
pub const VIEW: &str = "context";

/// `under 1m`, `5m`, `47m`, or `3:20` from two hours.
fn span(ms: u64) -> String {
    match ms {
        0..60_000 => "under 1m".into(),
        60_000..7_200_000 => format!("{}m", ms / 60_000),
        _ => hm(ms),
    }
}

/// Dollars as the view says them: four places under ten cents, else two.
pub(super) fn usd(x: f64) -> String {
    if x < 0.1 { format!("${x:.4}") } else { format!("${x:.2}") }
}

/// The entries of the view as the screen's `meter` takes them. `cache` is the provider's cache life of a doer (none: not said) and `price` what a
/// session's doer and model cost (none: not known). The chat `thread` is first, and is said even when nothing has run in it.
pub fn view(list: &[Session], thread: &str, cache: impl Fn(&str) -> Option<u64>, price: impl Fn(&Session) -> Option<Price>, now_ms: u64) -> Json {
    let mut out = Vec::new();
    let mut shown: Vec<&Session> = list.iter().collect();
    shown.sort_by_key(|s| s.thread != thread);
    if !shown.iter().any(|s| s.thread == thread) {
        out.push(json!({ "group": thread }));
        out.push(json!({ "text": "nothing held yet: no line has run in this chat", "tone": "dim" }));
    }
    for s in shown {
        out.push(json!({ "group": format!("{} · {}", s.thread, s.who) }));
        if let Some(w) = s.window {
            out.push(json!({ "label": "win", "f": w, "note": "of the doer's window" }));
        }
        out.push(json!({ "text": match (s.floor, s.history()) {
            _ if s.unsure => format!("{} held before the last :compact · what it holds now is not said: the next line shows it", k(s.held)),
            (Some(f), Some(h)) => format!("{} held: floor {} + history {}", k(s.held), k(f), k(h)),
            _ => format!("{} held · floor not known: the Log has no first turn of this session", k(s.held)),
        } }));
        if let Some(x) = &s.shrunk {
            let ago = span(now_ms.saturating_sub(x.at_ms));
            let how = if x.mode == "plain" { String::new() } else { format!(" {}", x.mode) };
            out.push(json!({ "text": match x.after {
                Some(a) => format!("last :compact{how}: {} → {} (cut {}) · {ago} ago", k(x.before), k(a), k(x.before.saturating_sub(a))),
                None => format!("last :compact{how}: {} before · after not said by the doer · {ago} ago", k(x.before)),
            }, "tone": "dim" }));
        }
        let (age, cache) = (now_ms.saturating_sub(s.last_ms), cache(&s.doer));
        let cold = s.cold(now_ms, cache);
        let state = match cold {
            Some(true) => "cold now",
            Some(false) => "warm now",
            None => "warm or cold not known",
        };
        let when = cache.map(|life| format!(" · last turn {} ago, the cache lasts {}", span(age), span(life))).unwrap_or_default();
        let text = match price(s) {
            Some(p) => {
                let (warm, cold) = p.next(s.held);
                format!("next line ≈ {} warm · {} cold ({state}){when}", usd(warm), usd(cold))
            }
            None => format!("next line: {} in ({state}) · price not known: no priced turn of {} in the Log, or {}'s cost source has no `ratio`{when}", k(s.held), s.who, s.doer),
        };
        out.push(if cold == Some(true) { json!({ "text": text, "tone": "warn" }) } else if cold.is_none() { json!({ "text": text, "tone": "dim" }) } else { json!({ "text": text }) });
    }
    Json::Array(out)
}

impl Front {
    /// The sessions the Log's tasks leave live: what `:fresh` ended is not one.
    pub(super) fn live_sessions(&self) -> Vec<Session> {
        let rows: Vec<_> = self.0.tasks.book().rows().into_iter().filter(|r| self.0.binds.cut(r.spec.thread.as_deref().unwrap_or_default()).and_then(|c| id_number(&c)).is_none_or(|c| id_number(&r.id).is_some_and(|n| n > c))).collect();
        sessions(&rows)
    }

    /// The view as the screen's `meter` field is given it, `{field, value}`, for the chat `thread`.
    pub fn context_value(&self, thread: &str) -> Json {
        let (plans, now, book, set) = (self.0.tasks.plans(), self.log().clock().now_ms(), self.0.tasks.book(), self.0.tasks.settings());
        let price = |s: &Session| Price::of(book, &s.doer, &s.model, &plans, &set.settings);
        json!({ "field": "meter", "value": view(&self.live_sessions(), thread, |d| plans.cache_ms(d), price, now) })
    }

    /// `/context`: the view, held in the chat as a row of its own, and its lines as plain words for a stream that cannot draw it.
    pub(super) fn context_say(&self, thread: &str) {
        let text = plain(&self.context_value(thread)["value"]);
        self.write(thread, "chat.view", Actor::Core, json!({ "view": VIEW, "title": "context", "text": text }));
    }
}
