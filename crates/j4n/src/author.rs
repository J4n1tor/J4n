//! `j4n pack new | check | install` (`docs/PACKS-GUIDE.md`): making a pack, validating it and installing it, for the people who
//! write packs. The checking is `j4n_core::pack::check`; this is the folder, the git clone and the words. No world is opened.

use crate::cli::PackCmd;
use crate::pack::catalog;
use j4n_core::pack::check::{Report, check, template};
use j4n_core::pack::{Manifest, packs_dir};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The status of `j4n pack …`: 0 it went as asked, 1 it did not.
pub fn run(cmd: &PackCmd) -> i32 {
    let done = match cmd {
        PackCmd::New { name, dir } => make(name, dir.as_deref()),
        PackCmd::Check { path } => verify(Path::new(path)),
        PackCmd::Install { from } => install(from),
    };
    match done {
        Ok(()) => 0,
        Err(why) => {
            if !why.is_empty() {
                eprintln!("j4n: {why}");
            }
            1
        }
    }
}

fn make(name: &str, dir: Option<&str>) -> Result<(), String> {
    let files = template(name)?;
    let dir = PathBuf::from(dir.unwrap_or(name));
    if std::fs::read_dir(&dir).is_ok_and(|mut d| d.next().is_some()) || dir.is_file() {
        return Err(format!("{} is there already and is not empty: pick another name or folder", dir.display()));
    }
    for (rel, text) in &files {
        let at = dir.join(rel);
        std::fs::create_dir_all(at.parent().unwrap_or(&dir)).and_then(|()| std::fs::write(&at, text)).map_err(|e| format!("cannot write {}: {e}", at.display()))?;
    }
    println!("pack {name}: {} files in {}\nnext: `j4n pack check {}`, then `j4n pack install {}`", files.len(), dir.display(), dir.display(), dir.display());
    Ok(())
}

/// The packs there are: the ones built in and the ones installed, which a pack under test may need.
fn known() -> (j4n_core::pack::Catalog, Option<PathBuf>) {
    let home = crate::wire::Setup::from_env().ok().map(|s| packs_dir(&s.home));
    (home.as_ref().map_or_else(catalog, |h| catalog().with_installed(h).0), home)
}

fn show(r: &Report) {
    for f in &r.found {
        eprintln!("{f}");
    }
    println!("{}", r.summary());
}

fn verify(dir: &Path) -> Result<(), String> {
    let report = check(dir, &known().0, std::env::var_os("J4N_BLESS").is_some_and(|v| !v.is_empty()));
    show(&report);
    if report.ok() { Ok(()) } else { Err(String::new()) }
}

fn install(from: &str) -> Result<(), String> {
    let home = crate::wire::Setup::from_env().map_err(|e| format!("no folder to install into: {e}"))?.home;
    install_at(from, &packs_dir(&home)).map(|_| ())
}

/// Check the pack at `from` (a folder, or a git URL cloned for this) and, if it passes, copy it into `into` (`~/.j4n/packs`). A pack that
/// fails is not installed.
pub fn install_at(from: &str, into: &Path) -> Result<Manifest, String> {
    let (mut catalog, _) = catalog().with_installed(into);
    let fetched = Fetched::of(from)?;
    let report = check(&fetched.dir, &catalog, false);
    show(&report);
    if !report.ok() {
        return Err(format!("{from} is not installed: it fails `j4n pack check`"));
    }
    let m = catalog.install(&fetched.dir, into)?;
    println!("installed pack {} {} in {}\nit is off: turn it on with `:pack on {}`", m.id, m.version, into.join(&m.id).display(), m.id);
    Ok(m)
}

/// A pack folder to read: the folder named, or a shallow clone in a temporary folder that goes when this does.
struct Fetched {
    dir: PathBuf,
    temp: Option<PathBuf>,
}

impl Drop for Fetched {
    fn drop(&mut self) {
        if let Some(t) = &self.temp {
            let _ = std::fs::remove_dir_all(t);
        }
    }
}

/// Whether `from` is a place git can clone (a URL, or `user@host:path`) and not a folder.
fn is_git(from: &str) -> bool {
    from.contains("://") || (from.contains('@') && from.contains(':')) || from.ends_with(".git")
}

impl Fetched {
    fn of(from: &str) -> Result<Fetched, String> {
        if Path::new(from).is_dir() {
            return Ok(Fetched { dir: PathBuf::from(from), temp: None });
        }
        if from.starts_with('-') || !is_git(from) {
            return Err(format!("{from} is not a folder, and does not look like a git URL"));
        }
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let temp = std::env::temp_dir().join(format!("j4n-pack-{}-{ms}", std::process::id()));
        let got = Fetched { dir: temp.join("pack"), temp: Some(temp.clone()) };
        std::fs::create_dir_all(&temp).map_err(|e| format!("cannot make {}: {e}", temp.display()))?;
        eprintln!("j4n: cloning {from} (shallow) …");
        let out = j4n_core::host::keyless(&mut Command::new("git"))
            .args(["clone", "--depth", "1", "--quiet", "--no-tags", "--", from])
            .arg(&got.dir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_ALLOW_PROTOCOL", "file:https:ssh:git")
            .output()
            .map_err(|e| format!("cannot run git: {e}"))?;
        if !out.status.success() {
            return Err(format!("git could not clone {from}: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        Ok(got)
    }
}
