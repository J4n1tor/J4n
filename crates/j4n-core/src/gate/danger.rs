//! What a shell line is (its rules are rows in
//! `packs/base/gates/shell.toml`). [`danger`] answers *what the line is* and never fails; the rows decide what that means.
//!
//! The line is taken apart like a shell would, without running anything: quotes and backslashes are resolved (`g\it`,
//! `'git' push`, `p""ush` are `git push`), a line is cut at `;` `&&` `||` `|` `&` and newlines, `$(…)`, backticks,
//! `<(…)`, subshells, `{ … }` groups, `bash -c '…'`, `eval …`, here-documents to a shell and `find -exec …` are opened
//! and every command in them counts, and wrappers (`env`, `sudo`, `xargs`, `nohup`, `timeout`, `command`, …) are looked
//! through. A line that cannot be read (unclosed quote or substitution, a control character, a program that is only known
//! after expansion, nesting past [`NEST`]) is `unknown` and `compound`, never `safe`.
//!
//! `class` is the most dangerous class of any command in the line (the order of the table, top first; every Block class
//! is above every Ask class); [`Danger::classes`] has all of them. The class vocabulary:
//!
//! | class | the line … |
//! |---|---|
//! | `wipe` | wipes: `rm -r` of `/`, a system folder or home, `mkfs`, `dd` or a redirect onto a device (not bash's `/dev/tcp/HOST/PORT`: that opens a connection and is `write`, as `curl` is), recursive `chmod` of those, a fork bomb |
//! | `pipe-to-shell` | feeds a shell or interpreter from a pipe, or from a substitution that fetches (`curl … \| sh`, `sh -c "$(curl …)"`) |
//! | `secret` | does something with a credentials file or a login: a command that reads, prints, copies, uploads or sources one (`cat`, `cp`, `curl -d @…`, `source`, `grep`, `git diff`, an interpreter's own code), a redirect from or to one, a remote shell told to, `--env-file`, a line that prints a login variable (`echo $X_TOKEN`) |
//! | `handed` | gives a credentials file's name to a program that is not seen into, or as the value of a `NAME=` in front of one (`ENV_FILE=.env.production ./deploy.sh`, `ssh -i ~/.ssh/id_rsa`): no read that J4n can see, so an ask with the reason, never a block |
//! | `own` | names J4n's rules or trust (`.j4n/gates`, `.j4n/projects`, `gates.toml`, `j4n trust`) |
//! | `foreign` | names another project's state: its transcripts (`~/.claude/projects/<folder>`, the folder itself, a glob), its `.j4n` (Log, memory, folds), J4n's per-project folders of it (`~/.j4n/{projects,checkpoints,worktrees}/<key>`); the doer's own are not (`gate/foreign.rs`) |
//! | `git-internals` | writes under `.git/` (a redirect, `tee`, `cp`, `sed -i`, …) |
//! | `push` | pushes: `git push`, `git send-pack` (forced or not) |
//! | `unknown` | cannot be read (see above) |
//! | `elevate` | `sudo`, `doas`, `su`, `pkexec` |
//! | `delete` | `rm -r`/`-f` (not one whose every operand is below the task's own temporary folder: that is a `write`), `shred`, `truncate`, `find -delete` |
//! | `discard` | throws work away: `git reset --hard`, `git clean`, `checkout -f`, `branch -D`, `stash drop` |
//! | `publish` · `login` · `install` · `uninstall` | `cargo publish`, `npm publish`, `docker push` · `cargo login` · `cargo install`, `npm i -g` · `pacman -R`, `apt remove` |
//! | `prune` · `kill` · `power` · `service` · `disk` | `docker system prune` · `killall`, `pkill` · `reboot`, `shutdown` · `systemctl restart` · any other `dd` |
//! | `dump` · `config` | dumps the environment (`env`, `printenv`) · names J4n's own files |
//! | `safe` | every command is a reader, a build tool or a test runner (`cargo`, `pytest`, `python -m pytest\|unittest`, `npm\|pnpm\|yarn test`, `make test\|check`, `node --test`, `go test`; `python -c CODE` or `python -` with a here-document that `pyprobe` reads as a probe), and every redirect goes to a plain relative file |
//! | `read` | only reads, but is no plain reader: a lone `--version`/`-V`/`--help`/`-h` of a program on the `PATH`, `pip list`/`show`/`freeze`/`check`, a `python -c` snippet that `pyread` reads, `pmars`; no row allows it unasked |
//! | `write` | anything else: it may change files, the repo or the machine (also any program this file does not know) |
//!
//! `writes`: some command changes something; a read that only names a path is not a write (`cat x > y` writes, `cat x`
//! does not; `2>&1`, `>/dev/null` are not writes). `broad`: some command is a program that can run anything. `program`:
//! the first word of the first command, without path and `VAR=value`. Not seen through: aliases, functions and
//! scripts defined elsewhere, wrappers not listed here (`strace`, …), code inside `python -c` beyond what `pyread` knows, globs that expand to a
//! secret's name, symlinks, a script that reads a credentials file without being told its name (`grep -r KEY .` reads the `.env` inside).

use crate::eval::glob_match;
use crate::value::Value;
use std::collections::{BTreeMap, HashSet};

/// Whether a line runs a remote shell, and whether J4n can put its redaction on what it prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Remote {
    /// No `ssh` in it.
    #[default]
    No,
    /// A remote shell whose output comes back as the line's own output: the whole line can be run inside the relay.
    Via,
    /// A remote shell whose output goes to a file, a background job or a copied file, or a line that cannot be read: no wrapper sees it.
    Cannot,
}

impl Remote {
    pub fn word(self) -> &'static str {
        match self {
            Remote::No => "no",
            Remote::Via => "via",
            Remote::Cannot => "cannot",
        }
    }
}

/// What the matcher says of one line.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Danger {
    /// Some command is a remote shell (`ssh`, `scp`, `rsync`, `sftp`, `mosh`) and what it prints can or cannot go through the relay.
    pub remote: Remote,
    /// One word: the most dangerous class of the line (the table above).
    pub class: String,
    /// It changes something: a file, the repository, the machine.
    pub writes: bool,
    /// More than one command in the line (`;` `&&` `||` `|` `&`, a newline, `$(…)`, a backtick, a subshell, `bash -c`).
    pub compound: bool,
    /// The program is one that can run anything (`python3`, `sh`, `sudo`, `curl`, `env`, …).
    pub broad: bool,
    /// The first word of the first command, with any path and `VAR=value` prefix taken off.
    pub program: String,
    /// Every class that some command in the line has, most dangerous first (`safe` or `write` when it has none).
    pub classes: Vec<String>,
    /// Some command writes or deletes outside the task's project, its own temporary folder and what the operator gave access to; false
    /// when the matcher was not told the project (see [`Scope`]).
    pub outside: bool,
    /// The scratch folders this line makes below a system temporary folder (`mkdir /tmp/x` of a first folder that is not there yet): the Gate keeps them as the
    /// task's own once the line is allowed. Each is one exact path, never a prefix.
    pub made: Vec<String>,
    /// What the switch `approval.inside` reads of the line (`gate/inside.rs`); no rule and no class reads it.
    pub reach: Reach,
}

/// Of a line, what `approval.inside` needs to say that its effect stays inside the project: the commands that are neither readers, build tools
/// nor test runners and none of the writers that stay in the project, the Python an interpreter is given on the line, whether it deletes
/// anything that is not below a build folder, and whether it is unreadable only for invisible characters inside a quoted here-document of Python.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reach {
    /// For each command that is not read as staying inside, its program (or why).
    pub loose: Vec<String>,
    /// The code of each `python -c CODE` or `python -` with a quoted here-document that is not a probe, for `gate/pyedit.rs`.
    pub code: Vec<String>,
    /// Some `rm`, `rmdir` or `unlink` removes what is not below a build folder of the project ([`BUILD`]).
    pub deletes: bool,
    /// The line is `unknown` only because of invisible characters (a zero-width joiner of right-to-left text) inside the strings of a quoted
    /// here-document that Python is given: the shell hands that text over as it is.
    pub odd_in_docs: bool,
}

/// Where a task may change things: its project folder, its own temporary folder and the folders the operator granted J4n. All absolute but a
/// grant that starts with `~`. A line is `outside` when a command in it names a write or a delete target that none of them holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    pub site: Option<String>,
    pub tmp: Option<String>,
    pub access: Vec<String>,
    /// Folders under the system's temporary folder that the task's own lines made: absolute, each one exact path. What the Gate registered from earlier
    /// lines of the task.
    pub own: Vec<String>,
    /// Where the doer is: a word that names another project's state is `foreign`.
    pub state: super::foreign::Own,
}

/// The temporary folders of the system, as the components of their paths: a task makes its scratch folders below them.
const SYSTEM_TMPS: [&[&str]; 3] = [&["tmp"], &["var", "tmp"], &["dev", "shm"]];

/// How deep substitutions and nested scripts are opened. Past it the rest is not read: the line is `unknown`.
pub const NEST: usize = 32;
/// The classes, most dangerous first. Block classes are above Ask classes, so a line's class is what the rows block on.
const ORDER: [&str; 23] = [
    "wipe", "pipe-to-shell", "secret", "own", "foreign", "git-internals", "push", "unknown", "handed", "elevate", "delete", "discard", "publish", "login",
    "install", "uninstall", "prune", "kill", "power", "service", "disk", "dump", "config",
];
/// Classes of a line that only reads.
const QUIET: [&str; 4] = ["secret", "foreign", "config", "dump"];
/// Words that start a command without being the program, and words after which no command follows.
const LEAD: [&str; 9] = ["if", "then", "else", "elif", "while", "until", "do", "{", "!"];
const NONE: [&str; 8] = ["for", "select", "case", "esac", "fi", "done", "}", "function"];
/// Programs with no effect on files, the repository or the machine.
const READ: &str = "ls cat pwd cd stat jq realpath readlink basename dirname echo rg grep egrep fgrep find wc head tail man which du df \
    free ps sort uniq cut tr diff cmp tree file nl column od xxd strings sha256sum sha1sum md5sum b2sum printf true false test [ [[ \
    sleep date uname hostname whoami id uptime less more type tac rev paste comm join fold expand seq expr yes tput lsblk lscpu \
    export unset alias set shopt read : exit return break continue wait jobs history help whereis locale nproc getconf stty tty \
    groups who last ldd readelf nm objdump hexdump bat eza fd lsof pgrep journalctl sed awk";
/// The programs the gate takes for readers, by plain name: what another program's permission rules are made from, so the two never drift. The
/// words of `READ` that are shell builtins with no use as a program of their own, and the one-line shell syntax (`[`, `:`), are left out.
pub fn read_programs() -> Vec<&'static str> {
    const NOT_PROGRAMS: &str = "export unset alias set shopt read exit return break continue wait jobs history help type yes";
    READ.split_whitespace().filter(|p| p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') && !has_word(NOT_PROGRAMS, p)).collect()
}

/// The git verbs that only read (the match of [`git`] below): `git <verb>` is `safe`, with or without the global flags before it.
pub const GIT_READ: &str = "status log diff show blame grep ls-files ls-tree rev-parse rev-list describe shortlog cat-file diff-tree name-rev merge-base check-ignore check-attr";

/// The lines of a test runner or build tool that are `safe` (and `cargo`): each is a prefix a permission rule may allow, and a line that
/// goes on with more arguments is allowed by the same rule. `test_runner` and the `cargo` arm decide; a law keeps this list true to them.
pub const SAFE_RUNNERS: [&str; 23] = [
    "pytest", "py.test", "python -m pytest", "python3 -m pytest", "python -m unittest", "python3 -m unittest", "python3 -I -m pytest", "python3 -I -m unittest",
    "python -I -m pytest", "python -I -m unittest", "npm test", "npm run test", "pnpm test", "pnpm run test", "yarn test", "yarn run test", "make test",
    "make check", "node --test", "go test", "cargo test", "cargo build", "cargo check",
];

/// The forms of a reader that make it more than a reader, as a permission rule would deny them (a glob over the whole line), each with a line that has
/// the form: the gate does not take it for `safe` (the `sed`, `sort`, `awk`, `rg` and `find` arms of [`classify`]).
pub const REFUSED_FORMS: [(&str, &str); 15] = [
    ("sed -i*", "sed -i s/a/b/ f"), ("sed * -i*", "sed s/a/b/ -i f"), ("sed --in-place*", "sed --in-place s/a/b/ f"), ("sort -o*", "sort -o out in"),
    ("sort * -o*", "sort in -o out"), ("sort --output*", "sort --output=out in"), ("find * -delete*", "find . -delete"), ("find * -exec*", "find . -exec rm {} ;"),
    ("find * -execdir*", "find . -execdir rm {} ;"), ("find * -ok*", "find . -ok rm {} ;"), ("find * -fprint*", "find . -fprint out"), ("find * -fls*", "find . -fls out"),
    ("rg --pre*", "rg --pre cmd x"), ("awk *system*", "awk BEGIN{system(\"x\")}"), ("awk *getline*", "awk BEGIN{getline}"),
];

/// The paths that hold credentials, as the gate knows them (`/.aws/credentials`, `/.ssh/id_`, …): what another program's rules must not let a model read.
pub fn secret_paths() -> &'static [&'static str] {
    &SECRETS
}

const BROAD: &str = "sh bash zsh dash ksh fish csh tcsh ash env sudo doas su pkexec xargs curl wget eval exec nohup timeout nice ionice \
    setsid stdbuf watch ssh source . command builtin busybox parallel time flock taskset chrt";
const SHELLS: &str = "sh bash zsh dash ksh fish csh tcsh ash";
const INTERPRETERS: [&str; 8] = ["python", "perl", "ruby", "node", "php", "lua", "deno", "bun"];
/// Programs that read their input files and print; not vetted as plain readers, so a line of them is `read`.
const SOFT: &str = "pmars";
const SYSTEM: &str = "bin boot dev etc home lib lib32 lib64 media mnt opt proc root run sbin srv sys usr var";
/// A command of one of `programs` with a word of `verbs` among its first three plain arguments has the class.
const VERBS: [(&str, &str, &str); 11] = [
    ("npm pnpm yarn cargo", "publish unpublish deprecate yank owner", "publish"),
    ("twine gem docker podman helm", "upload push", "publish"),
    ("npm pnpm yarn docker podman cargo", "login adduser logout", "login"),
    ("gh", "auth", "login"),
    ("cargo go apt apt-get dnf yum zypper brew", "install", "install"),
    ("apt apt-get aptitude dnf yum zypper brew pip pip3 npm pnpm cargo", "remove purge autoremove erase uninstall", "uninstall"),
    ("docker podman", "prune", "prune"),
    ("systemctl service", "restart stop disable mask start enable kill isolate reload daemon-reload try-restart", "service"),
    ("systemctl", "poweroff reboot halt suspend hibernate kexec", "power"),
    ("killall pkill", "", "kill"),
    ("reboot shutdown poweroff halt", "", "power"),
];
/// Paths (and what is left of one after its `/`) that hold credentials. A `*` is any process of `/proc`, and such an entry is a glob.
const SECRETS: [&str; 27] = [
    "/.aws/credentials", "/.netrc", "/.git-credentials", "/.config/gh/", "/.npmrc", "/.cargo/credentials", "/.docker/config.json",
    "/.kube/config", "/.ssh/id_", "/.gnupg", "/id_rsa", "/id_ed25519", "/id_ecdsa", "/id_dsa", "/.claude/.credentials.json",
    "/.claude.json", "/.codex/auth.json", "/.pypirc", "/.j4n/secrets", "/.j4n/keys", "/.j4n/accounts", "/.j4n/guard.salt", "/proc/*/environ", "/proc/*/mem",
    // the registry of the live sessions of a Claude config folder (their titles, folders and the keys to message them) and the prompt history of every project
    "/.claude/sessions", "/.claude/daemon", "/.claude/history.jsonl",
];

#[derive(Default, Clone)]
struct Word {
    text: String,
    /// Holds a `$`, a substitution or an unquoted glob or brace: what it is is only known after the shell expands it.
    dynamic: bool,
    /// Holds a substitution that runs `curl` or `wget`.
    fetch: bool,
}

#[derive(Default)]
struct Cmd {
    words: Vec<Word>,
    redirs: Vec<(String, Word)>,
    /// Its standard input is a pipe.
    piped: bool,
    /// Inside a substitution: not the line's own first command.
    inner: bool,
    /// Here-document bodies and here-strings.
    docs: Vec<String>,
    /// For each here-document body, in order: the shell hands it to the program as it is written (a quoted delimiter, or no `$`, backtick or backslash in it).
    plain_docs: Vec<bool>,
    /// Here-documents waiting for their body: delimiter, quoted, tabs stripped.
    heredocs: Vec<(String, bool, bool)>,
}

struct Parsed {
    cmds: Vec<Cmd>,
    ok: bool,
    subs: bool,
    /// A command ends in a single `&`: it runs in the background.
    bg: bool,
}

struct P {
    c: Vec<char>,
    i: usize,
    cmds: Vec<Cmd>,
    ok: bool,
    subs: bool,
    bg: bool,
    waiting: Vec<usize>,
}

fn parse(text: &str, nest: usize) -> Parsed {
    let mut p = P::new(text);
    p.list(nest, false, false);
    p.ok &= p.waiting.is_empty();
    Parsed { cmds: p.cmds, ok: p.ok, subs: p.subs, bg: p.bg }
}

/// The expression of `$(( … ))` holds only digits, operators, parentheses and whitespace, and no two numbers side by side. A name, `$name`, `${name}` or `{}`
/// (the placeholder of `xargs -I{}`) is not: bash reads a variable's value as an expression too, and an array subscript in it (`a[$(cmd)]`) runs `cmd`.
fn arith_body(b: &[char]) -> bool {
    let (mut i, mut depth, mut operand) = (0, 0usize, false);
    while i < b.len() {
        let c = b[i];
        i += 1;
        match c {
            _ if c.is_whitespace() => {}
            '0'..='9' if operand => return false,
            '0'..='9' => {
                while b.get(i).is_some_and(char::is_ascii_digit) {
                    i += 1;
                }
                operand = true;
            }
            '+' | '-' | '*' | '/' | '%' | '<' | '>' | '=' | '!' | '&' | '|' | '^' | '~' | '?' | ':' | ',' => operand = false,
            '(' if !operand => depth += 1,
            ')' if depth > 0 => (depth, operand) = (depth - 1, true),
            _ => return false,
        }
    }
    depth == 0
}

fn fetcher(c: &Cmd) -> bool {
    matches!(unwrap(&c.words).prog.as_str(), "curl" | "wget" | "aria2c")
}

impl P {
    fn new(text: &str) -> P {
        P { c: text.chars().collect(), i: 0, cmds: Vec::new(), ok: true, subs: false, bg: false, waiting: Vec::new() }
    }

    fn at(&self, k: usize) -> Option<char> {
        self.c.get(self.i + k).copied()
    }

    fn eat(&mut self, op: &mut String, set: &str) -> bool {
        match self.at(0) {
            Some(c) if set.contains(c) => {
                op.push(c);
                self.i += 1;
                true
            }
            _ => false,
        }
    }

    /// Commands up to the end, or up to the `)` that closes a subshell. `fed`: standard input is a pipe for all of them.
    fn list(&mut self, nest: usize, close: bool, mut fed: bool) {
        let (mut cur, mut pipe) = (Cmd::default(), fed);
        while let Some(c) = self.at(0) {
            match c {
                ' ' | '\t' => self.i += 1,
                '#' => {
                    while self.at(0).is_some_and(|c| c != '\n') {
                        self.i += 1;
                    }
                }
                '\n' | ';' => {
                    self.i += 1;
                    fed |= self.push(&mut cur, pipe);
                    pipe = fed;
                    if c == '\n' {
                        self.bodies(nest);
                    }
                }
                '&' if self.at(1) != Some('>') => {
                    self.bg |= self.at(1) != Some('&');
                    self.i += 1 + usize::from(self.at(1) == Some('&'));
                    fed |= self.push(&mut cur, pipe);
                    pipe = fed;
                }
                '|' => {
                    let next = self.at(1);
                    self.i += 1 + usize::from(matches!(next, Some('|' | '&')));
                    fed |= self.push(&mut cur, pipe);
                    pipe = fed || next != Some('|');
                }
                ')' => {
                    self.i += 1;
                    fed |= self.push(&mut cur, pipe);
                    pipe = fed;
                    if close {
                        return;
                    }
                }
                '(' => {
                    let feeds = if cur.words.is_empty() && cur.redirs.is_empty() { pipe } else { fed };
                    fed |= self.push(&mut cur, pipe);
                    pipe = fed;
                    self.i += 1;
                    self.subs = true;
                    self.nested(nest, feeds);
                }
                _ if self.is_redir() => self.redirect(&mut cur, nest),
                _ => {
                    let w = self.word(nest);
                    cur.words.push(w);
                }
            }
        }
        self.push(&mut cur, pipe);
        self.ok &= !close;
    }

    /// Finish the command; true when it only opened a group that a pipe feeds (so the commands in the group are fed).
    fn push(&mut self, cur: &mut Cmd, pipe: bool) -> bool {
        let mut cmd = std::mem::take(cur);
        if cmd.words.is_empty() && cmd.redirs.is_empty() {
            return false;
        }
        let group = cmd.redirs.is_empty() && cmd.words.iter().all(|w| LEAD.contains(&w.text.as_str()) || NONE.contains(&w.text.as_str()));
        cmd.piped = pipe;
        if !cmd.heredocs.is_empty() {
            self.waiting.push(self.cmds.len());
        }
        self.cmds.push(cmd);
        group && pipe
    }

    /// After `(`: the commands of a subshell or substitution, up to its `)`.
    fn nested(&mut self, nest: usize, fed: bool) {
        if nest + 1 < NEST {
            return self.list(nest + 1, true, fed);
        }
        self.ok = false;
        let mut depth = 1;
        while let Some(c) = self.at(0) {
            self.i += 1;
            depth += i32::from(c == '(') - i32::from(c == ')');
            if depth == 0 {
                break;
            }
        }
    }

    /// At `(` of `$(`, `<(` or `>(`: the substitution is a word; its commands are commands of the line.
    fn sub(&mut self, w: &mut Word, nest: usize, fed: bool) {
        let (start, before) = (self.i, self.cmds.len());
        self.i += 1;
        (self.subs, w.dynamic) = (true, true);
        self.nested(nest, fed);
        w.fetch |= self.cmds[before..].iter().any(fetcher);
        self.cmds[before..].iter_mut().for_each(|c| c.inner = true);
        w.text.extend(self.c[start..self.i.min(self.c.len())].iter());
    }

    /// At the first `(` of `$((`: an arithmetic expansion whose expression is only numbers, operators and parentheses is a word, not a command.
    /// Anything else in it (a name, `$x`, `{}`, `$(`, a backtick, a quote) leaves it to the substitution it may be.
    fn arith(&mut self, w: &mut Word) -> bool {
        if self.at(1) != Some('(') {
            return false;
        }
        let (mut depth, mut j) = (0usize, self.i + 2);
        while j < self.c.len() && !(self.c[j] == ')' && depth == 0) {
            depth += usize::from(self.c[j] == '(');
            depth = depth.saturating_sub(usize::from(self.c[j] == ')'));
            j += 1;
        }
        if self.c.get(j + 1) != Some(&')') || !arith_body(&self.c[self.i + 2..j]) {
            return false;
        }
        w.dynamic = true;
        w.text.extend(self.c[self.i..j + 2].iter());
        self.i = j + 2;
        true
    }

    /// After a backtick: its body is parsed on its own.
    fn tick(&mut self, w: &mut Word, nest: usize) {
        let mut body = String::new();
        let mut closed = false;
        while let Some(c) = self.at(0) {
            self.i += 1;
            match c {
                '`' => {
                    closed = true;
                    break;
                }
                '\\' if matches!(self.at(0), Some('`' | '\\' | '$')) => {
                    body.push(self.c[self.i]);
                    self.i += 1;
                }
                _ => body.push(c),
            }
        }
        (self.subs, w.dynamic, self.ok) = (true, true, self.ok && closed);
        w.text.push_str(&format!("`{body}`"));
        if nest + 1 >= NEST {
            self.ok = false;
            return;
        }
        let q = parse(&body, nest + 1);
        w.fetch |= q.cmds.iter().any(fetcher);
        self.ok &= q.ok;
        self.cmds.extend(q.cmds.into_iter().map(|c| Cmd { inner: true, ..c }));
    }

    /// After `"` (or `${`): up to `stop`; a `stop` of NUL reads to the end, as a here-document body does.
    fn dq(&mut self, w: &mut Word, nest: usize, stop: char) {
        while let Some(c) = self.at(0) {
            self.i += 1;
            if c == stop {
                return;
            }
            match (c, self.at(0)) {
                ('\\', Some('\n')) => self.i += 1,
                ('\\', Some(n)) if n == stop || "$`\"\\".contains(n) => {
                    w.text.push(n);
                    self.i += 1;
                }
                ('`', _) => self.tick(w, nest),
                ('$', Some('(')) => {
                    w.text.push('$');
                    if !self.arith(w) {
                        self.sub(w, nest, false);
                    }
                }
                ('$', Some('{')) => {
                    self.i += 1;
                    self.brace(w, nest);
                }
                _ => {
                    w.dynamic |= c == '$';
                    w.text.push(c);
                }
            }
        }
        self.ok &= stop == '\0';
    }

    /// After `${`: up to the `}`; a default (`${x:-$(cmd)}`) runs.
    fn brace(&mut self, w: &mut Word, nest: usize) {
        w.text.push_str("${");
        w.dynamic = true;
        if nest + 1 >= NEST {
            (self.ok, self.i) = (false, self.c.len());
            return;
        }
        self.dq(w, nest + 1, '}');
        w.text.push('}');
    }

    /// After `'` (or `$'`): up to the closing quote, nothing resolved.
    fn quoted(&mut self, w: &mut Word, ansi: bool) {
        while let Some(c) = self.at(0) {
            self.i += 1;
            if c == '\'' {
                return;
            }
            w.text.push(c);
            if ansi && c == '\\' {
                w.dynamic = true;
                w.text.extend(self.at(0));
                self.i += 1;
            }
        }
        self.ok = false;
    }

    fn word(&mut self, nest: usize) -> Word {
        let mut w = Word::default();
        while let Some(c) = self.at(0) {
            match (c, self.at(1)) {
                (' ' | '\t' | '\n' | ';' | '&' | '|' | '(' | ')', _) => break,
                ('<' | '>', Some('(')) => {
                    w.text.push(c);
                    self.i += 1;
                    self.sub(&mut w, nest, c == '>');
                }
                ('<' | '>', _) => break,
                ('\\', Some('\n')) => self.i += 2,
                ('\\', n) => {
                    w.text.extend(n);
                    self.i += 2;
                }
                ('\'', _) => {
                    self.i += 1;
                    self.quoted(&mut w, false);
                }
                ('"', _) => {
                    self.i += 1;
                    self.dq(&mut w, nest, '"');
                }
                ('`', _) => {
                    self.i += 1;
                    self.tick(&mut w, nest);
                }
                ('$', Some('\'')) => {
                    self.i += 2;
                    self.quoted(&mut w, true);
                }
                ('$', Some('"')) => {
                    self.i += 2;
                    self.dq(&mut w, nest, '"');
                }
                ('$', Some('(')) => {
                    w.text.push('$');
                    self.i += 1;
                    if !self.arith(&mut w) {
                        self.sub(&mut w, nest, false);
                    }
                }
                ('$', Some('{')) => {
                    self.i += 2;
                    self.brace(&mut w, nest);
                }
                _ => {
                    w.dynamic |= matches!(c, '$' | '*' | '?' | '[' | '{');
                    w.text.push(c);
                    self.i += 1;
                }
            }
        }
        w
    }

    fn is_redir(&self) -> bool {
        let mut k = 0;
        while self.at(k).is_some_and(|c| c.is_ascii_digit()) {
            k += 1;
        }
        match self.at(k) {
            Some('<' | '>') => k > 0 || self.at(1) != Some('('),
            Some('&') => k == 0 && self.at(1) == Some('>'),
            _ => false,
        }
    }

    fn redirect(&mut self, cur: &mut Cmd, nest: usize) {
        let mut op = String::new();
        while let Some(c) = self.at(0).filter(char::is_ascii_digit) {
            op.push(c);
            self.i += 1;
        }
        self.eat(&mut op, "&");
        let first = self.at(0).unwrap_or('>');
        op.push(first);
        self.i += 1;
        if first == '>' {
            self.eat(&mut op, ">");
            self.eat(&mut op, "&|");
        } else if self.eat(&mut op, "<") {
            if !self.eat(&mut op, "<") {
                self.eat(&mut op, "-");
            }
        } else {
            self.eat(&mut op, "&>");
        }
        while matches!(self.at(0), Some(' ' | '\t')) {
            self.i += 1;
        }
        let start = self.i;
        let target = self.word(nest);
        let quoted = self.c[start..self.i].iter().any(|c| "'\"\\".contains(*c));
        match op.trim_start_matches(|c: char| c.is_ascii_digit()) {
            "<<" | "<<-" => cur.heredocs.push((target.text, quoted, op.ends_with('-'))),
            "<<<" => cur.docs.push(target.text),
            _ => cur.redirs.push((op, target)),
        }
    }

    /// At a newline: the bodies of the here-documents the line before it started.
    fn bodies(&mut self, nest: usize) {
        for idx in std::mem::take(&mut self.waiting) {
            for (delim, quoted, tabs) in std::mem::take(&mut self.cmds[idx].heredocs) {
                let (mut body, mut found) = (String::new(), false);
                while !found && self.i < self.c.len() {
                    let end = (self.i..self.c.len()).find(|k| self.c[*k] == '\n').unwrap_or(self.c.len());
                    let line: String = self.c[self.i..end].iter().collect();
                    self.i = (end + 1).min(self.c.len());
                    found = if tabs { line.trim_start_matches('\t') == delim } else { line == delim };
                    if !found {
                        body = body + &line + "\n";
                    }
                }
                self.ok &= found;
                if !quoted && nest + 1 < NEST {
                    // an unquoted body is expanded: its `$(…)` and backticks run
                    let mut q = P::new(&body);
                    q.dq(&mut Word::default(), nest + 1, '\0');
                    (self.subs, self.ok) = (self.subs | q.subs, self.ok && q.ok);
                    self.cmds.extend(q.cmds.into_iter().map(|c| Cmd { inner: true, ..c }));
                }
                self.cmds[idx].plain_docs.push(quoted || !body.contains(['$', '`', '\\']));
                self.cmds[idx].docs.push(body);
            }
        }
    }
}

/// A command with its wrappers looked through.
struct Un<'a> {
    prog: String,
    args: &'a [Word],
    /// Classes the wrappers added (`sudo`).
    pre: Vec<&'static str>,
    envs: Vec<String>,
    wrapped: bool,
    dynamic: bool,
    /// Text the command runs as a script (`env -S`, `watch`).
    script: Option<String>,
    /// The program was named with no path: the one on the `PATH`, not a file of the project.
    bare: bool,
}

fn has_word(list: &str, w: &str) -> bool {
    list.split(' ').any(|x| x == w)
}

fn base(s: &str) -> &str {
    s.rsplit('/').next().unwrap_or("")
}

/// `NAME` of `NAME=value`.
fn assign(s: &str) -> Option<&str> {
    let (n, _) = s.split_once('=')?;
    (n.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).then_some(n)
}

/// How many words of `rest` are flags, counting the argument of those in `takes`.
fn flags(rest: &[Word], takes: &[&str]) -> usize {
    let mut k = 0;
    while let Some(w) = rest.get(k).map(|w| w.text.as_str()).filter(|w| w.starts_with('-') && w.len() > 1) {
        if w == "--" {
            return k + 1;
        }
        k += if takes.contains(&w) { 2 } else { 1 };
    }
    k.min(rest.len())
}

fn unwrap(mut ws: &[Word]) -> Un<'_> {
    let mut u = Un { prog: String::new(), args: &[], pre: Vec::new(), envs: Vec::new(), wrapped: false, dynamic: false, script: None, bare: true };
    for _ in 0..16 {
        while let Some(w) = ws.first() {
            match assign(&w.text) {
                _ if LEAD.contains(&w.text.as_str()) => {}
                Some(n) => u.envs.push(n.to_string()),
                None => break,
            }
            ws = &ws[1..];
        }
        let Some(first) = ws.first() else { return u };
        if NONE.contains(&first.text.as_str()) {
            (u.prog, u.args) = (String::new(), &[]);
            return u;
        }
        let rest = &ws[1..];
        u.prog = base(&first.text).to_lowercase();
        u.bare = !first.text.contains('/');
        u.args = rest;
        u.dynamic = first.dynamic && !matches!(first.text.as_str(), "[" | "[[" | "{" | "}");
        let (takes, skip): (&[&str], usize) = match u.prog.as_str() {
            "sudo" | "doas" => (&["-u", "-g", "-C", "-D", "-h", "-p", "-r", "-t", "-T", "-U", "--user", "--group", "--chdir", "--host"], 0),
            "env" => (&["-u", "-C", "-S", "--unset", "--chdir", "--split-string"], 0),
            "timeout" => (&["-s", "-k", "--signal", "--kill-after"], 1),
            "nice" => (&["-n", "--adjustment"], 0),
            "ionice" => (&["-c", "-n", "-p", "-P", "-u"], 0),
            "stdbuf" => (&["-i", "-o", "-e"], 0),
            "xargs" => (&["-I", "-n", "-P", "-d", "-L", "-s", "-E", "-a", "-l"], 0),
            "exec" => (&["-a"], 0),
            "watch" => (&["-n", "-d"], 0),
            "flock" | "taskset" | "chrt" => (&[], 1),
            "command" | "builtin" | "nohup" | "setsid" | "chronic" | "time" => (&[], 0),
            _ => return u,
        };
        if u.dynamic {
            return u;
        }
        let k = flags(rest, takes);
        if u.prog == "command" && rest[..k].iter().any(|w| w.text == "-v" || w.text == "-V") {
            u.prog = "type".into();
            return u;
        }
        if u.prog == "env"
            && let Some(j) = rest[..k].iter().position(|w| w.text == "-S" || w.text == "--split-string")
        {
            u.script = rest.get(j + 1).map(|w| w.text.clone());
        }
        let mut k = k + skip;
        while u.prog == "env" && rest.get(k).is_some_and(|w| assign(&w.text).is_some()) {
            u.envs.extend(assign(&rest[k].text).map(str::to_string));
            k += 1;
        }
        if u.prog == "watch" {
            u.script = Some(rest[k.min(rest.len())..].iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "));
        }
        if k >= rest.len() || u.prog == "watch" {
            // nothing is run through it: `env` alone dumps the environment, `sudo` alone is `sudo`
            u.wrapped |= u.prog != "env";
            if u.prog == "env" {
                u.args = &[];
            }
            return u;
        }
        if matches!(u.prog.as_str(), "sudo" | "doas") {
            u.pre.push("elevate");
        }
        u.wrapped = true;
        ws = &rest[k..];
    }
    u.dynamic = true;
    u
}

#[derive(Default)]
struct Facts {
    classes: Vec<&'static str>,
    writes: bool,
    broad: bool,
    /// Some command is not a plain reader or build tool.
    bad: bool,
    /// Some command runs another command or script that is not among the line's commands.
    extra: bool,
    /// Some command only reads but is not a plain reader: the line is `read`, which no row allows unasked.
    soft: bool,
    /// The temporary folder the task has of its own, as an absolute path: a `rm` of what is inside it is no delete.
    tmp: Option<String>,
    /// The project folder and the folders granted: a target outside them, and what is not known, is `outside`.
    site: Option<String>,
    access: Vec<String>,
    /// The scratch folders the task made before (`Scope::own`), the ones this line makes, and the variables that hold a `mktemp -d` of this line.
    own: Vec<String>,
    /// Where the doer is: a word that names another project's state is `foreign`.
    state: super::foreign::Own,
    made: Vec<String>,
    own_vars: Vec<String>,
    outside: bool,
    /// Some word names a file of J4n's own folder: `config` when the line also writes.
    named_config: bool,
    /// An earlier `cd` of the line left them: a relative path is no longer the project's.
    cwd_out: bool,
    /// The variables the line gives a credentials file's name to (`F=.env`, `for F in .env`), lower case: `cat "$F"` reads it.
    assigned: Vec<String>,
    /// Some command is a remote shell, some command writes a file or a pipe-to-file (so its output is not the line's output), some command is a background job.
    remote: Remote,
    sinks: bool,
    bg: bool,
    reach: Reach,
}

/// `path`, an absolute path below a system temporary folder with no `..`, `$` or glob: the first folder below that temporary folder, when it is not there yet
/// (or is one of the task's own already). A `mkdir -p /tmp/a/b` makes `/tmp/a` the task's, whole; one below a folder that exists and is nobody's of the task's
/// is not.
fn new_below_system_tmp(path: &str, f: &Facts) -> Option<String> {
    if path.contains(['$', '`', '*', '?', '[', '{']) || path.split('/').any(|c| c == "..") {
        return None;
    }
    let parts = parts_of(path)?;
    let at = SYSTEM_TMPS.iter().find(|t| parts.len() > t.len() && parts.iter().zip(t.iter()).all(|(a, b)| a == b))?.len();
    let root = format!("/{}", parts[..=at].join("/"));
    (!std::path::Path::new(&root).exists() || f.own.iter().chain(&f.made).any(|o| *o == root)).then_some(root)
}

/// The variables a line gives to a folder it makes with `mktemp`: `d=$(mktemp -d TEMPLATE)` or the same in backticks. The path is the exact one `mktemp` made, new
/// and its own; it is never a prefix, so another program's `/tmp/tmp.*` or `/tmp/claude-*` is not the task's.
fn mktemp_vars(cmd: &str) -> Vec<String> {
    let mut vars = Vec::new();
    for (at, _) in cmd.match_indices("mktemp") {
        let tail = &cmd[at + 6..];
        let end = tail.find([')', '`', ';', '\n', '&', '|']).unwrap_or(tail.len());
        let template = tail[..end].split_whitespace().find(|w| w.starts_with('/'));
        // a template outside a system temporary folder, with a `$`, a glob or a `..` makes a folder nobody knows: not a folder of the task's
        if template.is_some_and(|t| t.contains(['$', '`', '*', '?']) || t.contains("..") || !SYSTEM_TMPS.iter().any(|s| t.starts_with(&format!("/{}/", s.join("/"))))) {
            continue;
        }
        // `NAME=$(mktemp …)` or `NAME=\`mktemp …\``
        let before = cmd[..at].trim_end().trim_end_matches(['(', '`', '$']).trim_end();
        if let Some(name) = before.strip_suffix('=').map(|b| b.rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).next().unwrap_or_default()).filter(|n| !n.is_empty()) {
            vars.push(name.to_string());
        }
    }
    vars
}

/// The components of an absolute path with `.` and `..` resolved by the words alone; `None` when it is not absolute.
fn parts_of(p: &str) -> Option<Vec<String>> {
    let rest = p.strip_prefix('/')?;
    let mut out: Vec<String> = Vec::new();
    for c in rest.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            c => out.push(c.to_string()),
        }
    }
    Some(out)
}

impl Facts {
    /// Does `text`, a place a command writes or deletes, lie outside the project, the task's tmp and the granted folders? What is not known (a
    /// `$`, a substitution, `~`, a path above the project, a relative path after a `cd` that left it) is outside. Without a project it is nothing.
    fn leaves(&self, text: &str) -> bool {
        let Some(site) = &self.site else { return false };
        if matches!(text, "/dev/null" | "/dev/stdout" | "/dev/stderr" | "/dev/tty") || text.starts_with("/dev/fd/") {
            return false;
        }
        if !text.starts_with(['/', '~', '$']) {
            let mut depth = 0i32;
            for c in text.split('/') {
                match c {
                    "" | "." => {}
                    ".." => depth -= 1,
                    _ => depth += 1,
                }
                if depth < 0 {
                    return true;
                }
            }
            return self.cwd_out || text.contains(['$', '`']);
        }
        if let Some(rest) = text.strip_prefix('$') {
            // `$d`, `${d}`, `$d/x` where `d` holds a folder this line made with `mktemp -d`
            let (name, after) = match rest.strip_prefix('{') {
                Some(r) => r.split_once('}').unwrap_or((r, "\u{0}")),
                None => rest.split_at(rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(rest.len())),
            };
            if self.own_vars.iter().any(|v| v == name) && (after.is_empty() || (after.starts_with('/') && !after.split('/').any(|c| c == ".." || c.contains(['$', '`'])))) {
                return false;
            }
        }
        if text.starts_with(['~', '$']) || text.contains('`') {
            return !self.access.iter().any(|a| a.starts_with(['~', '$']) && (text == a || text.starts_with(&format!("{}/", a.trim_end_matches('/')))));
        }
        // the part before a glob: what follows one may be anything, `..` included
        let end = text.find(['*', '?', '[', '{']).unwrap_or(text.len());
        if text[end..].contains("..") {
            return true;
        }
        let Some(p) = parts_of(&text[..end]) else { return true };
        let roots = std::iter::once(site).chain(self.tmp.as_ref()).chain(self.access.iter().filter(|a| a.starts_with('/'))).chain(self.own.iter().chain(&self.made));
        !roots.filter_map(|r| parts_of(r)).any(|r| p.len() >= r.len() && p[..r.len()] == r[..])
    }
}

fn invisible(c: char) -> bool {
    matches!(c, '\u{ad}' | '\u{61c}' | '\u{180e}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2064}'
        | '\u{2066}'..='\u{206f}' | '\u{feff}' | '\u{e0000}'..='\u{e007f}')
}

/// Short-flag clusters (`-rf`) that hold one of `any`.
fn short(t: &[&str], any: &[char]) -> bool {
    t.iter().any(|x| x.starts_with('-') && !x.starts_with("--") && x.chars().skip(1).any(|c| any.contains(&c)))
}

/// Flags named `f`, or `f=value`.
fn has(t: &[&str], f: &[&str]) -> bool {
    t.iter().any(|x| f.iter().any(|f| x == f || x.strip_prefix(f).is_some_and(|r| r.starts_with('='))))
}

/// `python`, `python3.12`, `node`: an interpreter by name, not a program that merely starts like one (`bunzip2`).
pub fn interpreter(p: &str) -> bool {
    INTERPRETERS.iter().any(|i| p.strip_prefix(i).is_some_and(|r| r.chars().all(|c| c.is_ascii_digit() || c == '.')))
}

/// A path, resolved without touching the disk: `/` or home as its base, then the folders. `~/..` is above home.
fn norm(p: &str) -> Option<Vec<&str>> {
    let (mut stack, rest) = if let Some(r) = p.strip_prefix('/') {
        (Vec::new(), r)
    } else {
        let r = p.strip_prefix("~").or_else(|| p.strip_prefix("$home"))?;
        (vec!["home", "~"], r.strip_prefix('/').or(r.is_empty().then_some(r))?)
    };
    for c in rest.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            c => stack.push(c),
        }
    }
    Some(stack)
}

/// The filesystem root, a system folder (with or without `/*`), home or a home folder: what `rm -r` must never be given.
fn wipe_path(p: &str) -> bool {
    let p = p.to_lowercase().replace("${home}", "$home");
    let Some(mut s) = norm(&p) else { return false };
    if s.last().is_some_and(|c| c.contains('*') && c.chars().all(|c| c == '*' || c == '.')) {
        s.pop();
    }
    s.is_empty() || (s.len() == 1 && has_word(SYSTEM, s[0])) || (s[0] == "home" && s.len() <= 2)
}

/// `Some(true)` for `/dev/null`, a terminal and the like; `Some(false)` for any other thing under `/dev`.
fn dev(p: &str) -> Option<bool> {
    let p = p.to_lowercase();
    let s = norm(&p).filter(|s| s.first() == Some(&"dev") && !socket(&p))?;
    Some(s.get(1).is_some_and(|d| has_word("null zero full random urandom tty stdin stdout stderr fd pts ptmx shm", d)))
}

/// Bash's `/dev/tcp/HOST/PORT` and `/dev/udp/…`: opening one opens a connection, it is no device.
fn socket(p: &str) -> bool {
    norm(&p.to_lowercase()).is_some_and(|s| s.first() == Some(&"dev") && matches!(s.get(1), Some(&("tcp" | "udp"))))
}

/// A thing the line writes to: a device is a wipe, `.git/` is git's own.
fn dest(f: &mut Facts, p: &str) {
    if dev(p) == Some(false) {
        f.classes.push("wipe");
    }
    if p.to_lowercase().split('/').any(|c| c == ".git") {
        f.classes.push("git-internals");
    }
}

/// Is a redirect target a plain file in the project?
fn plain(w: &Word) -> bool {
    !w.dynamic && !w.text.is_empty() && !w.text.starts_with(['/', '~', '-']) && !w.text.split('/').any(|c| c == "..")
}

/// Does `text` name a credentials file or folder by its name alone? The text is cut at anything that is no part of a path, and each piece is looked
/// at: `.env` and `.env.*` (not an example), a glob that starts like them, and the paths of [`SECRETS`]. What the word is the name *of* is not looked at
/// here: [`credentials`] says what the command does with it.
pub(crate) fn secret_name(text: &str) -> bool {
    let lc = text.to_lowercase();
    lc.split(|c: char| !(c.is_alphanumeric() || "/._-~$%+*?[]{}".contains(c))).any(|tok| {
        // a word with `..` names the path it comes to (`~/.j4n/log/../keys/x` is the key store)
        let tok = &if tok.contains("..") || tok.contains("//") { super::clean_path(tok) } else { tok.to_string() };
        let name = tok.rsplit('/').next().unwrap_or("");
        let hit = |n: &str| tok.contains(n) || tok.starts_with(&n[1..]);
        let example = ["example", "sample", "template", "dist", "defaults"].iter().any(|e| name.ends_with(e));
        name == ".env"
            || (name.starts_with(".env.") && !example)
            || (name.starts_with(".env") && name.contains(['*', '?', '[']))
            || SECRETS.iter().any(|n| hit(n))
            || SECRETS.iter().filter(|n| n.contains('*')).any(|n| glob_match(&format!("*{n}"), &format!("/{}", tok.trim_end_matches(|c: char| !c.is_alphanumeric()))))
    })
}

/// A `$NAME` or `${NAME}` in `text` whose name is a login: a token, a secret, a password, an API key.
fn login_var(text: &str) -> bool {
    let lc = text.to_lowercase();
    lc.split('$').skip(1).any(|r| {
        let r = r.strip_prefix('{').unwrap_or(r);
        let name: String = r.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        ["token", "secret", "password", "passwd", "api_key", "apikey", "private_key", "credential"].iter().any(|k| name.contains(k))
    })
}

/// What a word names of J4n's own: its rules and trust, its other files.
fn names(text: &str, f: &mut Facts) {
    // Another project's transcripts, its `.j4n`, J4n's folders of it. A word that only leaves a path out (`--exclude-dir=…`) names it to skip it
    if !excluded(text) && super::foreign::foreign_in(text, f.site.as_deref(), &f.state, !f.cwd_out) {
        f.classes.push("foreign");
    }
    let lc = text.to_lowercase();
    for tok in lc.split(|c: char| !(c.is_alphanumeric() || "/._-~$%+*?[]{}".contains(c))) {
        let name = tok.rsplit('/').next().unwrap_or("");
        if name == "gates.toml" {
            f.classes.push("own");
        }
        let comps: Vec<&str> = tok.split('/').collect();
        if let Some(k) = comps.iter().position(|c| *c == ".j4n") {
            // J4n's rules and trust are `own` for any line that names them; its other files are `config` only for a line that may change something: a line that
            // reads them, or leaves them out (`--exclude-dir=.j4n`), is no change to them
            if matches!(comps.get(k + 1), Some(&("gates" | "projects"))) {
                f.classes.push("own");
            } else {
                f.named_config = true;
            }
        }
    }
}

/// The programs that open what their operands name and show, copy, send or load what is in it: a reader, a copier, an uploader, an editor, a loader of
/// a file as a script (`source`). A credentials file given to one of these is read. A program that is not here is not seen into: it is asked about.
const OPENERS: &str = "cat tac nl head tail less more bat batcat rev od xxd hexdump strings grep egrep fgrep rg ag ack sed awk gawk mawk cut sort uniq wc diff cmp \
    comm join paste fold expand unexpand column tr jq yq base64 base32 openssl gpg gzip gunzip zcat bzip2 bunzip2 bzcat xz unxz xzcat zstd tar zip unzip 7z 7za \
    cp mv rsync scp sftp install dd ln curl wget nc ncat socat source . tee xargs vi vim nvim nano emacs view split csplit shuf md5sum sha1sum sha256sum \
    sha512sum b2sum cksum sum tac strace ltrace";
/// What a program does that runs the rest of its words as a command, there or somewhere else: its operands are read as a script for what they open.
const RUNS: &str = "ssh mosh kubectl docker podman nerdctl lxc chroot sshpass ctr";
/// The subcommands of `git` that print or archive the content of a file.
const GIT_SHOWS: &str = "diff show log blame grep cat-file annotate whatchanged format-patch difftool archive stash";
/// The flags of an interpreter that take code.
const CODE_FLAGS: &str = "-c -e -E -r -p --eval --print --command";

/// The programs that run something on another machine, or move files to and from one: their output is what the relay looks at.
const REMOTE: &str = "ssh mosh sshpass scp sftp rsync";

/// `host:path`, `user@host:path`: a place on another machine.
fn remote_path(x: &str) -> bool {
    x.split_once(':').is_some_and(|(host, _)| host.len() > 1 && !host.contains('/'))
}

/// What a remote program does with its output: a remote shell prints it; a copy from another machine writes a file nobody sees, and so does `sftp`.
fn remote_of(p: &str, t: &[&str]) -> Remote {
    let ops: Vec<&&str> = t.iter().filter(|x| !x.starts_with('-')).collect();
    let pulls = ops.len() > 1 && ops[..ops.len() - 1].iter().any(|x| remote_path(x));
    match p {
        "sftp" => Remote::Cannot,
        "scp" | "rsync" if !ops.iter().any(|x| remote_path(x)) => Remote::No,
        "scp" | "rsync" if pulls => Remote::Cannot,
        _ => Remote::Via,
    }
}

/// A word that leaves a file out (`--exclude=.env`, `-g '!.env'`): it names the file so that it is not read.
fn excluded(w: &str) -> bool {
    let lc = w.to_lowercase();
    lc.starts_with("--exclude") || lc.starts_with("--ignore") || lc.starts_with('!')
}

/// `NAME=value` words that give a variable a credentials file's name, and `for NAME in …` loops over one: the names, so that `cat "$NAME"` is a read of it.
fn note_vars(cmds: &[Cmd], f: &mut Facts) {
    for c in cmds {
        for w in &c.words {
            if let Some(n) = assign(&w.text).filter(|n| secret_name(&w.text[n.len() + 1..])) {
                f.assigned.push(n.to_lowercase());
            }
        }
        if c.words.first().is_some_and(|w| w.text == "for") && c.words.get(2).is_some_and(|w| w.text == "in") && c.words[3..].iter().any(|w| secret_name(&w.text)) {
            f.assigned.push(c.words[1].text.to_lowercase());
        }
    }
}

/// What the command does with the credentials files its words name: a read, copy, upload, load or print of one is `secret`, a block with no
/// grant; a redirect from or to one is. A name that is only handed over (a variable's value, an operand of a program that is not seen into) is no read, it is a
/// question: `1` when it is an operand, `2` when it is the value of a `NAME=` in front of the program. No list of allowed shapes: what the program does decides.
fn credentials(cmd: &Cmd, u: &Un, t: &[&str], nest: usize, f: &mut Facts) -> u8 {
    let p = u.prog.as_str();
    let mut secret = false;
    let opens = has_word(OPENERS, p);
    let mut named = 0;
    let code = |k: usize| u.args.get(k + 1).map(|w| w.text.as_str());
    for (k, w) in u.args.iter().enumerate() {
        if !secret_name(&w.text) || excluded(&w.text) {
            continue;
        }
        let lc = w.text.to_lowercase();
        let loads = lc.contains("env-file") || lc.contains("envfile") || k.checked_sub(1).is_some_and(|j| u.args[j].text.to_lowercase().contains("env-file"));
        if opens || loads {
            secret = true;
        } else {
            named = 1;
        }
    }
    if opens {
        let var = |w: &Word| {
            let lc = w.text.to_lowercase();
            f.assigned.iter().any(|n| lc.contains(&format!("${n}")) || lc.contains(&format!("${{{n}")))
        };
        secret |= u.args.iter().any(var);
    }
    // a program that lists names (`find -name .env`) opens them when it runs a command on what it finds
    if p == "find" && t.iter().any(|x| has_word("-exec -execdir -ok -okdir -fprint -fprint0 -fprintf", x)) && u.args.iter().any(|w| secret_name(&w.text)) {
        secret = true;
    }
    // `git diff .env`, `git log -p -- .env`: the verbs that print a file's content
    if p == "git" && t.iter().any(|x| has_word(GIT_SHOWS, x)) && u.args.iter().any(|w| secret_name(&w.text) && !excluded(&w.text)) {
        secret = true;
    }
    // an interpreter's own code, and the here-document it is given, name the file it opens
    if interpreter(p) || has_word(SHELLS, p) {
        let inline = t.iter().position(|x| has_word(CODE_FLAGS, x)).and_then(code);
        secret |= inline.is_some_and(secret_name) || cmd.docs.iter().any(|d| secret_name(d));
    }
    // a login variable that is printed
    secret |= has_word("echo printf print", p) && u.args.iter().any(|w| login_var(&w.text));
    // what a remote or contained shell is told to run is a script of its own
    if has_word(RUNS, p) && nest + 1 < NEST {
        for k in 0..u.args.len() {
            // a word after a flag may be the flag's value (`ssh -i KEY host`), not the start of the command
            if k > 0 && u.args[k - 1].text.starts_with('-') && u.args[k - 1].text != "--" {
                continue;
            }
            let rest = u.args[k..].iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
            let mut g = Facts::default();
            g.assigned.clone_from(&f.assigned);
            let q = parse(&rest, nest + 1);
            q.cmds.iter().for_each(|c| classify(c, nest + 1, &mut g));
            secret |= g.classes.contains(&"secret");
        }
    }
    // a word a wrapper took (`xargs -a .env`, `timeout 5 ./.env`) or the program itself: it is opened by what runs
    let before = cmd.words.len() - u.args.len();
    secret |= cmd.words[..before].iter().any(|w| assign(&w.text).is_none() && !LEAD.contains(&w.text.as_str()) && secret_name(&w.text));
    // the value of a `NAME=` in front of the program: it is handed over to what runs
    if cmd.words.iter().any(|w| assign(&w.text).is_some_and(|n| secret_name(&w.text[n.len() + 1..]))) {
        named = 2;
    }
    if secret {
        f.classes.push("secret");
    }
    named
}

/// Things a command writes to, by program.
fn dests<'a>(p: &str, t: &[&'a str]) -> Vec<&'a str> {
    let plain: Vec<&str> = t.iter().copied().filter(|x| !x.starts_with('-')).collect();
    match p {
        "tee" | "touch" | "truncate" | "shred" | "patch" => plain,
        "cp" | "mv" | "install" | "ln" | "rsync" => plain.last().copied().into_iter().collect(),
        "sed" if short(t, &['i']) || has(t, &["--in-place"]) => plain,
        "dd" => t.iter().filter_map(|x| x.strip_prefix("of=")).collect(),
        _ => Vec::new(),
    }
}

/// A script some command runs (`bash -c`, `eval`, a here-document to a shell): its commands count as the line's.
fn script(text: &str, nest: usize, f: &mut Facts) {
    f.extra = true;
    f.broad = true;
    if fork_bomb(text) {
        f.classes.push("wipe");
    }
    if nest + 1 >= NEST {
        (f.bad, f.writes) = (true, true);
        return f.classes.push("unknown");
    }
    let q = parse(text, nest + 1);
    if !q.ok {
        f.classes.push("unknown");
        f.bad = true;
    }
    note_vars(&q.cmds, f);
    f.bg |= q.bg;
    q.cmds.iter().for_each(|c| classify(c, nest + 1, f));
}

/// `name(){ name|name& }`, in any spelling of the name.
fn fork_bomb(text: &str) -> bool {
    let s: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let before = |k: usize| s[..k].chars().rev().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':').map(char::len_utf8).sum::<usize>();
    let defined: HashSet<&str> = s.match_indices("(){").map(|(k, _)| &s[k - before(k)..k]).collect();
    s.match_indices('|').any(|(k, _)| {
        let name = &s[k - before(k)..k];
        !name.is_empty() && defined.contains(name) && s[k + 1..].strip_prefix(name).is_some_and(|r| r.starts_with('&'))
    })
}

fn classify(cmd: &Cmd, nest: usize, f: &mut Facts) {
    for (op, t) in &cmd.redirs {
        // A socket is opened by a redirect either way, and what is sent leaves the machine: it is as `curl` is, not a read
        if socket(&t.text) {
            (f.writes, f.bad, f.broad) = (true, true, true);
            f.outside |= f.site.is_some();
            continue;
        }
        let dup = op.ends_with('&') && (t.text == "-" || (!t.text.is_empty() && t.text.chars().all(|c| c.is_ascii_digit())));
        if op.contains('>') && !dup && dev(&t.text) != Some(true) {
            f.sinks = true;
            f.writes = true;
            f.bad |= !plain(t);
            dest(f, &t.text);
            f.outside |= f.leaves(&t.text);
        }
    }
    cmd.words.iter().chain(cmd.redirs.iter().map(|r| &r.1)).for_each(|w| names(&w.text, f));
    // the login of J4n's own account is never put on a line, whatever the line does with it
    if cmd.words.iter().chain(cmd.redirs.iter().map(|r| &r.1)).any(|w| w.text.contains('$') && w.text.to_lowercase().contains("oauth_token")) {
        f.classes.push("secret");
    }
    // a redirect from or to a credentials file, on any command or on the end of a loop (`done < .env`); a here-document's word is not a file
    if cmd.redirs.iter().any(|(op, w)| !op.starts_with("<<") && secret_name(&w.text)) {
        f.classes.push("secret");
    }
    let u = unwrap(&cmd.words);
    let risky = "BASH_ENV ENV PAGER EDITOR VISUAL PATH SHELL PROMPT_COMMAND IFS RUSTC_WRAPPER NODE_OPTIONS";
    if u.envs.iter().any(|e| e.starts_with("GIT_") || e.starts_with("LD_") || has_word(risky, e)) {
        (f.broad, f.bad, f.writes) = (true, true, true);
    }
    f.classes.extend(u.pre.iter().copied());
    f.broad |= u.wrapped;
    if u.dynamic {
        (f.bad, f.writes) = (true, true);
        f.reach.loose.push("a program only known when the line runs".into());
        f.classes.push("unknown");
        // a program only known after expansion that is given a credentials file's name may be a reader: the name is a block, as it was
        if cmd.words.iter().any(|w| secret_name(&w.text) && !excluded(&w.text)) {
            f.classes.push("secret");
        }
        return;
    }
    if let Some(s) = &u.script {
        script(s, nest, f);
    }
    let p = u.prog.as_str();
    if p.is_empty() {
        return;
    }
    let t: Vec<&str> = u.args.iter().map(|w| w.text.as_str()).collect();
    let named = credentials(cmd, &u, &t, nest, f);
    if has_word(REMOTE, p) {
        f.remote = f.remote.max(remote_of(p, &t));
    }
    f.sinks |= p == "tee";
    let words: Vec<&str> = t.iter().copied().filter(|x| !x.starts_with('-')).take(3).collect();
    let (shell, interp) = (has_word(SHELLS, p), has_word(SHELLS, p) || interpreter(p));
    f.broad |= has_word(BROAD, p) || interpreter(p);
    let dash_c = t.iter().position(|x| x.starts_with('-') && !x.starts_with("--") && x.contains('c')).and_then(|k| t.get(k + 1));
    if let Some(s) = dash_c.filter(|_| shell || p == "su") {
        script(s, nest, f);
    }
    if p == "eval" {
        script(&t.join(" "), nest, f);
    }
    for d in cmd.docs.iter().filter(|_| shell) {
        script(d, nest, f);
    }
    let fetched = cmd.words.iter().chain(cmd.redirs.iter().map(|r| &r.1)).any(|w| w.fetch);
    let reads_stdin = !t.iter().any(|x| !x.starts_with('-') || *x == "-") || t.contains(&"-") || short(&t, &['s']);
    if (interp && cmd.piped && dash_c.is_none() && reads_stdin) || (fetched && (interp || has_word("eval source .", p))) {
        f.classes.push("pipe-to-shell");
    }
    for d in dests(p, &t) {
        dest(f, d);
    }
    // The places this command writes or deletes, against the project, and where it leaves the line's folder
    if f.site.is_some() {
        let plain: Vec<&str> = t.iter().copied().filter(|x| !x.starts_with('-') || *x == "-").collect();
        let targets: Vec<&str> = match p {
            "rm" | "rmdir" | "unlink" | "shred" | "srm" | "truncate" | "mkdir" | "touch" | "tee" | "mv" | "patch" => plain.clone(),
            // a template in a system temporary folder is the task's scratch; one anywhere else is a write there
            "mktemp" => plain.iter().copied().filter(|x| x.contains("..") || !SYSTEM_TMPS.iter().any(|s| x.starts_with(&format!("/{}/", s.join("/"))))).collect(),
            "chmod" | "chown" | "chgrp" => plain.iter().copied().skip(1).collect(),
            "cp" | "install" | "ln" | "rsync" => plain.last().copied().into_iter().collect(),
            "sed" if short(&t, &['i']) || has(&t, &["--in-place"]) => plain.iter().copied().skip(usize::from(!has(&t, &["-e", "-f"]))).collect(),
            "dd" => t.iter().filter_map(|x| x.strip_prefix("of=")).collect(),
            "find" if has(&t, &["-delete"]) => plain.iter().copied().take_while(|x| !x.starts_with(['(', '!'])).collect(),
            _ => Vec::new(),
        };
        // a link out of a place the words say is inside makes every later word about it wrong: a `ln -s` is asked about
        if p == "ln" && (short(&t, &['s']) || has(&t, &["--symbolic"])) {
            f.outside = true;
        }
        // a folder directly below the system's temporary folder that does not exist is one this line makes, and is the task's own
        if matches!(p, "mkdir") {
            for x in &plain {
                if let Some(root) = new_below_system_tmp(x, f) {
                    f.made.push(root);
                }
            }
        }
        let out = targets.iter().any(|x| f.leaves(x));
        f.outside |= out;
        if matches!(p, "cd" | "pushd" | "chdir") && plain.first().is_none_or(|d| *d == "-" || f.leaves(d)) {
            f.cwd_out = true;
        }
    }
    for (progs, verbs, class) in VERBS {
        if has_word(progs, p) && (verbs.is_empty() || words.iter().any(|w| has_word(verbs, w))) {
            f.classes.push(class);
        }
    }
    let rec = short(&t, &['r', 'R']) || has(&t, &["--recursive"]);
    let mut safe = has_word(READ, p);
    match p {
        "rm" => {
            // What is below the task's own temporary folder is the task's to remove
            let own = f.tmp.as_deref().is_some_and(|tmp| rm_inside(u.args, tmp));
            if (rec || short(&t, &['f']) || has(&t, &["--force"])) && !own {
                f.classes.push("delete");
            }
            if (rec && t.iter().any(|x| wipe_path(x))) || has(&t, &["--no-preserve-root"]) {
                f.classes.push("wipe");
            }
        }
        "shred" | "truncate" | "srm" => f.classes.push("delete"),
        "chmod" | "chown" | "chgrp" if rec && t.iter().any(|x| wipe_path(x)) => f.classes.push("wipe"),
        "dd" => f.classes.push("disk"),
        "su" | "pkexec" | "run0" | "sudo" | "doas" => f.classes.push("elevate"),
        "printenv" => f.classes.push("dump"),
        "env" if u.args.iter().all(|w| w.text.starts_with('-')) => f.classes.push("dump"),
        "export" | "set" if t.is_empty() || (p == "export" && has(&t, &["-p"])) => f.classes.push("dump"),
        "j4n" if t.iter().any(|x| has_word("trust untrust grant revoke accept", &x.to_lowercase())) => f.classes.push("own"),
        "j4n" => safe = t.iter().all(|x| has_word("status help version doctor --help --version -h -V", x)),
        "pacman" if short(&t, &['R']) => f.classes.push("uninstall"),
        "npm" | "pnpm" | "yarn" if (short(&t, &['g']) || has(&t, &["--global"])) && words.iter().any(|w| has_word("install i add", w)) => {
            f.classes.push("install")
        }
        _ if p.starts_with("mkfs") || has_word("mke2fs mkswap wipefs blkdiscard sfdisk sgdisk", p) => f.classes.push("wipe"),
        "git" => safe = git(u.args, &t, f),
        "cargo" => {
            safe = true;
            let sub = t.iter().find(|x| !x.starts_with(['-', '+'])).copied().unwrap_or("");
            let reads = "build b check c test t clippy doc d bench run r tree metadata search fetch version help locate-project pkgid verify-project deny outdated nextest";
            f.writes |= !(sub.is_empty() || has_word(reads, sub) || (sub == "fmt" && has(&t, &["--check"])));
        }
        "systemctl" => safe = words.first().is_some_and(|w| w.starts_with("list-") || w.starts_with("is-") || has_word("status show cat", w)),
        "sort" => safe = !short(&t, &['o']) && !has(&t, &["--output"]),
        "sed" => safe = !short(&t, &['i']) && !has(&t, &["--in-place"]),
        "awk" => {
            let prog = t.iter().find(|x| !x.starts_with('-')).copied().unwrap_or("");
            safe = !(prog.contains("system") || prog.contains("getline") || prog.split_once("print").is_some_and(|(_, r)| r.contains(['>', '|'])));
        }
        "rg" => safe = !has(&t, &["--pre"]),
        "man" => safe = !has(&t, &["-P", "--pager", "-H", "--html"]),
        "find" => {
            let exec = ["-exec", "-execdir", "-ok", "-okdir"];
            let mut k = 0;
            while k < t.len() {
                if exec.contains(&t[k]) {
                    let end = (k + 1..t.len()).find(|j| t[*j] == ";" || t[*j] == "+").unwrap_or(t.len());
                    (f.extra, f.broad) = (true, true);
                    if nest + 1 < NEST {
                        classify(&Cmd { words: u.args[k + 1..end].to_vec(), ..Cmd::default() }, nest + 1, f);
                    }
                    k = end;
                }
                k += 1;
            }
            if has(&t, &["-delete"]) {
                f.classes.push("delete");
                if t.iter().take_while(|x| !x.starts_with(['-', '(', '!'])).any(|x| wipe_path(x)) {
                    f.classes.push("wipe");
                }
            }
            safe = !has(&t, &["-delete", "-fprint", "-fprint0", "-fprintf", "-fls", "-exec", "-execdir", "-ok", "-okdir"]);
        }
        _ => {}
    }
    // A test runner runs the project's own code as `cargo test` does, and is as safe; its redirects and chained commands count
    safe |= u.bare && test_runner(p, &t);
    // A doer's inline Python that is a probe of the project is as safe as its tests; one that is not is what it was
    let probe = !safe && u.bare && !u.wrapped && inline_python(p, &t, u.args, cmd, f);
    // What only reads but is no vetted reader: a version check, a listing of installed packages, a snippet of Python that reads
    let soft = !safe && !probe && u.bare && !u.wrapped && soft_read(p, &t, u.args);
    let safe = (safe && !u.wrapped) || soft || probe;
    f.soft |= soft;
    f.bad |= !safe;
    f.writes |= !safe;
    if !safe {
        reach(p, &t, &u, cmd, f);
    }
    // a credentials file's name given to a program that is not seen into, or put in front of any program as a value: a question, never a silent block
    if named > 0 && (!safe || named == 2) {
        f.classes.push("handed");
    }
}

/// The folders a build or a test run makes and that are made again: what `approval.inside` lets a line remove (`target/`, `__pycache__/` …).
pub const BUILD: [&str; 16] = [
    "target", "build", "dist", "out", "node_modules", "__pycache__", ".pytest_cache", ".mypy_cache", ".ruff_cache", ".tox", ".nox", "htmlcov",
    "coverage", ".coverage", ".next", ".turbo",
];
/// Programs that run another program in the project's own environment: `uv run pytest`, `poetry run ruff check`.
const RUNNERS: &str = "uv poetry pipenv pdm hatch rye";
/// The flags of `uv run` (and its kin) that change nothing but how it runs.
const RUN_FLAGS: &str = "-q --quiet --frozen --locked --offline --no-sync";
/// Linters and formatters: they read the project, and the ones that fix write in it.
const LINTERS: &str = "ruff black isort mypy pyright flake8 pylint eslint prettier tsc biome rustfmt gofmt shellcheck";
/// The first word of the name of a script of the project that checks it (`scripts/verify.sh`, `./test-all`).
const CHECKS: &str = "verify check checks test tests lint fmt format typecheck";
/// A word in a script's name that says it reaches beyond the project: such a script is asked about whatever else its name says.
const FENCE: [&str; 11] = ["deploy", "release", "publish", "push", "ship", "prod", "upload", "install", "sync", "remote", "migrate"];

/// What `approval.inside` reads of a command that is not a reader, a build tool or a test runner: a writer that stays in the project (its
/// targets are judged by `outside`), `git add`/`mv`/`stash`, a test run or a linter under `uv run`, a linter, a script of the project whose name
/// says it checks, a remove of build folders, and the Python an interpreter is given on the line. Anything else is loose: the operator is asked.
fn reach(p: &str, t: &[&str], u: &Un, cmd: &Cmd, f: &mut Facts) {
    let operands: Vec<&Word> = u.args.iter().filter(|w| !w.text.starts_with('-')).collect();
    let ok = !u.wrapped
        && !u.dynamic
        && match p {
            "mkdir" | "touch" | "cp" | "mv" | "tee" | "sed" | "patch" => true,
            "rm" | "rmdir" | "unlink" => {
                let build = !operands.is_empty() && operands.iter().all(|w| !w.dynamic && build_path(&w.text));
                f.reach.deletes |= !build;
                build
            }
            "git" => git_inside(t),
            _ if has_word(RUNNERS, p) => run_inside(t),
            _ if has_word(LINTERS, p) => true,
            _ if interpreter(p) && p.starts_with("python") => match python_code(t, u.args, cmd) {
                Some(code) => {
                    code_names(&code, f);
                    f.reach.code.push(code);
                    true
                }
                None => false,
            },
            _ if !u.bare => cmd.words.iter().find(|w| !LEAD.contains(&w.text.as_str()) && assign(&w.text).is_none()).is_some_and(|w| project_check(w, f.site.as_deref())),
            _ => false,
        };
    if !ok {
        f.reach.loose.push(p.to_string());
    }
}

/// A relative path below a build folder, or the folder itself: no `..`, nothing the shell fills in but a glob at its end.
fn build_path(p: &str) -> bool {
    let p = p.strip_prefix("./").unwrap_or(p);
    let first = p.split('/').next().unwrap_or("");
    !p.starts_with(['/', '~']) && !p.contains(['$', '`']) && !p.split('/').any(|c| c == "..") && BUILD.contains(&first)
}

/// `git add`, `git mv`, `git stash` (but `drop` and `clear`, which are `discard`): changes of the project's own tree and index, and no flag before the
/// command (`-C` names another folder).
fn git_inside(t: &[&str]) -> bool {
    match t {
        ["add" | "mv", ..] => true,
        ["stash", rest @ ..] => rest.iter().all(|x| x.starts_with('-') || has_word("push pop apply save list show", x)),
        _ => false,
    }
}

/// `uv run [quiet flags] PROGRAM …` (or poetry's, pdm's …) where the program is a test runner or a linter.
fn run_inside(t: &[&str]) -> bool {
    let Some(rest) = t.strip_prefix(&["run"]) else { return false };
    let k = rest.iter().take_while(|x| has_word(RUN_FLAGS, x)).count();
    let Some((prog, args)) = rest[k..].split_first() else { return false };
    let prog = base(prog);
    !prog.starts_with('-') && (test_runner(prog, args) || has_word(LINTERS, prog))
}

/// A script of the project run by its path (`./scripts/verify.sh`): relative, inside the project by its real path where it exists, and named for a
/// check ([`CHECKS`]) with no word of [`FENCE`] in its name. What the script does is not read: its name and its place decide.
fn project_check(w: &Word, site: Option<&str>) -> bool {
    let path = w.text.as_str();
    if w.dynamic || path.starts_with(['/', '~']) || path.split('/').any(|c| c == "..") {
        return false;
    }
    let name = base(path).to_lowercase();
    let stem = name.split(['-', '_', '.']).next().unwrap_or("");
    if !has_word(CHECKS, stem) || FENCE.iter().any(|x| name.contains(x)) {
        return false;
    }
    let Some(site) = site else { return false };
    let full = format!("{}/{}", site.trim_end_matches('/'), path.strip_prefix("./").unwrap_or(path));
    !std::path::Path::new(&full).exists() || super::real_under(site, &full)
}

/// The code of `python -c CODE`, or of `python -` / `python` given one quoted here-document: what the shell hands over as written.
fn python_code(t: &[&str], args: &[Word], cmd: &Cmd) -> Option<String> {
    if args.iter().any(|a| a.dynamic) {
        return None;
    }
    let k = t.iter().take_while(|x| has_word("-u -B -E -I -s -S -O -OO -q", x)).count();
    match t.get(k) {
        Some(&"-c") => args.get(k + 1).map(|c| c.text.clone()),
        Some(&"-") | None if cmd.docs.len() == 1 && cmd.plain_docs == [true] => cmd.docs.first().cloned(),
        _ => None,
    }
}

/// Python on the line that names credentials, J4n's own files or another project's state is loose. Read apart from the line's own classes, which it
/// does not change.
fn code_names(code: &str, f: &mut Facts) {
    let mut g = Facts { site: f.site.clone(), state: f.state.clone(), ..Facts::default() };
    names(code, &mut g);
    if !g.classes.is_empty() || g.named_config || secret_name(code) || login_var(code) {
        f.reach.loose.push("Python that names credentials or J4n's own files".into());
    }
}

/// Is every invisible character of `cmd` (as `shaky` says) inside a quoted here-document given to Python, and inside a string of it?
fn odd_in_python_docs(cmd: &str, shaky: impl Fn(char) -> bool) -> bool {
    let q = parse(cmd, 0);
    let mut seen = 0;
    for c in &q.cmds {
        let u = unwrap(&c.words);
        if !(interpreter(&u.prog) && u.prog.starts_with("python")) {
            continue;
        }
        for (doc, plain) in c.docs.iter().zip(&c.plain_docs) {
            let n = doc.chars().filter(|ch| shaky(*ch)).count();
            if n > 0 && (!plain || super::pyprobe::scan(doc, &mut Vec::new()).is_none()) {
                return false;
            }
            seen += n;
        }
    }
    seen > 0 && seen == cmd.chars().filter(|ch| shaky(*ch)).count()
}

/// Is every operand of this `rm` strictly below `tmp`, by its words and, where the folders exist, by where they lead? An operand the shell fills in,
/// a relative one, one with `..`, `tmp` itself and anything outside are not.
fn rm_inside(args: &[Word], tmp: &str) -> bool {
    let (mut operands, mut past) = (Vec::new(), false);
    for w in args {
        if past || !w.text.starts_with('-') || w.text == "-" {
            operands.push(w);
        } else if w.text == "--" {
            past = true;
        }
    }
    !operands.is_empty() && operands.iter().all(|w| !w.dynamic && below(&w.text, tmp))
}

/// `path` is below `tmp`: both absolute, `tmp`'s folders are the first of `path`'s and more follow, no `..` in it; and the folder the path sits in exists
/// at this moment and leads to a place inside `tmp`'s own (a link that goes out is not inside), also for a path that ends in `/`, which is followed itself.
fn below(path: &str, tmp: &str) -> bool {
    let parts = |p: &str| -> Option<Vec<String>> {
        let rest = p.strip_prefix('/')?;
        let mut out = Vec::new();
        for c in rest.split('/') {
            match c {
                "" | "." => {}
                ".." => return None,
                c => out.push(c.to_string()),
            }
        }
        Some(out)
    };
    let (Some(p), Some(t)) = (parts(path), parts(tmp)) else { return false };
    if p.len() <= t.len() || p[..t.len()] != t[..] {
        return false;
    }
    let mut at = std::path::PathBuf::from("/").join(p.join("/"));
    if !path.ends_with('/') {
        at.pop();
    }
    // the folder it sits in (the path itself when it ends in `/`, which `rm` follows through a link) must exist now and lead inside `tmp`'s own: a folder
    // that is not there yet may be made a link by an earlier command of the line (`ln -s ~ tmp/a && rm -rf tmp/a/b`), so what is not there is not inside
    match (at.canonicalize(), std::path::Path::new(tmp).canonicalize()) {
        (Ok(real), Ok(root)) => real.starts_with(root),
        _ => false,
    }
}

/// `python -c CODE`, with the quiet flags before it, or `python -` / `python` with the code in a here-document that the shell hands over as written:
/// is the code a probe that [`pyprobe`] reads? The words it holds are looked at for credentials as the words of any line are, the strings
/// of it joined as well (a path built from pieces).
fn inline_python(p: &str, t: &[&str], args: &[Word], cmd: &Cmd, f: &mut Facts) -> bool {
    if !(interpreter(p) && p.starts_with("python")) || args.iter().any(|a| a.dynamic) {
        return false;
    }
    let k = t.iter().take_while(|x| has_word("-u -B -E -I -s -S -O -OO -q", x)).count();
    let code = match t.get(k) {
        Some(&"-c") => args.get(k + 1).map(|c| c.text.as_str()),
        Some(&"-") | None if cmd.docs.len() == 1 && cmd.plain_docs == [true] => cmd.docs.first().map(String::as_str),
        _ => None,
    };
    let Some(literals) = code.and_then(super::pyprobe::probe) else { return false };
    names(code.unwrap_or_default(), f);
    names(&literals, f);
    if secret_name(code.unwrap_or_default()) || secret_name(&literals) {
        f.classes.push("secret");
    }
    true
}

/// `pytest`, `python -m pytest|unittest`, `npm|pnpm|yarn test` (or `run test`), `make test|check`, `node --test` and `go test`: the
/// project's test runners, which `cargo test` is one of. `t` are the arguments. A flag that runs another program (`go test -exec`) or
/// another script (`node --test -e`) is not one, and neither is `make test` with a word beside the targets (`-f x.mk`, `CC=x`, `install`).
fn test_runner(p: &str, t: &[&str]) -> bool {
    let plain: Vec<&str> = t.iter().copied().filter(|x| !x.starts_with('-')).collect();
    match p {
        "pytest" | "py.test" => true,
        "npm" | "pnpm" | "yarn" => matches!(plain.as_slice(), ["test", ..] | ["run", "test", ..]),
        "make" => !plain.is_empty() && plain.iter().all(|w| has_word("test check", w)),
        "go" => plain.first() == Some(&"test") && !has(t, &["-exec", "-toolexec", "-o"]),
        "node" => t.first() == Some(&"--test") && !has(t, &["-e", "--eval", "-p", "--print"]),
        _ if interpreter(p) && p.starts_with("python") => {
            let k = t.iter().take_while(|x| has_word("-u -B -E -I -s -S -O -OO -q", x)).count();
            t.get(k) == Some(&"-m") && t.get(k + 1).is_some_and(|m| has_word("pytest unittest", m))
        }
        _ => false,
    }
}

/// A command that only reads, by what it is asked: a lone `--version`/`-V`/`--help`/`-h`, `pip list|show|freeze|check`, a
/// `python -c` snippet that [`pyread`] reads, and the programs of [`SOFT`]. `t` are its arguments.
fn soft_read(p: &str, t: &[&str], args: &[Word]) -> bool {
    if has_word(SOFT, p) {
        return true;
    }
    if t.len() == 1 && has_word("--version -V -version --help -h", t[0]) {
        return true;
    }
    let pip = |a: &[&str]| a.first().is_some_and(|s| has_word("list show freeze check search index inspect --version -V", s));
    if p.starts_with("pip") && p[3..].chars().all(|c| c.is_ascii_digit() || c == '.') {
        return pip(t);
    }
    if interpreter(p) && p.starts_with("python") {
        if t.len() >= 3 && t[0] == "-m" && t[1] == "pip" {
            return pip(&t[2..]);
        }
        // `python -c CODE`, with the quiet flags before it: the code must be text, not something the shell fills in
        let k = t.iter().take_while(|x| has_word("-u -B -E -I -s -S -O -OO -q", x)).count();
        return t.get(k) == Some(&"-c") && args.get(k + 1).is_some_and(|c| !c.dynamic && super::pyread::reads(&c.text));
    }
    false
}

/// The classes of a `git` command; true when it only reads.
fn git(args: &[Word], t: &[&str], f: &mut Facts) -> bool {
    let mut i = 0;
    while i < t.len() {
        match t[i] {
            "-C" | "--git-dir" | "--work-tree" | "--namespace" | "--super-prefix" | "--config-env" => i += 2,
            "-c" => {
                let key = t.get(i + 1).map_or(String::new(), |k| k.to_lowercase());
                if key.starts_with("alias.") || key.starts_with("help.") {
                    f.classes.push("unknown");
                }
                if ["core.", "filter.", "diff.", "credential.", "gpg.", "http.", "include", "url.", "protocol."].iter().any(|k| key.starts_with(k)) {
                    (f.broad, f.bad) = (true, true);
                }
                i += 2;
            }
            x if x.starts_with('-') => i += 1,
            _ => break,
        }
    }
    let Some(sub) = t.get(i).map(|s| s.to_lowercase()) else { return true };
    if args[i].dynamic {
        f.classes.push("unknown");
        return false;
    }
    let rest = &t[i + 1..];
    let (hard, force) = (has(rest, &["--hard"]), short(rest, &['f']) || has(rest, &["--force"]));
    let first = rest.first().copied().unwrap_or("");
    match sub.as_str() {
        "push" | "send-pack" => f.classes.push("push"),
        "reset" if hard => f.classes.push("discard"),
        "clean" if !(short(rest, &['n']) || has(rest, &["--dry-run"])) => f.classes.push("discard"),
        "checkout" | "switch" if force => f.classes.push("discard"),
        "branch" if short(rest, &['D']) => f.classes.push("discard"),
        "stash" if rest.iter().any(|x| *x == "drop" || *x == "clear") => f.classes.push("discard"),
        "filter-branch" | "filter-repo" => f.classes.push("discard"),
        "reflog" if rest.iter().any(|x| *x == "expire" || *x == "delete") => f.classes.push("discard"),
        "bisect" | "rebase" | "submodule" | "difftool" | "mergetool" | "instaweb" => f.broad = true,
        _ => {}
    }
    let listing = first == "list" || first == "show" || has(rest, &["--get", "--list", "-l", "--get-all"]);
    match sub.as_str() {
        s if has_word(GIT_READ, s) => !has(rest, &["--ext-diff", "--output"]),
        "branch" | "tag" | "remote" | "config" | "worktree" => {
            listing || (rest.iter().all(|x| x.starts_with('-')) && !short(rest, &['D', 'd', 'm', 'M']))
        }
        "stash" => listing,
        "clean" => short(rest, &['n']) || has(rest, &["--dry-run"]),
        _ => false,
    }
}

/// What the matcher says of `cmd`.
pub fn danger(cmd: &str) -> Danger {
    danger_in(cmd, None)
}

/// [`danger`] for a task that has a temporary folder of its own, `tmp`: a `rm` whose every operand is inside it, below it, is a `write`
/// and not a `delete`. Inside is read from the words: an absolute path with no `$`, glob, brace or `..`, below `tmp` by its folders, and, where the
/// folders exist, below it by where they lead (a link out of it is not inside). The folder itself, its parent and anything else are `delete`.
pub fn danger_in(cmd: &str, tmp: Option<&str>) -> Danger {
    danger_scoped(cmd, &Scope { tmp: tmp.map(String::from), ..Scope::default() })
}

/// [`danger`] for a task that is told where it may change things: [`Danger::outside`] says whether a command writes or deletes anywhere else,
/// and the temporary folder is read as [`danger_in`] reads it.
pub fn danger_scoped(cmd: &str, scope: &Scope) -> Danger {
    let clean = |t: &String| Some(t.trim_end_matches('/').to_string()).filter(|t| t.starts_with('/') && t.len() > 1);
    let mut f = Facts {
        tmp: scope.tmp.as_ref().and_then(clean),
        site: scope.site.as_ref().and_then(clean),
        access: scope.access.iter().map(|a| a.trim_end_matches('/').to_string()).filter(|a| !a.is_empty()).collect(),
        own: scope.own.clone(),
        state: scope.state.clone(),
        ..Facts::default()
    };
    if f.site.is_some() {
        f.own_vars = mktemp_vars(cmd);
    }
    // a control or invisible character is read as a blank (so a push is still a push), and the line is unreadable all the same
    let odd = |c: char| (c.is_control() && !matches!(c, '\n' | '\t')) || invisible(c);
    let q = parse(&cmd.chars().map(|c| if odd(c) { ' ' } else { c }).collect::<String>(), 0);
    if fork_bomb(cmd) {
        f.classes.push("wipe");
    }
    note_vars(&q.cmds, &mut f);
    q.cmds.iter().for_each(|c| classify(c, 0, &mut f));
    if f.named_config && (f.writes || f.bad) {
        f.classes.push("config");
    }
    let braces = |w: &str| q.cmds.iter().flat_map(|c| &c.words).filter(|x| x.text == w).count();
    let shaky = |c: char| odd(c) || (c.is_whitespace() && !c.is_ascii());
    let unread = !q.ok || braces("{") != braces("}") || cmd.chars().any(shaky);
    let odd_in_docs = unread && q.ok && braces("{") == braces("}") && !f.classes.contains(&"unknown") && odd_in_python_docs(cmd, shaky);
    if unread {
        f.classes.push("unknown");
    }
    let mut classes: Vec<String> = ORDER.iter().filter(|c| f.classes.contains(c)).map(|c| c.to_string()).collect();
    let writes = f.writes || f.bad || f.classes.iter().any(|c| !QUIET.contains(c));
    if classes.is_empty() {
        classes.push(if f.bad { "write" } else if f.soft { "read" } else { "safe" }.to_string());
    }
    let program = q.cmds.iter().find(|c| !c.inner).and_then(|c| {
        c.words.iter().find(|w| !LEAD.contains(&w.text.as_str()) && assign(&w.text).is_none()).map(|w| base(&w.text).to_string())
    });
    Danger {
        class: classes[0].clone(),
        writes,
        compound: q.cmds.len() > 1 || q.subs || f.extra || f.classes.contains(&"unknown"),
        broad: f.broad,
        program: program.unwrap_or_default(),
        classes,
        outside: f.outside,
        made: f.made,
        reach: Reach { odd_in_docs, ..f.reach },
        remote: if f.remote == Remote::No {
            Remote::No
        } else if f.sinks || f.bg || q.bg || unread {
            Remote::Cannot
        } else {
            f.remote
        },
    }
}

/// The same as a [`Value`] map (`class`, `writes`, `compound`, `broad`, `program`, `classes`), for `danger(cmd).class == 'push'`.
pub fn danger_value(cmd: &str) -> Value {
    value_of(danger(cmd))
}

/// [`danger_value`] for a task with a [`Scope`].
pub fn danger_value_scoped(cmd: &str, scope: &Scope) -> Value {
    value_of(danger_scoped(cmd, scope))
}

fn value_of(d: Danger) -> Value {
    Value::Map(BTreeMap::from([
        ("class".to_string(), Value::Str(d.class)),
        ("writes".to_string(), Value::Bool(d.writes)),
        ("compound".to_string(), Value::Bool(d.compound)),
        ("broad".to_string(), Value::Bool(d.broad)),
        ("program".to_string(), Value::Str(d.program)),
        ("classes".to_string(), Value::List(d.classes.into_iter().map(Value::Str).collect())),
        ("outside".to_string(), Value::Bool(d.outside)),
        ("remote".to_string(), Value::Str(d.remote.word().to_string())),
    ]))
}
