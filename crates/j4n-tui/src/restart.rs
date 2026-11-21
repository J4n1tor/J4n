//! The restart card: after the operator changes a setting that acts only from the next start, a small card over
//! the page asks "restart now? ⏎ / later esc". `⏎` sends `:restart` (the guard starts the build on disk again, the chat where it was);
//! `esc` leaves it for later, and the row keeps saying `next start` and `r` on it restarts. It is a `parts::popup` box; the keys are its
//! own while it is up.

use crate::app::App;
use crate::parts::popup::boxed;
use crate::theme::glyph::waits;
use crate::wrap::fit;
use crossterm::event::{KeyCode, KeyEvent};
use j4n_proto::{Intent, Tone, Up};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

impl App {
    /// A key while the card is up: `⏎` restarts, `esc` is later; any other key is the card's and does nothing.
    pub(crate) fn restart_key(&mut self, k: KeyEvent) -> Vec<Up> {
        match k.code {
            KeyCode::Enter => {
                self.restart = None;
                vec![Up::Intent { intent: Intent::Run { command: "restart".into(), args: Vec::new() } }]
            }
            KeyCode::Esc => {
                self.restart = None;
                Vec::new()
            }
            _ => Vec::new(),
        }
    }
}

/// The name the restart card says for a row whose key acts from the next start and leaves it other than at the start (`restart_after`,
/// a pack of Settings › Packs); a setting row has its own check (`fields/edit.rs`).
pub(crate) fn after(row: &j4n_proto::Row) -> Option<String> {
    row.data.as_ref().and_then(|d| d.get("restart_after")).and_then(j4n_proto::Json::as_bool).filter(|b| *b).map(|_| row.key.clone())
}

/// The card, in the middle of the page, above the keys row.
pub fn draw(app: &App, area: Rect, f: &mut Frame) {
    let Some(id) = app.restart.as_deref() else { return };
    let th = &app.theme;
    let room = usize::from(area.width).saturating_sub(8);
    let said = fit(&format!("{id} · {}", waits(th.tier)), room);
    let ask = fit("restart now? ⏎ / later esc", room);
    let w = (said.width().max(ask.width()) as u16 + 4).max(24).min(area.width);
    if area.height < 8 || w < 12 {
        return;
    }
    let r = Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - 5) / 2, width: w, height: 4 };
    let inside = boxed(th, f, r, "restart");
    let lines = vec![Line::from(Span::styled(said, th.tone(Tone::Warn))), Line::from(Span::styled(ask, th.tone(Tone::Accent)))];
    f.render_widget(Paragraph::new(lines), inside);
}
