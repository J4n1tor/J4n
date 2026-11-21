//! Law 4, one table: every kind's Rust struct and its TOML schema say the same. [`agree`] is the harness every core kind
//! passes through (each step that adds a core kind adds one line); here it runs on the kind of kinds and on two kinds of
//! the lighthouse pack, and it is shown to catch a struct that has drifted from its schema.

use crate::world::*;
use j4n_core::schema::Mode;
use j4n_core::table::{Kind, KindDecl, Tables};
use serde::Deserialize;
use std::collections::BTreeMap;
use toml::{Table, Value};

/// Same verdict on `entry` (with its id), the schema's and the struct's, after the defaults are put in.
fn verdicts<T: Kind>(tables: &Tables, entry: &Table) -> (Result<(), String>, Result<(), String>) {
    let schema = tables.schema(T::NAME).expect("a declared kind");
    let mut fields = entry.clone();
    let id = fields.remove("id").expect("an example has an id");
    schema.fill(&mut fields);
    let checked = schema.check(&fields, Mode::Whole, &|_, _| true);
    let by_schema = if checked.errors.is_empty() { Ok(()) } else { Err(checked.errors.join("; ")) };
    fields.insert("id".into(), id);
    let by_struct = Value::Table(fields).try_into::<T>().map(|_| ()).map_err(|e| e.message().to_string());
    (by_schema, by_struct)
}

/// A value of another type than `v`, for the mutations that must be refused by both.
fn other_type(v: &Value) -> Value {
    match v {
        Value::String(_) | Value::Datetime(_) => Value::Integer(7),
        Value::Table(_) | Value::Boolean(_) | Value::Float(_) | Value::Integer(_) => Value::String("x".into()),
        Value::Array(_) => Value::Integer(7),
    }
}

/// Every example of the kind, and every small mutation of it (an unknown key; each field in the wrong type; each field
/// gone), is read by the schema and by the struct; where they differ it is reported. The number of readings if they agree.
pub fn agree<T: Kind>(tables: &Tables) -> Result<usize, Vec<String>> {
    let decl: KindDecl = tables.get(T::NAME).map_err(|e| vec![e.to_string()])?;
    let (mut readings, mut found) = (0, Vec::new());
    assert!(!decl.examples.is_empty(), "a core kind carries examples: that is what the law reads");
    for (n, example) in decl.examples.iter().enumerate() {
        let entry = example.as_table().expect("an example is a table").clone();
        let mut cases = vec![(format!("example {}", n + 1), entry.clone())];
        let mut with_extra = entry.clone();
        with_extra.insert("zz_unknown".into(), Value::Integer(1));
        cases.push((format!("example {} with an unknown key", n + 1), with_extra));
        for (name, value) in entry.iter().filter(|(k, _)| *k != "id") {
            let mut wrong = entry.clone();
            wrong.insert(name.clone(), other_type(value));
            cases.push((format!("example {}, `{name}` as {}", n + 1, other_type(value).type_str()), wrong));
            let mut gone = entry.clone();
            gone.remove(name);
            cases.push((format!("example {} without `{name}`", n + 1), gone));
        }
        for (what, case) in cases {
            readings += 1;
            let (schema, rust) = verdicts::<T>(tables, &case);
            if schema.is_ok() != rust.is_ok() {
                found.push(format!("{what}: the schema says {schema:?}, the struct says {rust:?}"));
            } else if what == format!("example {}", n + 1) && schema.is_err() {
                found.push(format!("{what}: neither reads its own example: {schema:?}"));
            }
        }
    }
    if found.is_empty() { Ok(readings) } else { Err(found) }
}

#[test]
fn the_kind_of_kinds_reads_the_same_in_the_schema_and_in_the_struct() {
    let (t, problems) = load(vec![]);
    assert!(problems.is_empty());
    let readings = agree::<KindDecl>(&t).unwrap();
    assert_eq!(readings, 14, "two examples: each as it is, with an unknown key, and each of its fields wrong and gone");
}

#[test]
fn the_kinds_the_log_adds_read_the_same_in_the_schema_and_in_the_struct() {
    let (t, problems) = load(vec![]);
    assert!(problems.is_empty());
    assert_eq!(agree::<j4n_core::fold::FoldDecl>(&t).unwrap(), 42, "fold");
    assert_eq!(agree::<j4n_core::log::EventDecl>(&t).unwrap(), 14, "event");
}

#[test]
fn the_kinds_the_loop_reads_read_the_same_in_the_schema_and_in_the_struct() {
    let (t, problems) = load(vec![]);
    assert!(problems.is_empty());
    assert_eq!(agree::<j4n_core::rule::RuleRow>(&t).unwrap(), 46, "rule");
    assert_eq!(agree::<j4n_core::rule::IncidentRow>(&t).unwrap(), 18, "incident");
    assert_eq!(agree::<j4n_core::setting::SettingRow>(&t).unwrap(), 20, "setting");
    assert_eq!(agree::<j4n_core::failure::FailureRow>(&t).unwrap(), 22, "failure");
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Keeper {
    #[allow(dead_code)] // never read: it is deserialized to see that it agrees with the schema
    id: String,
    name: String,
    years: i64,
    shift: String,
}

impl Kind for Keeper {
    const NAME: &'static str = "keeper";
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)] // fields are never read: the struct is only deserialized to see that it agrees with the schema
struct Beam {
    reach: i64,
    tilt: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)] // fields are never read: the struct is only deserialized to see that it agrees with the schema
struct Lamp {
    id: String,
    keeper: String,
    oil: String,
    burn: String,
    colors: Vec<String>,
    mode: String,
    brightness: f64,
    rule: i64,
    flash_when: Option<String>,
    label: Option<String>,
    beam: Option<Beam>,
    tags: Option<Vec<String>>,
    notes: Option<BTreeMap<String, String>>,
}

impl Kind for Lamp {
    const NAME: &'static str = "lamp";
}

#[test]
fn a_pack_kind_with_a_struct_reads_the_same_both_ways_and_get_gives_the_struct() {
    let (pack, _) = lighthouse();
    let (t, problems) = load(vec![pack]);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(agree::<Keeper>(&t).unwrap(), 12, "the first example has three fields, the second only a name");
    assert_eq!(agree::<Lamp>(&t).unwrap(), 14);
    let ada: Keeper = t.get("ada").unwrap();
    assert_eq!((ada.name.as_str(), ada.years, ada.shift.as_str()), ("Ada Quill", 12, "dawn"));
    let bo: Keeper = t.get("bo").unwrap();
    assert_eq!((bo.years, bo.shift.as_str()), (0, "night"), "the defaults are in before the struct reads");
    assert_eq!(t.list::<Keeper>().unwrap().len(), 2);
    let north: Lamp = t.get("north").unwrap();
    assert_eq!((north.oil.as_str(), north.rule, north.brightness, north.colors.len()), ("$0.40", 7, 0.5, 2));
    assert!(t.get::<Lamp>("nothing").unwrap_err().to_string().contains("there is no such thing"));
}

/// The struct has lost a field the schema still has.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)] // fields are never read: the struct is only deserialized to see that it agrees with the schema
struct StaleKeeper {
    id: String,
    name: String,
    years: i64,
}

impl Kind for StaleKeeper {
    const NAME: &'static str = "keeper";
}

/// The struct reads a field as text that the schema says is a number.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)] // fields are never read: the struct is only deserialized to see that it agrees with the schema
struct LooseKeeper {
    id: String,
    name: String,
    years: String,
    shift: String,
}

impl Kind for LooseKeeper {
    const NAME: &'static str = "keeper";
}

#[test]
fn a_struct_that_drifted_from_its_schema_is_caught() {
    let (pack, _) = lighthouse();
    let (t, _) = load(vec![pack]);
    let found = agree::<StaleKeeper>(&t).unwrap_err().join("\n");
    assert!(found.contains("shift") || found.contains("example 1"), "{found}");
    let found = agree::<LooseKeeper>(&t).unwrap_err().join("\n");
    assert!(found.contains("`years`"), "{found}");
}

#[test]
fn a_kind_without_examples_is_not_a_kind_the_law_can_read() {
    let (t, _) = load(vec![kind_of("bare", "a = \"int\"")]);
    #[derive(Deserialize)]
    struct Bare;
    impl Kind for Bare {
        const NAME: &'static str = "bare";
    }
    let result = std::panic::catch_unwind(|| agree::<Bare>(&t));
    assert!(result.is_err(), "agree refuses a kind that has no examples");
}
