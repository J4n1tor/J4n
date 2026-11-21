//! What the Table tests share: scratch directories, layers built from text, and the lighthouse pack — a pack of an
//! imaginary coast that has nothing to do with J4n's own work (law 10: no domain in the core).

use j4n_core::table::{Layer, LayerKind, Level, Problem, SourceFile, Tables};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A directory under the run's temporary directory, removed when dropped.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new(tag: &str) -> Scratch {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "j4n-core-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn join(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }

    pub fn put(&self, rel: &str, text: &str) {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    pub fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.0.join(rel)).unwrap()
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.0.join(rel).exists()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn layer(kind: LayerKind, name: &str, files: &[(&str, &str)]) -> Layer {
    Layer::new(
        kind,
        name,
        files.iter().map(|(path, text)| SourceFile { path: path.to_string(), text: text.to_string() }).collect(),
    )
}

/// A pack with one file.
pub fn pack(name: &str, text: &str) -> Layer {
    layer(LayerKind::Pack, name, &[("kinds.toml", text)])
}

pub fn home(files: &[(&str, &str)]) -> Layer {
    layer(LayerKind::User, "home", files)
}

pub fn project(files: &[(&str, &str)]) -> Layer {
    layer(LayerKind::Project, "project", files)
}

/// The Tables of a pack and then more layers, with a trusted project.
pub fn load(layers: Vec<Layer>) -> (Tables, Vec<Problem>) {
    Tables::load(layers)
}

pub fn refused(problems: &[Problem]) -> Vec<String> {
    problems.iter().filter(|p| p.level == Level::Refused).map(ToString::to_string).collect()
}

pub fn warned(problems: &[Problem]) -> Vec<String> {
    problems.iter().filter(|p| p.level == Level::Warn).map(ToString::to_string).collect()
}

/// Some problem line holds every one of `needles`.
pub fn mentions(lines: &[String], needles: &[&str]) -> bool {
    lines.iter().any(|l| needles.iter().all(|n| l.contains(n)))
}

/// The lighthouse pack, read from its directory.
pub fn lighthouse() -> (Layer, Vec<Problem>) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/core/lighthouse");
    Layer::from_dir(LayerKind::Pack, "lighthouse", &dir)
}

/// A small kind, `thing`, declared from the fields given (a TOML table body).
pub fn kind_of(name: &str, fields: &str) -> Layer {
    pack("p", &format!("api = 1\n[[kind]]\nid = \"{name}\"\ndoc = \"a test kind\"\n[kind.fields]\n{fields}\n"))
}

// ---- the Log ----

use j4n_core::change::Actor;
use j4n_core::clock::ManualClock;
use j4n_core::log::{Ev, Log, New, Opts, Scope};
use j4n_core::redact::Redactor;
use serde_json::Value as Json;
use std::sync::Arc;

/// Opts with a clock the test moves and small numbers, so rotation and checkpoints happen within a few events.
pub fn opts(clock: &Arc<ManualClock>) -> Opts {
    Opts { segment_bytes: 8 << 20, max_line: 1 << 20, checkpoint_every: 1000, clock: clock.clone() }
}

/// The project log under `<scratch>/.j4n`, with its clock at 1,000 ms.
pub fn open_log(s: &Scratch) -> (Log, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::at(1_000));
    let (log, problems) = Log::open(&s.join(".j4n"), Scope::Project, opts(&clock), Redactor::new()).expect("opens");
    assert!(problems.is_empty(), "{problems:?}");
    (log, clock)
}

pub fn ev(kind: &str, data: Json) -> New {
    New::of(kind, Actor::Core, data)
}

/// Every event in the log directory, read from the files.
pub fn all(log: &Log) -> Vec<Ev> {
    log.read(0).collect()
}
