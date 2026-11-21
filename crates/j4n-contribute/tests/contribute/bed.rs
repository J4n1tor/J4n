//! A Log, the folds and settings of the pack `contribute`, and the Table with a pack layer and the operator's, in a folder of the test's own.

use j4n_contribute::Finder;
use j4n_core::clock::ManualClock;
use j4n_core::fold::{Folds, declared};
use j4n_core::log::{Log, Opts, Scope};
use j4n_core::proposal::{Plant, Proposals, Proposer};
use j4n_core::changes::Changes;
use j4n_core::redact::Redactor;
use j4n_core::setting::Settings;
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use j4n_core::write::Writer;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct Dir(pub PathBuf);

impl Dir {
    pub fn new(name: &str) -> Dir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!("j4n-contribute-{}-{}-{name}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn file(path: &str, text: &str) -> SourceFile {
    SourceFile { path: path.into(), text: text.into() }
}

/// The pack's own files that the finder reads, and `rows` as a second pack file (what "the pack" says of the rows a fix is about).
pub fn pack(rows: &str) -> Layer {
    Layer::new(
        LayerKind::Pack,
        "contribute",
        vec![
            file("folds/contribute.toml", include_str!("../../../../packs/contribute/folds/contribute.toml")),
            file("settings/contribute.toml", include_str!("../../../../packs/contribute/settings/contribute.toml")),
            file("settings/rows.toml", &format!("api = 1\n{rows}")),
        ],
    )
}

pub struct Bed {
    pub dir: Dir,
    pub clock: Arc<ManualClock>,
    pub log: Log,
    pub folds: Folds,
    pub proposals: Proposals,
    pub changes: Changes,
}

pub fn bed(name: &str) -> Bed {
    let dir = Dir::new(name);
    let clock = Arc::new(ManualClock::at(1_000_000_000));
    let (log, problems) = Log::open(&dir.0.join(".j4n"), Scope::Project, Opts { segment_bytes: 8 << 20, max_line: 1 << 20, checkpoint_every: 1000, clock: clock.clone() }, Redactor::new()).unwrap();
    assert!(problems.is_empty());
    let folds = Folds::new(&dir.0.join("folds"));
    let (tables, problems) = Tables::load(vec![pack("")]);
    assert!(problems.is_empty(), "{problems:?}");
    let (declared, problems) = declared(&tables, Scope::Project);
    assert!(problems.is_empty(), "{problems:?}");
    for f in declared {
        assert!(folds.add(f, 0).is_empty());
    }
    let (proposals, changes) = (Proposals::new(), Changes::new());
    assert!(folds.add(Box::new(changes.clone()), 0).is_empty() && folds.add(Box::new(proposals.clone()), 0).is_empty());
    folds.attach(&log).unwrap();
    Bed { dir, clock, log, folds, proposals, changes }
}

/// The Table as it is loaded at a start: the pack's `rows` under the operator's folder.
pub fn tables(rows: &str, home: &Path, mode: Option<&str>) -> (Tables, Settings) {
    let mut user = Layer::from_dir(LayerKind::User, "home", home).0;
    if let Some(m) = mode {
        user.files.push(file("settings/mode.toml", &format!("api = 1\n[[setting]]\nid = \"contribute.mode\"\nvalue = \"{m}\"\n")));
    }
    let (tables, problems) = Tables::load(vec![pack(rows), user]);
    assert!(problems.is_empty(), "{problems:?}");
    let (settings, problems) = Settings::load(&tables);
    assert!(problems.is_empty(), "{problems:?}");
    (tables, settings)
}

impl Bed {
    pub fn finder(&self, rows: &str, mode: Option<&str>) -> Arc<Finder> {
        let home = self.dir.0.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let (tables, settings) = tables(rows, &home, mode);
        Arc::new(Finder::new(&self.log, &self.folds, &settings, &tables, &self.proposals))
    }

    /// A proposer whose plant writes the operator's folder `home` over the pack's `rows`.
    pub fn proposer(&self, rows: &str) -> Proposer {
        let home = self.dir.0.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let (dir, rows) = (home.clone(), rows.to_string());
        let plant = Plant { load: Box::new(move || tables(&rows, &dir, None).0), writer: Writer::new(Some(home), None) };
        Proposer::new(self.log.clone(), self.proposals.clone(), self.changes.clone(), Arc::new(plant), Arc::new(|| 5_000))
    }

    pub fn events(&self, kind: &str) -> Vec<Value> {
        self.log.sync();
        self.log.read(0).filter(|e| e.kind == kind).map(|e| e.data).collect()
    }

    pub fn at(&self, ms: u64) {
        self.clock.set(ms);
    }
}
