//! The read: a plain line under the switch is read by the Deciders first, by the quick model only for what they left open, and as typed where nothing did.
//! The Deciders here are scripted (`read:kind=one;…`), the model is a scripted cheap doer that plays back an answer, and what code does with the answers is read back from
//! the Log: no word of a line decides where it goes.

use crate::front_cases::{Fx, chatty, fx, fx_deciding};
use j4n_core::branch::Switch;
use j4n_core::front::Cfg;
use serde_json::Value;

fn looking(c: &mut Cfg) {
    c.doer = Some("claude".into());
    c.lanes = Switch::Orchestrate;
    c.chats.screen = true;
    c.orchestrate.look = true;
    c.cheap.doer = Some("cheap".into());
    // every question may go to the model here; the default is the questions cleared for the model
    c.orchestrate.read_model = ["kind", "goes_on", "chat", "asked"].map(String::from).to_vec();
}

fn looked(f: &Fx) -> Vec<Value> {
    f.rig.of_kind("orchestrate.looked").into_iter().map(|e| e.data).collect()
}

fn model_steps(f: &Fx) -> Vec<j4n_core::task::Row> {
    f.tasks().into_iter().filter(|r| r.spec.role.as_deref() == Some("look")).collect()
}

const SURE: &str = "read:kind=one;read:goes_on=no;read:chat=none;read:asked=none";

#[test]
fn a_decider_sure_of_every_question_settles_the_line_and_no_model_is_called() {
    let f = fx_deciding(vec![chatty("claude", &["done"]), chatty("cheap", &["{}"])], Some(SURE), looking);
    f.turn("fix the typo in the readme");
    let l = &looked(&f)[0];
    assert_eq!(l["read"]["kind"]["by"], "decider:jev", "{l}");
    assert_eq!(l["read"]["kind"]["pick"], "one");
    assert!(l["read"]["open"].as_array().unwrap().is_empty(), "{l}");
    assert_eq!(l["taken"], "one");
    assert!(model_steps(&f).is_empty(), "no model call: {}", f.rig.dump());
    assert!(l["task"].is_null() && l["model"].is_null(), "{l}");
    assert!(f.notes().iter().any(|n| n.starts_with("read as one") && n.contains("decider:jev")), "{:?}", f.notes());
}

#[test]
fn a_decider_under_its_confidence_passes_only_the_open_questions_to_the_model() {
    // the Decider is sure of the kind and says nothing of the doer: the model is asked for the doer alone
    let f = fx_deciding(vec![chatty("claude", &["done"]), chatty("cheap", &[r#"{"asked": null, "why": "no doer is asked for"}"#])], Some("read:kind=one"), looking);
    f.turn("fix the typo in the readme");
    let steps = model_steps(&f);
    assert_eq!(steps.len(), 1, "{}", f.rig.dump());
    let goal = &steps[0].spec.goal;
    assert!(goal.contains("\"asked\"") && !goal.contains("\"kind\": what the line is") && !goal.contains("Operator-line-catalogue"), "only the open question is put: {goal}");
    let l = &looked(&f)[0];
    assert_eq!((l["read"]["kind"]["by"].as_str(), l["read"]["asked"]["by"].as_str().map(|b| b.starts_with("model:"))), (Some("decider:jev"), Some(true)), "{l}");
    assert_eq!(l["read"]["open"].as_array().unwrap().len(), 0, "{l}");
}

#[test]
fn a_line_a_decider_read_as_split_reaches_the_model_for_its_parts_alone() {
    let parts = r#"{"chats": [{"goal": "rewrite the parser", "writes": true, "doer": "", "model": ""}, {"goal": "port the docs site", "writes": true, "doer": "", "model": ""}], "why": "two places"}"#;
    let f = fx_deciding(vec![chatty("claude", &["done", "done", "done"]), chatty("cheap", &[parts])], Some("read:kind=split;read:goes_on=no;read:chat=none;read:asked=none"), looking);
    f.say("rewrite the parser in src/parse and, separately, port the docs site in web/ to the new theme");
    let steps = model_steps(&f);
    assert_eq!(steps.len(), 1, "the model is asked, for the parts: {}", f.rig.dump());
    assert!(steps[0].spec.goal.contains("read as a split: give its parts") && !steps[0].spec.goal.contains("\"kind\": what the line is"), "{}", steps[0].spec.goal);
    let l = &looked(&f)[0];
    assert_eq!((l["read"]["kind"]["by"].as_str(), l["taken"].as_str(), l["chats"].as_u64()), (Some("decider:jev"), Some("split"), Some(2)), "{l}");
}

#[test]
fn a_line_settled_as_split_is_asked_for_its_parts_with_what_a_split_is_and_never_told_none_will_do() {
    // the kind was the Decider's: the model is not asked it again, but it is told what a split is, a new program of several features among it
    let parts = r#"{"chats": [{"goal": "the engine module", "writes": true, "doer": "", "model": ""}, {"goal": "the screen module", "writes": true, "doer": "", "model": ""}], "why": "two modules"}"#;
    let f = fx_deciding(vec![chatty("claude", &["done", "done", "done"]), chatty("cheap", &[parts])], Some("read:kind=split;read:goes_on=no;read:chat=none;read:asked=none"), looking);
    f.say("build a terminal chess game with an engine module, a pieces module and a screen module");
    let steps = model_steps(&f);
    assert_eq!(steps.len(), 1, "{}", f.rig.dump());
    let goal = &steps[0].spec.goal;
    assert!(goal.contains("read as a split: give its parts") && goal.contains("2 to 4"), "the parts are asked for: {goal}");
    assert!(goal.contains("A part that uses what another part makes") && goal.contains("A new program asked for with several features of its own") && goal.contains("modules that Main sets the interfaces of"), "what a split is: {goal}");
    assert!(!goal.contains("empty unless split") && !goal.contains("\"kind\": what the line is"), "no way out to none, and no kind question: {goal}");
}

#[test]
fn the_tetris_line_with_a_in_it_is_read_and_not_skipped() {
    // `with a` made a model name of the line, and the look was skipped without a word
    let f = fx(vec![chatty("claude", &["done"]), chatty("cheap", &[r#"{"kind": "one", "asked": null, "why": "one lane"}"#])], looking);
    f.turn("build me a tetris clone in python with a clean screen and use the arrow keys");
    let l = &looked(&f);
    assert_eq!(l.len(), 1, "{:?}", f.notes());
    assert_eq!((l[0]["decision"].as_str(), l[0]["read"]["kind"]["pick"].as_str()), (Some("one"), Some("one")), "{}", l[0]);
    assert!(l[0]["skipped"].is_null() && l[0]["read"]["asked"]["pick"].is_null(), "no doer was asked for: {}", l[0]);
    assert_eq!(f.front.binding("main").unwrap().doer, "claude", "the line named no doer");
}

#[test]
fn with_no_decider_and_no_model_the_line_goes_to_main_as_typed_and_the_row_says_why() {
    let f = fx(vec![chatty("claude", &["done"])], |c| {
        looking(c);
        c.cheap.doer = None;
    });
    f.turn("fix the typo in the readme");
    let l = &looked(&f)[0];
    assert!(l["error"].as_str().is_some_and(|e| e.contains("no cheap tier")), "{l}");
    assert!(l["read"]["kind"].is_null() && !l["read"]["open"].as_array().unwrap().is_empty(), "{l}");
    assert!(f.notes().iter().any(|n| n.starts_with("the line was not read") && n.contains("goes to Main as typed")), "{:?}", f.notes());
    assert!(model_steps(&f).is_empty());
}

#[test]
fn a_line_typed_while_a_task_works_is_not_read_and_the_log_says_so() {
    use crate::scripted::{Beat, Scripted, end};
    let slow = Scripted::new("claude", vec![vec![Beat::Session("s".into()), Beat::Hold, end("done")]]);
    let f = fx(vec![slow, chatty("cheap", &["{}"])], looking);
    let j4n_core::front::Said::Started { task } = f.say("start the work") else { panic!("{}", f.rig.dump()) };
    f.rig.until(&task, "running", |r| r.state == j4n_core::task::State::Running);
    let before = looked(&f).len();
    let _ = f.say("and also the second part");
    let all = looked(&f);
    assert_eq!(all.len(), before + 1, "the skipped line has its row: {}", f.rig.dump());
    assert_eq!(all.last().unwrap()["skipped"], "a task works: the line steers it");
    let _ = f.rig.tasks.ctl(&task, j4n_core::task::Ctl::Abort, j4n_core::change::Actor::Operator);
    f.rig.finished(&task);
}

/// What a line of the corpus is and what the scripted read says of it: the doer the line asks for (`""`: none), and the doer the line must end up on.
fn corpus() -> Vec<(String, &'static str, &'static str)> {
    // each template is a line whose words would have fooled a word rule (a trigger word before a doer, a model name, a doer's name in prose) or that really asks for a doer
    let prose = [
        "build me a tetris clone with a clean screen",
        "use the arrow keys to move the piece",
        "run the tests on the parser module",
        "compute pi to 50 digits in a script",
        "ask the team whether the release can go out",
        "what do you use for formatting here?",
        "claude is slow today, can you check the retry config",
        "switch to the new theme in the docs site",
        "via the cli, add a --verbose flag",
        "fix the failing test in test_parser.py with a small change",
        "explain how the pi doer is chosen",
        "document the own and test fields of the config",
    ];
    let asked = [
        "please have pi do the refactor of the lexer",
        "ask codex what the retry module does",
        "I want opus on this one: review the diff",
        "let pi write the tests for the gate",
        "get claude to explain the log format",
        "can omp look at the build script?",
    ];
    let mut out = Vec::new();
    for (i, p) in prose.iter().enumerate() {
        for v in ["", "now ", "ok, ", "and ", "then "] {
            let _ = i;
            out.push((format!("{v}{p}"), "", "claude"));
        }
    }
    for (i, p) in asked.iter().enumerate() {
        let doer = ["pi", "pi", "claude", "pi", "claude", "pi"][i];
        out.push((p.to_string(), doer, doer));
    }
    out
}

#[test]
fn sixty_lines_with_words_that_fooled_the_rules_and_lines_that_ask_for_a_doer_go_where_the_scripted_read_says() {
    let cases = corpus();
    assert!(cases.len() >= 60, "{}", cases.len());
    for (line, asked, doer) in &cases {
        let read = if asked.is_empty() { r#"{"kind": "one", "asked": null, "why": "x"}"#.to_string() } else { format!(r#"{{"kind": "one", "asked": {{"doer": "{asked}", "model": ""}}, "why": "x"}}"#) };
        let read: &'static str = Box::leak(read.into_boxed_str());
        let f = fx(vec![chatty("claude", &["a"]), chatty("pi", &["b"]), chatty("cheap", &[read])], looking);
        f.turn(line);
        assert_eq!(f.front.binding("main").unwrap().doer, *doer, "{line:?}: {}", f.rig.dump());
        let l = &looked(&f)[0];
        assert_eq!(l["read"]["asked"]["pick"].is_null(), asked.is_empty(), "{line:?}: {l}");
    }
}

#[test]
fn a_question_the_pilot_did_not_clear_is_not_put_to_the_model_and_the_line_goes_as_typed_for_it() {
    // the default lets the model answer `kind` alone: the doer asked for stays open, and the line is one lane on the doer it had
    let f = fx(vec![chatty("claude", &["a"]), chatty("pi", &["b"]), chatty("cheap", &[r#"{"kind": "one", "asked": {"doer": "pi", "model": ""}, "why": "x"}"#])], |c| {
        looking(c);
        c.orchestrate.read_model = j4n_core::front::look::READ_MODEL.map(String::from).to_vec();
    });
    f.turn("have pi do the refactor of the lexer");
    let steps = model_steps(&f);
    assert_eq!(steps.len(), 1);
    assert!(!steps[0].spec.goal.contains("\"asked\": the doer"), "{}", steps[0].spec.goal);
    let l = &looked(&f)[0];
    assert!(l["read"]["asked"].is_null() && l["read"]["open"].as_array().unwrap().iter().any(|o| o == "read:asked"), "{l}");
    assert_eq!(f.front.binding("main").unwrap().doer, "claude", "the line asked for no doer as far as the read could tell");
}
