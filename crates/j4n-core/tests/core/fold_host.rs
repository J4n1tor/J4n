//! The fold host and the gate: a fold started from a snapshot and fed what came after ends where a fold fed
//! everything ends.

use crate::fold_cases::folds_of;
use crate::world::*;
use j4n_core::change::Actor;
use j4n_core::clock::{Clock, ManualClock};
use j4n_core::eval::Scope as Env;
use j4n_core::expr::Expr;
use j4n_core::fold::{Fold, Folds, declared};
use j4n_core::log::{Ev, Log, New, Opts, Scope, Sink};
use j4n_core::redact::Redactor;
use j4n_core::value::{Key, Value};
use serde_json::{Value as Json, json};
use std::fs;
use std::sync::Arc;

const FOLDS: &str = r#"
api = 1
[[fold]]
id = "calls"
on = "run.*"
reduce = "count"
key = "{task}"
[[fold]]
id = "spend"
on = "run.cost"
reduce = "sum"
of = "data.usd"
unit = "usd"
key = "{task}"
[[fold]]
id = "recent"
on = "run.cost"
reduce = "sum"
of = "data.usd"
unit = "usd"
window = 5
[[fold]]
id = "busy"
on = "run.*"
reduce = "count"
within = "30s"
key = "{by}"
[[fold]]
id = "fails"
on = "task.settled"
reduce = "streak"
of = "data.ok == false"
key = "{task}"
[[fold]]
id = "kinds"
on = "*"
reduce = "distinct"
of = "kind"
[[fold]]
id = "slowest"
on = "run.shell"
reduce = "max"
of = "data.ms"
unit = "dur"
[[fold]]
id = "tags"
on = "run.*"
reduce = "set"
of = "data.tag"
"#;

const KINDS: [&str; 6] = ["run.cost", "run.shell", "run.message", "task.settled", "task.started", "note.added"];

fn declared_folds() -> Vec<Box<dyn Fold>> {
    let (t, problems) = load(vec![pack("p", FOLDS)]);
    assert!(problems.is_empty(), "{problems:?}");
    let (folds, p) = declared(&t, Scope::Project);
    assert!(p.is_empty(), "{p:?}");
    assert_eq!(folds.len(), 8);
    folds
}

/// `n` events from a fixed random walk, appended to `log` with the clock moving between them.
fn fill(log: &Log, clock: &ManualClock, n: usize, mut seed: u64) {
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..n {
        clock.advance(next() % 4000);
        let kind = KINDS[(next() % KINDS.len() as u64) as usize];
        let by = match next() % 3 {
            0 => Actor::Operator,
            1 => Actor::Rule("r1".into()),
            _ => Actor::Doer("claude".into()),
        };
        let mut e = New::of(
            kind,
            by,
            json!({"usd": (next() % 1000) as f64 / 100.0, "ok": next() % 2 == 0, "ms": next() % 5000, "tag": format!("g{}", next() % 7)}),
        );
        if next() % 5 != 0 {
            e = e.task(&format!("t{}", next() % 4));
        }
        log.append(e).unwrap();
    }
}

fn ids() -> Vec<String> {
    declared_folds().iter().map(|f| f.id().to_string()).collect()
}

/// What a host says: every fold's saved state and its value at a few moments.
fn picture(host: &Folds, end: u64) -> Vec<(String, Json, Vec<String>)> {
    ids()
        .into_iter()
        .map(|id| {
            let state = host.state(&id).expect("the fold is in the host");
            let reads = [0, end / 2, end, end + 10_000, end + 40_000].iter().map(|&now| format!("{:?}", host.read(&id, &[], now))).collect();
            (id, state, reads)
        })
        .collect()
}

fn host_with_folds(dir: &Scratch, head: u64) -> Folds {
    let host = Folds::new(&dir.join("folds"));
    for f in declared_folds() {
        let problems = host.add(f, head);
        assert!(problems.is_empty(), "{problems:?}");
    }
    host
}

#[test]
fn snapshot_plus_replay_equals_full_replay() {
    for seed in [0x9E37_79B9_7F4A_7C15u64, 0x2545_F491_4F6C_DD1D, 12345, 777_777_777] {
        let s = Scratch::new("host-gate");
        let (log, clock) = open_log(&s);
        fill(&log, &clock, 400, seed);
        let events = all(&log);
        assert_eq!(events.len(), 400);
        let end = events.last().unwrap().at_ms;

        // The reference: every fold fed every event.
        let reference = host_with_folds(&s, 0);
        reference.attach(&log).unwrap();
        let want = picture(&reference, end);
        assert!(want.iter().any(|(_, st, _)| st.to_string().len() > 50), "the folds hold something");

        for k in [0usize, 1, 2, 7, 100, 199, 399, 400] {
            let t = Scratch::new("host-gate-case");
            // A host that went through the first k events and wrote its snapshots …
            let before = host_with_folds(&t, 0);
            let mut sink = before.clone();
            events[..k].iter().for_each(|e| sink.event(e));
            assert!(before.snapshot().is_empty());
            // … and a new one, started from them, fed the rest from the log.
            let after = host_with_folds(&t, log.head());
            for id in ids() {
                assert_eq!(after.applied(&id), Some(k as u64), "{id}: started from the snapshot at {k}");
            }
            after.attach(&log).unwrap();
            for id in ids() {
                assert_eq!(after.applied(&id), Some(400), "{id}");
            }
            assert_eq!(picture(&after, end), want, "seed {seed}: from a snapshot at {k}");
        }
    }
}

#[test]
fn folds_that_start_at_different_places_all_end_in_the_same_place() {
    let s = Scratch::new("host-mixed");
    let (log, clock) = open_log(&s);
    fill(&log, &clock, 120, 21);
    let events = all(&log);
    let end = events.last().unwrap().at_ms;
    // One fold has a snapshot at 90; the others are new.
    let first = Folds::new(&s.join("folds"));
    for f in declared_folds().into_iter().filter(|f| f.id() == "calls") {
        first.add(f, 0);
    }
    let mut sink = first.clone();
    events[..90].iter().for_each(|e| sink.event(e));
    assert!(first.snapshot().is_empty());
    let host = host_with_folds(&s, log.head());
    assert_eq!((host.applied("calls"), host.applied("spend")), (Some(90), Some(0)));
    host.attach(&log).unwrap();
    let fresh = Scratch::new("host-mixed-fresh");
    let reference = host_with_folds(&fresh, 0);
    reference.attach(&log).unwrap();
    assert_eq!(picture(&host, end), picture(&reference, end));
}

#[test]
fn the_logs_own_checkpoints_write_snapshots_and_a_restart_goes_on_from_them() {
    let s = Scratch::new("host-checkpoint");
    let clock = Arc::new(ManualClock::at(1_000));
    let o = Opts { checkpoint_every: 10, ..opts(&clock) };
    let (log, _) = Log::open(&s.join(".j4n"), Scope::Project, o, Redactor::new()).unwrap();
    let host = host_with_folds(&s, 0);
    host.attach(&log).unwrap();
    fill(&log, &clock, 95, 99);
    let dir = s.join("folds");
    let snaps = |_: ()| fs::read_dir(&dir).unwrap().flatten().count();
    assert_eq!(snaps(()), 8, "one snapshot a fold, rewritten in place");
    let seq_of = |id: &str| {
        let f = fs::read_dir(&dir).unwrap().flatten().find(|e| e.file_name().to_string_lossy().starts_with(&format!("{id}.v"))).unwrap();
        serde_json::from_slice::<Json>(&fs::read(f.path()).unwrap()).unwrap()["seq"].as_u64().unwrap()
    };
    assert_eq!(seq_of("calls"), 90, "the last checkpoint");
    log.sync();
    assert_eq!(seq_of("calls"), 95);
    let want = picture(&host, 1_000 + 95 * 4000);
    drop(host);
    drop(log);

    // A restart: the snapshots are at 95, and the folds start there.
    let (log, _) = Log::open(&s.join(".j4n"), Scope::Project, opts(&clock), Redactor::new()).unwrap();
    let host = host_with_folds(&s, log.head());
    for id in ids() {
        assert_eq!(host.applied(&id), Some(95));
    }
    host.attach(&log).unwrap();
    assert_eq!(picture(&host, 1_000 + 95 * 4000), want);
    fill(&log, &clock, 5, 5);
    let fresh = Scratch::new("host-checkpoint-fresh");
    let all_again = host_with_folds(&fresh, 0);
    all_again.attach(&log).unwrap();
    let end = all(&log).last().unwrap().at_ms;
    assert_eq!(picture(&host, end), picture(&all_again, end));
}

#[test]
fn a_changed_declaration_is_another_version_and_replays() {
    let s = Scratch::new("host-version");
    let (log, clock) = open_log(&s);
    fill(&log, &clock, 60, 3);
    let (old, _) = folds_of("on = \"run.*\"\nreduce = \"count\"", Scope::Project);
    let host = Folds::new(&s.join("folds"));
    host.add(old.into_iter().next().unwrap(), 0);
    host.attach(&log).unwrap();
    let before = host.read("f", &[], 0);
    assert!(host.snapshot().is_empty());
    let old_file: Vec<String> = fs::read_dir(s.join("folds")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(old_file.len(), 1);

    let (new, _) = folds_of("on = \"task.*\"\nreduce = \"count\"", Scope::Project);
    let host = Folds::new(&s.join("folds"));
    let problems = host.add(new.into_iter().next().unwrap(), log.head());
    assert!(problems.is_empty(), "there is no snapshot of this version, which is not a problem: {problems:?}");
    assert_eq!(host.applied("f"), Some(0));
    host.attach(&log).unwrap();
    assert_ne!(host.read("f", &[], 0), before);
    let expect = all(&log).iter().filter(|e| e.kind.starts_with("task.")).count() as f64;
    assert_eq!(host.read("f", &[], 0), Value::num(expect));
    host.snapshot();
    let now: Vec<String> = fs::read_dir(s.join("folds")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(now.len(), 1, "the snapshot of the old version is gone: {now:?}");
    assert_ne!(now, old_file);
}

#[test]
fn a_snapshot_past_the_end_of_the_log_is_not_used() {
    // The log lost its unsynced tail in a power cut; a snapshot that had run ahead must not stand.
    let s = Scratch::new("host-ahead");
    let (log, clock) = open_log(&s);
    fill(&log, &clock, 10, 8);
    let host = host_with_folds(&s, 0);
    host.attach(&log).unwrap();
    host.snapshot();
    drop(log);
    let seg = s.join(".j4n/log/0001.jsonl");
    let kept: Vec<String> = fs::read_to_string(&seg).unwrap().lines().take(6).map(String::from).collect();
    fs::write(&seg, kept.join("\n") + "\n").unwrap();
    let (log, _) = Log::open(&s.join(".j4n"), Scope::Project, opts(&clock), Redactor::new()).unwrap();
    assert_eq!(log.head(), 6);
    let again = Folds::new(&s.join("folds"));
    let mut told = Vec::new();
    for f in declared_folds() {
        told.extend(again.add(f, log.head()));
    }
    assert_eq!(told.len(), 8, "{told:?}");
    assert!(told[0].msg.contains("the log ends at 6"), "{}", told[0]);
    assert_eq!(again.applied("calls"), Some(0));
    again.attach(&log).unwrap();
    let fresh = Scratch::new("host-ahead-fresh");
    let reference = host_with_folds(&fresh, 0);
    reference.attach(&log).unwrap();
    let end = all(&log).last().unwrap().at_ms;
    assert_eq!(picture(&again, end), picture(&reference, end));
}

#[test]
fn an_unusable_snapshot_means_a_replay_and_says_why() {
    let s = Scratch::new("host-bad-snapshot");
    let (log, clock) = open_log(&s);
    fill(&log, &clock, 20, 4);
    let make = || {
        let (f, _) = folds_of("on = \"run.*\"\nreduce = \"count\"\nkey = \"{task}\"", Scope::Project);
        f.into_iter().next().unwrap()
    };
    let host = Folds::new(&s.join("folds"));
    host.add(make(), 0);
    host.attach(&log).unwrap();
    host.snapshot();
    let file = fs::read_dir(s.join("folds")).unwrap().flatten().next().unwrap().path();
    let good = fs::read_to_string(&file).unwrap();
    let want = host.state("f").unwrap();
    let with = |text: &str, key: &str, value: Json| {
        let mut v: Json = serde_json::from_str(text).unwrap();
        v[key] = value;
        v.to_string()
    };
    for (name, bad, says) in [
        ("garbage", "not json at all".to_string(), "unreadable"),
        ("empty", String::new(), "unreadable"),
        ("state that does not load", good.replace("\"cells\"", "\"nothing\""), "could not be loaded"),
        ("another fold", with(&good, "fold", json!("g")), "another fold or version"),
        ("another version", with(&good, "version", json!(7)), "another fold or version"),
    ] {
        fs::write(&file, &bad).unwrap();
        let h = Folds::new(&s.join("folds"));
        let problems = h.add(make(), log.head());
        assert_eq!(problems.len(), 1, "{name}: {problems:?}");
        assert!(problems[0].msg.contains(says), "{name}: {}", problems[0]);
        assert_eq!(h.applied("f"), Some(0), "{name}");
        h.attach(&log).unwrap();
        assert_eq!(h.state("f").unwrap(), want, "{name}: replayed to the same state");
    }
}

#[test]
fn a_snapshot_with_a_field_a_newer_version_added_is_still_used() {
    let s = Scratch::new("host-newer-snapshot");
    let (log, clock) = open_log(&s);
    fill(&log, &clock, 20, 6);
    let host = Folds::new(&s.join("folds"));
    for f in declared_folds() {
        host.add(f, 0);
    }
    host.attach(&log).unwrap();
    host.snapshot();
    for entry in fs::read_dir(s.join("folds")).unwrap().flatten() {
        let mut v: Json = serde_json::from_slice(&fs::read(entry.path()).unwrap()).unwrap();
        v["added_later"] = json!({"anything": [1, 2]});
        v["state"]["also_later"] = json!(true);
        fs::write(entry.path(), v.to_string()).unwrap();
    }
    let again = Folds::new(&s.join("folds"));
    for f in declared_folds() {
        let problems = again.add(f, log.head());
        assert!(problems.is_empty(), "{problems:?}");
    }
    assert_eq!(again.applied("calls"), Some(20));
}

struct Plain {
    id: String,
    n: u64,
    boom: bool,
    /// `load` takes the state and then fails: a half-loaded fold.
    half: bool,
}

impl Fold for Plain {
    fn id(&self) -> &str {
        &self.id
    }

    fn version(&self) -> u64 {
        1
    }

    fn wants(&self, _: &str) -> bool {
        true
    }

    fn apply(&mut self, _: &Ev) {
        assert!(!self.boom, "a fold that cannot cope");
        self.n += 1;
    }

    fn read(&self, _: &[Key], _: u64) -> Value {
        Value::num(self.n as f64)
    }

    fn save(&self) -> Json {
        json!(self.n)
    }

    fn load(&mut self, state: Json) -> Result<(), String> {
        self.n = state.as_u64().ok_or("not a number")?;
        if self.half {
            self.n += 1000;
            return Err("went wrong half way".into());
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.n = 0;
    }
}

fn plain(id: &str) -> Box<dyn Fold> {
    Box::new(Plain { id: id.into(), n: 0, boom: false, half: false })
}

#[test]
fn a_fold_whose_load_failed_half_way_starts_again_from_nothing() {
    let s = Scratch::new("host-half");
    let (log, _) = open_log(&s);
    for _ in 0..4 {
        log.append(ev("a.b", json!(1))).unwrap();
    }
    let first = Folds::new(&s.join("folds"));
    first.add(plain("half"), 0);
    first.attach(&log).unwrap();
    first.snapshot();
    let again = Folds::new(&s.join("folds"));
    let problems = again.add(Box::new(Plain { id: "half".into(), n: 0, boom: false, half: true }), log.head());
    assert!(problems[0].msg.contains("could not be loaded"), "{problems:?}");
    assert_eq!(again.applied("half"), Some(0));
    assert_eq!(again.read("half", &[], 0), Value::num(0.0), "what the failed load left behind is cleared");
}

#[test]
fn a_fold_that_panics_is_dropped_and_the_others_and_the_log_go_on() {
    let s = Scratch::new("host-panic");
    let (log, _) = open_log(&s);
    let host = Folds::new(&s.join("folds"));
    host.add(Box::new(Plain { id: "bomb".into(), n: 0, boom: true, half: false }), 0);
    host.add(plain("fine"), 0);
    host.attach(&log).unwrap();
    for _ in 0..3 {
        log.append(ev("a.b", json!(1))).unwrap();
    }
    assert_eq!(host.ids(), ["fine"]);
    assert_eq!(host.read("fine", &[], 0), Value::num(3.0));
    assert_eq!(host.read("bomb", &[], 0), Value::Missing);
    let p = host.problems();
    assert_eq!(p.len(), 1, "{p:?}");
    assert!(p[0].at.contains("bomb") && p[0].msg.contains("panicked"), "{}", p[0]);
}

#[test]
fn a_fold_added_after_the_host_is_attached_catches_up_and_then_follows() {
    let s = Scratch::new("host-late");
    let (log, clock) = open_log(&s);
    let host = Folds::new(&s.join("folds"));
    host.add(plain("early"), 0);
    host.attach(&log).unwrap();
    fill(&log, &clock, 12, 1);
    host.add(plain("late"), log.head());
    assert!(host.catch_up(&log).is_empty());
    fill(&log, &clock, 3, 2);
    assert_eq!(host.read("early", &[], 0), Value::num(15.0));
    assert_eq!(host.read("late", &[], 0), Value::num(15.0));
    assert_eq!(host.applied("late"), Some(15));
}

#[test]
fn a_fold_added_to_an_attached_host_takes_no_live_event_before_it_has_caught_up() {
    let s = Scratch::new("host-late-gap");
    let (log, clock) = open_log(&s);
    let host = Folds::new(&s.join("folds"));
    host.attach(&log).unwrap();
    fill(&log, &clock, 12, 1);
    host.add(plain("late"), log.head());
    fill(&log, &clock, 3, 2);
    assert_eq!(host.applied("late"), Some(0), "an event past the ones it has not seen is not its next");
    assert!(host.catch_up(&log).is_empty());
    assert_eq!(host.read("late", &[], 0), Value::num(15.0));
    assert_eq!(host.applied("late"), Some(15));
    fill(&log, &clock, 2, 3);
    assert_eq!(host.read("late", &[], 0), Value::num(17.0), "after the catch-up it follows the log");
}

/// Rounds of the race: the writer is `at` events in when the fold is added, so the catch-up meets a log that is being
/// written at a different place each time. One round caught the old host about one run in ten.
#[test]
fn adding_a_fold_while_events_are_written_loses_none_and_counts_none_twice() {
    for round in 0..50u64 {
        let s = Scratch::new(&format!("host-late-race-{round}"));
        let (log, _) = open_log(&s);
        let host = Folds::new(&s.join("folds"));
        host.add(plain("early"), 0);
        host.attach(&log).unwrap();
        let writer = {
            let log = log.clone();
            std::thread::spawn(move || (0..400).for_each(|i| drop(log.append(ev("a.b", json!(i))).unwrap())))
        };
        let at = 1 + round * 7;
        while log.head() < at {
            std::thread::yield_now();
        }
        host.add(plain("late"), log.head());
        host.catch_up(&log);
        writer.join().unwrap();
        assert_eq!(host.read("early", &[], 0), Value::num(400.0), "round {round}");
        assert_eq!(host.read("late", &[], 0), Value::num(400.0), "round {round}");
    }
}

#[test]
fn odd_ids_make_safe_file_names_that_do_not_collide() {
    let s = Scratch::new("host-names");
    let host = Folds::new(&s.join("folds"));
    for id in ["a/b:c", "a%2Fb:c", "../x", "plain.id-1_"] {
        host.add(plain(id), 0);
    }
    assert!(host.snapshot().is_empty());
    let mut names: Vec<String> = fs::read_dir(s.join("folds")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names, ["%2E%2E%2Fx.v1.json", "a%252Fb%3Ac.v1.json", "a%2Fb%3Ac.v1.json", "plain.id-1_.v1.json"].map(|n| n.replace("%2E%2E", "..")));
    assert!(names.iter().all(|n| !n.contains('/')));
}

#[test]
fn removing_a_fold_takes_it_out() {
    let s = Scratch::new("host-remove");
    let host = Folds::new(&s.join("folds"));
    host.add(plain("a"), 0);
    host.add(plain("b"), 0);
    assert!(host.remove("a"));
    assert!(!host.remove("a"));
    assert_eq!(host.ids(), ["b"]);
    host.add(plain("b"), 0);
    assert_eq!(host.ids(), ["b"], "the same id replaces");
}

#[test]
fn folds_read_as_a_root_of_an_expression() {
    let s = Scratch::new("host-expr");
    let (log, clock) = open_log(&s);
    let host = host_with_folds(&s, 0);
    host.attach(&log).unwrap();
    for _ in 0..3 {
        log.append(ev("run.shell", json!({})).task("t1")).unwrap();
    }
    log.append(ev("run.shell", json!({"ms": 1500})).task("t2")).unwrap();
    clock.advance(1);
    let scope: Env = host.scope(clock.now_ms());
    let holds = |src: &str, scope: &Env| Expr::parse(src).unwrap().holds(scope);
    assert!(holds("fold.calls.t1 == 3 and fold.calls.t2 == 1", &scope));
    assert!(holds("fold.slowest == 1500ms", &scope), "a duration compares with a duration");
    assert!(holds("fold.calls[task] == 3", &scope.clone().with("task", Value::text("t1"))));
    assert!(holds("fold.kinds == 1", &scope));
    assert!(!holds("fold.calls.nobody == 0", &scope), "a key that is not there is not zero");
    assert!(!holds("fold.nosuchfold == 0", &scope));
}

#[test]
fn problems_include_what_a_fold_says_about_itself() {
    let s = Scratch::new("host-notes");
    let host = Folds::new(&s.join("folds"));
    let (mut f, _) = folds_of("on = \"x.y\"\nreduce = \"distinct\"\nof = \"data\"", Scope::Project);
    let mut fold = f.pop().unwrap();
    for i in 0..10_005u64 {
        fold.apply(&crate::fold_cases::event(i + 1, 1, "x.y", None, json!(i)));
    }
    host.add(fold, 0);
    let p = host.problems();
    assert!(p.iter().any(|p| p.at == "fold f" && p.msg.contains("values")), "{p:?}");
}
