//! A law: the top row of the screen holds nine tabs at most, so that `1`–`9` reach each. The pages of every pack the binary
//! carries, all on at once, make exactly the nine tabs, with their sub-tabs; and a Table that makes more is
//! cut to nine and said, never sent whole.

use j4n_core::eval::Scope;
use j4n_core::front::page::{MAX_TABS, Pages};
use j4n_core::table::{Layer, LayerKind, Level, SourceFile, Tables};
use j4n_proto::Node;
use std::collections::BTreeMap;

fn ids(n: &Node) -> Vec<&str> {
    n.children.iter().map(|c| c.id.as_str()).collect()
}

/// The pages of every built-in pack on at once, and what was said about them.
fn every_pack() -> (Node, Vec<String>) {
    let catalog = j4n::pack::catalog();
    let layers: Vec<Layer> = catalog.ids().iter().filter_map(|id| catalog.get(id)).map(|p| p.layer.clone()).collect();
    let (tables, _) = Tables::load(layers);
    let (pages, found) = Pages::load(&tables, &BTreeMap::new());
    // every page there is: the switches of the pages that wait for one are on
    let on = serde_json::json!({ "habits": { "mode": "on" }, "ledger": { "mode": "on" }, "inbox": { "mode": "on" }, "kit": { "mode": "on" }, "lanes": "orchestrate", "crew": { "on": true, "envoys": true } });
    let all = Scope::new().with("set", j4n_core::value::Value::from_json(&on));
    let tree = pages.tree("main", &all);
    (tree, found.into_iter().filter(|p| p.level == Level::Refused || p.msg.contains("tabs")).map(|p| p.to_string()).collect())
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
    assert_eq!(ids(top), ["chat", "tasks", "models", "connectors", "spend", "quality", "proposals", "log", "system"]);
    let titles: Vec<&str> = top.children.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, ["Chat", "Tasks", "Library", "Connect", "Spend", "Quality", "Grow", "Log", "Settings"], "the nine tabs; the third is Library");
    let sub = |id: &str| -> Vec<String> {
        let tab = top.children.iter().find(|c| c.id == id).unwrap();
        tab.children.iter().map(|c| c.id.clone()).collect()
    };
    assert_eq!(sub("tasks")[1..], ["points", "inbox", "habits", "catalogue", "seats", "links", "charters", "chatset", "opinions", "refusals"], "Tasks · Decisions · Inbox · Habits · Catalogue · Seats · Links · Charters · Chat settings · Opinions · Refusals (the first is what the tab drew)");
    assert_eq!(sub("models"), ["library", "providers", "plans", "trust"], "Models · Providers · Plans · Trust");
    assert_eq!(sub("connectors"), ["setup", "connect", "lab", "accounts", "account-add", "agents", "tools", "services", "mcp", "deciders", "kit", "skills", "reflexes"], "Setup · Overview · Lab · Accounts · Add account · Agents · Tools · Services · MCP · Deciders · Kit · Skills · Reflexes");
    assert_eq!(sub("spend")[1..], ["report"]);
    assert_eq!(sub("quality"), ["quality-runs", "quality-marks", "quality-patterns", "quality-labels", "bench"]);
    assert_eq!(sub("proposals")[1..], ["contribute", "packs", "upstream-news", "upstream-todo"], "Proposals · Contribute · Packs · Updates · TODO");
    assert_eq!(sub("system"), ["features", "look", "settings", "keys", "loop", "gates", "config", "commands"], "Features · Look · This Session · Keys · Loop · Gates · All · Commands");
    // the titles the operator reads, sub-tab by sub-tab
    let shown = |id: &str| -> Vec<String> {
        let tab = top.children.iter().find(|c| c.id == id).unwrap();
        tab.children.iter().map(|c| c.title.clone()).collect()
    };
    assert_eq!(shown("models"), ["Models", "Providers", "Plans", "Trust"]);
    assert_eq!(shown("connectors"), ["Setup", "Overview", "Lab", "Accounts", "Add account", "Agents", "Tools", "Services", "MCP", "Deciders", "Kit", "Skills", "Reflexes"]);
    assert_eq!(shown("proposals"), ["Grow", "Contribute", "Packs", "Updates", "TODO"]);
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

/// the names that were left behind are gone from every text the operator reads. The third tab is `Library`
/// (the tab was once called `Accounts`, then `Models`), so no pack file or source
/// line names the old places (`Accounts ›`, `Connect › Connect`, `Tasks › Asks`, `Grow › Releases`, `the Releases tab`); no crumb of a
/// renamed tab repeats its tab's name; and the current titles are there.
#[test]
fn no_text_the_operator_reads_names_the_old_places() {
    use crate::support::{read, rel, root, walk};
    let old = ["Accounts ›", "Accounts tab", "opens Accounts", "open Accounts", "Connect › Connect", "Tasks › Asks", "Grow › Releases", "Grow › Notes", "the Releases tab", "Settings › Chat", "Models › Library", "Models › Providers", "Models › Plans"];
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
    for tab in ["Library", "Connect", "Settings"] {
        assert!(!crumbs.iter().any(|(t, c)| t == tab && c == tab), "the tab {tab} has no sub-tab of the same name: {crumbs:?}");
    }
    for crumb in [("Library", "Models"), ("Connect", "Setup"), ("Connect", "Overview"), ("Connect", "Add account"), ("Settings", "This Session"), ("Tasks", "Decisions"), ("Grow", "Updates"), ("Grow", "TODO")] {
        assert!(crumbs.contains(&(crumb.0.into(), crumb.1.into())), "{crumb:?} is not in {crumbs:?}");
    }
}
