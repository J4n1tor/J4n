//! The loop over a project with the doers a manifest makes: connectors written as TOML, programs written as scripts.

use crate::world::{Scratch, script};
use j4n_connect::manifest::Connectors;
use j4n_core::answer::chain::Registry;
use j4n_core::answer::point::points;
use j4n_core::answer::state::Answerers;
use j4n_core::ask::Answer;
use j4n_core::change::Actor;
use j4n_core::clock::ManualClock;
use j4n_core::doer::Doer;
use j4n_core::fold::Folds;
use j4n_core::gate::Gate;
use j4n_core::gate::check::Gates;
use j4n_core::gate::grant::Grants;
use j4n_core::log::{Ev, Log, Opts, Scope};
use j4n_core::redact::Redactor;
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use j4n_core::task::{Asks, Config as Loop, Ctl, Row, State, TaskSpec, Tasks};
use j4n_core::timer::Timers;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long a test waits for something that is going to happen: a wait returns the moment its condition holds, so a long bound costs
/// nothing on a quiet machine, and a short one fails on a machine that other builds share.
const PATIENCE: Duration = Duration::from_secs(90);

pub struct Bench {
    pub tasks: Tasks,
    pub log: Log,
    pub project: Scratch,
    pub bin: Scratch,
    pub folds: Folds,
    _home: Scratch,
    _timers: Arc<Timers>,
}

/// The base pack's data that the loop needs, as one layer.
pub fn base_layer() -> Layer {
    let files = |list: &[(&str, &str)]| list.iter().map(|(p, t)| SourceFile { path: p.to_string(), text: t.to_string() }).collect::<Vec<_>>();
    Layer::new(
        LayerKind::Pack,
        "data",
        files(&[
            ("shell.toml", include_str!("../../../../packs/base/gates/shell.toml")),
            ("modes.toml", include_str!("../../../../packs/base/gates/modes.toml")),
            ("paths.toml", include_str!("../../../../packs/base/gates/paths.toml")),
            ("egress.toml", include_str!("../../../../packs/base/gates/egress.toml")),
            ("points.toml", include_str!("../../../../packs/base/asks/points.toml")),
        ]),
    )
}

/// The connectors of `manifest` (`{bin}` is where the scripts are), and what loading them said.
pub fn connectors(bin: &Scratch, manifest: &str) -> (Connectors, Vec<String>, Tables) {
    let text = manifest.replace("{bin}", &bin.path().display().to_string());
    let rows = Layer::new(LayerKind::User, "connectors", vec![SourceFile { path: "connectors/x.toml".into(), text }]);
    let (tables, problems) = Tables::load(vec![base_layer(), rows]);
    let mut said: Vec<String> = problems.iter().map(|p| p.to_string()).collect();
    let (list, more) = Connectors::load(&tables);
    said.extend(more.iter().map(|p| p.to_string()));
    (list, said, tables)
}

impl Bench {
    /// `programs` are written into a folder of their own (`{bin}` in the manifest); every connector must load clean.
    pub fn new(programs: &[(&str, &str)], manifest: &str) -> Bench {
        let bin = Scratch::new("bin");
        for (name, body) in programs {
            script(&bin, name, body);
        }
        let (list, said, tables) = connectors(&bin, manifest);
        assert!(said.is_empty(), "{said:?}");
        let (doers, said) = list.doers(&|_, r| Err(format!("no secret `{r}`")));
        assert!(said.is_empty(), "{said:?}");
        Bench::over(bin, doers, &tables)
    }

    pub fn over(bin: Scratch, doers: Vec<Arc<dyn Doer>>, tables: &Tables) -> Bench {
        Bench::tuned(bin, doers, tables, |_| {})
    }

    /// [`Bench::over`], with the loop's config changed first (a place of the test's own).
    pub fn tuned(bin: Scratch, doers: Vec<Arc<dyn Doer>>, tables: &Tables, tune: impl FnOnce(&mut Loop)) -> Bench {
        let (gates, _) = Gates::load(tables);
        Bench::gated(bin, doers, tables, Gate::new(gates, Grants::new("s1")), tune)
    }

    /// [`Bench::tuned`], with the Gate the loop asks given by the test (the tools and the doer hold the same one, as in the binary).
    pub fn gated(bin: Scratch, doers: Vec<Arc<dyn Doer>>, tables: &Tables, gate: Gate, tune: impl FnOnce(&mut Loop)) -> Bench {
        Bench::gated_full(bin, doers, tables, gate, false, tune)
    }

    /// [`Bench::gated`], under `--approval full` when `full` (nobody is asked: the approvals nobody blocked are a yes, and the rest a no).
    pub fn gated_full(bin: Scratch, doers: Vec<Arc<dyn Doer>>, tables: &Tables, gate: Gate, full: bool, tune: impl FnOnce(&mut Loop)) -> Bench {
        let (project, home) = (Scratch::new("proj"), Scratch::new("home"));
        let opts = Opts { segment_bytes: 8 << 20, max_line: 1 << 20, checkpoint_every: 1000, clock: Arc::new(ManualClock::at(1_000)) };
        let (log, problems) = Log::open(&project.join(".j4n"), Scope::Project, opts, Redactor::new()).expect("opens");
        assert!(problems.is_empty(), "{problems:?}");
        let folds = Folds::new(&project.join(".j4n/folds"));
        folds.attach(&log).expect("attaches");
        let timers = Arc::new(Timers::start(&log));
        let (pts, _) = points(tables);
        let mut cfg = Loop::new(&log, &folds, &timers, project.path(), home.path());
        cfg.doers = doers;
        tune(&mut cfg);
        cfg.asks = Some(Arc::new(Asks::new(pts, Registry::new(), Answerers::new()).with_gate(gate).full(full)));
        let (tasks, problems) = Tasks::start(cfg).expect("the loop starts");
        assert!(problems.is_empty(), "{problems:?}");
        Bench { tasks, log, project, bin, folds, _home: home, _timers: timers }
    }

    pub fn spawn(&self, doer: &str, goal: &str) -> String {
        self.tasks.spawn(TaskSpec::new(goal, doer), Actor::Operator).expect("spawns")
    }

    pub fn end(&self, id: &str) -> Row {
        self.tasks.book().wait(id, |r| r.state.is_terminal(), PATIENCE).unwrap_or_else(|| panic!("{id} never ended\n{}", self.dump()))
    }

    /// Run to the end, saying yes to every question the operator is put (a tool is asked of the Gate, and a line nothing
    /// allows waits for a person).
    pub fn run(&self, doer: &str, goal: &str) -> Row {
        self.run_spec(TaskSpec::new(goal, doer))
    }

    /// [`Bench::run`] for a task with more than a goal (`input`).
    pub fn run_spec(&self, spec: TaskSpec) -> Row {
        let id = self.tasks.spawn(spec, Actor::Operator).expect("spawns");
        loop {
            let row = self.tasks.book().wait(&id, |r| r.state.is_terminal() || matches!(r.state, State::Waiting { .. }), PATIENCE).unwrap_or_else(|| panic!("{id} never ended\n{}", self.dump()));
            match row.state {
                State::Waiting { ask } => {
                    // the machines have their say first; a person is asked of what they could not settle. The answer is in the Log
                    // (`ask.answered`) a moment before the task is running again in the book (`task.resumed`): a row that is still waiting
                    // beside an answer in the Log is an ask that is settled, and the operator's yes would be turned away
                    for _ in 0..4000 {
                        if self.settled(&ask) || self.log.read(0).any(|e| e.kind == "ask.note" && e.data["ask"] == ask.as_str()) {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    let still = self.tasks.book().get(&id).is_some_and(|r| matches!(r.state, State::Waiting { .. }));
                    if self.settled(&ask) {
                        std::thread::sleep(Duration::from_millis(1)); // the book catches up
                    } else if still {
                        match self.tasks.ctl(&ask, Ctl::Answer(Answer::Yes), Actor::Operator) {
                            Err(p) if self.settled(&ask) => drop(p), // settled between the look and the yes: the machines' answer stands
                            r => r.expect("answers"),
                        }
                    }
                }
                _ => return row,
            }
        }
    }

    /// Whether the question has its answer in the Log (someone settled it).
    fn settled(&self, ask: &str) -> bool {
        self.log.read(0).any(|e| e.kind == "ask.answered" && e.data["ask"] == ask)
    }

    pub fn events(&self, kind: &str) -> Vec<Ev> {
        self.log.read(0).filter(|e| e.kind == kind).collect()
    }

    pub fn dump(&self) -> String {
        self.log.read(0).map(|e| format!("{:>3} {:<16} {} {}", e.seq, e.kind, e.task.as_deref().unwrap_or("-"), e.data)).collect::<Vec<_>>().join("\n")
    }

    /// The question the task waits on, once it is the operator's to answer. A question is the task's waiting state from the moment it
    /// is opened, and the machines have their say first, on a thread of their own: a rule or the Gate may settle it a moment later, and
    /// then an answer of the operator's is turned away as too late. So a question is waited for until the chain has left it to a person
    /// (`ask.note`), and one the Log has settled is not the one waited for: a task that was just answered shows the answered question as
    /// its waiting state until it is running again.
    pub fn waiting(&self, id: &str) -> String {
        let end = Instant::now() + PATIENCE;
        loop {
            let left = end.checked_duration_since(Instant::now()).unwrap_or_default();
            let row = self.tasks.book().wait(id, |r| matches!(&r.state, State::Waiting { ask } if !self.settled(ask)), left).unwrap_or_else(|| panic!("never asked\n{}", self.dump()));
            let State::Waiting { ask } = row.state else { unreachable!() };
            loop {
                if self.settled(&ask) {
                    break;
                }
                if self.log.read(0).any(|e| e.kind == "ask.note" && e.data["ask"] == ask.as_str()) {
                    // a note can come before a machine's answer in the same step of the loop: the loop is let finish what it has
                    // before the question is taken to be the operator's
                    self.tasks.sync().expect("the loop answers");
                    if !self.settled(&ask) {
                        return ask;
                    }
                }
                assert!(Instant::now() < end, "the chain never left {ask} to the operator\n{}", self.dump());
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    pub fn answer(&self, ask: &str, a: Answer) {
        self.tasks.ctl(ask, Ctl::Answer(a), Actor::Operator).unwrap_or_else(|p| panic!("answers: {p:?}\n{}", self.dump()));
    }
}
