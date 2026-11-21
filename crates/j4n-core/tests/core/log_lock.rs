//! A log let go and opened again at once is refused ("another J4n has this log open") while a child that was
//! forked with the lock open has not yet reached its `exec`; a lock that is held for good is still refused.

use crate::world::*;
use j4n_core::log::{Log, Scope};
use j4n_core::redact::Redactor;
use std::fs::File;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// A child forked from this process that sits for `ms` before its `exec` (as a child does that is slow to get a core)
/// and holds every descriptor that was open at the fork, the log's lock among them. Returns when the child exists.
fn slow_child(ms: u64) -> JoinHandle<()> {
    let own = |_: ()| -> Vec<String> {
        let mut kids = Vec::new();
        for t in std::fs::read_dir("/proc/self/task").unwrap().flatten() {
            kids.extend(std::fs::read_to_string(t.path().join("children")).unwrap_or_default().split_whitespace().map(String::from));
        }
        kids
    };
    let before = own(());
    let handle = std::thread::spawn(move || {
        let mut c = Command::new("true");
        // SAFETY: the closure only sleeps, between the fork and the exec.
        unsafe {
            c.pre_exec(move || {
                std::thread::sleep(Duration::from_millis(ms));
                Ok(())
            })
        };
        assert!(c.status().unwrap().success());
    });
    let end = Instant::now() + Duration::from_secs(10);
    while own(()).len() <= before.len() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(1));
    }
    handle
}

fn held(s: &Scratch) -> bool {
    let other = File::options().write(true).open(s.join(".j4n/log/.lock")).unwrap();
    matches!(other.try_lock(), Err(std::fs::TryLockError::WouldBlock))
}

fn reopen(s: &Scratch, clock: &std::sync::Arc<j4n_core::clock::ManualClock>) -> Result<Log, String> {
    Log::open(&s.join(".j4n"), Scope::Project, opts(clock), Redactor::new()).map(|(l, _)| l).map_err(|p| p.to_string())
}

#[test]
fn a_log_let_go_while_a_forked_child_has_not_yet_exec_d_is_opened_again() {
    let s = Scratch::new("t116-fork");
    let (log, clock) = open_log(&s);
    let child = slow_child(100);
    drop(log);
    assert!(held(&s), "the child holds the lock the Log let go of (it has not reached its exec)");
    let again = reopen(&s, &clock);
    assert!(again.is_ok(), "{:?}", again.err());
    child.join().unwrap();
}

#[test]
fn a_lock_that_is_held_for_good_is_still_refused_after_the_wait_and_says_why() {
    let s = Scratch::new("t116-held");
    let (_log, clock) = open_log(&s);
    let began = Instant::now();
    let err = reopen(&s, &clock).err().expect("refused").to_string();
    assert!(err.contains("another J4n has this log open"), "{err}");
    let waited = began.elapsed();
    assert!(waited >= Duration::from_millis(300), "it waited for the lock before it refused: {waited:?}");
    assert!(waited < Duration::from_secs(10), "and did not wait for ever: {waited:?}");
}
