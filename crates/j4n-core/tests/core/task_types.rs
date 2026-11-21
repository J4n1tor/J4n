//! The words of Task, Ask and Doer that need no loop: ids, specs, stops and their menus, what makes an answer an answer,
//! and the fold of tasks fed by hand.

use j4n_core::ask::{Answer, AskShape, AskSpec, Opt, TEXT_MAX};
use j4n_core::change::Actor;
use j4n_core::fold::Fold;
use j4n_core::log::Ev;
use j4n_core::schema::DataClass;
use j4n_core::task::{Act, Book, KEEP, State, TaskSpec, Why, id_number, menu, micros};
use j4n_core::value::{Key, Value};
use serde_json::{Value as Json, json};

#[test]
fn a_task_is_t_and_a_number() {
    assert_eq!(id_number("t1"), Some(1));
    assert_eq!(id_number("t120"), Some(120));
    for bad in ["", "t", "t0", "t01", "T1", "t-1", "t+1", "t1 ", " t1", "t1/../x", "../t1", "tt1", "t1.2", "t1234567890123456789", "1"] {
        assert_eq!(id_number(bad), None, "{bad:?}");
    }
}

#[test]
fn money_is_kept_in_millionths_and_a_bad_amount_is_nothing() {
    assert_eq!(micros(0.0042), 4200);
    assert_eq!(micros(1.0), 1_000_000);
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(micros(bad), 0, "{bad}");
    }
    assert!(micros(f64::MAX) > 0, "a huge amount does not wrap");
}

#[test]
fn a_spec_is_checked() {
    let ok = TaskSpec::new("write the report", "claude");
    assert_eq!(ok.check(), Ok(()));
    let bad = |f: &dyn Fn(&mut TaskSpec)| {
        let mut s = ok.clone();
        f(&mut s);
        s.check().expect_err("refused")
    };
    assert!(bad(&|s| s.goal = "  \n".into()).contains("goal"));
    assert!(bad(&|s| s.goal = "x".repeat((1 << 20) + 1)).contains("goal"));
    assert!(bad(&|s| s.doer = String::new()).contains("doer"));
    assert!(bad(&|s| s.place = "a\nb".into()).contains("place"));
    assert!(bad(&|s| s.thread = Some("bell\u{7}".into())).contains("thread"));
    assert!(bad(&|s| s.parent = Some("../t1".into())).contains("not a task"));
    assert!(bad(&|s| s.follows = Some("t0".into())).contains("not a task"));
    assert!(bad(&|s| s.limits.wall_ms = Some(0)).contains("above zero"));
    assert!(bad(&|s| s.limits.usd_soft = Some(0.0)).contains("above zero"));
    assert!(bad(&|s| s.limits.usd_soft = Some(f64::NAN)).contains("above zero"));
    let mut full = ok.clone();
    full.limits.wall_ms = Some(1);
    full.limits.usd_soft = Some(0.01);
    full.parent = Some("t3".into());
    assert_eq!(full.check(), Ok(()));
}

#[test]
fn a_spec_reads_what_an_older_or_newer_writer_left_out_or_added() {
    let spec: TaskSpec = serde_json::from_value(json!({ "goal": "g", "doer": "d", "from_the_future": [1, 2], "tune": { "mode": "read", "newer": true } })).unwrap();
    assert_eq!((spec.place.as_str(), spec.limits.wall_ms, spec.parent.clone()), ("here", None, None));
    assert_eq!(spec.tune.mode, j4n_core::task::Mode::Read);
    let again: TaskSpec = serde_json::from_value(serde_json::to_value(&spec).unwrap()).unwrap();
    assert_eq!(again, spec);
    let plain: TaskSpec = serde_json::from_value(json!({ "goal": "g", "doer": "d" })).unwrap();
    assert_eq!(plain.tune.mode, j4n_core::task::Mode::Write, "work that is not said to be read-only may write");
    let asked: TaskSpec = serde_json::from_value(json!({ "goal": "g", "doer": "human", "input": { "ask": { "local": "q", "question": "ok?", "shape": "yesno", "inband": true } } })).unwrap();
    assert_eq!(asked.ask().map(|a| a.shape), Some(AskShape::YesNo));
    assert_eq!(plain.ask(), None);
}

#[test]
fn the_six_states_and_nine_stops_have_their_words() {
    let states = [
        (State::Queued, "queued"),
        (State::Running, "running"),
        (State::Cooling { until_ms: 5 }, "cooling"),
        (State::Waiting { ask: "t2".into() }, "waiting"),
        (State::Done { ok: false }, "done"),
        (State::Stopped { why: Why::Wall }, "stopped"),
    ];
    for (state, word) in states {
        assert_eq!(state.word(), word);
        assert_eq!(state.is_terminal(), matches!(word, "done" | "stopped"), "{word}");
        let back: State = serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
        assert_eq!(back, state);
    }
    let all = every_why();
    let mut words: Vec<&str> = all.iter().map(Why::word).collect();
    words.sort();
    words.dedup();
    assert_eq!(words.len(), 9);
    for why in all {
        let back: Why = serde_json::from_value(serde_json::to_value(&why).unwrap()).unwrap();
        assert_eq!(back, why);
    }
}

fn every_why() -> Vec<Why> {
    vec![
        Why::Aborted,
        Why::Killed,
        Why::Parked { ask: "t2".into() },
        Why::Deadline,
        Why::Wall,
        Why::HardCap,
        Why::Failed { message: "x".into() },
        Why::Interrupted,
        Why::Orphaned,
    ]
}

fn opts(spec: &[(&str, bool)]) -> Vec<Opt> {
    spec.iter().map(|(id, recommended)| Opt { id: id.to_string(), label: format!("the {id}"), recommended: *recommended }).collect()
}

fn ask_of(shape: AskShape) -> AskSpec {
    AskSpec { local: "q".into(), question: "which?".into(), shape, inband: true, point: None, act: None }
}

#[test]
fn every_stop_that_can_be_carried_on_has_a_menu_that_ends_with_abort() {
    let shapes = [
        None,
        Some(ask_of(AskShape::YesNo)),
        Some(ask_of(AskShape::Approval { action: "make".into(), risk: 30 })),
        Some(ask_of(AskShape::Choice { options: opts(&[("a", true), ("b", true), ("c", false)]) })),
        Some(ask_of(AskShape::Text)),
    ];
    for why in every_why() {
        for ask in &shapes {
            let items = menu(&why, ask.as_ref());
            let none_to_choose = matches!(why, Why::Aborted | Why::Killed | Why::Deadline | Why::Orphaned);
            assert_eq!(items.is_empty(), none_to_choose, "{why:?}");
            if none_to_choose {
                continue;
            }
            assert_eq!(items.last().map(|i| &i.act), Some(&Act::Abort), "{why:?}: {items:?}");
            assert!(items.iter().filter(|i| i.recommended).count() <= 1, "{why:?}: {items:?}");
            let mut ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
            ids.sort();
            ids.dedup();
            assert_eq!(ids.len(), items.len(), "an entry is offered twice: {items:?}");
            assert!(items.iter().all(|i| !i.label.is_empty()));
        }
    }
}

#[test]
fn what_a_parked_menu_offers_follows_the_question() {
    let parked = Why::Parked { ask: "t2".into() };
    let ids = |ask: Option<AskSpec>| menu(&parked, ask.as_ref()).into_iter().map(|i| i.id).collect::<Vec<_>>();
    assert_eq!(ids(None), ["resume", "abort"]);
    assert_eq!(ids(Some(ask_of(AskShape::YesNo))), ["yes", "no", "abort"]);
    assert_eq!(ids(Some(ask_of(AskShape::Text))), ["say", "abort"]);
    assert_eq!(ids(Some(ask_of(AskShape::Choice { options: opts(&[("a", false), ("b", false)]) }))), ["pick:a", "pick:b", "say", "abort"]);
    // an approval: nothing is recommended but the safe answer
    let approval = menu(&parked, Some(&ask_of(AskShape::Approval { action: "rm".into(), risk: 90 })));
    assert_eq!(approval.iter().filter(|i| i.recommended).map(|i| i.id.as_str()).collect::<Vec<_>>(), ["no"]);
    // a doer that recommends two options is heard on the first only
    let two = menu(&parked, Some(&ask_of(AskShape::Choice { options: opts(&[("a", true), ("b", true)]) })));
    assert_eq!(two.iter().filter(|i| i.recommended).map(|i| i.id.as_str()).collect::<Vec<_>>(), ["pick:a"]);
    // a failed task is retried, a task that ran out of time carries on, one that hit the cap is not pushed into it again
    let first = |why: &Why| menu(why, None).into_iter().next().map(|i| (i.id, i.recommended));
    assert_eq!(first(&Why::Failed { message: "x".into() }), Some(("retry".into(), true)));
    assert_eq!(first(&Why::Wall), Some(("resume".into(), true)));
    assert_eq!(first(&Why::Interrupted), Some(("resume".into(), true)));
    assert_eq!(first(&Why::HardCap), Some(("resume".into(), false)));
}

#[test]
fn an_answer_has_to_fit_its_question() {
    let yes = |a: &Answer, s: &AskShape| s.check(a).is_ok();
    let (yn, approval, text) = (AskShape::YesNo, AskShape::Approval { action: "x".into(), risk: 1 }, AskShape::Text);
    let choice = AskShape::Choice { options: opts(&[("a", false), ("b", false)]) };
    let (pick_a, pick_z) = (Answer::Pick { id: "a".into() }, Answer::Pick { id: "z".into() });
    let say = |t: &str| Answer::Text { text: t.into() };
    for s in [&yn, &approval] {
        assert!(yes(&Answer::Yes, s) && yes(&Answer::No, s));
        assert!(!yes(&pick_a, s) && !yes(&say("yes"), s));
    }
    assert!(yes(&pick_a, &choice) && !yes(&pick_z, &choice) && !yes(&Answer::Yes, &choice));
    assert!(!yes(&Answer::Yes, &text) && !yes(&pick_a, &text));
    assert!(choice.check(&pick_z).unwrap_err().contains("a, b"), "the offered options are named");
    for s in [&choice, &text] {
        assert!(yes(&say("a line of my own"), s));
        assert!(!yes(&say(""), s) && !yes(&say("   "), s), "empty");
        assert!(!yes(&say("two\nlines"), s) && !yes(&say("bell\u{7}"), s), "one line, no control characters");
        assert!(yes(&say(&"x".repeat(TEXT_MAX)), s) && !yes(&say(&"x".repeat(TEXT_MAX + 1)), s));
    }
    assert!(yes(&say(&"é".repeat(TEXT_MAX)), &text), "the length is characters, not bytes");
    assert_eq!(pick_a.render(), "a");
    assert_eq!((Answer::Yes.render(), Answer::No.render(), say("hi").render()), ("yes".into(), "no".into(), "hi".into()));
}

#[test]
fn a_question_a_doer_puts_has_to_be_showable() {
    let good = ask_of(AskShape::Choice { options: opts(&[("a", false), ("b", true)]) });
    assert_eq!(good.check(), Ok(()));
    let bad = |f: &dyn Fn(&mut AskSpec)| {
        let mut s = good.clone();
        f(&mut s);
        s.check().expect_err("refused")
    };
    assert!(bad(&|s| s.local = String::new()).contains("name"));
    assert!(bad(&|s| s.local = "x".repeat(65)).contains("name"));
    assert!(bad(&|s| s.question = "  ".into()).contains("question"));
    assert!(bad(&|s| s.question = "q".repeat(4001)).contains("question"));
    assert!(bad(&|s| s.shape = AskShape::Choice { options: vec![] }).contains("options"));
    assert!(bad(&|s| s.shape = AskShape::Choice { options: opts(&[("a", false); 13]) }).contains("options"));
    assert!(bad(&|s| s.shape = AskShape::Choice { options: opts(&[("a", false), ("a", false)]) }).contains("twice"));
    assert!(bad(&|s| s.shape = AskShape::Choice { options: opts(&[("", false)]) }).contains("option"));
    assert!(bad(&|s| s.shape = AskShape::Approval { action: "x".into(), risk: 101 }).contains("risk"));
    // and it survives the Log
    let back: AskSpec = serde_json::from_value(serde_json::to_value(&good).unwrap()).unwrap();
    assert_eq!(back, good);
    let yes_no: AskSpec = serde_json::from_value(json!({ "local": "q", "question": "ok?", "shape": "yesno", "inband": false })).unwrap();
    assert_eq!((yes_no.shape, yes_no.inband), (AskShape::YesNo, false));
}

// ---- the fold, fed by hand ----

fn ev(seq: u64, at_ms: u64, kind: &str, task: &str, data: Json) -> Ev {
    Ev { seq, at_ms, kind: kind.into(), task: Some(task.into()), thread: None, by: Actor::Core, cause: None, class: DataClass::default(), data }
}

fn feed(book: &mut Book, evs: &[Ev]) {
    for e in evs {
        if book.wants(&e.kind) {
            book.apply(e);
        }
    }
}

fn queued(seq: u64, task: &str) -> Ev {
    ev(seq, 100 + seq, "task.queued", task, json!({ "spec": TaskSpec::new(&format!("goal of {task}"), "d") }))
}

#[test]
fn the_fold_follows_a_task_from_queued_to_done() {
    let mut book = Book::new();
    let evs = [
        queued(1, "t1"),
        ev(2, 200, "task.started", "t1", json!({ "doer": "d" })),
        ev(3, 210, "run.session", "t1", json!({ "doer": "d", "id": "s-9" })),
        ev(4, 220, "run.metrics", "t1", json!({ "usd": 0.5, "tokens_in": 7, "tokens_out": 3 })),
        ev(5, 230, "run.metrics", "t1", json!({ "usd": 0.25, "tokens_in": 1, "tokens_out": 1 })),
        ev(6, 240, "task.cap_reached", "t1", json!({})),
        ev(7, 250, "task.settled", "t1", json!({ "ok": true, "answer": "done", "todo_left": 2, "changed": { "created": ["a"], "more": 4 } })),
    ];
    feed(&mut book, &evs[..1]);
    assert_eq!(book.get("t1").unwrap().state, State::Queued);
    feed(&mut book, &evs[1..2]);
    assert_eq!(book.get("t1").unwrap().state, State::Running);
    feed(&mut book, &evs[2..]);
    let row = book.get("t1").unwrap();
    assert_eq!(row.state, State::Done { ok: true });
    assert_eq!((row.answer.as_deref(), row.todo_left, row.spent, row.tokens_in, row.tokens_out), (Some("done"), 2, 750_000, 8, 4));
    assert_eq!((row.queued_ms, row.started_ms, row.ended_ms, row.capped), (101, Some(200), Some(250), true));
    assert_eq!((row.session.map(|s| s.id), row.changed.created, row.changed.more), (Some("s-9".into()), vec!["a".to_string()], 4));
}

#[test]
fn a_finished_task_stays_as_it_finished() {
    let mut book = Book::new();
    feed(&mut book, &[queued(1, "t1"), ev(2, 200, "task.started", "t1", json!({})), ev(3, 300, "task.settled", "t1", json!({ "ok": true, "answer": "a" }))]);
    let late = [
        ev(4, 400, "task.started", "t1", json!({})),
        ev(5, 410, "task.waiting", "t1", json!({ "ask": "t9" })),
        ev(6, 420, "task.resumed", "t1", json!({})),
        ev(7, 430, "task.stopped", "t1", json!({ "why": { "kind": "wall" } })),
        ev(8, 440, "task.settled", "t1", json!({ "ok": false, "answer": "b" })),
    ];
    feed(&mut book, &late);
    let row = book.get("t1").unwrap();
    assert_eq!((row.state, row.answer.as_deref(), row.ended_ms), (State::Done { ok: true }, Some("a"), Some(300)));
    // a task is queued once, whatever the Log says again
    feed(&mut book, &[ev(9, 500, "task.queued", "t1", json!({ "spec": TaskSpec::new("another", "d") }))]);
    assert_eq!(book.get("t1").unwrap().spec.goal, "goal of t1");
}

#[test]
fn the_fold_skips_what_it_does_not_understand() {
    let mut book = Book::new();
    feed(
        &mut book,
        &[
            queued(1, "t1"),
            ev(2, 1, "task.queued", "x7", json!({ "spec": TaskSpec::new("not a task id", "d") })),
            ev(3, 1, "task.queued", "t2", json!({ "spec": "not a spec" })),
            ev(4, 1, "task.queued", "t3", json!({})),
            ev(5, 1, "task.started", "t99", json!({})),
            ev(6, 1, "task.stopped", "t1", json!({ "why": { "kind": "a_reason_from_the_future" } })),
            ev(7, 1, "run.metrics", "t1", json!({ "usd": "a lot" })),
            ev(8, 1, "something.else", "t1", json!({})),
        ],
    );
    assert_eq!(book.rows().len(), 1);
    let row = book.get("t1").unwrap();
    assert!(matches!(row.state, State::Stopped { why: Why::Failed { .. } }), "{:?}", row.state);
    assert_eq!(row.spent, 0);
    assert!(book.get("x7").is_none() && book.get("t2").is_none() && book.get("t99").is_none());
}

#[test]
fn the_fold_is_read_by_task_and_field_and_saves_and_loads_whole() {
    let mut book = Book::new();
    let mut child = TaskSpec::new("which?", "human");
    child.parent = Some("t1".into());
    feed(
        &mut book,
        &[
            queued(1, "t1"),
            ev(2, 10, "task.started", "t1", json!({})),
            ev(3, 11, "task.queued", "t2", json!({ "spec": child })),
            ev(4, 12, "task.waiting", "t1", json!({ "ask": "t2" })),
            ev(5, 13, "run.metrics", "t1", json!({ "usd": 0.5 })),
        ],
    );
    let read = |path: &[&str]| book.read(&path.iter().map(|p| Key::Name(p.to_string())).collect::<Vec<_>>(), 0);
    assert_eq!(read(&["t1", "state"]), Value::text("waiting"));
    assert_eq!(read(&["t1", "ask"]), Value::text("t2"));
    assert_eq!(read(&["t1", "spent"]), Value::Usd(500_000));
    assert_eq!(read(&["t1", "children"]), Value::List(vec![Value::text("t2")]));
    assert_eq!(read(&["t2", "parent"]), Value::text("t1"));
    assert_eq!(read(&["t2", "doer"]), Value::text("human"));
    assert_eq!(read(&["t3", "state"]), Value::Missing);
    assert_eq!(read(&["t1", "nothing"]), Value::Missing);
    assert!(matches!(read(&[]), Value::Map(m) if m.len() == 2));
    let mut other = Book::new();
    other.load(book.save()).unwrap();
    assert_eq!(other.save(), book.save());
    assert_eq!(other.rows(), book.rows());
    assert!(other.load(json!({ "rows": "no" })).is_err());
    other.reset();
    assert!(other.rows().is_empty());
}

#[test]
fn the_fold_keeps_the_newest_tasks_and_says_what_it_left_out() {
    let mut book = Book::new();
    let total = KEEP + 25;
    for n in 1..=total {
        let t = format!("t{n}");
        let seq = (n as u64) * 3;
        feed(&mut book, &[queued(seq, &t), ev(seq + 1, 5, "task.started", &t, json!({})), ev(seq + 2, 6, "task.settled", &t, json!({ "ok": true }))]);
    }
    let rows = book.rows();
    assert_eq!(rows.len(), KEEP);
    assert_eq!(rows[0].id, format!("t{}", total - KEEP + 1), "the oldest went first");
    assert!(book.get("t1").is_none() && book.get(&format!("t{total}")).is_some());
    assert!(book.notes().iter().any(|n| n.contains("25 older")), "{:?}", book.notes());
    // a task that is still going is never the one left out
    let mut busy = Book::new();
    feed(&mut busy, &[queued(1, "t1"), ev(2, 5, "task.started", "t1", json!({}))]);
    for n in 2..=(KEEP + 3) {
        let t = format!("t{n}");
        feed(&mut busy, &[queued(10 + n as u64, &t), ev(11 + n as u64, 5, "task.settled", &t, json!({ "ok": true }))]);
    }
    assert!(busy.get("t1").is_some(), "the running task stays");
}
