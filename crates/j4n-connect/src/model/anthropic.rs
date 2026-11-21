//! The `anthropic` dialect: the Messages API. The same promises as `openai` (native tools, every call of a reply, reasoning
//! sent back, room to write), in its own words: `system` apart from the messages, content in blocks (`text`, `thinking` with
//! the `signature` that must go back with it, `tool_use`, and a call's result as a `tool_result` block of a user message), and a
//! stream of named events (`message_start` brings the input tokens and the cache's part of them, `content_block_start` /
//! `content_block_delta` build each block by its index, `message_delta` brings the stop reason and the output tokens).

use super::{Call, Delta, Fail, Finish, Msg, Reply, Req, Think, Usage, Wire, frames, thought_back};
use crate::net::NetError;
use crate::opening::target_in;
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

pub struct Anthropic;

/// How much a model that reasons may think, for each effort.
fn budget(effort: &str, max: u32) -> u32 {
    let b = match effort {
        "low" => 2048,
        "high" => 16384,
        _ => 8192,
    };
    b.min(max.saturating_sub(1024)).max(1024)
}

fn messages(req: &Req) -> Vec<Json> {
    let mut out: Vec<Json> = Vec::new();
    // results of one assistant message's calls travel together, in one user message
    let push_user = |out: &mut Vec<Json>, block: Json| match out.last_mut().filter(|m| m["role"] == "user") {
        Some(last) => last["content"].as_array_mut().into_iter().for_each(|c| c.push(block.clone())),
        None => out.push(json!({ "role": "user", "content": [block] })),
    };
    for m in thought_back(req.msgs) {
        match m {
            Msg::User(t) => push_user(&mut out, json!({ "type": "text", "text": t })),
            Msg::Result { call, text, error, .. } => {
                let mut b = json!({ "type": "tool_result", "tool_use_id": call, "content": text });
                if error {
                    b["is_error"] = json!(true);
                }
                push_user(&mut out, b);
            }
            Msg::Assistant { text, think, calls } => {
                let mut blocks: Vec<Json> = think.iter().filter(|t| !t.sig.is_empty()).map(|t| json!({ "type": "thinking", "thinking": t.text, "signature": t.sig })).collect();
                if !text.is_empty() {
                    blocks.push(json!({ "type": "text", "text": text }));
                }
                blocks.extend(calls.iter().map(|c| json!({ "type": "tool_use", "id": c.id, "name": c.name, "input": c.args })));
                out.push(json!({ "role": "assistant", "content": blocks }));
            }
        }
    }
    out
}

impl Wire for Anthropic {
    fn url(&self, base: &str) -> String {
        format!("{base}/v1/messages")
    }

    fn headers(&self, key: &str) -> Vec<(String, String)> {
        vec![("x-api-key".into(), key.into()), ("anthropic-version".into(), "2023-06-01".into()), ("Accept".into(), "text/event-stream".into())]
    }

    fn body(&self, req: &Req) -> Json {
        let mut v = json!({ "model": req.model, "stream": true, "max_tokens": req.max, "messages": messages(req) });
        if !req.system.is_empty() {
            v["system"] = json!(req.system);
        }
        if !req.tools.is_empty() {
            v["tools"] = req.tools.iter().map(|t| json!({ "name": t.name, "description": t.doc, "input_schema": t.schema })).collect();
        }
        if let Some(e) = req.effort {
            v["thinking"] = json!({ "type": "enabled", "budget_tokens": budget(e, req.max) });
        }
        v
    }

    fn read(&self, lines: &mut dyn Iterator<Item = Result<String, NetError>>, on: &mut dyn FnMut(Delta), cancel: &AtomicBool) -> Result<Reply, Fail> {
        let started = Instant::now();
        let mut st = Stream::default();
        for frame in frames(lines) {
            if cancel.load(Ordering::SeqCst) {
                return Err(Fail::Cancelled);
            }
            let frame = frame.map_err(|e| Fail::Stream(e.0))?;
            let Ok(v) = serde_json::from_str::<Json>(&frame) else { continue };
            if v["type"] == "error" {
                let kind = v.pointer("/error/type").and_then(Json::as_str).unwrap_or_default();
                let said = v.pointer("/error/message").and_then(Json::as_str).unwrap_or("the stream failed").to_string();
                return Err(match kind {
                    "overloaded_error" => Fail::Status { code: 529, body: said, retry_after: None },
                    "rate_limit_error" => Fail::Status { code: 429, body: said, retry_after: None },
                    _ => Fail::Stream(said),
                });
            }
            st.take(&v, on, &started);
        }
        Ok(st.finish(started))
    }
}

#[derive(Default)]
struct Block {
    kind: String,
    id: String,
    name: String,
    text: String,
    sig: String,
}

#[derive(Default)]
struct Stream {
    blocks: BTreeMap<u64, Block>,
    stop: Option<String>,
    usage: Usage,
    model: Option<String>,
    ttft: Option<u64>,
}

impl Stream {
    fn take(&mut self, v: &Json, on: &mut dyn FnMut(Delta), started: &Instant) {
        let n = |ptr: &str| v.pointer(ptr).and_then(Json::as_u64);
        match v["type"].as_str().unwrap_or_default() {
            "message_start" => {
                self.model = v.pointer("/message/model").and_then(Json::as_str).map(str::to_string);
                let (fresh, read, wrote) = (n("/message/usage/input_tokens").unwrap_or(0), n("/message/usage/cache_read_input_tokens").unwrap_or(0), n("/message/usage/cache_creation_input_tokens").unwrap_or(0));
                self.usage = Usage { input: fresh + read + wrote, output: n("/message/usage/output_tokens").unwrap_or(0), cached: read, wrote, usd: None };
            }
            "content_block_start" => {
                let b = &v["content_block"];
                let s = |k: &str| b.get(k).and_then(Json::as_str).unwrap_or_default().to_string();
                let at = v["index"].as_u64().unwrap_or(0);
                self.blocks.insert(at, Block { kind: s("type"), id: s("id"), name: s("name"), ..Block::default() });
                if b["type"] == "tool_use" {
                    self.ttft.get_or_insert(started.elapsed().as_millis() as u64);
                    on(Delta::Call { id: at.to_string(), name: s("name"), target: String::new(), bytes: 0 });
                }
            }
            "content_block_delta" => {
                let d = &v["delta"];
                let Some(b) = self.blocks.get_mut(&v["index"].as_u64().unwrap_or(0)) else { return };
                let piece = |k: &str| d.get(k).and_then(Json::as_str).unwrap_or_default().to_string();
                match d["type"].as_str().unwrap_or_default() {
                    "text_delta" => {
                        let t = piece("text");
                        self.ttft.get_or_insert(started.elapsed().as_millis() as u64);
                        b.text.push_str(&t);
                        on(Delta::Text(t));
                    }
                    "thinking_delta" => {
                        let t = piece("thinking");
                        self.ttft.get_or_insert(started.elapsed().as_millis() as u64);
                        b.text.push_str(&t);
                        on(Delta::Reasoning(t));
                    }
                    "input_json_delta" => {
                        b.text.push_str(&piece("partial_json"));
                        on(Delta::Call { id: v["index"].as_u64().unwrap_or(0).to_string(), name: b.name.clone(), target: target_in(&b.text), bytes: b.text.len() as u64 });
                    }
                    "signature_delta" => b.sig.push_str(&piece("signature")),
                    _ => {}
                }
            }
            "message_delta" => {
                if let Some(s) = v.pointer("/delta/stop_reason").and_then(Json::as_str) {
                    self.stop = Some(s.into());
                }
                if let Some(o) = n("/usage/output_tokens") {
                    self.usage.output = o;
                    on(Delta::Tokens(o));
                }
            }
            _ => {}
        }
    }

    fn finish(self, started: Instant) -> Reply {
        let cut = self.stop.as_deref() == Some("max_tokens");
        let (mut text, mut think, mut calls) = (String::new(), Vec::new(), Vec::new());
        for b in self.blocks.into_values() {
            match b.kind.as_str() {
                "text" => text.push_str(&b.text),
                "thinking" => think.push(Think { text: b.text, sig: b.sig }),
                "tool_use" if !b.name.is_empty() => calls.push(Call::from_stream(&b.id, &b.name, &b.text, cut)),
                _ => {}
            }
        }
        let finish = self.stop.map(|s| match s.as_str() {
            "end_turn" | "stop_sequence" => Finish::Stop,
            "max_tokens" => Finish::Length,
            "tool_use" => Finish::Calls,
            other => Finish::Other(other.into()),
        });
        Reply { text, think, calls, finish, usage: self.usage, served: None, model: self.model, ttft_ms: self.ttft, ms: started.elapsed().as_millis() as u64 }
    }
}
