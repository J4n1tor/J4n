//! Place `worktree` on real git repositories in temp folders: where it is cut, what it takes back, what it keeps and says, and
//! what `rewind` does with an apply (the same through the binary is in `j4n`'s `laws`).

use crate::world::Scratch;
use j4n_connect::place::{Apply, Worktree};
use j4n_core::place::checkpoint::{self, Cfg};
use j4n_core::place::{Place, Prepare, Settled, Shared, Site, Take, Taken};
use j4n_core::task::{Mode, TaskSpec};
use std::path::Path;
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git").arg("-C").arg(root).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

struct World {
    project: Scratch,
    home: Scratch,
}

impl World {
    /// A repository with a committed `a.txt`, `src/main.rs` and a `.gitignore` for `ledger/`.
    fn new(tag: &str) -> World {
        let w = World { project: Scratch::new(&format!("{tag}-p")), home: Scratch::new(&format!("{tag}-h")) };
        git(w.project.path(), &["init", "-q", "-b", "main"]);
        w.project.put("a.txt", "one\n");
        w.project.put("src/main.rs", "fn main() {}\n");
        w.project.put(".gitignore", "ledger/\ntarget/\n.j4n/\n");
        git(w.project.path(), &["add", "-A"]);
        git(w.project.path(), &["commit", "-qm", "base"]);
        w
    }

    fn place(&self, apply: Apply) -> Worktree {
        Worktree::new(self.home.path(), self.project.path(), apply, Cfg::default())
    }

    fn prepare(&self, place: &Worktree, task: &str) -> Site {
        let spec = TaskSpec::new("go", "d");
        place.prepare(&Prepare { task, spec: &spec, root: self.project.path() }).unwrap()
    }

    fn status(&self) -> String {
        git(self.project.path(), &["status", "--porcelain"])
    }
}

fn names(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

#[test]
fn a_worktree_is_cut_from_the_latest_commit_outside_the_project_and_the_project_is_untouched() {
    let w = World::new("cut");
    let place = w.place(Apply::Auto);
    let site = w.prepare(&place, "t1");
    assert!(site.dir.starts_with(w.home.path().join("worktrees")), "{:?}", site.dir);
    assert!(!site.dir.starts_with(w.project.path()));
    assert_eq!(std::fs::read_to_string(site.dir.join("a.txt")).unwrap(), "one\n");
    assert!(site.notes[0].starts_with("works in a copy of its own, cut from "), "{:?}", site.notes);
    assert_eq!(site.also.len(), 1, "the git directory is writable too: the copy's metadata lives there");
    assert!(site.also[0].ends_with(".git"), "{:?}", site.also);
    assert_eq!(git(&site.dir, &["rev-parse", "--abbrev-ref", "HEAD"]), "j4n/t1");
    assert_eq!(w.status(), "", "the project's folder was not touched");
    assert!(place.site_of("t1").is_some_and(|s| s.dir == site.dir));
}

#[test]
fn a_folder_with_uncommitted_changes_says_they_are_not_in_the_copy() {
    let w = World::new("dirty");
    w.project.put("a.txt", "one\nedited\n");
    let site = w.prepare(&w.place(Apply::Auto), "t1");
    assert!(site.notes.iter().any(|n| n.contains("not committed")), "{:?}", site.notes);
    assert_eq!(std::fs::read_to_string(site.dir.join("a.txt")).unwrap(), "one\n");
}

#[test]
fn a_folder_that_is_no_repository_or_has_no_commit_is_refused_with_the_way_out() {
    let none = Scratch::new("norepo-p");
    let home = Scratch::new("norepo-h");
    let place = Worktree::new(home.path(), none.path(), Apply::Auto, Cfg::default());
    let spec = TaskSpec::new("go", "d");
    let err = place.prepare(&Prepare { task: "t1", spec: &spec, root: none.path() }).unwrap_err();
    assert!(err.msg.contains("has no commit yet") && err.msg.contains("first commit"), "{}", err.msg);
    git(none.path(), &["init", "-q", "-b", "main"]);
    let err = place.prepare(&Prepare { task: "t1", spec: &spec, root: none.path() }).unwrap_err();
    assert!(err.msg.contains("has no commit yet"), "{}", err.msg);
    let err = place.prepare(&Prepare { task: "../x", spec: &spec, root: none.path() }).unwrap_err();
    assert!(err.msg.contains("is not a task"), "{}", err.msg);
}

#[test]
fn a_task_that_changed_nothing_leaves_no_worktree_and_says_nothing() {
    let w = World::new("nothing");
    let place = w.place(Apply::Auto);
    let site = w.prepare(&place, "t1");
    let settled = place.settle(&site);
    assert_eq!(settled, Settled::default());
    assert!(!site.dir.exists());
    assert!(place.site_of("t1").is_none());
    assert!(git(w.project.path(), &["branch", "--list", "j4n/t1"]).is_empty(), "the branch went with it");
    assert!(!w.home.path().join("worktrees").read_dir().unwrap().flatten().any(|e| e.path().is_dir() && e.path().read_dir().unwrap().count() > 0), "no record is left either");
}

#[test]
fn what_a_task_edited_added_and_deleted_lands_in_the_folder_uncommitted_and_the_copy_is_backed_up_and_removed() {
    let w = World::new("auto");
    let place = w.place(Apply::Auto);
    let site = w.prepare(&place, "t1");
    std::fs::write(site.dir.join("src/main.rs"), "fn main() { println!(\"chess\"); }\n").unwrap();
    std::fs::write(site.dir.join("new.txt"), "brand new\n").unwrap();
    std::fs::remove_file(site.dir.join("a.txt")).unwrap();
    std::fs::create_dir_all(site.dir.join(".j4n")).unwrap();
    std::fs::write(site.dir.join(".j4n/ledger.jsonl"), "{}\n").unwrap();
    let settled = place.settle(&site);
    let work = settled.applied.clone().unwrap_or_else(|| panic!("{settled:?}"));
    assert_eq!(names(&work.files), ["a.txt", "new.txt", "src/main.rs"]);
    assert_eq!((names(&settled.created), names(&settled.modified), names(&settled.deleted)), (vec!["new.txt"], vec!["src/main.rs"], vec!["a.txt"]));
    assert!(settled.kept.is_none(), "{settled:?}");
    assert_eq!((work.added, work.removed), (2, 2), "one line changed, one file made of one line, one file of one line gone: {work:?}");
    assert!(std::fs::read_to_string(w.project.join("src/main.rs")).unwrap().contains("chess"));
    assert!(w.project.join("new.txt").exists() && !w.project.join("a.txt").exists());
    assert!(!w.project.join(".j4n").exists(), "J4n's own folder is not the task's work");
    assert_eq!(git(w.project.path(), &["log", "--oneline"]).lines().count(), 1, "nothing was committed");
    assert!(!site.dir.exists() && place.site_of("t1").is_none());
    assert!(git(w.project.path(), &["branch", "--list", "j4n/t1"]).is_empty());
    assert!(!git(w.project.path(), &["rev-parse", "refs/j4n/backups/t1"]).is_empty(), "the copy is kept reachable");
}

#[test]
fn a_file_git_ignores_comes_back_with_the_patch_and_build_output_does_not() {
    let w = World::new("ignored");
    let place = w.place(Apply::Auto);
    let site = w.prepare(&place, "t1");
    std::fs::create_dir_all(site.dir.join("ledger/found")).unwrap();
    std::fs::write(site.dir.join("ledger/found/x.json"), "{}\n").unwrap();
    std::fs::create_dir_all(site.dir.join("target/debug")).unwrap();
    std::fs::write(site.dir.join("target/debug/junk"), "build output\n").unwrap();
    let settled = place.settle(&site);
    let work = settled.applied.clone().expect("applied: a change that is only ignored files is a change");
    assert_eq!((names(&work.files), names(&work.ignored)), (vec![], vec!["ledger/found/x.json"]));
    assert_eq!(std::fs::read_to_string(w.project.join("ledger/found/x.json")).unwrap(), "{}\n");
    assert!(!w.project.join("target/debug/junk").exists());
    assert_eq!(names(&settled.created), ["ledger/found/x.json"]);
    assert!(work.counts().contains("1 ignored by git"), "{}", work.counts());
}

#[test]
fn caches_a_task_made_in_a_project_with_no_gitignore_stay_out_of_the_patch_and_its_files_land() {
    let w = World::new("caches");
    let place = w.place(Apply::Auto);
    let site = w.prepare(&place, "t1");
    std::fs::create_dir_all(site.dir.join("chess/__pycache__")).unwrap();
    std::fs::write(site.dir.join("chess/pieces.py"), "A = 1\n").unwrap();
    std::fs::write(site.dir.join("chess/__pycache__/pieces.cpython-314.pyc"), "cache").unwrap();
    std::fs::create_dir_all(site.dir.join("__pycache__")).unwrap();
    std::fs::write(site.dir.join("__pycache__/x.pyc"), "cache").unwrap();
    let settled = place.settle(&site);
    let work = settled.applied.clone().expect("applied");
    assert_eq!(names(&work.files), ["chess/pieces.py"], "{work:?}");
    assert!(w.project.join("chess/pieces.py").exists() && !w.project.join("chess/__pycache__").exists() && !w.project.join("__pycache__").exists());
    assert_eq!(names(&settled.created), ["chess/pieces.py"]);
}

#[test]
fn work_the_operator_has_in_the_same_file_is_never_overwritten_and_the_copy_is_kept_with_the_reason() {
    let w = World::new("busy");
    let place = w.place(Apply::Auto);
    let site = w.prepare(&place, "t1");
    std::fs::write(site.dir.join("src/main.rs"), "fn main() { /* task */ }\n").unwrap();
    std::fs::create_dir_all(site.dir.join("ledger")).unwrap();
    std::fs::write(site.dir.join("ledger/x.json"), "{}\n").unwrap();
    w.project.put("src/main.rs", "fn main() { /* mine */ }\n");
    let settled = place.settle(&site);
    let kept = settled.kept.clone().expect("kept");
    assert!(kept.why.starts_with("not applied: ") && kept.why.contains("src/main.rs") && kept.why.contains("nothing was laid over"), "{}", kept.why);
    assert_eq!((names(&kept.work.files), names(&kept.work.ignored)), (vec!["src/main.rs"], vec!["ledger/x.json"]), "the file git ignores is named too");
    assert!(settled.applied.is_none());
    assert!(std::fs::read_to_string(w.project.join("src/main.rs")).unwrap().contains("mine"));
    assert!(!w.project.join("ledger").exists(), "the patch waited for the whole change to be safe");
    assert!(site.dir.exists(), "never reaped with work in it");
    // when the operator has dealt with their file, the work can still be taken
    git(w.project.path(), &["checkout", "--", "src/main.rs"]);
    let taken = place.take("t1", Take::Apply).unwrap();
    assert!(matches!(&taken, Taken::Clean(w, _) if w.files == ["src/main.rs"]), "{taken:?}");
    assert!(std::fs::read_to_string(w.project.join("src/main.rs")).unwrap().contains("task"));
    assert!(w.project.join("ledger/x.json").exists());
    assert!(!site.dir.exists());
}

#[test]
fn a_file_git_ignores_that_the_folder_already_has_with_other_content_is_the_operators() {
    let w = World::new("ignored-busy");
    let place = w.place(Apply::Auto);
    let site = w.prepare(&place, "t1");
    std::fs::create_dir_all(site.dir.join("ledger")).unwrap();
    std::fs::write(site.dir.join("ledger/x.json"), "task's\n").unwrap();
    std::fs::write(site.dir.join("a.txt"), "two\n").unwrap();
    w.project.put("ledger/x.json", "operator's own\n");
    let settled = place.settle(&site);
    let kept = settled.kept.expect("kept");
    assert!(kept.why.contains("ledger/x.json") && kept.why.contains("other content"), "{}", kept.why);
    assert_eq!(std::fs::read_to_string(w.project.join("ledger/x.json")).unwrap(), "operator's own\n");
    assert_eq!(std::fs::read_to_string(w.project.join("a.txt")).unwrap(), "one\n", "and nothing else was applied");
}

#[test]
fn a_patch_that_does_not_apply_is_kept_and_the_folder_is_left_alone() {
    let w = World::new("conflict");
    let place = w.place(Apply::Auto);
    let site = w.prepare(&place, "t1");
    std::fs::write(site.dir.join("a.txt"), "one\ntask line\n").unwrap();
    // the project moved on and committed: a.txt no longer looks like what the copy was cut from
    w.project.put("a.txt", "totally different\n");
    git(w.project.path(), &["commit", "-qam", "moved on"]);
    let settled = place.settle(&site);
    let kept = settled.kept.expect("kept");
    assert!(kept.why.contains("does not apply"), "{}", kept.why);
    assert_eq!(std::fs::read_to_string(w.project.join("a.txt")).unwrap(), "totally different\n");
    assert_eq!(w.status(), "");
}

#[test]
fn a_held_piece_stays_out_of_the_folder_whatever_apply_says_and_pieces_lay_one_at_a_time_the_overlapping_one_is_blocked() {
    let w = World::new("held");
    let place = w.place(Apply::Auto);
    let hold = |task: &str| {
        let mut spec = TaskSpec::new("go", "d");
        spec.input = serde_json::json!({ "hold": true });
        place.prepare(&Prepare { task, spec: &spec, root: w.project.path() }).unwrap()
    };
    let (one, two) = (hold("t1"), hold("t2"));
    std::fs::write(one.dir.join("a.txt"), "from one\n").unwrap();
    std::fs::write(two.dir.join("a.txt"), "from two\n").unwrap();
    let kept = place.settle(&one).kept.expect("held, not laid in");
    assert!(kept.why.starts_with("waiting to be applied"), "{}", kept.why);
    assert_eq!(w.status(), "", "work.apply = auto does not lay a held piece in");
    assert!(place.settle(&two).kept.is_some());
    // the merge: the first lays in; the second changed what is now the folder's, so it is not forced
    assert!(matches!(place.take("t1", Take::Apply).unwrap(), Taken::Clean(..)));
    assert_eq!(std::fs::read_to_string(w.project.join("a.txt")).unwrap(), "from one\n");
    match place.take("t2", Take::Apply).unwrap() {
        Taken::Blocked { why, work } => assert!(why.contains("a.txt") && names(&work.files) == ["a.txt"], "{why}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(std::fs::read_to_string(w.project.join("a.txt")).unwrap(), "from one\n", "and the folder is as the first left it");
    assert!(place.site_of("t2").is_some(), "its worktree is kept, to go back as a diff");
}

#[test]
fn a_file_the_folder_already_holds_as_the_piece_made_it_is_no_clash_and_one_with_other_content_still_is() {
    // two parts each make the same empty `src/__init__.py`; the second must not be refused for it
    let w = World::new("alike");
    let place = w.place(Apply::Auto);
    let hold = |task: &str| {
        let mut spec = TaskSpec::new("go", "d");
        spec.input = serde_json::json!({ "hold": true });
        place.prepare(&Prepare { task, spec: &spec, root: w.project.path() }).unwrap()
    };
    let (one, two, three, four) = (hold("t1"), hold("t2"), hold("t3"), hold("t4"));
    for (site, own) in [(&one, "one.py"), (&two, "two.py"), (&three, "three.py"), (&four, "four.py")] {
        std::fs::create_dir_all(site.dir.join("pkg")).unwrap();
        std::fs::write(site.dir.join("pkg/__init__.py"), "").unwrap();
        std::fs::write(site.dir.join("pkg").join(own), "x = 1\n").unwrap();
    }
    std::fs::write(three.dir.join("pkg/__init__.py"), "import os\n").unwrap();
    std::fs::write(four.dir.join("pkg/__init__.py"), "import sys\n").unwrap();
    for site in [&one, &two, &three, &four] {
        assert!(place.settle(site).kept.is_some());
    }
    assert!(matches!(place.take("t1", Take::Apply).unwrap(), Taken::Clean(..)));
    let second = place.take("t2", Take::Apply).unwrap();
    assert!(matches!(second, Taken::Clean(..)), "{second:?}");
    assert!(w.project.join("pkg/one.py").exists() && w.project.join("pkg/two.py").exists());
    assert_eq!(std::fs::read_to_string(w.project.join("pkg/__init__.py")).unwrap(), "");
    // an empty file git does not track holds nothing to lose: the piece's own is laid over it
    let third = place.take("t3", Take::Apply).unwrap();
    assert!(matches!(third, Taken::Clean(..)), "{third:?}");
    assert_eq!(std::fs::read_to_string(w.project.join("pkg/__init__.py")).unwrap(), "import os\n");
    match place.take("t4", Take::Apply).unwrap() {
        Taken::Blocked { why, .. } => assert!(why.contains("pkg/__init__.py"), "{why}"),
        other => panic!("other content is still a clash: {other:?}"),
    }
    assert!(!w.project.join("pkg/four.py").exists(), "nothing of it was laid in part");
}

#[test]
fn with_apply_off_the_copy_waits_says_whether_it_would_apply_and_a_look_changes_nothing() {
    let w = World::new("off");
    let place = w.place(Apply::Off);
    let site = w.prepare(&place, "t1");
    std::fs::write(site.dir.join("a.txt"), "two\n").unwrap();
    let settled = place.settle(&site);
    let kept = settled.kept.expect("kept");
    assert_eq!(kept.why, "waiting to be applied; it would apply cleanly");
    assert_eq!(names(&kept.work.files), ["a.txt"]);
    assert_eq!(w.status(), "");
    let looked = place.take("t1", Take::Look).unwrap();
    assert!(matches!(looked, Taken::Clean(..)));
    assert_eq!((w.status(), std::fs::read_to_string(w.project.join("a.txt")).unwrap()), (String::new(), "one\n".to_string()));
    // and when it would not apply, that is said while it waits
    let second = w.prepare(&place, "t2");
    std::fs::write(second.dir.join("a.txt"), "three\n").unwrap();
    w.project.put("a.txt", "mine\n");
    let kept = place.settle(&second).kept.expect("kept");
    assert!(kept.why.starts_with("waiting to be applied; it does not apply now: ") && kept.why.contains("a.txt"), "{}", kept.why);
}

#[test]
fn an_apply_can_be_rewound_it_restores_what_it_changed_and_removes_what_it_made() {
    let w = World::new("rewind");
    let place = w.place(Apply::Off);
    let site = w.prepare(&place, "t1");
    std::fs::write(site.dir.join("a.txt"), "two\n").unwrap();
    std::fs::write(site.dir.join("new.txt"), "n\n").unwrap();
    std::fs::create_dir_all(site.dir.join("ledger")).unwrap();
    std::fs::write(site.dir.join("ledger/x.json"), "{}\n").unwrap();
    assert!(place.settle(&site).kept.is_some());
    let taken = place.take("t1", Take::Apply).unwrap();
    assert!(matches!(&taken, Taken::Clean(_, notes) if notes.is_empty()), "{taken:?}");
    assert_eq!(std::fs::read_to_string(w.project.join("a.txt")).unwrap(), "two\n");
    assert!(w.project.join("new.txt").exists() && w.project.join("ledger/x.json").exists());
    let report = checkpoint::rewind(w.home.path(), w.project.path(), Some("t1"), false).unwrap();
    assert_eq!(report.task, "t1");
    assert_eq!(std::fs::read_to_string(w.project.join("a.txt")).unwrap(), "one\n");
    assert!(!w.project.join("new.txt").exists() && !w.project.join("ledger/x.json").exists());
    assert_eq!(w.status(), "");
}

#[test]
fn work_laid_in_and_rewound_can_be_taken_again_from_its_backup_and_lands_again() {
    let w = World::new("relay");
    let place = w.place(Apply::Off);
    let site = w.prepare(&place, "t1");
    std::fs::write(site.dir.join("a.txt"), "two\n").unwrap();
    std::fs::write(site.dir.join("new.txt"), "n\n").unwrap();
    std::fs::remove_file(site.dir.join("src/main.rs")).unwrap();
    assert!(place.settle(&site).kept.is_some());
    assert!(matches!(place.take("t1", Take::Apply).unwrap(), Taken::Clean(..)));
    assert!(place.site_of("t1").is_none(), "the copy is removed once its work is laid in");
    checkpoint::rewind(w.home.path(), w.project.path(), Some("t1"), false).unwrap();
    assert_eq!(w.status(), "", "the lay-in is taken out");
    match place.take("t1", Take::Look).unwrap() {
        Taken::Clean(work, _) => assert_eq!(names(&work.files), ["a.txt", "new.txt", "src/main.rs"], "cut again, it holds the same work"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(place.take("t1", Take::Apply).unwrap(), Taken::Clean(..)));
    assert_eq!(std::fs::read_to_string(w.project.join("a.txt")).unwrap(), "two\n");
    assert!(w.project.join("new.txt").exists() && !w.project.join("src/main.rs").exists(), "{}", w.status());
    checkpoint::rewind(w.home.path(), w.project.path(), Some("t1"), false).unwrap();
    assert_eq!(w.status(), "", "and the second lay-in can be rewound as the first");
    let fresh = w.place(Apply::Off);
    let err = fresh.take("t2", Take::Look).unwrap_err();
    assert!(err.msg.contains("is gone"), "a task none of whose work was laid in has nothing to cut again: {}", err.msg);
}

#[test]
fn a_committed_task_a_binary_file_and_an_odd_name_all_land() {
    let w = World::new("shapes");
    let place = w.place(Apply::Auto);
    let site = w.prepare(&place, "t1");
    std::fs::write(site.dir.join("committed.txt"), "in the copy's own history\n").unwrap();
    git(&site.dir, &["add", "-A"]);
    git(&site.dir, &["commit", "-qm", "task work"]);
    let bytes: Vec<u8> = (0..=255u8).cycle().take(4000).collect();
    std::fs::write(site.dir.join("blob.bin"), &bytes).unwrap();
    std::fs::write(site.dir.join("café \"x\".txt"), "odd\n").unwrap();
    let settled = place.settle(&site);
    let work = settled.applied.expect("applied");
    assert!(work.files.contains(&"café \"x\".txt".to_string()), "{:?}: names come back as they are", work.files);
    assert_eq!(std::fs::read_to_string(w.project.join("committed.txt")).unwrap(), "in the copy's own history\n");
    assert_eq!(std::fs::read(w.project.join("blob.bin")).unwrap(), bytes);
    assert!(w.project.join("café \"x\".txt").exists());
}

#[test]
fn a_record_that_names_no_commit_is_not_handed_to_git_as_one() {
    let w = World::new("tamper");
    let place = w.place(Apply::Off);
    let site = w.prepare(&place, "t1");
    std::fs::write(site.dir.join("a.txt"), "two\n").unwrap();
    let record = std::fs::read_dir(w.home.path().join("worktrees")).unwrap().flatten().next().unwrap().path().join("t1.json");
    let victim = w.project.put("victim.txt", "keep me\n");
    let text = std::fs::read_to_string(&record).unwrap();
    let tampered = with_base(&text, &format!("--output={}", victim.display()));
    std::fs::write(&record, tampered).unwrap();
    // a fresh place reads the record from disk
    let other = w.place(Apply::Off);
    let err = other.take("t1", Take::Apply).unwrap_err();
    assert!(err.msg.contains("not a commit id"), "{}", err.msg);
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep me\n");
}

/// The `base` value of the record, replaced.
fn with_base(json: &str, with: &str) -> String {
    let mut v: serde_json::Value = serde_json::from_str(json).unwrap();
    v["base"] = serde_json::Value::String(with.to_string());
    v.to_string()
}

#[test]
fn what_a_crash_leaves_is_found_again_and_can_be_taken() {
    let w = World::new("recover");
    let place = w.place(Apply::Auto);
    let site = w.prepare(&place, "t1");
    std::fs::write(site.dir.join("a.txt"), "two\n").unwrap();
    let idle = w.prepare(&place, "t2");
    let _ = idle;
    // J4n is gone: a new one knows nothing but the disk
    let after = w.place(Apply::Auto);
    let found = after.recover();
    assert_eq!(found.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(), ["t1"], "a copy with nothing in it is not work");
    assert!(found[0].1.why.contains("stopped while the task ran"), "{:?}", found[0].1);
    assert_eq!(names(&found[0].1.work.files), ["a.txt"]);
    assert!(after.site_of("t1").is_some());
    let taken = after.take("t1", Take::Apply).unwrap();
    assert!(matches!(taken, Taken::Clean(..)));
    assert_eq!(std::fs::read_to_string(w.project.join("a.txt")).unwrap(), "two\n");
    assert!(after.recover().is_empty());
}

#[test]
fn a_task_name_made_again_starts_from_nothing_and_a_read_task_is_told_it_may_not_write() {
    let w = World::new("again");
    let place = w.place(Apply::Off);
    let first = w.prepare(&place, "t1");
    std::fs::write(first.dir.join("a.txt"), "old attempt\n").unwrap();
    let second = w.prepare(&place, "t1");
    assert_eq!(std::fs::read_to_string(second.dir.join("a.txt")).unwrap(), "one\n");
    let mut spec = TaskSpec::new("go", "d");
    spec.tune.mode = Mode::Read;
    let read = place.prepare(&Prepare { task: "t2", spec: &spec, root: w.project.path() }).unwrap();
    assert!(read.read_only && !second.read_only);
}

#[test]
fn asking_for_the_work_of_a_task_with_no_copy_is_refused() {
    let w = World::new("gone");
    let place = w.place(Apply::Auto);
    let err = place.take("t7", Take::Look).unwrap_err();
    assert!(err.msg.contains("is gone"), "{}", err.msg);
    assert!(place.take("../etc", Take::Look).is_err());
    assert!(place.site_of("t7").is_none());
}

#[test]
fn apply_off_is_the_word_off_and_anything_else_is_auto() {
    assert_eq!(Apply::parse("off"), Apply::Off);
    assert_eq!(Apply::parse("  off \n"), Apply::Off);
    for other in ["auto", "", "on", "OFF", "offline"] {
        assert_eq!(Apply::parse(other), Apply::Auto, "{other:?}");
    }
}

#[test]
fn a_record_that_points_somewhere_else_is_not_believed() {
    let w = World::new("elsewhere");
    let place = w.place(Apply::Off);
    let site = w.prepare(&place, "t1");
    std::fs::write(site.dir.join("a.txt"), "two\n").unwrap();
    let record = std::fs::read_dir(w.home.path().join("worktrees")).unwrap().flatten().next().unwrap().path().join("t1.json");
    let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&record).unwrap()).unwrap();
    v["dir"] = serde_json::Value::String(w.project.path().display().to_string());
    std::fs::write(&record, v.to_string()).unwrap();
    let other = w.place(Apply::Off);
    assert!(other.site_of("t1").is_none(), "the project's own folder is not a worktree because a file says so");
    assert!(other.take("t1", Take::Apply).unwrap_err().msg.contains("is gone"));
    assert!(other.recover().is_empty());
}

#[test]
fn what_a_crash_left_is_recovered_in_the_order_of_the_tasks_not_of_their_names() {
    let w = World::new("order");
    let place = w.place(Apply::Off);
    for task in ["t10", "t2", "t9"] {
        let site = w.prepare(&place, task);
        std::fs::write(site.dir.join("a.txt"), format!("by {task}\n")).unwrap();
    }
    let found = w.place(Apply::Off).recover();
    assert_eq!(found.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(), ["t2", "t9", "t10"]);
}

#[test]
fn a_task_that_only_committed_still_has_its_work_backed_up_when_the_copy_is_removed() {
    let w = World::new("backup");
    let place = w.place(Apply::Auto);
    let site = w.prepare(&place, "t1");
    std::fs::write(site.dir.join("committed.txt"), "only in the copy's history\n").unwrap();
    git(&site.dir, &["add", "-A"]);
    git(&site.dir, &["commit", "-qm", "task work"]);
    assert!(place.settle(&site).applied.is_some());
    assert!(!site.dir.exists());
    let saved = git(w.project.path(), &["show", "refs/j4n/backups/t1:committed.txt"]);
    assert_eq!(saved, "only in the copy's history", "the commit is reachable from the backup, though no branch is left");
}

#[test]
fn work_that_was_taken_back_out_of_the_copy_leaves_nothing_to_take() {
    let w = World::new("nothing-left");
    let place = w.place(Apply::Off);
    let site = w.prepare(&place, "t1");
    std::fs::write(site.dir.join("a.txt"), "two\n").unwrap();
    assert!(place.settle(&site).kept.is_some());
    git(&site.dir, &["checkout", "HEAD", "--", "a.txt"]);
    assert_eq!(place.take("t1", Take::Look).unwrap(), Taken::Nothing);
    assert_eq!(place.take("t1", Take::Apply).unwrap(), Taken::Nothing);
    assert!(!site.dir.exists(), "an empty copy is removed");
    assert!(place.site_of("t1").is_none());
    assert!(!store_has(&w, "t1"), "and its record");
}

fn store_has(w: &World, task: &str) -> bool {
    std::fs::read_dir(w.home.path().join("worktrees")).unwrap().flatten().any(|p| p.path().join(format!("{task}.json")).exists())
}

#[test]
fn the_record_of_a_copy_goes_when_its_work_is_applied_either_way() {
    let w = World::new("records");
    let off = w.place(Apply::Off);
    let site = w.prepare(&off, "t1");
    std::fs::write(site.dir.join("a.txt"), "two\n").unwrap();
    assert!(off.settle(&site).kept.is_some());
    assert!(store_has(&w, "t1"), "kept: the record stays");
    off.take("t1", Take::Apply).unwrap();
    assert!(!store_has(&w, "t1"), "taken: it goes");
    let auto = w.place(Apply::Auto);
    let site = w.prepare(&auto, "t2");
    std::fs::write(site.dir.join("b.txt"), "b\n").unwrap();
    assert!(auto.settle(&site).applied.is_some());
    assert!(!store_has(&w, "t2"), "applied as the task ended: it goes");
}

#[test]
fn a_patch_that_does_not_apply_is_said_so_while_the_copy_waits() {
    let w = World::new("look-conflict");
    let place = w.place(Apply::Off);
    let site = w.prepare(&place, "t1");
    std::fs::write(site.dir.join("a.txt"), "one\ntask line\n").unwrap();
    w.project.put("a.txt", "totally different\n");
    git(w.project.path(), &["commit", "-qam", "moved on"]);
    let kept = place.settle(&site).kept.expect("kept");
    assert!(kept.why.starts_with("waiting to be applied; it does not apply now: the change does not apply to your folder as it is now"), "{}", kept.why);
    assert!(matches!(place.take("t1", Take::Look).unwrap(), Taken::Blocked { why, .. } if why.contains("does not apply")));
}

#[test]
fn a_task_that_asks_to_hold_its_work_keeps_it_though_apply_is_auto_and_it_can_be_taken_after() {
    let w = World::new("hold");
    let place = w.place(Apply::Auto);
    let mut spec = TaskSpec::new("go", "d");
    spec.input = serde_json::json!({ "hold": true });
    let site = place.prepare(&Prepare { task: "t1", spec: &spec, root: w.project.path() }).unwrap();
    std::fs::write(site.dir.join("new.txt"), "held\n").unwrap();
    let settled = place.settle(&site);
    assert!(settled.applied.is_none(), "not laid into the folder");
    let kept = settled.kept.expect("kept in the worktree");
    assert!(kept.why.contains("waiting to be applied"), "{}", kept.why);
    assert!(!w.project.path().join("new.txt").exists());
    // and what the place keeps is what it takes: the work a flow checked and was said yes to
    assert!(matches!(place.take("t1", Take::Apply).unwrap(), Taken::Clean(..)));
    assert_eq!(std::fs::read_to_string(w.project.path().join("new.txt")).unwrap(), "held\n");
    // a task that does not ask is applied at once, as before
    let site = w.prepare(&place, "t2");
    std::fs::write(site.dir.join("other.txt"), "auto\n").unwrap();
    assert!(place.settle(&site).applied.is_some());
}

/// A task whose worktree is no longer known leaves an empty change set that is not "nothing changed".
#[test]
fn a_task_with_no_copy_to_look_at_does_not_say_it_changed_nothing() {
    let w = World::new("worktree-gone");
    let place = w.place(Apply::Off);
    let gone = place.settle(&Site::new("t9", w.project.path()));
    assert!(gone.unknown && gone.kept.is_none(), "{gone:?}");
    let site = w.prepare(&place, "t1");
    assert!(!place.settle(&site).unknown, "a copy that was looked at and had nothing in it is a known empty");
}

/// A held task whose worktree is given the shares `[(path, source)]` before it starts.
fn shared(w: &World, place: &Worktree, task: &str, shares: &[(&str, &Path)]) -> Site {
    let mut spec = TaskSpec::new("go", "d");
    let list: Vec<serde_json::Value> = shares.iter().map(|(p, f)| serde_json::json!({ "path": p, "from": f })).collect();
    spec.input = serde_json::json!({ "hold": true, "shares": list });
    place.prepare(&Prepare { task, spec: &spec, root: w.project.path() }).unwrap()
}

#[test]
fn a_share_is_read_only_is_not_the_tasks_work_is_not_in_the_patch_and_never_clashes_in_the_folder() {
    // a part tests against the other part's module, not a stand-in of it: a share is the other part's file itself
    let w = World::new("share");
    let other = Scratch::new("share-o");
    let store = other.put("src/store.py", "def add_item(): pass\n");
    // the folder's uncommitted change to a tracked file (Main's own), and a file git ignores
    w.project.put("a.txt", "one\nMain's\n");
    let notes = other.put("ledger/notes.md", "kept\n");
    let place = w.place(Apply::Auto);
    let site = shared(&w, &place, "t1", &[("src/store.py", &store), ("a.txt", &w.project.join("a.txt")), ("ledger/notes.md", &notes)]);
    assert!(site.notes.iter().all(|n| !n.contains("not shared")), "{:?}", site.notes);
    assert_eq!(std::fs::read_to_string(site.dir.join("src/store.py")).unwrap(), "def add_item(): pass\n");
    assert_eq!(std::fs::read_to_string(site.dir.join("a.txt")).unwrap(), "one\nMain's\n");
    assert!(std::fs::metadata(site.dir.join("src/store.py")).unwrap().permissions().readonly(), "the copy is read-only");
    // git's exclude file is the project's too: nothing is written there, and the folder sees what it saw
    let exclude = git(w.project.path(), &["rev-parse", "--git-path", "info/exclude"]);
    assert!(std::fs::read_to_string(w.project.join(&exclude)).unwrap_or_default().lines().all(|l| !l.contains("store")), "{exclude}");
    std::fs::write(site.dir.join("src/cli.py"), "from store import add_item\n").unwrap();
    let settled = place.settle(&site);
    assert_eq!((names(&settled.created), names(&settled.modified)), (vec!["src/cli.py"], vec![]), "a share is not the task's own: {settled:?}");
    let kept = settled.kept.expect("held");
    assert_eq!((names(&kept.work.files), names(&kept.work.ignored)), (vec!["src/cli.py"], vec![]));
    // the folder already holds other content at a share's path: the share is no part of the patch, so it is no clash
    w.project.put("src/store.py", "def add_item(x): return x\n");
    match place.take("t1", Take::Apply).unwrap() {
        Taken::Clean(work, _) => assert_eq!(names(&work.files), ["src/cli.py"]),
        other => panic!("{other:?}"),
    }
    assert_eq!(std::fs::read_to_string(w.project.join("src/cli.py")).unwrap(), "from store import add_item\n", "the task's own file lands");
    assert_eq!(std::fs::read_to_string(w.project.join("src/store.py")).unwrap(), "def add_item(x): return x\n", "the folder's is left as it is");
    assert_eq!(std::fs::read_to_string(w.project.join("a.txt")).unwrap(), "one\nMain's\n");
    assert!(!w.project.join("ledger/notes.md").exists(), "an ignored share is not copied in");
}

#[test]
fn a_share_follows_its_source_and_one_the_task_wrote_over_is_a_change_of_its_own_that_is_not_laid_in() {
    let w = World::new("follow");
    let other = Scratch::new("follow-o");
    let store = other.put("src/store.py", "v1\n");
    let place = w.place(Apply::Auto);
    let site = shared(&w, &place, "t1", &[("src/store.py", &store)]);
    assert_eq!(place.share("t1", "src/store.py", &store), Ok(Shared::Same));
    other.put("src/store.py", "v2\n");
    assert_eq!(place.share("t1", "src/store.py", &store), Ok(Shared::Changed));
    assert_eq!(std::fs::read_to_string(site.dir.join("src/store.py")).unwrap(), "v2\n");
    assert!(std::fs::metadata(site.dir.join("src/store.py")).unwrap().permissions().readonly());
    // read-only is a mechanism, not a promise: a task that takes it off and writes is seen
    std::fs::remove_file(site.dir.join("src/store.py")).unwrap();
    std::fs::write(site.dir.join("src/store.py"), "mine\n").unwrap();
    let settled = place.settle(&site);
    assert_eq!(names(&settled.modified), ["src/store.py"], "{settled:?}");
    assert!(settled.created.is_empty() && settled.kept.is_none(), "and it is no work to keep: {settled:?}");
    assert!(!w.project.join("src/store.py").exists());
}

#[test]
fn a_share_is_refused_over_the_tasks_own_file_from_a_link_and_into_git() {
    let w = World::new("refuse");
    let other = Scratch::new("refuse-o");
    let store = other.put("src/store.py", "x\n");
    let place = w.place(Apply::Auto);
    let site = shared(&w, &place, "t1", &[]);
    std::fs::write(site.dir.join("src/mine.py"), "mine\n").unwrap();
    std::fs::write(site.dir.join("a.txt"), "edited\n").unwrap();
    for own in ["src/mine.py", "a.txt"] {
        assert!(place.share("t1", own, &store).unwrap_err().contains("own work"), "{own}");
    }
    assert_eq!(std::fs::read_to_string(site.dir.join("src/mine.py")).unwrap(), "mine\n");
    for bad in [".git/config", "../x.py", "/etc/x", "src/../a.txt", ".j4n/x"] {
        assert!(place.share("t1", bad, &store).unwrap_err().contains("not a path"), "{bad}");
    }
    std::os::unix::fs::symlink(&store, other.join("link.py")).unwrap();
    assert!(place.share("t1", "src/link.py", &other.join("link.py")).unwrap_err().contains("plain file"));
    assert!(!site.dir.join("src/link.py").exists());
}

#[test]
fn a_file_the_piece_made_where_the_folder_has_an_empty_untracked_one_is_laid_over_it_and_a_rewind_brings_the_empty_one_back() {
    // Main's line makes an empty `chess/__init__.py` in the folder, the part makes its own with a line in it, and the lay-in must not be blocked
    let w = World::new("empty");
    let place = w.place(Apply::Off);
    let site = w.prepare(&place, "t1");
    std::fs::create_dir_all(site.dir.join("chess")).unwrap();
    std::fs::write(site.dir.join("chess/__init__.py"), "\"\"\"The game.\"\"\"\n").unwrap();
    std::fs::write(site.dir.join("chess/screen.py"), "x = 1\n").unwrap();
    w.project.put("chess/__init__.py", "");
    let kept = place.settle(&site).kept.expect("kept");
    assert_eq!(kept.why, "waiting to be applied; it would apply cleanly");
    assert!(matches!(place.take("t1", Take::Look).unwrap(), Taken::Clean(..)));
    assert_eq!(std::fs::read_to_string(w.project.join("chess/__init__.py")).unwrap(), "", "a look writes nothing");
    let taken = place.take("t1", Take::Apply).unwrap();
    assert!(matches!(&taken, Taken::Clean(work, notes) if names(&work.files) == ["chess/__init__.py", "chess/screen.py"] && notes.is_empty()), "{taken:?}");
    assert_eq!(std::fs::read_to_string(w.project.join("chess/__init__.py")).unwrap(), "\"\"\"The game.\"\"\"\n");
    assert!(w.project.join("chess/screen.py").exists());
    checkpoint::rewind(w.home.path(), w.project.path(), Some("t1"), false).unwrap();
    assert_eq!(std::fs::read_to_string(w.project.join("chess/__init__.py")).unwrap(), "", "the empty file is back as it was");
    assert!(!w.project.join("chess/screen.py").exists());
}

#[test]
fn only_an_empty_untracked_file_of_the_folder_gives_way_and_content_a_tracked_file_emptied_or_a_link_still_block() {
    let w = World::new("empty-not");
    let place = w.place(Apply::Off);
    let hold = |task: &str, file: &str, text: &str| {
        let site = w.prepare(&place, task);
        std::fs::write(site.dir.join(file), text).unwrap();
        site
    };
    let sites = [hold("t1", "b.py", "task\n"), hold("t2", "c.txt", "task\n"), hold("t3", "d.py", ""), hold("t4", "e.py", "task\n"), hold("t5", "g.py", "task\n"), hold("t6", "f.py", "")];
    // the piece's own file is a link to content outside its worktree: it is not copied in through the link
    let outside = Scratch::new("empty-not-o");
    let secret = outside.put("s.txt", "not the piece's\n");
    std::fs::remove_file(sites[5].dir.join("f.py")).unwrap();
    std::os::unix::fs::symlink(&secret, sites[5].dir.join("f.py")).unwrap();
    w.project.put("f.py", "");
    // the folder: an untracked file with content, a committed file the operator emptied, content where the piece made an empty file, a link to an empty file,
    // an empty file staged as new
    w.project.put("b.py", "mine\n");
    w.project.put("c.txt", "kept\n");
    git(w.project.path(), &["add", "c.txt"]);
    git(w.project.path(), &["commit", "-qm", "c"]);
    w.project.put("c.txt", "");
    w.project.put("g.py", "");
    git(w.project.path(), &["add", "g.py"]);
    w.project.put("d.py", "mine\n");
    let empty = w.project.put("empty.txt", "");
    std::os::unix::fs::symlink(&empty, w.project.join("e.py")).unwrap();
    for (site, file) in sites.iter().zip(["b.py", "c.txt", "d.py", "e.py", "g.py", "f.py"]) {
        let kept = place.settle(site).kept.expect("kept");
        assert!(kept.why.contains("does not apply now") && kept.why.contains(file), "{file}: {}", kept.why);
    }
    for (task, file, mine) in [("t1", "b.py", "mine\n"), ("t2", "c.txt", ""), ("t3", "d.py", "mine\n"), ("t5", "g.py", ""), ("t6", "f.py", "")] {
        assert!(matches!(place.take(task, Take::Apply).unwrap(), Taken::Blocked { .. }), "{file}");
        assert_eq!(std::fs::read_to_string(w.project.join(file)).unwrap(), mine, "{file}");
    }
    assert!(matches!(place.take("t4", Take::Apply).unwrap(), Taken::Blocked { .. }));
    assert!(std::fs::symlink_metadata(w.project.join("e.py")).unwrap().file_type().is_symlink());
}
