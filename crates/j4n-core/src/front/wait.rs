//! The questions that wait for the operator's next free line as a yes: one slot for each kind in a chat, its lapse data of the kind, its
//! words [`crate::binder::yes_no`]'s. The ask book, `route:<n>` and `mail:<n>` (a book with buttons and options) keep their own places; `mail:<n>` shares the words.

use super::Front;
use super::reflexes::PENDING_MS;
use std::collections::BTreeMap;

/// What waits: a reflex that throws context away, or the review of what was changed (offload).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Kind {
    Reflex,
    Review,
}

/// A question in a chat: what it is about, and what its owner needs to act on the answer (a reflex's ask, judge and model; a review's line).
pub(super) struct Pending {
    pub id: String,
    pub kind: Kind,
    pub at_ms: u64,
    pub with: Vec<String>,
}

impl Pending {
    /// Past its time: a reflex's question lapses after [`PENDING_MS`], a review's never.
    pub(super) fn lapsed(&self, now: u64) -> bool {
        self.kind == Kind::Reflex && now.saturating_sub(self.at_ms) > PENDING_MS
    }
}

pub(super) type Waits = BTreeMap<(String, Kind), Pending>;

impl Front {
    fn waits(&self) -> std::sync::MutexGuard<'_, Waits> {
        self.0.waits.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(super) fn waiting(&self, thread: &str, kind: Kind) -> bool {
        self.waits().contains_key(&(thread.into(), kind))
    }

    /// Put a question: `false` when one of its kind waits in the chat already.
    pub(super) fn wait_put(&self, thread: &str, p: Pending) -> bool {
        let (mut waits, key) = (self.waits(), (thread.to_string(), p.kind));
        let free = !waits.contains_key(&key);
        if free {
            waits.insert(key, p);
        }
        free
    }

    /// Take the question out: any line ends it, the answer or not.
    pub(super) fn wait_take(&self, thread: &str, kind: Kind) -> Option<Pending> {
        self.waits().remove(&(thread.into(), kind))
    }

    /// Whether the question is past its time, taken then, in one look: `None` when none waits.
    pub(super) fn wait_lapse(&self, thread: &str, kind: Kind, now: u64) -> Option<bool> {
        let mut waits = self.waits();
        let lapsed = waits.get(&(thread.into(), kind))?.lapsed(now);
        if lapsed {
            waits.remove(&(thread.into(), kind));
        }
        Some(lapsed)
    }
}
