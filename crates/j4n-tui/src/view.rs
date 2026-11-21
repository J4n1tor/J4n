//! Drawing: the tree the core sent, field by field, in the theme's look. Every field is one
//! function of the state; nothing here sends or reads anything.

use crate::app::{App, Preview};
use crate::shape::{Card, ROWS};
use crate::nodes;
use crate::wrap::pad;
use crate::theme::glyph::Slot;
use crate::theme::sheet;
use crate::wrap::fit;
use j4n_proto::{Field, FieldKind, Row, SHAPE_CARD, Tone};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub fn draw(app: &mut App, f: &mut Frame) {
    let area = f.area();
    app.size = (area.width, area.height);
    app.opens.clear();
    app.bigs.clear();
    // the parts drawn below that wait note when they change again
    app.wait_due.set(None);
    f.buffer_mut().set_style(area, app.theme.base());
    if let Some(page) = app.preview {
        let lines = match page {
            Preview::Roles => sheet::roles(&app.theme),
            Preview::Glyphs => sheet::glyphs(&app.theme, area.width),
        };
        f.render_widget(Paragraph::new(lines), area);
        return;
    }
    if let Some(why) = app.fatal.clone() {
        f.render_widget(Paragraph::new(why).style(app.theme.tone(Tone::Bad)), area);
        return;
    }
    let Some(root) = app.tree.clone() else {
        f.render_widget(Paragraph::new("waiting for the core…").style(app.theme.tone(Tone::Dim)), area);
        return;
    };
    // a chat that is drawn sets this again; the clock redraws the screen for a breath only while one is in sight
    app.breathes = false;
    app.signs = false;
    app.age_reset();
    nodes::draw(app, &root, 0, area, f);
    crate::ask::card(app, area, f);
    crate::parts::toast::toast(app, area, f);
    crate::restart::draw(app, area, f);
    nodes::draw_jump(app, area, f);
    crate::keysrow::draw_sheet(app, area, f);
}

pub(crate) fn draw_field(app: &mut App, field: &Field, area: Rect, f: &mut Frame) {
    match field.kind {
        FieldKind::Chat => crate::chat::draw(app, field, area, f),
        // a window of the rows of a binding of any size
        FieldKind::Log => crate::display::log(app, field, area, f),
        FieldKind::Bar => crate::display::bar(app, field, area, f),
        FieldKind::Ask => crate::ask::draw(app, area, f),
        // a form of rows is a list like the others
        FieldKind::List | FieldKind::Form if nothing(app, field, area, f) => {}
        FieldKind::List | FieldKind::Form => crate::display::list(app, field, area, f),
        // a value field is drawn with its node, which holds its label
        _ => {}
    }
}

/// A binding that came with no rows says so, in the words of its `empty` prop or `nothing here`; one whose rows were asked for and have not
/// come shows the loading sign once the wait is long enough to see (`wait`). True when either was the case, and the rows are not drawn.
pub(crate) fn nothing(app: &mut App, field: &Field, area: Rect, f: &mut Frame) -> bool {
    if let Some(w) = app.rows_wait(&field.bind) {
        if let Some(line) = app.wait_line(w, "", usize::from(area.width)) {
            f.render_widget(Paragraph::new(line), area);
        }
        return true;
    }
    let said = app.rows.get(&field.bind).is_some_and(Vec::is_empty);
    if said {
        f.render_widget(Paragraph::new(empty(app, field)).style(app.theme.tone(Tone::Dim)), area);
    }
    said
}

/// The words of a binding that came with no rows: its `empty` prop, or `nothing here`.
pub(crate) fn empty(app: &App, field: &Field) -> String {
    format!("{} {}", app.theme.glyph(Slot::Bullet), field.props.get("empty").and_then(|e| e.as_str()).unwrap_or("nothing here"))
}

/// The lines of the shaper's card for the draft in the composer, if there is one to show.
pub(crate) fn card(app: &App) -> Vec<Line<'static>> {
    match app.card() {
        Card::Quiet | Card::Skip | Card::Pass => Vec::new(),
        Card::Working => vec![Line::styled(" ~ shaping  draft ▸ check ▸ judge", app.theme.tone(Tone::Accent))],
        Card::Failed(why) => vec![Line::styled(format!(" ~ shape did not run: {why}"), app.theme.tone(Tone::Warn))],
        Card::Dropped => vec![Line::styled(" ~ notes dropped · ⏎ sends the line as typed", app.theme.tone(Tone::Dim))],
        Card::Ready => {
            let rows = app.rows.get(SHAPE_CARD).map_or(&[][..], Vec::as_slice);
            let sel = app.place(SHAPE_CARD);
            let items: Vec<&Row> = rows.iter().skip(1).filter(|r| r.key != "~judge").collect();
            let mut lines = Vec::new();
            let mut at = 0;
            for r in items.iter().take(ROWS) {
                let editable = !r.key.starts_with("x:");
                let mark = if app.composer.focus && editable && at == sel { "▸" } else { " " };
                if editable {
                    at += 1;
                }
                let kind = r.cells.first().map_or("", |c| c.text.as_str());
                let (text, proof) = (r.cells.get(1).map_or("", |c| c.text.as_str()), r.cells.get(2).map_or("", |c| c.text.as_str()));
                let kept = r.tone == Tone::Good;
                let glyph = if !editable { "✗" } else if kept { "✓" } else { "·" };
                let room = (app.size.0 as usize).saturating_sub(24);
                let body = if r.detail.is_empty() { text.to_string() } else { format!("{text}  {}", r.detail.iter().map(|d| d.text.as_str()).collect::<Vec<_>>().join("  ")) };
                lines.push(Line::from(vec![
                    Span::styled(format!("{mark}~{glyph} "), app.theme.tone(r.tone)),
                    Span::styled(pad(kind, 8), app.theme.tone(Tone::Dim)),
                    Span::styled(fit(&body, room), app.theme.tone(if kept { Tone::Plain } else { r.tone })),
                    Span::styled(format!("  {proof}"), app.theme.tone(Tone::Dim)),
                ]));
            }
            let judge = rows.iter().find(|r| r.key == "~judge").and_then(|r| r.cells.get(1)).map(|c| format!("judge {}  ", c.text)).unwrap_or_default();
            let keys = if app.composer.focus { "↑↓ move  space keep/drop  1-4 pick  ⏎ back" } else { "⏎ send with notes   esc drop notes   ⌃o rows" };
            lines.push(Line::styled(format!(" {judge}{keys}"), app.theme.tone(Tone::Dim)));
            lines
        }
    }
}
