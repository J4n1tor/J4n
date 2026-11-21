//! Place `box`: the command line of the shipped wall, exactly (goldens), the order that makes it hold, what a task may touch,
//! a wall that cannot run, and — where bubblewrap runs — the real wall.

use crate::world::Scratch;
use j4n_connect::place::boxed::{Seen, hidden_in_goal, self, BoxRow, Fs, Spec, argv};
use j4n_connect::place::Boxed;
use j4n_core::place::{Here, Launch, Place, Prepare, Site};
use j4n_core::place::checkpoint::Cfg;
use j4n_core::setting::Settings;
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use j4n_core::task::{Mode, TaskSpec};
use j4n_core::value::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const SHIPPED: &str = include_str!("../../../../packs/base/boxes/bwrap.toml");

fn tables(text: &str) -> Tables {
    let layer = Layer::new(LayerKind::Pack, "boxes", vec![SourceFile { path: "boxes/b.toml".into(), text: text.into() }]);
    let (tables, problems) = Tables::load(vec![layer]);
    assert!(problems.is_empty(), "{problems:?}");
    tables
}

fn shipped() -> BoxRow {
    let (rows, problems) = boxed::rows(&tables(SHIPPED));
    assert!(problems.is_empty(), "{problems:?}");
    rows.into_iter().next().expect("the shipped row")
}

fn spec() -> Spec {
    Spec {
        dir: PathBuf::from("/home/me/.j4n/worktrees/k/t1"),
        also: vec![PathBuf::from("/work/proj/.git")],
        read_only: false,
        login: vec!["~/.claude".into()],
        home: PathBuf::from("/home/me"),
        j4n_home: PathBuf::from("/home/me/.j4n"),
        net: true,
        reads: Vec::new(),
    }
}

fn launch() -> Launch {
    Launch::new(vec!["claude".into(), "--print".into()], PathBuf::from("/home/me/.j4n/worktrees/k/t1"), BTreeMap::new())
}

/// A filesystem in which exactly these paths exist (all of them folders, but the ones named).
fn run(row: &BoxRow, spec: &Spec, exist: &[&str], files: &[&str], launch: Launch) -> Result<Launch, String> {
    let exists = |p: &Path| exist.iter().any(|e| Path::new(e) == p);
    let is_dir = |p: &Path| !files.iter().any(|f| Path::new(f) == p);
    argv(row, spec, &Fs { exists: &exists, is_dir: &is_dir }, launch)
}

const EXIST: &[&str] = &["/", "/home/me/.ssh", "/home/me/.aws", "/home/me/.netrc", "/home/me/.cache", "/home/me/.claude", "/home/me/.j4n", "/home/me/.j4n/worktrees/k/t1", "/work/proj/.git"];

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(ToString::to_string).collect()
}

#[test]
fn the_shipped_wall_is_this_command_line_exactly() {
    let got = run(&shipped(), &spec(), EXIST, &["/home/me/.netrc"], launch()).unwrap();
    let want = strs(&[
        "bwrap", "--die-with-parent", "--new-session", "--unshare-pid", "--unshare-ipc", "--unshare-uts",
        "--ro-bind", "/", "/",
        "--dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp",
        "--tmpfs", "/home/me/.ssh", "--tmpfs", "/home/me/.aws", "--ro-bind", "/dev/null", "/home/me/.netrc", "--tmpfs", "/home/me/.j4n",
        "--bind", "/home/me/.cache", "/home/me/.cache", "--bind", "/home/me/.claude", "/home/me/.claude",
        "--bind", "/home/me/.j4n/worktrees/k/t1", "/home/me/.j4n/worktrees/k/t1", "--bind", "/work/proj/.git", "/work/proj/.git",
        "--chdir", "/home/me/.j4n/worktrees/k/t1",
        "--",
        "claude", "--print",
    ]);
    assert_eq!(got.argv, want);
    assert_eq!(got.cwd, PathBuf::from("/home/me/.j4n/worktrees/k/t1"), "the program still starts where it did");
}

#[test]
fn the_order_is_the_order_that_makes_it_hold() {
    let a = run(&shipped(), &spec(), EXIST, &[], launch()).unwrap().argv;
    let at = |needle: &[&str]| a.windows(needle.len()).position(|w| w == needle).unwrap_or_else(|| panic!("{needle:?} in {a:?}"));
    assert!(at(&["--ro-bind", "/", "/"]) < at(&["--tmpfs", "/tmp"]), "the host first, then a private /tmp");
    assert!(at(&["--tmpfs", "/home/me/.j4n"]) < at(&["--bind", "/home/me/.j4n/worktrees/k/t1", "/home/me/.j4n/worktrees/k/t1"]), "J4n's home is emptied, and the task's folder under it is bound back");
    assert!(at(&["--tmpfs", "/home/me/.ssh"]) < at(&["--bind", "/home/me/.cache", "/home/me/.cache"]), "what is hidden is hidden before anything is made writable");
    assert!(at(&["--bind", "/work/proj/.git", "/work/proj/.git"]) < at(&["--"]), "and everything is before the program");
    assert_eq!(a.iter().filter(|w| *w == "--").count(), 1);
}

#[test]
fn a_path_that_does_not_exist_is_left_out_and_so_is_one_listed_twice() {
    let a = run(&shipped(), &spec(), &["/", "/home/me/.j4n/worktrees/k/t1"], &[], Launch { login: strs(&["~/.claude", "~/.claude"]), ..launch() }).unwrap().argv;
    assert!(!a.iter().any(|w| w.contains(".ssh") || w.contains(".claude") || w.contains(".git")), "{a:?}");
    let mut row = shipped();
    row.read_only.push("/not/there".into());
    let a = run(&row, &spec(), EXIST, &[], launch()).unwrap().argv;
    assert!(!a.iter().any(|w| w == "/not/there"), "a read-only path that does not exist is left out: {a:?}");
    let a = run(&shipped(), &Spec { login: vec![], ..spec() }, EXIST, &[], Launch { login: strs(&["~/.claude", "~/.claude", "~/.cache"]), ..launch() }).unwrap().argv;
    assert_eq!(a.iter().filter(|w| *w == "/home/me/.claude").count(), 2, "one bind has the path twice: the source and the place");
    assert_eq!(a.iter().filter(|w| *w == "/home/me/.cache").count(), 2, "and the cache, named by the row and by the connector, is one bind");
}

#[test]
fn a_connector_names_what_its_program_needs_for_its_login_and_it_is_writable() {
    let a = run(&shipped(), &Spec { login: vec![], ..spec() }, EXIST, &[], Launch { login: strs(&["~/.claude"]), ..launch() }).unwrap().argv;
    assert!(a.windows(3).any(|w| w == ["--bind", "/home/me/.claude", "/home/me/.claude"]), "{a:?}");
    let none = run(&shipped(), &Spec { login: vec![], ..spec() }, EXIST, &[], launch()).unwrap().argv;
    assert!(!none.iter().any(|w| w == "/home/me/.claude"), "a program that names nothing gets nothing of the home");
}

#[test]
fn a_task_that_only_reads_sees_its_folder_and_cannot_write_it() {
    let a = run(&shipped(), &Spec { read_only: true, ..spec() }, EXIST, &[], launch()).unwrap().argv;
    assert!(a.windows(3).any(|w| w == ["--ro-bind", "/home/me/.j4n/worktrees/k/t1", "/home/me/.j4n/worktrees/k/t1"]), "bound back read-only, under the emptied home: {a:?}");
    assert!(!a.windows(3).any(|w| w == ["--bind", "/home/me/.j4n/worktrees/k/t1", "/home/me/.j4n/worktrees/k/t1"]));
    assert!(!a.windows(3).any(|w| w == ["--bind", "/work/proj/.git", "/work/proj/.git"]), "nor its git directory");
    assert!(a.windows(3).any(|w| w == ["--bind", "/home/me/.cache", "/home/me/.cache"]), "its caches and its login are still its own");
}

#[test]
fn the_network_is_cut_only_when_asked_and_only_by_a_wall_that_can() {
    let a = run(&shipped(), &Spec { net: false, ..spec() }, EXIST, &[], launch()).unwrap().argv;
    let (cut, end) = (a.iter().position(|w| w == "--unshare-net").unwrap(), a.iter().position(|w| w == "--").unwrap());
    assert!(cut < end);
    assert!(!run(&shipped(), &spec(), EXIST, &[], launch()).unwrap().argv.iter().any(|w| w == "--unshare-net"));
    let mut row = shipped();
    row.no_net.clear();
    let err = run(&row, &Spec { net: false, ..spec() }, EXIST, &[], launch()).unwrap_err();
    assert!(err.contains("cannot cut the network"), "{err}");
    assert!(run(&row, &spec(), EXIST, &[], launch()).is_ok(), "a wall that cannot cut it is fine for a task that may use it");
    let err = run(&shipped(), &spec(), EXIST, &[], Launch { argv: vec![], ..launch() }).unwrap_err();
    assert!(err.contains("no program"), "{err}");
}

#[test]
fn what_the_wall_shows_is_the_rows_own_data() {
    let row = shipped();
    assert_eq!((row.read_only.clone(), row.id.as_str()), (strs(&["/"]), "bwrap"));
    assert!(row.hide.iter().any(|h| h == "~/.ssh") && row.hide.iter().any(|h| h == "~/.aws") && row.write_home.contains(&"~/.cache".to_string()));
    assert!(!row.write_home.iter().any(|h| h.contains("claude")), "a login is the connector's business, not the wall's");
}

// ---- the place ----

fn here_over(w: &Scratch, h: &Scratch) -> Arc<dyn Place> {
    let _ = w;
    Arc::new(Here::new(h.path(), w.path(), Cfg::default()))
}

fn broken_and_fine(first: &str) -> Vec<BoxRow> {
    let text = format!(
        "api = 1\n[[box]]\nid = \"{first}\"\nprobe = [\"false\"]\npre = [\"false\"]\nro = []\nrw = []\nhide_dir = []\nhide_file = []\nend = [\"--\"]\n\n[[box]]\nid = \"fine\"\nprobe = [\"true\"]\npre = [\"env\", \"MARKER=boxed\"]\nro = []\nrw = []\nhide_dir = []\nhide_file = []\nend = []\n"
    );
    boxed::rows(&tables(&text)).0
}

fn settings(pairs: &[(&'static str, Value)]) -> Settings {
    Settings::from_pairs(pairs.iter().cloned())
}

fn prepare(place: &Boxed, w: &Scratch, task: &str) -> Result<Site, j4n_core::table::Problem> {
    let spec = TaskSpec::new("go", "d");
    place.prepare(&Prepare { task, spec: &spec, root: w.path() })
}

#[test]
fn the_first_wall_that_runs_is_used_and_a_wall_that_cannot_is_skipped() {
    let (w, h) = (Scratch::new("box-p"), Scratch::new("box-h"));
    let place = Boxed::new("box", here_over(&w, &h), broken_and_fine("a-broken"), &settings(&[]), h.path(), h.path());
    let site = prepare(&place, &w, "t1").unwrap();
    assert!(site.notes.iter().any(|n| n.starts_with("behind fine: ")), "{:?}", site.notes);
    let wrapped = place.wrap(&site, Launch::new(vec!["prog".into()], w.path().to_path_buf(), BTreeMap::new()));
    assert_eq!(wrapped.argv[..2], strs(&["env", "MARKER=boxed"]), "{:?}", wrapped.argv);
    assert_eq!(wrapped.argv.last().map(String::as_str), Some("prog"));
}

#[test]
fn a_wall_that_cannot_run_refuses_the_task_with_the_way_out_and_here_runs_it_unboxed() {
    let (w, h) = (Scratch::new("box-p"), Scratch::new("box-h"));
    let only_broken: Vec<BoxRow> = broken_and_fine("a-broken").into_iter().filter(|r| r.id == "a-broken").collect();
    let place = Boxed::new("box", here_over(&w, &h), only_broken.clone(), &settings(&[]), h.path(), h.path());
    let err = prepare(&place, &w, "t1").unwrap_err();
    assert!(err.msg.contains("no box can run here") && err.msg.contains("a-broken: `false` exited 1") && err.msg.contains("`here`"), "{}", err.msg);
    let place = Boxed::new("box", here_over(&w, &h), only_broken, &settings(&[("box.unavailable", Value::text("here"))]), h.path(), h.path());
    let site = prepare(&place, &w, "t1").unwrap();
    assert!(site.notes.iter().any(|n| n.contains("runs unboxed")), "{:?}", site.notes);
    let program = Launch::new(vec!["prog".into()], w.path().to_path_buf(), BTreeMap::new());
    assert_eq!(place.wrap(&site, program.clone()).argv, program.argv, "and the program is not wrapped");
}

#[test]
fn box_use_names_the_wall_and_a_name_that_is_no_wall_is_said() {
    let (w, h) = (Scratch::new("box-p"), Scratch::new("box-h"));
    let named = Boxed::new("box", here_over(&w, &h), broken_and_fine("a-broken"), &settings(&[("box.use", Value::text("a-broken"))]), h.path(), h.path());
    assert!(prepare(&named, &w, "t1").unwrap_err().msg.contains("a-broken: `false` exited 1"), "only the named one is tried");
    let missing = Boxed::new("box", here_over(&w, &h), broken_and_fine("a-broken"), &settings(&[("box.use", Value::text("nope"))]), h.path(), h.path());
    assert!(prepare(&missing, &w, "t1").unwrap_err().msg.contains("`box.use` names `nope`, which is no box"));
    let none = Boxed::new("box", here_over(&w, &h), vec![], &settings(&[]), h.path(), h.path());
    assert!(prepare(&none, &w, "t1").unwrap_err().msg.contains("no box is described"));
}

#[test]
fn a_task_that_may_not_use_the_network_on_a_wall_that_cannot_cut_it_runs_nothing() {
    let (w, h) = (Scratch::new("box-p"), Scratch::new("box-h"));
    let place = Boxed::new("box", here_over(&w, &h), broken_and_fine("a-broken"), &settings(&[("box.net", Value::Bool(false))]), h.path(), h.path());
    let site = prepare(&place, &w, "t1").unwrap();
    let wrapped = place.wrap(&site, Launch::new(vec!["prog".into()], w.path().to_path_buf(), BTreeMap::new()));
    assert_eq!(wrapped.argv[0], "/bin/false", "not run outside the wall: {:?}", wrapped.argv);
    assert!(wrapped.argv[1].contains("cannot cut the network"), "{:?}", wrapped.argv);
}

#[test]
fn a_read_task_is_marked_so_and_the_places_inside_answer_for_the_box() {
    let (w, h) = (Scratch::new("box-p"), Scratch::new("box-h"));
    let place = Boxed::new("box", here_over(&w, &h), broken_and_fine("a-broken"), &settings(&[]), h.path(), h.path());
    let mut spec = TaskSpec::new("go", "d");
    spec.tune.mode = Mode::Read;
    let site = place.prepare(&Prepare { task: "t1", spec: &spec, root: w.path() }).unwrap();
    assert!(site.read_only);
    assert!(place.site_of("t1").is_some_and(|s| s.dir == w.path()), "the box over `here` is where `here` is");
    assert_eq!(place.id(), "box");
    assert!(place.settle(&site).is_empty());
    assert!(place.take("t1", j4n_core::place::Take::Look).is_err(), "`here` keeps no work to take");
}

// ---- the real wall, where there is one ----

fn bwrap_works() -> bool {
    std::process::Command::new("bwrap").args(["--ro-bind", "/", "/", "true"]).output().is_ok_and(|o| o.status.success())
}

#[test]
fn the_real_wall_hides_credentials_stops_writes_outside_and_lets_the_task_and_its_login_write() {
    if !bwrap_works() {
        return;
    }
    let (w, h, outside) = (Scratch::new("real-p"), Scratch::new("real-h"), Scratch::new("real-o"));
    h.put(".ssh/id_key", "SECRET\n");
    h.put(".claude/state", "login\n");
    outside.put("keep", "untouched\n");
    let place = Boxed::new("box", here_over(&w, &h), vec![shipped()], &settings(&[]), h.path(), &h.join(".j4n"));
    let site = prepare(&place, &w, "t1").unwrap();
    let script = format!(
        "cat '{ssh}' 2>/dev/null && echo LEAKED; echo project > '{proj}/made' && echo project-ok; echo login > '{claude}/new' && echo login-ok; echo bad > '{out}/keep' 2>/dev/null && echo OUTSIDE-WRITTEN; true",
        ssh = h.join(".ssh/id_key").display(),
        proj = w.path().display(),
        claude = h.join(".claude").display(),
        out = outside.path().display(),
    );
    let mut launch = Launch::new(vec!["/bin/sh".into(), "-c".into(), script], w.path().to_path_buf(), BTreeMap::new());
    launch.login = vec![format!("{}/.claude", h.path().display())];
    let wrapped = place.wrap(&site, launch);
    let (text, exit) = j4n_connect::proc::capture(&wrapped, std::time::Duration::from_secs(20)).unwrap();
    assert_eq!(exit.code, Some(0), "{text} / {}", exit.tail);
    assert!(text.contains("project-ok") && text.contains("login-ok"), "{text}");
    assert!(!text.contains("LEAKED") && !text.contains("OUTSIDE-WRITTEN"), "{text}");
    assert_eq!(std::fs::read_to_string(w.join("made")).unwrap(), "project\n");
    assert_eq!(std::fs::read_to_string(h.join(".claude/new")).unwrap(), "login\n");
    assert_eq!(std::fs::read_to_string(outside.join("keep")).unwrap(), "untouched\n");
}

#[test]
fn the_real_wall_keeps_a_read_task_from_writing_its_folder() {
    if !bwrap_works() {
        return;
    }
    let (w, h) = (Scratch::new("real-rp"), Scratch::new("real-rh"));
    let place = Boxed::new("box", here_over(&w, &h), vec![shipped()], &settings(&[]), h.path(), &h.join(".j4n"));
    let mut spec = TaskSpec::new("go", "d");
    spec.tune.mode = Mode::Read;
    let site = place.prepare(&Prepare { task: "t1", spec: &spec, root: w.path() }).unwrap();
    let script = format!("echo x > '{}/made' 2>/dev/null && echo WROTE; ls '{}' >/dev/null && echo SAW; true", w.path().display(), w.path().display());
    let wrapped = place.wrap(&site, Launch::new(vec!["/bin/sh".into(), "-c".into(), script], w.path().to_path_buf(), BTreeMap::new()));
    let (text, _) = j4n_connect::proc::capture(&wrapped, std::time::Duration::from_secs(20)).unwrap();
    assert!(text.contains("SAW") && !text.contains("WROTE"), "{text}");
    assert!(!w.join("made").exists());
}

#[test]
fn walls_are_tried_in_the_order_of_their_ids_whatever_the_order_in_the_file() {
    let one = |id: &str| format!("[[box]]\nid = \"{id}\"\nprobe = [\"true\"]\npre = [\"env\", \"WALL={id}\"]\nro = []\nrw = []\nhide_dir = []\nhide_file = []\nend = []\n");
    let (rows, problems) = boxed::rows(&tables(&format!("api = 1\n{}{}", one("z-last"), one("a-first"))));
    assert!(problems.is_empty());
    assert_eq!(rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["a-first", "z-last"]);
    let (w, h) = (Scratch::new("box-ord-p"), Scratch::new("box-ord-h"));
    let place = Boxed::new("box", here_over(&w, &h), rows, &settings(&[]), h.path(), h.path());
    let site = prepare(&place, &w, "t1").unwrap();
    let wrapped = place.wrap(&site, Launch::new(vec!["prog".into()], w.path().to_path_buf(), BTreeMap::new()));
    assert_eq!(wrapped.argv[..2], strs(&["env", "WALL=a-first"]));
}

// ---- a step's `reads` are bound into the wall, and a goal that names what the wall hides is told so ----

#[test]
fn a_path_a_step_reads_is_bound_read_only_over_the_private_tmp_and_before_the_hiding() {
    let exist: Vec<&str> = EXIST.iter().copied().chain(["/tmp/src", "/home/me/.ssh/keys"]).collect();
    let with_reads = Spec { reads: vec![PathBuf::from("/tmp/src"), PathBuf::from("/home/me/.ssh/keys"), PathBuf::from("/tmp/not-there"), PathBuf::from("relative/path")], ..spec() };
    let a = run(&shipped(), &with_reads, &exist, &[], launch()).unwrap().argv;
    let at = |needle: &[&str]| a.windows(needle.len()).position(|w| w == needle).unwrap_or_else(|| panic!("{needle:?} in {a:?}"));
    assert!(at(&["--tmpfs", "/tmp"]) < at(&["--ro-bind", "/tmp/src", "/tmp/src"]), "over the private /tmp, which is made first");
    assert!(!a.iter().any(|w| w.contains("not-there") || w.contains("relative")), "a path that is not there, and one that is not absolute, are left out: {a:?}");
    // a credentials folder stays hidden whatever is listed: the hiding comes after the binding
    assert!(at(&["--ro-bind", "/home/me/.ssh/keys", "/home/me/.ssh/keys"]) < at(&["--tmpfs", "/home/me/.ssh"]), "{a:?}");
    // the same line is the shipped one when nothing is listed
    let plain = run(&shipped(), &spec(), EXIST, &["/home/me/.netrc"], launch()).unwrap().argv;
    assert!(!plain.iter().any(|w| w == "/tmp/src"));
}

fn seen<'a>(own: &[&str], reads: &'a [PathBuf]) -> Seen<'a> {
    Seen { own: own.iter().map(PathBuf::from).collect(), reads }
}

#[test]
fn a_goal_that_names_a_path_under_the_private_tmp_is_told_the_box_hides_it() {
    let row = shipped();
    let all = |_: &Path| true;
    let goal = "Fix the bug in /tmp/proj-src/lib/graph.py (the tests are in /tmp/proj-src/t). Do not touch /etc/hosts or /tmp/other.";
    let said = hidden_in_goal(&row, goal, &seen(&["/home/me/.j4n/worktrees/k/t1"], &[]), &all).expect("a notice");
    assert_eq!(said, "the box hides /tmp/proj-src/lib/graph.py, /tmp/proj-src/t, /tmp/other; list them in the step's `reads` to see them");
    // one path: it
    let one = hidden_in_goal(&row, "see /tmp/x.", &seen(&[], &[]), &all).unwrap();
    assert_eq!(one, "the box hides /tmp/x; list it in the step's `reads` to see it");
    // a path that is listed, or is under one that is, or is in the task's own folder, is shown; one that is not there hides nothing
    let reads = [PathBuf::from("/tmp/proj-src")];
    assert_eq!(hidden_in_goal(&row, "see /tmp/proj-src/lib and /tmp/proj-src", &seen(&[], &reads), &all), None);
    assert_eq!(hidden_in_goal(&row, "work in /tmp/work/a", &seen(&["/tmp/work"], &[]), &all), None);
    assert_eq!(hidden_in_goal(&row, "see /tmp/gone", &seen(&[], &[]), &|_| false), None);
    // credentials and J4n's own home are hidden whatever is listed, so they are not offered; a path with `..` and text that is no path are left alone
    assert_eq!(hidden_in_goal(&row, "read /home/me/.ssh/id_rsa and /tmp/../etc/passwd and a/tmp/x and http://x.test/tmp/y", &seen(&[], &[]), &all), None);
    // at most five
    let many = (0..9).map(|i| format!("/tmp/d{i}")).collect::<Vec<_>>().join(" ");
    let said = hidden_in_goal(&row, &many, &seen(&[], &[]), &all).unwrap();
    assert_eq!(said.matches("/tmp/d").count(), 5, "{said}");
}
