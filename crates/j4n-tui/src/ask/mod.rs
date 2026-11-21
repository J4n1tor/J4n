//! `ask`: the questions the core is waiting on. One component with three placements: inline (the row of
//! the chat where it was raised), the **popup** (one question open) and the **tray** (two or more), both square-cornered and drawn
//! above the composer. Typed words go to the question the line is for (the binder, in the core); the keys here answer it without a
//! line: a digit, `⇥` or `→` for the hint, `⏎` to open the change, `?` for why, `↑↓` to another question, `esc` to put it off.
//! An approval never shows a model's pick as the hint, and a number that widens what is allowed is pressed twice.

mod card;
mod opts;
mod paint;

pub use card::{Confirm, RISKY};
pub(crate) use card::{card, card_key, tab_count};
pub use opts::{Opt, Send, custom, front_owned, is_score, options, shape_icon};
pub(crate) use paint::inline;

use crate::app::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use j4n_proto::{AskView, Field, FieldKind, Json, Shape, Up};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use std::collections::BTreeSet;

/// What the screen keeps of the questions: which are put off, which one the keys are for, what is opened.
#[derive(Default)]
pub struct Ui {
    /// The questions put off with `esc`: still open in the core, not drawn until the operator asks for them (`?`) or a new one comes.
    pub later: BTreeSet<String>,
    /// The question the keys are for.
    pub focus: Option<String>,
    /// The questions whose change is shown with its diff, and those whose `why` is.
    pub diff: BTreeSet<String>,
    pub why: BTreeSet<String>,
    /// The question whose row `custom` has the input: its key was pressed, or `c` brought the card back.
    pub typing: Option<String>,
    /// An option that widens waits for its key a second time: the question and the number.
    pub armed: Option<(String, usize)>,
    /// The questions whose answer was sent and is not yet seen taken: the question and the option that was pressed, and when.
    pub sent: std::collections::BTreeMap<String, (String, std::time::Instant)>,
}

/// How the questions are drawn: the props of the ask field.
pub struct Look {
    pub one: Place,
    pub many: Place,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Popup,
    Tray,
    /// Only inline, in the chat.
    Inline,
}

impl Look {
    pub fn of(f: Option<&Field>) -> Look {
        let place = |name: &str, default: Place| match f.and_then(|f| f.props.get(name)).and_then(Json::as_str) {
            Some("popup") => Place::Popup,
            Some("tray") => Place::Tray,
            Some("inline") => Place::Inline,
            _ => default,
        };
        Look { one: place("one", Place::Popup), many: place("many", Place::Tray) }
    }
}

impl App {
    /// The questions that are drawn: the ones not put off.
    pub fn asks_shown(&self) -> Vec<&AskView> {
        self.asks.iter().filter(|a| !self.ui.later.contains(&a.ask)).collect()
    }

    /// The question the keys are for among those shown: the one chosen with `↑↓`, else the first.
    pub fn ask_focus(&self) -> Option<&AskView> {
        let shown = self.asks_shown();
        shown.iter().find(|a| self.ui.focus.as_deref() == Some(a.ask.as_str())).or(shown.first()).copied()
    }

    /// The question a line is for: the focused one, or when all are put off the first, which the binder still takes the line for.
    pub fn ask_target(&self) -> Option<&AskView> {
        self.ask_focus().or(self.asks.first())
    }

    /// The core sent the open questions again: what the screen kept about one that is gone goes with it.
    pub(crate) fn asks_changed(&mut self) {
        let open: BTreeSet<&str> = self.asks.iter().map(|a| a.ask.as_str()).collect();
        self.ui.later.retain(|k| open.contains(k.as_str()));
        self.ui.diff.retain(|k| open.contains(k.as_str()));
        self.ui.why.retain(|k| open.contains(k.as_str()));
        if self.ui.focus.as_ref().is_some_and(|k| !open.contains(k.as_str())) {
            self.ui.focus = None;
        }
        if self.ui.armed.as_ref().is_some_and(|(k, _)| !open.contains(k.as_str())) {
            self.ui.armed = None;
        }
        self.ui.sent.retain(|k, _| open.contains(k.as_str()));
        if self.ui.typing.as_ref().is_some_and(|k| !open.contains(k.as_str())) {
            self.ui.typing = None;
        }
    }

    /// The ask field of the page, if it has one: where the props of the look are.
    fn ask_field(&self) -> Option<Field> {
        let mut found = None;
        if let Some(root) = self.root() {
            self.shown(root, 0, &mut |n| {
                if found.is_none() && n.field.as_ref().is_some_and(|f| f.kind == FieldKind::Ask) {
                    found = n.field.clone();
                }
            });
        }
        found
    }
}

/// The rows the questions take at this width, at most `max`; none when nothing is open or all are put off.
pub fn height(app: &App, width: u16, max: u16) -> u16 {
    lines(app, usize::from(width), usize::from(max)).len() as u16
}

fn lines(app: &App, width: usize, max: usize) -> Vec<Line<'static>> {
    let look = Look::of(app.ask_field().as_ref());
    paint::lines(app, &look, width, max)
}

/// Whether a chat drawn from rows is on the screen: it draws the questions above its composer, and the ask field takes no room.
pub(crate) fn floats(app: &App) -> bool {
    app.chats.iter().any(|b| app.rows.contains_key(b))
}

/// The questions drawn in `area` (the ask field's own room, when no chat floats them).
pub fn draw(app: &mut App, area: Rect, f: &mut Frame) {
    let ls = lines(app, usize::from(area.width), usize::from(area.height));
    let h = (ls.len() as u16).min(area.height);
    f.render_widget(Paragraph::new(ls), Rect { y: area.bottom() - h, height: h, ..area });
}

/// The questions drawn in the rows `area` that were kept for them above a composer.
pub(crate) fn float(app: &mut App, area: Rect, f: &mut Frame) {
    let ls = lines(app, usize::from(area.width), usize::from(area.height));
    let h = (ls.len() as u16).min(area.height);
    f.render_widget(Paragraph::new(ls), Rect { y: area.bottom() - h, height: h, ..area });
}

// ---- keys ----

/// A key for the questions, while the composer has the keys and its line is empty: `Some` when it was taken here.
pub(crate) fn key(app: &mut App, k: KeyEvent) -> Option<Vec<Up>> {
    let (ctrl, alt) = (k.modifiers.contains(KeyModifiers::CONTROL), k.modifiers.contains(KeyModifiers::ALT));
    // the hint is taken with `→` at the end of the line, as fish takes its suggestion, or `⌃→` wherever the cursor is; `⇥` takes it on an
    // empty line (`parts::suggest`)
    let take = k.code == KeyCode::Right && !alt && (ctrl || app.line.at >= app.line.chars());
    if take && !app.asks.is_empty() {
        return crate::parts::suggest::hint(app);
    }
    if ctrl || alt || !app.line.buf.is_empty() || app.asks.is_empty() {
        return None;
    }
    // a key that is not the second press of an option that widens lets go of it
    let armed = app.ui.armed.take();
    let Some(a) = app.ask_focus().cloned() else {
        // all are put off: `?` brings them back
        let back = k.code == KeyCode::Char('?') || (k.code == KeyCode::Char('c') && app.asks.iter().any(|a| custom(a) || a.shape == Shape::Text));
        return back.then(|| {
            app.ui.later.clear();
            if k.code == KeyCode::Char('c') {
                app.ui.typing = app.ask_focus().map(|a| a.ask.clone());
            }
            Vec::new()
        });
    };
    let opts = options(&a, "");
    let shown = app.asks_shown().len();
    match k.code {
        // J4n's own approval (the Setup card): `esc` is its no, which for that card is `later`
        KeyCode::Esc if front_owned(&a) => opts.iter().find(|o| o.id == "no").map(|o| vec![Up::Intent { intent: o.intent(&a) }]),
        KeyCode::Esc => {
            // the popup or the tray goes away; the questions stay open in the core
            let all: Vec<String> = app.asks_shown().iter().map(|a| a.ask.clone()).collect();
            app.ui.later.extend(all);
            app.ui.focus = None;
            app.ui.typing = None;
            // the question stays open: say where it is and how to bring it back
            app.toast = Some((format!("question kept open · {} brings it back", if app.asks.iter().any(opts::custom) { "c" } else { "?" }), j4n_proto::Tone::Dim));
            Some(Vec::new())
        }
        KeyCode::Char('?') if !a.why.is_empty() && app.ui.later.is_empty() => {
            crate::display::toggle(&mut app.ui.why, a.ask.as_str());
            Some(Vec::new())
        }
        KeyCode::Enter if has_diff(&a) => {
            crate::display::toggle(&mut app.ui.diff, a.ask.as_str());
            Some(Vec::new())
        }
        KeyCode::Char('?') if !app.ui.later.is_empty() => {
            app.ui.later.clear();
            Some(Vec::new())
        }
        KeyCode::Up | KeyCode::Down if shown > 1 => {
            let ids: Vec<String> = app.asks_shown().iter().map(|a| a.ask.clone()).collect();
            let at = ids.iter().position(|k| *k == a.ask).unwrap_or(0);
            let to = if k.code == KeyCode::Up { (at + ids.len() - 1) % ids.len() } else { (at + 1) % ids.len() };
            app.ui.focus = Some(ids[to].clone());
            Some(Vec::new())
        }
        KeyCode::Char(d @ '1'..='9') => press(app, &a, &opts, armed, d as usize - '0' as usize),
        _ => None,
    }
}

/// The digit `n` for the question `a`: the option numbered so, or only the first press of one that widens (`armed` is what the last key
/// left waiting).
fn press(app: &mut App, a: &AskView, opts: &[Opt], armed: Option<(String, usize)>, n: usize) -> Option<Vec<Up>> {
    match opts.get(n - 1) {
        Some(o) if o.widens && armed.as_ref() != Some(&(a.ask.clone(), n)) => {
            app.ui.armed = Some((a.ask.clone(), n));
            Some(Vec::new())
        }
        // the row `custom`: its first press is the input, the line is typed in the composer
        Some(o) if o.send == Send::Type => {
            app.ui.typing = Some(a.ask.clone());
            Some(Vec::new())
        }
        Some(o) => Some(vec![Up::Intent { intent: o.intent(a) }]),
        None => None,
    }
}

/// Whether a change has lines to show: a row that only names the file has none, and offers no key for them.
pub(crate) fn has_diff(a: &AskView) -> bool {
    a.change.iter().any(|r| !r.detail.is_empty())
}
