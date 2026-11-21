//! A connector names where its program keeps its login (`login`), and a place that wraps the launch is told: a box binds those
//! paths writable and nothing else of the home.

use crate::bench::{Bench, connectors};
use crate::world::Scratch;
use j4n_connect::claude::Config;
use j4n_core::place::{Launch, Place, Prepare, Settled, Site};
use j4n_core::table::Problem;
use j4n_core::task::{State, TaskSpec};
use std::sync::{Arc, Mutex};

struct Rec(Arc<Mutex<Vec<Launch>>>);

impl Place for Rec {
    fn id(&self) -> &str {
        "rec"
    }

    fn prepare(&self, cx: &Prepare) -> Result<Site, Problem> {
        Ok(Site::new(cx.task, cx.root))
    }

    fn wrap(&self, _site: &Site, launch: Launch) -> Launch {
        self.0.lock().unwrap().push(launch.clone());
        launch
    }

    fn settle(&self, _site: &Site) -> Settled {
        Settled::default()
    }
}

const TEXT: &str = r#"
api = 1
[[connector]]
id = "say"
is = "agent"
dialect = "text"
program = "{bin}/say"
login = ["~/.say", "~/.config/say"]
start = ["{prompt}"]

[[connector]]
id = "quiet"
is = "agent"
dialect = "text"
program = "{bin}/say"
start = ["{prompt}"]
"#;

fn bench(seen: &Arc<Mutex<Vec<Launch>>>) -> Bench {
    let bin = Scratch::new("bin");
    j4n_connect_script(&bin);
    let (list, said, tables) = connectors(&bin, TEXT);
    assert!(said.is_empty(), "{said:?}");
    let (doers, said) = list.doers(&|_, r| Err(format!("no secret `{r}`")));
    assert!(said.is_empty(), "{said:?}");
    let rec = Arc::new(Rec(seen.clone()));
    Bench::tuned(bin, doers, &tables, move |c| c.places.push(rec))
}

fn j4n_connect_script(bin: &Scratch) {
    crate::world::script(bin, "say", "echo ok");
}

fn run(b: &Bench, doer: &str) {
    let mut spec = TaskSpec::new("go", doer);
    spec.place = "rec".into();
    let id = b.tasks.spawn(spec, j4n_core::change::Actor::Operator).unwrap();
    assert_eq!(b.end(&id).state, State::Done { ok: true });
}

#[test]
fn the_launch_a_place_wraps_carries_the_paths_the_connector_names_for_its_login() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let b = bench(&seen);
    run(&b, "say");
    run(&b, "quiet");
    let launches = seen.lock().unwrap().clone();
    assert_eq!(launches.len(), 2);
    assert_eq!(launches[0].login, ["~/.say", "~/.config/say"]);
    assert!(launches[1].login.is_empty(), "a connector that names nothing gets nothing of the home");
}

#[test]
fn claude_and_the_rows_of_the_base_pack_name_their_login() {
    let tables = {
        let layer = j4n_core::table::Layer::new(
            j4n_core::table::LayerKind::Pack,
            "agents",
            vec![j4n_core::table::SourceFile { path: "connectors/agents.toml".into(), text: include_str!("../../../../packs/base/connectors/agents.toml").into() }],
        );
        j4n_core::table::Tables::load(vec![layer]).0
    };
    let (list, said) = j4n_connect::manifest::Connectors::load(&tables);
    assert!(said.is_empty(), "{said:?}");
    let login = |id: &str| list.all().iter().find(|c| c.id == id).map(|c| c.login.clone()).unwrap_or_else(|| panic!("{id}"));
    assert_eq!(login("claude"), ["~/.claude", "~/.claude.json"]);
    assert_eq!(login("pi"), ["~/.pi"]);
    assert_eq!(login("omp"), ["~/.omp"]);
    assert!(login("test").is_empty());
    let claude = list.all().iter().find(|c| c.id == "claude").unwrap();
    assert_eq!(Config::from_connector(claude, Default::default()).login, ["~/.claude", "~/.claude.json"], "and Claude's own launch takes them from its row");
}
