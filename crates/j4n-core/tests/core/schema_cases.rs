//! Schema: the written form of every type, what each accepts and refuses, and every way a field spec can be wrong.
//! Table-driven: one line per case, so a new type or a new quirk is one line.

use j4n_core::change::ActorKind;
use j4n_core::schema::{Mode, Schema, Ty, Widens};
use toml::Table;

fn schema(spec: &str) -> Result<Schema, Vec<String>> {
    let table: Table = format!("f = {spec}").parse().unwrap();
    Schema::parse(&table)
}

/// What `Schema::check` says of the entry `f = value` against the field spec; refs are known when `known` says so.
fn verdict(spec: &str, value: &str, known: bool) -> Vec<String> {
    let s = schema(spec).unwrap_or_else(|e| panic!("{spec} should parse: {e:?}"));
    let entry: Table = format!("f = {value}").parse().unwrap();
    let got = s.check(&entry, Mode::Whole, &|_, _| known);
    got.errors.into_iter().chain(got.dangling).collect()
}

/// (field spec, value, "" when accepted or a part of the refusal)
const CASES: &[(&str, &str, &str)] = &[
    // text
    (r#""str""#, r#""a line""#, ""),
    (r#""str""#, "5", "expected text, got a whole number"),
    (r#""str""#, r#""two\nlines""#, "more than one line"),
    (r#""str""#, r#""bell\u0007""#, "control character"),
    (r#""str""#, r#""tab\there""#, ""),
    (r#""text""#, r#""two\nlines\r\n""#, ""),
    (r#""text""#, r#""nul\u0000""#, "control character"),
    (r#""markdown""#, r##""# title\n\nbody""##, ""),
    // paths and patterns
    (r#""path""#, r#""src/main.rs""#, ""),
    (r#""path""#, r#""-rf""#, "not a path"),
    (r#""path""#, r#""""#, "not a path"),
    (r#""pattern""#, r#""git *""#, ""),
    (r#""pattern""#, r#""""#, "not empty"),
    (r#""argv""#, r#"["rg", "-n", "x y"]"#, ""),
    (r#""argv""#, "[]", "1 to 1000"),
    (r#""argv""#, r#"["", "x"]"#, "empty as the program"),
    (r#""argv""#, r#"["rg", 3]"#, "not text"),
    // numbers
    (r#""int""#, "5", ""),
    (r#""int""#, "5.5", "expected a whole number"),
    (r#"{ ty = "int", min = 1, max = 9 }"#, "9", ""),
    (r#"{ ty = "int", min = 1, max = 9 }"#, "10", "outside 1..9"),
    (r#"{ ty = "int", min = 1 }"#, "0", "outside 1..any"),
    (r#""num""#, "2", ""),
    (r#""num""#, "2.5", ""),
    (r#"{ ty = "num", min = 0, max = 1 }"#, "1.5", "outside 0..1"),
    (r#"{ ty = "num", min = 0.5 }"#, "0.25", "outside 0.5..any"),
    (r#"{ ty = "num", min = 0.5 }"#, "0.5", ""),
    (r#"{ ty = "num", min = 0, max = 1 }"#, "0", ""),
    (r#"{ ty = "num", min = 0, max = 1 }"#, "1", ""),
    (r#"{ ty = "num", min = 0, max = 1 }"#, "1.0", ""),
    (r#"{ ty = "num", min = 0, max = 1 }"#, "1.0000001", "outside 0..1"),
    (r#"{ ty = "int", min = 1, max = 9 }"#, "1", ""),
    (r#"{ ty = "int", min = 1, max = 9 }"#, "0", "outside 1..9"),
    (r#""num""#, r#""2""#, "expected a number"),
    (r#""bool""#, "true", ""),
    (r#""bool""#, r#""yes""#, "expected true or false"),
    // durations and money are the text an expression reads
    (r#""dur""#, r#""800ms""#, ""),
    (r#""dur""#, r#""1h30m""#, ""),
    (r#""dur""#, r#""5""#, "expected a duration"),
    (r#""dur""#, "5", "expected a duration"),
    (r#""dur""#, r#""$1""#, "expected a duration"),
    (r#""dur""#, r#""(5m)""#, "expected a duration"),
    (r#""dur""#, r#"" 5m""#, "expected a duration"),
    (r#""dur""#, r#""5m or 3""#, "expected a duration"),
    (r#""usd""#, r#""($1)""#, "expected money"),
    (r#""usd""#, r#""$1.50""#, ""),
    (r#""usd""#, r#""$0.000001""#, ""),
    (r#""usd""#, r#""1.50""#, "expected money"),
    (r#""usd""#, r#""5m""#, "expected money"),
    // words
    (r#""enum:a|b|c""#, r#""b""#, ""),
    (r#""enum:a|b|c""#, r#""d""#, "not one of a, b, c"),
    (r#"{ ty = "enum", options = ["x-1", "y.2"] }"#, r#""x-1""#, ""),
    (r#""multi:a|b|c""#, r#"["a", "c"]"#, ""),
    (r#""multi:a|b|c""#, r#"["a", "a"]"#, "listed twice"),
    (r#""multi:a|b|c""#, r#"["z"]"#, "not one of a, b, c"),
    (r#""multi:a|b|c""#, r#""a""#, "expected a list of options"),
    // containers
    (r#""list<int>""#, "[1, 2, 3]", ""),
    (r#""list<int>""#, r#"[1, "x"]"#, "f[1]: expected a whole number"),
    (r#""list<list<str>>""#, r#"[["a"], ["b", 2]]"#, "f[1][1]"),
    (r#""map<int>""#, "{ a = 1, b = 2 }", ""),
    (r#""map<int>""#, r#"{ a = "x" }"#, "f.a: expected a whole number"),
    (r#"{ ty = "list", of = { ty = "int", max = 3 } }"#, "[1, 4]", "f[1]: 4 is outside any..3"),
    (r#"{ ty = "map", of = "enum:on|off" }"#, r#"{ x = "maybe" }"#, "f.x"),
    (r#"{ ty = "obj", fields = { w = { ty = "int", required = true }, h = "int" } }"#, "{ w = 1, h = 2 }", ""),
    (r#"{ ty = "obj", fields = { w = { ty = "int", required = true } } }"#, "{ h = 2 }", "f.w: required"),
    (r#"{ ty = "obj", fields = { w = "int" } }"#, "{ w = 1, z = 2 }", "f.z: not a field of this"),
    (r#"{ ty = "obj", fields = { w = "int" } }"#, "5", "expected a table"),
    // references
    (r#""ref:model""#, r#""m1""#, ""),
    (r#""ref:model""#, r#""bad id""#, "not an id"),
    (r#""ref:model""#, "5", "expected the id of a thing"),
    (r#"{ ty = "ref", to = "model" }"#, r#""vendor/model-1.5""#, ""),
    (r#""secret""#, r#""env:API_KEY""#, ""),
    (r#""secret""#, r#""file:/run/keys/a""#, ""),
    (r#""secret""#, r#""file:~/.j4n/secrets/a""#, ""),
    (r#""secret""#, r#""sk-live-abc""#, "never the secret itself"),
    (r#""secret""#, r#""env:1BAD""#, "never the secret itself"),
    (r#""secret""#, r#""file:relative/path""#, "never the secret itself"),
    // the language
    (r#""expr""#, r#""a > 1 and not b""#, ""),
    (r#""expr""#, r#""a >""#, "found the end"),
    (r#"{ ty = "expr", roots = ["a"] }"#, r#""a > 1""#, ""),
    (r#"{ ty = "expr", roots = ["a"] }"#, r#""b > 1""#, "b"),
    (r#"{ ty = "expr", roots = [] }"#, r#""a > 1""#, "there are none"),
    (r#""expr""#, "5", "expected text"),
    (r#""fmt""#, r#""{cost:usd} of {cap:usd}""#, ""),
    (r#""fmt""#, r#""{cost:bogus}""#, "bogus"),
    (r#"{ ty = "fmt", roots = ["cost"] }"#, r#""{other:n}""#, "other"),
    (r#""query""#, r#"{ from = "tasks", filter = "state == 'done'", order = "-cost", limit = 5 }"#, ""),
    (r#""query""#, r#"{ from = "nowhere" }"#, "from"),
    (r#""query""#, r#"{ filter = "a" }"#, "`from` is needed"),
    (r#""query""#, r#"{ from = "tasks", limit = 0 }"#, "limit"),
    (r#""query""#, r#"{ from = "tasks", limit = -1 }"#, "`limit` is a whole number"),
    (r#""query""#, r#"{ from = "tasks", sort = "x" }"#, "`sort` is not part of a query"),
    (r#""query""#, r#"{ from = "tasks", filter = "a ==" }"#, "filter"),
    (r#""query""#, r#""tasks""#, "expected a table"),
    (r#"{ ty = "query", roots = ["fold"] }"#, r#"{ from = "tasks", filter = "set.x" }"#, "set"),
    (r#""any""#, r#"{ a = [1, { b = "c" }] }"#, ""),
    (r#""fields""#, r#"{ a = "int", b = { ty = "str", required = true } }"#, ""),
    (r#""fields""#, r#"{ a = "wat" }"#, "`wat` is not a type"),
    (r#""fields""#, "5", "expected a table of fields"),
];

#[test]
fn every_type_accepts_what_it_holds_and_refuses_the_rest() {
    for (spec, value, want) in CASES {
        let got = verdict(spec, value, true);
        if want.is_empty() {
            assert!(got.is_empty(), "{spec} should hold {value}, got {got:?}");
        } else {
            assert!(got.iter().any(|g| g.contains(want)), "{spec} against {value}: wanted `{want}`, got {got:?}");
        }
    }
}

/// (spec, a part of the refusal): a field spec that does not mean anything.
const BAD_SPECS: &[(&str, &str)] = &[
    (r#""wat""#, "`wat` is not a type"),
    ("5", "a type is a word or a table"),
    (r#"{ required = true }"#, "needs `ty`"),
    (r#"{ ty = "int", bogus = 1 }"#, "`bogus` is not a field key"),
    (r#"{ ty = "str", min = 1 }"#, "`min` does not apply to `str`"),
    (r#"{ ty = "int", min = 5, max = 1 }"#, "`min` is above `max`"),
    (r#"{ ty = "int", min = 1.5 }"#, "not a whole number"),
    (r#"{ ty = "enum" }"#, "needs `options`"),
    (r#""enum""#, "needs `options`"),
    (r#"{ ty = "enum", options = [] }"#, "options are empty"),
    (r#"{ ty = "enum", options = ["a", "a"] }"#, "listed twice"),
    (r#"{ ty = "enum", options = ["a b"] }"#, "not an option word"),
    (r#"{ ty = "enum", options = [1] }"#, "not text"),
    (r#"{ ty = "list" }"#, "needs `of`"),
    (r#"{ ty = "obj" }"#, "needs `fields`"),
    (r#"{ ty = "ref" }"#, "needs `to`"),
    (r#""ref:Bad Kind""#, "not a kind name"),
    (r#"{ ty = "list", of = { ty = "str", required = true } }"#, "belongs to a field, not to the type of its items"),
    (r#"{ ty = "int", required = true, default = 3 }"#, "a required field has no default"),
    (r#"{ ty = "int", default = "x" }"#, "the default of f"),
    (r#"{ ty = "int", min = 1, default = 0 }"#, "the default of f"),
    (r#"{ ty = "str", class = "loud" }"#, "`class` is public, project, private or secret"),
    (r#"{ ty = "secret", class = "public" }"#, "a secret is always class secret"),
    (r#"{ ty = "str", who_may = ["robot"] }"#, "`who_may` lists"),
    (r#"{ ty = "str", who_may = [] }"#, "`who_may` lists"),
    (r#"{ ty = "str", widens = ["x"] }"#, "only an enum or multi names the words"),
    (r#"{ ty = "enum:a|b", widens = ["c"] }"#, "not an option"),
    (r#"{ ty = "str", floor = "yes" }"#, "true or false"),
    (r#"{ ty = "str", doc = 5 }"#, "`doc` is text"),
    (r#""list<list<list<list<list<int>>>>>""#, "nest at most 4"),
    (
        r#"{ ty = "obj", fields = { a = { ty = "obj", fields = { b = { ty = "obj", fields = { c = { ty = "obj", fields = { d = { ty = "obj", fields = { e = "int" } } } } } } } } } }"#,
        "nest at most 4",
    ),
    (r#"{ ty = "obj", fields = { Bad = "int" } }"#, "not a field name"),
];

#[test]
fn a_field_spec_that_means_nothing_is_refused_with_why() {
    for (spec, want) in BAD_SPECS {
        let err = schema(spec).expect_err(spec).join("; ");
        assert!(err.contains(want), "{spec}: wanted `{want}`, got `{err}`");
    }
}

#[test]
fn field_names_are_checked_and_id_is_not_declarable() {
    for (name, want) in [
        ("Upper", "not a field name"),
        ("with-dash", "not a field name"),
        ("1st", "not a field name"),
        ("id", "every entry has an `id`"),
    ] {
        let table: Table = format!("{name} = \"str\"").parse().unwrap();
        let err = Schema::parse(&table).unwrap_err().join("; ");
        assert!(err.contains(want), "{name}: {err}");
    }
    let table: Table = r#"a = "wat"
b = "nope"
c = "str""#
        .parse()
        .unwrap();
    let err = Schema::parse(&table).unwrap_err();
    assert_eq!(err.len(), 2, "every field's first problem is reported, not only the first field's: {err:?}");
}

#[test]
fn a_reference_that_points_at_nothing_is_a_warning_and_a_good_one_is_clean() {
    assert!(verdict(r#""ref:model""#, r#""m1""#, true).is_empty());
    let got = verdict(r#""ref:model""#, r#""m1""#, false);
    assert_eq!(got, ["f: no model `m1`"]);
    let s = schema(r#""list<ref:model>""#).unwrap();
    let entry: Table = r#"f = ["a", "b"]"#.parse().unwrap();
    let got = s.check(&entry, Mode::Whole, &|_, id| id == "a");
    assert!(got.errors.is_empty() && got.dangling == ["f[1]: no model `b`"], "{got:?}");
}

#[test]
fn partial_checks_do_not_need_required_fields_and_whole_checks_do() {
    let s = schema(r#"{ ty = "obj", required = true, fields = { w = { ty = "int", required = true } } }"#).unwrap();
    let empty = Table::new();
    assert!(s.check(&empty, Mode::Partial, &|_, _| true).errors.is_empty());
    assert_eq!(s.check(&empty, Mode::Whole, &|_, _| true).errors, ["f: required"]);
    // a table that is there is whole in itself: what an obj requires is required even in a partial entry
    let partial: Table = "f = {}".parse().unwrap();
    assert_eq!(s.check(&partial, Mode::Partial, &|_, _| true).errors, ["f.w: required"]);
}

#[test]
fn defaults_fill_what_is_missing_also_inside_a_table_that_is_there() {
    let spec: Table = r#"
a = { ty = "int", default = 3 }
b = { ty = "obj", fields = { x = { ty = "str", default = "hi" }, y = "int" } }
c = { ty = "obj", fields = { z = { ty = "int", default = 1 } } }
"#
    .parse()
    .unwrap();
    let s = Schema::parse(&spec).unwrap();
    let mut entry: Table = "b = { y = 2 }".parse().unwrap();
    s.fill(&mut entry);
    let want: Table = r#"a = 3
b = { x = "hi", y = 2 }"#
        .parse()
        .unwrap();
    assert_eq!(entry, want, "a default is put where the table is, not made up where it is not");
}

#[test]
fn the_flags_are_read_and_the_default_writer_is_the_operator_and_the_core() {
    let s = schema(
        r#"{ ty = "enum:a|b|c", required = true, doc = "pick", class = "private", floor = true, who_may = ["operator", "rule"], widens = ["c"] }"#,
    )
    .unwrap();
    let f = &s.fields["f"];
    assert_eq!(f.ty, Ty::Enum(vec!["a".into(), "b".into(), "c".into()]));
    assert!(f.required && f.floor);
    assert_eq!(f.doc, "pick");
    assert_eq!(f.widens, Widens::Options(vec!["c".into()]));
    assert!(f.may_write(ActorKind::Operator) && f.may_write(ActorKind::Rule));
    assert!(!f.may_write(ActorKind::Model) && !f.may_write(ActorKind::Core));
    let plain = schema(r#""str""#).unwrap();
    for (kind, may) in [
        (ActorKind::Operator, true),
        (ActorKind::Core, true),
        (ActorKind::Rule, false),
        (ActorKind::Doer, false),
        (ActorKind::Model, false),
        (ActorKind::Pack, false),
    ] {
        assert_eq!(plain.may_write("f", kind), may, "{kind:?}");
    }
    assert!(!plain.may_write("nope", ActorKind::Operator), "a field the schema does not have is not writable");
    assert_eq!(schema(r#"{ ty = "secret" }"#).unwrap().fields["f"].class.word(), "secret");
}

#[test]
fn widening_names_the_fields_whose_value_widens() {
    let s = schema_of(
        r#"
mode = { ty = "enum:strict|loose|off", widens = ["loose", "off"] }
cap = { ty = "num", widens = true }
tags = { ty = "multi:a|b|c", widens = ["c"] }
inner = { ty = "obj", fields = { open = { ty = "bool", widens = true }, name = "str" } }
plain = "str"
"#,
    );
    let wide = |t: &str| s.widening(&t.parse::<Table>().unwrap());
    assert!(wide(r#"mode = "strict""#).is_empty());
    assert_eq!(wide(r#"mode = "loose""#), ["mode"]);
    assert_eq!(wide("cap = 1"), ["cap"]);
    assert!(wide(r#"tags = ["a", "b"]"#).is_empty());
    assert_eq!(wide(r#"tags = ["a", "c"]"#), ["tags"]);
    assert_eq!(wide(r#"inner = { open = false, name = "x" }"#), ["inner.open"]);
    assert!(wide(r#"plain = "x""#).is_empty());
    assert_eq!(wide("cap = 1\nmode = \"off\""), ["cap", "mode"]);
}

fn schema_of(spec: &str) -> Schema {
    Schema::parse(&spec.parse::<Table>().unwrap()).unwrap()
}

#[test]
fn a_value_in_the_language_of_expressions_keeps_durations_and_money_exact() {
    use j4n_core::value::Value;
    let s = schema_of(
        r#"
every = "dur"
cost = "usd"
n = "int"
ratio = "num"
tags = "list<str>"
waits = "map<dur>"
inner = { ty = "obj", fields = { cap = "usd" } }
loose = "any"
"#,
    );
    let entry: Table = r#"
every = "1h30m"
cost = "$1.50"
n = 3
ratio = 0.5
tags = ["a", "b"]
waits = { a = "5m" }
inner = { cap = "$2" }
loose = { x = [1, "y"] }
"#
    .parse()
    .unwrap();
    let Value::Map(row) = s.value(&entry) else { panic!("a map") };
    assert_eq!(row["every"], Value::Dur(90 * 60_000));
    assert_eq!(row["cost"], Value::Usd(1_500_000));
    assert_eq!(row["n"], Value::Num(3.0));
    assert_eq!(row["ratio"], Value::Num(0.5));
    assert_eq!(row["tags"], Value::List(vec!["a".into(), "b".into()]));
    assert_eq!(row["waits"], Value::Map([("a".to_string(), Value::Dur(300_000))].into()));
    assert_eq!(row["inner"], Value::Map([("cap".to_string(), Value::Usd(2_000_000))].into()));
    assert_eq!(row["loose"], Value::Map([("x".to_string(), Value::List(vec![Value::Num(1.0), "y".into()]))].into()));
}

#[test]
fn a_field_is_required_only_when_it_says_so() {
    let optional = schema(r#"{ ty = "int", required = false }"#).unwrap();
    assert!(!optional.fields["f"].required);
    assert!(optional.check(&Table::new(), Mode::Whole, &|_, _| true).errors.is_empty());
    let required = schema(r#"{ ty = "int", required = true }"#).unwrap();
    assert!(required.fields["f"].required);
    assert_eq!(required.check(&Table::new(), Mode::Whole, &|_, _| true).errors, ["f: required"]);
    let err = schema(r#"{ ty = "int", required = 1 }"#).unwrap_err().join("; ");
    assert!(err.contains("true or false"), "{err}");
}
