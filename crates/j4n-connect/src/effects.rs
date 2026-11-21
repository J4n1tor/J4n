//! Where an effect lands, for the Gate: whether a path is inside the folder a task works in, and which host a URL names. What
//! the dialects that put a tool's effect to the Gate (`claude-stream`, `acp`, `pi-rpc`) share.

use std::path::{Component, Path, PathBuf};

/// Whether `path` (as the tool was given it) lies inside `dir`, looking through links at the longest part that exists.
pub fn inside(dir: &Path, path: &str) -> bool {
    let p = Path::new(path);
    let mut full = PathBuf::new();
    for c in (if p.is_absolute() { p.to_path_buf() } else { dir.join(p) }).components() {
        match c {
            Component::ParentDir => {
                full.pop();
            }
            Component::CurDir => {}
            c => full.push(c.as_os_str()),
        }
    }
    let mut known = full.clone();
    let mut rest = Vec::new();
    while !known.exists() {
        match known.file_name().map(|n| n.to_owned()) {
            Some(n) => {
                rest.push(n);
                known.pop();
            }
            None => break,
        }
    }
    let mut real = known.canonicalize().unwrap_or(known);
    real.extend(rest.into_iter().rev());
    real.starts_with(dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf()))
}

/// The host a URL names (no scheme: the text up to the first slash).
pub fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or(authority);
    host.split(':').next().unwrap_or(host).to_lowercase()
}
