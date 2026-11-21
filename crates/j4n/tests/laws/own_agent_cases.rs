//! Through the binary's own wiring: the own agent (a model with the builtin tools) against a gateway on the loopback that
//! answers as a script says, in a project folder. What is checked is the loop J4n runs, not the model: a call is run and its result
//! goes back as a tool message, turn 2 holds turn 1's tool result, an empty reply is asked again once, a call cut
//! off is refused as that and a JSON string is decoded, forty-five steps finish, the reasoning goes back, every
//! call of a reply runs in order and through the Gate, and a path inside the root is that path.

use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::front::Said;
use j4n_core::task::{Row, State};
use crate::models_cases::mock::{Mock, Say, Seen};
use serde_json::{Value, json};
use std::fs;
use std::time::Duration;

fn open(d: &Dirs, gateway: &Mock, approval: Approval) -> World {
    open_with(d, gateway, approval, "")
}

/// [`open`], with more lines for the operator's row of `own` (`tools = [...]` to narrow what it is given).
fn open_with(d: &Dirs, gateway: &Mock, approval: Approval, more: &str) -> World {
    let key = d.dir.join("key");
    crate::support::key_file(&key, "sk-test-own-0001\n");
    fs::create_dir_all(d.j4n_home().join("connectors")).unwrap();
    // the pack `agent` carries the row `own`; the operator's file says where the gateway is and where the key is
    let mine = format!("api = 1\n[[connector]]\nid = \"own\"\nurl = \"{}/v1\"\nkey = \"file:{}\"\nmodels = [\"vendor/m\"]\n{more}", gateway.url(), key.display());
    fs::write(d.j4n_home().join("connectors/mine.toml"), mine).unwrap();
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    let opts = Opts { doer: Some("own".into()), model: None, approval: Some(approval), wall: None, usd: None, mode: j4n_core::task::Mode::Write, place: None, shape: false, anywhere: false };
    let w = wire::open(setup, &opts, false, "main").unwrap_or_else(|e| panic!("opens: {e}"));
    assert!(!w.notices.iter().any(|n| n.contains("own")), "{:?}", w.notices);
    w
}

fn frames(text: &str, think: Option<&str>, calls: &[(&str, &str, &str)], finish: &str) -> Say {
    let mut out = Vec::new();
    if let Some(t) = think {
        out.push(json!({ "choices": [{ "delta": { "reasoning": t } }] }).to_string());
    }
    if !text.is_empty() {
        out.push(json!({ "choices": [{ "delta": { "content": text } }] }).to_string());
    }
    if !calls.is_empty() {
        let frags: Vec<Value> = calls.iter().enumerate().map(|(i, (id, name, args))| json!({ "index": i, "id": id, "function": { "name": name, "arguments": args } })).collect();
        out.push(json!({ "choices": [{ "delta": { "tool_calls": frags } }] }).to_string());
    }
    out.push(json!({ "choices": [{ "delta": {}, "finish_reason": finish }] }).to_string());
    out.push(json!({ "choices": [], "provider": "Cerebras", "usage": { "prompt_tokens": 100, "completion_tokens": 10, "prompt_tokens_details": { "cached_tokens": 40 } } }).to_string());
    Say::Sse(out)
}

fn answer(text: &str) -> Say {
    frames(text, None, &[], "stop")
}

fn call(calls: &[(&str, &str, &str)]) -> Say {
    frames("", Some("I need to look."), calls, "tool_calls")
}

fn line(w: &World, text: &str) -> Row {
    let Said::Started { task } = w.front.say("main", text) else { panic!("{text:?} did not start a task") };
    w.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap_or_else(|| panic!("{task} never ended\n{}", dump(w)))
}

fn dump(w: &World) -> String {
    w.front.log().read(0).map(|e| format!("{:>4} {:<16} {} {}", e.seq, e.kind, e.task.as_deref().unwrap_or("-"), e.data)).collect::<Vec<_>>().join("\n")
}

fn events(w: &World, kind: &str) -> Vec<Value> {
    w.front.log().read(0).filter(|e| e.kind == kind).map(|e| e.data).collect()
}

fn messages(s: &Seen) -> Vec<Value> {
    s.json()["messages"].as_array().cloned().unwrap_or_default()
}

fn tool_results(s: &Seen) -> Vec<(String, String)> {
    messages(s).iter().filter(|m| m["role"] == "tool").map(|m| (m["tool_call_id"].as_str().unwrap_or_default().to_string(), m["content"].as_str().unwrap_or_default().to_string())).collect()
}

fn done(r: &Row) -> bool {
    matches!(r.state, State::Done { ok: true })
}

#[test]
fn a_native_tool_call_runs_through_the_real_loop_and_its_result_goes_back_as_a_tool_message() {
    let d = Dirs::new("own-agent-native-tool-call");
    d.put("notes.txt", "alpha\nbeta\n");
    let gateway = Mock::start(|_, n| if n == 0 { call(&[("c1", "read", "{\"path\":\"notes.txt\"}")]) } else { answer("It says alpha and beta.") });
    let w = open(&d, &gateway, Approval::Deny);
    let row = line(&w, "what is in notes.txt?");
    assert!(done(&row), "{:?}\n{}", row.state, dump(&w));
    assert_eq!(row.answer.as_deref(), Some("It says alpha and beta."));
    let seen = gateway.seen();
    assert_eq!(seen.len(), 2);
    let first = seen[0].json();
    let names: Vec<&str> = first["tools"].as_array().unwrap().iter().map(|t| t["function"]["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["list", "read", "grep", "edit", "write", "run", "git", "ask", "web"], "the same tools on every turn");
    let results = tool_results(&seen[1]);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, "c1");
    assert!(results[0].1.contains("    1  alpha") && results[0].1.contains("    2  beta"), "{}", results[0].1);
    // the Log has the call and its result, as events
    let (started, ended) = (events(&w, "run.tool_call"), events(&w, "run.tool_result"));
    assert!(started.iter().any(|t| t["name"] == "read") && ended.iter().any(|t| t["name"] == "read" && t["detail"].as_str().unwrap().contains("alpha")), "{started:?} {ended:?}");
    w.close();
}

#[test]
fn a_command_that_was_connected_as_a_tool_is_offered_to_the_model_and_runs_with_its_arguments_as_words() {
    let d = Dirs::new("own-command");
    let gateway = Mock::start(|_, n| if n == 0 { call(&[("c1", "shout", "{\"word\":\"plum; echo no\"}")]) } else { answer("It shouted.") });
    fs::create_dir_all(d.j4n_home().join("connectors")).unwrap();
    // a terminal command, said in data: its argument is one word of the command line, whatever it holds
    fs::write(
        d.j4n_home().join("connectors/shout.toml"),
        "api = 1\n[[connector]]\nid = \"shout\"\ndoc = \"Say a word in capitals.\"\nis = \"tool\"\ndialect = \"text\"\nprogram = \"sh\"\nargs = [{ name = \"word\", required = true }]\nargv = [\"sh\", \"-c\", \"echo \\\"$1\\\" | tr a-z A-Z\", \"sh\", \"{word}\"]\nrisk = \"read\"\n",
    )
    .unwrap();
    let w = open(&d, &gateway, Approval::Full);
    let row = line(&w, "shout plum");
    assert!(done(&row), "{:?}\n{}", row.state, dump(&w));
    let seen = gateway.seen();
    let first = seen[0].json();
    let names: Vec<&str> = first["tools"].as_array().unwrap().iter().map(|t| t["function"]["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["list", "read", "grep", "edit", "write", "run", "git", "shout", "ask", "web"], "{names:?}");
    let shout = first["tools"].as_array().unwrap().iter().find(|t| t["function"]["name"] == "shout").unwrap();
    assert_eq!(shout["function"]["parameters"]["required"], json!(["word"]));
    let results = tool_results(&seen[1]);
    assert_eq!(results[0].1.trim(), "PLUM; ECHO NO", "the value is a word of the command line, not syntax: {results:?}");
    w.close();
}

#[test]
fn the_second_line_of_the_thread_holds_the_first_lines_tool_result() {
    let d = Dirs::new("own-agent-second-line-thread");
    d.put("a.txt", "the secret word is plum\n");
    let gateway = Mock::start(|_, n| match n {
        0 => call(&[("c1", "read", "{\"path\":\"a.txt\"}")]),
        1 => answer("Read it."),
        _ => answer("Plum."),
    });
    let w = open(&d, &gateway, Approval::Deny);
    assert!(done(&line(&w, "read a.txt")));
    let row = line(&w, "what was the word?");
    assert_eq!(row.answer.as_deref(), Some("Plum."));
    let third = &gateway.seen()[2];
    let msgs = messages(third);
    let roles: Vec<&str> = msgs.iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles, ["system", "user", "assistant", "tool", "assistant", "user"], "{msgs:?}");
    assert!(tool_results(third)[0].1.contains("the secret word is plum"), "the tool result of turn 1 is in the request of turn 2");
    assert_eq!(msgs[5]["content"], "what was the word?", "the line is sent as it was typed");
    w.close();
}

#[test]
fn an_empty_reply_is_asked_again_once_and_a_second_stops_the_task_with_the_tokens() {
    let d = Dirs::new("own-agent-empty-reply-asked");
    let gateway = Mock::start(|_, n| if n == 0 { frames("", None, &[], "stop") } else { answer("Here is the answer.") });
    let w = open(&d, &gateway, Approval::Deny);
    let row = line(&w, "say something");
    assert!(done(&row), "{:?}\n{}", row.state, dump(&w));
    assert_eq!(row.answer.as_deref(), Some("Here is the answer."));
    let again = messages(&gateway.seen()[1]);
    assert_eq!(again.last().unwrap()["content"], "Your last reply was empty. Answer, or call a tool.");
    w.close();

    let d = Dirs::new("own-agent-empty-reply-asked-2");
    let gateway = Mock::start(|_, _| frames("", None, &[], "stop"));
    let w = open(&d, &gateway, Approval::Deny);
    let row = line(&w, "say something");
    assert!(!done(&row), "{:?}", row.state);
    assert_eq!(gateway.seen().len(), 2, "asked again once, and no more");
    let said = format!("{:?}", row.state);
    assert!(said.contains("no content twice") && said.contains("20 tokens"), "{said}");
    w.close();
}

#[test]
fn a_call_cut_off_in_its_json_is_refused_as_that_and_a_json_string_is_decoded() {
    let d = Dirs::new("own-agent-call-cut-off");
    let gateway = Mock::start(|_, n| match n {
        // the reply ran out of room in the middle of the file it was writing
        0 => frames("", None, &[("c1", "write", "{\"path\":\"big.txt\",\"content\":\"abcdef")], "length"),
        // arguments sent as a string that holds the object
        1 => call(&[("c2", "write", &serde_json::to_string(&json!({ "path": "small.txt", "content": "decoded" }).to_string()).unwrap())]),
        _ => answer("Done."),
    });
    let w = open(&d, &gateway, Approval::Deny);
    let row = line(&w, "write two files");
    assert!(done(&row), "{:?}\n{}", row.state, dump(&w));
    let refused = &tool_results(&gateway.seen()[1])[0].1;
    assert!(refused.contains("not valid JSON") && refused.contains("cut off"), "{refused}");
    assert!(!refused.contains("needs `path`"), "never told that a path is missing: {refused}");
    assert!(!d.project().join("big.txt").exists());
    assert_eq!(fs::read_to_string(d.project().join("small.txt")).unwrap(), "decoded");
    w.close();
}

#[test]
fn a_run_of_forty_five_steps_finishes() {
    let d = Dirs::new("own-agent-run-forty-five");
    d.put("x.txt", "x\n");
    let gateway = Mock::start(|_, n| if n < 45 { call(&[("c", "list", &format!("{{\"path\":\".\",\"depth\":{}}}", 1 + n % 3)), ("d", "grep", &format!("{{\"pattern\":\"p{n}\"}}")) ]) } else { answer("Looked at everything.") });
    let w = open(&d, &gateway, Approval::Deny);
    let row = line(&w, "look around");
    assert!(done(&row), "{:?}\n{}", row.state, dump(&w));
    assert_eq!(gateway.seen().len(), 46);
    assert_eq!(row.answer.as_deref(), Some("Looked at everything."));
    w.close();
}

#[test]
fn the_reasoning_that_came_with_a_call_goes_back_with_it() {
    let d = Dirs::new("own-agent-reasoning-came-call");
    d.put("x.txt", "x\n");
    let gateway = Mock::start(|_, n| if n == 0 { call(&[("c1", "list", "{}")]) } else { answer("ok") });
    let w = open(&d, &gateway, Approval::Deny);
    assert!(done(&line(&w, "look")));
    let second = messages(&gateway.seen()[1]);
    let call_message = second.iter().find(|m| m["role"] == "assistant").unwrap();
    assert!(call_message.to_string().contains("I need to look."), "{call_message}");
    // and the Log keeps it with the call, for the thread to be rebuilt from
    assert!(events(&w, "run.reasoning").iter().any(|m| m["text"].as_str().is_some_and(|t| t.contains("I need to look."))));
    w.close();
}

#[test]
fn a_reply_with_three_calls_runs_three_gated_effects_in_order() {
    let d = Dirs::new("own-agent-reply-three-calls");
    let gateway = Mock::start(|_, n| {
        if n == 0 {
            call(&[("c1", "write", "{\"path\":\"a.txt\",\"content\":\"one\"}"), ("c2", "run", "{\"cmd\":\"rm -rf /\"}"), ("c3", "write", "{\"path\":\"c.txt\",\"content\":\"three\"}")])
        } else {
            answer("Tried all three.")
        }
    });
    let w = open(&d, &gateway, Approval::Deny);
    let row = line(&w, "do three things");
    assert!(done(&row), "{:?}\n{}", row.state, dump(&w));
    // the first and the third happened; the second was a wipe, and the Gate does not let it be run, whoever asks
    assert_eq!(fs::read_to_string(d.project().join("a.txt")).unwrap(), "one");
    assert_eq!(fs::read_to_string(d.project().join("c.txt")).unwrap(), "three");
    let results = tool_results(&gateway.seen()[1]);
    assert_eq!(results.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["c1", "c2", "c3"], "in the order they were called");
    assert!(results[0].1.starts_with("made `a.txt`") && results[2].1.starts_with("made `c.txt`"), "{results:?}");
    assert!(results[1].1.contains("was not allowed"), "{results:?}");
    // the verdict of the Gate on the wipe is in the Log
    assert!(events(&w, "gate.verdict").iter().any(|v| v["verb"] == "shell" && v["verdict"] == "block"), "{}", dump(&w));
    w.close();
}

#[test]
fn a_write_makes_its_folders_and_a_path_inside_the_root_is_that_path_while_the_escapes_stay_refused() {
    let d = Dirs::new("own-agent-write-makes-folders");
    let abs = d.project().join("out/abs.md");
    let gateway = Mock::start(move |_, n| {
        if n == 0 {
            call(&[
                ("c1", "write", "{\"path\":\"out/reports/s.md\",\"content\":\"# report\"}"),
                ("c2", "write", &json!({ "path": abs.display().to_string(), "content": "absolute" }).to_string()),
                ("c3", "write", "{\"path\":\"../outside.txt\",\"content\":\"no\"}"),
                ("c4", "write", "{\"path\":\".git/hooks/x\",\"content\":\"no\"}"),
                ("c5", "write", "{\"path\":\".env\",\"content\":\"no\"}"),
            ])
        } else {
            answer("Done.")
        }
    });
    let w = open(&d, &gateway, Approval::Deny);
    let row = line(&w, "write the report");
    assert!(done(&row), "{:?}\n{}", row.state, dump(&w));
    assert_eq!(fs::read_to_string(d.project().join("out/reports/s.md")).unwrap(), "# report");
    assert_eq!(fs::read_to_string(d.project().join("out/abs.md")).unwrap(), "absolute");
    let results = tool_results(&gateway.seen()[1]);
    assert!(results[1].1.contains("`out/abs.md`"), "the absolute path is the relative one: {}", results[1].1);
    for r in &results[2..] {
        assert!(r.1.contains("..") || r.1.contains("not reached by a tool"), "{r:?}");
    }
    assert!(!d.dir.join("project/../outside.txt").exists() && !d.project().join(".env").exists() && !d.project().join(".git/hooks/x").exists());
    w.close();
}

// ---- the turn-path ratchet: the third line of a thread, through the real binary ----

/// What a token costs when it is counted the way the Brief counts (four characters).
fn tokens(text: &str) -> u64 {
    j4n_core::brief::tokens(text) as u64
}

#[test]
fn the_third_line_of_a_thread_is_measured_against_the_turn_path_ceilings() {
    let d = Dirs::new("turnpath");
    d.offload_off();
    d.put("README.md", "# small\n");
    let gateway = Mock::start(|_, _| answer("ok"));
    let w = open(&d, &gateway, Approval::Deny);
    for l in ["add a --verbose flag", "run the tests", "continue"] {
        assert!(done(&line(&w, l)), "{l}\n{}", dump(&w));
    }
    let log: Vec<_> = w.front.log().read(0).collect();
    // the Brief the third line was given
    let rendered: Vec<_> = log.iter().filter(|e| e.kind == "brief.rendered").collect();
    assert_eq!(rendered.len(), 3);
    let third = &rendered[2].data;
    let (brief_tokens, brief_sections) = (third["tokens"].as_u64().unwrap(), third["sections"].as_array().unwrap().len() as u64);
    // the questions raised between the line and the start of its task
    let said = log.iter().rposition(|e| e.kind == "chat.say").unwrap();
    let started = log.iter().rposition(|e| e.kind == "task.started" && e.data["doer"] == "own").unwrap();
    let decisions = log[said..started].iter().filter(|e| e.kind.starts_with("ask.") || e.kind == "intake.decided").count() as u64;
    // what every call carries whatever is said: the system prompt (the Brief is beside it, and counted above) and the tool schemas
    let req = gateway.seen()[2].json();
    let system = req["messages"][0]["content"].as_str().unwrap();
    let own_system = system.split("\n\n## ").next().unwrap();
    let fixed = tokens(own_system) + tokens(&req["tools"].to_string());
    let bad = crate::turn_path::judged(&[("brief_tokens", brief_tokens), ("brief_sections", brief_sections), ("decision_rows", decisions), ("fixed_context_tokens", fixed)]);
    assert!(bad.is_empty(), "measured: brief {brief_tokens} tokens in {brief_sections} section(s), {decisions} decision row(s), {fixed} tokens of fixed context\n{bad:#?}");
    // and the fixed context stays under 1,500 tokens
    assert!(fixed <= 1500, "{fixed}");
    w.close();
}

/// The numbers of the third line of a thread, with a plan open (`plan`) or not: Brief tokens and sections, decision rows, fixed context.
fn third_line(plan: bool) -> (u64, u64, u64, u64) {
    let d = Dirs::new(if plan { "turnpath_plan" } else { "turnpath_none" });
    d.offload_off();
    d.put("README.md", "# small\n");
    let gateway = Mock::start(|_, _| answer("ok"));
    let w = open(&d, &gateway, Approval::Deny);
    for l in ["add a --verbose flag", "run the tests"] {
        assert!(done(&line(&w, l)), "{l}\n{}", dump(&w));
    }
    if plan {
        let answer = json!({ "approach": "Read the cli, add the flag, test it.", "items": [
            { "id": "read", "goal": "read cli.rs", "reads": ["src/cli.rs"], "depends": [], "done": "known" },
            { "id": "flag", "goal": "add the flag", "writes": ["src/cli.rs"], "depends": ["read"], "done": "it parses" },
            { "id": "test", "goal": "test it", "writes": ["tests/cli.rs"], "depends": ["flag"], "done": "green" }] });
        for op in j4n_core::keeper::take(&answer.to_string(), None, j4n_core::keeper::How::Begin, "go").unwrap() {
            let (kind, data) = op.event("t0");
            w.front.log().append(j4n_core::log::New::of(kind, j4n_core::change::Actor::Core, data).thread("main")).unwrap();
        }
    }
    assert!(done(&line(&w, "continue")), "{}", dump(&w));
    let log: Vec<_> = w.front.log().read(0).collect();
    let third = &log.iter().filter(|e| e.kind == "brief.rendered").nth(2).unwrap().data;
    let (tokens_, sections) = (third["tokens"].as_u64().unwrap(), third["sections"].as_array().unwrap().len() as u64);
    let said = log.iter().rposition(|e| e.kind == "chat.say").unwrap();
    let started = log.iter().rposition(|e| e.kind == "task.started" && e.data["doer"] == "own").unwrap();
    let decisions = log[said..started].iter().filter(|e| e.kind.starts_with("ask.") || e.kind == "intake.decided").count() as u64;
    let req = gateway.seen()[2].json();
    let system = req["messages"][0]["content"].as_str().unwrap();
    let fixed = tokens(system.split("\n\n## ").next().unwrap()) + tokens(&req["tools"].to_string());
    w.close();
    (tokens_, sections, decisions, fixed)
}

#[test]
fn the_turn_path_is_green_with_a_plan_open_too_and_the_plan_costs_one_brief_line() {
    let (none, with) = (third_line(false), third_line(true));
    let bad = crate::turn_path::judged(&[("brief_tokens", none.0), ("brief_sections", none.1), ("decision_rows", none.2), ("fixed_context_tokens", none.3)]);
    assert!(bad.is_empty(), "{none:?}\n{bad:#?}");
    assert_eq!(with.1, none.1 + 1, "one more section: the plan line");
    assert!((1..=64).contains(&(with.0 - none.0)), "the plan line is at most 60 tokens and its heading: {} more", with.0 - none.0);
    assert!(with.2 <= none.2, "the fold raises no decision row: {with:?} against {none:?}");
    assert_eq!(with.3, none.3, "the fixed context does not move");
}

// ---- `ask`, `web` and the turn-path ceiling ----

/// Answer the next question that is open, as the operator would from a button; the question and its shape come back.
fn reply(w: &World, with: j4n_proto::Reply) -> (String, j4n_core::ask::AskShape) {
    let end = std::time::Instant::now() + crate::support::PATIENCE;
    loop {
        if let Some(a) = w.front.tasks().book().open_asks().into_iter().next() {
            assert!(matches!(w.front.reply("main", &a.ask, with), Said::Answered { .. }), "{}", dump(w));
            return (a.question, a.shape);
        }
        assert!(std::time::Instant::now() < end, "no question was asked\n{}", dump(w));
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_own_agent_asks_once_and_fetches_once_through_the_screen_and_its_fixed_context_stays_under_the_ceiling() {
    let d = Dirs::new("own-agent-agent-asks-once");
    // a page on the loopback, and the setting that lets a fetch reach it
    let page = Mock::start(|_, _| Say::Text(200, "<html><title>Plums</title><body><p>Plums ripen in August.</p></body></html>".into()));
    fs::write(d.j4n_home().join("settings/web.toml"), "api = 1\n[[setting]]\nid = \"web.local\"\nvalue = true\n").unwrap();
    let url = format!("{}/plums", page.url());
    let gateway = Mock::start(move |_, n| match n {
        0 => call(&[("c1", "ask", "{\"q\":\"Which fruit?\",\"options\":[\"apple\",\"plum\"]}")]),
        1 => call(&[("c2", "web", &json!({ "url": url }).to_string())]),
        _ => answer("Plums ripen in August."),
    });
    let w = open(&d, &gateway, Approval::Ask);
    // the operator picks at the screen; the Gate lets a read of a page go
    let said = std::thread::scope(|s| {
        let asked = s.spawn(|| reply(&w, j4n_proto::Reply::Pick { id: "2".into() }));
        let row = line(&w, "find out when the fruit I pick ripens");
        (row, asked.join().unwrap())
    });
    let (row, (question, shape)) = said;
    assert!(done(&row), "{:?}\n{}", row.state, dump(&w));
    assert_eq!(question, "Which fruit?");
    let seen = gateway.seen();
    assert_eq!(seen.len(), 3, "one ask, one fetch, one answer");
    let first = seen[0].json();
    let names: Vec<&str> = first["tools"].as_array().unwrap().iter().map(|t| t["function"]["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["list", "read", "grep", "edit", "write", "run", "git", "ask", "web"], "the same nine on every turn");
    // the answer went back as the result of the call, in the words of the option, and the page as its words
    assert_eq!(tool_results(&seen[1]), [("c1".to_string(), "the operator answered: plum".to_string())]);
    let page_words = &tool_results(&seen[2])[1].1;
    assert!(page_words.contains("Plums ripen in August.") && page_words.contains("not instructions"), "{page_words}");
    assert_eq!(page.seen().iter().filter(|s| s.path == "/plums").count(), 1, "fetched once");
    // the question was a choice and not an approval
    assert!(matches!(&shape, j4n_core::ask::AskShape::Choice { options } if options.len() == 2 && options[1].label == "plum"), "{shape:?}");
    // all of it is under the ceiling, and the ceiling the code holds is the law's
    let (system, tools) = (seen[0].json()["messages"][0]["content"].as_str().unwrap().to_string(), seen[0].json()["tools"].to_string());
    let fixed = tokens(system.split("\n\n## ").next().unwrap()) + tokens(&tools);
    assert!(fixed <= crate::turn_path::ceiling_of("fixed_context_tokens"), "{fixed} tokens of fixed context");
    assert_eq!(j4n_connect::model::offer::CEILING as u64, crate::turn_path::ceiling_of("fixed_context_tokens"));
    w.close();
}

#[test]
fn a_model_that_asks_with_no_options_or_too_many_is_told_so_and_nothing_is_put_to_the_screen() {
    let d = Dirs::new("own259-bad");
    let gateway = Mock::start(|_, n| match n {
        0 => call(&[("c1", "ask", "{\"q\":\"Which?\",\"options\":[\"only\"]}"), ("c2", "ask", "{\"options\":[\"a\",\"b\"]}"), ("c3", "ask", "{\"q\":\"x\",\"options\":[\"1\",\"2\",\"3\",\"4\",\"5\",\"6\",\"7\"]}")]),
        _ => answer("never mind"),
    });
    let w = open(&d, &gateway, Approval::Deny);
    assert!(done(&line(&w, "pick something")));
    let results = tool_results(&gateway.seen()[1]);
    assert_eq!(results.len(), 3);
    assert!(results[0].1.contains("2 to 6 `options`") && results[1].1.contains("needs a question") && results[2].1.contains("2 to 6 `options`"), "{results:?}");
    assert!(w.front.tasks().book().open_asks().is_empty());
    w.close();
}

#[test]
fn a_call_of_ask_is_not_put_to_the_screen_when_the_model_was_not_given_it() {
    let d = Dirs::new("own259-narrow");
    let gateway = Mock::start(|_, n| if n == 0 { call(&[("c1", "ask", "{\"q\":\"Which?\",\"options\":[\"a\",\"b\"]}")]) } else { answer("ok") });
    let w = open_with(&d, &gateway, Approval::Deny, "tools = [\"builtin\"]\n");
    assert!(done(&line(&w, "pick something")));
    let seen = gateway.seen();
    assert!(seen[0].json()["tools"].as_array().unwrap().iter().all(|t| t["function"]["name"] != "ask" && t["function"]["name"] != "web"));
    assert!(tool_results(&seen[1])[0].1.contains("no tool `ask`"), "{:?}", tool_results(&seen[1]));
    assert!(w.front.tasks().book().open_asks().is_empty());
    w.close();
}
