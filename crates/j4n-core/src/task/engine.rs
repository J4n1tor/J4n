//! The loop. One thread owns every live task: doers push what they say into its inbox (through their [`Emit`]), the timer
//! thread's `timer.fired` events arrive there too, and so do the operator's orders. It handles one message at a time, so a
//! task has exactly one state at any moment, and every change of state is an event before it is anything else — the
//! [`Book`] (a fold) is the state, and a restart finds it in the Log.
//!
//! What the loop does that no pack can undo (law 9): it kills a task past the hard spend cap, stops one whose wall has
//! passed (with its report, after flushing the Log), parks one whose question nobody answered, never lets an approval be
//! answered by a model, holds a question open until it is answered, and calls a task that said nothing and did nothing
//! failed. The answer of a task is the report that settled last.
//!
//! Do not call [`Tasks`] from a doer's `start` or from a sink: both run where the answer would have to come from.

use super::asks::{Asks, FULL, GATE, grant_in};
use super::book::{Book, queued};
use super::context;
use super::{Act, Changed, Ctl, HARD_FACTOR, HUMAN, Item, Limits, Mode, SessionRef, State, TaskSpec, Tune, Why, menu, micros};
use crate::answer::chain::{Ledger, Note, Outcome, Spent, Walk, fallback, pair_json, walk};
use crate::answer::{AskPoint, Kind, Question};
use crate::ask::{Answer, AskShape, AskSpec};
use crate::change::Actor;
use crate::compact::{MustKeep, clip_notes};
use crate::cost::{self, Plans};
use crate::doer::{Caps, Doer, Emit, Limit, Metrics, Role, Run, RunCtl, RunEv, Signal, StartCx};
use crate::failure::{Class, Failures};
use crate::fold::Folds;
use crate::gate::Verdict;
use crate::gate::grant::{self, Scope};
use crate::changes::LogJournal;
use crate::clock::Clock;
use crate::log::{Ev, Log, New, Sink};
use crate::place::checkpoint::{self, Cfg, RewindReport};
use crate::proposal::{self, Proposals};
use crate::place::{Here, Place, Prepare, Settled, Take, Taken, Via};
use crate::rule::{Act as RuleAct, Do, Due, RuleEnv, Rules, State as RuleState};
use crate::setting::Settings;
use crate::table::Problem;
use crate::timer::{Timer, Timers};
use serde_json::{Value as Json, json};
use std::collections::{HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::thread::JoinHandle;

/// How long a question waits for the operator before its task is parked.
pub(crate) const ASK_WAIT_MS: u64 = 30 * 60_000;
/// The streamed words kept for a report: the last this many bytes.
const TAIL_MAX: usize = 1 << 20;
/// The answer a settled task carries is its last this many bytes, fewer when its escapes make the line longer than [`ANSWER_LINE`]: a longer line would be
/// too long for the Log, which keeps such a line in a blob the book cannot read, and the task would end with nothing. The whole text is in the `run.message`
/// that said it.
const ANSWER_MAX: usize = 800_000;
const ANSWER_LINE: usize = 900_000;
/// A shell command's output is kept in the Log as its last this many bytes.
const SHELL_OUT_MAX: usize = 32 << 10;
const WALL: &str = "task.wall";
const WARN: &str = "task.wall_warn";
const DEADLINE: &str = "ask.deadline";
const AFTER: &str = "ask.after";
const COOL: &str = "task.cool";
const RULE: &str = "rule.after";
/// When the warning of the wall comes, as a part of the wall, and the shortest wall that has one (setting defaults).
const WARN_AT: f64 = 0.88;
const WARN_MIN_MS: u64 = 120_000;

/// What a doer is told when `left_ms` of its time is all there is: the files the task asks for first, then the rest.
pub(crate) fn wall_warning(left_ms: u64) -> String {
    let secs = left_ms / 1000;
    let left = if secs >= 120 { format!("about {} minutes", (secs + 30) / 60) } else { format!("about {secs} seconds") };
    format!("The time for this task is nearly up: {left} left, then it is stopped. Write the files and results the task asks for now, in the state they are in, and say what is done and what is not. Do not start anything new.")
}

/// What a task that waited out a limit is told when it goes on.
pub const CONTINUE: &str = "Carry on where you left off.";
/// The longest a task waits out a limit when the setting `cool.max` is not there.
const COOL_MAX_MS: u64 = 6 * 3_600_000;

/// What the loop is made of.
pub struct Config {
    pub log: Log,
    pub folds: Folds,
    pub timers: Arc<Timers>,
    pub doers: Vec<Arc<dyn Doer>>,
    /// Places by name; `here` is there unless replaced.
    pub places: Vec<Arc<dyn Place>>,
    /// The project's folder.
    pub root: PathBuf,
    /// J4n's own home (`~/.j4n`): checkpoints are filed there.
    pub home: PathBuf,
    pub ask_wait_ms: u64,
    /// Who may answer a question besides the operator; none: every question goes to them.
    pub asks: Option<Arc<Asks>>,
    /// What reacts to what happens, what failures are told apart by, and the numbers the rules read. None of them: the loop
    /// does what no rule can undo and nothing more — a task is not carried on, parked at its cap or waited out.
    pub rules: Arc<Rules>,
    pub failures: Arc<Failures>,
    /// What the connectors' subscriptions are, for the plan cost of a request; none: the list cost alone.
    pub plans: Arc<Plans>,
    pub settings: Settings,
    /// The live handles on the settings and the plans: an edit swaps what the loop reads. None: `settings` and
    /// `plans` as they are, for as long as the loop runs.
    pub live: Option<crate::setting::Live>,
    pub live_plans: Option<crate::cost::LivePlans>,
    /// Where a rule's `propose` is looked up before it is written: what was declined or accepted does not come back. None: a
    /// rule's proposal is always written.
    pub proposals: Option<Proposals>,
    /// What a launch becomes for the account a task runs as, before the place's walls are put round it:
    /// its environment and its proxy, and the folders its login is kept in.
    pub launch: Option<Launcher>,
}

/// Turns a launch into the same program run as an account; the problem is why it cannot be (`Err` ends the run's start).
pub type Launcher = Arc<dyn Fn(&TaskSpec, crate::place::Launch) -> Result<crate::place::Launch, String> + Send + Sync>;

impl Config {
    pub fn new(log: &Log, folds: &Folds, timers: &Arc<Timers>, root: &Path, home: &Path) -> Config {
        Config {
            log: log.clone(),
            folds: folds.clone(),
            timers: timers.clone(),
            doers: Vec::new(),
            places: vec![Arc::new(Here::new(home, root, Cfg::default()))],
            root: root.to_path_buf(),
            home: home.to_path_buf(),
            ask_wait_ms: ASK_WAIT_MS,
            asks: None,
            rules: Arc::default(),
            failures: Arc::default(),
            plans: Arc::default(),
            settings: Settings::default(),
            live: None,
            live_plans: None,
            proposals: None,
            launch: None,
        }
    }
}

enum Msg {
    Spawn(Box<TaskSpec>, Actor, Sender<Result<String, Problem>>),
    Ctl(String, Ctl, Actor, Sender<Result<(), Problem>>),
    Resume(String, Option<String>, Actor, Sender<Result<String, Problem>>),
    Run(String, u32, Signal),
    Timer(Arc<Ev>),
    /// A walk of an ask's chain is over: the ask, and what it came to (`None`: the walk panicked).
    Walked(String, Option<(Walk, Vec<Note>)>),
    /// An event some rule answers.
    Event(Arc<Ev>),
    /// Everything ahead of this has been handled.
    Sync(Sender<Result<(), Problem>>),
    /// A run said it failed, and the rules have had their say (they come first in the inbox): the task, the run, the words and
    /// the kind of failure. A task still running then fails.
    Failed(String, u32, String, Class),
    Stop,
}

/// Hands the loop the events it reacts to: its timers, and what the rules answer.
struct Relay {
    to: Sender<Msg>,
    rules: Arc<Rules>,
}

const TIMERS: [&str; 6] = [WALL, WARN, DEADLINE, AFTER, COOL, RULE];

impl Sink for Relay {
    fn event(&mut self, ev: &Ev) {
        if ev.kind == "timer.fired" && ev.data.get("tag").and_then(Json::as_str).is_some_and(|t| TIMERS.contains(&t)) {
            let _ = self.to.send(Msg::Timer(Arc::new(ev.clone())));
        }
        // what a rule says about its own refusals is not something to answer: that is how a storm feeds itself
        if ev.kind != "rule.dropped" && self.rules.wants(&ev.kind) {
            let _ = self.to.send(Msg::Event(Arc::new(ev.clone())));
        }
    }
}

struct Shared {
    tx: Sender<Msg>,
    book: Book,
    log: Log,
    root: PathBuf,
    home: PathBuf,
    places: HashMap<String, Arc<dyn Place>>,
    join: Mutex<Option<JoinHandle<()>>>,
    /// Set when the loop is going: what listens for flashes stops.
    down: Arc<AtomicBool>,
    /// What `:calibrate` sizes a request from.
    plans: crate::cost::LivePlans,
    folds: Folds,
    settings: crate::setting::Live,
    /// The doers that run code and no model (`Doer::code`), by id.
    code: HashSet<String>,
}

impl Drop for Shared {
    fn drop(&mut self) {
        self.down.store(true, Ordering::SeqCst);
        let _ = self.tx.send(Msg::Stop);
        if let Some(handle) = self.join.lock().ok().and_then(|mut j| j.take()) {
            let _ = handle.join();
        }
    }
}

/// A handle to the loop. Clones share it; the loop stops, and stops its tasks, when the last is dropped.
#[derive(Clone)]
pub struct Tasks(Arc<Shared>);

/// A handle that does not keep the loop alive: for what the loop itself holds (a flow doer), which would otherwise hold it up.
#[derive(Clone)]
pub struct WeakTasks(std::sync::Weak<Shared>);

impl WeakTasks {
    pub fn upgrade(&self) -> Option<Tasks> {
        self.0.upgrade().map(Tasks)
    }
}

/// The folder of a loop below the user's: its process and its number among the loops of the process (the tests of one binary run many).
fn tmp_tag() -> String {
    static LOOPS: AtomicU64 = AtomicU64::new(0);
    format!("{}-{}", std::process::id(), LOOPS.fetch_add(1, Ordering::Relaxed))
}

fn refused(at: &str, msg: impl Into<String>) -> Problem {
    Problem::refused(at, msg)
}

/// Flashed events are not written, so the Log's sinks never see them: a rule that answers one listens here.
fn listen_for_flashes(log: &Log, rules: &Arc<Rules>, to: Sender<Msg>, down: Arc<AtomicBool>) {
    let (sub, rules) = (log.subscribe("*", 1024), rules.clone());
    let _ = std::thread::Builder::new().name("j4n-flashes".into()).spawn(move || {
        while !down.load(Ordering::SeqCst) {
            if let Some(ev) = sub.recv_timeout(Duration::from_millis(100))
                && ev.seq == 0
                && rules.wants(&ev.kind)
                && to.send(Msg::Event(ev)).is_err()
            {
                return;
            }
        }
    });
}

impl Tasks {
    /// Start the loop. Tasks the Log shows as unfinished belonged to a process that is gone: they are stopped as
    /// interrupted, and the problems say so.
    pub fn start(cfg: Config) -> Result<(Tasks, Vec<Problem>), Problem> {
        let book = Book::new();
        let mut problems = cfg.folds.add(Box::new(book.clone()), cfg.log.head());
        if let Some(a) = &cfg.asks {
            problems.extend(cfg.folds.add(Box::new(a.state.clone()), cfg.log.head()));
            if let Some(g) = &a.gate {
                problems.extend(cfg.folds.add(Box::new(g.grants().clone()), cfg.log.head()));
            }
        }
        problems.extend(cfg.folds.catch_up(&cfg.log));
        let (tx, rx) = mpsc::channel();
        let places: HashMap<String, Arc<dyn Place>> = cfg.places.into_iter().map(|p| (p.id().to_string(), p)).collect();
        let doers = doers(cfg.doers, &mut problems);
        let code: HashSet<String> = doers.iter().filter(|(_, d)| catch_unwind(AssertUnwindSafe(|| d.code())).unwrap_or(false)).map(|(id, _)| id.clone()).collect();
        let live = cfg.live.clone().unwrap_or_else(|| crate::setting::Live::new(cfg.settings.clone()));
        let plans = cfg.live_plans.clone().unwrap_or_else(|| crate::cost::LivePlans::of(cfg.plans.clone()));
        let mut engine = Engine {
            log: cfg.log.clone(),
            book: book.clone(),
            timers: cfg.timers,
            tx: tx.clone(),
            doers,
            places: places.clone(),
            root: cfg.root.clone(),
            home: cfg.home.clone(),
            asks: cfg.asks.clone(),
            ledgers: HashMap::new(),
            clock: cfg.log.clock(),
            ask_wait_ms: cfg.ask_wait_ms.max(1),
            next: book.next_number(),
            tmp_tag: tmp_tag(),
            live: HashMap::new(),
            failed: None,
            folds: cfg.folds.clone(),
            settings: live.clone(),
            rules: cfg.rules.clone(),
            failures: cfg.failures.clone(),
            plans: plans.clone(),
            proposals: cfg.proposals.clone(),
            launch: cfg.launch.clone(),
            rstate: RuleState::default(),
            acting: None,
            missing: HashSet::new(),
        };
        for row in book.open() {
            engine.put(&row.id, "task.stopped", Actor::Core, json!({ "why": Why::Interrupted }));
            problems.push(Problem::warn(format!("task {}", row.id), "was running when J4n stopped; it is stopped as interrupted"));
        }
        if let Some(p) = engine.failed.take() {
            return Err(p);
        }
        cfg.log.attach(Box::new(Relay { to: tx.clone(), rules: cfg.rules.clone() }), cfg.log.head())?;
        let down = Arc::new(AtomicBool::new(false));
        if cfg.rules.wants_flash() {
            listen_for_flashes(&cfg.log, &cfg.rules, tx.clone(), down.clone());
        }
        let join = std::thread::Builder::new()
            .name("j4n-tasks".into())
            .spawn(move || engine.run(rx))
            .map_err(|e| refused("tasks", format!("cannot start the loop: {e}")))?;
        let shared = Shared { tx, book, log: cfg.log, root: cfg.root, home: cfg.home, places, join: Mutex::new(Some(join)), down, plans, folds: cfg.folds, settings: live, code };
        Ok((Tasks(Arc::new(shared)), problems))
    }

    fn ask<T>(&self, make: impl FnOnce(Sender<Result<T, Problem>>) -> Msg) -> Result<T, Problem> {
        let (reply, answer) = mpsc::channel();
        let down = || refused("tasks", "the loop has stopped");
        self.0.tx.send(make(reply)).map_err(|_| down())?;
        answer.recv().map_err(|_| down())?
    }

    /// The settings as they are now (`/context` reads the size of a typical line from them).
    pub fn settings(&self) -> Arc<crate::setting::Snap> {
        self.0.settings.now()
    }

    /// The fold host the loop reads and the rules read (a reflex's check reads `fold.…`).
    pub fn folds(&self) -> &Folds {
        &self.0.folds
    }

    /// Whether the doer runs code and no model (`Doer::code`): a flow gives it no Brief.
    pub fn code(&self, doer: &str) -> bool {
        self.0.code.contains(doer)
    }

    /// The plans there are and the connectors that show windows (`/usage` reads each account against its own).
    pub fn plans(&self) -> Arc<Plans> {
        self.0.plans.now()
    }

    /// What a request has to cost to make a pair for the plan of `doer` run as `account` (`:calibrate`), or why none can be sized.
    pub fn calibration(&self, doer: &str, account: Option<&str>) -> Result<cost::Calibration, String> {
        let s = &self.0;
        if !s.settings.flag("cost.plan.on", true) {
            return Err("`cost.plan.on` is off: nothing reads the windows".into());
        }
        let plans = s.plans.now();
        if plans.source(doer).is_some_and(|c| c.windows.is_none()) {
            return Err(format!("{doer} shows no window: a request on it is priced by its list cost"));
        }
        let plan = plans.of(doer, account).ok_or_else(|| format!("{doer} has no plan: a `[[plan]]` row names the connector and its windows"))?;
        let (now, acct) = (s.log.clock().now_ms(), account.unwrap_or(""));
        let size = s.settings.num("cost.calibrate.size", 3000.0) as u32;
        plan.calibration(|w| cost::rate(&s.folds, doer, acct, w, now), |w| cost::anchored(&s.folds, doer, acct, w, now))
            .map(|c| cost::Calibration { size, ..c })
            .ok_or_else(|| "nothing is measured and the plan has no `assume`: there is nothing to size a request from".into())
    }

    /// The windows the providers showed and that have not reset yet: where each stands, from the Log's `run.window` rows.
    pub fn windows(&self) -> Vec<cost::WindowNow> {
        cost::windows(&self.0.folds, self.0.log.clock().now_ms())
    }

    /// Queue a task and start it. Returns its id.
    pub fn spawn(&self, spec: TaskSpec, by: Actor) -> Result<String, Problem> {
        self.ask(|r| Msg::Spawn(Box::new(spec), by, r))
    }

    /// Tell a task something: steer it, answer its question (the task is the Ask), stop it, kill it, retune it.
    pub fn ctl(&self, task: &str, ctl: Ctl, by: Actor) -> Result<(), Problem> {
        self.ask(|r| Msg::Ctl(task.to_string(), ctl, by, r))
    }

    /// Return once the loop has handled everything that was written before the call, the rules' answers to it among them: a task
    /// that has just settled has by then been carried on, or never will be.
    pub fn sync(&self) -> Result<(), Problem> {
        // what the Log has taken is on its way to the loop from the writer's thread: it is let get there before the barrier is set
        self.0.log.sync();
        self.ask(Msg::Sync)
    }

    /// Carry on after a task that is over: a new task on the same doer, session and thread, with `text` as its words (the
    /// original goal when none). Returns the new task's id.
    pub fn resume(&self, task: &str, text: Option<&str>, by: Actor) -> Result<String, Problem> {
        self.ask(|r| Msg::Resume(task.to_string(), text.map(str::to_string), by, r))
    }

    /// The project's folder as it stands, in one word ([`checkpoint::stamp`]): a check that ran on the same word ran on the same files.
    pub fn stamp(&self) -> String {
        checkpoint::stamp(&self.0.home, &self.0.root)
    }

    /// Put the project's folder back as it was before `task` (the newest one that settled when `None`).
    pub fn rewind(&self, task: Option<&str>, force: bool) -> Result<RewindReport, Problem> {
        if let Some(row) = task.and_then(|t| self.0.book.get(t))
            && !row.state.is_terminal()
        {
            return Err(refused(&row.id, "it is still going — stop it first"));
        }
        let report = checkpoint::rewind(&self.0.home, &self.0.root, task, force).map_err(|m| refused("rewind", m))?;
        let data = json!({
            "restored": report.restored.len(),
            "removed": report.removed.len(),
            "skipped": report.skipped.len(),
            "not_restorable": report.not_restorable.len(),
        });
        let laid = self.0.book.get(&report.task).filter(|r| r.applied.is_some() && r.laid.is_some()).and_then(|r| self.0.places.get(&r.spec.place).cloned());
        self.0.log.append(New::of("task.rewound", Actor::Operator, data).task(&report.task))?;
        // its work is held again where its row says: a place that removed its copy at the lay-in cuts it again now, not at the next take
        if let Some(place) = laid {
            let _ = catch_unwind(AssertUnwindSafe(|| place.take(&report.task, Take::Look)));
        }
        Ok(report)
    }

    /// Say that a task that is over (and was interrupted) left work in a place of its own, which a restart found there. False when
    /// the row says so already, or the task is not over.
    pub fn keep(&self, task: &str, kept: crate::place::Kept) -> bool {
        let Some(row) = self.0.book.get(task).filter(|r| r.state.is_terminal() && r.kept.is_none() && r.applied.is_none()) else { return false };
        self.0.log.append(New::of("task.kept", Actor::Core, json!(kept)).task(&row.id)).is_ok()
    }

    /// Look at the work a task left in a place of its own, or bring it to the project's folder (`rewind` takes it out again).
    pub fn take(&self, task: &str, how: Take) -> Result<Taken, Problem> {
        let row = self.0.book.get(task).ok_or_else(|| refused(task, "no such task"))?;
        if !row.state.is_terminal() {
            return Err(refused(&row.id, "it is still going — stop it first"));
        }
        let Some(kept) = row.kept else { return Err(refused(&row.id, "it left no work in a place of its own")) };
        let place = self.0.places.get(&row.spec.place).ok_or_else(|| refused(&row.id, format!("no place called `{}`", row.spec.place)))?;
        let taken = catch_unwind(AssertUnwindSafe(|| place.take(&row.id, how))).map_err(|_| refused(&row.id, "the place failed while it was asked"))??;
        if let (Take::Apply, Taken::Clean(work, notes)) = (how, &taken) {
            self.0.log.append(New::of("task.applied", Actor::Operator, json!({ "work": work, "was": kept.dir, "notes": notes })).task(&row.id))?;
        }
        Ok(taken)
    }

    /// Put a share into the site of `task` ([`Place::share`]): refused where its place takes none.
    pub fn share(&self, task: &str, rel: &str, from: &Path) -> Result<crate::place::Shared, String> {
        let row = self.0.book.get(task).ok_or("no such task")?;
        let place = self.0.places.get(&row.spec.place).ok_or_else(|| format!("no place called `{}`", row.spec.place))?;
        catch_unwind(AssertUnwindSafe(|| place.share(&row.id, rel, from))).map_err(|_| "the place failed while it was asked".to_string())?
    }

    /// The folder of the site of `task`, while its place still holds it.
    pub fn site_dir(&self, task: &str) -> Option<PathBuf> {
        let row = self.0.book.get(task)?;
        let place = self.0.places.get(&row.spec.place)?;
        catch_unwind(AssertUnwindSafe(|| place.site_of(&row.id))).ok().flatten().map(|s| s.dir)
    }

    /// Whether a place of that name is registered.
    pub fn has_place(&self, name: &str) -> bool {
        self.0.places.contains_key(name)
    }

    pub fn downgrade(&self) -> WeakTasks {
        WeakTasks(Arc::downgrade(&self.0))
    }

    /// The project's folder, where a task in place `here` works.
    pub fn root(&self) -> &Path {
        &self.0.root
    }

    /// The fold of tasks: `get`, `rows`, `children`, `wait`.
    pub fn book(&self) -> &Book {
        &self.0.book
    }

    /// What the operator may do about the stop `task` is in.
    pub fn menu(&self, task: &str) -> Vec<Item> {
        let Some(row) = self.0.book.get(task) else { return Vec::new() };
        let State::Stopped { why } = row.state else { return Vec::new() };
        let ask = match &why {
            Why::Parked { ask } => self.0.book.get(ask).and_then(|r| r.spec.ask()),
            _ => None,
        };
        let mut items = menu(&why, ask.as_ref());
        // a limit passes and a login does not: what the failure was decides which way is the likely one
        match (&why, row.class.as_deref()) {
            (Why::Failed { .. }, Some("rate-limit")) => {
                if let Some(i) = items.iter_mut().find(|i| i.act == Act::Retry) {
                    (i.id, i.label, i.act) = ("resume".into(), "carry on once the limit is over".into(), Act::Resume);
                }
            }
            (Why::Failed { .. }, Some("permanent" | "refused")) => items.iter_mut().for_each(|i| i.recommended = false),
            _ => {}
        }
        items
    }
}

struct OpenAsk {
    id: String,
    spec: AskSpec,
    /// The deadline after which the asker is parked.
    timer: Option<u64>,
    /// The point and the question the chain was walked for.
    walked: Option<(AskPoint, Question)>,
    /// After this the point's fallback stands in for the operator.
    after: Option<u64>,
    /// `approval.inside`: the Decider of the point `inside` was put the line, and what the approval's chain said waits for its answer.
    inside: Inside,
}

/// Where the Decider of `approval.inside` is with an approval.
#[derive(Default)]
enum Inside {
    #[default]
    NotPut,
    /// Put; what the approval's own chain said stands if the Decider does not say `inside`.
    Put(Walk),
    Done,
}

struct Live {
    epoch: u32,
    run: Option<Box<dyn Run>>,
    doer: Arc<dyn Doer>,
    /// What the doer called itself and could do when the task started; a doer is asked once.
    doer_id: String,
    caps: Caps,
    place: Arc<dyn Place>,
    site: crate::place::Site,
    session: Option<SessionRef>,
    /// Streamed since the last boundary.
    tail: String,
    /// The last assistant message since the last boundary: the report, if nothing follows.
    report: Option<String>,
    /// Why the run could not do its job (`RunEv::NotRun`): `task.settled` says it as `not_run`.
    not_run: Option<String>,
    /// A tool call, a shell command or a change was seen.
    work: bool,
    wall: Option<u64>,
    /// The timer that tells a doer its time is nearly up.
    warn: Option<u64>,
    ask: Option<OpenAsk>,
    /// The doer's last reading of how full its context is.
    window: Option<f64>,
    /// The timer that ends a wait for a limit.
    cool: Option<u64>,
    /// The run now is a shrinking of the session; when it is over the task's own words are said (`then`), with whatever the
    /// operator steered in the meantime (`queued`).
    compacting: bool,
    then: Option<String>,
    queued: Vec<String>,
    /// The provider showed its windows in this run (`run.window` was written).
    windows: bool,
    /// The words of the item of the doer's own list that is in hand, as it last said.
    active: String,
    /// The model that served the run, as the last `run.served` named it: `run.metrics` carries it, so a task's spend is per model.
    model: String,
    /// The task has not started its doer: the place asked first (`Site::hold`), and the answer says go on or stop.
    held: bool,
    /// The temporary folder the task has of its own, made when it started and removed when it ends.
    tmp: Option<std::path::PathBuf>,
    /// What the turn in hand is worth so far, in millionths of a dollar, as `RunEv::Spend` and the price of an earlier turn say it; the exact figure
    /// replaces it when the turn's `Metrics` come.
    estimate: i64,
    /// Which of its shell lines are failed attempts.
    ran: super::ran::Ran,
}

struct Engine {
    log: Log,
    book: Book,
    timers: Arc<Timers>,
    tx: Sender<Msg>,
    doers: HashMap<String, Arc<dyn Doer>>,
    places: HashMap<String, Arc<dyn Place>>,
    root: PathBuf,
    home: PathBuf,
    asks: Option<Arc<Asks>>,
    /// What machines have answered each asker task.
    ledgers: HashMap<String, Arc<Ledger>>,
    clock: Arc<dyn Clock>,
    ask_wait_ms: u64,
    next: u64,
    /// What sets this loop's task folders apart from those of every other loop: every loop numbers its tasks from `t1`.
    tmp_tag: String,
    live: HashMap<String, Live>,
    /// The first time the Log refused an event: nothing more can be recorded, so nothing more is started.
    failed: Option<Problem>,
    folds: Folds,
    /// The settings as they are now, and the tree a rule reads of them (an edit swaps both).
    settings: crate::setting::Live,
    rules: Arc<Rules>,
    failures: Arc<Failures>,
    plans: crate::cost::LivePlans,
    proposals: Option<Proposals>,
    launch: Option<Launcher>,
    rstate: RuleState,
    /// While a rule's action is carried out: the event it answers and how deep its chain is. Whatever is written meanwhile
    /// has it for a cause.
    acting: Option<(u64, u8)>,
    /// The connectors whose windows were said to be missing: once each.
    missing: HashSet<String>,
}

/// Tell a run something; a run that panics when told is a run that refused.
fn tell(run: &mut dyn Run, ctl: RunCtl) -> Result<(), Problem> {
    catch_unwind(AssertUnwindSafe(|| run.ctl(ctl))).unwrap_or_else(|_| Err(refused("run", "the doer failed when told")))
}

/// The doers by the names they give themselves; one that panics when asked is left out, and said.
fn doers(list: Vec<Arc<dyn Doer>>, problems: &mut Vec<Problem>) -> HashMap<String, Arc<dyn Doer>> {
    let mut by_id = HashMap::new();
    for d in list {
        match catch_unwind(AssertUnwindSafe(|| d.id().to_string())) {
            Ok(id) => {
                by_id.insert(id, d);
            }
            Err(_) => problems.push(Problem::warn("tasks", "a doer panicked when asked its name and is left out")),
        }
    }
    by_id
}

/// An answer as the settling line keeps it: whole, or its last bytes with a word that says how much was left out.
fn answer_kept(a: &str) -> String {
    let line = |s: &str| serde_json::to_string(s).map_or(s.len(), |j| j.len());
    if a.len() <= ANSWER_MAX && line(a) <= ANSWER_LINE {
        return a.to_string();
    }
    let mut max = ANSWER_MAX;
    let mut kept = json_tail(a, max);
    while line(&kept) > ANSWER_LINE && max > 1024 {
        max = max / 4 * 3;
        kept = json_tail(a, max);
    }
    format!("(the first {} bytes of the answer are in the Log) {kept}", a.len() - kept.len() + '…'.len_utf8())
}

fn json_tail(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut at = s.len() - max;
    while !s.is_char_boundary(at) {
        at += 1;
    }
    format!("…{}", &s[at..])
}

impl Engine {
    fn run(mut self, rx: Receiver<Msg>) {
        for msg in rx {
            if matches!(msg, Msg::Stop) {
                break;
            }
            if catch_unwind(AssertUnwindSafe(|| self.handle(msg))).is_err() {
                // nothing the loop was in the middle of can be trusted: every unfinished task is stopped, loudly
                for row in self.book.open() {
                    let why = Why::Failed { message: "the loop failed while handling an order".into() };
                    let _ = catch_unwind(AssertUnwindSafe(|| self.stop(&row.id, why, Actor::Core)));
                }
            }
        }
        for row in self.book.open() {
            self.stop(&row.id, Why::Interrupted, Actor::Core);
        }
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Spawn(spec, by, reply) => {
                let r = self.guard().and_then(|()| self.spawn(*spec, by));
                let _ = reply.send(r);
            }
            Msg::Ctl(id, ctl, by, reply) => {
                let r = self.guard().and_then(|()| self.ctl(&id, ctl, by));
                let _ = reply.send(r);
            }
            Msg::Resume(id, text, by, reply) => {
                let r = self.guard().and_then(|()| self.resume(&id, text, by));
                let _ = reply.send(r);
            }
            Msg::Run(id, epoch, sig) => self.on_signal(&id, epoch, sig),
            Msg::Timer(ev) => self.on_timer(&ev),
            Msg::Walked(ask, walked) => self.on_walked(&ask, walked),
            Msg::Event(ev) => self.react(&ev),
            Msg::Sync(reply) => {
                let _ = reply.send(Ok(()));
            }
            Msg::Failed(id, epoch, message, class) => self.on_failed(&id, epoch, message, class),
            Msg::Stop => {}
        }
    }

    fn guard(&self) -> Result<(), Problem> {
        self.failed.clone().map_or(Ok(()), Err)
    }

    /// Say something live of task `id`, to the screens of its thread and nobody else.
    fn flash(&mut self, id: &str, kind: &str, by: Actor, data: Json) {
        let mut n = New::of(kind, by, data).task(id);
        if let Some(t) = self.book.thread(id) {
            n = n.thread(&t);
        }
        let _ = self.log.flash(n);
    }

    /// Write an event of task `id`.
    fn put(&mut self, id: &str, kind: &str, by: Actor, data: Json) -> Option<Ev> {
        let mut n = New::of(kind, by, data).task(id);
        if let Some(t) = self.book.thread(id) {
            n = n.thread(&t);
        }
        self.append(n)
    }

    /// Write an event. While a rule's action is carried out, what is written answers the event the rule answered.
    fn append(&mut self, mut n: New) -> Option<Ev> {
        if let Some((cause, _)) = self.acting {
            n = n.cause(cause);
        }
        match self.log.append(n) {
            Ok(ev) => {
                if let Some((_, depth)) = self.acting {
                    self.rstate.mark(ev.seq, depth + 1);
                }
                Some(ev)
            }
            Err(p) => {
                self.failed.get_or_insert(p);
                None
            }
        }
    }

    /// A task enters the Log; its row, and so its thread, does not exist yet.
    fn queue(&mut self, id: &str, spec: &TaskSpec, by: Actor) {
        let mut data = queued(spec);
        // a doer that runs code and no model is said so, for the row that draws the task
        if self.doers.get(&spec.doer).is_some_and(|d| catch_unwind(AssertUnwindSafe(|| d.code())).unwrap_or(false)) {
            data["code"] = json!(true);
        }
        let mut n = New::of("task.queued", by, data).task(id);
        if let Some(t) = &spec.thread {
            n = n.thread(t);
        }
        self.append(n);
    }

    /// The temporary folder a task has of its own: `<system tmp>/j4n-<user>/<process>-<loop>/<task>`, made private, given to its programs as `TMPDIR`, told to the
    /// Gate as the task's tmp, and removed when it ends. Behind the box the private `/tmp` is the task's own, so the Gate is told that and no folder is made.
    fn task_tmp(&self, id: &str, site: &crate::place::Site) -> Option<std::path::PathBuf> {
        let gate = self.asks.as_ref().and_then(|a| a.gate.clone());
        if site.notes.iter().any(|n| n.starts_with(crate::place::BEHIND)) {
            if let Some(g) = &gate {
                g.set_tmp(id, "/tmp");
            }
            return None;
        }
        let user = std::env::var("USER").ok().filter(|u| !u.is_empty() && u.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))).unwrap_or_else(|| "u".into());
        let dir = std::env::temp_dir().join(format!("j4n-{user}")).join(&self.tmp_tag).join(id);
        std::fs::create_dir_all(&dir).ok()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
        if let Some(g) = &gate {
            g.set_tmp(id, &dir.display().to_string());
        }
        Some(dir)
    }

    /// The place a task of a doer with `Caps::walled` is put in instead of `here` or `worktree`: their box twins, when they are described.
    fn walled_place(&self, spec: &TaskSpec) -> Option<&'static str> {
        let wall = match spec.place.as_str() {
            "here" => "box",
            "worktree" => "worktree-box",
            _ => return None,
        };
        let walled = self.doers.get(&spec.doer).is_some_and(|d| catch_unwind(AssertUnwindSafe(|| d.caps())).unwrap_or_default().walled);
        (walled && self.places.contains_key(wall)).then_some(wall)
    }

    fn doer_actor(&self, id: &str) -> Actor {
        Actor::Doer(self.live.get(id).map_or_else(|| "?".to_string(), |l| l.doer_id.clone()))
    }

    // ---- starting ----

    fn spawn(&mut self, spec: TaskSpec, by: Actor) -> Result<String, Problem> {
        let mut spec = spec;
        spec.check().map_err(|m| refused("task", m))?;
        // a task of a tree with a cap is given no more than the tree has left of it: a map item cannot start with more than is left
        if spec.doer != HUMAN
            && let Some(left) = self.left_above(spec.parent.as_deref())
        {
            if left <= 0 {
                return Err(refused("task", "the money of the task it belongs to has run out"));
            }
            let own = spec.limits.usd_soft.map_or(i64::MAX, micros);
            spec.limits.usd_soft = Some(own.min(left) as f64 / 1e6);
        }
        // a doer that has no wall of its own runs behind the box when one is described: the OS holds it to the project, whatever its own rules say
        if let Some(wall) = self.walled_place(&spec) {
            spec.place = wall.into();
        }
        if let Some(parent) = &spec.parent {
            match self.book.get(parent) {
                Some(r) if is_branch(&r.spec) && spec.doer != HUMAN && !r.spec.doer.starts_with("flow:") => {
                    return Err(refused("task", format!("{parent} is a branch, and a branch starts no branch (depth one)")));
                }
                // a branch of the main lane's line is a part of the line's task even after the turn is over
                Some(_) if spec.input.get("branch").is_some() => {}
                Some(r) if !r.state.is_terminal() => {}
                Some(_) => return Err(refused("task", format!("{parent} is over and takes no more parts"))),
                None => return Err(refused("task", format!("no task {parent}"))),
            }
        }
        if let Some(f) = &spec.follows
            && self.book.get(f).is_none()
        {
            return Err(refused("task", format!("no task {f} to carry on from")));
        }
        let id = format!("t{}", self.next);
        self.next += 1;
        self.queue(&id, &spec, by);
        self.begin(&id, spec);
        self.guard().map(|()| id)
    }

    /// Queued → Running: make the place ready, set the wall, start the doer.
    fn begin(&mut self, id: &str, spec: TaskSpec) {
        let Some(doer) = self.doers.get(&spec.doer).cloned() else {
            return self.stop(id, Why::Failed { message: format!("no doer called `{}`", spec.doer) }, Actor::Core);
        };
        let place = match self.place_for(&spec.place) {
            Ok(p) => p,
            Err(message) => return self.stop(id, Why::Failed { message }, Actor::Core),
        };
        let prepared = catch_unwind(AssertUnwindSafe(|| place.prepare(&Prepare { task: id, spec: &spec, root: &self.root })));
        let site = match prepared {
            Ok(Ok(site)) => site,
            Ok(Err(p)) => return self.stop(id, Why::Failed { message: format!("the place is not ready: {}", p.msg) }, Actor::Core),
            Err(_) => return self.stop(id, Why::Failed { message: "the place failed while getting ready".into() }, Actor::Core),
        };
        let wall = match spec.limits.wall_ms {
            Some(ms) => match self.timers.after(ms, Timer::new(WALL).task(id)) {
                Ok(t) => Some(t),
                Err(p) => {
                    place.settle(&site);
                    return self.stop(id, Why::Failed { message: p.msg }, Actor::Core);
                }
            },
            None => None,
        };
        let warn = spec.limits.wall_ms.and_then(|ms| self.warn_after(ms)).and_then(|at| self.timers.after(at, Timer::new(WARN).task(id)).ok());
        let notes = site.notes.clone();
        let hold = site.hold.clone();
        let resume = spec.resume.clone();
        let caps = catch_unwind(AssertUnwindSafe(|| doer.caps())).unwrap_or_default();
        let tmp = self.task_tmp(id, &site);
        self.live.insert(
            id.to_string(),
            Live {
                epoch: 0,
                run: None,
                doer: doer.clone(),
                doer_id: spec.doer.clone(),
                caps,
                place,
                site,
                session: resume.clone(),
                tail: String::new(),
                report: None,
                not_run: None,
                work: false,
                wall,
                warn,
                ask: None,
                window: None,
                cool: None,
                compacting: false,
                then: None,
                queued: Vec::new(),
                windows: false,
                active: String::new(),
                model: String::new(),
                held: false,
                tmp,
                estimate: 0,
                ran: Default::default(),
            },
        );
        // a task started for what a held thread read from outside is tainted from its first action
        if let (Some(from), Some(gate)) = (spec.input.get("taint_from").and_then(Json::as_str), self.asks.as_ref().and_then(|a| a.gate.as_ref())) {
            gate.taint(id, from);
        }
        let data = json!({ "doer": spec.doer, "place": spec.place, "resumed": resume.is_some(), "session": resume, "parent": spec.parent });
        self.put(id, "task.started", Actor::Core, data);
        for n in notes {
            self.put(id, "run.notice", Actor::Core, json!({ "text": n }));
        }
        match hold {
            Some(question) => self.hold(id, question),
            None => self.go(id),
        }
    }

    /// Start the doer of a task that is ready (shrinking its session first when it was asked).
    fn go(&mut self, id: &str) {
        let Some(spec) = self.book.get(id).map(|r| r.spec) else { return };
        let resume = self.live.get(id).and_then(|l| l.session.clone());
        match &spec.compact {
            Some(notes) => self.begin_compaction(id, resume, notes),
            // a doer that takes no Brief reads the reports that waited for the main lane after the goal
            None => {
                // and what a new session is told of the one before
                let told: Vec<&str> = ["cli_text", "carry", "reports", "context"].iter().filter_map(|k| spec.input.get(*k).and_then(Json::as_str)).collect();
                let text = (!told.is_empty()).then(|| format!("{}\n\n{}", spec.goal, told.join("\n\n")));
                self.run_doer(id, resume, text, false)
            }
        }
    }

    /// The place wants a yes before the task starts: the question is the operator's, as an approval with no effect
    /// for the Gate to look at, so the policy answers it when nobody is there (`deny` stops the task, `full` goes on).
    fn hold(&mut self, id: &str, question: String) {
        let spec = AskSpec {
            local: "place".into(),
            question,
            shape: AskShape::Approval { action: "write with no checkpoint".into(), risk: 40 },
            inband: false,
            point: None,
            act: None,
        };
        self.open_ask(id, spec);
        if let Some(live) = self.live.get_mut(id).filter(|l| l.ask.is_some()) {
            live.held = true;
        }
    }

    /// The answer to a held task: yes starts it, anything else stops it.
    fn held_answer(&mut self, id: &str, answer: &Answer) {
        if let Some(live) = self.live.get_mut(id) {
            live.held = false;
        }
        if *answer == Answer::Yes {
            self.go(id);
        } else {
            self.put(id, "run.notice", Actor::Core, json!({ "text": "stopped before it started: it would have written with no checkpoint, and the answer was no" }));
            self.stop(id, Why::Aborted, Actor::Core);
        }
    }

    /// The place a name means. `@t5` is where task t5 worked: the same folder behind the same walls, for as long as its place
    /// still has it (a worktree keeps it while the work is not in the project's folder).
    fn place_for(&self, name: &str) -> Result<Arc<dyn Place>, String> {
        let Some(origin) = name.strip_prefix('@') else {
            return self.places.get(name).cloned().ok_or_else(|| format!("no place called `{name}`"));
        };
        let (mut task, mut hops) = (origin.to_string(), 0);
        loop {
            let row = self.book.get(&task).ok_or_else(|| format!("`{name}`: no task {task}"))?;
            match row.spec.place.strip_prefix('@') {
                Some(next) if hops < 8 => (task, hops) = (next.to_string(), hops + 1),
                Some(_) => return Err(format!("`{name}` leads through too many tasks")),
                None => {
                    let home = self.places.get(&row.spec.place).cloned().ok_or_else(|| format!("`{name}`: no place called `{}`", row.spec.place))?;
                    let via = Via::of(home, &task).ok_or_else(|| format!("`{name}`: task {task} no longer has its place (its work was applied or let go)"))?;
                    return Ok(Arc::new(via));
                }
            }
        }
    }

    /// Start a run of the task's doer. A doer that cannot start stops the task.
    fn run_doer(&mut self, id: &str, resume: Option<SessionRef>, text: Option<String>, compacting: bool) {
        let Some(spec) = self.book.get(id).map(|r| r.spec) else { return };
        let Some(live) = self.live.get_mut(id) else { return };
        live.epoch += 1;
        let epoch = live.epoch;
        let (tx, task) = (self.tx.clone(), id.to_string());
        let out = Emit::new(move |sig| tx.send(Msg::Run(task.clone(), epoch, sig)).is_ok());
        let (place, site, tmp) = (live.place.clone(), live.site.clone(), live.tmp.clone());
        let launcher = self.launch.clone().filter(|_| spec.account.is_some());
        let for_task = spec.clone();
        // a launch that cannot be made as the account is not made as anyone else: it runs `false`, saying why
        let scratch = tmp.clone();
        let wrap = move |mut launch: crate::place::Launch| {
            // the task's programs put their scratch in its own folder
            if let Some(t) = &tmp {
                launch.env.entry("TMPDIR".into()).or_insert_with(|| t.display().to_string());
            }
            match &launcher {
            Some(as_account) => match as_account(&for_task, launch) {
                Ok(launch) => place.wrap(&site, launch),
                Err(why) => crate::place::Launch::new(vec!["/bin/false".into(), format!("j4n: {why}")], site.dir.clone(), Default::default()),
            },
            None => place.wrap(&site, launch),
            }
        };
        let cx = StartCx {
            task: id.to_string(),
            spec,
            site: live.site.clone(),
            resume,
            text,
            compacting,
            out,
            tmp: scratch,
            wrap: Arc::new(wrap),
        };
        let doer = live.doer.clone();
        match catch_unwind(AssertUnwindSafe(|| doer.start(cx))) {
            Ok(Ok(run)) => live.run = Some(run),
            Ok(Err(p)) => self.stop(id, Why::Failed { message: p.msg }, Actor::Core),
            Err(_) => self.stop(id, Why::Failed { message: "the doer failed while starting".into() }, Actor::Core),
        }
    }

    /// Stop the run, carry on in its session with `text`.
    fn restart(&mut self, id: &str, text: Option<String>, by: Actor, kind: &str, data: Json) -> Result<(), Problem> {
        let live = self.live.get_mut(id).ok_or_else(|| refused(id, "it is not running"))?;
        let session = live.session.clone().filter(|_| live.caps.resume);
        let session = session.ok_or_else(|| refused(id, "this doer cannot carry on in a session, so it cannot be changed while it works"))?;
        if let Some(run) = live.run.as_mut() {
            let _ = tell(run.as_mut(), RunCtl::Abort);
        }
        live.run = None;
        // what it had streamed is kept as it was said; the new run's report starts clean
        self.boundary(id);
        let Some(live) = self.live.get(id) else { return Ok(()) };
        let doer = live.doer_id.clone();
        self.put(id, kind, by, data);
        self.put(id, "task.started", Actor::Core, json!({ "doer": doer, "resumed": true, "session": session }));
        self.run_doer(id, Some(session), text, false);
        Ok(())
    }

    // ---- orders ----

    fn ctl(&mut self, id: &str, ctl: Ctl, by: Actor) -> Result<(), Problem> {
        let row = self.book.get(id).ok_or_else(|| refused("task", format!("no task {id}")))?;
        if let Ctl::Answer(a) = ctl {
            return self.answer(&row.id, a, by);
        }
        if let Ctl::Grant(scope) = ctl {
            return self.grant(&row.id, scope, by);
        }
        if row.state.is_terminal() {
            return Err(refused(id, format!("it is over ({}); carry on with resume", row.state.word())));
        }
        if row.spec.doer == HUMAN {
            return Err(refused(id, "this is a question: answer it, or stop the task that asked it"));
        }
        match ctl {
            Ctl::Abort => self.stop(id, Why::Aborted, by),
            Ctl::Kill => self.stop(id, Why::Killed, by),
            Ctl::Steer(text) => {
                if text.trim().is_empty() {
                    return Err(refused(id, "a steer needs words"));
                }
                if let State::Cooling { .. } = row.state {
                    return Err(refused(id, "it is waiting out a limit and goes on by itself — stop it, or wait"));
                }
                if !matches!(row.state, State::Running) {
                    return Err(refused(id, "it is not running — answer its question first, or resume it"));
                }
                if row.spec.shrinks_only() {
                    return Err(refused(id, "it is only shrinking the session: say it when that is over"));
                }
                let live = self.live.get_mut(id).ok_or_else(|| refused(id, "it is not running"))?;
                if live.compacting {
                    // the session is being shrunk and a steer would land in the middle of it: it is said with the first words after
                    live.queued.push(text.clone());
                    self.put(id, "task.steered", by, json!({ "text": text, "queued": true }));
                    return self.guard();
                }
                if live.caps.live {
                    let run = live.run.as_mut().ok_or_else(|| refused(id, "its doer has not started yet"))?;
                    tell(run.as_mut(), RunCtl::Steer(text.clone()))?;
                    self.put(id, "task.steered", by, json!({ "text": text }));
                } else {
                    self.restart(id, Some(text.clone()), by, "task.steered", json!({ "text": text, "restarted": true }))?;
                }
            }
            Ctl::Retune(tune) => {
                if !matches!(row.state, State::Running) {
                    return Err(refused(id, "only a running task can be retuned"));
                }
                if self.live.get(id).is_some_and(|l| l.compacting) {
                    return Err(refused(id, "its session is being shrunk — retune it when that is over"));
                }
                self.restart(id, None, by, "task.retuned", json!({ "tune": tune }))?;
            }
            // a task that waits out a limit goes on at once as another login, in a new session: the old one is the old login's
            Ctl::Move { account, why } => {
                if !matches!(row.state, State::Cooling { .. }) {
                    return Err(refused(id, "only a task that waits out a limit can move to another login"));
                }
                let live = self.live.get_mut(id).ok_or_else(|| refused(id, "it is not running"))?;
                if let Some(t) = live.cool.take() {
                    self.timers.cancel(t);
                }
                let from = row.spec.account.clone();
                live.session = None;
                self.put(id, "task.moved", by, json!({ "from": from, "to": account, "why": why }));
                self.put(id, "task.resumed", Actor::Core, json!({ "moved": true }));
                self.go(id);
            }
            Ctl::Answer(_) | Ctl::Grant(_) => {}
        }
        self.guard()
    }

    fn answer(&mut self, id: &str, answer: Answer, by: Actor) -> Result<(), Problem> {
        let row = self.book.get(id).ok_or_else(|| refused("task", format!("no task {id}")))?;
        let Some(ask) = row.spec.ask().filter(|_| row.spec.doer == HUMAN) else {
            return Err(refused(id, "this is not a question"));
        };
        if matches!(ask.shape, AskShape::Approval { .. }) && matches!(by, Actor::Model(_)) {
            return Err(refused(id, "an approval is answered by the operator or by a grant, never by a model"));
        }
        ask.shape.check(&answer).map_err(|m| refused(id, m))?;
        // a Block is never turned into a yes, not by the operator either: this holds however soon the answer comes, even before
        // the chain has walked (a piped line that answers a question the moment it is asked)
        if answer == Answer::Yes
            && let (Some(act), Some(gate)) = (ask.act.as_deref(), self.asks.as_ref().and_then(|a| a.gate.as_ref()))
            && let Verdict::Block { why, .. } = gate.check(act)
        {
            return Err(refused(id, format!("the Gate blocks this, and no answer changes that: {why}")));
        }
        let parent = row.spec.parent.clone();
        match &row.state {
            State::Running => {
                let parent = parent.ok_or_else(|| refused(id, "this question has no asker"))?;
                self.settle_ask(id, &parent, &ask, &answer, by, "human", json!({}))
            }
            State::Stopped { why: Why::Deadline } => {
                // too late for the child that asked, not for its task: carry on in its session with the answer
                let parent = parent.ok_or_else(|| refused(id, "this question has no asker"))?;
                let carried_on = self.book.rows().into_iter().find(|r| r.spec.follows.as_deref() == Some(parent.as_str()));
                if let Some(next) = carried_on {
                    return Err(refused(id, format!("nobody is waiting for this answer any more: {} carried on already", next.id)));
                }
                self.resume(&parent, Some(answer.render()), by).map(|_| ())
            }
            other => Err(refused(id, format!("this question is over ({})", other.word()))),
        }
    }

    /// The operator answered a question the task is waiting on.
    #[allow(clippy::too_many_arguments)]
    fn settle_ask(&mut self, id: &str, parent: &str, ask: &AskSpec, answer: &Answer, by: Actor, via: &str, extra: Json) -> Result<(), Problem> {
        let open = self.live.get_mut(parent).and_then(|l| l.ask.take_if(|a| a.id == id)).ok_or_else(|| refused(id, "its asker is not waiting for it"))?;
        for t in [open.timer, open.after].into_iter().flatten() {
            self.timers.cancel(t);
        }
        let data = json!({ "ok": true, "answer": answer.render(), "given": answer, "via": via });
        self.put(id, "task.settled", by.clone(), data);
        // a yes lets the line run: what it makes below a system temporary folder is the task's own from now on
        if *answer == Answer::Yes
            && let (Some(act), Some(gate)) = (ask.act.as_ref(), self.asks.as_ref().and_then(|a| a.gate.as_ref()))
        {
            gate.keep(act);
        }
        let point = open.walked.as_ref().map(|(p, _)| p.id.clone()).or_else(|| ask.point.clone());
        let impact = open.walked.as_ref().map(|(p, _)| p.impact.word());
        let mut said = json!({ "ask": id, "point": point, "impact": impact, "answer": answer.render(), "given": answer, "via": via });
        // an approval says what it was for, so a count of the same yes (the grow pack's) can be kept: the action as one word, and the
        // id the row that would remember it has
        if let (Some(into), Some(act)) = (said.as_object_mut(), ask.act.as_ref()) {
            into.insert("verb".into(), json!(act.verb.word()));
            into.insert("target".into(), json!(act.target));
            into.insert("action".into(), json!(format!("{}:{}", act.verb.word(), act.target)));
            into.insert("action_id".into(), json!(grant::here_id(act)));
            into.insert("grantable".into(), json!(grant::grantable(act).is_ok()));
        }
        if let (Some(into), Some(from)) = (said.as_object_mut(), extra.as_object()) {
            into.extend(from.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        self.put(parent, "ask.answered", by.clone(), said);
        if self.live.get(parent).is_some_and(|l| l.held) {
            self.put(parent, "task.resumed", Actor::Core, json!({ "ask": id }));
            self.held_answer(parent, answer);
        } else if ask.inband {
            self.put(parent, "task.resumed", Actor::Core, json!({ "ask": id }));
            if let Some(run) = self.live.get_mut(parent).and_then(|l| l.run.as_mut()) {
                // a no that the Gate's rule gave carries the rule's reason to the doer, which tells its model
                if *answer == Answer::No
                    && let (Some(act), Some(gate)) = (ask.act.as_deref(), self.asks.as_ref().and_then(|a| a.gate.as_ref()))
                    && let Verdict::Block { why, .. } = gate.check(act)
                {
                    let _ = tell(run.as_mut(), RunCtl::Told { local: ask.local.clone(), why });
                }
                let ctl = RunCtl::Answer { local: ask.local.clone(), answer: answer.clone() };
                if tell(run.as_mut(), ctl).is_err() {
                    self.stop(parent, Why::Failed { message: "the answer could not be given to the doer".into() }, Actor::Core);
                }
            }
        } else if let Err(p) = self.restart(parent, Some(answer.render()), Actor::Core, "task.resumed", json!({ "ask": id })) {
            self.stop(parent, Why::Failed { message: p.msg }, Actor::Core);
        }
        self.guard()
    }

    fn resume(&mut self, id: &str, text: Option<String>, by: Actor) -> Result<String, Problem> {
        self.resume_with(id, text, None, by)
    }

    /// `resume`, having the session shrunk first when `compact` has notes (possibly none).
    fn resume_with(&mut self, id: &str, text: Option<String>, compact: Option<Vec<String>>, by: Actor) -> Result<String, Problem> {
        let row = self.book.get(id).ok_or_else(|| refused("task", format!("no task {id}")))?;
        if !row.state.is_terminal() {
            return Err(refused(id, "it is still going — steer it, or stop it first"));
        }
        if row.spec.doer == HUMAN {
            return Err(refused(id, "a question is answered, not resumed"));
        }
        let mut spec = row.spec.clone();
        spec.follows = Some(row.id.clone());
        spec.resume = row.session.clone();
        if let Some(t) = text.filter(|t| !t.trim().is_empty()) {
            spec.goal = t;
        }
        spec.compact = compact;
        spec.input.as_object_mut().map(|o| (o.remove("reports"), o.remove("carry"), o.remove("context"))); // told once, with the turn that was resumed
        if matches!(by, Actor::Rule(_)) {
            // a task a rule carries on is the same piece of work: it has what the one before left of its time and its money, so
            // that carrying on again and again does not make the limits longer
            if let Some(wall) = spec.limits.wall_ms {
                let used = row.ended_ms.zip(row.started_ms).map_or(0, |(end, start)| end.saturating_sub(start));
                match wall.saturating_sub(used) {
                    0 => return Err(refused(id, "the time it was given has run out")),
                    left => spec.limits.wall_ms = Some(left),
                }
            }
            if let Some(soft) = spec.limits.usd_soft {
                match micros(soft) - row.spent {
                    left if left > 0 => spec.limits.usd_soft = Some(left as f64 / 1e6),
                    _ => return Err(refused(id, "the money it was given has run out")),
                }
            }
        }
        self.spawn(spec, by)
    }

    // ---- what a run says ----

    fn on_signal(&mut self, id: &str, epoch: u32, sig: Signal) {
        if self.live.get(id).is_none_or(|l| l.epoch != epoch) {
            return;
        }
        match sig {
            Signal::Gone => self.stop(id, Why::Failed { message: "the doer stopped without a report".into() }, Actor::Core),
            Signal::Ev(ev) => self.on_ev(id, ev),
        }
    }

    /// The streamed words so far are what was said before this point: keep them as they were said.
    fn boundary(&mut self, id: &str) {
        let Some(live) = self.live.get_mut(id) else { return };
        let said = std::mem::take(&mut live.tail);
        live.report = None;
        if !said.trim().is_empty() {
            let by = self.doer_actor(id);
            self.put(id, "run.message", by, json!({ "role": "assistant", "text": said, "narration": true }));
        }
    }

    fn on_ev(&mut self, id: &str, ev: RunEv) {
        let by = self.doer_actor(id);
        match ev {
            RunEv::Text(t) => {
                self.flash(id, "run.delta", by, json!({ "text": t }));
                if let Some(live) = self.live.get_mut(id) {
                    live.tail.push_str(&t);
                    if live.tail.len() > TAIL_MAX {
                        live.tail = json_tail(&live.tail, TAIL_MAX / 2);
                    }
                }
            }
            RunEv::Reasoning(t) => {
                self.flash(id, "run.reasoning_delta", by, json!({ "text": t }));
            }
            RunEv::Open { id: call, name, target, bytes } => {
                // the words before a call are settled as it begins, as they are when it is run
                self.boundary(id);
                self.flash(id, "run.open", by, json!({ "id": call, "name": name, "target": target, "bytes": bytes }));
            }
            RunEv::Tokens(n) => {
                self.flash(id, "run.tokens", by, json!({ "out": n }));
            }
            RunEv::Restart => {
                // what the broken try had streamed is not what the next one says: it is not kept as words said before a call
                if let Some(live) = self.live.get_mut(id) {
                    live.tail.clear();
                }
                self.flash(id, "run.tokens", by, json!({ "out": 0 }));
            }
            RunEv::Message { role: Role::Assistant, text } => {
                if let Some(live) = self.live.get_mut(id) {
                    live.tail.clear();
                    live.report = Some(text.clone());
                }
                self.put(id, "run.message", by, json!({ "role": "assistant", "text": text }));
            }
            RunEv::Message { role: Role::Reasoning, text } => {
                self.put(id, "run.reasoning", by, json!({ "text": text }));
            }
            RunEv::Tool { name, detail, end } => {
                self.boundary(id);
                self.worked(id);
                let kind = if end { "run.tool_result" } else { "run.tool_call" };
                self.put(id, kind, by, json!({ "name": name, "detail": detail }));
            }
            RunEv::Shell { cmd, out, exit } => {
                self.boundary(id);
                self.worked(id);
                let uncounted = self.live.get_mut(id).and_then(|l| l.ran.uncounted(&cmd, exit));
                let mut data = json!({ "cmd": cmd, "out": json_tail(&out, SHELL_OUT_MAX), "exit": exit });
                if let Some(why) = uncounted {
                    data["uncounted"] = json!(why);
                }
                self.put(id, "run.shell", by, data);
            }
            RunEv::Py(mut cell) => {
                self.worked(id);
                let code = cell.as_object_mut().and_then(|o| o.remove("code"));
                if let Some(blob) = code.as_ref().and_then(Json::as_str).and_then(|c| self.log.blobs().put(c.as_bytes()).ok()) {
                    cell["blob"] = blob.into();
                }
                self.put(id, "py.cell", by, cell);
            }
            RunEv::Changed(paths) => {
                self.worked(id);
                if let Some(live) = self.live.get_mut(id) {
                    live.ran.change();
                }
                let more = paths.len().saturating_sub(super::PATHS_KEPT);
                let kept: Vec<&String> = paths.iter().take(super::PATHS_KEPT).collect();
                self.put(id, "run.changed", by, json!({ "paths": kept, "more": more }));
            }
            RunEv::Notice(text) => {
                self.put(id, "run.notice", by, json!({ "text": text }));
            }
            RunEv::Unread { dialect, kind } => {
                self.put(id, "run.unread", by, json!({ "dialect": dialect, "kind": kind }));
            }
            RunEv::Health(h) => {
                self.put(id, "run.health", by, h);
            }
            RunEv::Spend(units) => self.running(id, units),
            RunEv::Metrics(m) => self.metrics(id, by, m),
            RunEv::Limits(now) => self.limits(id, by, now),
            RunEv::Served(s) => {
                let asked = self.book.get(id).and_then(|r| r.spec.tune.model).unwrap_or_default();
                let new = self.live.get_mut(id).filter(|l| !s.model.is_empty() && l.model != s.model).map(|l| l.model.clone_from(&s.model)).is_some();
                let data = json!({ "model": s.model, "host": s.host, "input": s.input, "cached": s.cached, "wrote": s.wrote, "output": s.output, "ttft_ms": s.ttft_ms, "ms": s.ms });
                self.put(id, "run.served", by.clone(), data);
                // a pin that is not what ran is said in plain words, once for each model that serves the run: the spend is that model's
                if new && crate::doer::served_other(&asked, &s.model) {
                    self.put(id, "run.notice", by, json!({ "text": format!("asked for the model {asked}, and {} served it: that is the model it is billed to", s.model) }));
                }
            }
            RunEv::Session(session) => {
                let doer = self.live.get(id).map(|l| l.doer_id.clone()).unwrap_or_default();
                if let Some(live) = self.live.get_mut(id) {
                    live.session = Some(SessionRef { doer: doer.clone(), id: session.clone() });
                }
                self.put(id, "run.session", by, json!({ "doer": doer, "id": session }));
            }
            RunEv::Window(w) => {
                if let Some(live) = self.live.get_mut(id) {
                    live.window = Some(if w.is_finite() { w.clamp(0.0, 1.0) } else { 0.0 });
                }
            }
            RunEv::List { open, total, active } => {
                if let Some(live) = self.live.get_mut(id) {
                    live.active = active.clone();
                }
                self.put(id, "run.list", by, json!({ "open": open, "total": total, "active": active }));
            }
            RunEv::NotRun(why) => {
                if let Some(live) = self.live.get_mut(id) {
                    live.not_run = Some(why);
                }
            }
            RunEv::Ask(spec) => self.open_ask(id, spec),
            RunEv::Error(message) if self.live.get(id).is_some_and(|l| l.compacting) => {
                // a shrinking that fails is a note, not a stop: the task goes on with its own words
                self.put(id, "run.notice", by, json!({ "text": format!("the session could not be shrunk ({}); it goes on as it is", json_tail(&message, 300)) }));
                let only = self.book.get(id).is_some_and(|r| r.spec.shrinks_only());
                self.compaction_over(id, false, only.then(|| message.clone()));
            }
            RunEv::Error(message) => {
                let seen = self.failures.classify(&message, self.clock.now_ms());
                let tune = self.book.get(id).map(|r| r.spec.tune).unwrap_or_default();
                let mut data = json!({ "text": message, "class": seen.class.word(), "wait_ms": seen.wait_ms, "row": seen.row, "endpoint": tune.endpoint, "model": tune.model });
                // which login said it: a limit is the login's, not the doer's
                if let Some(a) = self.book.get(id).and_then(|r| r.spec.account) {
                    data["account"] = json!(a);
                }
                self.put(id, "run.error", by, data);
                // the rules answer the event first (it is ahead of this in the inbox); a task they did not take in hand fails
                if let Some(epoch) = self.live.get(id).map(|l| l.epoch) {
                    let _ = self.tx.send(Msg::Failed(id.to_string(), epoch, message, seen.class));
                }
            }
            RunEv::End { answer, todo_left } => self.end(id, answer, todo_left),
        }
    }

    fn worked(&mut self, id: &str) {
        if let Some(live) = self.live.get_mut(id) {
            live.work = true;
        }
    }

    /// The doer a task runs on and the plan that pays for it, if there is one: not when the switch `cost.plan.on` is off, nor when the
    /// connector's `cost_source` says its stream shows no window (it is priced by its list cost).
    fn plan_of(&self, id: &str) -> Option<(String, crate::cost::Plan)> {
        if !self.settings.flag("cost.plan.on", true) {
            return None;
        }
        let doer = self.live.get(id)?.doer_id.clone();
        let plans = self.plans.now();
        if plans.source(&doer).is_some_and(|s| s.windows.is_none()) {
            return None;
        }
        let plan = plans.of(&doer, self.book.get(id)?.spec.account.as_deref())?.clone();
        Some((doer, plan))
    }

    /// A run on a plan whose stream should show windows ended its turn and showed none: said once for the connector, in the Log, and
    /// the request is priced as it would be without (`assumed`, or `not measured`).
    fn windows_missing(&mut self, id: &str, doer: &str) {
        let Some(path) = self.plans.now().source(doer).and_then(|s| s.windows.clone()) else { return };
        if self.live.get(id).is_none_or(|l| l.windows) || !self.missing.insert(doer.to_string()) {
            return;
        }
        self.put(id, "cost.path_missing", Actor::Core, json!({ "doer": doer, "path": path }));
    }

    /// The windows as the provider says them now, kept in the Log: what `plan_rate` measures a plan's windows from.
    fn limits(&mut self, id: &str, by: Actor, now: Vec<Limit>) {
        if !self.settings.flag("cost.plan.on", true) {
            return;
        }
        if let Some(live) = self.live.get_mut(id) {
            live.windows = true;
        }
        let account = self.book.get(id).and_then(|r| r.spec.account.clone());
        for l in &now {
            let mut data = json!({ "window": l.window, "utilization": l.utilization, "resets_at": l.resets_at, "status": l.status });
            if let Some(a) = &account {
                data["account"] = json!(a);
            }
            self.put(id, "run.window", by.clone(), data);
        }
    }

    fn metrics(&mut self, id: &str, by: Actor, m: Metrics) {
        // the turn's exact cost is in the book once this is written: what was only estimated of it is not counted twice
        if let Some(live) = self.live.get_mut(id) {
            live.estimate = 0;
        }
        let usd = if m.usd.is_finite() && m.usd > 0.0 { m.usd } else { 0.0 };
        if usd == 0.0 && m.tokens_in == 0 && m.tokens_out == 0 && m.cached == 0 && m.wrote == 0 && m.last == 0 {
            return;
        }
        let mut data = json!({ "usd": usd, "tokens_in": m.tokens_in, "tokens_out": m.tokens_out });
        // the cache parts of `tokens_in`, when the doer says them here (the model doers say them in `run.served`), and what the turn's last and first
        // call read (`last`: the session's size when the turn ended, which `tokens_in` overstates for a turn of several calls; `first`: its floor); a row without them is 0
        for (key, n) in [("cached", m.cached), ("wrote", m.wrote), ("last", m.last), ("first", m.first)] {
            if n > 0 {
                data[key] = json!(n);
            }
        }
        let account = self.book.get(id).and_then(|r| r.spec.account.clone());
        if let Some(a) = &account {
            data["account"] = json!(a);
        }
        if let Some(model) = self.live.get(id).map(|l| l.model.as_str()).filter(|m| !m.is_empty()) {
            data["model"] = json!(model);
        }
        if let Some((doer, plan)) = self.plan_of(id) {
            self.windows_missing(id, &doer);
            let (min_n, now) = (self.settings.num("cost.plan.min_n", 5.0) as usize, self.clock.now_ms());
            let rate = |w: &str| cost::rate(&self.folds, &doer, account.as_deref().unwrap_or(""), w, now);
            let spent = plan.cost(usd, min_n, rate);
            // the share of a window needs no price: it is the first money word wherever there is a measure or an assumption
            if let Some(s) = plan.share(usd, min_n, rate).filter(|s| s.frac.is_finite() && s.frac > 0.0) {
                (data["share"], data["share_of"], data["source"]) = (json!(s.frac), json!(s.of), json!(s.source));
                if let Some(w) = s.window {
                    data["window"] = json!(w);
                }
            }
            match (plan.price, spent) {
                (None, _) => data["plan"] = json!(plan.unset()),
                (Some(_), None) => data["plan"] = json!("not measured"),
                (Some(_), Some(c)) => {
                    (data["plan_usd"], data["source"]) = (json!(c.usd), json!(c.source));
                    if let Some(w) = c.window {
                        data["window"] = json!(w);
                    }
                }
            }
        }
        self.put(id, "run.metrics", by, data);
        self.check_caps(id);
    }

    /// A turn that is not over says what it has used so far: priced at what an earlier turn of the doer on this model cost, it is held against the
    /// hard cap of the task and of every capped task above it, so a long turn is not let run to its end past the cap. A model no turn has priced yet (the
    /// first chat on opus) is priced from another model of the doer by their price words (`offload.prices`); with no priced turn of the doer, nothing is
    /// estimated (no invented number), and the cap lands when the turn's cost is reported.
    fn running(&mut self, id: &str, units: f64) {
        let Some(live) = self.live.get(id) else { return };
        let Some(model) = Some(live.model.clone()).filter(|m| !m.is_empty()).or_else(|| self.book.get(id).map(|r| r.model).filter(|m| !m.is_empty())) else { return };
        let (plans, settings) = (self.plans.now(), self.settings.now());
        let price = context::Price::of(&self.book, &live.doer_id, &model, &plans, &settings.settings).map(|p| p.unit).or_else(|| {
            let words = crate::front::tiers::parse(settings.settings.word("offload.prices", crate::front::tiers::PRICES));
            self.book.unit_scaled(&live.doer_id, &model, &plans.ratios(&live.doer_id)?, &words)
        });
        let Some(unit) = price.filter(|u| u.is_finite() && *u > 0.0) else { return };
        if !units.is_finite() || units <= 0.0 {
            return;
        }
        let estimate = (units * unit * 1e6).round() as i64;
        if let Some(live) = self.live.get_mut(id) {
            live.estimate = estimate;
        }
        self.check_estimate(id);
    }

    /// The estimates of the turns in hand of `id` and everything it started.
    fn tree_estimate(&self, id: &str) -> i64 {
        self.live.get(id).map_or(0, |l| l.estimate) + self.book.children(id).iter().map(|c| self.tree_estimate(&c.id)).sum::<i64>()
    }

    /// The hard cap against what has been reported and what the turns in hand are estimated to have used: the capped task is stopped as the
    /// reported spend would stop it, and the Log says it was the estimate.
    fn check_estimate(&mut self, id: &str) {
        let mut at = Some(id.to_string());
        while let Some(cur) = at {
            let Some(row) = self.book.get(&cur) else { return };
            if let Limits { usd_soft: Some(soft), .. } = row.spec.limits {
                let (soft, spent, estimate) = (micros(soft), self.tree_spent(&cur), self.tree_estimate(&cur));
                if (spent + estimate) as f64 > soft as f64 * HARD_FACTOR && !row.state.is_terminal() {
                    let data = json!({ "soft": soft as f64 / 1e6, "spent": spent as f64 / 1e6, "estimated": estimate as f64 / 1e6, "by": id });
                    self.put(&cur, "task.cap_estimated", Actor::Core, data);
                    self.stop(&cur, Why::HardCap, Actor::Core);
                }
            }
            at = row.spec.parent;
        }
    }

    /// What the task `id` and everything it started (steps of a flow, map items, sub-flows, branches) have spent: millionths of a dollar.
    fn tree_spent(&self, id: &str) -> i64 {
        self.book.get(id).map_or(0, |r| r.spent) + self.book.children(id).iter().map(|c| self.tree_spent(&c.id)).sum::<i64>()
    }

    /// The money a task may still spend, as the nearest cap above it and every cap further up leave it: the least of what each capped ancestor has
    /// left of its soft cap; none when no ancestor has a cap.
    fn left_above(&self, parent: Option<&str>) -> Option<i64> {
        let (mut least, mut at) = (None::<i64>, parent.map(String::from));
        while let Some(id) = at {
            let Some(r) = self.book.get(&id) else { break };
            if let Some(soft) = r.spec.limits.usd_soft {
                let left = micros(soft) - self.tree_spent(&id);
                least = Some(least.map_or(left, |l| l.min(left)));
            }
            at = r.spec.parent;
        }
        least
    }

    /// The caps of the task that has just spent, and of every task above it that has one: a cap binds the whole tree. Past the soft cap
    /// the capped task says so once; past [`HARD_FACTOR`] times it the capped task is stopped, and so is everything that is still going under it.
    fn check_caps(&mut self, id: &str) {
        let mut at = Some(id.to_string());
        while let Some(cur) = at {
            let Some(row) = self.book.get(&cur) else { return };
            if let Limits { usd_soft: Some(soft), .. } = row.spec.limits {
                let (soft, spent) = (micros(soft), self.tree_spent(&cur));
                if spent > soft && !row.capped {
                    let mut data = json!({ "soft": soft as f64 / 1e6, "spent": spent as f64 / 1e6 });
                    if cur != id {
                        (data["tree"], data["by"]) = (json!(true), json!(id));
                    }
                    self.put(&cur, "task.cap_reached", Actor::Core, data);
                }
                if spent as f64 > soft as f64 * HARD_FACTOR {
                    self.stop(&cur, Why::HardCap, Actor::Core);
                }
            }
            at = row.spec.parent;
        }
    }

    fn open_ask(&mut self, id: &str, spec: AskSpec) {
        if let Err(m) = spec.check() {
            return self.stop(id, Why::Failed { message: format!("it asked a question that cannot be shown: {m}") }, Actor::Core);
        }
        if self.live.get(id).is_some_and(|l| l.ask.is_some()) {
            return self.stop(id, Why::Failed { message: "it asked a second question while the first was open".into() }, Actor::Core);
        }
        self.boundary(id);
        let by = self.doer_actor(id);
        let child = format!("t{}", self.next);
        self.next += 1;
        let thread = self.book.thread(id);
        let mut task = TaskSpec::new(&spec.question, HUMAN);
        task.parent = Some(id.to_string());
        task.thread = thread;
        task.limits.wall_ms = Some(self.ask_wait_ms);
        task.tune = Tune { mode: Mode::Read, ..Tune::default() };
        task.place = "none".into();
        task.input = json!({ "ask": spec });
        self.queue(&child, &task, by);
        self.put(&child, "task.started", Actor::Core, json!({ "doer": HUMAN }));
        let timer = self.timers.after(self.ask_wait_ms, Timer::new(DEADLINE).task(&child)).ok();
        let local = spec.local.clone();
        if let Some(live) = self.live.get_mut(id) {
            live.ask = Some(OpenAsk { id: child.clone(), spec, timer, walked: None, after: None, inside: Inside::NotPut });
        }
        self.put(id, "task.waiting", Actor::Core, json!({ "ask": child, "local": local }));
        self.start_walk(id, &child);
    }

    fn end(&mut self, id: &str, answer: Option<String>, todo_left: u32) {
        if self.live.get(id).is_some_and(|l| l.compacting) {
            return self.compaction_over(id, true, answer);
        }
        if let Some(open) = self.live.get(id).and_then(|l| l.ask.as_ref()) {
            if open.spec.inband {
                return self.stop(id, Why::Failed { message: "the doer ended while its question was open".into() }, Actor::Core);
            }
            // it asked and ended its run: it is waiting for the answer to start again
            if let Some(live) = self.live.get_mut(id) {
                live.run = None;
            }
            return;
        }
        let by = self.doer_actor(id);
        let Some(live) = self.live.get_mut(id) else { return };
        let tail = std::mem::take(&mut live.tail);
        let report = live.report.take();
        let given = answer.filter(|a| !a.trim().is_empty());
        let (answer, persisted) = match (given, report, !tail.trim().is_empty()) {
            (Some(a), r, _) => {
                let same = r.as_deref() == Some(a.as_str());
                (Some(a), same)
            }
            (None, Some(r), _) => (Some(r), true),
            (None, None, true) => (Some(tail.clone()), false),
            (None, None, false) => (None, false),
        };
        let did_work = live.work;
        let not_run = live.not_run.take();
        let (window, active) = (live.window, std::mem::take(&mut live.active));
        if !tail.trim().is_empty() && answer.as_deref() != Some(tail.as_str()) {
            self.put(id, "run.message", by.clone(), json!({ "role": "assistant", "text": tail, "narration": true }));
        }
        if let (Some(a), false) = (&answer, persisted) {
            self.put(id, "run.message", by, json!({ "role": "assistant", "text": a, "final": true }));
        }
        let settled = self.release(id);
        let nothing = answer.is_none() && !did_work && settled.is_empty();
        let reason = nothing.then_some("it ended with no answer and no sign of work: nothing was said, run or changed");
        let mut data = json!({
            "ok": !nothing,
            "answer": answer.as_deref().map(answer_kept),
            "reason": reason,
            "todo_left": todo_left,
            "changed": Changed::of(&settled),
            "window": window,
        });
        if !active.is_empty() {
            data["active"] = json!(active);
        }
        if let Some(why) = not_run {
            data["not_run"] = json!(why);
        }
        // what the session held: this turn's size, the floor and history, and whether it found the cache cold
        if let Some(row) = self.book.get(id)
            && let Some(session) = row.session.as_ref()
            && let Some(ctx) = context::turn(&self.book.of_session(row.spec.thread.as_deref(), session), id, self.plans.now().cache_ms(&session.doer), context::Price::of(&self.book, &session.doer, &row.model, &self.plans.now(), &self.settings.now().settings).as_ref())
        {
            data["ctx"] = ctx;
        }
        self.put(id, "task.settled", Actor::Core, data);
    }

    // ---- ending ----

    /// Let go of what a live task holds: its timers, its place. Says what the place found.
    fn release(&mut self, id: &str) -> Settled {
        let Some(live) = self.live.remove(id) else { return Settled::default() };
        self.ledgers.remove(id);
        let ask_timers = live.ask.into_iter().flat_map(|a| [a.timer, a.after]).flatten();
        for t in live.wall.into_iter().chain(live.warn).chain(live.cool).chain(ask_timers).chain(self.rstate.forget(id)) {
            self.timers.cancel(t);
        }
        let settled = catch_unwind(AssertUnwindSafe(|| live.place.settle(&live.site))).unwrap_or_else(|_| Settled { unknown: true, ..Settled::default() });
        // the scratch of a task is J4n's to clean up: its folder goes with it, and the Gate forgets the folders its lines made
        if let Some(t) = &live.tmp {
            let _ = std::fs::remove_dir_all(t);
            // the loop's folder goes with its last task (a plain remove: it stays while another task holds a folder in it)
            let _ = t.parent().map(std::fs::remove_dir);
        }
        if let Some(g) = self.asks.as_ref().and_then(|a| a.gate.as_ref()) {
            g.forget_tmp(id);
        }
        for n in &settled.notes {
            self.put(id, "run.notice", Actor::Core, json!({ "text": n }));
        }
        if let Some(kept) = &settled.kept {
            self.put(id, "task.kept", Actor::Core, json!(kept));
        }
        if let Some(work) = &settled.applied {
            self.put(id, "task.applied", Actor::Core, json!({ "work": work, "auto": true }));
        }
        settled
    }

    fn stop(&mut self, id: &str, why: Why, by: Actor) {
        self.stop_with(id, why, by, None);
    }

    /// `stop`, saying what kind of failure it was when it was one.
    fn stop_with(&mut self, id: &str, why: Why, by: Actor, class: Option<Class>) {
        if self.book.get(id).is_none_or(|r| r.state.is_terminal()) {
            return;
        }
        // what was going under a task that was capped stops because of the cap, and says so; under any other stop it is an orphan
        let kids = if matches!(why, Why::Capped | Why::HardCap) { why.clone() } else { Why::Orphaned };
        for child in self.book.children(id).into_iter().filter(|c| !c.state.is_terminal()) {
            self.stop(&child.id, kids.clone(), Actor::Core);
        }
        let (mut report, mut unsaid) = (None, None);
        if let Some(live) = self.live.get_mut(id) {
            if let Some(run) = live.run.as_mut() {
                let order = if matches!(why, Why::Killed | Why::HardCap) { RunCtl::Kill } else { RunCtl::Abort };
                let _ = tell(run.as_mut(), order);
            }
            // what was said and settled is the report; words still streaming are kept in the Log as they stood
            let said = std::mem::take(&mut live.tail);
            unsaid = (!said.trim().is_empty()).then_some(said);
            report = live.report.take().or(unsaid.clone());
        }
        if let Some(said) = unsaid {
            let by_doer = self.doer_actor(id);
            self.put(id, "run.message", by_doer, json!({ "role": "assistant", "text": said, "partial": true }));
        }
        let settled = self.release(id);
        let class = class.map(Class::word);
        self.put(id, "task.stopped", by, json!({ "why": why, "report": report, "changed": Changed::of(&settled), "class": class }));
        if matches!(why, Why::Wall) {
            self.log.sync();
        }
    }

    // ---- time ----

    /// When, after a task starts, its doer is told the time is nearly up: the setting `wall.warn.at` of the wall, for a wall that
    /// is `wall.warn.min` or longer (a shorter one is a probe or a test, not work to save). None: it is not told.
    fn warn_after(&self, wall_ms: u64) -> Option<u64> {
        let at = self.settings.num("wall.warn.at", WARN_AT);
        (at > 0.0 && at < 1.0 && wall_ms >= self.settings.dur_ms("wall.warn.min", WARN_MIN_MS)).then_some((wall_ms as f64 * at) as u64)
    }

    /// The warning is due: a doer that takes a steer in its run is told to write what it has now. One that would be stopped and
    /// started again to take it is not (it would lose the turn it is in), nor is a task that waits for an answer.
    fn warn_wall(&mut self, id: &str, timer: u64) {
        let Some(live) = self.live.get_mut(id).filter(|l| l.warn == Some(timer)) else { return };
        live.warn = None;
        if !live.caps.live || live.run.is_none() {
            return;
        }
        let wall = self.book.get(id).and_then(|r| r.spec.limits.wall_ms).unwrap_or(0);
        let left = wall.saturating_sub(self.warn_after(wall).unwrap_or(wall));
        let _ = self.ctl(id, Ctl::Steer(wall_warning(left)), Actor::Core);
    }

    fn on_timer(&mut self, ev: &Ev) {
        let Some(timer) = ev.data.get("id").and_then(Json::as_u64) else { return };
        if ev.data.get("tag").and_then(Json::as_str) == Some(RULE) {
            return self.rule_due(&ev.data["data"], timer);
        }
        let Some(id) = ev.task.as_deref() else { return };
        match ev.data.get("tag").and_then(Json::as_str) {
            Some(WALL) => {
                if self.live.get(id).is_some_and(|l| l.wall == Some(timer)) {
                    self.stop(id, Why::Wall, Actor::Core);
                }
            }
            Some(WARN) => self.warn_wall(id, timer),
            Some(DEADLINE) => {
                let Some(parent) = self.book.get(id).and_then(|r| r.spec.parent) else { return };
                if self.live.get(&parent).and_then(|l| l.ask.as_ref()).is_some_and(|a| a.id == id && a.timer == Some(timer)) {
                    self.stop(id, Why::Deadline, Actor::Core);
                    self.stop(&parent, Why::Parked { ask: id.to_string() }, Actor::Core);
                }
            }
            Some(COOL) => {
                if self.live.get(id).is_some_and(|l| l.cool == Some(timer)) {
                    if let Some(live) = self.live.get_mut(id) {
                        live.cool = None;
                    }
                    if let Err(p) = self.restart(id, Some(CONTINUE.into()), Actor::Core, "task.resumed", json!({ "cooled": true })) {
                        self.stop(id, Why::Failed { message: format!("it could not go on after the wait: {}", p.msg) }, Actor::Core);
                    }
                }
            }
            Some(AFTER) => {
                let Some(parent) = self.book.get(id).and_then(|r| r.spec.parent) else { return };
                let walked = self.live.get(&parent).and_then(|l| l.ask.as_ref()).filter(|a| a.id == id && a.after == Some(timer)).and_then(|a| a.walked.clone());
                let (Some((point, q)), Some(asks)) = (walked, self.asks.clone()) else { return };
                if let Some(mut o) = fallback(&point, &q, &asks.registry) {
                    o.why = format!("no answer in time: {}", o.why);
                    self.settle_machine(id, &parent, o, "fallback", Spent::default());
                }
            }
            _ => {}
        }
    }
}

// ---- limits, shrinking and the rules ----

impl Engine {
    /// A run said it failed. The rules have had their say by now: a task they took in hand (cooling) is not failed.
    fn on_failed(&mut self, id: &str, epoch: u32, message: String, class: Class) {
        let Some(row) = self.book.get(id) else { return };
        if self.live.get(id).is_none_or(|l| l.epoch != epoch) || matches!(row.state, State::Cooling { .. }) {
            return;
        }
        self.stop_with(id, Why::Failed { message }, Actor::Core, Some(class));
    }

    /// Wait out a limit: the run is let go, the session kept, and the task goes on by itself when the time has passed.
    fn cool(&mut self, id: &str, ms: u64, by: Actor, why: &str) -> Result<(), Problem> {
        let row = self.book.get(id).ok_or_else(|| refused("task", format!("no task {id}")))?;
        if !matches!(row.state, State::Running) {
            return Err(refused(id, "only a task that is running can wait out a limit"));
        }
        let longest = self.settings.dur_ms("cool.max", COOL_MAX_MS);
        if ms > longest {
            return Err(refused(id, format!("a wait of {} is longer than the {} a task is let wait", crate::fmt::duration(ms), crate::fmt::duration(longest))));
        }
        let live = self.live.get_mut(id).ok_or_else(|| refused(id, "it is not running"))?;
        if !(live.caps.resume && live.session.is_some()) {
            return Err(refused(id, "it cannot carry on in a session, so it cannot wait"));
        }
        let timer = self.timers.after(ms, Timer::new(COOL).task(id))?;
        if let Some(run) = live.run.as_mut() {
            let _ = tell(run.as_mut(), RunCtl::Abort);
        }
        live.run = None;
        live.cool = Some(timer);
        self.boundary(id);
        let until_ms = self.clock.now_ms().saturating_add(ms);
        self.put(id, "task.cooling", by, json!({ "until_ms": until_ms, "ms": ms, "why": why }));
        self.guard()
    }

    /// The task carries on in a session: shrink it first if it was asked, and the doer can. A task that only shrinks (`:compact`) that cannot
    /// has nothing else to do, and fails with why.
    fn begin_compaction(&mut self, id: &str, resume: Option<SessionRef>, notes: &[String]) {
        let can = resume.is_some() && self.live.get(id).is_some_and(|l| l.caps.compact);
        let only = self.book.get(id).is_some_and(|r| r.spec.shrinks_only());
        if !can {
            let why = if resume.is_none() { "there is no session to shrink" } else { "its doer cannot shrink a session" };
            if only {
                return self.stop(id, Why::Failed { message: format!("the session was not shrunk: {why}") }, Actor::Core);
            }
            self.put(id, "run.notice", Actor::Core, json!({ "text": format!("the session was not shrunk: {why}") }));
            return self.run_doer(id, resume, None, false);
        }
        let block = self.must_keep(id, notes).render();
        let goal = self.book.get(id).map(|r| r.spec.goal).unwrap_or_default();
        if let Some(live) = self.live.get_mut(id) {
            live.compacting = true;
            live.then = (!only).then(|| format!("{goal}\n\n{block}"));
        }
        let mut data = json!({ "keep": block });
        if only {
            data["only"] = json!(true);
        }
        self.put(id, "task.compacting", Actor::Core, data);
        self.run_doer(id, resume, Some(block), true);
    }

    /// The shrinking is over, well or not: the task's own words are said now, with what the operator steered meanwhile. A task that only
    /// shrinks (`:compact`) is over with it: the numbers are written (what the session held before and after, from the Book) and the
    /// task settles, or, when it failed, stops.
    fn compaction_over(&mut self, id: &str, ok: bool, note: Option<String>) {
        let before = self.book.get(id).and_then(|r| r.spec.follows).and_then(|f| self.book.get(&f)).and_then(|r| r.window);
        self.boundary(id);
        let Some(live) = self.live.get_mut(id) else { return };
        live.compacting = false;
        let (after, session) = (live.window.take(), live.session.clone());
        let note = note.map(|n| json_tail(n.trim(), 300));
        if self.book.get(id).is_some_and(|r| r.spec.shrinks_only()) {
            return self.shrunk(id, ok, note, (before, after));
        }
        let mut text = self.live.get_mut(id).and_then(|l| l.then.take()).unwrap_or_default();
        for q in self.live.get_mut(id).map(|l| std::mem::take(&mut l.queued)).unwrap_or_default() {
            text.push_str("\n\n");
            text.push_str(&q);
        }
        self.put(id, "task.compacted", Actor::Core, json!({ "ok": ok, "note": note, "before": before, "after": after }));
        self.run_doer(id, session, Some(text), false);
    }

    /// A `:compact` task is over: `task.compacted` says what the session held before and after in tokens (`held`, `held_after`: the doer says
    /// the second, or it is left out) and what was cut, then the task settles with the words of it as its answer; a failed one stops.
    fn shrunk(&mut self, id: &str, ok: bool, note: Option<String>, (before, after): (Option<f64>, Option<f64>)) {
        let (thread, session) = (self.book.get(id).and_then(|r| r.spec.thread), self.live.get(id).and_then(|l| l.session.clone()));
        let (held, held_after) = session.as_ref().map(|s| context::shrink(&self.book.of_session(thread.as_deref(), s), id)).unwrap_or_default();
        let mut data = json!({ "ok": ok, "note": note, "before": before, "after": after });
        if let Some(h) = held {
            data["held"] = json!(h);
            if let Some(a) = held_after.filter(|_| ok) {
                (data["held_after"], data["cut"]) = (json!(a), json!(h.saturating_sub(a)));
            }
        }
        self.put(id, "task.compacted", Actor::Core, data);
        if !ok {
            let why = note.unwrap_or_else(|| "the doer would not".into());
            return self.stop(id, Why::Failed { message: format!("the session was not shrunk: {why}") }, Actor::Core);
        }
        let window = self.live.get_mut(id).and_then(|l| l.window.take()).or(after);
        let settled = self.release(id);
        let answer = match (held, held_after) {
            (Some(h), Some(a)) => format!("the session was shrunk: {} → {}", context::k(h), context::k(a)),
            _ => "the session was shrunk".to_string(),
        };
        let mut data = json!({ "ok": true, "answer": answer, "reason": null, "todo_left": 0, "changed": Changed::of(&settled), "window": window });
        if let Some(row) = self.book.get(id)
            && let Some(session) = row.session.as_ref()
            && let Some(ctx) = context::turn(&self.book.of_session(row.spec.thread.as_deref(), session), id, self.plans.now().cache_ms(&session.doer), context::Price::of(&self.book, &session.doer, &row.model, &self.plans.now(), &self.settings.now().settings).as_ref())
        {
            data["ctx"] = ctx;
        }
        self.put(id, "task.settled", Actor::Core, data);
    }

    /// What the loop knows must stay when a session is shrunk, from the task and the ones it carried on from.
    fn must_keep(&self, id: &str, notes: &[String]) -> MustKeep {
        let Some(row) = self.book.get(id) else { return MustKeep::default() };
        let mut chain = vec![row.clone()];
        let mut at = row.spec.follows.clone();
        while let Some(prev) = at.take().and_then(|f| self.book.get(&f)).filter(|_| chain.len() < 64) {
            at = prev.spec.follows.clone();
            chain.push(prev);
        }
        let todo = chain.get(1).filter(|b| b.todo_left > 0).map(|b| format!("{} item(s) still open", b.todo_left));
        // a `:compact` is itself a line the operator typed, and no goal: the goal is the newest line of the chain that was worked on
        let worked = chain.iter().skip(1).find(|r| !r.spec.shrinks_only()).map(|r| r.spec.goal.clone());
        chain.reverse();
        let mut changed: Vec<String> = Vec::new();
        for r in &chain {
            for p in r.changed.created.iter().chain(&r.changed.modified) {
                if !changed.contains(p) {
                    changed.push(p.clone());
                }
            }
        }
        let all = |pick: fn(&super::Row) -> &Vec<String>| -> Vec<String> { chain.iter().flat_map(|r| pick(r).iter().cloned()).collect() };
        // the thread's last operator line; the origin (the first task's goal) is a planning turn's prompt when a plan began the chain
        let said = if row.spec.shrinks_only() { worked } else { row.spec.thread.as_deref().and_then(|t| self.book.said(t)) }.filter(|s| !s.trim().is_empty());
        let goal = said.unwrap_or_else(|| if row.origin.is_empty() { row.spec.goal.clone() } else { row.origin.clone() });
        let how = row.spec.input.get("compact_how").and_then(Json::as_str).map(str::to_string);
        MustKeep { goal, todo, decisions: all(|r| &r.decisions), changed, prompts: all(|r| &r.prompts), notes: clip_notes(notes.to_vec()), how }
    }

    /// The rules answer an event.
    fn react(&mut self, ev: &Ev) {
        if self.rules.is_empty() {
            return;
        }
        // time spent waiting for an answer is not quiet: the waits of the task stop while it waits and count from the resume
        match (ev.kind.as_str(), ev.task.as_deref()) {
            ("task.waiting", Some(task)) => self.rstate.hold_waits(task).into_iter().for_each(|t| {
                let _ = self.timers.cancel(t);
            }),
            ("task.resumed", Some(task)) => {
                for (rule, key, ms) in self.rstate.resume_waits(task, ev.at_ms) {
                    match self.timers.after(ms, Timer::new(RULE).data(json!({ "rule": rule, "key": key }))) {
                        Ok(t) => self.rstate.rearm(rule, &key, t),
                        Err(_) => self.rstate.drop_wait(rule, &key),
                    }
                }
            }
            _ => {}
        }
        let (rules, now) = (self.rules.clone(), self.clock.now_ms());
        let acts = {
            let snap = self.settings.now();
            let env = RuleEnv::new(ev, &self.folds, &snap.tree, now);
            rules.react(&mut self.rstate, ev, &env, now)
        };
        acts.into_iter().for_each(|a| self.run_act(a));
    }

    /// A rule's wait is over.
    fn rule_due(&mut self, data: &Json, timer: u64) {
        let (Some(rule), Some(key)) = (data.get("rule").and_then(Json::as_u64), data.get("key").and_then(Json::as_str)) else { return };
        let now = self.clock.now_ms();
        let ev = match self.rstate.take_due(rule as usize, key, timer, now) {
            Due::Stale => return,
            Due::Again(left) => {
                // something matched since: the quiet is counted from that
                let again = Timer::new(RULE).data(json!({ "rule": rule, "key": key }));
                match self.timers.after(left, again) {
                    Ok(t) => self.rstate.rearm(rule as usize, key, t),
                    Err(_) => self.rstate.drop_wait(rule as usize, key),
                }
                return;
            }
            Due::Fire(ev) => *ev,
        };
        let rules = self.rules.clone();
        let act = {
            let snap = self.settings.now();
            let env = RuleEnv::new(&ev, &self.folds, &snap.tree, now);
            rules.due(&mut self.rstate, rule as usize, &ev, &env, now)
        };
        act.into_iter().for_each(|a| self.run_act(a));
    }

    fn dropped(&mut self, act: &RuleAct, why: &str) {
        let mut n = New::of("rule.dropped", Actor::Core, json!({ "rule": act.rule, "why": why, "cause": act.cause }));
        n.task = act.task.clone();
        if let Err(p) = self.log.append(n) {
            self.failed.get_or_insert(p);
        }
    }

    fn run_act(&mut self, act: RuleAct) {
        match act.what.clone() {
            Do::Dropped(why) => self.dropped(&act, &why),
            Do::Arm { rule, key, ms, ev } => {
                if self.rstate.renew(rule, &key, &ev) {
                    return;
                }
                match self.timers.after(ms, Timer::new(RULE).data(json!({ "rule": rule, "key": key }))) {
                    Ok(timer) => self.rstate.arm(rule, &key, *ev, timer, ms),
                    Err(p) => self.dropped(&act, &p.msg),
                }
            }
            _ => self.apply(&act),
        }
    }

    /// Carry out what a rule asked for. What is written meanwhile answers the event the rule answered.
    fn apply(&mut self, act: &RuleAct) {
        let by = Actor::Rule(act.rule.clone());
        self.acting = Some((act.cause, act.depth));
        let then = match act.what {
            Do::Start { .. } => "start",
            Do::Steer(_) => "steer",
            Do::Notice(_) => "notice",
            Do::Alert { .. } => "alert",
            Do::Park => "park",
            Do::Label { .. } => "label",
            Do::Propose(_) => "propose",
            _ => "cool",
        };
        let mut fired = New::of("rule.fired", by.clone(), json!({ "rule": act.rule, "then": then }));
        (fired.task, fired.thread) = (act.task.clone(), act.thread.clone());
        let fired = self.append(fired);
        if fired.is_some()
            && let Err(p) = self.carry_out(act, &by)
        {
            self.dropped(act, &p.msg);
        }
        self.acting = None;
    }

    fn carry_out(&mut self, act: &RuleAct, by: &Actor) -> Result<(), Problem> {
        let about = || act.task.clone().ok_or_else(|| refused("rule", "the event is not about a task"));
        match &act.what {
            Do::Start { text, follow, doer, place, detached, compact, keep } => {
                if act.task.as_deref().and_then(|t| self.book.get(t)).is_some_and(|r| is_branch(&r.spec)) {
                    return Err(refused("rule", "the event is a branch's, and a branch starts no branch (depth one)"));
                }
                let new = if *follow {
                    self.resume_with(&about()?, Some(text.clone()), compact.then(|| keep.clone()), by.clone())?
                } else {
                    // what it begins is like the task of the event if there is one, else it needs a doer of its own
                    let row = act.task.as_deref().and_then(|t| self.book.get(t));
                    let doer = doer.clone().or_else(|| row.as_ref().map(|r| r.spec.doer.clone())).ok_or_else(|| refused("rule", "it names no doer, and the event has no task to take one from"))?;
                    let mut spec = TaskSpec::new(text, &doer);
                    spec.thread = row.as_ref().and_then(|r| r.spec.thread.clone()).or_else(|| act.thread.clone()).filter(|_| !*detached);
                    if let Some(r) = row {
                        (spec.place, spec.tune) = (r.spec.place, r.spec.tune);
                    }
                    match place.as_deref() {
                        Some("@same") => spec.place = format!("@{}", about()?),
                        Some(p) => spec.place = p.to_string(),
                        None => {}
                    }
                    self.spawn(spec, by.clone())?
                };
                self.rstate.mark_task(&new, act.depth + 1);
                Ok(())
            }
            Do::Steer(text) => self.ctl(&about()?, Ctl::Steer(text.clone()), by.clone()),
            Do::Notice(text) => {
                let mut n = New::of("run.notice", by.clone(), json!({ "text": text, "rule": act.rule }));
                n.task = act.task.clone();
                n.thread = act.task.as_deref().and_then(|t| self.book.thread(t)).or_else(|| act.thread.clone());
                self.append(n);
                self.guard()
            }
            Do::Alert { incident, like, class, done, source, text } => {
                let mut n = New::of("guard.alert", by.clone(), json!({ "kind": "incident", "incident": incident, "like": like, "class": class, "done": done, "source": source, "text": text }));
                n.task = act.task.clone();
                n.thread = act.task.as_deref().and_then(|t| self.book.thread(t)).or_else(|| act.thread.clone());
                self.append(n);
                self.guard()
            }
            Do::Park => {
                self.stop(&about()?, Why::Capped, by.clone());
                Ok(())
            }
            Do::Cool { ms } => self.cool(&about()?, *ms, by.clone(), &act.rule),
            Do::Label { is, of, subject, producer, model, point, weight } => {
                let data = json!({ "subject_kind": of, "subject": subject, "producer": producer, "model": model, "point": point, "label": is, "weight": weight });
                let mut n = New::of("label", by.clone(), data);
                (n.task, n.thread) = (act.task.clone(), act.thread.clone());
                self.append(n);
                self.guard()
            }
            Do::Propose(draft) => {
                if self.proposals.as_ref().is_none_or(|p| p.admits(draft)) {
                    let mut n = New::of(proposal::MADE, by.clone(), json!({ "draft": draft, "widens": [] }));
                    n.cause = Some(act.cause);
                    self.append(n);
                }
                self.guard()
            }
            Do::Arm { .. } | Do::Dropped(_) | Do::Ask { .. } => Ok(()),
        }
    }
}

/// A task the main lane started for an item of its plan (`input.branch`): it is given no way to start a branch.
fn is_branch(spec: &TaskSpec) -> bool {
    spec.input.get("branch").is_some()
}

// ---- who answers ----

/// The ask point of a question that names none: the one of its shape.
fn default_point(shape: &AskShape) -> &'static str {
    if matches!(shape, AskShape::Approval { .. }) { "approval" } else { "question" }
}

impl Engine {
    /// The breaker of an answerer opened: say so once, in the Log, where the operator will see it.
    fn announce_off(&mut self, asker: &str) {
        let Some(asks) = self.asks.clone() else { return };
        for answerer in asks.registry.ids() {
            if let Some(o) = asks.state.just_opened(&answerer) {
                let data = json!({ "answerer": answerer, "until_ms": o.until_ms, "failures": o.failures });
                self.put(asker, "answerer.off", Actor::Core, data);
            }
        }
    }

    /// Walk the chain of the question's ask point on a thread of its own (an answerer may take its time); the outcome comes back
    /// as a message. A question with no ask point, or one the walk cannot start for, is the operator's.
    fn start_walk(&mut self, parent: &str, child: &str) {
        let Some(asks) = self.asks.clone() else { return };
        self.announce_off(parent);
        let Some(spec) = self.live.get(parent).and_then(|l| l.ask.as_ref()).map(|a| a.spec.clone()) else { return };
        let point_id = spec.point.clone().unwrap_or_else(|| default_point(&spec.shape).to_string());
        let Some(point) = asks.points.get(&point_id).cloned() else {
            let why = format!("there is no ask point `{point_id}`: the operator answers");
            self.put(parent, "ask.note", Actor::Core, json!({ "ask": child, "why": why }));
            return;
        };
        let goal = self.book.get(parent).map(|r| r.spec.goal).unwrap_or_default();
        let q = Question {
            ask: child.to_string(),
            point: point.id.clone(),
            asker: parent.to_string(),
            thread: self.book.thread(parent),
            text: spec.question.clone(),
            shape: spec.shape.clone(),
            goal,
            act: spec.act.as_deref().cloned(),
        };
        if let Some(open) = self.live.get_mut(parent).and_then(|l| l.ask.as_mut()) {
            open.walked = Some((point.clone(), q.clone()));
        }
        asks.remember(&q);
        self.walk_on(parent, child, point, q);
    }

    /// Walk `point`'s chain for `q`, the question of the ask `child` of `parent`, on a thread of its own; the outcome comes back as a message.
    fn walk_on(&mut self, parent: &str, child: &str, point: AskPoint, q: Question) {
        let Some(asks) = self.asks.clone() else { return };
        let (tx, ledger, now) = (self.tx.clone(), self.ledgers.entry(parent.to_string()).or_default().clone(), self.clock.now_ms());
        let child = child.to_string();
        let started = std::thread::Builder::new().name("j4n-walk".into()).spawn(move || {
            let out = catch_unwind(AssertUnwindSafe(|| walk(&point, &q, &asks.registry, &asks.state, &ledger, now, asks.headless))).ok();
            let _ = tx.send(Msg::Walked(child, out));
        });
        if started.is_err() {
            self.put(parent, "ask.note", Actor::Core, json!({ "why": "the chain could not be started: the operator answers" }));
        }
    }

    /// A walk is over. What the answerers did is written whatever came of it; what it came to counts only if the question is
    /// still open (the operator may have answered while it was walking).
    fn on_walked(&mut self, ask: &str, walked: Option<(Walk, Vec<Note>)>) {
        let Some(parent) = self.book.get(ask).and_then(|r| r.spec.parent) else { return };
        let Some((walk, notes)) = walked else {
            let why = "the chain failed: the operator answers";
            return drop(self.put(&parent, "ask.note", Actor::Core, json!({ "ask": ask, "why": why })));
        };
        let spent = Spent::of(&notes);
        for n in notes {
            self.put(&parent, &n.kind, Actor::Core, n.data);
        }
        self.announce_off(&parent);
        let open = self.live.get(&parent).and_then(|l| l.ask.as_ref()).is_some_and(|a| a.id == ask);
        if !open || self.book.get(ask).is_none_or(|r| r.state.is_terminal()) {
            return;
        }
        // the Decider of `approval.inside` answered: its `inside` is a yes while code still reads the line as it did; anything else leaves the approval
        // to what its own chain said
        let put = self.live.get_mut(&parent).and_then(|l| l.ask.as_mut()).map(|a| std::mem::replace(&mut a.inside, Inside::Done));
        let walk = match put {
            Some(Inside::Put(first)) => match self.inside_said(&parent, walk) {
                Ok(o) => return self.settle_machine(ask, &parent, o, "chain", spent),
                Err(why) => {
                    self.put(&parent, "ask.note", Actor::Core, json!({ "ask": ask, "point": crate::gate::inside::POINT, "why": why }));
                    first
                }
            },
            Some(Inside::NotPut) => {
                if let Walk::ToHuman { .. } = walk
                    && let Some((point, q)) = self.inside_question(&parent, ask)
                {
                    if let Some(open) = self.live.get_mut(&parent).and_then(|l| l.ask.as_mut()) {
                        open.inside = Inside::Put(walk);
                    }
                    return self.walk_on(&parent, ask, point, q);
                }
                walk
            }
            _ => walk,
        };
        match walk {
            Walk::Answered(o) => self.settle_machine(ask, &parent, o, "chain", spent),
            Walk::Fallback(o) => self.settle_machine(ask, &parent, o, "fallback", spent),
            Walk::ToHuman { after_ms, why, pair } => {
                // the chain left it to the operator: a signed charter that covers it answers, and says so; nothing else is let through
                if let Some((_, q)) = self.live.get(&parent).and_then(|l| l.ask.as_ref()).and_then(|a| a.walked.clone())
                    && matches!(q.shape, AskShape::Approval { .. })
                    && let Some(o) = self.asks.as_ref().and_then(|a| a.charters.as_ref()).and_then(|c| c.offer(&q))
                {
                    return self.settle_machine(ask, &parent, o, "charter", spent);
                }
                // a grant may have landed while the chain walked
                if let Some((_, q)) = self.live.get(&parent).and_then(|l| l.ask.as_ref()).and_then(|a| a.walked.clone())
                    && matches!(q.shape, AskShape::Approval { .. })
                    && let Some(grant) = q.act.as_ref().zip(self.asks.as_ref().and_then(|a| a.gate.as_ref())).and_then(|(act, gate)| gate.granted(act))
                {
                    let o = Outcome { answer: Answer::Yes, by: GATE.into(), kind: Kind::Rule, conf: Some(1.0), why: format!("allowed by grant:{grant}"), pair: Vec::new() };
                    return self.settle_machine(ask, &parent, o, "chain", spent);
                }
                let point = self.live.get(&parent).and_then(|l| l.ask.as_ref()).and_then(|a| a.walked.as_ref()).map(|(p, _)| p.id.clone());
                self.put(&parent, "ask.note", Actor::Core, json!({ "ask": ask, "point": point, "why": why, "pair": pair_json(&pair) }));
                if let Some(ms) = after_ms {
                    let timer = self.timers.after(ms, Timer::new(AFTER).task(ask)).ok();
                    if let Some(open) = self.live.get_mut(&parent).and_then(|l| l.ask.as_mut()) {
                        open.after = timer;
                    }
                }
            }
            Walk::Skip { why } => {
                self.stop(ask, Why::Failed { message: why.clone() }, Actor::Core);
                self.stop(&parent, Why::Failed { message: format!("nobody could answer its question: {why}") }, Actor::Core);
            }
        }
    }

    /// Under `approval.inside`, the question for the Decider of the point `inside` about the approval `ask` of `parent`: only for a line code read as
    /// inside but for the paths of some Python, built while it runs (`gate::inside`), and only where the point is there.
    fn inside_question(&self, parent: &str, ask: &str) -> Option<(AskPoint, Question)> {
        let (_, q) = self.live.get(parent)?.ask.as_ref()?.walked.clone()?;
        let asks = self.asks.as_ref()?;
        let (gate, act) = (asks.gate.as_ref()?, q.act.as_ref()?);
        if !matches!(q.shape, AskShape::Approval { .. }) {
            return None;
        }
        let Some(crate::gate::inside::Inside::Unsure(why)) = gate.inside(act) else { return None };
        let point = asks.points.get(crate::gate::inside::POINT)?.clone();
        let text = crate::gate::inside::question(&gate.told(act), &why)?;
        let opt = |id: &str, label: &str| crate::ask::Opt { id: id.into(), label: label.into(), recommended: false };
        let shape = AskShape::Choice { options: vec![opt("inside", "inside the project, and harmless: J4n lets it run"), opt("ask", "not sure, or it reaches beyond the project: the operator is asked")] };
        Some((point.clone(), Question { ask: ask.to_string(), point: point.id, asker: parent.to_string(), thread: q.thread, text, shape, goal: q.goal, act: None }))
    }

    /// What the Decider's walk on the point `inside` came to, for the approval of `parent`: the yes it gives, or why it gives none. Only a Decider's
    /// `inside` is a yes, and only while code still reads the line as unsure (never as outside, network, secrets, a push, a deploy or a remote line).
    fn inside_said(&self, parent: &str, walk: Walk) -> Result<Outcome, String> {
        let Walk::Answered(o) = walk else { return Err("the Decider did not answer: the operator is asked".into()) };
        if o.kind != Kind::Decider || o.answer != (Answer::Pick { id: "inside".into() }) {
            return Err(format!("{} said ask: the operator is asked", o.by));
        }
        let act = self.live.get(parent).and_then(|l| l.ask.as_ref()).and_then(|a| a.walked.as_ref()).and_then(|(_, q)| q.act.clone());
        let read = act.and_then(|act| self.asks.as_ref()?.gate.as_ref()?.inside(&act));
        match read {
            Some(crate::gate::inside::Inside::Unsure(_)) => Ok(Outcome { answer: Answer::Yes, by: o.by, kind: Kind::Decider, conf: o.conf, why: format!("inside, harmless: {}", o.why), pair: Vec::new() }),
            other => Err(format!("{} said inside, but code now reads the line as {}: the operator is asked", o.by, other.map_or("off", |r| r.word()))),
        }
    }

    /// A machine answered, or the point's fallback stood in for the operator.
    fn settle_machine(&mut self, ask: &str, parent: &str, mut o: Outcome, via: &str, spent: Spent) {
        let Some(spec) = self.book.get(ask).and_then(|r| r.spec.ask()) else { return };
        // the Gate's no is an answer of its own; only the no that stood in for an absent operator becomes the operator's yes
        // What writes or deletes outside the project, its tmp and what the operator granted is asked at every level, `full` included: the operator
        // answers it, or a grant covers it, and with nobody to ask the no stands
        let outside = spec.act.as_ref().is_some_and(|act| self.asks.as_ref().and_then(|a| a.gate.as_ref()).is_some_and(|g| g.outside(act)));
        if via == "fallback" && self.asks.as_ref().is_some_and(|a| a.full) && matches!(spec.shape, AskShape::Approval { .. }) && !outside {
            o = Outcome { answer: Answer::Yes, by: FULL.into(), kind: Kind::Rule, conf: None, why: "--approval full: nothing blocked it".into(), pair: Vec::new() };
        }
        let by = match o.kind {
            Kind::Decider | Kind::Model => Actor::Model(o.by.clone()),
            _ => Actor::Rule(o.by.clone()),
        };
        if o.by == GATE {
            if let Some(act) = &spec.act {
                let verdict = if o.answer == Answer::Yes { "allow" } else { "block" };
                self.put(parent, "gate.verdict", Actor::Core, json!({ "verb": act.verb, "target": act.target, "verdict": verdict, "why": o.why }));
                if verdict == "block" && act.verb == crate::gate::Verb::Shell && let Some(live) = self.live.get_mut(parent) {
                    live.ran.refuse(&act.target);
                }
            }
            if let Some(id) = grant_in(&o.why) {
                self.put(parent, "grant.used", Actor::Core, json!({ "id": id }));
            }
        }
        let mut extra = json!({ "by": o.by, "kind": o.kind.word(), "conf": o.conf, "why": o.why, "pair": pair_json(&o.pair) });
        // what the walk cost, whole: a number, or `null` when a decider or a model did not say
        if let Some(e) = extra.as_object_mut().filter(|_| o.kind.is_model() || spent != Spent::default()) {
            e.extend(spent.json().as_object().cloned().unwrap_or_default());
        }
        if let Err(p) = self.settle_ask(ask, parent, &spec, &o.answer, by, via, extra) {
            self.stop(parent, Why::Failed { message: p.msg }, Actor::Core);
        }
    }

    /// The operator says yes to an approval and asks to have it remembered (`Ctl::Grant`).
    fn grant(&mut self, id: &str, scope: Scope, by: Actor) -> Result<(), Problem> {
        let row = self.book.get(id).ok_or_else(|| refused("task", format!("no task {id}")))?;
        let Some(ask) = row.spec.ask().filter(|_| row.spec.doer == HUMAN) else { return Err(refused(id, "this is not a question")) };
        if by != Actor::Operator {
            return Err(refused(id, "only the operator gives a grant"));
        }
        if !matches!(ask.shape, AskShape::Approval { .. }) {
            return Err(refused(id, "only an approval can be remembered"));
        }
        if row.state != State::Running {
            return Err(refused(id, format!("this question is over ({})", row.state.word())));
        }
        let parent = row.spec.parent.clone().ok_or_else(|| refused(id, "this question has no asker"))?;
        if scope == Scope::Once {
            return self.answer(id, Answer::Yes, by);
        }
        let gate = self.asks.as_ref().and_then(|a| a.gate.clone()).ok_or_else(|| refused(id, "there is no Gate to remember it in"))?;
        let act = ask.act.clone().ok_or_else(|| refused(id, "this approval names no effect to remember"))?;
        if !gate.offers(&act, &gate.check(&act)).contains(&scope) {
            let why = grant::grantable(&act).err().unwrap_or_else(|| "it is not something to remember".into());
            return Err(refused(id, format!("not remembered: {why}")));
        }
        // a folder or a program is judged from the action as the Gate tells it (the folders the task may reach)
        let told = gate.told(&act);
        let mut events = vec![gate.grants().event(&by, scope, &told).map_err(|m| refused(id, m))?];
        if scope == Scope::Here {
            // the row in the operator's own file is what lasts; a session grant is what takes effect now
            grant::write_here(&self.home, &self.root, &act, &by, self.clock.now_ms(), &mut LogJournal { log: &self.log })
                .map_err(|p| refused(id, format!("not remembered: {}", p.msg)))?;
            events.push(gate.grants().event(&by, Scope::Session, &act).map_err(|m| refused(id, m))?);
        }
        for e in events {
            self.log.append(e.task(&parent))?;
        }
        self.settle_ask(id, &parent, &ask, &Answer::Yes, by, "human", json!({ "grant": scope }))?;
        self.sweep_grants(&gate);
        self.guard()
    }

    /// A grant has landed: a question that was already open (another task's, or one whose walk began before the grant) and that the grant
    /// covers is answered by it now, not left to the operator.
    fn sweep_grants(&mut self, gate: &crate::gate::Gate) {
        let open: Vec<(String, String, AskSpec)> = self.live.iter().filter_map(|(parent, l)| l.ask.as_ref().map(|a| (a.id.clone(), parent.clone(), a.spec.clone()))).collect();
        for (ask, parent, spec) in open {
            let covered = spec.act.as_deref().filter(|_| matches!(spec.shape, AskShape::Approval { .. })).and_then(|act| gate.granted(act));
            if let Some(grant) = covered {
                let o = Outcome { answer: Answer::Yes, by: GATE.into(), kind: Kind::Rule, conf: Some(1.0), why: format!("allowed by grant:{grant}"), pair: Vec::new() };
                self.settle_machine(&ask, &parent, o, "chain", Spent::default());
            }
        }
    }
}
