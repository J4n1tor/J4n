//! Spot checks: after a machine's answer took effect, the next machine answerer of the point is asked the same
//! question off the path, the two are compared, and the live answer is never touched.

use crate::scripted::{Rig, load_base};
use crate::world::{Scratch, layer, load, open_log};
use j4n_core::answer::chain::Registry;
use j4n_core::answer::point::points;
use j4n_core::answer::state::Answerers;
use j4n_core::answer::{Answerer, Kind, Offer, Question};
use j4n_core::ask::{Answer, AskShape, Opt};
use j4n_core::change::Actor;
use j4n_core::log::New;
use j4n_core::table::LayerKind;
use j4n_core::task::spot::Spot;
use j4n_core::task::{Asks, State};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct Machine {
    id: String,
    kind: Kind,
    says: &'static str,
    zone: Option<&'static str>,
    calls: AtomicUsize,
}

impl Machine {
    fn new(id: &str, kind: Kind, says: &'static str) -> Arc<Machine> {
        Arc::new(Machine { id: id.into(), kind, says, zone: None, calls: AtomicUsize::new(0) })
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

    fn offer(&self, _: &Question) -> Offer {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Offer::Answer { answer: Answer::Pick { id: self.says.into() }, conf: 0.9, why: format!("{} thinks {}", self.id, self.says) }
    }

    fn model(&self) -> String {
        "m@1".into()
    }

    fn zone(&self) -> Option<String> {
        self.zone.map(String::from)
    }
}

struct Closed;

impl j4n_core::answer::Egress for Closed {
    fn check(&self, _: j4n_core::schema::DataClass, _: &str) -> j4n_core::gate::Verdict {
        j4n_core::gate::Verdict::Block { why: "data stays here".into(), by: "t".into() }
    }
}

const POINTS: &str = r#"
[[ask_point]]
id = "pick"
doc = "mid"
shape = "choice"
impact = "mid"
data = "project"
chain = ["decider:one", "decider:two", "human"]
fallback = "pick:a"

[[ask_point]]
id = "ranked"
doc = "a rank stands between the two deciders"
shape = "choice"
impact = "mid"
data = "project"
chain = ["decider:one", "rank", "decider:two", "human"]
fallback = "pick:a"

[[ask_point]]
id = "cheap"
doc = "low"
shape = "choice"
impact = "low"
data = "project"
chain = ["decider:one", "decider:two", "human"]
fallback = "pick:a"
"#;

struct World {
    rig: Rig,
    asks: Arc<Asks>,
    one: Arc<Machine>,
    two: Arc<Machine>,
}

fn world(second: &'static str) -> World {
    build(Machine::new("decider:two", Kind::Decider, second), None, &[])
}

/// `extra` are answerers registered besides the two deciders; `closed` has the egress Gate refuse every zone.
fn build(two: Arc<Machine>, closed: Option<()>, extra: &[Arc<dyn Answerer>]) -> World {
    let (tables, problems) = load(vec![layer(LayerKind::Pack, "t", &[("p.toml", &format!("api = 1\n{POINTS}"))])]);
    assert!(problems.is_empty(), "{problems:?}");
    let (pts, problems) = points(&tables);
    assert!(problems.is_empty(), "{problems:?}");
    let one = Machine::new("decider:one", Kind::Decider, "a");
    let mut reg = Registry::new();
    reg.register(one.clone());
    reg.register(two.clone());
    for a in extra {
        reg.register(a.clone());
    }
    if closed.is_some() {
        reg = reg.with_egress(Arc::new(Closed));
    }
    let state = Answerers::new();
    let asks = Arc::new(Asks::new(pts, reg, state.clone()));
    let (project, home) = (Scratch::new("proj"), Scratch::new("home"));
    let (log, clock) = open_log(&project);
    let spot = Arc::new(Spot::new(asks.clone(), log.clone()));
    let rig = Rig::over(project, home, log.clone(), clock, vec![], |c| {
        // the spot-check rule and the rules that label what it finds, as the base pack has them
        let trust = include_str!("../../../../packs/base/rules/trust.toml").replacen("api = 1", "", 1);
        load_base(c, &trust);
        assert!(c.folds.add(Box::new(state), c.log.head()).is_empty());
        c.doers.push(spot);
    });
    World { rig, asks, one, two }
}

/// What the front writes when a machine answered a question: the question is remembered and the answer is an event.
fn answered(w: &World, ask: &str, point: &str, impact: &str, by: &str, kind: &str, via: &str) {
    let q = Question {
        ask: ask.into(),
        point: point.into(),
        asker: "front".into(),
        thread: Some("main".into()),
        text: "which one?".into(),
        shape: AskShape::Choice { options: ["a", "b"].iter().map(|id| Opt { id: id.to_string(), label: id.to_string(), recommended: false }).collect() },
        goal: "which one?".into(),
        act: None,
    };
    w.asks.remember(&q);
    let data = json!({ "ask": ask, "point": point, "impact": impact, "by": by, "kind": kind, "answer": "a", "given": { "answer": "pick", "id": "a" }, "conf": 0.9, "why": "w", "via": via });
    w.rig.log.append(New::of("ask.answered", Actor::Model(by.into()), data).thread("main")).unwrap();
    w.rig.log.sync();
}

fn spot_tasks(w: &World) -> Vec<j4n_core::task::Row> {
    w.rig.tasks.book().rows().into_iter().filter(|r| r.spec.doer == "spot").collect()
}

fn wait_spot(w: &World, n: usize) {
    for _ in 0..4000 {
        let rows = spot_tasks(w);
        if rows.len() >= n && rows.iter().all(|r| r.state.is_terminal()) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("the spot check never ended\n{}", w.rig.dump());
}

#[test]
fn a_question_of_mid_impact_is_put_again_to_another_machine_and_the_two_answers_are_labelled() {
    let w = world("b");
    answered(&w, "pick#1", "pick", "mid", "decider:one", "decider", "chain");
    wait_spot(&w, 1);
    let spot = w.rig.of_kind("ask.spot");
    assert_eq!(spot.len(), 1, "{}", w.rig.dump());
    let d = &spot[0].data;
    assert_eq!((d["ask"].as_str(), d["first_by"].as_str(), d["second_by"].as_str(), d["agreed"].as_bool()), (Some("pick#1"), Some("decider:one"), Some("decider:two"), Some(false)));
    assert_eq!((d["first"].as_str(), d["second"].as_str(), d["second_model"].as_str()), (Some("a"), Some("b"), Some("m@1")));
    assert_eq!(w.two.calls(), 1);
    assert_eq!(w.one.calls(), 0, "the live answerer is not asked again");
    // both are labelled, with another subject for the checker, and neither label is a verdict
    let labels: Vec<_> = w.rig.of_kind("label").into_iter().map(|e| e.data).collect();
    let find = |producer: &str| labels.iter().find(|l| l["producer"] == producer).unwrap_or_else(|| panic!("{labels:?}"));
    assert_eq!((find("decider:one")["label"].as_str(), find("decider:one")["subject"].as_str(), find("decider:one")["weight"].as_f64()), (Some("disagreed"), Some("pick#1"), Some(0.5)));
    assert_eq!((find("decider:two")["label"].as_str(), find("decider:two")["subject"].as_str()), (Some("disagreed"), Some("pick#1#spot")));
    // the live answer stood before it and stands after it: one answer event, and the question is not settled again
    assert_eq!(w.rig.of_kind("ask.answered").len(), 1);
    assert!(matches!(spot_tasks(&w)[0].state, State::Done { ok: true }));
}

#[test]
fn two_machines_that_agree_are_labelled_agreed_and_weigh_nothing() {
    let w = world("a");
    answered(&w, "pick#2", "pick", "mid", "decider:one", "decider", "chain");
    wait_spot(&w, 1);
    assert_eq!(w.rig.of_kind("ask.spot")[0].data["agreed"], true);
    let labels: Vec<_> = w.rig.of_kind("label").into_iter().map(|e| e.data).collect();
    assert_eq!(labels.len(), 2);
    assert!(labels.iter().all(|l| l["label"] == "agreed" && l["weight"] == 0.0), "{labels:?}");
}

#[test]
fn only_a_machine_answer_to_a_question_of_mid_impact_is_checked() {
    let w = world("b");
    answered(&w, "cheap#1", "cheap", "low", "decider:one", "decider", "chain");
    answered(&w, "pick#3", "pick", "mid", "rule:intake", "rule", "chain");
    answered(&w, "pick#4", "pick", "mid", "decider:one", "decider", "fallback");
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(spot_tasks(&w).is_empty(), "{}", w.rig.dump());
    assert_eq!(w.two.calls(), 0);
}

#[test]
fn at_most_five_an_hour() {
    let w = world("b");
    for n in 0..8 {
        answered(&w, &format!("pick#{n}"), "pick", "mid", "decider:one", "decider", "chain");
    }
    wait_spot(&w, 5);
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(spot_tasks(&w).len(), 5, "{}", w.rig.dump());
    assert_eq!(w.two.calls(), 5);
}

#[test]
fn a_check_that_cannot_be_put_ends_well_and_finds_nothing() {
    let w = world("b");
    // the question is forgotten
    w.rig.log.append(New::of("ask.answered", Actor::Model("decider:one".into()), json!({ "ask": "gone#1", "point": "pick", "impact": "mid", "by": "decider:one", "kind": "decider", "answer": "a", "given": { "answer": "pick", "id": "a" }, "via": "chain" }))).unwrap();
    wait_spot(&w, 1);
    assert!(w.rig.of_kind("ask.spot").is_empty());
    let row = &spot_tasks(&w)[0];
    assert!(matches!(row.state, State::Done { ok: true }) && row.answer.as_deref().unwrap().contains("forgotten"), "{row:?}");
    // the other machine checks whichever one gave the live answer
    let other = world("b");
    answered(&other, "pick#9", "pick", "mid", "decider:two", "decider", "chain");
    wait_spot(&other, 1);
    assert_eq!(other.rig.of_kind("ask.spot")[0].data["second_by"], "decider:one");
}

#[test]
fn a_question_of_low_impact_is_not_checked_even_when_the_event_says_mid() {
    let w = world("b");
    // the rule reads the event, the doer reads the point: both must say mid
    answered(&w, "cheap#7", "cheap", "mid", "decider:one", "decider", "chain");
    wait_spot(&w, 1);
    assert!(w.rig.of_kind("ask.spot").is_empty() && w.two.calls() == 0);
    assert!(spot_tasks(&w)[0].answer.as_deref().unwrap().contains("low impact"));
}

#[test]
fn an_answerer_whose_breaker_is_open_is_not_asked_to_check() {
    let w = world("b");
    for _ in 0..3 {
        w.rig.log.append(New::of("ask.step", Actor::Core, json!({ "answerer": "decider:two", "ask": "x", "outcome": "failed" }))).unwrap();
    }
    w.rig.log.sync();
    answered(&w, "pick#5", "pick", "mid", "decider:one", "decider", "chain");
    wait_spot(&w, 1);
    assert_eq!(w.two.calls(), 0);
    assert!(w.rig.of_kind("ask.spot").is_empty());
}

#[test]
fn an_answerer_whose_zone_the_egress_gate_refuses_is_not_asked_to_check() {
    let mut two = Machine::new("decider:two", Kind::Decider, "b");
    Arc::get_mut(&mut two).unwrap().zone = Some("far");
    let w = build(two, Some(()), &[]);
    answered(&w, "pick#6", "pick", "mid", "decider:one", "decider", "chain");
    wait_spot(&w, 1);
    assert_eq!(w.two.calls(), 0, "the question stays where it is");
    assert!(w.rig.of_kind("ask.spot").is_empty());
}

#[test]
fn only_a_decider_or_a_model_checks_not_a_rank_that_stands_in_the_chain() {
    let rank = Machine::new("rank", Kind::Rank, "b");
    let w = build(Machine::new("decider:two", Kind::Decider, "b"), None, &[rank.clone() as Arc<dyn Answerer>]);
    answered(&w, "ranked#1", "ranked", "mid", "decider:one", "decider", "chain");
    wait_spot(&w, 1);
    assert_eq!(rank.calls(), 0, "a rank is data, and is no second opinion");
    assert_eq!(w.rig.of_kind("ask.spot")[0].data["second_by"], "decider:two");
}

#[test]
fn the_asks_remember_a_question_once_and_only_the_last_sixty_four() {
    let w = world("b");
    let q = |ask: &str, text: &str| Question { ask: ask.into(), point: "pick".into(), asker: "t".into(), thread: None, text: text.into(), shape: AskShape::YesNo, goal: String::new(), act: None };
    w.asks.remember(&q("a#1", "first"));
    w.asks.remember(&q("a#1", "again"));
    assert_eq!(w.asks.recall("a#1").unwrap().text, "again", "one entry, the newer");
    for n in 2..=70 {
        w.asks.remember(&q(&format!("a#{n}"), "x"));
    }
    assert!(w.asks.recall("a#1").is_none() && w.asks.recall("a#6").is_none(), "forgotten");
    assert!(w.asks.recall("a#7").is_some() && w.asks.recall("a#70").is_some(), "the last sixty-four");
}
