//! Place: where a task works. `here` is the default for every task: the project's own folder, with
//! a checkpoint before a mutating task so it can be rewound. A worktree and a box are places too, made by the
//! connect crate and registered by name.
//!
//! A place prepares a [`Site`] for a task, wraps the doer's launch (a box puts the program behind its walls; `here` leaves
//! it as it is) and settles the site when the task ends, saying what changed there.
//!
//! A place that works in a folder of its own (a worktree) may hold the work after the task is over: [`Settled::kept`] says
//! so, [`Place::site_of`] lets a later task work where it was done (`@t5`, [`Via`]) and [`Place::take`] brings it to the
//! project's folder, or says what stops it.

pub mod checkpoint;

/// How the note of a site that is behind a wall begins (the box): what the tools of a model, the config of a program and the loop look for to know it.
pub const BEHIND: &str = "behind ";

use crate::table::Problem;
use crate::task::{Mode, TaskSpec};
use checkpoint::{Cfg, Checkpoint};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Where one task works.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    pub task: String,
    /// The folder the doer's program runs in.
    pub dir: PathBuf,
    /// Things the place could not do, said once (no checkpoint, so no rewind).
    pub notes: Vec<String>,
    /// The task may not write: a box binds nothing writable.
    pub read_only: bool,
    /// Folders beside `dir` that the program must be able to write (the git directory of a worktree).
    pub also: Vec<PathBuf>,
    /// The task may not start until the operator says so: what is wrong, as the question (a write task in a folder too big
    /// to checkpoint). `--approval deny` says no, `full` says yes.
    pub hold: Option<String>,
    /// Absolute paths the program may read that a wall would hide (a step's `reads`): a box binds them read-only.
    pub reads: Vec<PathBuf>,
}

impl Site {
    pub fn new(task: &str, dir: &Path) -> Site {
        Site { task: task.to_string(), dir: dir.to_path_buf(), notes: Vec::new(), read_only: false, also: Vec::new(), hold: None, reads: Vec::new() }
    }
}

/// How a doer's program is started: the arguments, the folder and the environment.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Launch {
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    /// Paths the program needs for its login and its own state (`~/.claude`): a box binds them writable.
    pub login: Vec<String>,
    /// The program needs no network (the `py` kernel): a box that can cut it does, whatever `box.net` says.
    pub no_net: bool,
    /// Folders the program reads and may not write, J4n's own home among them (the `py` kernel's packages): a box binds them read-only.
    pub ro: Vec<PathBuf>,
}

impl Launch {
    pub fn new(argv: Vec<String>, cwd: PathBuf, env: BTreeMap<String, String>) -> Launch {
        Launch { argv, cwd, env, login: Vec::new(), no_net: false, ro: Vec::new() }
    }
}

/// A change a place holds, in numbers and names: the files git tracks, and the ones it ignores (`ledger/`, `.env`), which a
/// patch cannot carry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Work {
    pub files: Vec<String>,
    pub ignored: Vec<String>,
    pub added: u64,
    pub removed: u64,
}

impl Work {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.ignored.is_empty()
    }

    /// `3 files +10 −2 · 1 ignored by git`
    pub fn counts(&self) -> String {
        let n = self.files.len();
        let mut out = format!("{n} file{} +{} −{}", if n == 1 { "" } else { "s" }, self.added, self.removed);
        if !self.ignored.is_empty() {
            out.push_str(&format!(" · {} ignored by git", self.ignored.len()));
        }
        out
    }
}

/// Work that is still in a place of its own after the task is over, and why it is not in the project's folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    pub dir: String,
    pub work: Work,
    pub why: String,
}

/// What a task changed in its place.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settled {
    pub created: Vec<String>,
    pub modified: Vec<String>,
    pub deleted: Vec<String>,
    pub not_restorable: Vec<String>,
    /// Things the place could not do (the footprint could not be written).
    pub notes: Vec<String>,
    /// The place still holds the work: it is not in the project's folder, and the place is never emptied while it is so.
    pub kept: Option<Kept>,
    /// The place brought the work to the project's folder as the task ended.
    pub applied: Option<Work>,
    /// The lists above are empty because nothing could be looked at (no checkpoint), not because nothing changed.
    pub unknown: bool,
}

impl Settled {
    pub fn is_empty(&self) -> bool {
        self.created.is_empty() && self.modified.is_empty() && self.deleted.is_empty()
    }
}

/// What to do with the work a place kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Take {
    /// Say what would happen, and change nothing.
    Look,
    /// Bring it to the project's folder, so that `rewind` can take it out again.
    Apply,
}

/// What came of [`Place::take`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Taken {
    /// The place holds nothing.
    Nothing,
    /// It would apply cleanly (`Look`), or it has (`Apply`); the notes say what could not be done besides.
    Clean(Work, Vec<String>),
    /// It does not apply, and nothing was changed; the work stays where it is.
    Blocked { why: String, work: Work },
}

/// What came of [`Place::share`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shared {
    /// The copy was made.
    Placed,
    /// The copy held older content, and is now as its source is.
    Changed,
    /// The copy is as its source is.
    Same,
}

pub struct Prepare<'a> {
    pub task: &'a str,
    pub spec: &'a TaskSpec,
    /// The project's folder.
    pub root: &'a Path,
}

pub trait Place: Send + Sync {
    fn id(&self) -> &str;

    /// Make the site ready. An error stops the task, saying why.
    fn prepare(&self, cx: &Prepare) -> Result<Site, Problem>;

    fn wrap(&self, site: &Site, launch: Launch) -> Launch;

    /// The task is over: say what it changed, and let go of what the site held.
    fn settle(&self, site: &Site) -> Settled;

    /// The site of `task` as it is now, if this place still has it: a later task may work there (`@task`).
    fn site_of(&self, _task: &str) -> Option<Site> {
        None
    }

    /// Look at the work a settled task left here, or bring it to the project's folder.
    fn take(&self, _task: &str, _how: Take) -> Result<Taken, Problem> {
        Err(Problem::refused(self.id(), "this place keeps no work"))
    }

    /// Put a read-only copy of the file `from` at `rel` in the task's site, as a share: no part of the task's work, never laid in, and made as `from` is
    /// each time it is asked again.
    fn share(&self, _task: &str, _rel: &str, _from: &Path) -> Result<Shared, String> {
        Err("this place takes no share".into())
    }
}

/// A task that works where another one did (`@t5`): the same folder, behind the same walls, and it owns none of it — what it
/// finds there is the place's to settle, when its own task is.
pub struct Via {
    origin: std::sync::Arc<dyn Place>,
    site: Site,
}

impl Via {
    /// `None` when the origin has no site for the task any more.
    pub fn of(origin: std::sync::Arc<dyn Place>, task: &str) -> Option<Via> {
        let site = origin.site_of(task)?;
        Some(Via { origin, site })
    }
}

impl Place for Via {
    fn id(&self) -> &str {
        "via"
    }

    fn prepare(&self, cx: &Prepare) -> Result<Site, Problem> {
        Ok(Site { task: cx.task.to_string(), notes: Vec::new(), hold: None, reads: Vec::new(), ..self.site.clone() })
    }

    fn wrap(&self, _site: &Site, launch: Launch) -> Launch {
        self.origin.wrap(&self.site, launch)
    }

    /// What a task that works where another did changed is the origin's to say, when its own task settles: here it is not known.
    fn settle(&self, _site: &Site) -> Settled {
        Settled { unknown: true, ..Settled::default() }
    }
}

/// The project's own folder.
pub struct Here {
    home: PathBuf,
    root: PathBuf,
    cfg: Cfg,
    open: Mutex<HashMap<String, Checkpoint>>,
}

impl Here {
    pub fn new(home: &Path, root: &Path, cfg: Cfg) -> Here {
        Here { home: home.to_path_buf(), root: root.to_path_buf(), cfg, open: Mutex::new(HashMap::new()) }
    }
}

impl Place for Here {
    fn id(&self) -> &str {
        "here"
    }

    fn prepare(&self, cx: &Prepare) -> Result<Site, Problem> {
        let mut site = Site::new(cx.task, cx.root);
        if cx.spec.tune.mode == Mode::Write {
            match checkpoint::begin(&self.home, &self.root, cx.task, &self.cfg) {
                Ok(cp) => {
                    self.open.lock().unwrap_or_else(|e| e.into_inner()).insert(cx.task.to_string(), cp);
                }
                Err(why) => {
                    site.notes.push(format!("no checkpoint, so this task cannot be rewound: {why}"));
                    site.hold = Some(format!("This task writes, and there is no checkpoint to rewind it to ({why}). Go on with no checkpoint, or stop?"));
                }
            }
        }
        Ok(site)
    }

    fn wrap(&self, _site: &Site, launch: Launch) -> Launch {
        launch
    }

    fn site_of(&self, task: &str) -> Option<Site> {
        Some(Site::new(task, &self.root))
    }

    fn settle(&self, site: &Site) -> Settled {
        let Some(cp) = self.open.lock().unwrap_or_else(|e| e.into_inner()).remove(&site.task) else { return Settled { unknown: !site.notes.is_empty(), ..Settled::default() } };
        match cp.end() {
            Ok(f) => Settled {
                created: f.created,
                modified: f.modified,
                deleted: f.deleted,
                not_restorable: f.not_restorable,
                notes: f.left_out,
                ..Settled::default()
            },
            Err(why) => Settled { notes: vec![format!("what the task changed is not known: {why}")], unknown: true, ..Settled::default() },
        }
    }
}
