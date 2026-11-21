//! The view of a secret file: a file the name globs pick and the redactor changes is not shown to a
//! program behind the wall, its view is — the same text with each secret replaced by the placeholder that says what it was. The
//! view is a copy kept in J4n's home and mounted over the file, so every reader (`cat`, `grep -r`, a language's `open`, the
//! program's own tools) gets it; a program that may write gets a writable copy, and what it wrote is put back into the real file
//! at the end ([`merge`]) when no placeholder was touched.

use j4n_core::redact::{Redactor, Span};
use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Folders the search does not enter, how deep it goes and how many entries it looks at.
const SKIP: [&str; 3] = [".git", "node_modules", "target"];
const DEPTH: usize = 8;
const ENTRIES: usize = 50_000;
/// A file over this, or with a NUL in its first `HEAD` bytes, or that is not UTF-8, is not covered.
const MAX_BYTES: u64 = 1 << 20;
const HEAD: usize = 4096;

/// One mount of a view over a path of the real world.
#[derive(Debug, Clone)]
pub struct Mount {
    /// The copy of the view, in J4n's home.
    pub src: PathBuf,
    /// Where the real file is.
    pub at: PathBuf,
    pub writable: bool,
    /// What the copy holds: what a reader at `at` must get.
    pub view: String,
}

/// A secret file (all the names of one inode) with what it held, what is shown of it, and where the copy is.
#[derive(Debug, Clone)]
pub struct Cover {
    pub name: String,
    pub canon: PathBuf,
    pub paths: Vec<PathBuf>,
    pub src: PathBuf,
    pub writable: bool,
    pub real: String,
    pub view: String,
    pub spans: Vec<Span>,
}

impl Cover {
    pub fn mounts(&self) -> impl Iterator<Item = Mount> + '_ {
        self.paths.iter().map(|p| Mount { src: self.src.clone(), at: p.clone(), writable: self.writable, view: self.view.clone() })
    }

    /// The lines of the view that hold a placeholder.
    fn lines(&self) -> Vec<usize> {
        let mut lines: Vec<usize> = self.spans.iter().map(|s| self.view[..s.at.start].matches('\n').count() + 1).collect();
        lines.dedup();
        lines
    }
}

/// `*` is any run of characters and `?` one, in a file's name.
pub fn glob(pattern: &str, name: &str) -> bool {
    fn go(p: &[u8], n: &[u8]) -> bool {
        match p.split_first() {
            None => n.is_empty(),
            Some((b'*', rest)) => (0..=n.len()).any(|i| go(rest, &n[i..])),
            Some((b'?', rest)) => !n.is_empty() && go(rest, &n[1..]),
            Some((c, rest)) => n.first() == Some(c) && go(rest, &n[1..]),
        }
    }
    go(pattern.as_bytes(), name.as_bytes())
}

/// The files under `dir` whose name a glob picks (a symlink by its target), and every name of a file with more than one.
/// The last is true when it stopped at [`ENTRIES`].
/// A file's name as found and its canonical path; a (device, inode) and one of the names of that file.
type Picked = Vec<(PathBuf, PathBuf)>;
type Inodes = Vec<((u64, u64), PathBuf)>;

fn walk(dir: &Path, globs: &[String]) -> (Picked, Inodes, bool) {
    let (mut picked, mut multi, mut seen, mut stack) = (Vec::new(), Vec::new(), 0, vec![(dir.to_path_buf(), 0)]);
    while let Some((d, depth)) = stack.pop() {
        for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            seen += 1;
            if seen > ENTRIES {
                return (picked, multi, true);
            }
            let (path, name, Ok(kind)) = (entry.path(), entry.file_name().to_string_lossy().into_owned(), entry.file_type()) else { continue };
            let wanted = globs.iter().any(|g| glob(g, &name));
            if kind.is_dir() && depth < DEPTH && !SKIP.contains(&name.as_str()) {
                stack.push((path, depth + 1));
            } else if kind.is_symlink() && wanted {
                if let Some(real) = std::fs::canonicalize(&path).ok().filter(|p| p.is_file()) {
                    picked.push((path, real));
                }
            } else if kind.is_file() {
                if let Some(m) = entry.metadata().ok().filter(|m| m.nlink() > 1) {
                    multi.push(((m.dev(), m.ino()), path.clone()));
                }
                if wanted {
                    picked.push((path.clone(), path));
                }
            }
        }
    }
    (picked, multi, false)
}

/// The secret files under `dir` and their views, written to `keep`; and the globbed files that were left alone and why. A file
/// is covered only when its view differs from it.
pub fn find(red: &Redactor, globs: &[String], dir: &Path, keep: &Path, writable: bool) -> Result<(Vec<Cover>, Vec<String>), String> {
    let (picked, multi, cut) = walk(dir, globs);
    let mut left: Vec<String> = cut.then(|| format!("the search stopped after {ENTRIES} entries")).into_iter().collect();
    let mut groups: BTreeMap<(u64, u64), (PathBuf, PathBuf, BTreeSet<PathBuf>)> = BTreeMap::new();
    for (seen, canon) in picked {
        if let Ok(m) = std::fs::metadata(&canon) {
            groups.entry((m.dev(), m.ino())).or_insert_with(|| (seen, canon.clone(), BTreeSet::new())).2.insert(canon);
        }
    }
    for (key, path) in multi {
        if let Some(g) = groups.get_mut(&key) {
            g.2.insert(path);
        }
    }
    let mut covers = Vec::new();
    for (seen, canon, paths) in groups.into_values() {
        let name = seen.strip_prefix(dir).unwrap_or(&seen).display().to_string();
        let bytes = std::fs::metadata(&canon).ok().filter(|m| m.len() <= MAX_BYTES).and_then(|_| std::fs::read(&canon).ok());
        let Some(real) = bytes.filter(|b| !b[..b.len().min(HEAD)].contains(&0)).and_then(|b| String::from_utf8(b).ok()) else {
            left.push(format!("{name} (binary, large or unreadable)"));
            continue;
        };
        let (view, spans) = red.view(&real, &seen.to_string_lossy());
        if view == real {
            continue;
        }
        let src = keep.join(covers.len().to_string());
        let wrote = std::fs::create_dir_all(keep).and_then(|()| std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&src)).and_then(|mut f| std::io::Write::write_all(&mut f, view.as_bytes()));
        wrote.map_err(|e| format!("the view of {name} cannot be kept in {}: {e}", keep.display()))?;
        covers.push(Cover { name, canon, paths: paths.into_iter().collect(), src, writable, real, view, spans });
    }
    Ok((covers, left))
}

/// What the notes of a task say about its covers.
pub fn notes(covers: &[Cover], left: &[String]) -> Vec<String> {
    let mut notes = Vec::new();
    if !covers.is_empty() {
        let shown: Vec<String> = covers.iter().take(3).map(|c| match c.lines().as_slice() {
            [] => c.name.clone(),
            l => format!("{} (line {})", c.name, l.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")),
        }).collect();
        let more = covers.len().saturating_sub(3);
        notes.push(format!("guard: {} secret file{} shown as their view: {}{}", covers.len(), if covers.len() == 1 { " is" } else { "s are" }, shown.join(", "), if more > 0 { format!(" and {more} more") } else { String::new() }));
    }
    if !left.is_empty() {
        notes.push(format!("guard: not covered: {}", left.join(", ")));
    }
    notes
}

/// The check after mounting: the wall (`prefix`, everything up to the program) is started over a program that prints each path
/// and a NUL, and what it prints must be the view. Anything else — a mount that did not take, a later one that covered it, a
/// wall that will not run — is the reason the program is not started.
pub fn verify(prefix: &[String], mounts: &[Mount]) -> Result<(), String> {
    let script = "for f; do cat -- \"$f\"; printf '\\0'; done";
    let Some((cmd, args)) = prefix.split_first() else { return Err("there is no wall to ask".into()) };
    let out = j4n_core::host::keyless(&mut Command::new(cmd)).args(args).args(["/bin/sh", "-c", script, "sh"]).args(mounts.iter().map(|m| &m.at)).stdin(Stdio::null()).stderr(Stdio::null()).output().map_err(|e| format!("the wall cannot be asked: {e}"))?;
    let read: Vec<&[u8]> = out.stdout.split(|b| *b == 0).collect();
    match mounts.iter().enumerate().find(|(i, m)| read.get(*i).is_none_or(|r| *r != m.view.as_bytes())) {
        None => Ok(()),
        Some((_, m)) => Err(format!("a reader inside the wall does not get the view of {}", m.at.display())),
    }
}

/// What a mark of the view (a placeholder, or the `«secret:NAME»` of a registered value) hid.
struct Hidden {
    at: std::ops::Range<usize>,
    real: String,
}

/// The length of the ` (N lines)` the view puts after a private key's placeholder.
fn lines_len(s: &str) -> usize {
    let digits = s.strip_prefix(" (").map_or(0, |r| r.bytes().take_while(u8::is_ascii_digit).count());
    if digits > 0 && s[2 + digits..].starts_with(" lines)") { 2 + digits + 7 } else { 0 }
}

/// Walk the real text and the view together: the text between two marks is the same in both, a span hid the next
/// `length` characters, a registered marker hid what lies up to the next plain text. `Err` when they do not walk together.
fn hidden(real: &str, view: &str, spans: &[Span]) -> Result<Vec<Hidden>, String> {
    let mut marks: Vec<(std::ops::Range<usize>, Option<usize>)> = spans.iter().map(|s| (s.at.start..s.at.end + if s.kind == "private_key" { lines_len(&view[s.at.end..]) } else { 0 }, Some(s.length))).collect();
    let mut from = 0;
    while let Some(i) = view[from..].find("«secret:") {
        let start = from + i;
        let Some(end) = view[start..].find('»') else { break };
        marks.push((start..start + end + '»'.len_utf8(), None));
        from = start + end + '»'.len_utf8();
    }
    marks.sort_by_key(|m| m.0.start);
    let (mut v, mut r, mut out) = (0, 0, Vec::new());
    for (n, (at, length)) in marks.iter().enumerate() {
        let plain = view.get(v..at.start).ok_or("two marks overlap")?;
        if !real[r..].starts_with(plain) {
            return Err("the text between two marks is not the same in the file and in its view".into());
        }
        r += plain.len();
        let end = match length {
            Some(l) => real[r..].char_indices().nth(*l).map_or(real.len(), |(i, _)| r + i),
            None => {
                let tail = &view[at.end..marks.get(n + 1).map_or(view.len(), |m| m.0.start)];
                match (marks.get(n + 1), tail) {
                    (Some(_), "") => return Err("two registered values stand side by side".into()),
                    (Some(_), _) => real[r..].find(tail).map(|i| r + i),
                    (None, _) => real[r..].strip_suffix(tail).map(|rest| r + rest.len()),
                }
                .ok_or("the end of a registered value is not found")?
            }
        };
        out.push(Hidden { at: at.clone(), real: real[r..end].to_string() });
        (v, r) = (at.end, end);
    }
    (view[v..] == real[r..]).then_some(out).ok_or_else(|| "the end of the file is not the same in the file and in its view".into())
}

/// The real file after a program edited its view: every mark of the view stands in the edited text, unchanged and in order, and
/// is put back as the value it hid; what else the program wrote stays. `Err` says why the edit is not applied.
pub fn merge(c: &Cover, edited: &str) -> Result<String, String> {
    let marks = hidden(&c.real, &c.view, &c.spans)?;
    let count = |s: &str| s.matches("«REDACTED ").count() + s.matches("«secret:").count();
    if count(edited) != count(&c.view) {
        return Err("a placeholder was added or taken away".into());
    }
    let (mut out, mut cur) = (String::with_capacity(c.real.len()), 0);
    for m in &marks {
        let placeholder = &c.view[m.at.clone()];
        let i = edited[cur..].find(placeholder).ok_or_else(|| format!("the placeholder `{}` was changed or moved", placeholder.chars().take(40).collect::<String>()))?;
        out.push_str(&edited[cur..cur + i]);
        out.push_str(&m.real);
        cur += i + placeholder.len();
    }
    out.push_str(&edited[cur..]);
    Ok(out)
}
