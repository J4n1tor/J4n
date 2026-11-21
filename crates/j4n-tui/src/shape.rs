//! The composer's card: what the shaper added under the line being typed. The TUI only keeps the
//! draft's timing and the keys; the rows, which of them are kept and the words are the core's (`shape:card`), and the switch is the
//! core's too (`shape:on`). With the switch off nothing here does anything.

use crate::app::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use j4n_proto::{Intent, Row, SHAPE_CARD, Tone, Up};
use std::time::{Duration, Instant};

/// How long the draft stands still before it is sent to be shaped.
pub const PAUSE: Duration = Duration::from_millis(800);
/// The most rows of the card that are drawn.
pub const ROWS: usize = 8;

/// The draft's timing and a send that waits for the shaper.
#[derive(Default)]
pub struct Composer {
    pub on: bool,
    /// What the input was at the last look, since when, and what was last sent.
    seen: String,
    since: Option<Instant>,
    sent: String,
    /// An Enter that waits for the shaper's answer about this text.
    pub pending: Option<String>,
    /// The card has the keys (`⌃o`).
    pub focus: bool,
}

/// What the card says about the text in the composer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Card {
    /// The shaper has not been asked, or this draft is not shaped.
    Quiet,
    Working,
    /// The line is not one the shaper looks at (an answer, a command, a short reply).
    Skip,
    /// A line that needs no notes.
    Pass,
    Failed(String),
    Dropped,
    Ready,
}

fn up(command: &str, args: &[&str]) -> Up {
    Up::Intent { intent: Intent::Run { command: command.into(), args: args.iter().map(|a| a.to_string()).collect() } }
}

/// A line the operator means as a command or a flow: never waited on.
fn command(text: &str) -> bool {
    text.starts_with(':') || text.starts_with('/')
}

impl App {
    fn card_rows(&self) -> &[Row] {
        self.rows.get(SHAPE_CARD).map_or(&[], Vec::as_slice)
    }

    /// What the card says about the text now in the composer: the first row names the draft it is for.
    pub fn card(&self) -> Card {
        let text = self.line.buf.trim();
        let Some(head) = self.card_rows().first().filter(|r| r.key == "~draft" && r.cells.first().is_some_and(|c| c.text == text)) else { return Card::Quiet };
        let state = head.cells.get(1).map_or("", |c| c.text.as_str());
        match state {
            "working" => Card::Working,
            "ready" => Card::Ready,
            "pass" => Card::Pass,
            "skip" => Card::Skip,
            "dropped" => Card::Dropped,
            "failed" => Card::Failed(head.cells.get(2).map(|c| c.text.clone()).unwrap_or_default()),
            _ => Card::Quiet,
        }
    }

    /// The keys of the rows that are kept: the ones the core shows as good.
    fn kept(&self) -> Vec<String> {
        self.card_rows().iter().filter(|r| r.tone == Tone::Good && !r.key.starts_with('~') && !r.key.starts_with("x:")).map(|r| r.key.clone()).collect()
    }

    /// The clock moved on: a bare Esc that waited for a digit and saw none acts, and the draft is sent once it has stood still.
    pub fn tick(&mut self, now: Instant) -> Vec<Up> {
        let mut ups = self.esc_due(now);
        ups.extend(self.draft_tick(now));
        ups
    }

    /// The draft, sent once it has stood still for [`PAUSE`] and differs from what was sent.
    fn draft_tick(&mut self, now: Instant) -> Vec<Up> {
        self.breathe(now);
        self.glide_tick(now);
        self.pin_idle(now);
        // the core also looks at a draft while a question is open: it says what the line would answer (`composer:bind`)
        if !self.composer.on && self.asks.is_empty() {
            return Vec::new();
        }
        let text = self.line.buf.trim().to_string();
        if text != self.composer.seen {
            self.composer.seen = text.clone();
            self.composer.since = Some(now);
            return Vec::new();
        }
        let waited = self.composer.since.is_some_and(|s| now.duration_since(s) >= PAUSE);
        // a line that may be a key is never a draft: it is held at ⏎ (`chat::held`)
        if waited && !text.is_empty() && !command(&text) && !crate::chat::held::key_like(&text) && text != self.composer.sent {
            self.composer.sent = text.clone();
            return vec![Up::Draft { text, target: String::new() }];
        }
        Vec::new()
    }

    /// How long until the clock next matters, from `now`: the end of the pause that sends the draft, or the next tick of a breath. `None`
    /// when neither is coming, and the screen sleeps until a message.
    pub fn wake(&self, now: Instant) -> Option<Duration> {
        let text = self.line.buf.trim();
        let pause = if (self.composer.on || !self.asks.is_empty()) && !text.is_empty() && !command(text) && !crate::chat::held::key_like(text) && text != self.composer.sent {
            if text == self.composer.seen { self.composer.since.map(|s| (s + PAUSE).saturating_duration_since(now)) } else { Some(Duration::ZERO) }
        } else {
            None
        };
        [pause, self.beat_wait(now), self.numbers_wait(now), self.glide_wait(now), self.pin_wait(now), self.esc_wait(now)].into_iter().flatten().min()
    }

    /// An Enter on `text`. `None` when the line is sent as it is; otherwise what to send now (maybe nothing: the Enter waits).
    pub(crate) fn shape_enter(&mut self, text: &str) -> Option<Vec<Up>> {
        if !self.composer.on || command(text) {
            return None;
        }
        match self.card() {
            Card::Ready => {
                let with = self.kept();
                self.composer.focus = false;
                self.set_input(String::new());
                Some(self.say_with(text.to_string(), with))
            }
            Card::Working => {
                self.composer.pending = Some(text.to_string());
                Some(Vec::new())
            }
            Card::Quiet if self.composer.sent != text => {
                // typed and sent before the pause: ask now, and wait for the answer
                self.composer.sent = text.to_string();
                self.composer.pending = Some(text.to_string());
                Some(vec![Up::Draft { text: text.to_string(), target: String::new() }])
            }
            _ => None,
        }
    }

    /// The card changed. An Enter that waited is let go: a line that needs no notes goes now; one with notes waits for one more Enter.
    pub(crate) fn shape_resolve(&mut self) -> Vec<Up> {
        let Some(text) = self.composer.pending.clone() else { return Vec::new() };
        if self.line.buf.trim() != text {
            self.composer.pending = None;
            return Vec::new();
        }
        match self.card() {
            Card::Working => Vec::new(),
            Card::Ready => {
                self.composer.pending = None;
                Vec::new()
            }
            _ => {
                self.composer.pending = None;
                self.set_input(String::new());
                self.say_with(text, Vec::new())
            }
        }
    }

    /// Esc with a card: the notes are dropped and the line stays as typed; `None` when there is no card to drop.
    pub(crate) fn shape_esc(&mut self) -> Option<Vec<Up>> {
        if !self.composer.on || !matches!(self.card(), Card::Ready | Card::Working) {
            return None;
        }
        self.composer.pending = None;
        self.composer.focus = false;
        Some(vec![up("shape", &["drop"])])
    }

    /// The keys of the card while it has them (`⌃o`): move, keep or drop a row, pick an option, leave.
    pub(crate) fn shape_key(&mut self, k: KeyEvent) -> Option<Vec<Up>> {
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('o') {
            self.composer.focus = self.composer.on && self.card() == Card::Ready && !self.composer.focus;
            return Some(Vec::new());
        }
        if !self.composer.focus {
            return None;
        }
        let rows: Vec<String> = self.card_rows().iter().filter(|r| !r.key.starts_with('~') && !r.key.starts_with("x:")).map(|r| r.key.clone()).collect();
        let (cur, total) = (&mut self.win(SHAPE_CARD).cur, rows.len() as u64);
        cur.select(cur.at, total);
        match k.code {
            KeyCode::Up => cur.by(-1, total),
            KeyCode::Down => cur.by(1, total),
            KeyCode::Char(' ') => return Some(rows.get(cur.at as usize).map(|key| vec![up("shape", &["toggle", key])]).unwrap_or_default()),
            KeyCode::Char(d @ '1'..='4') => return Some(vec![up("shape", &["pick", &d.to_string()])]),
            KeyCode::Enter | KeyCode::Esc => self.composer.focus = false,
            _ => {}
        }
        Some(Vec::new())
    }
}
