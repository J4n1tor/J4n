//! Declarative folds (the kind `fold`): each reduction, keys, windows, units, what is refused, and what a snapshot of
//! one keeps.

use crate::world::*;
use j4n_core::change::Actor;
use j4n_core::fold::{Fold, FoldDecl, Reduce, Unit, declared};
use j4n_core::log::{Ev, Scope};
use j4n_core::schema::DataClass;
use j4n_core::value::{Key, Value};
use serde_json::{Value as Json, json};

pub fn fold(body: &str) -> Box<dyn Fold> {
    let (mut folds, problems) = folds_of(body, Scope::Project);
    assert!(problems.is_empty(), "{problems:?}");
    folds.pop().expect("a fold")
}

pub fn folds_of(body: &str, scope: Scope) -> (Vec<Box<dyn Fold>>, Vec<String>) {
    let (t, problems) = load(vec![pack("p", &format!("api = 1\n[[fold]]\nid = \"f\"\n{body}\n"))]);
    let (folds, more) = declared(&t, scope);
    (folds, problems.iter().chain(&more).map(ToString::to_string).collect())
}

pub fn event(seq: u64, at_ms: u64, kind: &str, task: Option<&str>, data: Json) -> Ev {
    Ev {
        seq,
        at_ms,
        kind: kind.into(),
        task: task.map(Into::into),
        thread: None,
        by: Actor::Core,
        cause: None,
        class: DataClass::Project,
        data,
    }
}

fn feed(f: &mut dyn Fold, events: &[Ev]) {
    for e in events {
        if f.wants(&e.kind) {
            f.apply(e);
        }
    }
}

fn read(f: &dyn Fold, path: &[&str]) -> Value {
    f.read(&path.iter().map(|p| Key::Name(p.to_string())).collect::<Vec<_>>(), 0)
}

fn num(n: f64) -> Value {
    Value::num(n)
}

#[test]
fn count_counts_events_by_key() {
    let mut f = fold("on = \"run.*\"\nreduce = \"count\"\nkey = \"{task}\"");
    let evs = [
        event(1, 1, "run.tool_call", Some("a"), json!({})),
        event(2, 2, "run.tool_call", Some("b"), json!({})),
        event(3, 3, "run.tool_call", Some("a"), json!({})),
        event(4, 4, "task.settled", Some("a"), json!({})),
        event(5, 5, "run.shell", None, json!({})),
    ];
    feed(f.as_mut(), &evs);
    assert_eq!(read(&*f, &["a"]), num(2.0));
    assert_eq!(read(&*f, &["b"]), num(1.0));
    assert_eq!(read(&*f, &["zzz"]), Value::Missing);
    let all = read(&*f, &[]);
    assert_eq!(all.at(&[Key::Name("a".into())]), num(2.0));
    assert!(matches!(&all, Value::Map(m) if m.len() == 2), "an event with no task has no key and is skipped: {all:?}");
    assert_eq!(f.read(&[Key::Idx(1)], 0), Value::Missing);
}

#[test]
fn a_fold_without_a_key_has_one_answer_even_before_the_first_event() {
    let mut f = fold("on = \"x.*\"\nreduce = \"count\"");
    assert_eq!(read(&*f, &[]), num(0.0));
    feed(f.as_mut(), &[event(1, 1, "x.y", None, json!(1)), event(2, 2, "x.z", None, json!(1))]);
    assert_eq!(read(&*f, &[]), num(2.0));
    let s = fold("on = \"x.*\"\nreduce = \"sum\"\nof = \"data\"\nunit = \"usd\"");
    assert_eq!(read(&*s, &[]), Value::Usd(0));
    let d = fold("on = \"x.*\"\nreduce = \"sum\"\nof = \"data\"\nunit = \"dur\"");
    assert_eq!(read(&*d, &[]), Value::Dur(0));
    let n = fold("on = \"x.*\"\nreduce = \"sum\"\nof = \"data\"");
    assert_eq!(read(&*n, &[]), num(0.0));
    for (reduce, empty) in [("last", Value::Missing), ("max", Value::Missing), ("set", Value::List(vec![])), ("distinct", num(0.0)), ("streak", num(0.0))] {
        let f = fold(&format!("on = \"x.*\"\nreduce = \"{reduce}\"\nof = \"data\""));
        assert_eq!(read(&*f, &[]), empty, "{reduce}");
    }
}

#[test]
fn sum_adds_numbers_money_or_durations_and_a_mix_has_no_sum() {
    let evs: Vec<Ev> = [(1, json!(0.1)), (2, json!(0.2)), (3, json!(1.5))].into_iter().map(|(i, d)| event(i, i, "x.y", None, d)).collect();
    let mut plain = fold("on = \"x.y\"\nreduce = \"sum\"\nof = \"data\"");
    feed(plain.as_mut(), &evs);
    assert_eq!(read(&*plain, &[]), num(0.1 + 0.2 + 1.5));
    let mut usd = fold("on = \"x.y\"\nreduce = \"sum\"\nof = \"data\"\nunit = \"usd\"");
    feed(usd.as_mut(), &evs);
    assert_eq!(read(&*usd, &[]), Value::Usd(1_800_000), "money is exact: 0.1 + 0.2 + 1.5 dollars");
    let mut dur = fold("on = \"x.y\"\nreduce = \"sum\"\nof = \"data\"\nunit = \"dur\"");
    feed(dur.as_mut(), &[event(1, 1, "x.y", None, json!(800)), event(2, 2, "x.y", None, json!(1200))]);
    assert_eq!(read(&*dur, &[]), Value::Dur(2000));
    let mut negative = fold("on = \"x.y\"\nreduce = \"sum\"\nof = \"data\"\nunit = \"dur\"");
    feed(negative.as_mut(), &[event(1, 1, "x.y", None, json!(-5)), event(2, 2, "x.y", None, json!(10))]);
    assert_eq!(read(&*negative, &[]), Value::Missing, "a negative duration is not one: it stays a number, and does not add to a duration");
    let mut mixed = fold("on = \"x.y\"\nreduce = \"sum\"\nof = \"data\"");
    feed(mixed.as_mut(), &[event(1, 1, "x.y", None, json!(1)), event(2, 2, "x.y", None, json!("two")), event(3, 3, "x.y", None, json!(3))]);
    assert_eq!(read(&*mixed, &[]), Value::Missing, "a mix poisons the sum for good");
    let mut huge = fold("on = \"x.y\"\nreduce = \"sum\"\nof = \"data\"\nunit = \"usd\"");
    feed(huge.as_mut(), &[event(1, 1, "x.y", None, json!(9.0e12)), event(2, 2, "x.y", None, json!(9.0e12))]);
    assert_eq!(read(&*huge, &[]), Value::Missing, "money past its range is no sum");
}

#[test]
fn last_and_max_keep_one_value() {
    let evs: Vec<Ev> = [json!(3), json!(9), json!(4)].into_iter().enumerate().map(|(i, d)| event(i as u64 + 1, 1, "x.y", None, d)).collect();
    let mut last = fold("on = \"x.y\"\nreduce = \"last\"\nof = \"data\"");
    feed(last.as_mut(), &evs);
    assert_eq!(read(&*last, &[]), num(4.0));
    let mut max = fold("on = \"x.y\"\nreduce = \"max\"\nof = \"data\"");
    feed(max.as_mut(), &evs);
    assert_eq!(read(&*max, &[]), num(9.0));
    feed(max.as_mut(), &[event(9, 9, "x.y", None, json!("text"))]);
    assert_eq!(read(&*max, &[]), num(9.0), "a value that cannot be compared does not take its place");
    let mut strs = fold("on = \"x.y\"\nreduce = \"max\"\nof = \"data\"");
    feed(strs.as_mut(), &[event(1, 1, "x.y", None, json!("pear")), event(2, 2, "x.y", None, json!("apple"))]);
    assert_eq!(read(&*strs, &[]), Value::Str("pear".into()));
}

#[test]
fn set_lists_the_distinct_values_and_distinct_counts_them() {
    let evs: Vec<Ev> = ["b", "a", "b", "c", "a"].iter().enumerate().map(|(i, d)| event(i as u64 + 1, 1, "x.y", None, json!(d))).collect();
    let mut set = fold("on = \"x.y\"\nreduce = \"set\"\nof = \"data\"");
    feed(set.as_mut(), &evs);
    let Value::List(items) = read(&*set, &[]) else { panic!("a list") };
    let mut got: Vec<String> = items.iter().map(|v| format!("{v:?}")).collect();
    got.sort();
    assert_eq!(got.len(), 3);
    let mut distinct = fold("on = \"x.y\"\nreduce = \"distinct\"\nof = \"data\"");
    feed(distinct.as_mut(), &evs);
    assert_eq!(read(&*distinct, &[]), num(3.0));
    feed(distinct.as_mut(), &[event(9, 9, "x.y", None, json!(1)), event(10, 10, "x.y", None, json!("1"))]);
    assert_eq!(read(&*distinct, &[]), num(5.0), "1 and \"1\" are different values");
}

#[test]
fn streak_counts_those_in_a_row_and_starts_again_after_one_that_does_not() {
    let mut f = fold("on = \"task.settled\"\nreduce = \"streak\"\nof = \"data.ok == false\"\nkey = \"{task}\"");
    let mk = |i, t, ok| event(i, i, "task.settled", Some(t), json!({"ok": ok}));
    feed(f.as_mut(), &[mk(1, "a", false), mk(2, "a", false), mk(3, "b", false), mk(4, "a", false)]);
    assert_eq!((read(&*f, &["a"]), read(&*f, &["b"])), (num(3.0), num(1.0)));
    feed(f.as_mut(), &[mk(5, "a", true)]);
    assert_eq!(read(&*f, &["a"]), num(0.0));
    feed(f.as_mut(), &[mk(6, "a", false)]);
    assert_eq!(read(&*f, &["a"]), num(1.0));
    // An event for which `when` fails is skipped altogether: it neither counts nor ends the run.
    let mut g = fold("on = \"task.settled\"\nreduce = \"streak\"\nof = \"data.ok == false\"\nwhen = \"data.kind == 'work'\"");
    let w = |i, kind, ok| event(i, i, "task.settled", None, json!({"ok": ok, "kind": kind}));
    feed(g.as_mut(), &[w(1, "work", false), w(2, "chat", true), w(3, "work", false)]);
    assert_eq!(read(&*g, &[]), num(2.0));
}

#[test]
fn when_filters_and_a_missing_of_skips_the_event() {
    let mut f = fold("on = \"x.y\"\nreduce = \"sum\"\nof = \"data.n\"\nwhen = \"by == 'rule:r1'\"");
    let mut e1 = event(1, 1, "x.y", None, json!({"n": 5}));
    e1.by = Actor::Rule("r1".into());
    let e2 = event(2, 2, "x.y", None, json!({"n": 7}));
    let mut e3 = event(3, 3, "x.y", None, json!({"other": 1}));
    e3.by = Actor::Rule("r1".into());
    let mut e4 = event(4, 4, "x.y", None, json!({"n": 1}));
    e4.by = Actor::Rule("r1".into());
    feed(f.as_mut(), &[e1, e2, e3, e4]);
    assert_eq!(read(&*f, &[]), num(6.0));
}

#[test]
fn expressions_read_the_event_whole_or_by_field() {
    let mut e = event(7, 90, "x.y", Some("t9"), json!({"deep": {"list": [10, 20]}}));
    e.thread = Some("th".into());
    e.cause = Some(3);
    e.by = Actor::Doer("claude".into());
    for (of, want) in [
        ("seq", num(7.0)),
        ("at_ms", num(90.0)),
        ("event.at_ms", num(90.0)),
        ("ev.data.deep.list[1]", num(20.0)),
        ("data.deep.list[0]", num(10.0)),
        ("by", Value::Str("doer:claude".into())),
        ("kind", Value::Str("x.y".into())),
        ("thread", Value::Str("th".into())),
        ("cause", num(3.0)),
        ("class", Value::Str("project".into())),
        ("task", Value::Str("t9".into())),
    ] {
        let mut f = fold(&format!("on = \"x.y\"\nreduce = \"last\"\nof = \"{of}\""));
        feed(f.as_mut(), std::slice::from_ref(&e));
        assert_eq!(read(&*f, &[]), want, "of = {of}");
    }
}

#[test]
fn a_key_that_looks_like_a_number_is_read_by_position_too() {
    let mut f = fold("on = \"x.y\"\nreduce = \"count\"\nkey = \"{data.n}\"");
    feed(f.as_mut(), &[event(1, 1, "x.y", None, json!({"n": 1})), event(2, 2, "x.y", None, json!({"n": 1})), event(3, 3, "x.y", None, json!({"n": 2}))]);
    assert_eq!(f.read(&[Key::Idx(1)], 0), num(2.0));
    assert_eq!(f.read(&[Key::Name("2".into())], 0), num(1.0));
    assert_eq!(f.read(&[Key::Idx(3)], 0), Value::Missing);
}

#[test]
fn a_window_keeps_the_last_n_events_of_a_key() {
    let mut f = fold("on = \"x.y\"\nreduce = \"sum\"\nof = \"data\"\nkey = \"{task}\"\nwindow = 3");
    let evs: Vec<Ev> = (1..=6).map(|i| event(i, i, "x.y", Some("a"), json!(i))).collect();
    feed(f.as_mut(), &evs);
    assert_eq!(read(&*f, &["a"]), num(4.0 + 5.0 + 6.0));
    let mut c = fold("on = \"x.y\"\nreduce = \"count\"\nwindow = 4");
    feed(c.as_mut(), &evs);
    assert_eq!(read(&*c, &[]), num(4.0));
    let mut s = fold("on = \"x.y\"\nreduce = \"streak\"\nof = \"data > 3\"\nwindow = 5");
    feed(s.as_mut(), &evs);
    assert_eq!(read(&*s, &[]), num(3.0), "of the last five (2..6): 4, 5, 6 hold");
}

#[test]
fn within_is_a_window_of_time_as_of_the_reading() {
    let mut f = fold("on = \"x.y\"\nreduce = \"count\"\nwithin = \"10s\"");
    feed(f.as_mut(), &[event(1, 1_000, "x.y", None, json!(1)), event(2, 5_000, "x.y", None, json!(1)), event(3, 9_000, "x.y", None, json!(1))]);
    let at = |f: &dyn Fold, now| f.read(&[], now);
    assert_eq!(at(&*f, 9_000), num(3.0));
    assert_eq!(at(&*f, 11_000), num(3.0), "an event exactly as old as the window is still in it");
    assert_eq!(at(&*f, 11_001), num(2.0));
    assert_eq!(at(&*f, 15_000), num(2.0));
    assert_eq!(at(&*f, 15_001), num(1.0));
    assert_eq!(at(&*f, 19_001), num(0.0));
    feed(f.as_mut(), &[event(4, 30_000, "x.y", None, json!(1))]);
    assert_eq!(at(&*f, 30_000), num(1.0), "an old event is dropped when a new one arrives");
    let mut edge = fold("on = \"x.y\"\nreduce = \"count\"\nwithin = \"10s\"");
    feed(edge.as_mut(), &[event(1, 1_000, "x.y", None, json!(1)), event(2, 11_000, "x.y", None, json!(1))]);
    assert_eq!(edge.read(&[], 11_000), num(2.0), "an event exactly as old as the window stays when the next arrives");
    // window and within together
    let mut both = fold("on = \"x.y\"\nreduce = \"count\"\nwindow = 2\nwithin = \"10s\"");
    feed(both.as_mut(), &[event(1, 1_000, "x.y", None, json!(1)), event(2, 2_000, "x.y", None, json!(1)), event(3, 3_000, "x.y", None, json!(1))]);
    assert_eq!(both.read(&[], 4_000), num(2.0), "the last two of three");
    assert_eq!(both.read(&[], 12_500), num(1.0), "of those two, one is older than ten seconds at 12.5 s");
}

#[test]
fn time_in_a_fold_is_the_events_not_the_clock() {
    let mut f = fold("on = \"x.y\"\nreduce = \"last\"\nof = \"since(at_ms)\"");
    feed(f.as_mut(), &[event(1, 12_345, "x.y", None, json!(1))]);
    assert_eq!(read(&*f, &[]), Value::Dur(0), "the event's own moment is now to the event");
}

#[test]
fn what_is_refused_is_refused_with_every_reason() {
    let ok = |on: &str, reduce: Reduce| FoldDecl {
        id: "f".into(),
        doc: None,
        on: on.into(),
        reduce,
        of: (reduce != Reduce::Count).then(|| "data".to_string()),
        key: None,
        when: None,
        unit: Unit::N,
        window: None,
        within: None,
        scope: Scope::Project,
    };
    assert!(ok("x.*", Reduce::Sum).compile().is_ok());
    let errs = |d: FoldDecl| d.compile().err().expect("refused").join(" | ");
    assert!(errs(FoldDecl { of: Some("data >".into()), ..ok("x", Reduce::Sum) }).contains("`of`"));
    assert!(errs(FoldDecl { of: Some("nosuchroot.x".into()), ..ok("x", Reduce::Sum) }).contains("`of`"));
    assert!(errs(FoldDecl { when: Some("(".into()), ..ok("x", Reduce::Sum) }).contains("`when`"));
    assert!(errs(FoldDecl { key: Some("{nosuch}".into()), ..ok("x", Reduce::Sum) }).contains("`key`"));
    assert!(errs(FoldDecl { key: Some("{task".into()), ..ok("x", Reduce::Sum) }).contains("`key`"));
    assert!(errs(FoldDecl { of: Some("data".into()), ..ok("x", Reduce::Count) }).contains("takes no `of`"));
    assert!(errs(FoldDecl { of: None, ..ok("x", Reduce::Max) }).contains("needs an `of`"));
    assert!(errs(FoldDecl { within: Some("soon".into()), ..ok("x", Reduce::Sum) }).contains("`within`"));
    assert!(errs(FoldDecl { within: Some("5".into()), ..ok("x", Reduce::Sum) }).contains("`within`"));
    assert!(errs(FoldDecl { window: Some(0), ..ok("x", Reduce::Sum) }).contains("`window`"));
    assert!(errs(FoldDecl { unit: Unit::Usd, ..ok("x", Reduce::Streak) }).contains("`unit`"));
    assert!(errs(FoldDecl { unit: Unit::Dur, ..ok("x", Reduce::Distinct) }).contains("`unit`"));
    assert!(errs(ok("", Reduce::Sum)).contains("`on`"));
    let many = errs(FoldDecl { of: Some("data >".into()), when: Some("(".into()), within: Some("x".into()), ..ok("x", Reduce::Sum) });
    assert!(many.contains("`of`") && many.contains("`when`") && many.contains("`within`"), "every fault, not the first: {many}");
}

#[test]
fn a_declaration_that_does_not_compile_is_reported_and_makes_no_fold() {
    let (folds, problems) = folds_of("on = \"x.y\"\nreduce = \"sum\"", Scope::Project);
    assert!(folds.is_empty());
    assert!(problems.iter().any(|p| p.contains("needs an `of`")), "{problems:?}");
    let (folds, problems) = folds_of("on = \"x.y\"\nreduce = \"median\"\nof = \"data\"", Scope::Project);
    assert!(folds.is_empty() && !problems.is_empty(), "the schema refuses a reduction that is not one");
}

#[test]
fn the_scope_of_a_fold_says_which_log_it_reads() {
    let body = "on = \"x.y\"\nreduce = \"count\"\nscope = \"home\"";
    assert_eq!(folds_of(body, Scope::Home).0.len(), 1);
    assert_eq!(folds_of(body, Scope::Project).0.len(), 0);
    assert_eq!(folds_of("on = \"x.y\"\nreduce = \"count\"", Scope::Project).0.len(), 1, "project is the default");
}

#[test]
fn a_declaration_has_a_version_that_follows_what_it_says() {
    let v = |body: &str| fold(body).version();
    let base = "on = \"x.y\"\nreduce = \"sum\"\nof = \"data\"";
    assert_eq!(v(base), v(base));
    for other in [
        "on = \"x.z\"\nreduce = \"sum\"\nof = \"data\"",
        "on = \"x.y\"\nreduce = \"max\"\nof = \"data\"",
        "on = \"x.y\"\nreduce = \"sum\"\nof = \"data.n\"",
        "on = \"x.y\"\nreduce = \"sum\"\nof = \"data\"\nunit = \"usd\"",
        "on = \"x.y\"\nreduce = \"sum\"\nof = \"data\"\nwindow = 5",
        "on = \"x.y\"\nreduce = \"sum\"\nof = \"data\"\nwhen = \"true\"",
        "on = \"x.y\"\nreduce = \"sum\"\nof = \"data\"\nkey = \"{task}\"",
    ] {
        assert_ne!(v(base), v(other), "{other}");
    }
}

#[test]
fn the_state_round_trips_with_money_durations_and_maps_in_it() {
    let mut f = fold("on = \"x.y\"\nreduce = \"set\"\nof = \"data\"");
    feed(f.as_mut(), &[event(1, 1, "x.y", None, json!({"k": [1, "a"], "$map": 1})), event(2, 2, "x.y", None, json!(null)), event(3, 3, "x.y", None, json!("s"))]);
    let saved = f.save();
    let mut g = fold("on = \"x.y\"\nreduce = \"set\"\nof = \"data\"");
    g.load(saved.clone()).unwrap();
    assert_eq!(g.save(), saved);
    assert_eq!(read(&*g, &[]), read(&*f, &[]));
    for (unit, v) in [("usd", Value::Usd(2_500_000)), ("dur", Value::Dur(1500))] {
        let mut f = fold(&format!("on = \"x.y\"\nreduce = \"last\"\nof = \"data\"\nunit = \"{unit}\""));
        feed(f.as_mut(), &[event(1, 1, "x.y", None, json!(if unit == "usd" { 2.5 } else { 1500.0 }))]);
        let mut g = fold(&format!("on = \"x.y\"\nreduce = \"last\"\nof = \"data\"\nunit = \"{unit}\""));
        g.load(f.save()).unwrap();
        assert_eq!(read(&*g, &[]), v, "{unit} stays {unit} through a snapshot");
    }
    assert!(g.load(json!({"cells": "no"})).is_err());
    assert!(g.load(json!(7)).is_err());
    g.reset();
    assert_eq!(read(&*g, &[]), Value::List(vec![]));
}

#[test]
fn the_caps_are_hit_said_and_not_silent() {
    let mut keys = fold("on = \"x.y\"\nreduce = \"count\"\nkey = \"{data.k}\"");
    let evs: Vec<Ev> = (0..100_050).map(|i| event(i + 1, 1, "x.y", None, json!({"k": format!("k{i}")}))).collect();
    feed(keys.as_mut(), &evs);
    assert!(matches!(read(&*keys, &[]), Value::Map(m) if m.len() == 100_000));
    assert!(keys.notes().iter().any(|n| n.contains("100000 keys")), "{:?}", keys.notes());
    assert_eq!(read(&*keys, &["k5"]), num(1.0));
    let mut set = fold("on = \"x.y\"\nreduce = \"distinct\"\nof = \"data\"");
    feed(set.as_mut(), &(0..10_005).map(|i| event(i + 1, 1, "x.y", None, json!(i))).collect::<Vec<_>>());
    assert_eq!(read(&*set, &[]), num(10_000.0));
    assert!(set.notes().iter().any(|n| n.contains("10000 values")), "{:?}", set.notes());
    let mut again = fold("on = \"x.y\"\nreduce = \"distinct\"\nof = \"data\"");
    feed(again.as_mut(), &(0..10_000).map(|i| event(i + 1, 1, "x.y", None, json!(i))).collect::<Vec<_>>());
    feed(again.as_mut(), &[event(10_001, 1, "x.y", None, json!(5))]);
    assert!(again.notes().is_empty(), "a value that is in the set already is not one too many");
    feed(again.as_mut(), &[event(10_002, 1, "x.y", None, json!(10_000))]);
    assert_eq!(again.notes().len(), 1);
    let mut big = fold("on = \"x.y\"\nreduce = \"count\"\nwindow = 20000");
    feed(big.as_mut(), &(0..10_050).map(|i| event(i + 1, 1, "x.y", None, json!(i))).collect::<Vec<_>>());
    assert_eq!(read(&*big, &[]), num(10_000.0), "a window wider than a key keeps is cut to it, and says so");
    assert!(big.notes().iter().any(|n| n.contains("window")), "{:?}", big.notes());
    let mut win = fold("on = \"x.y\"\nreduce = \"count\"\nwithin = \"365d\"");
    feed(win.as_mut(), &(0..10_050).map(|i| event(i + 1, 1, "x.y", None, json!(i))).collect::<Vec<_>>());
    assert_eq!(read(&*win, &[]), num(10_000.0));
    assert!(win.notes().iter().any(|n| n.contains("window")), "{:?}", win.notes());
    let quiet = fold("on = \"x.y\"\nreduce = \"count\"");
    assert!(quiet.notes().is_empty());
}
