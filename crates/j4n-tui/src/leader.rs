//! Moving between tabs: `⌥1`–`⌥9` go to tab n from anywhere and the number of the open tab again steps its sub-tabs (on Chat, the next
//! chat); `F1`–`F9` go to tab n; `⌥0` shows the numbers. After a key of the tabs the strip shows each tab's number for a moment, small and
//! over a cell that is blank already, so no tab moves. Where the terminal reports every key as a code (the kitty protocol, `keys.hold`), ⌥
//! held alone for [`HOLD`] shows the numbers until it is let go; a terminal that cannot say ⌥ is held still sends ⌥1 (as `esc` then `1`), and
//! where the two come apart a bare Esc waits [`ESC_WAIT`] for the digit. Esc with nothing to close goes to the first tab, Chat. Tab moves no tab
//! and never types. The state is the screen's own: the core knows nothing of it.
//!
//! What each key does is the table [`KEYS`], one row a key (or a family of keys): the code that reads a key, the keys row while the
//! numbers show and the `?` sheet all read it.

use crate::app::App;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, ModifierKeyCode};
use j4n_proto::Up;
use std::time::{Duration, Instant};

/// How long the numbers stay on the strip after a key of the tabs.
pub const WINDOW: Duration = Duration::from_millis(1200);
/// How long a bare Esc waits for a digit, where the terminal may send ⌥digit as Esc and the digit in two reads.
pub const ESC_WAIT: Duration = Duration::from_millis(40);
/// How long ⌥ is held alone before the numbers show: an ⌥← or an ⌥digit pressed sooner shows none of the hold's, and no number flashes.
pub const HOLD: Duration = Duration::from_millis(150);

/// ⌥ held alone: since when (none until the clock is read), and whether its time is over (the numbers shown, or another key came).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hold {
    at: Option<Instant>,
    done: bool,
}

/// Whether an event of the terminal is a key to read: a release, a repeat of Tab (it would step through a list), a modifier or a lock
/// pressed alone and a media key are not. Where every key is reported as a code each of those comes as its own event.
pub fn reads(k: &KeyEvent) -> bool {
    let tab_repeat = k.kind == KeyEventKind::Repeat && matches!(k.code, KeyCode::Tab | KeyCode::BackTab);
    k.kind != KeyEventKind::Release && !tab_repeat && !matches!(k.code, KeyCode::Modifier(_) | KeyCode::CapsLock | KeyCode::NumLock | KeyCode::ScrollLock | KeyCode::Media(_))
}

/// What a key types where every key is reported as a code: Caps Lock is said beside the key and not in it, so a small letter with no other
/// modifier is made a capital here; with ⇧ as well the letter is left as it came. The locks are then dropped from the key, so a key with
/// Num Lock on is the same key as without.
pub fn typed(mut k: KeyEvent) -> KeyEvent {
    let others = KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER | KeyModifiers::HYPER | KeyModifiers::META;
    if let (KeyCode::Char(c), true, false) = (k.code, k.state.contains(KeyEventState::CAPS_LOCK), k.modifiers.intersects(others)) {
        let mut up = c.to_uppercase();
        if let (Some(u), None, true) = (up.next(), up.next(), c.is_lowercase()) {
            k.code = KeyCode::Char(u);
        }
    }
    k.state = KeyEventState::NONE;
    k
}

fn bare(k: &KeyEvent) -> bool {
    k.code == KeyCode::Esc && k.modifiers.is_empty()
}

/// What a key of the tabs asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    /// `⌥1`–`⌥9`: tab n, or the next sub-tab of it when it is the open one (on Chat, the next chat).
    Alt(u8),
    /// `F1`–`F9`: tab n, or the next sub-tab of it when it is the open one.
    Fkey(u8),
    /// `⌥0`: the numbers, and nothing moves.
    Numbers,
}

/// One row of what the keys of the tabs do: the keys as they are written, the few words of the keys row (none: the keys alone), what it
/// does in a line for the sheet, and whether the keys row lists it while the numbers show.
pub struct Row {
    pub keys: &'static str,
    pub words: &'static str,
    pub doc: &'static str,
    pub row: bool,
    /// The request a key of the row makes, or none when the key is not one of the row's (or is read elsewhere).
    read: fn(&KeyEvent) -> Option<Nav>,
}

/// A digit held with ⌥ and no ⌃.
fn alt_digit(k: &KeyEvent) -> Option<u8> {
    let KeyCode::Char(c @ '0'..='9') = k.code else { return None };
    (k.modifiers.contains(KeyModifiers::ALT) && !k.modifiers.contains(KeyModifiers::CONTROL)).then_some(c as u8 - b'0')
}

/// The keys of the tabs, the first to claim a key reading it.
pub const KEYS: [Row; 5] = [
    Row { keys: "⌥1–9", words: "tab", doc: "Tab n, from any page.", row: true, read: |k| alt_digit(k).filter(|d| *d > 0).map(Nav::Alt) },
    Row { keys: "⌥n again", words: "sub-tab", doc: "Its next sub-tab; on Chat, the next chat.", row: true, read: |_| None },
    Row { keys: "⌥←→", words: "sub-tabs", doc: "The sub-tab before or after.", row: true, read: |_| None },
    Row { keys: "⌥0", words: "numbers", doc: "The numbers of the tabs, for a moment.", row: false, read: |k| (alt_digit(k) == Some(0)).then_some(Nav::Numbers) },
    Row { keys: "F1–F9", words: "", doc: "Tab n, as ⌥n.", row: true, read: |k| if let KeyCode::F(n @ 1..=9) = k.code { Some(Nav::Fkey(n)) } else { None } },
];

/// What a key of the tabs is.
pub fn read(k: &KeyEvent) -> Option<Nav> {
    KEYS.iter().find_map(|r| (r.read)(k))
}

/// The numbers on the strip: since when they have been up (none until the clock is read).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Numbers {
    since: Option<Instant>,
}

impl App {
    /// The clock the screen was last told; keys are timed by it (a scene or a test sets it by waiting).
    pub fn set_clock(&mut self, now: Instant) {
        self.clock = Some(now);
    }

    /// Whether the numbers are on the tab strip.
    pub fn numbers_shown(&self) -> bool {
        self.numbers.is_some()
    }

    /// Whether ⌥ is held alone, as the terminal last said.
    pub fn alt_held(&self) -> bool {
        self.alt_held.is_some()
    }

    /// ⌥'s own press and release make the hold, and none is read as a key; any other event that is not a key to read ([`reads`]) is
    /// nothing. A key without ⌥ ends the hold too (a release the terminal never sent, as when the window lost the keys); one with it ends
    /// only the hold's wait. `None`: nothing more to read.
    pub(crate) fn hold_key(&mut self, k: KeyEvent) -> Option<KeyEvent> {
        if let KeyCode::Modifier(ModifierKeyCode::LeftAlt | ModifierKeyCode::RightAlt) = k.code {
            match k.kind {
                KeyEventKind::Press => self.alt_held = Some(Hold { at: self.clock, done: false }),
                KeyEventKind::Release => {
                    self.alt_held = None;
                    if self.numbers.take().is_some() {
                        self.redraw = true;
                    }
                }
                KeyEventKind::Repeat => {}
            }
            return None;
        }
        if !reads(&k) {
            return None;
        }
        if k.modifiers.contains(KeyModifiers::ALT) {
            if let Some(h) = self.alt_held.as_mut() {
                h.done = true;
            }
        } else {
            self.alt_held = None;
        }
        Some(typed(k))
    }

    /// The clock moved on: ⌥ held alone for [`HOLD`] shows the numbers, and while it is held none go; else the numbers go once their
    /// window has passed.
    pub(crate) fn numbers_tick(&mut self, now: Instant) {
        self.clock = Some(now);
        if let Some(h) = self.alt_held.as_mut() {
            let at = *h.at.get_or_insert(now);
            if !h.done && now.saturating_duration_since(at) >= HOLD {
                h.done = true;
                (self.numbers, self.redraw) = (Some(Numbers { since: Some(now) }), true);
            }
            return;
        }
        if let Some(n) = self.numbers.as_mut() {
            let since = *n.since.get_or_insert(now);
            if now.saturating_duration_since(since) >= WINDOW {
                self.numbers = None;
                self.redraw = true;
            }
        }
    }

    /// How long until the numbers go, while they show.
    pub(crate) fn numbers_wait(&self, now: Instant) -> Option<Duration> {
        if let Some(h) = self.alt_held {
            return (!h.done).then(|| h.at.map_or(Duration::ZERO, |at| (at + HOLD).saturating_duration_since(now)));
        }
        let since = self.numbers.as_ref()?.since?;
        Some((since + WINDOW).saturating_duration_since(now))
    }

    /// A key as it came, with the bare Esc that may wait: `(what the Esc that waited did, the key to read now)`. A digit within
    /// [`ESC_WAIT`] of the Esc is ⌥digit and the Esc does nothing; any other key lets the Esc act first. The Esc waits only where the
    /// terminal may split the pair (`esc_waits`) and the clock is told.
    pub(crate) fn esc_pair(&mut self, k: KeyEvent) -> (Vec<Up>, Option<KeyEvent>) {
        let mut ups = Vec::new();
        if let Some(at) = self.esc_held.take() {
            let soon = self.clock.is_some_and(|now| now.saturating_duration_since(at) < ESC_WAIT);
            if let (true, KeyCode::Char(d @ '0'..='9'), true) = (soon, k.code, k.modifiers.is_empty()) {
                return (ups, Some(KeyEvent::new(KeyCode::Char(d), KeyModifiers::ALT)));
            }
            ups = self.key_now(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        }
        // an Esc in the burst after a kept key is not held: `key_in` drops it as it comes
        if let (true, true, None, Some(now)) = (self.esc_waits, bare(&k), self.burst, self.clock) {
            self.esc_held = Some(now);
            return (ups, None);
        }
        (ups, Some(k))
    }

    /// The bare Esc that waited its time with no digit after it acts now.
    pub(crate) fn esc_due(&mut self, now: Instant) -> Vec<Up> {
        match self.esc_held {
            Some(at) if now.saturating_duration_since(at) >= ESC_WAIT => {
                (self.esc_held, self.redraw) = (None, true);
                self.key_now(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            }
            _ => Vec::new(),
        }
    }

    /// How long until the bare Esc that waits acts.
    pub(crate) fn esc_wait(&self, now: Instant) -> Option<Duration> {
        self.esc_held.map(|at| (at + ESC_WAIT).saturating_duration_since(now))
    }

    /// Esc with nothing to close: the first tab, Chat. The tab left keeps its sub-tab and its scroll; on Chat, nothing.
    pub(crate) fn home(&mut self) -> Vec<Up> {
        if self.tab == 0 || self.top().is_none_or(|t| t.children.is_empty()) {
            return Vec::new();
        }
        self.go_tab(0, false)
    }

    /// Go to tab `to`, and on to its next sub-tab when `step` and it has sub-tabs.
    fn go_tab(&mut self, to: usize, step: bool) -> Vec<Up> {
        (self.tab, self.pin) = (to, None);
        if !step {
            return self.sync();
        }
        if self.inner_tabs().is_some() {
            return self.step_sub(true);
        }
        // the Chat tab has no sub-tabs, so its own number is free to mean the next chat: the one that waits, then the one that runs
        let mut ups = self.sync();
        if self.field().is_some_and(|f| f.kind == j4n_proto::FieldKind::Chat) && self.here().is_some() {
            ups.extend(App::enter("@next"));
        }
        ups
    }

    /// A key of the tabs, read before any field sees it: `Some` when it was taken. Any other key takes the numbers down and goes on.
    pub(crate) fn nav_key(&mut self, k: KeyEvent) -> Option<Vec<Up>> {
        if self.numbers.take().is_some() {
            self.redraw = true;
        }
        // Tab moves no tab and never types: it completes (`App::tab_key`)
        if matches!(k.code, KeyCode::Tab | KeyCode::BackTab) {
            return Some(Vec::new());
        }
        // a value being typed, the jump, a find: the tab keys wait for them to end (nothing typed is lost to a tab)
        let nav = read(&k).filter(|_| !self.typing())?;
        let n = self.top().map_or(0, |t| t.children.len());
        if n == 0 {
            return None;
        }
        let here = self.tab.min(n - 1);
        self.redraw = true;
        if matches!(nav, Nav::Alt(_) | Nav::Numbers) {
            self.numbers = Some(Numbers { since: self.clock });
        }
        Some(match nav {
            Nav::Numbers => Vec::new(),
            Nav::Fkey(f) | Nav::Alt(f) if usize::from(f) > n => Vec::new(),
            // F of the open tab steps its sub-tabs; ⌥ of the open tab steps them too, or goes to the next chat
            Nav::Fkey(f) => self.go_tab(usize::from(f) - 1, usize::from(f) - 1 == here && self.inner_tabs().is_some()),
            Nav::Alt(d) => self.go_tab(usize::from(d) - 1, usize::from(d) - 1 == here),
        })
    }
}
