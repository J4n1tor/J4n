//! A part of the page that waits: a binding the screen watches whose rows or value have not come, or a page whose parts it asked for. A page
//! is always in one of three states and says which: waiting (the loading sign, `parts::track::waiting`), empty (`nothing here`) or filled. The
//! sign comes only after [`DELAY`], so a page that fills at once never flickers, and after [`BUSY`] its words say the core is busy. The
//! moment each wait was asked is the screen's clock as it was told (`set_clock`, the loop's tick); with no clock told there is no sign. The
//! screen wakes at the moment a sign is due, and while one is drawn, and for nothing else.

use crate::app::App;
use j4n_proto::{Intent, Up};
use ratatui::text::Line;
use std::time::{Duration, Instant};

/// How long a part may wait before the sign shows.
pub const DELAY: Duration = Duration::from_millis(180);

/// How long a part waits before its words say the core is busy.
pub const BUSY: Duration = Duration::from_secs(3);

/// How far a wait has gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    /// Asked a moment ago: nothing is drawn yet.
    Soon,
    Loading,
    Busy,
}

impl App {
    /// How the rows of `bind` wait: `None` once they came, or while the binding is not watched (nothing asked for them).
    pub(crate) fn rows_wait(&self, bind: &str) -> Option<Wait> {
        (self.watched.contains(bind) && !self.rows.contains_key(bind)).then(|| self.wait_of(self.since.get(bind)))
    }

    /// How the value of `bind` waits: `None` once it came (a chart with no data comes as a null), or while it is not watched.
    pub(crate) fn value_wait(&self, bind: &str) -> Option<Wait> {
        (self.watched.contains(bind) && !self.values.contains_key(bind)).then(|| self.wait_of(self.since.get(bind)))
    }

    /// How the parts of the page `node` wait, while the core has been asked for them.
    pub(crate) fn node_wait(&self, node: &str) -> Option<Wait> {
        self.asked.contains(node).then(|| self.wait_of(self.opened.get(node)))
    }

    /// The wait of something asked at `at`, by the clock the screen was told; the moment it next changes is noted, so the screen wakes for it.
    fn wait_of(&self, at: Option<&Instant>) -> Wait {
        let (Some(at), Some(now)) = (at, self.clock) else { return Wait::Soon };
        let gone = now.saturating_duration_since(*at);
        let (wait, next) = if gone < DELAY { (Wait::Soon, Some(DELAY)) } else if gone < BUSY { (Wait::Loading, Some(BUSY)) } else { (Wait::Busy, None) };
        if let Some(next) = next {
            let due = *at + next;
            self.wait_due.set(Some(self.wait_due.get().map_or(due, |d| d.min(due))));
        }
        wait
    }

    /// The line a part that waits draws, `width` cells wide (with its `label`, a chart's); none while it is [`Wait::Soon`]. A sign drawn wakes the
    /// screen for its next frame.
    pub(crate) fn wait_line(&mut self, wait: Wait, label: &str, width: usize) -> Option<Line<'static>> {
        if wait == Wait::Soon {
            return None;
        }
        self.signs = true;
        Some(crate::parts::track::waiting(&self.theme, self.sign, wait == Wait::Busy, label, width))
    }

    /// What was just asked of the core (`ups`: a watch, a page opened) is stamped with the clock; what is no longer watched or asked is forgotten.
    pub(crate) fn stamp(&mut self, ups: &[Up]) {
        self.since.retain(|b, _| self.watched.contains(b));
        self.opened.retain(|n, _| self.asked.contains(n));
        let Some(now) = self.clock else { return };
        for up in ups {
            match up {
                Up::Watch { bind } => drop(self.since.insert(bind.clone(), now)),
                Up::Intent { intent: Intent::Open { node } } => drop(self.opened.insert(node.clone(), now)),
                _ => {}
            }
        }
    }

    /// The clock moved on to `now`: a part whose wait changes by then is drawn again.
    pub(crate) fn wait_tick(&mut self, now: Instant) {
        self.redraw |= self.wait_due.get().is_some_and(|due| due <= now);
    }

    /// How long until a part's wait changes; none when the last frame drew no part that waits.
    pub(crate) fn wait_wait(&self, now: Instant) -> Option<Duration> {
        self.wait_due.get().map(|due| due.saturating_duration_since(now))
    }
}
