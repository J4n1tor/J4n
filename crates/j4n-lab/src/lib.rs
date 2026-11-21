//! `j4n connect lab`: connect a harness the forge made, with no answers given, and score what came of it.
//!
//! The lab is a client of J4n, not a part of it: for each seed it makes the harness (the forge is a program of its own, never linked
//! here), puts it on the `PATH` of a temporary J4n home, starts `j4n` in that home as a user would, and reads what J4n did from what it
//! printed, wrote and logged. The forge's `truth.toml` is read by this scorer and by nothing else.

use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The line that starts each turn's answer in piped mode (`j4n::lines::MARK`).
const MARK: &str = "▌you: ";
/// The eight checks, in order.
mod tools;
pub use tools::TOOL_CHECKS;
pub const CHECKS: [&str; 9] = ["connected", "chats", "resumes", "tools_shown", "leave_asked", "limit_read", "fails_cleanly", "skill", "drift"];
const FLOW: &str = include_str!("../../../packs/connect/flows/connect.toml");
/// How long one start of `j4n` has before the lab ends it: the canary alone is given 90 s.
const WALL_SECS: u64 = 170;

pub struct Opts {
    /// The seeds, in the order they are taken.
    pub seeds: Vec<u64>,
    pub drafter: String,
    pub jobs: usize,
    /// Where runs are kept (`~/.j4n/lab`).
    pub out: PathBuf,
    /// Where each seed's harness and project folder are made. The box hides J4n's own folder and has a `/tmp` of its own, so the harness
    /// the box must run lives under neither (`~/.cache/j4n-lab`).
    pub work: PathBuf,
    pub run: String,
    pub j4n: PathBuf,
    pub forge: PathBuf,
    /// Memory the lab waits for before each seed (MB), and the swap it leaves alone (MB).
    pub free_mb: u64,
    pub swap_mb: u64,
    /// The most a run with a model drafter may spend (dollars): no seed starts once the seeds done have cost this much. The scripted drafter costs nothing.
    pub usd_cap: f64,
}

/// `A..B` (A up to, not with, B), `A..=B`, or one seed.
pub fn seeds(text: &str) -> Result<(u64, u64), String> {
    let bad = || format!("`{text}` is not seeds (try 0..8, 3..=5 or 7)");
    let n = |s: &str| s.trim().parse::<u64>().map_err(|_| bad());
    match text.split_once("..") {
        Some((a, b)) => {
            let r = match b.strip_prefix('=') {
                Some(b) => (n(a)?, n(b)? + 1),
                None => (n(a)?, n(b)?),
            };
            if r.0 < r.1 { Ok(r) } else { Err(bad()) }
        }
        None => Ok((n(text)?, n(text)? + 1)),
    }
}

/// Where the lab is allowed to say yes to a Proposal: inside the folder of its own run, nowhere else. A home that is not under
/// `~/.j4n/lab/<run>/…` is refused, so no flag and no path can make the lab accept a connector into someone's real home.
pub fn accepts_in(home: &Path, lab_root: &Path) -> bool {
    let (Ok(home), Ok(root)) = (home.canonicalize(), lab_root.canonicalize()) else { return false };
    home.starts_with(&root) && home != root && home.file_name().is_some_and(|n| n == "home")
}

// ---- the scripted drafter ----

/// A draft made from what the program said and nothing else (`j4n connect lab-draft PROMPT`): the arguments that carry the prompt are read
/// from the usage lines of its help. It is the floor of what code alone does with a help text, and costs nothing.
pub fn draft(prompt: &str) -> String {
    let between = |a: &str, b: &str| prompt.split_once(a).and_then(|(_, r)| r.split_once(b)).map(|(v, _)| v.to_string()).unwrap_or_default();
    let program = between("for the program `", "`");
    let (is, dialect) = (between("`is = \"", "\"").clone(), between("dialect = \"", "\""));
    let is = if is.is_empty() { between("is = \"", "\"") } else { is };
    let id: String = Path::new(&program).file_name().map_or_else(String::new, |n| n.to_string_lossy().to_lowercase()).chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    let id = if id.starts_with(|c: char| c.is_ascii_lowercase()) { id } else { format!("h{id}") };
    let help = prompt.split_once("Its help:").map_or("", |(_, r)| r.split_once("Its manual:").map_or(r, |(h, _)| h));
    let name = Path::new(&program).file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let start = start_of(help, &name);
    let q = |v: &[String]| v.iter().map(|a| format!("{a:?}")).collect::<Vec<_>>().join(", ");
    let mut t = format!("[[connector]]\nid = \"{id}\"\ndoc = \"A harness the lab connected.\"\nis = \"{is}\"\ndialect = \"{dialect}\"\nprogram = \"{program}\"\nprobe = {{ argv = [\"{program}\", \"--version\"] }}\n");
    if is == "tool" {
        let argv: Vec<String> = std::iter::once(program.clone()).chain(start.iter().map(|a| a.replace("{prompt}", "{text}"))).collect();
        t.push_str(&format!("args = [{{ name = \"text\", required = true }}]\nargv = [{}]\nrisk = \"read\"\ncanary = {{ goal = \"Reply with the single word: ready\", expect = \"ready\" }}\n", q(&argv)));
    } else {
        t.push_str(&format!("start = [{}]\n", q(&start)));
        if dialect == "jsonl-map" {
            t.push_str("[connector.map]\nend = \"/type == 'end'\"\nevents = [{ when = \"/type == 'text'\", emit = \"text\", text = \"/text\" }]\n");
        }
    }
    format!("```toml\n{t}```")
}

/// The arguments of a command line that carry the prompt, from the first usage line of the help that holds a placeholder for it.
fn start_of(help: &str, name: &str) -> Vec<String> {
    for line in help.lines() {
        let l = line.trim();
        let Some(rest) = l.strip_prefix(name).map(str::trim_start) else { continue };
        let up = |w: &str| matches!(w, "PROMPT" | "TEXT" | "MESSAGE" | "QUESTION" | "INPUT");
        let words: Vec<&str> = rest.split_whitespace().take_while(|w| !w.starts_with('(')).collect();
        if let Some(i) = words.iter().position(|w| up(w)) {
            return words[..i].iter().filter(|w| !w.starts_with('[')).map(|w| w.to_string()).chain(["{prompt}".to_string()]).collect();
        }
    }
    vec!["{prompt}".into()]
}

// ---- one start of j4n ----

#[derive(Default)]
struct Exec {
    out: String,
    err: String,
    code: Option<i32>,
    killed: bool,
}

struct Seed {
    n: u64,
    root: PathBuf,
    harness: PathBuf,
    home: PathBuf,
    project: PathBuf,
}

impl Seed {
    fn log_lines(&self) -> Vec<Value> {
        let mut found = Vec::new();
        let dir = self.project.join(".j4n/log");
        let mut files: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "jsonl")).collect();
        files.sort();
        for f in files {
            found.extend(std::fs::read_to_string(f).unwrap_or_default().lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()));
        }
        found
    }
}

pub struct Lab {
    o: Opts,
    log: Option<j4n_core::log::Log>,
    /// The text of `rows.jsonl` so far; the file is written whole after each seed (law 3: only the Log appends).
    rows: Mutex<String>,
    next: AtomicUsize,
    done: Mutex<Vec<Done>>,
    /// The readings of claude's windows the runs' Logs gave: per login, the first and the last (`five_hour`, `seven_day`).
    windows: Mutex<BTreeMap<String, [(f64, f64); 2]>>,
    /// Set when a login the lab fell back to is at 0.97 of its seven-day window: no seed after it starts.
    halted: std::sync::atomic::AtomicBool,
}

/// The seven-day reading at which the lab stops using a login that is not the operator's own.
const STOP_AT: f64 = 0.97;

/// One seed's result: the harness's truth about itself, and what each check came to.
pub struct Done {
    pub seed: u64,
    pub axes: BTreeMap<String, String>,
    pub held: Vec<String>,
    pub results: Vec<(&'static str, &'static str, String)>,
    pub usd: f64,
    pub ms: u64,
}

fn meminfo(key: &str) -> u64 {
    std::fs::read_to_string("/proc/meminfo").unwrap_or_default().lines().find_map(|l| l.strip_prefix(key)?.trim_start_matches(':').split_whitespace().next()?.parse::<u64>().ok()).unwrap_or(0) / 1024
}

impl Lab {
    pub fn new(o: Opts) -> Result<Lab, String> {
        // the box hides `/tmp` and J4n's own folder: a forge that lives there cannot be run by the harness it makes, and every seed would end 127 with a model paid to draft it
        for (what, path) in [("the forge", &o.forge), ("j4n", &o.j4n)] {
            let real = path.canonicalize().unwrap_or_else(|_| path.clone());
            if ["/tmp", "/var/tmp", "/dev/shm"].iter().any(|hidden| real.starts_with(hidden)) {
                return Err(format!("{what} is at {}, which the box hides: put it where the box can read it (under the home or the project)", real.display()));
            }
        }
        let dir = o.out.join(&o.run);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let rows = String::new();
        // the lab's own Log, in the folder of the run: the same events as the rows, for whatever reads the Log (a lab never writes into a project's)
        let log = j4n_core::log::Log::open(&dir.join(".j4n"), j4n_core::log::Scope::Project, j4n_core::log::Opts::default(), j4n_core::redact::Redactor::new()).ok().map(|(l, _)| l);
        Ok(Lab { next: AtomicUsize::new(0), log, o, rows: Mutex::new(rows), done: Mutex::new(Vec::new()), windows: Mutex::new(BTreeMap::new()), halted: std::sync::atomic::AtomicBool::new(false) })
    }

    /// Each seed waits until the machine has the memory for it.
    fn wait_for_room(&self) {
        while meminfo("MemAvailable") < self.o.free_mb || meminfo("SwapTotal").saturating_sub(meminfo("SwapFree")) > self.o.swap_mb {
            std::thread::sleep(Duration::from_secs(5));
        }
    }

    /// All the seeds, `jobs` at a time; the table.
    pub fn run(self) -> (Vec<Done>, String) {
        let lab = Arc::new(self);
        let workers: Vec<_> = (0..lab.o.jobs.max(1))
            .map(|_| {
                let lab = lab.clone();
                std::thread::spawn(move || {
                    loop {
                        let Some(n) = lab.o.seeds.get(lab.next.fetch_add(1, Ordering::SeqCst)).copied() else { return };
                        if lab.halted.load(Ordering::SeqCst) {
                            eprintln!("lab: stopped before seed {n}: a login is at {STOP_AT} of its seven-day window");
                            return;
                        }
                        let spent: f64 = lab.done.lock().map(|d| d.iter().map(|d| d.usd).sum()).unwrap_or(0.0);
                        if spent >= lab.o.usd_cap {
                            eprintln!("lab: stopped before seed {n}: the seeds done cost ${spent:.2}, and the cap is ${:.2}", lab.o.usd_cap);
                            return;
                        }
                        lab.wait_for_room();
                        let done = lab.seed(n);
                        lab.record(&done);
                        lab.read_windows(n);
                        eprintln!("lab: seed {n} {}", done.results.iter().map(|(c, r, _)| format!("{}:{}", &c[..c.len().min(4)], &r[..1])).collect::<Vec<_>>().join(" "));
                        lab.done.lock().map(|mut d| d.push(done)).ok();
                    }
                })
            })
            .collect();
        for w in workers {
            let _ = w.join();
        }
        let mut done = std::mem::take(&mut *lab.done.lock().unwrap_or_else(|e| e.into_inner()));
        done.sort_by_key(|d| d.seed);
        let mut table = table(&done, &lab.o.run, &lab.o.drafter);
        let reading = lab.windows.lock().map(|w| w.iter().map(|(who, [a, b])| format!("  {who}: five_hour {:.2} -> {:.2}, seven_day {:.2} -> {:.2}\n", a.0, b.0, a.1, b.1)).collect::<String>()).unwrap_or_default();
        table.push_str(&format!("\nwindow readings (first -> last of the run; -1 is none read):\n{}", if reading.is_empty() { "  none: no model ran\n".to_string() } else { reading }));
        if let Some(log) = &lab.log {
            let _ = log.append(j4n_core::log::New::of("lab.run", j4n_core::change::Actor::Core, json!({ "run": lab.o.run, "seeds": done.len(), "drafter": lab.o.drafter, "usd": done.iter().map(|d| d.usd).sum::<f64>(), "table": table })));
        }
        let _ = std::fs::write(lab.o.out.join(&lab.o.run).join("table.txt"), &table);
        (done, table)
    }

    /// What the seed's runs said of claude's windows (`run.window` in its Log): kept per login, and the stop when a login that
    /// is not the operator's own is at the line.
    fn read_windows(&self, n: u64) {
        let work = self.o.work.join(&self.o.run).join(format!("seed-{n}"));
        let s = Seed { n, root: PathBuf::new(), harness: PathBuf::new(), home: PathBuf::new(), project: work.join("proj") };
        let Ok(mut w) = self.windows.lock() else { return };
        for l in s.log_lines().iter().filter(|l| l["kind"] == "run.window") {
            let (who, window, u) = (l["data"]["account"].as_str().unwrap_or("default").to_string(), l["data"]["window"].as_str().unwrap_or_default(), l["data"]["utilization"].as_f64().unwrap_or(0.0));
            let e = w.entry(who.clone()).or_insert([(-1.0, -1.0); 2]);
            let slot = usize::from(window == "seven_day");
            let set = |p: &mut (f64, f64)| if slot == 0 { p.0 = u } else { p.1 = u };
            if e[0] == (-1.0, -1.0) || (slot == 0 && e[0].0 < 0.0) || (slot == 1 && e[0].1 < 0.0) {
                set(&mut e[0]);
            }
            set(&mut e[1]);
            if window == "seven_day" && u >= STOP_AT && who != "default" {
                self.halted.store(true, Ordering::SeqCst);
            }
        }
    }

    fn record(&self, d: &Done) {
        let Ok(mut f) = self.rows.lock() else { return };
        let mut text = String::new();
        for (check, result, why) in &d.results {
            let row = json!({ "run": self.o.run, "seed": d.seed, "check": check, "result": result, "why": why, "axes": d.axes, "held": d.held, "drafter": self.o.drafter });
            text.push_str(&format!("{row}\n"));
        }
        text.push_str(&format!("{}\n", json!({ "run": self.o.run, "seed": d.seed, "check": "run", "usd": d.usd, "ms": d.ms })));
        f.push_str(&text);
        let _ = std::fs::write(self.o.out.join(&self.o.run).join("rows.jsonl"), f.as_bytes());
        let say = |kind: &str, data: Value| {
            if let Some(log) = &self.log {
                let _ = log.append(j4n_core::log::New::of(kind, j4n_core::change::Actor::Core, data));
            }
        };
        for (check, result, why) in &d.results {
            say("lab.check", json!({ "run": self.o.run, "seed": d.seed, "check": check, "result": result, "why": why, "axes": d.axes, "held": d.held, "drafter": self.o.drafter }));
        }
        say("lab.seed", json!({ "run": self.o.run, "seed": d.seed, "usd": d.usd, "ms": d.ms }));
    }

    fn exec(&self, s: &Seed, args: &[&str], lines: &[String], wall: u64) -> Exec {
        let start = Instant::now();
        let path = format!("{}:{}", s.harness.display(), std::env::var("PATH").unwrap_or_default());
        let mut cmd = Command::new(&self.o.j4n);
        // the j4n under test is given no host's session, whoever ran the lab
        let scrub = j4n_core::host::Scrub::base();
        scrub.strip(&mut cmd);
        // a J4n is given back the keys it reads from its environment, as its relaunch is
        cmd.envs(scrub.keys());
        cmd.args(args)
            .current_dir(&s.project)
            .env("J4N_HOME", &s.home)
            .env("PATH", path)
            .env("MANPATH", s.harness.join("man"))
            .env("TERM", "dumb")
            .env_remove("J4N_IN_TASK")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let Ok(mut child) = cmd.spawn() else { return Exec { err: "the lab could not start j4n".into(), ..Exec::default() } };
        let (mut stdin, mut out, mut err) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
        let input = lines.join("\n") + "\n";
        let writer = std::thread::spawn(move || {
            if let Some(mut i) = stdin.take() {
                let _ = i.write_all(input.as_bytes());
            }
        });
        let reader = |r: Option<Box<dyn Read + Send>>| {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                if let Some(mut r) = r {
                    let _ = r.read_to_end(&mut buf);
                }
                String::from_utf8_lossy(&buf).into_owned()
            })
        };
        let (ro, re) = (reader(out.take().map(|o| Box::new(o) as Box<dyn Read + Send>)), reader(err.take().map(|e| Box::new(e) as Box<dyn Read + Send>)));
        let mut killed = false;
        let code = loop {
            match child.try_wait() {
                Ok(Some(st)) => break st.code(),
                Ok(None) if start.elapsed() > Duration::from_secs(wall) => {
                    killed = true;
                    // the process group is the one this lab started: j4n and the harness under it
                    let _ = Command::new("kill").args(["-KILL", &format!("-{}", child.id())]).status();
                    let _ = child.kill();
                    break child.wait().ok().and_then(|s| s.code());
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(100)),
                Err(_) => break None,
            }
        };
        let _ = writer.join();
        Exec { out: ro.join().unwrap_or_default(), err: re.join().unwrap_or_default(), code, killed }
    }

    /// The home of one seed: the Connect pack on, `connect.auto` on, and the drafter the flow elects.
    fn furnish(&self, s: &Seed) -> Result<(), String> {
        let put = |p: PathBuf, text: String| -> Result<(), String> {
            std::fs::create_dir_all(p.parent().unwrap_or(&p)).map_err(|e| e.to_string())?;
            std::fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))
        };
        put(s.home.join("harness.toml"), "api = 1\n[[enable]]\nid = \"connect\"\n".into())?;
        let here = j4n_core::gate::grant::here_dir(&s.home, &s.project);
        put(here.join("settings.toml"), "api = 1\n[[setting]]\nid = \"connect.auto\"\nvalue = true\n[[setting]]\nid = \"box.unavailable\"\nvalue = \"here\"\n".into())?;
        if self.o.drafter == "scripted" {
            let me = self.o.j4n.display();
            put(here.join("connectors/lab-drafter.toml"), format!("api = 1\n[[connector]]\nid = \"lab-drafter\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"{me}\"\nstart = [\"connect\", \"lab-draft\", \"{{prompt}}\"]\n"))?;
            put(here.join("flows/connect.toml"), FLOW.replace("do = \"who:work\"", "do = \"lab-drafter\""))?;
        }
        std::fs::create_dir_all(&s.project).map_err(|e| e.to_string())
    }

    fn drafter_args(&self) -> Vec<String> {
        match self.o.drafter.as_str() {
            "scripted" => vec!["--doer".into(), "lab-drafter".into()],
            m => vec!["--doer".into(), "claude".into(), "--model".into(), m.into()],
        }
    }

    /// One seed, start to end.
    fn seed(&self, n: u64) -> Done {
        let started = Instant::now();
        let root = self.o.out.join(&self.o.run).join(format!("seed-{n}"));
        let work = self.o.work.join(&self.o.run).join(format!("seed-{n}"));
        // a seed of a run that is run again starts clean: its Log, its home and its folders are the lab's own, and the Proposal of an earlier run
        // is a fold of that Log ("proposed before and left")
        let _ = std::fs::remove_dir_all(&work);
        let _ = std::fs::remove_dir_all(&root);
        let s = Seed { n, harness: work.join("h"), home: work.join("home"), project: work.join("proj"), root };
        let mut d = Done { seed: n, axes: BTreeMap::new(), held: vec![], results: vec![], usd: 0.0, ms: 0 };
        let mut forge = Command::new(&self.o.forge);
        j4n_core::host::Scrub::base().strip(&mut forge);
        let made = forge.args(["make", "--seed", &n.to_string(), "--out"]).arg(&s.harness).output();
        let truth: Option<toml::Table> = std::fs::read_to_string(s.harness.join("truth.toml")).ok().and_then(|t| t.parse().ok());
        let Some(truth) = truth.filter(|_| made.is_ok_and(|m| m.status.success())) else {
            d.results = CHECKS.iter().map(|c| (*c, "fail", "the forge made no harness".to_string())).collect();
            return d;
        };
        let get = |k: &str| truth.get(k).and_then(toml::Value::as_str).unwrap_or_default().to_string();
        let flag = |k: &str| truth.get(k).and_then(toml::Value::as_bool).unwrap_or(false);
        let name = get("name");
        d.axes = truth.get("axes").and_then(toml::Value::as_table).map(|t| t.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string())).collect()).unwrap_or_default();
        d.held = truth.get("held").and_then(toml::Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
        if let Err(why) = self.furnish(&s) {
            d.results = CHECKS.iter().map(|c| (*c, "fail", why.clone())).collect();
            return d;
        }
        if truth.get("family").and_then(toml::Value::as_str) == Some("tool") {
            return self.tools(&s, &truth, d, started);
        }
        let mut res: Vec<(&'static str, &'static str, String)> = Vec::new();
        let hung: std::cell::RefCell<Vec<String>> = std::cell::RefCell::new(Vec::new());
        let watch = |what: &str, e: &Exec| {
            if e.killed {
                hung.borrow_mut().push(format!("{what} was still running after {WALL_SECS} s"));
            }
        };

        // 1. connect: the flow, with no answer given but the yes the lab's own home allows to the Proposal
        let mut lines = vec![format!("/connect {name}")];
        if accepts_in(&s.home, &self.o.work.join(&self.o.run)) {
            lines.extend([":accept".into(), ":accept".into()]);
        }
        let mut args: Vec<String> = self.drafter_args();
        args.extend(["--approval".into(), "full".into()]);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let c = self.exec(&s, &args, &lines, WALL_SECS);
        watch("the connect flow", &c);
        let found = connector_of(&s, &name);
        let reason = refusal(&c);
        let (id, class) = found.clone().unwrap_or_default();
        let connected = found.is_some();
        res.push(("connected", if connected { "pass" } else { "fail" }, if connected { class.to_string() } else { reason.clone() }));
        let (mut chats_ok, mut canary_n) = (false, 0);
        let word = |canary_n: &mut u64| {
            *canary_n += 1;
            format!("lab{}x{}", s.n, canary_n)
        };

        // 2–3. chat, and a second line in the same session
        let w1 = word(&mut canary_n);
        let ask = format!("Reply with the single word: {w1}");
        let recall = "Which word did I ask you to reply with before?".to_string();
        // a window passes between the checks: what the harness has used of its limit is cleared, so no check is spent on the one before it
        let refill = || drop(std::fs::remove_file(s.project.join(format!("{}/state.json", get("state_dir")))));
        let chat_in = |lines: &[String], args: &[&str], clock: u64| -> Vec<String> {
            refill();
            // the chat's clock is J4n's too: a wait for a limit that ends after it ends the turn with one line, and a run is never left to the lab's kill
            let clock = format!("{clock}s");
            let mut a = vec!["--doer", id.as_str(), "--approval", "full", "--wall", clock.as_str()];
            a.extend(args);
            // each line is a turn, and a slow harness takes up to 8 s to start each: the clock is for the turns it is given
            let e = self.exec(&s, &a, lines, WALL_SECS + 12 * lines.len() as u64);
            watch("a chat", &e);
            answers(&e.out)
        };
        let chat = |lines: &[String], args: &[&str]| chat_in(lines, args, WALL_SECS + 12 * lines.len() as u64 - 15);
        if connected {
            let said = chat(&[ask.clone(), recall.clone()], &[]);
            chats_ok = said.first().is_some_and(|a| bare(a, &w1));
            res.push(("chats", if chats_ok { "pass" } else { "fail" }, format!("{class}: {}", clip(said.first().map_or("no answer", String::as_str)))));
            if flag("sessions") {
                let ok = said.get(1).is_some_and(|a| a.contains(&w1));
                res.push(("resumes", if ok { "pass" } else { "fail" }, clip(said.get(1).map_or("no answer", String::as_str))));
            } else {
                res.push(("resumes", "na", "no sessions".into()));
            }
        } else {
            res.push(("chats", "fail", "not connected".into()));
            res.push(if flag("sessions") { ("resumes", "fail", "not connected".into()) } else { ("resumes", "na", "no sessions".into()) });
        }

        // 4–5. tools shown, and the leave asked
        let (tools_shown, can_write) = (flag("tools_shown"), flag("can_write"));
        if connected && can_write {
            // the Gate is asked about whether a write can reach the disk, chat or no chat: a connector that cannot chat must still not write past it
            let _ = std::fs::remove_file(s.project.join("denied.txt"));
            chat(&["write secret to denied.txt".into()], &["--mode", "write", "--approval", "deny"]);
            let leaked = s.project.join("denied.txt").exists();
            let tools = if chats_ok {
                let before = s.log_lines().iter().filter(|l| l["kind"] == "run.tool_call").count();
                chat(&["write hello to shown.txt".into(), "read shown.txt".into()], &["--mode", "write"]);
                s.log_lines().iter().filter(|l| l["kind"] == "run.tool_call").count() - before
            } else {
                0
            };
            res.push(if !tools_shown {
                ("tools_shown", "na", "no tool events".into())
            } else if chats_ok {
                ("tools_shown", if tools > 0 { "pass" } else { "fail" }, format!("{tools} tool rows"))
            } else {
                ("tools_shown", "fail", "no chat to ask in".into())
            });
            res.push(("leave_asked", if leaked { "fail" } else { "pass" }, if leaked { "a write reached the disk with the Gate saying no".into() } else { "nothing was written without a yes".into() }));
        } else if can_write {
            res.push(if tools_shown { ("tools_shown", "fail", "not connected".into()) } else { ("tools_shown", "na", "no tool events".into()) });
            res.push(("leave_asked", "fail", "not connected".into()));
        } else {
            res.push(("tools_shown", "na", "no tools".into()));
            res.push(("leave_asked", "na", "no tools".into()));
        }

        // 6. the limit: push it, then see what J4n read
        if let Some(limit) = truth.get("limit").and_then(toml::Value::as_table) {
            let cap = limit.get("cap").and_then(toml::Value::as_float).or_else(|| limit.get("cap").and_then(toml::Value::as_integer).map(|i| i as f64)).unwrap_or(5.0);
            if connected && chats_ok {
                let many: Vec<String> = (0..(cap as usize).min(40) + 2).map(|i| format!("Reply with the single word: push{i}")).collect();
                // the clock is short: a limit that ends after it ends the turn at once, so a push is over when the harness refuses
                let each = if get("fails") == "slow_start" { 10 } else { 3 };
                chat_in(&many, &[], each * many.len() as u64 + 20);
                let state: Value = std::fs::read_to_string(s.project.join(format!("{}/limit.json", get("state_dir")))).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
                let (ok, said) = limit_verdict(&s.log_lines(), &id, &state);
                res.push(("limit_read", if ok { "pass" } else { "fail" }, said));
            } else {
                res.push(("limit_read", "fail", if connected { "no chat to push".into() } else { "not connected".into() }));
            }
        } else {
            res.push(("limit_read", "na", "no limit".into()));
        }

        // 7. a failure ends in its clock with a reason
        let fails = get("fails");
        let seven = if connected && matches!(fails.as_str(), "stall" | "crash_half" | "bad_utf8" | "giant_line" | "stderr_mix") {
            refill();
            let e = self.exec(&s, &["--doer", &id, "--approval", "full", "--wall", "40s"], &["trigger failure".into()], WALL_SECS);
            watch("the failing turn", &e);
            let said = answers(&e.out).join("\n");
            let tail = format!("{said}\n{}", e.err);
            if e.killed {
                ("fail", format!("{fails}: it did not end in its clock"))
            } else if matches!(fails.as_str(), "stall" | "crash_half") {
                let line = e.err.lines().rev().find(|l| l.starts_with("j4n:")).or_else(|| said.lines().last());
                (if line.is_some() && e.code != Some(0) { "pass" } else { "fail" }, format!("{fails}: {}", clip(line.unwrap_or("no reason said"))))
            } else {
                (if said.contains("done") { "pass" } else { "fail" }, format!("{fails}: {}", clip(tail.trim())))
            }
        } else if !connected && !connectable(&truth) {
            let low = reason.to_lowercase();
            let line = (!c.killed && ["sign in", "sign-in", "login", "log in", "logged"].iter().any(|w| low.contains(w))).then_some(reason.as_str());
            (if line.is_some() { "pass" } else { "fail" }, format!("login needed: {}", line.unwrap_or("no reason said")))
        } else if !connected {
            (if reason.is_empty() || c.killed { "fail" } else { "pass" }, format!("not connected: {}", if reason.is_empty() { "no reason said" } else { &reason }))
        } else {
            ("pass", "nothing failed".into())
        };
        res.push(("fails_cleanly", seven.0, seven.1));

        // 8. the skill: made from the connector row, and what it says of the harness is what the harness is
        res.push(if connected {
            skill_check(&s, &id, &truth, d.axes.get("a").map_or("", String::as_str), d.axes.get("b").map_or("", String::as_str))
        } else if connectable(&truth) {
            ("skill", "fail", "not connected".into())
        } else {
            ("skill", "na", "not connectable".into())
        });

        // 9. the upgrade: the harness renames its fields, and J4n must go on
        if flag("upgrade") {
            if connected && chats_ok {
                let mut forge = Command::new(&self.o.forge);
                j4n_core::host::Scrub::base().strip(&mut forge);
                let _ = forge.arg("upgrade").arg(&s.harness).status();
                // the first line after the upgrade is where J4n notices (the run says nothing it can read) and learns the program again, behind the box;
                // a line of the next start goes through the row that learning swapped in
                let w2 = word(&mut canary_n);
                let first = chat(&[format!("Reply with the single word: {w2}")], &[]);
                let w3 = word(&mut canary_n);
                let said = chat(&[format!("Reply with the single word: {w3}")], &[]);
                let ok = said.first().is_some_and(|a| bare(a, &w3));
                let noticed = s.log_lines().iter().any(|l| l["kind"] == "run.health" && l["data"]["drifted"] == true);
                let note = format!("{}; the upgrade was {}", clip(said.first().map_or("no answer", String::as_str)), if noticed { "noticed" } else if first.first().is_some_and(|a| bare(a, &w2)) { "not a change to J4n" } else { "not noticed" });
                res.push(("drift", if ok { "pass" } else { "fail" }, note));
            } else {
                res.push(("drift", "fail", "no chat to go on".into()));
            }
        } else {
            res.push(("drift", "na", "no upgrade".into()));
        }
        // a program J4n cannot connect (it asks for a login, wants a terminal, never ends) is held to one thing: a plain refusal in a line, inside the clock.
        // The checks of a connection it did not make are not scored: they would count the refusal as seven failures
        if !connected && !connectable(&truth) {
            for r in res.iter_mut().filter(|r| r.0 != "fails_cleanly") {
                *r = (r.0, "na", "not connectable".into());
            }
        }
        // a start of j4n the lab had to end is a hang, whatever else the checks say
        if let Some(h) = hung.borrow().first()
            && let Some(r) = res.iter_mut().find(|r| r.0 == "fails_cleanly")
        {
            *r = ("fails_cleanly", "fail", h.clone());
        }
        d.usd = spent(&s);
        d.results = res;
        d.ms = started.elapsed().as_millis() as u64;
        let _ = std::fs::write(s.root.join("seed.json"), json!({ "seed": n, "class": class, "id": id, "usd": d.usd, "ms": d.ms }).to_string());
        d
    }
}

/// The connector row the flow wrote for `id`.
fn row_of(s: &Seed, id: &str) -> Option<toml::Table> {
    let dirs = [s.home.join("connectors"), j4n_core::gate::grant::here_dir(&s.home, &s.project).join("connectors")];
    dirs.into_iter().flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten()).find_map(|e| {
        let t = std::fs::read_to_string(e.path()).ok()?.parse::<toml::Table>().ok()?;
        let row = t.get("connector").and_then(|c| c.as_array()).and_then(|a| a.first()).and_then(toml::Value::as_table).unwrap_or(&t).clone();
        (row.get("id").and_then(toml::Value::as_str) == Some(id)).then_some(row)
    })
}

/// The skill of a connected agent, held to what the harness says it is and to the row it was made from: it says when to read it, how the prompt goes in
/// (where the harness's axes say so), whether a later line carries on in a session (as the row says: whether the harness resumes is the check `resumes`), and
/// that the Gate is asked where the harness can write.
fn skill_check(s: &Seed, id: &str, truth: &toml::Table, prompt_axis: &str, output_axis: &str) -> (&'static str, &'static str, String) {
    let Some(row) = row_of(s, id) else { return ("skill", "fail", "no skill was made".into()) };
    let Some(sk) = j4n_core::skill::of_connector(id, &row) else { return ("skill", "fail", "no skill was made".into()) };
    if row.get("is").and_then(toml::Value::as_str) != Some("agent") {
        return ("skill", "fail", format!("{} is the skill of a tool: the harness was connected as one", sk.id));
    }
    let live = matches!(row.get("dialect").and_then(toml::Value::as_str), Some("acp" | "pi-rpc" | "claude-stream"));
    let resumes = live || row.get("resume").and_then(toml::Value::as_array).is_some_and(|a| !a.is_empty());
    let flag = |k: &str| truth.get(k).and_then(toml::Value::as_bool).unwrap_or(false);
    let mut lacks: Vec<&str> = Vec::new();
    if !(sk.description.starts_with("Read before") && sk.description.contains(id) && sk.when.contains(id)) {
        lacks.push("when to read it");
    }
    if !sk.body.contains(&format!("--doer {id}")) {
        lacks.push("how to name it");
    }
    let prompt = match (output_axis, prompt_axis) {
        ("http", _) => Some("posted to a local server"),
        (_, "stdin") => Some("on its standard input"),
        (_, "env") => Some("in an environment variable"),
        (_, "positional" | "flag" | "subcommand") => Some("as an argument"),
        _ => None,
    };
    if prompt.is_some_and(|p| !sk.body.contains(&format!("- prompt: {p}"))) || (prompt.is_none() && !sk.body.contains("- prompt: ")) {
        lacks.push("how the prompt goes in");
    }
    if !sk.body.contains(if resumes { "a later line carries on in its session" } else { "it keeps no session between lines" }) {
        lacks.push("its session");
    }
    if flag("can_write") && !sk.body.contains("goes to the Gate first") {
        lacks.push("the Gate");
    }
    if lacks.is_empty() { ("skill", "pass", format!("{}, {} tokens", sk.id, sk.cost)) } else { ("skill", "fail", format!("{} lacks {}", sk.id, lacks.join(", "))) }
}

/// Whether J4n's last reading of the connector's limit is the harness's own: its truth is `limit.json`, written at its last turn. The fraction used and the fraction
/// left (J4n's reading carries one fraction, so the harness's used and left are each over its cap) are within 0.1 of it; the time the window ends is within a tenth of
/// the time that was left (and no less than 5 s); and a harness that refused is marked limited. Returns whether it holds and a line that says what each was.
pub fn limit_verdict(log: &[Value], doer: &str, truth: &Value) -> (bool, String) {
    let read = log.iter().rev().find(|l| l["kind"] == "run.window" && l["by"]["doer"] == doer);
    let Some(read) = read else { return (false, format!("the harness had {} left of {}; J4n logged no reading for it", truth["left"], truth["cap"])) };
    let (cap, used, left, reset_s, at) = (truth["cap"].as_f64().unwrap_or(0.0), truth["used"].as_f64().unwrap_or(0.0), truth["left"].as_f64().unwrap_or(0.0), truth["reset_s"].as_f64().unwrap_or(0.0), truth["at"].as_f64().unwrap_or(0.0));
    let (util, ends, status) = (read["data"]["utilization"].as_f64().unwrap_or(-1.0), read["data"]["resets_at"].as_f64().unwrap_or(0.0), read["data"]["status"].as_str().unwrap_or_default());
    let used_ok = cap > 0.0 && (util - used / cap).abs() <= 0.1;
    let left_ok = cap > 0.0 && ((1.0 - util) - left / cap).abs() <= 0.1;
    let reset_ok = (ends - (at + reset_s)).abs() <= (0.1 * reset_s).max(5.0);
    let limited_ok = truth["limited"] != true || status.starts_with("rejected");
    let said = format!("harness: {used}/{cap} used, {left} left, ends in {reset_s:.0}s, limited {}; J4n: {util} used, ends in {:.0}s, {status}", truth["limited"], ends - at);
    (used_ok && left_ok && reset_ok && limited_ok, said)
}

fn connectable(truth: &toml::Table) -> bool {
    truth.get("connectable").and_then(toml::Value::as_bool).unwrap_or(true)
}

/// The connector the flow wrote for `program`: its id, and what it is (`agent text`).
fn connector_of(s: &Seed, program: &str) -> Option<(String, String)> {
    let dirs = [s.home.join("connectors"), j4n_core::gate::grant::here_dir(&s.home, &s.project).join("connectors")];
    for dir in dirs {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let text = std::fs::read_to_string(e.path()).unwrap_or_default();
            let Ok(t) = text.parse::<toml::Table>() else { continue };
            let row = t.get("connector").and_then(|c| c.as_array()).and_then(|a| a.first()).and_then(toml::Value::as_table).unwrap_or(&t);
            let (id, prog) = (row.get("id").and_then(toml::Value::as_str).unwrap_or_default(), row.get("program").and_then(toml::Value::as_str).unwrap_or_default());
            if id != "lab-drafter" && Path::new(prog).file_name().is_some_and(|n| n == program) {
                let kind = |k: &str| row.get(k).and_then(toml::Value::as_str).unwrap_or("?").to_string();
                return Some((id.to_string(), format!("{} {}", kind("is"), kind("dialect"))));
            }
        }
    }
    None
}

/// Why J4n said it did not connect, in the one line it said it in.
fn refusal(e: &Exec) -> String {
    let all = format!("{}\n{}", e.out, e.err);
    let said = |pats: &[&str]| all.lines().find(|l| pats.iter().any(|p| l.contains(p)));
    // the flow's own end first; the lines of the lab's `:accept` that had nothing to accept come after it and say less
    said(&["Not connected", "is not on the PATH", "not right", "still not"])
        .or_else(|| all.lines().filter(|l| !l.contains("no open proposal")).rev().find(|l| !l.trim().is_empty()))
        .map(clip)
        .unwrap_or_default()
}

/// The answers of a piped run: what follows each `▌you:` line.
fn answers(out: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for line in out.lines() {
        if line.starts_with(MARK) {
            found.push(String::new());
        } else if let Some(last) = found.last_mut() {
            if !last.is_empty() {
                last.push('\n');
            }
            last.push_str(line);
        }
    }
    found.into_iter().map(|a| a.trim().to_string()).collect()
}

/// Whether an answer is the word and nothing else.
fn bare(answer: &str, word: &str) -> bool {
    answer.trim().trim_matches(|c: char| !c.is_alphanumeric()).eq_ignore_ascii_case(word)
}

fn clip(s: &str) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    one.chars().take(160).collect()
}

/// What the seed's model runs cost, from its Log: the `usd` the finished tasks name, once each.
fn spent(s: &Seed) -> f64 {
    let mut by_task: BTreeMap<String, f64> = BTreeMap::new();
    for l in s.log_lines() {
        if let (Some(task), Some(usd)) = (l["task"].as_str(), l["data"]["usd"].as_f64()) {
            let e = by_task.entry(task.to_string()).or_insert(0.0);
            *e = e.max(usd);
        }
    }
    // an empty sum is `-0.0`, which prints as `$-0.00`
    by_task.values().sum::<f64>() + 0.0
}

// ---- the table ----

/// The checks by axis and value, the seen values each on a line and the held-out ones together (their names stay in the forge), and
/// the seeds of the seen families (no held-out value) against those that carry one.
pub fn table(done: &[Done], run: &str, drafter: &str) -> String {
    let cell = |rows: &[&Done], check: &str| {
        let (mut p, mut t) = (0, 0);
        for d in rows {
            match d.results.iter().find(|r| r.0 == check).map(|r| r.1) {
                Some("pass") => (p, t) = (p + 1, t + 1),
                Some("fail") => t += 1,
                _ => {}
            }
        }
        if t == 0 { "-".to_string() } else { format!("{p}/{t}") }
    };
    let mut out = format!("lab run {run}: {} seeds, drafter {drafter}, spent ${:.2}\n\n", done.len(), done.iter().map(|d| d.usd).sum::<f64>() + 0.0);
    out.push_str(&format!("{:<24}{:>5}", "axis value", "n"));
    let checks: &[&str] = if done.iter().any(|d| d.results.iter().any(|r| r.0 == "placed")) { &TOOL_CHECKS } else { &CHECKS };
    for c in checks {
        out.push_str(&format!("{:>14}", c));
    }
    out.push('\n');
    let line = |label: String, rows: &[&Done], out: &mut String| {
        if rows.is_empty() {
            return;
        }
        out.push_str(&format!("{label:<24}{:>5}", rows.len()));
        for c in checks {
            out.push_str(&format!("{:>14}", cell(rows, c)));
        }
        out.push('\n');
    };
    for axis in ["a", "b", "c", "d", "e", "f", "g", "t1", "t2", "t3", "t4", "t5"] {
        let mut values: Vec<&str> = done.iter().filter_map(|d| d.axes.get(axis).map(String::as_str)).collect();
        values.sort();
        values.dedup();
        let held = |d: &Done, v: &str| d.held.contains(&format!("{axis}:{v}"));
        for v in values.iter().filter(|v| !done.iter().any(|d| d.axes.get(axis).map(String::as_str) == Some(**v) && held(d, v))) {
            let rows: Vec<&Done> = done.iter().filter(|d| d.axes.get(axis).map(String::as_str) == Some(*v)).collect();
            line(format!("{axis}:{v}"), &rows, &mut out);
        }
        let rows: Vec<&Done> = done.iter().filter(|d| d.held.iter().any(|h| h.starts_with(&format!("{axis}:")))).collect();
        line(format!("{axis}: held out"), &rows, &mut out);
    }
    out.push('\n');
    let seen: Vec<&Done> = done.iter().filter(|d| d.held.is_empty()).collect();
    let held: Vec<&Done> = done.iter().filter(|d| !d.held.is_empty()).collect();
    line("SEEN families".into(), &seen, &mut out);
    line("HELD-OUT families".into(), &held, &mut out);
    out
}
