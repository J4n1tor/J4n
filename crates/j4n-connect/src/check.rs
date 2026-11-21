//! The doer `check` (pack `code`'s `lane_check`): it runs the project's own
//! tests where the work was done, with a clock on it and a cleared environment, and says what they came to.
//!
//! Which command is the first `check` row, in the order of the ids, whose `files` exist in the folder and whose `needs` text is
//! there. A `*` in the last name of a path in `files` matches any run of characters (`test_*.py`); a row with `top` applies only in
//! the folder the check is given, never in a folder above a write that is looked in for a project. A row run in such a folder has its parent on
//! `PYTHONPATH` after its own value, so the tests of a package import it by its name.
//! The command is a program with its arguments as words, never a line given to a shell. It runs as `env -i …`, so the
//! machine's keys are not in its environment: only `PATH`, `HOME`, `LANG` and the names the row lets through.
//!
//! **The project's own interpreter**: a row whose `files` name the project's own tools (`.venv/bin/python`, `uv.lock`) sorts before the
//! row that uses the machine's `python3`, which under `env -i` never sees a virtual environment. A row with `if_found` applies only where its
//! program is on the `PATH`, so a `uv` row gives way to the next one when there is no `uv`; a relative program (`.venv/bin/python`) is the
//! folder's own and is found when the file is there. The environment stays cleared: no `VIRTUAL_ENV`, nothing of the operator's.
//!
//! **Pass, fail, and neither**: exit 0 is a pass; a failure is an error carrying the end of its output; a check that cannot run
//! here (the clock ran out, the program is not there, its output says so) is no evidence either way, and the task ends well
//! saying that, so a missing toolchain does not read as a red project. It ends with `RunEv::NotRun`, so that `task.settled` carries `not_run` and no
//! screen draws it as a tick.

use crate::proc;
use j4n_core::doer::{Caps, Doer, Emit, Probe, Run, RunCtl, RunEv, StartCx};
use j4n_core::expr::Expr;
use j4n_core::place::Launch;
use j4n_core::table::{Kind, Problem, Tables};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const ID: &str = "check";

/// How much of the end of the output is kept and shown.
const TAIL: usize = 4000;
/// The most output held while the check runs: its end.
const HELD: usize = 64 * 1024;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Needs {
    pub file: String,
    pub contains: String,
}

/// An entry of kind `check`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckRow {
    pub id: String,
    pub doc: Option<String>,
    pub files: Vec<String>,
    /// Files that must all be there as well as one of `files`: the project's own `.venv/bin/python`, its `uv.lock`.
    #[serde(default)]
    pub also: Vec<String>,
    pub needs: Option<Needs>,
    pub argv: Vec<String>,
    pub timeout: Option<String>,
    #[serde(default)]
    pub pass_env: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub unrunnable: Vec<String>,
    /// Run only when something names it (a chore of `offload`): never the check a folder has by default, and not one a plan item may name.
    #[serde(default)]
    pub named: bool,
    /// `false`: the check runs with the network cut where a box can cut it (place `box`), whatever `box.net` says.
    #[serde(default)]
    pub net: Option<bool>,
    /// The row applies only where its program is found: on the `PATH`, or, for a program named by a path, in the folder.
    #[serde(default)]
    pub if_found: bool,
    /// The row applies only in the folder the check is given (a worktree, the project), never in a folder above a write that the check looks for a project
    /// in: a pattern such as `test_*.py` marks the top of a folder of scripts, and a package inside a project has such files as well.
    #[serde(default)]
    pub top: bool,
}

impl Kind for CheckRow {
    const NAME: &'static str = "check";
}

/// What a check came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Fail(i32),
    /// Neither: why it could not run.
    Unrunnable(String),
}

/// `exit` and `out` of one run, judged. A failing exit whose output holds one of the row's words did not run: it was not the work
/// that was wrong.
pub fn judge(row: &CheckRow, code: Option<i32>, timed_out: bool, out: &str) -> Verdict {
    if timed_out {
        return Verdict::Unrunnable(format!("it did not finish in {}", crate::check::clock(row)));
    }
    match code {
        Some(0) => Verdict::Pass,
        Some(c @ (126 | 127)) => Verdict::Unrunnable(format!("the program could not be run (exit {c})")),
        Some(c) => {
            let lower = out.to_lowercase();
            match row.unrunnable.iter().find(|w| lower.contains(&w.to_lowercase())) {
                Some(w) => Verdict::Unrunnable(format!("its output says `{w}`")),
                None => Verdict::Fail(c),
            }
        }
        None => Verdict::Unrunnable("it was ended by a signal".into()),
    }
}

fn clock(row: &CheckRow) -> String {
    row.timeout.clone().unwrap_or_else(|| "10m".into())
}

/// The clock of a row in milliseconds: ten minutes when it names none that is more than zero.
pub fn timeout_ms(row: &CheckRow) -> u64 {
    row.timeout.as_deref().and_then(Expr::dur).filter(|ms| *ms > 0).unwrap_or(600_000)
}

/// The tail of `text`, whole characters.
fn tail(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.trim().to_string();
    }
    let mut cut = text.len() - max;
    while !text.is_char_boundary(cut) {
        cut += 1;
    }
    format!("…{}", text[cut..].trim())
}

/// Whether `name` names the row: its short name, or for a row that only a name runs, the first words of it (`lint` is `lint-cargo`).
fn is_named(r: &CheckRow, name: &str) -> bool {
    let id = short(&r.id);
    id == name || (r.named && !name.is_empty() && id.starts_with(&format!("{name}-")))
}

/// Whether the program of a row is there: a path in the folder, or a name on the `PATH` the check is run with.
fn found(row: &CheckRow, dir: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let Some(program) = row.argv.first() else { return false };
    let runs = |p: std::path::PathBuf| p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
    if program.contains('/') {
        return runs(dir.join(program));
    }
    std::env::var("PATH").is_ok_and(|path| std::env::split_paths(&path).any(|d| runs(d.join(program))))
}

/// A `*` matches any run of characters within one name.
fn glob(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((head, rest)) => name.strip_prefix(head).is_some_and(|tail| (0..=tail.len()).filter(|i| tail.is_char_boundary(*i)).any(|i| glob(rest, &tail[i..]))),
    }
}

/// Whether `file` of a row is in `dir`: the path itself, or, with a `*` in its last part, a name of that folder that the pattern matches.
pub fn present(dir: &std::path::Path, file: &str) -> bool {
    let (folder, last) = file.rsplit_once('/').map_or((dir.to_path_buf(), file), |(d, l)| (dir.join(d), l));
    if !last.contains('*') {
        return dir.join(file).exists();
    }
    std::fs::read_dir(folder).is_ok_and(|rd| rd.flatten().any(|e| e.file_name().to_str().is_some_and(|n| glob(last, n))))
}

/// A row's name: its id without the digits and dash that order it.
fn short(id: &str) -> String {
    id.trim_start_matches(|c: char| c.is_ascii_digit()).trim_start_matches('-').to_string()
}

pub struct Check {
    rows: Vec<CheckRow>,
}

impl Check {
    /// The rows of the Table, in the order of their ids. A row that cannot be used is a problem and no row.
    pub fn load(tables: &Tables) -> (Check, Vec<Problem>) {
        let (mut rows, mut problems) = (Vec::new(), Vec::new());
        for id in tables.ids(CheckRow::NAME) {
            match tables.get::<CheckRow>(id) {
                Ok(row) => match row.timeout.as_deref().map(|t| (t, Expr::dur(t))) {
                    Some((t, None | Some(0))) => problems.push(Problem::refused(format!("check {id}"), format!("`timeout` is a duration more than zero, such as `10m`, not `{t}`"))),
                    _ if row.argv.is_empty() || row.files.is_empty() => problems.push(Problem::refused(format!("check {id}"), "`argv` and `files` have words")),
                    _ if row.files.iter().chain(&row.also).any(|f| f.rsplit_once('/').is_some_and(|(d, _)| d.contains('*'))) => problems.push(Problem::refused(format!("check {id}"), "a `*` matches within the last name of a path only")),
                    _ => rows.push(row),
                },
                Err(p) => problems.push(p),
            }
        }
        (Check { rows }, problems)
    }

    /// The check that applies to the folder.
    pub fn row_for<'a>(&'a self, dir: &'a std::path::Path) -> Option<&'a CheckRow> {
        self.rows_in(dir, true).find(|r| !r.named)
    }

    /// The rows that apply in `dir`; `top`: it is the folder the check is given, not one found above a write.
    fn rows_in<'a>(&'a self, dir: &'a std::path::Path, top: bool) -> impl Iterator<Item = &'a CheckRow> + 'a {
        self.rows.iter().filter(move |r| {
            (top || !r.top)
                && r.files.iter().any(|f| present(dir, f))
                && r.also.iter().all(|f| present(dir, f))
                && (!r.if_found || found(r, dir))
                && r.needs.as_ref().is_none_or(|n| std::fs::read_to_string(dir.join(&n.file)).is_ok_and(|t| t.contains(&n.contains)))
        })
    }

    /// The folder a plan item's check runs in. The nearest folder at or above each of its `writes` (inside `root`) where a row applies,
    /// the first write that has one first; else `root`, as it was before. A path that leaves `root` is not looked at.
    fn project_dir(&self, root: &std::path::Path, writes: &[String]) -> std::path::PathBuf {
        for w in writes.iter().filter(|w| !w.starts_with('/') && !w.split('/').any(|c| c == "..")) {
            // a write that is a folder (`cli`, as a brief names the folder a chat makes) is looked in first, a file from its folder on
            let mut dir = root.join(w);
            if !dir.is_dir() {
                dir.pop();
            }
            while dir != root && dir.starts_with(root) {
                if self.rows_in(&dir, false).next().is_some() {
                    return dir;
                }
                dir.pop();
            }
        }
        root.to_path_buf()
    }

    /// The names a plan item may give as its `check`: a row's id without its order digits (`1-cargo` is `cargo`).
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        // the rows that name the project's own interpreter share the name of the row they come before
        for n in self.rows.iter().filter(|r| !r.named).map(|r| short(&r.id)) {
            if !names.contains(&n) {
                names.push(n);
            }
        }
        names
    }

    /// The command line of each row a folder has by default, as a line of words: what a shell approval for one of the project's checks reads as.
    pub fn lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        for l in self.rows.iter().filter(|r| !r.named).map(|r| r.argv.join(" ")) {
            if !lines.contains(&l) {
                lines.push(l);
            }
        }
        lines
    }

    /// The command as it is run: `env -i` with what the row lets through. `top`: `dir` is the folder the check is given.
    fn launch(&self, row: &CheckRow, cx: &StartCx, dir: std::path::PathBuf, top: bool) -> Launch {
        let mut argv: Vec<String> = vec!["env".into(), "-i".into()];
        let mut set = |k: &str, v: String| argv.push(format!("{k}={v}"));
        for (k, fallback) in [("PATH", "/usr/local/bin:/usr/bin:/bin"), ("HOME", "/"), ("LANG", "C.UTF-8")] {
            set(k, std::env::var(k).unwrap_or_else(|_| fallback.into()));
        }
        set("TERM", "dumb".into());
        for name in &row.pass_env {
            if let Ok(v) = std::env::var(name) {
                set(name, v);
            }
        }
        for (k, v) in &row.env {
            // a folder found above a write is a package its `tests` import by its own name (`from chess import pieces`): its parent, still inside the
            // folder the check is given, is on Python's path too
            set(k, if k == "PYTHONPATH" && !top { format!("{v}:..") } else { v.clone() });
        }
        argv.extend(row.argv.iter().cloned());
        cx.wrap(Launch { no_net: row.net == Some(false), ..Launch::new(argv, dir, BTreeMap::new()) })
    }
}

enum Msg {
    Ctl(RunCtl),
    Exit(proc::Exit),
}

struct CheckRun {
    tx: Sender<Msg>,
}

impl Run for CheckRun {
    fn ctl(&mut self, ctl: RunCtl) -> Result<(), Problem> {
        self.tx.send(Msg::Ctl(ctl)).map_err(|_| Problem::refused(ID, "the run is over"))
    }
}

impl Doer for Check {
    fn id(&self) -> &str {
        ID
    }

    fn probe(&self) -> Probe {
        Probe { installed: true, version: None, authed: true, detail: format!("{} checks described", self.rows.len()) }
    }

    fn caps(&self) -> Caps {
        Caps::default()
    }

    fn code(&self) -> bool {
        true
    }

    fn start(&self, cx: StartCx) -> Result<Box<dyn Run>, Problem> {
        let (tx, rx) = mpsc::channel::<Msg>();
        let out = cx.out.clone();
        // a plan item names its check; the row of that name wins where it applies, else the first row that does
        let named = cx.spec.input.get("check").and_then(|c| c.as_str()).unwrap_or_default();
        let writes: Vec<String> = cx.spec.input.get("writes").and_then(|w| w.as_array()).into_iter().flatten().filter_map(|w| w.as_str().map(String::from)).collect();
        let dir = self.project_dir(&cx.site.dir, &writes);
        // a chore says `strict`: the check it names, or none, and never the folder's tests in its place
        let strict = cx.spec.input.get("strict").and_then(|c| c.as_bool()).unwrap_or(false);
        // a row for the top of a folder applies in the folder the check is given only
        let top = dir == cx.site.dir;
        let Some(row) = self.rows_in(&dir, top).find(|r| is_named(r, named)).or_else(|| self.rows_in(&dir, top).find(|r| !r.named).filter(|_| !strict)).cloned() else {
            let dir = cx.site.dir.display().to_string();
            let what = if strict { format!("no `{named}` check applies in {dir}") } else { format!("no check applies in {dir}: none of the described projects is there") };
            std::thread::spawn(move || {
                out.send(RunEv::NotRun(what.clone()));
                out.send(RunEv::End { answer: Some(what), todo_left: 0 });
            });
            return Ok(Box::new(CheckRun { tx }));
        };
        let launch = self.launch(&row, &cx, dir, top);
        let exit_tx = tx.clone();
        std::thread::spawn(move || run(&row, &launch, &rx, exit_tx, &out));
        Ok(Box::new(CheckRun { tx }))
    }
}

fn run(row: &CheckRow, launch: &Launch, rx: &mpsc::Receiver<Msg>, exit_tx: Sender<Msg>, out: &Emit) {
    let held = Arc::new(Mutex::new(String::new()));
    let sink = held.clone();
    let on_line = move |l: &str| {
        let mut s = sink.lock().unwrap_or_else(|e| e.into_inner());
        s.push_str(l);
        s.push('\n');
        // the end of the output is what says why a check failed: the beginning goes when there is too much
        if s.len() > 2 * HELD {
            let mut cut = s.len() - HELD;
            while !s.is_char_boundary(cut) {
                cut += 1;
            }
            s.drain(..cut);
        }
    };
    let (child, _) = match proc::spawn(launch, false, on_line, move |e| drop(exit_tx.send(Msg::Exit(e)))) {
        Ok(c) => c,
        Err(e) => {
            let said = format!("{}: not run — it could not be started ({e})", row.id);
            out.send(RunEv::NotRun(said.clone()));
            out.send(RunEv::End { answer: Some(said), todo_left: 0 });
            return;
        }
    };
    let (started, limit) = (Instant::now(), Duration::from_millis(timeout_ms(row)));
    let (mut timed_out, mut ended) = (false, None);
    while ended.is_none() {
        let left = limit.saturating_sub(started.elapsed());
        match rx.recv_timeout(left) {
            Ok(Msg::Exit(e)) => ended = Some(e),
            Ok(Msg::Ctl(RunCtl::Abort | RunCtl::Kill)) => {
                child.kill();
                return;
            }
            Ok(Msg::Ctl(_)) => {}
            Err(RecvTimeoutError::Timeout) => {
                timed_out = true;
                child.kill();
                // the end of the child still comes, and says how it went
                limit_is_over(rx, &mut ended);
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
    let exit = ended.unwrap_or(proc::Exit { code: None, signal: None, tail: String::new(), killed: true, broken: false });
    let text = {
        let held = held.lock().unwrap_or_else(|e| e.into_inner());
        if exit.tail.trim().is_empty() { held.clone() } else { format!("{held}{}", exit.tail) }
    };
    let verdict = judge(row, exit.code, timed_out, &text);
    out.send(RunEv::Shell { cmd: row.argv.join(" "), out: tail(&text, TAIL), exit: exit.code.unwrap_or(-1) });
    match verdict {
        Verdict::Pass => out.send(RunEv::End { answer: Some(format!("{}: passed", row.id)), todo_left: 0 }),
        Verdict::Unrunnable(why) => {
            out.send(RunEv::NotRun(why.clone()));
            out.send(RunEv::End { answer: Some(format!("{}: not run — {why}; that is no evidence either way", row.id)), todo_left: 0 })
        }
        Verdict::Fail(code) => out.send(RunEv::Error(format!("{} check failed (exit {code}):\n{}", row.id, tail(&text, TAIL)))),
    };
}

/// After the clock ran out and the child was killed: wait for its end.
fn limit_is_over(rx: &mpsc::Receiver<Msg>, ended: &mut Option<proc::Exit>) {
    while let Ok(msg) = rx.recv_timeout(Duration::from_secs(10)) {
        if let Msg::Exit(e) = msg {
            *ended = Some(e);
            return;
        }
    }
}
