//! Through the real front and scripted doers: the item states
//! move by events, an `unchecked` claim blocks `checked`, a piece works in a held worktree (a fake place that lives under the test's
//! own temp folder), is checked and reviewed on another doer with a clean context, and is merged one at a time in plan order with the
//! checks between; a piece that does not lay goes back to its item, the fourth send-back is a block, a branch past its cap is killed
//! with its report kept, and the plan's cost is the sum of its items' rows.

use crate::front_cases::{Fx, chatty, fx_deciding, fx_placed};
use crate::scripted::{Beat, Scripted, end};
use crate::world::Scratch;
use j4n_core::branch::Switch;
use j4n_core::doer::Caps;
use j4n_core::front::{Cfg, Said};
use j4n_core::place::{Kept, Launch, Place, Prepare, Settled, Site, Take, Taken, Work};
use j4n_core::table::Problem;
use j4n_core::task::Mode;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

pub const A: [&str; 3] = ["src/a/1.rs", "src/a/2.rs", "src/a/3.rs"];
pub const B: [&str; 3] = ["src/b/1.rs", "src/b/2.rs", "src/b/3.rs"];
pub const C: [&str; 3] = ["src/c/1.rs", "src/c/2.rs", "src/c/3.rs"];

pub fn item(id: &str, place: &str, reads: &[&str], writes: &[&str], deps: &[&str]) -> Value {
    json!({ "id": id, "goal": format!("do {id}"), "reads": reads, "writes": writes, "depends": deps, "done": format!("{id} works"), "where": place })
}

pub fn planned(items: Vec<Value>) -> String {
    json!({ "approach": "Look, change, check.", "items": items }).to_string()
}

pub fn wide(c: &mut Cfg) {
    c.doer = Some("claude".into());
    c.lanes = Switch::Wide;
}

/// A review that names nothing, in the report's shape: an answer in words goes back to the writer as its concern.
pub const CLEAN: &str = r#"{"answer": "read the change", "sure": "mid", "found": [], "ran": [], "open": []}"#;

pub fn caps() -> Caps {
    Caps { live: true, resume: true, brief: true, ..Caps::default() }
}

pub fn doer(id: &str, answers: &[&str]) -> Arc<Scripted> {
    Scripted::with_caps(id, caps(), answers.iter().map(|a| vec![Beat::Session(format!("{id}-s")), Beat::Say(a.to_string()), end(a)]).collect())
}

/// A held worktree that is no git: the folder it says it holds is under its own temp dir (removed with it), and what it lays is
/// scripted by task.
pub struct Held {
    dir: Scratch,
    writers: Mutex<BTreeSet<String>>,
    pub refuse: Mutex<BTreeMap<String, String>>,
    pub refuse_all: Mutex<Option<String>>,
    calls: Mutex<Vec<(String, Take)>>,
    /// More files the held work says it holds, beside `src/a/1.rs`.
    pub also: Mutex<Vec<String>>,
    /// The files of the held work its task made (none were there before it).
    pub created: Mutex<Vec<String>>,
    /// The folder a lay-in writes into, as a real one does: `chess/<task>.py` for each task laid in. None: it writes nothing.
    pub into: Mutex<Option<std::path::PathBuf>>,
    /// J4n's home: a lay-in into `into` is written under a checkpoint there, as a real one is, so that a rewind takes it out again.
    pub home: Mutex<Option<std::path::PathBuf>>,
}

impl Held {
    pub fn new() -> Arc<Held> {
        Arc::new(Held { dir: Scratch::new("held"), writers: Mutex::default(), refuse: Mutex::default(), refuse_all: Mutex::default(), calls: Mutex::default(), also: Mutex::default(), created: Mutex::default(), into: Mutex::default(), home: Mutex::default() })
    }

    fn work(&self) -> Work {
        let files = std::iter::once("src/a/1.rs".to_string()).chain(self.also.lock().unwrap().iter().cloned()).collect();
        Work { files, ignored: vec![], added: 3, removed: 1 }
    }

    /// The place, to give to `fx_placed`.
    pub fn place(self: &Arc<Held>) -> Arc<dyn Place> {
        Arc::new(Handle(self.clone()))
    }

    /// The folder the held place gives `task`.
    pub fn site(&self, task: &str) -> std::path::PathBuf {
        self.dir.join(task)
    }

    pub fn applied(&self) -> Vec<String> {
        self.calls.lock().unwrap().iter().filter(|(_, h)| *h == Take::Apply).map(|(t, _)| t.clone()).collect()
    }
}

struct Handle(Arc<Held>);

impl Place for Handle {
    fn id(&self) -> &str {
        "worktree"
    }

    fn prepare(&self, cx: &Prepare) -> Result<Site, Problem> {
        let dir = self.0.dir.join(cx.task);
        std::fs::create_dir_all(&dir).map_err(|e| Problem::refused("worktree", e.to_string()))?;
        if cx.spec.tune.mode == Mode::Write {
            self.0.writers.lock().unwrap().insert(cx.task.to_string());
        }
        Ok(Site::new(cx.task, &dir))
    }

    fn wrap(&self, _site: &Site, launch: Launch) -> Launch {
        launch
    }

    fn settle(&self, site: &Site) -> Settled {
        if !self.0.writers.lock().unwrap().contains(&site.task) {
            return Settled::default();
        }
        Settled { kept: Some(Kept { dir: site.dir.display().to_string(), work: self.0.work(), why: "waiting to be applied".into() }), created: self.0.created.lock().unwrap().clone(), ..Settled::default() }
    }

    fn site_of(&self, task: &str) -> Option<Site> {
        let dir = self.0.dir.join(task);
        dir.is_dir().then(|| Site::new(task, &dir))
    }

    fn share(&self, task: &str, rel: &str, from: &std::path::Path) -> Result<j4n_core::place::Shared, String> {
        let to = self.0.dir.join(task).join(rel);
        let bytes = std::fs::read(from).map_err(|e| e.to_string())?;
        let was = std::fs::read(&to).ok();
        if was.as_deref() == Some(&bytes[..]) {
            return Ok(j4n_core::place::Shared::Same);
        }
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(&to, &bytes).unwrap();
        Ok(if was.is_some() { j4n_core::place::Shared::Changed } else { j4n_core::place::Shared::Placed })
    }

    fn take(&self, task: &str, how: Take) -> Result<Taken, Problem> {
        self.0.calls.lock().unwrap().push((task.to_string(), how));
        let why = self.0.refuse.lock().unwrap().get(task).cloned().or_else(|| self.0.refuse_all.lock().unwrap().clone());
        if let (None, Take::Apply, Some(into)) = (&why, how, self.0.into.lock().unwrap().as_ref()) {
            let home = self.0.home.lock().unwrap().clone();
            let cp = home.map(|h| j4n_core::place::checkpoint::begin(&h, into, task, &j4n_core::place::checkpoint::Cfg::default()).unwrap());
            std::fs::create_dir_all(into.join("chess")).unwrap();
            std::fs::write(into.join(format!("chess/{task}.py")), format!("# laid in by {task}\n")).unwrap();
            if let Some(cp) = cp {
                cp.end().unwrap();
            }
        }
        Ok(match why {
            Some(why) => Taken::Blocked { why, work: self.0.work() },
            None => Taken::Clean(self.0.work(), Vec::new()),
        })
    }
}

pub fn checker(n: usize) -> Arc<Scripted> {
    Scripted::new("check", vec![vec![end("green")]; n])
}

pub struct Rig3 {
    pub f: Fx,
    pub held: Arc<Held>,
}

/// The main lane writes the plan; `pi` is the doer elected for work; `own` reviews; `check` checks.
pub fn rig(items: Vec<Value>, pi: Arc<Scripted>, own: Arc<Scripted>, check: Arc<Scripted>, tail: &[&str]) -> Rig3 {
    rig_cfg(items, pi, own, check, tail, wide)
}

/// [`wide`], and the first trigger of a replan asks the operator instead of starting one: these cases are about the report.
pub fn no_replan(c: &mut Cfg) {
    wide(c);
    c.revisions = 0;
}

/// [`wide`], and a piece the folder refuses at laying in goes back to its item (`plan.lay_refused` `retry`).
pub fn retry(c: &mut Cfg) {
    wide(c);
    c.lay_retry = true;
}

pub fn rig_cfg(items: Vec<Value>, pi: Arc<Scripted>, own: Arc<Scripted>, check: Arc<Scripted>, tail: &[&str], cfg: fn(&mut Cfg)) -> Rig3 {
    rig_on(items, pi, own, check, tail, cfg, None)
}

/// [`rig_cfg`] drawing `screen`, when there is one: the settings it holds are the front's.
pub fn rig_on(items: Vec<Value>, pi: Arc<Scripted>, own: Arc<Scripted>, check: Arc<Scripted>, tail: &[&str], cfg: fn(&mut Cfg), screen: Option<j4n_core::front::screen::Screen>) -> Rig3 {
    let held = Held::new();
    let mut answers = vec!["hello".to_string(), planned(items)];
    answers.extend(tail.iter().map(|s| s.to_string()));
    let refs: Vec<&str> = answers.iter().map(String::as_str).collect();
    let doers = vec![chatty("claude", &refs), pi, own, check];
    let places: Vec<Arc<dyn j4n_core::place::Place>> = vec![Arc::new(Handle(held.clone()))];
    let f = match screen {
        Some(screen) => crate::front_cases::fx_placed_on(doers, Some("pi"), places, screen, cfg),
        None => fx_placed(doers, Some("pi"), places, cfg),
    };
    Rig3 { f, held }
}

impl Rig3 {
    pub fn begin(&self) {
        self.f.turn("hello");
        match self.f.say(":plan do these: 1. look 2. look again 3. look more 4. change 5. change more 6. check 7. check again") {
            Said::Started { task } => {
                self.f.rig.finished(&task);
            }
            other => panic!("{other:?}\n{}", self.f.rig.dump()),
        }
        self.f.front.plan_collect("main");
        self.f.front.plan_step("main");
    }

    /// Every task is over, then the plan takes what came of them and goes on.
    pub fn step(&self) {
        for r in self.f.tasks() {
            self.f.rig.finished(&r.id);
        }
        self.f.front.plan_step("main");
    }

    pub fn until(&self, what: &str, done: impl Fn(&Rig3) -> bool) {
        for _ in 0..24 {
            if done(self) {
                return;
            }
            self.step();
        }
        panic!("never {what}: {:?}\n{}", self.states(), self.f.rig.dump());
    }

    /// The `sent-back` events of the plan's items, in the Log's order.
    pub fn sent_back(&self) -> Vec<j4n_core::log::Ev> {
        self.f.rig.of_kind("plan.item").into_iter().filter(|e| e.data["op"] == "state" && e.data["state"] == "sent-back").collect()
    }

    pub fn states(&self) -> Vec<(String, String)> {
        self.f.keeper.plan("main").map(|p| p.items.into_iter().map(|i| (i.id, i.state)).collect()).unwrap_or_default()
    }

    /// The states an item went through, from the Log.
    pub fn path(&self, id: &str) -> Vec<String> {
        self.f.rig.of_kind("plan.item").into_iter().filter(|e| e.data["id"] == id && e.data["op"] == "state").map(|e| e.data["state"].as_str().unwrap().to_string()).collect()
    }

    pub fn item(&self, id: &str) -> j4n_core::keeper::Item {
        self.f.keeper.plan("main").unwrap().items.into_iter().find(|i| i.id == id).unwrap()
    }
}

pub fn pieces() -> Vec<Value> {
    vec![
        item("p1", "piece", &A, &["src/a/1.rs"], &[]),
        item("p2", "piece", &B, &["src/b/1.rs"], &[]),
        item("p3", "piece", &C, &["src/c/1.rs"], &[]),
        item("end", "inline", &[], &["src/e.rs"], &["p1", "p2", "p3"]),
    ]
}


#[test]
fn an_unchecked_claim_blocks_checked_and_the_item_goes_back_with_the_claim_named() {
    let src = Scratch::new("src");
    src.put("src/a/1.rs", "fn retry() {}\n");
    let missing = json!({ "answer": "it is there", "found": [{ "claim": "the retry loop", "at": "src/gone.rs:3" }], "sure": "mid" }).to_string();
    let good = json!({ "answer": "it is there", "found": [{ "claim": "the retry loop", "at": "src/a/1.rs:1" }], "sure": "high" }).to_string();
    let answers = ["hello".to_string(), planned(vec![item("s", "scout", &A, &[], &[]), item("m", "inline", &[], &["src/m.rs"], &["s"]), item("n", "inline", &[], &["src/n.rs"], &["m"])]), "m done".into(), "n done".into()];
    let refs: Vec<&str> = answers.iter().map(String::as_str).collect();
    let pi = doer("pi", &[&missing, &good]);
    let f = fx_deciding(vec![chatty("claude", &refs), pi.clone()], Some("pi"), |c| {
        wide(c);
        c.root = src.path().to_path_buf();
    });
    let r = Rig3 { f, held: Held::new() };
    r.begin();
    r.step();
    assert_eq!(r.path("s"), ["running", "reported", "sent-back", "running"], "an unchecked claim does not reach `checked`: {}", r.f.rig.dump());
    let s = r.item("s");
    assert_eq!((s.sends, s.tries), (1, 2));
    assert!(pi.starts().len() == 2, "and the item is started again");
    let again = r.f.tasks().into_iter().rev().find(|t| t.spec.doer == "pi").unwrap();
    assert!(again.spec.goal.contains("Sent back (1 of 3)") && again.spec.goal.contains("the retry loop @ src/gone.rs:3"), "the branch is told which claim: {}", again.spec.goal);
    r.until("the scout ended", |r| r.item("s").done());
    assert_eq!(r.path("s"), ["running", "reported", "sent-back", "running", "reported", "checked", "reviewed"], "{}", r.f.rig.dump());
    assert_eq!(r.item("s").state, "reviewed", "a scout has nothing to merge");
}

#[test]
fn a_piece_is_checked_where_it_is_held_reviewed_on_another_doer_with_a_clean_context_and_merged() {
    let r = rig(pieces(), doer("pi", &["did p1", "did p2", "did p3"]), doer("own", &[CLEAN; 3]), checker(6), &["end done"]);
    r.begin();
    let tasks = r.f.tasks();
    let p1 = tasks.iter().find(|t| t.spec.input["item"] == "p1" && t.spec.role.as_deref() == Some("piece")).unwrap();
    assert_eq!((p1.spec.place.as_str(), p1.spec.input["hold"].as_bool()), ("worktree", Some(true)), "a piece works in a held worktree, whatever `work.apply` says");
    assert_eq!(p1.spec.tune.mode, Mode::Write);
    r.until("the plan was done", |r| r.item("end").state == "merged");
    // the first lay-in waits for a check of the folder before it; the folder does not change after, and the check after one piece stands for the next's
    assert_eq!(r.path("p1"), ["running", "reported", "reported", "checked", "checked", "reviewed", "reviewed", "reviewed", "merged"], "p1: {}", r.f.rig.dump());
    for id in ["p2", "p3"] {
        assert_eq!(r.path(id), ["running", "reported", "reported", "checked", "checked", "reviewed", "reviewed", "merged"], "{id}: {}", r.f.rig.dump());
    }
    let all = r.f.tasks();
    let p1 = all.iter().find(|t| t.spec.input["item"] == "p1" && t.spec.role.as_deref() == Some("piece")).unwrap();
    let check = all.iter().find(|t| t.spec.input["item"] == "p1" && t.spec.role.as_deref() == Some("check") && t.spec.place.starts_with('@')).unwrap();
    assert_eq!(check.spec.place, format!("@{}", p1.id), "check.after on the piece's held worktree");
    let review = all.iter().find(|t| t.spec.input["item"] == "p1" && t.spec.role.as_deref() == Some("review")).unwrap();
    assert_eq!(review.spec.doer, "own", "another doer than the writer ({}) and the main lane", p1.spec.doer);
    assert_eq!((review.spec.input["branch"]["context"].as_str(), review.spec.tune.mode, review.spec.place.as_str()), (Some("clean"), Mode::Read, format!("@{}", p1.id).as_str()));
    assert!(!review.spec.input["brief"].as_str().unwrap().contains("## plan"), "a clean context: nothing of the plan or the transcript");
}

#[test]
fn a_reviewers_concern_sends_the_piece_back_and_nobody_to_review_blocks_it_for_the_operator() {
    let concern = json!({ "answer": "no", "sure": "mid", "open": ["the retry is never reset"] }).to_string();
    let r = rig(pieces(), doer("pi", &["did"; 6]), doer("own", &[&concern, CLEAN, CLEAN, CLEAN, CLEAN]), checker(12), &["end done"]);
    r.begin();
    // the three checks end on their own threads, so which piece is reviewed first (and takes the scripted concern) is the order
    // they end in, not the plan's. The piece that goes back is the one whose review said the concern, whichever it is.
    r.until("a piece was sent back", |r| !r.sent_back().is_empty());
    let back = r.sent_back().remove(0);
    assert!(back.data["result"].as_str().unwrap().contains("the reviewer's concerns: the retry is never reset"), "{back:?}");
    let said = r.f.tasks().into_iter().find(|t| t.spec.role.as_deref() == Some("review") && t.answer.as_deref().is_some_and(|a| a.contains("the retry is never reset"))).expect("a review said the concern");
    assert_eq!(said.spec.input["item"], back.data["id"], "the piece that went back is the one that review was of: {}", r.f.rig.dump());
}

#[test]
fn a_piece_review_that_answers_in_words_sends_its_words_back_as_its_concern() {
    // a piece's review that is no report must not be taken for no concern, nor the piece laid in unread
    let words = "Read src/a/1.rs. The retry counter is never reset after a success.";
    let r = rig(pieces(), doer("pi", &["did"; 6]), doer("own", &[words, CLEAN, CLEAN, CLEAN, CLEAN]), checker(12), &["end done"]);
    r.begin();
    r.until("a piece was sent back", |r| !r.sent_back().is_empty());
    let back = r.sent_back().remove(0);
    assert!(back.data["result"].as_str().unwrap().contains("(the review's own words, not in the report's shape) Read src/a/1.rs. The retry counter is never reset"), "{back:?}");
}

#[test]
fn a_reviewer_that_fails_before_it_said_anything_is_out_and_the_piece_is_reviewed_on_another_doer() {
    let held = Held::new();
    let answers = ["hello".to_string(), planned(vec![item("p", "piece", &A, &["src/a/1.rs"], &[]), item("x", "inline", &[], &["src/x.rs"], &["p"]), item("y", "inline", &[], &["src/y.rs"], &["x"])])];
    let refs: Vec<&str> = answers.iter().map(String::as_str).collect();
    let no_login = Scripted::with_caps("ann", caps(), vec![vec![Beat::Fail("unexpected status 401 Unauthorized: Missing bearer or basic authentication in header".into())]]);
    let f = fx_placed(vec![chatty("claude", &refs), doer("pi", &["did"]), no_login, doer("gem", &[CLEAN; 2]), checker(2)], Some("pi"), vec![Arc::new(Handle(held.clone()))], no_replan);
    let r = Rig3 { f, held };
    r.begin();
    r.until("p was reviewed", |r| r.item("p").state == "reviewed" || r.item("p").state == "merged" || r.item("p").state == "blocked");
    assert_ne!(r.item("p").state, "blocked", "{}", r.f.rig.dump());
    let out = r.f.rig.of_kind("plan.doer_out");
    assert!(!out.is_empty(), "{}", r.f.rig.dump());
    assert_eq!((out.len(), out[0].data["doer"].as_str(), out[0].data["id"].as_str()), (1, Some("ann"), Some("p")), "{}", r.f.rig.dump());
    let reviewers: Vec<String> = r.f.tasks().into_iter().filter(|t| t.spec.role.as_deref() == Some("review")).map(|t| t.spec.doer).collect();
    assert_eq!(reviewers, ["ann", "gem"], "the second review is on another doer: {}", r.f.rig.dump());
}

/// One piece `p` and two inline items after it, reviewed by `own`, whose turns are `reviews`.
fn one_piece_reviewed_by(reviews: Vec<Vec<Beat>>) -> Rig3 {
    let held = Held::new();
    let answers = ["hello".to_string(), planned(vec![item("p", "piece", &A, &["src/a/1.rs"], &[]), item("x", "inline", &[], &["src/x.rs"], &["p"]), item("y", "inline", &[], &["src/y.rs"], &["x"])])];
    let refs: Vec<&str> = answers.iter().map(String::as_str).collect();
    let own = Scripted::with_caps("own", caps(), reviews);
    let f = fx_placed(vec![chatty("claude", &refs), doer("pi", &["did"; 6]), own, checker(10)], Some("pi"), vec![Arc::new(Handle(held.clone()))], no_replan);
    Rig3 { f, held }
}

fn broke_after(said: &str) -> Vec<Beat> {
    vec![Beat::Session("own-s".into()), Beat::Say(said.into()), Beat::Fail("The stream ended before completion.".into())]
}

#[test]
fn a_review_that_broke_off_after_it_had_begun_and_named_nothing_is_had_once_more_and_a_second_that_breaks_blocks_the_piece() {
    // a review on a doer whose stream ended, and one stopped at its limit, must not block its piece at once
    let fine = vec![Beat::Session("own-s".into()), Beat::Say(CLEAN.into()), end(CLEAN)];
    let r = one_piece_reviewed_by(vec![broke_after("reading src/a/1.rs"), fine]);
    r.begin();
    r.until("p was reviewed", |r| matches!(r.item("p").state.as_str(), "reviewed" | "merged" | "blocked"));
    assert_ne!(r.item("p").state, "blocked", "{}", r.f.rig.dump());
    assert_eq!(r.f.tasks().into_iter().filter(|t| t.spec.role.as_deref() == Some("review")).count(), 2, "{}", r.f.rig.dump());
    assert!(r.f.notes().iter().any(|n| n.contains("p: its review did not finish") && n.contains("reviewed once more")), "{:?}", r.f.notes());
    let r = one_piece_reviewed_by(vec![broke_after("reading src/a/1.rs"), broke_after("reading it again")]);
    r.begin();
    r.until("p was blocked", |r| r.item("p").state == "blocked");
    assert!(r.item("p").result.contains("its review did not finish"), "{}", r.item("p").result);
}

#[test]
fn a_review_that_broke_off_after_it_named_a_concern_sends_the_piece_back_with_it() {
    let concern = json!({ "answer": "src/a/1.rs read", "sure": "mid", "open": ["src/a/1.rs:1 the retry is never reset"] }).to_string();
    let fine = vec![Beat::Session("own-s".into()), Beat::Say(CLEAN.into()), end(CLEAN)];
    let r = one_piece_reviewed_by(vec![broke_after(&concern), fine]);
    r.begin();
    r.until("p was sent back", |r| !r.sent_back().is_empty());
    let back = r.sent_back().remove(0);
    assert!(back.data["result"].as_str().unwrap().contains("the retry is never reset"), "{back:?}\n{}", r.f.rig.dump());
}

/// One piece `p` and two inline items after it, whose reviews answer `reviews` in turn; the writer answers in words each time.
fn one_piece_reviewed_as(reviews: &[&str]) -> Rig3 {
    one_piece_reviewed_by(reviews.iter().map(|a| vec![Beat::Session("own-s".into()), Beat::Say(a.to_string()), end(a)]).collect())
}

fn review_said(open: &[&str], notes: &[&str]) -> String {
    json!({ "answer": "read the change", "sure": "mid", "found": [], "ran": [], "open": open, "notes": notes }).to_string()
}

#[test]
fn a_review_that_raises_again_a_point_it_sent_back_goes_to_main_and_does_not_hold_the_part_back() {
    // one part goes back four times, three of them for the review's points, until it is blocked; Main lays it in and its checks pass
    let first = review_said(&["src/a/1.rs:3 retry(0) returns 1; it should return 0"], &[]);
    let again = review_said(&["src/a/1.rs:3 the reset could be skipped if a later change reorders the calls"], &["src/a/1.rs:9 the name `n` is short"]);
    let r = one_piece_reviewed_as(&[&first, &again, CLEAN]);
    r.begin();
    r.until("p was laid in or blocked", |r| matches!(r.item("p").state.as_str(), "merged" | "blocked"));
    assert_eq!(r.item("p").state, "merged", "{}", r.f.rig.dump());
    assert_eq!(r.sent_back().len(), 1, "sent back once, for the case that failed; the point raised again is not sent back: {:?}", r.sent_back());
    let reviews: Vec<_> = r.f.tasks().into_iter().filter(|t| t.spec.role.as_deref() == Some("review")).collect();
    assert_eq!(reviews.len(), 2, "{}", r.f.rig.dump());
    let goal = &reviews[0].spec.goal;
    assert!(goal.contains("`open`") && goal.contains("`notes`") && goal.contains("would break if"), "the review is told what is a concern and what is a note: {goal}");
    let goal = &reviews[1].spec.goal;
    assert!(goal.contains("src/a/1.rs:3 retry(0) returns 1") && goal.contains("did"), "the second review is told what went back and what the part answered: {goal}");
    let reviewed = r.f.rig.of_kind("plan.item").into_iter().find(|e| e.data["id"] == "p" && e.data["state"] == "reviewed" && e.data["main"].is_array()).unwrap_or_else(|| panic!("{}", r.f.rig.dump()));
    let main = reviewed.data["main"].to_string();
    assert!(main.contains("src/a/1.rs:3 the reset could be skipped") && main.contains("src/a/1.rs:9 the name"), "the point raised again and the note go to Main: {main}");
    assert!(r.f.notes().iter().any(|n| n.starts_with("p:") && n.contains("for Main") && n.contains("src/a/1.rs:3 the reset could be skipped")), "{:?}", r.f.notes());
}

#[test]
fn a_review_that_names_a_failing_case_still_sends_the_part_back_and_one_with_notes_alone_does_not() {
    // points that name a file and no line: a new case in the same file is still new
    let first = review_said(&["src/a/2.rs retry(0) returns 1; it should return 0"], &[]);
    let second = review_said(&["src/a/2.rs retry(-1) loops forever"], &["src/a/1.rs:3 would break if `retry` were renamed"]);
    let last = review_said(&[], &["src/a/2.rs:1 a comment would help"]);
    let r = one_piece_reviewed_as(&[&first, &second, &last]);
    r.begin();
    r.until("p was laid in or blocked", |r| matches!(r.item("p").state.as_str(), "merged" | "blocked"));
    assert_eq!(r.item("p").state, "merged", "a review with notes and nothing open is no concern: {}", r.f.rig.dump());
    let back = r.sent_back();
    assert_eq!(back.len(), 2, "a failing case sends back each time it is a new one: {back:?}");
    assert!(back[1].data["result"].as_str().unwrap().contains("src/a/2.rs retry(-1) loops forever"), "{:?}", back[1]);
    assert!(!back[1].data["result"].as_str().unwrap().contains("would break if"), "a note is not sent back: {:?}", back[1]);
}

#[test]
fn a_review_that_answers_in_the_same_words_again_still_sends_the_part_back() {
    let words = "src/a/1.rs:3 The retry counter is never reset after a success.";
    let r = one_piece_reviewed_as(&[words, words, CLEAN]);
    r.begin();
    r.until("p was sent back twice, laid in or blocked", |r| r.sent_back().len() >= 2 || matches!(r.item("p").state.as_str(), "merged" | "blocked"));
    assert_eq!(r.sent_back().len(), 2, "a review in words is a concern each time: {}", r.f.rig.dump());
}

#[test]
fn with_nobody_but_the_writer_and_the_main_lane_to_review_the_piece_is_blocked_for_the_operator() {
    let held = Held::new();
    let answers = ["hello".to_string(), planned(vec![item("p", "piece", &A, &["src/a/1.rs"], &[]), item("x", "inline", &[], &["src/x.rs"], &["p"]), item("y", "inline", &[], &["src/y.rs"], &["x"])])];
    let refs: Vec<&str> = answers.iter().map(String::as_str).collect();
    let f = fx_placed(vec![chatty("claude", &refs), doer("pi", &["did"]), checker(2)], Some("pi"), vec![Arc::new(Handle(held.clone()))], no_replan);
    let r = Rig3 { f, held };
    r.begin();
    r.until("p was blocked", |r| r.item("p").state == "blocked");
    let p = r.item("p");
    assert_eq!(p.block, "no-doer");
    assert!(p.result.contains("nobody could review it"), "{}", p.result);
    let said = r.f.rig.of_kind("plan.reported");
    assert_eq!((said.len(), said[0].data["outcome"].as_str()), (1, Some("blocked")));
    assert!(said[0].data["decisions"][0].as_str().unwrap().starts_with("p:"), "no doer can be elected is the operator's: {:?}", said[0].data);
}


#[test]
fn held_pieces_merge_one_at_a_time_in_plan_order_with_the_checks_between_and_one_that_does_not_lay_goes_back() {
    let r = rig_cfg(pieces(), doer("pi", &["did"; 8]), doer("own", &[CLEAN; 8]), checker(16), &["end done"], retry);
    // the refusal is set before anything can reach a merge (the first pieces are t3, t4 and t5, in plan order), and the plan is
    // followed to its end: one `plan_step` takes an item through several states when the tasks end fast, so a state read between two
    // steps is the timing's and not the plan's. What is asserted is the order in the Log.
    r.held.refuse.lock().unwrap().insert("t4".into(), "src/b/1.rs changed in the folder".into());
    r.begin();
    let pieces = |id: &str| r.f.tasks().into_iter().filter(|t| t.spec.input["item"] == id && t.spec.role.as_deref() == Some("piece")).map(|t| t.id).collect::<Vec<_>>();
    assert_eq!(pieces("p2")[0], "t4", "the refused try is p2's first");
    r.until("the plan was done", |r| r.item("end").state == "merged");
    let (p1, p2, p3) = (pieces("p1"), pieces("p2"), pieces("p3"));
    assert_eq!(p2.len(), 2, "p2 was started again");
    assert_eq!(r.held.applied(), [p1[0].clone(), p2[1].clone(), p3[0].clone()], "one at a time, in plan order; the refused try was never applied");
    let events = r.f.rig.events();
    let applied = |t: &str| events.iter().position(|e| e.kind == "task.applied" && e.task.as_deref() == Some(t)).unwrap();
    let here = |e: &&j4n_core::log::Ev| e.kind == "task.queued" && e.data["spec"]["role"] == "check" && e.data["spec"]["place"] == "here";
    let checks_here: Vec<usize> = events.iter().enumerate().filter(|(_, e)| here(e) && e.data["spec"]["input"]["before"] != true).map(|(i, _)| i).collect();
    assert_eq!(checks_here.len(), 3, "a check of the folder after each merge");
    // nothing but the pieces changed the folder, and these write nothing in it: the check after one piece stands for the check before the next
    assert_eq!(events.iter().filter(|e| here(e) && e.data["spec"]["input"]["before"] == true).count(), 1, "one check of the folder before the first lay-in");
    assert!(applied(&p1[0]) < checks_here[0] && checks_here[0] < applied(&p2[1]) && applied(&p2[1]) < checks_here[1] && checks_here[1] < applied(&p3[0]), "the checks run between the merges");
    let back_at = events.iter().position(|e| e.kind == "plan.item" && e.data["id"] == "p2" && e.data["state"] == "sent-back").unwrap();
    assert!(applied(&p1[0]) < back_at && back_at < applied(&p3[0]), "p1 is laid in first; p2 is not forced, and p3, though its turn is next, waits until p2 went back and was laid in");
    let back = &events[back_at];
    let why = back.data["result"].as_str().unwrap();
    assert!(why.contains("src/b/1.rs changed in the folder") && why.contains("src/a/1.rs") && why.contains("1 file +3 −1"), "the reason and the diff: {why}");
    assert_eq!((r.item("p2").sends, r.item("p1").sends), (1, 0));
}

#[test]
fn a_piece_whose_checks_fail_once_it_is_laid_in_goes_back_and_is_not_merged() {
    // the checks on the held work and of the folder before a lay-in are green; the first check of the folder after a piece was laid in fails
    let failed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let judge: crate::scripted::Judge = Arc::new(move |_: &std::path::Path, spec: &j4n_core::task::TaskSpec| {
        let after = spec.place == "here" && spec.input["before"] != true;
        (after && !failed.swap(true, std::sync::atomic::Ordering::SeqCst)).then(|| vec![Beat::Fail("tests broke".into())])
    });
    let failing = Scripted::judging("check", judge, vec![vec![end("green")]]);
    let r = rig(pieces()[..3].to_vec(), doer("pi", &["did"; 6]), doer("own", &[CLEAN; 6]), failing, &[]);
    r.begin();
    r.until("p1 was sent back", |r| r.path("p1").contains(&"sent-back".to_string()));
    let back = r.f.rig.of_kind("plan.item").into_iter().find(|e| e.data["state"] == "sent-back").unwrap();
    assert!(back.data["result"].as_str().unwrap().contains("the checks failed once it was laid in"), "{back:?}");
    assert!(back.data["result"].as_str().unwrap().contains("tests broke"), "the check's own message goes back with the piece, not the word `stopped`: {back:?}");
    assert!(!r.path("p1").contains(&"merged".to_string()), "{:?}", r.path("p1"));
}


/// Python's unittest when the folder's own `tests/test_screen.py` imports a module that is not there yet.
pub const SCREEN_UNLOADED: &str = "6-unittest check failed (exit 1):\n======================================================================\nERROR: test_screen (unittest.loader._FailedTest.test_screen)\n----------------------------------------------------------------------\nImportError: Failed to import test module: test_screen\nTraceback (most recent call last):\n  File \"/home/user/chess/tests/test_screen.py\", line 4, in <module>\n    from chess import screen\nModuleNotFoundError: No module named 'chess.engine'\n\n\n----------------------------------------------------------------------\nRan 18 tests in 0.024s\n\nFAILED (errors=1)";

/// unittest once that module is there and the test loads: one of its own tests fails.
pub const SCREEN_DRAW_FAILS: &str = "6-unittest check failed (exit 1):\n======================================================================\nFAIL: test_draw (test_screen.ScreenTests.test_draw)\n----------------------------------------------------------------------\nTraceback (most recent call last):\n  File \"/home/user/chess/tests/test_screen.py\", line 20, in test_draw\n    self.assertEqual(rows, 12)\nAssertionError: 11 != 12\n\n----------------------------------------------------------------------\nRan 61 tests in 0.469s\n\nFAILED (failures=1)";

/// unittest with the module still missing, and a test that passed before failing too.
pub const SCREEN_AND_TURN: &str = "6-unittest check failed (exit 1):\n======================================================================\nERROR: test_screen (unittest.loader._FailedTest.test_screen)\n----------------------------------------------------------------------\nImportError: Failed to import test module: test_screen\nModuleNotFoundError: No module named 'chess.engine'\n\n======================================================================\nFAIL: test_turn (test_engine.Moves.test_turn)\n----------------------------------------------------------------------\nAssertionError: 'up' != 'left'\n\n----------------------------------------------------------------------\nRan 19 tests in 0.030s\n\nFAILED (failures=1, errors=1)";

/// A check that answers by the folder it is given: the checks of held work pass; the folder's own check says what `here` makes of the
/// number of modules laid in under `chess/` (the lay-ins of [`Held`]).
pub fn folder_check(here: fn(usize) -> Option<&'static str>) -> Arc<Scripted> {
    let judge: crate::scripted::Judge = Arc::new(move |dir: &std::path::Path, spec: &j4n_core::task::TaskSpec| {
        if spec.place != "here" {
            return None;
        }
        let laid = std::fs::read_dir(dir.join("chess")).map(|d| d.count()).unwrap_or(0);
        Some(match here(laid) {
            Some(out) => vec![Beat::Fail(out.into())],
            None => vec![end("6-unittest: passed")],
        })
    });
    Scripted::judging("check", judge, vec![vec![end("green")]])
}

#[test]
fn two_parts_that_the_folder_s_own_test_needs_both_land_in_turn_over_the_failure_the_folder_had_before_them() {
    // Main's uncommitted test imports the modules of two parts; a part's lay-in must not be rewound for failing on it, or neither could land first
    let items = vec![pieces()[0].clone(), pieces()[1].clone(), item("end", "inline", &[], &["src/e.rs"], &["p1", "p2"])];
    let check = folder_check(|laid| if laid < 2 { Some(SCREEN_UNLOADED) } else { Some(SCREEN_DRAW_FAILS) });
    let r = rig(items, doer("pi", &["did"; 6]), doer("own", &[CLEAN; 6]), check, &["end done"]);
    r.f.rig.project.put("tests/test_screen.py", "from chess import screen\n");
    *r.held.into.lock().unwrap() = Some(r.f.rig.project.path().to_path_buf());
    r.begin();
    r.until("both parts were merged", |r| r.item("p1").state == "merged" && r.item("p2").state == "merged");
    assert!(r.sent_back().is_empty(), "no part goes back for a failure that was there before it: {:?}", r.sent_back());
    let (p1, p2) = (r.item("p1").result, r.item("p2").result);
    assert!(p1.contains("the folder's check still fails as before") && p1.contains("_FailedTest.test_screen"), "{p1}");
    // the test module loads once both are there, and Main's own test that fails then is still the folder's: its module could not be loaded before
    assert!(p2.contains("the folder's check still fails as before") && p2.contains("test_screen.ScreenTests.test_draw"), "{p2}");
    let checks: Vec<Value> = r.f.tasks().into_iter().filter(|t| t.spec.role.as_deref() == Some("check") && t.spec.place == "here").map(|t| t.spec.input).collect();
    assert_eq!(checks.iter().filter(|i| i["before"] == true).count(), 1, "one check of the folder before the first lay-in; the check after it stands for the one before the next: {checks:?}");
    assert!(checks.iter().filter(|i| i["before"] != true).all(|i| i["base"].is_string()), "each check after a lay-in names the one it is held against: {checks:?}");
    assert!(r.f.notes().iter().any(|n| n.contains("Main's join is told")), "{:?}", r.f.notes());
}

#[test]
fn a_part_that_breaks_a_test_that_passed_before_it_is_still_sent_back_with_the_test_named() {
    let check = folder_check(|laid| if laid == 0 { Some(SCREEN_UNLOADED) } else { Some(SCREEN_AND_TURN) });
    let r = rig(vec![item("p", "piece", &A, &["src/a/1.rs"], &[]), item("x", "inline", &[], &["src/x.rs"], &["p"])], doer("pi", &["did"; 4]), doer("own", &[CLEAN; 4]), check, &[]);
    *r.held.into.lock().unwrap() = Some(r.f.rig.project.path().to_path_buf());
    r.begin();
    r.until("p was sent back", |r| !r.sent_back().is_empty());
    let why = r.sent_back()[0].data["result"].as_str().unwrap().to_string();
    assert!(why.contains("the checks failed once it was laid in") && why.contains("it broke test_engine.Moves.test_turn"), "{why}");
    assert!(!why.contains("_FailedTest"), "what failed before it is not what it broke: {why}");
    assert!(!r.path("p").contains(&"merged".to_string()), "{:?}", r.path("p"));
}

#[test]
fn a_check_whose_output_cannot_be_read_judges_the_part_by_the_check_alone_and_says_so() {
    let check = folder_check(|_| Some("7-make check failed (exit 2):\nmake: *** [test] Error 1"));
    let r = rig(vec![item("p", "piece", &A, &["src/a/1.rs"], &[]), item("x", "inline", &[], &["src/x.rs"], &["p"])], doer("pi", &["did"; 4]), doer("own", &[CLEAN; 4]), check, &[]);
    r.begin();
    r.until("p was sent back", |r| !r.sent_back().is_empty());
    assert!(r.sent_back()[0].data["result"].as_str().unwrap().contains("the checks failed once it was laid in"), "{:?}", r.sent_back());
    assert!(r.f.notes().iter().any(|n| n.contains("could not be held against the one before") && n.contains("not one J4n reads")), "{:?}", r.f.notes());
}


#[test]
fn an_item_sent_back_three_times_is_blocked_on_the_fourth_and_the_operator_is_told_once() {
    let r = rig_cfg(vec![item("p", "piece", &A, &["src/a/1.rs"], &[]), item("x", "inline", &[], &["src/x.rs"], &["p"]), item("y", "inline", &[], &["src/y.rs"], &["x"])], doer("pi", &["did"; 8]), doer("own", &[CLEAN; 8]), checker(16), &[], retry);
    *r.held.refuse_all.lock().unwrap() = Some("src/a/1.rs changed in the folder".into());
    r.begin();
    r.until("p was blocked", |r| r.item("p").state == "blocked");
    let p = r.item("p");
    assert_eq!((p.sends, p.tries, p.block.as_str()), (3, 4, "merge"), "{}", r.f.rig.dump());
    assert_eq!(r.path("p").iter().filter(|s| *s == "sent-back").count(), 3);
    assert_eq!(r.path("p").last().map(String::as_str), Some("blocked"));
    assert_eq!(r.path("p").iter().filter(|s| *s == "running").count(), 4, "no fourth retry after the block");
    assert!(p.result.contains("sent back 3 times"), "{}", p.result);
    r.step();
    r.step();
    assert_eq!(r.path("p").iter().filter(|s| *s == "running").count(), 4, "a blocked item is not started again");
    let said = r.f.rig.of_kind("plan.reported");
    assert_eq!(said.len(), 1, "one report when the plan blocks, not one for every look");
    assert_eq!(said[0].data["outcome"], "blocked");
    assert!(said[0].data["decisions"][0].as_str().unwrap().starts_with("p:"), "a merge the main lane cannot settle is the operator's: {:?}", said[0].data);
}


#[test]
fn a_piece_the_folder_refuses_at_laying_in_is_blocked_at_once_with_its_work_kept_and_named_and_not_started_again() {
    // its doer works in a copy that does not hold the folder's changes, so sending it back cannot clear the clash (`plan.lay_refused` `block`, the default)
    let r = rig(vec![item("p", "piece", &A, &["src/a/1.rs"], &[]), item("x", "inline", &[], &["src/x.rs"], &["p"])], doer("pi", &["did"; 4]), doer("own", &[CLEAN; 4]), checker(8), &[]);
    *r.held.refuse_all.lock().unwrap() = Some("src/a/1.rs changed in the folder".into());
    r.begin();
    r.until("p was blocked", |r| r.item("p").state == "blocked");
    let p = r.item("p");
    assert_eq!((p.sends, p.tries, p.block.as_str()), (0, 1, "merge"), "{}", r.f.rig.dump());
    assert!(!r.path("p").contains(&"sent-back".to_string()), "{:?}", r.path("p"));
    assert!(p.result.contains("src/a/1.rs changed in the folder") && p.result.contains("is kept at"), "the reason and where its work is: {}", p.result);
    r.step();
    assert_eq!(r.path("p").iter().filter(|s| *s == "running").count(), 1, "not started again");
    assert!(r.held.applied().is_empty(), "never laid in");
}

#[test]
fn a_branch_past_its_cap_is_killed_with_its_report_kept_its_item_is_blocked_and_the_main_lane_is_told() {
    let pi = Scripted::with_caps("pi", caps(), vec![vec![Beat::Session("pi-s".into()), Beat::Spend(0.21), Beat::Forever]]);
    let r = rig_cfg(vec![item("s", "scout", &A, &[], &[]), item("m", "inline", &[], &["src/m.rs"], &["s"]), item("n", "inline", &[], &["src/n.rs"], &["m"])], pi, doer("own", &[]), checker(1), &[], no_replan);
    r.begin();
    let scout = r.f.tasks().into_iter().find(|t| t.spec.role.as_deref() == Some("scout")).unwrap();
    r.f.rig.until(&scout.id, "capped", |t| t.capped);
    r.f.front.plan_step("main");
    r.f.rig.finished(&scout.id);
    r.f.front.plan_step("main");
    let s = r.item("s");
    assert_eq!((s.state.as_str(), s.block.as_str()), ("blocked", "cap"), "{}", r.f.rig.dump());
    assert!(s.result.contains("passed its cap"), "{}", s.result);
    assert_eq!(r.f.rig.of_kind("report.made").iter().filter(|e| e.task.as_deref() == Some(scout.id.as_str())).count(), 1, "its report is kept");
    assert!(r.f.lanes.waiting("main").iter().any(|(t, _)| *t == scout.id), "and it waits for the main lane's next Brief");
    assert!(r.f.notes().iter().any(|n| n.contains("passed its cap") && n.contains("blocked")), "the main lane is told: {:?}", r.f.notes());
    assert_eq!(r.f.lanes.lane("main").lines.iter().flat_map(|l| l.rows.iter()).find(|x| x.id == scout.id).map(|x| x.outcome), Some("killed"));
    assert_eq!(r.f.rig.of_kind("plan.reported").len(), 1);
    assert!(r.f.rig.of_kind("plan.reported")[0].data["decisions"][0].as_str().unwrap().starts_with("s:"), "a cap hit is the operator's to decide");
}


#[test]
fn the_plans_cost_and_time_are_the_sum_of_its_items_rows_and_of_its_branches_and_one_report_asks_nothing() {
    let scout = |n: usize| Scripted::with_caps("pi", caps(), (0..n).map(|k| vec![Beat::Session("pi-s".into()), Beat::Spend(0.01 * (k + 1) as f64), Beat::Say("looked".into()), end("looked")]).collect());
    let r = rig(vec![item("a", "scout", &A, &[], &[]), item("b", "scout", &B, &[], &[]), item("go", "inline", &[], &["src/o.rs"], &["a", "b"])], scout(2), doer("own", &[]), checker(1), &["went on"]);
    r.begin();
    r.until("the plan was done", |r| r.item("go").state == "merged");
    let ledger = r.f.front.plan_ledger("main").unwrap();
    let sum: i64 = ledger.items.iter().map(|(_, t)| t.micros).sum();
    assert_eq!(ledger.total.micros, sum, "the plan is the sum of its items' rows");
    assert_eq!(ledger.total.wall_ms, ledger.items.iter().map(|(_, t)| t.wall_ms).sum::<u64>());
    let lane = r.f.lanes.lane("main");
    let branches: i64 = ledger.items.iter().filter(|(id, _)| id == "a" || id == "b").map(|(_, t)| t.micros).sum();
    assert_eq!(branches, lane.total.micros, "and the branch items are what fold lanes says the branches took");
    assert!(branches >= 30_000, "{branches}");
    let said = r.f.rig.of_kind("plan.reported");
    assert_eq!(said.len(), 1, "one report per plan");
    let d = &said[0].data;
    assert_eq!((d["outcome"].as_str(), d["merged"].as_array().map(Vec::len), d["decisions"].as_array().map(Vec::len)), (Some("done"), Some(3), Some(0)), "{d}");
    assert_eq!(d["micros"].as_i64(), Some(ledger.micros()), "what it cost");
    assert!(r.f.tasks().iter().all(|t| t.spec.doer != "human"), "no question was opened: only the real decisions are asked, and there were none");
    r.step();
    assert_eq!(r.f.rig.of_kind("plan.reported").len(), 1, "and not again");
}
