//! The crew that works for quality (`crew.goal`): a doer is picked by its pass record and how steady it was, with the board's scores standing in while the
//! record is thin; the host by its record; a writing chat's review and a critic at the end; the cap on reviews at once. With the goal left at `cost` nothing
//! of it is read.

use crate::branch_validate::CLEAN;
use crate::scripted::{Beat, Scripted, end};
use crate::who_cases::who;
use j4n_core::change::Actor;
use j4n_core::crew::{self, Goal, Seed};
use j4n_core::flow::run::Elect;
use j4n_core::front::hosts::{Hosts, Pick, Want};
use j4n_core::front::who::{Standing, Who};
use j4n_core::rank::Columns;
use j4n_core::task::{Mode, TaskSpec};
use std::sync::{Arc, Mutex};

// ---- the numbers ----

#[test]
fn a_record_is_a_smoothed_rate_less_what_unsteadiness_takes_and_a_doer_that_fails_every_time_is_not_helped_by_being_steady() {
    let passes = [true; 6];
    let alternating = [true, false, true, false, true, false];
    let blocks = [true, true, true, false, false, false];
    let fails = [false; 6];
    let q = |o: &[bool]| crew::quality_of(o);
    // six passes: (6 + 1) / (6 + 2), no flips
    assert!((q(&passes) - 7.0 / 8.0).abs() < 1e-9);
    // the same rate, three and three: the one that passes and fails by turns is the less steady and scores less
    assert!(q(&blocks) > q(&alternating) && (q(&alternating) - 0.5 * (1.0 - crew::FLIP_PENALTY)).abs() < 1e-9, "{} {}", q(&blocks), q(&alternating));
    // a doer that fails every time never flips and keeps its low rate: (0 + 1) / (6 + 2)
    assert!((q(&fails) - 1.0 / 8.0).abs() < 1e-9);
    assert!(q(&fails) < q(&alternating));
    // only the latest results count
    let mut long = vec![false; 20];
    long.extend([true; 12]);
    assert!((q(&long) - 13.0 / 14.0).abs() < 1e-9);
}

#[test]
fn the_board_stands_in_only_while_the_record_is_thin_and_never_beside_a_record_that_is_enough() {
    let seed = Some(0.639);
    let enough = [true, true, true, true, true];
    // five results with `record_min` five: the record, whatever the board says
    assert!((crew::quality_column(&enough, seed, 5).unwrap() - 6.0 / 7.0).abs() < 1e-9);
    // four results: too few, the board's score
    assert_eq!(crew::quality_column(&enough[..4], seed, 5), seed);
    // no row for the doer: what little record there is, or nothing at all
    assert!((crew::quality_column(&enough[..4], None, 5).unwrap() - 5.0 / 6.0).abs() < 1e-9);
    assert_eq!(crew::quality_column(&[], None, 5), None);
    assert_eq!(crew::quality_column(&[], seed, 5), seed);
    // a room for a review while fewer than the cap are running
    assert!(crew::checkers_room(1, 2) && !crew::checkers_room(2, 2) && crew::checkers_room(0, 0));
}

#[test]
fn the_seeds_are_the_board_as_read_on_the_day_and_a_doer_is_given_the_lowest_of_its_rows() {
    let seeds = crew::shipped_seeds();
    assert_eq!(seeds.len(), 5);
    assert!(seeds.iter().all(|s| s.board == "Terminal-Bench" && s.version == "4.1.0" && s.dated == "2026-11-16"), "{seeds:?}");
    // claude has three rows (65.8, 65.8 and 63.9): what it can be counted on for whichever model it runs
    assert_eq!(crew::seed_score(&seeds, "claude"), Some(0.639));
    assert_eq!(crew::seed_score(&seeds, "codex"), Some(0.609));
    // the board's Antigravity is not the Gemini CLI: the row is there and counts for no doer
    assert_eq!(crew::seed_score(&seeds, "gemini"), None);
    assert!(seeds.iter().any(|s| s.harness == "Antigravity CLI" && s.doer.is_empty() && s.score == 64.5));
    assert_eq!(crew::seed_score(&[] as &[Seed], "claude"), None);
}

// ---- the pick of a doer ----

fn turns(outcomes: &[bool]) -> Vec<Vec<Beat>> {
    outcomes.iter().map(|ok| if *ok { vec![end("ok")] } else { vec![Beat::Fail("it broke".into())] }).collect()
}

/// Two doers with the records given (`true`: a task of the role `work` that ended well), `record_min` as set, and the goal as given.
fn election(claude: &[bool], codex: &[bool], quality: bool, record_min: usize) -> (Who, Standing) {
    let goal = Goal::default();
    goal.set(quality, record_min);
    let standing = Standing::new().with_quality(goal.clone(), crew::shipped_seeds());
    let (a, b) = (Scripted::new("claude", turns(claude)), Scripted::new("codex", turns(codex)));
    let (w, rig) = who(&[a, b], &[], &standing, Arc::new(j4n_core::front::hosts::NoHosts));
    for (doer, n) in [("claude", claude.len()), ("codex", codex.len())] {
        for _ in 0..n {
            let mut spec = TaskSpec::new("go", doer);
            spec.role = Some("work".into());
            let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
            rig.finished(&id);
        }
    }
    (w.with_goal(goal), standing)
}

fn pick(w: &Who) -> (String, String) {
    let (doer, _, why) = w.doer("work", None, "do it", &[]).unwrap();
    (doer, why)
}

#[test]
fn with_the_goal_at_cost_the_pick_is_what_it_was_and_the_extra_number_is_not_made() {
    let (alternating, blocks) = ([true, false, true, false, true, false], [true, true, true, false, false, false]);
    let (cost, standing) = election(&alternating, &blocks, false, 2);
    // the same rate, so the smaller id; and no `quality` column for anyone
    assert_eq!(pick(&cost).0, "claude");
    assert!(!standing.columns("who:work", "claude").contains_key("quality") && !standing.quality());
    // a standing with no crew at all picks the same, with the same words
    let plain = Standing::new();
    let (w, _) = who(&[Scripted::new("claude", turns(&alternating)), Scripted::new("codex", turns(&blocks))], &[], &plain, Arc::new(j4n_core::front::hosts::NoHosts));
    assert_eq!(pick(&cost).1, w.doer("work", None, "do it", &[]).unwrap().2, "the same words");
}

#[test]
fn under_quality_the_steadier_doer_wins_a_tie_of_rates_and_the_record_beats_the_board() {
    let (alternating, blocks) = ([true, false, true, false, true, false], [true, true, true, false, false, false]);
    // the same pass rate; claude's results flip at every turn, codex's once: quality prefers codex where cost went by the smaller id
    let (w, standing) = election(&alternating, &blocks, true, 2);
    assert_eq!(pick(&w).0, "codex");
    assert!(standing.columns("who:work", "codex")["quality"] > standing.columns("who:work", "claude")["quality"]);
    // a record that is enough beats the board: claude's four passes (5/6) against codex's seed (0.609)
    let (w, _) = election(&[true; 4], &[], true, 3);
    assert_eq!(pick(&w).0, "claude");
    // a doer that fails every time is no better for being steady, and a doer with only the board's score is picked over it
    let (w, standing) = election(&[false; 4], &[], true, 3);
    assert!(standing.columns("who:work", "claude")["quality"] < 0.2);
    assert_eq!(pick(&w).0, "codex");
}

#[test]
fn a_thin_record_is_read_as_the_boards_score_and_a_lead_inside_the_boards_spread_is_said_not_to_be_clear() {
    // no results at all: claude 0.639 and codex 0.609 are three points apart, and the board's own spread is three
    let (w, standing) = election(&[], &[], true, 5);
    assert_eq!((standing.columns("who:work", "claude")["quality"], standing.columns("who:work", "codex")["quality"]), (0.639, 0.609));
    let (doer, why) = pick(&w);
    assert_eq!(doer, "claude");
    assert!(why.contains("no clear lead"), "{why}");
    // three passes with `record_min` five are still thin: the column is the board's, not the record's 0.8
    let (_, standing) = election(&[true; 3], &[], true, 5);
    assert_eq!(standing.columns("who:work", "claude")["quality"], 0.639);
    // five are enough: the record
    let (_, standing) = election(&[true; 5], &[], true, 5);
    assert!((standing.columns("who:work", "claude")["quality"] - 6.0 / 7.0).abs() < 1e-9);
}

// ---- the host ----

struct Seen(Mutex<Vec<bool>>);

impl Hosts for Seen {
    fn elect(&self, _: &str, _: Option<&str>, want: &Want, _: u64) -> Option<Pick> {
        self.0.lock().unwrap().push(want.quality);
        None
    }

    fn cooling(&self, _: &str, _: Option<&str>, _: &str, _: u64) -> Option<String> {
        None
    }

    fn another(&self, _: &str, _: Option<&str>, _: &str, _: &str, _: u64) -> Option<(Pick, String)> {
        None
    }
}

#[test]
fn the_election_of_a_host_is_asked_for_quality_only_while_the_goal_is_quality() {
    for (quality, want) in [(false, false), (true, true)] {
        let goal = Goal::default();
        goal.set(quality, 5);
        let seen = Arc::new(Seen(Mutex::default()));
        let (w, _rig) = who(&[Scripted::new("claude", vec![vec![end("x")]])], &[], &Standing::new(), seen.clone());
        w.with_goal(goal).elect("work", "go", None, &[], Mode::Write).unwrap();
        assert_eq!(*seen.0.lock().unwrap(), [want]);
    }
}

// ---- the reviews, the critic and the chats ----

use crate::branch_validate::{Held, Rig3, checker, doer, no_replan, pieces, rig_cfg};
use crate::front_cases::fx_placed;
use crate::split_cases::{LOOK, answer, brief, chat_tasks, events, say, settings, settle};
use j4n_core::crew::Crew;
use j4n_core::front::Cfg;
use serde_json::json;

fn quality(c: &mut Cfg) {
    no_replan(c);
    c.crew = Crew { goal: "quality".into(), ..Crew::shipped() };
}

fn one_checker(c: &mut Cfg) {
    no_replan(c);
    c.crew = Crew { goal: "quality".into(), checkers: 1, ..Crew::shipped() };
}

fn cost(c: &mut Cfg) {
    no_replan(c);
    c.crew = Crew { goal: "cost".into(), ..Crew::shipped() };
}

/// The goal says quality and the switch is off: the crew is not on.
fn switch_off(c: &mut Cfg) {
    no_replan(c);
    c.crew = Crew { on: false, goal: "quality".into(), ..Crew::shipped() };
}

/// A reviewer that takes a while, so that reviews started together are running together.
fn slow_reviewer(n: usize) -> Arc<Scripted> {
    Scripted::with_caps("own", crate::branch_validate::caps(), (0..n).map(|_| vec![Beat::Session("own-s".into()), Beat::Pause(400), Beat::Say(CLEAN.into()), end(CLEAN)]).collect())
}

fn reviews(r: &Rig3) -> Vec<j4n_core::task::Row> {
    r.f.tasks().into_iter().filter(|t| t.spec.role.as_deref() == Some("review")).collect()
}

#[test]
fn under_quality_a_review_is_told_the_reviewers_words_and_where_to_put_a_finding_and_a_finding_still_sends_the_piece_back() {
    let concern = json!({ "answer": "no", "sure": "mid", "open": ["src/a/1.rs:3 the retry is never reset"] }).to_string();
    for (cfg, words) in [(quality as fn(&mut Cfg), true), (cost, false), (switch_off, false)] {
        let r = rig_cfg(pieces(), doer("pi", &["did"; 6]), doer("own", &[&concern, CLEAN, CLEAN, CLEAN, CLEAN]), checker(12), &["end done"], cfg);
        r.begin();
        r.until("a piece was sent back", |r| !r.sent_back().is_empty());
        let back = r.sent_back().remove(0);
        assert!(back.data["result"].as_str().unwrap().contains("the reviewer's concerns: src/a/1.rs:3 the retry is never reset"), "{back:?}");
        let goal = reviews(&r)[0].spec.goal.clone();
        assert_eq!((goal.contains("You are the reviewer of this part"), goal.contains("Put each case that fails in `open`")), (words, words), "{goal}");
        // the review itself is what it always was: read only, another doer, a clean context
        assert!(goal.contains("Read only. In `open` put only what fails") && reviews(&r)[0].spec.doer == "own");
    }
}

#[test]
fn under_quality_only_so_many_reviews_run_at_once_a_held_one_goes_on_when_a_place_frees_and_under_cost_there_is_no_cap() {
    let most = |cfg: fn(&mut Cfg)| {
        let r = rig_cfg(pieces(), doer("pi", &["did"; 6]), slow_reviewer(6), checker(12), &["end done", "fine"], cfg);
        r.begin();
        r.until("the plan was done", |r| r.item("end").state == "merged");
        // reviews at once, from the Log: a review is running from its start to its end
        let ids: Vec<String> = reviews(&r).into_iter().map(|t| t.id).collect();
        let mut at_once = (0usize, 0usize);
        for e in r.f.rig.events().into_iter().filter(|e| e.task.as_ref().is_some_and(|t| ids.contains(t))) {
            match e.kind.as_str() {
                "task.started" => at_once = (at_once.0 + 1, at_once.1.max(at_once.0 + 1)),
                "task.settled" => at_once.0 = at_once.0.saturating_sub(1),
                _ => {}
            }
        }
        assert_eq!(ids.len(), 3, "every piece was reviewed, the held ones when a place freed: {:?}\n{}", r.states(), r.f.rig.dump());
        at_once.1
    };
    assert_eq!(most(one_checker), 1);
    assert_eq!(most(quality), 2, "two is the default");
    assert_eq!(most(cost), 3, "without the crew every piece is reviewed at once");
}

#[test]
fn under_quality_a_critic_reads_the_whole_change_once_when_the_plan_ends_and_a_difference_is_a_decision_for_the_operator() {
    let differs = json!({ "answer": "it differs", "sure": "mid", "open": ["the operator asked for steady retries, the change adds a flag"] }).to_string();
    let r = rig_cfg(pieces(), doer("pi", &["did p1", "did p2", "did p3", &differs]), doer("own", &[CLEAN; 4]), checker(12), &["end done", &differs], quality);
    // the operator's own words, kept by the split that began the plan
    use j4n_core::log::New;
    r.f.rig.log.append(New::of("crew.verbatim", Actor::Operator, json!({ "line": "make the retries steady" })).thread("main")).unwrap();
    r.begin();
    r.until("the critic was added", |r| r.f.keeper.plan("main").is_some_and(|p| p.items.iter().any(|i| i.id == "critic")));
    r.until("the critic's words were said", |r| r.f.rig.of_kind("plan.reported").iter().any(|e| e.data["outcome"] == "done"));
    let plan = r.f.keeper.plan("main").unwrap();
    assert_eq!(plan.items.iter().filter(|i| i.id == "critic").count(), 1, "once for a plan");
    let critic = r.f.tasks().into_iter().find(|t| t.spec.input["item"] == "critic").expect("the critic ran");
    assert_eq!((critic.spec.tune.mode, critic.spec.role.as_deref()), (Mode::Read, Some("scout")));
    assert!(critic.spec.goal.contains("You are the critic of this whole change") && critic.spec.goal.contains("> make the retries steady") && critic.spec.goal.contains("- p2:"), "{}", critic.spec.goal);
    let said = r.f.rig.of_kind("plan.reported").into_iter().find(|e| e.data["outcome"] == "done").unwrap();
    assert!(said.data["decisions"].as_array().unwrap().iter().any(|d| d.as_str().unwrap().contains("the change differs from your words") && d.as_str().unwrap().contains("adds a flag")), "{said:?}");
    // under cost, or with the switch off, no critic is made and the plan ends as it always did
    for cfg in [cost as fn(&mut Cfg), switch_off] {
        let r = rig_cfg(pieces(), doer("pi", &["did p1", "did p2", "did p3"]), doer("own", &[CLEAN; 4]), checker(12), &["end done"], cfg);
        r.begin();
        r.until("the plan was done", |r| r.item("end").state == "merged");
        r.step();
        assert!(r.f.keeper.plan("main").unwrap().items.iter().all(|i| i.id != "critic"), "{:?}", r.states());
    }
}

fn four_chats(crew: bool, quality: bool) -> usize {
    let held = Held::new();
    let paths: [[&str; 3]; 4] = [["src/a/1.rs", "src/a/2.rs", "src/a/3.rs"], ["src/b/1.rs", "src/b/2.rs", "src/b/3.rs"], ["src/c/1.rs", "src/c/2.rs", "src/c/3.rs"], ["src/d/1.rs", "src/d/2.rs", "src/d/3.rs"]];
    let plan = answer((0..4).map(|n| { let mut b = brief(&format!("part-{n}"), false, &paths[n], &[]); b["doer"] = json!("pi"); b }).collect());
    let doers = vec![crate::front_cases::chatty("claude", &[&plan, "main worked"]), doer("pi", &["a", "b", "c", "d", "e"]), crate::front_cases::chatty("cheap", &[LOOK])];
    let f = fx_placed(doers, None, vec![held.place()], |c: &mut Cfg| {
        settings(c);
        c.usd_line = 2.0;
        if crew {
            c.crew = Crew { goal: if quality { "quality" } else { "cost" }.into(), ..Crew::shipped() };
        }
    });
    say(&f);
    settle(&f);
    chat_tasks(&f).len()
}

#[test]
fn under_quality_a_line_may_start_four_chats_where_orchestrate_chats_says_three_and_nothing_changes_otherwise() {
    assert_eq!(four_chats(true, true), 4);
    assert_eq!(four_chats(true, false), 3, "the goal is cost: `orchestrate.chats` stands");
    assert_eq!(four_chats(false, false), 3, "no crew: as before");
}

#[test]
fn under_quality_the_split_keeps_the_operators_line_for_the_critic_and_scrubs_it() {
    let held = Held::new();
    let plan = answer(vec![brief("only", false, &["src/a/1.rs", "src/a/2.rs", "src/a/3.rs"], &[])]);
    for (crew_on, want) in [(true, 1), (false, 0)] {
        let doers = vec![crate::front_cases::chatty("claude", &[&plan, "main worked"]), doer("pi", &["a"]), crate::front_cases::chatty("cheap", &[LOOK])];
        let f = fx_placed(doers, None, vec![held.place()], |c: &mut Cfg| {
            settings(c);
            c.crew = Crew { on: crew_on, goal: "quality".into(), ..Crew::shipped() };
        });
        say(&f);
        settle(&f);
        assert_eq!(events(&f, "crew.verbatim").len(), want);
    }
}

#[test]
fn the_goal_follows_the_switch_and_the_word_and_the_crew_reads_its_numbers_from_the_settings() {
    use j4n_core::setting::{Settings, Tune};
    use j4n_core::table::Tables;
    use j4n_core::value::Value;
    let settings = |on: bool, goal: &str, checkers: f64, min: f64| Settings::from_pairs([("crew.on", Value::Bool(on)), ("crew.goal", Value::text(goal)), ("crew.checkers", Value::num(checkers)), ("crew.record_min", Value::num(min))]);
    for (on, goal, want) in [(true, "quality", true), (true, "cost", false), (false, "quality", false), (false, "cost", false)] {
        let g = Goal::default();
        g.tune(&settings(on, goal, 3.0, 7.0));
        assert_eq!((g.quality(), g.record_min()), (want, 7), "{on} {goal}");
        let (crew, _) = Crew::load(&Tables::load(Vec::new()).0, &settings(on, goal, 3.0, 7.0));
        assert_eq!((crew.quality(), crew.checkers), (want, 3), "{on} {goal}");
    }
    // nothing set: cost, two reviews, five results
    let (crew, _) = Crew::load(&Tables::load(Vec::new()).0, &Settings::default());
    assert_eq!((crew.quality(), crew.checkers), (false, 2));
    let g = Goal::default();
    g.tune(&Settings::default());
    assert_eq!((g.quality(), g.record_min()), (false, 5));
}
