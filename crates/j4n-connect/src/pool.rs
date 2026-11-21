//! The session pool: live children kept between turns, found by the session they carry. One pool serves every doer. A child is shared by a later turn only when it would have been launched
//! exactly as that turn would launch it, in the same folder, and is idle; the pool never holds more than `max` and lets
//! the one longest idle go first; a child idle for `idle` is reaped. Whatever leaves the pool is killed here, so no path
//! leaves an orphan.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

/// Why a session is taken out and launched again: what it was launched with is not what the turn wants.
pub const RELAUNCHED: &str = "launched under other settings";
use std::time::{Duration, Instant};

/// What the pool needs to know of a live child.
pub trait Pooled: Send + Sync {
    fn session_id(&self) -> Option<String>;
    /// How it was launched ([`signature`]).
    fn launch(&self) -> &str;
    fn cwd(&self) -> &Path;
    fn is_dead(&self) -> bool;
    /// Ready (a frame has named its session) and between turns: a child just started is not, so it cannot be sent away before
    /// its first turn begins.
    fn is_idle(&self) -> bool;
    fn last_activity(&self) -> Instant;
    fn kill(&self);
}

/// What a later turn finds for the session it wants to carry on in.
pub enum Reuse<S> {
    /// The child is there, launched the same way, and idle.
    Live(Arc<S>),
    /// A child was there and is gone now (launched otherwise, or dead): start a fresh one on the session's transcript.
    Replaced(&'static str),
    /// A child is there, busy with another task: the session is not free.
    Busy,
    None,
}

pub struct Pool<S: Pooled> {
    live: Mutex<Vec<Arc<S>>>,
    max: usize,
    idle: Duration,
}

/// How a child is launched, in one string: the arguments without the session they resume (that is the question, not the
/// settings), the folder, and the environment's names and a hash of its values (a token is never kept).
pub fn signature(argv: &[String], cwd: &Path, env: &BTreeMap<String, String>) -> String {
    let mut args = Vec::new();
    let mut it = argv.iter();
    while let Some(a) = it.next() {
        if a == "--resume" {
            it.next();
        } else {
            args.push(a.as_str());
        }
    }
    let mut h = Sha256::new();
    for (k, v) in env {
        h.update(k.as_bytes());
        h.update([0]);
        h.update(v.as_bytes());
        h.update([0]);
    }
    let env: String = h.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect();
    format!("{}|{}|{env}", args.join("\u{1f}"), cwd.display())
}

fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

impl<S: Pooled> Pool<S> {
    pub fn new(max: usize, idle: Duration) -> Pool<S> {
        Pool { live: Mutex::new(Vec::new()), max, idle }
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Arc<S>>> {
        self.live.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many of the children held are alive.
    pub fn alive(&self) -> usize {
        self.lock().iter().filter(|s| !s.is_dead()).count()
    }

    /// The child carrying `session`, if one is still open.
    pub fn find(&self, session: &str) -> Option<Arc<S>> {
        self.lock().iter().find(|s| s.session_id().as_deref() == Some(session)).cloned()
    }

    /// Take a child in, sending the idle one that has waited longest away while the pool is over `max`. A child with a turn
    /// going is somebody's task and is never sent away, so the pool may be over its size for as long as they all work.
    pub fn insert(&self, session: Arc<S>) {
        let mut out = Vec::new();
        {
            let mut live = self.lock();
            let (dead, keep): (Vec<_>, Vec<_>) = live.drain(..).partition(|s| s.is_dead());
            out.extend(dead);
            *live = keep;
            live.push(session);
            while live.len() > self.max {
                let victim = live.iter().enumerate().filter(|(_, s)| s.is_idle()).min_by_key(|(_, s)| s.last_activity()).map(|(i, _)| i);
                match victim {
                    Some(i) => out.push(live.remove(i)),
                    None => break,
                }
            }
        }
        out.iter().for_each(|s| s.kill());
    }

    /// Send away the idle children that have waited longer than the pool keeps them, the dead, and the ones whose folder
    /// is gone (a worktree is reaped with its task, and a child in a deleted folder can host nobody's next turn).
    pub fn reap(&self, now: Instant) {
        let gone: Vec<Arc<S>> = {
            let mut live = self.lock();
            let (gone, keep) = live.drain(..).partition(|s| {
                s.is_dead() || (s.is_idle() && (now.saturating_duration_since(s.last_activity()) >= self.idle || !s.cwd().is_dir()))
            });
            *live = keep;
            gone
        };
        gone.iter().for_each(|s| s.kill());
    }

    /// The child a turn may share for `session`: launched exactly as `want` says ([`signature`]), in `cwd`, alive and idle.
    /// One that is there but launched otherwise (the settings were changed) is taken out and killed, so no second owner of
    /// the session lingers.
    pub fn reusable(&self, session: &str, want: &str, cwd: &Path) -> Reuse<S> {
        let Some(found) = self.find(session) else { return Reuse::None };
        if found.is_dead() {
            self.forget(session);
            return Reuse::Replaced("gone");
        }
        if found.launch() != want || canon(found.cwd()) != canon(cwd) {
            if let Some(stale) = self.forget(session) {
                stale.kill();
            }
            return Reuse::Replaced(RELAUNCHED);
        }
        if found.is_idle() { Reuse::Live(found) } else { Reuse::Busy }
    }

    /// Take the entry for `session` out without killing it: the caller owns the handle.
    pub fn forget(&self, session: &str) -> Option<Arc<S>> {
        let mut live = self.lock();
        let at = live.iter().position(|s| s.session_id().as_deref() == Some(session))?;
        Some(live.remove(at))
    }

    /// Kill every child (the front end is going away).
    pub fn kill_all(&self) {
        let all: Vec<Arc<S>> = self.lock().drain(..).collect();
        all.iter().for_each(|s| s.kill());
    }
}
