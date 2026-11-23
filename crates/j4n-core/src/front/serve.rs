//! One client of the front: it takes what the TUI sends (`Up`) and the events of the Log, and gives back what the TUI is to
//! draw (`Down`). No threads and no channels here: a caller feeds it and sends what it returns, so a test can drive it and
//! read every message, and `Front::connect` is the only place that spawns.

use crate::text::line as clip;
use super::rows::{self, Rows};
use super::window::{self, Ask};
use super::{Front, Said, askview, compose, page, screen};
use crate::cost::Show;
use crate::log::Ev;
use crate::task::{Row, State};
use j4n_proto::{Act, AskView, Cell, Control, Down, FieldKind, Intent, Node, Row as PRow, Tone, Up};
use serde_json::Value as Json;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

/// The most lines of a log that a view reads.
const REPLAY: usize = 500;
/// How much of a provider's window is used before the bottom bar says so.
const WINDOW_LOW: f64 = 0.8;
/// A page with more nodes than this under it is sent without them, and the screen asks for them when it shows the page.
pub const LAZY: usize = 400;
/// How many events of the Log are read from the files in one go; a longer Log is read over several ticks.
const READ: usize = 50_000;
/// The least time between two windows of the Log sent for the events that are written.
const EVERY: Duration = Duration::from_millis(50);
/// The most tasks listed.
const TASKS_SHOWN: usize = 200;

pub(super) fn usd(micros: i64) -> String {
    format!("${:.4}", micros as f64 / 1e6)
}

fn tone_of(state: &State) -> Tone {
    match state {
        State::Running => Tone::Accent,
        State::Waiting { .. } => Tone::Warn,
        State::Done { ok: true } => Tone::Good,
        State::Done { ok: false } => Tone::Bad,
        State::Stopped { .. } => Tone::Dim,
        _ => Tone::Plain,
    }
}

pub struct Serve {
    front: Front,
    /// The thread whose chat, tasks and strip the screen shows now: the chat in hand's own, or the one the screen opened on.
    thread: String,
    /// The thread the screen opened on (the chat field's binding is `chat:<home>` whichever chat is in hand), the chat in hand (empty:
    /// the orchestrator) and, in a branch chat, the branch task it shows.
    home: String,
    here: String,
    branch: Option<String>,
    /// `chat:here` as it was sent last, and when the list of the chats was last sent for the clock's sake.
    sent_here: Json,
    list_at: Option<Instant>,
    /// The chat of the thread, folded from its events as they come.
    chat: Rows,
    watched: BTreeSet<String>,
    asks: Vec<AskView>,
    /// The strip of the composer as it was sent last.
    strip: Json,
    rev: u64,
    /// The look as it was sent last: what `:look` says is sent again when it differs.
    look: j4n_proto::Look,
    /// How many changes of the settings this client was sent the tree for: after an edit the look and the tree are sent again.
    set_rev: u64,
    /// The tree as it was sent last: after an edit it is sent again only when a page's `when` made it another.
    sent_tree: Option<Node>,
    /// The rows of each binding that were sent last (`from`, `total`, the rows), so that only a change is sent again.
    sent: BTreeMap<String, (u64, u64, Vec<PRow>)>,
    /// The window the screen asked for of each binding, and the columns of the fields that show a window (their sorts).
    asked: BTreeMap<String, Ask>,
    windowed: BTreeMap<String, Vec<String>>,
    /// Events were written that the Log's window has not been sent for, and when it was last sent.
    log_dirty: bool,
    log_sent: Option<Instant>,
    /// The least time between two windows of the Log sent for the events that are written.
    every: Duration,
    /// Rows a binding may have and be sent whole, and the rows of the window it is sent before the screen asks for one.
    whole: usize,
    first: u64,
    /// What the chart fields are made of (`front/charts.rs`).
    charts: super::charts::Charts,
    /// How many of the pre-flight's probes had landed when the views were last sent.
    landed: u64,
    /// When the archive was last swept: a minute apart, and once as the screen starts.
    swept: Option<Instant>,
    /// What the completion keeps between keys: the project's file names as last read, and the Log as read so far.
    kept: super::complete::Kept,
}

impl Serve {
    pub fn new(front: Front) -> Serve {
        let (thread, look) = (front.cfg().thread.clone(), front.look());
        // what was held when J4n last stopped is asked about again
        front.mail_resume(None);
        front.route_resume();
        front.need_check(&thread);
        front.archive_sweep();
        Serve { front, home: thread.clone(), here: String::new(), branch: None, sent_here: Json::Null, list_at: None, thread, look, set_rev: 0, sent_tree: None, chat: Rows::new(), watched: BTreeSet::new(), asks: Vec::new(), strip: Json::Null, rev: 0, sent: Default::default(), asked: Default::default(), windowed: Default::default(), log_dirty: false, log_sent: None, every: EVERY, whole: window::WHOLE, first: window::FIRST, charts: Default::default(), landed: 0, swept: Some(Instant::now()), kept: Default::default() }
    }

    /// The same client, sent a window of the Log for the events that are written no oftener than every `every`.
    pub fn spaced(mut self, every: Duration) -> Serve {
        self.every = every;
        self
    }

    /// The same client, sent a binding of a field that shows a window whole while it has `whole` rows or fewer, and else the first `first`.
    pub fn windowed_over(mut self, whole: usize, first: u64) -> Serve {
        (self.whole, self.first) = (whole, first);
        self
    }

    pub fn thread(&self) -> &str {
        &self.thread
    }

    /// The chat in hand: its id, empty at the top (the orchestrator).
    pub fn here(&self) -> &str {
        &self.here
    }

    /// What the TUI is told when it says hello: who this is, what its keys do, the look, and the tree.
    fn hello(&mut self) -> Vec<Down> {
        let c = self.front.cfg();
        let hello = Down::Hello { api: j4n_proto::API, name: c.name.clone(), version: c.version.clone(), project: c.project.clone() };
        self.look = self.front.look();
        self.set_rev = self.front.settings_rev();
        let tree = self.tree();
        vec![hello, Down::Keys { binds: self.front.screen().keys.clone() }, Down::Theme { look: self.look.clone() }, Down::Tree { node: tree }]
    }

    /// The tree of the pages as the settings say now (a page's `when` reads them), its fields noted.
    fn tree(&mut self) -> Node {
        let screen = self.front.screen();
        let mut tree = screen.titled(screen.pages.tree_lazy(&self.home, &crate::view::outer(screen.set.clone()), LAZY));
        self.leave_empty_out(&mut tree);
        self.note_fields(&tree);
        self.sent_tree = Some(tree.clone());
        tree
    }

    /// Leave out of `node` the pages whose field says `hide_empty` and whose view lists nothing now, unless `tabs.all` is on: a sub-tab
    /// that would open on an empty list is not there until it has a row (the tree is made again when a setting changes).
    fn leave_empty_out(&self, node: &mut Node) {
        if self.front.screen().set.pointer("/tabs/all") == crate::value::Value::Bool(true) {
            return;
        }
        let empty = |n: &Node| {
            n.field.as_ref().is_some_and(|f| f.props.get(page::HIDE_EMPTY) == Some(&Json::Bool(true)) && f.bind.strip_prefix("view:").and_then(|v| self.view_rows(v)).is_some_and(|r| r.is_empty()))
        };
        node.children.retain(|c| !empty(c));
        for c in &mut node.children {
            self.leave_empty_out(c);
        }
    }

    /// Note the fields of `node` that show a window of their rows, and the columns they sort by.
    fn note_fields(&mut self, node: &Node) {
        if let Some(f) = node.field.as_ref().filter(|f| matches!(f.kind, FieldKind::Table | FieldKind::Log)) {
            let cols = match f.kind {
                FieldKind::Log => window::LOG_COLUMNS.iter().map(|c| c.to_string()).collect(),
                _ => f.props.get("columns").and_then(Json::as_array).map(|l| l.iter().filter_map(|c| c.as_str().or_else(|| c.get("name").and_then(Json::as_str)).map(String::from)).collect()).unwrap_or_default(),
            };
            self.windowed.insert(f.bind.clone(), cols);
        }
        node.children.iter().for_each(|c| self.note_fields(c));
    }

    /// What the client asked for; true when it is to be closed.
    pub fn up(&mut self, up: Up) -> (Vec<Down>, bool) {
        match up {
            Up::Hello { api } if api > j4n_proto::API => {
                (vec![Down::Toast { text: format!("this J4n speaks protocol {}, the screen asked for {api}", j4n_proto::API), tone: Tone::Bad }], true)
            }
            Up::Hello { .. } => (self.hello(), false),
            Up::Watch { bind } => {
                self.watched.insert(bind.clone());
                (self.snapshot(&bind), false)
            }
            Up::Unwatch { bind } => {
                self.watched.remove(&bind);
                self.sent.remove(&bind);
                self.asked.remove(&bind);
                (Vec::new(), false)
            }
            Up::Draft { text, target } => {
                self.front.draft(&self.thread, &text, &target);
                let mut downs = self.shape_downs();
                // with a question open, what the line would answer
                if self.watched.contains(j4n_proto::COMPOSER_BIND) {
                    self.rev += 1;
                    let open: Vec<_> = self.front.tasks().book().open_asks().into_iter().filter(|a| a.thread == self.thread).collect();
                    downs.push(Down::Value { bind: j4n_proto::COMPOSER_BIND.into(), value: compose::guess(&text, &open), rev: self.rev });
                }
                (downs, false)
            }
            Up::Resize { .. } => (Vec::new(), false),
            Up::Window { bind, from, count, sort, find } if self.watched.contains(&bind) => {
                if bind == j4n_proto::COMPLETE {
                    self.kept.asked();
                }
                self.asked.insert(bind.clone(), Ask { from, count, sort, find });
                (self.rows_now(&bind, true), false)
            }
            Up::Window { .. } => (Vec::new(), false),
            Up::Quit => (Vec::new(), true),
            Up::Intent { intent } => {
                if matches!(intent, Intent::Say { .. }) {
                    self.kept.sent();
                }
                self.intent(intent)
            }
        }
    }

    /// A plain line of the chat in hand, taken on a thread of its own: the core writes the line to the Log first and then spends what time it
    /// takes (Jev weighs or routes it, a task is made), and the server thread, which would else stand in that call, sends the row of the line
    /// at once. `None`: the line is not one of those (a lane's word, a branch chat's line, anything but a line), and is taken here.
    fn say_job(&mut self, up: &Up) -> Option<Box<dyn FnOnce() -> Said + Send>> {
        let Up::Intent { intent: Intent::Say { text, with } } = up else { return None };
        self.kept.sent();
        if self.branch.is_some() {
            return None;
        }
        // the form's add row said its line: the draft is done with, and the form opens on the next free name
        if text.trim_start().starts_with(":account add ") {
            self.front.draft_clear();
        }
        let (front, thread, text, with) = (self.front.clone(), self.thread.clone(), text.clone(), with.clone());
        Some(Box::new(move || front.say_with(&thread, &text, &with)))
    }

    fn intent(&mut self, intent: Intent) -> (Vec<Down>, bool) {
        let thread = self.thread.clone();
        let said = match intent {
            // a line typed in a branch chat steers its lane; a command is the chat's own
            // the words that act on a chat are not a lane's: refused, and the chat above is not touched
            Intent::Say { text, .. } if self.branch.is_some() && self.lane_word(&text).is_some() => {
                let why = format!("this is a lane of {}: say :{} in {}", self.thread, self.lane_word(&text).unwrap_or_default(), self.thread);
                self.front.note(&self.here, &why, "dim");
                Said::Refused(why)
            }
            Intent::Say { text, .. } if self.branch.is_some() && !text.trim_start().starts_with([':', '/']) => self.front.branch_say(&thread, self.branch.as_deref().unwrap_or_default(), text.trim()),
            Intent::Say { text, with } => {
                // the form's add row said its line: the draft is done with, and the form opens on the next free name
                if text.trim_start().starts_with(":account add ") {
                    self.front.draft_clear();
                }
                self.front.say_with(&thread, &text, &with)
            }
            Intent::Enter { chat } => return (self.enter(&chat), false),
            Intent::Leave => return (self.leave(), false),
            Intent::Answer { ask, reply } => self.front.reply(&thread, &ask, reply),
            Intent::Control { task, control } => self.front.control(&thread, &task, control),
            Intent::Run { command, args } => self.front.say(&thread, &format!(":{command} {}", args.join(" "))),
            Intent::Set { kind, field, value, .. } if kind == "thread" => {
                let word = value.as_str().unwrap_or_default().trim().to_string();
                let named = word.clone();
                let binding = self.front.binding(&thread);
                let args: Vec<String> = match (field.as_str(), binding) {
                    ("doer", b) => [word].into_iter().chain(b.and_then(|b| b.model)).collect(),
                    ("model", Some(b)) => vec![b.doer, word],
                    _ => Vec::new(),
                };
                // a doer set by name is a doer: a word that is none is no model word either
                let unknown = (field == "doer").then_some(named).filter(|w| !self.front.knows_doer(w));
                unknown.map_or_else(|| self.front.switch(&thread, &args), |w| Err(format!("no doer called `{w}`"))).unwrap_or_else(|why| {
                    self.front.note(&thread, &why, "warn");
                    Said::Refused(why)
                })
            }
            // a field of a form the screen fills (Agents › Add account): the core keeps the draft, and the form's rows are sent again
            Intent::Set { kind, id, field, value } if kind == "draft" => {
                return match self.front.draft_set(&id, &field, &value) {
                    Ok(()) => (self.views_again(), false),
                    Err(why) => {
                        self.front.note(&thread, &why, "warn");
                        (Vec::new(), false)
                    }
                };
            }
            // a provider's key, typed in its secret editor: kept in J4n's own key file, never a Change; its row says so at once
            Intent::Key { id, value } => {
                self.front.key_set(&thread, &id, &value);
                return (self.views_again(), false);
            }
            // a key comes by its own intent only: a value set on `key:` is not read, and its words name no part of it
            Intent::Set { kind, .. } if kind == "key" => self.front.key_said(&thread, &[]),
            // a setting, a field of an account or of a plan: the operator's edit, applied at once
            Intent::Set { kind, id, field, value } => self.front.set_said(&thread, &kind, &id, &field, &value),
            // a page whose children were left out is sent with them; any other is gone to
            Intent::Open { node } => {
                let screen = self.front.screen();
                return match screen.pages.subtree(&node, &self.home, &crate::view::outer(screen.set.clone()), LAZY) {
                    Some(sub) => {
                        let mut sub = screen.titled(sub);
                        self.leave_empty_out(&mut sub);
                        self.note_fields(&sub);
                        (vec![Down::Tree { node: sub }], false)
                    }
                    None => (vec![Down::Focus { node }], false),
                };
            }
        };
        self.after(said)
    }

    /// What the screen is sent for what came of a line: the page it opened, the look, the strip.
    fn after(&mut self, said: Said) -> (Vec<Down>, bool) {
        let mut downs = match &said {
            // a page opened by a line may show what the line just put in a draft (`:account add <connector>`)
            Said::Open(node) => std::iter::once(Down::Focus { node: node.clone() }).chain(self.views_again()).collect(),
            Said::Enter(chat) => self.enter(chat),
            Said::Chats => vec![Down::Focus { node: "chats".into() }],
            _ => Vec::new(),
        };
        downs.extend(self.look_down());
        // a line taken starts work: the strip says so now, not when the doer has spoken
        if matches!(said, Said::Started { .. } | Said::Steered { .. }) && self.watched.contains(j4n_proto::COMPOSER_STRIP) && self.strip_value() != self.strip {
            downs.extend(self.snapshot(j4n_proto::COMPOSER_STRIP));
        }
        (downs, said == Said::Quit)
    }

    /// The look, when it is not the one the screen was sent last (`:look`, or an `:undo` of it, by any client); and after any change of
    /// the settings, the tree again, since a page's `when` reads them, and the views that are watched.
    fn look_down(&mut self) -> Vec<Down> {
        let mut out = Vec::new();
        let look = self.front.look();
        if look != self.look {
            self.look = look.clone();
            out.push(Down::Theme { look });
        }
        let rev = self.front.settings_rev();
        if rev != self.set_rev {
            self.set_rev = rev;
            let was = self.sent_tree.clone();
            let node = self.tree();
            if was.as_ref() != Some(&node) {
                out.push(Down::Tree { node });
            }
            out.extend(self.views_again());
            // the chat's rows say money as `cost.show` said it when they were folded: folded again when it is another word now
            let bind = page::chat(&self.home);
            if self.watched.contains(&bind) && self.chat.show != self.front.screen().show() {
                let rows = self.replay();
                out.extend(self.send_rows(&bind, rows, false));
            }
        }
        out
    }

    // ---- what the Log says ----

    /// What an event changes on the screen.
    pub fn ev(&mut self, ev: &Ev) -> Vec<Down> {
        let mut out = self.complete_step();
        let bind = page::chat(&self.home);
        self.front.usage_see(ev);
        // a view held in the chat is drawn again when what it shows changes, whichever thread the run was in
        if matches!(ev.kind.as_str(), "run.metrics" | "bench.case" | "task.settled" | "task.stopped") {
            self.charts.see(ev);
        }
        let gauge = matches!(ev.kind.as_str(), "run.window" | "run.metrics" | "bench.case" | "task.settled" | "task.stopped") && self.chat.has_view();
        let mine = self.mine(ev);
        if mine && ev.kind == "chat.cleared" {
            self.chat = rows::fold_in(self.front.screen().show(), self.group(), std::iter::empty());
        } else if mine && ev.kind == "chat.history" {
            self.replay();
        }
        if self.watched.contains(&bind) && (mine || gauge) {
            if mine && !matches!(ev.kind.as_str(), "chat.cleared" | "chat.history") {
                self.chat.group = self.group();
                self.chat.put(&super::chats::tagged(ev, &self.here));
            }
            let rows = self.filled(self.chat.rows());
            out.extend(self.send_rows(&bind, rows, false));
        }
        if !ev.is_flash() {
            self.front.look_see(ev);
            self.front.look_soon();
        }
        if self.watched.contains(j4n_proto::LOOK_FILE) && ev.kind == "look.made" {
            out.extend(self.snapshot(j4n_proto::LOOK_FILE));
        }
        if self.watched.contains(page::LOG) && !ev.is_flash() {
            self.front.logrows().push(ev);
            self.log_dirty = true;
            if self.log_sent.is_none_or(|t| t.elapsed() >= self.every) {
                out.extend(self.log_down(false));
            }
        }
        let k = ev.kind.as_str();
        // a limit was said, or a login was signed in: what waits out a limit goes to a login that is open
        if matches!(k, "task.cooling" | "account.signin" | "account.checked") {
            self.front.limits_sweep();
        }
        // the usage page, on what `/usage` counts: a window read, a run's cost, a login
        if self.watched.contains(page::USAGE) && matches!(k, "run.window" | "run.metrics" | "run.error" | "account.signin" | "account.checked" | "account.removed") {
            out.extend(self.snapshot(page::USAGE));
        }
        if matches!(k, "run.metrics" | "bench.case" | "task.settled" | "task.stopped") {
            let binds: Vec<String> = self.watched.iter().filter(|b| super::charts::is_chart(b)).cloned().collect();
            out.extend(binds.iter().flat_map(|b| self.snapshot(b)));
        }
        // a doc may have changed while the task worked: the glossary is read again, and sent if it is not what it was
        if matches!(k, "task.settled" | "task.stopped") && self.watched.contains(j4n_proto::TERMS) && self.front.terms_changed() {
            out.extend(self.snapshot(j4n_proto::TERMS));
        }
        if matches!(k, "task.settled" | "task.stopped") {
            // the plan of the thread the task is of (a chat has its own), not of the one in hand
            let of = ev.thread.clone().filter(|_| self.front.chats_on()).unwrap_or_else(|| self.thread.clone());
            self.front.plan_step(&of);
            // a chat Main started ends in Main's plan
            if self.front.main_works() && of != self.front.cfg().thread {
                self.front.plan_step(&self.front.cfg().thread);
            }
        }
        out.extend(self.chats_again(k));
        if k.starts_with("plan.") || k.starts_with("task.") || k.starts_with("ask.") || k.starts_with("thread.") || k.starts_with("gate.") || k.starts_with("chat.route") || k.starts_with("chat.message") || matches!(k, "run.metrics" | "run.served" | "setup.asked") {
            for bind in [page::TASKS, page::BAR, page::FORM, page::ASKS] {
                if self.watched.contains(bind) {
                    out.extend(self.snapshot(bind));
                }
            }
        }
        if (k.starts_with("task.") || k.starts_with("thread.") || matches!(k, "run.metrics" | "run.served" | "run.list")) && self.watched.contains(j4n_proto::COMPOSER_STRIP) {
            let strip = self.strip_value();
            if strip != self.strip {
                out.extend(self.snapshot(j4n_proto::COMPOSER_STRIP));
            }
        }
        if k.starts_with("shape.") {
            out.extend(self.shape_downs());
            if self.watched.contains(page::BAR) {
                out.extend(self.snapshot(page::BAR));
            }
        }
        // a run that said nothing J4n could read is on the Agents tab, the one view a `run.` event changes
        if !k.starts_with("run.") || matches!(k, "run.metrics" | "run.served" | "run.health") {
            out.extend(self.views_again());
        }
        out
    }

    /// Everything that is watched, again, as it stands.
    pub fn resync(&mut self) -> Vec<Down> {
        let all: Vec<String> = self.watched.iter().cloned().collect();
        // the questions are sent only when they changed: what the client holds is what it may have missed, so say it all
        self.asks.clear();
        all.iter().flat_map(|b| self.snapshot(b)).collect()
    }

    // ---- what a binding holds ----

    fn snapshot(&mut self, bind: &str) -> Vec<Down> {
        self.rev += 1;
        let rev = self.rev;
        match bind {
            page::LOG => {
                self.log_catch();
                self.log_down(true).into_iter().collect()
            }
            page::ASKS => {
                let open = self.ask_views();
                if open == self.asks {
                    return Vec::new();
                }
                self.asks = open.clone();
                vec![Down::Asks { open }]
            }
            j4n_proto::COMPOSER_STRIP => {
                self.strip = self.strip_value();
                vec![Down::Value { bind: bind.into(), value: self.strip.clone(), rev }]
            }
            // the binder's guess is made for a draft, and there is none yet
            j4n_proto::COMPOSER_BIND => vec![Down::Value { bind: bind.into(), value: Json::Null, rev }],
            j4n_proto::CHAT_HERE => {
                self.sent_here = self.here_value();
                vec![Down::Value { bind: bind.into(), value: self.sent_here.clone(), rev }]
            }
            j4n_proto::LOOK_FILE => vec![Down::Value { bind: bind.into(), value: super::lookfile::json(&self.front.look_rows()), rev }],
            j4n_proto::TERMS => vec![Down::Value { bind: bind.into(), value: self.front.terms_value(), rev }],
            page::SHAPE_ON => vec![Down::Value { bind: bind.into(), value: Json::Bool(self.front.desk().is_some_and(|d| d.on())), rev }],
            page::USAGE => vec![Down::Value { bind: bind.into(), value: self.front.view_value(super::usage::VIEW).map(|v| v["value"].clone()).unwrap_or(Json::Null), rev }],
            b if super::charts::is_chart(b) => vec![Down::Value { bind: b.into(), value: self.front.chart(&mut self.charts, b, &self.thread).unwrap_or(Json::Null), rev }],
            // the newest tasks of the thread, unless the screen asked for a window of them
            page::TASKS if !self.asked.contains_key(bind) => {
                let (rows, show) = (self.thread_rows(), self.front.screen().show());
                let plan: Vec<_> = self.front.switch_row(&self.thread).into_iter().chain(self.front.plan_rows(&self.thread)).chain(self.front.lane_rows(&self.thread)).collect();
                let total = (plan.len() + rows.len()) as u64;
                let shown = plan.into_iter().chain(rows.iter().rev().take(TASKS_SHOWN).map(|r| task_row(r, show))).collect();
                vec![Down::Rows { bind: bind.into(), rows: shown, total, from: 0, rev }]
            }
            _ => self.rows_now(bind, true),
        }
    }

    /// The rows of a binding, as the screen is to be sent them now: `None` when nothing is bound as `bind`.
    fn all_rows(&mut self, bind: &str) -> Option<Vec<PRow>> {
        if bind == page::chat(&self.home) {
            return Some(self.replay());
        }
        let show = self.front.screen().show();
        Some(match bind {
            page::TASKS => self.front.switch_row(&self.thread).into_iter().chain(self.front.plan_rows(&self.thread)).chain(self.front.lane_rows(&self.thread)).chain(self.thread_rows().iter().rev().map(|r| task_row(r, show))).collect(),
            j4n_proto::CHATS if self.front.chats_on() => self.front.chat_list(),
            j4n_proto::CHATS => Vec::new(),
            page::BAR => vec![self.bar()],
            page::FORM => self.form(),
            page::SHAPE_CARD => self.front.desk().map(|d| d.card(&self.thread)).unwrap_or_default(),
            page::COMMANDS => self.front.screen().palette(),
            page::KEYS_ROW => self.front.screen().hint_rows(),
            page::PACKS => self.front.screen().pack_rows_with(&self.front.needs_by_pack(&self.thread)),
            page::CATALOGUE => self.front.catalogue_rows(),
            other => return other.strip_prefix("view:").and_then(|id| self.view_rows(id)),
        })
    }

    /// The rows of `bind` now, sent (when `force` is not set, only if they differ from what was sent last).
    fn rows_now(&mut self, bind: &str, force: bool) -> Vec<Down> {
        if bind == page::LOG {
            self.log_catch();
            return self.log_down(force).into_iter().collect();
        }
        // the candidates are made from the find, so they are sent whole: a cut by the find would drop them and `~for`
        if bind == j4n_proto::COMPLETE {
            let ask = self.asked.get(bind).cloned().unwrap_or_default();
            let most = if ask.count == 0 { super::complete::MOST } else { usize::try_from(ask.count).unwrap_or(usize::MAX).min(super::complete::MOST) };
            let rows = self.front.complete(&self.thread, &ask.find, most, &mut self.kept);
            return self.deliver(bind, 0, rows.len() as u64, rows, force).into_iter().collect();
        }
        match self.all_rows(bind) {
            Some(rows) => self.send_rows(bind, rows, force).into_iter().collect(),
            None => vec![Down::Toast { text: format!("nothing is bound as `{bind}`"), tone: Tone::Warn }],
        }
    }

    /// `rows` as the screen is to have them: the window it asked for; else all of them, unless the field shows windows and they are
    /// many, and then the first. Sent only if they differ from what was sent last, or `force`.
    fn send_rows(&mut self, bind: &str, rows: Vec<PRow>, force: bool) -> Option<Down> {
        let many = rows.len() > self.whole && self.windowed.contains_key(bind);
        let (from, total, rows) = match self.asked.get(bind).cloned().or(many.then(|| Ask { count: self.first, ..Ask::default() })) {
            None => (0, rows.len() as u64, rows),
            Some(ask) => {
                let (shown, total) = window::cut(&rows, self.windowed.get(bind).map_or(&[], Vec::as_slice), &ask);
                (ask.from.min(total), total, shown)
            }
        };
        self.deliver(bind, from, total, rows, force)
    }

    fn deliver(&mut self, bind: &str, from: u64, total: u64, rows: Vec<PRow>, force: bool) -> Option<Down> {
        let held = (from, total, rows);
        if !force && self.sent.get(bind) == Some(&held) {
            return None;
        }
        self.rev += 1;
        self.sent.insert(bind.to_string(), held.clone());
        Some(Down::Rows { bind: bind.into(), rows: held.2, total, from, rev: self.rev })
    }

    // ---- the Log as rows ----

    /// Read more of the Log into the rows, a part at a time. Whether it grew.
    fn log_catch(&mut self) -> bool {
        let (log, mut rows) = (self.front.log(), self.front.logrows());
        let before = rows.len();
        rows.hear(log.head());
        rows.catch_up(log, READ);
        rows.len() != before
    }

    /// The window of the Log the screen asked for (the last rows, until it asks), sent if it differs from the last. A window that held
    /// the end of the Log, in the order it is written in, holds the end still as events are written.
    fn log_down(&mut self, force: bool) -> Option<Down> {
        self.log_dirty = false;
        self.log_sent = Some(Instant::now());
        let (mut idx, before) = (self.front.logrows(), self.sent.get(page::LOG).map_or(0, |s| s.1));
        let mut ask = self.asked.get(page::LOG).cloned().unwrap_or_else(|| Ask { from: idx.rows().saturating_sub(self.first), count: self.first, ..Ask::default() });
        if ask.sort.is_empty() && ask.find.is_empty() && ask.from + ask.count >= before {
            ask.from = idx.rows().saturating_sub(ask.count.min(window::MOST));
        }
        let (mut rows, total) = idx.window(&ask);
        idx.fill(Some(self.front.log()), &mut rows);
        drop(idx);
        if self.asked.contains_key(page::LOG) {
            self.asked.insert(page::LOG.into(), ask.clone());
        }
        self.deliver(page::LOG, ask.from.min(total), total, rows, force)
    }

    /// The rows of a view, from what the front holds; `None` when there is no such view.
    fn view_rows(&self, id: &str) -> Option<Vec<PRow>> {
        let screen = self.front.screen();
        let view = screen.views.get(id)?;
        Some(view.rows(&Src { serve: self }, &crate::view::outer(screen.set.clone())))
    }

    /// The completion's clock, on every tick and event (a busy Log sends no tick): a pause long enough is put to the Decider, and an ask
    /// that came back sends the window again, in its order.
    fn complete_step(&mut self) -> Vec<Down> {
        if self.watched.contains(j4n_proto::COMPLETE) && self.front.complete_tick(&self.thread, &mut self.kept) {
            return self.rows_now(j4n_proto::COMPLETE, false);
        }
        Vec::new()
    }

    /// The card and the switch, again, for whoever watches them.
    fn shape_downs(&mut self) -> Vec<Down> {
        let binds: Vec<String> = [page::SHAPE_ON, page::SHAPE_CARD].into_iter().filter(|b| self.watched.contains(*b)).map(String::from).collect();
        binds.iter().flat_map(|b| self.snapshot(b)).collect()
    }

    /// The views that are watched, again, for those whose rows changed.
    fn views_again(&mut self) -> Vec<Down> {
        let mut out = Vec::new();
        let binds: Vec<String> = self.watched.iter().filter(|b| b.starts_with("view:")).cloned().collect();
        for bind in binds {
            let Some(rows) = bind.strip_prefix("view:").and_then(|id| self.view_rows(id)) else { continue };
            out.extend(self.send_rows(&bind, rows, false));
        }
        // Settings › Packs reads the packs, whose rows say a switch that waits for the next start (⟳)
        if self.watched.contains(page::PACKS) {
            out.extend(self.rows_now(page::PACKS, false));
        }
        // the catalogue page says whether the quick look reads it, and a switch said on the page acts at once; `~for` says `complete.on`
        for bind in [page::CATALOGUE, j4n_proto::COMPLETE] {
            if self.watched.contains(bind) {
                out.extend(self.rows_now(bind, false));
            }
        }
        out
    }

    /// The chat of the thread from the Log, as it stands: what a client that has just come, or come back, is shown.
    fn replay(&mut self) -> Vec<PRow> {
        let events: Vec<Ev> = self.front.log().read(0).filter(|e| self.mine(e)).collect();
        // what `:clear` emptied stays out until `:history` (the Log keeps it)
        let cut = super::chats::cleared_before(&events);
        let tagged: Vec<Ev> = events.iter().filter(|e| e.seq > cut).map(|e| super::chats::tagged(e, &self.here)).collect();
        self.chat = rows::fold_in(self.front.screen().show(), self.group(), tagged.iter());
        self.filled(self.chat.rows())
    }

    /// Under `orchestrate` the chat is drawn as group work: the word its own lane is headed with, `Main` at the top and the chat's id in a chat.
    fn group(&self) -> Option<String> {
        self.front.main_works().then(|| if self.here.is_empty() { "Main".to_string() } else { self.here.clone() })
    }

    /// The word of a command that acts on a whole chat (`clear`, `fresh`, `history`) when the line is one.
    fn lane_word(&self, text: &str) -> Option<String> {
        self.front.phrases().parse(text.trim()).map(|c| c.name).filter(|n| matches!(n.as_str(), "clear" | "fresh" | "history"))
    }

    /// Whether an event is of the chat in hand: its thread's, and in a branch chat only its branch task's and what the chat itself said.
    fn mine(&self, ev: &Ev) -> bool {
        // a message is of the chat that sends it, the chat it is for, and the orchestrator (all of them); it is written in the receiver's thread
        if matches!(ev.kind.as_str(), "chat.message" | "chat.message.delivered") {
            let is = |k: &str| ev.data.get(k).and_then(Json::as_str);
            return self.front.chats_on() && self.branch.is_none() && (self.here.is_empty() || is("from") == Some(self.here.as_str()) || is("to") == Some(self.here.as_str()));
        }
        let of = ev.thread.as_deref();
        match &self.branch {
            None => of == Some(self.thread.as_str()),
            Some(task) => of == Some(self.here.as_str()) || (of == Some(self.thread.as_str()) && ev.task.as_deref().is_some_and(|t| self.under(t, task))),
        }
    }

    /// Whether `task` is `branch` or something that hangs from it.
    fn under(&self, task: &str, branch: &str) -> bool {
        let book = self.front.tasks().book();
        let mut at = Some(task.to_string());
        for _ in 0..8 {
            match at.take() {
                Some(t) if t == branch => return true,
                Some(t) => at = book.get(&t).and_then(|r| r.spec.parent),
                None => return false,
            }
        }
        false
    }

    // ---- chats ----

    /// `chat:here` now: null with the switch off.
    fn here_value(&self) -> Json {
        if self.front.chats_on() { self.front.chat_here(&self.here) } else { Json::Null }
    }

    /// Go into the chat `to` (empty: the orchestrator). Everything the screen holds of a chat is sent again for it, `chat:here` first.
    fn enter(&mut self, to: &str) -> Vec<Down> {
        if !self.front.chats_on() {
            return vec![Down::Toast { text: "the chats are off: Settings › All › chats.on turns them on".into(), tone: Tone::Warn }];
        }
        // `@next`: the next chat that waits for you, then the next that runs (`⇥1`)
        let to = if to == "@next" {
            match self.front.next_chat_after(&self.here) {
                Some(c) => c,
                None => return vec![Down::Toast { text: "no chat waits or runs".into(), tone: Tone::Dim }],
            }
        } else {
            to.to_string()
        };
        let (here, thread, branch) = match to.as_str() {
            "" => (String::new(), self.home.clone(), None),
            id => match self.front.chat(id) {
                Some(c) => (c.id.clone(), c.parent.clone().unwrap_or_else(|| c.id.clone()), c.task.clone()),
                None => return vec![Down::Toast { text: format!("there is no chat `{id}`"), tone: Tone::Warn }],
            },
        };
        if here == self.here {
            return Vec::new();
        }
        (self.here, self.thread, self.branch) = (here, thread, branch);
        self.front.mail_resume(Some(&self.here).filter(|h| !h.is_empty()).map(String::as_str));
        self.chat = Rows::new();
        self.strip = Json::Null;
        self.sent.clear();
        self.asked.remove(&page::chat(&self.home));
        let mut downs = Vec::new();
        if !self.watched.contains(j4n_proto::CHAT_HERE) {
            self.rev += 1;
            self.sent_here = self.here_value();
            downs.push(Down::Value { bind: j4n_proto::CHAT_HERE.into(), value: self.sent_here.clone(), rev: self.rev });
        }
        downs.extend(self.resync());
        downs
    }

    /// `esc`: up one level, out of a branch to its chat, out of a chat to the orchestrator; at the top nothing.
    fn leave(&mut self) -> Vec<Down> {
        let up = match self.front.chat(&self.here) {
            Some(c) => c.parent.unwrap_or_default(),
            None => return Vec::new(),
        };
        self.enter(&up)
    }

    /// The list of the chats and `chat:here`, sent again when what an event changed makes them differ from what was sent.
    fn chats_again(&mut self, kind: &str) -> Vec<Down> {
        // the word of `lanes` turns the chats on or off while J4n runs: the list and `chat:here` follow it
        let flipped = kind == "lanes.set";
        if !flipped && (!self.front.chats_on() || !(kind.starts_with("chat.") || kind.starts_with("task.") || kind.starts_with("thread.") || kind.starts_with("ask.") || kind.starts_with("plan.") || kind == "run.window")) {
            return Vec::new();
        }
        let mut out = Vec::new();
        // the chats went off while one was in hand: the screen goes back to the thread it opened on
        if flipped && !self.front.chats_on() && !self.here.is_empty() {
            (self.here, self.thread, self.branch) = (String::new(), self.home.clone(), None);
            self.chat = Rows::new();
            self.strip = Json::Null;
            self.sent.clear();
            out.extend(self.resync());
        }
        if self.watched.contains(j4n_proto::CHATS) {
            out.extend(self.rows_now(j4n_proto::CHATS, false));
        }
        if self.watched.contains(j4n_proto::CHAT_HERE) && self.here_value() != self.sent_here {
            out.extend(self.snapshot(j4n_proto::CHAT_HERE));
        }
        out
    }

    /// The rows of the chat with the value of every view they hold, as it stands now (`data.view`: `{id}` from the Log's row, and
    /// `{id, title, field, value}` as the screen is sent it). One value is made for each kind of view.
    fn filled(&mut self, mut rows: Vec<PRow>) -> Vec<PRow> {
        let mut made: BTreeMap<String, Option<Json>> = BTreeMap::new();
        // a lane whose task has a chat of its own says it (`c3.1`): the lane row is the door into it
        if self.front.chats_on() {
            let doors: Vec<(String, String)> = self.front.chats().into_iter().filter_map(|c| c.task.map(|t| (t, c.id))).collect();
            for r in rows.iter_mut().filter(|r| r.data.as_ref().is_some_and(|d| d.get("lane").is_some())) {
                if let Some((_, chat)) = doors.iter().find(|(t, _)| *t == r.key) {
                    r.data = r.data.take().map(|mut d| {
                        d["lane"]["chat"] = Json::String(chat.clone());
                        d
                    });
                    // its chat is at the right of the row, and its steps are read there: they are folded here, but for a lane that failed
                    r.cells.push(Cell::new(format!("{chat} ▸"), Tone::Dim));
                    r.fold = r.fold.or(Some(j4n_proto::Fold::Folded)).filter(|f| *f != j4n_proto::Fold::Open);
                }
            }
        }
        for r in rows.iter_mut() {
            let Some(id) = r.data.as_ref().and_then(|d| d["view"]["id"].as_str()).map(String::from) else { continue };
            let value = made.entry(id.clone()).or_insert_with(|| self.view_value(&id)).clone();
            let title = r.cells.first().map_or_else(String::new, |c| c.text.clone());
            let mut view = value.unwrap_or_else(|| serde_json::json!({ "field": "chip", "value": { "text": format!("no view called {id}"), "tone": "warn" } }));
            (view["id"], view["title"]) = (Json::String(id), Json::String(title));
            r.data = Some(serde_json::json!({ "view": view }));
        }
        rows
    }

    /// The value of a view the chat holds: `usage`, `context`, or a chart drawn as the page that has it draws it, `{field, value, props}`.
    fn view_value(&mut self, id: &str) -> Option<Json> {
        if id == super::context::VIEW {
            return Some(self.front.context_value(&self.thread));
        }
        if !super::charts::is_chart(id) {
            return self.front.view_value(id);
        }
        let (kind, props) = self.front.screen().pages.chart_part(id)?;
        Some(match self.front.chart(&mut self.charts, id, &self.thread) {
            Some(value) => serde_json::json!({ "field": kind, "value": value, "props": props }),
            None => serde_json::json!({ "field": "chip", "value": { "text": "nothing to draw yet", "tone": "dim" } }),
        })
    }

    fn thread_rows(&self) -> Vec<Row> {
        self.front.tasks().book().rows().into_iter().filter(|r| r.spec.doer != crate::task::HUMAN && r.spec.thread.as_deref() == Some(self.thread.as_str())).collect()
    }

    /// Who the thread's lines go to, `doer · model`: the model that served the thread's last task when it ran under the binding as it
    /// is now, else the pin, else that it is not known yet. A bare doer is never said. A thread bound to an account says it last.
    fn who_now(&self) -> Option<String> {
        let b = self.front.binding(&self.thread)?;
        let served = self.thread_rows().into_iter().next_back().filter(|r| r.spec.doer == b.doer && r.spec.tune.model == b.model).map(|r| r.model).unwrap_or_default();
        let who = crate::task::who(&b.doer, &served, b.model.as_deref().unwrap_or_default());
        let ran_as = self.thread_rows().into_iter().next_back().filter(|r| r.spec.doer == b.doer).and_then(|r| r.spec.account);
        Some(match b.account.as_deref().or(ran_as.as_deref()).filter(|a| !a.is_empty()) {
            Some(account) => format!("{who} · {account}"),
            None => who,
        })
    }

    fn mode_word(&self) -> &'static str {
        if self.front.cfg().mode == crate::task::Mode::Read { "read" } else { "write" }
    }

    /// What the thread's tasks cost, the way `cost.show` says it: the newest task's spend limit when it has one, else all of them.
    fn money(&self, rows: &[Row]) -> Cell {
        let (spent, listed): (i64, i64) = (rows.iter().map(|r| r.spent).sum(), rows.iter().map(|r| r.listed).sum());
        let planned = rows.iter().any(|r| r.planned);
        let mut shares = crate::cost::Shares::default();
        rows.iter().for_each(|r| shares.merge(&r.shares));
        // the newest task has the spend limit that counts: how far it has come, and how far it may
        let show = self.front.screen().show();
        match rows.last().and_then(|r| r.spec.limits.usd_soft.map(|soft| (r, soft))) {
            Some((r, soft)) => Cell::new(show.money_of(self.share_words(&r.shares, rows).as_deref(), r.planned, &usd(r.spent), Some(&usd(crate::task::micros(soft))), &usd(r.listed)), if r.capped { Tone::Warn } else { Tone::Dim }),
            None => Cell::new(show.money_of(self.share_words(&shares, rows).as_deref(), planned, &usd(spent), None, &usd(listed)), Tone::Dim),
        }
    }

    /// The share words of the cost: a share made from an assumed worth of the window says `est.`, and the window's own reading, where the
    /// provider gave one for the login the newest task ran as, comes first (`5h 26% · est. 1.8% of 5h`).
    fn share_words(&self, shares: &crate::cost::Shares, rows: &[Row]) -> Option<String> {
        self.front.share_words(shares, rows.last())
    }

    /// The bottom bar: what the whole screen is doing, whichever tab is open. How many tasks work and how many wait for the
    /// operator; the window of a provider that runs low; what today has cost, the plan share first as `cost.show` words it; the shape switch;
    /// the project. The chat's own state (context, this thread's cost, the mode) is the composer strip's. The row's tone says whether the
    /// state is news (`accent` while work runs, `warn` while something waits for the operator, plain when idle): the screen shows the state in
    /// the keys row's place only then.
    fn bar(&self) -> PRow {
        let tasks = self.front.tasks();
        let all: Vec<Row> = tasks.book().rows().into_iter().filter(|r| r.spec.doer != crate::task::HUMAN).collect();
        let running = all.iter().filter(|r| matches!(r.state, State::Queued | State::Running | State::Cooling { .. })).count();
        let waiting = all.iter().filter(|r| matches!(r.state, State::Waiting { .. })).count();
        let mut cells = Vec::new();
        if running > 0 {
            cells.push(Cell::new(format!("{running} running"), Tone::Accent));
        }
        if waiting > 0 {
            cells.push(Cell::new(if waiting == 1 { "1 waits for you".to_string() } else { format!("{waiting} wait for you") }, Tone::Warn));
        }
        if running + waiting == 0 {
            cells.push(Cell::new("idle", Tone::Dim));
        }
        // a window past four fifths used that has not reset, bare, without the name of the provider or the account: the fullest of each
        // name; the main chat's windows row says whose each is
        let mut low: Vec<crate::cost::WindowNow> = Vec::new();
        for w in tasks.windows().into_iter().filter(|w| w.used >= WINDOW_LOW) {
            match low.iter_mut().find(|l| l.window == w.window) {
                Some(l) if l.used < w.used => *l = w,
                Some(_) => {}
                None => low.push(w),
            }
        }
        cells.extend(low.iter().map(|w| Cell::new(format!("{} {:.0}%", w.short(), w.used * 100.0), Tone::Warn)));
        if let Some(money) = self.front.today_words() {
            cells.push(Cell::new(format!("today {money}"), Tone::Dim));
        }
        if self.front.desk().is_some_and(|d| d.on()) {
            cells.push(Cell::new("shape", Tone::Accent));
        }
        // the Decider in the words of Agents › Setup's row (one reading for both): `Decider on · Jev`, dim `Decider off` while none answers
        let decider = self.front.decider_now();
        cells.push(if decider.on() { Cell::with_icon("decide", decider.bar(), Tone::Plain) } else { Cell::with_icon("decide-off", decider.bar(), Tone::Dim) });
        cells.push(Cell::new(self.front.cfg().project.clone(), Tone::Dim));
        let tone = if waiting > 0 { Tone::Warn } else if running > 0 { Tone::Accent } else { Tone::Plain };
        PRow { key: "status".into(), cells, tone, ..PRow::default() }
    }

    fn form(&self) -> Vec<PRow> {
        let b = self.front.binding(&self.thread);
        let set = |field: &str| Act {
            key: "enter".into(),
            label: "change".into(),
            intent: Intent::Set { kind: "thread".into(), id: self.thread.clone(), field: field.into(), value: Json::Null },
            risk: 0,
        };
        let row = |key: &str, value: String, change: bool| PRow {
            key: key.into(),
            cells: vec![Cell::new(key, Tone::Dim), Cell::new(value, Tone::Plain)],
            actions: if change { vec![set(key)] } else { Vec::new() },
            ..PRow::default()
        };
        let none = || "(chosen by the first line)".to_string();
        vec![
            row("doer", b.as_ref().map_or_else(none, |b| b.doer.clone()), true),
            row("model", b.as_ref().and_then(|b| b.model.clone()).unwrap_or_else(|| "(the doer's own)".into()), b.is_some()),
            row("how", b.map_or_else(|| "-".into(), |b| b.how), false),
            row("mode", if self.front.cfg().mode == crate::task::Mode::Read { "read" } else { "write" }.into(), false),
        ]
    }

    fn ask_views(&self) -> Vec<AskView> {
        let (book, now) = (self.front.tasks().book(), self.front.log().clock().now_ms());
        // a question of the work an envoy holds is shown in the envoy's chat and nowhere else; Main says where it waits
        let mut out: Vec<AskView> = book
            .open_asks()
            .iter()
            .filter(|o| match self.front.envoy_of_task(&o.asker) {
                Some(chat) if self.here != chat => {
                    self.front.envoy_waits(&o.ask, &chat, &o.question);
                    false
                }
                _ => true,
            })
            .filter_map(|o| askview::view(&self.front, o, now))
            .collect();
        // which chat takes a line, while the orchestrator waits to be told
        out.extend(self.front.route_asks());
        out.extend(self.front.welcome_asks());
        out.extend(self.front.need_asks());
        out.extend(self.front.hand_asks());
        out.extend(self.front.split_asks());
        out
    }

    /// The strip of the composer: the work bar for the tasks of the thread that run, the newest context gauge, the doer the line goes to,
    /// what the thread has cost and the write mode.
    fn strip_value(&self) -> Json {
        // the orchestrator has no strip: the list, its rows and the bar say what it has; with the switch off the one thread is the chat, and
        // Main under `orchestrate` has a doer of its own, which its strip names
        if self.front.chats_on() && !self.front.main_works() && (self.here.is_empty() || self.here == self.front.cfg().thread) {
            return Json::Null;
        }
        let rows = self.thread_rows();
        // the window of the session the line goes on in: the bound doer's own, never a branch's or another doer's
        let window = self.front.binding(&self.thread).and_then(|b| {
            let own = self.front.own_tasks(&self.thread, &b.doer);
            let session = own.iter().find_map(|r| r.session.clone())?;
            own.iter().filter(|r| r.session.as_ref() == Some(&session)).find_map(|r| r.window)
        });
        let route = self.who_now().map(|who| format!("{who}{}", if self.front.binding(&self.thread).is_some_and(|b| b.how == "pinned") { " · pinned (--doer)" } else { "" }));
        // a task that is queued, runs, or is cooling is work in progress (one that waits for the operator is not); it is counted from the moment the
        // line is taken, before the doer has said a word
        let live: Vec<&Row> = rows.iter().filter(|r| matches!(r.state, State::Queued | State::Running | State::Cooling { .. })).collect();
        let work = live
            .iter()
            .map(|r| {
                let label = match r.state {
                    State::Queued => "starting".to_string(),
                    State::Cooling { .. } => "cooling".to_string(),
                    _ => r.spec.role.clone().filter(|w| !w.is_empty()).unwrap_or_else(|| "working".to_string()),
                };
                compose::Work { label }
            })
            .collect();
        // the skills the running tasks were given, counted once each
        let skills = live.iter().flat_map(|r| r.spec.input.get("skills").and_then(|s| s.as_array()).into_iter().flatten().filter_map(|s| s.get("id").and_then(|i| i.as_str()))).collect::<std::collections::BTreeSet<_>>().len();
        let cost = if rows.iter().any(|r| r.spent + r.listed > 0) { self.money(&rows).text } else { String::new() };
        // no doer yet, and the one the first line goes to is known: the launch's `--doer`, else the pick of Setup with its account
        let cfg = self.front.cfg();
        let next = match (&route, &cfg.doer) {
            (Some(_), _) => String::new(),
            (None, Some(d)) => format!("{d}{}", cfg.model.as_deref().map(|m| format!(" · {m}")).unwrap_or_default()),
            (None, None) => self.front.main_pick().map(|d| format!("{d}{}", self.front.picked_account(&d).map(|a| format!(" · {a}")).unwrap_or_default())).unwrap_or_default(),
        };
        compose::strip(&compose::Strip { window, route: route.as_deref().unwrap_or(compose::NO_DOER), next: &next, lanes: self.front.lanes_now().word(), work, cost, mode: self.mode_word(), skills, plan: self.front.plan_counts(&self.thread), form: self.front.gauge_form(), skip: self.front.steps_hint(&self.thread) })
    }

    /// The clock: a question that waits is sent again when its wait changes (the list is sent only when it differs).
    pub fn tick(&mut self) -> Vec<Down> {
        // the envoys' watch runs on the screen's clock and nowhere else
        self.front.watch_step();
        let mut out: Vec<Down> = self.look_down();
        out.extend(self.complete_step());
        // the logins found on this machine, or a program's answer to the pre-flight, came in from a probe apart from this work: the views are sent again
        let landed = self.front.found_landed() + self.front.setup_landed();
        if landed != self.landed {
            self.landed = landed;
            out.extend(self.views_again());
        }
        if self.swept.is_none_or(|t| t.elapsed() >= Duration::from_secs(60)) {
            self.swept = Some(Instant::now());
            self.front.archive_sweep();
        }
        // the Log is read on, and the events written since it was last sent are sent now
        if self.watched.contains(page::LOG) && (self.log_catch() || self.log_dirty) {
            out.extend(self.log_down(false));
        }
        if !self.asks.is_empty() && self.watched.contains(page::ASKS) {
            out.extend(self.snapshot(page::ASKS));
        }
        // the time a chat has worked or waited moves with the clock: the list is sent again each second while one is not over
        if self.front.chats_on() && self.watched.contains(j4n_proto::CHATS) && self.list_at.is_none_or(|t| t.elapsed() >= Duration::from_secs(1)) {
            self.list_at = Some(Instant::now());
            out.extend(self.rows_now(j4n_proto::CHATS, false));
        }
        out
    }
}

/// What a view reads, from the front a client is served by.
struct Src<'a> {
    serve: &'a Serve,
}

impl crate::view::Sources for Src<'_> {
    fn rows(&self, coll: &crate::query::Coll) -> Vec<crate::value::Value> {
        use crate::query::Coll;
        let (front, screen) = (&self.serve.front, self.serve.front.screen());
        match coll {
            Coll::Tasks => self.serve.thread_rows().iter().map(screen::task_value).collect(),
            // the look is what the front holds now, which `:look` changes while the settings are the ones read at the start
            Coll::Table(kind) if kind == "setting" => {
                use crate::value::Value;
                let look = front.look_spoken();
                let mut rows = screen.setting_rows();
                let says = front.switch_says(&self.serve.thread);
                for r in &mut rows {
                    let Value::Map(m) = r else { continue };
                    let Some(Value::Str(id)) = m.get("id").cloned() else { continue };
                    if let Some(w) = id.strip_prefix("theme.").and_then(|p| look.word(p)) {
                        // the marks' `auto` says what it comes to on this machine, and why
                        m.insert("now".into(), Value::text(if id == "theme.marks" { super::fonts::row_word(&w) } else { w }));
                    }
                    // the one switch says what it turns on, the needs and the crew's ready switch
                    if id == "lanes" {
                        m.insert("says".into(), Value::text(&says));
                    }
                    // the edit `u` on the row takes back
                    m.insert("undo".into(), Value::text(front.proposer().and_then(|p| p.last_edit("setting", &id)).unwrap_or_default()));
                }
                rows
            }
            // an account with where it stands (the fold `accounts`), so the row turns ✓ when it is signed in, with its email and the reason
            Coll::Table(kind) if kind == "account" => {
                use crate::value::Value;
                let now = front.log().clock().now_ms();
                let state: BTreeMap<String, Value> = screen.fold_rows("accounts", now).into_iter().filter_map(|r| match r {
                    Value::Map(m) => match m.get("id") {
                        Some(Value::Str(id)) => Some((id.clone(), Value::Map(m))),
                        _ => None,
                    },
                    _ => None,
                }).collect();
                let mut rows = screen.tables.rows(kind);
                for r in &mut rows {
                    let Value::Map(m) = r else { continue };
                    let Some(Value::Str(id)) = m.get("id").cloned() else { continue };
                    let said = |k: &str| match state.get(&id) {
                        Some(Value::Map(s)) => match s.get(k) {
                            Some(Value::Str(w)) => w.clone(),
                            _ => String::new(),
                        },
                        _ => String::new(),
                    };
                    let word = said("state");
                    let word = if word.is_empty() { if matches!(m.get("keeps"), Some(Value::Str(k)) if k == "yours") { "yours".to_string() } else { "needs sign-in".to_string() } } else { word };
                    let tone = match word.as_str() {
                        "signed in" | "yours" => "good",
                        "failed" => "bad",
                        _ => "warn",
                    };
                    m.insert("state".into(), Value::text(&word));
                    m.insert("tone".into(), Value::text(tone));
                    m.insert("email".into(), Value::text(said("email")));
                    // why it stands so, opened under the row: Agents › Accounts is one page
                    m.insert("reason".into(), Value::text(said("reason")));
                }
                rows
            }
            // a plan with its price as the editor of Agents › Plans has it
            Coll::Table(kind) if kind == "plan" => {
                use crate::value::Value;
                let mut rows = screen.tables.rows(kind);
                for r in &mut rows {
                    let Value::Map(m) = r else { continue };
                    let Some(Value::Str(id)) = m.get("id").cloned() else { continue };
                    let price = match m.get("price") {
                        Some(Value::Usd(u)) => Value::Num(*u as f64 / 1e6),
                        _ => Value::Missing,
                    };
                    m.insert("edit".into(), Value::Map(BTreeMap::from([("kind".to_string(), Value::text("usd")), ("bind".into(), Value::text(format!("plan:{id}.price"))), ("value".into(), price)])));
                    m.insert("undo".into(), Value::text(front.proposer().and_then(|p| p.last_edit("plan", &id)).unwrap_or_default()));
                }
                rows
            }
            // a connector that takes a key: where it comes from (`keyed`, never the value), whether J4n keeps it (`kept`, the row's `x`), and the
            // secret editor its `⏎` opens; an answerer is made with no key and is asked once one is set, so nothing waits for a start
            Coll::Table(kind) if kind == "connector" => {
                use crate::value::Value;
                let mut rows = screen.tables.rows(kind);
                for r in &mut rows {
                    let Value::Map(m) = r else { continue };
                    let Some(Value::Str(id)) = m.get("id").cloned() else { continue };
                    let reference = match m.get("key") {
                        Some(Value::Str(k)) => k.clone(),
                        _ => String::new(),
                    };
                    let keyed = front.key_state(&id, &reference);
                    m.insert("kept".into(), Value::Bool(front.key_kept(&id)));
                    if !reference.is_empty() {
                        m.insert("edit".into(), Value::Map(BTreeMap::from([("kind".to_string(), Value::text("secret")), ("bind".into(), Value::text(format!("key:{id}.value")))])));
                    }
                    m.insert("keyed".into(), Value::text(&keyed));
                }
                rows
            }
            Coll::Table(kind) => screen.tables.rows(kind),
            Coll::Fold(id) => match id.as_str() {
                "account-add" => front.account_add_rows(),
                "connect" => front.connect_rows(),
                // the deciders there are; with none, the one row that says why and leads to the key that fixes it
                "deciders" => {
                    use crate::value::Value;
                    let mut rows: Vec<Value> = self.rows(&Coll::Table("connector".into())).into_iter().filter(|r| matches!(r, Value::Map(m) if m.get("is") == Some(&Value::text("decider")))).collect();
                    for r in &mut rows {
                        if let Value::Map(m) = r {
                            m.insert("kind".into(), Value::text("decider"));
                        }
                    }
                    rows.extend(front.decider_gap().filter(|_| rows.is_empty()));
                    rows
                }
                "lab" => front.lab_rows(),
                // the crew: the charters that were signed, and the settings of the chat the screen is in
                "charters" => front.charter_values(),
                "chatset" => front.chatset_values(&self.serve.thread),
                "library" => front.library_rows(),
                "providers" => front.provider_rows(),
                "answerers" => front.asks().state.rows(front.log().clock().now_ms()),
                "packs" => screen.pack_values(),
                "skills" => front.cfg().skills.as_ref().map(|l| l.rows()).unwrap_or_default(),
                // Agents › Setup's steps, then the doctor's lines (`view:setup`)
                "setup" => {
                    let report = front.cfg().setup.as_ref().map(|c| c.report_for(front.main_pick()));
                    let mut rows = front.setup_steps(report.as_ref());
                    rows.extend(report.map(|r| r.values()).unwrap_or_default());
                    rows
                }
                "spend" => screen.spend_rows(front.log().clock().now_ms()),
                "proposals" => front.proposer().map(|p| p.proposals().rows()).unwrap_or_default(),
                // the grants that count, first, then every gate (Settings › Gates): a grant's row says its scope and what it lets through
                "gates" => {
                    use crate::value::Value;
                    let field = |m: &BTreeMap<String, Value>, k: &str| m.get(k).cloned().unwrap_or_default();
                    let grants = front.grant_rows().into_iter().filter_map(|r| match r {
                        Value::Map(m) => Some(Value::Map(BTreeMap::from([
                            ("id".to_string(), field(&m, "id")),
                            ("on".to_string(), field(&m, "verb")),
                            ("verdict".to_string(), Value::text("allow")),
                            ("why".to_string(), field(&m, "target")),
                            ("grant".to_string(), field(&m, "says")),
                            ("doc".to_string(), Value::text("a grant you gave: what it lets through is asked again once it is taken back")),
                        ]))),
                        _ => None,
                    });
                    grants.chain(screen.tables.rows("gate")).collect()
                }
                other => screen.fold_rows(other, front.log().clock().now_ms()),
            },
            Coll::Log(glob) => {
                let all: Vec<_> = front.log().read(0).filter(|e| crate::eval::glob_match(glob, &e.kind)).collect();
                let from = all.len().saturating_sub(REPLAY);
                // every field of the event, so that a view's `{data.x}` has what the event said
                all[from..].iter().map(Ev::row).collect()
            }
            Coll::Changes => front.proposer().map(|p| p.change_rows()).unwrap_or_default(),
            Coll::Asks | Coll::Thread(_) => Vec::new(),
        }
    }
}

fn task_row(r: &Row, show: Show) -> PRow {
    let tone = tone_of(&r.state);
    let cells = vec![
        Cell::new(&r.id, Tone::Dim),
        Cell::new(r.state.word(), tone),
        Cell::new(r.who(), Tone::Plain),
        Cell::new(clip(&r.spec.goal, 60), Tone::Plain),
        Cell::new(if r.spent > 0 { show.money_of(r.shares.words().as_deref(), r.planned, &usd(r.spent), None, &usd(r.listed)) } else { String::new() }, Tone::Dim),
    ];
    let detail = [r.reason.as_deref(), r.answer.as_deref(), Some(r.plan_note.as_str()).filter(|n| !n.is_empty())].into_iter().flatten().map(|t| Cell::new(clip(t, 200), Tone::Dim)).collect();
    let actions = if r.state.is_terminal() {
        Vec::new()
    } else {
        vec![Act { key: "a".into(), label: "abort".into(), intent: Intent::Control { task: r.id.clone(), control: Control::Abort }, risk: 10 }]
    };
    PRow { key: r.id.clone(), cells, tone, detail, actions, parent: None, ..PRow::default() }
}

enum Msg {
    Up(Up),
    /// What came of the line a thread was taking.
    Said(Said),
    Ev(std::sync::Arc<Ev>),
    /// Events were lost on the way (the client was too slow): everything that is watched is sent again.
    Lost,
    Tick,
    Closed,
}

/// How many events wait for a client before the oldest are given up.
pub const BACKLOG: usize = 8192;

impl Front {
    /// A client on threads of its own: it sends `Up` and reads `Down`. Both ends close when the client says `Quit` or drops
    /// its sender, or when the front is told to quit (`:quit`).
    pub fn connect(&self) -> (Sender<Up>, Receiver<Down>) {
        self.connect_with(BACKLOG)
    }

    /// [`connect`](Front::connect) with room for `backlog` events: past it the oldest wait no longer, and the client is sent
    /// what it watches again, from the Log.
    pub fn connect_with(&self, backlog: usize) -> (Sender<Up>, Receiver<Down>) {
        let (up_tx, up_rx) = mpsc::channel::<Up>();
        let (down_tx, down_rx) = mpsc::channel::<Down>();
        let (msg_tx, msg_rx) = mpsc::channel::<Msg>();
        let sub = self.log().subscribe("*", backlog);
        let (tx, tx_said) = (msg_tx.clone(), msg_tx.clone());
        // a tick waits to be taken: no other is sent, so a slow step of the serve thread never leaves a pile of ticks behind it
        let ticking = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let tick_sent = ticking.clone();
        std::thread::spawn(move || {
            for up in up_rx {
                if tx.send(Msg::Up(up)).is_err() {
                    return;
                }
            }
            let _ = tx.send(Msg::Closed);
        });
        std::thread::spawn(move || {
            let mut lost = 0;
            loop {
                if sub.lagged() != lost {
                    lost = sub.lagged();
                    if msg_tx.send(Msg::Lost).is_err() {
                        return;
                    }
                }
                let sent = match sub.recv_timeout(Duration::from_millis(100)) {
                    Some(ev) => msg_tx.send(Msg::Ev(ev)),
                    None if sub.closed() => return,
                    None if tick_sent.swap(true, std::sync::atomic::Ordering::SeqCst) => Ok(()),
                    None => msg_tx.send(Msg::Tick),
                };
                if sent.is_err() {
                    return;
                }
            }
        });
        let mut serve = Serve::new(self.clone());
        let said_tx = tx_said;
        std::thread::spawn(move || {
            /// The serve thread ends however it ends: the next tick is sent, and the Log's reader finds the channel closed and ends too.
            struct Untick(std::sync::Arc<std::sync::atomic::AtomicBool>);
            impl Drop for Untick {
                fn drop(&mut self) {
                    self.0.store(false, std::sync::atomic::Ordering::SeqCst);
                }
            }
            let _untick = Untick(ticking.clone());
            // what the screen sent while a line was being taken waits, in order, until it has been: the events (the row of the line first of them) go on
            let (mut held, mut taking): (std::collections::VecDeque<Up>, bool) = (Default::default(), false);
            for msg in msg_rx {
                let mut out = Vec::new();
                match msg {
                    // the key of a pick row is what the line being taken waits for: it does not queue behind it
                    Msg::Up(Up::Intent { intent: Intent::Answer { ask, reply } }) if Front::is_hand_row(&ask) => out.push(serve.up(Up::Intent { intent: Intent::Answer { ask, reply } })),
                    Msg::Up(up) => held.push_back(up),
                    Msg::Said(said) => {
                        taking = false;
                        out.push(serve.after(said));
                    }
                    Msg::Ev(ev) => out.push((serve.ev(&ev), false)),
                    Msg::Lost => out.push((serve.resync(), false)),
                    Msg::Tick => {
                        ticking.store(false, std::sync::atomic::Ordering::SeqCst);
                        out.push((serve.tick(), false));
                    }
                    Msg::Closed => return,
                }
                while !taking && let Some(up) = held.pop_front() {
                    match serve.say_job(&up) {
                        Some(job) => {
                            taking = true;
                            let tx = said_tx.clone();
                            std::thread::spawn(move || {
                                let said = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).unwrap_or_else(|_| Said::Refused("the core failed on that line".into()));
                                let _ = tx.send(Msg::Said(said));
                            });
                        }
                        None => out.push(serve.up(up)),
                    }
                }
                for (downs, quit) in out {
                    for d in downs {
                        if down_tx.send(d).is_err() {
                            return;
                        }
                    }
                    if quit {
                        return;
                    }
                }
            }
        });
        (up_tx, down_rx)
    }
}
