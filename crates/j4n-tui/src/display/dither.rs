//! Charts drawn as dithered half-block pixels: `dither-core`'s engine (the pixel grid, the ordered fills, the line) and J4n's own
//! paint of it, which follows `dither-ratatui`'s `raster::paint` (MIT OR Apache-2.0): two pixels a cell, `▀` and `▄`, the
//! pixels of the series in the exact colour of its theme role (no named dither tone reaches the screen), a body under each line and in each bar
//! dithered by the ordered matrix of `dither-core`. The axes, the labels, the log scale, the window and the right axis stay J4n's: the values
//! are placed by `plot::Axis` (linear or log10, labels on powers of ten) and the labels are drawn as text beside the pixels.
//!
//! This drawer is used only where it can say what `plot.rs` says: truecolor, colour on, a tier above ascii, a palette whose colours tell the
//! series apart (not `mono`), the setting `look.chart` not `plain`, and the style braille (the default; `stream` and `wave` are asked for by name).
//! Anywhere else `plot::draw` falls back to its own drawer, which this file never touches. A pure function of the chart, the props and the theme.

use super::plot::{Axis, Chart, axes, dim, show, xrow};
use crate::theme::Theme;
use crate::theme::glyph::{Slot, Tier};
use crate::theme::palette::Role as Hue;
use dither_core::chart::line as dashes;
use dither_core::{Fill, PixelGrid, Rgba};
use j4n_proto::{Field, FieldKind, Json};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

/// The translucency of what is under a line or inside a bar, out of 255.
const BODY: u8 = 120;
/// Plots at least this many rows high have a dashed guide at each label (as `plot.rs` marks them).
const GUIDED: usize = 8;

/// Whether the theme and the props let this drawer draw.
pub(super) fn fits(th: &Theme, f: &Field) -> bool {
    th.dither && th.truecolor && !th.no_color && th.tier != Tier::Ascii && th.palette != "mono" && matches!(f.props.get("style").and_then(Json::as_str), None | Some("braille"))
}

/// The chart's plot with its axes, or `None` where `plot.rs` is to draw it.
pub(super) fn draw(th: &Theme, c: &Chart, f: &Field, width: usize, height: usize) -> Option<Vec<Line<'static>>> {
    if !fits(th, f) {
        return None;
    }
    if f.kind == FieldKind::Bars { bars(th, c, f, width, height) } else { lines(th, c, f, width, height) }
}

/// A role as the colour of a pixel.
fn rgba(th: &Theme, r: Hue) -> Rgba {
    let c = th.roles.get(r);
    Rgba::rgb(c.0, c.1, c.2)
}

/// The pixel row of a place `t` (0 at the bottom, 1 at the top) in a plot `h` pixels high.
fn py(t: f64, h: usize) -> f64 {
    (1.0 - t) * (h - 1) as f64
}

/// A label of an axis: the cell row it stands in, the pixel row of its value, and its words.
type Tick = (usize, usize, String);

/// The labels of an axis in a plot `ph` rows high: the cell the value is drawn in, four rows apart at least.
fn ticks(a: &Axis, ph: usize) -> Vec<Tick> {
    let mut out: Vec<Tick> = Vec::new();
    for v in a.wants() {
        let y = py(a.at(v).unwrap_or(0.0), ph * 2).round() as usize;
        if out.iter().all(|t| t.0.abs_diff(y / 2) >= 4) {
            out.push((y / 2, y, show(v, &a.unit)));
        }
    }
    out
}

/// The sizes of the plot: its width and rows, the widths of the two axes' labels, and whether a row of x labels stands under it.
struct Room {
    pw: usize,
    ph: usize,
    wl: usize,
    wr: usize,
    xrow: bool,
    tick: [Vec<Tick>; 2],
}

fn room(ax: &[Option<Axis>; 2], width: usize, height: usize) -> Option<Room> {
    let xrow = height >= 4;
    let ph = height.saturating_sub(1 + usize::from(xrow));
    let tick = [0, 1].map(|i| ax[i].as_ref().map(|a| ticks(a, ph.max(2))).unwrap_or_default());
    let (wl, wr) = (tick[0].iter().map(|t| t.2.width()).max().unwrap_or(0), tick[1].iter().map(|t| t.2.width()).max().unwrap_or(0));
    let pw = width.checked_sub(2 + wl + 1 + if tick[1].is_empty() { 0 } else { 1 + wr })?;
    (pw >= 6 && ph >= 2).then_some(Room { pw, ph, wl, wr, xrow, tick })
}

/// A pixel as the colour of a cell's half: its translucency blended over what is under it (the ground of the palette).
fn blend(c: Rgba, under: Rgba) -> Color {
    let c = c.over(under);
    Color::Rgb(c.r, c.g, c.b)
}

/// The cells of a grid as spans, a row each: two pixels a cell, `▀` with the top pixel as the foreground (and the bottom one as the background
/// when it is lit too), `▄` for a bottom pixel alone, a blank where neither is lit.
fn cells(th: &Theme, g: &PixelGrid, w: usize, rows: usize) -> Vec<Vec<Span<'static>>> {
    let under = rgba(th, Hue::Bg);
    (0..rows)
        .map(|y| {
            (0..w)
                .map(|x| match (g.get(x, y * 2), g.get(x, y * 2 + 1)) {
                    (Some(t), Some(b)) => Span::styled("▀", Style::default().fg(blend(t, under)).bg(blend(b, under))),
                    (Some(t), None) => Span::styled("▀", Style::default().fg(blend(t, under))),
                    (None, Some(b)) => Span::styled("▄", Style::default().fg(blend(b, under))),
                    (None, None) => Span::raw(" "),
                })
                .collect()
        })
        .collect()
}

/// The plot between its axes, as `plot.rs` frames it: the labels of the left axis, the cells, and the right axis if it has one, with the x row under.
fn framed(th: &Theme, r: &Room, g: &PixelGrid, x: Option<Line<'static>>) -> Vec<Line<'static>> {
    let edge = th.fg(Hue::Edge);
    let mut out: Vec<Line<'static>> = cells(th, g, r.pw, r.ph)
        .into_iter()
        .enumerate()
        .map(|(y, row)| {
            let label = |side: usize| r.tick[side].iter().find(|t| t.0 == y).map(|t| t.2.clone()).unwrap_or_default();
            let mut spans = vec![Span::raw("  "), dim(th, &label(0), r.wl, true), Span::styled(th.glyph(Slot::BoxV), edge)];
            spans.extend(row);
            if !r.tick[1].is_empty() {
                spans.push(Span::styled(th.glyph(Slot::BoxV), edge));
                spans.push(dim(th, &label(1), r.wr, false));
            }
            Line::from(spans)
        })
        .collect();
    out.extend(x);
    out
}

/// A line chart: each series a line with a dithered body under it. A point that cannot be drawn (no value, or ≤ 0 on a log axis) breaks the line.
fn lines(th: &Theme, c: &Chart, f: &Field, width: usize, height: usize) -> Option<Vec<Line<'static>>> {
    let ax = axes(c, f, false);
    let r = room(&ax, width, height)?;
    let (w, h, n) = (r.pw, r.ph * 2, c.x.len());
    let mut g = PixelGrid::new(w, h);
    if r.ph >= GUIDED {
        let guide = rgba(th, Hue::Edge);
        for t in r.tick.iter().flatten() {
            dashes(&mut g, (0, t.1 as i32), (w as i32 - 1, t.1 as i32), guide, 3);
        }
    }
    let at = |i: usize| if n == 1 { w / 2 } else { (i * (w - 1) + (n - 1) / 2) / (n - 1) };
    // the height of each series in each column of pixels: the points, and the straight line between two that stand next to each other
    let curves: Vec<(Rgba, Vec<Option<f64>>)> = c
        .series
        .iter()
        .filter_map(|s| {
            let a = ax[usize::from(s.right)].as_ref()?;
            let y: Vec<Option<f64>> = s.pts.iter().map(|v| v.and_then(|v| a.at(v)).map(|t| py(t, h))).collect();
            let mut col = vec![None; w];
            for (i, y0) in y.iter().enumerate() {
                let Some(y0) = *y0 else { continue };
                col[at(i)] = Some(y0);
                if let Some(y1) = y.get(i + 1).copied().flatten() {
                    let (x0, x1) = (at(i), at(i + 1));
                    (x0 + 1..x1).for_each(|x| col[x] = Some(y0 + (y1 - y0) * (x - x0) as f64 / (x1 - x0) as f64));
                }
            }
            Some((rgba(th, s.hue), col))
        })
        .collect();
    for (color, col) in &curves {
        for (x, y) in col.iter().enumerate().filter_map(|(x, y)| y.map(|y| (x, y))) {
            let span = ((h - 1) as f64 - y).max(1.0);
            for yy in y.round() as i32 + 1..h as i32 {
                let depth = 1.0 - (f64::from(yy) - y) / span;
                if Fill::Gradient.covers(x as i32, yy, ((0.1 + 0.9 * depth) * 0.35) as f32) {
                    g.set(x as i32, yy, color.with_alpha(BODY));
                }
            }
        }
    }
    for (color, col) in &curves {
        let mut prev: Option<i32> = None;
        for (x, y) in col.iter().enumerate() {
            prev = y.map(|y| {
                let now = y.round() as i32;
                let from = prev.unwrap_or(now);
                (from.min(now)..=from.max(now)).for_each(|yy| g.set(x as i32, yy, *color));
                now
            });
        }
    }
    let under = r.xrow.then(|| xrow(th, c, r.wl, r.pw));
    Some(framed(th, &r, &g, under))
}

/// A bar chart: the last bars that fit, a group for each label, a bar for each series, the top row of a bar in the colour and the body dithered.
fn bars(th: &Theme, c: &Chart, f: &Field, width: usize, height: usize) -> Option<Vec<Line<'static>>> {
    let ax = axes(c, f, true);
    let a = ax[0].as_ref()?;
    if f.props.get("orient").and_then(Json::as_str) == Some("horizontal") {
        return across(th, c, a, width, height);
    }
    let r = room(&ax, width, height)?;
    let (w, h, m) = (r.pw, r.ph * 2, c.series.len());
    let keep = (w / (m + 1)).max(1).min(c.x.len());
    let (first, gw) = (c.x.len() - keep, (w / keep).max(m));
    let bw = ((gw - 1) / m).max(1);
    let mut g = PixelGrid::new(w, h);
    for group in 0..keep {
        for (k, s) in c.series.iter().enumerate() {
            let t = a.at(s.pts[first + group].unwrap_or(0.0)).unwrap_or(0.0);
            let e = ((t * h as f64).round() as i32).max(i32::from(t > 0.0));
            let color = rgba(th, s.hue);
            for x in (group * gw + k * bw..group * gw + (k + 1) * bw).filter(|x| *x < w) {
                for d in 0..e {
                    let (cap, y) = (d == e - 1, h as i32 - 1 - d);
                    if cap {
                        g.set(x as i32, y, color);
                    } else if Fill::Gradient.covers(x as i32, y, 0.3 + 0.7 * d as f32 / e as f32) {
                        g.set(x as i32, y, color.with_alpha(BODY));
                    }
                }
            }
        }
    }
    let labels: String = (0..keep).map(|group| format!("{:<gw$}", crate::wrap::fit(&c.x[first + group], gw.saturating_sub(1)))).collect();
    let under = r.xrow.then(|| Line::from(vec![Span::raw(" ".repeat(2 + r.wl + 1)), dim(th, &labels, r.pw, false)]));
    Some(framed(th, &r, &g, under))
}

/// Bars across: a row for each bar, the label at the first of its group and the value after the bar.
fn across(th: &Theme, c: &Chart, a: &Axis, width: usize, height: usize) -> Option<Vec<Line<'static>>> {
    let m = c.series.len();
    let wl = c.x.iter().map(|x| x.width()).max()?.min(14);
    let values: Vec<Vec<String>> = c.series.iter().map(|s| s.pts.iter().map(|v| v.map_or(String::new(), |v| show(v, &s.unit))).collect()).collect();
    let wv = values.iter().flatten().map(|v| v.width()).max()?;
    let room = width.checked_sub(2 + wl + 1 + 1 + wv).filter(|r| *r >= 4)?;
    let keep = (height.saturating_sub(1) / m).max(1);
    let shown = c.x.len().saturating_sub(keep)..c.x.len();
    let rows = shown.len() * m;
    let mut g = PixelGrid::new(room, rows * 2);
    for (i, (_, s)) in shown.clone().flat_map(|gi| c.series.iter().map(move |s| (gi, s))).enumerate() {
        let gi = shown.start + i / m;
        let t = a.at(s.pts[gi].unwrap_or(0.0)).unwrap_or(0.0);
        let len = ((t * room as f64).round() as i32).max(i32::from(t > 0.0));
        let color = rgba(th, s.hue);
        for x in 0..len {
            for y in [i as i32 * 2, i as i32 * 2 + 1] {
                if x == len - 1 {
                    g.set(x, y, color);
                } else if Fill::Gradient.covers(x, y, 0.3 + 0.7 * x as f32 / len as f32) {
                    g.set(x, y, color.with_alpha(BODY));
                }
            }
        }
    }
    Some(
        cells(th, &g, room, rows)
            .into_iter()
            .enumerate()
            .map(|(i, row)| {
                let (gi, k) = (shown.start + i / m, i % m);
                let mut spans = vec![Span::raw("  "), dim(th, if k == 0 { &c.x[gi] } else { "" }, wl, true), Span::raw(" ")];
                spans.extend(row);
                spans.push(Span::raw(" "));
                spans.push(dim(th, &values[k][gi], wv, false));
                Line::from(spans)
            })
            .collect(),
    )
}
