//! `j4n py` for an agent that has a shell of its own: the cells of a Claude, a pi or a codex doer reach
//! the task's kernel, the one the own agent's tool `py` uses, so both share its variables.
//!
//! - **The socket is the task's.** While a CLI doer runs, J4n listens on `<task folder>/.j4n/run/<task>.sock` (the folder is 0700, the
//!   socket 0600) and keeps a key, 128 random bits, in `<task>.key` (0600) beside it. A request names the task and gives the key; one that
//!   names another task or gives another key is refused before anything is read from it. The files go when the run is dropped, and a
//!   socket nobody listens on is swept at the next start.
//! - **A cell is a model's cell.** It goes through [`PyTools`] as the tool `py` does: one Gate call (`pyread` decides `read`, else
//!   `exec`), the same kernel in the same walls, the same `py.cell` event (with `via` = `j4n py`) and the same host calls (`kithost`).
//!   What the Gate asks or blocks is refused with its question: a shell has no one to answer it (a grant answers it ahead).
//! - **The doer is told** ([`Kitted`]): any doer that runs a program of its own is wrapped, and its first text of a session ends with the
//!   card `packs/kit/prompts/card-cli.txt`. Doers without a shell (models, HTTP) are given the tool `py` instead. The wrapper has a
//!   second, separate part: the text of an answer turn ends with the one sentence of `brief.plain_answer` when the core marked the turn, kit or no kit.
//! - **The doer finds `j4n`**: while the socket is open, the doer's PATH starts with `<task>.bin` beside the socket, a folder
//!   holding one link, `j4n`, to this J4n's own binary, so no install is needed and no other `j4n` (another version, another protocol)
//!   wins; and its environment says `J4N_IN_TASK=<the task's .j4n folder, canonical>`, which the binary reads to refuse the commands that
//!   would open that same Log, and no other.
//! - **The client** ([`ask`]) is what the command `j4n py` does: find the folder's socket, give the key, print the answer.

use crate::kithost;
use crate::model::Call;
use crate::model::doer::Tools;
use crate::py::{self, PyTools};
use j4n_core::doer::{Caps, Doer, Emit, Probe, Run, RunCtl, RunEv, StartCx};
use j4n_core::gate::ReadOnly;
use j4n_core::place::{Launch, Site};
use j4n_core::state;
use j4n_core::table::Problem;
use j4n_core::task::Mode;
use serde_json::{Value as Json, json};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// What an agent that has the socket is told (`packs/kit/prompts/card-cli.txt`; law 14 reads that file).
pub const CARD: &str = include_str!("../../../packs/kit/prompts/card-cli.txt");
/// Where the sockets of a folder's tasks are.
pub const RUN: &str = ".j4n/run";
const MAX_REQUEST: u64 = 4_000_000;
/// A socket path is at most 108 bytes; a longer one is reached through the folder's descriptor.
const SHORT: usize = 100;

fn fresh() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes)).map_err(|e| format!("no key could be made: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// A path to `name` in `dir` that a socket can name, and the descriptor that keeps it good while it is used.
fn reach(dir: &Path, name: &str) -> std::io::Result<(PathBuf, Option<File>)> {
    let direct = dir.join(name);
    if direct.as_os_str().len() < SHORT {
        return Ok((direct, None));
    }
    let held = File::open(dir)?;
    Ok((PathBuf::from(format!("/proc/self/fd/{}/{name}", held.as_raw_fd())), Some(held)))
}

fn word(task: &str) -> bool {
    !task.is_empty() && task.len() <= 64 && task.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// What a task's socket answers with: the task, its key, and the place its cells run in.
struct Serving {
    py: Arc<PyTools>,
    task: String,
    key: String,
    site: Site,
    read_only: ReadOnly,
    /// The kernel's launch as the place's walls make it (made once, when the run began).
    ready: Launch,
    /// The place's walls, for a program a tool of the bridge runs: a copy of the run's start that outlives it.
    walls: StartCx,
    out: Emit,
}

impl Serving {
    fn answer(&self, line: &str) -> Json {
        let refuse = |why: &str| json!({ "ok": false, "error": why });
        let Ok(req) = serde_json::from_str::<Json>(line) else { return refuse("that is not a request") };
        if req["task"].as_str() != Some(&self.task) || req["key"].as_str() != Some(&self.key) {
            return refuse("this socket is another task's: the task and its key must be the ones it was opened with");
        }
        let code = req["code"].as_str().unwrap_or_default();
        let call = Call { id: "j4n-py".into(), name: "py".into(), args: json!({ "code": code, "timeout": req["timeout"] }), bad: None };
        if let Some(held) = self.py.gate(&call, &self.task, &self.site, self.read_only) {
            return refuse(&format!("not run: the Gate does not let this through ({}), and a shell has no one to answer it; grant it ahead", held.question));
        }
        let kernel = self.py.kernel(&self.site);
        let out = self.py.run(&call, &self.site, &|l| if l == kernel { self.ready.clone() } else { self.walls.wrap(l) });
        if !out.changed.is_empty() {
            self.out.send(RunEv::Changed(out.changed.clone()));
        }
        if let Some(mut cell) = out.py {
            cell["via"] = json!("j4n py");
            if let Some(m) = kithost::spend(&cell) {
                self.out.send(RunEv::Metrics(m));
            }
            self.out.send(RunEv::Py(cell));
        }
        json!({ "ok": !out.error, "text": out.text })
    }

    fn handle(&self, stream: UnixStream) {
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
        let mut line = String::new();
        if BufReader::new((&stream).take(MAX_REQUEST)).read_line(&mut line).is_err() {
            return;
        }
        let reply = self.answer(&line);
        let _ = writeln!(&stream, "{reply}");
    }
}

/// Set in the environment of a doer while its task has a socket, to the `.j4n` folder of the task (canonical): a command run in the folder
/// whose `.j4n` is that one would open the Log the running J4n holds; a command in any other folder opens its own.
pub const IN_TASK: &str = "J4N_IN_TASK";

/// An open socket; the files go with it.
pub struct Socket {
    dir: PathBuf,
    task: String,
    /// The folder with the link `j4n` the doer's PATH starts with.
    bin: PathBuf,
    stop: Arc<AtomicBool>,
}

impl Drop for Socket {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for ext in ["sock", "key"] {
            let _ = fs::remove_file(self.dir.join(format!("{}.{ext}", self.task)));
        }
        let _ = fs::remove_dir_all(&self.bin);
    }
}

/// The sockets nobody listens on, left by a J4n that ended badly.
fn sweep(dir: &Path) {
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = e.path();
        if path.extension().is_some_and(|x| x == "sock") && UnixStream::connect(&path).is_err() {
            let _ = fs::remove_file(&path);
            let _ = fs::remove_file(path.with_extension("key"));
            // its binary too, a copy of the build when the build was gone
            let _ = fs::remove_dir_all(path.with_extension("bin"));
        }
    }
}

/// A folder of the task's with one file in it, `j4n`, the binary that is running: a hard link to the file it was started from, so a build
/// that replaces or removes that file while the task runs leaves this one as it was. Where a hard link cannot be made (another filesystem,
/// a binary the user does not own), a symbolic one: a copy of the whole binary for every task would cost more than the rebuild it guards.
/// When that file is gone already, a copy of the process's own image, which no other process may read once J4n is not dumpable. The folder, not the binary's own, so nothing else that sits beside the binary is exposed.
fn link_bin(dir: &Path, task: &str) -> Result<PathBuf, String> {
    let bin = dir.join(format!("{task}.bin"));
    let _ = fs::remove_dir_all(&bin);
    let to = bin.join("j4n");
    let made = state::private_dir(&bin).and_then(|()| match std::env::current_exe().ok().filter(|p| p.is_file()) {
        Some(exe) => fs::hard_link(&exe, &to).or_else(|_| std::os::unix::fs::symlink(&exe, &to)),
        None => copy_bin(Path::new("/proc/self/exe"), &to),
    });
    made.map_err(|e| format!("{}: {e}", bin.display()))?;
    Ok(bin)
}

fn copy_bin(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::copy(from, to)?;
    fs::set_permissions(to, fs::Permissions::from_mode(0o700))
}

fn open(py: &Arc<PyTools>, cx: &StartCx) -> Result<Socket, String> {
    let (task, site) = (cx.task.clone(), cx.site.clone());
    if !word(&task) {
        return Err(format!("`{task}` cannot name a socket"));
    }
    let dir = site.dir.join(RUN);
    state::ensure_root(&site.dir.join(".j4n"), true).and_then(|()| state::private_dir(&dir)).map_err(|e| format!("{}: {e}", dir.display()))?;
    sweep(&dir);
    let key = fresh()?;
    let key_file = dir.join(format!("{task}.key"));
    drop(fs::remove_file(&key_file));
    // made 0600, never narrowed after: no moment exists in which it is readable by others
    let made = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&key_file);
    made.and_then(|mut f| f.write_all(key.as_bytes())).map_err(|e| format!("{}: {e}", key_file.display()))?;
    let name = format!("{task}.sock");
    drop(fs::remove_file(dir.join(&name)));
    let (path, held) = reach(&dir, &name).map_err(|e| e.to_string())?;
    let listener = UnixListener::bind(&path).map_err(|e| format!("{}: {e}", dir.join(&name).display()))?;
    drop(held);
    fs::set_permissions(dir.join(&name), fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let read_only = if cx.spec.tune.mode == Mode::Read { ReadOnly::Explicit } else { ReadOnly::No };
    let serving = Arc::new(Serving { py: py.clone(), task: task.clone(), key, ready: cx.wrap(py.kernel(&site)), walls: cx.nested("", cx.out.clone()), site, read_only, out: cx.out.clone() });
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::spawn(move || {
        while !flag.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let me = serving.clone();
                    std::thread::spawn(move || me.handle(stream));
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(20)),
                Err(_) => break,
            }
        }
    });
    let bin = link_bin(&dir, &task)?;
    Ok(Socket { dir, task, bin, stop })
}

/// One request to the socket of `name` in `dir`, as the client of any task would send it.
pub fn send(dir: &Path, name: &str, req: &Json, wait: Duration) -> Result<Json, String> {
    if !word(name) {
        return Err(format!("`{name}` is not a task"));
    }
    let (path, held) = reach(dir, &format!("{name}.sock")).map_err(|e| e.to_string())?;
    let stream = UnixStream::connect(&path).map_err(|e| format!("task {name} has no kernel socket here ({e})"))?;
    drop(held);
    stream.set_read_timeout(Some(wait)).map_err(|e| e.to_string())?;
    writeln!(&stream, "{req}").map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line).map_err(|e| format!("no answer: {e}"))?;
    serde_json::from_str(&line).map_err(|_| "the answer could not be read".into())
}

/// `j4n py`: run `code` in the kernel of the task whose socket is in `from` or above it. `task` names it when the folder has several.
/// `(ok, text)`: whether the cell ran without an error, and what to print.
pub fn ask(from: &Path, task: Option<&str>, code: &str, timeout: Option<u64>) -> Result<(bool, String), String> {
    let dir = from.ancestors().map(|a| a.join(RUN)).find(|d| d.is_dir()).ok_or("no task here has a kernel: j4n py works inside a task of a doer that J4n started, while the setting kit.mode is on")?;
    let mut tasks: Vec<String> = fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten().filter_map(|e| e.file_name().to_str()?.strip_suffix(".sock").map(String::from)).collect();
    tasks.sort();
    let task = match (task, tasks.as_slice()) {
        (Some(t), _) => t.to_string(),
        (None, [one]) => one.clone(),
        (None, []) => return Err("no task here has a kernel socket open".into()),
        (None, many) => return Err(format!("more than one task has a kernel here ({}): say which with --task", many.join(", "))),
    };
    let key = fs::read_to_string(dir.join(format!("{task}.key"))).map_err(|e| format!("the key of task {task} cannot be read: {e}"))?;
    let secs = timeout.unwrap_or(60);
    let a = send(&dir, &task, &json!({ "task": task, "key": key.trim(), "code": code, "timeout": secs }), Duration::from_secs(secs.min(600) + 60))?;
    match a["ok"].as_bool() {
        Some(ok) if a["text"].is_string() => Ok((ok, a["text"].as_str().unwrap_or_default().to_string())),
        _ => Err(a["error"].as_str().unwrap_or("the answer was not one").to_string()),
    }
}

/// A doer that runs a program of its own, told about the kit and given the task's socket while it runs.
pub struct Kitted {
    inner: Arc<dyn Doer>,
    /// The kit; none: the wrapper only carries the sentence of `brief.plain_answer` (the kit is off).
    py: Option<Arc<PyTools>>,
    /// Whether the doer has a shell (the socket and the card of `j4n py`); a model has the tool `py` and its card is in its system text.
    socket: bool,
}

impl Kitted {
    pub fn new(inner: Arc<dyn Doer>, py: Arc<PyTools>) -> Kitted {
        Kitted { inner, py: Some(py), socket: true }
    }

    /// A doer that runs a program, with no kit: its answer turns can still be asked for words.
    pub fn plain(inner: Arc<dyn Doer>) -> Kitted {
        Kitted { inner, py: None, socket: false }
    }

    /// A model that is given the tool `py`: told nothing but the offer line, after the goal of its first text.
    pub fn offering(inner: Arc<dyn Doer>, py: Arc<PyTools>) -> Kitted {
        Kitted { inner, py: Some(py), socket: false }
    }
}

struct Kept {
    run: Box<dyn Run>,
    _socket: Option<Socket>,
}

impl Run for Kept {
    fn ctl(&mut self, ctl: RunCtl) -> Result<(), Problem> {
        self.run.ctl(ctl)
    }
}

impl Doer for Kitted {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn probe(&self) -> Probe {
        self.inner.probe()
    }

    fn caps(&self) -> Caps {
        self.inner.caps()
    }

    fn shutdown(&self) {
        self.inner.shutdown();
    }

    fn start(&self, mut cx: StartCx) -> Result<Box<dyn Run>, Problem> {
        let socket = match self.py.as_ref().filter(|_| self.socket).map(|py| open(py, &cx)) {
            Some(Ok(s)) => Some(s),
            Some(Err(why)) => {
                cx.out.send(RunEv::Notice(format!("j4n py is not there for this run: {why}")));
                None
            }
            None => None,
        };
        // the doer's shell finds this run's own `j4n` first and knows it is inside the task
        if let Some(s) = &socket {
            let log = cx.site.dir.join(".j4n");
            let (bin, log, kernel) = (s.bin.display().to_string(), log.canonicalize().unwrap_or(log).display().to_string(), self.py.as_ref().map_or_else(|| py::kernel_launch(&cx.site), |p| p.kernel(&cx.site)));
            cx = cx.also(move |mut launch| {
                if launch == kernel {
                    return launch; // the task's kernel is one process for the own agent and the shell: its launch must not differ
                }
                let path = launch.env.get("PATH").cloned().or_else(|| std::env::var("PATH").ok()).filter(|p| !p.is_empty());
                launch.env.insert("PATH".into(), path.map_or(bin.clone(), |p| format!("{bin}:{p}")));
                launch.env.insert(IN_TASK.into(), log.clone());
                launch
            });
        }
        // the first words of a session carry the offer line and the card; a later turn of it has them already
        if let Some(py) = &self.py
            && (socket.is_some() || !self.socket)
            && cx.resume.is_none()
            && !cx.compacting
        {
            let text = cx.text.take().unwrap_or_else(|| cx.spec.goal.clone());
            let offer = crate::kitoffer::line_with(&text, &cx.site.dir, &py.offered(), py.off());
            if let Some(line) = &offer {
                cx.out.send(RunEv::Notice(format!("kit: {line}")));
            }
            let card = if self.socket { Some(py.card(CARD)) } else { None };
            cx.text = Some([Some(text.as_str()), offer.as_deref(), card.as_deref()].into_iter().flatten().collect::<Vec<_>>().join("\n\n"));
        }
        // the answer turn is asked for words (`brief.plain_answer`): a doer with a Brief has it in the contract already
        if cx.spec.input.get("plain_answer") == Some(&Json::Bool(true)) && !self.inner.caps().brief && !cx.compacting {
            let text = cx.text.take().unwrap_or_else(|| cx.spec.goal.clone());
            cx.text = Some(format!("{text}\n\n{}", j4n_core::brief::PLAIN));
        }
        let run = self.inner.start(cx)?;
        Ok(Box::new(Kept { run, _socket: socket }))
    }
}
