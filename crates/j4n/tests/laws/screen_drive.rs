//! A script drives the screen — type a line, see the streamed answer, allow an approval — against a
//! Claude Code that plays a recording, and the same child goes on with the next line; the chat is read back from the Log
//! when J4n is started again. The screen is the real TUI drawn into memory, the front, the loop, the Gate and the Log are the
//! real ones, wired as the binary wires them.

use crate::run_cases::{Dirs, fake_claude};
use crate::support::read;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::task::Mode;
use j4n_proto::{Down, Up};
use j4n_tui::App;
use j4n_tui::headless::{Headless, alt, ctrl, typed};
use std::fs;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

pub fn setup(d: &Dirs, fixture: &str) -> (Setup, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let (real, recording, log) = fake_claude(d, fixture);
    // the child is told which recording to play and where to log by a wrapper, since the environment is the process's
    let wrapper = d.dir.join("bin/claude-here");
    fs::write(&wrapper, format!("#!/bin/sh\nFAKE_FIXTURE='{}' FAKE_LOG='{}' exec '{}' \"$@\"\n", recording.display(), log.display(), real.display())).unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    (Setup { root: d.project(), home: d.j4n_home(), claude: wrapper.display().to_string() }, log)
}

fn opts() -> Opts {
    Opts { doer: Some("claude".into()), model: Some("haiku".into()), approval: Some(Approval::Ask), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false }
}

/// The operator's side: the app, its screen, and the channels to the core.
pub struct Operator {
    pub app: App,
    pub screen: Headless,
    pub up: Sender<Up>,
    pub down: Receiver<Down>,
    pub last: String,
    /// How many pages the core has opened (`Down::Focus`) so far.
    pub opened: usize,
    /// Texts no frame may show, each with its name: every frame drawn is looked at as it is drawn, waits included, and a hit is kept in
    /// `shown` (a frame is not kept: a long wait draws thousands).
    pub never: Vec<(String, String)>,
    /// Words no frame may show in any case, lower case (a shell command, a path, a file): a hit is kept in `shown` with the line that
    /// holds it, once per line.
    pub words: Vec<String>,
    /// `<name> in frame <n>` for each text of `never` a frame showed, and `` `<word>` in: <line> (frame <n>)`` for each of `words`.
    pub shown: Vec<String>,
    /// How many frames were drawn.
    pub drawn: usize,
}

impl Operator {
    pub fn sit(world: &World) -> Operator {
        Operator::watching(world, Vec::new())
    }

    /// [`Operator::sit`], with every frame from the first one looked at for the texts of `never` (`(name, text)`).
    pub fn watching(world: &World, never: Vec<(String, String)>) -> Operator {
        Operator::scanning(world, never, Vec::new())
    }

    /// [`Operator::watching`], with every frame from the first one looked at for `words` too (lower case, in any case).
    pub fn scanning(world: &World, never: Vec<(String, String)>, words: Vec<String>) -> Operator {
        let (up, down) = world.front.connect();
        let mut app = App::new();
        for u in app.start() {
            up.send(u).unwrap();
        }
        // 48 rows (the reader cuts prose at 76 columns, so the list wraps onto more rows): the list of packs is longer than 30 rows once the pack `inbox` stands in it, than 32 with `ledger`, than 34 with `parts`, than 36 with `habits`, than 38 with `kit`, than 40 with the reader's 76 columns, than 46 with `crew`
        let mut o = Operator { app, screen: Headless::new(160, 48), up, down, last: String::new(), opened: 0, never, words, shown: Vec::new(), drawn: 0 };
        o.until("the first screen", |s| s.contains("Chat") && s.contains("Settings"));
        o
    }

    pub fn look(&mut self) -> String {
        self.last = self.screen.show(&mut self.app);
        self.drawn += 1;
        for (name, text) in &self.never {
            if self.last.contains(text.as_str()) {
                self.shown.push(format!("{name} in frame {}", self.drawn));
            }
        }
        if !self.words.is_empty() {
            let low = self.last.to_lowercase();
            for w in self.words.iter().filter(|w| low.contains(w.as_str())) {
                for line in low.lines().filter(|l| l.contains(w.as_str())) {
                    let at = format!("`{w}` in: {}", line.trim());
                    if !self.shown.iter().any(|s| s.starts_with(&at)) {
                        self.shown.push(format!("{at} (frame {})", self.drawn));
                    }
                }
            }
        }
        self.last.clone()
    }

    /// Hand one thing the core sent to the app, and what the app answers to the core. Counts the pages the core opened.
    fn take(&mut self, d: Down) {
        if matches!(d, Down::Focus { .. }) {
            self.opened += 1;
        }
        for u in self.app.down(d) {
            self.up.send(u).unwrap();
        }
    }

    /// Take what the core sends, until the screen satisfies `ready`.
    pub fn until(&mut self, what: &str, ready: impl Fn(&str) -> bool) -> String {
        self.wait(what, 0, ready)
    }

    /// Say a line that opens a page (`:shelf`) and take what the core sends until that page is open and satisfies `ready`. The page
    /// is opened by a message of its own, which the core sends a moment after the line, so a screen that already satisfies `ready`
    /// (the chat shows the tasks a view of the tasks lists) is not yet the page; keys pressed on it would reach the wrong page, and the
    /// message would then take the page from under them.
    pub fn go(&mut self, line: &str, what: &str, ready: impl Fn(&str) -> bool) -> String {
        let seen = self.opened;
        self.type_line(line);
        self.wait(what, seen + 1, ready)
    }

    /// Take what the core sends until the screen satisfies `ready` and at least `opened` pages have been opened by the core.
    fn wait(&mut self, what: &str, opened: usize, ready: impl Fn(&str) -> bool) -> String {
        let end = Instant::now() + crate::support::PATIENCE;
        loop {
            while let Ok(d) = self.down.try_recv() {
                self.take(d);
            }
            let s = self.look();
            if self.opened >= opened && ready(&s) {
                return s;
            }
            assert!(Instant::now() < end, "never saw {what}{}; the screen is\n{s}", if self.opened < opened { " (the page was never opened by the core)" } else { "" });
            if let Ok(d) = self.down.recv_timeout(Duration::from_millis(20)) {
                self.take(d);
            }
        }
    }

    /// Take what the core sends, until `ready` holds of the app itself (what the screen does not show: a switch it was told of).
    pub fn until_app(&mut self, what: &str, ready: impl Fn(&App) -> bool) {
        let end = Instant::now() + crate::support::PATIENCE;
        while !ready(&self.app) {
            assert!(Instant::now() < end, "never saw {what}; the screen is\n{}", self.look());
            if let Ok(d) = self.down.recv_timeout(Duration::from_millis(20)) {
                self.take(d);
            }
        }
    }

    pub fn press(&mut self, keys: Vec<j4n_tui::headless::KeyEvent>) {
        for k in keys {
            for u in self.app.key(k) {
                self.up.send(u).unwrap();
            }
        }
    }

    pub fn type_line(&mut self, text: &str) {
        let keys = typed(text, true);
        self.press(keys);
    }
}

/// Every task is over, and none is about to be queued by the end of another: a line typed while one goes on is told to wait. The loop is
/// let handle what was written before the look (`sync`), the rules' answers to it among them, so a flow that has one step left has
/// queued it by then; a pause of so many milliseconds is a guess about how soon that is, which a loaded machine takes longer than.
pub fn wait_idle(world: &World) {
    let end = Instant::now() + crate::support::PATIENCE;
    loop {
        let over = || world.front.tasks().book().rows().iter().all(|r| r.state.is_terminal());
        world.front.tasks().sync().expect("the loop answers");
        let head = world.front.log().head();
        if over() {
            world.front.tasks().sync().expect("the loop answers");
            if world.front.log().head() == head && over() {
                return;
            }
        }
        assert!(Instant::now() < end, "a task never ended");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Whether a question is on the screen: its box (a short screen gives up the keys line, never the box).
pub fn asked(screen: &str) -> bool {
    // a question is a round box over the composer's own, so two boxes are on the screen
    let boxes = |open: char, close: char| screen.lines().filter(|l| l.starts_with(open) && l.trim_end().ends_with(close)).count();
    boxes('╭', '╮') >= 2 && boxes('╰', '╯') >= 2
}

/// The screen as words: the borders of the box are gone and every run of blanks, wrapped lines included, is one.
pub fn flat(screen: &str) -> String {
    screen.replace('│', " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether the question on the screen is a choice: its last line is the one for words of one's own.
pub fn choosing(screen: &str) -> bool {
    asked(screen) && screen.contains("type your own")
}

/// Whether a line of the screen is the operator's own: a turn of the transcript, which begins with the mark `you` in any tier.
pub fn is_you(line: &str) -> bool {
    // the old mark, the bracket head (the left cap), or the band: two blanks, the words, and the time at the right edge
    let t = line.trim_end().as_bytes();
    let time = t.len() > 6 && t[t.len() - 3] == b':' && t[t.len() - 5..].iter().all(|b| b.is_ascii_digit() || *b == b':') && t[t.len() - 6] == b' ';
    line.starts_with(['\u{F0013}', '❯', '>', '▌']) || line.starts_with("  ") && !line.starts_with("   ") && time
}

/// Whether the chat page is on the screen: the composer is, a rounded frame of its own.
pub fn on_chat(screen: &str) -> bool {
    screen.lines().any(|l| l.starts_with('╭'))
}

/// The chat part of a screen: the lines between the tabs and the composer.
fn chat_of(screen: &str) -> Vec<String> {
    let lines: Vec<&str> = screen.lines().collect();
    let composer = lines.iter().rposition(|l| l.starts_with('╭') || l.starts_with('?')).unwrap_or(lines.len());
    lines[1..composer].iter().map(|l| l.trim_end().to_string()).filter(|l| !l.is_empty()).collect()
}

pub fn open(s: &Setup) -> World {
    // a log another J4n has open is refused, and the one that was just let go may take a moment to be: wait for it
    crate::support::opened(|| wire::open(s.clone(), &opts(), true, "main"))
}

#[test]
fn type_a_line_see_the_streamed_answer_allow_an_approval_and_the_same_child_goes_on() {
    let d = Dirs::new("m1");
    let (setup, log) = setup(&d, "m1-session.jsonl");
    let world = open(&setup);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    let mut op = Operator::sit(&world);

    // a line, and the question it leads to
    op.type_line("build the thing");
    // the question is on the screen a moment before the task is marked as waiting for it: both are waited for
    let s = op.until("the approval", |s| s.contains("allow Bash: make build") && s.lines().next_back().is_some_and(|l| l.contains("waits for you")));
    assert!(s.lines().any(is_you) && s.contains("build the thing"), "{s}");
    assert!(!s.lines().any(|l| l.contains("A work") && l.contains("─")) && s.contains("claude · haiku 5.5"), "a lone lane has no row, and the composer strip says who works: {s}");
    assert!(s.lines().next_back().unwrap().contains("waits for you") && !s.lines().next_back().unwrap().contains("haiku"), "the bottom row says it waits (the state, not the keys), and not who works: {s}");

    // the operator allows it with a key; the command's result and the answer arrive
    op.press(vec![alt('y')]);
    let s = op.until("the answer", |s| s.contains("Built.") && !asked(s));
    assert!(s.contains("make build"), "{s}");
    assert!(!asked(&s), "the question is gone: {s}");
    op.until("the keys back in the bottom row", |s| s.lines().next_back().is_some_and(|l| l.contains("⌥1–9 tabs")));

    // the next line is answered by the same child, and its words stream
    op.type_line("was it clean?");
    let s = op.until("the second answer", |s| s.contains("The build was clean."));
    assert!(s.lines().any(|l| is_you(l) && l.contains("was it clean?")), "{s}");
    let sent = read(&log);
    assert_eq!(sent.lines().filter(|l| l.starts_with("start ")).count(), 1, "one child for both lines:\n{sent}");
    assert_eq!(sent.lines().filter(|l| l.contains("\"type\":\"user\"")).count(), 2);
    assert_eq!(sent.lines().filter(|l| l.contains("control_response") && l.contains("\"allow\"")).count(), 1, "the yes went to the child");

    // the tasks tab lists both turns, and the log tab the events
    op.press(vec![j4n_tui::headless::press(j4n_tui::headless::KeyCode::F(2))]);
    let s = op.until("the tasks", |s| s.contains("t1") && s.contains("t3"));
    assert!(s.contains("build the thing") && s.contains("was it clean?"), "{s}");
    op.press(vec![j4n_tui::headless::press(j4n_tui::headless::KeyCode::F(6))]);
    op.until("the log", |s| s.contains("task.settled"));

    op.press(vec![ctrl('c')]);
    assert!(op.app.quit);
}

#[test]
fn the_chat_is_read_back_from_the_log_when_j4n_is_started_again_and_the_session_is_resumed() {
    let d = Dirs::new("m1b");
    let (setup, log) = setup(&d, "m1-session.jsonl");
    let before = {
        let world = open(&setup);
        let mut op = Operator::sit(&world);
        op.type_line("build the thing");
        op.until("the approval", |s| s.contains("allow Bash") && asked(s));
        op.press(vec![alt('y')]);
        op.until("the answer", |s| s.contains("Built."));
        // the ledger is on: the card of the settled thread is part of the chat, and comes after the answer
        op.until("the report card", |s| s.contains("◆ ledger"));
        op.until("the keys back in the bottom row", |s| s.lines().next_back().is_some_and(|l| l.contains("⌥1–9 tabs")));
        chat_of(&op.last)
    };
    assert!(before.iter().any(|l| l.contains("Built.")) && before.len() >= 4, "{before:?}");

    // J4n is started again over the same folders
    let world = open(&setup);
    let mut op = Operator::sit(&world);
    // the strip and the keys of the bottom row are messages of their own, which may come after the rows: the test waits for all three
    let s = op.until("the old chat", |s| s.contains("Built.") && s.contains("claude · haiku 5.5") && s.lines().next_back().is_some_and(|l| l.contains("⌥1–9 tabs")));
    assert_eq!(chat_of(&s), before, "what was shown is shown again");
    let bar = s.lines().next_back().unwrap();
    assert!(s.contains("claude · haiku 5.5") && bar.contains("⌥1–9 tabs") && !bar.contains("haiku"), "the thread is still bound (the strip says it): {s}");

    // and the next line carries on in the transcript of the session that was there
    op.type_line("and again");
    op.until("a new child", |_| read(&log).lines().filter(|l| l.starts_with("start ")).count() == 2);
    let starts: Vec<String> = read(&log).lines().filter(|l| l.starts_with("start ")).map(String::from).collect();
    assert!(!starts[0].contains("--resume"), "{starts:?}");
    assert!(starts[1].contains("--resume 00000000-0000-4000-8000-0000000000b7"), "{starts:?}");
    op.until("the new line on the screen", |s| s.lines().any(|l| is_you(l) && l.contains("and again")));
}
