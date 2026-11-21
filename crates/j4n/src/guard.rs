//! The restart guard: `:restart` ends the screen, and this small process starts the
//! build that should run next and watches it come up. The new J4n writes a ready file once its first frame is drawn; a build that
//! dies, or stays silent for [`ready_secs`] seconds, is stopped and marked bad, and the build that was running is started again.
//! The operator sees one line about it, never an empty terminal; the chat is where it was, because it is read back from the Log.
//!
//! The guard is the foreground parent of every J4n that follows: one it started ends a later restart by exiting with
//! [`RESTART_CODE`], so every generation is watched, not just the first. A plain `j4n` that restarts for the first time becomes the
//! guard itself.

use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

/// The exit status of a J4n that asks its guard for the next build (`EX_TEMPFAIL`).
pub const RESTART_CODE: i32 = 75;
/// What the guard keeps: the last build that came up (`previous`), the hashes of builds that did not (`bad`).
const PREVIOUS: &str = "previous";
const BAD: &str = "bad";

/// How long a new build has to draw its first frame (`J4N_READY_SECS`).
pub fn ready_secs() -> u64 {
    std::env::var("J4N_READY_SECS").ok().and_then(|v| v.parse().ok()).filter(|n| *n > 0).unwrap_or(20)
}

/// Whether this process was started by a guard (so a restart is an exit, not a guard of its own).
pub fn guarded() -> bool {
    std::env::var_os("J4N_GUARD").is_some()
}

/// Why this start is not the usual one (a build failed and the one before it is back), for the first line of the chat.
pub fn start_note() -> Option<String> {
    std::env::var("J4N_START_NOTE").ok().filter(|n| !n.trim().is_empty())
}

/// The first frame is drawn: tell the guard this build came up.
pub fn mark_ready() {
    if let Some(path) = std::env::var_os("J4N_READY_FILE").filter(|p| !p.is_empty()) {
        let _ = std::fs::write(path, std::process::id().to_string());
    }
}

/// The build on disk that `/proc/self/exe` names: the file as it is now, which a rebuild may have replaced.
pub fn running_build() -> Option<PathBuf> {
    let exe = std::fs::read_link("/proc/self/exe").ok()?;
    let text = exe.to_string_lossy();
    Some(PathBuf::from(text.strip_suffix(" (deleted)").unwrap_or(&text)))
}

static CHILD: AtomicI32 = AtomicI32::new(0);

// SAFETY: both declarations match the libc functions of the same names (`kill(2)` and `signal(2)`: integer arguments and result,
// the handler as a plain address), and the process links libc.
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
    fn signal(sig: i32, handler: usize) -> usize;
}

const SIGHUP: i32 = 1;
const SIGINT: i32 = 2;
const SIGQUIT: i32 = 3;
const SIGTERM: i32 = 15;
const SIG_IGN: usize = 1;

extern "C" fn forward(sig: i32) {
    let pid = CHILD.load(Ordering::Relaxed);
    if pid > 0 {
        // SAFETY: one syscall that takes two integers, which a signal handler may make.
        unsafe { kill(pid, sig) };
    }
}

/// The terminal's interrupt keys belong to the J4n the guard supervises, and a hang-up or a kill reaches that J4n too.
fn install_signals() {
    // SAFETY: `signal` is given signal numbers and a handler that only makes the one syscall above.
    unsafe {
        signal(SIGINT, SIG_IGN);
        signal(SIGQUIT, SIG_IGN);
        signal(SIGTERM, forward as *const () as usize);
        signal(SIGHUP, forward as *const () as usize);
    }
}

/// How one start ended.
#[derive(Debug)]
enum Came {
    /// It drew its first frame, and later ended with this status.
    Up(ExitStatus),
    Died(ExitStatus),
    /// It was still running without a frame when the time ran out (and was stopped).
    Silent,
    NoStart(String),
}

impl Came {
    fn words(&self) -> String {
        match self {
            Came::Up(s) | Came::Died(s) => s.code().map_or_else(|| "it was stopped by a signal".into(), |n| format!("exit {n}")),
            Came::Silent => "no first frame in time".into(),
            Came::NoStart(e) => format!("could not start: {e}"),
        }
    }
}

fn code_of(status: ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
}

fn hash_of(path: &Path) -> Option<String> {
    Some(j4n_core::text::hex_hash(std::fs::read(path).ok()?))
}

/// The guard's folder: its copy of the last build that came up, the hashes of those that did not, and the ready file of a start.
pub struct Guard {
    dir: PathBuf,
    secs: u64,
}

impl Guard {
    pub fn new(dir: &Path) -> Guard {
        Guard { dir: dir.to_path_buf(), secs: ready_secs() }
    }

    /// With this long for a build to come up.
    pub fn within(mut self, secs: u64) -> Guard {
        self.secs = secs;
        self
    }

    fn bad(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.join(BAD)).map(|t| t.lines().map(str::to_string).collect()).unwrap_or_default()
    }

    fn mark_bad(&self, build: &Path) {
        if let Some(h) = hash_of(build) {
            let mut all = self.bad();
            if !all.contains(&h) {
                all.push(h);
                let _ = std::fs::write(self.dir.join(BAD), all.join("\n"));
            }
        }
    }

    /// Keep a copy of the build that is running: if the next does not come up, this is what comes back.
    pub fn keep(&self, build: &Path) -> Option<PathBuf> {
        std::fs::create_dir_all(&self.dir).ok()?;
        let to = self.dir.join(PREVIOUS);
        let _ = std::fs::remove_file(&to);
        let mut out = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o700).open(&to).ok()?;
        std::io::copy(&mut std::fs::File::open(build).ok()?, &mut out).ok()?;
        Some(to)
    }

    fn launch(&self, build: &Path, args: &[String], note: Option<&str>) -> Came {
        let ready = self.dir.join(format!("ready-{}", std::process::id()));
        let _ = std::fs::remove_file(&ready);
        let mut cmd = Command::new(build);
        j4n_connect::proc::scrub(&mut cmd);
        // the J4n it starts is this one again, and reads its keys from the environment as this one does
        cmd.envs(j4n_connect::proc::keys());
        cmd.args(args).env("J4N_GUARD", std::process::id().to_string()).env("J4N_READY_FILE", &ready);
        match note {
            Some(n) => cmd.env("J4N_START_NOTE", n),
            None => cmd.env_remove("J4N_START_NOTE"),
        };
        let mut child: Child = match j4n_connect::proc::start(&mut cmd) {
            Ok(c) => c,
            Err(e) => return Came::NoStart(e.to_string()),
        };
        CHILD.store(child.id() as i32, Ordering::Relaxed);
        let started = Instant::now();
        let came = loop {
            match child.try_wait() {
                Ok(Some(status)) => break if ready.exists() { Came::Up(status) } else { Came::Died(status) },
                Ok(None) => {}
                Err(e) => break Came::NoStart(e.to_string()),
            }
            if !ready.exists() && started.elapsed() >= Duration::from_secs(self.secs) {
                let _ = child.kill();
                let _ = child.wait();
                break Came::Silent;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        CHILD.store(0, Ordering::Relaxed);
        let _ = std::fs::remove_file(&ready);
        came
    }

    /// [`Guard::run`] as the process's own job.
    pub fn supervise(&self, previous: PathBuf, next: &dyn Fn() -> PathBuf, args: &[String]) -> i32 {
        install_signals();
        self.run(previous, next, args)
    }

    /// Start the next build, watch it, fall back, and go on while J4ns ask for a restart. `previous` is the build that was running
    /// when the restart was asked for. What it returns is the status the guard exits with: the last J4n's own.
    pub fn run(&self, mut previous: PathBuf, next: &dyn Fn() -> PathBuf, args: &[String]) -> i32 {
        let _ = std::fs::create_dir_all(&self.dir);
        let mut note: Option<String> = None;
        loop {
            let target = next();
            let skipped = hash_of(&target).is_some_and(|h| self.bad().contains(&h));
            let came = if skipped { Came::NoStart("it did not come up before, and has not changed".into()) } else { self.launch(&target, args, note.as_deref()) };
            note = None;
            match came {
                Came::Up(s) if s.code() == Some(RESTART_CODE) => {
                    // it came up: it is the build to go back to now
                    previous = self.keep(&target).unwrap_or(previous);
                    continue;
                }
                Came::Up(s) => return code_of(s),
                failed => {
                    let why = failed.words();
                    if !skipped {
                        self.mark_bad(&target);
                    }
                    if target == previous {
                        eprintln!("j4n: {} did not start ({why}); there is no other build to go back to — the chat is in the Log, `j4n` brings it back", target.display());
                        return 1;
                    }
                    let line = format!("restart failed · {} did not start ({why}) — back on the build before it, your chat is intact; it is not tried again until it changes", target.display());
                    eprintln!("j4n: {line}");
                    match self.launch(&previous, args, Some(&line)) {
                        Came::Up(s) if s.code() == Some(RESTART_CODE) => continue,
                        Came::Up(s) => return code_of(s),
                        other => {
                            eprintln!("j4n: the build before it did not start either ({}); the chat is in the Log — run `j4n`", other.words());
                            return 1;
                        }
                    }
                }
            }
        }
    }
}
