//! What a cell reports of the parts it used, the parts the operator turned
//! off (`kit.off`), the card's last sentence, the offer line without them, and an MCP server's tools reachable from code only where its row says
//! `code = true`. The kernel is the real one; the MCP server is a scripted set of tools; nothing leaves the machine.

use crate::bench::base_layer;
use crate::world::Scratch;
use j4n_connect::kithost::{Host, KitSet};
use j4n_connect::kituse::{PARTS, card_with};
use j4n_connect::manifest::{Connector, Connectors, Reach, Survey};
use j4n_connect::model::doer::{Gated, ToolOut, Tools};
use j4n_connect::model::{Call, Tool};
use j4n_connect::py::PyTools;
use j4n_core::gate::ReadOnly;
use j4n_core::place::{Launch, Site};
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use serde_json::json;
use std::path::Path;
use std::sync::{Arc, Mutex};

fn tools_with(off: &[&str], offer: Option<Arc<dyn Tools>>) -> PyTools {
    let set = KitSet { off: off.iter().map(|s| s.to_string()).collect(), ..KitSet::default() };
    PyTools::new().with_host(Arc::new(Host::new(set).with_offer(offer)))
}

fn cell(t: &PyTools, site: &Site, code: &str) -> ToolOut {
    t.run(&Call { id: "c1".into(), name: "py".into(), args: json!({ "code": code }), bad: None }, site, &|l: Launch| l)
}

fn value(out: &ToolOut) -> String {
    out.text.lines().find_map(|l| l.strip_prefix("=> ")).unwrap_or_default().to_string()
}

#[test]
fn a_cell_says_which_parts_of_the_library_it_used_and_a_part_calling_a_part_is_one_use() {
    if !j4n_connect::py::python_here() {
        return;
    }
    let dir = Scratch::new("ku483");
    let site = Site::new("t1", dir.path());
    let t = tools_with(&[], None);
    let out = cell(&t, &site, "import j4n\nj4n.chunks('a', 5)\nj4n.chunks('b', 5)\nj4n.grep(['a'], 'a')\n1");
    assert!(!out.error, "{}", out.text);
    assert_eq!(out.py.as_ref().unwrap()["used"], json!({ "chunks": 2, "grep": 1 }));
    // the next cell starts from nothing; a cell that used none says `{}`, not nothing
    let out = cell(&t, &site, "x = 1");
    assert_eq!(out.py.as_ref().unwrap()["used"], json!({}));
    // a recipe is itself and its name; what it calls inside is not counted
    let out = cell(&t, &site, "import j4n\nj4n.recipe('outline', text='a\\n\\nb', n=1)\n1");
    assert_eq!(out.py.as_ref().unwrap()["used"], json!({ "recipe": 1, "recipe:outline": 1 }), "{}", out.text);
    // a cell that fails reports what it used before it failed
    let out = cell(&t, &site, "import j4n\nj4n.chunks('a', 5)\n1 / 0");
    assert!(out.error);
    assert_eq!(out.py.as_ref().unwrap()["used"], json!({ "chunks": 1 }));
}

#[test]
fn a_cell_that_writes_the_librarys_account_by_hand_reaches_the_log_with_known_parts_and_clamped_counts_only() {
    if !j4n_connect::py::python_here() {
        return;
    }
    let dir = Scratch::new("ku-forged");
    let site = Site::new("t1", dir.path());
    let t = tools_with(&[], None);
    let out = cell(&t, &site, "import j4n\nj4n._USED.update({'evil': 5, 'recipe:nope': 1, 'chunks': 10**9, 'heads': 10**30, 'grep': 'x'})\nj4n._USED.update({f'k{i}': 1 for i in range(5000)})\nj4n.table('a,b')\n1");
    assert!(!out.error, "{}", out.text);
    assert_eq!(out.py.as_ref().unwrap()["used"], json!({ "chunks": 100_000, "heads": 1, "grep": 1, "table": 1 }));
}

#[test]
fn the_parts_the_fold_knows_are_the_parts_the_library_counts() {
    let lib = include_str!("../../../../packs/kit/py/j4n.py");
    let line = lib.lines().find(|l| l.starts_with("_PARTS = (")).expect("the library names its parts");
    let named: Vec<&str> = line.trim_start_matches("_PARTS = (").trim_end_matches(')').split(", ").map(|p| p.trim_matches('"')).collect();
    assert_eq!(named, PARTS, "`PARTS` in kituse.rs and `_PARTS` in j4n.py are one list");
    for (name, _) in j4n_connect::py::RECIPES {
        assert!(Path::new(&format!("{}/../../packs/kit/recipes/{name}.py", env!("CARGO_MANIFEST_DIR"))).exists(), "{name}");
    }
}

#[test]
fn a_part_in_kit_off_is_refused_in_a_cell_left_out_of_the_docs_and_recipes_and_the_rest_runs() {
    if !j4n_connect::py::python_here() {
        return;
    }
    let dir = Scratch::new("ku485");
    let site = Site::new("t1", dir.path());
    let t = tools_with(&["dedup", "recipe:outline"], None);
    let out = cell(&t, &site, "import j4n\nj4n.dedup(['a'])");
    assert!(out.error && out.text.contains("Refused") && out.text.contains("kit.off"), "{}", out.text);
    assert_eq!(out.py.as_ref().unwrap()["used"], json!({}), "a refused call is no use");
    let ok = cell(&t, &site, "import j4n\nj4n.heads(['# a\\nb'])\n'dedup' in j4n.__all__, 'dedup(' in j4n.__doc__, 'outline' in j4n.recipes(), 'matching' in j4n.recipes()");
    assert!(!ok.error, "{}", ok.text);
    assert_eq!(value(&ok), "(False, False, False, True)");
    let gone = cell(&t, &site, "import j4n\nj4n.recipe('outline', text='a')");
    assert!(gone.error && gone.text.contains("no recipe called 'outline'"), "{}", gone.text);
}

#[test]
fn the_card_is_as_it_was_measured_until_a_part_is_off_and_then_names_it_in_a_last_sentence() {
    let card = j4n_connect::py::CARD;
    assert_eq!(card_with(card, &[]), card.trim(), "nothing off: the card as it was measured");
    let one = card_with(card, &["dedup".into(), "recipe:each".into()]);
    assert!(one.starts_with(card.trim()) && one.ends_with("a cell that calls them is refused: `j4n.dedup`, the recipe `each`."), "{one}");
    assert!(!one.contains("  "), "{one}");
}

/// The judge's sentences are in the card of the first turn only while `kit.judge` is on.
#[test]
fn the_card_leaves_out_the_judge_while_kit_judge_is_off_and_is_whole_while_it_is_on() {
    let tools = |judge: bool| PyTools::new().with_host(Arc::new(Host::new(KitSet { judge, ..KitSet::default() })));
    for card in [j4n_connect::kitsock::CARD, j4n_connect::py::CARD] {
        let on = tools(true).card(card);
        assert_eq!(on, card.trim(), "on: the card as it was measured");
        let off = tools(false).card(card);
        for gone in ["judge", "j4n.choose", "j4n.score", "j4n.keep", "kit.judge", "the error says so"] {
            assert!(!off.contains(gone), "{gone} is still in the card with kit.judge off: {off}");
        }
        assert!(off.contains("j4n.heads(items)") && off.contains("j4n.model(prompt, context=kept)") && off.contains("`j4n.recipes()`"), "the rest stays: {off}");
        assert!(on.len() - off.len() > 300 && !off.contains("  "), "the judge's sentences are out: {} against {}", on.len(), off.len());
        // a part turned off is still named in the last sentence, with the judge in or out
        assert!(tools(false).card(card).len() < on.len() && card_with(&off, &["dedup".into()]).ends_with("`j4n.dedup`."));
    }
    // with no host at all there is no judge
    assert!(!PyTools::new().card(j4n_connect::kitsock::CARD).contains("kit.judge"));
}

#[test]
fn the_offer_line_does_not_name_a_recipe_or_many_that_is_off() {
    use j4n_connect::kitoffer::{line, line_with};
    let dir = Scratch::new("ku-offer");
    dir.put("bugs.txt", &(1..=24).map(|i| format!("item-{i}\n")).collect::<String>());
    let specs = vec![Tool { name: "recall".into(), doc: "Search the project's memory for words.".into(), schema: json!({ "type": "object", "properties": { "query": { "type": "string" } }, "required": ["query"] }) }];
    let goal = "bugs.txt lists 24 bug ids. For each one, find in the project's memory whether it is fixed.";
    let off = |names: &[&str]| names.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let base = line(goal, dir.path(), &specs).unwrap();
    assert_eq!(line_with(goal, dir.path(), &specs, &[]), Some(base.clone()), "nothing off: the line as it was");
    assert!(base.contains("j4n.recipe('each'"), "{base}");
    let many = line_with(goal, dir.path(), &specs, &off(&["recipe:each"])).unwrap();
    assert!(many.contains("j4n.many('recall'") && !many.contains("recipe"), "{many}");
    let none = line_with(goal, dir.path(), &specs, &off(&["recipe", "many"]));
    assert_eq!(none, None, "with `many` and the recipes off there is nothing to offer");
    assert!(line_with(goal, dir.path(), &specs, &off(&["many"])).unwrap().contains("recipe('each'"), "`each` runs `many` inside itself");
}

// ---- MCP: reachable from code only where the row says `code = true` ----

/// A server whose tools are `srv__ping`, called from the offer by its name; a page it brings back is untrusted.
struct Server(Mutex<Vec<String>>);

impl Tools for Server {
    fn specs(&self) -> Vec<Tool> {
        vec![Tool { name: "srv__ping".into(), doc: "Ping the server.".into(), schema: json!({ "type": "object", "properties": { "text": { "type": "string" } } }) }]
    }

    fn untrusted(&self, call: &Call) -> Option<String> {
        (call.name == "srv__ping").then(|| "srv".to_string())
    }

    fn gate(&self, _: &Call, _: &str, _: &Site, _: ReadOnly) -> Option<Gated> {
        None
    }

    fn run(&self, call: &Call, _: &Site, _: &(dyn Fn(Launch) -> Launch + Send + Sync)) -> ToolOut {
        self.0.lock().unwrap().push(call.args.to_string());
        ToolOut { text: format!("pong {}", call.args["text"].as_str().unwrap_or_default()), ..ToolOut::default() }
    }
}

struct Fake(Arc<Server>);

impl Reach for Fake {
    fn tools(&self, word: &str, row: Option<&Connector>) -> Option<Arc<dyn Tools>> {
        (word == "srv" && row.is_some()).then(|| self.0.clone() as Arc<dyn Tools>)
    }

    fn survey(&self, _: &Connector, _: &dyn Fn(Launch) -> Launch, _: &Path) -> Result<Survey, String> {
        Err("not here".into())
    }
}

fn rows(code: &str) -> String {
    format!(
        r#"
[[connector]]
id = "builtin"
doc = "The tools J4n carries."
is = "tool"
dialect = "builtin"

[[connector]]
id = "own"
doc = "A model."
is = "agent"
dialect = "openai"
url = "http://127.0.0.1:1/v1"
keyless = true
tools = ["builtin", "*", "ask", "py"]
models = ["m"]

[[connector]]
id = "srv"
doc = "A server."
is = "mcp"
dialect = "mcp"
program = "fake-server"
{code}
"#
    )
}

fn connectors(code: &str, server: &Arc<Server>) -> Connectors {
    let user = Layer::new(LayerKind::User, "mine", vec![SourceFile { path: "connectors/x.toml".into(), text: format!("api = 1\n{}", rows(code)) }]);
    let (tables, problems) = Tables::load(vec![base_layer(), user]);
    assert!(problems.is_empty(), "{problems:?}");
    let (mut list, said) = Connectors::load(&tables);
    assert!(said.is_empty(), "{said:?}");
    list.set_reach(Arc::new(Fake(server.clone())));
    list
}

#[test]
fn an_mcp_tool_is_refused_through_the_bridge_without_code_true_and_reachable_with_it() {
    if !j4n_connect::py::python_here() {
        return;
    }
    let dir = Scratch::new("ku486");
    let site = Site::new("t1", dir.path());
    let secret = |_: &str, _: &str| Err::<String, String>("no secret".into());
    let server = Arc::new(Server(Mutex::default()));
    let run = |code: &str| {
        let list = connectors(code, &server);
        let offer = list.bridge_offer(list.get("own").expect("the row"), &secret);
        let t = tools_with(&[], Some(offer));
        let listed = cell(&t, &site, "import j4n\nsorted(j4n.tools())");
        assert!(!listed.error, "{}", listed.text);
        let called = cell(&t, &site, "import j4n\nj4n.call('srv__ping', text='a')");
        (value(&listed), called)
    };
    // the default: off. The server's tools are not listed and a call by name is refused, and the server never hears of it
    for row in ["", "code = false"] {
        let (listed, called) = run(row);
        assert!(!listed.contains("srv__ping"), "{row:?}: {listed}");
        assert!(called.error && called.text.contains("is not a tool that code may call"), "{row:?}: {}", called.text);
        assert!(server.0.lock().unwrap().is_empty(), "the server was reached");
    }
    // the row says so: listed, and a call runs the server's own tool under the same walls as any (here: it answers, and says it is untrusted)
    let (listed, called) = run("code = true");
    assert!(listed.contains("srv__ping") && listed.contains("read"), "{listed}");
    assert!(!called.error && called.text.contains("pong a"), "{}", called.text);
    assert_eq!(server.0.lock().unwrap().len(), 1);
    let row = &called.py.as_ref().unwrap()["host"][0];
    assert_eq!((row["tool"].as_str(), row["untrusted"].as_str()), (Some("srv__ping"), Some("srv")), "the call is a row of the cell and taints the run: {row}");
    // `code` belongs to an MCP row: on another dialect the row is refused
    let user = Layer::new(LayerKind::User, "mine", vec![SourceFile { path: "connectors/y.toml".into(), text: "api = 1\n[[connector]]\nid = \"x\"\nis = \"tool\"\ndialect = \"text\"\nprogram = \"echo\"\nargv = [\"echo\"]\nrisk = \"read\"\ncode = true\n".into() }]);
    let (tables, _) = Tables::load(vec![base_layer(), user]);
    let (_, said) = Connectors::load(&tables);
    assert!(said.iter().any(|p| p.to_string().contains("`code` is for an `mcp` server")), "{said:?}");
}
