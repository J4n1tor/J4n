//! A model as a doer, through the real loop: its turns, its calls run in order and asked about, its conversation carried on,
//! the endpoint it is told to use, and what it cost.

use crate::bench::{Bench, base_layer};
use crate::mock::{Mock, Say};
use crate::world::Scratch;
use j4n_connect::model::doer::{Config, Dialect, Gated, ModelDoer, ToolOut, Tools, action};
use j4n_connect::model::{Call, Tool};
use j4n_connect::net::Net;
use j4n_core::ask::Answer;
use j4n_core::change::Actor;
use j4n_core::doer::Doer;
use j4n_core::gate::{ReadOnly, Verb};
use j4n_core::place::{Launch, Site};
use j4n_core::table::Tables;
use j4n_core::task::{State, TaskSpec};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

fn delta(d: Value) -> String {
    json!({ "choices": [{ "delta": d }] }).to_string()
}

fn finish(r: &str) -> String {
    json!({ "choices": [{ "delta": {}, "finish_reason": r }] }).to_string()
}

fn usage(i: u64, o: u64, cached: u64) -> String {
    json!({ "choices": [], "provider": "Cerebras", "usage": { "prompt_tokens": i, "completion_tokens": o, "prompt_tokens_details": { "cached_tokens": cached } } }).to_string()
}

fn answer(text: &str) -> Say {
    Say::Sse(vec![delta(json!({ "content": text })), finish("stop"), usage(100, 5, 80)])
}

fn calls(list: &[(&str, &str, &str)]) -> Say {
    let frags: Vec<Value> = list.iter().enumerate().map(|(i, (id, name, args))| json!({ "index": i, "id": id, "function": { "name": name, "arguments": args } })).collect();
    Say::Sse(vec![delta(json!({ "reasoning": "I will look first." })), delta(json!({ "tool_calls": frags })), finish("tool_calls"), usage(60, 20, 0)])
}

/// `echo` needs no question, `write` does; both note what they were given.
struct Scripted(Mutex<Vec<String>>);

impl Tools for Scripted {
    fn specs(&self) -> Vec<Tool> {
        vec![
            Tool { name: "echo".into(), doc: "say it back".into(), schema: json!({ "type": "object" }) },
            Tool { name: "write".into(), doc: "write a file".into(), schema: json!({ "type": "object" }) },
        ]
    }

    fn gate(&self, call: &Call, task: &str, site: &Site, ro: ReadOnly) -> Option<Gated> {
        (call.name == "write").then(|| Gated { action: action("gpt", Verb::Write, "out.txt", task, site, ro), question: "write out.txt?".into(), risk: 40 })
    }

    fn run(&self, call: &Call, _: &Site, _: &(dyn Fn(Launch) -> Launch + Send + Sync)) -> ToolOut {
        self.0.lock().unwrap().push(format!("{} {}", call.name, call.args));
        ToolOut { text: format!("{} done", call.name), error: false, changed: if call.name == "write" { vec!["out.txt".into()] } else { Vec::new() }, shell: (call.name == "sh").then(|| j4n_connect::model::doer::ShellOut { cmd: "make".into(), out: "boom".into(), exit: 3 }), py: None }
    }
}

fn config(mock: &Mock) -> Config {
    Config {
        id: "gpt".into(),
        dialect: Dialect::Openai,
        base: format!("{}/v1", mock.url()),
        key: "k-secret".into(),
        keyless: false,
        net: Net::new().with_env(|_| None),
        model: "vendor/m".into(),
        system: "You are brief.".into(),
        context: Some(1000),
        effort: None,
    }
}

fn bench(doer: ModelDoer) -> Bench {
    let (tables, _) = Tables::load(vec![base_layer()]);
    Bench::over(Scratch::new("bin"), vec![Arc::new(doer) as Arc<dyn Doer>], &tables)
}

#[test]
fn a_model_answers_and_says_what_it_cost_and_who_served_it() {
    let mock = Mock::start(|_, _| answer("Four."));
    let b = bench(ModelDoer::new(config(&mock)));
    let row = b.run("gpt", "what is 2+2?");
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}\n{}", row.state, b.dump());
    assert_eq!(row.answer.as_deref(), Some("Four."));
    let served = &b.events("run.served")[0].data;
    assert_eq!(served["host"], "Cerebras");
    assert_eq!((served["input"].clone(), served["cached"].clone(), served["output"].clone(), served["model"].clone()), (json!(100), json!(80), json!(5), json!("vendor/m")));
    assert!(served["ttft_ms"].is_u64());
    let m = &b.events("run.metrics")[0].data;
    assert_eq!((m["tokens_in"].clone(), m["tokens_out"].clone()), (json!(100), json!(5)));
    let req = mock.seen()[0].json();
    assert_eq!(req["messages"][0], json!({ "role": "system", "content": "You are brief." }));
    assert_eq!(req["messages"][1], json!({ "role": "user", "content": "what is 2+2?" }));
    assert_eq!(req["model"], "vendor/m");
    // the gateway is told the session, so it keeps one host and its cache; no host is named when none was elected
    assert!(req["session_id"].as_str().unwrap().starts_with('m'), "{req}");
    assert!(req.get("provider").is_none());
    assert_eq!(b.events("run.session")[0].data["id"], req["session_id"]);
}

#[test]
fn every_call_of_a_reply_runs_in_order_and_the_one_that_needs_a_yes_is_asked_l4() {
    let mock = Mock::start(|_, n| if n == 0 { calls(&[("c1", "echo", "{\"a\":1}"), ("c2", "write", "{\"p\":\"out.txt\"}"), ("c3", "echo", "{\"a\":3}")]) } else { answer("All three ran.") });
    let tools = Arc::new(Scripted(Mutex::default()));
    let b = bench(ModelDoer::new(config(&mock)).with_tools(tools.clone()));
    let row = b.run("gpt", "do three things");
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}\n{}", row.state, b.dump());
    assert_eq!(*tools.0.lock().unwrap(), ["echo {\"a\":1}", "write {\"p\":\"out.txt\"}", "echo {\"a\":3}"]);
    let names: Vec<String> = b.events("run.tool_call").iter().map(|e| e.data["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, ["echo", "write", "echo"]);
    assert_eq!(b.events("run.tool_result").len(), 3);
    assert_eq!(b.events("run.changed")[0].data["paths"], json!(["out.txt"]));
    // the second request holds the first reply's calls, with the reasoning that led to them, and their three results
    let second = mock.seen()[1].json();
    let m = second["messages"].as_array().unwrap();
    assert_eq!(m[2]["tool_calls"].as_array().unwrap().len(), 3);
    assert_eq!(m[2]["reasoning"], "I will look first.");
    assert_eq!(m[3], json!({ "role": "tool", "tool_call_id": "c1", "content": "echo done" }));
    assert_eq!(m[4]["tool_call_id"], "c2");
    assert_eq!(m[5]["tool_call_id"], "c3");
    assert_eq!(second["tools"].as_array().unwrap().len(), 2);
    assert_eq!(mock.seen()[0].json()["tools"], second["tools"], "the same tools every turn");
    assert_eq!(b.events("run.served").len(), 2);
}

#[test]
fn a_no_is_the_calls_result_and_the_model_goes_on() {
    let mock = Mock::start(|_, n| if n == 0 { calls(&[("c1", "write", "{}")]) } else { answer("Understood, I did not write.") });
    let tools = Arc::new(Scripted(Mutex::default()));
    let b = bench(ModelDoer::new(config(&mock)).with_tools(tools.clone()));
    let id = b.spawn("gpt", "write it");
    let ask = b.waiting(&id);
    b.answer(&ask, Answer::No);
    let row = b.end(&id);
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}\n{}", row.state, b.dump());
    assert!(tools.0.lock().unwrap().is_empty(), "nothing ran");
    let result = &mock.seen()[1].json()["messages"][3];
    assert!(result["content"].as_str().unwrap().contains("was not allowed"), "{result}");
}

#[test]
fn a_call_whose_arguments_are_not_json_is_not_run_and_is_told_why_l6() {
    let mock = Mock::start(|_, n| if n == 0 { calls(&[("c1", "echo", "{\"text\":\"a long file that was cut o")]) } else { answer("Shorter then.") });
    let tools = Arc::new(Scripted(Mutex::default()));
    let b = bench(ModelDoer::new(config(&mock)).with_tools(tools.clone()));
    assert!(matches!(b.run("gpt", "go").state, State::Done { ok: true }));
    assert!(tools.0.lock().unwrap().is_empty());
    let result = mock.seen()[1].json()["messages"][3]["content"].as_str().unwrap().to_string();
    assert!(result.contains("not valid JSON") && result.contains("cut off"), "{result}");
}

#[test]
fn a_later_task_carries_on_in_the_conversation_of_the_first() {
    let mock = Mock::start(|_, n| answer(&format!("answer {n}")));
    let b = bench(ModelDoer::new(config(&mock)));
    let first = b.run("gpt", "remember 42");
    let mut spec = TaskSpec::new("what did I say?", "gpt");
    spec.follows = Some(first.id.clone());
    spec.resume = first.session.clone();
    let id = b.tasks.spawn(spec, Actor::Operator).unwrap();
    assert!(matches!(b.end(&id).state, State::Done { ok: true }));
    let second = mock.seen()[1].json();
    let contents: Vec<&str> = second["messages"].as_array().unwrap().iter().filter_map(|m| m["content"].as_str()).collect();
    assert_eq!(contents, ["You are brief.", "remember 42", "answer 0", "what did I say?"]);
    assert_eq!(second["session_id"], mock.seen()[0].json()["session_id"]);
}

#[test]
fn a_conversation_that_is_gone_starts_again_and_says_so() {
    let mock = Mock::start(|_, _| answer("hi"));
    let b = bench(ModelDoer::new(config(&mock)));
    let mut spec = TaskSpec::new("go on", "gpt");
    spec.resume = Some(j4n_core::task::SessionRef { doer: "gpt".into(), id: "m1-77".into() });
    let id = b.tasks.spawn(spec, Actor::Operator).unwrap();
    assert!(matches!(b.end(&id).state, State::Done { ok: true }));
    assert!(b.events("run.notice").iter().any(|e| e.data["text"].as_str().unwrap().contains("was gone")));
}

#[test]
fn the_elected_host_is_the_only_one_asked_for() {
    let mock = Mock::start(|_, _| answer("ok"));
    let b = bench(ModelDoer::new(config(&mock)));
    let mut spec = TaskSpec::new("go", "gpt");
    spec.tune.endpoint = Some("Cerebras".into());
    spec.tune.model = Some("other/model".into());
    let id = b.tasks.spawn(spec, Actor::Operator).unwrap();
    assert!(matches!(b.end(&id).state, State::Done { ok: true }));
    let req = mock.seen()[0].json();
    assert_eq!(req["provider"], json!({ "order": ["Cerebras"], "allow_fallbacks": false }));
    assert_eq!(req["model"], "other/model");
}

#[test]
fn a_limit_ends_the_task_in_words_the_failure_table_knows() {
    let mock = Mock::start(|_, _| Say::Text(429, "{\"error\":{\"message\":\"slow down\"}}".into()));
    let b = bench(ModelDoer::new(config(&mock)));
    let row = b.run("gpt", "go");
    assert!(matches!(row.state, State::Done { ok: false } | State::Stopped { .. }), "{:?}\n{}", row.state, b.dump());
    let e = &b.events("run.error")[0].data;
    assert!(e["text"].as_str().unwrap().starts_with("429 rate limit"), "{e}");
    assert!(!b.dump().contains("k-secret"));
}

#[test]
fn a_model_without_a_key_does_not_start_and_a_local_one_needs_none() {
    let mock = Mock::start(|_, _| answer("local"));
    let mut cfg = config(&mock);
    cfg.key = String::new();
    let doer = ModelDoer::new(cfg.clone());
    assert!(!doer.probe().authed && doer.probe().detail == "no key");
    let b = bench(doer);
    let id = b.tasks.spawn(TaskSpec::new("go", "gpt"), Actor::Operator);
    assert!(id.is_err() || matches!(b.end(&id.unwrap()).state, State::Done { ok: false } | State::Stopped { .. }));
    assert!(mock.seen().is_empty(), "nothing was sent");
    cfg.keyless = true;
    let b = bench(ModelDoer::new(cfg));
    assert!(matches!(b.run("gpt", "go").state, State::Done { ok: true }));
    assert!(!mock.seen()[0].headers.contains_key("authorization"));
}

#[test]
fn how_full_the_window_is_comes_from_the_prompt_of_the_last_call() {
    let mock = Mock::start(|_, _| answer("ok"));
    let b = bench(ModelDoer::new(config(&mock)));
    let row = b.run("gpt", "go");
    assert!(matches!(row.state, State::Done { ok: true }));
    let settled = &b.events("task.settled")[0].data;
    assert_eq!(settled["window"], json!(0.1), "100 of 1000 tokens");
}

#[test]
fn a_limit_that_says_how_long_is_told_with_the_time_it_ends_and_the_conversation_is_not_doubled_when_it_goes_on() {
    let mock = Mock::start(|_, n| if n == 0 { Say::Headed(429, vec![("Retry-After", "7".into())], "{\"error\":{\"message\":\"slow\"}}".into()) } else { answer("done") });
    let b = bench(ModelDoer::new(config(&mock)));
    let first = b.run("gpt", "do it");
    let text = b.events("run.error")[0].data["text"].as_str().unwrap().to_string();
    assert!(text.starts_with("429 rate limit (resets at ") && text.contains("slow"), "{text}");
    // the task goes on in the same conversation, as the cooling rule would have it: its words are not said twice
    let mut spec = TaskSpec::new("do it", "gpt");
    spec.follows = Some(first.id.clone());
    spec.resume = first.session.clone();
    let id = b.tasks.spawn(spec, Actor::Operator).unwrap();
    assert!(matches!(b.end(&id).state, State::Done { ok: true }), "{}", b.dump());
    let second = mock.seen()[1].json();
    let users: Vec<&str> = second["messages"].as_array().unwrap().iter().filter(|m| m["role"] == "user").filter_map(|m| m["content"].as_str()).collect();
    assert_eq!(users, ["do it"]);
}

#[test]
fn what_the_model_thought_before_its_calls_is_kept_in_the_log_for_rebuilding_the_thread_l5() {
    let mock = Mock::start(|_, n| if n == 0 { calls(&[("c1", "echo", "{}")]) } else { answer("done") });
    let b = bench(ModelDoer::new(config(&mock)).with_tools(Arc::new(Scripted(Mutex::default()))));
    assert!(matches!(b.run("gpt", "go").state, State::Done { ok: true }));
    let kept = b.events("run.reasoning");
    assert_eq!(kept.len(), 1, "{}", b.dump());
    assert_eq!(kept[0].data["text"], "I will look first.");
}

#[test]
fn a_steer_while_the_model_works_is_said_to_it_on_its_next_turn() {
    let mock = Mock::start(|_, n| {
        if n == 0 {
            std::thread::sleep(std::time::Duration::from_millis(400));
            calls(&[("c1", "echo", "{}")])
        } else {
            answer("noted")
        }
    });
    let b = bench(ModelDoer::new(config(&mock)).with_tools(Arc::new(Scripted(Mutex::default()))));
    let id = b.spawn("gpt", "work");
    while mock.seen().is_empty() {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    b.tasks.ctl(&id, j4n_core::task::Ctl::Steer("and use tabs".into()), Actor::Operator).unwrap();
    assert!(matches!(b.end(&id).state, State::Done { ok: true }), "{}", b.dump());
    let second = mock.seen()[1].json();
    let roles: Vec<(&str, &str)> = second["messages"].as_array().unwrap().iter().map(|m| (m["role"].as_str().unwrap(), m["content"].as_str().unwrap_or(""))).collect();
    assert_eq!(roles.last(), Some(&("user", "and use tabs")), "{roles:?}");
    assert_eq!(roles[roles.len() - 2], ("tool", "echo done"));
}

#[test]
fn a_stop_while_a_question_waits_ends_the_run_and_asks_the_gateway_nothing_more() {
    let mock = Mock::start(|_, _| calls(&[("c1", "write", "{}")]));
    let tools = Arc::new(Scripted(Mutex::default()));
    let b = bench(ModelDoer::new(config(&mock)).with_tools(tools.clone()));
    let id = b.spawn("gpt", "write it");
    b.waiting(&id);
    b.tasks.ctl(&id, j4n_core::task::Ctl::Abort, Actor::Operator).unwrap();
    assert!(matches!(b.end(&id).state, State::Stopped { .. }), "{}", b.dump());
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(mock.seen().len(), 1);
    assert!(tools.0.lock().unwrap().is_empty());
}

#[test]
fn a_model_that_never_stops_calling_is_made_to_answer_on_its_last_turn_and_stopped_after_it() {
    // a different call every turn, so it is not the unchanged-reply stop that ends it
    let mock = Mock::start(|_, n| calls(&[("c", "echo", &format!("{{\"n\":{n}}}"))]));
    let b = bench(ModelDoer::new(config(&mock)).with_tools(Arc::new(Scripted(Mutex::default()))));
    let row = b.run("gpt", "loop");
    assert!(!matches!(row.state, State::Done { ok: true }), "{:?}", row.state);
    assert_eq!(mock.seen().len(), 100);
    assert!(b.events("run.error")[0].data["text"].as_str().unwrap().contains("100 turns"));
    // the last turn is for the answer: told so, with no tools to call
    let last = mock.seen()[99].json();
    assert!(last.get("tools").is_none_or(|t| t.as_array().is_some_and(Vec::is_empty)), "{last}");
    assert!(last["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap().contains("Step budget reached"));
    assert!(mock.seen()[98].json()["tools"].as_array().is_some_and(|t| !t.is_empty()), "every turn before it offers the same tools");
}

#[test]
fn a_reply_that_says_the_same_again_ends_the_run_and_reasoning_is_not_part_of_the_comparison() {
    // the same call twice in a row, though the thinking in front of it differs each time
    let mock = Mock::start(|_, n| {
        Say::Sse(vec![delta(json!({ "reasoning": format!("thought {n}") })), delta(json!({ "tool_calls": [{ "index": 0, "id": "c", "function": { "name": "echo", "arguments": "{}" } }] })), finish("tool_calls"), usage(60, 20, 0)])
    });
    let b = bench(ModelDoer::new(config(&mock)).with_tools(Arc::new(Scripted(Mutex::default()))));
    let row = b.run("gpt", "loop");
    assert!(!matches!(row.state, State::Done { ok: true }), "{:?}", row.state);
    assert_eq!(mock.seen().len(), 2, "the second reply was the same as the first");
    assert!(b.events("run.error")[0].data["text"].as_str().unwrap().contains("same reply twice"));
    // the second request carries the first reply's thinking, sent back with its call
    let second = mock.seen()[1].json();
    let reasoning = second["messages"].as_array().unwrap().iter().filter_map(|m| m.get("reasoning").or_else(|| m.get("reasoning_content"))).collect::<Vec<_>>();
    assert!(reasoning.iter().any(|r| r.to_string().contains("thought 0")), "{second}");
}

#[test]
fn the_messages_dialect_runs_the_same_loop() {
    let sse = |blocks: Vec<Value>| Say::Sse(blocks.into_iter().map(|b| b.to_string()).collect());
    let mock = Mock::start(move |_, n| {
        if n == 0 {
            sse(vec![
                json!({ "type": "message_start", "message": { "model": "claude-x", "usage": { "input_tokens": 30, "cache_read_input_tokens": 0 } } }),
                json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "tool_use", "id": "tu1", "name": "echo", "input": {} } }),
                json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "input_json_delta", "partial_json": "{\"a\":1}" } }),
                json!({ "type": "message_delta", "delta": { "stop_reason": "tool_use" }, "usage": { "output_tokens": 9 } }),
            ])
        } else {
            sse(vec![
                json!({ "type": "message_start", "message": { "model": "claude-x", "usage": { "input_tokens": 50, "cache_read_input_tokens": 30 } } }),
                json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "text", "text": "" } }),
                json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "All done." } }),
                json!({ "type": "message_delta", "delta": { "stop_reason": "end_turn" }, "usage": { "output_tokens": 4 } }),
            ])
        }
    });
    let mut cfg = config(&mock);
    cfg.dialect = Dialect::Anthropic;
    cfg.base = mock.url();
    cfg.model = "claude-x".into();
    let tools = Arc::new(Scripted(Mutex::default()));
    let b = bench(ModelDoer::new(cfg).with_tools(tools.clone()));
    let row = b.run("gpt", "go");
    assert_eq!(row.answer.as_deref(), Some("All done."), "{}", b.dump());
    assert_eq!(*tools.0.lock().unwrap(), ["echo {\"a\":1}"]);
    let second = mock.seen()[1].json();
    assert_eq!(second["messages"][2]["content"][0], json!({ "type": "tool_result", "tool_use_id": "tu1", "content": "echo done" }));
    assert_eq!(mock.seen()[0].path, "/v1/messages");
    let served = &b.events("run.served")[1].data;
    assert_eq!((served["input"].clone(), served["cached"].clone()), (json!(80), json!(30)));
}

#[test]
fn what_a_doer_can_do_and_who_it_is() {
    let mock = Mock::start(|_, _| answer("x"));
    let d = ModelDoer::new(config(&mock));
    let caps = d.caps();
    assert!(caps.live && caps.resume && !caps.compact, "{caps:?}");
    assert_eq!(d.id(), "gpt");
    let p = d.probe();
    assert!(p.installed && p.authed && p.detail.is_empty());
}

#[test]
fn a_session_of_another_doer_is_not_carried_on_in() {
    let mock = Mock::start(|_, _| answer("hi"));
    let b = bench(ModelDoer::new(config(&mock)));
    let mut spec = TaskSpec::new("go on", "gpt");
    spec.resume = Some(j4n_core::task::SessionRef { doer: "claude".into(), id: "s-1".into() });
    let id = b.tasks.spawn(spec, Actor::Operator).unwrap();
    assert!(matches!(b.end(&id).state, State::Done { ok: true }));
    let msgs = mock.seen()[0].json()["messages"].as_array().unwrap().len();
    assert_eq!(msgs, 2, "system and the goal: a new conversation");
}

#[test]
fn a_session_id_that_exists_but_is_another_doers_is_not_carried_on_in() {
    let mock = Mock::start(|_, _| answer("hi"));
    let b = bench(ModelDoer::new(config(&mock)));
    let first = b.run("gpt", "remember 42");
    let mut spec = TaskSpec::new("what?", "gpt");
    spec.resume = first.session.clone().map(|s| j4n_core::task::SessionRef { doer: "claude".into(), id: s.id });
    let id = b.tasks.spawn(spec, Actor::Operator).unwrap();
    assert!(matches!(b.end(&id).state, State::Done { ok: true }));
    let contents: Vec<String> = mock.seen()[1].json()["messages"].as_array().unwrap().iter().filter_map(|m| m["content"].as_str().map(String::from)).collect();
    assert_eq!(contents, ["You are brief.", "what?"], "the other doer's session is not this doer's conversation");
}

#[test]
fn what_the_gateway_says_a_call_cost_is_what_the_task_spent() {
    let mock = Mock::start(|_, _| Say::Sse(vec![delta(json!({ "content": "ok" })), finish("stop"), json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 2, "cost": 0.0042 } }).to_string()]));
    let b = bench(ModelDoer::new(config(&mock)));
    let row = b.run("gpt", "go");
    assert_eq!(row.spent, 4_200, "millionths of a dollar");
    assert_eq!(b.events("run.metrics")[0].data["usd"], json!(0.0042));
}

#[test]
fn a_stopped_run_does_not_end_with_an_error() {
    let mock = Mock::start(|_, _| calls(&[("c1", "write", "{}")]));
    let b = bench(ModelDoer::new(config(&mock)).with_tools(Arc::new(Scripted(Mutex::default()))));
    let id = b.spawn("gpt", "write it");
    b.waiting(&id);
    b.tasks.ctl(&id, j4n_core::task::Ctl::Abort, Actor::Operator).unwrap();
    b.end(&id);
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(b.events("run.error").is_empty(), "{}", b.dump());
}

#[test]
fn a_task_may_ask_for_fewer_turns_and_never_for_more_than_a_hundred() {
    let run = |visits: u32| {
        let mock = Mock::start(|_, n| calls(&[("c", "echo", &format!("{{\"n\":{n}}}"))]));
        let b = bench(ModelDoer::new(config(&mock)).with_tools(Arc::new(Scripted(Mutex::default()))));
        let mut spec = TaskSpec::new("loop", "gpt");
        spec.limits.visits = Some(visits);
        let id = b.tasks.spawn(spec, Actor::Operator).unwrap();
        b.end(&id);
        mock.seen().len()
    };
    assert_eq!(run(3), 3);
    assert_eq!(run(500), 100);
}

#[test]
fn the_brief_rides_in_the_system_message_beside_the_system_prompt_and_is_not_kept_as_history() {
    let mock = Mock::start(|_, _| answer("ok"));
    let b = bench(ModelDoer::new(config(&mock)));
    let mut spec = TaskSpec::new("hello", "gpt");
    spec.input = json!({ "brief": "## contract\nBe brief." });
    let id = b.tasks.spawn(spec, Actor::Operator).unwrap();
    b.end(&id);
    let req = mock.seen()[0].json();
    assert_eq!(req["messages"][0]["content"], "You are brief.\n\n## contract\nBe brief.");
    assert_eq!(req["messages"][1], json!({ "role": "user", "content": "hello" }));
    let mut again = TaskSpec::new("more", "gpt");
    again.input = json!({ "brief": "## contract\nOther." });
    again.resume = b.tasks.book().get(&id).unwrap().session;
    let id2 = b.tasks.spawn(again, Actor::Operator).unwrap();
    b.end(&id2);
    let req = mock.seen()[1].json();
    assert!(req["messages"][0]["content"].as_str().unwrap().ends_with("Other.") && !req.to_string().contains("Be brief."), "{req}");
}

#[test]
fn a_command_a_tool_ran_is_a_shell_event_with_its_real_exit_code() {
    let mock = Mock::start(|_, n| if n == 0 { calls(&[("c1", "sh", "{}")]) } else { answer("It failed with 3.") });
    let b = bench(ModelDoer::new(config(&mock)).with_tools(Arc::new(Scripted(Mutex::default()))));
    let row = b.run("gpt", "build it");
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}", row.state);
    let shells = b.events("run.shell");
    assert_eq!((shells[0].data["cmd"].clone(), shells[0].data["exit"].clone(), shells[0].data["out"].clone()), (json!("make"), json!(3), json!("boom")));
}

/// A model's call is said to the screen as it is written, with the tokens so far, and none of it is written to the Log.
#[test]
fn a_models_call_and_tokens_are_flashed_as_they_come_and_never_written() {
    let mock = Mock::start(|_, n| if n == 0 { calls(&[("c1", "echo", "{\"a\":1}")]) } else { answer("ran.") });
    let tools = Arc::new(Scripted(Mutex::default()));
    let b = bench(ModelDoer::new(config(&mock)).with_tools(tools));
    let sub = b.log.subscribe("run.*", 200);
    let row = b.run("gpt", "do it");
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}\n{}", row.state, b.dump());
    let mut flashes = Vec::new();
    while let Some(e) = sub.try_recv() {
        if e.seq == 0 && (e.kind == "run.open" || e.kind == "run.tokens") {
            flashes.push((e.kind.clone(), e.data.clone()));
        }
    }
    assert!(flashes.contains(&("run.open".to_string(), json!({ "id": "0", "name": "echo", "target": "", "bytes": 7 }))), "{flashes:?}");
    assert!(flashes.contains(&("run.tokens".to_string(), json!({ "out": 20 }))), "{flashes:?}");
    assert!(b.events("run.open").is_empty() && b.events("run.tokens").is_empty());
}

#[test]
fn a_task_rides_over_a_drop_before_the_status_line_and_says_so() {
    let mock = Mock::start(|_, n| if n == 0 { Say::Drop } else { answer("Four.") });
    let b = bench(ModelDoer::new(config(&mock)).riding(vec![std::time::Duration::from_millis(10); 2]));
    let row = b.run("gpt", "what is 2+2?");
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}\n{}", row.state, b.dump());
    assert_eq!(row.answer.as_deref(), Some("Four."));
    assert_eq!(mock.seen().len(), 2);
    let notes = b.events("run.notice");
    assert!(notes.len() == 1 && notes[0].data.to_string().contains("asking again"), "{notes:?}");
}

#[test]
fn a_task_whose_stream_broke_with_a_call_begun_fails_without_a_second_ask() {
    let begun = delta(json!({ "tool_calls": [{ "index": 0, "id": "c1", "function": { "name": "echo", "arguments": "{\"a" } }] }));
    let mock = Mock::start(move |_, _| Say::Cut(vec![delta(json!({ "content": "par" })), begun.clone()]));
    let b = bench(ModelDoer::new(config(&mock)).riding(vec![std::time::Duration::from_millis(10); 2]));
    let row = b.run("gpt", "what is 2+2?");
    assert!(!matches!(row.state, State::Done { ok: true }), "{:?}", row.state);
    assert_eq!(mock.seen().len(), 1);
    assert!(b.events("run.notice").is_empty());
}

#[test]
fn a_task_rides_over_a_stream_that_broke_and_the_words_it_had_streamed_are_not_said_twice() {
    // the first try breaks after "par"; the second says "Let me look" and calls a tool; the third answers
    let mock = Mock::start(|_, n| match n {
        0 => Say::Cut(vec![delta(json!({ "content": "par" }))]),
        1 => Say::Sse(vec![delta(json!({ "content": "Let me look" })), delta(json!({ "tool_calls": [{ "index": 0, "id": "c1", "function": { "name": "echo", "arguments": "{\"a\":1}" } }] })), finish("tool_calls"), usage(60, 20, 0)]),
        _ => answer("Done."),
    });
    let tools = Arc::new(Scripted(Mutex::default()));
    let b = bench(ModelDoer::new(config(&mock)).with_tools(tools.clone()).riding(vec![std::time::Duration::from_millis(10); 2]));
    let row = b.run("gpt", "look");
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}\n{}", row.state, b.dump());
    assert_eq!(row.answer.as_deref(), Some("Done."));
    assert_eq!(mock.seen().len(), 3);
    assert_eq!(mock.seen()[0].body, mock.seen()[1].body, "the same request, asked again");
    assert_eq!(*tools.0.lock().unwrap(), ["echo {\"a\":1}"], "the tool ran once");
    let said: Vec<String> = b.events("run.message").iter().map(|e| e.data["text"].as_str().unwrap_or_default().to_string()).collect();
    assert_eq!(said, ["Let me look", "Done."], "the narration is the second try's words alone: {said:?}");
    let notes = b.events("run.notice");
    assert!(notes.len() == 1 && notes[0].data.to_string().contains("stream broke"), "{notes:?}");
}

#[test]
fn a_tool_list_leaves_out_what_the_model_is_offered_and_a_call_to_a_tool_it_was_not_offered_is_refused() {
    // the model asks for `write`, which the list denies: it was not offered, and the call does not run
    let mock = Mock::start(|_, n| if n == 0 { calls(&[("c1", "write", "{}"), ("c2", "echo", "{}")]) } else { answer("Done.") });
    let tools = Arc::new(Scripted(Mutex::default()));
    let b = bench(ModelDoer::new(config(&mock)).with_tools(tools.clone()));
    let mut spec = TaskSpec::new("do it", "gpt");
    spec.input = json!({ "tools": { "deny": ["write"] } });
    let row = b.run_spec(spec);
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}\n{}", row.state, b.dump());
    let offered: Vec<String> = mock.seen()[0].json()["tools"].as_array().unwrap().iter().map(|t| t["function"]["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(offered, ["echo"]);
    assert_eq!(*tools.0.lock().unwrap(), ["echo {}"], "the denied call was not run");
    let results = b.events("run.tool_result");
    assert!(results.iter().any(|e| e.data.to_string().contains("there is no tool `write` for this task")), "{results:?}");
    // an allow list offers only what it names; with no list both are offered, as before
    for (input, want) in [(json!({ "tools": { "allow": ["write"] } }), vec!["write"]), (json!(null), vec!["echo", "write"])] {
        let mock = Mock::start(|_, _| answer("Done."));
        let b = bench(ModelDoer::new(config(&mock)).with_tools(Arc::new(Scripted(Mutex::default()))));
        let mut spec = TaskSpec::new("do it", "gpt");
        spec.input = input;
        b.run_spec(spec);
        let offered: Vec<String> = mock.seen()[0].json()["tools"].as_array().unwrap().iter().map(|t| t["function"]["name"].as_str().unwrap().to_string()).collect();
        assert_eq!(offered, want);
    }
}
