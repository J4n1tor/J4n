//! The approval card: a pending approval, or the confirm of a risky key of a row, is a `parts::popup`
//! over the open tab, whatever it is, and takes its keys there: `y` and `n`, a digit, `?` for why, `esc` to put it off (the question stays
//! open and waits in the Chat, where the tab shows how many wait). A plain question (yes or no, a choice, a text) is never a card: it
//! stays in the Chat, and an option that widens still waits for its second press. On the Chat tab the approval is drawn above the
//! composer as before, so nothing is drawn twice.

use super::{Opt, front_owned, has_diff, options, paint, press};
use crate::app::App;
use crate::parts::popup;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use j4n_proto::{Act, AskView, Choice, FieldKind, Node, Row, Shape, Up};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;

/// From this risk (0–100) a key of a row asks first, on a card (the same line as a button's second press).
pub const RISKY: u8 = 50;

/// The widest a card is drawn; narrower screens give it the width less two.
const CARD: usize = 60;

/// The key of a confirm: it is not a question of the core, and has no task.
const CONFIRM: &str = "confirm:";

/// A key of a row that was pressed and waits for its yes: the card says what it would do, and `y` does it.
pub struct Confirm {
    pub act: Act,
    /// The page it was pressed on: it goes when the page does.
    pub(crate) bind: String,
    /// What the card shows, drawn like any question.
    pub(crate) view: AskView,
    /// The restart card to bring up on the yes: a row whose key acts from the next start (a pack).
    pub(crate) restart: Option<String>,
}

/// Whether a question is a confirm of this screen's, not one of the core's.
pub(crate) fn is_confirm(a: &AskView) -> bool {
    a.ask.starts_with(CONFIRM)
}

impl Confirm {
    /// The confirm of `act` for the selected `row`: its words are the key's label and the row's first cell, and what the row says of
    /// itself is the reason (a proposal's `widens` among it).
    pub fn of(act: Act, bind: &str, row: Option<&Row>) -> Confirm {
        let cell = |i: usize| row.and_then(|r| r.cells.get(i)).map_or("", |c| c.text.as_str());
        let rest: Vec<&str> = row.map_or(Vec::new(), |r| r.cells.iter().skip(1).map(|c| c.text.as_str()).filter(|t| !t.is_empty()).collect());
        let detail: Vec<&str> = row.map_or(Vec::new(), |r| r.detail.iter().map(|c| c.text.as_str()).collect());
        let view = AskView {
            ask: format!("{CONFIRM}{}", act.key),
            asker: act.label.clone(),
            question: format!("{} {}?", act.label, cell(0)).replace("  ", " "),
            shape: Shape::YesNo,
            risk: act.risk,
            effect: rest.join(" · "),
            why: detail.join("\n"),
            options: vec![Choice { id: "yes".into(), label: act.label.clone(), ..Choice::default() }, Choice { id: "no".into(), label: "not now".into(), ..Choice::default() }],
            ..AskView::default()
        };
        Confirm { act, bind: bind.to_string(), view, restart: row.and_then(crate::restart::after) }
    }
}

/// Whether a card can be drawn now: not over the Chat (it draws its own), not while something is being typed, and not over a sheet.
fn open(app: &App) -> bool {
    app.preview.is_none() && app.fatal.is_none() && app.tree.is_some() && !app.sheet && !app.typing() && app.field().is_none_or(|f| f.kind != FieldKind::Chat)
}

/// The confirm, while the page it was pressed on is open.
fn confirm_here(app: &App) -> Option<&Confirm> {
    app.confirm.as_ref().filter(|c| app.field().is_some_and(|f| f.bind == c.bind))
}

/// The approvals that are drawn as a card: those not put off, while the chat floats its questions (without one the ask field draws them).
fn approvals(app: &App) -> Vec<&AskView> {
    if !super::floats(app) {
        return Vec::new();
    }
    app.asks_shown().into_iter().filter(|a| a.shape == Shape::Approval).collect()
}

/// The approval the card is for: the one the keys are on if it is one, else the first.
fn card_ask(app: &App) -> Option<&AskView> {
    let all = approvals(app);
    all.iter().find(|a| app.ui.focus.as_deref() == Some(a.ask.as_str())).or(all.first()).copied()
}

/// The question the card shows, when there is one: the confirm first, then an approval.
fn shown(app: &App) -> Option<AskView> {
    if !open(app) {
        return None;
    }
    confirm_here(app).map(|c| c.view.clone()).or_else(|| card_ask(app).cloned())
}

/// How many questions wait for the Chat tab to be opened: what is not drawn as a card, so a plain question, or an approval put off. The
/// tab of the chat shows it while another tab is open.
pub(crate) fn tab_count(app: &App, tab: &Node, open: bool) -> Option<u64> {
    /// The Chat field is at the top of its tab or one group down: a tree of a million nodes is not walked for it.
    fn chat(n: &Node, depth: u8) -> bool {
        n.field.as_ref().is_some_and(|f| f.kind == FieldKind::Chat) || (depth < 2 && n.children.iter().any(|c| chat(c, depth + 1)))
    }
    let waiting = app.asks.len().saturating_sub(approvals(app).len());
    (waiting > 0 && !open && chat(tab, 0)).then_some(waiting as u64)
}

/// The card over the open tab, centred in `area`, when there is a question for it.
pub(crate) fn card(app: &mut App, area: Rect, f: &mut Frame) {
    let Some(a) = shown(app) else { return };
    let w = CARD.min(usize::from(area.width).saturating_sub(2));
    if w < 24 || area.height < 8 {
        return;
    }
    // the tab strip (and the sub-tabs) stay as they are, and so does the last row: the card is centred in the room between them
    let top = crate::nodes::tab_rows(app, area.height) + 2 * u16::from(app.inner_tabs().is_some());
    let room = area.height.saturating_sub(top + 1);
    if room < 6 {
        return;
    }
    let (title, lines) = paint::card(app, &a, w - 4, usize::from(room));
    let (w, h) = (w as u16, lines.len() as u16 + 2);
    let r = Rect { x: area.x + (area.width - w) / 2, y: area.y + top + (room - h) / 2, width: w, height: h };
    let inner = popup::boxed(&app.theme, f, r, &title);
    f.render_widget(Paragraph::new(lines), inner);
}

/// A key for the card: `Some` when it was taken. The card is modal for what is typed or pressed on the page (Tab, which completes, too);
/// the keys that move between tabs (the function keys) and every key with ⌃ or ⌥ go on, and let a confirm go with the page it was for.
pub(crate) fn card_key(app: &mut App, k: KeyEvent) -> Option<Vec<Up>> {
    let through = k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) || matches!(k.code, KeyCode::F(_));
    if through {
        app.confirm = None;
        return None;
    }
    if app.confirm.is_some() && confirm_here(app).is_none() {
        app.confirm = None;
    }
    if !open(app) {
        return None;
    }
    if confirm_here(app).is_some() {
        return confirm_key(app, k);
    }
    let a = card_ask(app).cloned()?;
    Some(approval_key(app, &a, k))
}

/// The key of a confirm: `y` or `1` does it, `n`, `2` or `esc` leaves it, `?` shows or hides why, and no other key does anything.
fn confirm_key(app: &mut App, k: KeyEvent) -> Option<Vec<Up>> {
    let c = app.confirm.as_ref()?;
    let ask = c.view.ask.clone();
    match k.code {
        KeyCode::Char('y' | '1') => {
            let c = app.confirm.take()?;
            app.ui.why.remove(&ask);
            app.restart = c.restart.or(app.restart.take());
            Some(vec![Up::Intent { intent: c.act.intent }])
        }
        KeyCode::Char('n' | '2') | KeyCode::Esc => {
            app.confirm = None;
            app.ui.why.remove(&ask);
            Some(Vec::new())
        }
        KeyCode::Char('?') => {
            crate::display::toggle(&mut app.ui.why, ask.as_str());
            Some(Vec::new())
        }
        _ => Some(Vec::new()),
    }
}

/// The key of an approval's card, as the same question has it above the composer: the answer a key names is sent, an option that widens
/// waits for its second press, and `esc` puts the question off (it stays open, with its count on the Chat tab).
fn approval_key(app: &mut App, a: &AskView, k: KeyEvent) -> Vec<Up> {
    // a key that is not the second press of an option that widens lets go of it
    let armed = app.ui.armed.take();
    let opts: Vec<Opt> = options(a, "");
    let said = |id: &str| opts.iter().find(|o| o.id == id).map(|o| vec![Up::Intent { intent: o.intent(a) }]);
    match k.code {
        KeyCode::Esc if front_owned(a) => said("no").unwrap_or_default(),
        // `⏎` takes the answer the core recommends, as `→` does in the chat
        KeyCode::Enter if !has_diff(a) => opts.iter().find(|o| o.hint).map(|o| vec![Up::Intent { intent: o.intent(a) }]).unwrap_or_default(),
        KeyCode::Esc => {
            app.ui.later.insert(a.ask.clone());
            app.ui.focus = None;
            Vec::new()
        }
        KeyCode::Char('?') if !a.why.is_empty() => {
            crate::display::toggle(&mut app.ui.why, a.ask.as_str());
            Vec::new()
        }
        KeyCode::Enter if has_diff(a) => {
            crate::display::toggle(&mut app.ui.diff, a.ask.as_str());
            Vec::new()
        }
        KeyCode::Up | KeyCode::Down => {
            let ids: Vec<String> = approvals(app).iter().map(|a| a.ask.clone()).collect();
            let at = ids.iter().position(|k| *k == a.ask).unwrap_or(0);
            let to = if k.code == KeyCode::Up { (at + ids.len() - 1) % ids.len() } else { (at + 1) % ids.len() };
            app.ui.focus = Some(ids[to].clone());
            Vec::new()
        }
        KeyCode::Char('y') => said("yes").unwrap_or_default(),
        KeyCode::Char('n') => said("no").unwrap_or_default(),
        KeyCode::Char(d @ '1'..='9') => press(app, a, &opts, armed, d as usize - '0' as usize).unwrap_or_default(),
        _ => Vec::new(),
    }
}
