//! The two fields that are pressed or read at a glance: `action` (a button bound to an intent, which asks for a second press when it
//! widens what may be done) and `bar` (one row of chips: those at the left, those at the right, and the ones that go first when the
//! screen is narrow).

use super::{cell, mark_sp, width_of};
use crate::app::App;
use crate::fields::Spec;
use crate::theme::glyph::Slot;
use crate::theme::palette::Role as Hue;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use j4n_proto::{Field, Intent, Json, Node, Tone, Up};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

/// From this risk (0–100) a button asks for a second press.
const RISKY: f64 = 50.0;

/// Whether a press of this button is only the first of two: `confirm` says `always`, or `auto` with a risk over the line.
fn needs_two(spec: &Spec) -> bool {
    match spec.text("confirm") {
        Some("always") => true,
        Some("never") => false,
        _ => spec.num("risk").is_some_and(|r| r >= RISKY),
    }
}

fn applied(v: &Json) -> bool {
    v.as_str() == Some("done") || v.get("state").and_then(Json::as_str) == Some("done")
}

/// The button in the state it is in: at rest, focused (the colours swapped), disabled, risky, armed (waiting for the second press), applied.
pub(crate) fn action(app: &App, n: &Node, field: &Field) -> Line<'static> {
    let th = &app.theme;
    let spec = Spec::new(&n.title, field);
    let focused = app.page().is_some_and(|p| p.id == n.id);
    let armed = app.armed.as_deref() == Some(field.bind.as_str());
    let value = app.value(&field.bind);
    let (mark, label, tone) = if spec.disabled() {
        (mark_sp(th, Slot::KeyEnter), n.title.clone(), Tone::Dim)
    } else if armed {
        (mark_sp(th, Slot::Risky), format!("again {}", th.glyph(Slot::KeyEnter)), Tone::Warn)
    } else if needs_two(&spec) {
        (mark_sp(th, Slot::Risky), n.title.clone(), Tone::Warn)
    } else {
        (mark_sp(th, Slot::KeyEnter), n.title.clone(), Tone::Accent)
    };
    let boxed = focused && !spec.disabled();
    let style = if spec.disabled() { th.fg(Hue::Faint) } else if boxed { th.inverse() } else { th.tone(tone) };
    let text = format!("{}{label}", mark.content);
    let mut spans = vec![Span::raw(if boxed { " " } else { "  " }), Span::styled(if boxed { format!(" {text} ") } else { text }, style)];
    if spec.disabled() {
        spans.push(Span::raw(" "));
        spans.push(mark_sp(th, Slot::ReadOnly));
    } else if applied(value) {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(format!("{}done", mark_sp(th, Slot::Done).content), th.tone(Tone::Good)));
        if !spec.get("undo").is_null() {
            spans.push(Span::styled("  u undo", th.tone(Tone::Dim)));
        }
    }
    Line::from(spans)
}

impl App {
    /// The intent a button sends: the `intent` of its props.
    fn pressed(&mut self, f: &Field, key: &str) -> Vec<Up> {
        self.armed = None;
        Intent::from_json(Spec::new("", f).get(key)).map(|intent| vec![Up::Intent { intent }]).unwrap_or_default()
    }
}

pub(crate) fn key(app: &mut App, f: &Field, k: KeyEvent) -> Option<Vec<Up>> {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let label = app.page().map(|p| p.title.clone()).unwrap_or_default();
    let spec = Spec::new(&label, f);
    if spec.disabled() || ctrl {
        return None;
    }
    let armed = app.armed.as_deref() == Some(f.bind.as_str());
    match k.code {
        KeyCode::Enter | KeyCode::Char(' ') => {
            if needs_two(&spec) && !armed {
                app.armed = Some(f.bind.clone());
                return Some(Vec::new());
            }
            Some(app.pressed(f, "intent"))
        }
        KeyCode::Char('u') if applied(app.value(&f.bind)) && !spec.get("undo").is_null() => Some(app.pressed(f, "undo")),
        KeyCode::Esc if armed => {
            app.armed = None;
            Some(Vec::new())
        }
        // any other key gives up the second press and goes on to do what it does
        _ => {
            app.armed = None;
            app.val_move(k)
        }
    }
}

// ---- bar ----

/// One row of chips: the first row of the binding. `right` of them (the last ones) are at the right edge; while they do not fit,
/// the cell named by the next entry of `drop` goes, or else the last one that is left.
pub fn bar(app: &mut App, f: &Field, area: Rect, fr: &mut Frame) {
    // the keys row is a bar of its own kind: keys and words, picked for the page that is open
    if f.bind == j4n_proto::KEYS_ROW {
        return crate::keysrow::draw(app, f, area, fr);
    }
    let th = &app.theme;
    let Some(row) = app.rows.get(&f.bind).and_then(|r| r.first()) else { return };
    let spec = Spec::new("", f);
    let mut keep: Vec<usize> = (0..row.cells.len()).filter(|i| !row.cells[*i].text.is_empty() || !row.cells[*i].icon.is_empty()).collect();
    let right = spec.num("right").map_or(0, |r| r as usize);
    let mut order: Vec<usize> = spec.get("drop").as_array().map(|a| a.iter().filter_map(|x| x.as_u64().map(|x| x as usize)).collect()).unwrap_or_default();
    order.extend((1..row.cells.len()).rev());
    let build = |keep: &[usize]| {
        let part = |ids: Vec<&usize>| {
            let mut out = Vec::new();
            for (n, i) in ids.into_iter().enumerate() {
                if n > 0 {
                    out.push(Span::raw("  "));
                }
                out.extend(cell(th, &row.cells[*i], row.tone));
            }
            out
        };
        let cut = row.cells.len().saturating_sub(right);
        (part(keep.iter().filter(|i| **i < cut).collect()), part(keep.iter().filter(|i| **i >= cut).collect()))
    };
    let (mut left, mut far) = build(&keep);
    let mut drops = order.into_iter();
    while width_of(&left) + width_of(&far) + 3 > usize::from(area.width) && keep.len() > 1 {
        let Some(gone) = drops.next() else { break };
        keep.retain(|i| *i != gone);
        (left, far) = build(&keep);
    }
    let gap = usize::from(area.width).saturating_sub(width_of(&left) + width_of(&far) + 2);
    let mut spans = vec![Span::raw(" ")];
    spans.extend(left);
    spans.push(Span::raw(" ".repeat(gap)));
    spans.extend(far);
    let style = if th.no_color { Style::default().add_modifier(Modifier::REVERSED) } else { th.back(Hue::Raise) };
    fr.render_widget(Paragraph::new(Line::from(spans)).style(style), area);
}
