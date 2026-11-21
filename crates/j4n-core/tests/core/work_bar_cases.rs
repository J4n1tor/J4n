//! The short name of a model and the composer's strip with the work bar: the bar is there the
//! moment a line is taken, one entry per task that runs, gone when idle; the cost, the mode and the `lanes` tag as the plan says.

use crate::front_cases::fx;
use crate::scripted::{Beat, Scripted, end, say};
use j4n_core::front::Said;
use j4n_core::front::serve::Serve;
use j4n_core::task::{model_name, model_word, who};
use j4n_proto::{COMPOSER_STRIP, Down, Intent, Up};
use serde_json::{Value, json};

fn strip_of(downs: &[Down]) -> Option<Value> {
    downs.iter().rev().find_map(|d| match d {
        Down::Value { bind, value, .. } if bind == COMPOSER_STRIP => Some(value.clone()),
        _ => None,
    })
}

#[test]
fn a_model_id_is_said_short_and_the_rule_is_the_same_for_every_maker() {
    for (id, short) in [
        ("claude-sonnet-5-5", "sonnet 5.5"),
        ("claude-opus-5-5", "opus 5.5"),
        ("claude-haiku-5-5", "haiku 5.5"),
        ("anthropic/claude-haiku-5.5", "haiku 5.5"),
        ("claude-opus-4-1-20250805", "opus 4.1"),
        ("claude-3-5-sonnet-20241022", "sonnet 3.5"),
        ("claude-fable-5-1", "fable 5.1"),
        ("~anthropic/claude-sonnet-latest", "sonnet-latest"),
        ("claude-opus-5-5[1m]", "opus 5.5[1m]"),
        ("anthropic/claude-haiku-5.5:batch", "haiku 5.5:batch"),
        ("openai/gpt-oss-20b", "gpt-oss-20b"),
        ("gpt-4.1", "gpt 4.1"),
        ("gpt-4o", "gpt-4o"),
        ("deepseek/deepseek-v4.1-flash-1017", "deepseek-v4.1-flash-1017"),
        ("haiku", "haiku"),
        ("sonnet", "sonnet"),
        ("model not known yet", "model not known yet"),
        ("", ""),
    ] {
        assert_eq!(model_name(id), short, "{id:?}");
    }
}

#[test]
fn the_words_the_operator_reads_are_short_and_the_row_keeps_the_id() {
    assert_eq!(model_word("claude", "claude-sonnet-5-5", "sonnet"), "sonnet 5.5", "what served, short");
    assert_eq!(model_word("claude", "", "haiku"), "haiku", "what was asked for until it is served");
    assert_eq!(who("claude", "claude-opus-5-5", ""), "claude · opus 5.5");
    assert_eq!(model_word("test", "", ""), "", "the scripted doer serves no model");
    assert_eq!(model_word("claude", "", ""), "model not known yet");
}

#[test]
fn the_work_bar_is_in_the_strip_when_the_line_is_taken_and_gone_when_the_task_is_over() {
    // a doer that has said nothing yet: the bar must not wait for its first frame
    let f = fx(vec![Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Forever]])], |c| c.model = Some("haiku".into()));
    let mut s = Serve::new(f.front.clone());
    let _ = s.up(Up::Watch { bind: COMPOSER_STRIP.into() });
    let (downs, _) = s.up(Up::Intent { intent: Intent::Say { text: "hello".into(), with: Vec::new() } });
    let strip = strip_of(&downs).expect("the strip is sent in answer to the line");
    let work = strip["work"].as_array().expect("work");
    assert_eq!(work.len(), 1, "{strip}");
    assert!(work[0].get("frac").is_none(), "a task's own fraction is not sent, nothing drew it: {strip}");
    assert_eq!(strip["mode"], json!("write"));
    assert_eq!(strip["tags"], json!([]), "`lanes single` is not said: {strip}");
    // it ends with the task
    let Some(task) = f.tasks().last().map(|r| r.id.clone()) else { panic!("no task") };
    let _ = f.front.control("main", &task, j4n_proto::Control::Abort);
    f.rig.finished(&task);
    let (downs, _) = s.up(Up::Watch { bind: COMPOSER_STRIP.into() });
    assert_eq!(strip_of(&downs).expect("strip")["work"], json!([]));
}

#[test]
fn each_running_task_has_its_own_entry_and_the_doers_list_sends_no_fraction() {
    let f = fx(
        vec![
            Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::List(3, 4, "tests"), Beat::Forever]]),
        ],
        |_| {},
    );
    let mut s = Serve::new(f.front.clone());
    let _ = s.up(Up::Watch { bind: j4n_core::front::page::chat("main") });
    let _ = s.up(Up::Watch { bind: COMPOSER_STRIP.into() });
    let Said::Started { task } = f.say("go") else { panic!() };
    for _ in 0..100 {
        if f.rig.log.read(0).any(|e| e.kind == "run.list") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let _: Vec<Down> = f.rig.log.read(0).flat_map(|e| s.ev(&e)).collect();
    let (downs, _) = s.up(Up::Watch { bind: COMPOSER_STRIP.into() });
    let strip = strip_of(&downs).expect("strip");
    assert!(strip["work"][0].get("frac").is_none(), "the doer's list sets no fraction on the strip: {strip}");
    let _ = f.front.control("main", &task, j4n_proto::Control::Abort);
}

#[test]
fn the_cost_of_the_thread_rides_in_the_strip_once_it_has_cost_something() {
    let f = fx(vec![Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Served("claude-sonnet-5-5", "anthropic"), Beat::Spend(0.5), say("a"), end("a")]])], |c| c.model = Some("sonnet".into()));
    let mut s = Serve::new(f.front.clone());
    let (downs, _) = s.up(Up::Watch { bind: COMPOSER_STRIP.into() });
    assert!(strip_of(&downs).is_none_or(|v| v.is_null() || v["cost"] == json!("")), "nothing spent yet");
    f.turn("hello");
    let (downs, _) = s.up(Up::Watch { bind: COMPOSER_STRIP.into() });
    let strip = strip_of(&downs).expect("strip");
    assert_eq!(strip["route"], json!("alpha · sonnet 5.5"), "{strip}");
    assert!(strip["cost"].as_str().is_some_and(|c| c.contains('$')), "{strip}");
    assert_eq!(strip["work"], json!([]));
}

#[test]
fn a_thread_with_no_doer_yet_says_so_in_the_strip_and_a_bound_one_says_the_doer() {
    let f = fx(vec![Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Served("claude-sonnet-5-5", "anthropic"), say("a"), end("a")]])], |c| c.model = Some("sonnet".into()));
    let mut s = Serve::new(f.front.clone());
    let (downs, _) = s.up(Up::Watch { bind: COMPOSER_STRIP.into() });
    let strip = strip_of(&downs).expect("a strip before the first line");
    assert_eq!(strip["route"], json!("no doer yet"), "{strip}");
    assert_eq!(strip["route_by"], json!("none"), "the screen draws it dim, not as a route: {strip}");
    f.turn("hello");
    let (downs, _) = s.up(Up::Watch { bind: COMPOSER_STRIP.into() });
    let strip = strip_of(&downs).expect("strip");
    assert_eq!(strip["route"], json!("alpha · sonnet 5.5"), "{strip}");
    assert_eq!(strip["route_by"], json!("core"), "{strip}");
}

/// A pin to `haiku` served by sonnet was silent in the Log; now the run says it once, in plain words.
#[test]
fn a_run_served_by_another_model_than_the_one_asked_says_so_once_and_one_that_is_served_as_asked_says_nothing() {
    let turn = |first: &'static str, then: &'static str| vec![Beat::Session("s".into()), Beat::Served(first, "anthropic"), Beat::Served(then, "anthropic"), Beat::Served(then, "anthropic"), say("a"), end("a")];
    // the `init` frame names the setting, the `result` frame the model that ran: haiku asked, sonnet served
    let f = fx(vec![Scripted::new("alpha", vec![turn("claude-haiku-5-5", "claude-sonnet-5-5")])], |c| c.model = Some("haiku".into()));
    f.turn("hello");
    let notes: Vec<String> = f.rig.of_kind("run.notice").into_iter().map(|e| e.data["text"].as_str().unwrap_or("").to_string()).collect();
    assert_eq!(notes.len(), 1, "once, however often it is said again: {notes:?}");
    assert!(notes[0].contains("haiku") && notes[0].contains("claude-sonnet-5-5") && notes[0].contains("billed"), "{notes:?}");
    let served: Vec<String> = f.rig.of_kind("run.served").into_iter().map(|e| e.data["model"].as_str().unwrap_or("").to_string()).collect();
    assert_eq!(served.first().map(String::as_str), Some("claude-haiku-5-5"), "the notice comes after the row that shows it: {served:?}");
    // the model that was asked for is the model that ran: nothing to say
    let f = fx(vec![Scripted::new("alpha", vec![turn("claude-haiku-5-5", "claude-haiku-5-5")])], |c| c.model = Some("haiku".into()));
    f.turn("hello");
    assert!(f.rig.of_kind("run.notice").is_empty(), "{:?}", f.rig.of_kind("run.notice"));
}

#[test]
fn names_are_compared_by_their_words_not_their_spelling() {
    use j4n_core::doer::served_other;
    for (asked, served, other) in [
        ("haiku", "claude-haiku-5-5", false),
        ("haiku", "claude-sonnet-5-5", true),
        ("opusplan", "claude-sonnet-5-5", true),
        ("sonnet[1m]", "claude-sonnet-5-5", false),
        ("anthropic/claude-haiku-4.5", "anthropic/claude-4.5-haiku-20251001", false),
        ("meta/llama-3.1-70b-instruct", "llama-3.1-70b-instruct", false),
        ("gpt-5", "gpt-5-2025-08-07", false),
        ("gpt-5", "o3-mini", true),
        ("m", "m-5-5-full", false),
        ("default", "claude-sonnet-5-5", false),
        ("", "claude-sonnet-5-5", false),
        ("haiku", "", false),
    ] {
        assert_eq!(served_other(asked, served), other, "{asked} / {served}");
    }
}
