//! Table writes through the Log: the journal, the fold of changes (the undo stack), the writes a crash left unfinished,
//! and a project's `.j4n/` does not show in `git status`.

use crate::world::*;
use j4n_core::change::{Actor, Cause, Change, Where};
use j4n_core::changes::{Changes, FAILED, LogJournal, RECORDED};
use j4n_core::fold::{Fold, Folds};
use j4n_core::log::{Log, New};
use j4n_core::table::{Layer, LayerKind, Op, Tables, Write};
use j4n_core::value::Value;
use j4n_core::write::{Journal, NoJournal, Writer};
use serde_json::json;
use std::fs;

const KINDS: &str = r#"
api = 1
[[kind]]
id = "thing"
doc = "a thing"
# a project may write it while nothing widens: the project layer is never trusted
untrusted = "tighten"
[kind.fields]
name = { ty = "str", required = true }
size = { ty = "int", min = 1, max = 9, default = 3 }
"#;

struct Rig {
    s: Scratch,
    log: Log,
    folds: Folds,
    changes: Changes,
}

impl Rig {
    fn new(tag: &str) -> Rig {
        let s = Scratch::new(tag);
        let (log, _) = open_log(&s);
        let folds = Folds::new(&s.join("folds"));
        let changes = Changes::new();
        folds.add(Box::new(changes.clone()), 0);
        folds.attach(&log).unwrap();
        Rig { s, log, folds, changes }
    }

    fn dir(&self) -> std::path::PathBuf {
        self.s.join("project/.j4n")
    }

    fn tables(&self) -> Tables {
        let (project, _) = Layer::from_dir(LayerKind::Project, "project", &self.dir());
        let (t, problems) = Tables::load(vec![pack("p", KINDS), project]);
        assert!(refused(&problems).is_empty(), "{problems:?}");
        t
    }

    fn writer(&self) -> Writer {
        Writer::new(None, Some(self.dir()))
    }

    fn plan(&self, id: &str, op: Op, at_ms: u64) -> Change {
        let w = Write {
            to: Where::Project,
            file: None,
            kind: "thing".into(),
            id: id.into(),
            op,
            by: Actor::Operator,
            cause: None,
            at_ms,
        };
        self.tables().plan(&w).expect("plans").change
    }

    fn apply(&self, c: &Change) -> Result<(), String> {
        let mut j = LogJournal { log: &self.log };
        self.writer().apply(&self.tables(), c, None, &mut j).map(|_| ()).map_err(|e| e.to_string())
    }

    fn stack(&self) -> Vec<String> {
        self.changes.active(0).iter().map(|c| c.id.clone()).collect()
    }
}

fn set(body: &str) -> Op {
    Op::Set(body.parse().unwrap())
}

#[test]
fn a_change_is_in_the_log_before_the_file_changes_and_on_the_stack() {
    let rig = Rig::new("changes-basic");
    let c = rig.plan("a", set("name = \"Ada\""), 1_000);
    assert!(!rig.dir().join("thing.toml").exists());
    rig.apply(&c).unwrap();
    assert!(rig.dir().join("thing.toml").exists());
    let events = all(&rig.log);
    assert_eq!(events.len(), 1);
    assert_eq!((events[0].kind.as_str(), &events[0].by), (RECORDED, &Actor::Operator));
    let back: Change = serde_json::from_value(events[0].data.clone()).unwrap();
    assert_eq!(back, c, "the Log holds the whole change");
    assert_eq!(rig.stack(), std::slice::from_ref(&c.id));
}

#[test]
fn a_write_that_fails_after_it_was_recorded_says_so_and_leaves_the_stack() {
    let rig = Rig::new("changes-failed");
    fs::create_dir_all(rig.dir()).unwrap();
    // An entry written as an inline array is read but never edited: the writer refuses after the record.
    fs::write(rig.dir().join("thing.toml"), "api = 1\nthing = [{ id = \"a\", name = \"Ada\" }]\n").unwrap();
    let c = rig.plan("a", set("name = \"Ann\""), 1_000);
    let err = rig.apply(&c).unwrap_err();
    assert!(err.contains("inline"), "{err}");
    let kinds: Vec<String> = all(&rig.log).into_iter().map(|e| e.kind).collect();
    assert_eq!(kinds, [RECORDED, FAILED]);
    let failed = &all(&rig.log)[1];
    assert_eq!(failed.data["id"], c.id);
    assert!(failed.data["why"].as_str().unwrap().contains("inline"));
    assert!(rig.stack().is_empty());
}

#[test]
fn a_refusal_before_the_record_leaves_no_trace() {
    let rig = Rig::new("changes-refused");
    let c = rig.plan("a", set("name = \"Ada\""), 1_000);
    rig.apply(&c).unwrap();
    let again = rig.apply(&c).unwrap_err();
    assert!(again.contains("changed since"), "{again}");
    assert_eq!(all(&rig.log).len(), 1, "only the first was recorded");
}

#[test]
fn an_undo_is_a_change_and_so_is_taking_it_back() {
    let rig = Rig::new("changes-undo");
    let x = rig.plan("a", set("name = \"Ada\""), 1_000);
    rig.apply(&x).unwrap();
    let y = rig.plan("a", set("size = 7"), 2_000);
    rig.apply(&y).unwrap();
    assert_eq!(rig.stack(), [y.id.clone(), x.id.clone()], "newest first");

    let undo_y = y.inverse(Actor::Operator, 3_000);
    assert_eq!(undo_y.cause, Some(Cause::Undo(y.id.clone())));
    rig.apply(&undo_y).unwrap();
    assert_eq!(rig.stack(), std::slice::from_ref(&x.id), "y is undone, and the undo is not itself on the stack");
    assert!(!rig.s.read("project/.j4n/thing.toml").contains("size"));

    let redo = undo_y.inverse(Actor::Operator, 4_000);
    rig.apply(&redo).unwrap();
    assert_eq!(rig.stack(), [y.id.clone(), x.id.clone()], "taking the undo back brings y back");
    assert!(rig.s.read("project/.j4n/thing.toml").contains("size = 7"));

    // And across a restart: a new fold fed the same Log has the same stack.
    let fresh = Folds::new(&rig.s.join("folds2"));
    let again_changes = Changes::new();
    fresh.add(Box::new(again_changes.clone()), 0);
    fresh.attach(&rig.log).unwrap();
    let again: Vec<String> = again_changes.active(0).iter().map(|c| c.id.clone()).collect();
    assert_eq!(again, rig.stack());
}

#[test]
fn a_change_recorded_and_never_applied_is_found_by_comparing_with_the_tables() {
    let rig = Rig::new("changes-interrupted");
    let first = rig.plan("a", set("name = \"Ada\""), 1_000);
    rig.apply(&first).unwrap();
    // The process died after the record and before the rename.
    let second = rig.plan("a", set("name = \"Ann\""), 2_000);
    LogJournal { log: &rig.log }.record(&second).unwrap();
    let found = rig.changes.interrupted(&rig.tables());
    assert_eq!(found, std::slice::from_ref(&second.id));
    // Said, it is off the stack and not found again.
    LogJournal { log: &rig.log }.failed(&second, "interrupted");
    assert_eq!(rig.stack(), std::slice::from_ref(&first.id));
    assert!(rig.changes.interrupted(&rig.tables()).is_empty());
}

#[test]
fn a_change_that_was_applied_or_overtaken_by_a_hand_edit_is_not_interrupted() {
    let rig = Rig::new("changes-overtaken");
    let c = rig.plan("a", set("name = \"Ada\""), 1_000);
    rig.apply(&c).unwrap();
    let interrupted = || rig.changes.interrupted(&rig.tables());
    assert!(interrupted().is_empty(), "applied");
    let next = rig.plan("a", set("name = \"Ann\""), 2_000);
    LogJournal { log: &rig.log }.record(&next).unwrap();
    assert_eq!(interrupted(), std::slice::from_ref(&next.id), "recorded, not applied");
    fs::write(rig.dir().join("thing.toml"), "api = 1\n[[thing]]\nid = \"a\"\nname = \"Bo\"\n").unwrap();
    assert!(interrupted().is_empty(), "someone edited the file to a third thing: it is theirs, not ours to call unapplied");
}

#[test]
fn only_the_latest_change_to_a_thing_can_be_unapplied() {
    let rig = Rig::new("changes-latest");
    let a = rig.plan("a", set("name = \"Ada\""), 1_000);
    LogJournal { log: &rig.log }.record(&a).unwrap();
    let b = rig.plan("a", set("name = \"Ann\""), 2_000);
    LogJournal { log: &rig.log }.record(&b).unwrap();
    let found = rig.changes.interrupted(&rig.tables());
    assert_eq!(found, std::slice::from_ref(&b.id), "a was overtaken by b, which is the one to look at");
}

#[test]
fn session_changes_and_unloaded_layers_are_not_judged() {
    let rig = Rig::new("changes-session");
    let mut c = rig.plan("a", set("name = \"Ada\""), 1_000);
    c.target.layer = Where::Session;
    c.target.file = String::new();
    LogJournal { log: &rig.log }.record(&c).unwrap();
    let (stack_from, interrupted) = (rig.changes.active(0).len(), rig.changes.interrupted(&rig.tables()));
    assert_eq!(stack_from, 1);
    assert!(interrupted.is_empty());
    let head = rig.log.head();
    assert_eq!(rig.changes.active(head).len(), 1, "a session change from this run");
    assert_eq!(rig.changes.active(head + 1).len(), 0, "one from a run that ended");
    // A change to a layer the tables do not have.
    let mut home = rig.plan("b", set("name = \"Bo\""), 2_000);
    home.target.layer = Where::User;
    LogJournal { log: &rig.log }.record(&home).unwrap();
    assert!(rig.changes.interrupted(&rig.tables()).is_empty());
}

#[test]
fn the_stack_is_as_deep_as_the_fold_keeps() {
    let rig = Rig::new("changes-keep");
    let mut ids = Vec::new();
    for i in 0..1010u64 {
        let c = rig.plan("a", set(&format!("name = \"n{i}\"")), 1_000 + i);
        LogJournal { log: &rig.log }.record(&c).unwrap();
        ids.push(c.id);
    }
    let stack = rig.stack();
    assert_eq!(stack.len(), 1000);
    assert_eq!(stack[0], ids[1009]);
    assert_eq!(stack[999], ids[10]);
}

#[test]
fn the_fold_ignores_what_it_cannot_read_and_what_it_has_already() {
    let rig = Rig::new("changes-forward");
    let c = rig.plan("a", set("name = \"Ada\""), 1_000);
    rig.apply(&c).unwrap();
    LogJournal { log: &rig.log }.record(&c).unwrap();
    rig.log.append(New::of(RECORDED, Actor::Core, json!({"from": "a newer version"}))).unwrap();
    rig.log.append(New::of(FAILED, Actor::Core, json!({"no": "id"}))).unwrap();
    rig.log.append(New::of(FAILED, Actor::Core, json!({"id": "nobody"}))).unwrap();
    assert_eq!(rig.stack(), std::slice::from_ref(&c.id), "once, though it was recorded twice");
}

#[test]
fn the_fold_of_changes_survives_a_snapshot_and_reads_as_rows() {
    let rig = Rig::new("changes-snapshot");
    let c = rig.plan("a", set("name = \"Ada\""), 1_000);
    rig.apply(&c).unwrap();
    let state = rig.folds.state("changes").unwrap();
    let mut other = Changes::new();
    other.load(state.clone()).unwrap();
    assert_eq!(other.save(), state);
    assert_eq!(other.active(0).len(), 1);
    assert!(other.load(json!({"not": "a list"})).is_err());
    other.reset();
    assert!(other.active(0).is_empty());
    let rows = rig.folds.read("changes", &[], 0);
    let Value::List(rows) = rows else { panic!("a list") };
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].at(&[j4n_core::value::Key::Name("entry".into())]), Value::text("a"));
    assert_eq!(rows[0].at(&[j4n_core::value::Key::Name("layer".into())]), Value::text("project"));
    assert_eq!(rows[0].at(&[j4n_core::value::Key::Name("by".into())]), Value::text("operator"));
}


fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git is installed");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn a_fresh_projects_state_directory_leaves_git_status_empty() {
    let s = Scratch::new("s28");
    s.put("src/main.rs", "fn main() {}\n");
    git(s.path(), &["init", "-q"]);
    git(s.path(), &["add", "-A"]);
    git(s.path(), &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "start"]);
    assert_eq!(git(s.path(), &["status", "--porcelain"]), "");
    // Everything J4n keeps in a project: the log, a blob, a fold snapshot, a table write.
    let root = s.join(".j4n");
    let clock = std::sync::Arc::new(j4n_core::clock::ManualClock::at(1));
    let (log, _) = j4n_core::log::Log::open(&root, j4n_core::log::Scope::Project, opts(&clock), j4n_core::redact::Redactor::new()).unwrap();
    log.append(ev("a.b", json!(1))).unwrap();
    log.blobs().put(b"output").unwrap();
    let folds = Folds::new(&root.join("folds"));
    folds.add(Box::new(Changes::new()), 0);
    folds.attach(&log).unwrap();
    log.sync();
    assert!(root.join("folds").is_dir() && root.join("blobs").is_dir() && root.join("log").is_dir());
    drop(log);
    assert_eq!(git(s.path(), &["status", "--porcelain"]), "", "the directory shows nothing");
    assert_eq!(git(s.path(), &["status", "--porcelain", "--ignored"]).trim(), "!! .j4n/", "and git knows it is ignored");
}

#[test]
fn a_table_write_that_makes_the_project_directory_makes_its_gitignore_too() {
    let rig = Rig::new("changes-ignore");
    assert!(!rig.dir().exists());
    let c = rig.plan("a", set("name = \"Ada\""), 1_000);
    rig.writer().apply(&rig.tables(), &c, None, &mut NoJournal).unwrap();
    assert_eq!(rig.s.read("project/.j4n/.gitignore"), "*\n");
    // A directory somebody made keeps what it ignores.
    let other = Scratch::new("changes-ignore-kept");
    other.put(".j4n/.gitignore", "log/\n");
    let w = Writer::new(None, Some(other.join(".j4n")));
    let (project, _) = Layer::from_dir(LayerKind::Project, "project", &other.join(".j4n"));
    let (t, _) = Tables::load(vec![pack("p", KINDS), project]);
    let plan = t
        .plan(&Write { to: Where::Project, file: None, kind: "thing".into(), id: "a".into(), op: set("name = \"Ada\""), by: Actor::Operator, cause: None, at_ms: 1 })
        .unwrap();
    w.apply(&t, &plan.change, None, &mut NoJournal).unwrap();
    assert_eq!(other.read(".j4n/.gitignore"), "log/\n");
}

#[test]
fn the_event_that_caused_a_change_is_on_the_record() {
    let rig = Rig::new("changes-cause");
    let w = Write {
        to: Where::Project,
        file: None,
        kind: "thing".into(),
        id: "a".into(),
        op: set("name = \"Ada\""),
        by: Actor::Core,
        cause: Some(Cause::Event(7)),
        at_ms: 1,
    };
    let c = rig.tables().plan(&w).unwrap().change;
    let mut j = LogJournal { log: &rig.log };
    j.record(&c).unwrap();
    let recorded = &all(&rig.log)[0];
    assert_eq!((recorded.cause, &recorded.by), (Some(7), &Actor::Core));
    let other = Cause::Proposal("p1".into());
    let w2 = Write { cause: Some(other.clone()), id: "b".into(), ..w };
    let c2 = rig.tables().plan(&w2).unwrap().change;
    j.record(&c2).unwrap();
    assert_eq!(all(&rig.log)[1].cause, None, "only an event is a cause in the Log's sense; the proposal is in the change");
}

#[test]
fn a_directory_somebody_made_is_not_given_a_gitignore() {
    let s = Scratch::new("changes-existing");
    s.put(".j4n/settings.toml", "api = 1\n");
    let clock = std::sync::Arc::new(j4n_core::clock::ManualClock::at(1));
    drop(j4n_core::log::Log::open(&s.join(".j4n"), j4n_core::log::Scope::Project, opts(&clock), j4n_core::redact::Redactor::new()).unwrap());
    assert!(!s.exists(".j4n/.gitignore"), "it was there before J4n, and what git sees of it is the owner's to say");
}
