//! The native tools, reasoning and empty-reply rules against a gateway on the loopback: the `openai` and `anthropic` dialects through the real client, the real
//! stream and the real request, with a script for the gateway to follow.

use crate::mock::{Mock, Say, Seen};
use j4n_connect::model::anthropic::Anthropic;
use j4n_connect::model::openai::Openai;
use j4n_connect::model::{Call, CUT_OFF, Client, Delta, Fail, Finish, LADDER, Msg, NUDGE, Req, THINK_BACK, Think, Tool, Wire, below, refuses_number, step, thought_back};
use j4n_connect::net::Net;
use serde_json::{Value, json};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn client(wire: Box<dyn Wire>, mock: &Mock) -> Client {
    let base = if wire.url("x").contains("/v1/messages") { mock.url() } else { format!("{}/v1", mock.url()) };
    Client::new(wire, Net::new().with_env(|_| None), &base, "k-secret")
}

fn openai(mock: &Mock) -> Client {
    client(Box::new(Openai), mock)
}

fn anthropic(mock: &Mock) -> Client {
    client(Box::new(Anthropic), mock)
}

fn frame(v: Value) -> String {
    v.to_string()
}

fn delta(d: Value) -> String {
    frame(json!({ "choices": [{ "delta": d }] }))
}

fn finish(reason: &str) -> String {
    frame(json!({ "choices": [{ "delta": {}, "finish_reason": reason }] }))
}

fn text_reply(text: &str) -> Say {
    Say::Sse(vec![delta(json!({ "content": text })), finish("stop"), "[DONE]".into()])
}

fn no() -> AtomicBool {
    AtomicBool::new(false)
}

fn ask(c: &Client, msgs: &[Msg]) -> Result<j4n_connect::model::Reply, Fail> {
    c.send(&Req::new("m", msgs), &mut |_| {}, &no())
}

fn user(t: &str) -> Msg {
    Msg::User(t.into())
}

fn max_of(s: &Seen) -> u64 {
    s.json()["max_tokens"].as_u64().unwrap()
}

// ---- openai: the stream ----

#[test]
fn a_streamed_answer_arrives_whole_and_in_pieces_with_what_it_cost() {
    let mock = Mock::start(|_, _| {
        Say::Sse(vec![
            frame(json!({ "model": "vendor/m-1", "provider": "DeepInfra", "choices": [{ "delta": { "role": "assistant" } }] })),
            delta(json!({ "content": "Hel" })),
            delta(json!({ "content": "lo" })),
            finish("stop"),
            frame(json!({ "choices": [], "usage": { "prompt_tokens": 120, "completion_tokens": 7, "prompt_tokens_details": { "cached_tokens": 100, "cache_write_tokens": 5 }, "cost": 0.0003 } })),
            "[DONE]".into(),
        ])
    });
    let mut heard = Vec::new();
    let r = openai(&mock).send(&Req::new("m", &[user("hi")]), &mut |d| heard.push(d), &no()).unwrap();
    assert_eq!(r.text, "Hello");
    assert_eq!(heard, [Delta::Text("Hel".into()), Delta::Text("lo".into()), Delta::Tokens(7)]);
    assert_eq!(r.finish, Some(Finish::Stop));
    assert_eq!(r.served.as_deref(), Some("DeepInfra"));
    assert_eq!(r.model.as_deref(), Some("vendor/m-1"));
    assert_eq!((r.usage.input, r.usage.output, r.usage.cached, r.usage.wrote), (120, 7, 100, 5));
    assert_eq!(r.usage.usd, Some(0.0003));
    assert!(r.ttft_ms.is_some() && r.calls.is_empty() && r.think.is_empty());
    let seen = &mock.seen()[0];
    assert_eq!((seen.method.as_str(), seen.path.as_str()), ("POST", "/v1/chat/completions"));
    assert_eq!(seen.headers.get("authorization").map(String::as_str), Some("Bearer k-secret"));
    let b = seen.json();
    assert_eq!((b["model"].clone(), b["stream"].clone(), b["max_tokens"].clone()), (json!("m"), json!(true), json!(32768)));
    assert_eq!(b["messages"], json!([{ "role": "user", "content": "hi" }]));
    assert!(b.get("tools").is_none() && b.get("provider").is_none() && b.get("session_id").is_none() && b.get("reasoning").is_none(), "{b}");
}

#[test]
fn the_cache_is_read_from_whichever_field_a_gateway_uses() {
    for (usage, cached) in [
        (json!({ "prompt_tokens": 50, "completion_tokens": 1, "prompt_cache_hit_tokens": 40 }), 40),
        (json!({ "prompt_tokens": 50, "completion_tokens": 1, "cache_read_input_tokens": 30 }), 30),
        (json!({ "prompt_tokens": 50, "completion_tokens": 1 }), 0),
    ] {
        let mock = Mock::start(move |_, _| Say::Sse(vec![delta(json!({ "content": "x" })), finish("stop"), frame(json!({ "choices": [], "usage": usage.clone() }))]));
        assert_eq!(ask(&openai(&mock), &[user("a")]).unwrap().usage.cached, cached);
    }
}

#[test]
fn reasoning_is_collected_from_either_spelling_apart_from_the_answer_and_never_twice() {
    let mock = Mock::start(|_, _| {
        Say::Sse(vec![
            delta(json!({ "reasoning": "First " })),
            // the structured copy of the same words adds nothing
            delta(json!({ "reasoning": "look, ", "reasoning_details": [{ "type": "reasoning.text", "text": "First look, " }] })),
            delta(json!({ "reasoning_details": [{ "text": "no" }] })),
            delta(json!({ "reasoning_content": "then act." })),
            delta(json!({ "content": "Done." })),
            finish("stop"),
        ])
    });
    let mut heard = Vec::new();
    let r = openai(&mock).send(&Req::new("m", &[user("a")]), &mut |d| heard.push(d), &no()).unwrap();
    assert_eq!(r.text, "Done.");
    assert_eq!(r.think, [Think { text: "First look, then act.".into(), sig: String::new() }]);
    assert_eq!(heard.iter().filter(|d| matches!(d, Delta::Reasoning(_))).count(), 3);
    assert!(heard.iter().all(|d| match d {
        Delta::Text(t) => t == "Done.",
        Delta::Reasoning(t) => !t.contains("Done"),
        Delta::Call { .. } | Delta::Tokens(_) | Delta::Restart => false,
    }));
}

#[test]
fn every_call_of_a_reply_comes_back_in_order_with_its_fragments_put_together_l4() {
    let mock = Mock::start(|_, _| {
        let call = |index: u64, id: Option<&str>, name: Option<&str>, args: &str| {
            let mut f = json!({ "index": index, "function": { "arguments": args } });
            if let Some(id) = id {
                f["id"] = json!(id);
            }
            if let Some(n) = name {
                f["function"]["name"] = json!(n);
            }
            f
        };
        Say::Sse(vec![
            delta(json!({ "tool_calls": [call(0, Some("c0"), Some("read"), "{\"pa")] })),
            delta(json!({ "tool_calls": [call(1, Some("c1"), Some("grep"), "{\"pattern\":")] })),
            // the second call is named again in a later fragment, and the third has no id at all
            delta(json!({ "tool_calls": [call(0, None, Some("read"), "th\":\"a.rs\"}"), call(1, None, Some("grep"), "\"x\"}"), call(2, None, Some("ls"), "")] })),
            finish("tool_calls"),
        ])
    });
    let r = ask(&openai(&mock), &[user("a")]).unwrap();
    assert_eq!(r.finish, Some(Finish::Calls));
    let got: Vec<(&str, &str, Value, bool)> = r.calls.iter().map(|c| (c.id.as_str(), c.name.as_str(), c.args.clone(), c.bad.is_some())).collect();
    assert_eq!(got, [("c0", "read", json!({ "path": "a.rs" }), false), ("c1", "grep", json!({ "pattern": "x" }), false), ("call_2", "ls", json!({}), false)]);
    assert!(!r.is_empty(), "a reply with calls and no words is not empty");
}

#[test]
fn arguments_that_are_not_json_are_refused_as_that_and_a_string_of_json_is_decoded_l6() {
    let mock = Mock::start(|_, _| {
        let call = |i: u64, name: &str, args: &str| json!({ "index": i, "id": format!("c{i}"), "function": { "name": name, "arguments": args } });
        Say::Sse(vec![
            delta(json!({ "tool_calls": [call(0, "write", "{\"path\":\"a\",\"content\":\"long te")] })),
            delta(json!({ "tool_calls": [call(1, "write", &json!(json!({"path": "b"}).to_string()).to_string())] })),
            delta(json!({ "tool_calls": [call(2, "write", "[1,2]")] })),
            finish("tool_calls"),
        ])
    });
    let r = ask(&openai(&mock), &[user("a")]).unwrap();
    assert_eq!(r.calls[0].bad.as_deref(), Some(CUT_OFF));
    assert!(CUT_OFF.contains("not valid JSON") && CUT_OFF.contains("cut off"));
    assert_eq!((r.calls[1].args.clone(), r.calls[1].bad.clone()), (json!({ "path": "b" }), None));
    assert!(r.calls[2].bad.as_deref().unwrap().contains("not a JSON object"));
    // a reply that ran out of room names that as the reason
    let cut = Mock::start(|_, _| Say::Sse(vec![delta(json!({ "tool_calls": [{ "index": 0, "id": "c", "function": { "name": "write", "arguments": "{\"content\":\"abc" } }] })), finish("length")]));
    let r = ask(&openai(&cut), &[user("a")]).unwrap();
    assert_eq!(r.finish, Some(Finish::Length));
    assert!(r.calls[0].bad.as_deref().unwrap().starts_with("the reply ended before the call was complete"), "{:?}", r.calls[0].bad);
}

// ---- openai: the request ----

fn call(id: &str, name: &str, args: Value) -> Call {
    Call { id: id.into(), name: name.into(), args, bad: None }
}

#[test]
fn a_conversation_with_tools_goes_out_as_native_messages_and_the_functions_on_offer_l4() {
    let mock = Mock::start(|_, _| text_reply("ok"));
    let msgs = [
        user("fix it"),
        Msg::Assistant { text: String::new(), think: vec![Think { text: "my plan".into(), sig: String::new() }], calls: vec![call("c1", "read", json!({ "path": "a" })), call("c2", "ls", json!({}))] },
        Msg::Result { call: "c1".into(), name: "read".into(), text: "file".into(), error: false },
        Msg::Result { call: "c2".into(), name: "ls".into(), text: "a b".into(), error: false },
        Msg::Assistant { text: "done".into(), think: vec![Think { text: "old thoughts".into(), sig: String::new() }], calls: vec![] },
    ];
    let tools = [Tool { name: "read".into(), doc: "read a file".into(), schema: json!({ "type": "object", "properties": { "path": { "type": "string" } } }) }];
    let req = Req { system: "be brief", tools: &tools, effort: Some("low"), host: Some("Cerebras"), session: Some("s-9"), ..Req::new("m", &msgs) };
    openai(&mock).send(&req, &mut |_| {}, &no()).unwrap();
    let b = mock.seen()[0].json();
    assert_eq!(b["messages"][0], json!({ "role": "system", "content": "be brief" }));
    assert_eq!(b["messages"][2]["content"], Value::Null);
    assert_eq!(b["messages"][2]["tool_calls"][0], json!({ "id": "c1", "type": "function", "function": { "name": "read", "arguments": "{\"path\":\"a\"}" } }));
    assert_eq!(b["messages"][2]["tool_calls"][1]["id"], "c2");
    assert_eq!(b["messages"][2]["reasoning"], "my plan", "the reasoning goes back with the call it led to");
    assert_eq!(b["messages"][3], json!({ "role": "tool", "tool_call_id": "c1", "content": "file" }));
    assert_eq!(b["messages"][4]["tool_call_id"], "c2");
    assert_eq!(b["messages"][5], json!({ "role": "assistant", "content": "done" }), "an answer does not carry its reasoning");
    assert_eq!(b["tools"][0], json!({ "type": "function", "function": { "name": "read", "description": "read a file", "parameters": tools[0].schema } }));
    assert_eq!(b["tool_choice"], "auto");
    assert_eq!(b["reasoning"], json!({ "effort": "low" }));
    assert_eq!(b["provider"], json!({ "order": ["Cerebras"], "allow_fallbacks": false }));
    assert_eq!(b["session_id"], "s-9");
}

#[test]
fn only_the_latest_calls_carry_their_reasoning_back_l5() {
    let mut msgs = vec![user("go")];
    for i in 0..THINK_BACK + 3 {
        msgs.push(Msg::Assistant { text: String::new(), think: vec![Think { text: format!("t{i}"), sig: "s".into() }], calls: vec![call(&format!("c{i}"), "ls", json!({}))] });
        msgs.push(Msg::Result { call: format!("c{i}"), name: "ls".into(), text: "x".into(), error: false });
    }
    let kept: Vec<usize> = thought_back(&msgs).iter().enumerate().filter(|(_, m)| matches!(m, Msg::Assistant { think, .. } if !think.is_empty())).map(|(i, _)| i).collect();
    assert_eq!(kept.len(), THINK_BACK);
    assert_eq!(kept[0], 1 + 2 * 3, "the first three lost theirs");
    // the conversation itself is untouched
    assert!(matches!(&msgs[1], Msg::Assistant { think, .. } if think.len() == 1));
    let mock = Mock::start(|_, _| text_reply("ok"));
    openai(&mock).send(&Req::new("m", &msgs), &mut |_| {}, &no()).unwrap();
    let b = mock.seen()[0].json();
    let with: Vec<&str> = b["messages"].as_array().unwrap().iter().filter_map(|m| m["reasoning"].as_str()).collect();
    assert_eq!(with.len(), THINK_BACK);
    assert_eq!(with[0], "t3");
    assert_eq!(*with.last().unwrap(), format!("t{}", THINK_BACK + 2));
}

#[test]
fn the_messages_before_the_latest_turn_are_sent_unchanged_from_one_request_to_the_next_so_the_cache_holds() {
    // 15 tool-calling turns, a request after each: request n and request n+1 agree on every message before the last two
    let mock = Mock::start(|_, _| text_reply("ok"));
    let client = openai(&mock);
    let mut msgs = vec![user("go")];
    let mut sizes = Vec::new();
    for i in 0..15 {
        msgs.push(Msg::Assistant { text: String::new(), think: vec![Think { text: format!("t{i}"), sig: format!("s{i}") }], calls: vec![call(&format!("c{i}"), "ls", json!({}))] });
        msgs.push(Msg::Result { call: format!("c{i}"), name: "ls".into(), text: "x".into(), error: false });
        client.send(&Req::new("m", &msgs), &mut |_| {}, &no()).unwrap();
        sizes.push(msgs.len());
    }
    let seen = mock.seen();
    assert_eq!(seen.len(), 15);
    for n in 0..14 {
        let (a, b) = (seen[n].json(), seen[n + 1].json());
        let cut = sizes[n] - 2;
        assert_eq!(a["messages"].as_array().unwrap()[..cut], b["messages"].as_array().unwrap()[..cut], "request {} and {} differ before the latest turn", n + 1, n + 2);
    }
    // the latest tool-calling message keeps its reasoning and the proof that goes with it
    let last = thought_back(&msgs);
    assert!(matches!(&last[last.len() - 2], Msg::Assistant { think, .. } if think == &vec![Think { text: "t14".into(), sig: "s14".into() }]));
    assert!(matches!(&last[last.len() - 4], Msg::Assistant { think, .. } if think.is_empty()), "the one before it does not");
}

// ---- room to write, empty replies ----

fn too_big(n: u64) -> Say {
    Say::Json(400, json!({ "error": { "message": format!("max_tokens is too large: {n}. This model supports at most 8192 completion tokens") } }))
}

#[test]
fn a_gateway_that_refuses_the_number_is_asked_again_with_a_smaller_one_l6() {
    let mock = Mock::start(|s, _| if max_of(s) > 4096 { too_big(max_of(s)) } else { text_reply("fits") });
    let r = ask(&openai(&mock), &[user("a")]).unwrap();
    assert_eq!(r.text, "fits");
    assert_eq!(mock.seen().iter().map(max_of).collect::<Vec<_>>(), [32768, 16384, 8192, 4096]);
    // when even the last is refused, that is the failure
    let none = Mock::start(|s, _| too_big(max_of(s)));
    let e = ask(&openai(&none), &[user("a")]).unwrap_err();
    assert!(matches!(e, Fail::Status { code: 400, .. }), "{e:?}");
    assert_eq!(none.seen().len(), 4);
    // a request that starts lower starts the ladder lower
    let low = Mock::start(|s, _| if max_of(s) > 4096 { too_big(1) } else { text_reply("x") });
    openai(&low).send(&Req::new("m", &[user("a")]).max(8192), &mut |_| {}, &no()).unwrap();
    assert_eq!(low.seen().iter().map(max_of).collect::<Vec<_>>(), [8192, 4096]);
    // another 400 is not about the number, and is not retried
    let other = Mock::start(|_, _| Say::Json(400, json!({ "error": { "message": "unknown model" } })));
    assert!(ask(&openai(&other), &[user("a")]).is_err());
    assert_eq!(other.seen().len(), 1);
    assert_eq!(below(32768), Some(16384));
    assert_eq!(below(4096), None);
    assert_eq!(LADDER, [32768, 16384, 8192, 4096]);
    assert!(refuses_number(400, "{\"error\":{\"message\":\"max_completion_tokens too large\"}}") && !refuses_number(500, "max_tokens") && !refuses_number(400, "bad key"));
    for code in [400, 413, 422] {
        for word in ["max_tokens", "max_completion_tokens", "max_output_tokens", "maximum output", "output tokens", "output limit", "too many tokens"] {
            assert!(refuses_number(code, &format!("the {word} is over")), "{code} {word}");
        }
    }
    assert!(!refuses_number(401, "max_tokens") && !refuses_number(429, "max_tokens") && !refuses_number(404, "max_tokens"));
}

#[test]
fn the_first_empty_reply_is_asked_again_with_a_line_saying_so_l7() {
    let mock = Mock::start(|_, n| if n == 0 { Say::Sse(vec![finish("stop")]) } else { text_reply("here it is") });
    let mut convo = vec![user("a")];
    let req = Req::new("m", &[]);
    let r = step(&openai(&mock), &Req { msgs: &convo.clone(), ..req }, &mut convo, &mut |_| {}, &no()).unwrap();
    assert_eq!(r.text, "here it is");
    assert_eq!(convo, [user("a"), user(NUDGE)], "what the model was told is kept");
    let second = mock.seen()[1].json();
    assert_eq!(second["messages"].as_array().unwrap().last().unwrap(), &json!({ "role": "user", "content": NUDGE }));
    assert_eq!(mock.seen().len(), 2);
}

#[test]
fn a_second_empty_reply_stops_the_task_with_the_tokens_it_cost_l7() {
    let mock = Mock::start(|_, _| Say::Sse(vec![finish("stop"), frame(json!({ "choices": [], "usage": { "prompt_tokens": 9, "completion_tokens": 40 } }))]));
    let mut convo = vec![user("a")];
    let e = step(&openai(&mock), &Req::new("m", &convo.clone()), &mut convo, &mut |_| {}, &no()).unwrap_err();
    assert_eq!(e, Fail::Empty { tokens: 80 });
    assert!(e.words(0).contains("no content twice") && e.words(0).contains("80"));
    assert_eq!(convo, [user("a")], "nothing is kept of a retry that failed");
    assert_eq!(mock.seen().len(), 2);
}

#[test]
fn an_empty_reply_that_hit_its_limit_is_asked_again_with_room_l7() {
    let mock = Mock::start(|_, n| if n == 0 { Say::Sse(vec![delta(json!({ "reasoning": "thinking and thinking" })), finish("length")]) } else { text_reply("ok") });
    let mut convo = vec![user("a")];
    step(&openai(&mock), &Req::new("m", &convo.clone()).max(4096), &mut convo, &mut |_| {}, &no()).unwrap();
    assert_eq!(mock.seen().iter().map(max_of).collect::<Vec<_>>(), [4096, 16384]);
    // a larger limit is never made smaller, and an empty reply that did not hit its limit asks again with the same
    let big = Mock::start(|_, n| if n == 0 { Say::Sse(vec![finish("length")]) } else { text_reply("ok") });
    step(&openai(&big), &Req::new("m", &convo.clone()), &mut convo, &mut |_| {}, &no()).unwrap();
    assert_eq!(big.seen().iter().map(max_of).collect::<Vec<_>>(), [32768, 32768]);
    let same = Mock::start(|_, n| if n == 0 { Say::Sse(vec![finish("stop")]) } else { text_reply("ok") });
    step(&openai(&same), &Req::new("m", &convo.clone()).max(4096), &mut convo, &mut |_| {}, &no()).unwrap();
    assert_eq!(same.seen().iter().map(max_of).collect::<Vec<_>>(), [4096, 4096]);
}

#[test]
fn calls_and_no_words_is_a_reply_and_words_and_no_calls_is_too() {
    let mock = Mock::start(|_, _| Say::Sse(vec![delta(json!({ "tool_calls": [{ "index": 0, "id": "c", "function": { "name": "ls", "arguments": "{}" } }] })), finish("tool_calls")]));
    let mut convo = vec![user("a")];
    let r = step(&openai(&mock), &Req::new("m", &convo.clone()), &mut convo, &mut |_| {}, &no()).unwrap();
    assert_eq!(r.calls.len(), 1);
    assert_eq!(mock.seen().len(), 1);
}

// ---- failures ----

#[test]
fn a_status_a_dead_gateway_and_a_failed_stream_are_told_in_the_words_the_failure_table_reads() {
    let limited = Mock::start(|_, _| Say::Text(429, "{\"error\":{\"message\":\"slow down please\"}}".into()));
    let e = ask(&openai(&limited), &[user("a")]).unwrap_err();
    let words = e.words(1_700_000_000_000);
    assert!(matches!(e, Fail::Status { code: 429, .. }) && words.starts_with("429 rate limit") && words.contains("slow down please"), "{words}");
    assert!(e.transient());
    assert!(!Fail::Status { code: 401, body: String::new(), retry_after: None }.transient());
    assert!(Fail::Status { code: 529, body: "busy".into(), retry_after: Some(7) }.words(2_000_000).contains("(resets at 2007)"));
    // a failure in the middle of a stream, as OpenRouter writes one
    let mid = Mock::start(|_, _| Say::Sse(vec![delta(json!({ "content": "par" })), frame(json!({ "error": { "code": 429, "message": "provider limit" } }))]));
    assert!(matches!(ask(&openai(&mid), &[user("a")]), Err(Fail::Status { code: 429, .. })));
    let broke = Mock::start(|_, _| Say::Sse(vec![frame(json!({ "error": { "message": "upstream died" } }))]));
    assert_eq!(ask(&openai(&broke), &[user("a")]), Err(Fail::Stream("upstream died".into())));
    // nobody there
    let gone = Mock::start(|_, _| Say::Drop);
    assert!(matches!(ask(&openai(&gone), &[user("a")]), Err(Fail::Net(_))));
    assert!(Fail::Net("x".into()).words(0).starts_with("could not reach the model"));
}

#[test]
fn a_stop_between_frames_ends_the_read() {
    let mock = Mock::start(|_, _| Say::Sse(vec![delta(json!({ "content": "a" })), delta(json!({ "content": "b" }))]));
    let stop = AtomicBool::new(true);
    assert_eq!(openai(&mock).send(&Req::new("m", &[user("a")]), &mut |_| {}, &stop), Err(Fail::Cancelled));
}

// ---- anthropic ----

fn ev(v: Value) -> String {
    v.to_string()
}

#[test]
fn the_messages_stream_gives_text_thinking_with_its_signature_and_every_call() {
    let mock = Mock::start(|_, _| {
        Say::Sse(vec![
            ev(json!({ "type": "message_start", "message": { "model": "claude-x", "usage": { "input_tokens": 10, "cache_read_input_tokens": 200, "cache_creation_input_tokens": 30, "output_tokens": 1 } } })),
            ev(json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "thinking", "thinking": "" } })),
            ev(json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "thinking_delta", "thinking": "hm, " } })),
            ev(json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "thinking_delta", "thinking": "ok" } })),
            ev(json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "signature_delta", "signature": "SIG" } })),
            ev(json!({ "type": "content_block_start", "index": 1, "content_block": { "type": "text", "text": "" } })),
            ev(json!({ "type": "content_block_delta", "index": 1, "delta": { "type": "text_delta", "text": "Looking." } })),
            ev(json!({ "type": "content_block_start", "index": 2, "content_block": { "type": "tool_use", "id": "tu1", "name": "read", "input": {} } })),
            ev(json!({ "type": "content_block_delta", "index": 2, "delta": { "type": "input_json_delta", "partial_json": "{\"path\":" } })),
            ev(json!({ "type": "content_block_delta", "index": 2, "delta": { "type": "input_json_delta", "partial_json": "\"a\"}" } })),
            ev(json!({ "type": "content_block_start", "index": 3, "content_block": { "type": "tool_use", "id": "tu2", "name": "ls", "input": {} } })),
            ev(json!({ "type": "message_delta", "delta": { "stop_reason": "tool_use" }, "usage": { "output_tokens": 55 } })),
            ev(json!({ "type": "message_stop" })),
        ])
    });
    let mut heard = Vec::new();
    let r = anthropic(&mock).send(&Req::new("claude-x", &[user("a")]), &mut |d| heard.push(d), &no()).unwrap();
    assert_eq!(r.text, "Looking.");
    assert_eq!(r.think, [Think { text: "hm, ok".into(), sig: "SIG".into() }]);
    assert_eq!(r.calls, [call("tu1", "read", json!({ "path": "a" })), call("tu2", "ls", json!({}))]);
    assert_eq!(r.finish, Some(Finish::Calls));
    assert_eq!((r.usage.input, r.usage.output, r.usage.cached, r.usage.wrote), (240, 55, 200, 30));
    assert_eq!(r.model.as_deref(), Some("claude-x"));
    assert_eq!(heard.iter().filter(|d| matches!(d, Delta::Text(_) | Delta::Reasoning(_))).count(), 3);
    let seen = &mock.seen()[0];
    assert_eq!(seen.path, "/v1/messages");
    assert_eq!(seen.headers.get("x-api-key").map(String::as_str), Some("k-secret"));
    assert_eq!(seen.headers.get("anthropic-version").map(String::as_str), Some("2023-06-01"));
}

#[test]
fn the_messages_request_keeps_the_system_apart_and_the_results_of_a_reply_together() {
    let mock = Mock::start(|_, _| Say::Sse(vec![ev(json!({ "type": "message_delta", "delta": { "stop_reason": "end_turn" } }))]));
    let msgs = [
        user("fix it"),
        Msg::Assistant { text: "on it".into(), think: vec![Think { text: "plan".into(), sig: "SIG".into() }, Think { text: "unsigned".into(), sig: String::new() }], calls: vec![call("tu1", "read", json!({ "path": "a" })), call("tu2", "ls", json!({}))] },
        Msg::Result { call: "tu1".into(), name: "read".into(), text: "file".into(), error: false },
        Msg::Result { call: "tu2".into(), name: "ls".into(), text: "refused".into(), error: true },
        user("and then?"),
    ];
    let tools = [Tool { name: "read".into(), doc: "read a file".into(), schema: json!({ "type": "object" }) }];
    let req = Req { system: "be brief", tools: &tools, effort: Some("high"), max: 32768, ..Req::new("claude-x", &msgs) };
    anthropic(&mock).send(&req, &mut |_| {}, &no()).unwrap();
    let b = mock.seen()[0].json();
    assert_eq!(b["system"], "be brief");
    assert_eq!(b["messages"].as_array().unwrap().len(), 3, "{b}");
    assert_eq!(b["messages"][0], json!({ "role": "user", "content": [{ "type": "text", "text": "fix it" }] }));
    assert_eq!(
        b["messages"][1]["content"],
        json!([
            { "type": "thinking", "thinking": "plan", "signature": "SIG" },
            { "type": "text", "text": "on it" },
            { "type": "tool_use", "id": "tu1", "name": "read", "input": { "path": "a" } },
            { "type": "tool_use", "id": "tu2", "name": "ls", "input": {} },
        ])
    );
    assert_eq!(
        b["messages"][2]["content"],
        json!([
            { "type": "tool_result", "tool_use_id": "tu1", "content": "file" },
            { "type": "tool_result", "tool_use_id": "tu2", "content": "refused", "is_error": true },
            { "type": "text", "text": "and then?" },
        ])
    );
    assert_eq!(b["tools"][0], json!({ "name": "read", "description": "read a file", "input_schema": { "type": "object" } }));
    assert_eq!(b["thinking"], json!({ "type": "enabled", "budget_tokens": 16384 }));
    assert_eq!(b["max_tokens"], 32768);
}

#[test]
fn the_messages_stream_tells_overload_a_limit_and_a_cut_call() {
    let busy = Mock::start(|_, _| Say::Sse(vec![ev(json!({ "type": "error", "error": { "type": "overloaded_error", "message": "Overloaded" } }))]));
    let e = ask(&anthropic(&busy), &[user("a")]).unwrap_err();
    assert!(matches!(e, Fail::Status { code: 529, .. }) && e.words(0).contains("overloaded"), "{e:?}");
    let limited = Mock::start(|_, _| Say::Sse(vec![ev(json!({ "type": "error", "error": { "type": "rate_limit_error", "message": "slow" } }))]));
    assert!(matches!(ask(&anthropic(&limited), &[user("a")]), Err(Fail::Status { code: 429, .. })));
    let cut = Mock::start(|_, _| {
        Say::Sse(vec![
            ev(json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "tool_use", "id": "t", "name": "write", "input": {} } })),
            ev(json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "input_json_delta", "partial_json": "{\"content\":\"abc" } })),
            ev(json!({ "type": "message_delta", "delta": { "stop_reason": "max_tokens" }, "usage": { "output_tokens": 9 } })),
        ])
    });
    let r = ask(&anthropic(&cut), &[user("a")]).unwrap();
    assert_eq!(r.finish, Some(Finish::Length));
    assert!(r.calls[0].bad.as_deref().unwrap().starts_with("the reply ended before the call was complete"));
    // the ladder and the empty-reply rule hold for this dialect too
    let ladder = Mock::start(|s, _| {
        if max_of(s) > 8192 {
            Say::Json(400, json!({ "error": { "message": "max_tokens: 32768 > 8192, the maximum allowed number of output tokens" } }))
        } else {
            Say::Sse(vec![ev(json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "text", "text": "" } })), ev(json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "fits" } }))])
        }
    });
    assert_eq!(ask(&anthropic(&ladder), &[user("a")]).unwrap().text, "fits");
    assert_eq!(ladder.seen().iter().map(max_of).collect::<Vec<_>>(), [32768, 16384, 8192]);
}

#[test]
fn an_error_body_is_one_short_line_of_what_the_gateway_said() {
    let long = format!("{{\"error\":{{\"message\":\"{}\"}}}}", "word ".repeat(200));
    let e = Fail::Status { code: 500, body: long, retry_after: None };
    let words = e.words(0);
    let said = words.strip_prefix("HTTP 500: ").unwrap();
    assert_eq!(said.chars().count(), 300, "{words}");
    let spread = Fail::Status { code: 500, body: "a\n\n   b\t c".into(), retry_after: None };
    assert_eq!(spread.words(0), "HTTP 500: a b c");
}

#[test]
fn room_after_an_empty_reply_that_hit_its_limit_is_four_times_but_never_past_sixteen_thousand() {
    let mock = Mock::start(|_, n| if n == 0 { Say::Sse(vec![finish("length")]) } else { text_reply("ok") });
    let mut convo = vec![user("a")];
    step(&openai(&mock), &Req::new("m", &convo.clone()).max(8192), &mut convo, &mut |_| {}, &no()).unwrap();
    assert_eq!(mock.seen().iter().map(max_of).collect::<Vec<_>>(), [8192, 16384]);
}

#[test]
fn a_fragment_that_never_names_its_function_is_not_a_call() {
    let mock = Mock::start(|_, _| {
        Say::Sse(vec![
            delta(json!({ "tool_calls": [{ "index": 0, "id": "c0", "function": { "arguments": "{}" } }, { "index": 1, "id": "c1", "function": { "name": "ls", "arguments": "{}" } }] })),
            finish("tool_calls"),
        ])
    });
    let r = ask(&openai(&mock), &[user("a")]).unwrap();
    assert_eq!(r.calls.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["ls"]);
}

/// A call shows from its first byte, in the `openai` dialect and in the `anthropic` one, and grows with its arguments.
#[test]
fn a_call_is_heard_from_its_first_fragment_with_its_path_once_that_has_come() {
    let call = |name: Option<&str>, args: &str| {
        let mut f = json!({ "index": 0, "function": { "arguments": args } });
        if let Some(n) = name {
            f["id"] = json!("c0");
            f["function"]["name"] = json!(n);
        }
        f
    };
    let mock = Mock::start(move |_, _| {
        Say::Sse(vec![
            delta(json!({ "tool_calls": [call(Some("write"), "")] })),
            delta(json!({ "tool_calls": [call(None, "{\"path\":\"chess")] })),
            delta(json!({ "tool_calls": [call(None, ".py\",\"content\":\"import")] })),
            finish("tool_calls"),
            frame(json!({ "choices": [], "usage": { "prompt_tokens": 5, "completion_tokens": 77 } })),
        ])
    });
    let mut heard = Vec::new();
    openai(&mock).send(&Req::new("m", &[user("a")]), &mut |d| heard.push(d), &no()).unwrap();
    let open = |target: &str, bytes: u64| Delta::Call { id: "0".into(), name: "write".into(), target: target.into(), bytes };
    assert_eq!(heard, [open("", 0), open("", 14), open("chess.py", 36), Delta::Tokens(77)]);
}

#[test]
fn a_tool_use_block_is_heard_when_it_starts_and_grows() {
    let mock = Mock::start(|_, _| {
        Say::Sse(vec![
            ev(json!({ "type": "message_start", "message": { "model": "claude-x", "usage": { "input_tokens": 10, "output_tokens": 1 } } })),
            ev(json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "text", "text": "" } })),
            ev(json!({ "type": "content_block_start", "index": 1, "content_block": { "type": "tool_use", "id": "tu1", "name": "write", "input": {} } })),
            ev(json!({ "type": "content_block_delta", "index": 1, "delta": { "type": "input_json_delta", "partial_json": "{\"file_path\":\"/a/b.rs\",\"con" } })),
            ev(json!({ "type": "message_delta", "delta": { "stop_reason": "tool_use" }, "usage": { "output_tokens": 55 } })),
        ])
    });
    let mut heard = Vec::new();
    anthropic(&mock).send(&Req::new("claude-x", &[user("a")]), &mut |d| heard.push(d), &no()).unwrap();
    let open = |target: &str, bytes: u64| Delta::Call { id: "1".into(), name: "write".into(), target: target.into(), bytes };
    assert_eq!(heard, [open("", 0), open("/a/b.rs", 27), Delta::Tokens(55)]);
}

// ---- a drop before any byte is ridden over ----

fn riding(mock: &Mock, said: &Arc<Mutex<Vec<String>>>) -> Client {
    let said = said.clone();
    openai(mock).riding(&[Duration::from_millis(10), Duration::from_millis(10)], move |w| said.lock().unwrap().push(w))
}

#[test]
fn a_call_that_was_dropped_before_the_status_line_is_asked_again_and_each_try_is_said() {
    let mock = Mock::start(|_, n| if n == 0 { Say::Drop } else { text_reply("Four.") });
    let said = Arc::new(Mutex::new(Vec::new()));
    let r = ask(&riding(&mock, &said), &[user("2+2?")]).unwrap();
    assert_eq!(r.text, "Four.");
    assert_eq!(mock.seen().len(), 2, "the same request came twice");
    assert_eq!(mock.seen()[0].body, mock.seen()[1].body);
    let said = said.lock().unwrap();
    assert!(said.len() == 1 && said[0].contains("could not be reached") && said[0].contains("try 1 of 2"), "{said:?}");
}

#[test]
fn only_two_tries_follow_and_then_the_call_fails_as_it_did() {
    let mock = Mock::start(|_, _| Say::Drop);
    let said = Arc::new(Mutex::new(Vec::new()));
    assert!(matches!(ask(&riding(&mock, &said), &[user("a")]), Err(Fail::Net(_))));
    assert_eq!(mock.seen().len(), 3);
    assert_eq!(said.lock().unwrap().len(), 2);
    // a client that was not asked to ride asks once
    let once = Mock::start(|_, _| Say::Drop);
    assert!(matches!(ask(&openai(&once), &[user("a")]), Err(Fail::Net(_))));
    assert_eq!(once.seen().len(), 1);
}

#[test]
fn a_stream_that_broke_with_a_call_begun_is_not_asked_again() {
    // the words before a call are kept as it begins, so a second try would say them twice
    let said = Arc::new(Mutex::new(Vec::new()));
    let begun = delta(json!({ "tool_calls": [{ "index": 0, "id": "c0", "function": { "name": "read", "arguments": "{\"pa" } }] }));
    let cut = Mock::start(move |_, _| Say::Cut(vec![delta(json!({ "content": "I will look." })), begun.clone()]));
    assert!(matches!(ask(&riding(&cut, &said), &[user("a")]), Err(Fail::Stream(_))));
    assert_eq!(cut.seen().len(), 1);
    let status = Mock::start(|_, _| Say::Text(503, "busy".into()));
    assert!(matches!(ask(&riding(&status, &said), &[user("a")]), Err(Fail::Status { code: 503, .. })));
    assert_eq!(status.seen().len(), 1);
    assert!(said.lock().unwrap().is_empty());
}

#[test]
fn a_stream_that_broke_before_the_reply_finished_is_asked_again_and_what_it_streamed_is_dropped() {
    let mock = Mock::start(|_, n| if n == 0 { Say::Cut(vec![delta(json!({ "content": "par" }))]) } else { text_reply("Four.") });
    let said = Arc::new(Mutex::new(Vec::new()));
    let mut heard = Vec::new();
    let r = riding(&mock, &said).send(&Req::new("m", &[user("2+2?")]), &mut |d| heard.push(d), &no()).unwrap();
    assert_eq!(r.text, "Four.", "the reply is the second try's, not the two put together");
    assert_eq!(mock.seen().len(), 2);
    assert_eq!(mock.seen()[0].body, mock.seen()[1].body, "the same request");
    assert_eq!(heard, [Delta::Text("par".into()), Delta::Restart, Delta::Text("Four.".into())], "the caller is told to drop what it had");
    let said = said.lock().unwrap();
    assert!(said.len() == 1 && said[0].contains("stream broke") && said[0].contains("try 1 of 2"), "{said:?}");
}

#[test]
fn a_stream_that_breaks_again_is_asked_as_often_as_the_waits_allow_and_then_fails_as_it_did() {
    let said = Arc::new(Mutex::new(Vec::new()));
    let mock = Mock::start(|_, _| Say::Cut(vec![delta(json!({ "content": "par" }))]));
    assert!(matches!(ask(&riding(&mock, &said), &[user("a")]), Err(Fail::Stream(_))));
    assert_eq!(mock.seen().len(), 3);
    assert_eq!(said.lock().unwrap().len(), 2);
    // two breaks and then an answer
    let mock = Mock::start(|_, n| if n < 2 { Say::Cut(Vec::new()) } else { text_reply("ok") });
    assert_eq!(ask(&riding(&mock, &said), &[user("a")]).unwrap().text, "ok");
    // a client that was not asked to ride asks once
    let once = Mock::start(|_, _| Say::Cut(vec![delta(json!({ "content": "par" }))]));
    assert!(matches!(ask(&openai(&once), &[user("a")]), Err(Fail::Stream(_))));
    assert_eq!(once.seen().len(), 1);
}

#[test]
fn a_stop_during_the_wait_ends_the_ride() {
    let mock = Mock::start(|_, _| Say::Drop);
    let c = openai(&mock).riding(&[Duration::from_secs(30)], |_| {});
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = stop.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        s2.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    let t = std::time::Instant::now();
    assert_eq!(c.send(&Req::new("m", &[user("a")]), &mut |_| {}, &stop), Err(Fail::Cancelled));
    assert!(t.elapsed() < Duration::from_secs(5));
}

#[test]
fn the_time_to_the_first_token_is_counted_from_the_request_not_from_the_status_line() {
    // the gateway thinks 300 ms before it says anything at all, then streams at once
    let mock = Mock::start(|_, _| Say::After(Duration::from_millis(300), Box::new(text_reply("hi"))));
    let r = openai(&mock).send(&Req::new("m", &[user("hi")]), &mut |_| {}, &no()).unwrap();
    let ttft = r.ttft_ms.expect("a token came");
    assert!(ttft >= 250, "counted from the request: {ttft} ms");
    assert!(r.ms >= ttft, "{} / {ttft}", r.ms);
    // and one that answers at once has a short time to the first token
    let slow = Mock::start(|_, _| Say::After(Duration::from_millis(0), Box::new(text_reply("hi"))));
    let quick = openai(&slow).send(&Req::new("m", &[user("hi")]), &mut |_| {}, &no()).unwrap();
    assert!(quick.ttft_ms.is_some_and(|t| t < 250), "{:?}", quick.ttft_ms);
}
