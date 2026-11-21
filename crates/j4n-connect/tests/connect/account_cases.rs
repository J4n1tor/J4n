//! Accounts (`account.rs`): the rows, the environment of an account, its folder and its shims, what a task that runs as one is
//! launched as, and the window's command line. The doer that signs in is the binary's `accounts_flow` law.

use crate::world::Scratch;
use j4n_connect::account::{Account, Book};
use j4n_connect::manifest::Connectors;
use j4n_core::place::Launch;
use j4n_core::table::{Kind, KindDecl, Layer, LayerKind, SourceFile, Tables};
use j4n_core::task::TaskSpec;
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn pack(dir: &str, name: &str) -> Layer {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs").join(dir);
    let (mut layer, read) = Layer::from_dir(LayerKind::Pack, name, &path);
    assert!(read.is_empty(), "{read:?}");
    layer.files.retain(|f| !(dir == "base" && f.path.starts_with("kinds/")) && f.path != "pack.toml");
    layer
}

fn mine(files: &[(&str, &str)]) -> Layer {
    Layer::new(LayerKind::User, "mine", files.iter().map(|(p, t)| SourceFile { path: (*p).into(), text: format!("api = 1\n{t}") }).collect())
}

struct Rig {
    book: std::sync::Arc<Book>,
    root: PathBuf,
    home: PathBuf,
    said: Vec<String>,
    _dirs: (Scratch, Scratch),
}

fn rig(rows: &str, extra: &str) -> Rig {
    rig_with(rows, extra, None)
}

fn rig_with(rows: &str, extra: &str, system: Option<&str>) -> Rig {
    let (accounts, user) = (Scratch::new("accounts"), Scratch::new("user-home"));
    let (tables, problems) = Tables::load(vec![pack("base", "base-data"), pack("accounts", "accounts-data"), mine(&[("account/rows.toml", rows), ("connectors/extra.toml", extra)])]);
    assert!(problems.is_empty(), "{problems:?}");
    let (connectors, more) = Connectors::load(&tables);
    assert!(more.is_empty(), "{more:?}");
    let (book, found) = Book::load(&tables, &connectors, accounts.path(), user.path());
    let (root, home) = (accounts.path().to_path_buf(), user.path().to_path_buf());
    Rig { book: std::sync::Arc::new(book.with_system_proxy(system)), root, home, said: found.iter().map(ToString::to_string).collect(), _dirs: (accounts, user) }
}

/// A browser that exists, so that the account can have a window.
fn browser(dir: &Path) -> String {
    let p = dir.join("chromium");
    std::fs::write(&p, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p.display().to_string()
}

const PLAIN: &str = "[[connector]]\nid = \"plain\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"/bin/echo\"\nstart = [\"{prompt}\"]\nlogin = [\"~/.plain\"]\n";
const LAB: &str = "[[connector]]\nid = \"lab\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"/bin/echo\"\nstart = [\"{prompt}\"]\nlogin = [\"~/.lab\"]\n[connector.signin]\nargv = [\"/bin/true\"]\nenv = { LAB_HOME = \"{account.home}\", LAB_DIR = \"{account.dir}\" }\nshare = [\"settings.json\", \".credentials.json\", \"auth.json\", \"a/b\", \"..\"]\n";

fn browser_row(program: &str) -> String {
    format!("[[connector]]\nid = \"browser\"\nis = \"service\"\ndialect = \"text\"\nprogram = \"{program}\"\ncandidates = []\nprobe = {{ argv = [\"{program}\", \"--version\"] }}\nstart = [\"--user-data-dir={{account.browser}}\", \"--proxy-server={{account.proxy}}\", \"--app={{url}}\"]\n")
}

fn account(book: &Rig, id: &str) -> Account {
    let _ = book;
    Account { id: id.into(), doc: String::new(), connector: "lab".into(), remark: String::new(), proxy: "system".into(), view: "window".into(), keeps: "home".into(), plan: None }
}

#[test]
fn the_kind_declares_an_example_the_struct_reads_and_rows_that_do_not_fit_are_reported_by_name() {
    let r = rig("[[account]]\nid = \"good\"\nconnector = \"lab\"\n\n[[account]]\nid = \"fine-2_x\"\nconnector = \"lab\"\nproxy = \"socks5://127.0.0.1:1080\"\n", LAB);
    assert!(r.said.is_empty(), "{:?}", r.said);
    assert!(!r.book.is_empty());
    // the id is a file name: it may not climb out of the folder
    let r = rig("[[account]]\nid = \"Bad\"\nconnector = \"lab\"\n\n[[account]]\nid = \"far\"\nconnector = \"lab\"\nproxy = \"ftp://x\"\n", LAB);
    assert!(r.book.is_empty(), "{:?}", r.said);
    assert!(r.said.iter().any(|m| m.contains("lower case letters")) && r.said.iter().any(|m| m.contains("neither `system` nor a proxy URL")), "{:?}", r.said);
    // the example of the kind is a row the struct reads
    let (tables, problems) = Tables::load(vec![pack("base", "base-data"), pack("accounts", "accounts-data")]);
    assert!(problems.is_empty(), "{problems:?}");
    let decl: KindDecl = tables.get(Account::NAME).unwrap();
    assert_eq!(decl.examples.len(), 1);
    let mut doc = toml::Table::new();
    doc.insert("account".into(), toml::Value::Array(decl.examples.clone()));
    let (tables, problems) = Tables::load(vec![pack("base", "base-data"), pack("accounts", "accounts-data"), mine(&[("account/x.toml", &toml::to_string(&doc).unwrap())])]);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(tables.ids(Account::NAME).len(), 1);
}

#[test]
fn a_connector_without_a_sign_in_gets_a_home_of_the_accounts_own_and_one_with_it_gets_what_it_says() {
    let r = rig("[[account]]\nid = \"a\"\nconnector = \"plain\"\n\n[[account]]\nid = \"b\"\nconnector = \"lab\"\n", &format!("{PLAIN}{LAB}"));
    let run = |id: &str| {
        let mut spec = TaskSpec::new("go", if id == "a" { "plain" } else { "lab" });
        spec.account = Some(id.into());
        r.book.launcher()(&spec, Launch::new(vec!["x".into()], r.root.clone(), BTreeMap::new()))
    };
    // the account has no folder yet: not a launch as anyone else
    assert!(run("a").unwrap_err().contains("has no folder yet"), "nothing was made");
    for id in ["a", "b"] {
        std::fs::create_dir_all(r.root.join(id).join("home")).unwrap();
    }
    let a = run("a").unwrap();
    let home = r.root.join("a/home").display().to_string();
    assert_eq!(a.env["HOME"], home);
    assert_eq!(a.env["XDG_CONFIG_HOME"], format!("{home}/.config"));
    assert_eq!(a.env["XDG_DATA_HOME"], format!("{home}/.local/share"));
    assert_eq!(a.login, vec![home.clone()], "the walls leave open this account's folder and no login of the operator's");
    assert_eq!(a.env["no_proxy"], "localhost,127.0.0.1,::1");
    assert!(!a.env.contains_key("https_proxy"), "an account with no proxy of its own is told none");
    let b = run("b").unwrap();
    assert_eq!(b.env["LAB_HOME"], r.root.join("b/home").display().to_string());
    assert_eq!(b.env["LAB_DIR"], r.root.join("b").display().to_string());
    assert!(!b.env.contains_key("HOME"), "the connector said what its env is");
}

#[test]
fn a_task_runs_as_an_account_of_its_own_harness_only_and_an_unknown_account_is_refused() {
    let r = rig("[[account]]\nid = \"a\"\nconnector = \"lab\"\n", &format!("{PLAIN}{LAB}"));
    std::fs::create_dir_all(r.root.join("a/home")).unwrap();
    let launch = |doer: &str, account: &str| {
        let mut spec = TaskSpec::new("go", doer);
        spec.account = Some(account.into());
        r.book.launcher()(&spec, Launch::new(vec!["x".into()], r.root.clone(), BTreeMap::new()))
    };
    assert!(launch("lab", "a").is_ok());
    assert!(launch("plain", "a").unwrap_err().contains("a login of `lab`, and this task runs `plain`"));
    assert!(launch("lab", "zz").unwrap_err().contains("no account called `zz`"));
}

#[test]
fn an_account_with_a_proxy_of_its_own_tells_every_variable_a_program_reads_and_the_system_one_is_left_to_the_process() {
    let r = rig("[[account]]\nid = \"a\"\nconnector = \"lab\"\nproxy = \"http://user:secret@127.0.0.1:3128\"\n\n[[account]]\nid = \"b\"\nconnector = \"lab\"\n", LAB);
    for id in ["a", "b"] {
        std::fs::create_dir_all(r.root.join(id).join("home")).unwrap();
    }
    let env = |id: &str| {
        let mut spec = TaskSpec::new("go", "lab");
        spec.account = Some(id.into());
        r.book.launcher()(&spec, Launch::new(vec!["x".into()], r.root.clone(), BTreeMap::new())).map(|l| l.env)
    };
    // the proxy does not answer here, so the launch says so rather than start a child that would go round it
    let why = env("a").unwrap_err();
    assert!(why.contains("does not accept a connection") && why.contains("127.0.0.1:3128") && !why.contains("secret"), "{why}");
    let b = env("b").unwrap();
    for k in ["https_proxy", "HTTPS_PROXY", "http_proxy", "HTTP_PROXY", "ALL_PROXY", "all_proxy"] {
        assert!(!b.contains_key(k), "{k}");
    }
}

#[test]
fn the_folder_is_private_and_holds_two_shims_that_open_a_link_in_the_accounts_window_and_no_other() {
    let bin = Scratch::new("browser");
    let program = browser(bin.path());
    let r = rig("[[account]]\nid = \"a\"\nconnector = \"lab\"\nproxy = \"socks5://127.0.0.1:1080\"\n", &format!("{LAB}{}", browser_row(&program)));
    let (a, lab) = (account(&r, "a"), crate_connector(&r));
    let a = Account { proxy: "socks5://127.0.0.1:1080".into(), ..a };
    r.book.ensure(&a, &lab).unwrap();
    let dir = r.root.join("a");
    assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
    for sub in ["browser", "home", "bin"] {
        assert_eq!(std::fs::metadata(dir.join(sub)).unwrap().permissions().mode() & 0o777, 0o700, "{sub}");
    }
    let shim = std::fs::read_to_string(dir.join("bin/open")).unwrap();
    assert_eq!(shim, std::fs::read_to_string(dir.join("bin/xdg-open")).unwrap());
    assert!(shim.starts_with("#!/bin/sh\n") && shim.contains("exec "), "{shim}");
    // the link is the shim's first argument, the data dir and the proxy are the account's, each word quoted
    assert!(shim.contains(&format!("'--user-data-dir={}'", dir.join("browser").display())), "{shim}");
    assert!(shim.contains("'--proxy-server=socks5://127.0.0.1:1080'"), "{shim}");
    assert!(shim.contains("'--app='\"${1}\"''"), "{shim}");
    assert_eq!(std::fs::metadata(dir.join("bin/open")).unwrap().permissions().mode() & 0o777, 0o700);
    // a second call keeps what is there and mends what was changed
    std::fs::write(dir.join("bin/open"), "tampered").unwrap();
    r.book.ensure(&a, &lab).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("bin/open")).unwrap(), shim);
}

fn crate_connector(r: &Rig) -> j4n_connect::manifest::Connector {
    let _ = r;
    let (tables, _) = Tables::load(vec![pack("base", "base-data"), pack("accounts", "accounts-data"), mine(&[("connectors/extra.toml", LAB)])]);
    Connectors::load(&tables).0.get("lab").unwrap().clone()
}

#[test]
fn a_window_without_a_proxy_has_no_proxy_flag_and_with_no_browser_there_is_no_window_and_it_says_so() {
    let bin = Scratch::new("browser");
    let program = browser(bin.path());
    let r = rig("[[account]]\nid = \"a\"\nconnector = \"lab\"\n", &format!("{LAB}{}", browser_row(&program)));
    let (a, lab) = (account(&r, "a"), crate_connector(&r));
    r.book.ensure(&a, &lab).unwrap();
    let shim = std::fs::read_to_string(r.root.join("a/bin/open")).unwrap();
    assert!(!shim.contains("--proxy-server"), "{shim}");
    // the process's own proxy stands for `system`
    let with = rig_with("[[account]]\nid = \"a\"\nconnector = \"lab\"\n", &format!("{LAB}{}", browser_row(&program)), Some("http://127.0.0.1:2080"));
    with.book.ensure(&a, &lab).unwrap();
    assert!(std::fs::read_to_string(with.root.join("a/bin/open")).unwrap().contains("'--proxy-server=http://127.0.0.1:2080'"));

    let missing = rig("[[account]]\nid = \"a\"\nconnector = \"lab\"\n", &format!("{LAB}{}", browser_row("/nowhere/chromium")));
    let err = missing.book.ensure(&account(&missing, "a"), &lab).unwrap_err();
    assert!(err.contains("no browser for account windows") && err.contains("/nowhere/chromium"), "{err}");
}

#[test]
fn only_what_a_connector_shares_and_is_not_a_login_is_linked_in_from_the_operators_own_folder() {
    let bin = Scratch::new("browser");
    let program = browser(bin.path());
    let r = rig("[[account]]\nid = \"a\"\nconnector = \"lab\"\n", &format!("{LAB}{}", browser_row(&program)));
    let own = r.home.join(".lab");
    std::fs::create_dir_all(&own).unwrap();
    for f in ["settings.json", ".credentials.json", "auth.json"] {
        std::fs::write(own.join(f), "x").unwrap();
    }
    // a name with a slash is a path, not a name: it is not followed even where the folder it names is there
    std::fs::create_dir_all(own.join("a")).unwrap();
    std::fs::write(own.join("a/b"), "x").unwrap();
    std::fs::create_dir_all(r.root.join("a/home/a")).unwrap();
    let (a, lab) = (account(&r, "a"), crate_connector(&r));
    r.book.ensure(&a, &lab).unwrap();
    let home = r.root.join("a/home");
    assert!(!home.join("a/b").exists(), "a/b is not linked");
    assert!(home.join("settings.json").is_symlink(), "settings stay one");
    assert!(!home.join(".credentials.json").exists() && !home.join("auth.json").exists(), "a login is never linked, whatever is listed");
    assert!(!home.join("..").join("b").exists());
    // what the account already has is not replaced
    std::fs::remove_file(home.join("settings.json")).unwrap();
    std::fs::write(home.join("settings.json"), "mine").unwrap();
    r.book.ensure(&a, &lab).unwrap();
    assert_eq!(std::fs::read_to_string(home.join("settings.json")).unwrap(), "mine");
}

/// The sign-in window is a normal window of the account's profile (`--new-window`, tabs and an address bar), not a bare `--app` window, and
/// it keeps the account's data dir and proxy; the link is its last word.
#[test]
fn the_shipped_browser_starts_a_normal_window_with_the_accounts_data_dir_and_proxy() {
    let (tables, problems) = Tables::load(vec![pack("base", "base-data"), pack("accounts", "accounts-data")]);
    assert!(problems.is_empty(), "{problems:?}");
    let shipped = Connectors::load(&tables).0.get("browser").unwrap().start.clone();
    assert!(!shipped.iter().any(|w| w.starts_with("--app")), "a bare window has no tabs and no address bar: {shipped:?}");
    assert_eq!(shipped[shipped.len() - 3..], ["--new-window".to_string(), "--".to_string(), "{url}".to_string()], "`--` stands before the link: one that begins with `-` is not a flag");
    assert!(shipped.contains(&"--user-data-dir={account.browser}".to_string()) && shipped.contains(&"--proxy-server={account.proxy}".to_string()));
    // the shim an account gets is that line, with the link where `{url}` is
    let bin = Scratch::new("browser");
    let program = browser(bin.path());
    // (the program of the fixture stands for `sh -c … {found}`, the browser found, which the shipped line starts with)
    let args = shipped.iter().skip_while(|w| !w.starts_with("--user-data-dir")).map(|w| format!("{w:?}")).collect::<Vec<_>>().join(", ");
    let row = format!("[[connector]]\nid = \"browser\"\nis = \"service\"\ndialect = \"text\"\nprogram = \"{program}\"\ncandidates = []\nprobe = {{ argv = [\"{program}\", \"--version\"] }}\nstart = [{args}]\n");
    let r = rig_with("[[account]]\nid = \"a\"\nconnector = \"lab\"\nproxy = \"socks5://127.0.0.1:1080\"\n", &format!("{LAB}{row}"), None);
    let a = Account { proxy: "socks5://127.0.0.1:1080".into(), ..account(&r, "a") };
    r.book.ensure(&a, &crate_connector(&r)).unwrap();
    let shim = std::fs::read_to_string(r.root.join("a/bin/open")).unwrap();
    assert!(shim.contains("'--new-window' '--' ''\"${1}\"''"), "{shim}");
    assert!(shim.contains("'--proxy-server=socks5://127.0.0.1:1080'") && !shim.contains("--app"), "{shim}");
}

/// A row accepted after the start is live at once (the book reads the files again where it misses), and one removed is gone.
#[test]
fn an_account_row_accepted_after_the_start_is_known_at_once_and_one_removed_is_gone() {
    use std::sync::{Arc, Mutex};
    let (accounts, user) = (Scratch::new("accounts"), Scratch::new("user-home"));
    let files = Arc::new(Mutex::new(String::new()));
    let load = |rows: String| Tables::load(vec![pack("base", "base-data"), pack("accounts", "accounts-data"), mine(&[("account/rows.toml", &rows), ("connectors/extra.toml", LAB)])]).0;
    let none = load(String::new());
    let connectors = Connectors::load(&none).0;
    let launch = |book: &Arc<Book>, id: &str| {
        let mut spec = TaskSpec::new("go", "lab");
        spec.account = Some(id.into());
        book.launcher()(&spec, Launch::new(vec!["x".into()], accounts.path().to_path_buf(), BTreeMap::new()))
    };
    // with no way to read the files again the book is what it was at the start
    let fixed = Arc::new(Book::load(&none, &connectors, accounts.path(), user.path()).0);
    *files.lock().unwrap() = "[[account]]\nid = \"a\"\nconnector = \"lab\"\n".into();
    assert!(launch(&fixed, "a").unwrap_err().contains("no account called `a`"));
    // with it, the miss reads the files: the row accepted a moment ago is there
    let fresh = {
        let files = files.clone();
        Arc::new(move || load(files.lock().unwrap().clone()))
    };
    let book = Arc::new(Book::load(&none, &connectors, accounts.path(), user.path()).0.with_reload(fresh, Vec::new()));
    std::fs::create_dir_all(accounts.path().join("a/home")).unwrap();
    assert!(launch(&book, "a").is_ok());
    assert_eq!(book.logins(), vec![("lab".to_string(), "a".to_string())]);
    // a row that was removed is gone as soon as the files are read again
    files.lock().unwrap().clear();
    assert!(book.logins().is_empty());
    assert!(launch(&book, "a").unwrap_err().contains("no account called `a`"));
}

/// A word the harness hands `$BROWSER` that is no http(s) link never reaches the browser; a link does, after `--`, and a word that
/// begins with `-` leaves the account's proxy and data dir as they were.
#[test]
fn a_link_that_begins_with_a_dash_is_refused_by_the_shim_and_changes_neither_proxy_nor_profile() {
    let (tables, _) = Tables::load(vec![pack("base", "base-data"), pack("accounts", "accounts-data")]);
    let shipped = Connectors::load(&tables).0.get("browser").unwrap().start.clone();
    let bin = Scratch::new("browser");
    let program = bin.path().join("chromium");
    let record = bin.path().join("argv.txt");
    std::fs::write(&program, format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n", record.display())).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    let program = program.display().to_string();
    let args = shipped.iter().skip_while(|w| !w.starts_with("--user-data-dir")).map(|w| format!("{w:?}")).collect::<Vec<_>>().join(", ");
    let row = format!("[[connector]]\nid = \"browser\"\nis = \"service\"\ndialect = \"text\"\nprogram = \"{program}\"\ncandidates = []\nprobe = {{ argv = [\"{program}\", \"--version\"] }}\nstart = [{args}]\n");
    let r = rig_with("[[account]]\nid = \"a\"\nconnector = \"lab\"\nproxy = \"socks5://127.0.0.1:1080\"\n", &format!("{LAB}{row}"), None);
    let a = Account { proxy: "socks5://127.0.0.1:1080".into(), ..account(&r, "a") };
    r.book.ensure(&a, &crate_connector(&r)).unwrap();
    let shim = r.root.join("a/bin/open");
    let run = |link: &str| std::process::Command::new("sh").arg(&shim).arg(link).output().unwrap();
    for bad in ["--proxy-server=direct://", "--user-data-dir=/other", "-x", "file:///etc/passwd", ""] {
        let out = run(bad);
        assert!(!out.status.success(), "{bad:?} is refused");
        assert!(String::from_utf8_lossy(&out.stderr).contains("http(s) link"), "{bad:?}");
    }
    assert!(!record.exists(), "the browser was never started by a word that is no link");
    let out = run("https://lab.example/login");
    assert!(out.status.success());
    let started = std::fs::read_to_string(&record).unwrap();
    assert_eq!(started.lines().count(), 1, "{started}");
    let line = started.trim();
    assert!(line.ends_with("--new-window -- https://lab.example/login"), "{line}");
    assert_eq!(line.matches("--proxy-server=").count(), 1);
    assert_eq!(line.matches("--user-data-dir=").count(), 1);
}

/// A row that does not read (a hand edit, a file read while it is written) does not take the account away: the ones that were are
/// kept and the words say what was refused; and the files are read again only when one of them changed.
#[test]
fn a_row_that_is_refused_keeps_the_accounts_that_were_and_nothing_is_read_again_while_no_file_changed() {
    use std::sync::{Arc, Mutex};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let (accounts, user, watched) = (Scratch::new("accounts"), Scratch::new("user-home"), Scratch::new("watched"));
    let folder = watched.path().join("account");
    std::fs::create_dir_all(&folder).unwrap();
    let files = Arc::new(Mutex::new("[[account]]\nid = \"a\"\nconnector = \"lab\"\n".to_string()));
    let reads = Arc::new(AtomicUsize::new(0));
    let load = |rows: String| Tables::load(vec![pack("base", "base-data"), pack("accounts", "accounts-data"), mine(&[("account/rows.toml", &rows), ("connectors/extra.toml", LAB)])]).0;
    let connectors = Connectors::load(&load(String::new())).0;
    let fresh = {
        let (files, reads) = (files.clone(), reads.clone());
        Arc::new(move || {
            reads.fetch_add(1, Ordering::SeqCst);
            load(files.lock().unwrap().clone())
        })
    };
    let book = Arc::new(Book::load(&load(files.lock().unwrap().clone()), &connectors, accounts.path(), user.path()).0.with_reload(fresh, vec![folder.clone()]));
    assert_eq!(book.logins().len(), 1);
    assert_eq!(reads.load(Ordering::SeqCst), 0, "no file changed since the rows were read: nothing is read again");
    // a file changes, and one row of it is refused: `a` stays, `b` that reads well is taken, and the refusal is said once
    std::fs::write(folder.join("rows.toml"), "changed").unwrap();
    *files.lock().unwrap() = "[[account]]\nid = \"b\"\nconnector = \"lab\"\n\n[[account]]\nid = \"Bad\"\nconnector = \"lab\"\n".into();
    let mut ids: Vec<String> = book.logins().into_iter().map(|l| l.1).collect();
    ids.sort();
    assert_eq!(ids, ["a", "b"], "the account that was is not taken away by a row that is refused");
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    let notes = book.take_notes();
    assert!(notes.len() == 1 && notes[0].contains("lower case letters"), "{notes:?}");
    assert!(book.take_notes().is_empty(), "said once");
    // the same files again: not read; and an account that is missed reads them whatever
    assert_eq!(book.logins().len(), 2);
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    let mut spec = TaskSpec::new("go", "lab");
    spec.account = Some("zz".into());
    let why = book.launcher()(&spec, Launch::new(vec!["x".into()], accounts.path().to_path_buf(), BTreeMap::new())).unwrap_err();
    assert!(why.contains("no account called `zz`") && why.contains("refused"), "{why}");
    assert_eq!(reads.load(Ordering::SeqCst), 2);
}

/// The browser of account windows is the first of its candidates that is on this machine, wherever it stands in the
/// list and with spaces in its path (a macOS app bundle); with none of them here, the window is refused in a line.
#[test]
fn the_account_window_takes_the_first_browser_candidate_that_is_here() {
    let bin = Scratch::new("candidates");
    let app = bin.path().join("Google Chrome.app");
    std::fs::create_dir_all(&app).unwrap();
    let program = browser(&app);
    let row = |first: &str| {
        format!(
            "[[connector]]\nid = \"browser\"\nis = \"service\"\ndialect = \"text\"\nprogram = \"/bin/sh\"\ncandidates = [\"/nowhere/chromium\", \"{first}\"]\nprobe = {{ argv = [\"{{found}}\", \"--version\"] }}\nstart = [\"-c\", \"exec \\\"$0\\\" \\\"$@\\\"\", \"{{found}}\", \"--new-window\", \"--\", \"{{url}}\"]\n"
        )
    };
    let r = rig("[[account]]\nid = \"a\"\nconnector = \"lab\"\n", &format!("{LAB}{}", row(&program)));
    let lab = crate_connector(&r);
    r.book.ensure(&account(&r, "a"), &lab).unwrap();
    let shim = std::fs::read_to_string(r.root.join("a/bin/open")).unwrap();
    assert!(shim.contains(&format!("'{program}'")) && !shim.contains("{found}") && !shim.contains("/nowhere"), "{shim}");

    let none = rig("[[account]]\nid = \"a\"\nconnector = \"lab\"\n", &format!("{LAB}{}", row("/nowhere/chrome")));
    let err = none.book.ensure(&account(&none, "a"), &lab).unwrap_err();
    assert!(err.contains("no browser for account windows") && err.contains("Chromium, Google Chrome"), "{err}");
}

/// An account is a sign-in: a task that runs as one is not given the key J4n keeps for the program's provider, which the program would use
/// in its place; what else the launch carries stays.
#[test]
fn a_task_run_as_an_account_is_not_given_a_key_j4n_keeps_for_its_program() {
    let keyed = LAB.replace("login = [\"~/.lab\"]\n", "login = [\"~/.lab\"]\nsecrets = { LAB_API_KEY = \"kept:acme\" }\n");
    let r = rig("[[account]]\nid = \"a\"\nconnector = \"lab\"\n", &keyed);
    assert!(r.said.is_empty(), "{:?}", r.said);
    std::fs::create_dir_all(r.root.join("a/home")).unwrap();
    let mut spec = TaskSpec::new("go", "lab");
    spec.account = Some("a".into());
    let given: BTreeMap<String, String> = [("LAB_API_KEY", "kept-canary-plain-7c41d09e5b"), ("LAB_MODE", "fast")].into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let launch = r.book.launcher()(&spec, Launch::new(vec!["x".into()], r.root.clone(), given)).unwrap();
    assert!(!launch.env.contains_key("LAB_API_KEY"), "{:?}", launch.env);
    assert_eq!(launch.env.get("LAB_MODE").map(String::as_str), Some("fast"));
    assert_eq!(launch.env["LAB_HOME"], r.root.join("a/home").display().to_string(), "the account's own sign-in");
}
