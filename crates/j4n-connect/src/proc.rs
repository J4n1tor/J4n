//! A program run as a child: a process group of its own, so no descendant outlives a kill; its stdout
//! read line by line on a thread of its own; its stderr kept as a short tail for the failure message; the leader reaped.
//! The confinement of a child is the Place's (`StartCx::wrap`).
//!
//! The reader is a plain thread over a plain pipe, not an async pipe: Bun-based CLIs exit early (mid-run, exit 0) when their
//! stdout is a tokio-owned pipe, and not when it is a file descriptor read by a blocking thread.

use j4n_core::host::Scrub;
use j4n_core::place::Launch;
use std::io::{self, BufRead, BufReader, Read};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock};
use std::time::{Duration, Instant};

/// The longest line read whole; the rest of a longer one is dropped (and what was kept fails to parse, which is said).
pub const LINE_MAX: usize = 16 << 20;
/// How much of the end of stderr is kept.
pub const TAIL_MAX: usize = 2048;
/// How long a leader that has closed its output is given to exit on its own.
const GRACE: Duration = Duration::from_millis(300);
/// How many times a program that is "busy" is tried (the pauses between them double from 1 ms: 127 ms in all).
const BUSY_TRIES: u32 = 8;

/// What is taken out of the environment of every child (kind `host`): the Table's, once it is loaded; the base pack's before.
static SCRUB: RwLock<Option<Arc<Scrub>>> = RwLock::new(None);

/// Say what the loaded Table's `host` rows take out of every child's environment (the front end calls it when it loads the Table).
pub fn install_scrub(s: Scrub) {
    *SCRUB.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(s));
}

fn scrub_now() -> Arc<Scrub> {
    static BASE: OnceLock<Arc<Scrub>> = OnceLock::new();
    SCRUB.read().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_else(|| BASE.get_or_init(|| Arc::new(Scrub::base())).clone())
}

/// Take the host's session out of what `cmd` inherits: the variables by which a program that runs agents (the Claude desktop app, codex, herdr)
/// binds a child to itself, named by the rows of kind `host`, and the operator's own key variables, which only the program whose row names
/// one is given back ([`crate::manifest::Connector::environment`]). Call it before the variables J4n means to give (`.env`, `.envs`), which stay.
/// Every program J4n starts for an agent, a shell line or a tool goes through here (the doers and `spawn_with`, the lab, the relay).
pub fn scrub(cmd: &mut Command) {
    scrub_now().strip(cmd);
}

/// The key variables of J4n's environment that are no host's: what a J4n that J4n starts is given back after the scrub.
pub fn keys() -> Vec<(String, String)> {
    scrub_now().keys()
}

/// `cmd.spawn()`, tried again while the program is busy (`ETXTBSY`). A script written a moment ago can still be open for
/// writing in a child that another thread forked in that moment (the descriptor is closed at that child's exec, not before), and
/// exec of it fails until then: a hook an agent has just written, a script a test has just made. It passes in a few ms, so the
/// pause is short and the tries are few; any other error, and the last try's, is returned as it is. Every program J4n starts
/// that somebody may have just written goes through here.
pub fn start(cmd: &mut Command) -> io::Result<std::process::Child> {
    let mut pause = Duration::from_millis(1);
    for _ in 1..BUSY_TRIES {
        match cmd.spawn() {
            Err(e) if e.kind() == io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(pause);
                pause *= 2;
            }
            done => return done,
        }
    }
    cmd.spawn()
}

/// How a child ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exit {
    pub code: Option<i32>,
    pub signal: Option<i32>,
    /// The last of what it wrote to stderr.
    pub tail: String,
    /// Our own kill, not the child's end.
    pub killed: bool,
    /// The line handler panicked, so the child was stopped.
    pub broken: bool,
}

impl Exit {
    /// "exit 3", "signal 9": how it went, in words.
    pub fn how(&self) -> String {
        match (self.code, self.signal) {
            (Some(c), _) => format!("exit {c}"),
            (None, Some(s)) => format!("signal {s}"),
            _ => "an unknown end".into(),
        }
    }
}

#[derive(Default)]
struct State {
    /// The leader has been reaped: its pid may be anyone's now, so nothing is signalled.
    gone: bool,
    killed: bool,
}

/// A running child: its pid and the way to take its whole group down.
#[derive(Clone)]
pub struct Child {
    pub pid: u32,
    state: Arc<Mutex<State>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// SAFETY: both declarations match the libc functions of the same names (`kill(2)` and `prctl(2)`: integer arguments, an
// integer result, `unsigned long` as `u64` on the 64-bit Linux this runs on), and the process links libc.
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
    fn prctl(option: i32, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> i32;
}

const SIGKILL: i32 = 9;
const PR_SET_PDEATHSIG: i32 = 1;

/// Kill the group of `pid`. Only while the leader is not yet reaped: its zombie keeps the group id reserved.
fn kill_group(pid: u32) {
    // SAFETY: `kill` takes two integers; a group that is already empty answers ESRCH, which is nothing.
    unsafe { kill(-(pid as i32), SIGKILL) };
}

impl Child {
    /// Kill the child's whole process group, now. A child blocked on a silent pipe dies too (a flag alone left live
    /// children running until they happened to exit).
    pub fn kill(&self) {
        let mut st = lock(&self.state);
        if !st.gone {
            st.killed = true;
            kill_group(self.pid);
        }
    }

    /// Whether the leader has not been reaped yet (a zombie counts until the reader thread has waited for it).
    pub fn alive(&self) -> bool {
        !lock(&self.state).gone
    }
}

/// Read one line of at most [`LINE_MAX`] bytes; false at the end. The rest of a longer line is read and dropped.
fn read_line(reader: &mut impl BufRead, buf: &mut Vec<u8>) -> io::Result<bool> {
    buf.clear();
    let n = reader.by_ref().take(LINE_MAX as u64).read_until(b'\n', buf)?;
    if n == 0 {
        return Ok(false);
    }
    if buf.last() != Some(&b'\n') && buf.len() >= LINE_MAX {
        let mut skip = Vec::new();
        while reader.by_ref().take(1 << 20).read_until(b'\n', &mut skip)? > 0 && skip.last() != Some(&b'\n') {
            skip.clear();
        }
    }
    while matches!(buf.last(), Some(b'\n' | b'\r')) {
        buf.pop();
    }
    Ok(true)
}

fn keep_tail(tail: &Mutex<String>, line: &str) {
    let mut t = lock(tail);
    t.push_str(line);
    t.push('\n');
    if t.len() > TAIL_MAX {
        let mut cut = t.len() - TAIL_MAX;
        while !t.is_char_boundary(cut) {
            cut += 1;
        }
        t.drain(..cut);
    }
}

/// Start `launch` (the program is its first argument) in its own process group. `on_line` gets each line of stdout on the
/// reader thread; `on_exit` is called once from that thread when the output has ended and the leader is reaped. With
/// `piped_stdin` the child's stdin is returned, for what is said to it in-band; without, it reads nothing.
pub fn spawn(
    launch: &Launch,
    piped_stdin: bool,
    on_line: impl FnMut(&str) + Send + 'static,
    on_exit: impl FnOnce(Exit) + Send + 'static,
) -> io::Result<(Child, Option<ChildStdin>)> {
    spawn_with(launch, piped_stdin, false, on_line, on_exit)
}

/// [`spawn`] for a child that is to end with J4n (a sign-in waiting for a browser): it is killed when the thread that started it ends, which is
/// when J4n is killed in the middle of the wait. The caller waits for the child on that thread, and [`hold`]s it for the ends that run code.
pub fn spawn_bound(
    launch: &Launch,
    piped_stdin: bool,
    on_line: impl FnMut(&str) + Send + 'static,
    on_exit: impl FnOnce(Exit) + Send + 'static,
) -> io::Result<(Child, Option<ChildStdin>)> {
    spawn_with(launch, piped_stdin, true, on_line, on_exit)
}

fn spawn_with(
    launch: &Launch,
    piped_stdin: bool,
    bound: bool,
    mut on_line: impl FnMut(&str) + Send + 'static,
    on_exit: impl FnOnce(Exit) + Send + 'static,
) -> io::Result<(Child, Option<ChildStdin>)> {
    let (program, args) = launch.argv.split_first().ok_or_else(|| io::Error::other("nothing to run"))?;
    let mut cmd = Command::new(program);
    scrub(&mut cmd);
    cmd.args(args)
        .current_dir(&launch.cwd)
        .envs(&launch.env)
        // bun and node CLIs trust the environment's PWD over getcwd()
        .env("PWD", &launch.cwd)
        .env("OLDPWD", &launch.cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(if piped_stdin { Stdio::piped() } else { Stdio::null() })
        .process_group(0);
    if bound {
        // SAFETY: between fork and exec only this one syscall of two integers runs, which is async-signal-safe.
        unsafe {
            cmd.pre_exec(|| {
                prctl(PR_SET_PDEATHSIG, SIGKILL as u64, 0, 0, 0);
                Ok(())
            });
        }
    }
    let mut child = start(&mut cmd)?;
    let pid = child.id();
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        kill_group(pid);
        let _ = child.wait();
        return Err(io::Error::other("the child has no output"));
    };
    let stdin = child.stdin.take();
    let tail = Arc::new(Mutex::new(String::new()));
    let state = Arc::new(Mutex::new(State::default()));
    let errs = {
        let tail = tail.clone();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut buf = Vec::new();
            while read_line(&mut reader, &mut buf).unwrap_or(false) {
                keep_tail(&tail, &String::from_utf8_lossy(&buf));
            }
        })
    };
    let st = state.clone();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut buf = Vec::new();
        let mut broken = false;
        while read_line(&mut reader, &mut buf).unwrap_or(false) {
            let line = String::from_utf8_lossy(&buf);
            if catch_unwind(AssertUnwindSafe(|| on_line(&line))).is_err() {
                broken = true;
                break;
            }
        }
        // The output is over: the leader is reaped (a dropped `Child` only detaches, which leaves a zombie behind for every
        // finished turn) and whatever it left running goes with it. A leader that closes its output usually exits a moment
        // later, and one that is killed in that moment reports a signal and not its own exit code, so it is given [`GRACE`]
        // first. Held under the lock, so a kill never signals a group id that has been given to someone else.
        let (status, killed) = {
            let mut now = lock(&st);
            let until = Instant::now() + GRACE;
            let status = loop {
                match child.try_wait() {
                    Ok(Some(s)) => break Ok(s),
                    Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(2)),
                    Ok(None) | Err(_) => {
                        kill_group(pid);
                        break child.wait();
                    }
                }
            };
            kill_group(pid);
            now.gone = true;
            (status, now.killed)
        };
        let _ = errs.join();
        let (code, signal) = match status {
            Ok(s) => (s.code(), s.signal()),
            Err(_) => (None, None),
        };
        let tail = lock(&tail).trim().to_string();
        on_exit(Exit { code, signal, tail, killed, broken });
    });
    Ok((Child { pid, state }, stdin))
}

/// The short questions in flight, kept to be ended together.
#[derive(Default)]
pub struct Captures(Mutex<Vec<Child>>);

impl Captures {
    /// Kill every question still in flight; each `capture_in` that waited for one returns, with an exit that says it was killed.
    pub fn end(&self) {
        let live: Vec<Child> = lock(&self.0).clone();
        live.iter().for_each(Child::kill);
    }
}

static CAPTURES: Captures = Captures(Mutex::new(Vec::new()));

/// Keep `child` among those that [`end_captures`] kills when J4n ends.
pub fn hold(child: &Child) {
    lock(&CAPTURES.0).push(child.clone());
}

/// Let go of a child that has ended: [`hold`]'s other half.
pub fn release(child: &Child) {
    lock(&CAPTURES.0).retain(|c| c.pid != child.pid);
}

/// Kill every short question still in flight (a `--version`, a model list), as J4n ends: a program J4n started does not outlive it.
pub fn end_captures() {
    CAPTURES.end();
}

/// Run `launch` to its end and return what it wrote to stdout and how it ended; a child still going after `limit` is killed
/// (and its exit says so). For the short questions put to a CLI (`--version`, `auth status`), which must never hang a caller.
pub fn capture(launch: &Launch, limit: std::time::Duration) -> io::Result<(String, Exit)> {
    capture_in(&CAPTURES, launch, limit)
}

/// [`capture`], kept in `book` while it runs (`capture` keeps it in the process's own, which `end_captures` ends).
pub fn capture_in(book: &Captures, launch: &Launch, limit: std::time::Duration) -> io::Result<(String, Exit)> {
    let out = Arc::new(Mutex::new(String::new()));
    let (tx, rx) = std::sync::mpsc::channel();
    let sink = out.clone();
    let (child, _) = spawn(
        launch,
        false,
        move |l| {
            let mut s = lock(&sink);
            s.push_str(l);
            s.push('\n');
        },
        move |e| drop(tx.send(e)),
    )?;
    lock(&book.0).push(child.clone());
    let exit = match rx.recv_timeout(limit) {
        Ok(e) => e,
        Err(_) => {
            child.kill();
            rx.recv().map_err(|_| io::Error::other("the child's end was lost"))?
        }
    };
    lock(&book.0).retain(|c| c.pid != child.pid);
    let text = lock(&out).trim().to_string();
    Ok((text, exit))
}
