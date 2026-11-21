//! `log`: the rows of a binding as time, mark, text and meta, one line each, a row that has detail opening under itself. The core
//! holds all of them (a million, or more); the screen holds the window around the row the keys are on, and follows the end while the
//! operator has not moved. A row that carries its event (`Row::data`, the Log's) is drawn in the look of [`event`]: a line, and
//! opened, the event as a tree or (`r`) its raw JSON.
//! A row that stands for a run of the same line seen again (the core folds them) has `×n` as its fourth cell, shown before the task.

use super::event::{self, Spans};
use super::table::act;
use super::win::{finding, nav};
use super::{cell, group, padded, thumb, width_of};
use crate::app::App;
use crate::parts::{rows::lead, scroll::keep_by};
use crate::theme::glyph::Slot;
use crate::theme::palette::Role as Hue;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use j4n_proto::{Field, Row, Tone, Up};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

/// How many levels of a row's event are open (0: shut), and whether it shows its raw JSON.
fn opened(app: &App, bind: &str, r: &Row) -> (u8, bool) {
    let Some(w) = app.wins.get(bind) else { return (0, false) };
    let raw = w.raw.contains(&r.key);
    (w.depth.get(&r.key).copied().unwrap_or(if w.all { u8::MAX } else { 0 }).max(u8::from(raw)), raw)
}

/// The lines under an open row: its event as a tree or as raw JSON, or its detail; none when it is shut.
fn under_of(app: &App, bind: &str, r: &Row, width: usize) -> Vec<Spans> {
    let th = &app.theme;
    let Some(ev) = &r.data else {
        if !app.wins.get(bind).is_some_and(|w| w.flip.contains(&r.key)) {
            return Vec::new();
        }
        let elbow = |n: usize| if n == 0 { lead(th, true, false, false) } else { Span::styled("  ", th.fg(Hue::Edge)) };
        return r
            .detail
            .iter()
            .flat_map(|d| crate::read::spans(th, &app.read, th.tone(d.tone), &d.text, width.saturating_sub(4)).into_iter().enumerate())
            .map(|(n, mut line)| {
                line.insert(0, elbow(n));
                line
            })
            .collect();
    };
    match opened(app, bind, r) {
        (0, _) => Vec::new(),
        (_, true) => event::raw(ev).lines().flat_map(|l| crate::wrap::wrap(l, width.saturating_sub(2))).map(|l| vec![Span::styled(format!("{} ", th.glyph(Slot::Quote)), th.fg(Hue::Edge)), Span::styled(l, th.fg(Hue::Dim))]).collect(),
        (depth, false) => event::tree(th, &app.read, &app.events, ev, depth, width),
    }
}

/// The lines a row takes: its own, and what is under it while it is open.
fn lines_of(app: &App, bind: &str, r: &Row, width: usize) -> usize {
    1 + under_of(app, bind, r, width).len()
}

pub fn log(app: &mut App, f: &Field, area: Rect, fr: &mut Frame) {
    // rows asked for and not come: the sign once the wait is long enough to see, and no foot, which would count rows there are not yet
    if let Some(wait) = app.rows_wait(&f.bind) {
        if let Some(line) = app.wait_line(wait, "", usize::from(area.width)) {
            fr.render_widget(Paragraph::new(line), area);
        }
        return;
    }
    let follow = app.follows(f);
    let sel = app.selected(f);
    let (sort_find, moved, typing) = app.wins.get(&f.bind).map(|w| (w.find.clone(), w.moved, w.typing.clone())).unwrap_or_default();
    let foot = usize::from(typing.is_some() || !sort_find.is_empty() || moved);
    let body = usize::from(area.height).saturating_sub(foot).max(1);
    let inner = usize::from(area.width).saturating_sub(3);
    let height = |app: &App, at: u64| app.row_at(&f.bind, at).map_or(1, |r| lines_of(app, &f.bind, r, inner));
    let total = app.held(&f.bind).2;
    // the first row shown, and how many of its lines are above the screen: the end follows (the first row may show only its end),
    // else the row the keys are on stays in sight, and one taller than the screen shows from where the keys have scrolled it to
    let (top, skip) = if follow {
        let (mut top, mut used) = (total, 0);
        while top > 0 && used < body {
            top -= 1;
            used += height(app, top);
        }
        (top, used.saturating_sub(body))
    } else {
        let top = keep_by(app.win(&f.bind).cur.top, sel, body, |i| height(app, i));
        let over = if top == sel { height(app, sel).saturating_sub(body) } else { 0 };
        (top, app.win(&f.bind).skip.min(over))
    };
    let w = app.win(&f.bind);
    (w.cur.top, w.skip, w.seen) = (top, if follow { 0 } else { skip }, (inner, body));
    let th = &app.theme;
    let (rows, from, _) = app.held(&f.bind);
    let w = usize::from(area.width);
    let time_w = rows.iter().filter_map(|r| r.cells.first()).map(|c| c.text.width()).max().unwrap_or(0);
    let kind_w = rows.iter().filter_map(|r| r.data.as_ref()?["kind"].as_str().map(UnicodeWidthStr::width)).max().unwrap_or(0).min(18);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut at = top;
    let mut shown = 0;
    while lines.len() < body + skip && at < total {
        let Some(r) = at.checked_sub(from).and_then(|i| rows.get(i as usize)) else {
            lines.push(Line::styled("  …", th.tone(Tone::Dim)));
            at += 1;
            continue;
        };
        let on = at == sel;
        let under = under_of(app, &f.bind, r, inner);
        let th = &app.theme;
        let mut spans = vec![if on { Span::styled(format!("{} ", th.glyph(Slot::Focus)), th.fg(Hue::Accent)) } else { Span::raw("  ") }];
        let mut tail = Vec::new();
        // a run of the same line: the row is its first, and `×n` is its fourth cell (the core folds)
        if let Some(n) = r.cells.get(3) {
            tail.push(Span::styled(format!("{} ", n.text), th.tone(Tone::Dim)));
        }
        match &r.data {
            Some(ev) => {
                if let Some(task) = ev["task"].as_str() {
                    tail.push(Span::styled(task.to_string(), th.tone(Tone::Dim)));
                }
            }
            None => {
                if let Some(t) = r.cells.first() {
                    spans.push(Span::styled(format!("{:<time_w$} ", t.text), th.tone(Tone::Dim)));
                }
                if let Some(c) = r.cells.get(1) {
                    spans.extend(cell(th, c, r.tone));
                }
                if let Some(m) = r.cells.get(2) {
                    tail.extend(cell(th, m, Tone::Dim));
                }
            }
        }
        if r.data.is_some() || !r.detail.is_empty() {
            tail.push(Span::styled(format!(" {}", th.glyph(if under.is_empty() { Slot::FoldClosed } else { Slot::FoldOpen })), th.fg(Hue::Faint)));
        }
        if let Some(ev) = &r.data {
            let room = w.saturating_sub(width_of(&spans) + width_of(&tail) + 3);
            spans.extend(event::line(th, &app.events, ev, kind_w, room));
        }
        let gap = w.saturating_sub(width_of(&spans) + width_of(&tail) + 2);
        spans.push(Span::raw(" ".repeat(gap.max(1))));
        spans.extend(tail);
        lines.push(Line::from(padded(spans, w)).style(if on { th.back(Hue::Sel) } else { Default::default() }));
        for l in under {
            lines.push(Line::from(std::iter::once(Span::raw("  ")).chain(l).collect::<Vec<_>>()));
        }
        at += 1;
        shown += 1;
    }
    lines.drain(..skip.min(lines.len()));
    lines.truncate(body);
    // rows that came and are none say so; a find that matches none keeps its foot under the words
    if total == 0 && app.rows.contains_key(&f.bind) {
        lines.push(Line::styled(crate::view::empty(app, f), app.theme.tone(Tone::Dim)));
    }
    let th = &app.theme;
    let bar = thumb(body, top, shown.max(1), total);
    for (l, on) in lines.iter_mut().zip(bar) {
        if on && w > 2 {
            let used = width_of(&l.spans);
            if used < w {
                l.spans.push(Span::raw(" ".repeat(w - used - 1)));
            } else {
                // the thumb takes the last cell of a full line
                if let Some(last) = l.spans.last_mut() {
                    last.content = last.content.chars().take(last.content.chars().count().saturating_sub(1)).collect::<String>().into();
                }
            }
            l.spans.push(Span::styled(th.glyph(Slot::Thumb), th.fg(Slot::Thumb.role())));
        }
    }
    if foot == 1 {
        let dim = th.tone(Tone::Dim);
        let line = match &typing {
            Some(t) => Line::from(vec![Span::styled(" /", th.fg(Hue::Accent)), Span::raw(format!(" {}", t.buf)), Span::styled(th.glyph(Slot::Caret), th.fg(Hue::Accent))]),
            None => {
                let mut spans = vec![Span::styled(format!(" {} of {}", group(sel + 1), group(total)), dim)];
                if !sort_find.is_empty() {
                    spans.push(Span::styled(" · /", dim));
                    spans.push(Span::styled(format!(" {sort_find}"), th.fg(Hue::Accent)));
                }
                if w >= 48 {
                    spans.push(Span::styled(if sort_find.is_empty() { " · G follow · / find" } else { " · G follow · esc clear" }, dim));
                } else {
                    spans.push(Span::styled(" · G follow", dim));
                }
                Line::from(spans)
            }
        };
        lines.resize_with(body, Line::default);
        lines.push(line);
    }
    fr.render_widget(Paragraph::new(lines), area);
}

pub(crate) fn key(app: &mut App, f: &Field, k: KeyEvent) -> Option<Vec<Up>> {
    if let Some(ups) = finding(app, f, k) {
        return Some(ups);
    }
    if inside(app, f, k) {
        return Some(Vec::new());
    }
    if nav(app, f, k) {
        return Some(Vec::new());
    }
    if k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
        return None;
    }
    let sel = app.selected(f);
    if let Some(ev) = app.row_at(&f.bind, sel).and_then(|r| r.data.clone().map(|d| (r.key.clone(), d))) {
        return event_key(app, f, k, ev);
    }
    match k.code {
        KeyCode::Enter | KeyCode::Char(' ') => {
            let r = app.row_at(&f.bind, sel)?;
            if r.detail.is_empty() {
                return act(app, f, None).or(Some(Vec::new()));
            }
            let key = r.key.clone();
            super::toggle(&mut app.win(&f.bind).flip, key);
            Some(Vec::new())
        }
        KeyCode::Char(c) => act(app, f, Some(c)),
        _ => None,
    }
}

/// `↑↓ PgUp PgDn` scroll through the row the keys are on while it is open and taller than the screen, before they move to another row.
fn inside(app: &mut App, f: &Field, k: KeyEvent) -> bool {
    if !k.modifiers.is_empty() || app.follows(f) {
        return false;
    }
    let Some((inner, body)) = app.wins.get(&f.bind).map(|w| w.seen).filter(|s| s.1 > 0) else { return false };
    let Some(r) = app.row_at(&f.bind, app.selected(f)) else { return false };
    let over = lines_of(app, &f.bind, r, inner).saturating_sub(body);
    let w = app.win(&f.bind);
    let to = match k.code {
        KeyCode::Down if w.skip < over => w.skip + 1,
        KeyCode::PageDown if w.skip < over => (w.skip + body).min(over),
        KeyCode::Up if w.skip > 0 => w.skip - 1,
        KeyCode::PageUp if w.skip > 0 => w.skip.saturating_sub(body),
        _ => return false,
    };
    w.skip = to;
    true
}

/// `kind:<word>` or `task:<word>` put at the start of the find, or taken off it when it is there (`k`, `t`).
fn only(find: &str, field: &str, word: &str) -> String {
    let had = find.split_whitespace().any(|w| w.starts_with(field));
    let rest: Vec<&str> = find.split_whitespace().filter(|w| !w.starts_with(field)).collect();
    if had || word.is_empty() { rest.join(" ") } else { std::iter::once(format!("{field}{word}").as_str()).chain(rest).collect::<Vec<_>>().join(" ") }
}

/// The keys of a row that carries its event: open and shut it whole or a level at a time, all of them, only
/// its kind or task, its raw JSON, a copy of it, its cause. Any other letter is the row's action, as on any row.
fn event_key(app: &mut App, f: &Field, k: KeyEvent, (key, ev): (String, j4n_proto::Json)) -> Option<Vec<Up>> {
    let deepest = event::deepest(&ev);
    let r = app.row_at(&f.bind, app.selected(f)).cloned()?;
    let (depth, _) = opened(app, &f.bind, &r);
    let depth = depth.min(deepest);
    let w = app.win(&f.bind);
    let refind = |w: &mut super::Win, find: String| {
        w.find = find;
        w.rewind();
    };
    match k.code {
        KeyCode::Enter | KeyCode::Char(' ') => {
            w.raw.remove(&key);
            w.depth.insert(key, if depth > 0 { 0 } else { u8::MAX });
        }
        KeyCode::Right => {
            w.depth.insert(key, (depth + 1).min(deepest));
        }
        KeyCode::Left => {
            w.raw.remove(&key);
            w.depth.insert(key, depth.saturating_sub(1));
        }
        KeyCode::Char('z') => {
            w.all = !w.all && w.depth.values().all(|d| *d == 0);
            w.depth.clear();
            w.raw.clear();
        }
        KeyCode::Char('r') => {
            super::toggle(&mut w.raw, key);
        }
        KeyCode::Char('k') => refind(w, only(&w.find.clone(), "kind:", ev["kind"].as_str().unwrap_or(""))),
        KeyCode::Char('t') => refind(w, only(&w.find.clone(), "task:", ev["task"].as_str().unwrap_or(""))),
        KeyCode::Char('y') => {
            w.copy = Some(event::raw(&ev));
            app.toast = Some((format!("copied event {}", ev["seq"]), Tone::Dim));
        }
        KeyCode::Char('c') => {
            let Some(cause) = ev["cause"].as_u64() else {
                app.toast = Some(("no cause".into(), Tone::Dim));
                return Some(Vec::new());
            };
            if !w.find.is_empty() || !w.sort.is_empty() {
                // the cause is found among all the events in the order they were written, once their rows are here
                (w.find, w.sort) = (String::new(), String::new());
                (w.dirty, w.stale) = (true, true);
            }
            (w.goto, w.span) = (Some(cause), None);
            app.go_on(f);
        }
        KeyCode::Char(c) => return act(app, f, Some(c)),
        _ => return None,
    }
    Some(Vec::new())
}
