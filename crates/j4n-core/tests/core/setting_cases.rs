//! Settings (the kind `setting`): defaults, a layer's value, what does not fit, and the tree a rule reads.

use crate::scripted::base_tables;
use crate::world::*;
use j4n_core::setting::Settings;
use j4n_core::value::{Key, Value};

fn load_one(body: &str) -> (Settings, String) {
    let (t, problems) = load(vec![pack("p", &format!("api = 1\n{body}"))]);
    let (s, more) = Settings::load(&t);
    (s, problems.iter().chain(&more).map(ToString::to_string).collect::<Vec<_>>().join(" | "))
}

fn at(tree: &Value, path: &[&str]) -> Value {
    tree.at(&path.iter().map(|p| Key::Name((*p).into())).collect::<Vec<_>>())
}

#[test]
fn the_base_packs_settings_have_their_defaults_and_a_layer_changes_one_value() {
    let (s, problems) = Settings::load(&base_tables(""));
    assert!(problems.is_empty(), "{problems:?}");
    let tree = s.tree();
    assert_eq!(at(&tree, &["loop", "followups"]), Value::num(3.0));
    assert_eq!(at(&tree, &["compact", "at"]), Value::num(0.7));
    assert_eq!(at(&tree, &["cool", "max"]), Value::Dur(6 * 3_600_000));
    let (s, problems) = Settings::load(&base_tables("[[setting]]\nid = \"loop.followups\"\nvalue = 7\n"));
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(s.get("loop.followups"), Some(&Value::num(7.0)));
    assert_eq!(s.get("compact.max"), Some(&Value::num(3.0)), "the others stand");
}

#[test]
fn each_type_reads_its_own_kind_of_value_and_the_rest_are_refused() {
    let (s, said) = load_one(
        r#"
[[setting]]
id = "a.count"
ty = "int"
default = 2
[[setting]]
id = "a.ratio"
ty = "num"
default = 1
[[setting]]
id = "a.on"
ty = "bool"
default = true
[[setting]]
id = "a.wait"
ty = "dur"
default = "90s"
[[setting]]
id = "a.name"
ty = "str"
default = "x"
"#,
    );
    assert_eq!(said, "");
    assert_eq!(s.num("a.count", 0.0), 2.0);
    assert_eq!(s.num("a.ratio", 0.0), 1.0, "a whole number is a number");
    assert_eq!(s.get("a.on"), Some(&Value::Bool(true)));
    assert_eq!(s.dur_ms("a.wait", 0), 90_000);
    assert_eq!(s.get("a.name"), Some(&Value::text("x")));
    assert_eq!((s.word("a.name", "or"), s.word("a.count", "or"), s.word("nothing", "or")), ("x", "or", "or"), "words are for a setting that holds words");
    assert_eq!((s.num("a.wait", -1.0), s.dur_ms("a.count", 7), s.num("nothing", 4.0)), (-1.0, 7, 4.0), "a setting of another type, or none, is the fallback");
}

#[test]
fn a_default_that_does_not_fit_leaves_the_setting_out_and_a_value_that_does_not_leaves_the_default() {
    for (ty, default) in [("int", "\"two\""), ("int", "2.5"), ("bool", "1"), ("dur", "\"soon\""), ("dur", "5"), ("str", "3"), ("num", "\"x\"")] {
        let (s, said) = load_one(&format!("[[setting]]\nid = \"a.b\"\nty = \"{ty}\"\ndefault = {default}\n"));
        assert!(s.get("a.b").is_none() && said.contains("its default does not fit"), "{ty} {default}: {said}");
    }
    let (s, said) = load_one("[[setting]]\nid = \"a.b\"\nty = \"int\"\ndefault = 2\nmin = 1\nmax = 5\nvalue = 9\n");
    assert!(said.contains("is above 5") && said.contains("the default stands"), "{said}");
    assert_eq!(s.num("a.b", 0.0), 2.0);
    let (s, said) = load_one("[[setting]]\nid = \"a.b\"\nty = \"int\"\ndefault = 0\nmin = 1\n");
    assert!(s.get("a.b").is_none() && said.contains("is below 1"), "a default outside the bounds: {said}");
    let (s, said) = load_one("[[setting]]\nid = \"a.b\"\nty = \"int\"\ndefault = 2\nvalue = \"many\"\n");
    assert!(said.contains("is not Int") && s.num("a.b", 0.0) == 2.0, "{said}");
}

#[test]
fn ids_are_paths_and_one_cannot_sit_inside_another() {
    let (s, said) = load_one(
        r#"
[[setting]]
id = "a"
ty = "int"
default = 1
[[setting]]
id = "a.b"
ty = "int"
default = 2
[[setting]]
id = "c..d"
ty = "int"
default = 3
[[setting]]
id = "e.f"
ty = "int"
default = 4
[[setting]]
id = "e.g.h"
ty = "int"
default = 5
"#,
    );
    assert!(said.contains("part of this one") && said.contains("single dots"), "{said}");
    assert_eq!((s.get("a").is_some(), s.get("a.b").is_some(), s.get("c..d").is_some()), (true, false, false));
    let tree = s.tree();
    assert_eq!(at(&tree, &["e", "f"]), Value::num(4.0));
    assert_eq!(at(&tree, &["e", "g", "h"]), Value::num(5.0));
    assert_eq!(at(&tree, &["e", "nothing"]), Value::Missing);
}

const PACK: &str = r#"api = 1
[[setting]]
id = "compact.at"
ty = "num"
default = 0.7
min = 0.1
max = 1.0
[[setting]]
id = "compact.max"
ty = "int"
default = 3
"#;

/// A pack that defines two settings, and the operator's own file in the home layer.
fn with_home(text: &str) -> (Settings, String) {
    let (t, problems) = load(vec![pack("p", PACK), home(&[("settings.toml", &format!("api = 1\n{text}"))])]);
    let (s, more) = Settings::load(&t);
    (s, problems.iter().chain(&more).map(ToString::to_string).collect::<Vec<_>>().join(" | "))
}

#[test]
fn a_home_row_with_only_a_value_overrides_a_setting_a_pack_defines() {
    let (s, said) = with_home("[[setting]]\nid = \"compact.at\"\nvalue = 0.1\n");
    assert_eq!(said, "");
    assert_eq!(s.num("compact.at", 0.7), 0.1);
    assert_eq!(s.num("compact.max", 0.0), 3.0, "the others stand");
}

#[test]
fn a_home_row_with_a_value_that_does_not_fit_leaves_the_default_and_a_notice_names_the_file_and_the_id() {
    for (value, why) in [("\"x\"", "string is not Num"), ("0.05", "0.05 is below 0.1"), ("true", "boolean is not Num")] {
        let (s, said) = with_home(&format!("[[setting]]\nid = \"compact.at\"\nvalue = {value}\n"));
        assert_eq!(s.num("compact.at", -1.0), 0.7, "the default stands: {said}");
        assert!(said.contains("home:settings.toml") && said.contains("compact.at") && said.contains(why), "{value}: {said}");
    }
}

#[test]
fn a_home_row_with_only_a_value_for_an_id_no_layer_defines_is_refused_with_a_notice() {
    let (s, said) = with_home("[[setting]]\nid = \"compact.nope\"\nvalue = 1\n");
    assert!(s.get("compact.nope").is_none());
    assert!(said.contains("home:settings.toml") && said.contains("compact.nope") && said.contains("no layer defines it"), "{said}");
    assert_eq!(s.num("compact.at", 0.0), 0.7, "the others stand");
}
