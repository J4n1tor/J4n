//! `popup`: the frame of a box, from glyph slots: the theme's `Box*` slots (so `theme.corners` applies) or the heavy ones of the card the keys
//! are on. `edge` is one frame line, `frame` rows of spans inside two of them (the composer, a card, the ask), `boxed` a box on a cleared
//! ground over the page (the `?` sheet, the jump, the toast), `rows` the ask's box.

use crate::theme::glyph::Slot;
use crate::theme::palette::Role as Hue;
use crate::theme::{Theme, TitlePlace};
use crate::wrap::fit;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

/// The slots a frame is drawn with: the corners (top left, top right, bottom left, bottom right), the rule and the side.
pub type Glyphs = [Slot; 6];
pub const BOX: Glyphs = [Slot::BoxTl, Slot::BoxTr, Slot::BoxBl, Slot::BoxBr, Slot::BoxH, Slot::BoxV];
/// The square frame of a dropdown, whatever `theme.corners` says.
pub const POP: Glyphs = [Slot::PopTl, Slot::PopTr, Slot::PopBl, Slot::PopBr, Slot::BoxH, Slot::BoxV];
pub const HEAVY: Glyphs = [Slot::HeavyTl, Slot::HeavyTr, Slot::HeavyBl, Slot::HeavyBr, Slot::RuleHeavy, Slot::Focus];

/// The top (`bottom` false) or bottom line of a frame `w` columns wide: corners and the rule, and on the top `╭─ title ─` (bold) unless
/// `theme.title_place` is `none` or there is no room.
pub fn edge(th: &Theme, w: usize, title: &str, bottom: bool, style: Style, g: &Glyphs) -> Line<'static> {
    let (h, inner) = (th.glyph(g[4]), w.saturating_sub(2));
    let mut spans = vec![Span::styled(th.glyph(g[usize::from(bottom) * 2]), style)];
    let room = inner.saturating_sub(4);
    if bottom || th.title_place == TitlePlace::None || title.is_empty() || room < 2 {
        spans.push(Span::styled(h.repeat(inner), style));
    } else {
        let title = fit(title, room);
        spans.push(Span::styled(format!("{h} "), style));
        spans.push(Span::styled(title.clone(), th.fg(Hue::Text).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(format!(" {}", h.repeat(inner.saturating_sub(title.width() + 3))), style));
    }
    spans.push(Span::styled(th.glyph(g[usize::from(bottom) * 2 + 1]), style));
    Line::from(spans)
}

/// Rows of spans, each padded to the room inside, between a top edge (with `title`) and a bottom edge `w` columns wide.
pub fn frame(th: &Theme, body: Vec<Vec<Span<'static>>>, w: usize, title: &str, style: Style, g: &Glyphs) -> Vec<Line<'static>> {
    let side = || Span::styled(th.glyph(g[5]), style);
    let mut out = vec![edge(th, w, title, false, style, g)];
    for mut spans in body {
        spans.push(Span::raw(" ".repeat(w.saturating_sub(2).saturating_sub(spans.iter().map(|s| s.content.width()).sum::<usize>()))));
        out.push(Line::from([vec![side()], spans, vec![side()]].concat()));
    }
    out.push(edge(th, w, "", true, style, g));
    out
}

/// A box over `r`: cleared, filled, framed, titled. What is returned is the room inside the line and its padding.
pub fn boxed(th: &Theme, f: &mut Frame, r: Rect, title: &str) -> Rect {
    // a box one cell from the edge takes that cell too: what is behind it (the corner of the composer) would show in it as `╭│`
    let area = f.area();
    let margin = Rect { x: r.x.saturating_sub(u16::from(r.x == area.x + 1)), width: r.width + u16::from(r.x == area.x + 1) + u16::from(r.right() + 1 == area.right()), ..r };
    f.render_widget(Clear, margin);
    f.buffer_mut().set_style(margin, th.base());
    f.buffer_mut().set_style(r, th.base().patch(th.back(Hue::Raise)));
    if r.width < 4 || r.height < 2 {
        return Rect { height: 0, ..r };
    }
    f.render_widget(Paragraph::new(frame(th, vec![Vec::new(); usize::from(r.height) - 2], usize::from(r.width), title, th.fg(Hue::Edge), &BOX)), r);
    Rect { x: r.x + 2, y: r.y + 1, width: r.width - 4, height: r.height - 2 }
}

/// The ask's box `w` columns wide: one blank inside each side, and row `sel` has its band.
pub fn rows(th: &Theme, body: Vec<Vec<Span<'static>>>, w: usize, sel: Option<usize>) -> Vec<Line<'static>> {
    let band = |n: usize, spans: Vec<Span<'static>>| match sel {
        Some(s) if s == n => spans.into_iter().map(|p| Span::styled(p.content, p.style.patch(th.back(Hue::Sel)))).collect(),
        _ => spans,
    };
    let body = body.into_iter().enumerate().map(|(n, mut spans)| {
        spans.push(Span::raw(" ".repeat(w.saturating_sub(4).saturating_sub(spans.iter().map(|s| s.content.width()).sum::<usize>()))));
        [vec![Span::raw(" ")], band(n, spans), vec![Span::raw(" ")]].concat()
    });
    frame(th, body.collect(), w, "", th.fg(Hue::Edge), &BOX)
}
