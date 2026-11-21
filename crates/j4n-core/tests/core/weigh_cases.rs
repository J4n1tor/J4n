//! A line is weighed before it is sent, and the weight chooses where it goes. The loop, the front, the Log
//! and the ask point `weigh` are the real ones (the base pack's table of wordings is read from disk) and the doers are scripted; Jev is the fixture's
//! decider, which answers the pick it was given, or is not there. What is asserted is which doer was started, in which session, and what the Log says.

use crate::front_cases::{Fx, fx, fx_deciding};
use crate::scripted::{Beat, Scripted, end};
use j4n_core::front::phrase::Phrases;
use j4n_core::front::{Pin, Said};
use j4n_core::front::models::ModelLists;
use j4n_core::log::Ev;
use serde_json::Value;
use std::sync::Arc;

/// A main doer whose turns report sessions of their own: the first and the third are the chat's, the second is a light session.
fn claude() -> Arc<Scripted> {
    let turn = |session: &str, answer: &str| vec![Beat::Session(session.into()), end(answer)];
    Scripted::new("claude", vec![turn("main-1", "did the work"), turn("light-1", "it reads the files"), turn("main-1", "did more work")])
}

fn pong() -> Arc<Scripted> {
    Scripted::new("light", vec![vec![end("pong")]])
}

/// The switch on, and a cheap tier named (haiku on claude), with `lean` giving the doers whose lean profile says something.
fn on(lean: &[&str]) -> impl FnOnce(&mut j4n_core::front::Cfg) {
    let lean: Vec<String> = lean.iter().map(|d| d.to_string()).collect();
    move |c| {
        c.weigh.on = true;
        c.weigh.lean = lean;
        c.cheap = Pin { doer: Some("claude".into()), model: Some("haiku".into()) };
    }
}

fn weighed(f: &Fx) -> Vec<Ev> {
    f.rig.of_kind("intake.decided").into_iter().filter(|e| e.data["weighed"] == true).collect()
}

fn started(f: &Fx, line: &str) -> String {
    match f.say(line) {
        Said::Started { task } => {
            f.rig.finished(&task);
            task
        }
        other => panic!("{line:?} did not start a task: {other:?}\n{}", f.rig.dump()),
    }
}

fn row(f: &Fx, task: &str) -> j4n_core::task::Row {
    f.tasks().into_iter().find(|r| r.id == task).expect("the task")
}

#[test]
fn a_ping_is_answered_by_the_cheap_model_with_no_harness_and_the_chats_doer_is_never_started() {
    let (main, light) = (claude(), pong());
    let f = fx_deciding(vec![main.clone(), light.clone()], Some("absent"), on(&["claude"]));
    let task = started(&f, "ping");
    let r = row(&f, &task);
    assert_eq!((r.spec.doer.as_str(), r.spec.role.as_deref(), r.answer.as_deref()), ("light", Some("answer"), Some("pong")), "{}", f.rig.dump());
    assert_eq!(r.spec.tune.mode, j4n_core::task::Mode::Read);
    assert!(main.starts().is_empty(), "the chat's doer was not started");
    assert_eq!(light.starts().len(), 1);
    assert_eq!(light.starts()[0].goal, "ping", "it is given the line and nothing else");
    // the table decided: Jev was not asked, and the Log says what the line was taken as and why
    let w = weighed(&f);
    assert_eq!(w.len(), 1, "{}", f.rig.dump());
    assert_eq!((w[0].data["pick"].as_str(), w[0].data["by"].as_str(), w[0].data["doer"].as_str()), (Some("answer"), Some("rule:weigh"), Some("light")));
    assert!(w[0].data["why"].as_str().unwrap().contains("`ping`"), "{}", w[0].data);
    // the row in the chat says what it was taken as, who answers, and the way to the main doer
    assert!(f.notes().iter().any(|n| n.starts_with("took this for `answer`") && n.contains("light") && n.contains("`:ask ping` sends it to the doer of this chat as it is")), "{:?}", f.notes());
    // an answer binds nothing: the thread has no doer until a line of work elects one
    assert_eq!(f.front.binding("main"), None);
}

#[test]
fn a_project_question_goes_to_a_lean_session_on_the_cheap_tier_and_leaves_the_chats_own_session_alone() {
    let (main, light) = (claude(), pong());
    let f = fx_deciding(vec![main.clone(), light], Some("weigh=light;read:goes_on=no"), on(&["claude"]));
    started(&f, ":ask add the flag to main.rs");
    let task = started(&f, "what does the intake rule do in this project?");
    let r = row(&f, &task);
    assert_eq!((r.spec.doer.as_str(), r.spec.role.as_deref(), r.spec.tune.model.as_deref()), ("claude", Some("light"), Some("haiku")), "{}", f.rig.dump());
    assert_eq!(r.spec.tune.mode, j4n_core::task::Mode::Read, "a question is read, never worked");
    assert_eq!(r.spec.resume, None, "a session of its own");
    assert_eq!(main.starts()[1].resume, None, "the light line did not resume the chat's session");
    let w = weighed(&f);
    assert_eq!((w[0].data["pick"].as_str(), w[0].data["doer"].as_str(), w[0].data["model"].as_str()), (Some("light"), Some("claude"), Some("haiku")));
    assert!(f.notes().iter().any(|n| n.starts_with("took this for `light`") && n.contains("`:ask what does the intake rule do in this project?`")), "{:?}", f.notes());
    // the chat's next line goes on in its own session, not in the light one
    started(&f, ":ask and the rest");
    assert_eq!(main.starts()[2].resume.as_deref(), Some("main-1"), "{}", f.rig.dump());
    assert_eq!(f.bound().len(), 1, "the model of the chat's session was never changed");
}

#[test]
fn a_light_line_goes_on_as_a_task_when_the_doers_lean_profile_is_empty_and_the_log_says_so() {
    let f = fx_deciding(vec![claude(), pong()], Some("weigh=light"), on(&[]));
    let task = started(&f, "what does the intake rule do in this project?");
    let r = row(&f, &task);
    assert_eq!((r.spec.doer.as_str(), r.spec.role.as_deref()), ("claude", Some("work")), "it is a task, as it was: {}", f.rig.dump());
    assert_eq!(r.spec.tune.model.as_deref(), None, "and not on the cheap tier");
    let w = weighed(&f);
    assert_eq!((w[0].data["pick"].as_str(), w[0].data["was"].as_str(), w[0].data["by"].as_str()), (Some("task"), Some("light"), Some("decider:jev")), "{}", w[0].data);
    assert!(w[0].data["why"].as_str().unwrap().contains("no lean profile"), "{}", w[0].data);
    assert!(f.notes().iter().any(|n| n.starts_with("weighed `light` (decider:jev), but claude has no lean profile") && n.ends_with("it goes to the doer as a task")), "{:?}", f.notes());
}

#[test]
fn with_the_switch_off_nothing_is_weighed_nothing_is_written_and_a_ping_is_a_task() {
    let (main, light) = (claude(), pong());
    let f = fx_deciding(vec![main.clone(), light.clone()], Some("weigh=answer"), |c| c.cheap = Pin { doer: Some("claude".into()), model: Some("haiku".into()) });
    let task = started(&f, "ping");
    assert_eq!(row(&f, &task).spec.doer, "claude");
    assert_eq!(row(&f, &task).spec.role.as_deref(), Some("work"));
    assert!(light.starts().is_empty());
    assert!(weighed(&f).is_empty() && f.asked.lock().unwrap().iter().all(|q| q.point != "weigh"), "the point was not asked: {}", f.rig.dump());
    assert!(f.notes().iter().all(|n| !n.contains("took this for") && !n.starts_with("weighed")), "{:?}", f.notes());
}

#[test]
fn with_jev_not_installed_a_line_the_table_does_not_know_is_a_task() {
    let f = fx_deciding(vec![claude(), pong()], Some("absent"), on(&["claude"]));
    let task = started(&f, "what does the intake rule do in this project?");
    assert_eq!((row(&f, &task).spec.doer.as_str(), row(&f, &task).spec.role.as_deref()), ("claude", Some("work")));
    let w = weighed(&f);
    assert_eq!((w.len(), w[0].data["pick"].as_str(), w[0].data["doer"].clone()), (1, Some("task"), Value::Null), "{}", f.rig.dump());
    let skipped: Vec<Ev> = f.rig.of_kind("ask.step").into_iter().filter(|e| e.data["answerer"] == "decider:jev" || e.data["by"] == "decider:jev").collect();
    assert!(skipped.iter().any(|e| e.data.to_string().contains("not installed")), "the walk says Jev was not there: {}", f.rig.dump());
    assert!(f.notes().iter().all(|n| !n.contains("took this for")), "{:?}", f.notes());
}

#[test]
fn ask_sends_the_line_to_the_main_doer_as_it_is_whatever_it_would_weigh() {
    let (main, light) = (claude(), pong());
    let f = fx_deciding(vec![main.clone(), light.clone()], Some("weigh=answer;read:goes_on=no"), on(&["claude"]));
    let task = started(&f, ":ask ping");
    let r = row(&f, &task);
    assert_eq!((r.spec.doer.as_str(), r.spec.role.as_deref(), r.spec.goal.as_str()), ("claude", Some("work"), "ping"), "{}", f.rig.dump());
    assert!(light.starts().is_empty(), "the cheap model was not asked");
    assert!(weighed(&f).is_empty(), "a line sent with `:ask` is not weighed");
}

#[test]
fn an_answer_with_no_model_to_answer_it_is_a_task_and_says_why() {
    let f = fx(vec![claude()], on(&["claude"]));
    let task = started(&f, "ping");
    assert_eq!(row(&f, &task).spec.doer, "claude");
    let w = weighed(&f);
    assert_eq!((w[0].data["pick"].as_str(), w[0].data["was"].as_str()), (Some("task"), Some("answer")));
    assert!(w[0].data["why"].as_str().unwrap().contains("no doer `light`"), "{}", w[0].data);
}

#[test]
fn a_line_typed_while_a_task_works_is_not_weighed() {
    let hold = Scripted::new("claude", vec![vec![Beat::Session("s".into()), Beat::Hold, end("done")]]);
    let f = fx_deciding(vec![hold, pong()], Some("absent"), on(&["claude"]));
    let Said::Started { task } = f.say(":ask start the work") else { panic!("{}", f.rig.dump()) };
    f.rig.until(&task, "running", |r| r.state == j4n_core::task::State::Running);
    assert_eq!(f.say("ping"), Said::Steered { task: task.clone() });
    assert!(weighed(&f).is_empty());
    assert!(f.rig.tasks.ctl(&task, j4n_core::task::Ctl::Abort, j4n_core::change::Actor::Operator).is_ok());
    f.rig.finished(&task);
}

#[test]
fn the_tables_wordings_are_matched_by_words_and_typos_and_a_longer_request_is_nobodys() {
    let (phrases, problems) = Phrases::load(&crate::front_cases::base());
    assert!(problems.is_empty(), "{problems:?}");
    let word = |l: &str| phrases.weigh(l).map(|(w, _)| w);
    for line in ["ping", "Ping!", "hello", "helo", "Thanks!", "thank you", "good morning", "are you there"] {
        assert_eq!(word(line).as_deref(), Some("answer"), "{line:?}");
    }
    // the whole line is the wording: filler is not left out (`are you there` is not `there`), and a word of four letters or fewer has no typo
    for line in ["there", "what is there", "you there", "ring", "pin", "pnig", "pong", "ping please", "ping the staging host and report", "hello, add a flag to main.rs", "what does main.rs do?", ""] {
        assert_eq!(word(line), None, "{line:?}");
    }
    // the table's weights are no commands, and a command's wording is no weight
    assert!(phrases.take("ping").is_none() && phrases.weigh("show me the usage").is_none());
}

// ---- the review ----

/// A main doer whose first answer ends in a question; its second turn is in the same session.
fn asking(answer: &str) -> Arc<Scripted> {
    let turn = |a: &str| vec![Beat::Session("main-1".into()), end(a)];
    Scripted::new("claude", vec![turn(answer), turn("applied"), turn("applied again")])
}

fn tasks_of(f: &Fx, doer: &str) -> usize {
    f.tasks().iter().filter(|r| r.spec.doer == doer).count()
}

#[test]
fn a_line_that_goes_on_from_the_doers_question_is_the_doers_never_the_cheap_models() {
    // the answer ended in a question: `sounds good to me` is Jev's `answer` for the bare line, and the doer's confirmation
    let (main, light) = (asking("I changed main.rs. Shall I apply this?"), pong());
    let f = fx_deciding(vec![main.clone(), light.clone()], Some("weigh=answer;read:goes_on=no"), on(&["claude"]));
    started(&f, ":ask change main.rs");
    let task = started(&f, "sounds good to me");
    assert_eq!((row(&f, &task).spec.doer.as_str(), row(&f, &task).spec.resume.as_ref().map(|s| s.id.as_str())), ("claude", Some("main-1")), "{}", f.rig.dump());
    assert!(light.starts().is_empty());
    let w = weighed(&f);
    assert_eq!((w[0].data["pick"].as_str(), w[0].data["by"].as_str()), (Some("task"), Some("front:follow-up")), "{}", w[0].data);
    assert!(w[0].data["why"].as_str().unwrap().contains("ended in a question"), "{}", w[0].data);
    // the same line after an answer that asked nothing stands alone, and Jev is asked
    let (main, light) = (asking("I changed main.rs."), pong());
    let g = fx_deciding(vec![main, light.clone()], Some("weigh=answer;read:goes_on=no"), on(&["claude"]));
    started(&g, ":ask change main.rs");
    started(&g, "sounds good to me");
    assert_eq!(light.starts().len(), 1, "{}", g.rig.dump());
}

#[test]
fn a_short_follow_up_is_the_doers_whatever_it_answered_last_and_a_standalone_question_is_not() {
    for line in ["yes", "Yes, go ahead", "no", "continue", "go on", "and the second one?", "also the tests"] {
        let (main, light) = (asking("I changed main.rs."), pong());
        let f = fx_deciding(vec![main, light.clone()], Some("weigh=answer"), on(&["claude"]));
        started(&f, ":ask change main.rs");
        let task = started(&f, line);
        assert_eq!(row(&f, &task).spec.doer, "claude", "{line:?}: {}", f.rig.dump());
        assert!(light.starts().is_empty(), "{line:?}");
        assert_eq!(weighed(&f)[0].data["by"], "front:follow-up", "{line:?}");
    }
    // the twin: a question that stands alone is weighed, and Jev's `answer` is taken
    let (main, light) = (asking("I changed main.rs."), pong());
    let f = fx_deciding(vec![main, light.clone()], Some("weigh=answer;read:goes_on=no"), on(&["claude"]));
    started(&f, ":ask change main.rs");
    started(&f, "what is the second law of thermodynamics?");
    assert_eq!(light.starts().len(), 1);
    // a thread with no doer yet has nothing to go on from: `yes` is the weighed first line of one
    let g = fx_deciding(vec![asking("x"), pong()], Some("weigh=answer"), on(&["claude"]));
    assert!(matches!(g.say("yes"), Said::Started { .. }));
    assert_eq!(weighed(&g)[0].data["pick"], "answer");
}

struct Lists;

impl ModelLists for Lists {
    fn models(&self, doer: &str) -> Option<Vec<String>> {
        (doer == "claude").then(|| ["haiku", "sonnet", "opus"].map(String::from).into())
    }
}

#[test]
fn a_line_that_names_a_doer_or_a_model_is_not_weighed() {
    // `ask codex …`: the line goes to codex, where the line says, not to a lean session on claude
    let (main, codex, light) = (claude(), Scripted::new("codex", vec![vec![end("it reads files")]]), pong());
    let f = fx_deciding(vec![main.clone(), codex.clone(), light.clone()], Some("weigh=light"), on(&["claude"]));
    let task = started(&f, "@codex what this project does");
    assert_eq!(row(&f, &task).spec.doer, "codex", "{}", f.rig.dump());
    assert_eq!(tasks_of(&f, "claude"), 0);
    assert_eq!((weighed(&f)[0].data["pick"].as_str(), weighed(&f)[0].data["by"].as_str()), (Some("task"), Some("front:named")));
    // `use opus …` names a model of the doer: not a weight's either
    let light = pong();
    let g = fx_deciding(vec![claude(), light.clone()], Some("weigh=answer;read:goes_on=no"), |c| {
        on(&["claude"])(c);
        c.models = Arc::new(Lists);
    });
    started(&g, "@claude opus to say what this project does");
    assert!(light.starts().is_empty(), "{}", g.rig.dump());
    assert_eq!(weighed(&g)[0].data["by"], "front:named");
    // `use` and `on` before a word that is no model name are just words
    let h = fx_deciding(vec![claude(), pong()], Some("weigh=answer;read:goes_on=no"), |c| {
        on(&["claude"])(c);
        c.models = Arc::new(Lists);
    });
    started(&h, "what do you use for tea?");
    assert_eq!(weighed(&h)[0].data["pick"], "answer");
}

#[test]
fn a_jev_that_has_not_passed_its_cases_is_not_heard_and_the_line_is_a_task() {
    let f = fx_deciding(vec![claude(), pong()], Some("unproved:weigh=answer"), on(&["claude"]));
    let task = started(&f, "what is a hash chain?");
    assert_eq!(row(&f, &task).spec.doer, "claude", "{}", f.rig.dump());
    assert_eq!(weighed(&f)[0].data["pick"], "task");
    assert_ne!(weighed(&f)[0].data["by"], "decider:jev");
    assert!(f.asked.lock().unwrap().iter().all(|q| q.point != "weigh"), "the decider was not asked on the line's path");
}

#[test]
fn a_line_with_rows_kept_beside_it_and_a_line_in_an_open_plan_are_not_weighed() {
    let (main, light) = (claude(), pong());
    let f = fx_deciding(vec![main, light.clone()], Some("weigh=answer;read:goes_on=no"), on(&["claude"]));
    assert!(matches!(f.front.say_with("main", "ping", &["r1".to_string()]), Said::Started { .. }), "{}", f.rig.dump());
    assert!(light.starts().is_empty() && weighed(&f).is_empty(), "{}", f.rig.dump());
    // a plan is open: the line is the plan's (its fold), however the table would weigh it
    let items: Vec<Value> = ["a", "b"].iter().map(|id| serde_json::json!({ "id": id, "goal": format!("do {id}"), "reads": [], "writes": [], "depends": [], "done": format!("{id} works"), "where": "inline" })).collect();
    let plan = serde_json::json!({ "approach": "Do a, then b.", "items": items }).to_string();
    let (main, light) = (Scripted::new("claude", vec![vec![Beat::Session("p".into()), crate::scripted::say(&plan), end(&plan)], vec![end("did a")]]), pong());
    let g = crate::front_cases::fx_trusting(vec![main, light.clone()], Some("intake=flow"), |c| {
        on(&["claude"])(c);
        c.doer = Some("claude".into());
        c.lanes = j4n_core::branch::Switch::Auto;
    });
    let Said::Started { task } = g.say("plan how to do a and b, then do them") else { panic!("{}", g.rig.dump()) };
    g.rig.finished(&task);
    g.front.plan_collect("main");
    assert!(g.keeper.open("main"), "{}", g.rig.dump());
    g.say("ping");
    assert!(light.starts().is_empty(), "the plan's fold took it: {}", g.rig.dump());
    assert!(weighed(&g).iter().all(|e| e.data["pick"] != "answer"), "{}", g.rig.dump());
}
