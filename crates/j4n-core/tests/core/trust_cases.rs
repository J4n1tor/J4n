//! Answer trust: the probe cases and what loads of them, the prober and the state it sets
//!, the labels and the rates that shadow an answerer, and the egress a question is put through. A machine answerer is
//! a scripted one that has a model and says what it is told to.

use crate::world::{Scratch, all, ev, layer, open_log};
use j4n_core::answer::chain::{Ledger, Registry, Walk, walk};
use j4n_core::answer::point::points;
use j4n_core::answer::probe::{ProbeSet, Prober, STALE_MS};
use j4n_core::answer::record::Record;
use j4n_core::answer::state::{Answerers, LABELS_DAYS, LABELS_KEPT, Limits};
use j4n_core::answer::{Answerer, AskPoint, Egress, Kind, Offer, Question, State, StateView};
use j4n_core::ask::{Answer, AskShape};
use j4n_core::clock::{Clock, ManualClock};
use j4n_core::fold::Folds;
use j4n_core::gate::Verdict;
use j4n_core::log::Log;
use j4n_core::schema::DataClass;
use j4n_core::table::{LayerKind, Tables};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// A machine answerer with a model, that answers each probe case with what `says` makes of its text.
struct Machine {
    id: String,
    kind: Kind,
    model: Mutex<String>,
    zone: Option<String>,
    says: Box<dyn Fn(&str) -> Offer + Send + Sync>,
    calls: AtomicUsize,
}

impl Machine {
    fn new(id: &str, model: &str, says: impl Fn(&str) -> Offer + Send + Sync + 'static) -> Arc<Machine> {
        Arc::new(Machine { id: id.into(), kind: Kind::Decider, model: Mutex::new(model.into()), zone: None, says: Box::new(says), calls: AtomicUsize::new(0) })
    }

    fn far(id: &str, model: &str, says: impl Fn(&str) -> Offer + Send + Sync + 'static) -> Arc<Machine> {
        Arc::new(Machine { id: id.into(), kind: Kind::Decider, model: Mutex::new(model.into()), zone: Some("far".into()), says: Box::new(says), calls: AtomicUsize::new(0) })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Answerer for Machine {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind(&self) -> Kind {
        self.kind
    }
    fn model(&self) -> String {
        self.model.lock().unwrap().clone()
    }
    fn zone(&self) -> Option<String> {
        self.zone.clone()
    }
    fn offer(&self, q: &Question) -> Offer {
        self.calls.fetch_add(1, Ordering::SeqCst);
        (self.says)(&q.text)
    }
}

fn pick(id: &str) -> Offer {
    Offer::Answer { answer: Answer::Pick { id: id.into() }, conf: 0.9, why: "it knows".into() }
}

/// The right answer to the cases below, by the words of the question: a line about a bug is work, one that asks why is not.
fn knows(text: &str) -> Offer {
    pick(if text.contains("fix") || text.contains("rename") { "task" } else { "answer" })
}

const POINT: &str = r#"
[[ask_point]]
id = "intake"
doc = "what a line is"
shape = "choice"
impact = "mid"
data = "project"
options = ["answer", "task"]
chain = ["decider:jev?min_conf=0.6", "human"]
fallback = "pick:task"

[[ask_point]]
id = "yn"
doc = "a yes or no"
shape = "yesno"
impact = "low"
data = "project"
chain = ["decider:jev", "human"]
fallback = "safe"

[[ask_point]]
id = "talk"
doc = "something to say"
shape = "text"
impact = "low"
data = "project"
chain = ["human"]
fallback = "text:none"
"#;

fn case(id: &str, point: &str, line: &str, offered: &[&str], expect: &str, pair: &str) -> String {
    let offered: Vec<String> = offered.iter().map(|o| format!("\"{o}\"")).collect();
    format!("[[probe]]\nid = \"{id}\"\npoint = \"{point}\"\nslices = {{ line = \"{line}\" }}\noffered = [{}]\nexpect = \"{expect}\"\npair = \"{pair}\"\n", offered.join(", "))
}

fn good_cases() -> String {
    let both = ["answer", "task"];
    [
        case("work", "intake", "fix the failing test", &both, "task", "ask"),
        case("ask", "intake", "why does the test fail?", &both, "answer", "work"),
    ]
    .join("\n")
}

fn tables(cases: &str) -> Tables {
    let (kinds, _) = Tables::load(vec![]);
    drop(kinds);
    let l = layer(LayerKind::Pack, "t", &[("a.toml", &format!("api = 1\n{POINT}\n{cases}"))]);
    let (t, problems) = Tables::load(vec![l]);
    assert!(problems.iter().all(|p| !p.msg.contains("ask_point")), "{problems:?}");
    t
}

fn points_of(t: &Tables) -> BTreeMap<String, AskPoint> {
    let (list, problems) = points(t);
    assert!(problems.is_empty(), "{problems:?}");
    list.into_iter().map(|p| (p.id.clone(), p)).collect()
}

fn load(cases: &str) -> (ProbeSet, Vec<String>) {
    let t = tables(cases);
    let (set, problems) = ProbeSet::load(&t, &points_of(&t));
    (set, problems.iter().map(ToString::to_string).collect())
}

// ---- the cases ----

#[test]
fn a_pair_of_cases_whose_right_answers_differ_loads_and_has_a_hash() {
    let (set, problems) = load(&good_cases());
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(set.cases("intake").iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["ask", "work"]);
    assert!(set.cases("yn").is_empty() && !set.is_empty());
    let h = set.hashes();
    assert_eq!(h.len(), 1);
    assert_eq!(h["intake"].len(), 16);
    // the hash is what the cases are, and is the same on every load
    assert_eq!(load(&good_cases()).0.hashes(), h);
    let edited = good_cases().replace("fix the failing test", "fix the failing tests");
    assert_ne!(load(&edited).0.hashes()["intake"], h["intake"]);
    let other_answer = good_cases().replace("expect = \"task\"", "expect = \"answer\"").replace("expect = \"answer\"\npair = \"work\"", "expect = \"task\"\npair = \"work\"");
    assert_ne!(load(&other_answer).0.hashes()["intake"], h["intake"]);
}

#[test]
fn a_case_that_cannot_be_put_or_has_no_proper_twin_is_refused_with_its_twin() {
    let both = ["answer", "task"];
    let ok = || case("ask", "intake", "why does the test fail?", &both, "answer", "work");
    let faults: Vec<(String, &str)> = vec![
        (format!("{}\n{}", case("work", "nowhere", "x", &both, "task", "ask"), ok()), "no ask point `nowhere`"),
        (format!("{}\n{}", case("work", "talk", "x", &[], "yes", "ask"), ok()), "which is not probed"),
        (format!("{}\n{}", case("work", "intake", "x", &both, "flow", "ask"), ok()), "not among what is offered"),
        (format!("{}\n{}", case("work", "yn", "x", &both, "yes", "ask"), ok()), "offers nothing"),
        (format!("{}\n{}", case("work", "yn", "x", &[], "maybe", "ask"), ok()), "yes or no"),
        (case("work", "intake", "x", &both, "task", "ghost"), "its twin `ghost` is not there"),
        (format!("{}\n{}", case("work", "intake", "x", &both, "task", "ask"), case("ask", "intake", "y", &both, "answer", "elsewhere")), "does not name it back"),
        (format!("{}\n{}", case("work", "intake", "x", &both, "task", "ask"), case("ask", "intake", "y", &both, "task", "work")), "same right answer"),
        (format!("{}\n{}", case("work", "intake", "x", &both, "task", "ask"), case("ask", "intake", "y", &["answer", "flow"], "answer", "work")), "not a case of the same question"),
    ];
    for (cases, says) in faults {
        let (set, problems) = load(&cases);
        assert!(problems.iter().any(|p| p.contains(says)), "{says}: {problems:?}");
        assert!(set.is_empty(), "{says}: the twin of a refused case is refused too, so no case is left: {:?}", set.hashes());
    }
    // a pair next to a refused case still loads
    let (set, problems) = load(&format!("{}\n{}", good_cases(), case("lonely", "intake", "x", &both, "task", "ghost")));
    assert_eq!(set.cases("intake").len(), 2);
    assert!(problems.iter().any(|p| p.contains("probe lonely")), "{problems:?}");
    // a yes/no point takes yes/no cases
    let (set, problems) = load(&format!("{}\n{}", case("a", "yn", "is it red?", &[], "yes", "b"), case("b", "yn", "is it blue?", &[], "no", "a")));
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(set.cases("yn")[0].question().shape, AskShape::YesNo);
}

#[test]
fn a_case_is_put_as_the_question_it_says() {
    let (set, _) = load(&format!(
        "{}\n{}",
        "[[probe]]\nid = \"a\"\npoint = \"intake\"\nslices = { line = \"fix it\", goal = \"the build\", thread = \"new\" }\noffered = [\"answer\", \"task\"]\nexpect = \"task\"\npair = \"b\"\n",
        case("b", "intake", "why?", &["answer", "task"], "answer", "a")
    ));
    let q = set.cases("intake")[0].question();
    assert_eq!((q.text.as_str(), q.goal.as_str(), q.point.as_str(), q.asker.as_str(), q.ask.as_str()), ("fix it\nthread: new", "the build", "intake", "probe", "probe:a"));
    let AskShape::Choice { options } = &q.shape else { panic!() };
    assert_eq!(options.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["answer", "task"]);
    assert_eq!(set.cases("intake")[0].wanted(), Answer::Pick { id: "task".into() });
}

// ---- the prober and the state it sets ----

struct Loop {
    dir: Scratch,
    log: Log,
    clock: Arc<ManualClock>,
    state: Answerers,
    #[allow(dead_code)] // never read: held so the folds stay attached to the log as long as the loop lives
    folds: Folds,
}

impl Loop {
    fn new() -> Loop {
        let dir = Scratch::new("trust");
        let (log, clock) = open_log(&dir);
        let state = Answerers::new();
        let folds = Folds::new(&dir.join("folds"));
        assert!(folds.add(Box::new(state.clone()), 0).is_empty());
        folds.attach(&log).unwrap();
        Loop { dir, log, clock, state, folds }
    }

    fn prober(&self, cases: &str, machines: &[&Arc<Machine>], egress: Option<Arc<dyn Egress>>) -> (Prober, Registry, BTreeMap<String, AskPoint>) {
        let t = tables(cases);
        let points = points_of(&t);
        let (set, problems) = ProbeSet::load(&t, &points);
        assert!(problems.is_empty(), "{problems:?}");
        self.state.require(set.hashes());
        let mut reg = Registry::new();
        for m in machines {
            assert!(reg.register((*m).clone()));
        }
        if let Some(e) = egress {
            reg = reg.with_egress(e);
        }
        (Prober::new(self.log.clone(), reg.clone(), set, points.clone(), self.state.clone()), reg, points)
    }

    fn state_of(&self, a: &Machine, point: &str) -> State {
        self.state.state_for(&a.id, point, &a.model(), self.clock.now_ms())
    }

    fn sync(&self) {
        self.log.sync();
    }
}

fn shadow(s: State) -> String {
    match s {
        State::Shadow { why } => why,
        other => panic!("not shadow: {other:?}"),
    }
}

#[test]
fn an_answerer_that_fails_one_twin_stays_shadow_and_one_that_passes_both_is_active_for_its_model_only() {
    let rig = Loop::new();
    let always = Machine::new("decider:jev", "jev@1", |_| pick("task"));
    let (prober, reg, points) = rig.prober(&good_cases(), &[&always], None);
    // before any probe it has not proved itself: skipped on the live path, as if it were absent
    assert!(shadow(rig.state_of(&always, "intake")).contains("have not passed"));
    let q = Question { ask: "i#1".into(), point: "intake".into(), asker: "front".into(), thread: None, text: "why does it fail?".into(), shape: AskShape::Choice { options: vec![] }, goal: "g".into(), act: None };
    let _ = q;
    assert_eq!(prober.targets(None), [("decider:jev".to_string(), "intake".to_string())]);
    assert_eq!(prober.due(rig.clock.now_ms()).len(), 1);

    // it says "task" to everything: one twin of the pair fails
    let reports = prober.run(&prober.targets(None));
    rig.sync();
    assert_eq!(reports.len(), 1);
    assert!(!reports[0].passed && reports[0].failed == ["ask"], "{reports:?}");
    assert_eq!(always.calls(), 2, "both cases were put");
    assert!(shadow(rig.state_of(&always, "intake")).contains("have not passed"));
    let result = all(&rig.log).into_iter().find(|e| e.kind == "probe.result").unwrap();
    assert_eq!((result.data["answerer"].as_str(), result.data["point"].as_str(), result.data["model"].as_str(), result.data["passed"].as_bool()), (Some("decider:jev"), Some("intake"), Some("jev@1"), Some(false)));
    assert_eq!(result.data["failed"], json!(["ask"]));
    assert!(result.data["cases_hash"].as_str().unwrap().len() == 16 && result.data["ms"].is_u64());

    // the live chain skips it and says so
    let (w, notes) = walk(&points["intake"], &Question { shape: AskShape::Choice { options: ["answer", "task"].iter().map(|id| j4n_core::ask::Opt { id: (*id).into(), label: (*id).into(), recommended: false }).collect() }, ..q.clone() }, &reg, &rig.state, &Ledger::new(), rig.clock.now_ms(), true);
    assert!(matches!(w, Walk::Fallback(_)), "{w:?}");
    assert_eq!(always.calls(), 2, "it was not called on the live path");
    assert!(notes.iter().any(|n| n.data["outcome"] == "skipped" && n.data["why"].as_str().unwrap().starts_with("shadow: ")), "{notes:?}");

    // one that tells the two apart passes, and is active for that model at that point
    let wise = Machine::new("decider:jev", "jev@1", knows);
    let (prober, reg, _) = rig.prober(&good_cases(), &[&wise], None);
    let reports = prober.run(&prober.targets(None));
    rig.sync();
    assert!(reports[0].passed && reports[0].failed.is_empty() && reports[0].why.is_none());
    assert_eq!(rig.state_of(&wise, "intake"), State::Active);
    assert_eq!(rig.state.passed("decider:jev", "intake").map(|p| p.0), Some("jev@1".to_string()));
    assert!(prober.due(rig.clock.now_ms()).is_empty(), "nothing is due right after a pass");
    // a point with no cases needs no pass
    assert_eq!(rig.state.state_for("decider:jev", "yn", "jev@1", rig.clock.now_ms()), State::Active);

    // a pass given by another model does not count for this one
    *wise.model.lock().unwrap() = "jev@2".into();
    assert!(shadow(rig.state_of(&wise, "intake")).contains("passed for jev@1, not for jev@2"));
    assert_eq!(prober.due(rig.clock.now_ms()).len(), 1);
    let _ = reg;

    // edited cases need a new pass
    *wise.model.lock().unwrap() = "jev@1".into();
    assert_eq!(rig.state_of(&wise, "intake"), State::Active);
    let edited = good_cases().replace("fix the failing test", "fix the broken test");
    let (prober2, _, _) = rig.prober(&edited, &[&wise], None);
    assert!(shadow(rig.state_of(&wise, "intake")).contains("cases for intake changed"));
    prober2.run(&prober2.targets(None));
    rig.sync();
    assert_eq!(rig.state_of(&wise, "intake"), State::Active);

    // a failing run takes the pass away again, and a week is long enough to ask again
    let blind = Machine::new("decider:jev", "jev@1", |_| Offer::Pass { why: "no idea".into() });
    let (prober3, _, _) = rig.prober(&edited, &[&blind], None);
    prober3.run(&prober3.targets(None));
    rig.sync();
    assert!(shadow(rig.state_of(&blind, "intake")).contains("have not passed"));
    prober2.run(&prober2.targets(None));
    rig.sync();
    assert!(prober2.due(rig.clock.now_ms()).is_empty());
    rig.clock.advance(STALE_MS - 1);
    assert!(prober2.due(rig.clock.now_ms()).is_empty());
    rig.clock.advance(1);
    assert_eq!(prober2.due(rig.clock.now_ms()).len(), 1, "a pass is a week old");
    let _ = &rig.dir;
}

#[test]
fn what_is_probed_is_a_machine_with_a_model_in_the_chain_of_a_point_with_cases() {
    let rig = Loop::new();
    let jev = Machine::new("decider:jev", "jev@1", knows);
    let other = Machine::new("decider:other", "o@1", knows);
    let datum = Machine::new("decider:jev2", "", knows);
    let (prober, _, _) = rig.prober(&good_cases(), &[&jev, &other, &datum], None);
    // one named in the chain that has no model behind it is data, and has nothing to prove
    let nameless = Machine::new("decider:jev", "", knows);
    let (empty, _, _) = rig.prober(&good_cases(), &[&nameless], None);
    assert!(empty.targets(None).is_empty());
    assert_eq!(prober.targets(None), [("decider:jev".to_string(), "intake".to_string())], "only the one the chain names, and that has a model");
    assert!(prober.targets(Some("decider:other")).is_empty());
    assert_eq!(prober.targets(Some("decider:jev")).len(), 1);
    assert!(prober.set().cases("yn").is_empty());
}

struct Deny(&'static str);

impl Egress for Deny {
    fn check(&self, _: DataClass, zone: &str) -> Verdict {
        match (zone, self.0) {
            ("far", "ask") => Verdict::Ask { risk: 60, why: "far away".into(), by: "t".into() },
            ("far", "block") => Verdict::Block { why: "never".into(), by: "t".into() },
            _ => Verdict::Allow { by: "t".into() },
        }
    }
}

#[test]
fn the_probe_of_an_answerer_goes_through_the_egress_gate_like_a_question() {
    let rig = Loop::new();
    for (mode, says) in [("ask", "needs a yes"), ("block", "is blocked")] {
        let far = Machine::far("decider:jev", "jev@1", knows);
        let (prober, _, _) = rig.prober(&good_cases(), &[&far], Some(Arc::new(Deny(mode))));
        let reports = prober.run(&prober.targets(None));
        rig.sync();
        assert_eq!(far.calls(), 0, "{mode}: nothing was sent");
        assert!(!reports[0].passed && reports[0].why.as_deref().is_some_and(|w| w.contains(says)), "{reports:?}");
        let e = all(&rig.log).into_iter().rfind(|e| e.kind == "probe.result").unwrap();
        assert_eq!(e.data["passed"], false);
        assert!(e.data["why"].as_str().unwrap().contains(says));
    }
    let near = Machine::far("decider:jev", "jev@1", knows);
    let (prober, _, _) = rig.prober(&good_cases(), &[&near], Some(Arc::new(Deny("allow"))));
    assert!(prober.run(&prober.targets(None))[0].passed);
    // and with nothing to ask, an answerer with a zone is not probed at all
    let nothing = Machine::far("decider:jev", "jev@1", knows);
    let (prober, _, _) = rig.prober(&good_cases(), &[&nothing], None);
    assert!(prober.run(&prober.targets(None))[0].why.as_deref().is_some_and(|w| w.contains("nothing checks")));
    assert_eq!(nothing.calls(), 0);
}

// ---- the labels and the rates ----

fn label(rig: &Loop, producer: &str, point: &str, subject: &str, is: &str, weight: f64) {
    rig.log.append(ev("label", json!({ "subject_kind": "answer", "subject": subject, "producer": producer, "model": "jev@1", "point": point, "label": is, "weight": weight }))).unwrap();
    rig.log.sync();
}

#[test]
fn an_answerer_wrong_too_often_at_a_point_is_shadow_there_and_nowhere_else() {
    let rig = Loop::new();
    for i in 0..19 {
        rig.clock.advance(10);
        label(&rig, "decider:jev", "intake", &format!("a{i}"), "overridden", 1.0);
    }
    assert_eq!(rig.state.state_for("decider:jev", "intake", "jev@1", rig.clock.now_ms()), State::Active, "19 labels are too few, however wrong");
    rig.clock.advance(10);
    label(&rig, "decider:jev", "intake", "a19", "overridden", 1.0);
    let why = shadow(rig.state.state_for("decider:jev", "intake", "jev@1", rig.clock.now_ms()));
    assert!(why.contains("100%") && why.contains("20") && why.contains("intake"), "{why}");
    assert_eq!(rig.state.state_for("decider:jev", "scale", "jev@1", rig.clock.now_ms()), State::Active, "another point");
    assert_eq!(rig.state.state_for("decider:other", "intake", "o@1", rig.clock.now_ms()), State::Active, "another answerer");
    assert_eq!(rig.state.state("decider:jev", rig.clock.now_ms()), State::Active, "a data answerer or the breaker is not touched");
    assert_eq!(rig.state.labelled("decider:jev", "intake"), (20, 20.0));
    // a data answerer (no model) has no rate to speak of
    assert_eq!(rig.state.state_for("decider:jev", "intake", "", rig.clock.now_ms()), State::Active);
    // trusting it again for that point is the operator's, and is an event
    rig.log.append(ev("answerer.active", json!({ "answerer": "decider:jev", "point": "intake" }))).unwrap();
    rig.log.sync();
    assert_eq!(rig.state.state_for("decider:jev", "intake", "jev@1", rig.clock.now_ms()), State::Active);
    let read = j4n_core::fold::Fold::read(&rig.state, &[j4n_core::value::Key::Name("decider:jev".into()), j4n_core::value::Key::Name("rate".into())], 0);
    assert_eq!(read, j4n_core::value::Value::Map(Default::default()));
}

#[test]
fn the_rate_is_the_weight_of_the_wrong_over_the_labels_and_a_quarter_is_the_limit() {
    let rig = Loop::new();
    // 20 labels, 5 of them wrong: exactly 25% is not over it
    for i in 0..20 {
        rig.clock.advance(10);
        label(&rig, "decider:jev", "intake", &format!("a{i}"), if i < 5 { "overridden" } else { "held" }, if i < 5 { 1.0 } else { 0.0 });
    }
    assert_eq!(rig.state.state_for("decider:jev", "intake", "m", rig.clock.now_ms()), State::Active);
    rig.clock.advance(10);
    label(&rig, "decider:jev", "intake", "a20", "overridden", 1.0);
    assert!(matches!(rig.state.state_for("decider:jev", "intake", "m", rig.clock.now_ms()), State::Shadow { .. }), "6 of 21");
    // weak labels weigh half: 10 failed_after of 20 is 25%, 11 is over
    let weak = Loop::new();
    for i in 0..20 {
        weak.clock.advance(10);
        label(&weak, "model:light", "pick", &format!("w{i}"), "failed_after", if i < 10 { 0.5 } else { 0.0 });
    }
    assert_eq!(weak.state.state_for("model:light", "pick", "m", weak.clock.now_ms()), State::Active);
    weak.clock.advance(10);
    label(&weak, "model:light", "pick", "w20", "failed_after", 0.5);
    assert!(matches!(weak.state.state_for("model:light", "pick", "m", weak.clock.now_ms()), State::Shadow { .. }));
}

#[test]
fn an_answer_is_counted_once_and_later_evidence_only_makes_it_worse() {
    let rig = Loop::new();
    label(&rig, "decider:jev", "intake", "a1", "held", 0.0);
    assert_eq!(rig.state.labelled("decider:jev", "intake"), (1, 0.0));
    label(&rig, "decider:jev", "intake", "a1", "failed_after", 0.5);
    assert_eq!(rig.state.labelled("decider:jev", "intake"), (1, 0.5));
    label(&rig, "decider:jev", "intake", "a1", "held", 0.0);
    assert_eq!(rig.state.labelled("decider:jev", "intake"), (1, 0.5), "a later good label does not take a bad one back");
    label(&rig, "decider:jev", "intake", "a1", "undone", 1.0);
    assert_eq!(rig.state.labelled("decider:jev", "intake"), (1, 1.0));
    label(&rig, "decider:jev", "intake", "a2", "held", 0.0);
    assert_eq!(rig.state.labelled("decider:jev", "intake"), (2, 1.0));
    // what is not about an answer is not counted here
    rig.log.append(ev("label", json!({ "subject_kind": "qc_mark", "subject": "m1", "producer": "decider:jev", "point": "intake", "label": "dismissed", "weight": 1.0 }))).unwrap();
    rig.log.sync();
    assert_eq!(rig.state.labelled("decider:jev", "intake"), (2, 1.0));
}

#[test]
fn only_the_last_labels_of_the_last_thirty_days_are_counted() {
    let rig = Loop::new();
    for i in 0..LABELS_KEPT + 30 {
        rig.clock.advance(1_000);
        label(&rig, "decider:jev", "intake", &format!("a{i}"), "held", 0.0);
    }
    assert_eq!(rig.state.labelled("decider:jev", "intake").0, LABELS_KEPT);
    // thirty days later the old ones are not counted
    rig.clock.advance(LABELS_DAYS * 24 * 3600 * 1000 + 1_000);
    label(&rig, "decider:jev", "intake", "late", "overridden", 1.0);
    assert_eq!(rig.state.labelled("decider:jev", "intake"), (1, 1.0));
}

#[test]
fn the_limits_are_the_settings_and_the_state_survives_a_restart_and_a_replay() {
    let dir = Scratch::new("limits");
    let (log, clock) = open_log(&dir);
    let strict = Answerers::new().with_limits(Limits { max_wrong: 0.1, min_n: 3 });
    let folds = Folds::new(&dir.join("folds"));
    assert!(folds.add(Box::new(strict.clone()), 0).is_empty());
    folds.attach(&log).unwrap();
    let mut seen = 0;
    for (i, w) in [0.0, 1.0, 0.0].iter().enumerate() {
        clock.advance(10);
        log.append(ev("label", json!({ "subject_kind": "answer", "subject": format!("s{i}"), "producer": "decider:jev", "point": "p", "label": "x", "weight": w }))).unwrap();
        seen += 1;
    }
    log.append(ev("probe.result", json!({ "answerer": "decider:jev", "point": "p", "model": "m", "cases_hash": "h", "passed": true }))).unwrap();
    log.sync();
    assert_eq!(seen, 3);
    assert!(matches!(strict.state_for("decider:jev", "p", "m", clock.now_ms()), State::Shadow { .. }), "a third wrong with 3 labels, at a limit of a tenth");
    // a second host of the fold, started from a snapshot and from nothing, comes to the same
    folds.snapshot();
    let (a, b) = (Answerers::new().with_limits(Limits { max_wrong: 0.1, min_n: 3 }), Answerers::new().with_limits(Limits { max_wrong: 0.1, min_n: 3 }));
    let from_snapshot = Folds::new(&dir.join("folds"));
    assert!(from_snapshot.add(Box::new(a.clone()), log.head()).is_empty());
    let full = Folds::new(&dir.join("folds-full"));
    assert!(full.add(Box::new(b.clone()), 0).is_empty());
    full.attach(&log).unwrap();
    assert_eq!(j4n_core::fold::Fold::save(&a), j4n_core::fold::Fold::save(&strict));
    assert_eq!(j4n_core::fold::Fold::save(&b), j4n_core::fold::Fold::save(&strict));
    assert_eq!(a.passed("decider:jev", "p").map(|p| p.1), Some("h".to_string()));
    assert!(matches!(a.state_for("decider:jev", "p", "m", clock.now_ms()), State::Shadow { .. }), "the shadow by rate survives a restart");
    let _ = all(&log);
}

#[test]
fn trusting_an_answerer_again_for_one_point_leaves_the_others_shadowed_and_a_weight_is_at_most_one() {
    let rig = Loop::new();
    for point in ["intake", "scale"] {
        for i in 0..20 {
            rig.clock.advance(10);
            label(&rig, "decider:jev", point, &format!("{point}{i}"), "overridden", 5.0);
        }
    }
    assert_eq!(rig.state.labelled("decider:jev", "intake"), (20, 20.0), "a weight over one counts as one");
    for point in ["intake", "scale"] {
        assert!(matches!(rig.state.state_for("decider:jev", point, "m", rig.clock.now_ms()), State::Shadow { .. }), "{point}");
    }
    rig.log.append(ev("answerer.active", json!({ "answerer": "decider:jev", "point": "intake" }))).unwrap();
    rig.log.sync();
    assert_eq!(rig.state.state_for("decider:jev", "intake", "m", rig.clock.now_ms()), State::Active);
    assert!(matches!(rig.state.state_for("decider:jev", "scale", "m", rig.clock.now_ms()), State::Shadow { .. }));
    rig.log.append(ev("answerer.active", json!({ "answerer": "decider:jev" }))).unwrap();
    rig.log.sync();
    assert_eq!(rig.state.state_for("decider:jev", "scale", "m", rig.clock.now_ms()), State::Active, "without a point it is for all");
}

// ---- what a failed pass says, and a line that waits for the cases ----

/// An answerer that records the labels of the options it is put, and answers by `knows`.
struct Reader {
    labels: Mutex<Vec<Vec<String>>>,
    pause: std::time::Duration,
    right: bool,
}

impl Answerer for Reader {
    fn id(&self) -> &str {
        "decider:jev"
    }
    fn kind(&self) -> Kind {
        Kind::Decider
    }
    fn model(&self) -> String {
        "jev@1".into()
    }
    fn offer(&self, q: &Question) -> Offer {
        std::thread::sleep(self.pause);
        if let AskShape::Choice { options } = &q.shape {
            self.labels.lock().unwrap().push(options.iter().map(|o| o.label.clone()).collect());
        }
        if self.right { knows(&q.text) } else { Offer::Answer { answer: Answer::Pick { id: "task".into() }, conf: 0.81, why: "x".into() } }
    }
}

fn reader(pause_ms: u64, right: bool) -> Arc<Reader> {
    Arc::new(Reader { labels: Mutex::default(), pause: std::time::Duration::from_millis(pause_ms), right })
}

#[test]
fn a_failed_pass_names_each_missed_case_with_the_answer_given_and_how_sure_it_was() {
    let rig = Loop::new();
    let wrong = reader(0, false);
    let (mut reg, t) = (Registry::new(), tables(&good_cases()));
    assert!(reg.register(wrong.clone()));
    let points = points_of(&t);
    let (set, _) = ProbeSet::load(&t, &points);
    let prober = Prober::new(rig.log.clone(), reg, set, points, rig.state.clone());
    let reports = prober.run(&prober.targets(None));
    rig.sync();
    assert_eq!(reports[0].failed, ["ask"]);
    assert_eq!(reports[0].missed.len(), 1);
    assert_eq!(reports[0].missed[0].line(), "ask: wanted answer, got task (0.81)");
    let result = all(&rig.log).into_iter().find(|e| e.kind == "probe.result").unwrap();
    assert_eq!(result.data["missed"], json!([{ "case": "ask", "wanted": "answer", "got": "task", "conf": 0.8100000023841858 }]));
    // an answerer that gave none, or failed, says that instead
    let blind = Machine::new("decider:jev", "jev@1", |_| Offer::Failed { why: "HTTP 500:\nboom".into() });
    let (p2, _, _) = rig.prober(&good_cases(), &[&blind], None);
    let r = p2.run(&p2.targets(None));
    assert!(r[0].missed.iter().all(|m| m.got == "failed: HTTP 500: boom" && m.conf.is_none()), "{:?}", r[0].missed);
    assert_eq!(r[0].missed.len(), 2);
}

#[test]
fn a_case_is_put_with_the_words_its_point_says_each_option_means() {
    let rig = Loop::new();
    let seen = reader(0, true);
    let (mut reg, t) = (Registry::new(), tables(&good_cases()));
    assert!(reg.register(seen.clone()));
    let points = points_of(&t);
    let (set, _) = ProbeSet::load(&t, &points);
    let says: j4n_core::answer::Says = BTreeMap::from([("intake".to_string(), BTreeMap::from([("task".to_string(), "one piece of work".to_string())]))]);
    let prober = Prober::new(rig.log.clone(), reg, set, points, rig.state.clone()).with_says(says);
    let r = prober.run(&prober.targets(None));
    assert!(r[0].passed);
    // an option the point says nothing of keeps its id for its words
    assert!(seen.labels.lock().unwrap().iter().all(|l| l == &["answer".to_string(), "one piece of work".to_string()]), "{:?}", seen.labels.lock().unwrap());
}

#[test]
fn a_line_waits_for_the_cases_of_its_point_within_the_bound_and_a_pair_is_tried_once() {
    let rig = Loop::new();
    let slow = reader(1500, true);
    let (mut reg, t) = (Registry::new(), tables(&good_cases()));
    assert!(reg.register(slow.clone()));
    let points = points_of(&t);
    let (set, _) = ProbeSet::load(&t, &points);
    rig.state.require(set.hashes());
    let prober = Arc::new(Prober::new(rig.log.clone(), reg, set, points, rig.state.clone()));
    let now = rig.clock.now_ms();
    let state = || rig.state.state_for("decider:jev", "intake", "jev@1", now);
    assert!(matches!(state(), State::Shadow { .. }));
    // a point with no cases, or none in whose chain a machine is, has nothing to wait for
    assert_eq!(prober.wait_for("yn", std::time::Duration::from_secs(5), now), None);
    // a bound that is shorter than the cases: the line goes on, the cases go on
    let began = std::time::Instant::now();
    let waited = prober.wait_for("intake", std::time::Duration::from_millis(30), now).expect("it waited");
    assert!(waited < std::time::Duration::from_millis(1000) && began.elapsed() < std::time::Duration::from_millis(1000), "{waited:?}");
    assert!(matches!(state(), State::Shadow { .. }), "not written yet");
    // another line meanwhile waits for the run that is in hand, and does not begin another: two cases, once
    let waited = prober.wait_for("intake", std::time::Duration::from_secs(10), now).expect("it waited");
    assert!(waited < std::time::Duration::from_secs(9), "{waited:?}");
    assert_eq!(state(), State::Active, "the pass was in the Log when the wait ended");
    assert_eq!(slow.labels.lock().unwrap().len(), 2, "each case was put once");
    // and nothing is due now
    assert_eq!(prober.wait_for("intake", std::time::Duration::from_secs(10), now), None);
}

#[test]
fn a_pair_that_failed_is_not_run_again_by_the_next_line() {
    let rig = Loop::new();
    let wrong = reader(0, false);
    let (mut reg, t) = (Registry::new(), tables(&good_cases()));
    assert!(reg.register(wrong.clone()));
    let points = points_of(&t);
    let (set, _) = ProbeSet::load(&t, &points);
    rig.state.require(set.hashes());
    let prober = Arc::new(Prober::new(rig.log.clone(), reg, set, points, rig.state.clone()));
    let now = rig.clock.now_ms();
    assert!(prober.wait_for("intake", std::time::Duration::from_secs(10), now).is_some());
    assert!(matches!(rig.state.state_for("decider:jev", "intake", "jev@1", now), State::Shadow { .. }), "it failed its cases");
    assert_eq!(prober.wait_for("intake", std::time::Duration::from_secs(10), now), None, "tried in this process");
    assert_eq!(wrong.labels.lock().unwrap().len(), 2);
}

// ---- the machine's record of passes ----

/// A project of its own (a Log, a fold) in the user folder `home`, with a prober that keeps and reads its record.
fn project(home: &Scratch, cases: &str, machine: &Arc<Machine>) -> (Loop, Prober) {
    let rig = Loop::new();
    let (prober, _, _) = rig.prober(cases, &[machine], None);
    (rig, prober.with_record(Record::at(home.path())))
}

fn from_record(rig: &Loop) -> Vec<j4n_core::log::Ev> {
    all(&rig.log).into_iter().filter(|e| e.kind == "probe.result" && e.data["from"] == "record").collect()
}

#[test]
fn a_second_project_in_the_same_user_folder_starts_with_the_pass_and_puts_no_case() {
    let home = Scratch::new("home");
    let wise = Machine::new("decider:jev", "jev@1", knows);
    let (a, pa) = project(&home, &good_cases(), &wise);
    assert_eq!(pa.due(a.clock.now_ms()).len(), 1, "a project with no pass and an empty record probes");
    assert!(pa.run(&pa.targets(None))[0].passed);
    a.sync();
    assert_eq!(wise.calls(), 2);
    // the record holds the key and the time, no case text and no answers
    let kept = std::fs::read_to_string(home.join("cache/probe-passes.json")).unwrap();
    assert!(kept.contains("jev@1") && kept.contains("decider:jev") && !kept.contains("failing test") && !kept.contains("wanted"), "{kept}");

    a.clock.advance(1_000);
    let fresh = Machine::new("decider:jev", "jev@1", knows);
    let (b, pb) = project(&home, &good_cases(), &fresh);
    assert!(shadow(b.state_of(&fresh, "intake")).contains("have not passed"), "its own Log has no pass yet");
    assert!(pb.due(b.clock.now_ms()).is_empty(), "the record has it");
    b.sync();
    assert_eq!(fresh.calls(), 0, "no case was put");
    assert_eq!(b.state_of(&fresh, "intake"), State::Active);
    let seeded = from_record(&b);
    assert_eq!(seeded.len(), 1, "the Log says why it may act");
    assert_eq!((seeded[0].data["passed"].as_bool(), seeded[0].data["passed_at"].as_u64()), (Some(true), Some(1_000)));
    assert_eq!(b.state.passed("decider:jev", "intake").map(|p| p.2), Some(1_000), "as old as the pass, not as new as the seeding");
    assert!(pb.due(b.clock.now_ms()).is_empty() && from_record(&b).len() == 1, "seeded once");
}

#[test]
fn changed_cases_another_model_and_an_old_pass_are_probed_again() {
    let home = Scratch::new("home");
    let wise = Machine::new("decider:jev", "jev@1", knows);
    let (a, pa) = project(&home, &good_cases(), &wise);
    pa.run(&pa.targets(None));
    a.sync();

    let edited = good_cases().replace("fix the failing test", "fix the broken test");
    let (b, pb) = project(&home, &edited, &wise);
    assert_eq!(pb.due(b.clock.now_ms()).len(), 1, "other cases");
    assert!(from_record(&b).is_empty());

    let other = Machine::new("decider:jev", "jev@2", knows);
    let (c, pc) = project(&home, &good_cases(), &other);
    assert_eq!(pc.due(c.clock.now_ms()).len(), 1, "another model");
    assert!(from_record(&c).is_empty());

    let (d, pd) = project(&home, &good_cases(), &wise);
    d.clock.advance(STALE_MS - 1_001);
    assert!(pd.due(d.clock.now_ms()).is_empty(), "a moment short of the week");
    let (e, pe) = project(&home, &good_cases(), &wise);
    e.clock.advance(STALE_MS);
    assert_eq!(pe.due(e.clock.now_ms()).len(), 1, "a week old");
    assert!(from_record(&e).is_empty());
}

#[test]
fn a_failure_takes_the_entry_away_and_a_refused_zone_does_not() {
    let home = Scratch::new("home");
    let wise = Machine::new("decider:jev", "jev@1", knows);
    let (a, pa) = project(&home, &good_cases(), &wise);
    pa.run(&pa.targets(None));
    a.sync();

    // the same model gets it wrong later (in a project that was seeded): the entry is gone for every project
    let blind = Machine::new("decider:jev", "jev@1", |_| Offer::Pass { why: "no idea".into() });
    let (b, pb) = project(&home, &good_cases(), &blind);
    assert!(pb.due(b.clock.now_ms()).is_empty());
    assert!(!pb.run(&pb.targets(None))[0].passed);
    b.sync();
    let (c, pc) = project(&home, &good_cases(), &wise);
    assert_eq!(pc.due(c.clock.now_ms()).len(), 1, "no entry any more");
    assert!(from_record(&c).is_empty());

    // cases that were never put prove nothing either way
    pc.run(&pc.targets(None));
    c.sync();
    let far = Machine::far("decider:jev", "jev@1", knows);
    let (d, pd) = project(&home, &good_cases(), &far);
    assert!(pd.run(&pd.targets(None))[0].why.is_some());
    d.sync();
    assert_eq!(far.calls(), 0);
    let (e, pe) = project(&home, &good_cases(), &wise);
    assert!(pe.due(e.clock.now_ms()).is_empty(), "the pass of c is still there");
}
