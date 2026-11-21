//! The session pool: children found by their session, shared only when launched the same way, bounded, reaped, never orphaned.

use j4n_connect::pool::{Pool, Pooled, Reuse, signature};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct Fake {
    id: Option<String>,
    launch: String,
    cwd: PathBuf,
    dead: AtomicBool,
    idle: AtomicBool,
    last: Mutex<Instant>,
    killed: AtomicBool,
}

impl Fake {
    fn new(id: &str, launch: &str) -> Arc<Fake> {
        Arc::new(Fake {
            id: Some(id.into()),
            launch: launch.into(),
            cwd: std::env::temp_dir(),
            dead: AtomicBool::new(false),
            idle: AtomicBool::new(true),
            last: Mutex::new(Instant::now()),
            killed: AtomicBool::new(false),
        })
    }

    fn at(self: &Arc<Self>, ago: Duration) -> Arc<Self> {
        *self.last.lock().unwrap() = Instant::now() - ago;
        self.clone()
    }

    fn killed(&self) -> bool {
        self.killed.load(Ordering::SeqCst)
    }
}

impl Pooled for Fake {
    fn session_id(&self) -> Option<String> {
        self.id.clone()
    }

    fn launch(&self) -> &str {
        &self.launch
    }

    fn cwd(&self) -> &Path {
        &self.cwd
    }

    fn is_dead(&self) -> bool {
        self.dead.load(Ordering::SeqCst)
    }

    fn is_idle(&self) -> bool {
        self.idle.load(Ordering::SeqCst) && !self.is_dead()
    }

    fn last_activity(&self) -> Instant {
        *self.last.lock().unwrap()
    }

    fn kill(&self) {
        self.killed.store(true, Ordering::SeqCst);
    }
}

fn pool() -> Pool<Fake> {
    Pool::new(2, Duration::from_secs(600))
}

fn dir() -> PathBuf {
    std::env::temp_dir()
}

#[test]
fn a_session_is_found_by_its_id() {
    let p = pool();
    let a = Fake::new("a", "x");
    p.insert(a.clone());
    assert!(p.find("a").is_some_and(|f| Arc::ptr_eq(&f, &a)));
    assert!(p.find("b").is_none() && p.len() == 1 && !p.is_empty());
}

#[test]
fn over_its_size_the_pool_sends_away_the_idle_child_that_waited_longest() {
    let p = pool();
    let (old, mid, new) = (Fake::new("old", "x").at(Duration::from_secs(90)), Fake::new("mid", "x").at(Duration::from_secs(30)), Fake::new("new", "x"));
    p.insert(old.clone());
    p.insert(mid.clone());
    p.insert(new.clone());
    assert!(old.killed() && !mid.killed() && !new.killed());
    assert!(p.find("old").is_none() && p.find("mid").is_some() && p.len() == 2);
}

#[test]
fn a_child_with_a_turn_going_is_never_sent_away_even_over_the_size() {
    let p = pool();
    let (a, b, c) = (Fake::new("a", "x"), Fake::new("b", "x"), Fake::new("c", "x"));
    for f in [&a, &b, &c] {
        f.idle.store(false, Ordering::SeqCst);
        p.insert(f.clone());
    }
    assert!(!a.killed() && !b.killed() && !c.killed() && p.len() == 3, "all three work: nobody is anyone's to send away");
}

#[test]
fn reaping_sends_away_the_long_idle_the_dead_and_the_homeless_and_keeps_the_rest() {
    let p = Pool::new(9, Duration::from_secs(600));
    let long = Fake::new("long", "x").at(Duration::from_secs(601));
    let busy_long = Fake::new("busy", "x").at(Duration::from_secs(900));
    busy_long.idle.store(false, Ordering::SeqCst);
    let dead = Fake::new("dead", "x");
    dead.dead.store(true, Ordering::SeqCst);
    let fresh = Fake::new("fresh", "x");
    let homeless = Arc::new(Fake { cwd: PathBuf::from("/no/such/folder"), ..Arc::into_inner(Fake::new("home", "x")).unwrap() });
    for f in [&long, &busy_long, &dead, &fresh, &homeless] {
        p.insert(f.clone());
    }
    p.reap(Instant::now());
    assert!(long.killed() && dead.killed() && homeless.killed());
    assert!(!busy_long.killed() && !fresh.killed(), "a child on a turn is not idle, however long it has gone quiet");
    assert_eq!(p.len(), 2);
}

#[test]
fn a_child_is_shared_only_when_launched_the_same_way_in_the_same_folder_and_idle() {
    let p = pool();
    let same = Fake::new("s", "model=a");
    p.insert(same.clone());
    let d = dir();
    assert!(matches!(p.reusable("s", "model=a", &d), Reuse::Live(f) if Arc::ptr_eq(&f, &same)));
    assert!(matches!(p.reusable("nobody", "model=a", &d), Reuse::None));
    same.idle.store(false, Ordering::SeqCst);
    assert!(matches!(p.reusable("s", "model=a", &d), Reuse::Busy), "somebody's task has it");
    assert!(!same.killed(), "a busy child is not ours to kill");
    same.idle.store(true, Ordering::SeqCst);
    // other settings: the child goes, so no second owner of the session lingers
    assert!(matches!(p.reusable("s", "model=b", &d), Reuse::Replaced(w) if w.contains("other settings")));
    assert!(same.killed() && p.find("s").is_none());
    // another folder
    let p = pool();
    p.insert(Fake::new("s", "model=a"));
    assert!(matches!(p.reusable("s", "model=a", Path::new("/")), Reuse::Replaced(_)));
}

#[test]
fn a_dead_child_is_dropped_and_its_session_is_resumed_from_its_transcript() {
    let p = pool();
    let f = Fake::new("s", "x");
    p.insert(f.clone());
    f.dead.store(true, Ordering::SeqCst);
    assert!(matches!(p.reusable("s", "x", &dir()), Reuse::Replaced("gone")));
    assert!(p.find("s").is_none());
}

#[test]
fn forgetting_hands_the_child_back_without_killing_it_and_kill_all_kills_every_one() {
    let p = pool();
    let (a, b) = (Fake::new("a", "x"), Fake::new("b", "x"));
    p.insert(a.clone());
    p.insert(b.clone());
    let got = p.forget("a").expect("it was there");
    assert!(Arc::ptr_eq(&got, &a) && !a.killed() && p.forget("a").is_none());
    p.kill_all();
    assert!(b.killed() && p.is_empty());
}

#[test]
fn the_signature_leaves_out_the_session_a_turn_resumes_and_keeps_a_tokens_value_out_of_sight() {
    let argv = |extra: &[&str]| -> Vec<String> { ["claude", "-p", "--model", "haiku"].iter().chain(extra).map(|s| s.to_string()).collect() };
    let env = |t: &str| BTreeMap::from([("CLAUDE_CODE_OAUTH_TOKEN".to_string(), t.to_string())]);
    let d = Path::new("/p");
    let base = signature(&argv(&[]), d, &env("tok-1"));
    assert_eq!(base, signature(&argv(&["--resume", "abc"]), d, &env("tok-1")), "the session asked for is the question, not the settings");
    assert_ne!(base, signature(&argv(&["--effort", "high"]), d, &env("tok-1")));
    assert_ne!(base, signature(&argv(&[]), Path::new("/q"), &env("tok-1")));
    assert_ne!(base, signature(&argv(&[]), d, &env("tok-2")), "a new token is a new child");
    let other = BTreeMap::from([("OTHER_NAME".to_string(), "tok-1".to_string())]);
    assert_ne!(base, signature(&argv(&[]), d, &other), "the same value under another name is another environment");
    assert!(!base.contains("tok-1"), "{base}");
    assert_ne!(signature(&argv(&["--resume", "abc", "--effort", "high"]), d, &env("tok-1")), base, "what follows the resumed id still counts");
}

#[test]
fn a_dead_child_does_not_count_toward_the_size_and_is_dropped_when_the_next_comes_in() {
    let p = pool();
    let (a, b) = (Fake::new("a", "x"), Fake::new("b", "x"));
    p.insert(a.clone());
    p.insert(b.clone());
    b.dead.store(true, Ordering::SeqCst);
    assert_eq!((p.len(), p.alive()), (2, 1));
    let c = Fake::new("c", "x");
    p.insert(c.clone());
    assert!(p.find("b").is_none() && !a.killed() && !c.killed(), "the dead one made room: nobody else had to go");
    assert_eq!((p.len(), p.alive()), (2, 2));
}
