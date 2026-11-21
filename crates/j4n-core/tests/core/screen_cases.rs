//! The screen from data: pages that make a tree, views that make rows, commands that make words and a
//! palette. A pack adds all three with files only.

use j4n_core::eval::Scope;
use j4n_core::front::command::{Commands, Target};
use j4n_core::front::page::Pages;
use j4n_core::front::phrase::Phrases;
use j4n_core::front::screen::Screen;
use j4n_core::query::Coll;
use j4n_core::setting::Settings;
use j4n_core::table::{Layer, LayerKind, Level, SourceFile, Tables};
use j4n_core::value::Value;
use j4n_core::view::{Sources, Views, outer};
use j4n_proto::{Field, FieldKind, Intent, Json, Node, Present, Tone};
use std::collections::BTreeMap;

fn tables(files: &[(&str, &str)]) -> Tables {
    let files = files.iter().map(|(p, t)| SourceFile { path: (*p).into(), text: (*t).into() }).collect();
    let (t, problems) = Tables::load(vec![Layer::new(LayerKind::Pack, "p", files)]);
    // a reference to a page, view or flow that this one pack does not have is the Table's warning; a refusal would be a mistake of the test
    assert!(problems.iter().all(|p| p.level == Level::Warn), "{problems:?}");
    t
}

fn map(pairs: &[(&str, Value)]) -> Value {
    Value::Map(pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect())
}

fn ids(n: &Node) -> Vec<&str> {
    n.children.iter().map(|c| c.id.as_str()).collect()
}

fn find<'a>(n: &'a Node, id: &str) -> Option<&'a Node> {
    if n.id == id { Some(n) } else { n.children.iter().find_map(|c| find(c, id)) }
}

const PAGES: &str = r#"api = 1
[[page]]
id = "chat"
parent = "home"
title = "Chat"
order = 20
field = "chat"
bind = "chat:{thread}"
[[page]]
id = "early"
parent = "home"
title = "Early"
order = 10
field = "log"
bind = "log:all"
[[page]]
id = "models"
parent = "home"
title = "Models"
order = 30
present = "tabs"
[[page]]
id = "trust"
parent = "models"
title = "Trust"
order = 5
field = "list"
view = "tv"
help = "how far"
[[page]]
id = "bar"
parent = "main"
title = ""
field = "bar"
bind = "bar:status"
"#;

#[test]
fn pages_make_a_tree_tabs_by_order_then_id_a_group_with_children_and_parts_under_the_tabs() {
    let (pages, found) = Pages::load(&tables(&[("pages/p.toml", PAGES)]), &BTreeMap::new());
    assert!(found.is_empty(), "{found:?}");
    let tree = pages.tree("main", &Scope::new());
    assert_eq!(ids(&tree), ["pages", "bar"]);
    let tabs = &tree.children[0];
    assert_eq!((tabs.present, ids(tabs)), (Present::Tabs, vec!["early", "chat", "models"]));
    let chat = find(&tree, "chat").unwrap();
    assert_eq!(chat.field, Some(Field { kind: FieldKind::Chat, bind: "chat:main".into(), props: Json::Null }), "the thread fills the binding");
    let models = find(&tree, "models").unwrap();
    assert_eq!((models.present, models.field.clone(), ids(models)), (Present::Tabs, None, vec!["trust"]));
    let trust = find(&tree, "trust").unwrap();
    assert_eq!((trust.field.clone().map(|f| f.bind), trust.help.as_str()), (Some("view:tv".to_string()), "how far"), "a leaf that draws a view is bound to it");
    let mut binds = pages.binds("t9");
    binds.sort();
    assert_eq!(binds, ["bar:status", "chat:t9", "log:all", "view:tv"]);
}

#[test]
fn a_page_that_cannot_stand_is_left_out_and_said_and_the_rest_stands() {
    let bad = r#"api = 1
[[page]]
id = "ok"
parent = "home"
title = "Ok"
field = "log"
bind = "log:all"
[[page]]
id = "both"
parent = "home"
title = "Both"
field = "log"
bind = "log:all"
view = "v"
[[page]]
id = "neither"
parent = "home"
title = "Neither"
field = "log"
[[page]]
id = "group-with-bind"
parent = "home"
title = "G"
bind = "log:all"
[[page]]
id = "orphan"
parent = "nowhere"
title = "O"
field = "log"
bind = "log:all"
[[page]]
id = "a"
parent = "b"
title = "A"
present = "tabs"
[[page]]
id = "b"
parent = "a"
title = "B"
present = "tabs"
"#;
    let (pages, found) = Pages::load(&tables(&[("pages/p.toml", bad)]), &BTreeMap::new());
    // `both` has a `bind` and a `view`, which the Table passes and the pages refuse
    let at: Vec<&str> = found.iter().map(|p| p.at.as_str()).collect();
    for id in ["both", "neither", "group-with-bind", "orphan", "a", "b"] {
        assert!(at.contains(&format!("[[page]] {id}").as_str()), "{id}: {found:?}");
    }
    assert!(found.iter().all(|p| p.level == Level::Refused));
    assert_eq!(ids(&pages.tree("t", &Scope::new()).children[0]), ["ok"]);
}

#[test]
fn a_page_with_a_when_is_there_only_while_the_setting_holds_and_its_children_go_with_it() {
    let text = r#"api = 1
[[page]]
id = "g"
parent = "home"
title = "G"
present = "tabs"
when = "set.show.it"
[[page]]
id = "k"
parent = "g"
title = "K"
field = "log"
bind = "log:all"
[[page]]
id = "always"
parent = "home"
title = "Always"
field = "log"
bind = "log:all"
"#;
    let (pages, found) = Pages::load(&tables(&[("pages/p.toml", text)]), &BTreeMap::new());
    assert!(found.is_empty(), "{found:?}");
    let off = Scope::new().with("set", map(&[("show", map(&[("it", Value::Bool(false))]))]));
    let on = Scope::new().with("set", map(&[("show", map(&[("it", Value::Bool(true))]))]));
    assert_eq!(ids(&pages.tree("t", &off).children[0]), ["always"]);
    assert_eq!(ids(&pages.tree("t", &on).children[0]), ["always", "g"], "a tie in order goes by id");
    assert_eq!(ids(find(&pages.tree("t", &on), "g").unwrap()), ["k"]);
}

#[test]
fn a_page_of_a_pack_that_is_not_settled_carries_its_tag() {
    let (pages, _) = Pages::load(&tables(&[("pages/p.toml", PAGES)]), &BTreeMap::from([("p".to_string(), "experimental".to_string())]));
    assert_eq!(find(&pages.tree("t", &Scope::new()), "chat").unwrap().tag, "experimental");
    let (pages, _) = Pages::load(&tables(&[("pages/p.toml", PAGES)]), &BTreeMap::new());
    assert_eq!(find(&pages.tree("t", &Scope::new()), "chat").unwrap().tag, "");
}

#[test]
fn a_page_that_draws_a_view_nobody_has_goes_with_the_pages_under_it() {
    let text = r#"api = 1
[[page]]
id = "g"
parent = "home"
title = "G"
present = "tabs"
[[page]]
id = "lost"
parent = "g"
title = "Lost"
field = "list"
view = "nope"
[[page]]
id = "deeper"
parent = "lost"
title = "D"
field = "log"
bind = "log:all"
[[page]]
id = "kept"
parent = "g"
title = "Kept"
field = "list"
view = "yes"
"#;
    let t = tables(&[("pages/p.toml", text)]);
    let (mut pages, found) = Pages::load(&t, &BTreeMap::new());
    assert!(found.is_empty(), "{found:?}");
    let said = pages.lint(&|v| v == "yes");
    assert_eq!(said.len(), 1);
    assert!(said[0].msg.contains("no such view"), "{said:?}");
    assert!(pages.has("kept") && !pages.has("lost") && !pages.has("deeper"));
}

// ---- views ----

struct Fake(BTreeMap<String, Vec<Value>>);

impl Sources for Fake {
    fn rows(&self, coll: &Coll) -> Vec<Value> {
        self.0.get(&coll.to_string()).cloned().unwrap_or_default()
    }
}

const VIEWS: &str = r#"api = 1
[[view]]
id = "open"
from = { from = "tasks", filter = "state != 'done'", order = "-n", limit = 2 }
cells = ["{id}", "{state}", "{goal:short}", "{spent:usd}"]
tone = "tone"
detail = ["{why}"]
acts = [{ key = "s", label = "stop", run = ":stop {id}", risk = 7 }]
[[view]]
id = "keyed"
from = { from = "tasks" }
key = "k-{n}"
cells = ["{n}"]
[[view]]
id = "gated"
from = { from = "tasks" }
cells = ["{id}"]
when = "set.flag"
"#;

fn tasks() -> Fake {
    let row = |id: &str, state: &str, n: f64, tone: &str, why: &str| map(&[("id", Value::text(id)), ("state", Value::text(state)), ("n", Value::num(n)), ("goal", Value::text("write the thing")), ("spent", Value::usd(0.5)), ("tone", Value::text(tone)), ("why", Value::text(why))]);
    Fake(BTreeMap::from([("tasks".to_string(), vec![row("t1", "done", 1.0, "good", ""), row("t2", "running", 2.0, "warn", "slow"), row("t3", "waiting", 3.0, "bad", ""), row("t4", "running", 4.0, "nonsense", "")])]))
}

#[test]
fn a_view_makes_rows_filtered_ordered_limited_with_cells_a_tone_detail_and_keys_that_send_a_line() {
    let (views, found) = Views::load(&tables(&[("views/p.toml", VIEWS)]));
    assert!(found.is_empty(), "{found:?}");
    let v = views.get("open").unwrap();
    assert_eq!(v.from(), &Coll::Tasks);
    let rows = v.rows(&tasks(), &outer(map(&[])));
    assert_eq!(rows.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), ["t4", "t3"], "newest first, the done one filtered out, at most two");
    assert_eq!(rows[0].cells.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["t4", "running", "write the thing", "$0.50"]);
    assert_eq!((rows[0].tone, rows[1].tone), (Tone::Plain, Tone::Bad), "a word that is no tone reads plain");
    assert!(rows[0].detail.is_empty(), "a line with nothing in it is not shown: {:?}", rows[0].detail);
    let t2 = Views::load(&tables(&[("views/p.toml", &VIEWS.replace("limit = 2", "limit = 3"))])).0.get("open").unwrap().rows(&tasks(), &outer(map(&[])));
    assert_eq!(t2[2].detail.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["slow"], "only what the row says");
    let act = &rows[0].actions[0];
    assert_eq!((act.key.as_str(), act.label.as_str(), act.risk), ("s", "stop", 7));
    assert_eq!(act.intent, Intent::Say { text: ":stop t4".into(), with: Vec::new() }, "the key sends the line as the operator would type it");
}

#[test]
fn a_view_shows_a_yes_or_no_as_on_or_off_and_a_chain_in_words() {
    // `theme.motion true` beside `on` / `off`, and `rule:gate, human` on the Asks page
    let text = "api = 1\n[[view]]\nid = \"flags\"\nfrom = { from = \"tasks\" }\ncells = [\"{id}\", \"{flag}\", \"{chain:rungs}\"]\ndetail = [\"default {flag}\"]\n";
    let (views, found) = Views::load(&tables(&[("views/p.toml", text)]));
    assert!(found.is_empty(), "{found:?}");
    let src = Fake(BTreeMap::from([("tasks".to_string(), vec![map(&[("id", Value::text("a")), ("flag", Value::Bool(true)), ("chain", Value::List(vec![Value::text("rule:gate"), Value::text("human")]))]), map(&[("id", Value::text("b")), ("flag", Value::Bool(false)), ("chain", Value::List(vec![Value::text("human")]))])])]));
    let rows = views.get("flags").unwrap().rows(&src, &outer(map(&[])));
    let cells = |i: usize| rows[i].cells.iter().map(|c| c.text.as_str()).collect::<Vec<_>>();
    assert_eq!((cells(0), cells(1)), (vec!["a", "on", "a rule, then you"], vec!["b", "off", "you"]));
    assert_eq!(rows[1].detail.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["default off"]);
}

#[test]
fn a_row_is_named_by_its_key_template_else_its_id_else_its_place_so_a_selection_survives_new_rows() {
    let (views, _) = Views::load(&tables(&[("views/p.toml", VIEWS)]));
    let keyed = views.get("keyed").unwrap().rows(&tasks(), &outer(map(&[])));
    assert_eq!(keyed.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), ["k-1", "k-2", "k-3", "k-4"]);
    let src = Fake(BTreeMap::from([("tasks".to_string(), vec![map(&[("x", Value::num(1.0))]), map(&[("key", Value::text("named"))])])]));
    let plain = Views::load(&tables(&[("views/p.toml", "api = 1\n[[view]]\nid = \"v\"\nfrom = { from = \"tasks\" }\ncells = [\"{x}\"]\n")])).0;
    assert_eq!(plain.get("v").unwrap().rows(&src, &outer(map(&[]))).iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), ["0", "named"]);
}

#[test]
fn a_view_with_a_when_has_no_rows_while_it_does_not_hold() {
    let (views, _) = Views::load(&tables(&[("views/p.toml", VIEWS)]));
    let g = views.get("gated").unwrap();
    assert!(g.rows(&tasks(), &outer(map(&[("flag", Value::Bool(false))]))).is_empty());
    assert!(!g.shown(&outer(map(&[]))));
    assert_eq!(g.rows(&tasks(), &outer(map(&[("flag", Value::Bool(true))]))).len(), 4);
}

#[test]
fn a_view_that_does_not_read_is_left_out_and_said_with_the_reason() {
    let bad = r#"api = 1
[[view]]
id = "nowhere"
from = { from = "databases" }
cells = ["{id}"]
[[view]]
id = "hole"
from = { from = "tasks" }
cells = ["{id"]
[[view]]
id = "order"
from = { from = "tasks", order = "" , limit = 0 }
cells = ["{id}"]
[[view]]
id = "fine"
from = { from = "tasks" }
cells = ["{id}"]
"#;
    // the Table refuses a view whose query or templates do not read, so the views never see it
    let files = vec![SourceFile { path: "views/p.toml".into(), text: bad.into() }];
    let (t, found) = Tables::load(vec![Layer::new(LayerKind::Pack, "p", files)]);
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(found.iter().all(|p| p.level == Level::Refused && p.at.contains("[[view]]")));
    let (views, more) = Views::load(&t);
    assert!(more.is_empty());
    assert_eq!(views.ids(), ["fine"]);
}

// ---- commands ----

const COMMANDS: &str = r#"api = 1
[[command]]
id = "go"
doc = "Start the go flow."
flow = "go"
words = ["/go", ":go"]
args = ["what"]
[[command]]
id = "board"
doc = "Show the board."
open = "board"
[[command]]
id = "pack"
core = "pack"
[[command]]
id = "none"
[[command]]
id = "two"
core = "help"
open = "board"
"#;

#[test]
fn a_command_gives_a_line_its_words_by_default_a_colon_and_its_id_and_runs_a_core_command_a_flow_or_opens_a_page() {
    let t = tables(&[("commands/p.toml", COMMANDS)]);
    let (commands, found) = Commands::load(&t, &BTreeMap::new());
    assert_eq!(found.iter().map(|p| p.at.as_str()).collect::<Vec<_>>(), ["[[command]] none", "[[command]] two"], "one that does not do exactly one thing");
    assert_eq!(commands.all().iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["board", "go", "pack"]);
    let mut phrases = Phrases::default();
    assert!(commands.claim(&mut phrases).is_empty());
    let cmd = |line: &str| phrases.parse(line).map(|c| (c.name, c.args));
    assert_eq!(cmd(":board"), Some(("open".into(), vec!["board".into()])));
    assert_eq!(cmd("/go the thing"), Some(("flow".into(), vec!["go".into(), "the".into(), "thing".into()])));
    assert_eq!(cmd(":pack"), Some(("pack".into(), vec![])));
    assert_eq!(phrases.flows(), ["go"], "a page is not a flow");
    assert_eq!(phrases.lead("board"), Some(":board".into()), "from a shell the word comes without its lead");
    assert_eq!(phrases.lead("go"), Some(":go".into()));
    assert_eq!(phrases.lead("nothing"), None);
}

#[test]
fn a_word_that_another_command_has_or_that_does_not_open_with_a_colon_is_refused_and_said() {
    let text = r#"api = 1
[[command]]
id = "a"
core = "help"
words = [":same"]
[[command]]
id = "b"
core = "status"
words = [":same", "plain", ":has space"]
[[command]]
id = "c"
open = "x"
words = ["/slash"]
"#;
    let (commands, _) = Commands::load(&tables(&[("commands/p.toml", text)]), &BTreeMap::new());
    let found = commands.claim(&mut Phrases::default());
    assert_eq!(found.len(), 4, "{found:?}");
    assert!(found.iter().any(|p| p.msg.contains("already a word")));
    assert!(found.iter().any(|p| p.at == "[[command]] c" && p.msg.contains("colon")), "only a flow's words may open with a slash");
}

#[test]
fn the_palette_lists_each_command_with_its_words_what_follows_and_a_key_only_when_it_needs_no_more_words() {
    let tags = BTreeMap::from([("p".to_string(), "experimental".to_string())]);
    let (commands, _) = Commands::load(&tables(&[("commands/p.toml", COMMANDS)]), &tags);
    let rows = commands.rows();
    let go = rows.iter().find(|r| r.key == "go").unwrap();
    assert_eq!(go.cells.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["/go :go", "<what>", "Start the go flow.", "experimental"]);
    assert!(go.actions.is_empty(), "it takes a goal: the key would run it with none");
    let board = rows.iter().find(|r| r.key == "board").unwrap();
    assert_eq!(board.actions[0].intent, Intent::Say { text: ":board".into(), with: Vec::new() });
    assert_eq!(commands.all().iter().find(|c| c.id == "board").unwrap().target, Target::Page("board".into()));
}

// ---- the screen ----

#[test]
fn the_base_screen_has_the_tabs_the_trust_page_under_accounts_and_the_commands_that_open_pages() {
    let s = Screen::builtin();
    // out of the box five tabs: Chat, Usage (`spend`), Agents (`connectors`, which holds the Library's pages too), Log, Settings (`system`)
    let tree = s.pages.tree("main", &outer(s.set.clone()));
    assert_eq!(ids(&tree.children[0]), ["chat", "spend", "connectors", "log", "system"], "the five tabs");
    assert_eq!(ids(find(&tree, "connectors").unwrap()), ["setup", "agents", "library", "providers", "plans", "trust", "tools", "services", "mcp", "deciders"]);
    assert_eq!(ids(find(&tree, "system").unwrap()), ["features", "config", "keys", "packs", "commands"]);
    // `tabs.all` brings back Tasks, Grow (`proposals`) and the Settings pages for a look now and then
    let tree = s.pages.tree("main", &outer(Value::from_json(&serde_json::json!({ "tabs": { "all": true } }))));
    assert_eq!(ids(&tree.children[0]), ["chat", "tasks", "spend", "connectors", "proposals", "log", "system"], "the tree of the tabs with `tabs.all`");
    assert_eq!(ids(find(&tree, "tasks").unwrap()), ["tasks.own", "points"]);
    assert_eq!(ids(find(&tree, "proposals").unwrap()), ["proposals.own", "upstream-news", "upstream-todo"]);
    assert_eq!(ids(find(&tree, "system").unwrap()), ["features", "look", "settings", "config", "keys", "loop", "gates", "packs", "commands"]);
    assert!(s.views.get("trust").is_some() && s.views.get("settings").is_some());
    let words: Vec<&str> = s.commands.all().iter().flat_map(|c| c.words.iter().map(String::as_str)).collect();
    assert_eq!(words, [":archive", "/archive", ":calibrate", ":chart", "/chart", ":chats", ":clear", "/clear", ":compact", "/compact", ":config", ":settings", ":context", "/context", ":grant", ":grants", ":history", ":key", ":lanes", ":library", ":models", ":look", ":new", ":pack", ":packs", ":plan", ":plans", ":proposals", ":providers", ":resume", ":roster", ":setup", ":doctor", ":signin", ":term", ":top", ":trust", ":unarchive", ":usage", "/usage", ":watch"]);
}

#[test]
fn a_command_that_opens_a_page_that_is_not_there_is_said() {
    let t = tables(&[("commands/p.toml", "api = 1\n[[command]]\nid = \"lost\"\nopen = \"nowhere\"\n")]);
    let (settings, _) = Settings::load(&t);
    let (screen, found) = Screen::load(t, &settings, Vec::new(), &BTreeMap::new());
    assert!(found.iter().any(|p| p.level == Level::Refused && p.msg.contains("no such page")), "{found:?}");
    assert!(screen.commands.all().is_empty(), "a command that opens nothing is not offered");
}

#[test]
fn the_trust_page_shows_each_answerer_at_each_point_with_its_state_its_labels_and_how_wrong_they_were() {
    let s = Screen::builtin();
    let row = |id: &str, point: &str, state: &str, tone: &str, n: f64, wrong: f64, why: &str| {
        map(&[("id", Value::text(id)), ("point", Value::text(point)), ("state", Value::text(state)), ("tone", Value::text(tone)), ("model", Value::text("m1")), ("n", Value::num(n)), ("wrong", Value::num(wrong)), ("why", Value::text(why))])
    };
    let src = Fake(BTreeMap::from([("fold:answerers".to_string(), vec![row("decider:jev", "intake", "active", "good", 24.0, 0.125, ""), row("decider:jev", "approval", "shadow", "warn", 20.0, 0.4, "40% wrong over 20 labels")])]));
    let rows = s.views.get("trust").unwrap().rows(&src, &outer(s.set.clone()));
    assert_eq!(rows.iter().map(|r| r.key.as_str()).collect::<Vec<_>>().len(), 2);
    let by_point = |p: &str| rows.iter().find(|r| r.cells[1].text == p).unwrap();
    assert_eq!(by_point("intake").cells.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["decider:jev", "intake", "active", "m1", "24 labelled", "13% wrong"]);
    assert_eq!((by_point("intake").tone, by_point("approval").tone), (Tone::Good, Tone::Warn));
    assert!(by_point("intake").detail.is_empty(), "no reason, no line");
    assert_eq!(by_point("approval").detail[0].text, "40% wrong over 20 labels");
}

#[test]
fn the_settings_page_lists_every_setting_with_what_it_holds_and_what_it_is_for() {
    let s = Screen::builtin();
    let t = tables(&[("settings/s.toml", "api = 1\n[[setting]]\nid = \"trust.max_wrong\"\ndoc = \"The share of answers that may be wrong.\"\nty = \"num\"\ndefault = 0.25\nvalue = 0.5\n\n[[setting]]\nid = \"trust.min_x\"\ndoc = \"x\"\nty = \"int\"\ndefault = 7\n")]);
    let (settings, _) = Settings::load(&t);
    let (held, _) = Screen::load(t, &settings, Vec::new(), &BTreeMap::new());
    let rows = s.views.get("settings").unwrap().rows(&Fake(BTreeMap::from([("table:setting".to_string(), held.setting_rows())])), &outer(s.set.clone()));
    let min = rows.iter().find(|r| r.key == "trust.min_x").expect("a setting nobody said a value of");
    assert_eq!(min.cells[1].text, "7", "what stands is its default: {:?}", min.cells);
    let max = rows.iter().find(|r| r.key == "trust.max_wrong").expect("a setting is a row, named by its id");
    assert_eq!(max.cells[0].text, "trust.max_wrong");
    assert_eq!((max.cells[1].text.as_str(), max.cells[2].text.as_str()), ("0.5", "The share of answers that may be wrong."), "{:?}", max.cells);
    assert!(max.detail.iter().any(|c| c.text.starts_with("default 0.25")), "{:?}", max.detail);
}

#[test]
fn a_field_with_props_travels_with_them_and_one_without_leaves_them_out() {
    let with = Field { kind: FieldKind::Int, bind: "k:i.f".into(), props: serde_json::json!({ "min": 1, "max": 8 }) };
    let wire = serde_json::to_value(&with).expect("a field is JSON");
    assert_eq!(wire, serde_json::json!({ "kind": "int", "bind": "k:i.f", "props": { "min": 1, "max": 8 } }));
    assert_eq!(serde_json::from_value::<Field>(wire).expect("and back"), with);
    let plain = Field { kind: FieldKind::Chat, bind: "chat:main".into(), props: Json::Null };
    assert_eq!(serde_json::to_value(&plain).expect("JSON"), serde_json::json!({ "kind": "chat", "bind": "chat:main" }), "no props, no key");
    let old: Field = serde_json::from_str(r#"{"kind":"list","bind":"tasks:all"}"#).expect("a field from before props");
    assert_eq!(old.props, Json::Null);
    for (word, kind) in [("str", FieldKind::Str), ("text", FieldKind::Text), ("int", FieldKind::Int), ("num", FieldKind::Num), ("bool", FieldKind::Bool), ("enum", FieldKind::Enum), ("multi", FieldKind::Multi), ("ref", FieldKind::Ref), ("seq", FieldKind::Seq), ("map", FieldKind::Map), ("dur", FieldKind::Dur), ("usd", FieldKind::Usd), ("path", FieldKind::Path), ("argv", FieldKind::Argv), ("pattern", FieldKind::Pattern), ("expr", FieldKind::Expr), ("secret", FieldKind::Secret), ("markdown", FieldKind::Markdown)] {
        assert_eq!(serde_json::to_value(kind).expect("JSON"), serde_json::json!(word));
        assert!(kind.is_value());
    }
}

#[test]
fn the_display_kinds_and_the_window_travel_in_the_words_the_board_uses_and_leave_out_what_is_not_there() {
    use j4n_proto::{Cell, Down, Intent, Row, Tone, Up};
    for (word, kind) in [("stat", FieldKind::Stat), ("meter", FieldKind::Meter), ("spark", FieldKind::Spark), ("line", FieldKind::Line), ("bars", FieldKind::Bars), ("chip", FieldKind::Chip), ("action", FieldKind::Action), ("table", FieldKind::Table), ("tree", FieldKind::Tree), ("card", FieldKind::Card), ("cards", FieldKind::Cards)] {
        assert_eq!(serde_json::to_value(kind).expect("JSON"), serde_json::json!(word));
        assert!(!kind.is_value(), "{word} shows, it is not changed");
    }
    let cell = Cell::with_icon("done", "ready", Tone::Good);
    assert_eq!(serde_json::to_value(&cell).expect("JSON"), serde_json::json!({ "text": "ready", "tone": "good", "icon": "done" }));
    assert_eq!(serde_json::to_value(Cell::new("x", Tone::Plain)).expect("JSON"), serde_json::json!({ "text": "x", "tone": "plain" }), "no icon, no key");
    let row = Row { key: "a".into(), parent: Some("p".into()), ..Row::default() };
    assert_eq!(serde_json::to_value(&row).expect("JSON")["parent"], "p");
    assert!(serde_json::to_value(Row::default()).expect("JSON").get("parent").is_none());
    let whole = Down::Rows { bind: "b".into(), rows: Vec::new(), total: 3, from: 0, rev: 1 };
    assert!(serde_json::to_value(&whole).expect("JSON").get("from").is_none(), "all the rows from the start say nothing of where they are");
    let window = Down::Rows { bind: "b".into(), rows: Vec::new(), total: 1_000_000, from: 999_000, rev: 1 };
    assert_eq!(serde_json::from_value::<Down>(serde_json::to_value(&window).expect("JSON")).expect("and back"), window);
    let old: Down = serde_json::from_str(r#"{"down":"rows","bind":"b","rows":[],"total":0,"rev":1}"#).expect("rows from before windows");
    assert_eq!(old, Down::Rows { bind: "b".into(), rows: Vec::new(), total: 0, from: 0, rev: 1 });
    let ask = Up::Window { bind: "b".into(), from: 10, count: 61, sort: "-cost".into(), find: String::new() };
    assert_eq!(serde_json::to_value(&ask).expect("JSON"), serde_json::json!({ "up": "window", "bind": "b", "from": 10, "count": 61, "sort": "-cost" }), "no find, no key");
    assert_eq!(serde_json::from_value::<Up>(serde_json::to_value(&ask).expect("JSON")).expect("and back"), ask);
    let intent = Intent::from_json(&serde_json::json!({ "intent": "run", "command": "probe", "args": ["omp"] }));
    assert_eq!(intent, Some(Intent::Run { command: "probe".into(), args: vec!["omp".into()] }));
    assert_eq!(Intent::from_json(&serde_json::json!({ "intent": "nonsense" })), None);
    assert_eq!(Intent::from_json(&serde_json::json!(3)), None);
}

#[test]
fn who_made_a_row_how_it_is_folded_and_whether_it_merges_travel_as_words_and_leave_out_what_is_the_usual() {
    use j4n_proto::{By, Fold, Row};
    let plain = serde_json::to_value(Row::default()).expect("JSON");
    for key in ["by", "fold", "merge"] {
        assert!(plain.get(key).is_none(), "{key} is left out when it says nothing");
    }
    let row = Row { key: "g".into(), by: By::Model, fold: Some(Fold::Pinned), merge: true, ..Row::default() };
    let v = serde_json::to_value(&row).expect("JSON");
    assert_eq!((&v["by"], &v["fold"], &v["merge"]), (&serde_json::json!("model"), &serde_json::json!("pinned"), &serde_json::json!(true)));
    assert_eq!(serde_json::from_value::<Row>(v).expect("and back"), row);
    for (word, by) in [("operator", By::Operator), ("core", By::Core), ("rule", By::Rule), ("doer", By::Doer), ("model", By::Model)] {
        assert_eq!(serde_json::from_value::<By>(serde_json::json!(word)).expect("a word"), by);
    }
    for (word, fold) in [("open", Fold::Open), ("folded", Fold::Folded), ("pinned", Fold::Pinned)] {
        assert_eq!(serde_json::to_value(fold).expect("JSON"), serde_json::json!(word));
    }
    let old: Row = serde_json::from_str(r#"{"key":"a","cells":[]}"#).expect("a row from before the chat rows");
    assert_eq!((old.by, old.fold, old.merge), (By::Operator, None, false));
    assert_eq!(j4n_proto::COMPOSER_STRIP, "composer:strip");
}

#[test]
fn the_fields_of_a_question_that_the_ask_field_draws_are_on_the_wire_only_when_they_have_something() {
    use j4n_proto::{AskView, By, Cell, Choice, Row, Shape, Tone};
    let plain = AskView { ask: "a".into(), asker: "t".into(), question: "go?".into(), ..AskView::default() };
    let v = serde_json::to_value(&plain).unwrap();
    for field in ["wait", "facts", "change", "why", "options", "effect", "grants"] {
        assert!(v.get(field).is_none(), "{field} is not sent empty: {v}");
    }
    let old: AskView = serde_json::from_value(serde_json::json!({"ask": "a", "asker": "t", "question": "go?", "shape": "approval", "risk": 5})).unwrap();
    assert_eq!((old.wait.as_str(), old.change.len(), old.facts.len(), old.why.as_str()), ("", 0, 0, ""));
    let c: Choice = serde_json::from_value(serde_json::json!({"id": "yes", "recommended": true, "by": "rule"})).unwrap();
    assert_eq!((c.label.as_str(), c.by, c.widens, c.note.as_str()), ("", By::Rule, false, ""));
    let full = AskView {
        shape: Shape::Approval,
        wait: "8:42".into(),
        facts: vec![Cell::with_icon("worktree", "b-claude", Tone::Dim)],
        change: vec![Row { key: "c1".into(), cells: vec![Cell::new("parser.rs", Tone::Plain)], ..Row::default() }],
        why: "it can be undone".into(),
        options: vec![Choice { id: "session".into(), label: "yes".into(), widens: true, note: "n".into(), by: By::Model, recommended: false }],
        ..plain
    };
    let back: AskView = serde_json::from_value(serde_json::to_value(&full).unwrap()).unwrap();
    assert_eq!(back, full);
}

#[test]
fn the_share_and_the_height_of_a_node_are_on_the_wire_only_when_it_has_one_and_an_old_node_still_reads() {
    let sized = Node { id: "bar".into(), grow: 2, height: 1, ..Node::default() };
    let wire = serde_json::to_value(&sized).expect("a node is JSON");
    assert_eq!((wire["grow"].clone(), wire["height"].clone()), (serde_json::json!(2), serde_json::json!(1)));
    assert_eq!(serde_json::from_value::<Node>(wire).expect("and back"), sized);
    let plain = serde_json::to_value(Node { id: "x".into(), ..Node::default() }).expect("JSON");
    assert!(plain.get("grow").is_none() && plain.get("height").is_none(), "{plain}");
    let old: Node = serde_json::from_str(r#"{"id":"x","title":""}"#).expect("a node from before templates");
    assert_eq!((old.grow, old.height), (0, 0));
}

#[test]
fn the_providers_are_every_provider_the_library_lists_and_not_only_the_connectors_of_two_dialects() {
    // the old view took the connectors of dialect `openai` or `anthropic`, so claude and OpenRouter were not there
    let s = Screen::builtin();
    let p = |id: &str, kind: &str| map(&[("id", Value::text(id)), ("kind", Value::text(kind))]);
    let src = Fake(BTreeMap::from([("fold:providers".to_string(), vec![p("anthropic", "subscription"), p("openrouter", "router"), p("pi", "harness")])]));
    let rows = s.views.get("providers").unwrap().rows(&src, &outer(s.set.clone()));
    assert_eq!(rows.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), ["anthropic", "openrouter", "pi"]);
}

// ---- the titles of a list, and one list style in Settings ----

fn titles_of(n: &Node) -> Vec<String> {
    n.field.as_ref().and_then(|f| f.props.get("titles")).and_then(Json::as_array).map(|a| a.iter().filter_map(Json::as_str).map(String::from).collect()).unwrap_or_default()
}

#[test]
fn a_view_may_name_its_columns_and_a_view_file_with_no_titles_still_loads() {
    let old = Views::load(&tables(&[("views/p.toml", "api = 1\n[[view]]\nid = \"v\"\nfrom = { from = \"tasks\" }\ncells = [\"{x}\", \"{y}\"]\n")])).0;
    assert!(old.get("v").is_some_and(|v| v.titles().is_empty()), "a file written before the titles was read as it was");
    let new = Views::load(&tables(&[("views/p.toml", "api = 1\n[[view]]\nid = \"v\"\nfrom = { from = \"tasks\" }\ncells = [\"{x}\", \"{y}\"]\ntitles = [\"what\", \"how\"]\n")])).0;
    assert_eq!(new.get("v").map(|v| v.titles().to_vec()), Some(vec!["what".to_string(), "how".to_string()]));
}

/// The settings with `tabs.all` on, so Look, This Session and Loop (which wait for it) are in the tree.
fn every_tab() -> Value {
    Value::from_json(&serde_json::json!({ "tabs": { "all": true } }))
}

#[test]
fn the_list_of_a_view_is_sent_with_the_titles_of_the_view_in_the_props_of_its_field() {
    let screen = Screen::builtin();
    let tree = screen.titled(screen.pages.tree("main", &outer(every_tab())));
    for page in ["look", "loop", "config"] {
        let node = find(&tree, page).unwrap_or_else(|| panic!("the page {page}"));
        assert_eq!(titles_of(node), ["setting", "now", "what it does"], "{page}");
    }
    assert_eq!(titles_of(find(&tree, "keys").expect("keys")), ["key", "keys", "native", "what it does"]);
    // a page that names its own titles keeps them
    let (pages, _) = Pages::load(&tables(&[("pages/p.toml", "api = 1\n[[page]]\nid = \"x\"\nparent = \"home\"\ntitle = \"X\"\norder = 1\nfield = \"list\"\nview = \"settings-look\"\nprops = { titles = [\"mine\"] }\n")]), &BTreeMap::new());
    let own = screen.titled(pages.tree("main", &outer(screen.set.clone())));
    assert_eq!(titles_of(find(&own, "x").expect("x")), ["mine"]);
}

#[test]
fn settings_chat_is_a_list_like_the_other_sub_tabs() {
    let screen = Screen::builtin();
    let tree = screen.titled(screen.pages.tree("main", &outer(every_tab())));
    let chat = find(&tree, "settings").expect("the Chat page of Settings");
    let look = find(&tree, "look").expect("the Look page");
    assert_eq!(chat.field.as_ref().map(|f| (f.kind, f.bind.as_str())), Some((FieldKind::List, "form:thread")));
    assert_eq!(look.field.as_ref().map(|f| f.kind), Some(FieldKind::List), "the same kind of field");
    assert_eq!(titles_of(chat), ["setting", "now"]);
}

#[test]
fn a_setting_is_described_once_in_the_row_and_its_detail_says_it_again_in_full() {
    // the cell is the whole sentence, so the screen can give it all the width; the screen blanks it while the detail is open
    let screen = Screen::builtin();
    let view = screen.views.get("settings-look").expect("the Look view");
    struct Src;
    impl Sources for Src {
        fn rows(&self, coll: &Coll) -> Vec<Value> {
            match coll.to_string().as_str() {
                "table:setting" => vec![map(&[("id", Value::text("theme.corners")), ("now", Value::text("round")), ("doc", Value::text("The corners of a box: `round` or `square`, and then a good deal more words than forty characters"))])],
                _ => Vec::new(),
            }
        }
    }
    let rows = view.rows(&Src, &outer(screen.set.clone()));
    let row = rows.first().expect("a row");
    assert!(row.cells[2].text.chars().count() > 40 && !row.cells[2].text.ends_with('…'), "{:?}", row.cells);
    assert_eq!(row.detail.first().map(|d| d.text.as_str()), Some(row.cells[2].text.as_str()));
}

#[test]
fn the_chart_props_are_checked_the_same_for_the_core_and_the_screen_and_the_pack_lists_hold_the_kinds() {
    use j4n_proto::{CHART_SERIES, chart_problem, json};
    let ok = json!({ "style": "wave", "scale": "log", "right_scale": "lin", "window": 14, "rows": 7, "series": [{ "name": "a", "axis": "right", "role": "run", "unit": "usd" }] });
    assert_eq!(chart_problem(FieldKind::Line, &ok), None);
    let many: Vec<_> = (0..=CHART_SERIES).map(|i| json!({ "name": format!("s{i}") })).collect();
    for (props, word) in [
        (json!({ "series": many }), "one to 4"),
        (json!({}), "one to 4"),
        (json!({ "series": [{ "name": "a" }, { "name": "a" }] }), "used already"),
        (json!({ "style": "bubbly", "series": [{ "name": "a" }] }), "`style` is \"bubbly\""),
        (json!({ "series": [{ "name": "a", "axis": "up" }] }), "series `a`: `axis`"),
        (json!({ "series": [{ "name": "a", "role": "pink" }] }), "`role`"),
        (json!({ "series": [{ "name": "a", "unit": "kg" }] }), "`unit`"),
        (json!({ "window": 0, "series": [{ "name": "a" }] }), "`window`"),
        (json!({ "rows": 2, "series": [{ "name": "a" }] }), "`rows`"),
    ] {
        assert!(chart_problem(FieldKind::Line, &props).is_some_and(|w| w.contains(word)), "{props}");
    }
    assert!(chart_problem(FieldKind::Bars, &json!({ "scale": "log", "series": [{ "name": "a" }] })).is_some(), "bars are linear");
    assert_eq!(chart_problem(FieldKind::Bars, &json!({ "orient": "horizontal", "series": [{ "name": "a" }] })), None);
    // the words a pack may write are the words the screen draws: the lists of the base pack name both kinds
    for text in [include_str!("../../../../packs/base/kinds/page.toml"), include_str!("../../../../packs/base/kinds/template.toml")] {
        assert!(text.contains("|spark|line|bars|chip|"), "page and template list the chart kinds");
    }
}

#[test]
fn a_view_key_that_sets_a_key_without_the_rows_secret_editor_or_with_a_value_is_refused_at_load() {
    let view = |id: &str, data: &str, act: &str| format!("[[view]]\nid = \"{id}\"\nfrom = {{ from = \"tasks\" }}\ncells = [\"{{id}}\"]\n{data}acts = [{act}]\n");
    let text = format!(
        "api = 1\n{}{}{}",
        view("bare", "", r#"{ key = "enter", label = "type the key", set = "key:{id}.value" }"#),
        view("valued", "data = \"{edit}\"\n", r#"{ key = "enter", label = "type the key", set = "key:{id}.value", value = "{token}" }"#),
        view("edited", "data = \"{edit}\"\n", r#"{ key = "enter", label = "type the key", set = "key:{id}.value" }"#),
    );
    let (views, found) = Views::load(&tables(&[("views/p.toml", &text)]));
    let at = |id: &str| found.iter().find(|p| p.at == format!("[[view]] {id}")).map(|p| p.msg.clone());
    assert!(at("bare").is_some_and(|m| m.contains("secret editor")), "{found:?}");
    assert!(at("valued").is_some_and(|m| m.contains("no `value`")), "{found:?}");
    assert!(at("edited").is_none() && views.get("edited").is_some(), "a key set through the row's editor stands: {found:?}");
}
