//! MCP over HTTP (the streamable transport): every message is a POST of one JSON-RPC document, and the answer is that document's
//! result as JSON, or as a stream of server-sent events in which the answer is the event with our id. The server may hand out a
//! session (`Mcp-Session-Id`), which every later call carries. Calls leave through the one network builder, behind the
//! connector's own proxy.

use super::{PROTOCOL, Transport};
use j4n_connect::net::Net;
use serde_json::{Value, json};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub struct Http {
    net: Net,
    url: String,
    headers: Vec<(String, String)>,
    session: Mutex<Option<String>>,
    next: AtomicU64,
}

impl Http {
    pub fn new(net: Net, url: &str, headers: Vec<(String, String)>) -> Http {
        Http { net, url: url.into(), headers, session: Mutex::new(None), next: AtomicU64::new(1) }
    }

    fn post(&self, doc: &Value, wait: Duration) -> Result<j4n_connect::net::Reply, String> {
        let session = self.session.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut headers: Vec<(&str, &str)> = vec![("Accept", "application/json, text/event-stream"), ("MCP-Protocol-Version", PROTOCOL)];
        headers.extend(self.headers.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        if let Some(s) = &session {
            headers.push(("Mcp-Session-Id", s));
        }
        self.net.clone().idle(wait).request("POST", &self.url, &headers, Some(doc.to_string().into_bytes())).map_err(|e| e.to_string())
    }
}

/// The answer with `id` in a stream of events.
fn from_events(lines: impl Iterator<Item = Result<String, j4n_connect::net::NetError>>, id: u64) -> Result<Value, String> {
    let mut data = String::new();
    let take = |data: &mut String| -> Option<Value> {
        let doc = serde_json::from_str::<Value>(data.trim()).ok();
        data.clear();
        doc.filter(|d| d.get("id").and_then(Value::as_u64) == Some(id) && d.get("method").is_none())
    };
    for line in lines {
        let line = line.map_err(|e| e.to_string())?;
        if let Some(rest) = line.strip_prefix("data:") {
            data.push_str(rest.trim_start());
        } else if line.is_empty()
            && let Some(doc) = take(&mut data)
        {
            return Ok(doc);
        }
    }
    take(&mut data).ok_or_else(|| "the stream ended without an answer".to_string())
}

impl Transport for Http {
    fn call(&self, method: &str, params: Value, wait: Duration) -> Result<Value, String> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let reply = self.post(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }), wait)?;
        if let Some(s) = reply.headers.get("mcp-session-id") {
            *self.session.lock().unwrap_or_else(|e| e.into_inner()) = Some(s.clone());
        }
        if !reply.ok() {
            let status = reply.status;
            return Err(format!("{method}: the server answered {status}: {}", reply.text().unwrap_or_default().chars().take(200).collect::<String>()));
        }
        let stream = reply.headers.get("content-type").is_some_and(|c| c.contains("event-stream"));
        let doc = if stream { from_events(reply.lines(), id)? } else { serde_json::from_str::<Value>(&reply.text().map_err(|e| e.to_string())?).map_err(|e| format!("{method}: the answer is not JSON: {e}"))? };
        match doc.get("error") {
            Some(e) => Err(format!("{method}: {}", e.get("message").and_then(Value::as_str).unwrap_or("the server refused"))),
            None => Ok(doc.get("result").cloned().unwrap_or(Value::Null)),
        }
    }

    fn notify(&self, method: &str, params: Value) {
        let _ = self.post(&json!({ "jsonrpc": "2.0", "method": method, "params": params }), Duration::from_secs(10));
    }

    fn close(&self) {
        // a session is ended by the server when it is not used; a client that leaves without a word does no harm
    }
}
