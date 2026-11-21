//! The line chooses: the table in `packs/base/tests/named.toml` against `rule:named`,
//! and through the real front. (The case that needs the whole binary is in `crates/j4n/tests/laws/models_cases.rs`.)

use crate::front_cases::{chatty, fx, fx_deciding, fx_over};
use crate::front_models::held;
use crate::world::{Scratch, open_log};
use j4n_core::answer::{Answerer, Offer, Question};
use j4n_core::ask::{AskShape, Opt};
use j4n_core::change::Actor;
use j4n_core::front::Said;
use j4n_core::front::serve::Serve;
use j4n_proto::{Down, Up};
use j4n_core::front::named::{named_rule, parse};
use j4n_core::log::New;
use serde_json::json;

fn table() -> toml::Table {
    include_str!("../../../../packs/base/tests/named.toml").parse().unwrap()
}

#[test]
fn the_named_table_holds_and_its_false_positives_are_counted() {
    let t = table();
    let ids: Vec<String> = t["doers"].as_array().unwrap().iter().map(|d| d.as_str().unwrap().to_string()).collect();
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    let cases = t["case"].as_array().unwrap();
    assert!(cases.len() >= 40, "the table is at least 40 cases: {}", cases.len());
    let (mut named, mut taken, mut wrong, mut ambiguous, mut none) = (0, 0, 0, 0, 0);
    for c in cases {
        let line = c["line"].as_str().unwrap();
        let got = parse(line, &ids);
        match c.get("doer").and_then(|d| d.as_str()) {
            Some(want) => {
                named += 1;
                let got = got.unwrap_or_else(|| panic!("{line:?} names {want}"));
                assert_eq!(got.doer, want, "{line:?}");
                if let Some(words) = c.get("words") {
                    assert_eq!(got.words.join(" "), words.as_str().unwrap(), "{line:?}");
                }
                ambiguous += usize::from(c.get("ambiguous").is_some());
                taken += 1;
            }
            None => {
                none += 1;
                if let Some(got) = got {
                    wrong += 1;
                    eprintln!("false positive: {line:?} -> {}", got.doer);
                }
            }
        }
    }
    assert!(named >= 20 && none >= 15, "named {named}, none {none}");
    // the lines that name no doer are all left alone; the only doers taken that were not asked for are the marked lines
    assert_eq!(wrong, 0, "false positives among the lines that name nothing");
    assert_eq!(ambiguous, 0, "exact syntax has no line the rule cannot tell from a request");
    assert_eq!(taken, named, "every line that names a doer in exact syntax is taken");
}

fn ask(text: &str, ready: &[&str]) -> Question {
    let options = ready.iter().map(|id| Opt { id: (*id).into(), label: (*id).into(), recommended: false }).collect();
    Question { ask: "who:work#1".into(), point: "who:work".into(), asker: "front".into(), thread: None, text: text.into(), shape: AskShape::Choice { options }, goal: text.into(), act: None }
}

#[test]
fn the_rule_picks_only_a_doer_that_is_offered_and_passes_on_every_other_line() {
    let rule = named_rule(vec!["claude".into(), "pi".into()]);
    assert!(matches!(rule.offer(&ask("@pi for this", &["claude", "pi"])), Offer::Answer { answer: j4n_core::ask::Answer::Pick { id }, conf, .. } if id == "pi" && conf == 1.0));
    assert!(matches!(rule.offer(&ask("@pi for this", &["claude"])), Offer::Pass { why } if why.contains("not ready")));
    assert!(matches!(rule.offer(&ask("compute pi", &["claude", "pi"])), Offer::Pass { .. }));
    let yesno = Question { shape: AskShape::YesNo, ..ask("@pi", &[]) };
    assert!(matches!(rule.offer(&yesno), Offer::Pass { .. }));
}

#[test]
fn a_line_that_names_pi_and_a_model_binds_both_by_the_rule_and_one_that_only_has_the_word_binds_by_rank() {
    let (_, cfg) = held(None);
    let f = fx(vec![chatty("claude", &["a"]), chatty("pi", &["b"])], cfg);
    f.turn("@pi deepseek 4.1 to build a game");
    let b = f.front.binding("main").unwrap();
    assert_eq!((b.doer.as_str(), b.model.as_deref(), b.how.as_str()), ("pi", Some("deepseek/deepseek-v4.1-flash-1017"), "elected"));
    let steps = f.rig.of_kind("ask.step");
    assert!(steps.iter().any(|s| s.data["answerer"] == "rule:named" && s.data["outcome"] == "answer"), "{steps:?}");
    assert_eq!(f.tasks()[0].spec.tune.model.as_deref(), Some("deepseek/deepseek-v4.1-flash-1017"), "the task runs on the model in full");

    let (_, cfg) = held(None);
    let g = fx(vec![chatty("claude", &["a"]), chatty("pi", &["b"])], cfg);
    g.turn("compute pi to 50 digits");
    let steps = g.rig.of_kind("ask.step");
    assert!(steps.iter().all(|s| s.data["answerer"] != "rule:named" || s.data["outcome"] != "answer"), "{steps:?}");
    assert!(steps.iter().any(|s| s.data["answerer"] == "rule:named" && s.data["outcome"] == "pass"), "{steps:?}");
    assert_eq!(g.front.binding("main").unwrap().how, "elected");
}

#[test]
fn a_model_word_that_matches_several_takes_the_newest_and_says_so_and_one_the_doer_lacks_names_no_model() {
    let (_, cfg) = held(None);
    let f = fx(vec![chatty("claude", &["a"]), chatty("pi", &["b"])], cfg);
    f.turn("@pi claude 5 about this");
    let b = f.front.binding("main").unwrap();
    assert_eq!(b.model.as_deref(), Some("anthropic/claude-fable-5"));
    assert!(f.notes().iter().any(|n| n.contains("took the newest")), "{:?}", f.notes());
    assert!(matches!(f.say("2"), Said::Started { .. } | Said::Steered { .. } | Said::Refused(_)), "the choice is not left open for a number");
    let (_, cfg) = held(None);
    let g = fx(vec![chatty("claude", &["a"]), chatty("pi", &["b"])], cfg);
    g.turn("@pi mistral large to fix it");
    assert_eq!(g.front.binding("main").unwrap().doer, "pi");
    assert!(g.tasks()[0].spec.tune.model.as_deref().is_none_or(|m| !m.contains("mistral")), "a word the list lacks is not passed on");
}

#[test]
fn a_line_that_names_no_doer_is_bound_by_jev_when_it_answers_else_by_rank_and_the_log_says_which() {
    let (_, cfg) = held(None);
    let f = fx_deciding(vec![chatty("claude", &["a"]), chatty("pi", &["b"])], Some("pi"), cfg);
    f.turn("build a chess game");
    assert_eq!(f.front.binding("main").unwrap().doer, "pi");
    let said = f.rig.of_kind("ask.answered");
    assert!(said.iter().any(|e| e.data["point"] == "who:work" && e.data["by"] == "decider:jev" && e.data["via"] == "chain"), "{said:?}");
    let asked = f.asked.lock().unwrap();
    let q = asked.iter().find(|q| q.point == "who:work").expect("jev was asked who:work");
    let AskShape::Choice { options } = &q.shape else { panic!() };
    assert_eq!(options.iter().map(|o| o.id.as_str()).collect::<Vec<_>>().len(), 2, "the ready doers are the options");

    let (_, cfg) = held(None);
    let g = fx(vec![chatty("claude", &["a"]), chatty("pi", &["b"])], cfg);
    g.turn("build a chess game");
    let steps = g.rig.of_kind("ask.step");
    assert!(steps.iter().any(|s| s.data["answerer"] == "decider:jev" && s.data["outcome"] == "pass"), "{steps:?}");
    assert!(steps.iter().any(|s| s.data["answerer"] == "rank" && s.data["outcome"] == "answer"), "{steps:?}");
    // a line that names a doer never reaches jev
    let (_, cfg) = held(None);
    let h = fx_deciding(vec![chatty("claude", &["a"]), chatty("pi", &["b"])], Some("pi"), cfg);
    h.turn("@claude to build a chess game");
    assert_eq!(h.front.binding("main").unwrap().doer, "claude");
    assert!(h.asked.lock().unwrap().iter().all(|q| q.point != "who:work"));
}

#[test]
fn on_a_bound_thread_a_line_naming_another_doer_moves_it_with_the_way_back_and_one_naming_nothing_stays() {
    let (_, cfg) = held(None);
    let f = fx(vec![chatty("claude", &["a", "b", "c", "d"]), chatty("pi", &["p", "q"])], cfg);
    f.turn("@claude to fix the bug");
    assert_eq!(f.front.binding("main").unwrap().doer, "claude");
    f.turn("and now the second part");
    assert_eq!(f.front.binding("main").unwrap().doer, "claude", "a line that names nothing stays");
    assert_eq!(f.bound().len(), 1);
    f.turn("@pi deepseek 4.1 for the third part");
    let b = f.front.binding("main").unwrap();
    assert_eq!((b.doer.as_str(), b.model.as_deref(), b.how.as_str()), ("pi", Some("deepseek/deepseek-v4.1-flash-1017"), "named"));
    assert_eq!(f.tasks().last().unwrap().spec.doer, "pi", "the line itself goes to pi");
    assert!(f.notes().iter().any(|n| n.contains("now pi · deepseek/deepseek-v4.1-flash-1017 · `:switch claude` to go back")), "{:?}", f.notes());
    f.turn("keep going");
    assert_eq!(f.front.binding("main").unwrap().doer, "pi", "and it stays there");
    // naming the doer it has is no move, and a doer that is not there is said
    f.turn("@pi again");
    assert_eq!(f.bound().len(), 2);
    f.turn("@omp for this");
    assert_eq!(f.front.binding("main").unwrap().doer, "pi");
}

#[test]
fn a_task_that_is_going_is_not_left_by_a_line_that_names_another_doer_the_line_steers_it() {
    use crate::scripted::{Beat, Scripted, end};
    use j4n_core::task::{Ctl, State};
    let slow = Scripted::new("claude", vec![vec![Beat::Session("s".into()), Beat::Hold, end("done")]]);
    let (_, cfg) = held(None);
    let f = fx(vec![slow, chatty("pi", &["p"])], cfg);
    let Said::Started { task } = f.say("@claude to start") else { panic!() };
    f.rig.until(&task, "running", |r| r.state == State::Running);
    assert!(matches!(f.say("@pi for this instead"), Said::Steered { .. }), "{}", f.rig.dump());
    assert_eq!(f.front.binding("main").unwrap().doer, "claude");
    assert_eq!(f.bound().len(), 1);
    assert!(f.rig.tasks.ctl(&task, Ctl::Abort, Actor::Operator).is_ok());
    f.rig.finished(&task);
}

fn bound_before(doer: &str, model: Option<&str>) -> (Scratch, Scratch, j4n_core::log::Log, std::sync::Arc<j4n_core::clock::ManualClock>) {
    let (project, home) = (Scratch::new("proj"), Scratch::new("home"));
    let (log, clock) = open_log(&project);
    let _ = log.append(New::of("thread.bound", Actor::Core, json!({ "doer": doer, "model": model, "how": "elected" })).thread("main"));
    (project, home, log, clock)
}

#[test]
fn doer_pi_on_a_folder_whose_thread_is_on_claude_moves_it_says_so_and_the_strip_shows_the_pin() {
    let (project, home, log, clock) = bound_before("claude", Some("haiku"));
    let (_, cfg) = held(None);
    let f = fx_over(project, home, log, clock, vec![chatty("claude", &["a"]), chatty("pi", &["b"])], None, |c| {
        cfg(c);
        c.doer = Some("pi".into());
        c.model = Some("deepseek/deepseek-v4.1-flash-1017".into());
    });
    let b = f.front.binding("main").unwrap();
    assert_eq!((b.doer.as_str(), b.model.as_deref(), b.how.as_str()), ("pi", Some("deepseek/deepseek-v4.1-flash-1017"), "pinned"));
    let notes = f.notes();
    assert!(notes.iter().any(|n| n.contains("--doer moved this folder's thread from claude · haiku to pi · deepseek/deepseek-v4.1-flash-1017")), "{notes:?}");
    f.turn("keep going");
    assert_eq!(f.tasks().last().unwrap().spec.doer, "pi");
    // the strip says the pin while it holds
    let mut s = Serve::new(f.front.clone());
    let (downs, _) = s.up(Up::Watch { bind: j4n_proto::COMPOSER_STRIP.into() });
    assert!(matches!(&downs[..], [Down::Value { value, .. }] if value["route"] == "pi · deepseek-v4.1-flash-1017 · pinned (--doer)"), "{downs:?}");
    // the pin holds until the operator moves the thread
    assert_eq!(f.say(":switch claude haiku"), Said::Command("switch".into()));
    assert_eq!(f.front.binding("main").unwrap().how, "switch");
    let (downs, _) = s.up(Up::Watch { bind: j4n_proto::COMPOSER_STRIP.into() });
    assert!(matches!(&downs[..], [Down::Value { value, .. }] if value["route"] == "claude · haiku"), "a moved thread is no longer pinned: {downs:?}");
}

#[test]
fn doer_on_a_thread_already_there_moves_nothing_and_says_nothing_but_the_pin_is_marked() {
    let (project, home, log, clock) = bound_before("pi", None);
    let f = fx_over(project, home, log, clock, vec![chatty("claude", &["a"]), chatty("pi", &["b"])], None, |c| c.doer = Some("pi".into()));
    assert!(f.notes().is_empty(), "{:?}", f.notes());
    assert_eq!(f.front.binding("main").unwrap().how, "pinned");
    assert_eq!(f.bound().len(), 2);
    // a launch with the pin already marked writes nothing
    let (project, home, log, clock) = bound_before("pi", None);
    let _ = log.append(New::of("thread.bound", Actor::Core, json!({ "doer": "pi", "how": "pinned" })).thread("main"));
    let f = fx_over(project, home, log, clock, vec![chatty("pi", &["b"])], None, |c| c.doer = Some("pi".into()));
    assert_eq!(f.bound().len(), 2);
}

#[test]
fn a_first_line_that_names_a_doer_goes_past_the_pin_of_the_launch() {
    let (_, cfg) = held(None);
    let f = fx(vec![chatty("claude", &["a"]), chatty("pi", &["b"])], |c| {
        cfg(c);
        c.doer = Some("claude".into());
    });
    f.turn("@pi for this");
    let b = f.front.binding("main").unwrap();
    assert_eq!((b.doer.as_str(), b.how.as_str()), ("pi", "named"));
    let (_, cfg) = held(None);
    let g = fx(vec![chatty("claude", &["a"]), chatty("pi", &["b"])], |c| {
        cfg(c);
        c.doer = Some("claude".into());
    });
    g.turn("build it");
    assert_eq!(g.front.binding("main").unwrap().how, "pinned");
}
