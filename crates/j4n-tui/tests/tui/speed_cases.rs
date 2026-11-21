//! The loop of the real terminal, run on a memory writer with the messages given by the test. A frame is
//! wrapped in synchronized output, an idle screen draws nothing and sleeps, what is ready together is one frame, a window dragged is
//! told once, a core that never stops sending still gets frames, and an event that changes nothing draws nothing.

use crate::world::*;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use j4n_proto::{Cell, Down, Field, FieldKind, Json, Node, Present, Row, Tone, Up};
use j4n_tui::term::{GROUND_BACK, Msg, drive, ground_to};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::{Terminal, TerminalOptions, Viewport};
use std::io::{self, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const BEGIN: &str = "\x1b[?2026h";
const END: &str = "\x1b[?2026l";

/// What the terminal was sent.
#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().expect("the sink").extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Sink {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().expect("the sink")).into_owned()
    }

    /// Frames that were sent whole.
    fn frames(&self) -> usize {
        self.text().matches(END).count()
    }
}

struct Screen {
    tx: Sender<Msg>,
    sink: Sink,
    ups: Receiver<Up>,
    run: JoinHandle<io::Result<()>>,
}

/// The loop running on a thread of its own, with `before` already waiting in its channel.
fn screen(before: Vec<Msg>) -> Screen {
    screen_told(before, || {})
}

fn screen_told(before: Vec<Msg>, told: impl FnOnce() + Send + 'static) -> Screen {
    let (tx, rx) = mpsc::channel();
    let (up_tx, ups) = mpsc::channel();
    let sink = Sink::default();
    before.into_iter().for_each(|m| tx.send(m).expect("the channel"));
    let writer = sink.clone();
    let run = thread::spawn(move || {
        let backend = CrosstermBackend::new(writer);
        let mut terminal = Terminal::with_options(backend, TerminalOptions { viewport: Viewport::Fixed(Rect::new(0, 0, 80, 24)) }).expect("a memory terminal");
        drive(&mut terminal, &rx, &up_tx, told)
    });
    Screen { tx, sink, ups, run }
}

impl Screen {
    fn send(&self, m: Msg) {
        self.tx.send(m).expect("the loop is there");
    }

    fn key(&self, code: KeyCode) {
        self.send(Msg::Input(Event::Key(KeyEvent::new(code, KeyModifiers::NONE))));
    }

    /// Wait until `cond` holds, or fail after a few seconds.
    fn until(&self, what: &str, cond: impl Fn(&Sink) -> bool) {
        let end = Instant::now() + Duration::from_secs(5);
        while !cond(&self.sink) {
            assert!(Instant::now() < end, "{what}: {:?}", self.sink.text().len());
            thread::sleep(Duration::from_millis(2));
        }
    }

    /// Close the core's end and take what the screen sent up.
    fn finish(self) -> Vec<Up> {
        self.send(Msg::Closed);
        self.run.join().expect("the loop did not panic").expect("the loop ended well");
        self.ups.try_iter().collect()
    }
}

fn down(d: Down) -> Msg {
    Msg::Down(Box::new(d))
}

fn shown() -> Vec<Msg> {
    vec![down(hello()), down(Down::Tree { node: tree() })]
}

#[test]
fn every_frame_is_between_the_sequences_that_make_the_terminal_show_it_whole() {
    let s = screen(shown());
    s.until("two frames", |k| k.frames() >= 2);
    s.key(KeyCode::F(2));
    s.until("a frame for the key", |k| k.frames() >= 3);
    let sink = s.sink.clone();
    s.finish();
    let text = sink.text();
    // the terminal shows the cursor again as it is dropped, after the last frame
    assert!(text.starts_with(BEGIN) && text.trim_end_matches("\x1b[?25h").trim_end_matches(GROUND_BACK).ends_with(END), "the frames are all there is: {:?}", &text[..text.len().min(40)]);
    assert_eq!(text.matches(BEGIN).count(), text.matches(END).count());
    let mut open = false;
    let mut marks: Vec<(usize, bool)> = text.match_indices(BEGIN).map(|(i, _)| (i, true)).chain(text.match_indices(END).map(|(i, _)| (i, false))).collect();
    marks.sort();
    for (_, is_begin) in marks {
        assert_ne!(open, is_begin, "a frame begins when none is open and ends when one is");
        open = is_begin;
    }
    assert!(!open);
}

#[test]
fn an_idle_screen_draws_nothing_and_does_not_end() {
    let s = screen(shown());
    s.until("the tree", |k| k.frames() >= 2);
    let (frames, bytes) = (s.sink.frames(), s.sink.text().len());
    thread::sleep(Duration::from_millis(300));
    assert_eq!((s.sink.frames(), s.sink.text().len()), (frames, bytes), "nothing happened, so nothing was drawn");
    assert!(!s.run.is_finished(), "and the loop waits for a message");
    s.finish();
}

#[test]
fn events_that_change_nothing_draw_nothing() {
    let s = screen(shown());
    s.until("the tree", |k| k.frames() >= 2);
    let frames = s.sink.frames();
    s.send(Msg::Input(Event::FocusGained));
    s.send(Msg::Input(Event::FocusLost));
    let mut up = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
    up.kind = KeyEventKind::Release;
    s.send(Msg::Input(Event::Key(up)));
    thread::sleep(Duration::from_millis(150));
    assert_eq!(s.sink.frames(), frames);
    s.key(KeyCode::F(2));
    s.until("a key draws", |k| k.frames() > frames);
    s.finish();
}

#[test]
fn what_is_ready_together_is_one_frame() {
    let mut before = shown();
    before.extend((0..500).map(|i| down(Down::Value { bind: format!("v:{i}"), value: Json::from(i), rev: 1 })));
    before.push(Msg::Input(Event::Key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE))));
    let s = screen(before);
    s.until("a frame", |k| k.frames() >= 1);
    thread::sleep(Duration::from_millis(100));
    let frames = s.sink.frames();
    s.finish();
    assert!(frames <= 3, "the first frame and one for the 503 messages that were ready: {frames}");
}

#[test]
fn a_window_dragged_to_its_size_is_told_once_with_the_last_size() {
    let mut before = shown();
    before.extend((0..60u16).map(|i| Msg::Input(Event::Resize(80 + i, 24 + i / 2))));
    let s = screen(before);
    s.until("a frame", |k| k.frames() >= 1);
    thread::sleep(Duration::from_millis(100));
    let told: Vec<Up> = s.finish().into_iter().filter(|u| matches!(u, Up::Resize { .. })).collect();
    assert_eq!(told, [Up::Resize { cols: 139, rows: 53 }]);
}

#[test]
fn a_core_that_never_stops_sending_still_gets_frames() {
    let s = screen(shown());
    s.until("the tree", |k| k.frames() >= 2);
    let before = s.sink.frames();
    let tx = s.tx.clone();
    let end = Instant::now() + Duration::from_millis(400);
    let flood = thread::spawn(move || {
        let mut rev = 0;
        while Instant::now() < end {
            rev += 1;
            if tx.send(down(Down::Value { bind: "v:flood".into(), value: Json::from(rev), rev })).is_err() {
                return;
            }
        }
    });
    flood.join().expect("the flood");
    let during = s.sink.frames() - before;
    s.finish();
    assert!(during >= 3, "the loop took what was ready and drew, over and over, while the core kept sending: {during} frames");
}

#[test]
fn a_key_is_drawn_at_once_and_the_core_closing_ends_the_loop() {
    let s = screen(shown());
    s.until("the tree", |k| k.frames() >= 2);
    let frames = s.sink.frames();
    let t = Instant::now();
    s.key(KeyCode::F(2));
    s.until("the key", |k| k.frames() > frames);
    assert!(t.elapsed() < Duration::from_millis(1000), "a key waits for nothing: {:?}", t.elapsed());
    s.send(Msg::Input(Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))));
    assert!(s.run.join().expect("the loop did not panic").is_ok(), "⌃c leaves");
}

/// How many times the thread `tid` of this process has gone to sleep of its own accord.
fn sleeps(tid: &str) -> u64 {
    let status = std::fs::read_to_string(format!("/proc/self/task/{tid}/status")).expect("the thread's status");
    status.lines().find_map(|l| l.strip_prefix("voluntary_ctxt_switches:")).and_then(|n| n.trim().parse().ok()).expect("a count of switches")
}

#[test]
fn an_idle_screen_wakes_for_nothing() {
    let tid = Arc::new(Mutex::new(String::new()));
    let told = tid.clone();
    let s = screen_told(shown(), move || {
        *told.lock().expect("the id") = std::fs::read_link("/proc/thread-self").expect("this thread").file_name().expect("a thread id").to_string_lossy().into_owned();
    });
    s.until("the tree", |k| k.frames() >= 2);
    thread::sleep(Duration::from_millis(100));
    let tid = tid.lock().expect("the id").clone();
    let before = sleeps(&tid);
    thread::sleep(Duration::from_millis(600));
    let woke = sleeps(&tid) - before;
    s.finish();
    assert!(woke <= 2, "a loop that looks every 30 ms would wake 20 times in 600 ms: {woke}");
}

#[test]
fn the_draft_is_sent_after_its_pause_with_no_message_to_wake_the_loop() {
    let mut before = shown();
    before.push(down(Down::Value { bind: "shape:on".into(), value: Json::Bool(true), rev: 1 }));
    before.extend(j4n_tui::headless::typed("fix the parser thing", false).into_iter().map(|k| Msg::Input(Event::Key(k))));
    let s = screen(before);
    let end = Instant::now() + Duration::from_secs(5);
    let mut sent = Vec::new();
    while Instant::now() < end && !sent.iter().any(|u| matches!(u, Up::Draft { .. })) {
        sent.extend(s.ups.try_iter());
        thread::sleep(Duration::from_millis(10));
    }
    s.finish();
    assert!(sent.contains(&Up::Draft { text: "fix the parser thing".into(), target: String::new() }), "{sent:?}");
}

#[test]
fn a_doer_that_works_in_sight_has_the_screen_drawn_for_each_tick_of_its_breath_and_nothing_else_does() {
    let cell = |icon: &str, text: &str| Cell::with_icon(icon, text, Tone::Plain);
    let rows = vec![
        Row { key: "t".into(), cells: vec![cell("", "go"), cell("", ""), cell("", "1")], ..Row::default() },
        Row { key: "l".into(), cells: vec![cell("working", "B claude"), cell("", "")], parent: Some("t".into()), ..Row::default() },
    ];
    let chat = Node { id: "chat".into(), field: Some(Field { kind: FieldKind::Chat, bind: "chat:main".into(), props: Json::Null }), ..Node::default() };
    let root = Node { id: "root".into(), present: Present::Stack, children: vec![chat], ..Node::default() };
    let s = screen(vec![down(hello()), down(Down::Tree { node: root }), down(Down::Rows { bind: "chat:main".into(), rows, total: 2, from: 0, rev: 1 })]);
    s.until("the first frames", |k| k.frames() >= 2);
    thread::sleep(Duration::from_millis(100));
    let frames = s.sink.frames();
    thread::sleep(Duration::from_millis(1100));
    let drawn = s.sink.frames() - frames;
    s.finish();
    assert!((3..=6).contains(&drawn), "a tick is a quarter of a second: {drawn} frames in 1.1 s");
}

#[test]
fn a_paste_is_drawn() {
    let s = screen(shown());
    s.until("the tree", |k| k.frames() >= 2);
    let frames = s.sink.frames();
    s.send(Msg::Input(Event::Paste("pasted words".into())));
    s.until("the paste", |k| k.frames() > frames);
    s.finish();
}

// ---- the terminal's background ----

/// The look of the design, with the fill said.
fn look(fill: &str) -> Msg {
    down(Down::Theme { look: j4n_proto::Look { palette: "jjj".into(), marks: "icons".into(), frame: "pipes".into(), corners: "round".into(), title_place: "inside".into(), density: "normal".into(), fill: fill.into(), you: "band".into(), motion: true, mouse: "on".into(), read: Default::default(), time: "ago".into(), chart: "dither".into() } })
}

const JJJ_BG: (u8, u8, u8) = (0x16, 0x1a, 0x20);

#[test]
fn the_terminals_background_is_set_with_the_first_page_and_given_back_when_the_loop_ends() {
    let s = screen(shown());
    s.until("the page", |k| k.frames() >= 2);
    let sink = s.sink.clone();
    assert_eq!(sink.text().matches(&ground_to(JJJ_BG)).count(), 1, "OSC 11, once: {:?}", sink.text());
    assert_eq!(ground_to(JJJ_BG), "\x1b]11;rgb:16/1a/20\x07");
    assert_eq!(sink.text().matches(GROUND_BACK).count(), 0, "and not given back while the screen is up");
    s.finish();
    let text = sink.text();
    let (set, back) = (text.find(&ground_to(JJJ_BG)).expect("set"), text.find(GROUND_BACK).expect("given back"));
    assert_eq!(GROUND_BACK, "\x1b]111\x07");
    assert!(text.find(BEGIN).expect("a frame") < set && set < back, "the colour is set inside a frame, and given back after it");
    assert!(back > text.rfind(END).expect("the last frame"), "after the last frame of all");
    assert_eq!(text.matches(GROUND_BACK).count(), 1);
}

#[test]
fn the_terminals_background_is_not_set_before_the_tree_has_come() {
    let s = screen(vec![down(hello())]);
    s.until("a frame", |k| k.frames() >= 1);
    let sink = s.sink.clone();
    s.finish();
    assert!(!sink.text().contains("\x1b]11;") && !sink.text().contains(GROUND_BACK), "{:?}", sink.text());
}

#[test]
fn fill_paint_and_fill_terminal_do_not_touch_the_terminals_background() {
    for fill in ["paint", "terminal"] {
        let mut before = shown();
        before.insert(1, look(fill));
        let s = screen(before);
        s.until("the page", |k| k.frames() >= 2);
        let sink = s.sink.clone();
        s.finish();
        assert!(!sink.text().contains("\x1b]11;") && !sink.text().contains(GROUND_BACK), "{fill}: {:?}", sink.text());
    }
}

#[test]
fn a_look_that_changes_the_fill_while_the_screen_is_up_sets_the_background_or_gives_it_back_and_does_not_again_at_the_end() {
    let s = screen(shown());
    s.until("the page", |k| k.frames() >= 2);
    s.send(look("terminal"));
    s.until("given back", |k| k.text().contains(GROUND_BACK));
    s.send(look("match"));
    s.until("set again", |k| k.text().matches(&ground_to(JJJ_BG)).count() == 2);
    let sink = s.sink.clone();
    s.finish();
    let text = sink.text();
    assert_eq!(text.matches(GROUND_BACK).count(), 2, "once for the change, once at the end: {text:?}");
    ground_order(&text);
}

fn ground_order(text: &str) {
    let at: Vec<usize> = text.match_indices(&ground_to(JJJ_BG)).map(|(i, _)| i).chain(text.match_indices(GROUND_BACK).map(|(i, _)| i)).collect();
    let mut sorted = at.clone();
    sorted.sort_unstable();
    let kinds: Vec<bool> = sorted.iter().map(|i| text[*i..].starts_with("\x1b]11;")).collect();
    assert_eq!(kinds, [true, false, true, false], "set, back, set, back");
}
