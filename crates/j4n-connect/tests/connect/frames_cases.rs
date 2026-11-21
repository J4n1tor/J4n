//! The frames of `claude -p --output-format stream-json`, replayed through the translator: what each recorded stream means in
//! J4n's words. The recordings are `fixtures/claude-frames/` (two captured on claude 2.1.309, the rest recorded).

use crate::world::{Scratch, frames};
use j4n_connect::claude::frames::{Line, OAUTH_TOKEN_ENV, Pending, Translator, Turn, bash_exit, clip, effect};
use j4n_core::ask::AskShape;
use j4n_core::doer::{Limit, Metrics, Role, RunEv};
use j4n_core::gate::{ReadOnly, Verb};
use serde_json::json;
use std::collections::BTreeMap;

fn turn(dir: &std::path::Path) -> Turn {
    Turn { task: "t1".into(), dir: dir.to_path_buf(), read_only: ReadOnly::No, compacting: false }
}

struct Replay {
    evs: Vec<RunEv>,
    asks: Vec<Pending>,
    sessions: Vec<String>,
    ends: usize,
}

fn replay_lines(lines: &[String], env: &BTreeMap<String, String>) -> Replay {
    let dir = Scratch::new("replay");
    let t = turn(dir.path());
    let mut tr = Translator::new(env);
    let mut r = Replay { evs: Vec::new(), asks: Vec::new(), sessions: Vec::new(), ends: 0 };
    for l in lines {
        let Line { evs, session, turn_end, ask, .. } = tr.line(l, Some(&t));
        r.evs.extend(evs);
        r.asks.extend(ask);
        r.sessions.extend(session);
        r.ends += usize::from(turn_end);
    }
    r
}

fn replay(name: &str) -> Replay {
    replay_lines(&frames(name), &BTreeMap::new())
}

fn texts(evs: &[RunEv]) -> Vec<&str> {
    evs.iter().filter_map(|e| if let RunEv::Text(t) = e { Some(t.as_str()) } else { None }).collect()
}

fn one(line: serde_json::Value) -> Vec<RunEv> {
    replay_lines(&[line.to_string()], &BTreeMap::new()).evs
}

// ---- recorded turns ----

#[test]
fn a_failed_command_is_a_shell_event_with_its_real_exit_code() {
    let r = replay("bash-exit-3-2.1.309.jsonl");
    assert_eq!(r.ends, 1, "the result frame ends the turn once");
    assert!(r.evs.iter().any(|e| matches!(e, RunEv::Shell { cmd, out, exit: 3 } if cmd == "sh -c 'echo oops >&2; exit 3'" && out == "oops")), "{:?}", r.evs);
    assert!(
        r.evs.iter().any(|e| matches!(e, RunEv::Tool { name, detail, end: false } if name == "Bash" && detail.contains("exits 3"))),
        "the call is said before its result: {:?}",
        r.evs
    );
    assert!(r.sessions.iter().all(|s| s == "00000000-0000-4000-8000-0000000000a1") && !r.sessions.is_empty());
}

#[test]
fn the_report_is_the_text_that_settled_last_and_it_is_the_loops_own_tail() {
    let r = replay("bash-exit-3-2.1.309.jsonl");
    let said = "The command exited with code 3 and printed \"oops\" to stderr.";
    assert_eq!(texts(&r.evs).concat(), said, "streamed once, not again when it settled");
    assert!(matches!(r.evs.last(), Some(RunEv::End { answer: Some(a), todo_left: 0 }) if a == said), "{:?}", r.evs.last());
}

#[test]
fn the_question_for_a_command_carries_the_effect_it_asks_about() {
    let r = replay("bash-exit-3-2.1.309.jsonl");
    let [ask] = &mut r.asks.clone()[..] else { panic!("one question: {:?}", r.asks) };
    let spec = ask.spec().expect("a permission question");
    assert_eq!(spec.local, "req-1");
    assert!(spec.inband);
    assert!(matches!(&spec.shape, AskShape::Approval { action, risk: 50 } if action.starts_with("Bash: sh -c")), "{:?}", spec.shape);
    let act = spec.act.expect("the Gate is shown the command");
    assert_eq!((act.verb, act.target.as_str(), act.task.as_deref()), (Verb::Shell, "sh -c 'echo oops >&2; exit 3'", Some("t1")));
    assert_eq!(act.actor, j4n_core::change::Actor::Doer("claude".into()));
}

#[test]
fn the_meter_is_the_turns_own_spend_and_a_warning_window_is_said_once() {
    let r = replay("bash-exit-3-2.1.309.jsonl");
    let m: Vec<&Metrics> = r.evs.iter().filter_map(|e| if let RunEv::Metrics(m) = e { Some(m) } else { None }).collect();
    assert_eq!(m.len(), 1);
    assert!((m[0].usd - 0.0025).abs() < 1e-9 && m[0].tokens_in == 70_004 && m[0].tokens_out == 150, "4 fresh + 60,000 read + 10,000 written: {m:?}");
    let notes: Vec<&String> = r.evs.iter().filter_map(|e| if let RunEv::Notice(n) = e { Some(n) } else { None }).collect();
    // the window is said in words, with the reset on the operator's clock; the status token and the epoch are not shown
    assert!(matches!(&notes[..], [n] if n.starts_with("claude · 40% of the 7-day limit used · resets ")), "{notes:?}");
    let n = notes[0];
    assert!(!n.contains("allowed_warning") && !n.contains("seven_day") && n.split(|c: char| !c.is_ascii_digit()).all(|d| d.len() < 6), "{n}");
}

#[test]
fn a_window_running_low_is_told_in_words_with_the_reset_on_the_operators_clock() {
    use j4n_connect::claude::frames::limit_words;
    let at = |t: i64| format!("<{t}>");
    assert_eq!(limit_words("five_hour", Some(0.92), Some(1794000000), at), "claude · 92% of the 5-hour limit used · resets <1794000000>");
    assert_eq!(limit_words("seven_day_opus", Some(0.5), None, at), "claude · 50% of the 7-day opus limit used");
    assert_eq!(limit_words("five_hour", None, Some(7), at), "claude · close to the 5-hour limit · resets <7>");
    assert_eq!(limit_words("", Some(0.8), None, at), "claude · 80% of the usage limit used");
}

#[test]
fn an_ask_user_question_is_a_choice_one_question_at_a_time_with_the_tools_own_labels() {
    let r = replay("ask-question-2.1.309.jsonl");
    let [ask] = &mut r.asks.clone()[..] else { panic!("one question: {:?}", r.asks) };
    assert_eq!(ask.questions.len(), 1);
    let spec = ask.spec().expect("the first question");
    assert_eq!(spec.question, "Which colour do you prefer?");
    assert_eq!(spec.local, "req-1#1");
    let AskShape::Choice { options } = &spec.shape else { panic!("{:?}", spec.shape) };
    let shown: Vec<(&str, &str)> = options.iter().map(|o| (o.id.as_str(), o.label.as_str())).collect();
    assert_eq!(shown, vec![("1", "Red — Choose red"), ("2", "Blue — Choose blue")]);
    assert_eq!(ask.labels, vec!["Red", "Blue"], "what a pick means to the tool");
    assert!(spec.act.is_none(), "a question is no effect");
    ask.answers.push(("Which colour do you prefer?".into(), "Red".into()));
    assert!(ask.spec().is_none(), "that was the last one");
    assert_eq!(ask.answered_input()["answers"]["Which colour do you prefer?"], "Red");
    assert!(matches!(r.evs.last(), Some(RunEv::End { answer: Some(a), .. }) if a == "Red."));
}

#[test]
fn an_older_read_turn_still_reads_and_the_new_kinds_of_frame_say_nothing() {
    let r = replay("read-turn-2.1.295.jsonl");
    assert_eq!(r.ends, 1);
    assert!(r.evs.iter().any(|e| matches!(e, RunEv::Tool { name, end: false, .. } if name == "Read")));
    assert!(r.evs.iter().any(|e| matches!(e, RunEv::Tool { name, end: true, detail } if name == "Read" && detail.contains("hello"))));
    assert!(!r.evs.iter().any(|e| matches!(e, RunEv::Notice(_) | RunEv::Error(_))), "{:?}", r.evs);
    assert!(matches!(r.evs.last(), Some(RunEv::End { answer: Some(a), .. }) if a.trim() == "hello"));
    let m = r.evs.iter().find_map(|e| if let RunEv::Metrics(m) = e { Some(*m) } else { None }).expect("a meter");
    // the recorded `usage` is 4 fresh + 20,000 read from the cache + 20,300 written to it
    assert_eq!((m.tokens_in, m.tokens_out, m.cached, m.wrote), (40_304, 154, 20_000, 20_300), "every token the model read, the cache parts apart");
    assert!(m.usd > 0.0);
    // `tokens_in` adds the three calls up (40,304 for a window of 20,302): what the session holds is the last call's, its floor the first's
    assert_eq!((m.first, m.last), (20_002, 20_302), "the first call: 2 + 20,000 written; the last: 2 + 300 written + 20,000 read");
}

#[test]
fn a_result_counts_every_token_the_model_read_and_keeps_the_cache_parts_apart() {
    let result = |usage: serde_json::Value| json!({ "type": "result", "subtype": "success", "result": "x", "total_cost_usd": 0.333, "usage": usage }).to_string();
    let metrics = |usage: serde_json::Value| Translator::new(&BTreeMap::new()).line(&result(usage), None).evs.into_iter().find_map(|e| if let RunEv::Metrics(m) = e { Some(m) } else { None }).expect("a meter");
    // the operator's call: 2 fresh tokens, about 100k from the cache
    let m = metrics(json!({ "input_tokens": 2, "cache_read_input_tokens": 99_000, "cache_creation_input_tokens": 1_000, "output_tokens": 73 }));
    assert_eq!((m.tokens_in, m.tokens_out, m.cached, m.wrote), (100_002, 73, 99_000, 1_000));
    // a CLI that says no cache keys: the fresh input, and nothing cached
    let m = metrics(json!({ "input_tokens": 12, "output_tokens": 3 }));
    assert_eq!((m.tokens_in, m.cached, m.wrote), (12, 0, 0));
}

#[test]
fn a_tool_turn_reasoning_tools_and_a_result_that_names_only_an_id() {
    let all = frames("tool-turn.jsonl");
    let first = all.iter().position(|l| l.contains("\"type\":\"result\"")).expect("a result") + 1;
    let r = replay_lines(&all[..first], &BTreeMap::new());
    assert!(r.evs.iter().any(|e| matches!(e, RunEv::Message { role: Role::Reasoning, text } if text.contains("list the files"))));
    // the result frame names only the id: the command comes from the call
    assert!(r.evs.iter().any(|e| matches!(e, RunEv::Shell { cmd, out, exit: 0 } if cmd == "ls -1" && out.contains("Cargo.toml"))));
    assert!(r.evs.iter().any(|e| matches!(e, RunEv::Tool { name, detail, end: false } if name == "Bash" && detail == "List files")));
    // array-form content on a flagged result: a failed Read
    assert!(r.evs.iter().any(|e| matches!(e, RunEv::Tool { name, detail, end: true } if name == "Read" && detail.starts_with("failed: ") && detail.contains("[package]"))));
    // settled text that was never streamed has its place, once
    assert_eq!(texts(&r.evs), vec!["Two entries: Cargo.toml and src."]);
    assert!(matches!(r.evs.last(), Some(RunEv::End { answer: Some(a), .. }) if a == "Two entries: Cargo.toml and src."));
}

#[test]
fn a_refused_login_is_an_error_with_the_clis_words_and_with_a_token_the_first_retry_says_so() {
    let all = frames("a0-2.1.296.jsonl");
    let r = replay_lines(&all, &BTreeMap::new());
    assert!(!r.evs.iter().any(|e| matches!(e, RunEv::End { .. })), "a failed turn never reports an end");
    assert!(r.evs.iter().any(|e| matches!(e, RunEv::Error(m) if m.contains("Failed to authenticate"))), "{:?}", r.evs);
    assert_eq!(r.evs.iter().filter(|e| matches!(e, RunEv::Error(_))).count(), 1, "no token: the CLI may still refresh it, the retry says nothing");
    let env = BTreeMap::from([(OAUTH_TOKEN_ENV.to_string(), "sk-secret-value".to_string())]);
    let r = replay_lines(&all, &env);
    let errors: Vec<&String> = r.evs.iter().filter_map(|e| if let RunEv::Error(m) = e { Some(m) } else { None }).collect();
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors[0].contains("refused (401)") && errors.iter().all(|m| !m.contains("sk-secret")), "{errors:?}");
    let twice = replay_lines(&[all[1].clone(), all[1].clone()], &env);
    assert_eq!(twice.evs.len(), 1, "said once, however often the CLI retries: {:?}", twice.evs);
    let retry = all[1].replace("401", "529");
    assert!(replay_lines(&[retry], &env).evs.is_empty(), "a busy server is not a refused token");
    let empty = BTreeMap::from([(OAUTH_TOKEN_ENV.to_string(), String::new())]);
    assert!(replay_lines(&all[1..2], &empty).evs.is_empty(), "an empty token is none");
}

// ---- single frames ----

fn result_with(extra: serde_json::Value) -> Vec<RunEv> {
    let mut v = json!({ "type": "result", "usage": {}, "session_id": "s", "total_cost_usd": 0.0 });
    v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    one(v)
}

#[test]
fn a_failed_result_is_an_error_and_a_good_one_is_not() {
    let err = |evs: &[RunEv]| evs.iter().find_map(|e| if let RunEv::Error(m) = e { Some(m.clone()) } else { None });
    assert_eq!(err(&result_with(json!({ "subtype": "error_max_budget_usd" }))).as_deref(), Some("error_max_budget_usd"));
    let during = result_with(json!({ "subtype": "error_during_execution", "result": "boom" }));
    assert_eq!(err(&during).as_deref(), Some("error_during_execution: boom"));
    let limit = result_with(json!({ "subtype": "success", "is_error": true, "result": "You've hit your limit · resets 3pm" }));
    assert!(err(&limit).is_some_and(|m| m.contains("hit your limit")));
    let silent = result_with(json!({ "subtype": "success", "is_error": true }));
    assert_eq!(err(&silent).as_deref(), Some("claude reported an error"));
    assert!(!silent.iter().any(|e| matches!(e, RunEv::End { .. })), "a failed turn has no end");
    let good = result_with(json!({ "subtype": "success", "is_error": false, "result": "ok" }));
    assert!(err(&good).is_none() && matches!(good.last(), Some(RunEv::End { answer: Some(a), .. }) if a == "ok"), "{good:?}");
    let mute = result_with(json!({ "subtype": "success", "is_error": false }));
    assert!(matches!(mute.last(), Some(RunEv::End { answer: None, .. })), "no text is no answer: the loop decides what that is");
}

#[test]
fn a_rejected_window_is_an_error_in_the_words_the_failure_table_knows_and_an_allowed_one_says_nothing() {
    let rejected = one(json!({ "type": "rate_limit_event", "rate_limit_info": { "status": "rejected", "resetsAt": 1792000000 } }));
    assert!(matches!(&rejected[..], [RunEv::Error(m)] if m == "rate limit: claude window rejected (resets at 1792000000)"), "{rejected:?}");
    assert!(matches!(&one(json!({ "type": "rate_limit_event", "rate_limit_info": { "status": "blocked" } }))[..], [RunEv::Error(m)] if m.starts_with("rate limit:")));
    assert!(one(json!({ "type": "rate_limit_event", "rate_limit_info": { "status": "allowed" } })).is_empty());
    assert!(one(json!({ "type": "rate_limit_event" })).is_empty());
}

/// An allowed turn's event as claude 2.1.310 writes it: the windows are fractions of 1 that move in steps of 0.01.
fn allowed(five: f64, week: f64) -> serde_json::Value {
    json!({ "type": "rate_limit_event", "rate_limit_info": { "status": "allowed", "resetsAt": 1793000000, "rateLimitType": "five_hour", "overageStatus": "rejected", "isUsingOverage": false,
        "unifiedWindows": { "five_hour": { "utilization": five, "resetsAt": 1793000000, "status": "allowed" }, "seven_day": { "utilization": week, "resetsAt": 1793600000, "status": "allowed" } } } })
}

fn limit(window: &str, utilization: f64, resets_at: i64, status: &str) -> Limit {
    Limit { window: window.into(), utilization, resets_at, status: status.into() }
}

#[test]
fn the_windows_of_an_allowed_turn_are_read_in_a_fixed_order_and_a_stream_without_them_is_no_error() {
    let evs = one(allowed(0.03, 0.0));
    assert_eq!(evs, vec![RunEv::Limits(vec![limit("five_hour", 0.03, 1793000000, "allowed"), limit("seven_day", 0.0, 1793600000, "allowed")])]);
    assert_eq!(one(allowed(0.0, 0.0)).len(), 1, "a window at nothing is still a reading");
    // a window with no number, or no reset time, is left out; the others are kept
    let partial = one(json!({ "type": "rate_limit_event", "rate_limit_info": { "status": "allowed",
        "unifiedWindows": { "five_hour": { "resetsAt": 5 }, "seven_day": { "utilization": 0.5, "resetsAt": 9, "status": "allowed_warning" }, "seven_day_opus": { "utilization": 0.1 } } } }));
    assert_eq!(partial, vec![RunEv::Limits(vec![limit("seven_day", 0.5, 9, "allowed_warning")])]);
    assert!(one(json!({ "type": "rate_limit_event", "rate_limit_info": { "status": "allowed", "unifiedWindows": {} } })).is_empty());
}

#[test]
fn a_rejected_window_with_windows_gives_the_readings_and_then_the_error() {
    let mut event = allowed(1.0, 0.4);
    event["rate_limit_info"]["status"] = json!("rejected");
    let evs = one(event);
    assert!(matches!(&evs[..], [RunEv::Limits(l), RunEv::Error(m)] if l.len() == 2 && m.starts_with("rate limit: claude window rejected")), "{evs:?}");
}

#[test]
fn a_reused_child_reports_each_turn_once() {
    let result = |total: f64| json!({ "type": "result", "subtype": "success", "result": "x", "total_cost_usd": total, "usage": { "input_tokens": 1, "output_tokens": 1 } }).to_string();
    let mut tr = Translator::new(&BTreeMap::new());
    let usd = |l: Line| l.evs.iter().find_map(|e| if let RunEv::Metrics(m) = e { Some(m.usd) } else { None }).unwrap();
    // the child reports a running total: 0.04, then 0.08, then (restarted) 0.04 again
    assert!((usd(tr.line(&result(0.04), None)) - 0.04).abs() < 1e-12);
    assert!((usd(tr.line(&result(0.08), None)) - 0.04).abs() < 1e-12);
    assert_eq!(usd(tr.line(&result(0.04), None)), 0.0, "a total that went down adds nothing, and is never negative");
    assert_eq!(usd(tr.line(&result(0.05), None)), 0.0, "the high-water mark stays where it was, so a total below it adds nothing either");
    assert!((usd(tr.line(&result(0.09), None)) - 0.01).abs() < 1e-12, "and one above it adds the difference");
}

#[test]
fn text_that_was_streamed_is_said_once_and_text_that_was_not_is_still_said() {
    let delta = |t: &str| json!({ "type": "stream_event", "event": { "type": "content_block_delta", "delta": { "type": "text_delta", "text": t } } }).to_string();
    let block = |t: &str| json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": t }] } }).to_string();
    let said = |lines: Vec<String>| texts(&replay_lines(&lines, &BTreeMap::new()).evs).concat();
    assert_eq!(said(vec![delta("Hel"), delta("lo"), block("Hello")]), "Hello");
    assert_eq!(said(vec![block("Hello")]), "Hello");
    assert_eq!(said(vec![delta("A"), block("A"), block("B")]), "AB", "the second block was never streamed");
    assert_eq!(said(vec![block("   ")]), "", "nothing to say");
    // a thinking delta with nothing in it says nothing
    let think = json!({ "type": "stream_event", "event": { "type": "content_block_delta", "delta": { "type": "thinking_delta", "thinking": "" } } });
    assert!(one(think).is_empty());
}

#[test]
fn the_narration_before_a_tool_is_not_the_report() {
    let delta = |t: &str| json!({ "type": "stream_event", "event": { "type": "content_block_delta", "delta": { "type": "text_delta", "text": t } } }).to_string();
    let tool = json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": "u1", "name": "Bash", "input": { "command": "ls" } }] } }).to_string();
    let result = json!({ "type": "result", "subtype": "success", "result": "Two files.", "total_cost_usd": 0.0, "usage": {} }).to_string();
    let r = replay_lines(&[delta("Let me look."), tool, delta("Two files."), result], &BTreeMap::new());
    assert!(matches!(r.evs.last(), Some(RunEv::End { answer: Some(a), .. }) if a == "Two files."));
    let kinds: Vec<&str> = r
        .evs
        .iter()
        .map(|e| match e {
            RunEv::Text(_) => "text",
            RunEv::Tool { .. } => "tool",
            RunEv::Metrics(_) => "metrics",
            RunEv::End { .. } => "end",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, vec!["text", "tool", "text", "metrics", "end"], "the narration comes before the tool, the report after it");
}

#[test]
fn a_line_that_is_not_json_is_said_at_first_and_then_only_counted() {
    let lines: Vec<String> = (0..6).map(|i| format!("garbage {i}")).collect();
    let r = replay_lines(&lines, &BTreeMap::new());
    let notes: Vec<&RunEv> = r.evs.iter().collect();
    assert_eq!(notes.len(), 3, "{notes:?}");
    assert!(matches!(notes[0], RunEv::Notice(n) if n.contains("not JSON") && n.contains("garbage 0")));
    assert!(replay_lines(&["".into(), "   ".into()], &BTreeMap::new()).evs.is_empty(), "a blank line is nothing");
    assert_eq!(one(json!({ "type": "something_new", "x": 1 })), [RunEv::Unread { dialect: "claude-stream".into(), kind: "something_new".into() }], "a frame of a kind never seen is skipped, and said");
}

#[test]
fn a_kind_of_frame_that_cannot_be_read_is_said_once_in_a_run_and_the_kinds_that_are_heard_say_nothing() {
    let mut tr = Translator::new(&BTreeMap::new());
    let said = |tr: &mut Translator, line: serde_json::Value| tr.line(&line.to_string(), None).evs;
    let unread = |kind: &str| vec![RunEv::Unread { dialect: "claude-stream".into(), kind: kind.into() }];
    assert_eq!(said(&mut tr, json!({ "type": "mystery_frame", "n": 1 })), unread("mystery_frame"));
    assert!(said(&mut tr, json!({ "type": "mystery_frame", "n": 2 })).is_empty(), "once for each kind");
    assert_eq!(said(&mut tr, json!({ "type": "other_mystery" })), unread("other_mystery"), "another kind is another thing to say");
    for heard in ["assistant", "user", "result", "system", "stream_event", "control_request", "control_response", "control_cancel_request", "rate_limit_event", "keep_alive", "tool_progress", "auth_status", "tool_use_summary", "prompt_suggestion", "streamlined_text", "streamlined_tool_use_summary"] {
        assert!(!said(&mut tr, json!({ "type": heard })).iter().any(|e| matches!(e, RunEv::Unread { .. })), "{heard}");
    }
    assert!(said(&mut tr, json!({ "no_type": 1 })).is_empty(), "a frame with no kind is not a kind");
    let long = "k".repeat(200);
    assert!(matches!(said(&mut tr, json!({ "type": long })).as_slice(), [RunEv::Unread { kind, .. }] if kind.len() == 60), "a kind is clipped");
    // a new run is a new translator and says it again
    assert_eq!(said(&mut Translator::new(&BTreeMap::new()), json!({ "type": "mystery_frame" })), unread("mystery_frame"));
}

#[test]
fn a_question_cannot_be_asked_outside_a_turn_and_a_cancelled_one_is_named() {
    let req = json!({ "type": "control_request", "request_id": "r9", "request": { "subtype": "can_use_tool", "tool_name": "Bash", "input": { "command": "ls" } } }).to_string();
    let mut tr = Translator::new(&BTreeMap::new());
    assert!(tr.line(&req, None).ask.is_none());
    let other = json!({ "type": "control_request", "request_id": "r1", "request": { "subtype": "hook_callback" } }).to_string();
    let dir = Scratch::new("t");
    assert!(tr.line(&other, Some(&turn(dir.path()))).ask.is_none(), "only can_use_tool is a question");
    let cancel = json!({ "type": "control_cancel_request", "request_id": "r9" }).to_string();
    assert_eq!(tr.line(&cancel, None).cancelled.as_deref(), Some("r9"));
}

/// The child asks its next question the moment it is answered, so the question can join the queue before or after the answered one leaves it.
/// Either way it is put to the loop once: a second put is the engine's "asked a second question while the first was open".
#[test]
fn a_question_that_joins_as_the_answered_one_leaves_is_put_to_the_loop_once() {
    use j4n_connect::claude::session::{answered, joined};
    let dir = Scratch::new("frames-path");
    let mut tr = Translator::new(&BTreeMap::new());
    let mut ask = |id: &str| {
        let line = json!({ "type": "control_request", "request_id": id, "request": { "subtype": "can_use_tool", "tool_name": "Bash", "input": { "command": "ls" } } }).to_string();
        tr.line(&line, Some(&turn(dir.path()))).ask.expect("a question")
    };
    let mut asks = std::collections::VecDeque::new();
    // the order that raced: the answer leaves, then the next question joins an empty queue; the one who joins puts it, and the one who answered had nothing to put
    assert!(joined(&mut asks, ask("r0")).is_some(), "the first question is put at once");
    assert!(answered(&mut asks).is_none(), "the queue is empty when the child has asked nothing yet");
    let put = joined(&mut asks, ask("r1"));
    assert!(put.is_some_and(|s| s.local.contains("r1")), "it is put by the one that finds it at the head");
    // why a second look at the head after that is a second put: the head reads as the same question again
    assert!(asks.front_mut().and_then(Pending::spec).is_some(), "the head is still the question that was put");
    // the other order: it joins while the answered one is still in the queue, waits, and is put by the answer
    assert!(joined(&mut asks, ask("r2")).is_none(), "a question that waits for the head is not put");
    assert!(answered(&mut asks).is_some_and(|s| s.local.contains("r2")), "the answer puts the next one");
    assert!(answered(&mut asks).is_none() && asks.is_empty());
}

// ---- helpers ----

#[test]
fn the_exit_code_of_a_bash_result() {
    assert_eq!(bash_exit("3", false), (0, "3"));
    assert_eq!(bash_exit("Exit code 3\noops", true), (3, "oops"));
    assert_eq!(bash_exit("Exit code 127\n", true), (127, ""));
    assert_eq!(bash_exit("Exit code 3", true), (3, ""));
    assert_eq!(bash_exit("Exit code abc\nx", true), (1, "Exit code abc\nx"), "a failure that names no code is 1");
    assert_eq!(bash_exit("boom", true), (1, "boom"));
    assert_eq!(bash_exit("Exit code 3\nx", false), (0, "Exit code 3\nx"), "an unflagged result is a success whatever it says");
}

#[test]
fn clip_cuts_on_characters() {
    assert_eq!(clip("héllo wörld", 5), "héllo…");
    assert_eq!(clip("short", 5), "short");
    assert_eq!(clip("", 0), "");
}

#[test]
fn the_effect_a_tool_asks_about_is_one_the_gate_knows() {
    let dir = Scratch::new("effect");
    let other = Scratch::new("outside");
    let t = turn(dir.path());
    let at = |tool: &str, input: serde_json::Value| effect(tool, &input, &t);
    let shell = at("Bash", json!({ "command": "make build" })).unwrap();
    assert_eq!((shell.verb, shell.target.as_str(), shell.inside), (Verb::Shell, "make build", None));
    assert_eq!(shell.site.as_deref(), Some(dir.path().to_str().unwrap()));
    let inside = |tool, path: &str| at(tool, json!({ "file_path": path })).unwrap();
    assert_eq!((inside("Edit", "src/a.rs").verb, inside("Edit", "src/a.rs").inside), (Verb::Write, Some(true)));
    assert_eq!(inside("Write", dir.join("new/dir/x.txt").to_str().unwrap()).inside, Some(true), "a file that does not exist yet");
    assert_eq!(inside("Write", other.join("x").to_str().unwrap()).inside, Some(false));
    assert_eq!(inside("MultiEdit", "../x").inside, Some(false));
    assert_eq!(inside("Edit", "a/../../x").inside, Some(false));
    assert_eq!(inside("Edit", "a/../b").inside, Some(true));
    assert_eq!(at("NotebookEdit", json!({ "notebook_path": "n.ipynb" })).unwrap().verb, Verb::Write);
    assert_eq!((inside("Read", "/etc/passwd").verb, inside("Read", "/etc/passwd").inside), (Verb::Read, Some(false)));
    assert_eq!(at("Grep", json!({ "pattern": "x" })).unwrap().target, ".", "a search without a path is of the folder");
    let net = at("WebFetch", json!({ "url": "https://User@Docs.Example.com:8443/a/b?c#d" })).unwrap();
    assert_eq!((net.verb, net.target.as_str()), (Verb::Net, "docs.example.com"));
    assert_eq!(at("WebFetch", json!({ "url": "example.org/x" })).unwrap().target, "example.org");
    assert_eq!(at("WebSearch", json!({ "query": "q" })).unwrap().verb, Verb::Net);
    assert_eq!(at("Task", json!({ "subagent_type": "Explore" })).unwrap().target, "Explore");
    assert!(at("TodoWrite", json!({})).is_none() && at("mcp__x__y", json!({})).is_none(), "what is no effect the Gate knows is the operator's");
    assert!(at("Bash", json!({})).is_none() && at("Edit", json!({})).is_none(), "an effect with no target is no effect");
    let read_only = Turn { read_only: ReadOnly::Explicit, ..turn(dir.path()) };
    assert_eq!(effect("Edit", &json!({ "file_path": "a" }), &read_only).unwrap().read_only, ReadOnly::Explicit);
}

#[test]
fn a_path_through_a_link_that_leaves_the_project_is_outside_it() {
    let dir = Scratch::new("links");
    let elsewhere = Scratch::new("elsewhere");
    std::os::unix::fs::symlink(elsewhere.path(), dir.join("out")).unwrap();
    let t = turn(dir.path());
    let w = |p: &str| effect("Write", &json!({ "file_path": p }), &t).unwrap().inside;
    assert_eq!(w("out/x.txt"), Some(false), "the link is followed");
    assert_eq!(w("out/deeper/new/x.txt"), Some(false));
    assert_eq!(w("real/x.txt"), Some(true));
}

// ---- what the loop's tail and the report are made of ----

fn delta(t: &str) -> String {
    json!({ "type": "stream_event", "event": { "type": "content_block_delta", "delta": { "type": "text_delta", "text": t } } }).to_string()
}

fn tool_use(name: &str, input: serde_json::Value) -> String {
    json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": "u1", "name": name, "input": input }] } }).to_string()
}

fn tool_result(content: &str, is_error: bool) -> String {
    json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": "u1", "content": content, "is_error": is_error }] } }).to_string()
}

fn finish(text: &str) -> String {
    json!({ "type": "result", "subtype": "success", "result": text, "total_cost_usd": 0.0, "usage": {} }).to_string()
}

fn answer_of(lines: Vec<String>) -> Option<String> {
    match replay_lines(&lines, &BTreeMap::new()).evs.pop() {
        Some(RunEv::End { answer, .. }) => answer,
        other => panic!("no end: {other:?}"),
    }
}

#[test]
fn the_answer_is_the_loops_tail_to_the_letter_so_the_loop_files_it_once() {
    // the report differs from the words only in the white space around them: the answer is the very words that were said
    assert_eq!(answer_of(vec![delta("Hi "), finish("Hi")]).as_deref(), Some("Hi "));
    // what was said before a tool call is not part of it, whether the call or its result is what ends the narration
    assert_eq!(answer_of(vec![delta("A\n"), tool_use("Read", json!({})), delta("Done\n"), finish("Done")]).as_deref(), Some("Done\n"));
    assert_eq!(answer_of(vec![delta("A\n"), tool_result("x", false), delta("Done\n"), finish("Done")]).as_deref(), Some("Done\n"));
    // and a report that is not what was streamed is the report
    assert_eq!(answer_of(vec![delta("Working"), finish("All done")]).as_deref(), Some("All done"));
}

#[test]
fn a_frame_without_a_session_name_names_none() {
    let line = |sid: serde_json::Value| Translator::new(&BTreeMap::new()).line(&json!({ "type": "system", "subtype": "init", "session_id": sid }).to_string(), None).session;
    assert_eq!(line(json!("abc")).as_deref(), Some("abc"));
    assert_eq!(line(json!("")), None);
    assert_eq!(line(json!(null)), None);
}

#[test]
fn thinking_with_nothing_in_it_is_not_kept_and_thinking_with_something_is() {
    let thinking = |t: &str| json!({ "type": "assistant", "message": { "content": [{ "type": "thinking", "thinking": t, "signature": "sig" }] } });
    assert!(one(thinking("")).is_empty() && one(thinking("  \n")).is_empty());
    assert!(matches!(&one(thinking("plan"))[..], [RunEv::Message { role: Role::Reasoning, text }] if text == "plan"));
}

#[test]
fn a_tool_call_says_the_command_when_it_has_no_description_and_the_input_when_it_has_no_command() {
    let detail = |input| match &one(serde_json::from_str(&tool_use("Bash", input)).unwrap())[..] {
        [RunEv::Tool { detail, end: false, .. }] => detail.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(detail(json!({ "command": "ls -la" })), "ls -la");
    assert_eq!(detail(json!({ "command": "ls -la", "description": "list" })), "list");
    assert_eq!(detail(json!({ "pattern": "x" })), "{\"pattern\":\"x\"}");
    assert_eq!(detail(json!({ "command": "x".repeat(300) })), format!("{}…", "x".repeat(120)));
}

#[test]
fn the_paths_a_write_changed_are_said_when_it_went_through_and_not_when_it_did_not() {
    let changed = |tool: &str, input: serde_json::Value, is_error: bool| -> Vec<RunEv> {
        replay_lines(&[tool_use(tool, input), tool_result("x", is_error)], &BTreeMap::new()).evs.into_iter().filter(|e| matches!(e, RunEv::Changed(_))).collect()
    };
    let want = |p: &str| vec![RunEv::Changed(vec![p.to_string()])];
    assert_eq!(changed("Edit", json!({ "file_path": "src/a.rs" }), false), want("src/a.rs"));
    assert_eq!(changed("Write", json!({ "file_path": "b.txt" }), false), want("b.txt"));
    assert_eq!(changed("MultiEdit", json!({ "file_path": "c.rs" }), false), want("c.rs"));
    assert_eq!(changed("NotebookEdit", json!({ "notebook_path": "n.ipynb" }), false), want("n.ipynb"));
    assert!(changed("Edit", json!({ "file_path": "src/a.rs" }), true).is_empty(), "an edit that failed changed nothing");
    assert!(changed("Read", json!({ "file_path": "src/a.rs" }), false).is_empty(), "a read changes nothing");
    assert!(changed("Edit", json!({}), false).is_empty(), "no path, nothing to say");
}

#[test]
fn how_risky_a_question_looks_follows_the_effect() {
    let dir = Scratch::new("risk");
    let t = turn(dir.path());
    let risk = |tool: &str, input: serde_json::Value| {
        let line = json!({ "type": "control_request", "request_id": "r", "request": { "subtype": "can_use_tool", "tool_name": tool, "input": input } }).to_string();
        let mut p = Translator::new(&BTreeMap::new()).line(&line, Some(&t)).ask.expect("a question");
        match p.spec().expect("a spec").shape {
            AskShape::Approval { risk, .. } => risk,
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(risk("Read", json!({ "file_path": "a" })), 10);
    assert_eq!(risk("WebFetch", json!({ "url": "https://x.org" })), 30);
    assert_eq!(risk("Bash", json!({ "command": "ls" })), 50);
    assert_eq!(risk("Edit", json!({ "file_path": "a" })), 40);
    assert_eq!(risk("mcp__x__y", json!({})), 40, "what is no effect is asked about as a middling one");
}

#[test]
fn a_site_that_is_gone_still_has_an_inside() {
    let gone = Scratch::new("gone");
    let site = gone.join("not/made/yet");
    let t = Turn { dir: site, ..turn(gone.path()) };
    let w = |p: &str| effect("Write", &json!({ "file_path": p }), &t).unwrap().inside;
    assert_eq!(w("x.txt"), Some(true));
    assert_eq!(w("a/b/c/x.txt"), Some(true), "in order, folder by folder");
    assert_eq!(w("../x.txt"), Some(false));
    assert_eq!(w("a/../../x.txt"), Some(false));
}

#[test]
fn a_question_with_options_marks_the_first_recommended_one_and_offers_at_most_twelve() {
    let dir = Scratch::new("opts");
    let t = turn(dir.path());
    let opt = |l: &str| json!({ "label": l, "description": "" });
    let ask = |opts: Vec<serde_json::Value>| {
        let input = json!({ "questions": [{ "question": "Which?", "options": opts }] });
        let line = json!({ "type": "control_request", "request_id": "r", "request": { "subtype": "can_use_tool", "tool_name": "AskUserQuestion", "input": input } }).to_string();
        let mut p = Translator::new(&BTreeMap::new()).line(&line, Some(&t)).ask.expect("a question");
        (p.spec().expect("a spec"), p.labels.clone())
    };
    let (spec, _) = ask(vec![opt("A"), opt("B (Recommended)"), opt("C (recommended)")]);
    let AskShape::Choice { options } = spec.shape else { panic!() };
    assert_eq!(options.iter().map(|o| o.recommended).collect::<Vec<_>>(), vec![false, true, false], "one only, the first");
    assert_eq!(options[0].label, "A", "no description, no dash");
    let many: Vec<_> = (1..=15).map(|n| opt(&format!("o{n}"))).collect();
    let (spec, labels) = ask(many);
    let AskShape::Choice { options } = &spec.shape else { panic!() };
    assert_eq!((options.len(), labels.len()), (12, 12));
    assert!(spec.check().is_ok(), "something the loop can show");
    let (spec, _) = ask(vec![]);
    assert!(matches!(spec.shape, AskShape::Text), "no options: a line of text");
}

// ---- how full the window is ----

fn window_of(evs: &[RunEv]) -> Vec<f64> {
    evs.iter().filter_map(|e| if let RunEv::Window(w) = e { Some(*w) } else { None }).collect()
}

#[test]
fn the_window_is_what_the_last_call_put_in_it_over_the_models_window() {
    let r = replay("read-turn-2.1.295.jsonl");
    let w = window_of(&r.evs);
    assert_eq!(w.len(), 1, "once a turn: {:?}", r.evs);
    assert!((w[0] - 20_302.0 / 1_000_000.0).abs() < 1e-9, "the last call: 2 fresh + 300 + 20,000 cached, of the 1,000,000 the result names: {w:?}");
    let at = |p: fn(&RunEv) -> bool| r.evs.iter().position(p).unwrap();
    assert!(at(|e| matches!(e, RunEv::Window(_))) < at(|e| matches!(e, RunEv::End { .. })), "before the end, so the loop has it when the task settles");
}

#[test]
fn a_cli_that_does_not_say_what_a_call_held_gives_no_reading_rather_than_a_guess() {
    for name in ["bash-exit-3-2.1.309.jsonl", "ask-question-2.1.309.jsonl"] {
        assert!(window_of(&replay(name).evs).is_empty(), "{name}: the result adds the calls of the turn up, and that is not the window");
    }
}

#[test]
fn a_window_that_is_not_named_is_two_hundred_thousand_and_the_turn_after_starts_from_nothing() {
    let dir = Scratch::new("window");
    let t = turn(dir.path());
    let mut tr = Translator::new(&BTreeMap::new());
    let call = |tokens: u64| json!({ "type": "assistant", "message": { "content": [], "usage": { "input_tokens": 0, "cache_read_input_tokens": tokens, "cache_creation_input_tokens": 0 } } }).to_string();
    let result = json!({ "type": "result", "subtype": "success", "result": "ok", "is_error": false, "total_cost_usd": 0.0 }).to_string();
    tr.line(&call(100_000), Some(&t));
    tr.line(&call(140_000), Some(&t));
    assert_eq!(window_of(&tr.line(&result, Some(&t)).evs), [0.7], "the last call, of 200,000");
    assert!(window_of(&tr.line(&result, Some(&t)).evs).is_empty(), "a turn that made no call says nothing, not what the one before said");
    // a result that lists its calls says the last one's
    let listed = json!({ "type": "result", "subtype": "success", "result": "ok", "is_error": false, "usage": { "iterations": [{ "input_tokens": 10, "cache_read_input_tokens": 50_000 }, { "input_tokens": 10, "cache_read_input_tokens": 100_000, "cache_creation_input_tokens": 0 }] }, "modelUsage": { "m": { "contextWindow": 400_000 } } }).to_string();
    assert_eq!(window_of(&tr.line(&listed, Some(&t)).evs), [100_010.0 / 400_000.0]);
}

#[test]
fn a_turn_that_used_two_models_is_measured_against_the_largest_window() {
    let dir = Scratch::new("window2");
    let t = turn(dir.path());
    let mut tr = Translator::new(&BTreeMap::new());
    let result = json!({ "type": "result", "subtype": "success", "result": "ok", "is_error": false, "usage": { "iterations": [{ "input_tokens": 100_000 }] }, "modelUsage": { "small": { "contextWindow": 200_000 }, "big": { "contextWindow": 1_000_000 }, "none": { "contextWindow": 0 } } }).to_string();
    assert_eq!(window_of(&tr.line(&result, Some(&t)).evs), [0.1]);
}

#[test]
fn the_turn_that_shrinks_a_session_says_nothing_of_the_window_it_leaves() {
    let dir = Scratch::new("window");
    let shrinking = Turn { compacting: true, ..turn(dir.path()) };
    let mut tr = Translator::new(&BTreeMap::new());
    tr.line(&json!({ "type": "assistant", "message": { "content": [], "usage": { "input_tokens": 150_000 } } }).to_string(), Some(&shrinking));
    let r = tr.line(&json!({ "type": "result", "subtype": "success", "result": "compacted", "is_error": false }).to_string(), Some(&shrinking));
    assert!(window_of(&r.evs).is_empty(), "{:?}", r.evs);
    let after = tr.line(&json!({ "type": "result", "subtype": "success", "result": "ok", "is_error": false }).to_string(), Some(&turn(dir.path())));
    assert!(window_of(&after.evs).is_empty(), "and what it held before is not carried over");
}

#[test]
fn the_turn_that_shrinks_a_session_says_what_it_holds_after_when_the_cli_does_in_its_compact_boundary() {
    let dir = Scratch::new("boundary");
    let shrinking = Turn { compacting: true, ..turn(dir.path()) };
    let mut tr = Translator::new(&BTreeMap::new());
    // the turn's own call read the whole session to write the summary: that is not what the session holds
    tr.line(&json!({ "type": "assistant", "message": { "content": [], "usage": { "input_tokens": 150_000 } } }).to_string(), Some(&shrinking));
    tr.line(&json!({ "type": "system", "subtype": "compact_boundary", "compact_metadata": { "trigger": "manual", "pre_tokens": 150_000, "post_tokens": 20_000 } }).to_string(), Some(&shrinking));
    let r = tr.line(&json!({ "type": "result", "subtype": "success", "result": "compacted", "is_error": false, "usage": { "input_tokens": 150_000, "output_tokens": 900 }, "modelUsage": { "m": { "contextWindow": 200_000 } } }).to_string(), Some(&shrinking));
    let m = r.evs.iter().find_map(|e| if let RunEv::Metrics(m) = e { Some(*m) } else { None }).expect("a meter");
    assert_eq!((m.last, m.first, m.tokens_in), (20_000, 0, 150_000), "the size after, and the call that wrote the summary is only what it read: {m:?}");
    assert_eq!(window_of(&r.evs), [0.1]);
    // said once: the turn after it does not inherit it
    let next = tr.line(&json!({ "type": "result", "subtype": "success", "result": "ok", "is_error": false }).to_string(), Some(&turn(dir.path())));
    assert!(window_of(&next.evs).is_empty(), "{:?}", next.evs);
    // a boundary with no `post_tokens` says nothing of the size
    tr.line(&json!({ "type": "system", "subtype": "compact_boundary", "compact_metadata": { "trigger": "manual", "pre_tokens": 150_000 } }).to_string(), Some(&shrinking));
    let none = tr.line(&json!({ "type": "result", "subtype": "success", "result": "compacted", "is_error": false }).to_string(), Some(&shrinking));
    let m = none.evs.iter().find_map(|e| if let RunEv::Metrics(m) = e { Some(*m) } else { None });
    assert!(m.is_none_or(|m| m.last == 0) && window_of(&none.evs).is_empty(), "{:?}", none.evs);
}

// ---- the doer's own list ----

fn tool(id: &str, name: &str, input: serde_json::Value) -> String {
    json!({"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":id,"name":name,"input":input}]},"session_id":"s"}).to_string()
}

fn result() -> String {
    json!({"type":"result","subtype":"success","is_error":false,"result":"ok","session_id":"s","usage":{}}).to_string()
}

fn left(lines: &[String]) -> Option<u32> {
    replay_lines(lines, &BTreeMap::new()).evs.iter().rev().find_map(|e| if let RunEv::End { todo_left, .. } = e { Some(*todo_left) } else { None })
}

#[test]
fn a_list_that_is_all_completed_ends_with_nothing_left() {
    let lines: Vec<String> = [
        tool("a", "TaskCreate", json!({"subject": "x"})),
        tool("b", "TaskCreate", json!({"subject": "y"})),
        tool("c", "TaskUpdate", json!({"taskId": "1", "status": "completed"})),
        tool("d", "TaskUpdate", json!({"taskId": "2", "status": "completed"})),
        result(),
    ]
    .into();
    assert_eq!(left(&lines), Some(0));
    // a list never touched is not a list; a task put back to in_progress is open again
    assert_eq!(left(&[result()]), Some(0));
    let reopened = [tool("a", "TaskCreate", json!({})), tool("b", "TaskUpdate", json!({"taskId": "1", "status": "completed"})), tool("c", "TaskUpdate", json!({"taskId": "1", "status": "in_progress"})), result()];
    assert_eq!(left(&reopened), Some(1));
}

#[test]
fn an_older_claudes_todo_write_counts_the_same_and_an_unknown_list_tool_counts_as_nothing() {
    let todos = json!({"todos": [{"content": "a", "status": "completed"}, {"content": "b", "status": "in_progress"}, {"content": "c", "status": "pending"}]});
    assert_eq!(left(&[tool("a", "TodoWrite", todos), result()]), Some(2));
    // each TodoWrite is the whole list
    let again = json!({"todos": [{"content": "a", "status": "completed"}]});
    assert_eq!(left(&[tool("a", "TodoWrite", json!({"todos": [{"content": "a", "status": "pending"}]})), tool("b", "TodoWrite", again), result()]), Some(0));
    assert_eq!(left(&[tool("a", "PlanSomething", json!({"todos": [{"status": "pending"}]})), tool("b", "TaskGet", json!({"taskId": "1"})), result()]), Some(0));
    // a subagent's own list is not the doer's
    let sub = json!({"type":"assistant","parent_tool_use_id":"p","message":{"role":"assistant","content":[{"type":"tool_use","id":"z","name":"TaskCreate","input":{}}]},"session_id":"s"}).to_string();
    assert_eq!(left(&[sub, result()]), Some(0));
}

// ---- the doer's list is said ----

fn lists(lines: &[String]) -> Vec<(u32, u32, String)> {
    replay_lines(lines, &BTreeMap::new()).evs.iter().filter_map(|e| if let RunEv::List { open, total, active } = e { Some((*open, *total, active.clone())) } else { None }).collect()
}

#[test]
fn a_change_of_the_doers_list_is_said_once_with_the_words_of_the_item_in_progress() {
    let create = |i: &str, form: serde_json::Value| tool(i, "TaskCreate", form);
    let lines = [
        create("a", json!({"subject": "Write tests", "activeForm": "writing tests"})),
        create("b", json!({"subject": "Fix the docs"})),
        tool("c", "TaskUpdate", json!({"taskId": "1", "status": "in_progress"})),
        tool("d", "TaskUpdate", json!({"taskId": "1", "status": "in_progress"})),
        tool("e", "TaskUpdate", json!({"taskId": "1", "status": "completed"})),
        tool("f", "TaskUpdate", json!({"taskId": 2, "status": "in_progress"})),
        tool("g", "TaskUpdate", json!({"taskId": "2", "status": "completed"})),
        result(),
    ];
    let all = lists(&lines);
    let said: Vec<(u32, u32, &str)> = all.iter().map(|(o, t, a)| (*o, *t, a.as_str())).collect();
    assert_eq!(said, [(1, 1, ""), (2, 2, ""), (2, 2, "writing tests"), (1, 2, ""), (1, 2, "Fix the docs"), (0, 2, "")], "the same update twice says nothing the second time");
}

#[test]
fn an_older_claudes_todo_write_is_said_whole_and_what_is_not_the_doers_own_list_is_not_said() {
    let todos = json!({"todos": [{"content": "a", "activeForm": "doing a", "status": "completed"}, {"content": "b", "activeForm": "doing b", "status": "in_progress"}, {"content": "c", "status": "pending"}]});
    assert_eq!(lists(&[tool("a", "TodoWrite", todos), result()]), [(2, 3, "doing b".to_string())]);
    assert!(lists(&[tool("a", "TodoWrite", json!({"todos": []})), tool("b", "PlanSomething", json!({})), result()]).is_empty(), "an empty list and an unknown tool say nothing");
    let sub = json!({"type":"assistant","parent_tool_use_id":"p","message":{"role":"assistant","content":[{"type":"tool_use","id":"z","name":"TaskCreate","input":{"subject":"x"}}]},"session_id":"s"}).to_string();
    assert!(lists(&[sub, result()]).is_empty(), "a subagent's list is not the doer's");
}

/// The models and the cost a replay said, `(served models in order, the usd of each `run.metrics`)`.
fn priced(r: &Replay) -> (Vec<String>, Vec<f64>) {
    let served = r.evs.iter().filter_map(|e| if let RunEv::Served(s) = e { Some(s.model.clone()) } else { None }).collect();
    (served, r.evs.iter().filter_map(|e| if let RunEv::Metrics(m) = e { Some(m.usd) } else { None }).collect())
}

#[test]
fn a_resumed_session_carries_old_models_and_cost_and_the_turn_is_priced_as_what_it_did() {
    // a `ping`: haiku did it; the session's saved cost state held earlier turns on sonnet ($0.30), and the whole $0.40 was booked
    let (served, usd) = priced(&replay("resumed-carried-cost-2.1.310.jsonl"));
    assert_eq!(served, ["claude-haiku-5-5"], "said once: the model of the turn, not the one that cost most in the old state");
    assert_eq!(usd.len(), 1);
    assert!((usd[0] - 0.1).abs() < 1e-6, "the turn's own cost, not the session's: {usd:?}");
}

#[test]
fn a_child_that_is_kept_books_each_turn_once_and_an_entry_with_old_tokens_in_it_is_shared() {
    let dir = Scratch::new("carried");
    let t = turn(dir.path());
    let mut tr = Translator::new(&BTreeMap::new());
    let call = |id: &str, out: u64| json!({ "type": "assistant", "message": { "id": id, "model": "claude-haiku-5-5", "content": [], "usage": { "input_tokens": 0, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 1000, "output_tokens": out } } }).to_string();
    let result = |total: f64, wrote: u64, out: u64, entry: (u64, u64, f64)| {
        json!({ "type": "result", "subtype": "success", "result": "ok", "is_error": false, "total_cost_usd": total,
            "usage": { "input_tokens": 0, "cache_creation_input_tokens": wrote, "output_tokens": out },
            "modelUsage": { "claude-haiku-5-5": { "inputTokens": 0, "cacheCreationInputTokens": entry.0, "outputTokens": entry.1, "costUSD": entry.2 } } })
        .to_string()
    };
    let usd = |tr: &mut Translator, line: String| -> f64 { tr.line(&line, Some(&t)).evs.iter().find_map(|e| if let RunEv::Metrics(m) = e { Some(m.usd) } else { None }).unwrap() };
    // the saved state holds 3,000 written tokens ($3.0 of them) and this turn wrote 1,000 more: the entry is 4,000 and $4.0, the turn's share a quarter
    tr.line(&call("m1", 0), Some(&t));
    let first = usd(&mut tr, result(4.0, 1000, 0, (4000, 0, 4.0)));
    assert!((first - 1.0).abs() < 1e-9, "{first}");
    // the next turn of the same child adds 1,000 more and the entry grows by them: only what it added is booked
    tr.line(&call("m2", 0), Some(&t));
    let second = usd(&mut tr, result(5.0, 1000, 0, (5000, 0, 5.0)));
    assert!((second - 1.0).abs() < 1e-9, "{second}");
}

#[test]
fn the_model_of_a_child_is_said_once_for_each_turn_and_again_when_it_changes() {
    let t = turn(Scratch::new("served").path());
    let mut tr = Translator::new(&BTreeMap::new());
    let served = |evs: &[RunEv]| evs.iter().filter_map(|e| if let RunEv::Served(s) = e { Some((s.model.clone(), s.host.clone())) } else { None }).collect::<Vec<_>>();
    let mut say = |v: serde_json::Value| tr.line(&v.to_string(), Some(&t)).evs;
    let haiku = || ("claude-haiku-5-5".to_string(), Some("anthropic".to_string()));
    assert_eq!(served(&say(json!({ "type": "system", "subtype": "init", "model": "claude-haiku-5-5" }))), [haiku()]);
    let call = json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": "x" }] } });
    assert!(served(&say(call.clone())).is_empty(), "not on every frame");
    let result = json!({ "type": "result", "subtype": "success", "result": "x", "modelUsage": { "claude-haiku-5-5": { "costUSD": 0.1 } } });
    assert!(served(&say(result.clone())).is_empty(), "the model that did the turn is the one that was said");
    // the child is kept: the next turn hears it again, at its first frame
    assert_eq!(served(&say(call.clone())), [haiku()]);
    assert!(served(&say(call)).is_empty());
    // a placeholder is no model
    assert!(served(&say(json!({ "type": "system", "subtype": "init", "model": "<synthetic>" }))).is_empty());
}

#[test]
fn each_call_of_a_turn_says_what_the_turn_has_used_so_far_and_a_result_starts_the_next_from_nothing() {
    let call = |id: &str, input: u64, cached: u64, out: u64| {
        json!({ "type": "assistant", "message": { "id": id, "model": "claude-haiku-5-5", "content": [{ "type": "text", "text": "x" }], "usage": { "input_tokens": input, "cache_read_input_tokens": cached, "cache_creation_input_tokens": 0, "output_tokens": out } } }).to_string()
    };
    let spends = |l: &Line| l.evs.iter().filter_map(|e| if let RunEv::Spend(u) = e { Some(*u) } else { None }).collect::<Vec<_>>();
    let mut tr = Translator::new(&BTreeMap::new());
    // 100 fresh + 0.1 × 10,000 cached + 5 × 200 out = 2,100; the second call adds 50 + 0.1 × 12,000 + 5 × 300 = 2,750 to it
    assert_eq!(spends(&tr.line(&call("m1", 100, 10_000, 200), None)), [2_100.0]);
    assert_eq!(spends(&tr.line(&call("m2", 50, 12_000, 300), None)), [4_850.0]);
    // the same call said again (a partial and then the whole) replaces itself and is not added twice
    assert_eq!(spends(&tr.line(&call("m2", 50, 12_000, 400), None)), [5_350.0]);
    // a frame with no usage says nothing
    let bare = json!({ "type": "assistant", "message": { "id": "m3", "model": "claude-haiku-5-5", "content": [{ "type": "text", "text": "x" }] } }).to_string();
    assert_eq!(spends(&tr.line(&bare, None)), [5_350.0], "it has no usage, so the sum stands as it was");
    // the result prices the turn and the next turn starts from nothing
    let result = json!({ "type": "result", "subtype": "success", "result": "x", "total_cost_usd": 0.05, "usage": { "input_tokens": 150, "cache_read_input_tokens": 22_000, "output_tokens": 700 } }).to_string();
    tr.line(&result, None);
    assert_eq!(spends(&tr.line(&call("m4", 10, 0, 100), None)), [510.0]);
}

// ---- a call being written ----

/// A 24 kB file written in one call: the stream says what it is about and how big it is while it is written, in a few steps and not one for each
/// fragment, with the model's tokens as a count for the turn; none of it is a Tool event, and the settled call and the result frame say what they did.
#[test]
fn a_long_write_is_said_as_a_call_that_grows_before_the_call_ends() {
    let delta = |j: &str| json!({ "type": "stream_event", "api_message_id": "m1", "parent_tool_use_id": null, "event": { "type": "content_block_delta", "index": 1, "delta": { "type": "input_json_delta", "partial_json": j } } }).to_string();
    let mut lines = vec![delta(""), delta("{\"file_path\": \"/w/chess"), delta(".py\", \"content\": \"")];
    lines.extend((0..600).map(|_| delta(&"x".repeat(40))));
    let r = replay_lines(&lines, &BTreeMap::new());
    let opens: Vec<(&str, &str, u64)> = r.evs.iter().filter_map(|e| if let RunEv::Open { name, target, bytes, .. } = e { Some((name.as_str(), target.as_str(), *bytes)) } else { None }).collect();
    assert_eq!(opens.first().map(|o| (o.0, o.1)), Some(("", "")), "the first fragment: a call, not known yet");
    assert!(opens.iter().any(|o| o.0 == "Write" && o.1 == "/w/chess.py"), "the keys say Write and the path has parsed: {opens:?}");
    let sizes: Vec<u64> = opens.iter().map(|o| o.2).collect();
    assert!(sizes.windows(2).all(|w| w[0] <= w[1]) && sizes.last().copied().unwrap_or(0) > 20_000, "it grows: {sizes:?}");
    assert!(opens.len() <= 3 + 24_000 / 512 + 2 && opens.len() < lines.len() / 10, "a few steps, not one for each of {} fragments: {}", lines.len(), opens.len());
    assert!(r.evs.iter().all(|e| !matches!(e, RunEv::Tool { .. })), "the call has not ended");
    let counts: Vec<u64> = r.evs.iter().filter_map(|e| if let RunEv::Tokens(n) = e { Some(*n) } else { None }).collect();
    assert!(counts.windows(2).all(|w| w[0] < w[1]) && counts.last().copied().unwrap_or(0) > 5_000, "{counts:?}");
    assert!(counts.len() <= 24_000 / 4 / 40 + 2 && counts.len() < lines.len() / 3, "a count for each 40 tokens at the most: {}", counts.len());
}

#[test]
fn the_calls_of_two_streams_and_the_end_of_a_block_do_not_mix() {
    let delta = |parent: Option<&str>, at: u64, j: &str| json!({ "type": "stream_event", "parent_tool_use_id": parent, "event": { "type": "content_block_delta", "index": at, "delta": { "type": "input_json_delta", "partial_json": j } } }).to_string();
    let stop = |at: u64| json!({ "type": "stream_event", "parent_tool_use_id": null, "event": { "type": "content_block_stop", "index": at } }).to_string();
    let r = replay_lines(&[delta(None, 0, "{\"command\": \"ls\", \"x"), delta(Some("agent1"), 0, "{\"old_string\": \"a\", \"file_path\": \"b.rs\""), stop(0), delta(None, 0, "{\"pattern\": \"fn \", \"a\": 1")], &BTreeMap::new());
    let opens: Vec<(String, String, String)> = r.evs.iter().filter_map(|e| if let RunEv::Open { id, name, target, .. } = e { Some((id.clone(), name.clone(), target.clone())) } else { None }).collect();
    assert_eq!(opens[0], ("0".into(), "Bash".into(), "ls".into()));
    assert_eq!(opens[1], ("agent1:0".into(), "Edit".into(), "b.rs".into()), "a sub-agent's call has an id of its own");
    assert_eq!(opens[2], ("0".into(), "Grep".into(), "fn ".into()), "the block that ended is not carried on into the next one at its index");
}

#[test]
fn what_the_model_wrote_is_counted_by_message_and_settles_with_the_message_delta_and_the_result() {
    let text = |m: &str, t: &str| json!({ "type": "stream_event", "api_message_id": m, "event": { "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": t } } }).to_string();
    let closing = |m: &str, n: u64| json!({ "type": "stream_event", "api_message_id": m, "event": { "type": "message_delta", "delta": { "stop_reason": "end_turn" }, "usage": { "output_tokens": n } } }).to_string();
    let result = json!({ "type": "result", "subtype": "success", "result": "", "total_cost_usd": 0.0, "usage": { "output_tokens": 900 } }).to_string();
    let counts = |lines: &[String]| -> Vec<u64> { replay_lines(lines, &BTreeMap::new()).evs.iter().filter_map(|e| if let RunEv::Tokens(n) = e { Some(*n) } else { None }).collect() };
    // 800 bytes are about 200 tokens; the message delta says 500, and a second message adds to it: the count is the turn's
    assert_eq!(counts(&[text("a", &"w".repeat(800)), closing("a", 500), text("b", &"w".repeat(800))]), [200, 500, 700]);
    // a turn's count starts again at its result
    assert_eq!(counts(&[text("a", &"w".repeat(800)), result, text("b", &"w".repeat(800))]), [200, 200]);
    // a thinking the CLI keeps to itself says only how long it is, and says that the model thinks
    let hidden = json!({ "type": "stream_event", "event": { "type": "content_block_delta", "index": 0, "delta": { "type": "thinking_delta", "thinking": "", "estimated_tokens_delta": 50 } } });
    assert_eq!(one(hidden), [RunEv::Reasoning(String::new()), RunEv::Tokens(50)]);
}
