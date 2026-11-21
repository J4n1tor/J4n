//! The memory: `:remember` keeps a note, `:recall` finds it, a restart finds it still (the index is a file made from the
//! Log), and a model that is given `memory` among its tools can call `recall` — and is never given what is `private`.

use crate::screen_drive::Operator;
use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::gate::grant::here_dir;
use j4n_core::task::Mode;
use serde_json::json;
use std::fs;

fn open_as(d: &Dirs, doer: &str, approval: Approval, interactive: bool) -> World {
    let opts = Opts { doer: Some(doer.into()), model: None, approval: Some(approval), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    crate::support::opened(|| wire::open(setup.clone(), &opts, interactive, "main"))
}

fn say(op: &mut Operator, world: &World, line: &str, answer: &str) -> String {
    crate::screen_drive::wait_idle(world);
    op.type_line(line);
    op.until(answer, |s| s.contains(answer))
}

#[test]
fn a_note_is_kept_and_found_by_its_words_and_after_a_restart() {
    let d = Dirs::new("mem-note");
    let world = open_as(&d, "test", Approval::Ask, true);
    let mut op = Operator::sit(&world);
    say(&mut op, &world, ":remember the staging deploy needs the tunnel on", "remembered as #");
    let s = say(&mut op, &world, ":recall deploy tunnel", "note in main");
    assert!(s.contains('«'), "{s}");
    say(&mut op, &world, ":recall gardening", "nothing in the memory matches gardening");
    drop(op);
    world.close();
    drop(world);
    assert!(d.project().join(".j4n/memory.db").exists());

    let world = open_as(&d, "test", Approval::Ask, true);
    let mut op = Operator::sit(&world);
    let s = say(&mut op, &world, ":recall staging", "note in main");
    assert!(s.contains("needs the tunnel"), "{s}");
    drop(op);
    world.close();
    drop(world);

    // the index is only a copy: with the file gone it is made again from the Log
    fs::remove_file(d.project().join(".j4n/memory.db")).unwrap();
    let world = open_as(&d, "test", Approval::Ask, true);
    let mut op = Operator::sit(&world);
    say(&mut op, &world, ":recall staging", "note in main");
    world.close();
}

#[test]
fn a_model_that_names_memory_among_its_tools_can_recall_and_one_that_does_not_has_no_such_function() {
    use crate::models_cases::mock::{Mock, Say};
    let d = Dirs::new("mem-tool");
    let here = here_dir(&d.j4n_home(), &d.project());
    fs::create_dir_all(here.join("connectors")).unwrap();
    fs::write(here.join("settings.toml"), "api = 1\n[[setting]]\nid = \"box.unavailable\"\nvalue = \"here\"\n").unwrap();
    // a note, one that is private, and one that is not
    let world = open_as(&d, "test", Approval::Full, false);
    world.front.say("main", ":remember the staging key lives in the vault");
    crate::screen_drive::wait_idle(&world);
    world.front.log().append(j4n_core::log::New::of("note", j4n_core::change::Actor::Operator, json!({ "text": "the vault password is a private thing" })).class(j4n_core::schema::DataClass::Private)).unwrap();
    world.close();
    drop(world);

    let gateway = Mock::start(|seen, n| {
        if n == 0 {
            let names: Vec<String> = seen.json()["tools"].as_array().unwrap().iter().map(|t| t["function"]["name"].as_str().unwrap().to_string()).collect();
            assert!(names.contains(&"recall".to_string()), "{names:?}");
            Say::Sse(vec![
                json!({ "choices": [{ "delta": { "tool_calls": [{ "index": 0, "id": "c1", "function": { "name": "recall", "arguments": "{\"query\":\"vault\"}" } }] } }] }).to_string(),
                json!({ "choices": [{ "delta": {}, "finish_reason": "tool_calls" }] }).to_string(),
            ])
        } else {
            Say::Sse(vec![json!({ "choices": [{ "delta": { "content": "Found it." } }] }).to_string(), json!({ "choices": [{ "delta": {}, "finish_reason": "stop" }] }).to_string()])
        }
    });
    let key = d.dir.join("key");
    crate::support::key_file(&key, "sk-test\n");
    fs::write(here.join("connectors/own.toml"), format!("api = 1\n[[connector]]\nid = \"own\"\nurl = \"{}/v1\"\nkey = \"file:{}\"\nmodels = [\"vendor/m\"]\ntools = [\"builtin\", \"memory\"]\n", gateway.url(), key.display())).unwrap();
    let world = open_as(&d, "own", Approval::Full, false);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    let j4n_core::front::Said::Started { task } = world.front.say("main", "where does the staging key live?") else { panic!("no task") };
    let row = world.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).expect("ends");
    assert_eq!(row.answer.as_deref(), Some("Found it."), "{:?}", row.state);
    let seen = gateway.seen();
    let result = seen[1].json()["messages"].as_array().unwrap().iter().find(|m| m["role"] == "tool").unwrap()["content"].as_str().unwrap().replace(['«', '»'], "");
    assert!(result.contains("staging key lives in the vault"), "{result}");
    assert!(!result.contains("private thing"), "what is private is not given to a model: {result}");
    world.close();
    drop(world);

    // without `memory` among its tools, no such function is offered
    let gateway = Mock::start(|seen, _| {
        let names: Vec<String> = seen.json()["tools"].as_array().unwrap().iter().map(|t| t["function"]["name"].as_str().unwrap().to_string()).collect();
        assert!(!names.contains(&"recall".to_string()), "{names:?}");
        Say::Sse(vec![json!({ "choices": [{ "delta": { "content": "ok" } }] }).to_string(), json!({ "choices": [{ "delta": {}, "finish_reason": "stop" }] }).to_string()])
    });
    fs::write(here.join("connectors/own.toml"), format!("api = 1\n[[connector]]\nid = \"own\"\nurl = \"{}/v1\"\nkey = \"file:{}\"\nmodels = [\"vendor/m\"]\n", gateway.url(), key.display())).unwrap();
    let world = open_as(&d, "own", Approval::Full, false);
    let j4n_core::front::Said::Started { task } = world.front.say("main", "hello") else { panic!("no task") };
    world.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).expect("ends");
    world.close();
}
