//! A session's life, `session.mode`. The loop, the front and the Log are the real ones and the doer is scripted: a
//! claude session that reads what the 108,449-token `ping` read, five minutes' cache life from the base pack's cost source, and a clock the test
//! moves. What is asserted is what the doer was started with (`starts()`), the chat's rows and the Log.

use crate::front_cases::{Fx, fx, fx_deciding, fx_planned};
use crate::scripted::{Beat, Scripted, end, say};
use j4n_core::front::Said;
use j4n_core::front::context::VIEW;
use j4n_core::front::session::{CARRY_ALL, CARRY_ANSWERS, Life};
use j4n_core::front::usage::plain;
use serde_json::{Value, json};
use std::sync::Arc;

const MIN: u64 = 60_000;
const NOISE: &str = "TRANSCRIPT-ONLY: the whole tool output of this turn, line after line";

/// One turn of a claude session: it reads `all` tokens (all written: the cache was cold), says a long thing and then its answer.
fn turn(session: &str, all: u64, answer: &str) -> Vec<Beat> {
    vec![Beat::Session(session.into()), Beat::Served("claude-haiku-5-5", "anthropic"), Beat::Cached(0.1, all, 0, all), Beat::Window(0.3), say(NOISE), end(answer)]
}

fn rig(turns: Vec<Vec<Beat>>, mode: Life) -> (Fx, Arc<Scripted>) {
    let claude = Scripted::new("claude", turns);
    (fx_planned(vec![claude.clone()], move |c| c.session = mode), claude)
}

fn resumes(c: &Scripted) -> Vec<Option<String>> {
    c.starts().iter().map(|s| s.resume.clone()).collect()
}

fn ctx(f: &Fx) -> Vec<Value> {
    f.rig.of_kind("task.settled").iter().filter(|e| e.task.is_some()).map(|e| e.data["ctx"].clone()).collect()
}

const WHY: &str = "new session: cold and 108k held (session.mode auto) · :resume to keep the old one";

#[test]
fn auto_with_a_cold_big_session_starts_a_new_one_with_a_carry_over_and_the_chat_says_why() {
    let (f, claude) = rig(vec![turn("s1", 108_000, "answer one"), turn("s2", 30_000, "answer two")], Life::Auto);
    f.turn("one");
    f.rig.clock.advance(6 * MIN);
    let second = f.turn("two");
    assert_eq!(resumes(&claude), [None, None], "the second line did not carry on in s1: {}", f.rig.dump());
    assert_eq!(second.spec.follows, None);
    let told = claude.starts()[1].text.clone().unwrap_or_default();
    assert!(told.starts_with("two\n\n") && told.contains("Carried over") && told.contains("you: one → answer one"), "{told}");
    assert!(!told.contains("TRANSCRIPT-ONLY"), "the old transcript is not handed over: {told}");
    assert_eq!(f.notes().iter().filter(|n| n.starts_with("new session")).collect::<Vec<_>>(), [WHY], "the chat row says why");
    let changed = f.rig.of_kind("session.changed");
    assert_eq!((changed.len(), changed[0].data["from"].clone(), changed[0].data["mode"].clone()), (1, json!("s1"), json!("auto")));
    // the new session shows as its floor and no history, in the ledger's numbers and in /context
    assert_eq!(ctx(&f)[1]["floor"], 30_000);
    assert_eq!((ctx(&f)[1]["in"].clone(), ctx(&f)[1]["hist"].clone()), (json!(30_000), json!(0)));
    f.say("/context");
    let view = f.rig.of_kind("chat.view").last().map(|e| e.data.clone()).unwrap();
    assert_eq!(view["view"], VIEW);
    assert!(view["text"].as_str().unwrap().contains("30k held: floor 30k + history 0"), "{view}");
    assert!(plain(&f.front.context_value("main")["value"]).contains("30k held: floor 30k + history 0"));
}

#[test]
fn auto_with_a_warm_session_or_a_small_one_carries_on() {
    // warm: five minutes is not older than the cache life
    let (warm, c) = rig(vec![turn("s1", 108_000, "a"), turn("s1", 108_000, "b")], Life::Auto);
    warm.turn("one");
    warm.rig.clock.advance(5 * MIN);
    warm.turn("two");
    assert_eq!(resumes(&c), [None, Some("s1".into())], "{}", warm.rig.dump());
    assert!(warm.rig.of_kind("session.changed").is_empty());
    // small: cold, but 30,000 is not past session.big (40,000)
    let (small, c) = rig(vec![turn("s1", 30_000, "a"), turn("s1", 30_000, "b")], Life::Auto);
    small.turn("one");
    small.rig.clock.advance(6 * 60 * MIN);
    small.turn("two");
    assert_eq!(resumes(&c), [None, Some("s1".into())]);
    assert!(small.notes().iter().all(|n| !n.starts_with("new session")));
}

#[test]
fn auto_with_a_cache_life_nobody_said_is_never_cold_by_guess() {
    // `fx` has no cost sources: the provider's cache life is not known, and 108k held for six hours is still resumed
    let claude = Scripted::new("claude", vec![turn("s1", 108_000, "a"), turn("s1", 108_000, "b")]);
    let f = fx(vec![claude.clone()], |c| c.session = Life::Auto);
    f.turn("one");
    f.rig.clock.advance(6 * 60 * MIN);
    f.turn("two");
    assert_eq!(resumes(&claude), [None, Some("s1".into())], "{}", f.rig.dump());
    assert!(f.rig.of_kind("session.changed").is_empty());
}

#[test]
fn each_starts_a_new_session_for_every_line_and_the_first_line_has_nothing_to_leave() {
    let (f, claude) = rig(vec![turn("s1", 1_000, "a"), turn("s2", 1_000, "b"), turn("s3", 1_000, "c")], Life::Each);
    f.turn("one");
    f.turn("two");
    f.turn("three");
    assert_eq!(resumes(&claude), [None, None, None], "{}", f.rig.dump());
    let rows: Vec<String> = f.notes().into_iter().filter(|n| n.starts_with("new session")).collect();
    assert_eq!(rows, vec!["new session: every line its own (session.mode each) · :resume to keep the old one"; 2]);
    assert!(claude.starts()[2].text.as_deref().unwrap_or_default().contains("you: two → b"), "{:?}", claude.starts()[2].text);
}

#[test]
fn resume_always_carries_on_however_cold_and_big() {
    let (f, claude) = rig(vec![turn("s1", 108_000, "a"), turn("s1", 108_000, "b"), turn("s1", 108_000, "c")], Life::Resume);
    f.turn("one");
    f.rig.clock.advance(6 * 60 * MIN);
    f.turn("two");
    f.rig.clock.advance(6 * 60 * MIN);
    f.turn("three");
    assert_eq!(resumes(&claude), [None, Some("s1".into()), Some("s1".into())]);
    assert!(f.rig.of_kind("session.changed").is_empty());
}

#[test]
fn resume_brings_the_old_session_back_for_the_next_line_once() {
    let (f, claude) = rig(vec![turn("s1", 108_000, "a"), turn("s2", 30_000, "b"), turn("s1", 108_000, "c"), turn("s1", 108_000, "d")], Life::Auto);
    f.turn("one");
    f.rig.clock.advance(6 * MIN);
    f.turn("two"); // cold and big: s2
    assert_eq!(f.say(":resume"), Said::Command("resume".into()));
    assert!(f.notes().last().unwrap().contains("carries on in session s1 (once)"), "{:?}", f.notes());
    let third = f.turn("three");
    let first = f.tasks().into_iter().find(|r| r.spec.goal == "one").unwrap();
    assert_eq!((third.spec.resume.as_ref().map(|s| s.id.as_str()), third.spec.follows.as_deref()), (Some("s1"), Some(first.id.as_str())), "{}", f.rig.dump());
    // once: what asked for it is spent, and the line after carries on in the newest session as any line does
    f.turn("four");
    assert_eq!(resumes(&claude), [None, None, Some("s1".into()), Some("s1".into())]);
    assert_eq!(f.say(":resume"), Said::Command("resume".into()), "with nothing left, it keeps the session there is");
}

#[test]
fn without_resume_the_line_after_a_change_carries_on_in_the_new_session() {
    let (f, claude) = rig(vec![turn("s1", 108_000, "a"), turn("s2", 30_000, "b"), turn("s2", 30_000, "c")], Life::Auto);
    f.turn("one");
    f.rig.clock.advance(6 * MIN);
    f.turn("two");
    f.turn("three");
    assert_eq!(resumes(&claude), [None, None, Some("s2".into())]);
}

#[test]
fn resume_with_nothing_to_take_back_says_so_and_fresh_forgets_it() {
    let (f, claude) = rig(vec![turn("s1", 108_000, "a"), turn("s2", 30_000, "b"), turn("s3", 30_000, "c")], Life::Auto);
    assert!(matches!(f.say(":resume"), Said::Refused(w) if w.contains("not bound")));
    f.turn("one");
    f.rig.clock.advance(6 * MIN);
    f.turn("two");
    assert_eq!(f.say(":resume"), Said::Command("resume".into()));
    assert_eq!(f.say(":fresh"), Said::Command("fresh".into()));
    f.turn("three");
    assert_eq!(resumes(&claude), [None, None, None], "after :fresh nothing is taken back");
}

#[test]
fn the_carry_over_is_capped_and_redacted_and_holds_the_answers_the_plan_line_and_the_files() {
    let long = "x".repeat(3_000);
    let plan = json!({ "approach": "Read first, then change.", "items": [
        { "id": "read", "goal": "do read", "reads": ["src/read.rs"], "writes": ["src/read.rs"], "depends": [], "done": "read works" },
        { "id": "change", "goal": "do change", "reads": [], "writes": ["src/change.rs"], "depends": ["read"], "done": "change works" },
        { "id": "check", "goal": "do check", "reads": [], "writes": [], "depends": ["change"], "done": "check works" }] })
    .to_string();
    let mut write = turn("s1", 60_000, "the key is here");
    write.insert(0, Beat::Put("src/read.rs".into(), "fn main() {}\n".into()));
    let turns = vec![write, turn("s1", 60_000, &long), turn("s1", 60_000, &plan), turn("s1", 60_000, &long), turn("s1", 108_000, &long), turn("s2", 30_000, "after")];
    let (f, claude) = rig(turns, Life::Auto);
    f.turn("one");
    f.turn("two");
    let Said::Started { task } = f.say(":plan do it") else { panic!("{}", f.rig.dump()) };
    f.rig.finished(&task);
    f.front.plan_collect("main");
    // the next two lines are the plan's first two items; the third is open when the session is left
    f.turn("four");
    f.front.plan_collect("main");
    f.turn("five");
    f.front.plan_collect("main");
    f.rig.clock.advance(6 * MIN);
    f.turn("six");
    let told = claude.starts()[5].text.clone().unwrap_or_default();
    let carried = told.strip_prefix("six\n\n").unwrap_or_else(|| panic!("{told}"));
    assert!(carried.chars().count() <= CARRY_ALL, "capped: {}", carried.chars().count());
    assert_eq!(carried.matches("- you: ").count(), CARRY_ANSWERS, "the last {CARRY_ANSWERS} answers of the chat's lines (not the planning turn's) and no more: {carried}");
    assert!(carried.contains("you: five → xxx") && carried.contains("you: four → xxx") && carried.contains("you: two → xxx") && !carried.contains("the key is"), "{carried}");
    assert!(carried.contains("Plan: now check: do check · approach: Read first, then change."), "{carried}\n{:?}\n{:?}", f.notes(), f.keeper.plan("main"));
    assert!(carried.contains("Files changed: src/read.rs"), "{carried}");
    assert!(!carried.contains("TRANSCRIPT-ONLY") && !carried.contains(&"x".repeat(500)), "no transcript, each answer clipped: {carried}");
    // a secret the redactor learns after the answer was written is not carried over (the Log scrubs what it is given when it is given it)
    let (g, claude) = rig(vec![turn("s1", 108_000, "the token is hunter2-hunter2"), turn("s2", 30_000, "after")], Life::Auto);
    g.turn("one");
    assert!(g.rig.log.redactor().register("TOKEN", "hunter2-hunter2"));
    g.rig.clock.advance(6 * MIN);
    g.turn("two");
    let told = claude.starts()[1].text.clone().unwrap_or_default();
    assert!(told.contains("the token is «secret:TOKEN»") && !told.contains("hunter2"), "{told}");
}

#[test]
fn a_doer_that_takes_a_brief_is_handed_the_carry_over_as_a_section_of_it() {
    let caps = j4n_core::doer::Caps { live: true, resume: true, brief: true, ..Default::default() };
    let claude = Scripted::with_caps("claude", caps, vec![turn("s1", 108_000, "answer one"), turn("s2", 30_000, "answer two")]);
    let f = fx_planned(vec![claude.clone()], |c| c.session = Life::Auto);
    f.turn("one");
    f.rig.clock.advance(6 * MIN);
    f.turn("two");
    let rendered = f.rig.of_kind("brief.rendered");
    assert!(rendered[0].data["sections"].to_string().contains("contract") && !rendered[0].data["sections"].to_string().contains("carry"));
    assert!(rendered[1].data["sections"].to_string().contains("carry"), "{}", rendered[1].data);
    assert_eq!(rendered[1].data["in_session"], false);
}

/// A part runner that takes every item as a part that starts in the session, not shrunk (what `route.rs` builds a `Turn` from) and settles none.
struct Parts;

impl j4n_core::front::parts::Parts for Parts {
    fn on(&self) -> bool {
        true
    }
    fn part(&self, _: &str, _: &j4n_core::keeper::Item) -> bool {
        true
    }
    fn ended(&self, _: &str, _: &j4n_core::keeper::Item, _: &j4n_core::task::Row, _: &dyn j4n_core::front::parts::Hands) -> j4n_core::front::parts::Taken {
        j4n_core::front::parts::Taken::Not
    }
    fn boundary(&self, _: &str, _: &j4n_core::keeper::Item) -> Option<j4n_core::front::parts::Boundary> {
        Some(j4n_core::front::parts::Boundary { fresh: false, compact: false, brief: "the part".into() })
    }
    fn command(&self, _: &str, _: &[String], _: Option<&j4n_core::keeper::Plan>) -> Result<String, String> {
        Ok(String::new())
    }
}

#[test]
fn each_starts_a_session_for_the_operators_prompt_and_the_plan_items_that_follow_stay_in_it() {
    let items: Vec<Value> = ["a", "b", "c"].iter().map(|id| json!({ "id": id, "goal": format!("do {id}"), "reads": [], "writes": [], "depends": [], "done": format!("{id} works"), "where": "inline" })).collect();
    let plan = json!({ "approach": "Do a, b, then c.", "items": items }).to_string();
    let one = |s: &str, a: &str| vec![Beat::Session(s.into()), say(a), end(a)];
    for via_parts in [false, true] {
        let claude = Scripted::new("claude", vec![one("s1", &plan), one("s1", "did a"), one("s1", "did b"), one("s1", "did c"), one("s2", "more")]);
        let f = fx_deciding(vec![claude.clone()], Some("intake=flow"), |c| {
            c.doer = Some("claude".into());
            c.lanes = j4n_core::branch::Switch::Auto;
            c.session = Life::Each;
            if via_parts {
                c.parts = Some(Arc::new(Parts));
            }
        });
        let Said::Started { task } = f.say("plan how to do a, b and c, then do them") else { panic!("{}", f.rig.dump()) };
        f.rig.finished(&task);
        f.front.plan_collect("main");
        for _ in 0..3 {
            for r in f.tasks() {
                f.rig.finished(&r.id);
            }
            f.front.plan_step("main");
        }
        for r in f.tasks() {
            f.rig.finished(&r.id);
        }
        // the items after the prompt that planned are not prompts: they go on in its session; the next thing the operator types is one
        let Said::Started { task } = f.say("and one more thing") else { panic!("{}", f.rig.dump()) };
        f.rig.finished(&task);
        assert_eq!(resumes(&claude), [None, Some("s1".into()), Some("s1".into()), Some("s1".into()), None], "parts {via_parts}: {}", f.rig.dump());
        assert_eq!(f.rig.of_kind("session.changed").len(), 1, "parts {via_parts}");
    }
}

#[test]
fn a_secret_cut_by_the_clip_is_scrubbed_before_it_is_cut() {
    // an answer whose secret begins before the 400th character: cut first, half of it would not be the secret the redactor knows
    let secret = "hunter2-hunter2-hunter2";
    let answer = format!("{} {secret} and on", "x".repeat(383));
    let (f, claude) = rig(vec![turn("s1", 108_000, &answer), turn("s2", 30_000, "after")], Life::Auto);
    f.turn("one");
    assert!(f.rig.log.redactor().register("TOKEN", secret));
    f.rig.clock.advance(6 * MIN);
    f.turn("two");
    let told = claude.starts()[1].text.clone().unwrap_or_default();
    assert!(told.contains("«secret:TOKEN»") && !told.contains("hunter2"), "{told}");
}
