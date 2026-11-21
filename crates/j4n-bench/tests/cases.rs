//! The cases of the Table: what is read, and what is left out and why.

use j4n_bench::Bench;
use j4n_core::log::{Log, Opts, Scope};
use j4n_core::redact::Redactor;
use j4n_core::table::{KindDecl, Layer, LayerKind, SourceFile, Tables};
use std::path::Path;

fn tables(user: &str) -> Tables {
    let pack = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs");
    let (mut base, read) = Layer::from_dir(LayerKind::Pack, "base-data", &pack.join("base"));
    assert!(read.is_empty(), "{read:?}");
    base.files.retain(|f| !f.path.starts_with("kinds/") && f.path != "pack.toml");
    let (mut bench, read) = Layer::from_dir(LayerKind::Pack, "bench-data", &pack.join("bench"));
    assert!(read.is_empty(), "{read:?}");
    bench.files.retain(|f| f.path != "pack.toml");
    let mine = Layer::new(LayerKind::User, "mine", vec![SourceFile { path: "cases/mine.toml".into(), text: format!("api = 1\n{user}") }]);
    let (t, problems) = Tables::load(vec![base, bench, mine]);
    assert!(problems.iter().all(|p| p.to_string().contains("case")) || problems.is_empty(), "{problems:?}");
    t
}

fn load(user: &str) -> (Vec<String>, Vec<String>) {
    let dir = std::env::temp_dir().join(format!("j4n-bench-{}-{}", std::process::id(), user.len()));
    let _ = std::fs::remove_dir_all(&dir);
    let (log, _) = Log::open(&dir.join(".j4n"), Scope::Project, Opts::default(), Redactor::new()).unwrap();
    let (bench, found) = Bench::load(&tables(user), &dir.join("bench"), log);
    let _ = std::fs::remove_dir_all(&dir);
    drop(bench);
    (found.iter().map(ToString::to_string).collect(), Vec::new())
}

#[test]
fn the_pack_ships_cases_and_they_all_load_clean() {
    let t = tables("");
    assert!(t.ids("case").len() >= 2);
    let decl: KindDecl = t.get("case").unwrap();
    assert_eq!(decl.examples.len(), 1);
    let (found, _) = load("");
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_case_that_cannot_be_run_is_left_out_and_told_why() {
    for (case, why) in [
        ("[[case]]\nid = \"empty\"\nturns = []\n", "at least one line"),
        ("[[case]]\nid = \"climb\"\nfiles = { \"../x\" = \"y\" }\n[[case.turns]]\nsay = \"hi\"\n", "plain relative path"),
        ("[[case]]\nid = \"abs\"\nfiles = { \"/etc/x\" = \"y\" }\n[[case.turns]]\nsay = \"hi\"\n", "plain relative path"),
        ("[[case]]\nid = \"bad\"\n[[case.turns]]\nsay = \"hi\"\nexpect = \"contains(\"\n", "is not an expression"),
    ] {
        let (found, _) = load(case);
        assert!(found.iter().any(|m| m.contains(why)), "{case}: {found:?}");
    }
}
