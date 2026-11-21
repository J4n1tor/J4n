//! The profile a claude session began with. The CLI records a session's system prompt on its first request and reuses it
//! on every resume, so a session is lean or full for ever: a line that carries on in it is started with the profile it began with, whatever the
//! setting `profile` and the line's role say now. A session begun lean is written down in the place's `.j4n/claude/lean/<session>` when its id
//! is known; one that has no such file is full, as every session was before profiles, so a default run writes nothing.

use j4n_core::state;
use std::fs;
use std::path::{Path, PathBuf};

/// Which of a connector's two profiles a session is started with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    Full,
    Lean,
}

/// The file of a session, none for an id that is not a plain word (it comes from the CLI and from the Log: it never names a path out of the folder).
fn file(place: &Path, session: &str) -> Option<PathBuf> {
    let plain = !session.is_empty() && session.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    plain.then(|| place.join(".j4n/claude/lean").join(session))
}

/// The profile a session was begun with, from what was written when it began; `None` when nothing was.
pub fn recorded(place: &Path, session: &str) -> Option<Which> {
    file(place, session).filter(|f| f.is_file()).map(|_| Which::Lean)
}

/// Write down that a session was begun lean. A place that cannot be written to leaves it unwritten, and the session is then resumed full.
pub fn record(place: &Path, session: &str) {
    let Some(f) = file(place, session) else { return };
    let made = state::ensure_root(&place.join(".j4n"), true).and_then(|()| state::private_dir(f.parent().unwrap_or(place))).and_then(|()| fs::write(&f, b"lean\n"));
    drop(made);
}
