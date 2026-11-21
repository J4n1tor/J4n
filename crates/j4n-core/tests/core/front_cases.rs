//! The front: a typed line becomes work. The loop, the Log, the Gate and the ask points are the real ones, the base pack
//! is read from disk, and the doers are scripted; what is asserted is read back from the Log and the loop's own books.

use crate::scripted::{Beat, Rig, Scripted, approval, choice, end, nothing, say};
use crate::world::{Scratch, open_log};
use j4n_core::answer::chain::Registry;
use j4n_core::answer::state::Answerers;
use j4n_core::answer::{Answerer, Kind, Offer, Paid, Question, Step};
use j4n_core::change::Actor;
use j4n_core::clock::ManualClock;
use j4n_core::doer::Doer;
use j4n_core::front::phrase::{Phrases, intake_rule};
use j4n_core::front::thread::Binds;
use j4n_core::front::{Cfg, Front, Said};
use j4n_core::gate::Gate;
use j4n_core::gate::check::Gates;
use j4n_core::gate::grant::Grants;
use j4n_core::log::{Ev, Log};
use j4n_core::rank::{NoColumns, RankAnswerer, specs};
use j4n_core::answer::point::points;
use j4n_core::table::{Layer, LayerKind, Tables};
use j4n_core::task::{Asks, Ctl, Row, State, TaskSpec, Why};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub fn base() -> Tables {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/base");
    let (mut layer, read) = Layer::from_dir(LayerKind::Pack, "base-data", &dir);
    assert!(read.is_empty(), "{read:?}");
    // the kinds are the core's own, compiled in; what is loaded here is what the pack says with them
    layer.files.retain(|f| !f.path.starts_with("kinds/") && f.path != "pack.toml");
    let (tables, problems) = Tables::load(vec![layer]);
    assert!(problems.is_empty(), "{problems:?}");
    tables
}

/// A decider that counts how often it is asked — what a bound follow-up must never reach — keeps the questions it is put, and
/// answers the pick it was given, or passes.
/// What a test runs once, inside the decider's next answer to the point `reflex` or `retry` (what the operator does while the judge is thinking).
pub type Hook = Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>;

struct Decider(Arc<AtomicUsize>, Arc<Mutex<Vec<Question>>>, Option<&'static str>, Arc<Mutex<Option<Paid>>>, Arc<Mutex<Option<String>>>, bool, Hook, Arc<AtomicBool>, Lag);

/// How long the decider takes to answer a question, as a slow light model does: the questions of `chats`, `weigh` and `complete` wait this long.
pub type Lag = Arc<Mutex<std::time::Duration>>;

impl Answerer for Decider {
    fn id(&self) -> &str {
        "decider:jev"
    }

    fn kind(&self) -> Kind {
        Kind::Decider
    }

    fn offer(&self, q: &Question) -> Offer {
        self.offer_paid(q).0
    }

    /// What each call cost, if the test said (`Fx::pays`): else the decider does not report, as one that does not.
    fn offer_paid(&self, q: &Question) -> (Offer, Option<Paid>) {
        let paid = *self.3.lock().unwrap();
        (self.answer(q), paid)
    }

    /// A decider that names its model is held to its cases (`unproved:`); one that does not is not.
    fn model(&self) -> String {
        assert!(!self.7.load(Ordering::SeqCst), "the decider was told to panic (a panic on a reflex's thread)");
        if self.5 { "jev-test".into() } else { String::new() }
    }

    /// Where its questions go (`Fx::zone`): none, as it starts, so no `ask.egress` is written.
    fn zone(&self) -> Option<String> {
        self.4.lock().unwrap().clone()
    }
}

impl Decider {
    fn answer(&self, q: &Question) -> Offer {
        self.0.fetch_add(1, Ordering::SeqCst);
        self.1.lock().unwrap().push(q.clone());
        let lag = *self.8.lock().unwrap();
        if matches!(q.point.as_str(), "chats" | "weigh" | "complete") && !lag.is_zero() {
            std::thread::sleep(lag);
        }
        let once = if matches!(q.point.as_str(), "reflex" | "retry") { self.6.lock().unwrap().take() } else { None };
        if let Some(run) = once {
            run();
        }
        // "point=id": answers that point only; "point=a,b": the first of them that is offered, else it passes
        let offered = |id: &&str| matches!(&q.shape, j4n_core::ask::AskShape::Choice { options } if options.iter().any(|o| o.id == *id));
        // "point=id;point=id": each point its own pick
        // a bare id answers the questions of the front that name a doer or a chat, not the questions of the read (those are answered by `read:kind=…`)
        let seg = self.2.and_then(|spec| spec.split(';').find(|seg| seg.split_once('=').map_or(!q.point.starts_with("read:"), |(point, _)| point == q.point)));
        let pick = seg.map(|p| p.split_once('=').map_or(p, |(_, id)| id)).and_then(|id| if id.contains(',') { id.split(',').find(offered) } else { Some(id) });
        match pick {
            // "point=fail": the call fails, as a Decider that is down
            Some("fail") => Offer::Failed { why: "it is down".into() },
            Some(id) => Offer::Answer { answer: j4n_core::ask::Answer::Pick { id: id.into() }, conf: 0.9, why: "it knows".into() },
            None => Offer::Pass { why: "it has no opinion".into() },
        }
    }
}

pub struct Fx {
    pub front: Front,
    pub rig: Rig,
    /// How often the decider was asked.
    pub decided: Arc<AtomicUsize>,
    /// What it was asked.
    pub asked: Arc<Mutex<Vec<Question>>>,
    /// The plan of each thread.
    pub keeper: j4n_core::keeper::Keeper,
    /// The branches of each thread and the reports that wait for its main lane.
    pub lanes: j4n_core::lanes::Lanes,
    /// What every call of the decider reports it cost; `None`, as it starts: it does not report.
    pub pays: Arc<Mutex<Option<Paid>>>,
    /// The zone the decider's questions go to: `None`, as it starts, keeps them on this machine and the Log writes no `ask.egress`.
    pub zone: Arc<Mutex<Option<String>>>,
    /// One thing to do inside the decider's next answer to the point `reflex`.
    pub hook: Hook,
    /// The decider panics when its model is asked for (a panic that the chain does not catch).
    pub boom: Arc<AtomicBool>,
    /// How long the decider takes over the questions of `chats`, `weigh` and `complete`: none, as it starts.
    pub lag: Lag,
}

pub fn fx(doers: Vec<Arc<Scripted>>, cfg: impl FnOnce(&mut Cfg)) -> Fx {
    fx_deciding(doers, None, cfg)
}

/// [`fx`], with a decider that answers `pick` to every question it is asked.
pub fn fx_deciding(doers: Vec<Arc<Scripted>>, pick: Option<&'static str>, cfg: impl FnOnce(&mut Cfg)) -> Fx {
    let scratch = (Scratch::new("proj"), Scratch::new("home"));
    let (log, clock) = open_log(&scratch.0);
    fx_over(scratch.0, scratch.1, log, clock, doers, pick, cfg)
}

pub fn fx_over(project: Scratch, home: Scratch, log: Log, clock: Arc<ManualClock>, doers: Vec<Arc<Scripted>>, pick: Option<&'static str>, cfg: impl FnOnce(&mut Cfg)) -> Fx {
    build(project, home, log, clock, doers, pick, &[], false, None, Vec::new(), false, j4n_core::cost::Plans::default(), cfg)
}

/// [`fx`] with the plans and cost sources the base pack ships, so a doer named as one of them has its provider's cache life.
pub fn fx_planned(doers: Vec<Arc<Scripted>>, cfg: impl FnOnce(&mut Cfg)) -> Fx {
    let scratch = (Scratch::new("proj"), Scratch::new("home"));
    let (log, clock) = open_log(&scratch.0);
    let (plans, problems) = j4n_core::cost::Plans::load(&base());
    assert!(problems.is_empty(), "{problems:?}");
    build(scratch.0, scratch.1, log, clock, doers, None, &[], false, None, Vec::new(), false, plans, cfg)
}

/// [`fx_planned`] drawing `screen` and with a decider that answers `pick` (the reflexes' rows are in the screen's tables), in a run (`headless`) or at the keyboard.
pub fn fx_reflexes_in(doers: Vec<Arc<Scripted>>, pick: Option<&'static str>, screen: j4n_core::front::screen::Screen, headless: bool, cfg: impl FnOnce(&mut Cfg)) -> Fx {
    let scratch = (Scratch::new("proj"), Scratch::new("home"));
    let (log, clock) = open_log(&scratch.0);
    let (plans, problems) = j4n_core::cost::Plans::load(&base());
    assert!(problems.is_empty(), "{problems:?}");
    build(scratch.0, scratch.1, log, clock, doers, pick, &[], false, Some(screen), Vec::new(), headless, plans, cfg)
}

/// [`fx_reflexes_in`] again, over the Log and the clock of `old`: the process was stopped and started, and nothing of the front's memory is left.
pub fn fx_reflexes_again(old: &Fx, doers: Vec<Arc<Scripted>>, pick: Option<&'static str>, screen: j4n_core::front::screen::Screen, cfg: impl FnOnce(&mut Cfg)) -> Fx {
    let (plans, problems) = j4n_core::cost::Plans::load(&base());
    assert!(problems.is_empty(), "{problems:?}");
    build(Scratch::new("proj"), Scratch::new("home"), old.rig.log.clone(), old.rig.clock.clone(), doers, pick, &[], false, Some(screen), Vec::new(), false, plans, cfg)
}

/// [`fx_deciding`] in a run: nobody is at the keyboard (`j4n run`).
pub fn fx_headless(doers: Vec<Arc<Scripted>>, pick: Option<&'static str>, cfg: impl FnOnce(&mut Cfg)) -> Fx {
    let scratch = (Scratch::new("proj"), Scratch::new("home"));
    let (log, clock) = open_log(&scratch.0);
    build(scratch.0, scratch.1, log, clock, doers, pick, &[], false, None, Vec::new(), true, j4n_core::cost::Plans::default(), cfg)
}

/// [`fx_deciding`], with these places registered beside `here` (a held worktree).
pub fn fx_placed(doers: Vec<Arc<Scripted>>, pick: Option<&'static str>, places: Vec<Arc<dyn j4n_core::place::Place>>, cfg: impl FnOnce(&mut Cfg)) -> Fx {
    let scratch = (Scratch::new("proj"), Scratch::new("home"));
    let (log, clock) = open_log(&scratch.0);
    build(scratch.0, scratch.1, log, clock, doers, pick, &[], false, None, places, false, j4n_core::cost::Plans::default(), cfg)
}

/// [`fx_placed`] in a run: nobody is at the keyboard.
pub fn fx_placed_headless(doers: Vec<Arc<Scripted>>, pick: Option<&'static str>, places: Vec<Arc<dyn j4n_core::place::Place>>, cfg: impl FnOnce(&mut Cfg)) -> Fx {
    let scratch = (Scratch::new("proj"), Scratch::new("home"));
    let (log, clock) = open_log(&scratch.0);
    build(scratch.0, scratch.1, log, clock, doers, pick, &[], false, None, places, true, j4n_core::cost::Plans::default(), cfg)
}

thread_local! {
    /// The fixture being built is a run under `--approval full`.
    static FULL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// [`fx_placed_headless`] under `--approval full`: every yes the Gate does not forbid is given.
pub fn fx_placed_full(doers: Vec<Arc<Scripted>>, pick: Option<&'static str>, places: Vec<Arc<dyn j4n_core::place::Place>>, cfg: impl FnOnce(&mut Cfg)) -> Fx {
    FULL.with(|f| f.set(true));
    let fx = fx_placed_headless(doers, pick, places, cfg);
    FULL.with(|f| f.set(false));
    fx
}

/// [`fx_placed`] drawing `screen`, as the settings it holds say.
pub fn fx_placed_on(doers: Vec<Arc<Scripted>>, pick: Option<&'static str>, places: Vec<Arc<dyn j4n_core::place::Place>>, screen: j4n_core::front::screen::Screen, cfg: impl FnOnce(&mut Cfg)) -> Fx {
    let scratch = (Scratch::new("proj"), Scratch::new("home"));
    let (log, clock) = open_log(&scratch.0);
    build(scratch.0, scratch.1, log, clock, doers, pick, &[], false, Some(screen), places, false, j4n_core::cost::Plans::default(), cfg)
}

/// [`fx_deciding`], with the rules that label a machine's answers (`packs/base/rules/trust.toml`) in the loop.
pub fn fx_trusting(doers: Vec<Arc<Scripted>>, pick: Option<&'static str>, cfg: impl FnOnce(&mut Cfg)) -> Fx {
    let (project, home) = (Scratch::new("proj"), Scratch::new("home"));
    let (log, clock) = open_log(&project);
    build(project, home, log, clock, doers, pick, &[], true, None, Vec::new(), false, j4n_core::cost::Plans::default(), cfg)
}

/// [`fx_trusting_on`] in a run (`headless`): nobody is at the keyboard, the screen is only the settings.
pub fn fx_trusting_headless(doers: Vec<Arc<Scripted>>, pick: Option<&'static str>, screen: j4n_core::front::screen::Screen, cfg: impl FnOnce(&mut Cfg)) -> Fx {
    let (project, home) = (Scratch::new("proj"), Scratch::new("home"));
    let (log, clock) = open_log(&project);
    build(project, home, log, clock, doers, pick, &[], true, Some(screen), Vec::new(), true, j4n_core::cost::Plans::default(), cfg)
}

/// [`fx_trusting`] drawing `screen`: the base pack's rules and its table of failures (the class of a failure is read from it) and a screen of its settings.
pub fn fx_trusting_on(doers: Vec<Arc<Scripted>>, pick: Option<&'static str>, screen: j4n_core::front::screen::Screen, cfg: impl FnOnce(&mut Cfg)) -> Fx {
    let (project, home) = (Scratch::new("proj"), Scratch::new("home"));
    let (log, clock) = open_log(&project);
    build(project, home, log, clock, doers, pick, &[], true, Some(screen), Vec::new(), false, j4n_core::cost::Plans::default(), cfg)
}

/// [`fx`] drawing `screen` in place of the base pack's.
pub fn fx_screened(doers: Vec<Arc<Scripted>>, screen: j4n_core::front::screen::Screen, cfg: impl FnOnce(&mut Cfg)) -> Fx {
    let scratch = (Scratch::new("proj"), Scratch::new("home"));
    let (log, clock) = open_log(&scratch.0);
    build(scratch.0, scratch.1, log, clock, doers, None, &[], false, Some(screen), Vec::new(), false, j4n_core::cost::Plans::default(), cfg)
}

/// [`fx`] over a base pack that lacks these ask points.
pub fn fx_without(doers: Vec<Arc<Scripted>>, dropped: &[&str], cfg: impl FnOnce(&mut Cfg)) -> Fx {
    let (project, home) = (Scratch::new("proj"), Scratch::new("home"));
    let (log, clock) = open_log(&project);
    build(project, home, log, clock, doers, None, dropped, false, None, Vec::new(), false, j4n_core::cost::Plans::default(), cfg)
}

#[allow(clippy::too_many_arguments)]
fn build(project: Scratch, home: Scratch, log: Log, clock: Arc<ManualClock>, doers: Vec<Arc<Scripted>>, pick: Option<&'static str>, dropped: &[&str], trust: bool, screen: Option<j4n_core::front::screen::Screen>, places: Vec<Arc<dyn j4n_core::place::Place>>, headless: bool, plans: j4n_core::cost::Plans, cfg: impl FnOnce(&mut Cfg)) -> Fx {
    let tables = base();
    let (mut pts, problems) = points(&tables);
    pts.retain(|p| !dropped.contains(&p.id.as_str()));
    assert!(problems.is_empty(), "{problems:?}");
    let (gates, problems) = Gates::load(&tables);
    assert!(problems.is_empty(), "{problems:?}");
    let (mut phrases, problems) = Phrases::load(&tables);
    assert!(j4n_core::front::screen::Screen::builtin().commands.claim(&mut phrases).is_empty(), "the words of the base commands are free");
    assert!(problems.is_empty(), "{problems:?}");
    let (rank_specs, problems) = specs(&tables);
    assert!(problems.is_empty(), "{problems:?}");
    let (decided, asked) = (Arc::new(AtomicUsize::new(0)), Arc::new(Mutex::new(Vec::new())));
    let pays: Arc<Mutex<Option<Paid>>> = Arc::default();
    let zone: Arc<Mutex<Option<String>>> = Arc::default();
    let (hook, boom): (Hook, Arc<AtomicBool>) = (Arc::default(), Arc::default());
    let lag: Lag = Arc::default();
    let mut registry = Registry::new();
    let keeper = j4n_core::keeper::Keeper::new();
    let lanes = j4n_core::lanes::Lanes::new();
    registry.register(Arc::new(intake_rule(phrases.clone())));
    registry.register(Arc::new(j4n_core::front::phrase::weigh_rule(phrases.clone())));
    registry.register(Arc::new(j4n_core::keeper::fold_rule(keeper.clone())));
    registry.register(Arc::new(j4n_core::front::named::named_rule(doers.iter().map(|d| d.id().to_string()).collect())));
    registry.register(Arc::new(j4n_core::front::retry::retry_rule()));
    registry.register(Arc::new(j4n_core::front::reflexes::reflex_rule()));
    registry.register(Arc::new(j4n_core::front::access::access_rule()));
    registry.register(Arc::new(RankAnswerer::new(rank_specs, &pts, Box::new(NoColumns))));
    // `Some("unproved:<pick>")`: Jev is there and answers `<pick>`, but has not passed the cases of `weigh` (a decider whose cases are not proved is skipped)
    let (state, unproved) = (Answerers::new(), pick.is_some_and(|p| p.starts_with("unproved:")));
    let pick = match pick {
        Some(p) if p.starts_with("unproved:") => {
            state.require([("weigh".to_string(), "unproved".to_string())].into());
            Some(&p["unproved:".len()..])
        }
        p => p,
    };
    // `Some("absent")`: Jev is not installed, as in the operator's run (`decider:jev` skipped, `not installed`)
    if pick != Some("absent") {
        registry.register(Arc::new(Decider(decided.clone(), asked.clone(), pick, pays.clone(), zone.clone(), unproved, hook.clone(), boom.clone(), lag.clone())));
    }
    // the signed charters, where there is an operator to ask: the same handle in the loop, the fold and the front
    let charters = j4n_core::charter::Charters::new();
    let asks = Asks::new(pts, registry, state).with_gate(Gate::new(gates, Grants::new("s1"))).headless(headless).full(FULL.with(std::cell::Cell::get));
    let asks = Arc::new(if headless { asks } else { asks.with_charters(charters.clone()) });
    let binds = Binds::new();
    let all: Vec<Arc<dyn Doer>> = doers.iter().map(|d| d.clone() as Arc<dyn Doer>).collect();
    let rig = Rig::over(project, home, log, clock, doers.clone(), {
        let (binds, asks, keeper, lanes, charters) = (binds.clone(), asks.clone(), keeper.clone(), lanes.clone(), charters.clone());
        move |c| {
            c.asks = Some(asks);
            let problems = c.folds.add(Box::new(charters), c.log.head());
            assert!(problems.is_empty(), "{problems:?}");
            c.plans = Arc::new(plans);
            c.places.extend(places);
            let problems = c.folds.add(Box::new(binds), c.log.head());
            assert!(problems.is_empty(), "{problems:?}");
            let problems = c.folds.add(Box::new(keeper), c.log.head());
            assert!(problems.is_empty(), "{problems:?}");
            let problems = c.folds.add(Box::new(lanes), c.log.head());
            assert!(problems.is_empty(), "{problems:?}");
            // the windows the bottom bar reads, as the wiring adds them
            let problems = c.folds.add(Box::<j4n_core::cost::PlanRate>::default(), c.log.head());
            assert!(problems.is_empty(), "{problems:?}");
            if trust {
                crate::scripted::load_base(c, include_str!("../../../../packs/base/rules/trust.toml").trim_start_matches("api = 1\n"));
            }
        }
    });
    let mut c = Cfg::new("proj");
    c.charters = charters.clone();
    cfg(&mut c);
    charters.set_on(c.crew.on);
    // the doer `check` runs the project's checks, as its manifest says: it is nobody's pick for another role
    let who = Arc::new(j4n_core::front::who::Who::new(asks.clone(), &all, [("check".to_string(), vec!["check".to_string()])].into(), c.hosts.clone(), rig.log.clone()));
    let front = Front::new(c, rig.tasks.clone(), rig.log.clone(), binds, asks, phrases, all).with_who(who).with_keeper(keeper.clone()).with_lanes(lanes.clone());
    let front = match screen {
        Some(s) => front.with_screen(Arc::new(s)),
        None => front,
    };
    if let Some(hooks) = front.charter_hooks() {
        charters.attach(hooks);
    }
    if let Some(gate) = front.who().gate() {
        gate.hold_by(front.envoy_hold_hook());
    }
    Fx { front, rig, decided, asked, keeper, lanes, pays, zone, hook, boom, lag }
}

impl Fx {
    pub fn say(&self, line: &str) -> Said {
        self.front.say("main", line)
    }

    /// [`Fx::say`], and the question of which chat takes the line, if it is asked, answered with a new chat: what a line that is nobody's gets.
    pub fn say_new(&self, line: &str) -> Said {
        match self.say(line) {
            Said::Asking { ask } if ask.starts_with("route:") => self.front.reply("main", &ask, j4n_proto::Reply::Pick { id: "new".into() }),
            other => other,
        }
    }

    /// Say a line that starts a task, and wait for the task to be over.
    pub fn turn(&self, line: &str) -> Row {
        match self.say_new(line) {
            Said::Started { task } => self.rig.finished(&task),
            other => panic!("{line:?} did not start a task: {other:?}\n{}", self.rig.dump()),
        }
    }

    pub fn count(&self, kind: &str) -> usize {
        self.rig.of_kind(kind).len()
    }

    pub fn tasks(&self) -> Vec<Row> {
        self.rig.tasks.book().rows()
    }

    /// The text of the notes written to the chat.
    pub fn notes(&self) -> Vec<String> {
        self.rig.of_kind("chat.note").iter().filter_map(|e| e.data["text"].as_str().map(String::from)).collect()
    }

    pub fn bound(&self) -> Vec<Ev> {
        self.rig.of_kind("thread.bound")
    }
}

/// A doer that says one thing in each turn and reports a session, so the next turn can carry on in it.
pub fn chatty(id: &str, answers: &[&str]) -> Arc<Scripted> {
    let turns = answers.iter().map(|a| vec![Beat::Session(format!("{id}-session")), say(a), end(a)]).collect();
    Scripted::new(id, turns)
}

fn pinned(doer: &'static str, model: Option<&'static str>) -> impl FnOnce(&mut Cfg) {
    move |c| {
        c.doer = Some(doer.into());
        c.model = model.map(String::from);
    }
}


#[test]
fn three_lines_a_read_a_change_and_a_question_stay_on_one_doer_and_one_model_and_no_worktree_is_made() {
    let claude = chatty("claude", &["it prints a greeting", "added the flag", "--verbose, with -v as its short form"]);
    let f = fx(vec![claude.clone()], pinned("claude", Some("haiku")));
    let lines = ["what does main.rs do?", "add a --verbose flag to main.rs", "should the short form be -v?"];
    let rows: Vec<Row> = lines.iter().map(|l| f.turn(l)).collect();
    assert!(rows.iter().all(|r| r.state == State::Done { ok: true }), "{}", f.rig.dump());
    assert!(rows.iter().all(|r| r.spec.doer == "claude" && r.spec.tune.model.as_deref() == Some("haiku") && r.spec.place == "here"));
    assert_eq!(f.bound().len(), 1, "the thread is bound once");
    assert_eq!(rows[0].answer.as_deref(), Some("it prints a greeting"));
    assert_eq!(rows[2].answer.as_deref(), Some("--verbose, with -v as its short form"));
    // each turn carried on in the session of the one before
    let starts = claude.starts();
    assert_eq!(starts.iter().map(|s| s.resume.clone()).collect::<Vec<_>>(), [None, Some("claude-session".into()), Some("claude-session".into())]);
    assert_eq!(rows[1].spec.follows.as_deref(), Some(rows[0].id.as_str()));
    assert!(!f.rig.home.join("worktrees").exists() && !f.rig.project.join(".git").exists(), "nothing but the project's own folder");
}

#[test]
fn fresh_clears_the_binding_and_the_next_line_elects_once() {
    let f = fx(vec![chatty("alpha", &["a"]), chatty("beta", &["b"])], |_| {});
    f.turn("first line");
    assert_eq!(f.front.binding("main").map(|b| (b.doer, b.how)), Some(("alpha".into(), "elected".into())), "the smaller id when the numbers say nothing");
    assert_eq!(f.say(":switch beta"), Said::Command("switch".into()));
    assert_eq!(f.front.binding("main").map(|b| (b.doer, b.how)), Some(("beta".into(), "switch".into())));
    assert_eq!(f.say(":fresh"), Said::Command("fresh".into()));
    assert_eq!(f.front.binding("main"), None, ":fresh clears the binding");
    assert_eq!(f.count("intake.decided"), 1, "nothing was asked until the next line");
    f.turn("a new start");
    let elected: Vec<_> = f.bound().into_iter().filter(|e| e.data["how"] == "elected").collect();
    assert_eq!(elected.len(), 2, "once before :fresh, once after it");
    assert_eq!(f.count("intake.decided"), 2);
    assert_eq!(f.front.binding("main").map(|b| b.how), Some("elected".into()));
}

#[test]
fn fresh_ends_the_doers_session_and_the_line_after_it_starts_a_new_one() {
    // CONTEXT §1 row 2: `:fresh`, and the next line resumed the old session
    let claude = chatty("claude", &["a", "b", "c", "d"]);
    let f = fx(vec![claude.clone()], pinned("claude", None));
    f.turn("one");
    let second = f.turn("two");
    assert_eq!(second.spec.resume.as_ref().map(|s| s.id.as_str()), Some("claude-session"), "before :fresh a line carries on");
    assert_eq!(f.say(":fresh"), Said::Command("fresh".into()));
    let third = f.turn("three");
    assert_eq!(third.spec.resume, None, "after :fresh the line starts a session: {}", f.rig.dump());
    assert_eq!(third.spec.follows, None);
    let fourth = f.turn("four");
    assert_eq!(fourth.spec.resume.as_ref().map(|s| s.id.as_str()), Some("claude-session"), "and the line after that carries on in the new one");
    assert_eq!(fourth.spec.follows.as_deref(), Some(third.id.as_str()));
    assert_eq!(claude.starts().iter().map(|s| s.resume.clone()).collect::<Vec<_>>(), [None, Some("claude-session".into()), None, Some("claude-session".into())]);
}

#[test]
fn two_lines_are_one_thread_and_the_second_carries_on_in_the_session_of_the_first() {
    let claude = chatty("claude", &["one", "two"]);
    let f = fx(vec![claude.clone()], pinned("claude", None));
    let (a, b) = (f.turn("first"), f.turn("second"));
    assert_eq!(a.spec.thread, b.spec.thread);
    assert_eq!(b.spec.resume.as_ref().map(|s| s.id.as_str()), Some("claude-session"));
    assert_eq!(claude.starts()[1].resume.as_deref(), Some("claude-session"), "the doer was asked to carry on, not to begin");
    assert_eq!(f.bound().len(), 1);
}

#[test]
fn a_bound_follow_up_asks_nobody_the_decider_is_never_reached_and_no_intake_is_written() {
    let f = fx(vec![chatty("alpha", &["a", "b", "c", "d"])], |_| {});
    f.turn("first line");
    let (decided, intakes, asks) = (f.decided.load(Ordering::SeqCst), f.count("intake.decided"), f.count("ask.step"));
    assert_eq!(decided, 2, "the first line is asked about, twice (what it is, and who does it), and the rules did not know");
    for line in ["continue", "yes", "2", "do it"] {
        // "yes" and "2" answer nothing: no question is open, so they are the next turn like any other line
        f.turn(line);
    }
    assert_eq!(f.decided.load(Ordering::SeqCst), decided, "the decider was not asked again");
    assert_eq!(f.count("intake.decided"), intakes);
    assert_eq!(f.count("ask.step"), asks);
    assert_eq!(f.tasks().iter().filter(|t| t.spec.doer == "human").count(), 0, "no question task of any kind");
}

#[test]
fn the_wall_ends_a_never_ending_doer_with_its_report_and_a_flushed_log() {
    let hang = Scripted::new("claude", vec![vec![Beat::Session("s".into()), say("the first half"), Beat::Forever]]);
    // the deadline is the process's own time (an `Instant`), so the line has to be said before it and "one more" after it: it is far
    // enough for a busy machine to build the fixture and say the line (400 ms was not), and the task's wall is run out on its own
    // clock, so the test does not wait for it
    let end = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let f = fx(vec![hang], |c| {
        c.doer = Some("claude".into());
        c.deadline = Some(end);
    });
    let Said::Started { task } = f.say("go on forever") else { panic!("{}", f.rig.dump()) };
    f.rig.until_event("the first words", |e| e.kind == "run.message");
    f.rig.clock.advance(3_000);
    f.rig.timers.poke();
    let row = f.rig.finished(&task);
    assert_eq!(row.state, State::Stopped { why: Why::Wall });
    assert_eq!(row.answer.as_deref(), Some("the first half"), "the report is what it had said");
    // the Log was synced: every event of the task is in the files
    let on_disk: Vec<String> = f.rig.log.read(0).map(|e| e.kind).collect();
    assert!(on_disk.contains(&"task.stopped".to_string()), "{on_disk:?}");
    // a line after the time is up is refused, not started
    std::thread::sleep(end.saturating_duration_since(std::time::Instant::now()) + std::time::Duration::from_millis(50));
    assert!(matches!(f.say("one more"), Said::Refused(why) if why.contains("run out")));
}

// ---- the pin and the election ----

#[test]
fn a_pin_binds_the_thread_without_an_election_and_a_pin_that_names_nothing_is_refused_with_the_names() {
    let f = fx(vec![chatty("alpha", &["a"])], pinned("alpha", Some("m1")));
    f.turn("hello");
    let b = f.front.binding("main").unwrap();
    assert_eq!((b.doer.as_str(), b.model.as_deref(), b.how.as_str()), ("alpha", Some("m1"), "pinned"));
    assert!(!f.rig.dump().contains("who:work"), "nobody elected anything");
    let g = fx(vec![chatty("alpha", &["a"])], pinned("nosuch", None));
    let Said::Refused(why) = g.say("hello") else { panic!("{}", g.rig.dump()) };
    assert!(why.contains("nosuch") && why.contains("alpha"), "{why}");
    assert_eq!(g.front.binding("main"), None, "a refused election binds nothing");
    assert!(g.tasks().is_empty());
}

#[test]
fn with_no_doer_at_all_the_first_line_is_refused_in_words() {
    let f = fx(Vec::new(), |_| {});
    let Said::Refused(why) = f.say("hello") else { panic!("{}", f.rig.dump()) };
    assert!(why.contains("no doer"), "{why}");
}

#[test]
fn intake_is_asked_once_and_written_down_with_who_answered() {
    let f = fx(vec![chatty("alpha", &["a"])], |_| {});
    f.turn("what is in here?");
    let e = f.rig.of_kind("intake.decided");
    assert_eq!(e.len(), 1);
    assert_eq!((e[0].data["pick"].as_str(), e[0].data["by"].as_str()), (Some("task"), Some("fallback:pick")));
    assert_eq!(e[0].thread.as_deref(), Some("main"));
}

#[test]
fn the_options_of_intake_in_the_code_and_in_the_pack_are_the_same() {
    let t = base();
    assert_eq!(j4n_core::answer::point::options(&t, "intake"), j4n_core::front::INTAKE);
}

// ---- a line while the task works ----

#[test]
fn a_line_typed_while_the_task_works_steers_it_and_one_typed_while_it_waits_is_refused() {
    let doer = Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Hold, end("done")]]);
    let f = fx(vec![doer.clone()], |_| {});
    let Said::Started { task } = f.say("start") else { panic!() };
    f.rig.until(&task, "running", |r| r.state == State::Running);
    assert_eq!(f.say("and mind the tests"), Said::Steered { task: task.clone() });
    f.rig.until_event("the steer", |e| e.kind == "task.steered");
    assert!(doer.ctls().iter().any(|c| matches!(c, j4n_core::doer::RunCtl::Steer(t) if t == "and mind the tests")));
    assert!(f.rig.tasks.ctl(&task, Ctl::Abort, Actor::Operator).is_ok());
    f.rig.finished(&task);

    let asker = Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Ask(approval("a", "make build")), Beat::Hold, end("built")]]);
    let g = fx(vec![asker], |_| {});
    let Said::Started { task } = g.say("build it") else { panic!() };
    g.rig.until(&task, "waiting", |r| matches!(r.state, State::Waiting { .. }));
    let Said::Refused(why) = g.say("also run the tests afterwards") else { panic!("{}", g.rig.dump()) };
    assert!(why.contains("asks \"may I run `make build`?\"") && why.contains("y yes · n no"), "{why}");
    assert!(g.notes().iter().any(|n| n.contains("y yes · n no")), "the operator is told in the chat too");
}

// ---- answering ----

#[test]
fn yes_typed_while_an_approval_is_open_answers_it_and_the_same_task_goes_on() {
    let asker = Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Ask(approval("a", "make build")), Beat::Hold, say("built"), end("built")]]);
    let f = fx(vec![asker.clone()], |_| {});
    let Said::Started { task } = f.say("build it") else { panic!() };
    let row = f.rig.until(&task, "waiting", |r| matches!(r.state, State::Waiting { .. }));
    let State::Waiting { ask } = row.state else { unreachable!() };
    assert_eq!(f.say("yes"), Said::Answered { ask: ask.clone() });
    let done = f.rig.finished(&task);
    assert_eq!(done.answer.as_deref(), Some("built"));
    assert_eq!(asker.starts().len(), 1, "the same run went on: it was never started again");
    assert_eq!(f.count("intake.decided"), 1, "the yes was an answer, not a first line");
    assert_eq!(f.rig.of_kind("ask.answered").len(), 1);
}

#[test]
fn two_open_questions_make_a_line_that_could_answer_either_ask_which_and_send_nothing() {
    let asker = |s: &str| Scripted::new(s, vec![vec![Beat::Ask(approval("a", "make build")), Beat::Hold, end("ok")]]);
    let f = fx(vec![asker("alpha"), asker("beta")], |_| {});
    for doer in ["alpha", "beta"] {
        let mut spec = TaskSpec::new("go", doer);
        spec.thread = Some("main".into());
        f.rig.tasks.spawn(spec, Actor::Operator).unwrap();
    }
    let open = || f.rig.tasks.book().open_asks();
    for _ in 0..2000 {
        if open().len() == 2 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(open().len(), 2, "{}", f.rig.dump());
    let Said::Which(list) = f.say("yes") else { panic!("{}", f.rig.dump()) };
    assert_eq!(list.len(), 2);
    assert_eq!(open().len(), 2, "nothing was answered");
    assert!(f.notes().iter().any(|n| n.contains("Which question")));
}

#[test]
fn a_button_answers_with_the_replies_the_protocol_has_and_a_grant_is_a_yes_that_is_remembered() {
    use j4n_proto::Reply;
    let build = |local: &str| {
        let mut spec = approval(local, "make build");
        spec.act = Some(Box::new(j4n_core::gate::Action::shell(Actor::Doer("alpha".into()), "make build")));
        spec
    };
    let asker = Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Ask(build("a")), Beat::Hold, end("one")]]);
    let f = fx(vec![asker], |_| {});
    let Said::Started { task } = f.say("first") else { panic!() };
    let State::Waiting { ask } = f.rig.until(&task, "waiting", |r| matches!(r.state, State::Waiting { .. })).state else { unreachable!() };
    assert_eq!(f.front.reply("main", &ask, Reply::Pick { id: "x".into() }), Said::Refused("an approval takes yes or no".into()), "a wrong reply is refused in words");
    assert!(matches!(f.front.reply("main", &ask, Reply::Grant { scope: "forever".into() }), Said::Refused(w) if w.contains("forever") && w.contains("session")), "an unknown scope is no scope");
    assert_eq!(f.front.reply("main", &ask, Reply::Grant { scope: "session".into() }), Said::Answered { ask });
    assert_eq!(f.rig.finished(&task).answer.as_deref(), Some("one"));
}

// ---- commands ----

/// `:set` is no command, and the line went to the doer as a paid turn. A colon opens a command: a word that is none is answered here, with the
/// nearest commands, on a thread with no doer yet and on one that has it, and nothing is asked of the model or started. There is no escape for a literal
/// colon line: "send :set to the doer" has the line begin with a word.
#[test]
fn a_line_that_starts_with_a_colon_and_names_no_command_is_answered_here_and_never_sent_to_a_doer() {
    let f = fx(vec![chatty("alpha", &["a"])], |_| {});
    let said = f.say(":set check.after true");
    assert!(matches!(&said, Said::Refused(w) if w.contains("no command `:set`") && w.contains(":settings") && !w.contains(":new") && w.contains("not sent to a doer")), "{said:?}");
    assert!(f.tasks().is_empty() && f.bound().is_empty(), "nobody was elected or asked: {:?}", f.tasks());
    assert!(f.notes().last().unwrap().contains(":settings"), "the answer is in the chat too");
    assert!(matches!(f.say(":sttus"), Said::Refused(w) if w.contains(":status")), "a typo finds the command");
    assert!(matches!(f.say(":zzzzzz"), Said::Refused(w) if w.contains("no command `:zzzzzz`") && !w.contains("nearest") && w.contains(":help")));
    assert!(matches!(f.say(":"), Said::Refused(w) if w.contains("no command `:`") && !w.contains("nearest")));
    f.turn("hello");
    let before = f.tasks().len();
    assert!(matches!(f.say(":set x"), Said::Refused(w) if w.contains("no command `:set`")), "bound to a doer, the same");
    assert_eq!(f.tasks().len(), before, "no turn was started");
    // the words that are commands, in any case, are still run
    assert_eq!(f.say(":STATUS"), Said::Command("status".into()));
}

#[test]
fn the_commands_do_what_they_say_and_a_command_that_cannot_be_done_now_is_refused_in_words() {
    let hang = Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Forever]]);
    let f = fx(vec![hang, chatty("beta", &["b"])], |_| {});
    assert_eq!(f.say(":status"), Said::Command("status".into()));
    assert!(f.notes().last().unwrap().contains("not bound yet"));
    assert_eq!(f.say(":help"), Said::Command("help".into()));
    assert!(f.notes().last().unwrap().contains(":fresh"), "the help lists the table's words");
    assert!(matches!(f.say(":stop"), Said::Refused(w) if w.contains("nothing is running")));
    assert!(matches!(f.say(":switch"), Said::Refused(w) if w.contains("which doer")));
    assert!(matches!(f.say(":switch nosuch"), Said::Refused(w) if w.contains("nosuch")));
    let Said::Started { task } = f.say("hang about") else { panic!() };
    f.rig.until(&task, "running", |r| r.state == State::Running);
    assert!(matches!(f.say(":fresh"), Said::Refused(w) if w.contains(":stop")));
    assert!(matches!(f.say(":switch beta"), Said::Refused(w) if w.contains(":stop")));
    assert_eq!(f.say(":stop"), Said::Command("stop".into()));
    assert_eq!(f.rig.finished(&task).state, State::Stopped { why: Why::Aborted });
    assert_eq!(f.say(":switch beta haiku"), Said::Command("switch".into()));
    let b = f.front.binding("main").unwrap();
    assert_eq!((b.doer.as_str(), b.model.as_deref(), b.how.as_str()), ("beta", Some("haiku"), "switch"));
    let next = f.turn("now with beta");
    assert_eq!((next.spec.doer.as_str(), next.spec.tune.model.as_deref()), ("beta", Some("haiku")));
    assert!(next.spec.resume.is_none(), "another doer cannot carry on in alpha's session");
    assert_eq!(f.say(":q"), Said::Quit);
}

#[test]
fn a_model_switch_on_the_same_doer_keeps_the_session() {
    let claude = chatty("claude", &["one", "two"]);
    let f = fx(vec![claude.clone()], pinned("claude", Some("haiku")));
    f.turn("first");
    assert_eq!(f.say(":switch opus"), Said::Command("switch".into()));
    let second = f.turn("second");
    assert_eq!(second.spec.tune.model.as_deref(), Some("opus"));
    assert_eq!(claude.starts()[1].resume.as_deref(), Some("claude-session"));
}

#[test]
fn a_line_with_nothing_in_it_is_nothing_and_writes_nothing() {
    let f = fx(vec![chatty("alpha", &["a"])], |_| {});
    assert_eq!(f.say("   \t "), Said::Nothing);
    assert_eq!(f.say(""), Said::Nothing);
    assert_eq!(f.count("chat.say"), 0);
}

#[test]
fn a_line_typed_is_in_the_log_as_the_operators_whatever_became_of_it() {
    let f = fx(vec![chatty("alpha", &["a"])], |_| {});
    f.turn("  hello there  ");
    f.say(":status");
    let said: Vec<(String, String)> = f.rig.of_kind("chat.say").iter().map(|e| (e.data["text"].as_str().unwrap().to_string(), format!("{:?}", e.by))).collect();
    assert_eq!(said, [("hello there".to_string(), "Operator".to_string()), (":status".to_string(), "Operator".to_string())]);
    assert!(f.rig.of_kind("chat.say").iter().all(|e| e.thread.as_deref() == Some("main")));
}

// ---- --approval full ----

#[test]
fn an_approval_that_would_wait_for_an_operator_is_a_yes_under_full_and_the_blocked_one_stays_a_no() {
    let tables = base();
    let (pts, _) = points(&tables);
    let (gates, _) = Gates::load(&tables);
    let push = {
        let mut spec = approval("p", "git push origin main");
        spec.act = Some(Box::new(j4n_core::gate::Action::shell(Actor::Doer("alpha".into()), "git push origin main")));
        spec
    };
    let doer = Scripted::new("alpha", vec![vec![Beat::Ask(approval("a", "make build")), Beat::Hold, Beat::Ask(push), Beat::Hold, end("done")]]);
    let rig = Rig::tuned(vec![doer], |c| {
        c.asks = Some(Arc::new(Asks::new(pts, Registry::new(), Answerers::new()).with_gate(Gate::new(gates, Grants::new("s1"))).full(true)));
    });
    let id = rig.spawn("go", "alpha");
    assert_eq!(rig.finished(&id).state, State::Done { ok: true }, "{}", rig.dump());
    let said = rig.of_kind("ask.answered");
    assert_eq!(said.len(), 2, "{}", rig.dump());
    assert_eq!((said[0].data["answer"].as_str(), said[0].data["by"].as_str()), (Some("yes"), Some("rule:approval-full")));
    assert_eq!((said[1].data["answer"].as_str(), said[1].data["by"].as_str()), (Some("no"), Some("rule:gate")), "full does not loosen a block");
}

/// `line` asked of the Gate by a doer in the project `/work/proj`, as an approval that would wait for an operator, under `--approval full`: how it was answered.
fn full_answer(line: &str, grants: Grants) -> (String, String) {
    let tables = base();
    let (pts, _) = points(&tables);
    let (gates, _) = Gates::load(&tables);
    let spec = {
        let mut spec = approval("p", line);
        let mut act = j4n_core::gate::Action::shell(Actor::Doer("alpha".into()), line);
        act.site = Some("/work/proj".into());
        spec.act = Some(Box::new(act));
        spec
    };
    let doer = Scripted::new("alpha", vec![vec![Beat::Ask(spec), Beat::Hold, end("done")]]);
    let rig = Rig::tuned(vec![doer], |c| {
        c.asks = Some(Arc::new(Asks::new(pts, Registry::new(), Answerers::new()).with_gate(Gate::new(gates, grants)).full(true)));
    });
    let id = rig.spawn("go", "alpha");
    rig.finished(&id);
    let said = rig.of_kind("ask.answered");
    assert_eq!(said.len(), 1, "{}", rig.dump());
    (said[0].data["answer"].as_str().unwrap_or_default().to_string(), said[0].data["by"].as_str().unwrap_or_default().to_string())
}

#[test]
fn under_full_a_delete_inside_the_project_runs_and_a_write_or_delete_outside_it_is_asked_until_a_grant_covers_it() {
    let yes = |line: &str, g: Grants| full_answer(line, g) == ("yes".to_string(), "rule:approval-full".to_string());
    // inside: the delete the gate asks about is a yes under full
    for line in ["rm -rf ./build", "rm -rf build dist", "rm -f tie.csv messy.csv"] {
        assert!(yes(line, Grants::new("s1")), "{line}: {:?}", full_answer(line, Grants::new("s1")));
    }
    // outside: full does not answer it, and with nobody to ask the no stands
    for line in ["rm -rf ~/x", "echo x > ~/.bashrc", "cp a /etc/x", "rm -rf /tmp/x", "rm -rf ../y"] {
        let (answer, by) = full_answer(line, Grants::new("s1"));
        assert_eq!(answer, "no", "{line}");
        assert_ne!(by, "rule:approval-full", "{line}");
    }
    // with the operator's access to the folder it is inside, and full answers it as it does the rest
    let g = Grants::new("s1");
    let a = j4n_core::gate::Action::new(j4n_core::gate::Verb::Write, Actor::Doer("alpha".into()), "~/x");
    let e = g.event(&Actor::Operator, j4n_core::gate::grant::Scope::Session, &a).unwrap();
    j4n_core::fold::Fold::apply(&mut g.clone(), &j4n_core::log::Ev { seq: 1, at_ms: 1, kind: e.kind.clone(), task: None, thread: None, by: e.by.clone(), cause: None, class: Default::default(), data: e.data.clone() });
    assert!(yes("rm -rf ~/x", g.clone()), "{:?}", full_answer("rm -rf ~/x", g.clone()));
    assert!(yes("rm -rf ~/x/sub", g.clone()));
    assert_eq!(full_answer("rm -rf ~/y", g).0, "no");
}

/// A task that asks, one after the other, about each of `lines` as a doer working in `/work/proj`, under `--approval full`; how each was answered, and the Gate.
fn full_task(lines: &[&str]) -> (Vec<(String, String)>, Gate) {
    let tables = base();
    let (pts, _) = points(&tables);
    let (gates, _) = Gates::load(&tables);
    let gate = Gate::new(gates, Grants::new("s1"));
    let mut beats = Vec::new();
    for line in lines {
        let mut spec = approval("p", line);
        let mut act = j4n_core::gate::Action::shell(Actor::Doer("alpha".into()), line).task("t1");
        act.site = Some("/work/proj".into());
        spec.act = Some(Box::new(act));
        beats.extend([Beat::Ask(spec), Beat::Hold]);
    }
    beats.push(end("done"));
    let doer = Scripted::new("alpha", vec![beats]);
    let g = gate.clone();
    let rig = Rig::tuned(vec![doer], move |c| {
        c.asks = Some(Arc::new(Asks::new(pts, Registry::new(), Answerers::new()).with_gate(g).full(true)));
    });
    let id = rig.spawn("go", "alpha");
    assert_eq!(id, "t1");
    rig.finished(&id);
    let said: Vec<(String, String)> = rig.of_kind("ask.answered").iter().map(|e| (e.data["answer"].as_str().unwrap_or_default().to_string(), e.data["by"].as_str().unwrap_or_default().to_string())).collect();
    assert_eq!(said.len(), lines.len(), "{}", rig.dump());
    (said, gate)
}

#[test]
fn under_full_the_scratch_a_task_makes_below_tmp_is_its_own_and_runs_and_what_it_did_not_make_is_asked() {
    let yes = ("yes".to_string(), "rule:approval-full".to_string());
    let s = format!("/tmp/j4n-tmp-front-{}-order-scratch", std::process::id());
    let elses = format!("/tmp/j4n-tmp-front-{}-elses", std::process::id());
    std::fs::create_dir_all(&elses).unwrap();
    // a loop a doer may write: make a folder, go into it, write, and later remove it
    let (said, gate) = full_task(&[&format!("mkdir -p {s} && cd {s} && printf 'a,b\\n' > empty.csv"), &format!("rm -rf {s}"), &format!("rm -rf {elses}")]);
    assert_eq!(said[0], yes, "{said:?}");
    assert_eq!(said[1], yes, "the folder the task made, removed by the task: {said:?}");
    assert_eq!(said[2].0, "no", "a folder the task did not make is asked, and with nobody to ask the no stands: {said:?}");
    assert_ne!(said[2].1, "rule:approval-full");
    assert!(gate.owns_of("t1").is_empty(), "the Gate forgets what a task made when the task is over");
    // a bisect loop a doer may write: a folder from `mktemp -d`, the archive of each commit in it, the folder removed, in one line and in a later one
    let (said, gate) = full_task(&[
        "for c in 9a03e11 HEAD; do d=$(mktemp -d /tmp/snap.XXXX); git archive $c | tar -x -C $d; (cd $d && python3 -m unittest discover -s tests 2>&1 | tail -1); rm -rf $d; done",
        "rm -rf /tmp/snap.q7Zx",
        "rm -rf /tmp/someone-elses",
    ]);
    assert_eq!(said[0], yes, "{said:?}");
    assert_eq!(said[1].0, "no", "a mktemp is no prefix: another folder that starts like it is asked: {said:?}");
    assert_eq!(said[2].0, "no", "{said:?}");
    assert!(gate.owns_of("t1").is_empty());
    let _ = std::fs::remove_dir_all(&elses);
}

/// A doer that says what its program would be given: the task's temporary folder, and `TMPDIR` in the launch.
type Seen = Vec<(Option<std::path::PathBuf>, Option<String>, bool)>;

struct Probing(Arc<Mutex<Seen>>);

struct Idle;

impl j4n_core::doer::Run for Idle {
    fn ctl(&mut self, _: j4n_core::doer::RunCtl) -> Result<(), j4n_core::table::Problem> {
        Ok(())
    }
}

impl Doer for Probing {
    fn id(&self) -> &str {
        "probing"
    }

    fn probe(&self) -> j4n_core::doer::Probe {
        j4n_core::doer::Probe { installed: true, version: Some("probing".into()), authed: true, detail: String::new() }
    }

    fn caps(&self) -> j4n_core::doer::Caps {
        j4n_core::doer::Caps::default()
    }

    fn start(&self, cx: j4n_core::doer::StartCx) -> Result<Box<dyn j4n_core::doer::Run>, j4n_core::table::Problem> {
        let launch = cx.wrap(j4n_core::place::Launch::new(vec!["prog".into()], std::path::PathBuf::from("."), Default::default()));
        let env = launch.env.get("TMPDIR").cloned();
        let there = cx.tmp.as_ref().is_some_and(|t| t.is_dir());
        self.0.lock().unwrap().push((cx.tmp.clone(), env, there));
        let out = cx.out.clone();
        std::thread::spawn(move || out.send(j4n_core::doer::RunEv::End { answer: Some("ok".into()), todo_left: 0 }));
        Ok(Box::new(Idle))
    }
}

#[test]
fn a_task_has_a_temporary_folder_of_its_own_given_to_its_program_as_tmpdir_told_to_the_gate_and_removed_when_it_ends() {
    let tables = base();
    let (pts, _) = points(&tables);
    let (gates, _) = Gates::load(&tables);
    let gate = Gate::new(gates, Grants::new("s1"));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (g, probe) = (gate.clone(), Arc::new(Probing(seen.clone())));
    let rig = Rig::tuned(vec![], move |c| {
        c.doers.push(probe);
        c.asks = Some(Arc::new(Asks::new(pts, Registry::new(), Answerers::new()).with_gate(g)));
    });
    let id = rig.spawn("go", "probing");
    rig.finished(&id);
    let seen = seen.lock().unwrap();
    let (tmp, env, there) = seen[0].clone();
    let tmp = tmp.expect("a folder of its own");
    assert!(tmp.starts_with(std::env::temp_dir()) && tmp.ends_with(&id) && tmp.to_string_lossy().contains("/j4n-"), "{tmp:?}");
    assert_eq!(env.as_deref(), Some(tmp.to_str().unwrap()), "the program is given it as TMPDIR");
    assert!(there, "it is there when the doer starts");
    assert!(!tmp.exists(), "and removed when the task is over (J4n's to clean up)");
    assert_eq!(gate.own_tmp(&id), None, "and the Gate has forgotten it");
}

/// A doer that puts a file in its task's temporary folder and stays running until it is let go.
struct Holding(Arc<Mutex<Option<std::path::PathBuf>>>, Arc<AtomicBool>);

impl Doer for Holding {
    fn id(&self) -> &str {
        "holding"
    }

    fn probe(&self) -> j4n_core::doer::Probe {
        j4n_core::doer::Probe { installed: true, version: Some("holding".into()), authed: true, detail: String::new() }
    }

    fn caps(&self) -> j4n_core::doer::Caps {
        j4n_core::doer::Caps::default()
    }

    fn start(&self, cx: j4n_core::doer::StartCx) -> Result<Box<dyn j4n_core::doer::Run>, j4n_core::table::Problem> {
        let tmp = cx.tmp.clone().expect("a folder of its own");
        std::fs::write(tmp.join("prompt.txt"), "words").unwrap();
        *self.0.lock().unwrap() = Some(tmp);
        let (out, go) = (cx.out.clone(), self.1.clone());
        std::thread::spawn(move || {
            while !go.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            out.send(j4n_core::doer::RunEv::End { answer: Some("ok".into()), todo_left: 0 })
        });
        Ok(Box::new(Idle))
    }
}

/// Every loop numbers its tasks from `t1`, so two loops in one process (the tests of one binary) or two J4n of one user (two projects)
/// were given the same folder, and the end of one task took away the file another was reading: a canary that was given its prompt as a
/// file found none, said nothing, and failed in a gate that ran the tests of the crate together.
#[test]
fn the_temporary_folder_of_a_task_is_not_the_folder_of_the_task_of_the_same_number_in_another_loop() {
    let tables = base();
    let one = |probe: Arc<dyn Doer>| {
        let (pts, _) = points(&tables);
        let (gates, _) = Gates::load(&tables);
        Rig::tuned(vec![], move |c| {
            c.doers.push(probe);
            c.asks = Some(Arc::new(Asks::new(pts, Registry::new(), Answerers::new()).with_gate(Gate::new(gates, Grants::new("s1")))));
        })
    };
    let (held, go) = (Arc::new(Mutex::new(None)), Arc::new(AtomicBool::new(false)));
    let (first, second) = (one(Arc::new(Holding(held.clone(), go.clone()))), one(Arc::new(Probing(Arc::new(Mutex::new(Vec::new()))))));
    let a = first.spawn("go", "holding");
    let mine = loop {
        if let Some(t) = held.lock().unwrap().clone() {
            break t;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    };
    let b = second.spawn("go", "probing");
    assert_eq!(a, b, "both loops start at t1");
    second.finished(&b);
    assert!(mine.join("prompt.txt").is_file(), "the other loop's task ended and took {mine:?} with it");
    go.store(true, Ordering::SeqCst);
    first.finished(&a);
    assert!(!mine.exists(), "and it is removed when its own task is over");
}

#[test]
fn full_says_yes_to_an_approval_and_to_nothing_else() {
    let tables = base();
    let (pts, _) = points(&tables);
    let (gates, _) = Gates::load(&tables);
    let doer = Scripted::new("alpha", vec![vec![Beat::Ask(choice("c", "which one?", &[("x", false), ("y", false)])), Beat::Hold, end("done")]]);
    let rig = Rig::tuned(vec![doer], |c| {
        c.asks = Some(Arc::new(Asks::new(pts, Registry::new(), Answerers::new()).with_gate(Gate::new(gates, Grants::new("s1"))).full(true)));
    });
    let id = rig.spawn("go", "alpha");
    rig.finished(&id);
    let said = rig.of_kind("ask.answered");
    assert_eq!(said.len(), 1, "{}", rig.dump());
    assert_ne!(said[0].data["by"].as_str(), Some("rule:approval-full"), "{}", rig.dump());
}

#[test]
fn a_line_answers_the_questions_of_its_own_thread_only() {
    let asker = Scripted::new("alpha", vec![vec![Beat::Ask(approval("a", "make build")), Beat::Hold, end("ok")]]);
    let f = fx(vec![asker, chatty("beta", &["hello"])], pinned("beta", None));
    let mut spec = TaskSpec::new("go", "alpha");
    spec.thread = Some("other".into());
    let id = f.rig.tasks.spawn(spec, Actor::Operator).unwrap();
    f.rig.until(&id, "waiting", |r| matches!(r.state, State::Waiting { .. }));
    assert!(matches!(f.say("yes"), Said::Started { .. }), "a yes on the screen's thread is a line for the doer, not an answer to the other thread");
    assert_eq!(f.rig.tasks.book().open_asks().len(), 1, "{}", f.rig.dump());
    f.rig.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
}

/// An answerer that takes its time and then has no opinion: the Gate's walk is behind it.
struct Slow;

impl Answerer for Slow {
    fn id(&self) -> &str {
        "rule:slow"
    }

    fn kind(&self) -> Kind {
        Kind::Rule
    }

    fn offer(&self, _q: &Question) -> Offer {
        std::thread::sleep(std::time::Duration::from_millis(500));
        Offer::Pass { why: "slow".into() }
    }
}

#[test]
fn an_operators_yes_to_what_the_gate_blocks_is_refused_even_before_the_chain_has_walked() {
    let tables = base();
    let (mut pts, _) = points(&tables);
    let (gates, _) = Gates::load(&tables);
    for p in pts.iter_mut().filter(|p| p.id == "approval") {
        p.chain.insert(0, Step { kind: Kind::Rule, name: "slow".into(), min_conf: None, after_ms: None });
    }
    let mut registry = Registry::new();
    registry.register(Arc::new(Slow));
    let mut push = approval("p", "git push origin main");
    push.act = Some(Box::new(j4n_core::gate::Action::shell(Actor::Doer("alpha".into()), "git push origin main")));
    let doer = Scripted::new("alpha", vec![vec![Beat::Ask(push), Beat::Hold, end("done")]]);
    let rig = Rig::tuned(vec![doer], |c| {
        c.asks = Some(Arc::new(Asks::new(pts, registry, Answerers::new()).with_gate(Gate::new(gates, Grants::new("s1")))));
    });
    let id = rig.spawn("go", "alpha");
    let State::Waiting { ask } = rig.until(&id, "waiting", |r| matches!(r.state, State::Waiting { .. })).state else { unreachable!() };
    // the question is open and the chain is still walking: a quick yes (a piped line) must not get past the Gate
    let refused = rig.tasks.ctl(&ask, Ctl::Answer(j4n_core::ask::Answer::Yes), Actor::Operator).unwrap_err();
    assert!(refused.msg.contains("Gate blocks"), "{refused:?}");
    assert_eq!(rig.finished(&id).state, State::Done { ok: true }, "{}", rig.dump());
    let said = rig.of_kind("ask.answered");
    assert_eq!((said[0].data["answer"].as_str(), said[0].data["by"].as_str()), (Some("no"), Some("rule:gate")), "{}", rig.dump());
}

// ---- the doers that fail, and the loop that goes away ----

#[test]
fn a_task_that_ends_with_nothing_is_a_failure_the_chat_can_say_and_the_next_line_still_works() {
    let f = fx(vec![Scripted::new("alpha", vec![vec![Beat::Session("s".into()), nothing()], vec![Beat::Session("s".into()), end("fine")]])], |_| {});
    let first = f.turn("do something");
    assert_eq!(first.state, State::Done { ok: false });
    let second = f.turn("try again");
    assert_eq!(second.answer.as_deref(), Some("fine"));
    assert_eq!(f.bound().len(), 1);
}

#[test]
fn a_doer_that_cannot_start_stops_the_task_and_the_thread_stays_bound() {
    let f = fx(vec![Scripted::broken("alpha", "no such program")], |_| {});
    let Said::Started { task } = f.say("hello") else { panic!("{}", f.rig.dump()) };
    let row = f.rig.finished(&task);
    assert!(matches!(row.state, State::Stopped { why: Why::Failed { .. } }), "{:?}", row.state);
    assert!(f.front.binding("main").is_some());
}

// ---- what the front gives the task ----

#[test]
fn the_mode_and_the_spend_limit_the_front_is_set_to_are_the_tasks() {
    let f = fx(vec![chatty("alpha", &["a", "b"])], |c| {
        c.mode = j4n_core::task::Mode::Read;
        c.usd_soft = Some(2.5);
    });
    for line in ["look", "look again"] {
        let row = f.turn(line);
        assert_eq!((row.spec.tune.mode, row.spec.limits.usd_soft), (j4n_core::task::Mode::Read, Some(2.5)), "{line}");
    }
    let g = fx(vec![chatty("alpha", &["a"])], |_| {});
    let row = g.turn("write");
    assert_eq!((row.spec.tune.mode, row.spec.limits.usd_soft, row.spec.limits.wall_ms), (j4n_core::task::Mode::Write, None, None));
}

#[test]
fn a_doer_that_reported_no_session_has_nothing_to_carry_on_in_and_the_next_task_does_not_pretend_to() {
    let f = fx(vec![Scripted::new("alpha", vec![vec![say("one"), end("one")], vec![say("two"), end("two")]])], |_| {});
    f.turn("first");
    let second = f.turn("second");
    assert_eq!((second.spec.follows, second.spec.resume), (None, None));
}

#[test]
fn a_wall_given_to_the_front_is_what_is_left_of_it_for_each_task() {
    let f = fx(vec![chatty("alpha", &["a", "b"])], |c| c.deadline = Some(std::time::Instant::now() + std::time::Duration::from_secs(600)));
    let a = f.turn("first").spec.limits.wall_ms.expect("a wall");
    std::thread::sleep(std::time::Duration::from_millis(60));
    let b = f.turn("second").spec.limits.wall_ms.expect("a wall");
    assert!(a <= 600_000 && a > 590_000, "{a}");
    assert!(b < a - 40, "the second task has what the first left: {a} then {b}");
}

#[test]
fn what_intake_is_asked_is_the_line_the_thread_and_five_answers_with_task_recommended() {
    let f = fx(vec![chatty("alpha", &["a"])], |_| {});
    f.turn("what is in here?");
    let asked: Vec<_> = f.asked.lock().unwrap().iter().filter(|q| q.point == "intake").cloned().collect();
    assert_eq!(asked.len(), 1);
    let q = &asked[0];
    assert_eq!((q.point.as_str(), q.asker.as_str(), q.thread.as_deref(), q.text.as_str(), q.goal.as_str()), ("intake", "front", Some("main"), "what is in here?", "what is in here?"));
    let j4n_core::ask::AskShape::Choice { options } = &q.shape else { panic!("{q:?}") };
    assert_eq!(options.iter().map(|o| (o.id.as_str(), o.recommended)).collect::<Vec<_>>(), [("answer", false), ("command", false), ("setting", false), ("task", true), ("flow", false)]);
}

#[test]
fn an_intake_that_is_not_a_task_is_said_in_the_chat_and_the_line_still_goes_to_a_doer() {
    let f = fx_deciding(vec![chatty("alpha", &["done"])], Some("intake=setting"), |_| {});
    let row = f.turn("set the theme to dark");
    assert_eq!(row.answer.as_deref(), Some("done"));
    assert!(f.notes().iter().any(|n| n.contains("intake took this for `setting`") && n.contains("not built yet")), "{:?}", f.notes());
    let e = f.rig.of_kind("intake.decided");
    assert_eq!((e[0].data["pick"].as_str(), e[0].data["by"].as_str()), (Some("setting"), Some("decider:jev")));
    // a `flow` begins a plan where something keeps one: the line is a planning turn, and what it answers is checked
    let p = fx_deciding(vec![chatty("alpha", &["done"])], Some("intake=flow"), |c| c.lanes = j4n_core::branch::Switch::Auto);
    let row = p.turn("plan the move to rust");
    assert_eq!(row.spec.role.as_deref(), Some("plan"));
    // a task intake says nothing of itself
    let g = fx_deciding(vec![chatty("alpha", &["done"])], Some("task"), |_| {});
    g.turn("hello");
    assert!(g.notes().iter().all(|n| !n.contains("intake took")));
}

// ---- who is elected ----

#[test]
fn only_a_doer_that_is_installed_and_logged_in_is_a_candidate() {
    let turn = || vec![vec![Beat::Session("s".into()), say("x"), end("x")]];
    let f = fx(
        vec![Scripted::probed("a-not-logged-in", true, false, turn()), Scripted::probed("b-not-installed", false, true, turn()), Scripted::probed("c-ready", true, true, turn())],
        |_| {},
    );
    f.turn("hello");
    assert_eq!(f.front.binding("main").map(|b| (b.doer, b.how)), Some(("c-ready".into(), "elected".into())), "the smaller ids were not candidates");
    let g = fx(vec![Scripted::probed("a", true, false, turn()), Scripted::probed("b", false, true, turn())], |_| {});
    assert!(matches!(g.say("hello"), Said::Refused(w) if w.contains("no doer is installed, logged in and good for `work`")));
}

#[test]
fn with_no_point_to_walk_the_first_doer_that_is_ready_is_the_one() {
    let f = fx_without(vec![chatty("alpha", &["a"]), chatty("beta", &["b"])], &["who:work", "intake"], |_| {});
    f.turn("hello");
    assert_eq!(f.front.binding("main").map(|b| b.doer), Some("alpha".into()));
    assert_eq!(f.count("intake.decided"), 1, "an intake with no point is still written, as a task");
    assert_eq!(f.rig.of_kind("intake.decided")[0].data["by"], "front");
}

// ---- the buttons and what they leave in the Log ----

#[test]
fn a_yes_by_button_is_a_yes_and_a_grant_by_button_is_a_grant_of_the_scope_named() {
    use j4n_proto::Reply;
    let build = |local: &str| {
        let mut spec = approval(local, "make build");
        spec.act = Some(Box::new(j4n_core::gate::Action::shell(Actor::Doer("alpha".into()), "make build")));
        spec
    };
    let doer = Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Ask(build("a")), Beat::Hold, end("one")], vec![Beat::Ask(build("b")), Beat::Hold, end("two")], vec![Beat::Ask(build("c")), Beat::Hold, end("three")]]);
    let f = fx(vec![doer], |_| {});
    let ask_of = |task: &str| match f.rig.until(task, "waiting", |r| matches!(r.state, State::Waiting { .. })).state {
        State::Waiting { ask } => ask,
        _ => unreachable!(),
    };
    let Said::Started { task } = f.say("one") else { panic!() };
    let ask = ask_of(&task);
    f.front.reply("main", &ask, Reply::Yes);
    f.rig.finished(&task);
    let said = &f.rig.of_kind("ask.answered")[0].data;
    assert_eq!(said["answer"], "yes");
    // an approval says what it was for, so the same yes can be counted (pack `grow`): the action, the id of the row that would remember it
    assert_eq!((said["verb"].as_str(), said["target"].as_str(), said["action"].as_str()), (Some("shell"), Some("make build"), Some("shell:make build")));
    assert!(said["action_id"].as_str().is_some_and(|id| id.starts_with("here-")) && said["grantable"] == true, "{said}");
    let Said::Started { task } = f.say("two") else { panic!() };
    let ask = ask_of(&task);
    assert_eq!(f.front.reply("main", &ask, Reply::Grant { scope: "session".into() }), Said::Answered { ask });
    f.rig.finished(&task);
    let grants = f.rig.of_kind("grant");
    assert_eq!(grants.iter().map(|g| g.data["scope"].as_str().unwrap()).collect::<Vec<_>>(), ["session"], "{}", f.rig.dump());
    // the third asks the same and is answered by the grant: nobody is asked
    let Said::Started { task } = f.say("three") else { panic!() };
    assert_eq!(f.rig.finished(&task).answer.as_deref(), Some("three"));
}

#[test]
fn the_task_a_line_carries_on_from_is_the_newest_of_the_thread() {
    let f = fx(vec![chatty("alpha", &["a", "b", "c", "d"])], |_| {});
    let rows: Vec<Row> = ["one", "two", "three", "four"].iter().map(|l| f.turn(l)).collect();
    for i in 1..4 {
        assert_eq!(rows[i].spec.follows.as_deref(), Some(rows[i - 1].id.as_str()), "turn {i}");
    }
}
