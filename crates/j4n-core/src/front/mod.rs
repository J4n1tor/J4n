//! The front: where a typed line becomes work. It owns what is said to the
//! loop and nothing the loop owns; the TUI, `j4n run` and the piped mode all say their lines here and read the same Log.
//!
//! A line goes, in this order, to
//! 1. **the binder** (`crate::binder`) while a question is open: it is the answer to one, or the operator is asked which;
//! 2. **the phrase table** (`phrase`): `:fresh`, `:switch`, `:stop` … are commands, in any thread;
//! 3. **the bound doer**, when the thread has one: a line typed while the task works steers it, a line typed after it is the
//!    next turn, in the same session (the same child, when the doer keeps one). No decider, no model answerer, no ladder
//!    runs for it;
//! 4. else **intake**: the question "what is this line?" (the ask point `intake`), then the election of a doer and a model
//!    (a pin names them; otherwise the ask point `who:work`), once. The thread is then bound, and the line is its first turn.
//!
//! What the front writes is events (`chat.say`, `chat.note`, `thread.bound`, `thread.fresh`, `intake.decided`); the rest it
//! asks the loop to do. A thread's binding is the fold [`thread::Binds`].

pub mod access;
pub mod acct;
pub mod accounts;
pub mod archive;
pub mod chat;
pub mod chats;
pub mod messages;
pub mod compact;
pub mod context;
pub mod session;
pub mod command;
pub mod complete;
pub mod connect;
pub mod hints;
pub mod keys;
pub mod library;
mod keyfile;
pub mod lane;
pub mod live;
pub mod ready;
pub mod librarian;
pub mod skills;
pub mod catalogue;
mod chatset;
mod charters;
pub mod envoy;
pub mod watch;
mod links;
pub mod look;
pub mod fonts;
pub mod read;
pub mod clamp;
pub mod digest;
pub mod scout;
pub mod second;
pub mod split;
pub mod steps;
pub mod way;
pub mod lookfile;
pub mod page;
pub mod phrase;
pub mod askview;
pub mod branches;
pub mod charts;
pub mod route;
pub mod validate;
pub mod compose;
pub mod rows;
pub mod screen;
pub mod serve;
pub mod setup;
pub mod window;
pub mod welcome;
pub mod needs;
pub mod hand;
pub mod tiers;
pub mod shape;
pub mod hosts;
pub mod inbox;
pub mod ledger;
pub mod parts;
pub mod models;
pub mod named;
pub mod offload;
pub mod planning;
pub mod replan;
pub mod retry;
pub mod quiz;
pub mod terms;
pub mod reflexes;
mod wait;
pub mod usage;
pub mod thread;
pub mod weigh;
pub mod who;

use crate::ask::{Answer, Opt};
use crate::binder::{self, Bind, Which};
use crate::brief::{self, Brief};
use crate::schema::DataClass;
use crate::change::Actor;
use crate::doer::Doer;
use crate::gate::grant::Scope;
use crate::log::{Log, New};
use crate::proposal::{Accepted, Proposer};
use crate::task::{Asks, Ctl, HUMAN, Mode, State, TaskSpec, Tasks};
use screen::Screen;
use phrase::{Cmd, Phrases};
use serde_json::{Value as Json, json};
use std::sync::Arc;
use std::time::Instant;
use hosts::{Hosts, NoHosts, Want};
use shape::{Desk, Draft};
use who::Who;
use thread::{AnswerRef, Binding, Binds};
use crate::keeper::{How, Keeper};
use crate::lanes::Lanes;

/// The window a Brief is cut to when the doer does not say (tokens).
const WINDOW: usize = 128_000;

/// The answers the ask point `intake` takes. The point's own `options` (`packs/base/asks/points.toml`) say the same.
pub const INTAKE: [&str; 5] = ["answer", "command", "setting", "task", "flow"];

/// What the front is set to.
#[derive(Clone)]
pub struct Cfg {
    pub name: String,
    pub version: String,
    /// The project as the operator is shown it (its folder's name).
    pub project: String,
    /// The one thread of this front.
    pub thread: String,
    /// A doer, and a model, the operator named: the thread is bound to them without an election.
    pub doer: Option<String>,
    pub model: Option<String>,
    pub mode: Mode,
    /// Where the work is done, when the operator named a place (`--place`); else the task's default.
    pub place: Option<String>,
    /// When the process has to be done (`--wall`): a task is given what is left.
    pub deadline: Option<Instant>,
    pub usd_soft: Option<f64>,
    /// How long the quick look waits for its first reply, then for its answer.
    pub look_wait: [std::time::Duration; 2],
    /// Which host serves a model; the default knows none.
    pub hosts: Arc<dyn Hosts>,
    /// What a bound thread does when its host is cooling: `wait`, `move` or `ask`.
    pub failover: String,
    /// The tools J4n follows for changes; none: `:watch` says it is not wired.
    pub upstream: Option<Arc<dyn Upstream>>,
    /// What finds what is wrong with J4n and keeps the person's word on it; none: `:wrong` says it is not wired.
    pub contribute: Option<Arc<dyn Contribute>>,
    /// The lines typed while a task works; none, or its switch off: such a line steers the task at once.
    pub inbox: Option<Arc<dyn inbox::Inbox>>,
    /// The result ledger; none, or its switch off: nothing reports a settled thread but the plan's note.
    pub ledger: Option<Arc<dyn ledger::Ledger>>,
    /// The part runner; none, or its switch off: a plan's inline item is merged when its turn ends well.
    pub parts: Option<Arc<dyn parts::Parts>>,
    /// The models each doer has, which a model word is matched against; the default knows none.
    pub models: Arc<dyn models::ModelLists>,
    /// The pre-flight: Agents › Setup, `view:setup` and the notice of a new folder; none: the agents are not known.
    pub setup: Option<crate::doctor::Check>,
    /// The project's folder: where the claims of a branch's report are looked up (empty: the process's own folder).
    pub root: std::path::PathBuf,
    /// What acts on its own: the setting `lanes` at the start; `:lanes` says another while J4n runs.
    pub lanes: crate::branch::Switch,
    /// The classes of branch a rule may start and what each is given (the pack `lanes`, `packs/lanes/rules/branch.toml`).
    pub branches: crate::branch::Rules,
    /// What a line's branches may take together, in dollars, and how often an item is sent back.
    pub usd_line: f64,
    pub sends: u32,
    /// A piece the folder refuses at laying in goes back to its item (`plan.lay_refused` `retry`); off, it is blocked at once.
    pub lay_retry: bool,
    /// The names a plan item may give as its `check` (the project's `check` rows); none: every item names `none`.
    pub checks: Vec<String>,
    /// The same checks as the command lines they run: what a charter's `checks` is compared with (a shell approval is a line, not a name).
    pub check_lines: Vec<String>,
    /// How often a plan's approach is revised (`plan.revisions`), and how many items a replan may add (`plan.grow`), before the operator is asked.
    pub revisions: u32,
    pub grow: u32,
    /// The doer and model the planning turn (`plan.doer`, `plan.model`) and a review branch (`plan.review_doer`, `plan.review_model`) are pinned to; empty: the thread's own.
    pub planner: Pin,
    pub reviewer: Pin,
    /// The same for the cheap branches, scout and piece (`plan.cheap_doer`, `plan.cheap_model`).
    pub cheap: Pin,
    /// `offload.prices`: the price words the tiers are found by; `need.waived`: the needs the operator went on without.
    pub prices: Vec<(String, f64)>,
    pub waived: Vec<String>,
    /// `pick.hand`: the ask points the operator picks at by hand.
    pub hand: Vec<String>,
    /// How full a window may be, and how often a chain is shrunk, before an inline plan item starts (`compact.at`, `compact.max`).
    pub compact_at: f64,
    pub compact_max: u32,
    /// A session's life (`session.mode`) and how many tokens a session holds before `auto` may end it (`session.big`).
    pub session: session::Life,
    pub session_big: u64,
    /// The project card: the Brief of a session's first turn carries it; none, or off by its setting, and it carries nothing.
    pub card: Option<Arc<dyn ProjectFacts>>,
    /// What an earlier report of the same task claimed: the first turn's Brief carries it as the section `remembered`.
    pub recall: Option<Arc<dyn Remembered>>,
    /// Where the look's pass keeps what it made (`~/.j4n/cache/look.generated.toml`); none: no pass runs.
    pub look_made: Option<std::path::PathBuf>,
    /// The doers that are a key on a gateway, `(doer, gateway)` (`own`, `openrouter`): `/usage` says what each gateway was spent today.
    pub gateways: Vec<(String, String)>,
    /// What a gateway key has left, asked at most every ten minutes and only for `/usage` or a chore that is paid per request; none: not known.
    pub credits: Option<usage::Credits>,
    /// The logins there are, `(doer, account)`, read when asked (`/usage` says every one); none: no account is known.
    pub accounts: Option<Logins>,
    /// Where each login keeps its doers' sessions, for the removal of an archived chat; none: no file is deleted.
    pub homes: Option<archive::Homes>,
    /// Offload: the switch, the cap, the order, and the classes of chore the pack `offload` ships; the default sends nothing away.
    pub offload: crate::offload::Offload,
    /// Seats (`crew.on`): a role for a chat Main briefs; off by default.
    pub crew: crate::crew::Crew,
    /// The signed charters (the fold `charters`): the same handle the engine's rule reads.
    pub charters: crate::charter::Charters,
    /// The doer and the model the operator picked for the main lane (`lane.doer`, `lane.model`, `:setup doer`); the election falls to them when they are ready.
    pub main: Pin,
    /// The skills there are: a task is given the few that fit; none: nothing is picked and `:skill` says the pack is not on.
    pub skills: Option<Arc<crate::skill::Library>>,
    /// Where a line goes by its weight (`weigh.on`): off by default.
    pub weigh: weigh::Weigh,
    /// The quick look of `lanes = orchestrate` and the catalogue it is shown (`orchestrate.*`): off by default.
    pub orchestrate: look::Orchestrate,
    /// The librarian (`librarian.on`): a task's context made for the task and the doer; off by default.
    pub librarian: librarian::Librarian,
    /// Chats (`chats.on`): every prompt its own, the orchestrator above them; off by default.
    pub chats: chats::Chats,
    /// Reflexes (`reflexes.on`): commands that run by themselves when a sentence of the operator's holds; off by default.
    pub reflexes: reflexes::Reflexes,
    /// What else keeps a copy of some settings and takes new ones after an edit: the worktree's `work.apply`, the trust
    /// limits, the probe wait, the kit's host, the watcher.
    pub tunes: Vec<Arc<dyn crate::setting::Tune>>,
    /// Where a provider's key typed in Agents › Providers is kept; none: no key is kept and the row's `⏎` says so.
    pub keys: Option<crate::keys::Keys>,
    /// The free model lists kept for the library (`<home>/cache/models.json`); none: the library has only what J4n read for a model word.
    pub listing: Option<crate::listing::Cache>,
}

/// How the Table is read again from the files (after an edit).
/// What a bench case line is briefed with: the thread to write to, the task's spec, the line, whether the doer's session goes on.
pub type BriefHook = Arc<dyn Fn(Option<&str>, &mut TaskSpec, &str, bool) + Send + Sync>;
pub type Reload = Arc<dyn Fn() -> crate::table::Tables + Send + Sync>;

impl Core {
    fn cfg(&self) -> Arc<Cfg> {
        self.cfg.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// What says the logins there are, `(doer, account)`, as the files say now.
pub type Logins = Arc<dyn Fn() -> Vec<(String, String)> + Send + Sync>;

/// A role's own doer and model, set apart from the thread's binding; either half may be empty.
#[derive(Clone, Default)]
pub struct Pin {
    pub doer: Option<String>,
    pub model: Option<String>,
}

/// The stable facts of the project folder as text, derived from its files; none when the switch is off or there is nothing to say.
pub trait ProjectFacts: Send + Sync {
    fn text(&self) -> Option<String>;
}

/// The claims of an earlier report of the same goal, whose files are as they were, worded as the old report's and not as facts; none
/// when the switch is off or nothing holds.
pub trait Remembered: Send + Sync {
    fn text(&self, goal: &str) -> Option<String>;
}

/// What `:wrong` asks of whatever finds what is wrong with J4n: the person's words become a finding that stays on this machine.
pub trait Contribute: Send + Sync {
    /// Keep `words` as a finding about what the thread just did; the words to say, or why not.
    fn wrong(&self, thread: &str, words: &str) -> Result<String, String>;
}

/// What `:watch` asks of whatever follows the tools' releases.
pub trait Upstream: Send + Sync {
    /// Where each tool stands (running, latest, tested), from what is known; nothing leaves the machine.
    fn show(&self) -> String;

    /// Ask the sources now, quietly, and say what changed; `Err`: why nothing was asked (the switch is off).
    fn once(&self) -> Result<String, String>;

    /// "Plan it": propose a note on the operator's list about the release listed as `id`; the words to say, or why not.
    fn plan(&self, id: &str) -> Result<String, String>;
}

impl Cfg {
    pub fn new(project: &str) -> Cfg {
        Cfg {
            name: "j4n".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            project: project.into(),
            thread: "main".into(),
            doer: None,
            model: None,
            mode: Mode::Write,
            place: None,
            deadline: None,
            usd_soft: None,
            look_wait: [look::LOOK_FIRST, look::LOOK_WAIT],
            hosts: Arc::new(NoHosts),
            failover: "wait".into(),
            upstream: None,
            contribute: None,
            inbox: None,
            ledger: None,
            parts: None,
            models: Arc::new(models::NoLists),
            setup: None,
            root: std::path::PathBuf::new(),
            lanes: Default::default(),
            branches: Default::default(),
            usd_line: crate::branch::LINE_USD,
            sends: crate::branch::SENDS,
            lay_retry: false,
            checks: Vec::new(),
            check_lines: Vec::new(),
            revisions: crate::branch::REVISIONS,
            grow: crate::branch::GROW,
            planner: Pin::default(),
            reviewer: Pin::default(),
            cheap: Pin::default(),
            prices: tiers::parse(tiers::PRICES),
            waived: Vec::new(),
            hand: Vec::new(),
            compact_at: 0.7,
            compact_max: 3,
            session: Default::default(),
            session_big: 40_000,
            card: None,
            recall: None,
            look_made: None,
            gateways: Vec::new(),
            credits: None,
            accounts: None,
            homes: None,
            offload: Default::default(),
            crew: Default::default(),
            charters: Default::default(),
            main: Pin::default(),
            skills: None,
            weigh: Default::default(),
            orchestrate: Default::default(),
            librarian: Default::default(),
            chats: Default::default(),
            reflexes: Default::default(),
            tunes: Vec::new(),
            keys: None,
            listing: None,
        }
    }
}

/// What came of a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Said {
    Nothing,
    /// It answered the open question of this task.
    Answered { ask: String },
    /// It could answer several questions: nothing was sent.
    Which(Vec<Which>),
    /// It was a command of this name.
    Command(String),
    /// It was the first turn of a task, or the next one.
    Started { task: String },
    /// It went to the task that is working.
    Steered { task: String },
    /// The inbox kept it (item `id`, tag `tag`) for the right boundary.
    Kept { id: String, tag: &'static str },
    /// It was not taken, and why.
    Refused(String),
    /// It asked for a page: the screen goes to it.
    Open(String),
    /// It asked to leave.
    Quit,
    /// It asked to go into a chat (`:new`, `:top`): the screen goes there; empty, to the orchestrator.
    Enter(String),
    /// It asked for the list of the chats (`:chats`): the screen opens it.
    Chats,
    /// A question was put to the operator, which chat takes the line (`route:<n>`); nothing was sent.
    Asking { ask: String },
    /// A reflex's judge is looking at the line first: it is sent when the judge has answered, to a new session or as typed.
    Held,
}

/// The act a note offers: `⏎` on an empty composer does it while the note is the chat's newest row.
#[derive(Debug, Clone, PartialEq)]
pub struct Lead {
    pub label: String,
    pub intent: j4n_proto::Intent,
    /// From [`crate::front`]'s line of 50 the screen asks on the card first.
    pub risk: u8,
}

impl Lead {
    /// Go to a page.
    pub fn open(label: &str, node: &str) -> Lead {
        Lead { label: label.into(), intent: j4n_proto::Intent::Open { node: node.into() }, risk: 0 }
    }

    /// Set a setting, as its row in Settings does.
    pub fn set(label: &str, id: &str, value: Json) -> Lead {
        Lead { label: label.into(), intent: j4n_proto::Intent::Set { kind: "setting".into(), id: id.into(), field: "value".into(), value }, risk: 0 }
    }

    /// Say a command, as the palette does.
    pub fn run(label: &str, command: &str, args: &[&str]) -> Lead {
        Lead { label: label.into(), intent: j4n_proto::Intent::Run { command: command.into(), args: args.iter().map(|a| a.to_string()).collect() }, risk: 0 }
    }
}

/// How [`Front::begin`] starts a turn.
struct Turn {
    item: bool,
    compact: bool,
    fresh: bool,
    carry: Option<inbox::Due>,
    /// The line is one the operator typed (not a plan item's turn, a boundary's or a side item): `session.mode each` starts a session for it.
    operator: bool,
    /// The Brief's section `parts`.
    brief: Option<String>,
    /// What this turn's task carries in its input and no other does: the envoy a request is for, what it read from outside.
    input: Option<Json>,
}

struct Core {
    /// What the envoys keep (see [`envoy`]).
    envoys: std::sync::Mutex<envoy::Envoys>,
    tasks: Tasks,
    log: Log,
    binds: Binds,
    asks: Arc<Asks>,
    phrases: Phrases,
    doers: Vec<Arc<dyn Doer>>,
    /// What the front is set to; an edit of a setting puts a new one in (`Cfg::apply`).
    cfg: std::sync::RwLock<Arc<Cfg>>,
    /// Who is elected, and the questions that election walks.
    who: Arc<Who>,
    /// The screen: the pages, the views they draw, the commands and the packs, from the Table.
    screen: std::sync::RwLock<Arc<Screen>>,
    /// The settings every reader shares, and how the files are read again after an edit; none: nothing is read again.
    live: crate::setting::Live,
    plans: Option<crate::cost::LivePlans>,
    reload: Option<Reload>,
    /// How many times the settings changed while J4n runs: a screen that was sent fewer is sent the look and the tree again.
    settings_rev: std::sync::atomic::AtomicU64,
    /// The draft of a form the screen fills (Agents › Add account): a field to the words typed; never a file, never the Log.
    drafts: std::sync::Mutex<std::collections::BTreeMap<String, String>>,
    /// The logins of the operator's own the connectors' probes found (Agents › Add account).
    found: Arc<accounts::Found>,
    /// What the Agents tab has read: the programs found, the lab runs, the canaries of the Log.
    connect: connect::Seen,
    /// What makes, applies and takes back proposals; none, and `:accept` says there is nothing to accept with.
    proposer: Option<Arc<Proposer>>,
    /// `:restart` was said: the screen ends, and whoever runs the front starts J4n again.
    restart: std::sync::atomic::AtomicBool,
    /// The pack `shape`'s desk: the switch, the card and the notes kept; none, and a line is what was typed.
    desk: Option<Arc<dyn Desk>>,
    /// The Log as rows for a screen that shows it; read from the files when a screen first watches it, and kept as events are written.
    logrows: std::sync::Mutex<window::LogRows>,
    /// The look the screens draw once `:look` has said another than the settings named at the start.
    look: std::sync::Mutex<Option<j4n_proto::Look>>,
    /// The look file's pass: what it made, what is missed.
    lookfile: std::sync::Mutex<lookfile::State>,
    /// The models a model word matched more than one of, per thread: the next line that is a number (or one of them) picks.
    picks: std::sync::Mutex<std::collections::BTreeMap<String, Pick>>,
    /// The plan of each thread, a fold of the Log (`keeper`); none, and a line is always a task.
    keeper: Option<Keeper>,
    /// The branches of each thread and the reports that wait for its main lane, a fold of the Log (`lanes`); none, and a lane has none.
    lanes: Option<Lanes>,
    /// One planning turn is taken at a time.
    plan_lock: std::sync::Mutex<()>,
    /// The turns of Main whose review started, and the reviews acted on, while J4n runs ([`second`]).
    second: std::sync::Mutex<std::collections::BTreeSet<String>>,
    /// The doers that failed to start at all in a thread's plan: out of its elections until a new plan begins.
    out: std::sync::Mutex<std::collections::BTreeMap<String, std::collections::BTreeSet<String>>>,
    /// The switch `lanes` as `:lanes` last said it; none: what `Cfg.lanes` was at the start.
    lane: std::sync::Mutex<Option<crate::branch::Switch>>,
    /// The ids the project's docs define (`terms`), read when first asked for and again when a doc changed.
    terms: std::sync::Mutex<terms::Terms>,
    /// The windows and the spend `/usage` shows, read from the Log when first asked for and kept as events are written.
    usage: Arc<std::sync::Mutex<usage::Usage>>,
    /// Offload's own state: the switch and the pick as the operator last said them, the reviews that wait for a yes.
    chores: std::sync::Mutex<offload::State>,
    /// The reflexes the operator added, switched or removed while J4n runs, what each last said, and the yes a chat waits for.
    reflexes: std::sync::Mutex<reflexes::State>,
    /// The questions that wait for the operator's next free line as a yes, by chat and kind.
    waits: std::sync::Mutex<wait::Waits>,
    /// The failed tasks the point `retry` was asked about: once for each.
    retried: std::sync::Mutex<std::collections::BTreeSet<String>>,
    /// The session a change left and the one `:resume` asked for, per thread.
    moves: std::sync::Mutex<session::Moves>,
    /// The question of which chat takes a line, per thread, while it waits; one chat is opened or numbered at a time.
    routes: std::sync::Mutex<std::collections::BTreeMap<String, chats::Pending>>,
    /// The doer the read found a line to ask for, by thread: the election of the line's doer takes it (cleared by the next line).
    asked: std::sync::Mutex<std::collections::BTreeMap<String, read::Asked>>,
    /// The card of a feature's need: held while it waits for its answer, and the needs asked this run.
    needs: needs::Held,
    /// The Setup card: held while it waits for its answer.
    welcome: std::sync::Mutex<Option<j4n_proto::AskView>>,
    /// The splits that wait for the operator's yes, by thread, and the number of the next question.
    splits: std::sync::Mutex<std::collections::BTreeMap<String, split::Held>>,
    split_n: std::sync::atomic::AtomicU64,
    /// The digests of the chats Main started, read from the Log when first asked for.
    digests: std::sync::Mutex<Option<Vec<digest::Digest>>>,
    route_n: std::sync::atomic::AtomicU64,
    chat_lock: std::sync::Mutex<()>,
    /// The messages a turn being started has taken into its Brief and not yet delivered: no second turn takes them.
    mail_taken: std::sync::Mutex<std::collections::BTreeSet<String>>,
    /// The questions about messages that wait for you (`mail:<n>`), and the number of the next.
    mail_asks: std::sync::Mutex<Vec<messages::Asked>>,
    mail_n: std::sync::atomic::AtomicU64,
    /// What the chats of a split are given to read, the requests that wait for Main, and what each chat is told next.
    access: std::sync::Mutex<access::State>,
    /// When this front began (ms): what the orchestrator is told of a chat is what happened since, never what an older Log holds.
    since_ms: u64,
}

/// What a `:switch` offered the operator to choose between.
struct Pick {
    doer: String,
    account: Option<String>,
    options: Vec<String>,
}

/// The front. Clones share it.
#[derive(Clone)]
pub struct Front(Arc<Core>);

/// `lane.model` for the agent the election falls to: the model of its own `list` the word names, else none, so that agent runs on its own
/// default (a pick made for one agent is no word for another). No list at all (a front wired to none) takes the word as typed.
pub(crate) fn main_model_in(word: &str, list: Option<Vec<String>>) -> Option<String> {
    match list.map(|l| models::resolve(word, &l)) {
        None => Some(word.to_string()),
        Some(models::Resolved::One(m)) => Some(m),
        Some(_) => None,
    }
}

impl Front {
    pub fn new(cfg: Cfg, tasks: Tasks, log: Log, binds: Binds, asks: Arc<Asks>, phrases: Phrases, doers: Vec<Arc<dyn Doer>>) -> Front {
        let who = Arc::new(Who::new(asks.clone(), &doers, std::collections::BTreeMap::new(), cfg.hosts.clone(), log.clone()));
        who.hands().set(&cfg.hand);
        let since_ms = log.clock().now_ms();
        let front = Front(Arc::new(Core { envoys: Default::default(), since_ms, tasks, log, binds, asks, phrases, doers, cfg: std::sync::RwLock::new(Arc::new(cfg)), who, screen: std::sync::RwLock::new(Arc::new(Screen::builtin())), live: Default::default(), plans: None, reload: None, settings_rev: Default::default(), drafts: Default::default(), found: Default::default(), connect: Default::default(), proposer: None, restart: Default::default(), desk: None, logrows: Default::default(), look: Default::default(), lookfile: Default::default(), picks: Default::default(), keeper: None, lanes: None, lane: Default::default(), plan_lock: Default::default(), second: Default::default(), out: Default::default(), terms: Default::default(), usage: Default::default(), chores: Default::default(), reflexes: Default::default(), waits: Default::default(), retried: Default::default(), moves: Default::default(), routes: Default::default(), asked: Default::default(), welcome: Default::default(), needs: Default::default(), splits: Default::default(), split_n: Default::default(), digests: Default::default(), route_n: Default::default(), chat_lock: Default::default(), mail_taken: Default::default(), mail_asks: Default::default(), mail_n: Default::default(), access: Default::default() }));
        front.pin_main();
        front
    }

    /// The same front, drawing `screen` (what the Table holds) in place of the base pack's.
    pub fn with_screen(self, screen: Arc<Screen>) -> Front {
        match Arc::try_unwrap(self.0) {
            Ok(mut core) => {
                core.screen = std::sync::RwLock::new(screen);
                Front(Arc::new(core))
            }
            Err(shared) => Front(shared),
        }
    }

    /// The word a command is typed with, when `word` is one (`pack`, `plan`: from a shell it comes without its colon or slash).
    pub fn lead(&self, word: &str) -> Option<String> {
        self.0.phrases.lead(word)
    }

    /// Whether the operator asked to start J4n again (`:restart`) and not just to leave.
    pub fn restart_asked(&self) -> bool {
        self.0.restart.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// The screen as it is now: an edit of a setting makes a new one with the settings it left.
    pub fn screen(&self) -> Arc<Screen> {
        self.0.screen.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The look the screens draw: what the settings named at the start, with what `:look` has said since, and the marks `auto` says
    /// on this machine as their tier (`fonts`).
    pub fn look(&self) -> j4n_proto::Look {
        let look = self.look_spoken();
        if look.marks == fonts::AUTO { j4n_proto::Look { marks: fonts::found().tier().into(), ..look } } else { look }
    }

    /// The look as the operator said it, the marks `auto` still the word.
    pub(crate) fn look_spoken(&self) -> j4n_proto::Look {
        self.0.look.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_else(|| self.screen().look())
    }

    /// The Log as rows, shared by the screens that show it.
    pub fn logrows(&self) -> std::sync::MutexGuard<'_, window::LogRows> {
        self.0.logrows.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The same front, with `proposer` to accept, decline and undo with (`:accept`, `:decline`, `:undo`).
    pub fn with_proposer(self, proposer: Arc<Proposer>) -> Front {
        match Arc::try_unwrap(self.0) {
            Ok(mut core) => {
                core.proposer = Some(proposer);
                Front(Arc::new(core))
            }
            Err(shared) => Front(shared),
        }
    }

    /// The same front, shaping lines with `desk`.
    pub fn with_desk(self, desk: Arc<dyn Desk>) -> Front {
        match Arc::try_unwrap(self.0) {
            Ok(mut core) => {
                core.desk = Some(desk);
                Front(Arc::new(core))
            }
            Err(shared) => Front(shared),
        }
    }

    pub fn desk(&self) -> Option<&Arc<dyn Desk>> {
        self.0.desk.as_ref()
    }

    pub fn proposer(&self) -> Option<&Arc<Proposer>> {
        self.0.proposer.as_ref()
    }

    /// The same front, electing with `who` (which knows what each doer is good for).
    pub fn with_who(self, who: Arc<Who>) -> Front {
        match Arc::try_unwrap(self.0) {
            Ok(mut core) => {
                who.watch_limits(core.usage.clone());
                who.hands().set(&core.cfg.read().unwrap_or_else(|e| e.into_inner()).hand);
                core.who = who;
                Front(Arc::new(core))
            }
            Err(shared) => Front(shared),
        }
    }

    pub fn who(&self) -> &Arc<Who> {
        &self.0.who
    }

    /// What the front is set to now: an edit of a setting puts a new one in.
    pub fn cfg(&self) -> Arc<Cfg> {
        self.0.cfg()
    }

    pub fn tasks(&self) -> &Tasks {
        &self.0.tasks
    }

    pub fn log(&self) -> &Log {
        &self.0.log
    }

    pub fn binding(&self, thread: &str) -> Option<Binding> {
        self.0.binds.get(thread)
    }

    pub fn asks(&self) -> &Asks {
        &self.0.asks
    }

    pub fn phrases(&self) -> &Phrases {
        &self.0.phrases
    }

    fn write(&self, thread: &str, kind: &str, by: Actor, data: Json) -> bool {
        self.0.log.append(New::of(kind, by, data).thread(thread)).is_ok()
    }

    /// Say something to the operator, in the chat of the thread.
    pub fn note(&self, thread: &str, text: &str, tone: &str) {
        self.write(thread, "chat.note", Actor::Core, json!({ "text": text, "tone": tone }));
    }

    fn refuse(&self, thread: &str, why: String) -> Said {
        self.note(thread, &why, "warn");
        Said::Refused(why)
    }

    /// A note that leads somewhere: its row carries the act, which `⏎` on an empty composer does while the row is the
    /// chat's newest. The words name the key and the tab, so a stream that cannot press it still reads where to go.
    pub fn note_to(&self, thread: &str, text: &str, tone: &str, lead: Lead) {
        let act = j4n_proto::Act { key: "enter".into(), label: lead.label, intent: lead.intent, risk: lead.risk };
        self.write(thread, "chat.note", Actor::Core, json!({ "text": text, "tone": tone, "act": act }));
    }

    /// A refusal that carries the act that mends it.
    pub(super) fn refuse_to(&self, thread: &str, why: String, lead: Lead) -> Said {
        self.note_to(thread, &why, "warn", lead);
        Said::Refused(why)
    }

    /// What an edit says: one that waits for the next start (`⟳`) leads to the restart (`⏎`).
    pub(super) fn said_edit(&self, thread: &str, words: &str, tone: &str) {
        if words.contains('⟳') {
            self.note_to(thread, words, tone, Lead::run("restart now", "restart", &[]));
        } else {
            self.note(thread, words, tone);
        }
    }

    /// A command of a pack that is off: `⏎` turns the pack on; Settings › Packs has the same key.
    pub(super) fn pack_off(&self, thread: &str, pack: &str) -> Said {
        self.refuse_to(thread, format!("the pack `{pack}` is not on · ⏎ turns it on (Settings › Packs)"), Lead::run(&format!("turn on {pack}"), "pack", &["on", pack]))
    }

    /// The first start's row in a new folder: when something is missing, `⏎` opens Agents › Setup, whose rows mend each part.
    pub fn first_note(&self, thread: &str, text: &str) {
        if text.contains('⏎') {
            self.note_to(thread, text, "warn", Lead::open("open Setup", "setup"));
        } else {
            self.note(thread, text, "dim");
        }
    }

    /// A command of the chats while they are off: `⏎` turns `chats.on` on, the switch of Settings › All.
    pub(super) fn chats_off(&self, thread: &str, name: &str) -> Said {
        // the switch is on and no screen shows it (a pipe or a run): saying "off" would be wrong, and turning it on does nothing
        if self.0.cfg().chats.set {
            return self.refuse(thread, format!("the chats are on, but only a screen shows them: `:{name}` has none in a pipe or a run · start J4n on a screen"));
        }
        self.refuse_to(thread, format!("the chats are off, so `:{name}` has none · ⏎ turns them on (Settings › All › chats.on)"), Lead::set("turn the chats on", "chats.on", json!(true)))
    }

    /// A task that runs and is in the way: `⏎` stops it, on the card.
    pub(super) fn busy(&self, thread: &str, id: &str, then: &str) -> Said {
        let lead = Lead { label: format!("stop {id}"), intent: j4n_proto::Intent::Control { task: id.into(), control: j4n_proto::Control::Abort }, risk: 60 };
        self.refuse_to(thread, format!("{id} is still going: wait for it, or ⏎ stops it{then} (Tasks › x, or `:stop`)"), lead)
    }

    /// The newest task of the thread that a doer works on (not a question).
    fn last_task(&self, thread: &str) -> Option<crate::task::Row> {
        self.0.tasks.book().rows().into_iter().rev().find(|r| r.spec.thread.as_deref() == Some(thread) && r.spec.doer != HUMAN && r.spec.parent.is_none())
    }

    /// Law 9's settled boundary, said once: `None` while the thread's newest task runs, else that task (`Some(None)`: the thread has none). A hook of
    /// `plan_step` asks when it runs, not before: the hook ahead of it may have started a task.
    pub(super) fn settled(&self, thread: &str) -> Option<Option<crate::task::Row>> {
        Some(self.last_task(thread)).filter(|r| r.as_ref().is_none_or(|r| r.state.is_terminal()))
    }

    /// A line typed by the operator.
    pub fn say(&self, thread: &str, line: &str) -> Said {
        self.say_with(thread, line, &[])
    }

    /// A line with the rows the operator kept beside it (`Intent::Say.with`): the line is what was typed, and the rows ride in the
    /// Brief of the turn it starts.
    pub fn say_with(&self, thread: &str, line: &str, keys: &[String]) -> Said {
        let text = line.trim();
        if text.is_empty() {
            return Said::Nothing;
        }
        // a `:key` line that is not `:key remove <connector>` may hold a key typed where it does not go: the Log keeps the word only
        if self.key_masked(text) {
            return self.key_stray(thread, text);
        }
        if !self.write(thread, "chat.say", Actor::Operator, json!({ "text": text })) {
            return Said::Refused("the Log does not take events".into());
        }
        self.0.who.said();
        // a feature the line would use that lacks what it needs says so before it acts
        self.need_check(thread);
        self.plan_collect(thread);
        // a reflex that waits for the operator's yes takes it; any other line, command or task ends the question
        if let Some(said) = self.reflex_answer(thread, text) {
            return said;
        }
        // a split that waits for a yes takes `yes` and `no`; any other line ends it
        if let Some(said) = self.split_typed(thread, text) {
            return said;
        }
        // a question of which chat takes a line, waiting here, is answered before the questions of the chats: its `1` is its own
        if let Some(said) = self.route_picked(thread, text) {
            return said;
        }
        // the questions of every chat are answered from the orchestrator
        let orchestrating = self.chats_on() && thread == self.0.cfg().thread;
        // a question of the work an envoy holds is answered from the envoy's chat, and from no other
        let open: Vec<_> = self
            .0
            .tasks
            .book()
            .open_asks()
            .into_iter()
            .filter(|a| match self.envoy_of_task(&a.asker) {
                Some(chat) => chat == thread,
                None => a.thread == thread || (orchestrating && self.0.binds.chat(&a.thread).is_some()),
            })
            .collect();
        if open.is_empty()
            && let Some(said) = self.picked(thread, text)
        {
            return said;
        }
        match binder::bind(text, &open, None) {
            Bind::Answer { ask, answer } => return self.answer(thread, &ask, answer),
            Bind::Which(list) => {
                let menu: Vec<String> = list.iter().enumerate().map(|(i, w)| format!("{}. {}", i + 1, w.question)).collect();
                self.note(thread, &format!("Which question do you mean?\n{}", menu.join("\n")), "warn");
                return Said::Which(list);
            }
            Bind::NotAnswer => {}
        }
        // the one question about a message that waits is answered by its number or its word (`hold`, `send`), before a word is taken for a command's wording
        if let Some(said) = self.mail_picked(thread, text) {
            return said;
        }
        // while only J4n's own steps work in Main (a check, a review), `skip` and `stop` end them, and a plain line ends them and goes on as Main's
        if let Some(said) = self.step_line(thread, text) {
            return said;
        }
        if let Some(cmd) = self.0.phrases.parse(text) {
            // `:ask <line>`: the line as it is, to the model (the way back from a line the table took for a command)
            if cmd.name == "ask" {
                return self.carry_on(thread, &cmd.args.join(" "), keys);
            }
            // a command that says what it takes, given something else: its usage, and nothing is started
            if let Some(usage) = self.0.phrases.misuse(text) {
                return self.refuse(thread, usage);
            }
            return self.command(thread, cmd);
        }
        // a colon opens a command: a word that is none is a typo, and a paid turn is no answer to one
        if text.starts_with(':') {
            // a command of a pack that is off: the way out is to turn it on
            let word = text.split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
            if let Some(pack) = self.screen().packs.iter().find(|p| !p.on && p.note.is_empty() && p.words.contains(&word)).map(|p| p.id.clone()) {
                return self.pack_off(thread, &pack);
            }
            return self.refuse(thread, self.0.phrases.unknown(text));
        }
        // a line in plain language that is one of the wordings of a command runs it, with no model asked
        if let Some(t) = self.0.phrases.take(text) {
            let by = t.cmd.name.clone();
            self.write(thread, "intake.decided", Actor::Core, json!({ "pick": "command", "by": "front:phrases", "why": format!("the line is a wording of `{by}`: `{}`", t.wording) }));
            self.note(thread, &format!("took this as `{}`; to send it to the model as it is: `:ask {text}`", Phrases::said(&t.cmd)), "dim");
            return self.command(thread, t.cmd);
        }
        // `@c4 words`: a line of yours for c4 from where you are
        if let Some(said) = self.at_line(thread, text) {
            return said;
        }
        // under the switch a plain line is read once: what it is, whether it goes on, which chat it is about, which doer it asks for
        let read = self.read_plain(thread, text, keys);
        // a line the read found to be about a chat goes there
        if let Some(said) = self.route_line(thread, text, keys, read.as_ref()) {
            return said;
        }
        // a reflex whose check holds for the line has its judge look at it first: the line waits, bounded, and goes to a new session or on as typed
        if let Some(said) = self.reflex_hold(thread, text, keys) {
            return said;
        }
        match self.quick_look(thread, text, read) {
            split::After::Send(Some(told)) => self.send_line(thread, &told, keys),
            split::After::Done(said) => said,
            split::After::Send(None) => self.send_line(thread, text, keys),
        }
    }

    /// A plain line goes on: to a cheaper doer when it is a light chore, where its weight says, or to the chat's doer.
    pub(super) fn send_line(&self, thread: &str, text: &str, keys: &[String]) -> Said {
        // a light chore goes to a cheaper doer on its own; `:ask <line>` is the way back to the main lane
        if let Some(said) = self.chore_take(thread, text) {
            return said;
        }
        // a line is weighed, with `weigh.on`, and goes where its weight says; `:ask <line>` is the way back to the chat's doer
        if let Some(said) = self.weigh_take(thread, text, keys) {
            return said;
        }
        self.carry_on(thread, text, keys)
    }

    /// A line that is neither an answer nor a command: it steers the plan, goes on in the thread's session, or is the first line of a thread.
    fn carry_on(&self, thread: &str, text: &str, keys: &[String]) -> Said {
        if text.trim().is_empty() {
            return Said::Nothing;
        }
        if let Some(said) = self.fold_line(thread, text, keys) {
            return said;
        }
        match self.binding(thread) {
            Some(b) => {
                let b = self.moved(thread, text, b);
                self.turn(thread, text, &b, keys)
            }
            None => self.first(thread, text, keys),
        }
    }

    /// Give an answer to a question of `ask`'s own task.
    fn answer(&self, thread: &str, ask: &str, answer: Answer) -> Said {
        match self.0.tasks.ctl(ask, Ctl::Answer(answer), Actor::Operator) {
            Ok(()) => Said::Answered { ask: ask.into() },
            Err(p) => self.refuse(thread, p.msg),
        }
    }

    /// The operator's answer to a question, from a button or a key.
    pub fn reply(&self, thread: &str, ask: &str, reply: j4n_proto::Reply) -> Said {
        use j4n_proto::Reply;
        if let Some(said) = self.route_reply(ask, &reply).or_else(|| self.mail_reply(ask, &reply)).or_else(|| self.welcome_reply(ask, &reply)).or_else(|| self.need_reply(thread, ask, &reply)).or_else(|| self.hand_reply(thread, ask, &reply)).or_else(|| self.split_reply(ask, &reply)) {
            return said;
        }
        let answer = match reply {
            Reply::Yes => Answer::Yes,
            Reply::No => Answer::No,
            Reply::Pick { id } => Answer::Pick { id },
            // a line typed while the card of the question is up (the custom row) is for that question: an option's number or words pick it, any
            // other line is the operator's own answer
            Reply::Text { text } => {
                let open = self.0.tasks.book().open_asks();
                match open.iter().find(|a| a.ask == ask).map(|a| binder::bind(&text, std::slice::from_ref(a), Some(ask))) {
                    Some(Bind::Answer { ask, answer }) => return self.answer(thread, &ask, answer),
                    _ => Answer::Text { text },
                }
            }
            Reply::Grant { scope } => {
                let scope = match scope.as_str() {
                    "once" => Scope::Once,
                    "task" => Scope::Task,
                    "session" => Scope::Session,
                    "here" => Scope::Here,
                    "folder" => Scope::Folder,
                    "program" => Scope::Program,
                    other => return self.refuse(thread, format!("`{other}` is not something a yes can be kept for (once, task, session, here, folder, program)")),
                };
                return match self.0.tasks.ctl(ask, Ctl::Grant(scope), Actor::Operator) {
                    Ok(()) => Said::Answered { ask: ask.into() },
                    Err(p) => self.refuse(thread, p.msg),
                };
            }
        };
        self.answer(thread, ask, answer)
    }

    /// An order to a task, from a button.
    pub fn control(&self, thread: &str, task: &str, control: j4n_proto::Control) -> Said {
        let (ctl, said) = match control {
            j4n_proto::Control::Steer { text } => (Ctl::Steer(text), Said::Steered { task: task.into() }),
            j4n_proto::Control::Abort => (Ctl::Abort, Said::Command("stop".into())),
            j4n_proto::Control::Kill => (Ctl::Kill, Said::Command("kill".into())),
        };
        match self.0.tasks.ctl(task, ctl, Actor::Operator) {
            Ok(()) => said,
            Err(p) => self.refuse(thread, p.msg),
        }
    }

    // ---- a thread that has a doer ----

    /// The next turn of a bound thread: steer the task that works, or carry on in its session.
    fn turn(&self, thread: &str, text: &str, b: &Binding, keys: &[String]) -> Said {
        self.turn_as(thread, text, b, keys, (true, false, true))
    }

    /// [`turn`](Front::turn); `item`: the turn works on the current item of the open plan (a reply that touches none is not one).
    fn turn_as(&self, thread: &str, text: &str, b: &Binding, keys: &[String], (item, compact, operator): (bool, bool, bool)) -> Said {
        let last = self.last_task(thread);
        if let Some(row) = last.as_ref().filter(|r| !r.state.is_terminal()) {
            if let Some(said) = self.inboxed(thread, text, row) {
                return said;
            }
            return match row.state {
                State::Running => match self.0.tasks.ctl(&row.id, Ctl::Steer(text.into()), Actor::Operator) {
                    Ok(()) => Said::Steered { task: row.id.clone() },
                    Err(_) if self.0.tasks.book().get(&row.id).is_some_and(|r| r.state.is_terminal()) => return self.turn_as(thread, text, b, keys, (item, compact, operator)), // it ended between the look and the order: the line is the next turn
                    Err(p) => self.refuse(thread, p.msg),
                },
                State::Waiting { .. } => self.refuse(thread, self.waiting_words(&row.id)),
                _ => self.busy(thread, &row.id, ""),
            };
        }
        self.begin(thread, text, b, keys, Turn { item, compact, fresh: false, carry: None, operator, brief: None, input: None })
    }

    /// What a line typed with no card is told while a question of `task` waits: the question and the keys that answer it, not the id of the task.
    /// The keys are the ones the card has (`1–3` pick, `c` brings the card with its own line, `y`/`n` for a yes or no).
    fn waiting_words(&self, task: &str) -> String {
        let open = self.0.tasks.book().open_asks();
        let Some(a) = open.iter().find(|a| a.asker == task || a.ask == task) else { return format!("{task} is waiting for your answer") };
        let question = crate::text::line(&a.question.split_whitespace().collect::<Vec<_>>().join(" "), 60);
        let keys = match &a.shape {
            crate::ask::AskShape::Choice { options } => match options.len() {
                0 | 1 => "1 pick · c type your own".to_string(),
                n => format!("1–{n} pick · c type your own"),
            },
            crate::ask::AskShape::Text => "c type the answer".to_string(),
            crate::ask::AskShape::YesNo | crate::ask::AskShape::Approval { .. } => "y yes · n no".to_string(),
        };
        format!("{} asks \"{question}\" · {keys}", a.ask)
    }

    /// The start of a task for a line: a turn takes what the inbox keeps for it into the section `inbox` of its Brief (a doer that takes
    /// none gets it only as the boundary's line), and says so in the items' receipts; a side item is `fresh`, in no session.
    fn begin(&self, thread: &str, text: &str, b: &Binding, keys: &[String], turn: Turn) -> Said {
        let b = &self.account_route(thread, &self.failover(thread, b));
        let brief = self.0.doers.iter().any(|d| d.id() == b.doer && d.caps().brief);
        let cfg_ = self.0.cfg();
        let inbox = cfg_.inbox.as_ref().filter(|i| i.on());
        let operator = turn.operator;
        let due = turn.carry.or_else(|| inbox.filter(|_| brief && !turn.fresh).and_then(|i| i.due(thread, false)));
        let text = due.as_ref().filter(|d| !brief && text == d.line).map_or(text, |d| d.plain.as_str());
        let ids = due.as_ref().map(|d| d.ids.clone()).unwrap_or_default();
        let back = |said: Said| {
            if let Some(i) = inbox.filter(|_| !matches!(said, Said::Started { .. })) {
                i.back(&ids);
            }
            said
        };
        let mut spec = match self.spec(thread, text, &b.doer) {
            Ok(spec) => spec,
            Err(why) => return back(self.refuse(thread, why)),
        };
        spec.role = Some("work".into());
        spec.tune.model = b.model.clone();
        spec.tune.endpoint = b.endpoint.clone();
        spec.account = b.account.clone();
        for (k, v) in turn.input.iter().flat_map(|i| i.as_object()).flatten() {
            spec.input[k] = v.clone();
        }
        // what waits for this chat is carried in this turn's Brief (a side item is in no session and takes none)
        let mail = if turn.fresh { Vec::new() } else { self.mail_attach(thread, &mut spec) };
        let in_session = !turn.fresh && self.follow(thread, &b.doer, &mut spec, operator);
        if turn.compact && in_session {
            self.compact_between(thread, &mut spec);
        }
        let shape = self.kept(thread, text, keys);
        self.briefed(thread, &mut spec, text, in_session, (shape.as_deref(), due.as_ref().map(|d| d.text.as_str()), turn.brief.as_deref()));
        match self.0.tasks.spawn(spec, Actor::Operator) {
            Ok(task) => {
                if turn.item {
                    self.plan_started(thread, &task);
                }
                self.mail_delivered(thread, &task, &mail);
                if let Some(i) = inbox.filter(|_| !ids.is_empty()) {
                    i.sent(&ids, "delivered", &task);
                }
                Said::Started { task }
            }
            Err(p) => {
                self.mail_released(&mail);
                back(self.refuse(thread, p.msg))
            }
        }
    }

    /// The settled boundary. A task of the thread has ended well and nothing runs: what the inbox kept goes to the next turn of the same
    /// session as the section `inbox` (steer and hold lines as typed, numbered, and the pinned notes), else the oldest `side` item starts a
    /// task of its own. After a stop, a failure or a cap nothing starts: the items ride with the next turn the operator starts.
    pub fn inbox_step(&self, thread: &str) {
        let cfg_ = self.0.cfg();
        let Some(inbox) = cfg_.inbox.as_ref().filter(|i| i.on()) else { return };
        if self.settled(thread).flatten().is_none_or(|r| r.state != (State::Done { ok: true })) {
            return;
        }
        let Some(b) = self.binding(thread) else { return };
        if let Some(due) = inbox.due(thread, true) {
            let line = due.line.clone();
            self.begin(thread, &line, &b, &[], Turn { item: false, compact: false, fresh: false, carry: Some(due), operator: false, brief: None, input: None });
        } else if let Some((id, text)) = inbox.side(thread) {
            match self.begin(thread, &text, &b, &[], Turn { item: false, compact: false, fresh: true, carry: None, operator: false, brief: None, input: None }) {
                Said::Started { task } => inbox.sent(&[id], "started", &task),
                _ => inbox.back(&[id]),
            }
        }
    }

    /// A line typed over a task that is not over goes to the inbox first, when it is on. `stop` is sent at once, in any state; any other
    /// tag is kept, except for a task that waits on an ask (the answer is what it waits for). `None`: the front does what it always did.
    fn inboxed(&self, thread: &str, text: &str, row: &crate::task::Row) -> Option<Said> {
        let cfg_ = self.0.cfg();
        let inbox = cfg_.inbox.as_ref().filter(|i| i.on())?;
        let context = [row.spec.goal.as_str(), row.origin.as_str(), &[row.changed.created.as_slice(), row.changed.modified.as_slice()].concat().join(" "), row.active.as_str()].join(" ");
        let kept = inbox.take(&inbox::Over { thread, text, task: &row.id, state: row.state.word(), context })?;
        // a `steer` line of a rule that has earned it, in `auto`, goes into a task that takes a steer while it works
        if kept.now && row.state == State::Running && self.0.doers.iter().any(|d| d.id() == row.spec.doer && d.caps().live) && self.0.tasks.ctl(&row.id, Ctl::Steer(text.into()), Actor::Operator).is_ok() {
            inbox.sent(std::slice::from_ref(&kept.id), "delivered", &row.id);
            self.note(thread, &format!("{} → steered {} at once: this rule's tags have held", kept.id, row.id), "dim");
            return Some(Said::Steered { task: row.id.clone() });
        }
        if kept.tag != "stop" {
            self.note(thread, &kept.note, "dim");
            return Some(Said::Kept { id: kept.id, tag: kept.tag });
        }
        let aborted = self.0.tasks.ctl(&row.id, Ctl::Abort, Actor::Operator);
        inbox.settled(&kept.id, if aborted.is_ok() { "stopped-task" } else { "dropped" });
        Some(match aborted {
            Ok(()) => Said::Command("stop".into()),
            Err(p) => self.refuse(thread, p.msg),
        })
    }

    /// The next inline plan item starts in a session whose last turn left the window past `compact.at`: shrink it first, keeping
    /// the plan line, unless this chain has had `compact.max` already (law 2: the start of a task is a settled boundary).
    fn compact_between(&self, thread: &str, spec: &mut TaskSpec) {
        let Some(prev) = spec.follows.as_deref().and_then(|f| self.0.tasks.book().get(f)) else { return };
        if prev.window.is_none_or(|w| w <= self.0.cfg().compact_at) {
            return;
        }
        if prev.compactions >= self.0.cfg().compact_max {
            return self.note(thread, &format!("the window is {:.0}% full and this chain has been shrunk {} times already: the item starts as it is", prev.window.unwrap_or(0.0) * 100.0, prev.compactions), "warn");
        }
        spec.compact = Some(self.plan_line(thread).into_iter().collect());
    }

    /// The thread's own tasks with `doer`, newest first, back to the last `:fresh` (what it ended is not looked at): the turns of its
    /// lines, not a question, a step of a flow or anything a task began.
    pub fn own_tasks(&self, thread: &str, doer: &str) -> Vec<crate::task::Row> {
        let cut = self.0.binds.cut(thread);
        let since = self.0.tasks.book().rows().into_iter().rev().take_while(|r| Some(&r.id) != cut.as_ref());
        since.filter(|r| r.spec.thread.as_deref() == Some(thread) && r.spec.doer == doer && r.spec.parent.is_none() && !r.spec.role.as_deref().is_some_and(|x| weigh::ROLES.contains(&x))).collect()
    }

    /// A task for the thread as the process is set to run it: its mode, its place, its spend and what is left of the time it was
    /// given.
    fn spec(&self, thread: &str, goal: &str, doer: &str) -> Result<TaskSpec, String> {
        let mut spec = TaskSpec::new(goal, doer);
        spec.thread = Some(thread.into());
        spec.tune.mode = self.0.cfg().mode;
        if let Some(place) = &self.0.cfg().place {
            spec.place = place.clone();
        }
        spec.limits.usd_soft = self.0.cfg().usd_soft;
        if let Some(end) = self.0.cfg().deadline {
            let left = end.saturating_duration_since(Instant::now()).as_millis() as u64;
            if left == 0 {
                return Err("the time it was given has run out".into());
            }
            spec.limits.wall_ms = Some(left);
        }
        Ok(spec)
    }

    /// A doer that takes a Brief is given one with the line: the contract for it (small talk is the first line of a thread,
    /// not a "yes" in a going one), and that it may only read, when it may only read. What was rendered is an event, so the numbers
    /// of a turn can be read back.
    fn briefed(&self, thread: &str, spec: &mut TaskSpec, line: &str, in_session: bool, (shape, inbox, parts): (Option<&str>, Option<&str>, Option<&str>)) {
        // the turn that writes the operator's answer is asked for words with `brief.plain_answer`: the role `work` is that turn alone (a
        // planning turn is `plan`, a branch has its class for a role, a calibration none); a doer that takes no Brief is told by the hook of its
        // wrapper (`kitsock::Kitted`) from this mark
        self.envoy_seat(thread, spec);
        let plain = spec.role.as_deref() == Some("work") && matches!(self.screen().set.pointer("/brief/plain_answer"), crate::value::Value::Bool(true));
        if plain {
            spec.input["plain_answer"] = json!(true);
        }
        let caps = self.0.doers.iter().find(|d| d.id() == spec.doer).map(|d| d.caps()).unwrap_or_default();
        // what the chat's own settings give this turn (its model, effort and tool list), and the seat's words: a section of their own, ahead of the task. A doer that
        // keeps no Brief is given them once a session, as it is given its other sections
        let voice = self.chat_apply(thread, spec);
        let voice = if caps.brief || !in_session { voice } else { String::new() };
        // the messages that wait for this chat, as a quoted block (a doer that takes no Brief reads them after the goal, with the reports)
        let mail = spec.input.as_object_mut().and_then(|o| o.remove("mail")).and_then(|v| v.as_str().map(String::from));
        // the skills that fit the line: in the Brief of a doer that takes one, else in a folder of the task's own for a doer that reads one (claude)
        // the librarian is on, and Jev is asked only where two skills fit equally and there is room for one; how it went is said in the row of the start
        let on = self.0.cfg().librarian.on;
        let jev = std::cell::RefCell::new(String::from("not asked"));
        // a doer with no Brief that lists no skills (codex, gemini, opencode), where `brief.cli` is `all`, is given the best-fitting skill's words after its goal, one at most
        let word = match self.screen().set.pointer("/brief/cli") { crate::value::Value::Str(w) => w, _ => String::new() };
        let alone = !caps.brief && !caps.skills && word == "all";
        let skills = if caps.brief || caps.skills || alone { self.skills_pick(thread, spec, line, in_session, if alone { skills::Way::Words } else if caps.skills && !caps.brief { skills::Way::Folder } else { skills::Way::Brief }, on.then_some(&jev)) } else { None };
        // a doer with no Brief is given one after its goal, as text, by the setting `brief.cli` (claude, the doer that reads a folder of skills, by default)
        let to_text = !caps.brief && (word == "all" || (word == "claude" && caps.skills));
        let mut held: Vec<(&'static str, String)> = Vec::new();
        if !caps.brief && !to_text {
            if on {
                self.library_cli(thread, spec, in_session, &jev);
            }
            if let Some(line) = self.scout_line(thread, spec, in_session, false) {
                let before = spec.input.get("context").and_then(Json::as_str).map(|c| format!("{c} ")).unwrap_or_default();
                spec.input["context"] = json!(format!("{before}{line}"));
            }
            if let Some(line) = self.envoy_line(thread, in_session, false) {
                let before = spec.input.get("context").and_then(Json::as_str).map(|c| format!("{c} ")).unwrap_or_default();
                spec.input["context"] = json!(format!("{before}{line}"));
            }
            if let Some(line) = self.tell_line(thread, spec, in_session, false) {
                let before = spec.input.get("context").and_then(Json::as_str).map(|c| format!("{c} ")).unwrap_or_default();
                spec.input["context"] = json!(format!("{before}{line}"));
            }
            if let Some(text) = skills.filter(|_| alone) {
                spec.input["cli_text"] = json!(format!("A skill that fits this task:
{text}"));
            }
            // the notes the operator kept under the line (and the parts) are read after the goal too, as the sections they are in a Brief
            let notes = Brief::new().with("seat", DataClass::Public, &voice).with("shape", DataClass::Project, shape.unwrap_or_default()).with("parts", DataClass::Project, parts.unwrap_or_default()).with("digests", DataClass::Project, &self.digests_section(thread).unwrap_or_default()).render(WINDOW).text;
            return self.reports_in_text(thread, spec, &notes, mail.as_deref());
        }
        // a new session's carry-over is a section of the Brief of a doer that takes one; a doer that is given the Brief as text reads it from the engine
        let carry = if to_text { None } else { spec.input.as_object_mut().and_then(|o| o.remove("carry")) };
        let read_only = spec.tune.mode == Mode::Read;
        // as text the contract is said once, on the first line of the session, as the project card is
        let mut contract = if to_text && in_session { String::new() } else { brief::contract(line, in_session, read_only) };
        // a doer that can ask for more is told so on the first line of its session, and the card is held back from a line that does not call for it
        let asks = self.librarian_for(&spec.doer);
        let (wanted, why_not) = Front::card_wanted(line, spec.tune.mode);
        if asks && !in_session {
            contract = format!("{contract} {}", librarian::TELL);
        }
        if let Some(line) = self.scout_line(thread, spec, in_session, caps.brief) {
            contract = format!("{contract} {line}");
        }
        if let Some(line) = self.envoy_line(thread, in_session, caps.brief) {
            contract = format!("{contract} {line}");
        }
        if let Some(line) = self.tell_line(thread, spec, in_session, caps.brief) {
            contract = format!("{contract} {line}");
        }
        let mut brief = Brief::new().with("contract", DataClass::Public, &if plain && !to_text { format!("{contract} {}", brief::PLAIN) } else { contract });
        brief = brief.with("seat", DataClass::Public, &voice);
        if spec.input.get("bench").is_none() && spec.input.get("branch").is_none() && let Some(text) = self.chats_line(thread, line, in_session) {
            brief = brief.with("chats", DataClass::Project, &text);
        }
        if let Some(text) = shape {
            brief = brief.with("shape", DataClass::Project, text);
        }
        if let Some(text) = carry.as_ref().and_then(Json::as_str) {
            brief = brief.with("carry", DataClass::Project, text);
        }
        // the lines typed while a task worked go to a doer with no Brief inside its turn line (the inbox's own words), not here
        if let Some(text) = inbox.filter(|_| !to_text) {
            brief = brief.with("inbox", DataClass::Project, text);
        }
        if let Some(text) = parts {
            brief = brief.with("parts", DataClass::Project, text);
        }
        if let Some(text) = mail.as_deref() {
            brief = brief.with("messages", DataClass::Project, text);
        }
        // skills: for a doer with no Brief they are its folder (claude) or one block after its goal, not a section
        if let Some(text) = skills.as_ref().filter(|_| !to_text) {
            brief = brief.with("skills", DataClass::Project, text);
        }
        // a branch is given a `brief` (the contract, the plan line, the files this line touched) or a `clean` one (the contract and
        // what the work changed); the reports waiting for the main lane are the main lane's to read, not its branches'
        let branch = spec.input.get("branch").cloned();
        let clean = branch.as_ref().and_then(|b| b.get("context")).and_then(Json::as_str) == Some("clean");
        if !in_session && !clean && let Some(text) = self.0.cfg().card.as_ref().and_then(|c| c.text()) {
            if asks && !wanted {
                held.push(("project", why_not.to_string()));
            } else {
                brief = brief.with("project", DataClass::Project, &text);
            }
        }
        if !in_session && !clean && spec.role.as_deref() != Some("plan") && let Some(text) = self.0.cfg().recall.as_ref().and_then(|r| r.text(&spec.goal)) {
            brief = brief.with("remembered", DataClass::Project, &text);
        }
        if spec.role.as_deref() != Some("plan")
            && !clean
            && spec.input.get("bench").is_none()
            && let Some(line) = self.plan_line(thread)
        {
            brief = brief.with("plan", DataClass::Project, &line);
        }
        if let Some(text) = self.digests_section(thread) {
            brief = brief.with("digests", DataClass::Project, &text);
        }
        if let Some(paths) = branch.as_ref().and_then(|b| b.get("paths")).and_then(Json::as_array) {
            let paths: Vec<&str> = paths.iter().filter_map(Json::as_str).take(40).collect();
            brief = brief.with("changed", DataClass::Project, &paths.join("\n"));
        }
        // the reports of branches the main lane has not read: told once, in the section `branches`
        let told = self.waiting_for(thread, spec);
        if !told.is_empty() {
            brief = brief.with("branches", DataClass::Project, &told.iter().map(|(task, r)| r.text(task)).collect::<Vec<_>>().join("\n"));
        }
        let brief = brief.render(WINDOW);
        if !told.is_empty() && brief.sections.contains(&"branches") {
            self.write(thread, "report.sent", Actor::Core, json!({ "tasks": told.iter().map(|(t, _)| t).collect::<Vec<_>>() }));
        }
        if to_text {
            // the Brief as text after the goal (the engine reads `cli_text` first), the sections that fit, nothing for an empty one
            let skill = skills.filter(|_| alone).map(|t| format!("A skill that fits this task:\n{t}"));
            let text = [Some(brief.text.clone()).filter(|t| !t.is_empty()), skill].into_iter().flatten().collect::<Vec<_>>().join("\n\n");
            if !text.is_empty() {
                spec.input["cli_text"] = json!(text);
            }
        } else {
            spec.input["brief"] = json!(brief.text);
        }
        if on {
            spec.input["held"] = json!(held.iter().map(|(w, _)| w).collect::<Vec<_>>());
            if !in_session || !held.is_empty() || *jev.borrow() != "not asked" {
                self.library_said(thread, &spec.doer, &spec.goal, "brief", &librarian::Picked { held, jev: jev.into_inner() }, spec.input.get("skills").and_then(Json::as_array).map_or(0, Vec::len));
            }
        }
        self.write(thread, "brief.rendered", Actor::Core, json!({ "doer": spec.doer, "tokens": brief.tokens, "sections": brief.sections, "cut": brief.cut, "in_session": in_session, "as": if to_text { "text" } else { "brief" } }));
    }

    /// What a line of a bench case is given: the Brief a line of `j4n run` gets for that doer, in `spec`, so that skills, recall and the
    /// project card reach the doer under test. The thread's own plan and the reports waiting for its main lane are not the case's, and are left out.
    /// `thread`: where the rows are written, the front's own when none.
    pub fn brief_hook(&self) -> BriefHook {
        let core = Arc::downgrade(&self.0);
        Arc::new(move |thread, spec, line, in_session| {
            let Some(core) = core.upgrade() else { return };
            let front = Front(core);
            let thread = thread.map_or_else(|| front.0.cfg().thread.clone(), String::from);
            spec.input["bench"] = json!(true);
            front.briefed(&thread, spec, line, in_session, (None, None, None));
            if let Some(o) = spec.input.as_object_mut() {
                o.remove("bench");
            }
        })
    }

    /// The reports waiting for the main lane that this task is to be told: not a planning turn's, and not a branch's.
    fn waiting_for(&self, thread: &str, spec: &TaskSpec) -> Vec<(String, crate::report::Report)> {
        if spec.role.as_deref() == Some("plan") || spec.input.get("branch").is_some() || spec.input.get("bench").is_some() {
            return Vec::new();
        }
        self.0.lanes.as_ref().map(|l| l.waiting(thread)).unwrap_or_default()
    }

    /// A doer that takes no Brief is told the reports that wait for the main lane in the text of its turn (`spec.input.reports`, which the
    /// engine puts after the goal), as the kit's card is put in a CLI doer's first text; they are cut whole when they do not fit the window, as in
    /// a Brief, and sent once.
    fn reports_in_text(&self, thread: &str, spec: &mut TaskSpec, notes: &str, mail: Option<&str>) {
        let told = self.waiting_for(thread, spec);
        let rendered = Brief::new().with("branches", DataClass::Project, &told.iter().map(|(task, r)| r.text(task)).collect::<Vec<_>>().join("\n")).render(WINDOW);
        let mut text = notes.to_string();
        if !told.is_empty() && rendered.sections.contains(&"branches") {
            self.write(thread, "report.sent", Actor::Core, json!({ "tasks": told.iter().map(|(t, _)| t).collect::<Vec<_>>(), "in": "text" }));
            let reports = self.0.log.redactor().scrub(&rendered.text);
            text = if text.is_empty() { reports } else { format!("{text}\n\n{reports}") };
        }
        if let Some(mail) = mail {
            text = if text.is_empty() { mail.to_string() } else { format!("{text}\n\n{mail}") };
        }
        if !text.is_empty() {
            spec.input["reports"] = json!(text);
        }
    }

    /// `:calibrate`: what a request has to cost to make a pair for the plan the thread's doer is on. It says so
    /// first and spends only on `:calibrate yes`: the operator's window is what it would spend.
    fn calibrate(&self, thread: &str, args: &[String]) -> Said {
        let said = Said::Command("calibrate".into());
        // the doer the thread is bound to, or the one the operator pinned for it before the first line
        let bound = self.binding(thread).map(|b| (b.doer, b.model, b.endpoint, b.account));
        let Some((doer, model, endpoint, account)) = bound.or_else(|| self.0.cfg().doer.clone().map(|d| (d, self.0.cfg().model.clone(), None, None))) else {
            return self.refuse(thread, "not bound yet: the next line chooses a doer".into());
        };
        let c = match self.0.tasks.calibration(&doer, account.as_deref()) {
            Ok(c) => c,
            Err(why) => return self.refuse(thread, why),
        };
        let money = c.money.map(|m| format!(", about ${m:.4} of your plan")).unwrap_or_default();
        let asks = if c.requests == 1 { "one request" } else { "two requests (the window has not moved since J4n began to watch it: the first only marks where a step begins)" };
        let line = format!(
            "{} · {}: a full window is worth about ${:.2} of list cost ({}). A request that moves it two steps ({:.0} %) costs about ${:.3} of list cost{money}. It takes {asks}, each writing {} numbers. ⏎ runs one, on the card (`:calibrate yes` too).",
            doer, c.window, c.per, c.source, c.share * 100.0, c.target, c.size
        );
        if args.first().map(String::as_str) != Some("yes") {
            self.note_to(thread, &line, "dim", Lead { risk: 60, ..Lead::run("calibrate", "calibrate", &["yes"]) });
            return said;
        }
        if let Some(row) = self.last_task(thread).filter(|r| !r.state.is_terminal()) {
            return self.busy(thread, &row.id, "");
        }
        let goal = format!("This is a calibration of cost, not a task. Reply with the whole numbers from 1 to {}, separated by single spaces, and nothing else. Use no tools.", c.size);
        let mut spec = match self.spec(thread, &goal, &doer) {
            Ok(spec) => spec,
            Err(why) => return self.refuse(thread, why),
        };
        (spec.tune.mode, spec.tune.model, spec.tune.endpoint, spec.account) = (Mode::Read, model, endpoint, account);
        spec.limits.usd_soft = None;
        self.briefed(thread, &mut spec, &goal, false, (None, None, None));
        self.note(thread, &line, "dim");
        match self.0.tasks.spawn(spec, Actor::Operator) {
            Ok(task) => Said::Started { task },
            Err(p) => self.refuse(thread, p.msg),
        }
    }

    /// `/plan add a flag`: a flow of its own, for the words after the command. The thread keeps its doer and its model: nothing is
    /// bound, elected for, or written here, and the flow's steps say what they may touch.
    fn flow(&self, thread: &str, args: &[String]) -> Said {
        let Some((id, goal)) = args.split_first() else { return self.refuse(thread, "a flow command needs its flow".into()) };
        // `:account add` with no name (`:setup` opens the form too) is the form of Agents › Add account, the connector in it
        if id == "account" && goal.first().is_some_and(|w| w == "add") && goal.len() <= 2 {
            return self.account_form_open(thread, goal.get(1).map(String::as_str));
        }
        let doer = format!("flow:{id}");
        if !self.0.doers.iter().any(|d| d.id() == doer) {
            return self.refuse(thread, format!("there is no flow called `{id}` (the flows that loaded: {})", self.flows().join(", ")));
        }
        let goal = goal.join(" ");
        if goal.is_empty() {
            return self.refuse(thread, format!("say what it is for: `/{id} add a --verbose flag`"));
        }
        if let Some(row) = self.last_task(thread).filter(|r| !r.state.is_terminal()) {
            return self.busy(thread, &row.id, "");
        }
        let spec = match self.spec(thread, &goal, &doer) {
            Ok(s) => s,
            Err(why) => return self.refuse(thread, why),
        };
        match self.0.tasks.spawn(spec, Actor::Operator) {
            Ok(task) => Said::Started { task },
            Err(p) => self.refuse(thread, p.msg),
        }
    }

    /// The ids of the flows this front can start.
    pub fn flows(&self) -> Vec<String> {
        self.0.doers.iter().filter_map(|d| d.id().strip_prefix("flow:").map(String::from)).collect()
    }

    // ---- a thread that has none ----

    fn first(&self, thread: &str, text: &str, keys: &[String]) -> Said {
        let (what, intake) = self.intake(thread, text);
        // a plan-shaped first line begins a plan, where something keeps one (a plan of one or two items is a plan)
        let plan = what == "flow" && self.0.keeper.is_some() && self.lanes_now().begins_plans();
        if what != "task" && !plan {
            self.note(thread, &format!("intake took this for `{what}`, which is not built yet: it goes to a doer as a task"), "dim");
        }
        match self.bind_first(thread, text, intake) {
            Ok(b) if plan => self.plan_turn(thread, text, How::Begin, &b),
            Ok(b) => self.turn(thread, text, &b, keys),
            // no agent works: the refusal leads to the page that sets one up
            Err(why) if self.0.cfg().doer.is_none() && self.0.who.ready(self.elects_as(thread), &[]).is_empty() => self.refuse_to(thread, format!("no agent works: {why} · ⏎ opens Agents › Setup"), Lead::open("open Setup", "setup")),
            Err(why) => self.refuse(thread, why),
        }
    }

    /// Elect the doer of a thread that has none, and bind it.
    fn bind_first(&self, thread: &str, text: &str, intake: Option<AnswerRef>) -> Result<Binding, String> {
        let (mut b, who, route) = self.elect(thread, text)?;
        b.answer = who.or(intake);
        self.write(thread, "thread.bound", Actor::Core, json!({ "doer": b.doer, "model": b.model, "endpoint": b.endpoint, "route": route, "how": b.how, "answer": b.answer }));
        Ok(b)
    }

    /// What the first line of a thread is. Written down, so the Log shows that it was asked once.
    fn intake(&self, thread: &str, text: &str) -> (String, Option<AnswerRef>) {
        // where a plan can begin, a line that is plan-shaped by its words alone is `flow`, and nobody is asked
        if self.0.keeper.is_some() && self.lanes_now().begins_plans() && planning::plan_shaped(text) {
            self.write(thread, "intake.decided", Actor::Core, json!({ "pick": "flow", "by": "front:plan-shaped", "why": "the line asks for a plan or lists steps" }));
            return ("flow".into(), None);
        }
        let options = INTAKE.iter().map(|id| Opt { id: (*id).into(), label: (*id).into(), recommended: *id == "task" }).collect();
        let q = self.0.who.question("intake", Some(thread), text, options);
        let (what, by, why, said) = self.0.who.pick("intake", Some(thread), &q).unwrap_or_else(|| ("task".into(), "front".into(), "no ask point `intake`".into(), None));
        self.write(thread, "intake.decided", Actor::Core, json!({ "pick": what, "by": by, "why": why }));
        (what, said)
    }

    /// A binding of `doer` and `model` (else the model its host serves), and the route that chose the host.
    fn bind_to(&self, doer: String, model: Option<&str>, how: &str) -> (Binding, Option<String>) {
        // a thread is bound at the moment the operator said a line: someone is waiting
        let want = Want { waiting: true, ..Want::default() };
        let pick = self.0.cfg().hosts.elect(&doer, model, &want, self.0.log.clock().now_ms());
        let model = model.map(String::from).or_else(|| pick.as_ref().map(|p| p.model.clone()));
        let route = pick.as_ref().map(|p| p.why.clone());
        (Binding { doer, model, endpoint: pick.map(|p| p.host), account: None, how: how.into(), answer: None, lines: 0, tasks: 0 }, route)
    }

    /// The doer a line names among the ones that can do `work` now, and the model it names for it (`front::named`); `None`: the line
    /// names no doer. A doer that is named and not ready is said so.
    fn named(&self, thread: &str, text: &str) -> Option<(String, Option<String>)> {
        let ids: Vec<&str> = self.0.who.doers().iter().map(|d| d.id()).collect();
        let n = named::parse(text, &ids).or_else(|| self.read_named(thread))?;
        let role = self.elects_as(thread);
        if !self.0.who.ready(role, &[]).iter().any(|o| o.id == n.doer) {
            self.note(thread, &format!("{} is named and is not ready: it is not installed, not logged in or not for {role}", n.doer), "warn");
            return None;
        }
        let model = self.line_model(thread, &n.doer, &n.words);
        Some((n.doer, model))
    }

    /// The model a line's words name for `doer`: the longest start of them that is one of the doer's models, through `model_for`. Several
    /// matches take the newest, and say so. A doer that lists no models is given none: the words are not passed on as typed.
    fn line_model(&self, thread: &str, doer: &str, words: &[String]) -> Option<String> {
        let list = self.0.cfg().models.models(doer).filter(|l| !l.is_empty())?;
        let n = (1..=words.len()).rev().find(|n| !matches!(models::resolve(&words[..*n].join(" "), &list), models::Resolved::None(_)))?;
        let word = words[..n].join(" ");
        match self.model_for(thread, doer, None, &word) {
            Ok(model) => model,
            Err(_) => {
                let options = self.0.picks.lock().unwrap_or_else(|e| e.into_inner()).remove(thread)?.options;
                self.note(thread, &format!("{} models match `{word}`; took the newest, {}", options.len(), options[0]), "dim");
                options.into_iter().next()
            }
        }
    }

    /// The role a thread's doer is elected for: `main` for Main under `orchestrate` (`who:main`), else `work`. The turns it runs stay `work`.
    fn elects_as(&self, thread: &str) -> &'static str {
        if thread == self.0.cfg().thread && self.main_works() { "main" } else { "work" }
    }

    /// The doer and the model of a thread: a doer the line names, else the ones the operator pinned, else the one `who:work` picks
    /// among the doers that are installed and logged in; and the host that serves the model. `.1` is the machine answer that picked
    /// the doer.
    fn elect(&self, thread: &str, text: &str) -> Result<(Binding, Option<AnswerRef>, Option<String>), String> {
        let (mut b, said, route) = self.elect_by(thread, text)?;
        // Main under `orchestrate` is never left to a doer's own default (on claude, the dearest): with no model pinned it gets the standard tier's
        if b.model.is_none() && self.elects_as(thread) == "main" && !self.waived(needs::ORCHESTRATE) {
            b.model = self.tier_model(&b.doer, tiers::Tier::Standard).map(|p| p.model);
        }
        Ok((b, said, route))
    }

    fn elect_by(&self, thread: &str, text: &str) -> Result<(Binding, Option<AnswerRef>, Option<String>), String> {
        let cfg = self.0.cfg();
        if let Some(doer) = &cfg.doer {
            if !self.0.doers.iter().any(|d| d.id() == doer) {
                let known: Vec<&str> = self.0.doers.iter().map(|d| d.id()).collect();
                return Err(format!("no doer called `{doer}` (there are: {})", known.join(", ")));
            }
            // a line that names a doer is the later word: the pin of the launch does not hold against it
            if let Some((named, model)) = self.named(thread, text).filter(|(n, _)| n != doer) {
                let (b, route) = self.bind_to(named, model.as_deref(), "named");
                return Ok((b, None, route));
            }
            let (b, route) = self.bind_to(doer.clone(), cfg.model.as_deref(), "pinned");
            return Ok((b, None, route));
        }
        // the operator's pick for the main lane (`:setup doer`), when it is ready; a line that names a doer is the later word
        if let Some(pick) = self.main_pick() {
            if self.0.who.ready(self.elects_as(thread), &[]).iter().any(|o| o.id == pick) {
                if let Some((named, model)) = self.named(thread, text).filter(|(n, _)| *n != pick) {
                    let (b, route) = self.bind_to(named, model.as_deref(), "named");
                    return Ok((b, None, route));
                }
                let (b, route) = self.bind_to(pick, self.0.cfg().main.model.as_deref().or(cfg.model.as_deref()), "picked");
                return Ok((b, None, route));
            }
            self.note_to(thread, &format!("the doer you picked, {pick}, is not ready: the election chooses · ⏎ opens Agents › Setup, which says what to mend"), "warn", Lead::open("open Setup", "setup"));
        }
        let (doer, said, _) = self.0.who.doer(self.elects_as(thread), Some(thread), text, &[])?;
        let model = self.named(thread, text).filter(|(n, _)| *n == doer).and_then(|(_, m)| m);
        // the operator's model for the main lane holds for the election's doer too, after the launch's `--model`, when it is one of its own
        let model = model.or_else(|| cfg.model.clone()).or_else(|| {
            let word = cfg.main.model.as_deref()?;
            let own = main_model_in(word, cfg.models.models(&doer));
            if own.is_none() {
                self.note(thread, &format!("{word} is not a model of {doer} · it runs on its default"), "dim");
            }
            own
        });
        let (b, route) = self.bind_to(doer, model.as_deref(), "elected");
        Ok((b, said, route))
    }

    /// A bound thread whose line names another doer moves there, as `:switch` does, with the way back. A line that names
    /// nothing, or the doer the thread has, or a thread whose task is going, leaves it where it is.
    fn moved(&self, thread: &str, text: &str, b: Binding) -> Binding {
        if self.settled(thread).is_none() {
            return b;
        }
        let Some((doer, model)) = self.named(thread, text).filter(|(d, _)| *d != b.doer) else { return b };
        let (now, _) = self.bind_to(doer, model.as_deref(), "named");
        let overrides = b.recent_answer().cloned();
        self.write(thread, "thread.bound", Actor::Operator, json!({ "doer": now.doer, "model": now.model, "endpoint": now.endpoint, "how": now.how, "overrides": overrides }));
        let model = now.model.as_deref().map(|m| format!(" · {m}")).unwrap_or_default();
        self.note(thread, &format!("now {}{model} · `:switch {}` to go back", now.doer, b.doer), "accent");
        self.binding(thread).unwrap_or(now)
    }

    /// `--doer` (and `--model`) on a folder whose main thread is bound to something else: the thread moves there, and says so. A
    /// thread that is already there is only marked as pinned, so the strip says why it stays.
    fn pin_main(&self) {
        let cfg = self.0.cfg();
        let thread = &cfg.thread;
        let (Some(doer), Some(b)) = (&cfg.doer, self.binding(thread)) else { return };
        if !self.0.doers.iter().any(|d| d.id() == doer) {
            return;
        }
        let same = b.doer == *doer && cfg.model.as_ref().is_none_or(|m| b.model.as_ref() == Some(m));
        if same && b.how == "pinned" {
            return;
        }
        let (now, _) = self.bind_to(doer.clone(), cfg.model.as_deref().or(if b.doer == *doer { b.model.as_deref() } else { None }), "pinned");
        self.write(thread, "thread.bound", Actor::Operator, json!({ "doer": now.doer, "model": now.model, "endpoint": now.endpoint, "how": now.how, "overrides": b.recent_answer() }));
        if !same {
            let was = b.model.as_deref().map(|m| format!("{} · {m}", b.doer)).unwrap_or(b.doer);
            let to = now.model.as_deref().map(|m| format!("{} · {m}", now.doer)).unwrap_or(now.doer);
            self.note(thread, &format!("--doer moved this folder's thread from {was} to {to} · `:switch` changes it"), "warn");
        }
    }

    /// A thread whose host is cooling: it waits (the task is cooled by the rules), moves to another host with a notice of what the
    /// cache loss costs, or asks the operator to; `route.failover` says which. A move is an event, never
    /// silent.
    fn failover(&self, thread: &str, b: &Binding) -> Binding {
        let cfg = self.0.cfg();
        let Some(host) = b.endpoint.as_deref() else { return b.clone() };
        let now = self.0.log.clock().now_ms();
        let Some(why) = cfg.hosts.cooling(&b.doer, b.model.as_deref(), host, now) else { return b.clone() };
        if cfg.failover == "wait" {
            return b.clone();
        }
        let Some((pick, cost)) = cfg.hosts.another(&b.doer, b.model.as_deref(), thread, host, now) else {
            self.note(thread, &format!("{host} is cooling ({why}) and there is no other host for this model: waiting"), "warn");
            return b.clone();
        };
        if cfg.failover == "ask" {
            self.note(thread, &format!("{host} is cooling ({why}). `:switch {} {} {}` moves to {} — {cost}", b.doer, b.model.clone().unwrap_or_default(), pick.host, pick.host), "warn");
            return b.clone();
        }
        let moved = Binding { endpoint: Some(pick.host.clone()), how: "moved".into(), answer: None, lines: 0, ..b.clone() };
        self.write(thread, "thread.bound", Actor::Core, json!({ "doer": moved.doer, "model": moved.model, "endpoint": moved.endpoint, "how": "moved", "from": host, "why": why, "cost": cost }));
        self.note(thread, &format!("{host} is cooling ({why}): moved to {} — {cost}", pick.host), "warn");
        moved
    }

    // ---- commands ----

    /// `:pack on <id>` and `:pack off <id>`: a pack is turned on or off by a row in the operator's own folder for this project, which
    /// is a proposal like any other change: it is recorded, it can be undone, and one that adds connectors, rules or gates asks twice.
    /// A pack that widens nothing is accepted at once, since the command is the yes; the packs are read again when j4n starts.
    fn pack_switch(&self, thread: &str, args: &[String]) -> Said {
        if self.proposer().is_none() {
            return self.refuse(thread, "nothing here makes proposals".into());
        }
        let (on, id) = (args[0] == "on", args.get(1).map(String::as_str).unwrap_or_default());
        let screen_ = self.screen();
        let Some(info) = screen_.packs.iter().find(|p| p.id == id) else {
            let screen_ = self.screen();
            let known: Vec<&str> = screen_.packs.iter().map(|p| p.id.as_str()).collect();
            return self.refuse(thread, format!("there is no pack `{id}` (there are {})", known.join(", ")));
        };
        // what the files say now, which a switch said while J4n runs has changed for the next start
        let said = screen_.tables.raw("enable", id).and_then(|m| m.fields.get("on")).and_then(toml::Value::as_bool).unwrap_or(info.on);
        if on == said {
            let when = if said == info.on { "" } else { " from the next start" };
            return self.refuse(thread, format!("{id} is {} already{when}", if on { "on" } else { "off" }));
        }
        let mut row = toml::Table::new();
        row.insert("on".into(), toml::Value::Boolean(on));
        if on && !info.pin.is_empty() {
            row.insert("hash".into(), toml::Value::String(info.pin.clone()));
        }
        // a pack with `[ready]` writes its settings with the enable (ready to go on), or takes back what it wrote: the same edit, so one `:undo` is all of it
        let mut ready = self.ready_enable_steps(info, on, &mut row);
        // `all`: the home's own layer, so every project has it (Agents › Setup's Decider); else this project's folder of the operator's rules
        let to = if args.iter().skip(2).any(|w| w == "all") { crate::change::Where::Home } else { crate::change::Where::User };
        ready.iter_mut().for_each(|s| s.to = to);
        let step = crate::proposal::Step { to, file: Some("harness.toml".into()), kind: "enable".into(), id: id.to_string(), op: crate::proposal::StepOp::Set(row) };
        let widens = if on && info.widens { vec![format!("{id} adds {}", info.adds)] } else { Vec::new() };
        let verb = if on { "Turn on" } else { "Turn off" };
        let why = format!("{} — {}{}", info.summary, info.adds, if info.tag.is_empty() { String::new() } else { format!(" [{}]", info.tag) });
        let draft = crate::proposal::Draft { key: format!("pack:{}:{id}", args[0]), title: format!("{verb} the pack {id} {}", info.version), why, evidence: 1, steps: std::iter::once(step).chain(ready.iter().cloned()).collect(), widens };
        // `:pack on <id> yes` is the card's yes: the Packs row asked first, naming what it widens
        let done = if args.iter().skip(2).any(|y| y == "yes") { self.edit_seen(draft) } else { self.edit(draft) };
        match done {
            Ok(crate::proposal::Edited::Proposed { id: made, .. }) => {
                self.note(thread, &format!("proposed {made}: {verb} the pack {id}, which adds {}. `:accept {made}` says what it widens, and a second `:accept {made}` turns it on", info.adds), "warn");
            }
            Ok(crate::proposal::Edited::Done { id: made, .. }) => {
                let all = if to == crate::change::Where::Home { " for every project" } else { "" };
                self.said_edit(thread, &format!("{id} is {}{all} from the next start (`:undo {made}` takes it back; ⟳ ⏎ restarts now)", if on { "on" } else { "off" }), "dim");
                if !ready.is_empty() {
                    let ids: Vec<&str> = ready.iter().map(|s| s.id.as_str()).collect();
                    self.note(thread, &format!("ready to go {}: {}", if on { "writes" } else { "takes back" }, ids.join(", ")), "dim");
                    self.need_check(thread);
                }
            }
            Err(why) => return self.refuse(thread, why),
        }
        Said::Command("pack".into())
    }

    /// `:look`, `:look <part>`, `:look <part> <word>`: the look of the screens, said while J4n runs. A word is checked against the words the
    /// screen knows, written as a proposal in the operator's folder for this project (it is recorded and `:undo` takes it back) and accepted
    /// at once, since the command is the yes. The screens are sent the look as it is now; the next start reads it from the file.
    fn look_said(&self, thread: &str, args: &[String]) -> Said {
        use j4n_proto::Look;
        let (said, now) = (Said::Command("look".into()), self.look_spoken());
        let Some(part) = args.first() else {
            let all: Vec<String> = Look::PARTS.iter().map(|(p, _)| format!("{p} {}", now.word(p).unwrap_or_default())).collect();
            self.note(thread, &all.join(" · "), "dim");
            return said;
        };
        let part = part.strip_prefix("theme.").unwrap_or(part);
        let Some(words) = Look::PARTS.iter().find(|(p, _)| *p == part).map(|(p, w)| fonts::said_words(p, w)) else {
            let parts: Vec<&str> = Look::PARTS.iter().map(|(p, _)| *p).collect();
            return self.refuse(thread, format!("the look has no part `{part}` (there are {})", parts.join(", ")));
        };
        let Some(word) = args.get(1) else {
            let is = if part == "marks" && now.marks == fonts::AUTO { fonts::found().shown() } else { now.word(part).unwrap_or_default() };
            self.note(thread, &format!("{part} is {is}; it can be {}", words.join(", ")), "dim");
            return said;
        };
        // `next` is the word after the one it is now, round to the first: the key of a row of Settings › Look
        let word = &if word == "next" { words[words.iter().position(|w| now.word(part).as_deref() == Some(*w)).map_or(0, |i| i + 1) % words.len()].to_string() } else { word.clone() };
        if !words.contains(&word.as_str()) {
            return self.refuse(thread, format!("`{word}` is not a word of {part}: it is one of {}", words.join(", ")));
        }
        if now.word(part).as_deref() == Some(word.as_str()) {
            return self.refuse(thread, format!("{part} is {word} already"));
        }
        let value = if part == "motion" { toml::Value::Boolean(word == "true") } else { toml::Value::String(word.clone()) };
        match self.edit_setting(&format!("theme.{part}"), "look.toml", value, &format!("Look: {part} {word}")) {
            Ok(back) => {
                if self.0.reload.is_none() {
                    *self.0.look.lock().unwrap_or_else(|e| e.into_inner()) = Some(now.with(part, word));
                }
                self.said_edit(thread, &format!("{part} is {word}{back}"), "dim");
            }
            Err(why) => return self.refuse(thread, why),
        }
        said
    }

    /// Changes taken back that were of the look: the screens draw what the layers below say again.
    fn look_undone(&self, undone: &[crate::change::Change]) {
        let screen = self.screen();
        let tables = &screen.tables;
        for c in undone.iter().filter(|c| c.target.kind == "setting") {
            let Some(part) = c.target.id.strip_prefix("theme.") else { continue };
            let below = |f: &str| tables.field_below(c.target.layer, "setting", &c.target.id, f);
            let word = match c.after.as_ref().and_then(|t| t.get("value")).or_else(|| below("value")).or_else(|| below("default")) {
                Some(toml::Value::String(s)) => s.clone(),
                Some(toml::Value::Boolean(b)) => b.to_string(),
                _ => continue,
            };
            let look = self.look_spoken().with(part, &word);
            *self.0.look.lock().unwrap_or_else(|e| e.into_inner()) = Some(look);
        }
    }

    /// `:accept [p3]`, `:decline [p3]`, `:undo [p3]`, `:proposals`: what J4n proposed for itself, and the operator's say over it.
    fn proposal(&self, thread: &str, cmd: &Cmd) -> Said {
        let Some(proposer) = self.proposer() else { return self.refuse(thread, "nothing here makes proposals".into()) };
        let id = cmd.args.first().map(String::as_str).unwrap_or_default();
        let said = Said::Command(cmd.name.clone());
        let line = |p: &crate::proposal::Proposal| format!("{} {}  {}  — {}", p.id, p.fate.word(), p.draft.title, p.draft.why);
        let text = match cmd.name.as_str() {
            "proposals" => {
                proposer.log_sync();
                let all: Vec<_> = proposer.proposals().list().into_iter().filter(crate::proposal::shown).collect();
                if all.is_empty() { "nothing has been proposed".to_string() } else { all.iter().take(20).map(line).collect::<Vec<_>>().join("\n") }
            }
            // `:accept p3 yes` is the card's yes: a proposal that widens is armed and applied in the one command
            "accept" => match self.charter_before_accept(id, thread).and_then(|signing| proposer.accept(id).and_then(|a| if matches!(a, Accepted::Confirm(_)) && cmd.args.get(1).is_some_and(|y| y == "yes") { proposer.accept(id) } else { Ok(a) }).map(|a| (a, signing)).map_err(|p| p.msg)) {
                Ok((Accepted::Done(changes), signing)) => {
                    let named = if id.is_empty() { String::new() } else { format!(" {id}") };
                    // the accept of a charter is its signing: the operator's key, written to the Log with what the card showed
                    let signed = signing.map(|(c, proposal)| self.charter_signed(&c, &proposal)).unwrap_or_default();
                    format!("accepted{named}: {} change(s) written{signed}. `:undo{named}` takes them back", changes.len())
                }
                Ok((Accepted::Confirm(widens), _)) => {
                    let named = if id.is_empty() { String::new() } else { format!(" {id}") };
                    self.note(thread, &format!("this widens what J4n may do ({}): `:accept{named}` again to confirm", widens.join(", ")), "warn");
                    return said;
                }
                Err(why) => return match self.charter_lead(id) {
                    Some(lead) if why.contains("is signed in") => self.refuse_to(thread, why, lead),
                    _ => self.refuse(thread, why),
                },
            },
            "decline" => match proposer.decline(id) {
                Ok(p) => format!("declined {}: it comes back only if its evidence doubles", p.id),
                Err(p) => return self.refuse(thread, p.msg),
            },
            _ => match proposer.undo(id) {
                Ok(undone) => {
                    // where the settings are read again after an undo (`follow_edits`) the overlays went with them; else they are put back here
                    if self.0.reload.is_none() {
                        self.look_undone(&undone);
                        self.lanes_undone(&undone);
                        self.skills_undone(&undone);
                        self.chores_undone(&undone);
                    }
                    format!("took back {} change(s)", undone.len())
                }
                Err(p) => return self.refuse(thread, p.msg),
            },
        };
        self.note(thread, &text, "dim");
        said
    }

    fn command(&self, thread: &str, cmd: Cmd) -> Said {
        // with the switch off `:new` is what it always was, `:fresh`
        let cmd = if cmd.name == "new" && !self.chats_on() { Cmd { name: "fresh".into(), ..cmd } } else { cmd };
        let running = self.last_task(thread).filter(|r| !r.state.is_terminal());
        let said = Said::Command(cmd.name.clone());
        match cmd.name.as_str() {
            "quit" => return Said::Quit,
            "clear" | "history" | "chats" | "top" | "archive" | "unarchive" if !self.chats_on() => return self.chats_off(thread, &cmd.name),
            "archive" => return self.archive_said(thread, &cmd.args),
            "unarchive" => return self.unarchive_said(thread, &cmd.args),
            "new" => return self.new_said(thread, &cmd.args),
            "envoy" => return self.envoy_said(thread),
            "clear" => return self.clear_said(thread),
            "history" => return self.history_said(thread),
            "chats" => return self.chats_said(),
            "top" => return Said::Enter(String::new()),
            "restart" if running.is_some() => {
                let id = running.map(|r| r.id).unwrap_or_default();
                return self.busy(thread, &id, ", since a restart ends it");
            }
            "restart" => {
                self.0.restart.store(true, std::sync::atomic::Ordering::SeqCst);
                return Said::Quit;
            }
            "fresh" | "switch" if running.is_some() => {
                let id = running.map(|r| r.id).unwrap_or_default();
                return self.busy(thread, &id, "");
            }
            "fresh" if self.fresh_chat(thread) => {}
            "fresh" => {
                self.write(thread, "thread.fresh", reflexes::doing(), json!({}));
                self.forget_moves(thread);
            }
            "resume" => return self.resume_said(thread),
            "compact" => return self.compact_said(thread, &cmd.args),
            "switch" => return self.switch(thread, &cmd.args).unwrap_or_else(|why| self.refuse(thread, why)),
            "stop" | "kill" => {
                // a line that waits for a reflex's judge or the operator's yes is dropped too
                let dropped = self.reflex_drop(thread);
                let Some(row) = running else {
                    if dropped {
                        return said;
                    }
                    return self.refuse(thread, "nothing is running".into());
                };
                let ctl = if cmd.name == "stop" { Ctl::Abort } else { Ctl::Kill };
                if let Err(p) = self.0.tasks.ctl(&row.id, ctl, Actor::Operator) {
                    return self.refuse(thread, p.msg);
                }
            }
            "probe" if self.reflex_probe(thread, cmd.args.first().map(String::as_str)) => return said,
            "probe" => return self.probe(thread, cmd.args.first().map(String::as_str)),
            "reflex" => return self.reflex_said(thread, &cmd.args),
            "grant" => return self.grant_said(thread, &cmd.args),
            // `:setup` opens Agents › Setup; `:setup doer …` is still the pick, `:setup again` asks the programs again, `:setup test` makes the proof call
            command::OPEN if cmd.args.first().is_some_and(|p| p == "setup") && cmd.args.get(1).is_some_and(|w| w == "test") => return self.setup_test(thread),
            command::OPEN if cmd.args.first().is_some_and(|p| p == "setup") && cmd.args.get(1).is_some_and(|w| w == "doer") => return self.setup_pick(thread, &cmd.args[2..]),
            command::OPEN if cmd.args.first().is_some_and(|p| p == "setup") && cmd.args.get(1).is_some_and(|w| w == "again") => return self.setup_again(thread),
            command::OPEN => {
                let page = cmd.args.first().cloned().unwrap_or_default();
                self.note(thread, &format!("opening {page}"), "dim");
                return Said::Open(page);
            }
            "pack" if cmd.args.first().is_some_and(|w| w == "ready") => return self.ready_said(thread, &cmd.args[1..]),
            "pack" if matches!(cmd.args.first().map(String::as_str), Some("on" | "off")) => return self.pack_switch(thread, &cmd.args),
            "pack" => {
                let lines: Vec<String> = self
                    .screen()
                    .packs
                    .iter()
                    .map(|p| {
                        let state = if p.on { "on".to_string() } else if p.note.is_empty() { "off".to_string() } else { format!("left out: {}", p.note) };
                        format!("{} {}  {state}  {}{}", p.id, p.version, p.adds, if p.tag.is_empty() { String::new() } else { format!("  [{}]", p.tag) })
                    })
                    .collect();
                self.note(thread, &lines.join("\n"), "dim");
            }
            "undo" if cmd.args.first().is_some_and(|a| a.starts_with("cs") && a[2..].chars().all(|c| c.is_ascii_digit()) && a.len() > 2) => return self.chat_undo_said(thread, &cmd.args[0]),
            "accept" | "decline" | "undo" | "proposals" => return self.proposal(thread, &cmd),
            "key" => return self.key_said(thread, &cmd.args),
            "connector" => return self.connector_said(thread, &cmd.args),
            "look" => return self.look_said(thread, &cmd.args),
            "lanes" => return self.lanes_said(thread, &cmd.args),
            "offload" => return self.offload_said(thread, &cmd.args),
            "skill" => return self.skill_said(thread, &cmd.args),
            "calibrate" => return self.calibrate(thread, &cmd.args),
            // answered from the Log: no doer, no model, no task
            "usage" => self.usage_say(thread),
            "context" => self.context_say(thread),
            "chart" => {
                let bind = cmd.args.first().map(|a| if a.starts_with("chart:") { a.clone() } else { format!("chart:{a}") });
                let Some(bind) = bind.filter(|b| charts::is_chart(b)) else { return self.refuse(thread, format!("`:chart` takes one of {}", charts::NAMES.join(", "))) };
                self.chart_say(thread, &bind);
            }
            "term" => match self.term_said(&cmd.args) {
                Ok(text) => self.note(thread, &text, "dim"),
                Err(why) => return self.refuse(thread, why),
            },
            "watch" => {
                let Some(up) = &self.0.cfg().upstream else { return self.refuse(thread, "nothing follows the tools here".into()) };
                let said = match (cmd.args.first().map(String::as_str), cmd.args.get(1)) {
                    (Some("once"), _) => up.once(),
                    (Some("plan"), Some(id)) => up.plan(id),
                    (Some("plan"), None) => Err("`:watch plan` takes the id of a release: `p` on a row of Grow › Updates plans it".into()),
                    _ => Ok(up.show()),
                };
                match said {
                    Ok(text) => self.note(thread, &text, "dim"),
                    Err(why) => return self.refuse(thread, why),
                }
            }
            "wrong" => {
                let Some(c) = &self.0.cfg().contribute else { return self.pack_off(thread, "contribute") };
                match c.wrong(thread, &cmd.args.join(" ")) {
                    Ok(text) => self.note(thread, &text, "dim"),
                    Err(why) => return self.refuse(thread, why),
                }
            }
            "inbox" => {
                let Some(inbox) = &self.0.cfg().inbox else { return self.pack_off(thread, "inbox") };
                match inbox.command(thread, &cmd.args) {
                    Ok(text) => self.note(thread, &text, "dim"),
                    Err(why) => return self.refuse(thread, why),
                }
            }
            "parts" => {
                let cfg_ = self.0.cfg();
                let Some(parts) = cfg_.parts.as_ref().filter(|p| p.on()) else { return self.refuse_to(thread, "the part runner is off · ⏎ sets parts.mode to show (Settings › All › parts.mode)".into(), Lead::set("parts.mode show", "parts.mode", json!("show"))) };
                match parts.command(thread, &cmd.args, self.0.keeper.as_ref().and_then(|k| k.plan(thread)).as_ref()) {
                    Ok(text) => self.note(thread, &text, "dim"),
                    Err(why) => return self.refuse(thread, why),
                }
                // a run's parts are the plan's items: a thread with no doer yet elects one, as its first line would
                if cmd.args.first().is_some_and(|a| a == "go") && self.binding(thread).is_none() && let Err(why) = self.bind_first(thread, "work on the parts of a document, one at a time", None) {
                    return self.refuse(thread, why);
                }
                self.plan_step(thread);
            }
            "report" => {
                let Some(ledger) = &self.0.cfg().ledger else { return self.pack_off(thread, "ledger") };
                match ledger.command(thread, &cmd.args) {
                    Ok((text, tree)) => drop(self.write(thread, "chat.note", Actor::Core, json!({ "text": text, "tone": "dim", "tree": tree }))),
                    Err(why) => return self.refuse(thread, why),
                }
            }
            "shape" => {
                let Some(desk) = self.desk() else { return self.pack_off(thread, "shape") };
                match desk.command(thread, &cmd.args) {
                    Ok(text) => self.note(thread, &text, "dim"),
                    Err(why) => return self.refuse(thread, why),
                }
            }
            phrase::FLOW => return self.flow(thread, &cmd.args),
            "plan" => return self.plan_command(thread, &cmd.args),
            "status" => {
                let state = match (self.binding(thread), self.last_task(thread)) {
                    (None, _) => "not bound yet: the next line chooses a doer".to_string(),
                    (Some(b), last) => {
                        let model = b.model.map(|m| format!(" · {m}")).unwrap_or_default();
                        let task = last.map(|r| format!(", {} {}", r.id, r.state.word())).unwrap_or_default();
                        format!("bound to {}{model} ({}){task}", b.doer, b.how)
                    }
                };
                self.note(thread, &state, "dim");
            }
            "help" => {
                let lines: Vec<String> = self.0.phrases.docs().iter().map(|(w, d)| format!("{w}  {d}")).collect();
                self.note(thread, &lines.join("\n"), "dim");
            }
            _ => return self.refuse(thread, format!("`{}` is not a command this version has", cmd.name)),
        }
        said
    }

    // ---- prompt shaping ----

    /// The notes the operator kept for this line, as the Brief carries them; a key the desk does not take is dropped with a notice.
    fn kept(&self, thread: &str, text: &str, keys: &[String]) -> Option<String> {
        if keys.is_empty() {
            return None;
        }
        let Some(desk) = self.desk() else {
            self.note(thread, "nothing here shapes lines: the line goes as typed", "dim");
            return None;
        };
        match desk.keep(thread, text, keys) {
            Ok(section) => Some(section),
            Err(why) => {
                self.note(thread, &format!("{why}: the line goes as typed"), "warn");
                None
            }
        }
    }

    /// The composer's draft (`Up::Draft`). With the switch on, a line that may be shaped is flashed as a `draft` event for the rule
    /// `shape-draft`; the desk is told either way, so that the card can say `not run` and why, and an Enter does not wait for nothing.
    pub fn draft(&self, thread: &str, text: &str, target: &str) {
        let Some(desk) = self.desk().filter(|d| d.on()) else { return };
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let b = self.binding(thread);
        let target = if target.is_empty() { b.as_ref().map(|b| b.doer.clone()).unwrap_or_default() } else { target.to_string() };
        let open: Vec<_> = self.0.tasks.book().open_asks().into_iter().filter(|a| a.thread == thread).collect();
        let facts = shape::Facts {
            answers: !matches!(binder::bind(text, &open, None), Bind::NotAnswer),
            command: self.0.phrases.parse(text).is_some() || self.0.phrases.take(text).is_some(),
            short_reply: b.is_some() && (brief::is_short(text) || desk.skips(text)),
            handoff: self.handoff(thread, &target),
        };
        let draft = Draft { thread: thread.into(), text: text.into(), hash: shape::hash_of(&target, text), target, handoff: facts.handoff };
        let skip = shape::skip_reason(text, facts);
        desk.drafted(&draft, skip);
        if skip.is_none() {
            let data = json!({ "text": draft.text, "target": draft.target, "hash": draft.hash, "handoff": draft.handoff });
            let _ = self.0.log.flash(New::of("draft", Actor::Core, data).thread(thread));
        }
    }

    /// Whether the line goes to a doer that never received what the thread said: it has turns by another doer behind it and none
    /// of its own that kept a session.
    fn handoff(&self, thread: &str, target: &str) -> bool {
        if target.is_empty() || self.binding(thread).is_none() {
            return false;
        }
        let rows: Vec<_> = self.0.tasks.book().rows().into_iter().filter(|r| r.spec.thread.as_deref() == Some(thread) && r.spec.doer != HUMAN && r.spec.parent.is_none()).collect();
        rows.iter().any(|r| r.spec.doer != target) && !rows.iter().any(|r| r.spec.doer == target && r.session.is_some())
    }

    /// `:probe [answerer]`: put the probe cases to the machine answerers, off this line's path, and say how each did.
    fn probe(&self, thread: &str, only: Option<&str>) -> Said {
        let said = Said::Command("probe".into());
        let Some(prober) = self.0.asks.prober.clone() else { return self.refuse(thread, "no decider or model is installed to probe".into()) };
        let targets = prober.targets(only);
        if targets.is_empty() {
            self.note(thread, "nothing to probe: no installed decider or model answers a point that has cases", "dim");
            return said;
        }
        self.note(thread, &format!("probing {} answerer/point pair(s)…", targets.len()), "dim");
        let front = self.clone();
        let thread = thread.to_string();
        std::thread::spawn(move || {
            for r in prober.run(&targets) {
                let line = match (&r.why, r.passed) {
                    (Some(why), _) => format!("{} was not asked the cases for {}: {why}", r.answerer, r.point),
                    (None, true) => format!("{} passed its cases for {} ({}, {} ms)", r.answerer, r.point, r.model, r.ms),
                    (None, false) => format!("{} failed its cases for {}: {} — it does not answer there until it passes", r.answerer, r.point, r.missed.iter().map(|m| m.line()).collect::<Vec<_>>().join("; ")),
                };
                front.note(&thread, &line, if r.passed { "dim" } else { "warn" });
            }
        });
        said
    }

    /// The line answers the models a `:switch` offered (a number, or one of their names): the switch is made with that one.
    fn picked(&self, thread: &str, text: &str) -> Option<Said> {
        let pick = self.0.picks.lock().unwrap_or_else(|e| e.into_inner()).remove(thread)?;
        let chosen = match text.parse::<usize>() {
            Ok(n) => pick.options.get(n.wrapping_sub(1)),
            Err(_) => pick.options.iter().find(|o| o.eq_ignore_ascii_case(text)),
        }?
        .clone();
        let mut args = vec![pick.doer, chosen];
        args.extend(pick.account.map(|a| format!("account={a}")));
        Some(match self.switch(thread, &args) {
            Ok(said) => said,
            Err(why) => self.refuse(thread, why),
        })
    }

    /// Settle a model word for `doer` against the models it has (`front::models`): the model in full, `None` when this front knows
    /// no list, an error when it names none; and when it names several, the choice is offered on `thread` and the error says so.
    /// This is the one way a model word becomes a model; the choosing of a model for a line calls it too.
    pub fn model_for(&self, thread: &str, doer: &str, account: Option<&str>, word: &str) -> Result<Option<String>, String> {
        match models::settle(self.0.cfg().models.as_ref(), doer, word)? {
            None => Ok(None),
            Some(models::Resolved::One(m)) => Ok(Some(m)),
            Some(models::Resolved::Many(options)) => {
                let menu: Vec<String> = options.iter().enumerate().map(|(i, m)| format!("{}. {m}{}", i + 1, if i == 0 { " (newest, suggested)" } else { "" })).collect();
                let said = format!("{} models match `{word}`; say a number to pick:\n{}", options.len(), menu.join("\n"));
                self.0.picks.lock().unwrap_or_else(|e| e.into_inner()).insert(thread.to_string(), Pick { doer: doer.to_string(), account: account.map(str::to_string), options });
                Err(said)
            }
            Some(models::Resolved::None(_)) => Err(format!("{doer} has no model like `{word}`")),
        }
    }

    /// Whether a doer of this name is there to switch to.
    pub fn knows_doer(&self, id: &str) -> bool {
        self.0.doers.iter().any(|d| d.id() == id)
    }

    /// `:switch [to|on|with] <doer> [model words…]`, or `:switch <model words…>` for the doer the thread has. The model words are
    /// matched against the doer's models, and the notice says the full name that was picked.
    pub fn switch(&self, thread: &str, args: &[String]) -> Result<Said, String> {
        let known = |id: &str| self.0.doers.iter().any(|d| d.id() == id);
        let current = self.binding(thread);
        // `account=work` names the account the doer runs as; the rest is the doer and the model
        let account = args.iter().find_map(|a| a.strip_prefix("account=")).map(str::to_string);
        let rest: Vec<&String> = args.iter().filter(|a| !a.starts_with("account=")).collect();
        // `to`, `on` and `with` are filler: before the doer, or between it and the model
        let mut words: Vec<String> = Vec::new();
        for (i, a) in rest.iter().enumerate() {
            let after_doer = i > 0 && words.first().is_some_and(|d| known(d)) && words.len() == 1;
            if models::is_filler(a) && rest.len() > 1 && (words.is_empty() || after_doer) {
                continue;
            }
            words.push((*a).clone());
        }
        let (doer, said) = match words.as_slice() {
            [first, rest @ ..] if known(first) => (first.clone(), rest.join(" ")),
            [] => return Err("switch to which doer? `:switch claude haiku`".into()),
            all if current.is_some() => (current.map(|b| b.doer).unwrap_or_default(), all.join(" ")),
            [first, ..] => return Err(format!("no doer called `{first}`")),
        };
        let model = if said.is_empty() { None } else { Some(self.model_for(thread, &doer, account.as_deref(), &said)?.unwrap_or(said.clone())) };
        let host = self.0.cfg().hosts.elect(&doer, model.as_deref(), &Want { waiting: true, ..Want::default() }, self.0.log.clock().now_ms()).map(|p| p.host);
        let overrides = self.binding(thread).and_then(|b| b.recent_answer().cloned());
        self.write(thread, "thread.bound", Actor::Operator, json!({ "doer": doer, "model": model, "endpoint": host, "account": account, "how": "switch", "overrides": overrides }));
        if let Some(m) = model.as_ref().filter(|m| **m != said) {
            self.note(thread, &format!("{doer} · {m}"), "dim");
        }
        Ok(Said::Command("switch".into()))
    }
}
