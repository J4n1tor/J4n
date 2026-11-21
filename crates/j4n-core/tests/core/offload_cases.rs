//! Offload. A light chore goes by rule to a cheaper doer, as a task under the turn that sent it, and comes back to the
//! main lane as a report; a failed test run is the only test run that does; the day's cap keeps a chore with the main lane; a busy window sends
//! it to the Pro account; nothing offloaded writes; a review asks first; the operator's pick is the main lane's doer; `max20` is a plan.

use crate::front_cases::{Fx, base, chatty, fx};
use crate::scripted::{Beat, Scripted, end, say};
use j4n_core::change::Actor;
use j4n_core::cost::Plans;
use j4n_core::doer::Caps;
use j4n_core::front::usage::{Credits, Left, Usage, plain, view};
use j4n_core::front::{Cfg, Said};
use j4n_core::log::Ev;
use j4n_core::schema::DataClass;
use std::sync::atomic::{AtomicUsize, Ordering};
use j4n_core::gate::{Action, Gate, ReadOnly, Verb, Verdict};
use j4n_core::gate::check::Gates;
use j4n_core::gate::grant::Grants;
use j4n_core::log::New;
use j4n_core::offload::{Facts, Offload, Route, route};
use j4n_core::clock::Clock;
use j4n_core::task::{Mode, Row};
use serde_json::json;
use std::sync::Arc;

const NOW_S: i64 = 4_000_000_000;

/// A doer that takes a Brief and answers each turn with the next text.
fn lane(id: &str, answers: &[&str]) -> Arc<Scripted> {
    let turns = answers.iter().map(|a| vec![Beat::Session(format!("{id}-s")), say(a), end(a)]).collect();
    Scripted::with_caps(id, Caps { live: true, resume: true, brief: true, ..Caps::default() }, turns)
}

fn report(answer: &str) -> String {
    json!({ "answer": answer, "found": [], "sure": "high" }).to_string()
}

fn offloading(c: &mut Cfg) {
    c.doer = Some("claude".into());
    c.offload = Offload::shipped();
}

/// A thread that has had its first turn (a chore is a task under a turn).
fn bound(f: &Fx) -> Row {
    f.turn("explain how the Gate decides")
}

fn chores(f: &Fx) -> Vec<Row> {
    f.tasks().into_iter().filter(|r| r.spec.role.as_deref() == Some("chore")).collect()
}

/// The Brief of the newest turn of the main lane.
fn brief(f: &Fx) -> String {
    let work: Vec<Row> = f.tasks().into_iter().filter(|r| r.spec.role.as_deref() == Some("work")).collect();
    work.last().and_then(|r| r.spec.input["brief"].as_str().map(String::from)).unwrap_or_default()
}

fn window(f: &Fx, doer: &str, account: &str, util: f64) {
    let mut data = json!({ "window": "five_hour", "utilization": util, "resets_at": NOW_S, "status": "allowed" });
    if !account.is_empty() {
        data["account"] = json!(account);
    }
    f.rig.log.append(New::of("run.window", Actor::Doer(doer.into()), data).task("t99").thread("other")).unwrap();
}

// ---- which line is a chore ----

#[test]
fn a_line_is_a_chore_by_its_first_words_and_never_when_it_asks_for_a_change() {
    let o = Offload::shipped();
    let id = |l: &str| o.take(l).map(|t| t.chore.id.clone());
    assert_eq!(id("find where the Gate decides"), Some("search".into()));
    assert_eq!(id("Where is the size law checked?"), Some("search".into()));
    assert_eq!(id("run the tests"), Some("tests".into()));
    assert_eq!(id("run the tests again please"), Some("tests".into()));
    assert_eq!(id("run the linter"), Some("lint".into()));
    assert_eq!(id("cargo build"), Some("build".into()));
    assert_eq!(id("summarize the log of the last run"), Some("digest".into()));
    assert_eq!(id("review my change"), Some("review".into()));
    // a line that asks for a change is the main lane's, however it starts
    assert_eq!(id("review my change and fix it"), None, "a review that is also a change is the main lane's");
    for line in ["find the bug in main.rs and fix it", "where is the Gate? rename it to Door", "find where the Gate is to change it", "run the tests and commit", "find where X is and add a test", "run the tests in src/parser", "build a todo app", "hello", ""] {
        assert_eq!(id(line), None, "{line:?}");
    }
    let long = format!("find where {}", "the gate ".repeat(20));
    assert_eq!(id(&long), None, "a long line says more than the rule can read");
    assert!(Offload { on: false, ..Offload::shipped() }.take("run the tests").is_none(), "the switch");
    assert!(Offload::default().take("run the tests").is_none(), "no pack, no chore");
    assert!(o.chores.iter().find(|c| c.id == "review").unwrap().asks() && !o.chores.iter().any(|c| c.id != "review" && c.asks()), "a review is the one that asks");
}

// ---- where a chore goes ----

fn facts<'a>(main: (&'a str, &'a str), ready: &'a [String], pro: &'a [String], window: &'a dyn Fn(&str, &str) -> Option<f64>, spent: f64, credits: &'a dyn Fn() -> Option<f64>) -> Facts<'a> {
    Facts { main, ready, pro, window, spent, main_model: "", credits }
}

#[test]
fn a_busy_main_window_sends_the_chore_to_the_pro_account_and_a_free_one_keeps_it_on_haiku() {
    let o = Offload::shipped();
    let ready: Vec<String> = ["claude", "own"].map(String::from).into();
    let pro = vec!["side".to_string()];
    let none = || None;
    let go = |w: &dyn Fn(&str, &str) -> Option<f64>, spent: f64, ready: &[String]| route(&o, &facts(("claude", "work"), ready, &pro, w, spent, &none), 0.15);
    // the main account's five-hour window has room: haiku on it, which no cap counts
    let free = |_: &str, _: &str| Some(0.3);
    assert!(matches!(go(&free, 0.0, &ready), Route::Go { ref doer, ref model, account: Some(ref a), paid: false, .. } if doer == "claude" && model == "haiku" && a == "work"));
    // it is busy (at 0.8): the Pro account's window has room
    let busy = |_: &str, a: &str| if a == "work" { Some(0.9) } else { Some(0.1) };
    assert!(matches!(go(&busy, 0.0, &ready), Route::Go { ref doer, account: Some(ref a), paid: false, .. } if doer == "claude" && a == "side"), "{:?}", go(&busy, 0.0, &ready));
    // both are busy: the gateway key, under the cap
    let both = |_: &str, _: &str| Some(0.95);
    assert!(matches!(go(&both, 0.5, &ready), Route::Go { ref doer, paid: true, .. } if doer == "own"));
    // over the cap, or with no key, it stays, and says why
    let Route::Stay { why } = go(&both, 1.95, &ready) else { panic!("over the cap it stays") };
    assert!(why.contains("$1.95 of $2.00"), "{why}");
    let Route::Stay { why } = go(&both, 0.0, &["claude".to_string()]) else { panic!("no key") };
    assert!(why.contains("no gateway key"), "{why}");
    // a reading that has no number is no busy window; claude missing leaves haiku and pro out
    let unknown = |_: &str, _: &str| None;
    assert!(matches!(go(&unknown, 0.0, &ready), Route::Go { ref doer, .. } if doer == "claude"));
    assert!(matches!(go(&free, 0.0, &["own".to_string()]), Route::Go { ref doer, .. } if doer == "own"));
}

/// A chore goes to a
/// second claude session only when the main model is dearer per token by `offload.worth` times.
#[test]
fn a_chore_goes_to_a_second_claude_session_only_where_the_main_model_is_dearer_by_the_worth() {
    let o = Offload::shipped();
    let ready: Vec<String> = ["claude"].map(String::from).into();
    let none = || None;
    let free = |_: &str, _: &str| Some(0.3);
    let go = |main_model: &str, o: &Offload| {
        let f = Facts { main: ("claude", ""), ready: &ready, pro: &[], window: &free, spent: 0.0, main_model, credits: &none };
        route(o, &f, 0.15)
    };
    assert!(matches!(go("sonnet", &o), Route::Go { ref doer, ref model, .. } if doer == "claude" && model == "haiku"), "sonnet is 3x haiku: it pays");
    assert!(matches!(go("claude-opus-5-5", &o), Route::Go { .. }), "a full model name holds the word");
    let Route::Stay { why } = go("haiku", &o) else { panic!("haiku on haiku never pays") };
    assert!(why.contains("haiku is 1.0× haiku per token") && why.contains("offload.worth"), "{why}");
    let Route::Stay { why } = go("claude-haiku-5-5", &o) else { panic!("by its full name too") };
    assert!(why.contains("offload.worth"), "{why}");
    // a main model with no price is sent as before, and a worth of 0 sends always
    assert!(matches!(go("", &o), Route::Go { .. }) && matches!(go("gpt-5", &o), Route::Go { .. }), "no price: as before");
    assert!(matches!(go("haiku", &Offload { worth: 0.0, ..Offload::shipped() }), Route::Go { .. }), "0: always");
    // the operator's own prices: sonnet at 2 is under a worth of 3
    let dear = Offload { prices: vec![("haiku".into(), 1.0), ("sonnet".into(), 2.0)], ..Offload::shipped() };
    assert!(matches!(go("sonnet", &dear), Route::Stay { .. }));
    // the pro account is not a way round it: the window is free, and the reason is the price
    let pro = vec!["side".to_string()];
    let f = Facts { main: ("claude", "work"), ready: &ready, pro: &pro, window: &free, spent: 0.0, main_model: "haiku", credits: &none };
    assert!(matches!(route(&o, &f, 0.15), Route::Stay { .. }));
}

#[test]
fn the_row_says_why_a_chore_stayed_with_a_main_model_that_is_not_dearer() {
    for (model, goes) in [("haiku", false), ("sonnet", true)] {
        let f = fx(vec![lane("claude", &["main", "main two"])], |c| {
            offloading(c);
            c.model = Some(model.into());
            c.offload.order = vec!["haiku".into()];
        });
        bound(&f);
        let Said::Started { task } = f.say("find where the cap is read") else { panic!() };
        let row = f.rig.finished(&task);
        assert_eq!(row.spec.role.as_deref() == Some("chore"), goes, "{model}: {:?}", row.spec.role);
        let stayed = f.rig.of_kind("offload.stayed");
        if goes {
            assert!(stayed.is_empty(), "{model}: nothing stayed");
        } else {
            assert_eq!(stayed.len(), 1, "{model}");
            assert!(stayed[0].data["why"].as_str().unwrap().contains("haiku is 1.0× haiku per token"), "{}", stayed[0].data);
        }
    }
}

#[test]
fn a_window_reading_in_the_log_is_what_busy_means_and_an_old_one_is_not() {
    let f = fx(vec![lane("claude", &["a"])], offloading);
    bound(&f);
    assert_eq!(f.front.window_now("claude", ""), None, "no reading, no window");
    window(&f, "claude", "work", 0.9);
    assert_eq!(f.front.window_now("claude", "work"), Some(0.9));
    assert_eq!(f.front.window_now("claude", ""), None, "another account's window is not this one's");
    f.rig.clock.advance(((NOW_S * 1000) as u64).saturating_sub(f.rig.clock.now_ms()) + 1000);
    assert_eq!(f.front.window_now("claude", "work"), None, "it has reset since: it says nothing of now");
}

#[test]
fn accounts_that_name_the_plan_pro_are_where_a_chore_may_run_and_max20_is_a_tier() {
    let (mut plans, problems) = Plans::load(&base());
    assert!(problems.is_empty(), "{problems:?}");
    assert!(plans.name([("work".to_string(), "max5".to_string()), ("side".to_string(), "pro".to_string()), ("big".to_string(), "max20".to_string())]).is_empty(), "max20 is a plan row now");
    assert_eq!(plans.accounts_on("pro"), ["side"]);
    let (max5, max20) = (plans.of("claude", Some("work")).unwrap(), plans.of("claude", Some("big")).unwrap());
    assert_eq!((max20.id.as_str(), max20.price), ("max20", Some(200.0)));
    assert_eq!(max20.window_ms("five_hour"), max5.window_ms("five_hour"), "the same windows as Max5");
    assert_eq!(max20.window_ms("seven_day"), max5.window_ms("seven_day"));
    assert!(max20.window_ms("five_hour").is_some());
}

// ---- the chore as a task ----

#[test]
fn a_search_goes_to_haiku_under_the_turn_that_sent_it_and_is_told_to_the_main_lane_once() {
    let claude = lane("claude", &["the Gate decides in gate/check.rs", &report("the Gate is in check.rs"), "ok, I read that"]);
    let f = fx(vec![claude], offloading);
    let main = bound(&f);
    let Said::Started { task } = f.say("find where the Gate asks about a read") else { panic!("{}", f.rig.dump()) };
    let row = f.rig.finished(&task);
    assert_eq!((row.spec.role.as_deref(), row.spec.parent.as_deref(), row.spec.doer.as_str()), (Some("chore"), Some(main.id.as_str()), "claude"), "a task under the turn");
    assert_eq!((row.spec.tune.model.as_deref(), row.spec.tune.mode), (Some("haiku"), Mode::Read), "the cheap model, and reading only");
    assert!(row.spec.goal.starts_with("find where the Gate asks about a read") && row.spec.goal.contains("Write nothing"), "{}", row.spec.goal);
    assert!(row.spec.input.get("branch").is_some(), "it is a branch to the loop: it starts none of its own");
    let routed = f.rig.of_kind("offload.routed");
    assert_eq!(routed.len(), 1);
    assert_eq!((routed[0].data["chore"].as_str(), routed[0].data["task"].as_str(), routed[0].data["under"].as_str()), (Some("search"), Some(task.as_str()), Some(main.id.as_str())));
    assert!(f.notes().iter().any(|n| n.contains("offload: search →") && n.contains(&format!("under {}", main.id)) && n.contains(":ask find where")), "{:?}", f.notes());
    // the main lane's next turn reads what it found, once
    f.turn("and what does it say about a read");
    assert!(brief(&f).contains("## branches") && brief(&f).contains("the Gate is in check.rs"), "{}", brief(&f));
    f.turn("thanks, one more");
    assert!(!brief(&f).contains("the Gate is in check.rs"), "told once");
    // `:ask` is the way back: the line goes to the main lane as it is
    let Said::Started { task } = f.say(":ask find where the Gate asks about a read") else { panic!() };
    assert_eq!(f.rig.finished(&task).spec.role.as_deref(), Some("work"));
}

#[test]
fn a_line_that_writes_or_a_thread_with_no_turn_or_a_task_that_works_is_the_main_lanes() {
    let f = fx(vec![lane("claude", &["a", "b", "c"])], offloading);
    // no turn yet: there is no turn to be under
    let Said::Started { task } = f.say("run the tests") else { panic!() };
    assert_eq!(f.rig.finished(&task).spec.role.as_deref(), Some("work"));
    let Said::Started { task } = f.say("find where the Gate is and fix its name") else { panic!() };
    assert_eq!(f.rig.finished(&task).spec.role.as_deref(), Some("work"), "it asks for a change");
    assert!(chores(&f).is_empty());
    let off = fx(vec![lane("claude", &["a", "b"])], |c| {
        offloading(c);
        c.offload.on = false;
    });
    bound(&off);
    let Said::Started { task } = off.say("find where the Gate is") else { panic!() };
    assert_eq!(off.rig.finished(&task).spec.role.as_deref(), Some("work"), "the switch is off");
}

#[test]
fn only_a_failed_test_run_comes_back_to_the_main_lane() {
    let check = Scripted::new("check", vec![vec![end("1-cargo: passed")], vec![Beat::Fail("1-cargo check failed (exit 101):\nthread 'x' panicked: boom\ntest result: FAILED".into())]]);
    let claude = lane("claude", &["main", "main again"]);
    let f = fx(vec![claude, check], offloading);
    bound(&f);
    // a pass: the task says so in one line, and nothing waits for the main lane
    let Said::Started { task } = f.say("run the tests") else { panic!("{}", f.rig.dump()) };
    let row = f.rig.finished(&task);
    assert_eq!((row.spec.doer.as_str(), row.spec.role.as_deref(), row.spec.tune.mode, row.spec.tune.model.clone()), ("check", Some("chore"), Mode::Read, None), "the project's own command, no model");
    assert_eq!(row.answer.as_deref(), Some("1-cargo: passed"));
    f.front.chore_collect("main");
    assert!(f.lanes.waiting("main").is_empty(), "a pass is not told");
    // a failure: its first line and the end of its output are
    let Said::Started { task } = f.say("run the tests") else { panic!() };
    f.rig.finished(&task);
    f.front.chore_collect("main");
    f.front.chore_collect("main");
    let waiting = f.lanes.waiting("main");
    assert_eq!(waiting.len(), 1, "told once, however often it is collected");
    assert!(waiting[0].1.text(&waiting[0].0).contains("exit 101") && waiting[0].1.text("t").contains("test result: FAILED"), "{:?}", waiting);
    f.turn("what failed?");
    assert!(brief(&f).contains("exit 101"), "the next brief has it: {}", brief(&f));
    assert!(f.lanes.waiting("main").is_empty());
}

#[test]
fn the_day_s_cap_keeps_a_chore_with_the_main_lane_and_says_so() {
    let own = Scripted::with_caps("own", Caps { live: true, resume: true, brief: true, ..Caps::default() }, vec![vec![Beat::Session("o".into()), Beat::Spend(0.09), say(&report("found")), end(&report("found"))]]);
    let f = fx(vec![lane("claude", &["main", "main two"]), own], |c| {
        offloading(c);
        c.offload.order = vec!["own".into()];
        c.offload.cap_usd = 0.20;
    });
    bound(&f);
    let Said::Started { task } = f.say("find where the cap is read") else { panic!() };
    let row = f.rig.finished(&task);
    assert_eq!((row.spec.role.as_deref(), row.spec.doer.as_str()), (Some("chore"), "own"));
    assert!(f.front.spent_today("chore") > 0.08 && f.front.spent_today("chore") < 0.10, "{}", f.front.spent_today("chore"));
    // $0.09 spent and a chore may take $0.15: past the $0.20 a day
    let Said::Started { task } = f.say("find where the cap is written") else { panic!() };
    assert_eq!(f.rig.finished(&task).spec.role.as_deref(), Some("work"), "it stays with the main lane");
    let stayed = f.rig.of_kind("offload.stayed");
    assert_eq!(stayed.len(), 1);
    assert!(stayed[0].data["why"].as_str().unwrap().contains("of $0.20 is spent today"), "{}", stayed[0].data);
    assert!(f.notes().iter().any(|n| n.contains("search stays with the main lane") && n.contains("spent today")), "{:?}", f.notes());
    assert_eq!(chores(&f).len(), 1);
}

#[test]
fn nothing_offloaded_writes_a_chore_is_read_only_and_the_gate_blocks_a_read_only_write() {
    let check = Scripted::new("check", vec![vec![end("passed")]]);
    let f = fx(vec![lane("claude", &["main", &report("a"), &report("b")]), check], offloading);
    bound(&f);
    f.front.say("main", "find where the Gate is");
    f.rig.finished(&chores(&f)[0].id.clone());
    f.say("run the tests");
    f.say(":offload yes");
    for id in chores(&f).iter().map(|r| r.id.clone()) {
        f.rig.finished(&id);
    }
    let gate = Gate::new(Gates::load(&base()).0, Grants::new("s1"));
    let all = chores(&f);
    assert!(all.len() >= 2, "{}", f.rig.dump());
    for r in &all {
        assert_eq!(r.spec.tune.mode, Mode::Read, "{} is read-only", r.id);
        assert_eq!(r.spec.place, "here", "and in no place of its own that could keep a change");
        assert!(r.spec.input.get("hold").is_none());
        // what a connector makes of that mode, and what the Gate says of it: a write, an apply and a line that writes are blocked
        let ro = if r.spec.tune.mode == Mode::Read { ReadOnly::Explicit } else { ReadOnly::No };
        for mut a in [Action::new(Verb::Write, Actor::Doer(r.spec.doer.clone()), "src/a.rs"), Action::new(Verb::Apply, Actor::Doer(r.spec.doer.clone()), "c1"), Action::shell(Actor::Doer(r.spec.doer.clone()), "touch x")] {
            a.read_only = ro;
            assert!(matches!(gate.check(&a), Verdict::Block { .. }), "{}: {a:?}", r.id);
        }
    }
}

#[test]
fn a_review_asks_first_and_goes_only_on_a_yes() {
    let claude = lane("claude", &["main", &report("one point"), "main two"]);
    let f = fx(vec![claude], offloading);
    let main = bound(&f);
    // the line is not sent: a note asks
    assert_eq!(f.say("review my change"), Said::Command("offload".into()));
    assert!(chores(&f).is_empty(), "nothing started before the yes");
    assert!(f.notes().iter().any(|n| n.contains("a review reads what") && n.contains("`yes` sends it")), "{:?}", f.notes());
    assert_eq!(f.rig.of_kind("offload.asked").len(), 1);
    let Said::Started { task } = f.say("yes") else { panic!("{}", f.rig.dump()) };
    let row = f.rig.finished(&task);
    assert_eq!((row.spec.role.as_deref(), row.spec.parent.as_deref(), row.spec.tune.mode), (Some("chore"), Some(main.id.as_str()), Mode::Read));
    assert!(row.spec.goal.contains("what is wrong with the change"), "{}", row.spec.goal);
    // anything but a yes keeps it with the main lane: the next line is a turn, and nothing waits
    assert_eq!(f.say("review my change"), Said::Command("offload".into()));
    let Said::Started { task } = f.say("never mind, tell me about the log") else { panic!() };
    assert_eq!(f.rig.finished(&task).spec.role.as_deref(), Some("work"));
    assert_eq!(chores(&f).len(), 1);
    assert!(f.notes().iter().any(|n| n.contains("stays with the main lane")), "{:?}", f.notes());
    // `:offload yes` with nothing waiting is no yes to anything
    assert!(matches!(f.say(":offload yes"), Said::Refused(_)));
}

// ---- the main doer is the operator's pick ----

#[test]
fn the_first_election_falls_to_the_operators_pick_when_it_is_ready_and_a_line_that_names_a_doer_is_the_later_word() {
    let doers = || vec![chatty("alpha", &["a", "a2"]), chatty("beta", &["b", "b2"])];
    // with no pick: the smallest id, as the numbers say nothing
    let f = fx(doers(), |_| {});
    f.turn("first");
    assert_eq!(f.front.binding("main").map(|b| (b.doer, b.how)), Some(("alpha".into(), "elected".into())));
    // the pick
    let f = fx(doers(), |c| c.main.doer = Some("beta".into()));
    f.turn("first");
    assert_eq!(f.front.binding("main").map(|b| (b.doer, b.how)), Some(("beta".into(), "picked".into())));
    // `:setup doer` says it while J4n runs, for a thread that is not bound yet
    // `:setup doer` writes the setting through a proposal, which the binary wires (laws/offload_flow.rs); with none here it refuses and picks nothing
    let f = fx(doers(), |_| {});
    assert!(matches!(f.say(":setup doer beta"), Said::Refused(w) if w.contains("proposals")));
    assert_eq!(f.front.main_pick(), None);
    // a pick that is not a doer is refused, and the line that names a doer wins over a pick
    assert!(matches!(f.say(":setup doer nobody"), Said::Refused(w) if w.contains("not ready for work")));
    assert_eq!(f.say(":setup doer"), Said::Command("setup".into()), "alone it says what the pick is, in the chat (the Setup page is gone)");
    let g = fx(doers(), |c| c.main.doer = Some("beta".into()));
    g.turn("@alpha for this");
    assert_eq!(g.front.binding("main").map(|b| b.doer), Some("alpha".into()));
    // a pick that is not ready: the election chooses, and says so
    let h = fx(vec![chatty("alpha", &["a"]), Scripted::probed("beta", true, false, vec![])], |c| c.main.doer = Some("beta".into()));
    h.turn("first");
    assert_eq!(h.front.binding("main").map(|b| b.doer), Some("alpha".into()));
    assert!(h.notes().iter().any(|n| n.contains("the doer you picked, beta, is not ready")), "{:?}", h.notes());
}

#[test]
fn the_setup_page_says_the_default_doer_is_the_pick_and_each_doers_start_time() {
    use j4n_core::doctor::{DoerFact, Facts, report};
    use j4n_core::doer::Probe;
    let probe = |installed| Some(Probe { installed, version: Some("1.0".into()), authed: true, detail: String::new() });
    let doer = |id: &str, ms| DoerFact { id: id.into(), probe: probe(true), work: true, ms, ..DoerFact::default() };
    let f = Facts { doers: vec![doer("claude", Some(85)), doer("codex", Some(40))], ..Facts::default() };
    let r = report(&f);
    assert_eq!(r.default.as_deref(), Some("claude"), "the smallest id when nobody picked");
    let row = |r: &j4n_core::doctor::Report, id: &str| r.rows.iter().find(|x| x.id == id).unwrap().clone();
    assert_eq!(row(&r, "claude").shown(), "found · signed in · 1.0 · 85 ms");
    // a doer that has to be mended says the mend: its time is not what starting it costs
    let broken = Facts { doers: vec![DoerFact { id: "codex".into(), probe: probe(false), work: true, ms: Some(12), fix: "install it".into(), ..DoerFact::default() }], ..Facts::default() };
    assert_eq!(report(&broken).rows[1].shown(), "not found");
    assert_eq!(row(&r, "default doer").fix, "Connect › Setup › Agent: ⏎ on one uses it for the main lane");
    let r = report(&Facts { pick: Some("codex".into()), ..f.clone() });
    assert_eq!(r.default.as_deref(), Some("codex"));
    assert_eq!(row(&r, "default doer").fix, "your pick · Connect › Setup › Agent: ⏎ on another uses it");
    assert!(r.text().contains("codex") && r.text().contains("40 ms"), "{}", r.text());
    let r = report(&Facts { pick: Some("gemini".into()), ..f });
    assert_eq!(r.default.as_deref(), Some("claude"), "a pick that is not ready is not the default");
    assert_eq!(row(&r, "default doer").fix, "your pick, gemini, is not ready · Connect › Setup › Agent › gemini says what mends it");
    // a doer that was not probed has no time to show
    assert_eq!(row(&r, "gemini").shown(), "not declared");
}

// ---- /usage: max20, what reports no window, and what a key has left ----

fn reading(doer: &str, account: &str, util: f64, at: u64) -> Ev {
    let mut data = json!({ "window": "five_hour", "utilization": util, "resets_at": (at / 1000 + 4 * 3600) as i64, "status": "allowed" });
    data["account"] = json!(account);
    Ev { seq: 0, at_ms: at, kind: "run.window".into(), task: None, thread: None, by: Actor::Doer(doer.into()), cause: None, class: DataClass::default(), data }
}

#[test]
fn a_max20_account_shows_its_windows_like_a_max5_one_with_its_own_price() {
    let (mut plans, _) = Plans::load(&base());
    plans.name([("big".to_string(), "max20".to_string()), ("work".to_string(), "max5".to_string())]);
    let (mut u, t0) = (Usage::default(), 1_800_000_000_000);
    for account in ["big", "work"] {
        u.count(&reading("claude", account, 0.25, t0 + 3_600_000));
    }
    let text = plain(&view(&u, &plans, &[], t0 + 2 * 3_600_000));
    assert!(text.contains("claude · big · max20 $200/mo") && text.contains("claude · work · max5 $100/mo"), "{text}");
    assert_eq!(text.matches("5h  [###-------]  25%").count(), 2, "the same bar for the same use: {text}");
}

#[test]
fn doers_that_report_no_window_are_one_row_and_a_window_they_do_report_is_a_bar() {
    let f = fx(vec![chatty("codex", &["a"]), chatty("gemini", &["b"]), chatty("claude", &["c"])], |_| {});
    let text = |f: &Fx| plain(&f.front.view_value("usage").unwrap()["value"]);
    assert!(text(&f).contains("codex · gemini\n  these doers report no window"), "{}", text(&f));
    assert!(!text(&f).contains("claude · gemini"), "claude has a plan with windows and is not in the row");
    // codex reports one: it is a bar of its own, and only gemini is left in the row
    f.rig.log.append(New::of("run.window", Actor::Doer("codex".into()), json!({ "window": "primary", "utilization": 0.4, "resets_at": 4_000_000_000_i64 })).task("t9").thread("other")).unwrap();
    f.front.usage_see(&f.rig.of_kind("run.window")[0]);
    let after = text(&f);
    assert!(after.contains("codex · default") && after.contains("40%") && after.contains("\ngemini\n  these doers report no window"), "{after}");
    // none of the doers declared: no row at all
    let none = fx(vec![chatty("claude", &["c"])], |_| {});
    assert!(!plain(&none.front.view_value("usage").unwrap()["value"]).contains("report no window"));
}

#[test]
fn a_key_is_asked_what_it_has_left_only_once_usage_was_asked_and_at_most_once_in_ten_minutes() {
    let calls = Arc::new(AtomicUsize::new(0));
    let credits = Credits::new({
        let calls = calls.clone();
        move || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(Left { remaining: Some(5.75), used: 4.25, limit: Some(10.0) })
        }
    });
    let f = fx(vec![chatty("own", &["a"])], |c| {
        c.gateways = vec![("own".into(), "openrouter".into())];
        c.credits = Some(credits.clone());
    });
    let said = |f: &Fx| f.rig.of_kind("chat.view").last().and_then(|e| e.data["text"].as_str().map(String::from)).unwrap_or_default();
    // drawn before `/usage` was said (a held row replayed at the start): no call
    assert!(plain(&f.front.view_value("usage").unwrap()["value"]).contains("credits: unknown (a gateway call, not made here)"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    f.say(":usage");
    assert!(said(&f).contains("credits: $5.75 left of $10.00"), "{}", said(&f));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // again, and the row drawn again by an event: the reading stands
    f.say(":usage");
    f.front.view_value("usage");
    assert_eq!(calls.load(Ordering::SeqCst), 1, "once in ten minutes");
    f.rig.clock.advance(9 * 60_000);
    f.say(":usage");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    f.rig.clock.advance(2 * 60_000);
    f.say(":usage");
    assert_eq!(calls.load(Ordering::SeqCst), 2, "ten minutes on, it is asked again");
    // a key with no limit says what it used; a failure says why and costs no second call within the ten minutes
    let open = fx(vec![chatty("own", &["a"])], |c| {
        c.gateways = vec![("own".into(), "openrouter".into())];
        c.credits = Some(Credits::new(|| Ok(Left { remaining: None, used: 1.5, limit: None })));
    });
    open.say(":usage");
    assert!(said(&open).contains("credits: no limit on this key · $1.50 used in all"), "{}", said(&open));
    let bad_calls = Arc::new(AtomicUsize::new(0));
    let bad = fx(vec![chatty("own", &["a"])], |c| {
        c.gateways = vec![("own".into(), "openrouter".into())];
        let n = bad_calls.clone();
        c.credits = Some(Credits::new(move || {
            n.fetch_add(1, Ordering::SeqCst);
            Err("the gateway answered 401 (the key is not accepted)".into())
        }));
    });
    bad.say(":usage");
    bad.say(":usage");
    assert!(said(&bad).contains("credits: unknown (the gateway answered 401") && !said(&bad).contains("sk-"), "{}", said(&bad));
    assert_eq!(bad_calls.load(Ordering::SeqCst), 1);
}

/// A doer whose pack is off is `off` in the first-start row, as `j4n doctor` says it, never `not found`; a doer that is
/// really missing still is. Before the fix the row said `own, light not found`.
#[test]
fn the_first_start_row_says_off_for_a_doer_whose_pack_is_off_and_not_found_for_a_missing_one() {
    use j4n_core::doctor::{DoerFact, Facts, report};
    use j4n_core::doer::Probe;
    let missing = Some(Probe { installed: false, version: None, authed: false, detail: String::new() });
    let facts = Facts {
        doers: vec![
            DoerFact { id: "own".into(), probe: None, ..DoerFact::default() },
            DoerFact { id: "light".into(), probe: None, ..DoerFact::default() },
            DoerFact { id: "pi".into(), probe: missing, work: true, ..DoerFact::default() },
        ],
        ..Facts::default()
    };
    let note = report(&facts).notice().expect("something is missing");
    assert!(note.contains("pi not found") && note.contains("own, light off") && !note.contains("own, light not found"), "{note}");
    assert!(note.ends_with("⏎ opens Connect › Setup"), "{note}");
}
