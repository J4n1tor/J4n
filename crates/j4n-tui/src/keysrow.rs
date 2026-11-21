//! The keys row, the last row of the screen: the keys of the page that is open, glyph first, a few words,
//! the most used first. The words are data: the core sends the rows of `keys:row` (the kind `hint`), already in the order they stand, and
//! this picks the ones that are the open page's. Which places count, nearest first: the pages on the way down to the one that has the
//! keys (`page:<id>`), the open question (`ask`), the field the page is (`chat`, `list`, `form`, `table`, `tree`, `cards`, `log`), and
//! `global`. Of one name (the part of the id after the `/`) the nearest place wins, so a page can reword or hide (an empty word) a
//! field's hint. While the numbers of the tabs show, the row is the keys of the tabs instead; when it is too narrow it drops from the right.
//! While Tab completes, the nearest place is the completion's: `ghost` while the rest of a suggestion stands after the cursor, `step`
//! while Tab steps through the list (its `⏎ keep` rewords the line's `⏎ send`).
//! `?` opens the sheet: all the hints of the page, with what each does, the keys of the completion on a chat, and the keys of the tabs.

use crate::app::App;
use crate::leader;
use crate::theme::palette::Role as Hue;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use j4n_proto::{Act, Field, FieldKind, KEYS_ROW, Row, Tone};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use crate::wrap::fit;
use unicode_width::UnicodeWidthStr;

/// A key of the row: the keys, the few words after them, and what it does in a line (for the sheet). `temp` is a key that is live only
/// now (the newest note's act, the selected row's own keys) and is drawn by `Theme::temp_hint`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hint {
    pub keys: String,
    pub word: String,
    pub doc: String,
    pub temp: bool,
}

/// How a key of a row or a note reads in the row of keys: `enter` is `⏎`, a space is `space`.
fn key_glyph(key: &str) -> String {
    match key {
        "enter" => "⏎".into(),
        " " => "space".into(),
        k => k.into(),
    }
}

/// The gap between two hints.
const GAP: &str = "  ";

/// The name a kind of field has as a place of hints.
fn place_of(kind: FieldKind) -> Option<String> {
    match kind {
        FieldKind::Bar | FieldKind::Ask | FieldKind::Action | FieldKind::Stat | FieldKind::Meter | FieldKind::Spark | FieldKind::Line | FieldKind::Bars | FieldKind::Chip => None,
        FieldKind::Card | FieldKind::Cards => Some("cards".into()),
        FieldKind::Chat => Some("chat".into()),
        FieldKind::Log => Some("log".into()),
        FieldKind::List => Some("list".into()),
        FieldKind::Table => Some("table".into()),
        FieldKind::Tree => Some("tree".into()),
        _ => Some("form".into()),
    }
}

impl App {
    /// The places whose hints the open page shows, the nearest first.
    fn hint_places(&self) -> Vec<String> {
        // the keys are on the list of the chats: its own few keys, nothing of the line's
        if self.pin.is_some() {
            return vec!["pick".into()];
        }
        let kind = self.field().map(|f| f.kind);
        // the completion's keys are the nearest while it is at work in the line
        let complete = if kind != Some(FieldKind::Chat) || !self.composing() {
            None
        } else if self.suggest.step.is_some() {
            Some("step")
        } else {
            self.ghost().map(|_| "ghost")
        };
        let mut out: Vec<String> = complete.map(String::from).into_iter().collect();
        out.extend(self.trail().iter().rev().map(|(n, _)| format!("page:{}", n.id)));
        if kind == Some(FieldKind::Chat) && self.open_ask().is_some() {
            out.push("ask".into());
        }
        // `←` goes back to the orchestrator from a chat under it, while the line is empty
        if kind == Some(FieldKind::Chat) && self.line.buf.is_empty() && self.here().is_some_and(|h| !h.top) {
            out.push("sub".into());
        }
        // a lane that is over, or a chat's end, can be opened with the keys: say how
        if kind == Some(FieldKind::Chat) && self.has_door() {
            out.push("lanes".into());
        }
        out.extend(kind.and_then(place_of));
        out.push("global".into());
        out
    }

    /// Whether the open chat shows what the keys can open: a chat's end (its preview opens in place), or a turn with several lanes of which one is over
    /// (it folds, and its steps open).
    fn has_door(&self) -> bool {
        let Some(rows) = self.field().and_then(|f| self.rows.get(&f.bind)) else { return false };
        let lane = |r: &&Row| r.data.as_ref().is_some_and(|d| d.get("lane").is_some());
        let over = |r: &Row| r.cells.first().is_some_and(|c| matches!(c.icon.as_str(), "done" | "failed" | "stopped"));
        rows.iter().any(|r| r.data.as_ref().is_some_and(|d| d.get("door").is_some()))
            || rows.iter().filter(lane).any(|r| over(r) && rows.iter().filter(lane).filter(|o| o.parent == r.parent).count() > 1)
    }

    /// The keys that are live only now, first in the row: the act of the chat's newest note while the line is empty and no
    /// question waits (a bare `⏎` does it), and the actions of the selected row of a list or a table. A key with no word is not shown.
    fn temp_hints(&self) -> Vec<Hint> {
        if self.pin.is_some() {
            return Vec::new();
        }
        let field = self.field();
        let doc = if field.is_some_and(|f| f.kind == FieldKind::Chat) { "Now: the key of the newest note." } else { "Now: a key of the row under the cursor." };
        let acts: Vec<Act> = match field {
            Some(f) if f.kind == FieldKind::Chat && self.line.buf.trim().is_empty() && self.open_ask().is_none() => self.newest_act().into_iter().collect(),
            Some(f) if matches!(f.kind, FieldKind::List | FieldKind::Table | FieldKind::Tree) => {
                // a list's cursor is its place, a table's and a tree's the selected row; a tree row with rows under it folds with `⏎` and `space`
                let all = self.rows.get(&f.bind).map_or(&[][..], Vec::as_slice);
                let row = if f.kind == FieldKind::List { all.get(self.place(&f.bind)) } else { self.row_at(&f.bind, self.selected(f)) };
                let folds = |r: &Row| f.kind == FieldKind::Tree && all.iter().any(|o| o.parent.as_deref() == Some(r.key.as_str()));
                row.map(|r| r.actions.iter().filter(|a| !(folds(r) && matches!(a.key.as_str(), "enter" | " "))).cloned().collect()).unwrap_or_default()
            }
            _ => Vec::new(),
        };
        acts.into_iter().filter(|a| !a.label.is_empty()).map(|a| Hint { keys: key_glyph(&a.key), word: a.label, doc: doc.into(), temp: true }).collect()
    }

    /// The hints of the open page, in the order the core sent them; of one name the nearest place's, and none with no word. The keys that
    /// are live only now stand first, and a page's hint of the same keys gives way to them (the nearest wins).
    pub fn hints(&self) -> Vec<Hint> {
        let page = self.page_hints();
        // the sheet keeps what the page says of the key (`⏎ change`: "a switch flips, words open their picker"); the row's word is the row's own
        let temp = self.temp_hints().into_iter().map(|t| match page.iter().find(|h| h.keys == t.keys && !h.doc.is_empty()) {
            Some(h) => Hint { doc: h.doc.clone(), ..t },
            None => t,
        });
        let temp: Vec<Hint> = temp.collect();
        let rest = page.into_iter().filter(|h| !temp.iter().any(|t| t.keys == h.keys));
        temp.iter().cloned().chain(rest).collect()
    }

    /// The hints of the places of the open page (`keys:row`), without the keys that are live only now.
    fn page_hints(&self) -> Vec<Hint> {
        self.hints_at(&self.hint_places())
    }

    /// The hints of `places`, the nearest first: of one name the nearest place's, and none with no word.
    fn hints_at(&self, places: &[String]) -> Vec<Hint> {
        let rows: &[Row] = self.rows.get(KEYS_ROW).map_or(&[], Vec::as_slice);
        let at = |r: &Row| r.key.split_once('/').and_then(|(p, _)| places.iter().position(|x| x == p));
        let name = |r: &Row| r.key.split_once('/').map(|(_, n)| n.to_string()).unwrap_or_default();
        let mine: Vec<&Row> = rows.iter().filter(|r| at(r).is_some()).collect();
        mine.iter()
            .filter(|r| mine.iter().filter(|o| name(o) == name(r)).all(|o| at(o) >= at(r)))
            .filter(|r| r.cells.get(1).is_some_and(|w| !w.text.is_empty()))
            .filter(|r| !r.key.starts_with("page:") || !self.row_lacks(&r.cells[0].text))
            .map(|r| Hint { keys: r.cells[0].text.clone(), word: r.cells[1].text.clone(), doc: r.detail.first().map(|d| d.text.clone()).unwrap_or_default(), temp: false })
            .collect()
    }

    /// Whether the key of a page's hint is a row's key that the selected row has not: `u` undo on a row with nothing to take back.
    /// A key no row of the list carries is the page's own and is always shown.
    fn row_lacks(&self, keys: &str) -> bool {
        let Some(f) = self.field().filter(|f| matches!(f.kind, FieldKind::List | FieldKind::Table | FieldKind::Form)) else { return false };
        let key = match keys {
            "⏎" => "enter",
            "space" => " ",
            k => k,
        };
        let Some(rows) = self.rows.get(&f.bind) else { return false };
        let has = |r: &Row| r.actions.iter().any(|a| a.key == key);
        rows.iter().any(has) && !rows.get(self.place(&f.bind)).is_some_and(has)
    }

    /// The row as it is drawn now: the hints of the page, or the keys of the tabs while their numbers show.
    fn row_hints(&self) -> Vec<Hint> {
        if self.numbers_shown() {
            return leader::KEYS.iter().filter(|r| r.row).map(|r| Hint { keys: r.keys.into(), word: r.words.into(), ..Hint::default() }).collect();
        }
        self.hints()
    }

    /// Whether a key opens or closes the sheet; the sheet shuts at any key.
    pub(crate) fn sheet_key(&mut self, k: &KeyEvent) -> bool {
        if self.sheet {
            self.sheet = false;
            self.redraw = true;
            return true;
        }
        let kind = self.field().map(|f| f.kind);
        let free = kind != Some(FieldKind::Chat) || (self.line.buf.is_empty() && self.open_ask().is_none());
        if k.code == KeyCode::Char('?') && !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) && !self.typing() && free && self.rows.contains_key(KEYS_ROW) {
            self.sheet = true;
            return true;
        }
        false
    }
}

/// The binding of the status bar that shares the row with the keys.
pub const STATUS: &str = "bar:status";

/// Whether a part is the keys row that has no row of its own: it stands in the row of the part before it (`share` in the props of its field).
pub fn shared(n: &j4n_proto::Node) -> bool {
    n.field.as_ref().is_some_and(|f| f.bind == KEYS_ROW && f.props.get("share").and_then(|v| v.as_bool()) == Some(true))
}

impl App {
    /// Whether the state is news: work runs or something waits for the operator (the core says it in the tone of the status row), and the
    /// operator is not in the middle of a line.
    fn bar_news_now(&self) -> bool {
        let news = self.rows.get(STATUS).and_then(|r| r.first()).is_some_and(|r| matches!(r.tone, Tone::Accent | Tone::Warn));
        news && self.line.buf.is_empty() && self.edit.is_none() && self.val.is_none() && self.jump.is_none()
    }

    /// Which face the shared row shows: the state while it is news, the keys otherwise, and what the operator chose by hand over that
    /// until the news changes. The state also shows while there are no keys to show.
    pub(crate) fn state_shown(&mut self) -> bool {
        let news = self.bar_news_now();
        if news != self.bar_news {
            (self.bar_news, self.bar_hold) = (news, None);
        }
        self.bar_hold.unwrap_or(news) || self.hints().is_empty()
    }

    /// The key that turns the row over by hand.
    pub(crate) fn flip_bar(&mut self) {
        let shown = self.state_shown();
        self.bar_hold = Some(!shown);
        self.redraw = true;
    }
}

/// The shared row, drawn: the state bar or the keys (see [`App::state_shown`]); with the state, the key that brings the keys back stands at
/// the right edge when there is room.
pub fn pair(app: &mut App, bar: &j4n_proto::Node, keys: &j4n_proto::Node, d: usize, area: Rect, fr: &mut Frame) {
    if !app.state_shown() {
        return crate::nodes::draw(app, keys, d, area, fr);
    }
    let back = app.hints().into_iter().find(|h| h.word == "state").map(|h| format!("{} keys ", h.keys));
    let Some(back) = back.filter(|b| usize::from(area.width) > b.width() + 24) else { return crate::nodes::draw(app, bar, d, area, fr) };
    let w = back.width() as u16;
    crate::nodes::draw(app, bar, d, Rect { width: area.width - w, ..area }, fr);
    let th = &app.theme;
    let style = if th.no_color { Style::default().add_modifier(Modifier::REVERSED) } else { th.back(Hue::Raise) };
    fr.render_widget(Paragraph::new(Span::styled(back, th.tone(Tone::Dim))).style(style), Rect { x: area.right() - w, width: w, ..area });
}

/// The row: the hints in one line, as many as fit.
pub fn draw(app: &App, _f: &Field, area: Rect, fr: &mut Frame) {
    let th = &app.theme;
    let hints = app.row_hints();
    let room = usize::from(area.width).saturating_sub(1);
    let mut spans = vec![Span::raw(" ")];
    let mut used = 0;
    for (i, h) in hints.iter().enumerate() {
        let (mark, tstyle) = th.temp_hint();
        let lead = if h.temp { mark.width() + 1 } else { 0 };
        let word = if h.word.is_empty() { String::new() } else { format!(" {}", h.word) };
        let w = lead + h.keys.width() + word.width();
        let gap = if i == 0 { 0 } else { GAP.len() };
        if used + gap + w > room && i > 0 {
            break;
        }
        if i > 0 {
            spans.push(Span::raw(GAP));
        }
        let (keys, wstyle) = if h.temp { (tstyle, tstyle) } else { (th.tone(Tone::Plain), th.tone(Tone::Dim)) };
        if h.temp {
            spans.push(Span::styled(format!("{mark} "), tstyle));
        }
        spans.push(Span::styled(h.keys.clone(), keys));
        spans.push(Span::styled(word, wstyle));
        used += gap + w;
    }
    fr.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The sheet: every hint of the page with what it does, and the keys of the tabs, each key once; in a box over the screen, shut by any key.
/// A description longer than its column goes on under itself, and the keys and words stand in columns of their own.
pub fn draw_sheet(app: &App, area: Rect, f: &mut Frame) {
    if !app.sheet || area.width < 16 || area.height < 5 {
        return;
    }
    let th = &app.theme;
    // the page the keys are on may have no title of its own (a list under a tab): the nearest title on the way down is its name
    let title = app.trail().iter().rev().map(|(n, _)| n.title.clone()).find(|t| !t.is_empty()).unwrap_or_else(|| app.name.clone());
    let w = area.width.saturating_sub(2).min(64);
    let iw = usize::from(w) - 4;
    // the columns are as wide as their widest entry and a space, and no wider than a quarter and a third of the room
    let mut hints = app.hints();
    // on a chat, the keys of the completion too, whatever the line holds now; a key the page lists already is not listed again
    if app.field().is_some_and(|f| f.kind == FieldKind::Chat) {
        for h in app.hints_at(&["ghost".into(), "step".into()]) {
            if !hints.iter().any(|o| o.keys == h.keys) {
                hints.push(h);
            }
        }
    }
    let widest = |of: &dyn Fn(&Hint) -> usize, tab: &dyn Fn(&leader::Row) -> usize| hints.iter().map(of).chain(leader::KEYS.iter().map(tab)).max().unwrap_or(0) + 1;
    let (kw, ww) = (widest(&|h| h.keys.width(), &|r| r.keys.width()).max(8).min(iw / 4), widest(&|h| h.word.width(), &|r| r.words.width()).max(10).min(iw / 3));
    let pad = |s: &str, to: usize| crate::wrap::pad(&fit(s, to.saturating_sub(1)), to);
    let mut lines = vec![Line::raw("")];
    let line = |keys: &str, word: &str, doc: &str, lines: &mut Vec<Line>| {
        let docs = if doc.is_empty() { vec![Vec::new()] } else { crate::read::spans(th, &app.read, th.tone(Tone::Dim), doc, iw.saturating_sub(kw + ww).max(1)) };
        for (i, d) in docs.into_iter().enumerate() {
            let (k, wd) = if i == 0 { (pad(keys, kw), pad(word, ww)) } else { (" ".repeat(kw), " ".repeat(ww)) };
            lines.push(Line::from([vec![Span::styled(k, th.fg(Hue::Accent)), Span::styled(wd, th.tone(Tone::Plain))], d].concat()));
        }
    };
    for h in &hints {
        line(&h.keys, &h.word, &h.doc, &mut lines);
    }
    lines.push(Line::raw(""));
    // the page's word for a key wins: a key of the tabs the page already lists is not listed again
    for r in leader::KEYS.iter().filter(|r| !hints.iter().any(|h| h.keys == r.keys)) {
        line(r.keys, r.words, r.doc, &mut lines);
    }
    let h = (lines.len() as u16 + 3).min(area.height);
    let r = Rect { x: area.x + (area.width - w) / 2, y: area.y + area.height.saturating_sub(h) / 2, width: w, height: h };
    let inside = crate::parts::popup::boxed(th, f, r, &format!("keys · {title}"));
    lines.truncate(usize::from(inside.height).saturating_sub(1));
    let closes = "any key closes";
    lines.push(Line::styled(format!("{}{closes}", " ".repeat(iw.saturating_sub(closes.width()))), th.tone(Tone::Dim)));
    f.render_widget(Paragraph::new(lines), inside);
}
