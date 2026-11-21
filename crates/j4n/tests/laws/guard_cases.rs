//! The restart guard (`guard.rs`) and `:restart`: a build that comes up and a build that does not, as scripts; the guard is the real
//! one, and what it starts is a file that is executable.

use crate::run_cases::Dirs;
use j4n::guard::{Guard, RESTART_CODE};
use j4n_core::front::Said;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn build(d: &Dirs, name: &str, body: &str) -> PathBuf {
    let p = d.dir.join("bin").join(name);
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn count(d: &Dirs, name: &str) -> usize {
    fs::read_to_string(d.dir.join(name)).map(|t| t.lines().count()).unwrap_or(0)
}

/// A build that says it came up, notes that it ran, and ends with `code`.
fn good(d: &Dirs, name: &str, code: i32) -> PathBuf {
    let log = d.dir.join(format!("{name}.ran"));
    build(d, name, &format!("echo \"$J4N_START_NOTE\" >> '{}'\necho $$ > \"$J4N_READY_FILE\"\nexit {code}", log.display()))
}

#[test]
fn a_build_that_comes_up_and_ends_is_the_end_and_its_own_status_is_the_guards() {
    let d = Dirs::new("guard-ends");
    let b = good(&d, "b", 7);
    let g = Guard::new(&d.dir.join("guard")).within(5);
    assert_eq!(g.run(b.clone(), &|| b.clone(), &[]), 7);
    assert_eq!(count(&d, "b.ran"), 1);
}

#[test]
fn a_build_that_asks_for_a_restart_is_started_again_and_the_one_that_came_up_is_kept_to_go_back_to() {
    let d = Dirs::new("guard-again");
    let counter = d.dir.join("n");
    // the first start asks for another, the second ends
    let b = build(&d, "b", &format!("echo x >> '{c}'\necho $$ > \"$J4N_READY_FILE\"\nif [ $(wc -l < '{c}') -lt 2 ]; then exit {RESTART_CODE}; fi\nexit 0", c = counter.display()));
    let g = Guard::new(&d.dir.join("guard")).within(5);
    assert_eq!(g.run(b.clone(), &|| b.clone(), &["--flag".into()]), 0);
    assert_eq!(count(&d, "n"), 2);
    let kept = d.dir.join("guard/previous");
    assert!(kept.exists() && fs::metadata(&kept).unwrap().permissions().mode() & 0o777 == 0o700, "the build that came up is kept");
    assert_eq!(fs::read(&kept).unwrap(), fs::read(&b).unwrap());
}

#[test]
fn a_build_that_dies_before_its_first_frame_is_marked_bad_and_the_one_before_it_comes_back_with_a_line_about_it() {
    let d = Dirs::new("guard-dies");
    let before = good(&d, "before", 0);
    let bad = build(&d, "bad", &format!("echo x >> '{}'\nexit 3", d.dir.join("bad.ran").display()));
    let g = Guard::new(&d.dir.join("guard")).within(5);
    assert_eq!(g.run(before.clone(), &|| bad.clone(), &[]), 0);
    let note = fs::read_to_string(d.dir.join("before.ran")).unwrap();
    assert!(note.contains("restart failed") && note.contains("exit 3") && note.contains("your chat is intact"), "{note}");
    assert_eq!(count(&d, "bad.ran"), 1);
    // it is not tried again while it is the same file, and the one before it comes back at once
    assert_eq!(g.run(before.clone(), &|| bad.clone(), &[]), 0);
    assert_eq!(count(&d, "bad.ran"), 1, "the same build is not started a second time");
    assert_eq!(count(&d, "before.ran"), 2);
    // changed, it is tried: a build that has been mended is not held to what it was
    fs::write(&bad, "#!/bin/sh\necho $$ > \"$J4N_READY_FILE\"\nexit 0\n").unwrap();
    assert_eq!(g.run(before.clone(), &|| bad.clone(), &[]), 0);
    assert_eq!(count(&d, "before.ran"), 2, "the mended build came up by itself");
}

#[test]
fn a_build_that_stays_silent_is_stopped_when_the_time_is_out() {
    let d = Dirs::new("guard-silent");
    let before = good(&d, "before", 0);
    let silent = build(&d, "silent", "sleep 30");
    let g = Guard::new(&d.dir.join("guard")).within(1);
    let t = Instant::now();
    assert_eq!(g.run(before.clone(), &|| silent.clone(), &[]), 0);
    assert!(t.elapsed() < Duration::from_secs(10), "it was stopped, not waited for");
    assert!(fs::read_to_string(d.dir.join("before.ran")).unwrap().contains("no first frame in time"));
}

#[test]
fn with_no_other_build_to_go_back_to_the_guard_says_so_and_ends() {
    let d = Dirs::new("guard-alone");
    let bad = build(&d, "bad", &format!("echo x >> '{}'\nexit 1", d.dir.join("bad.ran").display()));
    let g = Guard::new(&d.dir.join("guard")).within(5);
    assert_eq!(g.run(bad.clone(), &|| bad.clone(), &[]), 1);
    assert_eq!(count(&d, "bad.ran"), 1, "the one build is started once, not again as the one to go back to");
    // a build that cannot be started at all, with one before it that cannot either
    let nowhere = d.dir.join("nowhere");
    assert_eq!(g.run(bad.clone(), &|| nowhere.clone(), &[]), 1);
}

#[test]
fn restart_ends_the_screen_for_the_guard_to_start_again_and_waits_for_a_task_that_goes_on() {
    use j4n::cli::{Approval, Opts};
    use j4n::wire::{self, Setup};
    use j4n_core::task::Mode;
    let d = Dirs::new("guard-command");
    let slow = build(&d, "slow", "sleep 20");
    let here = j4n_core::gate::grant::here_dir(&d.j4n_home(), &d.project());
    fs::create_dir_all(&here).unwrap();
    fs::write(here.join("settings.toml"), "api = 1\n[[setting]]\nid = \"box.unavailable\"\nvalue = \"here\"\n").unwrap();
    fs::create_dir_all(here.join("connectors")).unwrap();
    fs::write(here.join("connectors/slow.toml"), format!("api = 1\n[[connector]]\nid = \"slow\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"{}\"\nstart = [\"{{prompt}}\"]\n", slow.display())).unwrap();
    let opts = Opts { doer: Some("slow".into()), model: None, approval: Some(Approval::Full), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let world = wire::open(Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() }, &opts, false, "main").unwrap();
    let Said::Started { task } = world.front.say("main", "go") else { panic!("no task") };
    // a restart ends what goes on, so it waits for the operator to say so
    assert!(matches!(world.front.say("main", ":restart"), Said::Refused(why) if why.contains("is still going")));
    assert!(!world.front.restart_asked());
    world.front.say("main", ":kill");
    world.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).expect("ends");
    assert_eq!(world.front.say("main", ":restart"), Said::Quit);
    assert!(world.front.restart_asked());
    world.close();
}

#[test]
fn what_the_guard_was_given_to_start_with_is_given_to_every_build_it_starts() {
    let d = Dirs::new("guard-args");
    let b = build(&d, "b", &format!("echo \"$*\" >> '{}'\necho $$ > \"$J4N_READY_FILE\"\nexit 0", d.dir.join("args").display()));
    let g = Guard::new(&d.dir.join("guard")).within(5);
    assert_eq!(g.run(b.clone(), &|| b.clone(), &["--doer".into(), "claude".into()]), 0);
    assert_eq!(fs::read_to_string(d.dir.join("args")).unwrap().trim(), "--doer claude");
}
