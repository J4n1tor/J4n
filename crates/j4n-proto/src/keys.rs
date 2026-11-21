//! Key names: how a key is written in a keymap and in what the core tells the screen. One grammar for the
//! core, which checks the names a file gives, and the screen, which turns what a terminal reports into the same names.

use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Code {
    Char(char),
    Enter,
    Tab,
    Esc,
    Up,
    Down,
    Left,
    Right,
    PgUp,
    PgDn,
    Home,
    End,
    Backspace,
    Delete,
    F(u8),
}

/// A key with the modifiers held. Written `C-` (ctrl) `M-` (alt) `S-` (shift), in that order, then a name or one character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key {
    pub code: Code,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// The names of the keys that are not one character, the one a key is written with first.
const NAMES: [(&str, Code); 18] = [
    ("Enter", Code::Enter),
    ("Return", Code::Enter),
    ("Tab", Code::Tab),
    ("Esc", Code::Esc),
    ("Escape", Code::Esc),
    ("Up", Code::Up),
    ("Down", Code::Down),
    ("Left", Code::Left),
    ("Right", Code::Right),
    ("PgUp", Code::PgUp),
    ("PageUp", Code::PgUp),
    ("PgDn", Code::PgDn),
    ("PageDown", Code::PgDn),
    ("Home", Code::Home),
    ("End", Code::End),
    ("Backspace", Code::Backspace),
    ("Delete", Code::Delete),
    ("Del", Code::Delete),
];

impl Key {
    /// A key as a terminal reports it: a letter typed with shift is the capital letter and has no shift of its own.
    pub fn new(code: Code, ctrl: bool, alt: bool, shift: bool) -> Key {
        match code {
            Code::Char(c) => Key { code: Code::Char(if shift { c.to_ascii_uppercase() } else { c }), ctrl, alt, shift: false },
            _ => Key { code, ctrl, alt, shift },
        }
    }

    /// Whether this is a character that goes into a line being typed: no ctrl and no alt.
    pub fn types(&self) -> bool {
        matches!(self.code, Code::Char(_)) && !self.ctrl && !self.alt
    }
}

impl FromStr for Key {
    type Err = String;

    fn from_str(s: &str) -> Result<Key, String> {
        let (mut ctrl, mut alt, mut shift, mut rest) = (false, false, false, s);
        while rest.len() > 2 && rest.as_bytes()[1] == b'-' {
            match rest.as_bytes()[0] {
                b'C' => ctrl = true,
                b'M' => alt = true,
                b'S' => shift = true,
                _ => break,
            }
            rest = &rest[2..];
        }
        let named = NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(rest)).map(|(_, c)| *c);
        let f = rest.strip_prefix(['F', 'f']).and_then(|n| n.parse::<u8>().ok()).filter(|n| (1..=12).contains(n));
        let mut chars = rest.chars();
        let code = match (named, f, chars.next(), chars.next()) {
            (Some(c), ..) => c,
            (_, Some(n), ..) => Code::F(n),
            _ if rest.eq_ignore_ascii_case("space") => Code::Char(' '),
            _ if rest.eq_ignore_ascii_case("backtab") => {
                shift = true;
                Code::Tab
            }
            (_, _, Some(c), None) => Code::Char(c),
            _ => return Err(format!("`{s}` is not a key: write Enter, Tab, Esc, Up, Down, Left, Right, PgUp, PgDn, Home, End, Backspace, Delete, F1–F12, Space or one character, after C- M- S- as needed")),
        };
        Ok(Key::new(code, ctrl, alt, shift))
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (on, mark) in [(self.ctrl, "C-"), (self.alt, "M-"), (self.shift, "S-")] {
            if on {
                f.write_str(mark)?;
            }
        }
        match self.code {
            Code::Char(' ') => f.write_str("Space"),
            Code::Char(c) => write!(f, "{c}"),
            Code::F(n) => write!(f, "F{n}"),
            code => f.write_str(NAMES.iter().find(|(_, c)| *c == code).map_or("?", |(n, _)| n)),
        }
    }
}

/// Why a key cannot be bound, or none. tmux takes some chords before J4n sees them, and a terminal
/// sends some control keys as the key they are the same byte as.
pub fn refused(k: &Key) -> Option<&'static str> {
    let arrow = matches!(k.code, Code::Up | Code::Down | Code::Left | Code::Right);
    let tmux = match k.code {
        Code::Char(' ' | 'b') => k.ctrl && !k.alt,
        Code::Enter | Code::Esc => k.alt,
        Code::Char('1'..='9') => k.alt,
        _ => k.alt && arrow,
    };
    if tmux {
        return Some("tmux takes it before J4n sees it");
    }
    match k.code {
        Code::Char('i') if k.ctrl && !k.alt => Some("a terminal sends ⌃i as Tab: bind Tab"),
        Code::Char('m') if k.ctrl && !k.alt => Some("a terminal sends ⌃m as Enter: bind Enter"),
        Code::Char('[') if k.ctrl && !k.alt => Some("a terminal sends ⌃[ as Esc: bind Esc"),
        _ => None,
    }
}
