//! The checkpoint of place `here`, through its public functions. The first cases are the basic checkpoint cases
//! (a task's name is `t12`, the store is `<home>/checkpoints/<key>/<task>/`, a refusal is a string);
//! the rest are what J4n adds: a store that stays out of the project, a store that is not trusted with paths, and a folder
//! that was swapped for a link. A later change added what a checkpoint leaves out (nested repositories, build folders) and the byte cap
//! on a project's store.

use crate::world::Scratch;
use j4n_core::place::checkpoint::{Cfg, MAX_FILES, begin, latest, rewind, stamp, store};
use std::path::{Path, PathBuf};
use std::process::Command;

struct World {
    s: Scratch,
    root: PathBuf,
    home: PathBuf,
}

fn world(git: bool) -> World {
    let s = Scratch::new("cp");
    let (root, home) = (s.join("proj"), s.join("home"));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    if git {
        run(&root, &["init", "-q", "."]);
    }
    World { s, root, home }
}

fn run(root: &Path, args: &[&str]) {
    let st = Command::new("git").args(args).current_dir(root).status().unwrap();
    assert!(st.success(), "git {args:?}");
}

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git").arg("-C").arg(root).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

impl World {
    fn put(&self, rel: &str, text: &str) {
        let p = self.root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn read(&self, rel: &str) -> Option<String> {
        std::fs::read_to_string(self.root.join(rel)).ok()
    }

    fn commit_all(&self) {
        run(&self.root, &["add", "-A"]);
        run(&self.root, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "init"]);
    }

    fn begin(&self, task: &str) -> j4n_core::place::checkpoint::Checkpoint {
        begin(&self.home, &self.root, task, &Cfg::default()).unwrap()
    }

    fn rewind(&self, task: Option<&str>, force: bool) -> j4n_core::place::checkpoint::RewindReport {
        rewind(&self.home, &self.root, task, force).unwrap()
    }
}

/// Wait out the file system's timestamp grain, so "changed" is visible by mtime whatever the size.
fn tick() {
    std::thread::sleep(std::time::Duration::from_millis(30));
}

#[test]
fn a_task_in_a_repository_is_undone_for_tracked_untracked_and_ignored_files_alike() {
    let w = world(true);
    w.put(".gitignore", "ledger/\n");
    w.put("a.txt", "one\n");
    w.put("gone.txt", "bye\n");
    w.put("ledger/old.json", "{\"v\":1}\n");
    w.commit_all();
    w.put("a.txt", "one, edited by the operator before the task\n");
    w.put("untracked.txt", "mine\n");
    let cp = w.begin("t1");
    tick();
    // the task: edits a tracked file, deletes one, writes into and changes the ignored folder, adds a file in a new folder
    w.put("a.txt", "two\n");
    std::fs::remove_file(w.root.join("gone.txt")).unwrap();
    w.put("ledger/found/x.json", "{}\n");
    w.put("ledger/old.json", "{\"v\":2}\n");
    w.put("untracked.txt", "changed\n");
    w.put("out/reports/s.md", "hi\n");
    let f = cp.end().unwrap();
    assert_eq!(f.created, ["ledger/found/x.json", "out/reports/s.md"]);
    assert_eq!(f.modified, ["a.txt", "ledger/old.json", "untracked.txt"]);
    assert_eq!(f.deleted, ["gone.txt"]);
    assert!(f.not_restorable.is_empty(), "{f:?}");
    let report = w.rewind(None, false);
    assert!(report.skipped.is_empty() && report.not_restorable.is_empty(), "{report:?}");
    assert_eq!(w.read("a.txt").as_deref(), Some("one, edited by the operator before the task\n"));
    assert_eq!(w.read("gone.txt").as_deref(), Some("bye\n"));
    assert_eq!(w.read("ledger/old.json").as_deref(), Some("{\"v\":1}\n"));
    assert_eq!(w.read("untracked.txt").as_deref(), Some("mine\n"));
    assert!(w.read("ledger/found/x.json").is_none() && w.read("out/reports/s.md").is_none());
    assert!(!w.root.join("out").exists() && !w.root.join("ledger/found").exists(), "the folders the task made go with its files");
    // git itself was never touched: the operator's pre-task edit is still unstaged, nothing was stashed
    let status = git(&w.root, &["status", "--porcelain"]);
    assert!(status.starts_with("M a.txt") && !status.contains("A  "), "{status}");
    assert_eq!(git(&w.root, &["stash", "list"]), "");
    assert!(git(&w.root, &["for-each-ref", "refs/j4n/checkpoints"]).contains("refs/j4n/checkpoints/t1"));
}

#[test]
fn a_folder_that_is_not_a_repository_is_checkpointed_by_copy() {
    let w = world(false);
    w.put("words.txt", "one\ntwo\n");
    let cp = w.begin("t1");
    tick();
    w.put("words.txt", "one\ntwo\nthree\n");
    w.put("summary.md", "3\n");
    let f = cp.end().unwrap();
    assert_eq!((f.created, f.modified), (vec!["summary.md".to_string()], vec!["words.txt".to_string()]));
    w.rewind(Some("t1"), false);
    assert_eq!(w.read("words.txt").as_deref(), Some("one\ntwo\n"));
    assert!(w.read("summary.md").is_none());
    assert!(!w.root.join(".git").exists(), "nothing made it a repository");
}

#[test]
fn the_skipped_folders_are_neither_walked_nor_reported() {
    let w = world(false);
    w.put("keep.txt", "x\n");
    let cp = w.begin("t1");
    w.put("target/debug/big.bin", "build output\n");
    w.put("node_modules/x/index.js", "dep\n");
    w.put(".j4n/log/0001.jsonl", "{}\n");
    w.put("deep/er/__pycache__/m.pyc", "cache\n");
    let f = cp.end().unwrap();
    assert!(f.is_empty(), "{f:?}");
}

#[test]
fn an_ignored_file_over_the_cap_is_reported_as_not_restorable_and_left_alone() {
    let w = world(true);
    w.put(".gitignore", "data/\n");
    w.put("a.txt", "a\n");
    w.put("data/big.bin", &"x".repeat(5000));
    w.put("data/small.bin", "small\n");
    w.commit_all();
    let cp = begin(&w.home, &w.root, "t1", &Cfg { max_file_bytes: 1000, ..Cfg::default() }).unwrap();
    tick();
    w.put("data/big.bin", "changed\n");
    w.put("data/small.bin", "changed too\n");
    let f = cp.end().unwrap();
    assert_eq!(f.not_restorable, ["data/big.bin"], "{f:?}");
    assert!(f.left_out.iter().any(|l| l.contains("1 file over 1000 bytes")), "the cap says what it skipped: {:?}", f.left_out);
    let r = w.rewind(None, false);
    assert_eq!(r.not_restorable, ["data/big.bin"]);
    assert_eq!(w.read("data/big.bin").as_deref(), Some("changed\n"), "what cannot be put back is not touched");
    assert_eq!(w.read("data/small.bin").as_deref(), Some("small\n"));
    assert!(r.lines().iter().any(|l| l.contains("data/big.bin")), "{:?}", r.lines());
}

#[test]
fn the_total_copied_is_capped_too() {
    let w = world(false);
    w.put("a.bin", &"a".repeat(600));
    w.put("b.bin", &"b".repeat(600));
    let cp = begin(&w.home, &w.root, "t1", &Cfg { max_total_bytes: 1000, ..Cfg::default() }).unwrap();
    tick();
    w.put("a.bin", "changed\n");
    w.put("b.bin", "changed\n");
    let f = cp.end().unwrap();
    assert_eq!(f.not_restorable, ["b.bin"], "the first fits the 1000 bytes, the second does not: {f:?}");
}

#[test]
fn a_file_the_operator_changed_after_the_task_is_left_alone_unless_forced() {
    let w = world(false);
    w.put("a.txt", "before\n");
    let cp = w.begin("t1");
    tick();
    w.put("a.txt", "by the task\n");
    w.put("new.txt", "by the task\n");
    cp.end().unwrap();
    tick();
    w.put("a.txt", "by the operator, later, and longer\n");
    let r = w.rewind(None, false);
    assert_eq!(r.skipped.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(), ["a.txt"], "{r:?}");
    assert_eq!(w.read("a.txt").as_deref(), Some("by the operator, later, and longer\n"));
    assert!(w.read("new.txt").is_none(), "the untouched part of the task is still undone");
    let r = w.rewind(Some("t1"), true);
    assert!(r.skipped.is_empty());
    assert_eq!(w.read("a.txt").as_deref(), Some("before\n"));
}

#[test]
fn a_repository_without_a_commit_is_checkpointed_by_copy_too() {
    let w = world(true);
    w.put("a.txt", "one\n");
    let cp = w.begin("t1");
    tick();
    w.put("a.txt", "two\n");
    cp.end().unwrap();
    w.rewind(None, false);
    assert_eq!(w.read("a.txt").as_deref(), Some("one\n"));
}

#[test]
fn only_the_newest_checkpoints_are_kept_and_an_unsettled_one_is_never_the_latest() {
    let w = world(true);
    w.put("a.txt", "a\n");
    w.commit_all();
    let cfg = Cfg { keep: 2, ..Cfg::default() };
    for n in 1..=4 {
        let cp = begin(&w.home, &w.root, &format!("t{n}"), &cfg).unwrap();
        if n < 4 {
            cp.end().unwrap();
        }
    }
    let mut left: Vec<String> =
        std::fs::read_dir(store(&w.home, &w.root)).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    left.sort();
    assert_eq!(left, ["t3", "t4"]);
    assert_eq!(latest(&w.home, &w.root).as_deref(), Some("t3"), "t4 never settled");
    let refs = git(&w.root, &["for-each-ref", "refs/j4n/checkpoints"]);
    assert!(!refs.contains("/t1") && !refs.contains("/t2") && refs.contains("/t3"), "{refs}");
    let err = rewind(&w.home, &w.root, Some("t4"), false).unwrap_err();
    assert!(err.contains("never settled"), "{err}");
}

#[test]
fn a_checkpoint_still_open_is_never_pruned_so_its_task_settles_with_a_footprint() {
    let w = world(true);
    w.put("a.txt", "a\n");
    w.commit_all();
    let cfg = Cfg { keep: 2, ..Cfg::default() };
    // a long task (a flow) opens first; the short tasks under it begin and settle past what is kept
    let long = begin(&w.home, &w.root, "t1", &cfg).unwrap();
    for n in 2..=4 {
        begin(&w.home, &w.root, &format!("t{n}"), &cfg).unwrap().end().unwrap();
    }
    w.put("b.txt", "made by the long task\n");
    let f = long.end().expect("the open checkpoint's folder is still there");
    drop(long);
    assert_eq!(f.created, ["b.txt"]);
    assert!(store(&w.home, &w.root).join("t1/footprint.json").is_file());
    let mut left: Vec<String> =
        std::fs::read_dir(store(&w.home, &w.root)).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    left.sort();
    assert_eq!(left, ["t1", "t4"], "the settled ones past `keep` went; the open one stayed");
    let r = w.rewind(Some("t1"), false);
    assert_eq!(r.removed, ["b.txt"], "{:?}", r.lines());
    // once let go, it is pruned like any settled one
    begin(&w.home, &w.root, "t5", &cfg).unwrap().end().unwrap();
    assert!(!store(&w.home, &w.root).join("t1").exists());
}

#[test]
fn a_checkpoint_dropped_while_its_task_went_settles_with_what_changed_and_nothing_to_put_back() {
    let w = world(false);
    w.put("a.txt", "a\n");
    let cp = w.begin("t1");
    tick();
    w.put("a.txt", "changed\n");
    w.put("b.txt", "new\n");
    // another J4n on the same home, or a hand, took the folder away
    std::fs::remove_dir_all(store(&w.home, &w.root).join("t1")).unwrap();
    let f = cp.end().expect("no `cannot write the footprint`");
    assert_eq!((f.created.as_slice(), f.modified.as_slice(), f.not_restorable.as_slice()), (&["b.txt".to_string()][..], &["a.txt".to_string()][..], &["a.txt".to_string()][..]));
    assert!(f.left_out.iter().any(|l| l.contains("dropped before the task settled")), "{:?}", f.left_out);
    assert!(!store(&w.home, &w.root).join("t1").exists(), "no footprint is left without its manifest");
    assert_eq!(latest(&w.home, &w.root), None);
}

#[test]
fn a_folder_with_too_many_files_is_refused_a_checkpoint_and_leaves_nothing_behind() {
    let w = world(false);
    let sub = w.root.join("lots");
    std::fs::create_dir_all(&sub).unwrap();
    for i in 0..(MAX_FILES + 5) {
        std::fs::write(sub.join(format!("f{i}")), "").unwrap();
    }
    let err = begin(&w.home, &w.root, "t1", &Cfg::default()).err().expect("refused");
    assert!(err.contains("too many to checkpoint"), "{err}");
    assert!(!store(&w.home, &w.root).join("t1").exists());
}

#[test]
fn j4ns_own_home_inside_the_project_is_not_part_of_a_tasks_footprint() {
    let s = Scratch::new("cp-home");
    let (root, home) = (s.join("proj"), s.join("proj/home-j4n"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(root.join("a.txt"), "x\n").unwrap();
    std::fs::write(home.join("settings.toml"), "a = 1\n").unwrap();
    let cp = begin(&home, &root, "t1", &Cfg::default()).unwrap();
    std::fs::write(home.join("trust.json"), "{}").unwrap();
    assert!(cp.end().unwrap().is_empty());
}

#[test]
fn a_name_that_is_a_path_is_not_a_task_and_a_second_rewind_is_harmless() {
    let w = world(false);
    w.put("a.txt", "before\n");
    let cp = w.begin("t1");
    tick();
    w.put("a.txt", "after\n");
    cp.end().unwrap();
    for bad in ["../../etc", "t1/../t1", "x", "t0", "t", "t01", "T1"] {
        let err = rewind(&w.home, &w.root, Some(bad), false).unwrap_err();
        assert!(err.contains("is not a task"), "{bad}: {err}");
    }
    assert!(begin(&w.home, &w.root, "../x", &Cfg::default()).is_err());
    let r = w.rewind(None, false);
    assert!(r.lines()[0].starts_with("rewound task t1: 1 restored"), "{:?}", r.lines());
    assert_eq!(w.read("a.txt").as_deref(), Some("before\n"));
    // the folder is no longer as the task left it, so a second rewind finds nothing of the task's to undo and says so
    let again = w.rewind(None, false);
    assert!(again.restored.is_empty() && again.removed.is_empty() && again.not_restorable.is_empty(), "{again:?}");
    assert_eq!(w.read("a.txt").as_deref(), Some("before\n"));
    assert!(rewind(&w.home, &w.root, Some("t9"), false).unwrap_err().contains("no checkpoint"));
}

// ---- what J4n adds ----

#[test]
fn the_store_is_outside_the_project_and_the_project_is_left_exactly_as_it_was() {
    let w = world(true);
    w.put("a.txt", "a\n");
    w.commit_all();
    let before = git(&w.root, &["status", "--porcelain", "--ignored"]);
    let cp = w.begin("t1");
    cp.end().unwrap();
    assert_eq!(git(&w.root, &["status", "--porcelain", "--ignored"]), before);
    assert!(store(&w.home, &w.root).join("t1/manifest.json").is_file());
    assert!(!store(&w.home, &w.root).starts_with(&w.root));
    assert!(std::fs::read_dir(&w.root).unwrap().flatten().all(|e| e.file_name() == ".git" || e.file_name() == "a.txt"));
}

#[test]
fn a_task_name_made_again_starts_from_an_empty_store() {
    let w = world(false);
    w.put("old.txt", "x\n");
    w.begin("t1").end().unwrap();
    std::fs::remove_file(w.root.join("old.txt")).unwrap();
    w.put("new.txt", "y\n");
    let cp = w.begin("t1");
    tick();
    w.put("new.txt", "z\n");
    cp.end().unwrap();
    assert!(!store(&w.home, &w.root).join("t1/files/old.txt").exists(), "nothing of the earlier t1 is left");
}

#[test]
fn a_store_that_names_a_path_outside_the_folder_is_not_followed() {
    let w = world(false);
    w.put("a.txt", "before\n");
    let cp = w.begin("t1");
    tick();
    w.put("a.txt", "after\n");
    cp.end().unwrap();
    // someone edits the footprint to point at the home directory's file
    let victim = w.s.join("victim.txt");
    std::fs::write(&victim, "keep\n").unwrap();
    let fp = store(&w.home, &w.root).join("t1/footprint.json");
    let text = std::fs::read_to_string(&fp).unwrap().replace("\"created\":[]", "\"created\":[\"../victim.txt\",\"/etc/hostname\"]");
    std::fs::write(&fp, text).unwrap();
    let r = w.rewind(None, false);
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep\n");
    assert_eq!(r.skipped.len(), 2, "{r:?}");
    assert!(r.skipped.iter().all(|(_, why)| why.contains("not a path inside")));
    assert_eq!(w.read("a.txt").as_deref(), Some("before\n"), "the rest is still put back");
}

#[test]
fn a_folder_the_task_replaced_by_a_link_out_of_the_project_is_not_written_through() {
    let w = world(false);
    w.put("data/x.txt", "before\n");
    let cp = w.begin("t1");
    tick();
    let outside = w.s.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::remove_dir_all(w.root.join("data")).unwrap();
    std::os::unix::fs::symlink(&outside, w.root.join("data")).unwrap();
    cp.end().unwrap();
    tick();
    // the operator touches the link afterwards, so it is theirs now and is left alone
    std::fs::remove_file(w.root.join("data")).unwrap();
    std::os::unix::fs::symlink(&outside, w.root.join("data")).unwrap();
    let r = w.rewind(None, false);
    assert!(std::fs::read_dir(&outside).unwrap().next().is_none(), "nothing was written through the link: {r:?}");
    assert_eq!(r.not_restorable, ["data/x.txt"], "{r:?}");
}

#[test]
fn a_link_where_a_copied_file_was_is_replaced_not_followed() {
    let w = world(false);
    w.put("note.txt", "before\n");
    let cp = w.begin("t1");
    tick();
    let target = w.s.join("target.txt");
    std::fs::write(&target, "elsewhere\n").unwrap();
    std::fs::remove_file(w.root.join("note.txt")).unwrap();
    std::os::unix::fs::symlink(&target, w.root.join("note.txt")).unwrap();
    cp.end().unwrap();
    w.rewind(None, true);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "elsewhere\n");
    assert_eq!(w.read("note.txt").as_deref(), Some("before\n"));
    assert!(!std::fs::symlink_metadata(w.root.join("note.txt")).unwrap().file_type().is_symlink());
}

#[test]
fn a_file_as_big_as_the_cap_is_copied_and_one_byte_over_is_not() {
    let w = world(false);
    w.put("exact.bin", &"e".repeat(100));
    w.put("over.bin", &"o".repeat(101));
    let cp = begin(&w.home, &w.root, "t1", &Cfg { max_file_bytes: 100, ..Cfg::default() }).unwrap();
    tick();
    w.put("exact.bin", "changed\n");
    w.put("over.bin", "changed\n");
    let f = cp.end().unwrap();
    assert_eq!(f.not_restorable, ["over.bin"], "{f:?}");
}

#[test]
fn the_files_that_fill_the_total_cap_exactly_are_all_copied() {
    let w = world(false);
    w.put("a.bin", &"a".repeat(600));
    w.put("b.bin", &"b".repeat(400));
    let cp = begin(&w.home, &w.root, "t1", &Cfg { max_total_bytes: 1000, ..Cfg::default() }).unwrap();
    tick();
    w.put("a.bin", "changed\n");
    w.put("b.bin", "changed\n");
    assert!(cp.end().unwrap().not_restorable.is_empty());
}

#[test]
fn a_tracked_file_is_kept_by_git_and_not_copied_twice() {
    let w = world(true);
    w.put(".gitignore", "ignored/\n");
    w.put("tracked.txt", "t\n");
    w.put("ignored/i.txt", "i\n");
    w.commit_all();
    w.put("untracked.txt", "u\n");
    w.begin("t1");
    let files = store(&w.home, &w.root).join("t1/files");
    assert!(!files.join("tracked.txt").exists(), "git holds it");
    assert!(files.join("ignored/i.txt").exists() && files.join("untracked.txt").exists());
}

#[test]
fn undoing_a_task_that_made_the_only_file_in_the_folder_leaves_the_folder() {
    let w = world(false);
    let cp = w.begin("t1");
    w.put("only.txt", "x\n");
    w.put("deep/er/only.txt", "y\n");
    cp.end().unwrap();
    w.rewind(None, true);
    assert!(w.root.is_dir(), "the project's own folder is not one of the folders the task made");
    assert!(std::fs::read_dir(&w.root).unwrap().next().is_none());
}

#[test]
fn a_place_that_cannot_write_the_footprint_says_what_the_task_changed_is_not_known() {
    use j4n_core::place::{Here, Place, Prepare};
    use j4n_core::task::TaskSpec;
    let w = world(false);
    w.put("a.txt", "x\n");
    let here = Here::new(&w.home, &w.root, Cfg::default());
    let spec = TaskSpec::new("go", "d");
    let site = here.prepare(&Prepare { task: "t1", spec: &spec, root: &w.root }).unwrap();
    assert!(site.notes.is_empty() && site.dir == w.root);
    std::fs::create_dir(store(&w.home, &w.root).join("t1/footprint.json")).unwrap();
    let settled = here.settle(&site);
    assert!(settled.is_empty() && settled.notes.len() == 1 && settled.notes[0].contains("is not known"), "{settled:?}");
    assert!(here.settle(&site).notes.is_empty(), "a site is settled once");
}

// ---- what a checkpoint leaves out ----

/// Every file under `dir`, relative, sorted.
fn tree(dir: &Path) -> Vec<String> {
    fn go(base: &Path, dir: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if e.file_type().unwrap().is_dir() {
                go(base, &p, out);
            } else {
                out.push(p.strip_prefix(base).unwrap().to_string_lossy().to_string());
            }
        }
    }
    let mut out = Vec::new();
    go(dir, dir, &mut out);
    out.sort();
    out
}

#[test]
fn a_checkpoint_in_a_repository_leaves_out_ignored_worktrees_build_output_and_nested_repositories() {
    let w = world(true);
    w.put(".gitignore", ".claude/worktrees/\ntarget/\n.env\n");
    w.put("a.txt", "a\n");
    w.commit_all();
    // a branch's worktree inside the project, git-ignored, with a big file: the case that filled the disk
    run(&w.root, &["worktree", "add", "-q", ".claude/worktrees/w1", "-b", "w1"]);
    w.put(".claude/worktrees/w1/big.bin", &"w".repeat(50_000));
    w.put(".claude/worktrees/w1/target/debug/x", "build\n");
    w.put("target/debug/y", "build\n");
    // another repository the project does not track and git does not ignore (a clone)
    w.put("vendored/lib.txt", "theirs\n");
    run(&w.root.join("vendored"), &["init", "-q", "."]);
    w.put("mine.txt", "mine\n");
    w.put(".env", "KEY=1\n");
    let cp = w.begin("t1");
    let copied = tree(&store(&w.home, &w.root).join("t1/files"));
    assert_eq!(copied, [".env", "mine.txt"], "the project's own untracked and ignored files are copied, nothing of the worktree or the clone or the build");
    let manifest = std::fs::read_to_string(store(&w.home, &w.root).join("t1/manifest.json")).unwrap();
    assert!(!manifest.contains("worktrees/w1/") && !manifest.contains("target/") && !manifest.contains("vendored/lib.txt"), "{manifest}");
    // the task writes in those places: none of it is the task's footprint
    tick();
    w.put(".claude/worktrees/w1/big.bin", "changed\n");
    w.put("target/debug/z", "more\n");
    w.put("vendored/new.txt", "theirs too\n");
    let f = cp.end().unwrap();
    assert!(f.is_empty() && f.not_restorable.is_empty(), "{f:?}");
    assert_eq!(f.left_out.len(), 1, "{:?}", f.left_out);
    assert!(f.left_out[0].contains("2 nested repositories: .claude/worktrees/w1, vendored"), "named, as the build folder `target` (a skipped name) is not: {:?}", f.left_out);
}

#[test]
fn a_task_that_writes_a_tracked_and_an_untracked_file_has_both_put_back() {
    let w = world(true);
    w.put(".gitignore", ".claude/worktrees/\n");
    w.put("tracked.txt", "tracked, before\n");
    w.commit_all();
    w.put("untracked.txt", "untracked, before\n");
    w.put(".claude/worktrees/w1/f.txt", "not the project's\n");
    let cp = w.begin("t1");
    tick();
    w.put("tracked.txt", "tracked, after the task\n");
    w.put("untracked.txt", "untracked, after the task, and longer\n");
    w.put("new/created.txt", "made by the task\n");
    let f = cp.end().unwrap();
    assert_eq!(f.modified, ["tracked.txt", "untracked.txt"]);
    assert_eq!(f.created, ["new/created.txt"]);
    let r = w.rewind(None, false);
    assert!(r.skipped.is_empty() && r.not_restorable.is_empty(), "{r:?}");
    assert_eq!(w.read("tracked.txt").as_deref(), Some("tracked, before\n"));
    assert_eq!(w.read("untracked.txt").as_deref(), Some("untracked, before\n"));
    assert!(w.read("new/created.txt").is_none());
}

#[test]
fn an_ignored_file_of_the_operators_that_a_task_edits_or_deletes_is_put_back() {
    let w = world(true);
    w.put(".gitignore", ".env\nnotes/\ntarget/\n");
    w.put("a.txt", "a\n");
    w.commit_all();
    w.put(".env", "SECRET=1\n");
    w.put("notes/todo.md", "mine\n");
    w.put("notes/old.md", "older\n");
    let cp = w.begin("t1");
    tick();
    w.put(".env", "SECRET=2, and longer\n");
    std::fs::remove_file(w.root.join("notes/old.md")).unwrap();
    w.put("notes/new.md", "made by the task\n");
    w.put("target/debug/x", "build\n");
    let f = cp.end().unwrap();
    assert_eq!((f.modified.as_slice(), f.created.as_slice(), f.deleted.as_slice()), ([".env".to_string()].as_slice(), ["notes/new.md".to_string()].as_slice(), ["notes/old.md".to_string()].as_slice()), "{f:?}");
    assert!(f.not_restorable.is_empty(), "{f:?}");
    let r = w.rewind(None, false);
    assert!(r.skipped.is_empty() && r.not_restorable.is_empty(), "{r:?}");
    assert_eq!(w.read(".env").as_deref(), Some("SECRET=1\n"));
    assert_eq!(w.read("notes/old.md").as_deref(), Some("older\n"));
    assert!(w.read("notes/new.md").is_none());
}

#[test]
fn a_folder_that_is_not_a_repository_skips_nested_repositories_and_build_folders_and_says_so() {
    let w = world(false);
    w.put("keep.txt", "k\n");
    w.put("clone/.git/HEAD", "ref\n");
    w.put("clone/src.txt", "theirs\n");
    w.put("work/wt/.git", "gitdir: elsewhere\n");
    w.put("work/wt/f.txt", "theirs\n");
    w.put("dist/app.js", "built\n");
    w.put("web/.next/cache.bin", "cache\n");
    w.put("big.bin", &"b".repeat(3000));
    let cp = begin(&w.home, &w.root, "t1", &Cfg { max_file_bytes: 1000, ..Cfg::default() }).unwrap();
    assert_eq!(tree(&store(&w.home, &w.root).join("t1/files")), ["keep.txt"]);
    let f = cp.end().unwrap();
    assert!(f.is_empty(), "{f:?}");
    let said = f.left_out.join("\n");
    assert!(said.contains("2 nested repositories: clone, work/wt"), "{said}");
    assert!(said.contains("2 build folders: dist, web/.next"), "{said}");
    assert!(said.contains("1 file over 1000 bytes"), "{said}");
}

#[test]
fn a_project_keeps_checkpoints_up_to_a_byte_cap_dropping_the_oldest_settled_first() {
    let w = world(false);
    w.put("data.bin", &"x".repeat(3000));
    let cfg = Cfg { max_store_bytes: 5000, ..Cfg::default() };
    let names = || {
        let mut v: Vec<String> = std::fs::read_dir(store(&w.home, &w.root)).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        v.sort();
        v
    };
    // t1 never settles (its task is still going), t2 and t3 settle
    begin(&w.home, &w.root, "t1", &cfg).unwrap();
    begin(&w.home, &w.root, "t2", &cfg).unwrap().end().unwrap();
    assert_eq!(names(), ["t1", "t2"], "t1 is not settled, so it is not the one to go");
    begin(&w.home, &w.root, "t3", &cfg).unwrap();
    assert_eq!(names(), ["t1", "t3"], "the oldest settled one went, the one begun now stays");
    // a project far under the cap loses nothing
    let w2 = world(false);
    w2.put("a.txt", "a\n");
    for n in 1..=3 {
        begin(&w2.home, &w2.root, &format!("t{n}"), &Cfg::default()).unwrap().end().unwrap();
    }
    assert_eq!(std::fs::read_dir(store(&w2.home, &w2.root)).unwrap().count(), 3);
}

#[test]
fn the_stamp_of_a_folder_changes_with_its_files_and_not_with_what_a_test_run_leaves_in_its_caches() {
    for git in [false, true] {
        let w = world(git);
        std::fs::write(w.root.join("a.py"), "x = 1\n").unwrap();
        let first = stamp(&w.home, &w.root);
        assert_eq!(stamp(&w.home, &w.root), first, "the same files, the same stamp (git {git})");
        for cache in ["__pycache__/a.cpython-314.pyc", ".pytest_cache/v/cache/lastfailed"] {
            std::fs::create_dir_all(w.root.join(cache).parent().unwrap()).unwrap();
            std::fs::write(w.root.join(cache), "cache").unwrap();
        }
        assert_eq!(stamp(&w.home, &w.root), first, "a test run's caches are not the folder's files (git {git})");
        std::fs::write(w.root.join("b.py"), "y = 2\n").unwrap();
        let added = stamp(&w.home, &w.root);
        assert_ne!(added, first, "a file added (git {git})");
        tick();
        std::fs::write(w.root.join("a.py"), "x = 10\n").unwrap();
        assert_ne!(stamp(&w.home, &w.root), added, "a file rewritten (git {git})");
    }
}
