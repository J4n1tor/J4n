//! A gate rule the Table refused is not dropped. The binary's wiring puts an ask in its place (`Gates::distrust`), so what the rule
//! would have caught is asked, and denied where nobody can answer, and not run. The cases run a model that calls `run` through the
//! wiring of the binary, a gateway of its own and a project folder with one gate file.

use crate::models_cases::mock::{Mock, Say};
use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::gate::grant::here_dir;
use j4n_core::task::Mode;
use serde_json::json;
use std::fs;

/// The line is made by the shell, and its output is not in the line: the words `abc2` are in what the tool says only if it ran.
const LINE: &str = "printf %s abc$((1+1))";

/// A row of the kind `gate` that would block the line, and that the Table refuses (`risk` is a number).
const REFUSED: &str = "api = 1\n[[gate]]\nid = \"mine-blocks-printf\"\non = [\"shell\"]\nwhen = \"cmd matches 'printf*'\"\nverdict = \"block\"\nwhy = \"no printf here\"\nrisk = \"high\"\n";
/// The same row, one the Table reads.
const READ: &str = "api = 1\n[[gate]]\nid = \"mine-blocks-printf\"\non = [\"shell\"]\nwhen = \"cmd matches 'printf*'\"\nverdict = \"block\"\nwhy = \"no printf here\"\n";

/// A model that calls `run` once with `LINE`; what the tool said, the world (closed by the caller) and its folders.
fn run_line(tag: &str, gate: Option<(&str, &str)>) -> (String, World, Dirs) {
    let d = Dirs::new(tag);
    let key = d.dir.join("key");
    crate::support::key_file(&key, "sk-test\n");
    let calls = json!([{ "index": 0, "id": "c0", "function": { "name": "run", "arguments": json!({ "cmd": LINE }).to_string() } }]);
    let gateway = Mock::start(move |_, n| {
        if n == 0 {
            Say::Sse(vec![json!({ "choices": [{ "delta": { "tool_calls": calls } }] }).to_string(), json!({ "choices": [{ "delta": {}, "finish_reason": "tool_calls" }] }).to_string()])
        } else {
            Say::Sse(vec![json!({ "choices": [{ "delta": { "content": "Done." } }] }).to_string(), json!({ "choices": [{ "delta": {}, "finish_reason": "stop" }] }).to_string()])
        }
    });
    let here = here_dir(&d.j4n_home(), &d.project());
    let mut files = vec![("connectors/own.toml", format!("api = 1\n[[connector]]\nid = \"own\"\nurl = \"{}/v1\"\nkey = \"file:{}\"\nmodels = [\"vendor/m\"]\ntools = [\"builtin\"]\n", gateway.url(), key.display()))];
    files.extend(gate.map(|(rel, g)| (rel, g.to_string())));
    for (rel, text) in files {
        fs::create_dir_all(here.join(rel).parent().unwrap()).unwrap();
        fs::write(here.join(rel), text).unwrap();
    }
    let o = Opts { doer: Some("own".into()), model: None, approval: Some(Approval::Deny), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    let world = wire::open(setup, &o, false, "main").unwrap_or_else(|e| panic!("{e}"));
    let j4n_core::front::Said::Started { task } = world.front.say("main", "print it") else { panic!("no task") };
    world.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).expect("ends");
    let seen = gateway.seen();
    let told = seen[1].json()["messages"].as_array().unwrap().iter().filter(|m| m["role"] == "tool").map(|m| m["content"].as_str().unwrap_or_default().to_string()).collect::<Vec<_>>().join("\n");
    (told, world, d)
}

fn verdicts(w: &World) -> Vec<serde_json::Value> {
    w.front.log().sync();
    w.front.log().read(0).filter(|e| e.kind == "gate.verdict").map(|e| e.data.clone()).collect()
}

#[test]
fn a_command_runs_when_no_rule_has_a_say_and_a_rule_that_is_read_blocks_it() {
    let (told, world, _d) = run_line("gate-distrust-none", None);
    assert!(told.contains("abc2"), "{told}");
    world.close();
    let (told, world, _d) = run_line("gate-distrust-read", Some(("gates/mine.toml", READ)));
    assert!(!told.contains("abc2") && told.contains("not allowed"), "{told}");
    assert!(verdicts(&world).iter().any(|v| v["verdict"] == "block" && v["why"].as_str().is_some_and(|w| w.starts_with("mine-blocks-printf"))), "{:?}", verdicts(&world));
    world.close();
}

#[test]
fn a_gate_rule_the_table_refused_asks_in_its_place_and_the_command_it_would_have_caught_is_not_run() {
    let (told, world, _d) = run_line("gate-distrust-refused", Some(("gates/mine.toml", REFUSED)));
    assert!(!told.contains("abc2"), "the command ran: {told}");
    assert!(told.contains("not allowed"), "{told}");
    // the Gate asked, at risk 90 and for the reason the Table gave, and nobody could say yes
    world.front.log().sync();
    let log: Vec<_> = world.front.log().read(0).collect();
    let step = log.iter().find(|e| e.kind == "ask.step" && e.data["answerer"] == "rule:gate").unwrap_or_else(|| panic!("the Gate was not asked: {:?}", log.iter().map(|e| &e.kind).collect::<Vec<_>>()));
    let why = step.data["why"].as_str().unwrap_or_default();
    assert!(why.starts_with("a gate rule was refused") && why.contains("mine-blocks-printf") && why.contains("risk"), "{why}");
    assert!(log.iter().any(|e| e.kind == "task.queued" && e.data["spec"]["input"]["ask"]["risk"] == 90), "the ask is at risk 90");
    assert!(log.iter().any(|e| e.kind == "ask.answered" && e.data["answer"] == "no"), "denied where nobody answers");
    // the start said it was refused
    let said = world.notices.join("\n");
    assert!(said.contains("mine-blocks-printf") && said.contains("risk"), "{said}");
    world.close();
}

#[test]
fn a_file_refused_whole_whose_name_does_not_say_gate_asks_too_for_what_it_held_cannot_be_known() {
    // a syntax error in a file that holds the project's gate rows: the Table never read them
    let (told, world, _d) = run_line("gate-distrust-whole", Some(("rules/mine.toml", "api = 1\n[[gate]\nid = \"mine-blocks-printf\"\n")));
    assert!(!told.contains("abc2") && told.contains("not allowed"), "the command ran: {told}");
    let said = world.notices.join("\n");
    assert!(said.contains("rules/mine.toml") && said.contains("not TOML"), "{said}");
    world.close();
}
