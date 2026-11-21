//! The real terminal: raw mode, the alternate screen, keys in, frames out. It draws only when something changed, and when nothing is
//! to be done it sleeps until a message comes or until the clock matters (a breath, a draft's pause): an idle screen costs nothing.
//! The keys are read on a thread of their own and the core's messages are carried over by another, so the
//! thread that draws does nothing else; it takes what is ready for up to [`COALESCE`], draws once, and wraps the frame in
//! synchronized output (DEC 2026) so that the terminal shows it whole.

use crate::app::App;
use crate::read::{Caps, Late, xtversion};
use crate::theme::{Settings, Theme};
use crate::view;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, MouseEventKind, DisableBracketedPaste, EnableBracketedPaste, Event, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags};
use crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use j4n_proto::{Down, Up};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long what is ready may be read before the frame is drawn: a core that sends faster than the screen draws still gets a frame.
pub const COALESCE: Duration = Duration::from_millis(20);

/// How often the thread that reads keys looks whether the screen has been left. Nothing else wakes while the screen is idle.
const LOOK: Duration = Duration::from_millis(500);

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

unsafe extern "C" {
    fn poll(fds: *mut PollFd, count: usize, millis: i32) -> i32;
    fn write(fd: i32, buf: *const u8, count: usize) -> isize;
    fn read(fd: i32, buf: *mut u8, count: usize) -> isize;
    fn signal(sig: i32, handler: usize) -> usize;
    fn raise(sig: i32) -> i32;
}

/// What the terminal can draw, asked once before the keys are read (the terminal's reply would be taken for keys).
static CAPS: std::sync::OnceLock<Caps> = std::sync::OnceLock::new();

/// Whether the question was put to the terminal (a late reply is then dropped for a while).
static ASKED: AtomicBool = AtomicBool::new(false);

/// Whether ⌥digit may come as a bare Esc and the digit in two reads: the terminal does not speak the kitty protocol.
static SPLIT: AtomicBool = AtomicBool::new(false);

/// What gives the terminal its own background back (OSC 111).
pub const GROUND_BACK: &str = "\x1b]111\x07";

/// What sets the terminal's background to a colour (OSC 11).
pub fn ground_to((r, g, b): (u8, u8, u8)) -> String {
    format!("\x1b]11;rgb:{r:02x}/{g:02x}/{b:02x}\x07")
}

/// Whether the terminal's background is set to ours now: what a panic or a signal must undo.
static GROUND_SET: AtomicBool = AtomicBool::new(false);

/// Whether the terminal reports the mouse to us now (`theme.mouse`).
static MOUSE_SET: AtomicBool = AtomicBool::new(false);

/// What turns the mouse reports off (the modes crossterm turns on).
const MOUSE_BACK: &str = "\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l";

/// Whether the mouse is asked for: the look says (`theme.mouse`, on unless the operator turned it off), and `J4N_MOUSE` (`1` or `0`)
/// overrides it. Reporting the mouse takes the terminal's own selection away: with ⇧ held the terminal selects, as it does for any program.
pub fn mouse_wanted(look: bool, env: Option<&std::ffi::OsStr>) -> bool {
    match env.and_then(|v| v.to_str()) {
        Some("1") => true,
        Some("0") => false,
        _ => look,
    }
}

/// Give the terminal its background (and its own mouse) back if we set them, by `write`s that a panic hook or a signal handler may make.
fn ground_back() {
    if MOUSE_SET.swap(false, Ordering::SeqCst) {
        // SAFETY: one write of a constant string to the terminal on stdout; a failure is nothing to act on.
        unsafe { write(1, MOUSE_BACK.as_ptr(), MOUSE_BACK.len()) };
    }
    if GROUND_SET.swap(false, Ordering::SeqCst) {
        // SAFETY: one write of a constant string to the terminal on stdout; a failure is nothing to act on.
        unsafe { write(1, GROUND_BACK.as_ptr(), GROUND_BACK.len()) };
    }
}

const SIGHUP: i32 = 1;
const SIGTERM: i32 = 15;
const SIG_DFL: usize = 0;

extern "C" fn stopped(sig: i32) {
    ground_back();
    // SAFETY: `signal` and `raise` are async-signal-safe and take only integers; the default action is put back and re-sent.
    unsafe {
        signal(sig, SIG_DFL);
        raise(sig);
    }
}

const HUNG_UP: i16 = 0x10 | 0x08 | 0x20; // POLLHUP, POLLERR, POLLNVAL

/// Whether the terminal on stdin is gone: its master closed (a window closed, a `script` killed). crossterm's reader does not end on that,
/// it reads the hung-up tty again and again without a pause, so the keys are not where it is seen.
fn hung_up() -> bool {
    let mut fd = PollFd { fd: 0, events: 0, revents: 0 };
    // SAFETY: one `pollfd` that lives through the call, and a count of one.
    unsafe { poll(&mut fd, 1, 0) > 0 && fd.revents & HUNG_UP != 0 }
}

/// What the terminal can draw: the answer to `XTVERSION` (`CSI > 0 q`; a `CSI c` after it ends the wait for a terminal that does not
/// answer it), else `TERM_PROGRAM` and `TERM`. No question is put where `Caps::asks` says no (a test, `J4N_PROBE=0`, ssh, no terminal); a
/// reply that comes after the wait is dropped by `Late`.
fn probe() -> Caps {
    use std::io::IsTerminal;
    let env = |k: &str| std::env::var(k).ok();
    let asks = Caps::asks(&env, io::stdin().is_terminal(), io::stdout().is_terminal());
    ASKED.store(asks, Ordering::SeqCst);
    Caps::detect(if asks { ask_version() } else { None }.as_deref(), &env)
}

fn ask_version() -> Option<String> {
    let question = b"\x1b[>0q\x1b[c";
    // SAFETY: one write of a constant string to the terminal on stdout.
    unsafe { write(1, question.as_ptr(), question.len()) };
    let (mut got, end) = (Vec::new(), Instant::now() + Duration::from_millis(200));
    loop {
        let left = end.saturating_duration_since(Instant::now());
        let mut fd = PollFd { fd: 0, events: 1, revents: 0 };
        // SAFETY: one `pollfd` that lives through the call, and a buffer of the length told to `read`.
        let n = unsafe {
            if left.is_zero() || poll(&mut fd, 1, left.as_millis() as i32) <= 0 {
                break;
            }
            let mut buf = [0u8; 128];
            let n = read(0, buf.as_mut_ptr(), buf.len());
            got.extend_from_slice(&buf[..n.max(0) as usize]);
            n
        };
        // the answer to the second question comes last
        if n <= 0 || got.windows(3).any(|w| w == [0x1b, b'[', b'?']) && got.last() == Some(&b'c') {
            break;
        }
    }
    xtversion(&got)
}

/// What the screen waits for.
pub enum Msg {
    Down(Box<Down>),
    Input(Event),
    /// The core closed its end.
    Closed,
}

/// What J4n asks of a terminal that speaks the kitty keyboard protocol. Always: ⇧⏎ told from ⏎ (CSI-u), and a key pressed told from one
/// repeated or released, so a held key is not many presses. With `hold` (`keys.hold`, on unless `off`): every key as a code, so ⌥ pressed
/// alone is seen and the tabs' numbers show while it is held, and the character a key makes with ⇧ beside its code, or `⇧;` would come as
/// `;` and not `:`.
pub fn key_flags(hold: bool) -> KeyboardEnhancementFlags {
    let always = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES | KeyboardEnhancementFlags::REPORT_EVENT_TYPES;
    if hold { always | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES } else { always }
}

/// Show the screen until the operator leaves or the core closes the channel; `hold` asks the terminal to report ⌥ held ([`key_flags`]).
/// The terminal is given back on every way out.
pub fn run(down: Receiver<Down>, up: &Sender<Up>, hold: bool, first_frame: impl FnOnce()) -> io::Result<()> {
    let mut terminal = ratatui::init();
    // the terminal's background is given back when the screen panics or is killed, as well as when it ends
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        ground_back();
        before(info);
    }));
    // SAFETY: `stopped` makes only async-signal-safe calls (`write`, `signal`, `raise`).
    unsafe {
        signal(SIGTERM, stopped as *const () as usize);
        signal(SIGHUP, stopped as *const () as usize);
    }
    // a paste arrives as one piece of text, not as keys (a line break in it is not Enter)
    let _ = crossterm::execute!(io::stdout(), EnableBracketedPaste);
    // a terminal that does not speak the kitty protocol keeps ⌃j for a new line, a held key is told by the clock, and ⌥ held shows nothing
    let csi_u = crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false) && crossterm::execute!(io::stdout(), PushKeyboardEnhancementFlags(key_flags(hold))).is_ok();
    SPLIT.store(!csi_u, Ordering::SeqCst);
    CAPS.get_or_init(probe);
    let (tx, rx) = mpsc::channel();
    let left = Arc::new(AtomicBool::new(false));
    let carry = tx.clone();
    std::thread::spawn(move || {
        while let Ok(d) = down.recv() {
            if carry.send(Msg::Down(Box::new(d))).is_err() {
                return;
            }
        }
        let _ = carry.send(Msg::Closed);
    });
    // a SIGHUP that was ignored when J4n started (a `nohup` before it) does not end it with its terminal: the screen ends itself
    let (gone, watch) = (tx.clone(), left.clone());
    std::thread::spawn(move || {
        while !watch.load(Ordering::Relaxed) {
            if hung_up() {
                let _ = gone.send(Msg::Closed);
                return;
            }
            std::thread::sleep(LOOK / 5);
        }
    });
    let stop = left.clone();
    std::thread::spawn(move || {
        // a key is read only while the screen is up, so none is taken from whatever runs after it
        // a poll that finds nothing in LOOK is only a look at `stop`; it is not the end of the keys
        while !stop.load(Ordering::Relaxed) {
            match event::poll(LOOK) {
                Ok(false) => continue,
                Ok(true) => {}
                Err(_) => return,
            }
            let sent = event::read().is_ok_and(|e| !stop.load(Ordering::Relaxed) && tx.send(Msg::Input(e)).is_ok());
            if !sent {
                return;
            }
        }
    });
    let done = drive(&mut terminal, &rx, up, first_frame);
    left.store(true, Ordering::Relaxed);
    if csi_u {
        let _ = crossterm::execute!(io::stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = crossterm::execute!(io::stdout(), DisableBracketedPaste);
    if MOUSE_SET.swap(false, Ordering::SeqCst) {
        let _ = crossterm::execute!(io::stdout(), DisableMouseCapture);
    }
    // `restore` prints a failure with `eprintln!`, and on a terminal that is gone that print panics (stderr is the dead tty), the hook of `init`
    // restores again, panics again, and the process aborts with a core dump: the failure is not worth a word, and the hook is no use now
    let _ = ratatui::try_restore();
    drop(std::panic::take_hook());
    done
}

/// `cell` with its symbol put by kitty's text sizing (OSC 66, `s=2` twice as large, `w=0` as wide as the words need): drawn where the cell is,
/// it is the words, twice as large, in the cell's own style. A control character is left out of the words, so that they cannot end the
/// sequence.
pub fn sized(cell: &ratatui::buffer::Cell, text: &str) -> ratatui::buffer::Cell {
    let words: String = text.chars().filter(|c| !c.is_control()).collect();
    let mut c = cell.clone();
    c.set_symbol(&format!("\x1b]66;s=2:w=0;{words}\x1b\\"));
    c
}

/// One frame, between the sequences that tell the terminal to show it only when it is whole.
fn frame<W: Write>(terminal: &mut Terminal<CrosstermBackend<W>>, app: &mut App, ground: &mut Option<(u8, u8, u8)>) -> io::Result<()> {
    // with `theme.fill = match` the terminal's own background is set to the palette's once the page is up, and back when the theme says
    // otherwise; it goes in the same update as the cells, so that the two change together
    let want = if app.tree.is_some() { app.theme.ground() } else { None };
    crossterm::queue!(terminal.backend_mut(), BeginSynchronizedUpdate)?;
    if want != *ground {
        let osc = want.map_or_else(|| GROUND_BACK.to_string(), ground_to);
        crossterm::queue!(terminal.backend_mut(), crossterm::style::Print(osc))?;
        *ground = want;
        GROUND_SET.store(want.is_some(), Ordering::SeqCst);
    }
    // the mouse is asked for when the look says so, and given back when it says otherwise
    let mouse = mouse_wanted(app.theme.mouse, std::env::var_os("J4N_MOUSE").as_deref());
    if mouse != MOUSE_SET.load(Ordering::SeqCst) {
        if mouse {
            crossterm::queue!(terminal.backend_mut(), EnableMouseCapture)?;
        } else {
            crossterm::queue!(terminal.backend_mut(), DisableMouseCapture)?;
        }
        MOUSE_SET.store(mouse, Ordering::SeqCst);
    }
    // the cells under each link, taken from the frame that was drawn, to be drawn again between the sequences that open and close it (OSC 8)
    let (mut links, mut bigs) = (Vec::new(), Vec::new());
    let drawn = terminal
        .draw(|f| view::draw(app, f))
        .map(|done| {
            for h in &app.opens {
                let cells: Vec<(u16, u16, ratatui::buffer::Cell)> = (h.x..h.x + h.w).filter_map(|x| done.buffer.cell((x, h.y)).map(|c| (x, h.y, c.clone()))).collect();
                links.push((h.url.clone(), cells));
            }
            bigs = app.bigs.iter().filter_map(|b| done.buffer.cell((b.x, b.y)).map(|c| (b.x, b.y, sized(c, &b.text)))).collect::<Vec<_>>();
        });
    // a heading in kitty's text sizing (OSC 66): the terminal draws the words twice as large from their first cell, over the two rows the layout
    // left for them; the cell carries the style the words had
    if !bigs.is_empty() {
        ratatui::backend::Backend::draw(terminal.backend_mut(), bigs.iter().map(|(x, y, c)| (*x, *y, c)))?;
    }
    for (url, cells) in &links {
        crossterm::queue!(terminal.backend_mut(), crossterm::style::Print(format!("\x1b]8;;{url}\x1b\\")))?;
        ratatui::backend::Backend::draw(terminal.backend_mut(), cells.iter().map(|(x, y, c)| (*x, *y, c)))?;
        crossterm::queue!(terminal.backend_mut(), crossterm::style::Print("\x1b]8;;\x1b\\"))?;
    }
    // what a key asked to copy goes to the terminal's clipboard (`y` on the Log)
    for text in app.wins.values_mut().filter_map(|w| w.copy.take()) {
        crossterm::queue!(terminal.backend_mut(), crossterm::style::Print(crate::display::event::osc52(&text)))?;
    }
    crossterm::execute!(terminal.backend_mut(), EndSynchronizedUpdate)?;
    drawn
}

/// The loop: tick the clock, draw if anything changed, then sleep until a message or the time the clock next matters. The terminal's
/// background, if the loop set it, is given back however the loop ends.
pub fn drive<W: Write>(terminal: &mut Terminal<CrosstermBackend<W>>, rx: &Receiver<Msg>, up: &Sender<Up>, first_frame: impl FnOnce()) -> io::Result<()> {
    let mut ground = None;
    let done = run_loop(terminal, rx, up, first_frame, &mut ground);
    if ground.is_some() {
        let back = crossterm::queue!(terminal.backend_mut(), crossterm::style::Print(GROUND_BACK)).and_then(|()| terminal.backend_mut().flush());
        GROUND_SET.store(false, Ordering::SeqCst);
        return done.and(back);
    }
    done
}

fn run_loop<W: Write>(terminal: &mut Terminal<CrosstermBackend<W>>, rx: &Receiver<Msg>, up: &Sender<Up>, first_frame: impl FnOnce(), ground: &mut Option<(u8, u8, u8)>) -> io::Result<()> {
    let mut first_frame = Some(first_frame);
    let mut app = App::new();
    // the screen keeps its own time, for the words of how long ago
    app.set_wall(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64), Instant::now());
    app.read.caps = CAPS.get().copied().unwrap_or(Caps::PLAIN);
    app.esc_waits = SPLIT.load(Ordering::SeqCst);
    if ASKED.load(Ordering::SeqCst) {
        app.late = Late::after(Instant::now());
    }
    app.read.root = std::env::current_dir().ok().map(|d| d.to_string_lossy().into_owned());
    if let Ok(theme) = Theme::build(&Settings::with_no_color_var(std::env::var_os("NO_COLOR").as_deref()).with_colorterm(std::env::var_os("COLORTERM").as_deref()), &[]) {
        app.theme = theme;
    }
    let mut open = true;
    let send = |ups: Vec<Up>, open: &mut bool| {
        for u in ups {
            *open &= up.send(u).is_ok();
        }
    };
    send(app.start(), &mut open);
    let mut dirty = true;
    while open && !app.quit {
        let now = Instant::now();
        send(app.tick(now), &mut open);
        dirty |= app.take_redraw();
        if dirty {
            frame(terminal, &mut app, ground)?;
            dirty = false;
            if let Some(told) = first_frame.take() {
                told();
            }
        }
        // asleep until something comes; the clock is looked at again at the time it next matters
        let first = match app.wake(Instant::now()) {
            Some(wait) => rx.recv_timeout(wait),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        let mut msg = match first {
            Ok(m) => m,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        app.set_clock(Instant::now());
        // then everything that is ready, for a while: keys typed together and rows that came together are one frame
        let (until, mut resize) = (Instant::now() + COALESCE, None);
        loop {
            match msg {
                Msg::Down(d) => {
                    send(app.down(*d), &mut open);
                    dirty = true;
                }
                Msg::Closed => app.quit = true,
                // ⌥'s press and release reach the screen (the hold); another release or modifier alone is nothing and draws nothing
                Msg::Input(Event::Key(k)) => {
                    // timed as it came, not as the frame began: three arrows of a wheel notch are told by how close they are
                    app.set_clock(Instant::now());
                    send(app.key(k), &mut open);
                    dirty |= crate::leader::reads(&k);
                }
                // the wheel, where the terminal reports it (`theme.mouse`): three lines
                Msg::Input(Event::Mouse(m)) if matches!(m.kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown) => {
                    send(app.wheel(m.kind == MouseEventKind::ScrollUp), &mut open);
                    dirty = true;
                }
                Msg::Input(Event::Paste(mut text)) => {
                    app.paste(&text);
                    // a paste can be a key: what the terminal handed over is wiped once it is read
                    j4n_proto::Zeroize::zeroize(&mut text);
                    dirty = true;
                }
                // a window dragged to its size sends a resize for each step of it: only the last is told
                Msg::Input(Event::Resize(cols, rows)) => {
                    resize = Some((cols, rows));
                    dirty = true;
                }
                Msg::Input(_) => {}
            }
            if app.quit || Instant::now() >= until {
                break;
            }
            match rx.try_recv() {
                Ok(next) => msg = next,
                Err(_) => break,
            }
        }
        if let Some((cols, rows)) = resize {
            send(vec![Up::Resize { cols, rows }], &mut open);
        }
    }
    Ok(())
}
