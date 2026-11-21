//! `select`: where the keys are in a list of places, shaped like ratatui's `ListState` (`selected`, `offset`) but in `u64`: the core holds
//! the rows of a binding (a Log may have a million) and the screen a window of them.

use crossterm::event::KeyCode;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sel {
    /// The place the keys are on.
    pub at: u64,
    /// The first place shown (`scroll::keep` moves it).
    pub top: u64,
}

impl Sel {
    /// Go to `to`, or the last of `total` places when it is past them.
    pub fn select(&mut self, to: u64, total: u64) {
        self.at = to.min(total.saturating_sub(1));
    }

    /// `d` places up (negative) or down.
    pub fn by(&mut self, d: i64, total: u64) {
        self.select(self.at.saturating_add_signed(d), total);
    }

    /// `↑↓ PgUp PgDn Home End g G`, a page being `page` places. True when the key was one of them.
    pub fn key(&mut self, code: KeyCode, page: u64, total: u64) -> bool {
        let page = i64::try_from(page).unwrap_or(i64::MAX);
        match code {
            KeyCode::Up => self.by(-1, total),
            KeyCode::Down => self.by(1, total),
            KeyCode::PageUp => self.by(-page, total),
            KeyCode::PageDown => self.by(page, total),
            KeyCode::Home | KeyCode::Char('g') => self.select(0, total),
            KeyCode::End | KeyCode::Char('G') => self.select(total, total),
            _ => return false,
        }
        true
    }
}
