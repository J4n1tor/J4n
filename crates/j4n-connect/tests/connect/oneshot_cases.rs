//! The dialects that run a program once: `text`, `json-last` and `jsonl-map`. The programs are scripts the tests write; the
//! connectors are TOML rows read the way a user's file is, so what is tested is the path a program J4n has never seen takes.

use crate::bench::Bench;
use j4n_core::task::{State, Why};

fn done_with(row: &j4n_core::task::Row, answer: &str) {
    assert_eq!(row.state, State::Done { ok: true }, "{row:?}");
    assert_eq!(row.answer.as_deref(), Some(answer));
}

/// Why a run failed: the loop stops a task whose doer said it failed, and says what the doer said.
fn failed(row: &j4n_core::task::Row) -> String {
    match &row.state {
        State::Stopped { why: Why::Failed { message } } => message.clone(),
        other => panic!("not a failed run: {other:?} {row:?}"),
    }
}

const TEXT: &str = r#"
api = 1
[[connector]]
id = "say"
is = "agent"
dialect = "text"
program = "{bin}/say"
start = ["{prompt}"]
"#;

#[test]
fn text_prints_its_stdout_as_the_answer() {
    let b = Bench::new(&[("say", "echo \"you said: $1\"; echo second line")], TEXT);
    done_with(&b.run("say", "hello"), "you said: hello\nsecond line");
}

#[test]
fn a_program_that_reads_its_prompt_from_a_file_is_given_the_path_of_one_that_holds_it_and_the_file_goes_with_the_run() {
    let toml = "api = 1\n[[connector]]\nid = \"filer\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"{bin}/filer\"\nargv = [\"{bin}/filer\", \"--prompt-file\", \"{promptfile}\"]\n";
    let b = Bench::new(&[("filer", "cat \"$2\"")], toml);
    done_with(&b.run("filer", "a prompt with \"quotes\" and\nlines"), "a prompt with \"quotes\" and\nlines");
    let left: Vec<String> = std::fs::read_dir(b.project.path()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.contains("prompt")).collect();
    assert!(left.is_empty(), "the file is taken away when the run ends: {left:?}");
}

#[test]
fn a_nonzero_exit_fails_the_run_with_the_real_code_in_the_log_and_what_the_program_said() {
    let b = Bench::new(&[("say", "echo partial; echo 'it broke' >&2; exit 3")], TEXT);
    let why = failed(&b.run("say", "x"));
    assert!(why.contains("exit 3") && why.contains("it broke"), "{why}");
    let shell = b.events("run.shell");
    assert_eq!(shell.len(), 1, "{}", b.dump());
    assert_eq!(shell[0].data["exit"], 3);
}

#[test]
fn a_program_that_prints_nothing_is_a_run_that_did_nothing() {
    let b = Bench::new(&[("say", "exit 0")], TEXT);
    let row = b.run("say", "x");
    assert!(matches!(row.state, State::Done { ok: false }), "{row:?}");
}

#[test]
fn a_program_that_cannot_be_started_fails_the_run_in_words() {
    let b = Bench::new(&[], TEXT);
    assert!(failed(&b.run("say", "x")).contains("could not start"));
}

const JSON_LAST: &str = r#"
api = 1
[[connector]]
id = "op"
is = "tool"
dialect = "json-last"
program = "{bin}/op"
argv = ["{bin}/op", "--go"]
risk = "read"
"#;

/// A `json-last` connector printing `{"ok": false}` with exit 0 is a failed effect.
#[test]
fn ok_false_with_exit_0_is_a_failed_effect() {
    let b = Bench::new(&[("op", "echo 'some log'; echo '{\"ok\": false, \"error\": \"the disk is full\"}'; exit 0")], JSON_LAST);
    assert!(failed(&b.run("op", "go")).contains("the disk is full"));
    assert!(b.events("run.shell").is_empty(), "exit 0 is no shell failure");
}

#[test]
fn json_last_takes_the_answer_from_the_last_line_and_gets_the_goal_on_stdin() {
    let b = Bench::new(&[("op", "cat > \"$PWD/stdin.json\"; echo noise; echo '{\"ok\": true, \"answer\": \"saved\"}'")], JSON_LAST);
    done_with(&b.run("op", "go"), "saved");
    assert_eq!(b.project.read("stdin.json").trim(), "{\"prompt\":\"go\"}");
    let b2 = Bench::new(&[("op", "cat > \"$PWD/stdin.json\"; echo '{\"ok\": true}'")], JSON_LAST);
    done_with(&b2.run("op", "{\"path\": \"a.txt\"}"), "{\"ok\":true}");
    assert_eq!(b2.project.read("stdin.json").trim(), "{\"path\":\"a.txt\"}", "a goal that is a JSON object is sent as it is");
}

#[test]
fn json_last_without_json_or_with_a_bad_exit_fails() {
    let b = Bench::new(&[("op", "echo 'not json at all'")], JSON_LAST);
    assert!(failed(&b.run("op", "go")).contains("did not end with a line of JSON"));
    let b = Bench::new(&[("op", "echo '{\"ok\": true}'; exit 4")], JSON_LAST);
    assert!(failed(&b.run("op", "go")).contains("exit 4"));
}

const TOOL: &str = r#"
api = 1
[[connector]]
id = "grep"
is = "tool"
dialect = "text"
program = "{bin}/grep"
argv = ["{bin}/grep", "--line-number", "{pattern}", "{path}"]
risk = "read"
args = [{ name = "pattern", required = true }, { name = "path", default = "." }]
"#;

#[test]
fn a_tool_takes_its_arguments_from_a_json_goal_or_the_first_from_plain_words() {
    let b = Bench::new(&[("grep", "echo \"pattern=$2 path=$3\"")], TOOL);
    done_with(&b.run("grep", "{\"pattern\": \"todo\", \"path\": \"src\"}"), "pattern=todo path=src");
    done_with(&b.run("grep", "fixme"), "pattern=fixme path=.");
}

#[test]
fn a_tool_with_a_missing_argument_or_a_value_that_is_a_flag_does_not_run() {
    let b = Bench::new(&[("grep", "echo ran > \"$PWD/ran\"")], TOOL);
    assert!(failed(&b.run("grep", "{\"path\": \"src\"}")).contains("`pattern` is needed"));
    assert!(failed(&b.run("grep", "-rf")).contains("would be read as a flag"));
    assert_eq!(b.project.read("ran"), "", "nothing ran");
}

#[test]
fn a_tool_is_put_to_the_gate_and_a_block_stops_it_before_it_starts() {
    let manifest = r#"
api = 1
[[connector]]
id = "pusher"
is = "tool"
dialect = "text"
program = "git"
argv = ["git", "push", "origin", "main"]
risk = "exec"
"#;
    let b = Bench::new(&[], manifest);
    let row = b.run("pusher", "go");
    assert!(!matches!(row.state, State::Done { ok: true }), "{row:?}\n{}", b.dump());
    let asked = b.events("ask.answered");
    assert_eq!(asked.len(), 1, "{}", b.dump());
    assert_eq!(asked[0].data["by"], "rule:gate", "the Gate said no: {}", b.dump());
    assert_eq!(asked[0].data["answer"], "no");
    assert!(b.events("run.shell").is_empty(), "the program was never started");
}

#[test]
fn a_tool_is_asked_of_the_operator_when_nothing_allows_it_and_an_agent_never_is() {
    let b = Bench::new(&[("say", "echo hi")], TEXT);
    assert!(b.events("ask.answered").is_empty());
    done_with(&b.run("say", "x"), "hi");
    let b = Bench::new(&[("grep", "echo found")], TOOL);
    done_with(&b.run("grep", "x"), "found");
    assert_eq!(b.events("ask.answered").len(), 1, "a tool is put to the Gate: {}", b.dump());
}

#[test]
fn an_abort_takes_the_program_down() {
    let b = Bench::new(&[("say", "echo $$ > \"$PWD/pid\"; sleep 60")], TEXT);
    let id = b.spawn("say", "x");
    for _ in 0..2000 {
        if !b.project.read("pid").trim().is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let pid = b.project.read("pid").trim().to_string();
    assert!(!pid.is_empty(), "{}", b.dump());
    b.tasks.ctl(&id, j4n_core::task::Ctl::Abort, j4n_core::change::Actor::Operator).unwrap();
    let row = b.end(&id);
    assert!(matches!(row.state, State::Stopped { why: Why::Aborted }), "{row:?}");
    for _ in 0..400 {
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the program is still there");
}

// ---- a CLI J4n has never seen: jsonl-map, with no Rust ----

const NEWAGENT_SH: &str = include_str!("../../../../fixtures/connectors/newagent.sh");
const NEWAGENT_TOML: &str = include_str!("../../../../fixtures/connectors/newagent.toml");

fn newagent() -> Bench {
    Bench::new(&[("newagent", NEWAGENT_SH)], NEWAGENT_TOML)
}

#[test]
fn a_cli_never_seen_runs_by_its_manifest_alone_and_its_lines_become_the_chat() {
    let b = newagent();
    let row = b.run("newagent", "what is the answer?");
    done_with(&row, "The answer is 42 (no model, ).");
    assert_eq!(row.session.as_ref().map(|s| (s.doer.as_str(), s.id.as_str())), Some(("newagent", "ns-1")));
    assert_eq!(row.spent, 2000, "its cost, to the millionth of a dollar");
    let kinds: Vec<String> = b.log.read(0).filter(|e| e.task.as_deref() == Some("t1")).map(|e| e.kind).filter(|k| k.starts_with("run.")).collect();
    assert_eq!(kinds, ["run.notice", "run.session", "run.message", "run.tool_call", "run.tool_result", "run.metrics", "run.health", "run.message"], "the last words settle when the program is over");
    let messages: Vec<String> = b.events("run.message").iter().map(|e| e.data["text"].as_str().unwrap_or_default().to_string()).collect();
    assert_eq!(messages, ["Looking around.", "The answer is 42 (no model, )."], "the words before a tool stay narration, in their place");
    let notices: Vec<String> = b.events("run.notice").iter().map(|e| e.data["text"].as_str().unwrap_or_default().to_string()).collect();
    assert_eq!(notices, ["log: WARN: warming up (this line is not JSON)"]);
    let tools = b.events("run.tool_call");
    assert_eq!((tools[0].data["name"].as_str(), tools[0].data["detail"].as_str()), (Some("read"), Some("a.txt")));
}

#[test]
fn a_run_says_how_many_of_its_lines_the_map_read_and_a_program_none_of_whose_lines_are_read_has_drifted() {
    let b = newagent();
    done_with(&b.run("newagent", "what is the answer?"), "The answer is 42 (no model, ).");
    let health = b.events("run.health");
    assert_eq!(health.len(), 1, "one reading for each run of a program the map reads: {health:?}");
    let h = &health[0].data;
    assert_eq!((h["program"].as_str().map(|p| p.ends_with("/newagent")), h["lines"].as_u64(), h["unread"].as_u64(), h["ended"].as_bool(), h["drifted"].as_bool()), (Some(true), Some(8), Some(1), Some(true), Some(false)), "{h}");
    assert_eq!(health[0].by, j4n_core::change::Actor::Doer("newagent".into()), "the doer it is the health of");
    // the program changes what it says: its lines are JSON and the map reads none of them
    let why = failed(&b.run("newagent", "drift now"));
    assert!(why.contains("stopped without ending the run"), "{why}");
    let h = &b.events("run.health")[1].data;
    assert_eq!((h["lines"].as_u64(), h["read"].as_u64(), h["unread"].as_u64(), h["ended"].as_bool(), h["drifted"].as_bool()), (Some(3), Some(0), Some(3), Some(false), Some(true)), "{h}");
    assert_ne!(h["shape"], b.events("run.health")[0].data["shape"], "the shape of what it says is another");
}

#[test]
fn an_answer_too_long_for_the_settling_line_is_kept_by_its_end_and_the_run_still_ends_well() {
    let b = newagent();
    let row = b.run("newagent", "giant answer");
    assert_eq!(row.state, State::Done { ok: true }, "{row:?}");
    let answer = row.answer.as_deref().unwrap();
    assert!(answer.len() < 900_000 && answer.starts_with("(the first ") && answer.contains(" bytes of the answer are in the Log) "), "{}", &answer[..answer.len().min(120)]);
    assert!(answer.ends_with("xxxx"), "the end of the answer is what is kept");
    assert!(b.events("run.message").iter().any(|e| e.data.get("spilled").is_some()), "the whole answer is in the Log, in a blob");
}

#[test]
fn the_model_is_asked_for_only_when_the_task_names_one_and_a_session_is_resumed_in_the_program() {
    let b = newagent();
    let mut spec = j4n_core::task::TaskSpec::new("again", "newagent");
    spec.tune.model = Some("m1".into());
    spec.resume = Some(j4n_core::task::SessionRef { doer: "newagent".into(), id: "ns-9".into() });
    let id = b.tasks.spawn(spec, j4n_core::change::Actor::Operator).unwrap();
    done_with(&b.end(&id), "The answer is 42 (m1, resumed).");
    let log = b.bin.read("log");
    assert!(log.contains("run --json --model m1 --resume ns-9 again"), "the prompt stands last: {log}");
    b.run("newagent", "no model");
    assert!(b.bin.read("log").lines().last().unwrap().ends_with("run --json no model"), "{}", b.bin.read("log"));
}

#[test]
fn an_error_line_fails_the_run_with_its_words_and_a_crash_with_its_exit() {
    let b = newagent();
    assert!(failed(&b.run("newagent", "fail now")).contains("the model is down"));
    let why = failed(&b.run("newagent", "crash now"));
    assert!(why.contains("signal") || why.contains("exit 139"), "{why}");
    assert!(why.contains("segfault"), "{why}");
}

#[test]
fn the_probe_of_the_manifest_finds_the_program_and_its_version() {
    let bin = crate::world::Scratch::new("bin");
    crate::world::script(&bin, "newagent", NEWAGENT_SH);
    let (list, said, _) = crate::bench::connectors(&bin, NEWAGENT_TOML);
    assert!(said.is_empty(), "{said:?}");
    let p = list.get("newagent").unwrap().probe(&Default::default());
    assert_eq!((p.installed, p.authed, p.version.as_deref()), (true, true, Some("newagent")));
    let nowhere = crate::world::Scratch::new("none");
    let (list, _, _) = crate::bench::connectors(&nowhere, NEWAGENT_TOML);
    let p = list.get("newagent").unwrap().probe(&Default::default());
    assert!(!p.installed && !p.authed && p.detail.contains("did not answer"), "{p:?}");
}

#[test]
fn what_a_run_adds_goes_after_the_arguments_when_the_prompt_has_no_place_of_its_own() {
    let manifest = r#"
api = 1
[[connector]]
id = "bare"
is = "agent"
dialect = "text"
program = "{bin}/bare"
start = ["run"]
model = ["--model", "{model}"]
env = { LOG = "{bin}/log" }
"#;
    let b = Bench::new(&[("bare", "echo \"$*\" >> \"$LOG\"; echo ok")], manifest);
    let mut spec = j4n_core::task::TaskSpec::new("x", "bare");
    spec.tune.model = Some("m9".into());
    b.end(&b.tasks.spawn(spec, j4n_core::change::Actor::Operator).unwrap());
    assert_eq!(b.bin.read("log").trim(), "run --model m9");
}

#[test]
fn a_session_the_program_repeats_on_every_line_is_said_once_and_only_a_program_with_a_way_back_can_carry_on() {
    let manifest = r#"
api = 1
[[connector]]
id = "chatty"
is = "agent"
dialect = "jsonl-map"
program = "{bin}/chatty"
start = ["{prompt}"]
[connector.map]
session = "/sid"
events = [{ when = "/t == 'text'", emit = "text", text = "/x" }]
[[connector]]
id = "noway"
is = "agent"
dialect = "jsonl-map"
program = "{bin}/chatty"
start = ["{prompt}"]
[connector.map]
session = "/sid"
events = [{ when = "/t == 'text'", emit = "text", text = "/x" }]
[[connector]]
id = "way"
is = "agent"
dialect = "jsonl-map"
program = "{bin}/chatty"
start = ["{prompt}"]
resume = ["--resume", "{session}"]
[connector.map]
session = "/sid"
events = [{ when = "/t == 'text'", emit = "text", text = "/x" }]
[[connector]]
id = "textway"
is = "agent"
dialect = "text"
program = "{bin}/chatty"
start = ["{prompt}"]
resume = ["--resume", "{session}"]
"#;
    let b = Bench::new(&[("chatty", "echo '{\"sid\":\"S1\",\"t\":\"text\",\"x\":\"a\"}'; echo '{\"sid\":\"S1\",\"t\":\"text\",\"x\":\"b\"}'")], manifest);
    done_with(&b.run("chatty", "go"), "ab");
    assert_eq!(b.events("run.session").len(), 1, "{}", b.dump());
    let (list, said, _) = crate::bench::connectors(&b.bin, manifest);
    assert!(said.is_empty(), "{said:?}");
    let (doers, _) = list.doers(&|_, r| Err(r.to_string()));
    let caps = |id: &str| doers.iter().find(|d| d.id() == id).unwrap().caps();
    assert_eq!((caps("noway").resume, caps("way").resume, caps("textway").resume), (false, true, false));
    assert!(!caps("way").live, "a program that takes its words once cannot be steered");
}

// ---- opencode, replayed from captures: the connector row of the pack, a script that prints a capture ----

const OPENCODE_TOML: &str = include_str!("../../../../packs/connect/connectors/opencode.toml");
const OPENCODE_RUN: &str = include_str!("../../../../fixtures/connectors/opencode-run.jsonl");
const OPENCODE_REJECTED: &str = include_str!("../../../../fixtures/connectors/opencode-rejected.jsonl");
const OPENCODE_ERRORED: &str = include_str!("../../../../fixtures/connectors/opencode-errored-last.jsonl");
const OPENCODE_RECOVERED: &str = include_str!("../../../../fixtures/connectors/opencode-recovered.jsonl");

/// The pack's own row of opencode, started as a script that prints the capture its prompt names (a word of its own) and, for the rejected one, the line
/// opencode says on stderr when it refuses a permission itself.
fn opencode() -> Bench {
    let arm = |word: &str, text: &str, stderr: &str| format!("{word}*) grep -v '^#' <<'J4N_EOF'\n{text}J4N_EOF\n{stderr};;\n");
    let script = format!(
        "[ \"$1\" = --version ] && {{ echo '1.18.32'; exit 0; }}\nfor last; do :; done\ncase \"$last\" in\n{}{}{}{}{}envcheck*) printf '{{\"type\":\"text\",\"sessionID\":\"s\",\"part\":{{\"text\":\"auto=%s cfg=%s\"}}}}\\n' \"$(case \" $* \" in *\" --auto \"*) echo yes;; *) echo no;; esac)\" \"$(printf %s \"$OPENCODE_CONFIG_CONTENT\" | grep -c '\"bash\"')\"; printf '{{\"type\":\"step_finish\",\"part\":{{\"reason\":\"stop\"}}}}\\n';;\nstdin*) printf '{{\"type\":\"text\",\"sessionID\":\"s\",\"part\":{{\"text\":\"stdin=%s\"}}}}\\n' \"$(readlink /proc/$$/fd/0)\"; printf '{{\"type\":\"step_finish\",\"part\":{{\"reason\":\"stop\"}}}}\\n';;\nesac\n",
        arm("normal", OPENCODE_RUN, ""),
        arm("recovered", OPENCODE_RECOVERED, ""),
        arm("errored", OPENCODE_ERRORED, ""),
        arm("rejected", OPENCODE_REJECTED, "echo 'permission requested: external_directory (/tmp/*); auto-rejecting' >&2\n"),
        arm("denied", &denied_capture(), ""),
    );
    let toml = OPENCODE_TOML.replace("program = \"opencode\"", "program = \"{bin}/opencode\"").replace("argv = [\"opencode\", \"--version\"]", "argv = [\"{bin}/opencode\", \"--version\"]");
    Bench::new(&[("opencode", &script)], &toml)
}

/// What opencode says when its rules refuse a line: the error is every rule of the config (about 30 000 characters), here 600 rules of the shape it has.
fn denied_capture() -> String {
    let rules: Vec<serde_json::Value> = (0..600).map(|i| serde_json::json!({ "permission": "bash", "pattern": format!("git -C * verb{i} *"), "action": "allow" })).collect();
    let error = format!("The user has specified a rule which prevents you from using this specific tool call. Here are some of the relevant rules {}", serde_json::Value::Array(rules));
    assert!(error.len() > 30_000);
    let part = serde_json::json!({ "type": "tool", "tool": "bash", "callID": "call_1", "state": { "status": "error", "input": { "command": "rm -rf build", "description": "clean" }, "error": error, "time": { "start": 1, "end": 2 } } });
    let line = |v: serde_json::Value| format!("{v}\n");
    [
        line(serde_json::json!({ "type": "step_start", "sessionID": "s", "part": { "type": "step-start" } })),
        line(serde_json::json!({ "type": "tool_use", "sessionID": "s", "part": part })),
        line(serde_json::json!({ "type": "step_finish", "sessionID": "s", "part": { "type": "step-finish", "reason": "tool-calls", "tokens": { "input": 1, "output": 1, "reasoning": 0, "cache": { "read": 0, "write": 0 } }, "cost": 0 } })),
    ]
    .concat()
}

#[test]
fn a_refused_line_is_one_sentence_in_the_run_and_in_the_reason_and_not_the_ruleset() {
    let b = opencode();
    let row = b.run("opencode", "denied rm");
    let why = failed(&row);
    assert!(why.contains("the run ended on a tool that failed: J4n's guard refused `rm -rf build`: it deletes."), "{why}");
    assert!(why.len() < 600, "{} characters", why.len());
    let shells = b.events("run.shell");
    assert_eq!(shells.len(), 1, "{}", b.dump());
    assert_eq!((shells[0].data["cmd"].as_str(), shells[0].data["out"].as_str()), (Some("rm -rf build"), Some("J4n's guard refused `rm -rf build`: it deletes.")), "{:?}", shells[0].data);
    assert!(!b.dump().contains("verb599"), "no rule of the ruleset is anywhere in the Log");
}

#[test]
fn a_run_that_ends_on_a_tool_the_program_refused_is_failed_and_the_reason_names_the_permission_and_its_path() {
    let b = opencode();
    let row = b.run("opencode", "rejected the messy variants");
    let why = failed(&row);
    assert!(why.contains("the run ended on a tool that failed: The user rejected permission"), "{why}");
    assert!(why.contains("permission requested: external_directory (/tmp/*); auto-rejecting"), "the stderr line names the permission and the path: {why}");
    // its class is `refused`, not the `transient` that `retry.auto` tries again and the host's error count reads as the host's
    let errors = b.events("run.error");
    assert_eq!(errors.len(), 1, "{}", b.dump());
    let file = |p: &str, t: &str| j4n_core::table::SourceFile { path: p.into(), text: t.into() };
    let layer = j4n_core::table::Layer::new(
        j4n_core::table::LayerKind::Pack,
        "failures",
        vec![file("table.toml", include_str!("../../../../packs/base/failures/table.toml"))],
    );
    let (tables, problems) = j4n_core::table::Tables::load(vec![layer]);
    assert!(problems.is_empty(), "{problems:?}");
    let (failures, _) = j4n_core::failure::Failures::load(&tables);
    let seen = failures.classify(errors[0].data["text"].as_str().unwrap_or_default(), 0);
    assert_eq!(seen.class, j4n_core::failure::Class::Refused, "{:?}", errors[0].data);
    // the refused line is a shell line that did not run, with its error text, and not a quiet exit 0
    let shells = b.events("run.shell");
    assert_eq!(shells.len(), 1, "{}", b.dump());
    assert_eq!(shells[0].data["cmd"], "ls /tmp/messy");
    assert_ne!(shells[0].data["exit"], 0, "{:?}", shells[0].data);
    assert!(shells[0].data["out"].as_str().unwrap_or_default().contains("The user rejected permission"), "{:?}", shells[0].data);
}

#[test]
fn a_last_tool_with_an_error_fails_a_run_that_still_says_stop_and_a_later_tool_that_works_makes_it_a_good_run() {
    let b = opencode();
    let why = failed(&b.run("opencode", "errored then stop"));
    assert!(why.contains("the run ended on a tool that failed") && !why.contains("auto-rejecting"), "{why}");
    let row = b.run("opencode", "recovered after an error");
    done_with(&row, "Found a.txt.");
}

#[test]
fn a_normal_run_is_still_ok_and_a_stream_that_never_reaches_the_maps_end_is_failed() {
    let b = opencode();
    done_with(&b.run("opencode", "normal run"), "ok");
    // no `end` line, no error: the program stopped, and its last words are not an answer
    let b = Bench::new(
        &[("newagent", "[ \"$1\" = --version ] && exit 0\nprintf '{\"type\":\"text\",\"text\":\"Now checking:\"}\\n'\n")],
        NEWAGENT_TOML,
    );
    let why = failed(&b.run("newagent", "go"));
    assert!(why.contains("stopped without ending the run"), "{why}");
}

#[test]
fn the_tokens_of_a_step_finish_are_the_tokens_of_the_task() {
    let b = opencode();
    let row = b.run("opencode", "normal run");
    let metrics = b.events("run.metrics");
    let sum = |k: &str| metrics.iter().map(|e| e.data[k].as_u64().unwrap_or(0)).sum::<u64>();
    // two steps: 6,000 fresh and 2,000 cached in, 40 + 80 reasoning out; then 20 fresh and 8,000 cached in, 2 out
    assert_eq!((sum("tokens_in"), sum("tokens_out"), sum("cached")), (6000 + 2000 + 20 + 8000, 40 + 80 + 2, 2000 + 8000), "{}", b.dump());
    assert_eq!(metrics.len(), 2);
    assert!(row.tokens_in > 0 && row.tokens_out > 0, "the row carries them");
}

#[test]
fn the_program_is_started_with_its_stdin_closed() {
    let b = opencode();
    let row = b.run("opencode", "stdin please");
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}", row.state);
    assert!(b.dump().contains("stdin=/dev/null"), "opencode run hangs on an open pipe: {}", b.dump());
}

#[test]
fn the_program_is_started_with_auto_and_the_config_in_its_variable_and_the_operators_own_config_is_not_written() {
    let b = opencode();
    let home = std::env::var("HOME").unwrap_or_default();
    let before = std::path::Path::new(&home).join(".config/opencode").exists();
    let row = b.run("opencode", "envcheck please");
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}\n{}", row.state, b.dump());
    let said = row.answer.unwrap_or_default();
    assert_eq!(said, "auto=yes cfg=1", "`--auto` goes with the config, and the config is in OPENCODE_CONFIG_CONTENT");
    assert_eq!(std::path::Path::new(&home).join(".config/opencode").exists(), before, "no config of the operator's was made or touched");
}

// ---- what a program learned by trying needs of the engine ----

const LEARNED: &str = r#"
api = 1
[[connector]]
id = "ag"
is = "agent"
dialect = "jsonl-map"
program = "{bin}/ag"
argv = ["{bin}/ag"]
stdin = "{prompt}"
decode = ["ansi", "colon"]
gated = true
[connector.env]
AG_WORDS = "{prompt}"
[connector.map]
session = "/session"
events = [{ when = "not (/head == '*') and not (/session)", emit = "text", text = "/line" }]
"#;

/// The words on its standard input and in a variable, a colour and a spinner drawn over, a `session:` line and a tool line the answer is cut out of.
const AG: &str = "cat > \"$PWD/stdin.txt\"; printf '\\r\\342\\240\\231 thinking\\r\\033[K\\033[1;32msession: s7\\033[0m\\n'; echo '* read x'; echo \"$AG_WORDS\"";

#[test]
fn a_program_learned_by_trying_is_given_its_words_on_stdin_and_in_a_variable_and_its_answer_is_cut_out_of_the_noise() {
    let b = Bench::new(&[("ag", AG)], LEARNED);
    let row = b.run("ag", "plum42");
    done_with(&row, "plum42");
    assert_eq!(std::fs::read_to_string(b.project.join("stdin.txt")).unwrap().trim(), "plum42");
    assert!(b.events("run.tool_call").is_empty(), "the `* read x` line is noise to this map, not a tool");
}

#[test]
fn a_gated_agent_asks_the_gate_before_a_run_that_may_write_and_a_no_is_a_run_that_never_started() {
    let b = Bench::new(&[("ag", "echo started > \"$PWD/ran.txt\"; echo hi")], LEARNED);
    let id = b.spawn("ag", "go");
    let ask = b.waiting(&id);
    let asked = b.log.read(0).find(|e| e.kind == "task.queued" && e.task.as_deref() == Some(ask.as_str())).unwrap().data.to_string();
    assert!(asked.contains("run `"), "{asked}");
    b.answer(&ask, j4n_core::ask::Answer::No);
    let why = failed(&b.end(&id));
    assert!(why.contains("not allowed"), "{why}");
    assert!(!b.project.join("ran.txt").exists(), "a no is a program that never ran");
}

#[test]
fn a_gated_agent_in_read_mode_is_not_asked_because_the_walls_hold_it() {
    let b = Bench::new(&[("ag", "echo hi")], LEARNED);
    let mut spec = j4n_core::task::TaskSpec::new("go", "ag");
    spec.tune.mode = j4n_core::task::Mode::Read;
    let id = b.tasks.spawn(spec, j4n_core::change::Actor::Operator).unwrap();
    let row = b.end(&id);
    assert_eq!(row.state, State::Done { ok: true }, "{row:?}");
    assert!(!b.log.read(0).any(|e| e.kind == "ask.answered"), "{}", b.dump());
}
