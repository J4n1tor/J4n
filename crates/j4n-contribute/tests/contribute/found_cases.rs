//! What is a finding, and what is not. The events are the ones the engine writes, in the words it writes them.

use j4n_contribute::found::{self, Finding, clip, judge};
use j4n_core::change::Actor;
use j4n_core::log::Ev;
use j4n_core::schema::DataClass;
use serde_json::{Value, json};

fn ev(kind: &str, by: Actor, data: Value) -> Ev {
    Ev { seq: 7, at_ms: 1, kind: kind.into(), task: Some("t1".into()), thread: None, by, cause: None, class: DataClass::Project, data }
}

fn error(by: &str, text: &str, class: &str, row: Value) -> Ev {
    ev("run.error", Actor::Doer(by.into()), json!({ "text": text, "class": class, "row": row, "wait_ms": 0 }))
}

fn found_of(e: &Ev) -> Finding {
    judge(e).unwrap_or_else(|| panic!("{e:?} is a finding"))
}

#[test]
fn what_j4n_says_in_its_own_words_when_it_meets_its_own_floor_is_a_finding() {
    let f = found_of(&error("model", "the model sent the same reply twice in a row, so it is not getting anywhere", "transient", Value::Null));
    assert_eq!((f.source, f.id.as_str(), f.row.as_str(), f.cause), ("floor", "same-reply", "model", 7));
    let f = found_of(&error("model", "the model went 12 turns without finishing", "transient", Value::Null));
    assert_eq!((f.source, f.id.as_str()), ("floor", "turn-cap"));
    let f = found_of(&error("flow", "flow step `build` went back 3 times and it is still not right; that is as often as it may", "permanent", json!("flow-step")));
    assert_eq!((f.source, f.id.as_str()), ("floor", "flow-back"), "the flow-step row is a finding when it is J4n's own bound");
    let f = found_of(&error("claude", "claude said nothing for 30 seconds after it was asked", "transient", Value::Null));
    assert_eq!((f.source, f.id.as_str(), f.row.as_str()), ("hang", "silent-start", "claude"));
    let f = found_of(&error("pi", "`pi` did not end with a line of JSON: oops", "transient", Value::Null));
    assert_eq!((f.source, f.id.as_str()), ("dialect", "no-json"));
}

#[test]
fn a_finding_is_made_of_the_sentence_j4n_says_and_never_of_the_rest_of_the_message() {
    let a = found_of(&error("pi", "`pi` did not end with a line of JSON: secret output of the program /home/ana/x", "transient", Value::Null));
    let b = found_of(&error("pi", "`pi` did not end with a line of JSON: something else entirely", "transient", Value::Null));
    assert_eq!((a.text.as_str(), a.title.as_str()), ("the program did not end with a line of JSON", "the program did not end with a line of JSON"));
    assert_eq!((a.text, a.title), (b.text, b.title), "the tail is not in it");
    let k = |v: &str| found_of(&ev("run.unread", Actor::Core, json!({ "dialect": "claude-stream", "kind": v }))).id;
    assert_ne!(k("frame_v2"), k("frame_v3"), "a kind is not stripped: its digits tell one from another");
    assert_ne!(k("a/b"), k("a/c"));
}

#[test]
fn a_failure_that_is_the_providers_the_login_or_the_setup_is_never_a_finding_even_in_j4ns_words() {
    let own = "the model went 12 turns without finishing";
    assert!(judge(&error("model", own, "rate-limit", json!("limit"))).is_none(), "a rate limit");
    assert!(judge(&error("model", own, "transient", json!("overloaded"))).is_some(), "an overloaded provider is not in the never list: only its class and rows are");
    assert!(judge(&error("model", own, "permanent", json!("gone"))).is_none(), "a login, a model that is gone");
    assert!(judge(&error("model", own, "permanent", json!("agent-stop"))).is_none(), "the agent's own budget");
    // a provider's error in a provider's words: not J4n's, whatever its class
    for words in ["529 overloaded_error", "Internal server error", "connection reset by peer", "No such file or directory", "401 Unauthorized", "the flow step `x` failed"] {
        assert!(judge(&error("claude", words, "transient", Value::Null)).is_none(), "{words}");
    }
    assert!(judge(&error("check", "cargo check failed (exit 101)", "permanent", json!("flow-step"))).is_none(), "a step that failed is not a bound that was hit");
}

#[test]
fn a_panic_the_engine_caught_is_a_finding_and_any_other_failure_of_a_task_is_not() {
    for message in ["the loop failed while handling an order", "the doer stopped without a report", "the doer failed while starting"] {
        let f = found_of(&ev("task.stopped", Actor::Core, json!({ "why": { "kind": "failed", "message": message }, "class": null })));
        assert_eq!((f.source, f.id.as_str(), f.text.as_str()), ("panic", "caught", message));
    }
    assert!(judge(&ev("task.stopped", Actor::Core, json!({ "why": { "kind": "failed", "message": "claude said no" } }))).is_none());
    assert!(judge(&ev("task.stopped", Actor::Core, json!({ "why": { "kind": "operator" } }))).is_none(), "a stop is a stop");
    assert!(judge(&ev("task.stopped", Actor::Doer("x".into()), json!({ "why": { "kind": "failed", "message": "the doer stopped without a report" } }))).is_none(), "only the core says it");
}

#[test]
fn a_stream_the_dialect_could_not_read_is_a_finding_named_by_the_dialect_and_the_kind() {
    let f = found_of(&ev("run.unread", Actor::Doer("claude".into()), json!({ "dialect": "claude-stream", "kind": "mystery_frame" })));
    assert_eq!((f.source, f.id.as_str(), f.row.as_str(), f.text.as_str()), ("dialect", "unread:mystery_frame", "claude-stream", ""));
    assert!(f.title.contains("claude-stream") && f.title.contains("mystery_frame"), "{}", f.title);
    assert!(judge(&ev("run.unread", Actor::Core, json!({ "dialect": "", "kind": "x" }))).is_none() && judge(&ev("run.unread", Actor::Core, json!({ "dialect": "d", "kind": "" }))).is_none());
}

#[test]
fn nothing_else_is_a_finding() {
    for kind in ["run.notice", "task.started", "chat.say", "run.text", "contribute.found", "watch.release"] {
        assert!(judge(&ev(kind, Actor::Core, json!({ "text": "the model went 12 turns without finishing" }))).is_none(), "{kind}");
    }
}

#[test]
fn a_title_is_one_clipped_line_with_no_control_characters() {
    assert_eq!(clip("a\nb\t\u{1b}[31mc   d", 40), "a b [31mc d");
    assert_eq!(clip("abcdef", 6), "abcdef");
    assert_eq!(clip("abcdefg", 6), "abcdef…");
    assert_eq!([found::mark("floor"), found::mark("hang"), found::mark("word"), found::mark("panic"), found::mark("dialect")], ["▲", "◌", "¶", "✗", "✗"]);
}
