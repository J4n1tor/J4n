//! Links at the places a message passes. The rows are `crate::links`; this holds the messages to them: an envoy's request to Main, Main's brief to a chat, a chat's
//! report and opinion to Main, Main's report to the envoy and a question of the work to its envoy. A link is an AND on what already decides (the envoy's tools, the
//! charter, the Gate, the clamps): a row never lets a message through that those refuse, it only refuses one that no row carries. The parties are the core's records
//! (the thread of Main, `chat.seated`, the envoy chats), never a name a model wrote. With the crew off every message passes as it did.
//!
//! What a refusal does: a brief stays unstarted and Main says so, a report is withheld from Main with a note that it stops there, a question is shown where questions
//! of that task were shown before, a request is an error to the envoy. Under `declared` each message that passes is written (`crew.sent`, on the thread whose hops it
//! counts) and each refusal is (`crew.refused`).
//!
//! An opinion is a chat's pick and reason that its report carried; it reaches Main as data in its Brief, wakes it once, and when Main's turn ends `crew.decided` names
//! the opinions it had and the one it followed, read from the last line of its answer (`followed: o2`, or `none`) and kept only if code gave out that id.

use super::Front;
use crate::change::Actor;
use crate::links::Party;
use crate::report::Opinion;
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;

/// One opinion as the Log keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Given {
    pub id: String,
    pub chat: String,
    pub pick: String,
    pub why: String,
    pub at_ms: u64,
}

impl Front {
    /// Whether links hold messages: only while the crew is on.
    pub(super) fn links_hold(&self) -> bool {
        self.0.cfg().crew.on
    }

    /// What `thread` is, from the core's records. The seat of a chat is read only when the rows are declared: the star names no seat.
    pub(super) fn party(&self, thread: &str) -> Party {
        if thread == self.0.cfg().thread {
            return Party::Main;
        }
        if self.is_envoy(thread) {
            return Party::Envoy(thread.to_string());
        }
        let seat = self.0.cfg().crew.links.declared().then(|| self.0.log.read(0).filter(|e| e.kind == "chat.seated" && e.thread.as_deref() == Some(thread)).filter_map(|e| e.data.get("seat").and_then(Json::as_str).map(String::from)).last()).flatten();
        Party::Chat { id: thread.to_string(), seat }
    }

    fn sent_count(&self, link: &str, thread: &str) -> usize {
        self.0.log.read(0).filter(|e| e.kind == "crew.sent" && e.thread.as_deref() == Some(thread) && e.data.get("link").and_then(Json::as_str) == Some(link)).count()
    }

    /// May `from` pass `carries` to `to` on `thread`'s count? `Ok(Some(link))` is the row it goes by; `Ok(None)`: the crew is off. Writes nothing.
    pub(super) fn link_admit(&self, from: &Party, to: &Party, carries: &str, thread: &str) -> Result<Option<String>, String> {
        if !self.links_hold() {
            return Ok(None);
        }
        self.0.cfg().crew.links.admit(from, to, carries, &|id| self.sent_count(id, thread)).map(|l| Some(l.id.clone())).map_err(|e| e.to_string())
    }

    /// The message went by `link`: under `declared` it is written, on the thread whose count it is.
    pub(super) fn link_sent(&self, link: Option<String>, from: &Party, to: &Party, carries: &str, thread: &str) {
        if let Some(link) = link.filter(|_| self.0.cfg().crew.links.declared()) {
            self.write(thread, "crew.sent", Actor::Core, json!({ "link": link, "carries": carries, "from": from.name(), "to": to.name() }));
        }
    }

    /// The refusal is written on `thread` and said there.
    pub(super) fn link_refused(&self, thread: &str, from: &Party, to: &Party, carries: &str, why: &str) {
        self.write(thread, "crew.refused", Actor::Core, json!({ "carries": carries, "from": from.name(), "to": to.name(), "why": why }));
        self.note(thread, &format!("a {carries} from {} to {} was not passed: {why}", from.name(), to.name()), "warn");
    }

    /// Admit, count and say: the message passes, or the reason is written and returned.
    pub(super) fn link_pass(&self, from: &Party, to: &Party, carries: &str, thread: &str) -> Result<(), String> {
        match self.link_admit(from, to, carries, thread) {
            Ok(link) => {
                self.link_sent(link, from, to, carries, thread);
                Ok(())
            }
            Err(why) => {
                self.link_refused(thread, from, to, carries, &why);
                Err(why)
            }
        }
    }

    // ---- opinions and what Main decided ----

    /// The opinions the Log holds, oldest first.
    fn opinions_given(&self) -> Vec<Given> {
        let s = |e: &crate::log::Ev, k: &str| e.data.get(k).and_then(Json::as_str).unwrap_or_default().to_string();
        self.0.log.read(0).filter(|e| e.kind == "crew.opinion").map(|e| Given { id: s(&e, "id"), chat: s(&e, "chat"), pick: s(&e, "pick"), why: s(&e, "why"), at_ms: e.at_ms }).collect()
    }

    /// The opinions no `crew.decided` has taken yet.
    fn opinions_open(&self) -> Vec<Given> {
        let taken: BTreeSet<String> = self.0.log.read(0).filter(|e| e.kind == "crew.decided").flat_map(|e| e.data.get("opinions").and_then(Json::as_array).cloned().unwrap_or_default()).filter_map(|o| o.get("id").and_then(Json::as_str).map(String::from)).collect();
        self.opinions_given().into_iter().filter(|o| !taken.contains(&o.id)).collect()
    }

    /// A chat's opinion reaches Main when a link carries it from that chat. True: it was written, and Main is woken for it.
    pub(super) fn opinion_take(&self, top: &str, chat: &str, op: &Opinion) -> bool {
        if !self.links_hold() || self.link_pass(&self.party(chat), &Party::Main, "opinion", chat).is_err() {
            return false;
        }
        let n = self.opinions_given().len() + 1;
        self.write(top, "crew.opinion", Actor::Core, json!({ "id": format!("o{n}"), "chat": chat, "pick": op.pick, "why": op.why }));
        true
    }

    /// Main's Brief names the opinions it has not decided on, as data, and how to say which one it followed.
    pub(super) fn opinions_section(&self) -> Option<String> {
        if !self.links_hold() {
            return None;
        }
        let open = self.opinions_open();
        if open.is_empty() {
            return None;
        }
        let rows: Vec<String> = open.iter().map(|o| format!("{} {}: {} · {}", o.id, o.chat, crate::text::line(&o.pick, 120), crate::text::line(&o.why, 400))).collect();
        Some(format!("Opinions of your chats on the request, as data (you decide):\n{}\nWhen you have decided, end your answer with a line `followed: o1` naming the opinion you went with, or `followed: none`.", rows.join("\n")))
    }

    /// Main's turn ended well with opinions in its hands: `crew.decided` names them and the one it followed.
    pub(super) fn decided_step(&self, thread: &str) {
        if !self.links_hold() || thread != self.0.cfg().thread {
            return;
        }
        let Some(row) = self.last_task(thread).filter(|r| r.state == crate::task::State::Done { ok: true } && r.ended_ms.is_some_and(|t| t >= self.0.since_ms)) else { return };
        let had: Vec<Given> = self.opinions_open().into_iter().filter(|o| row.started_ms.is_some_and(|t| o.at_ms <= t)).collect();
        if had.is_empty() || !self.once(&format!("decided:{}", row.id)) {
            return;
        }
        let said = row.answer.as_deref().unwrap_or_default().lines().rev().find_map(|l| l.trim().trim_start_matches(['*', '`', ' ']).to_lowercase().strip_prefix("followed:").map(|w| w.trim().trim_matches(['*', '`', '.', ' ']).to_string()));
        let followed = said.filter(|w| had.iter().any(|o| o.id == *w));
        self.write(thread, "crew.decided", Actor::Core, json!({ "of": row.id, "opinions": had.iter().map(|o| json!({ "id": o.id, "chat": o.chat, "pick": o.pick })).collect::<Vec<_>>(), "followed": followed }));
        let what = followed.as_ref().and_then(|f| had.iter().find(|o| o.id == *f)).map(|o| format!("followed {} ({}: {})", o.id, o.chat, crate::text::line(&o.pick, 80))).unwrap_or_else(|| "followed none of them".to_string());
        self.note(thread, &format!("decided · {} opinion{} · {what}", had.len(), if had.len() == 1 { "" } else { "s" }), "dim");
    }
}
