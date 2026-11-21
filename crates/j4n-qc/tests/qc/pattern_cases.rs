//! Every seed pattern of the pack: it reads, it compiles, and its pair does what the pair says — the `bad` code is marked and the
//! `good` code is not.

use crate::{dir, ready, seeds, whole};
use j4n_qc::change::{Change, git};
use j4n_qc::diff::{FileDiff, Hunk, Line};
use j4n_qc::pattern::{Ready, Scope, Way, glob};
use j4n_qc::scan;

/// A path the pattern looks at: its first `lang` glob with the wildcards filled in.
fn path_for(r: &Ready) -> String {
    let g = r.p.lang[0].replace("**/", "a/").replace("**", "a").replace('*', "x");
    let found = if g == "x" { ["x.rs", "x.txt"].into_iter().find(|c| r.applies(c)).map(String::from) } else { Some(g.clone()) };
    let g = found.unwrap_or(g);
    assert!(r.applies(&g), "{}: {g} is not a path its own globs take", r.p.id);
    g
}

/// The code of a pair as a change: a removed-scope pair is written as diff lines (`-` for what was taken out), anything else is code
/// that was added.
fn pair(r: &Ready, text: &str) -> Change {
    let path = path_for(r);
    if r.p.scope == Scope::Removed {
        let (mut added, mut removed) = (Vec::new(), Vec::new());
        for (i, l) in text.lines().enumerate() {
            let line = Line { no: i as u32 + 1, text: l[1..].to_string() };
            if l.starts_with('-') { removed.push(line) } else { added.push(line) }
        }
        let mut c = whole(&path, "");
        c.files = vec![FileDiff { path, deleted: false, hunks: vec![Hunk { header: String::new(), added, removed, text: text.to_string() }] }];
        return c;
    }
    whole(&path, text)
}

#[test]
fn the_pack_ships_the_thirty_five_seeds_and_each_one_compiles() {
    let all = seeds();
    assert_eq!(all.len(), 35);
    let ways = |w: Way| all.iter().filter(|p| p.kind == w).count();
    assert_eq!((ways(Way::Regex), ways(Way::Cmd), ways(Way::Jev)), (20, 2, 13));
    for p in all {
        let id = p.id.clone();
        Ready::new(p).unwrap_or_else(|e| panic!("{id}: {e}"));
    }
}

#[test]
fn the_bad_code_of_each_regex_pattern_is_marked_and_the_good_code_is_not() {
    for r in seeds().into_iter().filter(|p| p.kind == Way::Regex).map(|p| Ready::new(p).unwrap()) {
        let bad = scan::regex(std::slice::from_ref(&r), &pair(&r, &r.p.bad));
        assert!(!bad.is_empty(), "{}: its bad code is not marked", r.p.id);
        assert!(bad.iter().all(|m| m.pattern == r.p.id && m.p == 1.0 && m.tier == 0), "{}", r.p.id);
        let good = scan::regex(std::slice::from_ref(&r), &pair(&r, &r.p.good));
        assert!(good.is_empty(), "{}: its good code is marked at {:?}", r.p.id, good.iter().map(|m| m.line).collect::<Vec<_>>());
    }
}

#[test]
fn a_new_dependency_is_marked_at_its_line_and_a_manifest_with_none_is_not() {
    let r = ready("new-dependency");
    let bad = j4n_qc::cmd::run(&r, &whole("Cargo.toml", &r.p.bad)).unwrap();
    assert_eq!(bad.len(), 1);
    assert_eq!((bad[0].file.as_str(), bad[0].line), ("Cargo.toml", 2));
    assert!(j4n_qc::cmd::run(&r, &whole("Cargo.toml", &r.p.good)).unwrap().is_empty());
    // against what the file was, only what is new counts
    let d = dir("deps");
    git(&d, &["init", "-q", "-b", "main"]).unwrap();
    std::fs::write(d.join("package.json"), r#"{"dependencies": {"left-pad": "1"}}"#).unwrap();
    git(&d, &["add", "."]).unwrap();
    git(&d, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "first"]).unwrap();
    std::fs::write(d.join("package.json"), r#"{"dependencies": {"left-pad": "1", "is-odd": "2"}}"#).unwrap();
    let c = Change::of_task(&d, None, &["package.json".to_string()]);
    let marks = j4n_qc::cmd::run(&r, &c).unwrap();
    assert_eq!(marks.len(), 1, "{marks:?}");
    assert_eq!(marks[0].line, 1);
}

#[test]
fn a_formatter_that_is_not_installed_is_skipped_with_its_name_and_never_counted_as_clean() {
    let mut p = seeds().into_iter().find(|p| p.id == "rs-fmt").unwrap();
    p.argv[0] = "no-such-formatter-here".into();
    let r = Ready::new(p).unwrap();
    let why = j4n_qc::cmd::run(&r, &whole("x.rs", "fn f( a:u32 )->u32{a}")).unwrap_err();
    assert!(why.contains("no-such-formatter-here") && why.contains("not installed"), "{why}");
}

/// A folder with `x.rs` committed as `before`, then changed to `after`, as the change of a task.
fn edited(tag: &str, before: &str, after: &str) -> (std::path::PathBuf, Change) {
    let d = dir(tag);
    git(&d, &["init", "-q", "-b", "main"]).unwrap();
    std::fs::write(d.join("x.rs"), before).unwrap();
    git(&d, &["add", "."]).unwrap();
    git(&d, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "first"]).unwrap();
    std::fs::write(d.join("x.rs"), after).unwrap();
    let c = Change::of_task(&d, None, &["x.rs".to_string()]);
    (d, c)
}

/// The pattern `rs-fmt` with `body` as the command that stands in for rustfmt.
fn standing_in(d: &std::path::Path, body: &str) -> Ready {
    let mut p = seeds().into_iter().find(|p| p.id == "rs-fmt").unwrap();
    p.argv = vec![fake(d, "fake-fmt", body), "{files}".into()];
    Ready::new(p).unwrap()
}

#[test]
fn a_formatter_that_names_a_line_in_an_added_one_marks_the_first_that_differs_and_one_far_away_is_not_the_changes() {
    let d = dir("fmt");
    // rustfmt's words: the hunk starts at the first line it shows (context), then the lines it takes out and puts in
    let r = standing_in(&d, "echo 'Diff in x.rs:1:'\necho ' fn a() {}'\necho '-fn b( x:u32 ){x}'\necho '+fn b(x: u32) {'\necho '+    x'\necho '+}'\necho ' fn c() {}'\necho 'Diff in x.rs:90:'\necho ' fn z() {}'\necho '-fn y( ){}'\necho '+fn y() {}'\nexit 1");
    let text = "fn a() {}\nfn b( x:u32 ){x}\nfn c() {}\n";
    std::fs::write(d.join("x.rs"), text).unwrap();
    let c = Change::of_path(&d, "x.rs").unwrap();
    let marks = j4n_qc::cmd::run(&r, &c).unwrap();
    assert_eq!(marks.iter().map(|m| m.line).collect::<Vec<_>>(), [2], "the line that differs, not the context line the hunk starts at");
}

#[test]
fn a_hunk_is_marked_at_its_first_line_in_the_change_and_not_at_a_line_the_change_did_not_add() {
    let before = "fn a() {}\nfn b( x:u32 ){x}\nfn c() {}\nfn d() {}\nfn e() {}\n";
    let after = "fn a() {}\nfn b( x:u32 ){x}\nfn c( ) {}\nfn d() {}\nfn e() {}\n";
    let (d, c) = edited("fmt-old", before, after);
    // one hunk, two lines differ: line 2 was there before the change, line 3 is the change's
    let r = standing_in(&d, "echo 'Diff in x.rs:1:'\necho ' fn a() {}'\necho '-fn b( x:u32 ){x}'\necho '+fn b(x: u32) {'\necho '+    x'\necho '+}'\necho '-fn c( ) {}'\necho '+fn c() {}'\necho ' fn d() {}'\necho ' fn e() {}'\nexit 1");
    let marks = j4n_qc::cmd::run(&r, &c).unwrap();
    assert_eq!(marks.iter().map(|m| m.line).collect::<Vec<_>>(), [3]);
    // the same hunk where nothing differs inside the change: no mark, though the context is
    let (d, c) = edited("fmt-ctx", before, "fn a() {}\nfn b( x:u32 ){x}\nfn c() {}\nfn d() {}\nfn e() {}\n// note\n");
    let r = standing_in(&d, "echo 'Diff in x.rs:1:'\necho ' fn a() {}'\necho '-fn b( x:u32 ){x}'\necho '+fn b(x: u32) {'\necho '+    x'\necho '+}'\necho ' fn c() {}'\necho ' fn d() {}'\necho ' fn e() {}'\nexit 1");
    assert!(j4n_qc::cmd::run(&r, &c).unwrap().is_empty());
}

#[test]
fn lines_only_put_in_are_marked_at_the_line_they_go_in_front_of_and_a_hunk_with_no_lines_names_its_own() {
    let (d, c) = edited("fmt-put", "fn a() {}\n", "fn a() {}\nfn b() {}\nfn c() {}\n");
    // a blank line wanted before line 2: the hunk starts at line 1 (context), the difference is at line 2
    let r = standing_in(&d, "echo 'Diff in x.rs:1:'\necho ' fn a() {}'\necho '+'\necho ' fn b() {}'\necho ' fn c() {}'\nexit 1");
    assert_eq!(j4n_qc::cmd::run(&r, &c).unwrap().iter().map(|m| m.line).collect::<Vec<_>>(), [2]);
    // the old words, a header and nothing under it: the line it says
    let r = standing_in(&d, "echo 'Diff in x.rs at line 3:'\nexit 1");
    assert_eq!(j4n_qc::cmd::run(&r, &c).unwrap().iter().map(|m| m.line).collect::<Vec<_>>(), [3]);
}

#[test]
fn the_real_rustfmt_agrees_with_the_pair_when_it_is_installed() {
    if std::process::Command::new("rustfmt").arg("--version").output().is_err_and(|_| true) {
        return;
    }
    let r = ready("rs-fmt");
    match j4n_qc::cmd::run(&r, &whole("x.rs", &format!("{}\n", r.p.bad))) {
        Ok(m) => assert!(!m.is_empty(), "rustfmt did not mark the bad code"),
        Err(why) => assert!(why.contains("rustfmt"), "{why}"),
    }
    if let Ok(m) = j4n_qc::cmd::run(&r, &whole("x.rs", &format!("{}\n", r.p.good))) {
        assert!(m.is_empty());
    }
}

#[test]
fn the_real_rustfmt_marks_a_function_added_mid_file_at_its_own_line_and_not_three_lines_before() {
    if std::process::Command::new("rustfmt").arg("--version").output().is_err_and(|_| true) {
        return;
    }
    let body = |from: u32, to: u32| (from..=to).map(|i| format!("fn f{i}() {{}}\n")).collect::<String>();
    let before = format!("{}{}", body(1, 20), body(21, 30));
    let after = format!("{}fn g( a:u32 )->u32{{a}}\n{}", body(1, 20), body(21, 30));
    let (_d, c) = edited("fmt-real", &before, &after);
    let marks = j4n_qc::cmd::run(&ready("rs-fmt"), &c).unwrap();
    assert_eq!(marks.iter().map(|m| m.line).collect::<Vec<_>>(), [21], "fn g is the 21st line; rustfmt's hunk starts at 18");
    // a file that was badly formatted before the change, and a change that is formatted: nothing is the change's
    let (_d, c) = edited("fmt-old-real", &format!("{}fn g( a:u32 )->u32{{a}}\n", body(1, 20)), &format!("{}fn g( a:u32 )->u32{{a}}\n{}", body(1, 20), body(21, 22)));
    assert!(j4n_qc::cmd::run(&ready("rs-fmt"), &c).unwrap().is_empty());
}

/// A formatter over a changed file may report what was wrong before the change; only what the change added is a mark.
#[test]
fn a_file_with_an_old_issue_and_a_new_one_marks_only_the_new_one() {
    let body = |from: u32, to: u32| (from..=to).map(|i| format!("fn f{i}() {{}}\n")).collect::<String>();
    let before = format!("{}fn g( a:u32 )->u32{{a}}\n{}", body(1, 10), body(11, 20));
    let after = format!("{before}fn h( b:u32 )->u32{{b}}\n");
    // a stand-in for rustfmt that names both hunks, as rustfmt does: g (line 11, old) and h (line 22, the change's)
    let (d, c) = edited("fmt-both", &before, &after);
    let r = standing_in(&d, "echo 'Diff in x.rs:8:'\necho ' fn f8() {}'\necho ' fn f9() {}'\necho ' fn f10() {}'\necho '-fn g( a:u32 )->u32{a}'\necho '+fn g(a: u32) -> u32 {'\necho '+    a'\necho '+}'\necho ' fn f11() {}'\necho 'Diff in x.rs:19:'\necho ' fn f18() {}'\necho ' fn f19() {}'\necho ' fn f20() {}'\necho '-fn h( b:u32 )->u32{b}'\necho '+fn h(b: u32) -> u32 {'\necho '+    b'\necho '+}'\nexit 1");
    assert_eq!(j4n_qc::cmd::run(&r, &c).unwrap().iter().map(|m| m.line).collect::<Vec<_>>(), [22], "g is line 11 and was there before; h is line 22");
    // the real rustfmt says the same of the same file
    if std::process::Command::new("rustfmt").arg("--version").output().is_err_and(|_| true) {
        return;
    }
    let (_d, c) = edited("fmt-both-real", &before, &after);
    let marks = j4n_qc::cmd::run(&ready("rs-fmt"), &c).unwrap();
    assert_eq!(marks.iter().map(|m| m.line).collect::<Vec<_>>(), [22]);
}

#[test]
fn every_jev_pattern_has_its_question_and_a_choice_names_what_it_may_pick() {
    let all = seeds();
    let jev: Vec<_> = all.iter().filter(|p| p.kind == Way::Jev).collect();
    assert_eq!(jev.len(), 13);
    for p in &jev {
        assert!(p.instructions.as_deref().is_some_and(|i| i.len() > 30), "{}: a question is a sentence", p.id);
        assert!(!p.bad.is_empty() && !p.good.is_empty(), "{}", p.id);
    }
    assert_eq!(jev.iter().filter(|p| p.ty.as_deref() == Some("choice")).map(|p| p.id.as_str()).collect::<Vec<_>>(), ["duplicate-helper"]);
}

#[test]
fn globs_take_a_name_anywhere_a_folder_tree_and_a_file_in_any_folder() {
    let m = |g: &str, p: &str| glob(g).unwrap().is_match(p);
    assert!(m("*.rs", "a.rs") && m("*.rs", "src/deep/a.rs") && !m("*.rs", "a.rs.bak"));
    assert!(m("tests/**", "tests/a/b.rs") && !m("tests/**", "src/tests/a.rs"));
    assert!(m("**/tests/**", "src/tests/a.rs") && m("**/Cargo.toml", "Cargo.toml") && m("**/Cargo.toml", "crates/x/Cargo.toml"));
    assert!(m("*", "anything/at/all.xyz") && m("requirements*.txt", "requirements-dev.txt"));
    assert!(!m("src/*.rs", "src/a/b.rs"));
}

fn fake(d: &std::path::Path, name: &str, body: &str) -> String {
    let f = d.join(name);
    std::fs::write(&f, format!("#!/bin/sh\n{body}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    f.to_string_lossy().into()
}

#[test]
fn a_formatter_that_fails_without_saying_where_is_skipped_and_a_deleted_file_is_not_given_to_it() {
    let d = dir("fmt2");
    std::fs::write(d.join("keep.rs"), "fn a() {}\n").unwrap();
    let args = d.join("args");
    let prog = fake(&d, "fake-fmt", &format!("echo \"$@\" > {}\necho 'error: expected item' >&2\nexit 1", args.display()));
    let mut p = seeds().into_iter().find(|p| p.id == "rs-fmt").unwrap();
    p.argv = vec![prog, "{files}".into()];
    let r = Ready::new(p).unwrap();
    let mut c = Change::of_path(&d, "keep.rs").unwrap();
    c.files.push(FileDiff { path: "gone.rs".into(), deleted: true, hunks: Vec::new() });
    let why = j4n_qc::cmd::run(&r, &c).unwrap_err();
    assert!(why.contains("did not say where") && why.contains("expected item"), "{why}");
    let seen = std::fs::read_to_string(&args).unwrap();
    assert!(seen.contains("keep.rs") && !seen.contains("gone.rs"), "{seen}");
}

#[test]
fn the_dependencies_of_a_pyproject_are_not_python_itself() {
    let r = ready("new-dependency");
    let d = dir("pyproject");
    std::fs::write(d.join("pyproject.toml"), "[tool.poetry.dependencies]\npython = \"^3.11\"\nrequests = \"^2\"\n\n[project]\ndependencies = [\"httpx>=0.2\"]\n").unwrap();
    let c = Change::of_path(&d, "pyproject.toml").unwrap();
    let marks = j4n_qc::cmd::run(&r, &c).unwrap();
    assert_eq!(marks.len(), 2, "requests and httpx, not python: {marks:?}");
}
