//! Answer trust through the loop: the rules of the base pack that label a machine's answer from what happened next,
//! the gate pair, the rule action `label`, and the host a thread is bound to.

use crate::front_cases::{Fx, base, fx, fx_trusting};
use crate::scripted::{Beat, Rig, Scripted, choice, end, load_base, say};
use crate::world::{layer, load, mentions, refused};
use j4n_core::answer::chain::Registry;
use j4n_core::answer::point::{parse_step, points};
use j4n_core::answer::probe::ProbeSet;
use j4n_core::answer::state::Answerers;
use j4n_core::answer::{Answerer, AskPoint, Fallback, Impact, Kind, Offer, Question, ShapeKind};
use j4n_core::ask::{Answer, AskSpec};
use j4n_core::front::hosts::{Hosts, Pick, Want};
use j4n_core::front::{Cfg, Said};
use j4n_core::rule::Rules;
use j4n_core::schema::DataClass;
use j4n_core::setting::Settings;
use j4n_core::table::LayerKind;
use j4n_core::task::{Asks, Ctl, State as TaskState};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

fn chatty(id: &str, answers: &[&str]) -> Arc<Scripted> {
    let turns = answers.iter().map(|a| vec![Beat::Session(format!("{id}-session")), say(a), end(a)]).collect();
    Scripted::new(id, turns)
}

fn labels(f: &Fx) -> Vec<Value> {
    f.rig.of_kind("label").into_iter().map(|e| e.data).collect()
}

#[test]
fn the_base_pack_loads_its_trust_rules_probes_and_settings_clean() {
    let t = base();
    let (rules, problems) = Rules::load(&t);
    assert!(problems.is_empty(), "{problems:?}");
    for id in ["label-overridden-switch", "label-overridden-stop", "label-failed-after", "label-failed-after-stop", "label-held", "label-undone", "label-agreed-first", "label-agreed-second", "label-disagreed-first", "label-disagreed-second"] {
        assert!(rules.ids().contains(&id), "{id}");
    }
    let (pts, problems) = points(&t);
    assert!(problems.is_empty(), "{problems:?}");
    let by: std::collections::BTreeMap<String, AskPoint> = pts.into_iter().map(|p| (p.id.clone(), p)).collect();
    let (set, problems) = ProbeSet::load(&t, &by);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(set.hashes().keys().cloned().collect::<Vec<_>>(), ["access", "chats", "complete", "fold", "inside", "intake", "librarian", "read:asked", "read:chat", "read:goes_on", "read:kind", "reflex", "retry", "weigh", "who:work"]);
    assert!(set.cases("intake").len() >= 4 && set.cases("retry").len() == 2 && set.cases("access").len() == 4 && set.cases("inside").len() == 4);
    let (settings, problems) = Settings::load(&t);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(settings.num("trust.max_wrong", 0.0), 0.25);
    assert_eq!(settings.num("trust.min_n", 0.0), 20.0);
    assert_eq!(settings.num("route.cooling", 0.0), 3.0);
    assert_eq!(settings.word("route.failover", ""), "wait");
    assert!(matches!(settings.get("route.hosts"), Some(j4n_core::value::Value::Bool(false))));
}

// ---- the rule action `label` ----

fn rule_text(fields: &str) -> String {
    format!("api = 1\n[[rule]]\nid = \"r\"\non = \"task.settled\"\nthen = \"label\"\n{fields}\n")
}

fn compile(fields: &str) -> Vec<String> {
    let (t, problems) = load(vec![layer(LayerKind::Pack, "t", &[("a.toml", &rule_text(fields))])]);
    let mut said = refused(&problems);
    let (_, more) = Rules::load(&t);
    said.extend(more.iter().map(ToString::to_string));
    said
}

#[test]
fn a_label_rule_says_what_it_labels_and_cannot_read_a_models_words_bare() {
    let ok = "label = { is = \"held\", subject = \"{task}\", producer = \"decider:jev\", weight = 0.5 }";
    assert!(compile(ok).is_empty(), "{:?}", compile(ok));
    for (fields, says) in [
        ("", "needs `label"),
        ("label = { is = \"held\", subject = \"{task}\", producer = \"p\", weight = 1.5 }", "weight"),
        ("label = { is = \"held\", of = \"thing\", subject = \"{task}\", producer = \"p\" }", "not one of answer, shape_item, qc_mark"),
        ("label = { is = \"two words\", subject = \"{task}\", producer = \"p\" }", "a word of at most 32"),
        ("label = { is = \"held\", subject = \"{ev.data.answer}\", producer = \"p\" }", "bare"),
        ("label = { is = \"held\", subject = \"{task}\", producer = \"{ev.data.text}\" }", "bare"),
        ("label = { is = \"held\", subject = \"{nonsense(}\", producer = \"p\" }", "label.subject"),
        ("text = \"x\"\nlabel = { is = \"held\", subject = \"{task}\", producer = \"p\" }", "`text` is for"),
    ] {
        let said = compile(fields);
        assert!(mentions(&said, &[says]), "{fields}: wanted `{says}` in {said:?}");
    }
    let wrong_then = "api = 1\n[[rule]]\nid = \"r\"\non = \"task.settled\"\nthen = \"notice\"\ntext = \"x\"\nlabel = { is = \"held\", subject = \"s\", producer = \"p\" }\n";
    let (t, _) = load(vec![layer(LayerKind::Pack, "t", &[("a.toml", wrong_then)])]);
    assert!(mentions(&Rules::load(&t).1.iter().map(ToString::to_string).collect::<Vec<_>>(), &["`label` is for `label`"]));
}

#[test]
fn a_label_rule_writes_a_label_event_with_its_cause_when_its_event_comes() {
    let rule = "[[rule]]\nid = \"mark\"\non = \"task.settled\"\nthen = \"label\"\nlabel = { is = \"held\", subject = \"{task}\", producer = \"decider:x\", model = \"m@1\", point = \"intake\", weight = 0.5 }\n";
    let doer = Scripted::new("script", vec![vec![say("done"), end("done")]]);
    let rig = Rig::based(vec![doer], rule, |_| {});
    let id = rig.spawn("go", "script");
    rig.finished(&id);
    let l = rig.until_event("the label", |e| e.kind == "label");
    assert_eq!((l.data["label"].as_str(), l.data["subject"].as_str(), l.data["producer"].as_str(), l.data["model"].as_str(), l.data["point"].as_str(), l.data["subject_kind"].as_str()), (Some("held"), Some(id.as_str()), Some("decider:x"), Some("m@1"), Some("intake"), Some("answer")));
    assert_eq!(l.data["weight"], json!(0.5));
    assert_eq!(l.by, j4n_core::change::Actor::Rule("mark".into()));
    let fired = rig.of_kind("rule.fired");
    assert_eq!(fired.iter().map(|e| e.data["then"].as_str().unwrap()).collect::<Vec<_>>(), ["label"]);
    let settled = rig.of_kind("task.settled")[0].seq;
    assert_eq!(l.cause, Some(settled), "the label follows from the event it answers");
}

// ---- labels from what happened next ----

#[test]
fn a_machines_pick_followed_by_a_switch_on_the_next_line_is_overridden_and_the_rate_reaches_the_fold() {
    let f = fx_trusting(vec![chatty("alpha", &["a", "b"]), chatty("beta", &["c"])], Some("task"), |_| {});
    f.turn("first line");
    let bound = f.bound();
    let said = &bound[0].data["answer"];
    assert_eq!((said["answerer"].as_str(), said["point"].as_str()), (Some("decider:jev"), Some("intake")));
    let ask = said["ask"].as_str().unwrap().to_string();
    assert!(ask.starts_with("intake#"), "{ask}");
    let answered = f.rig.of_kind("ask.answered");
    assert_eq!(answered.iter().filter(|e| e.data["ask"] == ask.as_str() && e.data["by"] == "decider:jev").count(), 1, "the machine answer is written down");
    // the first task settled well: held, which is not wrong
    f.rig.until_event("held", |e| e.kind == "label" && e.data["label"] == "held");
    assert_eq!(labels(&f).len(), 1);
    // the next line names another doer: the pick is overridden
    assert_eq!(f.say(":switch beta"), Said::Command("switch".into()));
    let l = f.rig.until_event("overridden", |e| e.kind == "label" && e.data["label"] == "overridden");
    assert_eq!((l.data["producer"].as_str(), l.data["point"].as_str(), l.data["subject"].as_str(), l.data["weight"].as_f64()), (Some("decider:jev"), Some("intake"), Some(ask.as_str()), Some(1.0)));
    let switch = f.bound().into_iter().find(|e| e.data["how"] == "switch").unwrap();
    assert_eq!(switch.data["overrides"]["ask"], ask.as_str());
    // the rate: one answer, counted once, at the worst thing said of it
    f.rig.log.sync();
    assert_eq!(f.front.asks().state.labelled("decider:jev", "intake"), (1, 1.0));
    // a switch that comes later does not count against it
    f.turn("second");
    f.turn("third");
    let n = labels(&f).len();
    assert_eq!(f.say(":switch alpha"), Said::Command("switch".into()));
    f.rig.log.sync();
    assert_eq!(labels(&f).len(), n, "the pick is no longer recent");
}

#[test]
fn a_task_a_machines_pick_started_is_held_or_failed_after_and_only_the_first_counts() {
    let ok = chatty("alpha", &["fine", "again"]);
    let f = fx_trusting(vec![ok], Some("task"), |c| c.doer = None);
    f.turn("one");
    f.turn("two");
    f.rig.log.sync();
    let all = labels(&f);
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!((all[0]["label"].as_str(), all[0]["weight"].as_f64()), (Some("held"), Some(0.0)));

    let bad = Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Fail("it broke".into())]]);
    let g = fx_trusting(vec![bad], Some("task"), |_| {});
    let Said::Started { task } = g.say("go") else { panic!("{}", g.rig.dump()) };
    g.rig.finished(&task);
    let l = g.rig.until_event("failed_after", |e| e.kind == "label");
    assert_eq!((l.data["label"].as_str(), l.data["weight"].as_f64(), l.data["producer"].as_str()), (Some("failed_after"), Some(0.5), Some("decider:jev")));
}

#[test]
fn the_operator_stopping_the_task_soon_after_overrides_it_and_a_question_the_task_asked_is_nobodys_label() {
    let hang = Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Ask(choice("q1", "which?", &[("a", true), ("b", false)])), Beat::Hold, Beat::Forever]]);
    let f = fx_trusting(vec![hang], Some("task"), |_| {});
    let Said::Started { task } = f.say("go") else { panic!("{}", f.rig.dump()) };
    // the question the task asked is answered by the operator: its child task settles, and that is not the pick's outcome
    let ask = f.rig.until(&task, "waited", |r| matches!(r.state, TaskState::Waiting { .. }));
    let TaskState::Waiting { ask } = ask.state else { unreachable!() };
    f.rig.tasks.ctl(&ask, Ctl::Answer(Answer::Pick { id: "a".into() }), j4n_core::change::Actor::Operator).unwrap();
    f.rig.log.sync();
    assert!(labels(&f).is_empty(), "{:?}", labels(&f));
    f.rig.clock.advance(5_000);
    assert_eq!(f.say(":stop"), Said::Command("stop".into()));
    let l = f.rig.until_event("overridden", |e| e.kind == "label");
    assert_eq!((l.data["label"].as_str(), l.data["weight"].as_f64()), (Some("overridden"), Some(1.0)));
}

#[test]
fn a_change_taken_out_again_is_undone() {
    let writes = Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Put("out.txt".into(), "hello".into()), Beat::Changed(vec!["out.txt".into()]), end("wrote it")]]);
    let f = fx_trusting(vec![writes], Some("task"), |_| {});
    let row = f.turn("write a file");
    assert!(f.rig.project.exists("out.txt"));
    f.rig.until_event("held", |e| e.kind == "label" && e.data["label"] == "held");
    f.rig.tasks.rewind(Some(&row.id), false).unwrap();
    let l = f.rig.until_event("undone", |e| e.kind == "label" && e.data["label"] == "undone");
    assert_eq!((l.data["producer"].as_str(), l.data["point"].as_str(), l.data["weight"].as_f64()), (Some("decider:jev"), Some("intake"), Some(1.0)));
    f.rig.log.sync();
    assert_eq!(f.front.asks().state.labelled("decider:jev", "intake"), (1, 1.0), "held, then undone: one answer, counted as wrong");
}

// ---- two must agree ----

struct Say(&'static str, &'static str, Answer);

impl Answerer for Say {
    fn id(&self) -> &str {
        self.0
    }
    fn kind(&self) -> Kind {
        if self.0.starts_with("decider") { Kind::Decider } else { Kind::Model }
    }
    fn model(&self) -> String {
        self.1.into()
    }
    fn offer(&self, _: &Question) -> Offer {
        Offer::Answer { answer: self.2.clone(), conf: 0.9, why: "it knows".into() }
    }
}

fn gate_point() -> AskPoint {
    AskPoint {
        id: "danger".into(),
        doc: "a test point".into(),
        shape: ShapeKind::Choice,
        impact: Impact::Gate,
        data: DataClass::Project,
        chain: ["decider:a", "model:b", "human"].iter().map(|s| parse_step(s).unwrap()).collect(),
        fallback: Fallback::Safe,
        rank: None,
        breaker: true,
    }
}

fn gate_rig(a: Answer, b: Answer, one: bool, headless: bool) -> (Rig, String) {
    let mut reg = Registry::new();
    reg.register(Arc::new(Say("decider:a", "ma@1", a)));
    if !one {
        reg.register(Arc::new(Say("model:b", "mb@1", b)));
    }
    let asks = Asks::new(vec![gate_point()], reg, Answerers::new()).headless(headless);
    let doer = Scripted::new("script", vec![vec![Beat::Ask(AskSpec { point: Some("danger".into()), ..choice("q1", "delete it?", &[("yes-do", false), ("safe", true)]) }), Beat::Hold, say("went on"), end("went on")]]);
    let rig = Rig::tuned(vec![doer], |c| {
        load_base(c, include_str!("../../../../packs/base/rules/trust.toml").trim_start_matches("api = 1\n"));
        c.asks = Some(Arc::new(asks));
    });
    let id = rig.spawn("clean up", "script");
    (rig, id)
}

#[test]
fn two_that_disagree_send_a_gate_ask_to_the_operator_and_both_are_labelled_disagreed() {
    let (rig, id) = gate_rig(Answer::Pick { id: "yes-do".into() }, Answer::Pick { id: "safe".into() }, false, false);
    let ask = match rig.until(&id, "waited", |r| matches!(r.state, TaskState::Waiting { .. })).state {
        TaskState::Waiting { ask } => ask,
        other => panic!("{other:?}"),
    };
    let ls = rig.until_event("both labelled", |e| e.kind == "label" && e.data["producer"] == "model:b");
    assert_eq!(ls.data["label"], "disagreed");
    rig.log.sync();
    let all: Vec<Value> = rig.of_kind("label").into_iter().map(|e| e.data).collect();
    assert_eq!(all.len(), 2, "{all:?}");
    assert!(all.iter().all(|l| l["label"] == "disagreed" && l["subject"] == ask.as_str() && l["point"] == "danger" && l["weight"] == json!(1.0)));
    let mut who: Vec<&str> = all.iter().map(|l| l["producer"].as_str().unwrap()).collect();
    who.sort();
    assert_eq!(who, ["decider:a", "model:b"]);
    rig.tasks.ctl(&ask, Ctl::Answer(Answer::Pick { id: "safe".into() }), j4n_core::change::Actor::Operator).unwrap();
    assert!(matches!(rig.finished(&id).state, TaskState::Done { ok: true }));
}

#[test]
fn headless_a_disagreement_takes_the_safe_fallback_and_with_one_machine_the_operator_is_asked() {
    let (rig, id) = gate_rig(Answer::Pick { id: "yes-do".into() }, Answer::Pick { id: "safe".into() }, false, true);
    assert!(matches!(rig.finished(&id).state, TaskState::Done { ok: true }));
    let said = rig.of_kind("ask.answered");
    assert_eq!((said[0].data["by"].as_str(), said[0].data["answer"].as_str(), said[0].data["via"].as_str()), (Some("fallback:safe"), Some("safe"), Some("fallback")));
    let labels: Vec<Value> = rig.of_kind("label").into_iter().map(|e| e.data).collect();
    assert_eq!(labels.iter().filter(|l| l["label"] == "disagreed").count(), 2, "{labels:?}");

    let (rig, id) = gate_rig(Answer::Pick { id: "yes-do".into() }, Answer::Pick { id: "yes-do".into() }, true, false);
    let ask = match rig.until(&id, "waited", |r| matches!(r.state, TaskState::Waiting { .. })).state {
        TaskState::Waiting { ask } => ask,
        other => panic!("{other:?}"),
    };
    assert_eq!(rig.of_kind("label").len(), 0, "one machine alone: nothing to agree with");
    rig.tasks.ctl(&ask, Ctl::Answer(Answer::Pick { id: "safe".into() }), j4n_core::change::Actor::Operator).unwrap();
    rig.finished(&id);
}

#[test]
fn two_that_agree_are_labelled_agreed_and_the_walk_goes_on() {
    let (rig, id) = gate_rig(Answer::Pick { id: "safe".into() }, Answer::Pick { id: "safe".into() }, false, false);
    assert!(matches!(rig.finished(&id).state, TaskState::Done { ok: true }));
    rig.until_event("agreed", |e| e.kind == "label" && e.data["producer"] == "model:b");
    rig.log.sync();
    let all: Vec<Value> = rig.of_kind("label").into_iter().map(|e| e.data).collect();
    assert_eq!(all.len(), 2);
    assert!(all.iter().all(|l| l["label"] == "agreed" && l["weight"] == json!(0.0)));
}

// ---- the host a thread is bound to ----

/// Hosts that a test moves: the first not cooling is the pick.
struct Hs {
    order: Vec<&'static str>,
    cooling: Mutex<Vec<String>>,
    elected: Mutex<Vec<(String, Option<String>)>>,
}

impl Hs {
    fn new(order: &[&'static str]) -> Arc<Hs> {
        Arc::new(Hs { order: order.to_vec(), cooling: Mutex::new(Vec::new()), elected: Mutex::new(Vec::new()) })
    }
}

impl Hosts for Hs {
    fn elect(&self, doer: &str, model: Option<&str>, _: &Want, _: u64) -> Option<Pick> {
        self.elected.lock().unwrap().push((doer.into(), model.map(String::from)));
        let cool = self.cooling.lock().unwrap().clone();
        let mut ok = self.order.iter().filter(|h| !cool.iter().any(|c| c == *h));
        let first = ok.next()?;
        Some(Pick { host: first.to_string(), model: "m".into(), why: "first".into(), alts: ok.map(|h| h.to_string()).collect(), lines: vec![], lambda: 0.0 })
    }

    fn cooling(&self, _: &str, _: Option<&str>, host: &str, _: u64) -> Option<String> {
        self.cooling.lock().unwrap().iter().any(|c| c == host).then(|| "3 limits in 5 minutes".to_string())
    }

    fn another(&self, doer: &str, model: Option<&str>, _: &str, from: &str, now: u64) -> Option<(Pick, String)> {
        let p = self.order.iter().find(|h| **h != from && self.cooling(doer, model, h, now).is_none())?;
        Some((Pick { host: p.to_string(), model: "m".into(), why: "next".into(), alts: vec![], lines: vec![], lambda: 0.0 }, "the cache on the old host (~40000 tokens) is left behind".into()))
    }
}

fn hosted(hs: &Arc<Hs>, failover: &'static str) -> Fx {
    let hs = hs.clone();
    fx(vec![chatty("alpha", &["a", "b", "c", "d"])], move |c: &mut Cfg| {
        c.hosts = hs;
        c.failover = failover.into();
    })
}

fn endpoints(f: &Fx) -> Vec<Option<String>> {
    f.tasks().iter().map(|t| t.spec.tune.endpoint.clone()).collect()
}

#[test]
fn a_bound_thread_keeps_its_host_on_every_line_and_the_binding_names_it() {
    let hs = Hs::new(&["Cerebras", "DeepInfra"]);
    let f = hosted(&hs, "wait");
    for line in ["one", "two", "three"] {
        f.turn(line);
    }
    assert_eq!(endpoints(&f), [Some("Cerebras".into()), Some("Cerebras".into()), Some("Cerebras".into())]);
    assert_eq!(f.front.binding("main").and_then(|b| b.endpoint), Some("Cerebras".into()));
    assert_eq!(f.bound()[0].data["route"], "first", "the route row can say why this host");
    let bound = f.bound();
    assert_eq!(bound.len(), 1);
    assert_eq!(bound[0].data["endpoint"], "Cerebras");
    assert_eq!(hs.elected.lock().unwrap().len(), 1, "elected once, not at every line");
    // a switch is an election again; a bound thread whose model is switched may get another host
    hs.cooling.lock().unwrap().push("Cerebras".into());
    assert_eq!(f.say(":switch alpha haiku"), Said::Command("switch".into()));
    assert_eq!(f.front.binding("main").and_then(|b| b.endpoint), Some("DeepInfra".into()));
    f.turn("four");
    assert_eq!(endpoints(&f).last().cloned().flatten(), Some("DeepInfra".into()));
}

#[test]
fn no_hosts_known_the_default_endpoint_stands_and_no_host_is_named() {
    let f = fx(vec![chatty("alpha", &["a"])], |_| {});
    f.turn("one");
    assert_eq!(endpoints(&f), [None]);
    assert_eq!(f.front.binding("main").and_then(|b| b.endpoint), None);
    assert!(f.bound()[0].data["endpoint"].is_null());
}

#[test]
fn a_bound_thread_on_a_cooling_host_waits_moves_or_asks_as_the_setting_says() {
    // wait: the host is kept, and nothing is said
    let hs = Hs::new(&["Cerebras", "DeepInfra"]);
    let f = hosted(&hs, "wait");
    f.turn("one");
    hs.cooling.lock().unwrap().push("Cerebras".into());
    f.turn("two");
    assert_eq!(endpoints(&f), [Some("Cerebras".into()), Some("Cerebras".into())]);
    assert_eq!(f.bound().len(), 1);

    // move: another host, an event, and a notice that says what the cache costs
    let hs = Hs::new(&["Cerebras", "DeepInfra"]);
    let f = hosted(&hs, "move");
    f.turn("one");
    hs.cooling.lock().unwrap().push("Cerebras".into());
    f.turn("two");
    f.turn("three");
    assert_eq!(endpoints(&f), [Some("Cerebras".into()), Some("DeepInfra".into()), Some("DeepInfra".into())]);
    let moved: Vec<_> = f.bound().into_iter().filter(|e| e.data["how"] == "moved").collect();
    assert_eq!(moved.len(), 1, "a move is an event, once");
    assert_eq!((moved[0].data["from"].as_str(), moved[0].data["endpoint"].as_str()), (Some("Cerebras"), Some("DeepInfra")));
    assert!(moved[0].data["cost"].as_str().unwrap().contains("40000 tokens"));
    assert!(f.notes().iter().any(|n| n.contains("Cerebras is cooling") && n.contains("moved to DeepInfra") && n.contains("40000 tokens")), "{:?}", f.notes());

    // ask: the operator is told how to move, and nothing moves
    let hs = Hs::new(&["Cerebras", "DeepInfra"]);
    let f = hosted(&hs, "ask");
    f.turn("one");
    hs.cooling.lock().unwrap().push("Cerebras".into());
    f.turn("two");
    assert_eq!(endpoints(&f), [Some("Cerebras".into()), Some("Cerebras".into())]);
    assert!(f.notes().iter().any(|n| n.contains("`:switch alpha") && n.contains("DeepInfra")), "{:?}", f.notes());

    // move, with no other host: it waits, and says so
    let hs = Hs::new(&["Cerebras"]);
    let f = hosted(&hs, "move");
    f.turn("one");
    hs.cooling.lock().unwrap().push("Cerebras".into());
    f.turn("two");
    assert_eq!(endpoints(&f), [Some("Cerebras".into()), Some("Cerebras".into())]);
    assert!(f.notes().iter().any(|n| n.contains("no other host")), "{:?}", f.notes());
}

#[test]
fn no_machine_pick_no_label_and_probe_with_nothing_to_probe_says_so() {
    let f = fx_trusting(vec![chatty("alpha", &["a", "b"])], None, |_| {});
    f.turn("one");
    f.turn("two");
    assert!(f.bound()[0].data["answer"].is_null(), "{:?}", f.bound()[0].data);
    f.rig.log.sync();
    assert!(labels(&f).is_empty(), "a rule and a rank are data, not labelled");
    assert!(f.rig.of_kind("ask.answered").is_empty());
    assert!(matches!(f.say(":probe"), Said::Refused(why) if why.contains("no decider or model")));
    assert!(f.notes().iter().any(|n| n.contains("no decider or model is installed to probe")));
}

#[test]
fn a_switch_counts_against_a_machines_pick_for_two_lines_and_not_for_three() {
    // one plain line, then the switch: the second line after the pick
    let f = fx_trusting(vec![chatty("alpha", &["a", "b"]), chatty("beta", &["c"])], Some("task"), |_| {});
    f.turn("first");
    f.turn("second");
    assert_eq!(f.say(":switch beta"), Said::Command("switch".into()));
    f.rig.until_event("overridden", |e| e.kind == "label" && e.data["label"] == "overridden");
    // two plain lines, then the switch: too late
    let g = fx_trusting(vec![chatty("alpha", &["a", "b", "c"]), chatty("beta", &["d"])], Some("task"), |_| {});
    g.turn("first");
    g.turn("second");
    g.turn("third");
    assert_eq!(g.say(":switch beta"), Said::Command("switch".into()));
    g.rig.log.sync();
    assert!(g.bound().last().unwrap().data["overrides"].is_null(), "{:?}", g.bound().last().unwrap().data);
    assert!(labels(&g).iter().all(|l| l["label"] != "overridden"), "{:?}", labels(&g));
}

#[test]
fn a_move_is_a_new_binding_with_no_pick_behind_it() {
    let hs = Hs::new(&["Cerebras", "DeepInfra"]);
    let h = hs.clone();
    let f = fx_trusting(vec![chatty("alpha", &["a", "b"])], Some("task"), move |c: &mut Cfg| {
        c.hosts = h;
        c.failover = "move".into();
    });
    f.turn("one");
    assert!(f.front.binding("main").unwrap().answer.is_some());
    hs.cooling.lock().unwrap().push("Cerebras".into());
    f.turn("two");
    let b = f.front.binding("main").unwrap();
    assert_eq!((b.endpoint.as_deref(), b.how.as_str()), (Some("DeepInfra"), "moved"));
    assert!(b.answer.is_none(), "the pick that made the first binding did not make this one");
}

#[test]
fn a_label_whose_subject_comes_out_empty_is_dropped_with_a_notice() {
    let rule = "[[rule]]\nid = \"mark\"\non = \"task.settled\"\nthen = \"label\"\nlabel = { is = \"held\", subject = \"{ev.data.nothing}\", producer = \"decider:x\" }\n";
    let doer = Scripted::new("script", vec![vec![say("done"), end("done")]]);
    let rig = Rig::based(vec![doer], rule, |_| {});
    let id = rig.spawn("go", "script");
    rig.finished(&id);
    let d = rig.until_event("the drop", |e| e.kind == "rule.dropped");
    assert!(d.data["why"].as_str().unwrap().contains("came out empty"), "{:?}", d.data);
    assert!(rig.of_kind("label").is_empty());
}

#[test]
fn an_error_says_which_host_and_model_it_came_from() {
    let doer = Scripted::new("script", vec![vec![Beat::Fail("429 rate limit".into())]]);
    let rig = Rig::based(vec![doer], "", |_| {});
    let mut spec = j4n_core::task::TaskSpec::new("go", "script");
    spec.tune.endpoint = Some("Cerebras".into());
    spec.tune.model = Some("vendor/m".into());
    let id = rig.tasks.spawn(spec, j4n_core::change::Actor::Operator).unwrap();
    rig.finished(&id);
    let e = rig.of_kind("run.error").remove(0);
    assert_eq!((e.data["endpoint"].as_str(), e.data["model"].as_str(), e.data["class"].as_str()), (Some("Cerebras"), Some("vendor/m"), Some("rate-limit")));
}
