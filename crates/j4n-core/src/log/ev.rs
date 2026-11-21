//! The event: one line of a segment, and what a caller hands the Log to make one.

use crate::change::Actor;
use crate::schema::DataClass;
use crate::value::Value;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::collections::BTreeMap;

/// A kind is lowercase words joined by dots: `run.message`, `task.settled`, `shape.result`.
pub const KIND_MAX: usize = 64;

/// Kinds that are only ever flashed: a streaming delta or a heartbeat is never written.
pub const FLASH_KINDS: [&str; 5] = ["run.delta", "run.reasoning_delta", "run.busy", "run.open", "run.tokens"];

/// Kinds that are always written, never flashed: the settled message, the reasoning that came with a call, every tool
/// call and its result, every shell exit. The own agent's thread is rebuilt from them.
pub const SETTLED_KINDS: [&str; 5] = ["run.message", "run.reasoning", "run.tool_call", "run.tool_result", "run.shell"];

pub fn is_kind(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= KIND_MAX
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.split('.').all(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
}

/// One event, as the segment holds it. Reading is forward-compatible: a field this binary does not know is
/// ignored, a field a newer writer left out takes its default, and an event kind nobody here knows is still an event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ev {
    /// 1, 2, 3 … in the order written, over the whole log; 0 is a flash, which is not written.
    pub seq: u64,
    pub at_ms: u64,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    pub by: Actor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<u64>,
    #[serde(default)]
    pub class: DataClass,
    #[serde(default, skip_serializing_if = "Json::is_null")]
    pub data: Json,
}

/// An actor as one word for Expr and globs: `operator`, `core`, `rule:followups`, `doer:claude`.
pub fn actor_word(a: &Actor) -> String {
    match a {
        Actor::Operator => "operator".into(),
        Actor::Core => "core".into(),
        Actor::Rule(id) => format!("rule:{id}"),
        Actor::Doer(id) => format!("doer:{id}"),
        Actor::Model(id) => format!("model:{id}"),
        Actor::Pack(id) => format!("pack:{id}"),
    }
}

impl Ev {
    pub fn is_flash(&self) -> bool {
        self.seq == 0
    }

    /// The event as Expr reads it: `seq at_ms kind task thread by cause class data`. `by` is [`actor_word`].
    pub fn row(&self) -> Value {
        let mut m = BTreeMap::new();
        m.insert("seq".to_string(), Value::num(self.seq as f64));
        m.insert("at_ms".to_string(), Value::num(self.at_ms as f64));
        m.insert("kind".to_string(), Value::text(&self.kind));
        if let Some(t) = &self.task {
            m.insert("task".to_string(), Value::text(t));
        }
        if let Some(t) = &self.thread {
            m.insert("thread".to_string(), Value::text(t));
        }
        m.insert("by".to_string(), Value::text(actor_word(&self.by)));
        if let Some(c) = self.cause {
            m.insert("cause".to_string(), Value::num(c as f64));
        }
        m.insert("class".to_string(), Value::text(self.class.word()));
        m.insert("data".to_string(), Value::from_json(&self.data));
        Value::Map(m)
    }

    /// The event as a screen is sent it to show in its own look: the fields of [`row`](Ev::row), as JSON.
    pub fn shown(&self) -> Json {
        let mut m = serde_json::Map::new();
        m.insert("seq".into(), self.seq.into());
        m.insert("at_ms".into(), self.at_ms.into());
        m.insert("kind".into(), self.kind.clone().into());
        for (k, v) in [("task", &self.task), ("thread", &self.thread)] {
            if let Some(v) = v {
                m.insert(k.into(), v.clone().into());
            }
        }
        m.insert("by".into(), actor_word(&self.by).into());
        if let Some(c) = self.cause {
            m.insert("cause".into(), c.into());
        }
        m.insert("class".into(), self.class.word().into());
        if !self.data.is_null() {
            m.insert("data".into(), self.data.clone());
        }
        Json::Object(m)
    }
}

/// What a caller gives the Log to append or flash. The Log adds the sequence number and the time.
#[derive(Debug, Clone, PartialEq)]
pub struct New {
    pub kind: String,
    pub task: Option<String>,
    pub thread: Option<String>,
    pub by: Actor,
    pub cause: Option<u64>,
    /// None takes the kind's declared class, else `project`.
    pub class: Option<DataClass>,
    pub data: Json,
}

impl New {
    pub fn of(kind: &str, by: Actor, data: Json) -> New {
        New { kind: kind.to_string(), task: None, thread: None, by, cause: None, class: None, data }
    }

    pub fn task(mut self, id: &str) -> New {
        self.task = Some(id.to_string());
        self
    }

    pub fn thread(mut self, id: &str) -> New {
        self.thread = Some(id.to_string());
        self
    }

    pub fn cause(mut self, seq: u64) -> New {
        self.cause = Some(seq);
        self
    }

    pub fn class(mut self, class: DataClass) -> New {
        self.class = Some(class);
        self
    }
}
