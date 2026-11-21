//! The real binary, run as an outside caller runs it: arguments, stdin, the environment, a folder and an exit status. The
//! `test` doer and a fake `claude` that plays a recording stand in for the models, so what is checked is J4n.
//! The tests are the promises an outside harness holds it to.

use crate::support::{read, root};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// The packs that were on by default before J4n basic, the basic four apart.
pub const LEGACY_PACKS: [&str; 14] = ["agent", "code", "jev", "contribute", "lanes", "inbox", "ledger", "parts", "habits", "kit", "accounts", "skills", "offload", "reflexes"];

pub struct Dirs {
    pub dir: PathBuf,
}

impl Dirs {
    pub fn new(tag: &str) -> Dirs {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("j4n-bin-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = fs::remove_dir_all(&dir);
        for sub in ["project", "home", "bin", "home/.j4n/settings"] {
            fs::create_dir_all(dir.join(sub)).unwrap();
        }
        // the watch switch is off by default: a test does not ask the internet whether a tool has a new release; the project check after a change (on by default) is pinned off: a test that is not about it does not run the project's tests after
        // each write. A test of the default calls `defaults_as_shipped`.
        fs::write(dir.join("home/.j4n/settings/no-watch.toml"), "api = 1\n[[setting]]\nid = \"check.after\"\nvalue = false\n").unwrap();
        // J4n basic loads four packs; a test that is not about J4n basic runs with the other packs turned on, as an operator who turned them on has it.
        // A test of J4n basic calls `bare`.
        fs::write(dir.join("home/.j4n/legacy-packs.toml"), LEGACY_PACKS.iter().fold("api = 1\n".to_string(), |t, p| t + &format!("[[enable]]\nid = \"{p}\"\n"))).unwrap();
        // the Setup card of a first start is answered: a screen test that is not about it starts on its own page. A test of the card uses `bare`
        // or `defaults_as_shipped`.
        fs::write(dir.join("home/.j4n/settings/setup-card.toml"), "api = 1\n[[setting]]\nid = \"setup.card\"\nvalue = false\n").unwrap();
        Dirs { dir }
    }

    /// A HOME and a project that are empty and stay the test's own to read only: no pin of [`Dirs::new`], no file at all, so
    /// whatever is in them later J4n wrote. `bin` (outside HOME) holds the programs a test puts on the PATH.
    pub fn bare(tag: &str) -> Dirs {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("j4n-bare-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = fs::remove_dir_all(&dir);
        for sub in ["project", "home", "bin"] {
            fs::create_dir_all(dir.join(sub)).unwrap();
        }
        Dirs { dir }
    }

    /// The result ledger (on by default) says nothing here: for a test of something else that counts the cards or the events of a run.
    pub fn ledger_off(&self) {
        fs::write(self.dir.join("home/.j4n/settings/ledger-off.toml"), "api = 1\n[[setting]]\nid = \"ledger.mode\"\nvalue = \"off\"\n").unwrap();
    }

    /// Offload (on by default) sends `run the tests` and the like to a cheaper doer: for a test of the main lane's turns, which counts them.
    pub fn offload_off(&self) {
        fs::write(self.dir.join("home/.j4n/settings/offload-off.toml"), "api = 1\n[[setting]]\nid = \"offload.on\"\nvalue = false\n").unwrap();
    }

    /// The habit rows (on by default) say nothing here: for a test that counts the rules that fire.
    pub fn habits_off(&self) {
        fs::write(self.dir.join("home/.j4n/settings/habits-off.toml"), "api = 1\n[[setting]]\nid = \"habits.mode\"\nvalue = \"off\"\n").unwrap();
    }

    /// One of the packs `new` turned on is turned off instead: the pack out of the harness (a pack of J4n basic's four is turned off by a row of its own).
    pub fn pack_off(&self, id: &str) {
        let rows = LEGACY_PACKS.iter().fold("api = 1\n".to_string(), |t, p| t + &format!("[[enable]]\nid = \"{p}\"\n{}", if *p == id { "on = false\n" } else { "" }));
        fs::write(self.dir.join("home/.j4n/legacy-packs.toml"), rows).unwrap();
    }

    /// Take the pins of `new` away: J4n is as it ships.
    pub fn defaults_as_shipped(&self) {
        let _ = fs::write(self.dir.join("home/.j4n/settings/no-watch.toml"), "api = 1\n");
        let _ = fs::write(self.dir.join("home/.j4n/settings/setup-card.toml"), "api = 1\n");
    }

    /// The project is a git repository with a first commit: what a chat's worktree is cut from, so the need `git` holds (a test that is not about it).
    pub fn committed(&self) {
        j4n_core::place::checkpoint::first_commit(&self.project()).expect("a first commit");
    }

    pub fn project(&self) -> PathBuf {
        self.dir.join("project")
    }

    pub fn home(&self) -> PathBuf {
        self.dir.join("home")
    }

    /// J4n's own folder.
    pub fn j4n_home(&self) -> PathBuf {
        self.dir.join("home/.j4n")
    }

    pub fn put(&self, rel: &str, text: &str) -> PathBuf {
        let p = self.project().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, text).unwrap();
        p
    }
}

impl Drop for Dirs {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

pub struct Out {
    pub rc: i32,
    pub out: String,
    pub err: String,
    pub secs: f64,
}

/// Run `j4n args` in the project of `d`, with `stdin` on its standard input (None: closed) and these extra variables.
pub fn j4n(d: &Dirs, args: &[&str], stdin: Option<&str>, env: &[(&str, &str)]) -> Out {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_j4n"));
    cmd.args(args)
        .current_dir(d.project())
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", d.home())
        .env("J4N_HOME", d.j4n_home())
        .env("TMPDIR", d.dir.join("tmp"))
        .env("https_proxy", "http://127.0.0.1:9")
        .envs(env.iter().copied())
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    fs::create_dir_all(d.dir.join("tmp")).unwrap();
    let t = Instant::now();
    let mut child = cmd.spawn().expect("j4n starts");
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(text.as_bytes()).unwrap();
    }
    let done = child.wait_with_output().expect("j4n ends");
    Out { rc: done.status.code().unwrap_or(-1), out: String::from_utf8_lossy(&done.stdout).into_owned(), err: String::from_utf8_lossy(&done.stderr).into_owned(), secs: t.elapsed().as_secs_f64() }
}

/// Every line of every event the project's Log holds.
pub fn log_events(d: &Dirs) -> Vec<serde_json::Value> {
    let dir = d.project().join(".j4n/log");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir).map(|r| r.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    files.sort();
    files.iter().filter(|p| p.extension().is_some_and(|e| e == "jsonl")).flat_map(|p| read(p).lines().map(String::from).collect::<Vec<_>>()).filter_map(|l| serde_json::from_str(&l).ok()).collect()
}

pub fn kinds(events: &[serde_json::Value]) -> Vec<String> {
    events.iter().filter_map(|e| e["kind"].as_str().map(String::from)).collect()
}

fn every_file(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            every_file(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// The recordings the fake `claude` plays, and the script itself.
pub fn fake_claude(d: &Dirs, fixture: &str) -> (PathBuf, PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let program = d.dir.join("bin/claude");
    fs::write(&program, read(&root().join("fixtures/claude-frames/fake-claude.sh"))).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    (program, root().join("fixtures/claude-frames").join(fixture), d.dir.join("bin/log"))
}

// ---- the promises an outside harness holds it to ----

/// `j4n 0.1.0`, then the commit and its date when the build knew them: ` (0123abcd 2026-01-01)`.
fn version_line_ok(line: &str) -> bool {
    let digits = |s: &str, n: Option<usize>| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && n.is_none_or(|n| s.len() == n);
    let Some(rest) = line.strip_prefix("j4n ") else { return false };
    let (number, stamp) = rest.split_once(' ').map_or((rest, None), |(n, s)| (n, Some(s)));
    let parts: Vec<&str> = number.split('.').collect();
    if parts.len() != 3 || !parts.iter().all(|p| digits(p, None)) {
        return false;
    }
    let Some(stamp) = stamp else { return true };
    let Some((commit, date)) = stamp.strip_prefix('(').and_then(|s| s.strip_suffix(')')).and_then(|s| s.split_once(' ')) else { return false };
    let d: Vec<&str> = date.split('-').collect();
    commit.len() == 8 && commit.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) && d.len() == 3 && digits(d[0], Some(4)) && digits(d[1], Some(2)) && digits(d[2], Some(2))
}

#[test]
fn version_line_is_strict() {
    assert!(version_line_ok("j4n 0.1.0") && version_line_ok("j4n 0.1.0 (0123abcd 2026-01-01)"));
    for bad in ["j4n", "j4n 0.1", "j4n 0.1.0 extra", "j4n 0.1.0 (0123ABCD 2026-01-01)", "j4n 0.1.0 (0123abc 2026-01-01)", "j4n 0.1.0 (0123abcd 2026-1-01)"] {
        assert!(!version_line_ok(bad), "{bad}");
    }
}

#[test]
fn version_prints_its_name_fast_and_writes_nothing_into_home() {
    let d = Dirs::new("run-version");
    let o = j4n(&d, &["--version"], None, &[]);
    assert_eq!(o.rc, 0);
    assert!(version_line_ok(o.out.trim_end_matches('\n')), "{:?}", o.out);
    // alone it takes a few hundredths of a second; the bound only says that the command does not wait for something
    assert!(o.secs < 20.0, "{}", o.secs);
    // HOME is as `Dirs` made it: J4n's folder holds the test's own switches (no watch, the Setup card answered) and nothing else
    let mut found = Vec::new();
    let mut stack = vec![d.home()];
    while let Some(dir) = stack.pop() {
        for e in fs::read_dir(&dir).unwrap().flatten() {
            if e.path().is_dir() { stack.push(e.path()) } else { found.push(e.path().strip_prefix(d.home()).unwrap().to_path_buf()) }
        }
    }
    // a folder lists its entries in the file system's own order, which is not the same on every machine
    found.sort();
    assert_eq!(found, [PathBuf::from(".j4n/legacy-packs.toml"), PathBuf::from(".j4n/settings/no-watch.toml"), PathBuf::from(".j4n/settings/setup-card.toml")], "HOME is as it was");
    assert!(fs::read_dir(d.project()).unwrap().next().is_none(), "and so is the project");
}

#[test]
fn with_no_terminal_and_stdin_closed_it_ends_by_itself() {
    let d = Dirs::new("run-no-terminal");
    let o = j4n(&d, &[], None, &[]);
    assert_eq!(o.rc, 0, "{}", o.err);
    assert!(o.secs < 60.0, "it did not end by itself: {}s", o.secs);
    assert!(o.out.is_empty());
}

/// What another process of the same user, a child of this test, reads of a running `j4n`'s environment: `Ok` with it, or the OS's words.
/// `j4n` waits on its stdin, and is read once it has made the project's `.j4n`, so it is past its start.
#[cfg(target_os = "linux")]
fn environ_of_a_running_j4n(d: &Dirs, env: &[(&str, &str)]) -> Result<String, String> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_j4n"));
    cmd.args(["--doer", "test", "--approval", "deny"])
        .current_dir(d.project())
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", d.home())
        .env("J4N_HOME", d.j4n_home())
        .env("TMPDIR", d.dir.join("tmp"))
        .env("https_proxy", "http://127.0.0.1:9")
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    fs::create_dir_all(d.dir.join("tmp")).unwrap();
    let mut child = cmd.spawn().expect("j4n starts");
    let end = Instant::now() + crate::support::PATIENCE;
    while !d.project().join(".j4n").exists() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    let read = Command::new("cat").arg(format!("/proc/{}/environ", child.id())).env("LC_ALL", "C").output().expect("cat runs");
    drop(child.stdin.take());
    assert!(child.wait().expect("j4n ends").success(), "j4n ended badly");
    if read.status.success() { Ok(String::from_utf8_lossy(&read.stdout).replace('\0', "\n")) } else { Err(String::from_utf8_lossy(&read.stderr).into_owned()) }
}

/// A running J4n is not dumpable: its environment, where the keys it was started with are, is refused by the OS to another process of the
/// same user. `J4N_DUMPABLE=1` leaves it readable, for a debugger, which also shows that the read is one the OS would otherwise allow.
#[cfg(target_os = "linux")]
#[test]
fn a_running_j4n_refuses_its_environment_to_another_process_of_the_same_user() {
    use std::os::unix::fs::MetadataExt;
    // root may read any process's environment
    if fs::metadata("/proc/self").is_ok_and(|m| m.uid() == 0) {
        return;
    }
    let mark = ("SOME_SERVICE_TOKEN", "dumpmark4Tq8wZx1Ln6Rv");
    let d = Dirs::new("run-dumpable");
    let refused = environ_of_a_running_j4n(&d, &[mark]);
    assert!(refused.as_ref().is_err_and(|why| why.contains("Permission denied")), "the OS refused the read: {refused:?}");
    let d = Dirs::new("run-debug");
    let open = environ_of_a_running_j4n(&d, &[mark, ("J4N_DUMPABLE", "1")]);
    assert!(open.as_ref().is_ok_and(|env| env.lines().any(|l| l == "SOME_SERVICE_TOKEN=dumpmark4Tq8wZx1Ln6Rv")), "with J4N_DUMPABLE=1 it is read: {open:?}");
}

#[test]
fn an_unknown_flag_is_refused_loudly_and_nothing_is_started() {
    let d = Dirs::new("run-unknown-flag");
    for args in [&["run", "--no-such-flag-at-all", "say ok"][..], &["--bogus"], &["run"], &["--wall", "soon"]] {
        let o = j4n(&d, args, None, &[]);
        assert_eq!(o.rc, 2, "{args:?}");
        assert!(o.out.is_empty() && o.err.contains("j4n:") && o.err.contains("--help"), "{args:?}: {:?}", o.err);
    }
    assert!(!d.project().join(".j4n").exists(), "no Log was made for a command that was refused");
}

/// a command word (`j4n pack`) answers on stdout, so `j4n pack > file` holds the answer and the screen shows none of it twice; what a
/// line of it refuses stays on stderr with the exit status it had.
#[test]
fn a_command_words_answer_is_on_stdout_and_a_redirect_holds_it() {
    let d = Dirs::new("run-command-words-answer");
    let o = j4n(&d, &["pack"], None, &[]);
    assert_eq!(o.rc, 0, "{}", o.err);
    assert!(o.out.lines().count() > 1 && o.out.contains("ledger"), "the packs, one a line: {:?}", o.out);
    assert!(o.err.trim().is_empty(), "the answer is not progress: {:?}", o.err);
    // the same through a shell's redirect: stdout to the file, stderr to the terminal (here a second file)
    let (file, errs) = (d.dir.join("answer.txt"), d.dir.join("errors.txt"));
    let done = Command::new("sh")
        .arg("-c")
        .arg(format!("\"$J4N\" pack > '{}' 2> '{}'", file.display(), errs.display()))
        .env_clear()
        .env("J4N", env!("CARGO_BIN_EXE_j4n"))
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", d.home())
        .env("J4N_HOME", d.j4n_home())
        .current_dir(d.project())
        .stdin(Stdio::null())
        .status()
        .expect("sh runs j4n");
    assert!(done.success());
    assert_eq!((read(&file), read(&errs)), (o.out.clone(), String::new()));
    // a refusal is not the answer: a command that cannot say what was asked leaves stdout empty
    let o = j4n(&d, &["report", "nobody"], None, &[]);
    assert!(o.out.is_empty() && o.err.contains("no report"), "{}\n{}", o.out, o.err);
    assert_ne!(o.rc, 0);
}

#[test]
fn help_says_what_an_outside_caller_looks_for() {
    let d = Dirs::new("help");
    let top = j4n(&d, &["--help"], None, &[]);
    let run = j4n(&d, &["run", "--help"], None, &[]);
    assert_eq!((top.rc, run.rc), (0, 0));
    assert!(top.out.contains("stdin") && top.out.contains("--wall"), "{}", top.out);
    assert!(run.out.contains("--wall") && run.out.contains("--approval") && run.out.contains("--doer") && run.out.contains("--model"));
}

#[test]
fn run_with_the_test_doer_prints_the_answer_exits_clean_and_leaves_a_log_that_git_does_not_see() {
    let d = Dirs::new("run");
    d.put("README.md", "a project\n");
    for args in [&["init", "-q"][..], &["add", "-A"], &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "setup"]] {
        assert!(Command::new("git").args(args).current_dir(d.project()).output().expect("git").status.success());
    }
    let o = j4n(&d, &["run", "say ok", "--doer", "test", "--approval", "deny", "--wall", "20s"], None, &[]);
    assert_eq!((o.rc, o.out.as_str()), (0, "ok\n"), "{}", o.err);
    assert!(d.project().join(".j4n").is_dir());
    let status = Command::new("git").args(["status", "--porcelain"]).current_dir(d.project()).output().unwrap();
    assert!(status.stdout.is_empty(), "`.j4n/` ignores itself: {}", String::from_utf8_lossy(&status.stdout));
    assert_eq!(read(&d.project().join("README.md")), "a project\n");
}

#[test]
fn a_fresh_home_never_elects_the_scripted_doer_for_work_and_the_flag_still_runs_it() {
    let d = Dirs::new("run-fresh-home-never");
    let none = d.dir.join("bin");
    let elected = j4n(&d, &["run", "say ok", "--approval", "deny", "--wall", "20s"], None, &[("PATH", none.to_str().unwrap())]);
    assert_ne!(elected.rc, 0, "nobody is installed, so nobody is elected: {}{}", elected.out, elected.err);
    assert!(elected.err.contains("good for `work`") && !elected.out.contains("ok"), "{}{}", elected.out, elected.err);
    assert!(log_events(&d).iter().all(|e| e["data"]["spec"]["doer"] != "test"), "no task was started on it");
    let named = j4n(&d, &["run", "say ok", "--doer", "test", "--approval", "deny", "--wall", "20s"], None, &[("PATH", none.to_str().unwrap())]);
    assert_eq!((named.rc, named.out.as_str()), (0, "ok\n"), "{}", named.err);
}

#[test]
fn a_run_in_a_folder_with_no_git_works_in_place_and_changes_nothing_it_was_not_asked_to() {
    let d = Dirs::new("nogit");
    let notes = d.put("notes.txt", "x\n");
    let o = j4n(&d, &["run", "say ok", "--doer", "test", "--approval", "deny", "--wall", "20s"], None, &[]);
    assert_eq!(o.rc, 0, "{}", o.err);
    assert_eq!(read(&notes), "x\n");
    assert!(!d.project().join(".git").exists());
}

#[test]
fn a_key_in_the_environment_is_in_no_file_afterwards() {
    let d = Dirs::new("run-key-in-env");
    let canary = "sk-j4n-canary-7f3a9c1e5b2d4f60";
    let o = j4n(&d, &["run", "say ok", "--doer", "test", "--approval", "deny"], None, &[("OPENROUTER_API_KEY", canary), ("SOME_SERVICE_TOKEN", "tok-canary-0123456789")]);
    assert_eq!(o.rc, 0, "{}", o.err);
    let mut files = Vec::new();
    every_file(&d.dir, &mut files);
    assert!(!files.is_empty());
    for f in files {
        let bytes = fs::read(&f).unwrap();
        for c in [canary, "tok-canary-0123456789"] {
            assert!(!bytes.windows(c.len()).any(|w| w == c.as_bytes()), "{} holds a key", f.display());
        }
    }
}

#[test]
fn a_secret_a_doer_echoes_is_scrubbed_before_it_reaches_the_log() {
    let d = Dirs::new("scrub");
    let token = "tok-canary-aaaaaaaaaaaaaaaa";
    let o = j4n(&d, &["run", &format!("say the token is {token}"), "--doer", "test"], None, &[("SOME_SERVICE_TOKEN", token)]);
    assert_eq!(o.rc, 0, "{}", o.err);
    let log = log_events(&d).iter().map(|e| e.to_string()).collect::<String>();
    assert!(!log.contains(token) && log.contains("secret:SOME_SERVICE_TOKEN"), "{log}");
}

#[test]
fn a_password_in_the_environment_is_scrubbed_like_a_key_and_a_short_value_is_left_alone() {
    let d = Dirs::new("password");
    let o = j4n(&d, &["run", "say pw-canary-0123456789 and ab", "--doer", "test"], None, &[("DB_PASSWORD", "pw-canary-0123456789"), ("SHORT_KEY", "ab")]);
    assert_eq!(o.rc, 0, "{}", o.err);
    let log = log_events(&d).iter().map(|e| e.to_string()).collect::<String>();
    assert!(!log.contains("pw-canary-0123456789") && log.contains("secret:DB_PASSWORD"), "{log}");
    assert!(log.contains("and ab") && !log.contains("secret:SHORT_KEY"), "a value of two letters is not a secret worth hiding: {log}");
}

#[test]
fn a_spend_limit_is_the_tasks_soft_cap_and_the_run_says_it_was_passed() {
    let d = Dirs::new("usd");
    let o = j4n(&d, &["run", "spend 0.5; say done", "--doer", "test", "--usd", "0.01"], None, &[]);
    assert!(log_events(&d).iter().any(|e| e["kind"] == "task.cap_reached"), "{}\n{}", o.out, o.err);
    assert!(o.err.contains("soft cap"), "{}", o.err);
}

#[test]
fn the_wall_ends_a_never_ending_run_by_itself_with_its_report_and_the_log_on_disk() {
    let d = Dirs::new("run-wall-ends-never");
    // the wall counts from the start of the process, and the first half is said some time after it: the wall is long enough for a
    // process on a slow machine to get there, and what the test holds is that the run ends by
    // itself, a run that never ended being the failure
    let o = j4n(&d, &["run", "say the first half; hang", "--doer", "test", "--wall", "15s"], None, &[]);
    assert!(o.secs >= 14.5 && o.secs < 120.0, "{}", o.secs);
    assert_eq!(o.rc, 1, "it did not finish, and says so");
    assert_eq!(o.out, "the first half\n", "the report is what had been said");
    assert!(o.err.contains("stopped: wall"), "{}", o.err);
    let events = log_events(&d);
    let stopped = events.iter().find(|e| e["kind"] == "task.stopped").expect("the Log holds the end of the task");
    assert_eq!(stopped["data"]["why"]["kind"], "wall");
}

#[test]
fn a_goal_of_many_lines_is_one_turn_the_doer_has_all_of_from_the_first_word() {
    let d = Dirs::new("run-goal");
    let o = j4n(&d, &["run", "say the first line\nsay the second line", "--doer", "test", "--approval", "deny"], None, &[]);
    assert_eq!((o.rc, o.out.as_str()), (0, "the second line\n"), "one task, one report: {}", o.err);
    let events = log_events(&d);
    let said: Vec<_> = events.iter().filter(|e| e["kind"] == "chat.say").collect();
    assert_eq!(said.len(), 1, "the whole goal is said once");
    assert_eq!(said[0]["data"]["text"], "say the first line\nsay the second line");
    assert_eq!(kinds(&events).iter().filter(|k| *k == "task.queued").count(), 1, "one task, not one for each line");
}

#[test]
fn the_wall_ends_the_run_once_and_the_lines_after_it_are_not_taken() {
    let d = Dirs::new("run-wall");
    d.ledger_off();
    let o = j4n(&d, &["--doer", "test", "--approval", "deny", "--wall", "15s"], Some("say a\nhang\nsay b\nsay c\nsay d\n"), &[]);
    assert!(o.secs >= 14.5 && o.secs < 120.0, "{}", o.secs);
    assert_eq!(o.rc, 1, "{}", o.err);
    assert_eq!(o.err.matches("stopped: wall").count(), 1, "said once: {}", o.err);
    assert!(!o.err.contains("has run out"), "no line after the wall is refused one by one: {}", o.err);
    let events = log_events(&d);
    assert_eq!(kinds(&events).iter().filter(|k| *k == "task.stopped").count(), 1, "one stop");
    assert_eq!(events.iter().filter(|e| e["kind"] == "chat.say").count(), 2, "the lines after the stop were never read");
    assert!(!kinds(&events).iter().any(|k| k == "task.resumed" || k == "chat.note"), "no carrying on, no notice for each line: {:?}", kinds(&events));
}

#[test]
fn deny_writes_nothing_and_full_writes_what_nobody_blocked() {
    let d = Dirs::new("run-deny-full");
    let o = j4n(&d, &["run", "write out/a.txt hello", "--doer", "test", "--approval", "deny"], None, &[]);
    assert_eq!(o.rc, 1);
    assert!(!d.project().join("out").exists(), "nothing was written");
    assert!(o.err.contains("not allowed"), "{}", o.err);
    let o = j4n(&d, &["run", "write out/a.txt hello", "--doer", "test", "--approval", "full"], None, &[]);
    assert_eq!(o.rc, 0, "{}", o.err);
    assert_eq!(read(&d.project().join("out/a.txt")), "hello");
    let said: Vec<_> = log_events(&d).into_iter().filter(|e| e["kind"] == "ask.answered").collect();
    assert_eq!(said.last().unwrap()["data"]["by"], "rule:approval-full");
}

#[test]
fn the_default_with_no_terminal_is_deny() {
    let d = Dirs::new("default");
    let o = j4n(&d, &["run", "write a.txt x", "--doer", "test"], None, &[]);
    assert_eq!(o.rc, 1);
    assert!(!d.project().join("a.txt").exists());
}

#[test]
fn a_doer_that_ends_with_nothing_is_a_failed_run_with_a_reason() {
    let d = Dirs::new("run-empty-doer");
    let o = j4n(&d, &["run", "silent", "--doer", "test"], None, &[]);
    assert_eq!(o.rc, 1);
    assert!(o.out.is_empty() && o.err.contains("no answer and no sign of work"), "{}", o.err);
}

#[test]
fn a_doer_that_does_not_exist_is_refused_with_the_names_there_are() {
    let d = Dirs::new("nodoer");
    let o = j4n(&d, &["run", "say ok", "--doer", "nosuch"], None, &[]);
    assert_eq!(o.rc, 1);
    assert!(o.err.contains("nosuch") && o.err.contains("claude") && o.err.contains("test"), "{}", o.err);
}

#[test]
fn with_no_doer_named_the_ready_ones_are_ranked_and_one_is_chosen() {
    let d = Dirs::new("elect");
    let (program, ..) = fake_claude(&d, "m1-session.jsonl");
    // claude is installed and logged in (the fake says so), so `claude` < `test` in the rank
    // the fake plays nothing, so the run is ended by its wall; what is looked at is who was chosen. The wall counts from the start
    // of the process, and a loaded machine can take seconds to start it, so the wall is long enough to get past the election; the
    // bound on the time only says that the wall ended the run, which a hang would not
    let o = j4n(&d, &["run", "say ok", "--wall", "15s"], None, &[("J4N_CLAUDE", program.to_str().unwrap()), ("FAKE_FIXTURE", "/dev/null")]);
    assert!(o.err.contains("bound to claude (elected)"), "{}", o.err);
    assert!(o.secs < 60.0, "{}", o.secs);
}

/// The row of an agent the way the Connect flow proposes one it learned by trying: a program that takes its prompt as an argument, the arm's role `work`.
#[test]
fn an_agent_the_flow_learned_is_elected_for_work_through_its_arm_and_is_not_offered_for_a_role_its_row_does_not_name() {
    use std::os::unix::fs::PermissionsExt;
    let d = Dirs::new("arm");
    let program = d.dir.join("bin/zorvik");
    fs::write(&program, "#!/bin/sh\ncase \"$1\" in --version) echo \"zorvik 1.0\" ;; *) echo \"answered: $*\" ;; esac\n").unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    let p = program.to_str().unwrap();
    fs::create_dir_all(d.j4n_home().join("connectors")).unwrap();
    let row = |roles: &str| format!("api = 1\n[[connector]]\nid = \"zorvik\"\ndoc = \"`zorvik`, learned by trying it\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"{p}\"\nprobe = {{ argv = [\"{p}\", \"--version\"] }}\nstart = [\"{{prompt}}\"]\nroles = {roles}\n");
    fs::write(d.j4n_home().join("connectors/zorvik.toml"), row("[\"work\"]")).unwrap();
    let o = j4n(&d, &["run", "say ok", "--wall", "30s"], None, &[]);
    assert!(o.err.contains("bound to zorvik (elected)"), "{}", o.err);
    assert!(o.out.contains("answered: say ok"), "{}", o.out);
    // a row that names another role is not offered for work: the election has nobody and says so
    fs::write(d.j4n_home().join("connectors/zorvik.toml"), row("[\"review\"]")).unwrap();
    let o = j4n(&d, &["run", "say ok", "--wall", "30s"], None, &[]);
    assert!(!o.err.contains("bound to zorvik"), "{}", o.err);
    assert_ne!(o.rc, 0, "{}", o.out);
}

#[test]
fn a_project_another_j4n_has_open_is_refused_and_not_shared() {
    let d = Dirs::new("lock");
    let mut first = Command::new(env!("CARGO_BIN_EXE_j4n"))
        .args(["--doer", "test"])
        .current_dir(d.project())
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", d.home())
        .env("J4N_HOME", d.j4n_home())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    for _ in 0..200 {
        if d.project().join(".j4n/log/.lock").exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(200));
    let o = j4n(&d, &["run", "say ok", "--doer", "test"], None, &[]);
    drop(first.stdin.take());
    let _ = first.wait();
    assert_eq!(o.rc, 1);
    assert!(o.err.contains("another J4n has this log open"), "{}", o.err);
}

// ---- piped mode ----

#[test]
fn two_lines_on_stdin_are_one_thread_with_one_session_and_the_marker_before_each_answer() {
    let d = Dirs::new("run-two-lines-on");
    let o = j4n(&d, &["--doer", "test", "--model", "m1"], Some("who\nwho\n\n  say hi; who  \n"), &[]);
    assert_eq!(o.rc, 0, "{}", o.err);
    assert_eq!(o.out, "▌you: who\nchild 1 turn 1\n▌you: who\nchild 1 turn 2\n▌you: say hi; who\nchild 1 turn 3\n");
    let events = log_events(&d);
    let bound: Vec<_> = events.iter().filter(|e| e["kind"] == "thread.bound").collect();
    assert_eq!(bound.len(), 1, "one election for three lines");
    let threads: std::collections::BTreeSet<_> = events.iter().filter(|e| e["kind"] == "task.queued").map(|e| e["thread"].as_str().unwrap().to_string()).collect();
    assert_eq!(threads.len(), 1);
    assert!(threads.iter().next().unwrap().starts_with("pipe-"));
}

#[test]
fn with_claude_the_same_child_serves_every_line() {
    let d = Dirs::new("run-claude-same-child");
    let (program, fixture, log) = fake_claude(&d, "two-turns.jsonl");
    let env = [("J4N_CLAUDE", program.to_str().unwrap()), ("FAKE_FIXTURE", fixture.to_str().unwrap()), ("FAKE_LOG", log.to_str().unwrap())];
    let o = j4n(&d, &["--doer", "claude", "--model", "haiku", "--approval", "full"], Some("first line\nsecond line\n"), &env);
    assert_eq!(o.rc, 0, "{}", o.err);
    assert_eq!(o.out, "▌you: first line\none\n▌you: second line\ntwo\n");
    let sent = read(&log);
    assert_eq!(sent.lines().filter(|l| l.starts_with("start ")).count(), 1, "one child was ever started:\n{sent}");
    assert_eq!(sent.lines().filter(|l| l.contains("\"type\":\"user\"")).count(), 2);
    assert!(sent.lines().next().unwrap().contains("--model haiku"));
    let events = log_events(&d);
    let started: Vec<_> = events.iter().filter(|e| e["kind"] == "task.started").collect();
    assert_eq!((started[0]["data"]["resumed"].as_bool(), started[1]["data"]["resumed"].as_bool()), (Some(false), Some(true)));
}

/// Through the real binary: J4n run inside the Claude desktop app (its messaging socket and token and its session id in J4n's
/// own environment) starts claude without them, with the project scope's flags, and the operator's own variables still arrive.
#[test]
fn claude_started_by_j4n_inside_a_host_gets_no_host_session_and_the_project_scope() {
    let d = Dirs::new("run-claude-started-inside");
    let (program, fixture, log) = fake_claude(&d, "two-turns.jsonl");
    let envlog = d.dir.join("child-env");
    let env = [
        ("J4N_CLAUDE", program.to_str().unwrap()),
        ("FAKE_FIXTURE", fixture.to_str().unwrap()),
        ("FAKE_LOG", log.to_str().unwrap()),
        ("FAKE_ENV_LOG", envlog.to_str().unwrap()),
        ("CLAUDE_CODE_MESSAGING_SOCKET", "/run/user/1/cc-socks/1.sock"),
        ("CLAUDE_CODE_MESSAGING_TOKEN", "tok-of-the-app"),
        ("CLAUDE_CODE_SESSION_ID", "session-of-the-app"),
        ("CLAUDECODE", "1"),
        ("J4N_TEST_OPERATORS_OWN", "kept"),
    ];
    let o = j4n(&d, &["--doer", "claude", "--model", "haiku", "--approval", "full"], Some("first line\n"), &env);
    assert_eq!(o.rc, 0, "{}", o.err);
    let child = read(&envlog);
    for name in ["CLAUDE_CODE_MESSAGING_SOCKET", "CLAUDE_CODE_MESSAGING_TOKEN", "CLAUDE_CODE_SESSION_ID", "CLAUDECODE"] {
        assert!(!child.lines().any(|l| l.starts_with(&format!("{name}="))), "{name} reached claude:\n{child}");
    }
    assert!(child.contains("J4N_TEST_OPERATORS_OWN=kept") && child.contains("CLAUDE_CODE_ENABLE_TODO_TOOLS=1"), "{child}");
    let start = read(&log).lines().next().unwrap().to_string();
    assert!(start.contains("--setting-sources project,local --strict-mcp-config --no-chrome --disallowedTools ListAgents,SendMessage,PushNotification,RemoteTrigger,DesignSync"), "{start}");
}

#[test]
fn ask_mode_takes_the_answer_to_a_question_from_the_next_line() {
    let d = Dirs::new("ask");
    let o = j4n(&d, &["--doer", "test", "--approval", "ask"], Some("write a.txt hello\nyes\n"), &[]);
    assert_eq!(o.rc, 0, "{}", o.err);
    assert_eq!(read(&d.project().join("a.txt")), "hello");
    assert!(o.err.contains("waiting for an answer"), "{}", o.err);
    let d = Dirs::new("ask2");
    let o = j4n(&d, &["--doer", "test", "--approval", "ask"], Some("write a.txt hello\nno\n"), &[]);
    assert!(!d.project().join("a.txt").exists());
    assert_eq!((o.rc, o.out.as_str()), (1, "▌you: write a.txt hello\n"), "the line that answers is no turn of its own: {}", o.err);
}

#[test]
fn ask_mode_with_no_answer_before_the_end_of_input_stops_the_task_and_fails() {
    let d = Dirs::new("ask3");
    let o = j4n(&d, &["--doer", "test", "--approval", "ask"], Some("write a.txt hello\n"), &[]);
    assert_eq!(o.rc, 1, "{}", o.err);
    assert!(!d.project().join("a.txt").exists());
}

#[test]
fn a_command_line_in_piped_mode_is_obeyed_and_quit_ends_the_input() {
    let d = Dirs::new("cmd");
    let o = j4n(&d, &["--doer", "test"], Some("who\n:fresh\nwho\n:status\n:quit\nwho\n"), &[]);
    assert_eq!(o.rc, 0, "{}", o.err);
    assert_eq!(o.out.matches("child ").count(), 2, "{}", o.out);
    assert!(o.out.contains("child 1 turn 1") && !o.out.contains("turn 3"));
    let events = log_events(&d);
    assert_eq!(kinds(&events).iter().filter(|k| *k == "thread.bound").count(), 2, ":fresh binds again");
    assert!(o.err.contains("bound to test (pinned)"), "{}", o.err);
}

#[test]
fn a_refused_line_fails_the_run_and_the_ones_after_it_still_go() {
    let d = Dirs::new("refuse");
    let o = j4n(&d, &["--doer", "test"], Some(":switch nosuch\nwho\n"), &[]);
    assert_eq!(o.rc, 1);
    assert!(o.err.contains("nosuch") && o.out.contains("child 1 turn 1"), "{o:?}", o = (o.out.clone(), o.err.clone()));
}

#[test]
fn j4n_home_is_where_its_own_files_go_and_the_project_gets_only_dot_j4n() {
    let d = Dirs::new("home");
    d.put("a.txt", "x");
    let o = j4n(&d, &["run", "write out/b.txt y", "--doer", "test", "--approval", "full"], None, &[]);
    assert_eq!(o.rc, 0, "{}", o.err);
    let mut names: Vec<String> = fs::read_dir(d.project()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names, [".j4n", "a.txt", "out"]);
    assert!(!d.home().join(".claude").exists());
}

// ---- connectors: a program J4n has never seen is a file, and no Rust ----

/// A script in the `bin` folder of the run.
pub fn program(d: &Dirs, name: &str, text: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = d.dir.join("bin").join(name);
    fs::write(&path, text).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// A connector file in J4n's own folder, with `{bin}` the folder of the programs.
pub fn connector(d: &Dirs, name: &str, text: &str) {
    let path = d.j4n_home().join("connectors").join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text.replace("{bin}", &d.dir.join("bin").display().to_string())).unwrap();
}

#[test]
fn a_cli_never_seen_is_one_file_in_the_home_folder_and_answers_through_the_front() {
    let d = Dirs::new("never");
    program(&d, "newagent", &read(&root().join("fixtures/connectors/newagent.sh")));
    connector(&d, "newagent.toml", &read(&root().join("fixtures/connectors/newagent.toml")));
    let o = j4n(&d, &["run", "what is the answer?", "--doer", "newagent"], None, &[]);
    assert_eq!((o.rc, o.out.as_str()), (0, "The answer is 42 (no model, ).\n"), "{}", o.err);
    let events = log_events(&d);
    assert!(kinds(&events).contains(&"run.tool_call".to_string()), "{:?}", kinds(&events));
    // the next line of a piped session carries on in its session
    let o = j4n(&d, &["--doer", "newagent", "--model", "m1"], Some("one\ntwo\n"), &[]);
    assert_eq!(o.rc, 0, "{}", o.err);
    assert!(o.out.contains("▌you: two\nThe answer is 42 (m1, resumed)."), "{}", o.out);
}

#[test]
fn pi_is_the_base_row_and_a_home_file_says_where_the_program_is() {
    let d = Dirs::new("pi");
    // pi is signed in where its folder is
    fs::create_dir_all(d.dir.join("home/.pi")).unwrap();
    program(&d, "fake-pi", &read(&root().join("fixtures/pi-frames/fake-pi.sh")));
    let turn = root().join("fixtures/pi-frames/pi-turn.jsonl");
    connector(&d, "pi.toml", &format!("api = 1\n[[connector]]\nid = \"pi\"\nprogram = \"{{bin}}/fake-pi\"\nprobe = {{ argv = [\"{{bin}}/fake-pi\", \"--version\"] }}\nenv = {{ FAKE_TURN = \"{}\", FAKE_LOG = \"{{bin}}/log\" }}\n", turn.display()));
    let o = j4n(&d, &["run", "which is larger?", "--doer", "pi", "--approval", "full"], None, &[]);
    assert_eq!((o.rc, o.out.as_str()), (0, "9.9 is larger.\n"), "{}", o.err);
    let log = read(&d.dir.join("bin/log"));
    assert!(log.starts_with("start --mode rpc\n") && log.contains("which is larger?"), "{log}");
    // with no doer named, a ready pi is a candidate: it is elected when nothing ranks before it
    let o = j4n(&d, &["run", "again", "--wall", "20s"], None, &[("J4N_CLAUDE", "/no/such/claude")]);
    assert!(o.err.contains("bound to pi (elected)") || o.err.contains("bound to omp (elected)") || o.err.contains("bound to test (elected)"), "{}", o.err);
}

#[test]
fn an_acp_agent_is_one_file_too() {
    let d = Dirs::new("acp");
    program(&d, "acp-agent", &read(&root().join("fixtures/connectors/acp-agent.sh")));
    connector(&d, "acp.toml", &read(&root().join("fixtures/connectors/acp-agent.toml")));
    let o = j4n(&d, &["run", "hello", "--doer", "acpagent"], None, &[]);
    assert_eq!((o.rc, o.out.as_str()), (0, "Hello world\n"), "{}", o.err);
    let o = j4n(&d, &["run", "ask me", "--doer", "acpagent", "--approval", "deny"], None, &[]);
    assert_eq!(o.out, "It was not allowed.\n", "a deny refuses the agent's question: {}", o.err);
}

#[test]
fn a_connector_file_that_is_wrong_is_said_and_the_rest_still_works() {
    let d = Dirs::new("badconn");
    connector(&d, "bad.toml", "api = 1\n[[connector]]\nid = \"bad\"\nis = \"agent\"\ndialect = \"text\"\n[[connector]]\nid = \"junk\"\nis = \"agent\"\nnonsense = 1\n");
    let o = j4n(&d, &["run", "say ok", "--doer", "test"], None, &[]);
    assert_eq!((o.rc, o.out.as_str()), (0, "ok\n"), "{}", o.err);
    assert!(o.err.contains("[[connector]] bad") && o.err.contains("`program` is needed"), "{}", o.err);
    assert!(o.err.contains("nonsense"), "{}", o.err);
}

#[test]
fn a_project_cannot_bring_its_own_program() {
    let d = Dirs::new("projconn");
    d.put(".j4n/connectors/evil.toml", "api = 1\n[[connector]]\nid = \"evil\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"echo\"\nstart = [\"pwned\"]\n");
    let o = j4n(&d, &["run", "x", "--doer", "evil"], None, &[]);
    assert_eq!(o.rc, 1);
    assert!(o.err.contains("no doer `evil`") || o.err.contains("evil"), "{}", o.err);
    assert!(!o.out.contains("pwned"));
}

#[test]
fn a_secret_a_connector_names_reaches_its_child_and_is_in_no_file() {
    let d = Dirs::new("connsecret");
    program(&d, "keyed", "#!/bin/sh\necho \"the key is $MY_API_KEY\"; echo \"$MY_API_KEY\" > \"$PWD/seen-by-child\"");
    connector(&d, "keyed.toml", "api = 1\n[[connector]]\nid = \"keyed\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"{bin}/keyed\"\nprobe = { argv = [\"{bin}/keyed\"] }\nstart = []\nsecrets = { MY_API_KEY = \"file:{bin}/keyfile\" }\n");
    crate::support::key_file(&d.dir.join("bin/keyfile"), "sk-conn-canary-0123456789abcdef\n");
    let o = j4n(&d, &["run", "x", "--doer", "keyed"], None, &[]);
    assert_eq!(o.rc, 0, "{}", o.err);
    assert_eq!(fs::read_to_string(d.project().join("seen-by-child")).unwrap().trim(), "sk-conn-canary-0123456789abcdef", "the child had it");
    let mut files = Vec::new();
    every_file(&d.project().join(".j4n"), &mut files);
    for f in files {
        let bytes = fs::read(&f).unwrap();
        let c = b"sk-conn-canary-0123456789abcdef";
        assert!(!bytes.windows(c.len()).any(|w| w == c), "{} holds the key", f.display());
    }
}

#[test]
fn a_secret_of_no_known_shape_a_connector_names_is_scrubbed_from_what_its_child_says() {
    let d = Dirs::new("connsecret2");
    program(&d, "keyed", "#!/bin/sh\necho \"it said $MY_PIN\"");
    connector(&d, "keyed.toml", "api = 1\n[[connector]]\nid = \"keyed\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"{bin}/keyed\"\nprobe = { argv = [\"{bin}/keyed\"] }\nsecrets = { MY_PIN = \"file:{bin}/pin\" }\n");
    crate::support::key_file(&d.dir.join("bin/pin"), "plainwordsecret77\n");
    let o = j4n(&d, &["run", "x", "--doer", "keyed"], None, &[]);
    assert_eq!(o.rc, 0, "{}", o.err);
    let log = log_events(&d).iter().map(|e| e.to_string()).collect::<String>();
    assert!(!log.contains("plainwordsecret77") && log.contains("secret:MY_PIN"), "{log}");
}

/// A key file others can read, and a value too short to be kept out of the Log, are refused before the child starts: the words name the
/// variable and the connector, never the value.
#[test]
fn a_secret_file_others_can_read_or_a_value_too_short_to_scrub_is_refused_and_not_shown() {
    use std::os::unix::fs::PermissionsExt;
    for (tag, value, mode, said) in [("short", "ab12z", 0o600, "shorter than"), ("wide", "sk-conn-wide-0123456789abcdef", 0o644, "readable by others")] {
        let d = Dirs::new(&format!("connsecret-{tag}"));
        program(&d, "keyed", "#!/bin/sh\necho \"$MY_API_KEY\" > \"$PWD/seen-by-child\"");
        connector(&d, "keyed.toml", "api = 1\n[[connector]]\nid = \"keyed\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"{bin}/keyed\"\nprobe = { argv = [\"{bin}/keyed\"] }\nstart = []\nsecrets = { MY_API_KEY = \"file:{bin}/keyfile\" }\n");
        let file = d.dir.join("bin/keyfile");
        fs::write(&file, format!("{value}\n")).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(mode)).unwrap();
        let o = j4n(&d, &["run", "x", "--doer", "keyed"], None, &[]);
        assert_ne!(o.rc, 0, "{tag}: {}", o.out);
        assert!(!d.project().join("seen-by-child").exists(), "{tag}: the child was not started with it");
        let words = format!("{}{}", o.out, o.err);
        assert!(words.contains("MY_API_KEY") && words.contains("keyed") && words.contains(said), "{tag}: {words}");
        assert!(!words.contains(value), "{tag}: the value is never said: {words}");
    }
}

/// A made-up key of no vendor's shape, kept in J4n's key file for a provider: the kept-key path is what is proven, not a pattern.
const KEPT_KEY: &str = "kept-canary-plain-3f8a61c2d9";

/// The key J4n keeps for `provider`, as Library › Providers writes it (mode 0600 in a folder of mode 0700), and the setting `doer.keys`.
fn kept_key(d: &Dirs, provider: &str, given: &str) {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let dir = d.j4n_home().join("keys");
    fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    crate::support::key_file(&dir.join(provider), KEPT_KEY);
    fs::write(d.j4n_home().join("settings/keys.toml"), format!("api = 1\n[[setting]]\nid = \"doer.keys\"\nvalue = \"{given}\"\n")).unwrap();
}

/// Through the real binary: claude given its provider's kept key (the setting `doer.keys`) has `ANTHROPIC_API_KEY` in its environment and in
/// no argument; claude on its sign-in, the key kept all the same, is started without it.
#[test]
fn claude_given_the_kept_key_has_it_in_its_environment_and_in_no_argument_and_claude_on_its_sign_in_has_none() {
    for (given, has) in [("claude", true), ("", false)] {
        let d = Dirs::new(&format!("kept-claude-{has}"));
        kept_key(&d, "anthropic", given);
        let (program, fixture, log) = fake_claude(&d, "two-turns.jsonl");
        let envlog = d.dir.join("child-env");
        let env = [("J4N_CLAUDE", program.to_str().unwrap()), ("FAKE_FIXTURE", fixture.to_str().unwrap()), ("FAKE_LOG", log.to_str().unwrap()), ("FAKE_ENV_LOG", envlog.to_str().unwrap())];
        let o = j4n(&d, &["--doer", "claude", "--model", "haiku", "--approval", "full"], Some("first line\n"), &env);
        assert_eq!(o.rc, 0, "{has}: {}", o.err);
        let child = read(&envlog);
        assert_eq!(child.lines().any(|l| l == format!("ANTHROPIC_API_KEY={KEPT_KEY}")), has, "{has}:\n{child}");
        let starts: Vec<String> = read(&log).lines().filter(|l| l.starts_with("start ")).map(String::from).collect();
        assert!(!starts.is_empty(), "{has}: claude was started");
        assert!(!starts.iter().any(|l| l.contains(KEPT_KEY)), "{has}: never in an argument: {starts:?}");
        assert!(!o.out.contains(KEPT_KEY) && !o.err.contains(KEPT_KEY), "{has}: never said");
        assert!(!log_events(&d).iter().any(|e| e.to_string().contains(KEPT_KEY)), "{has}: never in the Log");
    }
}

/// The argv law for a kept key: every child the run starts for a program given one (its probe, its turn) is started with no argument that holds
/// it, and only the program given it has it in its environment; given and with no key kept, it is refused before it starts, in words with no value.
#[test]
fn a_kept_key_is_in_no_argument_of_any_child_and_a_program_given_one_with_none_kept_is_not_started() {
    let d = Dirs::new("kept-argv");
    kept_key(&d, "acme", "keyed");
    program(&d, "keyed", "#!/bin/sh\nprintf '%s\\n' \"argv $*\" >> \"$PWD/argv-seen\"\necho \"env ${ACME_API_KEY:-none}\" >> \"$PWD/argv-seen\"\necho done");
    connector(&d, "keyed.toml", "api = 1\n[[connector]]\nid = \"keyed\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"{bin}/keyed\"\nprobe = { argv = [\"{bin}/keyed\", \"--version\"] }\nstart = [\"{prompt}\"]\nsecrets = { ACME_API_KEY = \"kept:acme\" }\n");
    let o = j4n(&d, &["run", "say hello", "--doer", "keyed"], None, &[]);
    assert_eq!(o.rc, 0, "{}", o.err);
    let seen = read(&d.project().join("argv-seen"));
    let argvs: Vec<&str> = seen.lines().filter(|l| l.starts_with("argv ")).collect();
    assert!(argvs.iter().any(|l| l.contains("say hello")), "the turn's child was seen: {seen}");
    assert!(!argvs.iter().any(|l| l.contains(KEPT_KEY)), "no argument of any child holds it: {seen}");
    assert!(seen.lines().any(|l| l == format!("env {KEPT_KEY}")), "the program given it has it: {seen}");
    // the key file gone: the program given it is refused before it starts, and the words lead to the row
    fs::remove_file(d.j4n_home().join("keys/acme")).unwrap();
    fs::remove_file(d.project().join("argv-seen")).unwrap();
    let o = j4n(&d, &["run", "say hello", "--doer", "keyed"], None, &[]);
    assert_ne!(o.rc, 0, "{}", o.out);
    let words = format!("{}{}", o.out, o.err);
    assert!(words.contains("no key is kept for acme") && words.contains("ACME_API_KEY"), "{words}");
    assert!(!fs::read_to_string(d.project().join("argv-seen")).unwrap_or_default().contains("say hello"), "its turn did not start");
}

/// A model whose key file others can read is not used, and the task's error says why (no value in it), not only that there is no key.
#[test]
fn a_model_whose_key_file_is_refused_says_why_in_the_tasks_error() {
    use std::os::unix::fs::PermissionsExt;
    let d = Dirs::new("key-why");
    let file = d.dir.join("bin/model-key");
    fs::write(&file, format!("{KEPT_KEY}\n")).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    connector(&d, "m.toml", "api = 1\n[[connector]]\nid = \"wide\"\nis = \"agent\"\ndialect = \"openai\"\nurl = \"http://127.0.0.1:9/v1\"\nkey = \"file:{bin}/model-key\"\nmodels = [\"m-1\"]\n");
    let o = j4n(&d, &["run", "say hello", "--doer", "wide"], None, &[]);
    let words = format!("{}{}", o.out, o.err);
    assert!(words.contains("its key is not used") && words.contains("readable by others"), "{words}");
    assert!(!words.contains(KEPT_KEY), "{words}");
}

// ---- rules: what happens around a task is a file ----

/// A file of the operator's own, in J4n's home.
fn mine(d: &Dirs, name: &str, text: &str) {
    let path = d.j4n_home().join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn of_kind<'a>(events: &'a [serde_json::Value], kind: &str) -> Vec<&'a serde_json::Value> {
    events.iter().filter(|e| e["kind"] == kind).collect()
}

#[test]
fn a_task_with_work_left_is_carried_on_by_itself_and_a_setting_in_a_home_file_says_how_often() {
    let d = Dirs::new("followups");
    let o = j4n(&d, &["run", "todo 2; say partway", "--doer", "test"], None, &[]);
    assert_eq!(o.rc, 0, "{}", o.err);
    let events = log_events(&d);
    let queued: Vec<&str> = of_kind(&events, "task.queued").iter().filter_map(|e| e["task"].as_str()).collect();
    assert_eq!(queued, ["t1", "t2"], "carried on once, and the second had nothing left: {}", o.err);
    let fired = of_kind(&events, "rule.fired");
    assert_eq!((fired.len(), fired[0]["by"]["rule"].as_str()), (1, Some("followups")));
    assert!(o.out.contains("2 item(s) are still open. Carry on and finish them."), "the last task's report is the answer: {}", o.out);

    mine(&d, "settings.toml", "api = 1\n[[setting]]\nid = \"loop.followups\"\nvalue = 0\n");
    let again = Dirs::new("followups0");
    mine(&again, "settings.toml", "api = 1\n[[setting]]\nid = \"loop.followups\"\nvalue = 0\n");
    let o = j4n(&again, &["run", "todo 2; say partway", "--doer", "test"], None, &[]);
    assert_eq!((o.rc, o.out.as_str()), (0, "partway\n"), "{}", o.err);
    assert_eq!(of_kind(&log_events(&again), "task.queued").len(), 1, "none");
}

#[test]
fn a_limit_is_waited_out_and_the_task_goes_on_by_itself() {
    let d = Dirs::new("limit");
    mine(&d, "failures.toml", "api = 1\n[[failure]]\nid = \"limit\"\nwait = \"1s\"\n");
    let o = j4n(&d, &["run", "fail usage limit reached", "--doer", "test", "--wall", "30s"], None, &[]);
    assert_eq!((o.rc, o.out.as_str()), (0, "Carry on where you left off.\n"), "{}", o.err);
    assert!(o.secs >= 1.0, "it waited: {}s", o.secs);
    let events = log_events(&d);
    assert_eq!(of_kind(&events, "task.cooling").len(), 1);
    assert_eq!(of_kind(&events, "task.stopped").len(), 0, "never stopped");
    assert_eq!(of_kind(&events, "run.error")[0]["data"]["class"], "rate-limit");
}

#[test]
fn a_limit_that_ends_after_the_time_the_run_was_given_ends_the_turn_at_once_with_one_line_that_says_when() {
    let d = Dirs::new("limit-clock");
    mine(&d, "failures.toml", "api = 1\n[[failure]]\nid = \"limit\"\nwait = \"10m\"\n");
    // the wall is long, and the bound is well under it, so that a loaded machine does not read as a run that waited for the limit
    let o = j4n(&d, &["run", "fail usage limit reached", "--doer", "test", "--wall", "120s"], None, &[]);
    assert_ne!(o.rc, 0, "{}", o.out);
    assert!(o.secs < 60.0, "it did not wait for a limit that ends after the clock: {}s", o.secs);
    assert!(o.err.contains("is limited until in 9m") && o.err.contains("usage limit reached"), "{}", o.err);
    assert_eq!(of_kind(&log_events(&d), "task.cooling").len(), 1);
}

#[test]
fn a_task_past_its_soft_cap_is_parked_and_the_menu_says_it_can_be_carried_on() {
    let d = Dirs::new("cap");
    let o = j4n(&d, &["run", "spend 0.006; hang", "--doer", "test", "--usd", "0.005", "--wall", "120s"], None, &[]);
    assert_ne!(o.rc, 0, "a task that was parked did not finish: {}", o.out);
    let events = log_events(&d);
    let stopped = of_kind(&events, "task.stopped");
    assert_eq!((stopped.len(), stopped[0]["data"]["why"]["kind"].as_str(), stopped[0]["by"]["rule"].as_str()), (1, Some("capped"), Some("soft-cap")), "{}", o.err);
    assert!(o.secs < 60.0, "it did not wait for its wall (120 s): {}s", o.secs);
}

#[test]
fn a_rule_of_the_operators_own_runs_and_one_that_says_what_a_doer_wrote_bare_is_refused() {
    let d = Dirs::new("rules");
    mine(
        &d,
        "rules.toml",
        "api = 1\n[[rule]]\nid = \"done\"\non = \"task.settled\"\nthen = \"notice\"\ntext = \"{task} is over\"\n[[rule]]\nid = \"leak\"\non = \"task.settled\"\nthen = \"notice\"\ntext = \"{ev.data.answer}\"\n",
    );
    let o = j4n(&d, &["run", "say ok", "--doer", "test"], None, &[]);
    assert_eq!((o.rc, o.out.as_str()), (0, "ok\n"), "{}", o.err);
    assert!(o.err.contains("rule leak") && o.err.contains("wrote, bare"), "{}", o.err);
    let events = log_events(&d);
    let notices: Vec<(&str, &str)> = of_kind(&events, "run.notice").iter().map(|e| (e["by"]["rule"].as_str().unwrap_or(""), e["data"]["text"].as_str().unwrap_or(""))).collect();
    assert!(notices.contains(&("done", "t1 is over")), "{notices:?}");
    assert!(!notices.iter().any(|(by, _)| *by == "leak"), "the refused rule never ran");
}

#[test]
fn a_project_cannot_bring_its_own_rules_or_settings() {
    let d = Dirs::new("projrules");
    d.ledger_off();
    d.habits_off();
    d.put(".j4n/rules.toml", "api = 1\n[[rule]]\nid = \"evil\"\non = \"task.settled\"\nthen = \"start\"\ntext = \"say pwned\"\n[[setting]]\nid = \"loop.followups\"\nvalue = 20\n");
    let o = j4n(&d, &["run", "todo 5; say partway", "--doer", "test"], None, &[]);
    assert_eq!(o.rc, 0, "{}", o.err);
    let events = log_events(&d);
    assert!(of_kind(&events, "rule.fired").iter().all(|e| e["by"]["rule"] == "followups"), "{events:?}");
    assert!(!o.out.contains("pwned"), "{}", o.out);
    assert!(of_kind(&events, "task.queued").len() <= 4, "the project's setting did not raise the follow-ups: 1 + 3");
}

#[test]
fn only_a_task_a_rule_carried_on_is_followed_to_the_end_of_a_run() {
    use j4n::lines::carried_on;
    use j4n_core::task::{Row, TaskSpec};
    let row = |id: &str, follows: Option<&str>, auto: u32| -> Row {
        let mut spec = TaskSpec::new("go", "test");
        spec.follows = follows.map(String::from);
        serde_json::from_value(serde_json::json!({ "id": id, "spec": spec, "state": { "state": "queued" }, "queued_ms": 0, "auto": auto })).unwrap()
    };
    let rows = vec![row("t1", None, 0), row("t2", Some("t1"), 0), row("t3", Some("t2"), 1)];
    assert_eq!(carried_on(rows.clone(), "t1"), None, "an operator's follow-up is theirs to watch");
    assert_eq!(carried_on(rows.clone(), "t2").as_deref(), Some("t3"));
    assert_eq!(carried_on(rows, "t3"), None);
}

// ---- places: a worktree, a box, a check where the work was done ----

fn git(d: &Dirs, args: &[&str]) -> String {
    let out = Command::new("git").arg("-C").arg(d.project()).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A repository with one commit: `a.txt`, a `.gitignore` for `ledger/` and `.j4n/`, and a Makefile whose test passes.
fn repo(d: &Dirs, makefile: &str) {
    git(d, &["init", "-q", "-b", "main"]);
    d.put("a.txt", "one\n");
    d.put(".gitignore", "ledger/\n.j4n/\n");
    d.put("Makefile", makefile);
    git(d, &["add", "-A"]);
    git(d, &["commit", "-qm", "base"]);
}

fn setting(d: &Dirs, id: &str, value: &str) {
    let path = d.j4n_home().join("settings.toml");
    let before = fs::read_to_string(&path).unwrap_or_else(|_| "api = 1\n".into());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("{before}[[setting]]\nid = \"{id}\"\nvalue = {value}\n")).unwrap();
}

fn worktrees(d: &Dirs) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let root = d.j4n_home().join("worktrees");
    for project in fs::read_dir(&root).into_iter().flatten().flatten() {
        for e in fs::read_dir(project.path()).into_iter().flatten().flatten().filter(|e| e.path().is_dir()) {
            found.push(e.path());
        }
    }
    found
}

fn bwrap_works() -> bool {
    Command::new("bwrap").args(["--ro-bind", "/", "/", "true"]).output().is_ok_and(|o| o.status.success())
}

#[test]
fn a_write_into_a_gitignored_folder_by_a_worktree_task_is_in_the_real_folder_afterwards() {
    let d = Dirs::new("run-write-into-gitignored");
    repo(&d, "test:\n\ttrue\n");
    let o = j4n(&d, &["run", "write ledger/found/x.json {}; say saved", "--doer", "test", "--place", "worktree", "--approval", "full"], None, &[]);
    assert_eq!((o.rc, o.out.as_str()), (0, "saved\n"), "{}", o.err);
    assert_eq!(read(&d.project().join("ledger/found/x.json")), "{}");
    assert_eq!(git(&d, &["status", "--porcelain"]), "", "ignored, so git sees nothing — and nothing else was touched");
    assert!(worktrees(&d).is_empty(), "the copy is gone: its work is in the folder");
    let events = log_events(&d);
    let applied = of_kind(&events, "task.applied");
    assert_eq!(applied.len(), 1, "{}", o.err);
    assert_eq!(applied[0]["data"]["work"]["ignored"][0], "ledger/found/x.json");
    assert!(o.err.contains("laid into your folder") && o.err.contains("ledger/found/x.json"), "{}", o.err);
    assert!(of_kind(&events, "task.kept").is_empty());
}

#[test]
fn a_worktree_task_that_wrote_an_ignored_file_is_kept_with_the_file_named_when_it_is_not_applied() {
    let d = Dirs::new("run-worktree-task-wrote");
    repo(&d, "test:\n\ttrue\n");
    setting(&d, "work.apply", "\"off\"");
    let o = j4n(&d, &["run", "write ledger/found/x.json {}; write a.txt two; say saved", "--doer", "test", "--place", "worktree", "--approval", "full"], None, &[]);
    assert_eq!((o.rc, o.out.as_str()), (0, "saved\n"), "{}", o.err);
    assert!(!d.project().join("ledger").exists() && read(&d.project().join("a.txt")) == "one\n", "nothing reached the folder");
    let kept = worktrees(&d);
    assert_eq!(kept.len(), 1, "the copy is still there: {kept:?}");
    assert_eq!(read(&kept[0].join("ledger/found/x.json")), "{}");
    assert!(o.err.contains("kept in") && o.err.contains("ledger/found/x.json") && o.err.contains("a.txt") && o.err.contains("would apply cleanly"), "{}", o.err);
    let events = log_events(&d);
    let k = of_kind(&events, "task.kept");
    assert_eq!((k.len(), k[0]["data"]["work"]["ignored"][0].as_str(), k[0]["data"]["work"]["files"][0].as_str()), (1, Some("ledger/found/x.json"), Some("a.txt")));
}

#[test]
fn a_folder_with_no_commit_and_a_place_that_is_not_there_are_refused_with_the_way_out() {
    let d = Dirs::new("noplace");
    git(&d, &["init", "-q", "-b", "main"]);
    let o = j4n(&d, &["run", "say ok", "--doer", "test", "--place", "worktree"], None, &[]);
    assert_ne!(o.rc, 0);
    assert!(o.err.contains("has no commit yet") && o.err.contains("first commit"), "{}", o.err);
    let o = j4n(&d, &["run", "say ok", "--doer", "test", "--place", "nowhere"], None, &[]);
    assert_ne!(o.rc, 0);
    assert!(o.err.contains("no place called `nowhere`"), "{}", o.err);
}

#[test]
fn a_box_that_cannot_run_refuses_the_task_and_says_the_way_out_or_runs_it_unboxed_when_the_setting_says() {
    let d = Dirs::new("broken-box");
    mine(&d, "boxes.toml", "api = 1\n[[box]]\nid = \"broken\"\nprobe = [\"false\"]\npre = [\"false\"]\nro = []\nrw = []\nhide_dir = []\nhide_file = []\nend = [\"--\"]\n");
    setting(&d, "box.use", "\"broken\"");
    let o = j4n(&d, &["run", "say ok", "--doer", "test", "--place", "box"], None, &[]);
    assert_ne!(o.rc, 0);
    assert!(o.err.contains("no box can run here") && o.err.contains("broken") && o.err.contains("box.unavailable"), "{}", o.err);
    setting(&d, "box.unavailable", "\"here\"");
    let o = j4n(&d, &["run", "say ok", "--doer", "test", "--place", "box"], None, &[]);
    assert_eq!((o.rc, o.out.as_str()), (0, "ok\n"), "{}", o.err);
    assert!(o.err.contains("runs unboxed"), "{}", o.err);
}

#[test]
fn a_task_in_a_box_says_which_wall_it_is_behind() {
    if !bwrap_works() {
        return;
    }
    let d = Dirs::new("box");
    let o = j4n(&d, &["run", "say ok", "--doer", "test", "--place", "box"], None, &[]);
    assert_eq!((o.rc, o.out.as_str()), (0, "ok\n"), "{}", o.err);
    assert!(o.err.contains("behind bwrap") && o.err.contains("credentials and J4n's own folder are hidden"), "{}", o.err);
}

#[test]
fn the_project_check_is_run_where_the_work_was_done_by_default_and_not_when_the_switch_is_off() {
    let d = Dirs::new("check");
    repo(&d, "test:\n\tpwd > checked-in.txt\n");
    let o = j4n(&d, &["run", "write a.txt two; say changed", "--doer", "test", "--approval", "full"], None, &[]);
    assert_eq!(o.rc, 0, "{}", o.err);
    assert!(!d.project().join("checked-in.txt").exists(), "nothing ran: the switch is off (here it is pinned off)");
    assert!(of_kind(&log_events(&d), "task.queued").iter().all(|e| e["data"]["spec"]["doer"] != "check"));
    let d = Dirs::new("check-on");
    repo(&d, "test:\n\tsleep 1; pwd > checked-in.txt\n");
    d.defaults_as_shipped(); // on by default
    let o = j4n(&d, &["run", "write a.txt two; say changed", "--doer", "test", "--approval", "full"], None, &[]);
    assert_eq!((o.rc, o.out.as_str()), (0, "changed\n"), "{}", o.err);
    let events = log_events(&d);
    let queued: Vec<&serde_json::Value> = of_kind(&events, "task.queued").into_iter().filter(|e| e["data"]["spec"]["doer"] == "check").collect();
    assert_eq!(queued.len(), 1, "{}", o.err);
    assert_eq!((queued[0]["data"]["spec"]["place"].as_str(), queued[0]["by"]["rule"].as_str()), (Some("@t1"), Some("lane-check")));
    let check = queued[0]["task"].as_str().unwrap();
    let settled: Vec<&serde_json::Value> = of_kind(&events, "task.settled").into_iter().filter(|e| e["task"] == check).collect();
    assert_eq!(settled[0]["data"]["answer"], "7-make: passed", "{}", o.err);
    assert_eq!(read(&d.project().join("checked-in.txt")).trim(), d.project().canonicalize().unwrap().display().to_string(), "it ran in the folder the work was done in");
}

#[test]
fn a_change_waiting_in_a_worktree_is_checked_there_and_a_red_check_is_a_failed_task_with_its_output() {
    let d = Dirs::new("check-wt");
    repo(&d, "test:\n\tpwd > checked-in.txt; echo broke the build; exit 3\n");
    d.defaults_as_shipped();
    setting(&d, "work.apply", "\"off\"");
    let o = j4n(&d, &["run", "write a.txt two; say changed", "--doer", "test", "--place", "worktree", "--approval", "full"], None, &[]);
    assert_eq!((o.rc, o.out.as_str()), (0, "changed\n"), "the task itself went well: {}", o.err);
    let kept = worktrees(&d);
    assert_eq!(kept.len(), 1);
    assert!(kept[0].join("checked-in.txt").exists() && !d.project().join("checked-in.txt").exists(), "it ran in the copy, not in the folder");
    let events = log_events(&d);
    let check = of_kind(&events, "task.queued").into_iter().find(|e| e["data"]["spec"]["doer"] == "check").expect("a check was started")["task"].as_str().unwrap().to_string();
    let stopped: Vec<&serde_json::Value> = of_kind(&events, "task.stopped").into_iter().filter(|e| e["task"] == check.as_str()).collect();
    let why = stopped[0]["data"]["why"]["message"].as_str().unwrap_or_default();
    assert!(why.contains("7-make check failed (exit 2)") && why.contains("broke the build"), "{why}");
    let shell = of_kind(&events, "run.shell");
    assert!(shell.iter().any(|e| e["task"] == check.as_str() && e["data"]["exit"] == 2 && e["data"]["cmd"] == "make test"), "{shell:?}");
}

#[test]
fn a_change_that_was_laid_into_the_folder_is_checked_there() {
    let d = Dirs::new("check-applied");
    repo(&d, "test:\n\tsleep 1; pwd > checked-in.txt\n");
    d.defaults_as_shipped(); // on by default
    let o = j4n(&d, &["run", "write a.txt two; say changed", "--doer", "test", "--place", "worktree", "--approval", "full"], None, &[]);
    assert_eq!(o.rc, 0, "{}", o.err);
    let events = log_events(&d);
    let queued: Vec<&serde_json::Value> = of_kind(&events, "task.queued").into_iter().filter(|e| e["data"]["spec"]["doer"] == "check").collect();
    assert_eq!((queued[0]["data"]["spec"]["place"].as_str(), queued[0]["by"]["rule"].as_str()), (Some("here"), Some("lane-check-applied")), "{}", o.err);
    assert!(d.project().join("checked-in.txt").exists());
}

#[test]
fn work_a_killed_run_left_in_a_worktree_is_found_by_the_next_one_and_said() {
    let d = Dirs::new("recover");
    repo(&d, "test:\n\ttrue\n");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_j4n"));
    cmd.args(["run", "write made.txt in-the-copy; hang", "--doer", "test", "--place", "worktree", "--approval", "full"])
        .current_dir(d.project())
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", d.home())
        .env("J4N_HOME", d.j4n_home())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().unwrap();
    let t = Instant::now();
    while !worktrees(&d).iter().any(|w| w.join("made.txt").exists()) {
        assert!(t.elapsed() < Duration::from_secs(80), "the task never wrote its file");
        std::thread::sleep(Duration::from_millis(20));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    let o = j4n(&d, &["run", "say ok", "--doer", "test"], None, &[]);
    assert_eq!((o.rc, o.out.as_str()), (0, "ok\n"), "{}", o.err);
    assert!(o.err.contains("t1 left work in") && o.err.contains("that is not in your folder") && o.err.contains("1 file"), "{}", o.err);
    assert!(!d.project().join("made.txt").exists(), "and it was only said: nothing was applied by itself");
    let events = log_events(&d);
    let kept = of_kind(&events, "task.kept");
    assert_eq!((kept.len(), kept[0]["task"].as_str()), (1, Some("t1")));
    // said once: another start finds it kept already
    let again = j4n(&d, &["run", "say ok", "--doer", "test"], None, &[]);
    assert!(!again.err.contains("t1 left work in"), "{}", again.err);
}

#[test]
fn a_run_on_a_plan_keeps_the_windows_in_the_log_and_says_what_its_price_says() {
    let goal = "limits 0.0 0.02; spend 0.25; limits 0.01 0.03; spend 0.25; say done";
    let plan = |price: &str| format!("api = 1\n[[plan]]\nid = \"test\"\nconnector = \"test\"\nwindows = [\"five_hour=5h\", \"seven_day=7d\"]\n{price}\n");
    let metrics = |d: &Dirs| log_events(d).into_iter().filter(|e| e["kind"] == "run.metrics").map(|e| e["data"].clone()).collect::<Vec<_>>();
    // the operator's plan has no price: the list cost stands, and the request says in words where to set it
    let d = Dirs::new("run-unset");
    std::fs::create_dir_all(d.j4n_home()).unwrap();
    std::fs::write(d.j4n_home().join("plan.toml"), plan("")).unwrap();
    let o = j4n(&d, &["run", goal, "--doer", "test"], None, &[]);
    assert_eq!(o.rc, 0, "{}\n{}", o.out, o.err);
    assert_eq!(log_events(&d).iter().filter(|e| e["kind"] == "run.window").count(), 4);
    assert!(metrics(&d).iter().all(|m| m["plan"].as_str().is_some_and(|w| w.starts_with("no price: Library › Plans, ⏎ on ")) && m["usd"] == 0.25 && m.get("share").is_none()), "{:?}", metrics(&d));
    // with the price and an assumption in their own row, a request says what it assumes the plan paid
    let d = Dirs::new("run-priced");
    std::fs::create_dir_all(d.j4n_home()).unwrap();
    std::fs::write(d.j4n_home().join("plan.toml"), plan("price = \"$24\"\nassume = \"$1000\"")).unwrap();
    let o = j4n(&d, &["run", goal, "--doer", "test"], None, &[]);
    assert_eq!(o.rc, 0, "{}\n{}", o.out, o.err);
    let m = metrics(&d);
    assert!(m.iter().all(|m| m["source"] == "assumed" && (m["plan_usd"].as_f64().unwrap() - 0.006).abs() < 1e-9), "{m:?}");
    // a project with no plan for its doer: the Log is what it was
    let d = Dirs::new("run-key");
    let o = j4n(&d, &["run", goal, "--doer", "test"], None, &[]);
    assert_eq!(o.rc, 0, "{}\n{}", o.out, o.err);
    assert!(metrics(&d).iter().all(|m| m.get("plan").is_none() && m.get("plan_usd").is_none()), "{:?}", metrics(&d));
}

#[test]
fn what_a_window_is_worth_is_learned_from_one_run_and_used_by_the_next() {
    let d = Dirs::new("run-learned");
    std::fs::create_dir_all(d.j4n_home()).unwrap();
    let setting = "[[setting]]\nid = \"cost.plan.min_n\"\ndoc = \"x\"\nty = \"int\"\ndefault = 5\nvalue = 1\n";
    let plan = "[[plan]]\nid = \"test\"\nconnector = \"test\"\nwindows = [\"five_hour=5h\", \"seven_day=7d\"]\nprice = \"$24\"\nassume = \"$1000\"\n";
    std::fs::write(d.j4n_home().join("plan.toml"), format!("api = 1\n{plan}{setting}")).unwrap();
    // the five-hour window moves two steps while $0.07 and $0.14 are spent: a pair, $14 a full window
    let first = j4n(&d, &["run", "limits 0 0.00; spend 0.07; limits 0 0.01; spend 0.14; limits 0 0.02; say done", "--doer", "test"], None, &[]);
    assert_eq!(first.rc, 0, "{}\n{}", first.out, first.err);
    // another run, another process: the Log has what the first learned
    let second = j4n(&d, &["run", "limits 0 0.02; spend 0.28; say done", "--doer", "test"], None, &[]);
    assert_eq!(second.rc, 0, "{}\n{}", second.out, second.err);
    let last = log_events(&d).into_iter().rfind(|e| e["kind"] == "run.metrics").unwrap()["data"].clone();
    assert_eq!((&last["source"], &last["window"]), (&serde_json::json!("measured n=1"), &serde_json::json!("five_hour")), "{last}");
    assert!((last["plan_usd"].as_f64().unwrap() - 0.28 * (24.0 * 5.0 / 24.0 / 30.44) / 14.0).abs() < 1e-9, "{last}");
}

// ---- a plan the line began is the work ----

/// A stand-in agent that plans when it is asked for a plan (`plan` is what it answers with) and otherwise does what it is told: it
/// appends the first line of its prompt to `done.txt` in the folder it runs in. A headless run plans a plan-shaped goal under `lanes = auto`
/// (the default is `single`, so the J4n home here says `auto` itself).
fn planner(d: &Dirs, plan: &str) {
    fs::write(d.j4n_home().join("settings/lanes.toml"), "api = 1\n[[setting]]\nid = \"lanes\"\nvalue = \"auto\"\n").unwrap();
    let escaped = plan.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        "#!/bin/sh\n[ \"$1\" = \"--version\" ] && {{ echo \"planner 1\"; exit 0; }}\nshift 2\nwhile [ $# -gt 1 ]; do case \"$1\" in --model|--resume) shift 2;; *) shift;; esac; done\nprompt=\"$1\"\nprintf '{{\"type\":\"start\",\"session_id\":\"ns-1\"}}\\n'\ncase \"$prompt\" in\n  *\"Plan this for the project\"*) printf '{{\"type\":\"text\",\"text\":\"%s\"}}\\n' '{}';;\n  *) printf '%s\\n' \"$prompt\" | head -n 1 >> done.txt; printf '{{\"type\":\"text\",\"text\":\"did it\"}}\\n';;\nesac\nprintf '{{\"type\":\"result\",\"usage\":{{\"cost_usd\":0.001}}}}\\n'\n",
        escaped.replace('\'', "'\\''")
    );
    program(d, "planner", &script);
    connector(
        d,
        "planner.toml",
        &read(&root().join("fixtures/connectors/newagent.toml")).replace("newagent", "planner"),
    );
}

const STEPS: &str = "Plan this and do it in order: (1) make a (2) make b (3) make c";

#[test]
fn a_plan_shaped_line_in_a_run_is_worked_through_the_plan_and_not_left_as_the_planning_turns_answer() {
    let d = Dirs::new("run-plan");
    let items: Vec<String> = ["a", "b", "c"].iter().map(|id| format!("{{\"id\":\"{id}\",\"goal\":\"make {id}\",\"reads\":[],\"writes\":[],\"depends\":[],\"done\":\"{id} exists\",\"where\":\"inline\"}}")).collect();
    planner(&d, &format!("{{\"approach\":\"a then b then c\",\"items\":[{}]}}", items.join(",")));
    let o = j4n(&d, &["run", STEPS, "--doer", "planner", "--approval", "full", "--wall", "60s"], None, &[]);
    assert_eq!(o.rc, 0, "{}{}", o.out, o.err);
    let done = read(&d.project().join("done.txt"));
    assert!(
        done.contains("make a") && done.contains("make b") && done.contains("make c"),
        "every item was worked: {done:?}\n{}",
        o.err
    );
    assert!(!o.out.contains("approach"), "the plan is J4n's, the work's answer is the run's: {}", o.out);
}

#[test]
fn a_planning_turn_that_gives_no_plan_is_a_failed_run_that_says_the_goal_was_never_worked_on() {
    let d = Dirs::new("run-none");
    planner(&d, "I would start by reading the folder.");
    let o = j4n(&d, &["run", STEPS, "--doer", "planner", "--approval", "full", "--wall", "60s"], None, &[]);
    assert_eq!(o.rc, 1, "{}{}", o.out, o.err);
    assert!(o.err.contains("the goal was never worked on"), "{}", o.err);
    assert!(!d.project().join("done.txt").exists());
}

#[test]
fn look_prints_one_json_object_and_exits_1_with_an_error_object_when_the_look_cannot_be_made() {
    // no cheap tier is named in a bare folder; a doer J4n has that cannot read is no look either; a doer it has not is said so
    let d = Dirs::new("look-cli");
    for (args, said) in [(vec!["look", "fix", "the", "typo"], "no cheap tier"), (vec!["look", "--doer", "test", "fix the typo"], "not ready for a read"), (vec!["look", "--doer", "nobody", "fix the typo"], "not one J4n has")] {
        let out = j4n(&d, &args, None, &[]);
        assert_eq!(out.rc, 1, "{args:?}: {}{}", out.out, out.err);
        let v: serde_json::Value = serde_json::from_str(out.out.trim()).unwrap_or_else(|e| panic!("{args:?}: {e}: {}", out.out));
        assert!(v["error"].as_str().is_some_and(|e| e.contains(said)), "{args:?}: {v}");
        assert_eq!(v.as_object().map(|o| o.len()), Some(1), "only the error");
    }
    // nothing was sent to a chat and no task was made for a look that was never asked
    assert!(!log_events(&d).iter().any(|e| e["kind"] == "chat.say" || e["kind"] == "task.queued"), "{:?}", log_events(&d).iter().map(|e| e["kind"].clone()).collect::<Vec<_>>());
    assert_eq!(j4n(&d, &["look"], None, &[]).rc, 2);
    assert!(j4n(&d, &["look", "--help"], None, &[]).out.contains("The read of the orchestrate mode"));
}
