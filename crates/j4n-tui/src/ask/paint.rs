//! The questions drawn: the popup for one, the tray for several, and the few marks of one inline in the chat. Square corners always; the
//! words come from the core and the marks from the glyph slots, so every tier and `NO_COLOR` draw them.

use super::opts::{Opt, Send, is_score, options, shape_icon};
use crate::parts::pick::{self, Custom, Item, MOST, Pick};
use super::{Look, Place};
use crate::app::App;
use crate::parts::rows::{lead, trunk};
use crate::display::{LINE_MAX, cell, fill_bar, join, lr, mark_sp, named, width_of};
use crate::theme::Theme;
use crate::theme::glyph::{Slot, Tier};
use crate::theme::palette::Role as Hue;
use crate::wrap::{fit, wrap};
use j4n_proto::{AskView, Row, Shape, Tone};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

/// The widest a popup is drawn: the width of the composer under it, up to this (100 is a line a reader can follow).
const POPUP: usize = 100;

type Spans = Vec<Span<'static>>;

/// The name of the lane that asked, as its row in a chat says it (`B claude`), or the key of the task.
fn lane_label(app: &App, a: &AskView) -> String {
    app.chats
        .iter()
        .filter_map(|b| app.rows.get(b))
        .flatten()
        .find(|r| r.key == a.asker)
        .and_then(|r| r.cells.first())
        .map_or_else(|| a.asker.clone(), |c| c.text.clone())
}

/// The first word of it (`B`).
fn short(label: &str) -> String {
    label.split(' ').next().unwrap_or(label).to_string()
}

fn gap(n: usize) -> Span<'static> {
    Span::raw(" ".repeat(n))
}

fn dim(th: &Theme, s: impl Into<String>) -> Span<'static> {
    Span::styled(s.into(), th.tone(Tone::Dim))
}

/// The questions to draw at `width` columns in at most `max` rows.
pub(crate) fn lines(app: &App, look: &Look, width: usize, max: usize) -> Vec<Line<'static>> {
    // a line held because it looks like a key: its card stands in the place of the questions until it is answered
    if let Some(held) = app.held_view().filter(|_| width >= 12 && max >= 3) {
        return popup(app, &held, width.min(POPUP), max);
    }
    let shown = app.asks_shown();
    if shown.is_empty() || width < 12 || max < 3 {
        return Vec::new();
    }
    let place = if shown.len() == 1 { look.one } else { look.many };
    match place {
        Place::Inline => Vec::new(),
        Place::Popup => match app.ask_focus() {
            Some(a) => popup(app, a, width.min(POPUP), max),
            None => Vec::new(),
        },
        Place::Tray => tray(app, &shown, width, max),
    }
}

// ---- pieces ----

/// `▕███▒░░░░░░▏ 30`: how risky, in the colour of how risky.
fn risk(th: &Theme, v: u8, cells: usize) -> Spans {
    let hue = if v >= 80 { Hue::Error } else if v >= 50 { Hue::Warn } else { Hue::Accent };
    let mut s = fill_bar(th, f64::from(v) / 100.0, cells, th.fg(hue));
    s.push(Span::styled(format!(" {v}"), th.fg(hue)));
    s
}

/// The marks of the answers in a few cells: `1 󰄬  2 󰅖`, a score `1 2 3 4 5`, the count of a choice.
fn mini(th: &Theme, a: &AskView, opts: &[Opt]) -> Spans {
    match a.shape {
        Shape::Text => named(th, "ask-text").into_iter().collect(),
        Shape::Choice if is_score(a) => {
            let mut s: Spans = named(th, "ask-score").into_iter().collect();
            s.push(dim(th, a.options.iter().map(|c| c.label.trim().to_string()).collect::<Vec<_>>().join(" ")));
            s
        }
        Shape::Choice => {
            let mut s: Spans = named(th, "ask-choice").into_iter().collect();
            s.push(dim(th, a.options.len().to_string()));
            s
        }
        _ => join(
            opts.iter().take(2).enumerate().map(|(i, o)| {
                let mut s = vec![dim(th, format!("{} ", i + 1))];
                s.extend(named(th, o.icon));
                s
            }).collect(),
            " ",
        ),
    }
}

/// The hint: `⇥ 1`, with `~` when a model made it.
fn hint_of(th: &Theme, opts: &[Opt]) -> Spans {
    let Some((i, o)) = opts.iter().enumerate().find(|(_, o)| o.hint) else { return Vec::new() };
    let mut s = Vec::new();
    if o.model {
        s.push(Span::styled(th.glyph(Slot::Made), th.fg(Hue::Made)));
    }
    s.push(Span::styled(format!("{} ", th.glyph(Slot::Hint)), th.fg(Hue::Accent)));
    s.push(Span::styled((i + 1).to_string(), th.fg(Hue::Accent)));
    s
}

/// What stands at the right of the row of a question in the chat: the marks of its answers and its hint.
pub(crate) fn inline(th: &Theme, a: &AskView, who: &str) -> Vec<Spans> {
    let opts = options(a, who);
    let mut out = vec![mini(th, a, &opts)];
    let h = hint_of(th, &opts);
    if !h.is_empty() {
        out.push(h);
    }
    // a mark that ends the row has no word after it to be a blank away from
    if let Some(last) = out.last_mut().and_then(|g| g.last_mut()) {
        last.content = last.content.trim_end().to_string().into();
    }
    out
}

/// The row of one option: the hint column, the number, the mark and the words, and at the right its note or that it widens.
fn option_row(th: &Theme, i: usize, o: &Opt, (armed, sent, phase): (bool, bool, u8), iw: usize) -> Spans {
    let hw = th.glyph(Slot::Hint).width();
    let mut left: Spans = Vec::new();
    if o.hint {
        left.push(Span::styled(th.glyph(Slot::Hint), th.fg(Hue::Accent)));
        left.push(if o.model { Span::styled(th.glyph(Slot::Made), th.fg(Hue::Made)) } else { gap(1) });
    } else {
        left.push(gap(hw + 1));
    }
    let n = Span::styled((i + 1).to_string(), if o.hint { th.fg(Hue::Accent).add_modifier(Modifier::BOLD) } else { th.tone(Tone::Dim) });
    left.push(n);
    left.push(gap(2));
    // the words tier says an answer once: its label is the word its mark would be
    let marks = th.tier != Tier::Words;
    if marks {
        left.extend(named(th, o.icon));
    }
    let mut right: Spans = Vec::new();
    if armed {
        right.push(Span::styled(format!("{} again", i + 1), th.tone(Tone::Warn).add_modifier(Modifier::BOLD)));
    } else if sent {
        // the key was pressed and the core has not yet taken the answer
        right.push(Span::styled("sending", th.breath(phase)));
    } else if o.widens {
        if marks {
            right.extend(named(th, "risky"));
        }
        right.push(Span::styled("widens", th.tone(Tone::Warn)));
    } else if !o.note.is_empty() {
        if o.model {
            right.push(Span::styled(th.glyph(Slot::Made), th.fg(Hue::Made)));
            right.push(gap(1));
        }
        right.push(dim(th, o.note.clone()));
    }
    let room = iw.saturating_sub(width_of(&left) + width_of(&right) + 2);
    left.push(Span::raw(fit(&o.label, room.max(1))));
    lr(left, right, iw)
}

/// The rows of a choice, drawn by the part `pick`: its options, the count of the rest, and the row `custom` with the ready answers under it.
fn choice(app: &App, a: &AskView, opts: &[Opt], iw: usize) -> Vec<Spans> {
    let sent = app.ui.sent.get(&a.ask).map(|(id, _)| id.as_str());
    let at = opts.iter().position(|o| o.send == Send::Type);
    let items = opts[..at.unwrap_or(opts.len())].iter().map(|o| Item { label: o.label.clone(), note: o.note.clone(), hint: o.hint, model: o.model, sent: sent == Some(o.id.as_str()) }).collect();
    let mine = app.ask_focus().is_some_and(|f| f.ask == a.ask);
    let custom = at.map(|i| Custom {
        typed: if mine { app.line.buf.lines().next().unwrap_or_default().to_string() } else { String::new() },
        on: app.ui.typing.as_deref() == Some(a.ask.as_str()) || (mine && !app.line.buf.is_empty()),
        ready: opts[i + 1..].iter().map(|o| o.label.clone()).collect(),
    });
    pick::rows(&app.theme, &Pick { items, more: a.options.len().saturating_sub(MOST), custom }, app.phase, iw)
}

/// The rows of the change an approval would make, as a small tree; with their diffs when `diff`.
fn change(th: &Theme, rows: &[Row], diff: bool, iw: usize) -> Vec<Spans> {
    let mut out = Vec::new();
    for (n, r) in rows.iter().enumerate() {
        let last = n + 1 == rows.len();
        let mut left = vec![lead(th, last, false, false)];
        if let Some(c) = r.cells.first() {
            left.extend(cell(th, c, Tone::Plain));
        }
        if let Some(w) = r.cells.get(1).filter(|c| !c.text.is_empty()) {
            left.push(dim(th, format!("  {}", w.text)));
        }
        let right = join(r.cells.iter().skip(2).map(|c| cell(th, c, Tone::Dim)).collect(), "  ");
        let room = iw.saturating_sub(width_of(&right) + 4);
        if width_of(&left) > room + 2 {
            left.truncate(1);
            left.extend(r.cells.first().map(|c| Span::raw(fit(&c.text, room))));
        }
        out.push(lr(left, right, iw));
        if diff {
            let under = trunk(th, last);
            for d in &r.detail {
                let style = if d.text.starts_with('+') { th.fg(Hue::Add) } else if d.text.starts_with(['-', '−']) { th.fg(Hue::Del) } else { th.tone(Tone::Dim) };
                out.push(vec![under.clone(), gap(2), Span::styled(fit(&d.text, iw.saturating_sub(4)), style)]);
            }
        }
    }
    out
}

// ---- the popup ----

/// A part of the popup and how much it matters when the room is short: the least goes first.
struct Part {
    keep: u8,
    rows: Vec<Spans>,
}

fn popup(app: &App, a: &AskView, w: usize, max: usize) -> Vec<Line<'static>> {
    crate::parts::popup::rows(&app.theme, body(app, a, w - 4, max, false), w, None)
}

/// The body of the approval card, `iw` columns wide, and its title (the lane that asked, or the key that was pressed): the popup without
/// its head row, which the top edge of the card says.
pub(super) fn card(app: &App, a: &AskView, iw: usize, max: usize) -> (String, Vec<Line<'static>>) {
    let title = if super::card::is_confirm(a) { a.asker.clone() } else { lane_label(app, a) };
    (title, body(app, a, iw, max, true).into_iter().map(Line::from).collect())
}

/// The rows inside a popup or a card: what was asked, what it touches, what it would change, the answers, and the keys. `max` is the rows
/// the box may take with its two edges; the least that matters goes first until it fits.
fn body(app: &App, a: &AskView, iw: usize, max: usize, card: bool) -> Vec<Spans> {
    let th = &app.theme;
    let who = lane_label(app, a);
    let opts = options(a, &short(&who));
    let mut parts: Vec<Part> = Vec::new();
    let mut head = vec![mark_sp(th, Slot::Ask), Span::styled(who.clone(), th.tone(Tone::Warn).add_modifier(Modifier::BOLD))];
    if shape_icon(a) == "ask-text" {
        head.push(gap(0));
    }
    let wait: Spans = if a.wait.is_empty() { Vec::new() } else { vec![mark_sp(th, Slot::Cooling), dim(th, a.wait.clone())] };
    if !card {
        parts.push(Part { keep: 8, rows: vec![lr(head, wait, iw)] });
    }
    parts.push(Part { keep: 1, rows: vec![Vec::new()] });
    parts.push(Part { keep: 9, rows: wrap(&a.question, iw).into_iter().map(|t| vec![Span::raw(t)]).collect() });
    parts.push(Part { keep: 1, rows: vec![Vec::new()] });
    let facts = join(a.facts.iter().map(|c| cell(th, c, Tone::Dim)).collect(), "   ");
    // the facts stand beside the risk or on one row while they fit; else each has a row of its own, cut to the width
    let apart = |rows: &mut Vec<Spans>| rows.extend(a.facts.iter().map(|c| cell(th, &j4n_proto::Cell { text: fit(&c.text, iw.saturating_sub(3)), ..c.clone() }, Tone::Dim)));
    if a.shape == Shape::Approval || a.risk > 0 {
        let mut r = risk(th, a.risk.min(100), LINE_MAX);
        let mut rows = Vec::new();
        if !facts.is_empty() && width_of(&r) + 3 + width_of(&facts) <= iw {
            r.push(gap(3));
            r.extend(facts);
            rows.push(r);
        } else {
            rows.push(r);
            apart(&mut rows);
        }
        parts.push(Part { keep: 6, rows });
    } else if !facts.is_empty() {
        let mut rows = Vec::new();
        if width_of(&facts) <= iw {
            rows.push(facts);
        } else {
            apart(&mut rows);
        }
        parts.push(Part { keep: 6, rows });
    }
    if !a.effect.is_empty() {
        parts.push(Part { keep: 7, rows: vec![vec![Span::styled(fit(&a.effect, iw), th.tone(if a.risk >= 50 { Tone::Warn } else { Tone::Dim }))]] });
    }
    if !a.change.is_empty() {
        parts.push(Part { keep: 4, rows: change(th, &a.change, app.ui.diff.contains(&a.ask), iw) });
    }
    parts.push(Part { keep: 1, rows: vec![Vec::new()] });
    let armed = app.ui.armed.as_ref().filter(|(k, _)| *k == a.ask).map(|(_, n)| *n);
    let sent = app.ui.sent.get(&a.ask).map(|(id, _)| id.as_str());
    let mut rows: Vec<Spans> = if a.shape == Shape::Choice {
        choice(app, a, &opts, iw)
    } else {
        opts.iter().enumerate().map(|(i, o)| option_row(th, i, o, (armed == Some(i + 1), sent == Some(o.id.as_str()), app.phase), iw)).collect()
    };
    if a.shape == Shape::Text {
        rows.push(vec![dim(th, "type the answer and send it")]);
    }
    parts.push(Part { keep: 9, rows });
    // the reason is there because `?` asked for it: on a short screen it outlasts the sketch of the change
    if app.ui.why.contains(&a.ask) && !a.why.is_empty() {
        parts.push(Part { keep: 5, rows: std::iter::once(Vec::new()).chain(wrap(&a.why, iw).into_iter().map(|t| vec![dim(th, t)])).collect() });
    }
    parts.push(Part { keep: 1, rows: vec![Vec::new()] });
    parts.push(Part { keep: 3, rows: vec![vec![dim(th, keys(th, a, &opts, app.ui.why.contains(&a.ask), card))]] });
    // the least that matters goes first until it fits; two blank rows in a row (a part between them was left out) are one
    let flat = |p: &[Part]| -> Vec<Spans> {
        let mut rows: Vec<Spans> = p.iter().flat_map(|x| x.rows.clone()).collect();
        rows.dedup_by(|b, a| b.is_empty() && a.is_empty());
        rows
    };
    while flat(&parts).len() + 2 > max {
        let Some(least) = parts.iter().map(|p| p.keep).filter(|k| *k < 9).min() else { break };
        let at = parts.iter().position(|p| p.keep == least).unwrap_or(0);
        parts.remove(at);
    }
    let mut body = flat(&parts);
    body.truncate(max - 2);
    body
}

/// The keys under the options: only the ones that do something for this question. The card says `y` and `n` too.
fn keys(th: &Theme, a: &AskView, opts: &[Opt], why: bool, card: bool) -> String {
    let enter = th.glyph(Slot::KeyEnter);
    let mut parts: Vec<String> = Vec::new();
    if card && matches!(a.shape, Shape::YesNo | Shape::Approval) {
        parts.push("y yes · n no".into());
    }
    match opts.iter().filter(|o| matches!(o.send, Send::Reply(j4n_proto::Reply::Pick { .. }))).count().max(if a.shape == Shape::Choice { 0 } else { opts.len() }) {
        0 => {}
        1 => parts.push("1".into()),
        n => parts.push(format!("1–{n}")),
    }
    if super::custom(a) {
        parts.push("type to answer".into());
    }
    if a.shape == Shape::Text {
        parts.push(format!("{enter} send"));
    }
    if super::has_diff(a) {
        parts.push(format!("{enter} diff"));
    } else if let Some(o) = card.then(|| opts.iter().find(|o| o.hint)).flatten() {
        parts.push(format!("{enter} {}", o.label));
    }
    if !a.why.is_empty() {
        parts.push(if why { "? hide".into() } else { "? why".into() });
    }
    parts.push(if a.ask == crate::chat::held::ASK { "esc drops it" } else if super::card::is_confirm(a) || super::front_owned(a) { "esc not now" } else { "esc later" }.into());
    parts.join(" · ")
}

// ---- the tray ----

fn tray(app: &App, shown: &[&AskView], w: usize, max: usize) -> Vec<Line<'static>> {
    let th = &app.theme;
    let iw = w - 4;
    let focus = app.ask_focus().map(|a| a.ask.clone());
    let at = shown.iter().position(|a| Some(&a.ask) == focus.as_ref()).unwrap_or(0);
    let labels: Vec<String> = shown.iter().map(|a| lane_label(app, a)).collect();
    let lw = labels.iter().map(|l| l.width()).max().unwrap_or(0).min(iw / 4);
    let mut blocks: Vec<Vec<Spans>> = Vec::new();
    for (n, a) in shown.iter().enumerate() {
        let last = n + 1 == shown.len();
        let mut block = vec![tray_row(app, a, &labels[n], lw, n == at, last, iw)];
        if n == at && !is_score(a) {
            block.extend(tray_open(app, a, &short(&labels[n]), last, iw));
        }
        blocks.push(block);
    }
    let mut head = vec![mark_sp(th, Slot::Ask), Span::styled(shown.len().to_string(), th.tone(Tone::Warn).add_modifier(Modifier::BOLD))];
    let help = format!("{} · 1–9 · esc", th.glyph(Slot::KeyArrows).chars().take(2).collect::<String>());
    head = lr(head, vec![dim(th, help)], iw);
    // what does not fit goes, the farthest from the question the keys are on first
    let (mut lo, mut hi) = (0, blocks.len());
    let total = |lo: usize, hi: usize| 1 + blocks[lo..hi].iter().map(Vec::len).sum::<usize>() + 2;
    while total(lo, hi) > max && hi - lo > 1 {
        if at - lo >= hi - 1 - at {
            lo += 1;
        } else {
            hi -= 1;
        }
    }
    let mut body = vec![head];
    let mut sel = None;
    for (n, b) in blocks.into_iter().enumerate().take(hi).skip(lo) {
        if n == at {
            sel = Some(body.len());
        }
        body.extend(b);
    }
    body.truncate(max - 2);
    crate::parts::popup::rows(th, body, w, sel.filter(|s| *s < max - 2))
}

fn tray_row(app: &App, a: &AskView, label: &str, lw: usize, focused: bool, last: bool, iw: usize) -> Spans {
    let th = &app.theme;
    let opts = options(a, &short(label));
    // the elbow stays on the question the keys are on: its mark takes the accent colour
    let mut left = vec![lead(th, last, focused, false), Span::styled(format!("{}{}", fit(label, lw), " ".repeat(lw.saturating_sub(label.width()))), th.tone(Tone::Plain).add_modifier(Modifier::BOLD)), gap(2)];
    let mut right: Spans = Vec::new();
    if focused && is_score(a) {
        right.extend(mini(th, a, &opts));
        let h = hint_of(th, &opts);
        if !h.is_empty() {
            right.push(gap(2));
            right.extend(h);
        }
    } else if focused {
        if !a.wait.is_empty() {
            right.push(mark_sp(th, Slot::Cooling));
            right.push(dim(th, a.wait.clone()));
            right.push(gap(1));
        }
        right.push(Span::styled(th.glyph(Slot::FoldOpen), th.fg(Hue::Faint)));
    } else {
        if a.shape == Shape::Approval {
            right.extend(risk(th, a.risk.min(100), 6));
            right.push(gap(2));
        }
        right.extend(mini(th, a, &opts));
        if super::has_diff(a) {
            right.push(gap(2));
            right.push(dim(th, th.glyph(Slot::KeyEnter)));
        }
    }
    let mut room = iw.saturating_sub(width_of(&left) + width_of(&right) + 2);
    if room < 8 {
        right.clear();
        room = iw.saturating_sub(width_of(&left));
    }
    left.push(Span::raw(fit(&a.question, room.max(1))));
    lr(left, right, iw)
}

/// The rows under the question the keys are on: its change if opened, then its options.
fn tray_open(app: &App, a: &AskView, who: &str, last: bool, iw: usize) -> Vec<Spans> {
    let th = &app.theme;
    let opts = options(a, who);
    let under = trunk(th, last);
    let armed = app.ui.armed.as_ref().filter(|(k, _)| *k == a.ask).map(|(_, n)| *n);
    let sent = app.ui.sent.get(&a.ask).map(|(id, _)| id.as_str());
    let mut rows: Vec<Spans> = Vec::new();
    if app.ui.diff.contains(&a.ask) {
        rows.extend(change(th, &a.change, true, iw - 2));
    }
    let items: Vec<Spans> = if a.shape == Shape::Choice {
        choice(app, a, &opts, iw.saturating_sub(5))
    } else {
        opts.iter().enumerate().map(|(i, o)| option_row(th, i, o, (armed == Some(i + 1), sent == Some(o.id.as_str()), app.phase), iw.saturating_sub(5))).collect()
    };
    let n = items.len();
    for (i, item) in items.into_iter().enumerate() {
        let mut r = vec![under.clone(), gap(1), lead(th, i + 1 == n, false, false)];
        r.extend(item);
        rows.push(r);
    }
    rows
}
