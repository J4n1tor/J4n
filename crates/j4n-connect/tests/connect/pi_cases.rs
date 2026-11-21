//! Dialect `pi-rpc`: the frames of `pi` and `omp` (real captures where there are any, `fixtures/pi-frames`), and live sessions
//! played by a child that prints recorded frames.

use crate::bench::Bench;
use j4n_connect::manifest::Family;
use j4n_connect::pi::frames::{Translator, approval_title, ends_turn};
use j4n_core::ask::Answer;
use j4n_core::change::Actor;
use j4n_core::doer::RunEv;
use j4n_core::task::{Ctl, Row, SessionRef, State, TaskSpec, Why};
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pi-frames").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Everything the translator makes of a capture, and the session and model it learned.
fn replay(name: &str) -> (Vec<RunEv>, Option<String>, Option<String>) {
    let (mut evs, mut session, mut model) = (Vec::new(), None, None);
    let mut tr = Translator::new();
    for line in fixture(name).lines().filter(|l| !l.starts_with('#')) {
        let out = tr.line(line);
        session = out.session.or(session);
        model = out.model.or(model);
        evs.extend(out.evs);
    }
    (evs, session, model)
}

fn joined(evs: &[RunEv], pick: fn(&RunEv) -> Option<&str>) -> String {
    evs.iter().filter_map(pick).collect()
}

fn reasoning(e: &RunEv) -> Option<&str> {
    match e {
        RunEv::Reasoning(t) => Some(t),
        _ => None,
    }
}

fn text(e: &RunEv) -> Option<&str> {
    match e {
        RunEv::Text(t) => Some(t),
        _ => None,
    }
}

fn no_errors(evs: &[RunEv]) {
    assert!(!evs.iter().any(|e| matches!(e, RunEv::Error(_))), "{evs:?}");
}

#[test]
fn the_pi_transcript_replays_as_a_turn_with_its_shell_line_its_words_and_its_cost() {
    let (evs, session, _) = replay("pi-json-mode.jsonl");
    assert_eq!(joined(&evs, reasoning), "Compare decimals: 0.9 > 0.11.", "the settled thinking is not said twice");
    assert_eq!(joined(&evs, text), "9.9 is larger.");
    assert!(evs.iter().any(|e| matches!(e, RunEv::Shell { cmd, out, exit: 0 } if cmd == "cat a.txt" && out.starts_with("hello"))), "{evs:?}");
    assert!(evs.iter().any(|e| matches!(e, RunEv::Message { text, .. } if text == "9.9 is larger.")));
    assert!(evs.iter().any(|e| matches!(e, RunEv::Metrics(m) if m.tokens_in > 0)));
    assert!(evs.iter().any(|e| matches!(e, RunEv::Tool { name, end: false, .. } if name == "bash")));
    no_errors(&evs);
    assert_eq!(session.as_deref(), Some("00000000-0000-7000-8000-000000000001"));
}

#[test]
fn the_real_omp_capture_replays() {
    let (evs, session, model) = replay("omp-json-mode.jsonl");
    assert!(joined(&evs, reasoning).contains("9.9 > 9.11"));
    assert!(!joined(&evs, text).is_empty());
    assert!(evs.iter().any(|e| matches!(e, RunEv::Shell { cmd, exit: 0, .. } if cmd == "cat a.txt")));
    assert!(evs.iter().any(|e| matches!(e, RunEv::Tool { name, end: true, detail } if name == "write" && !detail.starts_with("failed"))));
    assert!(evs.iter().any(|e| matches!(e, RunEv::Changed(p) if !p.is_empty())), "a write that went well is a change: {evs:?}");
    assert!(session.is_some());
    assert!(model.is_some_and(|m| m.contains("glm")));
    no_errors(&evs);
}

/// The frames of a turn whose provider key is dead (402): the refusal is an error carrying the provider's sentence, said once
/// though two frames carry it, and nothing makes up a reply.
#[test]
fn a_real_refused_turn_is_one_honest_error() {
    let (evs, session, _) = replay("omp-rpc-refused.jsonl");
    let errors: Vec<&String> = evs.iter().filter_map(|e| if let RunEv::Error(m) = e { Some(m) } else { None }).collect();
    assert_eq!(errors.len(), 1, "{evs:?}");
    assert!(errors[0].contains("402 Insufficient credits"), "{errors:?}");
    assert!(joined(&evs, text).is_empty(), "a refused turn streams no text");
    assert_eq!(session, None, "the frames carry no session id: the command layer reads it");
}

#[test]
fn a_real_completed_omp_turn_replays_as_a_reply_with_its_cache_and_cost() {
    let (evs, _, _) = replay("omp-rpc-ok.jsonl");
    assert!(evs.iter().any(|e| matches!(e, RunEv::Reasoning(_))));
    assert!(evs.iter().any(|e| matches!(e, RunEv::Message { .. })));
    assert!(evs.iter().any(|e| matches!(e, RunEv::Metrics(m) if m.tokens_in > 0)));
    no_errors(&evs);
}

#[test]
fn the_handshake_and_a_refusal_of_pi_are_not_turn_events() {
    let (evs, session, model) = replay("pi-rpc.jsonl");
    assert!(evs.is_empty(), "{evs:?}");
    assert_eq!((session, model), (None, None));
}

#[test]
fn a_line_that_is_not_json_is_a_notice_and_the_two_families_end_their_turns_on_their_own_frames() {
    let out = Translator::new().line("ERROR something broke");
    assert!(matches!(&out.evs[..], [RunEv::Notice(t)] if t.contains("something broke")));
    let v = |t: &str| serde_json::json!({ "type": t });
    assert!(ends_turn(Family::Pi, &v("agent_settled")) && !ends_turn(Family::Pi, &v("agent_end")), "an agent_end can still be followed by retries");
    assert!(ends_turn(Family::Omp, &v("prompt_result")) && ends_turn(Family::Omp, &v("session_settled")) && !ends_turn(Family::Omp, &v("agent_settled")));
}

#[test]
fn an_errored_message_is_an_error_and_a_bash_end_carries_the_command_of_its_start() {
    let line = r#"{"type":"message_end","message":{"role":"assistant","content":[],"stopReason":"error","errorMessage":"402 Insufficient credits","usage":{"input":0,"output":0,"totalTokens":0}}}"#;
    let out = Translator::new().line(line);
    assert!(matches!(&out.evs[..], [RunEv::Error(m)] if m.contains("402")), "{:?}", out.evs);
    let mut tr = Translator::new();
    tr.line(r#"{"type":"tool_execution_start","toolCallId":"b1","toolName":"bash","args":{"command":"cargo build"}}"#);
    let end = tr.line(r#"{"type":"tool_execution_end","toolCallId":"b1","toolName":"bash","result":{"content":[{"type":"text","text":"error: boom"}]},"isError":true}"#);
    assert!(matches!(&end.evs[..], [RunEv::Shell { cmd, out, exit: 1 }] if cmd == "cargo build" && out == "error: boom"), "{:?}", end.evs);
}

#[test]
fn the_title_of_an_approval_dialog_names_the_tool_and_what_it_acts_on() {
    assert_eq!(approval_title("Allow tool: write\nPath: hello.txt\nContent:\nhi"), Some(("write".into(), "hello.txt".into())));
    assert_eq!(approval_title("Allow tool: bash\nCommand: ls -la"), Some(("bash".into(), "ls -la".into())));
    assert_eq!(approval_title("Pick a colour"), None);
}

// ---- live sessions ----

const FAKE: &str = include_str!("../../../../fixtures/pi-frames/fake-pi.sh");

fn manifest(family: &str, turn: &str, extra_env: &str) -> String {
    let turn = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pi-frames").join(turn);
    format!(
        r#"
api = 1
[[connector]]
id = "{family}"
is = "agent"
dialect = "pi-rpc"
family = "{family}"
program = "{{bin}}/fake-pi"
probe = {{ argv = ["{{bin}}/fake-pi", "--version"] }}
start = ["--mode", "rpc"]
resume = ["--session", "{{session}}"]
model = ["--model", "{{model}}"]
oneshot = ["-p", "--mode", "json", "{{prompt}}"]
modes = {{ read = ["--tools", "read"] }}
env = {{ FAKE_TURN = "{}", FAKE_LOG = "{{bin}}/log", FAKE_FAMILY = "{family}"{extra_env} }}
"#,
        turn.display()
    )
}

fn pi(extra_env: &str) -> Bench {
    Bench::new(&[("fake-pi", FAKE)], &manifest("pi", "pi-turn.jsonl", extra_env))
}

fn omp(extra_env: &str) -> Bench {
    Bench::new(&[("fake-pi", FAKE)], &manifest("omp", "omp-turn.jsonl", extra_env))
}

fn sent(b: &Bench) -> Vec<String> {
    b.bin.read("log").lines().map(String::from).collect()
}

fn failed(row: &Row) -> String {
    match &row.state {
        State::Stopped { why: Why::Failed { message } } => message.clone(),
        other => panic!("not a failed run: {other:?}\n{row:?}"),
    }
}

fn wait_for(what: &str, b: &Bench, ready: impl Fn() -> bool) {
    for _ in 0..4000 {
        if ready() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("never: {what}\n{}\n{:?}", b.dump(), sent(b));
}

#[test]
fn a_pi_turn_runs_live_and_its_session_is_named_by_the_child() {
    let b = pi("");
    let row = b.run("pi", "which is larger?");
    assert_eq!((row.state.clone(), row.answer.as_deref()), (State::Done { ok: true }, Some("9.9 is larger.")), "{row:?}\n{}", b.dump());
    assert_eq!(row.session, Some(SessionRef { doer: "pi".into(), id: "pi-sess-1".into() }));
    let log = sent(&b);
    assert_eq!(log[0], "start --mode rpc", "no model, no read flags");
    assert!(log[1].contains("get_session_stats"), "pi sends no ready frame: its answer is the proof: {log:?}");
    assert!(log[2].contains("\"type\":\"prompt\"") && log[2].contains("which is larger?"), "{log:?}");
    assert_eq!(b.events("run.shell").len(), 1);
    assert!(b.events("run.tool_call").len() == 1);
    assert_eq!(row.spent, row.spent.max(1), "the cost is counted");
}

#[test]
fn the_second_turn_of_a_session_runs_in_the_same_child_and_a_model_or_a_read_task_is_a_different_one() {
    let b = pi("");
    let first = b.run("pi", "one");
    let mut spec = TaskSpec::new("two", "pi");
    spec.resume = first.session.clone();
    let second = b.end(&b.tasks.spawn(spec, Actor::Operator).unwrap());
    assert_eq!(second.answer.as_deref(), Some("9.9 is larger."));
    assert_eq!(sent(&b).iter().filter(|l| l.starts_with("start")).count(), 1, "{:?}", sent(&b));
    let mut spec = TaskSpec::new("three", "pi");
    spec.resume = first.session;
    spec.tune.model = Some("glm".into());
    spec.tune.mode = j4n_core::task::Mode::Read;
    b.end(&b.tasks.spawn(spec, Actor::Operator).unwrap());
    let starts: Vec<String> = sent(&b).into_iter().filter(|l| l.starts_with("start")).collect();
    assert_eq!(starts, ["start --mode rpc", "start --mode rpc --model glm --session pi-sess-1 --tools read"], "settings are fixed at launch: another child, on the transcript");
}

#[test]
fn a_refused_prompt_fails_the_turn_with_the_programs_words() {
    let b = pi(", FAKE_REFUSE = \"1\"");
    assert!(failed(&b.run("pi", "hello")).contains("No API key found"));
}

#[test]
fn a_steer_reaches_the_turn_that_is_going_on_and_an_abort_ends_it_and_keeps_the_child() {
    let b = pi("");
    let id = b.spawn("pi", "hang on");
    wait_for("the turn to start", &b, || sent(&b).iter().any(|l| l.contains("\"type\":\"prompt\"")));
    b.tasks.ctl(&id, Ctl::Steer("faster".into()), Actor::Operator).unwrap();
    wait_for("the steer to arrive", &b, || sent(&b).iter().any(|l| l.contains("\"type\":\"steer\"") && l.contains("faster")));
    b.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
    let row = b.end(&id);
    assert!(matches!(row.state, State::Stopped { why: Why::Aborted }), "{row:?}");
    wait_for("the abort to arrive", &b, || sent(&b).iter().any(|l| l.contains("\"type\":\"abort\"")));
    let mut spec = TaskSpec::new("again", "pi");
    spec.resume = row.session;
    let second = b.end(&b.tasks.spawn(spec, Actor::Operator).unwrap());
    assert_eq!(second.answer.as_deref(), Some("9.9 is larger."), "{}", b.dump());
    assert_eq!(sent(&b).iter().filter(|l| l.starts_with("start")).count(), 1, "the child survived the abort");
}

#[test]
fn a_program_that_will_not_speak_rpc_runs_the_turn_once_and_says_what_it_lost() {
    let b = pi(", FAKE_RPC = \"0\"");
    let row = b.run("pi", "hello");
    assert_eq!((row.state.clone(), row.answer.as_deref()), (State::Done { ok: true }, Some("9.9 is larger.")), "{row:?}\n{}", b.dump());
    let notices: Vec<String> = b.events("run.notice").iter().map(|e| e.data["text"].as_str().unwrap_or_default().to_string()).collect();
    assert!(notices.iter().any(|n| n.contains("runs once")), "{notices:?}");
    assert!(sent(&b).iter().any(|l| l.starts_with("start -p --mode json")), "{:?}", sent(&b));
}

#[test]
fn omp_says_ready_first_is_asked_for_the_events_it_needs_and_ends_its_turn_on_its_own_frames() {
    let b = omp("");
    let row = b.run("omp", "say hi");
    assert_eq!((row.state.clone(), row.answer.as_deref()), (State::Done { ok: true }, Some("omp says hi")), "{row:?}\n{}", b.dump());
    let log = sent(&b);
    assert!(log[1].contains("set_event_filter") && log[1].contains("tool_execution_end"), "{log:?}");
    assert!(log[2].contains("get_session_stats"), "{log:?}");
    assert_eq!(row.spent, 1200);
}

#[test]
fn an_omp_approval_dialog_is_a_question_for_the_gate_and_the_operator_and_the_answer_goes_in_band() {
    let b = omp(", FAKE_DIALOG = \"1\"");
    let id = b.spawn("omp", "build it");
    let ask = b.waiting(&id);
    assert_eq!(b.tasks.book().open_asks().len(), 1);
    b.answer(&ask, Answer::Yes);
    let row = b.end(&id);
    assert_eq!(row.answer.as_deref(), Some("omp says hi"), "{}", b.dump());
    assert!(sent(&b).iter().any(|l| l.contains("extension_ui_response") && l.contains("\"id\":\"ui1\"") && l.contains("\"value\":\"Approve\"")), "{:?}", sent(&b));
    let b = omp(", FAKE_DIALOG = \"1\"");
    let id = b.spawn("omp", "build it");
    let ask = b.waiting(&id);
    b.answer(&ask, Answer::No);
    b.end(&id);
    assert!(sent(&b).iter().any(|l| l.contains("\"value\":\"Deny\"")), "{:?}", sent(&b));
}

#[test]
fn a_dialog_pi_sends_is_cancelled_at_once_for_nobody_could_answer_it() {
    let b = pi(", FAKE_DIALOG = \"1\"");
    let row = b.run("pi", "build it");
    assert!(sent(&b).iter().any(|l| l.contains("extension_ui_response") && l.contains("\"cancelled\":true")), "{:?}", sent(&b));
    assert!(matches!(row.state, State::Done { .. }), "{row:?}");
}

#[test]
fn an_approval_that_comes_when_no_turn_is_going_on_is_denied_and_never_waved_through() {
    let b = omp(", FAKE_LATE = \"1\"");
    let row = b.run("omp", "say hi");
    assert_eq!(row.answer.as_deref(), Some("omp says hi"));
    wait_for("the late dialog to be answered", &b, || sent(&b).iter().any(|l| l.contains("\"id\":\"late1\"")));
    let reply = sent(&b).into_iter().find(|l| l.contains("\"id\":\"late1\"")).unwrap();
    assert!(reply.contains("\"value\":\"Deny\""), "{reply}");
}

#[test]
fn a_line_the_program_took_for_one_of_its_own_commands_ran_no_turn_and_the_run_says_so() {
    let b = pi(", FAKE_NOAGENT = \"1\"");
    assert!(failed(&b.run("pi", "/help")).contains("ran no turn"));
}

#[test]
fn a_write_that_failed_is_no_change_and_one_that_went_well_is() {
    let mut tr = Translator::new();
    tr.line(r#"{"type":"tool_execution_start","toolCallId":"w1","toolName":"write","args":{"path":"a.txt","content":"x"}}"#);
    let bad = tr.line(r#"{"type":"tool_execution_end","toolCallId":"w1","toolName":"write","result":{"content":[{"type":"text","text":"denied"}]},"isError":true}"#);
    assert!(bad.evs.iter().all(|e| !matches!(e, RunEv::Changed(_))), "{:?}", bad.evs);
    tr.line(r#"{"type":"tool_execution_start","toolCallId":"w2","toolName":"edit","args":{"path":"b.txt"}}"#);
    let good = tr.line(r#"{"type":"tool_execution_end","toolCallId":"w2","toolName":"edit","result":{"content":[{"type":"text","text":"ok"}]},"isError":false}"#);
    assert!(good.evs.iter().any(|e| matches!(e, RunEv::Changed(p) if p == &["b.txt".to_string()])), "{:?}", good.evs);
}

// ---- the window and shrinking a session ----

fn shrunk(b: &Bench, first: &Row, notes: Vec<String>) -> Row {
    let mut spec = TaskSpec::new("carry on", "pi");
    spec.resume = first.session.clone();
    spec.follows = Some(first.id.clone());
    spec.compact = Some(notes);
    b.end(&b.tasks.spawn(spec, Actor::Operator).unwrap())
}

#[test]
fn the_context_window_is_read_when_a_turn_ends_and_the_task_settles_with_it() {
    let b = pi(", FAKE_CTX = \"720\", FAKE_WINDOW = \"1000\"");
    let row = b.run("pi", "hello");
    assert_eq!(row.state, State::Done { ok: true }, "{}", b.dump());
    let settled = &b.events("task.settled")[0].data;
    assert_eq!(settled["window"].as_f64(), Some(0.72), "{settled}");
    assert_eq!(row.window, Some(0.72));
    let asked: Vec<String> = sent(&b).into_iter().filter(|l| l.contains("get_session_stats")).collect();
    assert_eq!(asked.len(), 2, "once to prove it speaks rpc and once for the window: {asked:?}");
}

#[test]
fn a_child_that_says_nothing_of_its_context_gives_no_reading() {
    let b = pi("");
    let row = b.run("pi", "hello");
    assert_eq!((row.state, row.window), (State::Done { ok: true }, None));
    assert!(b.events("task.settled")[0].data["window"].is_null());
}

#[test]
fn a_session_is_shrunk_by_the_compact_command_told_what_to_keep_and_the_task_goes_on_after_it() {
    let b = pi(", FAKE_CTX = \"900\", FAKE_CTX_AFTER = \"150\", FAKE_WINDOW = \"1000\"");
    let first = b.run("pi", "one");
    assert_eq!(first.window, Some(0.9));
    let second = shrunk(&b, &first, vec!["never touch vendor/".into()]);
    assert_eq!(second.state, State::Done { ok: true }, "{}", b.dump());
    let log = sent(&b);
    let asked: Vec<&String> = log.iter().filter(|l| l.contains("\"type\":\"compact\"")).collect();
    assert_eq!(asked.len(), 1, "{log:?}");
    assert!(asked[0].contains("MUST KEEP") && asked[0].contains("never touch vendor/") && asked[0].contains("customInstructions"), "{}", asked[0]);
    let prompts: Vec<&String> = log.iter().filter(|l| l.contains("\"type\":\"prompt\"")).collect();
    assert_eq!(prompts.len(), 2, "the first turn and the task's own words: nothing was prompted to shrink");
    assert!(prompts[1].contains("carry on") && prompts[1].contains("MUST KEEP"), "{}", prompts[1]);
    assert_eq!(log.iter().filter(|l| l.starts_with("start")).count(), 1, "one child throughout");
    let done = &b.events("task.compacted")[0].data;
    assert_eq!((done["ok"].clone(), done["note"].as_str(), done["before"].as_f64(), done["after"].as_f64()), (serde_json::json!(true), Some("900 → 120 tokens"), Some(0.9), Some(0.15)), "{done}");
    // what the session holds after is said as a size, the compaction's own call is not what it holds
    assert!(b.events("run.metrics").iter().any(|e| e.data["last"] == 120), "{:?}", b.events("run.metrics").iter().map(|e| e.data.clone()).collect::<Vec<_>>());
}

#[test]
fn a_child_of_a_session_is_started_to_shrink_it_when_none_is_alive() {
    let b = pi(", FAKE_CTX = \"900\", FAKE_CTX_AFTER = \"150\", FAKE_WINDOW = \"1000\"");
    let mut spec = TaskSpec::new("carry on", "pi");
    spec.resume = Some(SessionRef { doer: "pi".into(), id: "pi-sess-1".into() });
    spec.compact = Some(vec![]);
    let row = b.end(&b.tasks.spawn(spec, Actor::Operator).unwrap());
    assert_eq!(row.state, State::Done { ok: true }, "{}", b.dump());
    let log = sent(&b);
    assert!(log.iter().any(|l| l.starts_with("start") && l.contains("--session pi-sess-1")), "{log:?}");
    assert_eq!(log.iter().filter(|l| l.contains("\"type\":\"compact\"")).count(), 1, "told to shrink, once: {log:?}");
    assert_eq!(b.events("task.compacted")[0].data["ok"], true);
}

#[test]
fn a_turn_that_failed_is_not_asked_how_full_it_is() {
    let b = Bench::new(&[("fake-pi", FAKE)], &manifest("omp", "omp-rpc-refused.jsonl", ", FAKE_CTX = \"900\", FAKE_WINDOW = \"1000\""));
    let row = b.run("omp", "reply with exactly: pong");
    assert!(failed(&row).contains("402 Insufficient credits"), "{row:?}");
    wait_for("the log", &b, || !sent(&b).is_empty());
    std::thread::sleep(std::time::Duration::from_millis(200));
    let asked = sent(&b).into_iter().filter(|l| l.contains("get_session_stats")).count();
    assert_eq!(asked, 1, "only the proof that it speaks rpc: {:?}", sent(&b));
    assert_eq!(row.window, None);
}

#[test]
fn the_fullness_is_read_from_tokens_or_else_from_a_percentage() {
    use j4n_connect::pi::session::context_fraction;
    let stats = |usage: serde_json::Value| serde_json::json!({ "data": { "contextUsage": usage } });
    assert_eq!(context_fraction(&stats(serde_json::json!({ "tokens": 300, "contextWindow": 1000, "percent": 99.0 }))), Some(0.3), "tokens over the window first");
    assert_eq!(context_fraction(&stats(serde_json::json!({ "percent": 50.0 }))), Some(0.5), "a percentage is out of a hundred");
    assert_eq!(context_fraction(&stats(serde_json::json!({ "percent": 250.0 }))), Some(1.0));
    assert_eq!(context_fraction(&stats(serde_json::json!({ "tokens": 5, "contextWindow": 0, "percent": 10.0 }))), Some(0.1), "a window of nothing is no window");
    assert_eq!(context_fraction(&stats(serde_json::json!({}))), None);
    assert_eq!(context_fraction(&serde_json::json!({})), None);
}

#[test]
fn a_session_that_will_not_shrink_is_a_note_and_the_task_goes_on() {
    let b = pi(", FAKE_COMPACT_FAIL = \"1\"");
    let first = b.run("pi", "one");
    let second = shrunk(&b, &first, vec![]);
    assert_eq!(second.state, State::Done { ok: true }, "{}", b.dump());
    let notes: Vec<String> = b.events("run.notice").iter().filter_map(|e| e.data["text"].as_str().map(String::from)).collect();
    assert!(notes.iter().any(|n| n.contains("could not be shrunk") && n.contains("Nothing to compact")), "{notes:?}");
    assert_eq!(b.events("task.compacted")[0].data["ok"], false);
}

#[test]
fn a_program_that_will_not_speak_rpc_cannot_be_told_to_shrink_a_session_and_the_print_mode_is_not_asked_to() {
    let b = pi(", FAKE_RPC = \"0\"");
    let mut spec = TaskSpec::new("carry on", "pi");
    spec.resume = Some(SessionRef { doer: "pi".into(), id: "pi-sess-1".into() });
    spec.compact = Some(vec![]);
    let row = b.end(&b.tasks.spawn(spec, Actor::Operator).unwrap());
    assert_eq!(row.state, State::Done { ok: true }, "the task still ran, after the note: {}", b.dump());
    let notes: Vec<String> = b.events("run.notice").iter().filter_map(|e| e.data["text"].as_str().map(String::from)).collect();
    assert!(notes.iter().any(|n| n.contains("could not be shrunk") && n.contains("could not be started to shrink the session")), "{notes:?}");
    let prints = sent(&b).into_iter().filter(|l| l.starts_with("start") && l.contains("-p")).count();
    assert_eq!(prints, 1, "only the task's own words ran in the print mode, not the keep block: {:?}", sent(&b));
}

/// A call shows when pi starts it and grows as its arguments arrive, whether pi names it in the event or only in the message so far.
#[test]
fn a_pi_call_opens_at_its_first_byte_and_grows_with_its_arguments() {
    let mut tr = Translator::new();
    let mut say = |v: serde_json::Value| tr.line(&v.to_string()).evs;
    let open = |id: &str, name: &str, target: &str, bytes: u64| RunEv::Open { id: id.into(), name: name.into(), target: target.into(), bytes };
    let named = serde_json::json!({ "type": "message_update", "assistantMessageEvent": { "type": "toolcall_start", "contentIndex": 1, "toolName": "write" } });
    assert_eq!(say(named), [open("1", "write", "", 0)]);
    let grow = |d: &str| serde_json::json!({ "type": "message_update", "assistantMessageEvent": { "type": "toolcall_delta", "contentIndex": 1, "delta": d } });
    assert_eq!(say(grow("{\"path\":\"chess")), [open("1", "write", "", 14)]);
    assert_eq!(say(grow(".py\",\"content\":\"imp")), [open("1", "write", "chess.py", 33)]);
    // the name only in the message so far, the arguments parsed by pi
    let partial = |n: &str| serde_json::json!({ "type": "message_update", "assistantMessageEvent": { "type": "toolcall_start", "contentIndex": 0, "partial": { "content": [{ "type": "toolCall", "name": n, "arguments": {} }] } } });
    assert_eq!(say(partial("bash")), [open("0", "bash", "", 2)]);
    let parsed = serde_json::json!({ "type": "message_update", "assistantMessageEvent": { "type": "toolcall_delta", "contentIndex": 0, "delta": "", "partial": { "content": [{ "type": "toolCall", "name": "bash", "arguments": { "command": "ls" } }] } } });
    assert_eq!(say(parsed), [open("0", "bash", "ls", 16)]);
}

/// The real capture says a call began, and what it said stays what it said.
#[test]
fn the_capture_of_pi_opens_calls_by_their_index_and_the_real_call_follows() {
    let (evs, ..) = replay("pi-json-mode.jsonl");
    let at = |f: fn(&RunEv) -> bool| evs.iter().position(f);
    let opened = at(|e| matches!(e, RunEv::Open { .. })).expect("a call opened");
    let ran = at(|e| matches!(e, RunEv::Shell { .. })).unwrap();
    assert!(opened < ran, "{evs:?}");
}

/// Pi's settled thinking is a `think` step as claude's is, and its output tokens are said as they grow.
#[test]
fn pi_thinking_settles_as_reasoning_and_the_tokens_it_writes_are_said() {
    let mut tr = Translator::new();
    let usage = |n: u64| serde_json::json!({ "output": n, "input": 3 });
    let update = serde_json::json!({ "type": "message_update", "assistantMessageEvent": { "type": "text_delta", "delta": "hi", "partial": { "usage": usage(12) } } });
    let evs = tr.line(&update.to_string()).evs;
    assert_eq!(evs, [RunEv::Text("hi".into()), RunEv::Tokens(12)]);
    assert!(tr.line(&update.to_string()).evs.iter().all(|e| !matches!(e, RunEv::Tokens(_))), "the same count is not said again");
    let end = serde_json::json!({ "type": "message_end", "message": { "role": "assistant", "usage": usage(20), "content": [{ "type": "thinking", "thinking": "hm" }, { "type": "text", "text": "ok" }] } });
    let evs = tr.line(&end.to_string()).evs;
    assert!(evs.iter().any(|e| matches!(e, RunEv::Message { role: j4n_core::doer::Role::Reasoning, text } if text == "hm")), "{evs:?}");
}

/// What a call is about is read from arguments that are cut short, and only once the string has closed.
#[test]
fn what_a_cut_short_call_is_about_is_read_only_once_its_string_has_closed() {
    use j4n_connect::opening::target_in;
    assert_eq!(target_in(""), "");
    assert_eq!(target_in("{\"path\":\"snak"), "", "a half path is not shown");
    assert_eq!(target_in("{\"path\": \"a/b.py\", \"content\":\"x"), "a/b.py");
    assert_eq!(target_in("{\"content\":\"the \\\"path\\\" is\",\"file_path\":\"c.rs\"}"), "c.rs", "a key inside a string is not a key");
    assert_eq!(target_in("{\"command\":\"echo \\\"hi\\\"\",\"x"), "echo \"hi\"", "an escaped quote does not close the string");
    assert_eq!(target_in("{\"command\":\"ls\",\"path\":\"d\"}"), "d", "a path is preferred to a command");
    assert_eq!(target_in("{\"path\":5}"), "");
}

/// The claude CLI streams a call's arguments with no tool name: the keys written so far say it, and only the top-level ones count.
#[test]
fn the_keys_of_arguments_written_so_far_say_what_tool_they_are_of() {
    use j4n_connect::opening::{keys_in, name_in};
    assert_eq!(keys_in("{\"file_path\": \"/a\", \"content\": \"x\\\"y\", \"o"), ["file_path", "content"], "a key whose string has not closed is not one yet");
    assert_eq!(keys_in("{\"a\":{\"b\":1},\"c\":[\"d\",{\"e\":2}],\"f\""), ["a", "c"], "nor is a key of what is inside");
    assert_eq!(keys_in(""), Vec::<String>::new());
    assert_eq!(name_in("{\"file_path\": \"x.py\", \"content\": \"p"), "Write");
    assert_eq!(name_in("{\"file_path\": \"x.py\", \"old_string\": \"p"), "Edit");
    assert_eq!(name_in("{\"command\": \"ls\""), "Bash");
    assert_eq!(name_in("{\"file_path\": \"x.py\""), "", "a path alone is a read as well as a write");
    assert_eq!(name_in("{\"content\": \"x\""), "");
}

#[test]
fn every_call_a_pi_message_reports_is_a_request_that_a_model_served_by_a_host() {
    let (evs, _, _) = replay("omp-json-mode.jsonl");
    let served: Vec<_> = evs.iter().filter_map(|e| if let RunEv::Served(s) = e { Some(s) } else { None }).collect();
    assert!(!served.is_empty());
    assert!(served.iter().all(|s| s.model == "glm-5.3-flash" && s.host.as_deref() == Some("fireworks")), "{served:?}");
    let metrics = evs.iter().filter(|e| matches!(e, RunEv::Metrics(_))).count();
    assert_eq!(served.len(), metrics, "one for each call that was paid for");
}

/// `tokens_in` of a pi or omp call is every token the model read (fresh input and both cache buckets), the two cache parts ride
/// in `run.metrics` and not in `run.served`, so a Log total counts them once.
#[test]
fn a_pi_calls_tokens_in_is_input_and_both_cache_parts_and_they_are_said_once() {
    let end = |usage: serde_json::Value| serde_json::json!({ "type": "message_end", "message": { "role": "assistant", "model": "glm-x", "provider": "fireworks", "usage": usage, "content": [] } }).to_string();
    let mut tr = Translator::new();
    let evs = tr.line(&end(serde_json::json!({ "input": 5, "output": 7, "cacheRead": 1000, "cacheWrite": 200, "totalTokens": 1212, "cost": { "total": 0.01 } }))).evs;
    let m = evs.iter().find_map(|e| if let RunEv::Metrics(m) = e { Some(*m) } else { None }).expect("metrics");
    assert_eq!((m.tokens_in, m.tokens_out, m.cached, m.wrote), (1205, 7, 1000, 200), "{evs:?}");
    let served = evs.iter().find_map(|e| if let RunEv::Served(s) = e { Some(s.clone()) } else { None }).expect("served");
    assert_eq!((served.input, served.cached, served.wrote), (5, 0, 0), "the cache parts are in one row only: {served:?}");
    // a call without a cache says none
    let evs = Translator::new().line(&end(serde_json::json!({ "input": 5, "output": 7, "totalTokens": 12, "cost": { "total": 0.01 } }))).evs;
    assert!(evs.iter().any(|e| matches!(e, RunEv::Metrics(m) if m.tokens_in == 5 && m.cached == 0 && m.wrote == 0)), "{evs:?}");
}
