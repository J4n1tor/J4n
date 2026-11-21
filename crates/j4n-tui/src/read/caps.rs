//! What the terminal can draw, asked and not guessed: `XTVERSION` names the terminal, `TERM_PROGRAM` and `TERM` name it when it does
//! not answer, and the name picks the features. A terminal that is not known gets the plain subset: no links that open, no italics.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Caps {
    /// OSC 8 links that open.
    pub osc8: bool,
    pub italic: bool,
    pub undercurl: bool,
    /// Kitty's text sizing protocol (OSC 66), kitty 0.40 and later.
    pub sizing: bool,
}

impl Caps {
    pub const PLAIN: Caps = Caps { osc8: false, italic: false, undercurl: false, sizing: false };
    pub const ALL: Caps = Caps { osc8: true, italic: true, undercurl: true, sizing: true };

    /// The features of the terminal called `name` (any case), at `version` (major, minor) when the reply said one.
    pub fn named(name: &str, version: Option<(u32, u32)>) -> Caps {
        let name = name.to_ascii_lowercase();
        let has = |w: &str| name.contains(w);
        if has("kitty") {
            Caps { sizing: version.is_some_and(|v| v >= (0, 40)), ..Caps { osc8: true, italic: true, undercurl: true, sizing: false } }
        } else if has("ghostty") || has("wezterm") || has("foot") {
            Caps { osc8: true, italic: true, undercurl: true, sizing: false }
        } else if has("iterm") || has("alacritty") {
            Caps { osc8: true, italic: true, undercurl: false, sizing: false }
        } else {
            Caps::PLAIN
        }
    }

    /// Whether the terminal is asked at all: not in a test, not when `J4N_PROBE=0` or `TERM=dumb`, not over ssh (a reply that comes
    /// after the wait would be typed into the draft), and not when stdin or stdout is not a terminal. `env` reads a variable.
    pub fn asks(env: &dyn Fn(&str) -> Option<String>, stdin_tty: bool, stdout_tty: bool) -> bool {
        let set = |k: &str| env(k).is_some_and(|v| !v.is_empty());
        stdin_tty && stdout_tty && !set("J4N_HERMETIC") && env("J4N_PROBE").as_deref() != Some("0") && env("TERM").as_deref() != Some("dumb") && !set("SSH_CONNECTION") && !set("SSH_TTY")
    }

    /// The features from the reply to `XTVERSION` (`kitty(0.40.1)`, `WezTerm 20240203`, `ghostty 1.1.0`) when there is one, else from the
    /// environment. `env` reads a variable; nothing here touches the process, so a test says what the terminal is.
    pub fn detect(xtversion: Option<&str>, env: &dyn Fn(&str) -> Option<String>) -> Caps {
        if let Some(reply) = xtversion.map(str::trim).filter(|r| !r.is_empty()) {
            let (name, rest) = reply.split_once(['(', ' ']).unwrap_or((reply, ""));
            return Caps::named(name, version(rest));
        }
        if env("KITTY_WINDOW_ID").is_some() {
            return Caps::named("kitty", env("KITTY_PID").map(|_| (0, 0)));
        }
        let word = |k: &str| env(k).unwrap_or_default();
        let program = word("TERM_PROGRAM");
        let by = if program.is_empty() { word("TERM") } else { program };
        Caps::named(&by, None)
    }
}

/// `0.40.1)` or `1.1.0` as (major, minor).
fn version(s: &str) -> Option<(u32, u32)> {
    let mut parts = s.trim_matches(|c: char| !c.is_ascii_digit()).split(|c: char| !c.is_ascii_digit());
    Some((parts.next()?.parse().ok()?, parts.next().and_then(|p| p.parse().ok()).unwrap_or(0)))
}

/// The name and the version out of the bytes the terminal answered `CSI > 0 q` with (`DCS > | text ST`).
pub fn xtversion(reply: &[u8]) -> Option<String> {
    let at = reply.windows(3).position(|w| w == [0x1b, b'P', b'>'])?;
    let body = &reply[at + 3..];
    let body = body.strip_prefix(b"|")?;
    let end = body.iter().position(|b| *b == 0x1b || *b == 0x07).unwrap_or(body.len());
    String::from_utf8(body[..end].to_vec()).ok()
}

/// How long after the question a reply that came late is still told from keys.
pub(crate) const LATE: Duration = Duration::from_secs(2);

/// What crossterm makes of a terminal's late reply to the question: `ESC P > | name ESC \` arrives as Alt-P and the characters after it, and
/// `ESC [ ? 62 c` as Alt-[ and its characters. For `LATE` after the question those are told from keys and dropped, so none of them is typed.
#[derive(Debug, Clone, Copy, Default)]
pub struct Late {
    until: Option<Instant>,
    state: Eaten,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Eaten {
    #[default]
    Idle,
    /// Inside `DCS` (the count of keys taken).
    Dcs(u8),
    /// Inside `CSI ?` (the count of keys taken).
    Csi(u8),
}

impl Late {
    /// Watching until `LATE` after `asked`.
    pub fn after(asked: Instant) -> Late {
        Late { until: Some(asked + LATE), state: Eaten::Idle }
    }

    /// Whether `k`, read at `now`, is a part of a late reply (and so is dropped).
    pub fn eat(&mut self, now: Instant, k: &KeyEvent) -> bool {
        if self.until.is_none_or(|u| now >= u) {
            *self = Late::default();
            return false;
        }
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let (state, took) = match (self.state, k.code) {
            (Eaten::Idle, KeyCode::Char('P')) if alt => (Eaten::Dcs(0), true),
            (Eaten::Idle, KeyCode::Char('[')) if alt => (Eaten::Csi(0), true),
            (Eaten::Dcs(0), KeyCode::Char('>')) => (Eaten::Dcs(1), true),
            (Eaten::Dcs(n), KeyCode::Char('\\')) if n > 0 && alt => (Eaten::Idle, true),
            (Eaten::Dcs(n), KeyCode::Char(_)) if n > 0 && n < 200 => (Eaten::Dcs(n + 1), true),
            (Eaten::Csi(0), KeyCode::Char('?')) => (Eaten::Csi(1), true),
            (Eaten::Csi(n), KeyCode::Char('c')) if n > 0 => (Eaten::Idle, true),
            (Eaten::Csi(n), KeyCode::Char(c)) if n > 0 && n < 40 && (c.is_ascii_digit() || c == ';') => (Eaten::Csi(n + 1), true),
            _ => (Eaten::Idle, false),
        };
        self.state = state;
        took
    }
}
