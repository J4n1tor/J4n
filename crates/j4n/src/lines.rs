//! `j4n run` and the piped mode: lines in, answers out. Each line is said to the front like a line typed on the screen, so
//! the thread, the binding, the steering and the answering are the same; this only waits for each turn to end and prints
//! its answer. What happened on the way (tools, commands, notices) goes to stderr as it is written to the Log; but a command word's
//! own answer (`j4n report main`: the note its command wrote) is the output, and goes to stdout.

use crate::cli::{Approval, Opts};
use crate::wire::World;
use j4n_core::front::Said;
use j4n_core::front::chat::Chat;
use j4n_core::log::Sub;
use j4n_core::task::{Ctl, Row, State};
use j4n_core::task::Why;
use j4n_proto::{Item, Role, Tone};
use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

/// The line that starts each turn's answer in piped mode.
pub const MARK: &str = "▌you: ";

/// What went on, for stderr: the lines of the chat that are not the answer.
fn progress(sub: &Sub, chat: &mut Chat, thread: &str, err: &mut dyn Write) {
    drain(sub, chat, thread, None, err);
}

/// The lines of the chat so far. With `out` (the line was a command, said by a command word), the notes it wrote are its answer and
/// go there; a refusal or a warning, and everything else, goes to `err`.
fn drain(sub: &Sub, chat: &mut Chat, thread: &str, mut out: Option<&mut dyn Write>, err: &mut dyn Write) {
    while let Some(ev) = sub.try_recv() {
        if ev.thread.as_deref() != Some(thread) {
            continue;
        }
        for item in chat.items(&ev) {
            if let Item::Put { line } = item
                && !line.partial
                && !matches!(line.role, Role::You | Role::Doer | Role::Reasoning)
            {
                let answer = line.role == Role::Notice && !matches!(line.tone, Tone::Warn | Tone::Bad);
                match out.as_deref_mut().filter(|_| answer) {
                    Some(out) => {
                        let _ = writeln!(out, "{}", line.text);
                    }
                    None => {
                        let _ = writeln!(err, "{}", line.text);
                    }
                }
            }
        }
    }
}

/// How a run takes its lines: one goal (`j4n run`), piped lines with `▌you:` marks, or a command word's one line (`j4n report main`).
#[derive(Clone, Copy, PartialEq)]
enum Form {
    Goal,
    Marked,
    Word,
}

/// The turns of `input`; the exit status. With `marked`, each turn's answer follows a `▌you:` line (piped mode).
pub fn drive(world: &World, opts: &Opts, input: &mut dyn BufRead, out: &mut dyn Write, err: &mut dyn Write, marked: bool) -> i32 {
    turns(world, opts, None, input, out, err, if marked { Form::Marked } else { Form::Goal })
}

/// The command `line` of a command word (`pack`, `report main`): what its command says is the answer, on stdout; progress, refusals
/// and the end of a task it starts are as in [`drive`]. The exit status.
pub fn drive_word(world: &World, opts: &Opts, line: &str, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    turns(world, opts, None, &mut line.as_bytes(), out, err, Form::Word)
}

/// One turn for `goal`, the whole of it: a goal of many lines (a task written out, with its requirements and where the files go) is
/// one message and not a line each, so the doer has all of it from the first word. The exit status.
pub fn drive_goal(world: &World, opts: &Opts, goal: &str, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    turns(world, opts, Some(goal.to_string()), &mut std::io::empty(), out, err, Form::Goal)
}

/// Whether the time the process was given (`--wall`) has passed.
fn out_of_time(world: &World) -> bool {
    world.front.cfg().deadline.is_some_and(|end| end <= Instant::now())
}

/// The run's own time is out, or all but: a task's wall is the time left when it started, so the task the run's clock stops ends at the deadline,
/// a moment before the next look at the clock (two seconds of slack for the timer's own clock).
fn at_the_clock(world: &World) -> bool {
    world.front.cfg().deadline.is_some_and(|end| end <= Instant::now() + Duration::from_secs(2))
}

fn turns(world: &World, opts: &Opts, mut goal: Option<String>, input: &mut dyn BufRead, out: &mut dyn Write, err: &mut dyn Write, form: Form) -> i32 {
    let front = &world.front;
    let thread = front.cfg().thread.clone();
    let sub = front.log().subscribe("*", 65536);
    let mut chat = Chat::new();
    let ask = opts.approval == Some(Approval::Ask);
    let mut status = 0;
    let mut line = String::new();
    loop {
        line.clear();
        let text = match goal.take() {
            Some(whole) => whole.trim().to_string(),
            None => match input.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => line.trim().to_string(),
            },
        };
        if text.is_empty() {
            continue;
        }
        // once the time is out nothing more is started: the lines still to come are not read, and it is said once
        if out_of_time(world) {
            let _ = writeln!(err, "j4n: the time it was given has run out");
            status = 1;
            break;
        }
        if form == Form::Marked {
            let _ = writeln!(out, "{MARK}{text}");
        }
        // a line that answers a question, or is a command, or is refused, starts no task: there is nothing to wait for
        // with `--shape` the line is shaped first and the notes that held go with it; the report is for stderr, the answer stays on stdout
        let keys = if opts.shape && !text.starts_with(':') && !text.starts_with('/') {
            let (keys, report) = front.shape_run(&thread, &text);
            let _ = writeln!(err, "{report}");
            keys
        } else {
            Vec::new()
        };
        // what is already written is progress: the answer of a command word is what its command writes next
        if form == Form::Word {
            progress(&sub, &mut chat, &thread, err);
        }
        let mut said = front.say_with(&thread, &text, &keys);
        // a reflex's judge looks at the line first: the run waits for the line to go, and takes what came of it
        if said == Said::Held {
            said = front.held_wait(&thread);
        }
        let task = match said {
            Said::Command(_) if form == Form::Word => {
                drain(&sub, &mut chat, &thread, Some(&mut *out), err);
                None
            }
            Said::Started { task } | Said::Steered { task } => Some(task),
            Said::Quit => break,
            Said::Refused(why) => {
                let _ = writeln!(err, "j4n: {why}");
                status = 1;
                None
            }
            _ => None,
        };
        // under `orchestrate` a line may start chats and no task of its own (a split whose parts all went to chats): they are the work, waited for as a plan's items are
        let task = task.or_else(|| chats_at_work(world, &sub, &mut chat, &thread, err));
        let Some(task) = task else {
            progress(&sub, &mut chat, &thread, err);
            if front.main_works() {
                status |= plan_end(front, &thread, false, err);
            }
            continue;
        };
        let mut planned = false;
        let mut next = Some(task);
        while let Some(task) = next.take() {
            match follow(world, &sub, &mut chat, &thread, task, (ask, &mut *input), out, err) {
                Followed::Wall(s) => return status | s,
                Followed::Ended { status: s, planning } => {
                    status |= s;
                    planned |= planning;
                }
            }
            // a plan the line began is the work, and nothing but this loop starts its items when no screen does
            if out_of_time(world) {
                if front.keeper().and_then(|k| k.plan(&thread)).is_some_and(|p| p.open()) {
                    let _ = writeln!(err, "j4n: the time it was given has run out; the plan was left unfinished");
                    status = 1;
                }
                break;
            }
            next = plan_next(front, &thread);
        }
        status |= plan_end(front, &thread, planned, err);
    }
    progress(&sub, &mut chat, &thread, err);
    status
}

/// A task that waits out a limit that ends after the time the process was given (`--wall`) is ended now, and the line says so: the program, when its
/// limit ends, and what it said. A wait that fits in the time is let be (the task goes on by itself).
fn limit_past_the_clock(world: &World, task: &str, end_it: bool) -> Option<String> {
    let front = &world.front;
    let end = front.cfg().deadline?;
    let State::Cooling { until_ms } = front.tasks().book().get(task)?.state else { return None };
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as u64;
    let left = end.saturating_duration_since(Instant::now()).as_millis() as u64;
    if until_ms.saturating_sub(now_ms) <= left {
        return None;
    }
    if !end_it {
        return Some(String::new());
    }
    let doer = front.tasks().book().get(task).map(|r| r.spec.doer).unwrap_or_default();
    let said = front.log().read(0).filter(|e| e.kind == "run.error" && e.task.as_deref() == Some(task)).filter_map(|e| e.data["text"].as_str().map(String::from)).last().unwrap_or_default();
    let _ = front.tasks().ctl(task, Ctl::Abort, j4n_core::change::Actor::Core);
    let until = format!("in {}", j4n_core::fmt::duration(until_ms.saturating_sub(now_ms)));
    Some(format!("{doer} is limited until {until}{}", if said.is_empty() { String::new() } else { format!(": {}", j4n_core::text::clip(&said, 200)) }))
}

/// How one task of a run ended.
enum Followed {
    /// The wall passed: the status so far, and nothing after it is taken.
    Wall(i32),
    Ended { status: i32, planning: bool },
}

/// Wait for `task` to end, printing what happened on the way, answering a question when the run may ask, and following what a rule
/// carried it on with and what was started beside it.
#[allow(clippy::too_many_arguments)]
fn follow(world: &World, sub: &Sub, chat: &mut Chat, thread: &str, mut task: String, (ask, input): (bool, &mut dyn BufRead), out: &mut dyn Write, err: &mut dyn Write) -> Followed {
    let front = &world.front;
    let mut status = 0;
    let mut planning = false;
    let mut line = String::new();
    // the one line that says a limit ended the turn (no run waits past its clock)
    let mut limited: Option<String> = None;
    let mut cooling_past = 0;
    loop {
        let row = front.tasks().book().wait(&task, |r| r.state.is_terminal() || matches!(r.state, State::Waiting { .. }), Duration::from_millis(200));

        // a task that waits out a limit goes on as a login that is open, as the screen's event path does it
        if row.is_none() {
            front.limits_sweep();
            // a limit that ends after the clock, still there after the sweep had its say (it may have carried the task on as another login), ends the turn
            if limited.is_none() && limit_past_the_clock(world, &task, false).is_some() {
                cooling_past += 1;
                if cooling_past >= 3 {
                    limited = limit_past_the_clock(world, &task, true);
                }
            } else {
                cooling_past = 0;
            }
        }
        progress(sub, chat, thread, err);
        let Some(row) = row else { continue };
        if row.state.is_terminal() {
            // a rule may carry it on; once the loop has answered everything written before, that has happened or never will
            let _ = front.tasks().sync();
            if let Some(next) = carried_on(front.tasks().book().rows(), &row.id) {
                task = next;
                continue;
            }
            planning |= row.spec.role.as_deref() == Some("plan");
            match limited.take() {
                Some(said) => {
                    let _ = writeln!(err, "j4n: {said}");
                    status |= 1;
                }
                None => status |= finish(&row, out, err),
            }
            // the wall is the end of the process, not of one turn: no line after it is taken, nothing carries on. A task stopped at a limit of its
            // own (a review's five minutes) ends only itself, and the run goes on to what follows it
            if matches!(row.state, State::Stopped { why: Why::Wall }) && at_the_clock(world) {
                progress(sub, chat, thread, err);
                return Followed::Wall(status);
            }
            // what a rule started beside it (a check run where the work was done) is part of the run: it is waited for
            while front.tasks().book().open().iter().any(|r| matches!(r.state, State::Queued | State::Running | State::Cooling { .. })) {
                std::thread::sleep(Duration::from_millis(100));
                progress(sub, chat, thread, err);
            }
            return Followed::Ended { status, planning };
        }
        // waiting for the operator, who is whoever types the next line (only with `--approval ask`)
        if !ask {
            // nobody is going to answer: the book returns at once for a waiting task, so do not spin on it
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        let _ = writeln!(err, "j4n: {} is waiting for an answer: type it", row.id);
        line.clear();
        if !matches!(input.read_line(&mut line), Ok(n) if n > 0) {
            let _ = front.tasks().ctl(&task, Ctl::Abort, j4n_core::change::Actor::Operator);
            status = 1;
            continue;
        }
        front.say(thread, line.trim());
    }
}

/// The chats a line started under `orchestrate`, waited for, and then the task the plan started to take what they came to (none when it started none): the
/// run does not end while a chat is at work. Nothing for any other word.
fn chats_at_work(world: &World, sub: &Sub, chat: &mut Chat, thread: &str, err: &mut dyn Write) -> Option<String> {
    let front = &world.front;
    if !front.main_works() {
        return None;
    }
    let busy = || front.tasks().book().open().iter().any(|r| matches!(r.state, State::Queued | State::Running | State::Cooling { .. }));
    while busy() && !out_of_time(world) {
        std::thread::sleep(Duration::from_millis(100));
        progress(sub, chat, thread, err);
    }
    plan_next(front, thread)
}

/// The task the plan of the thread started next, if it started one: the plan takes what its turns came to and starts what is ready,
/// as the screen does at every task's end. None once the plan has nothing running and nothing ready.
fn plan_next(front: &j4n_core::front::Front, thread: &str) -> Option<String> {
    let _ = front.tasks().sync();
    // a step goes on while it still moves the plan: a check that ends within a step (a native one takes milliseconds) is taken by the next
    for _ in 0..16 {
        let head = front.log().head();
        front.plan_step(thread);
        if let Some(r) = front.tasks().book().open().into_iter().next() {
            return Some(r.id);
        }
        if front.log().head() == head {
            break;
        }
    }
    // a judge or a decider asked off the thread answers before the run ends (or the hold is out), and what a reflex started goes on
    front.judges_wait();
    front.tasks().book().open().into_iter().next().map(|r| r.id)
}

/// A run whose goal was never worked on is no success. 1 (with the reason on stderr) when a planning turn gave no plan, or the
/// plan stopped with items not done; 0 otherwise.
fn plan_end(front: &j4n_core::front::Front, thread: &str, planned: bool, err: &mut dyn Write) -> i32 {
    let plan = front.keeper().and_then(|k| k.plan(thread)).unwrap_or_default();
    if plan.items.is_empty() {
        if planned {
            let _ = writeln!(err, "j4n: the planning turn gave no plan, so the goal was never worked on");
            return 1;
        }
        return 0;
    }
    if !plan.open() {
        return 0;
    }
    let done = plan.items.iter().filter(|i| i.done()).count();
    let left: Vec<String> = plan.items.iter().filter(|i| !i.done()).map(|i| format!("{} {}", i.id, i.state)).collect();
    let _ = writeln!(err, "j4n: the plan stopped with {done} of {} items done ({})", plan.items.len(), left.join(", "));
    1
}

/// The task a rule started to carry `id` on, if one did.
pub fn carried_on(rows: Vec<Row>, id: &str) -> Option<String> {
    rows.into_iter().find(|r| r.spec.follows.as_deref() == Some(id) && r.auto > 0).map(|r| r.id)
}

/// Print how a task ended; 0 when it finished as asked.
fn finish(row: &Row, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    // a planning turn answers with the plan, which is for J4n: what the run prints is the work the plan's items did
    if let Some(a) = row.answer.as_ref().filter(|_| row.spec.role.as_deref() != Some("plan")) {
        let _ = writeln!(out, "{}", a.trim_end());
    }
    match &row.state {
        State::Done { ok: true } => 0,
        State::Done { ok: false } => {
            let _ = writeln!(err, "j4n: {} did not finish: {}", row.id, row.reason.as_deref().unwrap_or("it ended with nothing"));
            1
        }
        State::Stopped { why } => {
            let _ = writeln!(err, "j4n: {} stopped: {}", row.id, why.word());
            1
        }
        other => {
            let _ = writeln!(err, "j4n: {} ended {}", row.id, other.word());
            1
        }
    }
}
