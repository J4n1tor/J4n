//! `pick`: a choice drawn as the settings draw an enum. One part for every question with options: the radio marks of the `enum`
//! editor in a numbered column, the recommended row marked with the hint and filled, at most [`MOST`] rows and a line that says how many more, then
//! a last row `custom` whose first press is the input, and under it the ready answers (`decide yourself` first). The routing selection of a line
//! and the question of a model are drawn by it; neither writes a row of its own.

use crate::display::{lr, width_of};
use crate::parts::rows::lead;
use crate::theme::Theme;
use crate::theme::glyph::Slot;
use crate::theme::palette::Role as Hue;
use crate::wrap::fit;
use j4n_proto::Tone;
use ratatui::style::Modifier;
use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

/// The most options a choice shows; the rest are said as a count and are typed by their number or their words.
pub const MOST: usize = 5;

type Spans = Vec<Span<'static>>;

/// One option row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    /// At the right edge: who runs it, or why it is there.
    pub note: String,
    /// The one `⇥` takes: marked with the hint and filled.
    pub hint: bool,
    /// The hint was made by a model: drawn with `~`.
    pub model: bool,
    /// The key was pressed and the core has not taken the answer yet.
    pub sent: bool,
}

/// The last rows: the operator's own line, and the answers that are ready to send.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Custom {
    /// What is typed so far, shown on the row.
    pub typed: String,
    /// The row has the input (its key was pressed, or a letter was typed).
    pub on: bool,
    pub ready: Vec<String>,
}

/// A choice to draw: the rows are numbered from 1, then the custom row, then the ready answers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pick {
    pub items: Vec<Item>,
    /// Options past [`MOST`] that are not drawn.
    pub more: usize,
    pub custom: Option<Custom>,
}

fn number(th: &Theme, n: usize, hot: bool) -> Span<'static> {
    Span::styled(n.to_string(), if hot { th.fg(Hue::Accent).add_modifier(Modifier::BOLD) } else { th.tone(Tone::Dim) })
}

/// The rows of the choice `iw` columns wide, as they stand inside a card.
pub fn rows(th: &Theme, p: &Pick, phase: u8, iw: usize) -> Vec<Spans> {
    let hw = th.glyph(Slot::Hint).width();
    let pad = || Span::raw(" ".repeat(hw + 1));
    let marks = th.tier != crate::theme::glyph::Tier::Words;
    let mut out: Vec<Spans> = Vec::new();
    for (i, it) in p.items.iter().enumerate() {
        let mut left: Spans = Vec::new();
        if it.hint {
            left.push(Span::styled(th.glyph(Slot::Hint), th.fg(Hue::Accent)));
            left.push(if it.model { Span::styled(th.glyph(Slot::Made), th.fg(Hue::Made)) } else { Span::raw(" ") });
        } else {
            left.push(pad());
        }
        left.push(number(th, i + 1, it.hint));
        left.push(Span::raw("  "));
        if marks {
            left.push(crate::display::mark_sp(th, if it.hint { Slot::RadioOn } else { Slot::RadioOff }));
        }
        let mut right: Spans = Vec::new();
        if it.sent {
            right.push(Span::styled("sending", th.breath(phase)));
        } else if !it.note.is_empty() {
            if it.model {
                right.push(Span::styled(th.glyph(Slot::Made), th.fg(Hue::Made)));
                right.push(Span::raw(" "));
            }
            right.push(Span::styled(it.note.clone(), th.tone(Tone::Dim)));
        }
        let room = iw.saturating_sub(width_of(&left) + width_of(&right) + 2);
        left.push(Span::raw(fit(&it.label, room.max(1))));
        out.push(lr(left, right, iw));
    }
    if p.more > 0 {
        out.push(vec![pad(), Span::styled(format!("   +{} more · type the number or the words", p.more), th.tone(Tone::Dim))]);
    }
    if let Some(c) = &p.custom {
        let n = p.items.len() + 1;
        let mut left: Spans = vec![pad(), number(th, n, c.on), Span::raw("  ")];
        if marks {
            left.push(crate::display::mark_sp(th, if c.on { Slot::RadioOn } else { Slot::RadioOff }));
        }
        let room = iw.saturating_sub(width_of(&left));
        let name = Span::styled("custom", if c.on { th.fg(Hue::Accent).add_modifier(Modifier::BOLD) } else { th.tone(Tone::Plain) });
        left.push(name);
        let said = if c.typed.is_empty() { Span::styled(" · type your own", th.tone(Tone::Dim)) } else { Span::styled(format!(" · {}", fit(&c.typed, room.saturating_sub(9).max(1))), th.tone(Tone::Plain)) };
        left.push(said);
        out.push(left);
        for (i, r) in c.ready.iter().enumerate() {
            let mut row: Spans = vec![pad(), number(th, n + 1 + i, false), Span::raw("  "), lead(th, i + 1 == c.ready.len(), false, false)];
            row.push(Span::styled(fit(r, iw.saturating_sub(width_of(&row))), th.tone(Tone::Dim)));
            out.push(row);
        }
    }
    out
}
