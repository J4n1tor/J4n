//! Dialect `acp`: an agent that speaks the Agent Client Protocol, played by a shell script that knows nothing of J4n. The
//! manifest is the whole of the connection.

use crate::bench::{Bench, connectors};
use crate::world::{Scratch, script};
use j4n_core::fold::Fold;
use j4n_core::gate::check::Gates;
use j4n_core::gate::grant::{Grants, Scope};
use j4n_core::gate::{Action, Gate, Verb};
use j4n_core::log::Ev;
use j4n_core::change::Actor;
use j4n_core::task::{Ctl, Row, SessionRef, State, TaskSpec, Why};

const AGENT: &str = include_str!("../../../../fixtures/connectors/acp-agent.sh");
const MANIFEST: &str = include_str!("../../../../fixtures/connectors/acp-agent.toml");

fn bench() -> Bench {
    Bench::new(&[("acp-agent", AGENT)], MANIFEST)
}

fn with_env(extra: &str) -> Bench {
    Bench::new(&[("acp-agent", AGENT)], &MANIFEST.replace("env = { ", &format!("env = {{ {extra}, ")))
}

fn failed(row: &Row) -> String {
    match &row.state {
        State::Stopped { why: Why::Failed { message } } => message.clone(),
        other => panic!("not a failed run: {other:?}\n{row:?}"),
    }
}

fn texts(b: &Bench, kind: &str, key: &str) -> Vec<String> {
    b.events(kind).iter().map(|e| e.data[key].as_str().unwrap_or_default().to_string()).collect()
}

fn sent(b: &Bench) -> Vec<String> {
    b.bin.read("log").lines().map(String::from).collect()
}

#[test]
fn a_turn_streams_what_the_agent_says_and_ends_with_it_as_the_answer() {
    let b = bench();
    let row = b.run("acpagent", "hello there");
    assert_eq!((row.state.clone(), row.answer.as_deref()), (State::Done { ok: true }, Some("Hello world")), "{row:?}\n{}", b.dump());
    assert_eq!(row.session, Some(SessionRef { doer: "acpagent".into(), id: "sess-1".into() }));
    let log = sent(&b);
    assert_eq!(log[0], "start --acp");
    assert!(log[1].contains("\"method\":\"initialize\"") && log[1].contains("\"protocolVersion\":1") && log[1].contains("\"readTextFile\":false"), "{}", log[1]);
    assert!(log[2].contains("\"method\":\"session/new\"") && log[2].contains("\"mcpServers\":[]"), "{}", log[2]);
    assert!(log[3].contains("\"method\":\"session/prompt\"") && log[3].contains("\"text\":\"hello there\"") && log[3].contains("\"sessionId\":\"sess-1\""), "{}", log[3]);
}

#[test]
fn a_permission_is_a_question_and_the_option_the_answer_names_goes_back() {
    let b = bench();
    let id = b.spawn("acpagent", "ask me");
    let ask = b.waiting(&id);
    let open = b.tasks.book().open_asks();
    assert_eq!(open.len(), 1, "{}", b.dump());
    b.answer(&ask, j4n_core::ask::Answer::Yes);
    let row = b.end(&id);
    assert_eq!(row.answer.as_deref(), Some("It built."), "{}", b.dump());
    assert!(sent(&b).iter().any(|l| l.contains("\"id\":900") && l.contains("\"optionId\":\"allow\"") && l.contains("\"outcome\":\"selected\"")), "{:?}", sent(&b));
    assert_eq!(texts(&b, "run.reasoning", "text"), Vec::<String>::new(), "reasoning streams and is not written");
    let shell = b.events("run.shell");
    assert_eq!((shell[0].data["cmd"].as_str(), shell[0].data["out"].as_str(), shell[0].data["exit"].as_i64()), (Some("make build"), Some("built"), Some(0)));
    assert_eq!(texts(&b, "run.message", "text"), ["Let me run it.", "It built."], "the words before the tool stay narration");
    assert_eq!(row.spent, 10_000, "the cost it reported, as the sum of what each report added");
}

#[test]
fn a_no_goes_back_as_the_reject_option_and_the_agent_goes_on() {
    let b = bench();
    let id = b.spawn("acpagent", "ask me");
    let ask = b.waiting(&id);
    b.answer(&ask, j4n_core::ask::Answer::No);
    let row = b.end(&id);
    assert_eq!(row.answer.as_deref(), Some("It was not allowed."));
    assert!(sent(&b).iter().any(|l| l.contains("\"optionId\":\"deny\"")), "{:?}", sent(&b));
}

#[test]
fn a_permission_the_gate_blocks_is_refused_without_the_operator() {
    let b = bench();
    let row = b.run("acpagent", "push it");
    assert_eq!(row.answer.as_deref(), Some("It was not allowed."), "{}", b.dump());
    assert_eq!(b.events("ask.answered").iter().filter(|e| e.data["by"] == "rule:gate").count(), 1, "{}", b.dump());
}

#[test]
fn an_edit_is_a_change_and_the_plan_left_is_the_work_left() {
    let b = bench();
    let row = b.run("acpagent", "edit a file");
    assert_eq!(row.answer.as_deref(), Some("Edited."));
    assert_eq!(row.todo_left, 1);
    let changed = b.events("run.changed");
    assert_eq!(changed[0].data["paths"][0], "a.txt");
    assert_eq!(b.events("run.tool_result").len(), 1, "a tool ends once, on its final status: {}", b.dump());
}

#[test]
fn a_stop_reason_that_is_not_an_end_is_a_failure_in_words() {
    let b = bench();
    assert!(failed(&b.run("acpagent", "limit it")).contains("token limit"));
    assert!(failed(&b.run("acpagent", "refuse it")).contains("refused"));
}

#[test]
fn a_session_goes_on_in_the_same_child_and_an_abort_cancels_the_turn_and_keeps_the_child() {
    let b = bench();
    let first = b.spawn("acpagent", "hang on");
    for _ in 0..2000 {
        if b.events("run.session").len() == 1 && sent(&b).iter().any(|l| l.contains("session/prompt")) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    b.tasks.ctl(&first, Ctl::Abort, Actor::Operator).unwrap();
    let row = b.end(&first);
    assert!(matches!(row.state, State::Stopped { why: Why::Aborted }), "{row:?}");
    let mut spec = TaskSpec::new("hello again", "acpagent");
    spec.resume = Some(SessionRef { doer: "acpagent".into(), id: "sess-1".into() });
    let second = b.tasks.spawn(spec, Actor::Operator).unwrap();
    let row = b.end(&second);
    assert_eq!(row.answer.as_deref(), Some("Hello world"), "{}", b.dump());
    let log = sent(&b);
    assert_eq!(log.iter().filter(|l| l.starts_with("start")).count(), 1, "one child served both: {log:?}");
    assert!(log.iter().any(|l| l.contains("session/cancel")), "{log:?}");
    assert_eq!(log.iter().filter(|l| l.contains("session/prompt")).count(), 2);
}

#[test]
fn a_session_whose_child_is_gone_is_loaded_in_a_new_one() {
    let b = bench();
    let mut spec = TaskSpec::new("hello", "acpagent");
    spec.resume = Some(SessionRef { doer: "acpagent".into(), id: "sess-1".into() });
    let row = b.end(&b.tasks.spawn(spec, Actor::Operator).unwrap());
    assert_eq!(row.answer.as_deref(), Some("Hello world"), "{}", b.dump());
    let log = sent(&b);
    assert!(log.iter().any(|l| l.contains("\"method\":\"session/load\"") && l.contains("\"sessionId\":\"sess-1\"")), "{log:?}");
    assert!(!log.iter().any(|l| l.contains("session/new")), "{log:?}");
    assert!(b.events("run.message").iter().all(|e| !e.data["text"].as_str().unwrap_or_default().contains("old line")), "the replayed history is not the chat");
}

#[test]
fn an_agent_that_cannot_load_a_session_starts_a_new_one_and_says_so() {
    let b = with_env("ACP_NOLOAD = \"1\"");
    let mut spec = TaskSpec::new("hello", "acpagent");
    spec.resume = Some(SessionRef { doer: "acpagent".into(), id: "old".into() });
    let row = b.end(&b.tasks.spawn(spec, Actor::Operator).unwrap());
    assert_eq!(row.answer.as_deref(), Some("Hello world"));
    assert!(texts(&b, "run.notice", "text").iter().any(|t| t.contains("new one")), "{}", b.dump());
}

#[test]
fn an_agent_that_wants_a_login_says_how_and_a_dead_one_fails_the_turn_with_what_it_wrote() {
    let b = with_env("ACP_AUTH = \"1\", ACP_NOLOAD = \"1\"");
    let why = failed(&b.run("acpagent", "hello"));
    assert!(why.contains("authentication required") && why.contains("Sign in with a browser"), "{why}");
    let b = bench();
    let why = failed(&b.run("acpagent", "die now"));
    assert!(why.contains("exit 7") && why.contains("the agent fell over"), "{why}");
}

#[test]
fn a_method_the_agent_asks_of_the_client_is_refused_not_ignored() {
    let b = bench();
    let row = b.run("acpagent", "fs please");
    assert_eq!(row.answer.as_deref(), Some("done"), "{}", b.dump());
    assert!(sent(&b).iter().any(|l| l.contains("\"id\":77") && l.contains("\"error\"") && l.contains("-32601")), "{:?}", sent(&b));
}

/// The effect the Gate was asked about, from the question's task.
fn asked(b: &Bench) -> serde_json::Value {
    let q = b.events("task.queued").into_iter().find(|e| e.data["spec"]["doer"] == "human").unwrap_or_else(|| panic!("no question\n{}", b.dump()));
    q.data["spec"]["input"]["ask"]["act"].clone()
}

#[test]
fn a_permission_to_edit_is_a_write_with_its_place_for_the_gate() {
    let b = bench();
    b.run("acpagent", "permedit a.txt");
    let act = asked(&b);
    assert_eq!((act["verb"].as_str(), act["target"].as_str(), act["inside"].as_bool()), (Some("write"), Some("a.txt"), Some(true)), "{act}");
    let b = bench();
    b.run("acpagent", "permedit /etc/hosts");
    let act = asked(&b);
    assert_eq!((act["verb"].as_str(), act["inside"].as_bool()), (Some("write"), Some(false)), "{act}");
}

fn questions(b: &Bench) -> Vec<serde_json::Value> {
    b.events("task.queued").into_iter().filter(|e| e.data["spec"]["doer"] == "human").map(|e| e.data["spec"]["input"]["ask"]["act"].clone()).collect()
}

/// The bench with a Gate of the test's own (a clone is kept), as the binary holds one, and the grants it starts with.
fn gated_full(grants: Grants, full: bool) -> (Bench, Gate) {
    let bin = Scratch::new("bin");
    script(&bin, "acp-agent", AGENT);
    let (list, said, tables) = connectors(&bin, MANIFEST);
    assert!(said.is_empty(), "{said:?}");
    let (doers, said) = list.doers(&|_, r| Err(format!("no secret `{r}`")));
    assert!(said.is_empty(), "{said:?}");
    let (gates, _) = Gates::load(&tables);
    let gate = Gate::new(gates, grants);
    (Bench::gated_full(bin, doers, &tables, gate.clone(), full, |_| {}), gate)
}

fn granted(verb: Verb, target: &str) -> Grants {
    let g = Grants::new("s1");
    let a = Action::new(verb, Actor::Doer("acpagent".into()), target);
    let e = g.event(&Actor::Operator, Scope::Session, &a).expect("a grant");
    Fold::apply(&mut g.clone(), &Ev { seq: 1, at_ms: 1, kind: e.kind.clone(), task: None, thread: None, by: e.by.clone(), cause: None, class: Default::default(), data: e.data.clone() });
    g
}

fn action_of(act: &serde_json::Value) -> Action {
    serde_json::from_value(act.clone()).unwrap_or_else(|e| panic!("{e}: {act}"))
}

fn answered(b: &Bench) -> Vec<(String, String)> {
    b.events("ask.answered").iter().map(|e| (e.data["answer"].as_str().unwrap_or_default().to_string(), e.data["by"].as_str().unwrap_or_default().to_string())).collect()
}

#[test]
fn a_folder_ask_for_an_edit_outside_is_a_write_of_that_file_that_full_does_not_answer() {
    // opencode's rule for a folder outside the project (`kind: other`, `filepath`, `parentDir`) is the only ask before an edit it allows: the call's own `tool_call` says
    // the tool is an `edit` (and a call that was not seen is taken for one), so the Gate is asked about a WRITE of the file
    for words in ["folder edit /outside-root/f.txt", "folder none /outside-root/f.txt"] {
        let (b, gate) = gated_full(Grants::new("s1"), true);
        b.run("acpagent", words);
        let act = asked(&b);
        assert_eq!((act["verb"].as_str(), act["target"].as_str(), act["inside"].as_bool()), (Some("write"), Some("/outside-root/f.txt"), Some(false)), "{words}: {act}");
        assert!(gate.outside(&action_of(&act)));
        // under full, with nobody to ask, the no stands, and goes back to opencode as a reject
        let said = answered(&b);
        assert_eq!(said.len(), 1, "{words}: {}", b.dump());
        assert_eq!(said[0].0, "no", "{words}: {said:?}");
        assert_ne!(said[0].1, "rule:approval-full", "{words}");
        assert!(sent(&b).iter().any(|l| l.contains("\"id\":920") && l.contains("\"optionId\":\"deny\"")), "{words}: {:?}", sent(&b));
    }
}

#[test]
fn a_folder_ask_for_an_edit_in_a_folder_the_operator_granted_is_a_yes_under_full() {
    let (b, gate) = gated_full(granted(Verb::Write, "/outside-root"), true);
    b.run("acpagent", "folder edit /outside-root/sub/f.txt");
    let act = asked(&b);
    assert_eq!((act["verb"].as_str(), act["inside"].as_bool()), (Some("write"), Some(false)), "the doer's own look is the project: {act}");
    assert!(!gate.outside(&action_of(&act)), "the grant makes it inside");
    // the grant itself answers it now (it is read where no rule asks), so `full` has nothing left to answer
    assert_eq!(answered(&b), [("yes".to_string(), "rule:gate".to_string())], "{}", b.dump());
    assert!(sent(&b).iter().any(|l| l.contains("\"id\":920") && l.contains("\"optionId\":\"allow\"")), "{:?}", sent(&b));
    let mut beside = Action::new(Verb::Write, Actor::Doer("acpagent".into()), "/outside-t556-beside/f.txt");
    beside.inside = Some(false);
    assert!(gate.outside(&beside), "a folder beside the granted one is not covered");
}

#[test]
fn a_folder_ask_for_a_read_outside_is_a_read_as_it_was_and_full_answers_it() {
    let (b, gate) = gated_full(Grants::new("s1"), true);
    b.run("acpagent", "folder read /etc/hostname");
    let act = asked(&b);
    assert_eq!((act["verb"].as_str(), act["target"].as_str(), act["inside"].as_bool()), (Some("read"), Some("/etc/hostname"), Some(false)), "{act}");
    assert!(!gate.outside(&action_of(&act)));
    assert_eq!(answered(&b), [("yes".to_string(), "rule:approval-full".to_string())], "{}", b.dump());
}

#[test]
fn the_second_ask_of_one_tool_call_about_the_same_line_is_answered_as_the_first_and_another_line_is_asked() {
    // opencode asks about a command twice under one call id (its folder rule, then the command): the line is answered once and the second ask
    // is answered the same, unasked, and a different line of the call is asked. The first line is one the Gate allows by itself, so a
    // machine answers it a moment after it is opened; whoever answers, the second ask of it is not put again, and the operator is only
    // asked of the line the Gate leaves to them
    let b = bench();
    let id = b.spawn("acpagent", "twice please");
    let ask = b.waiting(&id);
    b.answer(&ask, j4n_core::ask::Answer::No);
    b.end(&id);
    let lines: Vec<String> = questions(&b).iter().map(|a| a["target"].as_str().unwrap_or_default().to_string()).collect();
    assert_eq!(lines, ["ls /etc", "rm -rf x"], "{}", b.dump());
    let log = sent(&b);
    let said = |n: u32, option: &str| log.iter().any(|l| l.contains(&format!("\"id\":{n}")) && l.contains(&format!("\"optionId\":\"{option}\"")));
    assert!(said(910, "allow") && said(911, "allow") && said(912, "deny"), "{log:?}");
}

#[test]
fn a_shell_call_that_begins_as_bash_is_the_command_its_ending_update_titles() {
    let b = bench();
    b.run("acpagent", "titled now");
    let shell = b.events("run.shell");
    assert_eq!((shell[0].data["cmd"].as_str(), shell[0].data["out"].as_str()), (Some("ls -la"), Some("total 0")), "{}", b.dump());
}

#[test]
fn a_turn_that_ends_silent_after_a_no_is_told_what_was_refused_and_goes_on_to_an_answer() {
    // opencode ends the turn when a request is answered no: the run is not left without an answer
    let b = bench();
    let id = b.spawn("acpagent", "rejectend now");
    let ask = b.waiting(&id);
    b.answer(&ask, j4n_core::ask::Answer::No);
    let row = b.end(&id);
    assert_eq!(row.answer.as_deref(), Some("Carried on."), "{}", b.dump());
    let prompts: Vec<String> = sent(&b).into_iter().filter(|l| l.contains("session/prompt")).collect();
    assert_eq!(prompts.len(), 2, "{prompts:?}");
    assert!(prompts[1].contains("Not allowed here: `printf x > ~/x`") && prompts[1].contains("Do not try it again"), "{}", prompts[1]);
}
