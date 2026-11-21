//! The patterns, the change as hunks, the parts and the questions to Jev, and the score.

mod judge_cases;
mod keep_cases;
mod pattern_cases;
mod scan_cases;
mod score_cases;

use j4n_qc::change::Change;
use j4n_qc::pattern::{Pattern, Ready};
use std::path::{Path, PathBuf};

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The seed patterns of the pack, as shipped.
pub fn seeds() -> Vec<Pattern> {
    let mut all = Vec::new();
    for f in ["tier0", "tier1"] {
        let text = std::fs::read_to_string(root().join(format!("packs/qc/patterns/{f}.toml"))).unwrap();
        let table: toml::Table = text.parse().unwrap();
        for p in table["pattern"].as_array().unwrap() {
            all.push(p.clone().try_into::<Pattern>().unwrap_or_else(|e| panic!("{f}: {e}")));
        }
    }
    all
}

pub fn ready(id: &str) -> Ready {
    Ready::new(seeds().into_iter().find(|p| p.id == id).unwrap_or_else(|| panic!("no seed {id}"))).unwrap()
}

/// A folder for a test, empty.
pub fn dir(tag: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!("j4n-qc-{tag}-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A change of one file taken as it stands.
pub fn whole(path: &str, text: &str) -> Change {
    let d = dir(&format!("whole-{}", path.replace('/', "_")));
    let full = d.join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(&full, text).unwrap();
    Change::of_path(&d, path).unwrap()
}

/// The surfaces of the pack, as shipped.
pub fn surfaces() -> Vec<j4n_qc::keep::Rule> {
    let text = std::fs::read_to_string(root().join("packs/qc/keeps/keep.toml")).unwrap();
    let table: toml::Table = text.parse().unwrap();
    table["qc_keep"].as_array().unwrap().iter().map(|p| j4n_qc::keep::Rule::new(p.clone().try_into().unwrap_or_else(|e| panic!("keep: {e}"))).unwrap()).collect()
}
