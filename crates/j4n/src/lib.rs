//! The binary: wiring, the command line, the restart guard. It builds the pieces the other crates
//! define and hands them to each other; it decides nothing a line means (that is `j4n_core::front`).
//!
//! - [`cli`]: the arguments; [`pack`]: the base pack, compiled in; [`wire`]: the pieces, wired;
//! - [`lines`]: `j4n run` and the piped mode, one thread of lines in and the answers out.

pub mod author;
pub mod cli;
pub mod doctor;
pub mod guard;
pub mod lines;
pub mod pack;
pub mod relay;
pub mod version;
pub mod wire;

use cli::Cmd;
use std::io::{IsTerminal, Write};

/// Run with these arguments; the exit status. 0: it went as asked. 1: a task did not finish, or was not taken. 2: the
/// arguments make no sense.
pub fn run(args: Vec<String>) -> i32 {
    let cmd = match cli::parse(&args) {
        Ok(c) => c,
        Err(why) => {
            eprintln!("j4n: {why}\nTry `j4n --help`.");
            return 2;
        }
    };
    // a shell inside a task of a running J4n (the kit's doers): a command that would open that task's own Log is a second writer; say what works
    if matches!(cmd, Cmd::Run(..) | Cmd::Word(..) | Cmd::Chat(..) | Cmd::Look { .. } | Cmd::Answer { gate: false, .. }) && opens_task_log() {
        eprintln!("j4n: this folder's Log is held by the running J4n whose task this shell is inside; here only `j4n py -c CODE` works (`import j4n` in the code; `print(j4n.tools())` lists the tools, `help(j4n)` the rest). Another folder runs as usual");
        return 2;
    }
    match cmd {
        Cmd::Version => {
            println!("j4n {}", version::line());
            0
        }
        Cmd::Help(text) => {
            print!("{text}");
            0
        }
        Cmd::Pack(cmd) => author::run(&cmd),
        Cmd::LabDraft(prompt) => {
            println!("{}", j4n_lab::draft(&prompt));
            0
        }
        Cmd::Lab(args) => lab_run(&args),
        Cmd::Doctor => match wire::Setup::from_env() {
            Ok(setup) => doctor::run(&setup),
            Err(why) => {
                eprintln!("j4n: {why}");
                1
            }
        },
        Cmd::Answer { by, gate } => answer(by.as_deref(), gate),
        Cmd::Look { doer, model, line, forced } => look(doer.as_deref(), model.as_deref(), &line, &forced),
        Cmd::Py { task, code, timeout } => py(task.as_deref(), &code, timeout),
        Cmd::Relay(argv) => relay::run(&argv),
        Cmd::Run(opts, goal) => with_world(&opts, false, &fresh("run"), |w| lines::drive_goal(w, &opts, &goal.join(" "), &mut std::io::stdout(), &mut std::io::stderr())),
        Cmd::Word(opts, words) => with_world(&opts, false, &fresh("run"), |w| match w.front.lead(&words[0]) {
            Some(lead) => {
                let line = std::iter::once(lead).chain(words[1..].iter().cloned()).collect::<Vec<_>>().join(" ");
                lines::drive_word(w, &opts, &line, &mut std::io::stdout(), &mut std::io::stderr())
            }
            None => {
                eprintln!("j4n: `{}` is not a command (`run` is one; so are those of `j4n pack`; lines for a thread go on stdin)\nTry `j4n --help`.", words[0]);
                2
            }
        }),
        Cmd::Chat(opts) => {
            let tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
            let thread = if tty { "main".to_string() } else { fresh("pipe") };
            // a folder J4n has not worked in: its `.j4n` is made by opening the world, so it is looked for first
            let new_folder = tty && std::env::current_dir().is_ok_and(|d| !d.join(".j4n").exists());
            let status = with_world(&opts, tty, &thread, |w| {
                if tty {
                    screen(w, new_folder.then(|| place_line(&std::env::current_dir().unwrap_or_default(), &opts)))
                } else {
                    lines::drive(w, &opts, &mut std::io::stdin().lock(), &mut std::io::stdout(), &mut std::io::stderr(), true)
                }
            });
            // `:restart`: the world is closed, so the next J4n can open the Log. One a guard started is asked for by its exit; a plain one
            // becomes the guard
            match status {
                guard::RESTART_CODE if guard::guarded() => status,
                guard::RESTART_CODE => restart(&args),
                _ => status,
            }
        }
    }
}

/// `j4n connect lab`: the lab over the seeds, its table on stdout.
fn lab_run(a: &cli::LabArgs) -> i32 {
    let exe = std::env::current_exe().unwrap_or_default();
    let forge = std::env::var_os("J4N_FORGE").map(std::path::PathBuf::from).unwrap_or_else(|| exe.with_file_name("j4n-forge"));
    let out = a.out.as_ref().map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".j4n/lab")));
    let (Some(out), true) = (out, forge.exists()) else {
        eprintln!("j4n: the lab needs the forge (`j4n-forge`, built beside j4n, or named by J4N_FORGE) and a home for its runs (--out DIR)");
        return 2;
    };
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let work = std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache/j4n-lab")).unwrap_or_else(|| out.join("work"));
    let run = a.run.clone().unwrap_or_else(|| format!("run-{ms}"));
    let opts = j4n_lab::Opts { seeds: (a.seeds.0..a.seeds.1).collect(), drafter: a.drafter.clone(), jobs: a.jobs, out, work, run, j4n: exe, forge, free_mb: 1500, swap_mb: 14_000, usd_cap: a.usd };
    match j4n_lab::Lab::new(opts) {
        Ok(l) => {
            print!("{}", l.run().1);
            0
        }
        Err(why) => {
            eprintln!("j4n: {why}");
            1
        }
    }
}

/// Whether this folder's `.j4n` is the one `J4N_IN_TASK` names, the Log of the task this shell is inside.
fn opens_task_log() -> bool {
    let theirs = std::env::var_os(j4n_connect::kitsock::IN_TASK).map(std::path::PathBuf::from).and_then(|p| p.canonicalize().ok());
    theirs.is_some() && theirs == std::env::current_dir().ok().and_then(|d| d.join(".j4n").canonicalize().ok())
}

/// `j4n answer`: a line of JSON out for each line in. The Gate's matcher needs no world; a question is put to the chain of its point
/// as a run with no operator puts it. 1 only when no line of stdin was JSON.
fn answer(by: Option<&str>, gate: bool) -> i32 {
    use std::io::BufRead;
    let go = |reply: &dyn Fn(&str) -> (String, bool)| {
        let (mut lines, mut json, mut out) = (0, false, std::io::stdout().lock());
        for line in std::io::stdin().lock().split(b'\n').map_while(Result::ok) {
            let text = String::from_utf8_lossy(&line);
            if text.trim().is_empty() {
                continue;
            }
            let (said, ok) = reply(text.trim());
            (lines, json) = (lines + 1, json || ok);
            if writeln!(out, "{said}").and_then(|()| out.flush()).is_err() {
                return 1;
            }
        }
        i32::from(lines > 0 && !json)
    };
    if gate {
        return go(&j4n_core::front::quiz::gate);
    }
    let opts = cli::Opts { doer: None, model: None, approval: None, wall: None, usd: None, mode: j4n_core::task::Mode::Write, place: None, shape: false, anywhere: false };
    with_world(&opts, false, &fresh("answer"), |w| go(&|text| j4n_core::front::quiz::answer(w.front.who(), by, text)))
}

/// `j4n look`: the read on one line, its answer as a JSON object on stdout; 1 when it could not be made (the object is `{error}`).
fn look(doer: Option<&str>, model: Option<&str>, line: &str, forced: &j4n_core::front::read::Forced) -> i32 {
    let opts = cli::Opts { doer: None, model: None, approval: None, wall: None, usd: None, mode: j4n_core::task::Mode::Read, place: None, shape: false, anywhere: false };
    with_world(&opts, false, &fresh("look"), |w| match w.front.look_once(&fresh("look"), line, doer, model, forced) {
        Ok(said) => {
            println!("{said}");
            0
        }
        Err(said) => {
            println!("{said}");
            1
        }
    })
}

/// `j4n py`: a cell for the kernel of the task whose socket is in this folder, its answer on stdout. No world is opened: the running J4n answers.
fn py(task: Option<&str>, code: &cli::PyCode, timeout: Option<u64>) -> i32 {
    let text = match code {
        cli::PyCode::Text(t) => Ok(t.clone()),
        cli::PyCode::File(f) if f == "-" => std::io::read_to_string(std::io::stdin()),
        cli::PyCode::File(f) => std::fs::read_to_string(f),
    };
    let from = std::env::current_dir().unwrap_or_default();
    match text.map_err(|e| format!("the code could not be read: {e}")).and_then(|t| j4n_connect::kitsock::ask(&from, task, &t, timeout)) {
        Ok((ok, said)) => {
            println!("{said}");
            i32::from(!ok)
        }
        Err(why) => {
            eprintln!("j4n: {why}");
            1
        }
    }
}

/// A thread name no earlier run has used.
fn fresh(kind: &str) -> String {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    format!("{kind}-{ms}")
}

/// Why J4n will not start in this folder, or none: the root of the disk and the root of `$HOME` are no project, and
/// a checkpoint, a Log and a tool's writes there reach everything the operator has.
pub fn refuse_place(dir: &std::path::Path, home: Option<&std::path::Path>) -> Option<String> {
    let real = |p: &std::path::Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let dir_real = real(dir);
    let what = if dir_real.parent().is_none() {
        "the root of the disk"
    } else if home.is_some_and(|h| real(h) == dir_real) {
        "your home folder"
    } else {
        return None;
    };
    Some(format!("this folder is {what} (`{}`), not a project; go into a project folder, or add --anywhere to work here anyway", dir.display()))
}

fn with_world(opts: &cli::Opts, interactive: bool, thread: &str, go: impl FnOnce(&wire::World) -> i32) -> i32 {
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty()).map(std::path::PathBuf::from);
    if let Some(why) = std::env::current_dir().ok().filter(|_| !opts.anywhere).and_then(|d| refuse_place(&d, home.as_deref())) {
        eprintln!("j4n: {why}");
        return 2;
    }
    match wire::Setup::from_env().and_then(|setup| wire::open(setup, opts, interactive, thread)) {
        Ok(world) => {
            for n in &world.notices {
                eprintln!("j4n: {n}");
            }
            let status = go(&world);
            world.close();
            j4n_connect::proc::end_captures(); // a question in flight ends with the process (not in `close`: tests hold many worlds)
            status
        }
        Err(why) => {
            let _ = writeln!(std::io::stderr(), "j4n: {why}");
            1
        }
    }
}

/// The one row a new folder's chat shows: the place and the mode, and, when the doctor's checks find something missing, what and where
/// to mend it (`… · pi not found · ⏎ opens Connect › Setup`, the key opening that page); none for a folder J4n has worked in before. It
/// waits for the doers to answer: it is made off the screen's way.
pub fn first_start_note(front: &j4n_core::front::Front, new_folder: bool, place: &str) -> Option<String> {
    new_folder.then(|| match front.cfg().setup.as_ref().and_then(|c| c.report_now(None).notice()) {
        Some(missing) => format!("{place} · {missing}"),
        None => place.to_string(),
    })
}

/// The place and the mode, in words: `new folder app · write here · a checkpoint before work` (the folder's name: the path is the shell's).
fn place_line(dir: &std::path::Path, opts: &cli::Opts) -> String {
    let how = match (opts.mode, opts.place.as_deref().unwrap_or("here")) {
        (j4n_core::task::Mode::Read, _) => "read only".to_string(),
        (_, "here") => "write here · a checkpoint before work".to_string(),
        (_, other) => format!("write in place {other}"),
    };
    format!("new folder {} · {how}", dir.file_name().map_or_else(|| dir.display().to_string(), |n| n.to_string_lossy().into_owned()))
}

/// Whether the screen asks the terminal to report ⌥ held alone (`keys.hold`): every word but `off` does, where the terminal speaks the kitty
/// protocol. The flags are asked once, before the screen is up, so the setting acts at the next start.
pub fn keys_hold(front: &j4n_core::front::Front) -> bool {
    !matches!(front.screen().set.pointer("/keys/hold"), j4n_core::value::Value::Str(s) if s == "off")
}

/// How many of the notices of the start the chat shows.
const NOTICES_SHOWN: usize = 8;

/// The screen: the TUI on one end of a client of the front, until the operator leaves.
fn screen(world: &wire::World, new_folder: Option<String>) -> i32 {
    let (up, down) = world.front.connect();
    // the notices of the start went to stderr, which the alternate screen covers: the first lines of the chat say them
    for n in world.notices.iter().take(NOTICES_SHOWN) {
        world.front.note("main", &format!("j4n: {n}"), "warn");
    }
    if world.notices.len() > NOTICES_SHOWN {
        world.front.note("main", &format!("j4n: and {} more notices at start; stderr has them all (`j4n 2>file`)", world.notices.len() - NOTICES_SHOWN), "warn");
    }
    if let Some(note) = guard::start_note() {
        world.front.note("main", &note, "warn");
    }
    // the Setup card (until it is answered once in this J4n home) and, in a new folder, the one row that says what is missing: both read the
    // doctor's look of the programs, so they are made off the screen's way
    let front = world.front.clone();
    std::thread::spawn(move || {
        front.setup_card_open();
        new_folder.and_then(|place| first_start_note(&front, true, &place)).into_iter().for_each(|n| front.first_note("main", &n));
    });
    match j4n_tui::term::run(down, &up, keys_hold(&world.front), guard::mark_ready) {
        Ok(()) if world.front.restart_asked() => guard::RESTART_CODE,
        Ok(()) => 0,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "j4n: the terminal failed: {e}"); // not `eprintln!`: on a terminal that is gone it panics
            1
        }
    }
}

/// Become the guard of the next J4n: the build that is running is kept to go back to, and the one on disk now is started and watched.
fn restart(args: &[String]) -> i32 {
    let (Some(running), Ok(setup)) = (guard::running_build(), wire::Setup::from_env()) else {
        eprintln!("j4n: cannot find the build that is running, so it is not restarted");
        return 1;
    };
    let guard = guard::Guard::new(&setup.home.join("guard"));
    let previous = guard.keep(&running).unwrap_or_else(|| running.clone());
    guard.supervise(previous, &|| guard::running_build().unwrap_or_else(|| running.clone()), args)
}
