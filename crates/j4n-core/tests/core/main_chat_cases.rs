//! The word `orchestrate`, and Main as a chat with a doer session of its own,
//! elected at `who:main`. A line in Main is Main's own turn, whole: the turn path is `single`'s and nothing more. The front, the Log, the
//! loop and the base pack's table are the real ones and the doers are scripted.

use crate::front_cases::{Fx, chatty, fx, fx_deciding};
use j4n_core::branch::Switch;
use j4n_core::front::Cfg;
use j4n_core::front::serve::Serve;
use j4n_proto::{COMPOSER_STRIP, Down, Up};
use serde_json::Value;
use std::collections::BTreeMap;

fn single(c: &mut Cfg) {
    c.doer = Some("claude".into());
}

fn orchestrate(c: &mut Cfg) {
    c.doer = Some("claude".into());
    c.lanes = Switch::Orchestrate;
    c.chats.screen = true;
}

fn auto(c: &mut Cfg) {
    c.doer = Some("claude".into());
    c.lanes = Switch::Auto;
}

fn kinds(f: &Fx) -> BTreeMap<String, usize> {
    let mut n = BTreeMap::new();
    for e in f.rig.events() {
        *n.entry(e.kind).or_insert(0) += 1;
    }
    n
}

const LINES: [&str; 3] = ["fix the typo in the readme", "Do these:\n1. read the log\n2. change the loop\n3. check it", "and the lexer too"];

#[test]
fn s_om1_a_line_in_main_runs_the_turn_path_of_single_and_nothing_more_even_a_plan_shaped_one() {
    let run = |tune: fn(&mut Cfg)| {
        let f = fx(vec![chatty("claude", &["a", "b", "c"])], tune);
        LINES.iter().for_each(|l| {
            f.turn(l);
        });
        f
    };
    let (s, o) = (run(single), run(orchestrate));
    // every kind of event, as often: no row, no ask, no plan, no chat that `single` does not have
    assert_eq!(kinds(&s), kinds(&o), "{}", o.rig.dump());
    let shape = |f: &Fx| f.tasks().iter().map(|r| (r.spec.doer.clone(), r.spec.role.clone(), r.spec.thread.clone(), r.spec.follows.is_some())).collect::<Vec<_>>();
    assert_eq!(shape(&s), shape(&o), "the turns are the same turns, in Main's own session");
    assert_eq!(o.front.binding("main").map(|b| b.doer), Some("claude".to_string()));
    for k in ["chat.opened", "chat.routed", "chat.route.asked", "plan.asked", "plan.routed", "intake.decided"].into_iter().filter(|k| s.count(k) == 0) {
        assert_eq!(o.count(k), 0, "{k}");
    }
    assert!(o.front.chats().is_empty(), "Main's turns open no chat");
    // the control: the same plan-shaped line in `auto` does begin a plan, so the equality above is a test of the line's path
    let a = fx(vec![chatty("claude", &["a"])], auto);
    a.say(LINES[1]);
    assert_eq!(a.count("plan.asked"), 1, "{}", a.rig.dump());
}

#[test]
fn s_om0_under_single_nothing_of_the_mode_is_asked_bound_or_shown() {
    let f = fx_deciding(vec![chatty("claude", &["a"]), chatty("pi", &["b"])], Some("who:main=pi"), |c| c.chats.screen = true);
    f.turn("fix the typo in the readme");
    assert!(f.asked.lock().unwrap().iter().all(|q| q.point != "who:main"), "single elects as it did: who:work");
    assert!(!f.front.chats_on() && !f.front.main_works());
    assert_eq!(f.bound()[0].data["doer"], "claude", "the smallest id, as `single` always chose it");
}

#[test]
fn the_word_turns_the_chats_on_where_a_screen_uses_them_and_nowhere_else_and_chats_on_stays_its_own_switch() {
    let f = fx(vec![chatty("claude", &["a"])], orchestrate);
    assert!(f.front.chats_on() && f.front.main_works());
    let f = fx(vec![chatty("claude", &["a"])], |c| {
        c.lanes = Switch::Orchestrate;
    });
    assert!(!f.front.chats_on() && !f.front.main_works(), "a run or a pipe has no orchestrator to be Main");
    let f = fx(vec![chatty("claude", &["a"])], |c| c.chats.on = true);
    assert!(f.front.chats_on() && !f.front.main_works(), "`chats.on` alone: the orchestrator has no session");
    // before a line there is no session and no chat
    let f = fx(vec![chatty("claude", &["a"])], orchestrate);
    assert!(f.front.binding("main").is_none() && f.front.chats().is_empty() && f.bound().is_empty());
}

#[test]
fn the_election_of_main_is_the_operators_bound_doer_then_the_decider_then_the_cheapest() {
    let both = || vec![chatty("claude", &["a"]), chatty("pi", &["b"])];
    let main = |c: &mut Cfg| {
        c.lanes = Switch::Orchestrate;
        c.chats.screen = true;
    };
    // 1. the doer the operator bound for the main lane (`:setup doer`, `lane.doer`) is Main, and nobody is asked
    let f = fx_deciding(both(), Some("who:main=claude"), |c| {
        main(c);
        c.main.doer = Some("pi".into());
    });
    f.turn("fix the typo in the readme");
    assert_eq!((f.bound()[0].data["doer"].as_str(), f.bound()[0].data["how"].as_str()), (Some("pi"), Some("picked")));
    assert!(f.asked.lock().unwrap().iter().all(|q| q.point != "who:main"));
    // 2. none bound: the Decider chooses at `who:main`, and the election is said as an election
    let f = fx_deciding(both(), Some("who:main=pi"), main);
    f.turn("fix the typo in the readme");
    assert_eq!((f.bound()[0].data["doer"].as_str(), f.bound()[0].data["how"].as_str()), (Some("pi"), Some("elected")));
    let asked: Vec<String> = f.asked.lock().unwrap().iter().map(|q| q.point.clone()).collect();
    assert!(asked.contains(&"who:main".to_string()) && !asked.contains(&"who:work".to_string()), "{asked:?}");
    // 3. no Decider: the ranking decides; with no record it is the recommended and then the smallest id
    let f = fx_deciding(both(), Some("absent"), main);
    f.turn("fix the typo in the readme");
    assert_eq!(f.bound()[0].data["doer"], "claude");
}

#[test]
fn mains_strip_names_its_doer_and_model_where_the_orchestrators_had_none_and_main_works_its_own_session() {
    use crate::scripted::{Beat, Scripted, end, say};
    let one = || vec![Scripted::new("claude", vec![vec![Beat::Session("s".into()), Beat::Served("claude-sonnet-5-5", "anthropic"), say("a"), end("a")], vec![Beat::Session("s".into()), say("b"), end("b")]])];
    let strip = |s: &mut Serve| {
        let (downs, _) = s.up(Up::Watch { bind: COMPOSER_STRIP.into() });
        downs.iter().rev().find_map(|d| match d {
            Down::Value { bind, value, .. } if bind == COMPOSER_STRIP => Some(value.clone()),
            _ => None,
        })
    };
    let f = fx(one(), |c| {
        orchestrate(c);
        c.model = Some("sonnet".into());
    });
    let mut s = Serve::new(f.front.clone());
    assert_eq!(strip(&mut s).and_then(|v| v["route"].as_str().map(String::from)), Some("no doer yet".into()), "before its first line Main has no doer");
    let first = f.turn("hello");
    let v = strip(&mut s).expect("a strip");
    let route = v["route"].as_str().unwrap_or_default();
    assert!(route.contains("claude") && route.contains("sonnet"), "`doer · model`: {v}");
    assert!(v["tags"].to_string().contains("lanes orchestrate"), "{v}");
    // the next line goes on in Main's own session, and `:fresh` ends it: it acts on Main
    let second = f.turn("and again");
    assert_eq!(second.spec.follows.as_deref(), Some(first.id.as_str()), "{}", f.rig.dump());
    f.say(":fresh");
    assert_eq!(f.count("thread.fresh"), 1, "{}", f.rig.dump());
    assert!(!f.notes().iter().any(|n| n.contains("no session of its own")), "{:?}", f.notes());
    // under `chats.on` alone the orchestrator has no strip and no session, as it had
    let g = fx(one(), |c| c.chats.on = true);
    let mut s = Serve::new(g.front.clone());
    assert_eq!(strip(&mut s), Some(Value::Null));
    g.say(":fresh");
    assert!(g.notes().iter().any(|n| n.contains("no session of its own")));
}
