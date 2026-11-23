//! The checkpoint of place `here`: a mutating task works in the operator's own folder, so before it starts J4n
//! notes what the folder holds, and after it settles what the task changed. That is the project's own files, tracked, untracked
//! and ignored alike (an operator's `.env` is lost like any file); not the nested repositories and worktrees (another branch's
//! checkout in an agent's worktrees folder can be most of a checkpoint), nor build output and dependencies. `rewind` puts
//! the folder back.
//!
//! Where it lives: `<home>/checkpoints/<project key>/<task>/`, outside the project, so a task in a box cannot reach it.
//!
//! * `manifest.json`  the folder as it was: every file's size and mtime, the git commit that holds the tracked files
//!   (`git stash create`, kept alive under `refs/j4n/checkpoints/<task>`), and which files were copied
//! * `files/<path>`   a copy of each untracked or ignored file within the caps (every file, outside a git repository)
//! * `footprint.json` what the task changed: created, modified, deleted, and what cannot be put back
//!
//! The folder's git state is never touched: `git stash create` writes objects and nothing else, and a restore goes through
//! `git restore --source=<commit> --worktree`, which leaves the index alone. A file counts as changed when its size or its
//! modification time did; a rewrite of the same size within the file system's time grain is not seen.

use crate::state::project_key;
use crate::task::id_number;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

/// The checkpoints this process holds open: a prune never drops one, however old, as its task writes its footprint there.
static OPEN: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

fn open_set() -> std::sync::MutexGuard<'static, BTreeSet<PathBuf>> {
    OPEN.lock().unwrap_or_else(|e| e.into_inner())
}

/// A folder with more files than this is not checkpointed (a home directory, a monorepo with its dependencies checked in):
/// the walk would take longer than the task, and a copy of it could not be kept.
pub const MAX_FILES: usize = 100_000;

/// Folders no checkpoint looks into: build output, dependencies, caches, git's own and J4n's own.
pub const SKIP: &[&str] = &["target", "node_modules", ".venv", "__pycache__", ".j4n", ".git"];

/// More build and cache folders, left out where git does not say what is the project's (a folder that is not a repository).
/// In a repository a tracked `build/` is the project's, so there only [`SKIP`] applies.
pub const BUILD: &[&str] = &["dist", "build", ".next", ".nuxt", ".gradle", ".tox", ".mypy_cache", ".pytest_cache", ".ruff_cache", ".cache"];

/// The caps on what a checkpoint copies.
#[derive(Debug, Clone)]
pub struct Cfg {
    /// A file over this many bytes is noted but not copied (it cannot be put back).
    pub max_file_bytes: u64,
    /// The most bytes one checkpoint copies in all.
    pub max_total_bytes: u64,
    /// Folder names left out, at any depth.
    pub skip: Vec<String>,
    /// How many checkpoints a project keeps.
    pub keep: usize,
    /// The most bytes a project's checkpoints hold in all: past it the oldest settled ones go.
    pub max_store_bytes: u64,
}

impl Default for Cfg {
    fn default() -> Cfg {
        Cfg {
            max_file_bytes: 1024 * 1024,
            max_total_bytes: 200 * 1024 * 1024,
            skip: SKIP.iter().map(|s| s.to_string()).collect(),
            keep: 20,
            max_store_bytes: 500 * 1024 * 1024,
        }
    }
}

/// What the folder holds at one moment for one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stat {
    pub size: u64,
    pub mtime_ns: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    task: String,
    /// The commit that holds the tracked files as they were (a stash commit when they were dirty, else HEAD).
    git_ref: Option<String>,
    before: BTreeMap<String, Stat>,
    /// Tracked files that `git_ref` restores (empty without a repository).
    tracked: BTreeSet<String>,
    /// Files whose content was copied to `files/`.
    copied: BTreeSet<String>,
    skip: Vec<String>,
    /// What was left out and why (nested repositories, build folders, files over a cap), in words.
    #[serde(default)]
    left_out: Vec<String>,
    /// Paths looked at though git ignores them: the ignored files an apply is about to write (see [`begin_with`]).
    #[serde(default)]
    also: Vec<String>,
}

/// What a task changed in the folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Footprint {
    pub created: Vec<String>,
    pub modified: Vec<String>,
    pub deleted: Vec<String>,
    /// Changed or deleted files that cannot be put back: not tracked, and over a cap (or a link).
    pub not_restorable: Vec<String>,
    /// How each created or modified file stood when the task settled; `rewind` refuses a file someone changed since.
    pub after: BTreeMap<String, Stat>,
    /// What the checkpoint left out, and why (nested repositories, build folders, files over a cap).
    #[serde(default)]
    pub left_out: Vec<String>,
}

impl Footprint {
    pub fn is_empty(&self) -> bool {
        self.created.is_empty() && self.modified.is_empty() && self.deleted.is_empty()
    }
}

/// An open checkpoint: made by [`begin`], closed by [`Checkpoint::end`].
pub struct Checkpoint {
    dir: PathBuf,
    root: PathBuf,
    /// J4n's own home, which no task's footprint counts.
    home: PathBuf,
    manifest: Manifest,
    _held: Held,
}

/// `<home>/checkpoints/<project key>/`
pub fn store(home: &Path, root: &Path) -> PathBuf {
    home.join("checkpoints").join(project_key(root))
}

fn git_raw(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let out = crate::host::keyless(&mut Command::new("git"))
        .arg("-C")
        .arg(root)
        .args(["-c", "user.name=j4n", "-c", "user.email=j4n@localhost", "-c", "commit.gpgsign=false"])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

fn git_out(root: &Path, args: &[&str]) -> Option<String> {
    git_raw(root, args).map(|b| String::from_utf8_lossy(&b).trim().to_string())
}

/// Whether the folder stands in a git repository that has a commit: what a worktree is cut from. Read from the files alone, no program is run (a page asks it
/// as it is drawn): the nearest `.git` up the tree, its `HEAD`, and the branch it names among the refs. A `.git` file (a worktree, a submodule) counts as one.
pub fn has_commit(root: &Path) -> bool {
    let start = if root.as_os_str().is_empty() { std::env::current_dir().unwrap_or_default() } else { root.to_path_buf() };
    let Some(git) = start.ancestors().map(|d| d.join(".git")).find(|g| g.exists()) else { return false };
    if git.is_file() {
        return true;
    }
    let head = fs::read_to_string(git.join("HEAD")).unwrap_or_default();
    match head.trim().strip_prefix("ref: ") {
        None => !head.trim().is_empty(),
        Some(name) => git.join(name).is_file() || fs::read_to_string(git.join("packed-refs")).is_ok_and(|p| p.lines().any(|l| l.ends_with(&format!(" {name}")))),
    }
}

/// A first commit, empty, in a folder that has none (`git init` first where it is no repository): what the needs card's answer does. It adds no file, so a
/// worktree cut from it holds only what is committed after.
pub fn first_commit(root: &Path) -> Result<(), String> {
    if git_out(root, &["rev-parse", "--is-inside-work-tree"]).as_deref() != Some("true") && git_raw(root, &["init", "-q"]).is_none() {
        return Err(format!("`git init` failed in {}", root.display()));
    }
    git_raw(root, &["commit", "-q", "--allow-empty", "-m", "first commit (made by j4n: a chat's worktree is cut from a commit)"])
        .map(drop)
        .ok_or_else(|| format!("the first commit failed in {}", root.display()))
}

fn mtime_ns(md: &fs::Metadata) -> u64 {
    md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos() as u64)
}

/// What a look at the folder found: its files, and what was left out and why.
#[derive(Default)]
struct Found {
    files: BTreeMap<String, Stat>,
    /// Folders left out, by the reason (`nested repository`, `build folder`).
    folders: BTreeMap<&'static str, Vec<String>>,
}

/// Every file under `dir` outside the skipped folders, the build folders and the nested repositories: path relative to `root`,
/// size and mtime. A link counts as a file.
fn walk(root: &Path, dir: &Path, cfg: &Cfg, exclude: &Path, out: &mut Found) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        if out.files.len() > MAX_FILES {
            return;
        }
        let Ok(ft) = e.file_type() else { continue };
        let path = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if ft.is_dir() {
            // J4n's own home is not the project's, wherever it sits (a test, a home that is a folder of the project)
            if cfg.skip.contains(&name) || path == exclude {
                continue;
            }
            let rel = || path.strip_prefix(root).map(|r| r.to_string_lossy().to_string()).unwrap_or_default();
            if BUILD.contains(&name.as_str()) {
                out.folders.entry("build folder").or_default().push(rel());
            } else if path.join(".git").exists() {
                // another repository (a worktree of one, a clone): its files are not this project's
                out.folders.entry("nested repository").or_default().push(rel());
            } else {
                walk(root, &path, cfg, exclude, out);
            }
        } else if let (Ok(md), Ok(rel)) = (fs::symlink_metadata(&path), path.strip_prefix(root)) {
            out.files.insert(rel.to_string_lossy().to_string(), Stat { size: md.len(), mtime_ns: mtime_ns(&md) });
        }
    }
}

/// The folder's files as git sees them: the tracked ones and the untracked ones, and the ones git ignores (an operator's
/// `.env`, a local database, notes in an ignored folder), which a task can lose as well as any. Left out are what is not the
/// project's: nested repositories and worktrees (a folder with a `.git`, listed or found under an ignored folder), the folders
/// of `cfg.skip` and the build and cache folders. `None` outside a work tree (or when git cannot answer), which sends the caller
/// to [`walk`].
fn git_files(root: &Path, cfg: &Cfg, exclude: &Path) -> Option<Found> {
    if git_out(root, &["rev-parse", "--is-inside-work-tree"]).as_deref() != Some("true") {
        return None;
    }
    let listed = git_raw(root, &["ls-files", "-z", "--cached", "--others", "--exclude-standard"])?;
    // an ignored folder comes back as one entry, which is walked here so that what is in it can be told from build output
    let ignored = git_raw(root, &["ls-files", "-z", "--others", "--ignored", "--exclude-standard", "--directory"])?;
    let mut out = Found::default();
    for (p, is_ignored) in listed.split(|c| *c == 0).map(|p| (p, false)).chain(ignored.split(|c| *c == 0).map(|p| (p, true))) {
        if p.is_empty() {
            continue;
        }
        let rel = String::from_utf8_lossy(p).to_string();
        let path = root.join(&rel);
        let parts: Vec<&str> = rel.split('/').filter(|part| !part.is_empty()).collect();
        if parts.iter().any(|part| cfg.skip.iter().any(|s| s == part)) || path.starts_with(exclude) {
            continue;
        }
        let kind = || rel.trim_end_matches('/').to_string();
        if is_ignored && parts.iter().any(|part| BUILD.contains(part)) {
            out.folders.entry("build folder").or_default().push(kind());
            continue;
        }
        // a deleted tracked file is not there to note; a folder is a nested repository, a submodule or an ignored folder
        let Ok(md) = fs::symlink_metadata(&path) else { continue };
        if !md.is_dir() {
            out.files.insert(rel, Stat { size: md.len(), mtime_ns: mtime_ns(&md) });
        } else if is_ignored && !path.join(".git").exists() {
            walk(root, &path, cfg, exclude, &mut out);
        } else {
            out.folders.entry("nested repository").or_default().push(kind());
        }
    }
    Some(out)
}

/// The folder's files as the project's own: through git where there is a repository, by walking where there is not; and the
/// `also` paths, which a caller that knows what it will write names though git ignores them.
fn look(root: &Path, cfg: &Cfg, exclude: &Path, also: &[String]) -> Found {
    let mut out = git_files(root, cfg, exclude).unwrap_or_else(|| {
        let mut out = Found::default();
        walk(root, root, cfg, exclude, &mut out);
        out
    });
    for rel in also.iter().filter(|p| plain(p)) {
        if let Ok(md) = fs::symlink_metadata(root.join(rel))
            && !md.is_dir()
        {
            out.files.insert(rel.clone(), Stat { size: md.len(), mtime_ns: mtime_ns(&md) });
        }
    }
    out
}

/// The folder's files as they stand, in one word: a hash of each one's path, size and modification time, as a checkpoint sees them,
/// without the build and cache folders a run of the tests writes in (`.pytest_cache`). The same word: nothing in the folder changed.
pub fn stamp(home: &Path, root: &Path) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for (rel, st) in look(root, &Cfg::default(), home, &[]).files.iter().filter(|(rel, _)| !rel.split('/').any(|c| BUILD.contains(&c))) {
        h.update(format!("{rel}\0{}\0{}\n", st.size, st.mtime_ns).as_bytes());
    }
    h.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// A byte count in words: `800 bytes`, `1 KB`, `200 MB`.
fn size_words(bytes: u64) -> String {
    match bytes {
        b if b >= 1024 * 1024 => format!("{} MB", b / (1024 * 1024)),
        b if b >= 1024 => format!("{} KB", b / 1024),
        b => format!("{b} bytes"),
    }
}

/// A few words on the folders left out: how many, which first.
fn left_out_lines(folders: &BTreeMap<&'static str, Vec<String>>) -> Vec<String> {
    folders
        .iter()
        .map(|(why, list)| {
            let mut list = list.clone();
            list.sort();
            list.dedup();
            let noun = match (list.len(), why.strip_suffix('y')) {
                (1, _) => why.to_string(),
                (_, Some(stem)) => format!("{stem}ies"),
                _ => format!("{why}s"),
            };
            let more = if list.len() > 3 { format!(" and {} more", list.len() - 3) } else { String::new() };
            format!("left out {} {noun}: {}{more}", list.len(), list.iter().take(3).map(String::as_str).collect::<Vec<_>>().join(", "))
        })
        .collect()
}

/// The commit that holds the tracked files now, and which files are tracked. `(None, ∅)` outside a repository or in one
/// with no commit yet.
fn git_state(root: &Path, task: &str) -> (Option<String>, BTreeSet<String>) {
    if git_out(root, &["rev-parse", "--is-inside-work-tree"]).as_deref() != Some("true") {
        return (None, BTreeSet::new());
    }
    // `stash create` makes a commit of the tracked files as they stand (and prints nothing when they are clean); it changes
    // neither the tree nor the index, and does not touch the stash list.
    let sha = git_out(root, &["stash", "create"]).filter(|s| !s.is_empty()).or_else(|| git_out(root, &["rev-parse", "HEAD"]));
    let Some(sha) = sha else { return (None, BTreeSet::new()) };
    let _ = git_out(root, &["update-ref", &format!("refs/j4n/checkpoints/{task}"), &sha]);
    let tracked = git_raw(root, &["ls-files", "-z"])
        .map(|b| b.split(|c| *c == 0).filter(|p| !p.is_empty()).map(|p| String::from_utf8_lossy(p).to_string()).collect())
        .unwrap_or_default();
    (Some(sha), tracked)
}

/// Note the folder as it stands, before a mutating task. A refusal says why, and leaves nothing behind.
pub fn begin(home: &Path, root: &Path, task: &str, cfg: &Cfg) -> Result<Checkpoint, String> {
    begin_with(home, root, task, cfg, &[])
}

/// [`begin`], and the `also` paths looked at and copied though git ignores them. An apply writes the ignored files its task
/// made, and `rewind` must take them out again: it names them here.
pub fn begin_with(home: &Path, root: &Path, task: &str, cfg: &Cfg, also: &[String]) -> Result<Checkpoint, String> {
    if id_number(task).is_none() {
        return Err(format!("`{task}` is not a task"));
    }
    let dir = store(home, root).join(task);
    let _ = fs::remove_dir_all(&dir);
    let found = look(root, cfg, home, also);
    let all = found.files;
    if all.len() > MAX_FILES {
        return Err(format!("this folder holds more than {MAX_FILES} files, too many to checkpoint"));
    }
    fs::create_dir_all(dir.join("files")).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    open_set().insert(dir.clone());
    let held = Held(dir.clone());
    let (git_ref, tracked) = git_state(root, task);
    let mut copied = BTreeSet::new();
    let mut total = 0u64;
    let (mut over_file, mut over_total) = (0usize, 0usize);
    for (rel, st) in &all {
        if tracked.contains(rel) {
            continue;
        }
        let from = root.join(rel);
        let is_link = fs::symlink_metadata(&from).map(|m| m.file_type().is_symlink()).unwrap_or(true);
        if is_link {
            continue;
        }
        if st.size > cfg.max_file_bytes {
            over_file += 1;
            continue;
        }
        if total + st.size > cfg.max_total_bytes {
            over_total += 1;
            continue;
        }
        let to = dir.join("files").join(rel);
        if let Some(parent) = to.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if fs::copy(&from, &to).is_ok() {
            total += st.size;
            copied.insert(rel.clone());
        }
    }
    let mut left_out = left_out_lines(&found.folders);
    if over_file > 0 {
        left_out.push(format!("not copied: {over_file} file{} over {}, so a change to {} cannot be put back", if over_file == 1 { "" } else { "s" }, size_words(cfg.max_file_bytes), if over_file == 1 { "it" } else { "them" }));
    }
    if over_total > 0 {
        left_out.push(format!("not copied: {over_total} file{} past the {} a checkpoint holds, so a change to {} cannot be put back", if over_total == 1 { "" } else { "s" }, size_words(cfg.max_total_bytes), if over_total == 1 { "it" } else { "them" }));
    }
    let manifest = Manifest { task: task.to_string(), git_ref, before: all, tracked, copied, skip: cfg.skip.clone(), left_out, also: also.to_vec() };
    let written = serde_json::to_vec(&manifest).map_err(|e| e.to_string()).and_then(|b| fs::write(dir.join("manifest.json"), b).map_err(|e| e.to_string()));
    if let Err(e) = written {
        let _ = fs::remove_dir_all(&dir);
        return Err(format!("cannot write the manifest: {e}"));
    }
    prune(home, root, cfg, task);
    Ok(Checkpoint { dir, root: root.to_path_buf(), home: home.to_path_buf(), manifest, _held: held })
}

/// A hold on an open checkpoint's folder ([`OPEN`]), let go with the checkpoint (or with one that is not made).
struct Held(PathBuf);

impl Drop for Held {
    fn drop(&mut self) {
        open_set().remove(&self.0);
    }
}

impl Checkpoint {
    /// What the task changed, written to `footprint.json` beside the manifest.
    pub fn end(&self) -> Result<Footprint, String> {
        let cfg = Cfg { skip: self.manifest.skip.clone(), ..Cfg::default() };
        let after = look(&self.root, &cfg, &self.home, &self.manifest.also).files;
        let mut f = Footprint { left_out: self.manifest.left_out.clone(), ..Footprint::default() };
        for (path, now) in &after {
            match self.manifest.before.get(path) {
                None => {
                    f.created.push(path.clone());
                    f.after.insert(path.clone(), *now);
                }
                Some(was) if was != now => {
                    f.modified.push(path.clone());
                    f.after.insert(path.clone(), *now);
                }
                Some(_) => {}
            }
        }
        f.deleted.extend(self.manifest.before.keys().filter(|p| !after.contains_key(*p)).cloned());
        // the checkpoint went while the task was going (another J4n on the same home): nothing can be put back, no lone footprint
        let gone = !self.dir.join("manifest.json").is_file();
        let lost: Vec<String> = f.modified.iter().chain(&f.deleted).filter(|p| gone || !self.restorable(p)).cloned().collect();
        f.not_restorable = lost;
        if gone {
            f.left_out.push("the checkpoint was dropped before the task settled, so nothing it changed can be put back".into());
            return Ok(f);
        }
        let bytes = serde_json::to_vec(&f).map_err(|e| e.to_string())?;
        fs::write(self.dir.join("footprint.json"), bytes).map_err(|e| format!("cannot write the footprint: {e}"))?;
        Ok(f)
    }

    fn restorable(&self, path: &str) -> bool {
        self.manifest.tracked.contains(path) || self.manifest.copied.contains(path)
    }
}

/// What `rewind` did.
#[derive(Debug, Default)]
pub struct RewindReport {
    pub task: String,
    pub restored: Vec<String>,
    pub removed: Vec<String>,
    /// Left alone, with the reason.
    pub skipped: Vec<(String, String)>,
    pub not_restorable: Vec<String>,
}

impl RewindReport {
    /// The lines `:rewind` prints.
    pub fn lines(&self) -> Vec<String> {
        let mut out = vec![format!(
            "rewound task {}: {} restored · {} removed · {} left alone",
            self.task,
            self.restored.len(),
            self.removed.len(),
            self.skipped.len() + self.not_restorable.len()
        )];
        out.extend(self.skipped.iter().map(|(p, why)| format!("  left {p} — {why}")));
        out.extend(self.not_restorable.iter().map(|p| format!("  cannot put back {p} — it was not tracked and was over the copy cap")));
        out
    }
}

fn settled_tasks(home: &Path, root: &Path) -> Vec<(u64, String)> {
    let Ok(entries) = fs::read_dir(store(home, root)) else { return Vec::new() };
    let mut tasks: Vec<(u64, String)> = entries
        .flatten()
        .filter(|e| e.path().join("footprint.json").is_file())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            Some((id_number(&name)?, name))
        })
        .collect();
    tasks.sort();
    tasks
}

/// The commit the tracked files were at when `task` began (a stash commit when they were dirty, else HEAD): what a diff of the
/// task's work is taken against. `None` when the task has no checkpoint, or the folder had no git.
pub fn base_ref(home: &Path, root: &Path, task: &str) -> Option<String> {
    id_number(task)?;
    let bytes = fs::read(store(home, root).join(task).join("manifest.json")).ok()?;
    serde_json::from_slice::<Manifest>(&bytes).ok()?.git_ref
}

/// The newest checkpoint of the project that has a footprint (a task that settled).
pub fn latest(home: &Path, root: &Path) -> Option<String> {
    settled_tasks(home, root).pop().map(|(_, name)| name)
}

/// A path as the store wrote it, safe to join to the project's root: relative, no `..`.
fn plain(path: &str) -> bool {
    !path.is_empty() && Path::new(path).components().all(|c| matches!(c, Component::Normal(_)))
}

/// Put the folder back as it was before `task` (the newest settled one when `None`). A file someone changed after the task
/// is left alone unless `force`.
pub fn rewind(home: &Path, root: &Path, task: Option<&str>, force: bool) -> Result<RewindReport, String> {
    let task = match task {
        Some(t) if id_number(t).is_none() => {
            return Err(format!("`{t}` is not a task (they are named like t12; leave it out for the newest)"));
        }
        Some(t) => t.to_string(),
        None => latest(home, root).ok_or("no task has left a checkpoint in this folder yet")?,
    };
    let dir = store(home, root).join(&task);
    let read = |name: &str| fs::read(dir.join(name));
    let manifest: Manifest = serde_json::from_slice(&read("manifest.json").map_err(|_| format!("no checkpoint for `{task}` in this folder"))?)
        .map_err(|e| format!("the checkpoint of `{task}` is unreadable: {e}"))?;
    let footprint: Footprint = serde_json::from_slice(&read("footprint.json").map_err(|_| format!("`{task}` never settled, so it has nothing to undo"))?)
        .map_err(|e| format!("the footprint of `{task}` is unreadable: {e}"))?;
    let mut report = RewindReport { task: task.clone(), ..Default::default() };
    let changed_since = |path: &str| -> bool {
        match (fs::symlink_metadata(root.join(path)), footprint.after.get(path)) {
            (Ok(md), Some(then)) => then.size != md.len() || then.mtime_ns != mtime_ns(&md),
            (Ok(_), None) | (Err(_), Some(_)) => true,
            (Err(_), None) => false,
        }
    };
    for path in &footprint.created {
        if !plain(path) {
            report.skipped.push((path.clone(), "not a path inside the folder".to_string()));
        } else if !force && changed_since(path) {
            report.skipped.push((path.clone(), "changed since the task".to_string()));
        } else if fs::remove_file(root.join(path)).is_ok() {
            report.removed.push(path.clone());
            remove_empty_parents(root, path);
        }
    }
    for path in footprint.modified.iter().chain(&footprint.deleted) {
        if !plain(path) {
            report.skipped.push((path.clone(), "not a path inside the folder".to_string()));
            continue;
        }
        let is_deleted = footprint.deleted.contains(path);
        if !force && ((is_deleted && root.join(path).exists()) || (!is_deleted && changed_since(path))) {
            report.skipped.push((path.clone(), "changed since the task".to_string()));
            continue;
        }
        let git_ok = manifest.git_ref.as_deref().filter(|_| manifest.tracked.contains(path)).is_some_and(|sha| {
            git_raw(root, &["restore", &format!("--source={sha}"), "--worktree", "--", path]).is_some()
        });
        if git_ok || (manifest.copied.contains(path) && copy_back(&dir, root, path).is_ok()) {
            report.restored.push(path.clone());
        } else {
            report.not_restorable.push(path.clone());
        }
    }
    Ok(report)
}

fn copy_back(dir: &Path, root: &Path, path: &str) -> std::io::Result<()> {
    let to = root.join(path);
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
        // a folder the task replaced by a link out of the project is not written through
        let (real, base) = (parent.canonicalize()?, root.canonicalize()?);
        if !real.starts_with(&base) {
            return Err(std::io::Error::other("the folder now leads out of the project"));
        }
    }
    if fs::symlink_metadata(&to).is_ok_and(|m| m.file_type().is_symlink()) {
        fs::remove_file(&to)?;
    }
    fs::copy(dir.join("files").join(path), to).map(|_| ())
}

/// A task that created `out/reports/x` also made `out/reports/` and `out/`; once the file is gone they go too if empty.
fn remove_empty_parents(root: &Path, path: &str) {
    let mut cur = root.join(path);
    while let Some(parent) = cur.parent().map(Path::to_path_buf) {
        if parent == *root || fs::remove_dir(&parent).is_err() {
            break;
        }
        cur = parent;
    }
}

/// How many bytes a checkpoint's copies hold.
fn dir_bytes(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| match e.file_type() {
                    Ok(t) if t.is_dir() => dir_bytes(&e.path()),
                    Ok(_) => e.metadata().map_or(0, |m| m.len()),
                    Err(_) => 0,
                })
                .sum()
        })
        .unwrap_or(0)
}

/// The project's checkpoints, oldest first.
fn checkpoints_of(home: &Path, root: &Path) -> Vec<(u64, String)> {
    let Ok(entries) = fs::read_dir(store(home, root)) else { return Vec::new() };
    let mut tasks: Vec<(u64, String)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            Some((id_number(&name)?, name))
        })
        .collect();
    tasks.sort();
    tasks
}

fn drop_checkpoint(home: &Path, root: &Path, name: &str) {
    let _ = fs::remove_dir_all(store(home, root).join(name));
    let _ = git_out(root, &["update-ref", "-d", &format!("refs/j4n/checkpoints/{name}")]);
}

/// Keep the newest `cfg.keep` checkpoints of the project (never `current`, nor one this process holds open), then, while the
/// rest hold more than `cfg.max_store_bytes`, drop the oldest settled one (a checkpoint with no footprint belongs to a task
/// that is still going, and `current` is never dropped); the refs go with them.
fn prune(home: &Path, root: &Path, cfg: &Cfg, current: &str) {
    let mut tasks = checkpoints_of(home, root);
    let mut over = tasks.len().saturating_sub(cfg.keep.max(1));
    tasks.retain(|(_, name)| {
        let cut = over > 0 && name != current && !open_set().contains(&store(home, root).join(name));
        if cut {
            over -= 1;
            drop_checkpoint(home, root, name);
        }
        !cut
    });
    let mut sizes: Vec<(String, u64)> = tasks.iter().map(|(_, n)| (n.clone(), dir_bytes(&store(home, root).join(n)))).collect();
    let mut total: u64 = sizes.iter().map(|(_, b)| b).sum();
    while total > cfg.max_store_bytes {
        let Some(at) = sizes.iter().position(|(n, _)| n != current && store(home, root).join(n).join("footprint.json").is_file()) else { break };
        let (name, bytes) = sizes.remove(at);
        drop_checkpoint(home, root, &name);
        total -= bytes;
    }
}
