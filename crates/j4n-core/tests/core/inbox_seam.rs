//! The seam of the inbox: what the front does with what an `Inbox` says, with a stand-in for the crate that
//! decides. The crate's own cases are in `crates/j4n/tests/laws/inbox_flow.rs`; here are the two conditions that the scripted `test`
//! doer cannot vary: a doer that cannot take a live steer, and a doer that takes no Brief.

use crate::front_cases::fx;
use crate::scripted::{Beat, Scripted, end, say};
use j4n_core::doer::{Caps, RunCtl};
use j4n_core::front::Said;
use j4n_core::front::inbox::{Due, Inbox, Kept, Over};
use j4n_core::task::State;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Stub {
    /// What was told to it: `sent <state> <task>`, `back`.
    told: Mutex<Vec<String>>,
    /// The next `due`.
    due: Mutex<Option<Due>>,
}

impl Inbox for Stub {
    fn on(&self) -> bool {
        true
    }

    fn take(&self, _over: &Over) -> Option<Kept> {
        Some(Kept { id: "i1".into(), tag: "steer", note: "kept".into(), now: true })
    }

    fn settled(&self, _id: &str, _state: &str) {}

    fn due(&self, _thread: &str, _work: bool) -> Option<Due> {
        self.due.lock().unwrap().take()
    }

    fn side(&self, _thread: &str) -> Option<(String, String)> {
        None
    }

    fn sent(&self, ids: &[String], state: &str, task: &str) {
        self.told.lock().unwrap().push(format!("sent {} {state} {task}", ids.join(",")));
    }

    fn back(&self, ids: &[String]) {
        self.told.lock().unwrap().push(format!("back {}", ids.join(",")));
    }

    fn command(&self, _thread: &str, _args: &[String]) -> Result<String, String> {
        Ok(String::new())
    }
}

fn due() -> Due {
    Due { ids: vec!["i1".into(), "i2".into()], text: "1. first\n2. second".into(), line: "the lines are in your brief".into(), plain: "the lines, numbered:\n\n1. first\n2. second".into() }
}

fn caps(live: bool, brief: bool) -> Caps {
    Caps { live, resume: true, brief, ..Caps::default() }
}

#[test]
fn a_line_that_is_to_steer_at_once_reaches_only_a_task_whose_doer_takes_a_live_steer() {
    for (live, steered) in [(true, true), (false, false)] {
        let doer = Scripted::with_caps("claude", caps(live, false), vec![vec![Beat::Session("s".into()), Beat::Forever]]);
        let stub = Arc::new(Stub::default());
        let f = fx(vec![doer.clone()], |c| {
            c.doer = Some("claude".into());
            c.inbox = Some(stub.clone());
        });
        let Said::Started { task } = f.say("work on the parser") else { panic!("{}", f.rig.dump()) };
        f.rig.until(&task, "running", |r| r.state == State::Running);
        let said = f.say("and the errors");
        let reached = doer.ctls().iter().any(|c| matches!(c, RunCtl::Steer(t) if t == "and the errors"));
        assert_eq!((reached, matches!(said, Said::Steered { .. })), (steered, steered), "live = {live}: {said:?}");
        assert_eq!(stub.told.lock().unwrap().clone(), if steered { vec![format!("sent i1 delivered {task}")] } else { Vec::new() });
        f.say(":stop");
        f.rig.finished(&task);
    }
}

#[test]
fn what_is_due_is_the_brief_section_for_a_doer_with_a_brief_and_the_line_itself_for_one_without_and_nothing_after_a_failure() {
    for brief in [true, false] {
        let doer = Scripted::with_caps("claude", caps(true, brief), vec![vec![Beat::Session("s".into()), say("first"), end("first")], vec![Beat::Session("s".into()), say("again"), end("again")]]);
        let stub = Arc::new(Stub::default());
        let f = fx(vec![doer], |c| {
            c.doer = Some("claude".into());
            c.inbox = Some(stub.clone());
        });
        f.turn("work on the parser");
        *stub.due.lock().unwrap() = Some(due());
        f.front.inbox_step("main");
        let rows = f.tasks();
        assert_eq!(rows.len(), 2, "brief = {brief}\n{}", f.rig.dump());
        let next = f.rig.finished(&rows[1].id);
        if brief {
            assert_eq!(next.spec.goal, "the lines are in your brief");
            assert!(next.spec.input["brief"].as_str().unwrap().contains("## inbox\n1. first\n2. second"), "{}", next.spec.input);
        } else {
            assert_eq!(next.spec.goal, "the lines, numbered:\n\n1. first\n2. second");
            assert!(!next.spec.input.to_string().contains("first"), "{}", next.spec.input);
        }
        assert_eq!(stub.told.lock().unwrap().clone(), [format!("sent i1,i2 delivered {}", next.id)]);
    }
    // a task that failed starts nothing, and the stand-in is not asked
    let doer = Scripted::with_caps("claude", caps(true, true), vec![vec![Beat::Fail("no".into())]]);
    let stub = Arc::new(Stub::default());
    let f = fx(vec![doer], |c| {
        c.doer = Some("claude".into());
        c.inbox = Some(stub.clone());
    });
    f.turn("work on the parser");
    *stub.due.lock().unwrap() = Some(due());
    f.front.inbox_step("main");
    assert_eq!(f.tasks().len(), 1, "{}", f.rig.dump());
    assert!(stub.due.lock().unwrap().is_some() && stub.told.lock().unwrap().is_empty());
}

#[test]
fn a_turn_that_cannot_start_gives_what_it_claimed_back() {
    let doer = Scripted::with_caps("claude", caps(true, true), vec![vec![Beat::Session("s".into()), say("first"), end("first")]]);
    let stub = Arc::new(Stub::default());
    // the time the process was given runs out after the first turn: the next cannot start
    let f = fx(vec![doer], |c| {
        c.doer = Some("claude".into());
        c.inbox = Some(stub.clone());
        c.deadline = Some(std::time::Instant::now() + std::time::Duration::from_millis(1500));
    });
    f.turn("work on the parser");
    std::thread::sleep(std::time::Duration::from_millis(1600));
    *stub.due.lock().unwrap() = Some(due());
    f.front.inbox_step("main");
    assert_eq!(f.tasks().len(), 1, "{}", f.rig.dump());
    assert_eq!(stub.told.lock().unwrap().clone(), ["back i1,i2"]);
}
