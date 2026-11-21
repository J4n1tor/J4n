//! `j4n pack new | check | install` the words of the command line, every pack the binary carries passing its own check, and
//! installing: a checked pack from a folder or a local git repository appears in the catalog, one that fails is refused.

use crate::support::root;
use j4n::author::install_at;
use j4n::cli::{Cmd, PackCmd, parse};
use j4n_core::pack::check::{check, template};
use j4n_core::pack::packs_dir;
use std::path::{Path, PathBuf};
use std::process::Command;

fn words(s: &str) -> Vec<String> {
    s.split_whitespace().map(String::from).collect()
}

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("j4n-author-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn new_pack(at: &Path, name: &str) -> PathBuf {
    let d = at.join(name);
    for (rel, text) in template(name).unwrap() {
        std::fs::create_dir_all(d.join(&rel).parent().unwrap()).unwrap();
        std::fs::write(d.join(rel), text).unwrap();
    }
    d
}

#[test]
fn the_pack_words_after_pack_are_new_check_and_install_and_the_rest_stay_the_list() {
    let p = |s: &str| parse(&words(s));
    assert_eq!(p("pack new demo"), Ok(Cmd::Pack(PackCmd::New { name: "demo".into(), dir: None })));
    assert_eq!(p("pack new demo here"), Ok(Cmd::Pack(PackCmd::New { name: "demo".into(), dir: Some("here".into()) })));
    assert_eq!(p("pack check ./x"), Ok(Cmd::Pack(PackCmd::Check { path: "./x".into() })));
    assert_eq!(p("pack install https://h/x.git"), Ok(Cmd::Pack(PackCmd::Install { from: "https://h/x.git".into() })));
    assert!(matches!(p("pack"), Ok(Cmd::Word(..))) && matches!(p("pack on x"), Ok(Cmd::Word(..))));
    for bad in ["pack new", "pack check", "pack check a b", "pack install", "pack check --nope a", "pack new a b c"] {
        assert!(p(bad).is_err(), "{bad}");
    }
    assert!(matches!(p("pack check --help"), Ok(Cmd::Help(_))));
}

#[test]
fn every_pack_the_binary_carries_passes_its_own_check() {
    let catalog = j4n::pack::catalog();
    for id in catalog.ids() {
        let r = check(&root().join("packs").join(id), &catalog, false);
        assert!(r.ok(), "packs/{id}: {:#?}", r.found.iter().map(ToString::to_string).collect::<Vec<_>>());
    }
}

#[test]
fn a_checked_pack_is_installed_and_then_is_in_the_catalog_but_off() {
    let (work, into) = (dir("local"), dir("local-into"));
    let pack = new_pack(&work, "mine");
    let m = install_at(pack.to_str().unwrap(), &into).unwrap();
    assert_eq!(m.id, "mine");
    assert!(into.join("mine/pack.toml").is_file() && into.join("mine/tests/hello.toml").is_file(), "the rest of the pack is copied");
    let (catalog, found) = j4n::pack::catalog().with_installed(&into);
    assert!(found.is_empty() && catalog.get("mine").is_some());
    let h = catalog.harness(&j4n::pack::ON, &[]);
    assert!(h.info.iter().any(|i| i.id == "mine" && !i.on), "installing does not turn it on");
    assert!(install_at(pack.to_str().unwrap(), &into).unwrap_err().contains("already there"));
    assert_eq!(packs_dir(Path::new("h")), Path::new("h/packs"));
}

#[test]
fn a_pack_that_fails_its_check_is_refused_and_nothing_is_copied() {
    let (work, into) = (dir("bad"), dir("bad-into"));
    let pack = new_pack(&work, "broken");
    let text = std::fs::read_to_string(pack.join("tests/hello.toml")).unwrap();
    std::fs::write(pack.join("tests/hello.toml"), text.replace("is = \"status\"", "is = \"stop\"")).unwrap();
    let why = install_at(pack.to_str().unwrap(), &into).unwrap_err();
    assert!(why.contains("fails `j4n pack check`"), "{why}");
    assert!(!into.join("broken").exists());
}

#[test]
fn a_name_that_is_neither_a_folder_nor_a_git_url_is_said() {
    let why = install_at("no/such/folder", &dir("none")).unwrap_err();
    assert!(why.contains("not a folder"), "{why}");
    assert!(install_at("--upload-pack=x", &dir("dash")).is_err());
}

fn git(at: &Path, args: &[&str]) {
    let out = Command::new("git").arg("-C").arg(at).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "-c", "protocol.file.allow=always"]).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn a_git_url_is_cloned_shallow_checked_and_installed_and_a_failing_repo_is_refused() {
    let work = dir("git");
    let bare = work.join("remote.git");
    for (name, good) in [("viagit", true), ("badgit", false)] {
        let repo = work.join(name);
        let pack = new_pack(&work, name);
        if !good {
            std::fs::write(pack.join("phrases/hello.toml"), "api = 1\n[[phrase]]\nid = \"badgit\"\nwords = [\"no colon\"]\ncommand = \"status\"\n").unwrap();
        }
        git(&pack, &["init", "--quiet", "-b", "main"]);
        git(&pack, &["add", "-A"]);
        git(&pack, &["commit", "--quiet", "-m", "pack"]);
        let _ = std::fs::remove_dir_all(&bare);
        git(&work, &["clone", "--quiet", "--bare", pack.to_str().unwrap(), bare.to_str().unwrap()]);
        let (url, into) = (format!("file://{}", bare.display()), dir(&format!("git-into-{name}")));
        let _ = repo;
        if good {
            let m = install_at(&url, &into).unwrap();
            assert_eq!(m.id, "viagit");
            assert!(into.join("viagit/pack.toml").is_file() && !into.join("viagit/.git").exists(), "the clone's .git is not copied");
        } else {
            assert!(install_at(&url, &into).unwrap_err().contains("fails"), "words with no colon fail the kind");
            assert!(!into.join("badgit").exists());
        }
    }
}

#[test]
fn a_ref_to_nothing_fails_the_check_and_names_the_row_and_a_ref_to_a_thing_that_is_there_passes() {
    let work = dir("dangling");
    let pack = new_pack(&work, "dang");
    let catalog = j4n::pack::catalog();
    assert!(check(&pack, &catalog, false).ok(), "the pack as `new` makes it passes");
    // a command that starts a flow no pack declares
    std::fs::create_dir_all(pack.join("commands")).unwrap();
    std::fs::write(pack.join("commands/go.toml"), "api = 1\n[[command]]\nid = \"go\"\ndoc = \"Start a flow that is not there.\"\nwords = [\":go\"]\nflow = \"nowhere\"\n").unwrap();
    std::fs::write(pack.join("tests/golden/entries.txt"), "command go\nphrase dang\n").unwrap();
    let r = check(&pack, &catalog, false);
    assert!(!r.ok(), "a pack with a dangling ref fails: {:#?}", r.found.iter().map(ToString::to_string).collect::<Vec<_>>());
    let line = r.found.iter().map(ToString::to_string).find(|l| l.contains("nowhere")).expect("the ref is said");
    assert!(line.contains("commands/go.toml:2") && line.contains("[[command]] go") && line.contains("no flow `nowhere`") && !line.contains("warning"), "{line}");
    assert!(r.summary().contains("1 problem"), "{}", r.summary());
    assert_eq!(j4n::author::run(&j4n::cli::PackCmd::Check { path: pack.to_string_lossy().into_owned() }), 1, "`j4n pack check` exits 1");
    // the same row pointing at a flow of a pack it needs is fine
    std::fs::write(pack.join("commands/go.toml"), "api = 1\n[[command]]\nid = \"go\"\ndoc = \"Start a flow that is there.\"\nwords = [\":go\"]\nflow = \"recall\"\n").unwrap();
    let r = check(&pack, &catalog, false);
    assert!(r.ok(), "{:#?}", r.found.iter().map(ToString::to_string).collect::<Vec<_>>());
}
