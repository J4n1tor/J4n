//! `chat`: the transcript and the composer. The core sends the transcript as the rows of the chat's
//! binding — a turn, its lanes, their steps, each hanging from the one it belongs to — and the screen lays them out on pipes: it
//! merges the steps of one kind, folds what a rule or the core says to fold, puts the stats at the right edge and makes the name of
//! the doer that works breathe. A chat with no rows yet is the composer alone.

pub(crate) mod chats;
mod compose;
pub(crate) mod held;
pub(crate) mod live;
mod plan;
pub(crate) mod scroll;

use crate::app::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use j4n_proto::{Control, Field, Intent, Json, Up};
use plan::{Out, Plan};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use unicode_width::UnicodeWidthStr;
use ratatui::widgets::Paragraph;

/// How the transcript is laid out: the options of the chat field.
pub struct Look {
    /// Where prose wraps.
    pub width: usize,
    /// How many lines of a step's output show before it is opened.
    pub tail: usize,
    /// Steps of one kind in a row become one row.
    pub merge: bool,
    /// A lane that is settled folds when a newer one has started.
    pub fold_older: bool,
    /// A lane that failed stays open.
    pub keep_failed: bool,
}

impl Look {
    pub fn of(f: &Field) -> Look {
        let flag = |name: &str| f.props.get(name).and_then(Json::as_bool).unwrap_or(true);
        let num = |name: &str, default: usize| f.props.get(name).and_then(Json::as_u64).map_or(default, |n| n as usize);
        Look { width: num("width", 88), tail: num("tail", 3), merge: flag("merge_same"), fold_older: flag("fold_older"), keep_failed: flag("keep_failed") }
    }
}

/// The lines of the turns, newest last, built from the end: only as many turns as the screen needs, and the one the keys are on.
fn build(app: &mut App, f: &Field, width: usize, room: usize) -> (Vec<Out>, usize, bool, bool) {
    let look = Look::of(f);
    let (flip, focus) = app.wins.get(&f.bind).map(|w| (w.flip.clone(), w.at.clone())).unwrap_or_default();
    let up = app.scroll.get(&f.bind).copied().unwrap_or(0);
    let core = app.rows.get(&f.bind).map_or(&[][..], Vec::as_slice);
    // the lines sent that the core has not shown yet are turns after its own
    let (waiting, live) = app.pending(&f.bind);
    let owned: Vec<j4n_proto::Row>;
    let rows = if waiting.is_empty() {
        core
    } else {
        owned = core.iter().cloned().chain(waiting).collect();
        &owned
    };
    // the words of time as the screen's clock says them now
    let aged = app.aged(rows);
    let rows = &*aged;
    let mut plan = Plan::new(&app.theme, rows, &flip, &look, width, app.phase, focus.as_deref()).aimed(app.aim.as_deref());
    plan.sign = app.sign;
    plan.asks = &app.asks;
    plan.read = &app.read;
    plan.live = &live;
    plan.pinned = app.pinned();
    let turns = plan.turns().to_vec();
    let mut blocks: Vec<Vec<Out>> = Vec::new();
    let (mut have, mut seen) = (0, focus.is_none());
    for (n, root) in turns.iter().enumerate().rev() {
        let mut lines = plan.turn(*root);
        // a blank row between turns, and above the first when the operator's line is a band
        if n > 0 || app.theme.you != crate::theme::You::Mark {
            lines.insert(0, Plan::gap());
        }
        have += lines.len();
        seen |= lines.iter().any(|o| o.head.as_deref() == focus.as_deref());
        blocks.push(lines);
        if have >= room + up && seen {
            break;
        }
    }
    let lines: Vec<Out> = blocks.into_iter().rev().flatten().collect();
    (lines, up, plan.breathes, plan.signs)
}

pub fn draw(app: &mut App, f: &Field, area: Rect, fr: &mut Frame) {
    // the crumb row, and in the orchestrator the pinned list, over the transcript; nothing while the chats are off
    let head = chats::head(app, usize::from(area.width), usize::from(area.height / 2));
    let head_h = (head.len() as u16).min(area.height.saturating_sub(4));
    if head_h > 0 {
        fr.render_widget(Paragraph::new(head.into_iter().take(usize::from(head_h)).collect::<Vec<_>>()), Rect { height: head_h, ..area });
    }
    let area = Rect { y: area.y + head_h, height: area.height - head_h, ..area };
    let card = crate::view::card(app);
    let comp_room = usize::from(area.height).saturating_sub(1);
    let comp = compose::layout(app, f, usize::from(area.width), comp_room);
    let comp_h = (comp.lines.len() as u16).min(area.height);
    let card_h = (card.len() as u16).min(area.height.saturating_sub(comp_h + 1));
    let top = area.height - comp_h - card_h;
    // the questions stand between the transcript and the composer; the transcript keeps its last lines above them
    let ask_h = crate::ask::height(app, area.width, top.saturating_sub(2));
    let room = usize::from(top - ask_h);
    let (mut lines, up, breathes, signs) = build(app, f, usize::from(area.width), room);
    // a chat that has no doer says so in its empty state, where the strip said it before
    if compose::no_doer(app.values.get(j4n_proto::COMPOSER_STRIP)) {
        if !lines.is_empty() {
            lines.push(Plan::gap());
        }
        lines.push(Out { line: compose::empty_state(&app.theme), ..Plan::gap() });
    }
    app.breathes = breathes || !app.ui.sent.is_empty();
    // a part that waits elsewhere on the page keeps its sign
    app.signs |= signs;
    let focus = app.wins.get(&f.bind).and_then(|w| w.at.clone());
    // the line the keys are on stays in sight: the head of the row, and the lines under it they have walked down
    let head = focus.as_deref().and_then(|k| lines.iter().position(|o| o.head.as_deref() == Some(k)));
    let sub = head.map_or(0, |h| app.wins.get(&f.bind).map_or(0, |w| w.sub).min(lines[h + 1..].iter().position(|o| o.head.is_some()).unwrap_or(lines.len() - h - 1)));
    let at = head.map(|h| h + sub);
    let mut up = up.min(lines.len().saturating_sub(room));
    if let Some(i) = at {
        let end = lines.len() - up;
        if i < end.saturating_sub(room) {
            up = lines.len() - room - i;
        } else if i >= end {
            up = lines.len() - 1 - i;
        }
    }
    if up == 0 {
        app.scroll.remove(&f.bind);
    } else {
        app.scroll.insert(f.bind.clone(), up);
    }
    let end = lines.len() - up;
    let first = end.saturating_sub(room);
    let mut shown: Vec<Line<'static>> = lines[first..end].iter().map(|o| o.line.clone()).collect();
    // a line under its head, the keys on it: it has the selection behind it as the head has
    if let (Some(i), true) = (at, sub > 0)
        && let Some(l) = i.checked_sub(first).and_then(|n| shown.get_mut(n))
    {
        *l = l.clone().style(app.theme.back(crate::theme::palette::Role::Sel));
    }
    let pad = room.saturating_sub(shown.len());
    // the links among the lines in sight, by cell, for the terminal to open once the frame is drawn
    for (k, o) in lines[first..end].iter().enumerate() {
        for l in &o.links {
            let (x, y) = (usize::from(area.x) + l.col, usize::from(area.y) + pad + k);
            let w = l.w.min(usize::from(area.right()).saturating_sub(x));
            if w > 0 {
                app.opens.push(crate::read::Open { x: x as u16, y: y as u16, w: w as u16, url: l.url.clone() });
            }
        }
    }
    // a heading in kitty's text sizing takes the cells of its row and the blank row under it, and only when both are in sight and the words
    // (twice as wide) end inside the area: a heading cut by the edge of the lines keeps its plain words
    for (k, o) in lines[first..end].iter().enumerate().take((end - first).saturating_sub(1)) {
        let Some((col, text)) = &o.big else { continue };
        let x = usize::from(area.x) + col;
        if x + 2 * text.width() <= usize::from(area.right()) {
            app.bigs.push(crate::read::Big { x: x as u16, y: area.y + (pad + k) as u16, text: text.clone() });
        }
    }
    fr.render_widget(Paragraph::new(shown), Rect { y: area.y + pad as u16, height: (room - pad) as u16, ..area });
    if ask_h > 0 {
        crate::ask::float(app, Rect { y: area.y + top - ask_h, height: ask_h, ..area }, fr);
    }
    if card_h > 0 {
        fr.render_widget(Paragraph::new(card.into_iter().take(usize::from(card_h)).collect::<Vec<_>>()), Rect { y: area.y + top, height: card_h, ..area });
    }
    let cy = area.y + top + card_h;
    let (col, row) = comp.cursor;
    fr.render_widget(Paragraph::new(comp.lines), Rect { y: cy, height: comp_h, ..area });
    if focus.is_none() {
        fr.set_cursor_position((area.x + col, cy + row));
    }
}

// ---- keys ----

/// The transcript as the keys walk it: how many lines it has, and where the head of each row is (its line, its key, whether it folds
/// and is open, whether it is a turn).
pub(crate) struct Walk {
    pub len: usize,
    pub heads: Vec<(usize, String, Option<bool>, bool)>,
    /// The ids of the project's docs named on each line.
    pub terms: Vec<Vec<String>>,
}

impl Walk {
    /// The line the keys are on, with the keys on the row at `place` and `sub` lines under its head.
    fn line(&self, place: usize, sub: usize) -> usize {
        let Some(h) = self.heads.get(place) else { return 0 };
        let end = self.heads.get(place + 1).map_or(self.len, |n| n.0);
        h.0 + sub.min(end.saturating_sub(h.0 + 1))
    }

    /// The row the keys are on and the lines under its head, for a line of the transcript; the first head is the top.
    fn at(&self, line: usize) -> Option<(&str, usize)> {
        let first = self.heads.first()?.0;
        let line = line.max(first);
        let i = self.heads.partition_point(|h| h.0 <= line).saturating_sub(1);
        Some((&self.heads[i].1, line - self.heads[i].0))
    }
}

pub(crate) fn walk(app: &mut App, f: &Field) -> Walk {
    let (lines, ..) = build(app, f, usize::from(app.size.0), usize::MAX / 4);
    let terms = lines.iter().map(|o| o.terms.clone()).collect();
    Walk { len: lines.len(), heads: lines.into_iter().enumerate().filter_map(|(i, o)| o.head.map(|k| (i, k, o.fold, o.turn))).collect(), terms }
}

fn toggle(app: &mut App, f: &Field, key: &str) {
    crate::display::toggle(&mut app.win(&f.bind).flip, key);
}

/// Put the keys on a line of the transcript (a row and the lines under its head), or give them back to the composer.
fn land(app: &mut App, f: &Field, to: Option<(String, usize)>) {
    let none = to.is_none();
    let w = app.win(&f.bind);
    (w.at, w.sub) = to.map_or((None, 0), |(k, n)| (Some(k), n));
    if none {
        app.scroll.remove(&f.bind);
    }
}

/// A key for the transcript: `Some` when it was taken here. The transcript has the keys only after `⌃↑`; until then they are the
/// composer's, except `⇧↑` and `⇧↓`, which scroll it a line without taking them.
pub fn key(app: &mut App, f: &Field, k: KeyEvent) -> Option<Vec<Up>> {
    let (ctrl, alt) = (k.modifiers.contains(KeyModifiers::CONTROL), k.modifiers.contains(KeyModifiers::ALT));
    if ctrl && k.code == KeyCode::Char('t') {
        app.next_aim();
        return Some(Vec::new());
    }
    // the keys are on the pinned list of the chats
    if let Some(ups) = chats::key(app, k) {
        return Some(ups);
    }
    let at = app.wins.get(&f.bind).and_then(|w| w.at.clone());
    let Some(at) = at else {
        if ctrl && matches!(k.code, KeyCode::Up) {
            let turn = walk(app, f).heads.into_iter().rev().find(|h| h.3).map(|h| h.1);
            // with no turn to go to, the list of the chats above them has the keys
            if turn.is_none() && chats::up_into(app) {
                return Some(Vec::new());
            }
            land(app, f, turn.map(|k| (k, 0)));
            return Some(Vec::new());
        }
        if k.modifiers.contains(KeyModifiers::SHIFT) && !ctrl && !alt && matches!(k.code, KeyCode::Up | KeyCode::Down) {
            app.scroll_lines(k.code == KeyCode::Up, 1);
            return Some(Vec::new());
        }
        return None;
    };
    let w = walk(app, f);
    if w.heads.is_empty() {
        land(app, f, None);
        return Some(Vec::new());
    }
    let place = w.heads.iter().position(|h| h.1 == at).unwrap_or(0);
    let cur = w.line(place, app.wins.get(&f.bind).map_or(0, |x| x.sub));
    let turn = |app: &mut App, i: Option<usize>| land(app, f, i.map(|i| (w.heads[i].1.clone(), 0)));
    match k.code {
        KeyCode::Up if ctrl => match w.heads[..place].iter().rposition(|h| h.3) {
            // above the first turn, the list of the chats
            None if chats::up_into(app) => land(app, f, None),
            above => turn(app, above),
        },
        KeyCode::Down if ctrl => turn(app, w.heads[place + 1..].iter().position(|h| h.3).map(|i| i + place + 1)),
        // one line, a wrapped one too; the first line keeps the keys when there is nothing above it, past the last they go back to the composer
        KeyCode::Up => land(app, f, w.at(cur.saturating_sub(1)).map(|(k, n)| (k.to_string(), n))),
        KeyCode::Down if cur + 1 >= w.len => land(app, f, None),
        KeyCode::Down => land(app, f, w.at(cur + 1).map(|(k, n)| (k.to_string(), n))),
        // half a screen, as a glide (`theme.motion`) or a jump
        KeyCode::PageUp => app.page_by(true),
        KeyCode::PageDown => app.page_by(false),
        // a lane that has a chat of its own is its door: `⏎` goes in, `space` opens the steps in place
        KeyCode::Enter if !ctrl && !alt && door(app, f, &at).is_some() => return door(app, f, &at).map(|c| App::enter(&c)),
        KeyCode::Enter | KeyCode::Char(' ') if !ctrl && !alt => {
            if w.heads.get(place).is_some_and(|h| h.2.is_some()) {
                toggle(app, f, &at);
            } else if let Some(ids) = w.terms.get(cur).filter(|t| !t.is_empty()) {
                // an id of the project's docs on the line: its row is said in the chat (`:term`)
                return Some(vec![Up::Intent { intent: Intent::Run { command: "term".into(), args: ids.iter().take(4).cloned().collect() } }]);
            }
        }
        KeyCode::Char('z') if !ctrl && !alt => {
            // every head that folds open, or when they all are, shut
            let folds: Vec<&(usize, String, Option<bool>, bool)> = w.heads.iter().filter(|h| h.2.is_some()).collect();
            let want = !folds.iter().all(|h| h.2 == Some(true));
            for (_, key, open, _) in folds {
                if *open != Some(want) {
                    toggle(app, f, key);
                }
            }
        }
        KeyCode::Esc | KeyCode::Char('G') => land(app, f, None),
        KeyCode::Tab | KeyCode::BackTab | KeyCode::F(_) => return None,
        KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete if !ctrl => {
            // typing is the composer's: the keys go back to it
            land(app, f, None);
            return None;
        }
        _ => {}
    }
    Some(Vec::new())
}

/// The chat the lane at row `key` is the door of (`data.lane.chat`), or the row of a chat's end (`data.door`).
fn door(app: &App, f: &Field, key: &str) -> Option<String> {
    let data = app.rows.get(&f.bind)?.iter().find(|r| r.key == key)?.data.as_ref()?;
    data.pointer("/lane/chat").or_else(|| data.get("door"))?.as_str().map(String::from)
}

impl App {
    /// The rows of the chat the keys are in, if it is drawn from rows.
    fn chat_rows(&self) -> Option<&[j4n_proto::Row]> {
        let f = self.field().filter(|f| f.kind == j4n_proto::FieldKind::Chat)?;
        self.rows.get(&f.bind).map(Vec::as_slice)
    }

    /// The lane the line would steer: the one chosen with `⌃t`, while it is still working.
    pub(crate) fn aimed(&self) -> Option<String> {
        let key = self.aim.as_ref()?;
        let rows = self.chat_rows()?;
        rows.iter().find(|r| &r.key == key && is_live(r)).map(|r| r.key.clone())
    }

    /// `⌃t`: the next lane that works, after the one chosen; after the last, the thread's own doer.
    pub(crate) fn next_aim(&mut self) {
        let Some(rows) = self.chat_rows() else { return };
        let live: Vec<&str> = rows.iter().filter(|r| is_live(r)).map(|r| r.key.as_str()).collect();
        let now = self.aimed();
        let next = match now.as_deref().and_then(|k| live.iter().position(|l| *l == k)) {
            Some(i) => live.get(i + 1).copied(),
            None => live.first().copied(),
        };
        self.aim = next.map(str::to_string);
    }

    /// The line steers the lane chosen, when one is: a steer for that task in place of a line to the thread.
    pub(crate) fn steer(&mut self, text: &str) -> Option<Vec<Up>> {
        if text.starts_with(':') || text.starts_with('/') {
            return None;
        }
        let task = self.aimed()?;
        Some(vec![Up::Intent { intent: Intent::Control { task, control: Control::Steer { text: text.to_string() } } }])
    }
}

/// A row of a lane that is working or waiting for an answer: one a line can steer.
fn is_live(r: &j4n_proto::Row) -> bool {
    matches!(r.cells.first().map(|c| c.icon.as_str()), Some("working" | "ask" | "cooling"))
}
