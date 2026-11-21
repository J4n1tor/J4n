//! Law 1, eight words: the size ratchet. A crate's non-test, non-blank lines stay under its ceiling in
//! `fixtures/size-budget.toml`; a feature is a pack, and the core grows only in a step that says why.

use crate::support::{assert_clean, code_only, crate_dirs, read, rel, root, walk_ext};
use std::collections::BTreeMap;

/// Non-blank lines of `src` outside inline `#[cfg(test)] mod … { … }` blocks.
fn production_lines(src: &str) -> usize {
    let code: Vec<String> = code_only(src).lines().map(str::to_owned).collect();
    let mut counted = 0;
    let mut i = 0;
    while i < code.len() {
        if let Some(module) = test_module_at(&code, i) {
            i = end_of_block(&code, module) + 1;
            continue;
        }
        counted += usize::from(!src.lines().nth(i).unwrap_or("").trim().is_empty());
        i += 1;
    }
    counted
}

/// The line of the module that the test-only attribute on line `i` gates, when it gates one: other attributes and blank
/// lines may sit between the two, and the module may be `pub`.
fn test_module_at(code: &[String], i: usize) -> Option<usize> {
    let attribute = code[i].trim();
    if attribute != "#[cfg(test)]" && !attribute.starts_with("#[cfg(all(test,") {
        return None;
    }
    let item = (i + 1..code.len()).find(|&j| {
        let line = code[j].trim_start();
        !line.is_empty() && !line.starts_with("#[")
    })?;
    let line = code[item].trim_start();
    let line = line.strip_prefix("pub").map_or(line, |rest| match rest.strip_prefix('(') {
        Some(scoped) => scoped.split_once(')').map_or(rest, |(_, after)| after).trim_start(),
        None => rest.trim_start(),
    });
    line.starts_with("mod ").then_some(item)
}

/// The line where the braces opened at or after line `from` are closed again.
fn end_of_block(code: &[String], from: usize) -> usize {
    let mut depth = 0;
    let mut opened = false;
    for (n, line) in code.iter().enumerate().skip(from) {
        for c in line.chars() {
            match c {
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        if opened && depth == 0 {
            return n;
        }
        if !opened && line.trim_end().ends_with(';') {
            return n;
        }
    }
    code.len().saturating_sub(1)
}

fn is_test_file(name: &str) -> bool {
    name == "tests.rs" || name.ends_with("_tests.rs")
}

/// Non-test, non-blank lines of a crate's `src/`.
fn crate_lines(crate_dir: &std::path::Path) -> usize {
    walk_ext(&crate_dir.join("src"), "rs")
        .iter()
        .filter(|path| !is_test_file(&path.file_name().unwrap_or_default().to_string_lossy()))
        .map(|path| production_lines(&read(path)))
        .sum()
}

fn ceilings() -> BTreeMap<String, usize> {
    let table: toml::Table = read(&root().join("fixtures/size-budget.toml")).parse().expect("size-budget.toml");
    assert_eq!(table["api"].as_integer(), Some(1), "size-budget.toml: api = 1");
    table["ceiling"]
        .as_table()
        .expect("[ceiling]")
        .iter()
        .map(|(name, n)| (name.clone(), usize::try_from(n.as_integer().expect("integer ceiling")).expect("ceiling")))
        .collect()
}

#[test]
fn every_crate_stays_under_its_ceiling() {
    let ceilings = ceilings();
    let mut violations = Vec::new();
    for dir in crate_dirs() {
        let name = dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let lines = crate_lines(&dir);
        match ceilings.get(&name) {
            None => violations.push(format!("{} has no ceiling in fixtures/size-budget.toml", rel(&dir))),
            Some(&ceiling) if lines > ceiling => {
                violations.push(format!(
                    "{name}: {lines} lines, ceiling {ceiling} — a ceiling rises only in a step that says why"
                ));
            }
            Some(_) => {}
        }
    }
    assert_clean("law 1 (size ratchet)", &violations);
}

#[test]
fn every_ceiling_names_a_crate() {
    let names: Vec<String> =
        crate_dirs().iter().map(|dir| dir.file_name().unwrap_or_default().to_string_lossy().into_owned()).collect();
    let stale: Vec<String> = ceilings()
        .into_keys()
        .filter(|name| !names.contains(name))
        .map(|name| format!("ceiling for `{name}`: no such crate"))
        .collect();
    assert_clean("law 1 (size ratchet)", &stale);
}

#[test]
fn blank_lines_do_not_count() {
    assert_eq!(production_lines("fn a() {}\n\n   \nfn b() {}\n"), 2);
}

#[test]
fn an_inline_test_module_does_not_count_but_the_code_after_it_does() {
    let src = "fn a() {\n    1;\n}\n\n#[cfg(test)]\nmod tests {\n    fn t() { let s = \"}\"; }\n    #[test]\n    fn u() {}\n}\n\nfn after() {}\n";
    assert_eq!(production_lines(src), 4);
}

#[test]
fn braces_in_strings_and_comments_do_not_end_a_test_module() {
    let src = "#[cfg(test)]\nmod t {\n    // }\n    const S: &str = \"}\";\n}\nfn kept() {}\n";
    assert_eq!(production_lines(src), 1);
}

#[test]
fn a_cfg_test_item_that_is_not_a_module_counts() {
    assert_eq!(production_lines("#[cfg(test)]\nfn helper() {}\n"), 2);
}

#[test]
fn test_files_are_recognised_by_name() {
    assert!(is_test_file("tests.rs") && is_test_file("table_tests.rs"));
    assert!(!is_test_file("table.rs") && !is_test_file("contests.rs"));
}

#[test]
fn attributes_between_the_cfg_test_and_the_module_do_not_hide_it() {
    let src = "#[cfg(test)]\n#[allow(clippy::unwrap_used)]\n\nmod tests {\n    fn t() {}\n}\nfn kept() {}\n";
    assert_eq!(production_lines(src), 1);
}

#[test]
fn a_public_or_all_test_module_does_not_count() {
    assert_eq!(production_lines("#[cfg(test)]\npub mod t {\n    fn a() {}\n}\nfn kept() {}\n"), 1);
    assert_eq!(production_lines("#[cfg(test)]\npub(crate) mod t {\n    fn a() {}\n}\nfn kept() {}\n"), 1);
    assert_eq!(production_lines("#[cfg(all(test, unix))]\nmod t {\n    fn a() {}\n}\nfn kept() {}\n"), 1);
}
