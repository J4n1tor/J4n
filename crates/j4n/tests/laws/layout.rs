//! The workspace keeps its shape (docs/ARCHITECTURE.md): the root manifest and the toolchain pin,
//! exactly the crates of the list (law 1: another is a deliberate edit that says why), manifests that inherit from
//! the workspace, one integration-test binary per crate, code only where the size ratchet counts it, the scripts,
//! `.gitignore` and the bug table.

use crate::support::{assert_clean, crate_dirs, read, rel, root, walk};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use toml::{Table, Value};

/// The crates of docs/ARCHITECTURE.md §3. `j4n-memory` is the one place that holds SQLite, and the bench is a doer of its own.
/// A new crate is a deliberate edit of this list that says why.
const CRATES: [&str; 17] = ["j4n-proto", "j4n-core", "j4n-connect", "j4n-reach", "j4n-memory", "j4n-bench", "j4n-shape", "j4n-qc", "j4n-contribute", "j4n-inbox", "j4n-ledger", "j4n-parts", "j4n-habits", "j4n-tui", "j4n-forge", "j4n-lab", "j4n"];

/// The toolchain pin.
const TOOLCHAIN: &str = "1.98.1";

/// Cargo manifests outside `crates/` and the root: the runner's self-test fixture is a workspace of its own.
const STANDALONE: [&str; 1] = ["fixtures/hermetic/Cargo.toml"];

/// What a crate takes from `[workspace.package]`.
const INHERITED: [&str; 4] = ["version", "edition", "rust-version", "publish"];

const SCRIPTS: [&str; 3] = ["scripts/b", "scripts/t", "scripts/check.sh"];

/// The owner-execute bit: the one permission bit git records.
const EXECUTABLE: u32 = 0o100;

/// The columns of the bug table in docs/dev/BUGS.md.
const BUG_COLUMNS: [&str; 5] = ["Id", "Found by", "Symptom", "Root cause", "Fixed in"];

fn parse(path: &Path) -> Table {
    read(path).parse().unwrap_or_else(|e| panic!("{}: {e}", rel(path)))
}

/// The value at the dotted `path` in `table`.
fn at<'a>(table: &'a Table, path: &str) -> Option<&'a Value> {
    let mut parts = path.split('.');
    let mut value = table.get(parts.next()?)?;
    for part in parts {
        value = value.get(part)?;
    }
    Some(value)
}

/// A violation unless `table` holds `want` at the dotted `path`.
fn expect_value(table: &Table, file: &str, path: &str, want: &Value, violations: &mut Vec<String>) {
    match at(table, path) {
        Some(found) if found == want => {}
        Some(found) => violations.push(format!("{file}: {path} is {found}, expected {want}")),
        None => violations.push(format!("{file}: {path} is missing, expected {want}")),
    }
}

/// Every `panic = "abort"` under `table`, whose own path is `path`.
fn abort_violations(table: &Table, path: &str, violations: &mut Vec<String>) {
    for (key, value) in table {
        let here = format!("{path}.{key}");
        if key == "panic" && value.as_str() == Some("abort") {
            violations.push(format!("Cargo.toml: {here} is \"abort\" — a lane thread must not take the process down"));
        } else if let Some(inner) = value.as_table() {
            abort_violations(inner, &here, violations);
        }
    }
}

/// The root manifest: members, resolver, edition, rust-version, profiles (docs/ARCHITECTURE.md).
fn workspace_violations(manifest: &Table) -> Vec<String> {
    let mut violations = Vec::new();
    let wanted: [(&str, Value); 7] = [
        ("workspace.members", vec!["crates/*"].into()),
        ("workspace.resolver", "3".into()),
        ("workspace.package.edition", "2024".into()),
        ("profile.dev.debug", 0_i64.into()),
        ("profile.release.strip", true.into()),
        ("profile.release.lto", "fat".into()),
        ("profile.release.codegen-units", 1_i64.into()),
    ];
    for (path, want) in &wanted {
        expect_value(manifest, "Cargo.toml", path, want, &mut violations);
    }
    if !at(manifest, "workspace.package.rust-version").and_then(Value::as_str).is_some_and(|v| !v.is_empty()) {
        violations.push("Cargo.toml: workspace.package.rust-version is missing or empty".to_owned());
    }
    if let Some(profiles) = manifest.get("profile").and_then(Value::as_table) {
        abort_violations(profiles, "profile", &mut violations);
    }
    violations
}

fn toolchain_violations(toolchain: &Table) -> Vec<String> {
    let mut violations = Vec::new();
    expect_value(toolchain, "rust-toolchain.toml", "toolchain.channel", &TOOLCHAIN.into(), &mut violations);
    violations
}

/// `found` are the names of the directories of `crates/`.
fn crate_list_violations(found: &[String]) -> Vec<String> {
    let mut violations: Vec<String> = found
        .iter()
        .filter(|name| !CRATES.contains(&name.as_str()))
        .map(|name| {
            format!("crates/{name}: not a crate of docs/ARCHITECTURE.md §3 — a new crate is an edit of CRATES that says why")
        })
        .collect();
    violations.extend(
        CRATES
            .iter()
            .filter(|name| !found.iter().any(|f| f == *name))
            .map(|name| format!("crates/{name}: named in docs/ARCHITECTURE.md §3, missing")),
    );
    violations
}

/// A crate manifest takes its package fields and its lints from the workspace. `dir` is the crate's path.
fn inheritance_violations(dir: &str, manifest: &Table) -> Vec<String> {
    let inherits = |path: &str| at(manifest, path) == Some(&Value::Boolean(true));
    let mut violations: Vec<String> = INHERITED
        .iter()
        .filter(|key| !inherits(&format!("package.{key}.workspace")))
        .map(|key| format!("{dir}/Cargo.toml: package.{key} must be `{key}.workspace = true`"))
        .collect();
    if !inherits("lints.workspace") {
        violations.push(format!("{dir}/Cargo.toml: [lints] workspace = true is missing"));
    }
    violations
}

/// The integration-test binaries cargo finds under a crate's `tests/`: `name.rs` and `name/main.rs`. `files` are the
/// paths below `tests/`, with `/` separators; other files and deeper directories are modules and data.
fn test_binaries(files: &[String]) -> Vec<&str> {
    let mut binaries: Vec<&str> = files
        .iter()
        .filter_map(|file| match file.split_once('/') {
            None => file.strip_suffix(".rs"),
            Some((dir, "main.rs")) => Some(dir),
            Some(_) => None,
        })
        .collect();
    binaries.sort_unstable();
    binaries.dedup();
    binaries
}

/// One integration-test binary per crate: every extra one links the crate again and makes `target/` grow fast. `dir` is the
/// crate's path.
fn test_binary_violations(dir: &str, manifest: &Table, files: &[String]) -> Vec<String> {
    let mut violations = Vec::new();
    let binaries = test_binaries(files);
    if binaries.len() > 1 {
        violations.push(format!(
            "{dir}/tests: {} integration-test binaries ({}) — the layout allows one per crate",
            binaries.len(),
            binaries.join(", ")
        ));
    }
    if manifest.contains_key("test") {
        violations.push(format!("{dir}/Cargo.toml: a [[test]] table — the binary under tests/ is found by itself"));
    }
    if at(manifest, "package.autotests") == Some(&Value::Boolean(false)) {
        violations.push(format!("{dir}/Cargo.toml: autotests = false — the binary under tests/ is found by itself"));
    }
    violations
}

/// A crate keeps its code in `src/`, where the size ratchet counts it: no `path` on a target table and no build script.
/// `dir` is the crate's path.
fn code_location_violations(dir: &str, manifest: &Table, has_build_script: bool) -> Vec<String> {
    let mut violations = Vec::new();
    for kind in ["lib", "bin", "example", "bench"] {
        let tables: Vec<&Table> = match manifest.get(kind) {
            Some(Value::Table(table)) => vec![table],
            Some(Value::Array(tables)) => tables.iter().filter_map(Value::as_table).collect(),
            _ => Vec::new(),
        };
        if tables.iter().any(|table| table.contains_key("path")) {
            violations
                .push(format!("{dir}/Cargo.toml: a `path` on [{kind}] — code outside src/ is not counted by law 1"));
        }
    }
    // the one build script: it stamps the commit and its date into `j4n --version`
    let stamp = dir == "crates/j4n";
    if (has_build_script && !stamp) || at(manifest, "package.build").is_some() {
        violations.push(format!("{dir}: a build script — code that law 1 does not count"));
    }
    violations
}

/// Every Cargo manifest in the repository is the workspace's, a crate's under `crates/`, or a listed standalone one: a
/// crate anywhere else joins the workspace through a path dependency unseen by the crate list. `manifests` are
/// repo-relative paths with `/` separators.
fn manifest_location_violations(manifests: &[String]) -> Vec<String> {
    let is_crate = |path: &str| {
        path.strip_prefix("crates/")
            .and_then(|rest| rest.strip_suffix("/Cargo.toml"))
            .is_some_and(|name| !name.contains('/'))
    };
    manifests
        .iter()
        .filter(|path| path.as_str() != "Cargo.toml" && !is_crate(path) && !STANDALONE.contains(&path.as_str()))
        .map(|path| {
            format!("{path}: a Cargo manifest outside crates/ — a crate is a deliberate edit of CRATES that says why")
        })
        .collect()
}

/// Every law file is compiled: it has a `mod <name>;` line in `main.rs`, and every such line has its file. `files` are the
/// names of the `.rs` files in the laws directory.
fn undeclared_law_violations(files: &[String], main_rs: &str) -> Vec<String> {
    let declared: Vec<&str> = main_rs
        .lines()
        .filter_map(|line| line.trim().strip_prefix("mod ").and_then(|rest| rest.strip_suffix(';')))
        .collect();
    let stems: Vec<&str> =
        files.iter().filter_map(|file| file.strip_suffix(".rs")).filter(|stem| *stem != "main").collect();
    let mut violations: Vec<String> = stems
        .iter()
        .filter(|stem| !declared.contains(stem))
        .map(|stem| format!("laws/{stem}.rs is not declared in main.rs — its tests never run"))
        .collect();
    violations.extend(
        declared
            .iter()
            .filter(|name| !stems.contains(name))
            .map(|name| format!("main.rs declares `mod {name};` but laws/{name}.rs does not exist")),
    );
    violations
}

fn gitignore_violations(text: &str) -> Vec<String> {
    if text.lines().any(|line| line.trim().trim_matches('/') == "target") {
        Vec::new()
    } else {
        vec![".gitignore: does not list target/".to_owned()]
    }
}

/// `modes` maps a script's path to its unix mode; a script missing from it does not exist.
fn script_violations(modes: &BTreeMap<String, u32>) -> Vec<String> {
    SCRIPTS
        .iter()
        .filter_map(|script| match modes.get(*script) {
            None => Some(format!("{script}: missing")),
            Some(mode) if mode & EXECUTABLE == 0 => Some(format!("{script}: not executable (mode {:o})", mode & 0o777)),
            Some(_) => None,
        })
        .collect()
}

/// The cells of a table row.
fn cells(line: &str) -> Vec<&str> {
    line.trim().trim_matches('|').split('|').map(str::trim).collect()
}

fn is_row(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

/// A row of dashes and colons: the line under a table header.
fn is_separator(line: &str) -> bool {
    is_row(line) && cells(line).iter().all(|cell| !cell.is_empty() && cell.chars().all(|c| c == '-' || c == ':'))
}

/// The cell is the column's name, then anything but a letter or digit — `Fixed in (commit + test)` names `Fixed in`.
fn names_column(cell: &str, column: &str) -> bool {
    cell.to_lowercase()
        .strip_prefix(&column.to_lowercase())
        .is_some_and(|rest| !rest.starts_with(char::is_alphanumeric))
}

/// Some table of docs/dev/BUGS.md has a header with every column of `BUG_COLUMNS`; no wording beyond the names is read.
fn bug_table_violations(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let found = lines.windows(2).any(|pair| {
        is_row(pair[0]) && is_separator(pair[1]) && {
            let header = cells(pair[0]);
            BUG_COLUMNS.iter().all(|column| header.iter().any(|cell| names_column(cell, column)))
        }
    });
    if found {
        Vec::new()
    } else {
        vec![format!("docs/dev/BUGS.md: no table header has the columns {}", BUG_COLUMNS.join(", "))]
    }
}

#[test]
fn the_workspace_manifest_keeps_its_shape() {
    assert_clean("layout (workspace manifest)", &workspace_violations(&parse(&root().join("Cargo.toml"))));
}

#[test]
fn the_toolchain_is_pinned() {
    assert_clean("layout (toolchain)", &toolchain_violations(&parse(&root().join("rust-toolchain.toml"))));
}

#[test]
fn the_crates_are_the_listed_ones() {
    let names: Vec<String> =
        crate_dirs().iter().map(|dir| dir.file_name().unwrap_or_default().to_string_lossy().into_owned()).collect();
    assert_clean("law 1 (crate list)", &crate_list_violations(&names));
}

#[test]
fn every_crate_inherits_its_package_fields_and_lints_from_the_workspace() {
    let mut violations = Vec::new();
    for dir in crate_dirs() {
        violations.extend(inheritance_violations(&rel(&dir), &parse(&dir.join("Cargo.toml"))));
    }
    assert_clean("layout (crate manifests)", &violations);
}

#[test]
fn every_crate_has_at_most_one_integration_test_binary() {
    let mut violations = Vec::new();
    for dir in crate_dirs() {
        let tests = dir.join("tests");
        let files: Vec<String> = walk(&tests)
            .iter()
            .map(|path| path.strip_prefix(&tests).unwrap_or(path).to_string_lossy().replace('\\', "/"))
            .collect();
        violations.extend(test_binary_violations(&rel(&dir), &parse(&dir.join("Cargo.toml")), &files));
    }
    assert_clean("layout (test binaries)", &violations);
}

#[test]
fn no_crate_keeps_code_outside_src() {
    let mut violations = Vec::new();
    for dir in crate_dirs() {
        let has_build_script = dir.join("build.rs").exists();
        violations.extend(code_location_violations(&rel(&dir), &parse(&dir.join("Cargo.toml")), has_build_script));
    }
    assert_clean("layout (code location)", &violations);
}

#[test]
fn every_cargo_manifest_is_the_workspace_a_crate_or_the_runner_fixture() {
    let manifests: Vec<String> = walk(&root())
        .iter()
        .filter(|path| path.file_name().is_some_and(|name| name == "Cargo.toml"))
        .map(|path| rel(path))
        .collect();
    assert_clean("layout (manifests)", &manifest_location_violations(&manifests));
}

#[test]
fn every_law_file_is_declared_in_main() {
    let laws = root().join("crates/j4n/tests/laws");
    let files: Vec<String> = walk(&laws)
        .iter()
        .filter(|path| path.parent() == Some(laws.as_path()))
        .filter_map(|path| path.file_name().map(|name| name.to_string_lossy().into_owned()))
        .filter(|name| name.ends_with(".rs"))
        .collect();
    assert_clean("layout (law files)", &undeclared_law_violations(&files, &read(&laws.join("main.rs"))));
}

#[test]
fn target_is_ignored_by_git() {
    assert_clean("layout (.gitignore)", &gitignore_violations(&read(&root().join(".gitignore"))));
}

#[test]
fn the_scripts_exist_and_are_executable() {
    let modes: BTreeMap<String, u32> = SCRIPTS
        .iter()
        .filter_map(|script| {
            let meta = fs::metadata(root().join(script)).ok()?;
            meta.is_file().then(|| ((*script).to_owned(), meta.permissions().mode()))
        })
        .collect();
    assert_clean("layout (scripts)", &script_violations(&modes));
}

#[test]
fn the_bug_table_has_its_columns() {
    if !crate::support::has_private_record() {
        return; // the public tree has no bug table
    }
    assert_clean("layout (bug table)", &bug_table_violations(&read(&root().join("docs/dev/BUGS.md"))));
}

fn table(text: &str) -> Table {
    text.parse().expect("test manifest")
}

fn mentions(violations: &[String], needle: &str) -> bool {
    violations.iter().any(|violation| violation.contains(needle))
}

const GOOD_WORKSPACE: &str = r#"
[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.package]
version = "0.0.0"
edition = "2024"
rust-version = "1.98"
publish = false

[profile.dev]
debug = 0

[profile.release]
strip = true
lto = "fat"
codegen-units = 1
"#;

/// The violations of `GOOD_WORKSPACE` with `from` replaced by `to`.
fn workspace_after(from: &str, to: &str) -> Vec<String> {
    assert!(GOOD_WORKSPACE.contains(from), "the good manifest lacks {from:?}");
    workspace_violations(&table(&GOOD_WORKSPACE.replace(from, to)))
}

#[test]
fn a_workspace_manifest_as_planned_is_clean() {
    let violations = workspace_violations(&table(GOOD_WORKSPACE));
    assert!(violations.is_empty(), "{violations:?}");
}

#[test]
fn a_changed_workspace_setting_is_named() {
    for (from, to, named) in [
        ("resolver = \"3\"", "resolver = \"2\"", "workspace.resolver"),
        ("members = [\"crates/*\"]", "members = [\"crates/a\"]", "workspace.members"),
        ("edition = \"2024\"", "edition = \"2021\"", "workspace.package.edition"),
        ("rust-version = \"1.98\"\n", "", "rust-version"),
        ("rust-version = \"1.98\"", "rust-version = \"\"", "rust-version"),
        ("debug = 0", "debug = 2", "profile.dev.debug"),
        ("strip = true", "strip = false", "profile.release.strip"),
        ("lto = \"fat\"", "lto = \"thin\"", "profile.release.lto"),
        ("codegen-units = 1", "codegen-units = 16", "profile.release.codegen-units"),
    ] {
        let violations = workspace_after(from, to);
        assert!(mentions(&violations, named), "{from} -> {to}: {violations:?}");
    }
}

#[test]
fn panic_abort_is_caught_in_any_profile_and_unwind_is_not() {
    for (from, to, named) in [
        ("codegen-units = 1", "codegen-units = 1\npanic = \"abort\"", "profile.release.panic"),
        ("debug = 0", "debug = 0\npanic = \"abort\"", "profile.dev.panic"),
        (
            "[profile.release]",
            "[profile.release.package.x]\npanic = \"abort\"\n\n[profile.release]",
            "profile.release.package.x.panic",
        ),
    ] {
        let violations = workspace_after(from, to);
        assert!(mentions(&violations, named), "{to}: {violations:?}");
    }
    assert!(workspace_after("codegen-units = 1", "codegen-units = 1\npanic = \"unwind\"").is_empty());
}

#[test]
fn a_manifest_without_a_workspace_lists_every_setting_as_missing() {
    let violations = workspace_violations(&table("[package]\nname = \"x\"\n"));
    assert!(mentions(&violations, "workspace.members is missing"), "{violations:?}");
    assert!(mentions(&violations, "profile.release.lto is missing"), "{violations:?}");
}

#[test]
fn the_pinned_toolchain_passes_and_any_other_channel_is_named() {
    assert!(toolchain_violations(&table(&format!("[toolchain]\nchannel = \"{TOOLCHAIN}\"\n"))).is_empty());
    for bad in ["[toolchain]\nchannel = \"stable\"\n", "[toolchain]\nchannel = \"1.98.0\"\n", "[toolchain]\n"] {
        let violations = toolchain_violations(&table(bad));
        assert!(mentions(&violations, "toolchain.channel"), "{bad}: {violations:?}");
    }
}

#[test]
fn exactly_the_six_crates_pass() {
    let names: Vec<String> = CRATES.iter().map(|name| (*name).to_owned()).collect();
    assert!(crate_list_violations(&names).is_empty());
}

#[test]
fn a_seventh_crate_is_caught() {
    let mut names: Vec<String> = CRATES.iter().map(|name| (*name).to_owned()).collect();
    names.push("j4n-extra".to_owned());
    let violations = crate_list_violations(&names);
    assert!(mentions(&violations, "crates/j4n-extra"), "{violations:?}");
    assert!(mentions(&violations, "says why"), "{violations:?}");
}

#[test]
fn a_missing_crate_is_caught() {
    let names: Vec<String> =
        CRATES.iter().filter(|name| **name != "j4n-reach").map(|name| (*name).to_owned()).collect();
    let violations = crate_list_violations(&names);
    assert!(mentions(&violations, "crates/j4n-reach"), "{violations:?}");
}

const DIR: &str = "crates/j4n-core";

const GOOD_CRATE: &str = r#"
[package]
name = "j4n-core"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[lints]
workspace = true

[dependencies]
toml.workspace = true
"#;

#[test]
fn a_crate_manifest_that_inherits_everything_is_clean() {
    assert!(inheritance_violations(DIR, &table(GOOD_CRATE)).is_empty());
}

#[test]
fn a_crate_field_that_is_not_inherited_is_named() {
    for (from, to, named) in [
        ("version.workspace = true", "version = \"0.1.0\"", "package.version"),
        ("edition.workspace = true", "edition = \"2021\"", "package.edition"),
        ("rust-version.workspace = true", "", "package.rust-version"),
        ("publish.workspace = true", "publish = true", "package.publish"),
        ("edition.workspace = true", "edition.workspace = false", "package.edition"),
    ] {
        let violations = inheritance_violations(DIR, &table(&GOOD_CRATE.replace(from, to)));
        assert!(mentions(&violations, named), "{from} -> {to}: {violations:?}");
    }
}

#[test]
fn a_crate_without_workspace_lints_is_caught() {
    for (from, to) in [("[lints]\nworkspace = true\n", ""), ("[lints]\nworkspace = true", "[lints]\nworkspace = false")]
    {
        let violations = inheritance_violations(DIR, &table(&GOOD_CRATE.replace(from, to)));
        assert!(mentions(&violations, "[lints] workspace = true"), "{from}: {violations:?}");
    }
}

fn files(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn test_binaries_are_top_level_rs_files_and_directories_with_a_main() {
    let listing = files(&["laws/main.rs", "laws/support.rs", "smoke.rs", "data/case.toml", "deep/inner/main.rs"]);
    assert_eq!(test_binaries(&listing), ["laws", "smoke"]);
    assert_eq!(test_binaries(&files(&["main.rs"])), ["main"]);
    assert!(test_binaries(&files(&["notes.txt", "fixtures/a.rs", "fixtures/b/c.rs"])).is_empty());
}

#[test]
fn zero_or_one_test_binary_passes() {
    let manifest = table(GOOD_CRATE);
    assert!(test_binary_violations(DIR, &manifest, &[]).is_empty());
    assert!(test_binary_violations(DIR, &manifest, &files(&["only.rs"])).is_empty());
    let one_with_modules = files(&["laws/main.rs", "laws/support.rs", "laws/size.rs", "data.json"]);
    assert!(test_binary_violations(DIR, &manifest, &one_with_modules).is_empty());
}

#[test]
fn a_second_test_binary_is_caught_whatever_its_shape() {
    let manifest = table(GOOD_CRATE);
    for second in [files(&["a.rs", "b.rs"]), files(&["laws/main.rs", "extra.rs"]), files(&["a/main.rs", "b/main.rs"])] {
        let violations = test_binary_violations(DIR, &manifest, &second);
        assert!(mentions(&violations, "2 integration-test binaries"), "{second:?}: {violations:?}");
    }
}

#[test]
fn a_test_table_or_disabled_autotests_is_caught() {
    let with_table = table(&format!("{GOOD_CRATE}\n[[test]]\nname = \"x\"\npath = \"tests/x.rs\"\n"));
    assert!(mentions(&test_binary_violations(DIR, &with_table, &[]), "[[test]]"));
    let no_autotests =
        table(&GOOD_CRATE.replace("publish.workspace = true", "publish.workspace = true\nautotests = false"));
    assert!(mentions(&test_binary_violations(DIR, &no_autotests, &[]), "autotests = false"));
    let autotests_on =
        table(&GOOD_CRATE.replace("publish.workspace = true", "publish.workspace = true\nautotests = true"));
    assert!(test_binary_violations(DIR, &autotests_on, &[]).is_empty());
}

#[test]
fn a_path_on_a_target_table_or_a_build_script_is_caught() {
    assert!(code_location_violations(DIR, &table(GOOD_CRATE), false).is_empty());
    let lib = table(&format!("{GOOD_CRATE}\n[lib]\npath = \"lib/lib.rs\"\n"));
    assert!(mentions(&code_location_violations(DIR, &lib, false), "[lib]"));
    let bin = table(&format!("{GOOD_CRATE}\n[[bin]]\nname = \"x\"\npath = \"tools/x.rs\"\n"));
    assert!(mentions(&code_location_violations(DIR, &bin, false), "[bin]"));
    let named_bin = table(&format!("{GOOD_CRATE}\n[[bin]]\nname = \"x\"\n"));
    assert!(code_location_violations(DIR, &named_bin, false).is_empty());
    assert!(mentions(&code_location_violations(DIR, &table(GOOD_CRATE), true), "build script"));
    assert!(code_location_violations("crates/j4n", &table(GOOD_CRATE), true).is_empty());
    let build = table(&GOOD_CRATE.replace("publish.workspace = true", "publish.workspace = true\nbuild = \"gen.rs\""));
    assert!(mentions(&code_location_violations(DIR, &build, false), "build script"));
}

#[test]
fn a_manifest_outside_crates_is_caught() {
    let listed =
        files(&["Cargo.toml", "crates/j4n/Cargo.toml", "crates/j4n-core/Cargo.toml", "fixtures/hermetic/Cargo.toml"]);
    assert!(manifest_location_violations(&listed).is_empty());
    for stray in
        ["extras/foo/Cargo.toml", "crates/j4n/inner/Cargo.toml", "packs/base/Cargo.toml", "fixtures/other/Cargo.toml"]
    {
        let violations = manifest_location_violations(&files(&[stray]));
        assert!(mentions(&violations, stray), "{stray}: {violations:?}");
    }
}

#[test]
fn a_law_file_main_does_not_declare_and_a_declaration_without_a_file_are_caught() {
    let main = "//! docs\n\nmod size;\nmod support;\n";
    assert!(undeclared_law_violations(&files(&["main.rs", "size.rs", "support.rs"]), main).is_empty());
    let forgotten = undeclared_law_violations(&files(&["main.rs", "size.rs", "support.rs", "proxy.rs"]), main);
    assert!(mentions(&forgotten, "proxy.rs is not declared"), "{forgotten:?}");
    let dangling = undeclared_law_violations(&files(&["main.rs", "size.rs"]), main);
    assert!(mentions(&dangling, "`mod support;`"), "{dangling:?}");
}

#[test]
fn a_gitignore_that_lists_target_passes() {
    for good in ["target/\n", "*.log\ntarget/\n.DS_Store\n", "/target\n", "  target/  \n", "target\n"] {
        assert!(gitignore_violations(good).is_empty(), "{good:?}");
    }
}

#[test]
fn a_gitignore_that_does_not_list_target_is_caught() {
    for bad in ["", "*.log\n", "targets/\n", "# target/\n", "!target/\n", "crates/target/\n", "target/x\n"] {
        assert!(mentions(&gitignore_violations(bad), "target/"), "{bad:?}");
    }
}

fn modes(entries: &[(&str, u32)]) -> BTreeMap<String, u32> {
    entries.iter().map(|(path, mode)| ((*path).to_owned(), *mode)).collect()
}

#[test]
fn executable_scripts_pass() {
    let all = modes(&[("scripts/b", 0o100_755), ("scripts/t", 0o100_700), ("scripts/check.sh", 0o100_755)]);
    assert!(script_violations(&all).is_empty());
}

#[test]
fn a_missing_script_is_caught() {
    let violations = script_violations(&modes(&[("scripts/b", 0o100_755), ("scripts/check.sh", 0o100_755)]));
    assert_eq!(violations, ["scripts/t: missing"]);
}

#[test]
fn a_script_without_the_execute_bit_is_caught() {
    let violations = script_violations(&modes(&[
        ("scripts/b", 0o100_755),
        ("scripts/t", 0o100_644),
        ("scripts/check.sh", 0o100_011),
    ]));
    assert_eq!(violations, ["scripts/t: not executable (mode 644)", "scripts/check.sh: not executable (mode 11)"]);
}

#[test]
fn a_bug_table_with_the_columns_passes_whatever_follows_the_names() {
    let current =
        "# Bugs\n\n| Id | Found by | Symptom | Root cause | Fixed in (commit + test) |\n|---|---|---|---|---|\n";
    let plain = "| id | found by | symptom | root cause | fixed in |\n| :-- | --- | --- | --- | --: |\n| B1 | a | b | c | d |\n";
    let extra = "| Id | Found by | Symptom | Root cause | Fixed in | Test |\n|---|---|---|---|---|---|\n";
    for good in [current, plain, extra] {
        assert!(bug_table_violations(good).is_empty(), "{good}");
    }
}

#[test]
fn a_bug_table_after_other_text_and_tables_passes() {
    let text = "Intro.\n\n| Key | Meaning |\n|---|---|\n| a | b |\n\n| Id | Found by | Symptom | Root cause | Fixed in |\n|---|---|---|---|---|\n";
    assert!(bug_table_violations(text).is_empty());
}

#[test]
fn a_bug_table_that_lacks_a_column_or_is_no_table_is_caught() {
    let all = "| Id | Found by | Symptom | Root cause | Fixed in |";
    for bad in [
        "".to_owned(),
        "No table here.\n".to_owned(),
        format!("{}\n", all.replace("Symptom", "What")),
        format!("{}\n|---|---|---|---|---|\n", all.replace("Found by", "Idea")),
        format!("{all}\nnot a separator\n"),
        format!("{}\n|---|---|---|---|\n", all.replace("| Fixed in |", "|")),
        "Id, Found by, Symptom, Root cause, Fixed in\n---\n".to_owned(),
    ] {
        let violations = bug_table_violations(&bad);
        assert!(mentions(&violations, "no table header has the columns Id, Found by"), "{bad:?}: {violations:?}");
    }
}
