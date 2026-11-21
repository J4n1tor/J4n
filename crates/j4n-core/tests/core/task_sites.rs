//! A place that keeps what a task did (`Settled::kept`), a later task that works where it was done (`@t1`), and the work
//! brought to the project's folder (`Tasks::take`).

use crate::scripted::{Rig, Scripted, end};
use j4n_core::change::Actor;
use j4n_core::place::{Kept, Launch, Place, Prepare, Settled, Site, Take, Taken, Work};
use j4n_core::table::Problem;
use j4n_core::task::{State, TaskSpec};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// A place with a folder of its own per task, which it keeps while the work in it is not applied.
struct Lane {
    applies: bool,
    dir: PathBuf,
    holds: Mutex<Vec<String>>,
    settled: Mutex<Vec<String>>,
    taken: Mutex<Vec<(String, Take)>>,
    blocked: bool,
}

impl Lane {
    fn new(dir: PathBuf, blocked: bool) -> Arc<Lane> {
        Arc::new(Lane { applies: false, dir, holds: Mutex::default(), settled: Mutex::default(), taken: Mutex::default(), blocked })
    }

    /// A lane that lays the work into the folder as the task ends.
    fn applying(dir: PathBuf) -> Arc<Lane> {
        Arc::new(Lane { applies: true, dir, holds: Mutex::default(), settled: Mutex::default(), taken: Mutex::default(), blocked: false })
    }

    fn work() -> Work {
        Work { files: vec!["src/a.rs".into()], ignored: vec!["ledger/x.json".into()], added: 3, removed: 1 }
    }
}

struct Handle(Arc<Lane>);

impl Place for Handle {
    fn id(&self) -> &str {
        "lane"
    }

    fn prepare(&self, cx: &Prepare) -> Result<Site, Problem> {
        Ok(Site::new(cx.task, &self.0.dir.join(cx.task)))
    }

    fn wrap(&self, _site: &Site, mut launch: Launch) -> Launch {
        launch.argv.insert(0, "walled".into());
        launch
    }

    fn settle(&self, site: &Site) -> Settled {
        self.0.settled.lock().unwrap().push(site.task.clone());
        if self.0.applies {
            return Settled { applied: Some(Lane::work()), ..Settled::default() };
        }
        self.0.holds.lock().unwrap().push(site.task.clone());
        Settled { kept: Some(Kept { dir: site.dir.display().to_string(), work: Lane::work(), why: "not applied".into() }), ..Settled::default() }
    }

    fn site_of(&self, task: &str) -> Option<Site> {
        self.0.holds.lock().unwrap().iter().any(|t| t == task).then(|| Site { notes: vec!["the lane is cold".into()], ..Site::new(task, &self.0.dir.join(task)) })
    }

    fn take(&self, task: &str, how: Take) -> Result<Taken, Problem> {
        self.0.taken.lock().unwrap().push((task.to_string(), how));
        if self.0.blocked {
            return Ok(Taken::Blocked { why: "src/a.rs has changes of yours".into(), work: Lane::work() });
        }
        if how == Take::Apply {
            self.0.holds.lock().unwrap().retain(|t| t != task);
        }
        Ok(Taken::Clean(Lane::work(), Vec::new()))
    }
}

fn rig(lane: &Arc<Lane>, doers: Vec<Arc<Scripted>>) -> Rig {
    let handle = Arc::new(Handle(lane.clone()));
    Rig::based(doers, "", move |c| c.places.push(handle))
}

fn in_lane(rig: &Rig, doer: &str, place: &str) -> String {
    let mut spec = TaskSpec::new("go", doer);
    spec.place = place.into();
    rig.tasks.spawn(spec, Actor::Operator).unwrap()
}

#[test]
fn work_a_place_keeps_is_said_in_the_log_and_on_the_row() {
    let lane = Lane::new(PathBuf::from("/lanes"), false);
    let rig = rig(&lane, vec![Scripted::new("d", vec![vec![end("ok")]])]);
    let id = in_lane(&rig, "d", "lane");
    let row = rig.finished(&id);
    let kept = row.kept.clone().expect("kept");
    assert_eq!((kept.dir.as_str(), kept.why.as_str(), kept.work.files.clone()), (format!("/lanes/{id}").as_str(), "not applied", vec!["src/a.rs".to_string()]));
    assert!(row.applied.is_none());
    let events = rig.of_kind("task.kept");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data["work"]["ignored"][0], "ledger/x.json", "the file git ignores is named");
    let at = |kind: &str| rig.events().iter().position(|e| e.kind == kind).unwrap();
    assert!(at("task.kept") < at("task.settled"), "said before the task is over, so what answers its end finds it");
}

#[test]
fn what_a_place_kept_can_be_looked_at_then_applied_and_is_then_no_longer_kept() {
    let lane = Lane::new(PathBuf::from("/lanes"), false);
    let rig = rig(&lane, vec![Scripted::new("d", vec![vec![end("ok")]])]);
    let id = in_lane(&rig, "d", "lane");
    rig.finished(&id);
    let looked = rig.tasks.take(&id, Take::Look).unwrap();
    assert_eq!(looked, Taken::Clean(Lane::work(), Vec::new()));
    assert!(rig.of_kind("task.applied").is_empty(), "looking changes nothing and writes nothing");
    assert!(rig.tasks.book().get(&id).unwrap().kept.is_some());
    let done = rig.tasks.take(&id, Take::Apply).unwrap();
    assert_eq!(done, Taken::Clean(Lane::work(), Vec::new()));
    rig.log.sync();
    let row = rig.tasks.book().get(&id).unwrap();
    assert!((row.kept.is_none(), row.applied.clone()) == (true, Some(Lane::work())), "{row:?}");
    assert_eq!(rig.of_kind("task.applied")[0].data["auto"], serde_json::Value::Null, "an applied-by-hand is not the place's own doing");
    assert_eq!(*lane.taken.lock().unwrap(), [(id.clone(), Take::Look), (id.clone(), Take::Apply)]);
    let again = rig.tasks.take(&id, Take::Apply).unwrap_err();
    assert!(again.msg.contains("left no work"), "{}", again.msg);
}

#[test]
fn work_that_does_not_apply_stays_kept_and_nothing_is_written() {
    let lane = Lane::new(PathBuf::from("/lanes"), true);
    let rig = rig(&lane, vec![Scripted::new("d", vec![vec![end("ok")]])]);
    let id = in_lane(&rig, "d", "lane");
    rig.finished(&id);
    let r = rig.tasks.take(&id, Take::Apply).unwrap();
    assert!(matches!(&r, Taken::Blocked { why, .. } if why.contains("changes of yours")), "{r:?}");
    rig.log.sync();
    assert!(rig.of_kind("task.applied").is_empty());
    assert!(rig.tasks.book().get(&id).unwrap().kept.is_some());
}

#[test]
fn a_task_that_is_still_going_or_left_nothing_cannot_be_taken_from() {
    let lane = Lane::new(PathBuf::from("/lanes"), false);
    let rig = rig(&lane, vec![Scripted::new("d", vec![vec![end("ok")]]), Scripted::new("w", vec![vec![crate::scripted::Beat::Forever]])]);
    let plain = rig.spawn("go", "d");
    rig.finished(&plain);
    let e = rig.tasks.take(&plain, Take::Apply).unwrap_err();
    assert!(e.msg.contains("left no work"), "{}", e.msg);
    let going = in_lane(&rig, "w", "lane");
    rig.until(&going, "running", |r| r.state == State::Running);
    let e = rig.tasks.take(&going, Take::Look).unwrap_err();
    assert!(e.msg.contains("still going"), "{}", e.msg);
    assert!(rig.tasks.take("t99", Take::Look).is_err());
}

#[test]
fn a_task_can_work_where_another_one_did_behind_the_same_walls_and_owns_none_of_it() {
    let lane = Lane::new(PathBuf::from("/lanes"), false);
    let checker = Scripted::new("check", vec![vec![end("green")]]);
    let rig = rig(&lane, vec![Scripted::new("d", vec![vec![end("ok")]]), checker.clone()]);
    let first = in_lane(&rig, "d", "lane");
    rig.finished(&first);
    let second = in_lane(&rig, "check", &format!("@{first}"));
    let row = rig.finished(&second);
    assert_eq!(row.state, State::Done { ok: true }, "{row:?}");
    let launch = &checker.launches()[0];
    assert_eq!((launch.argv.clone(), launch.cwd.clone()), (vec!["walled".to_string(), "prog".to_string()], PathBuf::from(format!("/lanes/{first}"))));
    assert_eq!(*lane.settled.lock().unwrap(), std::slice::from_ref(&first), "the place was settled for the task that made it, and for no other");
    assert!(row.kept.is_none(), "and the second task keeps nothing");
    let said: Vec<String> = rig.of_kind("run.notice").iter().filter(|e| e.task.as_deref() == Some(second.as_str())).map(|e| e.data["text"].as_str().unwrap().to_string()).collect();
    assert!(said.is_empty(), "what the place said going in was said for the task that made it: {said:?}");
    assert!(rig.tasks.book().get(&first).unwrap().kept.is_some(), "the work is still where it was");
}

#[test]
fn a_place_that_is_gone_or_never_was_stops_the_task_that_asked_for_it() {
    let lane = Lane::new(PathBuf::from("/lanes"), false);
    let rig = rig(&lane, vec![Scripted::new("d", vec![vec![end("ok")]])]);
    let first = in_lane(&rig, "d", "lane");
    rig.finished(&first);
    rig.tasks.take(&first, Take::Apply).unwrap();
    let why = |place: &str| match rig.finished(&in_lane(&rig, "d", place)).state {
        State::Stopped { why: j4n_core::task::Why::Failed { message } } => message,
        other => panic!("{other:?}"),
    };
    assert!(why(&format!("@{first}")).contains("no longer has its place"));
    assert!(why("@t99").contains("no task t99"));
    assert!(why("nowhere").contains("no place called `nowhere`"));
    let here = rig.spawn("go", "d");
    rig.finished(&here);
    assert_eq!(rig.finished(&in_lane(&rig, "d", &format!("@{here}"))).state, State::Done { ok: true }, "the project's own folder is a place to go back to");
}

#[test]
fn a_chain_of_at_places_leads_to_the_place_that_has_the_site_and_not_forever() {
    let lane = Lane::new(PathBuf::from("/lanes"), false);
    let rig = rig(&lane, vec![Scripted::new("d", vec![vec![end("ok")]])]);
    let a = in_lane(&rig, "d", "lane");
    rig.finished(&a);
    let b = in_lane(&rig, "d", &format!("@{a}"));
    rig.finished(&b);
    let c = in_lane(&rig, "d", &format!("@{b}"));
    assert_eq!(rig.finished(&c).state, State::Done { ok: true }, "a task that worked where another did can be the one a third goes to");
}

/// Everything the loop and the rules were to do about what was written has been done, and is in the fold.
fn settle(rig: &Rig) {
    rig.tasks.sync().unwrap();
    rig.log.sync();
}


#[test]
fn a_rule_can_start_a_task_where_the_work_was_done_and_read_whether_it_is_kept() {
    let lane = Lane::new(PathBuf::from("/lanes"), false);
    let checker = Scripted::new("check", vec![vec![end("green")]]);
    let rules = "[[rule]]\nid = \"look\"\non = \"task.settled\"\nwhen = \"fold.tasks[task].kept and not fold.tasks[task].applied and fold.tasks[task].doer == 'd'\"\nthen = \"start\"\ndoer = \"check\"\nplace = \"@same\"\ntext = \"check {task}\"\n";
    let handle = Arc::new(Handle(lane.clone()));
    let rig = Rig::based(vec![Scripted::new("d", vec![vec![end("ok")]]), checker.clone()], rules, move |c| c.places.push(handle));
    let first = in_lane(&rig, "d", "lane");
    rig.finished(&first);
    settle(&rig);
    let rows = rig.tasks.book().rows();
    let second = rows.iter().find(|r| r.spec.doer == "check").unwrap_or_else(|| panic!("{}", rig.dump()));
    assert_eq!(second.spec.place, format!("@{first}"));
    let done = rig.finished(&second.id);
    assert_eq!(done.state, State::Done { ok: true });
    assert_eq!(checker.launches()[0].argv, ["walled", "prog"], "behind the same walls, in the same folder");
    assert_eq!(rig.tasks.book().rows().iter().filter(|r| r.spec.doer == "check").count(), 1, "and the check itself, settling, starts no check: it kept nothing");
    // a task that was applied is not the rule's
    rig.tasks.take(&first, Take::Apply).unwrap();
    let again = in_lane(&rig, "d", "lane");
    rig.finished(&again);
    settle(&rig);
    assert_eq!(rig.tasks.book().rows().iter().filter(|r| r.spec.doer == "check").count(), 2); 
}

#[test]
fn a_rule_may_name_a_place_only_where_it_starts_a_task_of_its_own() {
    use j4n_core::rule::RuleRow;
    let row = |extra: &str| -> Result<(), Vec<String>> {
        let text = format!("id = \"r\"\non = \"task.settled\"\n{extra}");
        let row: RuleRow = toml::from_str(&text).unwrap();
        row.compile().map(|_| ())
    };
    assert!(row("then = \"start\"\ntext = \"x\"\nplace = \"@same\"").is_ok());
    assert!(row("then = \"start\"\ntext = \"x\"\nplace = \"worktree\"").is_ok());
    let e = row("then = \"start\"\nfollow = true\ntext = \"x\"\nplace = \"worktree\"").unwrap_err();
    assert!(e.iter().any(|m| m.contains("`place` is for `start` of a task that is not carried on")), "{e:?}");
    let e = row("then = \"notice\"\ntext = \"x\"\nplace = \"worktree\"").unwrap_err();
    assert!(e.iter().any(|m| m.contains("`place` is for")), "{e:?}");
    for bad in ["", "two words", "tab\\there"] {
        let e = row(&format!("then = \"start\"\ntext = \"x\"\nplace = \"{bad}\"")).unwrap_err();
        assert!(e.iter().any(|m| m.contains("is a place's name")), "{bad:?}: {e:?}");
    }
    let long = "p".repeat(65);
    assert!(row(&format!("then = \"start\"\ntext = \"x\"\nplace = \"{long}\"")).is_err());
    assert!(row(&format!("then = \"start\"\ntext = \"x\"\nplace = \"{}\"", "p".repeat(64))).is_ok());
}

#[test]
fn a_chain_of_at_places_that_is_longer_than_it_should_be_is_stopped() {
    let lane = Lane::new(PathBuf::from("/lanes"), false);
    let rig = rig(&lane, vec![Scripted::new("d", vec![vec![end("ok")]])]);
    let mut at = in_lane(&rig, "d", "lane");
    rig.finished(&at);
    // nine tasks that went where the one before them did (eight hops) are still found; the tenth is one too many
    for _ in 0..9 {
        at = in_lane(&rig, "d", &format!("@{at}"));
        assert_eq!(rig.finished(&at).state, State::Done { ok: true });
    }
    let last = in_lane(&rig, "d", &format!("@{at}"));
    match rig.finished(&last).state {
        State::Stopped { why: j4n_core::task::Why::Failed { message } } => assert!(message.contains("leads through too many tasks"), "{message}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_rule_that_names_a_place_starts_its_task_there_and_not_where_the_event_was() {
    let lane = Lane::new(PathBuf::from("/lanes"), false);
    let checker = Scripted::new("check", vec![vec![end("green")]]);
    let rules = "[[rule]]\nid = \"over\"\non = \"task.settled\"\nwhen = \"fold.tasks[task].doer == 'd'\"\nthen = \"start\"\ndoer = \"check\"\nplace = \"lane\"\ntext = \"check {task}\"\n";
    let handle = Arc::new(Handle(lane.clone()));
    let rig = Rig::based(vec![Scripted::new("d", vec![vec![end("ok")]]), checker.clone()], rules, move |c| c.places.push(handle));
    let first = rig.spawn("go", "d");
    rig.finished(&first);
    settle(&rig);
    let rows = rig.tasks.book().rows();
    let second = rows.iter().find(|r| r.spec.doer == "check").expect("the rule started a task");
    assert_eq!((rows[0].spec.place.as_str(), second.spec.place.as_str()), ("here", "lane"));
    assert_eq!(rig.finished(&second.id).state, State::Done { ok: true });
    assert_eq!(checker.launches()[0].argv[0], "walled", "and it ran behind the lane");
}

#[test]
fn a_rule_reads_whether_the_work_was_applied_as_the_task_ended() {
    let lane = Lane::applying(PathBuf::from("/lanes"));
    let checker = Scripted::new("check", vec![vec![end("green")]]);
    let rules = "[[rule]]\nid = \"after\"\non = \"task.settled\"\nwhen = \"fold.tasks[task].applied and not fold.tasks[task].kept\"\nthen = \"start\"\ndoer = \"check\"\nplace = \"here\"\ntext = \"check {task}\"\n";
    let handle = Arc::new(Handle(lane.clone()));
    let rig = Rig::based(vec![Scripted::new("d", vec![vec![end("ok")]]), checker], rules, move |c| c.places.push(handle));
    let first = in_lane(&rig, "d", "lane");
    let row = rig.finished(&first);
    assert!(row.applied.is_some() && row.kept.is_none());
    settle(&rig);
    assert_eq!(rig.tasks.book().rows().iter().filter(|r| r.spec.doer == "check").count(), 1);
    assert_eq!(rig.of_kind("task.applied")[0].data["auto"], true, "and it says the place did it as the task ended");
}
