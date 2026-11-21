//! Table under attack and under load: random values against every type, damaged files, and long random runs of writes
//! and undos against a model of what the layer must hold. Deterministic (a fixed generator), so a failure repeats.

use crate::world::*;
use j4n_core::change::{Actor, Where};
use j4n_core::schema::{Mode, Schema};
use j4n_core::table::{KindDecl, Layer, LayerKind, Op, Tables, Write};
use j4n_core::write::{NoJournal, Writer};
use std::collections::BTreeMap;
use std::path::Path;
use toml::{Table, Value};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

fn value(rng: &mut Rng, depth: usize) -> Value {
    match rng.below(if depth >= 3 { 5 } else { 7 }) {
        0 => Value::Integer(rng.below(20) as i64 - 5),
        1 => Value::Float(rng.below(1000) as f64 / 8.0),
        2 => Value::Boolean(rng.below(2) == 0),
        3 => Value::String(
            rng.pick(&[
                "",
                "a",
                "x y",
                "800ms",
                "$1.50",
                "env:K",
                "a > 1",
                "{a:usd}",
                "tasks",
                "-x",
                "two\nlines",
                "é",
            ])
            .to_string(),
        ),
        4 => Value::String(rng.pick(&["1h30m", "file:/x", "git *", "src/main.rs", "m1"]).to_string()),
        5 => Value::Array((0..rng.below(4)).map(|_| value(rng, depth + 1)).collect()),
        _ => Value::Table((0..rng.below(4)).map(|i| (format!("k{i}"), value(rng, depth + 1))).collect()),
    }
}

const SPECS: &[&str] = &[
    r#""str""#,
    r#""text""#,
    r#""markdown""#,
    r#""path""#,
    r#""pattern""#,
    r#""argv""#,
    r#""bool""#,
    r#""dur""#,
    r#""usd""#,
    r#""secret""#,
    r#""any""#,
    r#""fields""#,
    r#""int""#,
    r#""num""#,
    r#""expr""#,
    r#""fmt""#,
    r#""query""#,
    r#""enum:a|b""#,
    r#""multi:a|b""#,
    r#""list<str>""#,
    r#""map<int>""#,
    r#""ref:m""#,
    r#"{ ty = "obj", fields = { a = "int", b = { ty = "str", required = true } } }"#,
    r#"{ ty = "list", of = { ty = "obj", fields = { x = "dur" } } }"#,
    r#"{ ty = "int", min = 0, max = 9 }"#,
    r#"{ ty = "expr", roots = ["a"] }"#,
];

#[test]
fn any_value_against_any_type_is_judged_without_a_panic_and_a_default_always_fits() {
    let mut rng = Rng(7);
    for round in 0..4000 {
        let spec = *rng.pick(SPECS);
        let schema = Schema::parse(&format!("f = {spec}").parse::<Table>().unwrap()).unwrap();
        let mut entry = Table::new();
        if rng.below(5) != 0 {
            entry.insert("f".into(), value(&mut rng, 0));
        }
        if rng.below(4) == 0 {
            entry.insert("extra".into(), value(&mut rng, 0));
        }
        let partial = schema.check(&entry, Mode::Partial, &|_, _| round % 2 == 0);
        let whole = schema.check(&entry, Mode::Whole, &|_, _| true);
        assert!(whole.errors.len() >= partial.errors.len(), "{spec} {entry:?}");
        let _ = schema.value(&entry);
        let _ = schema.widening(&entry);
        let mut filled = entry.clone();
        schema.fill(&mut filled);
        assert!(filled.len() >= entry.len());
    }
}

#[test]
fn a_value_that_passes_is_kept_by_the_typed_reading() {
    let mut rng = Rng(11);
    for _ in 0..3000 {
        let spec = *rng.pick(SPECS);
        let schema = Schema::parse(&format!("f = {spec}").parse::<Table>().unwrap()).unwrap();
        let mut entry = Table::new();
        entry.insert("f".into(), value(&mut rng, 0));
        if schema.check(&entry, Mode::Whole, &|_, _| true).errors.is_empty() {
            let j4n_core::value::Value::Map(row) = schema.value(&entry) else { panic!("a map") };
            assert!(
                !matches!(row["f"], j4n_core::value::Value::Missing) || spec.contains("dur") || spec.contains("usd"),
                "{spec} {entry:?}"
            );
        }
    }
}

/// Damage a text: a few random deletions, insertions, swaps, or a cut.
fn damage(rng: &mut Rng, text: &str) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    for _ in 0..1 + rng.below(4) {
        if chars.is_empty() {
            break;
        }
        let at = rng.below(chars.len());
        match rng.below(5) {
            0 => {
                chars.remove(at);
            }
            1 => chars.insert(at, *rng.pick(&['[', ']', '"', '=', '\n', '{', '}', '#', ',', '.', '\\', '\''])),
            2 => {
                let other = rng.below(chars.len());
                chars.swap(at, other);
            }
            3 => chars.truncate(at),
            _ => {
                let to = (at + rng.below(30)).min(chars.len());
                chars.drain(at..to);
            }
        }
    }
    chars.into_iter().collect()
}

#[test]
fn damaged_files_never_panic_and_every_problem_says_where() {
    let (pack, _) = lighthouse();
    let mut rng = Rng(3);
    for round in 0..600 {
        let mut layers = vec![];
        let mut p = pack.clone();
        let victim = rng.below(p.files.len());
        p.files[victim].text = damage(&mut rng, &p.files[victim].text);
        layers.push(p);
        if round % 3 == 0 {
            let text = damage(&mut rng, &pack.files[1].text);
            layers.push(project(&[("p.toml", &text)]));
        }
        let (t, problems) = j4n_core::table::Tables::load(layers);
        for p in &problems {
            assert!(!p.at.is_empty() && !p.msg.is_empty(), "{p:?}");
        }
        for (kind, _) in t.kinds() {
            for id in t.ids(kind) {
                assert!(t.raw(kind, id).is_some());
            }
            let _ = t.rows(kind);
        }
    }
}

#[test]
fn the_repositorys_own_packs_load_clean() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs");
    let mut layers = Vec::new();
    for entry in std::fs::read_dir(&root).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "base" {
            continue; // its kinds are the core's own, embedded; the disk copy is compared below
        }
        let (layer, read) = Layer::from_dir(LayerKind::Pack, &name, &entry.path());
        assert!(read.is_empty(), "{name}: {read:?}");
        layers.push(layer);
    }
    assert!(layers.len() >= 7, "the built-in packs are there");
    let (_, problems) = load(layers);
    assert!(problems.is_empty(), "{problems:?}");
}

#[test]
fn the_core_kinds_on_disk_are_the_ones_in_the_binary() {
    let (t, _) = load(vec![]);
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/base/kinds/kind.toml"))
        .unwrap();
    let file: Table = text.parse().unwrap();
    let mut on_disk = file["kind"][0].as_table().unwrap().clone();
    on_disk.remove("id");
    let loaded: KindDecl = t.get("kind").unwrap();
    assert_eq!(Value::Table(loaded.fields), on_disk["fields"]);
    assert_eq!(loaded.examples, on_disk["examples"].as_array().unwrap().clone());
    let (base, read) =
        Layer::from_dir(LayerKind::Pack, "base", &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/base"));
    assert!(read.is_empty() && base.files.iter().any(|f| f.path == "kinds/kind.toml"));
}

const KINDS: &str = r#"
api = 1
[[kind]]
id = "thing"
doc = "d"
[kind.fields]
name = { ty = "str", required = true }
size = { ty = "int", min = 1, max = 9 }
tags = "list<str>"
meta = "any"
every = "dur"
"#;

fn random_set(rng: &mut Rng) -> Table {
    let mut t = Table::new();
    for _ in 0..1 + rng.below(3) {
        match rng.below(5) {
            0 => t.insert(
                "name".into(),
                Value::String(rng.pick(&["Ada", "Bo", "a b", "x\"y", "q'r", "é", "#hash", "[br]"]).to_string()),
            ),
            1 => t.insert("size".into(), Value::Integer(1 + rng.below(9) as i64)),
            2 => t.insert(
                "tags".into(),
                Value::Array((0..rng.below(3)).map(|i| Value::String(format!("t{i}"))).collect()),
            ),
            3 => {
                let mut meta = value(rng, 1);
                if !matches!(meta, Value::Table(_)) {
                    meta = Value::Table(std::iter::once(("v".to_string(), meta)).collect());
                }
                t.insert("meta".into(), meta)
            }
            _ => t.insert("every".into(), Value::String(rng.pick(&["5m", "1h", "90s", "250ms"]).to_string())),
        };
    }
    t
}

#[test]
fn long_runs_of_writes_and_undos_leave_the_file_holding_exactly_the_model() {
    for seed in [1u64, 2, 3] {
        let mut rng = Rng(seed);
        let home = Scratch::new("long");
        home.put(
            "crew.toml",
            "api = 1\n\n# kept for good\n[[thing]]\nid = \"t0\"\nname = \"Zero\"   # first\nsize = 2\n",
        );
        let writer = Writer::new(Some(home.path().to_path_buf()), None);
        let mut model: BTreeMap<String, Table> = BTreeMap::new();
        model.insert("t0".into(), "name = \"Zero\"\nsize = 2".parse().unwrap());
        let mut stack = Vec::new();
        let (mut applied, mut undos, mut removed, mut refused_plans) = (0, 0, 0, 0);
        for step in 0..160 {
            let (layer, read) = Layer::from_dir(LayerKind::User, "home", home.path());
            assert!(read.is_empty());
            let (t, problems) = Tables::load(vec![pack("p", KINDS), layer]);
            assert!(refused(&problems).is_empty(), "seed {seed} step {step}: {problems:?}\n{}", home.read("crew.toml"));
            let ids: Vec<String> = model.keys().cloned().collect();
            let id = if ids.is_empty() || rng.below(4) == 0 {
                format!("t{}", rng.below(12))
            } else {
                rng.pick(&ids).clone()
            };
            let change = if !stack.is_empty() && rng.below(6) == 0 {
                let (undone, prior): (j4n_core::change::Change, BTreeMap<String, Table>) = stack.pop().unwrap();
                let inverse = undone.inverse(Actor::Operator, step);
                match (t.vet(&inverse), writer.apply(&t, &inverse, None, &mut NoJournal)) {
                    (Ok(()), Ok(_)) => {
                        model = prior;
                        undos += 1;
                        None
                    }
                    (_, Err(e)) => panic!("seed {seed} step {step}: an undo of the latest change failed: {e}"),
                    (Err(e), _) => panic!("{e}"),
                }
            } else {
                let op = match rng.below(6) {
                    0 => Op::Remove,
                    1 => Op::Unset(vec![rng.pick(&["size", "tags", "meta", "every", "name"]).to_string()]),
                    _ => Op::Set(random_set(&mut rng)),
                };
                let w = Write {
                    to: Where::User,
                    file: Some("crew.toml".into()),
                    kind: "thing".into(),
                    id: id.clone(),
                    op: op.clone(),
                    by: Actor::Operator,
                    cause: None,
                    at_ms: step,
                };
                let planned = t.plan(&w).ok();
                refused_plans += usize::from(planned.is_none());
                planned.map(|p| (p, op))
            };
            if let Some((planned, op)) = change {
                let before = model.clone();
                writer.apply(&t, &planned.change, planned.file_hash.as_deref(), &mut NoJournal).unwrap_or_else(|e| {
                    panic!("seed {seed} step {step}: {e}\nchange {:?}\n{}", planned.change, home.read("crew.toml"))
                });
                applied += 1;
                match op {
                    Op::Remove => {
                        removed += 1;
                        model.remove(&id);
                    }
                    Op::Unset(names) => {
                        if let Some(e) = model.get_mut(&id) {
                            names.iter().for_each(|n| {
                                e.remove(n);
                            });
                        }
                    }
                    Op::Set(set) => model.entry(id.clone()).or_default().extend(set),
                }
                stack.push((planned.change, before));
            }
            let (layer, _) = Layer::from_dir(LayerKind::User, "home", home.path());
            let (after, problems) = Tables::load(vec![pack("p", KINDS), layer]);
            assert!(refused(&problems).is_empty(), "seed {seed} step {step}: {problems:?}\n{}", home.read("crew.toml"));
            for i in 0..12 {
                let id = format!("t{i}");
                assert_eq!(
                    after.layer_entry(Where::User, "thing", &id),
                    model.get(&id),
                    "seed {seed} step {step}: {id}\n{}",
                    home.read("crew.toml")
                );
            }
        }
        assert!(home.read("crew.toml").starts_with("api = 1\n"), "{}", home.read("crew.toml"));
        assert!(
            applied >= 60 && undos >= 5 && removed >= 3 && refused_plans >= 3,
            "seed {seed}: {applied} {undos} {removed} {refused_plans}"
        );
    }
}
