//! What `pi` and `omp` print, as events. The two share one wire (`{"type":"session","version":3,…}` and the frames of a turn,
//! verified on real captures, `fixtures/pi-frames`). Streamed text and
//! thinking are flashes, `message_end` settles them; a tool is its start and its end, and a `bash` end is a shell line with its
//! real exit code. An errored message is an error, never a silent end (a dead key looks exactly like a finished turn
//! otherwise).

use j4n_core::text::line as clip;
use crate::manifest::Family;
use j4n_core::doer::{Metrics, Role, RunEv, Served};
use serde_json::Value;
use std::collections::HashMap;

/// The longest tool result kept in a tool line.
const DETAIL: usize = 200;

/// What one line meant.
#[derive(Debug, Default)]
pub struct Line {
    pub evs: Vec<RunEv>,
    /// The session id, from a `{"type":"session"}` line.
    pub session: Option<String>,
    /// The model, from the usage a message reports.
    pub model: Option<String>,
}

/// One child's translator: a tool's end does not repeat its arguments, so the start's are kept by the call's id.
#[derive(Default)]
pub struct Translator {
    commands: HashMap<String, String>,
    targets: HashMap<String, String>,
    /// The words of the last assistant message that had any: the answer when the turn ends.
    said: Option<String>,
    /// The last error said, so the same sentence is not said twice.
    last_error: Option<String>,
    /// The tool calls the model is still writing, by their place in the message: the name, and the arguments so far (live only).
    writing: HashMap<u64, (String, String)>,
    /// The output tokens last said, so that the same count is not said again.
    tokens: u64,
}

/// Whether this frame ends the turn: pi's `agent_settled` (an `agent_end` can still be followed by retries, compaction or
/// follow-up work), omp's `prompt_result` or `session_settled`.
pub fn ends_turn(family: Family, v: &Value) -> bool {
    let ty = v.get("type").and_then(Value::as_str).unwrap_or_default();
    match family {
        Family::Omp => ty == "prompt_result" || ty == "session_settled",
        Family::Pi => ty == "agent_settled",
    }
}

impl Translator {
    pub fn new() -> Translator {
        Translator::default()
    }

    /// The answer so far.
    pub fn said(&self) -> Option<String> {
        self.said.clone()
    }

    pub fn forget(&mut self) {
        self.said = None;
        self.last_error = None;
        self.commands.clear();
        self.targets.clear();
        self.writing.clear();
        self.tokens = 0;
    }

    fn error(&mut self, out: &mut Line, message: String) {
        if self.last_error.as_deref() != Some(message.as_str()) {
            self.last_error = Some(message.clone());
            out.evs.push(RunEv::Error(message));
        }
    }

    /// Translate one line of stdout. A line that is not JSON (the children interleave log lines) is a notice, never dropped.
    pub fn line(&mut self, line: &str) -> Line {
        match serde_json::from_str::<Value>(line) {
            Ok(v) => self.frame(&v),
            Err(_) => Line { evs: vec![RunEv::Notice(format!("log: {}", clip(line, 120)))], ..Line::default() },
        }
    }

    pub fn frame(&mut self, v: &Value) -> Line {
        let mut out = Line::default();
        let text = |p: &str| v.pointer(p).and_then(Value::as_str).map(String::from);
        let call = text("/toolCallId");
        match v.get("type").and_then(Value::as_str).unwrap_or_default() {
            "session" => out.session = text("/id"),
            "message_update" | "message_start" => {
                let delta = text("/assistantMessageEvent/delta").filter(|d| !d.is_empty());
                match (text("/assistantMessageEvent/type").as_deref(), delta) {
                    (Some("text_delta"), Some(d)) => out.evs.push(RunEv::Text(d)),
                    (Some("thinking_delta"), Some(d)) => out.evs.push(RunEv::Reasoning(d)),
                    (Some("toolcall_start"), _) => self.writing_start(v, &mut out),
                    (Some("toolcall_delta"), d) => self.writing_grow(v, d.as_deref().unwrap_or_default(), &mut out),
                    (Some("toolcall_end"), _) => {
                        self.writing.remove(&v.pointer("/assistantMessageEvent/contentIndex").and_then(Value::as_u64).unwrap_or(0));
                    }
                    _ => {}
                }
                let said = ["/assistantMessageEvent/partial/usage/output", "/message/usage/output"].iter().find_map(|p| v.pointer(p).and_then(Value::as_u64)).unwrap_or(0);
                if said > 0 && said != self.tokens {
                    self.tokens = said;
                    out.evs.push(RunEv::Tokens(said));
                }
            }
            "message_end" => {
                if let Some(msg) = v.get("message") {
                    self.message_end(msg, &mut out);
                }
            }
            "tool_execution_start" => {
                let name = text("/toolName").unwrap_or_else(|| "tool".into());
                if let Some(id) = &call {
                    if let Some(cmd) = text("/args/command") {
                        self.commands.insert(id.clone(), cmd);
                    }
                    if let Some(t) = ["/args/path", "/args/file", "/args/file_path"].iter().find_map(|p| text(p)) {
                        self.targets.insert(id.clone(), t);
                    }
                }
                let detail = text("/intent").or_else(|| text("/args/command")).or_else(|| text("/args/path")).unwrap_or_default();
                out.evs.push(RunEv::Tool { name, detail: clip(&detail, DETAIL), end: false });
            }
            "tool_execution_end" => {
                let name = text("/toolName").unwrap_or_else(|| "tool".into());
                let failed = v.get("isError").and_then(Value::as_bool).unwrap_or(false);
                let result = text("/result/content/0/text").unwrap_or_default();
                let cmd = call.as_ref().and_then(|id| self.commands.remove(id));
                let target = call.as_ref().and_then(|id| self.targets.remove(id));
                if name == "bash" {
                    out.evs.push(RunEv::Shell { cmd: cmd.or_else(|| text("/args/command")).unwrap_or_default(), out: result, exit: i32::from(failed) });
                } else {
                    let detail = if failed { format!("failed: {}", clip(&result, DETAIL)) } else { clip(&result, DETAIL) };
                    out.evs.push(RunEv::Tool { name: name.clone(), detail, end: true });
                    if !failed && matches!(name.as_str(), "write" | "edit") {
                        out.evs.extend(target.map(|t| RunEv::Changed(vec![t])));
                    }
                }
            }
            "error" => {
                let message = text("/message").unwrap_or_else(|| "unknown".into());
                self.error(&mut out, message);
            }
            // omp echoes the turn's outcome: when the turn died before it produced a message, this is the only place the
            // reason appears
            "prompt_result" => {
                if let Some(m) = text("/error/message") {
                    self.error(&mut out, m);
                }
            }
            _ => {}
        }
        out
    }

    /// A call begins: its name is in the event, or in the message so far.
    fn writing_start(&mut self, v: &Value, out: &mut Line) {
        let at = v.pointer("/assistantMessageEvent/contentIndex").and_then(Value::as_u64).unwrap_or(0);
        let name = ["/assistantMessageEvent/toolName", "/assistantMessageEvent/toolCall/name"]
            .iter()
            .map(|p| v.pointer(p))
            .chain(["/assistantMessageEvent/partial/content", "/message/content"].iter().map(|p| v.pointer(&format!("{p}/{at}/name"))))
            .find_map(|n| n.and_then(Value::as_str))
            .unwrap_or("tool")
            .to_string();
        self.writing.insert(at, (name, String::new()));
        self.said_writing(at, v, out);
    }

    /// A call grows: the delta is a piece of its arguments' JSON; a pi that sends the arguments parsed so far says them too.
    fn writing_grow(&mut self, v: &Value, delta: &str, out: &mut Line) {
        let at = v.pointer("/assistantMessageEvent/contentIndex").and_then(Value::as_u64).unwrap_or(0);
        if !self.writing.contains_key(&at) {
            self.writing_start(v, out);
        }
        if let Some((_, args)) = self.writing.get_mut(&at) {
            args.push_str(delta);
        }
        self.said_writing(at, v, out);
    }

    fn said_writing(&self, at: u64, v: &Value, out: &mut Line) {
        let Some((name, raw)) = self.writing.get(&at) else { return };
        let parsed = ["/assistantMessageEvent/partial/content", "/message/content"].iter().find_map(|p| v.pointer(&format!("{p}/{at}/arguments"))).filter(|a| a.is_object());
        let mut target = crate::opening::target_in(raw);
        let mut bytes = raw.len() as u64;
        if let Some(a) = parsed {
            if target.is_empty() {
                target = ["file_path", "path", "file", "command"].iter().find_map(|k| a.get(*k).and_then(Value::as_str)).unwrap_or_default().to_string();
            }
            bytes = bytes.max(a.to_string().len() as u64);
        }
        out.evs.push(RunEv::Open { id: at.to_string(), name: name.clone(), target, bytes });
    }

    fn message_end(&mut self, msg: &Value, out: &mut Line) {
        let role = msg.get("role").and_then(Value::as_str).unwrap_or_default();
        if msg.get("stopReason").and_then(Value::as_str) == Some("error") {
            let why = msg.get("errorMessage").and_then(Value::as_str).unwrap_or("model turn errored (no message)").to_string();
            self.error(out, why);
            return;
        }
        if role != "assistant" {
            return;
        }
        if let Some(usage) = msg.get("usage") {
            let usd = usage.pointer("/cost/total").and_then(Value::as_f64).unwrap_or(0.0);
            if usd > 0.0 || usage.get("totalTokens").is_some() {
                out.model = msg.get("model").and_then(Value::as_str).map(String::from);
                let n = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
                // who served the call, and what it read fresh: the same `run.served` the own agent writes; the cache parts ride in
                // `run.metrics` with the cost and are in `tokens_in` (as claude's do) — one row, never both
                if let Some(model) = msg.get("model").and_then(Value::as_str).filter(|m| !m.is_empty()) {
                    let host = msg.get("provider").and_then(Value::as_str).filter(|h| !h.is_empty()).map(String::from);
                    out.evs.push(RunEv::Served(Served { model: model.to_string(), host, input: n("input"), output: n("output"), ..Served::default() }));
                }
                let (cached, wrote) = (n("cacheRead"), n("cacheWrite"));
                out.evs.push(RunEv::Metrics(Metrics { usd, tokens_in: n("input").saturating_add(cached).saturating_add(wrote), tokens_out: n("output"), cached, wrote, ..Metrics::default() }));
            }
        }
        self.writing.clear();
        self.tokens = 0;
        for block in msg.get("content").and_then(Value::as_array).into_iter().flatten() {
            if block.get("type").and_then(Value::as_str) == Some("thinking") {
                let t = block.get("thinking").and_then(Value::as_str).unwrap_or_default();
                if !t.trim().is_empty() {
                    out.evs.push(RunEv::Message { role: Role::Reasoning, text: t.to_string() });
                }
            }
            if block.get("type").and_then(Value::as_str) == Some("text") {
                let t = block.get("text").and_then(Value::as_str).unwrap_or_default();
                if !t.trim().is_empty() {
                    self.said = Some(t.trim().to_string());
                    out.evs.push(RunEv::Message { role: Role::Assistant, text: t.to_string() });
                }
            }
        }
    }
}

/// The title of omp's approval dialog — `Allow tool: bash\nCommand: …`, `Allow tool: edit\nFile: …`,
/// `Allow tool: write\nPath: …\nContent:\n…` — as `(tool, target)`.
pub fn approval_title(title: &str) -> Option<(String, String)> {
    let mut lines = title.lines();
    let tool = lines.next()?.strip_prefix("Allow tool:")?.trim().to_string();
    if tool.is_empty() {
        return None;
    }
    let target = lines.find_map(|l| ["Command:", "File:", "Path:"].iter().find_map(|k| l.strip_prefix(k))).map(|t| t.trim().to_string()).unwrap_or_default();
    Some((tool, target))
}
