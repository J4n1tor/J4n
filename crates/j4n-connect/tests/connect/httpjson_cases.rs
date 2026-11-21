//! The dialect `http-json`: a REST endpoint said in data. The endpoint is a server on the loopback that writes down what it was
//! asked, so what is tested is the real client and the real wire; the rows are read the way a user's file is.

use crate::bench::{Bench, connectors};
use crate::mock::{Mock, Say};
use crate::world::Scratch;
use j4n_connect::command::CommandTools;
use j4n_connect::httpjson::HttpJson;
use j4n_connect::model::Call;
use j4n_connect::model::doer::Tools;
use j4n_core::place::Site;
use j4n_core::task::{State, Why};
use serde_json::json;
use std::collections::BTreeMap;

fn row(url: &str, extra: &str) -> String {
    format!(
        "api = 1\n[[connector]]\nid = \"lookup\"\ndoc = \"Look a word up.\"\nis = \"tool\"\ndialect = \"http-json\"\nurl = \"{url}\"\nargs = [{{ name = \"word\", required = true }}, {{ name = \"lang\", default = \"en\" }}]\n{extra}\n"
    )
}

fn load(text: &str) -> (j4n_connect::manifest::Connectors, Vec<String>) {
    let (list, said, _) = connectors(&Scratch::new("hj"), text);
    (list, said)
}

fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[test]
fn a_get_fills_the_address_with_escaped_values_and_the_key_goes_in_a_header_only() {
    let server = Mock::start(|_, _| Say::Json(200, json!({ "data": [{ "text": "a small round fruit" }] })));
    let text = row(&format!("{}/define/{{lang}}?q={{word}}", server.url()), "result = \"/data/0/text\"\nkey = \"env:HJ_KEY\"\nheaders = { Authorization = \"Bearer {key}\" }");
    let (list, said) = load(&text);
    assert!(said.is_empty(), "{said:?}");
    let c = list.get("lookup").unwrap();
    let call = HttpJson::new(c, "sk-1".into());
    let answer = call.call(&vars(&[("word", "plum & pear/é"), ("lang", "en")])).expect("answers");
    assert_eq!(answer, "a small round fruit");
    let seen = &server.seen()[0];
    assert_eq!((seen.method.as_str(), seen.path.as_str()), ("GET", "/define/en?q=plum%20%26%20pear%2F%C3%A9"), "{seen:?}");
    assert_eq!(seen.headers.get("authorization").map(String::as_str), Some("Bearer sk-1"));
}

#[test]
fn a_post_puts_each_value_in_the_body_as_a_json_string_whatever_it_holds() {
    let server = Mock::start(|_, _| Say::Json(200, json!({ "ok": true })));
    let text = row(&format!("{}/define", server.url()), "method = \"post\"\nbody = '{ \"word\": \"{word}\", \"lang\": \"{lang}\" }'");
    let (list, said) = load(&text);
    assert!(said.is_empty(), "{said:?}");
    let call = HttpJson::new(list.get("lookup").unwrap(), String::new());
    let answer = call.call(&vars(&[("word", "say \"hi\"\n\\ and } more"), ("lang", "en")])).expect("answers");
    assert_eq!(answer, "{\"ok\":true}", "with no `result`, the answer is the body");
    let seen = &server.seen()[0];
    assert_eq!(seen.method, "POST");
    assert_eq!(seen.json(), json!({ "word": "say \"hi\"\n\\ and } more", "lang": "en" }), "{}", seen.body);
}

#[test]
fn a_status_of_400_or_more_and_an_answer_without_what_was_pointed_at_are_failures_in_words() {
    let server = Mock::start(|_, n| if n == 0 { Say::Text(404, "no such word".into()) } else { Say::Json(200, json!({ "data": [] })) });
    let text = row(&format!("{}/define", server.url()), "result = \"/data/0/text\"");
    let (list, _) = load(&text);
    let call = HttpJson::new(list.get("lookup").unwrap(), String::new());
    let why = call.call(&vars(&[("word", "x")])).unwrap_err();
    assert!(why.contains("lookup answered 404") && why.contains("no such word"), "{why}");
    let why = call.call(&vars(&[("word", "x")])).unwrap_err();
    assert!(why.contains("nothing at `/data/0/text`"), "{why}");
    // a null is nothing too
    let server = Mock::start(|_, _| Say::Json(200, json!({ "data": null })));
    let (list, _) = load(&row(&format!("{}/define", server.url()), "result = \"/data\""));
    let why = HttpJson::new(list.get("lookup").unwrap(), String::new()).call(&vars(&[("word", "x")])).unwrap_err();
    assert!(why.contains("nothing at `/data`"), "{why}");
}

#[test]
fn a_row_that_would_leak_the_key_or_name_an_argument_it_has_not_is_refused_when_it_is_read() {
    let cases = [
        (row("http://localhost/{word}/{key}", "key = \"env:K\""), "stands in a header"),
        (row("http://localhost/{nope}", ""), "not one of the `args`"),
        (row("http://localhost/x", "method = \"fetch\""), "is not"),
        (row("ftp://localhost/x", ""), "starts with http"),
        (row("http://localhost/x", "headers = { Authorization = \"Bearer {key}\" }"), "names no `key`"),
        (row("http://localhost/x", "method = \"post\"\nbody = 'not json {word}'"), "JSON document"),
        (row("http://localhost/x", "result = \"data\""), "JSON pointer"),
    ];
    for (text, want) in cases {
        let (list, said) = load_unchecked(&text);
        assert!(list.get("lookup").is_none(), "{text}");
        assert!(said.iter().any(|s| s.contains(want)), "{want}: {said:?}");
    }
}

fn load_unchecked(text: &str) -> (j4n_connect::manifest::Connectors, Vec<String>) {
    let (list, said, _) = connectors(&Scratch::new("hj"), text);
    (list, said)
}

#[test]
fn an_endpoint_is_a_doer_that_asks_before_it_reaches_out_and_answers_with_the_words_it_was_pointed_at() {
    let server = Mock::start(|_, _| Say::Json(200, json!({ "data": [{ "text": "a small round fruit" }] })));
    let text = row(&format!("{}/define/{{lang}}?q={{word}}", server.url()), "result = \"/data/0/text\"");
    let b = Bench::new(&[], &text);
    let row = b.run("lookup", "{\"word\":\"plum\"}");
    assert_eq!(row.state, State::Done { ok: true }, "{}", b.dump());
    assert_eq!(row.answer.as_deref(), Some("a small round fruit"));
    assert_eq!(b.events("ask.answered").len(), 1, "reaching an address is put to the Gate: {}", b.dump());
    assert_eq!(b.events("ask.answered")[0].data["verb"], "net", "{}", b.dump());
    // plain words are the first argument
    assert_eq!(b.run("lookup", "pear").answer.as_deref(), Some("a small round fruit"));
    assert!(server.seen()[1].path.ends_with("q=pear"), "{:?}", server.seen());
    // a refusal is the run's end, and nothing was sent
    let b2 = Bench::new(&[], &row_for_refusal(&server.url()));
    let before = server.seen().len();
    let id = b2.spawn("lookup", "x");
    let ask = b2.waiting(&id);
    b2.answer(&ask, j4n_core::ask::Answer::No);
    let ended = b2.end(&id);
    assert!(matches!(ended.state, State::Stopped { why: Why::Failed { .. } }), "{ended:?}");
    assert_eq!(server.seen().len(), before);
}

fn row_for_refusal(url: &str) -> String {
    row(&format!("{url}/define"), "")
}

#[test]
fn an_endpoint_is_also_a_tool_of_a_model_with_the_same_gate_and_a_key_it_was_given() {
    let server = Mock::start(|_, _| Say::Json(200, json!({ "data": [{ "text": "a small round fruit" }] })));
    let text = row(&format!("{}/define/{{lang}}?q={{word}}", server.url()), "result = \"/data/0/text\"\nkey = \"env:HJ\"\nheaders = { Authorization = \"Bearer {key}\" }");
    let (list, said) = load(&text);
    assert!(said.is_empty(), "{said:?}");
    let rows = list.all().to_vec();
    assert!(j4n_connect::command::offered(&rows[0]));
    let tools = CommandTools::new(rows).with_keys(vars(&[("lookup", "sk-9")]));
    let dir = Scratch::new("hjt");
    let out = tools.run(&Call { id: "c".into(), name: "lookup".into(), args: json!({ "word": "plum" }), bad: None }, &Site::new("t1", dir.path()), &|l| l);
    assert!(!out.error, "{}", out.text);
    assert_eq!(out.text, "a small round fruit");
    assert_eq!(server.seen()[0].headers.get("authorization").map(String::as_str), Some("Bearer sk-9"));
    let asked = tools.gate(&Call { id: "c".into(), name: "lookup".into(), args: json!({ "word": "plum" }), bad: None }, "t1", &Site::new("t1", dir.path()), j4n_core::gate::ReadOnly::No);
    assert!(asked.is_none() || asked.is_some_and(|g| g.question.starts_with("call `")), "a call to an address is a `net` action");
}
