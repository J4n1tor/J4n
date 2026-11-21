//! The four fields that show a few numbers: `stat` (a big number with its label and what changed), `meter` (how full), `spark` (a
//! series in a row of cells) and `chip` (a mark and a word in the colour of what it says); and the two gauges that say how full a context
//! is: `g3`, the thin line, and `g9`, one slice of a circle.

use super::{named, number, tone_of, width_of};
use crate::wrap::{fit, wrap};
use crate::fields::{Spec, parse};
use crate::nodes::NARROW;
use crate::parts::rows;
use crate::theme::Theme;
use crate::theme::glyph::{Slot, Tier};
use crate::theme::palette::Role as Hue;
use j4n_proto::{Field, Json, Tone};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

/// The pipe at the start of a row: the tee for the first row of a field, the bar under it.
fn lead(th: &Theme, first: bool) -> Span<'static> {
    if first { rows::lead(th, false, false, false) } else { rows::trunk(th, false) }
}

/// The value as the list of entries it holds: a list, or one object.
fn entries(v: &Json) -> Vec<&Json> {
    match v {
        Json::Array(a) => a.iter().collect(),
        Json::Object(_) => vec![v],
        _ => Vec::new(),
    }
}

fn text(v: &Json, key: &str) -> String {
    match v.get(key) {
        Some(Json::String(s)) => s.clone(),
        Some(Json::Number(n)) => n.as_f64().map(parse::num).unwrap_or_default(),
        _ => String::new(),
    }
}

// ---- stat ----

struct Stat {
    v: String,
    label: String,
    /// The mark of the change and all of it, in its tone.
    delta: Option<(String, String, Tone)>,
}

fn stats(th: &Theme, value: &Json) -> Vec<Stat> {
    entries(value)
        .into_iter()
        .map(|e| {
            let delta = e.get("delta").and_then(Json::as_f64).map(|d| {
                let (slot, tone) = if d > 0.0 { (Slot::DeltaUp, Tone::Good) } else if d < 0.0 { (Slot::DeltaDown, Tone::Bad) } else { (Slot::DeltaFlat, Tone::Dim) };
                let unit = e.get("unit").and_then(Json::as_str).unwrap_or("%");
                let mark = th.glyph(slot).to_string();
                let all = if d == 0.0 { mark.clone() } else { format!("{mark} {}{unit}", parse::num(d.abs())) };
                (mark, all, e.get("tone").and_then(Json::as_str).and_then(tone_of).unwrap_or(tone))
            });
            Stat { v: text(e, "v"), label: text(e, "label"), delta }
        })
        .collect()
}

/// Big numbers side by side, each over its label and the change; where they do not fit, one row of numbers with the mark of each change.
pub(crate) fn stat(th: &Theme, value: &Json, width: usize) -> Vec<Line<'static>> {
    let items = stats(th, value);
    if items.is_empty() {
        return Vec::new();
    }
    let cols: Vec<usize> = items.iter().map(|s| s.v.width().max(s.label.width()).max(s.delta.as_ref().map_or(0, |d| d.1.width()))).collect();
    let wide = 2 + cols.iter().sum::<usize>() + 4 * (items.len() - 1) <= width;
    let big = Style::default().add_modifier(Modifier::BOLD);
    if !wide {
        let mut spans = vec![Span::raw("  ")];
        for (i, s) in items.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw("  "));
            }
            spans.push(Span::styled(s.v.clone(), big));
            if let Some((mark, _, tone)) = &s.delta {
                spans.push(Span::styled(format!(" {mark}"), th.tone(*tone)));
            }
        }
        return vec![Line::from(spans)];
    }
    let row = |f: &dyn Fn(&Stat) -> (String, Style)| {
        let mut spans = vec![Span::raw("  ")];
        for (i, (s, w)) in items.iter().zip(&cols).enumerate() {
            let (t, style) = f(s);
            spans.push(Span::styled(format!("{t}{}{}", " ".repeat(w.saturating_sub(t.width())), if i + 1 < items.len() { "    " } else { "" }), style));
        }
        Line::from(spans)
    };
    let mut out = vec![row(&|s| (s.v.clone(), big)), row(&|s| (s.label.clone(), th.tone(Tone::Dim)))];
    if items.iter().any(|s| s.delta.is_some()) {
        out.push(row(&|s| s.delta.as_ref().map_or((String::new(), Style::default()), |d| (d.1.clone(), th.tone(d.2)))));
    }
    out
}

// ---- meter ----

/// A bar `bw` cells wide with `f` of it filled (1 being all), in eighths of a cell where the tier has them.
pub(crate) fn bar(th: &Theme, f: f64, bw: usize, fill: Style) -> Vec<Span<'static>> {
    let parts: Vec<char> = th.glyph(Slot::MeterParts).chars().collect();
    let cells = f.clamp(0.0, 1.0) * bw as f64;
    let full = (cells.floor() as usize).min(bw);
    let part = if parts.len() > 1 { ((cells - full as f64) * 8.0).floor() as usize } else { 0 };
    let mut spans = vec![Span::styled(th.glyph(Slot::FillLeft), th.fg(Hue::Edge))];
    spans.push(Span::styled(th.glyph(Slot::FillFull).repeat(full), fill));
    if part > 0 {
        spans.push(Span::styled(parts[part - 1].to_string(), fill));
    }
    spans.push(Span::styled(th.glyph(Slot::FillEmpty).repeat(bw - full - usize::from(part > 0)), th.fg(Hue::Edge)));
    spans.push(Span::styled(th.glyph(Slot::FillRight), th.fg(Hue::Edge)));
    spans
}

/// Where a meter is drawn: a strip or a row that shares its line with other words (the composer, a chat's row), a page that has room, or the page that is
/// about that number (Usage).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Spot {
    Strip,
    Page,
    Usage,
}

/// The longest a meter is outside the page about that number, in cells. One rule for every meter and progress row.
pub(crate) const LINE_MAX: usize = 8;

/// Whether a meter at `spot` is the circle slice under the word of `look.gauge` (`auto`, `line` or `slice`; a word left out is `line`, what a hand-made
/// strip says): `slice` everywhere, `auto` where the meter shares its line, `line` nowhere.
pub(crate) fn slice_for(form: &str, spot: Spot) -> bool {
    form == "slice" || form == "auto" && spot == Spot::Strip
}

/// The cells of a thin line with `room` to draw in: all of it on the page about the number, at most [`LINE_MAX`] anywhere else.
pub(crate) fn line_cells(spot: Spot, room: usize) -> usize {
    if spot == Spot::Usage { room } else { room.min(LINE_MAX) }
}

/// The warm point of a gauge: from it the fill is `warn`, over 1 `error` (the gauge counts what is used, not what is left).
const WARM: f64 = 0.8;

/// The slices of a circle, empty to full (`g9`): nine Material marks in the icons tier; the other tiers take the rising block of `spark-levels`.
const SLICES: &str = "\u{F0766}\u{F0A9E}\u{F0A9F}\u{F0AA0}\u{F0AA1}\u{F0AA2}\u{F0AA3}\u{F0AA4}\u{F0AA5}";

/// The role a gauge's fill is drawn in, and the tone of its number: `warn` from the warm point, `error` over the whole.
pub(crate) fn fill_role(f: f64) -> (Hue, Tone) {
    if f > 1.0 { (Hue::Error, Tone::Bad) } else if f >= WARM { (Hue::Warn, Tone::Warn) } else { (Hue::Accent, Tone::Dim) }
}

/// `g3`, the thin line: `cells` wide, a heavy line over a light one with a half cell between, so the empty part differs by shape (it reads under
/// `NO_COLOR` and in `mono`); the track is `faint`, never `edge`. The fill keeps to whole halves: the line never says more than there is.
pub(crate) fn thin(th: &Theme, f: f64, cells: usize, fill: Style) -> Vec<Span<'static>> {
    let halves = ((f.clamp(0.0, 1.0) * (2 * cells) as f64 + 1e-9).floor() as usize).min(2 * cells);
    let (heavy, half) = (th.glyph(Slot::RuleHeavy), if th.glyph(Slot::RuleHeavy) == "━" { "╸" } else { "-" });
    vec![
        Span::styled(heavy.repeat(halves / 2), fill),
        Span::styled(half.repeat(halves % 2), fill),
        Span::styled(th.glyph(Slot::BoxH).repeat(cells - halves.div_ceil(2)), th.fg(Hue::Faint)),
    ]
}

/// The heavy line over `len` cells from cell `at` of a thin line `cells` wide, the rest light and faint: what the thin line draws for a fill, with the fill
/// where the caller says (`parts::track`'s segment).
pub(crate) fn thin_cells(th: &Theme, at: usize, len: usize, cells: usize, fill: Style) -> Vec<Span<'static>> {
    let (at, len) = (at.min(cells), len.min(cells.saturating_sub(at)));
    let (heavy, light) = (th.glyph(Slot::RuleHeavy), th.glyph(Slot::BoxH));
    let faint = th.fg(Hue::Faint);
    vec![Span::styled(light.repeat(at), faint), Span::styled(heavy.repeat(len), fill), Span::styled(light.repeat(cells - at - len), faint)]
}

/// The number of a gauge, in four cells so that a row does not move as the context fills, and right after its mark or line (the cells left over trail it,
/// never stand between): `9%  `, `41% `, `104%`.
pub(crate) fn percent(f: f64) -> String {
    format!("{:<4}", format!("{:.0}%", whole(f) * 100.0))
}

/// A share as whole percents, never below nothing: what the number says, and so what the colour and the line go by (79.97% is 80% and warm, 99.6% is 100%).
pub(crate) fn whole(f: f64) -> f64 {
    if f.is_finite() { (f.max(0.0) * 100.0).round() / 100.0 } else { 0.0 }
}

/// How full a context is: `g3` is six cells of the thin line and the number (`━━──── 41%`), `g9` one slice of a circle and the number
/// (`󰪠 41%`, a rising block in the plain tiers, a letter of `_.-=#` in ascii). The fill is the accent, `warn` from 80% and `error` over 100%; the empty
/// part is `faint` and the number `dim`. The gauge counts what is used.
pub(crate) fn gauge(th: &Theme, f: f64, slice: bool) -> Vec<Span<'static>> {
    let f = whole(f);
    let (hue, tone) = fill_role(f);
    let mut spans = if slice {
        let glyphs: Vec<char> = if th.tier == Tier::Icons { SLICES.chars().collect() } else { th.glyph(Slot::SparkLevels).chars().collect() };
        let at = (f.clamp(0.0, 1.0) * (glyphs.len() - 1) as f64).round() as usize;
        // a context with nothing in it is an empty outline, in the track's colour
        vec![Span::styled(format!("{} ", glyphs[at]), th.fg(if f <= 0.0 { Hue::Faint } else { hue }))]
    } else {
        let mut line = thin(th, f, 6, th.fg(hue));
        line.push(Span::raw(" "));
        line
    };
    // the number right after the mark or the line, with no cells for a longer one: a gauge stands in a row with others, and blank cells there are wasted
    spans.push(Span::styled(percent(f).trim_end().to_string(), th.tone(tone)));
    spans
}

/// The label at the left and the bar with its percentage at the right; under 60 columns the bar has a row of its own.
pub(crate) fn meter(th: &Theme, label: &str, value: &Json, field: &Field, width: usize) -> Vec<Line<'static>> {
    let warn = Spec::new(label, field).num("warn").unwrap_or(0.8);
    let Some(f) = number(value) else {
        return vec![Line::from(vec![lead(th, true), Span::raw(label.to_string()), Span::styled("  —", th.tone(Tone::Dim))])];
    };
    let (hue, tone) = if f > 1.0 { (Hue::Error, Tone::Bad) } else if f >= warn { (Hue::Warn, Tone::Warn) } else { (Slot::FillFull.role(), Tone::Plain) };
    let pct = Span::styled(format!("{:<4}", format!("{}%", (f * 100.0).round() as i64)), th.tone(tone));
    let room = width.saturating_sub(2 + label.width() + 2 + 7);
    if width >= usize::from(NARROW) && room >= 4 {
        let mut spans = vec![lead(th, true), Span::raw(label.to_string())];
        let bw = line_cells(Spot::Page, room);
        spans.push(Span::raw(" ".repeat(width - 2 - label.width() - (bw + 1 + 4))));
        spans.extend(thin(th, f, bw, th.fg(hue)));
        spans.push(Span::raw(" "));
        spans.push(pct);
        return vec![Line::from(spans)];
    }
    let bw = line_cells(Spot::Page, width.saturating_sub(2 + 2 + 1 + 4)).max(4);
    let mut under = vec![lead(th, false)];
    under.extend(thin(th, f, bw, th.fg(hue)));
    under.push(Span::raw(" "));
    under.push(pct);
    vec![Line::from(vec![lead(th, true), Span::raw(label.to_string())]), Line::from(under)]
}

/// A list of meters: a `group` is a title, a bar (`label`, `f`, `note`, `tone`) is its label, the bar, its percentage and the note after it (under it
/// where the line is short), and a line of `text` stands as it is. What a view held in the chat draws for `/usage`.
pub(crate) fn meters(th: &Theme, value: &Json, width: usize) -> Vec<Line<'static>> {
    let entries = entries(value);
    let lw = entries.iter().map(|e| text(e, "label").width()).max().unwrap_or(0);
    let bw = line_cells(Spot::Usage, if width >= 70 { 16 } else if width >= 40 { 10 } else { 6 });
    let mut out = Vec::new();
    for e in entries {
        let tone = e.get("tone").and_then(Json::as_str).and_then(tone_of);
        if e.get("group").is_some() {
            out.push(Line::from(Span::styled(text(e, "group"), th.tone(Tone::Accent).add_modifier(Modifier::BOLD))));
            continue;
        }
        let Some(f) = e.get("f").and_then(Json::as_f64) else {
            out.push(Line::from(vec![Span::raw("  "), Span::styled(fit(&text(e, "text"), width.saturating_sub(2)), th.tone(tone.unwrap_or(Tone::Plain)))]));
            continue;
        };
        let (hue, tone) = match tone {
            Some(Tone::Bad) => (Hue::Error, Tone::Bad),
            Some(Tone::Warn) => (Hue::Warn, Tone::Warn),
            _ if f >= 0.8 => (Hue::Warn, Tone::Warn),
            _ => (Slot::FillFull.role(), Tone::Plain),
        };
        let label = text(e, "label");
        let mut spans = vec![Span::raw(format!("  {label}{} ", " ".repeat(lw - label.width())))];
        spans.extend(thin(th, f, bw, th.fg(hue)));
        spans.push(Span::styled(format!(" {:<4}", format!("{}%", (f * 100.0).round() as i64)), th.tone(tone)));
        let (note, indent) = (text(e, "note"), 2 + lw + 1);
        let room = width.saturating_sub(width_of(&spans) + 2);
        if note.width() <= room {
            spans.push(Span::styled(format!("  {note}"), th.tone(Tone::Dim)));
            out.push(Line::from(spans));
        } else {
            out.push(Line::from(spans));
            out.extend(wrap(&note, width.saturating_sub(indent).max(8)).into_iter().map(|l| Line::from(vec![Span::raw(" ".repeat(indent)), Span::styled(l, th.tone(Tone::Dim))])));
        }
    }
    out
}

// ---- spark ----

/// The series and the last value as text: a list of numbers, or `{points, last}`.
fn series(v: &Json, unit: &str) -> (Vec<f64>, String) {
    let pts = match v {
        Json::Array(a) => a.as_slice(),
        _ => v.get("points").and_then(Json::as_array).map_or(&[][..], Vec::as_slice),
    };
    let points: Vec<f64> = pts.iter().filter_map(Json::as_f64).collect();
    let last = match v.get("last") {
        Some(Json::String(s)) => s.clone(),
        _ => points.last().map(|x| format!("{}{unit}", parse::num(*x))).unwrap_or_default(),
    };
    (points, last)
}

/// The label, then the series as cells of eight heights between its lowest and its highest, then the last value.
pub(crate) fn spark(th: &Theme, label: &str, value: &Json, field: &Field, width: usize) -> Vec<Line<'static>> {
    let spec = Spec::new(label, field);
    let (points, last) = series(value, spec.text("unit").unwrap_or(""));
    // the last values line up under one another, so the charts end where the others do
    let last = format!("{last:>6}");
    let window = spec.num("window").map_or(40, |w| (w as usize).max(1));
    let levels: Vec<char> = th.glyph(Slot::SparkLevels).chars().collect();
    let room = width.saturating_sub(2 + label.width() + 2 + 2 + last.width());
    let narrow = width < usize::from(NARROW) || room < 4;
    let take = window.min(if narrow { width.saturating_sub(2 + 2 + last.width()).max(4) } else { room });
    let tail = &points[points.len().saturating_sub(take)..];
    let (lo, hi) = tail.iter().fold((f64::MAX, f64::MIN), |(l, h), x| (l.min(*x), h.max(*x)));
    let cells: String = tail.iter().map(|x| levels[if hi > lo { (((x - lo) / (hi - lo)) * (levels.len() - 1) as f64).round() as usize } else { 0 }]).collect();
    let mut chart = vec![Span::styled(cells, th.fg(Slot::SparkLevels.role())), Span::raw("  "), Span::raw(last)];
    if narrow {
        let mut under = vec![lead(th, false)];
        under.append(&mut chart);
        return vec![Line::from(vec![lead(th, true), Span::raw(label.to_string())]), Line::from(under)];
    }
    let used = 2 + label.width() + width_of(&chart);
    let mut spans = vec![lead(th, true), Span::raw(label.to_string()), Span::raw(" ".repeat(width.saturating_sub(used)))];
    spans.append(&mut chart);
    vec![Line::from(spans)]
}

// ---- chip ----

/// A mark and a word in the colour of what they say; a list of them is one row with two blanks between.
pub(crate) fn chip(th: &Theme, value: &Json) -> Vec<Line<'static>> {
    let mut spans = vec![Span::raw("  ")];
    for (i, e) in entries(value).into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        if let Some(mark) = e.get("icon").and_then(Json::as_str).and_then(|n| named(th, n)) {
            spans.push(mark);
        }
        let tone = e.get("tone").and_then(Json::as_str).and_then(tone_of).unwrap_or(Tone::Plain);
        spans.push(Span::styled(text(e, "text"), th.tone(tone)));
    }
    vec![Line::from(spans)]
}
