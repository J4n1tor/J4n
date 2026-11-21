//! Accounts (pack `accounts`): several logins of one harness kept apart, each in a folder of its own, a browser
//! window of its own and, when it has one, a proxy of its own.
//!
//! - **The folder** `~/.j4n/accounts/<id>/` (0700, never read as a table): `browser/` is the account's Chromium data dir, `home/` the
//!   harness's config folder, `bin/` two shims (`open`, `xdg-open`) that open a link in the account's window. J4n never reads the
//!   browser's data.
//! - **The row** `[[account]]` names the connector whose login this is, a proxy (`system`, or a URL) and a remark. What J4n learns
//!   about an account (signed in, as whom, when it was last looked at) is events and the fold `accounts`, never the row.
//! - **The doer `account`** does what the operator asks of one: `add`, `signin`, `open`, `check`, `remove`. A sign-in runs the
//!   connector's `[connector.signin]` with the account's folder as its config and the shims first on its path, then asks the
//!   harness who it is signed in as; it is done only when that says so (law 16).
//! - **The launcher** gives a task that runs as an account the account's environment, its proxy and its folder as the login the
//!   place's walls leave open, and nothing of another account's.
//!
//! The browser is a connector (`is = "service"`) like the harnesses are, so another browser is a row, not code.

use crate::connect::{Job, Link, StepDoer, Work, clip, find};
use crate::manifest::{Connector, Connectors, Is, Signin, SigninCheck};
use crate::proc;
use j4n_core::change::{Actor, Where};
use j4n_core::doer::{Doer, RunCtl, RunEv};
use j4n_core::gate::Verb;
use j4n_core::log::{Log, New};
use j4n_core::schema::DataClass;
use j4n_core::place::Launch;
use j4n_core::proposal::{Draft, Edited, Step, StepOp};
use j4n_core::table::{Kind, Problem, Tables};
use j4n_core::task::Launcher;
use serde::Deserialize;
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

pub const DOER: &str = "account";
/// The connector that is the account's window.
pub const BROWSER: &str = "browser";
/// What the person is given to sign in, when the connector does not say.
const WAIT_MINUTES: u64 = 20;
/// How long the check of a login is given.
const CHECK_SECS: u64 = 20;
/// How long a proxy is given to accept a connection before a sign-in or a window is refused.
const PROXY_SECS: u64 = 3;
/// What the account's children are never to ask a proxy about.
const NO_PROXY: &str = "localhost,127.0.0.1,::1";
/// Files that hold a login: never linked from the operator's own folder, whatever a connector lists.
const NEVER_SHARED: [&str; 6] = [".credentials.json", ".claude.json", "auth.json", "credentials.json", "oauth_creds.json", "token.json"];

/// An entry of kind `account`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Account {
    pub id: String,
    #[serde(default)]
    pub doc: String,
    pub connector: String,
    #[serde(default)]
    pub remark: String,
    #[serde(default = "system")]
    pub proxy: String,
    #[serde(default = "window")]
    pub view: String,
    #[serde(default = "home")]
    pub keeps: String,
    /// The `plan` row the account's requests are paid by (`max5`, `pro`); none: the connector's own plan.
    #[serde(default)]
    pub plan: Option<String>,
}

fn system() -> String {
    "system".into()
}

fn window() -> String {
    "window".into()
}

fn home() -> String {
    "home".into()
}

/// What `keeps` says of the operator's own login on this machine, named as an account and never copied (`:account use`).
pub const YOURS: &str = "yours";

impl Account {
    /// The operator's own login, named: its config is where it always was, and J4n made no folder for it.
    pub fn is_yours(&self) -> bool {
        self.keeps == YOURS
    }
}

impl Kind for Account {
    const NAME: &'static str = "account";
}

fn id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.starts_with(|c: char| c.is_ascii_lowercase()) && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// The accounts there are, the connectors they are logins of, and where their folders are.
pub struct Book {
    root: PathBuf,
    user_home: PathBuf,
    /// The rows, read again when one is accepted or removed: [`Book::refresh`].
    rows: RwLock<Vec<Account>>,
    connectors: Connectors,
    /// What the system's proxy is, as a process was started with it.
    system_proxy: Option<String>,
    /// The tables as the files are now: where a row accepted a moment ago comes from. None: the rows are the ones `load` read.
    reload: Option<Arc<dyn Fn() -> Tables + Send + Sync>>,
    /// The folders the rows are written in (`account/` of the home and of the project's): the files read again only when one of them changed.
    watch: Vec<PathBuf>,
    seen: Mutex<Option<Stamp>>,
    /// What the last reading refused, as words: said once by the next `:account` line, and named when an account is not found.
    said: Mutex<Vec<String>>,
    trouble: Mutex<Vec<String>>,
}

/// What the files of the watched folders were: each one's name, its time of change and its size.
type Stamp = Vec<(PathBuf, std::time::SystemTime, u64)>;

/// The rows of kind `account` a table holds, and what is wrong with the ones that are left out.
fn read_rows(tables: &Tables) -> (Vec<Account>, Vec<Problem>) {
    let mut found = Vec::new();
    let mut rows = Vec::new();
    for id in tables.ids(Account::NAME) {
        match tables.get::<Account>(id) {
            Ok(a) if !id_ok(&a.id) => found.push(Problem::refused(format!("account {id}"), "an account's id is lower case letters, digits, `-` and `_`, and starts with a letter".to_string())),
            Ok(a) if crate::net::Proxy::parse(&a.proxy).is_err() => found.push(Problem::refused(format!("account {id}"), format!("`proxy = {}` is neither `system` nor a proxy URL like http://host:port", a.proxy))),
            Ok(a) => rows.push(a),
            Err(p) => found.push(p),
        }
    }
    (rows, found)
}

impl Book {
    /// `root` is `~/.j4n/accounts`; `user_home` the operator's own home, where a connector's login is when nobody signed in as an account.
    pub fn load(tables: &Tables, connectors: &Connectors, root: &Path, user_home: &Path) -> (Book, Vec<Problem>) {
        let (rows, found) = read_rows(tables);
        let system_proxy = ["https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()));
        (Book { root: root.to_path_buf(), user_home: user_home.to_path_buf(), rows: RwLock::new(rows), connectors: connectors.clone(), system_proxy, reload: None, watch: Vec::new(), seen: Mutex::new(None), said: Mutex::new(Vec::new()), trouble: Mutex::new(Vec::new()) }, found)
    }

    /// Where the rows are read again from when one is accepted, removed or missed: the tables as the files are now, and the folders
    /// the rows are in, which are looked at first: nothing is read again while none of their files changed.
    pub fn with_reload(mut self, reload: Arc<dyn Fn() -> Tables + Send + Sync>, watch: Vec<PathBuf>) -> Book {
        self.reload = Some(reload);
        self.watch = watch;
        *self.seen.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(self.stamp());
        self
    }

    fn stamp(&self) -> Stamp {
        let mut out = Vec::new();
        for dir in &self.watch {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                if let Ok(m) = entry.metadata() {
                    out.push((entry.path(), m.modified().unwrap_or(std::time::UNIX_EPOCH), m.len()));
                }
            }
        }
        out.sort();
        out
    }

    /// Read the rows again if a file of theirs changed since they were read. An accepted account is live at once, and one removed is gone.
    pub fn refresh(&self) {
        self.read_again(false);
    }

    /// Read them again whether or not a file changed (after an add or a remove, and where an account is missed).
    pub fn refresh_now(&self) {
        self.read_again(true);
    }

    /// A row that is refused (a hand edit, a file read while it is written) does not take the account away: the rows that were are kept, the
    /// ones that read well are taken, and the words of what was refused are kept to be said.
    fn read_again(&self, force: bool) {
        let Some(reload) = &self.reload else { return };
        let stamp = self.stamp();
        {
            let mut seen = self.seen.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if !force && !self.watch.is_empty() && seen.as_ref() == Some(&stamp) {
                return;
            }
            *seen = Some(stamp);
        }
        let (mut rows, problems) = read_rows(&reload());
        let words: Vec<String> = problems.iter().map(ToString::to_string).collect();
        if !problems.is_empty() {
            let old = self.rows();
            let kept: Vec<Account> = old.into_iter().filter(|o| !rows.iter().any(|r| r.id == o.id)).collect();
            rows.extend(kept);
            self.said.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone_from(&words);
        }
        *self.trouble.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = words;
        *self.rows.write().unwrap_or_else(std::sync::PoisonError::into_inner) = rows;
    }

    /// What a reading of the rows refused since this was last asked: for a notice.
    pub fn take_notes(&self) -> Vec<String> {
        std::mem::take(&mut *self.said.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
    }

    fn rows(&self) -> Vec<Account> {
        self.rows.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    /// The logins there are, `(connector, account)`, as the files say now.
    pub fn logins(&self) -> Vec<(String, String)> {
        self.refresh();
        self.rows().into_iter().map(|a| (a.connector, a.id)).collect()
    }

    /// The proxy that stands for `system`, in place of the one the process has (for a caller that knows better).
    pub fn with_system_proxy(mut self, proxy: Option<&str>) -> Book {
        self.system_proxy = proxy.map(str::to_string);
        self
    }

    /// The plan each account names, `(account, plan id)`, for the cost to read.
    pub fn plans(&self) -> Vec<(String, String)> {
        self.rows().into_iter().filter_map(|a| a.plan.clone().map(|p| (a.id.clone(), p))).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.rows().is_empty()
    }

    pub fn dir(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    /// The account and the connector it is a login of. An account that is not known is looked for once more in the files: a row accepted a
    /// moment ago is live.
    fn get(&self, id: &str) -> Result<(Account, &Connector), String> {
        let found = |book: &Book| book.rows().into_iter().find(|a| a.id == id);
        let a = match found(self) {
            Some(a) => a,
            None => {
                self.refresh_now();
                let trouble = self.trouble.lock().unwrap_or_else(std::sync::PoisonError::into_inner).first().cloned();
                found(self).ok_or_else(|| format!("no account called `{id}`: `account add <connector> <id>` makes one{}", trouble.map(|t| format!(" (a row of the files is refused: {t})")).unwrap_or_default()))?
            }
        };
        let c = self.connectors.get(&a.connector).ok_or_else(|| format!("account `{id}` is a login of `{}`, and there is no such connector", a.connector))?;
        Ok((a, c))
    }

    /// The proxy of an account as a URL: its own, else the system's, else none.
    fn proxy_of(&self, a: &Account) -> Option<String> {
        match a.proxy.as_str() {
            "system" | "" => self.system_proxy.clone(),
            url => Some(url.to_string()),
        }
    }

    /// What `{account.…}` stands for, and `{url}` when a link is going to be opened.
    fn vars(&self, a: &Account, email: Option<&str>, url: Option<&str>) -> Vec<(&'static str, String)> {
        let dir = self.dir(&a.id);
        let mut v = vec![
            ("account.id", a.id.clone()),
            ("account.dir", dir.display().to_string()),
            ("account.home", dir.join("home").display().to_string()),
            ("account.browser", dir.join("browser").display().to_string()),
            ("account.bin", dir.join("bin").display().to_string()),
            ("account.proxy", self.proxy_of(a).unwrap_or_default()),
            ("account.email", email.unwrap_or_default().to_string()),
        ];
        if let Some(u) = url {
            v.push(("url", u.to_string()));
        }
        v
    }

    /// The environment of an account's children: the connector's `signin.env`, else a home of the account's own; its proxy in the
    /// variables programs read, loopback excepted; nothing of another account's.
    pub fn env(&self, a: &Account, c: &Connector) -> BTreeMap<String, String> {
        let v = self.vars(a, None, None);
        let mut env: BTreeMap<String, String> = match c.signin.as_ref().filter(|s| !s.env.is_empty()) {
            // the operator's own login: the harness finds its config where it always does
            _ if a.is_yours() => BTreeMap::new(),
            Some(s) => s.env.iter().map(|(k, val)| (k.clone(), fill(val, &v))).collect(),
            None => {
                let home = self.dir(&a.id).join("home");
                [("HOME", home.clone()), ("XDG_CONFIG_HOME", home.join(".config")), ("XDG_DATA_HOME", home.join(".local/share")), ("XDG_STATE_HOME", home.join(".local/state")), ("XDG_CACHE_HOME", home.join(".cache"))]
                    .into_iter()
                    .map(|(k, p)| (k.to_string(), p.display().to_string()))
                    .collect()
            }
        };
        // a proxy the account names is the account's; the system's is whatever the process already carries
        if a.proxy != "system" && !a.proxy.is_empty() {
            for k in ["https_proxy", "HTTPS_PROXY", "http_proxy", "HTTP_PROXY", "all_proxy", "ALL_PROXY"] {
                env.insert(k.into(), a.proxy.clone());
            }
        }
        env.insert("no_proxy".into(), NO_PROXY.into());
        env.insert("NO_PROXY".into(), NO_PROXY.into());
        env
    }

    /// The folder of the account and what is in it, made if it was not there: private to the operator, with the two shims.
    pub fn ensure(&self, a: &Account, c: &Connector) -> Result<(), String> {
        let dir = self.dir(&a.id);
        for d in [&self.root, &dir, &dir.join("browser"), &dir.join("home"), &dir.join("bin")] {
            private_dir(d).map_err(|e| format!("cannot make {}: {e}", d.display()))?;
        }
        let shim = self.shim(a)?;
        for name in ["open", "xdg-open"] {
            let path = dir.join("bin").join(name);
            write_mode(&path, shim.as_bytes(), 0o700).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        }
        self.link_shared(a, c);
        Ok(())
    }

    /// Link in what the connector says to share, from where the operator's own login is kept (its first folder). A name that is not
    /// a plain file name, one that holds a login, one that is not there or is there already, is left alone.
    fn link_shared(&self, a: &Account, c: &Connector) {
        let (Some(s), Some(from)) = (c.signin.as_ref(), c.login.iter().map(|l| expand(l, &self.user_home)).find(|p| p.is_dir())) else { return };
        let home = self.dir(&a.id).join("home");
        for item in &s.share {
            if item.is_empty() || item.contains('/') || item == "." || item == ".." || NEVER_SHARED.contains(&item.as_str()) {
                continue;
            }
            let (src, dst) = (from.join(item), home.join(item));
            if src.exists() && dst.symlink_metadata().is_err() {
                let _ = std::os::unix::fs::symlink(&src, &dst);
            }
        }
        if c.signin.as_ref().is_none_or(|s| s.env.is_empty()) {
            let (src, dst) = (self.user_home.join(".gitconfig"), home.join(".gitconfig"));
            if src.exists() && dst.symlink_metadata().is_err() {
                let _ = std::os::unix::fs::symlink(&src, &dst);
            }
        }
    }

    /// The window of an account as a command line, with the link where `{url}` is, or `None` of the link at all.
    fn browser_argv(&self, a: &Account, url: &str) -> Result<Vec<String>, String> {
        let b = self.connectors.get(BROWSER).filter(|b| b.is == Is::Service).ok_or_else(|| format!("there is no `{BROWSER}` connector, so no account has a window (pack `accounts` carries one)"))?;
        // the real program is looked for first, so that a machine without a browser is told so before anything is started
        if !b.candidates.is_empty() {
            if b.found.is_none() {
                return Err(format!("no browser for account windows: none of Chromium, Google Chrome, Brave or Edge is on this machine (install one; Agents › Services › {BROWSER} lists the paths J4n looks for)"));
            }
        } else {
            let probe = b.probe.argv.first().map_or(b.program(), String::as_str);
            find(probe, Path::new("/")).map_err(|_| format!("no browser for account windows: `{probe}` is not on this machine (Agents › Services › {BROWSER} is the one J4n looks for)"))?;
        }
        let program = find(b.program(), Path::new("/"))?;
        let v = self.vars(a, None, Some(url));
        let mut argv = vec![program.display().to_string()];
        for arg in &b.start {
            // a proxy flag is left out where the account has no proxy
            if arg.contains("{account.proxy}") && self.proxy_of(a).is_none() {
                continue;
            }
            argv.push(fill(arg, &v));
        }
        Ok(argv)
    }

    /// The script the shims are: the window's command line with the link of the call where the link goes.
    fn shim(&self, a: &Account) -> Result<String, String> {
        const MARK: &str = "\u{1}url\u{1}";
        let argv = self.browser_argv(a, MARK)?;
        let words: Vec<String> = argv
            .iter()
            .map(|w| w.split(MARK).map(sh_quote).collect::<Vec<_>>().join("\"${1}\""))
            .collect();
        // what the harness hands `$BROWSER` is not checked by the harness: a word that is no http(s) link (`--proxy-server=direct://`) is refused here
        // and never reaches the browser, which would take it for a flag of its own (`--` stands before the link besides)
        const GUARD: &str = "case \"${1-}\" in http://*|https://*) ;; *) echo \"J4n: an account's window opens an http(s) link, and that is not one\" >&2; exit 1 ;; esac";
        Ok(format!("#!/bin/sh\n# J4n wrote this for the account `{}`: a link opens in its own window and no other.\n{GUARD}\nexec {}\n", a.id, words.join(" ")))
    }

    /// An account's proxy answers, or says why not.
    fn proxy_ready(&self, a: &Account) -> Result<(), String> {
        let proxy = crate::net::Proxy::parse(&a.proxy)?;
        proxy.answers(PROXY_SECS).map_err(|why| format!("account `{}`: {why}", a.id))
    }

    /// Who the log last said the account was signed in as.
    fn email(&self, log: &Log, id: &str) -> Option<String> {
        let from = log.head().saturating_sub(20_000);
        log.read(from).filter(|e| e.kind == "account.checked" && e.data.get("account").and_then(Json::as_str) == Some(id)).last().and_then(|e| e.data.get("email").and_then(Json::as_str).filter(|s| !s.is_empty()).map(str::to_string))
    }

    /// What a task that runs as an account is launched as (the engine puts it before the place's walls).
    pub fn launcher(self: &Arc<Book>) -> Launcher {
        let book = self.clone();
        Arc::new(move |spec, mut launch| {
            let id = spec.account.as_deref().ok_or("no account")?;
            let (a, c) = book.get(id)?;
            if c.id != spec.doer {
                return Err(format!("account `{id}` is a login of `{}`, and this task runs `{}`", c.id, spec.doer));
            }
            if !a.is_yours() && !book.dir(id).join("home").is_dir() {
                return Err(format!("account `{id}` has no folder yet: Agents › Accounts, s signs it in"));
            }
            book.proxy_ready(&a)?;
            // an account is a sign-in: a key J4n keeps, given to the program for its tasks of no account, would win over it
            for name in c.kept_names() {
                launch.env.remove(name);
            }
            launch.env.extend(book.env(&a, c));
            // the walls leave open the account's folder and no other login; the operator's own login is the one a task with no account has
            if !a.is_yours() {
                launch.login = vec![book.dir(id).join("home").display().to_string()];
            }
            Ok(launch)
        })
    }
}

/// `text` with every `{name}` of `vars` replaced.
fn fill(text: &str, vars: &[(&str, String)]) -> String {
    vars.iter().fold(text.to_string(), |t, (k, v)| t.replace(&format!("{{{k}}}"), v))
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn expand(path: &str, home: &Path) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(path),
    }
}

fn private_dir(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        return std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
    }
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(path)
}

fn write_mode(path: &Path, bytes: &[u8], mode: u32) -> std::io::Result<()> {
    let _ = std::fs::remove_file(path);
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(mode).open(path)?;
    f.write_all(bytes)
}

// ---- the doer ----

/// The doer `account`: `add`, `signin`, `open`, `check` and `remove`, as words (`signin work`).
pub fn doer(book: Arc<Book>, link: &Link, log: Log) -> Arc<dyn Doer> {
    let link = link.clone();
    let work: Work = Arc::new(move |job| run(job, &book, &link, &log));
    Arc::new(StepDoer { id: DOER, doc: "keeps accounts: add, sign in, open the window of, check and remove one", work })
}

fn run(job: &Job, book: &Arc<Book>, link: &Link, log: &Log) -> Result<String, String> {
    let words: Vec<&str> = job.goal.split_whitespace().collect();
    // the rows are the files' as they are now: one accepted since the last line is here
    book.refresh();
    for note in book.take_notes() {
        job.out().send(RunEv::Notice(format!("a row of the accounts is refused, and the accounts that were are kept: {note}")));
    }
    match words.as_slice() {
        ["add", connector, id, rest @ ..] => add(job, book, link, log, connector, id, rest),
        ["use", connector] => adopt(job, book, link, log, connector, None),
        ["use", connector, id] => adopt(job, book, link, log, connector, Some(id)),
        ["signin" | "sign-in", id] => signin(job, book, log, id),
        ["login", connector] => login(job, book, connector),
        ["open", id] => open(job, book, log, id, None),
        ["open", id, url] => open(job, book, log, id, Some(url)),
        ["check", id] => check(job, book, log, id),
        ["remove", id] => remove(job, book, link, log, id),
        _ => Err("say what to do with an account: `add <connector> <id> [proxy=<url>] [plan=<plan>] [remark]`, `use <connector> [<id>]`, `signin <id>`, `login <connector>`, `open <id> [link]`, `check <id>` or `remove <id>`".into()),
    }
}

fn add(job: &Job, book: &Book, link: &Link, log: &Log, connector: &str, id: &str, rest: &[&str]) -> Result<String, String> {
    if !id_ok(id) {
        return Err(format!("`{id}` is not an id: lower case letters, digits, `-` and `_`, starting with a letter"));
    }
    if book.rows().iter().any(|a| a.id == id) {
        return Err(format!("there is an account called `{id}` already"));
    }
    let c = book.connectors.get(connector).ok_or_else(|| format!("there is no connector called `{connector}`"))?;
    let (mut proxy, mut plan, mut remark, mut later) = ("system".to_string(), None, Vec::new(), false);
    for w in rest {
        match (w.strip_prefix("proxy="), w.strip_prefix("plan=")) {
            (Some(p), _) => proxy = p.to_string(),
            (_, Some(p)) if !p.is_empty() => plan = Some(p.to_string()),
            _ if *w == "signin=no" => later = true,
            _ => remark.push(*w),
        }
    }
    crate::net::Proxy::parse(&proxy)?;
    let mut row = toml::Table::new();
    row.insert("connector".into(), connector.into());
    if !remark.is_empty() {
        row.insert("remark".into(), remark.join(" ").into());
    }
    if proxy != "system" {
        row.insert("proxy".into(), proxy.clone().into());
    }
    if let Some(p) = &plan {
        row.insert("plan".into(), p.clone().into());
    }
    let declared = c.signin.as_ref().is_some_and(|s| !s.argv.is_empty());
    let sign = if declared { "it can be signed in to" } else { "it declares no sign-in, so it runs with a home of its own and `check` asks its probe" };
    // the operator's own line is the approval: it is written at once, and the sign-in goes on from it. What a machine adds is a proposal
    let mine = by_operator(log, job);
    let draft = Draft {
        // an account that was removed and is added again is a new proposal, not the one that was accepted
        key: format!("account:{id}:{}", removals(log, id)),
        title: format!("Add account {id} ({connector})"),
        why: format!("A login of `{connector}` kept in a folder of its own, with a window of its own and {}; {sign}.", if proxy == "system" { "the system's proxy".to_string() } else { format!("the proxy {}", redacted(&proxy)) }),
        evidence: 1,
        steps: vec![Step { to: Where::User, file: Some(format!("account/{id}.toml")), kind: "account".into(), id: id.to_string(), op: StepOp::Set(row) }],
        widens: Vec::new(),
    };
    let proposer = link.get()?;
    // what a machine asks for is a proposal of J4n's own, for the operator's Tab (law 12)
    if !mine {
        let made = proposer.make(draft, Actor::Core, None).map_err(|p| p.msg)?;
        let Some(p) = made else { return Ok(format!("account {id} was proposed before and left; nothing new was proposed.")) };
        return Ok(format!("proposed {p}: add account {id}. `:accept {p}` to add it, then `:account signin {id}`."));
    }
    // the operator's own line is their edit: written at once, a Change with an undo, never a row of Grow
    if let Edited::Proposed { id: p, .. } = proposer.edit(draft).map_err(|p| p.msg)? {
        return Ok(format!("proposed {p}: add account {id}. `:accept {p}` to add it, then `:account signin {id}`."));
    }
    book.refresh_now();
    if !declared {
        return Ok(format!("added account `{id}` ({connector}); {sign}."));
    }
    if later {
        return Ok(format!("added account `{id}` ({connector}); `:account signin {id}` signs it in."));
    }
    match signin(job, book, log, id) {
        Ok(done) => Ok(format!("added account `{id}` ({connector}). {done}")),
        Err(why) => Err(format!("added account `{id}` ({connector}), but {why}")),
    }
}

/// `use <connector> [<id>]` (Agents › Add account › found on this machine › `u`): the operator's own login of the connector, which a task
/// with no account runs on, named as an account. Nothing is copied and no folder is made: the row says `keeps = "yours"`, and the
/// harness is asked, as the operator, whether it is signed in before the row is written.
fn adopt(job: &Job, book: &Book, link: &Link, log: &Log, connector: &str, id: Option<&str>) -> Result<String, String> {
    let c = book.connectors.get(connector).ok_or_else(|| format!("there is no connector called `{connector}`"))?;
    if c.signin.is_none() {
        return Err(format!("`{connector}` keeps no accounts (it declares no `[connector.signin]`)"));
    }
    let rows = book.rows();
    if let Some(a) = rows.iter().find(|a| a.connector == connector && a.is_yours()) {
        return Err(format!("your own login of `{connector}` is account `{}` already", a.id));
    }
    let id = id.map_or_else(|| j4n_core::front::accounts::next_free(rows.iter().map(|a| a.id.as_str()), connector), str::to_string);
    if let Some(why) = j4n_core::front::accounts::name_trouble(&id, rows.iter().map(|a| a.id.as_str())) {
        return Err(why);
    }
    let a = Account { id: id.clone(), doc: String::new(), connector: connector.into(), remark: "your own login".into(), proxy: system(), view: window(), keeps: YOURS.into(), plan: None };
    let who = looked_at(book, &a, c, &c.signin.clone().unwrap_or_default(), book.env(&a, c), &job.cx.site.dir)?;
    if !who.ok {
        return Err(format!("`{connector}` is not signed in on this machine: Agents › Add account makes an account of its own and signs it in"));
    }
    let mut row = toml::Table::new();
    row.insert("connector".into(), connector.into());
    row.insert("keeps".into(), YOURS.into());
    row.insert("remark".into(), a.remark.clone().into());
    let draft = Draft {
        key: format!("account:{id}:{}", removals(log, &id)),
        title: format!("Name your own {connector} login {id}"),
        why: format!("The login of `{connector}` already on this machine, named `{id}`: nothing is copied, and a task with no account still runs on it."),
        evidence: 1,
        steps: vec![Step { to: Where::User, file: Some(format!("account/{id}.toml")), kind: "account".into(), id: id.clone(), op: StepOp::Set(row) }],
        widens: Vec::new(),
    };
    let proposer = link.get()?;
    if !by_operator(log, job) {
        let made = proposer.make(draft, Actor::Core, None).map_err(|p| p.msg)?;
        let Some(p) = made else { return Ok(format!("naming your own {connector} login {id} was proposed before and left; nothing new was proposed.")) };
        return Ok(format!("proposed {p}: name your own {connector} login {id}. `:accept {p}` names it."));
    }
    if let Edited::Proposed { id: p, .. } = proposer.edit(draft).map_err(|p| p.msg)? {
        return Ok(format!("proposed {p}: name your own {connector} login {id}. `:accept {p}` names it."));
    }
    book.refresh_now();
    note(log, job, "account.checked", json!({ "account": id, "ok": true, "email": who.email, "plan": who.plan, "state": "signed in" }));
    Ok(format!("account `{id}` is your own login of `{connector}`{}: J4n copied nothing, and a task with no account still runs on it.", who.email.as_deref().map(|e| format!(" ({e})")).unwrap_or_default()))
}

/// Whether this add is the operator's own typed line: the flow `account` was started by them (the root task was queued by the operator), and the
/// goal of this step is the goal of that flow, word for word. A flow of a pack that builds the goal from what a model said (`{steps.x.answer}`), or a
/// rule, a doer or a model that asks for the same, is not their line: it is a proposal.
fn by_operator(log: &Log, job: &Job) -> bool {
    log.sync();
    let from = log.head().saturating_sub(20_000);
    let queued: Vec<_> = log.read(from).filter(|e| e.kind == "task.queued").collect();
    let spec_of = |task: &str| queued.iter().rev().find(|e| e.task.as_deref() == Some(task)).map(|e| (e.by.clone(), e.data.get("spec").cloned().unwrap_or(Json::Null)));
    let Some((_, mine)) = spec_of(&job.cx.task) else { return false };
    let Some(parent) = mine.get("parent").and_then(Json::as_str) else { return false };
    let Some((by, root)) = spec_of(parent) else { return false };
    let text = |s: &Json, k: &str| s.get(k).and_then(Json::as_str).unwrap_or_default().trim().to_string();
    by == Actor::Operator && root.get("parent").is_none_or(Json::is_null) && text(&root, "doer") == "flow:account" && !text(&root, "goal").is_empty() && text(&root, "goal") == text(&mine, "goal")
}

/// How many times the log says the account was removed: what keeps the next proposal about its name from being the old one.
fn removals(log: &Log, id: &str) -> usize {
    log.read(0).filter(|e| e.kind == "account.removed" && e.data.get("account").and_then(Json::as_str) == Some(id)).count()
}

/// A proxy URL without the password it may carry.
fn redacted(url: &str) -> String {
    match url.split_once("://").and_then(|(s, r)| r.rsplit_once('@').map(|(_, host)| format!("{s}://{host}"))) {
        Some(clean) => clean,
        None => url.to_string(),
    }
}

fn note(log: &Log, job: &Job, kind: &str, mut data: Json) {
    data["at"] = json!(log.clock().now_ms());
    let _ = log.append(New::of(kind, Actor::Doer(DOER.into()), data).task(&job.cx.task).class(DataClass::Private));
}

fn signin(job: &Job, book: &Book, log: &Log, id: &str) -> Result<String, String> {
    let (a, c) = book.get(id)?;
    if a.is_yours() {
        return Err(format!("account `{id}` is your own login of `{}`: J4n does not sign it in or out; c checks it", c.id));
    }
    let s = c.signin.as_ref().filter(|s| !s.argv.is_empty()).ok_or_else(|| format!("`{}` declares no sign-in (`[connector.signin]`), so there is nothing to run; its account still has a folder of its own, and `check {id}` asks it who it is", c.id))?;
    let email = book.email(log, id);
    let v = book.vars(&a, email.as_deref(), None);
    let mut argv: Vec<String> = s.argv.iter().map(|w| fill(w, &v)).collect();
    if email.is_some() {
        argv.extend(s.again.iter().map(|w| fill(w, &v)));
    }
    let line = argv.join(" ");
    // before anything is started: the window can open, and the proxy answers. Without a window the sign-in cannot finish here, and the way to finish it is said
    book.browser_argv(&a, "https://example.invalid/").map_err(|why| format!("{why}; the sign-in cannot finish here: {}", by_hand(book, &a, &s.env, &line)))?;
    book.proxy_ready(&a)?;
    job.approve("signin", format!("sign in account `{id}`: run `{}` with the folder {} as its config, and open its window?", clip(&line, 120), book.dir(id).display()), &format!("account signin {id}: {line}"))?;
    book.ensure(&a, c)?;
    let mut env = book.env(&a, c);
    let bin = book.dir(id).join("bin");
    env.insert("BROWSER".into(), bin.join("open").display().to_string());
    env.insert("PATH".into(), format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()));
    // a login that fails on a missing program says so before the person waits for a window
    let program = find(argv.first().map_or("", String::as_str), &job.cx.site.dir)?;
    argv[0] = program.display().to_string();
    let wait = s.wait_secs.map_or_else(|| Duration::from_secs(s.wait.unwrap_or(WAIT_MINUTES) * 60), Duration::from_secs);
    let ended = run_login(job, argv, env.clone(), wait).map_err(|why| if why == "timed out" { format!("timed out after {}: it waited for a browser and nobody finished it here; {}", span(wait), by_hand(book, &a, &s.env, &line)) } else { why });
    let fail = |reason: String| {
        note(log, job, "account.signin", json!({ "account": id, "ok": false, "reason": reason, "state": "failed" }));
        Err(format!("account `{id}` is not signed in: {reason}"))
    };
    if let Err(why) = ended {
        return fail(why);
    }
    match looked_at(book, &a, c, s, env, &job.cx.site.dir) {
        Ok(who) if who.ok => {
            note(log, job, "account.signin", json!({ "account": id, "ok": true, "state": "signed in" }));
            note(log, job, "account.checked", json!({ "account": id, "ok": true, "email": who.email, "plan": who.plan, "state": "signed in" }));
            Ok(format!("account `{id}` is signed in{}.", who.email.as_deref().map(|e| format!(" as {e}")).unwrap_or_default()))
        }
        Ok(_) => fail("the harness says it is not signed in".into()),
        Err(why) => fail(why),
    }
}

/// `login <connector>`: the program's own sign-in, in its own folder, as when nobody keeps accounts (Agents › Setup's `s`, no pack `accounts`).
/// The connector's `[connector.signin]` runs with the operator's own environment: a word or a variable of an account's (`{account.home}`,
/// `{account.email}`) is left out, so nothing points it at a folder of J4n's. Its link is shown as it comes; it is done when its check says so.
fn login(job: &Job, book: &Book, connector: &str) -> Result<String, String> {
    let c = book.connectors.get(connector).ok_or_else(|| format!("there is no program `{connector}` to sign in: Agents › Setup lists the agents"))?;
    let s = c.signin.as_ref().filter(|s| !s.argv.is_empty()).ok_or_else(|| format!("{connector} has no sign-in J4n can start: sign in to it its own way, then Agents › Setup › r looks again"))?;
    let mut argv: Vec<String> = s.argv.iter().filter(|w| !w.contains("{account.")).cloned().collect();
    job.approve("signin", format!("sign in to {connector} with its own login? J4n starts its sign-in, shows the link it gives and asks it who it is after"), &format!("signin {connector}: {}", argv.join(" ")))?;
    // a bare name stands for the program the connector says, where it says one by its path (as the agent itself is started)
    let own = |w: &mut String| if let Some(p) = c.program.as_deref().filter(|p| p.contains('/') && !w.contains('/')) { p.clone_into(w) };
    own(&mut argv[0]);
    let program = find(argv.first().map_or("", String::as_str), &job.cx.site.dir)?;
    argv[0] = program.display().to_string();
    let wait = s.wait_secs.map_or_else(|| Duration::from_secs(s.wait.unwrap_or(WAIT_MINUTES) * 60), Duration::from_secs);
    run_login(job, argv, BTreeMap::new(), wait).map_err(|why| format!("{connector} is not signed in: {}", if why == "timed out" { format!("nobody finished its sign-in in {}", span(wait)) } else { why }))?;
    let mut check = s.check.clone().filter(|k| !k.argv.is_empty() && !k.argv.iter().any(|w| w.contains("{account."))).unwrap_or_else(|| SigninCheck { argv: c.probe.auth.clone(), ..SigninCheck::default() });
    check.argv.first_mut().into_iter().for_each(own);
    match ask_who(&check, &[], BTreeMap::new(), &job.cx.site.dir) {
        Ok(who) if who.ok => Ok(format!("{connector} is signed in{}.", who.email.as_deref().map(|e| format!(" as {e}")).unwrap_or_default())),
        Ok(_) => Err(format!("{connector} is not signed in: it says so after its sign-in ended")),
        Err(why) => Err(format!("{connector} is not signed in: {why}")),
    }
}

/// How long, in words: `20 minutes`, `1 s`.
fn span(d: Duration) -> String {
    if d.as_secs() >= 120 { format!("{} minutes", d.as_secs() / 60) } else { format!("{} s", d.as_secs()) }
}

/// How an operator finishes a sign-in J4n could not: the same line, run by hand where there is a screen, with the account's folder as its config.
fn by_hand(book: &Book, a: &Account, env: &BTreeMap<String, String>, line: &str) -> String {
    let v = book.vars(a, None, None);
    let set: Vec<String> = env.iter().map(|(k, w)| format!("{k}={}", fill(w, &v))).collect();
    format!("on a machine with a screen run `{}{line}` and say `:account check {}`", if set.is_empty() { String::new() } else { format!("{} ", set.join(" ")) }, a.id)
}

/// Run the login in its own process group, its words shown as they come, until it ends, the time is out, or the operator stops it. The process group ends
/// with J4n, whichever way J4n ends: held for the ends that run code, bound to the thread for the ones that do not.
fn run_login(job: &Job, argv: Vec<String>, env: BTreeMap<String, String>, wait: Duration) -> Result<(), String> {
    // both streams in one: the link may come on either
    let mut line = vec!["sh".to_string(), "-c".into(), "exec 2>&1; exec \"$@\"".into(), "sh".into()];
    line.extend(argv);
    let launch = Launch::new(line, job.cx.site.dir.clone(), env);
    let (tx, rx) = mpsc::channel();
    let out = job.out().clone();
    let (child, _) = proc::spawn_bound(&launch, false, move |l| { out.send(RunEv::Text(format!("{l}\n"))); }, move |e| drop(tx.send(e))).map_err(|e| format!("cannot start the login: {e}"))?;
    proc::hold(&child);
    let ended = wait_login(job, &child, &rx, wait);
    proc::release(&child);
    ended
}

fn wait_login(job: &Job, child: &proc::Child, rx: &mpsc::Receiver<proc::Exit>, wait: Duration) -> Result<(), String> {
    let until = Instant::now() + wait;
    loop {
        match rx.try_recv() {
            Ok(e) if e.code == Some(0) => return Ok(()),
            Ok(e) => return Err(format!("the login ended: {}{}", e.how(), if e.tail.is_empty() { String::new() } else { format!(" ({})", clip(&e.tail, 200)) })),
            Err(mpsc::TryRecvError::Disconnected) => return Err("the login's end was lost".into()),
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if Instant::now() >= until {
            child.kill();
            return Err("timed out".into());
        }
        match job.rx.recv_timeout(Duration::from_millis(100)) {
            Ok(RunCtl::Abort | RunCtl::Kill) | Err(RecvTimeoutError::Disconnected) => {
                child.kill();
                return Err("stopped".into());
            }
            Ok(_) | Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

/// What the harness says about who an account is signed in as.
struct Who {
    ok: bool,
    email: Option<String>,
    plan: Option<String>,
}

/// Ask the harness, as the account: its `check`, else the connector's `probe.auth`.
fn looked_at(book: &Book, a: &Account, c: &Connector, s: &Signin, env: BTreeMap<String, String>, cwd: &Path) -> Result<Who, String> {
    let v = book.vars(a, None, None);
    let check = s.check.clone().filter(|k| !k.argv.is_empty()).unwrap_or_else(|| SigninCheck { argv: c.probe.auth.clone(), ..SigninCheck::default() });
    ask_who(&check, &v, env, cwd)
}

fn ask_who(check: &SigninCheck, v: &[(&str, String)], env: BTreeMap<String, String>, cwd: &Path) -> Result<Who, String> {
    if check.argv.is_empty() {
        return Err("the connector has no check to ask who the account is, so it cannot be said to be signed in".into());
    }
    let argv: Vec<String> = check.argv.iter().map(|w| fill(w, v)).collect();
    let program = find(&argv[0], cwd)?;
    let mut line = vec![program.display().to_string()];
    line.extend(argv.into_iter().skip(1));
    let (text, exit) = proc::capture(&Launch::new(line, cwd.to_path_buf(), env), Duration::from_secs(CHECK_SECS)).map_err(|e| format!("cannot ask: {e}"))?;
    if exit.killed {
        return Err("timed out asking who it is signed in as".into());
    }
    let said: Option<Json> = serde_json::from_str(text.trim()).ok();
    let word = |p: &Option<String>| p.as_deref().and_then(|p| said.as_ref()?.pointer(p)).and_then(|v| v.as_str().filter(|s| !s.is_empty()).map(str::to_string));
    let ok = match &check.ok {
        Some(p) => said.as_ref().and_then(|s| s.pointer(p)).is_some_and(|v| v.as_bool().unwrap_or_else(|| !v.is_null() && v.as_str().is_none_or(|s| !s.is_empty()))),
        None => exit.code == Some(0),
    };
    Ok(Who { ok, email: word(&check.email), plan: word(&check.plan) })
}

fn check(job: &Job, book: &Book, log: &Log, id: &str) -> Result<String, String> {
    let (a, c) = book.get(id)?;
    if !a.is_yours() && !book.dir(id).join("home").is_dir() {
        return Err(format!("account `{id}` has no folder yet: Agents › Accounts, s signs it in"));
    }
    book.proxy_ready(&a)?;
    let s = c.signin.clone().unwrap_or_default();
    let who = looked_at(book, &a, c, &s, book.env(&a, c), &job.cx.site.dir)?;
    let state = if who.ok { "signed in" } else { "needs sign-in" };
    note(log, job, "account.checked", json!({ "account": id, "ok": who.ok, "email": who.email, "plan": who.plan, "state": state }));
    Ok(if who.ok { format!("account `{id}` is signed in{}.", who.email.as_deref().map(|e| format!(" as {e}")).unwrap_or_default()) } else { format!("account `{id}` is not signed in: `:account signin {id}`.") })
}

fn open(job: &Job, book: &Book, log: &Log, id: &str, url: Option<&str>) -> Result<String, String> {
    let (a, c) = book.get(id)?;
    let page = url.map(str::to_string).or_else(|| c.signin.as_ref().and_then(|s| s.page.clone())).ok_or("this connector names no page for the window: give a link, `open <id> <link>`")?;
    if !(page.starts_with("https://") || page.starts_with("http://")) || page.chars().any(|ch| ch.is_whitespace() || ch.is_control()) {
        return Err(format!("`{}` is not a link that starts with https:// or http://", clip(&page, 80)));
    }
    book.proxy_ready(&a)?;
    book.ensure(&a, c)?;
    let argv = book.browser_argv(&a, &page)?;
    let launch = Launch::new(argv, job.cx.site.dir.clone(), BTreeMap::new());
    let (_, exit) = proc::capture(&launch, Duration::from_secs(10)).map_err(|e| format!("cannot open the window: {e}"))?;
    if exit.code != Some(0) {
        return Err(format!("the window did not open: {}", exit.how()));
    }
    note(log, job, "account.opened", json!({ "account": id, "page": page }));
    Ok(format!("opened the window of account `{id}` at {page}"))
}

fn remove(job: &Job, book: &Book, link: &Link, log: &Log, id: &str) -> Result<String, String> {
    let (a, _) = book.get(id)?;
    let dir = book.dir(&a.id);
    // asked every time: a write outside the project is not remembered, and a model never answers an approval
    // the operator's own login is not J4n's to delete: only the row goes, and the window's folder J4n made, if it made one
    let what = if a.is_yours() { format!("remove `{id}`? The row goes; your own login stays as it is") } else { format!("remove `{id}`? Its folder goes for good: an undo brings back the row, not the folder") };
    job.approve_as(
        "remove",
        what,
        &dir.display().to_string(),
        Verb::Write,
        Some(false),
    )?;
    if dir.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(format!("{} is a link, and is not followed to be deleted", dir.display()));
    }
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("cannot delete {}: {e}", dir.display()))?;
    }
    let draft = Draft {
        key: format!("account-remove:{id}:{}", removals(log, id)),
        title: format!("Remove account {id}"),
        why: format!("account `{id}` was removed by the operator; its folder is gone and does not come back with this."),
        evidence: 1,
        steps: vec![Step { to: Where::User, file: Some(format!("account/{id}.toml")), kind: "account".into(), id: id.to_string(), op: StepOp::Remove }],
        widens: Vec::new(),
    };
    let proposer = link.get()?;
    // the operator's own remove is their edit: the row goes at once, `:undo` brings it back
    proposer.edit(draft).map_err(|p| p.msg)?;
    book.refresh_now();
    note(log, job, "account.removed", json!({ "account": id, "state": "removed" }));
    Ok(format!("account `{id}` is removed: its folder is deleted, and its row is out of the table (`:undo` brings the row back and not the folder)."))
}
