//! The process runner: a child in a group of its own, read line by line, killed with all it started, reaped.

use crate::world::{Scratch, script};
use j4n_connect::proc::{Exit, LINE_MAX, TAIL_MAX, capture, spawn};
use j4n_core::place::Launch;
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn sh(dir: &Scratch, body: &str) -> Launch {
    Launch::new(vec!["/bin/sh".into(), "-c".into(), body.into()], dir.path().to_path_buf(), BTreeMap::new())
}

struct Ran {
    lines: Arc<Mutex<Vec<String>>>,
    exit: Receiver<Exit>,
    child: j4n_connect::proc::Child,
    stdin: Option<std::process::ChildStdin>,
}

fn run(launch: &Launch, piped: bool) -> Ran {
    let lines = Arc::new(Mutex::new(Vec::new()));
    let l = lines.clone();
    let (tx, exit) = mpsc::channel();
    let (child, stdin) = spawn(launch, piped, move |s| l.lock().unwrap().push(s.to_string()), move |e| drop(tx.send(e))).expect("starts");
    Ran { lines, exit, child, stdin }
}

impl Ran {
    fn end(&self) -> Exit {
        self.exit.recv_timeout(Duration::from_secs(20)).expect("the child ends")
    }

    fn lines(&self) -> Vec<String> {
        self.lines.lock().unwrap().clone()
    }
}

#[test]
fn a_child_streams_its_lines_and_ends_with_its_code() {
    let dir = Scratch::new("proc");
    let r = run(&sh(&dir, "for i in 1 2 3; do echo line$i; done; exit 4"), false);
    let exit = r.end();
    assert_eq!(r.lines(), vec!["line1", "line2", "line3"]);
    assert_eq!((exit.code, exit.signal, exit.killed, exit.broken), (Some(4), None, false, false));
    assert_eq!(exit.how(), "exit 4");
    assert!(!r.child.alive(), "reaped: no zombie is left to count as alive");
}

#[test]
fn stderr_is_kept_as_a_tail_never_mixed_into_the_lines() {
    let dir = Scratch::new("proc");
    let body = "echo first >&2; for i in $(seq 1 400); do echo noise-$i-padding-padding-padding >&2; done; echo last >&2; echo out";
    let r = run(&sh(&dir, body), false);
    let exit = r.end();
    assert_eq!(r.lines(), vec!["out"]);
    assert!(exit.tail.ends_with("last") && exit.tail.len() <= TAIL_MAX && !exit.tail.contains("first"), "newest kept, capped: {} bytes", exit.tail.len());
}

#[test]
fn the_working_folder_and_the_environment_reach_the_child() {
    let dir = Scratch::new("proc");
    let mut launch = sh(&dir, "pwd; echo $J4N_PROBE");
    launch.env.insert("J4N_PROBE".into(), "hello".into());
    let r = run(&launch, false);
    r.end();
    let here = std::fs::canonicalize(dir.path()).unwrap().display().to_string();
    let lines = r.lines();
    assert_eq!((std::fs::canonicalize(&lines[0]).unwrap().display().to_string(), lines[1].as_str()), (here, "hello"));
}

#[test]
fn a_program_that_trusts_pwd_over_the_real_folder_sees_the_real_folder() {
    // not through a shell, which would put right a PWD that is wrong
    let dir = Scratch::new("proc");
    let launch = Launch::new(vec!["/usr/bin/env".into()], dir.path().to_path_buf(), BTreeMap::new());
    let r = run(&launch, false);
    r.end();
    let lines = r.lines();
    let here = dir.path().display().to_string();
    assert!(lines.contains(&format!("PWD={here}")) && lines.contains(&format!("OLDPWD={here}")), "{lines:?}");
}

#[test]
fn a_carriage_return_before_the_line_end_is_not_part_of_the_line() {
    let dir = Scratch::new("proc");
    let r = run(&sh(&dir, "printf 'a\\r\\nb\\n\\r\\nc'"), false);
    r.end();
    assert_eq!(r.lines(), vec!["a", "b", "", "c"], "and a last line with no end is still a line");
}

#[test]
fn a_piped_stdin_is_what_the_child_reads_and_without_one_it_reads_nothing() {
    use std::io::Write;
    let dir = Scratch::new("proc");
    let mut r = run(&sh(&dir, "read a; echo got:$a"), true);
    writeln!(r.stdin.as_mut().unwrap(), "hi").unwrap();
    r.end();
    assert_eq!(r.lines(), vec!["got:hi"]);
    let r = run(&sh(&dir, "read a; echo eof:${a:-none}"), false);
    r.end();
    assert_eq!(r.lines(), vec!["eof:none"], "stdin is null, not the parent's");
}

#[test]
fn a_kill_takes_the_whole_group_down_and_says_it_was_ours() {
    let dir = Scratch::new("proc");
    let r = run(&sh(&dir, "echo started; sleep 300 & sleep 300; wait"), false);
    for _ in 0..200 {
        if !r.lines().is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(r.child.alive());
    r.child.kill();
    let exit = r.end();
    assert!(exit.killed && exit.signal == Some(9), "{exit:?}");
    assert!(!r.child.alive());
    r.child.kill(); // after the end: nothing is signalled, nothing breaks
}

#[test]
fn a_descendant_that_outlives_its_leader_is_not_left_running() {
    let dir = Scratch::new("proc");
    let pidfile = dir.join("pid");
    let body = format!("echo leader; sleep 300 >/dev/null 2>&1 & echo $! > {}; exit 0", pidfile.display());
    let r = run(&sh(&dir, &body), false);
    r.end();
    let pid: i32 = std::fs::read_to_string(&pidfile).unwrap().trim().parse().unwrap();
    // the backgrounded sleep shared the leader's group: the end of the output killed it
    let gone = (0..100).any(|_| {
        std::thread::sleep(Duration::from_millis(20));
        !std::path::Path::new(&format!("/proc/{pid}")).exists() || std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| s.contains(") Z "))
    });
    assert!(gone, "pid {pid} still runs");
}

#[test]
fn a_program_that_does_not_exist_is_an_error_not_a_hang() {
    let dir = Scratch::new("proc");
    let launch = Launch::new(vec!["/no/such/program".into()], dir.path().to_path_buf(), BTreeMap::new());
    assert!(spawn(&launch, false, |_| {}, |_| {}).is_err());
    let empty = Launch { argv: vec![], ..launch };
    assert!(spawn(&empty, false, |_| {}, |_| {}).is_err());
}

#[test]
fn a_line_longer_than_the_limit_is_cut_and_the_rest_dropped_and_the_next_line_is_whole() {
    let dir = Scratch::new("proc");
    let p = script(&dir, "long.sh", &format!("head -c {} /dev/zero | tr '\\0' x; echo; echo after", LINE_MAX + 5000));
    let r = run(&Launch::new(vec![p.display().to_string()], dir.path().to_path_buf(), BTreeMap::new()), false);
    r.end();
    let lines = r.lines();
    assert_eq!((lines.len(), lines[0].len(), lines[1].as_str()), (2, LINE_MAX, "after"));
}

#[test]
fn a_line_handler_that_panics_stops_the_child_and_says_so() {
    let dir = Scratch::new("proc");
    let (tx, exit) = mpsc::channel();
    let (child, _) = spawn(&sh(&dir, "echo one; sleep 300"), false, |_| panic!("the handler failed"), move |e| drop(tx.send(e))).unwrap();
    let e = exit.recv_timeout(Duration::from_secs(20)).expect("the child ends");
    assert!(e.broken && e.signal == Some(9), "{e:?}");
    assert!(!child.alive());
}

#[test]
fn bytes_that_are_not_text_do_not_stop_the_reader() {
    let dir = Scratch::new("proc");
    let r = run(&sh(&dir, "printf 'a\\377b\\n'; echo next"), false);
    r.end();
    let lines = r.lines();
    assert_eq!((lines.len(), lines[0].as_str(), lines[1].as_str()), (2, "a\u{fffd}b", "next"));
}

#[test]
fn a_short_question_to_a_program_is_answered_with_its_output_and_its_end() {
    let dir = Scratch::new("proc");
    let (text, exit) = capture(&sh(&dir, "echo one; echo two; exit 2"), Duration::from_secs(20)).unwrap();
    assert_eq!((text.as_str(), exit.code, exit.killed), ("one\ntwo", Some(2), false));
}

#[test]
fn a_program_that_outstays_its_limit_is_killed_with_what_it_started() {
    let dir = Scratch::new("proc");
    let started = std::time::Instant::now();
    let (text, exit) = capture(&sh(&dir, "echo before; sleep 300 & sleep 300"), Duration::from_millis(300)).unwrap();
    assert!(started.elapsed() < Duration::from_secs(20), "it did not wait for the sleep");
    assert_eq!(text, "before");
    assert!(exit.killed && exit.signal == Some(9), "{exit:?}");
    assert!(capture(&Launch { argv: vec!["/no/such/program".into()], ..sh(&dir, "") }, Duration::from_secs(1)).is_err());
}

#[test]
fn a_leader_that_closes_its_output_and_exits_a_moment_later_reports_its_own_code_not_a_kill() {
    let dir = Scratch::new("proc");
    let r = run(&sh(&dir, "echo before; exec >&- 2>&-; sleep 0.1; exit 7"), false);
    let exit = r.end();
    assert_eq!((exit.code, exit.signal, exit.killed), (Some(7), None, false), "{exit:?}");
    assert_eq!(r.lines(), ["before"]);
}

#[test]
fn what_the_leader_left_running_still_goes_with_it_once_its_output_is_over() {
    let dir = Scratch::new("proc");
    let r = run(&sh(&dir, "sleep 30 >/dev/null 2>&1 & echo $!; exit 0"), false);
    let exit = r.end();
    assert_eq!(exit.code, Some(0));
    let pid = r.lines()[0].trim().to_string();
    for _ in 0..200 {
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the background process {pid} is still there");
}

#[test]
fn a_leader_that_keeps_running_after_closing_its_output_is_killed_when_the_grace_is_up() {
    let dir = Scratch::new("proc");
    let r = run(&sh(&dir, "exec >&- 2>&-; sleep 30"), false);
    let t = std::time::Instant::now();
    let exit = r.end();
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    assert_eq!((exit.code, exit.signal), (None, Some(9)), "{exit:?}");
}

#[test]
fn a_question_in_flight_is_ended_with_j4n_and_its_group_goes_with_it() {
    let dir = Scratch::new("proc");
    let pidfile = dir.path().join("pids");
    // a leader and a grandchild, both asleep: the question would outlive the process that asked it
    let body = format!("sleep 60 & echo $! > {p}; echo $$ >> {p}; wait", p = pidfile.display());
    let launch = sh(&dir, &body);
    let book = Arc::new(j4n_connect::proc::Captures::default());
    let asker = {
        let book = book.clone();
        std::thread::spawn(move || j4n_connect::proc::capture_in(&book, &launch, Duration::from_secs(60)))
    };
    let end = std::time::Instant::now() + Duration::from_secs(10);
    while std::fs::read_to_string(&pidfile).map_or(true, |t| t.lines().count() < 2) {
        assert!(std::time::Instant::now() < end, "the question starts");
        std::thread::sleep(Duration::from_millis(10));
    }
    let pids: Vec<String> = std::fs::read_to_string(&pidfile).unwrap().lines().map(String::from).collect();
    book.end();
    let (_, exit) = asker.join().unwrap().expect("the asker is given its end");
    assert!(exit.killed, "{exit:?}");
    let gone = |pid: &str| !std::path::Path::new(&format!("/proc/{pid}")).exists() || std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| s.contains(") Z "));
    let end = std::time::Instant::now() + Duration::from_secs(5);
    while !pids.iter().all(|p| gone(p)) {
        assert!(std::time::Instant::now() < end, "still running: {pids:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_bound_child_ends_with_the_thread_that_started_it_and_a_plain_one_does_not() {
    let dir = Scratch::new("proc");
    let (bound_pid, plain_pid) = (dir.path().join("bound"), dir.path().join("plain"));
    let start = |bound: bool, pidfile: std::path::PathBuf| {
        let launch = sh(&dir, &format!("echo $$ > {p}; exec sleep 60", p = pidfile.display()));
        // the thread that starts the child ends at once: what J4n's death does to the thread that waits for a sign-in
        std::thread::spawn(move || {
            let (tx, _rx) = mpsc::channel();
            let done = move |e| drop(tx.send(e));
            if bound { j4n_connect::proc::spawn_bound(&launch, false, |_| {}, done) } else { spawn(&launch, false, |_| {}, done) }.expect("starts").0
        })
        .join()
        .unwrap()
    };
    let (bound, plain) = (start(true, bound_pid), start(false, plain_pid));
    let dead = |child: &j4n_connect::proc::Child| {
        let end = std::time::Instant::now() + Duration::from_secs(5);
        while child.alive() && std::time::Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        !child.alive()
    };
    assert!(dead(&bound), "the bound child ended with its thread");
    assert!(plain.alive(), "a plain child is not tied to it");
    plain.kill();
    assert!(dead(&plain));
}

/// How many of `rounds` scripts, each written and run at once, could not be run because they were busy, with other threads
/// forking all the while (a fork in the moment between the write and the exec keeps the script open for writing until it execs).
fn busy_runs(rounds: usize, retry: bool) -> usize {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let dir = Scratch::new("busy");
    let stop = Arc::new(AtomicBool::new(false));
    let noise: Vec<_> = (0..4)
        .map(|_| {
            let stop = stop.clone();
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let _ = std::process::Command::new("/bin/true").spawn().and_then(|mut c| c.wait());
                }
            })
        })
        .collect();
    let busy = Arc::new(AtomicUsize::new(0));
    let writers: Vec<_> = (0..4)
        .map(|w| {
            let (path, busy) = (dir.join(&format!("s{w}")), busy.clone());
            std::thread::spawn(move || {
                for _ in 0..rounds / 4 {
                    std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
                    let mut cmd = std::process::Command::new(&path);
                    cmd.stdin(std::process::Stdio::null());
                    match if retry { j4n_connect::proc::start(&mut cmd) } else { cmd.spawn() } {
                        Ok(mut c) => drop(c.wait()),
                        Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => drop(busy.fetch_add(1, Ordering::Relaxed)),
                        Err(e) => panic!("{e}"),
                    }
                }
            })
        })
        .collect();
    writers.into_iter().for_each(|w| w.join().unwrap());
    stop.store(true, Ordering::Relaxed);
    noise.into_iter().for_each(|n| n.join().unwrap());
    busy.load(Ordering::Relaxed)
}

#[test]
fn a_script_written_a_moment_ago_is_started_while_other_threads_fork() {
    let (plain, retried) = (busy_runs(2000, false), busy_runs(2000, true));
    eprintln!("of 2000 scripts, {plain} were busy when started plainly, {retried} through `start`");
    assert_eq!(retried, 0, "{retried} of 2000 scripts were still busy after the retries ({plain} were busy without them)");
}

/// The variables by which a program that runs agents binds a child to itself (the Claude desktop app: a messaging socket and token, the
/// session id, the account) are not given to a child, whatever started J4n; what J4n sets for the child, and the operator's own variables, are.
/// The names are set here in the test process: the one test of this binary that does, so no other reads them.
#[test]
fn a_child_is_given_none_of_the_hosts_session_and_all_that_j4n_sets() {
    let host = [
        ("CLAUDE_CODE_MESSAGING_SOCKET", "/run/user/1/cc-socks/1.sock"),
        ("CLAUDE_CODE_MESSAGING_TOKEN", "tok-of-the-app"),
        ("CLAUDECODE", "1"),
        ("CLAUDE_CODE_SESSION_ID", "s-1"),
        ("CODEX_THREAD_ID", "t-1"),
        ("HERDR_SOCKET_PATH", "/tmp/herdr.sock"),
    ];
    // SAFETY: nothing else in this binary reads these names, and the test runs its child before it returns
    unsafe {
        host.iter().for_each(|(k, v)| std::env::set_var(k, v));
        std::env::set_var("J4N_TEST_OPERATORS_OWN", "kept");
    }
    let dir = Scratch::new("proc");
    let mut launch = sh(&dir, "env | sort");
    // what J4n means to give: an account's token, a flag of the lean profile (its prefix is the host's too), a variable of its own
    launch.env = [("CLAUDE_CODE_OAUTH_TOKEN", "tok-of-the-account"), ("CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS", "1"), ("J4N_TEST_SET", "yes"), ("CLAUDE_CODE_SESSION_ID", "set-by-j4n")]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let r = run(&launch, false);
    r.end();
    let seen = r.lines();
    let has = |name: &str| seen.iter().any(|l| l.split('=').next() == Some(name));
    for (name, _) in host.iter().filter(|(k, _)| *k != "CLAUDE_CODE_SESSION_ID") {
        assert!(!has(name), "{name} reached the child:\n{}", seen.join("\n"));
    }
    assert!(has("PATH") && seen.iter().any(|l| l == "J4N_TEST_OPERATORS_OWN=kept"), "the operator's own environment stays:\n{}", seen.join("\n"));
    for set in ["CLAUDE_CODE_OAUTH_TOKEN=tok-of-the-account", "CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS=1", "J4N_TEST_SET=yes", "CLAUDE_CODE_SESSION_ID=set-by-j4n"] {
        assert!(seen.iter().any(|l| l == set), "what J4n sets arrives ({set}):\n{}", seen.join("\n"));
    }
    // a short question put to a program goes the same way
    let (out, _) = capture(&sh(&dir, "env | grep -c '^CLAUDE_CODE_MESSAGING_' || true"), Duration::from_secs(10)).unwrap();
    assert_eq!(out.trim(), "0");
}
