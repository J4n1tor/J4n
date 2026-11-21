//! The fields that show: numbers, meters, series, chips, buttons, and the rows of a binding as a table, a
//! list, a tree, cards, one card or a log. A field is a function of what the core sent (a `Value` or the `Rows` of its binding), the
//! props it was given and the view state (`Win`: the row the keys are on, the sort, the find), so the board of every type is a table
//! of calls; the keys are a function of the state and the key, and the one thing a key can ask the core for is another window of
//! rows (`Up::Window`), because the core holds the million rows and the screen only the ones it shows.

mod cards;
mod dither;
pub mod event;
pub(crate) mod gauge;
mod held;
pub mod layout;
mod log;
pub mod plot;
mod press;
mod table;
mod tree;
pub(crate) mod win;

pub use held::held;
pub use press::bar;
pub use table::{is_header, list, shown, visible};
pub use win::Win;
pub(crate) use gauge::bar as fill_bar;
pub(crate) use gauge::{LINE_MAX, Spot, fill_role, gauge as context_gauge, percent, slice_for, thin, whole};

use crate::app::App;
use crate::theme::Theme;
use crate::theme::glyph::Slot;
use crate::theme::palette::Role as Hue;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use j4n_proto::{Cell, Field, FieldKind, Json, Node, Tone, Up};
use crossterm::event::KeyEvent;
use unicode_width::UnicodeWidthStr;

/// The kinds drawn here, apart from the bar, the list and the log, which have a place of their own in the page.
pub fn shows(k: FieldKind) -> bool {
    matches!(k, FieldKind::Stat | FieldKind::Meter | FieldKind::Spark | FieldKind::Line | FieldKind::Bars | FieldKind::Chip | FieldKind::Action | FieldKind::Table | FieldKind::Tree | FieldKind::Card | FieldKind::Cards)
}

/// The kinds whose rows are a window of a binding of any size.
pub fn windowed(k: FieldKind) -> bool {
    matches!(k, FieldKind::Table | FieldKind::Log)
}

/// The rows a field that knows its height takes at this width, or `None` for one that takes what is left.
pub fn height(app: &App, n: &Node, width: u16) -> Option<u16> {
    let f = n.field.as_ref()?;
    let w = usize::from(width);
    let v = app.value(&f.bind);
    let rows = match f.kind {
        FieldKind::Stat => gauge::stat(&app.theme, v, w).len(),
        // a list of entries (the usage view) is drawn as the chat draws it: a bar a window
        FieldKind::Meter if v.is_array() => gauge::meters(&app.theme, v, w).len(),
        FieldKind::Meter => gauge::meter(&app.theme, &n.title, v, f, w).len(),
        FieldKind::Spark => gauge::spark(&app.theme, &n.title, v, f, w).len(),
        // a chart whose value has not come takes the one row of its sign
        FieldKind::Line | FieldKind::Bars if app.value_wait(&f.bind).is_some() => 1,
        FieldKind::Line | FieldKind::Bars => plot::rows(f, v),
        FieldKind::Chip => 1,
        FieldKind::Action => 1,
        FieldKind::Card => cards::card_lines(app, n, f).len(),
        _ => return None,
    };
    Some(rows as u16)
}

pub fn draw(app: &mut App, n: &Node, area: Rect, f: &mut Frame) {
    let Some(field) = n.field.as_ref() else { return };
    let w = usize::from(area.width);
    let lines = match field.kind {
        FieldKind::Stat => gauge::stat(&app.theme, app.value(&field.bind), w),
        FieldKind::Meter if app.value(&field.bind).is_array() => gauge::meters(&app.theme, app.value(&field.bind), w),
        FieldKind::Meter => gauge::meter(&app.theme, &n.title, app.value(&field.bind), field, w),
        FieldKind::Spark => gauge::spark(&app.theme, &n.title, app.value(&field.bind), field, w),
        // a chart whose value has not come: its label, and the sign once the wait is long enough to see (a chart with no data says `—`)
        FieldKind::Line | FieldKind::Bars if let Some(wait) = app.value_wait(&field.bind) => {
            vec![app.wait_line(wait, &n.title, w).unwrap_or_else(|| Line::from(vec![Span::raw("  "), Span::styled(n.title.clone(), app.theme.fg(Hue::Dim))]))]
        }
        FieldKind::Line | FieldKind::Bars => plot::draw(&app.theme, &n.title, app.value(&field.bind), field, w, usize::from(area.height)),
        FieldKind::Chip => gauge::chip(&app.theme, app.value(&field.bind)),
        FieldKind::Action => vec![press::action(app, n, field)],
        FieldKind::Card => cards::card_lines(app, n, field),
        FieldKind::Table | FieldKind::Tree | FieldKind::Cards if crate::view::nothing(app, field, area, f) => return,
        FieldKind::Table => return table::table(app, field, area, f),
        FieldKind::Tree => return tree::tree(app, field, area, f),
        FieldKind::Cards => return cards::cards(app, field, area, f),
        _ => return,
    };
    f.render_widget(Paragraph::new(lines), area);
}

/// Whether the keys of this kind are handled here.
pub fn keyed(k: FieldKind) -> bool {
    matches!(k, FieldKind::Table | FieldKind::Tree | FieldKind::Card | FieldKind::Cards | FieldKind::Log | FieldKind::Action)
}

/// A key for the field that has the keys: `Some` when it was taken here.
pub fn key(app: &mut App, f: &Field, k: KeyEvent) -> Option<Vec<Up>> {
    match f.kind {
        FieldKind::Table => table::key(app, f, k),
        FieldKind::Tree => tree::key(app, f, k),
        FieldKind::Cards | FieldKind::Card => cards::key(app, f, k),
        // a log of lines (a stream) keeps its old keys; one of rows has these
        FieldKind::Log if app.rows.contains_key(&f.bind) => log::key(app, f, k),
        FieldKind::Action => press::key(app, f, k),
        _ => None,
    }
}

pub use log::log;

// ---- what the kinds share ----

/// A tone written as a word in the value of a field.
pub(crate) fn tone_of(word: &str) -> Option<Tone> {
    Some(match word {
        "plain" => Tone::Plain,
        "dim" => Tone::Dim,
        "good" | "ok" => Tone::Good,
        "warn" => Tone::Warn,
        "bad" | "error" => Tone::Bad,
        "accent" => Tone::Accent,
        _ => return None,
    })
}

/// A count with a comma between the thousands: `1,204`.
pub fn group(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The glyph of a slot and the blank after it, whether the tier's glyph is an icon (which brings its own) or a letter or a word.
pub(crate) fn mark_sp(th: &Theme, slot: Slot) -> Span<'static> {
    let (mut g, style) = th.mark(slot);
    if !g.ends_with(' ') {
        g.push(' ');
    }
    Span::styled(g, style)
}

/// The slot called `name`, as `mark_sp` draws it.
pub(crate) fn named(th: &Theme, name: &str) -> Option<Span<'static>> {
    Slot::by_name(name).map(|s| mark_sp(th, s))
}

/// A cell as spans: its icon, then its text in its tone (`base` when the cell has none).
pub(crate) fn cell(th: &Theme, c: &Cell, base: Tone) -> Vec<Span<'static>> {
    let tone = if c.tone == Tone::Plain { base } else { c.tone };
    let mut out = Vec::new();
    if let Some(i) = named(th, &c.icon) {
        out.push(i);
    }
    out.push(Span::styled(c.text.clone(), th.tone(tone)));
    out
}

/// `left` and `right` on one row `iw` wide: the right at the end, two blanks from the left at the least (the right is left out if it
/// does not fit).
pub(crate) fn lr(left: Vec<Span<'static>>, mut right: Vec<Span<'static>>, iw: usize) -> Vec<Span<'static>> {
    // the blank of a mark that ends the row is not a cell of the row
    if let Some(last) = right.last_mut() {
        last.content = last.content.trim_end().to_string().into();
    }
    let (lw, rw) = (width_of(&left), width_of(&right));
    if right.is_empty() || lw + rw + 2 > iw {
        return left;
    }
    let mut out = left;
    out.push(Span::raw(" ".repeat(iw - lw - rw)));
    out.extend(right);
    out
}

pub(crate) fn width_of(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.content.width()).sum()
}

/// The spans with blanks after them up to `width`.
pub(crate) fn padded(mut spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    let w = width_of(&spans);
    if w < width {
        spans.push(Span::raw(" ".repeat(width - w)));
    }
    spans
}

/// `text` as spans cut to `width` columns, and padded to it.
pub(crate) fn column(text: &str, width: usize, right: bool, style: Style) -> Span<'static> {
    let t = crate::wrap::fit(text, width);
    let gap = " ".repeat(width.saturating_sub(t.width()));
    Span::styled(if right { format!("{gap}{t}") } else { format!("{t}{gap}") }, style)
}

/// The rows of a scroll thumb: which of `height` rows it covers, for a window of `shown` rows from `top` of `total`.
pub(crate) fn thumb(height: usize, top: u64, shown: usize, total: u64) -> Vec<bool> {
    if total <= shown as u64 || height == 0 {
        return vec![false; height];
    }
    let size = ((height as u64 * shown as u64) / total).clamp(1, height as u64) as usize;
    let room = total - shown as u64;
    // at the end of the rows it is at the end of the track
    let at = if top + shown as u64 >= total { height - size } else { ((top.min(room) * (height - size) as u64) / room) as usize };
    (0..height).map(|i| i >= at && i < at + size).collect()
}

/// `key` goes into the set, or out of it when it is in.
pub(crate) fn toggle(set: &mut std::collections::BTreeSet<String>, key: impl Into<String>) {
    let key = key.into();
    if !set.remove(&key) {
        set.insert(key);
    }
}

/// The parts one after another, `sep` between them.
pub(crate) fn join(parts: Vec<Vec<Span<'static>>>, sep: &str) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    for (n, p) in parts.into_iter().enumerate() {
        if n > 0 {
            out.push(Span::raw(sep.to_string()));
        }
        out.extend(p);
    }
    out
}

/// A line the width of the screen, behind the selection if `sel`.
pub(crate) fn row(th: &Theme, spans: Vec<Span<'static>>, width: usize, sel: bool) -> Line<'static> {
    let l = Line::from(padded(spans, width));
    if sel { l.style(th.back(Hue::Sel)) } else { l }
}

/// The value as a number: a number, or the `v` of an object.
pub(crate) fn number(v: &Json) -> Option<f64> {
    v.as_f64().or_else(|| v.get("v").and_then(Json::as_f64))
}
