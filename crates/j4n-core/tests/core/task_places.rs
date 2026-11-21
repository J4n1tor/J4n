//! A place is asked to get ready, to wrap the doer's launch and to settle, once per task, whatever ends the task.

use crate::scripted::{Beat, Rig, Scripted, end};
use j4n_core::change::Actor;
use j4n_core::place::{Launch, Place, Prepare, Settled, Site};
use j4n_core::table::Problem;
use j4n_core::task::{Ctl, State, TaskSpec, Why};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, PartialEq)]
enum Fault {
    None,
    Refuses,
    PanicsOnPrepare,
    PanicsOnSettle,
}

struct Counting {
    fault: Fault,
    prepared: Mutex<Vec<String>>,
    settled: Mutex<Vec<String>>,
}

impl Counting {
    fn new(fault: Fault) -> Arc<Counting> {
        Arc::new(Counting { fault, prepared: Mutex::default(), settled: Mutex::default() })
    }
}

struct Handle(Arc<Counting>);

impl Place for Handle {
    fn id(&self) -> &str {
        "boxed"
    }

    fn prepare(&self, cx: &Prepare) -> Result<Site, Problem> {
        match self.0.fault {
            Fault::Refuses => return Err(Problem::refused("boxed", "no sandbox on this machine")),
            Fault::PanicsOnPrepare => panic!("the place broke"),
            _ => {}
        }
        self.0.prepared.lock().unwrap().push(cx.task.to_string());
        Ok(Site { notes: vec!["running behind walls".into()], ..Site::new(cx.task, cx.root) })
    }

    fn wrap(&self, _site: &Site, mut launch: Launch) -> Launch {
        launch.argv.insert(0, "sandbox".into());
        launch.env.insert("INSIDE".into(), "yes".into());
        launch
    }

    fn settle(&self, site: &Site) -> Settled {
        self.0.settled.lock().unwrap().push(site.task.clone());
        if self.0.fault == Fault::PanicsOnSettle {
            panic!("the place broke on the way out");
        }
        Settled { created: vec!["made".into()], notes: vec!["left a lamp on".into()], ..Settled::default() }
    }
}

fn in_box(rig: &Rig, doer: &str) -> String {
    let mut spec = TaskSpec::new("go", doer);
    spec.place = "boxed".into();
    rig.tasks.spawn(spec, Actor::Operator).unwrap()
}

fn rig_with(place: &Arc<Counting>, doers: Vec<Arc<Scripted>>) -> Rig {
    let handle = Arc::new(Handle(place.clone()));
    Rig::tuned(doers, move |c| c.places.push(handle))
}

#[test]
fn the_doer_launches_through_its_place_and_what_the_place_found_is_said() {
    let place = Counting::new(Fault::None);
    let doer = Scripted::new("d", vec![vec![end("ok")]]);
    let rig = rig_with(&place, vec![doer.clone()]);
    let id = in_box(&rig, "d");
    let row = rig.finished(&id);
    assert_eq!(row.state, State::Done { ok: true });
    let launch = &doer.launches()[0];
    assert_eq!(launch.argv, ["sandbox", "prog"]);
    assert_eq!(launch.env.get("INSIDE").map(String::as_str), Some("yes"));
    assert_eq!(row.changed.created, ["made"]);
    let notices: Vec<String> = rig.of_kind("run.notice").iter().map(|e| e.data["text"].as_str().unwrap().to_string()).collect();
    assert_eq!(notices, ["running behind walls", "left a lamp on"], "what the place says is said once, going in and coming out");
    assert_eq!(*place.prepared.lock().unwrap(), std::slice::from_ref(&id));
    assert_eq!(*place.settled.lock().unwrap(), [id]);
}

#[test]
fn a_place_is_settled_once_whatever_ends_the_task() {
    let place = Counting::new(Fault::None);
    let rig = rig_with(
        &place,
        vec![
            Scripted::new("done", vec![vec![end("x")]]),
            Scripted::new("stuck", vec![vec![Beat::Forever]]),
            Scripted::new("fails", vec![vec![Beat::Fail("no".into())]]),
            Scripted::new("vanishes", vec![vec![Beat::Vanish]]),
            Scripted::broken("unstartable", "cannot start"),
            Scripted::panicking("panics"),
        ],
    );
    let ids: Vec<String> = ["done", "fails", "vanishes", "unstartable", "panics"].iter().map(|d| in_box(&rig, d)).collect();
    let stuck = in_box(&rig, "stuck");
    rig.until(&stuck, "started", |r| r.state == State::Running);
    rig.tasks.ctl(&stuck, Ctl::Kill, Actor::Operator).unwrap();
    for id in ids.iter().chain([&stuck]) {
        rig.finished(id);
    }
    let mut settled = place.settled.lock().unwrap().clone();
    settled.sort();
    let mut all: Vec<String> = ids.into_iter().chain([stuck]).collect();
    all.sort();
    assert_eq!(settled, all, "each task's site is let go of exactly once");
    assert_eq!(place.prepared.lock().unwrap().len(), 6);
    let panicked = rig.tasks.book().rows().into_iter().find(|r| r.spec.doer == "panics").unwrap();
    assert!(matches!(&panicked.state, State::Stopped { why: Why::Failed { message } } if message.contains("failed while starting")), "{:?}", panicked.state);
}

#[test]
fn a_place_that_is_not_ready_or_breaks_stops_the_task_saying_so() {
    let refusing = Counting::new(Fault::Refuses);
    let rig = rig_with(&refusing, vec![Scripted::new("d", vec![vec![end("x")]])]);
    let id = in_box(&rig, "d");
    let row = rig.finished(&id);
    assert!(matches!(&row.state, State::Stopped { why: Why::Failed { message } } if message == "the place is not ready: no sandbox on this machine"), "{:?}", row.state);
    assert!(refusing.settled.lock().unwrap().is_empty(), "a site that was never made is not settled");
    let broken = Counting::new(Fault::PanicsOnPrepare);
    let rig = rig_with(&broken, vec![Scripted::new("d", vec![vec![end("x")]])]);
    let id = in_box(&rig, "d");
    assert!(matches!(&rig.finished(&id).state, State::Stopped { why: Why::Failed { message } } if message.contains("failed while getting ready")));
    let other = rig.spawn("go", "d");
    assert_eq!(rig.finished(&other).state, State::Done { ok: true }, "the loop is still there");
}

#[test]
fn a_place_that_breaks_on_the_way_out_does_not_keep_the_task_from_ending() {
    let place = Counting::new(Fault::PanicsOnSettle);
    let rig = rig_with(&place, vec![Scripted::new("d", vec![vec![end("fine")]])]);
    let id = in_box(&rig, "d");
    let row = rig.finished(&id);
    assert_eq!((row.state, row.answer.as_deref()), (State::Done { ok: true }, Some("fine")));
    assert!(row.changed.is_empty(), "nothing is known of what it changed");
    let other = rig.spawn("go", "d");
    assert_eq!(rig.finished(&other).state, State::Done { ok: true }, "the loop is still there");
}

// ---- a doer that has no wall of its own is put behind the box when there is one ----

struct Named(&'static str, Arc<Counting>);

impl Place for Named {
    fn id(&self) -> &str {
        self.0
    }

    fn prepare(&self, cx: &Prepare) -> Result<Site, Problem> {
        self.1.prepared.lock().unwrap().push(format!("{}:{}", self.0, cx.task));
        Ok(Site { notes: vec!["behind a wall".into()], ..Site::new(cx.task, cx.root) })
    }

    fn wrap(&self, _site: &Site, launch: Launch) -> Launch {
        launch
    }

    fn settle(&self, _site: &Site) -> Settled {
        Settled::default()
    }
}

fn walled_rig(doers: Vec<Arc<Scripted>>, box_described: bool) -> (Rig, Arc<Counting>) {
    let seen = Counting::new(Fault::None);
    let places: Vec<Arc<dyn Place>> = if box_described { vec![Arc::new(Named("box", seen.clone())), Arc::new(Named("worktree-box", seen.clone()))] } else { Vec::new() };
    (Rig::tuned(doers, move |c| c.places.extend(places)), seen)
}

#[test]
fn a_walled_doer_in_here_runs_in_the_box_and_one_in_a_worktree_in_its_box_twin_and_the_row_says_so() {
    let walled = Scripted::with_caps("w", j4n_core::doer::Caps { walled: true, ..Default::default() }, vec![vec![end("ok")]; 2]);
    let plain = Scripted::new("p", vec![vec![end("ok")]]);
    let (rig, seen) = walled_rig(vec![walled, plain], true);
    let id = rig.spawn("go", "w");
    assert_eq!(rig.finished(&id).state, State::Done { ok: true });
    assert_eq!(*seen.prepared.lock().unwrap(), [format!("box:{id}")]);
    assert_eq!(rig.of_kind("task.queued")[0].data["spec"]["place"], "box", "{}", rig.dump());
    let mut spec = TaskSpec::new("go", "w");
    spec.place = "worktree".into();
    let id2 = rig.tasks.spawn(spec, Actor::Operator).unwrap();
    rig.finished(&id2);
    assert_eq!(seen.prepared.lock().unwrap().last().cloned(), Some(format!("worktree-box:{id2}")));
    // a doer that is not walled stays where it was put
    let id3 = rig.spawn("go", "p");
    rig.finished(&id3);
    assert!(!seen.prepared.lock().unwrap().iter().any(|p| p.ends_with(&format!(":{id3}"))), "{:?}", seen.prepared.lock().unwrap());
}

#[test]
fn where_no_box_is_described_a_walled_doer_runs_as_it_was_and_a_place_it_was_given_is_kept() {
    let walled = Scripted::with_caps("w", j4n_core::doer::Caps { walled: true, ..Default::default() }, vec![vec![end("ok")]]);
    let (rig, _) = walled_rig(vec![walled], false);
    let id = rig.spawn("go", "w");
    assert_eq!(rig.finished(&id).state, State::Done { ok: true });
    assert_eq!(rig.of_kind("task.queued")[0].data["spec"]["place"], "here");
    let place = Counting::new(Fault::None);
    let other = Scripted::with_caps("w", j4n_core::doer::Caps { walled: true, ..Default::default() }, vec![vec![end("ok")]]);
    let rig = rig_with(&place, vec![other]);
    let id = in_box(&rig, "w");
    assert_eq!(rig.finished(&id).state, State::Done { ok: true });
    assert_eq!(*place.prepared.lock().unwrap(), std::slice::from_ref(&id), "a place the task was given is not swapped");
}
