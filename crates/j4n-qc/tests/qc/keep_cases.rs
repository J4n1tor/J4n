//! The step `keep`: a surface taken before and after, and what is gone as the difference. The rows are the pack's own.

use crate::{dir, surfaces};
use j4n_qc::keep::{Item, lost, take};
use std::path::Path;

fn put(d: &Path, rel: &str, text: &str) {
    let full = d.join(rel);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, text).unwrap();
}

fn names(d: &Path) -> Vec<String> {
    take(&surfaces(), d).items.iter().map(|i| format!("{} {}", i.0, i.1)).collect()
}

const ALL: &[(&str, &str)] = &[
    ("src/lib.rs", "pub fn parse_x() {}\nfn private() {}\npub(crate) struct Table;\n#[test]\nfn parses() {}\n"),
    ("pkg/mod.py", "def public():\n    pass\ndef _hidden():\n    pass\nclass Thing:\n    def test_it(self):\n        pass\n"),
    ("web/app.ts", "export async function load() {}\nexport const KEY = 1;\nconst local = 2;\n"),
    ("web/app.test.ts", "it('loads the page', () => {});\ntest(\"keeps state\", () => {});\n"),
    ("srv/main.go", "func Serve() {}\nfunc helper() {}\nfunc (s *S) Close() {}\n"),
    ("srv/main_test.go", "func TestServe(t *testing.T) {}\n"),
    ("README.md", "# Demo\n\n## Install\n\nSee [the guide](docs/guide.md) and [site](https://example.org).\n"),
];

#[test]
fn the_seeds_find_the_public_names_the_tests_the_headings_and_the_links_of_five_languages() {
    let d = dir("keep-seeds");
    for (p, t) in ALL {
        put(&d, p, t);
    }
    let found = names(&d);
    for want in [
        "fn parse_x", "fn Table", "test parses", "fn public", "fn Thing", "test test_it", "fn load", "fn KEY", "test loads the page", "test keeps state", "fn Serve", "fn Close",
        "test TestServe", "heading Demo", "heading Install", "link docs/guide.md", "link https://example.org",
    ] {
        assert!(found.contains(&want.to_string()), "{want} is not in {found:?}");
    }
    for not in ["fn private", "fn _hidden", "fn local", "fn helper"] {
        assert!(!found.contains(&not.to_string()), "{not} is not public: {found:?}");
    }
    // the line is the one the name stands on
    let snap = take(&surfaces(), &d);
    assert!(snap.items.contains(&Item("test".into(), "parses".into(), "src/lib.rs".into(), 5, false)), "{:?}", snap.items);
    assert!(snap.items.contains(&Item("fn".into(), "Close".into(), "srv/main.go".into(), 3, false)));
}

#[test]
fn a_removed_name_is_gone_and_a_name_moved_to_another_file_is_not() {
    let d = dir("keep-gone");
    put(&d, "src/a.rs", "pub fn parse_x() {}\npub fn parse_y() {}\n");
    let before = take(&surfaces(), &d).items;
    // parse_x leaves the project; parse_y moves into another file; a name is added
    put(&d, "src/a.rs", "pub fn parse_z() {}\n");
    put(&d, "src/b.rs", "pub fn parse_y() {}\n");
    let now = take(&surfaces(), &d).items;
    let gone: Vec<String> = lost(&before, &now).iter().map(|i| format!("{} {} {}:{}", i.0, i.1, i.2, i.3)).collect();
    assert_eq!(gone, ["fn parse_x src/a.rs:1"]);
    // untouched, nothing is gone
    assert!(lost(&now, &now).is_empty());
}

#[test]
fn a_name_found_twice_before_and_once_now_is_one_gone() {
    let d = dir("keep-twice");
    put(&d, "src/a.rs", "pub fn new() {}\n");
    put(&d, "src/b.rs", "pub fn new() {}\n");
    let before = take(&surfaces(), &d).items;
    std::fs::remove_file(d.join("src/b.rs")).unwrap();
    let now = take(&surfaces(), &d).items;
    assert_eq!(lost(&before, &now).len(), 1);
}

#[test]
fn a_heading_belongs_to_its_file_and_a_function_does_not() {
    let d = dir("keep-file");
    put(&d, "README.md", "# Demo\n## Install\n");
    put(&d, "docs/a.md", "# Other\n");
    let before = take(&surfaces(), &d).items;
    put(&d, "README.md", "# Demo\n");
    put(&d, "docs/a.md", "# Other\n## Install\n");
    let gone: Vec<String> = lost(&before, &take(&surfaces(), &d).items).iter().map(|i| format!("{} {} {}", i.0, i.1, i.2)).collect();
    assert_eq!(gone, ["heading Install README.md"], "moved to another document, the heading is gone from the first");
}

#[test]
fn a_file_the_work_deletes_loses_its_names_and_an_untracked_new_one_is_seen() {
    let d = dir("keep-deleted");
    put(&d, "src/old.rs", "pub fn a() {}\npub fn b() {}\n");
    let before = take(&surfaces(), &d).items;
    std::fs::remove_file(d.join("src/old.rs")).unwrap();
    put(&d, "src/new.rs", "pub fn a() {}\n");
    let gone: Vec<String> = lost(&before, &take(&surfaces(), &d).items).iter().map(|i| i.1.clone()).collect();
    assert_eq!(gone, ["b"]);
}

#[test]
fn a_snapshot_over_its_cap_says_it_was_cut() {
    let d = dir("keep-cut");
    let big: String = (0..20_100).map(|i| format!("pub fn f{i}() {{}}\n")).collect();
    put(&d, "src/big.rs", &big);
    let snap = take(&surfaces(), &d);
    assert!(snap.cut && snap.items.len() == 20_000, "{} {}", snap.cut, snap.items.len());
}
