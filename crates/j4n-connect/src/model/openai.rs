//! The `openai` dialect: chat completions, as OpenAI, OpenRouter and every compatible gateway speak them. Streaming, `tools`
//! (native), reasoning in either spelling, a host preference and a session id for gateways that route.
//!
//! The request carries `messages` (the assistant's calls as `tool_calls`, a call's result as a `tool` message naming it), the
//! functions on offer in `tools` with `tool_choice: auto`, and `max_tokens`. A thinking model's reasoning goes back on the
//! assistant message that made a call, as `reasoning`. `provider.order` names the host and `allow_fallbacks: false` keeps the
//! gateway from going to another, when a host is elected; `session_id` goes whenever there is one.
//!
//! The stream is read frame by frame: text from `delta.content`; reasoning from `delta.reasoning` or `delta.reasoning_content`
//! (a frame that carries only `reasoning_details` adds nothing, or the same words would count twice); a call's fragments by
//! their `index`, the name once, the arguments in order; the last frame with `usage` gives the tokens, the cached part and the
//! cost; `provider` names who served it.

use super::{Call, Delta, Fail, Finish, Msg, Reply, Req, Think, Usage, Wire, frames, thought_back};
use crate::net::NetError;
use crate::opening::target_in;
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

pub struct Openai;

/// The conversation in the dialect's words.
fn messages(req: &Req) -> Vec<Json> {
    let mut out = Vec::new();
    if !req.system.is_empty() {
        out.push(json!({ "role": "system", "content": req.system }));
    }
    for m in thought_back(req.msgs) {
        out.push(match m {
            Msg::User(t) => json!({ "role": "user", "content": t }),
            Msg::Result { call, text, .. } => json!({ "role": "tool", "tool_call_id": call, "content": text }),
            Msg::Assistant { text, think, calls } => {
                let mut v = json!({ "role": "assistant", "content": if text.is_empty() && !calls.is_empty() { Json::Null } else { Json::String(text) } });
                if !calls.is_empty() {
                    v["tool_calls"] = calls
                        .iter()
                        .map(|c| json!({ "id": c.id, "type": "function", "function": { "name": c.name, "arguments": c.args.to_string() } }))
                        .collect();
                }
                let thought: String = think.iter().map(|t| t.text.as_str()).collect();
                if !thought.is_empty() {
                    v["reasoning"] = Json::String(thought);
                }
                v
            }
        });
    }
    out
}

impl Wire for Openai {
    fn url(&self, base: &str) -> String {
        format!("{base}/chat/completions")
    }

    fn headers(&self, key: &str) -> Vec<(String, String)> {
        let mut h = vec![("Accept".to_string(), "text/event-stream".to_string())];
        if !key.is_empty() {
            h.push(("Authorization".into(), format!("Bearer {key}")));
        }
        h
    }

    fn body(&self, req: &Req) -> Json {
        let mut v = json!({
            "model": req.model,
            "stream": true,
            "stream_options": { "include_usage": true },
            "max_tokens": req.max,
            "messages": messages(req),
        });
        if !req.tools.is_empty() {
            v["tools"] = req.tools.iter().map(|t| json!({ "type": "function", "function": { "name": t.name, "description": t.doc, "parameters": t.schema } })).collect();
            v["tool_choice"] = json!("auto");
        }
        if let Some(e) = req.effort {
            v["reasoning"] = json!({ "effort": e });
        }
        if let Some(h) = req.host {
            v["provider"] = json!({ "order": [h], "allow_fallbacks": false });
        }
        if let Some(s) = req.session {
            v["session_id"] = json!(s);
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
            if let Some(e) = v.get("error").filter(|e| !e.is_null()) {
                let said = e.get("message").and_then(Json::as_str).map(str::to_string).unwrap_or_else(|| e.to_string());
                return Err(match e.get("code").and_then(Json::as_u64).and_then(|c| u16::try_from(c).ok()) {
                    Some(code) if code >= 400 => Fail::Status { code, body: said, retry_after: None },
                    _ => Fail::Stream(said),
                });
            }
            st.take(&v, on, &started);
        }
        Ok(st.finish(started))
    }
}

#[derive(Default)]
struct Part {
    id: String,
    name: String,
    args: String,
}

#[derive(Default)]
struct Stream {
    text: String,
    reasoning: String,
    calls: BTreeMap<u64, Part>,
    finish: Option<String>,
    usage: Usage,
    served: Option<String>,
    model: Option<String>,
    ttft: Option<u64>,
}

/// The reasoning words of a delta, from either field. `reasoning_details` is the same words again, structured.
fn reasoning_of(delta: &Json) -> Option<&str> {
    ["reasoning", "reasoning_content"].into_iter().find_map(|k| delta.get(k).and_then(Json::as_str)).filter(|s| !s.is_empty())
}

impl Stream {
    fn first(&mut self, started: &Instant) {
        self.ttft.get_or_insert(started.elapsed().as_millis() as u64);
    }

    fn take(&mut self, v: &Json, on: &mut dyn FnMut(Delta), started: &Instant) {
        if let Some(p) = v.get("provider").and_then(Json::as_str).filter(|s| !s.is_empty()) {
            self.served = Some(p.to_string());
        }
        if let Some(m) = v.get("model").and_then(Json::as_str).filter(|s| !s.is_empty()) {
            self.model = Some(m.to_string());
        }
        if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
            self.usage = usage_of(u);
            if self.usage.output > 0 {
                on(Delta::Tokens(self.usage.output));
            }
        }
        let Some(choice) = v.pointer("/choices/0") else { return };
        if let Some(f) = choice.get("finish_reason").and_then(Json::as_str) {
            self.finish = Some(f.to_string());
        }
        let Some(delta) = choice.get("delta") else { return };
        if let Some(t) = delta.get("content").and_then(Json::as_str).filter(|s| !s.is_empty()) {
            self.first(started);
            self.text.push_str(t);
            on(Delta::Text(t.into()));
        }
        if let Some(r) = reasoning_of(delta) {
            self.first(started);
            self.reasoning.push_str(r);
            on(Delta::Reasoning(r.into()));
        }
        for frag in delta.get("tool_calls").and_then(Json::as_array).into_iter().flatten() {
            self.first(started);
            let part = self.calls.entry(frag.get("index").and_then(Json::as_u64).unwrap_or(0)).or_default();
            if let Some(id) = frag.get("id").and_then(Json::as_str).filter(|s| !s.is_empty()) {
                part.id = id.into();
            }
            let f = frag.get("function");
            // a gateway that names the function in every fragment must not make it `readread`
            if let Some(n) = f.and_then(|f| f.get("name")).and_then(Json::as_str).filter(|s| !s.is_empty())
                && part.name.is_empty()
            {
                part.name = n.into();
            }
            if let Some(a) = f.and_then(|f| f.get("arguments")).and_then(Json::as_str) {
                part.args.push_str(a);
            }
            if !part.name.is_empty() {
                on(Delta::Call { id: frag.get("index").and_then(Json::as_u64).unwrap_or(0).to_string(), name: part.name.clone(), target: target_in(&part.args), bytes: part.args.len() as u64 });
            }
        }
    }

    fn finish(self, started: Instant) -> Reply {
        let cut = self.finish.as_deref() == Some("length");
        let calls: Vec<Call> = self
            .calls
            .into_iter()
            .filter(|(_, p)| !p.name.is_empty())
            .map(|(i, p)| Call::from_stream(&if p.id.is_empty() { format!("call_{i}") } else { p.id }, &p.name, &p.args, cut))
            .collect();
        let finish = self.finish.map(|f| match f.as_str() {
            "stop" => Finish::Stop,
            "length" => Finish::Length,
            "tool_calls" | "function_call" => Finish::Calls,
            other => Finish::Other(other.into()),
        });
        let think = if self.reasoning.is_empty() { Vec::new() } else { vec![Think { text: self.reasoning, sig: String::new() }] };
        Reply { text: self.text, think, calls, finish, usage: self.usage, served: self.served, model: self.model, ttft_ms: self.ttft, ms: started.elapsed().as_millis() as u64 }
    }
}

/// The tokens of a `usage` object. The cached part is named differently by different gateways.
fn usage_of(u: &Json) -> Usage {
    let n = |ptr: &str| u.pointer(ptr).and_then(Json::as_u64);
    Usage {
        input: n("/prompt_tokens").unwrap_or(0),
        output: n("/completion_tokens").unwrap_or(0),
        cached: n("/prompt_tokens_details/cached_tokens").or_else(|| n("/prompt_cache_hit_tokens")).or_else(|| n("/cache_read_input_tokens")).unwrap_or(0),
        wrote: n("/prompt_tokens_details/cache_write_tokens").or_else(|| n("/cache_creation_input_tokens")).unwrap_or(0),
        usd: u.get("cost").and_then(Json::as_f64),
    }
}
