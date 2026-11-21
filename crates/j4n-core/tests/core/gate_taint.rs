//! The Gate keeps which tasks have read untrusted content, and stamps it on their actions so a rule
//! can read `action.taint`. The rules of the pack `guard` (`gates/taint.toml`) are tested where the tools are (`j4n-connect`).

use crate::world::*;
use j4n_core::change::Actor;
use j4n_core::gate::check::Gates;
use j4n_core::gate::grant::Grants;
use j4n_core::gate::{Action, Gate, Verb, Verdict};
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};

const RULE: &str = r#"
api = 1
[[gate]]
id = "after-reading"
on = ["write"]
when = "action.taint == 'web'"
verdict = "block"
why = "this run read the web"

[[gate]]
id = "after-anything"
on = ["net"]
when = "action.taint"
verdict = "ask"
why = "this run read something"
"#;

fn gate() -> Gate {
    let (t, problems) = Tables::load(vec![Layer::new(LayerKind::Pack, "gates", vec![SourceFile { path: "g.toml".into(), text: RULE.into() }])]);
    assert!(refused(&problems).is_empty(), "{problems:?}");
    Gate::new(Gates::load(&t).0, Grants::new("s1"))
}

fn write(task: &str) -> Action {
    Action::new(Verb::Write, Actor::Doer("d".into()), "out.txt").task(task)
}

fn says(v: &Verdict) -> &'static str {
    v.word()
}

#[test]
fn a_task_that_read_untrusted_content_has_its_actions_stamped_and_a_rule_reads_it() {
    let g = gate();
    assert_eq!(says(&g.check(&write("t1"))), "allow");
    assert!(g.taint("t1", "web"), "the first time is new");
    assert!(!g.taint("t1", "mcp:notes"), "the second is not, and the first source stays");
    assert_eq!(g.tainted("t1").as_deref(), Some("web"));
    match g.check(&write("t1")) {
        Verdict::Block { by, .. } => assert_eq!(by, "after-reading"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn another_task_is_not_tainted_and_a_clone_of_the_gate_shares_what_one_task_read() {
    let g = gate();
    g.taint("t1", "web");
    assert_eq!(says(&g.check(&write("t2"))), "allow", "taint is per task");
    assert_eq!(says(&g.check(&Action::new(Verb::Write, Actor::Doer("d".into()), "x"))), "allow", "an action of no task is nobody's");
    let other = g.clone();
    assert_eq!(says(&other.check(&write("t1"))), "block", "the loop's clone of the Gate sees what the tools told");
    assert_eq!(other.tainted("t1").as_deref(), Some("web"));
}

#[test]
fn a_task_that_is_over_is_forgotten_and_the_action_given_to_check_is_not_changed() {
    let g = gate();
    g.taint("t1", "web");
    let a = write("t1");
    g.check(&a);
    assert_eq!(a.taint, None, "the stamp is on a copy");
    g.untaint("t1");
    assert_eq!(g.tainted("t1"), None);
    assert_eq!(says(&g.check(&write("t1"))), "allow");
    // a rule that only asks "has it read anything" reads the stamp as text
    g.taint("t3", "mcp:notes");
    assert_eq!(says(&g.check(&Action::new(Verb::Net, Actor::Doer("d".into()), "https://a.test").task("t3"))), "ask");
    assert_eq!(says(&g.check(&Action::new(Verb::Net, Actor::Doer("d".into()), "https://a.test").task("t4"))), "allow");
}
