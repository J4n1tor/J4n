//! The answerer chain: the walk, the invariants it keeps whatever an answerer says, the gate pair, the fallback, and the
//! state fold with the breaker. Scripted answerers with call counters stand in for rules, ranks, deciders and models.

use crate::world::{Scratch, all, open_log};
use j4n_core::answer::chain::{Ledger, MAX_MACHINE, Note, Registry, Spent, Walk, fallback, pair_json, walk};
use j4n_core::answer::state::{Answerers, BREAKER_AFTER, OFF_MS, Opened};
use j4n_core::answer::{Answerer, AskPoint, Fallback, Impact, Kind, Offer, Paid, Question, ShapeKind, State, StateView, Step};
use j4n_core::ask::{Answer, AskShape, Opt, TEXT_MAX};
use j4n_core::change::Actor;
use j4n_core::clock::{Clock, ManualClock};
use j4n_core::fold::{Fold, Folds};
use j4n_core::log::{Ev, Log, New, Sink};
use j4n_core::schema::DataClass;
use j4n_core::value::{Key, Value};
use serde_json::{Value as Json, json};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

// ---- scripted answerers ----

type Reply = Box<dyn Fn(&Question) -> Offer + Send + Sync>;

struct Fake {
    id: String,
    kind: Kind,
    reply: Reply,
    calls: AtomicUsize,
    seen: Mutex<Vec<Question>>,
}

impl Answerer for Fake {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind(&self) -> Kind {
        self.kind
    }
    fn offer(&self, q: &Question) -> Offer {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.seen.lock().unwrap().push(q.clone());
        (self.reply)(q)
    }
}

impl Fake {
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

fn fake(id: &str, kind: Kind, reply: impl Fn(&Question) -> Offer + Send + Sync + 'static) -> Arc<Fake> {
    Arc::new(Fake { id: id.into(), kind, reply: Box::new(reply), calls: AtomicUsize::new(0), seen: Mutex::new(Vec::new()) })
}

fn says(id: &str, kind: Kind, answer: Answer, conf: f32) -> Arc<Fake> {
    let id = id.to_string();
    let why = format!("{id} says so");
    fake(&id, kind, move |_| Offer::Answer { answer: answer.clone(), conf, why: why.clone() })
}

fn passes(id: &str, kind: Kind) -> Arc<Fake> {
    fake(id, kind, |_| Offer::Pass { why: "no idea".into() })
}

fn fails(id: &str, kind: Kind) -> Arc<Fake> {
    fake(id, kind, |_| Offer::Failed { why: "timed out".into() })
}

fn registry(list: &[&Arc<Fake>]) -> Registry {
    let mut r = Registry::new();
    for f in list {
        assert!(r.register((*f).clone()));
    }
    r
}

// ---- points and questions ----

/// `rule:route`, `decider:jev@0.6` (min_conf), `human@60000` (after_ms), `rank`.
fn s(text: &str) -> Step {
    let (head, arg) = text.split_once('@').map_or((text, None), |(h, a)| (h, Some(a)));
    let (kind, name) = head.split_once(':').unwrap_or((head, ""));
    let kind = Kind::from_word(kind).unwrap();
    Step {
        kind,
        name: name.into(),
        min_conf: arg.filter(|_| kind != Kind::Human).map(|a| a.parse().unwrap()),
        after_ms: arg.filter(|_| kind == Kind::Human).map(|a| a.parse().unwrap()),
    }
}

fn point(impact: Impact, shape: ShapeKind, chain: &[&str], fallback: Fallback) -> AskPoint {
    AskPoint {
        id: "p".into(),
        doc: "a test point".into(),
        shape,
        impact,
        data: DataClass::Project,
        chain: chain.iter().map(|t| s(t)).collect(),
        fallback,
        rank: None,
        breaker: true,
    }
}

fn opt(id: &str, recommended: bool) -> Opt {
    Opt { id: id.into(), label: format!("the {id} way"), recommended }
}

fn question(text: &str, shape: AskShape) -> Question {
    Question { ask: "t9".into(), point: "p".into(), asker: "t1".into(), thread: None, text: text.into(), shape, goal: "tidy up".into(), act: None }
}

fn choice(text: &str, ids: &[&str]) -> Question {
    question(text, AskShape::Choice { options: ids.iter().map(|i| opt(i, false)).collect() })
}

fn yesno(text: &str) -> Question {
    question(text, AskShape::YesNo)
}

fn approval(text: &str) -> Question {
    question(text, AskShape::Approval { action: "make build".into(), risk: 40 })
}

fn pick(id: &str) -> Answer {
    Answer::Pick { id: id.into() }
}

fn text(t: &str) -> Answer {
    Answer::Text { text: t.into() }
}

// ---- running a walk ----

fn go(p: &AskPoint, q: &Question, reg: &Registry, ledger: &Ledger, headless: bool) -> (Walk, Vec<Note>) {
    walk(p, q, reg, &Answerers::new(), ledger, 1_000, headless)
}

/// One walk with a fresh ledger and everything active.
fn once(p: &AskPoint, q: &Question, reg: &Registry) -> (Walk, Vec<Note>) {
    go(p, q, reg, &Ledger::new(), false)
}

/// `(answerer, outcome)` of every `ask.step` note, in order.
fn steps(notes: &[Note]) -> Vec<(String, String)> {
    notes
        .iter()
        .filter(|n| n.kind == "ask.step")
        .map(|n| (n.data["answerer"].as_str().unwrap().to_string(), n.data["outcome"].as_str().unwrap().to_string()))
        .collect()
}

fn step_of<'a>(notes: &'a [Note], answerer: &str) -> &'a Json {
    &notes.iter().find(|n| n.kind == "ask.step" && n.data["answerer"] == answerer).unwrap_or_else(|| panic!("no step for {answerer}: {notes:?}")).data
}

fn said(notes: &[Note], needle: &str) -> bool {
    notes.iter().any(|n| n.kind == "ask.note" && n.data["why"].as_str().unwrap().contains(needle))
}

fn answered(w: &Walk) -> &j4n_core::answer::chain::Outcome {
    match w {
        Walk::Answered(o) => o,
        other => panic!("expected an answer, got {other:?}"),
    }
}

fn to_human(w: &Walk) -> (Option<u64>, &str, &Vec<(String, Answer)>) {
    match w {
        Walk::ToHuman { after_ms, why, pair } => (*after_ms, why, pair),
        other => panic!("expected the operator, got {other:?}"),
    }
}

// ---- the answerer's basic cases ----

#[test]
fn a_text_answer_is_one_short_plain_line() {
    let q = question("what is the test command?", AskShape::Text);
    for (t, ok) in [
        ("cargo test --quiet [from the project's check rows]", true),
        ("", false),
        ("   ", false),
        ("two\nlines", false),
        ("tab\there", false),
        ("nul\0byte", false),
        ("carriage\rreturn", false),
        ("x", true),
    ] {
        assert_eq!(q.shape.check(&text(t)).is_ok(), ok, "{t:?}");
    }
    assert!(q.shape.check(&text(&"x".repeat(TEXT_MAX))).is_ok());
    assert!(q.shape.check(&text(&"x".repeat(TEXT_MAX + 1))).is_err());
    assert!(q.shape.check(&text(&"é".repeat(TEXT_MAX))).is_ok(), "240 characters, though 480 bytes");
}

#[test]
fn an_offer_has_no_allow_or_deny_a_yes_to_a_choice_is_not_an_answer() {
    // `Offer` itself has only Answer, Pass and Failed; and the shape refuses a yes or no to a question that is not yes/no.
    let (a, b) = (fake("rule:r", Kind::Rule, |_| Offer::Answer { answer: Answer::Yes, conf: 1.0, why: String::new() }), passes("rule:s", Kind::Rule));
    let p = point(Impact::Low, ShapeKind::Choice, &["rule:r", "rule:s", "human"], Fallback::Rank);
    let (w, notes) = once(&p, &choice("which?", &["a", "b"]), &registry(&[&a, &b]));
    assert_eq!(to_human(&w).1, "no machine answered");
    assert_eq!(steps(&notes), [("rule:r".into(), "failed".into()), ("rule:s".into(), "pass".into())]);
    assert_eq!(b.calls(), 1, "the walk went on past the bad answer");
}

#[test]
fn a_pick_must_be_one_of_the_offered_options_and_a_bad_answer_is_a_failure_the_walk_goes_past() {
    let cases: Vec<(&str, Question, Answer)> = vec![
        ("a pick that is not offered", choice("which?", &["a", "b"]), pick("zzz")),
        ("an empty pick", choice("which?", &["a", "b"]), pick("")),
        ("a pick differing in case", choice("which?", &["a", "b"]), pick("A")),
        ("a text with a newline", choice("which?", &["a", "b"]), text("one\ntwo")),
        ("a text with a tab", question("say", AskShape::Text), text("one\ttwo")),
        ("a text with a NUL", question("say", AskShape::Text), text("one\0two")),
        ("a text of 241 characters", question("say", AskShape::Text), text(&"x".repeat(TEXT_MAX + 1))),
        ("a blank text", question("say", AskShape::Text), text("  \u{a0} ")),
        ("a yes to a choice", choice("which?", &["a"]), Answer::Yes),
        ("a no to a choice", choice("which?", &["a"]), Answer::No),
        ("a pick to a yes/no", yesno("ok?"), pick("a")),
        ("a text to a yes/no", yesno("ok?"), text("yes")),
        ("a text to an approval", approval("make build?"), text("yes please")),
        ("a pick to an approval", approval("make build?"), pick("always")),
        ("a yes to a text", question("say", AskShape::Text), Answer::Yes),
        ("a pick to a text", question("say", AskShape::Text), pick("a")),
    ];
    for (name, q, bad) in cases {
        let first = says("rule:bad", Kind::Rule, bad, 1.0);
        let second = passes("rule:ok", Kind::Rule);
        let p = point(Impact::Low, ShapeKind::of(&q.shape), &["rule:bad", "rule:ok", "human"], Fallback::Safe);
        let (w, notes) = once(&p, &q, &registry(&[&first, &second]));
        assert!(matches!(w, Walk::ToHuman { .. }), "{name}: {w:?}");
        assert_eq!(second.calls(), 1, "{name}: the walk went on");
        assert_eq!(steps(&notes)[0], ("rule:bad".to_string(), "failed".to_string()), "{name}");
        assert!(step_of(&notes, "rule:bad")["why"].as_str().unwrap().contains("not valid"), "{name}");
    }
}

#[test]
fn a_valid_answer_of_each_shape_stands() {
    let cases = vec![
        (choice("which?", &["a", "b"]), pick("b")),
        (choice("which?", &["a", "b"]), text("something else entirely")),
        (yesno("ok?"), Answer::No),
        (approval("make build?"), Answer::Yes),
        (question("say", AskShape::Text), text(&"é".repeat(TEXT_MAX))),
    ];
    for (q, good) in cases {
        let r = says("rule:r", Kind::Rule, good.clone(), 0.9);
        let p = point(Impact::Low, ShapeKind::of(&q.shape), &["rule:r", "human"], Fallback::Safe);
        let (w, _) = once(&p, &q, &registry(&[&r]));
        assert_eq!(answered(&w).answer, good);
    }
}

#[test]
fn a_confidence_that_is_not_a_number_from_zero_to_one_is_a_failure() {
    for conf in [f32::NAN, -0.1, 1.01, f32::INFINITY, f32::NEG_INFINITY] {
        let r = says("rule:r", Kind::Rule, Answer::Yes, conf);
        let p = point(Impact::Low, ShapeKind::YesNo, &["rule:r", "human"], Fallback::Safe);
        let (w, notes) = once(&p, &yesno("ok?"), &registry(&[&r]));
        assert!(matches!(w, Walk::ToHuman { .. }), "{conf}: {w:?}");
        assert_eq!(steps(&notes), [("rule:r".into(), "failed".into())], "{conf}");
    }
    for conf in [0.0, 1.0] {
        let r = says("rule:r", Kind::Rule, Answer::Yes, conf);
        let p = point(Impact::Low, ShapeKind::YesNo, &["rule:r", "human"], Fallback::Safe);
        assert_eq!(answered(&once(&p, &yesno("ok?"), &registry(&[&r])).0).conf, Some(conf));
    }
}

// ---- the walk ----

#[test]
fn steps_run_in_order_and_the_first_confident_answer_stands() {
    let (a, b, c) = (passes("rule:a", Kind::Rule), says("decider:b", Kind::Decider, pick("b"), 0.9), says("model:c", Kind::Model, pick("c"), 0.99));
    let p = point(Impact::Low, ShapeKind::Choice, &["rule:a", "decider:b", "model:c", "human"], Fallback::Rank);
    let (w, notes) = once(&p, &choice("which?", &["a", "b", "c"]), &registry(&[&a, &b, &c]));
    let o = answered(&w);
    assert_eq!((o.answer.clone(), o.by.as_str(), o.kind, o.conf), (pick("b"), "decider:b", Kind::Decider, Some(0.9)));
    assert!(o.pair.is_empty());
    assert_eq!((a.calls(), b.calls(), c.calls()), (1, 1, 0));
    assert_eq!(steps(&notes), [("rule:a".into(), "pass".into()), ("decider:b".into(), "answer".into())]);
    let n = step_of(&notes, "decider:b");
    assert_eq!((n["ask"].as_str(), n["kind"].as_str(), n["conf"].as_f64()), (Some("t9"), Some("decider"), Some(0.9)));
    assert!(n["ms"].is_u64() && n["why"].is_string());
}

#[test]
fn mid_impact_also_takes_the_first_confident_answer() {
    let (a, b) = (says("rule:a", Kind::Rule, Answer::Yes, 0.5), says("model:b", Kind::Model, Answer::No, 0.9));
    let p = point(Impact::Mid, ShapeKind::YesNo, &["rule:a", "model:b", "human"], Fallback::Safe);
    assert_eq!(answered(&once(&p, &yesno("ok?"), &registry(&[&a, &b])).0).answer, Answer::Yes);
    assert_eq!(b.calls(), 0);
}

#[test]
fn a_machine_step_that_is_not_installed_is_passed_with_a_note_once_per_task() {
    let b = says("rule:b", Kind::Rule, Answer::Yes, 1.0);
    let p = point(Impact::Low, ShapeKind::YesNo, &["decider:gone", "rule:b", "human"], Fallback::Safe);
    let reg = registry(&[&b]);
    let ledger = Ledger::new();
    let (w, notes) = go(&p, &yesno("one?"), &reg, &ledger, false);
    assert_eq!(answered(&w).by, "rule:b");
    assert_eq!(steps(&notes)[0], ("decider:gone".into(), "skipped".into()));
    assert!(step_of(&notes, "decider:gone")["why"].as_str().unwrap().contains("not installed"));
    let (_, again) = go(&p, &yesno("two?"), &reg, &ledger, false);
    assert_eq!(steps(&again), [("rule:b".into(), "answer".into())], "the ledger remembers it said so");
    let (_, other_task) = go(&p, &yesno("one?"), &reg, &Ledger::new(), false);
    assert_eq!(steps(&other_task).len(), 2, "another task is told again");
}

#[test]
fn the_human_step_stops_the_walk_and_carries_its_wait() {
    let (a, late) = (passes("rule:a", Kind::Rule), says("model:late", Kind::Model, Answer::Yes, 1.0));
    let p = point(Impact::Low, ShapeKind::YesNo, &["rule:a", "human@60000", "model:late"], Fallback::Safe);
    let (w, notes) = once(&p, &yesno("ok?"), &registry(&[&a, &late]));
    assert_eq!(to_human(&w).0, Some(60_000));
    assert_eq!(late.calls(), 0, "nothing after the operator is tried");
    assert_eq!(steps(&notes), [("rule:a".into(), "pass".into())], "the operator is not a step");
    let p = point(Impact::Low, ShapeKind::YesNo, &["human"], Fallback::Safe);
    assert_eq!(to_human(&once(&p, &yesno("ok?"), &Registry::new()).0).0, None);
}

#[test]
fn headless_the_human_step_is_the_fallback_and_says_there_is_no_operator() {
    let a = passes("rule:a", Kind::Rule);
    let q = choice("which?", &["a", "b", "safe"]);
    let p = point(Impact::Low, ShapeKind::Choice, &["rule:a", "human@60000"], Fallback::Safe);
    let (w, _) = go(&p, &q, &registry(&[&a]), &Ledger::new(), true);
    match w {
        Walk::Fallback(o) => {
            assert_eq!(o.answer, pick("safe"));
            assert!(o.why.starts_with("no operator"), "{}", o.why);
            assert!(q.shape.check(&o.answer).is_ok());
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(a.calls(), 1, "the machines are still asked first");
}

#[test]
fn headless_with_a_fallback_that_has_no_answer_nothing_waits() {
    // `safe` on a choice that offers no `safe` option makes no answer; there is no one to ask, so the asker goes on.
    let p = point(Impact::Low, ShapeKind::Choice, &["human"], Fallback::Safe);
    let (w, _) = go(&p, &choice("which?", &["a", "b"]), &Registry::new(), &Ledger::new(), true);
    assert!(matches!(w, Walk::Skip { .. }), "{w:?}");
    let (w, _) = go(&p, &choice("which?", &["a", "b"]), &Registry::new(), &Ledger::new(), false);
    assert!(matches!(w, Walk::ToHuman { .. }), "with an operator the question stays open: {w:?}");
}

#[test]
fn a_chain_with_no_human_step_ends_in_the_fallback() {
    let a = passes("rule:a", Kind::Rule);
    let p = point(Impact::Low, ShapeKind::YesNo, &["rule:a"], Fallback::Safe);
    let (w, _) = once(&p, &yesno("ok?"), &registry(&[&a]));
    match w {
        Walk::Fallback(o) => assert_eq!((o.answer, o.by.as_str()), (Answer::No, "fallback:safe")),
        other => panic!("{other:?}"),
    }
    let p = point(Impact::Low, ShapeKind::Text, &["rule:a"], Fallback::Safe);
    assert!(matches!(once(&p, &question("say", AskShape::Text), &registry(&[&a])).0, Walk::ToHuman { after_ms: None, .. }), "a fallback with no answer keeps the question open");
}

#[test]
fn an_answerer_that_is_off_or_shadow_is_not_called_and_is_said_once() {
    let (off, shadow, ok) = (says("decider:off", Kind::Decider, Answer::No, 1.0), says("model:shadow", Kind::Model, Answer::No, 1.0), says("rule:ok", Kind::Rule, Answer::Yes, 1.0));
    let p = point(Impact::Low, ShapeKind::YesNo, &["decider:off", "model:shadow", "rule:ok", "human"], Fallback::Safe);
    let reg = registry(&[&off, &shadow, &ok]);
    let state = Answerers::new();
    let mut fold = state.clone();
    for (i, (kind, data)) in [
        ("ask.step", json!({"answerer": "decider:off", "outcome": "failed"})),
        ("ask.step", json!({"answerer": "decider:off", "outcome": "failed"})),
        ("ask.step", json!({"answerer": "decider:off", "outcome": "failed"})),
        ("answerer.shadow", json!({"answerer": "model:shadow", "why": "probes not passed"})),
    ]
    .into_iter()
    .enumerate()
    {
        fold.apply(&ev(i as u64 + 1, 1_000, kind, data));
    }
    let ledger = Ledger::new();
    let (w, notes) = walk(&p, &yesno("one?"), &reg, &state, &ledger, 2_000, false);
    assert_eq!(answered(&w).by, "rule:ok");
    assert_eq!((off.calls(), shadow.calls()), (0, 0));
    assert_eq!(steps(&notes), [("decider:off".into(), "skipped".into()), ("model:shadow".into(), "skipped".into()), ("rule:ok".into(), "answer".into())]);
    assert!(step_of(&notes, "decider:off")["why"].as_str().unwrap().contains("off until"));
    assert!(step_of(&notes, "model:shadow")["why"].as_str().unwrap().contains("probes not passed"));
    let (_, again) = walk(&p, &yesno("two?"), &reg, &state, &ledger, 2_000, false);
    assert_eq!(steps(&again), [("rule:ok".into(), "answer".into())], "one note each, then quiet");
    // The day passes: it is asked again.
    let later = 1_000 + OFF_MS;
    let (_, notes) = walk(&p, &yesno("three?"), &reg, &state, &Ledger::new(), later, false);
    assert_eq!(off.calls(), 1, "at `until` it is active again");
    assert_eq!(step_of(&notes, "decider:off")["outcome"], "answer");
}

#[test]
fn an_answerer_of_another_kind_than_the_chain_names_is_not_called() {
    // A model registered under the id of a rule must not slip past what the chain says it is.
    let sneaky = says("rule:x", Kind::Model, Answer::Yes, 1.0);
    let p = point(Impact::Mid, ShapeKind::Approval, &["rule:x", "human"], Fallback::Safe);
    let (w, notes) = once(&p, &approval("make build?"), &registry(&[&sneaky]));
    assert!(matches!(w, Walk::ToHuman { .. }), "{w:?}");
    assert_eq!(sneaky.calls(), 0);
    assert!(step_of(&notes, "rule:x")["why"].as_str().unwrap().contains("it is a model"));
    let rule_named_model = says("model:x", Kind::Rule, Answer::Yes, 1.0);
    let p = point(Impact::Low, ShapeKind::YesNo, &["model:x", "human"], Fallback::Safe);
    assert!(matches!(once(&p, &yesno("ok?"), &registry(&[&rule_named_model])).0, Walk::ToHuman { .. }));
    assert_eq!(rule_named_model.calls(), 0);
}

#[test]
fn a_step_listed_twice_is_asked_once() {
    let a = passes("rule:a", Kind::Rule);
    let p = point(Impact::Low, ShapeKind::YesNo, &["rule:a", "rule:a", "human"], Fallback::Safe);
    let (_, notes) = once(&p, &yesno("ok?"), &registry(&[&a]));
    assert_eq!(a.calls(), 1);
    assert_eq!(steps(&notes), [("rule:a".into(), "pass".into()), ("rule:a".into(), "skipped".into())]);
}

#[test]
fn a_confidence_below_the_steps_minimum_is_a_pass_with_a_note() {
    let (low, next) = (says("decider:j", Kind::Decider, pick("a"), 0.59), says("rule:r", Kind::Rule, pick("b"), 0.1));
    let p = point(Impact::Low, ShapeKind::Choice, &["decider:j@0.6", "rule:r", "human"], Fallback::Rank);
    let (w, notes) = once(&p, &choice("which?", &["a", "b"]), &registry(&[&low, &next]));
    assert_eq!(answered(&w).by, "rule:r", "a step without min_conf takes any confidence");
    let n = step_of(&notes, "decider:j");
    assert_eq!(n["outcome"], "pass");
    assert!(n["why"].as_str().unwrap().contains("below 0.60"), "{n}");
    assert_eq!(n["conf"].as_f64(), Some(0.59));
    let exact = says("decider:j", Kind::Decider, pick("a"), 0.6);
    assert_eq!(answered(&once(&p, &choice("which?", &["a", "b"]), &registry(&[&exact])).0).by, "decider:j", "at the minimum is enough");
}

#[test]
fn a_panicking_answerer_is_a_failure_and_the_walk_goes_on() {
    let boom = fake("model:boom", Kind::Model, |_| panic!("the connector blew up"));
    let ok = says("rule:ok", Kind::Rule, Answer::Yes, 1.0);
    let p = point(Impact::Low, ShapeKind::YesNo, &["model:boom", "rule:ok", "human"], Fallback::Safe);
    let (w, notes) = once(&p, &yesno("ok?"), &registry(&[&boom, &ok]));
    assert_eq!(answered(&w).by, "rule:ok");
    assert_eq!(step_of(&notes, "model:boom")["outcome"], "failed");
}

#[test]
fn what_an_answerer_says_about_itself_is_one_short_line_in_the_events() {
    let noisy = fake("rule:r", Kind::Rule, |_| Offer::Answer { answer: Answer::Yes, conf: 0.8, why: format!("line one\nline two\0{}", "y".repeat(5000)) });
    let p = point(Impact::Low, ShapeKind::YesNo, &["rule:r"], Fallback::Safe);
    let (w, notes) = once(&p, &yesno("ok?"), &registry(&[&noisy]));
    for why in [step_of(&notes, "rule:r")["why"].as_str().unwrap(), answered(&w).why.as_str()] {
        assert!(why.chars().count() <= 200 && !why.chars().any(char::is_control), "{why:?}");
    }
    let failing = fake("rule:f", Kind::Rule, |_| Offer::Failed { why: "multi\nline\u{7}".into() });
    let (_, notes) = once(&point(Impact::Low, ShapeKind::YesNo, &["rule:f"], Fallback::Safe), &yesno("ok?"), &registry(&[&failing]));
    assert!(!step_of(&notes, "rule:f")["why"].as_str().unwrap().contains('\n'));
}

// ---- approvals never reach a model ----

#[test]
fn an_approval_is_never_put_to_a_decider_or_a_model_and_they_are_not_called() {
    let (rule, decider, model) = (passes("rule:gate", Kind::Rule), says("decider:j", Kind::Decider, Answer::Yes, 1.0), says("model:m", Kind::Model, Answer::Yes, 1.0));
    let reg = registry(&[&rule, &decider, &model]);
    // The point says approval.
    let p = point(Impact::Mid, ShapeKind::Approval, &["rule:gate", "decider:j", "model:m", "human"], Fallback::Safe);
    let (w, notes) = once(&p, &approval("make build?"), &reg);
    assert!(matches!(w, Walk::ToHuman { .. }), "{w:?}");
    // The question's own shape says approval, though the point is declared as something else.
    let p2 = point(Impact::Low, ShapeKind::Choice, &["rule:gate", "decider:j", "model:m", "human"], Fallback::Rank);
    let (w2, _) = once(&p2, &approval("make build?"), &reg);
    assert!(matches!(w2, Walk::ToHuman { .. }), "{w2:?}");
    // The point says approval, the question is yes/no.
    let (w3, _) = once(&p, &yesno("make build?"), &reg);
    assert!(matches!(w3, Walk::ToHuman { .. }), "{w3:?}");
    assert_eq!((decider.calls(), model.calls()), (0, 0));
    assert_eq!(rule.calls(), 3, "a rule is still asked");
    for who in ["decider:j", "model:m"] {
        let n = step_of(&notes, who);
        assert_eq!(n["outcome"], "skipped");
        assert!(n["why"].as_str().unwrap().contains("approval"));
    }
}

#[test]
fn a_rule_may_answer_an_approval() {
    let rule = says("rule:gate", Kind::Rule, Answer::No, 1.0);
    let model = passes("model:m", Kind::Model);
    let p = point(Impact::Mid, ShapeKind::Approval, &["rule:gate", "model:m", "human"], Fallback::Safe);
    let (w, _) = once(&p, &approval("make build?"), &registry(&[&rule, &model]));
    assert_eq!(answered(&w).answer, Answer::No);
}

#[test]
fn a_gate_impact_approval_is_answered_by_one_rule_and_never_needs_a_pair() {
    let r1 = says("rule:a", Kind::Rule, Answer::Yes, 1.0);
    let p = point(Impact::Gate, ShapeKind::Approval, &["rule:a", "human"], Fallback::Safe);
    let (w, _) = once(&p, &approval("make build?"), &registry(&[&r1]));
    assert_eq!(answered(&w).answer, Answer::Yes);
    assert!(answered(&w).pair.is_empty());
}

// ---- the caps ----

#[test]
fn a_task_gets_at_most_six_machine_answers_then_machines_are_skipped() {
    let r = says("decider:r", Kind::Decider, Answer::Yes, 1.0);
    let p = point(Impact::Low, ShapeKind::YesNo, &["decider:r", "human"], Fallback::Safe);
    let reg = registry(&[&r]);
    let ledger = Ledger::new();
    for i in 0..MAX_MACHINE {
        let (w, _) = go(&p, &yesno(&format!("question {i}")), &reg, &ledger, false);
        assert!(matches!(w, Walk::Answered(_)), "{i}");
    }
    assert_eq!(ledger.given(), MAX_MACHINE);
    let (w, notes) = go(&p, &yesno("question 7"), &reg, &ledger, false);
    assert!(matches!(w, Walk::ToHuman { .. }), "{w:?}");
    assert_eq!(r.calls(), MAX_MACHINE, "the seventh never reached the answerer");
    assert!(said(&notes, "machine answers"), "{notes:?}");
    // Headless the fallback stands instead.
    assert!(matches!(go(&p, &yesno("question 8"), &reg, &ledger, true).0, Walk::Fallback(_)));
    // Another task has its own count.
    assert!(matches!(go(&p, &yesno("question 7"), &reg, &Ledger::new(), false).0, Walk::Answered(_)));
}

#[test]
fn a_rule_is_not_limited_the_same_approval_is_answered_every_time_it_is_asked() {
    let r = says("rule:gate", Kind::Rule, Answer::Yes, 1.0);
    let p = point(Impact::Gate, ShapeKind::Approval, &["rule:gate", "human"], Fallback::Safe);
    let reg = registry(&[&r]);
    let ledger = Ledger::new();
    for i in 0..(MAX_MACHINE * 3) {
        assert!(matches!(go(&p, &approval("make build?"), &reg, &ledger, false).0, Walk::Answered(_)), "{i}");
    }
    assert_eq!((ledger.given(), r.calls()), (0, MAX_MACHINE * 3));
}

#[test]
fn only_answers_that_stood_count_not_passes_or_failures() {
    let (p1, f1) = (passes("rule:p", Kind::Rule), fails("decider:f", Kind::Decider));
    let p = point(Impact::Low, ShapeKind::YesNo, &["rule:p", "decider:f", "human"], Fallback::Safe);
    let ledger = Ledger::new();
    for i in 0..(MAX_MACHINE * 2) {
        go(&p, &yesno(&format!("q{i}")), &registry(&[&p1, &f1]), &ledger, false);
    }
    assert_eq!(ledger.given(), 0);
    assert_eq!(f1.calls(), MAX_MACHINE * 2);
}

#[test]
fn a_gate_pair_spends_two_machine_answers() {
    let (a, b) = (says("decider:a", Kind::Decider, Answer::Yes, 1.0), says("model:b", Kind::Model, Answer::Yes, 1.0));
    let p = point(Impact::Gate, ShapeKind::YesNo, &["decider:a", "model:b", "human"], Fallback::Safe);
    let reg = registry(&[&a, &b]);
    let ledger = Ledger::new();
    for i in 0..3 {
        assert!(matches!(go(&p, &yesno(&format!("q{i}")), &reg, &ledger, false).0, Walk::Answered(_)));
    }
    assert_eq!(ledger.given(), 6);
    assert!(matches!(go(&p, &yesno("q3"), &reg, &ledger, false).0, Walk::ToHuman { .. }));
    assert_eq!((a.calls(), b.calls()), (3, 3));
}

#[test]
fn the_same_question_is_never_put_to_machines_twice() {
    let r = says("model:r", Kind::Model, pick("a"), 1.0);
    let p = point(Impact::Low, ShapeKind::Choice, &["model:r", "human"], Fallback::Rank);
    let reg = registry(&[&r]);
    let ledger = Ledger::new();
    let q = choice("which?", &["a", "b"]);
    assert!(matches!(go(&p, &q, &reg, &ledger, false).0, Walk::Answered(_)));
    // Another ask id, another asker: it is still the same question.
    let mut again = q.clone();
    again.ask = "t10".into();
    again.asker = "t2".into();
    let (w, notes) = go(&p, &again, &reg, &ledger, false);
    assert!(matches!(w, Walk::ToHuman { .. }), "{w:?}");
    assert_eq!(r.calls(), 1);
    assert!(said(&notes, "put to machines before"));
    // A different text, different options, or a different point is a different question.
    let other_point = AskPoint { id: "q".into(), ..p.clone() };
    for (name, point, q) in [
        ("text", &p, choice("which one?", &["a", "b"])),
        ("options", &p, choice("which?", &["a", "b", "c"])),
        ("label", &p, question("which?", AskShape::Choice { options: vec![opt("a", true), opt("b", false)] })),
        ("point", &other_point, q.clone()),
    ] {
        assert!(matches!(go(point, &q, &reg, &ledger, false).0, Walk::Answered(_)), "{name}");
    }
    // Two approvals for different actions are different.
    let pa = point_approval();
    let r2 = says("rule:g", Kind::Rule, Answer::No, 1.0);
    let reg2 = registry(&[&r2]);
    let l2 = Ledger::new();
    let mut a1 = approval("run it?");
    let mut a2 = approval("run it?");
    a2.shape = AskShape::Approval { action: "rm -rf build".into(), risk: 40 };
    assert!(matches!(go(&pa, &a1, &reg2, &l2, false).0, Walk::Answered(_)));
    assert!(matches!(go(&pa, &a2, &reg2, &l2, false).0, Walk::Answered(_)));
    a1.ask = "t77".into();
    assert!(matches!(go(&pa, &a1, &reg2, &l2, false).0, Walk::Answered(_)), "a rule answers the same approval again");
}

fn point_approval() -> AskPoint {
    point(Impact::Mid, ShapeKind::Approval, &["rule:g", "human"], Fallback::Safe)
}

// ---- gate pairs ----

fn gate_point() -> AskPoint {
    point(Impact::Gate, ShapeKind::Choice, &["rule:a", "decider:b@0.6", "model:c", "human@30000"], Fallback::Safe)
}

fn gate_q() -> Question {
    choice("which?", &["x", "y", "safe"])
}

#[test]
fn two_distinct_machine_answerers_that_agree_stand_with_their_pair() {
    let (a, b, c) = (says("rule:a", Kind::Rule, pick("x"), 0.8), says("decider:b", Kind::Decider, pick("x"), 0.7), says("model:c", Kind::Model, pick("y"), 1.0));
    let (w, notes) = once(&gate_point(), &gate_q(), &registry(&[&a, &b, &c]));
    let o = answered(&w);
    assert_eq!((o.answer.clone(), o.by.as_str(), o.kind, o.conf), (pick("x"), "rule:a", Kind::Rule, Some(0.7)));
    assert_eq!(o.pair, [("rule:a".to_string(), pick("x")), ("decider:b".to_string(), pick("x"))]);
    assert!(o.why.contains("agree"));
    assert_eq!(c.calls(), 0, "a third is not needed");
    assert_eq!(steps(&notes).len(), 2);
    assert_eq!(pair_json(&o.pair), json!([{"by": "rule:a", "answer": {"answer": "pick", "id": "x"}}, {"by": "decider:b", "answer": {"answer": "pick", "id": "x"}}]));
}

#[test]
fn two_that_disagree_go_to_the_operator_with_both_answers_and_no_third_is_asked() {
    let (a, b, c) = (says("rule:a", Kind::Rule, pick("x"), 1.0), says("decider:b", Kind::Decider, pick("y"), 1.0), says("model:c", Kind::Model, pick("x"), 1.0));
    let (w, notes) = once(&gate_point(), &gate_q(), &registry(&[&a, &b, &c]));
    let (after, why, pair) = to_human(&w);
    assert_eq!((after, why), (Some(30_000), "the two answerers disagreed"));
    assert_eq!(pair, &[("rule:a".to_string(), pick("x")), ("decider:b".to_string(), pick("y"))]);
    assert_eq!(c.calls(), 0, "the tie is not broken by a third machine");
    assert!(said(&notes, "disagreed"));
    // Headless the conservative answer stands and the pair is kept for the label.
    let (w, _) = go(&gate_point(), &gate_q(), &registry(&[&a, &b, &c]), &Ledger::new(), true);
    match w {
        Walk::Fallback(o) => {
            assert_eq!(o.answer, pick("safe"));
            assert_eq!(o.pair.len(), 2);
            assert!(o.why.contains("no operator"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn fewer_than_two_available_goes_to_the_operator() {
    let a = says("rule:a", Kind::Rule, pick("x"), 1.0);
    let (w, _) = once(&gate_point(), &gate_q(), &registry(&[&a]));
    let (_, why, pair) = to_human(&w);
    assert_eq!(why, "fewer than two answerers were available");
    assert_eq!(pair.len(), 1);
    let (w, _) = once(&gate_point(), &gate_q(), &Registry::new());
    assert!(to_human(&w).2.is_empty());
    // None at all, headless: the fallback.
    assert!(matches!(go(&gate_point(), &gate_q(), &Registry::new(), &Ledger::new(), true).0, Walk::Fallback(_)));
}

#[test]
fn an_answer_below_its_minimum_or_invalid_or_failed_is_not_one_of_the_pair() {
    let a = says("rule:a", Kind::Rule, pick("x"), 1.0);
    for second in [
        says("decider:b", Kind::Decider, pick("x"), 0.59),
        says("decider:b", Kind::Decider, pick("nope"), 1.0),
        says("decider:b", Kind::Decider, pick("x"), f32::NAN),
        fails("decider:b", Kind::Decider),
        passes("decider:b", Kind::Decider),
    ] {
        let (w, _) = once(&gate_point(), &gate_q(), &registry(&[&a, &second]));
        assert!(matches!(w, Walk::ToHuman { .. }), "{w:?}");
    }
    // A third answerer may make the pair when the second did not count.
    let (b, c) = (says("decider:b", Kind::Decider, pick("x"), 0.1), says("model:c", Kind::Model, pick("x"), 0.2));
    let (w, _) = once(&gate_point(), &gate_q(), &registry(&[&a, &b, &c]));
    assert_eq!(answered(&w).pair.iter().map(|p| p.0.as_str()).collect::<Vec<_>>(), ["rule:a", "model:c"]);
}

#[test]
fn a_rank_is_not_one_of_the_pair() {
    let (rank, a) = (says("rank", Kind::Rank, pick("x"), 1.0), says("rule:a", Kind::Rule, pick("x"), 1.0));
    let p = point(Impact::Gate, ShapeKind::Choice, &["rank", "rule:a", "human"], Fallback::Safe);
    let (w, notes) = once(&p, &gate_q(), &registry(&[&rank, &a]));
    assert!(matches!(w, Walk::ToHuman { .. }), "{w:?}");
    assert_eq!(rank.calls(), 0);
    assert_eq!(step_of(&notes, "rank")["outcome"], "skipped");
}

#[test]
fn one_answerer_listed_twice_is_not_a_pair() {
    let a = says("rule:a", Kind::Rule, pick("x"), 1.0);
    let p = point(Impact::Gate, ShapeKind::Choice, &["rule:a", "rule:a", "human"], Fallback::Safe);
    assert!(matches!(once(&p, &gate_q(), &registry(&[&a])).0, Walk::ToHuman { .. }));
    assert_eq!(a.calls(), 1);
}

#[test]
fn an_answerer_that_is_off_cannot_make_a_pair() {
    let (a, b) = (says("rule:a", Kind::Rule, pick("x"), 1.0), says("decider:b", Kind::Decider, pick("x"), 1.0));
    let state = Answerers::new();
    let mut f = state.clone();
    f.apply(&ev(1, 10, "answerer.shadow", json!({"answerer": "decider:b", "why": "rate"})));
    let (w, _) = walk(&gate_point(), &gate_q(), &registry(&[&a, &b]), &state, &Ledger::new(), 20, false);
    assert!(matches!(w, Walk::ToHuman { .. }));
    assert_eq!(b.calls(), 0);
}

#[test]
fn text_answers_pair_when_equal() {
    let (a, b) = (says("rule:a", Kind::Rule, text("cargo test"), 1.0), says("decider:b", Kind::Decider, text("cargo test"), 1.0));
    let p = point(Impact::Gate, ShapeKind::Text, &["rule:a", "decider:b", "human"], Fallback::Text("make test".into()));
    let q = question("what is the test command?", AskShape::Text);
    assert_eq!(answered(&once(&p, &q, &registry(&[&a, &b])).0).answer, text("cargo test"));
    let c = says("decider:b", Kind::Decider, text("cargo test "), 1.0);
    assert!(matches!(once(&p, &q, &registry(&[&a, &c])).0, Walk::ToHuman { .. }), "equal means equal");
}

// ---- an empty offered set never parks ----

#[test]
fn an_empty_offered_set_never_parks() {
    let a = says("rule:a", Kind::Rule, pick("x"), 1.0);
    let reg = registry(&[&a]);
    let empty = choice("which?", &[]);
    // Gate impact: the operator (or, headless, the fallback if it has an answer).
    let g = point(Impact::Gate, ShapeKind::Choice, &["rule:a", "decider:b", "human"], Fallback::Safe);
    let (w, notes) = once(&g, &empty, &reg);
    assert_eq!(to_human(&w).1, "nothing is offered to choose from");
    assert!(said(&notes, "nothing is offered"));
    assert!(matches!(go(&g, &empty, &reg, &Ledger::new(), true).0, Walk::Skip { .. }), "no operator and `safe` is not offered");
    let g_text = point(Impact::Gate, ShapeKind::Choice, &["human"], Fallback::Text("none".into()));
    assert!(matches!(go(&g_text, &empty, &reg, &Ledger::new(), true).0, Walk::Fallback(_)));
    // Lower impact: a note and the fallback, or a skip; never the operator, never a call.
    let l = point(Impact::Low, ShapeKind::Choice, &["rule:a", "human"], Fallback::Rank);
    let (w, notes) = once(&l, &empty, &reg);
    assert!(matches!(w, Walk::Skip { .. }), "{w:?}");
    assert!(said(&notes, "nothing is offered"));
    let lt = point(Impact::Low, ShapeKind::Choice, &["rule:a", "human"], Fallback::Text("whatever".into()));
    match once(&lt, &empty, &reg).0 {
        Walk::Fallback(o) => assert_eq!(o.answer, text("whatever")),
        other => panic!("{other:?}"),
    }
    assert_eq!(a.calls(), 0);
}

// ---- the fallback ----

fn fb(f: Fallback, q: &Question, reg: &Registry) -> Option<Answer> {
    fallback(&point(Impact::Low, ShapeKind::of(&q.shape), &["human"], f), q, reg).map(|o| {
        assert!(q.shape.check(&o.answer).is_ok(), "{o:?}");
        o.answer
    })
}

#[test]
fn rank_fallback_asks_the_rank_answerer_else_takes_the_recommended_then_the_first() {
    let q = question("which?", AskShape::Choice { options: vec![opt("a", false), opt("b", true), opt("c", false)] });
    assert_eq!(fb(Fallback::Rank, &q, &Registry::new()), Some(pick("b")), "no rank installed: the recommended one");
    let plain = choice("which?", &["m", "n"]);
    assert_eq!(fb(Fallback::Rank, &plain, &Registry::new()), Some(pick("m")), "none recommended: the first offered");
    let rank = says("rank", Kind::Rank, pick("c"), 0.2);
    assert_eq!(fb(Fallback::Rank, &q, &registry(&[&rank])), Some(pick("c")), "the rank answerer's head, whatever its confidence");
    let o = fallback(&point(Impact::Low, ShapeKind::Choice, &["human"], Fallback::Rank), &q, &registry(&[&rank])).unwrap();
    assert_eq!((o.by.as_str(), o.kind, o.conf), ("rank", Kind::Rank, None));
    for bad in [says("rank", Kind::Rank, pick("zz"), 1.0), passes("rank", Kind::Rank), fails("rank", Kind::Rank), fake("rank", Kind::Rank, |_| panic!("no"))] {
        assert_eq!(fb(Fallback::Rank, &q, &registry(&[&bad])), Some(pick("b")), "an answer that is not offered is not taken");
    }
    let not_rank = says("rank", Kind::Model, pick("c"), 1.0);
    assert_eq!(fb(Fallback::Rank, &q, &registry(&[&not_rank])), Some(pick("b")), "only a rank is the rank");
    assert_eq!(not_rank.calls(), 0);
    assert_eq!(fb(Fallback::Rank, &yesno("ok?"), &Registry::new()), None, "a yes/no cannot be ranked");
    assert_eq!(fb(Fallback::Rank, &choice("which?", &[]), &Registry::new()), None);
}

#[test]
fn safe_fallback_is_no_or_the_option_named_safe() {
    assert_eq!(fb(Fallback::Safe, &yesno("ok?"), &Registry::new()), Some(Answer::No));
    assert_eq!(fb(Fallback::Safe, &approval("make build?"), &Registry::new()), Some(Answer::No));
    assert_eq!(fb(Fallback::Safe, &choice("which?", &["a", "safe"]), &Registry::new()), Some(pick("safe")));
    assert_eq!(fb(Fallback::Safe, &choice("which?", &["a", "b"]), &Registry::new()), None);
    assert_eq!(fb(Fallback::Safe, &question("say", AskShape::Text), &Registry::new()), None);
}

#[test]
fn pick_fallback_must_be_offered_else_the_first_offered() {
    let q = choice("which?", &["a", "b"]);
    assert_eq!(fb(Fallback::Pick("b".into()), &q, &Registry::new()), Some(pick("b")));
    assert_eq!(fb(Fallback::Pick("zzz".into()), &q, &Registry::new()), Some(pick("a")));
    assert_eq!(fb(Fallback::Pick("a".into()), &choice("which?", &[]), &Registry::new()), None);
    assert_eq!(fb(Fallback::Pick("a".into()), &yesno("ok?"), &Registry::new()), None);
}

#[test]
fn text_fallback_is_the_line_when_the_shape_takes_text_and_the_line_is_plain() {
    let line = Fallback::Text("use the default".into());
    assert_eq!(fb(line.clone(), &question("say", AskShape::Text), &Registry::new()), Some(text("use the default")));
    assert_eq!(fb(line.clone(), &choice("which?", &["a"]), &Registry::new()), Some(text("use the default")));
    assert_eq!(fb(line, &yesno("ok?"), &Registry::new()), None);
    let long = "x".repeat(TEXT_MAX + 1);
    for bad in ["", "a\nb", long.as_str()] {
        assert_eq!(fb(Fallback::Text(bad.into()), &question("say", AskShape::Text), &Registry::new()), None);
    }
}

// ---- the registry ----

#[test]
fn the_registry_installs_by_id_replaces_and_refuses_the_operator_and_the_nameless() {
    let mut r = Registry::new();
    let (a1, a2) = (passes("rule:a", Kind::Rule), says("rule:a", Kind::Rule, Answer::Yes, 1.0));
    assert!(r.register(a1.clone()) && r.register(a2.clone()));
    assert_eq!(r.ids(), ["rule:a"]);
    assert_eq!(r.get("rule:a").unwrap().offer(&yesno("ok?")), Offer::Answer { answer: Answer::Yes, conf: 1.0, why: "rule:a says so".into() });
    assert!(r.get("rule:b").is_none());
    assert!(!r.register(passes("human", Kind::Human)));
    assert!(!r.register(passes("", Kind::Rule)));
    assert_eq!(r.clone().ids(), ["rule:a"]);
}

// ---- random offers never produce an answer the shape rejects ----

struct Lcg(u64);

impl Lcg {
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

fn random_offer(r: &mut Lcg, q: &Question) -> Offer {
    let ids: Vec<String> = match &q.shape {
        AskShape::Choice { options } => options.iter().map(|o| o.id.clone()).collect(),
        _ => Vec::new(),
    };
    let answer = match r.below(9) {
        0 => Answer::Yes,
        1 => Answer::No,
        2 if !ids.is_empty() => pick(&ids[r.below(ids.len())]),
        2 | 3 => pick("junk"),
        4 => text("a fine line"),
        5 => text("two\nlines"),
        6 => text(&"x".repeat(TEXT_MAX + r.below(3))),
        7 => text(""),
        _ => text("é\u{0}"),
    };
    let conf = [0.0, 0.3, 0.6, 0.95, 1.0, 1.5, -1.0, f32::NAN][r.below(8)];
    match r.below(10) {
        0 => Offer::Pass { why: "pass\nwhy".into() },
        1 => Offer::Failed { why: "failed".into() },
        _ => Offer::Answer { answer, conf, why: "because\n".repeat(r.below(3)) },
    }
}

#[test]
fn random_offers_never_produce_an_answer_the_shape_rejects_or_a_model_answer_to_an_approval() {
    let mut r = Lcg(0x9E37_79B9_7F4A_7C15);
    let seed = Arc::new(Mutex::new(Lcg(0xD1B5_4A32_D192_ED03)));
    let (mut taken, mut approvals) = (0, 0);
    for round in 0..3000 {
        let mut fakes: Vec<Arc<Fake>> = Vec::new();
        for (id, kind) in [("rule:r1", Kind::Rule), ("rule:r2", Kind::Rule), ("rank", Kind::Rank), ("decider:d", Kind::Decider), ("model:m", Kind::Model)] {
            let seed = seed.clone();
            fakes.push(fake(id, kind, move |q| random_offer(&mut seed.lock().unwrap(), q)));
        }
        let reg = registry(&fakes.iter().collect::<Vec<_>>());
        let shape_n = r.below(5);
        let options: Vec<Opt> = (0..r.below(4)).map(|i| opt(["a", "b", "safe"][i % 3], i == 1)).collect();
        let shape = [AskShape::YesNo, AskShape::Choice { options }, AskShape::Approval { action: "x".into(), risk: 9 }, AskShape::Text, AskShape::Choice { options: vec![opt("a", false)] }][shape_n].clone();
        let impact = [Impact::Low, Impact::Mid, Impact::Gate][r.below(3)];
        let words = ["rule:r1", "rule:r2", "rank", "decider:d@0.5", "model:m@0.9", "model:absent", "rule:r1", "human@10"];
        let chain: Vec<&str> = (0..r.below(7)).map(|_| words[r.below(words.len())]).collect();
        let fb = [Fallback::Rank, Fallback::Safe, Fallback::Pick("b".into()), Fallback::Text("fine".into())][r.below(4)].clone();
        let p = point(impact, ShapeKind::of(&shape), &chain, fb);
        let q = question(&format!("question {round}"), shape.clone());
        let mut state = Answerers::new();
        if r.below(4) == 0 {
            let who = ["rule:r1", "decider:d", "model:m"][r.below(3)];
            state.apply(&ev(1, 1, "answerer.shadow", json!({"answerer": who, "why": "x"})));
        }
        let headless = r.below(3) == 0;
        let ledger = Ledger::new();
        let (w, notes) = walk(&p, &q, &reg, &state, &ledger, 50, headless);
        let is_approval = matches!(shape, AskShape::Approval { .. });
        let mut checked = |answer: &Answer| {
            assert!(shape.check(answer).is_ok(), "round {round}: {answer:?} for {shape:?}");
            taken += 1;
        };
        match &w {
            Walk::Answered(o) | Walk::Fallback(o) => {
                checked(&o.answer);
                o.pair.iter().for_each(|(_, a)| checked(a));
                if let Some(c) = o.conf {
                    assert!((0.0..=1.0).contains(&c), "{c}");
                }
                if matches!(w, Walk::Answered(_)) {
                    assert!(!(is_approval && o.kind.is_model()), "round {round}: a model answered an approval");
                    assert!(matches!(o.kind, Kind::Rule | Kind::Rank | Kind::Decider | Kind::Model));
                    if impact == Impact::Gate && !is_approval {
                        assert_eq!(o.pair.len(), 2, "round {round}: a gate answer is a pair");
                        assert!(o.pair[0].0 != o.pair[1].0 && o.pair[0].1 == o.pair[1].1);
                        assert!(o.pair.iter().all(|(by, _)| by != "rank"));
                    }
                }
            }
            Walk::ToHuman { pair, .. } => {
                assert!(!headless, "round {round}: headless never waits for the operator");
                pair.iter().for_each(|(_, a)| checked(a));
            }
            Walk::Skip { .. } => {}
        }
        if is_approval {
            approvals += 1;
            assert_eq!((fakes[3].calls(), fakes[4].calls()), (0, 0), "round {round}: an approval reached a model");
        }
        assert!(ledger.given() <= MAX_MACHINE);
        for n in &notes {
            let why = n.data["why"].as_str().unwrap_or("");
            assert!(!why.chars().any(char::is_control) && why.chars().count() <= 200);
        }
    }
    assert!(taken > 1000 && approvals > 300, "the run covered something: {taken} answers, {approvals} approvals");
}

// ---- the state fold and the breaker ----

fn ev(seq: u64, at_ms: u64, kind: &str, data: Json) -> Ev {
    Ev { seq, at_ms, kind: kind.into(), task: None, thread: None, by: Actor::Core, cause: None, class: DataClass::default(), data }
}

fn step_ev(seq: u64, at: u64, who: &str, outcome: &str) -> Ev {
    ev(seq, at, "ask.step", json!({"ask": "t3", "answerer": who, "kind": "decider", "outcome": outcome, "why": "x", "ms": 5}))
}

fn feed(fold: &Answerers, events: &[Ev]) {
    let mut f = fold.clone();
    events.iter().for_each(|e| f.apply(e));
}

#[test]
fn three_failures_in_a_row_open_the_breaker_for_a_day() {
    let a = Answerers::new();
    feed(&a, &[step_ev(1, 1_000, "decider:j", "failed"), step_ev(2, 2_000, "decider:j", "failed")]);
    assert_eq!(a.state("decider:j", 3_000), State::Active);
    assert_eq!(a.just_opened("decider:j"), None);
    feed(&a, &[step_ev(3, 5_000, "decider:j", "failed")]);
    let until = 5_000 + OFF_MS;
    assert_eq!(BREAKER_AFTER, 3);
    assert_eq!(a.state("decider:j", 5_000), State::Off { until_ms: until });
    assert_eq!(a.state("decider:j", until - 1), State::Off { until_ms: until });
    assert_eq!(a.state("decider:j", until), State::Active, "at `until` it is active again");
    assert_eq!(a.state("decider:other", 5_000), State::Active, "the breaker is per answerer");
    assert_eq!(a.just_opened("decider:j"), Some(Opened { until_ms: until, failures: 3 }));
    assert_eq!(a.just_opened("decider:j"), Some(Opened { until_ms: until, failures: 3 }), "it stays until the notice is in the Log");
    feed(&a, &[ev(4, 5_001, "answerer.off", json!({"answerer": "decider:j", "until_ms": until, "failures": 3}))]);
    assert_eq!(a.just_opened("decider:j"), None, "the notice is written once");
    assert_eq!(a.state("decider:j", 6_000), State::Off { until_ms: until }, "the notice does not close it");
}

#[test]
fn an_answer_or_a_pass_ends_the_streak_and_a_skip_does_not() {
    for between in ["answer", "pass"] {
        let a = Answerers::new();
        feed(&a, &[step_ev(1, 10, "d", "failed"), step_ev(2, 20, "d", "failed"), step_ev(3, 30, "d", between), step_ev(4, 40, "d", "failed"), step_ev(5, 50, "d", "failed")]);
        assert_eq!(a.state("d", 60), State::Active, "{between}: two, then {between}, then two");
    }
    let a = Answerers::new();
    feed(&a, &[step_ev(1, 10, "d", "failed"), step_ev(2, 20, "d", "failed"), step_ev(3, 30, "d", "skipped"), step_ev(4, 40, "d", "failed")]);
    assert!(matches!(a.state("d", 50), State::Off { .. }), "a skipped step is not an answer");
    let a = Answerers::new();
    feed(&a, &[step_ev(1, 10, "d", "failed"), step_ev(2, 20, "d", "failed"), step_ev(3, 30, "d", "mystery"), step_ev(4, 40, "d", "failed")]);
    assert!(matches!(a.state("d", 50), State::Off { .. }), "an outcome this version does not know changes nothing");
}

#[test]
fn after_the_day_one_more_failure_opens_it_again_and_an_answer_starts_the_count_over() {
    let a = Answerers::new();
    feed(&a, &[step_ev(1, 100, "d", "failed"), step_ev(2, 200, "d", "failed"), step_ev(3, 300, "d", "failed"), ev(4, 301, "answerer.off", json!({"answerer": "d"}))]);
    let day = 300 + OFF_MS;
    feed(&a, &[step_ev(5, day + 10, "d", "failed")]);
    assert_eq!(a.state("d", day + 11), State::Off { until_ms: day + 10 + OFF_MS }, "the streak was kept");
    assert_eq!(a.just_opened("d"), Some(Opened { until_ms: day + 10 + OFF_MS, failures: 4 }), "a second opening is a second notice");
    let b = Answerers::new();
    feed(&b, &[step_ev(1, 100, "d", "failed"), step_ev(2, 200, "d", "failed"), step_ev(3, 300, "d", "failed"), step_ev(4, day + 10, "d", "answer")]);
    feed(&b, &[step_ev(5, day + 20, "d", "failed"), step_ev(6, day + 30, "d", "failed")]);
    assert_eq!(b.state("d", day + 40), State::Active, "after an answer it takes three again");
    feed(&b, &[step_ev(7, day + 50, "d", "failed")]);
    assert!(matches!(b.state("d", day + 60), State::Off { .. }));
}

#[test]
fn a_late_failure_or_answer_while_it_is_open_neither_extends_nor_closes_it() {
    let a = Answerers::new();
    feed(&a, &[step_ev(1, 100, "d", "failed"), step_ev(2, 200, "d", "failed"), step_ev(3, 300, "d", "failed"), ev(4, 301, "answerer.off", json!({"answerer": "d"}))]);
    feed(&a, &[step_ev(5, 400, "d", "failed")]);
    assert_eq!(a.state("d", 500), State::Off { until_ms: 300 + OFF_MS }, "not extended");
    assert_eq!(a.just_opened("d"), None, "and no second notice");
    feed(&a, &[step_ev(6, 600, "d", "answer")]);
    assert_eq!(a.state("d", 700), State::Off { until_ms: 300 + OFF_MS }, "not closed early");
    feed(&a, &[step_ev(7, 800, "d", "failed"), step_ev(8, 900, "d", "failed")]);
    assert_eq!(a.state("d", OFF_MS + 301), State::Active, "the streak restarted at the answer; two failures are not three");
}

#[test]
fn shadow_and_active_are_events_and_off_wins_over_shadow() {
    let a = Answerers::new();
    feed(&a, &[ev(1, 10, "answerer.shadow", json!({"answerer": "m", "why": "wrong rate 0.30 over 25"}))]);
    assert_eq!(a.state("m", 20), State::Shadow { why: "wrong rate 0.30 over 25".into() });
    feed(&a, &[step_ev(2, 30, "m", "failed"), step_ev(3, 31, "m", "failed"), step_ev(4, 32, "m", "failed")]);
    assert!(matches!(a.state("m", 40), State::Off { .. }), "off is the stronger word");
    assert_eq!(a.state("m", 32 + OFF_MS), State::Shadow { why: "wrong rate 0.30 over 25".into() }, "still shadow when the day is over");
    feed(&a, &[ev(5, 50, "answerer.active", json!({"answerer": "m"}))]);
    assert!(matches!(a.state("m", 60), State::Off { .. }), "a promotion does not reopen a breaker");
    assert_eq!(a.state("m", 32 + OFF_MS), State::Active);
    feed(&a, &[ev(6, 70, "answerer.shadow", json!({"answerer": "m"}))]);
    assert_eq!(a.state("m", 32 + OFF_MS), State::Shadow { why: String::new() }, "a shadow without a reason is still a shadow");
}

#[test]
fn events_this_fold_does_not_understand_change_nothing() {
    let a = Answerers::new();
    feed(
        &a,
        &[
            ev(1, 1, "ask.step", json!({"outcome": "failed"})),
            ev(2, 2, "ask.step", json!({"answerer": "", "outcome": "failed"})),
            ev(3, 3, "ask.step", json!({"answerer": 7, "outcome": "failed"})),
            ev(4, 4, "ask.step", json!("text")),
            ev(5, 5, "ask.step", Json::Null),
            ev(6, 6, "ask.step", json!({"answerer": "d", "outcome": 5})),
            ev(7, 7, "answerer.shadow", json!([1, 2])),
            ev(8, 8, "answerer.off", json!({"answerer": "never-seen"})),
            ev(9, 9, "answerer.active", json!({"answerer": "never-seen"})),
        ],
    );
    assert_eq!(a.save(), json!({}), "nothing is kept for what is default");
    assert!(!a.wants("task.queued") && a.wants("ask.step") && a.wants("answerer.off"));
    assert_eq!((a.id(), a.version()), ("answerers", 3));
}

#[test]
fn the_fold_reads_as_a_value_and_saves_and_loads() {
    let a = Answerers::new();
    feed(&a, &[ev(1, 10, "answerer.shadow", json!({"answerer": "m", "why": "probes"})), step_ev(2, 11, "d", "failed"), step_ev(3, 12, "d", "failed"), step_ev(4, 13, "d", "failed")]);
    let all = a.read(&[], 100);
    let Value::Map(m) = &all else { panic!("{all:?}") };
    assert_eq!(m.len(), 2);
    assert_eq!(a.read(&[Key::Name("d".into()), Key::Name("state".into())], 100), Value::text("off"));
    assert_eq!(a.read(&[Key::Name("d".into()), Key::Name("until_ms".into())], 100), Value::num((13 + OFF_MS) as f64));
    assert_eq!(a.read(&[Key::Name("m".into()), Key::Name("why".into())], 100), Value::text("probes"));
    assert_eq!(a.read(&[Key::Name("d".into()), Key::Name("state".into())], 13 + OFF_MS), Value::text("active"));
    assert_eq!(a.read(&[Key::Name("nobody".into()), Key::Name("state".into())], 100), Value::text("active"));
    let mut b = Answerers::new();
    b.load(a.save()).unwrap();
    assert_eq!(b.save(), a.save());
    assert_eq!(b.state("d", 100), a.state("d", 100));
    assert_eq!(b.just_opened("d"), a.just_opened("d"));
    assert!(b.load(json!("garbage")).is_err() && b.load(json!({"d": {"failures": "many"}})).is_err());
    b.reset();
    assert_eq!(b.save(), json!({}));
}

#[test]
fn a_fold_started_from_a_snapshot_equals_one_fed_every_event() {
    let mut r = Lcg(0x2545_F491_4F6C_DD1D);
    let who = ["decider:a", "model:b", "rule:c"];
    let mut at = 1_000u64;
    let events: Vec<Ev> = (1..=300u64)
        .map(|seq| {
            at += (r.next() % 20_000_000) * (r.below(3) as u64 == 0) as u64 + r.next() % 1000;
            let w = who[r.below(3)];
            match r.below(12) {
                0 => ev(seq, at, "answerer.shadow", json!({"answerer": w, "why": "rate"})),
                1 => ev(seq, at, "answerer.active", json!({"answerer": w})),
                2 => ev(seq, at, "answerer.off", json!({"answerer": w})),
                3 | 4 => step_ev(seq, at, w, ["answer", "pass", "skipped"][r.below(3)]),
                _ => step_ev(seq, at, w, "failed"),
            }
        })
        .collect();
    let end = at;
    let picture = |host: &Folds| {
        let reads: Vec<String> = [0, end / 2, end, end + OFF_MS].iter().map(|&t| format!("{:?}", host.read("answerers", &[], t))).collect();
        (host.state("answerers"), reads)
    };
    let s = Scratch::new("answerers-ref");
    let reference = Folds::new(&s.join("folds"));
    assert!(reference.add(Box::new(Answerers::new()), 0).is_empty());
    let mut sink = reference.clone();
    events.iter().for_each(|e| sink.event(e));
    let want = picture(&reference);
    assert!(want.0.as_ref().unwrap().to_string().len() > 20, "the walk of events left something to compare");
    for k in [0usize, 1, 2, 5, 40, 150, 299, 300] {
        let t = Scratch::new("answerers-case");
        let before = Folds::new(&t.join("folds"));
        before.add(Box::new(Answerers::new()), 0);
        let mut sink = before.clone();
        events[..k].iter().for_each(|e| sink.event(e));
        assert!(before.snapshot().is_empty());
        let after = Folds::new(&t.join("folds"));
        assert!(after.add(Box::new(Answerers::new()), 300).is_empty());
        assert_eq!(after.applied("answerers"), Some(k as u64), "started from the snapshot at {k}");
        let mut sink = after.clone();
        events[k..].iter().for_each(|e| sink.event(e));
        assert_eq!(picture(&after), want, "from a snapshot at {k}");
    }
}

// ---- the breaker end to end, through a real Log and the fold host ----

struct Loop {
    log: Log,
    clock: Arc<ManualClock>,
    folds: Folds,
    state: Answerers,
    dir: Scratch,
}

impl Loop {
    fn new() -> Loop {
        let dir = Scratch::new("breaker");
        let (log, clock) = open_log(&dir);
        let state = Answerers::new();
        let folds = Folds::new(&dir.join("folds"));
        assert!(folds.add(Box::new(state.clone()), 0).is_empty());
        folds.attach(&log).unwrap();
        Loop { log, clock, folds, state, dir }
    }

    /// What the loop does for one ask: walk, write the notes, write the one `answerer.off` notice when a breaker opened.
    fn ask(&self, p: &AskPoint, q: &Question, reg: &Registry, ledger: &Ledger) -> Walk {
        let (w, notes) = walk(p, q, reg, &self.state, ledger, self.clock.now_ms(), false);
        for n in notes {
            self.log.append(New::of(&n.kind, Actor::Core, n.data)).unwrap();
        }
        self.log.sync();
        for id in reg.ids() {
            if let Some(o) = self.state.just_opened(&id) {
                self.log.append(New::of("answerer.off", Actor::Core, json!({"answerer": id, "until_ms": o.until_ms, "failures": o.failures}))).unwrap();
                self.log.sync();
            }
        }
        w
    }

    fn count(&self, kind: &str) -> usize {
        all(&self.log).iter().filter(|e| e.kind == kind).count()
    }

    fn steps_of(&self, answerer: &str, outcome: &str) -> usize {
        all(&self.log).iter().filter(|e| e.kind == "ask.step" && e.data["answerer"] == answerer && e.data["outcome"] == outcome).count()
    }
}

#[test]
fn an_answerer_that_keeps_failing_is_left_alone_and_says_so_once() {
    let rig = Loop::new();
    let flaky = fails("decider:jev", Kind::Decider);
    let p = point(Impact::Low, ShapeKind::YesNo, &["decider:jev", "human"], Fallback::Safe);
    let reg = registry(&[&flaky]);
    for i in 0..3 {
        rig.clock.advance(1_000);
        assert!(matches!(rig.ask(&p, &yesno(&format!("failing {i}")), &reg, &Ledger::new()), Walk::ToHuman { .. }));
        assert_eq!(rig.count("answerer.off"), usize::from(i == 2), "the notice comes with the third failure");
    }
    assert_eq!(flaky.calls(), 3);
    // The fourth ask, and the fifth in the same task: no call, one note, one notice in all.
    let task = Ledger::new();
    rig.clock.advance(1_000);
    assert!(matches!(rig.ask(&p, &yesno("fourth"), &reg, &task), Walk::ToHuman { .. }));
    assert!(matches!(rig.ask(&p, &yesno("fifth"), &reg, &task), Walk::ToHuman { .. }));
    assert_eq!(flaky.calls(), 3, "an answerer that is off is not called, so nothing is sent");
    assert_eq!(rig.steps_of("decider:jev", "skipped"), 1);
    assert_eq!(rig.count("answerer.off"), 1);
    let off = all(&rig.log).into_iter().find(|e| e.kind == "answerer.off").unwrap();
    assert_eq!((off.data["answerer"].as_str(), off.data["failures"].as_u64()), (Some("decider:jev"), Some(3)));
    let third = all(&rig.log).into_iter().filter(|e| e.kind == "ask.step" && e.data["outcome"] == "failed").nth(2).unwrap();
    assert_eq!(off.data["until_ms"].as_u64(), Some(third.at_ms + OFF_MS), "the day counts from the third failure");

    // It survives a restart: a new host starts from the snapshot, then replays what came after.
    rig.folds.snapshot();
    let before_snapshot_events = all(&rig.log).len();
    assert_eq!(before_snapshot_events, 5, "three failures, the notice, one skip");
    let reborn = Answerers::new();
    let host = Folds::new(&rig.dir.join("folds"));
    assert!(host.add(Box::new(reborn.clone()), rig.log.head()).is_empty());
    assert_eq!(host.applied("answerers"), Some(rig.log.head()), "started from the snapshot");
    host.attach(&rig.log).unwrap();
    assert!(matches!(reborn.state("decider:jev", rig.clock.now_ms()), State::Off { .. }));
    assert_eq!(reborn.just_opened("decider:jev"), None, "the notice is not written again after a restart");
    let (w, notes) = walk(&p, &yesno("sixth"), &reg, &reborn, &Ledger::new(), rig.clock.now_ms(), false);
    assert!(matches!(w, Walk::ToHuman { .. }));
    assert_eq!(flaky.calls(), 3);
    assert_eq!(steps(&notes), [("decider:jev".into(), "skipped".into())]);
    // And from a full replay.
    let full = Folds::new(&rig.dir.join("folds-full"));
    let replayed = Answerers::new();
    full.add(Box::new(replayed.clone()), 0);
    full.attach(&rig.log).unwrap();
    assert_eq!(replayed.save(), reborn.save());

    // The day passes: it gets one call, fails once more, and the breaker closes the door again with a new notice.
    rig.clock.advance(OFF_MS);
    rig.ask(&p, &yesno("seventh"), &reg, &Ledger::new());
    assert_eq!(flaky.calls(), 4);
    assert_eq!(rig.count("answerer.off"), 2);
    rig.ask(&p, &yesno("eighth"), &reg, &Ledger::new());
    assert_eq!(flaky.calls(), 4);
}

#[test]
fn an_answerer_that_fails_by_answering_badly_is_counted_the_same() {
    let rig = Loop::new();
    let bad = says("model:m", Kind::Model, pick("not offered"), 1.0);
    let p = point(Impact::Low, ShapeKind::Choice, &["model:m", "human"], Fallback::Rank);
    let reg = registry(&[&bad]);
    for i in 0..4 {
        rig.clock.advance(10);
        rig.ask(&p, &choice(&format!("which {i}?"), &["a", "b"]), &reg, &Ledger::new());
    }
    assert_eq!(bad.calls(), 3);
    assert_eq!(rig.steps_of("model:m", "failed"), 3);
    assert_eq!(rig.count("answerer.off"), 1);
}

#[test]
fn a_flaky_answerer_that_answers_in_between_is_never_cut_off() {
    let rig = Loop::new();
    let n = Arc::new(AtomicUsize::new(0));
    let c = n.clone();
    let flaky = fake("decider:jev", Kind::Decider, move |_| {
        if c.fetch_add(1, Ordering::SeqCst) % 3 == 2 {
            Offer::Answer { answer: Answer::Yes, conf: 0.9, why: "ok".into() }
        } else {
            Offer::Failed { why: "timeout".into() }
        }
    });
    let p = point(Impact::Low, ShapeKind::YesNo, &["decider:jev", "human"], Fallback::Safe);
    for i in 0..30 {
        rig.clock.advance(10);
        rig.ask(&p, &yesno(&format!("q{i}")), &registry(&[&flaky]), &Ledger::new());
    }
    assert_eq!(flaky.calls(), 30);
    assert_eq!(rig.count("answerer.off"), 0);
}

// ---- what an answerer cost ----

/// A decider or a model whose call cost what `paid` says (`None`: it does not report), and which answers `a` at a confidence of `conf`.
struct Pays {
    id: String,
    kind: Kind,
    conf: f32,
    paid: Option<Paid>,
}

impl Answerer for Pays {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind(&self) -> Kind {
        self.kind
    }
    fn offer(&self, q: &Question) -> Offer {
        self.offer_paid(q).0
    }
    fn offer_paid(&self, _: &Question) -> (Offer, Option<Paid>) {
        (Offer::Answer { answer: pick("a"), conf: self.conf, why: "it says so".into() }, self.paid)
    }
}

fn pays(id: &str, kind: Kind, conf: f32, paid: Option<Paid>) -> Arc<Pays> {
    Arc::new(Pays { id: id.into(), kind, conf, paid })
}

fn walked_with(chain: &[&str], answerers: Vec<Arc<dyn Answerer>>) -> Vec<Note> {
    let mut reg = Registry::new();
    answerers.into_iter().for_each(|a| assert!(reg.register(a)));
    let p = point(Impact::Low, ShapeKind::Choice, chain, Fallback::Rank);
    once(&p, &choice("which?", &["a", "b"]), &reg).1
}

#[test]
fn a_decider_that_reports_its_cost_has_it_in_its_step_and_in_the_sum_of_the_walk() {
    let (one, two) = (Paid { usd: Some(0.0004), tokens_in: 120, tokens_out: 7 }, Paid { usd: Some(0.001), tokens_in: 80, tokens_out: 20 });
    // the first is not sure enough (min_conf), so the walk goes on to the second: both calls were paid for
    let notes = walked_with(&["decider:one@0.95", "model:two"], vec![pays("decider:one", Kind::Decider, 0.9, Some(one)), pays("model:two", Kind::Model, 0.9, Some(two))]);
    assert_eq!(steps(&notes), [("decider:one".into(), "pass".into()), ("model:two".into(), "answer".into())]);
    let first = step_of(&notes, "decider:one");
    assert_eq!((first["usd"].as_f64(), first["tokens_in"].as_u64(), first["tokens_out"].as_u64()), (Some(0.0004), Some(120), Some(7)), "{first}");
    assert_eq!(step_of(&notes, "model:two")["usd"].as_f64(), Some(0.001));
    let spent = Spent::of(&notes);
    assert!((spent.usd.unwrap() - 0.0014).abs() < 1e-12, "{spent:?}");
    assert_eq!((spent.tokens_in, spent.tokens_out), (200, 27));
}

#[test]
fn a_decider_that_does_not_report_its_cost_is_unknown_and_never_a_guess_or_nothing() {
    let known = Paid { usd: Some(0.0004), tokens_in: 120, tokens_out: 7 };
    let notes = walked_with(&["decider:silent@0.95", "model:two"], vec![pays("decider:silent", Kind::Decider, 0.9, None), pays("model:two", Kind::Model, 0.9, Some(known))]);
    let silent = step_of(&notes, "decider:silent");
    assert!(silent["usd"].is_null() && silent.as_object().unwrap().contains_key("usd"), "said as unknown: {silent}");
    assert!(silent.get("tokens_in").is_none(), "{silent}");
    let spent = Spent::of(&notes);
    assert_eq!(spent.usd, None, "one unknown step makes the sum unknown, not the part that is known");
    assert_eq!(spent.tokens_in, 120, "the tokens that were reported still add up");
}

#[test]
fn a_rule_and_a_step_that_was_skipped_cost_nothing_and_say_so_by_a_sum_of_zero() {
    let rule = fake("rule:r", Kind::Rule, |_| Offer::Answer { answer: pick("a"), conf: 1.0, why: "it is a fact".into() });
    let notes = walked_with(&["rule:r", "decider:never"], vec![rule]);
    assert!(step_of(&notes, "rule:r").get("usd").is_none(), "a rule has no price to say");
    assert_eq!(Spent::of(&notes), Spent { usd: Some(0.0), tokens_in: 0, tokens_out: 0 });
    // a decider that is not installed is skipped, not called: no price either
    let skipped = walked_with(&["decider:gone", "rule:r"], vec![fake("rule:r", Kind::Rule, |_| Offer::Answer { answer: pick("a"), conf: 1.0, why: "x".into() })]);
    assert!(step_of(&skipped, "decider:gone").get("usd").is_none());
    assert_eq!(Spent::of(&skipped).usd, Some(0.0));
}
