//! The law over every pack: a feature setting that names a model, a doer or a Decider has a `[[need]]` row that speaks for it, so a feature never acts on
//! one that is missing without saying so. It reads the packs' own files: the settings of every `packs/*/settings`, the rows of every `packs/*/needs`.

use j4n_core::front::needs::{NeedRow, names_a_model, uncovered};
use std::collections::BTreeSet;
use std::fs;

/// The entries `[[<kind>]]` of a file.
fn entries(text: &str, kind: &str) -> Vec<toml::Value> {
    let table: toml::Table = toml::from_str(text).unwrap();
    table.get(kind).and_then(toml::Value::as_array).cloned().unwrap_or_default()
}

fn toml_files(dir: &std::path::Path) -> Vec<String> {
    let Ok(read) = fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<_> = read.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "toml")).collect();
    names.sort();
    names.iter().filter_map(|p| fs::read_to_string(p).ok()).collect()
}

fn packs() -> (Vec<String>, Vec<NeedRow>) {
    let (mut settings, mut rows) = (Vec::new(), Vec::new());
    for pack in fs::read_dir(crate::support::root().join("packs")).unwrap().filter_map(Result::ok).map(|e| e.path()) {
        for text in toml_files(&pack.join("settings")) {
            settings.extend(entries(&text, "setting").iter().filter_map(|s| s.get("id")?.as_str().map(String::from)));
        }
        for text in toml_files(&pack.join("needs")) {
            rows.extend(entries(&text, "need").into_iter().map(|r| r.try_into::<NeedRow>().unwrap_or_else(|e| panic!("{}: {e}", pack.display()))));
        }
    }
    (settings, rows)
}

#[test]
fn every_setting_of_a_pack_that_names_a_model_a_doer_or_a_decider_has_a_need_row_that_speaks_for_it() {
    let (settings, rows) = packs();
    assert!(settings.iter().filter(|s| names_a_model(s)).count() > 15, "the law reads the packs' settings");
    let missing = uncovered(&settings, &rows);
    assert!(missing.is_empty(), "no `[[need]]` row speaks for: {missing:?} (a row names it in `doers`, `models` or `covers`, in the pack's `needs/`)");
}

#[test]
fn a_row_reads_names_real_settings_and_the_card_it_asks_has_what_it_needs() {
    let (settings, rows) = packs();
    let known: BTreeSet<&String> = settings.iter().collect();
    for r in &rows {
        r.check().unwrap_or_else(|e| panic!("{e}"));
        let named: Vec<&String> = r.doers.iter().chain(&r.models).chain(&r.covers).chain(std::iter::once(&r.feature)).collect();
        for id in named {
            assert!(known.contains(id), "need `{}` names `{id}`, which no pack declares", r.id);
        }
    }
    let mut ids = rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), rows.len(), "a need id is in one row");
}

#[test]
fn the_law_fails_on_a_setting_with_a_model_and_no_row() {
    let settings = vec!["zz.model".to_string(), "zz.on".to_string(), "zz.judge".to_string(), "zz.plain_answer".to_string()];
    assert_eq!(uncovered(&settings, &[]), [&"zz.model".to_string(), &"zz.judge".to_string()]);
    let row = NeedRow { id: "zz".into(), feature: "zz.on".into(), covers: vec!["zz.model".into(), "zz.judge".into()], kind: "key".into(), connector: "light".into(), ..from_toml() };
    assert!(uncovered(&settings, &[row]).is_empty());
}

fn from_toml() -> NeedRow {
    entries("[[need]]\nid = \"x\"\nfeature = \"x.on\"\n", "need").remove(0).try_into().unwrap()
}
