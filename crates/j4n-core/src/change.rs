//! Who did a thing and what it changed: the value types behind every Table write and every undo.
//! A [`Change`] is plain data — it serializes to the Log and its [`inverse`](Change::inverse) is
//! another `Change`, so an undo works across restarts.

use serde::{Deserialize, Serialize};
use toml::Table;

/// The six kinds of actor, without the name. A schema field's `who_may` lists these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActorKind {
    Operator,
    Core,
    Rule,
    Doer,
    Model,
    Pack,
}

impl ActorKind {
    pub const ALL: [ActorKind; 6] =
        [ActorKind::Operator, ActorKind::Core, ActorKind::Rule, ActorKind::Doer, ActorKind::Model, ActorKind::Pack];

    pub fn word(self) -> &'static str {
        match self {
            ActorKind::Operator => "operator",
            ActorKind::Core => "core",
            ActorKind::Rule => "rule",
            ActorKind::Doer => "doer",
            ActorKind::Model => "model",
            ActorKind::Pack => "pack",
        }
    }

    pub fn from_word(word: &str) -> Option<ActorKind> {
        ActorKind::ALL.into_iter().find(|k| k.word() == word)
    }
}

/// Who an event or an action is on behalf of (provenance).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Actor {
    Operator,
    Core,
    Rule(String),
    Doer(String),
    Model(String),
    Pack(String),
}

impl Actor {
    pub fn kind(&self) -> ActorKind {
        match self {
            Actor::Operator => ActorKind::Operator,
            Actor::Core => ActorKind::Core,
            Actor::Rule(_) => ActorKind::Rule,
            Actor::Doer(_) => ActorKind::Doer,
            Actor::Model(_) => ActorKind::Model,
            Actor::Pack(_) => ActorKind::Pack,
        }
    }
}

/// What a change followed from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Cause {
    /// The Log event with this sequence number.
    Event(u64),
    /// The proposal the operator accepted.
    Proposal(String),
    /// The change this one takes back.
    Undo(String),
    /// The operator's own edit with this id (`e3`): applied at once, never a proposal; its changes are undone together.
    Edit(String),
}

/// The layers a write can reach. A pack is never written: it is installed, not edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Where {
    /// `~/.j4n/`
    User,
    /// `<project>/.j4n/`
    Project,
    /// In memory, until the process ends.
    Session,
    /// `~/.j4n/` itself, the layer every project reads below its own (`harness.toml` there turns a pack on for all of them).
    Home,
}

impl Where {
    pub fn word(self) -> &'static str {
        match self {
            Where::User => "home",
            Where::Project => "project",
            Where::Session => "session",
            Where::Home => "every project",
        }
    }
}

/// One thing in one layer: `file` is where it is written, relative to the layer (empty for the session).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub layer: Where,
    pub file: String,
    pub kind: String,
    pub id: String,
}

/// One inverse-able record behind every accept and every undo: the entry as the layer held it (`before`) and as it holds
/// it after (`after`), without its `id`; `None` is no entry. Applying checks that the layer still holds `before`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub id: String,
    pub target: Target,
    pub before: Option<Table>,
    pub after: Option<Table>,
    pub by: Actor,
    pub cause: Option<Cause>,
    pub at_ms: u64,
}

impl Change {
    /// A change with its id made from everything else in it, so the same edit at another moment is another change.
    pub(crate) fn sealed(
        target: Target,
        before: Option<Table>,
        after: Option<Table>,
        by: Actor,
        cause: Option<Cause>,
        at_ms: u64,
    ) -> Change {
        let seed = format!("{at_ms}|{target:?}|{before:?}|{after:?}|{by:?}|{cause:?}");
        let id = digest(&seed)[..16].to_string();
        Change { id, target, before, after, by, cause, at_ms }
    }

    /// The change that takes this one back: the same thing, `after` and `before` swapped.
    pub fn inverse(&self, by: Actor, at_ms: u64) -> Change {
        Change::sealed(
            self.target.clone(),
            self.after.clone(),
            self.before.clone(),
            by,
            Some(Cause::Undo(self.id.clone())),
            at_ms,
        )
    }
}

/// The SHA-256 of `text`, in lowercase hex.
pub fn digest(text: &str) -> String {
    crate::text::hex_hash(text)
}
