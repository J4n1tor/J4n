//! Flows: the lint that refuses a flow before it runs, and the runner that makes every step a task of its own — with
//! scripted doers, through the real loop.

use crate::scripted::{Beat, Rig, Scripted, end};
use crate::world::{layer, load, refused};
use j4n_core::ask::Answer;
use j4n_core::change::Actor;
use j4n_core::flow::Flows;
use j4n_core::flow::run::{Elect, Elected, Link, TaskHost, doers};
use j4n_core::table::LayerKind;
use j4n_core::task::{Ctl, Mode, State};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

fn flows_of(text: &str) -> (Flows, Vec<String>) {
    let (tables, problems) = load(vec![layer(LayerKind::Pack, "t", &[("flows.toml", &format!("api = 1\n{text}"))])]);
    let mut said = refused(&problems);
    let (flows, more) = Flows::load(&tables);
    said.extend(more.iter().map(ToString::to_string));
    (flows, said)
}

fn clean(text: &str) -> Flows {
    let (flows, said) = flows_of(text);
    assert!(said.is_empty(), "{said:?}");
    flows
}

/// What lint says of one flow, as lines.
fn lint(text: &str) -> Vec<String> {
    flows_of(text).1
}

fn says(lines: &[String], needle: &str) -> bool {
    lines.iter().any(|l| l.contains(needle))
}

struct Roles(BTreeMap<String, String>);

impl Elect for Roles {
    fn elect(&self, role: &str, _: &str, _: Option<&str>, exclude: &[String], _: Mode) -> Result<Elected, String> {
        let doer = self.0.get(role).filter(|d| !exclude.contains(d)).ok_or_else(|| format!("nobody does `{role}`"))?;
        Ok(Elected { doer: doer.clone(), model: None, endpoint: None, why: format!("{doer} is the {role}") })
    }
}

fn rig(flows: &Flows, doers_: Vec<Arc<Scripted>>, roles: &[(&str, &str)]) -> Rig {
    let link = Link::default();
    let fd = doers(flows, &link);
    let rig = Rig::tuned(doers_, |c| c.doers.extend(fd));
    let who = Roles(roles.iter().map(|(r, d)| (r.to_string(), d.to_string())).collect());
    link.set(Arc::new(TaskHost { tasks: rig.tasks.downgrade(), who: Arc::new(who), log: rig.log.clone(), lanes: Default::default(), root: Default::default(), settings: Default::default() }));
    rig
}

/// Answer the flow's questions as they come, until it is over: `says` is the answer to each, in turn.
fn answering(rig: &Rig, flow: &str, says: &[Answer]) -> j4n_core::task::Row {
    let mut answers = says.iter();
    loop {
        let row = rig.tasks.book().wait(flow, |r| r.state.is_terminal() || matches!(r.state, State::Waiting { .. }), Duration::from_secs(20)).unwrap_or_else(|| panic!("{flow} never ended\n{}", rig.dump()));
        match row.state {
            State::Waiting { ask } => {
                let a = answers.next().unwrap_or_else(|| panic!("a question nobody has an answer for: {ask}\n{}", rig.dump()));
                rig.tasks.ctl(&ask, Ctl::Answer(a.clone()), Actor::Operator).expect("answers");
                // the ask is over once the flow's state moves on
                let _ = rig.tasks.book().wait(flow, |r| !matches!(&r.state, State::Waiting { ask: a } if *a == ask), Duration::from_secs(20));
            }
            _ => return row,
        }
    }
}

/// Why a flow that did not go well stopped, in its words.
fn why(row: &j4n_core::task::Row) -> String {
    match &row.state {
        State::Stopped { why: j4n_core::task::Why::Failed { message } } => message.clone(),
        other => panic!("it did not stop with a failure: {other:?}"),
    }
}

fn goal(rig: &Rig, doer: &str) -> Vec<String> {
    rig.tasks.book().rows().into_iter().filter(|r| r.spec.doer == doer).map(|r| r.spec.goal).collect()
}

const PWV: &str = r#"
[[flow]]
id = "pwv"
doc = "plan, ask, work, validate; a concern sends the work back"
in = { goal = "text" }
steps = [
  { id = "plan", do = "who:plan", mode = "read", goal = "Plan this: {in.goal}", out = { plan = "text" } },
  { id = "go", kind = "ask", shape = "yesno", question = "Build this plan?", show = "{steps.plan.out.plan}", no = "done" },
  { id = "work", do = "who:work", goal = "Do this: {steps.plan.out.plan}", redo = "Fix: {steps.validate.out.verdict.concerns}", after = ["go"], out = { changes = "text" } },
  { id = "validate", do = "who:validate", mode = "read", goal = "Check {steps.work.answer}", exclude = ["{steps.work.doer}"], out = { verdict = "verdict" } },
  { id = "again", kind = "branch", on = "steps.validate.out.verdict.pass", yes = "done", no = "work", max_visits = 3 },
  { id = "done", kind = "end", answer = "{steps.work.answer}" },
]
"#;

#[test]
fn a_flow_that_is_sound_is_read_and_ordered_by_its_ports() {
    let flows = clean(PWV);
    let f = flows.get("pwv").expect("loaded");
    let order: Vec<&str> = f.order.iter().map(|i| f.row.steps[*i].id.as_str()).collect();
    assert_eq!(order, ["plan", "go", "work", "validate", "again", "done"]);
    // a reader of a kind waits for the writers of it, wherever it is written
    let flows = clean(
        r#"[[flow]]
id = "wired"
steps = [
  { id = "review", do = "reviewer", goal = "look at the diff", reads = ["diff"] },
  { id = "build", do = "builder", goal = "make it", out = { patch = "diff" } },
]"#,
    );
    let f = flows.get("wired").unwrap();
    assert_eq!(f.order.iter().map(|i| f.row.steps[*i].id.as_str()).collect::<Vec<_>>(), ["build", "review"]);
}

#[test]
fn a_flow_with_a_fault_is_left_out_and_says_what_it_is() {
    let step = |s: &str| format!("[[flow]]\nid = \"f\"\nsteps = [{s}]");
    for (steps, needle) in [
        (r#"{ id = "a", do = "x", goal = "g" }, { id = "a", do = "y", goal = "g" }"#, "used twice"),
        (r#"{ id = "a", do = "x", goal = "{steps.b.answer}" }"#, "`steps.b` is not a step"),
        (r#"{ id = "a", do = "x", goal = "g", out = { p = "text" } }, { id = "b", do = "x", goal = "{steps.a.out.q}" }"#, "declares no such port"),
        (r#"{ id = "a", do = "x", goal = "{in.nope}" }"#, "`in.nope` is not an input"),
        (r#"{ id = "a", do = "x", goal = "g", after = ["b"] }, { id = "b", do = "x", goal = "g", after = ["a"] }"#, "wait for each other"),
        (r#"{ id = "a", do = "x", goal = "g" }, { id = "b", kind = "branch", on = "true", yes = "a", no = "a" }"#, "needs `max_visits`"),
        (r#"{ id = "a", kind = "branch", on = "true", yes = "z", no = "a", max_visits = 2 }"#, "`z` is not a step"),
        (r#"{ id = "a", do = "flow:ghost", goal = "g" }"#, "no flow called `ghost`"),
        (r#"{ id = "a", do = "web:search", goal = "g" }"#, "is not a way to say who does it"),
        (r#"{ id = "a", goal = "g" }"#, "says what does it"),
        (r#"{ id = "a", kind = "ask", shape = "yesno" }"#, "`question`"),
        (r#"{ id = "a", do = "x", goal = "g", out = { p = "blob" } }"#, "a port has a word for a name and a kind"),
        (r#"{ id = "a", do = "x", goal = "g", reads = ["diff"] }"#, "no step writes it"),
        (r#"{ id = "a", do = "x", goal = "{unclosed" }"#, "never closed"),
        (r#"{ id = "a", kind = "ask", question = "q" }"#, "an `ask` step has a `shape`"),
        (r#"{ id = "a", kind = "ask", question = "q", shape = "choice" }"#, "a `choice` offers `options`"),
        (r#"{ id = "a", kind = "branch", on = "true", yes = "a", max_visits = 2 }"#, "a `branch` has `on`, `yes` and `no`"),
        (r#"{ id = "a", kind = "map", over = "in.xs", goal = "g" }"#, "a `map` has `over`, `each`"),
        (r#"{ id = "a", do = "who:two words", goal = "g" }"#, "a role is one word"),
        (r#"{ id = "a", do = "x", goal = "g", after = ["a"] }"#, "it comes after itself"),
        (r#"{ id = "a", do = "x", goal = "g", out = { p = "text" } }, { id = "b", do = "y", goal = "g", reads = ["diff"] }"#, "no step writes it"),
        (r#"{ id = "a", kind = "end", do = "x" }"#, "`do` belongs to a `do` step"),
        (r#"{ id = "a", do = "who:read", goal = "g", model = "{in.nope}" }"#, "`in.nope` is not an input"),
        (r#"{ id = "a", do = "who:read", goal = "g", model = "{in.nope" }"#, "steps[0].model: a `{` is never closed"),
    ] {
        let said = lint(&step(steps));
        assert!(says(&said, needle), "{steps}\nwanted `{needle}`; got {said:?}");
    }
    let said = lint("[[flow]]\nid = \"a\"\nsteps = [{ id = \"s\", do = \"flow:b\", goal = \"g\" }]\n[[flow]]\nid = \"b\"\nsteps = [{ id = \"s\", do = \"flow:a\", goal = \"g\" }]");
    assert!(says(&said, "runs itself"), "{said:?}");
}

#[test]
fn a_step_reads_that_start_with_a_slash_are_paths_for_the_box_and_the_rest_are_kinds() {
    // a path needs no step that writes it, and a kind still does
    let step = |s: &str| format!("[[flow]]\nid = \"f\"\nsteps = [{s}]");
    assert!(lint(&step(r#"{ id = "a", do = "x", goal = "g", reads = ["/tmp/src"] }"#)).is_empty());
    assert!(says(&lint(&step(r#"{ id = "a", do = "x", goal = "g", reads = ["/tmp/src", "diff"] }"#)), "no step writes it"));
    let flows = clean(
        r#"[[flow]]
id = "paths"
steps = [
  { id = "build", do = "builder", goal = "make it", out = { patch = "diff" } },
  { id = "review", do = "reviewer", goal = "look at /tmp/src", reads = ["/tmp/src", "diff"] },
]"#,
    );
    let builder = Scripted::new("builder", vec![vec![end("made")]]);
    let reviewer = Scripted::new("reviewer", vec![vec![end("looked")]]);
    let rig = rig(&flows, vec![builder, reviewer], &[]);
    let id = rig.spawn("go", "flow:paths");
    let row = answering(&rig, &id, &[]);
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}\n{}", row.state, rig.dump());
    let reads = |doer: &str| rig.tasks.book().children(&id).into_iter().find(|k| k.spec.doer == doer).map(|k| k.spec.reads).expect("a child");
    assert_eq!(reads("reviewer"), ["/tmp/src"], "the path goes to the place, the kind does not");
    assert!(reads("builder").is_empty());
}

#[test]
fn plan_then_work_then_validate_with_each_step_a_task_of_its_own() {
    let flows = clean(PWV);
    let planner = Scripted::new("planner", vec![vec![end("1. add the file")]]);
    let worker = Scripted::new("worker", vec![vec![end("added it")]]);
    let checker = Scripted::new("checker", vec![vec![end(r#"all good {"pass": true, "concerns": []}"#)]]);
    let rig = rig(&flows, vec![planner.clone(), worker.clone(), checker.clone()], &[("plan", "planner"), ("work", "worker"), ("validate", "checker")]);
    let id = rig.spawn("make the thing", "flow:pwv");
    let row = answering(&rig, &id, &[Answer::Yes]);
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}\n{}", row.state, rig.dump());
    assert_eq!(row.answer.as_deref(), Some("added it"), "the flow's answer is the one its end gives");
    assert_eq!(goal(&rig, "planner"), ["Plan this: make the thing"]);
    assert_eq!(goal(&rig, "worker"), ["Do this: 1. add the file"]);
    assert_eq!(goal(&rig, "checker"), ["Check added it"]);
    // the steps are children of the flow, the roles are on them, and the plan was told to only read
    let kids = rig.tasks.book().children(&id);
    let own: Vec<_> = kids.iter().filter(|k| k.spec.doer != "human").collect();
    assert_eq!(own.iter().map(|k| (k.spec.doer.as_str(), k.spec.role.as_deref())).collect::<Vec<_>>(), [("planner", Some("plan")), ("worker", Some("work")), ("checker", Some("validate"))]);
    assert_eq!(own[0].spec.tune.mode, Mode::Read);
    assert_eq!(own[1].spec.tune.mode, Mode::Write);
    let steps: Vec<String> = rig.of_kind("flow.step.started").iter().map(|e| e.data["step"].as_str().unwrap().to_string()).collect();
    assert_eq!(steps, ["plan", "go", "work", "validate", "again", "done"], "{}", rig.dump());
}

#[test]
fn a_concern_sends_the_work_back_with_the_concern_and_the_second_pass_ends_it() {
    let flows = clean(PWV);
    let planner = Scripted::new("planner", vec![vec![end("plan")]]);
    let worker = Scripted::new("worker", vec![vec![end("first try")], vec![end("second try")]]);
    let checker = Scripted::new("checker", vec![vec![end(r#"{"pass": false, "concerns": ["no test"]}"#)], vec![end(r#"{"pass": true}"#)]]);
    let rig = rig(&flows, vec![planner, worker, checker], &[("plan", "planner"), ("work", "worker"), ("validate", "checker")]);
    let id = rig.spawn("go", "flow:pwv");
    let row = answering(&rig, &id, &[Answer::Yes]);
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}\n{}", row.state, rig.dump());
    assert_eq!(row.answer.as_deref(), Some("second try"));
    assert_eq!(goal(&rig, "worker"), ["Do this: plan", "Fix: no test"], "the redo reads what the validator said");
    assert_eq!(goal(&rig, "checker").len(), 2);
}

#[test]
fn a_loop_that_never_mends_stops_the_flow_after_its_visits() {
    let flows = clean(&PWV.replace("max_visits = 3", "max_visits = 2"));
    let always_no = Scripted::new("checker", vec![vec![end(r#"{"pass": false, "concerns": ["still wrong"]}"#)]]);
    let rig = rig(&flows, vec![Scripted::new("planner", vec![vec![end("p")]]), Scripted::new("worker", vec![vec![end("w")]]), always_no], &[("plan", "planner"), ("work", "worker"), ("validate", "checker")]);
    let id = rig.spawn("go", "flow:pwv");
    let row = answering(&rig, &id, &[Answer::Yes]);
    assert!(why(&row).contains("`again` went back 1 times"), "{:?}", row.state);
    assert_eq!(goal(&rig, "worker").len(), 2, "two passes at the work, no more");
}

#[test]
fn a_no_to_a_question_with_a_way_out_takes_it_and_does_no_work() {
    let flows = clean(PWV);
    let worker = Scripted::new("worker", vec![vec![end("w")]]);
    let rig = rig(&flows, vec![Scripted::new("planner", vec![vec![end("p")]]), worker.clone(), Scripted::new("checker", vec![vec![end("x")]])], &[("plan", "planner"), ("work", "worker"), ("validate", "checker")]);
    let id = rig.spawn("go", "flow:pwv");
    let row = answering(&rig, &id, &[Answer::No]);
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}", row.state);
    assert!(worker.starts().is_empty(), "nothing was built");
}

#[test]
fn the_validator_is_never_the_worker_when_the_election_is_told_so() {
    let flows = clean(PWV);
    // one doer is offered for every role; the validate step excludes whoever worked
    let same = Scripted::new("solo", vec![vec![end(r#"{"pass": true}"#)]]);
    let rig = rig(&flows, vec![same], &[("plan", "solo"), ("work", "solo"), ("validate", "solo")]);
    let id = rig.spawn("go", "flow:pwv");
    let row = answering(&rig, &id, &[Answer::Yes]);
    assert!(why(&row).contains("nobody can be elected for `validate`"), "{:?}\n{}", row.state, rig.dump());
}

#[test]
fn a_step_that_fails_stops_the_flow_with_its_reason() {
    let flows = clean("[[flow]]\nid = \"f\"\nsteps = [{ id = \"a\", do = \"bad\", goal = \"g\" }, { id = \"b\", do = \"good\", goal = \"never\" }]");
    let good = Scripted::new("good", vec![vec![end("ok")]]);
    let bad = Scripted::new("bad", vec![vec![Beat::Fail("it broke".into())]]);
    let rig = rig(&flows, vec![bad, good.clone()], &[]);
    let id = rig.spawn("go", "flow:f");
    let row = answering(&rig, &id, &[]);
    assert!(why(&row).contains("flow step `a` (bad, t2) did not finish: it broke"), "{:?}", row.state);
    assert!(good.starts().is_empty());
}

#[test]
fn a_stop_of_the_flow_stops_the_step_that_works() {
    let flows = clean("[[flow]]\nid = \"f\"\nsteps = [{ id = \"a\", do = \"slow\", goal = \"g\" }]");
    let slow = Scripted::new("slow", vec![vec![Beat::Forever]]);
    let rig = rig(&flows, vec![slow.clone()], &[]);
    let id = rig.spawn("go", "flow:f");
    rig.until_event("the step started", |e| e.kind == "flow.child");
    rig.tasks.ctl(&id, Ctl::Abort, Actor::Operator).expect("aborts");
    let flow = rig.finished(&id);
    assert!(matches!(flow.state, State::Stopped { .. }), "{:?}", flow.state);
    let child = rig.tasks.book().children(&id).into_iter().find(|k| k.spec.doer == "slow").expect("the step's task");
    let child = rig.finished(&child.id);
    assert!(matches!(child.state, State::Stopped { .. }), "{:?}\n{}", child.state, rig.dump());
}

#[test]
fn a_steer_of_the_flow_reaches_the_step_that_works() {
    let flows = clean("[[flow]]\nid = \"f\"\nsteps = [{ id = \"a\", do = \"slow\", goal = \"g\" }]");
    let slow = Scripted::new("slow", vec![vec![Beat::Pause(300), end("done")]]);
    let rig = rig(&flows, vec![slow.clone()], &[]);
    let id = rig.spawn("go", "flow:f");
    rig.until_event("the step started", |e| e.kind == "flow.child");
    rig.tasks.ctl(&id, Ctl::Steer("also the docs".into()), Actor::Operator).expect("steers");
    rig.finished(&id);
    assert!(slow.ctls().iter().any(|c| matches!(c, j4n_core::doer::RunCtl::Steer(t) if t == "also the docs")), "{:?}", slow.ctls());
}

#[test]
fn a_flow_runs_a_flow_and_the_chain_has_a_depth_cap() {
    let text = r#"
[[flow]]
id = "outer"
in = { goal = "text" }
depth = 2
steps = [{ id = "go", do = "flow:middle", goal = "{in.goal}" }]
[[flow]]
id = "middle"
in = { goal = "text" }
steps = [{ id = "go", do = "flow:inner", goal = "{in.goal}" }]
[[flow]]
id = "inner"
in = { goal = "text" }
steps = [{ id = "go", do = "leaf", goal = "{in.goal}!" }]
"#;
    let flows = clean(text);
    let leaf = Scripted::new("leaf", vec![vec![end("leaf says hi")]]);
    let rig = rig(&flows, vec![leaf.clone()], &[]);
    // outer (0) runs middle (1), which would run inner (2): the cap of the chain is outer's 2
    let id = rig.spawn("hello", "flow:outer");
    let row = answering(&rig, &id, &[]);
    assert!(why(&row).contains("flow step `go` (flow:middle, t2) did not finish") && why(&row).contains("2 deep"), "{:?}\n{}", row.state, rig.dump());
    assert!(leaf.starts().is_empty());
    // run from the middle, the same chain is shallow enough
    let id = rig.spawn("hello", "flow:middle");
    let row = answering(&rig, &id, &[]);
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}\n{}", row.state, rig.dump());
    assert_eq!(row.answer.as_deref(), Some("leaf says hi"), "the answer travels up through the flows");
    assert_eq!(goal(&rig, "leaf"), ["hello!"]);
}

#[test]
fn a_map_runs_a_doer_over_a_list_and_gathers_the_answers() {
    let flows = clean(
        r#"[[flow]]
id = "each"
in = { goal = "text", files = "json" }
steps = [
  { id = "per", kind = "map", over = "in.files", each = "worker", goal = "look at {item}", out = { results = "json" } },
  { id = "done", kind = "end", answer = "{steps.per.answer}" },
]"#,
    );
    let worker = Scripted::new("worker", vec![vec![end("seen")]]);
    let rig = rig(&flows, vec![worker.clone()], &[]);
    let mut spec = j4n_core::task::TaskSpec::new("go", "flow:each");
    spec.input = serde_json::json!({ "in": { "files": ["a.rs", "b.rs", "c.rs"] } });
    let id = rig.tasks.spawn(spec, Actor::Operator).expect("spawns");
    let row = answering(&rig, &id, &[]);
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}\n{}", row.state, rig.dump());
    let mut goals = goal(&rig, "worker");
    goals.sort();
    assert_eq!(goals, ["look at a.rs", "look at b.rs", "look at c.rs"]);
    // a map gives a list of reports, not the answers joined; an answer that is no report says so
    let reports: Vec<serde_json::Value> = serde_json::from_str(row.answer.as_deref().unwrap()).expect("a list");
    assert_eq!(reports.len(), 3);
    assert!(reports.iter().all(|r| r["answer"] == "seen" && r["sure"] == "low" && r["found"] == serde_json::json!([])), "{reports:?}");
    assert_eq!(rig.of_kind("report.made").len(), 3);
    assert!(rig.of_kind("report.made").iter().all(|e| e.data["deliver"] == false));
}

#[test]
fn a_step_model_may_come_from_the_flows_inputs_so_one_step_serves_every_model_of_a_role() {
    let flows = clean(
        r#"[[flow]]
id = "readers"
in = { goal = "text", reader = "text", skeptic = "text", spare = "text", files = "json" }
steps = [
  { id = "read", do = "who:read", goal = "read: {in.goal}", model = "{in.reader}" },
  { id = "doubt", do = "who:read", goal = "doubt: {in.goal}", model = "{in.skeptic}", after = ["read"] },
  { id = "plain", do = "who:read", goal = "plain: {in.goal}", model = "{in.spare}", after = ["doubt"] },
  { id = "per", kind = "map", over = "in.files", each = "alpha", goal = "each: {item}", model = "{in.reader}", after = ["plain"] },
  { id = "done", kind = "end", answer = "{steps.per.answer}" },
]"#,
    );
    let alpha = Scripted::new("alpha", vec![vec![end("ok")]]);
    let rig = rig(&flows, vec![alpha.clone()], &[("read", "alpha")]);
    let mut spec = j4n_core::task::TaskSpec::new("the folder", "flow:readers");
    spec.input = serde_json::json!({ "in": { "reader": "base-model", "skeptic": "light-model", "files": ["a.rs"] } });
    let id = rig.tasks.spawn(spec, Actor::Operator).expect("spawns");
    let row = answering(&rig, &id, &[]);
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}\n{}", row.state, rig.dump());
    let mut models: Vec<(String, Option<String>)> = rig.tasks.book().children(&id).into_iter().map(|k| (k.spec.goal, k.spec.tune.model)).collect();
    models.sort();
    let want = |g: &str, m: Option<&str>| (g.to_string(), m.map(String::from));
    // an input that is not given names no model: the election's (none here) stands
    assert_eq!(models, [want("doubt: the folder", Some("light-model")), want("each: a.rs", Some("base-model")), want("plain: the folder", None), want("read: the folder", Some("base-model"))]);
}

#[test]
fn a_step_whose_when_does_not_hold_is_skipped() {
    let flows = clean(
        "[[flow]]\nid = \"f\"\nin = { goal = \"text\" }\nsteps = [{ id = \"a\", do = \"x\", goal = \"g\", when = \"in.goal == 'yes'\" }, { id = \"b\", do = \"y\", goal = \"h\", after = [\"a\"] }]",
    );
    let (x, y) = (Scripted::new("x", vec![vec![end("x")]]), Scripted::new("y", vec![vec![end("y")]]));
    let rig = rig(&flows, vec![x.clone(), y.clone()], &[]);
    let id = rig.spawn("no", "flow:f");
    let row = answering(&rig, &id, &[]);
    assert_eq!(row.answer.as_deref(), Some("y"));
    assert!(x.starts().is_empty() && y.starts().len() == 1);
    assert_eq!(rig.of_kind("flow.step.skipped").len(), 1);
}

/// A flow rig whose election is the real `Who`, bound by `pin` (the launch's `--doer` and `--model`), over the doers given.
fn rig_pinned(flows: &Flows, doers_: Vec<Arc<Scripted>>, pin: j4n_core::front::who::Pin, lists: &[(&str, &[&str])]) -> Rig {
    use j4n_core::front::hosts::NoHosts;
    let link = Link::default();
    let fd = doers(flows, &link);
    let rig = Rig::tuned(doers_.clone(), |c| c.doers.extend(fd));
    let standing = j4n_core::front::who::Standing::new();
    standing.set(rig.tasks.book().clone());
    let all: Vec<Arc<dyn j4n_core::doer::Doer>> = doers_.iter().map(|d| d.clone() as Arc<dyn j4n_core::doer::Doer>).collect();
    let lists = crate::who_cases::Lists(lists.iter().map(|(d, m)| (d.to_string(), m.iter().map(|x| x.to_string()).collect())).collect());
    let who = j4n_core::front::who::Who::new(crate::who_cases::asks_for(&standing), &all, BTreeMap::new(), Arc::new(NoHosts), rig.log.clone()).with_pin(pin, Arc::new(lists));
    link.set(Arc::new(TaskHost { tasks: rig.tasks.downgrade(), who: Arc::new(who), log: rig.log.clone(), lanes: Default::default(), root: Default::default(), settings: Default::default() }));
    rig
}

#[test]
fn a_who_step_runs_the_doer_the_launch_pinned_and_on_the_model_it_pinned_and_a_model_of_the_step_wins() {
    let step = |extra: &str| clean(&format!("[[flow]]\nid = \"f\"\nsteps = [{{ id = \"a\", do = \"who:work\", goal = \"g\"{extra} }}]"));
    let (alpha, beta) = (Scripted::new("alpha", vec![vec![end("a")]]), Scripted::new("beta", vec![vec![end("b")]]));
    let lists: &[(&str, &[&str])] = &[("beta", &["m1", "m2"])];
    let pin = j4n_core::front::who::Pin { doer: Some("beta".into()), model: Some("m2".into()) };
    // the pin: the step is the script doer's, on the pinned model, and the Log's election says why
    let rig = rig_pinned(&step(""), vec![alpha.clone(), beta.clone()], pin.clone(), lists);
    let id = rig.spawn("go", "flow:f");
    answering(&rig, &id, &[]);
    assert!(alpha.starts().is_empty() && beta.starts().len() == 1, "the pinned doer did the step: {}", rig.dump());
    let child = rig.tasks.book().children(&id).into_iter().find(|k| k.spec.doer == "beta").expect("the step's task");
    assert_eq!(child.spec.tune.model.as_deref(), Some("m2"));
    let why = rig.log.read(0).find(|e| e.kind == "flow.elected").map(|e| e.data["why"].as_str().unwrap_or_default().to_string()).expect("the election is in the Log");
    assert!(why.contains("--doer beta is pinned") && why.contains("--model puts it on m2"), "{why}");
    // a model the step names itself is the later word
    let rig = rig_pinned(&step(", model = \"m1\""), vec![alpha, beta], pin, lists);
    let id = rig.spawn("go", "flow:f");
    answering(&rig, &id, &[]);
    let child = rig.tasks.book().children(&id).into_iter().find(|k| k.spec.doer == "beta").expect("the step's task");
    assert_eq!(child.spec.tune.model.as_deref(), Some("m1"), "{}", rig.dump());
}

// ---- the cap binds the whole tree ----

fn capped(rig: &Rig, flow: &str, cap: f64, input: serde_json::Value) -> String {
    let mut spec = j4n_core::task::TaskSpec::new("go", flow);
    spec.limits.usd_soft = Some(cap);
    spec.input = input;
    rig.tasks.spawn(spec, Actor::Operator).expect("spawns")
}

#[test]
fn a_map_whose_items_each_spend_more_than_a_third_of_the_cap_is_stopped_at_one_and_a_half_times_it_with_every_item() {
    let flows = clean(
        r#"[[flow]]
id = "each"
in = { goal = "text", files = "json" }
steps = [
  { id = "per", kind = "map", over = "in.files", each = "worker", goal = "look at {item}", out = { results = "json" } },
  { id = "done", kind = "end", answer = "{steps.per.answer}" },
]"#,
    );
    // each item spends $0.12 and goes on: three of them pass the cap of $0.30, the fourth passes one and a half times it
    let turn = || vec![Beat::Session("s".into()), Beat::Spend(0.12), Beat::Forever];
    let worker = Scripted::new("worker", (0..8).map(|_| turn()).collect());
    let rig = rig(&flows, vec![worker.clone()], &[]);
    let id = capped(&rig, "flow:each", 0.30, serde_json::json!({ "in": { "files": ["a", "b", "c", "d", "e", "f", "g", "h"] } }));
    let root = rig.finished(&id);
    assert!(matches!(root.state, State::Stopped { why: j4n_core::task::Why::HardCap }), "{:?}\n{}", root.state, rig.dump());
    assert!(root.capped);
    // the cap was said once, on the root, by the tree
    let said: Vec<_> = rig.of_kind("task.cap_reached").into_iter().filter(|e| e.task.as_deref() == Some(id.as_str())).collect();
    assert_eq!(said.len(), 1, "{}", rig.dump());
    assert_eq!((said[0].data["tree"].as_bool(), said[0].data["soft"].as_f64()), (Some(true), Some(0.30)));
    // every item that was going is stopped, and the second chunk of the map never began
    let kids = rig.tasks.book().children(&id);
    assert_eq!(worker.starts().len(), 4, "the items that began: {}", rig.dump());
    for k in kids.iter().filter(|k| k.spec.doer == "worker") {
        let k = rig.finished(&k.id);
        assert!(matches!(k.state, State::Stopped { why: j4n_core::task::Why::HardCap }), "stopped by the cap, not as an orphan: {:?}", k.state);
    }
    let spent: f64 = rig.tasks.book().children(&id).iter().map(|k| k.spent as f64 / 1e6).sum();
    assert!(spent > 0.45 && spent < 0.6, "the tree stopped about one and a half times its cap: {spent}");
}

#[test]
fn a_step_is_given_the_less_of_its_own_usd_and_what_the_tree_has_left_and_a_tree_with_nothing_left_starts_nothing() {
    let flows = clean(
        r#"[[flow]]
id = "two"
steps = [
  { id = "a", do = "worker", goal = "first", usd = 5.0 },
  { id = "b", do = "worker", goal = "second", usd = 0.02, after = ["a"] },
]"#,
    );
    let worker = Scripted::new("worker", vec![vec![Beat::Session("s".into()), Beat::Spend(0.01), end("ok")], vec![Beat::Session("s".into()), Beat::Spend(0.01), end("ok")]]);
    let first = rig(&flows, vec![worker], &[]);
    let id = capped(&first, "flow:two", 0.10, serde_json::json!({}));
    let row = answering(&first, &id, &[]);
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}\n{}", row.state, first.dump());
    let limit = |goal: &str| first.tasks.book().children(&id).into_iter().find(|k| k.spec.goal == goal).and_then(|k| k.spec.limits.usd_soft).expect("a limit");
    assert!((limit("first") - 0.10).abs() < 1e-9, "its own $5 is cut to what the tree has: {}", limit("first"));
    assert!((limit("second") - 0.02).abs() < 1e-9, "its own $0.02 is less than the $0.09 left: {}", limit("second"));
    // past the soft cap and not at the hard one, nothing new is started under it
    let flows = clean("[[flow]]\nid = \"two\"\nsteps = [{ id = \"a\", do = \"worker\", goal = \"first\" }, { id = \"b\", do = \"worker\", goal = \"second\", after = [\"a\"] }]");
    let worker = Scripted::new("worker", vec![vec![Beat::Session("s".into()), Beat::Spend(0.12), end("ok")], vec![end("never")]]);
    let rig = rig(&flows, vec![worker.clone()], &[]);
    let id = capped(&rig, "flow:two", 0.10, serde_json::json!({}));
    let row = answering(&rig, &id, &[]);
    assert!(why(&row).contains("the money of the task it belongs to has run out"), "{:?}", row.state);
    assert_eq!(worker.starts().len(), 1);
}

#[test]
fn a_task_with_no_tree_is_capped_exactly_as_before_and_one_without_a_cap_has_none() {
    let doer = Scripted::new("script", vec![vec![Beat::Session("s".into()), Beat::Spend(0.005), Beat::End(Some("done".into()), 0)]]);
    let plain = Rig::new(vec![doer]);
    let mut spec = j4n_core::task::TaskSpec::new("go", "script");
    spec.limits.usd_soft = Some(0.004);
    let id = plain.tasks.spawn(spec, Actor::Operator).unwrap();
    let row = plain.finished(&id);
    assert!(row.capped && matches!(row.state, State::Done { ok: true }));
    let said = plain.of_kind("task.cap_reached");
    assert_eq!(said.len(), 1);
    assert!(said[0].data.get("tree").is_none() && said[0].data["spent"].as_f64() == Some(0.005), "{:?}", said[0].data);
    // no cap above, none given: the child is not capped by anyone
    let flows = clean("[[flow]]\nid = \"one\"\nsteps = [{ id = \"a\", do = \"worker\", goal = \"g\" }]");
    let rig = rig(&flows, vec![Scripted::new("worker", vec![vec![end("ok")]])], &[]);
    let id = rig.spawn("go", "flow:one");
    answering(&rig, &id, &[]);
    assert!(rig.tasks.book().children(&id).iter().all(|k| k.spec.limits.usd_soft.is_none()));
}

#[test]
fn a_flow_parked_at_its_soft_cap_stops_what_is_going_under_it_and_says_capped() {
    let flows = clean("[[flow]]\nid = \"one\"\nsteps = [{ id = \"a\", do = \"worker\", goal = \"g\" }]");
    let link = Link::default();
    let fd = doers(&flows, &link);
    let worker = Scripted::new("worker", vec![vec![Beat::Session("s".into()), Beat::Spend(0.12), Beat::Forever]]);
    // the base pack's rules are on: `soft-cap` parks the task that passes its cap
    let rig = Rig::based(vec![worker], "", |c| c.doers.extend(fd));
    link.set(Arc::new(TaskHost { tasks: rig.tasks.downgrade(), who: Arc::new(Roles(BTreeMap::new())), log: rig.log.clone(), lanes: Default::default(), root: Default::default(), settings: Default::default() }));
    let id = capped(&rig, "flow:one", 0.10, serde_json::json!({}));
    let root = rig.finished(&id);
    assert!(matches!(root.state, State::Stopped { why: j4n_core::task::Why::Capped }), "a flow stops at its cap: {:?}\n{}", root.state, rig.dump());
    let kid = rig.tasks.book().children(&id).into_iter().find(|k| k.spec.doer == "worker").expect("the step's task");
    let kid = rig.finished(&kid.id);
    assert!(matches!(kid.state, State::Stopped { why: j4n_core::task::Why::Capped }), "{:?}", kid.state);
    let stopped = rig.of_kind("task.stopped").into_iter().find(|e| e.task.as_deref() == Some(kid.id.as_str())).expect("its stop is in the Log");
    assert_eq!(stopped.data["why"]["kind"], "capped", "{:?}", stopped.data);
}
