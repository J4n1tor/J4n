//! Proposals: nothing is written until the operator says yes; an accept is a set of changes with one cause, and an undo takes
//! them back, across a restart; a declined proposal returns when its evidence has doubled; what widens takes two presses;
//! what a model drafts goes through the author gate.

use crate::world::*;
use j4n_core::change::{Actor, Cause, Change, Where};
use j4n_core::changes::Changes;
use j4n_core::fold::Folds;
use j4n_core::log::Log;
use j4n_core::proposal::*;
use j4n_core::table::{Layer, LayerKind, Planned, Problem, Tables, Write};
use j4n_core::write::{Applied, Journal, Writer};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const KINDS: &str = r#"
api = 1
[[kind]]
id = "thing"
doc = "a thing"
[kind.fields]
name = { ty = "str", required = true }
size = { ty = "int", min = 1, max = 9, default = 3 }
mode = { ty = "enum:strict|loose", default = "strict", widens = ["loose"] }
"#;

fn tables_in(dir: &Path) -> Tables {
    // the operator's home layer: the one a proposal writes (a project layer is never trusted)
    let (home, _) = Layer::from_dir(LayerKind::User, "home", dir);
    let (t, problems) = Tables::load(vec![pack("p", KINDS), home]);
    assert!(refused(&problems).is_empty(), "{problems:?}");
    t
}

fn plant(dir: &Path) -> Plant {
    let d = dir.to_path_buf();
    Plant { load: Box::new(move || tables_in(&d)), writer: Writer::new(Some(dir.to_path_buf()), None) }
}

struct Rig {
    s: Scratch,
    log: Log,
    folds: Folds,
    changes: Changes,
    proposals: Proposals,
    proposer: Proposer,
}

fn clock() -> Arc<dyn Fn() -> u64 + Send + Sync> {
    let n = AtomicU64::new(10_000);
    Arc::new(move || n.fetch_add(1_000, Ordering::SeqCst))
}

impl Rig {
    fn new(tag: &str) -> Rig {
        Rig::with(tag, |dir| Arc::new(plant(dir)))
    }

    fn with(tag: &str, hands: impl Fn(&Path) -> Arc<dyn Hands>) -> Rig {
        let s = Scratch::new(tag);
        let (log, _) = open_log(&s);
        let (folds, changes, proposals) = (Folds::new(&s.join("folds")), Changes::new(), Proposals::new());
        folds.add(Box::new(changes.clone()), 0);
        folds.add(Box::new(proposals.clone()), 0);
        folds.attach(&log).unwrap();
        let dir = s.join("home");
        let proposer = Proposer::new(log.clone(), proposals.clone(), changes.clone(), hands(&dir), clock());
        Rig { s, log, folds, changes, proposals, proposer }
    }

    fn dir(&self) -> PathBuf {
        self.s.join("home")
    }

    fn file(&self) -> String {
        fs::read_to_string(self.dir().join("thing.toml")).unwrap_or_default()
    }

    fn make(&self, d: Draft) -> Option<String> {
        self.proposer.make(d, Actor::Rule("grow".into()), None).expect("made")
    }

    fn state(&self, id: &str) -> &'static str {
        self.log.sync();
        self.proposals.get(id).map_or("none", |p| p.fate.word())
    }
}

fn set(id: &str, fields: &str) -> Step {
    Step { to: Where::User, file: None, kind: "thing".into(), id: id.into(), op: StepOp::Set(fields.parse().unwrap()) }
}

fn draft(key: &str, evidence: u64, steps: Vec<Step>) -> Draft {
    Draft { key: key.into(), title: format!("do {key}"), why: format!("{evidence} times"), evidence, steps, widens: Vec::new() }
}

#[test]
fn a_proposal_writes_no_file_until_it_is_accepted_and_undo_puts_it_back() {
    let rig = Rig::new("prop-roundtrip");
    let id = rig.make(draft("a", 3, vec![set("a", "name = \"Ada\"")])).expect("taken");
    assert_eq!(rig.state(&id), "open");
    assert!(rig.file().is_empty(), "making a proposal changes nothing");

    let Accepted::Done(changes) = rig.proposer.accept(&id).unwrap() else { panic!("applies") };
    assert_eq!(changes.len(), 1);
    assert!(rig.file().contains("name = \"Ada\""));
    assert_eq!(changes[0].cause, Some(Cause::Proposal(id.clone())), "the change says which proposal it followed");
    assert_eq!(changes[0].by, Actor::Operator, "the operator's yes is who made it");
    assert_eq!(rig.state(&id), "accepted");

    let undone = rig.proposer.undo(&id).unwrap();
    assert_eq!(undone.len(), 1);
    assert!(!rig.file().contains("Ada"), "{}", rig.file());
    assert_eq!(rig.state(&id), "undone");
    assert!(rig.changes.active(0).is_empty(), "the stack is empty again");
}

#[test]
fn a_proposal_of_several_steps_is_one_accept_and_one_undo() {
    let rig = Rig::new("prop-steps");
    let id = rig.make(draft("two", 1, vec![set("a", "name = \"A\""), set("b", "name = \"B\"")])).unwrap();
    let Accepted::Done(changes) = rig.proposer.accept(&id).unwrap() else { panic!() };
    assert_eq!(changes.len(), 2);
    assert!(rig.file().contains("\"A\"") && rig.file().contains("\"B\""), "the second step saw the file the first left");
    rig.proposer.undo(&id).unwrap();
    assert!(!rig.file().contains("\"A\"") && !rig.file().contains("\"B\""), "{}", rig.file());
}

#[test]
fn undo_without_a_name_takes_back_the_newest_change() {
    let rig = Rig::new("prop-undo-last");
    let a = rig.make(draft("a", 1, vec![set("a", "name = \"A\"")])).unwrap();
    let b = rig.make(draft("b", 1, vec![set("b", "name = \"B\"")])).unwrap();
    rig.proposer.accept(&a).unwrap();
    rig.proposer.accept(&b).unwrap();
    rig.proposer.undo("").unwrap();
    assert!(rig.file().contains("\"A\"") && !rig.file().contains("\"B\""), "{}", rig.file());
    assert_eq!((rig.state(&a), rig.state(&b)), ("accepted", "undone"));
    rig.proposer.undo("").unwrap();
    assert!(!rig.file().contains("\"A\""));
    let none = rig.proposer.undo("").unwrap_err();
    assert!(none.msg.contains("nothing to undo"), "{}", none.msg);
}

#[test]
fn a_declined_proposal_stays_gone_until_its_evidence_has_doubled() {
    let rig = Rig::new("prop-decline");
    let id = rig.make(draft("tip", 5, vec![set("a", "name = \"A\"")])).unwrap();
    rig.proposer.decline(&id).unwrap();
    assert_eq!(rig.state(&id), "declined");
    assert!(rig.file().is_empty());
    assert!(rig.proposer.accept(&id).unwrap_err().msg.contains("declined already"));

    for evidence in [5, 6, 9] {
        assert_eq!(rig.make(draft("tip", evidence, vec![set("a", "name = \"A\"")])), None, "{evidence} is not double of 5");
    }
    let again = rig.make(draft("tip", 10, vec![set("a", "name = \"A\"")])).expect("ten is");
    assert_ne!(again, id, "it is a new proposal");
    assert_eq!(rig.state(&again), "open");
}

#[test]
fn the_same_proposal_made_again_raises_the_one_that_is_open() {
    let rig = Rig::new("prop-merge");
    let id = rig.make(draft("tip", 3, vec![set("a", "name = \"A\"")])).unwrap();
    assert_eq!(rig.make(draft("tip", 3, vec![set("a", "name = \"A\"")])), None, "no more evidence, nothing to say");
    assert_eq!(rig.make(draft("tip", 7, vec![set("a", "name = \"A\"")])), Some(id.clone()), "more evidence: the same one");
    rig.log.sync();
    assert_eq!(rig.proposals.open().len(), 1);
    assert_eq!(rig.proposals.get(&id).unwrap().draft.evidence, 7);
}

#[test]
fn an_accepted_proposal_does_not_come_back() {
    let rig = Rig::new("prop-accepted");
    let id = rig.make(draft("tip", 3, vec![set("a", "name = \"A\"")])).unwrap();
    rig.proposer.accept(&id).unwrap();
    assert_eq!(rig.make(draft("tip", 300, vec![set("a", "name = \"A\"")])), None);
    assert!(rig.proposer.accept(&id).unwrap_err().msg.contains("accepted already"));
}

#[test]
fn what_widens_takes_two_presses() {
    let rig = Rig::new("prop-widen");
    let id = rig.make(draft("loose", 1, vec![set("a", "name = \"A\"\nmode = \"loose\"")])).unwrap();
    assert_eq!(rig.proposals.get(&id).map(|p| p.widens), Some(vec!["mode".to_string()]), "it says what it widens when it is made");
    let first = rig.proposer.accept(&id).unwrap();
    assert_eq!(first, Accepted::Confirm(vec!["mode".into()]));
    assert!(rig.file().is_empty(), "the first press writes nothing");
    let other = rig.make(draft("plain", 1, vec![set("b", "name = \"B\"")])).unwrap();
    assert!(matches!(rig.proposer.accept(&other).unwrap(), Accepted::Done(_)), "a plain one takes one press");
    assert_eq!(rig.proposer.accept(&id).unwrap(), Accepted::Confirm(vec!["mode".into()]), "another accept between did not count as a second press");
}

#[test]
fn the_second_press_applies_it() {
    let rig = Rig::new("prop-widen-twice");
    let id = rig.make(draft("loose", 1, vec![set("a", "name = \"A\"\nmode = \"loose\"")])).unwrap();
    assert!(matches!(rig.proposer.accept(&id).unwrap(), Accepted::Confirm(_)));
    assert!(matches!(rig.proposer.accept(&id).unwrap(), Accepted::Done(_)));
    assert!(rig.file().contains("loose"));
}

/// Hands that refuse the nth apply.
struct Flaky {
    inner: Plant,
    applies: AtomicU64,
    fail_at: u64,
}

impl Hands for Flaky {
    fn plan(&self, w: &Write) -> Result<Planned, Problem> {
        self.inner.plan(w)
    }

    fn apply(&self, p: &Planned, journal: &mut dyn Journal) -> Result<Applied, Problem> {
        if self.applies.fetch_add(1, Ordering::SeqCst) + 1 == self.fail_at {
            return Err(Problem::refused("flaky", "the disk said no"));
        }
        self.inner.apply(p, journal)
    }

    fn redo(&self, c: &Change, journal: &mut dyn Journal) -> Result<Applied, Problem> {
        self.inner.redo(c, journal)
    }
}

#[test]
fn a_step_that_is_refused_takes_back_the_ones_before_it_and_the_proposal_stays_open() {
    let rig = Rig::with("prop-flaky", |dir| Arc::new(Flaky { inner: plant(dir), applies: AtomicU64::new(0), fail_at: 2 }));
    let id = rig.make(draft("two", 1, vec![set("a", "name = \"A\""), set("b", "name = \"B\"")])).unwrap();
    let err = rig.proposer.accept(&id).unwrap_err();
    assert!(err.msg.contains("disk said no") && err.msg.contains("nothing was changed"), "{}", err.msg);
    assert!(!rig.file().contains("\"A\"") && !rig.file().contains("\"B\""), "step one was taken back: {}", rig.file());
    assert_eq!(rig.state(&id), "open", "the operator can try again");
    assert!(rig.changes.active(0).is_empty(), "and the stack shows nothing in effect");
    assert!(matches!(rig.proposer.accept(&id).unwrap(), Accepted::Done(_)), "the disk is fine now");
    assert!(rig.file().contains("\"A\"") && rig.file().contains("\"B\""));
}

#[test]
fn a_draft_that_cannot_be_applied_is_refused_when_it_is_made() {
    let rig = Rig::new("prop-bad");
    let bad = |step: Step| rig.proposer.make(draft("bad", 1, vec![step]), Actor::Rule("grow".into()), None).unwrap_err().msg;
    assert!(bad(set("a", "size = 99")).contains("size"), "the schema's limits hold at the proposal");
    let mut nokind = set("a", "name = \"A\"");
    nokind.kind = "nothing".into();
    assert!(bad(nokind).contains("no kind"));
    assert!(rig.proposer.make(draft("empty", 1, vec![]), Actor::Core, None).is_err());
    rig.log.sync();
    assert!(rig.proposals.list().is_empty(), "none of them was written");
}

#[test]
fn what_a_model_drafts_goes_through_the_author_gate() {
    let rig = Rig::new("prop-author");
    let by = Actor::Model("m".into());
    let view = Step {
        to: Where::User,
        file: None,
        kind: "view".into(),
        id: "mine".into(),
        op: StepOp::Set("from = { from = \"fold:answerers\" }\ncells = [\"{id}\"]".parse().unwrap()),
    };
    assert!(rig.proposer.make(draft("v", 1, vec![view.clone()]), by.clone(), None).unwrap().is_some(), "a view in a file is allowed");
    let session = Step { to: Where::Session, ..view.clone() };
    let e = rig.proposer.make(draft("u", 1, vec![session]), by.clone(), None).unwrap_err().msg;
    assert!(e.contains("not the session"), "{e}");
    let e = rig.proposer.make(draft("t", 1, vec![set("a", "name = \"A\"")]), by.clone(), None).unwrap_err().msg;
    assert!(e.contains("not a kind it may draft"), "{e}");
    for kind in ["connector", "gate", "model", "enable", "rule", "flow", "setting", "provider"] {
        let step = Step { kind: kind.into(), ..view.clone() };
        assert!(author_vet(&draft("k", 1, vec![step])).is_err(), "{kind} is not for an author");
    }
    assert!(rig.proposer.make(draft("t", 1, vec![set("a", "name = \"A\"")]), Actor::Rule("grow".into()), None).is_ok(), "a rule of the pack is data, not an author");
}

#[test]
fn proposals_and_changes_come_back_after_a_restart() {
    let rig = Rig::new("prop-restart");
    let a = rig.make(draft("a", 1, vec![set("a", "name = \"A\"")])).unwrap();
    let b = rig.make(draft("b", 1, vec![set("b", "name = \"B\"")])).unwrap();
    rig.proposer.accept(&a).unwrap();
    rig.log.sync();

    let (folds, changes, proposals) = (Folds::new(&rig.s.join("folds2")), Changes::new(), Proposals::new());
    folds.add(Box::new(changes.clone()), 0);
    folds.add(Box::new(proposals.clone()), 0);
    folds.attach(&rig.log).unwrap();
    rig.log.sync();
    let again = Proposer::new(rig.log.clone(), proposals.clone(), changes, Arc::new(plant(&rig.dir())), clock());
    assert_eq!((proposals.get(&a).unwrap().fate.word(), proposals.get(&b).unwrap().fate.word()), ("accepted", "open"));
    again.undo(&a).unwrap();
    assert!(!rig.file().contains("\"A\""), "the new process took back what the old one wrote");
    assert!(matches!(again.accept(&b).unwrap(), Accepted::Done(_)));
    let _ = &rig.folds;
}

#[test]
fn the_fold_reads_as_rows_for_a_view() {
    let rig = Rig::new("prop-rows");
    let id = rig.make(draft("a", 4, vec![set("a", "name = \"A\"")])).unwrap();
    rig.log.sync();
    let rows = rig.folds.read("proposals", &[], 0);
    let j4n_core::value::Value::List(rows) = rows else { panic!("{rows:?}") };
    assert_eq!(rows.len(), 1);
    let at = |k: &str| rows[0].at(&[j4n_core::value::Key::Name(k.into())]);
    use j4n_core::value::Value as V;
    assert_eq!(at("id"), V::text(id));
    assert_eq!((at("state"), at("by"), at("evidence")), (V::text("open"), V::text("rule:grow"), V::num(4.0)));
}

// ---- the operator's edit is the yes ----

#[test]
fn an_operators_edit_applies_at_once_with_no_proposal_and_one_undo_takes_all_its_steps_back() {
    let rig = Rig::new("ts2-edit");
    let Edited::Done { id, changes } = rig.proposer.edit(draft("e", 1, vec![set("a", "name = \"A\""), set("b", "name = \"B\"")])).unwrap() else { panic!("applies at once") };
    assert_eq!(id, "e1", "the first edit is e1");
    assert!(is_edit_id(&id) && !is_edit_id("p3") && !is_edit_id("e3f9a1c2d4e5f6a7"));
    assert_eq!(changes.len(), 2);
    assert!(changes.iter().all(|c| c.cause == Some(Cause::Edit(id.clone())) && c.by == Actor::Operator), "{changes:?}");
    assert!(rig.file().contains("\"A\"") && rig.file().contains("\"B\""));
    rig.log.sync();
    assert!(rig.proposals.list().is_empty(), "no proposal is made: Grow has nothing of it");
    let kinds: Vec<String> = rig.log.read(0).map(|e| e.kind).collect();
    assert!(kinds.contains(&EDITED.to_string()) && !kinds.contains(&MADE.to_string()), "{kinds:?}");
    // the next one is e2, and `:undo` with no name takes the newest edit back whole
    let Edited::Done { id: second, .. } = rig.proposer.edit(draft("f", 1, vec![set("c", "name = \"C\"")])).unwrap() else { panic!() };
    assert_eq!(second, "e2");
    rig.proposer.undo("").unwrap();
    assert!(!rig.file().contains("\"C\"") && rig.file().contains("\"A\""), "{}", rig.file());
    // an edit named by its id, or by the id of one of its changes, goes back whole
    rig.proposer.undo(&changes[0].id).unwrap();
    assert!(!rig.file().contains("\"A\"") && !rig.file().contains("\"B\""), "{}", rig.file());
    assert!(rig.changes.active(0).is_empty());
    assert!(rig.proposer.undo(&id).is_err(), "nothing of it is in effect any more");
}

#[test]
fn an_edit_that_widens_is_still_a_proposal_asked_twice() {
    let rig = Rig::new("ts2-widen");
    let Edited::Proposed { id, widens } = rig.proposer.edit(draft("w", 1, vec![set("a", "name = \"A\"\nmode = \"loose\"")])).unwrap() else { panic!("widening is proposed") };
    assert!(!widens.is_empty() && id.starts_with('p'));
    assert!(rig.file().is_empty(), "nothing is written before the two presses");
    assert!(matches!(rig.proposer.accept(&id).unwrap(), Accepted::Confirm(_)));
    assert!(matches!(rig.proposer.accept(&id).unwrap(), Accepted::Done(_)));
    rig.log.sync();
    assert_eq!(rig.proposals.rows().len(), 1, "a widening proposal of the operator's stays a row of Grow");
}

#[test]
fn grow_holds_no_row_for_an_operators_line_a_log_of_before_kept_as_a_proposal_accepted_at_once() {
    let rig = Rig::new("ts2-old-log");
    // how `:look` wrote before the live edits: made by the operator, accepted at once
    let mine = rig.proposer.make(draft("look:palette", 1, vec![set("a", "name = \"A\"")]), Actor::Operator, None).unwrap().unwrap();
    rig.proposer.accept(&mine).unwrap();
    let grow = rig.make(draft("grow", 1, vec![set("b", "name = \"B\"")])).unwrap();
    rig.log.sync();
    let ids: Vec<j4n_core::value::Value> = rig.proposals.rows().iter().map(|r| r.pointer("/id")).collect();
    assert_eq!(ids, vec![j4n_core::value::Value::text(&grow)], "only what J4n drafted");
    assert!(rig.proposals.get(&mine).is_some(), "the old one is kept, so `:undo {mine}` still works");
    rig.proposer.undo(&mine).unwrap();
    assert!(!rig.file().contains("\"A\""));
}

#[test]
fn every_edit_accept_and_undo_tells_who_follows_them() {
    let rig = Rig::new("ts2-told");
    let n = Arc::new(AtomicU64::new(0));
    let m = n.clone();
    rig.proposer.on_change(Arc::new(move || {
        m.fetch_add(1, Ordering::SeqCst);
    }));
    rig.proposer.edit(draft("e", 1, vec![set("a", "name = \"A\"")])).unwrap();
    let id = rig.make(draft("g", 1, vec![set("b", "name = \"B\"")])).unwrap();
    assert_eq!(n.load(Ordering::SeqCst), 1, "making a proposal changes nothing");
    rig.proposer.accept(&id).unwrap();
    rig.proposer.undo("").unwrap();
    assert_eq!(n.load(Ordering::SeqCst), 3);
}
