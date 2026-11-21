//! The chats on the screen: the first row of the chat is a crumb (`j4n › c3 fix trailing commas`, the project's
//! name in the first cell, no word `main`), and in the orchestrator it is followed by the pinned list of the chats, one row each, a plan's branches
//! nested under their chat, the ones that wait first (the core sends them in that order). The keys go to the list with `⌃↑` as they go to the turns:
//! `↑` `↓` move, `⏎` goes in, `esc` or a letter gives the keys back to the composer. `esc` on an empty line goes up one level; `⇥1` steps to the
//! next chat that waits, then the next that runs. From an empty line `↓` puts the keys on the list too; a few seconds with no key hand them back,
//! `↑` on the first chat does too, and `⇥` goes in as `⏎` does. All of it is drawn only when the core sends `chat:here` (the switch `chats.on`).

use crate::app::App;
use crate::parts::scroll::keep;
use crate::display::layout::{self, Item, Laid, Look, Piece};
use crate::display::{Spot, context_gauge, fill_role, join, percent, slice_for, thin, whole, width_of};
use crate::theme::Theme;
use crate::theme::glyph::Slot;
use crate::theme::palette::Role as Hue;
use crate::wrap::fit;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use j4n_proto::{CHAT_HERE, CHATS, Intent, Json, Row, Tone, Up};
use ratatui::text::{Line, Span};
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthStr;

/// How long the list keeps the keys with none pressed.
pub(crate) const IDLE: Duration = Duration::from_secs(4);

/// Where the screen is, as `chat:here` says it.
pub(crate) struct Here {
    pub top: bool,
    /// `(id, title)` of each level from the first chat down.
    pub crumb: Vec<(String, String)>,
    pub state: String,
    pub icon: String,
    pub chats: u64,
    pub running: u64,
    pub waits: u64,
    /// The windows of the providers in use, as the core words them, for the orchestrator's row: `(doer, window, used, resets)`.
    pub windows: Vec<(String, String, f64, String)>,
    pub today: String,
    /// The gauge is one circle slice (`look.gauge`).
    pub slice: bool,
}

impl Here {
    pub(crate) fn of(v: &Json) -> Option<Here> {
        if v.is_null() {
            return None;
        }
        let text = |k: &str| v.get(k).and_then(Json::as_str).unwrap_or_default().to_string();
        let num = |k: &str| v.get(k).and_then(Json::as_u64).unwrap_or(0);
        let crumb = v.get("crumb").and_then(Json::as_array).map(|l| l.iter().map(|c| (c["id"].as_str().unwrap_or_default().to_string(), c["title"].as_str().unwrap_or_default().to_string())).collect()).unwrap_or_default();
        let windows = v.get("windows").and_then(Json::as_array).map(|l| l.iter().map(|w| (w["doer"].as_str().unwrap_or_default().to_string(), w["window"].as_str().unwrap_or_default().to_string(), w["used"].as_f64().unwrap_or(0.0), w["resets"].as_str().unwrap_or_default().to_string())).collect()).unwrap_or_default();
        Some(Here { top: v.get("top").and_then(Json::as_bool).unwrap_or(true), crumb, state: text("state"), icon: text("icon"), chats: num("chats"), running: num("running"), waits: num("waits"), windows, today: text("today"), slice: slice_for(&text("form"), Spot::Strip) })
    }
}

impl App {
    /// Where the screen is among the chats; none while the switch is off (the core sends `null`) or before it has said.
    pub(crate) fn here(&self) -> Option<Here> {
        self.values.get(CHAT_HERE).and_then(Here::of)
    }

    /// The rows of the pinned list.
    pub(crate) fn pinned(&self) -> &[Row] {
        self.rows.get(CHATS).map_or(&[][..], Vec::as_slice)
    }

    /// The core said where the screen is: a screen that moved to another chat starts its transcript over.
    pub(crate) fn here_changed(&mut self, now: &Json) {
        let was = self.values.get(CHAT_HERE).and_then(|v| v.get("id")).and_then(Json::as_str).unwrap_or_default().to_string();
        let is = now.get("id").and_then(Json::as_str).unwrap_or_default();
        if was != is {
            for bind in self.chats.clone() {
                self.scroll.remove(&bind);
                self.wins.remove(&bind);
            }
            (self.aim, self.pin) = (None, None);
        }
    }

    /// The `⏎` of the list or of a lane that is a door: go into the chat.
    pub(crate) fn enter(chat: &str) -> Vec<Up> {
        vec![Up::Intent { intent: Intent::Enter { chat: chat.into() } }]
    }

    /// The clock moved on: the list hands the keys back once it has waited [`IDLE`] for one.
    pub(crate) fn pin_idle(&mut self, now: Instant) {
        let Some(_) = self.pin else { return self.pin_since = None };
        let since = *self.pin_since.get_or_insert(now);
        if now.saturating_duration_since(since) >= IDLE {
            (self.pin, self.pin_since, self.redraw) = (None, None, true);
        }
    }

    /// How long until the list hands the keys back, while it has them.
    pub(crate) fn pin_wait(&self, now: Instant) -> Option<Duration> {
        self.pin?;
        Some((self.pin_since? + IDLE).saturating_duration_since(now))
    }
}

/// A mark of the theme, by the name of its slot, in its own colour.
fn mark(th: &Theme, name: &str) -> Span<'static> {
    match Slot::by_name(name) {
        Some(slot) => {
            let (g, style) = th.mark(slot);
            Span::styled(g, style)
        }
        None => Span::raw("  "),
    }
}

/// The crumb row: the project's name, then the chats from the first level down, the last one kept when the row is short; at the right the counts of
/// the orchestrator, or the state of the chat.
fn crumb(app: &App, h: &Here, width: usize) -> Line<'static> {
    let th = &app.theme;
    let project = if app.project.is_empty() { app.name.clone() } else { app.project.clone() };
    let right: Vec<Span<'static>> = if h.top {
        let mut parts = vec![if h.chats == 1 { "1 chat".to_string() } else { format!("{} chats", h.chats) }];
        if h.running > 0 {
            parts.push(format!("{} running", h.running));
        }
        if h.waits > 0 {
            parts.push(if h.waits == 1 { "1 waits".to_string() } else { format!("{} wait", h.waits) });
        }
        let tone = if h.waits > 0 { Tone::Warn } else { Tone::Dim };
        vec![Span::styled(parts.join(" · "), th.tone(tone))]
    } else {
        let mut s = vec![mark(th, &h.icon)];
        s.push(Span::styled(h.state.clone(), th.tone(Tone::Dim)));
        s
    };
    let room = width.saturating_sub(width_of(&right) + 4 + project.width());
    // every level in full while it fits; else the earlier levels shrink to their ids, and the last one is cut
    let level = |(id, title): &(String, String), full: bool| if full && !title.is_empty() { format!("{id} {title}") } else { id.clone() };
    let mut shown: Vec<String> = h.crumb.iter().map(|c| level(c, true)).collect();
    let used = |s: &[String]| s.iter().map(|t| t.width() + 3).sum::<usize>();
    let mut at = 0;
    while used(&shown) > room && at + 1 < h.crumb.len() {
        shown[at] = level(&h.crumb[at], false);
        at += 1;
    }
    if used(&shown) > room
        && let Some(last) = shown.last_mut()
    {
        let others = h.crumb.len().saturating_sub(1);
        *last = fit(last, room.saturating_sub(used(&[]) + others * 6 + 3).max(6));
    }
    let mut spans = vec![Span::raw(" "), Span::styled(project, th.fg(Hue::Text).add_modifier(ratatui::style::Modifier::BOLD))];
    for s in shown {
        spans.push(Span::styled(" › ", th.tone(Tone::Dim)));
        spans.push(Span::styled(s, th.tone(Tone::Plain)));
    }
    let gap = width.saturating_sub(width_of(&spans) + width_of(&right) + 1);
    spans.push(Span::raw(" ".repeat(gap)));
    spans.extend(right);
    Line::from(spans)
}

/// One row of the list (`laid` is its place in the tree, from [`layout::lay`]): the pipes, the state mark, the id and the title at the left, then at the right `doer · model`, how full the context is (the gauge,
/// `g3` or `g9`), what it cost and the time or the word; what does not fit goes from the left of the right group first.
fn row(app: &App, r: &Row, laid: &Laid, width: usize, on: bool, slice: bool) -> Line<'static> {
    let th = &app.theme;
    let mut spans = vec![Span::raw(" "), Span::styled(Look::new(th).prefix(laid), th.fg(Hue::Edge))];
    // a chat that is over ends in how long ago, from the screen's clock
    let aged = app.aged(std::slice::from_ref(r));
    let cell = |n: usize| aged[0].cells.get(n).cloned().unwrap_or_default();
    spans.push(mark(th, &cell(0).icon));
    spans.push(Span::styled(format!("{} ", cell(1).text), th.fg(Hue::Faint)));
    // the right group: who, the gauge, the cost, the time or the word
    let text = |n: usize| Some(cell(n)).filter(|c| !c.text.is_empty()).map(|c| vec![Span::styled(c.text, th.tone(c.tone))]);
    let window = r.data.as_ref().and_then(|d| d.get("window")).and_then(Json::as_f64);
    let mut right: Vec<Vec<Span<'static>>> = [text(3), window.map(|w| context_gauge(th, w, slice)), text(5), text(6)].into_iter().flatten().collect();
    let lead = width_of(&spans);
    let want = 14usize;
    let size = |g: &[Vec<Span<'static>>]| g.iter().map(|p| width_of(p) + 2).sum::<usize>();
    while !right.is_empty() && lead + want + size(&right) + 1 > width {
        right.remove(0);
    }
    let room = width.saturating_sub(lead + size(&right) + 1);
    let title = cell(2);
    spans.push(Span::styled(fit(&title.text, room), th.tone(title.tone)));
    let gap = width.saturating_sub(width_of(&spans) + size(&right) + 1);
    spans.push(Span::raw(" ".repeat(gap)));
    for part in right {
        spans.push(Span::raw("  "));
        spans.extend(part);
    }
    let line = Line::from(spans);
    if on { line.style(th.back(Hue::Sel)) } else { line }
}

/// The windows row of the orchestrator: each provider's windows in use, the name once for a provider, the gauge (a circle slice, or the thin line) and the number, and
/// when the time it resets once it is near its limit; what today has cost stands at the right. What does not fit goes from the right: `today` first.
fn windows_row(app: &App, h: &Here, width: usize) -> Option<Line<'static>> {
    let th = &app.theme;
    if h.windows.is_empty() && h.today.is_empty() {
        return None;
    }
    let dim = th.tone(Tone::Dim);
    let mut parts: Vec<Vec<Span<'static>>> = Vec::new();
    let mut seen = "";
    for (doer, window, used, resets) in &h.windows {
        let used = whole(*used);
        let mut p = Vec::new();
        if doer != seen {
            p.push(Span::styled(format!("{doer} "), th.tone(Tone::Plain)));
            seen = doer;
        }
        let (hue, tone) = fill_role(used);
        p.push(Span::styled(format!("{window} "), dim));
        if h.slice {
            p.extend(context_gauge(th, used, true));
        } else {
            p.extend(thin(th, used, 6, th.fg(hue)));
            p.push(Span::styled(format!(" {}", percent(used).trim_end()), th.tone(tone)));
        }
        if used >= 0.8 && !resets.is_empty() {
            p.push(Span::styled(format!(" resets {resets}"), dim));
        }
        parts.push(p);
    }
    let today = (!h.today.is_empty()).then(|| vec![Span::styled(format!("today {}", h.today), dim)]);
    let windows = |n: usize| join(parts[..n].to_vec(), "   ");
    let mut n = parts.len();
    // the windows give up from the last; `today` goes first when the row is short
    let fits = |n: usize, with_today: bool| 1 + width_of(&windows(n)) + if with_today { today.as_ref().map_or(0, |t| width_of(t) + 3) } else { 0 } < width;
    let with_today = today.is_some() && fits(n, true);
    while n > 0 && !fits(n, with_today) {
        n -= 1;
    }
    let mut spans = vec![Span::raw(" ")];
    spans.extend(windows(n));
    if with_today && let Some(t) = today {
        let gap = width.saturating_sub(width_of(&spans) + width_of(&t) + 1);
        spans.push(Span::raw(" ".repeat(gap)));
        spans.extend(t);
    }
    Some(Line::from(spans))
}

/// One entry of the pinned list as it is shown: a chat's row (its index in the rows), or one of the two fold rows that end it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Entry {
    Row(usize),
    Done,
    Archive,
}

/// Whether a row's data carries `key` (`old`: done for a while, `archived`).
fn flagged(r: &Row, key: &str) -> bool {
    r.data.as_ref().and_then(|d| d.get(key)).and_then(Json::as_bool) == Some(true)
}

/// The keys of the two fold rows, as items of the layout (no chat has one: a chat's key starts with a letter).
const DONE: &str = "\0done";
const ARCHIVE: &str = "\0archive";

/// What the pinned list shows, in the order it is read as a tree: the chats that are not folded, then the fold row of the chats done before, with the chats it
/// holds under it when the operator opened it, then the row `archived (N)` that ends the list, with the archived chats under it when it is open.
fn shown(app: &App) -> Vec<Entry> {
    let rows = app.pinned();
    let of = |keep: &dyn Fn(&Row) -> bool| rows.iter().enumerate().filter(|(_, r)| keep(r)).map(|(i, _)| Entry::Row(i)).collect::<Vec<_>>();
    let mut out = of(&|r| !flagged(r, "archived") && !flagged(r, "old"));
    if rows.iter().any(|r| flagged(r, "old") && !flagged(r, "archived")) {
        out.push(Entry::Done);
        if app.done_open {
            out.extend(of(&|r| flagged(r, "old") && !flagged(r, "archived")));
        }
    }
    if rows.iter().any(|r| flagged(r, "archived")) {
        out.push(Entry::Archive);
        if app.archive_open {
            out.extend(of(&|r| flagged(r, "archived")));
        }
    }
    out
}

/// The shown entries as the items of the layout: a fold row is a top item, and the top chats it holds hang under it.
fn items<'a>(app: &App, rows: &'a [Row], list: &[Entry]) -> Vec<Item<'a>> {
    list.iter()
        .map(|e| match *e {
            Entry::Done => Item::fold(DONE, None, app.done_open),
            Entry::Archive => Item::fold(ARCHIVE, None, app.archive_open),
            Entry::Row(i) => {
                let r = &rows[i];
                let hold = if flagged(r, "archived") { ARCHIVE } else { DONE };
                Item::new(&r.key, r.parent.as_deref().or((flagged(r, "archived") || flagged(r, "old")).then_some(hold)))
            }
        })
        .collect()
}

/// How many top chats a fold row holds: the ones done before, or the archived ones.
fn held(app: &App, key: &str) -> usize {
    app.pinned().iter().filter(|r| r.parent.is_none() && flagged(r, key)).count()
}

/// A fold row: `4 done before ▸` on an elbow, the way a list ends when chats that are done have been folded away, and `archived (2) ▸` after it (`▾` while open); it is
/// an item of the tree like a chat, so only the last of the list has the elbow.
fn fold_row(app: &App, laid: &Laid, width: usize, on: bool, which: Entry) -> Line<'static> {
    let th = &app.theme;
    let look = Look::new(th);
    let label = if which == Entry::Archive { format!("archived ({})", held(app, "archived")) } else { format!("{} done before", held(app, "old")) };
    let mut spans = vec![Span::raw(" "), Span::styled(look.prefix(laid), th.fg(Hue::Edge)), mark(th, if which == Entry::Archive { "" } else { "done" })];
    spans.push(Span::styled(label, th.tone(Tone::Dim)));
    let tail = vec![Span::raw(" "), Span::styled(look.fold(laid.fold.unwrap_or(Piece::FoldClosed)).to_string(), th.fg(Hue::Faint))];
    let gap = width.saturating_sub(width_of(&spans) + width_of(&tail) + 1);
    spans.push(Span::raw(" ".repeat(gap)));
    spans.extend(tail);
    let line = Line::from(spans);
    if on { line.style(th.back(Hue::Sel)) } else { line }
}

/// The header of a chat: the crumb row, and in the orchestrator the windows row, the pinned list and a blank row under it. At most `max` rows; a list that
/// is longer ends in a row that says how many are left out.
pub(crate) fn head(app: &App, width: usize, max: usize) -> Vec<Line<'static>> {
    let Some(h) = app.here() else { return Vec::new() };
    let mut out = vec![crumb(app, &h, width)];
    if !h.top || max < 2 {
        return out;
    }
    out.extend(windows_row(app, &h, width).filter(|_| max > 3));
    let rows = app.pinned();
    if rows.is_empty() {
        out.push(Line::styled(" no chats yet · the first line opens one", app.theme.tone(Tone::Dim)));
        out.push(Line::raw(""));
        return out;
    }
    let list = shown(app);
    let room = max.saturating_sub(out.len() + 1);
    let at = app.pin.unwrap_or(0).min(list.len().saturating_sub(1));
    // the rows in sight keep the selected one in them
    let from = keep(0, at as u64, at as u64, room as u64) as usize;
    let laid = layout::lay(&items(app, rows, &list));
    for (i, item) in list.iter().enumerate().take(from + room).skip(from) {
        let on = app.pin == Some(i);
        out.push(match *item {
            Entry::Row(r) => row(app, &rows[r], &laid[i], width, on, h.slice),
            fold => fold_row(app, &laid[i], width, on, fold),
        });
    }
    let left = list.len().saturating_sub(room);
    if left > 0 {
        out.push(Line::styled(format!(" {} {left} more", Look::new(&app.theme).more()), app.theme.tone(Tone::Dim)));
    }
    out.push(Line::raw(""));
    out
}

/// The chat's row the cursor is on, not a fold row.
fn selected<'a>(app: &'a App, list: &[Entry], at: usize) -> Option<&'a Row> {
    match list.get(at) {
        Some(Entry::Row(i)) => app.pinned().get(*i),
        _ => None,
    }
}

/// A key while the keys are on the list; `None` for a key the composer takes.
pub(crate) fn key(app: &mut App, k: KeyEvent) -> Option<Vec<Up>> {
    let at = app.pin?;
    // the rest of a wheel notch is the composer's
    if matches!(k.code, KeyCode::Up | KeyCode::Down) && app.burst(k.code == KeyCode::Up) {
        return None;
    }
    app.pin_since = app.clock;
    let list = shown(app);
    let n = list.len();
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    match k.code {
        KeyCode::Up if at == 0 => app.pin = None,
        KeyCode::Up => app.pin = Some(at - 1),
        KeyCode::Down if at + 1 >= n => app.pin = None,
        KeyCode::Down => app.pin = Some(at + 1),
        // `⏎` on a fold shows the chats it holds, or folds them again; on a chat it goes in
        KeyCode::Enter | KeyCode::Char(' ') if matches!(list.get(at), Some(Entry::Done | Entry::Archive)) => {
            let which = list[at];
            if which == Entry::Done { app.done_open = !app.done_open } else { app.archive_open = !app.archive_open }
            // the list changed length: the cursor stays on the fold row
            app.pin = shown(app).iter().position(|e| *e == which);
        }
        // `a` archives the chat the cursor is on, or takes it out of the archive: the one letter the list keeps
        KeyCode::Char('a') if !ctrl && selected(app, &list, at).is_some() => {
            let r = selected(app, &list, at).expect("checked");
            let word = if flagged(r, "archived") { "unarchive" } else { "archive" };
            return Some(vec![Up::Intent { intent: Intent::Say { text: format!(":{word} {}", r.key), with: Vec::new() } }]);
        }
        KeyCode::Enter => {
            let chat = selected(app, &list, at).map(|r| r.key.clone());
            app.pin = None;
            return chat.map(|c| App::enter(&c));
        }
        KeyCode::Esc => app.pin = None,
        KeyCode::Tab | KeyCode::BackTab | KeyCode::F(_) => return None,
        KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete if !ctrl => {
            // typing is the composer's
            app.pin = None;
            return None;
        }
        _ => {}
    }
    Some(Vec::new())
}

/// `⌃↑` past the first turn: the keys go to the list, when the orchestrator has one.
pub(crate) fn up_into(app: &mut App) -> bool {
    if app.here().is_some_and(|h| h.top) && !shown(app).is_empty() {
        (app.pin, app.pin_since) = (Some(0), app.clock);
        return true;
    }
    false
}
