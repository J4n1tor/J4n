//! The sweep leftovers: what a view prints and how a view makes rows under a row.

use j4n_core::query::Coll;
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use j4n_core::value::Value;
use j4n_core::view::{Sources, Views, outer};
use std::collections::BTreeMap;

struct Fake(Vec<Value>);

impl Sources for Fake {
    fn rows(&self, _coll: &Coll) -> Vec<Value> {
        self.0.clone()
    }
}

fn views(toml: &str) -> Views {
    let files = vec![SourceFile { path: "views/t.toml".into(), text: toml.into() }];
    let (t, found) = Tables::load(vec![Layer::new(LayerKind::Pack, "t", files)]);
    assert!(found.is_empty(), "{found:?}");
    let (views, found) = Views::load(&t);
    assert!(found.is_empty(), "{found:?}");
    views
}

fn map(fields: Vec<(&str, Value)>) -> Value {
    Value::Map(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect::<BTreeMap<_, _>>())
}

fn pair(depth: u32, text: &str) -> Value {
    Value::List(vec![Value::num(f64::from(depth)), Value::text(text)])
}

#[test]
fn a_view_makes_rows_under_a_row_from_a_list_of_depth_and_text() {
    const V: &str = "api = 1\n[[view]]\nid = \"v\"\nfrom = { from = \"tasks\" }\nkey = \"{rid}\"\ncells = [\"{head}\"]\nchildren = \"{tree}\"\n";
    let tree = Value::List(vec![pair(0, "head"), pair(1, "facts"), pair(1, "line #1"), pair(2, "t1 answer"), pair(2, "t2 answer"), pair(1, "needs you")]);
    let report = map(vec![("rid", Value::text("r1")), ("head", Value::text("head")), ("tree", tree)]);
    let rows = views(V).get("v").unwrap().rows(&Fake(vec![report]), &outer(Value::Map(BTreeMap::new())));
    let got: Vec<(&str, Option<&str>, &str)> = rows.iter().map(|r| (r.key.as_str(), r.parent.as_deref(), r.cells[0].text.as_str())).collect();
    assert_eq!(
        got,
        [("r1", None, "head"), ("r1/1", Some("r1"), "facts"), ("r1/2", Some("r1"), "line #1"), ("r1/3", Some("r1/2"), "t1 answer"), ("r1/4", Some("r1/2"), "t2 answer"), ("r1/5", Some("r1"), "needs you")]
    );
}

#[test]
fn children_is_one_hole_and_a_row_with_none_is_alone() {
    let bad = "api = 1\n[[view]]\nid = \"v\"\nfrom = { from = \"tasks\" }\ncells = [\"{head}\"]\nchildren = \"tree {tree}\"\n";
    let files = vec![SourceFile { path: "views/t.toml".into(), text: bad.into() }];
    let (t, _) = Tables::load(vec![Layer::new(LayerKind::Pack, "t", files)]);
    let (_, found) = Views::load(&t);
    assert!(found.iter().any(|p| format!("{p:?}").contains("one hole")), "{found:?}");
    const V: &str = "api = 1\n[[view]]\nid = \"v\"\nfrom = { from = \"tasks\" }\nkey = \"{rid}\"\ncells = [\"{head}\"]\nchildren = \"{tree}\"\n";
    let report = map(vec![("rid", Value::text("r1")), ("head", Value::text("head"))]);
    let rows = views(V).get("v").unwrap().rows(&Fake(vec![report]), &outer(Value::Map(BTreeMap::new())));
    assert_eq!(rows.len(), 1);
}

#[test]
fn the_report_page_is_a_tree_of_the_ledger_view_whose_rows_hang_from_the_report() {
    let pack = views(include_str!("../../../../packs/ledger/views/ledger.toml"));
    let page = include_str!("../../../../packs/ledger/pages/ledger.toml");
    assert!(page.contains("field = \"tree\"") && page.contains("view = \"ledger\""), "{page}");
    let tree = Value::List(vec![pair(0, "◆ ledger · main · 1 tasks"), pair(1, "± 0 files"), pair(1, "❯ #1 “go” → t1 ✓"), pair(2, "t1  done")]);
    let report = map(vec![("rid", Value::text("r17")), ("head", Value::text("◆ ledger · main · 1 tasks")), ("tree", tree)]);
    let rows = pack.get("ledger").unwrap().rows(&Fake(vec![report]), &outer(Value::Map(BTreeMap::new())));
    let hung: Vec<(&str, Option<&str>)> = rows.iter().map(|r| (r.cells[0].text.as_str(), r.parent.as_deref())).collect();
    assert_eq!(hung, [("◆ ledger · main · 1 tasks", None), ("± 0 files", Some("r17")), ("❯ #1 “go” → t1 ✓", Some("r17")), ("t1  done", Some("r17/2"))]);
    assert_eq!(rows[0].cells[1].text, "r17");
}

fn said(v: Value, how: &str) -> String {
    let text = format!("api = 1\n[[view]]\nid = \"v\"\nfrom = {{ from = \"tasks\" }}\ncells = [\"{{x:{how}}}\"]\n");
    let rows = views(&text).get("v").unwrap().rows(&Fake(vec![map(vec![("x", v)])]), &outer(Value::Map(BTreeMap::new())));
    rows[0].cells[0].text.clone()
}

fn words(list: &[&str]) -> Value {
    Value::List(list.iter().map(|w| Value::text(*w)).collect())
}

#[test]
fn a_run_of_keys_is_its_two_ends() {
    let nine = words(&["1", "2", "3", "4", "5", "6", "7", "8", "9"]);
    assert_eq!(said(nine, "runs"), "1–9");
    assert_eq!(said(words(&["a", "b", "c", "x", "y", "enter"]), "runs"), "a–c, x, y, enter", "three follow each other; two do not make a run");
    assert_eq!(said(words(&["ctrl+a", "ctrl+b", "ctrl+c"]), "runs"), "ctrl+a, ctrl+b, ctrl+c", "a key of more than one character is no run");
    assert_eq!(said(words(&["1"]), "runs"), "1");
    assert_eq!(said(Value::List(Vec::new()), "runs"), "—");
}

#[test]
fn what_stands_when_no_one_answers_is_in_words() {
    for (rung, want) in [("pick:add", "picks add"), ("rank", "the ranking"), ("safe", "the safe answer"), ("text:No one answered; use your own judgment.", "No one answered; use your own judgment."), ("odd", "odd")] {
        assert_eq!(said(Value::text(rung), "fallback"), want, "{rung}");
    }
}

#[test]
fn the_keys_and_asks_pages_use_them() {
    let f1 = include_str!("../../../../packs/base/views/f1.toml");
    assert!(f1.contains("{native:runs}") && f1.contains("{fallback:fallback}"), "the Keys page says a run of keys by its ends and the Asks page its last column in words");
}

#[test]
fn a_view_says_which_rows_start_shut_and_the_report_page_opens_only_the_newest() {
    use j4n_proto::Fold;
    let pack = views(include_str!("../../../../packs/ledger/views/ledger.toml"));
    let report = |rid: &str| map(vec![("rid", Value::text(rid)), ("head", Value::text("head")), ("tree", Value::List(vec![pair(0, "head"), pair(1, "facts")]))]);
    let rows = pack.get("ledger").unwrap().rows(&Fake(vec![report("r3"), report("r2"), report("r1")]), &outer(Value::Map(BTreeMap::new())));
    let folds: Vec<(&str, Option<Fold>)> = rows.iter().map(|r| (r.key.as_str(), r.fold)).collect();
    assert_eq!(folds, [("r3", None), ("r3/1", None), ("r2", Some(Fold::Folded)), ("r2/1", None), ("r1", Some(Fold::Folded)), ("r1/1", None)], "the newest is open, every older report starts shut");
    const V: &str = "api = 1\n[[view]]\nid = \"v\"\nfrom = { from = \"tasks\" }\nkey = \"{n}\"\ncells = [\"{n}\"]\nfolded = \"n > 1 and place > 0\"\n";
    let rows = views(V).get("v").unwrap().rows(&Fake(vec![map(vec![("n", Value::num(5.0))]), map(vec![("n", Value::num(1.0))]), map(vec![("n", Value::num(7.0))])]), &outer(Value::Map(BTreeMap::new())));
    assert_eq!(rows.iter().map(|r| r.fold).collect::<Vec<_>>(), [None, None, Some(Fold::Folded)], "`folded` reads the row's fields and its place");
    let bad = V.replace("n > 1 and place > 0", "n >");
    let files = vec![SourceFile { path: "views/t.toml".into(), text: bad }];
    let (t, found) = Tables::load(vec![Layer::new(LayerKind::Pack, "t", files)]);
    let found = format!("{found:?}{:?}", Views::load(&t).1);
    assert!(found.contains("folded"), "an expression that does not read is refused: {found}");
}
