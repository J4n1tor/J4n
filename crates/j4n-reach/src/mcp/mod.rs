//! The dialect `mcp`: the Model Context Protocol, JSON-RPC 2.0,
//! over a program's stdio ([`stdio`]) or over HTTP, answered as JSON or as a stream of events ([`http`]). A client starts the
//! conversation (`initialize`, `notifications/initialized`), lists the tools, and calls them.
//!
//! Nothing a server says is trusted: its tool list is hashed ([`pin`]) and a tool is offered to a model only while the hash is the
//! one the operator accepted; a call is checked against the schema the server gave ([`validate`]) and classified for the Gate
//! ([`classify`]); what comes back is words for the model to read, cut to a size.

pub mod classify;
pub mod http;
pub mod stdio;
pub mod tools;
pub mod validate;

use serde_json::{Value, json};
use std::time::Duration;

pub const PROTOCOL: &str = "2025-03-26";
/// How long a server is given to answer the handshake and a list; a call is given longer.
pub const SETUP: Duration = Duration::from_secs(30);
pub const CALL: Duration = Duration::from_secs(120);
const PAGES: usize = 20;
const TOOLS_MAX: usize = 200;
const RESULT_CHARS: usize = 20_000;

/// One way to talk JSON-RPC to a server.
pub trait Transport: Send + Sync {
    /// A request and its result (the `result` of the answer), or the words of why not (a transport failure, or the server's
    /// `error`).
    fn call(&self, method: &str, params: Value, wait: Duration) -> Result<Value, String>;
    fn notify(&self, method: &str, params: Value);
    /// Let go of the server.
    fn close(&self);
}

#[derive(Debug, Clone, PartialEq)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub schema: Value,
    pub annotations: Value,
}

impl McpTool {
    /// What the operator pins: the name, the words that tell a model what the tool is for, and the shape of its arguments.
    pub fn hash(&self) -> String {
        let canon = json!({ "name": self.name, "description": self.description, "inputSchema": self.schema }).to_string();
        j4n_core::text::short_hash(canon)
    }
}

/// What a call came to.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub text: String,
    pub error: bool,
}

pub struct Client {
    t: Box<dyn Transport>,
    pub server: String,
}

impl Client {
    /// The handshake. A server that answers anything but a result is not one.
    pub fn start(t: Box<dyn Transport>) -> Result<Client, String> {
        let init = t.call("initialize", json!({ "protocolVersion": PROTOCOL, "capabilities": {}, "clientInfo": { "name": "j4n", "version": env!("CARGO_PKG_VERSION") } }), SETUP).map_err(|e| format!("the server did not start: {e}"))?;
        let server = init.pointer("/serverInfo/name").and_then(Value::as_str).unwrap_or("server").to_string();
        t.notify("notifications/initialized", json!({}));
        Ok(Client { t, server })
    }

    pub fn tools(&self) -> Result<Vec<McpTool>, String> {
        let (mut out, mut cursor) = (Vec::new(), None::<String>);
        for _ in 0..PAGES {
            let params = cursor.as_ref().map_or_else(|| json!({}), |c| json!({ "cursor": c }));
            let page = self.t.call("tools/list", params, SETUP)?;
            for t in page.get("tools").and_then(Value::as_array).into_iter().flatten() {
                let Some(name) = t.get("name").and_then(Value::as_str).filter(|n| !n.is_empty()) else { continue };
                out.push(McpTool {
                    name: name.into(),
                    description: t.get("description").and_then(Value::as_str).unwrap_or_default().into(),
                    schema: t.get("inputSchema").cloned().unwrap_or_else(|| json!({ "type": "object" })),
                    annotations: t.get("annotations").cloned().unwrap_or(Value::Null),
                });
            }
            cursor = page.get("nextCursor").and_then(Value::as_str).map(String::from);
            if cursor.is_none() || out.len() >= TOOLS_MAX {
                break;
            }
        }
        out.truncate(TOOLS_MAX);
        Ok(out)
    }

    pub fn call(&self, name: &str, args: &Value) -> Result<Outcome, String> {
        let result = self.t.call("tools/call", json!({ "name": name, "arguments": args }), CALL)?;
        let mut parts = Vec::new();
        for c in result.get("content").and_then(Value::as_array).into_iter().flatten() {
            match c.get("type").and_then(Value::as_str) {
                Some("text") => parts.push(c.get("text").and_then(Value::as_str).unwrap_or_default().to_string()),
                Some("resource") => parts.push(c.pointer("/resource/text").and_then(Value::as_str).map_or_else(|| format!("(a resource: {})", c.pointer("/resource/uri").and_then(Value::as_str).unwrap_or("?")), String::from)),
                Some(other) => parts.push(format!("({other} content, not shown)")),
                None => {}
            }
        }
        if parts.is_empty()
            && let Some(s) = result.get("structuredContent")
        {
            parts.push(s.to_string());
        }
        let text: String = parts.join("\n");
        let cut = if text.chars().count() > RESULT_CHARS { format!("{}…\n(cut at {RESULT_CHARS} characters)", text.chars().take(RESULT_CHARS).collect::<String>()) } else { text };
        Ok(Outcome { text: cut, error: result.get("isError").and_then(Value::as_bool).unwrap_or(false) })
    }

    pub fn close(&self) {
        self.t.close();
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.t.close();
    }
}
