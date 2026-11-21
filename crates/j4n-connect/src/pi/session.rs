//! A live `pi --mode rpc` or `omp --mode rpc` child: commands as JSON lines on stdin, frames as JSON lines on stdout, kept across
//! turns (protocol v1). `omp` says `ready` at start and
//! `pi` does not (an answered `get_session_stats` is its proof); a command is answered by a `response` frame with its `id`;
//! the turn runs after `prompt` is accepted and ends on a frame of its own ([`super::frames::ends_turn`]). `omp` asks for
//! approvals and puts questions as `extension_ui_request` dialogs, which become questions for the Gate and the operator.

use super::frames::{Translator, approval_title, ends_turn};
use crate::effects;
use crate::manifest::Family;
use crate::pool::Pooled;
use crate::proc::{self, Child, Exit};
use j4n_core::ask::{Answer, AskShape, AskSpec, Opt};
use j4n_core::change::Actor;
use j4n_core::doer::{Emit, RunEv};
use j4n_core::gate::{Action, ReadOnly, Verb};
use j4n_core::place::Launch;
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ChildStdin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// How long a command may take to be answered. `prompt` is accepted at once (the turn runs after), so this is the transport.
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
/// How long the context reading is waited for at the end of a turn: the turn is over, and this only adds to it.
const STATS_TIMEOUT: Duration = Duration::from_secs(2);
/// A shrinking is a call of the model of its own, and gets a longer bound.
const COMPACT_TIMEOUT: Duration = Duration::from_secs(180);
/// How long the start is given to say it is ready.
pub const READY_TIMEOUT: Duration = Duration::from_secs(15);
/// How long an aborted turn is given to end by itself before the child is taken down.
const ABORT_GRACE: Duration = Duration::from_secs(5);
/// What omp is asked to send: the rest of its machinery frames are filtered at the child.
const TRANSLATED: [&str; 11] = [
    "agent_start",
    "agent_end",
    "turn_start",
    "turn_end",
    "message_start",
    "message_update",
    "message_end",
    "tool_execution_start",
    "tool_execution_end",
    "error",
    "notice",
];

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Ui {
    Approval,
    Select,
    Confirm,
    Text,
}

/// A dialog the child is blocked on.
struct Dialog {
    id: String,
    kind: Ui,
    local: String,
    spec: AskSpec,
}

/// The turn that is going on.
struct Open {
    out: Emit,
    failed: bool,
    task: String,
    site: String,
    read_only: ReadOnly,
}

struct Inner {
    id: String,
    family: Family,
    next: AtomicU64,
    pending: Mutex<HashMap<String, Sender<Result<Value, String>>>>,
    turn: Mutex<Option<Open>>,
    translator: Mutex<Translator>,
    dialogs: Mutex<VecDeque<Dialog>>,
    session: Mutex<Option<String>>,
    activity: Mutex<Option<Instant>>,
    ready_tx: Mutex<Option<Sender<()>>>,
    ready: AtomicBool,
    dead: AtomicBool,
    stdin: Mutex<Option<ChildStdin>>,
    /// How the child ended, in words, once it has.
    ended: Mutex<Option<String>>,
}

impl Inner {
    fn say(&self, ev: RunEv) -> bool {
        let mut guard = lock(&self.turn);
        let Some(turn) = guard.as_mut() else { return false };
        if matches!(ev, RunEv::Error(_)) {
            turn.failed = true;
        }
        turn.out.send(ev)
    }

    fn close(&self) {
        lock(&self.turn).take();
        lock(&self.dialogs).clear();
    }

    fn fail(&self, message: String) {
        self.say(RunEv::Error(message));
        self.close();
    }

    fn write(&self, frame: &Value) -> Result<(), String> {
        if self.dead.load(Ordering::SeqCst) {
            return Err("the child is gone".into());
        }
        let mut stdin = lock(&self.stdin);
        let Some(stdin) = stdin.as_mut() else { return Err("the child has no input".into()) };
        writeln!(stdin, "{frame}").and_then(|_| stdin.flush()).map_err(|e| format!("could not write to the child: {e}"))
    }

    /// A command, with its frame built from `extra`'s fields; the receiver for its `response`.
    fn send(&self, cmd: &str, extra: Value) -> Result<mpsc::Receiver<Result<Value, String>>, String> {
        let id = format!("j4n-{}", self.next.fetch_add(1, Ordering::SeqCst));
        let mut frame = json!({ "id": id, "type": cmd });
        if let (Value::Object(extra), Some(f)) = (extra, frame.as_object_mut()) {
            f.extend(extra);
        }
        let (tx, rx) = mpsc::channel();
        lock(&self.pending).insert(id.clone(), tx);
        if let Err(e) = self.write(&frame) {
            lock(&self.pending).remove(&id);
            return Err(e);
        }
        Ok(rx)
    }

    fn command(&self, cmd: &str, extra: Value, limit: Duration) -> Result<Value, String> {
        let rx = self.send(cmd, extra)?;
        rx.recv_timeout(limit).map_err(|_| format!("{cmd}: no response in {} seconds", limit.as_secs()))?
    }

    fn ui_reply(&self, id: &str, body: Value) {
        let mut frame = json!({ "type": "extension_ui_response", "id": id });
        if let (Value::Object(b), Some(f)) = (body, frame.as_object_mut()) {
            f.extend(b);
        }
        let _ = self.write(&frame);
    }

    fn pump(self: &Arc<Self>, line: &str) {
        *lock(&self.activity) = Some(Instant::now());
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            let l = lock(&self.translator).line(line);
            for e in l.evs {
                self.say(e);
            }
            return;
        };
        match v.get("type").and_then(Value::as_str).unwrap_or_default() {
            "ready" => {
                self.ready.store(true, Ordering::SeqCst);
                if let Some(tx) = lock(&self.ready_tx).take() {
                    let _ = tx.send(());
                }
            }
            "response" => self.response(&v),
            "extension_ui_request" => self.dialog(&v),
            "rpc_chunk" => {
                self.say(RunEv::Notice("rpc: a chunked frame arrived on protocol v1 and was ignored".into()));
            }
            _ => {
                let l = lock(&self.translator).frame(&v);
                if let Some(id) = l.session {
                    *lock(&self.session) = Some(id);
                }
                for e in l.evs {
                    self.say(e);
                }
                if ends_turn(self.family, &v) {
                    self.end_turn();
                }
            }
        }
    }

    /// The turn is over: its answer is what was said last, unless it already failed. A dialog that comes from here on has no turn to
    /// ask through. Before the run says it has ended, the child is asked how full its context is — on a thread of its own, for the
    /// answer is read on this one.
    fn end_turn(self: &Arc<Self>) {
        let said = lock(&self.translator).said();
        let open = lock(&self.turn).take();
        lock(&self.dialogs).clear();
        lock(&self.translator).forget();
        let Some(open) = open.filter(|t| !t.failed) else { return };
        let inner = self.clone();
        std::thread::spawn(move || {
            if let Some(w) = inner.window() {
                open.out.send(RunEv::Window(w));
            }
            open.out.send(RunEv::End { answer: said, todo_left: 0 });
        });
    }

    /// How full the context is, from 0 to 1, when the child says.
    fn window(&self) -> Option<f64> {
        self.command("get_session_stats", json!({}), STATS_TIMEOUT).ok().and_then(|stats| context_fraction(&stats))
    }

    fn response(&self, v: &Value) {
        let command = v.get("command").and_then(Value::as_str).unwrap_or("command").to_string();
        let ok = v.get("success").and_then(Value::as_bool).unwrap_or(false);
        let error = v.get("error").and_then(Value::as_str).unwrap_or("unknown failure").to_string();
        if let Some(tx) = v.get("id").and_then(Value::as_str).and_then(|id| lock(&self.pending).remove(id)) {
            let _ = tx.send(if ok { Ok(v.clone()) } else { Err(format!("{command}: {error}")) });
        }
        // a refused prompt is the error of the call that sent it (and runs no turn, so no frame will end one)
        if !ok && command != "prompt" {
            self.say(RunEv::Notice(format!("rpc {command}: {error}")));
        }
        if command == "prompt" && v.pointer("/data/agentInvoked").and_then(Value::as_bool) == Some(false) {
            self.fail("the line was taken as a command of the program and ran no turn".into());
        }
    }

    /// A dialog of omp's UI. pi has none worth answering, and any dialog nobody can answer is cancelled at once.
    fn dialog(&self, v: &Value) {
        let method = v.get("method").and_then(Value::as_str).unwrap_or_default();
        if !matches!(method, "select" | "confirm" | "input" | "editor") {
            return;
        }
        let Some(id) = v.get("id").and_then(Value::as_str).map(String::from) else { return };
        let title = v.get("title").and_then(Value::as_str).unwrap_or("question").to_string();
        let options: Vec<String> = v.get("options").and_then(Value::as_array).into_iter().flatten().filter_map(|o| o.as_str().map(String::from)).collect();
        let has = |w: &str| options.iter().any(|o| o == w);
        let (task, site, read_only) = lock(&self.turn).as_ref().map(|t| (t.task.clone(), t.site.clone(), t.read_only)).unwrap_or_default();
        let approval = (self.family == Family::Omp && method == "select" && has("Approve") && has("Deny")).then(|| approval_title(&title)).flatten();
        let turning = lock(&self.turn).is_some();
        let (kind, spec) = match (approval, self.family, turning) {
            (Some((tool, target)), _, true) => {
                let (verb, risk) = match tool.as_str() {
                    "bash" => (Verb::Shell, 60),
                    "read" => (Verb::Read, 10),
                    _ => (Verb::Write, 40),
                };
                let what = if target.is_empty() { tool.clone() } else { target.clone() };
                let mut act = Action::new(verb, Actor::Doer(self.id.clone()), &what).task(&task);
                if matches!(verb, Verb::Read | Verb::Write) {
                    act.inside = Some(effects::inside(Path::new(&site), &what));
                }
                act.site = Some(site);
                act.read_only = read_only;
                let question = if tool == "bash" && !target.is_empty() { format!("run `{target}`?") } else { format!("allow {tool}: {what}?") };
                (Ui::Approval, AskSpec { local: id.clone(), question, shape: AskShape::Approval { action: format!("{tool}: {what}"), risk }, inband: true, point: None, act: Some(Box::new(act)) })
            }
            // an approval with no turn to ask through is refused, never left hanging and never waved through
            (Some(_), _, false) => return self.ui_reply(&id, json!({ "value": "Deny" })),
            (None, Family::Omp, true) => {
                let (kind, shape, question) = match method {
                    "select" => (Ui::Select, AskShape::Choice { options: options.iter().map(|o| Opt { id: o.clone(), label: o.clone(), recommended: false }).collect() }, title),
                    "confirm" => {
                        let m = v.get("message").and_then(Value::as_str).filter(|m| !m.is_empty());
                        (Ui::Confirm, AskShape::YesNo, m.map_or(title.clone(), |m| format!("{title}\n{m}")))
                    }
                    _ => (Ui::Text, AskShape::Text, title),
                };
                (kind, AskSpec { local: id.clone(), question, shape, inband: true, point: None, act: None })
            }
            _ => return self.ui_reply(&id, json!({ "cancelled": true })),
        };
        let first = {
            let mut q = lock(&self.dialogs);
            q.push_back(Dialog { id: id.clone(), kind, local: id, spec });
            q.len() == 1
        };
        if first {
            self.put_head();
        }
    }

    fn put_head(&self) {
        let spec = lock(&self.dialogs).front().map(|d| d.spec.clone());
        if let Some(spec) = spec {
            self.say(RunEv::Ask(spec));
        }
    }

    fn exited(&self, exit: Exit) {
        self.dead.store(true, Ordering::SeqCst);
        let tail = if exit.tail.is_empty() { String::new() } else { format!(": {}", exit.tail.replace('\n', " | ").chars().take(300).collect::<String>()) };
        let note = format!("{} ended ({}){tail}", self.id, exit.how());
        // said before anyone waiting is woken, so the one who wakes can tell why
        *lock(&self.ended) = Some(note.clone());
        for (_, tx) in lock(&self.pending).drain() {
            let _ = tx.send(Err(GONE.into()));
        }
        lock(&self.ready_tx).take();
        if exit.killed {
            return self.close();
        }
        // a child that ends before it is ready is the start's to report (it may fall back to the print mode)
        if self.ready.load(Ordering::SeqCst) {
            self.fail(format!("{note} before it finished its turn"));
        }
    }
}

/// How full the context window is (0 to 1) out of a `get_session_stats` response: `contextUsage {tokens, contextWindow, percent}`;
/// tokens over the window is exact, and `percent` (0 to 100) is what there is when it is not.
pub fn context_fraction(stats: &Value) -> Option<f64> {
    let usage = stats.pointer("/data/contextUsage")?;
    let num = |k: &str| usage.get(k).and_then(Value::as_f64).filter(|n| n.is_finite());
    match (num("tokens"), num("contextWindow")) {
        (Some(t), Some(w)) if w > 0.0 => Some((t / w).clamp(0.0, 1.0)),
        _ => num("percent").map(|p| (p / 100.0).clamp(0.0, 1.0)),
    }
}

/// What a command is told when the child ends under it.
pub const GONE: &str = "the child ended";

pub struct Session {
    launch: String,
    cwd: PathBuf,
    child: Child,
    inner: Arc<Inner>,
}

impl Session {
    pub fn spawn(id: &str, family: Family, launch: &Launch, signature: String) -> std::io::Result<Arc<Session>> {
        let inner = Arc::new(Inner {
            id: id.to_string(),
            family,
            next: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
            turn: Mutex::new(None),
            translator: Mutex::new(Translator::new()),
            dialogs: Mutex::new(VecDeque::new()),
            session: Mutex::new(None),
            activity: Mutex::new(None),
            ready_tx: Mutex::new(None),
            ready: AtomicBool::new(false),
            dead: AtomicBool::new(false),
            stdin: Mutex::new(None),
            ended: Mutex::new(None),
        });
        let (i1, i2) = (inner.clone(), inner.clone());
        let (child, stdin) = proc::spawn(launch, true, move |l| i1.pump(l), move |e| i2.exited(e))?;
        *lock(&inner.stdin) = stdin;
        Ok(Arc::new(Session { launch: signature, cwd: launch.cwd.clone(), child, inner }))
    }

    /// Wait for omp's `ready` frame (pi sends none); then ask the child to name its session. Pi's answer is also its proof that
    /// it speaks RPC at all, so a child that cannot answer is not one.
    pub fn handshake(&self) -> Result<String, String> {
        if self.inner.family == Family::Omp {
            let (tx, rx) = mpsc::channel();
            *lock(&self.inner.ready_tx) = Some(tx);
            if !self.inner.ready.load(Ordering::SeqCst) && rx.recv_timeout(READY_TIMEOUT).is_err() {
                return Err(self.why_not_ready(format!("{} never said it was ready in {} seconds", self.inner.id, READY_TIMEOUT.as_secs())));
            }
            let _ = self.inner.command("set_event_filter", json!({ "events": TRANSLATED }), COMMAND_TIMEOUT);
        }
        let stats = self.inner.command("get_session_stats", json!({}), READY_TIMEOUT).map_err(|e| self.why_not_ready(if e == GONE { String::new() } else { format!("{} is not speaking rpc: {e}", self.inner.id) }))?;
        self.inner.ready.store(true, Ordering::SeqCst);
        let id = stats.pointer("/data/sessionId").and_then(Value::as_str).ok_or("get_session_stats carried no sessionId")?.to_string();
        *lock(&self.inner.session) = Some(id.clone());
        Ok(id)
    }

    /// Why the child did not come up: how it ended if it did, else `otherwise`.
    fn why_not_ready(&self, otherwise: String) -> String {
        lock(&self.inner.ended).clone().map_or(otherwise, |n| format!("{n} before it was ready"))
    }

    pub fn begin(&self, out: Emit, task: String, site: String, read_only: ReadOnly) {
        *lock(&self.inner.activity) = Some(Instant::now());
        *lock(&self.inner.turn) = Some(Open { out, failed: false, task, site, read_only });
    }

    /// Shrink the session: the child is told what the summary must keep, and the run ends with its account of what it did. The
    /// answer takes a model call, so it is waited for on a thread of its own.
    pub fn compact(&self, keep: &str) {
        let (inner, keep) = (self.inner.clone(), keep.to_string());
        std::thread::spawn(move || match inner.command("compact", json!({ "customInstructions": keep }), COMPACT_TIMEOUT) {
            Ok(res) => {
                let at = |p: &str| res.pointer(&format!("/data/{p}")).and_then(Value::as_u64);
                let after = at("estimatedTokensAfter").or_else(|| at("tokensAfter")).filter(|n| *n > 0);
                // what the session holds now, in tokens, as the Log's size of it: the compaction call itself is not what it holds
                if let Some(a) = after {
                    inner.say(RunEv::Metrics(j4n_core::doer::Metrics { last: a, ..Default::default() }));
                }
                let note = match (at("tokensBefore"), after) {
                    (Some(b), Some(a)) => format!("{b} → {a} tokens"),
                    (Some(b), None) => format!("{b} tokens summarised"),
                    _ => "compacted".into(),
                };
                if let Some(w) = inner.window() {
                    inner.say(RunEv::Window(w));
                }
                inner.say(RunEv::End { answer: Some(note), todo_left: 0 });
                inner.close();
            }
            Err(e) => inner.fail(e),
        });
    }

    /// Start the turn: the prompt is accepted at once and the rest is frames.
    pub fn prompt(&self, text: &str) -> Result<(), String> {
        self.inner.command("prompt", json!({ "message": text }), COMMAND_TIMEOUT).map(drop)
    }

    /// A line into the turn that is going on. Not waited for: the response is read on the other thread.
    pub fn steer(&self, text: &str) -> Result<(), String> {
        if lock(&self.inner.turn).is_none() {
            return Err("the session is between turns".into());
        }
        self.inner.send("steer", json!({ "message": text })).map(drop)
    }

    /// Answer the dialog at the head of the queue: an approval takes yes or no, a choice a pick, a yes/no a yes or a no, a
    /// text a line (a no cancels it). A kind of answer the dialog cannot take is refused and it stays open.
    pub fn answer(&self, local: &str, answer: &Answer) -> Result<(), String> {
        let mut dialogs = lock(&self.inner.dialogs);
        let head = dialogs.front().ok_or("no question is open")?;
        if head.local != local {
            return Err(format!("`{local}` is not the question that is open"));
        }
        let body = match (head.kind, answer) {
            (Ui::Approval, Answer::Yes) => json!({ "value": "Approve" }),
            (Ui::Approval, Answer::No) => json!({ "value": "Deny" }),
            (Ui::Confirm, Answer::Yes) => json!({ "confirmed": true }),
            (Ui::Confirm, Answer::No) => json!({ "confirmed": false }),
            (Ui::Select, Answer::Pick { id }) => json!({ "value": id }),
            (Ui::Text, Answer::Text { text }) => json!({ "value": text }),
            (Ui::Select | Ui::Text, Answer::No) => json!({ "cancelled": true }),
            (Ui::Approval, _) => return Err("this asks for permission: yes or no".into()),
            (Ui::Select, _) => return Err("this is a choice: pick one of the options, or say no to cancel".into()),
            (Ui::Confirm, _) => return Err("this is a yes or no".into()),
            (Ui::Text, _) => return Err("this asks for text, or no to cancel".into()),
        };
        let id = head.id.clone();
        dialogs.pop_front();
        drop(dialogs);
        self.inner.ui_reply(&id, body);
        self.inner.put_head();
        Ok(())
    }

    /// Abort the turn: the child stays for the next one; a turn that does not end by itself is taken down after a grace.
    pub fn abort(self: &Arc<Self>) {
        let open: Vec<Dialog> = lock(&self.inner.dialogs).drain(..).collect();
        for d in open {
            self.inner.ui_reply(&d.id, json!({ "cancelled": true }));
        }
        if self.inner.send("abort", json!({})).is_err() {
            return self.stop();
        }
        let weak = Arc::downgrade(self);
        std::thread::spawn(move || {
            std::thread::sleep(ABORT_GRACE);
            if let Some(s) = weak.upgrade().filter(|s| s.in_turn()) {
                s.stop();
            }
        });
    }

    pub fn stop(&self) {
        self.inner.close();
        self.child.kill();
        self.inner.dead.store(true, Ordering::SeqCst);
    }

    pub fn fail(&self, message: String) {
        self.inner.fail(message);
    }

    pub fn in_turn(&self) -> bool {
        lock(&self.inner.turn).is_some()
    }

    pub fn notice(&self, text: &str) {
        self.inner.say(RunEv::Notice(text.to_string()));
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
