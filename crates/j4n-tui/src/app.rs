//! What the TUI keeps: the tree and the rows the core sent, and the view state (the tab, the scroll, the draft).
//! It turns what the core sends (`Down`) into state and what the operator presses into what the core is asked (`Up`).
//! It never reads a file, runs a program or evaluates anything: a binding is a name the core resolves.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crate::ask;
use crate::display::Win;
use crate::fields::Ed;
use crate::keymap::{Keymap, Step};
use crate::shape::Composer;
use crate::nodes::Jump;
use crate::parts::line::Typed;
use crate::theme::{Settings, Theme};
use j4n_proto::{API, Act, AskView, Down, FieldKind, Intent, Json, Node, Reply, Row, SHAPE_CARD, SHAPE_ON, Shape, Tone, Up};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

/// How long one of the eight ticks of the breath lasts, in milliseconds.
const BEAT_MS: u128 = 250;
/// The most lines of the draft's history.
const HISTORY_MAX: usize = 100;

/// A value being typed for a field of a form.
pub struct Edit {
    pub act: Act,
    pub field: String,
    pub line: Typed,
}

pub struct App {
    pub name: String,
    pub version: String,
    pub project: String,
    pub tree: Option<Arc<Node>>,
    /// Where the group of top tabs is: the position of each child on the way down from the root.
    pub(crate) top: Option<Vec<usize>>,
    pub tab: usize,
    /// The sub-tab each group that shows its children as tabs has open, by the group's id (the top tabs are `tab`).
    pub sub: HashMap<String, usize>,
    /// The first field of each stack of fields that is drawn, by the stack's id: a stack taller than its room scrolls to the field the keys are on.
    pub(crate) stack_top: HashMap<String, usize>,
    pub rows: HashMap<String, Vec<Row>>,
    pub asks: Vec<AskView>,
    /// What the screen keeps of the questions: the ones put off, the one the keys are for, what is opened.
    pub ui: ask::Ui,
    /// A risky key of a row that waits for its yes, drawn as the approval card over the page it was pressed on.
    pub confirm: Option<ask::Confirm>,
    pub toast: Option<(String, Tone)>,
    /// The line of the composer, and its cursor.
    pub line: Typed,
    pub history: Vec<String>,
    browsing: Option<usize>,
    /// The line as it was before the first ↑ of a walk through the history: ↓ past the newest, or `esc`, gives it back.
    walked: Option<Typed>,
    /// How far each chat is scrolled up from its end, in lines.
    pub scroll: HashMap<String, usize>,
    pub edit: Option<Edit>,
    /// Why the screen cannot be used (a protocol this build does not speak).
    pub fatal: Option<String>,
    pub quit: bool,
    pub size: (u16, u16),
    /// The composer's card and the timing of the draft (`shape`).
    pub composer: Composer,
    pub(crate) watched: BTreeSet<String>,
    /// The bindings of the chats in the tree, wherever they are.
    pub(crate) chats: BTreeSet<String>,
    /// The sections that are folded, by the id of the section.
    pub shut: BTreeSet<String>,
    /// The nodes the core has been asked to send the children of.
    pub asked: BTreeSet<String>,
    /// When each watched binding and each page asked for was asked, by the clock the screen was told, and the soonest moment the last frame's
    /// waiting parts change (`wait`).
    pub(crate) since: HashMap<String, std::time::Instant>,
    pub(crate) opened: HashMap<String, std::time::Instant>,
    pub(crate) wait_due: std::cell::Cell<Option<std::time::Instant>>,
    /// The jump (`⌃k`), while it is open.
    pub jump: Option<Jump>,
    /// The value of each binding of a value field.
    pub values: HashMap<String, Json>,
    /// How the Log shows each kind and field: the look file the core sent (`look:file`), the bare look before it came.
    pub events: crate::display::event::Look,
    /// The value field being edited.
    pub val: Option<Ed>,
    /// The row of a list whose value is being edited in place with the editor of its kind (`data.edit`); the edit is `val`.
    pub row_ed: Option<crate::fields::RowEd>,
    /// The setting just changed that acts from the next start: the restart card asks.
    pub restart: Option<String>,
    /// The forms whose fields keep their values as a draft until the form is submitted (`parts::form`).
    pub forms: Vec<crate::parts::form::Form>,
    /// The groups of sections whose body has the keys, by the id of the group.
    pub inside: BTreeSet<String>,
    /// The view state of each binding of rows that is shown as a table, a tree, cards or a log.
    pub wins: HashMap<String, Win>,
    /// Where the window the screen holds of each binding starts, and how many rows the binding has in all.
    pub span: HashMap<String, (u64, u64)>,
    /// The button that has been pressed once and waits for the second press.
    pub armed: Option<String>,
    /// How the screen looks: the roles of the palette and the tier of glyphs.
    pub theme: Theme,
    /// How prose is drawn (`read.*`) and what the terminal can do; the terminal's part is asked once, by the loop that runs it.
    pub read: crate::read::Opts,
    /// Watches for a terminal's late reply to the question put at the start; its keys are dropped, not typed.
    pub late: crate::read::Late,
    /// The links the last frame drew, by screen cell, for the terminal to open (OSC 8).
    pub opens: Vec<crate::read::Open>,
    /// The headings the last frame left room for in kitty's text sizing, by screen cell.
    pub bigs: Vec<crate::read::Big>,
    /// A page of the theme shown in place of the screen, to look at the colours and the glyphs.
    pub preview: Option<Preview>,
    /// Where the clock was first read, and which of the eight ticks of the breath it is on.
    t0: Option<std::time::Instant>,
    pub phase: u8,
    /// The frame of the loading sign (`parts::track`), `0..FRAMES`, on a beat of its own that the same clock `t0` counts.
    pub sign: u8,
    /// A loading sign was drawn at the last frame: the screen wakes every `track::BEAT_MS` for it, and for nothing else.
    pub signs: bool,
    /// The screen's wall clock: epoch milliseconds at an instant, none until it is told (a replay's rows say the time of day they came with),
    /// the time it says now, whether a row's time is how long ago (`look.time`), and the soonest moment a word of time on the last frame changes.
    pub(crate) wall: Option<(u64, std::time::Instant)>,
    pub(crate) now_ms: Option<u64>,
    pub(crate) time_ago: bool,
    pub(crate) age_due: std::cell::Cell<Option<u64>>,
    /// A row that breathes was drawn at the last frame; the screen is drawn again when the tick changes.
    pub breathes: bool,
    pub(crate) redraw: bool,
    /// The lane (the key of its row) the line in the composer is for; none for the thread's own doer.
    pub aim: Option<String>,
    /// The row of the pinned list of the chats the keys are on.
    pub(crate) pin: Option<usize>,
    /// When the list last had a key, or none until the clock has been read; it hands the keys back after [`chats::IDLE`].
    pub(crate) pin_since: Option<std::time::Instant>,
    /// The chats that are done for a while are shown, not folded into `N done before` (`space` or `⏎` on the fold).
    pub(crate) done_open: bool,
    /// The archived chats are shown under their row, `archived (N)`.
    pub(crate) archive_open: bool,
    /// What the keys do, once the core has said; until then every key is read as it is.
    pub keymap: Option<Keymap>,
    /// The numbers of the tabs while they show (`leader`), and the clock the screen was last told.
    pub(crate) numbers: Option<crate::leader::Numbers>,
    pub(crate) clock: Option<std::time::Instant>,
    /// The last key of the burst that came with the ⏎ that kept a key, while it lasts.
    pub(crate) burst: Option<std::time::Instant>,
    /// The half screen `PgUp`/`PgDn` is drawing in steps, while it is.
    pub(crate) glide: Option<crate::chat::scroll::Glide>,
    /// The ↑ or ↓ of the composer that came last, while more may be coming with it: a terminal on the alternate screen sends a
    /// wheel notch as three of them within a few ms.
    pub(crate) arrow: Option<Arrow>,
    /// The sheet of all the keys of the page (`?`), while it is open.
    pub sheet: bool,
    /// The bottom row's two faces: whether the state was news at the last look, and the face the operator chose by hand over it
    /// (`true` the state, `false` the keys); the choice goes when the news changes.
    pub(crate) bar_news: bool,
    pub(crate) bar_hold: Option<bool>,
    /// The lines sent that are not yet seen taken: each is a turn of its chat the moment it is sent.
    pub(crate) sent: Vec<crate::chat::live::Sent>,
    /// What Tab completes: the find asked of the core, the ghost `esc` hid, Tab's steps (`parts::suggest`).
    pub suggest: crate::parts::suggest::State,
    /// A line held because it looks like a key, with its card (`chat::held`).
    pub(crate) held: Option<crate::chat::held::Held>,
    /// The terminal may split ⌥digit into a bare Esc and a digit (it has no kitty protocol): a bare Esc waits a moment for the digit.
    pub esc_waits: bool,
    /// When the bare Esc that waits came (`leader::ESC_WAIT`).
    pub(crate) esc_held: Option<std::time::Instant>,
    /// ⌥ held alone, where the terminal reports it (`leader::HOLD`).
    pub(crate) alt_held: Option<crate::leader::Hold>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preview {
    Roles,
    Glyphs,
}

impl Default for App {
    fn default() -> App {
        App {
            name: "j4n".into(),
            version: String::new(),
            project: String::new(),
            tree: None,
            top: None,
            tab: 0,
            sub: HashMap::new(),
            rows: HashMap::new(),
            asks: Vec::new(),
            ui: ask::Ui::default(),
            confirm: None,
            toast: None,
            line: Typed::default(),
            forms: Vec::new(),
            history: Vec::new(),
            browsing: None,
            walked: None,
            scroll: HashMap::new(),
            edit: None,
            fatal: None,
            quit: false,
            size: (80, 24),
            composer: Composer::default(),
            watched: BTreeSet::new(),
            chats: BTreeSet::new(),
            shut: BTreeSet::new(),
            asked: BTreeSet::new(),
            since: HashMap::new(),
            opened: HashMap::new(),
            wait_due: std::cell::Cell::new(None),
            jump: None,
            values: HashMap::new(),
            events: Default::default(),
            val: None,
            row_ed: None,
            restart: None,
            inside: BTreeSet::new(),
            wins: HashMap::new(),
            stack_top: HashMap::new(),
            span: HashMap::new(),
            armed: None,
            theme: Theme::default(),
            read: crate::read::Opts::default(),
            late: crate::read::Late::default(),
            opens: Vec::new(),
            bigs: Vec::new(),
            preview: None,
            t0: None,
            phase: 0,
            sign: 0,
            signs: false,
            wall: None,
            now_ms: None,
            time_ago: true,
            age_due: std::cell::Cell::new(None),
            breathes: false,
            redraw: false,
            aim: None,
            pin: None,
            pin_since: None,
            done_open: false,
            archive_open: false,
            keymap: None,
            numbers: None,
            clock: None,
            burst: None,
            glide: None,
            arrow: None,
            sheet: false,
            bar_news: false,
            bar_hold: None,
            sent: Vec::new(),
            suggest: Default::default(),
            held: None,
            esc_waits: false,
            esc_held: None,
            alt_held: None,
        }
    }
}

/// How close two ↑ or ↓ are when a terminal sends them for one wheel notch.
const NOTCH: std::time::Duration = std::time::Duration::from_millis(8);
/// How close after the ⏎ that kept a key, and after each other, keys are taken as the rest of one burst and dropped.
const BURST: std::time::Duration = std::time::Duration::from_millis(30);

/// An ↑ or ↓ of the composer: when, which way, how many of a burst so far, and the draft as it was before the first recalled.
#[derive(Debug, Clone)]
pub(crate) struct Arrow {
    at: std::time::Instant,
    up: bool,
    n: u8,
    before: (Typed, Option<usize>, Option<usize>, Option<Typed>),
}

impl App {
    pub fn new() -> App {
        App::default()
    }

    /// What is sent first.
    pub fn start(&mut self) -> Vec<Up> {
        vec![Up::Hello { api: API }]
    }


    pub fn down(&mut self, d: Down) -> Vec<Up> {
        match d {
            Down::Hello { api, name, version, project } => {
                if api > API {
                    self.fatal = Some(format!("this screen speaks protocol {API}; the core speaks {api}: update J4n"));
                    self.quit = true;
                }
                (self.name, self.version, self.project) = (name, version, project);
                Vec::new()
            }
            Down::Tree { node } => {
                self.plant(node);
                self.sync()
            }
            // the core sends no stream (the chat and the Log are rows); a screen of this build draws none
            Down::Stream { .. } => Vec::new(),
            Down::Rows { bind, rows, total, from, .. } => {
                let n = rows.len();
                let card = bind == SHAPE_CARD;
                self.rows.insert(bind.clone(), rows);
                self.span.insert(bind.clone(), (from, total));
                self.wins.get_mut(&bind).into_iter().for_each(|w| w.stale = false);
                let cur = &mut self.win(&bind).cur;
                cur.select(cur.at, total.max(n as u64));
                self.sent_sync(&bind);
                if bind == j4n_proto::COMPLETE {
                    self.next_came();
                }
                if card { self.shape_resolve() } else { self.windows() }
            }
            Down::Asks { open } => {
                self.asks = open;
                self.asks_changed();
                Vec::new()
            }
            Down::Toast { text, tone } => {
                self.toast = Some((text, tone));
                Vec::new()
            }
            // `:chats`: the list of the chats, as a jump of their own
            Down::Focus { node } if node == "chats" => {
                self.open_chats();
                Vec::new()
            }
            Down::Focus { node } => self.focus(&node),
            Down::Keys { binds } => {
                self.keymap = Some(Keymap::of(binds));
                Vec::new()
            }
            Down::Theme { look } => self.look(look),
            Down::Value { bind, value, .. } => {
                if bind == SHAPE_ON {
                    self.composer.on = value.as_bool().unwrap_or(false);
                }
                if bind == j4n_proto::LOOK_FILE {
                    (self.events, self.redraw) = (crate::display::event::Look::of(&value), true);
                }
                if bind == j4n_proto::TERMS {
                    self.read.glossary(&value);
                }
                if bind == j4n_proto::CHAT_HERE {
                    self.here_changed(&value);
                }
                self.values.insert(bind, value);
                Vec::new()
            }
        }
    }

    /// The look the core named, with the colour the terminal asked for; a word it cannot read leaves the look as it was, and says why.
    /// The next (or the previous) sub-tab of the open tab, round the end.
    pub(crate) fn step_sub(&mut self, forward: bool) -> Vec<Up> {
        let Some((id, n)) = self.inner_tabs().map(|t| (t.id.clone(), t.children.len())) else { return Vec::new() };
        let now = self.sub.get(&id).copied().unwrap_or(0).min(n.saturating_sub(1));
        self.sub.insert(id, if forward { (now + 1) % n } else { (now + n - 1) % n });
        self.sync()
    }

    fn look(&mut self, look: j4n_proto::Look) -> Vec<Up> {
        let s = Settings { palette: look.palette, marks: look.marks, frame: look.frame, corners: look.corners, title_place: look.title_place, density: look.density, fill: look.fill, you: look.you, motion: look.motion, mouse: look.mouse, no_color: self.theme.no_color, chart: look.chart, truecolor: self.theme.truecolor };
        self.time_ago = look.time != "clock";
        self.read = crate::read::Opts { caps: self.read.caps, root: self.read.root.take(), ..crate::read::Opts::of(&look.read) };
        if let Some(v) = self.values.get(j4n_proto::TERMS) {
            self.read.glossary(v);
        }
        match Theme::build(&s, &[]) {
            Ok(theme) => (self.theme, self.redraw) = (theme, true),
            Err(why) => self.toast = Some((why, Tone::Bad)),
        }
        Vec::new()
    }

    /// The breath of the doer that works: a tick is a quarter of a second, eight make a breath. The screen needs drawing again only
    /// when the tick changed and a row that breathes is in sight.
    pub(crate) fn breathe(&mut self, now: std::time::Instant) {
        self.numbers_tick(now);
        self.age_tick(now);
        self.wait_tick(now);
        self.sent_gc();
        let t0 = *self.t0.get_or_insert(now);
        let phase = ((now.saturating_duration_since(t0).as_millis() / BEAT_MS) % 8) as u8;
        if phase != self.phase {
            self.phase = phase;
            self.redraw |= self.breathes && self.theme.motion;
        }
        let sign = ((now.saturating_duration_since(t0).as_millis() / crate::parts::track::BEAT_MS) % u128::from(crate::parts::track::FRAMES)) as u8;
        if sign != self.sign {
            self.sign = sign;
            self.redraw |= self.signs && self.theme.motion;
        }
    }

    /// How long until the breath changes tick, while a row that breathes is in sight and the look has motion, or until a word of time on the screen
    /// changes (the clock, which motion does not stop); else the clock is no reason to wake.
    pub(crate) fn beat_wait(&self, now: std::time::Instant) -> Option<std::time::Duration> {
        let breath = self.t0.filter(|_| self.breathes && self.theme.motion).map(|t0| {
            let into = now.saturating_duration_since(t0).as_millis() % BEAT_MS;
            std::time::Duration::from_millis((BEAT_MS - into) as u64)
        });
        // the loading sign: a frame every 120 ms while one is drawn, aligned to the same clock; none drawn, no wake
        let sign = self.t0.filter(|_| self.signs && self.theme.motion).map(|t0| {
            let into = now.saturating_duration_since(t0).as_millis() % crate::parts::track::BEAT_MS;
            std::time::Duration::from_millis((crate::parts::track::BEAT_MS - into) as u64)
        });
        [breath, sign, self.age_wait(now), self.wait_wait(now)].into_iter().flatten().min()
    }

    /// Whether the screen must be drawn again because of the clock; reading it clears it.
    pub fn take_redraw(&mut self) -> bool {
        std::mem::take(&mut self.redraw)
    }

    // ---- keys ----

    fn say(&mut self, text: String) -> Vec<Up> {
        self.say_with(text, Vec::new())
    }

    /// A line sent with the keys of the rows the operator kept beside it.
    pub(crate) fn say_with(&mut self, text: String, with: Vec<String>) -> Vec<Up> {
        // a `:key …` line is kept in neither the history nor the echo: what follows the word may be a key
        let keyed = crate::chat::held::key_line(&text);
        if !keyed && self.history.last() != Some(&text) {
            self.history.push(text.clone());
            let extra = self.history.len().saturating_sub(HISTORY_MAX);
            self.history.drain(..extra);
        }
        self.browsing = None;
        // the line is a turn of the chat now, whatever the core takes to say so
        if !keyed {
            self.echo(&text);
        }
        // the card of a question with the row `custom` is up: the line is typed into that row and is its answer, whatever it says; a `:command`
        // and a `/` line are still commands
        if !text.starts_with([':', '/'])
            && let Some(a) = self.ask_focus().filter(|a| crate::ask::custom(a)).cloned()
        {
            self.ui.typing = None;
            return self.answer(&a, Reply::Text { text });
        }
        vec![Up::Intent { intent: Intent::Say { text, with } }]
    }

    fn answer(&self, ask: &AskView, reply: Reply) -> Vec<Up> {
        vec![Up::Intent { intent: Intent::Answer { ask: ask.ask.clone(), reply } }]
    }

    /// The question the keys are for.
    pub(crate) fn open_ask(&self) -> Option<&AskView> {
        self.ask_target()
    }

    pub(crate) fn set_input(&mut self, text: String) {
        self.line = Typed::new(text);
        // a new line: what `esc` hid of the completion was for the line before
        self.suggest.step = None;
        (self.suggest.hidden, self.suggest.shut, self.suggest.open) = (None, None, false);
    }

    /// ↑ or ↓ in the composer. One that comes alone recalls the draft before or after; a second of the same way within [`NOTCH`] of the first
    /// is a wheel notch (a terminal sends three), so the recall is taken back and the chat scrolls three lines, and the third is dropped.
    /// Keys are timed by the clock the screen was told; with none, each one is a press.
    fn arrow(&mut self, up: bool) {
        let Some(now) = self.clock else { return self.press(up) };
        match self.arrow.take() {
            Some(a) if a.up == up && now.saturating_duration_since(a.at) <= NOTCH => {
                if a.n == 1 {
                    (self.line, self.browsing, self.pin, self.walked) = a.before;
                    self.scroll_lines(up, 3);
                    self.arrow = Some(Arrow { at: now, n: 2, before: (Typed::default(), None, None, None), ..a });
                }
            }
            _ => {
                let before = (self.line.clone(), self.browsing, self.pin, self.walked.clone());
                self.press(up);
                self.arrow = Some(Arrow { at: now, up, n: 1, before });
            }
        }
    }

    /// A ↑ or ↓ that is a press: ↓ on an empty line, past the newest of the history, puts the keys on the list of the chats; else it recalls.
    fn press(&mut self, up: bool) {
        if !up && self.line.buf.is_empty() && self.browsing.is_none() && crate::chat::chats::up_into(self) {
            return;
        }
        self.recall(up);
    }

    /// Whether an ↑ or ↓ of this way came within [`NOTCH`] of the last: the rest of a wheel notch, which the composer reads, not the list.
    pub(crate) fn burst(&self, up: bool) -> bool {
        self.clock.zip(self.arrow.as_ref()).is_some_and(|(now, a)| a.up == up && now.saturating_duration_since(a.at) <= NOTCH)
    }

    fn recall(&mut self, older: bool) {
        if self.history.is_empty() {
            return;
        }
        let at = match (self.browsing, older) {
            (None, true) => {
                self.walked = Some(self.line.clone());
                self.history.len() - 1
            }
            (Some(i), true) => i.saturating_sub(1),
            (Some(i), false) if i + 1 < self.history.len() => i + 1,
            // past the newest: the line typed before the walk comes back; with no walk, ↓ leaves the line as it is
            (Some(_), false) => return self.walk_end(),
            (None, false) => return,
        };
        self.browsing = Some(at);
        self.set_input(self.history[at].clone());
    }

    /// Whether the line is one recalled from the history and not changed.
    fn recalled(&self) -> bool {
        self.browsing.is_some_and(|i| self.history.get(i) == Some(&self.line.buf))
    }

    /// The walk through the history ends and the line typed before it comes back.
    fn walk_end(&mut self) {
        self.browsing = None;
        let line = self.walked.take().unwrap_or_default();
        self.set_input(String::new());
        self.line = line;
    }

    /// `esc` in a walk through the history, on a line recalled and not changed: the line typed before the walk comes back. False when
    /// there is no walk, or the recalled line was changed (what was typed is kept).
    fn walk_back(&mut self) -> bool {
        let recalled = self.recalled();
        if recalled {
            self.walk_end();
        }
        recalled
    }

    pub(crate) fn scroll_by(&mut self, up: bool, lines: usize) {
        let Some(bind) = self.field().map(|f| f.bind.clone()) else { return };
        let at = self.scroll.entry(bind).or_default();
        *at = if up { *at + lines } else { at.saturating_sub(lines) };
    }

    fn move_sel(&mut self, down: bool) {
        let Some(bind) = self.field().map(|f| f.bind.clone()) else { return };
        // the rows under a section that is shut are not stepped on
        let seen = crate::display::shown(self, &bind);
        let at = self.place(&bind);
        let pos = seen.iter().position(|i| *i >= at).unwrap_or(seen.len().saturating_sub(1));
        let to = if down { (pos + usize::from(seen.get(pos) == Some(&at))).min(seen.len().saturating_sub(1)) } else { pos.saturating_sub(1) };
        if let Some(i) = seen.get(to) {
            self.win(&bind).cur.at = *i as u64;
        }
    }

    /// The action of the selected row of the open list or form with this key (`enter` when `key` is none).
    pub(crate) fn row_act(&self, key: Option<&str>) -> Option<Act> {
        let bind = &self.field()?.bind;
        let row = self.rows.get(bind)?.get(self.place(bind))?;
        row.actions.iter().find(|a| key.is_none_or(|k| a.key == k)).cloned()
    }

    /// The act of the chat's newest row (a note that leads somewhere): only while that row is the last one there is, so an older
    /// note's act is never done by a bare `⏎`.
    pub(crate) fn newest_act(&self) -> Option<Act> {
        let bind = &self.field()?.bind;
        let rows = self.rows.get(bind)?;
        let (from, total) = self.span.get(bind).copied().unwrap_or((0, rows.len() as u64));
        if from + rows.len() as u64 != total {
            return None;
        }
        rows.last()?.actions.iter().find(|a| a.key == "enter").cloned()
    }

    pub(crate) fn run_act(&mut self, act: Act) -> Vec<Up> {
        // a row that carries its editor: a switch flips, anything else opens the editor of its kind under the row
        if let Intent::Set { value, .. } = &act.intent
            && let Some(ups) = self.row_set(value)
        {
            return ups;
        }
        match &act.intent {
            // a key has no road but its masked editor on the row: never a value sent, never the plain line, which does not mask
            Intent::Set { kind, .. } if kind == "key" => {
                self.toast = Some((crate::fields::NO_ROAD.into(), Tone::Bad));
                Vec::new()
            }
            // a value that came with the act and no editor on the row (a note's `⏎ turns the chats on`): sent as it is
            Intent::Set { value, .. } if !value.is_null() => vec![Up::Intent { intent: act.intent }],
            Intent::Set { field, .. } => {
                self.edit = Some(Edit { field: field.clone(), act, line: Typed::default() });
                Vec::new()
            }
            // a key that is risky asks first, on the card; the command of one that asks the Gate itself has no risk of its own here
            _ if act.risk >= ask::RISKY => {
                let bind = self.field().map(|f| f.bind.clone()).unwrap_or_default();
                let confirm = ask::Confirm::of(act, &bind, self.rows.get(&bind).and_then(|r| r.get(self.place(&bind))));
                self.ui.why.insert(confirm.view.ask.clone());
                self.confirm = Some(confirm);
                Vec::new()
            }
            _ => {
                // a row whose key acts from the next start brings up the restart card
                let bind = self.field().map(|f| f.bind.clone()).unwrap_or_default();
                if let Some(id) = self.rows.get(&bind).and_then(|r| r.get(self.place(&bind))).and_then(crate::restart::after) {
                    self.restart = Some(id);
                }
                vec![Up::Intent { intent: act.intent }]
            }
        }
    }

    fn edit_key(&mut self, k: KeyEvent) -> Vec<Up> {
        let Some(edit) = self.edit.as_mut() else { return Vec::new() };
        match k.code {
            KeyCode::Esc => self.edit = None,
            KeyCode::Enter => {
                if let Some(Edit { act, line, .. }) = self.edit.take()
                    && let Intent::Set { kind, id, field, .. } = act.intent
                {
                    return vec![Up::Intent { intent: Intent::Set { kind, id, field, value: Json::String(line.buf.trim().into()) } }];
                }
            }
            _ => drop(edit.line.edit(&k)),
        }
        Vec::new()
    }

    /// Text pasted whole (bracketed paste): into the value being typed, or the draft. A line break is one character of the
    /// draft and is sent as it is; it is not Enter.
    pub fn paste(&mut self, text: &str) {
        // a key's paste is read as it came, so that no copy of it is made on the way
        if self.val.as_ref().is_some_and(|e| e.secret) && self.edit.is_none() {
            self.val_paste(text);
            self.toast = None;
            return;
        }
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if let Some(edit) = self.edit.as_mut() {
            text.chars().filter(|c| !c.is_control()).for_each(|c| edit.line.insert(c));
        } else if !self.val_paste(&text) && self.field().is_some_and(|f| f.kind == FieldKind::Chat) {
            // a paste ends Tab's steps: what is in the line is kept
            self.suggest.step = None;
            for c in text.chars().map(|c| if c == '\t' { ' ' } else { c }).filter(|c| *c == '\n' || !c.is_control()) {
                self.line.insert(c);
            }
        }
        self.toast = None;
    }

    pub fn key(&mut self, k: KeyEvent) -> Vec<Up> {
        // ⌥ alone is the hold; another modifier alone and a release are nothing
        let Some(k) = self.hold_key(k) else { return Vec::new() };
        // a bare Esc may be the first half of ⌥digit: it waits, and a digit soon after makes the pair
        let (mut ups, k) = self.esc_pair(k);
        if let Some(k) = k {
            ups.extend(self.key_now(k));
        }
        ups
    }

    /// A key, read now.
    pub(crate) fn key_now(&mut self, k: KeyEvent) -> Vec<Up> {
        // a `⇥` that waits for the core's next line waits through no other key
        self.suggest.next = None;
        let mut ups = self.key_in(k);
        // a key that answers a question is marked on the option it chose until the question goes
        self.sent_answers(&ups);
        // the core is asked for the words of the line as it is now (not for plain words recalled from the history as they were sent: a
        // wheel's arrows would ask for each); the key never waits for them
        if !(self.recalled() && crate::parts::suggest::slot_of(&self.line.buf).slot == crate::parts::suggest::Slot::Sentence) {
            ups.extend(self.complete_window());
        }
        ups
    }

    fn key_in(&mut self, k: KeyEvent) -> Vec<Up> {
        // a reply of the terminal that came after the wait is not typed
        if self.late.eat(self.clock.unwrap_or_else(std::time::Instant::now), &k) {
            return Vec::new();
        }
        // the keys that come with the ⏎ that kept a key (a terminal that types a paste out) are dropped, each one carrying the window on
        if let (Some(at), Some(now)) = (self.burst, self.clock) {
            if now.saturating_duration_since(at) <= BURST {
                self.burst = Some(now);
                return Vec::new();
            }
            self.burst = None;
        }
        self.toast = None;
        // a glide goes on only under the page keys
        if !matches!(k.code, KeyCode::PageUp | KeyCode::PageDown) {
            self.glide = None;
        }
        // a terminal that reports Tab as the character does not make it text: Tab never types
        let k = match k.code {
            KeyCode::Char('\t') if !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => KeyEvent::new(if k.modifiers.contains(KeyModifiers::SHIFT) { KeyCode::BackTab } else { KeyCode::Tab }, k.modifiers),
            _ => k,
        };
        // a line held because it looks like a key: its card takes the chat's keys
        if let Some(ups) = self.held_key(k) {
            return ups;
        }
        // an approval or a confirm is a card over the open page: it takes its keys before the page or the keymap sees them
        if let Some(ups) = ask::card_key(self, k) {
            return ups;
        }
        let k = match self.mapped(k) {
            Step::Go(k) => k,
            Step::Done(ups) => return ups,
        };
        // `⇥` on the list of the chats goes into the chat it is on, as `⏎` does
        let k = if k.code == KeyCode::Tab && self.pin.is_some() && !self.typing() && self.field().is_some_and(|f| f.kind == FieldKind::Chat) { KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE) } else { k };
        // Tab and ⇧Tab complete: they never type and never move a tab
        if matches!(k.code, KeyCode::Tab | KeyCode::BackTab) {
            self.nav_key(k);
            return self.tab_key(k);
        }
        if let Some(ups) = self.nav_key(k) {
            return ups;
        }
        if self.edit.is_some() {
            return self.edit_key(k);
        }
        if self.restart.is_some() {
            return self.restart_key(k);
        }
        if self.row_ed.is_some() {
            return self.row_key(k);
        }
        if self.jump.is_some() {
            return self.jump_key(k);
        }
        if self.sheet_key(&k) {
            return Vec::new();
        }
        // an edit belongs to the field it was begun in
        if self.val.as_ref().is_some_and(|e| self.field().is_none_or(|f| f.bind != e.bind)) {
            self.val = None;
        }
        if k.code == KeyCode::Char('k') && k.modifiers.contains(KeyModifiers::CONTROL) {
            self.open_jump();
            return Vec::new();
        }
        if k.code == KeyCode::Char('g') && k.modifiers.contains(KeyModifiers::CONTROL) {
            self.flip_bar();
            return Vec::new();
        }
        let (ctrl, alt) = (k.modifiers.contains(KeyModifiers::CONTROL), k.modifiers.contains(KeyModifiers::ALT));
        let kind = self.field().map(|f| f.kind);
        if kind.is_some_and(FieldKind::is_value)
            && let Some(ups) = self.val_key(k)
        {
            return ups;
        }
        if let Some(f) = self.field().filter(|f| crate::display::keyed(f.kind)).cloned()
            && let Some(mut ups) = crate::display::key(self, &f, k)
        {
            ups.extend(self.windows());
            return ups;
        }
        if let Some(f) = self.field().filter(|f| f.kind == FieldKind::Chat).cloned()
            && let Some(ups) = crate::chat::key(self, &f, k)
        {
            return ups;
        }
        if kind == Some(FieldKind::Chat)
            && let Some(ups) = self.complete_key(k)
        {
            return ups;
        }
        if kind == Some(FieldKind::Chat)
            && let Some(ups) = ask::key(self, k)
        {
            return ups;
        }
        if kind == Some(FieldKind::Chat)
            && let Some(ups) = self.shape_key(k)
        {
            return ups;
        }
        if let (true, Some(ask)) = (alt, self.open_ask().cloned())
            && let KeyCode::Char(c) = k.code
        {
            let reply = match (c, ask.shape) {
                ('y', Shape::Approval | Shape::YesNo) => Some(Reply::Yes),
                ('n', Shape::Approval | Shape::YesNo) => Some(Reply::No),
                ('s', Shape::Approval) if ask.grants.iter().any(|g| g == "session") => Some(Reply::Grant { scope: "session".into() }),
                ('h', Shape::Approval) if ask.grants.iter().any(|g| g == "here") => Some(Reply::Grant { scope: "here".into() }),
                _ => None,
            };
            if let Some(r) = reply {
                return self.answer(&ask, r);
            }
        }
        match k.code {
            KeyCode::Char('c' | 'd') if ctrl => {
                self.quit = true;
                return vec![Up::Quit];
            }
            KeyCode::Char('u') if ctrl => self.set_input(String::new()),
            KeyCode::Char('a' | 'e') if ctrl => drop(self.line.cursor_key(&k)),
            KeyCode::Left | KeyCode::Right if alt => return self.step_sub(k.code == KeyCode::Right),
            KeyCode::PageUp => self.page_by(true),
            KeyCode::PageDown => self.page_by(false),
            KeyCode::Esc if kind == Some(FieldKind::Chat) && let Some(ups) = self.shape_esc() => return ups,
            // `esc` on an empty line goes up one level, out of a chat to the orchestrator
            KeyCode::Esc if kind == Some(FieldKind::Chat) && self.line.buf.is_empty() && self.here().is_some_and(|h| !h.top) => return vec![Up::Intent { intent: Intent::Leave }],
            // a walk through the history gives back the line typed before it (and Esc goes on); a typed line is kept, and Esc does no more
            KeyCode::Esc if kind == Some(FieldKind::Chat) && !self.walk_back() && !self.line.buf.is_empty() => return Vec::new(),
            _ => {}
        }
        // `/` finds in a list as in a table and the Log, in the rows the screen holds
        if let Some(f) = self.field().filter(|f| f.kind == FieldKind::List).cloned()
            && let Some(ups) = crate::display::win::finding(self, &f, k)
        {
            return ups;
        }
        if let Some(ups) = self.node_key(k, kind == Some(FieldKind::Chat)) {
            return ups;
        }
        // nothing took the Esc: it goes to the first tab, Chat; the tab it left keeps its sub-tab and its scroll
        if k.code == KeyCode::Esc {
            return self.home();
        }
        match kind {
            Some(FieldKind::Chat) => self.chat_key(k),
            Some(FieldKind::List | FieldKind::Form) => self.list_key(k),
            _ => Vec::new(),
        }
    }

    /// `⇥` or `⇧⇥`: a Path or Ref being typed (in a field or a row) completes from its options, the composer from its candidates; elsewhere
    /// nothing.
    fn tab_key(&mut self, k: KeyEvent) -> Vec<Up> {
        if self.edit.is_some() || self.restart.is_some() || self.jump.is_some() || self.sheet {
            return Vec::new();
        }
        if self.row_ed.is_some() {
            return self.row_key(k);
        }
        if self.field().is_some_and(|f| f.kind.is_value()) {
            return self.val_key(k).unwrap_or_default();
        }
        self.complete_tab(k.code == KeyCode::BackTab || k.modifiers.contains(KeyModifiers::SHIFT))
    }

    fn chat_key(&mut self, k: KeyEvent) -> Vec<Up> {
        match k.code {
            // a new line in the draft: ⇧⏎ (where the terminal tells it from ⏎) or ⌃j
            KeyCode::Enter if k.modifiers.contains(KeyModifiers::SHIFT) => self.line.insert('\n'),
            KeyCode::Char('j') if k.modifiers.contains(KeyModifiers::CONTROL) => self.line.insert('\n'),
            KeyCode::Enter => {
                let text = self.line.buf.trim().to_string();
                if text.is_empty() {
                    // a bare Enter on a question with options takes the one it recommends
                    let pick = self.open_ask().filter(|a| a.shape == Shape::Choice).and_then(|a| ask::options(a, "").into_iter().find(|o| o.hint).map(|o| (a.clone(), o)));
                    if let Some((a, o)) = pick {
                        return vec![Up::Intent { intent: o.intent(&a) }];
                    }
                    // else the newest row of the chat, when it leads somewhere: a refusal's way out
                    return self.newest_act().map(|act| self.run_act(act)).unwrap_or_default();
                }
                // a line that is one key-like word is held on its card before anything of it goes
                if crate::chat::held::key_like(&text) {
                    return self.hold(text);
                }
                if let Some(ups) = self.shape_enter(&text) {
                    return ups;
                }
                self.set_input(String::new());
                self.scroll.remove(&page_bind(self));
                if let Some(ups) = self.steer(&text) {
                    return ups;
                }
                return self.say(text);
            }
            KeyCode::Up => self.arrow(true),
            KeyCode::Down => self.arrow(false),
            // `←` on an empty line, in a chat under the orchestrator, goes back to it; with text it is the cursor's
            KeyCode::Left if self.line.buf.is_empty() && k.modifiers.is_empty() && self.here().is_some_and(|h| !h.top) => return Self::enter(""),
            _ => drop(self.line.cursor_key(&k) || self.line.edit(&k)),
        }
        Vec::new()
    }

    /// ⏎ or space on the head of a section of a list folds or unfolds it. True when the selected row is one.
    fn fold_row(&mut self) -> bool {
        let Some(bind) = self.field().filter(|f| f.kind == FieldKind::List).map(|f| f.bind.clone()) else { return false };
        let sel = self.place(&bind);
        let rows = self.rows.get(&bind).map_or(&[][..], Vec::as_slice);
        if !rows.get(sel).is_some_and(|_| crate::display::is_header(rows, sel)) {
            return false;
        }
        let key = rows[sel].key.clone();
        crate::display::toggle(&mut self.win(&bind).flip, key);
        true
    }

    fn list_key(&mut self, k: KeyEvent) -> Vec<Up> {
        match k.code {
            KeyCode::Up => self.move_sel(false),
            KeyCode::Down => self.move_sel(true),
            KeyCode::Enter | KeyCode::Char(' ') if self.fold_row() => {}
            KeyCode::Enter => {
                if let Some(act) = self.row_act(None) {
                    return self.run_act(act);
                }
            }
            KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(act) = self.row_act(Some(&c.to_string())) {
                    return self.run_act(act);
                }
            }
            _ => {}
        }
        Vec::new()
    }
}

fn page_bind(app: &App) -> String {
    app.field().map(|f| f.bind.clone()).unwrap_or_default()
}
