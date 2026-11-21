//! The word `orchestrate` in a run with no screen: the real binary, `j4n run`, with a `claude` that answers by what it is asked. The quick look says split, Main writes
//! one brief for a chat that reads, the chat is started and the run waits for it, for the plan to take what came of it and for its digest, and ends with status 0. Under
//! any other word the same run has one thread and no chat.

use crate::run_cases::{Dirs, j4n, kinds, log_events};
use std::fs;
use std::os::unix::fs::PermissionsExt;

/// A `claude` that answers by the prompt: the quick look, Main's planning turn, a chat's brief, and whatever else (a check, a review) with a short word.
/// The `claude` below, whose chat takes `pause` seconds over its report (so a screen can be looked at while it works).
pub fn claude(pause: u32) -> String {
    CLAUDE.replace("@PAUSE@", &pause.to_string()).replace("@LOOK@", "0")
}

/// [`claude`] whose quick look takes `look` seconds too, so a screen can be looked at while the line waits for it.
pub fn claude_slow_look(pause: u32, look: u32) -> String {
    CLAUDE.replace("@PAUSE@", &pause.to_string()).replace("@LOOK@", &look.to_string())
}

const CLAUDE: &str = r##"#!/bin/sh
case "$1" in
  --version) echo "2.1.323 (Claude Code)"; exit 0;;
  auth) echo '{"loggedIn":true,"email":"a@b.c","subscriptionType":"max","authMethod":"claude.ai"}'; exit 0;;
esac
SID=00000000-0000-4000-8000-0000000000c1
LOOK='{\"decision\":\"split\",\"chats\":[{\"goal\":\"read the code\",\"writes\":true,\"doer\":\"claude\",\"model\":\"\"}],\"features\":[],\"why\":\"one part apart\"}'
PLAN='{\"chats\":[{\"id\":\"read-code\",\"goal\":\"read the code and say what it does\",\"paths\":[\"src/a/1.rs\",\"src/a/2.rs\",\"src/a/3.rs\"],\"writes\":[],\"done\":\"it says what it does\",\"doer\":\"claude\",\"model\":\"\",\"mode\":\"read\"}],\"main\":\"\"}'
REPORT='{\"answer\":\"it parses lines\",\"found\":[],\"changed\":\"\",\"ran\":[],\"sure\":\"high\",\"open\":[]}'
reply() {
  printf '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"%s"}]},"session_id":"%s"}\n' "$1" "$SID"
  printf '{"type":"result","subtype":"success","is_error":false,"result":"%s","session_id":"%s","total_cost_usd":0.001,"usage":{"input_tokens":3,"output_tokens":1}}\n' "$1" "$SID"
}
while IFS= read -r line; do
  case "$line" in
    *'"type":"user"'*)
      echo '{"type":"system","subtype":"init","cwd":"/work","session_id":"'$SID'","model":"claude-haiku-5-5","permissionMode":"default","tools":[]}'
      case "$line" in
        *"You are Main, the first chat"*) sleep @LOOK@; reply "$LOOK";;
        *"A quick look says this line splits"*) reply "$PLAN";;
        *"You are one chat of J4n, started by Main"*) sleep @PAUSE@; reply "$REPORT";;
        *) reply "no concern";;
      esac;;
  esac
done
"##;

pub fn settings(d: &Dirs, lanes: &str) {
    fs::create_dir_all(d.home().join(".j4n/settings")).unwrap();
    let rows = [("lanes", format!("\"{lanes}\"")), ("orchestrate.look", "true".into()), ("orchestrate.ask", "\"never\"".into()), ("plan.cheap_doer", "\"claude\"".into())];
    let text = rows.iter().fold("api = 1\n".to_string(), |t, (id, v)| t + &format!("[[setting]]\nid = \"{id}\"\nvalue = {v}\n"));
    // the pack `lanes` holds the classes and limits a chat is started under: J4n basic leaves it off
    let text = text + "[[enable]]\nid = \"lanes\"\n";
    fs::write(d.home().join(".j4n/settings/orchestrate.toml"), text).unwrap();
}

fn run(tag: &str, lanes: &str) -> (Dirs, crate::run_cases::Out) {
    let d = Dirs::new(tag);
    let program = d.dir.join("bin/claude");
    fs::write(&program, claude(0)).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    settings(&d, lanes);
    let env = [("J4N_CLAUDE", program.to_str().unwrap())];
    let o = j4n(&d, &["run", "--doer", "claude", "--approval", "full", "--wall", "90s", "explain what the code in src/a does"], None, &env);
    (d, o)
}

#[test]
fn a_run_under_orchestrate_splits_starts_the_chat_waits_for_it_and_ends_with_its_digest() {
    let (d, o) = run("om-run", "orchestrate");
    let events = log_events(&d);
    let ks = kinds(&events);
    assert_eq!(o.rc, 0, "{}\n{}\n{ks:?}", o.out, o.err);
    let of = |k: &str| events.iter().filter(|e| e["kind"] == k).collect::<Vec<_>>();
    assert_eq!(of("orchestrate.looked").len(), 1, "{ks:?}");
    let split = of("orchestrate.split");
    assert_eq!(split[0]["data"]["started"], serde_json::json!(["c1"]), "{}\n{}\n{ks:?}", o.out, o.err);
    assert_eq!(of("chat.briefed").len(), 1);
    // the run did not end while the chat was at work: its turn is over, and so is what came of it
    let settled: Vec<_> = of("task.settled");
    assert_eq!(settled.len(), 3, "the look, the planning turn and the chat: {ks:?}");
    let states: Vec<&str> = of("plan.item").iter().filter(|e| e["data"]["id"] == "c1" && e["data"]["op"] == "state").filter_map(|e| e["data"]["state"].as_str()).collect();
    assert_eq!(states.last(), Some(&"reviewed"), "the plan took what the chat came to: {states:?}");
    let digest = of("orchestrate.digest");
    assert_eq!(digest.len(), 1, "{}\n{}\n{ks:?}", o.out, o.err);
    assert!(digest[0]["data"]["text"].as_str().unwrap().starts_with("c1 "), "{}", digest[0]["data"]);
    assert!(o.secs < 80.0, "{}", o.secs);
}

#[test]
fn the_same_run_under_single_has_one_thread_and_no_chat() {
    let (d, o) = run("om-run-single", "single");
    let ks = kinds(&log_events(&d));
    assert_eq!(o.rc, 0, "{}\n{}\n{ks:?}", o.out, o.err);
    for gone in ["chat.opened", "chat.briefed", "orchestrate.looked", "orchestrate.split", "orchestrate.digest"] {
        assert!(!ks.iter().any(|k| k == gone), "{gone} in {ks:?}");
    }
}

/// The one switch is the whole of the setup. A bare home (no legacy packs, no `[[enable]]` rows) that says only `lanes = orchestrate` (and the cheap tier's doer, which
/// the need cards make unnecessary) splits the same line and starts the chat: the look, the crew's data and the lanes' classes are all there with no other setting touched.
#[test]
fn a_bare_home_that_says_only_the_word_orchestrate_splits_the_line_and_starts_the_chat() {
    let d = Dirs::bare("orchestrate-word-only");
    let program = d.dir.join("bin/claude");
    fs::write(&program, claude(0)).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    fs::create_dir_all(d.j4n_home().join("settings")).unwrap();
    fs::write(d.j4n_home().join("settings/word.toml"), "api = 1\n[[setting]]\nid = \"lanes\"\nvalue = \"orchestrate\"\n[[setting]]\nid = \"plan.cheap_doer\"\nvalue = \"claude\"\n[[setting]]\nid = \"check.after\"\nvalue = false\n").unwrap();
    let env = [("J4N_CLAUDE", program.to_str().unwrap())];
    let o = j4n(&d, &["run", "--doer", "claude", "--approval", "full", "--wall", "90s", "explain what the code in src/a does"], None, &env);
    let events = log_events(&d);
    let ks = kinds(&events);
    assert_eq!(o.rc, 0, "{}\n{}\n{ks:?}", o.out, o.err);
    let of = |k: &str| events.iter().filter(|e| e["kind"] == k).collect::<Vec<_>>();
    assert_eq!(of("orchestrate.looked").len(), 1, "the quick look follows the word: {ks:?}");
    assert_eq!(of("orchestrate.split")[0]["data"]["started"], serde_json::json!(["c1"]), "{}\n{}\n{ks:?}", o.out, o.err);
    assert_eq!(of("chat.opened").len(), 1, "{ks:?}");
}

/// A `claude` whose quick look keeps the line one piece, whose Main writes a file, and whose review of that change takes longer than the review may.
const SLOW_REVIEW: &str = r##"#!/bin/sh
case "$1" in
  --version) echo "2.1.323 (Claude Code)"; exit 0;;
  auth) echo '{"loggedIn":true,"email":"a@b.c","subscriptionType":"max","authMethod":"claude.ai"}'; exit 0;;
esac
SID=00000000-0000-4000-8000-0000000000c2
reply() {
  printf '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"%s"}]},"session_id":"%s"}\n' "$1" "$SID"
  printf '{"type":"result","subtype":"success","is_error":false,"result":"%s","session_id":"%s","total_cost_usd":0.001,"usage":{"input_tokens":3,"output_tokens":1}}\n' "$1" "$SID"
}
while IFS= read -r line; do
  case "$line" in
    *'"type":"user"'*)
      echo '{"type":"system","subtype":"init","cwd":"/work","session_id":"'$SID'","model":"claude-haiku-5-5","permissionMode":"default","tools":[]}'
      case "$line" in
        *"You are Main, the first chat"*) reply '{\"decision\":\"one\",\"chats\":[],\"features\":[],\"why\":\"one piece\"}';;
        *"Review the change Main just made"*) sleep 8; reply "no concern";;
        *) echo made > made.txt; reply "made it";;
      esac;;
  esac
done
"##;

/// A review of Main's turn that stops at its own limit (five minutes, here two seconds) ends the review, not the piped run: the step that reads
/// the review still runs. A task's own limit ends that task; only the run's clock ends the run.
#[test]
fn a_review_stopped_at_its_own_limit_ends_the_review_and_not_the_run() {
    let d = Dirs::new("review-own-wall");
    let program = d.dir.join("bin/claude");
    fs::write(&program, SLOW_REVIEW).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    settings(&d, "orchestrate");
    // the operator's own layer gives the review two seconds (one field of the pack's row)
    fs::create_dir_all(d.j4n_home().join("rules")).unwrap();
    fs::write(d.j4n_home().join("rules/review.toml"), "api = 1\n[[branch]]\nid = \"review\"\nwall = \"2s\"\n").unwrap();
    let env = [("J4N_CLAUDE", program.to_str().unwrap())];
    let o = j4n(&d, &["run", "--doer", "claude", "--approval", "full", "--wall", "90s", "write made.txt"], None, &env);
    let events = log_events(&d);
    let ks = kinds(&events);
    let of = |k: &str| events.iter().filter(|e| e["kind"] == k).collect::<Vec<_>>();
    let started = of("second.started");
    assert_eq!(started.len(), 1, "Main's change is reviewed: {}\n{}\n{ks:?}", o.out, o.err);
    let review = started[0]["data"]["task"].as_str().unwrap().to_string();
    // it is told its time, and that a command that writes is refused (a review that runs a build can spend its limit and say nothing)
    let goal = of("task.queued").into_iter().find(|e| e["task"] == review.as_str()).and_then(|e| e["data"]["spec"]["goal"].as_str().map(String::from)).unwrap_or_default();
    assert!(goal.contains("You have 2s: say what you found before it is out") && goal.contains("read the code instead of running it"), "{goal}");
    let stopped = of("task.stopped").into_iter().find(|e| e["task"] == review.as_str()).map(|e| e["data"]["why"]["kind"].clone());
    assert_eq!(stopped, Some(serde_json::json!("wall")), "the review stops at its own limit: {ks:?}");
    let ended = of("second.ended");
    assert_eq!(ended.len(), 1, "the step that reads the review ran after it stopped: {}\n{}\n{ks:?}", o.out, o.err);
    assert_eq!(ended[0]["data"]["outcome"], "failed", "{}", ended[0]);
    assert!(o.secs < 60.0, "{}", o.secs);
}
