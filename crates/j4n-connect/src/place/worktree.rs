//! Place `worktree`: a task that mutates, in a git worktree of its own, cut from the commit the project stands on.
//!
//! * The folder lives under `<home>/worktrees/<project key>/<task>/`, outside the project, so a task that walks up from its
//!   folder does not find the project's files, and a box can be shown the one without the other. Beside it, `<task>.json` says
//!   what it was cut from: the record that outlives a crash and the one `recover` reads.
//! * When the task is over the change is taken as one patch against that commit — committed or not, tracked or not — and the
//!   files git ignores (`ledger/`, `data/`, `.env`), which a patch cannot carry, are listed beside it and copied. With
//!   `work.apply = auto` it goes into the project's folder at once if it applies cleanly and touches nothing the operator
//!   changed there; otherwise, and with `off`, the folder is kept with the reason, and `take` does it later. Nothing is
//!   committed, merged or laid over work that is not the task's: the one file it writes over is an empty one git does not track, where the task
//!   made that file, and the checkpoint holds it.
//! * **A folder that holds work is never removed unless the work is somewhere else**: applied, or backed up under
//!   `refs/j4n/backups/<task>`. A change that is empty is the only thing reaped without a word.
//! * A **share** is a read-only copy of a file from outside the worktree (another task's part, the folder's uncommitted build files), put at the path it
//!   has in the project. The record lists it with its hash, and every git look at the work leaves it out, so it is never the task's work, never in the
//!   patch and never laid in. Git's own exclude file is not used: a linked worktree shares it with the project's folder. A share the task changed is
//!   said as a change, so the scope check sends it back.
//! * An apply is taken under a checkpoint of the project's folder, so `rewind` undoes it (the checkpoint of place `here`, filed
//!   under the task that made the change). Its record is kept under `laid/`: a copy laid in and rewound is cut again from the commit it was cut
//!   from, holding what its backup holds, when it is taken once more. The files git ignores are not in the backup, so they do not come back.

use j4n_core::place::checkpoint::{self, Cfg, SKIP};
use j4n_core::place::{Kept, Launch, Place, Prepare, Settled, Shared, Site, Take, Taken, Work};
use j4n_core::state::project_key;
use j4n_core::table::Problem;
use j4n_core::task::{Mode, id_number};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

const ID: &str = "worktree";

/// J4n's own folder is never part of a task's work.
const EXCLUDE: &str = ":(exclude).j4n";

/// The paths a patch is taken over: the worktree, less J4n's folder and the build output and caches a task's tools make (`checkpoint::SKIP`), which a
/// project with no `.gitignore` would otherwise carry into the operator's folder (`__pycache__/` from a test run).
fn over() -> Vec<String> {
    let mut specs = vec![".".to_string(), EXCLUDE.to_string()];
    specs.extend(SKIP.iter().filter(|s| !matches!(**s, ".j4n" | ".git")).map(|s| format!(":(exclude,glob)**/{s}/**")));
    specs
}

/// [`over`], less the task's shares.
fn specs_of(lane: &Lane) -> Vec<String> {
    over().into_iter().chain(lane.shares.keys().map(|p| format!(":(exclude,literal){p}"))).collect()
}

/// The largest file a share copies.
const SHARE_MAX: u64 = 2 * 1024 * 1024;

/// `git <args> -- <over()>`.
fn with_over<'a>(args: &[&'a str], specs: &'a [String]) -> Vec<&'a str> {
    args.iter().copied().chain(std::iter::once("--")).chain(specs.iter().map(String::as_str)).collect()
}

/// What happens to a task's change when it is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Apply {
    /// Into the project's folder at once, when that is safe.
    Auto,
    /// Kept in the worktree until someone takes it.
    Off,
}

impl Apply {
    pub fn parse(word: &str) -> Apply {
        if word.trim() == "off" { Apply::Off } else { Apply::Auto }
    }
}

/// What a worktree was cut from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Lane {
    dir: PathBuf,
    branch: String,
    /// The commit it was cut from: what the change is measured against.
    base: String,
    git_dir: Option<PathBuf>,
    /// The task that works here asked to have its work held (`input.hold`, as a flow's step does while the work is still to be
    /// checked and said yes to): `work.apply` does not lay it into the folder when the task is over.
    #[serde(default)]
    hold: bool,
    /// The shares in the worktree, by path, with the hash of what was copied.
    #[serde(default)]
    shares: BTreeMap<String, String>,
}

pub struct Worktree {
    home: PathBuf,
    root: PathBuf,
    /// `work.apply` as the settings say now (an edit tunes it).
    apply: Mutex<Apply>,
    cfg: Cfg,
    open: Mutex<HashMap<String, Lane>>,
    /// One change at a time goes into the project's folder.
    writing: Mutex<()>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn git(cwd: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = j4n_core::host::keyless(&mut Command::new("git"))
        .arg("-C")
        .arg(cwd)
        .args(["-c", "user.name=j4n", "-c", "user.email=j4n@localhost", "-c", "commit.gpgsign=false"])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("git cannot run ({e})"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(format!("git {} failed: {}", args.first().copied().unwrap_or(""), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

fn git_text(cwd: &Path, args: &[&str]) -> Result<String, String> {
    git(cwd, args).map(|b| String::from_utf8_lossy(&b).trim().to_string())
}

/// What git printed, as it printed it (names with spaces at their ends, and the first entry of a `-z` list, are not trimmed).
fn git_raw(cwd: &Path, args: &[&str]) -> Result<String, String> {
    git(cwd, args).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// `git apply <flags>` with the patch on stdin; the refusal when it refuses.
fn apply_patch(cwd: &Path, flags: &[&str], patch: &[u8]) -> Result<(), String> {
    let mut child = j4n_core::host::keyless(&mut Command::new("git"))
        .arg("-C")
        .arg(cwd)
        .arg("apply")
        .args(flags)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("git apply cannot run ({e})"))?;
    if let Some(mut stdin) = child.stdin.take() {
        // a refusal closes the pipe before the patch is all written
        let _ = stdin.write_all(patch);
    }
    let out = child.wait_with_output().map_err(|e| format!("git apply: {e}"))?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()) }
}

/// A commit id is data read back from disk and goes to git as a revision: only hex digits are let through, so a record that was
/// tampered with cannot hand git an option such as `--output=…`.
fn check_rev(base: &str) -> Result<(), String> {
    if (4..=64).contains(&base.len()) && base.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("the commit the worktree was cut from is not a commit id — git is not run with it".into())
    }
}

/// What a worktree changed against `base`, as numbers and as the names git tracks and the names it ignores. Stages everything in
/// the worktree (it is J4n's, and a patch needs the new files to be known to git).
fn work_of(l: &Lane) -> Result<(Work, Settled), String> {
    let (lane, base) = (l.dir.as_path(), l.base.as_str());
    check_rev(base)?;
    git(lane, &["add", "-A"])?;
    // `-z`: names come back as they are and not quoted
    let specs = specs_of(l);
    let status = git_raw(lane, &with_over(&["diff", "--cached", "--name-status", "-z", "--no-renames", base], &specs))?;
    let numstat = git_raw(lane, &with_over(&["diff", "--cached", "--numstat", "-z", "--no-renames", base], &specs))?;
    let (mut work, mut found) = (Work::default(), Settled::default());
    let mut parts = status.split('\0');
    while let (Some(how), Some(name)) = (parts.next(), parts.next()) {
        match how {
            "A" => found.created.push(name.to_string()),
            "D" => found.deleted.push(name.to_string()),
            _ => found.modified.push(name.to_string()),
        }
        work.files.push(name.to_string());
    }
    for line in numstat.split('\0') {
        let mut cols = line.splitn(3, '\t');
        let (a, r) = (cols.next().unwrap_or(""), cols.next().unwrap_or(""));
        // a binary file has `-` for both
        work.added += a.parse::<u64>().unwrap_or(0);
        work.removed += r.parse::<u64>().unwrap_or(0);
    }
    // a worktree holds no ignored file but the ones the task made, so each is its work; build output and dependencies are not
    let others = git(lane, &["ls-files", "--others", "--ignored", "--exclude-standard", "-z"])?;
    work.ignored = others
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .filter(|p| !p.split('/').any(|part| SKIP.contains(&part)) && !l.shares.contains_key(p))
        .collect();
    found.created.extend(work.ignored.iter().cloned());
    // a share the task wrote over is no work of its own to lay in, but it is a change, and the scope check sends it back
    found.modified.extend(l.shares.iter().filter(|(p, h)| std::fs::read(lane.join(p)).ok().map(j4n_core::text::hex_hash).as_ref() != Some(*h)).map(|(p, _)| p.clone()));
    Ok((work, found))
}

/// Copy `from` into the worktree at `rel` as a share, read-only. A file the task made or changed there is its own and is never laid over.
fn share_into(lane: &mut Lane, rel: &str, from: &Path) -> Result<Shared, String> {
    if rel.is_empty() || rel.starts_with('/') || rel.split('/').any(|c| matches!(c, "" | "." | ".." | ".git" | ".j4n")) {
        return Err(format!("`{rel}` is not a path a share may take"));
    }
    // the source is a plain file: a link would carry any file of the machine in under the name of a part
    let meta = std::fs::symlink_metadata(from).map_err(|e| format!("{} cannot be read ({e})", from.display()))?;
    if !meta.file_type().is_file() || meta.len() > SHARE_MAX {
        return Err(format!("{} is not a plain file of at most {SHARE_MAX} bytes", from.display()));
    }
    if !lane.shares.contains_key(rel) && !git_raw(&lane.dir, &["--literal-pathspecs", "status", "--porcelain", "--ignored", "--", rel])?.trim().is_empty() {
        return Err(format!("{rel} is this task's own work"));
    }
    let bytes = std::fs::read(from).map_err(|e| format!("{} cannot be read ({e})", from.display()))?;
    let hash = j4n_core::text::hex_hash(&bytes);
    let to = lane.dir.join(rel);
    let was = lane.shares.get(rel).cloned();
    if was.as_deref() == Some(hash.as_str()) && std::fs::read(&to).ok().map(j4n_core::text::hex_hash).as_deref() == Some(hash.as_str()) {
        return Ok(Shared::Same);
    }
    let dir = to.parent().ok_or("no folder")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot make {} ({e})", dir.display()))?;
    // a folder on the way that is a link out of the worktree is not written through
    if !dir.canonicalize().is_ok_and(|d| lane.dir.canonicalize().is_ok_and(|w| d.starts_with(w))) {
        return Err(format!("{rel} is not inside the worktree"));
    }
    if std::fs::symlink_metadata(&to).is_ok() {
        std::fs::remove_file(&to).map_err(|e| format!("cannot replace {rel} ({e})"))?;
    }
    std::fs::write(&to, &bytes).map_err(|e| format!("cannot write {rel} ({e})"))?;
    let mut perm = std::fs::metadata(&to).map_err(|e| e.to_string())?.permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&to, perm).map_err(|e| format!("cannot make {rel} read-only ({e})"))?;
    lane.shares.insert(rel.to_string(), hash);
    Ok(if was.is_some() { Shared::Changed } else { Shared::Placed })
}

enum Mood {
    Look,
    Do,
}

enum Landed {
    Nothing,
    Clean(Work),
    Blocked(String, Work),
}

/// Bring the worktree's change to the project's folder: the patch, then the ignored files. Everything is checked first, so a
/// change that cannot be laid down whole is not laid down in part.
fn land(root: &Path, lane: &Lane, mood: Mood) -> Result<Landed, String> {
    let (work, found) = work_of(lane)?;
    if work.is_empty() {
        return Ok(Landed::Nothing);
    }
    // work the operator already has in those files is theirs: never laid over, never merged blind
    let (mut same, mut over): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    if !work.files.is_empty() {
        let mut args: Vec<&str> = vec!["--literal-pathspecs", "status", "--porcelain", "-z", "--no-renames", "--"];
        args.extend(work.files.iter().map(String::as_str));
        let busy = git_raw(root, &args)?;
        let busy: Vec<(String, String)> = busy.split('\0').filter(|l| !l.trim().is_empty()).map(|l| (l.get(..2).unwrap_or_default().to_string(), l.get(3..).unwrap_or(l).trim().to_string())).collect();
        // a file the folder already holds as the task left it (two parts that made the same empty `__init__.py`) is no clash: it is left out of the patch
        let file = |dir: &Path, p: &str| Some(dir.join(p)).filter(|f| f.is_file()).and_then(|f| std::fs::read(f).ok());
        // nor is an empty file git does not track where the task made one (Main's empty `__init__.py`): it holds nothing to lose, and is written over after the
        // patch; both are plain files, so nothing is read or written through a link
        let plain = |dir: &Path, p: &str| std::fs::symlink_metadata(dir.join(p)).ok().filter(|m| m.file_type().is_file()).map(|m| m.len());
        let mut names = Vec::new();
        for (how, p) in busy {
            if file(root, &p).is_some_and(|mine| file(&lane.dir, &p) == Some(mine)) {
                same.push(format!(":(exclude,literal){p}"));
            } else if how == "??" && plain(root, &p) == Some(0) && found.created.contains(&p) && plain(&lane.dir, &p).is_some() {
                same.push(format!(":(exclude,literal){p}"));
                over.push(p);
            } else {
                names.push(p);
            }
        }
        if !names.is_empty() {
            let why = format!("{} of those files already have uncommitted changes in your folder ({}) — nothing was laid over them", names.len(), names.join(", "));
            return Ok(Landed::Blocked(why, work));
        }
    }
    // a file git ignores that the task wrote where the folder already has another is the operator's to decide, the same way
    let differing: Vec<&str> = work.ignored.iter().map(String::as_str).filter(|p| std::fs::read(root.join(p)).is_ok_and(|mine| std::fs::read(lane.dir.join(p)).ok().as_deref() != Some(&mine))).collect();
    if !differing.is_empty() {
        let why = format!("{} of the files git ignores already exist in your folder with other content ({}) — nothing was laid over them", differing.len(), differing.join(", "));
        return Ok(Landed::Blocked(why, work));
    }
    let specs: Vec<String> = specs_of(lane).into_iter().chain(same).collect();
    let patch = if work.files.is_empty() { Vec::new() } else { git(&lane.dir, &with_over(&["diff", "--cached", "--binary", "--full-index", &lane.base], &specs))? };
    if !patch.is_empty() {
        if let Err(why) = apply_patch(root, &["--check"], &patch) {
            return Ok(Landed::Blocked(format!("the change does not apply to your folder as it is now: {why}"), work));
        }
        if matches!(mood, Mood::Do)
            && let Err(why) = apply_patch(root, &["--whitespace=nowarn"], &patch)
        {
            return Ok(Landed::Blocked(format!("applying failed part-way: {why}"), work));
        }
    }
    if matches!(mood, Mood::Do) {
        for rel in over.iter().chain(&work.ignored) {
            let to = root.join(rel);
            let copied = to.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::copy(lane.dir.join(rel), &to).map(|_| ()));
            if let Err(e) = copied {
                return Ok(Landed::Blocked(format!("could not copy {rel} into your folder: {e}"), work));
            }
        }
    }
    Ok(Landed::Clean(work))
}

/// Remove a worktree whose work is known to be somewhere else, after a backup that keeps whatever is in it reachable.
fn remove(root: &Path, lane: &Lane, task: &str, backup: bool) -> Result<(), String> {
    if backup {
        let _ = git(&lane.dir, &["add", "-A"]);
        let sha = git_text(&lane.dir, &["stash", "create"]).ok().filter(|s| !s.is_empty()).or_else(|| git_text(&lane.dir, &["rev-parse", "HEAD"]).ok());
        if let Some(sha) = sha {
            let _ = git(root, &["update-ref", &format!("refs/j4n/backups/{task}"), &sha]);
        }
    }
    git(root, &["worktree", "remove", "--force", &lane.dir.to_string_lossy()])?;
    let _ = git(root, &["branch", "-D", &lane.branch]);
    Ok(())
}

impl Worktree {
    pub fn new(home: &Path, root: &Path, apply: Apply, cfg: Cfg) -> Worktree {
        Worktree { home: home.to_path_buf(), root: root.to_path_buf(), apply: Mutex::new(apply), cfg, open: Mutex::new(HashMap::new()), writing: Mutex::new(()) }
    }

    fn store(&self) -> PathBuf {
        self.home.join("worktrees").join(project_key(&self.root))
    }

    fn record(&self, task: &str) -> PathBuf {
        self.store().join(format!("{task}.json"))
    }

    fn save(&self, task: &str, lane: &Lane) -> Result<(), String> {
        let path = self.record(task);
        let text = serde_json::to_string_pretty(lane).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("tmp");
        std::fs::create_dir_all(self.store()).and_then(|()| std::fs::write(&tmp, text)).and_then(|()| std::fs::rename(&tmp, &path)).map_err(|e| format!("cannot write {}: {e}", path.display()))
    }

    /// The record of a copy whose work was laid in: what it was cut from, to cut it again if the lay-in is rewound.
    fn laid(&self, task: &str) -> PathBuf {
        self.store().join("laid").join(format!("{task}.json"))
    }

    /// The copy of a task whose work was laid in and is not in the folder any more (the lay-in was rewound): cut again from the commit it was
    /// cut from, holding what its backup holds. `None`: no work of the task was laid in, or it has no backup.
    fn revive(&self, task: &str) -> Option<Result<Lane, String>> {
        id_number(task)?;
        let lane: Lane = serde_json::from_str(&std::fs::read_to_string(self.laid(task)).ok()?).ok()?;
        if lane.dir != self.store().join(task) || lane.dir.exists() {
            return None;
        }
        let backup = git_text(&self.root, &["rev-parse", "--verify", "-q", &format!("refs/j4n/backups/{task}^{{commit}}")]).ok().filter(|s| !s.is_empty())?;
        let cut = || -> Result<Lane, String> {
            check_rev(&lane.base)?;
            git(&self.root, &["worktree", "add", "-B", &lane.branch, &lane.dir.to_string_lossy(), &lane.base])?;
            git(&lane.dir, &["read-tree", "-u", "--reset", &backup])?;
            self.save(task, &lane)?;
            let _ = std::fs::remove_file(self.laid(task));
            Ok(lane.clone())
        };
        Some(cut())
    }

    fn forget(&self, task: &str) {
        lock(&self.open).remove(task);
        let _ = std::fs::remove_file(self.record(task));
    }

    /// The worktree of a task: in memory, else from its record on disk.
    fn lane(&self, task: &str) -> Option<Lane> {
        id_number(task)?;
        if let Some(l) = lock(&self.open).get(task) {
            return Some(l.clone());
        }
        let lane: Lane = serde_json::from_str(&std::fs::read_to_string(self.record(task)).ok()?).ok()?;
        (lane.dir == self.store().join(task) && lane.dir.exists()).then_some(lane)
    }

    /// Worktrees whose task is not running any more and that nobody took: what a crash leaves. Each is said to hold what it
    /// holds, so the task's row can offer it.
    pub fn recover(&self) -> Vec<(String, Kept)> {
        let Ok(entries) = std::fs::read_dir(self.store()) else { return Vec::new() };
        let mut found: Vec<(String, Kept)> = entries
            .flatten()
            .filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".json").map(String::from))
            .filter_map(|task| {
                let lane = self.lane(&task)?;
                let (work, _) = work_of(&lane).ok()?;
                let why = "J4n stopped while the task ran: its work is here, and not in your folder".to_string();
                (!work.is_empty()).then(|| (task, Kept { dir: lane.dir.display().to_string(), work, why }))
            })
            .collect();
        found.sort_by_key(|(t, _)| id_number(t));
        found
    }

    /// Lay the change down under a checkpoint of the folder, so that `rewind` can take it out again. The notes say what could
    /// not be done.
    fn lay(&self, task: &str, lane: &Lane) -> Result<(Landed, Vec<String>), String> {
        let _one = lock(&self.writing);
        // a share placed since `lane` was read is on the record, and is left out with the others
        let fresh = self.lane(task);
        self.lay_held(task, fresh.as_ref().unwrap_or(lane))
    }

    /// [`Self::lay`], with `writing` held.
    fn lay_held(&self, task: &str, lane: &Lane) -> Result<(Landed, Vec<String>), String> {
        let mut notes = Vec::new();
        // the ignored files the apply will write are the checkpoint's too, so that rewinding the apply takes them out
        let ignored = work_of(lane).map(|(w, _)| w.ignored).unwrap_or_default();
        let cp = match checkpoint::begin_with(&self.home, &self.root, task, &self.cfg, &ignored) {
            Ok(cp) => Some(cp),
            Err(why) => {
                notes.push(format!("no checkpoint, so this cannot be rewound: {why}"));
                None
            }
        };
        let landed = land(&self.root, lane, Mood::Do)?;
        if let Some(cp) = cp
            && let Err(why) = cp.end()
        {
            notes.push(format!("what the apply changed was not written down, so it cannot be rewound: {why}"));
        }
        Ok((landed, notes))
    }
}

impl Place for Worktree {
    fn id(&self) -> &str {
        ID
    }

    fn prepare(&self, cx: &Prepare) -> Result<Site, Problem> {
        let refuse = |why: String| Problem::refused(ID, why);
        if id_number(cx.task).is_none() {
            return Err(refuse(format!("`{}` is not a task", cx.task)));
        }
        let base = git_text(cx.root, &["rev-parse", "HEAD"]).map_err(|_| {
            refuse(format!("{} has no commit yet — a task in a worktree works in a copy cut from the latest commit, so make a first commit (an empty one is enough) and ask again", cx.root.display()))
        })?;
        let dir = self.store().join(cx.task);
        let branch = format!("j4n/{}", cx.task);
        let _ = std::fs::remove_file(self.laid(cx.task));
        if dir.exists() || self.record(cx.task).exists() {
            // a task name made again starts from nothing
            let _ = git(cx.root, &["worktree", "remove", "--force", &dir.to_string_lossy()]);
            let _ = std::fs::remove_dir_all(&dir);
            let _ = git(cx.root, &["branch", "-D", &branch]);
            let _ = git(cx.root, &["worktree", "prune"]);
        }
        std::fs::create_dir_all(self.store()).map_err(|e| refuse(format!("cannot make {}: {e}", self.store().display())))?;
        git(cx.root, &["worktree", "add", "-b", &branch, &dir.to_string_lossy(), &base]).map_err(refuse)?;
        let git_dir = git_text(&dir, &["rev-parse", "--git-common-dir"]).ok().map(|p| if Path::new(&p).is_absolute() { PathBuf::from(p) } else { dir.join(p) }).and_then(|p| p.canonicalize().ok());
        let lane = Lane { dir: dir.clone(), branch, base: base.clone(), git_dir: git_dir.clone(), hold: cx.spec.input.get("hold").and_then(serde_json::Value::as_bool).unwrap_or(false), shares: BTreeMap::new() };
        self.save(cx.task, &lane).map_err(refuse)?;
        lock(&self.open).insert(cx.task.to_string(), lane);
        let mut site = Site::new(cx.task, &dir);
        // the shares the task is given before it starts: `[{path, from}]`
        for s in cx.spec.input.get("shares").and_then(serde_json::Value::as_array).into_iter().flatten() {
            let word = |k: &str| s.get(k).and_then(serde_json::Value::as_str).unwrap_or_default();
            if let Err(why) = self.share(cx.task, word("path"), Path::new(word("from"))) {
                site.notes.push(format!("{} is not shared: {why}", word("path")));
            }
        }
        site.read_only = cx.spec.tune.mode == Mode::Read;
        site.also = git_dir.into_iter().collect();
        let short = base.chars().take(8).collect::<String>();
        site.notes.push(format!("works in a copy of its own, cut from {short}"));
        if git_text(cx.root, &["status", "--porcelain", "--untracked-files=no"]).is_ok_and(|s| !s.is_empty()) {
            site.notes.push("your folder has changes that are not committed: they are not in the copy".into());
        }
        Ok(site)
    }

    fn wrap(&self, _site: &Site, launch: Launch) -> Launch {
        launch
    }

    fn settle(&self, site: &Site) -> Settled {
        // no share is placed while the work is read and laid in: what is read is what is left out
        let _one = lock(&self.writing);
        let Some(lane) = self.lane(&site.task) else { return Settled { unknown: true, ..Settled::default() } };
        let task = site.task.as_str();
        let (work, mut found) = match work_of(&lane) {
            Ok(w) => w,
            Err(why) => {
                let kept = Kept { dir: lane.dir.display().to_string(), work: Work::default(), why: format!("what the task changed is not known: {why}") };
                return Settled { kept: Some(kept), ..Settled::default() };
            }
        };
        if work.is_empty() {
            if let Err(why) = remove(&self.root, &lane, task, false) {
                found.notes.push(format!("the worktree of a task that changed nothing could not be removed: {why}"));
            } else {
                self.forget(task);
            }
            return found;
        }
        let kept = |why: String| Kept { dir: lane.dir.display().to_string(), work: work.clone(), why };
        if *self.apply.lock().unwrap_or_else(|e| e.into_inner()) == Apply::Auto && !lane.hold {
            match self.lay_held(task, &lane) {
                Ok((Landed::Clean(done), notes)) => {
                    found.notes.extend(notes);
                    match remove(&self.root, &lane, task, true) {
                        Ok(()) => self.forget(task),
                        Err(why) => {
                            found.notes.push(format!("applied to your folder, but the worktree could not be removed: {why}"));
                            self.forget(task);
                        }
                    }
                    found.applied = Some(done);
                    return found;
                }
                Ok((Landed::Blocked(why, _), notes)) => {
                    found.notes.extend(notes);
                    found.kept = Some(kept(format!("not applied: {why}")));
                    return found;
                }
                Ok((Landed::Nothing, _)) => return found,
                Err(why) => {
                    found.kept = Some(kept(format!("could not bring the change to your folder: {why}")));
                    return found;
                }
            }
        }
        // off: it waits; look, so that what is said is true
        let why = match land(&self.root, &lane, Mood::Look) {
            Ok(Landed::Clean(_)) => "waiting to be applied; it would apply cleanly".to_string(),
            Ok(Landed::Blocked(why, _)) => format!("waiting to be applied; it does not apply now: {why}"),
            Ok(Landed::Nothing) => "waiting to be applied".to_string(),
            Err(why) => format!("waiting to be applied; it could not be tried: {why}"),
        };
        found.kept = Some(kept(why));
        found
    }

    fn site_of(&self, task: &str) -> Option<Site> {
        let lane = self.lane(task)?;
        let mut site = Site::new(task, &lane.dir);
        site.also = lane.git_dir.into_iter().collect();
        Some(site)
    }

    fn share(&self, task: &str, rel: &str, from: &Path) -> Result<Shared, String> {
        let _one = lock(&self.writing);
        let mut lane = self.lane(task).ok_or_else(|| format!("the worktree of {task} is gone"))?;
        let said = share_into(&mut lane, rel, from)?;
        if said != Shared::Same {
            self.save(task, &lane)?;
            lock(&self.open).insert(task.to_string(), lane);
        }
        Ok(said)
    }

    fn take(&self, task: &str, how: Take) -> Result<Taken, Problem> {
        let refuse = |why: String| Problem::refused(ID, why);
        let lane = match self.lane(task) {
            Some(lane) => lane,
            None => self.revive(task).ok_or_else(|| refuse(format!("the worktree of {task} is gone")))?.map_err(refuse)?,
        };
        match how {
            Take::Look => Ok(match {
                let _one = lock(&self.writing);
                let lane = self.lane(task).unwrap_or(lane);
                land(&self.root, &lane, Mood::Look)
            }
            .map_err(refuse)?
            {
                Landed::Nothing => Taken::Nothing,
                Landed::Clean(w) => Taken::Clean(w, Vec::new()),
                Landed::Blocked(why, work) => Taken::Blocked { why, work },
            }),
            Take::Apply => {
                let (landed, mut notes) = self.lay(task, &lane).map_err(refuse)?;
                match landed {
                    Landed::Nothing => {
                        remove(&self.root, &lane, task, false).map_err(refuse)?;
                        self.forget(task);
                        Ok(Taken::Nothing)
                    }
                    Landed::Clean(work) => {
                        let held = self.lane(task).unwrap_or(lane.clone());
                        let record = serde_json::to_string_pretty(&held).map_err(|e| e.to_string()).and_then(|t| {
                            std::fs::create_dir_all(self.store().join("laid")).and_then(|()| std::fs::write(self.laid(task), t)).map_err(|e| e.to_string())
                        });
                        if let Err(why) = record {
                            notes.push(format!("the record of the copy was not kept, so it cannot be laid in again once rewound: {why}"));
                        }
                        if let Err(why) = remove(&self.root, &lane, task, true) {
                            notes.push(format!("applied to your folder, but the worktree could not be removed: {why}"));
                        }
                        self.forget(task);
                        Ok(Taken::Clean(work, notes))
                    }
                    Landed::Blocked(why, work) => Ok(Taken::Blocked { why, work }),
                }
            }
        }
    }
}

impl j4n_core::setting::Tune for Worktree {
    fn tune(&self, settings: &j4n_core::setting::Settings) {
        *self.apply.lock().unwrap_or_else(|e| e.into_inner()) = Apply::parse(settings.word("work.apply", "auto"));
    }
}
