//! The keymap the core sent (`Down::Keys`): what a pressed key is taken for. The screen's own code keeps
//! reading the keys it always read; a key the operator bound is read as the one of them it stands for, a key that was freed is read
//! as nothing, a page's key sends its intent. Until the map comes every key is read as it is.

use crate::app::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use j4n_proto::keys::{Code, Key};
use j4n_proto::{Bind, FieldKind, Intent, Up};
use std::collections::HashMap;

enum To {
    Key(Key),
    Run(Intent),
    Free,
}

/// What a key does in each context, as the core compiled it.
#[derive(Default)]
pub struct Keymap {
    binds: HashMap<Key, Vec<(String, To)>>,
}

impl Keymap {
    pub fn of(binds: Vec<Bind>) -> Keymap {
        let mut map = Keymap::default();
        for b in binds {
            let Ok(key) = b.key.parse::<Key>() else { continue };
            let to = match (b.run, b.to.parse::<Key>()) {
                (Some(run), _) => To::Run(run),
                (None, Ok(to)) => To::Key(to),
                _ => To::Free,
            };
            map.binds.entry(key).or_default().push((b.context, to));
        }
        map
    }

    /// What `key` does in the first of `contexts` that binds it; a freed key only when no context binds it.
    fn find(&self, contexts: &[String], key: &Key) -> Option<&To> {
        let here = self.binds.get(key)?;
        let found = contexts.iter().filter_map(|c| here.iter().find(|(h, _)| h == c).map(|(_, to)| to));
        let all: Vec<&To> = found.collect();
        all.iter().find(|t| !matches!(t, To::Free)).or(all.first()).copied()
    }
}

/// A key as a terminal reported it, in the names of the keymap.
pub fn key_of(k: &KeyEvent) -> Option<Key> {
    let has = |m| k.modifiers.contains(m);
    let code = match k.code {
        KeyCode::Char(c) => Code::Char(c),
        KeyCode::Enter => Code::Enter,
        KeyCode::Tab | KeyCode::BackTab => Code::Tab,
        KeyCode::Esc => Code::Esc,
        KeyCode::Up => Code::Up,
        KeyCode::Down => Code::Down,
        KeyCode::Left => Code::Left,
        KeyCode::Right => Code::Right,
        KeyCode::PageUp => Code::PgUp,
        KeyCode::PageDown => Code::PgDn,
        KeyCode::Home => Code::Home,
        KeyCode::End => Code::End,
        KeyCode::Backspace => Code::Backspace,
        KeyCode::Delete => Code::Delete,
        KeyCode::F(n) => Code::F(n),
        _ => return None,
    };
    Some(Key::new(code, has(KeyModifiers::CONTROL), has(KeyModifiers::ALT), has(KeyModifiers::SHIFT) || k.code == KeyCode::BackTab))
}

/// The event a terminal would report for a key.
fn event_of(k: Key) -> KeyEvent {
    let mut mods = KeyModifiers::NONE;
    for (on, m) in [(k.ctrl, KeyModifiers::CONTROL), (k.alt, KeyModifiers::ALT), (k.shift, KeyModifiers::SHIFT)] {
        if on {
            mods |= m;
        }
    }
    let code = match k.code {
        Code::Char(c) => {
            if c.is_uppercase() {
                mods |= KeyModifiers::SHIFT;
            }
            KeyCode::Char(c)
        }
        Code::Tab if k.shift => KeyCode::BackTab,
        Code::Tab => KeyCode::Tab,
        Code::Enter => KeyCode::Enter,
        Code::Esc => KeyCode::Esc,
        Code::Up => KeyCode::Up,
        Code::Down => KeyCode::Down,
        Code::Left => KeyCode::Left,
        Code::Right => KeyCode::Right,
        Code::PgUp => KeyCode::PageUp,
        Code::PgDn => KeyCode::PageDown,
        Code::Home => KeyCode::Home,
        Code::End => KeyCode::End,
        Code::Backspace => KeyCode::Backspace,
        Code::Delete => KeyCode::Delete,
        Code::F(n) => KeyCode::F(n),
    };
    KeyEvent::new(code, mods)
}

/// What a key came to.
pub(crate) enum Step {
    /// Read this key.
    Go(KeyEvent),
    /// Done: these are the messages for the core.
    Done(Vec<Up>),
}

impl App {
    /// The places a key is read now, the nearest first: the pages on the way down to the one that has the keys (not for a character that
    /// goes into a line), the field that has them, the screen.
    fn contexts(&self, key: &Key) -> Vec<String> {
        let kind = self.field().map(|f| f.kind);
        let mut out = Vec::new();
        if !(key.types() && kind == Some(FieldKind::Chat)) {
            out.extend(self.trail().iter().rev().map(|(n, _)| format!("page:{}", n.id)));
        }
        if kind == Some(FieldKind::Chat) {
            let on_transcript = self.field().is_some_and(|f| self.wins.get(&f.bind).is_some_and(|w| w.at.is_some()));
            if on_transcript {
                out.push("transcript".into());
            } else {
                if self.open_ask().is_some() {
                    out.push("ask".into());
                }
                out.push("composer".into());
            }
        } else {
            out.push("list".into());
        }
        out.push("global".into());
        out
    }

    /// Whether a key is text for a field being typed into, which no keymap reads.
    pub(crate) fn typing(&self) -> bool {
        self.edit.is_some() || self.jump.is_some() || self.val.is_some() || self.composer.focus || self.field().is_some_and(|f| self.wins.get(&f.bind).is_some_and(|w| w.typing.is_some()))
    }

    /// A pressed key, read through the keymap.
    pub(crate) fn mapped(&self, k: KeyEvent) -> Step {
        let Some(map) = self.keymap.as_ref().filter(|_| !self.typing()) else { return Step::Go(k) };
        let Some(key) = key_of(&k) else { return Step::Go(k) };
        match map.find(&self.contexts(&key), &key) {
            Some(To::Key(to)) if *to != key => Step::Go(event_of(*to)),
            Some(To::Run(intent)) => Step::Done(vec![Up::Intent { intent: intent.clone() }]),
            Some(To::Free) => Step::Done(Vec::new()),
            _ => Step::Go(k),
        }
    }
}
