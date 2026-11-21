//! The way of working, asked when the read is unsure. Under `orchestrate` a plain line is read once (`look`); when the read is unsure which way fits
//! (today one case, read in the code: it said `split` and its model step named no parts, so no part was seen and Main would plan from nothing), one
//! short choice is put before any work starts: one lane, or split into chats, the read's way marked recommended with its reason, and the row
//! `custom` for the operator's own words. The question is the ask point `way` shown as a pick row (`hand`); its options are the point's.
//!
//! `orchestrate.way` `decide` takes the read's way with no question. A run with nobody to ask (no keyboard, `--approval full`) takes it too, and
//! the Log says so (`orchestrate.way`, `by`). No cost is shown: none of these ways has a measured price per line.

use super::Front;
use super::look::{Decision, Looked};
use super::split::After;
use crate::ask::Opt;
use crate::change::Actor;
use serde_json::json;

/// The ask point of the question.
pub const POINT: &str = "way";
/// The ways, as the point's options name them.
pub const ONE: &str = "one";
pub const SPLIT: &str = "split";

impl Front {
    /// The read is unsure of the way for `text` (`why`): the operator picks it, or the read's way (one lane) stands. Returns what goes to Main.
    pub(super) fn way(&self, thread: &str, text: &str, looked: Looked, why: &str) -> After {
        let reason = if looked.why.is_empty() { why.to_string() } else { format!("{why}: {}", looked.why) };
        let says = self.0.who.says().get(POINT).cloned().unwrap_or_default();
        let options: Vec<Opt> = [ONE, SPLIT].iter().map(|id| Opt { id: (*id).into(), label: says.get(*id).cloned().unwrap_or_else(|| (*id).into()), recommended: *id == ONE }).collect();
        let point = self.0.who.point(POINT).cloned();
        let nobody = self.0.who.headless() || self.0.who.approves_all();
        let (picked, by) = match point {
            _ if !self.0.cfg().orchestrate.way_ask => (None, "decide"),
            None => (None, "no question"),
            Some(_) if nobody => (None, "no operator"),
            Some(p) => {
                let wait = p.chain.iter().find_map(|s| s.after_ms).map_or(super::hand::WAIT, std::time::Duration::from_millis);
                let why = format!("unsure: {reason} · ⏎ takes the recommended, a digit picks, or type your own way");
                let a = super::hand::Asking { asker: "read", question: "how should this line be worked?", why: &why, line: text, options: &options, words: true, wait };
                match self.0.who.hands().ask_as(&a) {
                    Some(p) => (Some(p), "you"),
                    None => (None, "fallback"),
                }
            }
        };
        use super::hand::Picked;
        let (pick, words) = match &picked {
            Some(Picked::Id(id)) => (id.as_str(), None),
            Some(Picked::Words(w)) => (ONE, Some(w.as_str())),
            None => (ONE, None),
        };
        self.write(thread, "orchestrate.way", if by == "you" { Actor::Operator } else { Actor::Core }, json!({ "line": crate::text::clip(text, super::thread::CHAT_LINE), "ways": [ONE, SPLIT], "recommended": ONE, "pick": pick, "by": by, "why": reason, "words": words }));
        let head = match (pick, words) {
            (SPLIT, _) => "split into chats",
            (_, Some(_)) => "one lane, in your words",
            _ => "one lane",
        };
        let who = match by {
            "you" => "you picked it",
            "decide" => "the read's way (`orchestrate.way` decide)",
            "no operator" => "the read's way: nobody to ask",
            _ => "the read's way",
        };
        self.note(thread, &format!("way: {head} · {who}"), "dim");
        match (pick, words) {
            (SPLIT, _) => self.split(thread, text, &Looked { decision: Decision::Split, ..looked }, &self.catalogue()),
            (_, Some(w)) => After::Send(Some(format!("{text}\n\nHow to work it, in the operator's words: {w}"))),
            _ => After::Send(None),
        }
    }
}
