//! The theme laid out as pages to look at: the twenty-four roles with their colours, how each is made and the
//! contrast of the three that must be read, and every glyph slot of the tier. Used by the Look settings; the scenes show it as goldens.

use super::Theme;
use super::glyph::Slot;
use super::palette::Role;
use crate::wrap::pad;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

fn words(m: Modifier) -> String {
    let all = [(Modifier::BOLD, "bold"), (Modifier::UNDERLINED, "underline"), (Modifier::DIM, "dim"), (Modifier::REVERSED, "reverse")];
    let found: Vec<&str> = all.iter().filter(|(f, _)| m.contains(*f)).map(|(_, w)| *w).collect();
    if found.is_empty() { "plain".into() } else { found.join(" ") }
}

/// The floor a role has to meet, if it has one.
fn floor(t: &Theme, r: Role) -> Option<f64> {
    match r {
        Role::Dim => Some(t.floors.dim),
        Role::Faint => Some(t.floors.faint),
        Role::Soft => Some(t.floors.soft),
        Role::Accent | Role::Ok | Role::Warn | Role::Error | Role::Route | Role::Run | Role::Fetch => Some(t.floors.hue),
        _ => None,
    }
}

pub fn roles(t: &Theme) -> Vec<Line<'static>> {
    let mut out = vec![Line::styled(format!(" roles · {}{}", t.palette, if t.no_color { " · no colour" } else { "" }), t.fg(Role::Accent))];
    for r in Role::ALL {
        let c = t.roles.get(r);
        let sample = if r.is_back() { Span::styled("    ", t.back(r)) } else { Span::styled("Aa  ", t.fg(r)) };
        let mut spans = vec![Span::raw(" "), Span::raw(pad(r.name(), 7)), Span::styled(pad(&c.hex(), 9), t.fg(Role::Dim)), sample, Span::styled(pad(r.from(), 26), t.fg(Role::Dim))];
        if let Some(f) = floor(t, r) {
            let (got, _) = t.roles.worst(r);
            let (mark, role) = if got >= f { ("✓", Role::Ok) } else { ("✗", Role::Error) };
            spans.push(Span::raw(pad(&format!("{got:.1}:1 "), 7)));
            spans.push(Span::styled(mark, t.fg(role)));
        } else {
            spans.push(Span::raw(" ".repeat(8)));
        }
        if t.no_color {
            spans.push(Span::styled(format!("  {}", words(r.attributes())), t.fg(Role::Dim)));
        }
        out.push(Line::from(spans));
    }
    out
}

pub fn glyphs(t: &Theme, width: u16) -> Vec<Line<'static>> {
    const CELL: usize = 22;
    let mut out = vec![Line::styled(format!(" glyphs · {} · corners {}", t.tier.name(), t.corners.word()), t.fg(Role::Accent))];
    let per = (usize::from(width).saturating_sub(1) / CELL).max(1);
    for row in Slot::ALL.chunks(per) {
        let mut spans = vec![Span::raw(" ")];
        for slot in row {
            let (g, style) = t.mark(*slot);
            spans.push(Span::styled(pad(&format!("{g} "), 9), style));
            spans.push(Span::styled(pad(slot.name(), CELL - 9), Style::default().patch(t.fg(Role::Dim))));
        }
        out.push(Line::from(spans));
    }
    out
}
