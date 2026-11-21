//! `/context` and the numbers of a turn in `task.settled`. The loop, the front and the Log are the real
//! ones and the doers are scripted; a session is resumed over three turns that read what claude's frames said of the 108,449-token
//! `ping` (`wrote` is most of it when the cache was cold), and what is asserted is read back from the Log and from the view.

use crate::front_cases::{fx, fx_planned};
use crate::scripted::{Beat, Scripted, end, say};
use j4n_core::change::Actor;
use j4n_core::front::Said;
use j4n_core::front::context::{self, VIEW};
use j4n_core::front::usage::plain;
use j4n_core::log::Ev;
use j4n_core::task::context::{sessions, turn};
use j4n_core::doer::RunCtl;
use j4n_core::task::{Book, Row, SessionRef, State, TaskSpec, Why};
use serde_json::{Value, json};
use std::sync::Arc;

const MIN: u64 = 60_000;

/// One turn of a claude session: the session, the model that served it, what it read (all, the part read, the part written) and how
/// full the window was.
fn claude_turn(session: &str, all: u64, cached: u64, wrote: u64, window: f64) -> Vec<Beat> {
    vec![Beat::Session(session.into()), Beat::Served("claude-haiku-5-5", "anthropic"), Beat::Cached(0.1, all, cached, wrote), Beat::Window(window), say("x"), end("x")]
}

/// The `ctx` of each task that settled, without the dollars (they have their own test).
fn settled_ctx(f: &crate::front_cases::Fx) -> Vec<Value> {
    let bare = |mut c: Value| {
        c.as_object_mut().map(|o| (o.remove("warm_usd"), o.remove("cold_usd")));
        c
    };
    f.rig.of_kind("task.settled").iter().filter(|e| e.task.is_some()).map(|e| bare(e.data["ctx"].clone())).collect()
}

fn next_line(f: &crate::front_cases::Fx) -> String {
    text(f, "main").lines().find(|l| l.trim_start().starts_with("next line")).unwrap_or_default().to_string()
}

fn text(f: &crate::front_cases::Fx, thread: &str) -> String {
    plain(&f.front.context_value(thread)["value"])
}

#[test]
fn a_resumed_session_growing_over_three_turns_has_its_floor_and_history_and_each_turn_says_them() {
    let claude = Scripted::new(
        "claude",
        vec![claude_turn("s1", 30_000, 12_000, 18_000, 0.15), claude_turn("s1", 60_000, 58_000, 2_000, 0.3), claude_turn("s1", 108_000, 0, 108_000, 0.54)],
    );
    let f = fx_planned(vec![claude], |_| {});
    f.turn("one");
    f.rig.clock.advance(MIN);
    f.turn("two");
    // the cache lives five minutes (the row of the claude cost source): six minutes is cold
    f.rig.clock.advance(6 * MIN);
    f.turn("three");
    assert_eq!(
        settled_ctx(&f),
        vec![
            json!({ "in": 30_000, "floor": 30_000, "hist": 0, "cold": true }),
            json!({ "in": 60_000, "floor": 30_000, "hist": 30_000, "cold": false }),
            json!({ "in": 108_000, "floor": 30_000, "hist": 78_000, "cold": true }),
        ]
    );
    let said = text(&f, "main");
    assert!(said.contains("main · claude · haiku 5.5"), "{said}");
    assert!(said.contains("108k held: floor 30k + history 78k"), "{said}");
    assert!(said.contains("54%"), "the window: {said}");
    assert!(said.contains("(warm now)"), "the last turn has just ended: {said}");
}

#[test]
fn the_next_line_is_priced_warm_and_cold_from_the_price_of_a_token_the_log_has_seen() {
    // 30,000 read (12,000 from the cache, 18,000 written) and 5 out for $0.10: at the ratios 0.1, 1.25 and 5 that is 23,725 input tokens, so a
    // token is $0.1 / 23,725; the next line reads the 30,000 from the cache ($0.0126) or writes them again ($0.1581)
    let f = fx_planned(vec![Scripted::new("claude", vec![claude_turn("s1", 30_000, 12_000, 18_000, 0.15)])], |_| {});
    f.turn("one");
    let ctx = &f.rig.of_kind("task.settled")[0].data["ctx"];
    let (warm, cold) = (ctx["warm_usd"].as_f64().unwrap(), ctx["cold_usd"].as_f64().unwrap());
    assert!((warm - 0.012645).abs() < 1e-6 && (cold - 0.158061).abs() < 1e-6, "{ctx}");
    let line = next_line(&f);
    assert!(line.contains("next line ≈ $0.0126 warm · $0.16 cold (warm now)"), "{line}");
    f.rig.clock.advance(6 * MIN);
    assert!(next_line(&f).contains("$0.0126 warm · $0.16 cold (cold now)"), "both prices, and the state marked: {}", next_line(&f));
}

#[test]
fn a_model_with_no_priced_turn_or_no_ratios_has_its_price_not_known_and_never_zero_dollars() {
    // a turn that cost nothing prices no token, though the provider's ratios are there
    let free = fx_planned(vec![Scripted::new("claude", vec![claude_turn("s1", 30_000, 12_000, 18_000, 0.15).into_iter().map(|b| if let Beat::Cached(_, a, c, w) = b { Beat::Cached(0.0, a, c, w) } else { b }).collect()])], |_| {});
    free.turn("one");
    let ctx = &free.rig.of_kind("task.settled")[0].data["ctx"];
    assert!(ctx.get("warm_usd").is_none() && ctx.get("cold_usd").is_none() && ctx["in"] == 30_000, "{ctx}");
    let line = next_line(&free);
    assert!(line.contains("price not known") && line.contains("30k in (warm now)") && !line.contains('$'), "{line}");
    // a provider whose cost source says no ratios
    let other = fx(vec![Scripted::new("alpha", vec![claude_turn("a", 30_000, 12_000, 18_000, 0.15)])], |_| {});
    other.turn("one");
    assert!(other.rig.of_kind("task.settled")[0].data["ctx"].get("warm_usd").is_none());
    let line = next_line(&other);
    assert!(line.contains("price not known") && line.contains("cost source has no `ratio`") && !line.contains('$'), "{line}");
}

#[test]
fn a_typical_small_line_is_added_to_what_the_session_holds_and_a_later_turn_that_does_not_say_its_last_call_does_not_keep_an_old_one() {
    let ratios = j4n_core::cost::Ratios { read: 0.1, write: 1.25, out: 5.0 };
    let price = j4n_core::task::context::Price { unit: 1e-5, ratios, line: (200.0, 100.0) };
    // 1,000 held: read 100 + line (200 in, 100 out as 500) = 800 tokens; written 1,250 + 700 = 1,950
    let (warm, cold) = price.next(1_000);
    assert!((warm - 0.008).abs() < 1e-12 && (cold - 0.0195).abs() < 1e-12, "{warm} {cold}");
    // `last` is the newest turn's: a metrics event that has none says 0 (the size falls back to what the turn read), not the one before it
    let mut feed = vec![queued(1, "t1", None)];
    feed.extend(turn_evs(2, "t1", 10_000, 80_000));
    feed.insert(3, ev(100, 10_500, "run.metrics", "t1", json!({ "usd": 0.1, "tokens_in": 90_000, "tokens_out": 4, "first": 29_000, "last": 5_000 })));
    feed.insert(4, ev(101, 10_600, "run.metrics", "t1", json!({ "usd": 0.1, "tokens_in": 70_000, "tokens_out": 4 })));
    let row = &rows(feed)[0];
    assert_eq!((row.last, row.first), (0, 29_000), "no stale `last`; the floor stays the first one said");
}

#[test]
fn a_turn_of_several_calls_holds_what_its_last_call_read_and_its_floor_is_what_its_first_read() {
    // the recorded read turn: three calls, 40,304 tokens added up for a window of 20,302 (frames_cases); then a second turn of five calls
    // that read 400,000 added up and ended at 41,000
    let turns = vec![
        vec![Beat::Session("s1".into()), Beat::Called(0.1, 40_304, 20_002, 20_302), Beat::Window(0.03), say("x"), end("x")],
        vec![Beat::Session("s1".into()), Beat::Called(0.3, 400_000, 30_400, 41_000), Beat::Window(0.04), say("y"), end("y")],
    ];
    let f = fx_planned(vec![Scripted::new("claude", turns)], |_| {});
    f.turn("one");
    f.turn("two");
    assert_eq!(settled_ctx(&f), vec![json!({ "in": 20_302, "floor": 20_002, "hist": 300, "cold": true }), json!({ "in": 41_000, "floor": 20_002, "hist": 20_998, "cold": false })]);
    let said = text(&f, "main");
    assert!(said.contains("41k held: floor 20k + history 21k"), "{said}");
    // what the run's metrics say, for the readers of the Log
    let m = &f.rig.of_kind("run.metrics")[1].data;
    assert_eq!((m["tokens_in"].clone(), m["first"].clone(), m["last"].clone()), (json!(400_000), json!(30_400), json!(41_000)));
}

#[test]
fn the_next_line_is_cold_only_once_the_last_turn_is_older_than_the_cache_life() {
    let f = fx_planned(vec![Scripted::new("claude", vec![claude_turn("s1", 108_000, 0, 108_000, 0.5)])], |_| {});
    f.turn("one");
    f.rig.clock.advance(5 * MIN);
    let at_the_edge = text(&f, "main");
    assert!(at_the_edge.contains("(warm now)") && at_the_edge.contains("last turn 5m ago, the cache lasts 5m"), "exactly the cache life is not older than it: {at_the_edge}");
    f.rig.clock.advance(1);
    let past = text(&f, "main");
    assert!(past.contains("(cold now)"), "{past}");
    let entries = f.front.context_value("main")["value"].clone();
    assert!(entries.as_array().unwrap().iter().any(|e| e["tone"] == "warn" && e["text"].as_str().is_some_and(|t| t.contains("(cold now)"))), "a cold line is a warning: {entries}");
}

#[test]
fn a_doer_whose_provider_has_no_cache_life_is_not_called_warm_or_cold() {
    let f = fx(vec![Scripted::new("alpha", vec![claude_turn("a", 50_000, 0, 50_000, 0.2), claude_turn("a", 70_000, 50_000, 20_000, 0.3)])], |_| {});
    f.turn("one");
    f.rig.clock.advance(60 * MIN);
    f.turn("two");
    f.rig.clock.advance(60 * MIN);
    let said = text(&f, "main");
    assert!(said.contains("70k held: floor 50k + history 20k"), "{said}");
    let line = next_line(&f);
    assert!(line.contains("warm or cold not known") && !line.contains("warm now") && !line.contains("cold now"), "{line}");
    // the turn says what it read, and cold only where it can be said: the first turn of a session always writes what it reads
    let ctx = settled_ctx(&f);
    assert_eq!((ctx[0]["cold"].clone(), ctx[1].get("cold").cloned(), ctx[1]["hist"].clone()), (json!(true), None, json!(20_000)));
}

#[test]
fn two_doers_in_one_chat_are_two_rows() {
    let claude = Scripted::new("claude", vec![claude_turn("s1", 30_000, 12_000, 18_000, 0.15)]);
    let beta = Scripted::new("beta", vec![vec![Beat::Session("b1".into()), Beat::Served("gpt-x", "gw"), Beat::Cached(0.02, 9_000, 0, 9_000), Beat::Window(0.07), say("y"), end("y")]]);
    let f = fx_planned(vec![claude, beta], |_| {});
    for doer in ["claude", "beta"] {
        let mut spec = TaskSpec::new("a line", doer);
        spec.thread = Some("main".into());
        let task = f.rig.tasks.spawn(spec, Actor::Core).unwrap();
        f.rig.finished(&task);
    }
    let said = text(&f, "main");
    let groups: Vec<&str> = said.lines().filter(|l| !l.starts_with(' ')).collect();
    assert_eq!(groups, vec!["main · beta · gpt-x", "main · claude · haiku 5.5"], "one group each, by doer: {said}");
    assert!(said.contains("30k held: floor 30k + history 0") && said.contains("9k held: floor 9k + history 0"), "{said}");
}

#[test]
fn a_chat_with_no_task_yet_has_an_empty_view_and_a_session_fresh_ended_is_not_held() {
    let f = fx_planned(vec![Scripted::new("claude", vec![claude_turn("s1", 30_000, 12_000, 18_000, 0.15)])], |_| {});
    let empty = f.front.context_value("main");
    assert_eq!(empty["field"], "meter");
    assert_eq!(plain(&empty["value"]), "main\n  nothing held yet: no line has run in this chat");
    // the command is answered with a row of its own, and starts no task
    assert_eq!(f.say("/context"), Said::Command("context".into()));
    assert_eq!((f.count("chat.view"), f.count("task.queued")), (1, 0));
    assert_eq!(f.rig.of_kind("chat.view")[0].data["view"], VIEW);
    f.turn("one");
    assert!(text(&f, "main").contains("30k held"));
    // another chat that has not run says so as well, and the chat that has run is still listed
    let other = text(&f, "side");
    assert!(other.starts_with("side\n  nothing held yet") && other.contains("main · claude"), "{other}");
    // `:fresh` ends the session: what it held is gone from the view
    assert_eq!(f.say(":fresh"), Said::Command("fresh".into()));
    assert_eq!(plain(&f.front.context_value("main")["value"]), "main\n  nothing held yet: no line has run in this chat");
}

// ---- the fold alone: a session whose first turn the Log does not hold ----

fn ev(seq: u64, at: u64, kind: &str, task: &str, data: Value) -> Ev {
    Ev { seq, at_ms: at, kind: kind.into(), task: Some(task.into()), thread: Some("main".into()), by: Actor::Core, cause: None, class: Default::default(), data }
}

fn rows(feed: Vec<Ev>) -> Vec<Row> {
    use j4n_core::fold::Fold;
    let mut book = Book::new();
    for e in &feed {
        if book.wants(&e.kind) {
            book.apply(e);
        }
    }
    book.rows()
}

fn queued(seq: u64, task: &str, resume: Option<&str>) -> Ev {
    let mut spec = TaskSpec::new("a line", "claude");
    spec.thread = Some("main".into());
    spec.resume = resume.map(|id| SessionRef { doer: "claude".into(), id: id.into() });
    ev(seq, 1_000 * seq, "task.queued", task, json!({ "spec": spec }))
}

fn turn_evs(seq: u64, task: &str, at: u64, tokens: u64) -> Vec<Ev> {
    vec![
        ev(seq, at, "task.started", task, json!({ "doer": "claude" })),
        ev(seq + 1, at, "run.session", task, json!({ "doer": "claude", "id": "old" })),
        ev(seq + 2, at, "run.metrics", task, json!({ "usd": 0.1, "tokens_in": tokens, "tokens_out": 4 })),
        ev(seq + 3, at + 5_000, "task.settled", task, json!({ "ok": true, "answer": "a", "window": 0.4 })),
    ]
}

#[test]
fn a_session_begun_before_the_log_has_no_floor_and_says_so_and_a_resumed_turn_is_not_a_first_one() {
    let mut feed = vec![queued(1, "t1", Some("old"))];
    feed.extend(turn_evs(2, "t1", 10_000, 80_000));
    feed.push(queued(6, "t2", Some("old")));
    feed.extend(turn_evs(7, "t2", 20_000, 90_000));
    let rows = rows(feed);
    let list = sessions(&rows);
    assert_eq!(list.len(), 1);
    assert_eq!((list[0].floor, list[0].history(), list[0].held, list[0].turns), (None, None, 90_000, 2));
    let text = plain(&context::view(&list, "main", |_| Some(5 * MIN), |_| None, 30_000));
    assert!(text.contains("90k held · floor not known: the Log has no first turn of this session"), "{text}");
    // the turn's own numbers: what it read, and no floor to take it from
    assert_eq!(turn(&rows, "t2", Some(5 * MIN), None), Some(json!({ "in": 90_000, "cold": false })));
    assert_eq!(turn(&rows, "t9", Some(5 * MIN), None), None);
}

#[test]
fn a_turn_that_read_nothing_has_no_numbers_and_the_first_turn_of_a_session_is_its_floor() {
    let mut feed = vec![queued(1, "t1", None)];
    feed.extend(turn_evs(2, "t1", 10_000, 26_000));
    feed.push(queued(6, "t2", Some("old")));
    feed.extend(turn_evs(7, "t2", 20_000, 0));
    let rows = rows(feed);
    assert_eq!(turn(&rows, "t1", None, None), Some(json!({ "in": 26_000, "floor": 26_000, "hist": 0, "cold": true })));
    assert_eq!(turn(&rows, "t2", None, None), None, "a turn that read nothing says nothing");
    // the live session is held at what its last turn that read said
    assert_eq!(sessions(&rows)[0].held, 26_000);
}

// ---- a turn that is not over is held against the hard cap ----

/// A second task of `claude`, capped at `soft` dollars, running `beats`, after a first turn that priced a token (`claude_turn`: $0.1 for 23,725 input tokens).
fn capped_second(soft: f64, beats: Vec<Beat>, price_first: bool) -> (crate::front_cases::Fx, Arc<Scripted>, String) {
    capped_on("claude-haiku-5-5", soft, beats, price_first)
}

/// [`capped_second`] with the second task served by `model`.
fn capped_on(model: &'static str, soft: f64, beats: Vec<Beat>, price_first: bool) -> (crate::front_cases::Fx, Arc<Scripted>, String) {
    let mut first = claude_turn("s1", 30_000, 12_000, 18_000, 0.15);
    if !price_first {
        first = vec![Beat::Session("s1".into()), Beat::Served("claude-haiku-5-5", "anthropic"), say("x"), end("x")];
    }
    let mut second = vec![Beat::Session("s2".into()), Beat::Served(model, "anthropic")];
    second.extend(beats);
    let claude = Scripted::new("claude", vec![first, second]);
    let f = fx_planned(vec![claude.clone()], |_| {});
    f.turn("one");
    let mut spec = TaskSpec::new("write it", "claude");
    spec.limits.usd_soft = Some(soft);
    let id = f.rig.tasks.spawn(spec, Actor::Operator).unwrap();
    (f, claude, id)
}

#[test]
fn a_turn_that_says_what_it_has_used_is_stopped_at_the_hard_cap_by_the_price_of_a_token_the_log_has_seen() {
    // a token is $0.1 / 23,725; 10,000 input-token equivalents are about $0.042, past 1.5 × $0.01
    let (f, claude, id) = capped_second(0.01, vec![Beat::Running(10_000.0), say("never"), Beat::Spend(0.5), end("never")], true);
    let row = f.rig.finished(&id);
    assert_eq!(row.state, State::Stopped { why: Why::HardCap }, "{}", f.rig.dump());
    let said = f.rig.of_kind("task.cap_estimated");
    assert_eq!(said.len(), 1, "{}", f.rig.dump());
    assert_eq!((said[0].task.as_deref(), said[0].data["soft"].clone()), (Some(id.as_str()), json!(0.01)));
    let estimated = said[0].data["estimated"].as_f64().unwrap();
    assert!((estimated - 0.0421).abs() < 0.0005, "{said:?}");
    assert!(claude.ctls().contains(&RunCtl::Kill), "the hard cap kills");
    assert_eq!(row.spent, 0, "nothing was reported: the stop came before the turn ended");
    assert!(f.rig.of_kind("run.message").iter().all(|e| e.data["text"] != json!("never")));
}

#[test]
fn an_estimate_under_the_hard_cap_lets_the_turn_go_on_and_is_replaced_by_the_reported_cost() {
    // $0.0084 estimated, then $0.008 reported (a turn of 2,000 tokens: the same price per token, the estimate is dropped), then about $0.005 more estimated: 0.013 is under 0.015 only if the first estimate was not counted twice
    let (f, _, id) = capped_second(0.01, vec![Beat::Running(2_000.0), Beat::Cached(0.008, 2_000, 800, 1_200), Beat::Running(1_000.0), say("done"), end("done")], true);
    let row = f.rig.finished(&id);
    assert_eq!(row.state, State::Done { ok: true }, "{}", f.rig.dump());
    assert!(f.rig.of_kind("task.cap_estimated").is_empty());
    assert_eq!(row.spent, 8_000);
}

#[test]
fn with_no_priced_turn_of_the_model_nothing_is_estimated_and_the_cap_lands_when_the_turn_ends() {
    let (f, _, id) = capped_second(0.01, vec![Beat::Running(1_000_000.0), Beat::Spend(0.02), say("late"), end("late")], false);
    let row = f.rig.finished(&id);
    assert!(f.rig.of_kind("task.cap_estimated").is_empty(), "no invented number");
    assert_eq!(row.spent, 20_000);
    assert_eq!(row.state, State::Stopped { why: Why::HardCap }, "the reported $0.02 is past the hard cap of $0.015, as it always was");
}

#[test]
fn a_model_no_turn_has_priced_is_estimated_from_another_model_of_the_doer_by_their_price_words() {
    // a chat on opus, the first of its model in the Log, ran to 3.5 × its cap: nothing was estimated until its turn's cost came
    let (f, claude, id) = capped_on("claude-opus-5-5", 0.01, vec![Beat::Running(10_000.0), say("never"), Beat::Spend(0.5), end("never")], true);
    let row = f.rig.finished(&id);
    assert_eq!(row.state, State::Stopped { why: Why::HardCap }, "{}", f.rig.dump());
    let said = f.rig.of_kind("task.cap_estimated");
    assert_eq!(said.len(), 1, "{}", f.rig.dump());
    // a haiku token is $0.1 / 23,725 and opus is five times it by `offload.prices`: 10,000 units are about $0.21
    let estimated = said[0].data["estimated"].as_f64().unwrap();
    assert!((estimated - 0.2107).abs() < 0.002, "{said:?}");
    assert!(claude.ctls().contains(&RunCtl::Kill) && row.spent == 0, "stopped before the turn ended");
}
