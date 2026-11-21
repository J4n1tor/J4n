//! Scrolling the chat: a line at a time from the composer (`⇧↑`, `⇧↓`), the keys on the transcript
//! moving by line, half a screen for `PgUp`/`PgDn` drawn as a short glide (or a jump when the look has no motion), and the wheel
//! where the terminal reports it.

use super::{land, walk};
use crate::app::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use j4n_proto::FieldKind;
use std::time::{Duration, Instant};

/// How long one step of a glide stands, and how many steps a half screen is cut into: the first is drawn with the key, so a glide
/// is over 90 ms after it.
const FRAME: Duration = Duration::from_millis(30);
const STEPS: usize = 4;

/// A glide on its way: how many lines are left, which way, in how many steps, and when the next one is due.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Glide {
    up: bool,
    left: usize,
    steps: usize,
    due: Instant,
}

impl App {
    /// Half of what a page was: the screen less its tabs, bar and line.
    fn half(&self) -> usize {
        (usize::from(self.size.1).saturating_sub(6) / 2).max(1)
    }

    /// `n` lines: the view of the chat when the keys are on the composer, else the line the keys are on, which the view follows.
    fn move_lines(&mut self, up: bool, n: usize) {
        let Some(f) = self.field().filter(|f| f.kind == FieldKind::Chat).cloned() else {
            self.glide = None;
            return self.scroll_by(up, n);
        };
        let Some((at, sub)) = self.wins.get(&f.bind).and_then(|w| w.at.clone().map(|a| (a, w.sub))) else { return self.scroll_by(up, n) };
        let w = walk(self, &f);
        let place = w.heads.iter().position(|h| h.1 == at).unwrap_or(0);
        let cur = w.line(place, sub);
        let to = if up { cur.saturating_sub(n) } else { (cur + n).min(w.len.saturating_sub(1)) };
        land(self, &f, w.at(to).map(|(k, n)| (k.to_string(), n)));
    }

    /// `⇧↑`, `⇧↓` from the composer: the chat a few lines, the keys where they were.
    pub(crate) fn scroll_lines(&mut self, up: bool, n: usize) {
        self.glide = None;
        self.move_lines(up, n);
    }

    /// The wheel: three lines at once, never a glide. On the chat it scrolls the transcript; on any other page it is three presses of ↑
    /// or ↓ on the field that has the keys (the Log, a list, a table), so it does what those do.
    pub fn wheel(&mut self, up: bool) -> Vec<j4n_proto::Up> {
        if self.field().is_none_or(|f| f.kind == FieldKind::Chat) {
            self.scroll_lines(up, 3);
            return Vec::new();
        }
        let key = KeyEvent::new(if up { KeyCode::Up } else { KeyCode::Down }, KeyModifiers::NONE);
        (0..3).flat_map(|_| self.key(key)).collect()
    }

    /// `PgUp`, `PgDn`: half a screen. With motion it is cut into steps drawn 30 ms apart (the first now); a key pressed while it glides
    /// adds to it, or takes from it the other way. Without motion it jumps.
    pub(crate) fn page_by(&mut self, up: bool) {
        let n = self.half();
        if !self.theme.motion {
            self.glide = None;
            return self.move_lines(up, n);
        }
        let now = self.clock.unwrap_or_else(Instant::now);
        let (way, left) = match self.glide.take() {
            Some(g) if g.up == up => (up, g.left + n),
            Some(g) if g.left > n => (g.up, g.left - n),
            Some(g) => (up, n - g.left),
            None => (up, n),
        };
        self.glide = Some(Glide { up: way, left, steps: STEPS, due: now });
        self.glide_step(now);
    }

    fn glide_step(&mut self, now: Instant) {
        let Some(g) = self.glide.as_mut() else { return };
        let take = g.left.div_ceil(g.steps.max(1));
        (g.left, g.steps, g.due) = (g.left - take, g.steps.saturating_sub(1), now + FRAME);
        let (up, over) = (g.up, g.left == 0);
        if over {
            self.glide = None;
        }
        self.move_lines(up, take);
        self.redraw = true;
    }

    /// The clock moved on: the next step of a glide, once it is due.
    pub(crate) fn glide_tick(&mut self, now: Instant) {
        if self.glide.is_some_and(|g| now >= g.due) {
            self.glide_step(now);
        }
    }

    /// How long until the next step of a glide.
    pub(crate) fn glide_wait(&self, now: Instant) -> Option<Duration> {
        self.glide.map(|g| g.due.saturating_duration_since(now))
    }
}
