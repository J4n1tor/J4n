//! Through the loop: who answers a question before the operator does (the chain of its ask point), what the Gate
//! says of an approval, what *always here* does to the next one, and what the loop will not do whatever an answerer says.
//! The loop battery's scenarios of the ask points, the gate side of "always here", and a task's decision rows.

use crate::scripted::{Beat, Rig, Scripted, approval, choice, end, load_base, pick, say};
use crate::world::{Scratch, layer, load};
use j4n_core::answer::chain::Registry;
use j4n_core::answer::point::{parse_step, points};
use j4n_core::answer::state::Answerers;
use j4n_core::answer::{Answerer, AskPoint, Fallback, Impact, Kind, Offer, Question, ShapeKind};
use j4n_core::ask::{Answer, AskSpec};
use j4n_core::binder::{Bind, bind};
use j4n_core::change::Actor;
use j4n_core::doer::RunCtl;
use j4n_core::fold::declared;
use j4n_core::gate::check::Gates;
use j4n_core::gate::grant::{Grants, Scope, here_dir, here_layer};
use j4n_core::gate::{Action, Gate, Verb, Verdict};
use j4n_core::log::Scope as LogScope;
use j4n_core::schema::DataClass;
use j4n_core::table::LayerKind;
use j4n_core::task::{Asks, Ctl, State};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

/// An answerer that says what it is told to and counts the times it was asked.
struct Fake {
    id: String,
    kind: Kind,
    calls: AtomicUsize,
    f: Box<dyn Fn(&Question) -> Offer + Send + Sync>,
}

impl Fake {
    fn new(id: &str, kind: Kind, f: impl Fn(&Question) -> Offer + Send + Sync + 'static) -> Arc<Fake> {
        Arc::new(Fake { id: id.into(), kind, calls: AtomicUsize::new(0), f: Box::new(f) })
    }

    fn says(id: &str, kind: Kind, answer: Answer) -> Arc<Fake> {
        Fake::new(id, kind, move |_| Offer::Answer { answer: answer.clone(), conf: 0.9, why: "it knows".into() })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Answerer for Fake {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind(&self) -> Kind {
        self.kind
    }
    fn offer(&self, q: &Question) -> Offer {
        self.calls.fetch_add(1, Ordering::SeqCst);
        (self.f)(q)
    }
}

fn point(id: &str, shape: ShapeKind, impact: Impact, chain: &[&str], fallback: Fallback) -> AskPoint {
    AskPoint {
        id: id.into(),
        doc: "a test point".into(),
        shape,
        impact,
        data: DataClass::Project,
        chain: chain.iter().map(|s| parse_step(s).unwrap()).collect(),
        fallback,
        rank: None,
        breaker: true,
    }
}

fn asks(points: Vec<AskPoint>, fakes: &[&Arc<Fake>]) -> Asks {
    let mut reg = Registry::new();
    for f in fakes {
        assert!(reg.register((*f).clone()));
    }
    Asks::new(points, reg, Answerers::new())
}

fn with(a: Asks) -> impl FnOnce(&mut j4n_core::task::Config) {
    move |c| c.asks = Some(Arc::new(a))
}

fn waiting_on(rig: &Rig, id: &str) -> String {
    match rig.until(id, "waited", |r| matches!(r.state, State::Waiting { .. })).state {
        State::Waiting { ask } => ask,
        other => panic!("{other:?}"),
    }
}

fn asked(local: &str, q: &str, point: &str, options: &[(&str, bool)]) -> AskSpec {
    AskSpec { point: Some(point.into()), ..choice(local, q, options) }
}

fn answered(rig: &Rig) -> Vec<j4n_core::log::Ev> {
    rig.of_kind("ask.answered")
}

// 13 — a question something can answer never reaches the operator, and the same child goes on.
#[test]
fn an_answerable_question_never_reaches_the_operator() {
    let rule = Fake::says("rule:r", Kind::Rule, pick("b"));
    let a = asks(vec![point("pick", ShapeKind::Choice, Impact::Low, &["rule:r", "human"], Fallback::Rank)], &[&rule]);
    let doer = Scripted::new("script", vec![vec![Beat::Ask(asked("q1", "which?", "pick", &[("a", false), ("b", true)])), Beat::Hold, say("went on"), end("went on")]]);
    let rig = Rig::tuned(vec![doer.clone()], with(a));
    let id = rig.spawn("choose", "script");
    let row = rig.finished(&id);
    assert_eq!(row.state, State::Done { ok: true });
    assert_eq!(row.answer.as_deref(), Some("went on"));
    assert_eq!(doer.starts().len(), 1, "the same child, not a re-run");
    assert_eq!(doer.ctls(), vec![RunCtl::Answer { local: "q1".into(), answer: pick("b") }]);
    let said = answered(&rig);
    assert_eq!(said.len(), 1);
    let (d, ev) = (&said[0].data, &said[0]);
    assert_eq!((d["by"].as_str(), d["via"].as_str(), d["point"].as_str(), d["kind"].as_str()), (Some("rule:r"), Some("chain"), Some("pick"), Some("rule")));
    assert_eq!(ev.task.as_deref(), Some(id.as_str()), "the decision belongs to the task that asked");
    assert_eq!(ev.by, Actor::Rule("rule:r".into()));
    let ask = d["ask"].as_str().unwrap();
    let child = rig.tasks.book().get(ask).unwrap();
    assert_eq!(child.state, State::Done { ok: true });
    assert!(rig.tasks.ctl(ask, Ctl::Answer(pick("a")), Actor::Operator).is_err(), "nobody is left to answer it");
    assert_eq!(rule.calls(), 1);
}

// 14 — a question nothing can answer goes to the operator exactly as it was put.
#[test]
fn a_question_nothing_can_answer_goes_to_the_operator_unchanged() {
    let passer = Fake::new("rule:p", Kind::Rule, |_| Offer::Pass { why: "no idea".into() });
    let liar = Fake::says("rule:l", Kind::Rule, pick("zzz")); // not an offered option
    let a = asks(vec![point("pick", ShapeKind::Choice, Impact::Low, &["rule:p", "rule:l", "human"], Fallback::Rank)], &[&passer, &liar]);
    let doer = Scripted::new("script", vec![vec![Beat::Ask(asked("q1", "which?", "pick", &[("a", false), ("b", true)])), Beat::Hold, end("ok")]]);
    let rig = Rig::tuned(vec![doer.clone()], with(a));
    let id = rig.spawn("choose", "script");
    let ask = waiting_on(&rig, &id);
    let steps = |rig: &Rig| rig.of_kind("ask.step").len();
    for _ in 0..4000 {
        if steps(&rig) >= 2 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let outcomes: Vec<String> = rig.of_kind("ask.step").iter().map(|e| e.data["outcome"].as_str().unwrap().to_string()).collect();
    assert_eq!(outcomes, ["pass", "failed"], "{}", rig.dump());
    assert_eq!(rig.tasks.book().get(&ask).unwrap().state, State::Running, "the question is open");
    assert!(doer.ctls().is_empty(), "the child was not touched");
    assert!(answered(&rig).is_empty());
    let q = rig.tasks.book().get(&ask).unwrap().spec.ask().unwrap();
    assert_eq!(q.question, "which?");
    rig.tasks.ctl(&ask, Ctl::Answer(pick("a")), Actor::Operator).unwrap();
    assert_eq!(rig.finished(&id).state, State::Done { ok: true });
    let said = answered(&rig);
    assert_eq!((said[0].data["via"].as_str(), said[0].by.clone()), (Some("human"), Actor::Operator));
}

// 15 — an approval is never put to the answerer, whatever the point says.
#[test]
fn an_approval_is_never_put_to_the_answerer() {
    let model = Fake::says("decider:d", Kind::Decider, Answer::Yes);
    let lm = Fake::says("model:m", Kind::Model, Answer::Yes);
    let a = asks(vec![point("pick", ShapeKind::Choice, Impact::Low, &["decider:d", "model:m", "human"], Fallback::Safe)], &[&model, &lm]);
    let spec = AskSpec { point: Some("pick".into()), ..approval("q1", "make build") };
    let doer = Scripted::new("script", vec![vec![Beat::Ask(spec), Beat::Hold, end("ok")]]);
    let rig = Rig::tuned(vec![doer.clone()], with(a));
    let id = rig.spawn("build", "script");
    let ask = waiting_on(&rig, &id);
    rig.until_event("the walk's steps", |e| e.kind == "ask.step" && e.data["answerer"] == "model:m");
    assert_eq!((model.calls(), lm.calls()), (0, 0));
    assert_eq!(rig.tasks.book().get(&ask).unwrap().state, State::Running);
    // and nothing that is not the operator can make the child say yes
    assert!(rig.tasks.ctl(&ask, Ctl::Answer(Answer::Yes), Actor::Model("m".into())).is_err());
    rig.tasks.ctl(&ask, Ctl::Answer(Answer::Yes), Actor::Operator).unwrap();
    assert_eq!(rig.finished(&id).state, State::Done { ok: true });
}

// ---- the Gate ----

const GATES: [&str; 4] = [
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/base/gates/shell.toml")),
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/base/gates/modes.toml")),
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/base/gates/paths.toml")),
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/base/gates/egress.toml")),
];
const POINTS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/base/asks/points.toml"));

/// The base rules above the operator's own file for the project in `rig`, and the base ask points.
fn gate_for(project: &Scratch, home: &Scratch, session: &str) -> (Gate, Vec<AskPoint>) {
    let files: Vec<(&str, &str)> = ["shell.toml", "modes.toml", "paths.toml", "egress.toml"].into_iter().zip(GATES).collect();
    let (here, problems) = here_layer(home.path(), project.path());
    assert!(problems.is_empty(), "{problems:?}");
    let (tables, problems) = load(vec![layer(LayerKind::Pack, "base", &files), layer(LayerKind::Pack, "asks", &[("points.toml", POINTS)]), here]);
    assert!(problems.is_empty(), "{problems:?}");
    let (gates, notes) = Gates::load(&tables);
    assert!(notes.is_empty(), "{notes:?}");
    let (pts, notes) = points(&tables);
    assert!(notes.is_empty(), "{notes:?}");
    (Gate::new(gates, Grants::new(session)), pts)
}

fn act(cmd: &str) -> AskSpec {
    AskSpec { act: Some(Box::new(Action::shell(Actor::Doer("script".into()), cmd))), ..approval("q", cmd) }
}

/// A rig whose loop has the base points and the base Gate for its own project and home.
fn gated(doers: Vec<Arc<Scripted>>) -> Rig {
    gated_with(doers).0
}

fn gated_with(doers: Vec<Arc<Scripted>>) -> (Rig, Gate) {
    gated_in(Scratch::new("proj"), doers)
}

/// [`gated_with`] over a project the test made first, so that the doers' actions can name its folder.
fn gated_in(project: Scratch, doers: Vec<Arc<Scripted>>) -> (Rig, Gate) {
    let home = Scratch::new("home");
    let (gate, pts) = gate_for(&project, &home, "s1");
    let (log, clock) = crate::world::open_log(&project);
    let g = gate.clone();
    let rig = Rig::over(project, home, log, clock, doers, move |c| c.asks = Some(Arc::new(Asks::new(pts, Registry::new(), Answerers::new()).with_gate(g))));
    (rig, gate)
}

/// Under the switch, an edit inside the project is the Gate's yes; a line inside but for the paths Python builds as it runs is put to the Decider of the
/// point `inside`, whose `inside` is the yes and whose `ask` leaves the operator asked; a deploy is asked and no Decider is put it.
#[test]
fn under_the_switch_the_gate_takes_what_stays_inside_and_a_decider_is_put_only_what_code_cannot_read() {
    let looped = "python3 - <<'EOF'\nimport os.path\nfor name in ('a', 'b'):\n    p = os.path.join('tests', 'data', name + '.json')\n    open(p, 'w').write('{}')\nEOF";
    let literal = "python3 - <<'EOF'\np='src/app.py'\ns=open(p).read()\nopen(p,'w').write(s.replace('a','b'))\nEOF";
    // (line, what the Decider says, who says yes: None is the operator)
    let cases = [(literal, "ask", Some("rule:gate")), (looped, "inside", Some("decider:jev")), (looped, "ask", None), ("./scripts/deploy.sh", "inside", None)];
    for (line, said, by) in cases {
        let (project, home) = (Scratch::new("proj"), Scratch::new("home"));
        let (gate, pts) = gate_for(&project, &home, "s1");
        gate.set_inside(true);
        let jev = Fake::says("decider:jev", Kind::Decider, pick(said));
        let mut a = Action::shell(Actor::Doer("script".into()), line);
        a.site = Some(project.path().display().to_string());
        let spec = AskSpec { act: Some(Box::new(a)), ..approval("q", line) };
        let doer = Scripted::new("script", vec![vec![Beat::Ask(spec), Beat::Hold, end("done")]]);
        let (log, clock) = crate::world::open_log(&project);
        let (g, j) = (gate.clone(), jev.clone());
        let rig = Rig::over(project, home, log, clock, vec![doer.clone()], move |c| {
            let mut reg = Registry::new();
            assert!(reg.register(j));
            c.asks = Some(Arc::new(Asks::new(pts, reg, Answerers::new()).with_gate(g)));
        });
        let id = rig.spawn("edit", "script");
        match by {
            Some(by) => {
                assert_eq!(rig.finished(&id).state, State::Done { ok: true }, "{line}\n{}", rig.dump());
                let yes = answered(&rig);
                assert_eq!((yes.len(), yes[0].data["by"].as_str()), (1, Some(by)), "{line}: {}", rig.dump());
                assert_eq!(doer.ctls(), vec![RunCtl::Answer { local: "q".into(), answer: Answer::Yes }]);
            }
            None => {
                let ask = waiting_on(&rig, &id);
                // the walk goes on after the task waits: the chain's word for the operator is the last thing it says
                rig.until_event("the operator is asked", |e| e.kind == "ask.note" && e.data["point"] != "inside");
                assert!(answered(&rig).is_empty(), "{line}: the operator is asked\n{}", rig.dump());
                rig.tasks.ctl(&ask, Ctl::Answer(Answer::Yes), Actor::Operator).unwrap();
                assert_eq!(rig.finished(&id).state, State::Done { ok: true });
            }
        }
        // the Decider is put only the line whose paths Python builds as it runs
        assert_eq!(jev.calls(), usize::from(line == looped), "{line}");
    }
}

// an approval answered "always here" is not shown the second time, and the rule is in the operator's file.
#[test]
fn always_here_the_second_identical_approval_is_not_shown() {
    let doer = Scripted::new("script", vec![vec![Beat::Ask(act("make build")), Beat::Hold, end("built")]]);
    let rig = gated(vec![doer.clone()]);
    let first = rig.spawn("build", "script");
    let ask = waiting_on(&rig, &first);
    assert!(answered(&rig).is_empty(), "nobody objected, nobody said yes: the operator is asked");
    rig.tasks.ctl(&ask, Ctl::Grant(Scope::Here), Actor::Operator).unwrap();
    assert_eq!(rig.finished(&first).state, State::Done { ok: true });
    let file = here_dir(rig.home.path(), rig.project.path()).join("gates.toml");
    let text = std::fs::read_to_string(&file).expect("the operator's file for this project");
    assert!(text.contains("target == 'make build'") && text.contains("allow"), "{text}");
    assert!(file.starts_with(rig.home.path()), "outside the project");
    // the same approval again, in another task: answered by the Gate, no ask is shown to anyone
    let second = rig.spawn("build again", "script");
    let row = rig.finished(&second);
    assert_eq!(row.state, State::Done { ok: true }, "{}", rig.dump());
    let said: Vec<_> = answered(&rig).into_iter().filter(|e| e.task.as_deref() == Some(second.as_str())).collect();
    assert_eq!(said.len(), 1);
    assert_eq!((said[0].data["via"].as_str(), said[0].data["by"].as_str()), (Some("chain"), Some("rule:gate")));
    assert!(said[0].data["why"].as_str().unwrap().contains("grant:"), "in this process the session grant takes effect at once: {}", said[0].data);
    // and the file is what lasts: rules built from it, as the next run builds them, allow the line by their own row
    let (later, _) = gate_for(&rig.project, &rig.home, "s2");
    assert!(matches!(later.check(&Action::shell(Actor::Operator, "make build")), Verdict::Allow { by } if by.starts_with("here-")));
    assert!(matches!(later.check(&Action::shell(Actor::Operator, "make deploy")), Verdict::Allow { by } if by == "default"));
    assert_eq!(doer.ctls().iter().filter(|c| matches!(c, RunCtl::Answer { answer: Answer::Yes, .. })).count(), 2);
}

#[test]
fn what_the_gate_blocks_is_refused_without_asking_anyone_and_cannot_be_granted() {
    let doer = Scripted::new("script", vec![vec![Beat::Ask(act("git push origin main")), Beat::Hold, end("done")]]);
    let rig = gated(vec![doer.clone()]);
    let id = rig.spawn("push", "script");
    assert_eq!(rig.finished(&id).state, State::Done { ok: true });
    // the rule's reason goes to the doer first, so it can tell its model
    assert_eq!(doer.ctls(), vec![RunCtl::Told { local: "q".into(), why: "pushing is yours".into() }, RunCtl::Answer { local: "q".into(), answer: Answer::No }]);
    let v = rig.of_kind("gate.verdict");
    assert_eq!((v.len(), v[0].data["verdict"].as_str()), (1, Some("block")), "{}", rig.dump());
    assert!(!here_dir(rig.home.path(), rig.project.path()).exists());
}

/// The evidence of run `chores1`, Log seq 70: one line the Gate refused, two `python3 -I` import errors and the task's own red test made four
/// "failed commands" for the rule `stuck`. A refusal is not an attempt and neither is the first run of the test before anything is changed; the threshold
/// stays at four, and the same lines after a change count.
#[test]
fn a_refused_line_and_the_first_red_test_are_not_failed_attempts_for_the_rule_stuck() {
    let steers = |d: &Scripted| -> Vec<String> { d.ctls().into_iter().filter_map(|c| if let RunCtl::Steer(t) = c { Some(t) } else { None }).collect() };
    let shell = |cmd: &str, out: &str| Beat::Shell(cmd.into(), out.into(), 1);
    let evidence = || {
        vec![
            Beat::Session("s".into()),
            Beat::Ask(act("git push origin main")),
            Beat::Hold,
            shell("git push origin main", "this was not allowed"),
            shell("python3 -I -c 'import app'", "ModuleNotFoundError: No module named 'app'"),
            shell("python3 -I -c 'import app.core'", "ModuleNotFoundError: No module named 'app'"),
            shell("cd /work && python3 -I -m pytest -q", "1 failed"),
        ]
    };
    let seen = Scripted::new("seen", vec![[evidence(), vec![Beat::Forever]].concat()]);
    let after = Scripted::new("after", vec![[evidence(), vec![Beat::Changed(vec!["app.py".into()]), shell("python3 -I -m pytest -q", "1 failed"), shell("python3 -I -m pytest -q", "1 failed"), Beat::Forever]].concat()]);
    let (project, home) = (Scratch::new("proj"), Scratch::new("home"));
    let (gate, pts) = gate_for(&project, &home, "s1");
    let (log, clock) = crate::world::open_log(&project);
    let rig = Rig::over(project, home, log, clock, vec![seen.clone(), after.clone()], move |c| {
        load_base(c, "");
        c.asks = Some(Arc::new(Asks::new(pts, Registry::new(), Answerers::new()).with_gate(gate)));
    });
    let a = rig.spawn("fix it", "seen");
    rig.until_event("the red test", |e| e.kind == "run.shell" && e.task.as_deref() == Some(a.as_str()) && e.data["cmd"].as_str().is_some_and(|c| c.contains("pytest")));
    rig.log.sync();
    std::thread::sleep(std::time::Duration::from_millis(100));
    let shells: Vec<_> = rig.of_kind("run.shell").into_iter().filter(|e| e.task.as_deref() == Some(a.as_str())).collect();
    let marks: Vec<_> = shells.iter().map(|e| e.data["uncounted"].as_str()).collect();
    assert_eq!(marks, [Some("refused"), None, None, Some("first test")], "{}", rig.dump());
    let b = rig.spawn("fix it", "after");
    rig.until_event("steered", |e| e.kind == "task.steered" && e.task.as_deref() == Some(b.as_str()));
    let ours = steers(&after);
    assert_eq!(ours.len(), 1, "{ours:?}");
    assert!(ours[0].starts_with("The last 4 commands have failed."), "the threshold is still four, counted from the two imports and the two reds after the change: {}", ours[0]);
    assert!(steers(&seen).is_empty(), "the refused line, two import errors and the first red test: {:?}", steers(&seen));
    for id in [a, b] {
        rig.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
    }
}

#[test]
fn a_grant_is_the_operators_and_only_for_what_can_be_remembered() {
    let doer = Scripted::new("script", vec![vec![Beat::Ask(act("make build; make test")), Beat::Hold, end("ok")]]);
    let rig = gated(vec![doer.clone()]);
    let id = rig.spawn("two things", "script");
    let ask = waiting_on(&rig, &id);
    for scope in [Scope::Task, Scope::Session, Scope::Here] {
        let e = rig.tasks.ctl(&ask, Ctl::Grant(scope), Actor::Operator).unwrap_err();
        assert!(e.msg.contains("not remembered"), "{scope:?}: {e}");
    }
    for by in [Actor::Core, Actor::Model("m".into()), Actor::Doer("d".into()), Actor::Rule("r".into())] {
        assert!(rig.tasks.ctl(&ask, Ctl::Grant(Scope::Once), by).is_err());
    }
    assert_eq!(rig.tasks.book().get(&ask).unwrap().state, State::Running, "all refused, still open");
    rig.tasks.ctl(&ask, Ctl::Grant(Scope::Once), Actor::Operator).unwrap();
    assert_eq!(rig.finished(&id).state, State::Done { ok: true });
    assert!(rig.of_kind("grant").is_empty(), "a yes once remembers nothing");
}

#[test]
fn a_session_grant_covers_the_same_line_in_another_task_and_a_task_grant_does_not() {
    let doer = Scripted::new("script", vec![vec![Beat::Ask(act("make build")), Beat::Hold, end("built")]]);
    let rig = gated(vec![doer.clone()]);
    let first = rig.spawn("one", "script");
    let ask = waiting_on(&rig, &first);
    rig.tasks.ctl(&ask, Ctl::Grant(Scope::Task), Actor::Operator).unwrap();
    rig.finished(&first);
    let second = rig.spawn("two", "script");
    let ask2 = waiting_on(&rig, &second);
    rig.tasks.ctl(&ask2, Ctl::Grant(Scope::Session), Actor::Operator).unwrap();
    rig.finished(&second);
    let third = rig.spawn("three", "script");
    assert_eq!(rig.finished(&third).state, State::Done { ok: true });
    let by_gate = answered(&rig).into_iter().filter(|e| e.data["by"] == "rule:gate").count();
    assert_eq!(by_gate, 1, "{}", rig.dump());
}

/// An approval of a write of `rel` in the project, as the claude doer asks it: inside the project, for a task of the project.
fn write_ask(project: &Scratch, local: &str, rel: &str) -> AskSpec {
    let path = format!("{}/{rel}", project.path().display());
    let mut a = Action::new(Verb::Write, Actor::Doer("script".into()), &path).task("t1");
    a.site = Some(project.path().display().to_string());
    a.inside = Some(true);
    AskSpec { act: Some(Box::new(a)), ..approval(local, &path) }
}

/// An approval of a shell line run in the project.
fn run_ask(project: &Scratch, local: &str, line: &str) -> AskSpec {
    let mut a = Action::shell(Actor::Doer("script".into()), line).task("t1");
    a.site = Some(project.path().display().to_string());
    AskSpec { act: Some(Box::new(a)), ..approval(local, line) }
}

/// The ask `id` waits on once it is about the effect `target` (the asks before it that a grant answered are passed over). Polls: the book
/// cannot be read from inside `until`'s test.
fn wait_for(rig: &Rig, id: &str, target: &str) -> String {
    for _ in 0..2000 {
        let book = rig.tasks.book();
        if let Some(State::Waiting { ask }) = book.get(id).map(|r| r.state)
            && book.get(&ask).and_then(|r| r.spec.ask()).and_then(|a| a.act).is_some_and(|a| a.target.ends_with(target))
        {
            return ask;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("{id} never waited on {target}\n{}", rig.dump());
}

fn by_gate(rig: &Rig) -> usize {
    answered(rig).into_iter().filter(|e| e.data["by"] == "rule:gate").count()
}

// a Write and three Edits of one file in one task: the session grant on the first covers the rest.
#[test]
fn a_session_grant_for_a_file_covers_the_next_edits_of_it_and_asks_once() {
    let project = Scratch::new("proj");
    let beats = ["q1", "q2", "q3", "q4"].iter().flat_map(|q| [Beat::Ask(write_ask(&project, q, "chess.py")), Beat::Hold]).chain([end("played")]).collect();
    let (rig, _) = gated_in(project, vec![Scripted::new("script", vec![beats])]);
    let id = rig.spawn("chess", "script");
    let ask = waiting_on(&rig, &id);
    rig.tasks.ctl(&ask, Ctl::Grant(Scope::Session), Actor::Operator).unwrap();
    assert_eq!(rig.finished(&id).state, State::Done { ok: true }, "{}", rig.dump());
    assert_eq!(by_gate(&rig), 3, "the three edits are answered by the grant, not asked: {}", rig.dump());
    assert_eq!(rig.of_kind("grant").len(), 1, "one grant, written once: {}", rig.dump());
}

// an ask that is already open when the grant lands is answered by it, not left to the operator.
#[test]
fn a_grant_answers_the_question_another_task_had_already_open() {
    let project = Scratch::new("proj");
    let one = Scripted::new("one", vec![vec![Beat::Ask(write_ask(&project, "q1", "chess.py")), Beat::Hold, end("a")]]);
    let two = Scripted::new("two", vec![vec![Beat::Ask(write_ask(&project, "q2", "chess.py")), Beat::Hold, end("b")]]);
    let (rig, _) = gated_in(project, vec![one, two]);
    let (a, b) = (rig.spawn("first", "one"), rig.spawn("second", "two"));
    let (ask_a, _) = (waiting_on(&rig, &a), waiting_on(&rig, &b));
    rig.tasks.ctl(&ask_a, Ctl::Grant(Scope::Session), Actor::Operator).unwrap();
    assert_eq!(rig.finished(&a).state, State::Done { ok: true });
    assert_eq!(rig.finished(&b).state, State::Done { ok: true }, "the second was asked although the grant covers it: {}", rig.dump());
    assert_eq!(by_gate(&rig), 1, "{}", rig.dump());
}

// a write ask offers the folder; the grant covers the writes below it, in this session, inside the project
#[test]
fn a_folder_grant_covers_the_writes_below_it_and_nothing_else() {
    let project = Scratch::new("proj");
    let beats = vec![
        Beat::Ask(write_ask(&project, "q1", "src/a.py")),
        Beat::Hold,
        Beat::Ask(write_ask(&project, "q2", "src/deep/b.py")),
        Beat::Hold,
        Beat::Ask(write_ask(&project, "q3", "docs/c.md")),
        Beat::Hold,
        end("done"),
    ];
    let (rig, gate) = gated_in(project, vec![Scripted::new("script", vec![beats])]);
    let id = rig.spawn("write", "script");
    let ask = waiting_on(&rig, &id);
    let card = |ask: &str| rig.tasks.book().get(ask).unwrap().spec.ask().unwrap().act.map(|a| gate.offers(&a, &gate.check(&a))).unwrap();
    assert_eq!(card(&ask), vec![Scope::Once, Scope::Task, Scope::Session, Scope::Here, Scope::Folder], "the folder is offered with the file");
    rig.tasks.ctl(&ask, Ctl::Grant(Scope::Folder), Actor::Operator).unwrap();
    let docs = wait_for(&rig, &id, "docs/c.md");
    assert_eq!(by_gate(&rig), 1, "src/deep/b.py is below src: {}", rig.dump());
    assert_eq!(rig.of_kind("grant").len(), 1);
    rig.tasks.ctl(&docs, Ctl::Answer(Answer::Yes), Actor::Operator).unwrap();
    assert_eq!(rig.finished(&id).state, State::Done { ok: true }, "{}", rig.dump());
    assert_eq!(by_gate(&rig), 1, "docs/ is another folder and was asked");
}

// the second line of a program that was allowed offers the program; a program grant covers the lines of it in this folder and not what the Gate keeps asking about
#[test]
fn a_program_grant_covers_that_program_in_this_folder_for_this_session() {
    let project = Scratch::new("proj");
    let beats = vec![
        Beat::Ask(run_ask(&project, "q1", "python3 chess.py")),
        Beat::Hold,
        Beat::Ask(run_ask(&project, "q2", "python3 chess.py --fast")),
        Beat::Hold,
        Beat::Ask(run_ask(&project, "q3", "python3 test_snake.py -v")),
        Beat::Hold,
        Beat::Ask(run_ask(&project, "q4", "rm -r build")),
        Beat::Hold,
        end("done"),
    ];
    let (rig, gate) = gated_in(project, vec![Scripted::new("script", vec![beats])]);
    let id = rig.spawn("run", "script");
    let card = |ask: &str| rig.tasks.book().get(ask).unwrap().spec.ask().unwrap().act.map(|a| gate.offers(&a, &gate.check(&a))).unwrap();
    let first = waiting_on(&rig, &id);
    assert_eq!(card(&first), vec![Scope::Once], "nothing was allowed yet: a program that can run anything has no line grant, and no program grant yet");
    rig.tasks.ctl(&first, Ctl::Answer(Answer::Yes), Actor::Operator).unwrap();
    let second = wait_for(&rig, &id, "chess.py --fast");
    assert_eq!(card(&second), vec![Scope::Once, Scope::Program], "python3 was allowed here: the program is offered");
    rig.tasks.ctl(&second, Ctl::Grant(Scope::Program), Actor::Operator).unwrap();
    // the next line of python3 is answered by the grant; a delete is not, it is asked
    let asked = wait_for(&rig, &id, "rm -r build");
    assert_eq!(rig.tasks.book().get(&asked).unwrap().spec.ask().unwrap().act.unwrap().target, "rm -r build", "{}", rig.dump());
    assert_eq!(by_gate(&rig), 1, "{}", rig.dump());
    rig.tasks.ctl(&asked, Ctl::Answer(Answer::No), Actor::Operator).unwrap();
    rig.finished(&id);
}

#[test]
fn a_yes_once_from_the_operator_is_not_a_grant() {
    let doer = Scripted::new("script", vec![vec![Beat::Ask(act("make build")), Beat::Hold, end("built")]]);
    let rig = gated(vec![doer.clone()]);
    let first = rig.spawn("one", "script");
    let ask = waiting_on(&rig, &first);
    rig.tasks.ctl(&ask, Ctl::Answer(Answer::Yes), Actor::Operator).unwrap();
    rig.finished(&first);
    let second = rig.spawn("two", "script");
    waiting_on(&rig, &second);
    assert!(answered(&rig).iter().all(|e| e.data["by"] != "rule:gate"), "an answer is not a grant");
    assert!(rig.of_kind("grant").is_empty());
}

#[test]
fn a_grant_for_one_use_is_spent_by_the_use_and_the_next_ask_goes_to_the_operator() {
    let doer = Scripted::new("script", vec![vec![Beat::Ask(act("make build")), Beat::Hold, end("built")]]);
    let (rig, gate) = gated_with(vec![doer.clone()]);
    let once = gate.grants().event(&Actor::Operator, Scope::Once, &Action::shell(Actor::Operator, "make build")).unwrap();
    rig.log.append(once).unwrap();
    let first = rig.spawn("one", "script");
    assert_eq!(rig.finished(&first).state, State::Done { ok: true });
    assert_eq!(rig.of_kind("grant.used").len(), 1, "{}", rig.dump());
    let second = rig.spawn("two", "script");
    waiting_on(&rig, &second);
    assert_eq!(answered(&rig).len(), 1, "the second was not answered by the spent grant");
}

// ---- time, headless, the breaker, races ----

#[test]
fn headless_a_question_that_would_wait_for_the_operator_takes_the_fallback() {
    let a = asks(vec![point("pick", ShapeKind::Choice, Impact::Low, &["human"], Fallback::Pick("b".into()))], &[]).headless(true);
    let doer = Scripted::new("script", vec![vec![Beat::Ask(asked("q1", "which?", "pick", &[("a", true), ("b", false)])), Beat::Hold, end("ok")]]);
    let rig = Rig::tuned(vec![doer.clone()], with(a));
    let id = rig.spawn("go", "script");
    assert_eq!(rig.finished(&id).state, State::Done { ok: true });
    assert_eq!(doer.ctls(), vec![RunCtl::Answer { local: "q1".into(), answer: pick("b") }]);
    let said = answered(&rig);
    assert_eq!((said[0].data["via"].as_str(), said[0].data["by"].as_str()), (Some("fallback"), Some("fallback:pick")));
}

#[test]
fn a_question_nobody_can_answer_and_nothing_may_wait_for_stops_its_asker_loudly() {
    // headless, and the point's fallback cannot make an answer to this shape: the asker does not hang on a yes nobody will give
    let a = asks(vec![point("odd", ShapeKind::YesNo, Impact::Low, &["human"], Fallback::Text("fine".into()))], &[]).headless(true);
    let spec = AskSpec { point: Some("odd".into()), question: "ok?".into(), shape: j4n_core::ask::AskShape::YesNo, ..choice("q1", "ok?", &[("a", true)]) };
    let doer = Scripted::new("script", vec![vec![Beat::Ask(spec), Beat::Hold, end("ok")]]);
    let rig = Rig::tuned(vec![doer.clone()], with(a));
    let id = rig.spawn("go", "script");
    let row = rig.finished(&id);
    match row.state {
        State::Stopped { why: j4n_core::task::Why::Failed { message } } => assert!(message.contains("nobody could answer"), "{message}"),
        other => panic!("{other:?}\n{}", rig.dump()),
    }
    assert!(rig.of_kind("task.waiting").iter().all(|e| rig.tasks.book().get(e.data["ask"].as_str().unwrap()).unwrap().state.is_terminal()));
}

#[test]
fn after_a_while_without_the_operator_the_points_fallback_stands_in() {
    let a = asks(vec![point("pick", ShapeKind::Choice, Impact::Low, &["human?after=60s"], Fallback::Pick("b".into()))], &[]);
    let doer = Scripted::new("script", vec![vec![Beat::Ask(asked("q1", "which?", "pick", &[("a", true), ("b", false)])), Beat::Hold, end("ok")]]);
    let rig = Rig::tuned(vec![doer.clone()], with(a));
    let id = rig.spawn("go", "script");
    let ask = waiting_on(&rig, &id);
    rig.until_event("the walk", |e| e.kind == "ask.note" || e.kind == "ask.step");
    std::thread::sleep(std::time::Duration::from_millis(100));
    rig.clock.advance(59_999);
    rig.timers.poke();
    assert_eq!(rig.tasks.book().get(&ask).unwrap().state, State::Running, "not yet");
    rig.clock.advance(1);
    rig.timers.poke();
    assert_eq!(rig.finished(&id).state, State::Done { ok: true });
    assert_eq!(doer.ctls(), vec![RunCtl::Answer { local: "q1".into(), answer: pick("b") }]);
    assert_eq!(answered(&rig)[0].data["via"], "fallback");
}

#[test]
fn an_answerer_that_fails_three_times_is_left_alone_and_the_loop_says_so_once() {
    let bad = Fake::new("decider:d", Kind::Decider, |_| Offer::Failed { why: "timed out".into() });
    let a = asks(vec![point("pick", ShapeKind::Choice, Impact::Low, &["decider:d", "human"], Fallback::Rank)], &[&bad]);
    let beats: Vec<Beat> = (1..=4).flat_map(|i| [Beat::Ask(asked(&format!("q{i}"), &format!("which {i}?"), "pick", &[("a", true), ("b", false)])), Beat::Hold]).chain([end("ok")]).collect();
    let doer = Scripted::new("script", vec![beats]);
    let rig = Rig::tuned(vec![doer.clone()], with(a));
    let id = rig.spawn("go", "script");
    let mut seen = String::new();
    for i in 1..=4 {
        let ask = loop {
            let ask = waiting_on(&rig, &id);
            if ask != seen {
                break ask;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        // let the walk finish before the operator answers
        rig.until_event("the walk of this ask", |e| (e.kind == "ask.step" || e.kind == "ask.note") && e.data["ask"] == ask.as_str());
        std::thread::sleep(std::time::Duration::from_millis(30));
        rig.tasks.ctl(&ask, Ctl::Answer(pick("a")), Actor::Operator).unwrap();
        seen = ask;
        let _ = i;
    }
    rig.finished(&id);
    assert_eq!(bad.calls(), 3, "the fourth question never reached it");
    assert_eq!(rig.of_kind("answerer.off").len(), 1);
    assert_eq!(rig.of_kind("answerer.off")[0].data["answerer"], "decider:d");
}

#[test]
fn an_answerer_that_panics_has_failed_and_the_operator_is_asked() {
    let boom = Fake::new("rule:boom", Kind::Rule, |_| panic!("an answerer that is broken"));
    let a = asks(vec![point("pick", ShapeKind::Choice, Impact::Low, &["rule:boom", "human"], Fallback::Rank)], &[&boom]);
    let doer = Scripted::new("script", vec![vec![Beat::Ask(asked("q1", "which?", "pick", &[("a", true)])), Beat::Hold, end("ok")]]);
    let rig = Rig::tuned(vec![doer.clone()], with(a));
    let id = rig.spawn("go", "script");
    let ask = waiting_on(&rig, &id);
    let step = rig.until_event("the failed step", |e| e.kind == "ask.step");
    assert_eq!(step.data["outcome"], "failed");
    rig.tasks.ctl(&ask, Ctl::Answer(pick("a")), Actor::Operator).unwrap();
    assert_eq!(rig.finished(&id).state, State::Done { ok: true });
}

/// An answerer that waits for a go-ahead before it answers.
fn slow(go: Receiver<()>) -> Arc<Fake> {
    let go = Mutex::new(go);
    Fake::new("rule:slow", Kind::Rule, move |_| {
        let _ = go.lock().unwrap().recv();
        Offer::Answer { answer: pick("b"), conf: 1.0, why: "late".into() }
    })
}

#[test]
fn an_answer_that_comes_after_the_operators_changes_nothing_but_is_still_written_down() {
    let (release, go) = mpsc::channel();
    let s = slow(go);
    let a = asks(vec![point("pick", ShapeKind::Choice, Impact::Low, &["rule:slow", "human"], Fallback::Rank)], &[&s]);
    let doer = Scripted::new("script", vec![vec![Beat::Ask(asked("q1", "which?", "pick", &[("a", true), ("b", false)])), Beat::Hold, end("ok")]]);
    let rig = Rig::tuned(vec![doer.clone()], with(a));
    let id = rig.spawn("go", "script");
    let ask = waiting_on(&rig, &id);
    rig.tasks.ctl(&ask, Ctl::Answer(pick("a")), Actor::Operator).unwrap();
    assert_eq!(rig.finished(&id).state, State::Done { ok: true });
    release.send(()).unwrap();
    rig.until_event("the late step", |e| e.kind == "ask.step" && e.data["outcome"] == "answer");
    assert_eq!(doer.ctls(), vec![RunCtl::Answer { local: "q1".into(), answer: pick("a") }], "only the operator's reached the doer");
    assert_eq!(rig.of_kind("task.settled").iter().filter(|e| e.task.as_deref() == Some(ask.as_str())).count(), 1);
    assert_eq!(answered(&rig).len(), 1);
}

#[test]
fn a_question_whose_point_does_not_exist_is_the_operators() {
    let a = asks(vec![], &[]);
    let doer = Scripted::new("script", vec![vec![Beat::Ask(asked("q1", "which?", "nowhere", &[("a", true)])), Beat::Hold, end("ok")]]);
    let rig = Rig::tuned(vec![doer.clone()], with(a));
    let id = rig.spawn("go", "script");
    let ask = waiting_on(&rig, &id);
    let note = rig.until_event("the note", |e| e.kind == "ask.note");
    assert!(note.data["why"].as_str().unwrap().contains("nowhere"));
    rig.tasks.ctl(&ask, Ctl::Answer(pick("a")), Actor::Operator).unwrap();
    assert_eq!(rig.finished(&id).state, State::Done { ok: true });
}

// ---- the binder, over the open questions of the Book ----

#[test]
fn a_typed_line_answers_the_open_question_through_the_binder() {
    let doer = Scripted::new("script", vec![vec![Beat::Ask(asked("q1", "which database?", "question", &[("postgres", true), ("sqlite", false)])), Beat::Hold, end("ok")]]);
    let rig = Rig::new(vec![doer.clone()]);
    let id = rig.spawn("set up", "script");
    let ask = waiting_on(&rig, &id);
    let open = rig.tasks.book().open_asks();
    assert_eq!((open.len(), open[0].ask.as_str(), open[0].asker.as_str()), (1, ask.as_str(), id.as_str()));
    assert!(matches!(bind("let's talk about something else entirely, please", &open, None), Bind::NotAnswer));
    let Bind::Answer { ask: target, answer } = bind("2", &open, None) else { panic!("a number picks") };
    assert_eq!((target.as_str(), &answer), (ask.as_str(), &pick("sqlite")));
    rig.tasks.ctl(&target, Ctl::Answer(answer), Actor::Operator).unwrap();
    assert_eq!(rig.finished(&id).state, State::Done { ok: true });
    assert!(rig.tasks.book().open_asks().is_empty(), "an answered question is not open");
}

// a task's decision rows hold only its own.
#[test]
fn a_tasks_decision_rows_hold_only_its_own() {
    let rule = Fake::says("rule:r", Kind::Rule, pick("b"));
    let a = asks(vec![point("pick", ShapeKind::Choice, Impact::Low, &["rule:r", "human"], Fallback::Rank)], &[&rule]);
    let one = Scripted::new("one", vec![vec![Beat::Ask(asked("q1", "first?", "pick", &[("a", false), ("b", true)])), Beat::Hold, Beat::Ask(asked("q2", "second?", "pick", &[("a", false), ("b", true)])), Beat::Hold, end("one")]]);
    let two = Scripted::new("two", vec![vec![Beat::Ask(asked("q1", "third?", "pick", &[("a", false), ("b", true)])), Beat::Hold, end("two")]]);
    let rig = Rig::tuned(vec![one, two], with(a));
    let decl = format!("api = 1\n{}", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/base/folds/decisions.toml")).trim_start_matches("api = 1\n"));
    let (tables, problems) = load(vec![layer(LayerKind::Pack, "base", &[("folds.toml", &decl)])]);
    assert!(problems.is_empty(), "{problems:?}");
    let (folds, problems) = declared(&tables, LogScope::Project);
    assert!(problems.is_empty() && folds.len() == 1, "{problems:?}");
    for f in folds {
        assert!(rig.folds.add(f, 0).is_empty());
    }
    rig.folds.catch_up(&rig.log);
    let a = rig.spawn("a", "one");
    let b = rig.spawn("b", "two");
    rig.finished(&a);
    rig.finished(&b);
    rig.until_event("the last decision", |e| e.kind == "ask.answered" && e.task.as_deref() == Some(b.as_str()));
    let read = |t: &str| -> Vec<String> {
        match rig.folds.read("decisions", &[j4n_core::value::Key::Name(t.to_string())], 0) {
            j4n_core::value::Value::List(items) => items.into_iter().filter_map(|v| if let j4n_core::value::Value::Str(s) = v { Some(s) } else { None }).collect(),
            other => panic!("{other:?}"),
        }
    };
    let own = |t: &str| -> Vec<String> {
        let mut v: Vec<String> = answered(&rig).iter().filter(|e| e.task.as_deref() == Some(t)).map(|e| e.data["ask"].as_str().unwrap().to_string()).collect();
        v.sort();
        v
    };
    let (mut ra, mut rb) = (read(&a), read(&b));
    ra.sort();
    rb.sort();
    assert_eq!((ra.len(), rb.len()), (2, 1), "{}", rig.dump());
    assert_eq!((ra, rb), (own(&a), own(&b)), "each row is the asks answered for that task and no other");
    let events = answered(&rig);
    assert_eq!(events.iter().filter(|e| e.task.as_deref() == Some(a.as_str())).count(), 2);
    assert_eq!(events.iter().filter(|e| e.task.as_deref() == Some(b.as_str())).count(), 1);
}

#[test]
fn a_grant_on_a_question_that_is_over_is_refused_and_leaves_nothing_behind() {
    let doer = Scripted::new("script", vec![vec![Beat::Ask(act("make build")), Beat::Hold, end("built")]]);
    let rig = gated(vec![doer.clone()]);
    let id = rig.spawn("build", "script");
    let ask = waiting_on(&rig, &id);
    rig.tasks.ctl(&ask, Ctl::Answer(Answer::Yes), Actor::Operator).unwrap();
    rig.finished(&id);
    for scope in [Scope::Session, Scope::Here] {
        assert!(rig.tasks.ctl(&ask, Ctl::Grant(scope), Actor::Operator).is_err(), "{scope:?}");
    }
    assert!(rig.of_kind("grant").is_empty());
    assert!(!here_dir(rig.home.path(), rig.project.path()).exists());
}

#[test]
fn a_walk_that_ends_after_its_question_was_answered_does_not_touch_the_next_one() {
    let (release, go) = mpsc::channel();
    let s = slow(go);
    let a = asks(vec![point("pick", ShapeKind::Choice, Impact::Low, &["rule:slow", "human"], Fallback::Rank)], &[&s]);
    let q = |n: &str, t: &str| Beat::Ask(asked(n, t, "pick", &[("a", true), ("b", false)]));
    let doer = Scripted::new("script", vec![vec![q("q1", "first?"), Beat::Hold, q("q2", "second?"), Beat::Hold, end("ok")]]);
    let rig = Rig::tuned(vec![doer.clone()], with(a));
    let id = rig.spawn("go", "script");
    let first = waiting_on(&rig, &id);
    rig.tasks.ctl(&first, Ctl::Answer(pick("a")), Actor::Operator).unwrap();
    let second = loop {
        let ask = waiting_on(&rig, &id);
        if ask != first {
            break ask;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    release.send(()).unwrap(); // the slow answerer now answers the first question, which is over
    release.send(()).unwrap(); // and the second
    rig.until_event("the late step", |e| e.kind == "ask.step" && e.data["ask"] == first.as_str());
    std::thread::sleep(std::time::Duration::from_millis(100));
    // the second question was walked on its own and answered by the rule; the asker was never failed by the first one's walk
    assert_eq!(rig.finished(&id).state, State::Done { ok: true }, "{}", rig.dump());
    assert_eq!(rig.tasks.book().get(&second).unwrap().state, State::Done { ok: true });
}
