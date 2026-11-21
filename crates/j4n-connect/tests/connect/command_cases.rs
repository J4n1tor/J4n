//! Terminal commands as the tools of a model (`command.rs`): what they offer, how a call becomes a command line, and what they
//! refuse. And the Connect flow's helpers that need no loop: the TOML a drafter wrote, out of its words.

use crate::bench::base_layer;
use crate::world::Scratch;
use j4n_connect::command::{CommandTools, Union, offered};
use j4n_connect::connect::toml_of;
use j4n_connect::manifest::{Connector, Connectors};
use j4n_connect::model::Call;
use j4n_connect::model::doer::Tools;
use j4n_core::gate::check::Gates;
use j4n_core::gate::grant::Grants;
use j4n_core::gate::{Gate, ReadOnly};
use j4n_core::place::Site;
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use serde_json::{Value, json};
use std::sync::Arc;

fn connectors(text: &str) -> Vec<Connector> {
    let rows = Layer::new(LayerKind::User, "mine", vec![SourceFile { path: "connectors/x.toml".into(), text: format!("api = 1\n{text}") }]);
    let (tables, problems) = Tables::load(vec![base_layer(), rows]);
    assert!(problems.is_empty(), "{problems:?}");
    let (list, said) = Connectors::load(&tables);
    assert!(said.is_empty(), "{said:?}");
    list.all().to_vec()
}

fn call(name: &str, args: Value) -> Call {
    Call { id: "c1".into(), name: name.into(), args, bad: None }
}

const ECHO: &str = r#"
[[connector]]
id = "say"
doc = "Say a word."
is = "tool"
dialect = "text"
program = "echo"
args = [{ name = "word", required = true }, { name = "times", default = "1" }]
argv = ["echo", "{word}", "x{times}"]
risk = "read"
"#;

#[test]
fn a_command_offers_its_arguments_as_the_parameters_of_a_function() {
    let rows = connectors(ECHO);
    let tools = CommandTools::new(rows);
    let specs = tools.specs();
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].name, "say");
    assert_eq!(specs[0].doc, "Say a word.");
    assert_eq!(specs[0].schema["properties"]["word"]["type"], "string");
    assert_eq!(specs[0].schema["required"], json!(["word"]), "an argument with a default is not required");
}

#[test]
fn a_call_is_one_word_per_argument_and_a_value_is_never_syntax() {
    let dir = Scratch::new("cmd");
    let tools = CommandTools::new(connectors(ECHO));
    let out = tools.run(&call("say", json!({ "word": "a b; rm -rf /" })), &Site::new("t1", dir.path()), &|l| l);
    assert!(!out.error, "{}", out.text);
    assert_eq!(out.text.trim(), "a b; rm -rf / x1", "the value stayed one word of the command line");
    let shell = out.shell.expect("the command and its exit are in the Log");
    assert_eq!(shell.exit, 0);
    assert!(shell.cmd.starts_with("echo a b; rm -rf / x1"), "{}", shell.cmd);
}

#[test]
fn a_call_that_lacks_a_required_argument_or_would_pass_a_flag_does_not_run() {
    let dir = Scratch::new("cmd");
    let tools = CommandTools::new(connectors(ECHO));
    let site = Site::new("t1", dir.path());
    let missing = tools.run(&call("say", json!({})), &site, &|l| l);
    assert!(missing.error && missing.text.contains("needs `word`"), "{}", missing.text);
    let flag = tools.run(&call("say", json!({ "word": "--help" })), &site, &|l| l);
    assert!(flag.error && flag.text.contains("flag"), "{}", flag.text);
    let nothing = tools.run(&call("other", json!({})), &site, &|l| l);
    assert!(nothing.error && nothing.text.contains("no tool"), "{}", nothing.text);
}

#[test]
fn a_call_is_put_to_the_gate_as_a_command_line_that_blocks_one_it_cannot_place_in_a_read_only_step() {
    let (tables, _) = Tables::load(vec![base_layer()]);
    let gate = Gate::new(Gates::load(&tables).0, Grants::new("s1"));
    let dir = Scratch::new("cmd");
    let mystery = "[[connector]]\nid = \"mystery\"\nis = \"tool\"\ndialect = \"text\"\nprogram = \"mystery\"\nargs = [{ name = \"x\", required = true }]\nargv = [\"mystery\", \"{x}\"]\nrisk = \"write\"\n";
    let tools = CommandTools::new(connectors(&format!("{ECHO}\n{mystery}"))).with_gate(gate);
    let site = Site::new("t1", dir.path());
    assert!(tools.gate(&call("say", json!({ "word": "hi" })), "t1", &site, ReadOnly::No).is_none(), "the Gate knows `echo` writes nothing");
    assert!(tools.gate(&call("mystery", json!({ "x": "1" })), "t1", &site, ReadOnly::No).is_none(), "no rule objects to it, as to `run`");
    let blocked = tools.gate(&call("mystery", json!({ "x": "1" })), "t1", &site, ReadOnly::Explicit).expect("and in a read-only step it is refused");
    assert_eq!(blocked.risk, 100);
}

#[test]
fn only_a_text_tool_with_a_command_line_is_offered_and_never_under_the_name_of_a_tool_j4n_carries() {
    let rows = connectors(&format!(
        "{ECHO}\n[[connector]]\nid = \"grep\"\nis = \"tool\"\ndialect = \"text\"\nprogram = \"grep\"\nargv = [\"grep\", \"{{prompt}}\"]\n\n[[connector]]\nid = \"bare\"\nis = \"tool\"\ndialect = \"text\"\nprogram = \"true\"\n"
    ));
    let names: Vec<&str> = rows.iter().filter(|c| offered(c)).map(|c| c.id.as_str()).collect();
    assert_eq!(names, ["say"], "`grep` would be taken for the tool J4n carries and `bare` has no command line");
}

#[test]
fn several_sets_of_tools_are_one_and_a_call_goes_to_the_set_that_has_the_name() {
    let dir = Scratch::new("cmd");
    let union = Union(vec![Arc::new(j4n_connect::tools::Builtin::new()), Arc::new(CommandTools::new(connectors(ECHO)))]);
    let names: Vec<String> = union.specs().into_iter().map(|t| t.name).collect();
    assert_eq!(names, ["list", "read", "grep", "edit", "write", "run", "git", "say"]);
    let site = Site::new("t1", dir.path());
    assert_eq!(union.run(&call("say", json!({ "word": "x" })), &site, &|l| l).text.trim(), "x x1");
    assert!(union.run(&call("nope", json!({})), &site, &|l| l).error);
}

#[test]
fn the_toml_of_a_drafters_words_is_the_fenced_block_or_the_text_from_its_first_table() {
    assert_eq!(toml_of("Here:\n```toml\na = 1\n```\nbye").trim(), "a = 1");
    assert_eq!(toml_of("```\nb = 2\n```").trim(), "b = 2");
    assert_eq!(toml_of("I think this:\n\n[[connector]]\nid = \"x\"\n").trim(), "[[connector]]\nid = \"x\"");
    assert_eq!(toml_of("sure thing\nid = \"x\"\nis = \"tool\"").trim(), "id = \"x\"\nis = \"tool\"");
    // a fence of another language is not the connector
    assert_eq!(toml_of("```json\n{}\n```\n[[connector]]\nid = \"y\"").trim(), "[[connector]]\nid = \"y\"");
}

#[test]
fn an_argument_with_a_default_is_not_required_of_the_model_and_a_value_with_a_nul_is_refused() {
    let rows = connectors("[[connector]]\nid = \"d\"\nis = \"tool\"\ndialect = \"text\"\nprogram = \"echo\"\nargs = [{ name = \"a\", required = true, default = \"x\" }, { name = \"b\", required = true }]\nargv = [\"echo\", \"{a}\", \"{b}\"]\n");
    let tools = CommandTools::new(rows);
    assert_eq!(tools.specs()[0].schema["required"], json!(["b"]), "`a` has a default: the model need not give it");
    let dir = Scratch::new("cmd");
    let out = tools.run(&call("d", json!({ "b": "one\u{0}two" })), &Site::new("t1", dir.path()), &|l| l);
    assert!(out.error && out.text.contains("a character a command line cannot"), "{}", out.text);
    assert_eq!(tools.run(&call("d", json!({ "b": "ok" })), &Site::new("t1", dir.path()), &|l| l).text.trim(), "x ok");
}

#[test]
fn the_warning_before_a_call_is_what_the_row_says_it_can_do() {
    let row = |risk: &str| format!("[[connector]]\nid = \"r{risk}\"\nis = \"tool\"\ndialect = \"text\"\nprogram = \"x\"\nargv = [\"x\"]\nrisk = \"{risk}\"\n");
    let rows = connectors(&["read", "write", "net", "exec"].map(row).join("\n"));
    let tools = CommandTools::new(rows);
    let dir = Scratch::new("cmd");
    let site = Site::new("t1", dir.path());
    let risk = |name: &str| tools.gate(&call(name, json!({})), "t1", &site, ReadOnly::No).expect("with no rules loaded, asked").risk;
    assert_eq!((risk("rread"), risk("rwrite"), risk("rnet"), risk("rexec")), (10, 40, 40, 60));
}

/// A pack installed under a home of its own, as `j4n pack install` leaves it: a folder with a manifest, a connector row and a script.
fn installed(home: &Scratch, rows: &str) -> (Layer, Vec<j4n_core::table::Problem>) {
    let dir = home.path().join("packs/shipper");
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    std::fs::create_dir_all(dir.join("connectors")).unwrap();
    std::fs::write(dir.join("pack.toml"), "api = 1\nid = \"shipper\"\nversion = \"0.1.0\"\nsummary = \"a pack that ships a script\"\nstability = \"experimental\"\n").unwrap();
    std::fs::write(dir.join("bin/x"), "#!/bin/sh\necho \"shipped $1 from $(dirname \"$0\")\"\n").unwrap();
    std::fs::set_permissions(dir.join("bin/x"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    std::fs::write(dir.join("connectors/x.toml"), format!("api = 1\n{rows}")).unwrap();
    let (catalog, found) = j4n_core::pack::Catalog::new(Vec::new()).with_installed(&home.path().join("packs"));
    assert!(found.is_empty(), "{found:?}");
    let pin = catalog.get("shipper").expect("installed").pin();
    let on = j4n_core::pack::EnableRow { id: "shipper".into(), doc: String::new(), on: true, hash: Some(pin), ready: None, wrote: Vec::new() };
    let harness = catalog.harness(&[], &[on]);
    assert!(harness.info.iter().any(|i| i.id == "shipper" && i.on), "{:?}", harness.info);
    (harness.layers.into_iter().find(|l| l.name == "shipper").expect("its layer"), Vec::new())
}

const SHIPPED: &str = r#"
[[connector]]
id = "ship"
doc = "Run the script the pack ships."
is = "tool"
dialect = "text"
program = "{pack}/bin/x"
args = [{ name = "word", required = true }]
argv = ["{pack}/bin/x", "{word}"]
risk = "read"
net = false
"#;

#[test]
fn a_pack_names_the_script_it_ships_with_pack_wherever_its_home_is() {
    for tag in ["one", "two"] {
        let home = Scratch::new(&format!("pack-home-{tag}"));
        let (pack, _) = installed(&home, SHIPPED);
        let (tables, problems) = Tables::load(vec![base_layer(), pack]);
        assert!(problems.is_empty(), "{problems:?}");
        let (list, said) = Connectors::load(&tables);
        assert!(said.is_empty(), "{said:?}");
        let ship = list.all().iter().find(|c| c.id == "ship").expect("the row").clone();
        assert_eq!(ship.argv[0], format!("{}/packs/shipper/bin/x", home.path().display()), "the folder of the pack in the home it was found in");
        let tools = CommandTools::new(vec![ship]);
        let dir = Scratch::new("cmd");
        let seen = std::sync::Mutex::new(Vec::new());
        let out = tools.run(&call("ship", json!({ "word": "go" })), &Site::new("t1", dir.path()), &|l| {
            seen.lock().unwrap().push(l.no_net);
            l
        });
        assert!(!out.error, "{}", out.text);
        assert_eq!(out.text.trim(), format!("shipped go from {}/packs/shipper/bin", home.path().display()), "the script of the pack ran");
        assert_eq!(*seen.lock().unwrap(), [true], "`net = false` reaches the launch the box walls");
    }
}

#[test]
fn the_word_means_nothing_outside_a_pack_and_the_file_is_refused_with_the_way_out() {
    let rows = Layer::new(LayerKind::User, "home", vec![SourceFile { path: "connectors/x.toml".into(), text: format!("api = 1\n# a comment may say {{pack}}\n{SHIPPED}") }]);
    let (tables, problems) = Tables::load(vec![base_layer(), rows]);
    let said: Vec<String> = problems.iter().map(ToString::to_string).collect();
    assert!(said.iter().any(|p| p.starts_with("home:connectors/x.toml") && p.contains("`{pack}` is the folder of the pack whose file says it") && p.contains("this layer is not a pack") && p.contains("left out")), "{said:?}");
    assert!(tables.ids("connector").iter().all(|id| *id != "ship"), "no half of the row is read");
    // the comment alone is not the word
    let rows = Layer::new(LayerKind::User, "home", vec![SourceFile { path: "connectors/x.toml".into(), text: "api = 1\n# a comment may say {pack}\n".into() }]);
    assert!(Tables::load(vec![base_layer(), rows]).1.is_empty());
}

#[test]
fn a_check_row_can_say_net_false() {
    let rows = Layer::new(LayerKind::User, "home", vec![SourceFile { path: "checks/x.toml".into(), text: "api = 1\n[[check]]\nid = \"offline\"\nfiles = [\"f\"]\nargv = [\"true\"]\nnet = false\n".into() }]);
    let (tables, problems) = Tables::load(vec![base_layer(), rows]);
    assert!(problems.is_empty(), "{problems:?}");
    let row: j4n_connect::check::CheckRow = tables.get("offline").expect("the row");
    assert_eq!(row.net, Some(false));
}
