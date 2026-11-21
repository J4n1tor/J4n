//! Claude Code as a doer, through the real loop: a child that plays recorded frames stands in for `claude`. A command's
//! real exit code is a persisted event, the answer is the report, and the probe says "logged in".

use crate::world::{Rig, Scratch, config, fake, fixtures, script};
use j4n_connect::Claude;
use j4n_connect::claude::{VERIFIED_VERSION, describe};
use j4n_core::answer::chain::Registry;
use j4n_core::answer::point::points;
use j4n_core::answer::state::Answerers;
use j4n_core::ask::Answer;
use j4n_core::change::Actor;
use j4n_core::doer::Doer;
use j4n_core::gate::Gate;
use j4n_core::gate::check::Gates;
use j4n_core::gate::grant::{Grants, Scope, here_layer};
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use j4n_core::task::{Asks, Ctl, Mode, State, TaskSpec, Tune, Why};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

fn done(rig: &Rig, id: &str) -> j4n_core::task::Row {
    let row = rig.finished(id);
    assert_eq!(row.state, State::Done { ok: true }, "{}", rig.dump());
    row
}

fn answer(rig: &Rig, ask: &str, a: Answer) {
    rig.tasks.ctl(ask, Ctl::Answer(a), Actor::Operator).unwrap_or_else(|e| panic!("{e}\n{}", rig.dump()));
}

/// What the child was sent that is JSON: the user's messages and the answers to its questions.
fn sent(rig: &Rig) -> Vec<Value> {
    rig.sent().iter().filter(|l| l.starts_with('{')).filter_map(|l| serde_json::from_str(l).ok()).collect()
}

fn responses(rig: &Rig) -> Vec<Value> {
    sent(rig).into_iter().filter(|v| v["type"] == "control_response").map(|v| v["response"]["response"].clone()).collect()
}

fn users(rig: &Rig) -> Vec<String> {
    sent(rig).into_iter().filter(|v| v["type"] == "user").map(|v| v["message"]["content"][0]["text"].as_str().unwrap_or("").to_string()).collect()
}

fn starts(rig: &Rig) -> Vec<String> {
    rig.sent().into_iter().filter_map(|l| l.strip_prefix("start ").map(String::from)).collect()
}

fn events(rig: &Rig, task: &str, kind: &str) -> Vec<Value> {
    rig.events().into_iter().filter(|e| e.kind == kind && e.task.as_deref() == Some(task)).map(|e| e.data).collect()
}

// ---- a command's exit code, and the answer as the report ----

#[test]
fn a_commands_real_exit_code_is_a_persisted_event_the_chat_can_show() {
    let rig = Rig::playing("bash-exit-3-2.1.309.jsonl");
    let id = rig.spawn("run the command");
    let ask = rig.waiting_on(&id);
    answer(&rig, &ask, Answer::Yes);
    let row = done(&rig, &id);
    let shells = events(&rig, &id, "run.shell");
    assert_eq!(shells.len(), 1, "{}", rig.dump());
    assert_eq!(shells[0]["cmd"], "sh -c 'echo oops >&2; exit 3'");
    assert_eq!(shells[0]["exit"], 3, "the code the command really returned, not a guess from the flag");
    assert_eq!(shells[0]["out"], "oops");
    assert_eq!(row.answer.as_deref(), Some("The command exited with code 3 and printed \"oops\" to stderr."));
    // the child itself was told yes, with its own input back
    let [allow] = &responses(&rig)[..] else { panic!("{:?}", responses(&rig)) };
    assert_eq!(allow["behavior"], "allow");
    assert_eq!(allow["updatedInput"]["command"], "sh -c 'echo oops >&2; exit 3'");
}

#[test]
fn narration_then_a_tool_then_the_report_the_answer_is_the_report() {
    let rig = Rig::playing("narration-report.jsonl");
    let id = rig.spawn("what does a.txt say");
    let row = done(&rig, &id);
    assert_eq!(row.answer.as_deref(), Some("Report: a.txt says hello."));
    let messages = events(&rig, &id, "run.message");
    let narration: Vec<&Value> = messages.iter().filter(|m| m["narration"] == true).collect();
    let finals: Vec<&Value> = messages.iter().filter(|m| m["final"] == true).collect();
    assert_eq!(narration.len(), 1, "{}", rig.dump());
    assert_eq!(narration[0]["text"], "I'll look at the folder.", "what was said before the tool keeps its place");
    assert_eq!(finals.len(), 1);
    assert_eq!(finals[0]["text"], "Report: a.txt says hello.");
    // in the order it happened
    let kinds: Vec<String> = rig.events().into_iter().filter(|e| e.task.as_deref() == Some(id.as_str())).map(|e| e.kind).collect();
    let at = |k: &str, from: usize| from + kinds[from..].iter().position(|x| x == k).unwrap_or_else(|| panic!("no {k} after {from} in {kinds:?}"));
    let (n, call) = (at("run.message", 0), at("run.tool_call", 0));
    let (result, last) = (at("run.tool_result", call), at("run.message", call));
    assert!(n < call && call < result && result < last, "{kinds:?}");
    assert_eq!(row.tokens_out, 9);
    assert!(row.spent > 0, "the meter reached the task");
}

/// Through the real loop: one 24 kB Write. Before the call ends the chat has a live row that names the file and grows; the Log has no row for a
/// fragment (a few written events, as for any turn), and when the turn ends the row is gone.
#[test]
fn a_long_write_is_a_live_row_that_grows_before_the_call_ends_and_the_log_has_no_row_for_a_fragment() {
    use j4n_core::front::rows::Rows;
    let rig = Rig::playing("write-long.jsonl");
    let sub = rig.log.subscribe("*", 10_000);
    let id = rig.spawn("write chess.py");
    done(&rig, &id);
    // the book is done a moment before the subscriber (fed by the writer's thread) has the last events: wait for the one that ends the task, which
    // a machine under load can be late with, and then take what is waiting
    let mut all: Vec<Arc<j4n_core::log::Ev>> = Vec::new();
    while let Some(e) = sub.recv_timeout(std::time::Duration::from_secs(90)) {
        let last = e.kind == "task.settled" && e.task.as_deref() == Some(id.as_str());
        all.push(e);
        if last {
            break;
        }
    }
    all.extend(std::iter::from_fn(|| sub.try_recv()));
    assert!(all.iter().any(|e| e.kind == "task.settled"), "the end of the task reached the subscriber: {}", rig.dump());
    let written = all.iter().filter(|e| e.seq != 0 && e.kind.starts_with("run.")).count();
    assert!(written <= 8, "the turn's own events, none for the {} fragments: {written}", all.len());
    // a flashed event and a written one reach the subscriber in no fixed order (the writer is another thread), so the turn is rebuilt by what each says: the
    // written events before the call ends, in their order, then what was flashed up to the last word of the call, in its own order
    let call = all.iter().find(|e| e.kind == "run.tool_call").map(|e| e.seq).unwrap_or_else(|| panic!("{}", rig.dump()));
    let flashed: Vec<&Arc<j4n_core::log::Ev>> = all.iter().filter(|e| e.seq == 0).collect();
    let last_open = flashed.iter().rposition(|e| e.kind == "run.open").expect("the call was said as it was written");
    let before = all.iter().filter(|e| e.seq != 0 && e.seq < call).chain(flashed[..=last_open].iter().copied());
    let live = |rows: &Rows| rows.rows().into_iter().find(|r| r.key == format!("w:{id}")).map(|r| (r.cells[0].icon.clone(), r.cells[0].text.clone(), r.cells[2].text.clone()));
    let mut rows = Rows::new();
    let mut sizes = Vec::new();
    for e in before {
        rows.put(e);
        if e.kind == "run.open" && let Some((_, _, size)) = live(&rows) {
            sizes.push(size);
        }
    }
    let (icon, text, size) = live(&rows).unwrap_or_else(|| panic!("no live row before the call ends: {:?}", rows.rows()));
    assert_eq!((icon.as_str(), text.as_str()), ("new", "writing /w/chess.py"));
    assert!(size.ends_with(" kB") && size.trim_end_matches(" kB").parse::<f64>().is_ok_and(|n| n > 20.0), "{size}");
    sizes.dedup();
    assert!(sizes.len() > 20, "it grew step by step: {sizes:?}");
    // the settled call is the step, and the row goes with the turn
    all.iter().filter(|e| e.seq >= call).for_each(|e| rows.put(e));
    assert!(live(&rows).is_none(), "{:?}", rows.rows());
    assert_eq!(rows.rows().iter().filter(|r| r.cells[0].icon == "new").count(), 1, "one step for the call");
}

// ---- starting it ----

#[test]
fn it_is_started_in_the_task_s_folder_with_its_settings_and_the_goal_is_its_first_message() {
    let rig = Rig::playing("two-turns.jsonl");
    let mut spec = TaskSpec::new("do the thing", "claude");
    spec.tune = Tune { model: Some("haiku".into()), endpoint: None, effort: Some("high".into()), mode: Mode::Write };
    let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
    done(&rig, &id);
    let args = &starts(&rig)[0];
    for want in ["-p", "--input-format stream-json", "--output-format stream-json", "--verbose", "--permission-prompt-tool stdio", "--include-partial-messages", "--permission-mode default", "--model haiku", "--effort high"] {
        assert!(args.contains(want), "{want} in {args}");
    }
    assert_eq!(users(&rig), vec!["do the thing"]);
}

#[test]
fn a_read_task_has_the_tools_that_write_denied_and_an_effort_claude_does_not_know_is_not_passed() {
    let rig = Rig::playing("two-turns.jsonl");
    let mut spec = TaskSpec::new("look", "claude");
    spec.tune = Tune { model: None, endpoint: None, effort: Some("turbo".into()), mode: Mode::Read };
    let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
    done(&rig, &id);
    let args = &starts(&rig)[0];
    assert!(args.contains("--permission-mode default") && args.contains("--disallowedTools Edit,Write,NotebookEdit") && !args.contains("--effort") && !args.contains("--model"), "{args}");
}

/// The CLI runs another model in its `plan` permission mode for the settings `haiku` and `opusplan`, and
/// still names the setting in its `init` frame, so a plan, review or scout turn pinned to haiku was served and billed by sonnet. A read turn is
/// the ordinary mode with the writing tools denied; Gate stays the wall for commands.
#[test]
fn a_read_turn_never_uses_the_cli_s_plan_mode_so_a_haiku_pin_means_haiku_and_a_write_turn_is_as_before() {
    for mode in [Mode::Read, Mode::Write] {
        let rig = Rig::playing("two-turns.jsonl");
        let mut spec = TaskSpec::new("look", "claude");
        spec.tune = Tune { model: Some("haiku".into()), endpoint: None, effort: None, mode };
        let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
        done(&rig, &id);
        let args = &starts(&rig)[0];
        let words: Vec<&str> = args.split(' ').collect();
        assert!(!words.contains(&"plan") && words.windows(2).any(|w| w == ["--permission-mode", "default"]) && args.contains("--model haiku"), "{args}");
        assert_eq!(args.contains("--disallowedTools Edit,Write,NotebookEdit"), mode == Mode::Read, "{args}");
    }
}

/// The row of the connectors says the same as the default, and a row of the operator's can still say another: `modes.read` is data.
#[test]
fn the_connector_row_of_claude_says_a_read_turn_is_default_with_the_writing_tools_denied() {
    let layer = j4n_core::table::Layer::new(
        j4n_core::table::LayerKind::Pack,
        "agents",
        vec![j4n_core::table::SourceFile { path: "connectors/agents.toml".into(), text: include_str!("../../../../packs/base/connectors/agents.toml").into() }],
    );
    let (list, said) = j4n_connect::manifest::Connectors::load(&Tables::load(vec![layer]).0);
    assert!(said.is_empty(), "{said:?}");
    let row = list.all().iter().find(|c| c.id == "claude").unwrap();
    let cfg = j4n_connect::claude::Config::from_connector(row, Default::default());
    assert_eq!(cfg.read, ["--permission-mode", "default", "--disallowedTools", "Edit,Write,NotebookEdit"]);
    assert_eq!(cfg.read, j4n_connect::claude::Config::default().read, "the row and the default say one thing");
}

/// Claude takes no Brief, so the skills J4n picked are its own, in a plugin folder of the task's place that it is started on.
#[test]
fn the_skills_picked_for_a_task_are_claudes_own_in_a_folder_of_the_tasks_place_and_a_task_with_none_has_none() {
    let rig = Rig::playing("two-turns.jsonl");
    let mut spec = TaskSpec::new("run the tests", "claude");
    spec.input = json!({ "skills": [{ "id": "j4n-checks", "why": "said tests", "tokens": 20, "when": "tests, lint: ci", "body": "# Run the checks\nUse scripts/t." }, { "id": "no-body", "why": "x", "tokens": 1 }] });
    let id = rig.tasks.spawn(spec.clone(), Actor::Operator).unwrap();
    done(&rig, &id);
    let args = &starts(&rig)[0];
    let dir = std::path::PathBuf::from(args.split("--plugin-dir ").nth(1).expect("a plugin folder").split(' ').next().unwrap());
    assert!(dir.ends_with(format!(".j4n/skills/{id}")), "the task's own place, under its `.j4n`: {}", dir.display());
    assert!(!dir.to_string_lossy().contains(".claude"), "never claude's own config folder: {}", dir.display());
    let manifest: Value = serde_json::from_str(&std::fs::read_to_string(dir.join(".claude-plugin/plugin.json")).unwrap()).unwrap();
    assert_eq!(manifest["name"], "j4n-skills");
    let skill = std::fs::read_to_string(dir.join("skills/j4n-checks/SKILL.md")).unwrap();
    assert_eq!(skill, "---\nname: j4n-checks\ndescription: \"Use when: tests, lint: ci\"\n---\n# Run the checks\nUse scripts/t.\n");
    assert!(!dir.join("skills/no-body").exists(), "a skill with no words is not a skill");
    // the same skills are the same child; none is no flag at all
    let other = TaskSpec { goal: "again".into(), ..spec.clone() };
    assert_eq!(j4n_connect::claude::skills::mark(&spec), j4n_connect::claude::skills::mark(&other));
    assert_eq!(j4n_connect::claude::skills::mark(&TaskSpec::new("x", "claude")), Vec::<String>::new());
    let rig = Rig::playing("two-turns.jsonl");
    let id = rig.tasks.spawn(TaskSpec::new("no skills", "claude"), Actor::Operator).unwrap();
    done(&rig, &id);
    assert!(!starts(&rig)[0].contains("--plugin-dir"), "{}", starts(&rig)[0]);
}

/// A skill that has a sentence of its own is listed by claude with that sentence; one that has none keeps `Use when: <words>`.
#[test]
fn claude_lists_a_skill_by_its_own_sentence_when_it_has_one() {
    let rig = Rig::playing("two-turns.jsonl");
    let mut spec = TaskSpec::new("update the changelog", "claude");
    spec.input = json!({ "skills": [
        { "id": "release-chores", "why": "said changelog", "tokens": 20, "when": "changelog, release", "description": "Read before a changelog line, a version bump or a tag.", "body": "# Chores\nDo them as written." },
        { "id": "plain", "why": "x", "tokens": 1, "when": "tests", "body": "# Plain\nWords." }] });
    let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
    done(&rig, &id);
    let args = &starts(&rig)[0];
    let dir = std::path::PathBuf::from(args.split("--plugin-dir ").nth(1).expect("a plugin folder").split(' ').next().unwrap());
    let with = std::fs::read_to_string(dir.join("skills/release-chores/SKILL.md")).unwrap();
    assert_eq!(with, "---\nname: release-chores\ndescription: \"Read before a changelog line, a version bump or a tag.\"\n---\n# Chores\nDo them as written.\n");
    let without = std::fs::read_to_string(dir.join("skills/plain/SKILL.md")).unwrap();
    assert!(without.contains("description: \"Use when: tests\""), "{without}");
}

#[test]
fn a_model_or_effort_that_would_be_read_as_a_flag_is_refused_not_passed() {
    for (model, effort) in [(Some("--dangerously-skip-permissions"), None), (None, Some("-x")), (Some("haiku\n--x"), None)] {
        let rig = Rig::playing("two-turns.jsonl");
        let mut spec = TaskSpec::new("x", "claude");
        spec.tune = Tune { model: model.map(String::from), endpoint: None, effort: effort.map(String::from), mode: Mode::Write };
        let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
        let row = rig.finished(&id);
        let State::Stopped { why: Why::Failed { message } } = &row.state else { panic!("{:?}", row.state) };
        assert!(message.contains("cannot start with a dash"), "{message}");
        assert!(starts(&rig).is_empty(), "no child was started");
    }
}

#[test]
fn a_program_that_cannot_start_stops_the_task_and_says_why() {
    let bin = Scratch::new("bin");
    let rig = Rig::over(bin, config(Path::new("/no/such/claude"), &[]), |_| {});
    let id = rig.spawn("x");
    let row = rig.finished(&id);
    let State::Stopped { why: Why::Failed { message } } = &row.state else { panic!("{:?}", row.state) };
    assert!(message.contains("could not start `/no/such/claude`"), "{message}");
}

// ---- the session and the child ----

#[test]
fn a_follow_up_turn_is_the_same_child_and_the_same_session() {
    let rig = Rig::playing("two-turns.jsonl");
    let first = rig.spawn("one");
    let row = done(&rig, &first);
    let session = row.session.clone().expect("the task knows its session");
    assert_eq!((session.doer.as_str(), session.id.as_str()), ("claude", "00000000-0000-4000-8000-0000000000b1"));
    assert_eq!(row.answer.as_deref(), Some("one"));
    assert_eq!(rig.claude.live(), 1, "the child stays for the next turn");
    let second = rig.tasks.resume(&first, Some("again"), Actor::Operator).unwrap();
    let row = done(&rig, &second);
    assert_eq!(row.answer.as_deref(), Some("two"));
    assert_eq!(starts(&rig).len(), 1, "one process for the whole conversation: {:?}", rig.sent());
    assert_eq!(users(&rig), vec!["one", "again"]);
    assert_eq!(rig.claude.live(), 1);
    let metrics: Vec<f64> = events(&rig, &second, "run.metrics").iter().map(|m| m["usd"].as_f64().unwrap()).collect();
    assert!(matches!(&metrics[..], [m] if (m - 0.02).abs() < 1e-9), "the second turn books its own spend, not the running total: {metrics:?}");
}

#[test]
fn a_session_whose_child_is_gone_is_resumed_from_its_transcript() {
    let rig = Rig::playing("two-turns.jsonl");
    let first = rig.spawn("one");
    done(&rig, &first);
    rig.claude.shutdown();
    assert_eq!(rig.claude.live(), 0);
    let second = rig.tasks.resume(&first, Some("again"), Actor::Operator).unwrap();
    done(&rig, &second);
    let all = starts(&rig);
    assert_eq!(all.len(), 2, "{all:?}");
    assert!(all[1].ends_with("--resume 00000000-0000-4000-8000-0000000000b1"), "{}", all[1]);
}

#[test]
fn shutdown_takes_every_child_down_with_all_it_started() {
    let bin = Scratch::new("bin");
    let pidfile = bin.join("pid");
    let program = script(
        &bin,
        "claude",
        r#"echo $$ > "$PIDFILE"
IFS= read -r first
printf '%s\n' '{"type":"system","subtype":"init","session_id":"S"}'
printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"done","session_id":"S","total_cost_usd":0,"usage":{}}'
while IFS= read -r l; do :; done"#,
    );
    let rig = Rig::over(bin, config(&program, &[("PIDFILE", pidfile.to_str().unwrap())]), |_| {});
    let id = rig.spawn("x");
    done(&rig, &id);
    let pid: u32 = rig.bin.read("pid").trim().parse().unwrap();
    let there = |pid: u32| std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| !s.contains(") Z "));
    assert!(there(pid));
    rig.claude.shutdown();
    assert!((0..200).any(|_| {
        std::thread::sleep(Duration::from_millis(10));
        !there(pid)
    }), "pid {pid} still runs");
}

#[test]
fn a_steer_reaches_the_running_turn_as_a_line_of_its_own() {
    let bin = Scratch::new("bin");
    let log = bin.join("log");
    let program = script(
        &bin,
        "claude",
        r#"IFS= read -r first; printf '%s\n' "$first" >> "$FAKE_LOG"
printf '%s\n' '{"type":"system","subtype":"init","session_id":"S"}'
IFS= read -r second; printf '%s\n' "$second" >> "$FAKE_LOG"
printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"steered","session_id":"S","total_cost_usd":0,"usage":{}}'
while IFS= read -r l; do :; done"#,
    );
    let rig = Rig::over(bin, config(&program, &[("FAKE_LOG", log.to_str().unwrap())]), |_| {});
    let id = rig.spawn("start");
    rig.until_event("the run's session", |e| e.kind == "run.session");
    rig.tasks.ctl(&id, Ctl::Steer("also this".into()), Actor::Operator).unwrap();
    let row = done(&rig, &id);
    assert_eq!(row.answer.as_deref(), Some("steered"));
    assert_eq!(users(&rig), vec!["start", "also this"]);
}

#[test]
fn a_child_that_dies_in_the_middle_of_a_turn_fails_the_task_with_what_it_wrote() {
    let bin = Scratch::new("bin");
    let program = script(&bin, "claude", "IFS= read -r first; echo 'something broke' >&2; exit 7");
    let rig = Rig::over(bin, config(&program, &[]), |_| {});
    let id = rig.spawn("x");
    let row = rig.finished(&id);
    let State::Stopped { why: Why::Failed { message } } = &row.state else { panic!("{:?}\n{}", row.state, rig.dump()) };
    assert!(message.contains("exit 7") && message.contains("something broke"), "{message}");
    assert!((0..200).any(|_| {
        std::thread::sleep(Duration::from_millis(10));
        rig.claude.live() == 0
    }), "a child that ended is not a live one");
}

#[test]
fn a_child_that_stays_mute_is_given_up_on_and_taken_down() {
    let bin = Scratch::new("bin");
    let program = script(&bin, "claude", "IFS= read -r first; sleep 30");
    let mut cfg = config(&program, &[]);
    cfg.init_timeout = Duration::from_millis(300);
    let rig = Rig::over(bin, cfg, |_| {});
    let id = rig.spawn("x");
    let row = rig.finished(&id);
    let State::Stopped { why: Why::Failed { message } } = &row.state else { panic!("{:?}", row.state) };
    assert!(message.contains("said nothing for 0 seconds"), "{message}");
}

#[test]
fn stopping_the_task_stops_the_child_and_keeps_the_session_to_carry_on_in() {
    let bin = Scratch::new("bin");
    let log = bin.join("log");
    let program = script(
        &bin,
        "claude",
        r#"echo "start $*" >> "$FAKE_LOG"
IFS= read -r first
printf '%s\n' '{"type":"system","subtype":"init","session_id":"S9"}'
sleep 300"#,
    );
    let rig = Rig::over(bin, config(&program, &[("FAKE_LOG", log.to_str().unwrap())]), |_| {});
    let id = rig.spawn("x");
    rig.until_event("the run's session", |e| e.kind == "run.session");
    rig.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
    let row = rig.finished(&id);
    assert!(matches!(row.state, State::Stopped { .. }), "{:?}", row.state);
    assert_eq!(row.session.map(|s| s.id).as_deref(), Some("S9"));
    assert!((0..200).any(|_| {
        std::thread::sleep(Duration::from_millis(10));
        rig.claude.live() == 0
    }), "the child is not kept");
}

#[test]
fn the_child_is_given_the_list_tools_of_the_print_mode_unless_the_environment_says_otherwise() {
    let body = r#"echo "start tools=$CLAUDE_CODE_ENABLE_TODO_TOOLS" >> "$FAKE_LOG"
IFS= read -r first
printf '%s\n' '{"type":"system","subtype":"init","session_id":"S1"}'
printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"ok","session_id":"S1","total_cost_usd":0,"usage":{"input_tokens":1,"output_tokens":1}}'
exit 0"#;
    let rig = Rig::scripted(body);
    let id = rig.spawn("x");
    rig.finished(&id);
    assert_eq!(starts(&rig), ["tools=1"], "{}", rig.dump());
    let bin = Scratch::new("bin");
    let log = bin.join("log");
    let program = script(&bin, "claude", body);
    let rig = Rig::over(bin, config(&program, &[("FAKE_LOG", log.to_str().unwrap()), ("CLAUDE_CODE_ENABLE_TODO_TOOLS", "0")]), |_| {});
    let id = rig.spawn("x");
    rig.finished(&id);
    assert_eq!(starts(&rig), ["tools=0"], "an account's own setting stays: {}", rig.dump());
}

// ---- profiles ----

use j4n_connect::claude::Pick;
use j4n_connect::manifest::Profile;

/// A row of the connectors made from text, as the loader reads it (the problems it reports come back with it).
fn rows(text: &str) -> (j4n_connect::manifest::Connectors, Vec<j4n_core::table::Problem>) {
    let layer = Layer::new(LayerKind::Pack, "t", vec![SourceFile { path: "connectors/t.toml".into(), text: text.into() }]);
    let (tables, mut said) = Tables::load(vec![layer]);
    let (list, more) = j4n_connect::manifest::Connectors::load(&tables);
    said.extend(more);
    (list, said)
}

fn profiled(pick: Pick) -> impl FnOnce(&mut j4n_connect::claude::Config) {
    move |cfg| {
        cfg.full = Profile { args: vec![], env: [("WHO".to_string(), "full".to_string())].into() };
        cfg.lean = Profile { args: vec!["--safe-mode".into(), "--tools".into(), "Read".into()], env: [("WHO".to_string(), "lean".to_string())].into() };
        cfg.lean_for = vec!["chore".into(), "answer".into()];
        cfg.pick = pick;
    }
}

fn started_as(pick: Pick, role: Option<&str>) -> String {
    let rig = Rig::playing_with("two-turns.jsonl", profiled(pick));
    let mut spec = TaskSpec::new("look", "claude");
    spec.role = role.map(String::from);
    let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
    done(&rig, &id);
    starts(&rig).remove(0)
}

/// Under `full` every task is started as the tool would load itself, whatever its role; under `lean` every one is
/// lean; under `auto` the roles the row names (a chore, a decider) are lean and work, plan and a task with no role are full.
#[test]
fn the_profile_of_a_task_is_picked_by_the_setting_and_under_auto_by_its_role() {
    let lean = "--safe-mode --tools Read";
    for role in [None, Some("work"), Some("plan"), Some("chore"), Some("answer")] {
        assert!(!started_as(Pick::Full, role).contains("--safe-mode"), "full is for every role: {role:?}");
        assert!(started_as(Pick::Lean, role).ends_with(lean), "lean is for every role: {role:?}");
    }
    for (role, is_lean) in [(None, false), (Some("work"), false), (Some("plan"), false), (Some("review"), false), (Some("chore"), true), (Some("answer"), true)] {
        let args = started_as(Pick::Auto, role);
        assert_eq!(args.ends_with(lean), is_lean, "auto, role {role:?}: {args}");
        assert!(args.contains("--permission-mode default"), "what the row's modes say stays in both: {args}");
    }
}

/// What a child is started with when nothing is said: the setting is `full` (its default), the row's `lean` is not empty and the task's role is a lean one.
/// Byte for byte, so a change of the default's argv shows here and not only as a missing flag.
#[test]
fn the_default_argv_is_exactly_what_it_was_before_profiles_for_every_role() {
    // the settings of the door come after the mode's flags and before the task's model and effort
    let door = format!("--settings {}", j4n_core::gate::door::claude_settings());
    let head = "-p --input-format stream-json --output-format stream-json --verbose --permission-prompt-tool stdio --include-partial-messages --permission-mode default";
    let base = format!("{head} {door}");
    for role in [None, Some("work"), Some("chore"), Some("answer")] {
        assert_eq!(started_as(Pick::Full, role), base, "{role:?}");
    }
    let rig = Rig::playing_with("two-turns.jsonl", profiled(Pick::Full));
    let mut spec = TaskSpec::new("look", "claude");
    spec.role = Some("chore".into());
    spec.tune = Tune { model: Some("haiku".into()), endpoint: None, effort: Some("low".into()), mode: Mode::Read };
    let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
    done(&rig, &id);
    assert_eq!(starts(&rig)[0], format!("{head} --disallowedTools Edit,Write,NotebookEdit {door} --model haiku --effort low"));
    assert!(!rig.project_dir().join(".j4n/claude").exists(), "a default run writes no record");
}

/// A session keeps the profile it began with. A line that carries on in it is started with that one, whatever the setting
/// and the line's role say now; and only a session begun lean is written down (one no record knows is full, as before profiles).
#[test]
fn a_session_is_started_again_with_the_profile_it_began_with() {
    let begun = |role: &str| {
        let rig = Rig::playing_with("two-turns.jsonl", profiled(Pick::Auto));
        let mut spec = TaskSpec::new("one", "claude");
        spec.role = Some(role.into());
        let first = rig.tasks.spawn(spec, Actor::Operator).unwrap();
        let session = done(&rig, &first).session.expect("its session");
        rig.claude.shutdown();
        let mut next = TaskSpec::new("two", "claude");
        (next.role, next.resume) = (Some(if role == "chore" { "work" } else { "chore" }.into()), Some(session));
        let second = rig.tasks.spawn(next, Actor::Operator).unwrap();
        done(&rig, &second);
        let args = starts(&rig).remove(1);
        (rig, args)
    };
    // begun lean (a chore), carried on by a task whose role is full: lean still, and written down when its id was known
    let (rig, args) = begun("chore");
    assert!(args.contains("--safe-mode --tools Read") && args.ends_with("--resume 00000000-0000-4000-8000-0000000000b1"), "{args}");
    assert!(rig.project_dir().join(".j4n/claude/lean/00000000-0000-4000-8000-0000000000b1").is_file());
    // begun full (work), carried on by a chore: full still, and nothing was written
    let (rig, args) = begun("work");
    assert!(!args.contains("--safe-mode") && args.ends_with("--resume 00000000-0000-4000-8000-0000000000b1"), "{args}");
    assert!(!rig.project_dir().join(".j4n/claude").exists(), "a session begun full leaves no record");
}

/// The profile's environment is the child's, over the connector's own, and a child of another profile is never shared with it.
#[test]
fn a_profile_adds_its_environment_to_the_child_and_two_profiles_do_not_share_a_child() {
    let body = r#"echo "start who=$WHO keep=$KEEP" >> "$FAKE_LOG"
IFS= read -r first
printf '%s\n' '{"type":"system","subtype":"init","session_id":"S1"}'
printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"ok","session_id":"S1","total_cost_usd":0,"usage":{"input_tokens":1,"output_tokens":1}}'
exit 0"#;
    for (pick, who) in [(Pick::Full, "full"), (Pick::Lean, "lean")] {
        let bin = Scratch::new("bin");
        let log = bin.join("log");
        let program = script(&bin, "claude", body);
        let mut cfg = config(&program, &[("FAKE_LOG", log.to_str().unwrap()), ("KEEP", "yes"), ("WHO", "row")]);
        profiled(pick)(&mut cfg);
        let rig = Rig::over(bin, cfg, |_| {});
        let id = rig.spawn("x");
        rig.finished(&id);
        assert_eq!(starts(&rig), [format!("who={who} keep=yes")], "{}", rig.dump());
    }
    let (a, b) = (started_as(Pick::Full, Some("work")), started_as(Pick::Lean, Some("work")));
    assert_ne!(a, b, "the argv is what a live child is known by: another profile is another child");
}

/// The row of the connectors names the roles a lean task is for, ships `lean` empty until it is measured, and the
/// setting's word is one of three.
#[test]
fn the_row_of_claude_names_its_lean_roles_and_a_word_that_is_no_profile_is_told() {
    let layer = Layer::new(LayerKind::Pack, "agents", vec![SourceFile { path: "connectors/agents.toml".into(), text: include_str!("../../../../packs/base/connectors/agents.toml").into() }]);
    let (list, said) = j4n_connect::manifest::Connectors::load(&Tables::load(vec![layer]).0);
    assert!(said.is_empty(), "{said:?}");
    let cfg = j4n_connect::claude::Config::from_connector(list.all().iter().find(|c| c.id == "claude").unwrap(), Default::default());
    assert_eq!(cfg.lean_for, ["chore", "answer", "light", "look"]);
    assert_eq!(cfg.pick, Pick::Full, "`full` until the operator says another");
    // The measured set, flag for flag: a change of it is a new measure, not an edit.
    assert_eq!(cfg.lean.args, ["--strict-mcp-config", "--disable-slash-commands", "--tools", "Read"]);
    let env: Vec<_> = cfg.lean.env.iter().map(|(k, v)| format!("{k}={v}")).collect();
    assert_eq!(
        env,
        [
            "CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS=1",
            "CLAUDE_CODE_DISABLE_AUTO_MEMORY=1",
            "CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1",
            "CLAUDE_CODE_DISABLE_BUNDLED_SKILLS=1",
            "CLAUDE_CODE_DISABLE_CLAUDE_MDS=1",
            "CLAUDE_CODE_DISABLE_CRON=1",
            "CLAUDE_CODE_DISABLE_GIT_INSTRUCTIONS=1",
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1",
        ]
    );
    assert!(cfg.full.args.is_empty() && cfg.full.env.is_empty(), "`full` adds nothing: it is the tool as it loads itself");
    assert_eq!(Pick::parse("auto"), Ok(Pick::Auto));
    let why = Pick::parse("tiny").unwrap_err();
    assert!(why.contains("`tiny`") && why.contains("full, auto or lean") && why.contains("full stands"), "{why}");
}

/// A row that names a profile that is not `full` or `lean` is refused with the name, and so is a profile on a doer whose options were never
/// looked up.
#[test]
fn a_profile_that_is_none_of_the_two_is_refused_and_so_is_one_on_another_doer() {
    let head = "api = 1\n[[connector]]\nid = \"c\"\nis = \"agent\"\nprogram = \"claude\"\n";
    let (list, said) = rows(&format!("{head}dialect = \"claude-stream\"\n[connector.profile.lean]\nargs = [\"--safe-mode\"]\n"));
    assert!(said.is_empty(), "{said:?}");
    assert_eq!(list.get("c").unwrap().profile.lean.args, ["--safe-mode"]);
    let (list, said) = rows(&format!("{head}dialect = \"claude-stream\"\n[connector.profile.tiny]\nargs = [\"--safe-mode\"]\n"));
    assert!(list.get("c").is_none(), "a row with a profile of no such name is no row");
    assert!(said.iter().any(|p| p.to_string().contains("tiny")), "the name is in the problem: {said:?}");
    let (list, said) = rows("api = 1\n[[connector]]\nid = \"o\"\nis = \"agent\"\nprogram = \"codex\"\ndialect = \"text\"\nstart = [\"{prompt}\"]\n[connector.profile.lean]\nargs = [\"--x\"]\n");
    assert!(list.get("o").is_none(), "{said:?}");
    assert!(said.iter().any(|p| p.to_string().contains("claude-stream")), "{said:?}");
}

// ---- the questions it asks ----

#[test]
fn an_ask_user_question_is_put_to_the_operator_as_a_choice_and_the_pick_goes_back_as_the_tools_own_label() {
    let rig = Rig::playing("ask-question-2.1.309.jsonl");
    let id = rig.spawn("ask me");
    let ask = rig.waiting_on(&id);
    let row = rig.tasks.book().get(&ask).unwrap();
    let spec = row.spec.ask().expect("a question");
    assert_eq!(spec.question, "Which colour do you prefer?");
    answer(&rig, &ask, Answer::Pick { id: "2".into() });
    done(&rig, &id);
    let [reply] = &responses(&rig)[..] else { panic!("{:?}", responses(&rig)) };
    assert_eq!(reply["behavior"], "allow");
    assert_eq!(reply["updatedInput"]["answers"]["Which colour do you prefer?"], "Blue", "option 2 is the label the tool defined");
    assert_eq!(reply["updatedInput"]["questions"][0]["header"], "Colour", "and the rest of the tool's input comes back with it");
}

#[test]
fn a_free_text_answer_to_a_question_is_the_answer() {
    let rig = Rig::playing("ask-question-2.1.309.jsonl");
    let id = rig.spawn("ask me");
    answer(&rig, &rig.waiting_on(&id), Answer::Text { text: "green, please".into() });
    done(&rig, &id);
    assert_eq!(responses(&rig)[0]["updatedInput"]["answers"]["Which colour do you prefer?"], "green, please");
}

#[test]
fn a_no_to_a_permission_question_is_a_deny_the_model_can_read() {
    let rig = Rig::playing("build-turn.jsonl");
    let id = rig.spawn("build");
    answer(&rig, &rig.waiting_on(&id), Answer::No);
    done(&rig, &id);
    assert_eq!(responses(&rig), vec![json!({ "behavior": "deny", "message": "this was not allowed" })]);
}

#[test]
fn several_questions_asked_at_once_are_put_one_at_a_time_in_the_order_they_came() {
    let bin = Scratch::new("bin");
    let log = bin.join("log");
    let program = script(
        &bin,
        "claude",
        r#"IFS= read -r first
printf '%s\n' '{"type":"system","subtype":"init","session_id":"S"}'
printf '%s\n' '{"type":"control_request","request_id":"r1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"make one"}}}'
printf '%s\n' '{"type":"control_request","request_id":"r2","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"make two"}}}'
IFS= read -r a; printf '%s\n' "$a" >> "$FAKE_LOG"
IFS= read -r b; printf '%s\n' "$b" >> "$FAKE_LOG"
printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"both","session_id":"S","total_cost_usd":0,"usage":{}}'
while IFS= read -r l; do :; done"#,
    );
    let rig = Rig::over(bin, config(&program, &[("FAKE_LOG", log.to_str().unwrap())]), |_| {});
    let id = rig.spawn("x");
    let first = rig.waiting_on(&id);
    assert_eq!(rig.tasks.book().get(&first).unwrap().spec.ask().unwrap().question, "allow Bash: make one?");
    answer(&rig, &first, Answer::Yes);
    let second = rig.until(&id, "waited on the second", |r| matches!(&r.state, State::Waiting { ask } if *ask != first));
    let State::Waiting { ask: second } = second.state else { unreachable!() };
    assert_eq!(rig.tasks.book().get(&second).unwrap().spec.ask().unwrap().question, "allow Bash: make two?");
    answer(&rig, &second, Answer::No);
    done(&rig, &id);
    let ids: Vec<(String, String)> = sent(&rig)
        .into_iter()
        .filter(|v| v["type"] == "control_response")
        .map(|v| (v["response"]["request_id"].as_str().unwrap().to_string(), v["response"]["response"]["behavior"].as_str().unwrap().to_string()))
        .collect();
    assert_eq!(ids, vec![("r1".to_string(), "allow".to_string()), ("r2".to_string(), "deny".to_string())]);
}

// ---- the Gate ----

/// The loop's Gate as the base pack makes it.
fn gate_tune(c: &mut j4n_core::task::Config) {
    const GATES: [(&str, &str); 4] = [
        ("shell.toml", include_str!("../../../../packs/base/gates/shell.toml")),
        ("modes.toml", include_str!("../../../../packs/base/gates/modes.toml")),
        ("paths.toml", include_str!("../../../../packs/base/gates/paths.toml")),
        ("egress.toml", include_str!("../../../../packs/base/gates/egress.toml")),
    ];
    const POINTS: &str = include_str!("../../../../packs/base/asks/points.toml");
    let (here, problems) = here_layer(&c.home, &c.root);
    assert!(problems.is_empty(), "{problems:?}");
    let files = |f: &[(&str, &str)]| f.iter().map(|(p, t)| SourceFile { path: p.to_string(), text: t.to_string() }).collect::<Vec<_>>();
    let layers = vec![Layer::new(LayerKind::Pack, "base", files(&GATES)), Layer::new(LayerKind::Pack, "asks", files(&[("points.toml", POINTS)])), here];
    let (tables, problems) = Tables::load(layers);
    assert!(problems.is_empty(), "{problems:?}");
    let (gates, notes) = Gates::load(&tables);
    assert!(notes.is_empty(), "{notes:?}");
    let (pts, notes) = points(&tables);
    assert!(notes.is_empty(), "{notes:?}");
    c.asks = Some(Arc::new(Asks::new(pts, Registry::new(), Answerers::new()).with_gate(Gate::new(gates, Grants::new("s1")))));
}

fn gated(fixture: &str) -> Rig {
    Rig::with(fixture, gate_tune)
}

#[test]
fn what_the_gate_blocks_claude_is_told_no_and_nobody_is_asked() {
    let rig = gated("push-turn.jsonl");
    let id = rig.spawn("push it");
    done(&rig, &id);
    // the model is told the rule's own words, not only that it was refused
    assert_eq!(responses(&rig), vec![json!({ "behavior": "deny", "message": "this was not allowed: J4n's gate refused it. pushing is yours" })]);
    let verdicts = events(&rig, &id, "gate.verdict");
    assert!(matches!(&verdicts[..], [v] if v["verdict"] == "block" && v["verb"] == "shell" && v["target"] == "git push origin main"), "{verdicts:?}");
}

#[test]
fn an_edit_in_a_read_task_is_blocked_by_the_gate_because_the_effect_carries_the_mode() {
    let rig = gated("edit-turn.jsonl");
    let mut spec = TaskSpec::new("look, do not touch", "claude");
    spec.tune.mode = Mode::Read;
    let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
    done(&rig, &id);
    assert_eq!(responses(&rig)[0]["behavior"], "deny");
    let verdicts = events(&rig, &id, "gate.verdict");
    assert!(matches!(&verdicts[..], [v] if v["verb"] == "write" && v["target"] == "src/a.rs" && v["verdict"] == "block"), "{verdicts:?}");
}

#[test]
fn an_edit_inside_the_project_in_a_task_that_may_write_is_the_operators_to_allow() {
    let rig = gated("edit-turn.jsonl");
    let id = rig.spawn("edit");
    let ask = rig.waiting_on(&id);
    answer(&rig, &ask, Answer::Yes);
    done(&rig, &id);
    assert_eq!(responses(&rig)[0]["behavior"], "allow");
    assert!(events(&rig, &id, "gate.verdict").is_empty(), "nobody objected, and nobody approved: the operator did");
}

#[test]
fn a_session_grant_for_a_command_answers_the_same_question_of_the_next_task() {
    let rig = gated("build-turn.jsonl");
    let first = rig.spawn("build");
    let ask = rig.waiting_on(&first);
    rig.tasks.ctl(&ask, Ctl::Grant(Scope::Session), Actor::Operator).unwrap();
    done(&rig, &first);
    // another task, another child, the same line: the Gate has the operator's yes and nobody is asked
    let second = rig.spawn("build again");
    done(&rig, &second);
    let said: Vec<Value> = events(&rig, &second, "ask.answered");
    assert_eq!(said.len(), 1, "{}", rig.dump());
    assert_eq!((said[0]["via"].as_str(), said[0]["by"].as_str()), (Some("chain"), Some("rule:gate")));
    assert_eq!(responses(&rig).iter().filter(|r| r["behavior"] == "allow").count(), 2);
}

// ---- the probe ----

#[test]
fn the_probe_says_logged_in_and_how() {
    let bin = Scratch::new("bin");
    let claude = Claude::new(config(&fake(&bin), &[]));
    let p = claude.probe();
    assert!(p.installed && p.authed, "{p:?}");
    assert_eq!(p.version.as_deref(), Some("2.1.309 (Claude Code)"));
    assert_eq!(p.detail, "logged in (claude.ai)");
    assert_eq!(claude.id(), "claude");
    assert!(claude.caps().live && claude.caps().resume);
}

#[test]
fn the_probe_of_a_logged_out_or_missing_cli_says_so_instead_of_pretending() {
    let bin = Scratch::new("bin");
    let out = script(&bin, "claude", r#"case "$1" in --version) echo "2.1.309 (Claude Code)";; auth) echo '{"loggedIn": false}';; esac"#);
    let p = Claude::new(config(&out, &[])).probe();
    assert!(p.installed && !p.authed && p.detail.contains("not logged in"), "{p:?}");
    let failing = script(&bin, "claude2", r#"case "$1" in --version) echo "2.1.309";; auth) echo nope >&2; exit 1;; esac"#);
    let p = Claude::new(config(&failing, &[])).probe();
    assert!(p.installed && !p.authed, "{p:?}");
    let broken = script(&bin, "claude3", r#"echo "2.1.309 (Claude Code)"; exit 3"#);
    let p = Claude::new(config(&broken, &[])).probe();
    assert!(!p.installed && p.version.is_none(), "a program that fails when asked its version is not an install of the CLI: {p:?}");
    let p = Claude::new(config(Path::new("/no/such/claude"), &[])).probe();
    assert!(!p.installed && !p.authed && p.version.is_none(), "{p:?}");
}

#[test]
fn describe_reports_the_login_a_key_that_bills_per_token_and_a_version_that_moved() {
    let ok = r#"{"loggedIn": true, "authMethod": "claude.ai"}"#;
    let (authed, detail) = describe(Some(ok), false, Some(&format!("{VERIFIED_VERSION} (Claude Code)")));
    assert!(authed);
    assert_eq!(detail, "logged in (claude.ai)");
    let (_, detail) = describe(Some(ok), true, Some("2.1.290 (Claude Code)"));
    assert!(detail.contains("ANTHROPIC_API_KEY") && detail.contains("2.1.290") && detail.contains(VERIFIED_VERSION), "{detail}");
    for bad in [None, Some("garbage"), Some(r#"{"loggedIn": false}"#), Some(r#"{"authMethod": "claude.ai"}"#)] {
        let (authed, detail) = describe(bad, true, Some(VERIFIED_VERSION));
        assert!(!authed && detail.contains("not logged in"), "{detail}");
    }
    let (_, detail) = describe(Some(r#"{"loggedIn": true}"#), false, None);
    assert_eq!(detail, "logged in (unknown)");
}

#[test]
fn a_refused_login_in_a_turn_fails_the_task_with_the_clis_words() {
    let rig = Rig::playing("a0-2.1.296.jsonl");
    let id = rig.spawn("x");
    let row = rig.finished(&id);
    let State::Stopped { why: Why::Failed { message } } = &row.state else { panic!("{:?}\n{}", row.state, rig.dump()) };
    assert!(message.contains("Failed to authenticate"), "{message}");
    assert!(fixtures().join("a0-2.1.296.jsonl").exists());
}

// ---- the corners of a conversation ----

const INIT: &str = r#"printf '%s\n' '{"type":"system","subtype":"init","session_id":"S"}'"#;

fn result(text: &str) -> String {
    format!(r#"printf '%s\n' '{{"type":"result","subtype":"success","is_error":false,"result":"{text}","session_id":"S","total_cost_usd":0,"usage":{{}}}}'"#)
}

fn request(id: &str, tool: &str, input: &str) -> String {
    format!(r#"printf '%s\n' '{{"type":"control_request","request_id":"{id}","request":{{"subtype":"can_use_tool","tool_name":"{tool}","input":{input}}}}}'"#)
}

const REST: &str = r#"while IFS= read -r l; do :; done"#;

#[test]
fn a_tool_that_asks_several_questions_puts_them_one_at_a_time_and_answers_the_child_once() {
    let questions = r#"{"questions":[{"question":"First?","options":[{"label":"x"},{"label":"y"}]},{"question":"Second?","options":[{"label":"p"},{"label":"q"}]}]}"#;
    let body = format!("IFS= read -r first\n{INIT}\n{}\nIFS= read -r a; printf '%s\\n' \"$a\" >> \"$FAKE_LOG\"\n{}\n{REST}", request("r1", "AskUserQuestion", questions), result("done"));
    let rig = Rig::scripted(&body);
    let id = rig.spawn("x");
    let first = rig.waiting_on(&id);
    assert_eq!(rig.tasks.book().get(&first).unwrap().spec.ask().unwrap().question, "First?");
    answer(&rig, &first, Answer::Pick { id: "2".into() });
    let second = rig.until(&id, "waited on the second question", |r| matches!(&r.state, State::Waiting { ask } if *ask != first));
    let State::Waiting { ask: second } = second.state else { unreachable!() };
    assert_eq!(rig.tasks.book().get(&second).unwrap().spec.ask().unwrap().question, "Second?");
    assert!(responses(&rig).is_empty(), "the child waits for all of them");
    answer(&rig, &second, Answer::Pick { id: "1".into() });
    done(&rig, &id);
    let [reply] = &responses(&rig)[..] else { panic!("{:?}", responses(&rig)) };
    assert_eq!(reply["updatedInput"]["answers"], json!({ "First?": "y", "Second?": "p" }));
}

#[test]
fn a_child_that_ended_between_turns_is_not_reused_and_its_session_is_resumed() {
    let body = format!("echo \"start $*\" >> \"$FAKE_LOG\"\nIFS= read -r first\n{INIT}\n{}\nexit 0", result("one"));
    let rig = Rig::scripted(&body);
    let first = rig.spawn("one");
    done(&rig, &first);
    assert!((0..300).any(|_| {
        std::thread::sleep(Duration::from_millis(10));
        rig.claude.live() == 0
    }), "the child left");
    let second = rig.tasks.resume(&first, Some("again"), Actor::Operator).unwrap();
    done(&rig, &second);
    let all = starts(&rig);
    assert!(all.len() == 2 && all[1].ends_with("--resume S"), "{all:?}");
    assert!(events(&rig, &second, "run.notice").is_empty(), "a child that ended on its own is no news: it was dropped before it could be chosen");
}

#[test]
fn an_answer_to_a_question_the_child_took_back_is_not_written_to_it() {
    let body = format!("IFS= read -r first\n{INIT}\n{}\nprintf '%s\\n' '{{\"type\":\"control_cancel_request\",\"request_id\":\"r1\"}}'\nIFS= read -r a; printf '%s\\n' \"$a\" >> \"$FAKE_LOG\"\n{}\n{REST}", request("r1", "Bash", r#"{"command":"make"}"#), result("done"));
    let rig = Rig::scripted(&body);
    let id = rig.spawn("x");
    let ask = rig.waiting_on(&id);
    std::thread::sleep(Duration::from_millis(150));
    answer(&rig, &ask, Answer::Yes);
    let row = rig.finished(&id);
    let State::Stopped { why: Why::Failed { message } } = &row.state else { panic!("{:?}\n{}", row.state, rig.dump()) };
    assert!(message.contains("could not be given"), "{message}");
    assert!(responses(&rig).is_empty(), "nothing was written to the child");
}

#[test]
fn a_task_hears_of_its_session_once() {
    let rig = Rig::playing("two-turns.jsonl");
    let first = rig.spawn("one");
    done(&rig, &first);
    let second = rig.tasks.resume(&first, Some("again"), Actor::Operator).unwrap();
    done(&rig, &second);
    for t in [&first, &second] {
        let said = events(&rig, t, "run.session");
        assert!(matches!(&said[..], [s] if s["id"] == "00000000-0000-4000-8000-0000000000b1"), "{t}: {said:?}");
    }
}

#[test]
fn a_session_that_another_task_has_is_not_shared() {
    let body = format!("IFS= read -r first\n{INIT}\n{}\nIFS= read -r a\n{}\n{REST}", request("r1", "Bash", r#"{"command":"make"}"#), result("done"));
    let rig = Rig::scripted(&body);
    let holder = rig.spawn("x");
    let ask = rig.waiting_on(&holder);
    let session = rig.tasks.book().get(&holder).unwrap().session.expect("it knows its session by now");
    let mut spec = TaskSpec::new("me too", "claude");
    spec.resume = Some(session);
    let other = rig.tasks.spawn(spec, Actor::Operator).unwrap();
    let row = rig.finished(&other);
    let State::Stopped { why: Why::Failed { message } } = &row.state else { panic!("{:?}", row.state) };
    assert!(message.contains("busy with another task"), "{message}");
    answer(&rig, &ask, Answer::Yes);
    done(&rig, &holder);
}

#[test]
fn a_child_idle_for_longer_than_the_pool_keeps_it_goes_when_the_next_one_starts() {
    let rig = Rig::playing_with("two-turns.jsonl", |cfg| cfg.idle = Duration::from_millis(60));
    let first = rig.spawn("one");
    done(&rig, &first);
    assert_eq!(rig.claude.live(), 1);
    std::thread::sleep(Duration::from_millis(200));
    let second = rig.spawn("two");
    done(&rig, &second);
    assert_eq!(rig.claude.live(), 1, "the first child was reaped at the second start: {}", rig.dump());
}

#[test]
fn a_stopped_task_s_session_is_resumed_in_a_new_child_without_a_word_about_a_live_one() {
    let body = format!("echo \"start $*\" >> \"$FAKE_LOG\"\nIFS= read -r first\n{INIT}\nsleep 300");
    let rig = Rig::scripted(&body);
    let id = rig.spawn("x");
    rig.until_event("the run's session", |e| e.kind == "run.session");
    rig.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
    rig.finished(&id);
    let second = rig.tasks.resume(&id, Some("again"), Actor::Operator).unwrap();
    rig.until_event("the second run's session", |e| e.kind == "run.session" && e.task.as_deref() == Some(second.as_str()));
    let all = starts(&rig);
    assert!(all.len() == 2 && all[1].ends_with("--resume S"), "{all:?}");
    assert!(events(&rig, &second, "run.notice").is_empty(), "the stopped child was taken out of the pool, not left there dead");
    rig.tasks.ctl(&second, Ctl::Abort, Actor::Operator).unwrap();
}

#[test]
fn a_child_that_has_only_just_started_is_not_sent_away_when_the_pool_is_full_of_busy_ones() {
    let body = format!("IFS= read -r first\n{INIT}\n{}\nIFS= read -r a\n{}\n{REST}", request("r1", "Bash", r#"{"command":"make"}"#), result("done"));
    let bin = Scratch::new("bin");
    let program = script(&bin, "claude", &body);
    let mut cfg = config(&program, &[]);
    cfg.live_max = 1;
    let rig = Rig::over(bin, cfg, |_| {});
    let (a, b) = (rig.spawn("a"), rig.spawn("b"));
    let (ask_a, ask_b) = (rig.waiting_on(&a), rig.waiting_on(&b));
    assert_eq!(rig.claude.live(), 2, "both work: the pool is over its size and nobody is sent away");
    answer(&rig, &ask_a, Answer::Yes);
    answer(&rig, &ask_b, Answer::Yes);
    done(&rig, &a);
    done(&rig, &b);
}

#[test]
fn a_model_with_nothing_in_it_is_not_passed_on_and_a_goal_with_nothing_in_it_reaches_no_child() {
    let rig = Rig::playing("two-turns.jsonl");
    let mut spec = TaskSpec::new("fine", "claude");
    spec.tune.model = Some(String::new());
    let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
    done(&rig, &id);
    assert!(!starts(&rig)[0].contains("--model"), "{}", starts(&rig)[0]);
    let blank = rig.tasks.spawn(TaskSpec::new("   ", "claude"), Actor::Operator);
    match blank {
        Err(_) => {}
        Ok(id) => {
            rig.finished(&id);
        }
    }
    assert_eq!(users(&rig), vec!["fine"], "no empty message reached a child");
}

#[test]
fn a_child_launched_under_other_settings_is_replaced_and_the_conversation_goes_on_in_a_new_one() {
    let rig = Rig::playing("two-turns.jsonl");
    let first = rig.spawn("one");
    let session = done(&rig, &first).session.expect("a session");
    let mut spec = TaskSpec::new("again", "claude");
    spec.resume = Some(session);
    spec.tune.model = Some("sonnet".into());
    let second = rig.tasks.spawn(spec, Actor::Operator).unwrap();
    done(&rig, &second);
    let all = starts(&rig);
    assert!(all.len() == 2 && all[1].contains("--model sonnet") && all[1].ends_with("--resume 00000000-0000-4000-8000-0000000000b1"), "{all:?}");
    let notices = events(&rig, &second, "run.notice");
    assert!(notices.iter().any(|n| n["text"].as_str().is_some_and(|t| t.contains("launched under other settings"))), "{notices:?}");
    assert_eq!(rig.claude.live(), 1, "the old child is gone, not kept beside the new one");
}

#[test]
fn a_turn_that_writes_in_a_conversation_resumed_from_a_reading_one_is_told_it_may_write() {
    // Main's planning turn reads only; its work turn resumes the conversation and must not answer "this task is read-only"
    let rig = Rig::playing("two-turns.jsonl");
    let mut plan = TaskSpec::new("plan it, change nothing", "claude");
    plan.tune.mode = Mode::Read;
    let first = rig.tasks.spawn(plan, Actor::Operator).unwrap();
    let session = done(&rig, &first).session.expect("a session");
    let mut work = TaskSpec::new("now build it", "claude");
    work.resume = Some(session);
    work.tune.mode = Mode::Write;
    let second = rig.tasks.spawn(work, Actor::Operator).unwrap();
    done(&rig, &second);
    let said = users(&rig);
    assert_eq!(said.len(), 2, "{said:?}");
    assert!(said[1].starts_with("(J4n: this turn may change files and run commands") && said[1].ends_with("now build it"), "{said:?}");
    assert_eq!(said[0], "plan it, change nothing", "a first turn is sent as it is");
}

#[test]
fn a_retune_resumes_the_session_in_a_child_started_with_the_new_settings() {
    let body = format!("echo \"start $*\" >> \"$FAKE_LOG\"\nIFS= read -r first\n{INIT}\nsleep 300");
    let rig = Rig::scripted(&body);
    let id = rig.spawn("x");
    rig.until_event("the run's session", |e| e.kind == "run.session");
    rig.tasks.ctl(&id, Ctl::Retune(Tune { model: Some("sonnet".into()), endpoint: None, effort: Some("high".into()), mode: Mode::Write }), Actor::Operator).unwrap();
    rig.until_event("the second child", |e| e.kind == "run.session" && e.seq > rig.events().iter().find(|x| x.kind == "task.retuned").map_or(u64::MAX, |x| x.seq));
    let all = starts(&rig);
    assert!(all.len() == 2 && !all[0].contains("--model"), "{all:?}");
    assert!(all[1].contains("--model sonnet") && all[1].contains("--effort high") && all[1].ends_with("--resume S"), "{}", all[1]);
    assert_eq!(rig.claude.live(), 1, "the old child is gone");
    rig.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
}

// ---- the window and shrinking a session ----

fn assistant_using(tokens: u64) -> String {
    format!(r#"printf '%s\n' '{{"type":"assistant","session_id":"S","message":{{"content":[{{"type":"text","text":"x"}}],"usage":{{"input_tokens":1,"cache_read_input_tokens":{tokens}}}}}}}'"#)
}

#[test]
fn a_session_is_shrunk_by_the_clis_own_command_in_the_same_child_and_the_task_goes_on_after_it() {
    let read = |n: &str| format!("IFS= read -r {n}; printf '%s\\n' \"${n}\" >> \"$FAKE_LOG\"");
    let body = format!(
        "echo \"start $*\" >> \"$FAKE_LOG\"\n{}\n{INIT}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{REST}",
        read("a"),
        assistant_using(150_000),
        result("one"),
        read("b"),
        result("compacted"),
        read("c"),
        assistant_using(40_000),
        result("two"),
    );
    let rig = Rig::scripted(&body);
    let first = rig.spawn("one");
    let row = done(&rig, &first);
    let settled = events(&rig, &first, "task.settled");
    assert_eq!(settled[0]["window"].as_f64(), Some(150_001.0 / 200_000.0), "the doer said how full its window was, and the task settled with it");
    let mut spec = TaskSpec::new("carry on", "claude");
    spec.resume = row.session.clone();
    spec.compact = Some(vec!["never touch vendor/".into()]);
    let second = rig.tasks.spawn(spec, Actor::Operator).unwrap();
    let row = done(&rig, &second);
    assert_eq!(row.answer.as_deref(), Some("two"), "the compaction's own report is not the answer");
    assert_eq!(starts(&rig).len(), 1, "one child throughout: {:?}", rig.sent());
    let said = users(&rig);
    assert_eq!(said.len(), 3, "{said:?}");
    assert!(said[1].starts_with("/compact MUST KEEP (re-stated after compaction):\n- goal: carry on"), "{}", said[1]);
    assert!(said[1].contains("never touch vendor/"), "{}", said[1]);
    assert!(said[2].starts_with("carry on\n\nMUST KEEP"), "the task's own words come after, with what must stay: {}", said[2]);
    assert_eq!(events(&rig, &second, "task.compacted")[0]["ok"], true);
    assert_eq!(events(&rig, &second, "task.settled")[0]["window"].as_f64(), Some(40_001.0 / 200_000.0), "and the window it ended with is the one after");
}

#[test]
fn a_session_that_will_not_shrink_is_a_note_and_the_task_goes_on() {
    let read = |n: &str| format!("IFS= read -r {n}; printf '%s\\n' \"${n}\" >> \"$FAKE_LOG\"");
    let refused = r#"printf '%s\n' '{"type":"result","subtype":"success","is_error":true,"result":"Not enough messages to compact","session_id":"S","total_cost_usd":0,"usage":{}}'"#;
    let body = format!("{}\n{INIT}\n{}\n{}\n{refused}\n{}\n{}\n{REST}", read("a"), result("one"), read("b"), read("c"), result("two"));
    let rig = Rig::scripted(&body);
    let first = rig.spawn("one");
    let row = done(&rig, &first);
    let mut spec = TaskSpec::new("carry on", "claude");
    spec.resume = row.session.clone();
    spec.compact = Some(vec![]);
    let second = rig.tasks.spawn(spec, Actor::Operator).unwrap();
    assert_eq!(done(&rig, &second).answer.as_deref(), Some("two"));
    let notices: Vec<String> = events(&rig, &second, "run.notice").iter().map(|d| d["text"].as_str().unwrap().to_string()).collect();
    assert!(notices.iter().any(|n| n.contains("could not be shrunk") && n.contains("Not enough messages to compact")), "{notices:?}");
    assert_eq!(events(&rig, &second, "task.compacted")[0]["ok"], false);
}

// ---- the doer's list carries a turn on ----

/// A rig that plays `fixture` with the base pack's loop rules and settings, so `followups` carries a turn on while items are open.
fn followups_rig(fixture: &str) -> Rig {
    Rig::with(fixture, |c| {
        const BASE: [(&str, &str); 6] = [
            ("failures/table.toml", include_str!("../../../../packs/base/failures/table.toml")),
            ("folds/spend.toml", include_str!("../../../../packs/base/folds/spend.toml")),
            ("rules/loop.toml", include_str!("../../../../packs/base/rules/loop.toml")),
            ("settings/cost.toml", include_str!("../../../../packs/base/settings/cost.toml")),
            ("settings/loop.toml", include_str!("../../../../packs/base/settings/loop.toml")),
            ("settings/work.toml", include_str!("../../../../packs/base/settings/work.toml")),
        ];
        let files = BASE.iter().map(|(p, t)| SourceFile { path: p.to_string(), text: t.to_string() }).collect();
        let (tables, problems) = Tables::load(vec![Layer::new(LayerKind::Pack, "base", files)]);
        assert!(problems.is_empty(), "{problems:?}");
        let (rules, problems) = j4n_core::rule::Rules::load(&tables);
        assert!(problems.is_empty(), "{problems:?}");
        let (settings, problems) = j4n_core::setting::Settings::load(&tables);
        assert!(problems.is_empty(), "{problems:?}");
        (c.rules, c.settings) = (Arc::new(rules), settings);
    })
}

#[test]
fn open_items_of_the_doers_own_list_make_followups_start_a_carried_on_turn() {
    let rig = followups_rig("tasks-open-2.1.310.jsonl");
    let id = rig.spawn("do the three things");
    let first = done(&rig, &id);
    assert_eq!(first.todo_left, 2, "three TaskCreate and one TaskUpdate completed: {}", rig.dump());
    let second = done(&rig, "t2");
    assert_eq!((second.spec.follows.as_deref(), second.auto, second.todo_left), (Some(id.as_str()), 1, 0), "{}", rig.dump());
    let queued = rig.events().into_iter().find(|e| e.kind == "task.queued" && e.task.as_deref() == Some("t2")).expect("carried on");
    assert_eq!(queued.by, Actor::Rule("followups".into()));
    assert_eq!(users(&rig), vec!["do the three things", "2 item(s) are still open. Carry on and finish them."], "in the same session, so the same child");
    assert_eq!(starts(&rig).len(), 1);
}

// ---- the doer's list is one row, said again in place ----

#[test]
fn the_doers_list_is_one_row_of_its_lane_said_again_in_place_and_folded_when_done() {
    use j4n_core::front::rows::fold;
    let rig = followups_rig("tasks-active-2.1.310.jsonl");
    let id = rig.spawn("do the three things");
    assert_eq!(done(&rig, &id).todo_left, 2, "{}", rig.dump());
    done(&rig, "t2");
    let lists = |task: &str| events(&rig, task, "run.list").iter().map(|d| format!("{}/{} {}", d["open"], d["total"], d["active"].as_str().unwrap())).collect::<Vec<_>>();
    assert_eq!(lists(&id), ["1/1 ", "2/2 ", "3/3 ", "3/3 writing the parser", "2/3 ", "2/3 writing tests"], "a change each, and nothing said twice");
    assert_eq!(lists("t2"), ["1/3 ", "1/3 updating the docs", "0/3 "]);
    let events = rig.events();
    let rows = fold(events.iter()).rows();
    let of = |lane: &str| -> Vec<(String, String)> { rows.iter().filter(|r| r.parent.as_deref() == Some(lane)).filter(|r| r.cells[0].icon.starts_with("todo")).map(|r| (r.cells[0].icon.clone(), r.cells[0].text.clone())).collect() };
    assert_eq!(of(&id), [("todo".to_string(), "2 of 3 left · writing tests".to_string())], "one row for seven changes of the list");
    assert_eq!(of("t2"), [("todo-done".to_string(), "3 done".to_string())], "the second turn is a lane of its own; its list is done");
    assert_eq!(rows.iter().filter(|r| r.cells[0].icon.starts_with("todo")).count(), 2, "{rows:?}");
}

// ---- a doer without a list shows no row ----

#[test]
fn a_run_with_no_list_tool_writes_no_list_event_and_has_no_list_row() {
    use j4n_core::front::rows::fold;
    let rig = Rig::playing("narration-report.jsonl");
    let id = rig.spawn("run it");
    done(&rig, &id);
    let events = rig.events();
    assert!(events.iter().all(|e| e.kind != "run.list"));
    assert!(fold(events.iter()).rows().iter().all(|r| !r.cells[0].icon.starts_with("todo")));
}

// ---- the model that ran ----

/// A scripted child that says `init` with `model` and ends its turn with a `result` whose `modelUsage` has `usage`.
fn serving(init_model: &str, usage: &str) -> String {
    let init = format!(r#"printf '%s\n' '{{"type":"system","subtype":"init","session_id":"S","model":"{init_model}"}}'"#);
    let result = format!(
        r#"printf '%s\n' '{{"type":"result","subtype":"success","is_error":false,"result":"done","session_id":"S","total_cost_usd":0.002,"usage":{{"input_tokens":30,"output_tokens":7}},"modelUsage":{usage}}}'"#
    );
    format!("IFS= read -r first\n{init}\n{result}\n{REST}")
}

#[test]
fn with_no_model_asked_for_the_model_that_served_is_in_the_log_the_spend_and_the_task_s_row() {
    let rig = Rig::scripted(&serving("claude-haiku-5-5", r#"{"claude-haiku-5-5":{"costUSD":0.002,"contextWindow":200000}}"#));
    let id = rig.spawn("x");
    let row = done(&rig, &id);
    let served = events(&rig, &id, "run.served");
    assert_eq!(served.len(), 1, "once, not on every frame\n{}", rig.dump());
    assert_eq!((served[0]["model"].as_str(), served[0]["host"].as_str()), (Some("claude-haiku-5-5"), Some("anthropic")));
    assert_eq!(events(&rig, &id, "run.metrics")[0]["model"], "claude-haiku-5-5", "spend is per model");
    assert_eq!(row.spec.tune.model, None, "nothing was asked for");
    assert_eq!(row.model, "claude-haiku-5-5");
    assert_eq!(row.who(), "claude · haiku 5.5");
    let bound = rig.events().into_iter().position(|e| e.kind == "run.served").unwrap() < rig.events().into_iter().position(|e| e.kind == "run.metrics").unwrap();
    assert!(bound, "who served is said before what it cost");
}

#[test]
fn a_pin_that_resolves_to_a_full_id_shows_the_full_id_and_the_pin_stays_in_the_spec() {
    let rig = Rig::scripted(&serving("claude-haiku-5-5", r#"{"claude-haiku-5-5":{"costUSD":0.002}}"#));
    let mut spec = TaskSpec::new("x", "claude");
    spec.tune.model = Some("haiku".into());
    let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
    assert_eq!(rig.tasks.book().get(&id).unwrap().who(), "claude · haiku", "until it is served, what was asked for");
    let row = done(&rig, &id);
    assert_eq!(row.who(), "claude · haiku 5.5");
    assert_eq!(row.spec.tune.model.as_deref(), Some("haiku"), "the pin is what was asked for");
}

#[test]
fn a_model_that_did_the_turn_and_is_not_the_one_init_named_is_said_as_a_change() {
    let usage = r#"{"claude-sonnet-5-5":{"costUSD":0.5},"claude-haiku-5-5":{"costUSD":0.01}}"#;
    let rig = Rig::scripted(&serving("claude-haiku-5-5", usage));
    let id = rig.spawn("x");
    let row = done(&rig, &id);
    let models: Vec<Value> = events(&rig, &id, "run.served").iter().map(|d| d["model"].clone()).collect();
    assert_eq!(models, [json!("claude-haiku-5-5"), json!("claude-sonnet-5-5")], "{}", rig.dump());
    assert_eq!(row.who(), "claude · sonnet 5.5", "what ran, not what init said");
}

#[test]
fn before_the_model_is_known_the_row_says_so_and_never_the_bare_doer() {
    let rig = Rig::scripted(&format!("IFS= read -r first\n{INIT}\n{}\n{REST}", result("done")));
    let id = rig.spawn("x");
    let row = done(&rig, &id);
    assert!(events(&rig, &id, "run.served").is_empty());
    assert_eq!(row.who(), "claude · model not known yet");
}

// ---- the operator's own allow rules do not answer for the Gate ----

/// The `--settings` text a child was started with, as JSON.
fn door_of(args: &str) -> Value {
    let at = args.find("--settings ").unwrap_or_else(|| panic!("no --settings in {args}")) + "--settings ".len();
    let text = &args[at..];
    let end = text.find("} --").map_or(text.len(), |e| e + 1);
    serde_json::from_str(&text[..end]).unwrap_or_else(|e| panic!("{e}: {}", &text[..end]))
}

#[test]
fn every_child_is_started_with_settings_that_ask_for_what_the_gate_judges_and_deny_the_credentials() {
    for mode in [Mode::Write, Mode::Read] {
        let rig = Rig::playing("two-turns.jsonl");
        let mut spec = TaskSpec::new("x", "claude");
        spec.tune.mode = mode;
        let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
        done(&rig, &id);
        let door = door_of(&starts(&rig)[0]);
        let list = |k: &str| door["permissions"][k].as_array().unwrap_or_else(|| panic!("{door}")).iter().map(|v| v.as_str().unwrap().to_string()).collect::<Vec<_>>();
        let (ask, deny) = (list("ask"), list("deny"));
        for tool in ["Bash", "Edit", "Write", "NotebookEdit", "WebFetch"] {
            assert!(ask.iter().any(|a| a == tool), "{tool} is asked: {ask:?}");
        }
        for path in ["~/.ssh/**", "~/.aws/**", "**/.env", "**/.env.production", "**/*.pem", "~/.j4n/secrets/**", "~/.claude.json"] {
            assert!(deny.iter().any(|d| d == &format!("Read({path})")), "{path} is denied for reading: {deny:?}");
        }
    }
}

#[test]
fn the_list_of_denied_paths_has_a_rule_for_every_path_the_shell_matcher_calls_a_credential() {
    assert_eq!(j4n_core::gate::door::uncovered(), Vec::<&str>::new(), "a path was added to the matcher's SECRETS and not to the door");
}

#[test]
fn a_project_that_allows_every_command_and_read_still_has_each_command_decided_by_the_gate_and_a_credential_denied_before_it_is_read() {
    // the child models the CLI as far as it matters here: the project's rules allow `Bash(*)` and `Read(**)`; an ask or a deny of the settings it was given beats
    // an allow (the order of the CLI: deny, ask, allow). A call that is asked is put to the loop as a question; a read of `.env` is read unless a deny matches.
    let ask = |id: &str, cmd: &str| request(id, "Bash", &format!(r#"{{"command":"{cmd}"}}"#));
    let body = format!(
        r#"echo "start $*" >> "$FAKE_LOG"
IFS= read -r first
{INIT}
case "$*" in *'"ask":["Bash"'*) ASKED=1;; esac
if [ -n "$ASKED" ]; then
{}
IFS= read -r a; printf '%s\n' "$a" >> "$FAKE_LOG"
{}
IFS= read -r b; printf '%s\n' "$b" >> "$FAKE_LOG"
else echo RAN-WITHOUT-THE-GATE >> "$FAKE_LOG"; fi
case "$*" in *'Read(**/.env)'*) echo read-denied >> "$FAKE_LOG";; *) echo "READ-ENV $(cat .env)" >> "$FAKE_LOG";; esac
{}
{REST}"#,
        ask("r1", "cat .env.production"),
        ask("r2", "echo marker"),
        result("done")
    );
    let bin = Scratch::new("bin");
    let log = bin.join("log");
    let program = script(&bin, "claude", &body);
    let rig = Rig::over(bin, config(&program, &[("FAKE_LOG", log.to_str().unwrap())]), gate_tune);
    std::fs::create_dir_all(rig.project_dir().join(".claude")).unwrap();
    std::fs::write(rig.project_dir().join(".claude/settings.local.json"), r#"{"permissions":{"allow":["Bash(*)","Read(**)"]}}"#).unwrap();
    std::fs::write(rig.project_dir().join(".env"), "MARKER_VALUE=1\n").unwrap();
    let id = rig.spawn("x");
    done(&rig, &id);
    let logged = rig.bin.read("log");
    assert!(!logged.contains("RAN-WITHOUT-THE-GATE"), "{logged}");
    assert!(logged.contains("read-denied") && !logged.contains("READ-ENV") && !logged.contains("MARKER_VALUE"), "{logged}");
    let verdicts: Vec<(String, String)> = events(&rig, &id, "gate.verdict").iter().map(|v| (v["verdict"].as_str().unwrap().into(), v["target"].as_str().unwrap().into())).collect();
    assert_eq!(verdicts, [("block".to_string(), "cat .env.production".to_string()), ("allow".to_string(), "echo marker".to_string())], "{}", rig.dump());
    let sent = responses(&rig);
    assert_eq!(sent[0]["behavior"], "deny");
    assert!(sent[0]["message"].as_str().unwrap().contains("a lane never reads it"), "the model is told why: {sent:?}");
    assert_eq!(sent[1]["behavior"], "allow");
}

// ---- a Read of the project's state is asked of the Gate, which answers the doer's own folder with no card and another project's with a block ----

#[test]
fn a_read_of_its_own_transcript_folder_is_allowed_with_no_card_and_another_projects_is_blocked() {
    let read = |id: &str, path: &str| request(id, "Read", &format!(r#"{{"file_path":"{path}"}}"#));
    // the child names its own folder as the CLI does: its working directory with every character that is no letter or digit a dash
    let body = format!(
        r#"IFS= read -r first
{INIT}
ENC=$(pwd | sed 's/[^a-zA-Z0-9]/-/g')
printf '%s\n' "{{\"type\":\"control_request\",\"request_id\":\"r1\",\"request\":{{\"subtype\":\"can_use_tool\",\"tool_name\":\"Read\",\"input\":{{\"file_path\":\"/h/.claude/projects/$ENC/memory/MEMORY.md\"}}}}}}"
IFS= read -r a; printf '%s\n' "$a" >> "$FAKE_LOG"
{}
IFS= read -r b; printf '%s\n' "$b" >> "$FAKE_LOG"
{}
{REST}"#,
        read("r2", "/h/.claude/projects/-h-Projects-other/9c1.jsonl"),
        result("done")
    );
    let bin = Scratch::new("bin");
    let log = bin.join("log");
    let program = script(&bin, "claude", &body);
    let rig = Rig::over(bin, config(&program, &[("FAKE_LOG", log.to_str().unwrap())]), gate_tune);
    let id = rig.spawn("x");
    done(&rig, &id);
    let verdicts: Vec<(String, String)> = events(&rig, &id, "gate.verdict").iter().map(|v| (v["verdict"].as_str().unwrap().into(), v["target"].as_str().unwrap().into())).collect();
    assert_eq!(verdicts.iter().map(|v| v.0.as_str()).collect::<Vec<_>>(), ["allow", "block"], "{verdicts:?} {}", rig.dump());
    let sent = responses(&rig);
    assert_eq!((sent[0]["behavior"].as_str(), sent[1]["behavior"].as_str()), (Some("allow"), Some("deny")), "{sent:?}");
}

// ---- a remote shell goes through the relay ----

#[test]
fn a_remote_shell_the_operator_allows_runs_inside_the_relay_and_the_card_says_so() {
    let ask = |id: &str, cmd: &str| request(id, "Bash", &format!(r#"{{"command":"{cmd}"}}"#));
    let body = format!(
        "IFS= read -r first\n{INIT}\n{}\nIFS= read -r a; printf '%s\\n' \"$a\" >> \"$FAKE_LOG\"\n{}\nIFS= read -r b; printf '%s\\n' \"$b\" >> \"$FAKE_LOG\"\n{}\n{REST}",
        ask("r1", "ssh h cat /srv/app.conf | tail -5"),
        ask("r2", "ssh h cat /srv/x > out.txt"),
        result("done")
    );
    let bin = Scratch::new("bin");
    let log = bin.join("log");
    let program = script(&bin, "claude", &body);
    let mut cfg = config(&program, &[("FAKE_LOG", log.to_str().unwrap())]);
    cfg.relay = Some("/opt/j4n/j4n".into());
    let rig = Rig::over(bin, cfg, |_| {});
    let id = rig.spawn("x");
    let first = rig.waiting_on(&id);
    let q = rig.tasks.book().get(&first).unwrap().spec.ask().unwrap().question.clone();
    assert!(q.contains("goes through J4n's redaction"), "the card says the line is routed: {q}");
    answer(&rig, &first, Answer::Yes);
    let second = rig.until(&id, "waited on the second question", |r| matches!(&r.state, State::Waiting { ask } if *ask != first));
    let State::Waiting { ask: second } = second.state else { unreachable!() };
    let q = rig.tasks.book().get(&second).unwrap().spec.ask().unwrap().question.clone();
    assert!(!q.contains("redaction"), "a line J4n cannot wrap is not said to be: {q}");
    answer(&rig, &second, Answer::Yes);
    done(&rig, &id);
    let sent = responses(&rig);
    assert_eq!(sent[0]["updatedInput"]["command"], "'/opt/j4n/j4n' relay -- bash -c 'ssh h cat /srv/app.conf | tail -5'", "the whole line runs inside the relay, as written");
    assert_eq!(sent[1]["updatedInput"]["command"], "ssh h cat /srv/x > out.txt", "what cannot be wrapped runs as it is, after a yes");
    let notices = events(&rig, &id, "run.notice");
    assert!(notices.iter().any(|n| n["text"].as_str().is_some_and(|t| t.contains("redaction"))), "{notices:?}");
}

/// The claude doer's child is started without the host's session. The names are set in the test process and no other test of this binary
/// reads them; the fake `claude` writes the environment it was started with.
#[test]
fn the_claude_child_does_not_see_the_apps_socket_token_and_session_and_does_see_what_j4n_sets() {
    // SAFETY: only this test sets or reads these names in this binary
    unsafe {
        std::env::set_var("CLAUDE_CODE_MESSAGING_SOCKET", "/run/user/1/cc-socks/1.sock");
        std::env::set_var("CLAUDE_CODE_MESSAGING_TOKEN", "tok-of-the-app");
        std::env::set_var("CLAUDECODE", "1");
    }
    let rig = Rig::playing_with("two-turns.jsonl", |cfg| {
        let log = std::env::temp_dir().join(format!("t704-env-{}", std::process::id()));
        cfg.env.insert("FAKE_ENV_LOG".into(), log.display().to_string());
        cfg.env.insert("CLAUDE_CODE_OAUTH_TOKEN".into(), "tok-of-the-account".into());
    });
    let id = rig.spawn("look");
    done(&rig, &id);
    let path = std::env::temp_dir().join(format!("t704-env-{}", std::process::id()));
    let env = std::fs::read_to_string(&path).expect("the child wrote its environment");
    let _ = std::fs::remove_file(&path);
    for name in ["CLAUDE_CODE_MESSAGING_SOCKET", "CLAUDE_CODE_MESSAGING_TOKEN", "CLAUDECODE"] {
        assert!(!env.lines().any(|l| l.starts_with(&format!("{name}="))), "{name} reached the child:\n{env}");
    }
    assert!(env.contains("CLAUDE_CODE_OAUTH_TOKEN=tok-of-the-account") && env.contains("CLAUDE_CODE_ENABLE_TODO_TOOLS=1") && env.contains("PATH="), "{env}");
}

/// Under the scope `project` the row's `profile.project` is added to every child, its flags after the others and its environment with the
/// profile's; with the scope off (`user`, the default of a bare config) the argv is what it was.
#[test]
fn the_project_scope_adds_its_flags_and_environment_to_every_child_and_off_adds_nothing() {
    let project = Profile { args: vec!["--setting-sources".into(), "project,local".into(), "--strict-mcp-config".into()], env: [("SCOPE".to_string(), "project".to_string())].into() };
    let started = |scoped: bool| {
        let rig = Rig::playing_with("two-turns.jsonl", |cfg| {
            (cfg.scoped, cfg.project) = (scoped, project.clone());
        });
        let mut spec = TaskSpec::new("look", "claude");
        spec.tune = Tune { model: Some("haiku".into()), endpoint: None, effort: None, mode: Mode::Read };
        let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
        done(&rig, &id);
        starts(&rig).remove(0)
    };
    assert!(started(true).ends_with("--model haiku --setting-sources project,local --strict-mcp-config"), "{}", started(true));
    assert!(!started(false).contains("--strict-mcp-config"), "{}", started(false));
}

/// The row of claude ships the project scope (read from `claude --help` 2.1.323 and a child's init frame), and `set_scope` is what tells the
/// config; the lists of the flag `--disallowedTools` add up with the read mode's own.
#[test]
fn the_row_of_claude_ships_the_project_scope_and_the_setting_tells_the_config() {
    let layer = Layer::new(LayerKind::Pack, "agents", vec![SourceFile { path: "connectors/agents.toml".into(), text: include_str!("../../../../packs/base/connectors/agents.toml").into() }]);
    let (mut list, said) = j4n_connect::manifest::Connectors::load(&Tables::load(vec![layer]).0);
    assert!(said.is_empty(), "{said:?}");
    let of = |list: &j4n_connect::manifest::Connectors| j4n_connect::claude::Config::from_connector(list.all().iter().find(|c| c.id == "claude").unwrap(), Default::default());
    assert!(!of(&list).scoped, "the wire says it; a row alone adds nothing");
    list.set_scope(true);
    let cfg = of(&list);
    assert!(cfg.scoped);
    assert_eq!(cfg.project.args[..4], ["--setting-sources", "project,local", "--strict-mcp-config", "--no-chrome"]);
    let denied = cfg.project.args.last().unwrap();
    for tool in ["ListAgents", "SendMessage", "PushNotification", "RemoteTrigger", "DesignSync"] {
        assert!(denied.split(',').any(|t| t == tool), "{tool} is denied: {denied}");
    }
}

// ---- a chat's tool list ----

#[test]
fn a_tool_list_limits_the_built_in_set_and_the_denied_names_join_the_ones_the_read_mode_denies_in_one_flag() {
    for (mode, want, not) in [(Mode::Write, "--disallowedTools Bash,WebFetch", "Edit,Write"), (Mode::Read, "--disallowedTools Edit,Write,NotebookEdit,Bash,WebFetch", "--disallowedTools Bash,WebFetch")] {
        let rig = Rig::playing("two-turns.jsonl");
        let mut spec = TaskSpec::new("look", "claude");
        spec.tune = Tune { model: None, endpoint: None, effort: None, mode };
        spec.input = json!({ "tools": { "allow": ["Read", "Grep"], "deny": ["Bash", "WebFetch"] } });
        let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
        done(&rig, &id);
        let args = &starts(&rig)[0];
        assert!(args.contains("--tools Read,Grep") && args.contains(want), "{args}");
        assert_eq!(args.matches("--disallowedTools").count(), 1, "one flag, or the second would replace the first: {args}");
        assert!(mode == Mode::Read || !args.contains(not), "{args}");
    }
}

#[test]
fn a_task_with_no_tool_list_or_an_empty_one_is_started_exactly_as_before_and_a_name_that_looks_like_a_flag_is_left_out() {
    let mut plain = Vec::new();
    for input in [json!(null), json!({ "tools": { "allow": [], "deny": [] } }), json!({ "tools": { "deny": ["--dangerously-skip-permissions", "a,b", "c d"] } })] {
        let rig = Rig::playing("two-turns.jsonl");
        let mut spec = TaskSpec::new("look", "claude");
        spec.input = input;
        let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
        done(&rig, &id);
        plain.push(starts(&rig)[0].clone());
    }
    assert!(plain.iter().all(|a| *a == plain[0] && !a.contains("--tools") && !a.contains("--disallowedTools")), "{plain:?}");
}
