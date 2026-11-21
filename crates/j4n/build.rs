//! Stamps the commit and its date into the binary (`j4n --version`). The date is the commit's, not the clock's, so one commit
//! prints one line. The script watches HEAD, the ref HEAD names and `packed-refs`, never the tree: a commit, a branch switch or a
//! merge reruns it, an edit does not. Outside git (a source archive) it sets nothing.

use std::path::{Path, PathBuf};
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !text.is_empty()).then_some(text)
}

/// A path git prints is relative to the folder the command ran in.
fn watch(path: &str, here: &Path) {
    let p = PathBuf::from(path);
    let p = if p.is_absolute() { p } else { here.join(p) };
    println!("cargo:rerun-if-changed={}", p.display());
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let here = std::env::current_dir().unwrap_or_default();
    let Some(commit) = git(&["rev-parse", "--short=8", "HEAD"]) else { return };
    let Some(date) = git(&["log", "-1", "--format=%cs"]) else { return };
    println!("cargo:rustc-env=J4N_COMMIT={commit}");
    println!("cargo:rustc-env=J4N_DATE={date}");
    if let Some(head) = git(&["rev-parse", "--git-path", "HEAD"]) {
        watch(&head, &here);
    }
    if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"]).and_then(|r| git(&["rev-parse", "--git-path", &r])) {
        watch(&reference, &here);
    }
    if let Some(common) = git(&["rev-parse", "--git-common-dir"]) {
        watch(&format!("{common}/packed-refs"), &here);
    }
}
