//! The binding of a thread: which doer, model and host a thread elected, once. A fold of the events
//! `thread.bound` and `thread.fresh`, so a restart finds every thread as it was. It also keeps which machine answer made the
//! binding, and how many lines the operator has typed since, so the rules can label that answer from what followed.

use crate::fold::Fold;
use crate::log::Ev;
use crate::value::{Key, Value};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// What a thread is bound to, and how it came to be: `pinned` (the operator named it), `elected` (the standing numbers
/// chose it), `switch` (`:switch`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub doer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The host that serves the model: part of the binding, changed only by a switch or a move.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// The account the doer runs as: named by the operator, `:switch claude account=work`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    pub how: String,
    /// The machine answer that made it, if one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<AnswerRef>,
    /// Lines the operator typed since it was made.
    #[serde(default)]
    pub lines: u32,
    /// Tasks a doer began for the thread since it was made (a question put to someone is not one).
    #[serde(default)]
    pub tasks: u32,
}

/// A machine answer, as the rules that label it need to name it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerRef {
    pub ask: String,
    pub answerer: String,
    #[serde(default)]
    pub model: String,
    pub point: String,
}

/// A chat of the screen: `c3`, and a plan's branch under it `c3.1`. A chat is a thread of its own, named by its id, with
/// its own doer session; a branch chat has no thread, it is the view of one branch task of its parent's. Folded from `chat.opened`, so a restart finds
/// them as they were.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatRow {
    pub id: String,
    /// The chat a branch is under; none for a top-level chat.
    pub parent: Option<String>,
    /// The first line of the chat, cut: what the list and the crumb say of it.
    pub title: String,
    /// For a branch chat: the branch task it shows, and the plan item it was started for.
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub item: String,
    pub opened_ms: u64,
    /// When the chat last had a line typed in it or a task begin or end.
    #[serde(default)]
    pub touched_ms: u64,
    /// The lines typed in it, newest last, cut: what a line is matched against (`front::chats`).
    #[serde(default)]
    pub lines: Vec<String>,
    /// When the chat was archived (`chat.archived`); none while it is in the list. A line typed in it, or a turn begun, takes it out again.
    #[serde(default)]
    pub archived_ms: Option<u64>,
    /// `chat.removed`: the chat is gone from every list. The row stays in the fold so that its id is not given again.
    #[serde(default)]
    pub removed: bool,
    /// The seat the chat sits in (`chat.seated`): what the list says before its doer.
    #[serde(default)]
    pub seat: Option<String>,
}

/// A message between chats, folded from `chat.message` and `chat.message.delivered`: who sent it, who it is for, its words
/// (redacted, cut), and where it is: `queued` (it waits for a settled boundary of the receiver), `held` (over a floor: it waits for your word) or
/// `delivered` (a turn's brief carried it). A message is data and is read by no ask, grant or Gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailRow {
    /// `m3`.
    pub id: String,
    /// `c3` (a chat's doer), `J4n` (the core itself).
    pub from: String,
    pub to: String,
    pub text: String,
    /// `queued`, `held` or `delivered`.
    pub state: String,
    pub at_ms: u64,
    /// The messages that were in the brief of the turn that sent this one, the newest of them.
    #[serde(default)]
    pub cause: Option<String>,
    /// How long the cause chain is: 1 for a message nothing caused.
    #[serde(default)]
    pub hops: u32,
    /// What main decided of a wake: none while it has not, else whether the receiver wakes with it.
    #[serde(default)]
    pub wake: Option<bool>,
    /// The turn whose brief carried it.
    #[serde(default)]
    pub turn: Option<String>,
    /// Whether it was cut to the floor.
    #[serde(default)]
    pub cut: bool,
    /// Why it was held, in words (empty when it was not): what the question `hold their messages?` is asked again with.
    #[serde(default)]
    pub why: String,
    /// What kind of note it is: `merge` (a lane was merged into the chat; it informs and wakes nobody), else empty.
    #[serde(default)]
    pub kind: String,
}

/// How many lines of a chat are kept to match against, and how long each may be.
pub const CHAT_LINES: usize = 8;
pub const CHAT_LINE: usize = 200;

/// How many of the operator's lines after a machine's pick a change of mind still counts against it.
pub const OVERRIDE_LINES: u32 = 2;

impl Binding {
    /// The machine answer this binding followed, if the operator is still within [`OVERRIDE_LINES`] of it.
    pub fn recent_answer(&self) -> Option<&AnswerRef> {
        self.answer.as_ref().filter(|_| self.lines <= OVERRIDE_LINES)
    }
}

/// The fold `threads`. A clone is a second handle on the same state.
#[derive(Clone, Default)]
pub struct Binds(Arc<Mutex<Held>>);

/// What the fold holds: the bindings, and for each thread the newest task it queued and the one that was newest when `:fresh` was said
/// (a doer's session of a task up to that one is over).
#[derive(Default, Serialize, Deserialize)]
struct Held {
    binds: BTreeMap<String, Binding>,
    last: BTreeMap<String, String>,
    cut: BTreeMap<String, String>,
    #[serde(default)]
    chats: BTreeMap<String, ChatRow>,
    /// The messages between chats, by id.
    #[serde(default)]
    mail: BTreeMap<String, MailRow>,
    /// What the orchestrator has been told once of a chat: `end:<task>`, `check:<task>`, `plan:<chat>:<done>/<total>`, from `chat.ended`, `chat.checked` and `chat.plan`.
    #[serde(default)]
    said: std::collections::BTreeSet<String>,
    /// The question of which chat takes a line that nothing has closed, by thread: its ask and line. A restart finds it here, and not in memory.
    #[serde(default)]
    route: BTreeMap<String, (String, String)>,
    /// The settings of each chat of its own (`crew.on`), from `chat.set`; a chat that is archived or removed has none.
    #[serde(default)]
    layer: crate::crew::Layer,
    /// The operator's own lines of a split, by thread, from `crew.verbatim` (the latest eight): what the critic reads a plan's change against.
    #[serde(default)]
    verbatim: BTreeMap<String, Vec<String>>,
}

impl Binds {
    pub fn new() -> Binds {
        Binds::default()
    }

    /// The layer of the chats as it stands (a copy: a write is planned on it and then written to the Log, which this fold reads).
    pub fn layer(&self) -> crate::crew::Layer {
        self.lock().layer.clone()
    }

    /// The operator's lines kept for the plan of `thread` (oldest first).
    pub fn verbatim(&self, thread: &str) -> Vec<String> {
        self.lock().verbatim.get(thread).cloned().unwrap_or_default()
    }

    /// What `chat` holds of its own, `(setting, value)`.
    pub fn chat_settings(&self, chat: &str) -> Vec<(String, toml::Value)> {
        self.lock().layer.of(chat)
    }

    fn lock(&self) -> MutexGuard<'_, Held> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get(&self, thread: &str) -> Option<Binding> {
        self.lock().binds.get(thread).cloned()
    }

    /// The newest task of `thread` that `:fresh` ended the session of: a task up to it, in the order they were queued, is not one a line goes on from.
    pub fn cut(&self, thread: &str) -> Option<String> {
        self.lock().cut.get(thread).cloned()
    }

    /// Every chat, top-level ones in the order they were opened and each branch after its chat.
    pub fn chats(&self) -> Vec<ChatRow> {
        let held = self.lock();
        let mut all: Vec<ChatRow> = held.chats.values().filter(|c| !c.removed).cloned().collect();
        all.sort_by_key(|c| chat_order(&c.id));
        all
    }

    pub fn chat(&self, id: &str) -> Option<ChatRow> {
        self.lock().chats.get(id).filter(|c| !c.removed).cloned()
    }

    /// The questions of which chat takes a line that the Log opened and nothing closed: thread, ask and line.
    pub fn route_open(&self) -> Vec<(String, String, String)> {
        self.lock().route.iter().map(|(t, (a, l))| (t.clone(), a.clone(), l.clone())).collect()
    }

    /// Whether the orchestrator was told `key` already (`end:<task>`, `plan:<chat>:<done>/<total>`).
    pub fn said(&self, key: &str) -> bool {
        self.lock().said.contains(key)
    }

    /// Every message, in the order they were sent.
    pub fn mail(&self) -> Vec<MailRow> {
        let mut all: Vec<MailRow> = self.lock().mail.values().cloned().collect();
        all.sort_by_key(|m| mail_order(&m.id));
        all
    }

    pub fn message(&self, id: &str) -> Option<MailRow> {
        self.lock().mail.get(id).cloned()
    }

    /// The id the next message gets: `m<n>`, one past the newest.
    pub fn next_mail(&self) -> String {
        format!("m{}", self.lock().mail.keys().map(|k| mail_order(k)).max().unwrap_or(0) + 1)
    }

    /// The id the next chat gets: `c<n>` one past the newest, or `<parent>.<m>` one past the newest branch of `parent`.
    pub fn next_chat(&self, parent: Option<&str>) -> String {
        let held = self.lock();
        match parent {
            None => format!("c{}", held.chats.values().filter(|c| c.parent.is_none()).filter_map(|c| c.id.strip_prefix('c')?.parse::<u64>().ok()).max().unwrap_or(0) + 1),
            Some(p) => format!("{p}.{}", held.chats.values().filter(|c| c.parent.as_deref() == Some(p)).filter_map(|c| c.id.rsplit('.').next()?.parse::<u64>().ok()).max().unwrap_or(0) + 1),
        }
    }
}

/// The number of a message id (`m12`).
fn mail_order(id: &str) -> u64 {
    id.trim_start_matches('m').parse().unwrap_or(0)
}

/// `c3` before `c3.1` before `c10`: the numbers of an id, in order.
fn chat_order(id: &str) -> Vec<u64> {
    id.trim_start_matches('c').split('.').map(|n| n.parse().unwrap_or(0)).collect()
}

/// A message's steps: `sent` (and `held`) make it, `released` and `decided` change it, `chat.message.delivered` ends it.
fn apply_mail(mail: &mut BTreeMap<String, MailRow>, ev: &Ev) {
    let text = |k: &str| ev.data.get(k).and_then(Json::as_str).unwrap_or_default().to_string();
    let id = text("id");
    if id.is_empty() {
        return;
    }
    if ev.kind == "chat.message.delivered" {
        if let Some(m) = mail.get_mut(&id) {
            m.state = "delivered".into();
            m.turn = Some(text("turn")).filter(|t| !t.is_empty());
        }
        return;
    }
    match text("step").as_str() {
        "released" => {
            if let Some(m) = mail.get_mut(&id) {
                m.state = "queued".into();
            }
        }
        "decided" => {
            if let Some(m) = mail.get_mut(&id) {
                m.wake = ev.data.get("wake").and_then(Json::as_bool);
            }
        }
        _ => {
            let n = |k: &str| ev.data.get(k).and_then(Json::as_u64).unwrap_or(0);
            mail.insert(
                id.clone(),
                MailRow {
                    id,
                    from: text("from"),
                    to: text("to"),
                    text: text("text"),
                    state: text("state"),
                    at_ms: ev.at_ms,
                    cause: Some(text("cause")).filter(|c| !c.is_empty()),
                    hops: n("hops") as u32,
                    wake: ev.data.get("wake").and_then(Json::as_bool),
                    turn: None,
                    cut: ev.data.get("cut").and_then(Json::as_bool).unwrap_or(false),
                    why: text("why"),
                    kind: text("kind"),
                },
            );
        }
    }
}

impl Fold for Binds {
    fn id(&self) -> &str {
        "threads"
    }

    fn version(&self) -> u64 {
        9
    }

    fn wants(&self, kind: &str) -> bool {
        matches!(kind, "thread.bound" | "thread.fresh" | "chat.say" | "task.queued" | "task.started" | "task.settled" | "chat.opened" | "chat.message" | "chat.message.delivered" | "chat.ended" | "chat.checked" | "chat.plan" | "chat.route.asked" | "chat.route.dropped" | "chat.routed" | "chat.archived" | "chat.unarchived" | "chat.removed" | "chat.set" | "chat.seated" | "crew.verbatim")
    }

    fn apply(&mut self, ev: &Ev) {
        let Some(thread) = ev.thread.clone() else { return };
        let mut held = self.lock();
        if matches!(ev.kind.as_str(), "chat.ended" | "chat.checked" | "chat.plan") {
            if let Some(key) = ev.data.get("key").and_then(Json::as_str) {
                held.said.insert(key.to_string());
            }
            return;
        }
        if ev.kind == "crew.verbatim" {
            if let Some(line) = ev.data.get("line").and_then(Json::as_str) {
                let kept = held.verbatim.entry(thread).or_default();
                kept.push(line.to_string());
                let over = kept.len().saturating_sub(8);
                kept.drain(..over);
            }
            return;
        }
        if ev.kind == "chat.set" {
            held.layer.take(&ev.kind, &thread, &ev.data);
            return;
        }
        if ev.kind == "chat.seated" {
            if let Some(chat) = held.chats.get_mut(&thread) {
                chat.seat = ev.data.get("seat").and_then(Json::as_str).filter(|s| !s.is_empty()).map(String::from);
            }
            return;
        }
        if ev.kind == "chat.message" || ev.kind == "chat.message.delivered" {
            apply_mail(&mut held.mail, ev);
            return;
        }
        if matches!(ev.kind.as_str(), "chat.route.asked" | "chat.route.dropped" | "chat.routed") {
            let text = |k: &str| ev.data.get(k).and_then(Json::as_str).unwrap_or_default().to_string();
            if ev.kind == "chat.route.asked" {
                held.route.insert(thread, (text("ask"), text("line")));
            } else {
                held.route.remove(&thread);
            }
            return;
        }
        // the archive is a fold of these three and nothing else; they do not touch the chat, so the time it was idle stands
        if matches!(ev.kind.as_str(), "chat.archived" | "chat.unarchived" | "chat.removed") {
            held.layer.take(&ev.kind, &thread, &ev.data);
            if let Some(chat) = held.chats.get_mut(&thread) {
                match ev.kind.as_str() {
                    "chat.archived" => chat.archived_ms = Some(ev.at_ms),
                    "chat.unarchived" => (chat.archived_ms, chat.touched_ms) = (None, ev.at_ms),
                    _ => chat.removed = true,
                }
            }
            return;
        }
        if ev.kind == "chat.opened" {
            let text = |k: &str| ev.data.get(k).and_then(Json::as_str).unwrap_or_default().to_string();
            let parent = Some(text("parent")).filter(|p| !p.is_empty());
            held.chats.insert(thread.clone(), ChatRow { id: thread, parent, title: text("title"), task: Some(text("task")).filter(|t| !t.is_empty()), item: text("item"), opened_ms: ev.at_ms, touched_ms: ev.at_ms, lines: Vec::new(), archived_ms: None, removed: false, seat: None });
            return;
        }
        // a chat is touched by its lines and its tasks; what it was said is kept to match the next line against (`front::chats`)
        if let Some(chat) = held.chats.get_mut(&thread) {
            chat.touched_ms = chat.touched_ms.max(ev.at_ms);
            // a line typed in an archived chat, or a turn begun in it, brings it back
            if matches!(ev.kind.as_str(), "chat.say" | "task.queued") {
                chat.archived_ms = None;
            }
            if ev.kind == "chat.say" && let Some(line) = ev.data.get("text").and_then(Json::as_str) {
                // a chat opened with `:new` is named by the first line typed in it
                if chat.title.is_empty() {
                    chat.title = crate::text::line(line.trim(), 80);
                }
                chat.lines.push(crate::text::clip(line, CHAT_LINE));
                let over = chat.lines.len().saturating_sub(CHAT_LINES);
                chat.lines.drain(..over);
            }
        }
        if ev.kind == "task.settled" {
            return;
        }
        if ev.kind == "task.queued" {
            if let Some(task) = &ev.task {
                held.last.insert(thread, task.clone());
            }
            return;
        }
        if ev.kind == "thread.fresh" {
            held.binds.remove(&thread);
            if let Some(last) = held.last.get(&thread).cloned() {
                held.cut.insert(thread, last);
            }
            return;
        }
        let binds = &mut held.binds;
        if ev.kind != "thread.bound" {
            if let Some(b) = binds.get_mut(&thread) {
                match ev.kind.as_str() {
                    "chat.say" => b.lines = b.lines.saturating_add(1),
                    // the tasks of the thread's own lines: not a question (the operator's), not a step of a flow, not a flow
                    _ if ev.data.get("doer").and_then(Json::as_str).is_some_and(|d| d != crate::task::HUMAN && !d.starts_with("flow:")) && ev.data.get("parent").is_none_or(Json::is_null) => {
                        b.tasks = b.tasks.saturating_add(1)
                    }
                    _ => {}
                }
            }
            return;
        }
        let word = |k: &str| ev.data.get(k).and_then(Json::as_str).filter(|s| !s.is_empty()).map(str::to_string);
        if let Some(doer) = word("doer") {
            let answer = ev.data.get("answer").and_then(|a| serde_json::from_value(a.clone()).ok());
            binds.insert(thread, Binding { doer, model: word("model"), endpoint: word("endpoint"), account: word("account"), how: word("how").unwrap_or_default(), answer, lines: 0, tasks: 0 });
        }
    }

    fn read(&self, path: &[Key], _now_ms: u64) -> Value {
        let text = Value::text;
        let all: BTreeMap<String, Value> = self
            .lock()
            .binds
            .iter()
            .map(|(t, b)| {
                let mut m = BTreeMap::from([("doer".to_string(), text(&b.doer)), ("how".to_string(), text(&b.how))]);
                if let Some(model) = &b.model {
                    m.insert("model".into(), text(model));
                }
                if let Some(e) = &b.endpoint {
                    m.insert("endpoint".into(), text(e));
                }
                if let Some(a) = &b.account {
                    m.insert("account".into(), text(a));
                }
                m.insert("lines".into(), Value::num(f64::from(b.lines)));
                m.insert("tasks".into(), Value::num(f64::from(b.tasks)));
                if let Some(a) = &b.answer {
                    let fields = [("ask", &a.ask), ("answerer", &a.answerer), ("model", &a.model), ("point", &a.point)];
                    m.insert("picked".into(), Value::Map(fields.into_iter().map(|(k, v)| (k.to_string(), text(v))).collect()));
                }
                (t.clone(), Value::Map(m))
            })
            .collect();
        Value::Map(all).at(path)
    }

    fn save(&self) -> Json {
        serde_json::to_value(&*self.lock()).unwrap_or(Json::Null)
    }

    fn load(&mut self, state: Json) -> Result<(), String> {
        *self.lock() = serde_json::from_value(state).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn reset(&mut self) {
        *self.lock() = Held::default();
    }
}
