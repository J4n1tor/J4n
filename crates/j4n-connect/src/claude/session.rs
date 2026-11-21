//! A live `claude` child: `claude -p --input-format stream-json --output-format stream-json
//! --verbose --include-partial-messages`, kept alive across turns. One JSON line on stdin is a user message (the first of
//! a turn, or a steer in the middle of one); the `result` frame ends the turn and the child stays for the next.
//!
//! The session speaks through the [`Emit`] of the turn that is going on. A question the child asks (`can_use_tool`) blocks
//! it until a `control_response` carries the answer; several may be asked at once (parallel tool calls), and are put to
//! the loop one at a time, in the order they came.

use super::frames::{Line, Pending, Translator, Turn};
use crate::pool::Pooled;
use crate::proc::{self, Child, Exit};
use j4n_core::ask::{Answer, AskSpec};
use j4n_core::doer::{Emit, RunEv};
use j4n_core::place::Launch;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ChildStdin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

/// The turn that is going on, and what has been said to its [`Emit`] about the session.
struct Open {
    turn: Turn,
    out: Emit,
    announced: Option<String>,
}

struct Inner {
    translator: Mutex<Translator>,
    turn: Mutex<Option<Open>>,
    session: Mutex<Option<String>>,
    asks: Mutex<VecDeque<Pending>>,
    activity: Mutex<Option<Instant>>,
    ready: AtomicBool,
    dead: AtomicBool,
    /// Where to write that the session was begun lean, once its id is known (a session begun full, or resumed, has none).
    lean_at: Mutex<Option<PathBuf>>,
    /// Why the Gate refused the question with this local id, until the no is sent: the model is told the rule's words.
    told: Mutex<std::collections::HashMap<String, String>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A question joins the queue: the one to put to the loop when it is the only one. Decided in the same hold of the queue as the join.
pub fn joined(asks: &mut VecDeque<Pending>, p: Pending) -> Option<AskSpec> {
    asks.push_back(p);
    if asks.len() == 1 { asks.front_mut().and_then(Pending::spec) } else { None }
}

/// The head was answered: it leaves the queue and the next question is read in the same hold, or one that joined in between is put twice.
pub fn answered(asks: &mut VecDeque<Pending>) -> Option<AskSpec> {
    asks.pop_front();
    asks.front_mut().and_then(Pending::spec)
}

impl Inner {
    fn new(env: &std::collections::BTreeMap<String, String>) -> Inner {
        Inner {
            translator: Mutex::new(Translator::new(env)),
            turn: Mutex::new(None),
            session: Mutex::new(None),
            asks: Mutex::new(VecDeque::new()),
            activity: Mutex::new(None),
            ready: AtomicBool::new(false),
            dead: AtomicBool::new(false),
            lean_at: Mutex::new(None),
            told: Mutex::new(Default::default()),
        }
    }

    fn touch(&self) {
        *lock(&self.activity) = Some(Instant::now());
    }

    /// Say `ev` to the turn that is going on; false when there is none.
    fn say(&self, ev: RunEv) -> bool {
        lock(&self.turn).as_ref().is_some_and(|o| o.out.send(ev))
    }

    /// End the turn, leaving nothing said: whoever held it hears no more.
    fn close(&self) {
        lock(&self.turn).take();
        lock(&self.asks).clear();
    }

    /// End the turn with a failure.
    fn fail(&self, message: String) {
        self.say(RunEv::Error(message));
        self.close();
    }

    /// Put the question at the head of the queue to the loop.
    fn put_head(&self) {
        let spec = lock(&self.asks).front_mut().and_then(Pending::spec);
        if let Some(spec) = spec {
            self.say(RunEv::Ask(spec));
        }
    }

    /// One line of stdout: translate it under the turn that is going on, learn the session, put out what it made, and end
    /// the turn on the `result` frame (after its events: the meter belongs to the turn that just finished).
    fn pump(&self, line: &str) {
        let turn = lock(&self.turn).as_ref().map(|o| o.turn.clone());
        let Line { evs, session, turn_end, ask, cancelled } = lock(&self.translator).line(line, turn.as_ref());
        self.touch();
        if let Some(id) = session {
            *lock(&self.session) = Some(id.clone());
            if let Some(place) = lock(&self.lean_at).take() {
                super::profile::record(&place, &id);
            }
            self.ready.store(true, Ordering::SeqCst);
            let fresh = lock(&self.turn).as_mut().is_some_and(|o| {
                let fresh = o.announced.as_ref() != Some(&id);
                if fresh {
                    o.announced = Some(id.clone());
                }
                fresh
            });
            if fresh {
                self.say(RunEv::Session(id));
            }
        }
        for ev in evs {
            self.say(ev);
        }
        if let Some(id) = cancelled {
            lock(&self.asks).retain(|p| p.request_id != id);
        }
        if let Some(p) = ask {
            let spec = joined(&mut lock(&self.asks), p);
            spec.map(|spec| self.say(RunEv::Ask(spec)));
        }
        if turn_end {
            self.close();
        }
    }

    /// The child is gone. A turn it leaves open fails, with what it wrote to stderr.
    fn exited(&self, exit: Exit) {
        self.dead.store(true, Ordering::SeqCst);
        if exit.killed {
            return self.close();
        }
        let tail = if exit.tail.is_empty() { String::new() } else { format!(": {}", super::frames::clip(&exit.tail.replace('\n', " | "), 300)) };
        self.fail(format!("claude ended before it finished its turn ({}){tail}", exit.how()));
    }
}

/// A live claude child: one process for a whole conversation.
pub struct Session {
    launch: String,
    cwd: PathBuf,
    child: Child,
    stdin: Mutex<Option<ChildStdin>>,
    inner: Arc<Inner>,
}

impl Session {
    /// Start the child. It says nothing until it is sent its first message (claude 2.1.284 does not), so nothing is waited for
    /// here: [`Session::begin`] and [`Session::prompt`] come next, and a child that stays mute is the watchdog's.
    pub fn spawn(launch: &Launch, signature: String, env: &std::collections::BTreeMap<String, String>, lean_at: Option<PathBuf>) -> std::io::Result<Arc<Session>> {
        let inner = Arc::new(Inner::new(env));
        *lock(&inner.lean_at) = lean_at;
        let (i1, i2) = (inner.clone(), inner.clone());
        let (child, stdin) = proc::spawn(launch, true, move |l| i1.pump(l), move |e| i2.exited(e))?;
        Ok(Arc::new(Session { launch: signature, cwd: launch.cwd.clone(), child, stdin: Mutex::new(stdin), inner }))
    }

    /// Open a turn: what the child says goes to `out` until its `result` frame, or its end.
    pub fn begin(&self, turn: Turn, out: Emit) {
        self.inner.touch();
        *lock(&self.inner.turn) = Some(Open { turn, out, announced: None });
    }

    fn write(&self, frame: &Value) -> Result<(), String> {
        if self.is_dead() {
            return Err("the child is gone".into());
        }
        let mut stdin = lock(&self.stdin);
        let Some(stdin) = stdin.as_mut() else { return Err("the child has no input".into()) };
        writeln!(stdin, "{frame}").and_then(|_| stdin.flush()).map_err(|e| format!("could not write to the child: {e}"))
    }

    fn user(&self, text: &str) -> Result<(), String> {
        self.write(&json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "text", "text": text }] } }))
    }

    /// The first message of a turn.
    pub fn prompt(&self, text: &str) -> Result<(), String> {
        self.user(text)
    }

    /// A line in the middle of the turn (claude's streaming input). Refused between turns: a line queued then would look
    /// delivered and land in the next turn.
    pub fn steer(&self, text: &str) -> Result<(), String> {
        if lock(&self.inner.turn).is_none() {
            return Err("the session is between turns".into());
        }
        self.user(text)
    }

    /// Answer the question at the head of the queue, in-band. A permission question takes yes or no. A question of the
    /// `AskUserQuestion` tool takes a pick or a line of text once for each question it asks (the next is put to the loop, and
    /// the child is answered after the last one), and a no tells the model nobody could answer.
    pub fn answer(&self, local: &str, answer: &Answer) -> Result<(), String> {
        let (body, id, routed) = {
            let mut asks = lock(&self.inner.asks);
            let head = asks.front_mut().ok_or("no question is open")?;
            if head.local() != local {
                return Err(format!("`{local}` is not the question that is open"));
            }
            let asking = !head.questions.is_empty();
            let body = match (answer, asking) {
                (Answer::Yes, false) => json!({ "behavior": "allow", "updatedInput": head.allowed_input() }),
                (Answer::No, false) => {
                    let message = match lock(&self.inner.told).remove(local) {
                        Some(why) => format!("this was not allowed: J4n's gate refused it. {why}"),
                        None => "this was not allowed".to_string(),
                    };
                    json!({ "behavior": "deny", "message": message })
                }
                (Answer::Yes | Answer::No, true) | (Answer::Pick { .. } | Answer::Text { .. }, false) => {
                    return Err(if asking { "this is a question: pick an option or give a line of text" } else { "this asks for permission: yes or no" }.into());
                }
                (Answer::Pick { .. } | Answer::Text { .. }, true) => {
                    let value = match answer {
                        Answer::Pick { id } => id
                            .parse::<usize>()
                            .ok()
                            .and_then(|n| head.labels.get(n.wrapping_sub(1)))
                            .cloned()
                            .ok_or_else(|| format!("`{id}` is not one of the offered options"))?,
                        Answer::Text { text } => text.clone(),
                        _ => String::new(),
                    };
                    let asked = head.asked();
                    head.answers.push((asked, value));
                    let more = head.spec().is_some();
                    if more {
                        drop(asks);
                        self.inner.put_head();
                        return Ok(());
                    }
                    json!({ "behavior": "allow", "updatedInput": head.answered_input() })
                }
            };
            (body, head.request_id.clone(), head.routed.is_some() && matches!(answer, Answer::Yes))
        };
        let sent = self.write(&json!({ "type": "control_response", "response": { "subtype": "success", "request_id": id, "response": body } }));
        let next = answered(&mut lock(&self.inner.asks));
        next.map(|spec| self.inner.say(RunEv::Ask(spec)));
        if routed && sent.is_ok() {
            self.inner.say(RunEv::Notice("a remote shell: its output is run through J4n's redaction (j4n relay) before the model sees it".into()));
        }
        sent
    }

    /// The Gate's reason for a no that comes next to the question `local` (kept short; it is the rule's own text).
    pub fn told(&self, local: &str, why: String) {
        lock(&self.inner.told).insert(local.to_string(), why.chars().take(400).collect());
    }

    /// Stop the turn and the child: the session's transcript stays on disk for `--resume`, the process does not stay.
    pub fn stop(&self) {
        self.inner.close();
        self.child.kill();
        self.inner.dead.store(true, Ordering::SeqCst);
    }

    /// Fail the turn and stop the child (a child that never spoke).
    pub fn give_up(&self, message: String) {
        self.inner.fail(message);
        self.child.kill();
        self.inner.dead.store(true, Ordering::SeqCst);
    }

    /// Whether any frame has named the session yet: the child speaks stream-json.
    pub fn is_ready(&self) -> bool {
        self.inner.ready.load(Ordering::SeqCst)
    }

    /// Whether a turn is going on.
    pub fn in_turn(&self) -> bool {
        lock(&self.inner.turn).is_some()
    }

    pub fn pid(&self) -> u32 {
        self.child.pid
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
        self.is_ready() && !self.in_turn()
    }

    fn last_activity(&self) -> Instant {
        lock(&self.inner.activity).unwrap_or_else(Instant::now)
    }

    fn kill(&self) {
        self.stop();
    }
}
