//! The same screen drawn into memory, so a script can type, look at what is shown and type again. What a test sees here is
//! what the operator sees, character for character.

use crate::app::App;
use crate::theme::palette::Role;
use crate::{golden, view};
pub use crossterm::event::{KeyCode, KeyEvent};
use crossterm::event::KeyModifiers;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;

/// One frame as a golden keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snap {
    pub text: String,
    pub ansi: String,
    pub cursor: (u16, u16),
}

pub struct Headless {
    terminal: Terminal<TestBackend>,
}

impl Headless {
    pub fn new(cols: u16, rows: u16) -> Headless {
        // a test backend has no terminal behind it to fail
        Headless { terminal: Terminal::new(TestBackend::new(cols, rows)).expect("a memory backend") }
    }

    /// The cells of the last frame, for a test that reads the colour of a cell.
    pub fn buffer(&self) -> &ratatui::buffer::Buffer {
        self.terminal.backend().buffer()
    }

    /// Draw the app and keep nothing of it: the cost of a frame, without the cost of reading it back as text.
    pub fn draw(&mut self, app: &mut App) {
        self.terminal.draw(|f| view::draw(app, f)).expect("a memory backend draws");
    }

    /// Draw the app and return what is shown: one string, a line per row, the blanks at the end of each cut off.
    pub fn show(&mut self, app: &mut App) -> String {
        self.terminal.draw(|f| view::draw(app, f)).expect("a memory backend draws");
        golden::text(self.terminal.backend().buffer())
    }

    /// Draw the app and return the screen the way a golden keeps it: the text, the same with its style as ANSI, and the cursor.
    pub fn snap(&mut self, app: &mut App) -> Snap {
        let text = self.show(app);
        Snap { text, ansi: golden::ansi(self.terminal.backend().buffer()), cursor: self.cursor() }
    }

    /// How the screen is coloured: a row of letters for each row shown, one for each cell — `.` text, `d` dim, `g` ok, `y` warn,
    /// `r` error, `c` accent, `?` any other. The letters name the roles of the app's theme, not colours.
    pub fn tones(&mut self, app: &mut App) -> Vec<String> {
        self.terminal.draw(|f| view::draw(app, f)).expect("a memory backend draws");
        let buffer = self.terminal.backend().buffer();
        let role = |r: Role| {
            let c = app.theme.roles.get(r);
            Color::Rgb(c.0, c.1, c.2)
        };
        let (text, dim, ok, warn, error, accent) = (role(Role::Text), role(Role::Dim), role(Role::Ok), role(Role::Warn), role(Role::Error), role(Role::Accent));
        buffer
            .content()
            .chunks(buffer.area.width as usize)
            .map(|row| {
                row.iter()
                    .map(|c| match c.fg {
                        f if f == text || f == Color::Reset => '.',
                        f if f == dim => 'd',
                        f if f == ok => 'g',
                        f if f == warn => 'y',
                        f if f == error => 'r',
                        f if f == accent => 'c',
                        _ => '?',
                    })
                    .collect()
            })
            .collect()
    }

    /// The rows drawn with the selection behind them (the band of `sel`, reversed without colour), as text with the blanks at the end cut
    /// off: the row the keys are on in a list, a tray or a stack of fields. The pipe in front of it is the same as in any other row.
    pub fn banded(&mut self, app: &mut App) -> Vec<String> {
        self.terminal.draw(|f| view::draw(app, f)).expect("a memory backend draws");
        let buffer = self.terminal.backend().buffer();
        let sel = app.theme.back(Role::Sel);
        let text = golden::text(buffer);
        buffer
            .content()
            .chunks(buffer.area.width as usize)
            .zip(text.lines())
            .filter(|(row, _)| row.iter().any(|c| if app.theme.no_color { c.modifier.contains(ratatui::style::Modifier::REVERSED) } else { c.bg == sel.bg.unwrap_or(Color::Reset) }))
            .map(|(_, line)| line.to_string())
            .collect()
    }

    /// Where the cursor is, as `(column, row)`.
    pub fn cursor(&mut self) -> (u16, u16) {
        self.terminal.get_cursor_position().map_or((0, 0), |p| (p.x, p.y))
    }
}

pub fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

pub fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

pub fn alt(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT)
}

/// A named key held with alt (alt-right is the next tab).
pub fn alt_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::ALT)
}

/// The keys that type `text`, then Enter if `enter`.
pub fn typed(text: &str, enter: bool) -> Vec<KeyEvent> {
    text.chars().map(|c| press(KeyCode::Char(c))).chain(enter.then(|| press(KeyCode::Enter))).collect()
}
