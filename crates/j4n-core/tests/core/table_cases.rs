//! Table: loading layers, merging them field by field, kinds as things, the trust gate on the project layer.

use crate::world::*;
use j4n_core::query::Query;
use j4n_core::schema::Mode;
use j4n_core::table::{KindDecl, Layer, LayerKind, Level, Problem, SourceFile, Tables, Untrusted};
use j4n_core::value::Value;

const THING: &str = r#"
name = { ty = "str", required = true }
size = { ty = "int", min = 1, max = 9, default = 3 }
ratio = "num"
extra = "any"
locked = { ty = "int", floor = true }
tags = "list<str>"
"#;

fn thing() -> Layer {
    kind_of("thing", THING)
}

#[test]
fn the_core_declares_the_kind_of_kinds_and_loads_clean() {
    let (t, problems) = load(vec![]);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(t.kinds().map(|(k, _)| k).collect::<Vec<_>>(), ["ask_point", "box", "check", "command", "compact_mode", "connector", "cost_source", "enable", "event", "failure", "flow", "fold", "gate", "hint", "host", "incident", "key", "kind", "look", "model", "need", "page", "phrase", "plan", "probe", "provider", "rank", "reflex", "reflex_case", "rule", "setting", "source", "template", "todo", "view"]);
    let decl: KindDecl = t.get("kind").unwrap();
    assert_eq!(decl.untrusted, Untrusted::Ignore, "a kind is never declared by a project that is not trusted");
    assert!(decl.doc.contains("kind of thing"));
    assert_eq!(decl.fields.keys().cloned().collect::<Vec<_>>(), ["doc", "examples", "fields", "untrusted"]);
    assert_eq!(decl.examples.len(), 2);
    assert!(!decl.fields.contains_key("id"), "`id` is every entry's own");
    let schema = t.schema("kind").unwrap();
    assert!(schema.fields["fields"].required && schema.fields["doc"].required);
}

#[test]
fn a_pack_of_an_absurd_domain_declares_kinds_and_its_entries_validate() {
    let (pack, read) = lighthouse();
    assert!(read.is_empty(), "{read:?}");
    let files: Vec<&str> = pack.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(files, ["keepers.toml", "kinds.toml"], "pack.toml is the manifest, not a table file");
    let (t, problems) = load(vec![pack]);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(t.kinds().map(|(k, _)| k).collect::<Vec<_>>(), ["ask_point", "box", "check", "command", "compact_mode", "connector", "cost_source", "enable", "event", "failure", "flow", "fog_signal", "fold", "gate", "hint", "host", "incident", "keeper", "key", "kind", "lamp", "look", "model", "need", "page", "phrase", "plan", "probe", "provider", "rank", "reflex", "reflex_case", "rule", "setting", "source", "template", "todo", "view"]);
    assert_eq!(t.ids("keeper"), ["ada", "bo"]);
    assert_eq!(t.ids("lamp"), ["north", "south"]);
    let north = t.raw("lamp", "north").unwrap();
    assert_eq!(north.fields["oil"].as_str(), Some("$0.40"));
    assert_eq!(north.fields["mode"].as_str(), Some("steady"), "a default");
    assert_eq!(north.from["oil"], "lighthouse");
    assert_eq!(north.from["mode"], "default");
    assert_eq!(t.raw("lamp", "south").unwrap().fields["mode"].as_str(), Some("off"));
    // kinds are things: the lamp kind is an entry of the kind `kind`
    let decl: KindDecl = t.get("lamp").unwrap();
    assert_eq!(decl.untrusted, Untrusted::Tighten);
    assert_eq!(t.list::<KindDecl>().unwrap().len(), 38);
}

#[test]
fn rows_carry_durations_and_money_as_such_so_a_query_compares_them_right() {
    let (pack, _) = lighthouse();
    let (t, problems) = load(vec![pack]);
    assert!(problems.is_empty(), "{problems:?}");
    let rows = t.rows("lamp");
    assert_eq!(rows.len(), 2);
    let ask = |filter: &str, order: Option<&str>| -> Vec<String> {
        let q = Query::parse("table:lamp", Some(filter), order, None).unwrap();
        q.apply(rows.clone(), &j4n_core::eval::Scope::new())
            .iter()
            .map(|r| r.at(&[j4n_core::value::Key::Name("id".into())]).to_string_lossy())
            .collect()
    };
    assert_eq!(ask("oil > $0.30", None), ["north"]);
    assert_eq!(ask("burn >= 8h", Some("id")), ["north", "south"], "the south lamp burns the default 8h");
    assert_eq!(ask("burn > 8h", None), ["north"]);
    assert_eq!(ask("oil == $0.25 and mode == 'off'", None), ["south"]);
    assert_eq!(ask("len(colors) > 1", None), ["north"]);
    assert_eq!(ask("keeper == 'bo'", None), ["south"]);
    assert_eq!(ask("true", Some("-burn")), ["north", "south"]);
}

trait Show {
    fn to_string_lossy(&self) -> String;
}

impl Show for Value {
    fn to_string_lossy(&self) -> String {
        match self {
            Value::Str(s) => s.clone(),
            other => format!("{other:?}"),
        }
    }
}

/// (the file, a part of what is refused of it)
const BAD_FILES: &[(&str, &str)] = &[
    ("[unclosed", "not TOML (line 1)"),
    ("api = \"1\"", "needs `api = 1` at the top"),
    ("api = 2", "api = 2: this J4n reads api 1 — update J4n"),
    ("api = 1\nthing = 5", "`thing` is not `[[kind]]` entries"),
    ("api = 1\n[thing]\nid = \"a\"", "`thing` is not `[[kind]]` entries"),
    ("api = 1\n[[Thing]]\nid = \"a\"", "`Thing` is not `[[kind]]` entries"),
    ("api = 1\n[[nope]]\nid = \"a\"", "no kind `nope` is declared"),
    ("api = 1\n[[thing]]\nname = \"a\"", "needs an `id`"),
    ("api = 1\n[[thing]]\nid = \"bad id\"\nname = \"x\"", "is not an id"),
    ("api = 1\n[[thing]]\nid = 5\nname = \"x\"", "is not an id"),
    ("api = 1\n[[thing]]\nid = \"a\"\nname = \"x\"\nbogus = 1", "bogus: not a field of this"),
    ("api = 1\n[[thing]]\nid = \"a\"\nname = \"x\"\nsize = \"big\"", "size: expected a whole number"),
    ("api = 1\n[[thing]]\nid = \"a\"\nname = \"x\"\nsize = 10", "size: 10 is outside 1..9"),
    (
        "api = 1\n[[thing]]\nid = \"a\"\nname = \"x\"\n[[thing]]\nid = \"a\"\nname = \"y\"",
        "declared twice in this layer",
    ),
    ("api = 1\n[[thing]]\nid = \"a\"\nname = \"x\"\nextra = 2024-01-01", "a date is not a value here"),
    ("api = 1\n[[thing]]\nid = \"a\"\nname = \"x\"\nratio = nan", "a number that is not finite"),
    (
        "api = 1\n[[thing]]\nid = \"a\"\nname = \"x\"\nextra = [[[[[[[[[[[[[[1]]]]]]]]]]]]]]",
        "nested deeper than 12 levels",
    ),
    (
        "api = 1\n[[thing]]\nid = \"a\"\nname = \"x\"\nextra = [[[[[[[[[[[[[1]]]]]]]]]]]]]",
        "nested deeper than 12 levels",
    ),
];

#[test]
fn a_value_may_nest_twelve_deep_and_not_thirteen() {
    let entry = |n: usize| {
        format!("api = 1\n[[thing]]\nid = \"a\"\nname = \"x\"\nextra = {}1{}\n", "[".repeat(n), "]".repeat(n))
    };
    let (t, problems) = load(vec![thing(), home(&[("t.toml", &entry(12))])]);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(t.ids("thing"), ["a"]);
    let (t, problems) = load(vec![thing(), home(&[("t.toml", &entry(13))])]);
    assert!(mentions(&refused(&problems), &["nested deeper than 12 levels"]), "{problems:?}");
    assert!(t.ids("thing").is_empty());
}

#[test]
fn a_file_that_cannot_be_read_is_refused_with_where_and_why_and_the_rest_still_loads() {
    for (text, want) in BAD_FILES {
        let good = "api = 1\n[[thing]]\nid = \"fine\"\nname = \"ok\"\n";
        let (t, problems) = load(vec![thing(), home(&[("bad.toml", text), ("good.toml", good)])]);
        let lines = refused(&problems);
        assert!(mentions(&lines, &[want]), "{text:?}: wanted `{want}`, got {lines:?}");
        assert!(mentions(&lines, &["home:bad.toml"]), "{text:?}: the problem names the layer and file: {lines:?}");
        assert!(t.ids("thing").contains(&"fine"), "{text:?}: a bad file or entry does not take the good ones with it");
        if !want.contains("declared twice") {
            assert!(!t.ids("thing").contains(&"a"), "{text:?}: a refused entry is not loaded");
        }
    }
}

const NO_API: &str = "[[thing]]\nid = \"a\"\nname = \"x\"\n";

#[test]
fn a_file_of_the_operators_own_layers_without_api_is_read_and_a_notice_says_it_was_assumed() {
    // the project layer is never trusted: the operator's own files are the home layer
    let own = Layer::new(LayerKind::User, "mine", vec![SourceFile { path: "t/x.toml".into(), text: NO_API.into() }]);
    let (t, problems) = Tables::load(vec![thing(), own]);
    assert_eq!(t.ids("thing"), ["a"], "the entries are taken");
    let said: Vec<String> = problems.iter().map(ToString::to_string).collect();
    assert!(mentions(&said, &["mine:t/x.toml", "has no `api`", "read as `api = 1`"]), "{said:?}");
    assert!(problems.iter().all(|p| p.level == Level::Warn), "a warning, not a refusal: {problems:?}");
}

#[test]
fn a_pack_file_without_api_is_left_out_and_the_notice_names_the_file_and_what_it_lacks() {
    for text in [NO_API, "api = \"1\"\n[[thing]]\nid = \"a\"\nname = \"x\"\n"] {
        let (t, problems) = load(vec![thing(), pack("p", text)]);
        assert!(t.ids("thing").is_empty(), "{text:?}");
        let said = refused(&problems);
        assert!(mentions(&said, &["p:", "needs `api = 1` at the top", "left out"]), "{text:?}: {said:?}");
    }
    let (_, problems) = load(vec![thing(), pack("p", NO_API)]);
    assert!(refused(&problems).iter().any(|l| l.contains("no `api`")), "{problems:?}");
}

#[test]
fn an_api_this_j4n_cannot_read_is_left_out_of_the_operators_layers_too() {
    let (t, problems) = load(vec![thing(), home(&[("old.toml", "api = 0\n[[thing]]\nid = \"a\"\nname = \"x\"\n")])]);
    assert!(t.ids("thing").is_empty());
    assert!(mentions(&refused(&problems), &["home:old.toml", "needs `api = 1` at the top", "`api = 0`"]), "{problems:?}");
}

#[test]
fn a_bad_kind_declaration_is_refused_and_its_entries_have_no_kind() {
    let decl = |body: &str| pack("p", &format!("api = 1\n[[kind]]\n{body}\n"));
    let cases: &[(&str, &str)] = &[
        ("id = \"Bad\"\ndoc = \"d\"\nfields = {}", "not a kind name"),
        ("id = \"api\"\ndoc = \"d\"\nfields = {}", "not a kind name"),
        ("id = \"a_very_long_kind_name_that_goes_on\"\ndoc = \"d\"\nfields = {}", "not a kind name"),
        ("id = \"x\"\nfields = {}", "doc: required"),
        ("id = \"x\"\ndoc = \"d\"", "fields: required"),
        ("id = \"x\"\ndoc = \"d\"\nfields = {}\nbogus = 1", "bogus: not a field of this"),
        (
            "id = \"x\"\ndoc = \"d\"\nuntrusted = \"allow\"\nfields = {}",
            "untrusted: `allow` is not one of ignore, tighten",
        ),
        ("id = \"x\"\ndoc = \"d\"\nfields = { a = \"wat\" }", "`wat` is not a type"),
        ("id = \"x\"\ndoc = \"d\"\nfields = { id = \"str\" }", "every entry has an `id`"),
        (
            "id = \"x\"\ndoc = \"d\"\nfields = { a = \"int\" }\nexamples = [{ id = \"e\", a = \"no\" }]",
            "example 1: a: expected a whole number",
        ),
        (
            "id = \"x\"\ndoc = \"d\"\nfields = { a = { ty = \"int\", required = true } }\nexamples = [{ id = \"e\" }]",
            "example 1: a: required",
        ),
        ("id = \"x\"\ndoc = \"d\"\nfields = {}\nexamples = [{ b = 1 }]", "example 1: needs an `id`"),
        ("id = \"x\"\ndoc = \"d\"\nfields = {}\nexamples = [5]", "example 1: is not a table"),
    ];
    for (body, want) in cases {
        let (t, problems) = load(vec![decl(body)]);
        assert!(mentions(&refused(&problems), &[want]), "{body:?}: wanted `{want}`, got {problems:?}");
        assert!(t.schema("x").is_none(), "{body:?}: a kind with a mistake does not exist");
    }
}

#[test]
fn a_kind_is_closed_once_declared_and_the_lowest_layer_owns_it() {
    let upper = pack("q", "api = 1\n[[kind]]\nid = \"thing\"\ndoc = \"another\"\nfields = { zzz = \"str\" }\n");
    let (t, problems) = load(vec![thing(), upper]);
    assert!(mentions(&refused(&problems), &["q:kinds.toml [[kind]] thing", "already declared by p"]), "{problems:?}");
    assert!(
        t.schema("thing").unwrap().fields.contains_key("name")
            && !t.schema("thing").unwrap().fields.contains_key("zzz")
    );
    // not even the core's own kind can be declared again
    let again = pack("q", "api = 1\n[[kind]]\nid = \"kind\"\ndoc = \"mine\"\nfields = {}\n");
    let (_, problems) = load(vec![again]);
    assert!(mentions(&refused(&problems), &["already declared by base"]), "{problems:?}");
}

#[test]
fn a_kind_that_refers_to_a_kind_nothing_declares_is_a_warning() {
    let (t, problems) = load(vec![kind_of("a", "b = \"ref:ghost\"")]);
    assert!(t.schema("a").is_some());
    assert!(mentions(&warned(&problems), &["[[kind]] a", "the kind `ghost`, which nothing declares"]), "{problems:?}");
    assert!(refused(&problems).is_empty());
}

#[test]
fn layers_merge_field_by_field_and_say_where_each_field_came_from() {
    let low = pack(
        "p",
        &format!(
            "{}\n{}",
            kind_text(),
            "[[thing]]\nid = \"a\"\nname = \"from the pack\"\nlocked = 1\ntags = [\"low\"]\n"
        ),
    );
    let (t, problems) = load(vec![
        low,
        home(&[("t.toml", "api = 1\n[[thing]]\nid = \"a\"\nsize = 5\nlocked = 2\ntags = [\"home\", \"x\"]\n")]),
        // a second layer of the operator's own above it (a project layer is never trusted)
        layer(LayerKind::User, "upper", &[("t.toml", "api = 1\n[[thing]]\nid = \"a\"\ntags = [\"upper\"]\n")]),
    ]);
    assert!(refused(&problems).is_empty(), "{problems:?}");
    let a = t.raw("thing", "a").unwrap();
    assert_eq!(a.fields["name"].as_str(), Some("from the pack"));
    assert_eq!(a.fields["size"].as_integer(), Some(5));
    assert_eq!(a.fields["locked"].as_integer(), Some(1), "a floor is the lowest layer's");
    assert_eq!(a.fields["tags"].as_array().unwrap().len(), 1, "a list is replaced whole, never merged");
    let from: Vec<(&str, &str)> = a.from.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    assert_eq!(from, [("locked", "p"), ("name", "p"), ("size", "home"), ("tags", "upper")]);
    assert!(
        mentions(&warned(&problems), &["home sets `locked`, which is a floor an earlier layer already set"]),
        "{problems:?}"
    );
}

fn kind_text() -> String {
    format!("api = 1\n[[kind]]\nid = \"thing\"\ndoc = \"a test kind\"\n[kind.fields]\n{THING}\n")
}

#[test]
fn defaults_come_last_and_a_required_field_may_be_given_by_any_layer() {
    let (t, problems) = load(vec![
        thing(),
        home(&[("t.toml", "api = 1\n[[thing]]\nid = \"a\"\nname = \"x\"\n")]),
        layer(LayerKind::User, "upper", &[("t.toml", "api = 1\n[[thing]]\nid = \"b\"\nname = \"y\"\n[[thing]]\nid = \"a\"\nsize = 9\n")]),
    ]);
    assert!(refused(&problems).is_empty(), "{problems:?}");
    let a = t.raw("thing", "a").unwrap();
    assert_eq!((a.fields["size"].as_integer(), a.from["size"].as_str()), (Some(9), "upper"));
    assert_eq!(a.from["name"], "home");
    assert_eq!(t.raw("thing", "b").unwrap().fields["size"].as_integer(), Some(3));
    assert_eq!(t.raw("thing", "b").unwrap().from["size"], "default");
}

#[test]
fn an_entry_missing_a_required_field_in_every_layer_is_refused_whole() {
    let (t, problems) = load(vec![thing(), home(&[("t.toml", "api = 1\n[[thing]]\nid = \"a\"\nsize = 4\n")])]);
    assert!(mentions(&refused(&problems), &["[[thing]] a", "name: required"]), "{problems:?}");
    assert!(t.raw("thing", "a").is_none() && t.ids("thing").is_empty());
}

#[test]
fn a_reference_to_nothing_warns_and_the_entry_still_counts() {
    let (pack, _) = lighthouse();
    let lamp = "api = 1\n[[lamp]]\nid = \"lost\"\nkeeper = \"nobody\"\n";
    let (t, problems) = load(vec![pack, home(&[("l.toml", lamp)])]);
    assert!(mentions(&warned(&problems), &["[[lamp]] lost", "keeper: no keeper `nobody`"]), "{problems:?}");
    assert!(refused(&problems).is_empty());
    assert!(t.raw("lamp", "lost").is_some());
}

#[test]
fn the_same_id_twice_across_layers_is_one_entry_and_twice_in_a_layer_is_a_mistake() {
    let a = "api = 1\n[[thing]]\nid = \"a\"\nname = \"x\"\n";
    let (t, problems) = load(vec![thing(), home(&[("one.toml", a), ("two.toml", a)])]);
    assert!(mentions(&refused(&problems), &["home:two.toml", "declared twice in this layer"]), "{problems:?}");
    assert_eq!(t.ids("thing"), ["a"]);
}

const GATES: &str = r#"
api = 1
[[kind]]
id = "plain"
doc = "a kind a stranger's project may not touch"
fields = { name = "str", power = { ty = "int", widens = true } }

[[kind]]
id = "tight"
doc = "a kind a stranger's project may only tighten"
untrusted = "tighten"
fields = { name = "str", verdict = { ty = "enum:allow|ask|block", widens = ["allow"] } }

[[tight]]
id = "rm"
verdict = "ask"
"#;

const STRANGER: &str = r#"
api = 1
[[kind]]
id = "mine"
doc = "declared by a project"
fields = {}

[[plain]]
id = "p1"
name = "p"

[[tight]]
id = "t-block"
verdict = "block"

[[tight]]
id = "t-allow"
verdict = "allow"

[[tight]]
id = "rm"
verdict = "block"

[[tight]]
id = "t-name"
name = "no verdict at all"
"#;

#[test]
fn a_project_that_is_not_trusted_may_only_tighten() {
    let (t, problems) = Tables::load(vec![pack("p", GATES), project(&[("s.toml", STRANGER)])]);
    let warns = warned(&problems);
    assert!(
        mentions(
            &warns,
            &["project:s.toml [[kind]] mine", "ignored: a project that is not trusted cannot declare a kind"]
        ),
        "{warns:?}"
    );
    assert!(
        mentions(
            &warns,
            &["[[plain]] p1", "ignored: this project is not trusted, and this kind is not one it may add to"]
        ),
        "{warns:?}"
    );
    assert!(mentions(&warns, &["[[tight]] t-allow", "verdict would widen what J4n may do"]), "{warns:?}");
    assert!(refused(&problems).is_empty(), "ignoring is a warning, not a refusal: {problems:?}");
    assert!(t.schema("mine").is_none());
    assert!(t.ids("plain").is_empty());
    assert_eq!(t.ids("tight"), ["rm", "t-block", "t-name"], "tightening entries count, the widening one does not");
    assert_eq!(
        t.raw("tight", "rm").unwrap().fields["verdict"].as_str(),
        Some("block"),
        "a stranger can raise ask to block"
    );
    assert_eq!(t.raw("tight", "rm").unwrap().from["verdict"], "project");
}

#[test]
fn only_the_project_layer_is_gated_by_trust() {
    let (t, problems) = Tables::load(vec![pack("p", GATES), home(&[("h.toml", STRANGER)])]);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(t.ids("tight").len(), 4, "the operator's own layer is the operator's");
}

#[test]
fn a_layer_hash_pins_the_files_and_their_text() {
    let a = project(&[("a.toml", "api = 1\n"), ("b.toml", "api = 1\n# x\n")]);
    let same_in_another_order = project(&[("b.toml", "api = 1\n# x\n"), ("a.toml", "api = 1\n")]);
    let edited = project(&[("a.toml", "api = 1\n"), ("b.toml", "api = 1\n# y\n")]);
    let renamed = project(&[("a.toml", "api = 1\n"), ("c.toml", "api = 1\n# x\n")]);
    assert_eq!(a.hash(), same_in_another_order.hash());
    assert_ne!(a.hash(), edited.hash());
    assert_ne!(a.hash(), renamed.hash());
    assert_eq!(a.hash().len(), 64);
}

#[test]
fn a_directory_is_read_as_a_layer_without_state_hidden_files_and_links() {
    let dir = Scratch::new("fromdir");
    let ok = "api = 1\n";
    for (rel, text) in [
        ("a.toml", ok),
        ("sub/deep/b.toml", ok),
        ("pack.toml", "id = \"x\"\n"),
        ("log/0001.toml", "not table files"),
        ("folds/x.toml", "state"),
        ("tests/case.toml", "cases"),
        ("bench/note-1700000000000/foundry.toml", "a doer's own file in a case folder"),
        ("worktrees/key/t1/Cargo.toml", "a checkout of a task"),
        (".hidden.toml", "no"),
        (".git/c.toml", "no"),
        ("notes.txt", "no"),
    ] {
        dir.put(rel, text);
    }
    std::os::unix::fs::symlink(dir.join("a.toml"), dir.join("link.toml")).unwrap();
    std::fs::write(dir.join("big.toml"), vec![b'#'; 1 << 21]).unwrap();
    std::fs::write(dir.join("latin.toml"), [0xff, 0xfe]).unwrap();

    let (pack, problems) = Layer::from_dir(LayerKind::Pack, "px", dir.path());
    let files: Vec<&str> = pack.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(files, ["a.toml", "sub/deep/b.toml"], "a pack's pack.toml is its manifest");
    let lines: Vec<String> = problems.iter().map(ToString::to_string).collect();
    assert!(mentions(&lines, &["px:link.toml", "a link is not followed"]), "{lines:?}");
    assert!(mentions(&lines, &["px:big.toml", "larger than"]), "{lines:?}");
    assert!(mentions(&lines, &["px:latin.toml", "cannot read"]), "{lines:?}");
    assert!(problems.iter().all(|p| p.level == if p.msg.contains("link") { Level::Warn } else { Level::Refused }));

    let (user, _) = Layer::from_dir(LayerKind::User, "home", dir.path());
    assert!(
        user.files.iter().any(|f| f.path == "pack.toml"),
        "outside a pack, pack.toml is a table file like any other"
    );

    let (empty, problems) = Layer::from_dir(LayerKind::User, "home", &dir.join("nothing/here"));
    assert!(empty.files.is_empty() && problems.is_empty(), "a layer that is not there is empty");
}

#[test]
fn a_session_layer_cannot_be_handed_in() {
    let (t, problems) = load(vec![layer(LayerKind::Session, "mine", &[])]);
    assert!(mentions(&warned(&problems), &["the session layer is made by the Tables"]), "{problems:?}");
    assert_eq!(t.kinds().count(), 35, "the core's own kinds");
}

#[test]
fn the_schema_of_a_loaded_kind_checks_entries_like_the_loader_does() {
    let (t, _) = load(vec![thing()]);
    let s = t.schema("thing").unwrap();
    let entry = "name = \"x\"\nsize = 20".parse().unwrap();
    assert_eq!(s.check(&entry, Mode::Whole, &|_, _| true).errors, ["size: 20 is outside 1..9"]);
}

#[test]
fn problems_read_as_one_line_with_where_first() {
    let p = Problem::refused("home:a.toml [[thing]] x", "size: nope");
    assert_eq!(p.to_string(), "home:a.toml [[thing]] x: size: nope");
    assert_eq!(Problem::warn("a", "b").level, Level::Warn);
}

#[test]
fn a_directory_that_is_hostile_is_read_only_so_far() {
    let dir = Scratch::new("hostile");
    dir.put("ok.toml", "api = 1\n");
    // a pipe named like a table file would block a reader for ever
    let made = std::process::Command::new("mkfifo").arg(dir.join("pipe.toml")).status().is_ok_and(|s| s.success());
    for n in 0..520 {
        std::fs::create_dir_all(dir.join(&format!("d{n:03}"))).unwrap();
    }
    let (layer, problems) = Layer::from_dir(LayerKind::User, "home", dir.path());
    assert_eq!(layer.files.len(), 1, "the one file there is");
    let lines: Vec<String> = problems.iter().map(ToString::to_string).collect();
    assert!(mentions(&lines, &["more than 500 directories in a layer"]), "{lines:?}");
    if made {
        assert!(mentions(&lines, &["home:pipe.toml", "not a regular file"]), "{lines:?}");
    }
}
