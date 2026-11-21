//! `tree`: rows that hang from the row named in their `parent`, on pipes, each fold shut or open.

use super::table::act;
use super::{cell, row, width_of};
use crate::app::App;
use crate::parts::{scroll::{keep, keep_by}, select::Sel};
use super::layout::{self, Item as Node, Laid, Look, Piece};
use crate::theme::glyph::Slot;
use crate::theme::palette::Role as Hue;
use crate::theme::Theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use j4n_proto::{Field, Fold, Row, Tone, Up};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::collections::{BTreeSet, HashMap};

/// A row of the tree as it is laid out.
struct Place {
    at: usize,
    /// Its place on the base: the pipes before the row and whether it ends its parent's.
    laid: Laid,
    kids: bool,
    parent: Option<usize>,
}

/// The rows that are shut: those the core starts folded, and those the operator shut, but not those the operator opened (`flip` holds the rows whose
/// state the operator turned over from the core's).
fn shut_rows(rows: &[Row], flip: &BTreeSet<String>) -> BTreeSet<String> {
    rows.iter().filter(|r| (r.fold == Some(Fold::Folded)) != flip.contains(&r.key)).map(|r| r.key.clone()).collect()
}

/// The rows that are in sight, in the order the core sent them (a parent before its children): rows whose parent is shut are not. Their pipes are the base's (`layout::lay`):
/// a row at the top is a head, drawn without a pipe, and what hangs from it starts at the first column.
fn places(rows: &[Row], shut: &BTreeSet<String>) -> Vec<Place> {
    let index: HashMap<&str, usize> = rows.iter().enumerate().map(|(i, r)| (r.key.as_str(), i)).collect();
    let parent: Vec<Option<usize>> = rows.iter().enumerate().map(|(i, r)| r.parent.as_deref().and_then(|p| index.get(p).copied()).filter(|p| *p < i)).collect();
    let mut kids = vec![0usize; rows.len()];
    for p in parent.iter().flatten() {
        kids[*p] += 1;
    }
    let mut hidden = vec![false; rows.len()];
    for i in 0..rows.len() {
        if let Some(p) = parent[i] {
            hidden[i] = hidden[p] || shut.contains(&rows[p].key);
        }
    }
    // the siblings of a row in sight are in sight (they share its parent), so the last of a parent is the same with the hidden rows left out
    let seen: Vec<usize> = (0..rows.len()).filter(|i| !hidden[*i]).collect();
    let items: Vec<Node> = seen.iter().map(|i| Node::new(&rows[*i].key, rows[*i].parent.as_deref())).collect();
    seen.into_iter().zip(layout::lay(&items)).map(|(i, laid)| Place { at: i, laid, kids: kids[i] > 0, parent: parent[i] }).collect()
}

/// The place in sight of the selected row: its own, or, when a fold shut over it (its step was done and folded while it was picked), the
/// row in sight before it, which is the shut row that holds it. A cursor on a row out of sight would draw no focus and take no key.
fn in_sight(seen: &[Place], sel: usize) -> Option<usize> {
    seen.iter().position(|p| p.at == sel).or_else(|| seen.iter().rposition(|p| p.at < sel))
}

/// A row as lines: the pipes and the mark in front of its words, and the words wrapped to the room that is left.
struct Item {
    /// What stands before the words on the first line and, under them, before each line after it (the pipes, then a blank for the mark).
    lead: Vec<Span<'static>>,
    under: Vec<Span<'static>>,
    mark: Vec<Span<'static>>,
    words: Vec<String>,
    style: ratatui::style::Style,
}

fn item(th: &Theme, rows: &[Row], p: &Place, room: usize) -> Item {
    let edge = th.fg(Hue::Edge);
    let r = &rows[p.at];
    // a head has no pipe: its column is left out, and what hangs from it starts at the first column
    let look = Look::new(th);
    let (mut lead, mut under) = (Vec::new(), Vec::new());
    let (cells, stem) = (look.cells(&p.laid), look.under(&p.laid));
    for (a, b) in cells.into_iter().zip(stem).skip(1) {
        lead.push(Span::styled(a, edge));
        under.push(Span::styled(b, edge));
    }
    // the mark of the row wants a blank before it
    if lead.last().is_some_and(|c| !c.content.ends_with(' ')) {
        lead.push(Span::raw(" "));
        under.push(Span::raw(" "));
    }
    let c = r.cells.first();
    let mark: Vec<Span<'static>> = c.and_then(|c| super::named(th, &c.icon)).into_iter().collect();
    let tone = c.map_or(r.tone, |c| if c.tone == Tone::Plain { r.tone } else { c.tone });
    let front = width_of(&lead) + width_of(&mark);
    let words = crate::wrap::wrap(c.map_or("", |c| c.text.as_str()), room.saturating_sub(front).max(4));
    Item { lead, under, mark, words, style: th.tone(tone) }
}

/// The fewest cells a column is cut to.
const MIN_COL: usize = 6;

/// The longest a name is taken to be when the columns are fitted: a longer one wraps under itself.
const MAX_NAME: usize = 24;

/// A row that says one thing after its title and nothing between: a section's head with its hint.
fn lone(r: &Row) -> bool {
    r.cells.len() > 2 && r.cells[1..r.cells.len() - 1].iter().all(|c| c.text.trim().is_empty()) && r.cells.last().is_some_and(|c| !c.text.trim().is_empty())
}

/// The cell cut to `wd` cells, with `…` where it was cut.
fn clip(c: &j4n_proto::Cell, wd: usize) -> j4n_proto::Cell {
    j4n_proto::Cell { text: crate::wrap::fit(&c.text, wd.saturating_sub(if c.icon.is_empty() { 0 } else { 2 })), ..c.clone() }
}

pub(crate) fn tree(app: &mut App, f: &Field, area: Rect, fr: &mut Frame) {
    let flip = app.wins.get(&f.bind).map(|w| w.flip.clone()).unwrap_or_default();
    let (rows, _, _) = app.held(&f.bind);
    let shut = shut_rows(rows, &flip);
    let seen = places(rows, &shut);
    let at = in_sight(&seen, (app.selected(f) as usize).min(rows.len().saturating_sub(1))).unwrap_or(0);
    let sel = seen.get(at).map_or(0, |p| p.at);
    let h = usize::from(area.height).max(1);
    let first = keep(app.win(&f.bind).cur.top, at as u64, at as u64, h as u64) as usize;
    let th = &app.theme;
    let (rows, _, _) = app.held(&f.bind);
    let w = usize::from(area.width);
    // the cells after the first line up under one another, as in a table
    let cols = seen.iter().skip(first).take(h).map(|p| rows[p.at].cells.len()).max().unwrap_or(0).saturating_sub(1);
    // a head with one word after its title (a hint) stands beside its title: it takes no column
    let mut col_w: Vec<usize> = (1..=cols).map(|k| seen.iter().skip(first).take(h).filter(|p| !lone(&rows[p.at])).filter_map(|p| rows[p.at].cells.get(k)).map(|c| width_of(&cell(th, c, Tone::Dim))).max().unwrap_or(0)).collect();
    // short of width the columns give way from the right, to a few cells each, before the words of a row do: a name is never cut to make room for a note
    let names = seen.iter().skip(first).take(h).map(|p| { let it = item(th, rows, p, w); width_of(&it.lead) + width_of(&it.mark) + it.words.iter().map(|l| unicode_width::UnicodeWidthStr::width(l.as_str())).max().unwrap_or(0) }).max().unwrap_or(0).min(MAX_NAME);
    let mut over = (2 + names + col_w.iter().map(|c| c + 2).sum::<usize>()).saturating_sub(w);
    for c in col_w.iter_mut().rev() {
        let cut = over.min(c.saturating_sub(MIN_COL));
        *c -= cut;
        over -= cut;
    }
    // the words of a row are wrapped under themselves, as the chat's are, to the room the cells after them leave
    let room = w.saturating_sub(2 + col_w.iter().map(|c| c + 2).sum::<usize>()).max(8);
    // the rows from the first one to the selected one must fit: the top moves down until they do
    let top = if seen.is_empty() { first } else { keep_by(first as u64, at as u64, h, |i| item(th, rows, &seen[i as usize], room).words.len()) as usize };
    app.win(&f.bind).cur.top = top as u64;
    let th = &app.theme;
    let (rows, _, _) = app.held(&f.bind);
    let items: Vec<(&Place, Item)> = seen.iter().skip(top).take(h).map(|p| (p, item(th, rows, p, room))).collect();
    let label_w = items.iter().map(|(_, it)| width_of(&it.lead) + width_of(&it.mark) + it.words.iter().map(|l| unicode_width::UnicodeWidthStr::width(l.as_str())).max().unwrap_or(0)).max().unwrap_or(0).min(room);
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (p, it) in &items {
        let r = &rows[p.at];
        let on = p.at == sel;
        // the row being changed is its editor, in its place, as a list draws it
        if on && let Some(ed) = app.row_lines(&f.bind, r, w, p.laid.last) {
            lines.extend(ed);
            continue;
        }
        for (n, text) in it.words.iter().enumerate() {
            let mut spans = vec![if on { Span::styled(format!("{} ", th.glyph(Slot::Focus)), th.fg(Hue::Accent)) } else { Span::raw("  ") }];
            let mut label = if n == 0 { it.lead.clone() } else { it.under.clone() };
            label.extend(if n == 0 { it.mark.clone() } else { vec![Span::raw(" ".repeat(width_of(&it.mark)))] });
            label.push(Span::styled(text.clone(), it.style));
            let pad = label_w.saturating_sub(width_of(&label));
            spans.append(&mut label);
            spans.push(Span::raw(" ".repeat(pad)));
            if let Some(hint) = r.cells.last().filter(|_| n == 0 && lone(r)) {
                let room = w.saturating_sub(width_of(&spans) + 2);
                spans.push(Span::raw("  "));
                spans.extend(cell(th, &clip(hint, room), Tone::Dim));
            }
            for (k, wd) in col_w.iter().enumerate().filter(|_| !lone(r)) {
                spans.push(Span::raw("  "));
                let parts = if n == 0 { r.cells.get(k + 1).map(|c| cell(th, &clip(c, *wd), Tone::Dim)).unwrap_or_default() } else { Vec::new() };
                let pad = wd.saturating_sub(width_of(&parts));
                spans.extend(parts);
                spans.push(Span::raw(" ".repeat(pad)));
            }
            if n == 0 && p.kids && shut.contains(&r.key) {
                let mark = Look::new(th).fold(Piece::FoldClosed);
                let gap = w.saturating_sub(width_of(&spans) + 2).max(1);
                spans.push(Span::raw(" ".repeat(gap)));
                spans.push(Span::styled(mark, th.fg(Hue::Faint)));
            }
            lines.push(row(th, spans, w, on));
        }
        if lines.len() >= h {
            break;
        }
    }
    lines.truncate(h);
    fr.render_widget(Paragraph::new(lines), area);
}

pub(crate) fn key(app: &mut App, f: &Field, k: KeyEvent) -> Option<Vec<Up>> {
    if k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
        return None;
    }
    let flip = app.wins.get(&f.bind).map(|w| w.flip.clone()).unwrap_or_default();
    let (rows, _, _) = app.held(&f.bind);
    let shut = shut_rows(rows, &flip);
    let seen = places(rows, &shut);
    let at = in_sight(&seen, (app.selected(f) as usize).min(rows.len().saturating_sub(1)))?;
    let sel = seen[at].at;
    if sel as u64 != app.selected(f) {
        app.move_to(f, sel as u64);
    }
    let (rows, _, _) = app.held(&f.bind);
    let key = rows[sel].key.clone();
    let kids = seen[at].kids;
    let parent = seen[at].parent;
    let mut s = Sel { at: at as u64, top: 0 };
    if s.key(k.code, app.screenful(), seen.len() as u64) {
        app.move_to(f, seen[s.at as usize].at as u64);
        return Some(Vec::new());
    }
    // the operator's word is kept as a turn over from the core's: the row is shut when it is shut now and `shut_it`, or open when it is
    let was = shut.contains(&key);
    let flip = |app: &mut App, shut_it: bool| {
        if was != shut_it {
            super::toggle(&mut app.win(&f.bind).flip, key.as_str());
        }
    };
    match k.code {
        KeyCode::Right if kids => flip(app, false),
        KeyCode::Left if kids && !shut.contains(&key) => flip(app, true),
        KeyCode::Left => {
            if let Some(p) = parent {
                app.move_to(f, p as u64);
            }
        }
        KeyCode::Enter | KeyCode::Char(' ') if kids => flip(app, !shut.contains(&key)),
        KeyCode::Enter => return act(app, f, None).or(Some(Vec::new())),
        KeyCode::Char(c) => return act(app, f, Some(c)),
        _ => return None,
    }
    Some(Vec::new())
}
