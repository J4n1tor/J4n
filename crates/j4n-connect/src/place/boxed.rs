//! Place `box`: the program of a
//! task run behind a wall that no prompt can talk its way past. An agent is untrusted code with a shell, and the same wall holds
//! for every connector, because every connector starts its program through the place's `wrap`.
//!
//! What the wall shows (`packs/base/boxes/`): the host read-only, the task's folder (and the git directory of a worktree)
//! writable unless the task only reads, the folders its program keeps its login and state in writable (`login` of the connector,
//! and what the row's `write_home` names), credentials and J4n's own home empty. The network stays on: the agents are API clients.
//!
//! A box **fails closed**: a wall that cannot run here (a container has no user namespaces) is found by a probe before the task,
//! and the task is refused with the way out, unless `box.unavailable` is `here`, which runs it unboxed and says so.

use super::view::{self, Cover, Mount};
use j4n_core::place::{Launch, Place, Prepare, Settled, Site, Take, Taken};
use j4n_core::redact::Redactor;
use j4n_core::setting::Settings;
use j4n_core::table::{Kind, Problem, Tables};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

/// How the note of a site that is behind a wall begins: what the tools of a model look for to know it (`py`).
pub use j4n_core::place::BEHIND;

/// An entry of kind `box`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoxRow {
    pub id: String,
    pub doc: Option<String>,
    pub probe: Vec<String>,
    pub pre: Vec<String>,
    pub ro: Vec<String>,
    pub rw: Vec<String>,
    #[serde(default)]
    pub mounts: Vec<String>,
    pub hide_dir: Vec<String>,
    pub hide_file: Vec<String>,
    #[serde(default)]
    pub cwd: Vec<String>,
    #[serde(default)]
    pub no_net: Vec<String>,
    pub end: Vec<String>,
    #[serde(default = "root_only")]
    pub read_only: Vec<String>,
    #[serde(default)]
    pub hide: Vec<String>,
    #[serde(default)]
    pub write_home: Vec<String>,
    /// Mounts the view of a secret file (`{src}`, the copy) over the real one (`{p}`): read-only, and writable for a task that may write.
    #[serde(default)]
    pub view: Vec<String>,
    #[serde(default)]
    pub view_rw: Vec<String>,
}

fn root_only() -> Vec<String> {
    vec!["/".into()]
}

impl Kind for BoxRow {
    const NAME: &'static str = "box";
}

/// What one task may touch.
#[derive(Debug, Clone)]
pub struct Spec {
    pub dir: PathBuf,
    /// Folders beside `dir` that must be writable too (the git directory of a worktree).
    pub also: Vec<PathBuf>,
    /// A task that only reads gets no writable folder of its own.
    pub read_only: bool,
    /// Where the program keeps its login and its state, as `~/…` paths.
    pub login: Vec<String>,
    /// The user's home (what `~` means).
    pub home: PathBuf,
    /// J4n's own home: its checkpoints hold copies of the project's ignored files, so a task does not see it.
    pub j4n_home: PathBuf,
    pub net: bool,
    /// Absolute paths the program may read that the wall would hide: bound read-only after the `mounts` (a private `/tmp`), before the hiding.
    pub reads: Vec<PathBuf>,
}

/// What the pure [`argv`] asks about the filesystem, so it can be tested exactly.
pub struct Fs<'a> {
    pub exists: &'a dyn Fn(&Path) -> bool,
    pub is_dir: &'a dyn Fn(&Path) -> bool,
}

fn expand(p: &str, home: &Path) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if p == "~" => home.to_path_buf(),
        None => PathBuf::from(p),
    }
}

fn each(template: &[String], p: &Path, out: &mut Vec<String>) {
    out.extend(template.iter().map(|t| t.replace("{p}", &p.to_string_lossy())));
}

/// The command line that runs `launch` behind the wall, or why this wall cannot run it. A path that does not exist is left out
/// (a bind of nothing is an error to the wall). The order is the kind's: `pre`, read-only paths, `mounts`, hidden paths, writable
/// paths, the network switch, `end`, the program.
pub fn argv(row: &BoxRow, spec: &Spec, fs: &Fs, launch: Launch) -> Result<Launch, String> {
    argv_with(row, spec, fs, launch, &[])
}

/// [`argv`] with the views of secret files, which are mounted **last**: after every path a later bind could cover (a login folder
/// that holds the project) and just before the network switch.
pub fn argv_with(row: &BoxRow, spec: &Spec, fs: &Fs, launch: Launch, views: &[Mount]) -> Result<Launch, String> {
    if !spec.net && row.no_net.is_empty() {
        return Err(format!("{} cannot cut the network", row.id));
    }
    let mut out: Vec<String> = row.pre.clone();
    for p in row.read_only.iter().map(|p| expand(p, &spec.home)).filter(|p| (fs.exists)(p)) {
        each(&row.ro, &p, &mut out);
    }
    out.extend(row.mounts.iter().cloned());
    // what the step lists in `reads`: over a private `/tmp` and the like, and before the hiding, so a credentials folder stays hidden whatever is listed
    for p in spec.reads.iter().filter(|p| p.is_absolute() && (fs.exists)(p)) {
        each(&row.ro, p, &mut out);
    }
    // hidden before writable: a folder of the task's that lives under a hidden one (the worktrees are filed in J4n's home) is bound back
    let hidden = row.hide.iter().map(|p| expand(p, &spec.home)).chain([spec.j4n_home.clone()]);
    for p in hidden.filter(|p| (fs.exists)(p)) {
        each(if (fs.is_dir)(&p) { &row.hide_dir } else { &row.hide_file }, &p, &mut out);
    }
    // after the hiding, so a folder under J4n's home (the kit's packages) is bound back, read-only
    for p in launch.ro.iter().filter(|p| (fs.exists)(p)) {
        each(&row.ro, p, &mut out);
    }
    let mut writable: Vec<PathBuf> = row.write_home.iter().chain(&spec.login).chain(&launch.login).map(|p| expand(p, &spec.home)).collect();
    let own: Vec<PathBuf> = std::iter::once(spec.dir.clone()).chain(spec.also.iter().cloned()).collect();
    if spec.read_only {
        // a task that only reads still sees its folder, which may be filed under a hidden one
        for p in own.iter().filter(|p| (fs.exists)(p)) {
            each(&row.ro, p, &mut out);
        }
    } else {
        writable.extend(own);
    }
    let mut seen: Vec<PathBuf> = Vec::new();
    for p in writable.into_iter().filter(|p| (fs.exists)(p)) {
        if !seen.contains(&p) {
            each(&row.rw, &p, &mut out);
            seen.push(p);
        }
    }
    each(&row.cwd, &launch.cwd, &mut out);
    for m in views {
        let template = if m.writable { &row.view_rw } else { &row.view };
        if template.is_empty() {
            return Err(format!("{} has no way to mount a view of {}", row.id, m.at.display()));
        }
        out.extend(template.iter().map(|t| t.replace("{src}", &m.src.to_string_lossy()).replace("{p}", &m.at.to_string_lossy())));
    }
    if !spec.net {
        out.extend(row.no_net.iter().cloned());
    }
    out.extend(row.end.iter().cloned());
    out.extend(launch.argv.iter().cloned());
    if launch.argv.is_empty() {
        return Err("there is no program to put behind the wall".into());
    }
    Ok(Launch { argv: out, ..launch })
}

/// What a task's folders and its `reads` are, to tell what the wall hides of what its goal names.
pub struct Seen<'a> {
    /// The folders the task works in: they are bound, whatever is under them.
    pub own: Vec<PathBuf>,
    pub reads: &'a [PathBuf],
}

/// The notice, if there is one, that the wall hides a path the goal names: an absolute path that exists on the host, lies under a folder the wall makes
/// empty by a `--tmpfs` mount (a private `/tmp`) and is neither under the task's folders nor under a `reads` path. The task would look for it and not
/// find it, which is not what the wall is for: it is told, and how to have it seen. Credentials and J4n's own home stay hidden whatever is listed, so
/// they are not offered.
pub fn hidden_in_goal(row: &BoxRow, goal: &str, seen: &Seen, exists: &dyn Fn(&Path) -> bool) -> Option<String> {
    let walls: Vec<PathBuf> = row.mounts.windows(2).filter(|w| w[0] == "--tmpfs").map(|w| PathBuf::from(&w[1])).collect();
    let mut named: Vec<PathBuf> = Vec::new();
    for word in goal.split(|c: char| c.is_whitespace() || "\"'`()[]{}<>,;".contains(c)) {
        let word = word.trim_end_matches(['.', ':', '!', '?']);
        let p = Path::new(word);
        if !word.starts_with('/') || word.len() < 2 || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            continue;
        }
        let hidden = walls.iter().any(|w| p.starts_with(w));
        let shown = seen.own.iter().chain(seen.reads).any(|o| p.starts_with(o));
        if hidden && !shown && exists(p) && !named.contains(&p.to_path_buf()) {
            named.push(p.to_path_buf());
        }
    }
    if named.is_empty() {
        return None;
    }
    named.truncate(5);
    let list = named.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ");
    Some(format!("the box hides {list}; list {} in the step's `reads` to see {}", if named.len() == 1 { "it" } else { "them" }, if named.len() == 1 { "it" } else { "them" }))
}

/// Run the probe of a wall: a trivial program behind it. The error says what the wall said.
fn probe(row: &BoxRow) -> Result<(), String> {
    let (cmd, args) = row.probe.split_first().ok_or_else(|| format!("box `{}` has no probe", row.id))?;
    match j4n_core::host::keyless(&mut Command::new(cmd)).args(args).stdin(Stdio::null()).output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => {
            let said = String::from_utf8_lossy(&o.stderr);
            let said = said.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
            Err(format!("`{}` exited {}{}", row.probe.join(" "), o.status.code().map_or("by signal".into(), |c| c.to_string()), if said.is_empty() { String::new() } else { format!(": {said}") }))
        }
        Err(e) => Err(format!("`{cmd}` cannot run: {e}")),
    }
}

/// What to do when no wall can run here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    /// The task is refused, with the way out.
    Refuse,
    /// It runs where its inner place puts it, unboxed, and says so.
    Here,
}

impl Unavailable {
    pub fn parse(word: &str) -> Unavailable {
        if word.trim() == "here" { Unavailable::Here } else { Unavailable::Refuse }
    }
}

/// The rows of the Table, by id.
pub fn rows(tables: &Tables) -> (Vec<BoxRow>, Vec<Problem>) {
    let (mut list, mut problems) = (Vec::new(), Vec::new());
    for id in tables.ids(BoxRow::NAME) {
        match tables.get::<BoxRow>(id) {
            Ok(row) => list.push(row),
            Err(p) => problems.push(p),
        }
    }
    (list, problems)
}

/// Which wall `box.use` names (`auto`: the first row whose probe passes), proven once and kept.
enum Wall {
    Unproven,
    Found(Arc<BoxRow>),
    None(String),
}

pub struct Boxed {
    id: String,
    inner: Arc<dyn Place>,
    rows: Vec<BoxRow>,
    choice: String,
    unavailable: Unavailable,
    net: bool,
    home: PathBuf,
    j4n_home: PathBuf,
    wall: Mutex<Wall>,
    /// The redactor and the name globs of the pack `guard` when it is on; none, and nothing below runs.
    guard: Option<(Redactor, Vec<String>)>,
    covers: Mutex<HashMap<String, Vec<Cover>>>,
    checked: Mutex<HashSet<String>>,
}

impl Boxed {
    pub fn new(id: &str, inner: Arc<dyn Place>, rows: Vec<BoxRow>, settings: &Settings, home: &Path, j4n_home: &Path) -> Boxed {
        Boxed {
            id: id.to_string(),
            inner,
            rows,
            choice: settings.word("box.use", "auto").to_string(),
            unavailable: Unavailable::parse(settings.word("box.unavailable", "refuse")),
            net: !matches!(settings.get("box.net"), Some(j4n_core::value::Value::Bool(false))),
            home: home.to_path_buf(),
            j4n_home: j4n_home.to_path_buf(),
            wall: Mutex::new(Wall::Unproven),
            guard: None,
            covers: Mutex::default(),
            checked: Mutex::default(),
        }
    }

    /// The pack `guard` is on: the secret files `guard.files` names (and the redactor changes) are shown as their view.
    pub fn guarded(mut self, red: &Redactor, settings: &Settings) -> Boxed {
        self.guard = Some((red.clone(), settings.word("guard.files", "").split_whitespace().map(String::from).collect()));
        self
    }

    /// Make the views of the secret files of the task's folder, and keep them for `wrap` and `settle`.
    fn cover(&self, site: &Site, row: &BoxRow, writable: bool) -> Result<Vec<String>, String> {
        let Some((red, globs)) = &self.guard else { return Ok(Vec::new()) };
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&site.dir, &mut hash);
        let keep = self.j4n_home.join("views").join(format!("{}-{:x}", site.task, std::hash::Hasher::finish(&hash)));
        drop(std::fs::remove_dir_all(&keep));
        let (covers, left) = view::find(red, globs, &site.dir, &keep, writable)?;
        if !covers.is_empty() && row.view.is_empty() {
            return Err(format!("`{}` has no way to mount a view, so the secret files of {} would show as they are. Use a box that can, or turn the pack `guard` off", row.id, site.dir.display()));
        }
        let notes = view::notes(&covers, &left);
        self.covers.lock().unwrap_or_else(|e| e.into_inner()).insert(site.task.clone(), covers);
        Ok(notes)
    }

    /// Put what the program wrote into the views back into the real files, and let go of the copies.
    fn write_back(&self, site: &Site) -> Vec<String> {
        let covers = self.covers.lock().unwrap_or_else(|e| e.into_inner()).remove(&site.task).unwrap_or_default();
        let mut notes = Vec::new();
        for c in covers.iter().filter(|c| c.writable) {
            let Ok(edited) = std::fs::read_to_string(&c.src) else { continue };
            if edited == c.view {
                continue;
            }
            let said = match std::fs::read_to_string(&c.canon) {
                Ok(now) if now == c.real => view::merge(c, &edited).and_then(|text| std::fs::write(&c.canon, text).map_err(|e| e.to_string())),
                _ => Err("it was changed on the host meanwhile".into()),
            };
            notes.push(match said {
                Ok(()) => format!("guard: {} was edited, and its values are back in it", c.name),
                Err(why) => format!("guard: the edits to {} were not applied ({why}); the file is as it was", c.name),
            });
        }
        if let Some(c) = covers.first() {
            drop(std::fs::remove_dir_all(c.src.parent().unwrap_or(&c.src)));
        }
        notes
    }

    /// The mounts of the views of this task's secret files. A site `prepare` did not make (a later task that works there) gets
    /// them read-only.
    fn mounts(&self, site: &Site, row: &BoxRow) -> Result<Vec<Mount>, String> {
        if self.guard.is_none() {
            return Ok(Vec::new());
        }
        if !self.covers.lock().unwrap_or_else(|e| e.into_inner()).contains_key(&site.task) {
            self.cover(site, row, false)?;
        }
        let covers = self.covers.lock().unwrap_or_else(|e| e.into_inner());
        Ok(covers.get(&site.task).into_iter().flatten().flat_map(Cover::mounts).collect())
    }

    /// The wall, proven; or why there is none.
    fn wall(&self) -> Result<Arc<BoxRow>, String> {
        let mut wall = self.wall.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(*wall, Wall::Unproven) {
            let named: Vec<&BoxRow> = self.rows.iter().filter(|r| self.choice == "auto" || r.id == self.choice).collect();
            *wall = if named.is_empty() {
                Wall::None(if self.choice == "auto" { "no box is described (kind `box`)".into() } else { format!("`box.use` names `{}`, which is no box", self.choice) })
            } else {
                let mut why = Vec::new();
                match named.iter().find(|r| probe(r).map_err(|e| why.push(format!("{}: {e}", r.id))).is_ok()) {
                    Some(r) => Wall::Found(Arc::new((*r).clone())),
                    None => Wall::None(why.join("; ")),
                }
            };
        }
        match &*wall {
            Wall::Found(r) => Ok(r.clone()),
            Wall::None(why) => Err(why.clone()),
            Wall::Unproven => Err("no box".into()),
        }
    }
}

impl Place for Boxed {
    fn id(&self) -> &str {
        &self.id
    }

    fn prepare(&self, cx: &Prepare) -> Result<Site, Problem> {
        let mut site = match (self.wall(), self.unavailable) {
            (Ok(row), _) => {
                let mut site = self.inner.prepare(cx)?;
                site.notes.push(format!("{BEHIND}{}: {} is {}; the host is read-only, credentials and J4n's own folder are hidden", row.id, site.dir.display(), if site.read_only { "read-only" } else { "writable" }));
                site
            }
            (Err(why), Unavailable::Here) => {
                let mut site = self.inner.prepare(cx)?;
                site.notes.push(format!("no box can run here ({why}), so this task runs unboxed{}", if self.guard.is_some() { " and its secret files are not covered" } else { "" }));
                site
            }
            (Err(why), Unavailable::Refuse) => {
                return Err(Problem::refused("box", format!("no box can run here ({why}). Use the place `here`, or set `box.unavailable` to `here` to run unboxed")));
            }
        };
        site.read_only = cx.spec.tune.mode == j4n_core::task::Mode::Read;
        site.reads = cx.spec.reads.iter().map(PathBuf::from).collect();
        if let Ok(row) = self.wall() {
            let seen = Seen { own: std::iter::once(site.dir.clone()).chain(site.also.iter().cloned()).collect(), reads: &site.reads };
            site.notes.extend(hidden_in_goal(&row, &cx.spec.goal, &seen, &|p| p.exists()));
        }
        if let (Some(_), Ok(row)) = (&self.guard, self.wall()) {
            site.notes.extend(self.cover(&site, &row, !site.read_only).map_err(|why| Problem::refused("box", why))?);
        }
        Ok(site)
    }

    fn wrap(&self, site: &Site, launch: Launch) -> Launch {
        let launch = self.inner.wrap(site, launch);
        let Ok(row) = self.wall() else { return launch };
        let spec = Spec { dir: site.dir.clone(), also: site.also.clone(), read_only: site.read_only, login: Vec::new(), home: self.home.clone(), j4n_home: self.j4n_home.clone(), net: self.net && !launch.no_net, reads: site.reads.clone() };
        let fs = Fs { exists: &|p| p.exists(), is_dir: &|p| p.is_dir() };
        // a program that cannot be put behind the wall is not run outside it: what runs instead says why
        let refused = |why: String| Launch { argv: vec!["/bin/false".into(), format!("j4n: {why}")], ..launch.clone() };
        let mounts = match self.mounts(site, &row) {
            Ok(m) => m,
            Err(why) => return refused(why),
        };
        let wrapped = match argv_with(&row, &spec, &fs, launch.clone(), &mounts) {
            Ok(w) => w,
            Err(why) => return refused(why),
        };
        // the check after mounting, once for each command line: a reader inside the wall must get the view
        let prefix = &wrapped.argv[..wrapped.argv.len() - launch.argv.len()];
        let key = prefix.join("\0");
        if !mounts.is_empty() && !self.checked.lock().unwrap_or_else(|e| e.into_inner()).contains(&key) {
            if let Err(why) = view::verify(prefix, &mounts) {
                return refused(why);
            }
            self.checked.lock().unwrap_or_else(|e| e.into_inner()).insert(key);
        }
        wrapped
    }

    fn settle(&self, site: &Site) -> Settled {
        // before the inner place looks at what changed: a file written back is a change
        let notes = self.write_back(site);
        let mut settled = self.inner.settle(site);
        settled.notes.extend(notes);
        settled
    }

    fn site_of(&self, task: &str) -> Option<Site> {
        self.inner.site_of(task)
    }

    fn take(&self, task: &str, how: Take) -> Result<Taken, Problem> {
        self.inner.take(task, how)
    }

    fn share(&self, task: &str, rel: &str, from: &std::path::Path) -> Result<j4n_core::place::Shared, String> {
        self.inner.share(task, rel, from)
    }
}
