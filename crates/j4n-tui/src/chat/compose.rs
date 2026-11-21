//! The composer: a rounded frame under the transcript with the strip above the line — at the left one still mark
//! with the count of the chat's open tasks, the plan's thin line and the tags the draft was given with how sure the core is of
//! each; at the right edge, as one group, what the chat has cost, the doer · model it goes to and how full the context is — then a
//! dashed rule, then the line itself, which grows upward to eight rows. The target of the line stands at the right of its last row; while a step of
//! J4n's own runs, the words that end it stand dim in the empty line (`parts::hint`); the rest of what Tab would complete stands dim after the
//! cursor (`parts::suggest`). The strip
//! is not drawn when it has nothing to say: in the orchestrator (the core sends none) and in a chat that has no doer and does no work.
//! While there are suggestions their list takes the strip's left and the right group stays; with no strip the list's row and its rule
//! appear, and stay while the line is not empty.

use crate::app::App;
use crate::parts::hint::{self, Hint};
use crate::parts::suggest::{self, List};
use crate::parts::{popup::{BOX, frame}, scroll::keep};
use crate::display::{Spot, context_gauge, join, mark_sp, named, slice_for, thin, width_of};
use crate::shape::Card;
use crate::theme::Theme;
use crate::theme::glyph::Slot;
use crate::theme::palette::Role as Hue;
use crate::wrap::fit;
use j4n_proto::{COMPOSER_BIND, COMPOSER_STRIP, Field, Json, Row, Tone};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// The most rows the line takes before it scrolls.
const MAX_ROWS: usize = 8;
/// The mark at which the context gauge turns warm.
const WARM: f64 = 0.8;
/// From this width the list and the right group share a row; under [`SHORT`] the list is the one in the line and its place.
const WIDE: usize = 80;
const SHORT: usize = 50;

/// The composer as it is drawn: its lines, how tall it is, and where the cursor stands in it.
pub(crate) struct Composer {
    pub lines: Vec<Line<'static>>,
    pub cursor: (u16, u16),
}

struct Strip {
    tags: Vec<(String, String, Option<u64>)>,
    gauge: Option<f64>,
    route: String,
    /// The route is a model's guess (drawn with `~`) unless the core says it is its own word or a rule's: `route_by` of `core`, `rule` or `operator`.
    route_made: bool,
    /// The thread has no doer yet (`route_by` of `none`): the route is said dim.
    route_none: bool,
    /// How many tasks of the chat are open: running, queued or cooling.
    work: usize,
    /// The chat's plan: items done and all of them.
    plan: Option<(u64, u64)>,
    /// What the thread has cost, as the core words it; and the mode, `write` or `read`.
    cost: String,
    mode: String,
    /// How many skills the running tasks were given: the first thing a short row gives up.
    skills: usize,
    /// The gauge is one circle slice (`g9`) and not the thin line (`g3`): the core says it (`look.gauge`).
    slice: bool,
}

fn strip_of(v: Option<&Json>) -> Option<Strip> {
    let v = v?;
    let tags: Vec<(String, String, Option<u64>)> = v
        .get("tags")
        .and_then(Json::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    let text = t.get("text").and_then(Json::as_str)?;
                    Some((t.get("icon").and_then(Json::as_str).unwrap_or("tag").to_string(), text.to_string(), t.get("pct").and_then(Json::as_u64)))
                })
                .collect()
        })
        .unwrap_or_default();
    let gauge = v.get("gauge").and_then(Json::as_f64);
    let route = match v.get("route") {
        Some(Json::String(s)) => s.clone(),
        Some(Json::Array(a)) => a.iter().filter_map(Json::as_str).collect::<Vec<_>>().join(" ▸ "),
        _ => String::new(),
    };
    let by = v.get("route_by").and_then(Json::as_str);
    let (route_made, route_none) = (!matches!(by, Some("core" | "rule" | "operator" | "none")), by == Some("none"));
    let work = v.get("work").and_then(Json::as_array).map_or(0, Vec::len);
    let plan = v.get("plan").and_then(|p| Some((p.get("done")?.as_u64()?, p.get("total")?.as_u64()?))).filter(|(_, total)| *total > 0);
    let text = |k: &str| v.get(k).and_then(Json::as_str).unwrap_or_default().to_string();
    // a chat with no doer and no work has nothing to say: the strip is hidden, and the chat's empty state says `no doer yet`
    (!(route_none && work == 0) && (!tags.is_empty() || gauge.is_some() || !route.is_empty() || work > 0))
        .then(|| Strip { tags, gauge, route, route_made, route_none, work, plan, cost: text("cost"), mode: text("mode"), skills: v.get("skills").and_then(Json::as_u64).unwrap_or(0) as usize, slice: slice_for(&text("form"), Spot::Strip) })
}

/// The strip says the chat has no doer: its empty state tells so.
pub(crate) fn no_doer(v: Option<&Json>) -> bool {
    v.and_then(|v| v.get("route_by")).and_then(Json::as_str) == Some("none") && v.and_then(|v| v.get("work")).and_then(Json::as_array).is_none_or(Vec::is_empty)
}

/// The words of a chat's empty state: dim, with the mark of a thing that waits. The doer the first line goes to is named when the core
/// knows it (`next`: the launch's `--doer`, or Setup's pick).
pub(crate) fn empty_state(th: &Theme, v: Option<&Json>) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    spans.extend(named(th, "queued"));
    let words = match v.and_then(|v| v.get("next")).and_then(Json::as_str).filter(|n| !n.is_empty()) {
        Some(next) => format!("{next} · the first line goes to it"),
        None => "no doer yet · the first line picks one".to_string(),
    };
    spans.push(Span::styled(words, th.tone(Tone::Dim)));
    Line::from(spans)
}

/// The plan's progress as a thin line: `plan 2/3 ━━━━──`, six cells, a half cell where the share falls between two; the words alone when the row
/// is short. The orchestrator's plan row is drawn by this one function too (`plan_row`).
pub(crate) fn plan_line(th: &Theme, done: u64, total: u64, line: bool) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(format!("plan {done}/{total}"), th.tone(Tone::Dim))];
    if line {
        spans.push(Span::raw(" "));
        spans.extend(thin(th, done.min(total) as f64 / total.max(1) as f64, 6, th.fg(Hue::Accent)));
    }
    spans
}

/// What of the strip is said, and how: the strip gives up its parts one at a time, in the order of [`GIVE_UP`], until its row fits.
#[derive(Clone, Copy)]
struct Say {
    /// The count of skills, at the left beside the mark.
    skills: bool,
    mode: bool,
    /// 2 whole, 1 the first money word (the share of the window, else the dollars), 0 none.
    cost: u8,
    /// 2 the gauge, 1 the number alone (`g11`), 0 none.
    gauge: u8,
    /// The plan's thin line after its words.
    line: bool,
}

/// Narrow screens: the count of skills goes first, then the mode, then the cost is cut to its first word, the gauge to its percent, the plan's
/// line goes and leaves its words, then the cost, then the gauge. The doer and model, and the mark with its count, stay.
const GIVE_UP: [fn(&mut Say); 7] = [|s| s.skills = false, |s| s.mode = false, |s| s.cost = 1, |s| s.gauge = 1, |s| s.line = false, |s| s.cost = 0, |s| s.gauge = 0];

fn parts(th: &Theme, s: &Strip, say: Say, route_max: usize) -> (Vec<Vec<Span<'static>>>, Vec<Vec<Span<'static>>>) {
    let dim = th.tone(Tone::Dim);
    let mut left: Vec<Vec<Span<'static>>> = Vec::new();
    // one still mark and the count of the chat's open tasks: it does not move
    if s.work > 0 {
        left.push(vec![mark_sp(th, Slot::Working), Span::styled(s.work.to_string(), dim)]);
    }
    if let Some((done, total)) = s.plan {
        left.push(plan_line(th, done, total, say.line));
    }
    if say.skills && s.skills > 0 {
        left.push(vec![Span::styled(format!("{} skill{}", s.skills, if s.skills == 1 { "" } else { "s" }), dim)]);
    }
    // the group stuck to the right edge: the mode when it is the unusual one, the cost, `doer · model`, and last the gauge named by them
    let mut right: Vec<Vec<Span<'static>>> = Vec::new();
    if say.mode && s.mode == "read" {
        right.push(vec![Span::styled(s.mode.clone(), dim)]);
    }
    if say.cost > 0 && !s.cost.is_empty() {
        let first = s.cost.split(" · ").next().unwrap_or_default();
        right.push(vec![Span::styled(if say.cost == 2 { s.cost.clone() } else { first.to_string() }, dim)]);
    }
    let mut who: Vec<Span<'static>> = Vec::new();
    if !s.route.is_empty() {
        if s.route_made {
            who.push(Span::styled("~", th.fg(Hue::Made)));
        }
        who.extend(named(th, "route"));
        who.push(if s.route_none { Span::styled(fit(&s.route, route_max), dim) } else { Span::raw(fit(&s.route, route_max)) });
    }
    if let Some(g) = s.gauge.filter(|_| say.gauge > 0) {
        if !who.is_empty() {
            who.push(Span::raw(" "));
        }
        who.extend(if say.gauge == 2 { context_gauge(th, g, s.slice) } else { vec![Span::styled(format!("{:.0}%", g * 100.0), th.tone(if g >= WARM { Tone::Warn } else { Tone::Dim }))] });
    }
    if !who.is_empty() {
        right.push(who);
    }
    (left, right)
}

/// The rows of the strip, `iw` columns wide: the mark with its count, the plan and the tags at the left; at the right edge, as one group, the cost, the
/// doer · model (it gives up nothing) and the gauge; when they do not fit side by side, even with the right giving up what it can, the right under the left.
fn strip_rows(th: &Theme, s: &Strip, iw: usize) -> Vec<Vec<Span<'static>>> {
    // a tag is the mark, the words and ` ~100%`: the words give way before the row overflows
    let cap = iw.saturating_sub(10).max(4);
    let mut pills: Vec<Vec<Span<'static>>> = s
        .tags
        .iter()
        .map(|(icon, text, pct)| {
            let mut p: Vec<Span<'static>> = named(th, icon).into_iter().collect();
            p.push(Span::raw(fit(text, cap)));
            if let Some(n) = pct {
                p.push(Span::styled(format!(" ~{n}%"), th.fg(Hue::Made)));
            }
            p
        })
        .collect();
    let mut say = WHOLE;
    // the core always says the mode; a strip without it is a hand-made one (a test's) and the right of it gives nothing up
    for step in 0..=if s.mode.is_empty() { 0 } else { GIVE_UP.len() } {
        let (lead, right) = parts(th, s, say, iw.saturating_sub(4));
        let left = join(lead.into_iter().chain(pills.clone()).collect(), "  ");
        let right = join(right, "   ");
        let (lw, rw) = (width_of(&left), width_of(&right));
        if lw + rw + 2 <= iw || (lw == 0 || rw == 0) && lw + rw <= iw {
            let mut row = left;
            row.push(Span::raw(" ".repeat(iw.saturating_sub(lw + rw))));
            row.extend(right);
            return vec![row];
        }
        match GIVE_UP.get(step) {
            Some(f) => f(&mut say),
            None => break,
        }
    }
    // under each other: the tags that do not fit in a row of their own are left out, last first
    let (lead, right) = apart(th, s, iw);
    let lead = join(lead, "  ");
    let mut left = join(std::iter::once(lead.clone()).filter(|b| !b.is_empty()).chain(pills.clone()).collect(), "  ");
    let mut lw = width_of(&left);
    while lw > iw && pills.len() > 1 {
        pills.pop();
        left = join(std::iter::once(lead.clone()).filter(|b| !b.is_empty()).chain(pills.clone()).collect(), "  ");
        lw = width_of(&left);
    }
    let mut rows = Vec::new();
    if !left.is_empty() {
        rows.push(left);
    }
    if !right.is_empty() {
        rows.push(right);
    }
    rows
}

/// The strip whole, before it gives anything up.
const WHOLE: Say = Say { skills: true, mode: true, cost: 2, gauge: 2, line: true };

/// The parts of the strip when the left and the right have a row each: the right gives up what it must to fit as well, and a route longer
/// than the row is cut.
fn apart(th: &Theme, s: &Strip, iw: usize) -> (Vec<Vec<Span<'static>>>, Vec<Span<'static>>) {
    let (mut say, mut n) = (WHOLE, 0);
    loop {
        let (lead, right) = parts(th, s, say, iw.saturating_sub(4));
        let right = join(right, "   ");
        if width_of(&right) <= iw || n == GIVE_UP.len() || s.mode.is_empty() {
            return (lead, right);
        }
        GIVE_UP[n](&mut say);
        n += 1;
    }
}

/// The rows of the strip while the list of suggestions is up: the list in the left, the right group where it stood. From [`WIDE`] columns
/// the two share a row and the right gives up its parts by [`GIVE_UP`] until the list has half of it; narrower, or when they still do not
/// fit, the list has the first row whole and the right group the second. With no strip, the list's row alone.
fn list_rows(th: &Theme, s: Option<&Strip>, list: &List, iw: usize, width: usize) -> Vec<Vec<Span<'static>>> {
    let short = width < SHORT;
    let Some(s) = s else { return vec![suggest::row(th, list, iw, short)] };
    if width >= WIDE {
        let whole = width_of(&suggest::row(th, list, usize::MAX, short));
        let mut say = WHOLE;
        for step in 0..=if s.mode.is_empty() { 0 } else { GIVE_UP.len() } {
            let right = join(parts(th, s, say, iw.saturating_sub(4)).1, "   ");
            let rw = width_of(&right);
            let room = if rw == 0 { iw } else { iw.saturating_sub(rw + 2) };
            if whole <= room || room * 2 >= iw {
                let mut row = suggest::row(th, list, room, short);
                row.push(Span::raw(" ".repeat(iw.saturating_sub(width_of(&row) + rw))));
                row.extend(right);
                return vec![row];
            }
            match GIVE_UP.get(step) {
                Some(f) => f(&mut say),
                None => break,
            }
        }
    }
    let right = apart(th, s, iw).1;
    std::iter::once(suggest::row(th, list, iw, short)).chain((!right.is_empty()).then_some(right)).collect()
}

/// Where the rows of the line break: char ranges `(from, to)`, a break at a blank when there is one in reach, else in the word.
fn breaks(text: &[char], width: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let (mut start, mut used, mut space) = (0, 0, None);
    let mut i = 0;
    while i < text.len() {
        let c = text[i];
        if c == '\n' {
            out.push((start, i));
            (start, used, space) = (i + 1, 0, None);
            i += 1;
            continue;
        }
        let w = c.width().unwrap_or(0);
        // a row always takes its first letter, however wide it is
        if used > 0 && used + w > width {
            // a blank that is the letter over the edge ends the row and is not kept
            if c == ' ' {
                out.push((start, i));
                (start, used, space) = (i + 1, 0, None);
                i += 1;
                continue;
            }
            match space {
                Some(s) if s > start => {
                    out.push((start, s));
                    (start, used, space) = (s + 1, 0, None);
                    i = start;
                    continue;
                }
                _ => {
                    out.push((start, i));
                    (start, used, space) = (i, 0, None);
                    continue;
                }
            }
        }
        if c == ' ' {
            space = Some(i);
        }
        used += w;
        i += 1;
    }
    out.push((start, text.len()));
    out
}

fn lane_name(rows: &[Row], key: &str) -> Option<String> {
    rows.iter().find(|r| r.key == key).and_then(|r| r.cells.first()).map(|c| c.text.clone())
}

/// Who the line is for, at the right of its last row: the lane it steers, or the question it answers.
fn target(app: &App, th: &Theme, rows: &[Row]) -> Vec<Span<'static>> {
    let arrow = Span::styled(format!("{} ", th.glyph(Slot::Target)), th.tone(Tone::Dim));
    if let Some(target) = app.ask_target() {
        // what the binder would make of the line, if the core has said so for this very line; it names the question it would answer
        let guess = app.values.get(COMPOSER_BIND).filter(|b| !app.line.buf.trim().is_empty() && b.get("text").and_then(Json::as_str) == Some(app.line.buf.trim()));
        let a = guess.and_then(|b| b.get("ask").and_then(Json::as_str)).and_then(|id| app.asks.iter().find(|a| a.ask == id)).unwrap_or(target);
        let who = lane_name(rows, &a.asker).and_then(|n| n.split(' ').next().map(str::to_string)).unwrap_or_else(|| a.asker.clone());
        let mut s = vec![arrow, mark_sp(th, Slot::Ask), Span::styled(who, th.tone(Tone::Warn))];
        if let Some(b) = guess {
            let mark = b.get("mark").and_then(Json::as_str).and_then(|m| named(th, m));
            let label = b.get("label").and_then(Json::as_str).map(|l| Span::styled(l.to_string(), th.tone(Tone::Plain).add_modifier(Modifier::BOLD)));
            if mark.is_some() || label.is_some() {
                s.push(Span::raw(" "));
                s.extend(mark);
                s.extend(label);
                // a mark alone has no word after it to be a blank away from
                if let Some(last) = s.last_mut() {
                    last.content = last.content.trim_end().to_string().into();
                }
            }
        }
        return s;
    }
    match app.aimed().and_then(|k| lane_name(rows, &k)) {
        Some(name) => vec![arrow, Span::styled(name, th.tone(Tone::Accent))],
        None => Vec::new(),
    }
}

/// The composer drawn at `width` columns, with room for `rows` of the screen at most.
pub(crate) fn layout(app: &mut App, f: &Field, width: usize, room: usize) -> Composer {
    // the completion, made once a frame: the ghost after the cursor, and the list while it is up
    let (ghost, list) = app.suggest_now();
    let th = &app.theme;
    let iw = width.saturating_sub(4).max(1);
    let rows: &[Row] = app.rows.get(&f.bind).map_or(&[], Vec::as_slice);
    let focused = !app.wins.get(&f.bind).is_some_and(|w| w.at.is_some());
    let frame_style = if app.ask_target().is_some() {
        th.fg(Hue::Warn).add_modifier(Modifier::BOLD)
    } else if focused {
        th.fg(Hue::Accent).add_modifier(Modifier::BOLD)
    } else {
        th.fg(Hue::Edge)
    };
    let strip = strip_of(app.values.get(COMPOSER_STRIP));
    let strip_rows = match (&list, &strip) {
        (Some(l), s) => list_rows(th, s.as_ref(), l, iw, width),
        (None, Some(s)) => strip_rows(th, s, iw),
        (None, None) => Vec::new(),
    };
    let chars: Vec<char> = app.line.buf.chars().collect();
    // what is drawn of the line: the words after `:key` are masked, one cell for each, so the cursor and the rows stay where they are
    let mask = th.glyph(Slot::Mask);
    let drawn: Vec<String> = match crate::chat::held::key_tail(&app.line.buf) {
        Some(from) => chars.iter().enumerate().map(|(i, c)| if i >= from && !c.is_whitespace() && c.width() == Some(1) { mask.to_string() } else { c.to_string() }).collect(),
        None => chars.iter().map(char::to_string).collect(),
    };
    // a line the shaper looked at and needs nothing added to has a faint mark at the edge (a line of more than one row has its keys there)
    let multi = chars.contains(&'\n');
    let mut right = target(app, th, rows);
    // the words a step of J4n's own takes while it runs: in the empty line, or as the word the line starts; never over a question
    let step = if app.ask_target().is_none() { Hint::of(app.values.get(COMPOSER_STRIP).and_then(|v| v.get("skip"))) } else { None };
    if let Some(t) = step.as_ref().and_then(|h| hint::tail(th, h, &app.line.buf)) {
        if !right.is_empty() {
            right.push(Span::raw(" "));
        }
        right.extend(t);
    }
    if !multi && app.card() == Card::Pass {
        right.push(Span::styled(format!("{}{}", if right.is_empty() { "" } else { " " }, th.glyph(Slot::Yes)), th.tone(Tone::Dim)));
    }
    let hint = format!("{}{} line · {} send", th.glyph(Slot::KeyShift), th.glyph(Slot::KeyEnter), th.glyph(Slot::KeyEnter));
    let mut rs = breaks(&chars, iw);
    // a draft of more than one line has its keys at the end of the last
    // what stands at the right of the last row: the target, or when the line has more than one row, the keys; the last row gives it room
    let tail_w = if right.is_empty() { if multi { hint.width() } else { 0 } } else { width_of(&right) };
    let used = |rs: &[(usize, usize)]| rs.last().map_or(0, |(s, e)| chars[*s..*e].iter().map(|c| c.width().unwrap_or(0)).sum::<usize>());
    if tail_w > 0 && used(&rs) + tail_w + 2 > iw {
        // only the last line of the draft is cut shorter for it: the lines before it have the whole width
        let split = chars.iter().rposition(|c| *c == '\n').map_or(0, |i| i + 1);
        rs = if split == 0 { Vec::new() } else { breaks(&chars[..split - 1], iw) };
        rs.extend(breaks(&chars[split..], iw.saturating_sub(tail_w + 2)).into_iter().map(|(s, e)| (s + split, e + split)));
    }
    // frame, strip, rule, line: the line gives up rows when the screen is short
    let extra = if strip_rows.is_empty() { 0 } else { strip_rows.len() + 1 };
    let input_room = MAX_ROWS.min(room.saturating_sub(2 + extra).max(1));
    let cursor_at = app.line.at.min(chars.len());
    let crow = rs.iter().position(|(s, e)| *s <= cursor_at && cursor_at <= *e).unwrap_or(rs.len() - 1);
    let first = keep(0, crow as u64, crow as u64, input_room as u64) as usize;
    let shown = &rs[first..(first + input_room).min(rs.len())];
    let mut body: Vec<Vec<Span<'static>>> = strip_rows.iter().map(|r| [vec![Span::raw(" ")], r.clone()].concat()).collect();
    if !strip_rows.is_empty() {
        body.push(vec![Span::styled(th.glyph(Slot::RuleDashed).repeat(width.saturating_sub(2)), th.fg(Hue::Edge))]);
    }
    let placeholder = f.props.get("placeholder").and_then(Json::as_str).unwrap_or_default();
    let top = body.len() + 1;
    for (n, (s, e)) in shown.iter().enumerate() {
        let text: String = drawn[*s..*e].concat();
        let mut spans = vec![Span::raw(" ")];
        if let Some(h) = step.as_ref().filter(|_| chars.is_empty()) {
            spans.extend(hint::empty(th, h));
        } else if chars.is_empty() && !placeholder.is_empty() {
            spans.push(Span::styled(fit(placeholder, iw), th.fg(Hue::Faint)));
        } else {
            spans.push(Span::raw(text));
        }
        if n + 1 == shown.len() {
            let tail: Vec<Span<'static>> = if !right.is_empty() {
                right.clone()
            } else if multi {
                vec![Span::styled(hint.clone(), th.tone(Tone::Dim))]
            } else {
                Vec::new()
            };
            // the ghost: the rest of the best candidate, dim after the cursor (italic too where there is no colour), never typed
            if let Some(g) = ghost.as_deref() {
                let room = width.saturating_sub(2).saturating_sub(width_of(&spans) + width_of(&tail) + 2);
                let g: String = g.chars().scan(0, |w, c| {
                    *w += c.width().unwrap_or(0);
                    (*w <= room).then_some(c)
                }).collect();
                spans.push(Span::styled(g, if th.no_color { th.tone(Tone::Dim).add_modifier(Modifier::ITALIC) } else { th.fg(Hue::Faint) }));
            }
            let gap = (width.saturating_sub(2)).saturating_sub(width_of(&spans) + width_of(&tail) + 1);
            spans.push(Span::raw(" ".repeat(gap)));
            spans.extend(tail);
        }
        body.push(spans);
    }
    let lines = frame(th, body, width, "", frame_style, &BOX);
    let (s, _) = rs[crow];
    let before: usize = chars[s..cursor_at].iter().map(|c| c.width().unwrap_or(0)).sum();
    let row = top + crow.saturating_sub(first);
    Composer { cursor: ((2 + before).min(width.saturating_sub(2)) as u16, row as u16), lines }
}
