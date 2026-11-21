//! Templates and pages as data. The screen is the template `main`; an operator's file with the same
//! id changes it field by field; a pack that adds a page under `home` adds a tab; a template can use another and a page can hold parts.

use j4n_core::eval::Scope;
use j4n_core::front::page::Pages;
use j4n_core::table::{Layer, LayerKind, Level, Problem, SourceFile, Tables};
use j4n_core::value::Value;
use j4n_proto::{Field, FieldKind, Json, Node, Present, json};
use std::collections::BTreeMap;

const BASE_PAGES: &str = include_str!("../../../../packs/base/pages/main.toml");
const BASE_MAIN: &str = include_str!("../../../../packs/base/templates/main.toml");

fn layer(kind: LayerKind, name: &str, files: &[(&str, &str)]) -> Layer {
    Layer::new(kind, name, files.iter().map(|(p, t)| SourceFile { path: (*p).into(), text: (*t).into() }).collect())
}

/// The pages and templates of the layers, and what was said about them (the Table's own warnings left out).
fn load(layers: Vec<Layer>) -> (Pages, Vec<Problem>) {
    let (t, mut found) = Tables::load(layers);
    let (pages, more) = Pages::load(&t, &BTreeMap::new());
    found.extend(more);
    found.retain(|p| p.level == Level::Refused);
    (pages, found)
}

fn one(text: &str) -> (Pages, Vec<Problem>) {
    load(vec![layer(LayerKind::Pack, "p", &[("t.toml", text)])])
}

fn tree(p: &Pages) -> Node {
    p.tree("t1", &Scope::new())
}

fn ids(n: &Node) -> Vec<&str> {
    n.children.iter().map(|c| c.id.as_str()).collect()
}

fn find<'a>(n: &'a Node, id: &str) -> Option<&'a Node> {
    if n.id == id { Some(n) } else { n.children.iter().find_map(|c| find(c, id)) }
}

fn said(found: &[Problem], at: &str, word: &str) -> bool {
    found.iter().any(|p| p.at == at && p.msg.contains(word))
}

fn base() -> Layer {
    layer(LayerKind::Pack, "base", &[("pages/main.toml", BASE_PAGES), ("templates/main.toml", BASE_MAIN)])
}

#[test]
fn the_base_screen_is_the_template_main_the_tabs_then_the_questions_then_the_bar() {
    let (pages, found) = load(vec![base()]);
    assert!(found.is_empty(), "{found:?}");
    let root = tree(&pages);
    assert_eq!((root.id.as_str(), root.present, ids(&root)), ("root", Present::Stack, vec!["pages", "asks", "bar"]));
    let (tabs, asks, bar) = (&root.children[0], &root.children[1], &root.children[2]);
    assert_eq!((tabs.present, tabs.grow, tabs.children.is_empty()), (Present::Tabs, 1, false));
    assert_eq!(asks.field, Some(Field { kind: FieldKind::Ask, bind: "asks:open".into(), props: Json::Null }));
    assert_eq!(asks.title, "Questions");
    assert_eq!(bar.field, Some(Field { kind: FieldKind::Bar, bind: "bar:status".into(), props: Json::Null }));
}

#[test]
fn a_file_of_the_operator_with_the_id_main_changes_the_children_and_leaves_the_rest_as_it_was() {
    let mine = "api = 1\n[[template]]\nid = \"main\"\nchildren = [{ id = \"bar\", field = \"bar\", bind = \"bar:status\" }, { id = \"pages\", tabs = \"home\", grow = 1 }]\n";
    let (pages, found) = load(vec![base(), layer(LayerKind::User, "home", &[("ui/templates.toml", mine)])]);
    assert!(found.is_empty(), "{found:?}");
    let root = tree(&pages);
    assert_eq!(ids(&root), ["bar", "pages"], "the bar stands over the tabs and the questions are gone");
    assert_eq!(root.present, Present::Stack, "what the file did not say is as it was");
    // and a file that says only `present` leaves the children
    let split = "api = 1\n[[template]]\nid = \"main\"\npresent = \"split\"\nratio = 30\n";
    let (pages, found) = load(vec![base(), layer(LayerKind::User, "home", &[("ui/templates.toml", split)])]);
    assert!(found.is_empty(), "{found:?}");
    let root = tree(&pages);
    assert_eq!((root.present, root.ratio, ids(&root)), (Present::Split, 30, vec!["pages", "asks", "bar"]));
}

#[test]
fn a_project_changes_nothing_of_what_the_operator_laid_out() {
    let mine = "api = 1\n[[template]]\nid = \"main\"\nchildren = [{ id = \"bar\", field = \"bar\", bind = \"bar:status\" }]\n";
    let theirs = "api = 1\n[[template]]\nid = \"main\"\nchildren = [{ id = \"pages\", tabs = \"home\" }]\n";
    let ls = vec![base(), layer(LayerKind::User, "home", &[("ui/t.toml", mine)]), layer(LayerKind::Project, "here", &[("ui/t.toml", theirs)])];
    let (t, _) = Tables::load(ls);
    assert_eq!(ids(&tree(&Pages::load(&t, &BTreeMap::new()).0)), ["bar"], "a stranger's repository does not lay out the screen");
}

#[test]
fn a_pack_that_adds_a_page_under_home_adds_a_tab_to_the_first_part_of_main() {
    let pack = "api = 1\n[[page]]\nid = \"shelf\"\nparent = \"home\"\ntitle = \"Shelf\"\norder = 900\nicon = \"worktree\"\nfield = \"log\"\nbind = \"log:all\"\n";
    let (pages, found) = load(vec![base(), layer(LayerKind::Pack, "widgets", &[("pages/p.toml", pack)])]);
    assert!(found.is_empty(), "{found:?}");
    let root = tree(&pages);
    let tabs = find(&root, "pages").unwrap();
    assert_eq!(tabs.children.last().map(|c| (c.id.as_str(), c.icon.as_str(), c.title.as_str())), Some(("shelf", "worktree", "Shelf")));
    assert_eq!(ids(&root), ["pages", "asks", "bar"]);
}

#[test]
fn a_page_under_main_stands_after_the_parts_of_the_template() {
    let extra = "api = 1\n[[page]]\nid = \"hint\"\nparent = \"main\"\ntitle = \"\"\norder = 5\nfield = \"bar\"\nbind = \"bar:hint\"\n";
    let (pages, found) = load(vec![base(), layer(LayerKind::Pack, "x", &[("pages/x.toml", extra)])]);
    assert!(found.is_empty(), "{found:?}");
    assert_eq!(ids(&tree(&pages)), ["pages", "asks", "bar", "hint"]);
}

#[test]
fn a_part_carries_its_size_its_props_and_its_title_and_takes_an_id_that_no_node_has() {
    let text = r#"api = 1
[[template]]
id = "main"
children = [
  { field = "log", bind = "log:all", grow = 3, title = "Log", props = { follow = true, tail = 5 } },
  { field = "log", bind = "log:all", height = 4 },
  { field = "bar", bind = "bar:status", id = "chat" },
]
[[page]]
id = "chat"
parent = "home"
title = "Chat"
field = "chat"
bind = "chat:{thread}"
"#;
    let (pages, found) = one(text);
    assert!(found.is_empty(), "{found:?}");
    let root = tree(&pages);
    assert_eq!(ids(&root), ["main.log", "main.log-2", "chat-2"], "the second log and the bar that asked for the id of a page are given others");
    let first = &root.children[0];
    assert_eq!((first.grow, first.height, first.title.as_str()), (3, 0, "Log"));
    assert_eq!(first.field.as_ref().unwrap().props, json!({"follow": true, "tail": 5}));
    assert_eq!((root.children[1].grow, root.children[1].height), (0, 4));
}

#[test]
fn a_template_uses_another_in_place_and_the_chat_of_the_thread_is_the_thread_it_was_made_for() {
    let text = r#"api = 1
[[template]]
id = "main"
children = [{ use = "side", grow = 2 }, { field = "chat", bind = "chat:{thread}" }]
[[template]]
id = "side"
present = "split"
ratio = 40
children = [{ field = "log", bind = "log:all" }, { field = "list", bind = "tasks:all" }]
"#;
    let (pages, found) = one(text);
    assert!(found.is_empty(), "{found:?}");
    let root = tree(&pages);
    assert_eq!(ids(&root), ["main.side", "main.chat"]);
    let side = &root.children[0];
    assert_eq!((side.present, side.ratio, side.grow, ids(side)), (Present::Split, 40, 2, vec!["side.log", "side.list"]));
    assert_eq!(root.children[1].field.as_ref().unwrap().bind, "chat:t1");
    let mut binds = pages.binds("t1");
    binds.sort();
    assert_eq!(binds, ["chat:t1", "log:all", "tasks:all"]);
}

#[test]
fn a_template_that_extends_another_starts_from_it_and_one_that_extends_its_own_id_starts_from_the_layer_below() {
    let text = r#"api = 1
[[template]]
id = "plain"
children = [{ field = "log", bind = "log:all" }]
[[template]]
id = "wide"
extends = "plain"
present = "split"
ratio = 60
[[template]]
id = "tall"
extends = "wide"
children = [{ field = "bar", bind = "bar:status" }]
[[template]]
id = "same"
children = [{ field = "list", bind = "tasks:all" }]
[[template]]
id = "main"
children = [{ use = "wide" }, { use = "tall" }, { use = "same" }]
"#;
    let mine = "api = 1\n[[template]]\nid = \"same\"\nextends = \"same\"\npresent = \"grid\"\ncols = 2\n";
    let (pages, found) = load(vec![layer(LayerKind::Pack, "p", &[("t.toml", text)]), layer(LayerKind::User, "home", &[("ui/t.toml", mine)])]);
    assert!(found.is_empty(), "{found:?}");
    let t = tree(&pages);
    let [wide, tall, same] = &t.children[..] else { panic!("{t:?}") };
    assert_eq!((wide.present, wide.ratio, ids(wide)), (Present::Split, 60, vec!["wide.log"]), "it has the children of `plain` and says its own layout");
    assert_eq!((tall.present, tall.ratio, ids(tall)), (Present::Split, 60, vec!["tall.bar"]), "its own children, the layout of the one before");
    assert_eq!((same.present, same.cols, ids(same)), (Present::Grid, 2, vec!["same.list"]), "the layer below gave the children");
}

#[test]
fn a_template_that_cannot_stand_is_left_out_and_said_and_a_part_that_cannot_stand_goes_alone() {
    let text = r#"api = 1
[[template]]
id = "a"
extends = "b"
[[template]]
id = "b"
extends = "a"
[[template]]
id = "c"
extends = "nowhere"
[[template]]
id = "main"
children = [
  { field = "log", bind = "log:all" },
  { field = "log" },
  { field = "log", bind = "log:all", view = "v" },
  { field = "log", bind = "log:all", use = "a" },
  { use = "a", bind = "log:all" },
  { tabs = "home", props = { x = 1 } },
  { id = "Not An Id", field = "log", bind = "log:all" },
  { use = "gone" },
  { tabs = "nowhere" },
  { title = "nothing" },
]
"#;
    let (pages, found) = one(text);
    for (at, word) in [
        ("[[template]] a", "which extends it"),
        ("[[template]] b", "which extends it"),
        ("[[template]] c", "no such template"),
        ("[[template]] main part 2", "`bind` or a `view`"),
        ("[[template]] main part 3", "`bind` or a `view`"),
        ("[[template]] main part 4", "one of the three"),
        ("[[template]] main part 5", "draws nothing of its own"),
        ("[[template]] main part 6", "draws nothing of its own"),
        ("[[template]] main part 7", "not an id"),
        ("[[template]] main part 8", "there is none"),
        ("[[template]] main part 9", "not a page"),
        ("[[template]] main part 10", "one of the three"),
    ] {
        assert!(said(&found, at, word), "{at}: {word}: {found:?}");
    }
    assert_eq!(ids(&tree(&pages)), ["main.log"], "the first part stands");
}

#[test]
fn templates_that_use_each_other_are_cut_where_they_would_loop_and_the_rest_stands() {
    let text = r#"api = 1
[[template]]
id = "main"
children = [{ use = "x" }, { field = "log", bind = "log:all" }]
[[template]]
id = "x"
children = [{ use = "y" }, { field = "bar", bind = "bar:status" }]
[[template]]
id = "y"
children = [{ use = "x" }, { use = "y" }, { field = "list", bind = "tasks:all" }]
"#;
    let (pages, found) = one(text);
    for at in ["[[template]] x part 1", "[[template]] y part 1", "[[template]] y part 2"] {
        assert!(said(&found, at, "which uses this template"), "{at}: {found:?}");
    }
    assert_eq!(found.len(), 3, "every use that lies on a loop is cut: {found:?}");
    let root = tree(&pages);
    let x = &root.children[0];
    assert_eq!((ids(&root), ids(x)), (vec!["main.x", "main.log"], vec!["x.bar"]));
}

#[test]
fn a_part_with_a_when_is_there_only_while_the_setting_holds() {
    let text = "api = 1\n[[template]]\nid = \"main\"\nchildren = [{ field = \"log\", bind = \"log:all\", when = \"set.show.log\" }, { field = \"bar\", bind = \"bar:status\" }]\n";
    let (pages, found) = one(text);
    assert!(found.is_empty(), "{found:?}");
    let map = |on| Value::Map(BTreeMap::from([("show".to_string(), Value::Map(BTreeMap::from([("log".to_string(), Value::Bool(on))])))]));
    assert_eq!(ids(&pages.tree("t", &Scope::new().with("set", map(false)))), ["main.bar"]);
    assert_eq!(ids(&pages.tree("t", &Scope::new().with("set", map(true)))), ["main.log", "main.bar"]);
}

#[test]
fn a_part_that_draws_a_view_nobody_has_is_left_out_and_said() {
    let text = "api = 1\n[[template]]\nid = \"main\"\nchildren = [{ field = \"list\", view = \"nope\" }, { field = \"list\", view = \"yes\" }]\n[[page]]\nid = \"g\"\nparent = \"home\"\ntitle = \"G\"\nchildren = [{ field = \"list\", view = \"nope\" }]\n";
    let (mut pages, found) = one(text);
    assert!(found.is_empty(), "{found:?}");
    let found = pages.lint(&|v| v == "yes");
    assert!(said(&found, "[[template]] main part 1", "no such view") && said(&found, "[[page]] g part 1", "no such view"), "{found:?}");
    assert_eq!(pages.binds("t"), ["view:yes"]);
    assert_eq!(ids(&tree(&pages)), ["main.list"], "the tabs of `home` come from a template that holds them: this one holds none");
}

#[test]
fn a_page_holds_parts_of_its_own_and_says_how_they_share_its_room() {
    let text = r#"api = 1
[[page]]
id = "connectors"
parent = "home"
title = "Connectors"
icon = "connector"
order = 30
present = "tabs"
[[page]]
id = "agents"
parent = "connectors"
title = "agents"
present = "split"
ratio = 40
children = [
  { id = "list", field = "list", bind = "view:agents", props = { group = "state" } },
  { id = "card", field = "card", bind = "table:connector" },
]
[[page]]
id = "grid"
parent = "connectors"
title = "grid"
present = "grid"
cols = 3
order = 5
props = { x = 1 }
"#;
    let (pages, found) = one(text);
    assert!(said(&found, "[[page]] grid", "no `bind`, `view` or `props`"), "a group has no props: {found:?}");
    let text = text.replace("props = { x = 1 }\n", "");
    let (pages2, found) = one(&text);
    assert!(found.is_empty(), "{found:?}");
    let _ = pages;
    let root = pages2.tree("t", &Scope::new());
    let agents = find(&root, "agents").unwrap();
    assert_eq!((agents.present, agents.ratio, ids(agents)), (Present::Split, 40, vec!["list", "card"]));
    assert_eq!(agents.children[0].field.as_ref().unwrap().props, json!({"group": "state"}));
    assert_eq!(agents.children[1].field.as_ref().unwrap().kind, FieldKind::Card);
    let grid = find(&root, "grid").unwrap();
    assert_eq!((grid.present, grid.cols), (Present::Grid, 3));
    assert_eq!(find(&root, "connectors").map(|c| (c.icon.as_str(), ids(c))), Some(("connector", vec!["grid", "agents"])));
}

#[test]
fn a_leaf_with_children_and_a_field_that_is_not_one_are_refused_and_every_kind_of_field_can_be_drawn() {
    let text = r#"api = 1
[[page]]
id = "both"
parent = "home"
title = "Both"
field = "log"
bind = "log:all"
children = [{ field = "log", bind = "log:all" }]
[[page]]
id = "stats"
parent = "home"
title = "Stats"
field = "stat"
bind = "view:stats"
"#;
    let (pages, found) = one(text);
    assert!(said(&found, "[[page]] both", "a leaf has no `children`"), "{found:?}");
    let t = tree(&pages);
    assert_eq!(find(&t, "stats").unwrap().field.as_ref().map(|f| f.kind), Some(FieldKind::Stat));
}

#[test]
fn a_template_that_extends_another_has_its_cols_where_it_says_none_and_a_page_may_present_its_parts_as_subtabs() {
    let text = r#"api = 1
[[template]]
id = "base"
present = "grid"
cols = 3
children = [{ tabs = "home" }, { field = "log", bind = "log:all" }]
[[template]]
id = "main"
extends = "base"
cols = 2
children = [{ tabs = "home" }, { use = "sub" }]
[[template]]
id = "sub"
extends = "base"
[[page]]
id = "g"
parent = "home"
title = "G"
present = "subtabs"
[[page]]
id = "k"
parent = "g"
title = "K"
field = "log"
bind = "log:all"
"#;
    let (pages, found) = one(text);
    assert!(found.is_empty(), "{found:?}");
    let root = tree(&pages);
    assert_eq!((root.present, root.cols), (Present::Grid, 2), "its own cols, and the grid of the one it extends");
    assert_eq!(find(&root, "main.sub").map(|t| (t.present, t.cols)), Some((Present::Grid, 3)), "and the cols of the one it extends where it says none");
    assert_eq!(find(&root, "g").map(|g| g.present), Some(Present::Subtabs));
}

#[test]
fn a_part_that_uses_a_template_or_holds_tabs_has_no_props_of_its_own() {
    let text = r#"api = 1
[[template]]
id = "side"
children = [{ field = "log", bind = "log:all" }]
[[template]]
id = "main"
children = [{ use = "side", props = { x = 1 } }, { tabs = "home", props = { x = 1 } }, { use = "side" }]
"#;
    let (pages, found) = one(text);
    assert!(said(&found, "[[template]] main part 1", "draws nothing of its own") && said(&found, "[[template]] main part 2", "draws nothing of its own"), "{found:?}");
    assert_eq!(ids(&tree(&pages)), ["main.side"], "the third part stands");
}

// ---- at most nine tabs ----

fn page(id: &str, parent: &str, order: i64, extra: &str) -> String {
    format!("[[page]]\nid = \"{id}\"\nparent = \"{parent}\"\ntitle = \"{id}\"\norder = {order}\nfield = \"list\"\nbind = \"tasks:all\"\n{extra}")
}

fn problems(layers: Vec<Layer>) -> (Pages, Vec<Problem>) {
    let (t, _) = Tables::load(layers);
    Pages::load(&t, &BTreeMap::new())
}

#[test]
fn a_page_that_names_a_group_is_a_sub_tab_of_that_tab_and_a_leaf_it_joins_keeps_what_it_drew_as_the_first() {
    let text = format!("api = 1\n{}{}{}", page("tasks", "home", 20, ""), page("points", "home", 30, "group = \"tasks\"\n"), page("log", "home", 40, ""));
    let (pages, found) = one(&text);
    assert!(found.is_empty(), "{found:?}");
    let root = tree(&pages);
    let tabs = &root.children[0];
    assert_eq!(ids(tabs), ["tasks", "log"], "the joined page is not a tab");
    let tasks = find(&root, "tasks").unwrap();
    assert_eq!((tasks.present, tasks.field.is_none(), ids(tasks)), (Present::Tabs, true, vec!["tasks.own", "points"]));
    assert_eq!(tasks.children[0].title, "tasks");
    assert_eq!(tasks.children[0].field.as_ref().map(|f| f.bind.as_str()), Some("tasks:all"), "the first sub-tab draws what the tab drew");
    let alone = format!("api = 1\n{}{}", page("tasks", "home", 20, ""), page("points", "home", 30, "group = \"tasks\"\nwhen = \"false\"\n"));
    let (pages, _) = one(&alone);
    let tasks = find(&tree(&pages), "tasks").cloned().unwrap();
    assert!(tasks.field.is_some() && tasks.children.is_empty(), "no page joined it now: it stays a leaf, as it was");
}

#[test]
fn a_group_that_is_no_tab_is_a_tab_of_its_own_and_a_group_on_a_page_not_under_home_is_said() {
    let text = format!("api = 1\n{}{}{}", page("a", "home", 10, ""), page("b", "home", 20, "group = \"nowhere\"\n"), page("c", "a", 30, "group = \"a\"\n"));
    let (t, _) = Tables::load(vec![layer(LayerKind::Pack, "p", &[("t.toml", &text)])]);
    let (pages, found) = Pages::load(&t, &BTreeMap::new());
    assert!(said(&found, "[[page]] c", "only a page under `home`") && !found.iter().any(|p| p.at == "[[page]] b"), "the tab a pack's page names may be of a pack that is off: {found:?}");
    assert_eq!(ids(&tree(&pages).children[0]), ["a", "b"], "b names a tab that is not there: it is a tab");
}

#[test]
fn a_tenth_tab_joins_grow_and_is_said_and_nine_stay() {
    let mut text = String::from("api = 1\n");
    text.push_str(&page("proposals", "home", 55, ""));
    for i in 1..=10 {
        text.push_str(&page(&format!("t{i:02}"), "home", i * 5, ""));
    }
    let (pages, found) = problems(vec![layer(LayerKind::Pack, "p", &[("t.toml", &text)])]);
    let tabs = ids(&tree(&pages).children[0]).iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(tabs.len(), 9, "{tabs:?}");
    assert!(tabs.contains(&"proposals".to_string()), "Grow is one of the nine");
    assert_eq!(&tabs[..3], ["t01", "t02", "t03"], "the first by order stay");
    let grow = find(&tree(&pages), "proposals").cloned().unwrap();
    assert_eq!(ids(&grow), ["proposals.own", "t09", "t10"]);
    assert!(said(&found, "[[page]] t10", "more than 9 tabs") && said(&found, "[[page]] t09", "joins `proposals`"), "{found:?}");
}

#[test]
fn the_base_template_ends_with_the_keys_row_while_look_keys_row_holds_and_the_setting_is_on_by_default() {
    let pages = Pages::builtin();
    let map = |on| Value::Map(BTreeMap::from([("look".to_string(), Value::Map(BTreeMap::from([("keys_row".to_string(), Value::Bool(on))])))]));
    let last = |on| {
        let root = pages.tree("t", &Scope::new().with("set", map(on)));
        ids(&root).last().map(|s| s.to_string())
    };
    assert_eq!(last(true).as_deref(), Some("keysrow"), "the keys row is the last part, under the status bar");
    assert_eq!(last(false).as_deref(), Some("bar"), "off gives the row back to the page");
    let screen = j4n_core::front::screen::Screen::builtin();
    assert_eq!(screen.set.pointer("/look/keys_row"), Value::Bool(true), "on by default");
    assert!(pages.binds("t").contains(&"keys:row".to_string()));
}

// ---- charts ----

#[test]
fn a_chart_over_a_leaf_is_checked_against_what_the_screen_draws_and_the_base_pages_carry_the_charts() {
    let page = |series: &str, join: bool| format!("api = 1\n[[page]]\nid = \"g\"\nparent = \"home\"\ntitle = \"G\"\nfield = \"list\"\nbind = \"tasks:all\"\nchart = {{ id = \"c\", field = \"line\", bind = \"chart:spend\", props = {{ series = [{series}] }} }}\n{}", if join { "[[page]]\nid = \"h\"\nparent = \"home\"\ngroup = \"g\"\ntitle = \"H\"\nfield = \"list\"\nbind = \"tasks:all\"\n" } else { "" });
    let names = |n: usize| (0..n).map(|i| format!("{{ name = \"s{i}\" }}")).collect::<Vec<_>>().join(", ");
    let kinds = |n: &Node| n.children.iter().map(|c| c.field.as_ref().map(|f| f.kind)).collect::<Vec<_>>();
    let (pages, found) = one(&page(&names(2), false));
    assert!(found.is_empty(), "{found:?}");
    let g = find(&tree(&pages), "g").unwrap().clone();
    assert_eq!((g.present, g.field.is_none(), kinds(&g)), (Present::Stack, true, vec![Some(FieldKind::Line), Some(FieldKind::List)]), "the chart over the list");
    assert_eq!(g.children[1].grow, 1);
    let (pages, found) = one(&page(&names(2), true));
    assert!(found.is_empty(), "{found:?}");
    let g = find(&tree(&pages), "g").unwrap().clone();
    assert_eq!(ids(&g)[..2], ["g.own", "h"], "the first sub-tab is what the page drew, with its chart: {:?}", ids(&g));
    assert_eq!(kinds(&g.children[0]), vec![Some(FieldKind::Line), Some(FieldKind::List)]);
    let (pages, found) = one(&page(&names(5), false));
    assert!(said(&found, "[[page]] g chart", "one to 4 series, not 5"), "{found:?}");
    assert_eq!(find(&tree(&pages), "g").and_then(|n| n.field.as_ref()).map(|f| f.kind), Some(FieldKind::List), "the leaf stands as it was");
    let (_, found) = one("api = 1\n[[page]]\nid = \"g\"\nparent = \"home\"\ntitle = \"G\"\nchart = { field = \"list\", bind = \"x\" }\n");
    assert!(said(&found, "[[page]] g", "a chart stands over a leaf"), "{found:?}");
    let (pages, found) = load(vec![base()]);
    assert!(found.is_empty(), "{found:?}");
    // with `tabs.all` on: Grow and Tasks wait for it out of the box
    let root = pages.tree("t1", &Scope::new().with("set", Value::from_json(&json!({ "tabs": { "all": true } }))));
    for (id, kind) in [("spend-days", FieldKind::Line), ("models-wall", FieldKind::Line), ("grow-bench", FieldKind::Line)] {
        assert_eq!(find(&root, id).and_then(|n| n.field.as_ref()).map(|f| f.kind), Some(kind), "{id}");
    }
    // the bars of the Tasks page stand only while the lanes are not `single`, which a tree with no settings does not say
    assert!(BASE_PAGES.contains("chart = { id = \"tasks-lanes\"") && find(&root, "tasks.own").and_then(|n| n.field.as_ref()).map(|f| f.kind) == Some(FieldKind::List));
}

#[test]
fn a_secret_field_on_a_forms_draft_and_a_key_bind_drawn_by_another_field_are_refused_at_load() {
    let leaf = |id: &str, field: &str, bind: &str| format!("[[page]]\nid = \"{id}\"\nparent = \"home\"\ntitle = \"{id}\"\norder = 5\nfield = \"{field}\"\nbind = \"{bind}\"\n");
    let text = format!(
        "api = 1\n{}{}{}[[page]]\nid = \"form\"\nparent = \"home\"\ntitle = \"form\"\norder = 9\nchildren = [{{ field = \"secret\", bind = \"draft:account.pin\" }}, {{ field = \"str\", bind = \"key:own.value\" }}, {{ field = \"secret\", bind = \"key:gw.value\" }}]\n",
        leaf("drafted", "secret", "draft:account.token"),
        leaf("plain", "str", "key:own.value"),
        leaf("masked", "secret", "key:own.value"),
    );
    let (pages, found) = problems(vec![layer(LayerKind::Pack, "p", &[("t.toml", &text)])]);
    assert!(said(&found, "[[page]] drafted", "never a form's draft"), "{found:?}");
    assert!(said(&found, "[[page]] plain", "the `secret` field only"), "{found:?}");
    assert!(said(&found, "[[page]] form part 1", "never a form's draft") && said(&found, "[[page]] form part 2", "the `secret` field only"), "{found:?}");
    assert!(!found.iter().any(|p| p.at == "[[page]] masked" || p.at == "[[page]] form part 3"), "a key on the secret field stands: {found:?}");
    assert!(find(&tree(&pages), "masked").is_some() && find(&tree(&pages), "drafted").is_none());
}
