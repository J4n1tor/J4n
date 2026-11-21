//! What counts as a mark: the net-new matches of a hunk (a rename or a move that rewrites a line holding an old match marks nothing),
//! the removed side for `scope = "removed"`, a match across lines, and only the files a pattern looks at.

use crate::{dir, ready};
use j4n_qc::change::{Change, git};
use j4n_qc::scan::{self, Mark};

fn repo(tag: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let d = dir(tag);
    git(&d, &["init", "-q", "-b", "main"]).unwrap();
    for (p, t) in files {
        let full = d.join(p);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, t).unwrap();
    }
    git(&d, &["add", "."]).unwrap();
    git(&d, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "first"]).unwrap();
    d
}

fn put(d: &std::path::Path, p: &str, t: &str) {
    std::fs::write(d.join(p), t).unwrap();
}

fn at(marks: &[Mark]) -> Vec<(String, u32)> {
    marks.iter().map(|m| (m.file.clone(), m.line)).collect()
}

#[test]
fn an_unwrap_already_in_the_file_is_not_marked_and_a_new_one_is_at_its_file_and_line() {
    let d = repo("s40a", &[("src/a.rs", "fn a() {\n    let x = s.parse().unwrap();\n}\n")]);
    put(&d, "src/a.rs", "fn a() {\n    let x = s.parse().unwrap();\n}\nfn b() {\n    let y = t.parse().unwrap();\n}\n");
    let c = Change::of_task(&d, None, &["src/a.rs".to_string()]);
    assert_eq!(at(&scan::regex(&[ready("rs-unwrap")], &c)), [("src/a.rs".to_string(), 5)]);
}

#[test]
fn a_rename_that_rewrites_a_line_holding_an_old_todo_marks_nothing() {
    let d = repo("s40b", &[("a.py", "def old_name():\n    return 1  # TODO later\n")]);
    put(&d, "a.py", "def new_name():\n    return 1  # TODO later\n");
    let c = Change::of_task(&d, None, &["a.py".to_string()]);
    assert!(scan::regex(&[ready("todo-left")], &c).is_empty());
    // a second one is new
    put(&d, "a.py", "def new_name():\n    return 1  # TODO later\n    # TODO also\n");
    let c = Change::of_task(&d, None, &["a.py".to_string()]);
    assert_eq!(at(&scan::regex(&[ready("todo-left")], &c)), [("a.py".to_string(), 3)]);
}

#[test]
fn a_removed_assertion_is_marked_at_its_old_line_and_a_renamed_one_is_not() {
    let test = "def test_parse():\n    assert parse('a,b,') == ['a', 'b']\n    assert parse('') == []\n";
    let d = repo("s40c", &[("tests/test_parse.py", test)]);
    put(&d, "tests/test_parse.py", "def test_parse():\n    assert parse('') == []\n");
    let c = Change::of_task(&d, None, &["tests/test_parse.py".to_string()]);
    assert_eq!(at(&scan::regex(&[ready("assert-removed")], &c)), [("tests/test_parse.py".to_string(), 2)]);
    // the same assertion with another name for the function it calls
    put(&d, "tests/test_parse.py", "def test_parse():\n    assert parse_csv('a,b,') == ['a', 'b']\n    assert parse('') == []\n");
    let c = Change::of_task(&d, None, &["tests/test_parse.py".to_string()]);
    assert!(scan::regex(&[ready("assert-removed")], &c).is_empty(), "a renamed assertion is still there");
}

#[test]
fn a_pattern_looks_only_at_the_files_its_globs_take() {
    let d = repo("s40d", &[("src/a.rs", "fn a() {}\n"), ("tests/t.rs", "fn t() {}\n")]);
    put(&d, "src/a.rs", "fn a() { x.unwrap(); }\n");
    put(&d, "tests/t.rs", "fn t() { x.unwrap(); }\n");
    let c = Change::of_task(&d, None, &["src/a.rs".to_string(), "tests/t.rs".to_string()]);
    assert_eq!(at(&scan::regex(&[ready("rs-unwrap")], &c)), [("src/a.rs".to_string(), 1)], "tests may unwrap");
}

#[test]
fn a_match_that_runs_over_lines_is_found_in_the_added_text_of_a_hunk() {
    let d = repo("s40e", &[("a.py", "x = 1\n")]);
    put(&d, "a.py", "x = 1\ntry:\n    os.remove(t)\nexcept Exception:\n    pass\nfinally_done = True\n");
    let c = Change::of_task(&d, None, &["a.py".to_string()]);
    assert_eq!(at(&scan::regex(&[ready("py-except-pass")], &c)), [("a.py".to_string(), 4)]);
    // a named exception that passes is a choice
    put(&d, "a.py", "x = 1\ntry:\n    os.remove(t)\nexcept FileNotFoundError:\n    pass\n");
    let c = Change::of_task(&d, None, &["a.py".to_string()]);
    assert!(scan::regex(&[ready("py-except-pass")], &c).is_empty());
}

#[test]
fn a_file_git_does_not_know_yet_counts_whole_and_a_deleted_one_has_only_removed_lines() {
    let d = repo("s40f", &[("tests/t.rs", "#[test]\nfn t() {\n    assert_eq!(1, 1);\n}\n")]);
    put(&d, "src.rs", "fn main() { dbg!(1); }\n");
    std::fs::remove_file(d.join("tests/t.rs")).unwrap();
    let c = Change::of_task(&d, None, &["src.rs".to_string(), "tests/t.rs".to_string()]);
    assert_eq!(c.files.len(), 2);
    assert_eq!(at(&scan::regex(&[ready("rs-dbg")], &c)), [("src.rs".to_string(), 1)]);
    assert_eq!(at(&scan::regex(&[ready("assert-removed")], &c)), [("tests/t.rs".to_string(), 3)]);
}

#[test]
fn a_path_is_checked_as_it_stands_and_a_range_as_its_diff() {
    let d = repo("s40g", &[("src/a.rs", "fn a() { x.unwrap(); }\n")]);
    let c = Change::of_path(&d, "src").unwrap();
    assert_eq!(at(&scan::regex(&[ready("rs-unwrap")], &c)), [("src/a.rs".to_string(), 1)], "every line of a path counts as added");
    put(&d, "src/a.rs", "fn a() { x.unwrap(); }\nfn b() { y.unwrap(); }\n");
    git(&d, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qam", "second"]).unwrap();
    let c = Change::of_range(&d, "HEAD~1..HEAD").unwrap();
    assert_eq!(at(&scan::regex(&[ready("rs-unwrap")], &c)), [("src/a.rs".to_string(), 2)], "a range counts only what it adds");
    assert!(Change::of_path(&d, "../elsewhere").is_err() && Change::of_path(&d, "nope").is_err() && Change::of_range(&d, "--output=x").is_err());
}

#[test]
fn two_matches_on_one_line_are_one_mark() {
    let d = repo("s40h", &[("src/a.rs", "fn a() {}\n")]);
    put(&d, "src/a.rs", "fn a() { x.unwrap(); y.unwrap(); }\n");
    let c = Change::of_task(&d, None, &["src/a.rs".to_string()]);
    assert_eq!(at(&scan::regex(&[ready("rs-unwrap")], &c)), [("src/a.rs".to_string(), 1)]);
}

#[test]
fn a_mode_change_has_no_lines_and_a_deleted_file_is_kept_for_its_removed_ones() {
    let text = "diff --git a/run.sh b/run.sh\nold mode 100644\nnew mode 100755\ndiff --git a/gone.py b/gone.py\ndeleted file mode 100644\n--- a/gone.py\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-a = 1\n-b = 2\n";
    let files = j4n_qc::diff::parse(text);
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(files[0].deleted && files[0].path == "gone.py" && files[0].hunks[0].removed.len() == 2);
    assert_eq!(files[0].hunks[0].removed[1].no, 2);
}

#[test]
fn a_path_up_and_out_is_not_a_file_of_the_task_and_a_big_file_and_the_three_hundredth_are_the_limits() {
    let d = repo("s40i", &[("a.txt", "x\n")]);
    assert!(Change::of_task(&d, None, &["../x".to_string(), "/etc/passwd".to_string()]).files.is_empty());
    assert!(Change::of_path(&d, "..").is_err() && Change::of_path(&d, "/etc").is_err());
    std::fs::write(d.join("big.txt"), "x".repeat(1_000_001)).unwrap();
    std::fs::write(d.join("edge.txt"), "x".repeat(1_000_000)).unwrap();
    let c = Change::of_path(&d, ".").unwrap();
    assert!(c.files.iter().any(|f| f.path == "edge.txt") && c.files.iter().all(|f| f.path != "big.txt"), "a megabyte is read, a byte more is not");
    let d2 = repo("s40j", &[("seed.txt", "x\n")]);
    for i in 0..301 {
        std::fs::write(d2.join(format!("f{i:03}.txt")), "x\n").unwrap();
    }
    let c = Change::of_path(&d2, ".").unwrap();
    assert_eq!((c.files.len(), c.cut), (300, 2), "302 files with the seed, 300 kept");
}
