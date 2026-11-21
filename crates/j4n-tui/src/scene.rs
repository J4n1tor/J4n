//! A scene: a list of what happens to the screen (messages from the core, keys, a paste, a resize, time passing) and the sizes to
//! show it at. Running one draws it into memory and keeps a [`Shot`] of the end, and of each `Shot` step on the way. A shot is what a
//! golden holds. The scene files are TOML and are read by the tests; this layer reads no file.

use crate::App;
use crate::app::Preview;
use crate::theme::{Settings, Theme};
use crate::headless::{Headless, KeyCode, KeyEvent, Snap, press};
use crossterm::event::KeyModifiers;
use j4n_proto::{Down, Up};
use std::time::{Duration, Instant};

/// One thing that happens.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// A message from the core.
    Down(Box<Down>),
    Key(KeyEvent),
    /// Typed letters, one key each (no Enter unless the text holds one).
    Text(String),
    /// A paste: one piece of text, not keys.
    Paste(String),
    Resize(u16, u16),
    /// Time passes (milliseconds): the app is given the clock, for what waits on a pause.
    Wait(u64),
    /// A page of the theme in place of the screen (`roles`, `glyphs`), or the screen again.
    Preview(Option<Preview>),
    /// A picture of the screen as it is now, under this name.
    Shot(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub name: String,
    /// Each size the scene is run at, as `(columns, rows)`.
    pub sizes: Vec<(u16, u16)>,
    /// The look the scene is run with.
    pub theme: Settings,
    /// How prose is read and what the terminal can do: a scene is run on a terminal that is not asked, so the plain subset unless it says.
    pub read: crate::read::Opts,
    pub steps: Vec<Step>,
}

/// A picture of the screen, and what the app had sent by then.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shot {
    /// The scene's name; with `.<shot>` when a `Shot` step took it.
    pub name: String,
    pub cols: u16,
    pub rows: u16,
    pub snap: Snap,
    pub sent: Vec<Up>,
}

impl Shot {
    /// The name of its golden files, without the extension: `main.160x48`.
    pub fn file(&self) -> String {
        format!("{}.{}x{}", self.name, self.cols, self.rows)
    }
}

/// A key as it is written in a scene: `enter`, `esc`, `ctrl+k`, `alt+right`, `shift+tab`, `f1`, `a`, `space`.
pub fn key(word: &str) -> Option<KeyEvent> {
    let lower = word.to_lowercase();
    let parts: Vec<&str> = if lower == "+" { vec!["+"] } else { lower.split('+').collect() };
    let (name, mods) = parts.split_last()?;
    let mut modifiers = KeyModifiers::NONE;
    for m in mods {
        modifiers |= match *m {
            "ctrl" => KeyModifiers::CONTROL,
            "alt" => KeyModifiers::ALT,
            "shift" => KeyModifiers::SHIFT,
            _ => return None,
        };
    }
    let code = match *name {
        "enter" => KeyCode::Enter,
        "esc" | "escape" => KeyCode::Esc,
        "tab" => KeyCode::Tab,
        "backtab" => KeyCode::BackTab,
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "space" => KeyCode::Char(' '),
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" | "pgup" => KeyCode::PageUp,
        "pagedown" | "pgdn" => KeyCode::PageDown,
        f if f.starts_with('f') && f.len() > 1 => KeyCode::F(f[1..].parse().ok().filter(|n| (1..=12).contains(n))?),
        c if c.chars().count() == 1 => {
            // a capital letter stays one, and is typed with shift as a terminal sends it
            let typed = word.rsplit('+').next().and_then(|w| w.chars().next()).filter(|ch| ch.is_uppercase() && modifiers.is_empty()).unwrap_or(c.chars().next()?);
            if typed.is_uppercase() {
                modifiers |= KeyModifiers::SHIFT;
            }
            KeyCode::Char(typed)
        }
        _ => return None,
    };
    // shift+tab is the key the terminal calls back-tab
    let (code, modifiers) = if code == KeyCode::Tab && modifiers == KeyModifiers::SHIFT { (KeyCode::BackTab, KeyModifiers::SHIFT) } else { (code, modifiers) };
    Some(KeyEvent::new(code, modifiers))
}

/// Run the scene at each of its sizes (a scene with none is run at 80×24). The shots come size by size, in the order they were taken.
pub fn run(scene: &Scene) -> Vec<Shot> {
    let sizes = if scene.sizes.is_empty() { vec![(80, 24)] } else { scene.sizes.clone() };
    sizes.into_iter().flat_map(|(cols, rows)| run_at(scene, cols, rows)).collect()
}

fn run_at(scene: &Scene, cols: u16, rows: u16) -> Vec<Shot> {
    let mut app = App::new();
    app.read = scene.read.clone();
    app.theme = Theme::build(&scene.theme, &[]).unwrap_or_else(|e| panic!("scene `{}`: {e}", scene.name));
    let mut screen = Headless::new(cols, rows);
    let (mut size, mut sent, mut shots) = ((cols, rows), app.start(), Vec::new());
    let (start, mut waited) = (Instant::now(), Duration::ZERO);
    // the clock starts with the scene, so the breath is at its first tick
    sent.extend(app.tick(start));
    let mut take = |app: &mut App, screen: &mut Headless, size: (u16, u16), sent: &[Up], name: String| {
        shots.push(Shot { name, cols: size.0, rows: size.1, snap: screen.snap(app), sent: sent.to_vec() });
    };
    for step in &scene.steps {
        match step {
            Step::Down(d) => sent.extend(app.down((**d).clone())),
            Step::Key(k) => sent.extend(app.key(*k)),
            Step::Text(t) => t.chars().for_each(|c| sent.extend(app.key(press(if c == '\n' { KeyCode::Enter } else { KeyCode::Char(c) })))),
            Step::Paste(t) => app.paste(t),
            Step::Resize(c, r) => {
                (screen, size) = (Headless::new(*c, *r), (*c, *r));
                sent.push(Up::Resize { cols: *c, rows: *r });
            }
            Step::Wait(ms) => {
                waited += Duration::from_millis(*ms);
                sent.extend(app.tick(start + waited));
            }
            Step::Preview(p) => app.preview = *p,
            Step::Shot(name) => take(&mut app, &mut screen, size, &sent, format!("{}.{name}", scene.name)),
        }
    }
    take(&mut app, &mut screen, size, &sent, scene.name.clone());
    shots
}
