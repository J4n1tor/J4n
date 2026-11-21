//! The timer thread: a timer is due, it is written to the Log as `timer.fired`, and not before.

use crate::world::*;
use j4n_core::clock::{Clock, ManualClock};
use j4n_core::log::{Log, Opts, Scope, Sub};
use j4n_core::redact::Redactor;
use j4n_core::timer::{MAX_TIMERS, Timer, Timers};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

fn rig(tag: &str) -> (Scratch, Log, Arc<ManualClock>, Timers, Sub) {
    let s = Scratch::new(tag);
    let (log, clock) = open_log(&s);
    let timers = Timers::start(&log);
    let sub = log.subscribe("timer.*", 100);
    (s, log, clock, timers, sub)
}

fn fired(sub: &Sub) -> Option<Arc<j4n_core::log::Ev>> {
    sub.recv_timeout(Duration::from_secs(5))
}

fn nothing(sub: &Sub) -> bool {
    sub.recv_timeout(Duration::from_millis(150)).is_none()
}

#[test]
fn a_timer_fires_when_due_and_not_before_and_what_it_says_is_in_the_log() {
    let (_s, log, clock, timers, sub) = rig("timer-basic");
    let id = timers.after(500, Timer::new("ask-deadline").task("t1").cause(3).data(json!({"ask": "a1"}))).unwrap();
    assert_eq!(timers.pending(), 1);
    clock.advance(499);
    timers.poke();
    assert!(nothing(&sub), "not yet");
    clock.advance(1);
    timers.poke();
    let ev = fired(&sub).expect("fired");
    assert_eq!(ev.kind, "timer.fired");
    assert_eq!(ev.data, json!({"id": id, "tag": "ask-deadline", "due_ms": 1_500, "data": {"ask": "a1"}}));
    assert_eq!((ev.task.as_deref(), ev.cause, ev.seq), (Some("t1"), Some(3), 1));
    assert_eq!(ev.by, j4n_core::change::Actor::Core);
    assert_eq!(all(&log).len(), 1, "it is written, not only flashed");
    assert_eq!(timers.pending(), 0);
    assert!(nothing(&sub), "once");
}

#[test]
fn timers_due_together_fire_by_time_and_then_by_when_they_were_set() {
    let (_s, _log, clock, timers, sub) = rig("timer-order");
    let c = timers.at(2_000, Timer::new("c")).unwrap();
    let a = timers.at(1_500, Timer::new("a")).unwrap();
    let b = timers.at(1_500, Timer::new("b")).unwrap();
    assert!(a < b && b > c);
    clock.set(10_000);
    timers.poke();
    let tags: Vec<String> = (0..3).map(|_| fired(&sub).unwrap().data["tag"].as_str().unwrap().to_string()).collect();
    assert_eq!(tags, ["a", "b", "c"]);
}

#[test]
fn a_timer_set_in_the_past_fires_at_once() {
    let (_s, _log, _clock, timers, sub) = rig("timer-past");
    std::thread::sleep(Duration::from_millis(100)); // the thread is asleep, waiting for nothing
    timers.at(1, Timer::new("late")).unwrap();
    assert_eq!(fired(&sub).unwrap().data["tag"], "late");
}

#[test]
fn a_cancelled_timer_does_not_fire_and_cancelling_twice_says_no() {
    let (_s, _log, clock, timers, sub) = rig("timer-cancel");
    let a = timers.after(100, Timer::new("a")).unwrap();
    let b = timers.after(100, Timer::new("b")).unwrap();
    assert!(timers.cancel(a));
    assert!(!timers.cancel(a));
    assert!(!timers.cancel(9999));
    assert_eq!(timers.pending(), 1);
    clock.advance(100);
    timers.poke();
    assert_eq!(fired(&sub).unwrap().data["id"], b);
    assert!(!timers.cancel(b), "it has fired");
    assert!(nothing(&sub));
}

#[test]
fn a_timer_that_is_set_while_the_thread_sleeps_wakes_it() {
    let (_s, _log, clock, timers, sub) = rig("timer-wake");
    timers.after(1_000_000, Timer::new("far")).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    timers.at(clock.now_ms(), Timer::new("near")).unwrap(); // due already, and no poke: only being set wakes the thread
    assert_eq!(fired(&sub).unwrap().data["tag"], "near");
    assert_eq!(timers.pending(), 1);
}

#[test]
fn a_real_clock_fires_a_real_timer() {
    let s = Scratch::new("timer-real");
    let (log, _) = Log::open(&s.join(".j4n"), Scope::Project, Opts::default(), Redactor::new()).unwrap();
    let timers = Timers::start(&log);
    let sub = log.subscribe("timer.*", 10);
    let started = std::time::Instant::now();
    timers.after(60, Timer::new("real")).unwrap();
    assert!(fired(&sub).is_some());
    assert!(started.elapsed() >= Duration::from_millis(55), "{:?}", started.elapsed());
}

#[test]
fn dropping_the_timers_stops_the_thread_and_nothing_pending_fires() {
    let (_s, log, clock, timers, sub) = rig("timer-drop");
    timers.after(10, Timer::new("never")).unwrap();
    drop(timers);
    clock.advance(1_000);
    assert!(nothing(&sub));
    assert_eq!(all(&log).len(), 0);
}

#[test]
fn only_so_many_timers_wait_at_once() {
    let (_s, _log, _clock, timers, _sub) = rig("timer-max");
    for i in 0..MAX_TIMERS {
        timers.at(10_000_000 + i as u64, Timer::new("x")).unwrap();
    }
    let e = timers.at(1, Timer::new("one too many")).unwrap_err();
    assert!(e.to_string().contains("100000 timers"), "{e}");
    assert_eq!(timers.pending(), MAX_TIMERS);
}
