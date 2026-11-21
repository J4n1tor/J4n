//! Packages for the kit's kernel. The base is the standard library; `need("pandas")` in a cell
//! adds a package from the network with `uv`, into J4n's own folder and nowhere else: `<home>/py/env` for all tasks, or
//! `<home>/py/tmp/<task>` for one (`temp=True`), which goes with the task's kernel. The folder is read-only inside a box, and
//! the kernel gains it on `sys.path` in the same session.
//!
//! The install is an effect outside the task's folder, so the Gate is asked **before the cell runs** (`PyTools::gate`, from the cell's
//! literal `need("name")`): a host call mid-cell has nobody to ask. What is here is the parts that do not need the Gate: which
//! names a cell asks for, whether a package is already there, and the install itself.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Temp folders left by a J4n that ended badly are removed after this long.
const STALE: Duration = Duration::from_secs(3 * 24 * 3600);
/// One install at a time in this process: two `--target` runs into one folder would trample each other.
static ONE: Mutex<()> = Mutex::new(());

/// What `uv` reads as a file, not a name, when it ends one (`evil.whl`, `x.tar.gz`): such a name could be a path in the folder `uv` runs in.
const FILES: [&str; 17] = ["whl", "zip", "gz", "tar", "bz2", "xz", "egg", "tgz", "tbz", "tbz2", "txz", "tlz", "lz", "zst", "tzst", "z", "pyz"];

/// A distribution name and nothing else: no version, extra, URL, path or flag, and not a name that ends as an archive does.
pub fn valid(name: &str) -> bool {
    let b = name.as_bytes();
    (1..=100).contains(&b.len())
        && b[0].is_ascii_alphanumeric()
        && b[b.len() - 1].is_ascii_alphanumeric()
        && b.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_'))
        && !name.rsplit_once('.').is_some_and(|(_, last)| FILES.contains(&last.to_ascii_lowercase().as_str()))
}

/// PEP 503: lower case, every run of `-`, `_` and `.` one `-`.
pub fn canon(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_ascii_lowercase().chars() {
        if matches!(c, '-' | '_' | '.') {
            if !out.ends_with('-') {
                out.push('-');
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The packages a cell names with a literal call, `need("x")` or `j4n.need('x', temp=True)`, as (canonical name, temp). A name that is not a
/// literal is not found here; the call itself is then refused unless the package is already there.
pub fn named(code: &str) -> Vec<(String, bool)> {
    let (mut out, mut at) = (Vec::new(), 0);
    while let Some(i) = code[at..].find("need(") {
        let (start, end) = (at + i, at + i + 5);
        at = end;
        if code[..start].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let rest = code[end..].trim_start();
        let Some(q) = rest.chars().next().filter(|c| matches!(c, '"' | '\'')) else { continue };
        let Some(len) = rest[1..].find(q) else { continue };
        let (name, tail) = (&rest[1..1 + len], &rest[2 + len..]);
        let args: String = tail.split(')').next().unwrap_or_default().chars().filter(|c| !c.is_whitespace()).collect();
        if valid(name) {
            let found = (canon(name), args.contains("temp=True"));
            if !out.contains(&found) {
                out.push(found);
            }
        }
    }
    out
}

/// A task id as one safe folder name.
fn word(task: &str) -> String {
    if !task.is_empty() && task.len() <= 64 && task.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')) {
        return task.to_string();
    }
    format!("x{}", j4n_core::text::short_hash(task))
}

/// `<home>/py`, with `env` and `tmp` made: a box binds it before the first install, so the folders must be there when the wall goes up.
pub fn ready(home: &Path) -> Option<PathBuf> {
    let py = home.join("py");
    (!home.as_os_str().is_empty() && std::fs::create_dir_all(py.join("env")).and_then(|()| std::fs::create_dir_all(py.join("tmp"))).is_ok()).then_some(py)
}

/// The folder a package goes into.
pub fn dir_for(home: &Path, task: &str, temp: bool) -> PathBuf {
    if temp { home.join("py").join("tmp").join(word(task)) } else { home.join("py").join("env") }
}

/// Remove the temp folders no task of this process holds and nothing has touched for [`STALE`].
pub fn sweep(home: &Path) {
    let Ok(list) = std::fs::read_dir(home.join("py").join("tmp")) else { return };
    for e in list.flatten() {
        if e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > STALE) {
            drop(std::fs::remove_dir_all(e.path()));
        }
    }
}

/// Whether `name` is installed in `dir`: a `<name>-<version>.dist-info` folder.
pub fn present(dir: &Path, name: &str) -> bool {
    let want = canon(name);
    std::fs::read_dir(dir).into_iter().flatten().flatten().any(|e| {
        let file = e.file_name().to_string_lossy().into_owned();
        file.strip_suffix(".dist-info").and_then(|s| s.split_once('-')).is_some_and(|(n, _)| canon(n) == want)
    })
}

/// Where `uv` is: a path that is a file, or a name found on the `PATH`.
pub fn find(uv: &str) -> Option<PathBuf> {
    if uv.contains('/') {
        return Some(PathBuf::from(uv)).filter(|p| p.is_file());
    }
    std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(uv)).find(|f| f.is_file()))
}

fn size_of(dir: &Path) -> u64 {
    let mut total = 0;
    let mut todo = vec![dir.to_path_buf()];
    let mut seen = HashSet::new();
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() && seen.insert(e.path()) {
                todo.push(e.path());
            } else if m.is_file() {
                total += m.len();
            }
        }
    }
    total
}

/// A size in words: `31.4 MB`.
pub fn bytes(n: u64) -> String {
    match n {
        0..=1023 => format!("{n} B"),
        1024..=1_048_575 => format!("{:.1} KB", n as f64 / 1024.0),
        _ => format!("{:.1} MB", n as f64 / 1_048_576.0),
    }
}

/// What the install changed: the folder grew by this many bytes.
pub struct Installed {
    pub size: u64,
}

/// `uv pip install --target <dir> --only-binary :all: <name>`, run by J4n and not in a box (it needs the network and writes outside the
/// task). Wheels only, so no build script of a package runs here; its code runs only when imported, inside the kernel's wall. `uv` gets a
/// bare environment: the `PATH`, the home and its cache, none of J4n's keys. Stopped at `until`.
pub fn install(uv: &Path, dir: &Path, name: &str, until: Instant) -> Result<Installed, String> {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let before = size_of(dir);
    let mut cmd = Command::new(uv);
    crate::proc::scrub(&mut cmd);
    cmd.args(["pip", "install", "--quiet", "--no-progress", "--color", "never", "--python", "python3", "--only-binary", ":all:", "--target"]).arg(dir).args(["--", name]);
    // never the project's folder, where a name could be a file a cell wrote: `uv` runs in the folder it installs into
    cmd.env_clear().current_dir(dir).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    for key in ["PATH", "HOME", "XDG_CACHE_HOME", "UV_CACHE_DIR"] {
        if let Some(v) = std::env::var_os(key) {
            cmd.env(key, v);
        }
    }
    let mut child = cmd.spawn().map_err(|e| format!("uv could not be run: {e}"))?;
    // drained while it runs: a pipe that fills would stall `uv` until the time is over; the end of what it said is kept for the message
    let said = child.stderr.take().map(|mut e| {
        std::thread::spawn(move || {
            let (mut tail, mut buf) = (Vec::<u8>::new(), [0u8; 4096]);
            while let Ok(n) = std::io::Read::read(&mut e, &mut buf) {
                if n == 0 {
                    break;
                }
                tail.extend_from_slice(&buf[..n]);
                if tail.len() > 8192 {
                    tail.drain(..tail.len() - 4096);
                }
            }
            String::from_utf8_lossy(&tail).into_owned()
        })
    });
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if Instant::now() >= until => {
                drop(child.kill());
                drop(child.wait());
                return Err(format!("uv ran past the cell's time and was stopped; `{name}` may be half installed, call need again with more `timeout`"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(e) => return Err(format!("uv could not be waited for: {e}")),
        }
    };
    if !status.success() {
        let said = said.and_then(|t| t.join().ok()).unwrap_or_default();
        let said = said.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
        return Err(format!("uv could not install `{name}`{}", if said.is_empty() { String::new() } else { format!(": {}", said.chars().skip(said.chars().count().saturating_sub(300)).collect::<String>()) }));
    }
    if !present(dir, name) {
        return Err(format!("uv ran, but `{name}` is not in {} afterwards (the name may be an extra or a package with no wheel)", dir.display()));
    }
    Ok(Installed { size: size_of(dir).saturating_sub(before) })
}
