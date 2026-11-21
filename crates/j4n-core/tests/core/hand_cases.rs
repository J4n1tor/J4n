//! Pick by hand: a point in `pick.hand` gets the `human` step, and a walk that comes to it shows one pick row: the suggested option first, Enter takes it, a digit
//! picks, Esc takes the point's fallback. Nothing waits in a run with no one to ask, and a point not in the list is walked as it was.

use crate::scripted::Rig;
use crate::who_cases::{script, who};
use j4n_core::ask::Opt;
use j4n_core::front::hosts::NoHosts;
use j4n_core::front::who::{Standing, Who};
use j4n_proto::{AskView, Reply};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn rig() -> (Arc<Who>, Rig) {
    let (w, rig) = who(&[script("alpha", "x")], &[], &Standing::new(), Arc::new(NoHosts));
    (Arc::new(w), rig)
}

fn options() -> Vec<Opt> {
    vec![Opt { id: "c1".into(), label: "c1: the parser".into(), recommended: false }, Opt { id: "new".into(), label: "a new chat".into(), recommended: true }]
}

/// The pick as the walk returned it: the id, who picked, why.
type Picked = Option<(String, String, String)>;

/// Ask the point `chats` on a thread of its own and wait for its pick row.
fn asking(w: &Arc<Who>) -> (std::thread::JoinHandle<Picked>, AskView) {
    let (w2, q) = (w.clone(), w.question("chats", None, "check the parser", options()));
    let job = std::thread::spawn(move || w2.pick("chats", None, &q).map(|(id, by, why, _)| (id, by, why)));
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(v) = w.hands().views().into_iter().next() {
            return (job, v);
        }
        assert!(Instant::now() < end, "no pick row was shown");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_point_not_picked_by_hand_is_walked_as_it_was_and_goes_to_its_fallback_with_no_decider() {
    let (w, _rig) = rig();
    let q = w.question("chats", None, "check the parser", options());
    let (id, by, _, _) = w.pick("chats", None, &q).expect("the fallback answers");
    assert_eq!(id, "new");
    assert!(by.starts_with("fallback"), "{by}");
    assert!(w.hands().views().is_empty());
}

#[test]
fn by_hand_one_pick_row_shows_the_suggested_option_first_and_enter_takes_it() {
    let (w, rig) = rig();
    w.hands().set(&["chats".to_string()]);
    let (job, row) = asking(&w);
    assert_eq!(row.options.iter().map(|o| (o.id.as_str(), o.recommended)).collect::<Vec<_>>(), [("new", true), ("c1", false)], "the suggested one first: {row:?}");
    assert_eq!(row.question, "chats: you pick");
    // Enter on the row takes the suggestion
    w.hands().answer(&row.ask, &Reply::Yes).expect("a pick row").unwrap();
    assert_eq!(job.join().unwrap().unwrap(), ("new".into(), "you".into(), "you picked it".into()));
    assert!(w.hands().views().is_empty() && !rig.of_kind("ask.picked").is_empty());
}

#[test]
fn a_digit_picks_another_option_and_esc_takes_the_points_fallback() {
    let (w, _rig) = rig();
    w.hands().set(&["chats".to_string()]);
    let (job, row) = asking(&w);
    assert!(w.hands().answer(&row.ask, &Reply::Pick { id: "nobody".into() }).unwrap().is_err(), "a pick that is not offered is refused and the row waits on");
    w.hands().answer(&row.ask, &Reply::Pick { id: "c1".into() }).unwrap().unwrap();
    assert_eq!(job.join().unwrap().unwrap().0, "c1");
    let (job, row) = asking(&w);
    w.hands().answer(&row.ask, &Reply::No).unwrap().unwrap();
    let (id, by, _) = job.join().unwrap().unwrap();
    assert_eq!(id, "new");
    assert!(by.starts_with("fallback"), "Esc is the fallback: {by}");
}
