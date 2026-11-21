//! The command line, by hand: no framework, and an option nobody knows is an error, never ignored.

use j4n_core::expr::Expr;
use j4n_core::task::Mode;
use std::time::Duration;

/// What an approval does when nobody is there to say yes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    /// Yes to everything the Gate does not block.
    Full,
    /// The operator is asked: in the TUI by the keys, in piped mode by the next line.
    Ask,
    /// The deterministic fallback: no.
    Deny,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Opts {
    pub doer: Option<String>,
    pub model: Option<String>,
    pub approval: Option<Approval>,
    /// The time the process has, from its start; a task stops itself, with its report, when it is up.
    pub wall: Option<Duration>,
    pub usd: Option<f64>,
    pub mode: Mode,
    /// Where the work is done: `here` (the default), `worktree` or `box`.
    pub place: Option<String>,
    /// `--shape`: the line is shaped before it is said, and the notes that held are kept (the pack `shape`; never a default).
    pub shape: bool,
    /// `--anywhere`: work in the root of `$HOME` or of the disk, which J4n otherwise refuses to start in.
    pub anywhere: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    Version,
    Help(&'static str),
    /// One task: the words after `run`.
    Run(Opts, Vec<String>),
    /// The screen, or, with no terminal, the lines on stdin.
    Chat(Opts),
    /// A command by name, as the palette lists it (`j4n pack`, `j4n plan add a flag`): the words, the first of which is its own.
    /// Whether it is one is for the commands the packs give, which are known once they are loaded.
    Word(Opts, Vec<String>),
    /// `j4n py -c CODE` / `-f FILE`: a cell for the kernel of the task whose socket is in this folder.
    Py { task: Option<String>, code: PyCode, timeout: Option<u64> },
    /// `j4n relay -- PROGRAM ARGS…`: run a program and print what it printed with every secret in it replaced. No world is opened.
    Relay(Vec<String>),
    /// `j4n pack new|check|install`: making, validating and installing a pack from a folder or a git URL. No world is opened.
    Pack(PackCmd),
    /// `j4n doctor`: the pre-flight on stdout (each doer, the plans, the key, the safety defaults); 0 when the default doer works. No world is opened.
    Doctor,
    /// `j4n answer`: probe-shaped lines on stdin, one answer line each; `by` asks one answerer of the chain, `gate` the Gate's matcher.
    Answer { by: Option<String>, gate: bool },
    /// `j4n look [--doer ID] [--model WORD] [--after] [--chats LIST] [--doers LIST] WORDS`: the read of `orchestrate` on one line, its answer as JSON on stdout.
    Look { doer: Option<String>, model: Option<String>, line: String, forced: j4n_core::front::read::Forced },
    /// `j4n connect lab --seeds A..B [--drafter scripted|haiku|sonnet] [--jobs N]`: the Connect lab. No world is opened.
    Lab(LabArgs),
    /// `j4n connect lab-draft PROMPT`: the scripted drafter of the lab, a draft made from the help text alone, on stdout.
    LabDraft(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct LabArgs {
    pub seeds: (u64, u64),
    pub drafter: String,
    pub jobs: usize,
    pub run: Option<String>,
    pub out: Option<String>,
    /// The most a run that has a model draft may spend, in dollars: no seed starts once the seeds done have cost this much.
    pub usd: f64,
}

pub const LAB_HELP: &str = "Usage: j4n connect lab --seeds A..B [--drafter scripted|haiku|sonnet] [--jobs N] [--run ID] [--out DIR] [--usd N]

For each seed the forge (`j4n-forge`, next to j4n or named by J4N_FORGE) makes a random harness; j4n is started in a temporary home with it on the
PATH, with connect.auto on and no answer given, and eight checks are scored against what the forge knows: connected, chats, resumes, tools shown,
leave asked, limit read, fails cleanly, drift. One JSONL row per seed and check goes to ~/.j4n/lab/RUN/rows.jsonl, and a table by axis and value
(held-out values together, apart from the seen ones) to table.txt and stdout. Seeds A..B are A up to B; A..=B and a single seed are taken too.
  --drafter D   who writes the connector: scripted (the default: code only, $0), haiku or sonnet (claude's limits; never opus)
  --usd N       the most a run with a model drafter may spend (default 0.50): no seed starts once the seeds done have cost this much
  --jobs N      harnesses at once (default 2, at most 2 are advised: each seed waits for 1500 MB free and swap under 14 GB)
Exit status: 0 the lab ran, 2 the arguments make no sense.
";

#[derive(Debug, Clone, PartialEq)]
pub enum PackCmd {
    New { name: String, dir: Option<String> },
    Check { path: String },
    Install { from: String },
}

/// Where the code of a `j4n py` cell comes from: the argument, or a file (`-` is stdin).
#[derive(Debug, Clone, PartialEq)]
pub enum PyCode {
    Text(String),
    File(String),
}

pub const PY_HELP: &str = "Usage: j4n py [--task ID] [--timeout SECS] (-c CODE | -f FILE)

Run Python in the kernel of the task you are working in, which keeps its variables for the task (the setting kit.mode is on; J4n opened the
task's socket in this folder). What the cell prints and the value of its last expression are printed. -f - reads the code from stdin.
Exit status: 0 the cell ran, 1 it raised an error or was refused, 2 the arguments make no sense.
";

pub const PACK_HELP: &str = "Usage: j4n pack new NAME [DIR]
       j4n pack check PATH
       j4n pack install PATH-OR-GIT-URL

  new      write a template pack called NAME (default folder ./NAME): pack.toml, a phrase, a test with a golden, a README
  check    the manifest, every kind file against its kind, the cases of tests/*.toml and their goldens; problems as FILE:LINE: WHY
           (J4N_BLESS=1 writes the goldens that differ)
  install  check first, and refuse a pack that fails; then copy it into ~/.j4n/packs/ (off until `:pack on ID`). A git URL is
           cloned shallow into a temporary folder, only for this command
Without a word after `pack`, `j4n pack` lists the packs. Exit status: 0 it went as asked, 1 it did not, 2 the arguments make no sense.
";

pub const DOCTOR_HELP: &str = "Usage: j4n doctor

What J4n has to work with, a line each: the doers (claude, codex, gemini, opencode, pi, own, light) found, signed in or missing and the one line
that mends it; the plans and their prices; whether an OpenRouter key is set (never its value); the safety defaults in words; and the doer a first
election falls to. Nothing is started or written. Agents › Overview shows the same in the screen.
Exit status: 0 the default doer works, 1 none does, 2 the arguments make no sense.
";

pub const HELP: &str = "j4n — one operator, one thread, any doer.

Usage:
  j4n [options]            the screen, in a terminal; with lines on stdin (piped mode), each line is one turn of one
                           thread: the same doer, the same model, the same session
  j4n run [options] WORDS  one task, its answer on stdout
  j4n [options] COMMAND [WORDS]
                           one of the commands the packs give (`j4n pack`, `j4n plan add a flag`), as the palette lists them; its answer is
                           on stdout (`j4n report main > file`), progress and refusals on stderr
  j4n doctor               what there is to work with: each doer found, signed in or missing and how to mend it, the plans, the safety defaults
  j4n answer [--by ID | --gate]
                           questions as JSON lines on stdin, an answer line each (for an outside test harness to ask a decider; no task is started)
  j4n look [--doer ID] [--model WORD] WORDS
                           what the quick look of the orchestrate mode makes of one line, as JSON on stdout (the line is sent to no chat)
  j4n --version

Options:
  --doer ID        the doer: claude, pi, omp, test, or a connector of yours (left out, the ready ones are ranked and one is chosen, once)
  --model NAME     the doer's model
  --approval WHO   full: yes to what the Gate does not block · ask: the operator answers (piped mode: the next line)
                   · deny: no (default when there is no terminal)
  --wall DUR       stop before this much time has passed (90s, 15m, 1h), with the report so far
  --usd N          soft spend limit in dollars, for the task and everything it starts (a flow's steps count): past it the task is
                   parked, and a run that goes on to one and a half times it is stopped
  --mode MODE      write (default) or read
  --place PLACE    where the work is done: here (default; the folder itself, with a checkpoint to rewind) · worktree (a copy of
                   its own, laid into the folder when it is over) · box (here, behind a wall) · worktree-box
  --shape          shape each line first (the pack `shape`): the notes that hold are sent with it, and listed on stderr
  --anywhere       work in your home folder or in `/` itself (refused without it: go into a project folder instead)

Piped mode prints `▌you: <line>` and then the answer for each line. Exit status: 0 it went as asked, 1 a task did not
finish or a line was not taken, 2 the arguments make no sense.
";

pub const RUN_HELP: &str = "Usage: j4n run [options] WORDS

Run one task for WORDS in this folder and print its answer.

Options:
  --doer ID        the doer: claude, pi, omp, test, or a connector of yours
  --model NAME     the doer's model
  --approval WHO   full | ask | deny (default deny: nobody is there to ask)
  --wall DUR       stop before this much time has passed (90s, 15m, 1h), keeping the report so far
  --usd N          soft spend limit in dollars, for the task and everything it starts: parked past it, stopped at 1.5 times it
  --mode MODE      write (default) or read
  --place PLACE    where the work is done: here (default; the folder itself, with a checkpoint to rewind) · worktree (a copy of
                   its own, laid into the folder when it is over) · box (here, behind a wall) · worktree-box
  --shape          shape the line first (the pack `shape`); what was added is listed on stderr, the answer stays on stdout
  --anywhere       work in your home folder or in `/` itself (refused without it: go into a project folder instead)

Exit status: 0 the task finished, 1 it did not (a plan that was not carried out, or a planning turn that gave none, is 1 too: the goal was
never worked on), 2 the arguments make no sense.
";

pub const LOOK_HELP: &str = "Usage: j4n look [--doer ID] [--model WORD] [--after] [--chats LIST] [--doers LIST] WORDS

The read of the orchestrate mode, alone: the Deciders are asked what the line is (answered, worked in one lane, or split into chats), whether it goes on
from the last answer, which open chat it is about and which doer it asks for; a cheap model with no tool that writes is shown the line and what this project
offers (its features, doers and skills) and is asked only what they left open. The answer is one JSON object on stdout: {decision, taken, clamped, chats, features,
why, read, doer, model, usd, ms, tokens}, where `decision` is what was read and `taken` what J4n makes of it (a split of reads only stays one lane, a standing rule is
never a reply), and `read` says for each question the answer and who gave it. Nothing is sent to a chat and no session is kept; the model step is one read-only task of
a thread of its own, which the Log keeps.
  --doer ID     the doer that makes the look (left out: the setting orchestrate.look_doer, else the cheap tier's)
  --model WORD  its model, as a word of the doer (`haiku`)
  --after       put the question of whether the line goes on from the last answer, as if the thread had one
  --chats LIST  put the question of which open chat the line is about: `c1: first line; c2: first line`
  --doers LIST  the doers the line may ask for, `claude,pi` (left out: the ones that are ready)
Exit status: 0 the look was made, 1 it was not (the object is {error}), 2 the arguments make no sense.
";

pub const ANSWER_HELP: &str = "Usage: j4n answer [--by ID | --gate]

JSON lines on stdin, one JSON line out for each; no task is started. A question is {\"id\", \"point\", \"slices\", \"offered\"} (a probe
case), answered {id, answer, by, confidence, ms, usd, why, waited_ms, steps} by the chain of its point as a run with no operator walks it.
  --by ID    one answerer of the chain only (`rule:intake`, `decider:jev`); the point's fallback does not stand in
  --gate     lines are {\"id\", \"gate\": {\"cmd\"}}, answered {id, class, detail} by the Gate's matcher
A line that cannot be taken is answered {id, error}. Exit status: 0, 1 no line of stdin was JSON, 2 the arguments make no sense.
";

/// A duration as the Table writes one (`90s`, `15m`, `1h30m`, `250ms`, `2d`: `Expr::dur`), or a bare number of seconds.
pub fn duration(text: &str) -> Result<Duration, String> {
    let bad = || format!("`{text}` is not a time (try 90s, 15m or 1h30m)");
    let t = text.trim();
    let bare = !t.is_empty() && t.chars().all(|c| c.is_ascii_digit() || c == '.');
    let secs = if bare { t.parse::<f64>().map_err(|_| bad())? } else { Expr::dur(t).ok_or_else(bad)? as f64 / 1000.0 };
    if !secs.is_finite() || secs <= 0.0 || secs > 1e7 {
        return Err(bad());
    }
    Ok(Duration::from_secs_f64(secs))
}

fn answer(args: &[String]) -> Result<Cmd, String> {
    let (mut by, mut gate) = (None, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.split_once('=').filter(|(n, _)| n.starts_with("--")).unwrap_or((a.as_str(), "")) {
            ("--help" | "-h", _) => return Ok(Cmd::Help(ANSWER_HELP)),
            ("--gate", "") => gate = true,
            ("--by", inline) => match Some(inline.to_string()).filter(|v| !v.is_empty()).or_else(|| it.next().cloned()) {
                Some(v) if !v.is_empty() => by = Some(v),
                _ => return Err("`--by` needs a value".into()),
            },
            _ if a.starts_with('-') => return Err(format!("unknown option `{a}` for `answer`")),
            _ => return Err(format!("`answer` takes no words (`{a}`): the questions are lines on stdin")),
        }
    }
    if gate && by.is_some() {
        return Err("`--gate` and `--by` do not go together: the matcher is the one answerer".into());
    }
    Ok(Cmd::Answer { by, gate })
}

fn look(args: &[String]) -> Result<Cmd, String> {
    let (mut doer, mut model, mut words) = (None, None, Vec::new());
    let mut forced = j4n_core::front::read::Forced::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.split_once('=').filter(|(n, _)| n.starts_with("--")).unwrap_or((a.as_str(), "")) {
            ("--help" | "-h", _) => return Ok(Cmd::Help(LOOK_HELP)),
            (name @ ("--doer" | "--model"), inline) => match Some(inline.to_string()).filter(|v| !v.is_empty()).or_else(|| it.next().cloned()) {
                Some(v) if !v.is_empty() => *(if name == "--doer" { &mut doer } else { &mut model }) = Some(v),
                _ => return Err(format!("`{name}` needs a value")),
            },
            ("--after", _) => forced.after = true,
            (name @ ("--chats" | "--doers"), inline) => match Some(inline.to_string()).filter(|v| !v.is_empty()).or_else(|| it.next().cloned()) {
                Some(v) if name == "--doers" => forced.doers = v.split(',').map(|d| d.trim().to_string()).filter(|d| !d.is_empty()).collect(),
                // `c1: the first line; c2: the second`
                Some(v) => forced.chats = v.split(';').filter_map(|c| c.split_once(':')).map(|(id, t)| (id.trim().to_string(), t.trim().to_string())).collect(),
                None => return Err(format!("`{name}` needs a value")),
            },
            _ if a.starts_with('-') && words.is_empty() => return Err(format!("unknown option `{a}` for `look`")),
            _ => words.push(a.clone()),
        }
    }
    if words.is_empty() {
        return Err("`look` needs the words of the line".into());
    }
    Ok(Cmd::Look { doer, model, line: words.join(" "), forced })
}

fn pack(args: &[String]) -> Result<Cmd, String> {
    let (sub, rest) = (args[0].as_str(), &args[1..]);
    if rest.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(Cmd::Help(PACK_HELP));
    }
    if let Some(o) = rest.iter().find(|a| a.starts_with('-') && a.len() > 1) {
        return Err(format!("unknown option `{o}` for `pack {sub}`"));
    }
    let words = |most: usize, what: &str| if rest.is_empty() || rest.len() > most { Err(format!("`pack {sub}` takes {what}")) } else { Ok(()) };
    Ok(Cmd::Pack(match sub {
        "new" => {
            words(2, "a name, and optionally a folder")?;
            PackCmd::New { name: rest[0].clone(), dir: rest.get(1).cloned() }
        }
        "check" => {
            words(1, "the path of a pack folder")?;
            PackCmd::Check { path: rest[0].clone() }
        }
        _ => {
            words(1, "a folder or a git URL")?;
            PackCmd::Install { from: rest[0].clone() }
        }
    }))
}

fn py(args: &[String]) -> Result<Cmd, String> {
    let (mut task, mut code, mut timeout) = (None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| it.next().cloned().filter(|v| !v.is_empty() || name == "-c").ok_or_else(|| format!("`{name}` needs a value"));
        match a.as_str() {
            "--help" | "-h" => return Ok(Cmd::Help(PY_HELP)),
            "-c" => code = Some(PyCode::Text(value("-c")?)),
            "-f" => code = Some(PyCode::File(value("-f")?)),
            "--task" => task = Some(value("--task")?),
            "--timeout" => timeout = Some(value("--timeout")?.parse::<u64>().ok().filter(|n| *n > 0).ok_or("`--timeout` is a number of seconds")?),
            other => return Err(format!("unknown option `{other}` for `py`")),
        }
    }
    Ok(Cmd::Py { task, code: code.ok_or("`py` needs -c CODE or -f FILE")?, timeout })
}

fn lab(args: &[String]) -> Result<Cmd, String> {
    if args.first().map(String::as_str) == Some("lab-draft") {
        return Ok(Cmd::LabDraft(args[1..].join(" ")));
    }
    let mut a = LabArgs { seeds: (0, 8), drafter: "scripted".into(), jobs: 2, run: None, out: None, usd: 0.5 };
    let mut it = args[1..].iter();
    while let Some(f) = it.next() {
        if f == "--help" || f == "-h" {
            return Ok(Cmd::Help(LAB_HELP));
        }
        let v = it.next().ok_or_else(|| format!("`{f}` needs a value"))?;
        match f.as_str() {
            "--seeds" => a.seeds = j4n_lab::seeds(v)?,
            "--drafter" if matches!(v.as_str(), "scripted" | "haiku" | "sonnet") => a.drafter = v.clone(),
            "--drafter" => return Err(format!("`--drafter {v}`: it is scripted, haiku or sonnet (never opus)")),
            "--jobs" => a.jobs = v.parse::<usize>().ok().filter(|n| (1..=4).contains(n)).ok_or("`--jobs` is 1 to 4")?,
            "--run" => a.run = Some(v.clone()),
            "--out" => a.out = Some(v.clone()),
            "--usd" => a.usd = v.parse::<f64>().ok().filter(|u| u.is_finite() && *u >= 0.0).ok_or_else(|| format!("`--usd {v}` is not an amount of dollars"))?,
            other => return Err(format!("unknown option `{other}` for `connect lab`")),
        }
    }
    Ok(Cmd::Lab(a))
}

pub fn parse(args: &[String]) -> Result<Cmd, String> {
    if args.first().map(String::as_str) == Some("connect") && matches!(args.get(1).map(String::as_str), Some("lab" | "lab-draft")) {
        return lab(&args[1..]);
    }
    if args.first().map(String::as_str) == Some("pack") && matches!(args.get(1).map(String::as_str), Some("new" | "check" | "install")) {
        return pack(&args[1..]);
    }
    if args.first().map(String::as_str) == Some("doctor") {
        return match &args[1..] {
            [] => Ok(Cmd::Doctor),
            [h] if h == "--help" || h == "-h" => Ok(Cmd::Help(DOCTOR_HELP)),
            [other, ..] => Err(format!("`doctor` takes nothing (`{other}`)")),
        };
    }
    if args.first().map(String::as_str) == Some("py") {
        return py(&args[1..]);
    }
    if args.first().map(String::as_str) == Some("relay") {
        let rest = &args[1..];
        return Ok(Cmd::Relay(rest.strip_prefix(&["--".to_string()]).unwrap_or(rest).to_vec()));
    }
    if args.first().map(String::as_str) == Some("answer") {
        return answer(&args[1..]);
    }
    if args.first().map(String::as_str) == Some("look") {
        return look(&args[1..]);
    }
    let (run, rest) = match args.first().map(String::as_str) {
        Some("run") => (true, &args[1..]),
        _ => (false, args),
    };
    let mut opts = Opts { doer: None, model: None, approval: None, wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let mut words = Vec::new();
    let mut it = rest.iter();
    let mut flags = true;
    while let Some(a) = it.next() {
        if !flags || !a.starts_with('-') || a == "-" {
            words.push(a.clone());
            continue;
        }
        if a == "--" {
            flags = false;
            continue;
        }
        let (name, inline) = match a.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v.to_string())),
            _ => (a.as_str(), None),
        };
        match name {
            "--version" | "-V" => return Ok(Cmd::Version),
            "--shape" if inline.is_none() => opts.shape = true,
            "--anywhere" if inline.is_none() => opts.anywhere = true,
            "--help" | "-h" => return Ok(Cmd::Help(if run { RUN_HELP } else { HELP })),
            "--doer" | "--model" | "--approval" | "--wall" | "--usd" | "--mode" | "--place" => {
                let value = match inline.or_else(|| it.next().cloned()) {
                    Some(v) if !v.is_empty() => v,
                    _ => return Err(format!("`{name}` needs a value")),
                };
                match name {
                    "--doer" => opts.doer = Some(value),
                    "--model" => opts.model = Some(value),
                    "--place" => opts.place = Some(value),
                    "--approval" => {
                        opts.approval = Some(match value.as_str() {
                            "full" => Approval::Full,
                            "ask" => Approval::Ask,
                            "deny" => Approval::Deny,
                            other => return Err(format!("`--approval {other}`: it is full, ask or deny")),
                        })
                    }
                    "--wall" => opts.wall = Some(duration(&value)?),
                    "--usd" => {
                        opts.usd = Some(value.parse::<f64>().ok().filter(|u| u.is_finite() && *u > 0.0).ok_or_else(|| format!("`--usd {value}` is not an amount of dollars"))?)
                    }
                    _ => {
                        opts.mode = match value.as_str() {
                            "read" => Mode::Read,
                            "write" => Mode::Write,
                            other => return Err(format!("`--mode {other}`: it is read or write")),
                        }
                    }
                }
            }
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    if run {
        if words.is_empty() {
            return Err("`run` needs the words of the task".into());
        }
        return Ok(Cmd::Run(opts, words));
    }
    if !words.is_empty() {
        return Ok(Cmd::Word(opts, words));
    }
    Ok(Cmd::Chat(opts))
}
