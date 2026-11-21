//! Programs on the machine that are not on the `PATH`: `/connect` takes a path as well as a name, and the Connect tab's add line lists what
//! is found in the folders programs are put in by `cargo install`, `go install`, `bun`, `pip`/`uv`, `npm`, a build of a repo here, an AppImage or a script.

use std::path::{Path, PathBuf};

/// The folders searched, for a project at `cwd`.
pub fn folders(cwd: &Path) -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let mut dirs: Vec<PathBuf> = [".local/bin", ".cargo/bin", "go/bin", ".bun/bin", "Applications", "bin"].iter().map(|d| home.join(d)).collect();
    dirs.extend(["node_modules/.bin", ".venv/bin", "target/release", "target/debug", "build", "dist", "bin", "out"].iter().map(|d| cwd.join(d)));
    dirs
}

fn runnable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    // a build's folder holds objects and metadata beside its programs
    const NOT: [&str; 8] = [".d", ".so", ".rlib", ".rmeta", ".o", ".a", ".dylib", ".pdb"];
    p.is_file() && !name.starts_with('.') && !NOT.iter().any(|e| name.ends_with(e)) && p.metadata().is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

/// Every program in those folders that can be run, by path, once each (a link and its target are one), sorted; none that is a folder or
/// a hidden file, and at most 2000.
pub fn found(cwd: &Path) -> Vec<PathBuf> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for dir in folders(cwd) {
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        for e in read.flatten() {
            let path = e.path();
            if !runnable(&path) {
                continue;
            }
            let real = path.canonicalize().unwrap_or_else(|_| path.clone());
            if seen.insert(real) {
                out.push(path);
            }
            if out.len() >= 2000 {
                return out;
            }
        }
    }
    out.sort();
    out
}

/// The path of a program by its name, in those folders (a name that is on the `PATH` is found there first, by the caller).
pub fn named(name: &str, cwd: &Path) -> Option<PathBuf> {
    folders(cwd).into_iter().map(|d| d.join(name)).find(|p| runnable(p))
}
