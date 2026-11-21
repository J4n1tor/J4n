//! MCP over a program's stdio: one JSON document a line each way. The server is a child in a process group of its own; what it
//! writes to stdout is read on a thread, an answer finds the call that waits for it by its id, and a request the server makes of
//! us (`ping`, `roots/list`, `sampling`) is answered at once — `ping` with nothing, the rest with "not supported" — because a
//! client that stays silent hangs the server.

use super::Transport;
use j4n_connect::proc::{self, Child};
use j4n_core::place::Launch;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::Write;
use std::process::ChildStdin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

type Pending = Arc<Mutex<BTreeMap<u64, Sender<Value>>>>;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct Stdio {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    pending: Pending,
    next: AtomicU64,
    /// Why the server is gone, once it is.
    ended: Arc<Mutex<Option<String>>>,
}

fn send(stdin: &Mutex<ChildStdin>, doc: &Value) -> Result<(), String> {
    let mut pipe = lock(stdin);
    writeln!(pipe, "{doc}").and_then(|()| pipe.flush()).map_err(|e| format!("the server cannot be written to: {e}"))
}

impl Stdio {
    pub fn start(launch: &Launch) -> Result<Stdio, String> {
        let pending: Pending = Arc::default();
        let ended: Arc<Mutex<Option<String>>> = Arc::default();
        let stdin_cell: Arc<Mutex<Option<Arc<Mutex<ChildStdin>>>>> = Arc::default();
        let (p, cell) = (pending.clone(), stdin_cell.clone());
        let (p2, e2) = (pending.clone(), ended.clone());
        let (child, pipe) = proc::spawn(
            launch,
            true,
            move |line| {
                let Ok(doc) = serde_json::from_str::<Value>(line) else { return };
                let id = doc.get("id").cloned();
                if doc.get("method").is_some() {
                    // the server asks something of us
                    let (Some(id), Some(stdin)) = (id, lock(&cell).clone()) else { return };
                    let reply = if doc["method"] == "ping" { json!({ "jsonrpc": "2.0", "id": id, "result": {} }) } else { json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "not supported by this client" } }) };
                    let _ = send(&stdin, &reply);
                } else if let Some(n) = id.and_then(|i| i.as_u64())
                    && let Some(tx) = lock(&p).remove(&n)
                {
                    let _ = tx.send(doc);
                }
            },
            move |exit| {
                *lock(&e2) = Some(format!("the server ended ({}){}", exit.how(), if exit.tail.is_empty() { String::new() } else { format!(": {}", exit.tail.lines().last().unwrap_or_default()) }));
                // every call that waits is told by its sender going away
                lock(&p2).clear();
            },
        )
        .map_err(|e| format!("the server could not be started: {e}"))?;
        let stdin = Arc::new(Mutex::new(pipe.ok_or("the server has no input")?));
        *lock(&stdin_cell) = Some(stdin.clone());
        Ok(Stdio { child, stdin, pending, next: AtomicU64::new(1), ended })
    }
}

impl Stdio {
    /// Why the server is gone, when a write to it failed: the pipe breaks a moment before the exit is read, so the reason is waited for
    /// (a second at most) rather than answering "Broken pipe" for a program that was never there.
    fn gone(&self) -> Option<String> {
        for _ in 0..20 {
            if let Some(why) = lock(&self.ended).clone() {
                return Some(why);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }
}

impl Transport for Stdio {
    fn call(&self, method: &str, params: Value, wait: Duration) -> Result<Value, String> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        lock(&self.pending).insert(id, tx);
        if let Some(why) = lock(&self.ended).clone() {
            return Err(why);
        }
        if let Err(e) = send(&self.stdin, &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })) {
            return Err(self.gone().unwrap_or(e));
        }
        match rx.recv_timeout(wait) {
            Ok(doc) => match doc.get("error") {
                Some(e) => Err(format!("{method}: {}", e.get("message").and_then(Value::as_str).unwrap_or("the server refused"))),
                None => Ok(doc.get("result").cloned().unwrap_or(Value::Null)),
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {
                lock(&self.pending).remove(&id);
                Err(format!("{method}: the server did not answer in {} s", wait.as_secs()))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(lock(&self.ended).clone().unwrap_or_else(|| "the server ended".into())),
        }
    }

    fn notify(&self, method: &str, params: Value) {
        let _ = send(&self.stdin, &json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    fn close(&self) {
        self.child.kill();
    }
}
