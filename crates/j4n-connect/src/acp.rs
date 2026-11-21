//! Dialect `acp`: the Agent Client Protocol (JSON-RPC 2.0 over the stdio of an agent), one dialect for the many agents that speak
//! it (agentclientprotocol.com, protocol version 1). A child is started once and kept between turns:
//! `initialize`, `session/new` (or `session/load` to carry on in a session whose child is gone), then a `session/prompt` for each
//! turn. What the agent streams (`session/update`) becomes the chat; its `session/request_permission` becomes a question for the
//! Gate and the operator.
//!
//! J4n offers the agent no file or terminal methods (it advertises none): the agent does its work with its own tools and asks
//! leave for each. A steer is not in the protocol, so the connector is not `live`: the loop stops the run (`session/cancel`, the
//! child stays) and starts it again in the same session.

use j4n_core::text::line as clip;
use crate::effects;
use crate::manifest::Connector;
use crate::permit;
use crate::pool::{Pool, Pooled, Reuse, signature};
use crate::proc::{self, Child, Exit};
use crate::template::{self, Vars};
use j4n_core::ask::{Answer, AskShape, AskSpec};
use j4n_core::change::Actor;
use j4n_core::doer::{Caps, Doer, Emit, Metrics, Probe, Role, Run, RunCtl, RunEv, StartCx};
use j4n_core::gate::{Action, ReadOnly, Verb};
use j4n_core::place::Launch;
use j4n_core::table::Problem;
use j4n_core::task::Mode;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ChildStdin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// The protocol version this speaks.
pub const PROTOCOL: u64 = 1;
/// What a request is told when the agent ends under it: the turn is failed by whoever saw the end, with what it wrote.
const GONE: &str = "the agent ended";
/// How long each step of the start (`initialize`, `session/new`) is given.
const HANDSHAKE: Duration = Duration::from_secs(60);
/// How long a cancelled turn is given to end by itself before the child is taken down.
const CANCEL_GRACE: Duration = Duration::from_secs(5);
/// How long a start waits for a session that is still ending its last turn.
const BUSY_WAIT: Duration = Duration::from_secs(5);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A question the agent has put and is blocked on.
struct Perm {
    /// The JSON-RPC id the answer goes back to.
    rpc: Value,
    /// The tool call it is for: opencode asks about a command twice (its folder rule, then the command itself), both under one call id.
    call: String,
    options: Vec<(String, String)>,
    spec: AskSpec,
    /// Still to be put to the operator (once, by the agent's next request or by the reply to the answer before it).
    queued: bool,
}

/// A tool call that has begun: what it is, for when it ends.
#[derive(Default)]
struct Tool {
    kind: String,
    title: String,
    cmd: Option<String>,
    paths: Vec<String>,
}

/// The turn that is going on.
struct Open {
    out: Emit,
    /// The words since the last tool: the answer if nothing follows.
    para: String,
    said: Option<String>,
    /// What each tool call is (`execute`, `edit`…), by its id, for when it ends.
    kinds: HashMap<String, Tool>,
    cost: f64,
    plan_left: u32,
    cancelled: bool,
    task: String,
    site: String,
    read_only: ReadOnly,
    /// The lines this turn was answered no for, and how many times the agent was told to go on after one.
    refused: Vec<String>,
    told: u8,
}

struct Inner {
    id: String,
    next: AtomicU64,
    pending: Mutex<HashMap<u64, Sender<Result<Value, String>>>>,
    turn: Mutex<Option<Open>>,
    asks: Mutex<VecDeque<Perm>>,
    session: Mutex<Option<String>>,
    activity: Mutex<Option<Instant>>,
    ready: AtomicBool,
    dead: AtomicBool,
    stdin: Mutex<Option<ChildStdin>>,
    /// What this turn answered for a shell line, by tool call: the second ask of the same call about the same line is answered the same, unasked.
    decided: Mutex<HashMap<String, (String, Answer)>>,
}

/// The option an answer picks among the ones the agent offered: yes is an allow, no a reject, and an answer it has no option for is a cancelled question.
fn outcome(options: &[(String, String)], answer: &Answer) -> Result<Value, String> {
    let pick = |kinds: [&str; 2]| kinds.iter().find_map(|k| options.iter().find(|(_, kind)| kind == k)).map(|(id, _)| id.clone());
    let chosen = match answer {
        Answer::Yes => pick(["allow_once", "allow_always"]),
        Answer::No => pick(["reject_once", "reject_always"]),
        _ => return Err("this asks for permission: yes or no".into()),
    };
    Ok(json!({ "outcome": chosen.map_or_else(|| json!({ "outcome": "cancelled" }), |id| json!({ "outcome": "selected", "optionId": id })) }))
}

impl Inner {
    fn say(&self, ev: RunEv) -> bool {
        lock(&self.turn).as_ref().is_some_and(|o| o.out.send(ev))
    }

    fn close(&self) {
        lock(&self.turn).take();
        lock(&self.asks).clear();
    }

    fn fail(&self, message: String) {
        self.say(RunEv::Error(message));
        self.close();
    }

    fn write(&self, frame: &Value) -> Result<(), String> {
        if self.dead.load(Ordering::SeqCst) {
            return Err("the agent is gone".into());
        }
        let mut stdin = lock(&self.stdin);
        let Some(stdin) = stdin.as_mut() else { return Err("the agent has no input".into()) };
        writeln!(stdin, "{frame}").and_then(|_| stdin.flush()).map_err(|e| format!("could not write to the agent: {e}"))
    }

    /// A request, and its answer. `limit` None waits as long as the agent takes (a turn).
    fn request(&self, method: &str, params: Value, limit: Option<Duration>) -> Result<Value, String> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        lock(&self.pending).insert(id, tx);
        if let Err(e) = self.write(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })) {
            lock(&self.pending).remove(&id);
            return Err(e);
        }
        let got = match limit {
            Some(l) => rx.recv_timeout(l).map_err(|_| format!("{method}: no answer in {} seconds", l.as_secs())),
            None => rx.recv().map_err(|_| format!("{method}: the agent went away")),
        };
        lock(&self.pending).remove(&id);
        got?
    }

    fn respond(&self, id: &Value, result: Value) {
        let _ = self.write(&json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }

    fn refuse(&self, id: &Value, message: &str) {
        let _ = self.write(&json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": message } }));
    }

    /// One line of stdout.
    fn pump(&self, line: &str) {
        *lock(&self.activity) = Some(Instant::now());
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            self.say(RunEv::Notice(format!("log: {}", clip(line, 120))));
            return;
        };
        let method = v.get("method").and_then(Value::as_str);
        match (method, v.get("id")) {
            (Some("session/request_permission"), Some(id)) => self.permission(id.clone(), v.get("params").unwrap_or(&Value::Null)),
            (Some(m), Some(id)) => self.refuse(id, &format!("J4n offers no `{m}`")),
            (Some("session/update"), None) => self.update(v.pointer("/params/update").unwrap_or(&Value::Null)),
            (Some(_), None) => {}
            (None, Some(id)) => {
                let answer = match v.get("error") {
                    Some(e) => Err(e.get("message").and_then(Value::as_str).unwrap_or("the agent refused").to_string()),
                    None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
                };
                if let Some(tx) = id.as_u64().and_then(|n| lock(&self.pending).remove(&n)) {
                    let _ = tx.send(answer);
                }
            }
            (None, None) => {}
        }
    }

    fn text_of(content: &Value) -> String {
        match content {
            Value::Array(items) => items.iter().map(Self::text_of).filter(|t| !t.is_empty()).collect::<Vec<_>>().join("\n"),
            Value::Object(_) => {
                let inner = content.get("content").unwrap_or(content);
                match inner.get("type").and_then(Value::as_str) {
                    Some("text") => inner.get("text").and_then(Value::as_str).unwrap_or_default().to_string(),
                    Some("diff") => format!("diff {}", inner.get("path").and_then(Value::as_str).unwrap_or_default()),
                    _ => String::new(),
                }
            }
            _ => String::new(),
        }
    }

    fn flush(turn: &mut Open) {
        let words = std::mem::take(&mut turn.para);
        if !words.trim().is_empty() {
            turn.said = Some(words.trim().to_string());
            turn.out.send(RunEv::Message { role: Role::Assistant, text: words.trim().to_string() });
        }
    }

    fn update(&self, u: &Value) {
        let mut guard = lock(&self.turn);
        let Some(turn) = guard.as_mut() else { return };
        let text = |k: &str| u.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        match u.get("sessionUpdate").and_then(Value::as_str).unwrap_or_default() {
            "agent_message_chunk" => {
                let t = Self::text_of(u.get("content").unwrap_or(&Value::Null));
                if !t.is_empty() {
                    turn.para.push_str(&t);
                    turn.out.send(RunEv::Text(t));
                }
            }
            "agent_thought_chunk" => {
                let t = Self::text_of(u.get("content").unwrap_or(&Value::Null));
                if !t.is_empty() {
                    turn.out.send(RunEv::Reasoning(t));
                }
            }
            "tool_call" => {
                Self::flush(turn);
                let (id, kind) = (text("toolCallId"), text("kind"));
                let title = if text("title").is_empty() { text("name") } else { text("title") };
                let paths: Vec<String> = u.get("locations").and_then(Value::as_array).into_iter().flatten().filter_map(|l| l.get("path").and_then(Value::as_str)).map(String::from).collect();
                let cmd = u.pointer("/rawInput/command").and_then(Value::as_str).map(String::from);
                let target = paths.first().cloned().unwrap_or_default();
                turn.kinds.insert(id, Tool { kind, title: title.clone(), cmd, paths });
                turn.out.send(RunEv::Tool { name: title, detail: target, end: false });
            }
            "tool_call_update" => {
                let status = text("status");
                if !matches!(status.as_str(), "completed" | "failed") {
                    return;
                }
                let id = text("toolCallId");
                let tool = turn.kinds.remove(&id).unwrap_or_default();
                let out = Self::text_of(u.get("content").unwrap_or(&Value::Null));
                let failed = status == "failed";
                if tool.kind == "execute" {
                    // opencode names the call `bash` when it begins and gives the command as the title of the update that ends it
                    let cmd = u.pointer("/rawInput/command").and_then(Value::as_str).map(String::from).or(tool.cmd).or_else(|| Some(text("title")).filter(|t| !t.is_empty())).unwrap_or(tool.title);
                    turn.out.send(RunEv::Shell { cmd, out, exit: i32::from(failed) });
                } else {
                    turn.out.send(RunEv::Tool { name: tool.title, detail: if failed { format!("failed: {}", clip(&out, 160)) } else { clip(&out, 160) }, end: true });
                    if !failed && matches!(tool.kind.as_str(), "edit" | "delete" | "move") && !tool.paths.is_empty() {
                        turn.out.send(RunEv::Changed(tool.paths));
                    }
                }
            }
            "plan" => {
                turn.plan_left = u.get("entries").and_then(Value::as_array).map_or(0, |e| e.iter().filter(|x| x.get("status").and_then(Value::as_str) != Some("completed")).count() as u32);
            }
            "usage_update" => {
                if let Some(total) = u.pointer("/cost/amount").and_then(Value::as_f64) {
                    let delta = total - turn.cost;
                    turn.cost = total;
                    if delta > 0.0 {
                        turn.out.send(RunEv::Metrics(Metrics { usd: delta, ..Metrics::default() }));
                    }
                }
            }
            "notice" => {
                let words = format!("{} {}", text("title"), text("description"));
                turn.out.send(RunEv::Notice(words.trim().to_string()));
            }
            _ => {}
        }
    }

    /// The agent asks leave for a tool call: a question for the Gate and then the operator, one at a time.
    fn permission(&self, rpc: Value, params: &Value) {
        let call = params.get("toolCall").unwrap_or(&Value::Null);
        let field = |k: &str| call.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        let title = if field("title").is_empty() { "a tool".to_string() } else { field("title") };
        let kind = field("kind");
        let path = call.pointer("/locations/0/path").and_then(Value::as_str).map(String::from);
        let command = call.pointer("/rawInput/command").and_then(Value::as_str).map(String::from);
        let call_id = field("toolCallId");
        let (verb, target, risk) = match kind.as_str() {
            "read" | "search" => (Verb::Read, path.unwrap_or_else(|| title.clone()), 10),
            "edit" | "delete" | "move" => (Verb::Write, path.unwrap_or_else(|| title.clone()), 40),
            "fetch" => (Verb::Net, call.pointer("/rawInput/url").and_then(Value::as_str).map_or_else(|| title.clone(), effects::host_of), 30),
            // opencode's rule for a folder outside the project (`kind: other`, `filepath`, `parentDir`) is asked before the tool, and says nothing of the tool: the call's
            // own `tool_call` did (`edit`, `read`…). A read is a read; an edit, a delete, a move, and a call that was not seen, is a WRITE of that file, because with the
            // tool's own permission `allow` this is the only ask that comes before it writes
            _ if command.is_none() && call.pointer("/rawInput/filepath").is_some() => {
                let file = call.pointer("/rawInput/filepath").and_then(Value::as_str).unwrap_or(&title).to_string();
                let seen = lock(&self.turn).as_ref().and_then(|o| o.kinds.get(&call_id).map(|t| t.kind.clone()));
                if matches!(seen.as_deref(), Some("read" | "search")) { (Verb::Read, file, 20) } else { (Verb::Write, file, 40) }
            }
            _ => (Verb::Shell, command.unwrap_or_else(|| title.clone()), 60),
        };
        if verb == Verb::Shell {
            let before = lock(&self.decided).get(&call_id).filter(|(line, _)| *line == target).map(|(_, a)| a.clone());
            let options: Vec<(String, String)> = params.get("options").and_then(Value::as_array).into_iter().flatten().filter_map(|o| Some((o.get("optionId")?.as_str()?.to_string(), o.get("kind")?.as_str()?.to_string()))).collect();
            if let Some(result) = before.and_then(|a| outcome(&options, &a).ok()) {
                self.respond(&rpc, result);
                return;
            }
        }
        let options: Vec<(String, String)> = params
            .get("options")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|o| Some((o.get("optionId")?.as_str()?.to_string(), o.get("kind")?.as_str()?.to_string())))
            .collect();
        let (task, site, read_only) = lock(&self.turn).as_ref().map(|o| (o.task.clone(), o.site.clone(), o.read_only)).unwrap_or_default();
        let mut act = Action::new(verb, Actor::Doer(self.id.clone()), &target).task(&task);
        if matches!(verb, Verb::Read | Verb::Write) {
            act.inside = Some(effects::inside(Path::new(&site), &target));
        }
        act.site = Some(site);
        act.read_only = read_only;
        let spec = AskSpec {
            local: field("toolCallId").chars().chain("-perm".chars()).collect(),
            question: format!("allow {}?", clip(&title, 160)),
            shape: AskShape::Approval { action: format!("{}: {}", kind_word(&kind), clip(&target, 200)), risk },
            inband: true,
            point: None,
            act: Some(Box::new(act)),
        };
        lock(&self.asks).push_back(Perm { rpc, call: call_id, options, spec, queued: true });
        self.put_head();
    }

    /// Put the question at the head of the queue to the operator, once: put twice, the task is stopped for a second question.
    fn put_head(&self) {
        let spec = lock(&self.asks).front_mut().and_then(|p| std::mem::take(&mut p.queued).then(|| p.spec.clone()));
        if let Some(spec) = spec {
            self.say(RunEv::Ask(spec));
        }
    }

    /// The agent is gone. A turn it leaves open fails, with what it wrote to stderr.
    fn exited(&self, exit: Exit) {
        self.dead.store(true, Ordering::SeqCst);
        for (_, tx) in lock(&self.pending).drain() {
            let _ = tx.send(Err(GONE.into()));
        }
        if exit.killed {
            return self.close();
        }
        let tail = if exit.tail.is_empty() { String::new() } else { format!(": {}", clip(&exit.tail.replace('\n', " | "), 300)) };
        self.fail(format!("{} ended before it finished its turn ({}){tail}", self.id, exit.how()));
    }
}

fn kind_word(kind: &str) -> &str {
    if kind.is_empty() { "tool" } else { kind }
}

/// A live agent child: one process for a whole conversation.
pub struct Session {
    launch: String,
    cwd: PathBuf,
    child: Child,
    inner: Arc<Inner>,
}

impl Session {
    fn spawn(id: &str, launch: &Launch, signature: String) -> std::io::Result<Arc<Session>> {
        let inner = Arc::new(Inner {
            id: id.to_string(),
            next: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
            turn: Mutex::new(None),
            asks: Mutex::new(VecDeque::new()),
            session: Mutex::new(None),
            activity: Mutex::new(None),
            ready: AtomicBool::new(false),
            dead: AtomicBool::new(false),
            stdin: Mutex::new(None),
            decided: Mutex::new(HashMap::new()),
        });
        let (i1, i2) = (inner.clone(), inner.clone());
        let (child, stdin) = proc::spawn(launch, true, move |l| i1.pump(l), move |e| i2.exited(e))?;
        *lock(&inner.stdin) = stdin;
        Ok(Arc::new(Session { launch: signature, cwd: launch.cwd.clone(), child, inner }))
    }

    fn begin(&self, out: Emit, task: String, site: String, read_only: ReadOnly) {
        *lock(&self.inner.activity) = Some(Instant::now());
        lock(&self.inner.decided).clear();
        *lock(&self.inner.turn) = Some(Open { out, para: String::new(), said: None, kinds: HashMap::new(), cost: 0.0, plan_left: 0, cancelled: false, task, site, read_only, refused: Vec::new(), told: 0 });
    }

    /// `initialize` and then `session/new` (or `session/load` in `resume`): the id of the session, and whether it was loaded.
    fn open(&self, cwd: &Path, resume: Option<&str>) -> Result<(String, bool), String> {
        let init = self.inner.request(
            "initialize",
            json!({ "protocolVersion": PROTOCOL, "clientCapabilities": { "fs": { "readTextFile": false, "writeTextFile": false }, "terminal": false }, "clientInfo": { "name": "j4n", "title": "J4n", "version": env!("CARGO_PKG_VERSION") } }),
            Some(HANDSHAKE),
        )?;
        let can_load = init.pointer("/agentCapabilities/loadSession").and_then(Value::as_bool) == Some(true);
        let cwd = cwd.display().to_string();
        if let Some(id) = resume.filter(|_| can_load)
            && self.inner.request("session/load", json!({ "sessionId": id, "cwd": cwd, "mcpServers": [] }), Some(HANDSHAKE)).is_ok()
        {
            *lock(&self.inner.session) = Some(id.to_string());
            self.inner.ready.store(true, Ordering::SeqCst);
            return Ok((id.to_string(), true));
        }
        let made = self.inner.request("session/new", json!({ "cwd": cwd, "mcpServers": [] }), Some(HANDSHAKE)).map_err(|e| {
            let methods: Vec<&str> = init.get("authMethods").and_then(Value::as_array).into_iter().flatten().filter_map(|m| m.get("name").and_then(Value::as_str)).collect();
            if methods.is_empty() { e } else { format!("{e} (the agent signs in by {}, which J4n cannot open for it yet; Agents › List › r looks again once it is signed in)", methods.join(", ")) }
        })?;
        let id = made.get("sessionId").and_then(Value::as_str).ok_or("session/new gave no sessionId")?.to_string();
        *lock(&self.inner.session) = Some(id.clone());
        self.inner.ready.store(true, Ordering::SeqCst);
        Ok((id, false))
    }

    /// The turn: `session/prompt` until the agent answers it, and how it ended.
    fn prompt(&self, session: &str, text: &str) {
        let mut text = text.to_string();
        loop {
            let r = self.inner.request("session/prompt", json!({ "sessionId": session, "prompt": [{ "type": "text", "text": text }] }), None);
            // an agent that ended under the request: whoever saw it end fails the turn, with what it wrote
            if matches!(&r, Err(e) if e == GONE) {
                return;
            }
            let mut guard = lock(&self.inner.turn);
            let Some(turn) = guard.as_mut() else { return };
            let cancelled = turn.cancelled;
            let mut again = false;
            let ev = match r {
                Err(_) if cancelled => None,
                Err(e) => Some(RunEv::Error(e)),
                Ok(v) => match v.get("stopReason").and_then(Value::as_str).unwrap_or("end_turn") {
                    "end_turn" => {
                        Self::settle(turn);
                        // opencode ends the turn when a request is answered no, with nothing said: the agent is told what was refused and to go on without it, so the
                        // task ends with an answer and not with the refusal; twice at most
                        if turn.refused.is_empty() || turn.said.as_deref().is_some_and(|s| !s.trim().is_empty()) || turn.told >= 2 || cancelled {
                            Some(RunEv::End { answer: turn.said.clone(), todo_left: turn.plan_left })
                        } else {
                            turn.told += 1;
                            again = true;
                            let lines: Vec<String> = turn.refused.drain(..).map(|l| format!("`{}`", clip(&l, 200))).collect();
                            text = format!("Not allowed here: {}. Do not try it again or another way round it. Carry on with what you can do, and answer, saying what you could not do.", lines.join(", "));
                            None
                        }
                    }
                    "cancelled" => None,
                    "max_tokens" => Some(RunEv::Error("the agent stopped: it reached its token limit".into())),
                    "max_turn_requests" => Some(RunEv::Error("the agent stopped: it made too many requests in one turn".into())),
                    "refusal" => Some(RunEv::Error("the agent refused to go on".into())),
                    other => Some(RunEv::Error(format!("the agent stopped: {other}"))),
                },
            };
            if let Some(ev) = ev {
                turn.out.send(ev);
            }
            drop(guard);
            if again {
                continue;
            }
            self.inner.close();
            return;
        }
    }

    fn settle(turn: &mut Open) {
        Inner::flush(turn);
    }

    fn answer(&self, local: &str, answer: &Answer) -> Result<(), String> {
        let mut asks = lock(&self.inner.asks);
        let head = asks.front().ok_or("no question is open")?;
        if head.spec.local != local {
            return Err(format!("`{local}` is not the question that is open"));
        }
        let result = outcome(&head.options, answer)?;
        if let Some(act) = head.spec.act.as_ref().filter(|a| a.verb == Verb::Shell) {
            lock(&self.inner.decided).insert(head.call.clone(), (act.target.clone(), answer.clone()));
        }
        let refused = (*answer == Answer::No).then(|| head.spec.act.as_ref().map_or_else(|| head.spec.question.clone(), |a| a.target.clone()));
        let rpc = head.rpc.clone();
        asks.pop_front();
        drop(asks);
        if let (Some(line), Some(turn)) = (refused, lock(&self.inner.turn).as_mut()) {
            turn.refused.push(line);
        }
        self.inner.respond(&rpc, result);
        self.inner.put_head();
        Ok(())
    }

    /// Cancel the turn: the agent is told, answers its open questions as cancelled, and ends the turn by itself; one that does
    /// not is taken down after a grace.
    fn cancel(self: &Arc<Self>) {
        if let Some(turn) = lock(&self.inner.turn).as_mut() {
            turn.cancelled = true;
        }
        let pending: Vec<Perm> = lock(&self.inner.asks).drain(..).collect();
        for p in pending {
            self.inner.respond(&p.rpc, json!({ "outcome": { "outcome": "cancelled" } }));
        }
        let session = lock(&self.inner.session).clone();
        match session {
            Some(id) => {
                let _ = self.inner.write(&json!({ "jsonrpc": "2.0", "method": "session/cancel", "params": { "sessionId": id } }));
            }
            None => return self.stop(),
        }
        let weak = Arc::downgrade(self);
        std::thread::spawn(move || {
            std::thread::sleep(CANCEL_GRACE);
            if let Some(s) = weak.upgrade().filter(|s| lock(&s.inner.turn).is_some()) {
                s.stop();
            }
        });
    }

    fn stop(&self) {
        self.inner.close();
        self.child.kill();
        self.inner.dead.store(true, Ordering::SeqCst);
    }

    fn in_turn(&self) -> bool {
        lock(&self.inner.turn).is_some()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.child.kill();
    }
}

impl Pooled for Session {
    fn session_id(&self) -> Option<String> {
        lock(&self.inner.session).clone()
    }

    fn launch(&self) -> &str {
        &self.launch
    }

    fn cwd(&self) -> &Path {
        &self.cwd
    }

    fn is_dead(&self) -> bool {
        self.inner.dead.load(Ordering::SeqCst)
    }

    fn is_idle(&self) -> bool {
        self.inner.ready.load(Ordering::SeqCst) && !self.in_turn()
    }

    fn last_activity(&self) -> Instant {
        lock(&self.inner.activity).unwrap_or_else(Instant::now)
    }

    fn kill(&self) {
        self.stop();
    }
}

pub struct Acp {
    c: Connector,
    env: BTreeMap<String, String>,
    pool: Arc<Pool<Session>>,
}

impl Acp {
    pub fn new(c: Connector, env: BTreeMap<String, String>) -> Acp {
        Acp { c, env, pool: Arc::new(Pool::new(3, Duration::from_secs(600))) }
    }

    fn argv(&self, cx: &StartCx) -> Result<Vec<String>, String> {
        let mut vars = Vars::new();
        vars.insert("cwd".into(), cx.site.dir.display().to_string());
        if let Some(m) = cx.spec.tune.model.as_ref().filter(|m| !m.is_empty()) {
            vars.insert("model".into(), m.clone());
        }
        let mut tpl = self.c.start.clone();
        if vars.contains_key("model") {
            tpl.extend(self.c.model.iter().cloned());
        }
        tpl.extend(self.c.modes.get(if cx.spec.tune.mode == Mode::Read { "read" } else { "write" }).into_iter().flatten().cloned());
        let mut argv = vec![self.c.program().to_string()];
        argv.extend(template::render(&tpl, &vars)?);
        Ok(argv)
    }
}

impl Doer for Acp {
    fn id(&self) -> &str {
        &self.c.id
    }

    fn probe(&self) -> Probe {
        self.c.probe(&self.env)
    }

    fn caps(&self) -> Caps {
        Caps { live: false, resume: true, walled: self.c.guard.is_some(), ..Caps::default() }
    }

    fn shutdown(&self) {
        self.pool.kill_all();
    }

    fn start(&self, cx: StartCx) -> Result<Box<dyn Run>, Problem> {
        let id = self.c.id.clone();
        let refuse = |why: String| Problem::refused(&id, why);
        let text = cx.text.clone().unwrap_or_else(|| cx.spec.goal.clone());
        let argv = self.argv(&cx).map_err(&refuse)?;
        let dir = cx.site.dir.clone();
        // a program with permission rules of its own is given the config J4n made for the task: its asks come to J4n's gate, which answers them
        let mut env = self.env.clone();
        if let Some(g) = &self.c.guard {
            let tmp = cx.tmp.as_ref().map(|t| t.display().to_string()).or_else(|| env.get("TMPDIR").cloned()).or_else(|| std::env::var("TMPDIR").ok());
            let scratch = permit::opencode_scratch(tmp.as_deref());
            let walled = cx.site.notes.iter().any(|n| n.starts_with(crate::place::boxed::BEHIND));
            let read_only = cx.spec.tune.mode == Mode::Read || cx.site.read_only;
            env.insert(g.env.clone(), permit::opencode_asking(g, self.c.permit_full, read_only, &dir, &cx.site.also, &scratch, walled, cx.spec.tune.model.as_deref()));
        }
        let want = signature(&argv, &dir, &env);
        self.pool.reap(Instant::now());
        let resume = cx.resume.as_ref().filter(|r| r.doer == id).map(|r| r.id.clone());
        let mut live: Option<Arc<Session>> = None;
        if let Some(sid) = &resume {
            let until = Instant::now() + BUSY_WAIT;
            loop {
                match self.pool.reusable(sid, &want, &dir) {
                    Reuse::Live(s) => {
                        live = Some(s);
                        break;
                    }
                    // the last turn of that session is still ending (a stop is not instant)
                    Reuse::Busy if Instant::now() < until => std::thread::sleep(Duration::from_millis(20)),
                    Reuse::Busy => return Err(refuse("that session is busy with another task".into())),
                    Reuse::Replaced(_) | Reuse::None => break,
                }
            }
        }
        let (session, known) = match live {
            Some(s) => (s, resume.clone()),
            None => {
                let launch = cx.wrap(Launch { login: self.c.login.clone(), ..Launch::new(argv, dir.clone(), env) });
                let s = Session::spawn(&id, &launch, want).map_err(|e| refuse(format!("could not start `{}`: {e}", self.c.program())))?;
                self.pool.insert(s.clone());
                (s, None)
            }
        };
        session.begin(cx.out.clone(), cx.task.clone(), dir.display().to_string(), if cx.spec.tune.mode == Mode::Read { ReadOnly::Explicit } else { ReadOnly::No });
        let (run, driver) = (session.clone(), session.clone());
        let out = cx.out.clone();
        let (spec_resume, announced) = (resume, known.is_some());
        std::thread::spawn(move || {
            let sid = match known {
                Some(sid) => sid,
                None => match driver.open(&dir, spec_resume.as_deref()) {
                    Ok((sid, loaded)) => {
                        if spec_resume.is_some() && !loaded {
                            out.send(RunEv::Notice("the session could not be loaded: this is a new one".into()));
                        }
                        sid
                    }
                    Err(e) => {
                        if e != GONE {
                            driver.inner.fail(e);
                            driver.stop();
                        }
                        return;
                    }
                },
            };
            if !announced {
                out.send(RunEv::Session(sid.clone()));
            }
            driver.prompt(&sid, &text);
        });
        Ok(Box::new(AcpRun { id: self.c.id.clone(), session: run }))
    }
}

struct AcpRun {
    id: String,
    session: Arc<Session>,
}

impl Run for AcpRun {
    fn ctl(&mut self, ctl: RunCtl) -> Result<(), Problem> {
        match ctl {
            RunCtl::Answer { local, answer } => self.session.answer(&local, &answer).map_err(|e| Problem::refused(&self.id, e)),
            RunCtl::Abort => {
                self.session.cancel();
                Ok(())
            }
            RunCtl::Kill => {
                self.session.stop();
                Ok(())
            }
            RunCtl::Told { .. } => Ok(()),
            RunCtl::Steer(_) => Err(Problem::refused(&self.id, "the protocol has no steer: the run is stopped and started again")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_question_at_the_head_is_put_once_however_many_ways_there_are_to_put_it() {
        let inner = Inner {
            id: "agent".into(),
            next: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
            turn: Mutex::new(None),
            asks: Mutex::new(VecDeque::new()),
            session: Mutex::new(None),
            activity: Mutex::new(None),
            ready: AtomicBool::new(false),
            dead: AtomicBool::new(false),
            stdin: Mutex::new(None),
            decided: Mutex::new(HashMap::new()),
        };
        let (out, seen) = Emit::channel();
        *lock(&inner.turn) = Some(Open { out, para: String::new(), said: None, kinds: HashMap::new(), cost: 0.0, plan_left: 0, cancelled: false, task: "t1".into(), site: "/p".into(), read_only: ReadOnly::No, refused: Vec::new(), told: 0 });
        let options = json!([{ "optionId": "allow", "name": "Allow once", "kind": "allow_once" }]);
        // the agent's request puts the question; the answer before it, which has replied and now puts the head, comes after it
        inner.permission(json!(1), &json!({ "toolCall": { "toolCallId": "c1", "title": "rm -rf x", "kind": "execute", "rawInput": { "command": "rm -rf x" } }, "options": options }));
        inner.put_head();
        inner.put_head();
        let asked = seen.try_iter().filter(|e| matches!(e, RunEv::Ask(_))).count();
        assert_eq!(asked, 1, "one question, put once");
    }
}
