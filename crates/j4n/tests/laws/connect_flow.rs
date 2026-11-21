//! A program J4n has never seen is connected through the Connect flow with no change to J4n — it is looked at,
//! a doer drafts how to speak to it into the closed schema, the draft is checked, tried once, and proposed with what the canary
//! said; a yes (twice, as it adds a program to run) writes the connector, and after a restart the program is a doer. The front,
//! the loop, the Gate, the box walls, the rules and the files are the real ones, wired as the binary wires them; the program is a
//! script, the drafter is a script, and only the model that would write the draft is not there.

use crate::screen_drive::{Operator, asked, choosing, flat};
use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::gate::grant::here_dir;
use j4n_core::task::Mode;
use serde_json::json;
use j4n_tui::headless::{KeyCode, alt, press};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const FLOW: &str = include_str!("../../../../packs/connect/flows/connect.toml");

fn opts() -> Opts {
    Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Ask), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false }
}

fn open(d: &Dirs) -> World {
    open_as(d, &opts(), true)
}

fn open_as(d: &Dirs, opts: &Opts, interactive: bool) -> World {
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    crate::support::opened(|| wire::open(setup.clone(), opts, interactive, "main"))
}

fn script(d: &Dirs, name: &str, body: &str) -> PathBuf {
    let p = d.dir.join("bin").join(name);
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn put(dir: &std::path::Path, rel: &str, text: &str) {
    let p = dir.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

fn held(d: &Dirs, file: &str) -> String {
    fs::read_to_string(here_dir(&d.j4n_home(), &d.project()).join(file)).unwrap_or_default()
}

/// The operator's folders: the pack on, no walls where a container has none, and a drafter that is a script (the model that would
/// write the draft), put into the flow in place of the election.
fn furnish(d: &Dirs, draft: &str) -> PathBuf {
    let drafter = script(d, "drafter", &format!("cat <<'EOF'\n{draft}\nEOF"));
    put(&d.j4n_home(), "harness.toml", "api = 1\n[[enable]]\nid = \"connect\"\n");
    let here = here_dir(&d.j4n_home(), &d.project());
    put(&here, "settings.toml", "api = 1\n[[setting]]\nid = \"box.unavailable\"\nvalue = \"here\"\n");
    put(&here, "connectors/drafter.toml", &format!("api = 1\n[[connector]]\nid = \"drafter\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"{}\"\nstart = [\"{{prompt}}\"]\n", drafter.display()));
    put(&here, "flows/connect.toml", &FLOW.replace("do = \"who:work\"", "do = \"drafter\""));
    drafter
}

const AGENT: &str = r#"case "$1" in
  --version) echo "newagent 1.2.3" ;;
  --help) echo "newagent - a small agent"; echo "Usage: newagent run [--json] PROMPT"; echo "  --json   print one JSON event per line (jsonl)" ;;
  run) echo '{"type":"text","text":"ready"}'; echo '{"type":"result","session_id":"s1"}' ;;
  *) echo "unknown: $1" >&2; exit 2 ;;
esac"#;

fn manifest(program: &str) -> String {
    format!(
        "Here it is.\n```toml\n[[connector]]\nid = \"newagent\"\ndoc = \"A small agent that prints JSON lines.\"\nis = \"agent\"\ndialect = \"jsonl-map\"\nprogram = \"{program}\"\nstart = [\"run\", \"--json\", \"{{prompt}}\"]\nprobe = {{ argv = [\"{program}\", \"--version\"] }}\n\n[connector.map]\nsession = \"/session_id\"\nend = \"/type == 'result'\"\nevents = [{{ when = \"/type == 'text'\", emit = \"text\", text = \"/text\" }}]\n```"
    )
}

#[test]
fn a_script_agent_that_nobody_wrote_a_manifest_for_is_connected_through_the_flow_and_is_a_doer_after_a_restart() {
    let d = Dirs::new("connect-agent");
    let agent = script(&d, "newagent", AGENT);
    let program = agent.display().to_string();
    furnish(&d, &manifest(&program));
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    let mut op = Operator::sit(&world);

    op.type_line(&format!("/connect {program}"));
    // it asks leave to look at the program, once
    let s = op.until("the leave to look", |s| asked(s) && s.contains("connect:"));
    assert!(s.contains("newagent") && s.contains("connect:"), "{s}");
    op.press(vec![alt('y')]);
    // its help says it prints JSON lines: the rows cannot tell whether that is an agent or a tool, so the operator is asked
    let s = op.until("the question", choosing);
    assert!(s.contains("an agent you talk with") && s.contains("⇥"), "{s}");
    op.press(vec![press(KeyCode::Char('1'))]);
    // J4n tries the program a few ways to see how it takes a prompt: it asks leave for that too, and learns nothing of a program that
    // answers every prompt with the same word
    op.until("the leave to try it a few ways", |s| asked(s) && flat(s).contains("a few ways"));
    op.press(vec![alt('y')]);
    // the draft is checked, and the connector is tried once: that too is asked
    op.until("the leave to try it", |s| asked(s) && flat(s).contains("once, to see that it works"));
    op.press(vec![alt('y')]);
    let s = op.until("the proposal", |s| s.contains("proposed p"));
    assert!(s.contains("connect newagent"), "{s}");
    assert_eq!(held(&d, "connectors/newagent.toml"), "", "the flow wrote nothing");

    // adding a program to run widens what J4n may do: two presses
    op.type_line(":accept");
    op.until("what it widens", |s| s.contains("widens what J4n may do"));
    assert_eq!(held(&d, "connectors/newagent.toml"), "");
    op.type_line(":accept");
    op.until("the yes", |s| s.contains("accepted"));
    let file = held(&d, "connectors/newagent.toml");
    assert!(file.contains("jsonl-map") && file.contains("/session_id") && file.contains(&program), "{file}");

    // J4n is started again: the program is a doer, and it answers
    drop(op);
    drop(world);
    let world = open(&d);
    assert!(world.notices.is_empty(), "the row it wrote is a row the Table reads: {:?}", world.notices);
    let mut op = Operator::sit(&world);
    op.type_line(":switch newagent");
    op.type_line("say it");
    op.until("its word", |s| s.contains("ready"));
}

/// Say yes to every leave that is asked, until `done` shows.
fn yes_until(op: &mut Operator, what: &str, done: impl Fn(&str) -> bool) -> String {
    loop {
        let s = op.until(what, |s| done(s) || asked(s));
        if done(&s) {
            return s;
        }
        op.press(vec![alt('y')]);
        std::thread::sleep(Duration::from_millis(150));
    }
}

const COMMAND: &str = r#"case "$1" in
  --version) echo "lister 0.1" ;;
  --help) echo "Usage: lister WORD"; echo "Prints the word it is given, after 'found: '." ;;
  *) echo "found: $1" ;;
esac"#;

#[test]
fn a_terminal_command_is_connected_through_the_same_flow_as_a_tool_and_is_asked_before_each_run() {
    let d = Dirs::new("connect-command");
    let command = script(&d, "lister", COMMAND);
    let program = command.display().to_string();
    let draft = format!(
        "```toml\n[[connector]]\nid = \"lister\"\ndoc = \"Print a word after 'found: '.\"\nis = \"tool\"\ndialect = \"text\"\nprogram = \"{program}\"\nargs = [{{ name = \"word\", required = true }}]\nargv = [\"{program}\", \"{{word}}\"]\nrisk = \"read\"\ncanary = {{ goal = \"plum\", expect = \"found: plum\" }}\n```"
    );
    furnish(&d, &draft);
    let world = open(&d);
    let mut op = Operator::sit(&world);
    op.type_line(&format!("/connect {program}"));
    op.until("the leave to look", asked);
    op.press(vec![alt('y')]);
    // a plain program: agent or tool? the rows recommend the tool
    let s = op.until("the question", choosing);
    assert!(s.lines().any(|l| l.contains("⇥ 2 ") && l.contains("tool")), "{s}");
    op.press(vec![press(KeyCode::Char('2'))]);
    // it is tried once, with a leave for the connecting and one for the run itself
    let s = yes_until(&mut op, "the proposal", |s| s.contains("proposed p"));
    assert!(s.contains("connect lister"), "{s}");
    op.type_line(":accept");
    op.until("what it widens", |s| s.contains("widens what J4n may do"));
    op.type_line(":accept");
    op.until("the yes", |s| s.contains("accepted"));
    let file = held(&d, "connectors/lister.toml");
    assert!(file.contains("is = \"tool\"") && file.contains("found: plum") && file.contains("risk = \"read\""), "{file}");
    drop(op);
    drop(world);
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
}

#[test]
fn a_draft_that_is_wrong_is_sent_back_with_what_is_wrong_and_one_that_names_another_program_never_gets_to_run() {
    let d = Dirs::new("connect-wrong");
    let agent = script(&d, "newagent", AGENT);
    let program = agent.display().to_string();
    // the drafter names a program of its own choosing
    let draft = manifest("/bin/sh");
    furnish(&d, &draft);
    let world = open(&d);
    let mut op = Operator::sit(&world);
    op.type_line(&format!("/connect {program}"));
    op.until("the leave to look", asked);
    op.press(vec![alt('y')]);
    op.until("the question", choosing);
    op.press(vec![press(KeyCode::Char('1'))]);
    op.until("the leave to try it a few ways", |s| asked(s) && flat(s).contains("a few ways"));
    op.press(vec![alt('y')]);
    // each lane that folded says its last words under its head, so the flow's own lane, the first, is above the screen when it ends: the end is
    // read from the Log, and the ledger row under the lanes says the flow failed
    let end = Instant::now() + crate::support::PATIENCE;
    while !world.front.log().read(0).any(|e| e.data.to_string().contains("still not right")) {
        assert!(Instant::now() < end, "never saw the end of the flow: {}", op.look());
        std::thread::sleep(Duration::from_millis(20));
    }
    let s = op.until("the end", |s| s.contains("needs you: t1"));
    assert!(s.contains("runs that program and no other"), "{s}");
    assert!(!s.contains("once, to see"), "the canary never ran: {s}");
    assert_eq!(held(&d, "connectors/newagent.toml"), "");
    assert_eq!(world.front.log().read(0).filter(|e| e.kind == "proposal.made").count(), 0);
}

/// The MCP server of the next test. The test binary is started again as the server (`J4N_FAKE_MCP` set); run as a test it does
/// nothing.
#[test]
fn fake_mcp_server() {
    use std::io::{BufRead, Write};
    if std::env::var("J4N_FAKE_MCP").is_err() {
        return;
    }
    let mut out = std::io::stdout();
    // the test harness has written "test … ... " on this line already
    let _ = writeln!(out);
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(doc) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
        let (Some(id), Some(method)) = (doc.get("id").cloned(), doc["method"].as_str()) else { continue };
        let result = match method {
            "initialize" => json!({ "protocolVersion": "2025-03-26", "capabilities": { "tools": {} }, "serverInfo": { "name": "notes" } }),
            "tools/list" => json!({ "tools": [
                { "name": "get_note", "description": "Read a note by its title.", "inputSchema": { "type": "object", "properties": { "title": { "type": "string" } }, "required": ["title"] }, "annotations": { "readOnlyHint": true } },
                { "name": "add_note", "description": "Write a note.", "inputSchema": { "type": "object", "properties": { "title": { "type": "string" } }, "required": ["title"] } },
            ] }),
            "tools/call" => json!({ "content": [{ "type": "text", "text": format!("the note {}", doc["params"]["arguments"]["title"]) }] }),
            _ => json!({}),
        };
        let _ = writeln!(out, "{}", json!({ "jsonrpc": "2.0", "id": id, "result": result }));
        let _ = out.flush();
    }
}

#[test]
fn an_mcp_server_is_connected_through_the_flow_with_its_tools_pinned_by_the_same_yes_and_a_model_then_calls_one() {
    use crate::models_cases::mock::{Mock, Say};
    let d = Dirs::new("connect-mcp");
    let exe = std::env::current_exe().unwrap().display().to_string();
    let program = script(
        &d,
        "notes-server",
        &format!("case \"$1\" in\n  --version) echo \"notes-server 0.3\" ;;\n  --help) echo \"notes-server - an MCP server\"; echo \"  serve   speak the Model Context Protocol on stdio\" ;;\n  serve) exec env J4N_FAKE_MCP=1 '{exe}' --exact connect_flow::fake_mcp_server --nocapture --test-threads 1 ;;\nesac"),
    )
    .display()
    .to_string();
    let draft = format!("```toml\n[[connector]]\nid = \"notes\"\ndoc = \"A notes server.\"\nis = \"mcp\"\ndialect = \"mcp\"\nprogram = \"{program}\"\nstart = [\"serve\"]\n```");
    furnish(&d, &draft);
    let world = open(&d);
    let mut op = Operator::sit(&world);
    op.type_line(&format!("/connect {program}"));
    // the sniff rows say it is an MCP server: nothing to choose, only leave to look and leave to start it
    let s = yes_until(&mut op, "the proposal", |s| s.contains("proposed p"));
    assert!(s.contains("connect notes"), "{s}");
    op.type_line(":accept");
    op.until("what it widens", |s| s.contains("widens what J4n may do"));
    op.type_line(":accept");
    op.until("the yes", |s| s.contains("accepted"));
    let file = held(&d, "connectors/notes.toml");
    assert!(file.contains("dialect = \"mcp\"") && file.contains("pins = ") && file.contains("get_note") && file.contains("add_note"), "the server and the hash of each tool it offered are written together: {file}");
    drop(op);
    drop(world);

    // a model that is given the server calls one of its tools; the unpinned world would have offered none
    let gateway = Mock::start(|seen, n| {
        if n == 0 {
            let names: Vec<String> = seen.json()["tools"].as_array().unwrap().iter().map(|t| t["function"]["name"].as_str().unwrap().to_string()).collect();
            assert!(names.contains(&"notes__get_note".to_string()) && names.contains(&"notes__add_note".to_string()), "{names:?}");
            Say::Sse(vec![
                json!({ "choices": [{ "delta": { "tool_calls": [{ "index": 0, "id": "c1", "function": { "name": "notes__get_note", "arguments": "{\"title\":\"plums\"}" } }] } }] }).to_string(),
                json!({ "choices": [{ "delta": {}, "finish_reason": "tool_calls" }] }).to_string(),
            ])
        } else {
            Say::Sse(vec![json!({ "choices": [{ "delta": { "content": "Read it." } }] }).to_string(), json!({ "choices": [{ "delta": {}, "finish_reason": "stop" }] }).to_string()])
        }
    });
    let key = d.dir.join("key");
    crate::support::key_file(&key, "sk-test\n");
    let here = here_dir(&d.j4n_home(), &d.project());
    put(&here, "connectors/own.toml", &format!("api = 1\n[[connector]]\nid = \"own\"\nurl = \"{}/v1\"\nkey = \"file:{}\"\nmodels = [\"vendor/m\"]\ntools = [\"builtin\", \"*\"]\n", gateway.url(), key.display()));
    let o = Opts { doer: Some("own".into()), model: None, approval: Some(Approval::Full), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let world = open_as(&d, &o, false);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    let j4n_core::front::Said::Started { task } = world.front.say("main", "what is in the plums note?") else { panic!("no task") };
    let row = world.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).expect("ends");
    assert_eq!(row.answer.as_deref(), Some("Read it."), "{:?}", row.state);
    let seen = gateway.seen();
    let result = seen[1].json()["messages"].as_array().unwrap().iter().find(|m| m["role"] == "tool").unwrap()["content"].as_str().unwrap().to_string();
    assert_eq!(result, "the note \"plums\"");
    world.close();
}

#[test]
fn tools_a_server_offers_that_nobody_pinned_are_not_offered_and_are_proposed_for_a_yes() {
    use crate::models_cases::mock::{Mock, Say};
    let d = Dirs::new("connect-pin");
    let exe = std::env::current_exe().unwrap().display().to_string();
    let program = script(&d, "notes-server", &format!("exec env J4N_FAKE_MCP=1 '{exe}' --exact connect_flow::fake_mcp_server --nocapture --test-threads 1")).display().to_string();
    let key = d.dir.join("key");
    crate::support::key_file(&key, "sk-test\n");
    let here = here_dir(&d.j4n_home(), &d.project());
    put(&here, "connectors/notes.toml", &format!("api = 1\n[[connector]]\nid = \"notes\"\nis = \"mcp\"\ndialect = \"mcp\"\nprogram = \"{program}\"\n"));
    let gateway = Mock::start(|_, _| Say::Sse(vec![json!({ "choices": [{ "delta": { "content": "Nothing to call." } }] }).to_string(), json!({ "choices": [{ "delta": {}, "finish_reason": "stop" }] }).to_string()]));
    put(&here, "connectors/own.toml", &format!("api = 1\n[[connector]]\nid = \"own\"\nurl = \"{}/v1\"\nkey = \"file:{}\"\nmodels = [\"vendor/m\"]\ntools = [\"builtin\", \"notes\"]\n", gateway.url(), key.display()));
    let o = Opts { doer: Some("own".into()), model: None, approval: Some(Approval::Full), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let world = open_as(&d, &o, false);
    let j4n_core::front::Said::Started { task } = world.front.say("main", "hello") else { panic!("no task") };
    world.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).expect("ends");
    let names: Vec<String> = gateway.seen()[0].json()["tools"].as_array().unwrap().iter().map(|t| t["function"]["name"].as_str().unwrap().to_string()).collect();
    assert!(!names.iter().any(|n| n.starts_with("notes__")), "an unpinned tool is not offered: {names:?}");
    // the server's tools were found, and the operator is asked to pin them
    let end = Instant::now() + crate::support::PATIENCE;
    let made = loop {
        world.front.log().sync();
        let made: Vec<_> = world.front.log().read(0).filter(|e| e.kind == "proposal.made").collect();
        if !made.is_empty() || Instant::now() > end {
            break made;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(made.len(), 1);
    assert!(made[0].data["draft"]["title"].as_str().unwrap().contains("Pin 2 tool(s) of notes"), "{}", made[0].data);
    assert!(!held(&d, "connectors/notes.toml").contains("pins"), "a proposal writes nothing");
    // and what the server offers is kept as the baseline a later change is held to
    let sets: Vec<_> = world.front.log().read(0).filter(|e| e.kind == "watch.set").collect();
    assert_eq!(sets.len(), 1, "{sets:?}");
    assert_eq!(sets[0].data["of"], "mcp:notes");
    assert_eq!(sets[0].data["set"].as_object().unwrap().keys().collect::<Vec<_>>(), ["add_note", "get_note"]);
    world.close();
}
