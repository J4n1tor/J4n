//! What a line is before it is JSON (`decode`), for each shape the forge's seen values print. The words are made up.

use j4n_connect::decode::{line, steps};
use serde_json::json;

fn run(words: &[&str], raw: &str) -> Option<serde_json::Value> {
    line(&steps(&words.iter().map(|w| w.to_string()).collect::<Vec<_>>()).unwrap(), raw)
}

#[test]
fn colours_redraws_and_spinners_are_gone_and_the_last_drawn_text_stays() {
    let raw = "\r\u{2819} thinking\u{2026}\r\u{2803} thinking\u{2026}\r\x1b[K\x1b[1;32mplum41\x1b[0m";
    assert_eq!(run(&["ansi", "plain"], raw), Some(json!({ "line": "plum41" })));
}

#[test]
fn a_server_sent_event_keeps_its_data_and_drops_its_other_lines() {
    assert_eq!(run(&["sse"], "data: {\"a\":{\"b\":\"x\"}}"), Some(json!({ "a": { "b": "x" } })));
    assert_eq!(run(&["sse"], "event: say"), None);
    assert_eq!(run(&["sse"], ""), None);
    assert_eq!(run(&["sse"], "data: [DONE]"), None);
}

#[test]
fn a_frame_loses_its_length_and_a_plain_json_line_is_read_as_it_stands() {
    assert_eq!(run(&["frames"], "000012{\"k\":\"say\"}"), Some(json!({ "k": "say" })));
    assert_eq!(run(&[], "{\"k\":\"end\"}"), Some(json!({ "k": "end" })));
    assert_eq!(run(&[], "not json"), None);
}

#[test]
fn logfmt_becomes_an_object_and_a_quoted_value_keeps_its_spaces() {
    assert_eq!(run(&["logfmt"], "lvl=info evt=say body=\"two words\" n=3"), Some(json!({ "lvl": "info", "evt": "say", "body": "two words", "n": "3" })));
}

#[test]
fn a_colon_line_is_a_pair_and_every_line_has_its_head_and_tail() {
    assert_eq!(run(&["colon"], "session: s12ab"), Some(json!({ "session": "s12ab", "line": "session: s12ab", "head": "session:", "tail": "s12ab" })));
    assert_eq!(run(&["colon"], "* read notes.txt"), Some(json!({ "line": "* read notes.txt", "head": "*", "tail": "read notes.txt" })));
}

#[test]
fn a_step_that_is_not_one_is_named() {
    assert!(steps(&["ansi".into(), "yaml".into()]).unwrap_err().contains("`yaml`"));
}
