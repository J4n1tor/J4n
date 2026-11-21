//! `line`: a line of text and the cursor in it, in characters. The one editor under the composer, a value field, the edit of a row, the find
//! of a table and the query of the jump.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Typed {
    pub buf: String,
    /// The cursor, as a number of characters into `buf`.
    pub at: usize,
}

/// The text is wiped when the line goes: a line can hold a key (the secret field's, or one pasted where it does not go).
impl Drop for Typed {
    fn drop(&mut self) {
        j4n_proto::Zeroize::zeroize(&mut self.buf);
    }
}

impl Typed {
    /// `text`, the cursor at its end.
    pub fn new(text: impl Into<String>) -> Typed {
        let buf = text.into();
        Typed { at: buf.chars().count(), buf }
    }

    /// The text, leaving the line empty.
    pub fn take(&mut self) -> String {
        self.at = 0;
        std::mem::take(&mut self.buf)
    }

    pub fn chars(&self) -> usize {
        self.buf.chars().count()
    }

    pub fn insert(&mut self, c: char) {
        self.buf.insert(self.buf.char_indices().nth(self.at).map_or(self.buf.len(), |(i, _)| i), c);
        self.at += 1;
    }

    /// The character before the cursor goes, or (`forward`) the one at it. True when there was one.
    pub fn erase(&mut self, forward: bool) -> bool {
        let at = if forward { self.at } else { self.at.saturating_sub(1) };
        let Some((i, c)) = self.buf.char_indices().nth(at).filter(|_| forward || self.at > 0) else { return false };
        self.buf.replace_range(i..i + c.len_utf8(), "");
        self.at = at;
        true
    }

    /// `← → Home End ⌃a ⌃e`. True when the key was one of them.
    pub fn cursor_key(&mut self, k: &KeyEvent) -> bool {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Left => self.at = self.at.saturating_sub(1),
            KeyCode::Right => self.at = (self.at + 1).min(self.chars()),
            KeyCode::Home => self.at = 0,
            KeyCode::End => self.at = self.chars(),
            KeyCode::Char('a') if ctrl => self.at = 0,
            KeyCode::Char('e') if ctrl => self.at = self.chars(),
            _ => return false,
        }
        true
    }

    /// A printable key types, `Backspace` and `Delete` erase. True when the text changed.
    pub fn edit(&mut self, k: &KeyEvent) -> bool {
        match k.code {
            KeyCode::Char(c) if !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => self.insert(c),
            KeyCode::Backspace => return self.erase(false),
            KeyCode::Delete => return self.erase(true),
            _ => return false,
        }
        true
    }
}
