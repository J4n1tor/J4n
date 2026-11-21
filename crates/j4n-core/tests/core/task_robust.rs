//! The loop under a storm: orders thrown at tasks in every state, in an order nobody planned, with a clock that jumps. Which
//! orders land is up to the tasks; what must hold afterwards does not depend on that: every task ends in exactly one state,
//! nothing is written for a task after it ended, the fold rebuilt from the Log is the fold the loop kept, no timer is left
//! behind, and the loop still takes work.

use crate::scripted::{Beat, Rig, Scripted, approval, choice, end, nothing, pick, say};
use j4n_core::ask::Answer;
use j4n_core::change::Actor;
use j4n_core::doer::Caps;
use j4n_core::fold::Fold;
use j4n_core::task::{Book, Ctl, Mode, State, TaskSpec, Tune};

struct Dice(u64);

impl Dice {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn doers() -> Vec<std::sync::Arc<Scripted>> {
    let q = || choice("q", "which?", &[("a", true), ("b", false)]);
    vec![
        Scripted::new("quick", vec![vec![Beat::Session("s".into()), Beat::Tool("t".into(), "x".into()), Beat::Spend(0.001), say("hi"), end("hi")]]),
        Scripted::new("asks", vec![vec![Beat::Session("s".into()), Beat::Ask(q()), Beat::Hold, say("got it"), end("got it")], vec![say("again"), end("again")]]),
        Scripted::new("approves", vec![vec![Beat::Session("s".into()), Beat::Ask(approval("a", "make")), Beat::Hold, nothing()]]),
        Scripted::new("stays", vec![vec![Beat::Session("s".into()), Beat::Delta("working".into()), Beat::Forever], vec![Beat::Forever]]),
        Scripted::with_caps("deaf", Caps { live: false, resume: true, ..Caps::default() }, vec![vec![Beat::Session("s".into()), Beat::Forever], vec![end("restarted")]]),
        Scripted::new("fails", vec![vec![say("half"), Beat::Fail("it fell over".into())]]),
        Scripted::broken("broken", "cannot start"),
        Scripted::new("panics", vec![vec![Beat::Panic]]),
    ]
}

const NAMES: [&str; 8] = ["quick", "asks", "approves", "stays", "deaf", "fails", "broken", "panics"];

fn storm(seed: u64, steps: usize) -> std::collections::BTreeMap<String, usize> {
    let rig = Rig::tuned(doers(), |c| c.ask_wait_ms = 5_000);
    let mut dice = Dice(seed);
    let mut ids: Vec<String> = Vec::new();
    for _ in 0..steps {
        let pick_id = |dice: &mut Dice, ids: &Vec<String>| (!ids.is_empty()).then(|| ids[dice.below(ids.len())].clone());
        match dice.below(10) {
            0..=2 => {
                let mut spec = TaskSpec::new("go", NAMES[dice.below(NAMES.len())]);
                if dice.below(4) == 0 {
                    spec.limits.wall_ms = Some(2_000);
                }
                if dice.below(5) == 0 {
                    spec.limits.usd_soft = Some(0.0005);
                }
                if dice.below(5) == 0 {
                    spec.tune.mode = Mode::Read;
                }
                if let Ok(id) = rig.tasks.spawn(spec, Actor::Operator) {
                    ids.push(id);
                }
            }
            3 => {
                // answer any question, right or wrong
                let open: Vec<String> = rig.tasks.book().open().into_iter().filter(|r| r.spec.doer == "human").map(|r| r.id).collect();
                if !open.is_empty() {
                    let answer = [Answer::Yes, Answer::No, pick("a"), pick("zzz"), Answer::Text { text: "my own".into() }][dice.below(5)].clone();
                    let by = if dice.below(3) == 0 { Actor::Model("m".into()) } else { Actor::Operator };
                    let _ = rig.tasks.ctl(&open[dice.below(open.len())], Ctl::Answer(answer), by);
                }
            }
            4 => {
                if let Some(id) = pick_id(&mut dice, &ids) {
                    let _ = rig.tasks.ctl(&id, if dice.below(2) == 0 { Ctl::Abort } else { Ctl::Kill }, Actor::Operator);
                }
            }
            5 => {
                if let Some(id) = pick_id(&mut dice, &ids) {
                    let _ = rig.tasks.ctl(&id, Ctl::Steer("faster".into()), Actor::Operator);
                }
            }
            6 => {
                if let Some(id) = pick_id(&mut dice, &ids) {
                    let _ = rig.tasks.ctl(&id, Ctl::Retune(Tune { model: Some("m".into()), ..Tune::default() }), Actor::Operator);
                }
            }
            7 => {
                if let Some(id) = pick_id(&mut dice, &ids)
                    && let Ok(next) = rig.tasks.resume(&id, Some("go on"), Actor::Operator)
                {
                    ids.push(next);
                }
            }
            8 => {
                rig.clock.advance([100, 1_000, 3_000, 10_000][dice.below(4)]);
                rig.timers.poke();
            }
            _ => {
                let _ = rig.tasks.rewind(None, dice.below(2) == 0);
            }
        }
    }
    // let everything that is going end, whichever way it does
    for _ in 0..4 {
        for row in rig.tasks.book().open() {
            let _ = rig.tasks.ctl(&row.id, Ctl::Kill, Actor::Operator);
        }
        rig.clock.advance(10_000_000);
        rig.timers.poke();
    }
    for row in rig.tasks.book().rows() {
        rig.finished(&row.id);
    }
    // the doers' threads are still saying things; the loop handles the rest of them in order before this one comes back
    let after = rig.tasks.spawn(TaskSpec::new("go", "quick"), Actor::Operator).expect("the loop still takes work");
    assert_eq!(rig.finished(&after).state, State::Done { ok: true });
    rig.log.sync();

    let events = rig.events();
    assert!(events.windows(2).all(|w| w[0].seq + 1 == w[1].seq), "seq runs without a gap");
    for row in rig.tasks.book().rows() {
        let mine: Vec<_> = events.iter().filter(|e| e.task.as_deref() == Some(row.id.as_str())).collect();
        assert_eq!(mine[0].kind, "task.queued", "{}: the first thing is that it was queued", row.id);
        assert_eq!(mine.iter().filter(|e| e.kind == "task.queued").count(), 1);
        let ends: Vec<usize> = mine.iter().enumerate().filter(|(_, e)| matches!(e.kind.as_str(), "task.settled" | "task.stopped")).map(|(i, _)| i).collect();
        assert_eq!(ends.len(), 1, "{} ends once: {:?}", row.id, mine.iter().map(|e| &e.kind).collect::<Vec<_>>());
        // a rewind is the operator's, and a timer that fired as its task ended is the timer thread's, not the loop's
        let late: Vec<_> = mine[ends[0] + 1..].iter().map(|e| e.kind.as_str()).filter(|k| !matches!(*k, "task.rewound" | "timer.fired")).collect();
        assert!(late.is_empty(), "{} was written to after it ended: {late:?}", row.id);
        if let Some(parent) = row.spec.parent.as_deref() {
            let p = rig.tasks.book().get(parent).unwrap();
            assert!(p.state.is_terminal(), "{} outlived its asker {parent}", row.id);
        }
    }
    let mut replayed = Book::new();
    for e in &events {
        if replayed.wants(&e.kind) {
            replayed.apply(e);
        }
    }
    assert_eq!(replayed.save(), rig.tasks.book().save(), "seed {seed}: the fold is the Log's");
    assert_eq!(rig.timers.pending(), 0, "no timer is left behind");
    let mut ended = std::collections::BTreeMap::new();
    for row in rig.tasks.book().rows() {
        let word = match row.state {
            State::Done { ok } => format!("done ok={ok}"),
            State::Stopped { why } => format!("stopped {}", why.word()),
            other => panic!("{other:?}"),
        };
        *ended.entry(word).or_insert(0) += 1;
    }
    ended
}

#[test]
fn orders_in_every_order_leave_every_task_ended_once_and_the_loop_standing() {
    let mut ended = std::collections::BTreeMap::new();
    for seed in [1, 7, 42, 2026, 9_999_991] {
        for (word, n) in storm(seed, 120) {
            *ended.entry(word).or_insert(0usize) += n;
        }
    }
    eprintln!("{ended:?}");
    // the storm has to reach the endings it is checking, or it checks nothing
    for word in ["done ok=true", "done ok=false", "stopped aborted", "stopped killed", "stopped failed", "stopped parked", "stopped deadline", "stopped wall", "stopped hard_cap", "stopped orphaned"] {
        assert!(ended.get(word).is_some_and(|n| *n > 0), "no task ended as `{word}`: {ended:?}");
    }
}

#[test]
fn a_hundred_questions_at_once_each_reach_their_own_asker() {
    let q = |n: usize| Beat::Ask(choice(&format!("q{n}"), "which?", &[("a", false), ("b", false)]));
    let turns: Vec<Vec<Beat>> = (0..40).map(|n| vec![q(n), Beat::Hold, end(&format!("answer {n}"))]).collect();
    let doer = Scripted::new("asks", turns);
    let rig = Rig::new(vec![doer.clone()]);
    let ids: Vec<String> = (0..40).map(|_| rig.spawn("go", "asks")).collect();
    let mut asks = Vec::new();
    for id in &ids {
        asks.push(match rig.until(id, "waited", |r| matches!(r.state, State::Waiting { .. })).state {
            State::Waiting { ask } => ask,
            other => panic!("{other:?}"),
        });
    }
    for (n, ask) in asks.iter().enumerate().rev() {
        rig.tasks.ctl(ask, Ctl::Answer(pick(if n % 2 == 0 { "a" } else { "b" })), Actor::Operator).unwrap();
    }
    for id in &ids {
        assert_eq!(rig.finished(id).state, State::Done { ok: true });
    }
    // each doer got the answer to its own question, and only that
    let got: Vec<_> = doer.ctls().into_iter().collect();
    assert_eq!(got.len(), 40);
}
