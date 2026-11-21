//! What is checked: the work of a task (its changed files against the commit they were at before it began), a path as it stands
//! (every line counts as added), or a git range (its diff). Read with `git` and the folder, never with a model.

use crate::diff::{self, FileDiff};
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

/// The most files a change holds, and the largest file read whole.
pub const MAX_FILES: usize = 300;
const MAX_BYTES: u64 = 1_000_000;
const SKIP: [&str; 6] = ["target", "node_modules", ".venv", "__pycache__", ".j4n", ".git"];

pub struct Change {
    pub dir: PathBuf,
    pub files: Vec<FileDiff>,
    /// The commit the files are compared with; what `before` reads.
    base: Option<String>,
    /// Files left out because there were too many.
    pub cut: usize,
}

/// What `git <args>` prints in `dir`, when it ends well.
pub fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = j4n_core::host::keyless(&mut Command::new("git")).arg("-C").arg(dir).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A path as the operator may name it: relative, inside the folder.
pub fn inside(path: &str) -> bool {
    !path.is_empty() && Path::new(path).components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

pub(crate) fn text_of(dir: &Path, rel: &str) -> Option<String> {
    let full = dir.join(rel);
    let meta = std::fs::metadata(&full).ok()?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return None;
    }
    String::from_utf8(std::fs::read(&full).ok()?).ok().filter(|t| !t.contains('\0'))
}

fn tracked(dir: &Path, rel: &str) -> bool {
    git(dir, &["ls-files", "--error-unmatch", "--", rel]).is_some()
}

impl Change {
    fn new(dir: &Path, base: Option<String>) -> Change {
        Change { dir: dir.to_path_buf(), files: Vec::new(), base, cut: 0 }
    }

    fn push(&mut self, f: FileDiff) {
        if self.files.len() >= MAX_FILES {
            self.cut += 1;
        } else {
            self.files.push(f);
        }
    }

    /// The work of a task: `paths` against `base` (the commit its checkpoint kept, else `HEAD`). A file git does not know yet counts
    /// whole; with no git at all, every file does.
    pub fn of_task(dir: &Path, base: Option<String>, paths: &[String]) -> Change {
        let in_git = git(dir, &["rev-parse", "--git-dir"]).is_some();
        let base = base.or_else(|| in_git.then(|| "HEAD".to_string()));
        let mut c = Change::new(dir, base.clone());
        for rel in paths.iter().filter(|p| inside(p)) {
            match &base {
                Some(b) if in_git && tracked(dir, rel) => {
                    let text = git(dir, &["diff", "--no-color", "--no-ext-diff", "--src-prefix=a/", "--dst-prefix=b/", "-U3", b, "--", rel]).unwrap_or_default();
                    c.files.extend(diff::parse(&text));
                }
                _ => {
                    if let Some(t) = text_of(dir, rel) {
                        c.push(diff::whole(rel, &t));
                    }
                }
            }
        }
        c.files.truncate(MAX_FILES);
        c
    }

    /// A path as it stands: a file, or every text file under a folder.
    pub fn of_path(dir: &Path, rel: &str) -> Result<Change, String> {
        let rel = rel.trim_end_matches('/');
        let rel = if rel.is_empty() { "." } else { rel };
        if rel != "." && !inside(rel) {
            return Err(format!("`{rel}` is not inside this folder"));
        }
        let full = dir.join(rel);
        if !full.exists() {
            return Err(format!("`{rel}` is not here"));
        }
        let mut names: BTreeSet<String> = BTreeSet::new();
        if full.is_file() {
            names.insert(rel.to_string());
        } else if let Some(listed) = git(dir, &["ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", rel]) {
            names.extend(listed.split('\0').filter(|p| !p.is_empty()).map(String::from));
        } else {
            walk(dir, &full, &mut names);
        }
        let mut c = Change::new(dir, None);
        for rel in names {
            if let Some(t) = text_of(dir, &rel) {
                c.push(diff::whole(&rel, &t));
            }
        }
        Ok(c)
    }

    /// Every file of the folder git knows or would add, else the capped walk: the names `keep` looks in.
    pub fn listing(dir: &Path) -> Vec<String> {
        let mut names: BTreeSet<String> = BTreeSet::new();
        match git(dir, &["ls-files", "-z", "--cached", "--others", "--exclude-standard"]) {
            Some(listed) => names.extend(listed.split('\0').filter(|p| !p.is_empty()).map(String::from)),
            None => walk(dir, dir, &mut names),
        }
        names.into_iter().collect()
    }

    /// A git range (`HEAD~2..HEAD`) or one commit-ish compared with the folder.
    pub fn of_range(dir: &Path, range: &str) -> Result<Change, String> {
        if range.starts_with('-') {
            return Err(format!("`{range}` is not a range"));
        }
        let text = git(dir, &["diff", "--no-color", "--no-ext-diff", "--src-prefix=a/", "--dst-prefix=b/", "-U3", range, "--"]).ok_or_else(|| format!("`{range}` is not a range git can show here"))?;
        let base = range.split_once("..").map_or(range, |(a, _)| a);
        let mut c = Change::new(dir, Some(base.to_string()));
        for f in diff::parse(&text) {
            c.push(f);
        }
        Ok(c)
    }

    /// The file as it was before.
    pub fn before(&self, rel: &str) -> Option<String> {
        let base = self.base.as_ref()?;
        git(&self.dir, &["show", &format!("{base}:{rel}")])
    }

    /// The file as it stands.
    pub fn after(&self, rel: &str) -> Option<String> {
        text_of(&self.dir, rel)
    }

    pub fn added(&self) -> usize {
        self.files.iter().map(|f| f.added().count()).sum()
    }
}

fn walk(root: &Path, dir: &Path, out: &mut BTreeSet<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let path = e.path();
        if path.is_dir() {
            if !SKIP.contains(&name.as_str()) {
                walk(root, &path, out);
            }
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.insert(rel.to_string_lossy().to_string());
        }
        if out.len() > MAX_FILES * 4 {
            return;
        }
    }
}
