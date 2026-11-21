//! The view place: a secret file reaches a program behind the wall only as its view, the view is mounted last and checked after mounting, and what the program wrote into it is put back, placeholder
//! for value, or refused whole. The ones that need the real wall run where `bwrap` does.

use crate::world::Scratch;
use j4n_connect::place::boxed::{self, argv_with, BoxRow, Fs, Spec};
use j4n_connect::place::view::{self, Mount};
use j4n_connect::place::Boxed;
use j4n_core::place::checkpoint::Cfg;
use j4n_core::place::{Here, Launch, Place, Prepare, Site};
use j4n_core::redact::Redactor;
use j4n_core::setting::Settings;
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use j4n_core::task::{Mode, TaskSpec};
use j4n_core::value::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const SHIPPED: &str = include_str!("../../../../packs/base/boxes/bwrap.toml");
const GUARD_SETTINGS: &str = include_str!("../../../../packs/guard/settings/guard.toml");

const PASSWORD: &str = "Canary9xK2mQ7vL4pT8";
const API_KEY: &str = "sk-proj-Qm7Xk2Lp9Tz4Vb8Nc3Wd6Hy1Rj5Fs0AaBbCc";
const PEM_BODY: &str = "MIIEowIBAAKCAQEAcanarycanarycanary0123456789\nabcdefghijklmnopqrstuvwxyzABCDEFGHIJKL=";
const ENV: &str = "APP_ENV=development\nPORT=8080\nDB_PASSWORD=Canary9xK2mQ7vL4pT8\nOPENAI_API_KEY=sk-proj-Qm7Xk2Lp9Tz4Vb8Nc3Wd6Hy1Rj5Fs0AaBbCc\n";

fn pem() -> String {
    format!("# the key of the test server\n-----BEGIN RSA PRIVATE KEY-----\n{PEM_BODY}\n-----END RSA PRIVATE KEY-----\n")
}

fn tables(text: &str) -> Tables {
    let layer = Layer::new(LayerKind::Pack, "boxes", vec![SourceFile { path: "boxes/b.toml".into(), text: text.into() }]);
    let (tables, problems) = Tables::load(vec![layer]);
    assert!(problems.is_empty(), "{problems:?}");
    tables
}

fn rows(text: &str) -> Vec<BoxRow> {
    let (rows, problems) = boxed::rows(&tables(text));
    assert!(problems.is_empty(), "{problems:?}");
    rows
}

fn shipped() -> BoxRow {
    rows(SHIPPED).remove(0)
}

/// A wall that starts nothing and mounts nothing: its words only show in the command line.
fn fake(views: bool) -> Vec<BoxRow> {
    let view = if views { "view = [\"MOUNT\", \"{src}\", \"{p}\"]\nview_rw = [\"MOUNT-RW\", \"{src}\", \"{p}\"]\n" } else { "" };
    rows(&format!("api = 1\n[[box]]\nid = \"fake\"\nprobe = [\"true\"]\npre = [\"env\", \"MARKER=boxed\"]\nro = []\nrw = []\nhide_dir = []\nhide_file = []\nend = []\n{view}"))
}

fn globs() -> String {
    let line = GUARD_SETTINGS.lines().find(|l| l.starts_with("default = ")).expect("the default of guard.files");
    line.trim_start_matches("default = ").trim_matches('"').to_string()
}

fn settings(extra: &[(&'static str, Value)]) -> Settings {
    let mut pairs = vec![("guard.files", Value::text(globs().as_str()))];
    pairs.extend(extra.iter().cloned());
    Settings::from_pairs(pairs)
}

fn place(w: &Scratch, h: &Scratch, rows: Vec<BoxRow>, red: Option<&Redactor>, extra: &[(&'static str, Value)]) -> Boxed {
    let s = settings(extra);
    let boxed = Boxed::new("box", Arc::new(Here::new(h.path(), w.path(), Cfg::default())), rows, &s, h.path(), &h.join(".j4n"));
    match red {
        Some(r) => boxed.guarded(r, &s),
        None => boxed,
    }
}

fn prepare(p: &Boxed, w: &Scratch, mode: Mode) -> Result<Site, j4n_core::table::Problem> {
    let mut spec = TaskSpec::new("go", "d");
    spec.tune.mode = mode;
    p.prepare(&Prepare { task: "t1", spec: &spec, root: w.path() })
}

fn program(w: &Scratch, words: &[&str]) -> Launch {
    Launch::new(words.iter().map(ToString::to_string).collect(), w.path().to_path_buf(), BTreeMap::new())
}

fn bwrap_works() -> bool {
    std::process::Command::new("bwrap").args(["--ro-bind", "/", "/", "true"]).output().is_ok_and(|o| o.status.success())
}

/// The copies of the views J4n keeps, found by what they hold.
fn copy_of(h: &Scratch, needle: &str) -> PathBuf {
    let mut found = Vec::new();
    for dir in std::fs::read_dir(h.join(".j4n/views")).unwrap().flatten() {
        for f in std::fs::read_dir(dir.path()).unwrap().flatten() {
            if std::fs::read_to_string(f.path()).is_ok_and(|t| t.contains(needle)) {
                found.push(f.path());
            }
        }
    }
    assert_eq!(found.len(), 1, "one copy holds {needle:?}: {found:?}");
    found.remove(0)
}

fn edit(h: &Scratch, needle: &str, f: impl Fn(&str) -> String) {
    let p = copy_of(h, needle);
    let text = std::fs::read_to_string(&p).unwrap();
    std::fs::write(&p, f(&text)).unwrap();
}

fn canaries_in(text: &str) -> Vec<&'static str> {
    [PASSWORD, API_KEY, "canarycanarycanary0123456789", "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKL"].into_iter().filter(|c| text.contains(c)).collect()
}

// ---- a secret file reaches a program only as its view ----

#[test]
fn the_globs_pick_names_the_way_a_shell_would() {
    for (pattern, name, yes) in [(".env", ".env", true), (".env.*", ".env.local", true), (".env.*", ".env", false), ("*.pem", "a.b.pem", true), ("*.pem", "pem", false), ("id_rsa", "id_rsa.pub", false), ("service-account*.json", "service-account-prod.json", true), ("?.key", "a.key", true), ("?.key", "ab.key", false), ("*", "", true)] {
        assert_eq!(view::glob(pattern, name), yes, "{pattern} on {name}");
    }
    let shipped_globs = globs();
    let words: Vec<&str> = shipped_globs.split_whitespace().collect();
    for name in [".env", ".env.production", "server.pem", "id_ed25519", ".npmrc", "terraform.tfvars", "my-credentials.json"] {
        assert!(words.iter().any(|g| view::glob(g, name)), "{name} is picked by the shipped globs");
    }
    for name in ["main.rs", "README.md", "package.json", "Cargo.lock"] {
        assert!(!words.iter().any(|g| view::glob(g, name)), "{name} is not");
    }
}

fn project(w: &Scratch) {
    w.put(".env", ENV);
    w.put("config/.env.local", "SESSION_SECRET=Zk3pQ9wL7vT2xR8mN4bC6yH1\nDEBUG=1\n");
    w.put("server.pem", &pem());
    w.put(".env.example", "OPENAI_API_KEY=your-key-here\nPORT=8080\n");
    w.put("notes.txt", "API_KEY=NotesFileOutsideTheGlobs77\n");
    w.put("data.key", "\0\u{1}binary");
    w.put("node_modules/dep/.env", ENV);
    std::os::unix::fs::symlink(".env", w.join("linked")).unwrap();
    std::fs::hard_link(w.join(".env"), w.join("hard-copy")).unwrap();
}

#[test]
fn the_secret_files_are_found_by_name_and_by_what_is_in_them_and_the_notes_say_where() {
    let (w, h) = (Scratch::new("v82-p"), Scratch::new("v82-h"));
    project(&w);
    let red = Redactor::new();
    let p = place(&w, &h, fake(true), Some(&red), &[]);
    let site = prepare(&p, &w, Mode::Write).unwrap();
    let note = site.notes.iter().find(|n| n.starts_with("guard: ")).unwrap_or_else(|| panic!("{:?}", site.notes));
    assert!(note.contains("3 secret files are shown as their view"), "{note}");
    assert!(note.contains(".env (line 3, 4)") && note.contains("server.pem (line 3)") && note.contains("config/.env.local (line 1)"), "{note}");
    assert!(!note.contains(".env.example") && !note.contains("notes.txt") && !note.contains("node_modules"), "{note}");
    assert!(site.notes.iter().any(|n| n.contains("not covered: data.key (binary")), "{:?}", site.notes);
    // the host's files are as they were: the view is a copy
    assert_eq!(w.read(".env"), ENV);
}

#[test]
fn a_task_that_only_reads_gets_a_read_only_view_and_one_that_writes_a_writable_one() {
    if !bwrap_works() {
        return;
    }
    let (w, h) = (Scratch::new("v82r-p"), Scratch::new("v82r-h"));
    w.put(".env", ENV);
    let p = place(&w, &h, vec![shipped()], Some(&Redactor::new()), &[]);
    let at = w.join(".env").display().to_string();
    for (mode, flag, other) in [(Mode::Read, "--ro-bind", "--bind"), (Mode::Write, "--bind", "--ro-bind")] {
        let site = prepare(&p, &w, mode).unwrap();
        let a = p.wrap(&site, program(&w, &["prog"])).argv;
        let view = |f: &str| a.windows(3).any(|x| x[0] == f && x[1].contains("/views/t1-") && x[2] == at);
        assert!(view(flag) && !view(other), "{mode:?}: {a:?}");
    }
}

#[test]
fn with_the_pack_off_nothing_is_scanned_mounted_or_said() {
    let (w, h) = (Scratch::new("v82o-p"), Scratch::new("v82o-h"));
    project(&w);
    let p = place(&w, &h, fake(true), None, &[]);
    let site = prepare(&p, &w, Mode::Write).unwrap();
    assert!(!site.notes.iter().any(|n| n.contains("guard")), "{:?}", site.notes);
    let wrapped = p.wrap(&site, program(&w, &["prog"]));
    assert_eq!(wrapped.argv, ["env", "MARKER=boxed", "prog"], "today's command line");
    assert!(!h.join(".j4n/views").exists());
    assert!(p.settle(&site).notes.iter().all(|n| !n.contains("guard")));
}

#[test]
fn a_wall_with_no_way_to_mount_a_view_refuses_a_task_that_has_a_secret_file_and_runs_one_that_has_none() {
    let (w, h) = (Scratch::new("v82n-p"), Scratch::new("v82n-h"));
    let p = place(&w, &h, fake(false), Some(&Redactor::new()), &[]);
    w.put(".env.example", "OPENAI_API_KEY=your-key-here\n");
    assert!(prepare(&p, &w, Mode::Write).is_ok(), "no secret in any file: nothing to cover");
    w.put(".env", ENV);
    let err = prepare(&p, &w, Mode::Write).unwrap_err();
    assert!(err.msg.contains("has no way to mount a view") && err.msg.contains("turn the pack `guard` off"), "{}", err.msg);
}

#[test]
fn a_box_that_cannot_run_and_is_let_run_unboxed_says_the_secret_files_are_not_covered() {
    let (w, h) = (Scratch::new("v82u-p"), Scratch::new("v82u-h"));
    let broken = rows("api = 1\n[[box]]\nid = \"b\"\nprobe = [\"false\"]\npre = [\"false\"]\nro = []\nrw = []\nhide_dir = []\nhide_file = []\nend = []\n");
    w.put(".env", ENV);
    let extra = [("box.unavailable", Value::text("here"))];
    let site = prepare(&place(&w, &h, broken.clone(), Some(&Redactor::new()), &extra), &w, Mode::Write).unwrap();
    assert!(site.notes.iter().any(|n| n.contains("runs unboxed and its secret files are not covered")), "{:?}", site.notes);
    let site = prepare(&place(&w, &h, broken, None, &extra), &w, Mode::Write).unwrap();
    assert!(site.notes.iter().any(|n| n.ends_with("runs unboxed")), "the pack off says what it always said: {:?}", site.notes);
}

#[test]
fn the_real_wall_shows_a_secret_file_to_every_reader_only_as_its_view() {
    if !bwrap_works() {
        return;
    }
    let (w, h, outside) = (Scratch::new("v82w-p"), Scratch::new("v82w-h"), Scratch::new("v82w-o"));
    project(&w);
    // a name the globs pick that is a link to a secret file outside the folder: the file is covered where it is
    outside.put(".env", "STRIPE_SECRET_KEY=Zk3pQ9wL7vT2xR8mN4bC6yH1OUTSIDE\n");
    std::os::unix::fs::symlink(outside.join(".env"), w.join("prod.env")).unwrap();
    let p = place(&w, &h, vec![shipped()], Some(&Redactor::new()), &[]);
    let site = prepare(&p, &w, Mode::Write).unwrap();
    let wrapped = p.wrap(&site, program(&w, &["prog"]));
    let views: Vec<&[String]> = wrapped.argv.windows(3).filter(|x| x[0] == "--bind" && x[1].contains("/views/t1-")).collect();
    assert_eq!(views.len(), 5, "{:?}: three files, the second name of the first (a hard link) and the file outside that a link points to", wrapped.argv);
    assert!(views.iter().any(|x| x[2].ends_with("/hard-copy")) && !views.iter().any(|x| x[2].ends_with("/linked")), "a symlink is covered by its target: a mount on a symlink is refused by the wall");
    let script = "for f in .env config/.env.local server.pem linked hard-copy prod.env; do echo \"== $f\"; cat \"$f\"; done; echo '== grep'; grep -r -e Canary -e Qm7X -e canarycanary -e Zk3pQ9 . ; echo '== cp'; cp .env /tmp/o && cat /tmp/o; echo '== py'; command -v python3 >/dev/null && python3 -c \"print(open('.env').read())\"; echo '== example'; cat .env.example; echo '== notes'; cat notes.txt; true";
    let (text, exit) = j4n_connect::proc::capture(&p.wrap(&site, program(&w, &["/bin/sh", "-c", script])), std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(exit.code, Some(0), "{text} / {}", exit.tail);
    let (secret, rest) = text.split_once("== notes").expect("the script ran to its end");
    // the known limit: the search does not enter `node_modules`, `target` or `.git`, so a file there is as it is
    assert!(secret.contains("./node_modules/dep/.env:DB_PASSWORD=Canary9xK2mQ7vL4pT8"), "{secret}");
    let secret = secret.lines().filter(|l| !l.contains("node_modules/")).collect::<Vec<_>>().join("\n");
    let secret = secret.as_str();
    assert_eq!(canaries_in(secret), Vec::<&str>::new(), "no reader got a secret:\n{secret}");
    assert!(!secret.contains("Zk3pQ9wL7vT2xR8mN4bC6yH1"), "{secret}");
    assert!(secret.contains("== prod.env\nSTRIPE_SECRET_KEY=«REDACTED"), "a link out of the folder gets the view of its target:\n{secret}");
    assert!(secret.contains("«REDACTED openai_key starts=sk-proj-") && secret.contains("«REDACTED private_key") && secret.contains("PORT=8080"), "the view keeps the schema and the rest of the file:\n{secret}");
    assert!(secret.contains("# the key of the test server") && secret.contains("-----BEGIN RSA PRIVATE KEY-----"), "{secret}");
    assert!(secret.matches("DB_PASSWORD=«REDACTED password length=19").count() >= 4, "the same view by every route (.env, its two other names, a copy, python)\n{secret}");
    assert!(secret.contains("OPENAI_API_KEY=your-key-here"), "a file with no secret is as it was:\n{secret}");
    assert!(rest.contains("NotesFileOutsideTheGlobs77"), "a name outside the globs is not touched: {rest}");
    assert_eq!(w.read(".env"), ENV, "and the host's file is untouched");
}

// ---- mounted last, checked after mounting ----

fn mount(src: &str, at: &str, writable: bool) -> Mount {
    Mount { src: PathBuf::from(src), at: PathBuf::from(at), writable, view: String::new() }
}

fn spec() -> Spec {
    Spec { dir: PathBuf::from("/work/p"), also: vec![PathBuf::from("/work/p/.git")], read_only: false, login: vec!["~/.claude".into()], home: PathBuf::from("/home/me"), j4n_home: PathBuf::from("/home/me/.j4n"), net: true, reads: Vec::new() }
}

fn exists(p: &Path) -> bool {
    ["/", "/home/me/.claude", "/home/me/.j4n", "/work/p", "/work/p/.git", "/home/me/.ssh"].iter().any(|e| Path::new(e) == p)
}

fn claude() -> Launch {
    Launch::new(vec!["claude".into()], PathBuf::from("/work/p"), BTreeMap::new())
}

fn run(row: &BoxRow, spec: &Spec, views: &[Mount]) -> Result<Vec<String>, String> {
    argv_with(row, spec, &Fs { exists: &exists, is_dir: &|_| true }, claude(), views).map(|l| l.argv)
}

#[test]
fn the_views_come_last_after_every_bind_that_could_cover_them_and_before_the_program() {
    let views = [mount("/home/me/.j4n/views/t1-a/0", "/work/p/.env", true), mount("/home/me/.j4n/views/t1-a/1", "/work/p/k.pem", false)];
    let got = run(&shipped(), &spec(), &views).unwrap();
    let want: Vec<String> = [
        "bwrap", "--die-with-parent", "--new-session", "--unshare-pid", "--unshare-ipc", "--unshare-uts", "--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp", "--tmpfs", "/home/me/.ssh", "--tmpfs", "/home/me/.j4n", "--bind", "/home/me/.claude", "/home/me/.claude", "--bind", "/work/p", "/work/p", "--bind", "/work/p/.git", "/work/p/.git", "--chdir", "/work/p",
        "--bind", "/home/me/.j4n/views/t1-a/0", "/work/p/.env", "--ro-bind", "/home/me/.j4n/views/t1-a/1", "/work/p/k.pem", "--", "claude",
    ].iter().map(ToString::to_string).collect();
    assert_eq!(got, want);
    let cut = run(&shipped(), &Spec { net: false, ..spec() }, &views).unwrap();
    let at = |w: &str| cut.iter().position(|a| a == w).unwrap();
    assert!(cut.iter().rposition(|a| a == "/work/p/k.pem").unwrap() < at("--unshare-net") && at("--unshare-net") < at("--"), "{cut:?}");
    assert_eq!(run(&shipped(), &spec(), &[]).unwrap(), boxed::argv(&shipped(), &spec(), &Fs { exists: &exists, is_dir: &|_| true }, claude()).unwrap().argv, "with no view the line is the one of before");
}

#[test]
fn a_wall_that_has_no_words_for_a_view_cannot_mount_one() {
    let mut row = shipped();
    row.view_rw.clear();
    let err = run(&row, &spec(), &[mount("/s", "/work/p/.env", true)]).unwrap_err();
    assert!(err.contains("bwrap has no way to mount a view of /work/p/.env"), "{err}");
    assert!(run(&row, &spec(), &[mount("/s", "/work/p/.env", false)]).is_ok(), "the read-only words are another list");
}

#[test]
fn a_wall_that_will_not_run_for_the_check_is_the_reason_not_to_start() {
    let m = Mount { view: "x".into(), ..mount("/s", "/work/p/.env", false) };
    let err = view::verify(&["/no/such/wall".to_string()], &[m]).unwrap_err();
    assert!(err.contains("the wall cannot be asked"), "{err}");
}

#[test]
fn a_login_folder_that_holds_the_project_does_not_cover_the_view() {
    if !bwrap_works() {
        return;
    }
    let (w, h) = (Scratch::new("v83-p"), Scratch::new("v83-h"));
    w.put(".env", ENV);
    let p = place(&w, &h, vec![shipped()], Some(&Redactor::new()), &[]);
    let site = prepare(&p, &w, Mode::Write).unwrap();
    // the connector's login names the folder the project is in: a bind made later than the view would cover it
    let mut launch = program(&w, &["/bin/sh", "-c", "cat .env"]);
    launch.login = vec![w.path().display().to_string()];
    let wrapped = p.wrap(&site, launch);
    assert_ne!(wrapped.argv[0], "/bin/false", "{:?}", wrapped.argv);
    let (text, _) = j4n_connect::proc::capture(&wrapped, std::time::Duration::from_secs(30)).unwrap();
    assert!(text.contains("DB_PASSWORD=«REDACTED") && canaries_in(&text).is_empty(), "{text}");
}

#[test]
fn a_view_that_does_not_take_is_found_after_mounting_and_the_program_is_not_started() {
    if !bwrap_works() {
        return;
    }
    let (w, h) = (Scratch::new("v83b-p"), Scratch::new("v83b-h"));
    w.put(".env", ENV);
    // a wall whose words for a view mount the real file over itself: the mount "works" and shows the secret
    let mut row = shipped();
    row.view = vec!["--ro-bind".into(), "{p}".into(), "{p}".into()];
    row.view_rw = row.view.clone();
    let p = place(&w, &h, vec![row], Some(&Redactor::new()), &[]);
    let site = prepare(&p, &w, Mode::Write).unwrap();
    for _ in 0..2 {
        let wrapped = p.wrap(&site, program(&w, &["/bin/sh", "-c", "cat .env"]));
        assert_eq!(wrapped.argv[0], "/bin/false", "not run behind a wall that shows the secret: {:?}", wrapped.argv);
        assert!(wrapped.argv[1].contains("a reader inside the wall does not get the view of") && wrapped.argv[1].ends_with(".env"), "{:?}", wrapped.argv);
    }
}

// ---- writing back ----

/// A project with the files of the write-back cases, and the place over it (a wall that starts nothing: the program is played
/// by the test, which edits the copy of the view as the program would through a writable mount).
fn rig(tag: &str, red: &Redactor) -> (Scratch, Scratch, Boxed, Site) {
    let (w, h) = (Scratch::new(&format!("v84{tag}-p")), Scratch::new(&format!("v84{tag}-h")));
    w.put(".env", ENV);
    let p = place(&w, &h, fake(true), Some(red), &[]);
    let site = prepare(&p, &w, Mode::Write).unwrap();
    (w, h, p, site)
}

#[test]
fn what_the_program_wrote_elsewhere_stays_and_the_values_come_back() {
    let (w, h, p, site) = rig("a", &Redactor::new());
    edit(&h, "PORT=", |t| t.replace("PORT=8080", "PORT=9090\nNEW_FLAG=1") + "# added by the agent\n");
    let settled = p.settle(&site);
    assert_eq!(w.read(".env"), ENV.replace("PORT=8080", "PORT=9090\nNEW_FLAG=1") + "# added by the agent\n", "the edit stands and every value is the real one again");
    assert!(settled.notes.iter().any(|n| n.contains(".env was edited, and its values are back in it")), "{:?}", settled.notes);
    assert!(settled.modified.iter().any(|m| m.ends_with(".env")), "the place sees the real file written, because the write-back is before it settles: {settled:?}");
    assert!(!h.join(".j4n/views").read_dir().is_ok_and(|mut d| d.next().is_some()), "the copies are let go");
}

#[test]
fn a_placeholder_that_was_changed_removed_added_or_moved_refuses_the_whole_write() {
    let lines = |t: &str| t.lines().map(String::from).collect::<Vec<_>>();
    type Edit = Box<dyn Fn(&str) -> String>;
    let cases: Vec<(&str, Edit)> = vec![
        ("changed", Box::new(|t| t.replace("length=19", "length=20"))),
        ("removed", Box::new(move |t| lines(t).into_iter().filter(|l| !l.starts_with("DB_PASSWORD")).collect::<Vec<_>>().join("\n") + "\n")),
        ("added", Box::new(move |t| format!("{t}COPY={}\n", lines(t)[2].split_once('=').unwrap().1))),
        ("moved", Box::new(move |t| { let mut l = lines(t); l.swap(2, 3); l.join("\n") + "\n" })),
        ("typed", Box::new(|t| t.replace("«REDACTED password", "«REDACTED passwort"))),
    ];
    for (what, f) in cases {
        let (w, h, p, site) = rig(what, &Redactor::new());
        edit(&h, "PORT=", |t| f(&t.replace("PORT=8080", "PORT=9090")));
        let settled = p.settle(&site);
        assert_eq!(w.read(".env"), ENV, "{what}: the file is as it was, and the edit of another line is not applied either");
        assert!(settled.notes.iter().any(|n| n.contains("the edits to .env were not applied") && n.ends_with("the file is as it was")), "{what}: {:?}", settled.notes);
    }
}

#[test]
fn a_file_changed_on_the_host_meanwhile_is_not_written_over() {
    let (w, h, p, site) = rig("c", &Redactor::new());
    edit(&h, "PORT=", |t| t.replace("PORT=8080", "PORT=9090"));
    w.put(".env", &format!("{ENV}FROM_THE_OPERATOR=1\n"));
    let settled = p.settle(&site);
    assert_eq!(w.read(".env"), format!("{ENV}FROM_THE_OPERATOR=1\n"));
    assert!(settled.notes.iter().any(|n| n.contains("it was changed on the host meanwhile")), "{:?}", settled.notes);
}

#[test]
fn a_private_key_and_a_registered_secret_round_trip() {
    let red = Redactor::new();
    assert!(red.register("MYTOKEN", "RegisteredValue12345"));
    let (w, h) = (Scratch::new("v84d-p"), Scratch::new("v84d-h"));
    let (env, key) = ("SOME_SETTING=RegisteredValue12345\nPORT=8080\n".to_string(), pem());
    w.put(".env", &env);
    w.put("server.pem", &key);
    let p = place(&w, &h, fake(true), Some(&red), &[]);
    let site = prepare(&p, &w, Mode::Write).unwrap();
    assert!(std::fs::read_to_string(copy_of(&h, "PORT=")).unwrap().contains("«secret:MYTOKEN»"));
    assert!(std::fs::read_to_string(copy_of(&h, "the key of the test server")).unwrap().contains(" (2 lines)"));
    edit(&h, "PORT=", |t| t.replace("PORT=8080", "PORT=1"));
    edit(&h, "the key of the test server", |t| t.replace("# the key of the test server", "# the key of the staging server"));
    let settled = p.settle(&site);
    assert_eq!(w.read(".env"), env.replace("PORT=8080", "PORT=1"), "{:?}", settled.notes);
    assert_eq!(w.read("server.pem"), key.replace("test", "staging"), "{:?}", settled.notes);
}

#[test]
fn a_task_that_only_reads_writes_nothing_back_and_a_task_that_wrote_nothing_changes_nothing() {
    let (w, h) = (Scratch::new("v84e-p"), Scratch::new("v84e-h"));
    w.put(".env", ENV);
    let p = place(&w, &h, fake(true), Some(&Redactor::new()), &[]);
    let site = prepare(&p, &w, Mode::Read).unwrap();
    edit(&h, "PORT=", |t| t.replace("PORT=8080", "PORT=9090"));
    p.settle(&site);
    assert_eq!(w.read(".env"), ENV, "the copy of a task that only reads was never going to be merged");
    let site = prepare(&p, &w, Mode::Write).unwrap();
    let settled = p.settle(&site);
    assert_eq!(w.read(".env"), ENV);
    assert!(settled.notes.iter().all(|n| !n.contains(".env")), "{:?}", settled.notes);
}

#[test]
fn the_real_wall_takes_a_write_into_the_view_and_the_values_are_back_at_the_end() {
    if !bwrap_works() {
        return;
    }
    let (w, h) = (Scratch::new("v84w-p"), Scratch::new("v84w-h"));
    w.put(".env", ENV);
    let p = place(&w, &h, vec![shipped()], Some(&Redactor::new()), &[]);
    let site = prepare(&p, &w, Mode::Write).unwrap();
    let script = "echo 'NEW_FLAG=1' >> .env && echo appended; sed -i 's/PORT=8080/PORT=1/' .env 2>/dev/null || echo sed-refused; cat .env";
    let (text, exit) = j4n_connect::proc::capture(&p.wrap(&site, program(&w, &["/bin/sh", "-c", script])), std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(exit.code, Some(0), "{text} / {}", exit.tail);
    assert!(text.contains("appended") && text.contains("sed-refused") && canaries_in(&text).is_empty(), "{text}");
    assert_eq!(w.read(".env"), ENV, "nothing reaches the real file before the end");
    p.settle(&site);
    assert_eq!(w.read(".env"), format!("{ENV}NEW_FLAG=1\n"));
}
