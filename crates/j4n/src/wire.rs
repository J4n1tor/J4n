//! The pieces, wired: the Log of the project, its folds, the timers, the Gate and the ask points the base pack and the
//! operator's files give, the doers, the loop, and the front over them.

use crate::cli::{Approval, Opts};
use crate::pack;
use j4n_connect::account;
use j4n_connect::check::Check;
use j4n_connect::connect;
use j4n_reach::Outside;
use j4n_connect::hosts::{Election, HostBook, Models, Route};
use j4n_connect::manifest::{Connector, Connectors, Reach, Survey};
use j4n_core::place::Launch;
use j4n_bench::Bench;
use j4n_memory::{Index, MemoryTools};
use j4n_connect::place::boxed::{self, Boxed};
use j4n_connect::place::{Apply, Worktree};
use j4n_core::answer::chain::Registry;
use j4n_core::answer::point::points;
use j4n_core::answer::probe::{ProbeSet, Prober};
use j4n_core::answer::state::{Answerers, Limits};
use j4n_core::change::Actor;
use j4n_core::changes::Changes;
use j4n_core::doer::Doer;
use j4n_core::place::Place;
use j4n_core::cost::Plans;
use j4n_core::failure::Failures;
use j4n_core::flow::Flows;
use j4n_core::flow::run::{self as flow_run, Link, TaskHost};
use j4n_core::fold::{Folds, declared};
use j4n_core::front::hosts::BookRecord;
use j4n_core::front::models::{ModelLists, Resolved};
use j4n_core::front::named::named_rule;
use j4n_core::keeper::{Keeper, fold_rule};
use j4n_core::front::phrase::{Phrases, intake_rule, weigh_rule};
use j4n_core::front::thread::Binds;
use j4n_core::front::who::{Standing, Who};
use j4n_core::front::screen::Screen;
use j4n_core::front::{Cfg, Front};
use j4n_core::pack::{enables, packs_dir};
use j4n_core::skill;
use j4n_core::gate::check::Gates;
use j4n_core::gate::grant::{Grants, here_dir, here_layer};
use j4n_core::gate::Gate;
use j4n_core::log::{Log, New, Opts as LogOpts, Scope};
use j4n_core::place::checkpoint::Cfg as Checkpoints;
use j4n_core::place::Here;
use j4n_core::proposal::{Plant, Proposals, Proposer};
use j4n_core::rank::{RankAnswerer, specs};
use j4n_core::redact::Redactor;
use j4n_core::rule::Rules;
use j4n_core::setting::Settings;
use j4n_core::table::{Layer, LayerKind, Tables};
use j4n_core::task::spot::Spot;
use j4n_core::task::{Asks, Config as Loop, Tasks};
use j4n_core::timer::Timers;
use j4n_core::value::Value;
use j4n_core::write::Writer;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub struct World {
    pub front: Front,
    doers: Vec<Arc<dyn Doer>>,
    /// What loading found that the operator should read: a file that is wrong, a log that was repaired.
    pub notices: Vec<String>,
    _timers: Arc<Timers>,
}

impl World {
    /// Let go of what outlives the process if it is not told: the children, and the end of the Log.
    pub fn close(&self) {
        self.doers.iter().for_each(|d| d.shutdown());
        self.front.log().sync();
    }
}

/// Where J4n works: the project, its own folder, and the program that is Claude Code.
#[derive(Debug, Clone)]
pub struct Setup {
    pub root: PathBuf,
    pub home: PathBuf,
    pub claude: String,
}

impl Setup {
    /// This folder; J4n's own folder is `$J4N_HOME`, else `~/.j4n`; Claude is `$J4N_CLAUDE`, else `claude`.
    pub fn from_env() -> Result<Setup, String> {
        let root = std::env::current_dir().map_err(|e| format!("cannot tell which folder this is: {e}"))?;
        let home = match std::env::var_os("J4N_HOME").filter(|h| !h.is_empty()) {
            Some(h) => PathBuf::from(h),
            None => std::env::var_os("HOME")
                .filter(|h| !h.is_empty())
                .map(|h| PathBuf::from(h).join(".j4n"))
                .ok_or("HOME is not set, so there is no folder for J4n's own files")?,
        };
        let claude = std::env::var("J4N_CLAUDE").ok().filter(|p| !p.is_empty()).unwrap_or_else(|| "claude".into());
        Ok(Setup { root, home, claude })
    }
}

/// A secret a connector points at: `env:NAME` or `file:/path`, read when its child is started and never written down. The rules are the
/// redactor's own (`redact::resolve_in`): a file others can read is not used, and neither is a value the redactor cannot keep out of the Log.
pub(crate) fn secret(red: &Redactor, name: &str, reference: &str) -> Result<String, String> {
    let value = if let Some(name) = reference.strip_prefix("env:") {
        std::env::var(name).map_err(|_| format!("`{name}` is not set"))?
    } else if let Some(path) = reference.strip_prefix("file:") {
        use std::os::unix::fs::PermissionsExt;
        let home = std::env::var("HOME").unwrap_or_default();
        let path = path.strip_prefix("~/").map_or_else(|| path.to_string(), |rest| format!("{home}/{rest}"));
        let mode = std::fs::metadata(&path).map_err(|e| format!("cannot read `{path}`: {e}"))?.permissions().mode();
        if mode & 0o077 != 0 {
            return Err(format!("`{path}` is readable by others (mode {:o}), so {name} is not used: chmod 600 it", mode & 0o777));
        }
        std::fs::read_to_string(&path).map_err(|e| format!("cannot read `{path}`: {e}"))?.trim().to_string()
    } else {
        return Err(format!("`{reference}` is neither env:NAME nor file:/path"));
    };
    if value.is_empty() {
        return Err("it is empty".into());
    }
    if value.chars().count() < j4n_core::redact::MIN_SECRET {
        return Err(format!("{name} is shorter than {} characters: it cannot be kept out of the Log, so it is not used", j4n_core::redact::MIN_SECRET));
    }
    if !red.register(name, &value) {
        return Err(format!("`{name}` cannot name a secret the Log is kept from, so it is not used"));
    }
    Ok(value)
}

/// `thread` is the one thread of this process: the screen's is `main` and goes on where the last run left it; a run, or a
/// stream of lines, has one of its own.
pub fn open(setup: Setup, opts: &Opts, interactive: bool, thread: &str) -> Result<World, String> {
    let started = Instant::now();
    let Setup { root, home, claude: program } = setup;
    let red = Redactor::new();
    // the salt of `same-as-id` is the machine's, kept in the home folder; the ids still work for the run if it cannot be kept
    let mut notices: Vec<String> = Vec::new();
    notices.extend(red.keep_salt(&home).err().map(|e| format!("guard: {e}")));
    // a value too short to be kept out of the Log is not registered: the redactor says no, and it stays as it is
    for (name, value) in std::env::vars().filter(|(n, _)| j4n_core::host::key_like(n)) {
        red.register(&name, &value);
    }
    // the keys typed in Library › Providers: the redactor knows each before the Log is opened
    let keys = j4n_core::keys::Keys::new(&home);
    notices.extend(keys.register_all(&red));
    let (log, found) = Log::open(&root.join(".j4n"), Scope::Project, LogOpts::default(), red.clone()).map_err(|p| p.to_string())?;
    notices.extend(found.iter().map(ToString::to_string));
    let folds = Folds::new(&root.join(".j4n/folds"));
    let binds = Binds::new();
    let keeper = Keeper::new();
    let lanes = j4n_core::lanes::Lanes::new();
    // the undo stack and the proposals are folds of the Log, so a restart finds them where they were
    let (changes, proposals) = (Changes::new(), Proposals::new());
    notices.extend(folds.add(Box::new(changes.clone()), log.head()).iter().map(ToString::to_string));
    notices.extend(folds.add(Box::new(proposals.clone()), log.head()).iter().map(ToString::to_string));
    // the signed charters: a fold of what the operator signed, read by the rule that answers for them
    let charters = j4n_core::charter::Charters::new();
    notices.extend(folds.add(Box::new(charters.clone()), log.head()).iter().map(ToString::to_string));
    notices.extend(folds.attach(&log).map_err(|p| p.to_string())?.iter().map(ToString::to_string));
    notices.extend(folds.add(Box::new(binds.clone()), log.head()).iter().map(ToString::to_string));
    notices.extend(folds.add(Box::new(keeper.clone()), log.head()).iter().map(ToString::to_string));
    notices.extend(folds.add(Box::new(lanes.clone()), log.head()).iter().map(ToString::to_string));
    notices.extend(folds.add(Box::<j4n_core::cost::PlanRate>::default(), log.head()).iter().map(ToString::to_string));
    let timers = Arc::new(Timers::start(&log));

    let (user, found) = Layer::from_dir(LayerKind::User, "home", &home);
    notices.extend(found.iter().map(ToString::to_string));
    let (here, found) = here_layer(&home, &root);
    notices.extend(found.iter().map(ToString::to_string));
    // which packs are on: the defaults, and the operator's own `enable` rows (never the project's: a clone cannot turn a pack on)
    let (catalog, found) = pack::catalog().with_installed(&packs_dir(&home));
    notices.extend(found.iter().map(ToString::to_string));
    let (rows, found) = enables(vec![user.clone(), here.clone()]);
    notices.extend(found.iter().map(ToString::to_string));
    let (harness, kept) = pack::harness(&catalog, &[user.clone(), here.clone()], &rows);
    let (packs_on_rows, full_packs) = (rows.clone(), j4n_core::pack::says_full(&[user.clone(), here.clone()]));
    // the pack `guard` is the redactor's switch: the Log's clone of `red` is the same one
    let guard = harness.info.iter().any(|i| i.id == "guard" && i.on);
    red.set_guard(guard);
    notices.extend(harness.info.iter().filter(|i| !i.on && !i.note.is_empty()).map(|i| format!("pack {} is left out: {}", i.id, i.note)));
    // what the look's pass made goes over the packs and under the operator's folder, whose entries win
    let (mut layers, made) = (harness.layers.clone(), home.join("cache/look.generated.toml"));
    let made_path = made.clone();
    // under `packs.full` the pack's own switches of the packs that are on are on, below the operator's rows
    layers.extend(Some(full_packs).filter(|f| *f).and_then(|_| j4n_core::pack::full_layer(&harness.layers)));
    layers.extend([j4n_core::front::lookfile::layer(&made), user, here]);
    let (tables, refused) = Tables::load(layers);
    notices.extend(refused.iter().map(ToString::to_string));
    // what the `host` rows say is taken out of every child's environment, from here on
    j4n_connect::proc::install_scrub(j4n_core::host::Scrub::from_tables(&tables));
    // what a pack that is not settled puts on the screen says so; the base pack's own is the screen itself
    let tags: BTreeMap<String, String> = harness.info.iter().filter(|i| i.on && i.id != "base" && !i.tag.is_empty()).map(|i| (i.id.clone(), i.tag.to_string())).collect();
    let (settings, found) = Settings::load(&tables);
    notices.extend(found.iter().map(ToString::to_string));
    // the one live handle on the settings: every reader keeps it, and an edit swaps what they all read
    let live = j4n_core::setting::Live::new(settings.clone());
    let (mut screen, found) = Screen::load(tables.clone(), &settings, harness.info.clone(), &tags);
    screen.folds = Some(folds.clone());
    notices.extend(found.iter().map(ToString::to_string));
    let book = HostBook::new();
    notices.extend(folds.add(Box::new(book.clone()), log.head()).iter().map(ToString::to_string));
    // a gate rule the Table refused is not in the rows: it asks in its place
    let (gates, found) = Gates::load(&tables);
    let gates = gates.distrust(&refused);
    notices.extend(found.iter().map(ToString::to_string));
    let (pts, found) = points(&tables);
    notices.extend(found.iter().map(ToString::to_string));
    let (mut phrases, found) = Phrases::load(&tables);
    notices.extend(found.iter().map(ToString::to_string));
    notices.extend(screen.commands.claim(&mut phrases).iter().map(ToString::to_string));
    let (flows, found) = Flows::load(&tables);
    notices.extend(found.iter().map(ToString::to_string));
    for id in phrases.flows().iter().filter(|f| flows.get(f).is_none()) {
        notices.push(format!("a phrase starts the flow `{id}`, and no such flow loaded"));
    }
    let (ranks, found) = specs(&tables);
    notices.extend(found.iter().map(ToString::to_string));

    let policy = opts.approval.unwrap_or(if interactive { Approval::Ask } else { Approval::Deny });
    let (mut connectors, found) = Connectors::load(&tables);
    notices.extend(found.iter().map(ToString::to_string));
    connectors.set_program("claude", &program);
    // each row that takes a key reads it at each child start: `env:` when set, else J4n's own key file, else its own `file:`
    connectors.keep_keys(keys.dir(), &j4n_core::keys::provider_refs(&tables));
    // the programs the operator gave the key J4n keeps for their provider, in place of their own sign-in
    notices.extend(connectors.bind_keys(&keys, &settings.word("doer.keys", "").split_whitespace().collect::<Vec<_>>()));
    connectors.pass_to_agents(&settings.word("keys.pass", "GH_TOKEN GITHUB_TOKEN").split_whitespace().collect::<Vec<_>>());
    connectors.set_secret({
        let red = red.clone();
        Arc::new(move |name, reference| secret(&red, name, reference))
    });
    // which profile a task of a CLI doer gets; `full` stands for a word that is none
    connectors.set_permit_full(policy == Approval::Full);
    connectors.set_profile(j4n_connect::claude::Pick::parse(settings.word("profile", "full")).unwrap_or_else(|why| {
        notices.push(why);
        Default::default()
    }));
    // a CLI doer works inside the project (not in the operator's user settings, plugins, MCP servers and other sessions) unless `doer.scope` is `user`
    connectors.set_scope(settings.word("doer.scope", "project") != "user");
    let (models, found) = Models::load(&tables);
    notices.extend(found.iter().map(ToString::to_string));
    let mut registry = Registry::new();
    registry.register(Arc::new(intake_rule(phrases.clone())));
    registry.register(Arc::new(weigh_rule(phrases.clone())));
    registry.register(Arc::new(fold_rule(keeper.clone())));
    registry.register(Arc::new(j4n_core::front::planning::plan_rule()));
    registry.register(Arc::new(j4n_core::front::retry::retry_rule()));
    registry.register(Arc::new(j4n_core::front::reflexes::reflex_rule()));
    registry.register(Arc::new(j4n_core::front::access::access_rule()));
    registry.register(Arc::new(named_rule(connectors.all().iter().map(|c| c.id.clone()).collect())));
    // what each doer has done for a role is read from the tasks fold, which the loop makes: the numbers are given to it afterwards
    // the crew's goal (`crew.goal`) and the board's scores it starts from: one handle for the pick of a doer and of a host, kept by the settings after an edit
    let goal = j4n_core::crew::Goal::default();
    j4n_core::setting::Tune::tune(&goal, &settings);
    let (seeds, found) = j4n_core::crew::seeds(&tables);
    notices.extend(found.iter().map(ToString::to_string));
    let standing = Standing::new().with_quality(goal.clone(), seeds);
    registry.register(Arc::new(RankAnswerer::new(ranks, &pts, Box::new(standing.clone()))));
    // deciders and models that answer questions: made with no key too, asked once one is typed; until then a point answers by its rule, rank or fallback
    for a in connectors.answerers(&|name, reference| secret(&red, name, reference)) {
        registry.register(a);
    }
    // a pack that is on and has no key says so on the Packs page: it costs nothing, and does nothing, until the key is there
    let keyless: Vec<&str> = connectors.keyless(&|name, reference| secret(&red, name, reference));
    if let Some(p) = screen.packs.iter_mut().find(|p| p.id == "jev" && p.on).filter(|_| !keyless.is_empty()) {
        p.note = format!("the key is missing for {}: until it is typed in Library › Providers every point answers by its rule, rank or fallback", keyless.join(", "));
    }
    let session = format!("s{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis()));
    let state = Answerers::new().with_limits(Limits { max_wrong: settings.num("trust.max_wrong", 0.25), min_n: settings.num("trust.min_n", 20.0) as usize });
    let state_handle = state.clone();
    let all: std::collections::BTreeMap<String, _> = pts.iter().map(|p| (p.id.clone(), p.clone())).collect();
    let (probes, found) = ProbeSet::load(&tables, &all);
    notices.extend(found.iter().map(ToString::to_string));
    // with `weigh.on` off the point is not asked, so its cases are not put to Jev (a paid call), nothing is required of it, and the point is not walked
    let weigh_on = settings.flag("weigh.on", false);
    let (probes, pts) = if weigh_on { (probes, pts) } else { (probes.without("weigh"), pts.into_iter().filter(|p| p.id != "weigh").collect::<Vec<_>>()) };
    // the same for `librarian.on` (an off switch probes nothing): the point `librarian` and its cases are there only while it is on
    let librarian_on = settings.flag("librarian.on", false);
    let (probes, pts) = if librarian_on { (probes, pts) } else { (probes.without("librarian"), pts.into_iter().filter(|p| p.id != "librarian").collect::<Vec<_>>()) };
    // the same for `chats.on`, and only where a screen uses it (`interactive`: a run or a pipe has no orchestrator): the point `chats` and its cases are there only then
    // the word `orchestrate` wires them too, screen or not (its chats are worked by the run's own loop, which waits for them); the front reads the word live, so
    // `:lanes single` ends the chats and this start keeps what it wired
    let orchestrating = settings.word("lanes", "single") == "orchestrate";
    let chats_flag = settings.flag("chats.on", false) && interactive;
    let chats_on = (settings.flag("chats.on", false) && interactive) || orchestrating;
    let (probes, pts) = if chats_on { (probes, pts) } else { (probes.without("chats"), pts.into_iter().filter(|p| p.id != "chats").collect::<Vec<_>>()) };
    // the read (`read:*`) is walked only under the word `orchestrate`, and `weigh` asks `read:goes_on` too: the points and their cases are there only then
    let (probes, pts) = ["read:kind", "read:goes_on", "read:chat", "read:asked"].into_iter().fold((probes, pts), |(probes, pts), id| {
        if orchestrating || (weigh_on && id == "read:goes_on") { (probes, pts) } else { (probes.without(id), pts.into_iter().filter(|p| p.id != id).collect::<Vec<_>>()) }
    });
    // the point `access` and its cases are there only while chats are worked under the word `orchestrate` with `orchestrate.access` on
    let access_on = orchestrating && settings.flag("orchestrate.access", false);
    let (probes, pts) = if access_on { (probes, pts) } else { (probes.without("access"), pts.into_iter().filter(|p| p.id != "access").collect::<Vec<_>>()) };
    // the same for `reflexes.on`: the point `reflex` and its cases are there only while it is on
    let reflexes_on = settings.flag("reflexes.on", false);
    let (probes, pts) = if reflexes_on { (probes, pts) } else { (probes.without("reflex"), pts.into_iter().filter(|p| p.id != "reflex").collect::<Vec<_>>()) };
    // the same for `complete.decider` (and `complete.on`, without which nothing is completed): the point `complete` and its cases are there
    // only while both are on, so a screen that never asks the Decider for its order pays for no proof of it
    let complete_on = settings.flag("complete.on", true) && settings.flag("complete.decider", true);
    let (probes, pts) = if complete_on { (probes, pts) } else { (probes.without("complete"), pts.into_iter().filter(|p| p.id != "complete").collect::<Vec<_>>()) };
    // the same for `approval.inside`: the point `inside`, where a Decider reads the lines code cannot, and its cases are there only while it is on
    let (probes, pts) = if settings.flag("approval.inside", false) { (probes, pts) } else { (probes.without("inside"), pts.into_iter().filter(|p| p.id != "inside").collect::<Vec<_>>()) };
    let points: std::collections::BTreeMap<String, _> = pts.iter().map(|p| (p.id.clone(), p.clone())).collect();
    state.require(probes.hashes());
    state.chains(pts.iter().map(|p| (p.id.clone(), p.chain.iter().map(ToString::to_string).collect())).collect());
    let gate = Gate::new(gates, Grants::new(&session)).with_state(j4n_core::gate::foreign::Own {
        user: std::env::var("HOME").ok().filter(|h| !h.is_empty()),
        j4n: Some(home.display().to_string()),
        root: Some(root.display().to_string()),
    });
    let asks = Asks::new(pts, registry, state.clone()).with_gate(gate.clone());
    let says = j4n_core::answer::point::says(&tables);
    let prober = Prober::new(log.clone(), asks.registry.clone(), probes, points, state).with_says(says.clone()).with_record(j4n_core::answer::record::Record::at(&home));
    let asks = asks.with_prober(prober).with_says(says).with_probe_wait(std::time::Duration::from_secs_f64(settings.num("probe.wait", 30.0).max(0.0))).headless(policy == Approval::Deny).full(policy == Approval::Full);
    // a charter answers only where the operator is there to be asked about the rest: a screen, or a piped run that asks, never a run that denies or says yes to all
    let asks = if interactive || policy == Approval::Ask { asks.with_charters(charters.clone()) } else { asks };
    // `approval.inside` (on under the word `orchestrate`): what stays inside the project is taken by the Gate, and a Decider reads what code cannot
    gate.set_inside(asks.inside_allowed(&settings));
    let asks = Arc::new(asks);
    let trust = state_handle.clone();

    connectors.set_gate(gate.clone());
    // the pack `guard` also holds a model run to the taint rules once it has read from outside
    connectors.set_guard(guard);
    // the pack `kit`: a model that names `py` or `*` is given the tool `py` while `kit.mode` is `on`; with no python3 it is not
    let kit = harness.info.iter().any(|i| i.id == "kit" && i.on) && settings.word("kit.mode", "off") == "on";
    connectors.set_kit(kit);
    // the librarian's shelf is given to the connectors only while its switch is on: with it off no doer is offered the tool `context`
    let shelf = Arc::new(j4n_core::front::librarian::Shelf::default());
    connectors.set_librarian(librarian_on.then(|| shelf.clone()));
    // the post's mailbox is given to the connectors only where chats are on (and a screen uses them): with it off no doer is offered the tool `tell`
    let mailbox = Arc::new(j4n_core::front::messages::Mailbox::default());
    connectors.set_post(chats_on.then(|| mailbox.clone()));
    // the tool `chat`, which starts a scout, is offered beside it only under the word `orchestrate` with the quick look's switch on
    connectors.set_scouts(orchestrating && settings.flag("orchestrate.look", false));
    // the envoy's tools, with the crew and its envoys on under the same word
    connectors.set_envoys(orchestrating && settings.flag("crew.on", false) && settings.flag("crew.envoys", false));
    // the tool `access`, which asks for a read-only copy of another part's file, with `orchestrate.access` on under the same word
    connectors.set_access(access_on);
    // the parts of the kit the operator turned off, and the fold that counts what the cells use of it: only while it is on
    let off: Vec<String> = settings.word("kit.off", "").split_whitespace().map(String::from).collect();
    if kit {
        let recipes: Vec<&str> = j4n_connect::py::RECIPES.iter().map(|r| r.0).collect();
        notices.extend(folds.add(Box::new(j4n_connect::kituse::KitUse::new(off.clone(), &recipes)), log.head()).iter().map(ToString::to_string));
    }
    // what the kit's host calls are held to: kit.judge (off until the operator turns it on), the redactor, and the egress Gate
    let registry = asks.registry.clone();
    connectors.set_kit_set(j4n_connect::kithost::KitSet { home: home.clone(), uv: "uv".into(), grants: settings.word("kit.grants", "").split_whitespace().map(String::from).collect(), off, judge: kit && settings.word("kit.judge", "off") == "on", keep: settings.num("kit.keep", 0.15), model: settings.word("kit.model", "light").into(), red: red.clone(), egress: Arc::new(move |zone| registry.allows(j4n_core::schema::DataClass::Project, zone)) });
    if let Some(p) = screen.packs.iter_mut().find(|p| p.id == "kit").filter(|_| kit && !j4n_connect::py::python_here()) {
        p.note = "there is no python3 on the PATH, so the tool py is not offered".into();
    }
    // the tools that reach out: the web, and the MCP servers a model names. A tool a server offers that nobody has pinned is
    // proposed to the operator once the front exists (`pin_to` is set then).
    let pin_to: Arc<OnceLock<Arc<Proposer>>> = Arc::default();
    let watch_to: Arc<OnceLock<Arc<j4n_connect::watch::Watcher>>> = Arc::default();
    let outside = Arc::new(Outside {
        secret: {
            let red = red.clone();
            Arc::new(move |name, reference| secret(&red, name, reference))
        },
        gate: Some(gate.clone()),
        local: matches!(settings.get("web.local"), Some(Value::Bool(true))),
        search: Some(settings.word("web.search", "")).filter(|id| !id.is_empty()).and_then(|id| connectors.get(id).cloned()),
        pinner: {
            let pin_to = pin_to.clone();
            let now: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>> = connectors.all().iter().map(|c| (c.id.clone(), c.pins.clone())).collect();
            Some(Arc::new(move |id, found| {
                if let Some(p) = pin_to.get() {
                    let draft = j4n_reach::mcp::tools::pin_draft(id, &now.get(id).cloned().unwrap_or_default(), found);
                    let _ = p.make(draft, Actor::Core, None);
                }
            }))
        },
        listing: Some({
            let watch_to = watch_to.clone();
            Arc::new(move |id, tools| {
                if let Some(w) = watch_to.get() {
                    w.observe(&format!("mcp:{id}"), tools);
                }
            })
        }),
        // the pack `guard` looks at an MCP tool again before each call and tells the Log what it stopped
        alert: guard.then(|| {
            let log = log.clone();
            Arc::new(move |data| {
                let _ = log.append(New::of("guard.alert", Actor::Core, data));
            }) as j4n_reach::mcp::tools::Alerter
        }),
    });
    // the memory: one derived index of the Log, in a file that can be deleted; a model that names `memory` among its tools may recall
    let memory = match Index::open(&root.join(".j4n/memory.db")) {
        Ok(i) => Arc::new(i),
        Err(why) => {
            notices.push(format!("{why}; the memory is kept in memory only"));
            Arc::new(Index::in_memory().map_err(|e| e.to_string())?)
        }
    };
    let reach: Arc<dyn Reach> = Arc::new(Remembering { outside: outside.clone(), memory: Arc::new(MemoryTools::new(memory.clone(), log.clone())) });
    connectors.set_reach(reach.clone());
    let (doers, found) = connectors.doers_with(&|name, reference| secret(&red, name, reference), &models);
    notices.extend(found.iter().map(ToString::to_string));
    // a flow is a doer too, `flow:<id>`; it finds the loop once the loop is there
    let link = Link::default();
    let flow_doers = flow_run::doers(&flows, &link);
    let (rules, found) = Rules::load(&tables);
    notices.extend(found.iter().map(ToString::to_string));
    let (failures, found) = Failures::load(&tables);
    notices.extend(found.iter().map(ToString::to_string));
    let (mut plans, found) = Plans::load(&tables);
    notices.extend(found.iter().map(ToString::to_string));
    let (counts, found) = declared(&tables, Scope::Project);
    notices.extend(found.iter().map(ToString::to_string));
    for fold in counts {
        notices.extend(folds.add(fold, log.head()).iter().map(ToString::to_string));
    }
    let (check, found) = Check::load(&tables);
    notices.extend(found.iter().map(ToString::to_string));
    // the Connect flow's steps are doers; the Proposer they make a proposal with exists once the front does
    let connect_link = connect::Link::default();
    connect_link.set_auto(settings.flag("connect.auto", false));
    let (connect_doers, found) = connect::doers(&tables, &connect_link);
    notices.extend(found.iter().map(ToString::to_string));
    let (walls, found) = boxed::rows(&tables);
    notices.extend(found.iter().map(ToString::to_string));
    let user_home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| home.clone());
    let worktree = Arc::new(Worktree::new(&home, &root, Apply::parse(settings.word("work.apply", "auto")), Checkpoints::default()));
    let here: Arc<dyn Place> = Arc::new(Here::new(&home, &root, Checkpoints::default()));
    let mut cfg = Loop::new(&log, &folds, &timers, &root, &home);
    cfg.places.push(worktree.clone());
    if !walls.is_empty() {
        // the pack `guard` also lets a box show a secret file as its view
        let boxed = |id: &str, inner: Arc<dyn Place>, walls: Vec<boxed::BoxRow>| {
            let place = Boxed::new(id, inner, walls, &settings, &user_home, &home);
            Arc::new(if guard { place.guarded(&red, &settings) } else { place })
        };
        cfg.places.push(boxed("box", here, walls.clone()));
        cfg.places.push(boxed("worktree-box", worktree.clone(), walls));
    }
    cfg.doers = doers.clone();
    cfg.doers.extend(flow_doers.iter().cloned());
    let check_names = check.names();
    let check_lines = check.lines();
    let check: Arc<dyn Doer> = Arc::new(check);
    cfg.doers.push(check.clone());
    cfg.doers.extend(connect_doers);
    // accounts: the doer that keeps them, and what a task that runs as one is launched as
    let (accounts, found) = account::Book::load(&tables, &connectors, &home.join("accounts"), &user_home);
    notices.extend(found.iter().map(ToString::to_string));
    // the tables as the files are now: a proposal writes through it, and the accounts read it again when one is accepted
    let fresh: Arc<dyn Fn() -> Tables + Send + Sync> = {
        let (layers, home, root) = (harness.layers.clone(), home.clone(), root.clone());
        Arc::new(move || {
            let mut all = layers.clone();
            all.push(Layer::from_dir(LayerKind::User, "home", &home).0);
            all.push(here_layer(&home, &root).0);
            Tables::load(all).0
        })
    };
    let accounts = accounts.with_reload(fresh.clone(), vec![home.join("account"), here_dir(&home, &root).join("account")]);
    // the plan an account names pays for its requests
    notices.extend(plans.name(accounts.plans()).iter().map(ToString::to_string));
    let accounts = Arc::new(accounts);
    cfg.launch = Some(accounts.launcher());
    cfg.doers.push(account::doer(accounts.clone(), &connect_link, log.clone()));
    cfg.doers.extend(j4n_memory::doers(memory.clone(), log.clone()));
    // the bench: cases are data, and a run has the doer under test say each line in a folder of its own
    let (bench, found) = Bench::load(&tables, &home.join("bench"), log.clone());
    notices.extend(found.iter().map(ToString::to_string));
    cfg.doers.push(bench.doer());
    // prompt shaping, when the pack is on: the desk the front asks, and the doer a draft starts
    let shape = harness.info.iter().any(|i| i.id == "shape" && i.on).then(|| j4n_shape::load(&tables, &settings, &root, &log));
    if let Some((desk, shaper, found)) = &shape {
        notices.extend(found.iter().cloned());
        cfg.doers.push(shaper.doer());
        if opts.shape {
            desk.force_on();
        }
    } else if opts.shape {
        notices.push("--shape needs the pack shape: Grow › Packs › shape, ⏎".into());
    }
    // the result ledger, when the pack is on (its switch, the setting `ledger.mode`, is on): the book and the doer of its paragraph
    let ledger = harness.info.iter().any(|i| i.id == "ledger" && i.on).then(|| Arc::new(j4n_ledger::Book::new(&log, &settings)));
    cfg.doers.extend(ledger.iter().map(j4n_ledger::Book::doer));
    // the part runner, when the pack is on (its switch, the setting `parts.mode`, is off until it is turned on): the runner and the doer of its gates and commits
    let parts = harness.info.iter().any(|i| i.id == "parts" && i.on).then(|| Arc::new(j4n_parts::Runner::new(&log, &settings, &root)));
    cfg.doers.extend(parts.iter().map(|_| j4n_parts::Runner::doer()));
    // the habit detectors are pack rules; what needs code is the operator's label on a row (`:habit keep|wrong`)
    cfg.doers.extend(harness.info.iter().any(|i| i.id == "habits" && i.on).then(|| j4n_habits::doer(&log)));
    // the kit's notebook (`:kit export`), when the pack is on
    cfg.doers.extend(harness.info.iter().any(|i| i.id == "kit" && i.on).then(|| j4n_connect::kitexport::doer(&log, &root)));
    // quality control, when the pack is on: the doer the rules and `:qc` start, and Jev for its second tier when there is a key
    let qc = harness.info.iter().any(|i| i.id == "qc" && i.on).then(|| j4n_qc::load(&tables, &settings, &root, &home, &log, &red));
    if let Some((checker, found)) = &qc {
        notices.extend(found.iter().cloned());
        cfg.doers.extend([checker.doer(), checker.keeper()]);
        if let Some(jev) = connectors.decider("jev", &|name, reference| secret(&red, name, reference)) {
            let registry = asks.registry.clone();
            checker.set_jev(jev, Arc::new(move |zone| registry.allows(j4n_core::schema::DataClass::Project, zone)));
        }
    }
    cfg.doers.push(Arc::new(Spot::new(asks.clone(), log.clone())));
    cfg.asks = Some(asks.clone());
    cfg.rules = Arc::new(rules);
    cfg.failures = Arc::new(failures);
    let plans = Arc::new(plans);
    let live_plans = j4n_core::cost::LivePlans::of(plans.clone());
    cfg.plans = plans.clone();
    cfg.settings = settings.clone();
    cfg.live = Some(live.clone());
    cfg.live_plans = Some(live_plans.clone());
    cfg.proposals = Some(proposals.clone());
    let (tasks, found) = Tasks::start(cfg).map_err(|p| p.to_string())?;
    notices.extend(found.iter().map(ToString::to_string));
    standing.set(tasks.book().clone());
    // work a task left in a worktree when J4n stopped, and nobody took: its row says so, and it can be taken
    for (task, kept) in worktree.recover() {
        let (dir, counts) = (kept.dir.clone(), kept.work.counts());
        if tasks.keep(&task, kept) {
            notices.push(format!("{task} left work in {dir} that is not in your folder: {counts}"));
        }
    }

    let mut front = Cfg::new(&root.file_name().map_or_else(|| root.display().to_string(), |n| n.to_string_lossy().into_owned()));
    front.thread = thread.into();
    front.root = root.clone();
    // the models each doer has: a model word is matched against them, here for `--model` and later for `:switch`
    let lists = Arc::new(
        j4n_connect::modellist::ModelsOf::new(connectors.all(), {
            let red = red.clone();
            Arc::new(move |name, reference| secret(&red, name, reference))
        })
        .cached(j4n_core::listing::Cache::new(&home)),
    );
    front.models = lists.clone();
    // the pre-flight: `view:setup` and the notice of a new folder ask the doers' own probes, apart from the screen's work, each landing as it answers
    front.setup = Some({
        let (rows, doers, plans, red) = (Arc::new(connectors.all().to_vec()), doers.clone(), plans.clone(), red.clone());
        let (full, on) = (full_packs, pack::why_on(&harness, &kept, &packs_on_rows, full_packs));
        let (accounts, live) = (harness.info.iter().any(|i| i.id == "accounts" && i.on), live.clone());
        let (probe_rows, probe_doers) = (rows.clone(), doers.clone());
        j4n_core::doctor::Check::new(
            move || j4n_core::doctor::Facts { kept: on.clone(), full, orchestrating: live.word("lanes", "single") == "orchestrate", ..crate::doctor::frame(&rows, &doers, &plans, &|name, reference| secret(&red, name, reference), accounts) },
            move |id| crate::doctor::probe(&probe_rows, &probe_doers, accounts, id),
        )
    });
    // the skills of the packs that are on and of the home folder: a task is given the few that fit it; the pack `skills` off, none
    front.skills = harness.info.iter().any(|i| i.id == "skills" && i.on).then(|| {
        // each connection has a skill made from its row; the pack's, the operator's own of the same name come after it and replace it
        let mut all: Vec<skill::Skill> = tables.ids("connector").into_iter().filter_map(|id| tables.raw("connector", id).and_then(|m| skill::of_connector(id, &m.fields))).collect();
        for (pack, folder, text) in pack::SKILL_FILES.iter().filter(|(p, ..)| harness.info.iter().any(|i| i.on && i.id == *p)) {
            match skill::parse(&format!("pack:{pack}"), folder, text) {
                Ok(s) => all.push(s),
                Err(why) => notices.push(format!("skill {folder} of {pack}: {why}")),
            }
        }
        let places = harness.info.iter().filter(|i| i.on).map(|i| (format!("pack:{}", i.id), packs_dir(&home).join(&i.id).join("skills"))).chain([("home".to_string(), home.join("skills"))]);
        for (from, dir) in places {
            let (found, bad) = skill::Library::read_dir(&from, &dir);
            all.extend(found);
            notices.extend(bad);
        }
        Arc::new(skill::Library::new(all, settings.word("skills.off", "")).in_project(&root))
    });
    // the doers that are a key on a gateway (`own` on openrouter.ai): `/usage` says what each gateway was spent today
    // the logins there are, as the files say when `/usage` asks: each is a row there, whether or not it has shown a window
    front.accounts = Some({
        let accounts = accounts.clone();
        Arc::new(move || accounts.logins())
    });
    // where a login keeps its doers' sessions: an archived chat's are deleted when it is removed
    front.homes = Some({
        let (user_home, home) = (user_home.clone(), home.clone());
        Arc::new(move |account: &str| if account.is_empty() { (user_home.join(".claude"), user_home.clone()) } else { let h = home.join("accounts").join(account).join("home"); (h.clone(), h) })
    });
    front.gateways = connectors.all().iter().filter(|c| c.key.is_some()).filter_map(|c| Some((c.id.clone(), gateway_of(c.url.as_deref()?)?))).collect();
    // offload: what the gateway key has left (the switch, its numbers and the main lane's pick are `Cfg::apply`'s)
    front.credits = connectors.all().iter().find(|c| c.key.is_some() && c.url.as_deref().and_then(gateway_of).as_deref() == Some("openrouter")).and_then(|c| Some((c.url.clone()?, c.clone()))).map(|(base, row)| {
        let red = red.clone();
        j4n_core::front::usage::Credits::new(move || {
            // the gateway's key where it is now: the environment, else the one J4n keeps
            let key = row.read_key(&|name, reference| secret(&red, name, reference)).map_err(|why| why.map_or_else(|| format!("there is no key for {}", row.id), |why| format!("the key of {} is not used: {why}", row.id)))?;
            j4n_connect::credits::ask(&j4n_connect::net::Net::new().idle(std::time::Duration::from_secs(10)), &base, &key)
        })
    });
    front.keys = Some(keys.clone());
    // the free model lists the library shows: read once a day into the cache by a timer of its own, on a screen only, never at the start
    let cache = j4n_core::listing::Cache::new(&home);
    front.listing = Some(cache.clone());
    if interactive {
        let source = Arc::new(j4n_connect::modellist::Real::new({
            let red = red.clone();
            Arc::new(move |name, reference| secret(&red, name, reference))
        }));
        j4n_connect::modellist::Listing::start(Arc::new(j4n_connect::modellist::Listing::new(cache, connectors.all(), source, Some(live.clone()))));
    }
    front.doer = opts.doer.clone();
    front.model = match &opts.model {
        Some(word) => Some(model_word(lists.as_ref(), &connectors, opts.doer.as_deref(), word)?),
        None => None,
    };
    front.mode = opts.mode;
    front.place = opts.place.clone();
    front.usd_soft = opts.usd;
    front.deadline = opts.wall.map(|w| started + w);
    // which host serves a model: the switch is off until the operator turns it on; the book is the Log's account of each host
    let route = Route::of(&settings);
    let mut election = Election::new(models, book, route, Some(Arc::new(gate)));
    election.record = Arc::new(BookRecord(tasks.book().clone()));
    election.plan = Arc::new(j4n_core::cost::PlanPricing { plans: live_plans.clone(), folds: folds.clone(), settings: live.clone() });
    front.hosts = Arc::new(election);
    let (branches, found) = j4n_core::branch::Rules::load(&tables);
    notices.extend(found.iter().map(ToString::to_string));
    front.branches = branches;
    front.checks = check_names;
    front.check_lines = check_lines;
    // the fields the settings fill, the same that an edit fills again while J4n runs
    notices.extend(front.apply(&settings, &tables));
    // where a line goes by its weight; off until it is measured on mixed lines. The switches below are wired
    // at the start (`live = false`): what they decide is made here
    front.weigh.on = weigh_on;
    front.weigh.lean = connectors.lean_doers("light");
    // reflexes, behind one switch, off by default
    front.reflexes.on = reflexes_on;
    // the librarian, off by default: the doers that can ask are those the connectors offer `context` to
    (front.librarian.on, front.librarian.asks) = (librarian_on, connectors.context_doers());
    front.orchestrate.scouts = connectors.scout_doers();
    front.charters = charters.clone();
    charters.set_on(settings.flag("crew.on", false));
    // chats, off by default, and only on a screen: a run or a pipe has one thread and no orchestrator
    (front.chats.on, front.chats.set, front.chats.screen, front.chats.wired) = (chats_flag, settings.flag("chats.on", false), interactive || orchestrating, chats_on);
    // the doers the connectors offer `tell` to are told of it on the first line of their session
    front.chats.tells = connectors.tell_doers();
    front.look_made = Some(made);
    front.card = Some(Arc::new(j4n_memory::ProjectCard::new(&root, &live)));
    front.recall = Some(Arc::new(j4n_memory::Recall::new(memory.clone(), log.clone(), &root, &live)));
    // the tools J4n follows: a look on a timer while the switch is on, and `:watch` either way
    let (sources, found) = j4n_connect::watch::Source::load(&tables);
    notices.extend(found.iter().map(ToString::to_string));
    let uses = connectors.all().iter().map(|c| (format!("connector:{}", c.id), j4n_connect::relevance::Uses::of_connector(c))).collect();
    let watch = Arc::new(j4n_connect::watch::Watcher::new(j4n_connect::net::Net::new(), &log, &folds, &live, sources).with_uses(uses));
    j4n_connect::watch::Watcher::start(&watch);
    let _ = watch_to.set(watch.clone());
    front.upstream = Some(watch);
    // what finds what is wrong with J4n and keeps it here, when the pack is on
    if harness.info.iter().any(|i| i.id == "contribute" && i.on) {
        let finder = Arc::new(j4n_contribute::Finder::new(&log, &folds, &live, &tables, &proposals));
        j4n_contribute::Finder::start(&finder);
        front.contribute = Some(finder);
    }
    // the lines typed while a task works, when the pack is on (its switch, the setting `inbox`, is off until it is turned on)
    if harness.info.iter().any(|i| i.id == "inbox" && i.on) {
        front.inbox = Some(Arc::new(j4n_inbox::Desk::new(&log, &folds, &settings, &tables)));
    }
    // what else keeps a copy of some settings and takes new ones after an edit
    front.tunes = vec![worktree.clone(), Arc::new(trust), asks.clone(), Arc::new(goal.clone())];
    front.ledger = ledger.clone().map(|l| l as Arc<dyn j4n_core::front::ledger::Ledger>);
    front.parts = parts.map(|p| p as Arc<dyn j4n_core::front::parts::Parts>);
    // what no decider or model has proved yet, or proved too long ago, is put to them now, off the path of any line
    if let Some(prober) = asks.prober.clone() {
        let now = log.clock().now_ms();
        if !prober.due(now).is_empty() {
            std::thread::spawn(move || drop(prober.run_due(now)));
        }
    }
    // who is elected for a role: the doers a connector says are good for it, over what the Log says each has done; a flow elects
    // through the same
    let roles = connectors.all().iter().filter(|c| !c.roles.is_empty()).map(|c| (c.id.clone(), c.roles.clone())).collect();
    let docs = connectors.all().iter().filter(|c| !c.doc.is_empty()).map(|c| (c.id.clone(), c.doc.clone())).collect();
    let pin = j4n_core::front::who::Pin { doer: front.doer.clone(), model: front.model.clone() };
    let who = Arc::new(Who::new(asks.clone(), &doers, roles, front.hosts.clone(), log.clone()).with_docs(docs).with_pin(pin, lists.clone()).with_limit_readers(connectors.all().iter().filter(|c| !c.limits.is_empty()).map(|c| c.id.clone()).collect()).with_goal(goal.clone()));
    link.set(Arc::new(TaskHost { tasks: tasks.downgrade(), who: who.clone(), log: log.clone(), lanes: lanes.clone(), root: root.clone(), settings: live.clone() }));
    let mut everyone = doers.clone();
    everyone.extend(flow_doers);
    bench.set_doers(everyone.clone());
    if let Some((_, shaper, _)) = &shape {
        shaper.set_doers(everyone.clone());
    }
    if let Some(l) = &ledger {
        l.set_doers(everyone.clone());
    }
    // a proposal writes the operator's folder for this project (`~/.j4n/projects/<key>`), the layer above the packs and the home layer
    let plant = Plant {
        load: Box::new(move || fresh()),
        writer: Writer::new(Some(here_dir(&home, &root)), None).with_home(home.clone()),
    };
    let clock = log.clock();
    let proposer = Arc::new(Proposer::new(log.clone(), proposals, changes, Arc::new(plant), Arc::new(move || clock.now_ms())));
    connect_link.set(proposer.clone());
    connect_link.set_reach(reach);
    let _ = pin_to.set(proposer.clone());
    if let Some(w) = watch_to.get() {
        w.propose_to(proposer.clone());
    }
    // the front starts the doer `check` itself (a held piece's checks), though no election may pick it
    let known: Vec<Arc<dyn Doer>> = everyone.iter().cloned().chain([check]).collect();
    // after an edit the files are read again as the screen read them at the start: the packs, the look's pass, the operator's folders
    let reload: j4n_core::front::Reload = {
        let (layers, home, root, made) = (harness.layers.clone(), home.clone(), root.clone(), made_path.clone());
        Arc::new(move || {
            let mut all = layers.clone();
            all.extend([j4n_core::front::lookfile::layer(&made), Layer::from_dir(LayerKind::User, "home", &home).0, here_layer(&home, &root).0]);
            Tables::load(all).0
        })
    };
    let mut front = Front::new(front, tasks, log, binds, asks, phrases, known).with_who(who).with_screen(Arc::new(screen)).with_proposer(proposer).with_keeper(keeper).with_lanes(lanes).with_librarian(&shelf).with_post(&mailbox).with_live(live, live_plans, reload);
    // the rule that answers for a signed charter asks this program for the Gate, the clock, the tree of a task and what a tree has spent
    if let Some(hooks) = front.charter_hooks() {
        charters.attach(hooks);
    }
    // what an envoy chat read from outside in one turn is held for the chat, so what it sends in another turn carries it
    if let Some(gate) = front.who().gate() {
        gate.hold_by(front.envoy_hold_hook());
    }
    if let Some((desk, _, _)) = shape {
        front = front.with_desk(desk);
    }
    bench.set_brief(front.brief_hook());
    front.follow_edits();
    Ok(World { front, doers: everyone, notices, _timers: timers })
}

/// The tools that reach out, and the memory among them: `memory` is the model's `recall`, anything else is the outside's.
struct Remembering {
    outside: Arc<Outside>,
    memory: Arc<MemoryTools>,
}

impl Reach for Remembering {
    fn tools(&self, word: &str, row: Option<&Connector>) -> Option<Arc<dyn j4n_connect::model::doer::Tools>> {
        if word == "memory" { Some(self.memory.clone()) } else { self.outside.tools(word, row) }
    }

    fn survey(&self, row: &Connector, wrap: &dyn Fn(Launch) -> Launch, cwd: &Path) -> Result<Survey, String> {
        self.outside.survey(row, wrap, cwd)
    }
}

/// `--model WORD` as a model in full: matched against the models of `--doer`, or, with no doer named, of the one doer that has such a
/// model. A word that matches none, or more than one, is refused here, before anything starts; it is never passed on as typed.
fn model_word(lists: &dyn ModelLists, connectors: &Connectors, doer: Option<&str>, word: &str) -> Result<String, String> {
    let one = |doer: &str| -> Result<Option<String>, String> {
        match j4n_core::front::models::settle(lists, doer, word) {
            Ok(Some(Resolved::One(m))) => Ok(Some(m)),
            Ok(Some(Resolved::Many(all))) => Err(format!("--model {word}: {} models of {doer} match: {}; say which", all.len(), all.join(", "))),
            Ok(_) => Ok(Some(word.to_string())),
            Err(why) => Err(format!("--model {word}: {why}")),
        }
    };
    if let Some(d) = doer {
        return Ok(one(d)?.unwrap_or_else(|| word.to_string()));
    }
    let mut found: Vec<(String, String)> = Vec::new();
    let mut last = String::new();
    for c in connectors.all().iter().filter(|c| matches!(c.is, j4n_connect::manifest::Is::Agent | j4n_connect::manifest::Is::Model)) {
        match one(&c.id) {
            Ok(Some(m)) => found.push((c.id.clone(), m)),
            Ok(None) => {}
            Err(why) => last = why,
        }
    }
    match found.as_slice() {
        [(_, m)] => Ok(m.clone()),
        [] => Err(if last.is_empty() { format!("--model {word}: no doer has a model like it") } else { last }),
        many => Err(format!("--model {word}: {} match it; name the doer with --doer", many.iter().map(|(d, m)| format!("{d} ({m})")).collect::<Vec<_>>().join(", "))),
    }
}

/// The name of a gateway from its URL: `https://openrouter.ai/api/v1` is `openrouter`.
fn gateway_of(url: &str) -> Option<String> {
    let host = url.split("://").nth(1)?.split(['/', ':']).next()?;
    host.rsplit('.').nth(1).or(Some(host)).filter(|h| !h.is_empty()).map(String::from)
}
