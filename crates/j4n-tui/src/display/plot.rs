//! Charts on one canvas: the fields `line` and `bars`. Cells: braille (2×4 dots) for lines, quadrants
//! (2×2) when the points are many, box lines for `stream` and `wave`, eighth blocks for bars. Axes are linear or log10 (labels on powers of
//! ten, a value ≤ 0 not drawn), with a right axis of its own; the window is the last points, and the axes are fitted to what is in it.
//! Labels are `Dim`, gridlines `Edge` at least four rows apart, and each series has a role colour. Pure functions of the value, the props
//! and the theme; the props are checked by `j4n_proto::chart_problem`, which the core checks pages with, and a field that fails it says so.

use super::number;
use crate::fields::parse;
use crate::theme::Theme;
use crate::theme::glyph::{Slot, Tier};
use crate::theme::palette::Role as Hue;
use j4n_proto::{Field, FieldKind, Json, Tone, chart_problem};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

/// The rows a chart takes unless its props say (`rows`), and the points it shows (`window`).
const ROWS: usize = 8;
const WINDOW: usize = 30;
/// The colour a series has when it names none, by its place: one is `Accent`, two are `Accent` and `Run`.
const ROLES: [Hue; 4] = [Hue::Accent, Hue::Run, Hue::Fetch, Hue::Warn];
/// Braille dots by `[row][column]`, quadrants by mask (up-left 1, up-right 2, down-left 4, down-right 8), box lines by mask (left 1, right 2, up 4, down 8).
const DOTS: [[u8; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];
const QUADS: [char; 16] = [' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█'];

pub(super) struct Serie {
    pub(super) name: String,
    pub(super) right: bool,
    pub(super) hue: Hue,
    pub(super) unit: String,
    pub(super) pts: Vec<Option<f64>>,
}

pub(super) struct Chart {
    pub(super) x: Vec<String>,
    pub(super) series: Vec<Serie>,
}

/// Why the props cannot be drawn, or the chart in the window (`None`: there is nothing to draw yet).
fn read(f: &Field, v: &Json) -> Result<Option<Chart>, String> {
    if let Some(why) = chart_problem(f.kind, &f.props) {
        return Err(why);
    }
    let window = f.props.get("window").and_then(Json::as_u64).map_or(WINDOW, |w| w as usize);
    let x: Vec<String> = v.get("x").and_then(Json::as_array).map_or(Vec::new(), |l| l.iter().map(|s| s.as_str().map(String::from).unwrap_or_else(|| s.to_string())).collect());
    let from = x.len().saturating_sub(window);
    let spec = f.props.get("series").and_then(Json::as_array).map_or(&[][..], Vec::as_slice);
    let series = spec
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let name = s["name"].as_str().unwrap_or_default().to_string();
            let col = v.get("series").and_then(|l| l.get(&name)).and_then(Json::as_array).map_or(&[][..], Vec::as_slice);
            let pts = (from..x.len()).map(|i| col.get(i).and_then(number).filter(|n| n.is_finite())).collect();
            Serie { right: s["axis"] == "right" && f.kind == FieldKind::Line, hue: s["role"].as_str().and_then(Hue::parse).unwrap_or(ROLES[i]), unit: s["unit"].as_str().unwrap_or("n").into(), name, pts }
        })
        .collect();
    let chart = Chart { x: x[from..].to_vec(), series };
    // nothing in the window that can be drawn (every value ≤ 0 on a log axis) is the same as nothing to draw
    Ok((x.len() > from && axes(&chart, f, false).iter().any(Option::is_some)).then_some(chart))
}

/// How many rows the field takes: its `rows` (8), or one when it has nothing to say.
pub fn rows(f: &Field, v: &Json) -> usize {
    match read(f, v) {
        Ok(Some(c)) if one_point(&c, f) => 1 + c.series.iter().filter(|s| s.pts[0].is_some()).count(),
        Ok(Some(_)) => f.props.get("rows").and_then(Json::as_u64).map_or(ROWS, |r| r as usize),
        _ => 1,
    }
}

/// A value as short words in its unit: `$1.24`, `45%`, `3m`, `1.2k`.
pub(super) fn show(v: f64, unit: &str) -> String {
    let n = |x: f64| parse::num((x * 10.0).round() / 10.0);
    match unit {
        "usd" if v >= 100.0 => format!("${v:.0}"),
        "usd" if v >= 1.0 => format!("${}", n(v)),
        "usd" if v >= 0.1 => format!("${}", parse::num((v * 100.0).round() / 100.0)),
        "usd" => format!("${}", parse::num((v * 1e4).round() / 1e4)),
        "pct" => format!("{}%", (v * 100.0).round()),
        "s" if v < 10.0 => format!("{}s", n(v)),
        "s" if v < 60.0 => format!("{v:.0}s"),
        "s" if v < 3600.0 => format!("{}m", n(v / 60.0)),
        "s" => format!("{}h", n(v / 3600.0)),
        _ if v.abs() >= 1e6 => format!("{}M", n(v / 1e6)),
        _ if v.abs() >= 1e3 => format!("{}k", n(v / 1e3)),
        _ => n(v),
    }
}

/// A value axis: linear between two numbers, or log10 between two powers of ten.
pub(super) struct Axis {
    pub(super) lo: f64,
    pub(super) hi: f64,
    pub(super) log: bool,
    pub(super) unit: String,
}

impl Axis {
    fn new(vals: Vec<f64>, log: bool, unit: &str, zero: bool) -> Option<Axis> {
        let vals: Vec<f64> = vals.into_iter().filter(|v| !log || *v > 0.0).collect();
        let (lo, hi) = vals.iter().fold((f64::MAX, f64::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
        if vals.is_empty() {
            return None;
        }
        let (lo, hi) = if log { (lo.log10().floor(), hi.log10().ceil()) } else if zero { (lo.min(0.0), hi) } else { (lo, hi) };
        Some(Axis { lo, hi: if hi <= lo { lo + 1.0 } else { hi }, log, unit: unit.into() })
    }

    /// Where a value stands, 0 at the bottom and 1 at the top; none when it cannot be drawn (≤ 0 on a log axis).
    pub(super) fn at(&self, v: f64) -> Option<f64> {
        let v = if self.log { (v > 0.0).then(|| v.log10())? } else { v };
        Some(((v - self.lo) / (self.hi - self.lo)).clamp(0.0, 1.0))
    }

    /// The values the axis is labelled at: the top, the middle and the bottom, or every power of ten on a log axis, from the top.
    pub(super) fn wants(&self) -> Vec<f64> {
        if self.log { (self.lo as i64..=self.hi as i64).rev().map(|e| 10f64.powi(e as i32)).collect() } else { vec![self.hi, (self.lo + self.hi) / 2.0, self.lo] }
    }

    /// The labels, as `(row, words)` for a plot of `rows` rows: the top and the bottom, and what stands four rows from them (powers of ten on a log axis).
    fn ticks(&self, rows: usize) -> Vec<(usize, String)> {
        let at = |t: f64| ((1.0 - t) * (rows - 1) as f64).round() as usize;
        let mut out: Vec<(usize, String)> = Vec::new();
        for v in self.wants() {
            let row = at(self.at(v).unwrap_or(0.0));
            if out.iter().all(|(r, _)| r.abs_diff(row) >= 4) || out.is_empty() {
                out.push((row, show(v, &self.unit)));
            }
        }
        out
    }
}

/// The cells of the plot: what is drawn in each (a mask) and by which series.
struct Canvas {
    w: usize,
    cell: Vec<(u8, Option<Hue>)>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Canvas {
        Canvas { w, cell: vec![(0, None); w * h] }
    }

    fn put(&mut self, x: usize, y: usize, bits: u8, hue: Hue) {
        let c = &mut self.cell[y * self.w + x];
        *c = (c.0 | bits, Some(hue));
    }

    /// A dot at `(x, y)` of a grid two dots wide and `per` dots high a cell.
    fn dot(&mut self, x: usize, y: usize, per: usize, hue: Hue) {
        let bit = if per == 4 { DOTS[y % 4][x % 2] } else { 1 << (y % 2 * 2 + x % 2) };
        self.put(x / 2, y / per, bit, hue);
    }

    /// A line of dots between two points (Bresenham).
    fn line(&mut self, a: (i64, i64), b: (i64, i64), per: usize, hue: Hue, dotted: bool) {
        let (dx, dy) = ((b.0 - a.0).abs(), -(b.1 - a.1).abs());
        let (sx, sy, mut err, mut p, mut step) = (if a.0 < b.0 { 1 } else { -1 }, if a.1 < b.1 { 1 } else { -1 }, dx + dy, a, 0);
        loop {
            // a dotted line keeps every other dot of the steps
            if !dotted || step % 2 == 0 {
                self.dot(p.0 as usize, p.1 as usize, per, hue);
            }
            step += 1;
            if p == b {
                return;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                (err, p.0) = (err + dy, p.0 + sx);
            }
            if e2 <= dx {
                (err, p.1) = (err + dx, p.1 + sy);
            }
        }
    }

    /// A line of box lines, a value a column: each step goes right, and the rise or fall is made in the new column.
    fn stream(&mut self, rows: &[Option<usize>], hue: Hue) {
        for (c, r) in rows.iter().enumerate() {
            let Some(r) = *r else { continue };
            let Some(p) = c.checked_sub(1).and_then(|p| rows[p]) else {
                self.put(c, r, 0, hue);
                continue;
            };
            self.put(c - 1, p, 2, hue);
            self.put(c, p, 1, hue);
            let (a, b) = (p.min(r), p.max(r));
            if a < b {
                self.put(c, p, if r < p { 4 } else { 8 }, hue);
                self.put(c, r, if r < p { 8 } else { 4 }, hue);
                (a + 1..b).for_each(|y| self.put(c, y, 12, hue));
            }
        }
    }
}

/// The box line of a mask (left 1, right 2, up 4, down 8), or a plain one in the ascii tier.
fn boxed(mask: u8, ascii: bool) -> char {
    if ascii {
        return match mask { 0..=3 => '-', 4 | 8 | 12 => '|', _ => '+' };
    }
    match mask { 0..=3 => '─', 4 | 8 | 12 => '│', 10 => '╭', 9 => '╮', 6 => '╰', 5 => '╯', 15 => '┼', 7 => '┴', 11 => '┬', 13 => '┤', _ => '├' }
}

/// The spans of a row of text, `Dim`, in `width` cells.
pub(super) fn dim(th: &Theme, text: &str, width: usize, right: bool) -> Span<'static> {
    super::column(text, width, right, th.fg(Hue::Dim))
}

/// A line chart with one point in the window: a dot and its value, with no axis (a range made up around one number says nothing).
fn one_point(c: &Chart, f: &Field) -> bool {
    f.kind == FieldKind::Line && c.x.len() == 1
}

/// Colour cannot tell the series apart (the palette `mono`, or colours off): the second, fourth… series is drawn dotted, and its mark says so.
fn dotted(th: &Theme, i: usize) -> bool {
    i % 2 == 1 && (th.no_color || th.palette == "mono")
}

/// The mark of a series in the names: a dot, or the two dots its dotted line is made of.
fn mark(th: &Theme, i: usize) -> String {
    if dotted(th, i) { "⠂⠐".into() } else { th.glyph(Slot::Bullet).to_string() }
}

/// The row of names, each in its colour.
fn legend(th: &Theme, label: &str, c: &Chart) -> Line<'static> {
    let mut spans = vec![Span::raw("  "), Span::styled(label.to_string(), th.fg(Hue::Dim))];
    // one series called what the chart is called needs no second name
    for (i, s) in c.series.iter().enumerate().filter(|(_, s)| c.series.len() > 1 || s.name != label) {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(format!("{} {}", mark(th, i), s.name), th.fg(s.hue)));
    }
    Line::from(spans)
}

/// The dot of a chart with one point, a row for each series that has a value: the mark, then the value (and the series' name if it has more).
fn dots(th: &Theme, c: &Chart) -> Vec<Line<'static>> {
    c.series
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let v = s.pts.first().copied().flatten()?;
            let name = if c.series.len() > 1 { format!("  {}", s.name) } else { String::new() };
            Some(Line::from(vec![Span::raw("  "), Span::styled(format!("{} {}", mark(th, i), show(v, &s.unit)), th.fg(s.hue)), Span::styled(name, th.fg(Hue::Dim))]))
        })
        .collect()
}

/// Words that say why nothing is drawn.
fn aside(th: &Theme, label: &str, why: &str, tone: Tone) -> Vec<Line<'static>> {
    vec![Line::from(vec![Span::raw("  "), Span::styled(label.to_string(), th.fg(Hue::Dim)), Span::raw("  "), Span::styled(why.to_string(), th.tone(tone))])]
}

/// The sizes the plot is given: its width and rows, the widths of the two axes' labels, and whether a row of x labels stands under it.
struct Plan {
    pw: usize,
    ph: usize,
    wl: usize,
    wr: usize,
    xrow: bool,
}

/// The room a chart `height` rows high has, over the names, for a plot whose axes have these labels.
fn plan(width: usize, height: usize, left: &[(usize, String)], right: &[(usize, String)]) -> Option<Plan> {
    let (wl, wr) = (left.iter().map(|t| t.1.width()).max().unwrap_or(0), right.iter().map(|t| t.1.width()).max().unwrap_or(0));
    let pw = width.checked_sub(2 + wl + 1 + if right.is_empty() { 0 } else { 1 + wr })?;
    let xrow = height >= 4;
    let ph = height.saturating_sub(1 + usize::from(xrow));
    (pw >= 6 && ph >= 2).then_some(Plan { pw, ph, wl, wr, xrow })
}

/// The chart as lines `width` cells wide and `height` rows high: the names, the plot between its axes, and under it the first and last label.
pub fn draw(th: &Theme, label: &str, v: &Json, f: &Field, width: usize, height: usize) -> Vec<Line<'static>> {
    let c = match read(f, v) {
        Err(why) => return aside(th, label, &why, Tone::Warn),
        Ok(None) => return aside(th, label, "—", Tone::Dim),
        Ok(Some(c)) => c,
    };
    if one_point(&c, f) {
        let mut body = dots(th, &c);
        body.insert(0, legend(th, label, &c));
        body.truncate(height.max(1));
        return body;
    }
    // dithered half blocks where the terminal and the props allow, the braille and block drawer everywhere else
    let lines = super::dither::draw(th, &c, f, width, height).or_else(|| if f.kind == FieldKind::Bars { bars(th, &c, f, width, height) } else { line(th, &c, f, width, height) });
    let Some(mut body) = lines else { return aside(th, label, "too small for a chart", Tone::Dim) };
    body.insert(0, legend(th, label, &c));
    body.truncate(height.max(1));
    body
}

pub(super) fn axes(c: &Chart, f: &Field, zero: bool) -> [Option<Axis>; 2] {
    let log = |key: &str| f.props.get(key).is_some_and(|s| s == "log");
    [false, true].map(|right| {
        let on: Vec<&Serie> = c.series.iter().filter(|s| s.right == right).collect();
        let unit = on.first().map_or("n", |s| s.unit.as_str());
        Axis::new(on.iter().flat_map(|s| s.pts.iter().flatten().copied()).collect(), log(if right { "right_scale" } else { "scale" }), unit, zero)
    })
}

/// The rows of a plot between its axes: each as the spans of its cells, with the labels of the axes beside it.
fn framed(th: &Theme, p: &Plan, cv: &Canvas, ax: &[Option<Axis>; 2], paint: &dyn Fn(u8) -> char, x: &Chart) -> Vec<Line<'static>> {
    let ticks = [ax[0].as_ref().map(|a| a.ticks(p.ph)).unwrap_or_default(), ax[1].as_ref().map(|a| a.ticks(p.ph)).unwrap_or_default()];
    let grid = th.glyph(Slot::RuleDashed).chars().next().unwrap_or('.').to_string();
    let edge = th.fg(Hue::Edge);
    let mut out = Vec::new();
    for y in 0..p.ph {
        let label = |side: usize| ticks[side].iter().find(|t| t.0 == y).map(|t| t.1.clone());
        let mut spans = vec![Span::raw("  "), dim(th, &label(0).unwrap_or_default(), p.wl, true), Span::styled(th.glyph(Slot::BoxV), edge)];
        let marked = ticks.iter().any(|t| t.iter().any(|t| t.0 == y)) && p.ph >= 8;
        for col in 0..p.pw {
            let (mask, hue) = cv.cell[y * cv.w + col];
            match hue {
                Some(h) => spans.push(Span::styled(paint(mask).to_string(), th.fg(h))),
                _ if marked => spans.push(Span::styled(grid.clone(), edge)),
                _ => spans.push(Span::raw(" ")),
            }
        }
        if !ticks[1].is_empty() {
            spans.push(Span::styled(th.glyph(Slot::BoxV), edge));
            spans.push(dim(th, &label(1).unwrap_or_default(), p.wr, false));
        }
        out.push(Line::from(spans));
    }
    if p.xrow {
        out.push(xrow(th, x, p.wl, p.pw));
    }
    out
}

/// The row under a plot: the first and the last label of the x axis, `wl` cells of the left axis in.
pub(super) fn xrow(th: &Theme, x: &Chart, wl: usize, pw: usize) -> Line<'static> {
    let (first, last) = (x.x.first().cloned().unwrap_or_default(), x.x.last().cloned().unwrap_or_default());
    let room = pw.saturating_sub(first.width() + last.width());
    let words = if room >= 1 && x.x.len() > 1 { format!("{first}{}{last}", " ".repeat(room)) } else { first };
    Line::from(vec![Span::raw(" ".repeat(2 + wl + 1)), dim(th, &words, pw, false)])
}

fn line(th: &Theme, c: &Chart, f: &Field, width: usize, height: usize) -> Option<Vec<Line<'static>>> {
    let ax = axes(c, f, false);
    let ticks = |i: usize| ax[i].as_ref().map(|a| a.ticks(height.saturating_sub(2).max(2))).unwrap_or_default();
    let p = plan(width, height, &ticks(0), &ticks(1))?;
    let ascii = th.tier == Tier::Ascii;
    let style = match f.props.get("style").and_then(Json::as_str) {
        _ if ascii => "stream",
        Some(s) => s,
        None => "braille",
    };
    let n = c.x.len();
    let mut cv = Canvas::new(p.pw, p.ph);
    // quadrants take over from braille when there are many more points than columns: the dots would be a haze
    let per = if n > 3 * p.pw { 2 } else { 4 };
    let (sw, sh) = (if style == "braille" { p.pw * 2 } else { p.pw }, if style == "braille" { p.ph * per } else { p.ph });
    for (si, s) in c.series.iter().enumerate() {
        let Some(a) = &ax[usize::from(s.right)] else { continue };
        let mut pts: Vec<Option<f64>> = s.pts.clone();
        if style == "wave" {
            // a moving mean of three, so the line moves like a wave
            pts = (0..n).map(|i| s.pts[i].map(|_| { let near: Vec<f64> = s.pts[i.saturating_sub(1)..(i + 2).min(n)].iter().flatten().copied().collect(); near.iter().sum::<f64>() / near.len() as f64 })).collect();
        }
        let y = |v: Option<f64>| v.and_then(|v| a.at(v)).map(|t| ((1.0 - t) * (sh - 1) as f64).round() as usize);
        if style == "braille" {
            let at = |i: usize| (if n == 1 { sw / 2 } else { (i * (sw - 1) + (n - 1) / 2) / (n - 1) }, y(pts[i]));
            let mut prev: Option<(i64, i64)> = None;
            for i in 0..n {
                prev = match at(i) {
                    (x, Some(y)) => {
                        let now = (x as i64, y as i64);
                        cv.line(prev.unwrap_or(now), now, per, s.hue, dotted(th, si));
                        Some(now)
                    }
                    _ => None,
                };
            }
        } else {
            let rows: Vec<Option<usize>> = (0..sw).map(|col| y(pts[if n == 1 { 0 } else { (col * (n - 1) + (sw - 1) / 2) / (sw - 1).max(1) }])).collect();
            cv.stream(&rows, s.hue);
        }
    }
    let quad = style == "braille" && per == 2;
    let paint = |mask: u8| match (style == "braille", quad) {
        (true, true) => QUADS[usize::from(mask & 15)],
        (true, false) => char::from_u32(0x2800 + u32::from(mask)).unwrap_or(' '),
        _ => boxed(mask, ascii),
    };
    Some(framed(th, &p, &cv, &ax, &paint, c))
}

/// Eighths of a cell, as the tier draws them: the partial block of `e` eighths.
fn part(th: &Theme, e: usize, slot: Slot) -> String {
    let levels: Vec<char> = th.glyph(slot).chars().collect();
    levels[(e * levels.len() / 8).clamp(1, levels.len()) - 1].to_string()
}

fn bars(th: &Theme, c: &Chart, f: &Field, width: usize, height: usize) -> Option<Vec<Line<'static>>> {
    let ax = axes(c, f, true);
    let a = ax[0].as_ref()?;
    let m = c.series.len();
    let vert = f.props.get("orient").and_then(Json::as_str) != Some("horizontal");
    let full = th.glyph(Slot::FillFull);
    if !vert {
        // one row for each bar; the label stands at the first of its group, and the value after the bar
        let wl = c.x.iter().map(|x| x.width()).max()?.min(14);
        let values: Vec<Vec<String>> = c.series.iter().map(|s| s.pts.iter().map(|v| v.map_or(String::new(), |v| show(v, &s.unit))).collect()).collect();
        let wv = values.iter().flatten().map(|v| v.width()).max()?;
        let room = width.checked_sub(2 + wl + 1 + 1 + wv).filter(|r| *r >= 4)?;
        let keep = (height.saturating_sub(1) / m).max(1);
        let mut out = Vec::new();
        #[allow(clippy::needless_range_loop)]
        for g in c.x.len().saturating_sub(keep)..c.x.len() {
            for (k, s) in c.series.iter().enumerate() {
                let cells = a.at(s.pts[g].unwrap_or(0.0)).unwrap_or(0.0) * room as f64;
                let (whole, rest) = (cells.floor() as usize, ((cells.fract() * 8.0) as usize).min(7));
                let tail = if rest > 0 && whole < room { part(th, rest, Slot::MeterParts) } else { String::new() };
                out.push(Line::from(vec![
                    Span::raw("  "),
                    dim(th, if k == 0 { &c.x[g] } else { "" }, wl, true),
                    Span::raw(" "),
                    Span::styled(format!("{}{tail}", full.repeat(whole)), th.fg(s.hue)),
                    Span::raw(" ".repeat(room.saturating_sub(whole + tail.chars().count()) + 1)),
                    dim(th, &values[k][g], wv, false),
                ]));
            }
        }
        return Some(out);
    }
    let p = plan(width, height, &a.ticks(height.saturating_sub(2).max(2)), &[])?;
    let keep = (p.pw / (m + 1)).max(1).min(c.x.len());
    let (first, gw) = (c.x.len() - keep, (p.pw / keep).max(m));
    let bw = ((gw - 1) / m).max(1);
    let mut cv = Canvas::new(p.pw, p.ph);
    for g in 0..keep {
        for (k, s) in c.series.iter().enumerate() {
            let e = (a.at(s.pts[first + g].unwrap_or(0.0)).unwrap_or(0.0) * (p.ph * 8) as f64).round() as usize;
            for col in (g * gw + k * bw..g * gw + (k + 1) * bw).filter(|col| *col < p.pw) {
                for r in 0..e.div_ceil(8) {
                    cv.put(col, p.ph - 1 - r, if e - r * 8 >= 8 { 8 } else { (e - r * 8) as u8 }, s.hue);
                }
            }
        }
    }
    let (edge, ticks) = (th.fg(Hue::Edge), a.ticks(p.ph));
    let mut out: Vec<Line<'static>> = (0..p.ph)
        .map(|y| {
            let label = ticks.iter().find(|t| t.0 == y).map(|t| t.1.clone()).unwrap_or_default();
            let mut spans = vec![Span::raw("  "), dim(th, &label, p.wl, true), Span::styled(th.glyph(Slot::BoxV), edge)];
            for col in 0..p.pw {
                let (e, hue) = cv.cell[y * p.pw + col];
                spans.push(match hue {
                    Some(h) if e >= 8 => Span::styled(full.to_string(), th.fg(h)),
                    Some(h) if e > 0 => Span::styled(part(th, usize::from(e), Slot::SparkLevels), th.fg(h)),
                    _ => Span::raw(" "),
                });
            }
            Line::from(spans)
        })
        .collect();
    if !p.xrow {
        return Some(out);
    }
    let labels: String = (0..keep).map(|g| format!("{:<gw$}", crate::wrap::fit(&c.x[first + g], gw.saturating_sub(1)))).collect();
    out.push(Line::from(vec![Span::raw(" ".repeat(2 + p.wl + 1)), dim(th, &labels, p.pw, false)]));
    Some(out)
}
