//! Agents › Overview and Lab: what J4n is connected to, what on this machine is not, and what the lab found.
//! Two folds, `fold:connect` and `fold:lab`, drawn by the views of the pack `connect`; the TUI draws rows, this reads the files and the Log.
//!
//! - **Connect.** An add line (the program typed, kept as the draft `connect.program`), then `programs found on this machine and not
//!   connected` (the home's install folders, the project's, and the `PATH` folders under the home, the project, `/opt` and `/usr/local/bin`), then one row per connection, an
//!   agent or a tool that runs a program. Its page hangs under it: the connector row, its skill, its limit, its history.
//! - **Where there is no data yet** the row says `—` and one function is the place a later step fills: [`Front::limit_of`] (the
//!   limit's gauge) and [`Front::health_of`] (`limited`, `drifted`, `learning`). Nothing else of the tab changes when they do. The skill of a
//!   connection is made from its row ([`crate::skill::of_connector`]), so it is always there.
//! - **Lab.** The runs found in `<home>/lab/<run>/` and `~/.cache/j4n-lab/runs/<run>/`: only their `rows.jsonl` and `table.txt` are read,
//!   never a seed's folder, and nothing here starts a run.

use super::{Front, Said};
use crate::value::Value;
use serde_json::Value as Json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, UNIX_EPOCH};

/// The form of the add line (`draft:connect.program`).
pub const FORM: &str = "connect";
/// The one field of it.
pub const FIELD: &str = "program";
/// The folders of a home that hold programs people put there, the project's own after them.
const HOME_BINS: [&str; 4] = [".local/bin", ".cargo/bin", "go/bin", ".bun/bin"];
const PROJECT_BINS: [&str; 5] = ["node_modules/.bin", ".venv/bin", "target/release", "build", "dist"];
/// `PATH` folders outside the home and the project that still hold what people install by hand. Every other folder of the `PATH` is the
/// system's (`/usr/bin` holds thousands of its own programs): it is not listed, and a program in it is connected by typing its name.
const INSTALLED_BINS: [&str; 2] = ["/opt", "/usr/local/bin"];
/// Words that run in a shell and may also be files in `/usr/bin`: they are not programs to connect.
const BUILTINS: [&str; 30] = [
    "[", "alias", "bg", "cd", "command", "echo", "exit", "export", "false", "fc", "fg", "getopts", "hash", "jobs", "kill", "printf", "pwd", "read", "set", "shift", "source", "test", "times", "trap", "true", "type", "ulimit", "umask", "unset", "wait",
];
/// How many programs the list shows, how many a folder is read for, how long what was read stands, and how many runs and lines of a table are shown.
const SHOWN: usize = 12;
const PER_DIR: usize = 60;
const STALE: Duration = Duration::from_secs(30);
const RUNS: usize = 12;
const TABLE_LINES: usize = 90;

/// What the tab has read and keeps for a while: the programs found, the lab runs, and what the Log said of the canaries.
#[derive(Default)]
pub struct Seen {
    found: Mutex<Option<(Instant, Vec<Program>)>>,
    lab: Mutex<Option<(Instant, Vec<Run>)>>,
    canary: Mutex<Canaries>,
    /// Agents › Setup's proof call: the last one the Log holds, and whether one is being made.
    pub(super) proof: Mutex<super::setup::Proof>,
}

/// A program found on the machine, as the list says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    /// The word `/connect` is given: the name when the `PATH` has it, else the path.
    pub said: String,
    pub name: String,
    /// Where, with the home written `~`.
    pub at: String,
}

/// A lab run, as its two files say it.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub name: String,
    pub dir: PathBuf,
    pub at_ms: u64,
    pub seeds: usize,
    pub drafter: String,
    /// `(passed, of)` for the seeds of seen families and the seeds with a held-out value, on `connected` and on `chats`.
    pub connected: [(usize, usize); 2],
    pub chats: [(usize, usize); 2],
    pub usd: Option<f64>,
    pub table: Vec<String>,
}

#[derive(Default)]
struct Canaries {
    upto: u64,
    /// A flow's task → what it was for: a connector's id, or the program the Connect flow was given.
    flows: BTreeMap<String, String>,
    /// `(what, at_ms, ok)`, oldest first.
    runs: Vec<(String, u64, bool)>,
    /// Whether the last run of a connection said nothing J4n could read (`run.health`), by the doer's id.
    drifted: BTreeMap<String, bool>,
}

fn text(s: impl AsRef<str>) -> Value {
    Value::text(s.as_ref())
}

fn field(m: &toml::Table, k: &str) -> String {
    m.get(k).and_then(toml::Value::as_str).unwrap_or_default().to_string()
}

/// The home of the operator's own files, as the process has it (`J4N_HOME`, else `~/.j4n`), and the user's home.
fn user_home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn j4n_home() -> PathBuf {
    std::env::var_os("J4N_HOME").map(PathBuf::from).unwrap_or_else(|| user_home().join(".j4n"))
}

/// The word a program is known by: the last part of its path.
fn base(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// The path of `program` when it can run: a path that is an executable, or a name some folder of `path` has.
pub fn resolve(program: &str, path: &[PathBuf]) -> Option<PathBuf> {
    if program.contains('/') {
        return Some(PathBuf::from(program)).filter(|p| is_executable(p));
    }
    path.iter().map(|d| d.join(program)).find(|p| is_executable(p))
}

/// The programs in the folders J4n looks in, in the order of the folders, each folder's by name, the ones `connected` names left out.
/// `path` is the `PATH`, `home` the user's home and `root` the open project. Pure of the process: the tests give their own.
pub fn find_programs(home: &Path, root: &Path, path: &[PathBuf], connected: &[String]) -> Vec<Program> {
    let mut dirs: Vec<PathBuf> = HOME_BINS.iter().map(|d| home.join(d)).collect();
    dirs.extend(PROJECT_BINS.iter().map(|d| root.join(d)));
    for d in path {
        let mine = d.starts_with(home) || d.starts_with(root) || INSTALLED_BINS.iter().any(|s| d.starts_with(s));
        if mine && !dirs.contains(d) {
            dirs.push(d.clone());
        }
    }
    let mut out: Vec<Program> = Vec::new();
    for dir in dirs {
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        let mut names: Vec<String> = read.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        let mut taken = 0;
        for name in names {
            let own = name.starts_with("j4n") || name.starts_with('.') || name.contains(".so") || BUILTINS.contains(&name.as_str());
            if own || connected.contains(&name) || out.iter().any(|p| p.name == name) || !is_executable(&dir.join(&name)) {
                continue;
            }
            taken += 1;
            if taken > PER_DIR {
                break;
            }
            let on_path = path.contains(&dir);
            let at = match dir.strip_prefix(home) {
                Ok(rel) => format!("~/{}", rel.display()),
                Err(_) => dir.display().to_string(),
            };
            out.push(Program { said: if on_path { name.clone() } else { dir.join(&name).display().to_string() }, name, at });
        }
    }
    out
}

/// A lab run in `dir` (its `rows.jsonl` and `table.txt`, and nothing else of it); none when it has neither.
pub fn read_run(dir: &Path) -> Option<Run> {
    let rows = std::fs::read_to_string(dir.join("rows.jsonl")).unwrap_or_default();
    let table = std::fs::read_to_string(dir.join("table.txt")).unwrap_or_default();
    if rows.is_empty() && table.is_empty() {
        return None;
    }
    let at = |f: &str| std::fs::metadata(dir.join(f)).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64);
    // a seed is held when any of its rows names a held-out value; it passes a check when that row says `pass`
    let mut seeds: BTreeMap<u64, (bool, [bool; 2])> = BTreeMap::new();
    let mut drafter = String::new();
    for row in rows.lines().filter_map(|l| serde_json::from_str::<Json>(l).ok()) {
        let Some(seed) = row.get("seed").and_then(Json::as_u64) else { continue };
        let me = seeds.entry(seed).or_default();
        me.0 |= row.get("held").and_then(Json::as_array).is_some_and(|h| !h.is_empty());
        let pass = row.get("result").and_then(Json::as_str) == Some("pass");
        match row.get("check").and_then(Json::as_str) {
            Some("connected") => me.1[0] = pass,
            Some("chats") => me.1[1] = pass,
            _ => {}
        }
        if drafter.is_empty() {
            drafter = row.get("drafter").and_then(Json::as_str).unwrap_or_default().to_string();
        }
    }
    let count = |held: bool, i: usize| {
        let of = seeds.values().filter(|s| s.0 == held);
        (of.clone().filter(|s| s.1[i]).count(), of.count())
    };
    // the first line of the table says the spend: `lab run X: 200 seeds, drafter haiku, spent $0.91`
    let usd = table.lines().next().and_then(|l| l.split("spent $").nth(1)).and_then(|s| s.split_whitespace().next().and_then(|n| n.trim_end_matches(',').parse::<f64>().ok()));
    let drafter = if drafter.is_empty() { table.lines().next().and_then(|l| l.split("drafter ").nth(1)).and_then(|s| s.split([',', ' ']).next()).unwrap_or_default().to_string() } else { drafter };
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Some(Run {
        name,
        dir: dir.to_path_buf(),
        at_ms: at("rows.jsonl").max(at("table.txt")),
        seeds: seeds.len(),
        drafter,
        connected: [count(false, 0), count(true, 0)],
        chats: [count(false, 1), count(true, 1)],
        usd,
        table: table.lines().take(TABLE_LINES).map(String::from).collect(),
    })
}

/// The runs under `roots` (each holds `<run>/rows.jsonl`), newest first, at most [`RUNS`].
pub fn read_runs(roots: &[PathBuf]) -> Vec<Run> {
    let mut runs: Vec<Run> = roots.iter().filter_map(|r| std::fs::read_dir(r).ok()).flat_map(|d| d.flatten()).filter(|e| e.path().is_dir()).filter_map(|e| read_run(&e.path())).collect();
    runs.sort_by(|a, b| b.at_ms.cmp(&a.at_ms).then_with(|| a.name.cmp(&b.name)));
    runs.truncate(RUNS);
    runs
}

/// `passed/of` for each of the two groups, as the Lab's row says it: seen families, then held-out ones.
fn pair(p: [(usize, usize); 2]) -> String {
    format!("{}/{} · {}/{}", p[0].0, p[0].1, p[1].0, p[1].1)
}

/// A date as the lab's row reads it, `2001-02-03`, from milliseconds since 1970 (UTC: it does not move with the zone).
fn date_of(ms: u64) -> String {
    let days = (ms / 86_400_000) as i64;
    // days since 1970-01-01 to a civil date (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let (y, doy) = (yoe + era * 400, doe - (365 * yoe + yoe / 4 - yoe / 100));
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{m:02}-{d:02}", if m <= 2 { y + 1 } else { y })
}

impl Front {
    // ---- the connections ----

    /// The connections: the agents and tools of the Table that run a program (`test` and `builtin` are J4n's own).
    fn connections(&self) -> Vec<(String, toml::Table)> {
        let tables = &self.screen().tables;
        tables
            .ids("connector")
            .into_iter()
            .filter_map(|id| tables.raw("connector", id).map(|m| (id.to_string(), m.fields.clone())))
            .filter(|(_, f)| matches!(field(f, "is").as_str(), "agent" | "tool") && !field(f, "program").is_empty() && !matches!(field(f, "dialect").as_str(), "test" | "builtin"))
            .collect()
    }

    /// The limit gauge of a connection: how full the tightest window the program has said is (`80% req`), and when a limit shut it, when it is let in
    /// again. `None` until the program has said one. The windows are the router's own (`run.window`), read from the program's output by its `limits`.
    fn limit_of(&self, id: &str) -> Option<String> {
        let now = self.0.log.clock().now_ms();
        let shut = {
            let mut usage = self.usage();
            usage.catch_up(&self.0.log);
            usage.limited_until(id, "", "", now)
        }
        .map(|at| format!("limited, back in {}", super::usage::hm(at - now)));
        let tightest = self.tasks().windows().into_iter().filter(|w| w.doer == id).max_by(|a, b| a.used.total_cmp(&b.used));
        let gauge = tightest.map(|w| format!("{:.0}% {}", w.used * 100.0, w.short()));
        match (gauge, shut) {
            (Some(g), Some(s)) => Some(format!("{g} · {s}")),
            (g, s) => g.or(s),
        }
    }

    /// What the health of a connection is: `limited`, `drifted` or `learning`; `None` until it is read. The one place that is filled.
    fn health_of(&self, id: &str, program: &str) -> Option<&'static str> {
        let relearning = self.tasks().book().open().iter().any(|r| r.spec.doer == "flow:connect" && r.spec.goal.trim() == program);
        if relearning {
            return Some("learning");
        }
        if self.read_canaries().drifted.get(id).copied().unwrap_or(false) {
            return Some("drifted");
        }
        None
    }

    /// The skill a connection has, by what it takes of a doer's context (a longer line would widen the whole list; its name is `connector-<id>`). It is made from the connector row, so a connection the operator
    /// accepted has one; `None` for a row that runs no program.
    fn skill_of(&self, id: &str) -> Option<String> {
        let screen = self.screen();
        let row = screen.tables.raw("connector", id)?;
        crate::skill::of_connector(id, &row.fields).map(|s| format!("{} tokens", s.cost))
    }

    /// The canary runs the Log has for a connection, newest first: `(at_ms, ok)`. Those of the Connect flow (a program given to `/connect`,
    /// or a connector tested by `:connector test`) are told apart by what the flow's task was given.
    fn canary_runs(&self, id: &str, program: &str) -> Vec<(u64, bool)> {
        let c = self.read_canaries();
        c.runs.iter().rev().filter(|(what, _, _)| what == id || what == program || base(what) == base(program)).map(|(_, at, ok)| (*at, *ok)).collect()
    }

    /// What the Log has said, read on from where it was read last: the canary runs and the health of each connection.
    fn read_canaries(&self) -> std::sync::MutexGuard<'_, Canaries> {
        let mut c = self.0.connect.canary.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let from = c.upto;
        for ev in self.log().read(from).filter(|e| e.seq > from) {
            c.upto = c.upto.max(ev.seq);
            match (ev.kind.as_str(), &ev.task) {
                ("task.queued", Some(task)) => {
                    let doer = ev.data.pointer("/spec/doer").and_then(Json::as_str).unwrap_or_default();
                    let goal = ev.data.pointer("/spec/goal").and_then(Json::as_str).unwrap_or_default();
                    if doer == "flow:connect" {
                        c.flows.insert(task.clone(), goal.trim().to_string());
                    } else if doer == "flow:connect-test" {
                        let id = goal.parse::<toml::Table>().map(|t| field(&t, "id")).unwrap_or_default();
                        c.flows.insert(task.clone(), id);
                    }
                }
                ("flow.step.ended", Some(task)) if ev.data.get("step").and_then(Json::as_str) == Some("canary") => {
                    if let Some(what) = c.flows.get(task).cloned() {
                        let ok = ev.data.get("ok").and_then(Json::as_bool).unwrap_or(false);
                        c.runs.push((what, ev.at_ms, ok));
                    }
                }
                ("run.health", _) => {
                    if let crate::change::Actor::Doer(doer) = &ev.by {
                        c.drifted.insert(doer.clone(), ev.data.get("drifted").and_then(Json::as_bool).unwrap_or(false));
                    }
                }
                _ => {}
            }
        }
        c
    }

    /// Whether `program` can run here: where the `PATH` and the project's folders find it.
    fn program_found(&self, program: &str) -> bool {
        let path: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
        resolve(program, &path).is_some() || (!program.contains('/') && PROJECT_BINS.iter().any(|d| is_executable(&self.0.cfg().root.join(d).join(program))))
    }

    /// The one row Agents › Deciders shows when J4n has no decider: the pack `jev` is off, so the ask points use their fixed
    /// pick and `⏎` turns the pack on; or it is on and no decider was made, for want of the key, which Agents › Providers types.
    pub fn decider_gap(&self) -> Option<Value> {
        let screen = self.screen();
        let jev = screen.packs.iter().find(|p| p.id == "jev")?;
        let (kind, now, note) = if jev.on {
            ("no-key", "no key", "no decider is made until its key is typed · ⏎ opens Agents › Providers")
        } else {
            ("pack-off", "off", "the ask points use their fixed pick · ⏎ turns the pack on")
        };
        Some(Value::Map(BTreeMap::from([
            ("id".to_string(), text("jev")),
            ("is".into(), text("decider")),
            ("kind".into(), text(kind)),
            ("dialect".into(), text(now)),
            ("url".into(), text(note)),
            ("tone".into(), text("warn")),
        ])))
    }

    /// The rows of `fold:connect`.
    pub fn connect_rows(&self) -> Vec<Value> {
        let typed = self.0.drafts.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&format!("{FORM}.{FIELD}")).cloned().unwrap_or_default();
        let mut rows = vec![Value::Map(BTreeMap::from([
            ("id".to_string(), text("add")),
            ("kind".into(), text("add")),
            ("label".into(), text("add")),
            ("now".into(), text(if typed.is_empty() { "a program or a path" } else { typed.as_str() })),
            ("note".into(), text(if typed.is_empty() { "⏎ types it" } else { "⏎ changes it" })),
            ("tone".into(), text(if typed.is_empty() { "dim" } else { "plain" })),
            ("edit".into(), Value::Map(BTreeMap::from([("kind".to_string(), text("str")), ("bind".into(), text(format!("draft:{FORM}.{FIELD}"))), ("value".into(), text(&typed))]))),
        ]))];
        if !typed.is_empty() {
            rows.push(Value::Map(BTreeMap::from([
                ("id".to_string(), text("go")),
                ("kind".into(), text("go")),
                ("label".into(), text("connect")),
                ("now".into(), text(&typed)),
                ("note".into(), text("⏎ learns how to speak to it, tries it once in the box, and asks for your yes")),
                ("said".into(), text(&typed)),
                ("tone".into(), text("accent")),
            ])));
        }
        let mine = self.connections();
        let connected: Vec<String> = mine.iter().flat_map(|(id, f)| [id.clone(), base(&field(f, "program")).to_string()]).collect();
        let found = self.found_programs(&connected);
        if !found.is_empty() {
            let more = found.len().saturating_sub(SHOWN);
            rows.push(Value::Map(BTreeMap::from([
                ("id".to_string(), text("found")),
                ("kind".into(), text("head")),
                ("label".into(), text("found here")),
                ("now".into(), text("not connected")),
                ("note".into(), text(if more > 0 { format!("{more} more: type a name above") } else { "⏎ connects one".to_string() })),
            ])));
            rows.extend(found.into_iter().take(SHOWN).map(|p| {
                Value::Map(BTreeMap::from([("id".to_string(), text(format!("found:{}", p.said))), ("kind".into(), text("found")), ("label".into(), text(&p.name)), ("now".into(), text(&p.at)), ("note".into(), text("⏎ connects it")), ("said".into(), text(&p.said))]))
            }));
        }
        let head = if mine.is_empty() { "nothing connected yet" } else { "connected" };
        rows.push(Value::Map(BTreeMap::from([("id".to_string(), text("connected")), ("kind".into(), text("head")), ("label".into(), text(head)), ("note".into(), text(if mine.is_empty() { "⏎ on a program above connects it" } else { "⏎ opens one · t tests · r learns again · o turns off" }))])));
        rows.extend(mine.iter().map(|(id, f)| self.connection_row(id, f)));
        // a column a row has nothing for is a space, not a dash (an empty text shows as `—`, which a found program has no use for): the rows share the view's cells
        for row in &mut rows {
            if let Value::Map(m) = row {
                for k in ["is", "state", "limit", "canary", "now", "note"] {
                    m.entry(k.to_string()).or_insert_with(|| text(" "));
                }
            }
        }
        rows
    }

    fn connection_row(&self, id: &str, f: &toml::Table) -> Value {
        let program = field(f, "program");
        let off = f.get("off").and_then(toml::Value::as_bool).unwrap_or(false);
        let health = self.health_of(id, &program);
        let (state, tone) = if off {
            ("off", "dim")
        } else if !self.program_found(&program) {
            ("broken", "bad")
        } else {
            match health {
                Some("limited") => ("limited", "warn"),
                Some("drifted") => ("drifted", "warn"),
                Some("learning") => ("learning", "accent"),
                _ => ("ok", "good"),
            }
        };
        let runs = self.canary_runs(id, &program);
        let when = |at: u64| crate::clock::when_local((at / 1000) as i64);
        let canary = runs.first().map_or("—".to_string(), |(at, ok)| format!("{} {}", if *ok { "pass" } else { "fail" }, when(*at)));
        let limit = self.limit_of(id).unwrap_or_else(|| "—".into());
        let argv = ["argv", "start"].iter().find_map(|k| f.get(*k).and_then(toml::Value::as_array)).map(|a| a.iter().filter_map(toml::Value::as_str).collect::<Vec<_>>().join(" ")).unwrap_or_default();
        let risk = match field(f, "is").as_str() {
            "tool" if field(f, "risk").is_empty() => "exec".to_string(),
            "tool" => field(f, "risk"),
            _ => "—".into(),
        };
        let none = |s: String| if s.is_empty() { "—".to_string() } else { s };
        // short lines: the words of a row that hangs are wrapped to the room the row's cells leave
        let line = |k: &str, v: String| vec![Value::Num(1.0), text(format!("{k}  {v}"))];
        let mut tree = vec![
            line("dialect", field(f, "dialect")),
            line("runs", program.clone()),
            line("args", none(argv)),
            // a connector that was learned runs behind the Gate in write mode: said next to the risk
            line("risk", if f.get("gated").and_then(toml::Value::as_bool).unwrap_or(false) { format!("{risk} · gated") } else { risk }),
            line("skill", none(self.skill_of(id).unwrap_or_default())),
            line("limit", limit.clone()),
            line("history", if runs.is_empty() { "—".to_string() } else { format!("{} canaries", runs.len()) }),
        ];
        tree.extend(runs.iter().take(5).map(|(at, ok)| vec![Value::Num(2.0), text(format!("{}  {}", if *ok { "pass" } else { "fail" }, when(*at)))]));
        Value::Map(BTreeMap::from([
            ("id".to_string(), text(id)),
            ("kind".into(), text("conn")),
            ("label".into(), text(id)),
            ("is".into(), text(field(f, "is"))),
            ("state".into(), text(state)),
            ("limit".into(), text(limit)),
            ("canary".into(), text(canary)),
            ("tone".into(), text(tone)),
            ("program".into(), text(&program)),
            ("off".into(), Value::Bool(off)),
            ("tree".into(), Value::List(tree.into_iter().map(Value::List).collect())),
        ]))
    }

    /// The programs the machine has and J4n is not connected to; read again after [`STALE`].
    fn found_programs(&self, connected: &[String]) -> Vec<Program> {
        let mut last = self.0.connect.found.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if last.as_ref().is_none_or(|(at, _)| at.elapsed() >= STALE) {
            let path: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
            *last = Some((Instant::now(), find_programs(&user_home(), &self.0.cfg().root, &path, &[])));
        }
        // what was read once stands; what is connected now is left out each time
        last.as_ref().map(|(_, l)| l.iter().filter(|p| !connected.contains(&p.name)).cloned().collect()).unwrap_or_default()
    }

    /// `:connector test <id>`: the canary of a connection, run once in the box (the Connect flow's own step), the Gate asking as it does there.
    /// `:connector off <id>` and `on` switch it: an operator's edit of the row (a Change with an undo), which the Agents tab asks about first and
    /// which is read at the next start.
    pub(super) fn connector_said(&self, thread: &str, args: &[String]) -> Said {
        let (verb, Some(id)) = (args.first().map(String::as_str), args.get(1)) else {
            return self.refuse(thread, "`:connector test <name>` runs a connection's canary, `off` and `on` switch it: Agents › Overview has `t` and `o` on its row".into());
        };
        if !matches!(verb, Some("test" | "off" | "on")) {
            return self.refuse(thread, "`:connector` takes `test`, `off` or `on` and the name of a connection: Agents › Overview has `t` and `o` on its row".into());
        }
        let Some((_, f)) = self.connections().into_iter().find(|(c, _)| c == id) else {
            return self.refuse(thread, format!("there is no connection `{id}`: Agents › Overview lists them"));
        };
        if verb != Some("test") {
            return self.set_said(thread, "connector", id, "off", &Json::Bool(verb == Some("off")));
        }
        let mut entry = f.clone();
        entry.insert("id".into(), toml::Value::String(id.clone()));
        let manifest = match toml::to_string(&entry) {
            Ok(m) => m,
            Err(e) => return self.refuse(thread, format!("`{id}` could not be written out to test: {e}")),
        };
        self.flow(thread, &["connect-test".to_string(), manifest])
    }

    // ---- the lab ----

    /// The rows of `fold:lab`: a row per run, its table hanging under it.
    pub fn lab_rows(&self) -> Vec<Value> {
        let mut last = self.0.connect.lab.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if last.as_ref().is_none_or(|(at, _)| at.elapsed() >= STALE) {
            let roots = [j4n_home().join("lab"), user_home().join(".cache/j4n-lab/runs")];
            *last = Some((Instant::now(), read_runs(&roots)));
        }
        let runs = last.as_ref().map(|(_, r)| r.clone()).unwrap_or_default();
        if runs.is_empty() {
            let row = |k: &str, v: &str| (k.to_string(), text(v));
            return vec![Value::Map(BTreeMap::from([("id".to_string(), text("none")), row("run", "no lab runs yet"), row("when", " "), row("seeds", " "), row("drafter", " "), row("connected", " "), row("chats", " "), row("spend", "the lab keeps its runs; this page only reads them")]))];
        }
        runs.into_iter()
            .map(|r| {
                let tree = r.table.iter().map(|l| Value::List(vec![Value::Num(1.0), text(l)])).collect();
                Value::Map(BTreeMap::from([
                    ("id".to_string(), text(&r.name)),
                    ("kind".into(), text("run")),
                    ("run".into(), text(&r.name)),
                    ("when".into(), text(if r.at_ms == 0 { "—".to_string() } else { date_of(r.at_ms) })),
                    ("seeds".into(), text(format!("{} seeds", r.seeds))),
                    ("drafter".into(), text(if r.drafter.is_empty() { "—" } else { r.drafter.as_str() })),
                    ("connected".into(), text(format!("conn {}", pair(r.connected)))),
                    ("chats".into(), text(format!("chat {}", pair(r.chats)))),
                    ("spend".into(), text(r.usd.map_or("—".to_string(), |u| format!("${u:.2}")))),
                    ("tree".into(), Value::List(tree)),
                ]))
            })
            .collect()
    }
}
