//! A question comes from the core whole (how long it waited, what its effect touches and would change, why the Gate asks, who
//! recommended what, which answers widen), and the composer's strip and the binder's guess come from the core too.

use crate::front_cases::{Fx, fx};
use crate::scripted::{Beat, Scripted, approval, choice, end, say};
use j4n_core::change::Actor;
use j4n_core::front::Said;
use j4n_core::front::askview::wait;
use j4n_core::front::serve::Serve;
use j4n_core::gate::{Action, ReadOnly, Verb};
use j4n_core::schema::DataClass;
use j4n_core::task::State;
use j4n_proto::{AskView, By, COMPOSER_BIND, COMPOSER_STRIP, Down, Shape, Tone, Up};
use serde_json::{Value, json};

fn up(s: &mut Serve, u: Up) -> Vec<Down> {
    let (downs, quit) = s.up(u);
    assert!(!quit, "{downs:?}");
    downs
}

fn watch(s: &mut Serve, bind: &str) -> Vec<Down> {
    up(s, Up::Watch { bind: bind.into() })
}

fn value(downs: &[Down], bind: &str) -> Value {
    downs.iter().rev().find_map(|d| match d {
        Down::Value { bind: b, value, .. } if b == bind => Some(value.clone()),
        _ => None,
    }).unwrap_or_else(|| panic!("no value for {bind} in {downs:?}"))
}

fn asks(downs: &[Down]) -> Vec<AskView> {
    downs.iter().rev().find_map(|d| match d {
        Down::Asks { open } => Some(open.clone()),
        _ => None,
    }).unwrap_or_default()
}

/// A thread that has asked: `act` rides with an approval of "make build".
fn asked(act: Option<Action>) -> (Fx, Serve, String) {
    let mut spec = approval("a", "make build");
    spec.act = act.map(Box::new);
    let asker = Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Window(0.42), Beat::Ask(spec), Beat::Hold, end("x")]]);
    let f = fx(vec![asker], |c| c.model = Some("haiku".into()));
    let mut s = Serve::new(f.front.clone());
    let Said::Started { task } = f.say("build") else { panic!() };
    f.rig.until(&task, "waiting", |r| matches!(r.state, State::Waiting { .. }));
    let _ = watch(&mut s, "asks:open");
    (f, s, task)
}

fn open_one(s: &mut Serve) -> AskView {
    s.resync().iter().find_map(|d| match d {
        Down::Asks { open } => open.first().cloned(),
        _ => None,
    }).expect("a question is open")
}

fn shell() -> Action {
    Action::shell(Actor::Doer("alpha".into()), "make build")
}

// ---- how long it waited ----

#[test]
fn a_wait_is_worded_in_minutes_and_seconds_and_hours_after_the_hour() {
    for (ms, want) in [(0, "0:00"), (999, "0:00"), (1_000, "0:01"), (59_999, "0:59"), (60_000, "1:00"), (522_000, "8:42"), (3_599_999, "59:59"), (3_600_000, "1:00:00"), (3_723_000, "1:02:03"), (86_400_000, "24:00:00")] {
        assert_eq!(wait(ms), want, "{ms} ms");
    }
}

#[test]
fn a_question_that_waits_is_sent_again_each_second_with_its_wait_and_not_in_between() {
    let (f, mut s, _) = asked(Some(shell()));
    assert_eq!(open_one(&mut s).wait, "0:00");
    assert!(s.tick().is_empty(), "nothing changed");
    f.rig.clock.advance(522_000);
    let again = asks(&s.tick());
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].wait, "8:42");
    assert!(s.tick().is_empty(), "the same wait is not sent twice");
    f.rig.clock.advance(1_000);
    assert_eq!(asks(&s.tick())[0].wait, "8:43");
}

#[test]
fn a_clock_that_ticks_with_no_question_open_or_watched_sends_nothing() {
    let f = fx(vec![Scripted::new("alpha", vec![vec![say("a"), end("a")]])], |_| {});
    let mut s = Serve::new(f.front.clone());
    assert!(s.tick().is_empty());
    let (f2, mut s2, _) = asked(None);
    s2.up(Up::Unwatch { bind: "asks:open".into() });
    f2.rig.clock.advance(5_000);
    assert!(s2.tick().is_empty(), "a question nobody watches is not sent, though its wait has changed");
}

// ---- what it touches, what it would change, why ----

#[test]
fn an_approval_says_why_the_gate_asks_in_the_gates_own_words() {
    // `make build` is allowed by default, which asks nothing; a write that is not known to lie in the project is asked about
    let act = Action::new(Verb::Write, Actor::Doer("alpha".into()), "src/a.rs");
    let (f, mut s, _) = asked(Some(act.clone()));
    let v = f.front.asks().gate.as_ref().unwrap().check(&act);
    let j4n_core::gate::Verdict::Ask { why, .. } = v else { panic!("a write outside the project is asked about: {v:?}") };
    assert!(!why.is_empty());
    assert_eq!(open_one(&mut s).why, why);
    // an effect no rule speaks of is put to the operator because nothing allows it yet
    let (_f, mut s, _) = asked(Some(shell()));
    assert_eq!(open_one(&mut s).why, "nothing allows it yet");
}

#[test]
fn an_approval_without_an_effect_has_no_why_no_facts_and_no_change() {
    let (_f, mut s, _) = asked(None);
    let a = open_one(&mut s);
    assert_eq!((a.why.as_str(), a.facts.len(), a.change.len(), a.effect.as_str()), ("", 0, 0, ""));
    assert!(a.grants.is_empty() && a.options.is_empty(), "no effect, no grant to offer");
}

#[test]
fn the_facts_are_what_the_effect_carries_and_nothing_else() {
    let mut act = shell();
    act.site = Some("/home/me/work/proj/".into());
    act.inside = Some(false);
    act.read_only = ReadOnly::Inferred;
    act.usd = 0.5;
    let (_f, mut s, _) = asked(Some(act));
    let facts = open_one(&mut s).facts;
    let shown: Vec<(&str, &str, Tone)> = facts.iter().map(|c| (c.icon.as_str(), c.text.as_str(), c.tone)).collect();
    assert_eq!(shown, [("folder", "proj", Tone::Dim), ("risky", "outside the project", Tone::Warn), ("read-only", "read-only", Tone::Dim), ("", "$0.50", Tone::Dim)]);
}

#[test]
fn a_plain_effect_in_the_project_has_only_its_folder_for_a_fact() {
    let mut act = shell();
    act.site = Some("/p/proj".into());
    act.inside = Some(true);
    let (_f, mut s, _) = asked(Some(act));
    let facts = open_one(&mut s).facts;
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].text, "proj");
    let mut bare = shell();
    bare.site = Some(String::new());
    let (_f, mut s, _) = asked(Some(bare));
    assert!(open_one(&mut s).facts.is_empty(), "an empty folder is no fact");
}

#[test]
fn data_leaving_names_its_class() {
    let mut act = Action::new(Verb::Egress, Actor::Core, "zone");
    act.class = Some(DataClass::Private);
    let (_f, mut s, _) = asked(Some(act));
    assert_eq!(open_one(&mut s).facts.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["private data"]);
    // the class means something only for data that leaves
    let mut act = shell();
    act.class = Some(DataClass::Private);
    let (_f, mut s, _) = asked(Some(act));
    assert!(open_one(&mut s).facts.is_empty());
}

#[test]
fn a_write_changes_the_file_it_names_and_a_command_changes_nothing_it_can_name() {
    for verb in [Verb::Write, Verb::Apply] {
        let (_f, mut s, _) = asked(Some(Action::new(verb, Actor::Doer("alpha".into()), "src/a.rs")));
        let a = open_one(&mut s);
        assert_eq!(a.change.len(), 1, "{verb:?}");
        assert_eq!((a.change[0].key.as_str(), a.change[0].cells[0].icon.as_str(), a.change[0].cells[0].text.as_str()), ("src/a.rs", "edit", "src/a.rs"));
        assert!(a.change[0].detail.is_empty(), "the lines of a change not yet made are not known");
    }
    for verb in [Verb::Shell, Verb::Read, Verb::Net, Verb::Spawn] {
        let (_f, mut s, _) = asked(Some(Action::new(verb, Actor::Doer("alpha".into()), "x")));
        assert!(open_one(&mut s).change.is_empty(), "{verb:?}");
    }
}

// ---- which answers widen ----

#[test]
fn the_grants_of_an_approval_are_options_the_core_says_widen() {
    let (_f, mut s, _) = asked(Some(shell()));
    let a = open_one(&mut s);
    assert!(!a.grants.is_empty());
    let ids: Vec<&str> = a.options.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, a.grants.iter().map(String::as_str).collect::<Vec<_>>(), "an option for each grant, in its order");
    for c in &a.options {
        assert!(c.widens && c.by == By::Core && c.label.is_empty() && !c.recommended, "{c:?}");
    }
    assert!(!ids.contains(&"once"), "a yes is already once");
}

#[test]
fn a_choice_marks_a_recommendation_as_the_askers_and_the_rest_as_nobodys() {
    let ask = choice("c", "which colour?", &[("red", false), ("blue", true), ("green", false)]);
    let asker = Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Ask(ask), Beat::Hold, end("x")]]);
    let f = fx(vec![asker], |_| {});
    let mut s = Serve::new(f.front.clone());
    let Said::Started { task } = f.say("pick") else { panic!() };
    f.rig.until(&task, "waiting", |r| matches!(r.state, State::Waiting { .. }));
    let _ = watch(&mut s, "asks:open");
    let a = open_one(&mut s);
    assert_eq!(a.shape, Shape::Choice);
    let by: Vec<(&str, bool, By, bool)> = a.options.iter().map(|c| (c.id.as_str(), c.recommended, c.by, c.widens)).collect();
    assert_eq!(by, [("red", false, By::Operator, false), ("blue", true, By::Doer, false), ("green", false, By::Operator, false)]);
    assert!(a.grants.is_empty());
}

// ---- the strip ----

#[test]
fn the_strip_says_no_doer_yet_until_a_doer_is_chosen_and_then_names_the_route_and_the_gauge() {
    let f = fx(vec![Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Window(0.42), say("a"), end("a")]])], |c| c.model = Some("haiku".into()));
    let mut s = Serve::new(f.front.clone());
    assert_eq!(
        value(&watch(&mut s, COMPOSER_STRIP), COMPOSER_STRIP),
        json!({ "tags": [], "gauge": null, "route": "no doer yet", "route_by": "none", "work": [], "cost": "", "mode": "write", "form": "auto" }),
        "no doer, no gauge: the strip says so"
    );
    let Said::Started { task } = f.say("hello") else { panic!() };
    f.rig.finished(&task);
    let strip = value(&s.resync(), COMPOSER_STRIP);
    assert_eq!(strip, json!({ "tags": [], "gauge": 0.42, "route": "alpha · haiku", "route_by": "core", "work": [], "cost": "", "mode": "write", "form": "auto" }));
}

#[test]
fn the_gauge_is_the_window_of_the_bound_doers_own_session_not_a_branchs_nor_one_fresh_ended() {
    let alpha = Scripted::new("alpha", vec![vec![Beat::Session("a".into()), Beat::Window(0.3), say("a"), end("a")], vec![Beat::Session("a2".into()), Beat::Window(0.05), say("b"), end("b")]]);
    let beta = Scripted::new("beta", vec![vec![Beat::Session("b".into()), Beat::Window(0.9), say("c"), end("c")]]);
    let f = fx(vec![alpha, beta], |_| {});
    let mut s = Serve::new(f.front.clone());
    let gauge = |s: &mut Serve| value(&s.resync(), COMPOSER_STRIP)["gauge"].clone();
    let _ = watch(&mut s, COMPOSER_STRIP);
    f.turn("hello");
    assert_eq!(gauge(&mut s), json!(0.3));
    // a task of another doer in the thread (a branch) reports a fuller window: the strip's doer is not that one
    let mut spec = j4n_core::task::TaskSpec::new("look further", "beta");
    spec.thread = Some("main".into());
    let branch = f.rig.tasks.spawn(spec, Actor::Core).unwrap();
    f.rig.finished(&branch);
    assert_eq!(f.front.tasks().book().get(&branch).and_then(|r| r.window), Some(0.9), "the branch did report one");
    assert_eq!(gauge(&mut s), json!(0.3), "the gauge reads the bound doer's session, not the newest window of the thread");
    // `:fresh` ends the session: the window of the old one is not the new one's
    assert_eq!(f.say(":fresh"), Said::Command("fresh".into()));
    assert_eq!(gauge(&mut s), Value::Null);
    f.turn("again");
    assert_eq!(gauge(&mut s), json!(0.05));
}

#[test]
fn the_route_alone_is_a_strip_and_a_doer_with_no_model_known_says_so() {
    let f = fx(vec![Scripted::new("alpha", vec![vec![Beat::Session("s".into()), say("a"), end("a")]])], |_| {});
    let mut s = Serve::new(f.front.clone());
    let _ = watch(&mut s, COMPOSER_STRIP);
    let Said::Started { task } = f.say("hello") else { panic!() };
    f.rig.finished(&task);
    assert_eq!(value(&s.resync(), COMPOSER_STRIP), json!({ "tags": [], "gauge": null, "route": "alpha · model not known yet", "route_by": "core", "work": [], "cost": "", "mode": "write", "form": "auto" }));
}

/// A chat bound to an account says it in the strip, last; the default login says nothing.
#[test]
fn the_strip_names_the_account_the_thread_is_bound_to() {
    let f = fx(vec![Scripted::new("alpha", vec![vec![Beat::Session("s".into()), say("a"), end("a")]])], |_| {});
    let mut s = Serve::new(f.front.clone());
    let _ = watch(&mut s, COMPOSER_STRIP);
    let Said::Started { task } = f.say("hello") else { panic!() };
    f.rig.finished(&task);
    assert_eq!(value(&s.resync(), COMPOSER_STRIP)["route"], json!("alpha · model not known yet"), "the default login is not named");
    assert_eq!(f.say(":switch alpha account=work"), Said::Command("switch".into()));
    assert_eq!(value(&s.resync(), COMPOSER_STRIP)["route"], json!("alpha · model not known yet · work"));
}

#[test]
fn a_changed_strip_is_sent_when_the_thread_moves_and_the_same_strip_is_not_sent_twice() {
    let f = fx(vec![Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Window(0.1), say("a"), end("a")], vec![Beat::Window(0.7), say("b"), end("b")]])], |_| {});
    let mut s = Serve::new(f.front.clone());
    let _ = watch(&mut s, COMPOSER_STRIP);
    let strips = |s: &mut Serve, from: u64, f: &Fx| -> Vec<Value> {
        f.rig.log.read(from).flat_map(|e| s.ev(&e)).filter_map(|d| match d {
            Down::Value { bind, value, .. } if bind == COMPOSER_STRIP => Some(value),
            _ => None,
        }).collect()
    };
    let before = f.rig.log.head();
    let Said::Started { task } = f.say("one") else { panic!() };
    f.rig.finished(&task);
    let sent = strips(&mut s, before, &f);
    assert!(!sent.is_empty());
    assert_eq!(sent.last().unwrap()["gauge"], json!(0.1));
    assert!(sent.windows(2).all(|w| w[0] != w[1]), "each one differs from the one before: {sent:?}");
    let before = f.rig.log.head();
    let Said::Started { task } = f.say("two") else { panic!() };
    f.rig.finished(&task);
    assert_eq!(strips(&mut s, before, &f).last().unwrap()["gauge"], json!(0.7));
}

// ---- the binder's guess ----

#[test]
fn a_draft_that_would_answer_the_open_question_is_told_what_it_would_do() {
    let (_f, mut s, _) = asked(Some(shell()));
    let _ = watch(&mut s, COMPOSER_BIND);
    let ask = open_one(&mut s).ask;
    let g = value(&up(&mut s, Up::Draft { text: "  yes  ".into(), target: String::new() }), COMPOSER_BIND);
    assert_eq!(g, json!({ "text": "yes", "ask": ask, "mark": "yes", "label": "yes" }));
    let g = value(&up(&mut s, Up::Draft { text: "no thanks".into(), target: String::new() }), COMPOSER_BIND);
    assert_eq!((g["mark"].as_str(), g["label"].as_str(), g["text"].as_str()), (Some("no"), Some("no"), Some("no thanks")));
}

#[test]
fn a_draft_that_is_not_an_answer_is_told_nothing() {
    let (_f, mut s, _) = asked(Some(shell()));
    let _ = watch(&mut s, COMPOSER_BIND);
    for text in ["what does it do?", "also fix the tests", ""] {
        assert_eq!(value(&up(&mut s, Up::Draft { text: text.into(), target: String::new() }), COMPOSER_BIND), Value::Null, "{text:?}");
    }
}

#[test]
fn a_pick_is_labelled_with_the_option_it_names_and_a_draft_unwatched_gets_no_answer() {
    let ask = choice("c", "which colour?", &[("red", false), ("blue", false)]);
    let asker = Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Ask(ask), Beat::Hold, end("x")]]);
    let f = fx(vec![asker], |_| {});
    let mut s = Serve::new(f.front.clone());
    let Said::Started { task } = f.say("pick") else { panic!() };
    f.rig.until(&task, "waiting", |r| matches!(r.state, State::Waiting { .. }));
    assert!(up(&mut s, Up::Draft { text: "2".into(), target: String::new() }).is_empty(), "nobody watches the guess");
    let _ = watch(&mut s, COMPOSER_BIND);
    let g = value(&up(&mut s, Up::Draft { text: "2".into(), target: String::new() }), COMPOSER_BIND);
    assert_eq!((g["label"].as_str(), g["mark"].as_str()), (Some("blue"), Some("")));
}

// ---- more of the same, from the other side ----

#[test]
fn an_effect_that_says_nothing_of_where_it_is_has_no_facts() {
    let (_f, mut s, _) = asked(Some(shell()));
    assert!(open_one(&mut s).facts.is_empty(), "not knowing is not outside the project");
}

#[test]
fn a_recommendation_of_a_flow_is_a_rules_and_of_any_other_doer_a_doers() {
    let ask = choice("c", "which colour?", &[("red", true), ("blue", false)]);
    let flow = Scripted::new("flow:check", vec![vec![Beat::Session("s".into()), Beat::Ask(ask), Beat::Hold, end("x")]]);
    let f = fx(vec![flow], |c| c.doer = Some("flow:check".into()));
    let mut s = Serve::new(f.front.clone());
    let Said::Started { task } = f.say("pick") else { panic!() };
    f.rig.until(&task, "waiting", |r| matches!(r.state, State::Waiting { .. }));
    let _ = watch(&mut s, "asks:open");
    let a = open_one(&mut s);
    assert_eq!((a.options[0].recommended, a.options[0].by, a.options[1].by), (true, By::Rule, By::Operator));
}

#[test]
fn a_line_that_answers_a_question_of_a_free_text_is_a_reply() {
    let ask = j4n_core::ask::AskSpec { local: "t".into(), question: "what should the file be called?".into(), shape: j4n_core::ask::AskShape::Text, inband: true, point: None, act: None };
    let asker = Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Ask(ask), Beat::Hold, end("x")]]);
    let f = fx(vec![asker], |_| {});
    let mut s = Serve::new(f.front.clone());
    let Said::Started { task } = f.say("name it") else { panic!() };
    f.rig.until(&task, "waiting", |r| matches!(r.state, State::Waiting { .. }));
    let _ = watch(&mut s, COMPOSER_BIND);
    let g = value(&up(&mut s, Up::Draft { text: "notes.txt".into(), target: String::new() }), COMPOSER_BIND);
    assert_eq!((g["mark"].as_str(), g["label"].as_str()), (Some(""), Some("reply")), "{g}");
}

#[test]
fn a_question_of_another_thread_is_not_what_a_draft_here_would_answer() {
    let asker = Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Ask(approval("a", "make build")), Beat::Hold, end("x")]]);
    let f = fx(vec![asker], |_| {});
    let mut s = Serve::new(f.front.clone());
    let Said::Started { task } = f.front.say("other", "build") else { panic!() };
    f.rig.until(&task, "waiting", |r| matches!(r.state, State::Waiting { .. }));
    let _ = watch(&mut s, COMPOSER_BIND);
    assert_eq!(value(&up(&mut s, Up::Draft { text: "yes".into(), target: String::new() }), COMPOSER_BIND), Value::Null, "the thread of this client has no question");
}

#[test]
fn a_strip_nobody_watches_is_not_sent() {
    let f = fx(vec![Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Window(0.2), say("a"), end("a")]])], |_| {});
    let mut s = Serve::new(f.front.clone());
    let before = f.rig.log.head();
    let Said::Started { task } = f.say("hello") else { panic!() };
    f.rig.finished(&task);
    let sent: Vec<Down> = f.rig.log.read(before).flat_map(|e| s.ev(&e)).collect();
    assert!(sent.iter().all(|d| !matches!(d, Down::Value { bind, .. } if bind == COMPOSER_STRIP)), "{sent:?}");
}

#[test]
fn a_pick_is_labelled_with_the_label_of_the_question_it_answers_and_not_of_another() {
    use j4n_core::ask::{AskShape, AskSpec, Opt};
    let opt = |id: &str, label: &str| Opt { id: id.into(), label: label.into(), recommended: false };
    let spec = |local: &str, options| AskSpec { local: local.into(), question: format!("{local}?"), shape: AskShape::Choice { options }, inband: true, point: None, act: None };
    let asker = Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Ask(spec("first", vec![opt("a", "Apple"), opt("b", "Bean")])), Beat::Hold, end("x")]]);
    let f = fx(vec![asker], |_| {});
    let mut s = Serve::new(f.front.clone());
    let Said::Started { task } = f.say("pick") else { panic!() };
    f.rig.until(&task, "waiting", |r| matches!(r.state, State::Waiting { .. }));
    let _ = watch(&mut s, COMPOSER_BIND);
    let g = value(&up(&mut s, Up::Draft { text: "b".into(), target: String::new() }), COMPOSER_BIND);
    assert_eq!(g["label"].as_str(), Some("Bean"), "{g}");
}

#[test]
fn the_strip_names_the_model_that_ran_and_not_the_one_asked_for() {
    let f = fx(vec![Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Served("m-5-5-full", "anthropic"), say("a"), end("a")]])], |c| c.model = Some("m".into()));
    let mut s = Serve::new(f.front.clone());
    let _ = watch(&mut s, COMPOSER_STRIP);
    let Said::Started { task } = f.say("hello") else { panic!() };
    f.rig.finished(&task);
    assert_eq!(value(&s.resync(), COMPOSER_STRIP)["route"], json!("alpha · m 5.5-full"));
}

#[test]
fn the_two_cells_of_the_strip_have_their_own_icons() {
    // `lanes auto` and the doer both had the route icon (`lanes single` is not said at all)
    let strip = j4n_core::front::compose::strip(&j4n_core::front::compose::Strip { window: Some(0.4), route: "alpha · haiku", lanes: "auto", ..Default::default() });
    let tag = strip["tags"][0]["icon"].as_str().unwrap_or_default().to_string();
    assert!(!tag.is_empty() && tag != "route", "the right-hand cell is the route's: {strip}");
}

// ---- the card of a question takes any line; with no card the refusal names the question and its keys ----

/// A thread whose doer asks `which c1?` with three options and waits; the doer, and the id of the question.
fn waiting_on_three() -> (Fx, std::sync::Arc<Scripted>, String) {
    let ask = choice("q", "which c1?", &[("tunnel", false), ("jev", false), ("cloud", false)]);
    let asker = Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Ask(ask), Beat::Hold, end("x")]]);
    let f = fx(vec![asker.clone()], |_| {});
    let Said::Started { task } = f.say("pick") else { panic!() };
    f.rig.until(&task, "waiting", |r| matches!(r.state, State::Waiting { .. }));
    let id = f.rig.tasks.book().open_asks().first().map(|a| a.ask.clone()).expect("a question is open");
    (f, asker, id)
}

/// Wait for the doer to be handed an order that `ok` takes: its next input (the answer the line became).
fn saw(doer: &Scripted, ok: impl Fn(&j4n_core::doer::RunCtl) -> bool, what: &str) {
    for _ in 0..400 {
        if doer.ctls().iter().any(&ok) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("the doer was never handed {what}: {:?}", doer.ctls());
}

#[test]
fn a_line_that_is_no_option_answers_a_choice_as_text_while_its_card_is_up() {
    for line in ["sub chat 1", "4", "."] {
        let (f, doer, ask) = waiting_on_three();
        let said = f.front.reply("main", &ask, j4n_proto::Reply::Text { text: line.into() });
        assert_eq!(said, Said::Answered { ask: ask.clone() }, "{line}: {}", f.rig.dump());
        let want = j4n_core::ask::Answer::Text { text: line.into() };
        saw(&doer, |c| matches!(c, j4n_core::doer::RunCtl::Answer { answer, .. } if *answer == want), line);
    }
    // an option's number or words still pick it
    let (f, doer, ask) = waiting_on_three();
    assert_eq!(f.front.reply("main", &ask, j4n_proto::Reply::Text { text: "2".into() }), Said::Answered { ask: ask.clone() });
    saw(&doer, |c| matches!(c, j4n_core::doer::RunCtl::Answer { answer: j4n_core::ask::Answer::Pick { id }, .. } if id == "jev"), "2");
}

#[test]
fn a_line_with_no_card_while_a_question_waits_is_refused_with_the_question_and_its_keys_and_not_the_task() {
    let (f, _doer, ask) = waiting_on_three();
    let Said::Refused(why) = f.say("sub chat 1") else { panic!("{}", f.rig.dump()) };
    assert_eq!(why, format!("{ask} asks \"which c1?\" · 1–3 pick · c type your own"));
    assert!(!why.contains("is waiting for your answer"), "{why}");
}

#[test]
fn a_choice_that_allows_text_takes_a_position_that_is_no_option_as_text_only_when_its_card_is_up() {
    use j4n_core::ask::{Answer, AskShape, Opt};
    use j4n_core::binder::{Bind, OpenAsk, bind};
    let opts = ["a", "b", "c"].map(|o| Opt { id: o.into(), label: o.into(), recommended: false }).to_vec();
    let open = [OpenAsk { ask: "t2".into(), asker: "t1".into(), thread: "main".into(), question: "which?".into(), shape: AskShape::Choice { options: opts }, opened_ms: 0 }];
    assert_eq!(bind("4", &open, None), Bind::NotAnswer, "with no card it is not an answer");
    assert_eq!(bind("4", &open, Some("t2")), Bind::Answer { ask: "t2".into(), answer: Answer::Text { text: "4".into() } });
    assert_eq!(bind("decide yourself", &open, Some("t1")), Bind::Answer { ask: "t2".into(), answer: Answer::Text { text: "decide yourself".into() } });
    assert_eq!(bind("2", &open, Some("t2")), Bind::Answer { ask: "t2".into(), answer: Answer::Pick { id: "b".into() } });
}
