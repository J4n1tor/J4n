//! A provider's key kept in J4n's own key file: one file per connector, `<home>/keys/<connector>`, mode 0600 in a
//! folder of mode 0700, written to a file beside it and renamed over it, so a reader finds the old key or the new one and never half of
//! one. It is not an operator edit: no Change, no undo, no Grow row. The Log gets `key.set` or `key.removed` with the connector's id and
//! never the value, and the redactor is told of the value when it is typed and, from the file, at every start ([`Keys::register_all`]).
//! A connector reads it at each child start ([`state`] says which place wins: `env:` when it is set, then this file, then the row's own `file:`).

use crate::redact::{MIN_SECRET, Redactor};
use std::path::{Path, PathBuf};

/// The Log's word for a key typed in Library › Providers (data: `connector`, nothing else).
pub const SET: &str = "key.set";
/// The Log's word for a key that J4n kept and the operator removed (`x` on the row, through the card).
pub const REMOVED: &str = "key.removed";
/// The folder under J4n's own, where each connector's key is one file.
pub const DIR: &str = "keys";

/// Where J4n keeps the keys typed in Library › Providers.
#[derive(Debug, Clone)]
pub struct Keys {
    dir: PathBuf,
}

/// The name the redactor's marker gives a connector's key: the one `Connector::api_key` registers it under (`OWN_KEY`).
pub fn marker(connector: &str) -> String {
    format!("{}_KEY", connector.to_uppercase().replace('-', "_"))
}

/// `(provider id, key reference)` of every `provider` row of the Table that takes a key: what a connector with the same reference reads after its own file.
pub fn provider_refs(tables: &crate::table::Tables) -> Vec<(String, String)> {
    let word = |m: &std::collections::BTreeMap<String, crate::value::Value>, k: &str| match m.get(k) {
        Some(crate::value::Value::Str(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    };
    tables.rows("provider").into_iter().filter_map(|r| if let crate::value::Value::Map(m) = r { word(&m, "id").zip(word(&m, "key")) } else { None }).collect()
}

/// A connector id that can be a file's name: letters, digits, `-`, `_` and `.`, not starting with a dot.
pub fn fits(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && !id.starts_with('.') && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

impl Keys {
    /// The keys of the J4n folder `home`.
    pub fn new(home: &Path) -> Keys {
        Keys { dir: home.join(DIR) }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The file of a connector's key; none for an id that cannot name a file.
    pub fn path(&self, connector: &str) -> Option<PathBuf> {
        fits(connector).then(|| self.dir.join(connector))
    }

    /// Whether J4n keeps a key for the connector.
    pub fn has(&self, connector: &str) -> bool {
        self.path(connector).is_some_and(|p| p.is_file())
    }

    /// Keep `value` as the connector's key: the redactor is told of it first (a value it refuses, too short to tell from text, is not kept),
    /// then it is written to a file of mode 0600 beside the key's and renamed over it. The old key, if any, is replaced.
    pub fn set(&self, red: &Redactor, connector: &str, value: &str) -> Result<(), String> {
        let path = self.path(connector).ok_or_else(|| format!("`{connector}` cannot name a key"))?;
        let value = value.trim();
        if value.is_empty() {
            return Err("nothing was typed".into());
        }
        if value.chars().any(char::is_control) {
            return Err("a key is one line".into());
        }
        if !red.register(&marker(connector), value) {
            return Err(format!("a key is at least {MIN_SECRET} characters"));
        }
        write_atomic(&self.dir, &path, value)
    }

    /// Forget the connector's key: its file is removed. False when there was none.
    pub fn remove(&self, connector: &str) -> Result<bool, String> {
        let Some(path) = self.path(connector).filter(|p| p.is_file()) else { return Ok(false) };
        std::fs::remove_file(&path).map_err(|e| format!("the key of {connector} could not be removed: {e}"))?;
        Ok(true)
    }

    /// Tell the redactor of every key kept here, at the start before the Log is opened; what could not be read, said as a notice.
    pub fn register_all(&self, red: &Redactor) -> Vec<String> {
        let Ok(dir) = std::fs::read_dir(&self.dir) else { return Vec::new() };
        let mut found = Vec::new();
        for entry in dir.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !fits(&name) || !entry.path().is_file() {
                continue;
            }
            match std::fs::read_to_string(entry.path()) {
                Ok(v) if red.register(&marker(&name), v.trim()) => {}
                Ok(_) => found.push(format!("the key J4n keeps for {name} is too short to be kept out of the Log; Library › Providers › {name}: ⏎ types it again")),
                Err(e) => found.push(format!("the key J4n keeps for {name} cannot be read ({e}); Library › Providers › {name}: ⏎ types it again")),
            }
        }
        found
    }
}

/// Where a connector's key comes from now, in the words of its row (never the value or any part of it): `set · env` when its `env:`
/// name is set (it wins), `set · J4n file` when J4n keeps one, `set · file` when the row's own `file:` is there, else `not set`. A place that
/// holds a key the connector refuses (a file others can read, a value too short to keep out of the Log) says so, `not used · …`, when no
/// place after it gives one: the same rules as the reader of the run.
pub fn state(kept: Option<&Keys>, connector: &str, reference: &str) -> &'static str {
    use std::os::unix::fs::PermissionsExt;
    const SHORT: &str = "not used · too short";
    let mut refused = None;
    if let Some(v) = reference.strip_prefix("env:").and_then(|name| std::env::var(name).ok()).filter(|v| !v.trim().is_empty()) {
        if v.trim().chars().count() >= MIN_SECRET {
            return "set · env";
        }
        refused = Some(SHORT);
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let row = reference.strip_prefix("file:").map(|p| p.strip_prefix("~/").map_or_else(|| PathBuf::from(p), |rest| Path::new(&home).join(rest)));
    for (path, word) in [(kept.and_then(|k| k.path(connector)), "set · J4n file"), (row, "set · file")] {
        let Some((path, text)) = path.and_then(|p| std::fs::read_to_string(&p).ok().map(|t| (p, t))).filter(|(_, t)| !t.trim().is_empty()) else { continue };
        let wide = std::fs::metadata(&path).is_ok_and(|m| m.permissions().mode() & 0o077 != 0);
        match (wide, text.trim().chars().count() >= MIN_SECRET) {
            (false, true) => return word,
            (true, _) => refused = refused.or(Some("not used · others can read the file")),
            (false, false) => refused = refused.or(Some(SHORT)),
        }
    }
    refused.unwrap_or("not set")
}

/// `value` into `path` by a file of mode 0600 in the same folder and a rename: a reader finds the old file or the new one.
fn write_atomic(dir: &Path, path: &Path, value: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
    let e = |what: &Path, e: std::io::Error| format!("{}: {e}", what.display());
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir).map_err(|x| e(dir, x))?;
    // a folder made before with a wider mode is narrowed: the keys are the operator's alone
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(|x| e(dir, x))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let temp = dir.join(format!(".{name}.{}.new", std::process::id()));
    let _ = std::fs::remove_file(&temp);
    let wrote = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temp).and_then(|mut f| {
        f.write_all(value.as_bytes())?;
        f.sync_all()
    });
    if let Err(err) = wrote.and_then(|()| std::fs::rename(&temp, path)) {
        let _ = std::fs::remove_file(&temp);
        return Err(e(path, err));
    }
    Ok(())
}
