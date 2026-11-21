//! The chats and the plan keeper: a line routed to a chat that cannot take it is said not to be sent, in the
//! orchestrator where it was typed.

use crate::front_cases::fx_deciding;
use crate::scripted::{Beat, Scripted, choice, end};
use j4n_core::front::{Cfg, Said};
use j4n_core::task::State;

fn on(c: &mut Cfg) {
    c.chats.on = true;
}

#[test]
fn a_line_routed_to_a_chat_whose_task_waits_for_an_answer_is_said_not_sent_in_the_orchestrator() {
    let doer = Scripted::new("claude", vec![vec![Beat::Session("s".into()), Beat::Ask(choice("q", "keep the tabs?", &[("yes", true), ("no", false)])), Beat::Hold, end("done")]]);
    let f = fx_deciding(vec![doer], Some("absent"), on);
    let Said::Started { task } = f.say("fix trailing commas in parser.rs") else { panic!("{}", f.rig.dump()) };
    f.rig.until(&task, "waiting", |r| matches!(r.state, State::Waiting { .. }));
    let before = f.tasks().len();
    // the line names c1's file, so it is routed there; c1's task waits, and the chat refuses it
    let said = f.say("also update the docs for parser.rs");
    assert!(matches!(said, Said::Refused(_)), "{said:?}\n{}", f.rig.dump());
    assert_eq!(f.tasks().len(), before, "no turn was started for it");
    let routed = f.rig.of_kind("chat.routed").pop().expect("the route is in the Log").data;
    assert_eq!(routed["to"], "c1");
    assert!(routed["refused"].as_str().is_some_and(|w| w.contains("keep the tabs?")), "the Log says it was not taken: {routed}");
    let notes = f.notes();
    assert!(notes.iter().any(|n| n.starts_with("not sent to c1:") && n.contains("keep the tabs?")), "the row says it was not sent: {notes:?}");
}
