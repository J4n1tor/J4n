//! Quick input. A line in plain language that is a wording of a command runs it with no model asked (the
//! phrase table's `likes`: synonyms, small typos), a line that is not goes on to Jev and the doer, and the chart wordings put the chart in the
//! chat as a held row.

use crate::front_cases::{base, chatty, fx, fx_deciding};
use j4n_core::change::Actor;
use j4n_core::front::Said;
use j4n_core::front::phrase::Phrases;
use j4n_core::front::serve::Serve;
use j4n_core::log::New;
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use j4n_proto::{Down, Intent, Up};
use serde_json::json;
use std::sync::atomic::Ordering;

fn table() -> Phrases {
    let (phrases, problems) = Phrases::load(&base());
    assert!(problems.is_empty(), "{problems:?}");
    phrases
}

/// The base pack with a user's rows beside it.
fn with(mine: &str) -> Tables {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/base");
    let (mut layer, read) = Layer::from_dir(LayerKind::Pack, "base-data", &dir);
    assert!(read.is_empty(), "{read:?}");
    layer.files.retain(|f| !f.path.starts_with("kinds/") && f.path != "pack.toml");
    let user = Layer::new(LayerKind::User, "home", vec![SourceFile { path: "mine.toml".into(), text: format!("api = 1\n{mine}") }]);
    Tables::load(vec![layer, user]).0
}

fn took(p: &Phrases, line: &str) -> Option<String> {
    p.take(line).map(|t| Phrases::said(&t.cmd))
}

#[test]
fn the_table_takes_a_wording_its_synonyms_and_small_typos_for_a_command_and_nothing_near_it_for_one() {
    let p = table();
    for line in ["show me the usage", "Show me the usage!", "usage", "show usage", "please show me my usage", "show me the usege", "shwo me the usgae", "how much have I used?", "how much did i use", "what is left", "limits"] {
        assert_eq!(took(&p, line).as_deref(), Some(":usage"), "{line}");
    }
    assert_eq!(took(&p, "show me spend this week").as_deref(), Some(":chart spend"));
    assert_eq!(took(&p, "spend this week").as_deref(), Some(":chart spend"));
    assert_eq!(took(&p, "show me the spending chart").as_deref(), Some(":chart spend"));
    assert_eq!(took(&p, "how much did I spnd").as_deref(), Some(":chart spend"));
    assert_eq!(took(&p, "show me task time").as_deref(), Some(":chart wall"));
    assert_eq!(took(&p, "show me the bench results").as_deref(), Some(":chart bench"));
    assert_eq!(took(&p, "branches per line").as_deref(), Some(":chart lanes"));
    assert_eq!(took(&p, "list commands").as_deref(), Some(":help"));
    // a word more or fewer, a word two typos away, a long request: the table does not guess, the line goes on
    for line in ["show me the usage of grep in this repo", "show me the usage example for clap", "usagexyz", "say a", "say", "tell me more", "can you show me", "show me the", "", "fix the spend function in billing.rs", "show me the usage and then write a long essay about it please now"] {
        assert_eq!(took(&p, line), None, "{line}");
    }
}

#[test]
fn a_line_the_table_does_not_know_is_not_taken_and_a_pack_adds_wordings_as_data() {
    let (phrases, problems) = Phrases::load(&with(
        "[[phrase]]\nid = \"mine-status\"\ncommand = \"status\"\nlikes = [\"where are we\", \"how are we doing\"]\n\n[[phrase]]\nid = \"mine-bad\"\ncommand = \"status\"\nlikes = [\"show me\"]\n",
    ));
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].to_string().contains("filler"), "{problems:?}");
    assert_eq!(took(&phrases, "wher are we").as_deref(), Some(":status"));
    assert_eq!(took(&phrases, "how are we doing?").as_deref(), Some(":status"));
    assert_eq!(took(&phrases, "show me the usage").as_deref(), Some(":usage"), "what the base pack says stands");
    // the same wording for two commands at the same distance is nobody's
    let (phrases, _) = Phrases::load(&with("[[phrase]]\nid = \"mine-twin\"\ncommand = \"status\"\nlikes = [\"usage\"]\n"));
    assert_eq!(phrases.take("usage"), None);
    assert_eq!(took(&phrases, "usage limits").as_deref(), Some(":usage"));
}

#[test]
fn show_me_the_usage_runs_usage_with_no_task_no_question_and_a_row_that_says_how_to_send_it_to_the_model() {
    let f = fx_deciding(vec![chatty("alpha", &["ok"])], Some("task"), |_| {});
    let said = f.say("show me the usage");
    assert!(matches!(&said, Said::Command(c) if c == "usage"), "{said:?}");
    assert!(f.tasks().is_empty(), "no task was started");
    assert_eq!(f.decided.load(Ordering::SeqCst), 0, "nobody was asked");
    assert_eq!(f.count("chat.view"), 1);
    let note = f.notes().into_iter().find(|n| n.contains("took this as")).expect("the row says what the line was taken as");
    assert!(note.contains("`:usage`") && note.contains(":ask show me the usage"), "{note}");
    let why = f.rig.of_kind("intake.decided");
    assert_eq!((why.len(), why[0].data["pick"].as_str(), why[0].data["by"].as_str()), (1, Some("command"), Some("front:phrases")));
    // a typo is the same, and the colon word is no different
    assert!(matches!(f.say("shwo me the usege"), Said::Command(c) if c == "usage"));
    assert!(matches!(f.say(":usage"), Said::Command(c) if c == "usage"));
    assert_eq!(f.notes().iter().filter(|n| n.contains("took this as")).count(), 2, "the colon word is a command, not a guess: no row");
}

#[test]
fn a_line_that_is_unclear_goes_to_jev_and_then_to_the_doer_and_ask_sends_a_wording_to_the_model_as_it_is() {
    let f = fx_deciding(vec![chatty("alpha", &["here it is", "ok"])], Some("task"), |c| c.doer = None);
    let r = f.turn("show me the usage of grep in this repo");
    assert_eq!(r.spec.doer, "alpha");
    assert!(f.decided.load(Ordering::SeqCst) >= 1, "intake asked the decider");
    assert!(f.notes().iter().all(|n| !n.contains("took this as")), "{:?}", f.notes());
    assert_eq!(f.count("chat.view"), 0);
    // `:ask` is the way back from a wording: the thread has its doer, so the line is its next turn, as typed
    let r = f.turn(":ask show me the usage");
    assert_eq!(r.spec.goal, "show me the usage", "{:?}", r.spec);
    assert_eq!(f.count("chat.view"), 0, "/usage was not run");
}

// ---- the charts ----

fn serve_chat(f: &crate::front_cases::Fx) -> (Serve, Vec<Down>) {
    let mut s = Serve::new(f.front.clone());
    s.up(Up::Hello { api: j4n_proto::API });
    let (downs, _) = s.up(Up::Watch { bind: "chat:main".into() });
    (s, downs)
}

fn views(downs: &[Down]) -> Vec<serde_json::Value> {
    downs.iter().rev().find_map(|d| match d {
        Down::Rows { bind, rows, .. } if bind == "chat:main" => Some(rows.iter().filter_map(|r| r.data.as_ref()?.get("view").cloned()).collect()),
        _ => None,
    }).unwrap_or_default()
}

#[test]
fn show_me_spend_this_week_holds_the_spend_chart_in_the_chat_as_the_page_draws_it_and_draws_it_again_when_money_is_spent() {
    let f = fx(vec![chatty("alpha", &["ok"])], |_| {});
    let (mut s, _) = serve_chat(&f);
    for usd in [0.25, 0.5] {
        f.rig.log.append(New::of("run.metrics", Actor::Doer("alpha".into()), json!({ "usd": usd, "tokens_in": 10, "tokens_out": 5 })).task("t1").thread("other")).unwrap();
    }
    let (downs, _) = s.up(Up::Intent { intent: Intent::Say { text: "show me spend this week".into(), with: Vec::new() } });
    assert!(downs.iter().all(|d| !matches!(d, Down::Toast { .. })), "{downs:?}");
    assert!(f.tasks().is_empty(), "no task");
    let made = f.rig.of_kind("chat.view");
    assert_eq!((made.len(), made[0].data["view"].as_str(), made[0].data["title"].as_str()), (1, Some("chart:spend"), Some("spend")));
    assert!(made[0].data["text"].as_str().unwrap_or_default().contains("usd"), "{:?}", made[0].data);
    let sent: Vec<Down> = f.rig.log.read(0).flat_map(|e| s.ev(&e)).collect();
    let held = views(&sent);
    assert_eq!(held.len(), 1, "{sent:?}");
    assert_eq!((held[0]["field"].as_str(), held[0]["title"].as_str()), (Some("line"), Some("spend")));
    assert_eq!(held[0]["props"]["series"][0]["name"], "usd", "the props are the page's: {}", held[0]["props"]);
    let usd = held[0]["value"]["series"]["usd"].as_array().unwrap().iter().filter_map(|v| v.as_f64()).sum::<f64>();
    assert!((usd - 0.75).abs() < 1e-9, "{}", held[0]["value"]);
    // money spent in another thread: the row is sent again with it counted
    let at = f.rig.log.append(New::of("run.metrics", Actor::Doer("alpha".into()), json!({ "usd": 1.0 })).task("t2").thread("other")).unwrap();
    let again = views(&s.ev(&at));
    let usd = again[0]["value"]["series"]["usd"].as_array().unwrap().iter().filter_map(|v| v.as_f64()).sum::<f64>();
    assert!((usd - 1.75).abs() < 1e-9, "{again:?}");
}

#[test]
fn each_chart_has_its_words_and_a_chart_with_nothing_to_draw_says_so_in_its_row() {
    let f = fx(vec![chatty("alpha", &["ok"])], |_| {});
    let (mut s, _) = serve_chat(&f);
    for (line, view) in [("show me the task time", "chart:wall"), ("show me the bench results", "chart:bench"), ("show me branches per line", "chart:lanes")] {
        assert!(matches!(f.say(line), Said::Command(c) if c == "chart"), "{line}");
        let at = f.rig.of_kind("chat.view").last().cloned().unwrap();
        assert_eq!(at.data["view"], view, "{line}");
        let sent = s.ev(&at);
        let held = views(&sent);
        assert_eq!(held.last().and_then(|v| v["field"].as_str()), Some("chip"), "{line}: nothing to draw yet: {held:?}");
    }
    assert!(matches!(f.say(":chart nonsense"), Said::Refused(_)));
}
