//! `toast`: one line of news in its tone, in a box above the keys row, in the middle; on a screen too short for a box, the line alone.

use super::popup::boxed;
use crate::app::App;
use crate::wrap::fit;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

pub fn toast(app: &App, area: Rect, f: &mut Frame) {
    let Some((text, tone)) = app.toast.clone() else { return };
    let th = &app.theme;
    if area.height < 6 || area.width < 12 {
        let at = Rect { y: area.bottom().saturating_sub(2), height: 1, ..area };
        f.render_widget(Paragraph::new(fit(&text, area.width as usize)).style(th.tone(tone)), at);
        return;
    }
    let text = fit(&text, usize::from(area.width).saturating_sub(6));
    let w = (text.width() as u16 + 4).max(12);
    let r = Rect { x: area.x + (area.width - w) / 2, y: area.bottom() - 4, width: w, height: 3 };
    let inside = boxed(th, f, r, "");
    f.render_widget(Paragraph::new(Span::styled(text, th.tone(tone))), inside);
}
