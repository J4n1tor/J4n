//! `table` (columns of a window of rows, sorted and found by the core) and `list` (rows from a view, with sections that fold).

use super::win::{finding, nav};
use super::{cell, column, group, padded, row, thumb, width_of};
use crate::app::App;
use super::layout::{self, Item, Laid, Look};
use crate::parts::{rows as pipe, scroll::keep};
use crate::theme::glyph::Slot;
use crate::theme::palette::Role as Hue;
use crate::wrap::fit;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use j4n_proto::{Field, Json, Row, Tone, Up};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::collections::{BTreeSet, HashSet};
use unicode_width::UnicodeWidthStr;

/// The widest a column grows by itself.
const COL_MAX: usize = 28;
/// The widest a column of a list grows, but the last, which takes what is left.
const LIST_COL_MAX: usize = 32;
/// Under this width the description column of a list goes, and shows only as the detail of the row the keys are on.
const LIST_NARROW: usize = 64;
/// The room before the first column of a list: a pipe of the look, two cells (`Look::stack`).
const LEAD: usize = 2;
/// The least the last column of a list is given, if it needs it, before the others are narrowed.
const LAST_MIN: usize = 10;
/// The least a column is narrowed to.
const COL_MIN: usize = 6;
/// How long a cell is for it to count as the start of a description, and not a word that happens to begin it.
const LIST_SAYS: usize = 12;

struct Col {
    name: String,
    title: String,
    right: bool,
    width: Option<usize>,
}

/// The columns of `props.columns`: a name, or `{name, title, right, width}`.
fn columns(f: &Field) -> Vec<Col> {
    let Some(list) = f.props.get("columns").and_then(Json::as_array) else { return Vec::new() };
    list.iter()
        .filter_map(|c| match c {
            Json::String(s) => Some(Col { name: s.clone(), title: s.clone(), right: false, width: None }),
            Json::Object(_) => {
                let name = c.get("name").and_then(Json::as_str)?.to_string();
                let title = c.get("title").and_then(Json::as_str).unwrap_or(&name).to_string();
                Some(Col { name, title, right: c.get("right").and_then(Json::as_bool).unwrap_or(false), width: c.get("width").and_then(Json::as_u64).map(|w| w as usize) })
            }
            _ => None,
        })
        .collect()
}

pub(crate) fn table(app: &mut App, f: &Field, area: Rect, fr: &mut Frame) {
    app.view_of(f);
    let sel = app.selected(f);
    let (_, _, total) = app.held(&f.bind);
    let cols = columns(f);
    let h = usize::from(area.height);
    let head = usize::from(cols.iter().any(|c| !c.title.is_empty()));
    let foot = usize::from(h >= 4);
    let body = h.saturating_sub(head + foot).max(1);
    let w = app.win(&f.bind);
    let top = keep(w.cur.top, sel, sel, body as u64);
    w.cur.top = top;
    let (sort, find, typing) = app.wins.get(&f.bind).map(|w| (w.sort.clone(), w.find.clone(), w.typing.clone())).unwrap_or_default();
    let th = &app.theme;
    let (rows, from, _) = app.held(&f.bind);
    let shown: Vec<Option<&Row>> = (top..(top + body as u64).min(total)).map(|i| i.checked_sub(from).and_then(|k| rows.get(k as usize))).collect();
    let cell_w = |c: &j4n_proto::Cell| c.text.width() + if c.icon.is_empty() { 0 } else { 2 };
    let sorted = sort.trim_start_matches('-');
    let mut widths: Vec<usize> = (0..cols.len().max(rows.iter().map(|r| r.cells.len()).max().unwrap_or(0)))
        .map(|i| {
            let natural = cols.get(i).map_or(0, |c| c.title.width() + if c.name == sorted { 2 } else { 0 });
            let cells = shown.iter().flatten().filter_map(|r| r.cells.get(i)).map(cell_w).max().unwrap_or(0);
            cols.get(i).and_then(|c| c.width).unwrap_or(natural.max(cells).min(COL_MAX))
        })
        .collect();
    let avail = usize::from(area.width).saturating_sub(4);
    while widths.len() > 1 && widths.iter().sum::<usize>() + 2 * (widths.len() - 1) > avail {
        widths.pop();
    }
    if let Some(first) = widths.first_mut() {
        *first = (*first).min(avail.saturating_sub(0).max(1));
    }
    let cols_n = widths.len();
    let w = usize::from(area.width);
    let mut lines: Vec<Line<'static>> = Vec::new();
    if head == 1 {
        let mut spans = vec![Span::raw("  ")];
        for (i, wd) in widths.iter().enumerate() {
            let c = cols.get(i);
            let title = c.map_or("", |c| c.title.as_str());
            let mark = if c.is_some_and(|c| c.name == sorted) { format!(" {}", th.glyph(if sort.starts_with('-') { Slot::SortDown } else { Slot::SortUp })) } else { String::new() };
            let right = c.is_some_and(|c| c.right);
            spans.push(column(&format!("{title}{mark}"), *wd, right, th.tone(Tone::Dim)));
            spans.push(Span::raw("  "));
        }
        lines.push(Line::from(padded(spans, w)));
    }
    let bar = thumb(body, top, body, total);
    for (k, r) in shown.iter().enumerate() {
        let on = top + k as u64 == sel;
        let mut spans = vec![if on { Span::styled(format!("{} ", th.glyph(Slot::Focus)), th.fg(Hue::Accent)) } else { Span::raw("  ") }];
        match r {
            Some(r) => {
                for (i, wd) in widths.iter().enumerate() {
                    let right = cols.get(i).is_some_and(|c| c.right);
                    match r.cells.get(i) {
                        Some(c) => {
                            let mut parts = cell(th, c, r.tone);
                            let used = width_of(&parts);
                            if used > *wd {
                                let text = fit(&parts.iter().map(|p| p.content.to_string()).collect::<String>(), *wd);
                                parts = vec![Span::styled(text, parts.last().map(|p| p.style).unwrap_or_default())];
                            }
                            let gap = wd.saturating_sub(width_of(&parts));
                            if right {
                                spans.push(Span::raw(" ".repeat(gap)));
                            }
                            spans.extend(parts);
                            if !right {
                                spans.push(Span::raw(" ".repeat(gap)));
                            }
                        }
                        None => spans.push(Span::raw(" ".repeat(*wd))),
                    }
                    spans.push(Span::raw("  "));
                }
            }
            None => spans.push(Span::styled("…", th.tone(Tone::Dim))),
        }
        let mut spans = padded(spans, w.saturating_sub(1));
        spans.push(if bar[k] { Span::styled(th.glyph(Slot::Thumb), th.fg(Slot::Thumb.role())) } else { Span::raw(" ") });
        let l = Line::from(spans);
        lines.push(if on { l.style(th.back(Hue::Sel)) } else { l });
    }
    if foot == 1 {
        let dim = th.tone(Tone::Dim);
        let mut spans = vec![Span::styled(format!(" {} of {}", group(shown.iter().flatten().count() as u64), group(total)), dim)];
        if let Some(t) = &typing {
            spans = vec![Span::styled(" /", th.fg(Hue::Accent)), Span::raw(format!(" {}", t.buf)), Span::styled(th.glyph(Slot::Caret), th.fg(Hue::Accent))];
        } else if !find.is_empty() {
            spans.push(Span::styled(" · /", dim));
            spans.push(Span::styled(format!(" {find}"), th.fg(Hue::Accent)));
            spans.push(Span::styled(" · esc clear", dim));
        } else if w >= 40 && cols_n > 0 {
            spans.push(Span::styled(" · s sort · / find", dim));
        }
        lines.resize_with(h - 1, Line::default);
        lines.push(Line::from(spans));
    }
    fr.render_widget(Paragraph::new(lines), area);
}

/// The next column to sort by after `now` (`-name` or `name`), or none after the last: the direction stays.
fn next_sort(f: &Field, now: &str) -> String {
    let names: Vec<String> = columns(f).into_iter().map(|c| c.name).collect();
    let desc = now.starts_with('-');
    let at = names.iter().position(|n| n == now.trim_start_matches('-'));
    match (now.is_empty(), at) {
        (true, _) => names.first().cloned().unwrap_or_default(),
        (false, Some(i)) => names.get(i + 1).map(|n| if desc { format!("-{n}") } else { n.clone() }).unwrap_or_default(),
        (false, None) => String::new(),
    }
}

pub(crate) fn key(app: &mut App, f: &Field, k: KeyEvent) -> Option<Vec<Up>> {
    app.view_of(f);
    if let Some(ups) = finding(app, f, k) {
        return Some(ups);
    }
    if nav(app, f, k) {
        return Some(Vec::new());
    }
    if k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
        return None;
    }
    let w = app.win(&f.bind);
    match k.code {
        KeyCode::Char('s') => {
            let to = next_sort(f, &w.sort);
            let w = app.win(&f.bind);
            w.sort = to;
            w.rewind();
            Some(Vec::new())
        }
        KeyCode::Char('S') if !w.sort.is_empty() => {
            w.sort = match w.sort.strip_prefix('-') {
                Some(rest) => rest.to_string(),
                None => format!("-{}", w.sort),
            };
            w.rewind();
            Some(Vec::new())
        }
        KeyCode::Enter => act(app, f, None),
        KeyCode::Char(c) => act(app, f, Some(c)),
        _ => None,
    }
}

/// What the selected row does for this key (`⏎` when none): its action with that key, if it has one.
pub(crate) fn act(app: &mut App, f: &Field, key: Option<char>) -> Option<Vec<Up>> {
    let at = app.selected(f);
    let act = app.row_at(&f.bind, at)?.actions.iter().find(|a| key.is_none_or(|c| a.key == c.to_string())).cloned()?;
    Some(app.run_act(act))
}

// ---- list ----

/// The places of the rows that are in sight: those whose section is not shut.
pub fn visible(rows: &[Row], shut: &BTreeSet<String>) -> Vec<usize> {
    (0..rows.len()).filter(|i| rows[*i].parent.as_ref().is_none_or(|p| !shut.contains(p))).collect()
}

/// The places of the rows in sight: those whose section is not shut, and while the list is found in (`/`) those that hold the
/// words, with the heads of their sections.
pub fn shown(app: &App, bind: &str) -> Vec<usize> {
    let rows = app.rows.get(bind).map_or(&[][..], Vec::as_slice);
    let (shut, find) = app.wins.get(bind).map(|w| (w.flip.clone(), w.find.to_lowercase())).unwrap_or_default();
    let seen = visible(rows, &shut);
    if find.is_empty() {
        return seen;
    }
    let hit = |r: &Row| r.key.to_lowercase().contains(&find) || r.cells.iter().any(|c| c.text.to_lowercase().contains(&find));
    let held = |i: usize| rows.iter().any(|r| r.parent.as_deref() == Some(rows[i].key.as_str()) && hit(r));
    seen.into_iter().filter(|i| hit(&rows[*i]) || is_header(rows, *i) && held(*i)).collect()
}

/// The place of each row in sight on the base: a run of rows of one parent, not cut by a section's head, is a stack, and the end of the run is the elbow. A head is drawn
/// without a pipe and ends the run above it; its place is not used.
fn laid(rows: &[Row], seen: &[usize]) -> Vec<Laid> {
    let heads: HashSet<&str> = rows.iter().filter_map(|r| r.parent.as_deref()).collect();
    let head = |i: usize| heads.contains(rows[i].key.as_str());
    let mut out: Vec<Laid> = Vec::with_capacity(seen.len());
    let mut at = 0;
    while at < seen.len() {
        let mut end = at + 1;
        while !head(seen[at]) && end < seen.len() && !head(seen[end]) && rows[seen[end]].parent == rows[seen[at]].parent {
            end += 1;
        }
        let items: Vec<Item> = seen[at..end].iter().map(|i| Item::new(&rows[*i].key, None)).collect();
        out.extend(layout::lay(&items));
        at = end;
    }
    out
}

/// A row as a list draws it: a switch has its mark in its first cell, and a setting changed since the start that acts from the next one
/// says so after what it is now.
fn marked(th: &crate::theme::Theme, r: &Row) -> Row {
    let Some(e) = r.data.as_ref().and_then(|d| d.get("edit")) else { return r.clone() };
    let mut r = r.clone();
    // a row that is no switch has a blank where a switch has its mark, so that the names stand in one column
    if let Some(c) = r.cells.first_mut() {
        match e.get("kind").and_then(Json::as_str) {
            Some("bool") => c.icon = if e.get("value").and_then(Json::as_bool).unwrap_or(false) { "bool-on" } else { "bool-off" }.into(),
            _ => c.text = format!("{}{}", " ".repeat(super::mark_sp(th, Slot::BoolOff).content.width()), c.text),
        }
    }
    if e.get("waits").and_then(Json::as_bool).unwrap_or(false)
        && let Some(c) = r.cells.get_mut(1)
    {
        c.text = format!("{} {}", c.text, crate::theme::glyph::waits(th.tier));
        c.tone = Tone::Warn;
    }
    r
}

/// Whether some row hangs from this one.
pub fn is_header(rows: &[Row], at: usize) -> bool {
    rows.iter().any(|r| r.parent.as_deref() == Some(rows[at].key.as_str()))
}

/// The text with its blanks collapsed and a mark of a cut taken off the end, to tell whether one line is the short of another.
fn gist(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").trim_end_matches('…').trim_end().to_string()
}

/// Whether cell `k` of the row says again what a line of its detail says in full (the short description of a setting, and then the
/// description): it is the same words, or the start of them.
fn restated(r: &Row, k: usize) -> bool {
    let Some(c) = r.cells.get(k) else { return false };
    let me = gist(&c.text);
    let at_least = me.chars().count() >= LIST_SAYS;
    !me.is_empty() && r.detail.iter().any(|d| gist(&d.text) == me || at_least && gist(&d.text).starts_with(&me))
}

/// A row that says one thing after its title and nothing between: a section's head with its hint.
fn lone(r: &Row) -> bool {
    r.cells.len() > 2 && r.cells[1..r.cells.len() - 1].iter().all(|c| c.text.trim().is_empty()) && r.cells.last().is_some_and(|c| !c.text.trim().is_empty())
}

/// The widths of the columns of a list in `room` cells: the columns before the last are as wide as what they hold, at most
/// [`LIST_COL_MAX`] — unless the last needs less than it would be given, and then they take the difference, up to what they hold — and
/// the last takes what is left; when it would have too little the widest of the others give up cells.
fn shared(natural: &[usize], room: usize) -> Vec<usize> {
    let Some((&last, fixed)) = natural.split_last() else { return Vec::new() };
    let mut widths: Vec<usize> = fixed.iter().map(|w| (*w).min(LIST_COL_MAX)).collect();
    let gaps = 2 * fixed.len();
    let mut spare = room.saturating_sub(widths.iter().sum::<usize>() + gaps + last);
    for (w, n) in widths.iter_mut().zip(fixed) {
        let more = spare.min(n.saturating_sub(*w));
        *w += more;
        spare -= more;
    }
    let want = last.min(LAST_MIN);
    while room < widths.iter().sum::<usize>() + gaps + want {
        let Some((at, w)) = widths.iter().copied().enumerate().max_by_key(|(_, w)| *w).filter(|(_, w)| *w > COL_MIN) else { break };
        widths[at] = w - 1;
    }
    let rest = room.saturating_sub(widths.iter().sum::<usize>() + gaps).max(1);
    widths.push(rest);
    widths
}

/// The lines of a list: the titles, the rows and the detail of the one the keys are on; and where that row stands among the lines.
struct Drawn {
    head: Option<Line<'static>>,
    lines: Vec<Line<'static>>,
    /// The line of the row the keys are on, and the last line of its detail.
    at: (usize, usize),
}

fn drawn(app: &App, f: &Field, w: usize) -> Drawn {
    let bind = f.bind.as_str();
    let th = &app.theme;
    let rows: Vec<Row> = app.rows.get(bind).map_or(&[][..], Vec::as_slice).iter().map(|r| marked(th, r)).collect();
    let rows = rows.as_slice();
    let sel = app.place(bind);
    let shut = app.wins.get(bind).map(|w| w.flip.clone()).unwrap_or_default();
    let seen = shown(app, bind);
    let laid = laid(rows, &seen);
    let titles: Vec<&str> = f.props.get("titles").and_then(Json::as_array).map(|a| a.iter().filter_map(Json::as_str).collect()).unwrap_or_default();
    // the cells of each column line up under one another, whatever the section
    let cell_w = |c: &j4n_proto::Cell| c.text.width() + if c.icon.is_empty() { 0 } else { 2 };
    let plain: Vec<usize> = seen.iter().copied().filter(|i| !is_header(rows, *i)).collect();
    let mut cols = plain.iter().map(|i| rows[*i].cells.len()).max().unwrap_or(0);
    // narrow, the description goes: it shows only as the detail of the row the keys are on
    if w < LIST_NARROW && cols > 1 && plain.iter().any(|i| restated(&rows[*i], cols - 1)) {
        cols -= 1;
    }
    // a row that says one thing after its title (a head with its hint) stands beside its title and sizes no column
    let natural: Vec<usize> = (0..cols).map(|c| plain.iter().filter(|i| !lone(&rows[**i])).filter_map(|i| rows[*i].cells.get(c)).map(cell_w).max().unwrap_or(0).max(titles.get(c).map_or(0, |t| t.width()))).collect();
    let widths = shared(&natural, w.saturating_sub(LEAD));
    let head = titles.iter().any(|t| !t.is_empty()).then(|| {
        let mut spans = vec![Span::raw(" ".repeat(LEAD))];
        for (k, wd) in widths.iter().enumerate() {
            if k > 0 {
                spans.push(Span::raw("  "));
            }
            spans.push(column(titles.get(k).copied().unwrap_or(""), *wd, false, th.fg(Hue::Dim)));
        }
        row(th, spans, w, false)
    });
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut at = (0, 0);
    for (pos, &i) in seen.iter().enumerate() {
        let r = &rows[i];
        let on = i == sel;
        if on {
            at = (lines.len(), lines.len());
        }
        if is_header(rows, i) {
            let (fold, _) = th.mark(if shut.contains(&r.key) { Slot::FoldClosed } else { Slot::FoldOpen });
            let n = r.cells.get(1).map(|c| c.text.clone()).unwrap_or_else(|| rows.iter().filter(|x| x.parent.as_deref() == Some(r.key.as_str())).count().to_string());
            let title = r.cells.first().map(|c| c.text.clone()).unwrap_or_default();
            let spans = vec![Span::styled(format!("{fold} "), th.fg(Hue::Faint)), Span::styled(title, if on { th.fg(Hue::Accent) } else { Style::default() }), Span::styled(format!("  {n}"), th.tone(Tone::Dim))];
            lines.push(row(th, spans, w, on));
            continue;
        }
        let (place, last) = (&laid[pos], laid[pos].last);
        // the row being changed is its editor, in its place: the field of its kind, being edited
        if on && let Some(ed) = app.row_lines(bind, r, w, last) {
            lines.extend(ed);
            at.1 = lines.len() - 1;
            continue;
        }
        // the elbow stays whatever is selected: the selection is the band behind the row and the colour of its mark
        let mut spans = vec![pipe::place(th, place, on, false)];
        // the hint of a key other than ⏎ stands at the right end of the row the keys are on; ⏎ itself is in the keys row
        let hint = r.actions.first().filter(|a| on && a.key != "enter").map(|a| format!("{} {}", th.glyph(Slot::KeyEnter), a.label));
        for (k, wd) in widths.iter().enumerate() {
            if k > 0 {
                spans.push(Span::raw("  "));
            }
            if k == 1 && lone(r) {
                let room = w.saturating_sub(width_of(&spans) + 1);
                spans.extend(r.cells.last().map(|c| cell(th, &j4n_proto::Cell { text: fit(&c.text, room.saturating_sub(if c.icon.is_empty() { 0 } else { 2 })), ..c.clone() }, Tone::Dim)).unwrap_or_default());
                break;
            }
            // one description at a time: while the detail is open the short one is not said
            let mut parts = if on && k > 0 && restated(r, k) { Vec::new() } else { r.cells.get(k).map(|c| cell(th, c, if k == 0 { r.tone } else { Tone::Dim })).unwrap_or_default() };
            if on && k == 0 && r.tone == Tone::Plain && let Some(text) = parts.last_mut() {
                text.style = th.fg(Hue::Accent).add_modifier(Modifier::BOLD);
            }
            // a description is cut to its cell: the marks of its markdown are not part of the words
            if let Some(text) = parts.last_mut().filter(|p| p.content.contains(['`', '*'])) {
                text.content = crate::read::flat(th, &text.content).into();
            }
            let room = if k + 1 == widths.len() { wd.saturating_sub(hint.as_ref().map_or(0, |h| h.width() + 3)).max(1) } else { *wd };
            if width_of(&parts) > room {
                let text = fit(&parts.iter().map(|p| p.content.to_string()).collect::<String>(), room);
                parts = vec![Span::styled(text, parts.last().map(|p| p.style).unwrap_or_default())];
            }
            let pad = if k + 1 == widths.len() { 0 } else { wd.saturating_sub(width_of(&parts)) };
            spans.extend(parts);
            spans.push(Span::raw(" ".repeat(pad)));
        }
        // a hint that does not fit whole is left out: a word cut in two ("⏎ abo") says nothing; the keys row and the sheet say the key
        if let Some(hint) = hint.filter(|h| width_of(&spans) + h.width() + 3 <= w) {
            let gap = w.saturating_sub(width_of(&spans) + hint.width() + 1);
            spans.push(Span::raw(" ".repeat(gap.max(2))));
            spans.push(Span::styled(hint, th.fg(Hue::Accent)));
        }
        lines.push(row(th, spans, w, on));
        if on {
            // under the last row the detail hangs from a blank, not from the bar
            let trunk = Look::new(th).stack(place).1;
            // a description is read whole: a line that is longer than the window goes on the next line
            for d in &r.detail {
                for part in crate::read::spans(th, &app.read, th.tone(d.tone), &d.text, w.saturating_sub(LEAD + 3).max(1)) {
                    lines.push(Line::from([vec![Span::styled(format!("{trunk}  "), th.fg(Hue::Edge))], part].concat()));
                }
            }
            at.1 = lines.len() - 1;
        }
    }
    if let Some(e) = &app.edit {
        lines.push(Line::from(vec![Span::styled(format!("{}: ", e.field), th.tone(Tone::Accent)), Span::raw(e.line.buf.clone()), Span::styled(th.glyph(Slot::Caret), th.tone(Tone::Dim))]));
    }
    Drawn { head, lines, at }
}

/// The rows of a view, in sections where some rows hang from others: the section's title and how many it holds, and under it the rows on
/// pipes, with the titles of the columns over them. The row the keys are on has its band, and its detail under it; the lines scroll so
/// that the row and all its detail are in sight.
pub fn list(app: &mut App, f: &Field, area: Rect, fr: &mut Frame) {
    let seen = shown(app, &f.bind);
    let w = app.win(&f.bind);
    if let Some(first) = seen.first().filter(|_| !seen.contains(&(w.cur.at as usize))) {
        w.cur.at = *first as u64;
    }
    let (find, typing) = (w.find.clone(), w.typing.clone());
    let foot = (typing.is_some() || !find.is_empty()) && area.height > 3;
    let Drawn { head, lines, at } = drawn(app, f, usize::from(area.width));
    let head = head.filter(|_| area.height > 2);
    let take = usize::from(area.height) - usize::from(head.is_some()) - usize::from(foot);
    let max = lines.len().saturating_sub(take);
    let w = app.win(&f.bind);
    let top = keep(w.cur.top.min(max as u64), at.0 as u64, at.1 as u64, take as u64);
    w.cur.top = top;
    let top = top as usize;
    let mut out: Vec<Line<'static>> = head.into_iter().collect();
    out.extend(lines.into_iter().skip(top).take(take));
    if foot {
        let th = &app.theme;
        let dim = th.tone(Tone::Dim);
        let spans = match &typing {
            Some(t) => vec![Span::styled(" /", th.fg(Hue::Accent)), Span::raw(format!(" {}", t.buf)), Span::styled(th.glyph(Slot::Caret), th.fg(Hue::Accent))],
            None => vec![Span::styled(format!(" {} found · /", group(seen.len() as u64)), dim), Span::styled(format!(" {find}"), th.fg(Hue::Accent)), Span::styled(" · esc clear", dim)],
        };
        out.resize_with(usize::from(area.height) - 1, Line::default);
        out.push(Line::from(spans));
    }
    fr.render_widget(Paragraph::new(out), area);
}
