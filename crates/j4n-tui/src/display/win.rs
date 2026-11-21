//! What the screen keeps of a binding of rows: which row the keys are on, how the core is asked to sort and filter them, and which
//! window of them it holds. A binding may have a million rows; the screen asks for the ones around the row it is on (`Up::Window`)
//! and draws the ones it was sent, so what it holds is never more than three screens.

use super::windowed;
use crate::app::App;
use crate::parts::{line::Typed, select::Sel};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use j4n_proto::{Field, FieldKind, Row, Up};
use std::collections::{BTreeMap, BTreeSet};

/// The view state of one binding of rows.
#[derive(Debug, Clone, Default)]
pub struct Win {
    /// The row the keys are on (`at`: its place among all the rows of the binding, in the order the screen asked for) and the first row
    /// shown (`top`, which the screen moves as far as the row needs to be in sight).
    pub cur: Sel,
    /// The operator moved: a log that follows its end no longer does.
    pub moved: bool,
    /// The column the rows are sorted by, with a `-` when the other way; empty is the core's own order.
    pub sort: String,
    /// What the rows must match; empty is all of them.
    pub find: String,
    /// The find being typed.
    pub typing: Option<Typed>,
    /// The keys of the rows whose fold is not the usual one: the ones opened in a log, the ones shut in a tree or a list.
    pub flip: BTreeSet<String>,
    /// What the core was last asked for: `(from, count, sort, find)`.
    pub asked: Option<(u64, u64, String, String)>,
    /// The order or the filter changed since the core was told.
    pub dirty: bool,
    /// The cards in a row, as the cards were last drawn.
    pub cols: usize,
    /// The sort the props name has been taken as the first one.
    pub started: bool,
    /// The key of the row of the transcript the keys are on, while they are on it and not in the composer.
    pub at: Option<String>,
    /// With the keys on the transcript: how many lines below the head of that row they are (the lines of a wrapped row, of its output).
    pub sub: usize,
    /// How many levels of a Log event's tree are open, by the row's key, where it is not what `all` says.
    pub depth: BTreeMap<String, u8>,
    /// Every event of the Log is open (`z`).
    pub all: bool,
    /// The rows of the Log shown as their raw JSON (`r`).
    pub raw: BTreeSet<String>,
    /// The event the keys go to once the screen holds it (`c`, its cause).
    pub goto: Option<u64>,
    /// While on the way to it: the first row it can be at, and the first row it cannot be at (past it).
    pub span: Option<(u64, u64)>,
    /// The rows held are of an order or a find that was left for the event, and the rows of the new one are not here yet.
    pub stale: bool,
    /// What to put on the terminal's clipboard at the next frame (`y`).
    pub copy: Option<String>,
    /// The lines of the row the keys are on that are scrolled above the screen, when it is open and taller than the screen.
    pub skip: usize,
    /// The width of a row's tree and the rows of the screen, as the log was last drawn.
    pub seen: (usize, usize),
}

impl Win {
    /// The rows are another set (a new find or order): from the first of them, not moved, and the core is asked again.
    pub(crate) fn rewind(&mut self) {
        (self.cur, self.dirty, self.moved) = (Sel::default(), true, false);
    }
}

impl App {
    pub(crate) fn win(&mut self, bind: &str) -> &mut Win {
        self.wins.entry(bind.into()).or_default()
    }

    /// The view state of a field, the first time with the sort its props name.
    pub(crate) fn view_of(&mut self, f: &Field) -> &mut Win {
        let w = self.wins.entry(f.bind.clone()).or_default();
        if !w.started {
            w.started = true;
            w.sort = f.props.get("sort").and_then(j4n_proto::Json::as_str).unwrap_or_default().into();
        }
        w
    }

    /// The rows the screen holds of a binding, the place of the first among all of them, and how many there are in all.
    pub(crate) fn held(&self, bind: &str) -> (&[Row], u64, u64) {
        let rows = self.rows.get(bind).map_or(&[][..], Vec::as_slice);
        let (from, total) = self.span.get(bind).copied().unwrap_or((0, rows.len() as u64));
        (rows, from, total)
    }

    /// Whether the field shows the end of its rows and stays there as they come.
    pub(crate) fn follows(&self, f: &Field) -> bool {
        f.kind == FieldKind::Log && !self.wins.get(&f.bind).is_some_and(|w| w.moved) && f.props.get("follow").and_then(j4n_proto::Json::as_bool).unwrap_or(true)
    }

    /// The row of a list or a form the keys are on, as its place among its rows.
    /// While a find is waiting on the core's answer the rows held are still all of them and only the ones that match are drawn: the keys
    /// are on the first row drawn, the one `list` puts the band on, never on one the screen hides.
    pub fn place(&self, bind: &str) -> usize {
        let Some(w) = self.wins.get(bind) else { return 0 };
        let at = w.cur.at as usize;
        if w.find.is_empty() {
            return at;
        }
        let seen = super::table::shown(self, bind);
        if seen.is_empty() || seen.contains(&at) {
            return at;
        }
        seen[0]
    }

    /// The row the keys are on.
    pub fn selected(&self, f: &Field) -> u64 {
        let total = self.held(&f.bind).2;
        let last = total.saturating_sub(1);
        if self.follows(f) { last } else { self.wins.get(&f.bind).map_or(0, |w| w.cur.at).min(last) }
    }

    /// The row with this place among all of them, when it is one the screen holds.
    pub(crate) fn row_at(&self, bind: &str, at: u64) -> Option<&Row> {
        let (rows, from, _) = self.held(bind);
        at.checked_sub(from).and_then(|i| rows.get(i as usize))
    }

    /// A screen's worth of rows, for the keys that move by a page and for how far around the row to hold.
    pub fn screenful(&self) -> u64 {
        u64::from(self.size.1).saturating_sub(4).max(8)
    }

    /// Ask the core for the window of rows around the row the keys are on, for each field on the screen that shows a window of rows, if
    /// the screen does not hold it, or the order or the filter changed. Whatever was asked is not asked twice.
    pub fn windows(&mut self) -> Vec<Up> {
        let mut fields: Vec<Field> = Vec::new();
        if let Some(root) = self.tree.clone() {
            self.shown(&root, 0, &mut |n| fields.extend(n.field.iter().filter(|f| windowed(f.kind)).cloned()));
        }
        fields.iter().filter_map(|f| self.ask_window(f)).collect()
    }

    fn ask_window(&mut self, f: &Field) -> Option<Up> {
        if !self.rows.contains_key(&f.bind) {
            return None;
        }
        self.go_on(f);
        self.view_of(f);
        let (rows, from, total) = self.held(&f.bind);
        let held = rows.len() as u64;
        let page = self.screenful();
        let sel = self.selected(f);
        let (first, last) = (sel.saturating_sub(page), (sel + page).min(total.saturating_sub(1)));
        let covers = total == 0 || (held > 0 && from <= first && from + held > last);
        let (need_first, need_last) = (first, last);
        let first = first.saturating_sub(page / 2);
        let count = 3 * page + 1;
        let w = self.win(&f.bind);
        // a window that was asked for and has not come yet is not asked for again while it would cover the rows wanted (a new order or
        // find is `dirty`, and asked for at once)
        let coming = w.asked.as_ref().is_some_and(|(af, ac, ..)| *af <= need_first && af + ac > need_last);
        let ask = (first, count, w.sort.clone(), w.find.clone());
        if !w.dirty && (covers || coming) {
            return None;
        }
        w.dirty = false;
        w.asked = Some(ask.clone());
        Some(Up::Window { bind: f.bind.clone(), from: ask.0, count: ask.1, sort: ask.2, find: ask.3 })
    }

    /// On the way to an event (`goto`): the row that is it, or the run of the same line it is in (a row holds its first event and the next
    /// `×n - 1`), when the screen holds it. Else the window is asked for where the row can be: at the place the events in sight, numbered
    /// one after the other, give it, and in a Log that folds runs (fewer rows than events) halfway between the rows known to come before
    /// the event and those known to come after it. Given up when nothing is left between them.
    pub(crate) fn go_on(&mut self, f: &Field) {
        let Some((seq, mut span)) = self.wins.get(&f.bind).filter(|w| !w.stale).and_then(|w| Some((w.goto?, w.span))) else { return };
        let (rows, from, total) = self.held(&f.bind);
        let lead = |r: &Row| r.key.strip_prefix('e').and_then(|n| n.parse::<u64>().ok());
        let count = |r: &Row| r.cells.get(3).and_then(|c| c.text.strip_prefix('×')?.parse::<u64>().ok()).unwrap_or(1);
        let at = rows.iter().position(|r| lead(r).is_some_and(|s| (s..s + count(r)).contains(&seq)));
        let (mut lo, mut hi) = span.unwrap_or((0, total));
        match (at, rows.first().and_then(lead)) {
            (None, Some(s)) if s > seq => hi = hi.min(from),
            (None, Some(_)) => lo = lo.max(from + rows.len() as u64),
            _ => {}
        }
        span = Some((lo, hi));
        let folded = rows.iter().any(|r| count(r) > 1);
        let guess = match at {
            Some(i) => Some(from + i as u64),
            None => rows.first().and_then(lead).filter(|_| !folded && seq > 0).map(|s| (from + seq).saturating_sub(s)).filter(|g| (lo..hi).contains(g)).or((lo < hi).then(|| (lo + hi) / 2)),
        };
        let sel = self.place(&f.bind) as u64;
        self.win(&f.bind).span = span;
        match guess.filter(|&to| to < total) {
            Some(to) if at.is_some() || to != sel || self.row_at(&f.bind, to).is_none() => {
                self.move_to(f, to);
                if at.is_some() {
                    self.win(&f.bind).goto = None;
                }
            }
            _ => self.win(&f.bind).goto = None,
        }
    }

    /// The row the keys are on moves to `to`; the operator moved it, so a log stops following its end.
    pub(crate) fn move_to(&mut self, f: &Field, to: u64) {
        let total = self.held(&f.bind).2;
        let w = self.win(&f.bind);
        w.cur.select(to, total);
        (w.moved, w.skip) = (true, 0);
    }
}

/// `↑↓ PgUp PgDn Home End g G` for the row the keys are on. A log that is moved to its last row follows its end again.
pub(crate) fn nav(app: &mut App, f: &Field, k: KeyEvent) -> bool {
    if k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
        return false;
    }
    let (total, page) = (app.held(&f.bind).2, app.screenful());
    let mut s = Sel { at: app.selected(f), top: 0 };
    if !s.key(k.code, page, total) {
        return false;
    }
    app.win(&f.bind).goto = None;
    app.move_to(f, s.at);
    if f.kind == FieldKind::Log && s.at + 1 >= total {
        app.win(&f.bind).moved = false;
    }
    true
}

/// The find: `/` opens it, what is typed goes into it, ⏎ sends it to the core and `esc` takes it off again. `Some` when the key was its.
pub(crate) fn finding(app: &mut App, f: &Field, k: KeyEvent) -> Option<Vec<Up>> {
    let (ctrl, alt) = (k.modifiers.contains(KeyModifiers::CONTROL), k.modifiers.contains(KeyModifiers::ALT));
    let w = app.win(&f.bind);
    if ctrl || alt {
        return None;
    }
    if let Some(t) = w.typing.as_mut() {
        match k.code {
            KeyCode::Esc => w.typing = None,
            KeyCode::Enter => {
                w.find = t.buf.trim().into();
                w.typing = None;
                w.rewind();
            }
            _ => drop(t.edit(&k)),
        }
        return Some(Vec::new());
    }
    match k.code {
        KeyCode::Char('/') => w.typing = Some(Typed::new(w.find.clone())),
        KeyCode::Esc if !w.find.is_empty() => {
            w.find.clear();
            w.rewind();
        }
        _ => return None,
    }
    Some(Vec::new())
}
