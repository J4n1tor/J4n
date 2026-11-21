//! The `py` tool of a model that works on its own, offered while the setting `kit.mode`
//! is `on`: Python in a kernel that keeps its variables for the task.
//!
//! - **One kernel per task**, a stdlib Python process (`packs/kit/py/kernel.py`) that speaks JSON lines, started at the task's first
//!   cell and kept in a [`Pool`] by the task's id, so a later cell, and a later turn of the same task, finds its variables. It is
//!   started behind the place's walls like every program (`wrap`), with the network cut where the wall can (`Launch::no_net`) and
//!   an environment of its own (`env -i`): no key of J4n's is in it.
//! - **Every cell is one Gate call.** The cell is put to the Gate as the line `python3 -c '<cell>'`, so the rows that read a shell
//!   line read it: a cell that `pyread` reads (a known reader, nothing that writes, runs, opens or fetches) is `read`, any other
//!   is `exec`; a read-only task passes the first and is blocked or asked about the second.
//! - **A cell is capped and timed** (4,000 characters of output and of value, 60 s unless the call says up to 600). One that runs
//!   past its time is stopped by killing the kernel: its variables are gone, and the call and the next cell say so. So does a
//!   kernel that ended on its own, one that was launched under other walls, and `restart`.
//! - **Every cell is a `py.cell` event** in the Log (`RunEv::Py`): the code (in a blob), the risk, ok, the time and what came back.
//! - **Host calls**: a kernel line with a `call` and no `id` is the cell asking J4n for work (`kithost`: a decider, a model,
//!   a tool of the doer's offer). It is served on a thread while the tool waits, inside the cell's own time, and its answer is written
//!   back; each call is a row of the cell's event (`host`), and their cost goes to the spend fold. A task has one doer, so a doer's view
//!   of the tool ([`PyTools::view`]) holds its host, and with it the offer its cells may call; the kernels are shared.
//! - **Packages**: a cell that names `need("x")` is put to the Gate for the install before it runs (a host call
//!   mid-cell has nobody to ask); the kernel reaches `<home>/py` read-only inside a box, and a task's temp folder goes with its kernel.

use j4n_core::text::clip;
use crate::kithost::{Cell, Host};
use crate::model::doer::{Gated, ToolOut, Tools, action};
use crate::model::{Call, Tool};
use crate::place::boxed::BEHIND;
use crate::pool::{Pool, Pooled, Reuse, signature};
use crate::proc;
use j4n_core::gate::{Action, Gate, ReadOnly, Verb, Verdict, pyread};
use j4n_core::place::{Launch, Site};
use serde_json::{Value as Json, json};
use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ChildStdin;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, Once};
use std::time::{Duration, Instant};

/// The kernel, as the pack `kit` has it: given to `python3 -c`, so it needs no file in the box.
const KERNEL: &str = include_str!("../../../packs/kit/py/kernel.py");
/// The library `j4n` and its recipes: not files in the box. The kernel is sent them as its first line and makes the module by name, so
/// `import j4n` finds it.
const LIBRARY: &str = include_str!("../../../packs/kit/py/j4n.py");
pub const RECIPES: [(&str, &str); 6] = [
    ("checks", include_str!("../../../packs/kit/recipes/checks.py")),
    ("column_counts", include_str!("../../../packs/kit/recipes/column_counts.py")),
    ("each", include_str!("../../../packs/kit/recipes/each.py")),
    ("matching", include_str!("../../../packs/kit/recipes/matching.py")),
    ("near_duplicates", include_str!("../../../packs/kit/recipes/near_duplicates.py")),
    ("outline", include_str!("../../../packs/kit/recipes/outline.py")),
];
/// What a model that has the tool `py` is told of the kit (`packs/kit/prompts/card.txt`); law 14 reads that file.
pub const CARD: &str = include_str!("../../../packs/kit/prompts/card.txt");
/// A kernel idle this long is stopped.
pub const IDLE: Duration = Duration::from_secs(600);
const KERNELS: usize = 8;
const SECS: u64 = 60;
const SECS_MAX: u64 = 600;
const OUT_CHARS: usize = 12_000;
const HEAD: usize = 200;
const DOC: &str = "Run Python in a kernel that keeps its variables for this task (standard library only, no network). What the cell prints \
    and the value of its last expression come back, cut at 4,000 characters each: keep the data in variables and print counts and the \
    parts you need. A cell that runs past `timeout` seconds (60, at most 600) is stopped and the kernel restarts, so its variables are \
    gone; `restart` starts a fresh kernel first.";

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

enum Msg {
    Line(String),
    /// The kernel ended; the end of what it wrote to stderr.
    Gone(String),
}

enum Fail {
    Timeout,
    Gone(String),
}

/// The host calls of one cell: who serves them, for which task, and the rows they leave.
struct Hosting<'a> {
    host: Option<Arc<Host>>,
    site: &'a Site,
    /// How the task may write, the walls a tool's program runs behind, and the tool calls made so far: the bridge holds a call to them.
    read_only: ReadOnly,
    wrap: &'a (dyn Fn(Launch) -> Launch + Send + Sync),
    used: AtomicUsize,
    rows: Vec<Json>,
    /// The packages the Gate let through for this cell (canonical name, temp): `need` installs these and no other.
    needs: Vec<(String, bool)>,
    /// Where a tool brought back words that are not the operator's, the first one; the paths a tool changed; the task was tainted here.
    from: Option<String>,
    changed: Vec<String>,
    tainted: bool,
}

impl Hosting<'_> {
    /// One call. The bridge's run here, behind the cell's walls, and start no call once the cell's time is over; any other is served on a
    /// thread of its own so that the cell's time covers it: past `until` the kernel goes (the call ends by itself and its answer is dropped).
    fn serve(&mut self, name: &str, payload: &Json, until: Instant) -> Result<Json, Fail> {
        let Some(host) = self.host.clone() else {
            self.rows.push(json!({ "call": clip(name, 40), "ok": false, "error": "this kernel has no host calls" }));
            return Ok(json!({ "ok": false, "error": "this kernel has no host calls: they are not set up here" }));
        };
        if name == "need" {
            let (reply, row) = host.need(payload, self.site, &self.needs, until);
            self.keep(vec![row]);
            return Ok(reply);
        }
        if Host::bridged(name) {
            let cx = Cell { read_only: self.read_only, until, used: &self.used, wrap: self.wrap };
            let (reply, rows) = host.bridge(name, payload, self.site, &cx);
            self.keep(rows);
            return Ok(reply);
        }
        let (tx, rx) = mpsc::channel();
        let (n, p) = (name.to_string(), payload.clone());
        std::thread::spawn(move || drop(tx.send(host.serve(&n, &p))));
        match rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
            Ok((reply, row)) => {
                self.keep(vec![row]);
                Ok(reply)
            }
            Err(RecvTimeoutError::Timeout) => {
                self.rows.push(json!({ "call": clip(name, 40), "ok": false, "error": "it ran past the cell's time" }));
                Err(Fail::Timeout)
            }
            Err(RecvTimeoutError::Disconnected) => Err(Fail::Gone("a host call failed".into())),
        }
    }

    /// The rows of calls that were made, and what they say of the words they brought back and the files they changed.
    fn keep(&mut self, rows: Vec<Json>) {
        for r in &rows {
            if let Some(from) = r["untrusted"].as_str() {
                self.from.get_or_insert_with(|| from.to_string());
            }
            self.tainted |= r["taint"] == true;
            self.changed.extend(r["changed"].as_array().into_iter().flatten().filter_map(|c| c.as_str().map(String::from)));
        }
        self.rows.extend(rows);
    }
}

/// A live kernel.
pub struct Kernel {
    task: String,
    launch: String,
    cwd: PathBuf,
    child: proc::Child,
    stdin: Mutex<ChildStdin>,
    rx: Mutex<Receiver<Msg>>,
    dead: Arc<AtomicBool>,
    busy: AtomicBool,
    last: Mutex<Instant>,
    next: AtomicU64,
    started: Instant,
    /// This task's temp packages (`need(.., temp=True)`): they go with the kernel.
    temp: Option<PathBuf>,
}

impl Kernel {
    fn start(task: &str, launch: &Launch, sig: String, (keep, off): (Option<f64>, &[String]), temp: Option<PathBuf>) -> Result<Kernel, String> {
        let (tx, rx) = mpsc::channel();
        let (gone, dead) = (tx.clone(), Arc::new(AtomicBool::new(false)));
        let flag = dead.clone();
        let (child, stdin) = proc::spawn(
            launch,
            true,
            move |l| drop(tx.send(Msg::Line(l.to_string()))),
            move |exit| {
                flag.store(true, Ordering::SeqCst);
                drop(gone.send(Msg::Gone(exit.tail)));
            },
        )
        .map_err(|e| format!("could not start python3: {e}"))?;
        let mut stdin = stdin.ok_or("the kernel has no input")?;
        let recipes: HashMap<&str, &str> = RECIPES.into_iter().collect();
        let load = json!({ "id": 0, "load": { "modules": { "j4n": LIBRARY }, "recipes": recipes, "config": { "keep": keep, "off": off } } });
        writeln!(stdin, "{load}").and_then(|()| stdin.flush()).map_err(|e| format!("the kernel could not be given its library: {e}"))?;
        Ok(Kernel { task: task.into(), launch: sig, cwd: launch.cwd.clone(), child, stdin: Mutex::new(stdin), rx: Mutex::new(rx), dead, busy: AtomicBool::new(false), last: Mutex::new(Instant::now()), next: AtomicU64::new(1), started: Instant::now(), temp })
    }

    /// A task that works on keeps its temp folder from being swept as one left behind.
    fn touch(&self) {
        if let Some(f) = self.temp.as_ref().and_then(|t| std::fs::File::open(t).ok()) {
            drop(f.set_modified(std::time::SystemTime::now()));
        }
    }

    fn end(&self) {
        self.child.kill();
        if let Some(t) = &self.temp {
            drop(std::fs::remove_dir_all(t));
        }
    }

    /// One cell: the kernel's answer, or why there is none. A cell that outruns `secs` is `Fail::Timeout`; the caller takes the kernel down.
    /// The host calls the cell makes are served inside those `secs` and their rows are added to `rows`.
    fn cell(&self, code: &str, secs: u64, hosting: &mut Hosting) -> Result<Json, Fail> {
        self.busy.store(true, Ordering::SeqCst);
        let done = self.exchange(code, secs, hosting);
        *lock(&self.last) = Instant::now();
        self.touch();
        self.busy.store(false, Ordering::SeqCst);
        done
    }

    fn exchange(&self, code: &str, secs: u64, hosting: &mut Hosting) -> Result<Json, Fail> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let line = format!("{}\n", json!({ "id": id, "code": code }));
        {
            let mut w = lock(&self.stdin);
            w.write_all(line.as_bytes()).and_then(|()| w.flush()).map_err(|e| Fail::Gone(format!("it could not be written to: {e}")))?;
        }
        let rx = lock(&self.rx);
        let until = Instant::now() + Duration::from_secs(secs);
        loop {
            match rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
                Ok(Msg::Line(l)) => {
                    let Ok(a) = serde_json::from_str::<Json>(&l) else { continue };
                    if a["id"] == id {
                        return Ok(a);
                    }
                    if let Some(name) = a["call"].as_str() {
                        let reply = hosting.serve(name, &a["payload"], until)?;
                        let mut w = lock(&self.stdin);
                        writeln!(w, "{reply}").and_then(|()| w.flush()).map_err(|e| Fail::Gone(format!("it could not be written to: {e}")))?;
                    }
                }
                Ok(Msg::Gone(tail)) => return Err(Fail::Gone(tail)),
                Err(RecvTimeoutError::Timeout) => return Err(Fail::Timeout),
                Err(RecvTimeoutError::Disconnected) => return Err(Fail::Gone(String::new())),
            }
        }
    }
}

impl Drop for Kernel {
    fn drop(&mut self) {
        self.end();
    }
}

impl Pooled for Kernel {
    fn session_id(&self) -> Option<String> {
        Some(self.task.clone())
    }

    fn launch(&self) -> &str {
        &self.launch
    }

    fn cwd(&self) -> &Path {
        &self.cwd
    }

    fn is_dead(&self) -> bool {
        self.dead.load(Ordering::SeqCst) || !self.child.alive()
    }

    fn is_idle(&self) -> bool {
        !self.busy.load(Ordering::SeqCst)
    }

    fn last_activity(&self) -> Instant {
        *lock(&self.last)
    }

    fn kill(&self) {
        self.end();
    }
}

/// Whether a `python3` is on the `PATH`. With none the tool is not offered (J4n works without it, law 6).
pub fn python_here() -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("python3").is_file()))
}

/// The kernel's command line: a bare environment with the `PATH` and nothing else, so no key of J4n's is in it, and Python in its
/// isolated mode (no user site, no `PYTHON*` variables).
fn argv() -> Vec<String> {
    let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into());
    ["env", "-i"].into_iter().map(String::from).chain([format!("PATH={path}"), "LANG=C.UTF-8".into()]).chain(["python3", "-I", "-c"].into_iter().map(String::from)).chain([KERNEL.to_string()]).collect()
}

/// The kernel's launch for a task's place, as the place's walls make it (`wrap`): the same for every cell of the task.
pub fn kernel_launch(site: &Site) -> Launch {
    Launch { no_net: true, ..Launch::new(argv(), site.dir.clone(), Default::default()) }
}

/// The host calls of a cell in one line, for the Kit page: each with its time and tokens; the first dozen, and how many more.
fn calls_line(rows: &[Json]) -> String {
    let one = |r: &Json| {
        let tokens = r["tokens_in"].as_u64().unwrap_or(0) + r["tokens_out"].as_u64().unwrap_or(0);
        let what = [r["call"].as_str().unwrap_or("?"), r["tool"].as_str().unwrap_or("")].join(" ");
        let end = if r["refused"] == true { " refused" } else if r["ok"] == false { " failed" } else { "" };
        format!("{} {}ms{}{end}", what.trim(), r["ms"].as_u64().unwrap_or(0), if tokens > 0 { format!(" {tokens} tokens") } else { String::new() })
    };
    let more = rows.len().saturating_sub(12);
    rows.iter().take(12).map(one).chain((more > 0).then(|| format!("+{more} more"))).collect::<Vec<_>>().join(" · ")
}

/// The cell as the shell line the Gate reads.
fn line(code: &str) -> String {
    format!("python3 -c '{}'", code.replace('\'', "'\\''"))
}

/// The tool `py`.
pub struct PyTools {
    gate: Option<Gate>,
    pool: Arc<Pool<Kernel>>,
    /// What a task is told with its next cell: its kernel is not the one it had.
    lost: Arc<Mutex<HashMap<String, String>>>,
    reaper: Arc<Once>,
    idle: Duration,
    /// Serves the host calls of cells; none, and a cell's call is answered "not set up".
    host: Option<Arc<Host>>,
    /// Where the last cell read words from outside, for the taint rules.
    fetched: Mutex<Option<String>>,
    /// How each task may write, as the last Gate question for its cell said: the bridge holds a tool call to it.
    ro: Mutex<HashMap<String, ReadOnly>>,
    /// The packages the Gate let through for each task's next cell: what `gate` found in the cell, for `run` to hold `need` to.
    needed: Mutex<HashMap<String, Vec<(String, bool)>>>,
}

impl Default for PyTools {
    fn default() -> PyTools {
        PyTools::new()
    }
}

impl PyTools {
    pub fn new() -> PyTools {
        PyTools { gate: None, pool: Arc::new(Pool::new(KERNELS, IDLE)), lost: Arc::default(), reaper: Arc::new(Once::new()), idle: IDLE, host: None, fetched: Mutex::new(None), ro: Mutex::default(), needed: Mutex::default() }
    }

    pub fn with_gate(mut self, gate: Gate) -> PyTools {
        self.gate = Some(gate);
        self
    }

    /// Who serves the host calls a cell makes (`j4n.judge`, `j4n.model`, `j4n.call`).
    pub fn with_host(mut self, host: Arc<Host>) -> PyTools {
        self.host = Some(host);
        self
    }

    /// The tool as one doer has it: the same kernels, the same Gate, and the host that holds what this doer's cells may call.
    pub fn view(&self, host: Arc<Host>) -> PyTools {
        PyTools { gate: self.gate.clone(), pool: self.pool.clone(), lost: self.lost.clone(), reaper: self.reaper.clone(), idle: self.idle, host: Some(host), fetched: Mutex::new(None), ro: Mutex::default(), needed: Mutex::default() }
    }

    /// The parts of the kit that are turned off (`kit.off`).
    pub fn off(&self) -> &[String] {
        self.host.as_ref().map_or(&[][..], |h| h.off())
    }

    /// `card` with a last sentence naming the parts of the kit that are turned off (`kit.off`), and without the sentences about the
    /// judge while `kit.judge` is off: they could only fail.
    pub fn card(&self, card: &str) -> String {
        let judge = self.host.as_ref().is_some_and(|h| h.judge());
        crate::kituse::card_with(&if judge { card.to_string() } else { crate::kituse::without_judge(card) }, self.off())
    }

    /// What this doer's cells may call (the specs), for the offer line.
    pub fn offered(&self) -> Vec<crate::model::Tool> {
        self.host.as_ref().map(|h| h.specs()).unwrap_or_default()
    }

    /// The launch of a task's kernel: the same for every cell, and for the shell's socket. In a box it reads `<home>/py` (the packages).
    pub fn kernel(&self, site: &Site) -> Launch {
        let mut launch = kernel_launch(site);
        launch.ro.extend(self.host.as_ref().and_then(|h| h.pydir()));
        launch
    }

    /// Kernels idle for `idle` are stopped (the default is [`IDLE`]).
    pub fn idle(mut self, idle: Duration) -> PyTools {
        self.pool = Arc::new(Pool::new(KERNELS, idle));
        self.idle = idle;
        self
    }

    /// How many kernels are alive.
    pub fn alive(&self) -> usize {
        self.pool.alive()
    }

    /// A thread that stops the kernels that have waited too long, for as long as the tool is there.
    fn reap(&self) {
        self.reaper.call_once(|| {
            let (pool, every) = (Arc::downgrade(&self.pool), (self.idle / 4).clamp(Duration::from_millis(50), Duration::from_secs(30)));
            std::thread::spawn(move || {
                while let Some(p) = pool.upgrade() {
                    p.reap(Instant::now());
                    drop(p);
                    std::thread::sleep(every);
                }
            });
        });
    }

    fn said(&self, task: &str, why: &str) {
        lock(&self.lost).insert(task.into(), format!("the kernel was restarted ({why}), so the variables of earlier cells are gone"));
    }

    /// The kernel of `task`: the one it has, or a new one (and then why it is new, when it had one before).
    fn kernel_of(&self, task: &str, site: &Site, launch: Launch) -> Result<(Arc<Kernel>, Option<String>), String> {
        let sig = signature(&launch.argv, &launch.cwd, &launch.env);
        match self.pool.reusable(task, &sig, &site.dir) {
            Reuse::Live(k) => return Ok((k, None)),
            Reuse::Busy => return Err("a cell is still running in this task's kernel".into()),
            Reuse::Replaced(why) => self.said(task, if why == "gone" { "it had ended" } else { why }),
            Reuse::None => {}
        }
        let temp = self.host.as_ref().and_then(|h| h.temp_of(task));
        if let Some(h) = &self.host {
            h.sweep();
        }
        let kernel = Arc::new(Kernel::start(task, &launch, sig, (self.host.as_ref().map(|h| h.keep()), self.host.as_ref().map_or(&[][..], |h| h.off())), temp)?);
        self.reap();
        self.pool.insert(kernel.clone());
        Ok((kernel, lock(&self.lost).remove(task)))
    }

    /// The Gate's question for the packages a cell names: an install is a `net` effect on `pypi:<name>`, put once, before the
    /// cell runs. A package already there, or allowed by the rules, a grant or the setting `kit.grants`, goes through; a Block ends the cell;
    /// the first one that asks is asked, and the cell may then install it (one package a cell is asked about). What passes is kept for `run`.
    fn packages(&self, code: &str, task: &str, site: &Site, read_only: ReadOnly, check: &dyn Fn(&Action) -> Verdict) -> Result<Option<Gated>, Box<Gated>> {
        let Some(host) = self.host.as_ref() else { return Ok(None) };
        let (mut passed, mut asking): (Vec<(String, bool)>, Option<Gated>) = (Vec::new(), None);
        for (name, temp) in crate::kitpkg::named(code).into_iter().filter(|(n, t)| !host.has(task, n, *t)) {
            let a = action("py", Verb::Net, &format!("pypi:{name}"), task, site, read_only);
            let words = format!("install the Python package `{name}` from the network into {}", if temp { "a folder for this task only" } else { "J4n's shared Python folder (py/env)" });
            match check(&a) {
                Verdict::Allow { .. } => passed.push((name, temp)),
                Verdict::Block { .. } => return Err(Box::new(Gated { action: a, question: words, risk: 100 })),
                Verdict::Ask { .. } if host.granted(&name) => passed.push((name, temp)),
                Verdict::Ask { risk, .. } if asking.is_none() => {
                    passed.push((name, temp));
                    asking = Some(Gated { action: a, question: words, risk });
                }
                Verdict::Ask { .. } => {}
            }
        }
        lock(&self.needed).insert(task.to_string(), passed);
        Ok(asking)
    }

    fn cell(&self, call: &Call, site: &Site, wrap: &(dyn Fn(Launch) -> Launch + Send + Sync)) -> ToolOut {
        let code = call.args.get("code").and_then(Json::as_str).unwrap_or_default();
        let secs = call.args.get("timeout").and_then(Json::as_u64).unwrap_or(SECS).clamp(1, SECS_MAX);
        let task = site.task.as_str();
        let restart = call.args.get("restart").and_then(Json::as_bool).unwrap_or(false);
        if restart && let Some(old) = self.pool.forget(task) {
            old.kill();
            self.said(task, "you asked for it");
        }
        if code.trim().is_empty() && !restart {
            return ToolOut { text: "`py` needs `code`".into(), error: true, ..ToolOut::default() };
        }
        let walled = site.notes.iter().any(|n| n.starts_with(BEHIND));
        let launch = wrap(self.kernel(site));
        let (kernel, notice) = match self.kernel_of(task, site, launch) {
            Ok(k) => k,
            Err(why) => return ToolOut { text: why, error: true, ..ToolOut::default() },
        };
        let risk = if pyread::reads(code) { "read" } else { "exec" };
        let t = Instant::now();
        let mut notes: Vec<String> = notice.into_iter().collect();
        let read_only = lock(&self.ro).get(task).copied().unwrap_or(if site.read_only { ReadOnly::Explicit } else { ReadOnly::No });
        let needs = lock(&self.needed).remove(task).unwrap_or_default();
        let mut hosting = Hosting { host: self.host.clone(), site, read_only, wrap, used: AtomicUsize::new(0), rows: Vec::new(), needs, from: None, changed: Vec::new(), tainted: false };
        let (mut names, mut mem, mut used) = (Json::Null, Json::Null, Json::Null);
        let (ok, out, value, error) = if code.trim().is_empty() {
            (true, String::new(), None, None)
        } else {
            match kernel.cell(code, secs, &mut hosting) {
                Ok(a) => {
                    (names, mem, used) = (a["names"].clone(), a["mem"].clone(), a["used"].clone());
                    let text = |k: &str| a[k].as_str().filter(|s| !s.is_empty()).map(String::from);
                    (a["ok"].as_bool().unwrap_or(false), text("out").unwrap_or_default(), text("value"), text("error"))
                }
                Err(fail) => {
                    drop(self.pool.forget(task));
                    kernel.kill();
                    let why = match fail {
                        Fail::Timeout => format!("the cell ran past {secs} s and was stopped; the kernel was restarted, so the variables of earlier cells are gone"),
                        Fail::Gone(tail) => format!("the kernel ended while it ran the cell{}; it was restarted, so the variables of earlier cells are gone", if tail.is_empty() { String::new() } else { format!(" ({})", clip(tail.trim(), 300)) }),
                    };
                    self.said(task, "it was stopped");
                    (false, String::new(), None, Some(why))
                }
            }
        };
        *lock(&self.fetched) = hosting.from.clone();
        if let (true, Some(from)) = (hosting.tainted, &hosting.from) {
            notes.push(format!("this task has read from {from}, which is not the operator's: it may not send anything out or write to a sensitive path until the next line"));
        }
        if !walled && kernel.next.load(Ordering::SeqCst) <= 2 {
            notes.push("this task is not behind a box, so the kernel can reach the network".into());
        }
        let mut text: String = notes.iter().map(|n| format!("[{n}]\n")).collect();
        for (label, part) in [("", Some(out.clone())), ("=> ", value.clone())] {
            if let Some(p) = part.filter(|p| !p.is_empty()) {
                text.push_str(&format!("{label}{p}{}", if p.ends_with('\n') { "" } else { "\n" }));
            }
        }
        if let Some(e) = &error {
            text.push_str(&format!("error: {e}\n"));
        }
        if text.is_empty() {
            text = "(no output)".into();
        }
        let hash = j4n_core::text::short_hash(code);
        let mut cell = json!({ "code": code, "head": clip(code, HEAD), "hash": hash, "risk": risk, "ok": ok, "ms": t.elapsed().as_millis() as u64, "out": out, "value": value, "error": error, "walled": walled, "up": kernel.started.elapsed().as_secs(), "names": names, "mem": mem });
        if let Some(used) = crate::kituse::clean_used(&used, &RECIPES.map(|r| r.0)) {
            cell["used"] = used;
        }
        if !hosting.rows.is_empty() {
            cell["calls"] = json!(calls_line(&hosting.rows));
            cell["host"] = Json::Array(hosting.rows);
        }
        ToolOut { text: clip(text.trim_end(), OUT_CHARS), error: !ok, changed: hosting.changed, py: Some(cell), ..ToolOut::default() }
    }
}

impl Tools for PyTools {
    fn specs(&self) -> Vec<Tool> {
        if !python_here() {
            return Vec::new();
        }
        let props = json!({ "code": { "type": "string" }, "timeout": { "type": "integer" }, "restart": { "type": "boolean" } });
        vec![Tool { name: "py".into(), doc: DOC.into(), schema: json!({ "type": "object", "properties": props, "required": ["code"] }) }]
    }

    fn gate(&self, call: &Call, task: &str, site: &Site, read_only: ReadOnly) -> Option<Gated> {
        if call.name != "py" {
            return None;
        }
        let code = call.args.get("code").and_then(Json::as_str).unwrap_or_default();
        let reads = pyread::reads(code);
        lock(&self.ro).insert(task.to_string(), read_only);
        lock(&self.needed).remove(task);
        let mut a = action("py", Verb::Shell, &line(code), task, site, read_only);
        a.inside = Some(true);
        let question = format!("run a Python cell{}: {}", if reads { " (it only reads)" } else { "" }, clip(code.trim(), HEAD));
        let check = |a: &Action| self.gate.as_ref().map_or_else(|| Verdict::Ask { risk: 40, why: "no rules are loaded".into(), by: "no gate".into() }, |g| g.check(a));
        let cell = match check(&a) {
            Verdict::Allow { .. } => None,
            Verdict::Ask { risk, .. } => Some(Gated { action: a, question, risk }),
            Verdict::Block { .. } => return Some(Gated { action: a, question, risk: 100 }),
        };
        // one question for the cell and its packages: a yes to it is a yes to both, and a Block of a package ends the cell
        match (cell, self.packages(code, task, site, read_only, &check)) {
            (_, Err(blocked)) => Some(*blocked),
            (Some(mut c), Ok(Some(p))) => {
                c.question = format!("{}, and {}", p.question, c.question);
                c.risk = c.risk.max(p.risk);
                Some(c)
            }
            (c, Ok(p)) => c.or(p),
        }
    }

    fn untrusted(&self, call: &Call) -> Option<String> {
        if call.name == "py" { lock(&self.fetched).take() } else { None }
    }

    fn run(&self, call: &Call, site: &Site, wrap: &(dyn Fn(Launch) -> Launch + Send + Sync)) -> ToolOut {
        if call.name == "py" { self.cell(call, site, wrap) } else { ToolOut { text: format!("there is no tool `{}`", call.name), error: true, ..ToolOut::default() } }
    }
}
