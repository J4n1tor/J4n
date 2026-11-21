//! A plan item carries its own acceptance check and write scope, and J4n judges the end
//! of its task from the Log, not from the doer's word. Through the real front, a scripted main lane and a scripted doer `check`.

use crate::branch_validate::CLEAN;
use crate::front_cases::{Fx, fx_deciding};
use crate::scripted::{Beat, Scripted, end, say};
use j4n_core::branch::Switch;
use j4n_core::front::{Cfg, Said};
use j4n_core::keeper::{How, Op, bind_checks, take_within};
use serde_json::{Value, json};

fn plan_of(check: Value) -> String {
    let mut item = json!({ "id": "a", "goal": "do a", "reads": [], "writes": ["src/a.rs"], "depends": [], "done": "a works (tests/a.rs)", "where": "inline" });
    if !check.is_null() {
        item["check"] = check;
    }
    json!({ "approach": "Do a.", "items": [item] }).to_string()
}

fn edits(path: &str) -> Vec<Beat> {
    vec![Beat::Session("s".into()), Beat::Put(path.into(), "fn a() {}\n".into()), say("a is done"), end("a is done")]
}

fn red() -> Vec<Beat> {
    vec![Beat::Shell("tests".into(), "assertion left != right".into(), 1), Beat::Fail("tests check failed (exit 1):\nassertion left != right".into())]
}

fn green() -> Vec<Beat> {
    vec![Beat::Shell("tests".into(), "ok".into(), 0), end("tests: passed")]
}

/// A main lane that plans (`check`), then plays `turns` for the item; a doer `check` that plays `checks`.
fn rig(check: Value, turns: Vec<Vec<Beat>>, checks: Vec<Vec<Beat>>) -> (Fx, std::sync::Arc<Scripted>) {
    rig_seeded(check, turns, checks, &[])
}

/// [`rig`] in a folder that holds `seed` (path, text) before anything starts.
fn rig_seeded(check: Value, turns: Vec<Vec<Beat>>, checks: Vec<Vec<Beat>>, seed: &[(&str, &str)]) -> (Fx, std::sync::Arc<Scripted>) {
    let plan = plan_of(check);
    let mut all = vec![vec![Beat::Session("s".into()), say(&plan), end(&plan)]];
    all.extend(turns);
    let checker = Scripted::new("check", checks);
    let f = fx_deciding(vec![Scripted::new("claude", all), checker.clone()], None, |c: &mut Cfg| {
        c.doer = Some("claude".into());
        c.lanes = Switch::Auto;
        c.checks = vec!["tests".into()];
    });
    for (path, text) in seed {
        let to = f.rig.tasks.root().join(path);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(to, text).unwrap();
    }
    let Said::Started { task } = f.say("plan how to do a, then do it") else { panic!("{}", f.rig.dump()) };
    f.rig.finished(&task);
    f.front.plan_collect("main");
    f.front.plan_step("main");
    (f, checker)
}

fn run(f: &Fx, what: &str, done: impl Fn(&j4n_core::keeper::Item) -> bool) -> j4n_core::keeper::Item {
    for _ in 0..24 {
        let a = f.keeper.plan("main").unwrap().items.remove(0);
        if done(&a) {
            return a;
        }
        for r in f.tasks() {
            f.rig.finished(&r.id);
        }
        f.front.plan_step("main");
    }
    panic!("never {what}: {:?}\n{}", f.keeper.plan("main").unwrap().items[0].state, f.rig.dump());
}

fn path(f: &Fx) -> Vec<String> {
    f.rig.of_kind("plan.item").into_iter().filter(|e| e.data["id"] == "a" && e.data["op"] == "state").map(|e| e.data["state"].as_str().unwrap().to_string()).collect()
}

#[test]
fn a_doer_that_says_done_while_its_check_exits_1_is_sent_back_with_the_output_and_the_scope_then_blocked() {
    let (f, checker) = rig(json!("tests"), vec![edits("src/a.rs")], vec![red()]);
    let a = run(&f, "the item was blocked", |a| a.state == "blocked");
    assert_eq!((a.sends, a.block.as_str()), (3, "sends"), "{}", f.rig.dump());
    assert!(!path(&f).iter().any(|s| s == "checked" || s == "merged"), "{:?}", path(&f));
    let back: Vec<String> = f.rig.of_kind("plan.item").into_iter().filter(|e| e.data["state"] == "sent-back").map(|e| e.data["result"].as_str().unwrap().to_string()).collect();
    assert_eq!(back.len(), 3);
    assert!(back[0].contains("assertion left != right") && back[0].contains("this item only: src/a.rs"), "the output and the scope go back: {}", back[0]);
    let again = f.tasks().into_iter().filter(|t| t.spec.doer == "claude").nth(2).unwrap();
    assert!(again.spec.goal.contains("Sent back (1 of 3)") && again.spec.goal.contains("assertion left != right"), "{}", again.spec.goal);
    assert_eq!(checker.starts().len(), 4, "the check ran at the end of each of the four turns");
}

#[test]
fn a_check_that_exits_0_checks_the_item_and_only_then_is_it_merged() {
    let (f, checker) = rig(json!("tests"), vec![edits("src/a.rs")], vec![green()]);
    let a = run(&f, "the item was merged", |a| a.state == "merged");
    assert_eq!(path(&f), ["running", "reported", "reported", "checked", "merged"], "{}", f.rig.dump());
    assert!(a.result.is_empty() || a.sends == 0);
    let t = f.tasks().into_iter().find(|t| t.spec.doer == "check").unwrap();
    assert_eq!((t.spec.place.as_str(), t.spec.input["check"].as_str()), ("here", Some("tests")), "the item's check is what the doer is told");
    assert_eq!(checker.starts().len(), 1);
}

#[test]
fn a_path_outside_the_items_writes_is_not_checked_the_item_goes_back_with_the_scope_and_no_check_ran() {
    // a file that was there before: what the task made itself, and no other item writes, is its own
    let (f, checker) = rig_seeded(json!("tests"), vec![edits("src/other.rs"), edits("src/a.rs")], vec![green()], &[("src/other.rs", "fn other() {}\n")]);
    let a = run(&f, "the item was merged", |a| a.state == "merged");
    assert_eq!(path(&f), ["running", "reported", "sent-back", "running", "reported", "reported", "checked", "merged"], "{}", f.rig.dump());
    let back = f.rig.of_kind("plan.item").into_iter().find(|e| e.data["state"] == "sent-back").unwrap();
    let why = back.data["result"].as_str().unwrap();
    assert!(why.contains("this file only: src/a.rs") && why.contains("src/other.rs"), "{why}");
    assert_eq!((a.sends, checker.starts().len()), (1, 1), "the scope is judged before the check costs anything");
}

#[test]
fn a_test_file_the_done_line_names_is_in_scope() {
    let turn = vec![Beat::Session("s".into()), Beat::Put("src/a.rs".into(), "x".into()), Beat::Put("tests/a.rs".into(), "y".into()), end("done")];
    let (f, _) = rig(json!("tests"), vec![turn], vec![green()]);
    run(&f, "the item was merged", |a| a.state == "merged");
    assert_eq!(f.keeper.plan("main").unwrap().items[0].sends, 0);
}

#[test]
fn an_empty_file_outside_the_writes_that_no_other_item_writes_is_in_scope_and_a_change_to_one_that_was_there_is_not() {
    // a part's tests need `tests/__init__.py`, empty; it must not be sent back for it, make it again and be blocked
    let marker = vec![Beat::Session("s".into()), Beat::Put("src/a.rs".into(), "x".into()), Beat::Put("tests/__init__.py".into(), String::new()), end("done")];
    let (f, _) = rig(json!("tests"), vec![marker], vec![green()]);
    run(&f, "the item was merged", |a| a.state == "merged");
    assert_eq!(f.keeper.plan("main").unwrap().items[0].sends, 0, "{}", f.rig.dump());
    let text = vec![Beat::Session("s".into()), Beat::Put("src/a.rs".into(), "x".into()), Beat::Put("tests/__init__.py".into(), "import os".into()), end("done")];
    let (f, _) = rig_seeded(json!("tests"), vec![text, edits("src/a.rs")], vec![green()], &[("tests/__init__.py", "")]);
    run(&f, "the item was merged", |a| a.state == "merged");
    let back = f.rig.of_kind("plan.item").into_iter().find(|e| e.data["state"] == "sent-back").expect("a file that was there before is outside");
    assert!(back.data["result"].as_str().unwrap().contains("tests/__init__.py"), "{}", back.data);
}

#[test]
fn a_test_runner_s_own_settings_file_that_no_other_item_writes_is_in_scope() {
    // a part's pytest needs a `pytest.ini` to import its package; it must not be sent back for it, make it again and be blocked
    let turn = vec![Beat::Session("s".into()), Beat::Put("src/a.rs".into(), "x".into()), Beat::Put("pytest.ini".into(), "[pytest]\npythonpath = .\n".into()), Beat::Put("tests/conftest.py".into(), "import sys\n".into()), end("done")];
    let (f, _) = rig(json!("tests"), vec![turn], vec![green()]);
    run(&f, "the item was merged", |a| a.state == "merged");
    assert_eq!(f.keeper.plan("main").unwrap().items[0].sends, 0, "{}", f.rig.dump());
}

#[test]
fn an_item_whose_check_is_none_is_unproven_and_goes_on_as_a_mark_not_a_stop() {
    // it was `blocked` until the operator accepted it; now the mark stays, the dependents may start
    let (f, checker) = rig(json!("none"), vec![edits("src/a.rs")], vec![green()]);
    let a = run(&f, "the item was merged", |a| a.state == "merged");
    assert_eq!((a.unproven, a.block.as_str(), checker.starts().len()), (true, "", 0), "{}", f.rig.dump());
    assert!(a.result.starts_with("unproven:"), "{}", a.result);
    assert_eq!(path(&f), ["running", "reported", "merged"], "never `checked`");
    let said = f.rig.of_kind("plan.reported");
    assert!(said[0].data["decisions"][0].as_str().unwrap().starts_with("a: unproven"), "the operator is told it is theirs: {:?}", said[0].data);
    assert!(f.rig.of_kind("plan.pending").is_empty() && f.tasks().iter().filter(|t| t.spec.role.as_deref() == Some("plan")).count() == 1, "an unproven item does not start a replan");
}

/// The Gate refused the item's write, the doer said so, `task.settled` had an empty `changed`, and
/// `cargo test` (0 tests, exit 0) on the untouched tree made the item `checked` and `merged` with `unproven []`.
fn nothing_done() -> Vec<Beat> {
    vec![Beat::Session("s".into()), say("the edit was refused, so nothing has changed"), end("the edit was refused, so nothing has changed")]
}

#[test]
fn an_item_that_writes_is_unproven_when_its_task_changed_nothing_though_the_check_is_green() {
    let (f, checker) = rig(json!("tests"), vec![nothing_done()], vec![green()]);
    let a = run(&f, "the item was merged", |a| a.state == "merged");
    assert!(a.unproven && a.result.starts_with("unproven:") && a.result.contains("changed nothing in src/a.rs"), "{a:?}");
    assert_eq!(path(&f), ["running", "reported", "merged"], "never `checked`: {}", f.rig.dump());
    assert_eq!(checker.starts().len(), 0, "a check of a tree nobody touched is not paid for");
    let said = f.rig.of_kind("plan.reported");
    assert_eq!((said[0].data["outcome"].as_str(), said[0].data["unproven"].as_array().map(Vec::len)), (Some("done"), Some(1)), "the report lists it: {:?}", said[0].data);
}

#[test]
fn an_item_that_wrote_what_it_was_to_write_is_checked_as_before_and_one_with_no_writes_is_not_asked_to_have_changed_anything() {
    let (f, _) = rig(json!("tests"), vec![edits("src/a.rs")], vec![green()]);
    let a = run(&f, "the item was merged", |a| a.state == "merged");
    assert!(!a.unproven, "{a:?}");
    // no `writes`: a read or a manual item, whatever it changed
    let item = json!({ "id": "a", "goal": "look at a", "reads": ["src/a.rs"], "writes": [], "depends": [], "done": "a is understood", "where": "inline", "check": "tests" });
    let plan = json!({ "approach": "Look.", "items": [item] }).to_string();
    let checker = Scripted::new("check", vec![green()]);
    let f = fx_deciding(vec![Scripted::new("claude", vec![vec![Beat::Session("s".into()), say(&plan), end(&plan)], nothing_done()]), checker.clone()], None, |c: &mut Cfg| {
        c.doer = Some("claude".into());
        c.lanes = Switch::Auto;
        c.checks = vec!["tests".into()];
    });
    let Said::Started { task } = f.say("plan how to look at a") else { panic!("{}", f.rig.dump()) };
    f.rig.finished(&task);
    f.front.plan_collect("main");
    f.front.plan_step("main");
    let a = run(&f, "the item was merged", |a| a.state == "merged");
    assert!(!a.unproven && checker.starts().len() == 1, "{a:?}\n{}", f.rig.dump());
}

#[test]
fn a_try_after_a_send_back_that_changed_nothing_is_judged_by_its_check_as_before() {
    // the first try wrote outside the item (sent back), the second has nothing left to write
    let (f, checker) = rig_seeded(json!("tests"), vec![edits("src/other.rs"), nothing_done()], vec![green()], &[("src/other.rs", "fn other() {}\n")]);
    let a = run(&f, "the item was merged", |a| a.state == "merged");
    assert!(!a.unproven && a.sends == 1 && checker.starts().len() == 1, "{a:?}\n{}", f.rig.dump());
}

/// An empty `changed` is "nothing changed" only where the place looked. The place a task follows another into
/// (`@t5`) leaves what changed to its origin, so it says it does not know, and an item settled there is judged by its check as before.
#[test]
fn a_place_that_did_not_look_says_so_and_here_says_so_when_it_had_no_checkpoint() {
    use j4n_core::place::checkpoint::Cfg;
    use j4n_core::place::{Here, Place, Site, Via};
    let (project, home) = (crate::world::Scratch::new("t226-p"), crate::world::Scratch::new("t226-h"));
    let here: std::sync::Arc<dyn Place> = std::sync::Arc::new(Here::new(home.path(), project.path(), Cfg::default()));
    let via = Via::of(here.clone(), "t1").expect("here has a site for any task");
    assert!(via.settle(&Site::new("t2", project.path())).unknown, "a task that works where another did does not know what it changed");
    // `here` itself: a read task has nothing to look at and nothing was lost; a write task with no checkpoint does not know
    assert!(!here.settle(&Site::new("t3", project.path())).unknown);
    let mut lost = Site::new("t4", project.path());
    lost.notes.push("no checkpoint, so this task cannot be rewound: x".into());
    assert!(here.settle(&lost).unknown);
}

#[test]
fn an_item_with_no_check_named_has_no_contract_as_before() {
    let (f, checker) = rig(Value::Null, vec![edits("src/other.rs")], vec![red()]);
    run(&f, "the item was merged", |a| a.state == "merged");
    assert_eq!((path(&f), checker.starts().len()), (vec!["running".to_string(), "merged".to_string()], 0));
}

#[test]
fn a_word_that_is_no_check_of_the_project_becomes_none_and_is_said_never_run() {
    let answer = plan_of(json!("cargo test; rm -rf /"));
    let (mut ops, _) = take_within(&answer, None, How::Begin, "plan a", &[], 3).unwrap();
    let said = bind_checks(&mut ops, &["tests".to_string()]);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].starts_with("a: `cargo test; rm -rf /` is no check"), "{said:?}");
    assert!(matches!(&ops[1], Op::Add(i) if i.check == "none"));
    let (mut ops, _) = take_within(&plan_of(json!(" Tests ")), None, How::Begin, "plan a", &[], 3).unwrap();
    assert!(bind_checks(&mut ops, &["tests".to_string()]).is_empty() && matches!(&ops[1], Op::Add(i) if i.check == "tests"));
}

// ---- a piece, in its held worktree ----

use crate::branch_validate::{doer, no_replan, rig_cfg};

fn with_tests(c: &mut Cfg) {
    no_replan(c);
    c.checks = vec!["tests".into()];
}

/// The three pieces and the inline end of `branch_validate::pieces`, each with this check and these writes.
fn pieces_with(writes: [&str; 3]) -> Vec<Value> {
    let mut all = crate::branch_validate::pieces();
    for (n, p) in all.iter_mut().take(3).enumerate() {
        p["check"] = json!("tests");
        p["writes"] = json!([writes[n]]);
    }
    all
}

#[test]
fn a_piece_is_checked_where_it_is_held_with_its_check_before_its_review() {
    let r = rig_cfg(pieces_with(["src/a/1.rs", "src/b/1.rs", "src/c/1.rs"]), doer("pi", &["did"; 6]), doer("own", &[CLEAN; 6]), Scripted::new("check", vec![green()]), &["end done"], with_tests);
    r.begin();
    // the worktree holds src/a/1.rs: p1's scope; the other two go back, which is the next test's
    r.until("p1 was reviewed", |r| matches!(r.item("p1").state.as_str(), "reviewed" | "merged"));
    let t = r.f.tasks().into_iter().find(|t| t.spec.doer == "check" && t.spec.input["item"] == "p1" && t.spec.place.starts_with('@')).unwrap();
    assert_eq!(t.spec.input["check"], "tests", "{}", r.f.rig.dump());
    assert_eq!(r.path("p1")[..4], ["running", "reported", "reported", "checked"]);
}

#[test]
fn what_a_piece_s_tools_leave_in_their_cache_folders_is_not_outside_its_writes() {
    // a piece must not be sent back for the `.pytest_cache` its own test run left in its worktree
    // the held work of every piece is `src/a/1.rs` and the caches: p1 writes it, so only the caches could send p1 back (p2 and p3 go back for it)
    let r = rig_cfg(pieces_with(["src/a/1.rs", "src/b/1.rs", "src/c/1.rs"]), doer("pi", &["did"; 6]), doer("own", &[CLEAN; 6]), Scripted::new("check", vec![green(); 12]), &["end done"], with_tests);
    r.held.also.lock().unwrap().extend([".pytest_cache/v/cache/lastfailed".to_string(), "src/a/__pycache__/1.cpython-313.pyc".to_string()]);
    r.begin();
    r.until("p1 was reviewed", |r| matches!(r.item("p1").state.as_str(), "reviewed" | "merged" | "blocked"));
    assert!(r.sent_back().iter().all(|e| e.data["id"] != "p1"), "{}", r.f.rig.dump());
    assert!(r.sent_back().iter().any(|e| e.data["id"] != "p1"), "the others still go back for the path they do not write: {}", r.f.rig.dump());
}

#[test]
fn what_a_piece_made_that_no_other_item_writes_is_its_own_and_a_file_another_item_writes_is_not() {
    // c1 must not be blocked for the tests and the lock file it made, which no item wrote, nor c2 for the `package.json` c1 writes
    let r = rig_cfg(pieces_with(["src/a/1.rs", "src/b/1.rs", "src/c/1.rs"]), doer("pi", &["did"; 6]), doer("own", &[CLEAN; 6]), Scripted::new("check", vec![green(); 12]), &["end done"], with_tests);
    let made = ["test/a/1.test.ts", "package-lock.json", ".gitignore"].map(String::from);
    r.held.also.lock().unwrap().extend(made.clone());
    r.held.created.lock().unwrap().extend(made.into_iter().chain(["src/a/1.rs".to_string()]));
    r.begin();
    r.until("p1 was reviewed", |r| matches!(r.item("p1").state.as_str(), "reviewed" | "merged" | "blocked"));
    assert!(r.sent_back().iter().all(|e| e.data["id"] != "p1"), "{}", r.f.rig.dump());
    let back = r.sent_back().into_iter().find(|e| e.data["id"] != "p1").expect("the others go back for the file p1 writes");
    let why = back.data["result"].as_str().unwrap();
    assert!(why.contains("it also changed src/a/1.rs") && !why.contains("package-lock.json"), "{why}");
}

#[test]
fn a_piece_whose_held_change_leaves_its_writes_goes_back_with_the_scope_and_the_extra_path() {
    let r = rig_cfg(pieces_with(["src/a/1.rs", "src/b/1.rs", "src/c/1.rs"]), doer("pi", &["did"; 6]), doer("own", &[CLEAN; 6]), Scripted::new("check", vec![green()]), &["end done"], with_tests);
    r.begin();
    r.until("a piece went back for its scope", |r| r.sent_back().iter().any(|e| e.data["result"].as_str().is_some_and(|w| w.contains("this file only"))));
    let back = r.sent_back().into_iter().find(|e| e.data["result"].as_str().is_some_and(|w| w.contains("this file only"))).unwrap();
    let why = back.data["result"].as_str().unwrap();
    assert!(why.contains("src/a/1.rs") && matches!(back.data["id"].as_str(), Some("p2" | "p3")), "{why}: {}", r.f.rig.dump());
    assert_ne!(back.data["id"], "p1", "p1 writes what the worktree holds");
}

#[test]
fn an_unproven_piece_is_still_reviewed_and_laid_in_it_is_not_skipped_to_merged() {
    // a piece with no check goes on as `checked` with the mark, so its review and its merge still happen
    let mut items = pieces_with(["src/a/1.rs", "src/b/1.rs", "src/c/1.rs"]);
    items[0]["check"] = json!("none");
    let r = rig_cfg(items, doer("pi", &["did"; 6]), doer("own", &[CLEAN; 6]), Scripted::new("check", vec![green()]), &["end done"], with_tests);
    r.begin();
    r.until("p1 was reviewed", |r| matches!(r.item("p1").state.as_str(), "reviewed" | "merged"));
    assert_eq!(r.path("p1")[..3], ["running", "reported", "checked"], "{}", r.f.rig.dump());
    assert!(r.item("p1").unproven && r.item("p1").block.is_empty(), "{:?}", r.item("p1"));
    assert!(r.f.tasks().iter().all(|t| !(t.spec.doer == "check" && t.spec.input["item"] == "p1" && t.spec.place.starts_with('@'))), "no check ran on its held work");
    r.until("p1 was merged", |r| r.item("p1").state == "merged");
    assert!(r.item("p1").unproven, "the mark stays through its review and merge: {:?}", r.item("p1"));
}
