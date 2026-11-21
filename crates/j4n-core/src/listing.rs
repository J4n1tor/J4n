//! The model lists J4n keeps for the library: what a doer's own list said (`pi --list-models`) and what OpenRouter's public
//! `/models` said, each with the time it was read, in one file, `<home>/cache/models.json`. Whatever reads them (`j4n_connect::modellist::Listing`)
//! asks at most once a day and only when it may; the library reads this file and runs nothing, so a page opened is never a program started.
//! It is a cache, not state of the work: not a fold, and a replay does not depend on it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A list is read again when it is this old.
pub const DAY_MS: u64 = 24 * 3600 * 1000;
/// The source name of OpenRouter's public list; a doer's list is under its connector's id.
pub const ROUTER: &str = "provider:openrouter";
/// The most models one list keeps.
const MOST: usize = 5000;

/// Whether `w`, the first word of a line a list command printed, is no model: a path (`/…`, `~/…`, `./…`), a file of words
/// (a name ending `.md`, `.txt`, `.html`), or a word that ends a clause (`Warning:`, `available.`). `pi --list-models` with no provider signed in prints prose.
pub fn prose(w: &str) -> bool {
    w.starts_with('/') || w.starts_with("~/") || w.starts_with("./") || w.ends_with([':', '.', ',']) || [".md", ".txt", ".html"].iter().any(|e| w.ends_with(e))
}

/// One list as it was read.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Listed {
    pub at_ms: u64,
    pub models: Vec<String>,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct File {
    #[serde(default)]
    lists: BTreeMap<String, Listed>,
}

/// Where the lists are kept.
#[derive(Debug, Clone)]
pub struct Cache {
    path: PathBuf,
}

impl Cache {
    /// The cache of the J4n folder `home`.
    pub fn new(home: &Path) -> Cache {
        Cache { path: home.join("cache/models.json") }
    }

    /// Every list kept; none for a file that is not there or cannot be read (it is only a cache).
    pub fn read(&self) -> BTreeMap<String, Listed> {
        let mut lists = std::fs::read_to_string(&self.path).ok().and_then(|t| serde_json::from_str::<File>(&t).ok()).map(|f| f.lists).unwrap_or_default();
        // a list an older J4n kept from a command's prose (`No`, the path of a doc) is a bad read: it is left out, and read again when due
        lists.retain(|_, l| !l.models.iter().any(|m| prose(m)));
        lists
    }

    /// Whether `source` has not been read in the last day.
    pub fn due(&self, source: &str, now_ms: u64) -> bool {
        self.read().get(source).is_none_or(|l| now_ms.saturating_sub(l.at_ms) >= DAY_MS)
    }

    /// Keep `models` as what `source` said at `at_ms`, beside the lists already kept; written to a file beside the cache and renamed over it.
    pub fn put(&self, source: &str, at_ms: u64, models: &[String]) -> Result<(), String> {
        let mut file = File { lists: self.read() };
        let mut seen = std::collections::BTreeSet::new();
        let models = models.iter().map(|m| m.trim()).filter(|m| !m.is_empty() && seen.insert(*m)).take(MOST).map(String::from).collect();
        file.lists.insert(source.to_string(), Listed { at_ms, models });
        let e = |what: &Path, e: std::io::Error| format!("{}: {e}", what.display());
        let dir = self.path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir).map_err(|x| e(dir, x))?;
        let temp = self.path.with_extension(format!("{}.new", std::process::id()));
        let text = serde_json::to_string(&file).map_err(|x| x.to_string())?;
        std::fs::write(&temp, text).and_then(|()| std::fs::rename(&temp, &self.path)).map_err(|x| {
            let _ = std::fs::remove_file(&temp);
            e(&self.path, x)
        })
    }
}
