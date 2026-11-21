//! Accounts: several logins of one harness kept apart. The front, the loop, the Gate, the files
//! and the folders are the real ones, wired as the binary wires them; the harness is a script that calls `$BROWSER` or `xdg-open` and
//! writes a login into its config folder, and the browser is a script that records its arguments.

use crate::screen_drive::{Operator, asked, flat};
use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::gate::grant::here_dir;
use j4n_core::task::Mode;
use j4n_tui::headless::alt;
use std::fs;
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn opts() -> Opts {
    Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Ask), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false }
}

fn open(d: &Dirs) -> World {
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    crate::support::opened(|| wire::open(setup.clone(), &opts(), true, "main"))
}

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn put(dir: &Path, rel: &str, text: &str) {
    let p = dir.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

fn held(d: &Dirs, file: &str) -> String {
    fs::read_to_string(here_dir(&d.j4n_home(), &d.project()).join(file)).unwrap_or_default()
}

/// What a sign-in leaves behind for the test to look at, outside the account's folder.
struct Lab {
    out: PathBuf,
}

impl Lab {
    fn read(&self, name: &str) -> String {
        fs::read_to_string(self.out.join(name)).unwrap_or_default()
    }
}

/// The operator's files: the pack as it comes (on by default), no walls, a harness `lab` whose login calls `how` (`$BROWSER`, `xdg-open`, or only prints its
/// link) and writes a login into `$LAB_HOME` unless it is made to fail, and a browser that records what it was started with.
fn furnish(d: &Dirs, how: &str, login_ok: bool) -> Lab {
    let out = d.dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let bin = d.dir.join("bin");
    let o = out.display().to_string();
    let browser = script(&bin, "fake-browser", &format!("case \"$1\" in --version) echo \"Chromium 1.0\"; exit 0;; esac\nprintf '%s\\n' \"$*\" >> '{o}/browser.log'\n[ -e '{o}/fail' ] && exit 1\nexit 0"));
    let open_link = match how {
        "browser" => "\"$BROWSER\" https://lab.example/login",
        "xdg" => "xdg-open https://lab.example/login",
        _ => "echo 'open this link: https://lab.example/login'",
    };
    let write_login = if login_ok { "printf '{\"email\":\"person@example.org\",\"plan\":\"pro\"}' > \"$LAB_HOME/auth.json\"" } else { "true" };
    let login = script(
        &bin,
        "lab-login",
        &format!(
            "echo \"login for $LAB_HOME\"\necho \"$*\" > '{o}/args.txt'\nenv | grep -E '^(https?_proxy|HTTPS?_PROXY|ALL_PROXY|all_proxy|no_proxy|NO_PROXY|BROWSER|LAB_HOME|HOME)=' | sort > '{o}/env.txt'\ncommand -v xdg-open > '{o}/xdg.txt'\n{open_link}\n{write_login}"
        ),
    );
    let status = script(&bin, "lab-status", "if [ -f \"$LAB_HOME/auth.json\" ]; then e=$(sed -n 's/.*\"email\":\"\\([^\"]*\\)\".*/\\1/p' \"$LAB_HOME/auth.json\"); echo \"{\\\"loggedIn\\\":true,\\\"email\\\":\\\"$e\\\",\\\"plan\\\":\\\"pro\\\"}\"; else echo '{\"loggedIn\":false}'; fi");
    // a lane run as an account: it says where its config is and what it was told about proxies
    let agent = script(&bin, "lab-agent", "echo \"home=$LAB_HOME proxy=$https_proxy no=$no_proxy\"");
    let here = here_dir(&d.j4n_home(), &d.project());
    put(&here, "settings.toml", "api = 1\n[[setting]]\nid = \"box.unavailable\"\nvalue = \"here\"\n");
    put(
        &here,
        "connectors/browser.toml",
        &format!("api = 1\n[[connector]]\nid = \"browser\"\nis = \"service\"\ndialect = \"text\"\nprogram = \"{b}\"\ncandidates = []\nprobe = {{ argv = [\"{b}\", \"--version\"] }}\nstart = [\"--user-data-dir={{account.browser}}\", \"--proxy-server={{account.proxy}}\", \"--app={{url}}\"]\n", b = browser.display()),
    );
    put(
        &here,
        "connectors/lab.toml",
        &format!(
            "api = 1\n[[connector]]\nid = \"lab\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"{agent}\"\nstart = [\"{{prompt}}\"]\n\n[connector.signin]\nargv = [\"{login}\"]\nagain = [\"--email\", \"{{account.email}}\"]\nenv = {{ LAB_HOME = \"{{account.home}}\" }}\npage = \"https://lab.example/settings\"\n\n[connector.signin.check]\nargv = [\"{status}\"]\nok = \"/loggedIn\"\nemail = \"/email\"\nplan = \"/plan\"\n",
            agent = agent.display(),
            login = login.display(),
            status = status.display()
        ),
    );
    Lab { out }
}

fn accounts(d: &Dirs) -> PathBuf {
    d.j4n_home().join("accounts")
}

/// Add an account the way the operator does: the line is the approval, so it is written at once as their edit; `signin=no` leaves the sign-in for
/// the test to start. J4n is left so that the next start reads the row, as a restart of the operator's would.
fn add(d: &Dirs, line: &str) {
    let world = open(d);
    let mut op = Operator::sit(&world);
    let edited = events(&world, "edit.applied").len();
    op.type_line(&format!("{line} signin=no"));
    wait_for(&mut op, &world, "the row", || events(&world, "edit.applied").len() == edited + 1);
    drop(op);
    world.close();
}

/// Let the screen take what the core says until `done` holds: the screen replays the chat of an earlier start, so what it shows is not
/// evidence of what just happened.
fn wait_for(op: &mut Operator, _world: &World, what: &str, done: impl Fn() -> bool) {
    let end = Instant::now() + crate::support::PATIENCE;
    while !done() {
        assert!(Instant::now() < end, "never saw {what}; the screen is\n{}", op.look());
        op.look();
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn events(world: &World, kind: &str) -> Vec<serde_json::Value> {
    world.front.log().sync();
    world.front.log().read(0).filter(|e| e.kind == kind).map(|e| e.data).collect()
}

/// Sign in and wait for the account's own words that it is: the screen keeps the chat of earlier sign-ins, so the account is named.
fn sign_in(op: &mut Operator, world: &World, id: &str) -> String {
    sign_in_until(op, world, id, &format!("account {id} is signed in"))
}

fn sign_in_until(op: &mut Operator, world: &World, id: &str, done: &str) -> String {
    crate::screen_drive::wait_idle(world);
    op.type_line(&format!(":account signin {id}"));
    op.until("the leave to sign in", |s| asked(s) && s.contains(&format!("sign in account `{id}`")));
    op.press(vec![alt('y')]);
    let s = op.until("the end of the sign-in", |s| s.contains(done));
    crate::screen_drive::wait_idle(world);
    s
}

/// Type a line once nothing else goes on, and wait for its answer.
fn say(op: &mut Operator, world: &World, line: &str, answer: &str) -> String {
    crate::screen_drive::wait_idle(world);
    op.type_line(line);
    op.until(answer, |s| s.contains(answer))
}

#[test]
fn a_sign_in_runs_the_harness_in_its_own_folder_and_its_link_opens_in_the_accounts_own_window() {
    let d = Dirs::new("acct-signin");
    let lab = furnish(&d, "browser", true);
    add(&d, ":account add lab work the work subscription");
    assert!(held(&d, "account/work.toml").contains("connector = \"lab\""), "{}", held(&d, "account/work.toml"));
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    let mut op = Operator::sit(&world);
    let s = sign_in(&mut op, &world, "work");
    assert!(s.contains("login for"), "the harness's own words are shown: {s}");

    let dir = accounts(&d).join("work");
    for sub in ["browser", "home", "bin"] {
        assert!(dir.join(sub).is_dir(), "{sub}");
    }
    let mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700, "the folder is the operator's alone");
    // the login's config folder is the account's, and the real home is where it was
    assert!(lab.read("env.txt").contains(&format!("LAB_HOME={}", dir.join("home").display())), "{}", lab.read("env.txt"));
    assert!(dir.join("home/auth.json").exists());
    // its link went to the shim, which started the browser with the account's data dir and the link as an app window
    let log = lab.read("browser.log");
    assert!(log.contains(&format!("--user-data-dir={}", dir.join("browser").display())) && log.contains("--app=https://lab.example/login"), "{log}");
    assert!(lab.read("env.txt").contains(&format!("BROWSER={}", dir.join("bin/open").display())));
    // the sign-in is a fact in the Log, and the email is private
    let signin = events(&world, "account.signin");
    assert_eq!(signin.len(), 1);
    assert_eq!(signin[0]["ok"], true);
    let checked = events(&world, "account.checked");
    assert_eq!(checked[0]["email"], "person@example.org");
    let class = world.front.log().read(0).find(|e| e.kind == "account.checked").unwrap().class;
    assert_eq!(class.word(), "private");
    world.close();
}

#[test]
fn two_accounts_have_two_folders_and_a_lane_run_as_one_has_its_environment_and_nothing_of_the_other() {
    let d = Dirs::new("acct-two");
    let _lab = furnish(&d, "browser", true);
    add(&d, ":account add lab work");
    add(&d, ":account add lab personal");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    sign_in(&mut op, &world, "work");
    sign_in(&mut op, &world, "personal");
    assert!(accounts(&d).join("work/home/auth.json").exists() && accounts(&d).join("personal/home/auth.json").exists());
    assert_ne!(accounts(&d).join("work/browser"), accounts(&d).join("personal/browser"));

    // a thread bound to the harness as `personal`: its child is told the account's folder, and not the other's
    crate::screen_drive::wait_idle(&world);
    op.type_line(":switch lab account=personal");
    op.type_line("say where you are");
    let s = op.until("the lane's words", |s| s.contains("home="));
    // a long word is cut at the width of the transcript, the rest on a line indented under it: put it back together
    let lane: String = s.split("say where you are").last().unwrap().lines().map(|l| l.trim_start_matches(|c: char| " │├╰─".contains(c))).collect();
    assert!(lane.contains(&accounts(&d).join("personal/home").display().to_string()), "{s}");
    assert!(!lane.contains("work/home"), "{s}");
    crate::screen_drive::wait_idle(&world);
    op.type_line(":switch lab account=work");
    op.type_line("say it again");
    op.until("the other account", |s| s.split("say it again").last().unwrap().lines().map(|l| l.trim_start_matches(|c: char| " │├╰─".contains(c))).collect::<String>().contains(&accounts(&d).join("work/home").display().to_string()));
    world.close();
}

#[test]
fn an_accounts_proxy_reaches_its_window_and_its_children_and_the_system_one_stands_for_an_account_that_names_none() {
    let d = Dirs::new("acct-proxy");
    let lab = furnish(&d, "browser", true);
    // a proxy that answers: anything that connects is let in and dropped
    let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}", proxy.local_addr().unwrap().port());
    std::thread::spawn(move || for _ in proxy.incoming() {});
    add(&d, &format!(":account add lab routed proxy={url}"));
    add(&d, ":account add lab plain");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    sign_in(&mut op, &world, "routed");
    let env = lab.read("env.txt");
    for name in ["https_proxy", "HTTPS_PROXY", "http_proxy", "ALL_PROXY"] {
        assert!(env.contains(&format!("{name}={url}")), "{name}: {env}");
    }
    assert!(env.contains("no_proxy=localhost,127.0.0.1,::1"), "{env}");
    assert!(lab.read("browser.log").contains(&format!("--proxy-server={url}")), "{}", lab.read("browser.log"));
    // the other names none: its window gets the proxy the process has (the hermetic runner has a dead one), its children are not told one
    let system = ["https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()));
    fs::write(lab.out.join("browser.log"), "").unwrap();
    sign_in(&mut op, &world, "plain");
    let log = lab.read("browser.log");
    match &system {
        Some(p) => assert!(log.contains(&format!("--proxy-server={p}")), "{log}"),
        None => assert!(!log.contains("--proxy-server"), "{log}"),
    }
    assert!(!lab.read("env.txt").contains(&url), "{}", lab.read("env.txt"));
    world.close();
}

#[test]
fn a_proxy_that_does_not_answer_stops_the_sign_in_and_is_named() {
    let d = Dirs::new("acct-deadproxy");
    let lab = furnish(&d, "browser", true);
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    add(&d, &format!(":account add lab far proxy=socks5://127.0.0.1:{port}"));
    let world = open(&d);
    let mut op = Operator::sit(&world);
    crate::screen_drive::wait_idle(&world);
    op.type_line(":account signin far");
    let s = op.until("why", |s| s.contains("does not accept a connection"));
    assert!(s.contains(&format!("127.0.0.1:{port}")), "{s}");
    assert_eq!(lab.read("env.txt"), "", "the login never ran");
    world.close();
}

#[test]
fn a_login_that_calls_xdg_open_lands_in_the_accounts_window_and_one_that_only_prints_its_link_opens_nothing() {
    let d = Dirs::new("acct-xdg");
    let lab = furnish(&d, "xdg", true);
    add(&d, ":account add lab work");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    sign_in(&mut op, &world, "work");
    // the xdg-open the login found first is the account's own
    let found = lab.read("xdg.txt");
    assert_eq!(found.trim(), accounts(&d).join("work/bin/xdg-open").display().to_string());
    assert!(lab.read("browser.log").contains("--app=https://lab.example/login"), "{}", lab.read("browser.log"));
    world.close();

    // the other only prints a link: nothing opens, and the link pasted into `open` lands in the window
    let d = Dirs::new("acct-print");
    let lab = furnish(&d, "print", true);
    add(&d, ":account add lab work");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    let s = sign_in(&mut op, &world, "work");
    assert!(s.contains("open this link: https://lab.example/login"), "{s}");
    assert_eq!(lab.read("browser.log"), "", "nothing was opened");
    say(&mut op, &world, ":account open work https://lab.example/login", "opened the window of account work");
    let log = lab.read("browser.log");
    assert!(log.contains("--app=https://lab.example/login") && log.contains(&format!("--user-data-dir={}", accounts(&d).join("work/browser").display())), "{log}");
    assert_eq!(events(&world, "account.opened").len(), 1);
    // without a link it opens the page the connector names
    say(&mut op, &world, ":account open work", "opened the window of account work at https://lab.example/settings");
    world.close();
}

#[test]
fn a_login_that_the_harness_says_did_not_happen_is_a_failure_with_no_email_and_the_account_still_needs_a_sign_in() {
    let d = Dirs::new("acct-fail");
    let _lab = furnish(&d, "browser", false);
    add(&d, ":account add lab work");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    let s = sign_in_until(&mut op, &world, "work", "is not signed in");
    assert!(s.contains("the harness says it is not signed in"), "{s}");
    let signin = events(&world, "account.signin");
    assert_eq!(signin[0]["ok"], false);
    assert_eq!(signin[0]["state"], "failed");
    assert!(events(&world, "account.checked").is_empty(), "no email was recorded");
    // a look says the same
    say(&mut op, &world, ":account check work", "is not signed in: :account signin work");
    let checked = events(&world, "account.checked");
    assert_eq!(checked[0]["ok"], false);
    assert_eq!(checked[0]["state"], "needs sign-in");
    assert!(checked[0]["email"].is_null());
    world.close();
}

#[test]
fn a_config_file_in_an_accounts_home_is_not_read_as_a_table() {
    let d = Dirs::new("acct-config");
    let _lab = furnish(&d, "browser", true);
    add(&d, ":account add lab work");
    // what a harness keeps in its config folder: a `config.toml` that is nothing of J4n's
    put(&accounts(&d).join("work/home"), "config.toml", "[[connector]]\nid = \"zz\"\nthis is not even a row\n[[gate]]\nverdict = \"allow\"\n");
    put(&accounts(&d).join("work/home"), "nested/more.toml", "api = 1\n[[connector]]\nid = \"zz2\"\n");
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    world.close();
}

#[test]
fn removing_an_account_asks_every_time_deletes_its_folder_and_an_undo_brings_the_row_back_and_not_the_folder() {
    let d = Dirs::new("acct-remove");
    let _lab = furnish(&d, "browser", true);
    add(&d, ":account add lab work");
    add(&d, ":account add lab other");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    sign_in(&mut op, &world, "work");
    sign_in(&mut op, &world, "other");
    crate::screen_drive::wait_idle(&world);
    op.type_line(":account remove work");
    let s = op.until("the question", |s| s.contains("remove `work`?") && asked(s));
    assert!(flat(&s).contains("not the folder"), "the Ask says what an undo will not do: {s}");
    assert!(accounts(&d).join("work").exists(), "nothing is deleted before the yes");
    // a write outside the project is asked about every time, and there is no standing yes to give for it
    assert!(!s.contains("always") && !s.contains("session"), "{s}");
    op.press(vec![alt('y')]);
    op.until("the end", |s| s.contains("account work is removed"));
    assert!(!accounts(&d).join("work").exists());
    assert!(accounts(&d).join("other").exists());
    assert!(!held(&d, "account/work.toml").contains("connector"), "{}", held(&d, "account/work.toml"));
    // the other is asked as well: the first yes was for the first
    crate::screen_drive::wait_idle(&world);
    op.type_line(":account remove other");
    op.until("the question again", |s| s.contains("remove `other`?") && asked(s));
    op.press(vec![alt('n')]);
    op.until("the refusal", |s| s.contains("not allowed"));
    assert!(accounts(&d).join("other").exists(), "a no leaves the folder");
    // undo of the removal: the row is back, the folder is not
    crate::screen_drive::wait_idle(&world);
    op.type_line(":undo");
    op.until("the undo", |s| s.contains("took back 1 change(s)"));
    assert!(held(&d, "account/work.toml").contains("connector = \"lab\""), "{}", held(&d, "account/work.toml"));
    assert!(!accounts(&d).join("work").exists(), "the folder does not come back");
    world.close();
}

#[test]
fn a_connector_that_declares_no_sign_in_is_told_so_and_an_account_of_a_name_nobody_made_is_refused() {
    let d = Dirs::new("acct-refuse");
    let _lab = furnish(&d, "browser", true);
    put(&here_dir(&d.j4n_home(), &d.project()), "connectors/plain.toml", "api = 1\n[[connector]]\nid = \"plain\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"/bin/echo\"\nstart = [\"{prompt}\"]\n");
    add(&d, ":account add plain solo");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    say(&mut op, &world, ":account signin solo", "declares no sign-in");
    say(&mut op, &world, ":account signin nobody", "no account called `nobody`");
    say(&mut op, &world, ":account add nothing-like-it x", "there is no connector called `nothing-like-it`");
    say(&mut op, &world, ":account add lab Bad_Id", "lower case letters, digits");
    world.close();
}

#[test]
fn a_second_sign_in_starts_the_page_on_the_account_that_was_signed_in_before() {
    let d = Dirs::new("acct-again");
    let lab = furnish(&d, "browser", true);
    add(&d, ":account add lab work");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    sign_in(&mut op, &world, "work");
    assert_eq!(lab.read("args.txt").trim(), "", "the first time it knows nobody");
    sign_in(&mut op, &world, "work");
    assert_eq!(lab.read("args.txt").trim(), "--email person@example.org", "the second time it is told whose page to start on");
    world.close();
}

#[test]
fn a_check_with_no_pointer_takes_an_exit_of_zero_for_a_yes_and_anything_else_for_a_no() {
    let d = Dirs::new("acct-exit");
    let lab = furnish(&d, "browser", true);
    let exit = script(&d.dir.join("bin"), "lab-exit", "test -f \"$LAB_HOME/auth.json\"");
    let login = d.dir.join("bin/lab-login");
    put(
        &here_dir(&d.j4n_home(), &d.project()),
        "connectors/exiter.toml",
        &format!(
            "api = 1\n[[connector]]\nid = \"exiter\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"/bin/echo\"\nstart = [\"{{prompt}}\"]\n\n[connector.signin]\nargv = [\"{login}\"]\nenv = {{ LAB_HOME = \"{{account.home}}\" }}\n\n[connector.signin.check]\nargv = [\"{exit}\"]\n",
            login = login.display(),
            exit = exit.display()
        ),
    );
    add(&d, ":account add exiter work");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    sign_in(&mut op, &world, "work");
    let checked = events(&world, "account.checked");
    assert_eq!(checked[0]["ok"], true);
    assert!(checked[0]["email"].is_null(), "an exit says nothing of who");
    // the login is taken away: the same check says no
    fs::remove_file(accounts(&d).join("work/home/auth.json")).unwrap();
    say(&mut op, &world, ":account check work", "is not signed in: :account signin work");
    assert_eq!(events(&world, "account.checked")[1]["ok"], false);
    let _ = lab;
    world.close();
}

#[test]
fn the_accounts_page_lists_each_account_with_its_keys_and_says_where_each_stands() {
    use j4n_tui::headless::{KeyCode, press};
    let d = Dirs::new("acct-page");
    let _lab = furnish(&d, "browser", true);
    add(&d, ":account add lab work the work subscription");
    add(&d, ":account add lab spare");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    sign_in(&mut op, &world, "work");
    crate::screen_drive::wait_idle(&world);
    op.type_line(":accounts");
    // a row of the page: the id, the connector, the route and the remark, on one line
    let row = |s: &str, id: &str| s.lines().any(|l| l.contains(id) && l.contains("lab") && l.contains("system") && !crate::screen_drive::is_you(l) && !l.contains("proposed"));
    let s = op.until("the page", |s| row(s, "work") && row(s, "spare"));
    assert!(s.contains("the work subscription"), "{s}");
    // the first row's key `c` sends the line the operator would type: a look at the account
    op.press(vec![press(KeyCode::Char('c'))]);
    let end = Instant::now() + crate::support::PATIENCE;
    while !world.front.tasks().book().rows().iter().any(|r| r.spec.goal == "check spare") {
        assert!(Instant::now() < end, "the key sent no line");
        op.look();
        std::thread::sleep(Duration::from_millis(20));
    }
    crate::screen_drive::wait_idle(&world);
    // where each stands is the fold of what J4n learned, on the same page (Signed in): the one that was signed in, with
    // the email it signed in as under its row, and the one that was not
    op.type_line(":accounts");
    let s = op.until("where each stands", |s| s.lines().any(|l| l.contains("work") && l.contains("signed in") && !crate::screen_drive::is_you(l)));
    assert!(s.lines().any(|l| l.contains("spare") && l.contains("needs sign-in")), "{s}");
    op.press(vec![press(KeyCode::Down)]);
    op.until("the email under the row of work", |s| s.contains("person@example.org"));
    world.close();
}

#[test]
fn a_login_that_ends_badly_is_a_failure_whatever_it_wrote_and_an_account_is_not_added_twice() {
    let d = Dirs::new("acct-badend");
    let _lab = furnish(&d, "browser", true);
    let login = d.dir.join("bin/lab-login");
    fs::write(&login, format!("{}\nexit 3\n", fs::read_to_string(&login).unwrap())).unwrap();
    add(&d, ":account add lab work");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    let s = sign_in_until(&mut op, &world, "work", "is not signed in");
    assert!(s.contains("the login ended: exit 3"), "{s}");
    assert_eq!(events(&world, "account.signin")[0]["ok"], false);
    // there is an account of that name: a second is refused
    say(&mut op, &world, ":account add lab work", "there is an account called `work` already");
    drop(op);
    drop(world);
}

#[test]
fn a_window_is_opened_for_a_link_only_and_one_that_does_not_open_is_said() {
    let d = Dirs::new("acct-open");
    let lab = furnish(&d, "browser", true);
    add(&d, ":account add lab work");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    say(&mut op, &world, ":account open work javascript:alert(1)", "is not a link that starts with https://");
    crate::screen_drive::wait_idle(&world);
    say(&mut op, &world, ":account open work file:///etc/passwd", "is not a link that starts with https://");
    crate::screen_drive::wait_idle(&world);
    assert_eq!(lab.read("browser.log"), "", "no window was started for either");
    fs::write(lab.out.join("fail"), "").unwrap();
    say(&mut op, &world, ":account open work https://lab.example/x", "the window did not open: exit 1");
    drop(op);
    drop(world);
}

#[test]
fn an_account_removed_and_added_again_can_be_removed_again() {
    let d = Dirs::new("acct-readd");
    let _lab = furnish(&d, "browser", true);
    for round in 0..2 {
        add(&d, ":account add lab work");
        assert!(held(&d, "account/work.toml").contains("connector = \"lab\""), "round {round}: {}", held(&d, "account/work.toml"));
        let world = open(&d);
        let mut op = Operator::sit(&world);
        crate::screen_drive::wait_idle(&world);
        op.type_line(":account remove work");
        op.until("the question", |s| s.contains("remove `work`?") && asked(s));
        op.press(vec![alt('y')]);
        let end = Instant::now() + crate::support::PATIENCE;
        while held(&d, "account/work.toml").contains("connector") {
            assert!(Instant::now() < end, "round {round}: the row stayed: {}", held(&d, "account/work.toml"));
            op.look();
            std::thread::sleep(Duration::from_millis(20));
        }
        crate::screen_drive::wait_idle(&world);
        drop(op);
        drop(world);
    }
}

/// Everything an account would have left on disk or in the Log.
fn account_traces(d: &Dirs, world: &World) -> Vec<String> {
    let mut found = Vec::new();
    if accounts(d).exists() {
        found.push(format!("the folder {}", accounts(d).display()));
    }
    found.extend(world.front.log().read(0).filter(|e| e.kind.starts_with("account.")).map(|e| e.kind));
    found
}

#[test]
fn with_no_account_added_a_run_asks_nothing_makes_no_folder_and_the_pack_is_on_without_a_word() {
    let d = Dirs::new("acct-quiet");
    // the pack as it comes: no harness.toml, no accounts folder, no account row
    assert!(!d.j4n_home().join("harness.toml").exists());
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    let harness = j4n::pack::catalog().harness(&j4n::pack::ON, &[]);
    assert!(harness.info.iter().any(|i| i.id == "accounts" && !i.on), "the pack `accounts` is off in J4n basic");
    assert!(world.front.screen().packs.iter().any(|i| i.id == "accounts" && i.on), "and on in this folder, which turned it on");
    let mut op = Operator::sit(&world);
    // a plain line runs as the operator, under their own login: no account to ask for, none to fail on
    op.type_line("say hello");
    // the line reaches the core a moment after it is typed, and "every task is over" is true of no task at all: the task is waited for
    wait_for(&mut op, &world, "the line's task", || !world.front.tasks().book().rows().is_empty());
    crate::screen_drive::wait_idle(&world);
    let rows = world.front.tasks().book().rows();
    assert!(!rows.is_empty() && rows.iter().all(|r| r.spec.account.is_none()), "{:?}", rows.iter().map(|r| &r.spec.account).collect::<Vec<_>>());
    assert!(rows.iter().all(|r| matches!(r.state, j4n_core::task::State::Done { ok: true })), "{:?}", rows.iter().map(|r| &r.state).collect::<Vec<_>>());
    // the page is there and empty; looking at it makes nothing
    op.type_line(":accounts");
    op.until("the page", |s| s.contains("Accounts"));
    assert_eq!(account_traces(&d, &world), Vec::<String>::new(), "nothing is made until `:account add`");
    assert!(!asked(&op.look()), "nothing was asked");
    world.close();
    assert!(!accounts(&d).exists(), "no folder after the run either");
}

#[test]
fn account_add_claude_and_switch_work_out_of_the_box_and_a_row_names_its_plan() {
    let d = Dirs::new("acct-box");
    // the real `claude` connector, as shipped: adding an account only proposes a row, and signs in to nothing
    add(&d, ":account add claude work plan=max5 the max subscription");
    add(&d, ":account add claude side plan=pro");
    let row = held(&d, "account/work.toml");
    assert!(row.contains("connector = \"claude\"") && row.contains("plan = \"max5\""), "{row}");
    assert!(held(&d, "account/side.toml").contains("plan = \"pro\""));
    assert!(!accounts(&d).exists(), "adding makes the row; the folder is made by the first sign-in");
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    let mut op = Operator::sit(&world);
    // a thread is bound to the account; the binding is a fact in the Log
    crate::screen_drive::wait_idle(&world);
    // (no `claude` program is on the PATH of a hermetic run, so the doer here is the test one; the binding is what is looked at)
    op.type_line(":switch test account=work");
    let end = Instant::now() + crate::support::PATIENCE;
    while !world.front.log().read(0).any(|e| e.kind == "thread.bound" && e.data["account"] == "work") {
        assert!(Instant::now() < end, "never bound; the screen is\n{}", op.look());
        op.look();
        std::thread::sleep(Duration::from_millis(20));
    }
    // the page lists both
    op.type_line(":accounts");
    op.until("both rows", |s| ["work", "side"].iter().all(|id| s.lines().any(|l| l.contains(id) && l.contains("claude") && l.contains("system") && !crate::screen_drive::is_you(l) && !l.contains("proposed"))));
    assert!(!accounts(&d).exists(), "binding makes no folder either: a launch does, and none was made");
    world.close();
}

#[test]
fn an_account_naming_a_plan_nobody_wrote_is_said_once_and_keeps_the_connectors_own_plan() {
    let d = Dirs::new("acct-plan-ghost");
    add(&d, ":account add claude ghost plan=no-such-tier");
    let world = open(&d);
    assert!(world.notices.iter().any(|n| n.contains("no-such-tier") && n.contains("ghost")), "{:?}", world.notices);
    world.close();
}

#[test]
fn an_account_command_given_wrong_words_answers_with_its_usage_and_starts_nothing() {
    let d = Dirs::new("acct-usage");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    for line in [":account signin", ":account", ":account frob work", ":account remove a b"] {
        op.type_line(line);
        let s = op.until("the usage", |s| s.contains("usage: :account add <connector> <id>"));
        assert!(!s.contains("needs you") && !s.contains("failed"), "{s}");
    }
    assert!(events(&world, "task.queued").is_empty(), "no task was made");
    assert!(events(&world, "flow.started").is_empty(), "no flow was started");
    // a line that fits a form still gets to the flow: the usage is only for what fits none
    op.type_line(":account check nobody");
    let end = Instant::now() + crate::support::PATIENCE;
    while events(&world, "task.queued").is_empty() {
        assert!(Instant::now() < end, "never started; the screen is\n{}", op.look());
        op.look();
        std::thread::sleep(Duration::from_millis(20));
    }
    // `:account add claude` with no id opens the form and starts no task.
    // The count is taken once the check's flow has queued all it will
    crate::screen_drive::wait_idle(&world);
    let queued = events(&world, "task.queued").len();
    op.type_line(":account add claude");
    op.until("the form", |s| s.contains("add account") && s.contains("claude-1"));
    assert_eq!(events(&world, "task.queued").len(), queued, "the form is no task");
    world.close();
}

// ---- :account add, end to end ----

/// the operator's own `:account add` writes the row at once and goes on to the sign-in in the same run, with no `:accept`
/// and no restart; the line draws one row and no model, and its task is given no Brief.
#[test]
fn the_operators_own_add_is_the_approval_and_goes_on_to_the_sign_in_with_no_restart() {
    let d = Dirs::new("acct-add-live");
    let lab = furnish(&d, "browser", true);
    let world = open(&d);
    let mut op = Operator::sit(&world);
    crate::screen_drive::wait_idle(&world);
    op.type_line(":account add lab work the work subscription");
    op.until("the leave to sign in", |s| asked(s) && s.contains("sign in account `work`"));
    // the row is written before the sign-in is asked for: no proposal waits for an `:accept`
    assert!(held(&d, "account/work.toml").contains("connector = \"lab\""), "{}", held(&d, "account/work.toml"));
    assert_eq!((events(&world, "proposal.made").len(), events(&world, "edit.applied").len()), (0, 1), "an edit, no Proposal");
    op.press(vec![alt('y')]);
    let s = op.until("the end of the sign-in", |s| s.contains("account work is signed in as person@example.org"));
    crate::screen_drive::wait_idle(&world);
    assert!(s.contains("added account work (lab)"), "{s}");
    assert!(accounts(&d).join("work/home/auth.json").exists());
    assert_eq!(events(&world, "account.signin").len(), 1);
    // a flow with one code step is one row, which names no model; the code step is given no Brief (the contract of a line said to a model)
    assert!(!s.contains("model not known yet") && !s.contains("A flow:account") && !s.contains("B account"), "{s}");
    // the ledger of the turn says the answer once: the flow's is what its step said, and is not `= t2 same as t1`
    let s = op.until("the ledger of the turn", |s| s.contains("◆ ledger · main") && s.contains("started by J4n") && s.contains("t2"));
    assert!(!s.contains("same as t1"), "{s}");
    let rows = world.front.tasks().book().rows();
    let child = rows.iter().find(|r| r.spec.doer == "account").expect("the code step");
    assert!(child.spec.input.get("brief").is_none(), "{:?}", child.spec.input);
    // the account is known to the running J4n: a second sign-in needs no restart either
    let s = sign_in_until(&mut op, &world, "work", "account work is signed in as person@example.org");
    assert!(s.contains("signed in"), "{s}");
    assert_eq!(lab.read("args.txt").trim(), "--email person@example.org");
    world.close();
}

/// an account accepted by `:accept` (a machine's proposal) is live too, and a line that names an account nobody made fails as `permanent`.
#[test]
fn an_accepted_account_row_is_live_at_once_and_a_missing_one_is_a_permanent_failure() {
    use j4n_core::change::Actor;
    use j4n_core::task::TaskSpec;
    let d = Dirs::new("acct-machine-add");
    let _lab = furnish(&d, "browser", true);
    let world = open(&d);
    let mut op = Operator::sit(&world);
    crate::screen_drive::wait_idle(&world);
    // a machine's add (a rule, not the operator's line) is a proposal: nothing is written, and it says what to do
    let spec = TaskSpec::new("add lab ghost a machine's idea", "account");
    world.front.tasks().spawn(spec, Actor::Rule("machine".into())).unwrap();
    crate::screen_drive::wait_idle(&world);
    let made = events(&world, "proposal.made");
    assert_eq!(made.len(), 1, "{made:?}");
    assert!(events(&world, "proposal.accepted").is_empty());
    assert!(!held(&d, "account/ghost.toml").contains("connector"), "nothing was written");
    // before the yes it is no account; after it, the running J4n knows it with no restart
    let before = world.front.tasks().spawn(TaskSpec::new("check ghost", "account"), Actor::Operator).unwrap();
    crate::screen_drive::wait_idle(&world);
    let row = world.front.tasks().book().get(&before).unwrap();
    assert_eq!(row.class.as_deref(), Some("permanent"), "an account nobody made is not made by a retry: {row:?}");
    op.type_line(":accept");
    wait_for(&mut op, &world, "the yes", || events(&world, "proposal.accepted").len() == 1);
    crate::screen_drive::wait_idle(&world);
    op.type_line(":account check ghost");
    // the account is known now: it is looked for, and has no folder yet (the answer is not "no account called")
    op.until("the account is known", |s| s.contains("account `ghost` has no folder yet"));
    world.close();
}

/// The operator's line is the approval only where the goal of the add is what they typed. A flow of a pack whose `account` step has a
/// goal it builds (from what a model said, `{steps.x.answer}`) is not their line, though the operator started the flow: it is a proposal.
#[test]
fn a_flow_that_builds_the_goal_of_an_add_is_a_proposal_though_the_operator_started_it() {
    use j4n_core::change::Actor;
    use j4n_core::task::TaskSpec;
    let d = Dirs::new("acct-templated");
    let _lab = furnish(&d, "browser", true);
    put(
        &here_dir(&d.j4n_home(), &d.project()),
        "flows/relay.toml",
        "api = 1\n[[flow]]\nid = \"relay\"\ndoc = \"builds the goal of an add\"\nin = { goal = \"text\" }\nsteps = [\n  { id = \"do\", do = \"account\", goal = \"add lab sneaky {in.goal}\", out = { done = \"text\" } },\n  { id = \"done\", kind = \"end\", answer = \"{steps.do.answer}\" },\n]\n",
    );
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    crate::screen_drive::wait_idle(&world);
    world.front.tasks().spawn(TaskSpec::new("signin=no", "flow:relay"), Actor::Operator).unwrap();
    crate::screen_drive::wait_idle(&world);
    assert_eq!(events(&world, "proposal.made").len(), 1);
    assert!(events(&world, "proposal.accepted").is_empty(), "nothing was accepted for them");
    assert!(!held(&d, "account/sneaky.toml").contains("connector"), "nothing was written");
    // the flow `account` with their own words is the line they typed
    let mut op = Operator::sit(&world);
    op.type_line(":account add lab typed signin=no");
    wait_for(&mut op, &world, "the row", || events(&world, "edit.applied").len() == 1);
    assert!(held(&d, "account/typed.toml").contains("connector = \"lab\""));
    world.close();
}

/// The same where the flow is `account` itself, changed by a row of the operator's to build its goal: the root is the operator's, and the goal is not
/// their words.
#[test]
fn the_flow_account_with_a_goal_it_builds_is_a_proposal_too() {
    use j4n_core::change::Actor;
    use j4n_core::task::TaskSpec;
    let d = Dirs::new("acct-templated-own");
    let _lab = furnish(&d, "browser", true);
    put(
        &here_dir(&d.j4n_home(), &d.project()),
        "flows/account.toml",
        "api = 1\n[[flow]]\nid = \"account\"\ndoc = \"builds the goal\"\nin = { goal = \"text\" }\nsteps = [\n  { id = \"do\", do = \"account\", goal = \"add lab sneaky {in.goal}\", out = { done = \"text\" } },\n  { id = \"done\", kind = \"end\", answer = \"{steps.do.answer}\" },\n]\n",
    );
    let world = open(&d);
    crate::screen_drive::wait_idle(&world);
    world.front.tasks().spawn(TaskSpec::new("signin=no", "flow:account"), Actor::Operator).unwrap();
    crate::screen_drive::wait_idle(&world);
    assert_eq!(events(&world, "proposal.made").len(), 1, "{:?}", world.notices);
    assert!(events(&world, "proposal.accepted").is_empty());
    assert!(!held(&d, "account/sneaky.toml").contains("connector"));
    world.close();
}

// ---- Connect › Add account, keys only ----

/// A login of the operator's own, as a harness keeps one where it always does: `mine` reads `$MINE_HOME`, else its own folder (as `claude`
/// reads `~/.claude` when no `CLAUDE_CONFIG_DIR` is given), and its probe asks whether it is signed in there.
fn furnish_mine(d: &Dirs) -> PathBuf {
    let bin = d.dir.join("bin");
    let own = d.dir.join("mine-home");
    let o = own.display();
    let whoami = script(&bin, "mine-whoami", &format!("[ -f \"${{MINE_HOME:-{o}}}/auth.json\" ]"));
    let status = script(&bin, "mine-status", &format!("if [ -f \"${{MINE_HOME:-{o}}}/auth.json\" ]; then echo '{{\"loggedIn\":true,\"email\":\"me@example.org\"}}'; else echo '{{\"loggedIn\":false}}'; fi"));
    let agent = script(&bin, "mine-agent", "echo mine");
    put(
        &here_dir(&d.j4n_home(), &d.project()),
        "connectors/mine.toml",
        &format!(
            "api = 1\n[[connector]]\nid = \"mine\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"{agent}\"\nstart = [\"{{prompt}}\"]\nlogin = [\"~/.mine\"]\nprobe = {{ argv = [\"{agent}\", \"--version\"], auth = [\"{whoami}\"] }}\n\n[connector.signin]\nargv = [\"{agent}\"]\nenv = {{ MINE_HOME = \"{{account.home}}\" }}\n\n[connector.signin.check]\nargv = [\"{status}\"]\nok = \"/loggedIn\"\nemail = \"/email\"\n",
            agent = agent.display(),
            whoami = whoami.display(),
            status = status.display()
        ),
    );
    put(&own, "auth.json", "{\"email\":\"me@example.org\"}");
    own
}

/// in a fresh HOME the usage refusal of `:account add <connector>` opens the form; the form is filled with the next free name, and
/// `⏎` on its add row adds the account and asks to sign it in on the card; the row turns signed in. A login already on this machine shows as
/// "mine · yours", and `u` names it as an account: no file of it is copied, no folder is made, and neither makes a Grow row.
#[test]
fn an_account_is_added_from_the_form_and_a_login_of_yours_is_named_with_no_grow_row() {
    use j4n_tui::headless::{KeyCode, press};
    let d = Dirs::new("accounts-account-added-from");
    let _lab = furnish(&d, "browser", true);
    let own = furnish_mine(&d);
    let world = open(&d);
    let mut op = Operator::sit(&world);
    crate::screen_drive::wait_idle(&world);
    // the connector only: the form, with the connector in it and the next free name
    op.type_line(":account add lab");
    op.until("the form", |s| s.lines().any(|l| l.contains("name") && l.contains("lab-1") && l.contains("the next free name")) && s.contains("add account"));
    assert!(events(&world, "task.queued").is_empty(), "the form is no task");
    // connector, name, plan, proxy, then the add row: its ⏎ is the operator's line, and the line is the approval
    op.press(vec![press(KeyCode::Down), press(KeyCode::Down), press(KeyCode::Down), press(KeyCode::Down)]);
    op.press(vec![press(KeyCode::Enter)]);
    // the card comes over the tab that is open
    op.until("the sign-in card", |s| s.contains("sign in account `lab-1`") && s.contains("y yes · n no"));
    assert!(held(&d, "account/lab-1.toml").contains("connector = \"lab\""), "{}", held(&d, "account/lab-1.toml"));
    op.press(vec![press(KeyCode::Char('y'))]);
    wait_for(&mut op, &world, "the sign-in", || events(&world, "account.signin").iter().any(|e| e["account"] == "lab-1" && e["ok"] == true));
    crate::screen_drive::wait_idle(&world);
    // the row of the account says where it stands (a line is typed in the Chat tab: a list has the keys of its rows)
    op.press(vec![press(KeyCode::F(1))]);
    op.type_line(":accounts");
    op.until("the row signed in", |s| s.lines().any(|l| l.contains("lab-1") && l.contains("signed in") && l.contains("system") && !l.contains("account lab-1")));
    // the login of `mine` on this machine is found by its own probe, apart from the screen's work, and `u` names it
    op.press(vec![press(KeyCode::F(1))]);
    op.type_line(":account add");
    op.until("the login found", |s| s.contains("mine · yours") && s.contains("found on this machine"));
    op.press((0..12).map(|_| press(KeyCode::Down)).collect());
    op.press(vec![press(KeyCode::Char('u'))]);
    wait_for(&mut op, &world, "the login named", || held(&d, "account/mine-1.toml").contains("keeps = \"yours\""));
    crate::screen_drive::wait_idle(&world);
    assert!(events(&world, "account.checked").iter().any(|e| e["account"] == "mine-1" && e["ok"] == true && e["email"] == "me@example.org"), "{:?}", events(&world, "account.checked"));
    op.until("the login named on its row", |s| s.contains("named mine-1"));
    // nothing copied: no folder of J4n's for it, and the login is where it was, as it was
    assert!(!accounts(&d).join("mine-1").exists(), "no folder is made for your own login");
    assert_eq!(fs::read_to_string(own.join("auth.json")).unwrap(), "{\"email\":\"me@example.org\"}");
    assert!(!held(&d, "account/mine-1.toml").contains("home"), "{}", held(&d, "account/mine-1.toml"));
    // a plan's price, in place on the plan rows: ⏎ opens the dollars editor of the first row (`claude`, which has none)
    op.press(vec![press(KeyCode::F(1))]);
    op.type_line(":plans");
    op.until("the plan rows", |s| s.contains("⏎ set price") && s.lines().any(|l| l.contains("claude") && l.contains("five_hour=5h")));
    op.press(vec![press(KeyCode::Enter)]);
    op.until_app("the dollars editor", |app| app.row_ed.is_some());
    op.press(j4n_tui::headless::typed("24", true));
    wait_for(&mut op, &world, "the price", || held(&d, "plan.toml").contains("price = \"$24\""));
    // three edits of the operator's, each a Change with an undo, and no Proposal: Grow holds only what J4n drafts
    assert_eq!((events(&world, "proposal.made").len(), events(&world, "edit.applied").len()), (0, 3), "edits, no Proposal");
    world.close();
}

#[test]
fn a_sign_in_that_waits_for_a_browser_has_a_limit_ends_its_own_process_and_says_how_to_finish() {
    let d = Dirs::new("acct-wait");
    let lab = furnish(&d, "browser", true);
    // the harness waits for the browser as `claude auth login` does: no end of its own
    let login = d.dir.join("bin/lab-login");
    fs::write(&login, format!("#!/bin/sh\necho $$ > '{}/pid'\necho waiting for the browser\nexec sleep 120\n", lab.out.display())).unwrap();
    let row = here_dir(&d.j4n_home(), &d.project()).join("connectors/lab.toml");
    fs::write(&row, fs::read_to_string(&row).unwrap().replace("page = ", "wait_secs = 1\npage = ")).unwrap();
    add(&d, ":account add lab work");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    let began = Instant::now();
    sign_in_until(&mut op, &world, "work", "is not signed in");
    assert!(began.elapsed() < Duration::from_secs(30), "the limit held");
    // the screen clips a long line: the words are read from the Log
    let said = world.front.log().read(0).map(|e| e.data.to_string()).filter(|t| t.contains("timed out after")).collect::<Vec<_>>().join("\n");
    assert!(said.contains("timed out after 1 s") && said.contains("nobody finished it here") && said.contains("on a machine with a screen run"), "{said}");
    assert!(said.contains("LAB_HOME=") && said.contains(":account check work"), "the way to finish names the folder and the check: {said}");
    assert_eq!(events(&world, "account.signin")[0]["ok"], false);
    // the harness's own process is gone with the wait
    let pid = lab.read("pid").trim().to_string();
    assert!(!pid.is_empty());
    let end = Instant::now() + crate::support::PATIENCE;
    while Path::new(&format!("/proc/{pid}")).exists() && !fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|t| t.contains(") Z ")) {
        assert!(Instant::now() < end, "process {pid} of the login is still running");
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(op);
    drop(world);
}

#[test]
fn with_no_browser_on_the_machine_the_sign_in_says_it_cannot_finish_here_and_how_to_finish() {
    let d = Dirs::new("acct-nobrowser");
    let lab = furnish(&d, "browser", true);
    fs::remove_file(d.dir.join("bin/fake-browser")).unwrap();
    add(&d, ":account add lab work");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    crate::screen_drive::wait_idle(&world);
    op.type_line(":account signin work");
    // the screen clips a long line: the words are read from the Log
    let said = || world.front.log().read(0).map(|e| e.data.to_string()).filter(|t| t.contains("cannot finish here")).collect::<Vec<_>>().join("\n");
    wait_for(&mut op, &world, "the refusal", || !said().is_empty());
    let s = said();
    assert!(s.contains("no browser for account windows") && s.contains("on a machine with a screen run") && s.contains("LAB_HOME="), "{s}");
    assert_eq!(lab.read("env.txt"), "", "the login never ran");
    drop(op);
    drop(world);
}
