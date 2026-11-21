//! The real binary on a real terminal. A screen test in memory (`App`, `term::drive`) cannot see the key thread
//! end while idle; this one can. Here util-linux `script` gives the binary a pty, the `test`
//! doer stands in for the model, HOME and the project are hermetic, and what the terminal is sent is read as bytes: the screen is up, it
//! reads keys after more than one idle second, it answers a line, opens the jump, takes a resize, and gives the terminal back on quit.
//! the bytes also go into a terminal in miniature (`vt`), so a test looks at the screen as the operator does — a line typed,
//! a line steered, a question answered, the tabs and sub-tabs, a command from the jump, a page's own key — and the goldens in
//! `crates/j4n/tests/tty/` are that screen at 120×36 and 80×24.

use crate::run_cases::Dirs;
use crate::support::{read_text, root};
use crate::vt::Vt;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const ALT_ON: &str = "\x1b[?1049h";
const ALT_OFF: &str = "\x1b[?1049l";

pub(crate) struct Tty {
    pub(crate) child: Child,
    keys: ChildStdin,
    seen: Arc<Mutex<Vec<u8>>>,
    resize: std::path::PathBuf,
    /// The file the shell writes its pid in: the pid of the screen, and the id of the process group the shell, its fifo reader and the screen are in.
    pid: std::path::PathBuf,
    /// The screen the bytes make, and how many of them it has been given.
    vt: Vt,
    fed: usize,
}

impl Tty {
    /// `j4n --doer test` on a 120×36 pty, in the project of `d`.
    pub(crate) fn start(d: &Dirs) -> Tty {
        Tty::with(d, "--doer test", &[])
    }

    /// `j4n ARGS` on a 120×36 pty, in the project of `d`, with `env` added to the hermetic environment.
    pub(crate) fn with(d: &Dirs, args: &str, env: &[(&str, String)]) -> Tty {
        Tty::sized(d, args, env, (120, 36))
    }

    /// `j4n ARGS` on a pty of `(cols, rows)`.
    pub(crate) fn sized(d: &Dirs, args: &str, env: &[(&str, String)], (cols, high): (usize, usize)) -> Tty {
        // a row ends in how long ago it was (`look.time = ago`), which a golden cannot hold still: the screens are kept in the time of day, unless the test says
        // `look.time` itself in `settings/time.toml`
        let time = d.j4n_home().join("settings/time.toml");
        if !time.exists() {
            fs::create_dir_all(time.parent().unwrap()).unwrap();
            fs::write(&time, "api = 1\n[[setting]]\nid = \"look.time\"\nvalue = \"clock\"\n").unwrap();
        }
        let resize = d.dir.join("resize");
        let mk = Command::new("mkfifo").arg(&resize).status().expect("mkfifo");
        assert!(mk.success(), "a fifo for the resize");
        // the shell sets the size, waits on the fifo in the background to set another, and becomes the screen
        let pid = d.dir.join("pid");
        let line = format!("echo $$ > '{3}'; stty rows {high} cols {cols}; ( read _ < '{0}'; stty rows 24 cols 80 < /dev/tty ) & exec '{1}' {2}", resize.display(), env!("CARGO_BIN_EXE_j4n"), args, pid.display());
        // a gate that runs under `nohup` starts every test with SIGHUP ignored, and the screen must still end when its terminal does
        let mut child = Command::new("sh")
            .args(["-c", "trap '' HUP; exec \"$@\"", "sh", "script", "-q", "-e", "-c", &line, "/dev/null"])
            .current_dir(d.project())
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("SHELL", "/bin/sh")
            .env("HOME", d.home())
            .env("J4N_HOME", d.j4n_home())
            .env("TMPDIR", d.dir.join("tmp"))
            .env("TERM", "xterm-256color")
            .env("J4N_FONTS", "icons")
            .env("https_proxy", "http://127.0.0.1:9")
            .envs(env.iter().map(|(k, v)| (*k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("script starts (util-linux, /usr/bin/script)");
        fs::create_dir_all(d.dir.join("tmp")).unwrap();
        let (keys, mut out) = (child.stdin.take().unwrap(), child.stdout.take().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = out.read(&mut buf) {
                if n == 0 {
                    return;
                }
                sink.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        });
        Tty { child, keys, seen, resize, pid, vt: Vt::new(cols, high), fed: 0 }
    }

    pub(crate) fn text(&self) -> String {
        String::from_utf8_lossy(&self.seen.lock().unwrap()).into_owned()
    }

    pub(crate) fn mark(&self) -> usize {
        self.seen.lock().unwrap().len()
    }

    /// What was sent to the terminal after `mark`.
    pub(crate) fn since(&self, mark: usize) -> String {
        String::from_utf8_lossy(&self.seen.lock().unwrap()[mark..]).into_owned()
    }

    /// The screen as it is now: what the terminal was sent, painted up to the end of the last whole frame. A frame comes between the
    /// sequences that tell a terminal to show it only when it is whole (DEC 2026, `frame` in `term.rs`), and a real terminal shows nothing of
    /// one until the closing sequence. The screen paints only the cells that changed, so a frame taken half-way is a screen no operator saw:
    /// a border cut short, or the rows of the frame before still standing among the rows of this one.
    pub(crate) fn screen(&mut self) -> String {
        let seen = self.seen.lock().unwrap();
        let upto = self.fed + whole_frames(&seen[self.fed..]);
        self.vt.feed(&seen[self.fed..upto]);
        self.fed = upto;
        self.vt.text()
    }

    /// Wait until the screen satisfies `ready`; it is what the operator sees, whole.
    pub(crate) fn until_screen(&mut self, what: &str, ready: impl Fn(&str) -> bool) -> String {
        let end = Instant::now() + crate::support::PATIENCE;
        loop {
            let s = self.screen();
            if ready(&s) {
                return s;
            }
            if Instant::now() >= end {
                let _ = self.child.kill();
                panic!("never saw {what}; the screen is\n{s}");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// The terminal is made 80×24; the screen is painted again at the new size.
    fn shrink(&mut self) {
        // what came before is painted at the old size; what comes after, at the new
        self.screen();
        self.vt.resize(80, 24);
        let m = self.mark();
        fs::write(&self.resize, "go\n").unwrap();
        self.until("a repaint after the resize", m, |s| s.contains("\x1b[2J"));
        self.until_screen("the screen at its new size", |s| s.lines().nth(1).is_some_and(|l| l.contains("Settings") && l.chars().count() >= 60));
    }

    pub(crate) fn send(&mut self, bytes: &[u8]) {
        self.keys.write_all(bytes).expect("the pty takes keys");
        self.keys.flush().unwrap();
    }

    /// Wait until `ready` holds for what came after `mark`.
    pub(crate) fn until(&mut self, what: &str, mark: usize, ready: impl Fn(&str) -> bool) -> String {
        let end = Instant::now() + crate::support::PATIENCE;
        loop {
            let s = self.since(mark);
            if ready(&s) {
                return s;
            }
            if Instant::now() >= end {
                let _ = self.child.kill();
                panic!("never saw {what}; after the mark the terminal was sent {s:?}");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

/// How many of `bytes` end with a whole frame: up to and with the last closing sequence of a synchronized update (DEC 2026); none when no
/// frame has been closed yet.
fn whole_frames(bytes: &[u8]) -> usize {
    const END: &[u8] = b"\x1b[?2026l";
    bytes.windows(END.len()).rposition(|w| w == END).map_or(0, |at| at + END.len())
}

/// a frame that has not been closed is not on the screen yet, and one that has is, with what came after it still held back.
#[test]
fn the_screen_of_the_tty_harness_is_taken_at_the_end_of_a_whole_frame() {
    assert_eq!(whole_frames(b""), 0);
    assert_eq!(whole_frames(b"\x1b[?2026h\x1b[1;1Hab"), 0, "a frame that is open is not taken");
    assert_eq!(whole_frames(b"\x1b[?2026h\x1b[1;1Hab\x1b[?2026l"), b"\x1b[?2026h\x1b[1;1Hab\x1b[?2026l".len(), "a closed frame is");
    let two = b"\x1b[?2026hab\x1b[?2026l\x1b[?2026hc\x1b[?2026";
    assert_eq!(whole_frames(two), b"\x1b[?2026hab\x1b[?2026l".len(), "the first is taken, the second is half sent");
}

impl Tty {
    /// The pid of the screen, once the shell has written it.
    fn screen_pid(&self) -> Option<i32> {
        fs::read_to_string(&self.pid).ok()?.trim().parse().ok()
    }

    /// Whether the process is there and not a zombie.
    fn alive(pid: i32) -> bool {
        fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| s.rsplit_once(')').is_some_and(|(_, rest)| !rest.trim_start().starts_with('Z')))
    }
}

unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

/// `script` puts its shell in a session of its own, so killing `script` does not reach the screen, and a screen that stayed behind is
/// nobody's child: the whole group goes with the handle, on a panic or a timeout too.
impl Drop for Tty {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(pid) = self.screen_pid() {
            // SAFETY: one syscall that takes two integers; a negative pid is the process group.
            unsafe { kill(-pid, 9) };
        }
    }
}

pub(crate) fn no_pty() -> bool {
    !Path::new("/usr/bin/script").exists() && !Path::new("/bin/script").exists()
}

/// A closed terminal must not leave a screen spinning: the terminal goes away under the screen (its `script` is killed and
/// nothing kills the rest, and SIGHUP is ignored), and the screen ends by itself within a second. The test ends its own child.
#[test]
fn a_screen_whose_terminal_went_away_ends_by_itself() {
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    let d = Dirs::new("tty-hup");
    let mut t = Tty::start(&d);
    t.until("the alternate screen and the first frame", 0, |s| s.contains(ALT_ON) && s.contains("Chat"));
    let pid = t.screen_pid().expect("the shell wrote its pid");
    assert!(Tty::alive(pid), "the screen is up");
    let _ = t.child.kill();
    let _ = t.child.wait();
    let end = Instant::now() + crate::support::PATIENCE;
    while Tty::alive(pid) && Instant::now() < end {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(!Tty::alive(pid), "the screen is still running {pid} after its terminal went away");
}

#[repr(C)]
struct Winsize {
    rows: u16,
    cols: u16,
    x: u16,
    y: u16,
}

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

unsafe extern "C" {
    fn openpty(master: *mut i32, slave: *mut i32, name: *mut u8, termios: *const u8, size: *const Winsize) -> i32;
    fn poll(fds: *mut PollFd, count: usize, millis: i32) -> i32;
    fn fcntl(fd: i32, cmd: i32, arg: i32) -> i32;
}

/// A child that is killed when its handle goes, so a failed assertion leaves no screen behind.
struct Reaped(Child);

impl Drop for Reaped {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A screen whose terminal went away *exits*, with a status of its own, and does not abort: a restore that prints its failure to the dead
/// tty with `eprintln!` panics, the panic hook of `init` restores (and prints, and panics) again, and the abort dumps core, so a window
/// closed on `j4n` would leave a core file. Here the child is ours, so its status is read: a signal is a failure.
#[test]
fn a_screen_whose_terminal_went_away_exits_and_does_not_abort() {
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::os::unix::process::ExitStatusExt;
    let d = Dirs::new("tty-gone");
    let (mut master, mut slave) = (-1, -1);
    let size = Winsize { rows: 36, cols: 120, x: 0, y: 0 };
    // SAFETY: two integers to write into, no name or termios wanted, and a size that lives through the call.
    assert_eq!(unsafe { openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null(), &size) }, 0, "a pty");
    // the child must not inherit the master, or the terminal never goes away (F_SETFD, FD_CLOEXEC)
    for fd in [master, slave] {
        // SAFETY: a descriptor `openpty` just made, and two integers.
        assert_eq!(unsafe { fcntl(fd, 2, 1) }, 0, "close-on-exec");
    }
    // SAFETY: both are descriptors `openpty` just made, and nothing else owns them.
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
    let tty = |fd: &OwnedFd| Stdio::from(fd.try_clone().expect("the slave is cloned"));
    let child = Command::new(env!("CARGO_BIN_EXE_j4n"))
        .args(["--doer", "test"])
        .current_dir(d.project())
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", d.home())
        .env("J4N_HOME", d.j4n_home())
        .env("TMPDIR", d.dir.join("tmp"))
        .env("TERM", "xterm-256color")
        .env("J4N_FONTS", "icons")
        .env("https_proxy", "http://127.0.0.1:9")
        .stdin(tty(&slave))
        .stdout(tty(&slave))
        .stderr(tty(&slave))
        .spawn()
        .expect("j4n starts");
    fs::create_dir_all(d.dir.join("tmp")).unwrap();
    let mut child = Reaped(child);
    drop(slave);
    // the first frame is up when the screen has been drawn
    let mut master = fs::File::from(master);
    let (mut seen, end) = (Vec::new(), Instant::now() + crate::support::PATIENCE);
    while !String::from_utf8_lossy(&seen).contains("Chat") {
        assert!(Instant::now() < end, "no first frame; the terminal was sent {:?}", String::from_utf8_lossy(&seen));
        use std::os::fd::AsRawFd;
        let mut fd = PollFd { fd: master.as_raw_fd(), events: 1, revents: 0 };
        // SAFETY: one `pollfd` that lives through the call, and a count of one.
        if unsafe { poll(&mut fd, 1, 100) } > 0 {
            let mut buf = [0u8; 8192];
            let n = master.read(&mut buf).expect("the master is read");
            seen.extend_from_slice(&buf[..n]);
        }
    }
    drop(master);
    let end = Instant::now() + crate::support::PATIENCE;
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < end, "the screen is still running after its terminal went away");
        thread::sleep(Duration::from_millis(10));
    };
    assert!(status.signal().is_none(), "the screen was ended by signal {:?} after its terminal went away; it must exit by itself", status.signal());
}

/// What a test starts goes with its handle: dropped, none of its processes is left (the screen, and the shell that waits on the fifo).
#[test]
fn a_dropped_terminal_leaves_no_process() {
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    let d = Dirs::new("tty-drop");
    let t = Tty::start(&d);
    let pid = loop {
        if let Some(p) = t.screen_pid() {
            break p;
        }
        thread::sleep(Duration::from_millis(10));
    };
    drop(t);
    let end = Instant::now() + crate::support::PATIENCE;
    while Tty::alive(pid) && Instant::now() < end {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(!Tty::alive(pid), "{pid} is still running after the handle was dropped");
}

#[test]
fn the_real_screen_reads_keys_after_it_has_been_idle_answers_jumps_resizes_and_gives_the_terminal_back() {
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    let d = Dirs::new("tty");
    let mut t = Tty::start(&d);
    t.until("the alternate screen and the first frame", 0, |s| s.contains(ALT_ON) && s.contains("Chat"));

    // more than one idle second: the keys are still read after an idle poll
    thread::sleep(Duration::from_millis(1600));
    t.send(b"who");
    t.until_screen("the typed line in the composer", |s| s.lines().any(|l| l.starts_with("│ who")));
    t.send(b"\r");
    // the doer answers with a text the line does not hold
    let shown = t.until_screen("the doer's answer on pipes, in a lane", |s| s.contains("child") && s.contains("turn"));
    // the core's rows, turn › lane › step: the lane is drawn on a pipe, not as a line of plain chat
    assert!(shown.contains("╰─") || shown.contains("├─"), "no pipe in\n{shown}");

    // ⌃k opens the jump
    t.send(&[0x0b]);
    t.until_screen("the jump", |s| s.contains("jump") && s.contains("esc close"));
    t.send(b"\x1b");
    t.until_screen("the jump closed", |s| !s.contains("esc close"));

    // a resize reaches the loop: the terminal is painted again at its new size, and the keys go on being read
    t.shrink();
    t.send(b"ok");
    t.until_screen("keys after the resize", |s| s.lines().any(|l| l.starts_with("│ ok")));

    // the terminal's own background was set to the palette's once, while the screen was up, and is still ours
    let ground = j4n_tui::term::ground_to((0x16, 0x1a, 0x20));
    assert_eq!(t.text().matches(&ground).count(), 1, "OSC 11, once: {:?}", t.text());
    assert!(!t.text().contains(j4n_tui::term::GROUND_BACK), "not given back while the screen is up");

    // ⌃c leaves, and the terminal is given back
    let m = t.mark();
    t.send(&[0x03]);
    t.until("the terminal given back", m, |s| s.contains(ALT_OFF));
    let text = t.text();
    let back = text.find(j4n_tui::term::GROUND_BACK).expect("its own background given back (OSC 111)");
    assert!(back > text.find(&ground).expect("set") && back < text.rfind(ALT_OFF).expect("left the alternate screen"), "OSC 111 before the screen is left: {text:?}");
    let end = Instant::now() + crate::support::PATIENCE;
    let status = loop {
        if let Some(s) = t.child.try_wait().unwrap() {
            break s;
        }
        assert!(Instant::now() < end, "j4n did not end after ⌃c; the terminal was sent {:?}", t.text());
        thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success(), "j4n left with {status:?}");
}

/// What the start has to say is on the screen, not only on stderr, which the alternate screen covers: a home file that is left out
/// says so where the operator looks.
#[test]
fn the_notices_of_the_start_are_the_first_lines_of_the_real_screen() {
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    let d = Dirs::new("tty-notices");
    fs::create_dir_all(d.j4n_home().join("settings")).unwrap();
    fs::write(d.j4n_home().join("settings/mine.toml"), "api = 0\n[[setting]]\nid = \"compact.at\"\nvalue = 0.1\n").unwrap();
    let mut t = Tty::start(&d);
    let shown = t.until_screen("the notice of the start", |s| s.contains("left out"));
    // the words as the operator reads them: a notice is CommonMark on the screen, so the code span is drawn without its backticks; the
    // backticks are only in the line the binary prints to the terminal before the screen opens
    assert!(shown.contains("home:settings/mine.toml") && shown.contains("needs api = 1"), "{shown}");
    t.send(&[0x03]);
}

/// A line that opens with a colon and names no command is answered on the screen and never reaches the doer. The `test` doer
/// answers every line it gets with "child 1 turn 1" (a step, since a turn with one lane has no lane row), so a line that went to it would show that; the same doer answers the next plain line.
#[test]
fn a_colon_line_that_names_no_command_is_answered_on_the_real_screen_and_not_sent_to_the_doer() {
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    let d = Dirs::new("tty-colon");
    let mut t = Tty::start(&d);
    t.until_screen("the first frame", |s| s.contains("Chat"));
    t.send(b":set check.after true\r");
    let shown = t.until_screen("the answer to the unknown command", |s| s.contains("no command :set"));
    assert!(shown.contains(":settings") && !shown.contains(":new"), "{shown}");
    thread::sleep(Duration::from_millis(1200));
    let later = t.screen();
    assert!(!later.contains("child"), "a doer answered the line:\n{later}");
    t.send(b"who\r");
    t.until_screen("the doer answering a plain line", |s| s.contains("child"));
    t.send(&[0x03]);
}

/// a line is steered. The doer is held in a `hang`; `⌃t` aims the composer at its lane, and the next line goes to that lane, not to the
/// thread: the doer says it was steered.
#[test]
fn a_line_steers_the_lane_chosen_with_ctrl_t_on_the_real_screen() {
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    let d = Dirs::new("tty-steer");
    let mut t = Tty::start(&d);
    t.until_screen("the first frame", |s| s.contains("Chat"));
    t.send(b"hang\r");
    // a lone lane has no row: its clock stands at the right of the operator's line
    t.until_screen("a lane at work", |s| s.lines().any(|l| l.contains("hang") && l.split_whitespace().any(|w| w.len() > 1 && w.ends_with('s') && w[..w.len() - 1].chars().all(|c| c.is_ascii_digit()))));
    t.send(&[0x14]);
    t.until_screen("the line aimed at the lane", |s| s.lines().any(|l| l.starts_with('│') && l.contains('→')));
    t.send(b"go faster\r");
    t.until_screen("the doer saying it was steered", |s| s.contains("steered: go faster"));
    t.send(&[0x03]);
}

/// the tabs and the sub-tabs. `⌥2` and `F4` move among the tabs, `⌥→` among the sub-tabs of the Agents tab, and each shows its own page.
/// `⌥2` is sent as a terminal without the kitty protocol sends it, `esc` then `2`, and is still the second tab.
#[test]
fn the_tabs_and_the_sub_tabs_of_the_real_screen_move_and_show_their_pages() {
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    let d = Dirs::new("tty-tabs");
    let mut t = Tty::start(&d);
    t.until_screen("the first frame", |s| s.contains("Chat"));
    t.send(b"\x1b2");
    t.until_screen("the Tasks tab", |s| !s.lines().any(|l| l.starts_with('╭')));
    t.send(b"\x1bOS");
    let accounts = t.until_screen("the Agents tab with its sub-tabs", |s| s.contains("Providers") && s.contains("Trust"));
    assert!(accounts.lines().nth(1).is_some_and(|l| l.contains("Agents")), "{accounts}");
    // the sub-tab whose title is underlined, in the strip of the sub-tabs
    let under = |s: &str, word: &str| {
        let rows: Vec<&str> = s.lines().collect();
        let col = rows.get(3).and_then(|r| r.find(word)).map(|b| rows[3][..b].chars().count());
        col.is_some_and(|c| rows.get(4).and_then(|r| r.chars().nth(c)) == Some('━'))
    };
    // the sub-tab to the right of Setup: Overview
    t.send(b"\x1b[1;3C");
    t.until_screen("the Overview page", |s| under(s, "Overview"));
    // the key of the open tab goes on to the next sub-tab: the Lab
    t.send(b"\x1bOS");
    t.until_screen("the Lab", |s| under(s, "Lab"));
    t.send(&[0x03]);
}

/// The harness plays a terminal that speaks the kitty keyboard protocol: it answers the screen's question with the flags it has and its
/// device attributes, as foot does, and from then on sends each key as the protocol says.
fn speaks_kitty(t: &mut Tty) {
    t.until("the question of the kitty protocol", 0, |s| s.contains("\x1b[?u"));
    t.send(b"\x1b[?0u\x1b[?62c");
}

/// Under the full flags (`keys.hold = auto`) every key comes as a code, so what is typed must still be what the key makes: the bytes are
/// the ones foot sends with every key as a code and the shifted key beside it, each key's release after it, and ⇧ pressed and let go
/// around the shifted ones. Then ⌥ held alone shows the tabs' numbers beside them, and its release hides them.
#[test]
fn under_the_full_kitty_flags_typing_is_unchanged_and_a_held_alt_shows_the_numbers_on_the_real_screen() {
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    let d = Dirs::new("tty-hold");
    let mut t = Tty::start(&d);
    speaks_kitty(&mut t);
    t.until("every key asked for as a code, with the shifted key beside it", 0, |s| s.contains("\x1b[>15u"));
    t.until_screen("the first frame", |s| s.contains("Chat"));
    // x; ⇧; is `:` and ⇧1 is `!`; ⇧é is `É`; AltGr+q on a German layout is `@` (the AltGr symbol is the key, q the base); Caps Lock + a is `A`
    t.send(b"\x1b[120u\x1b[120;1:3u");
    t.send(b"\x1b[57441;2u\x1b[59:58;2u\x1b[59:58;2:3u\x1b[49:33;2u\x1b[49:33;2:3u\x1b[233:201;2u\x1b[233:201;2:3u\x1b[57441;1:3u");
    t.send(b"\x1b[64::113u\x1b[64::113;1:3u\x1b[97:65;65u\x1b[97:65;65:3u");
    // traced from foot's source, not seen from it: y with Num Lock on; a composed É (a dead key, then ⇧e) as its own code with ⇧ and no
    // shifted key; Caps Lock with ⇧ + a, whose shifted key is the small one and so not sent
    t.send(b"\x1b[121;129u\x1b[201;2u\x1b[97;66u");
    let line = |s: &str| s.lines().find(|l| l.starts_with("│ x")).map(String::from).unwrap_or_default();
    let shown = t.until_screen("the typed line", |s| line(s).contains("yÉa") || line(s).contains("yÉA"));
    assert!(line(&shown).starts_with("│ x:!É@AyÉa"), "what the keys make, and no release typed again:\n{shown}");
    // ⌥ pressed alone, held: the small numbers beside the tabs; let go: gone, and no tab moved
    let strip = |s: &str| s.lines().nth(1).map(String::from).unwrap_or_default();
    let before = strip(&shown);
    t.send(b"\x1b[57443;3u");
    let held = t.until_screen("the numbers while ⌥ is held", |s| strip(s).contains('¹') && strip(s).contains('²'));
    let plain: String = strip(&held).chars().map(|c| if "¹²³⁴⁵⁶⁷⁸⁹".contains(c) { ' ' } else { c }).collect();
    assert_eq!(plain, before, "the numbers stand on blank cells: no tab moved");
    t.send(b"\x1b[57443;1:3u");
    t.until_screen("the numbers gone with ⌥'s release", |s| !strip(s).contains('¹'));
    let m = t.mark();
    t.send(b"\x1b[99;5u");
    t.until("the terminal given back, its flags too", m, |s| s.contains(ALT_OFF) && s.contains("\x1b[<1u"));
}

/// `keys.hold = off` keeps the two flags the keys always had, on a terminal that would give more.
#[test]
fn with_keys_hold_off_the_real_screen_asks_only_for_the_two_flags_it_always_did() {
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    let d = Dirs::new("tty-hold-off");
    fs::create_dir_all(d.j4n_home().join("settings")).unwrap();
    fs::write(d.j4n_home().join("settings/keys.toml"), "api = 1\n[[setting]]\nid = \"keys.hold\"\nvalue = \"off\"\n").unwrap();
    let mut t = Tty::start(&d);
    speaks_kitty(&mut t);
    t.until("the two flags", 0, |s| s.contains("\x1b[>3u"));
    t.until_screen("the first frame", |s| s.contains("Chat"));
    t.send(b"x:");
    t.until_screen("a line typed as text", |s| s.lines().any(|l| l.starts_with("│ x:")));
    assert!(!t.text().contains("\x1b[>15u"), "no more than the two");
    t.send(&[0x03]);
}

/// a command is a word on the line, and it is also in the jump. `:library` typed opens its page; `⌃k` finds `:pack` and says it in the
/// chat, where the packs are listed; `:remember`, which wants a note, leaves its word in the composer to be finished.
#[test]
fn a_command_is_typed_or_found_with_ctrl_k_on_the_real_screen() {
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    let d = Dirs::new("tty-commands");
    // the standard pty: what the jump said is looked for in the Log, not on a screen whose height every pack takes with its rows
    let mut t = Tty::start(&d);
    t.until_screen("the first frame", |s| s.contains("Chat"));
    t.send(b":library\r");
    t.until_screen("the Agents tab, opened by the word", |s| !s.lines().any(|l| l.starts_with('╭')) && s.contains("Providers"));
    // from the Agents tab, the jump finds `:pack` among the nodes and says it in the chat
    t.send(&[0x0b]);
    t.send(b":pack");
    let jump = t.until_screen("the command in the jump", |s| s.contains(":pack :packs") && s.contains("List the packs"));
    assert!(jump.contains("jump"), "{jump}");
    t.send(b"\r");
    // the packs were listed: the tail of the list is on the screen, whatever its height
    t.until_screen("the packs listed in the chat", |s| s.contains("0.0.0 on") || s.contains("0.0.0 off"));
    let end = Instant::now() + crate::support::PATIENCE;
    while !crate::run_cases::log_events(&d).iter().any(|e| e["kind"] == "chat.say" && e["data"]["text"] == ":pack") {
        assert!(Instant::now() < end, "the Log never held the word the jump said: {:?}", crate::run_cases::log_events(&d).iter().map(|e| e["kind"].clone()).collect::<Vec<_>>());
        thread::sleep(Duration::from_millis(20));
    }
    // a command that wants more leaves its word in the composer
    t.send(&[0x0b]);
    t.send(b":remember");
    t.until_screen("the command that takes a note", |s| s.contains("<the note>"));
    t.send(b"\r");
    t.until_screen("its word in the composer", |s| s.lines().any(|l| l.starts_with("│ :remember")));
    t.send(&[0x03]);
}

/// a page's own key. The operator's `ui/keys.toml` gives the Agents page (`:roster` opens it) a key that sends an intent; pressed there, it
/// takes the screen to the chat.
#[test]
fn a_page_of_the_real_screen_has_a_key_of_its_own() {
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    let d = Dirs::new("tty-pagekey");
    fs::create_dir_all(d.j4n_home().join("ui")).unwrap();
    fs::write(d.j4n_home().join("ui/keys.toml"), "api = 1\n[[key]]\nid = \"page:agents/home\"\nkeys = [\"x\"]\nrun = { intent = \"open\", node = \"chat\" }\n").unwrap();
    let mut t = Tty::start(&d);
    t.until_screen("the first frame", |s| s.contains("Chat"));
    t.send(b":roster\r");
    t.until_screen("the Agents page", |s| !s.lines().any(|l| l.starts_with('╭')) && s.contains("Tools"));
    t.send(b"x");
    t.until_screen("the chat, where the page's key sent us", |s| s.lines().any(|l| l.starts_with('╭')));
    t.send(&[0x03]);
}

/// with `chats.on` the first screen is the orchestrator: the crumb row (the project's name, no word `main`, the counts at the
/// right) over the pinned list of the chats, and the orchestrator's own rows under it (your line, `→ c1 · …` the route). A chat is entered from the list
/// (`:chats`, `⏎`) and its crumb begins with the project's name too.
#[test]
fn with_the_chats_on_the_real_screen_pins_the_list_and_a_chat_has_a_crumb() {
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    let d = Dirs::new("tty-chats");
    fs::create_dir_all(d.j4n_home().join("settings")).unwrap();
    fs::write(d.j4n_home().join("settings/chats.toml"), "api = 1\n[[setting]]\nid = \"chats.on\"\nvalue = true\n[[setting]]\nid = \"need.waived\"\nvalue = \"chats\"\n").unwrap();
    let mut t = Tty::start(&d);
    let first = t.until_screen("the first frame", |s| s.contains("⌥1–9 tabs") && s.contains("no chats yet"));
    assert!(first.lines().any(|l| l.trim_start().starts_with("project") && l.trim_end().ends_with("0 chats")), "the first cell is the project's name: {first}");
    t.send(b"fix trailing commas in parser.rs\r");
    t.until_screen("the first chat", |s| s.contains("→ c1 · new chat · none matched") && s.contains("test"));
    t.send(b"what does the budget gate check\r");
    // no Decider is on in a fresh home and the line is none of c1's: the operator is asked which chat takes it, and `2` is a new chat
    t.until_screen("the question of which chat takes the line", |s| s.contains("which chat takes this?") && s.contains("new chat"));
    t.send(b"2");
    t.until_screen("the second chat", |s| s.contains("c2") && s.contains("2 chats"));
    thread::sleep(Duration::from_millis(800));
    let top = t.screen();
    assert!(!top.lines().any(|l| l.contains(" main")), "no title main: {top}");
    golden("chats-main", (120, 36), &top);
    // the list of the chats, and into the first row: the crumb begins with the project's name and the chat
    t.send(b":chats\r");
    t.until_screen("the list of the chats", |s| s.contains("find") && s.contains("⏎ jump") && s.lines().any(|l| l.contains("▸") && l.contains("c2 what does")));
    t.send(b"\r");
    t.until_screen("the chat", |s| s.lines().any(|l| l.contains("› c2 what does the budget gate check")));
    thread::sleep(Duration::from_millis(500));
    golden("chats-sub", (120, 36), &t.screen());
    // esc on an empty line goes up
    t.send(b"\x1b");
    t.until_screen("the orchestrator again", |s| s.contains("2 chats") && !s.lines().any(|l| l.contains("› c2")));
    // `↓` on the empty line puts the keys on the list (the row says its keys, four seconds would hand them back), `⏎` goes in, and `←` on the empty
    // line of the chat comes back
    t.send(b"\x1b[B");
    t.until_screen("the list has the keys", |s| s.contains("esc back"));
    thread::sleep(Duration::from_millis(500));
    golden("chats-pick", (120, 36), &t.screen());
    t.send(b"\r");
    t.until_screen("a chat, with its way back said", |s| s.lines().any(|l| l.contains(" › c")) && s.contains("← main"));
    t.send(b"\x1b[D");
    t.until_screen("the orchestrator again, by `←`", |s| s.contains("2 chats") && !s.lines().any(|l| l.contains(" › c")));
    // `:archive c1` takes the chat out of the list, whose last row is `archived (1)`, closed; `↓ ↓ ⏎` opens it and shows the chat
    t.send(b":archive c1\r");
    t.until_screen("the archive row", |s| s.lines().any(|l| l.contains("archived (1)") && l.trim_end().ends_with('▸')) && !s.lines().any(|l| l.starts_with(" ├─") && l.contains("c1 fix trailing") || l.starts_with(" ╰─") && l.contains("c1 fix trailing")));
    thread::sleep(Duration::from_millis(500));
    golden("chats-archive", (120, 36), &t.screen());
    // one key at a time: keys that come together are a notch of the wheel
    t.send(b"\x1b[B");
    t.until_screen("the list has the keys", |s| s.contains("esc back"));
    t.send(b"\x1b[B");
    thread::sleep(Duration::from_millis(300));
    t.send(b"\r");
    t.until_screen("the archive open", |s| s.lines().any(|l| l.contains("archived (1)") && l.trim_end().ends_with('▾')) && s.lines().any(|l| l.starts_with("   ╰─") && l.contains("c1 fix trailing")));
    thread::sleep(Duration::from_millis(500));
    golden("chats-archive-open", (120, 36), &t.screen());
    t.send(&[0x03]);
}

/// the look. The operator's file sets `theme.frame` and `theme.marks`; the core sends them as `Down::Theme` when the screen says
/// hello, and the chat is drawn in the gutter look in words: the turn's `you` stands in the column, and there are no pipes.
#[test]
fn the_look_the_operator_chose_is_the_look_of_the_real_screen() {
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    let d = Dirs::new("tty-look");
    fs::create_dir_all(d.j4n_home().join("ui")).unwrap();
    let set = |id: &str, ty: &str, def: &str, value: &str| format!("[[setting]]\nid = \"{id}\"\ndoc = \"x\"\nty = \"{ty}\"\ndefault = {def}\nvalue = {value}\n\n");
    let file = format!("api = 1\n{}{}", set("theme.frame", "str", "\"pipes\"", "\"gutter\""), set("theme.marks", "str", "\"icons\"", "\"words\""));
    fs::write(d.j4n_home().join("ui/look.toml"), file).unwrap();
    let mut t = Tty::start(&d);
    t.until_screen("the first frame", |s| s.contains("⌥1–9 tabs"));
    t.send(b"who\r");
    let shown = t.until_screen("the turn in the gutter", |s| s.lines().any(|l| l.starts_with("      you  ") && l.contains("who")) && s.contains("child"));
    assert!(!shown.lines().any(|l| l.contains("├─") || (l.contains("╰─") && !l.starts_with('╰'))), "pipes in the gutter look:\n{shown}");
    t.send(&[0x03]);
}

/// On the real screen a row ends in how long ago it was, and it moves on with no key and no event: the word of the turn at its right edge is
/// `now` or a second or two when it is said (before the right cap of the bracket head), and a few seconds older a little later. (The goldens above keep the time of day: `look.time = clock`.)
#[test]
fn a_row_of_the_real_screen_ages_by_itself() {
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    let d = Dirs::new("tty-age");
    fs::create_dir_all(d.j4n_home().join("settings")).unwrap();
    fs::write(d.j4n_home().join("settings/time.toml"), "api = 1\n[[setting]]\nid = \"look.time\"\nvalue = \"ago\"\n").unwrap();
    let mut t = Tty::start(&d);
    t.until_screen("the first frame", |s| s.contains("⌥1–9 tabs"));
    t.send(b"who\r");
    // the last word of the line of the turn: `now`, or a few seconds
    let last = |s: &str| s.lines().find(|l| l.contains("who") && !l.contains("tabs")).and_then(|l| l.trim_end_matches('▐').split_whitespace().last()).map(String::from).unwrap_or_default();
    let secs = |w: &str| w.strip_suffix('s').and_then(|n| n.parse::<u64>().ok()).or((w == "now").then_some(0));
    let first = t.until_screen("the turn with its age", |s| secs(&last(s)).is_some());
    let was = secs(&last(&first)).unwrap();
    let later = t.until_screen("the age moved on, with no key", |s| secs(&last(s)).is_some_and(|n| n >= was + 3));
    assert!(!later.contains("##:##") && !last(&later).contains(':'), "no time of day in a row:\n{later}");
    t.send(&[0x03]);
}

/// A screen without what changes from one run to the next: a clock or a length of time has its digits worn down to `#`, so the line keeps
/// its width.
fn steady(screen: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = screen.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() && (i == 0 || !chars[i - 1].is_alphanumeric() && chars[i - 1] != '$' && chars[i - 1] != '.') {
            let mut j = i;
            while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == ':' || chars[j] == '.') {
                j += 1;
            }
            let run: String = chars[i..j].iter().collect();
            let clock = run.contains(':') && !run.ends_with(':') && !run.ends_with('.');
            let length = chars.get(j) == Some(&'s') && !chars.get(j + 1).is_some_and(|c| c.is_alphanumeric());
            if clock || length {
                out.extend(run.chars().map(|c| if c.is_ascii_digit() { '#' } else { c }));
                i = j;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// The golden of the real screen called `name`: `crates/j4n/tests/tty/<name>.<cols>x<rows>.txt`. `J4N_BLESS=1` writes it.
fn golden(name: &str, (cols, rows): (usize, usize), screen: &str) {
    let path = root().join("crates/j4n/tests/tty").join(format!("{name}.{cols}x{rows}.txt"));
    let shown = steady(screen);
    let lines = shown.lines().count();
    assert_eq!(lines, rows, "{name}: the screen has {rows} rows");
    if std::env::var_os("J4N_BLESS").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, format!("{shown}\n")).unwrap();
        return;
    }
    let want = read_text(&path).unwrap_or_else(|| panic!("no golden {}; J4N_BLESS=1 writes it\n{shown}", path.display()));
    assert_eq!(want.trim_end_matches('\n'), shown, "{name} at {cols}×{rows}: the real screen is not its golden ({}); J4N_BLESS=1 writes it", path.display());
}

/// Whether the golden `name` holds `screen` (a bless run has nothing to hold it to, and takes any screen).
fn is_golden(name: &str, (cols, rows): (usize, usize), screen: &str) -> bool {
    if std::env::var_os("J4N_BLESS").is_some() {
        return true;
    }
    let path = root().join("crates/j4n/tests/tty").join(format!("{name}.{cols}x{rows}.txt"));
    read_text(&path).is_some_and(|want| want.trim_end_matches('\n') == steady(screen))
}

/// The sizes of a session's goldens (`fresh`, `turn`, `jump`).
const SESSIONS: [(usize, usize); 3] = [(120, 36), (80, 24), (60, 20)];

const RIGHT: &[u8] = b"\x1b[1;3C";
// the tabs with tabs.all (`Dirs::new`): Chat F1, Tasks F2, Usage F3, Agents F4 (Setup · Overview · Lab · Accounts · Add account · List · then the
// Library's pages and the tools), Grow F5, Log F6, Settings F7 (no more with these packs on; the Inbox, Habits, Kit and Report pages wait for their
// switches, and a sub-tab that is not there is not stepped to)
const PAGES: [(&str, &[u8]); 34] = [
    ("tasks", b"\x1bOQ"),
    ("asks", RIGHT),
    ("spend", b"\x1bOR"),
    ("connect-setup", b"\x1bOS"),
    ("connect-connect", RIGHT),
    ("connect-lab", RIGHT),
    ("connect-accounts", RIGHT),
    ("connect-account-add", RIGHT),
    ("tools-agents", RIGHT),
    ("accounts-models", RIGHT),
    ("accounts-providers", RIGHT),
    ("accounts-plans", RIGHT),
    ("accounts-trust", RIGHT),
    ("tools-tools", RIGHT),
    ("tools-services", RIGHT),
    ("tools-mcp", RIGHT),
    ("tools-deciders", RIGHT),
    ("tools-skills", RIGHT),
    ("tools-reflexes", RIGHT),
    ("grow", b"\x1b[15~"),
    ("contribute", RIGHT),
    ("grow-releases", RIGHT),
    ("grow-notes", RIGHT),
    ("log", b"\x1b[17~"),
    ("settings-features", b"\x1b[18~"),
    ("settings-look", RIGHT),
    ("settings-chat", RIGHT),
    ("settings-all", RIGHT),
    ("settings-keys", RIGHT),
    ("settings-loop", RIGHT),
    ("settings-gates", RIGHT),
    ("packs", RIGHT),
    ("settings-commands", RIGHT),
    ("chat", b"\x1bOP"),
];

/// One session of the real binary at `size`, and the screen at three places in it: the first frame, after a turn, and with a command found
/// in the jump.
fn session_goldens(size: (usize, usize)) {
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    let d = Dirs::new(&format!("tty-golden-{}", size.0));
    let mut t = Tty::sized(&d, "--doer test", &[], size);
    t.until_screen("the first frame", |s| s.contains("⌥1–9 tabs"));
    thread::sleep(Duration::from_millis(300));
    golden("fresh", size, &t.screen());

    t.send(b"who\r");
    t.until_screen("the turn done", |s| s.contains("child") && s.contains("⌥1–9 tabs"));
    thread::sleep(Duration::from_millis(500));
    golden("turn", size, &t.screen());

    t.send(&[0x0b]);
    t.send(b":pack");
    // the row is found before the last letter is typed: wait for the letter too, or the golden is taken a key short (a frame that reaches
    // the terminal in two pieces is not taken in the middle: `screen` stops at the end of the last whole one)
    t.until_screen("the command in the jump", |s| s.contains(":pack :packs") && s.contains("› :pack") && s.contains("esc close"));
    golden("jump", size, &t.screen());
    t.send(&[0x03]);
}

/// one golden per page of the F1 tree, from the real binary at 120×36: the F-keys open the tabs, `⌥→` the sub-tabs, and a page is
/// taken once the screen has changed, held still and is its golden.
#[test]
fn every_page_of_the_f1_tree_is_its_golden() {
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    let d = Dirs::new("tty-pages");
    let mut t = Tty::with(&d, "--doer test", &[]);
    t.until_screen("the first frame", |s| s.contains("Chat") && s.contains("⌥1–9 tabs"));
    thread::sleep(Duration::from_millis(300));
    let mut shown = t.screen();
    for (name, keys) in PAGES {
        t.send(keys);
        let (mut last, mut held, end) = (String::new(), 0, Instant::now() + crate::support::PATIENCE);
        loop {
            let s = t.screen();
            held = if s != shown && s == last { held + 1 } else { 0 };
            // the rows of a page come from the core a moment after its frame, and on a loaded machine that moment is long: a screen that
            // has held still is the page when it is its golden; one that holds still and is not is reported with its difference once
            // the patience is out, never taken for the page
            if held >= 3 && (name == "chat" || is_golden(&format!("page-{name}"), (120, 36), &s)) {
                break;
            }
            if Instant::now() >= end && held >= 3 {
                break;
            }
            assert!(Instant::now() < end, "{name}: the screen did not change and hold still:\n{s}");
            last = s;
            thread::sleep(Duration::from_millis(80));
        }
        if name != "chat" {
            golden(&format!("page-{name}"), (120, 36), &last);
        }
        shown = last;
    }
    t.send(&[0x03]);
}

/// At 50 columns the strip has no room for words, so the tabs are their icons (the marks of the unicode tier here, so the
/// golden is plain text); a page with no rows says so, and a page's help is its last line.
#[test]
fn a_narrow_strip_shows_icons_and_a_page_with_no_rows_says_so() {
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    let d = Dirs::new("tty-narrow");
    fs::create_dir_all(d.j4n_home().join("ui")).unwrap();
    fs::write(d.j4n_home().join("ui/look.toml"), "api = 1\n[[setting]]\nid = \"theme.marks\"\ndoc = \"x\"\nty = \"str\"\ndefault = \"icons\"\nvalue = \"unicode\"\n").unwrap();
    let mut t = Tty::sized(&d, "--doer test", &[], (50, 18));
    t.until_screen("the first frame", |s| s.contains("⌥1–9 tabs"));
    thread::sleep(Duration::from_millis(300));
    let strip = t.screen();
    assert!(!strip.lines().nth(1).unwrap_or("").contains("Chat"), "words in a strip with no room for them:\n{strip}");
    golden("narrow-strip", (50, 18), &strip);
    t.send(b"\x1bOQ");
    t.send(b"\x1b[1;3C");
    t.until_screen("the help of the Decisions page, its last line", |s| s.lines().nth(16).is_some_and(|l| l.starts_with("the questions that are open")));
    t.send(b"\x1b[1;3D");
    let none = t.until_screen("the Tasks page, with no rows, saying so", |s| s.contains("nothing here"));
    thread::sleep(Duration::from_millis(300));
    golden("narrow-empty", (50, 18), &t.screen());
    assert!(none.lines().nth(5).is_some_and(|l| l.contains("nothing here")), "the empty line is the first row of the page, under the strip of its sub-tabs:\n{none}");
    t.send(&[0x03]);
}

/// a row of Settings › Look has a key of its own: `n` takes the next word of the part the cursor is on, as `:look <part> next`.
#[test]
fn a_row_of_the_look_page_changes_with_its_own_key() {
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    let d = Dirs::new("tty-lookkey");
    let mut t = Tty::start(&d);
    t.until_screen("the first frame", |s| s.contains("Chat") && s.contains("⌥1–9 tabs"));
    t.send(b"\x1b[18~");
    t.until_screen("the Features page, the first of Settings", |s| s.contains("packs.full"));
    t.send(RIGHT);
    t.until_screen("the Look page", |s| s.contains("theme.corners") && s.contains("round"));
    t.send(b"n");
    t.until_screen("corners square", |s| s.lines().any(|l| l.contains("theme.corners") && l.contains("square")));
    t.send(&[0x03]);
}

#[test]
fn the_real_screen_at_120_by_36_is_its_golden() {
    session_goldens(SESSIONS[0]);
}

#[test]
fn the_real_screen_at_80_by_24_is_its_golden() {
    session_goldens(SESSIONS[1]);
}

/// the narrow screen too (60 columns), where the strip keeps its names and a row still ends a cell before the edge.
#[test]
fn the_real_screen_at_60_by_20_is_its_golden() {
    session_goldens(SESSIONS[2]);
}

/// the operator's look (`theme.you = mark`) on the real screen: the line starts at the left edge, in no person's mark, and the rows
/// of its turn hang from it on their pipes. The line is on the screen as it is sent: the golden is taken once the doer has answered.
#[test]
fn the_real_screen_in_the_mark_look_has_your_line_at_the_edge_with_no_mark() {
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    let d = Dirs::new("tty-mark");
    let mut t = Tty::sized(&d, "--doer test", &[], (80, 24));
    t.until_screen("the first frame", |s| s.contains("⌥1–9 tabs"));
    t.send(b":look you mark\r");
    t.until_screen("the look is the mark", |s| s.contains("you") && s.contains("mark"));
    t.send(b"who\r");
    let shown = t.until_screen("the turn done", |s| s.contains("child") && s.contains("⌥1–9 tabs") && s.lines().any(|l| l.starts_with("who")));
    assert!(!shown.contains('\u{F0013}'), "no person on the real screen: {shown}");
    thread::sleep(Duration::from_millis(500));
    golden("turn-mark", (80, 24), &t.screen());
    t.send(&[0x03]);
}

/// The Connect tab with two connections (an agent and a tool), a program found on this machine, and one lab run, from the real binary:
/// Connect and Lab, each shut and open, at the two sizes of the sweep and of the tty goldens.
const CONNECT_SHOTS: [&str; 4] = ["connect-rows", "connect-open", "connect-lab", "connect-lab-open"];
const CONNECT_SIZES: [(usize, usize); 2] = [(120, 36), (160, 44)];

#[test]
fn the_connect_tab_and_the_lab_with_two_connections_and_a_run_are_their_goldens() {
    use std::os::unix::fs::PermissionsExt;
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    for size in CONNECT_SIZES {
        let d = Dirs::new(&format!("tty-connect-{}", size.0));
        // the programs are in the home's own folder, which is on the PATH of the run: two are connected, one is only found
        let bin = d.home().join(".local/bin");
        fs::create_dir_all(&bin).unwrap();
        for name in ["mytool", "lint", "newagent"] {
            fs::write(bin.join(name), "#!/bin/sh\necho ready\n").unwrap();
            fs::set_permissions(bin.join(name), fs::Permissions::from_mode(0o755)).unwrap();
        }
        let conn = j4n_core::gate::grant::here_dir(&d.j4n_home(), &d.project()).join("connectors");
        fs::create_dir_all(&conn).unwrap();
        fs::write(conn.join("mytool.toml"), "api = 1\n[[connector]]\nid = \"mytool\"\ndoc = \"A small agent.\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"mytool\"\nstart = [\"{prompt}\"]\n").unwrap();
        fs::write(conn.join("lint.toml"), "api = 1\n[[connector]]\nid = \"lint\"\ndoc = \"A linter.\"\nis = \"tool\"\ndialect = \"text\"\nprogram = \"lint\"\nargs = [{ name = \"path\", required = true }]\nargv = [\"lint\", \"{path}\"]\nrisk = \"read\"\ncanary = { goal = \"src\", expect = \"ready\" }\n").unwrap();
        // one lab run, its two files with a fixed time (2025-03-04 12:00 UTC)
        let run = d.j4n_home().join("lab/run-a");
        fs::create_dir_all(run.join("seed-0")).unwrap();
        let row = |seed: u32, held: &str, check: &str, result: &str| format!("{{\"axes\":{{}},\"check\":\"{check}\",\"drafter\":\"haiku\",\"held\":[{held}],\"result\":\"{result}\",\"run\":\"run-a\",\"seed\":{seed},\"why\":\"\"}}\n");
        let rows: String = [(0, "", "pass", "pass"), (1, "", "pass", "fail"), (2, "\"c:server\"", "pass", "pass"), (3, "\"c:server\"", "fail", "fail")].iter().map(|(s, h, a, b)| row(*s, h, "connected", a) + &row(*s, h, "chats", b)).collect();
        fs::write(run.join("rows.jsonl"), rows).unwrap();
        fs::write(run.join("table.txt"), "lab run run-a: 4 seeds, drafter haiku, spent $0.04\n\naxis value  n  connected  chats\na:env        2        2/2    1/2\na:flag       2        1/2    1/2\nSEEN         2        2/2    1/2\nHELD         2        1/2    1/2\n").unwrap();
        let at = std::time::UNIX_EPOCH + Duration::from_secs(1_741_089_600);
        for f in ["rows.jsonl", "table.txt"] {
            fs::File::options().write(true).open(run.join(f)).unwrap().set_modified(at).unwrap();
        }
        let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
        let mut t = Tty::sized(&d, "--doer test", &[("PATH", path)], size);
        t.until_screen("the first frame", |s| s.contains("Chat") && s.contains("⌥1–9 tabs"));
        thread::sleep(Duration::from_millis(300));
        let settle = |t: &mut Tty, what: &str, ready: &dyn Fn(&str) -> bool| {
            let end = Instant::now() + crate::support::PATIENCE;
            let (mut last, mut held) = (String::new(), 0);
            loop {
                let s = t.screen();
                held = if ready(&s) && s == last { held + 1 } else { 0 };
                if held >= 3 {
                    return s;
                }
                assert!(Instant::now() < end, "{what}: the screen did not hold still:\n{s}");
                last = s;
                thread::sleep(Duration::from_millis(80));
            }
        };
        // Connect opens on Setup; Overview is the next sub-tab
        t.send(b"\x1bOS");
        settle(&mut t, "Agents › Setup", &|s| s.contains("Agent") && s.contains("Custom"));
        t.send(RIGHT);
        let s = settle(&mut t, "the Agents tab", &|s| s.contains("mytool") && s.contains("newagent") && s.contains("connected"));
        golden("connect-rows", size, &s);
        // the cursor goes down to the connection `mytool` (add, found here, newagent, connected, claude, codex, gemini, lint, mytool), and ⏎ opens its page
        t.send(&b"\x1b[B".repeat(8));
        t.send(b"\r");
        let s = settle(&mut t, "the page of a connection", &|s| s.contains("dialect") && s.contains("history"));
        golden("connect-open", size, &s);
        t.send(RIGHT);
        let s = settle(&mut t, "the Lab", &|s| s.contains("run-a") && s.contains("2025-03-04"));
        golden("connect-lab", size, &s);
        t.send(b"\r");
        let s = settle(&mut t, "the table of the run", &|s| s.contains("a:flag"));
        golden("connect-lab-open", size, &s);
        t.send(&[0x03]);
    }
}

/// Every golden in `crates/j4n/tests/tty` is one a test here makes: a stale one fails, it is never left to look checked.
#[test]
fn every_golden_of_the_real_screen_is_one_a_test_makes() {
    let mut made: Vec<String> = SESSIONS.iter().flat_map(|(c, r)| ["fresh", "turn", "jump"].map(|n| format!("{n}.{c}x{r}.txt"))).collect();
    made.extend(PAGES.iter().filter(|(n, _)| *n != "chat").map(|(n, _)| format!("page-{n}.120x36.txt")));
    made.extend(CONNECT_SHOTS.iter().flat_map(|n| CONNECT_SIZES.map(|(c, r)| format!("{n}.{c}x{r}.txt"))));
    made.extend(["narrow-strip.50x18.txt", "narrow-empty.50x18.txt", "chats-main.120x36.txt", "chats-sub.120x36.txt", "chats-pick.120x36.txt", "chats-archive.120x36.txt", "chats-archive-open.120x36.txt", "turn-mark.80x24.txt"].map(String::from));
    let stale: Vec<String> = fs::read_dir(root().join("crates/j4n/tests/tty")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| !made.contains(n)).collect();
    assert!(stale.is_empty(), "goldens no test makes; delete them: {stale:?}");
}

/// A `claude` that plays the recording `fixture`, found by the binary as `$J4N_CLAUDE`.
fn fake_claude_env(d: &Dirs, fixture: &str) -> Vec<(&'static str, String)> {
    use std::os::unix::fs::PermissionsExt;
    let (real, recording, log) = crate::run_cases::fake_claude(d, fixture);
    let wrapper = d.dir.join("bin/claude-here");
    fs::write(&wrapper, format!("#!/bin/sh\nFAKE_FIXTURE='{}' FAKE_LOG='{}' exec '{}' \"$@\"\n", recording.display(), log.display(), real.display())).unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    vec![("J4N_CLAUDE", wrapper.display().to_string())]
}

/// a question comes from the core whole and is answered on the real screen. The test doer asks leave to write a file; its popup has
/// the wait, the risk, the effect and the file it would change, and `1` is a yes that writes it.
#[test]
fn an_approval_of_the_test_doer_is_shown_whole_and_a_key_answers_it() {
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    let d = Dirs::new("tty-ask");
    let mut t = Tty::start(&d);
    t.until("the first frame", 0, |s| s.contains(ALT_ON) && s.contains("Chat"));
    let m = t.mark();
    t.send(b"write notes.txt hello\r");
    let popup = t.until("the question, with what the core knows of it", m, |s| s.contains("notes.txt?") && s.contains("write: notes.txt") && s.contains("? why"));
    assert!(popup.contains("0:0"), "it says how long it has waited: {popup:?}");
    assert!(popup.contains("40"), "and how risky it is: {popup:?}");
    assert!(!d.project().join("notes.txt").exists(), "nothing is written before the answer");
    // the reason the core gives is behind `?`
    let m = t.mark();
    t.send(b"?");
    t.until("the reason", m, |s| s.contains("nothing allows it yet"));
    // `1` is a yes
    let m = t.mark();
    t.send(b"1");
    let end = Instant::now() + crate::support::PATIENCE;
    while !d.project().join("notes.txt").exists() {
        assert!(Instant::now() < end, "the file was never written; the terminal was sent {:?}", t.since(m));
        thread::sleep(Duration::from_millis(20));
    }
    t.until("the keys back in the bottom row", m, |s| s.contains("tabs"));
    assert_eq!(fs::read_to_string(d.project().join("notes.txt")).unwrap().trim(), "hello");
    t.send(&[0x03]);
}

/// an answer that widens what is allowed is pressed twice. The recorded claude asks leave to run `make build`; the first press of
/// the grant for the session is only armed, the second gives it, and the turn goes on to its end.
#[test]
fn an_answer_that_widens_needs_two_presses_on_the_real_screen() {
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    let d = Dirs::new("tty-widen");
    let env = fake_claude_env(&d, "m1-session.jsonl");
    let mut t = Tty::with(&d, "--doer claude --model haiku --approval ask", &env);
    t.until("the first frame", 0, |s| s.contains(ALT_ON) && s.contains("Chat"));
    let m = t.mark();
    t.send(b"build it\r");
    t.until("the approval of the recorded claude, with the answers that widen marked", m, |s| s.contains("make build") && s.contains("widens"));
    // the grant for the session is the 4th: yes, no, this task, this session
    let m = t.mark();
    t.send(b"4");
    t.until("the press armed, not answered", m, |s| s.contains("4 again"));
    thread::sleep(Duration::from_millis(700));
    assert!(!t.since(m).contains("uilt"), "one press does not widen what is allowed: {:?}", t.since(m));
    let m = t.mark();
    t.send(b"4");
    // the terminal is painted where it changed, so a word may arrive in pieces
    t.until("the turn goes on once the grant is given", m, |s| s.contains("uilt"));
    t.send(&[0x03]);
}

/// the Log tab is a window of the core's index. Three turns write their events; the tab shows them as rows with the place among all of
/// them, `g` goes to the first, and a find asks the core for the events that hold the word and says how many there are.
#[test]
fn the_log_tab_of_the_real_screen_scrolls_and_finds_through_the_core() {
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    let d = Dirs::new("tty-log");
    let mut t = Tty::start(&d);
    t.until("the first frame", 0, |s| s.contains(ALT_ON) && s.contains("Chat"));
    for line in ["one", "two", "three"] {
        let m = t.mark();
        t.send(format!("{line}\r").as_bytes());
        t.until("the doer answers", m, |s| s.contains("├─"));
        thread::sleep(Duration::from_millis(500));
    }
    // F6: the Log
    let m = t.mark();
    t.send(b"\x1b[17~");
    let shown = t.until("the events as rows in the Log tab", m, |s| s.contains("task.settled"));
    assert!(shown.contains("task.queued") || shown.contains("task.started") || shown.contains("run.session"), "{shown:?}");
    // the first event, and its place among all of them
    let m = t.mark();
    t.send(b"g");
    t.until("the place of the first row among all", m, |s| s.contains(" 1 of "));
    // a find is sent to the core with ⏎, and the events that hold the word come back (the terminal is painted where it changed, so the
    // second turn's answer, in the row it takes once the others are gone, is what shows the rows are the core's; the tab strip draws it as the
    // summary `answer two`, not as JSON)
    let m = t.mark();
    t.send(b"/settled\r");
    t.until("the events that hold the word", m, |s| s.contains("answer") && s.contains("two"));
    t.send(&[0x03]);
}

/// a pi that writes a 20 kB file in one tool call over 3 s shows the live row within 200 ms of its first `toolcall_start`, grows it
/// while the arguments come, and the real call is the step when the row goes.
#[test]
fn a_tool_call_of_pi_shows_from_its_first_byte_and_grows_on_the_real_screen() {
    use crate::run_cases::{connector, program};
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    let d = Dirs::new("tty-pi-open");
    program(&d, "fake-pi", &read_text(&root().join("fixtures/pi-frames/fake-pi.sh")).unwrap());
    // 30 frames of about 700 bytes, 0.1 s apart
    let mut turn = String::from("{\"type\":\"agent_start\"}\n{\"type\":\"message_update\",\"assistantMessageEvent\":{\"type\":\"toolcall_start\",\"contentIndex\":0,\"toolName\":\"write\"}}\n");
    turn.push_str("{\"type\":\"message_update\",\"assistantMessageEvent\":{\"type\":\"toolcall_delta\",\"contentIndex\":0,\"delta\":\"{\\\"path\\\":\\\"chess.py\\\",\\\"content\\\":\\\"\"}}\n");
    let chunk = "x".repeat(700);
    for _ in 0..29 {
        turn.push_str(&format!("{{\"type\":\"message_update\",\"assistantMessageEvent\":{{\"type\":\"toolcall_delta\",\"contentIndex\":0,\"delta\":\"{chunk}\"}}}}\n"));
    }
    turn.push_str("{\"type\":\"tool_execution_start\",\"toolCallId\":\"c1\",\"toolName\":\"write\",\"args\":{\"path\":\"chess.py\"}}\n");
    turn.push_str("{\"type\":\"tool_execution_end\",\"toolCallId\":\"c1\",\"toolName\":\"write\",\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"wrote\"}]}}\n");
    turn.push_str("{\"type\":\"message_end\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Written.\"}]}}\n{\"type\":\"agent_end\"}\n{\"type\":\"agent_settled\"}\n");
    let path = d.dir.join("turn.jsonl");
    fs::write(&path, &turn).unwrap();
    let t0 = d.dir.join("t0");
    connector(&d, "pi.toml", &format!("api = 1\n[[connector]]\nid = \"pi\"\nprogram = \"{{bin}}/fake-pi\"\nprobe = {{ argv = [\"{{bin}}/fake-pi\", \"--version\"] }}\nenv = {{ FAKE_TURN = \"{}\", FAKE_PACE = \"0.1\", FAKE_T0 = \"{}\", FAKE_LOG = \"{{bin}}/log\" }}\n", path.display(), t0.display()));
    let mut t = Tty::with(&d, "--doer pi --approval full", &[]);
    t.until("the first frame", 0, |s| s.contains(ALT_ON) && s.contains("Chat"));
    t.send(b"make the chess game\r");
    let step = |s: &str| s.lines().any(|l| (l.contains('├') || l.contains('╰')) && l.contains("writing"));
    let shown = t.until_screen("the live row", step);
    let seen = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let started: u128 = fs::read_to_string(&t0).expect("pi wrote the time of its first toolcall_start").trim().parse().unwrap();
    // the live row is there with the verb, before the path has come
    assert!(seen.saturating_sub(started) < 200_000_000, "the step showed {} ms after the first frame (0.1 s between frames)\n{shown}", seen.saturating_sub(started) / 1_000_000);
    let first = shown.lines().find(|l| l.contains("writing")).unwrap_or_default().to_string();
    t.until_screen("the row with its path and its size", |s| s.lines().any(|l| l.contains("chess.py") && l.contains(" kB")));
    let grown = t.until_screen("the row grown", |s| s.lines().any(|l| l.contains("chess.py") && l.contains(" kB") && *l != first));
    assert!(grown.contains("make the chess game"));
    t.until_screen("the real call in its place, the size gone", |s| s.contains("Written.") && !s.contains(" kB"));
    t.send(&[0x03]);
}
