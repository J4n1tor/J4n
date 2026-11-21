//! The envoy through the real front: it opens only where the switches and a screen allow, the questions of the work it holds are shown in its chat and nowhere
//! else (Main says where one waits, and the typed answer is the envoy chat's), what any turn of the envoy read from outside is held for the chat past the end of
//! the turn, a charter is signed in its own chat, and a call that is not an envoy chat's is refused.

use crate::front_cases::{Fx, fx};
use crate::scripted::{Beat, Scripted, approval, end};
use j4n_core::branch::Switch;
use j4n_core::change::Actor;
use j4n_core::front::envoy::CharterArgs;
use j4n_core::front::{Cfg, Said};
use j4n_core::front::serve::Serve;
use j4n_core::task::{State, TaskSpec};
use j4n_proto::{Down, Intent, Up};
use serde_json::json;

fn settings(c: &mut Cfg) {
    c.lanes = Switch::Orchestrate;
    c.chats.screen = true;
    c.crew = j4n_core::crew::Crew::shipped();
    c.crew.envoys = true;
    c.doer = Some("alpha".into());
}

/// An envoy chat is open (the first chat), and a task of Main's for a request of its charter asks leave to run `make build`.
fn asking() -> (Fx, String, String) {
    let alpha = Scripted::new("alpha", vec![vec![Beat::Ask(approval("q", "make build")), Beat::Hold, end("done")]]);
    let f = fx(vec![alpha], settings);
    let Said::Enter(chat) = f.front.envoy_said("main") else { panic!("{}", f.rig.dump()) };
    let mut spec = TaskSpec::new("work for the envoy", "alpha");
    spec.thread = Some("main".into());
    spec.input = json!({ "envoy": { "chat": chat, "charter": "k1" } });
    let task = f.rig.tasks.spawn(spec, Actor::Core).expect("spawns");
    f.rig.until(&task, "asked", |r| matches!(r.state, State::Waiting { .. }));
    (f, chat, task)
}

fn open_asks(s: &mut Serve) -> usize {
    let downs = s.up(Up::Watch { bind: "asks:open".into() }).0;
    downs.iter().filter_map(|d| if let Down::Asks { open } = d { Some(open.len()) } else { None }).next_back().unwrap_or(0)
}

#[test]
fn an_envoy_opens_only_where_the_crew_and_the_word_and_a_screen_allow_and_the_refusal_leads_to_the_switch() {
    let off = fx(vec![Scripted::new("alpha", vec![])], |c| {
        settings(c);
        c.crew.envoys = false;
    });
    let said = off.front.envoy_said("main");
    assert!(!matches!(said, Said::Enter(_)), "{said:?}");
    assert!(off.notes().iter().any(|n| n.contains("envoys are off") && n.contains("⏎ turns them on")), "{:?}", off.notes());
    let single = fx(vec![Scripted::new("alpha", vec![])], |c| {
        settings(c);
        c.lanes = Switch::Single;
    });
    assert!(!matches!(single.front.envoy_said("main"), Said::Enter(_)));
    let on = fx(vec![Scripted::new("alpha", vec![])], settings);
    let Said::Enter(chat) = on.front.envoy_said("main") else { panic!("{:?}", on.notes()) };
    assert!(on.front.is_envoy(&chat), "the core wrote that it is an envoy");
    assert!(on.rig.of_kind("chat.seated").iter().any(|e| e.thread.as_deref() == Some(chat.as_str()) && e.data["seat"] == "envoy"));
}

#[test]
fn a_question_of_the_work_is_shown_in_the_envoy_chat_and_nowhere_else_and_main_says_where_it_waits() {
    let (f, chat, task) = asking();
    assert_eq!(f.front.envoy_of_task(&task).as_deref(), Some(chat.as_str()));
    let mut s = Serve::new(f.front.clone());
    // in Main it is not shown; Main has a row that leads into the chat
    assert_eq!(open_asks(&mut s), 0, "{}", f.rig.dump());
    assert!(f.rig.of_kind("chat.note").iter().any(|e| e.data["text"].as_str().is_some_and(|t| t.contains(&format!("{chat} · a question of its work waits there")))), "{:?}", f.notes());
    // typed in Main it answers nothing; typed in the chat it does
    assert!(!matches!(f.front.say("main", "yes"), Said::Answered { .. }));
    s.up(Up::Intent { intent: Intent::Enter { chat: chat.clone() } });
    let mut inside = Serve::new(f.front.clone());
    inside.up(Up::Intent { intent: Intent::Enter { chat: chat.clone() } });
    assert_eq!(open_asks(&mut inside), 1, "in the envoy chat the question is there");
    assert!(matches!(f.front.say(&chat, "y"), Said::Answered { .. }), "{}", f.rig.dump());
    f.rig.finished(&task);
}

#[test]
fn a_question_of_a_task_that_is_not_for_an_envoy_stays_where_it_was() {
    let alpha = Scripted::new("alpha", vec![vec![Beat::Ask(approval("q", "make build")), Beat::Hold, end("done")]]);
    let f = fx(vec![alpha], settings);
    let Said::Enter(_) = f.front.envoy_said("main") else { panic!() };
    let mut spec = TaskSpec::new("work", "alpha");
    spec.thread = Some("main".into());
    let task = f.rig.tasks.spawn(spec, Actor::Core).expect("spawns");
    f.rig.until(&task, "asked", |r| matches!(r.state, State::Waiting { .. }));
    assert_eq!(f.front.envoy_of_task(&task), None);
    let mut s = Serve::new(f.front.clone());
    assert_eq!(open_asks(&mut s), 1, "an ordinary question of Main's is shown in Main");
    assert!(matches!(f.front.say("main", "y"), Said::Answered { .. }));
    f.rig.finished(&task);
}

#[test]
fn what_a_turn_of_the_envoy_read_from_outside_is_held_for_its_chat_past_the_end_of_the_turn() {
    let alpha = Scripted::new("alpha", vec![vec![end("read it")], vec![end("sends it")]]);
    let f = fx(vec![alpha], settings);
    let Said::Enter(chat) = f.front.envoy_said("main") else { panic!() };
    let gate = f.front.who().gate().expect("a Gate");
    let first = match f.front.say(&chat, "read the page about the tax and tell me") {
        Said::Started { task } => task,
        other => panic!("{other:?}\n{}", f.rig.dump()),
    };
    // the doer reads a page: the Gate marks the task, and the chat holds it
    assert!(gate.taint(&first, "web"));
    f.rig.finished(&first);
    gate.untaint(&first);
    assert_eq!(gate.tainted(&first), None, "the task's own mark is gone with the task");
    assert_eq!(gate.held(&chat).as_deref(), Some("web"), "the chat's is not");
    // a task of an ordinary chat that reads from outside holds nothing for any chat
    let other = f.front.say("main", "something else");
    let _ = other;
    assert_eq!(gate.held("main"), None);
}

#[test]
fn a_call_that_is_not_an_envoy_chats_is_refused_and_so_is_a_draft_with_nothing_to_draft_it() {
    let f = fx(vec![Scripted::new("alpha", vec![vec![end("a")]])], settings);
    let mut spec = TaskSpec::new("a task of Main's", "alpha");
    spec.thread = Some("main".into());
    let task = f.rig.tasks.spawn(spec, Actor::Core).expect("spawns");
    f.rig.finished(&task);
    let args = CharterArgs { goal: "tidy the docs".into(), paths: vec!["docs".into()], may: vec!["write".into()], usd: 1.0, ..CharterArgs::default() };
    assert_eq!(f.front.envoy_charter(&task, &args).unwrap_err(), "only an envoy chat does this");
    assert_eq!(f.front.envoy_request(&task, "k1", "do it").unwrap_err(), "only an envoy chat does this");
    // from an envoy chat's task the call reaches the proposals, and there are none to make them here
    let Said::Enter(chat) = f.front.envoy_said("main") else { panic!() };
    let mut spec = TaskSpec::new("the envoy", "alpha");
    spec.thread = Some(chat);
    let envoy = f.rig.tasks.spawn(spec, Actor::Core).expect("spawns");
    f.rig.finished(&envoy);
    assert_eq!(f.front.envoy_charter(&envoy, &args).unwrap_err(), "nothing here makes proposals");
    let why = f.front.envoy_request(&envoy, "k9", "do it").unwrap_err();
    assert!(why.contains("not signed in this chat"), "{why}");
}

#[test]
fn a_request_while_main_works_waits_for_the_end_of_its_turn_is_never_steered_in_and_is_sent_then() {
    let alpha = Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Pause(1500), end("worked")], vec![end("envoy")], vec![end("worked again")]]);
    let f = fx(vec![alpha], settings);
    let Said::Enter(chat) = f.front.envoy_said("main") else { panic!() };
    let mut spec = TaskSpec::new("the envoy", "alpha");
    spec.thread = Some(chat.clone());
    let charter = j4n_core::charter::Charter { id: "k1".into(), goal: "tidy".into(), watch: String::new(), watch_every_min: 10, paths: vec!["docs".into()], may: vec!["write".into()], doers: vec![], usd: 1.0, until_ms: None, thread: chat.clone(), verbatim: vec![] };
    f.rig.log.append(j4n_core::log::New::of(j4n_core::charter::SIGNED, Actor::Operator, json!({ "charter": charter, "proposal": "p1" }))).expect("signed");
    f.rig.log.sync();
    // Main works a turn of its own, the first of the script
    let main = match f.front.say("main", "look at the tax code") {
        Said::Started { task } => task,
        other => panic!("{other:?}\n{}", f.rig.dump()),
    };
    let envoy = f.rig.tasks.spawn(spec, Actor::Core).expect("spawns");
    f.rig.finished(&envoy);
    let said = f.front.envoy_request(&envoy, "k1", "tidy the docs").expect("it waits");
    assert!(said.contains("waiting for its turn to end"), "{said}");
    assert!(f.rig.of_kind("task.steered").is_empty() && f.rig.of_kind("crew.request").is_empty(), "{}", f.rig.dump());
    // Main's turn ends: at the settled boundary the request goes, as the same request would
    f.rig.finished(&main);
    f.front.plan_step("main");
    assert_eq!(f.rig.of_kind("crew.request").len(), 1, "{}", f.rig.dump());
    assert!(f.rig.of_kind("task.steered").is_empty());
    for t in f.tasks() {
        f.rig.finished(&t.id);
    }
}

#[test]
fn a_chat_main_starts_for_a_request_is_in_the_envoys_tree_and_starts_tainted_when_the_request_was() {
    use j4n_core::front::scout::Want;
    let main = Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Pause(1500), end("worked")]]);
    let pi = Scripted::new("pi", vec![vec![end("found it")]]);
    let f = fx(vec![main, pi], |c| {
        settings(c);
        c.orchestrate.look = true;
        c.orchestrate.scouts = vec!["alpha".into()];
    });
    let Said::Enter(chat) = f.front.envoy_said("main") else { panic!() };
    // Main's task for a request of the envoy, tainted by what the envoy read
    let mut spec = TaskSpec::new("you via c1 · charter k1: look", "alpha");
    spec.thread = Some("main".into());
    spec.role = Some("work".into());
    spec.input = json!({ "envoy": { "chat": chat, "charter": "k1" }, "taint_from": "web" });
    let task = f.rig.tasks.spawn(spec, Actor::Core).expect("spawns");
    let want = Want { goal: "read the three files".into(), paths: ["src/a/1.rs", "src/a/2.rs", "src/a/3.rs"].map(String::from).to_vec(), doer: "pi".into(), model: String::new(), write: false };
    let said = f.front.scout(&task, &want).unwrap_or_else(|e| panic!("{e}\n{}", f.rig.dump()));
    assert!(said.contains("is started"), "{said}");
    let started = f.tasks().into_iter().find(|r| r.spec.input.get("chat").is_some()).expect("the chat's turn");
    assert_eq!(started.spec.input["envoy"]["chat"], chat.as_str(), "its questions go to the envoy");
    assert_eq!(started.spec.input["taint_from"], "web", "it starts as tainted as the request that started Main");
    assert_eq!(f.front.envoy_of_task(&started.id).as_deref(), Some(chat.as_str()));
    for t in f.tasks() {
        f.rig.finished(&t.id);
    }
}
