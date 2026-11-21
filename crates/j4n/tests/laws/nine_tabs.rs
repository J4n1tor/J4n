//! A law: the top row of the screen holds nine tabs at most, so that `1`–`9` reach each. The pages of every pack the binary
//! carries, all on at once and `tabs.all` on, make the tabs and sub-tabs below; and a Table that makes more is
//! cut to nine and said, never sent whole. With the packs of J4n basic and no setting, five tabs stand: Chat, Usage, Agents, Log
//! and Settings, each a stats page, an interactive page or a form; the rest wait behind `tabs.all`, a pack or its switch.

use j4n_core::eval::Scope;
use j4n_core::front::page::{MAX_TABS, Pages};
use j4n_core::table::{Layer, LayerKind, Level, SourceFile, Tables};
use j4n_proto::Node;
use std::collections::BTreeMap;

fn ids(n: &Node) -> Vec<&str> {
    n.children.iter().map(|c| c.id.as_str()).collect()
}

/// The pages of the packs `ids` under the settings `set`, and what was said about them.
fn tree_of(ids: &[&str], set: serde_json::Value) -> (Node, Vec<String>) {
    let catalog = j4n::pack::catalog();
    let layers: Vec<Layer> = ids.iter().filter_map(|id| catalog.get(id)).map(|p| p.layer.clone()).collect();
    let (tables, _) = Tables::load(layers);
    let (pages, found) = Pages::load(&tables, &BTreeMap::new());
    let all = Scope::new().with("set", j4n_core::value::Value::from_json(&set));
    let tree = pages.tree("main", &all);
    (tree, found.into_iter().filter(|p| p.level == Level::Refused || p.msg.contains("tabs")).map(|p| p.to_string()).collect())
}

/// The pages of every built-in pack on at once, and what was said about them.
fn every_pack() -> (Node, Vec<String>) {
    let catalog = j4n::pack::catalog();
    // every page there is: the switches of the pages that wait for one are on, and `tabs.all`
    let on = serde_json::json!({ "habits": { "mode": "on" }, "ledger": { "mode": "on" }, "inbox": { "mode": "on" }, "kit": { "mode": "on" }, "lanes": "orchestrate", "crew": { "on": true, "envoys": true }, "tabs": { "all": true } });
    tree_of(&catalog.ids(), on)
}

fn tabs(root: &Node) -> &Node {
    root.children.iter().find(|c| c.id == "pages").expect("the tabs")
}

#[test]
fn the_pages_of_every_pack_make_nine_tabs_and_the_sub_tabs_of_the_guide() {
    let (root, said) = every_pack();
    assert!(said.is_empty(), "no page was left out and none was moved for being a tenth: {said:?}");
    let top = tabs(&root);
    assert!(top.children.len() <= MAX_TABS, "{:?}", ids(top));
    assert_eq!(ids(top), ["chat", "tasks", "spend", "connectors", "quality", "proposals", "log", "system"]);
    let titles: Vec<&str> = top.children.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, ["Chat", "Tasks", "Usage", "Agents", "Quality", "Grow", "Log", "Settings"], "the tabs with tabs.all; the Library's pages are sub-tabs of Agents");
    let sub = |id: &str| -> Vec<String> {
        let tab = top.children.iter().find(|c| c.id == id).unwrap();
        tab.children.iter().map(|c| c.id.clone()).collect()
    };
    assert_eq!(sub("tasks")[1..], ["points", "inbox", "habits", "catalogue", "seats", "links", "charters", "chatset", "opinions", "refusals"], "Tasks · Decisions · Inbox · Habits · Catalogue · Seats · Links · Charters · Chat settings · Opinions · Refusals (the first is what the tab drew)");
    assert_eq!(sub("connectors"), ["setup", "connect", "lab", "accounts", "account-add", "agents", "library", "providers", "plans", "trust", "tools", "services", "mcp", "deciders", "kit", "skills", "reflexes"], "Setup · Overview · Lab · Accounts · Add account · List · Models · Providers · Plans · Trust · Tools · Services · MCP · Deciders · Kit · Skills · Reflexes");
    assert_eq!(sub("spend"), ["usage-now", "spend-days", "spend-tasks", "report"], "one page: the usage view, the spend a day, the spend of each task, and the ledger's reports under them");
    assert_eq!(sub("quality"), ["quality-runs", "quality-marks", "quality-patterns", "quality-labels", "bench"]);
    assert_eq!(sub("proposals")[1..], ["contribute", "upstream-news", "upstream-todo"], "Proposals · Contribute · Updates · TODO");
    assert_eq!(sub("system"), ["features", "look", "settings", "config", "keys", "loop", "gates", "packs", "commands"], "Features · Look · This Session · All · Keys · Loop · Gates · Packs · Commands");
    // the titles the operator reads, sub-tab by sub-tab
    let shown = |id: &str| -> Vec<String> {
        let tab = top.children.iter().find(|c| c.id == id).unwrap();
        tab.children.iter().map(|c| c.title.clone()).collect()
    };
    assert_eq!(shown("connectors"), ["Setup", "Overview", "Lab", "Accounts", "Add account", "List", "Models", "Providers", "Plans", "Trust", "Tools", "Services", "MCP", "Deciders", "Kit", "Skills", "Reflexes"]);
    assert_eq!(shown("proposals"), ["Grow", "Contribute", "Updates", "TODO"]);
    assert_eq!(shown("system"), ["Features", "Look", "This Session", "All", "Keys", "Loop", "Gates", "Packs", "Commands"]);
}

/// J4n basic with no setting: the five tabs the operator works in, and in each only the sub-tabs that are a stats page, a page to act on
/// or a form. Tasks comes with the lanes that act on their own, the rest with `tabs.all`; a sub-tab marked `hide_empty` (Tools,
/// Services, MCP, Trust) is left out by the server while its list is empty (`front_serve`).
#[test]
fn j4n_basic_opens_on_five_tabs_and_tabs_all_brings_back_the_rest() {
    let (root, said) = tree_of(&j4n::pack::ON, serde_json::json!({}));
    assert!(said.is_empty(), "{said:?}");
    let top = tabs(&root);
    assert_eq!(ids(top), ["chat", "spend", "connectors", "log", "system"]);
    let titles: Vec<&str> = top.children.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, ["Chat", "Usage", "Agents", "Log", "Settings"]);
    let shown = |id: &str| -> Vec<String> { top.children.iter().find(|c| c.id == id).unwrap().children.iter().map(|c| c.title.clone()).collect() };
    assert_eq!(shown("connectors"), ["Setup", "Overview", "List", "Models", "Providers", "Plans", "Trust", "Tools", "Services", "MCP", "Deciders"], "Setup first; Lab waits for tabs.all");
    assert_eq!(shown("system"), ["Features", "All", "Keys", "Packs", "Commands"], "the short form first, then every switch, the keys, the packs and the commands");
    let usage = top.children.iter().find(|c| c.id == "spend").unwrap();
    assert_eq!(usage.children.first().and_then(|c| c.field.as_ref()).map(|f| f.bind.as_str()), Some("usage:all"), "the usage view stands at the top of Usage");
    // the lanes that act on their own bring Tasks; tabs.all brings every tab the packs have
    for word in ["auto", "wide", "orchestrate"] {
        let (root, _) = tree_of(&j4n::pack::ON, serde_json::json!({ "lanes": word }));
        assert!(ids(tabs(&root)).contains(&"tasks"), "{word}: {:?}", ids(tabs(&root)));
    }
    let (root, _) = tree_of(&j4n::pack::ON, serde_json::json!({ "lanes": "single" }));
    assert!(!ids(tabs(&root)).contains(&"tasks"));
    let (root, _) = tree_of(&j4n::pack::ON, serde_json::json!({ "tabs": { "all": true } }));
    assert_eq!(ids(tabs(&root)), ["chat", "tasks", "spend", "connectors", "proposals", "log", "system"]);
}

#[test]
fn a_table_with_ten_tabs_is_cut_to_nine_and_said() {
    let mut text = String::from("api = 1\n");
    for (i, id) in ["proposals", "a", "b", "c", "d", "e", "f", "g", "h", "i"].iter().enumerate() {
        text.push_str(&format!("[[page]]\nid = \"{id}\"\nparent = \"home\"\ntitle = \"{id}\"\norder = {}\nfield = \"list\"\nbind = \"tasks:all\"\n", i * 10 + 10));
    }
    let layer = Layer::new(LayerKind::Pack, "p", vec![SourceFile { path: "t.toml".into(), text }]);
    let (pages, found) = Pages::load(&Tables::load(vec![layer]).0, &BTreeMap::new());
    let root = pages.tree("main", &Scope::new());
    assert_eq!(tabs(&root).children.len(), MAX_TABS);
    assert!(found.iter().any(|p| p.at == "[[page]] i" && p.msg.contains("more than 9 tabs")), "{found:?}");
}

/// the names that were left behind are gone from every text the operator reads. The models are sub-tabs of `Agents`
/// (their tab was once called `Accounts`, then `Models`, then `Library`; `Agents` was `Connect`), so no pack file or source
/// line names the old places (`Accounts ›`, `Connect › Connect`, `Tasks › Asks`, `Grow › Releases`, `the Releases tab`); no crumb of a
/// renamed tab repeats its tab's name; and the current titles are there.
#[test]
fn no_text_the_operator_reads_names_the_old_places() {
    use crate::support::{read, rel, root, walk};
    // `Connect ›` and `Library ›`: the tab Connect is Agents and the Library's pages are its sub-tabs; `Grow › Packs` is Settings › Packs; Spend is Usage
    let old = ["Accounts ›", "Accounts tab", "opens Accounts", "open Accounts", "Connect › ", "Connect tab", "Library ›", "Library tab", "Grow › Packs", "Spend tab", "Spend page", "Tasks › Asks", "Grow › Releases", "Grow › Notes", "the Releases tab", "Settings › Chat", "Models › Library", "Models › Providers", "Models › Plans"];
    let mut found = Vec::new();
    for path in walk(&root()) {
        let file = rel(&path);
        let ours = (file.starts_with("packs/") && file.ends_with(".toml")) || (file.starts_with("crates/") && file.contains("/src/") && file.ends_with(".rs"));
        if !ours {
            continue;
        }
        for (n, line) in read(&path).lines().enumerate() {
            if let Some(w) = old.iter().find(|w| line.contains(**w)) {
                found.push(format!("{file}:{}: `{w}`", n + 1));
            }
        }
    }
    assert!(found.is_empty(), "the names the tabs left behind are in a text: {found:#?}");
    let (root, _) = every_pack();
    let crumbs: Vec<(String, String)> = tabs(&root).children.iter().flat_map(|t| t.children.iter().map(move |c| (t.title.clone(), c.title.clone()))).collect();
    for tab in ["Agents", "Settings"] {
        assert!(!crumbs.iter().any(|(t, c)| t == tab && c == tab), "the tab {tab} has no sub-tab of the same name: {crumbs:?}");
    }
    for crumb in [("Agents", "Models"), ("Agents", "Setup"), ("Agents", "Overview"), ("Agents", "Add account"), ("Settings", "This Session"), ("Settings", "Packs"), ("Tasks", "Decisions"), ("Grow", "Updates"), ("Grow", "TODO")] {
        assert!(crumbs.contains(&(crumb.0.into(), crumb.1.into())), "{crumb:?} is not in {crumbs:?}");
    }
}
