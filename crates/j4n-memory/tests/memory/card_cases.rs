//! The project card over a fixture folder. That the Brief is what it
//! was with the switch off is `j4n-core`'s `p1_card`.

use j4n_core::brief::{Brief, tokens};
use j4n_core::front::ProjectFacts;
use j4n_core::schema::DataClass;
use j4n_core::setting::Settings;
use j4n_core::value::Value;
use j4n_memory::{CARD_TOKENS, ProjectCard};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!("j4n-card-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }

    fn put(&self, path: &str, text: &str) {
        let at = self.0.join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn on(root: &Path) -> ProjectCard {
    ProjectCard::new(root, Settings::from_pairs([("brief.project_card", Value::Bool(true))]))
}

const CARGO: &str = "[workspace]\nmembers = [\"crates/a\", \"crates/b\"]\n";

fn fixture() -> Dir {
    let d = Dir::new();
    d.put("Cargo.toml", CARGO);
    d.put("package.json", "{\"name\":\"web\",\"scripts\":{\"test\":\"vitest run\",\"build\":\"tsc -p .\",\"postinstall\":\"x\"},\"devDependencies\":{\"typescript\":\"5\"}}");
    d.put("Makefile", "all: build\nbuild:\n\tcc x.c\ntest:\n\t./x\nclean:\n\trm x\n");
    d.put("crates/a/Cargo.toml", "[package]\nname = \"a\"\n");
    d.put("crates/b/src/lib.rs", "");
    d.put("scripts/check.sh", "#!/usr/bin/env bash\n# check.sh — the gate of every step\nset -e\n");
    d.put("docs/PLAN.md", "intro\n# The plan\nwords\n");
    d.put("docs/NOTES.md", "no heading here\n");
    d.put("README.md", "# Fixture\n");
    d.put("target/debug/out.txt", "test result: ok. 12 passed");
    d.put(".git/HEAD", "ref: refs/heads/main");
    d
}

#[test]
fn with_the_switch_on_a_fixture_project_gives_the_expected_card() {
    let d = fixture();
    let card = on(&d.0).text().expect("a card");
    let expected = "Project card, derived from the files:
Cargo.toml: rust · workspace of crates/a crates/b
package.json: typescript · web · build: tsc -p . · test: vitest run
Makefile: make targets: build test all
files: Cargo.toml Makefile README.md package.json
crates/ a/ b/
docs/ NOTES.md PLAN.md
scripts/ check.sh
scripts/check.sh: check.sh — the gate of every step
README.md: Fixture
docs/PLAN.md: The plan";
    assert_eq!(card, expected);
    // the switch is the setting: off, and there is no card, whatever the folder holds
    assert_eq!(ProjectCard::new(&d.0, Settings::from_pairs([("brief.project_card", Value::Bool(false))])).text(), None);
    assert_eq!(ProjectCard::new(&d.0, Settings::from_pairs([])).text(), None);
}

#[test]
fn editing_a_manifest_changes_exactly_the_facts_from_that_manifest() {
    let d = fixture();
    let c = on(&d.0);
    let first = c.refresh();
    assert!(first.contains(&"Cargo.toml".to_string()) && first.contains(&"docs/PLAN.md".to_string()), "{first:?}");
    let before = c.facts();
    assert!(before.iter().all(|f| f.hash.len() == 16));
    // nothing changed: nothing is made again
    assert!(c.refresh().is_empty());
    d.put("Cargo.toml", "[workspace]\nmembers = [\"crates/a\", \"crates/b\", \"crates/c\"]\n");
    assert_eq!(c.refresh(), ["Cargo.toml"]);
    let after = c.facts();
    let changed: Vec<&str> = after.iter().filter(|f| !before.contains(f)).map(|f| f.source.as_str()).collect();
    assert_eq!(changed, ["Cargo.toml"]);
    assert!(after.iter().find(|f| f.source == "Cargo.toml").unwrap().line.contains("crates/c"));
    // a source that goes away loses its facts
    std::fs::remove_file(d.0.join("Makefile")).unwrap();
    c.refresh();
    assert!(c.facts().iter().all(|f| f.source != "Makefile"));
    // a new file in a folder remakes the listing of that folder and the fact of the file, and nothing else
    d.put("scripts/t", "#!/bin/sh\n# t — one cargo command\n");
    let mut again = c.refresh();
    again.sort();
    assert_eq!(again, ["scripts/", "scripts/t"]);
}

#[test]
fn the_card_never_holds_the_diff_or_the_output_of_a_test() {
    let d = fixture();
    d.put("src/main.rs", "fn main() {}\n");
    let c = on(&d.0);
    let before = c.text().unwrap();
    // the work of a task: a change, a log of a test run, a plan
    d.put("src/main.rs", "fn main() { println!(\"changed\"); }\n");
    d.put("target/debug/test.log", "test result: FAILED. 3 failed");
    d.put(".j4n/plan.json", "{\"items\":[\"do the thing\"]}");
    d.put("out/diff.patch", "--- a/src/main.rs\n+++ b/src/main.rs\n");
    assert_eq!(c.text().unwrap(), before, "a task's work is not in the card");
    for what in ["changed", "FAILED", "do the thing", "+++", "passed"] {
        assert!(!before.contains(what), "{what}");
    }
    // every fact has a source that is a manifest, a folder, a script or a doc
    for f in c.facts() {
        assert!(f.source == "." || f.source.ends_with('/') || f.source.starts_with("scripts/") || f.source.starts_with("docs/") || f.source == "README.md" || ["Cargo.toml", "package.json", "Makefile"].contains(&f.source.as_str()), "{}", f.source);
    }
}

#[test]
fn the_card_fits_its_cap_and_so_does_its_section_of_the_brief() {
    let d = fixture();
    for i in 0..200 {
        d.put(&format!("docs/DOC-{i:03}.md"), &format!("# A document about the {i}th thing that the project has a long title for\n"));
        d.put(&format!("scripts/s{i:03}.sh"), "#!/bin/sh\n# a script that does a long thing the project needs often and says so at length\n");
        d.put(&format!("dir{i:03}/file-with-a-long-name.rs"), "");
    }
    let card = on(&d.0).text().expect("a card");
    assert!(tokens(&card) < CARD_TOKENS && CARD_TOKENS <= 300, "{} tokens", tokens(&card));
    assert!(card.starts_with("Project card") && card.contains("Cargo.toml: rust"), "the manifests come first and are kept: {card}");
    let section = Brief::new().with("contract", DataClass::Public, "Do it.").with("project", DataClass::Project, &card).render(8000);
    assert_eq!(section.sections, ["contract", "project"]);
    assert!(tokens(&section.text) - tokens("## contract\nDo it.") <= CARD_TOKENS + 2, "{} tokens", section.tokens);
}
