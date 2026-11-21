//! The reader of `jsonl-map`: a line, the rows of a map, and what comes out.

use j4n_connect::jsonmap::{Mapper, show};
use j4n_connect::manifest::{EventSpec, MapSpec};
use j4n_core::doer::{Metrics, Role, RunEv};
use j4n_core::value::Value;
use serde_json::json;

fn row(when: &str, emit: &str) -> EventSpec {
    EventSpec { when: when.into(), emit: emit.into(), text: None, name: None, detail: None, cmd: None, out: None, exit: None }
}

fn mapper(events: Vec<EventSpec>) -> Mapper {
    Mapper::new(&MapSpec { events, ..MapSpec::default() }).expect("a map that reads")
}

#[test]
fn a_shell_line_reads_its_exit_code_from_whatever_the_program_prints() {
    let mut r = row("/type == 'sh'", "shell");
    (r.cmd, r.out, r.exit) = (Some("/cmd".into()), Some("/out".into()), Some("/exit".into()));
    let m = mapper(vec![r]);
    let exit = |v: serde_json::Value| match &m.line(&json!({ "type": "sh", "cmd": "ls", "out": "x", "exit": v })).evs[..] {
        [RunEv::Shell { exit, .. }] => *exit,
        other => panic!("{other:?}"),
    };
    assert_eq!(exit(json!(3)), 3);
    assert_eq!(exit(json!("7")), 7);
    assert_eq!(exit(json!("x")), 1, "text that is no number is a failure");
    assert_eq!(exit(json!(false)), 1);
    assert_eq!(exit(json!(true)), 0);
    assert_eq!(exit(json!(null)), 0);
}

#[test]
fn a_line_is_an_event_only_when_a_row_holds_and_a_row_with_nothing_to_say_says_nothing() {
    let mut t = row("/type == 'text'", "text");
    t.text = Some("/text".into());
    let m = mapper(vec![t, row("/type == 'tool'", "tool")]);
    assert_eq!(m.line(&json!({ "type": "text", "text": "hi" })).evs, [RunEv::Text("hi".into())]);
    assert!(m.line(&json!({ "type": "text" })).evs.is_empty(), "no words, no event");
    assert!(m.line(&json!({ "type": "other", "text": "hi" })).evs.is_empty());
    assert_eq!(m.line(&json!({ "type": "tool" })).evs, [RunEv::Tool { name: "tool".into(), detail: String::new(), end: false }], "a tool with no name is `tool`");
}

#[test]
fn the_other_kinds_of_event_and_the_fields_beside_them() {
    let mut message = row("/t == 'm'", "message");
    message.text = Some("/x".into());
    let mut changed = row("/t == 'c'", "changed");
    changed.text = Some("/x".into());
    let spec = MapSpec {
        session: Some("/s".into()),
        end: Some("/t == 'end'".into()),
        answer: Some("/a".into()),
        cost: Some("/c".into()),
        error: Some("/e".into()),
        events: vec![message, changed],
        ..MapSpec::default()
    };
    let m = Mapper::new(&spec).unwrap();
    assert_eq!(m.line(&json!({ "t": "m", "x": "done" })).evs, [RunEv::Message { role: Role::Assistant, text: "done".into() }]);
    assert_eq!(m.line(&json!({ "t": "c", "x": ["a.txt", "b.txt"] })).evs, [RunEv::Changed(vec!["a.txt".into(), "b.txt".into()])]);
    assert_eq!(m.line(&json!({ "t": "c", "x": "one.txt" })).evs, [RunEv::Changed(vec!["one.txt".into()])]);
    assert!(m.line(&json!({ "t": "c" })).evs.is_empty());
    let end = m.line(&json!({ "t": "end", "s": "sess", "a": "the answer", "c": "$0.25", "e": "boom" }));
    assert_eq!((end.end, end.session.as_deref(), end.answer.as_deref(), end.cost, end.error.as_deref()), (true, Some("sess"), Some("the answer"), Some(0.25), Some("boom")));
    assert_eq!(end.metrics(), Some(Metrics { usd: 0.25, ..Metrics::default() }));
    let plain = m.line(&json!({ "t": "none", "c": 0 }));
    assert_eq!((plain.end, plain.session.clone(), plain.answer.clone(), plain.error.clone(), plain.metrics()), (false, None, None, None, None), "a cost of nothing is no metric");
    assert_eq!(m.line(&json!({ "t": "x", "c": 1.5 })).cost, Some(1.5));
}

#[test]
fn a_map_that_does_not_read_is_refused_with_where() {
    let bad = |spec: MapSpec| Mapper::new(&spec).err().unwrap_or_default();
    assert!(bad(MapSpec { events: vec![row("/t ==", "text")], ..MapSpec::default() }).contains("map.events[0]"));
    assert!(bad(MapSpec { events: vec![row("/t == 'x'", "dance")], ..MapSpec::default() }).contains("`dance`"));
    assert!(bad(MapSpec { cost: Some("nosuchroot".into()), ..MapSpec::default() }).contains("map.cost"));
}

#[test]
fn values_are_shown_as_the_words_they_are() {
    assert_eq!(show(&Value::Missing), "");
    assert_eq!(show(&Value::Num(3.0)), "3");
    assert_eq!(show(&Value::Num(2.5)), "2.5");
    assert_eq!(show(&Value::Bool(true)), "true");
    assert_eq!(show(&Value::Str("x".into())), "x");
    assert_eq!(show(&Value::List(vec![Value::Num(1.0), Value::Str("a".into())])), "[1.0,\"a\"]");
}
