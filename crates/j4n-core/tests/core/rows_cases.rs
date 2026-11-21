//! The chat as rows. Events of a thread become turn › lane › step rows on `chat:<thread>`: what hangs from what, the mark
//! of each row, what merges, what folds, the stats, the output of a command, a question and its answer. The fold is pure, so the tests
//! feed it events as the Log would write them.

use j4n_core::change::Actor;
use j4n_core::front::rows::{self, Rows, kind_of, target_of};
use j4n_core::log::Ev;
use j4n_proto::{By, Fold, Row, Tone};
use serde_json::{Value, json};

struct Feed {
    seq: u64,
    at: u64,
    all: Vec<Ev>,
}

impl Feed {
    fn new() -> Feed {
        Feed { seq: 0, at: 12 * 3_600_000 + 29 * 60_000, all: Vec::new() }
    }

    fn ev(&mut self, kind: &str, task: Option<&str>, by: Actor, data: Value) -> &mut Feed {
        self.seq += 1;
        self.at += 1_000;
        self.all.push(Ev { seq: self.seq, at_ms: self.at, kind: kind.into(), task: task.map(Into::into), thread: Some("main".into()), by, cause: None, class: Default::default(), data });
        self
    }

    fn say(&mut self, text: &str) -> &mut Feed {
        self.ev("chat.say", None, Actor::Operator, json!({ "text": text }))
    }

    fn lane(&mut self, task: &str, doer: &str, model: &str) -> &mut Feed {
        self.ev("task.queued", Some(task), Actor::Core, json!({ "spec": { "goal": "g", "doer": doer, "place": "here", "tune": { "model": model } } }));
        self.ev("task.started", Some(task), Actor::Core, json!({ "doer": doer, "place": "here" }))
    }

    fn call(&mut self, task: &str, name: &str, detail: &str) -> &mut Feed {
        self.ev("run.tool_call", Some(task), Actor::Doer("x".into()), json!({ "name": name, "detail": detail }))
    }

    /// The rows with the zone pinned to UTC: no test here reads the machine's.
    fn rows(&self) -> Vec<Row> {
        self.rows_in(0)
    }

    fn rows_in(&self, zone: i64) -> Vec<Row> {
        let mut r = Rows::new();
        r.zone = Some(zone);
        self.all.iter().for_each(|ev| r.put(ev));
        r.rows()
    }
}

fn texts(r: &Row) -> Vec<String> {
    r.cells.iter().map(|c| c.text.clone()).collect()
}

fn icon(r: &Row) -> &str {
    r.cells[0].icon.as_str()
}

fn by_key<'a>(rows: &'a [Row], key: &str) -> &'a Row {
    rows.iter().find(|r| r.key == key).unwrap_or_else(|| panic!("no row {key}: {:?}", rows.iter().map(|r| &r.key).collect::<Vec<_>>()))
}

#[test]
fn a_turn_is_the_line_said_with_its_time_and_a_lane_hangs_from_it_with_its_letter_doer_model_time_and_money() {
    let mut f = Feed::new();
    f.say("fix the parser").lane("t1", "omp", "sonnet");
    f.ev("run.metrics", Some("t1"), Actor::Doer("omp".into()), json!({ "usd": 0.034, "tokens_in": 10, "tokens_out": 5 }));
    f.at += 20_000;
    f.ev("task.settled", Some("t1"), Actor::Core, json!({ "ok": true }));
    let rows = f.rows();
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!((rows[0].key.as_str(), rows[0].parent.clone(), texts(&rows[0])), ("s1", None, vec!["fix the parser".into(), "".into(), "12:29".into()]));
    let lane = &rows[1];
    assert_eq!((lane.key.as_str(), lane.parent.as_deref(), icon(lane), lane.by), ("t1", Some("s1"), "done", By::Core));
    assert_eq!(texts(lane), ["A omp", "sonnet", "22s", "$0.03"]);
}

#[test]
fn lanes_are_lettered_in_their_turn_and_the_letters_begin_again_in_the_next() {
    let mut f = Feed::new();
    f.say("one").lane("t1", "omp", "").lane("t2", "claude", "");
    f.say("two").lane("t3", "pi", "");
    let rows = f.rows();
    let lanes: Vec<(String, String)> = rows.iter().filter(|r| r.parent.is_some()).map(|r| (r.key.clone(), r.cells[0].text.clone())).collect();
    assert_eq!(lanes, [("t1".into(), "A omp".into()), ("t2".into(), "B claude".into()), ("t3".into(), "A pi".into())]);
    assert_eq!(by_key(&rows, "t3").parent.as_deref(), Some("s6"), "it hangs from the turn it began in");
}

#[test]
fn the_steps_of_a_lane_have_the_mark_of_their_kind_and_only_reads_merge() {
    let mut f = Feed::new();
    f.say("go").lane("t1", "claude", "sonnet");
    f.call("t1", "Read", r#"{"file_path":"/p/parser.rs"}"#).call("t1", "Read", r#"{"file_path":"/p/lexer.rs"}"#);
    f.call("t1", "Grep", r#"{"pattern":"trailing comma","path":"/p"}"#);
    f.call("t1", "Edit", r#"{"file_path":"/p/parser.rs","old_string":"if self.eat(Comma) { continue }","new_string":"if self.eat(Comma) && self.at(RPa"#);
    f.call("t1", "Write", "/p/new.rs").call("t1", "WebFetch", "https://example.org").call("t1", "mcp__thing", "does it");
    let rows = f.rows();
    let steps: Vec<(&str, String, bool)> = rows.iter().filter(|r| r.parent.as_deref() == Some("t1") && !r.key.starts_with("w:")).map(|r| (icon(r), r.cells[0].text.clone(), r.merge)).collect();
    assert_eq!(
        steps,
        [
            ("read", "/p/parser.rs".to_string(), true),
            ("read", "/p/lexer.rs".to_string(), true),
            ("search", "trailing comma".to_string(), false),
            ("edit", "/p/parser.rs".to_string(), false),
            ("new", "/p/new.rs".to_string(), false),
            ("fetch", "https://example.org".to_string(), false),
            ("tool", "mcp__thing does it".to_string(), false),
        ]
    );
}

#[test]
fn a_command_is_a_step_with_its_exit_and_its_output_as_detail_and_a_failed_call_says_so() {
    let mut f = Feed::new();
    f.say("go").lane("t1", "claude", "");
    f.ev("run.shell", Some("t1"), Actor::Doer("x".into()), json!({ "cmd": "cargo test", "out": "running 2 tests\ntest a ... ok\ntest b ... FAILED", "exit": 101 }));
    f.ev("run.shell", Some("t1"), Actor::Doer("x".into()), json!({ "cmd": "true", "out": "", "exit": 0 }));
    f.call("t1", "Read", r#"{"file_path":"/p/gone.rs"}"#);
    f.ev("run.tool_result", Some("t1"), Actor::Doer("x".into()), json!({ "name": "Read", "detail": "failed: no such file" }));
    let rows = f.rows();
    let (bad, good, read) = (by_key(&rows, "s4"), by_key(&rows, "s5"), by_key(&rows, "s6"));
    assert_eq!((icon(bad), bad.cells[0].text.as_str()), ("run", "cargo test"));
    assert_eq!((bad.cells[2].icon.as_str(), bad.cells[2].text.as_str(), bad.cells[2].tone), ("no", "101", Tone::Bad));
    assert_eq!(bad.detail.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["running 2 tests", "test a ... ok", "test b ... FAILED"]);
    assert_eq!((good.cells[2].icon.as_str(), good.detail.len()), ("done", 0));
    assert!(!read.merge && read.tone == Tone::Bad && read.detail[0].text.contains("no such file"), "{read:?}");
}

#[test]
fn the_words_of_a_doer_are_prose_steps_and_the_report_that_repeats_the_last_words_is_not_said_twice() {
    let mut f = Feed::new();
    f.say("go").lane("t1", "claude", "");
    f.ev("run.message", Some("t1"), Actor::Doer("x".into()), json!({ "role": "assistant", "text": "Trailing commas now parse." }));
    f.ev("run.message", Some("t1"), Actor::Doer("x".into()), json!({ "role": "assistant", "text": "Trailing commas now parse.", "final": true }));
    f.ev("run.reasoning", Some("t1"), Actor::Doer("x".into()), json!({ "text": "the lexer drops it" }));
    let rows = f.rows();
    let kids: Vec<(&str, String)> = rows.iter().filter(|r| r.parent.as_deref() == Some("t1") && !r.key.starts_with("w:")).map(|r| (icon(r), r.cells[0].text.clone())).collect();
    assert_eq!(kids, [("", "Trailing commas now parse.".to_string()), ("think", "the lexer drops it".to_string())]);
    assert!(rows.iter().filter(|r| r.parent.as_deref() == Some("t1")).all(|r| r.by == By::Doer));
}

#[test]
fn streamed_words_are_one_step_that_grows_and_the_settled_message_takes_its_place() {
    let mut f = Feed::new();
    f.say("go").lane("t1", "claude", "");
    let mut live = Rows::new();
    f.all.iter().for_each(|e| live.put(e));
    let delta = |t: &str| Ev { seq: 0, at_ms: 1, kind: "run.delta".into(), task: Some("t1".into()), thread: Some("main".into()), by: Actor::Core, cause: None, class: Default::default(), data: json!({ "text": t }) };
    live.put(&delta("It pri"));
    live.put(&delta("nts"));
    let prose = |rows: &[Row]| rows.iter().filter(|r| icon(r).is_empty() && r.parent.as_deref() == Some("t1")).map(|r| r.cells[0].text.clone()).collect::<Vec<_>>();
    assert_eq!(prose(&live.rows()), ["It prints"]);
    live.put(&Ev { seq: 9, kind: "run.message".into(), data: json!({ "role": "assistant", "text": "It prints a greeting." }), ..delta("") });
    assert_eq!(prose(&live.rows()), ["It prints a greeting."], "the same step");
    live.put(&delta("More"));
    assert_eq!(prose(&live.rows()), ["It prints a greeting.", "More"]);
}

#[test]
fn a_lane_that_failed_is_pinned_open_and_says_why_and_a_stopped_one_is_just_stopped() {
    let mut f = Feed::new();
    f.say("go").lane("t1", "pi", "").lane("t2", "omp", "");
    f.ev("task.stopped", Some("t1"), Actor::Core, json!({ "why": { "kind": "failed", "message": "the doer stopped without a report" } }));
    f.ev("task.stopped", Some("t2"), Actor::Operator, json!({ "why": { "kind": "aborted" } }));
    let rows = f.rows();
    let (t1, t2) = (by_key(&rows, "t1"), by_key(&rows, "t2"));
    assert_eq!((icon(t1), t1.fold), ("failed", Some(Fold::Pinned)));
    assert_eq!((icon(t2), t2.fold), ("stopped", None));
    let why: Vec<(&str, Tone)> = rows.iter().filter(|r| r.parent.as_deref() == Some("t1")).map(|r| (r.cells[0].text.as_str(), r.cells[0].tone)).collect();
    assert_eq!(why, [("failed: the doer stopped without a report", Tone::Bad)]);
}

#[test]
fn a_question_is_a_step_of_the_lane_that_asked_with_the_key_of_the_question_and_its_answer_stands_on_it() {
    let mut f = Feed::new();
    f.say("go").lane("t1", "claude", "");
    f.ev("task.queued", Some("t2"), Actor::Doer("claude".into()), json!({ "spec": { "goal": "commit to b-claude?", "doer": "human", "parent": "t1", "place": "none", "input": { "ask": { "question": "commit to b-claude?" } } } }));
    let open = f.rows();
    let ask = by_key(&open, "t2");
    assert_eq!((ask.parent.as_deref(), icon(ask), ask.cells[0].text.as_str()), (Some("t1"), "ask", "commit to b-claude?"));
    f.ev("task.settled", Some("t2"), Actor::Operator, json!({ "ok": true, "answer": "yes", "via": "human" }));
    f.ev("ask.answered", Some("t1"), Actor::Operator, json!({ "ask": "t2", "answer": "yes", "via": "human" }));
    let done = f.rows();
    let ask = by_key(&done, "t2");
    assert_eq!((icon(ask), ask.cells[1].text.as_str()), ("yes", "yes"));
    assert!(done.iter().all(|r| !r.cells[0].text.starts_with("you:")), "the answer is said once, on the question's own row");
    assert_eq!(done.iter().filter(|r| r.parent.is_some()).count(), 2, "the lane and the question; the question is no lane");
}

#[test]
fn what_the_core_says_with_no_lane_hangs_from_the_turn_and_a_thread_that_began_with_it_does_not_show_an_empty_turn() {
    let mut f = Feed::new();
    f.ev("thread.bound", None, Actor::Core, json!({ "doer": "claude", "how": "pinned" }));
    f.say("hello");
    f.ev("chat.note", None, Actor::Core, json!({ "text": "fresh start", "tone": "dim" }));
    let rows = f.rows();
    let shape: Vec<(String, Option<String>, String)> = rows.iter().map(|r| (r.key.clone(), r.parent.clone(), r.cells[0].text.clone())).collect();
    assert_eq!(shape.iter().filter(|(_, p, _)| p.is_none()).count(), 1, "{shape:?}");
    assert_eq!(rows[0].cells[0].text, "hello");
    // the binding is a mark the screen folds; the note is words the operator reads
    assert_eq!(rows.iter().filter(|r| r.parent.is_some()).map(|r| (r.parent.as_deref(), icon(r), r.by, r.cells[0].text.as_str())).collect::<Vec<_>>(), [(Some("s1"), "session", By::Core, "bound to claude (pinned)"), (Some("s1"), "", By::Core, "fresh start")]);
}

#[test]
fn an_event_folded_twice_is_folded_once_and_a_replay_is_what_the_live_fold_made() {
    let mut f = Feed::new();
    f.say("go").lane("t1", "claude", "sonnet").call("t1", "Read", "/a.rs").ev("run.message", Some("t1"), Actor::Doer("x".into()), json!({ "text": "done" }));
    let mut live = Rows::new();
    for e in &f.all {
        live.put(e);
    }
    for e in &f.all {
        live.put(e);
    }
    assert_eq!(live.rows(), f.rows());
}

#[test]
fn a_changed_file_that_no_edit_call_showed_is_an_edit_step_and_one_that_was_shown_is_not_shown_again() {
    let mut f = Feed::new();
    f.say("go").lane("t1", "claude", "");
    f.call("t1", "Edit", r#"{"file_path":"/p/a.rs"}"#);
    f.ev("run.changed", Some("t1"), Actor::Doer("x".into()), json!({ "paths": ["/p/a.rs", "/p/b.rs"], "more": 0 }));
    let rows = f.rows();
    let edits: Vec<String> = rows.iter().filter(|r| icon(r) == "edit" && !r.key.starts_with("w:")).map(|r| r.cells[0].text.clone()).collect();
    assert_eq!(edits, ["/p/a.rs", "/p/b.rs"]);
    let keys: std::collections::BTreeSet<&str> = rows.iter().map(|r| r.key.as_str()).collect();
    assert_eq!(keys.len(), rows.len(), "every row has a key of its own");
}

#[test]
fn only_the_last_rows_are_sent_a_turn_at_a_time_and_a_long_turn_keeps_its_line_and_its_last_steps() {
    let mut f = Feed::new();
    for i in 0..300 {
        f.say(&format!("line {i}")).ev("chat.note", None, Actor::Core, json!({ "text": format!("n{i}") }));
    }
    let rows = f.rows();
    assert!(rows.len() <= rows::SENT && rows[0].parent.is_none(), "{}", rows.len());
    assert_eq!(rows.last().unwrap().cells[0].text, "n299");
    let mut g = Feed::new();
    g.say("long");
    for i in 0..620 {
        g.ev("chat.note", None, Actor::Core, json!({ "text": format!("note {i}") }));
    }
    let rows = g.rows();
    assert_eq!(rows.len(), rows::SENT);
    assert!(rows[0].parent.is_none() && rows[1..].iter().all(|r| r.parent.as_deref() == Some("s1")));
    assert_eq!(rows.last().unwrap().cells[0].text, "note 619");
}

#[test]
fn the_kind_of_a_call_is_read_from_the_name_the_doer_gave_and_its_target_from_the_detail_whole_or_cut_short() {
    for (name, kind) in [("Read", "read"), ("read_file", "read"), ("Grep", "search"), ("Glob", "search"), ("MultiEdit", "edit"), ("edit_file", "edit"), ("Write", "new"), ("Bash", "run"), ("WebFetch", "fetch"), ("mcp__x", "tool"), ("", "tool")] {
        assert_eq!(kind_of(name), kind, "{name}");
    }
    assert_eq!(target_of(r#"{"file_path":"/a/b.rs"}"#), "/a/b.rs");
    assert_eq!(target_of(r#"{"file_path":"/a/b.rs","old_string":"xxxxxxxxxxxxxxxxxxxxxx"#), "/a/b.rs");
    assert_eq!(target_of(r#"{"pattern":"fn main","path":"/a"}"#), "fn main");
    assert_eq!(target_of("read the config"), "read the config");
    assert_eq!(target_of(r#"{"other":1}"#), r#"{"other":1}"#);
}

#[test]
fn the_time_of_a_lane_is_seconds_then_minutes_and_seconds_then_hours_and_its_money_has_a_digit_more_under_a_cent() {
    let lane_after = |ms: u64| {
        let mut f = Feed::new();
        f.say("go").lane("t1", "claude", "");
        // the event that settles it is a second later than the time it was given
        f.at += ms;
        f.ev("task.settled", Some("t1"), Actor::Core, json!({ "ok": true }));
        texts(&f.rows()[1])[2].clone()
    };
    assert_eq!([lane_after(58_000), lane_after(59_000), lane_after(63_000), lane_after(3_660_000)], ["59s", "1:00", "1:04", "1:01:01"]);
    let spent = |usd: f64| {
        let mut f = Feed::new();
        f.say("go").lane("t1", "claude", "");
        f.ev("run.metrics", Some("t1"), Actor::Core, json!({ "usd": usd }));
        texts(&f.rows()[1]).last().cloned().unwrap()
    };
    assert_eq!([spent(0.004), spent(0.034), spent(1.5)], ["$0.004", "$0.03", "$1.50"]);
}

#[test]
fn the_money_of_a_lane_is_the_sum_of_what_it_reported_and_its_time_runs_from_its_first_start() {
    let mut f = Feed::new();
    f.say("go").lane("t1", "claude", "");
    f.ev("run.metrics", Some("t1"), Actor::Core, json!({ "usd": 0.02 }));
    f.ev("run.metrics", Some("t1"), Actor::Core, json!({ "usd": 0.03 }));
    f.at += 9_000;
    f.ev("task.started", Some("t1"), Actor::Core, json!({ "doer": "claude", "resumed": true }));
    f.at += 10_000;
    f.ev("task.settled", Some("t1"), Actor::Core, json!({ "ok": true }));
    assert_eq!(texts(&f.rows()[1]), ["A claude", "model not known yet", "23s", "$0.05"]);
}

#[test]
fn a_lane_that_waits_for_an_answer_has_the_mark_of_a_question_and_goes_on_working_when_it_is_answered() {
    let mut f = Feed::new();
    f.say("go").lane("t1", "claude", "");
    f.ev("task.waiting", Some("t1"), Actor::Core, json!({ "ask": "t2" }));
    assert_eq!(icon(by_key(&f.rows(), "t1")), "ask");
    f.ev("task.resumed", Some("t1"), Actor::Core, json!({ "ask": "t2" }));
    assert_eq!(icon(by_key(&f.rows(), "t1")), "working");
}

#[test]
fn an_error_of_a_run_is_a_step_marked_as_one_and_a_notice_is_a_notice() {
    let mut f = Feed::new();
    f.say("go").lane("t1", "claude", "");
    f.ev("run.error", Some("t1"), Actor::Doer("x".into()), json!({ "text": "the model is overloaded" }));
    f.ev("run.notice", Some("t1"), Actor::Core, json!({ "text": "the session is resumed" }));
    let rows = f.rows();
    let marks: Vec<(&str, &str, Tone)> = rows.iter().filter(|r| r.parent.as_deref() == Some("t1")).map(|r| (icon(r), r.cells[0].text.as_str(), r.cells[0].tone)).collect();
    assert_eq!(marks, [("error", "the model is overloaded", Tone::Bad), ("notice", "the session is resumed", Tone::Dim)]);
}

#[test]
fn an_empty_delta_makes_no_step_and_the_output_of_a_command_keeps_its_last_two_hundred_lines() {
    let mut f = Feed::new();
    f.say("go").lane("t1", "claude", "");
    let mut live = Rows::new();
    f.all.iter().for_each(|e| live.put(e));
    let before = live.rows().len();
    live.put(&Ev { seq: 0, at_ms: 1, kind: "run.delta".into(), task: Some("t1".into()), thread: Some("main".into()), by: Actor::Core, cause: None, class: Default::default(), data: json!({ "text": "" }) });
    assert_eq!(live.rows().len(), before);
    let long: String = (0..250).map(|i| format!("line {i}\n")).collect();
    f.ev("run.shell", Some("t1"), Actor::Doer("x".into()), json!({ "cmd": "seq", "out": long, "exit": 0 }));
    let rows = f.rows();
    let shell = rows.iter().find(|r| icon(r) == "run").unwrap();
    assert_eq!((shell.detail.len(), shell.detail[0].text.as_str(), shell.detail[199].text.as_str()), (200, "line 50", "line 249"));
}

#[test]
fn a_notebook_edit_is_about_its_notebook_and_a_patch_is_an_edit() {
    assert_eq!(target_of(r#"{"notebook_path":"/n.ipynb","new_source":"x"}"#), "/n.ipynb");
    assert_eq!(kind_of("apply_patch"), "edit");
}

#[test]
fn a_lane_on_a_plan_says_what_it_took_of_the_plan_with_its_list_cost_beside_it_as_cost_show_says() {
    use j4n_core::cost::Show;
    let mut f = Feed::new();
    f.say("go").lane("t1", "claude", "");
    f.ev("run.metrics", Some("t1"), Actor::Core, json!({ "usd": 0.5, "plan_usd": 0.012, "source": "assumed" }));
    f.ev("run.metrics", Some("t1"), Actor::Core, json!({ "usd": 0.25, "plan_usd": 0.006, "source": "assumed" }));
    let said = |show: Show| texts(&rows::fold_with(show, f.all.iter()).rows()[1]).last().cloned().unwrap();
    assert_eq!([said(Show::Both), said(Show::Plan), said(Show::List)], ["$0.02 plan · $0.75 list", "$0.02 plan", "$0.75 list"]);
    // a lane with a request on no plan has the one number, and a mix of the two counts the list cost of the one without a plan
    let mut g = Feed::new();
    g.say("go").lane("t1", "claude", "");
    g.ev("run.metrics", Some("t1"), Actor::Core, json!({ "usd": 0.5 }));
    assert_eq!(texts(&rows::fold_with(Show::Plan, g.all.iter()).rows()[1]).last().unwrap(), "$0.50");
    g.ev("run.metrics", Some("t1"), Actor::Core, json!({ "usd": 0.25, "plan_usd": 0.006 }));
    assert_eq!(texts(&rows::fold_with(Show::Both, g.all.iter()).rows()[1]).last().unwrap(), "$0.51 plan · $0.75 list");
}

fn flash(kind: &str, task: &str, data: Value) -> Ev {
    Ev { seq: 0, at_ms: 1, kind: kind.into(), task: Some(task.into()), thread: Some("main".into()), by: Actor::Core, cause: None, class: Default::default(), data }
}

/// A call that has begun is the live row of its lane, which grows; the real call is the step, and the row goes when the lane stops working.
#[test]
fn a_call_being_written_is_the_live_row_of_its_lane_and_the_real_call_is_the_step() {
    let mut f = Feed::new();
    f.say("go").lane("t1", "claude", "");
    let mut live = Rows::new();
    f.all.iter().for_each(|e| live.put(e));
    let at = f.at;
    let kids = |rows: &[Row]| rows.iter().filter(|r| r.parent.as_deref() == Some("t1")).map(|r| (r.key.clone(), icon(r).to_string(), texts(r))).collect::<Vec<_>>();
    let put = |live: &mut Rows, ms: u64, kind: &str, data: Value| live.put(&Ev { at_ms: at + ms, ..flash(kind, "t1", data) });
    assert!(kids(&live.rows()).is_empty(), "a lane that has said nothing has no live row");
    put(&mut live, 2_000, "run.open", json!({ "id": "1", "name": "write", "target": "", "bytes": 0 }));
    assert_eq!(kids(&live.rows()), [("w:t1".to_string(), "new".to_string(), vec!["writing".to_string(), String::new(), String::new(), String::new(), "0s".to_string()])]);
    put(&mut live, 4_000, "run.open", json!({ "id": "1", "name": "write", "target": "chess.py", "bytes": 4200 }));
    put(&mut live, 5_000, "run.tokens", json!({ "out": 900 }));
    let row = kids(&live.rows()).remove(0);
    assert_eq!(row.2, ["writing chess.py", "", "4.2 kB", "900 tok", "3s"], "how long it has been at it: from the first word of the call");
    assert_eq!(live.rows().last().map(|r| r.key.as_str()), Some("w:t1"), "the last of the rows, which makes it the last step of its lane");
    assert_eq!(live.rows().last().and_then(|r| r.data.clone()), Some(json!({ "at_ms": at + 2_000 })), "the screen counts the time from its own clock");
    // the call as it ran is the step; the row still says what is going on
    live.put(&Ev { seq: 99, at_ms: at + 6_000, ..flash("run.tool_call", "t1", json!({ "name": "write", "detail": "chess.py" })) });
    let steps: Vec<_> = kids(&live.rows()).into_iter().map(|(k, i, t)| (k.starts_with("w:"), i, t[0].clone())).collect();
    assert_eq!(steps, [(false, "new".to_string(), "chess.py".to_string()), (true, "new".to_string(), "writing chess.py".to_string())]);
    // a call that ends leaves the lane working and says what it did last; thinking and words are what is going on after it
    live.put(&Ev { seq: 100, at_ms: at + 7_000, ..flash("run.tool_result", "t1", json!({ "name": "write", "detail": "ok" })) });
    assert_eq!(texts(live.rows().last().unwrap())[..2], ["working", "after chess.py"]);
    put(&mut live, 8_000, "run.reasoning_delta", json!({ "text": "hm" }));
    assert_eq!(texts(live.rows().last().unwrap())[0], "thinking");
    put(&mut live, 9_000, "run.delta", json!({ "text": "Done." }));
    assert_eq!(texts(live.rows().last().unwrap())[0], "replying");
    // a command is `running` and what it runs; a call of a tool with no verb says its own words
    put(&mut live, 10_000, "run.open", json!({ "id": "2", "name": "Bash", "target": "python3 chess.py", "bytes": 40 }));
    assert_eq!(texts(live.rows().last().unwrap())[0], "running python3 chess.py");
    put(&mut live, 11_000, "run.open", json!({ "id": "3", "name": "Agent", "target": "", "bytes": 40 }));
    assert_eq!(texts(live.rows().last().unwrap())[0], "Agent");
    // the lane that is over, or waits for an answer, has no live row
    live.put(&Ev { seq: 101, at_ms: at + 12_000, ..flash("task.waiting", "t1", json!({ "ask": "a1" })) });
    assert!(live.rows().iter().all(|r| !r.key.starts_with("w:")));
    live.put(&Ev { seq: 102, at_ms: at + 13_000, ..flash("task.resumed", "t1", json!({ "ask": "a1" })) });
    assert!(live.rows().iter().any(|r| r.key == "w:t1"));
    live.put(&Ev { seq: 103, at_ms: at + 14_000, ..flash("task.settled", "t1", json!({ "ok": true })) });
    assert!(live.rows().iter().all(|r| !r.key.starts_with("w:")), "the row goes with the turn");
    // a call nobody opened is a step of its own, and a lane that is over has no open step to be taken over by a call of a later run
    live.put(&Ev { seq: 104, ..flash("run.tool_call", "t1", json!({ "name": "read", "detail": "d.py" })) });
    let rows = live.rows();
    assert!(rows.iter().any(|r| r.parent.as_deref() == Some("t1") && r.cells[0].text == "d.py") && rows.iter().all(|r| !r.key.starts_with("w:")));
}

/// A doer that says nothing of what it writes still has a live row once it says anything: what it did last and how long ago it began.
#[test]
fn a_doer_with_no_progress_events_has_a_live_row_that_says_its_last_step() {
    let mut f = Feed::new();
    f.say("go").lane("t1", "pi", "");
    f.call("t1", "Bash", "cargo test");
    let rows = f.rows();
    assert_eq!(texts(rows.iter().find(|r| r.key == "w:t1").expect("a live row"))[..2], ["running cargo test", ""]);
    f.ev("run.shell", Some("t1"), Actor::Doer("x".into()), json!({ "cmd": "cargo test", "out": "ok", "exit": 0 }));
    f.at += 90_000;
    f.ev("run.message", Some("t1"), Actor::Doer("x".into()), json!({ "role": "assistant", "text": "Running the tests now" }));
    let rows = f.rows();
    let live = rows.iter().find(|r| r.key == "w:t1").expect("a live row");
    assert_eq!(texts(live)[..2], ["working", "after Running the tests now"]);
    assert_eq!(live.data, Some(json!({ "at_ms": f.at - 91_000 })), "since the call ended, which the screen counts from its own clock");
}

/// A lane of a doer that runs code names no model, not even that it is not known yet; the flow's own lane and the lane of its code step are
/// marked so that the screen draws the two as one.
#[test]
fn a_lane_of_code_names_no_model_and_a_flows_lane_and_its_code_step_are_marked() {
    let mut f = Feed::new();
    f.say(":account check x");
    f.ev("task.queued", Some("t1"), Actor::Operator, json!({ "code": true, "spec": { "goal": "check x", "doer": "flow:account", "place": "here", "tune": {} } }));
    f.ev("task.queued", Some("t2"), Actor::Doer("flow:account".into()), json!({ "code": true, "spec": { "goal": "check x", "doer": "account", "parent": "t1", "place": "here", "tune": {} } }));
    f.lane("t3", "claude", "");
    let rows = f.rows();
    assert_eq!(texts(by_key(&rows, "t1")), ["A flow:account", ""]);
    assert_eq!(texts(by_key(&rows, "t2"))[..2], ["B account", ""]);
    assert_eq!(by_key(&rows, "t1").data, Some(json!({ "lane": { "doer": "flow:account", "own": false, "flow": true } })));
    assert_eq!(by_key(&rows, "t2").data, Some(json!({ "lane": { "doer": "account", "own": false, "code": true } })));
    // a doer that runs a model and has none known says so, as it did
    assert_eq!(texts(by_key(&rows, "t3"))[..2], ["C claude", "model not known yet"]);
    assert_eq!(by_key(&rows, "t3").data, Some(json!({ "lane": { "doer": "claude", "own": false } })));
}

#[test]
fn a_lane_that_ended_well_but_did_not_run_is_a_warn_mark_and_the_words_not_run_never_a_tick() {
    let mut f = Feed::new();
    f.say("go").lane("t1", "claude", "").lane("t2", "check", "");
    f.ev("task.settled", Some("t1"), Actor::Core, json!({ "ok": true, "answer": "done" }));
    f.ev("task.settled", Some("t2"), Actor::Core, json!({ "ok": true, "answer": "4-pytest-project: not run", "not_run": "its output says `no module named pytest`" }));
    let rows = f.rows();
    let (a, b) = (by_key(&rows, "t1"), by_key(&rows, "t2"));
    assert_eq!(icon(a), "done", "a lane that did its work is a tick");
    assert_eq!((icon(b), texts(b)[2].as_str(), b.cells[2].tone), ("risky", "not run", Tone::Warn), "{b:?}");
    assert_eq!(b.data.as_ref().unwrap()["lane"]["not_run"], "its output says `no module named pytest`", "the field a screen reads");
    assert!(a.data.as_ref().unwrap()["lane"].get("not_run").is_none());
}

/// A running lane says its clock and the tokens its model has written; a lane that is over does not.
#[test]
fn a_running_lane_shows_its_clock_and_its_tokens() {
    let mut f = Feed::new();
    f.say("go").lane("t1", "pi", "");
    let mut live = Rows::new();
    f.all.iter().for_each(|e| live.put(e));
    let lane = |rows: &[Row]| texts(rows.iter().find(|r| r.key == "t1").unwrap());
    live.put(&Ev { at_ms: f.at + 3_000, ..flash("run.tokens", "t1", json!({ "out": 1234 })) });
    assert_eq!(lane(&live.rows()), ["A pi", "model not known yet", "3s", "1.2k tok"]);
    live.put(&Ev { seq: 98, ..flash("run.metrics", "t1", json!({ "usd": 0.0, "tokens_in": 5, "tokens_out": 1300 })) });
    live.put(&Ev { at_ms: f.at + 4_000, ..flash("run.open", "t1", json!({ "id": "0", "name": "write", "target": "", "bytes": 0 })) });
    assert_eq!(lane(&live.rows()).last().map(String::as_str), Some("1.3k tok"), "what settled, and nothing is going on until the next count");
    live.put(&Ev { at_ms: f.at + 4_000, ..flash("run.tokens", "t1", json!({ "out": 100 })) });
    assert_eq!(lane(&live.rows()).last().map(String::as_str), Some("1.4k tok"), "what settled and what is going on");
    live.put(&Ev { seq: 101, at_ms: f.at + 5_000, ..flash("task.settled", "t1", json!({ "ok": true })) });
    assert!(lane(&live.rows()).iter().all(|c| !c.ends_with("tok")));
}

/// The doer's own list is one step of its lane, said again in place and folded to `N done` when every item is closed;
/// a lane that has no list has no such step, and two lanes have a step each.
#[test]
fn the_doers_list_is_one_quiet_step_updated_in_place() {
    let list = |f: &mut Feed, task: &str, open: u64, total: u64, active: &str| {
        f.ev("run.list", Some(task), Actor::Doer("claude".into()), json!({ "open": open, "total": total, "active": active }));
    };
    let mut f = Feed::new();
    f.say("go").lane("t1", "claude", "").lane("t2", "pi", "");
    f.call("t1", "Read", "{\"file_path\":\"a.rs\"}");
    list(&mut f, "t1", 5, 5, "");
    f.call("t1", "Edit", "{\"file_path\":\"a.rs\"}");
    list(&mut f, "t1", 3, 5, "writing tests");
    f.call("t2", "read", "b.rs");
    let steps = |rows: &[Row], lane: &str| -> Vec<(String, String)> { rows.iter().filter(|r| r.parent.as_deref() == Some(lane) && !r.key.starts_with("w:")).map(|r| (r.cells[0].icon.clone(), r.cells[0].text.clone())).collect() };
    let rows = f.rows();
    assert_eq!(steps(&rows, "t1"), [("read".into(), "a.rs".into()), ("todo".into(), "3 of 5 left · writing tests".into()), ("edit".into(), "a.rs".into())], "one row, where the list first appeared");
    assert!(steps(&rows, "t2").iter().all(|(icon, _)| !icon.starts_with("todo")), "a doer with no list has no such row");
    let row = rows.iter().find(|r| r.cells[0].icon == "todo").unwrap();
    assert_eq!((row.cells[0].tone, row.merge, row.by), (Tone::Dim, false, By::Doer));
    // without the item at work the words are the count alone; with every item closed the row folds to one short line
    list(&mut f, "t1", 1, 5, "");
    assert_eq!(steps(&f.rows(), "t1")[1], ("todo".to_string(), "1 of 5 left".to_string()));
    list(&mut f, "t1", 0, 5, "");
    assert_eq!(steps(&f.rows(), "t1")[1], ("todo-done".to_string(), "5 done".to_string()));
    // the chat read back from the Log is the chat that was on the screen
    let mut live = Rows::new();
    f.all.iter().for_each(|e| live.put(e));
    assert_eq!(live.rows(), f.rows());
    // a long active item is clipped to a line
    list(&mut f, "t2", 1, 2, &"w ".repeat(300));
    let long = f.rows().into_iter().find(|r| r.parent.as_deref() == Some("t2") && r.cells[0].icon == "todo").unwrap();
    assert!(long.cells[0].text.chars().count() < 230, "{}", long.cells[0].text.len());
}

#[test]
fn a_lane_says_the_pin_until_the_model_that_served_is_known_and_then_that() {
    let mut f = Feed::new();
    f.say("go").lane("t1", "claude", "haiku");
    assert_eq!(texts(&f.rows()[1])[..2], ["A claude", "haiku"]);
    f.ev("run.served", Some("t1"), Actor::Doer("claude".into()), json!({ "model": "claude-haiku-5-5", "host": "anthropic" }));
    assert_eq!(texts(&f.rows()[1])[..2], ["A claude", "haiku 5.5"], "what ran, not what was asked for");
}

// ---- who is named once ----

#[test]
fn a_lane_on_the_threads_own_doer_says_the_step_it_is_for_and_one_on_another_doer_names_it() {
    let mut f = Feed::new();
    f.say("go");
    f.ev("thread.bound", None, Actor::Core, json!({ "doer": "claude", "model": "haiku", "how": "pinned" }));
    // the thread's own doer and pin: the composer's strip says them, so the lane says the letter and what it is for
    f.ev("task.queued", Some("t1"), Actor::Core, json!({ "spec": { "goal": "g", "doer": "claude", "role": "work", "place": "here", "tune": { "model": "haiku" } } }));
    f.ev("task.started", Some("t1"), Actor::Core, json!({ "doer": "claude", "place": "here" }));
    f.ev("task.queued", Some("t2"), Actor::Core, json!({ "spec": { "goal": "g", "doer": "claude", "place": "here", "tune": { "model": "haiku" } } }));
    // another model of the same doer is told apart by the model; another doer by its name
    f.ev("task.queued", Some("t3"), Actor::Core, json!({ "spec": { "goal": "g", "doer": "claude", "place": "here", "tune": { "model": "opus" } } }));
    f.lane("t4", "omp", "sonnet");
    let rows = f.rows();
    let lane = |key: &str| texts(by_key(&rows, key));
    assert_eq!(&lane("t1")[..2], ["A work", ""], "{rows:#?}");
    assert_eq!(&lane("t2")[..2], ["B", ""]);
    assert_eq!(&lane("t3")[..2], ["C", "opus"]);
    assert_eq!(&lane("t4")[..2], ["D omp", "sonnet"]);
    // without a binding the lane names its doer, as it always did
    let mut g = Feed::new();
    g.say("go");
    g.lane("t1", "claude", "haiku");
    assert_eq!(&texts(by_key(&g.rows(), "t1"))[..2], ["A claude", "haiku"]);
}

#[test]
fn a_lane_row_says_whose_doer_it_is_and_whether_that_is_the_threads_own_for_a_screen_that_leaves_a_lone_lane_out() {
    // `data.lane` is what tells a lane from any other row at the top of a turn
    let mut f = Feed::new();
    f.say("go");
    f.ev("thread.bound", None, Actor::Core, json!({ "doer": "claude", "model": "haiku", "how": "pinned" }));
    f.ev("task.queued", Some("t1"), Actor::Core, json!({ "spec": { "goal": "g", "doer": "claude", "role": "work", "place": "here", "tune": { "model": "haiku" } } }));
    f.lane("t2", "omp", "sonnet");
    let rows = f.rows();
    assert_eq!(by_key(&rows, "t1").data, Some(json!({ "lane": { "doer": "claude", "own": true } })));
    assert_eq!(by_key(&rows, "t2").data, Some(json!({ "lane": { "doer": "omp", "own": false } })));
    assert!(rows.iter().filter(|r| r.data.as_ref().is_some_and(|d| d.get("lane").is_some())).count() == 2, "only lanes: {rows:#?}");
    // without a binding the lane is nobody's own
    let mut g = Feed::new();
    g.say("go");
    g.lane("t1", "claude", "haiku");
    assert_eq!(by_key(&g.rows(), "t1").data, Some(json!({ "lane": { "doer": "claude", "own": false } })));
}

#[test]
fn a_notice_the_doer_sends_is_a_row_of_the_doer_on_its_lane_and_the_core_s_own_is_a_row_of_the_core() {
    let mut f = Feed::new();
    f.say("go").lane("t1", "claude", "haiku");
    f.ev("run.notice", Some("t1"), Actor::Doer("claude".into()), json!({ "text": "claude · 92% of the 5-hour limit used · resets 14:20" }));
    f.ev("run.notice", Some("t1"), Actor::Core, json!({ "text": "the session was not shrunk: x" }));
    let rows = f.rows();
    let doer = rows.iter().find(|r| r.cells[0].text.starts_with("claude · 92%")).expect("the doer's notice");
    assert_eq!((icon(doer), doer.by, doer.parent.as_deref()), ("notice", By::Doer, Some("t1")), "the screen lifts this one out of the tree after the turn");
    let core = rows.iter().find(|r| r.cells[0].text.starts_with("the session was not shrunk")).expect("the core's notice");
    assert_eq!((icon(core), core.by), ("notice", By::Core));
}

#[test]
fn a_notice_a_rule_says_of_a_task_stays_in_its_lane_where_it_was_said() {
    // the quiet notice was lifted after the turn, below the operator's answer and later rows, as if the doer had said it
    let mut f = Feed::new();
    f.say("go").lane("t1", "claude", "haiku");
    f.ev("run.notice", Some("t1"), Actor::Rule("quiet".into()), json!({ "text": "Nothing has been heard from this task for 5 minutes.", "rule": "quiet" }));
    let rows = f.rows();
    let n = rows.iter().find(|r| r.cells[0].text.starts_with("Nothing has been heard")).expect("the rule's notice");
    assert_eq!((icon(n), n.by, n.parent.as_deref()), ("notice", By::Core, Some("t1")), "a row of J4n's own under the task's lane, not lifted");
}

#[test]
fn a_card_made_as_rows_hangs_from_its_head_by_depth_so_the_pipes_are_the_screens() {
    // the result ledger's card was one note with `├─` and `╰─` written in its text
    let mut f = Feed::new();
    f.say("go");
    f.ev("chat.note", None, Actor::Core, json!({ "text": "card", "tone": "plain", "tree": [[0, "◆ ledger · main"], [1, "± 0 files"], [1, "❯ #1 “go”"], [2, "t1 answered"], [2, "t2 answered"], [1, "◇ needs you"]] }));
    let rows = f.rows();
    let at = |t: &str| rows.iter().find(|r| r.cells[0].text == t).unwrap_or_else(|| panic!("no row {t}"));
    let (head, files, line, a1, a2, needs) = (at("◆ ledger · main"), at("± 0 files"), at("❯ #1 “go”"), at("t1 answered"), at("t2 answered"), at("◇ needs you"));
    assert_eq!(head.parent.as_deref(), Some(rows[0].key.as_str()), "the head is the turn's");
    for kid in [files, line, needs] {
        assert_eq!(kid.parent.as_deref(), Some(head.key.as_str()), "{}", kid.cells[0].text);
    }
    assert_eq!((a1.parent.as_deref(), a2.parent.as_deref()), (Some(line.key.as_str()), Some(line.key.as_str())), "the answers hang from their line");
    assert!(rows.iter().all(|r| !r.cells[0].text.contains('├') && !r.cells[0].text.contains('╰')), "no pipes in the words");
}

/// A row that ends in a time carries the event's time (`data.at_ms`, no new type) for the screen to say how long ago it was, and its last cell stays the
/// time of day for a reader that has no clock.
#[test]
fn a_turn_carries_the_time_of_its_event_and_keeps_the_time_of_day() {
    let mut f = Feed::new();
    f.say("fix the parser");
    let rows = f.rows_in(0);
    let t = &rows[0];
    assert_eq!(t.data.as_ref().and_then(|d| d["at_ms"].as_u64()), Some(f.at), "{t:?}");
    assert_eq!(t.cells.last().map(|c| c.text.as_str()), Some("12:29"), "the plain text is the time of day");
}

/// The time at the right of a turn is the operator's clock, the zone's offset moves it; the offset is handed in, the process zone is not touched.
#[test]
fn the_time_of_a_turn_is_on_the_clock_of_the_zone_not_utc() {
    let mut f = Feed::new();
    f.say("fix the parser");
    let at = |zone: i64| texts(&f.rows_in(zone)[0])[2].clone();
    // the turn was said at 12:29 UTC
    let hour = 3_600;
    assert_eq!(at(0), "12:29");
    assert_eq!(at(2 * hour), "14:29");
    assert_eq!(at(-8 * hour), "04:29");
    assert_eq!(at(5 * hour + 30 * 60), "17:59");
    // across midnight both ways
    assert_eq!(at(12 * hour), "00:29");
    assert_eq!(at(-13 * hour), "23:29");
    // the machine's zone, when none is pinned, is what `clock::offset_at` says at that instant
    let mut live = Rows::new();
    f.all.iter().for_each(|ev| live.put(ev));
    let off = j4n_core::clock::offset_at((12 * 3_600 + 29 * 60) as i64);
    let m = (12 * 3_600 + 29 * 60 + off).rem_euclid(86_400) / 60;
    assert_eq!(texts(&live.rows()[0])[2], format!("{:02}:{:02}", m / 60, m % 60));
}
