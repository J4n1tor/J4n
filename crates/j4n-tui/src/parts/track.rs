//! The track: J4n's own sign that a line is being taken. The time a line
//! takes is not known, so the sign does not say how far it has come: a bright segment of two cells runs right and back along a dim track of six
//! (`━━────`, `─━━───`, … `────━━`, `───━━─`, …), a frame every 120 ms, and it never ends. No frame is a complete one and the sign does not count; the
//! stage (`sending`, `routing`, `opening`, `starting`) is a word beside it, which is what says where the line is. The heavy `━` over the light `─` is
//! the thin line of the context gauge (`gauge::thin`), so the segment reads by shape under `NO_COLOR` and in `mono`, and `=` over `-` in ascii. A row
//! that is narrow has four cells. With `theme.motion` off the sign stands still: the dim track, one fixed frame, and the word.
//! The frame is `App.sign`, which the screen's own clock steps every [`BEAT_MS`] while a sign is on the screen and for nothing else.
//! [`waiting`] is the same sign for a part of a page whose rows or value the core has not sent yet: a dim `loading` beside it, and once the
//! wait has gone on the words say the core is busy, so a part that waits is never taken for one that is empty.

use crate::display::gauge::thin_cells;
use crate::theme::Theme;
use crate::theme::palette::Role as Hue;
use j4n_proto::Tone;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

/// The cells of the sign when there is room (`track` takes fewer when the row is narrow).
pub const CELLS: usize = 6;

/// The cells of the sign on a narrow row.
pub const NARROW: usize = 4;

/// The cells of the bright segment.
pub const SEGMENT: usize = 2;

/// How long one frame lasts, in milliseconds.
pub const BEAT_MS: u128 = 120;

/// The frames of the screen's counter (`App.sign`): the longest cycle there is, which every shorter one divides.
pub const FRAMES: u8 = 8;

/// The frames of one run right and back on a track of `cells`: two less than the cells, there and back (eight on six cells, four on four).
pub fn cycle(cells: usize) -> usize {
    2 * cells.clamp(SEGMENT + 1, CELLS).saturating_sub(SEGMENT)
}

/// Where the segment starts on frame `frame` of a track of `cells`: `0, 1, 2, 3, 4, 3, 2, 1` on six cells.
fn start(frame: usize, cells: usize) -> usize {
    let (run, n) = (cells - SEGMENT, cycle(cells));
    let f = frame % n;
    if f <= run { f } else { n - f }
}

/// The sign on frame `frame` (the counter `App.sign`, `0..FRAMES`), `cells` wide (`CELLS`, or `NARROW`): the segment of the heavy line in the accent
/// over the dim light one. With `motion` off it is the dim track alone, on every frame.
pub fn track(th: &Theme, frame: u8, cells: usize) -> Vec<Span<'static>> {
    let cells = cells.clamp(SEGMENT + 1, CELLS);
    if !th.motion {
        return thin_cells(th, 0, 0, cells, th.fg(Hue::Faint));
    }
    let at = start(usize::from(frame), cells);
    thin_cells(th, at, SEGMENT, cells, th.fg(Hue::Accent))
}

/// The word of a part that waits for the core.
pub const LOADING: &str = "loading";

/// What is said after [`LOADING`] once the wait has gone on.
pub const BUSY: &str = " · the core is busy";

/// The line of a part that waits, `width` cells at most: its `label` first when it has one (a chart's), then the sign and `loading`, and with
/// `busy` why it takes so long. As the row narrows the why goes first, then the sign takes four cells, then none.
pub fn waiting(th: &Theme, frame: u8, busy: bool, label: &str, width: usize) -> Line<'static> {
    let dim = th.tone(Tone::Dim);
    let mut spans: Vec<Span<'static>> = if label.is_empty() { Vec::new() } else { vec![Span::raw("  "), Span::styled(label.to_string(), th.fg(Hue::Dim)), Span::raw("  ")] };
    let used = spans.iter().map(|s| s.content.width()).sum::<usize>() + LOADING.width();
    let why = if busy && used + CELLS + 1 + BUSY.width() <= width { BUSY } else { "" };
    if let Some(cells) = [CELLS, NARROW].into_iter().find(|c| used + c + 1 + why.width() <= width) {
        spans.extend(track(th, frame, cells));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(LOADING, dim));
    if !why.is_empty() {
        spans.push(Span::styled(why, dim));
    }
    Line::from(spans)
}
