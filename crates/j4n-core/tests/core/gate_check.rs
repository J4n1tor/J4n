//! The Gate's rules engine (`Gates::load`, `check`, `explain`, the rows of the base pack, and the law that nothing
//! loosens a Block. The matcher is a small fake here (`fake`), so these tests hold whether or not the real `danger` is in
//! the tree; the two tests at the end run on the real one when it is there.

use crate::gate_danger::BENCH_READS;
use crate::world::*;
use j4n_core::change::{Actor, Where};
use j4n_core::gate::check::{DEFAULT, Gates, Says, check, explain};
use j4n_core::gate::danger::danger_value;
use j4n_core::gate::{Action, Covers, NoGrants, ReadOnly, Verb, Verdict};
use j4n_core::schema::DataClass;
use j4n_core::table::{Layer, LayerKind, Level, Op, Problem, SourceFile, Tables, Write};
use j4n_core::value::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

// ---- the fixtures ----

/// A stand-in for the matcher: just enough of its vocabulary for the rules below.
fn fake(cmd: &str) -> Value {
    let norm = cmd.split_whitespace().collect::<Vec<_>>().join(" ");
    let class = if cmd.contains('\0') {
        "unknown"
    } else if norm.starts_with("git push") {
        "push"
    } else if norm.starts_with("rm ") {
        "delete"
    } else if norm.starts_with("ls") || norm.starts_with("cat ") && !norm.contains('>') {
        "safe"
    } else {
        "other"
    };
    let writes = ["rm ", "touch ", "git push", "git commit"].iter().any(|p| norm.starts_with(p)) || norm.contains('>');
    let compound = [";", "&&", "||", "|", "\n"].iter().any(|s| cmd.contains(s));
    let m: BTreeMap<String, Value> = [
        ("class", Value::from(class)),
        ("writes", Value::Bool(writes)),
        ("compound", Value::Bool(compound)),
        ("broad", Value::Bool(false)),
        ("program", Value::from(norm.split(' ').next().unwrap_or(""))),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    Value::Map(m)
}

/// Blocks and asks of the kind `shell.toml` will hold, in the words of the fake.
const SHELL: &str = r#"
api = 1
[[gate]]
id = "push-is-yours"
on = ["shell"]
when = "danger(cmd).class == 'push'"
verdict = "block"
why = "pushing is yours"
[[gate]]
id = "delete-asks"
on = ["shell"]
when = "danger(cmd).class == 'delete'"
verdict = "ask"
why = "this deletes"
risk = 70
[[gate]]
id = "safe-lines-run"
on = ["shell"]
when = "danger(cmd).class == 'safe'"
verdict = "allow"
why = "a plain read"
"#;

/// The base rows of this step (`modes`, `paths`, `egress`), read from the pack directory.
fn base() -> Layer {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/base/gates");
    let files = ["modes.toml", "paths.toml", "egress.toml"]
        .map(|f| SourceFile { path: f.to_string(), text: std::fs::read_to_string(dir.join(f)).unwrap() });
    Layer::new(LayerKind::Pack, "gates", files.to_vec())
}

fn rules(text: &str) -> Layer {
    layer(LayerKind::Pack, "rules", &[("rules.toml", text)])
}

fn user(text: &str) -> Layer {
    home(&[("gates.toml", text)])
}

fn proj(text: &str) -> Layer {
    project(&[("gates.toml", text)])
}

fn gates_of(layers: Vec<Layer>) -> (Gates, Vec<Problem>) {
    let (t, problems) = Tables::load(layers);
    assert!(refused(&problems).is_empty(), "{problems:?}");
    let (g, more) = Gates::load(&t);
    (g.with_danger(fake), more)
}

fn gates(layers: Vec<Layer>) -> Gates {
    gates_of(layers).0
}

fn who() -> Actor {
    Actor::Doer("d".to_string())
}

fn sh(cmd: &str) -> Action {
    Action::shell(who(), cmd)
}

fn act(verb: Verb, target: &str) -> Action {
    Action::new(verb, who(), target)
}

fn write_in(path: &str, inside: bool) -> Action {
    Action { inside: Some(inside), ..act(Verb::Write, path) }
}

fn mode(mut a: Action, m: ReadOnly) -> Action {
    a.read_only = m;
    a
}

fn egress(class: Option<DataClass>, zone: &str) -> Action {
    Action { class, ..act(Verb::Egress, zone) }
}

fn run(g: &Gates, a: &Action) -> Verdict {
    check(g, &NoGrants, a)
}

/// A grant that covers everything, and counts how often it was asked.
struct Always(&'static str, AtomicUsize);

impl Always {
    fn new(id: &'static str) -> Always {
        Always(id, AtomicUsize::new(0))
    }

    fn asked(&self) -> usize {
        self.1.load(Ordering::SeqCst)
    }
}

impl Covers for Always {
    fn covers(&self, _action: &Action) -> Option<String> {
        self.1.fetch_add(1, Ordering::SeqCst);
        Some(self.0.to_string())
    }
}

fn is_block(v: &Verdict, by: &str) -> bool {
    matches!(v, Verdict::Block { by: b, .. } if b == by)
}

fn allow_default() -> Verdict {
    Verdict::Allow { by: DEFAULT.to_string() }
}

/// A row as the Table would give it, for the cases the Table itself refuses.
fn row(id: &str, on: &[&str], when: Option<&str>, verdict: &str) -> Value {
    let mut m: BTreeMap<String, Value> = BTreeMap::new();
    m.insert("id".into(), Value::from(id));
    m.insert("on".into(), Value::List(on.iter().map(|v| Value::from(*v)).collect()));
    m.insert("verdict".into(), Value::from(verdict));
    m.insert("why".into(), Value::from("because"));
    if let Some(w) = when {
        m.insert("when".into(), Value::from(w));
    }
    Value::Map(m)
}

fn without(mut r: Value, field: &str) -> Value {
    if let Value::Map(m) = &mut r {
        m.remove(field);
    }
    r
}

fn with(mut r: Value, field: &str, v: Value) -> Value {
    if let Value::Map(m) = &mut r {
        m.insert(field.to_string(), v);
    }
    r
}

// ---- precedence, default, the shape of a verdict ----

#[test]
fn no_rule_at_all_is_allow_by_default() {
    let g = gates(vec![]);
    assert!(g.is_empty());
    for verb in Verb::ALL {
        assert_eq!(run(&g, &act(verb, "x")), allow_default(), "{verb:?}");
    }
}

#[test]
fn block_beats_ask_beats_allow_whatever_the_ids() {
    // ids run against the precedence: the allow sorts first, the block last
    let g = gates(vec![rules(
        r#"api = 1
[[gate]]
id = "a-allow"
on = ["shell"]
when = "true"
verdict = "allow"
why = "fine"
[[gate]]
id = "b-ask"
on = ["shell"]
when = "true"
verdict = "ask"
why = "careful"
risk = 30
[[gate]]
id = "c-block"
on = ["shell"]
when = "true"
verdict = "block"
why = "never"
"#,
    )]);
    let a = sh("anything");
    assert_eq!(run(&g, &a), Verdict::Block { why: "never".into(), by: "c-block".into() });
    let seen: Vec<_> = explain(&g, &NoGrants, &a).into_iter().map(|h| (h.rule, h.says)).collect();
    assert_eq!(seen, [("a-allow".into(), Says::Allow), ("b-ask".into(), Says::Ask), ("c-block".into(), Says::Block)]);
    // without the block, the ask stands; without the ask, the allow names itself
    let (g, _) = Gates::from_rows(&[row("a-allow", &["shell"], None, "allow"), row("b-ask", &["shell"], None, "ask")]);
    assert_eq!(run(&g, &a), Verdict::Ask { risk: 50, why: "because".into(), by: "b-ask".into() });
    let (g, _) = Gates::from_rows(&[row("a-allow", &["shell"], None, "allow")]);
    assert_eq!(run(&g, &a), Verdict::Allow { by: "a-allow".into() });
}

#[test]
fn the_first_block_by_id_and_the_riskiest_ask_win_and_ties_go_to_the_first_id() {
    let mut all = vec![
        row("z-block", &["read"], None, "block"),
        row("m-block", &["read"], None, "block"),
        row("q-allow", &["net"], None, "allow"),
        row("b-allow", &["net"], None, "allow"),
    ];
    for (id, risk) in [("ask-1", 40.0), ("ask-2", 80.0), ("ask-3", 80.0), ("ask-0", 10.0)] {
        all.push(with(row(id, &["write"], None, "ask"), "risk", Value::Num(risk)));
    }
    let mut reversed = all.clone();
    reversed.reverse();
    let mut rotated = all.clone();
    rotated.rotate_left(5);
    for rows in [all, reversed, rotated] {
        let (g, problems) = Gates::from_rows(&rows);
        assert!(problems.is_empty());
        assert!(is_block(&run(&g, &act(Verb::Read, "f")), "m-block"), "the first block by id");
        assert_eq!(
            run(&g, &act(Verb::Write, "f")),
            Verdict::Ask { risk: 80, why: "because".into(), by: "ask-2".into() },
            "the riskiest ask, the first id of a tie"
        );
        assert_eq!(run(&g, &act(Verb::Net, "h")), Verdict::Allow { by: "b-allow".into() }, "the first allow by id");
    }
}

#[test]
fn a_rule_applies_only_to_its_verbs_and_only_when_its_when_holds() {
    let g = gates(vec![rules(
        r#"api = 1
[[gate]]
id = "reads-of-secrets"
on = ["read", "write"]
when = "path matches '*.secret'"
verdict = "block"
why = "no"
[[gate]]
id = "always-asks-about-spend"
on = ["spend"]
when = "true"
verdict = "ask"
why = "money"
[[gate]]
id = "no-when-means-always"
on = ["connect"]
verdict = "block"
why = "no connecting"
"#,
    )]);
    assert!(is_block(&run(&g, &act(Verb::Read, "a.secret")), "reads-of-secrets"));
    assert!(is_block(&run(&g, &act(Verb::Write, "dir/a.secret")), "reads-of-secrets"));
    assert_eq!(run(&g, &act(Verb::Read, "a.txt")).word(), "allow", "the when does not hold");
    assert_eq!(run(&g, &act(Verb::Net, "a.secret")).word(), "allow", "not one of its verbs");
    assert_eq!(run(&g, &act(Verb::Spend, "x")).by(), "always-asks-about-spend");
    assert_eq!(run(&g, &act(Verb::Connect, "x")).word(), "block", "a rule with no `when` applies to all of its verbs");
    assert_eq!(run(&g, &act(Verb::Spawn, "x")), allow_default());
}

#[test]
fn a_grant_turns_an_ask_into_an_allow_and_is_never_asked_about_a_block() {
    let g = gates(vec![rules(SHELL)]);
    let ask = sh("rm -rf build");
    let grant = Always::new("g7");
    assert_eq!(check(&g, &grant, &ask), Verdict::Allow { by: "grant:g7".into() });
    assert_eq!(grant.asked(), 1);
    assert_eq!(run(&g, &ask).word(), "ask", "no grant, no allow");
    let grant = Always::new("g8");
    assert!(is_block(&check(&g, &grant, &sh("git push")), "push-is-yours"));
    assert_eq!(grant.asked(), 0, "a block never even asks the grants");
    // an action no rule has a say in is allowed by default, grants or not
    let grant = Always::new("g9");
    assert_eq!(check(&g, &grant, &sh("make")), allow_default());
    assert_eq!(grant.asked(), 0, "and there is no ask to cover");
}

#[test]
fn explain_lists_every_rule_that_applied_with_what_it_said_and_the_grant_that_would_cover() {
    let g = gates(vec![rules(SHELL), base()]);
    let hits = explain(&g, &Always::new("g1"), &mode(sh("rm -rf x"), ReadOnly::Inferred));
    let seen: Vec<(&str, Says, Option<&str>)> = hits.iter().map(|h| (h.rule.as_str(), h.says, h.grant.as_deref())).collect();
    assert_eq!(seen, [("delete-asks", Says::Ask, Some("g1")), ("inferred-read-only-shell-writes", Says::Ask, Some("g1"))]);
    assert_eq!((hits[0].risk, hits[1].risk), (70, 40));
    assert_eq!(hits[0].why, "this deletes");
    let hits = explain(&g, &Always::new("g1"), &sh("ls -la"));
    assert_eq!(hits.len(), 1);
    assert_eq!((hits[0].rule.as_str(), hits[0].says, hits[0].grant.clone()), ("safe-lines-run", Says::Allow, None));
    assert!(explain(&g, &NoGrants, &sh("make")).is_empty(), "nothing applied");
    let push = explain(&g, &Always::new("g1"), &sh("git push"));
    assert_eq!(push[0].says, Says::Block);
    assert_eq!(push[0].grant, None, "a block is not covered");
}

// ---- the world a `when` reads ----

#[test]
fn a_when_reads_the_whole_action_and_the_root_that_fits_the_verb() {
    let cases: Vec<(&str, Action, bool)> = vec![
        ("action.verb == 'shell' and cmd == 'ls' and target == 'ls'", sh("ls"), true),
        ("action.actor == 'd' and action.actor_kind == 'doer'", sh("ls"), true),
        ("action.actor == 'operator' and action.actor_kind == 'operator'", Action::shell(Actor::Operator, "ls"), true),
        ("action.actor == 'core' and action.actor_kind == 'core'", Action::shell(Actor::Core, "ls"), true),
        ("action.actor == 'm' and action.actor_kind == 'model'", Action::shell(Actor::Model("m".into()), "ls"), true),
        ("action.task == 't1'", sh("ls").task("t1"), true),
        ("action.task == 't1'", sh("ls"), false),
        ("action.site == '/w'", Action { site: Some("/w".into()), ..sh("ls") }, true),
        ("action.usd > $1.50 and action.usd < $2", Action { usd: 1.75, ..act(Verb::Spend, "x") }, true),
        ("action.read_only == 'explicit'", mode(sh("ls"), ReadOnly::Explicit), true),
        ("action.read_only == 'no'", sh("ls"), true),
        ("action.inside == true", write_in("a", true), true),
        ("action.inside == false", write_in("a", false), true),
        ("action.inside == false", act(Verb::Write, "a"), false),
        ("action.class == 'secret'", egress(Some(DataClass::Secret), "any"), true),
        ("action.class == 'project'", egress(Some(DataClass::Project), "any"), true),
        ("path == 'a/b' and target == 'a/b'", act(Verb::Read, "a/b"), true),
        ("path == 'a/b'", act(Verb::Apply, "a/b"), true),
        ("host == 'example.org'", act(Verb::Net, "example.org"), true),
        ("cmd == 'x'", act(Verb::Read, "x"), false),
        ("path == 'x'", sh("x"), false),
        ("host == 'x'", act(Verb::Read, "x"), false),
    ];
    for (when, action, expect) in cases {
        let on: Vec<&str> = Verb::ALL.iter().map(|v| v.word()).collect();
        let (g, problems) = Gates::from_rows(&[row("r", &on, Some(when), "block")]);
        assert!(problems.is_empty(), "{when}: {problems:?}");
        assert_eq!(run(&g, &action).word() == "block", expect, "{when} on {action:?}");
    }
}

#[test]
fn the_matcher_is_asked_once_per_line_however_many_rules_read_it() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    fn counted(cmd: &str) -> Value {
        CALLS.fetch_add(1, Ordering::SeqCst);
        fake(cmd)
    }
    let rows: Vec<Value> =
        (0..50).map(|n| row(&format!("r{n}"), &["shell"], Some("danger(cmd).class == 'push'"), "ask")).collect();
    let g = Gates::from_rows(&rows).0.with_danger(counted);
    assert_eq!(run(&g, &sh("git push")).word(), "ask");
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
}

#[test]
fn a_target_of_any_shape_is_checked_without_a_panic() {
    let g = gates(vec![rules(SHELL), base()]);
    let huge = "x".repeat(300_000);
    let odd = [
        "", " ", "\0", "\u{202e}git push", "git push\0", "日本語 ;; ⚠", "\n\n", "'", "$(", "`", huge.as_str(), "..", "/", "~",
    ];
    for target in odd {
        for verb in Verb::ALL {
            for ro in [ReadOnly::No, ReadOnly::Explicit, ReadOnly::Inferred] {
                let a = mode(act(verb, target), ro);
                assert_eq!(run(&g, &a), run(&g, &a), "deterministic");
                explain(&g, &NoGrants, &a);
            }
        }
    }
}

// ---- rules that cannot be read fail closed ----

#[test]
fn a_rule_that_does_not_parse_asks_for_its_verbs_and_names_the_problem() {
    let (g, problems) = Gates::from_rows(&[
        row("good-block", &["write"], Some("path matches '*.lock'"), "block"),
        row("broken", &["shell", "net"], Some("danger(cmd).class == "), "block"),
    ]);
    assert_eq!(problems.len(), 1);
    assert!(problems[0].msg.contains("broken") && problems[0].msg.contains("does not parse"), "{:?}", problems[0]);
    assert_eq!(problems[0].level, Level::Warn);
    for verb in [Verb::Shell, Verb::Net] {
        match run(&g, &act(verb, "anything")) {
            Verdict::Ask { risk, why, by } => {
                assert_eq!((risk, by.as_str()), (90, "broken"));
                assert!(why.contains("broken") && why.contains("does not parse") && why.contains("asking"), "{why}");
            }
            other => panic!("{verb:?}: {other:?}"),
        }
    }
    assert_eq!(run(&g, &act(Verb::Read, "x")), allow_default(), "other verbs are not touched");
    assert!(is_block(&run(&g, &act(Verb::Write, "a.lock")), "good-block"), "the good rule still works");
}

#[test]
fn a_grant_does_not_cover_the_ask_of_a_rule_that_could_not_be_read() {
    let (g, _) = Gates::from_rows(&[row("broken", &["shell"], Some("(("), "block")]);
    let grant = Always::new("g1");
    assert_eq!(check(&g, &grant, &sh("ls")).word(), "ask");
    assert_eq!(grant.asked(), 0);
    // beside a readable ask the unreadable one still cannot be covered
    let (g, _) = Gates::from_rows(&[row("broken", &["shell"], Some("(("), "block"), row("fine", &["shell"], None, "ask")]);
    assert_eq!(check(&g, &Always::new("g1"), &sh("ls")).word(), "ask");
}

#[test]
fn every_way_a_row_can_be_unreadable_asks() {
    // (what is wrong, the row, whether every verb is asked about)
    let bad: Vec<(&str, Value, bool)> = vec![
        ("unknown root", row("r", &["shell"], Some("elsewhere.x == 1"), "block"), false),
        ("function misuse", row("r", &["shell"], Some("'a' matches 3"), "block"), false),
        ("when is not text", with(row("r", &["shell"], None, "block"), "when", Value::Num(1.0)), false),
        ("too deep", row("r", &["shell"], Some(&"(".repeat(500)), "block"), false),
        ("unknown verdict", row("r", &["shell"], None, "permit"), false),
        ("no verdict", without(row("r", &["shell"], None, "block"), "verdict"), false),
        ("unknown verb in on", row("r", &["shell", "teleport"], None, "block"), true),
        ("empty on", row("r", &[], None, "block"), true),
        ("no on", without(row("r", &["shell"], None, "block"), "on"), true),
    ];
    for (what, r, every_verb) in bad {
        let (g, problems) = Gates::from_rows(&[r]);
        assert_eq!(problems.len(), 1, "{what}");
        assert_eq!(run(&g, &sh("ls")).word(), "ask", "{what}: shell asks");
        let other = run(&g, &act(Verb::Read, "f")).word();
        assert_eq!(other, if every_verb { "ask" } else { "allow" }, "{what}");
    }
}

#[test]
fn an_allow_row_that_cannot_be_read_allows_nothing() {
    let (g, problems) = Gates::from_rows(&[row("loose", &["shell"], Some("cmd matches"), "allow")]);
    assert_eq!(problems.len(), 1);
    assert!(g.is_empty());
    assert_eq!(run(&g, &sh("ls")), allow_default(), "it is not even named");
}

#[test]
fn a_gate_entry_the_table_refused_is_put_back_as_an_ask() {
    let text = "api = 1\n[[gate]]\nid = \"bad\"\non = [\"shell\"]\nwhen = \"danger(cmd).class == \"\nverdict = \"block\"\nwhy = \"x\"\n\
                [[gate]]\nid = \"fine\"\non = [\"read\"]\nwhen = \"true\"\nverdict = \"block\"\nwhy = \"no\"\n";
    let (t, problems) = Tables::load(vec![rules(text)]);
    assert_eq!(refused(&problems).len(), 1, "{problems:?}");
    let (g, more) = Gates::load(&t);
    assert!(more.is_empty());
    let g = g.with_danger(fake);
    assert_eq!(run(&g, &sh("ls")).word(), "allow", "the Table dropped the row: the gates alone cannot know");
    let g = g.distrust(&problems);
    match run(&g, &sh("ls")) {
        Verdict::Ask { risk: 90, why, by } => {
            assert!(by.starts_with("refused:") && by.contains("bad"), "{by}");
            assert!(why.contains("refused"), "{why}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(run(&g, &act(Verb::Spend, "x")).word(), "ask", "every verb");
    assert!(is_block(&run(&g, &act(Verb::Read, "x")), "fine"), "a block still comes first");
    assert_eq!(check(&g, &Always::new("g"), &sh("ls")).word(), "ask", "no grant covers it");
}

#[test]
fn a_file_the_table_refused_whole_asks_whatever_its_name_for_what_it_held_cannot_be_known() {
    let broken = "this is [not toml";
    // a gate file, and one that does not say so: a syntax error hides every row it held
    for name in ["gates/shell.toml", "lamps.toml", "rules.toml"] {
        let (t, problems) = Tables::load(vec![layer(LayerKind::Pack, "p", &[(name, broken)])]);
        let g = Gates::load(&t).0.with_danger(fake).distrust(&problems);
        assert_eq!(g.len(), 1, "{name}: {problems:?}");
        assert_eq!(run(&g, &sh("ls")).word(), "ask", "{name}");
    }
    // a file with no `api` is refused whole too
    let (t, problems) = Tables::load(vec![layer(LayerKind::Pack, "p", &[("rules.toml", "[[gate]]\nid = \"x\"\non = [\"shell\"]\nverdict = \"block\"\n")])]);
    assert_eq!(Gates::load(&t).0.with_danger(fake).distrust(&problems).len(), 1, "{problems:?}");
}

#[test]
fn an_entry_of_another_kind_the_table_refused_does_not_make_the_gate_ask() {
    let text = "api = 1\n[[lamp]]\nid = \"a\"\nlit = \"not a bool\"\n[[gate]]\nid = \"fine\"\non = [\"read\"]\nwhen = \"true\"\nverdict = \"block\"\nwhy = \"no\"\n";
    let (t, problems) = Tables::load(vec![layer(LayerKind::Pack, "p", &[("lamps.toml", text)])]);
    assert!(!refused(&problems).is_empty(), "the Table refuses the lamp: {problems:?}");
    let g = Gates::load(&t).0.with_danger(fake).distrust(&problems);
    assert_eq!(g.len(), 1, "{problems:?}");
    assert_eq!(run(&g, &sh("ls")).word(), "allow", "an entry the Table named as a lamp is not a gate's");
}

#[test]
fn warnings_do_not_make_the_gate_ask() {
    let g = Gates::from_rows(&[]).0.distrust(&[Problem::warn("gates:x.toml [[gate]] y", "a floor was ignored")]);
    assert!(g.is_empty());
}

// ---- layers, trust and floors, through the Table ----

#[test]
fn an_untrusted_project_can_add_ask_and_block_rows_but_not_allow_rows() {
    let project_rows = proj(
        r#"api = 1
[[gate]]
id = "mine-blocks-npm"
on = ["shell"]
when = "cmd matches 'npm*'"
verdict = "block"
why = "this project says no"
[[gate]]
id = "mine-asks-make"
on = ["shell"]
when = "cmd matches 'make*'"
verdict = "ask"
why = "this project says careful"
[[gate]]
id = "mine-allows-delete"
on = ["shell"]
when = "true"
verdict = "allow"
why = "trust me"
"#,
    );
    let (g, problems) = gates_of(vec![rules(SHELL), project_rows]);
    assert!(problems.is_empty());
    assert_eq!(g.len(), 5, "the allow row is not there");
    assert!(is_block(&run(&g, &sh("npm publish")), "mine-blocks-npm"));
    assert_eq!(run(&g, &sh("make all")).by(), "mine-asks-make");
    assert_eq!(run(&g, &sh("rm -rf x")).word(), "ask", "the project's allow-all did not loosen the delete ask");
    assert_eq!(run(&g, &sh("cp a b")), allow_default(), "and it does not even name itself");
}

#[test]
fn a_layer_above_cannot_change_the_floors_of_a_rule_it_did_not_declare() {
    let above = r#"api = 1
[[gate]]
id = "push-is-yours"
on = ["read"]
when = "false"
verdict = "allow"
why = "ok then"
"#;
    // the operator's own layer: the floors are ignored, the rest of the row counts
    let (t, problems) = Tables::load(vec![rules(SHELL), user(above)]);
    let floors = warned(&problems).into_iter().filter(|w| w.contains("floor")).count();
    assert_eq!(floors, 4, "on, when, verdict and why are all ignored: {problems:?}");
    let g = Gates::load(&t).0.with_danger(fake);
    assert!(is_block(&run(&g, &sh("git push")), "push-is-yours"));
    assert_eq!(run(&g, &act(Verb::Read, "x")), allow_default());
    // a project's row is never trusted: it widens, so it is dropped whole and the pack's row stands
    let (t, problems) = Tables::load(vec![rules(SHELL), proj(above)]);
    assert!(mentions(&warned(&problems), &["push-is-yours", "widen"]), "{problems:?}");
    assert_eq!(t.raw("gate", "push-is-yours").unwrap().from["verdict"], "rules", "the project set none of it");
    let g = Gates::load(&t).0.with_danger(fake);
    assert!(is_block(&run(&g, &sh("git push")), "push-is-yours"));
    assert_eq!(run(&g, &act(Verb::Read, "x")), allow_default());
}

#[test]
fn a_layer_cannot_fill_in_a_when_the_declaring_layer_left_out() {
    // the pack's rule says "all shell is blocked" by leaving `when` out; a layer above adds `when = "false"`
    let pack = rules("api = 1\n[[gate]]\nid = \"no-shell\"\non = [\"shell\"]\nverdict = \"block\"\nwhy = \"no shell\"\n");
    type Make = fn(&str) -> Layer;
    let cases: [(Make, &str); 2] = [(user, "home"), (proj, "project")];
    for (above, name) in cases {
        let layer = above("api = 1\n[[gate]]\nid = \"no-shell\"\nwhen = \"false\"\n");
        let (g, problems) = gates_of(vec![pack.clone(), layer]);
        assert!(is_block(&run(&g, &sh("ls")), "no-shell"), "{name}");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].msg.contains("`when`") && problems[0].msg.contains("ignored"), "{problems:?}");
    }
}

#[test]
fn a_layer_cannot_fill_in_the_on_of_a_rule_either_and_the_rule_then_covers_every_verb() {
    let pack = rules("api = 1\n[[gate]]\nid = \"nothing\"\nwhen = \"true\"\nverdict = \"block\"\nwhy = \"no\"\n");
    let above = user("api = 1\n[[gate]]\nid = \"nothing\"\non = [\"connect\"]\n");
    let (g, problems) = gates_of(vec![pack, above]);
    assert_eq!(problems.len(), 1);
    assert!(is_block(&run(&g, &sh("ls")), "nothing"));
    assert!(is_block(&run(&g, &act(Verb::Read, "x")), "nothing"));
}

#[test]
fn an_allow_row_put_together_from_two_layers_is_dropped() {
    let pack = rules("api = 1\n[[gate]]\nid = \"split\"\nverdict = \"allow\"\nwhy = \"fine\"\n");
    let above = user("api = 1\n[[gate]]\nid = \"split\"\non = [\"shell\"]\nwhen = \"true\"\n");
    let (g, problems) = gates_of(vec![pack, above]);
    assert!(g.is_empty());
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert_eq!(run(&g, &sh("ls")), allow_default());
}

#[test]
fn only_the_operator_and_the_core_write_gate_rows() {
    let (t, problems) = Tables::load(vec![rules(SHELL), user("api = 1\n")]);
    assert!(refused(&problems).is_empty());
    let write = |by: Actor, verdict: &str| Write {
        to: Where::User,
        file: None,
        kind: "gate".into(),
        id: "mine".into(),
        op: Op::Set(format!("on = [\"shell\"]\nverdict = \"{verdict}\"\nwhy = \"x\"\nwhen = \"true\"").parse().unwrap()),
        by,
        cause: None,
        at_ms: 1,
    };
    for by in [Actor::Rule("r".into()), Actor::Doer("d".into()), Actor::Model("m".into()), Actor::Pack("p".into())] {
        let kind = by.kind();
        assert!(t.plan(&write(by, "block")).is_err(), "{kind:?} must not write a gate row");
    }
    for by in [Actor::Operator, Actor::Core] {
        assert!(t.plan(&write(by, "block")).is_ok());
    }
    assert_eq!(t.plan(&write(Actor::Operator, "allow")).unwrap().widens, ["verdict"], "an allow row takes two presses");
    assert!(t.plan(&write(Actor::Operator, "ask")).unwrap().widens.is_empty());
}

// ---- the base rows ----

#[test]
fn the_base_gate_rows_load_clean_and_every_rule_sets_every_floor_itself() {
    let (t, problems) = Tables::load(vec![base()]);
    assert!(problems.is_empty(), "{problems:?}");
    let (g, more) = Gates::load(&t);
    assert!(more.is_empty(), "{more:?}");
    assert_eq!(g.len(), 19, "the 14th is `tell-across-quarantine`, the 15th and 16th are `other-projects-state` and `own-state-is-read`, and `j4n-keys`, `j4n-salt` and `proc-of-others` block J4n's own secrets");
    for id in t.ids("gate") {
        let m = t.raw("gate", id).unwrap();
        let mut must = vec!["on", "when", "verdict", "why", "doc"];
        if m.fields["verdict"].as_str() == Some("ask") {
            must.push("risk");
        }
        for f in must {
            assert_eq!(m.from.get(f).map(String::as_str), Some("gates"), "{id}.{f} must be written in the pack");
        }
    }
}

#[test]
fn an_explicit_read_only_step_blocks_writes_applies_and_writing_lines_but_not_reads() {
    let g = gates(vec![base()]);
    let explicit = |a: Action| mode(a, ReadOnly::Explicit);
    assert!(is_block(&run(&g, &explicit(write_in("src/a.rs", true))), "plan-no-writes"));
    assert!(is_block(&run(&g, &explicit(act(Verb::Apply, "change-1"))), "plan-no-writes"));
    assert!(is_block(&run(&g, &explicit(sh("touch x"))), "plan-no-shell-writes"));
    assert!(is_block(&run(&g, &explicit(sh("echo hi > out.txt"))), "plan-no-shell-writes"));
    assert!(is_block(&run(&g, &explicit(sh("rm -rf build"))), "plan-no-shell-writes"));
    assert!(is_block(&run(&g, &explicit(sh("a thing\0 odd"))), "plan-no-shell-writes"), "a line the matcher cannot place");
    for allowed in [
        explicit(sh("ls -la")),
        explicit(sh("cat Cargo.toml")),
        explicit(act(Verb::Read, "src/a.rs")),
        explicit(act(Verb::Net, "example.org")),
        explicit(act(Verb::Spawn, "worker")),
        explicit(act(Verb::Setting, "theme")),
        explicit(act(Verb::Spend, "x")),
        explicit(act(Verb::Connect, "mail")),
    ] {
        assert_eq!(run(&g, &allowed), allow_default(), "{allowed:?}");
    }
    // the same writes in a step that is not read-only are not stopped by a mode
    assert_eq!(run(&g, &write_in("src/a.rs", true)), allow_default());
    assert_eq!(run(&g, &sh("touch x")), allow_default());
}

#[test]
fn a_matcher_that_says_nothing_makes_an_explicit_read_only_step_block_the_line() {
    let (t, _) = Tables::load(vec![base()]);
    let g = Gates::load(&t).0.with_danger(|_| Value::Missing);
    assert!(is_block(&run(&g, &mode(sh("ls"), ReadOnly::Explicit)), "plan-no-shell-writes"));
    assert_eq!(run(&g, &mode(sh("ls"), ReadOnly::Inferred)).by(), "inferred-read-only-shell-writes");
    assert_eq!(run(&g, &sh("ls")), allow_default());
}

#[test]
fn an_inferred_read_only_task_asks_before_it_writes_and_a_grant_may_cover_the_ask() {
    let g = gates(vec![base()]);
    let inferred = |a: Action| mode(a, ReadOnly::Inferred);
    for a in [
        inferred(write_in("src/a.rs", true)),
        inferred(act(Verb::Apply, "c1")),
        inferred(sh("touch x")),
        inferred(sh("cat a > b")),
    ] {
        let v = run(&g, &a);
        assert!(matches!(v, Verdict::Ask { risk: 40, .. }), "{a:?}: {v:?}");
        assert_eq!(check(&g, &Always::new("g1"), &a), Verdict::Allow { by: "grant:g1".into() });
    }
    for a in [inferred(sh("ls")), inferred(act(Verb::Read, "x")), inferred(act(Verb::Net, "h"))] {
        assert_eq!(run(&g, &a), allow_default(), "{a:?}");
    }
}

#[test]
fn a_read_is_allowed_unless_the_path_names_a_secret_and_then_it_is_refused_whatever_was_granted() {
    let g = gates(vec![base()]);
    for path in [
        ".env",
        ".env.local",
        "app/.env",
        "/home/u/proj/.env.production",
        "./.ENV",
        "a/b/.envrc",
        ".env ",
        "../../.env",
        "src/.environment/x",
        "server.pem",
        "/etc/ssl/private/site.key",
        "certs/x.P12",
        "k.pfx",
        "~/.ssh/id_rsa",
        "a/id_ed25519",
        "x/id_ecdsa",
        ".ssh/config",
        "/home/u/.ssh/known_hosts",
        "/home/u/.aws/credentials",
        ".aws/config",
        "/home/u/.gnupg/pubring.kbx",
        "/home/u/.netrc",
        ".netrc",
        "/home/u/.git-credentials",
        "dir/.ssh/./id_rsa",
        "a/../.ssh/x",
    ] {
        let v = run(&g, &act(Verb::Read, path));
        assert!(matches!(v, Verdict::Block { .. }), "reading `{path}` is refused (a lane never reads a credential, approved or not): {v:?}");
        assert_eq!(check(&g, &Always::new("g"), &act(Verb::Read, path)).word(), "block", "no grant covers `{path}`");
    }
    for path in [
        "src/main.rs",
        "README.md",
        "docs/environment.md",
        "src/env.rs",
        "ssh/config",
        "notes/aws.txt",
        "src/keys.rs",
        "monkey.keyboard",
        "id_rsa.pub",
        ".env.example",
        "config/.env.sample",
        ".gitignore",
        ".github/workflows/ci.yml",
        "pem",
        "x.pem.txt",
        "",
        "/",
    ] {
        assert_eq!(run(&g, &act(Verb::Read, path)), allow_default(), "reading `{path}`");
    }
}

#[test]
fn a_write_outside_the_project_or_not_known_to_be_inside_asks() {
    let g = gates(vec![base()]);
    assert_eq!(run(&g, &write_in("src/a.rs", true)), allow_default());
    assert!(matches!(run(&g, &write_in("/etc/hosts", false)), Verdict::Ask { risk: 40, .. }));
    assert!(matches!(run(&g, &act(Verb::Write, "src/a.rs")), Verdict::Ask { risk: 40, .. }), "inside unknown asks");
    assert_eq!(check(&g, &Always::new("g"), &write_in("/etc/hosts", false)).word(), "allow");
    // a read is not a write
    let outside_read = Action { inside: Some(false), ..act(Verb::Read, "/etc/hosts") };
    assert_eq!(run(&g, &outside_read), allow_default());
}

#[test]
fn the_repositorys_own_folder_is_never_written_by_a_tool_in_any_spelling() {
    let g = gates(vec![base()]);
    for path in [
        ".git/config",
        ".git/hooks/pre-commit",
        "./.git/HEAD",
        "a/b/.git/config",
        "/abs/proj/.git/index",
        ".GIT/config",
        "x/.Git/hooks/x",
        ".git",
        "sub/.git",
        ".git/",
        "a/.git/",
        "a/../.git/config",
        ".git//config",
    ] {
        for inside in [true, false] {
            let a = write_in(path, inside);
            assert!(is_block(&run(&g, &a), "git-internals"), "write `{path}` inside={inside}: {:?}", run(&g, &a));
        }
        assert!(is_block(&run(&g, &act(Verb::Apply, path)), "git-internals"), "apply `{path}`");
        let v = check(&g, &Always::new("g"), &write_in(path, true));
        assert!(is_block(&v, "git-internals"), "a grant does not help `{path}`");
    }
    for path in [
        ".gitignore",
        ".github/ci.yml",
        "src/git/mod.rs",
        "my.git/config",
        "git/x",
        "a/.gitmodules",
        "digit/x",
        ".gi/t",
        "src/.gitkeep",
    ] {
        assert_eq!(run(&g, &write_in(path, true)), allow_default(), "write `{path}`");
    }
    assert_eq!(run(&g, &act(Verb::Read, ".git/config")), allow_default(), "reading the repository is fine");
}

#[test]
fn secrets_never_leave_and_private_data_leaves_only_to_the_local_zone() {
    let g = gates(vec![base()]);
    for zone in ["local", "trusted", "any", "", "elsewhere", "LOCAL"] {
        assert!(is_block(&run(&g, &egress(Some(DataClass::Secret), zone)), "egress-secret"), "secret to `{zone}`");
    }
    assert_eq!(run(&g, &egress(Some(DataClass::Private), "local")), allow_default());
    for zone in ["trusted", "any", "", "elsewhere", "Local"] {
        let a = egress(Some(DataClass::Private), zone);
        assert!(matches!(run(&g, &a), Verdict::Ask { risk: 60, .. }), "private to `{zone}`");
        assert_eq!(check(&g, &Always::new("g"), &a).word(), "allow", "a grant may cover the ask, not the secret");
    }
    for class in [DataClass::Public, DataClass::Project] {
        for zone in ["local", "trusted", "any"] {
            assert_eq!(run(&g, &egress(Some(class), zone)), allow_default(), "{class:?} to {zone}");
        }
    }
    // no class given: not known to be harmless
    assert_eq!(run(&g, &egress(None, "local")), allow_default());
    for zone in ["trusted", "any"] {
        assert_eq!(run(&g, &egress(None, zone)).by(), "egress-unclassified");
    }
}

// ---- nothing loosens a Block ----

/// A Block to attack: the layers it lives in, an action it stops, whether the action's `read_only` matters to the rule, and
/// spellings of the same action that the rule's `when` is meant to catch too.
struct Target {
    name: &'static str,
    layers: fn() -> Vec<Layer>,
    id: &'static str,
    verb: Verb,
    action: Action,
    ignores_mode: bool,
    spellings: Vec<Action>,
}

fn targets() -> Vec<Target> {
    vec![
        Target {
            name: "a shell line that pushes",
            layers: || vec![rules(SHELL)],
            id: "push-is-yours",
            verb: Verb::Shell,
            action: sh("git push origin main"),
            ignores_mode: true,
            spellings: ["git  push", "  git push", "git\tpush --force", "git push\n", "git push origin main  "].map(sh).to_vec(),
        },
        Target {
            name: "a write in an explicit read-only step",
            layers: || vec![base()],
            id: "plan-no-writes",
            verb: Verb::Write,
            action: mode(write_in("src/a.rs", true), ReadOnly::Explicit),
            ignores_mode: false,
            spellings: vec![mode(write_in("src/a.rs", false), ReadOnly::Explicit), mode(act(Verb::Write, "x"), ReadOnly::Explicit)],
        },
        Target {
            name: "a writing line in an explicit read-only step",
            layers: || vec![base()],
            id: "plan-no-shell-writes",
            verb: Verb::Shell,
            action: mode(sh("rm -rf build"), ReadOnly::Explicit),
            ignores_mode: false,
            spellings: ["rm   -rf build", " rm -rf build", "touch x", "echo a > b"]
                .map(|c| mode(sh(c), ReadOnly::Explicit))
                .to_vec(),
        },
        Target {
            name: "a write into the repository's folder",
            layers: || vec![base()],
            id: "git-internals",
            verb: Verb::Write,
            action: write_in(".git/config", true),
            ignores_mode: true,
            spellings: [".GIT/config", "./.git/config", "a/b/.git/hooks/pre-push", "x/.Git/", ".git"]
                .map(|p| write_in(p, true))
                .to_vec(),
        },
        Target {
            name: "secret data leaving",
            layers: || vec![base()],
            id: "egress-secret",
            verb: Verb::Egress,
            action: egress(Some(DataClass::Secret), "local"),
            ignores_mode: true,
            spellings: ["trusted", "any", "", "x"].map(|z| egress(Some(DataClass::Secret), z)).to_vec(),
        },
    ]
}

/// Every way to try to loosen the rule `id` of verb `verb`: (what it is, a layer of the operator's, a layer of a project).
fn loosenings(id: &str, verb: Verb) -> Vec<(String, Option<Layer>, Option<Layer>)> {
    let v = verb.word();
    let all: Vec<&str> = Verb::ALL.iter().map(|v| v.word()).collect();
    let allow = |id: &str, on: String, when: &str| {
        format!("api = 1\n[[gate]]\nid = \"{id}\"\non = {on}\nwhen = \"{when}\"\nverdict = \"allow\"\nwhy = \"let it\"\nrisk = 0\n")
    };
    let mut out = Vec::new();
    for (what, text) in [
        ("an allow row of the same id", allow(id, format!("[\"{v}\"]"), "true")),
        ("an allow row of the same id for every verb", allow(id, format!("{all:?}"), "true")),
        ("an allow row of another id", allow("let-it", format!("[\"{v}\"]"), "true")),
        ("an allow row of another id for every verb", allow("let-it", format!("{all:?}"), "true")),
        ("a bypass allow over safe lines", allow("bypass", format!("{all:?}"), "danger(cmd).class == 'safe' or true")),
        ("an allow row named like a grant", allow(&format!("here-{id}"), format!("[\"{v}\"]"), "target matches '*'")),
        ("the same id with a when that never holds", format!("api = 1\n[[gate]]\nid = \"{id}\"\nwhen = \"false\"\n")),
        ("the same id with no verbs", format!("api = 1\n[[gate]]\nid = \"{id}\"\non = [\"setting\"]\n")),
        ("the same id turned into an ask", format!("api = 1\n[[gate]]\nid = \"{id}\"\nverdict = \"ask\"\nrisk = 0\n")),
        ("the same id turned into an allow", format!("api = 1\n[[gate]]\nid = \"{id}\"\nverdict = \"allow\"\nwhy = \"ok\"\n")),
    ] {
        out.push((format!("{what}, in the operator's layer"), Some(user(&text)), None));
        out.push((format!("{what}, in a project"), None, Some(proj(&text))));
        out.push((format!("{what}, in both"), Some(user(&text)), Some(proj(&text))));
    }
    out.push(("nothing added".to_string(), None, None));
    out
}

#[test]
fn nothing_loosens_a_block_the_loosen_a_block_matrix() {
    let actors = [
        Actor::Operator,
        Actor::Core,
        Actor::Rule("r".into()),
        Actor::Doer("d".into()),
        Actor::Model("m".into()),
        Actor::Pack("p".into()),
    ];
    let (mut setups, mut checks) = (0, 0);
    for t in targets() {
        let modes: &[ReadOnly] =
            if t.ignores_mode { &[ReadOnly::No, ReadOnly::Explicit, ReadOnly::Inferred] } else { &[ReadOnly::No] };
        for (what, user_layer, project_layer) in &loosenings(t.id, t.verb) {
            let mut layers = (t.layers)();
            layers.extend(user_layer.clone());
            layers.extend(project_layer.clone());
            let (tables, problems) = Tables::load(layers);
            assert!(refused(&problems).is_empty(), "{what}: {problems:?}");
            let g = Gates::load(&tables).0.with_danger(fake);
            assert!(is_block(&run(&g, &t.action), t.id), "{}: {what}: {:?}", t.name, run(&g, &t.action));
            setups += 1;
            for actor in &actors {
                for ro in modes {
                    for a in std::iter::once(&t.action).chain(&t.spellings) {
                        let mut a = a.clone();
                        a.actor = actor.clone();
                        if t.ignores_mode {
                            a.read_only = *ro;
                        }
                        let v = check(&g, &Always::new("covers-all"), &a);
                        checks += 1;
                        assert!(
                            is_block(&v, t.id),
                            "{}: {what}, actor {actor:?}, {ro:?}, {:?} became {v:?}",
                            t.name,
                            a.target
                        );
                    }
                }
            }
        }
    }
    // each setup once: the trusted pass of a project is gone, it ran the same layers again
    assert!(setups > 150 && checks > 10_000, "{setups} setups, {checks} checks");
}

#[test]
fn a_session_row_cannot_loosen_a_block_either() {
    let (mut t, _) = Tables::load(vec![rules(SHELL), base()]);
    let session = |id: &str, body: &str| Write {
        to: Where::Session,
        file: None,
        kind: "gate".into(),
        id: id.into(),
        op: Op::Set(body.parse().unwrap()),
        by: Actor::Operator,
        cause: None,
        at_ms: 1,
    };
    // a floor of a rule a lower layer declared is refused where it is planned ...
    for (id, body) in [
        ("push-is-yours", "verdict = \"allow\""),
        ("push-is-yours", "when = \"false\""),
        ("push-is-yours", "on = [\"read\"]"),
        ("git-internals", "when = \"false\""),
        ("git-internals", "verdict = \"ask\""),
    ] {
        let err = t.plan(&session(id, body)).unwrap_err();
        assert!(err.msg.contains("floor"), "{id} {body}: {err}");
    }
    // ... and a rule of its own, however loose, is only an allow row
    let planned = t.plan(&session("let-me", "verdict = \"allow\"\nwhy = \"ok\"\nwhen = \"true\"\non = [\"shell\", \"write\", \"egress\"]")).unwrap();
    t.apply_session(&planned.change).unwrap();
    let g = Gates::load(&t).0.with_danger(fake);
    assert!(is_block(&run(&g, &sh("git push")), "push-is-yours"));
    assert!(is_block(&run(&g, &write_in(".git/config", true)), "git-internals"));
    assert!(is_block(&run(&g, &egress(Some(DataClass::Secret), "any")), "egress-secret"));
    assert!(is_block(&check(&g, &Always::new("g"), &sh("git push")), "push-is-yours"));
    assert_eq!(run(&g, &sh("make")), Verdict::Allow { by: "let-me".into() }, "the row is there and names itself");
}

#[test]
fn an_allow_row_never_overrides_an_ask_and_only_names_itself() {
    let allow_all = "api = 1\n[[gate]]\nid = \"let-it\"\non = [\"shell\", \"write\"]\nwhen = \"true\"\nverdict = \"allow\"\nwhy = \"ok\"\n";
    let g = gates(vec![rules(SHELL), base(), user(allow_all)]);
    assert_eq!(run(&g, &sh("rm -rf x")).word(), "ask");
    assert_eq!(run(&g, &act(Verb::Write, "x")).word(), "ask", "inside unknown still asks");
    assert_eq!(run(&g, &sh("make")), Verdict::Allow { by: "let-it".into() });
}

// ---- determinism and size ----

#[test]
fn the_same_rules_in_any_order_give_the_same_verdicts() {
    let rows: Vec<Value> = (0..40)
        .map(|n| {
            let verdict = ["allow", "ask", "block"][n % 3];
            row(&format!("rule-{:02}", (n * 7) % 40), &["shell"], Some(&format!("cmd matches '*{}*'", n % 5)), verdict)
        })
        .collect();
    let mut reversed = rows.clone();
    reversed.reverse();
    let mut rotated = rows.clone();
    rotated.rotate_left(13);
    let (a, b, c) = (Gates::from_rows(&rows).0, Gates::from_rows(&reversed).0, Gates::from_rows(&rotated).0);
    for target in ["0", "1 2", "x3x", "4", "01234", "none", "", "2 4"] {
        let action = sh(target);
        let want = run(&a, &action);
        assert_eq!(run(&b, &action), want, "{target}");
        assert_eq!(run(&c, &action), want, "{target}");
        assert_eq!(run(&a, &action), want, "same input, same output");
        assert_eq!(explain(&a, &NoGrants, &action), explain(&c, &NoGrants, &action));
    }
}

#[test]
fn ten_thousand_rules_are_checked_in_under_two_hundred_milliseconds() {
    let rows: Vec<Value> = (0..10_000)
        .map(|n| {
            let when = match n % 3 {
                0 => format!("cmd matches 'tool-{n}*'"),
                1 => format!("action.verb == 'shell' and danger(cmd).class == 'class-{n}'"),
                _ => format!("cmd matches '*{n}*' and not (action.read_only == 'explicit')"),
            };
            row(&format!("r{n:05}"), &["shell", "read"], Some(&when), ["allow", "ask", "block"][n % 3])
        })
        .collect();
    let (g, problems) = Gates::from_rows(&rows);
    assert!(problems.is_empty());
    assert_eq!(g.len(), 10_000);
    let g = g.with_danger(fake);
    let line = sh(&format!("tool-9998 {}", "x".repeat(200)));
    let mut v = None;
    crate::cpu::assert_within(std::time::Duration::from_millis(200), "ten thousand rules, one of them applies", || crate::cpu::least_of(3, || v = Some(run(&g, &line))));
    assert!(is_block(&v.expect("a verdict"), "r00008"));
    // the worst case: every rule applies
    let every: Vec<Value> =
        (0..10_000).map(|n| row(&format!("r{n:05}"), &["shell"], Some("danger(cmd).class == 'other'"), "ask")).collect();
    let g = Gates::from_rows(&every).0.with_danger(fake);
    let line = sh("make");
    let mut v = None;
    crate::cpu::assert_within(std::time::Duration::from_millis(200), "ten thousand rules, every one applies", || crate::cpu::least_of(3, || v = Some(run(&g, &line))));
    assert!(matches!(v, Some(Verdict::Ask { .. })));
    let mut explained = 0;
    crate::cpu::assert_within(std::time::Duration::from_millis(200), "the explanation of ten thousand rules", || crate::cpu::least_of(3, || explained = explain(&g, &NoGrants, &line).len()));
    assert_eq!(explained, 10_000);
}

#[test]
fn a_huge_line_does_not_make_the_check_slow() {
    let g = gates(vec![rules(SHELL), base()]);
    let wide = format!("ls {}", "a b ".repeat(50_000));
    crate::cpu::assert_within(std::time::Duration::from_millis(1_000), "a line of two hundred thousand bytes", || {
        crate::cpu::of(|| {
            for ro in [ReadOnly::No, ReadOnly::Explicit, ReadOnly::Inferred] {
                run(&g, &mode(sh(&wide), ro));
            }
        })
        .1
    });
}

// ---- the real matcher, once it is in the tree ----

fn real_matcher_is_here() -> bool {
    danger_value("git push origin main") != Value::Missing
}

#[test]
fn the_default_matcher_is_the_real_one() {
    let (t, _) = Tables::load(vec![rules(SHELL)]);
    let g = Gates::load(&t).0;
    if real_matcher_is_here() {
        assert!(is_block(&run(&g, &sh("git push origin main")), "push-is-yours"));
    } else {
        assert_eq!(run(&g, &sh("git push origin main")), allow_default(), "the stub matcher knows nothing");
    }
}

#[test]
fn the_real_matcher_stops_what_the_base_modes_say_it_should() {
    if !real_matcher_is_here() {
        return;
    }
    let (t, problems) = Tables::load(vec![base()]);
    assert!(problems.is_empty());
    let g = Gates::load(&t).0;
    let explicit = |c: &str| mode(sh(c), ReadOnly::Explicit);
    for line in ["touch x", "echo hi > out.txt", "rm -rf build", "git commit -m x", "cat a | tee b", "ls; rm x", "sed -i s/a/b/ f"] {
        assert!(is_block(&run(&g, &explicit(line)), "plan-no-shell-writes"), "`{line}`");
    }
    for line in ["ls -la", "cat Cargo.toml", "git status", "git log --oneline", "grep -rn x src", "cat x 2>/dev/null"] {
        assert_eq!(run(&g, &explicit(line)), allow_default(), "`{line}`");
    }
}

#[test]
fn a_read_only_step_lets_the_benchs_lines_through_and_still_stops_what_writes() {
    let (t, problems) = Tables::load(vec![base()]);
    assert!(problems.is_empty());
    let g = Gates::load(&t).0;
    let explicit = |c: &str| mode(sh(c), ReadOnly::Explicit);
    for line in BENCH_READS {
        assert_eq!(run(&g, &explicit(line)), allow_default(), "`{line}`");
    }
    for line in [
        "pip install x",
        "python3 -c \"open('x','w')\"",
        "python3 train.py",
        "python3 --version > v",
        "./tool --version",
        "curl https://x.test",
        "echo hi > /dev/tcp/10.0.0.1/80",
    ] {
        assert!(is_block(&run(&g, &explicit(line)), "plan-no-shell-writes"), "`{line}`");
    }
}
