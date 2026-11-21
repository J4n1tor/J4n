//! `cards` (a grid of cards, one for each row, the one the keys are on in a heavy frame) and `card` (one thing: its title and state,
//! what is known of it on pipes, and what can be done with it).

use super::table::act;
use super::{cell, width_of};
use crate::app::App;
use crate::parts::{popup::{BOX, HEAVY, frame as framed}, rows::lead, scroll::keep, select::Sel};
use crate::theme::glyph::Slot;
use crate::theme::palette::Role as Hue;
use crate::wrap::fit;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use j4n_proto::{Field, Node, Row, Tone, Up};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

/// The narrowest and the widest a card grows by itself, frame included.
const CARD_MIN: usize = 14;
const CARD_MAX: usize = 28;
/// The most lines of detail under a title.
const LINES_MAX: usize = 3;

fn title_w(r: &Row) -> usize {
    r.cells.first().map_or(0, |c| c.text.width() + if c.icon.is_empty() { 0 } else { 2 })
}

/// How wide the cards are and how many are in a row, for a width.
fn grid(f: &Field, rows: &[Row], width: usize) -> (usize, usize) {
    let natural = (rows.iter().map(title_w).max().unwrap_or(0) + 4).clamp(CARD_MIN, CARD_MAX);
    let cw = f.props.get("width").and_then(j4n_proto::Json::as_u64).map_or(natural, |w| w as usize).max(6);
    let cols = f.props.get("cols").and_then(j4n_proto::Json::as_u64).map_or((width + 1) / (cw + 1), |c| c as usize);
    (cw, cols.max(1))
}

/// The lines of one card, `cw` wide: the frame, the title, the other cells of the row each on a line.
fn frame(app: &App, r: &Row, cw: usize, h: usize, on: bool) -> Vec<Line<'static>> {
    let th = &app.theme;
    let inner = cw.saturating_sub(2);
    let title = r.cells.first().map(|c| {
        let mut parts = cell(th, c, r.tone);
        if width_of(&parts) + 2 > inner {
            let text = fit(&parts.iter().map(|p| p.content.to_string()).collect::<String>(), inner.saturating_sub(2));
            parts = vec![Span::styled(text, th.tone(if c.tone == Tone::Plain { r.tone } else { c.tone }))];
        }
        parts
    });
    let mut body = vec![[vec![Span::raw(" ")], title.unwrap_or_default()].concat()];
    let extras: Vec<&j4n_proto::Cell> = r.cells.iter().skip(1).collect();
    for j in 0..h.saturating_sub(3) {
        body.push(extras.get(j).map(|c| [vec![Span::raw(" ")], { let mut s = cell(th, c, Tone::Dim); if let Some(last) = s.last_mut() { last.content = fit(&last.content, inner.saturating_sub(2)).into(); } s }].concat()).unwrap_or_default());
    }
    framed(th, body, cw, "", th.fg(if on { Hue::Accent } else { Hue::Edge }), if on { &HEAVY } else { &BOX })
}

pub(crate) fn cards(app: &mut App, f: &Field, area: Rect, fr: &mut Frame) {
    let (rows, _, _) = app.held(&f.bind);
    let (cw, cols) = grid(f, rows, usize::from(area.width));
    let lines = rows.iter().map(|r| r.cells.len().saturating_sub(1)).max().unwrap_or(0).clamp(1, LINES_MAX);
    let h = 3 + lines;
    let sel = (app.selected(f) as usize).min(rows.len().saturating_sub(1));
    let seen_rows = (usize::from(area.height) / h).max(1);
    let top = {
        let (w, line) = (app.win(&f.bind), (sel / cols) as u64);
        w.cols = cols;
        w.cur.top = keep(w.cur.top, line, line, seen_rows as u64);
        w.cur.top as usize
    };
    let (rows, _, _) = app.held(&f.bind);
    let mut out: Vec<Line<'static>> = Vec::new();
    for g in top..(top + seen_rows).min(rows.len().div_ceil(cols)) {
        let cells: Vec<Vec<Line<'static>>> = (g * cols..((g + 1) * cols).min(rows.len())).map(|i| frame(app, &rows[i], cw, h, i == sel)).collect();
        for j in 0..h {
            let mut spans = Vec::new();
            for (k, c) in cells.iter().enumerate() {
                if k > 0 {
                    spans.push(Span::raw(" "));
                }
                spans.extend(c[j].spans.clone());
            }
            out.push(Line::from(spans));
        }
    }
    fr.render_widget(Paragraph::new(out), area);
}

// ---- card ----

/// The card as lines: the title with the state beside it, the rest of what is known on pipes, and the actions.
pub(crate) fn card_lines(app: &App, n: &Node, f: &Field) -> Vec<Line<'static>> {
    let th = &app.theme;
    let rows = app.rows.get(&f.bind).map_or(&[][..], Vec::as_slice);
    let Some(head) = rows.first() else { return vec![Line::styled(format!("  {}", n.title), th.tone(Tone::Dim))] };
    let mut title = vec![Span::raw("  ")];
    for (i, c) in head.cells.iter().enumerate() {
        if i > 0 {
            title.push(Span::raw("  "));
        }
        title.extend(cell(th, c, if i == 0 { head.tone } else { Tone::Dim }));
    }
    let mut out = vec![Line::from(title)];
    let key_w = rows.iter().skip(1).filter_map(|r| r.cells.first()).map(|c| c.text.width()).max().unwrap_or(0).min(14);
    let n_kv = rows.len() - 1;
    for (i, r) in rows.iter().skip(1).enumerate() {
        let mut spans = vec![lead(th, i + 1 == n_kv, false, false)];
        let name = r.cells.first().map(|c| c.text.as_str()).unwrap_or("");
        spans.push(Span::styled(format!("{name}{}  ", " ".repeat(key_w.saturating_sub(name.width()))), th.tone(Tone::Dim)));
        for (j, c) in r.cells.iter().skip(1).enumerate() {
            if j > 0 {
                spans.push(Span::raw("  "));
            }
            spans.extend(cell(th, c, r.tone));
        }
        out.push(Line::from(spans));
    }
    if !head.actions.is_empty() {
        let focused = app.page().is_some_and(|p| p.id == n.id);
        let mut spans = vec![Span::raw(if focused { " " } else { "  " })];
        for (i, a) in head.actions.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw("  "));
            }
            let key = if i == 0 { th.glyph(Slot::KeyEnter).to_string() } else { a.key.clone() };
            let text = format!("{key} {}", a.label);
            spans.push(if i == 0 && focused { Span::styled(format!(" {text} "), th.inverse()) } else { Span::styled(text, th.fg(if i == 0 { Hue::Accent } else { Hue::Dim })) });
        }
        out.push(Line::from(spans));
    }
    out
}

pub(crate) fn key(app: &mut App, f: &Field, k: KeyEvent) -> Option<Vec<Up>> {
    if k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
        return None;
    }
    let total = app.held(&f.bind).2;
    let sel = app.selected(f);
    let cols = app.wins.get(&f.bind).map_or(1, |w| w.cols.max(1)) as i64;
    let by = match k.code {
        KeyCode::Left => Some(-1),
        KeyCode::Right => Some(1),
        KeyCode::Up => Some(-cols),
        KeyCode::Down => Some(cols),
        KeyCode::Home => Some(i64::MIN),
        KeyCode::End => Some(i64::MAX),
        _ => None,
    };
    if let Some(by) = by {
        // one card has nowhere to move to: the arrows are the stack's
        if f.kind != j4n_proto::FieldKind::Cards {
            return None;
        }
        let mut s = Sel { at: sel, top: 0 };
        s.by(by, total);
        app.move_to(f, s.at);
        return Some(Vec::new());
    }
    match k.code {
        KeyCode::Enter => act(app, f, None).or(Some(Vec::new())),
        KeyCode::Char(c) => act(app, f, Some(c)),
        _ => None,
    }
}
