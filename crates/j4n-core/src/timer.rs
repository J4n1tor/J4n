//! The one timer thread: when a timer is due it appends `timer.fired` to the Log, and
//! whoever waited for it — an Ask's deadline, a Rule's `after`, a cooling task — reacts to that event. No daemon and no
//! scheduler: the timers live in this process, and what survives a restart is the Log, from which whoever set a timer sets
//! it again (the open asks' deadlines are a fold).
//!
//! The thread sleeps until the earliest timer is due, or at most a minute (so a clock that jumps is noticed). With a
//! [`ManualClock`](crate::clock::ManualClock), move the clock and [`poke`](Timers::poke).

use crate::change::Actor;
use crate::log::{Log, New};
use crate::table::Problem;
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

/// Most timers waiting at once.
pub const MAX_TIMERS: usize = 100_000;
const NAP: u64 = 60_000;

/// What `timer.fired` says: the `tag` (what it is for), the task and cause it was set for, and any `data`.
#[derive(Debug, Clone, Default)]
pub struct Timer {
    pub tag: String,
    pub task: Option<String>,
    pub cause: Option<u64>,
    pub data: Json,
}

impl Timer {
    pub fn new(tag: &str) -> Timer {
        Timer { tag: tag.to_string(), ..Timer::default() }
    }

    pub fn task(mut self, id: &str) -> Timer {
        self.task = Some(id.to_string());
        self
    }

    pub fn cause(mut self, seq: u64) -> Timer {
        self.cause = Some(seq);
        self
    }

    pub fn data(mut self, data: Json) -> Timer {
        self.data = data;
        self
    }
}

#[derive(Default)]
struct Inner {
    next: u64,
    due: BTreeMap<(u64, u64), Timer>,
    stop: bool,
}

type Shared = Arc<(Mutex<Inner>, Condvar)>;

pub struct Timers {
    shared: Shared,
    log: Log,
    join: Option<JoinHandle<()>>,
}

fn lock(shared: &Shared) -> MutexGuard<'_, Inner> {
    shared.0.lock().unwrap_or_else(|e| e.into_inner())
}

impl Timers {
    /// Start the thread. It reads the time from the log's clock and writes to the log.
    pub fn start(log: &Log) -> Timers {
        let shared: Shared = Arc::default();
        let (thread_shared, thread_log) = (shared.clone(), log.clone());
        let join = std::thread::Builder::new().name("j4n-timer".into()).spawn(move || run(thread_shared, thread_log)).ok();
        Timers { shared, log: log.clone(), join }
    }

    /// Fire `timer` at `at_ms`. The number is its id, in `timer.fired` and for [`cancel`](Timers::cancel).
    pub fn at(&self, at_ms: u64, timer: Timer) -> Result<u64, Problem> {
        let mut inner = lock(&self.shared);
        if inner.due.len() >= MAX_TIMERS {
            return Err(Problem::refused("timers", format!("{MAX_TIMERS} timers are waiting already")));
        }
        inner.next += 1;
        let id = inner.next;
        inner.due.insert((at_ms, id), timer);
        self.shared.1.notify_all();
        Ok(id)
    }

    /// Fire `timer` `ms` from now.
    pub fn after(&self, ms: u64, timer: Timer) -> Result<u64, Problem> {
        self.at(self.log.clock().now_ms().saturating_add(ms), timer)
    }

    /// Take a timer back. False when it has fired or never was.
    pub fn cancel(&self, id: u64) -> bool {
        let mut inner = lock(&self.shared);
        let key = inner.due.keys().find(|(_, i)| *i == id).copied();
        key.is_some_and(|k| inner.due.remove(&k).is_some())
    }

    pub fn pending(&self) -> usize {
        lock(&self.shared).due.len()
    }

    /// Look at the clock now (after moving a manual one).
    pub fn poke(&self) {
        drop(lock(&self.shared));
        self.shared.1.notify_all();
    }
}

impl Drop for Timers {
    fn drop(&mut self) {
        lock(&self.shared).stop = true;
        self.shared.1.notify_all();
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn run(shared: Shared, log: Log) {
    let clock = log.clock();
    loop {
        let mut inner = lock(&shared);
        if inner.stop {
            return;
        }
        let now = clock.now_ms();
        let mut fire = Vec::new();
        while let Some(((at, _), _)) = inner.due.first_key_value() {
            if *at > now {
                break;
            }
            if let Some(entry) = inner.due.pop_first() {
                fire.push(entry);
            }
        }
        if fire.is_empty() {
            let wait = inner.due.first_key_value().map_or(NAP, |((at, _), _)| (*at - now).min(NAP));
            let _ = shared.1.wait_timeout(inner, Duration::from_millis(wait));
            continue;
        }
        drop(inner);
        for ((due, id), t) in fire {
            let mut ev = New::of("timer.fired", Actor::Core, json!({ "id": id, "tag": t.tag, "due_ms": due, "data": t.data }));
            ev.task = t.task;
            ev.cause = t.cause;
            let _ = log.append(ev);
        }
    }
}
