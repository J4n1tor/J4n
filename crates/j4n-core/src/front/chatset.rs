//! A chat's own settings (`crew.on`): the layer of `crew::Layer` as the front writes and reads it. A write is checked (the class of the key, the list
//! `crew.chat_settings`, the value, who writes), planned on a copy of the layer and written to the Log as `chat.set` with its `Change`; the fold `threads` takes
//! the layer from those rows, so a restart finds it and an archive ends it. The turns of the chat read it here: its model and effort, the tool list and the
//! seat's words. Off, nothing here reads or writes anything.

use super::{Front, Pin};
use crate::crew::{self, Writer};
use crate::task::TaskSpec;
use serde_json::{Value as Json, json};

impl Front {
    /// Set `key` of `chat` as `by` (`Json::Null` leaves it out): the id of the change, or why not. The writer is the code path's, never a model's word.
    pub fn chat_set(&self, chat: &str, key: &str, value: &Json, by: &Writer) -> Result<String, String> {
        let crew = self.0.cfg().crew.clone();
        if !crew.on {
            return Err("the crew is off (`crew.on`): a chat has no settings of its own".into());
        }
        if self.chat(chat).is_none() {
            return Err(format!("there is no chat `{chat}`"));
        }
        if let Some(why) = crew::refusal(key, &crew.listed) {
            return Err(why);
        }
        let value = crew::value_of(key, value, crew.voice_max)?;
        if let (Some(toml::Value::String(word)), Some(bound)) = (&value, self.binding(chat))
            && key == "model"
        {
            self.pinned(&Pin { doer: None, model: Some(word.clone()) }, &bound, "chat")?;
        }
        let at = self.0.log.clock().now_ms();
        let Some((id, change)) = self.0.binds.layer().set(chat, key, value, by, at)? else { return Ok("already so".into()) };
        // a doer that takes no tool list cannot keep one: the row says so, and so does the chat
        let doer = self.binding(chat).map(|b| b.doer).unwrap_or_default();
        let kept = !key.starts_with("tools.") || self.0.doers.iter().any(|d| d.id() == doer && d.caps().tools);
        let data = json!({ "id": id, "chat": chat, "key": key, "change": change, "applied": kept, "why": if kept { Json::Null } else { json!(format!("{doer} takes no tool list")) } });
        if !self.write(chat, "chat.set", by.actor(), data) {
            return Err("the Log did not take the change".into());
        }
        if !kept {
            self.note(chat, &format!("`{key}` is written down, and not applied: {doer} takes no tool list"), "warn");
        }
        Ok(id)
    }

    /// Take back change `id` of a chat's settings as `by`; the id of the undo.
    pub fn chat_undo(&self, id: &str, by: &Writer) -> Result<String, String> {
        if !self.0.cfg().crew.on {
            return Err("the crew is off (`crew.on`)".into());
        }
        let at = self.0.log.clock().now_ms();
        let (new, change) = self.0.binds.layer().undo(id, by, at)?;
        let chat = change.target.id.split(':').next().unwrap_or_default().to_string();
        let key = change.target.id.split_once(':').map(|(_, k)| k.to_string()).unwrap_or_default();
        if !self.write(&chat, "chat.set", by.actor(), json!({ "id": new, "chat": chat, "key": key, "change": change, "undoes": id, "applied": true })) {
            return Err("the Log did not take the change".into());
        }
        Ok(new)
    }

    /// What `chat` holds of its own, `(setting, value)`; none while the crew is off.
    pub fn chat_layer(&self, chat: &str) -> Vec<(String, toml::Value)> {
        if self.0.cfg().crew.on { self.0.binds.chat_settings(chat) } else { Vec::new() }
    }

    /// `:undo cs3`: a change of a chat's settings, by the operator.
    pub(super) fn chat_undo_said(&self, thread: &str, id: &str) -> super::Said {
        match self.chat_undo(id, &Writer::Operator) {
            Ok(_) => {
                self.note(thread, &format!("took back {id}"), "dim");
                super::Said::Command("undo".into())
            }
            Err(why) => self.refuse(thread, why),
        }
    }

    /// `Intent::Set` of the kind `chat`: a setting of the chat `id`, as the operator's.
    pub(super) fn set_chat(&self, id: &str, field: &str, value: &Json) -> Result<String, String> {
        // a page row names a dotted setting as `c4.tools` and `allow`: the chat is what comes before the first dot, and a branch has no settings of its own
        let (id, key) = match id.split_once('.') {
            Some((chat, head)) => (chat, format!("{head}.{field}")),
            None => (id, field.to_string()),
        };
        let done = self.chat_set(id, &key, value, &Writer::Operator)?;
        Ok(format!("{id}: {key} set (`:undo {done}` takes it back)"))
    }

    /// The rows of the page Tasks › Chat settings: each setting a chat may hold, what it holds and who set it last, with the editor and the undo of the change. The crew
    /// off, or no chat of its own, none.
    pub fn chatset_values(&self, chat: &str) -> Vec<crate::value::Value> {
        use crate::value::Value;
        let crew = self.0.cfg().crew.clone();
        if !crew.on || self.chat(chat).is_none_or(|c| c.parent.is_some()) {
            return Vec::new();
        }
        let held = self.0.binds.chat_settings(chat);
        let by = self.0.binds.layer().held_by(chat);
        crew::ACTING
            .iter()
            .filter(|k| crew.listed.iter().any(|l| l == *k))
            .map(|key| {
                let now = held.iter().find(|(h, _)| h == key).map(|(_, v)| v.clone());
                let shown = match &now {
                    Some(toml::Value::Array(a)) => a.iter().filter_map(toml::Value::as_str).collect::<Vec<_>>().join(", "),
                    Some(v) => v.as_str().map(String::from).unwrap_or_else(|| v.to_string()),
                    None => String::new(),
                };
                let (head, tail) = key.split_once('.').unwrap_or((key, ""));
                let bind = if tail.is_empty() { format!("chat:{chat}.{key}") } else { format!("chat:{chat}.{head}.{tail}") };
                let mut edit = std::collections::BTreeMap::from([("kind".to_string(), Value::text(if *key == "effort" { "enum" } else { "str" })), ("bind".into(), Value::text(bind)), ("value".into(), Value::text(&shown))]);
                if *key == "effort" {
                    edit.insert("options".into(), Value::List(crew::EFFORTS.iter().map(|e| Value::text(*e)).collect()));
                }
                let doc = match *key {
                    "model" => "the model this chat's own turns use",
                    "effort" => "how hard this chat's doer thinks",
                    "tools.allow" => "the tools this chat is limited to; only narrower than what it holds",
                    "tools.deny" => "tools this chat is kept from",
                    _ => "the words this chat works by",
                };
                Value::Map(std::collections::BTreeMap::from([
                    ("id".to_string(), Value::text(*key)),
                    ("chat".into(), Value::text(chat)),
                    ("now".into(), Value::text(if shown.is_empty() { "not set" } else { &shown })),
                    ("by".into(), Value::text(by.get(*key).map(|(_, w)| w.as_str()).unwrap_or_default())),
                    ("undo".into(), Value::text(by.get(*key).map(|(id, _)| id.as_str()).unwrap_or_default())),
                    ("doc".into(), Value::text(doc)),
                    ("edit".into(), Value::Map(edit)),
                ]))
            })
            .collect()
    }

    /// What a work turn of `thread` takes from its chat's settings: the model and effort on the spec, the tool list in `input.tools`, and the seat's words,
    /// which are returned. Only the chat's own work turns: a chore, a check or a planning turn on the thread is not the chat's work.
    pub fn chat_apply(&self, thread: &str, spec: &mut TaskSpec) -> String {
        let crew = &self.0.cfg().crew;
        if !crew.on || spec.role.as_deref() != Some("work") || spec.input.get("branch").is_some() || spec.input.get("bench").is_some() {
            return String::new();
        }
        let held = self.0.binds.chat_settings(thread);
        let word = |k: &str| held.iter().find(|(h, _)| h == k).and_then(|(_, v)| v.as_str().map(String::from));
        let list = |k: &str| -> Vec<String> { held.iter().find(|(h, _)| h == k).and_then(|(_, v)| v.as_array()).into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect() };
        if let (Some(model), Some(bound)) = (word("model"), self.binding(thread))
            && let Ok(b) = self.pinned(&Pin { doer: None, model: Some(model) }, &bound, "chat")
        {
            (spec.tune.model, spec.tune.endpoint) = (b.model, b.endpoint);
        }
        if let Some(effort) = word("effort") {
            spec.tune.effort = Some(effort);
        }
        let (allow, deny) = (list("tools.allow"), list("tools.deny"));
        if !allow.is_empty() || !deny.is_empty() {
            spec.input["tools"] = json!({ "allow": allow, "deny": deny });
        }
        word("personality").unwrap_or_default()
    }

    /// The seat's words, the tool lists and the settings Main wrote are the first writes of a chat that was started from a brief. A refused one is said in the
    /// chat and the chat starts without it.
    pub(super) fn chat_seated(&self, chat: &str, b: &super::split::Brief) {
        let write = |key: &str, value: Json, by: &Writer| {
            if let Err(why) = self.chat_set(chat, key, &value, by) {
                self.note(chat, &format!("setting `{key}` was not set: {why}"), "warn");
            }
        };
        if let Some(seat) = &b.seat {
            write("personality", json!(seat.voice), &Writer::Seat);
            // a seat that names no tools narrows nothing: an empty list is not written (it would read as leaving the key out)
            if !seat.tools_allow.is_empty() {
                write("tools.allow", json!(seat.tools_allow), &Writer::Seat);
            }
            if !seat.tools_deny.is_empty() {
                write("tools.deny", json!(seat.tools_deny), &Writer::Seat);
            }
        }
        for (key, value) in &b.settings {
            write(key, value.clone(), &Writer::Main);
        }
    }
}
