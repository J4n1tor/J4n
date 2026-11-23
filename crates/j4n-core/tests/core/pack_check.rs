//! `j4n pack check` and the template of `j4n pack new`: a new pack is green; a broken manifest, a bad kind file, a failing
//! case and a golden that differs each fail naming `file:line`; what a pack needs must be there.

use j4n_core::pack::check::{Report, check, template};
use j4n_core::pack::{Catalog, Pack};
use j4n_core::table::Level;
use std::path::{Path, PathBuf};

fn catalog() -> Catalog {
    let base = "api = 1\nid = \"base\"\nversion = \"0.0.0\"\nsummary = \"the base\"\nneeds = []\nstability = \"experimental\"\n";
    Catalog::new(vec![Pack::builtin(base, &[]).unwrap()])
}

fn new_pack(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("j4n-check-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    for (rel, text) in template("demo").unwrap() {
        let at = d.join(rel);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }
    d
}

fn edit(d: &Path, rel: &str, from: &str, to: &str) {
    let text = std::fs::read_to_string(d.join(rel)).unwrap();
    assert!(text.contains(from), "{rel} has no `{from}`");
    std::fs::write(d.join(rel), text.replacen(from, to, 1)).unwrap();
}

/// The line (from 1) of the first line of `rel` that holds `needle`.
fn line(d: &Path, rel: &str, needle: &str) -> usize {
    std::fs::read_to_string(d.join(rel)).unwrap().lines().position(|l| l.contains(needle)).unwrap() + 1
}

/// The line of the `n`th (from 0) `[[expect]]` of `rel`.
fn expect_line(d: &Path, rel: &str, n: usize) -> usize {
    std::fs::read_to_string(d.join(rel)).unwrap().lines().enumerate().filter(|(_, l)| l.trim() == "[[expect]]").nth(n).unwrap().0 + 1
}

/// The refused findings, each as `path:line: msg` with the pack's folder taken off.
fn refused(d: &Path, r: &Report) -> Vec<String> {
    let cut = format!("{}/", d.display());
    r.found.iter().filter(|f| f.level == Level::Refused).map(|f| f.to_string().replace(&cut, "")).collect()
}

fn says(d: &Path, r: &Report, at: &str, msg: &str) {
    let all = refused(d, r);
    assert!(all.iter().any(|f| f.starts_with(&format!("{at}: ")) && f.contains(msg)), "wanted `{at}: …{msg}…` in {all:#?}");
}

#[test]
fn a_new_pack_is_checked_green_with_its_case_and_its_golden() {
    let d = new_pack("green");
    let r = check(&d, &catalog(), false);
    assert!(r.ok() && r.found.is_empty(), "{:#?}", r.found);
    assert_eq!((r.id.as_str(), r.files, r.entries, r.cases, r.goldens), ("demo", 1, 1, 3, 1));
    assert_eq!(r.summary(), "pack demo: ok (1 files, 1 entries, 3 cases, 1 goldens)");
}

#[test]
fn a_name_that_is_not_a_pack_id_makes_no_template() {
    for bad in ["", "a/b", "a:b", "-x", "has space"] {
        assert!(template(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn a_broken_manifest_is_refused_at_its_line_and_nothing_else_is_read() {
    let d = new_pack("manifest");
    edit(&d, "pack.toml", "stability = \"experimental\"\n", "stability = \"experimental\"\ncolour = \"red\"\n");
    let r = check(&d, &catalog(), false);
    says(&d, &r, &format!("pack.toml:{}", line(&d, "pack.toml", "colour")), "colour");
    assert!(!r.ok() && r.id.is_empty());

    let d = new_pack("manifest2");
    edit(&d, "pack.toml", "api = 1", "api = 7");
    says(&d, &check(&d, &catalog(), false), "pack.toml:1", "api");

    let d = new_pack("manifest3");
    edit(&d, "pack.toml", "version = \"0.1.0\"", "version = \"1\"");
    says(&d, &check(&d, &catalog(), false), &format!("pack.toml:{}", 3), "major.minor.patch");

    let d = new_pack("manifest4");
    std::fs::remove_file(d.join("pack.toml")).unwrap();
    says(&d, &check(&d, &catalog(), false), "pack.toml:1", "no readable pack.toml");
}

#[test]
fn what_a_pack_needs_must_be_a_pack_there_is() {
    let d = new_pack("needs");
    edit(&d, "pack.toml", "needs = [\"base\"]", "needs = [\"base\", \"nobody\"]");
    let r = check(&d, &catalog(), false);
    says(&d, &r, &format!("pack.toml:{}", line(&d, "pack.toml", "needs")), "`nobody`");
}

#[test]
fn a_kind_file_that_does_not_fit_its_kind_is_refused_at_the_entry() {
    let d = new_pack("kind");
    edit(&d, "phrases/hello.toml", "command = \"status\"", "command = \"bogus\"");
    let r = check(&d, &catalog(), false);
    says(&d, &r, &format!("phrases/hello.toml:{}", line(&d, "phrases/hello.toml", "[[phrase]]")), "bogus");
    // the cases that lean on the entry fail too, each at its own line
    says(&d, &r, &format!("tests/hello.toml:{}", expect_line(&d, "tests/hello.toml", 0)), "phrase:demo is not there");

    let d = new_pack("kind2");
    edit(&d, "phrases/hello.toml", "[[phrase]]", "[[nosuchkind]]");
    says(&d, &check(&d, &catalog(), false), &format!("phrases/hello.toml:{}", line(&d, "phrases/hello.toml", "[[nosuchkind]]")), "no kind `nosuchkind`");

    let d = new_pack("kind3");
    edit(&d, "phrases/hello.toml", "words = ", "words == ");
    let n = line(&d, "phrases/hello.toml", "words ==");
    says(&d, &check(&d, &catalog(), false), &format!("phrases/hello.toml:{n}"), "not TOML");
}

#[test]
fn a_case_that_fails_is_refused_at_its_expect_and_the_others_still_run() {
    let d = new_pack("case");
    edit(&d, "tests/hello.toml", "is = \"status\"", "is = \"stop\"");
    edit(&d, "tests/hello.toml", "runs = \"status\"", "runs = \"kill\"");
    let r = check(&d, &catalog(), false);
    says(&d, &r, &format!("tests/hello.toml:{}", expect_line(&d, "tests/hello.toml", 0)), "expected \"stop\"");
    says(&d, &r, &format!("tests/hello.toml:{}", expect_line(&d, "tests/hello.toml", 1)), "runs `status`, expected `kill`");
    assert_eq!(refused(&d, &r).len(), 2, "the golden case held: {:#?}", r.found);
    assert_eq!(r.cases, 3);

    let d = new_pack("case2");
    edit(&d, "tests/hello.toml", "say = ", "sayy = ");
    says(&d, &check(&d, &catalog(), false), &format!("tests/hello.toml:{}", expect_line(&d, "tests/hello.toml", 1)), "unknown key `sayy`");
}

#[test]
fn a_golden_that_differs_is_refused_at_its_line_and_blessing_rewrites_it() {
    let d = new_pack("golden");
    std::fs::write(d.join("tests/golden/entries.txt"), "phrase demo\nphrase old\n").unwrap();
    let r = check(&d, &catalog(), false);
    says(&d, &r, "tests/golden/entries.txt:2", "`phrase old`, the pack gives (nothing)");
    assert!(check(&d, &catalog(), true).ok(), "blessing writes");
    assert_eq!(std::fs::read_to_string(d.join("tests/golden/entries.txt")).unwrap(), "phrase demo\n");
    assert!(check(&d, &catalog(), false).ok());
    std::fs::remove_file(d.join("tests/golden/entries.txt")).unwrap();
    says(&d, &check(&d, &catalog(), false), &format!("tests/hello.toml:{}", expect_line(&d, "tests/hello.toml", 2)), "is not there");
}

#[test]
fn a_kind_can_be_shown_as_the_pack_gives_it() {
    let d = new_pack("show");
    std::fs::write(d.join("tests/shown.toml"), "api = 1\n[[expect]]\nshow = \"phrase\"\ngolden = \"phrase.txt\"\n").unwrap();
    assert!(!check(&d, &catalog(), false).ok());
    assert!(check(&d, &catalog(), true).ok());
    let shown = std::fs::read_to_string(d.join("tests/golden/phrase.txt")).unwrap();
    assert!(shown.starts_with("[[phrase]]\n") && shown.contains("id = \"demo\"") && shown.contains("words = [\":demo\"]"), "{shown}");
    assert!(check(&d, &catalog(), false).ok());
}

#[test]
fn a_tests_file_with_no_expect_is_a_warning_and_does_not_fail_the_pack() {
    let d = new_pack("data");
    std::fs::write(d.join("tests/data.toml"), "api = 1\n[[case]]\nline = \"x\"\n").unwrap();
    let r = check(&d, &catalog(), false);
    assert!(r.ok() && r.found.len() == 1 && r.found[0].level == Level::Warn && r.found[0].to_string().contains("tests/data.toml:1: warning: holds no [[expect]]"), "{:#?}", r.found);
}

#[test]
fn a_packs_scripts_folder_is_data_for_its_scripts_and_not_read_as_kind_files() {
    let d = new_pack("scripts");
    std::fs::create_dir_all(d.join("scripts")).unwrap();
    // what a script reads, in TOML: no `api`, no kind the loader knows
    std::fs::write(d.join("scripts/checks.toml"), "[[check]]\nkind = \"grep\"\nwords = [\"x\"]\n").unwrap();
    std::fs::create_dir_all(d.join("phrases/scripts")).unwrap();
    std::fs::write(d.join("phrases/scripts/more.toml"), "api = 1\n[[phrase]]\nid = \"deeper\"\nwords = [\":deeper\"]\ncommand = \"status\"\n").unwrap();
    // the golden of what the pack gives takes the new phrase in first
    check(&d, &catalog(), true);
    let r = check(&d, &catalog(), false);
    assert!(r.ok() && r.found.is_empty(), "{:#?}", r.found);
    assert_eq!((r.files, r.entries), (2, 2), "only the pack's own root `scripts/` is left out");
    // outside a pack, a folder called `scripts` is read as before
    let (user, _) = j4n_core::table::Layer::from_dir(j4n_core::table::LayerKind::User, "home", &d);
    assert!(user.files.iter().any(|f| f.path == "scripts/checks.toml"));
}

#[test]
fn a_flow_whose_step_model_reads_an_input_it_does_not_declare_is_refused_at_its_line() {
    let d = new_pack("flowmodel");
    std::fs::create_dir_all(d.join("flows")).unwrap();
    let flow = "api = 1\n\n[[flow]]\nid = \"review\"\nin = { goal = \"text\", reader = \"text\" }\nsteps = [{ id = \"read\", do = \"who:read\", goal = \"{in.goal}\", model = \"{in.reader}\" }]\n";
    std::fs::write(d.join("flows/review.toml"), flow).unwrap();
    check(&d, &catalog(), true);
    let r = check(&d, &catalog(), false);
    assert!(r.ok() && r.found.is_empty(), "a model from a declared input is fine: {:#?}", r.found);
    edit(&d, "flows/review.toml", "model = \"{in.reader}\"", "model = \"{in.skeptic}\"");
    let r = check(&d, &catalog(), false);
    says(&d, &r, "flows/review.toml:3", "`in.skeptic` is not an input of this flow");
}
