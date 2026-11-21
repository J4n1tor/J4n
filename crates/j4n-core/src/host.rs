//! The hosts that bind a child to themselves by environment variables (`packs/base/hosts`, kind `host`): a program that runs agents
//! (the Claude desktop app, codex, herdr) gives each child a socket, a token and a session id, and J4n run inside one would hand them on to
//! every doer, shell line and tool it starts. [`Scrub`] is the names the rows say to take out; what J4n sets itself is put in after it.
//! The operator's own key variables go too ([`key_like`]): a child is given one only where its connector names it.

use crate::table::{Kind, Tables};
use serde::Deserialize;

/// An entry of kind `host`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostRow {
    pub id: String,
    pub doc: Option<String>,
    #[serde(default)]
    pub drop: Vec<String>,
    #[serde(default)]
    pub drop_prefix: Vec<String>,
    pub source: String,
}

impl Kind for HostRow {
    const NAME: &'static str = "host";
}

/// A variable named like a key (`*_KEY`, `*_TOKEN`, `*_SECRET`, `*_PASSWORD`, `*_PASSWD`): its value is kept out of every output, and a
/// child J4n starts inherits none of them; the one whose connector names it is given it back.
pub fn key_like(name: &str) -> bool {
    ["_KEY", "_TOKEN", "_SECRET", "_PASSWORD", "_PASSWD"].iter().any(|s| name.ends_with(s))
}

/// Take the key variables of this process's environment out of what `cmd` inherits, and nothing else: for a program that runs no agent
/// (git, a box's probe), whose own variables (`GIT_*`, `SSH_AUTH_SOCK`) end in none of a key's endings and stay. A project's git hooks run there.
pub fn keyless(cmd: &mut std::process::Command) -> &mut std::process::Command {
    for name in std::env::vars_os().filter_map(|(k, _)| k.into_string().ok()).filter(|k| key_like(k)) {
        cmd.env_remove(name);
    }
    cmd
}

/// The base pack's rows, for a process that has not loaded the Table (a test, the lab): the same file the pack brings.
const BASE: &str = include_str!("../../../packs/base/hosts/hosts.toml");

/// The names and prefixes to take out of a child's environment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scrub {
    names: Vec<String>,
    prefixes: Vec<String>,
}

impl Scrub {
    pub fn from_rows(rows: &[HostRow]) -> Scrub {
        let mut s = Scrub::default();
        for r in rows {
            s.names.extend(r.drop.iter().filter(|n| !n.is_empty()).cloned());
            // a prefix of nothing would take the whole environment
            s.prefixes.extend(r.drop_prefix.iter().filter(|p| p.len() >= 3).cloned());
        }
        s
    }

    /// What the loaded Table says (the operator's own rows included); the base pack's when the Table has none.
    pub fn from_tables(t: &Tables) -> Scrub {
        match t.list::<HostRow>() {
            Ok(rows) if !rows.is_empty() => Scrub::from_rows(&rows),
            _ => Scrub::base(),
        }
    }

    /// The base pack's rows.
    pub fn base() -> Scrub {
        #[derive(Deserialize)]
        struct File {
            host: Vec<HostRow>,
        }
        toml::from_str::<File>(BASE).map(|f| Scrub::from_rows(&f.host)).unwrap_or_default()
    }

    /// Whether a variable of this name is a host's, taken out of every child.
    pub fn drops(&self, name: &str) -> bool {
        self.names.iter().any(|n| n == name) || self.prefixes.iter().any(|p| name.starts_with(p.as_str()))
    }

    /// The key variables of this process that are no host's: what a J4n that J4n starts (its relaunch, the lab's) is given back, as it reads
    /// its keys there.
    pub fn keys(&self) -> Vec<(String, String)> {
        self.keys_of(std::env::vars())
    }

    /// [`Scrub::keys`] of these variables.
    pub fn keys_of(&self, vars: impl IntoIterator<Item = (String, String)>) -> Vec<(String, String)> {
        vars.into_iter().filter(|(k, _)| key_like(k) && !self.drops(k)).collect()
    }

    /// Take out of what `cmd` inherits every variable of this process's environment that is a host's or a key; what is set on `cmd`
    /// after stays.
    pub fn strip(&self, cmd: &mut std::process::Command) {
        for name in std::env::vars_os().filter_map(|(k, _)| k.into_string().ok()).filter(|k| self.drops(k) || key_like(k)) {
            cmd.env_remove(name);
        }
    }
}
