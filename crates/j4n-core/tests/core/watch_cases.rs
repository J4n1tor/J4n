//! The envoy's watch through the real front: a check that a signed charter names is run when it is due, on the screen's clock only, and a failure sends Main a request
//! under the charter; a pass, a check that could not run, a tree that is at work, an ended charter and a request that cannot be sent each have their twin. In Tier 0
//! form: a check breaks while a charter watches it, the envoy notices within one period, Main's task carries the charter's mark so that an approval inside its paths is
//! answered by the charter and one outside is shown in the envoy chat, and every case where nothing could be sent is a row there.

use crate::front_cases::{Fx, fx};
use crate::scripted::{Beat, Scripted, approval, end};
use j4n_core::branch::Switch;
use j4n_core::change::Actor;
use j4n_core::charter::{Charter, SIGNED};
use j4n_core::clock::Clock;
use j4n_core::front::serve::Serve;
use j4n_core::front::{Cfg, Said};
use j4n_core::gate::{Action, Verb};
use j4n_core::log::New;
use j4n_core::task::{Row, State, TaskSpec};
use j4n_proto::{Down, Intent, Up};
use serde_json::json;
use std::sync::Arc;

const MINUTE: u64 = 60_000;

fn settings(c: &mut Cfg) {
    c.lanes = Switch::Orchestrate;
    c.chats.screen = true;
    c.crew = j4n_core::crew::Crew::shipped();
    c.crew.envoys = true;
    c.doer = Some("alpha".into());
    c.checks = vec!["unittest".into()];
}

fn charter(chat: &str) -> Charter {
    Charter { id: "k1".into(), goal: "keep the tax tests green".into(), watch: "unittest".into(), watch_every_min: 1, paths: vec!["docs".into()], may: vec!["write".into()], doers: vec![], usd: 5.0, until_ms: None, thread: chat.into(), verbatim: vec!["keep the tax tests green".into()] }
}

/// The envoy chat has talked, and its charter is signed; the watch's first run is one minute away.
fn watched(alpha: Vec<Vec<Beat>>, check: Vec<Vec<Beat>>) -> (Fx, String) {
    let f = fx(vec![Scripted::new("alpha", alpha), Scripted::new("check", check)], settings);
    let Said::Enter(chat) = f.front.envoy_said("main") else { panic!("{:?}", f.notes()) };
    let Said::Started { task } = f.front.say(&chat, "keep the tax tests green") else { panic!("{}", f.rig.dump()) };
    f.rig.finished(&task);
    f.rig.log.append(New::of(SIGNED, Actor::Operator, json!({ "charter": charter(&chat), "proposal": "p1" }))).expect("signed");
    f.rig.log.sync();
    f.front.watch_step();
    (f, chat)
}

fn checks(f: &Fx) -> Vec<Row> {
    f.tasks().into_iter().filter(|r| r.spec.doer == "check").collect()
}

fn requests(f: &Fx) -> Vec<j4n_core::log::Ev> {
    f.rig.of_kind("crew.request")
}

/// Every task is over and the steps of the threads have run, until nothing more starts.
fn quiet(f: &Fx) {
    for _ in 0..6 {
        let n = f.tasks().len();
        for t in f.tasks() {
            f.rig.finished(&t.id);
        }
        f.front.plan_step("main");
        f.front.plan_step("c1");
        if f.tasks().len() == n {
            return;
        }
    }
}

/// Everything is quiet, a minute passes, the watch runs the check, and what it came to is taken.
fn run_once(f: &Fx) {
    quiet(f);
    due_run(f);
}

/// A minute passes, the watch runs the check, and what it came to is taken, whatever else is at work.
fn due_run(f: &Fx) {
    let n = checks(f).len();
    f.rig.clock.advance(MINUTE);
    f.front.watch_step();
    assert_eq!(checks(f).len(), n + 1, "the check was started: {:?}", f.notes().iter().filter(|n| n.contains("watch")).collect::<Vec<_>>());
    let last = checks(f).pop().expect("started");
    f.rig.finished(&last.id);
    f.front.watch_step();
}

fn said_in(f: &Fx, chat: &str, what: &str) -> bool {
    f.rig.of_kind("chat.note").iter().any(|e| e.thread.as_deref() == Some(chat) && e.data["text"].as_str().is_some_and(|t| t.contains(what)))
}

#[test]
fn a_check_that_a_charter_watches_is_run_when_it_is_due_and_not_before_and_a_failure_sends_main_a_request_under_the_charter() {
    let (f, chat) = watched(vec![vec![end("ok")], vec![end("fixed")]], vec![vec![Beat::Fail("2 failed: test_round, test_cart".into())]]);
    assert!(checks(&f).is_empty(), "not yet: one minute after the signing");
    f.rig.clock.advance(MINUTE - 1);
    f.front.watch_step();
    assert!(checks(&f).is_empty(), "a millisecond short");
    f.rig.clock.advance(1);
    f.front.watch_step();
    let run = checks(&f).pop().expect("due: it runs");
    assert_eq!((run.spec.role.as_deref(), run.spec.input["check"].as_str(), run.spec.input["strict"].as_bool()), (Some("check"), Some("unittest"), Some(true)));
    assert_eq!(run.spec.input["envoy"]["chat"], chat.as_str(), "the envoy's tree holds it, so its cost is the charter's");
    assert_eq!(f.front.envoy_of_task(&run.id).as_deref(), Some(chat.as_str()));
    // it is no turn of the chat: the chat's own last turn is still its own, and a line the operator types goes to the chat
    assert_ne!(run.spec.thread.as_deref(), Some(chat.as_str()));
    f.front.watch_step();
    assert_eq!(checks(&f).len(), 1, "one run at a time: nothing else starts while it is at work");
    f.rig.finished(&run.id);
    f.front.watch_step();
    let sent = requests(&f);
    assert_eq!((sent.len(), sent[0].data["source"].as_str(), sent[0].by.clone()), (1, Some("watch"), Actor::Core), "{}", f.rig.dump());
    let main = f.tasks().into_iter().find(|r| r.spec.thread.as_deref() == Some("main") && r.spec.goal.starts_with(&format!("you via {chat} · charter k1 · watch:"))).expect("Main's turn");
    assert!(main.spec.goal.contains("2 failed: test_round, test_cart") && main.spec.goal.contains("quoted as data"), "{}", main.spec.goal);
    assert_eq!(main.spec.input["envoy"]["charter"], "k1");
    assert!(said_in(&f, &chat, "watch · `unittest` fails · sent to Main"), "{:?}", f.notes());
    // the next run is one period after this one
    f.rig.finished(&main.id);
    f.rig.clock.advance(MINUTE - 1);
    f.front.watch_step();
    assert_eq!(checks(&f).len(), 1);
}

#[test]
fn a_pass_and_a_check_that_could_not_run_send_nothing_and_a_pass_after_a_failure_says_so() {
    let (f, chat) = watched(vec![vec![end("ok")], vec![end("fixed")]], vec![vec![Beat::Fail("1 failed".into())], vec![end("green")], vec![Beat::NotRun("no unittest here".into()), end("no check applies")]]);
    run_once(&f);
    assert_eq!(requests(&f).len(), 1);
    // green: no request, and the envoy chat says it passes again
    run_once(&f);
    assert_eq!(requests(&f).len(), 1);
    assert!(said_in(&f, &chat, "watch · `unittest` passes"), "{:?}", f.notes());
    // could not run: no evidence either way
    run_once(&f);
    assert_eq!(requests(&f).len(), 1);
    assert!(said_in(&f, &chat, "could not run: no unittest here"), "{:?}", f.notes());
}

#[test]
fn nothing_is_run_after_the_charter_ends_or_while_a_task_of_its_tree_works_or_without_the_switches() {
    let (f, chat) = watched(vec![vec![end("ok")], vec![Beat::Session("s".into()), Beat::Pause(1200), end("working")]], vec![vec![Beat::Fail("1 failed".into())], vec![Beat::Fail("1 failed".into())]]);
    // a task of the tree is at work (Main on a request of the charter): half-written work is no check, and the run waits
    let mut spec = TaskSpec::new("work for the envoy", "alpha");
    spec.thread = Some("main".into());
    spec.input = json!({ "envoy": { "chat": chat, "charter": "k1" } });
    let busy = f.rig.tasks.spawn(spec, Actor::Core).expect("spawns");
    f.rig.clock.advance(MINUTE);
    f.front.watch_step();
    assert!(checks(&f).is_empty(), "the tree is at work");
    f.rig.finished(&busy);
    f.front.watch_step();
    assert_eq!(checks(&f).len(), 1, "the same moment, with the tree quiet, it runs");
    let run = checks(&f).pop().unwrap();
    f.rig.finished(&run.id);
    // the operator ends the charter: no run starts and nothing is sent
    f.rig.log.append(New::of(j4n_core::charter::ENDED, Actor::Operator, json!({ "id": "k1" }))).expect("ended");
    f.rig.log.sync();
    f.front.watch_step();
    f.rig.clock.advance(MINUTE);
    f.front.watch_step();
    assert_eq!((checks(&f).len(), requests(&f).len()), (1, 0), "{}", f.rig.dump());
    // with the switch off there is no watch, though a charter and a record of an envoy are there
    let off = fx(vec![Scripted::new("alpha", vec![]), Scripted::new("check", vec![vec![end("green")]])], |c| {
        settings(c);
        c.crew.envoys = false;
    });
    off.rig.log.append(New::of("chat.seated", Actor::Core, json!({ "seat": "envoy" })).thread("c1")).expect("seated");
    off.rig.log.append(New::of(SIGNED, Actor::Operator, json!({ "charter": charter("c1"), "proposal": "p1" }))).expect("signed");
    off.rig.log.sync();
    off.front.watch_step();
    off.rig.clock.advance(5 * MINUTE);
    off.front.watch_step();
    assert!(checks(&off).is_empty());
}

#[test]
fn the_screens_clock_is_what_runs_the_watch() {
    let (f, _) = watched(vec![vec![end("ok")]], vec![vec![end("green")]]);
    let mut serve = Serve::new(f.front.clone());
    serve.tick();
    assert!(checks(&f).is_empty());
    f.rig.clock.advance(MINUTE);
    serve.tick();
    assert_eq!(checks(&f).len(), 1, "a tick of the screen runs what is due");
}

#[test]
fn a_failure_while_main_works_waits_for_its_turn_to_end_and_goes_then() {
    let busy = || vec![Beat::Session("s".into()), Beat::Pause(1200), end("busy")];
    let (f, chat) = watched(vec![vec![end("ok")], busy(), vec![end("fixed")]], vec![vec![Beat::Fail("1 failed".into())]]);
    let Said::Started { task: main } = f.front.say("main", "look at something else") else { panic!("{}", f.rig.dump()) };
    due_run(&f);
    assert!(requests(&f).is_empty(), "Main works: nothing is steered in");
    assert!(said_in(&f, &chat, "waits for Main's turn to end"), "{:?}", f.notes());
    f.rig.finished(&main);
    f.front.plan_step("main");
    assert_eq!(requests(&f).len(), 1, "Main's turn ended: the request goes, checked again");
    assert!(f.rig.of_kind("task.steered").is_empty());
}

#[test]
fn a_request_that_waits_for_main_is_dropped_with_a_row_when_the_check_passes_before_it_goes() {
    let busy = || vec![Beat::Session("s".into()), Beat::Pause(2500), end("busy")];
    let (f, chat) = watched(vec![vec![end("ok")], busy()], vec![vec![Beat::Fail("1 failed".into())], vec![end("green")]]);
    let Said::Started { task: main } = f.front.say("main", "look at something else") else { panic!("{}", f.rig.dump()) };
    due_run(&f);
    due_run(&f);
    assert!(said_in(&f, &chat, "the request that waited for Main is dropped"), "{:?}", f.notes());
    f.rig.finished(&main);
    f.front.plan_step("main");
    assert!(requests(&f).is_empty(), "what was dropped is never sent");
}

#[test]
fn q7_the_envoy_notices_a_broken_check_within_a_period_and_the_fix_is_inside_the_charter_and_a_question_outside_it_is_in_the_envoy_chat() {
    // Main's turn for the watch's request writes inside `docs`, then outside it
    let inside = |f: &Fx| -> Action {
        let mut a = Action::new(Verb::Write, Actor::Doer("alpha".into()), "docs/tax.md");
        a.inside = Some(true);
        a.site = Some(f.rig.project.path().display().to_string());
        a
    };
    let _ = inside;
    let project = std::env::temp_dir();
    let write = |path: &str| {
        let mut spec = approval("q", path);
        let mut a = Action::new(Verb::Write, Actor::Doer("alpha".into()), path);
        a.inside = Some(true);
        a.site = Some(project.display().to_string());
        spec.act = Some(Box::new(a));
        spec
    };
    let alpha = vec![vec![end("ok")], vec![Beat::Ask(write("docs/tax.md")), Beat::Hold, Beat::Ask(write("src/tax.rs")), Beat::Hold, end("fixed")]];
    let (f, chat) = watched(alpha, vec![vec![Beat::Fail("test_round fails".into())]]);
    let before = f.rig.clock.now_ms();
    run_once(&f);
    let noticed = f.rig.clock.now_ms() - before;
    assert!(noticed <= MINUTE, "noticed within one period");
    let main = f.tasks().into_iter().find(|r| r.spec.thread.as_deref() == Some("main") && r.spec.input.get("envoy").is_some()).expect("Main's task carries the charter's mark");
    // the write inside the charter's paths is answered by the charter; the one outside waits, and is shown in the envoy chat and not in Main
    let end_at = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while f.rig.of_kind("ask.answered").is_empty() && std::time::Instant::now() < end_at {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let answered = f.rig.of_kind("ask.answered");
    assert_eq!(answered.first().map(|e| e.data["by"].as_str().unwrap_or_default()), Some("rule:charter-k1"), "{}", f.rig.dump());
    f.rig.until(&main.id, "waiting for the question outside", |r| matches!(r.state, State::Waiting { .. }));
    let end_at = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let mut in_chat = 0;
    while in_chat == 0 && std::time::Instant::now() < end_at {
        let mut inside = Serve::new(f.front.clone());
        inside.up(Up::Intent { intent: Intent::Enter { chat: chat.clone() } });
        let downs = inside.up(Up::Watch { bind: "asks:open".into() }).0;
        in_chat = downs.iter().filter_map(|d| if let Down::Asks { open } = d { Some(open.len()) } else { None }).next_back().unwrap_or(0);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(in_chat, 1, "the question outside the charter is in the envoy chat: none is lost");
    let mut top = Serve::new(f.front.clone());
    let downs = top.up(Up::Watch { bind: "asks:open".into() }).0;
    assert!(downs.iter().all(|d| !matches!(d, Down::Asks { open } if !open.is_empty())), "and not in Main");
    let _ = Arc::new(());
    f.front.say(&chat, "n");
    f.rig.finished(&main.id);
}

#[test]
fn every_case_where_a_failure_cannot_be_sent_is_a_row_in_the_envoy_chat_the_rounds_and_a_destructive_output_included() {
    let ok = || vec![end("ok")];
    let (f, chat) = watched(vec![ok(), ok(), ok(), ok(), ok(), ok(), ok(), ok(), ok(), ok()], vec![vec![Beat::Fail("1 failed".into())], vec![Beat::Fail("2 failed".into())], vec![Beat::Fail("3 failed".into())], vec![Beat::Fail("4 failed".into())], vec![Beat::Fail("5 failed".into())]]);
    for _ in 0..3 {
        run_once(&f);
    }
    assert_eq!(requests(&f).len(), 3);
    run_once(&f);
    assert_eq!(requests(&f).len(), 3, "the rounds are used");
    assert!(said_in(&f, &chat, "watch · `unittest` fails and nothing was sent: charter k1 has sent Main 3 requests"), "{:?}", f.notes());
    // signed again by the operator, the rounds start again: the old requests were another signing's
    f.rig.clock.advance(1);
    f.rig.log.append(New::of(SIGNED, Actor::Operator, json!({ "charter": charter(&chat), "proposal": "p2" }))).expect("signed again");
    f.rig.log.sync();
    f.rig.clock.advance(1);
    run_once(&f);
    assert_eq!(requests(&f).len(), 4, "{:?}", f.notes());
    // an output that carries a destructive step the operator never said is not sent, and the row says why
    let (g, chat) = watched(vec![ok(), ok()], vec![vec![Beat::Fail("cleanup failed: rm -rf build".into())]]);
    run_once(&g);
    assert!(requests(&g).is_empty());
    assert!(said_in(&g, &chat, "nothing was sent: the request carries `rm -rf`"), "{:?}", g.notes());
}

#[test]
fn a_charter_that_has_spent_its_budget_sends_nothing_and_says_so_and_one_with_money_left_does() {
    use j4n_core::charter::Hooks;
    use j4n_core::gate::{Gate, check::Gates, grant::Grants};
    use std::sync::Mutex;
    let spent = Arc::new(Mutex::new(0.0_f64));
    let s = spent.clone();
    let f = fx(vec![Scripted::new("alpha", vec![vec![end("ok")], vec![end("fixed")]]), Scripted::new("check", vec![vec![Beat::Fail("1 failed".into())], vec![Beat::Fail("1 failed".into())]])], move |c| {
        settings(c);
        let (gates, _) = Gates::load(&crate::front_cases::base());
        let s = s.clone();
        c.charters.attach(Hooks { gate: Gate::new(gates, Grants::new("s")), now: Box::new(|| 0), tree: Box::new(|_| Vec::new()), spent: Box::new(move |_| *s.lock().unwrap()), checks: Box::new(Vec::new) });
    });
    let Said::Enter(chat) = f.front.envoy_said("main") else { panic!() };
    let Said::Started { task } = f.front.say(&chat, "keep the tax tests green") else { panic!() };
    f.rig.finished(&task);
    f.rig.log.append(New::of(SIGNED, Actor::Operator, json!({ "charter": charter(&chat), "proposal": "p1" }))).expect("signed");
    f.rig.log.sync();
    f.front.watch_step();
    *spent.lock().unwrap() = 5.0;
    run_once(&f);
    assert!(requests(&f).is_empty());
    assert!(said_in(&f, &chat, "nothing was sent: charter k1 has spent its $5.00"), "{:?}", f.notes());
    *spent.lock().unwrap() = 4.99;
    run_once(&f);
    assert_eq!(requests(&f).len(), 1, "a cent under, it goes");
}
