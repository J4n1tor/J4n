//! `rows`: the pipe in front of a row of a stack, drawn by the base of the trees (`display/layout`) so that every look of `theme.frame` reaches it.

use crate::display::layout::{self, Laid, Look};
use crate::theme::Theme;
use crate::theme::palette::Role as Hue;
use ratatui::text::Span;

/// The pipe in front of a row of a stack in the edge colour, two cells (`last`: the elbow; else a tee); `on` (the keys are on the row) takes the accent for the mark, `bad` the
/// error colour. The elbow stays whatever is selected: the band behind the row and this colour are the selection.
pub fn lead(th: &Theme, last: bool, on: bool, bad: bool) -> Span<'static> {
    place(th, &layout::one(last), on, bad)
}

/// The same for a row that has its place from the base already.
pub fn place(th: &Theme, laid: &Laid, on: bool, bad: bool) -> Span<'static> {
    let role = if on { Hue::Accent } else if bad { Hue::Error } else { Hue::Edge };
    Span::styled(Look::new(th).stack(laid).0, th.fg(role))
}

/// What stands before the lines that go on under a row of a stack (its detail, its output): the bar while a row follows, a blank under the last; two cells.
pub fn trunk(th: &Theme, last: bool) -> Span<'static> {
    Span::styled(Look::new(th).stack(&layout::one(last)).1, th.fg(Hue::Edge))
}
