//! Reflexes, behind `reflexes.on`, off by default. The loop, the front, the Log and the ask point `reflex` are the real
//! ones, the doer is scripted (a claude session that can shrink), and the judge is the fixture's decider, which answers `fire` or `hold` as the test says: no live model.
//! What is asserted is what the Log says (`rule.fired`, `reflex.checked`, `reflex.asked`, `reflex.probed`), what ran, and what the judge was put.

use crate::front_cases::{Fx, fx_reflexes_again, fx_reflexes_in};
use crate::scripted::{Beat, Scripted, end, say};
use j4n_core::answer::{Answerer, Offer, Paid, Question};
use j4n_core::doer::Caps;
use j4n_core::front::screen::Screen;
use j4n_core::front::session::Life;
use j4n_core::front::reflexes::{NO_CHECK, Reflexes, reflex_rule};
use j4n_core::rule::RuleRow;
use j4n_core::front::{Cfg, Said};
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use j4n_core::task::State;
use serde_json::Value;
use std::sync::Arc;

fn turn(session: &str, all: u64, answer: &str) -> Vec<Beat> {
    vec![Beat::Session(session.into()), Beat::Served("claude-haiku-5-5", "anthropic"), Beat::Cached(0.1, all, 0, all), Beat::Window(0.3), say(answer), end(answer)]
}

fn slow(session: &str, all: u64, answer: &str) -> Vec<Beat> {
    vec![Beat::Session(session.into()), Beat::Served("claude-haiku-5-5", "anthropic"), Beat::Cached(0.1, all, 0, all), Beat::Pause(800), say(answer), end(answer)]
}

fn shrink(session: &str, after: u64) -> Vec<Beat> {
    vec![Beat::Session(session.into()), Beat::Called(0.05, 110_000, 0, after), Beat::Window(0.15), say("kept"), end("kept")]
}

/// The ways `:compact` shrinks, as the base pack says them.
const MODES: &str = "[[compact_mode]]\nid = \"plain\"\n[[compact_mode]]\nid = \"junk\"\nwords = \"Drop what nothing reads again.\"\n[[compact_mode]]\nid = \"keep\"\nwords = \"Keep: {words}\"\ntakes = true\n";

fn screen(rows: &str) -> Screen {
    let mut screen = Screen::builtin();
    let (tables, problems) = Tables::load(vec![Layer::new(LayerKind::Pack, "reflexes", vec![
        SourceFile { path: "modes.toml".into(), text: format!("api = 1\n{MODES}") },
        SourceFile { path: "reflexes/t.toml".into(), text: format!("api = 1\n{rows}") },
    ])]);
    assert!(problems.is_empty(), "{problems:?}");
    screen.tables = tables;
    screen
}

/// A reflex (a rule that asks) that runs `do` when `check` holds, with a short cooldown.
fn row(id: &str, run: &str, check: &str, event: &str) -> String {
    format!("[[rule]]\nid = \"{id}\"\non = \"{event}\"\nwhen = \"{check}\"\nthen = \"ask\"\ntext = \"the context holds more than a twentieth of junk\"\ndo = \"{run}\"\nlimit = {{ per = \"thread\", max = 1, within = \"1s\" }}\n")
}

fn rig(turns: Vec<Vec<Beat>>, pick: &'static str, rows: &str, reflexes: Reflexes) -> (Fx, Arc<Scripted>) {
    rig_in(turns, pick, rows, reflexes, false)
}

fn rig_in(turns: Vec<Vec<Beat>>, pick: &'static str, rows: &str, reflexes: Reflexes, headless: bool) -> (Fx, Arc<Scripted>) {
    let claude = Scripted::with_caps("claude", Caps { live: true, resume: true, compact: true, ..Caps::default() }, turns);
    let f = fx_reflexes_in(vec![claude.clone()], Some(pick), screen(rows), headless, move |c: &mut Cfg| {
        c.doer = Some("claude".into());
        c.session = Life::Resume;
        c.reflexes = reflexes;
    });
    (f, claude)
}

/// [`rig`] once more, after a restart: the Log is the one of `old`, and the front's memory starts empty.
fn rig_again(old: &Fx, turns: Vec<Vec<Beat>>, pick: &'static str, rows: &str, reflexes: Reflexes) -> Fx {
    let claude = Scripted::with_caps("claude", Caps { live: true, resume: true, compact: true, ..Caps::default() }, turns);
    fx_reflexes_again(old, vec![claude], Some(pick), screen(rows), move |c: &mut Cfg| {
        c.doer = Some("claude".into());
        c.session = Life::Resume;
        c.reflexes = reflexes;
    })
}

fn on(trust: usize) -> Reflexes {
    Reflexes { on: true, judge: true, cap_usd: 0.25, trust }
}

fn events(f: &Fx, kind: &str) -> Vec<Value> {
    f.rig.of_kind(kind).into_iter().map(|e| e.data).collect()
}

/// How often the judge was put the point `reflex`.
fn judged(f: &Fx) -> usize {
    f.asked.lock().unwrap().iter().filter(|q| q.point == "reflex").count()
}

fn args(words: &str) -> Vec<String> {
    words.split_whitespace().map(String::from).collect()
}

fn compacts(f: &Fx) -> Vec<j4n_core::task::Row> {
    f.tasks().into_iter().filter(|r| r.spec.role.as_deref() == Some("compact")).collect()
}

const SEC: u64 = 1_000;

#[test]
fn a_reflex_with_no_check_is_refused_whether_it_is_said_or_a_row() {
    let (f, _) = rig(vec![turn("s1", 108_000, "one")], "reflex=fire", "[[rule]]\nid = \"nocheck\"\non = \"task.settled\"\nthen = \"ask\"\ntext = \"always\"\ndo = \":context\"\nlimit = { per = \"thread\", max = 1, within = \"1s\" }\n", on(5));
    // a row with no check is no reflex: it is a problem, and it is never asked about
    let (rows, problems) = f.front.reflex_rows();
    assert!(rows.is_empty() && problems.iter().any(|p| p.contains("nocheck") && p.contains(NO_CHECK)), "{rows:?} {problems:?}");
    f.turn("one");
    assert!(f.front.reflexes_at("main", "task.settled", None).is_empty());
    assert_eq!(judged(&f), 0, "a judge on every turn is what the check is for");
    // and one said in words is refused the same way
    let said = f.front.reflex_said("main", &args("add junk do :compact junk when too much junk"));
    assert!(matches!(&said, Said::Refused(why) if why.contains("needs a condition")), "{said:?}");
    // a check that does not read is refused, and so is a command that would widen what J4n may do
    for (line, why) in [
        ("add a do :context when x check ctx.grew >=", "`when`"),
        ("add b do :set cost.plan.on false when x check ctx.held > 0", "never runs anything that widens"),
        ("add c do :context when x check nothing.at.all > 0", "`when`"),
    ] {
        let said = f.front.reflex_said("main", &args(line));
        assert!(matches!(&said, Said::Refused(m) if m.contains(why)), "{line}: {said:?}");
    }
    assert!(f.front.reflex_rows().0.is_empty());
}

#[test]
fn a_check_that_holds_and_does_not_fire_writes_reflex_checked_with_the_judges_reason() {
    let (f, claude) = rig(vec![turn("s1", 108_000, "one"), turn("s1", 120_000, "two")], "reflex=hold", &row("junk", ":context", "ctx.grew >= 0.01", "task.settled"), on(5));
    f.turn("one");
    // the first look takes the size as it is: nothing has grown, the check does not hold, the judge is not asked
    assert!(f.front.reflexes_at("main", "task.settled", None).is_empty());
    assert_eq!((judged(&f), f.count("reflex.checked")), (0, 0));
    f.turn("two");
    assert!(f.front.reflexes_at("main", "task.settled", None).is_empty(), "the judge said hold");
    assert_eq!(judged(&f), 1);
    let checked = events(&f, "reflex.checked");
    assert_eq!(checked.len(), 1, "{checked:?}");
    assert_eq!((checked[0]["reflex"].clone(), checked[0]["fire"].clone(), checked[0]["by"].clone(), checked[0]["why"].clone()), ("junk".into(), false.into(), "decider:jev".into(), "it knows".into()), "{}", f.rig.dump());
    assert_eq!(f.count("rule.fired"), 0);
    assert_eq!(claude.starts().len(), 2, "nothing ran");
    // the question the judge was put is the digest code made: the numbers and the sentence, and the heads of the last turns, never the transcript
    let q = f.asked.lock().unwrap().iter().find(|q| q.point == "reflex").cloned().unwrap();
    assert!(q.text.contains("reflex: junk") && q.text.contains("when: the context holds more than a twentieth of junk") && q.text.contains("held: 120k"), "{}", q.text);
    assert!(q.text.contains("- you: two → two"), "{}", q.text);
}

#[test]
fn a_firing_writes_rule_fired_and_runs_the_command_only_at_a_settled_boundary() {
    let (f, _) = rig(vec![turn("s1", 108_000, "one"), slow("s1", 108_000, "two")], "reflex=fire", &row("view", ":context", "ctx.held > 0", "task.settled"), on(5));
    f.turn("one");
    // a task runs: not a boundary, nothing is checked, asked or written
    let Said::Started { task } = f.say("two") else { panic!("{}", f.rig.dump()) };
    assert!(f.front.reflexes_at("main", "task.settled", None).is_empty());
    assert_eq!((judged(&f), f.count("reflex.checked"), f.count("rule.fired"), f.count("chat.view")), (0, 0, 0, 0), "{}", f.rig.dump());
    f.rig.finished(&task);
    // the boundary: the judge says fire and the command runs, and the Log says why
    assert_eq!(f.front.reflexes_at("main", "task.settled", None), vec!["view".to_string()]);
    let fired = events(&f, "rule.fired");
    assert_eq!(fired.len(), 1, "{}", f.rig.dump());
    assert_eq!((fired[0]["rule"].clone(), fired[0]["then"].clone(), fired[0]["do"].clone(), fired[0]["by"].clone(), fired[0]["why"].clone()), ("reflex:view".into(), "reflex".into(), ":context".into(), "decider:jev".into(), "it knows".into()));
    assert_eq!(f.count("chat.view"), 1, "`:context` ran");
    assert_eq!(f.count("reflex.checked"), 0, "a firing is rule.fired, not a check that did not fire");
}

#[test]
fn compact_asks_the_operator_the_first_times_and_stops_when_its_calls_have_held() {
    let turns = vec![turn("s1", 108_000, "one"), shrink("s1", 31_000), turn("s1", 31_000, "two"), shrink("s1", 20_000), turn("s1", 20_000, "three"), shrink("s1", 10_000)];
    let (f, claude) = rig(turns, "reflex=fire", &row("squeeze", ":compact junk", "ctx.held > 0", "task.settled"), on(2));
    f.turn("one");
    // the first time: asked, and nothing was shrunk
    assert_eq!(f.front.reflexes_at("main", "task.settled", None), vec!["squeeze".to_string()]);
    assert_eq!((f.count("reflex.asked"), f.count("rule.fired"), compacts(&f).len()), (1, 0, 0), "{}", f.rig.dump());
    assert!(f.notes().iter().any(|n| n.contains("`yes` runs `:compact junk`") && n.contains("0 so far")), "{:?}", f.notes());
    // while it waits nothing else is asked
    assert!(f.front.reflexes_at("main", "task.settled", None).is_empty());
    assert_eq!(judged(&f), 1);
    // yes: the call held, the command runs
    assert_eq!(f.say("yes"), Said::Command("reflex".into()));
    assert_eq!((f.count("rule.fired"), compacts(&f).len()), (1, 1), "{}", f.rig.dump());
    f.rig.finished(&compacts(&f)[0].id);
    assert!(claude.starts()[1].compacting);
    let labels = events(&f, "label");
    assert_eq!((labels.len(), labels[0]["label"].clone(), labels[0]["point"].clone(), labels[0]["producer"].clone()), (1, "held".into(), "reflex".into(), "decider:jev".into()));
    // the second time it asks again (one call has held, two must)
    f.turn("two");
    f.rig.clock.advance(2 * SEC);
    f.front.reflexes_at("main", "task.settled", None);
    assert_eq!(f.count("reflex.asked"), 2);
    assert_eq!(f.say("yes"), Said::Command("reflex".into()));
    f.rig.finished(&compacts(&f)[1].id);
    // two calls have held, none was overridden: the third runs at once, with no question
    f.turn("three");
    f.rig.clock.advance(2 * SEC);
    assert_eq!(f.front.reflexes_at("main", "task.settled", None), vec!["squeeze".to_string()]);
    assert_eq!((f.count("reflex.asked"), f.count("rule.fired"), compacts(&f).len()), (2, 3, 3), "{}", f.rig.dump());
    f.rig.finished(&compacts(&f)[2].id);
    assert!(f.tasks().iter().filter(|r| r.spec.role.as_deref() == Some("compact")).all(|r| r.state == State::Done { ok: true }));
}

#[test]
fn a_no_is_a_call_overridden_so_the_reflex_keeps_asking() {
    let turns = vec![turn("s1", 108_000, "one"), turn("s1", 108_000, "two"), shrink("s1", 31_000)];
    let (f, _) = rig(turns, "reflex=fire", &row("squeeze", ":compact junk", "ctx.held > 0", "task.settled"), on(1));
    f.turn("one");
    f.front.reflexes_at("main", "task.settled", None);
    assert_eq!(f.say("no"), Said::Command("reflex".into()));
    assert_eq!((f.count("rule.fired"), compacts(&f).len()), (0, 0), "nothing ran");
    assert_eq!(events(&f, "label")[0]["label"], "overridden");
    f.turn("two");
    f.rig.clock.advance(2 * SEC);
    f.front.reflexes_at("main", "task.settled", None);
    assert_eq!(f.count("reflex.asked"), 2, "one call was overridden: it asks, though one held would have been enough");
}

#[test]
fn the_cap_stops_the_judge() {
    let rows = row("view", ":context", "ctx.held > 0", "task.settled");
    // a cap of nothing: the judge is never put the digest
    let (f, _) = rig(vec![turn("s1", 108_000, "one")], "reflex=fire", &rows, Reflexes { cap_usd: 0.0, ..on(5) });
    f.turn("one");
    assert!(f.front.reflexes_at("main", "task.settled", None).is_empty());
    assert_eq!(judged(&f), 0);
    assert!(events(&f, "reflex.checked")[0]["why"].as_str().unwrap().contains("cap"), "{}", f.rig.dump());
    // a call that costs what the judge says it costs: the cap is spent after two, and the third is not asked
    let (f, _) = rig(vec![turn("s1", 108_000, "one")], "reflex=fire", &rows, Reflexes { cap_usd: 0.03, ..on(5) });
    *f.pays.lock().unwrap() = Some(Paid { usd: Some(0.02), tokens_in: 10, tokens_out: 1 });
    f.turn("one");
    assert_eq!(f.front.reflexes_at("main", "task.settled", None), vec!["view".to_string()]);
    f.rig.clock.advance(2 * SEC);
    assert!(f.front.reflexes_at("main", "task.settled", None).is_empty(), "the second call spent the cap: it does not fire");
    f.rig.clock.advance(2 * SEC);
    assert!(f.front.reflexes_at("main", "task.settled", None).is_empty());
    assert_eq!(judged(&f), 2, "the third was not asked");
    assert!(events(&f, "reflex.checked").last().unwrap()["why"].as_str().unwrap().contains("cap"));
}

#[test]
fn the_days_spend_of_the_judge_is_still_counted_after_a_restart() {
    // the cap of 3 cents is spent by two calls of 2 cents; a new front over the same Log does not start the day again at nothing
    let rows = row("view", ":context", "ctx.held > 0", "task.settled");
    let cap = Reflexes { cap_usd: 0.03, ..on(5) };
    let (f, _) = rig(vec![turn("s1", 108_000, "one")], "reflex=fire", &rows, cap.clone());
    *f.pays.lock().unwrap() = Some(Paid { usd: Some(0.02), tokens_in: 10, tokens_out: 1 });
    f.turn("one");
    f.front.reflexes_at("main", "task.settled", None);
    f.rig.clock.advance(2 * SEC);
    f.front.reflexes_at("main", "task.settled", None);
    assert_eq!(judged(&f), 2);
    assert!((f.front.spent_today("reflex") - 0.04).abs() < 1e-9, "{}", f.front.spent_today("reflex"));
    let g = rig_again(&f, vec![turn("s1", 108_000, "two")], "reflex=fire", &rows, cap.clone());
    assert!((g.front.spent_today("reflex") - 0.04).abs() < 1e-9, "the Log has it: {}", g.front.spent_today("reflex"));
    g.rig.clock.advance(2 * SEC);
    g.turn("two");
    assert!(g.front.reflexes_at("main", "task.settled", None).is_empty());
    assert_eq!(judged(&g), 0, "the cap was spent before the restart: the judge is not asked");
    assert!(events(&g, "reflex.checked").last().unwrap()["why"].as_str().unwrap().contains("cap"), "{}", g.rig.dump());
    // tomorrow it is asked again, and a day's spend of another ask point is not the judge's
    g.rig.clock.advance(25 * 3600 * SEC);
    assert_eq!(g.front.spent_today("reflex"), 0.0);
    g.front.reflexes_at("main", "task.settled", None);
    assert_eq!(judged(&g), 1);
}

#[test]
fn after_a_restart_a_reserve_is_still_made_over_what_the_log_says_was_spent() {
    // 2 cents of the 3 are spent before the restart; the call that is in flight has reserved the cent that is left, so a second check meanwhile is stopped
    let rows = row("a", ":context", "true", "task.settled") + &row("b", ":context", "true", "task.settled");
    let cap = Reflexes { cap_usd: 0.03, ..on(5) };
    let (f, _) = rig(vec![turn("s1", 108_000, "one")], "reflex=fire", &rows, cap.clone());
    *f.pays.lock().unwrap() = Some(Paid { usd: Some(0.02), tokens_in: 10, tokens_out: 1 });
    f.turn("one");
    assert_eq!(f.front.reflexes_at("main", "task.settled", None), vec!["a".to_string()]);
    let g = rig_again(&f, vec![turn("s1", 108_000, "two")], "reflex=fire", &rows, cap);
    g.turn("two");
    let front = g.front.clone();
    *g.hook.lock().unwrap() = Some(Box::new(move || drop(front.reflexes_at("main", "task.settled", None))));
    g.front.reflexes_at("main", "task.settled", None);
    assert_eq!(judged(&g), 1, "the second call was not made: {}", g.rig.dump());
}

#[test]
fn a_reflex_whose_judge_fails_its_cases_does_not_fire() {
    let rows = row("view", ":context", "ctx.held > 0", "task.settled");
    let (f, _) = rig(vec![turn("s1", 108_000, "one")], "reflex=fire", &rows, on(5));
    f.turn("one");
    // the judge always says fire: the case that says it should hold is one it misses
    assert!(matches!(f.front.reflex_said("main", &args("case view fire the junk is three long logs nothing read again")), Said::Command(_)));
    assert!(matches!(f.front.reflex_said("main", &args("case view hold the chat is one design, nothing is junk")), Said::Command(_)));
    assert!(f.front.reflexes_at("main", "task.settled", None).is_empty());
    let probed = events(&f, "reflex.probed");
    assert_eq!((probed.len(), probed[0]["passed"].clone(), probed[0]["cases"].clone()), (1, false.into(), 2.into()), "{probed:?}");
    assert!(probed[0]["missed"][0].as_str().unwrap().contains("should hold"), "{probed:?}");
    assert_eq!((f.count("rule.fired"), f.count("chat.view")), (0, 0), "it did not fire");
    assert!(events(&f, "reflex.checked")[0]["why"].as_str().unwrap().contains("missed 1"));
    // a reflex whose cases all say fire is one this judge gets right
    let (g, _) = rig(vec![turn("s1", 108_000, "one")], "reflex=fire", &rows, on(5));
    g.turn("one");
    g.front.reflex_said("main", &args("case view fire the junk is three long logs nothing read again"));
    assert_eq!(g.front.reflexes_at("main", "task.settled", None), vec!["view".to_string()]);
    assert_eq!(events(&g, "reflex.probed")[0]["passed"], true);
    assert_eq!(g.count("rule.fired"), 1);
}

#[test]
fn nothing_fires_with_the_switch_off() {
    let rows = row("view", ":context", "ctx.held > 0", "task.settled") + &row("fresh", ":fresh", "true", "chat.say");
    let (f, _) = rig(vec![turn("s1", 108_000, "one"), turn("s1", 120_000, "two")], "reflex=fire", &rows, Reflexes { on: false, ..on(5) });
    f.turn("one");
    f.turn("two");
    assert!(f.front.reflexes_at("main", "task.settled", None).is_empty());
    assert!(f.front.reflexes_at("main", "chat.say", Some("anything")).is_empty());
    assert_eq!(judged(&f), 0, "nobody is asked");
    for kind in ["reflex.checked", "reflex.asked", "reflex.probed", "rule.fired", "thread.fresh"] {
        assert_eq!(f.count(kind), 0, "{kind}");
    }
    // the words still work, and say it is off
    f.front.reflex_said("main", &[]);
    assert!(f.notes().iter().any(|n| n.contains("the reflexes are off") && n.contains("view on")), "{:?}", f.notes());
}

#[test]
fn the_judge_is_not_sent_the_project_without_its_consent() {
    let (f, _) = rig(vec![turn("s1", 108_000, "one")], "reflex=fire", &row("view", ":context", "ctx.held > 0", "task.settled"), Reflexes { judge: false, ..on(5) });
    f.turn("one");
    assert!(f.front.reflexes_at("main", "task.settled", None).is_empty());
    assert_eq!(judged(&f), 0);
    let why = events(&f, "reflex.checked")[0]["why"].as_str().unwrap().to_string();
    assert!(why.contains("reflexes.judge") && why.contains("nothing was sent"), "{why}");
}

#[test]
fn a_reflex_that_checks_a_line_asks_at_a_boundary_when_the_next_line_is_cold() {
    let (f, _) = rig(vec![turn("s1", 108_000, "one")], "reflex=fire", &row("new-session", ":fresh", "ctx.cold", "chat.say"), on(5));
    f.turn("one");
    f.rig.clock.advance(6 * 60 * SEC);
    // `:fresh` throws the session away: the operator is asked
    assert_eq!(f.front.reflexes_at("main", "chat.say", Some("write the release notes")), vec!["new-session".to_string()]);
    let asked = events(&f, "reflex.asked");
    assert_eq!((asked.len(), asked[0]["do"].clone()), (1, ":fresh".into()), "{}", f.rig.dump());
    assert_eq!(f.count("thread.fresh"), 0);
    assert!(asked[0]["why"].is_string());
    let q = f.asked.lock().unwrap().iter().find(|q| q.point == "reflex").cloned().unwrap();
    assert!(q.text.contains("next line: cold") && q.text.contains("the line about to be sent: write the release notes"), "{}", q.text);
}

/// Wait for `what` to be true, up to three seconds.
fn until(what: &str, mut ok: impl FnMut() -> bool) {
    for _ in 0..60 {
        if ok() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("never: {what}");
}

/// A chat whose next line starts cold (the cache has lapsed) and the reflex `new-session`, which runs `:fresh` for it.
fn cold(pick: &'static str, reflexes: Reflexes) -> Fx {
    cold_in(pick, reflexes, false)
}

fn cold_in(pick: &'static str, reflexes: Reflexes, headless: bool) -> Fx {
    let (f, _) = rig_in(vec![turn("s1", 108_000, "one"), turn("s2", 20_000, "two")], pick, &row("new-session", ":fresh", "ctx.cold", "chat.say"), reflexes, headless);
    f.turn("one");
    f.rig.clock.advance(6 * 60 * SEC);
    f
}

/// The line went on: a second task is there, and the one of it.
fn went_on(f: &Fx) -> j4n_core::task::Row {
    until("the held line went on", || f.tasks().iter().filter(|r| r.spec.doer != "human").count() >= 2);
    f.tasks().into_iter().rfind(|r| r.spec.doer != "human").unwrap()
}

fn seq(f: &Fx, kind: &str) -> Vec<u64> {
    f.rig.of_kind(kind).iter().map(|e| e.seq).collect()
}

#[test]
fn a_line_that_would_start_cold_waits_for_the_judge_and_goes_to_a_new_session_on_new_subject() {
    let f = cold("reflex=fire", on(0));
    let said = f.say("a new subject: the release notes");
    assert_eq!(said, Said::Held, "{}", f.rig.dump());
    assert!(f.notes().iter().any(|n| n.contains("holding your line while the judge looks at it")), "{:?}", f.notes());
    let task = went_on(&f);
    assert_eq!(task.spec.goal, "a new subject: the release notes");
    // `:fresh` ran first, as the reflex, and the line is the first of the new session
    let fresh = f.rig.of_kind("thread.fresh");
    assert_eq!((fresh.len(), fresh[0].by.clone()), (1, j4n_core::change::Actor::Rule("reflex:new-session".into())), "{}", f.rig.dump());
    assert!(seq(&f, "thread.fresh")[0] < seq(&f, "task.queued").into_iter().max().unwrap());
    assert_eq!(f.count("rule.fired"), 1);
    let q = f.asked.lock().unwrap().iter().find(|q| q.point == "reflex").cloned().unwrap();
    assert!(q.text.contains("the line about to be sent: a new subject: the release notes"), "{}", q.text);
}

#[test]
fn a_held_line_goes_on_as_typed_on_same_with_no_new_session() {
    let f = cold("reflex=hold", on(0));
    assert_eq!(f.say("and the same thing, once more"), Said::Held);
    let task = went_on(&f);
    assert_eq!(task.spec.goal, "and the same thing, once more");
    assert_eq!((f.count("thread.fresh"), f.count("rule.fired")), (0, 0), "{}", f.rig.dump());
    assert_eq!(events(&f, "reflex.checked").len(), 1);
    // what came of the line is in the Log (a refused line shows too, with its reason)
    until("the release is written", || f.count("reflex.released") == 1);
    assert!(events(&f, "reflex.released")[0]["said"].as_str().unwrap().starts_with("Started"), "{}", f.rig.dump());
}

#[test]
fn a_judge_that_fails_or_does_not_answer_sends_the_line_as_typed() {
    // nobody answers: the fallback is not to fire
    let f = cold("absent", on(0));
    assert_eq!(f.say("write it"), Said::Held);
    assert_eq!(went_on(&f).spec.goal, "write it");
    assert_eq!(f.count("thread.fresh"), 0);
    // the judge panics on its thread: the line is not lost
    let f = cold("reflex=fire", on(0));
    f.boom.store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(f.say("write it again"), Said::Held);
    assert_eq!(went_on(&f).spec.goal, "write it again");
    f.boom.store(false, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(f.count("thread.fresh"), 0);
    // the judge is slow past the bound: the line goes on without it, and a late `fire` runs nothing
    let f = cold("reflex=fire", on(0));
    let clock = f.rig.clock.clone();
    *f.hook.lock().unwrap() = Some(Box::new(move || {
        clock.advance(20 * SEC);
        std::thread::sleep(std::time::Duration::from_millis(400));
    }));
    assert_eq!(f.say("write it late"), Said::Held);
    assert_eq!(went_on(&f).spec.goal, "write it late");
    until("the late judge is heard", || events(&f, "reflex.checked").iter().any(|c| c["why"].as_str().is_some_and(|w| w.contains("no longer waited"))));
    assert_eq!((f.count("thread.fresh"), f.count("rule.fired")), (0, 0), "{}", f.rig.dump());
}

#[test]
fn a_line_whose_check_does_not_hold_is_never_held_and_with_the_switch_off_nothing_is() {
    // warm: the cache has not lapsed, so the check does not hold
    let (f, _) = rig(vec![turn("s1", 108_000, "one"), turn("s1", 108_000, "two")], "reflex=fire", &row("new-session", ":fresh", "ctx.cold", "chat.say"), on(0));
    f.turn("one");
    assert!(matches!(f.say("next"), Said::Started { .. }));
    assert_eq!((judged(&f), f.count("thread.fresh")), (0, 0));
    // the switch off: a cold line goes at once, and the judge is not asked
    let f = cold("reflex=fire", Reflexes { on: false, ..on(0) });
    assert!(matches!(f.say("write it"), Said::Started { .. }), "{}", f.rig.dump());
    assert_eq!((judged(&f), f.count("thread.fresh")), (0, 0));
    // no consent for the judge, or the cap spent: the check is told and the line goes at once
    let f = cold("reflex=fire", Reflexes { judge: false, ..on(0) });
    assert!(matches!(f.say("write it"), Said::Started { .. }));
    assert_eq!(judged(&f), 0);
    let f = cold("reflex=fire", Reflexes { cap_usd: 0.0, ..on(0) });
    assert!(matches!(f.say("write it"), Said::Started { .. }));
    assert_eq!(judged(&f), 0);
}

#[test]
fn a_judge_not_yet_trusted_asks_and_the_held_line_follows_the_answer() {
    // a yes runs `:fresh`, then the line
    let f = cold("reflex=fire", on(5));
    assert_eq!(f.say("a new subject"), Said::Held);
    until("the operator is asked", || f.count("reflex.asked") == 1);
    assert!(f.tasks().iter().filter(|r| r.spec.doer != "human").count() == 1, "the line went before the answer");
    assert!(f.notes().iter().any(|n| n.contains("your line waits")), "{:?}", f.notes());
    f.say("yes");
    assert_eq!(went_on(&f).spec.goal, "a new subject");
    assert_eq!(f.count("thread.fresh"), 1);
    // a no sends it as typed
    let f = cold("reflex=fire", on(5));
    assert_eq!(f.say("the same subject"), Said::Held);
    until("the operator is asked", || f.count("reflex.asked") == 1);
    f.say("no");
    assert_eq!(went_on(&f).spec.goal, "the same subject");
    assert_eq!(f.count("thread.fresh"), 0);
    // any other line: the held one goes first, as typed, and the new one follows it
    let f = cold("reflex=fire", on(5));
    assert_eq!(f.say("first words"), Said::Held);
    until("the operator is asked", || f.count("reflex.asked") == 1);
    f.say("something else entirely");
    went_on(&f);
    let goals: Vec<String> = f.tasks().into_iter().filter(|r| r.spec.doer != "human").map(|r| r.spec.goal).collect();
    assert_eq!(goals[1], "first words", "{goals:?}");
    assert_eq!(f.count("thread.fresh"), 0);
}

#[test]
fn a_headless_run_waits_for_the_held_line_and_takes_what_came_of_it() {
    // many times over: the wait never sees the gap between the line leaving the map and its being sent
    for _ in 0..25 {
        let f = cold_in("reflex=hold", on(0), true);
        assert_eq!(f.say("write it"), Said::Held);
        assert!(matches!(f.front.held_wait("main"), Said::Started { .. }), "{}", f.rig.dump());
    }
}

#[test]
fn a_headless_run_asks_no_one_so_an_untrusted_judge_does_not_stall_the_line() {
    let f = cold_in("reflex=fire", on(5), true);
    let began = std::time::Instant::now();
    assert_eq!(f.say("a new subject"), Said::Held);
    // a wait that never ends is a failure, not a hang: the clock of a test does not run, so a question would wait for ever
    let (front, tx) = (f.front.clone(), std::sync::mpsc::channel());
    std::thread::spawn(move || drop(tx.0.send(front.held_wait("main"))));
    let said = tx.1.recv_timeout(std::time::Duration::from_secs(5)).unwrap_or_else(|_| panic!("the run waits for a yes nobody can give\n{}", f.rig.dump()));
    assert!(matches!(said, Said::Started { .. }), "{said:?}\n{}", f.rig.dump());
    // the bound is the judge's, not the operator's 120 s: the line went on at once, as typed, and nobody was asked
    assert!(began.elapsed() < std::time::Duration::from_secs(5), "{:?}", began.elapsed());
    assert_eq!((f.count("reflex.asked"), f.count("thread.fresh")), (0, 0));
    assert!(events(&f, "reflex.checked")[0]["why"].as_str().unwrap().contains("asks no one"));
}

#[test]
fn stop_drops_a_held_line_and_its_question() {
    // while the judge thinks
    let f = cold("reflex=fire", on(0));
    *f.hook.lock().unwrap() = Some(Box::new(|| std::thread::sleep(std::time::Duration::from_millis(500))));
    assert_eq!(f.say("write it"), Said::Held);
    assert!(matches!(f.say(":stop"), Said::Command(_)), "{}", f.rig.dump());
    std::thread::sleep(std::time::Duration::from_millis(900));
    assert_eq!(f.tasks().iter().filter(|r| r.spec.doer != "human").count(), 1, "the line was sent after :stop\n{}", f.rig.dump());
    assert_eq!((f.count("reflex.dropped"), f.count("thread.fresh"), f.count("rule.fired")), (1, 0, 0), "{}", f.rig.dump());
    assert!(f.notes().iter().any(|n| n.contains("stopped: the line that waited for the judge is dropped")), "{:?}", f.notes());
    // while the operator is asked: the stop is no answer, and the line is not sent first
    let f = cold("reflex=fire", on(5));
    assert_eq!(f.say("a new subject"), Said::Held);
    until("the operator is asked", || f.count("reflex.asked") == 1);
    f.say(":stop");
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(f.tasks().iter().filter(|r| r.spec.doer != "human").count(), 1, "{}", f.rig.dump());
    assert_eq!(f.count("reflex.dropped"), 1);
    assert!(f.notes().iter().any(|n| n.contains("the line that waited and the question about it are dropped")), "{:?}", f.notes());
}

#[test]
fn a_task_that_begins_while_the_judge_thinks_stops_a_firing_and_a_question() {
    let go = |run: &str| {
        let (f, _) = rig(vec![turn("s1", 108_000, "one"), slow("s1", 108_000, "two")], "reflex=fire", &row("t", run, "ctx.held > 0", "task.settled"), on(5));
        f.turn("one");
        let front = f.front.clone();
        *f.hook.lock().unwrap() = Some(Box::new(move || drop(front.say("main", "next"))));
        assert!(f.front.reflexes_at("main", "task.settled", None).is_empty(), "{run}");
        f
    };
    // a command that does not throw: it did not run
    let f = go(":context");
    assert_eq!((f.count("rule.fired"), f.count("chat.view")), (0, 0), "{}", f.rig.dump());
    assert!(events(&f, "reflex.checked")[0]["why"].as_str().unwrap().contains("began while the judge was thinking"));
    // one that throws: it was not even asked about
    let f = go(":compact junk");
    assert_eq!((f.count("rule.fired"), f.count("reflex.asked"), compacts(&f).len()), (0, 0, 0), "{}", f.rig.dump());
    assert!(events(&f, "reflex.checked")[0]["why"].as_str().unwrap().contains("began while the judge was thinking"));
}

#[test]
fn a_command_of_a_reflex_takes_no_words_but_compacts_own() {
    let (f, _) = rig(vec![turn("s1", 108_000, "one")], "reflex=fire", "", on(5));
    for line in ["add n do :new write the release notes when x check true", "add c do :context all when x check true", "add r do :fresh now when x check true"] {
        let said = f.front.reflex_said("main", &args(line));
        assert!(matches!(&said, Said::Refused(m) if m.contains("takes no words")), "{line}: {said:?}");
    }
    assert!(matches!(f.front.reflex_said("main", &args("add k do :compact keep the plan when x check true")), Said::Command(_)));
    assert_eq!(f.front.reflex_rows().0.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["k"]);
}

#[test]
fn calls_of_the_judge_reserve_the_cap_before_they_are_made() {
    let rows = row("a", ":context", "true", "task.settled") + &row("b", ":context", "true", "task.settled");
    let (f, _) = rig(vec![turn("s1", 108_000, "one")], "reflex=fire", &rows, Reflexes { cap_usd: 0.01, ..on(5) });
    f.turn("one");
    // while the judge answers for `a`, another check (another chat's, or `:probe`) reaches the cap: what `a` will cost is already spoken for
    let front = f.front.clone();
    *f.hook.lock().unwrap() = Some(Box::new(move || drop(front.reflexes_at("main", "task.settled", None))));
    assert_eq!(f.front.reflexes_at("main", "task.settled", None), vec!["a".to_string()]);
    assert_eq!(judged(&f), 1, "the second call was not made");
    let checked = events(&f, "reflex.checked");
    assert!(checked.len() == 1 && checked[0]["reflex"] == "b" && checked[0]["why"].as_str().unwrap().contains("cap"), "{checked:?}");
}

#[test]
fn a_cap_that_stops_the_cases_is_not_proven_and_is_not_a_failed_probe() {
    let rows = row("view", ":context", "true", "task.settled");
    let (f, _) = rig(vec![turn("s1", 108_000, "one")], "reflex=fire", &rows, Reflexes { cap_usd: 0.03, ..on(5) });
    *f.pays.lock().unwrap() = Some(Paid { usd: Some(0.02), tokens_in: 10, tokens_out: 1 });
    f.turn("one");
    for n in 1..=3 {
        f.front.reflex_said("main", &args(&format!("case view fire the junk is long logs, number {n}")));
    }
    assert!(f.front.reflexes_at("main", "task.settled", None).is_empty());
    assert_eq!((judged(&f), f.count("reflex.probed"), f.count("rule.fired")), (2, 0, 0), "two cases were put, the third had no cap left: {}", f.rig.dump());
    assert!(events(&f, "reflex.checked")[0]["why"].as_str().unwrap().contains("not proven"), "{}", f.rig.dump());
    // tomorrow it is tried again; it was not marked as one whose judge failed
    f.rig.clock.advance(25 * 3600 * SEC);
    f.front.reflexes_at("main", "task.settled", None);
    let why = events(&f, "reflex.checked").last().unwrap()["why"].as_str().unwrap().to_string();
    assert!(why.contains("not proven") && !why.contains("failed its cases"), "{why}");
    assert_eq!(f.count("reflex.probed"), 0);
}

#[test]
fn what_a_reflex_runs_is_written_as_the_reflex_and_what_the_operator_types_as_the_operator() {
    // trusted at once (no calls needed): the reflexes run with no question
    let (f, _) = rig(vec![turn("s1", 108_000, "one")], "reflex=fire", &row("clear", ":fresh", "true", "chat.say"), on(0));
    f.turn("one");
    f.front.reflexes_at("main", "chat.say", Some("anything"));
    assert_eq!(f.rig.of_kind("thread.fresh").iter().map(|e| e.by.clone()).collect::<Vec<_>>(), vec![j4n_core::change::Actor::Rule("reflex:clear".into())]);
    // the operator's own `:fresh` is theirs, as before
    f.say(":fresh");
    assert_eq!(f.rig.of_kind("thread.fresh").last().unwrap().by, j4n_core::change::Actor::Operator);
    // and so is the shrinking
    let (g, _) = rig(vec![turn("s1", 108_000, "one"), shrink("s1", 31_000)], "reflex=fire", &row("squeeze", ":compact junk", "ctx.held > 0", "task.settled"), on(0));
    g.turn("one");
    g.front.reflexes_at("main", "task.settled", None);
    assert_eq!(g.rig.of_kind("task.queued").last().unwrap().by, j4n_core::change::Actor::Rule("reflex:squeeze".into()), "{}", g.rig.dump());
}

#[test]
fn a_yes_does_not_run_a_reflex_that_was_switched_off_meanwhile() {
    let (f, _) = rig(vec![turn("s1", 108_000, "one")], "reflex=fire", &row("squeeze", ":compact junk", "ctx.held > 0", "task.settled"), on(5));
    f.turn("one");
    f.front.reflexes_at("main", "task.settled", None);
    assert_eq!(f.count("reflex.asked"), 1);
    f.front.reflex_said("main", &args("off squeeze"));
    assert_eq!(f.say("yes"), Said::Command("reflex".into()));
    assert_eq!((f.count("rule.fired"), compacts(&f).len(), events(&f, "label").len()), (0, 0, 0), "{}", f.rig.dump());
    assert!(f.notes().iter().any(|n| n.contains("is off now")), "{:?}", f.notes());
}

#[test]
fn a_question_ends_with_any_other_line_and_with_time_so_a_later_yes_is_the_doers() {
    let rows = row("squeeze", ":compact junk", "ctx.held > 0", "task.settled");
    // any line that is not its answer, a command included, ends it
    let (f, _) = rig(vec![turn("s1", 108_000, "one"), turn("s1", 108_000, "yes it is")], "reflex=fire", &rows, on(5));
    f.turn("one");
    f.front.reflexes_at("main", "task.settled", None);
    f.say(":context");
    f.turn("yes");
    assert_eq!((f.count("rule.fired"), compacts(&f).len()), (0, 0), "{}", f.rig.dump());
    // and it lapses
    let (g, _) = rig(vec![turn("s1", 108_000, "one"), turn("s1", 108_000, "yes it is")], "reflex=fire", &rows, on(5));
    g.turn("one");
    g.front.reflexes_at("main", "task.settled", None);
    g.rig.clock.advance(3 * 60 * SEC);
    g.turn("yes");
    assert_eq!((g.count("rule.fired"), compacts(&g).len()), (0, 0), "{}", g.rig.dump());
    assert!(g.notes().iter().any(|n| n.contains("the question lapsed")), "{:?}", g.notes());
}

#[test]
fn the_digest_holds_no_secret_in_the_name_of_the_model() {
    // (the Log scrubs an event before the fold reads it, so this holds with or without the digest's own scrub of the name: a guard, not a proof of it)
    let secret = format!("sk-or-v1-{}", "a1".repeat(32));
    let beats = vec![Beat::Session("s1".into()), Beat::Served(Box::leak(secret.clone().into_boxed_str()), "anthropic"), Beat::Cached(0.1, 108_000, 0, 108_000), Beat::Window(0.3), say("one"), end("one")];
    let (f, _) = rig(vec![beats], "reflex=hold", &row("view", ":context", "ctx.held > 0", "task.settled"), on(5));
    f.turn("one");
    f.front.reflexes_at("main", "task.settled", None);
    let q = f.asked.lock().unwrap().iter().find(|q| q.point == "reflex").cloned().unwrap();
    assert!(q.text.contains("held: 108k") && !q.text.contains(&secret), "{}", q.text);
}

#[test]
fn a_panic_on_the_thread_of_a_chat_does_not_end_its_reflexes() {
    let (f, _) = rig(vec![turn("s1", 108_000, "one")], "reflex=hold", &row("view", ":context", "ctx.held > 0", "task.settled"), on(5));
    f.turn("one");
    f.boom.store(true, std::sync::atomic::Ordering::SeqCst);
    f.front.reflex_step("main");
    std::thread::sleep(std::time::Duration::from_millis(400));
    f.boom.store(false, std::sync::atomic::Ordering::SeqCst);
    f.rig.clock.advance(2 * SEC);
    f.front.reflex_step("main");
    until("the chat's reflexes are judged again", || judged(&f) >= 1);
}

#[test]
fn a_judge_that_panics_gives_back_the_cent_it_reserved() {
    // the cap is one cent and every call costs nothing: a cent a panic keeps would stop every call after it
    let (f, _) = rig(vec![turn("s1", 108_000, "one")], "reflex=hold", &row("view", ":context", "ctx.held > 0", "task.settled"), Reflexes { cap_usd: 0.01, ..on(5) });
    *f.pays.lock().unwrap() = Some(Paid { usd: Some(0.0), tokens_in: 10, tokens_out: 1 });
    f.turn("one");
    f.boom.store(true, std::sync::atomic::Ordering::SeqCst);
    f.front.reflex_step("main");
    assert!(f.front.judges_wait());
    f.boom.store(false, std::sync::atomic::Ordering::SeqCst);
    f.rig.clock.advance(2 * SEC);
    f.front.reflex_step("main");
    assert!(f.front.judges_wait());
    let checked = events(&f, "reflex.checked");
    assert!(checked.iter().any(|c| c["by"] == "decider:jev") && checked.iter().all(|c| !c["why"].as_str().unwrap_or("").contains("cap")), "the cent was kept\n{}", f.rig.dump());
}

#[test]
fn a_run_ends_a_boundary_only_after_the_judge_answered_or_the_hold_ran_out() {
    // a judge that thinks a while: the boundary returns at once (the screen draws on), and a run waits for the answer
    let (f, _) = rig(vec![turn("s1", 108_000, "one")], "reflex=hold", &row("view", ":context", "ctx.held > 0", "task.settled"), on(5));
    f.turn("one");
    *f.hook.lock().unwrap() = Some(Box::new(|| std::thread::sleep(std::time::Duration::from_millis(400))));
    f.front.reflex_step("main");
    assert_eq!(f.count("reflex.checked"), 0, "the judge is still thinking");
    assert!(f.front.judges_wait(), "it answered in time");
    assert_eq!((judged(&f), f.count("reflex.checked")), (1, 1), "{}", f.rig.dump());
    // a judge that does not answer in the hold: the run goes on without it, and does not hang. The hook moves the clock on, over and over until it is let go, so it makes no
    // difference whether it runs before or after the wait has read the clock
    f.rig.clock.advance(2 * SEC);
    let (clock, done) = (f.rig.clock.clone(), Arc::new(std::sync::atomic::AtomicBool::new(false)));
    let d = done.clone();
    *f.hook.lock().unwrap() = Some(Box::new(move || {
        while !d.load(std::sync::atomic::Ordering::SeqCst) {
            clock.advance(20 * SEC);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }));
    f.front.reflex_step("main");
    assert!(!f.front.judges_wait(), "the hold ran out");
    // (the thread ends before the test does)
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(f.front.judges_wait());
}

#[test]
fn rule_reflex_holds_where_the_chat_holds_nothing_and_leaves_the_rest_to_the_judge() {
    let q = |text: &str| Question { ask: "t".into(), point: "reflex".into(), asker: "front".into(), thread: None, text: text.into(), shape: j4n_core::ask::AskShape::YesNo, goal: String::new(), act: None };
    assert!(matches!(reflex_rule().offer(&q("reflex: x\nheld: none\nnext line: cold")), Offer::Answer { answer: j4n_core::ask::Answer::Pick { id }, .. } if id == "hold"));
    assert!(matches!(reflex_rule().offer(&q("reflex: x\nheld: 108k")), Offer::Pass { .. }));
}

/// A row of the kind `rule` that asks, with `edit` applied to what a good one says.
fn ask(edit: impl FnOnce(&mut toml::Table)) -> Result<(), Vec<String>> {
    let mut t: toml::Table = toml::from_str("id = \"a\"\non = \"task.settled\"\nwhen = \"ctx.grew >= 0.01\"\nthen = \"ask\"\ntext = \"much junk\"\ndo = \":compact junk\"\nlimit = { per = \"thread\", max = 1, within = \"10m\" }\n").unwrap();
    edit(&mut t);
    toml::Value::Table(t).try_into::<RuleRow>().unwrap().compile().map(|_| ())
}

#[test]
fn a_rule_that_asks_is_refused_for_what_it_says() {
    assert_eq!(ask(|_| {}), Ok(()));
    let set = |k: &'static str, v: toml::Value| move |t: &mut toml::Table| drop(t.insert(k.into(), v));
    let gone = |k: &'static str| move |t: &mut toml::Table| drop(t.remove(k));
    for (got, why) in [
        (ask(gone("when")), "condition"),
        (ask(set("when", "  ".into())), "condition"),
        (ask(set("on", "run.metrics".into())), "`on`"),
        (ask(set("do", ":accept p3".into())), "widens"),
        (ask(set("do", ":ask do something".into())), "widens"),
        (ask(gone("do")), "widens"),
        (ask(set("text", "".into())), "sentence"),
        (ask(gone("limit")), "limit"),
        (ask(set("limit", toml::Value::Table(toml::from_str("per = \"thread\"\nmax = 50\nwithin = \"10m\"").unwrap()))), "limit"),
        (ask(set("id", "a b".into())), "id"),
        (ask(set("limit", toml::Value::Table(toml::from_str("per = \"task\"\nmax = 1\nwithin = \"10m\"").unwrap()))), "limit"),
        (ask(set("limit", toml::Value::Table(toml::from_str("per = \"thread\"\nmax = 1\nwithin = \"soon\"").unwrap()))), "within"),
    ] {
        assert!(got.as_ref().is_err_and(|e| e.iter().any(|m| m.contains(why))), "{why}: {got:?}");
    }
}

#[test]
fn a_reflex_row_of_the_old_form_still_loads_as_the_rule_it_says() {
    // `[[reflex]]` with `do`, `when` (the sentence), `check`, `event`, `on` and `cool`: read as a rule that asks; and with no check it is refused as before
    let old = "[[reflex]]\nid = \"old\"\ndo = \":context\"\nwhen = \"the context holds junk\"\ncheck = \"ctx.held > 0\"\nevent = \"task.settled\"\non = true\ncool = \"1s\"\n[[reflex]]\nid = \"bare\"\ndo = \":context\"\nwhen = \"x\"\n";
    let (f, _) = rig(vec![turn("s1", 108_000, "one")], "reflex=fire", old, on(5));
    let (rows, problems) = f.front.reflex_rows();
    assert_eq!(rows.iter().map(|r| (r.id.as_str(), r.on.as_str(), r.off, r.text.as_deref())).collect::<Vec<_>>(), [("old", "task.settled", false, Some("the context holds junk"))]);
    assert!(problems.iter().any(|p| p.contains("bare") && p.contains(NO_CHECK)), "{problems:?}");
    f.turn("one");
    assert_eq!(f.front.reflexes_at("main", "task.settled", None), vec!["old".to_string()]);
    assert_eq!((f.count("rule.fired"), f.count("chat.view")), (1, 1), "{}", f.rig.dump());
}

#[test]
fn the_trust_of_the_judge_is_read_from_the_answer_state_not_counted_by_the_reflex() {
    // calls that held were labelled in the Log, by whoever labelled them: the reflex asks no question once the answer state counts enough of them
    let (f, _) = rig(vec![turn("s1", 108_000, "one"), shrink("s1", 31_000)], "reflex=fire", &row("squeeze", ":compact junk", "ctx.held > 0", "task.settled"), on(2));
    let label = |n: u32, what: &str, weight: f64| {
        let data = serde_json::json!({ "subject_kind": "answer", "subject": format!("a{n}"), "producer": "decider:jev", "model": "", "point": "reflex", "label": what, "weight": weight });
        f.rig.log.append(j4n_core::log::New::of("label", j4n_core::change::Actor::Core, data).thread("main")).unwrap();
    };
    f.turn("one");
    label(1, "held", 0.0);
    label(2, "overridden", 1.0);
    // one held and one overridden: not trusted, the operator is asked
    assert_eq!(f.front.reflexes_at("main", "task.settled", None), vec!["squeeze".to_string()]);
    assert_eq!((f.count("reflex.asked"), compacts(&f).len()), (1, 0), "{}", f.rig.dump());
    f.say("no");
    let g = rig(vec![turn("s1", 108_000, "one"), shrink("s1", 31_000)], "reflex=fire", &row("squeeze", ":compact junk", "ctx.held > 0", "task.settled"), on(2)).0;
    g.turn("one");
    for n in 1..=2 {
        let data = serde_json::json!({ "subject_kind": "answer", "subject": format!("h{n}"), "producer": "decider:jev", "model": "", "point": "reflex", "label": "held", "weight": 0.0 });
        g.rig.log.append(j4n_core::log::New::of("label", j4n_core::change::Actor::Core, data).thread("main")).unwrap();
    }
    // two held, none wrong: it runs at once
    assert_eq!(g.front.reflexes_at("main", "task.settled", None), vec!["squeeze".to_string()]);
    assert_eq!((g.count("reflex.asked"), g.count("rule.fired"), compacts(&g).len()), (0, 1, 1), "{}", g.rig.dump());
}

#[test]
fn the_reflexes_the_pack_ships_hold_together_are_off_and_each_has_a_case_that_fires_and_one_that_holds() {
    let (tables, problems) = Tables::load(vec![Layer::new(LayerKind::Pack, "reflexes", vec![SourceFile { path: "reflexes/shipped.toml".into(), text: include_str!("../../../../packs/reflexes/reflexes/shipped.toml").into() }])]);
    assert!(problems.is_empty(), "{problems:?}");
    let rows: Vec<RuleRow> = tables.ids("rule").into_iter().map(|id| tables.get::<RuleRow>(id).unwrap()).collect();
    assert_eq!(rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["compact-junk", "new-session"]);
    for r in &rows {
        assert!(r.compile().is_ok(), "{}: {:?}", r.id, r.compile().err());
        assert!(r.off, "{} ships off", r.id);
        let cases: Vec<j4n_core::front::reflexes::CaseRow> = tables.ids("reflex_case").into_iter().map(|id| tables.get(id).unwrap()).filter(|c: &j4n_core::front::reflexes::CaseRow| c.reflex == r.id).collect();
        assert!(cases.iter().any(|c| c.fire) && cases.iter().any(|c| !c.fire), "{}: a pair of cases", r.id);
    }
}

#[test]
fn an_event_of_the_old_name_still_reads_and_shows_beside_the_new_one() {
    // the Log is append-only and hash-chained, so a `toggle.checked` written before the rename stays as it was: nothing reads it as a reflex's, and it shows as any event does
    let (f, _) = rig(vec![turn("s1", 108_000, "one"), turn("s1", 120_000, "two")], "reflex=hold", &row("junk", ":context", "ctx.grew >= 0.01", "task.settled"), on(5));
    let old = serde_json::json!({ "toggle": "junk", "fire": false, "by": "decider:jev", "why": "it knew", "event": "task.settled", "check": "ctx.grew >= 0.01", "usd": 0.01, "ask": "" });
    f.rig.log.append(j4n_core::log::New::of("toggle.checked", j4n_core::change::Actor::Core, old.clone()).thread("main")).unwrap();
    f.turn("one");
    f.front.reflexes_at("main", "task.settled", None);
    f.turn("two");
    f.front.reflexes_at("main", "task.settled", None);
    assert_eq!(events(&f, "toggle.checked"), [old], "the old event is as it was written");
    assert_eq!(events(&f, "reflex.checked").len(), 1, "the new check is written under the new name: {}", f.rig.dump());
    let mut rows = j4n_core::front::window::LogRows::of(f.rig.log.read(0));
    let (shown, _) = rows.window(&j4n_core::front::window::Ask { from: 0, count: 500, sort: String::new(), find: "checked".into() });
    let texts: Vec<&str> = shown.iter().map(|r| r.cells[1].text.as_str()).collect();
    assert!(texts.iter().any(|t| t.starts_with("toggle.checked") && t.contains("it knew")), "{texts:?}");
    assert!(texts.iter().any(|t| t.starts_with("reflex.checked")), "{texts:?}");
}

// ---- one `yes_no`, one slot for what waits for a yes ----

/// A chat where a reflex asks and a review asks first, the one claude turns of `turns`.
fn both(turns: Vec<Vec<Beat>>) -> Fx {
    let claude = Scripted::with_caps("claude", Caps { live: true, resume: true, compact: true, brief: true, ..Caps::default() }, turns);
    fx_reflexes_in(vec![claude], Some("reflex=fire"), screen(&row("squeeze", ":compact junk", "ctx.held > 0", "task.settled")), false, |c: &mut Cfg| {
        c.doer = Some("claude".into());
        c.session = Life::Resume;
        c.reflexes = on(5);
        c.offload = j4n_core::offload::Offload::shipped();
    })
}

fn chore(answer: &str) -> Vec<Beat> {
    vec![Beat::Session("claude-s".into()), say(answer), end(answer)]
}

fn chores(f: &Fx) -> Vec<j4n_core::task::Row> {
    f.tasks().into_iter().filter(|r| r.spec.role.as_deref() == Some("chore")).collect()
}

#[test]
fn a_yes_reaches_only_the_question_of_its_own_kind() {
    let f = both(vec![turn("s1", 108_000, "one"), chore("one point"), shrink("s1", 31_000)]);
    f.turn("one");
    // a review waits and nothing else: the yes sends it, and no reflex heard it
    assert_eq!(f.say("review my change"), Said::Command("offload".into()));
    let Said::Started { task } = f.say("yes") else { panic!("{}", f.rig.dump()) };
    f.rig.finished(&task);
    assert_eq!((chores(&f).len(), f.count("rule.fired"), f.count("label")), (1, 0, 0), "{}", f.rig.dump());
    // a reflex waits and nothing else: the yes runs it, and no review started
    f.rig.clock.advance(2 * SEC);
    assert_eq!(f.front.reflexes_at("main", "task.settled", None), vec!["squeeze".to_string()]);
    assert_eq!(f.say("yes"), Said::Command("reflex".into()));
    assert_eq!((chores(&f).len(), f.count("rule.fired"), compacts(&f).len()), (1, 1, 1), "{}", f.rig.dump());
    // and the one word list is the binder's: `okay` is a yes, `go` and `hold` are not
    assert_eq!((j4n_core::binder::yes_no("Okay"), j4n_core::binder::yes_no("go"), j4n_core::binder::yes_no("hold"), j4n_core::binder::yes_no("not now")), (Some(true), None, None, Some(false)));
}

#[test]
fn with_a_reflex_and_a_review_both_waiting_the_first_yes_is_the_reflexs_and_the_review_waits_for_the_next() {
    let f = both(vec![turn("s1", 108_000, "one"), shrink("s1", 31_000), chore("one point")]);
    f.turn("one");
    assert_eq!(f.say("review my change"), Said::Command("offload".into()));
    assert_eq!(f.front.reflexes_at("main", "task.settled", None), vec!["squeeze".to_string()]);
    assert_eq!((f.count("offload.asked"), f.count("reflex.asked")), (1, 1), "both wait");
    // the order of `say_with`: the reflex takes the first yes; the review is untouched
    assert_eq!(f.say("yes"), Said::Command("reflex".into()));
    assert_eq!((f.count("rule.fired"), compacts(&f).len(), chores(&f).len()), (1, 1, 0), "{}", f.rig.dump());
    f.rig.finished(&compacts(&f)[0].id);
    // the next yes is the review's
    let Said::Started { task } = f.say("yes") else { panic!("{}", f.rig.dump()) };
    assert_eq!(f.rig.finished(&task).spec.role.as_deref(), Some("chore"));
    assert_eq!((f.count("rule.fired"), chores(&f).len()), (1, 1), "a yes is never answered twice");
}

#[test]
fn rule_reflex_reads_the_field_held_not_a_sentence_that_says_so() {
    let q = |text: &str| Question { ask: "t".into(), point: "reflex".into(), asker: "front".into(), thread: None, text: text.into(), shape: j4n_core::ask::AskShape::YesNo, goal: String::new(), act: None };
    assert!(matches!(reflex_rule().offer(&q("reflex: x\nthe digest says held: none, in words\nheld: 108k")), Offer::Pass { .. }));
    assert!(matches!(reflex_rule().offer(&q("a different first line\nheld: none")), Offer::Answer { .. }));
}
