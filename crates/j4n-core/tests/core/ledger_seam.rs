//! The seam of the ledger: with a ledger whose switch is on, the plan keeper still writes its `plan.reported` but says no note of
//! its own (the ledger carries it), and the ledger is asked at the settled boundary (that it is not asked while a task of the thread is open is held elsewhere); with the switch off the front
//! is what it was. A stand-in ledger counts what it was asked.

use crate::branch_validate::{caps, checker, doer, item, no_replan, rig_cfg};
use crate::scripted::{Beat, Scripted, end};
use j4n_core::front::Cfg;
use j4n_core::front::ledger::Ledger;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Stub {
    on: bool,
    asked: &'static AtomicUsize,
}

impl Ledger for Stub {
    fn on(&self) -> bool {
        self.on
    }

    fn settled(&self, thread: &str) {
        assert_eq!(thread, "main");
        self.asked.fetch_add(1, Ordering::SeqCst);
    }

    fn command(&self, _thread: &str, _args: &[String]) -> Result<(String, Vec<(u8, String)>), String> {
        Ok(("the report".into(), Vec::new()))
    }
}

static ON_ASKED: AtomicUsize = AtomicUsize::new(0);
static OFF_ASKED: AtomicUsize = AtomicUsize::new(0);

fn on(c: &mut Cfg) {
    no_replan(c);
    c.ledger = Some(Arc::new(Stub { on: true, asked: &ON_ASKED }));
}

fn off(c: &mut Cfg) {
    no_replan(c);
    c.ledger = Some(Arc::new(Stub { on: false, asked: &OFF_ASKED }));
}

fn scouts() -> Arc<Scripted> {
    Scripted::with_caps("pi", caps(), (0..2).map(|_| vec![Beat::Session("pi-s".into()), Beat::Say("looked".into()), end("looked")]).collect())
}

/// A plan of two scouts and an inline item, run to its end; the notes the chat got.
fn run(cfg: fn(&mut Cfg)) -> (Vec<String>, usize) {
    let r = rig_cfg(vec![item("a", "scout", &["src/a.rs"], &[], &[]), item("b", "scout", &["src/b.rs"], &[], &[]), item("go", "inline", &[], &["src/o.rs"], &["a", "b"])], scouts(), doer("own", &[]), checker(1), &["went on"], cfg);
    r.begin();
    r.until("the plan was done", |r| r.item("go").state == "merged");
    r.step();
    let notes = r.f.rig.of_kind("chat.note").iter().map(|e| e.data["text"].as_str().unwrap_or_default().to_string()).collect();
    (notes, r.f.rig.of_kind("plan.reported").len())
}

#[test]
fn with_the_ledger_on_the_plan_report_is_written_and_says_no_note_of_its_own_and_the_ledger_is_asked() {
    let (notes, reported) = run(on);
    assert_eq!(reported, 1, "the plan's report is still the Log's");
    assert!(!notes.iter().any(|n| n.starts_with("plan done")), "the ledger carries it: {notes:?}");
    assert!(ON_ASKED.load(Ordering::SeqCst) >= 1, "the ledger was asked at a settled boundary");
}

#[test]
fn with_the_ledger_off_the_plan_report_is_the_note_it_was_and_the_ledger_is_never_asked() {
    let (notes, reported) = run(off);
    assert_eq!(reported, 1);
    assert!(notes.iter().any(|n| n.starts_with("plan done: 3/3 done")), "{notes:?}");
    assert_eq!(OFF_ASKED.load(Ordering::SeqCst), 0);
}
