//! The way of working, asked when the read is unsure: a line the read said splits and named no parts for is not worked as one lane in silence. The
//! operator gets one pick row before any work starts (one lane recommended, with the read's reason; split into chats; their own words), and the Log
//! says who decided. `orchestrate.way` `decide`, and a run with nobody to ask, take one lane and say so.

use crate::front_cases::{Fx, chatty, fx, fx_headless};
use j4n_core::branch::Switch;
use j4n_core::front::Cfg;
use j4n_core::front::look::{Decision, Looked, NO_PARTS, taken};
use j4n_core::front::Said;
use j4n_proto::{AskView, Reply};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// The read's model says split and gives no part ("one small build … single task").
const NO_PART: &str = r#"{"kind":"split","chats":[],"features":[],"why":"one small build, a single task"}"#;
const LINE: &str = "add a status page to the service at example.com and its test";

fn looking(c: &mut Cfg) {
    c.doer = Some("claude".into());
    c.lanes = Switch::Orchestrate;
    c.chats.screen = true;
    c.orchestrate.look = true;
    c.cheap.doer = Some("cheap".into());
}

fn rig(tune: fn(&mut Cfg)) -> Fx {
    fx(vec![chatty("claude", &["no plan here", "done", "done"]), chatty("cheap", &[NO_PART])], |c| {
        looking(c);
        tune(c);
    })
}

fn kinds(f: &Fx, kind: &str) -> Vec<Value> {
    f.rig.of_kind(kind).into_iter().map(|e| e.data).collect()
}

fn work(f: &Fx) -> Vec<j4n_core::task::Row> {
    f.tasks().into_iter().filter(|r| r.spec.role.as_deref() == Some("work")).collect()
}

/// Type the line on a thread of its own and wait for the pick row it puts.
fn asking(f: &Fx) -> (std::thread::JoinHandle<Said>, AskView) {
    let front = f.front.clone();
    let job = std::thread::spawn(move || front.say("main", LINE));
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(v) = f.front.hand_asks().into_iter().next() {
            return (job, v);
        }
        assert!(!job.is_finished() && Instant::now() < end, "no pick row was shown\n{}", f.rig.dump());
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_split_the_read_named_no_parts_for_is_one_lane_and_says_why() {
    let empty = Looked { decision: Decision::Split, chats: vec![], features: vec![], why: String::new() };
    assert_eq!(taken(&empty, LINE, false), (Decision::One, NO_PARTS));
    assert_eq!(taken(&empty, LINE, true).0, Decision::One, "big files do not make parts the read did not name");
}

#[test]
fn an_unsure_read_puts_one_pick_row_before_any_work_and_split_goes_to_main_s_planning_turn() {
    let f = rig(|_| {});
    let (job, row) = asking(&f);
    assert!(work(&f).is_empty(), "no work before the pick: {}", f.rig.dump());
    assert_eq!(row.question, "how should this line be worked?");
    assert_eq!(row.asker, "read", "a question with the row custom: {row:?}");
    // the labels are the point's `says`, which the start wires and this rig does not
    assert_eq!(row.options.iter().map(|o| (o.id.as_str(), o.recommended)).collect::<Vec<_>>(), [("one", true), ("split", false)]);
    assert!(row.why.contains(NO_PARTS) && row.why.contains("one small build"), "the read's reason: {}", row.why);
    let looked = &kinds(&f, "orchestrate.looked")[0];
    assert_eq!((looked["decision"].as_str(), looked["taken"].as_str(), looked["clamped"].as_str()), (Some("split"), Some("one"), Some(NO_PARTS)), "{looked}");
    assert!(matches!(f.front.reply("main", &row.ask, Reply::Pick { id: "split".into() }), Said::Nothing));
    job.join().unwrap();
    let way = &kinds(&f, "orchestrate.way")[0];
    assert_eq!((way["pick"].clone(), way["by"].clone(), way["recommended"].clone(), way["ways"].clone()), (json!("split"), json!("you"), json!("one"), json!(["one", "split"])), "{way}");
    // Main's planning turn was asked for the briefs; its answer was none, so Main works the line itself and says so
    assert!(f.tasks().iter().any(|r| r.spec.role.as_deref() == Some("split")), "{}", f.rig.dump());
    assert!(!kinds(&f, "orchestrate.split").is_empty() && f.notes().iter().any(|n| n == "way: split into chats · you picked it"), "{:?}", f.notes());
}

#[test]
fn enter_takes_one_lane_and_the_operator_s_own_words_go_to_main_with_the_line() {
    let f = rig(|_| {});
    let (job, row) = asking(&f);
    f.front.reply("main", &row.ask, Reply::Yes);
    job.join().unwrap();
    assert_eq!(kinds(&f, "orchestrate.way")[0]["pick"], json!("one"));
    assert!(f.tasks().iter().all(|r| r.spec.role.as_deref() != Some("split")));
    assert_eq!(work(&f).len(), 1, "{}", f.rig.dump());
    // the row custom: words are the operator's way, a ready answer that leaves it to J4n is the read's way
    let f = rig(|_| {});
    let (job, row) = asking(&f);
    f.front.reply("main", &row.ask, Reply::Text { text: "tests first, then the page".into() });
    job.join().unwrap();
    let way = &kinds(&f, "orchestrate.way")[0];
    assert_eq!((way["pick"].clone(), way["words"].clone()), (json!("one"), json!("tests first, then the page")), "{way}");
    let turn = work(&f).pop().expect("Main's turn");
    assert!(turn.spec.goal.contains(LINE) && turn.spec.goal.contains("How to work it, in the operator's words: tests first, then the page"), "{}", turn.spec.goal);
    let f = rig(|_| {});
    let (job, row) = asking(&f);
    f.front.reply("main", &row.ask, Reply::Text { text: "Decide yourself: pick what you judge best and go on.".into() });
    job.join().unwrap();
    let way = &kinds(&f, "orchestrate.way")[0];
    assert_eq!((way["pick"].clone(), way["by"].clone(), way["words"].clone()), (json!("one"), json!("fallback"), Value::Null), "{way}");
}

#[test]
fn decide_and_a_run_with_nobody_to_ask_take_one_lane_with_no_question_and_the_log_says_who() {
    let f = rig(|c| c.orchestrate.way_ask = false);
    f.turn(LINE);
    assert!(f.front.hand_asks().is_empty());
    assert_eq!((kinds(&f, "orchestrate.way")[0]["by"].clone(), work(&f).len()), (json!("decide"), 1), "{}", f.rig.dump());
    let f = fx_headless(vec![chatty("claude", &["done", "done"]), chatty("cheap", &[NO_PART])], None, looking);
    f.turn(LINE);
    let way = &kinds(&f, "orchestrate.way")[0];
    assert_eq!((way["pick"].clone(), way["by"].clone()), (json!("one"), json!("no operator")), "{way}");
    assert!(f.notes().iter().any(|n| n.starts_with(&format!("read as one ({NO_PARTS})"))), "the row says the clamp: {:?}", f.notes());
    assert!(f.notes().iter().any(|n| n == "way: one lane · the read's way: nobody to ask"), "{:?}", f.notes());
    assert_eq!(work(&f).len(), 1);
}

#[test]
fn a_read_with_parts_or_of_one_lane_asks_nothing() {
    let parts = r#"{"kind":"split","chats":[{"goal":"the page","writes":true,"doer":"claude","model":""},{"goal":"its test","writes":true,"doer":"claude","model":""}],"features":[],"why":"two parts"}"#;
    for answer in [r#"{"kind":"one","chats":[],"features":[],"why":"one change"}"#, parts] {
        let f = fx(vec![chatty("claude", &["done", "done"]), chatty("cheap", &[answer])], looking);
        f.turn(LINE);
        assert!(kinds(&f, "orchestrate.way").is_empty() && f.front.hand_asks().is_empty(), "{}", f.rig.dump());
    }
}
