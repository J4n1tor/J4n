//! The chat as the screen draws it: the events of one thread folded into rows that hang from each
//! other, turn › lane › step. A turn is a line the operator said; a lane is a task that worked for it (a letter, its doer, its model, its
//! place, its time and its money); a step is what the task did (a read, a search, an edit, a command with its output, a question, its
//! words). The rows say what they are by where they hang (`parent`) and by the mark of their first cell, and leave to the screen what it
//! decides itself: pipes, folding, the stats at the right edge. One fold serves the live path and the replay after a restart, so the chat of
//! a thread that is read back from the Log is the chat that was on the screen (the words still streaming are the one difference: the
//! settled message stands where they were).

use super::chat::{clip, settled_line};
use crate::cost::Show;
use crate::log::Ev;
use crate::task::HUMAN;
use j4n_proto::{By, Cell, Fold, Role, Row, Tone};
use serde_json::Value as Json;
use std::collections::HashMap;

/// The longest line of output kept under a command.
const OUT_LINES: usize = 200;
/// The longest line of a step or of its output.
const LINE_MAX: usize = 200;
/// The most rows a client is sent; whole turns older than this are left out.
pub const SENT: usize = 500;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Queued,
    Working,
    Waiting,
    Cooling,
    Done(bool),
    Stopped,
    Failed,
}

impl Phase {
    fn icon(self) -> &'static str {
        match self {
            Phase::Queued => "queued",
            Phase::Working => "working",
            Phase::Waiting => "ask",
            Phase::Cooling => "cooling",
            Phase::Done(true) => "done",
            Phase::Done(false) | Phase::Failed => "failed",
            Phase::Stopped => "stopped",
        }
    }
}

/// What a working lane is doing now, as its last live event says (live only: a replay has the written ones). The live row says it.
#[derive(Default, Clone, PartialEq)]
enum Doing {
    #[default]
    Idle,
    Think,
    Say,
    /// A call that is being written (`size` bytes of its arguments so far), or one that runs: its mark, and the words of what it is about.
    Call { icon: &'static str, text: String, size: u64 },
}

/// A task shown as a lane.
struct Lane {
    row: usize,
    letter: char,
    doer: String,
    /// What the task is for (`work`, `review`, …), when it says: a lane that runs on the thread's own doer says this in place of the doer.
    step: String,
    /// What was asked for (the pin), and what served: the lane says the second once it is known.
    asked: String,
    served: String,
    place: String,
    phase: Phase,
    started: Option<u64>,
    last: u64,
    usd: f64,
    /// What the requests that ran on a plan took of it; `planned` once one did.
    plan: f64,
    planned: bool,
    /// The output tokens of the calls that settled, and of the one going on (live only).
    out_done: u64,
    out_live: u64,
    /// The doer runs code and no model (the log said so when the task was queued): no model is named for it.
    code: bool,
    /// Why the task ended well without doing its job (`task.settled` says `not_run`): a check that could not run. Empty when it did.
    not_run: String,
    /// What it is doing now, since when, and whether it has said anything yet (the live row stands from its first word); the last thing it said.
    doing: Doing,
    since: u64,
    live: bool,
    word: String,
    /// The time of the line the task works for; said on its head once the group view has moved it under a newer line.
    of: String,
}

/// A message between chats as the fold of a chat holds it.
struct Mail {
    row: Option<usize>,
    from: String,
    to: String,
    text: String,
    state: String,
    /// What main made of waking the receiver: `woken`, `left asleep`, or empty.
    woke: String,
    /// ` · cut to 2000 of 3100 characters`, or empty.
    cut: String,
    /// When the receiver's row was made, for its age; 0 for a row that says a state and no time.
    at_ms: u64,
}

/// The fold of one thread.
#[derive(Default)]
pub struct Rows {
    rows: Vec<Row>,
    /// The line the operator said last: what the lanes that start now hang from.
    turn: Option<usize>,
    lanes: HashMap<String, Lane>,
    /// Which of the plan cost and the list cost a lane says (`cost.show`).
    pub show: Show,
    /// The doer and the model pin the thread is bound to, as of the last `thread.bound`: the composer's strip says them, so a lane row does not.
    bound: Option<(String, String)>,
    /// The lanes of each turn so far, for their letters.
    lettered: HashMap<usize, u8>,
    /// The turn is one the thread made for what came before any line of the operator's: the first line takes its place.
    implicit: bool,
    /// The tasks whose turn is no row of this thread: the quick look and Main's planning turn say what they came to in a note of their own, never as the JSON they answered.
    hidden: std::collections::HashSet<String>,
    /// The step that holds the words a task is still streaming.
    open: HashMap<String, usize>,
    /// The calls of a task that have no result yet, as `(name, row)`.
    calls: HashMap<String, Vec<(String, usize)>>,
    /// The row of the doer's own list, by the task: one for a lane, said again in place.
    lists: HashMap<String, usize>,
    /// The row of each chat's plan in the orchestrator, by the chat.
    plans: HashMap<String, usize>,
    /// The rows of the questions put to the operator, by the task that is the question.
    asks: HashMap<String, usize>,
    /// The row of each chat's end in the orchestrator, by the task that ended: the check that ran after it puts its word there.
    ends: HashMap<String, usize>,
    made: u64,
    /// The messages between chats this chat has seen, by id: the row each has, and its words.
    mails: HashMap<String, Mail>,
    /// The event the last key was made for, and how many rows it has made.
    keyed: (u64, u32),
    /// The last written event folded: one that comes again (a resync met the queue) is not folded twice.
    seen: u64,
    /// The zone the time of a turn is said in, seconds east of UTC; `None` is the machine's (a test pins it, never the process zone).
    pub zone: Option<i64>,
    /// Orchestrate's group view: the word the thread's own lane is headed with (`Main`, or the chat's id), and every lane is headed by its role
    /// and model so the screen says who printed what. `None`: the plain view.
    pub group: Option<String>,
    /// The row that says the line is being read, which the read's head takes the place of (the group view), and whether the head was made.
    reading: Option<usize>,
    read_said: bool,
    /// The approvals put to the operator, by the question's task: the lane that asked and what the approval is for (the command, the path). In the
    /// group view the answer of one is a word on the asker's step.
    approvals: HashMap<String, (String, String)>,
    /// The rows the group view leaves out, by key: an approval answered, whose word stands on its step.
    dropped: std::collections::HashSet<String>,
    /// An approval's word that waits, by its lane, for the command it is about: a doer that names its call by a description reports the command apart.
    held: HashMap<String, (String, &'static str, usize)>,
    /// The group view: the lanes that still worked when a newer line was typed, by key, and the line they now hang from.
    moved: HashMap<String, String>,
}

/// `12:29`, the time of day on the operator's clock: `zone` is the offset east of UTC in seconds, or the machine's zone at that instant.
pub(super) fn time_of_day(ms: u64, zone: Option<i64>) -> String {
    let at = (ms / 1000) as i64;
    let m = (at + zone.unwrap_or_else(|| crate::clock::offset_at(at))).rem_euclid(86_400) / 60;
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// What a row says of the time it ends in: `data.at_ms`, the event's time. The row's last cell stays the time of day for a reader that has no
/// clock (a replay, a pipe); the screen puts its own word for how long ago it was in place of it (`look.time`).
fn stamp(at_ms: u64) -> Json {
    serde_json::json!({ "at_ms": at_ms })
}

/// `data` with `at_ms` in it, whatever else it holds.
fn stamped(data: Json, at_ms: u64) -> Json {
    let mut data = data;
    data["at_ms"] = Json::from(at_ms);
    data
}

/// `21s`, `1:04`, `1:02:09`.
pub(super) fn clock(ms: u64) -> String {
    let s = ms / 1000;
    match s {
        0..=59 => format!("{s}s"),
        60..=3599 => format!("{}:{:02}", s / 60, s % 60),
        _ => format!("{}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60),
    }
}

/// `4.2 kB`, `812 B`.
fn size_of(bytes: u64) -> String {
    if bytes < 1000 { format!("{bytes} B") } else { format!("{:.1} kB", bytes as f64 / 1000.0) }
}

/// `812`, `1.2k`.
fn count_of(n: u64) -> String {
    if n < 1000 { n.to_string() } else { format!("{:.1}k", n as f64 / 1000.0) }
}

/// The icon and the words of a call, from its name and what it is about (the same for the open step and the real one).
fn open_text(name: &str, target: &str) -> (&'static str, String) {
    let icon = kind_of(name);
    let text = match (icon, target.is_empty()) {
        ("tool", false) => format!("{name} {target}"),
        (_, true) => name.to_string(),
        _ => target.to_string(),
    };
    (icon, text)
}

/// The mark and the words of a call as the live row says them: what it is about (the file, the command), or the tool's name when it says nothing.
fn call_words(name: &str, target: &str) -> (&'static str, String) {
    let icon = kind_of(name);
    (icon, if icon == "tool" { format!("{name} {target}").trim().to_string() } else { target.to_string() })
}

/// The words of the live row for a call: a verb and what it is about, the tool's own words when it has no verb.
fn verb_of(icon: &str, text: &str) -> String {
    let verb = match icon {
        "new" => "writing",
        "edit" => "editing",
        "read" => "reading",
        "search" => "searching",
        "run" => "running",
        "fetch" => "fetching",
        _ => return if text.is_empty() { "working".into() } else { text.to_string() },
    };
    if text.is_empty() { verb.to_string() } else { format!("{verb} {text}") }
}

pub(super) fn money(usd: f64) -> String {
    if usd < 0.01 { format!("${usd:.3}") } else { format!("${usd:.2}") }
}

fn s<'a>(v: &'a Json, key: &str) -> &'a str {
    v.get(key).and_then(Json::as_str).unwrap_or_default()
}

/// What kind of step a call is, by the name the doer gave its tool: the mark of its row. Every doer has its own names.
pub fn kind_of(name: &str) -> &'static str {
    let n = name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| n == *w || n.starts_with(&format!("{w}_")) || n.contains(w));
    if has(&["multiedit", "edit", "patch", "replace", "notebookedit"]) {
        "edit"
    } else if has(&["write", "create"]) {
        "new"
    } else if has(&["grep", "glob", "search", "find", "ripgrep", "ls"]) {
        "search"
    } else if has(&["read", "cat", "view", "open"]) {
        "read"
    } else if has(&["bash", "shell", "exec", "run", "command"]) {
        "run"
    } else if has(&["web", "fetch", "http", "browse", "url"]) {
        "fetch"
    } else {
        "tool"
    }
}

/// What a call is about, from the detail the doer gave: a path, a pattern, a command, whichever the detail holds. A detail that is a
/// JSON object (cut short or not) gives its first of these; a plain one is the words as they are.
pub fn target_of(detail: &str) -> String {
    const KEYS: [&str; 7] = ["file_path", "notebook_path", "pattern", "command", "url", "query", "path"];
    let d = detail.trim();
    if !d.starts_with('{') {
        return clip(d, LINE_MAX);
    }
    if let Ok(v) = serde_json::from_str::<Json>(d) {
        if let Some(t) = KEYS.iter().find_map(|k| v.get(*k).and_then(Json::as_str)) {
            return clip(t, LINE_MAX);
        }
        return clip(d, LINE_MAX);
    }
    // cut short, so not JSON any more: the first of the keys that is there, read up to its closing quote
    for k in KEYS {
        if let Some(at) = d.find(&format!("\"{k}\":\"")) {
            let rest = &d[at + k.len() + 4..];
            let end = rest.find('"').unwrap_or(rest.len());
            return clip(&rest[..end], LINE_MAX);
        }
    }
    clip(d, LINE_MAX)
}

fn cell(text: impl Into<String>, tone: Tone) -> Cell {
    Cell::new(text, tone)
}

fn row(key: &str, parent: &str, by: By, cells: Vec<Cell>) -> Row {
    Row { key: key.into(), cells, parent: Some(parent.into()), by, ..Row::default() }
}

impl Rows {
    pub fn new() -> Rows {
        Rows::default()
    }

    /// The chat holds a view (`chat.view`): the value of it is the server's to keep up to date.
    pub fn has_view(&self) -> bool {
        self.rows.iter().any(|r| r.data.as_ref().is_some_and(|d| d.get("view").is_some()))
    }

    /// The rows as they stand: the last turns, whole, up to [`SENT`] rows. A turn longer than that is sent with its last rows only,
    /// and a row whose parent was left out is left out with it.
    pub fn rows(&self) -> Vec<Row> {
        let placed = if self.moved.is_empty() { std::borrow::Cow::Borrowed(&self.rows[..]) } else { std::borrow::Cow::Owned(self.placed()) };
        let mut out = self.kept(&placed);
        out.retain(|r| !self.dropped.contains(&r.key));
        let live = self.working(&out);
        out.extend(live);
        out
    }

    /// The rows in the order they are sent: in the group view a lane that moved under a newer line goes there with every row that hangs from it,
    /// after that line's own rows. The fold keeps them where they began, so every index into the rows stays.
    fn placed(&self) -> Vec<Row> {
        // the rows of the moved lanes by the line they go to, in the order they came
        let (mut of, mut taken, mut out) = (HashMap::new(), std::collections::BTreeMap::<String, Vec<Row>>::new(), Vec::with_capacity(self.rows.len()));
        for r in &self.rows {
            let lane = match self.moved.get(&r.key) {
                Some(_) => Some(r.key.clone()),
                None => r.parent.as_ref().and_then(|p| of.get(p)).cloned(),
            };
            match lane {
                Some(l) => {
                    let mut r = r.clone();
                    if let Some(to) = self.moved.get(&r.key) {
                        r.parent = Some(to.clone());
                    }
                    of.insert(r.key.clone(), l.clone());
                    taken.entry(self.moved[&l].clone()).or_default().push(r);
                }
                None => out.push(r.clone()),
            }
        }
        // each line's moved lanes go before the line that follows it, the newest line's at the end
        let mut placed = Vec::with_capacity(self.rows.len());
        let mut turn = String::new();
        for r in out {
            if r.parent.is_none() {
                placed.extend(taken.remove(&turn).unwrap_or_default());
                turn = r.key.clone();
            }
            placed.push(r);
        }
        placed.extend(taken.remove(&turn).unwrap_or_default());
        placed.extend(taken.into_values().flatten());
        placed
    }

    fn kept(&self, rows: &[Row]) -> Vec<Row> {
        if rows.len() <= SENT {
            return rows.to_vec();
        }
        let turns: Vec<usize> = rows.iter().enumerate().filter(|(_, r)| r.parent.is_none()).map(|(i, _)| i).collect();
        let last = turns.last().copied().unwrap_or(0);
        let from = turns.iter().rev().take_while(|t| rows.len() - **t <= SENT).last().copied().unwrap_or(last);
        let kept = &rows[from..];
        if kept.len() <= SENT {
            return kept.to_vec();
        }
        let tail = kept.len() - (SENT - 1);
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut out = Vec::with_capacity(SENT);
        for (i, r) in kept.iter().enumerate() {
            let held = i == 0 || (i >= tail && r.parent.as_deref().is_some_and(|p| seen.contains(p)));
            if held {
                seen.insert(r.key.as_str());
                out.push(r.clone());
            }
        }
        out
    }

    fn push(&mut self, r: Row) -> usize {
        self.rows.push(r);
        self.rows.len() - 1
    }

    /// A turn: the line the operator said (`implicit`: no line, the thread began with something else). A first line takes the place of
    /// an implicit turn, so that a thread that began with a notice does not show an empty turn before it.
    fn new_turn(&mut self, key: String, text: &str, at: u64, implicit: bool) {
        self.reading = None;
        if let (true, Some(t)) = (self.implicit && !implicit, self.turn) {
            let r = &mut self.rows[t];
            r.cells = vec![cell(text, Tone::Plain), cell("", Tone::Dim), cell(time_of_day(at, self.zone), Tone::Dim)];
            r.data = Some(stamp(at));
            self.implicit = false;
            return;
        }
        let r = Row { key, cells: vec![cell(text, Tone::Plain), cell("", Tone::Dim), cell(time_of_day(at, self.zone), Tone::Dim)], data: Some(stamp(at)), ..Row::default() };
        let at = self.push(r);
        self.turn = Some(at);
        self.implicit = implicit;
        // the group view: the roles that still work stand beside the new line's, under it, and say which line they work for
        if self.group.is_some() && !implicit {
            let turn = self.rows[at].key.clone();
            let live: Vec<String> = self.lanes.iter().filter(|(_, l)| matches!(l.phase, Phase::Queued | Phase::Working | Phase::Waiting | Phase::Cooling)).map(|(id, _)| id.clone()).collect();
            for id in live {
                self.moved.insert(self.rows[self.lanes[&id].row].key.clone(), turn.clone());
                self.redraw(&id);
            }
        }
    }

    /// The key of the turn the thread is in, made if the first thing the thread did was not a line of the operator's.
    fn turn_key(&mut self, ev: &Ev) -> String {
        if self.turn.is_none() {
            let key = self.key(ev);
            self.new_turn(key, "", ev.at_ms, true);
        }
        self.rows[self.turn.unwrap_or(0)].key.clone()
    }

    /// Where a step of `task` hangs: its lane, else the turn.
    fn home(&mut self, ev: &Ev) -> String {
        match ev.task.as_deref().and_then(|t| self.lanes.get(t)) {
            Some(l) => self.rows[l.row].key.clone(),
            None => self.turn_key(ev),
        }
    }

    /// A key no other row has: the event's own, with a number after it for the second row it makes.
    fn key(&mut self, ev: &Ev) -> String {
        if ev.seq == 0 {
            self.made += 1;
            return format!("f{}", self.made);
        }
        self.keyed = if self.keyed.0 == ev.seq { (ev.seq, self.keyed.1 + 1) } else { (ev.seq, 0) };
        match self.keyed.1 {
            0 => format!("s{}", ev.seq),
            n => format!("s{}.{n}", ev.seq),
        }
    }

    fn step(&mut self, ev: &Ev, by: By, cells: Vec<Cell>) -> usize {
        let home = self.home(ev);
        let key = self.key(ev);
        self.push(row(&key, &home, by, cells))
    }

    fn note(&mut self, ev: &Ev, icon: &str, text: String, tone: Tone) -> usize {
        // what a rule says of a task is J4n's own and stays in the task's lane where it was said; only the doer's own notices are
        // lifted after the turn
        let by = if matches!(ev.by, crate::change::Actor::Core | crate::change::Actor::Pack(_) | crate::change::Actor::Rule(_)) { By::Core } else { By::Doer };
        let home = ev.task.as_deref().and_then(|t| self.lanes.get(t)).map(|l| self.rows[l.row].key.clone());
        let mut cells = vec![Cell::with_icon(icon, text, tone), cell("", Tone::Dim)];
        cells[0].tone = tone;
        match home {
            Some(home) => {
                let key = self.key(ev);
                self.push(row(&key, &home, by, cells))
            }
            // without a lane it is the turn's own, and folds with the other things the core said
            None => {
                let t = self.turn_key(ev);
                let key = self.key(ev);
                self.push(row(&key, &t, By::Core, cells))
            }
        }
    }

    /// A row of the orchestrator's own: it is about a chat, not an answer to the line typed last, so it hangs from no turn.
    fn own(&mut self, ev: &Ev, icon: &str, text: String, tone: Tone) -> usize {
        let key = self.key(ev);
        let mut r = row(&key, "", By::Core, vec![Cell::with_icon(icon, text, tone), cell("", Tone::Dim)]);
        r.parent = None;
        self.push(r)
    }

    /// An incident matched (`guard.alert` of kind `incident`): one row, `⚠ like <name> · <what was done>`, its words beside it and
    /// where it is written up under it. The pack's other alert, a changed pin, is said by its own notice (`rules/alert.toml`).
    fn alert(&mut self, ev: &Ev) {
        let d = &ev.data;
        if s(d, "kind") != "incident" {
            return;
        }
        let at = self.note(ev, "risky", format!("like {} · {}", clip(s(d, "like"), crate::rule::LIKE_MAX), s(d, "done")), Tone::Warn);
        self.rows[at].cells[1] = cell(clip(s(d, "text"), LINE_MAX), Tone::Dim);
        self.rows[at].detail = vec![cell(clip(s(d, "source"), LINE_MAX), Tone::Dim)];
    }

    /// The lane a task is, redrawn from what is known of it.
    fn redraw(&mut self, id: &str) {
        let Some(l) = self.lanes.get(id) else { return };
        // the composer's strip names the thread's doer and model: a lane names its doer only when it is another one, and says the
        // step it is for in place of it; a model only when it is not the pin. In the group view the head is the role and a dot before who serves it
        let role = self.group.as_deref().map(|g| self.role_head(l, g));
        let (name, model) = match &self.bound {
            _ if role.is_some() => role.clone().unwrap_or_default(),
            Some((doer, pin)) if *doer == l.doer => (l.step.clone(), if l.asked.is_empty() || l.asked == *pin { String::new() } else { crate::task::model_word(&l.doer, &l.served, &l.asked) }),
            _ => (l.doer.clone(), crate::task::model_word(&l.doer, &l.served, &l.asked)),
        };
        // a doer that runs code names no model, not even that it is not known yet
        let model = if l.code { String::new() } else { model };
        let text = match (&role, name.is_empty()) {
            (Some(_), _) => name,
            (None, true) => l.letter.to_string(),
            (None, false) => format!("{} {name}", l.letter),
        };
        // a task that ended well and did not do its job is no tick: the warn mark and the words `not run`
        let icon = if l.phase == Phase::Done(true) && !l.not_run.is_empty() { "risky" } else { l.phase.icon() };
        let mut cells = vec![Cell::with_icon(icon, text, Tone::Plain), cell(model, Tone::Dim)];
        if self.moved.contains_key(&self.rows[l.row].key) && !l.of.is_empty() {
            cells.push(cell(format!("of {}", l.of), Tone::Dim));
        }
        if icon == "risky" {
            cells.push(cell("not run", Tone::Warn));
        }
        if !matches!(l.place.as_str(), "" | "here" | "none") {
            cells.push(Cell::with_icon("worktree", l.place.clone(), Tone::Dim));
        }
        if let Some(start) = l.started {
            cells.push(cell(clock(l.last.saturating_sub(start)), Tone::Dim));
        }
        if l.phase == Phase::Working && l.out_done + l.out_live > 0 {
            cells.push(cell(format!("{} tok", count_of(l.out_done + l.out_live)), Tone::Dim));
        }
        if l.usd > 0.0 {
            cells.push(cell(self.show.money(l.planned, &money(l.plan), None, &money(l.usd)), Tone::Dim));
        }
        let fold = matches!(l.phase, Phase::Failed | Phase::Done(false)).then_some(Fold::Pinned);
        let at = l.row;
        self.rows[at].cells = cells;
        self.rows[at].fold = fold;
        // for a screen that leaves a lone lane out: whose doer it is, and whether that is the thread's own
        let own = self.bound.as_ref().is_some_and(|(doer, _)| *doer == l.doer);
        let mut lane = serde_json::json!({ "doer": l.doer, "own": own });
        if !l.not_run.is_empty() {
            lane["not_run"] = serde_json::json!(l.not_run);
        }
        // a flow's lane wraps its steps, and a lane of code has no model: a turn of the two is one lane to the screen
        if l.doer.starts_with("flow:") {
            lane["flow"] = serde_json::json!(true);
        } else if l.code {
            lane["code"] = serde_json::json!(true);
        }
        // a lane headed by its role keeps its row even when it is the turn's only one
        if let Some((role, _)) = role {
            // the thread's own work is the lane the group view opens first
            lane["main"] = serde_json::json!(self.group.as_deref() == Some(role.as_str()));
            lane["role"] = serde_json::json!(role);
        }
        self.rows[at].data = Some(serde_json::json!({ "lane": lane }));
    }

    /// The head of a lane in the group view: the role (`group` for the thread's own work), and after a dot the doer when it is not the role's own
    /// name and the model it serves on: `Main · claude sonnet 5.5`, `review · omp glm 5.3`, `check`. A lane of code names no model.
    fn role_head(&self, l: &Lane, group: &str) -> (String, String) {
        let own = self.bound.as_ref().filter(|(doer, _)| *doer == l.doer);
        // the line's own work is the group's, whichever doer serves it, and whether or not the binding is still among the rows (`:clear` cuts it)
        let role = match l.step.as_str() {
            "work" => group.to_string(),
            "" if own.is_some() => group.to_string(),
            "" => l.doer.clone(),
            r => r.to_string(),
        };
        let asked = if l.asked.is_empty() { own.map_or("", |(_, pin)| pin.as_str()) } else { l.asked.as_str() };
        let model = [l.served.as_str(), asked].into_iter().find(|m| !m.is_empty() && !l.code).map(crate::task::model_name).unwrap_or_default();
        let who: Vec<&str> = [if l.doer == role { "" } else { l.doer.as_str() }, model.as_str()].into_iter().filter(|w| !w.is_empty()).collect();
        (role, if who.is_empty() { String::new() } else { format!("· {}", who.join(" ")) })
    }

    fn phase(&mut self, id: &str, phase: Phase) {
        if let Some(l) = self.lanes.get_mut(id) {
            l.phase = phase;
        }
        self.redraw(id);
    }

    /// What one event of the thread changes. An event that is not the thread's is the caller's to leave out.
    pub fn put(&mut self, ev: &Ev) {
        if ev.seq != 0 {
            if ev.seq <= self.seen {
                return;
            }
            self.seen = ev.seq;
        }
        let id = ev.task.clone().unwrap_or_default();
        if !id.is_empty() && self.hidden.contains(&id) {
            return;
        }
        if let Some(l) = self.lanes.get_mut(&id) {
            l.last = l.last.max(ev.at_ms);
        }
        let d = &ev.data;
        match ev.kind.as_str() {
            // a chat Main started: its brief is the head of its first turn, which the thread began with no line of the operator's
            "chat.briefed" => {
                if self.implicit
                    && let Some(t) = self.turn
                    && self.rows[t].cells[0].text.is_empty()
                {
                    self.rows[t].cells[0].text = clip(s(d, "goal"), LINE_MAX);
                }
            }
            "chat.say" => {
                let key = self.key(ev);
                self.new_turn(key, s(d, "text"), ev.at_ms, false);
            }
            "thread.bound" => {
                self.bound = Some((s(d, "doer").to_string(), s(d, "model").to_string()));
                self.noted(ev);
            }
            "task.queued" => self.queued(ev),
            "task.started" => {
                if let Some(l) = self.lanes.get_mut(&id) {
                    l.started.get_or_insert(ev.at_ms);
                    l.last = l.last.max(ev.at_ms);
                    if let Some(p) = d.get("place").and_then(Json::as_str) {
                        l.place = p.into();
                    }
                }
                self.phase(&id, Phase::Working);
            }
            "task.waiting" => self.phase(&id, Phase::Waiting),
            "task.resumed" => self.phase(&id, Phase::Working),
            "task.cooling" => {
                self.phase(&id, Phase::Cooling);
                self.noted(ev);
            }
            "task.settled" => self.settled(ev),
            "task.stopped" => {
                let failed = matches!(d.pointer("/why/kind").and_then(Json::as_str), Some("failed" | "hardcap"));
                self.phase(&id, if failed { Phase::Failed } else { Phase::Stopped });
                self.noted(ev);
            }
            "run.metrics" => {
                if let Some(l) = self.lanes.get_mut(&id) {
                    let list = d.get("usd").and_then(Json::as_f64).unwrap_or(0.0);
                    l.usd += list;
                    l.plan += d.get("plan_usd").and_then(Json::as_f64).unwrap_or(list);
                    l.planned |= d.get("plan_usd").is_some();
                    l.out_done += d.get("tokens_out").and_then(Json::as_u64).unwrap_or(0);
                    l.out_live = 0;
                }
                self.redraw(&id);
            }
            "run.served" => {
                if let Some(l) = self.lanes.get_mut(&id) {
                    l.served = s(d, "model").to_string();
                }
                self.redraw(&id);
            }
            "run.delta" => {
                if !s(d, "text").is_empty() {
                    self.doing(ev, Doing::Say);
                }
                self.delta(ev);
            }
            "run.reasoning_delta" => self.doing(ev, Doing::Think),
            "run.open" => self.opening(ev),
            "run.tokens" => {
                if let Some(l) = self.lanes.get_mut(&id) {
                    l.out_live = d.get("out").and_then(Json::as_u64).unwrap_or(0);
                }
                self.redraw(&id);
            }
            "run.message" => {
                self.message(ev);
                self.idle(ev, clip(s(d, "text").trim(), 40));
            }
            "chat.message" | "chat.message.delivered" => self.mail(ev),
            "chat.ended" | "chat.checked" | "chat.merged" | "chat.plan" => self.orchestrated(ev),
            "orchestrate.looked" if self.group.is_some() => self.looked(ev),
            // what the read came to is said by its head in the group view; the note says it to every other reader
            "chat.note" if self.group.is_some() && s(d, "read") == "read" && std::mem::take(&mut self.read_said) => {}
            "run.reasoning" => {
                let text = s(d, "text").trim();
                if !text.is_empty() {
                    self.step(ev, By::Doer, vec![Cell::with_icon("think", text, Tone::Dim), cell("", Tone::Dim)]);
                }
                self.idle(ev, String::new());
            }
            "run.list" => self.list(ev),
            "run.tool_call" => self.call(ev),
            "run.tool_result" => {
                self.result(ev);
                self.idle(ev, String::new());
            }
            "run.shell" => {
                self.shell(ev);
                self.idle(ev, clip(s(d, "cmd"), 40));
            }
            "run.changed" => self.changed(ev),
            "guard.alert" => self.alert(ev),
            // the answer stands on the row of the question
            "ask.answered" | "ask.note" => {}
            // in the group view the Gate's yes is the word on the step it let run
            "gate.verdict" if self.group.is_some() && s(d, "verdict") == "allow" => {}
            // a view the screen draws and keeps up to date (`/usage`): the row says which, the server puts the value in it
            "chat.view" => {
                let (turn, key) = (self.turn_key(ev), self.key(ev));
                let mut r = row(&key, &turn, By::Core, vec![cell(s(d, "title"), Tone::Dim)]);
                r.data = Some(serde_json::json!({ "view": { "id": s(d, "view") } }));
                self.push(r);
            }
            // a branch chat begins with where it came from; a chat of its own begins with the line you type
            "chat.opened" => {
                if let Some(parent) = d.get("parent").and_then(Json::as_str).filter(|p| !p.is_empty()) {
                    // the turn of a branch chat is where it came from, not a line you typed (the lane may be queued before this is written)
                    let key = self.key(ev);
                    self.new_turn(key, &format!("from {parent} · {}", s(d, "title")), ev.at_ms, false);
                    if let Some(t) = self.turn {
                        self.rows[t].by = By::Core;
                        self.rows[t].data = Some(serde_json::json!({ "from": parent }));
                    }
                }
            }
            "task.steered" => {
                let text = s(d, "text");
                self.note(ev, "notice", format!("steered: {}", clip(text, LINE_MAX)), Tone::Dim);
            }
            _ => self.noted(ev),
        }
    }

    /// What the read of a line came to, in the group view: a head of its own under the line, `read · haiku 5.5 + Decider`, with what the line was
    /// taken as, how long the read took and what it cost at its right, and why under it. It takes the place of the row that said the line was being
    /// read. A read that was skipped or came to nothing has no head: its note says why.
    /// The note the read ends with is left out only once its head is made (`read_said`).
    fn looked(&mut self, ev: &Ev) {
        let d = &ev.data;
        let Some(taken) = d.get("taken").and_then(Json::as_str) else { return };
        // who read it, each once: the quick model by its name, any Decider as the word `Decider`, never its inside name
        let mut by: Vec<String> = Vec::new();
        for b in ["kind", "goes_on", "chat", "asked"].iter().filter_map(|k| d.pointer(&format!("/read/{k}/by")).and_then(Json::as_str)) {
            let word = match b.split_once(':') {
                Some(("model", m)) => crate::task::model_name(m),
                Some(("decider", _)) => "Decider".to_string(),
                _ => b.to_string(),
            };
            if !by.contains(&word) {
                by.push(word);
            }
        }
        let chats = d.get("chats").and_then(Json::as_u64).unwrap_or(0);
        let words = match taken {
            "one" => "one lane".to_string(),
            "split" => format!("split · {chats} chat{}", if chats == 1 { "" } else { "s" }),
            "goes_on" => "goes on".to_string(),
            "asked" => "asks for a doer".to_string(),
            w => w.to_string(),
        };
        let ms = d.get("ms").and_then(Json::as_u64).unwrap_or(0);
        let mut cells = vec![Cell::with_icon("decide", "read", Tone::Plain), cell(if by.is_empty() { String::new() } else { format!("· {}", by.join(" + ")) }, Tone::Dim), cell(words, Tone::Dim), cell(format!("{:.1}s", ms as f64 / 1000.0), Tone::Dim)];
        if let Some(usd) = d.get("usd").and_then(Json::as_f64).filter(|u| *u > 0.0) {
            cells.push(cell(format!("${usd:.4}"), Tone::Dim));
        }
        // a Log written before the notes were marked as the read's has no such row: its notes stand, and no head is made beside them
        let Some(at) = self.reading.take() else { return };
        self.read_said = true;
        let why = s(d, "why");
        let r = &mut self.rows[at];
        (r.cells, r.by) = (cells, By::Core);
        r.detail = if why.is_empty() { Vec::new() } else { vec![cell(clip(why, LINE_MAX), Tone::Dim)] };
    }

    /// What the orchestrator says of its chats: a chat's turn that ended, as a preview of its answer (the row is the door: `⏎` goes in), the word
    /// of the check that ran after it, a lane merged into its chat, and a chat's plan (one row for each chat, said again in place as the plan moves; the screen draws
    /// its thin line). Each is a row of its own, not a child of the line typed last in the thread.
    fn orchestrated(&mut self, ev: &Ev) {
        let d = &ev.data;
        let (chat, at_ms) = (s(d, "chat").to_string(), ev.at_ms);
        let time = cell(time_of_day(at_ms, self.zone), Tone::Dim);
        match ev.kind.as_str() {
            "chat.ended" => {
                let ok = d.get("ok").and_then(Json::as_bool) == Some(true);
                let head = match s(d, "title") {
                    "" => chat.clone(),
                    title => format!("{chat} · {title}"),
                };
                let at = self.own(ev, if ok { "done" } else { "failed" }, head, if ok { Tone::Plain } else { Tone::Bad });
                // the answer, a line to a cell, for the screen to read as markdown; an older event carries only its cut line
                let text = d.get("text").and_then(Json::as_str).unwrap_or_else(|| s(d, "line"));
                self.rows[at].detail = text.lines().map(|l| cell(l.trim_end(), Tone::Plain)).collect();
                self.rows[at].cells.push(time);
                self.rows[at].data = Some(stamped(serde_json::json!({ "door": chat, "read": true }), at_ms));
                self.ends.insert(s(d, "task").to_string(), at);
            }
            "chat.checked" => {
                let by_task = self.ends.get(s(d, "of")).copied();
                let at = by_task.or_else(|| self.rows.iter().rposition(|r| r.data.as_ref().and_then(|x| x.get("door")).and_then(Json::as_str) == Some(chat.as_str())));
                if let Some(at) = at {
                    let verdict = s(d, "verdict");
                    let word = cell(verdict, match verdict { "passed" => Tone::Good, "failed" => Tone::Bad, _ => Tone::Warn });
                    // the word stands before the time; a second check of the same turn takes its place
                    let r = &mut self.rows[at];
                    if let Some(data) = r.data.as_mut() {
                        if data.get("verdict").is_some() {
                            r.cells[2] = word;
                        } else {
                            r.cells.insert(2, word);
                        }
                        data["verdict"] = serde_json::json!(verdict);
                    }
                }
            }
            "chat.merged" => {
                let ok = d.get("ok").and_then(Json::as_bool) != Some(false);
                let what = s(d, "what");
                let words = format!("{} → {chat} merged{}  {}", s(d, "lane"), if what.is_empty() { String::new() } else { format!("  {what}") }, if ok { "gate ✓" } else if d.get("still").and_then(Json::as_bool) == Some(true) { "fails as before" } else { "unproven" });
                let at = self.own(ev, "commit", words, if ok { Tone::Good } else { Tone::Warn });
                self.rows[at].cells.push(time);
                self.rows[at].data = Some(stamp(at_ms));
            }
            _ => {
                let (done, total) = (d.get("done").and_then(Json::as_u64).unwrap_or(0), d.get("total").and_then(Json::as_u64).unwrap_or(0));
                let lanes: Vec<String> = d.get("lanes").and_then(Json::as_array).map(|l| l.iter().map(|x| format!("{} {}", s(x, "label"), s(x, "doer")).trim().to_string()).collect()).unwrap_or_default();
                let data = serde_json::json!({ "plan": { "chat": chat, "done": done, "total": total, "lanes": lanes.join(" ▸ ") } });
                let words = format!("plan {done}/{total}");
                match self.plans.get(&chat).copied() {
                    // the plan of a chat is one row, said again in place
                    Some(at) => {
                        self.rows[at].cells = vec![Cell::with_icon("round", words, Tone::Plain), cell("", Tone::Dim), time];
                        self.rows[at].data = Some(stamped(data, at_ms));
                    }
                    None => {
                        let at = self.own(ev, "round", words, Tone::Plain);
                        self.rows[at].cells.push(time);
                        self.rows[at].data = Some(stamped(data, at_ms));
                        self.plans.insert(chat, at);
                    }
                }
            }
        }
    }

    /// A message between chats. `data.here` is the chat this fold is of (empty: the orchestrator), put there by whoever feeds it: the
    /// sender shows `→ c4` and then `delivered`, the receiver `from c3` once a turn's brief carried it (nothing while it waits), the orchestrator `c3 → c4`.
    fn mail(&mut self, ev: &Ev) {
        let d = &ev.data;
        let Some(here) = d.get("here").and_then(Json::as_str) else { return };
        let id = s(d, "id").to_string();
        let top = here.is_empty();
        let delivered = ev.kind == "chat.message.delivered";
        match s(d, "step") {
            "released" | "decided" if !delivered => {
                if let Some(m) = self.mails.get_mut(&id) {
                    if s(d, "step") == "released" {
                        m.state = "queued".into();
                    } else {
                        m.woke = if d.get("wake").and_then(Json::as_bool) == Some(true) { "woken".into() } else { "left asleep".into() };
                    }
                }
                return self.mail_redraw(&id);
            }
            _ => {}
        }
        if !delivered {
            let m = Mail { row: None, from: s(d, "from").into(), to: s(d, "to").into(), text: s(d, "text").into(), state: s(d, "state").into(), woke: String::new(), cut: String::new(), at_ms: 0 };
            let drawn = top || m.from == here;
            let tone = if m.state == "held" { Tone::Warn } else { Tone::Plain };
            let head = if top { format!("{} → {}", m.from, m.to) } else { format!("→ {}", m.to) };
            let cut = if d.get("cut").and_then(Json::as_bool) == Some(true) { format!(" · cut to {} of {} characters", m.text.chars().count(), d.get("len").and_then(Json::as_u64).unwrap_or(0)) } else { String::new() };
            let at = drawn.then(|| self.note(ev, "message", format!("{head}  {}", clip(&m.text, LINE_MAX)), tone));
            self.mails.insert(id.clone(), Mail { row: at, cut, ..m });
            return self.mail_redraw(&id);
        }
        // the delivery: the sender's and the orchestrator's row says so, and the receiver is shown the message now
        let (from, to) = (s(d, "from").to_string(), s(d, "to").to_string());
        let known = self.mails.get(&id).map(|m| m.text.clone());
        if let Some(m) = self.mails.get_mut(&id) {
            m.state = "delivered".into();
            if d.get("woke").and_then(Json::as_bool) == Some(true) {
                m.woke = "woken".into();
            }
        }
        if !top && to == here {
            let text = known.unwrap_or_else(|| s(d, "preview").to_string());
            let at = self.note(ev, "message", format!("from {from}  {}", clip(&text, LINE_MAX)), Tone::Plain);
            self.rows[at].cells[1] = cell(time_of_day(ev.at_ms, self.zone), Tone::Dim);
            let m = Mail { row: Some(at), from, to, text, state: "delivered".into(), woke: String::new(), cut: String::new(), at_ms: ev.at_ms };
            self.mails.insert(format!("{id}@in"), m);
            return self.mail_redraw(&format!("{id}@in"));
        }
        self.mail_redraw(&id)
    }

    /// The row of a message, said again from what is known of it.
    fn mail_redraw(&mut self, id: &str) {
        let Some(m) = self.mails.get(id) else { return };
        let Some(at) = m.row else { return };
        let state = match (m.state.as_str(), m.woke.as_str()) {
            ("delivered", "") => "delivered".to_string(),
            (st, "") => st.to_string(),
            (st, w) => format!("{st} · {} {w}", m.to),
        };
        let state = format!("{state}{}", m.cut);
        let at_ms = m.at_ms;
        let message = j4n_proto::Message { id: id.trim_end_matches("@in").into(), from: m.from.clone(), to: m.to.clone(), via: "J4n".into(), text: m.text.clone(), state: m.state.clone() };
        let inbound = id.ends_with("@in");
        let r = &mut self.rows[at];
        if !inbound {
            r.cells[1] = cell(state, Tone::Dim);
        }
        r.data = Some(serde_json::json!({ "message": message }));
        if inbound {
            r.data = r.data.take().map(|d| stamped(d, at_ms));
        }
    }

    /// An event the chat says in a line of words. What the core says to the operator (`chat.note`: the answer to a command, a refusal) is
    /// words on the turn that the operator reads; what the core only reports (a binding, a notice, a stop) is a mark the screen folds.
    fn noted(&mut self, ev: &Ev) {
        let Some(line) = settled_line(ev) else { return };
        if ev.kind == "chat.note" && let Some(tree) = ev.data.get("tree").and_then(Json::as_array).filter(|t| !t.is_empty()) {
            return self.card(ev, tree, line.tone);
        }
        let (icon, tone) = match (ev.kind.as_str(), line.role) {
            ("chat.note", _) => ("", line.tone),
            ("thread.bound" | "thread.fresh", _) => ("session", line.tone),
            (_, Role::Error) => ("error", Tone::Bad),
            _ => ("notice", line.tone),
        };
        let at = self.note(ev, icon, line.text, tone);
        if ev.kind == "chat.note" && s(&ev.data, "read") == "reading" {
            self.reading = Some(at);
        }
        // a note that leads somewhere carries its act: the screen does it on `⏎` while the row is the newest
        if let Some(act) = ev.data.get("act").and_then(|a| serde_json::from_value::<j4n_proto::Act>(a.clone()).ok()) {
            self.rows[at].actions = vec![act];
        }
    }

    /// A card a pack made as rows `[depth, text]` (the result ledger's): the head is the note, the rest hang from the row of the depth above, so the
    /// pipes are the screen's and a long row wraps under its own words.
    fn card(&mut self, ev: &Ev, tree: &[Json], tone: Tone) {
        let mut keys: Vec<String> = Vec::new();
        for (i, e) in tree.iter().enumerate() {
            let (depth, text) = (e[0].as_u64().unwrap_or(1) as usize, e[1].as_str().unwrap_or_default().to_string());
            if i == 0 {
                let at = self.note(ev, "", text, tone);
                keys.push(self.rows[at].key.clone());
                continue;
            }
            let parent = keys[depth.clamp(1, keys.len()) - 1].clone();
            let key = self.key(ev);
            let mut cells = vec![cell(text, tone), cell("", Tone::Dim)];
            cells[0].tone = tone;
            self.push(row(&key, &parent, By::Core, std::mem::take(&mut cells)));
            keys.truncate(depth.max(1));
            keys.push(key);
        }
    }

    fn queued(&mut self, ev: &Ev) {
        let Some(id) = ev.task.clone() else { return };
        let d = &ev.data;
        let spec = d.get("spec").unwrap_or(&Json::Null);
        let doer = s(spec, "doer").to_string();
        if doer == HUMAN {
            return self.question(ev, &id, spec);
        }
        if matches!(s(spec, "role"), "look" | "split") {
            self.hidden.insert(id);
            return;
        }
        // a task a rule carried on: said once, among the turn's own
        self.noted(ev);
        let turn = self.turn_key(ev);
        let at = self.turn.unwrap_or(0);
        let n = self.lettered.entry(at).or_insert(0);
        let letter = (b'A' + (*n % 26)) as char;
        *n += 1;
        let model = spec.pointer("/tune/model").and_then(Json::as_str).unwrap_or_default().to_string();
        let place = spec.get("place").and_then(Json::as_str).unwrap_or_default().to_string();
        let step = s(spec, "role").to_string();
        let row_at = self.push(row(&id, &turn, By::Core, Vec::new()));
        self.lanes.insert(id.clone(), Lane { row: row_at, letter, doer, step, asked: model, served: String::new(), place, phase: Phase::Queued, not_run: String::new(), started: None, last: ev.at_ms, usd: 0.0, plan: 0.0, planned: false, out_done: 0, out_live: 0, code: d.get("code").and_then(Json::as_bool).unwrap_or(false), doing: Doing::Idle, since: ev.at_ms, live: false, word: String::new(), of: self.rows[at].cells.get(2).map(|c| c.text.clone()).unwrap_or_default() });
        self.redraw(&id);
    }

    /// A question put to the operator: a step of the lane that asked, its key the question's own so that the screen finds its answers.
    fn question(&mut self, ev: &Ev, id: &str, spec: &Json) {
        let home = spec.get("parent").and_then(Json::as_str).and_then(|p| self.lanes.get(p)).map(|l| self.rows[l.row].key.clone());
        let home = match home {
            Some(h) => h,
            None => self.turn_key(ev),
        };
        let text = spec.pointer("/input/ask/question").and_then(Json::as_str).unwrap_or_else(|| s(spec, "goal"));
        let at = self.push(row(id, &home, By::Doer, vec![Cell::with_icon("ask", clip(text, LINE_MAX), Tone::Plain), cell("", Tone::Dim)]));
        self.asks.insert(id.to_string(), at);
        if let Some(parent) = spec.get("parent").and_then(Json::as_str).filter(|p| self.lanes.contains_key(*p) && spec.pointer("/input/ask/shape").and_then(Json::as_str) == Some("approval")) {
            let target = spec.pointer("/input/ask/act/target").and_then(Json::as_str).unwrap_or_default().trim().to_string();
            self.approvals.insert(id.to_string(), (parent.to_string(), target));
        }
    }

    /// The group view: an approval the Gate, a rule or the operator said yes to is no row of its own but one word at the right of the step it let
    /// run, the call of its lane that waits for it: `✓ allowed`, or `✓ you` for the operator's. A no, a Decider's or a model's answer, and a question
    /// whose step is not known keep their row, which says who answered.
    fn answered(&mut self, ev: &Ev, ask: &str, at: usize) {
        let Some((lane, target)) = self.approvals.get(ask).filter(|_| self.group.is_some() && s(&ev.data, "answer").eq_ignore_ascii_case("yes")) else { return };
        let word = match &ev.by {
            crate::change::Actor::Operator => "you",
            _ if s(&ev.data, "via") == "human" => "you",
            crate::change::Actor::Rule(_) => "allowed",
            _ => return,
        };
        // only on the call the approval is about: its words and the approval's target agree; a doer that names its call otherwise keeps the row
        let about = |r: &Row| r.cells.first().is_some_and(|c| !target.is_empty() && !c.text.is_empty() && (c.text.contains(target.as_str()) || target.contains(c.text.as_str())));
        let Some(step) = self.calls.get(lane).and_then(|c| c.last()).map(|(_, r)| *r).filter(|r| about(&self.rows[*r])) else {
            let held = (target.clone(), word, at);
            self.held.insert(lane.clone(), held);
            return;
        };
        self.worded(step, word, at);
    }

    /// The word of an answered approval on its step, `at` the question's row, which the group view leaves out from now on. A step with a word of
    /// its own is not merged with its neighbours, which would show their names and a count in place of it; a bare tick of the step gives way to it.
    fn worded(&mut self, step: usize, word: &'static str, at: usize) {
        let r = &mut self.rows[step];
        r.merge = false;
        r.cells.retain(|c| !(c.icon == "done" && c.text.is_empty()));
        r.cells.push(Cell::with_icon("yes", word, Tone::Good));
        let key = self.rows[at].key.clone();
        self.dropped.insert(key);
    }

    fn settled(&mut self, ev: &Ev) {
        let id = ev.task.clone().unwrap_or_default();
        let d = &ev.data;
        if let Some(at) = self.asks.get(&id).copied() {
            let answer = s(d, "answer");
            let icon = match answer.to_lowercase().as_str() {
                "yes" => "yes",
                "no" => "no",
                _ => "done",
            };
            let r = &mut self.rows[at];
            r.cells[0].icon = icon.into();
            r.cells[1] = cell(clip(answer, LINE_MAX), Tone::Dim);
            return self.answered(ev, &id, at);
        }
        if self.lanes.contains_key(&id) {
            let ok = d.get("ok").and_then(Json::as_bool).unwrap_or(true);
            if let Some(l) = self.lanes.get_mut(&id) {
                l.not_run = s(d, "not_run").to_string();
            }
            self.phase(&id, Phase::Done(ok));
            self.noted(ev);
        }
    }

    /// Words the task is writing now: one step that grows until the settled message takes its place.
    fn delta(&mut self, ev: &Ev) {
        let id = ev.task.clone().unwrap_or_default();
        let text = s(&ev.data, "text");
        if text.is_empty() {
            return;
        }
        if let Some(at) = self.open.get(&id).copied() {
            self.rows[at].cells[0].text.push_str(text);
            return;
        }
        self.made += 1;
        let home = self.home(ev);
        let key = format!("p{}.{}", if id.is_empty() { "-" } else { &id }, self.made);
        let at = self.push(row(&key, &home, By::Doer, vec![cell(text, Tone::Plain), cell("", Tone::Dim)]));
        self.open.insert(id, at);
    }

    fn message(&mut self, ev: &Ev) {
        let id = ev.task.clone().unwrap_or_default();
        let text = s(&ev.data, "text");
        if text.trim().is_empty() {
            return;
        }
        let home = self.home(ev);
        if let Some(at) = self.open.remove(&id) {
            self.rows[at].cells[0].text = text.to_string();
            return;
        }
        // the report that ends a task says what the last words said: once
        if self.rows.iter().rev().take_while(|r| r.parent.as_deref() == Some(home.as_str())).any(|r| r.cells.first().is_some_and(|c| c.icon.is_empty() && c.text == text)) {
            return;
        }
        self.step(ev, By::Doer, vec![cell(text, Tone::Plain), cell("", Tone::Dim)]);
    }

    /// A call that has begun and is still being written: what the live row says, never a step of its own (the settled call is the step).
    fn opening(&mut self, ev: &Ev) {
        let d = &ev.data;
        let (icon, text) = call_words(s(d, "name"), s(d, "target"));
        let size = d.get("bytes").and_then(Json::as_u64).unwrap_or(0);
        self.doing(ev, Doing::Call { icon, text, size });
    }

    /// The task has nothing going on that it says: what it said last stands as the live row's words, when it has any.
    fn idle(&mut self, ev: &Ev, word: String) {
        self.doing(ev, Doing::Idle);
        if let Some(l) = self.lanes.get_mut(ev.task.as_deref().unwrap_or_default()).filter(|_| !word.is_empty()) {
            l.word = word;
        }
    }

    /// What the task does now, as its last live word says. It is since when the kind of thing it does changed, not the target (a written call and its run are one), nor the size.
    fn doing(&mut self, ev: &Ev, now: Doing) {
        let Some(l) = self.lanes.get_mut(ev.task.as_deref().unwrap_or_default()) else { return };
        let same = match (&l.doing, &now) {
            (Doing::Call { icon: a, .. }, Doing::Call { icon: b, .. }) => a == b,
            (a, b) => a == b,
        };
        if !same {
            l.since = ev.at_ms;
        }
        l.doing = now;
        l.live = true;
    }

    /// The live row of every lane that works and has said something: what it is doing, the size of the call it is writing, the tokens its model has
    /// written, and how long it has been at it. A row of the fold's end, after all the others: it hangs from its lane, which makes it the lane's last
    /// step, and it is gone when the lane stops working. The screen says the time from its own clock (`data.at_ms`).
    fn working(&self, kept: &[Row]) -> Vec<Row> {
        let mut lanes: Vec<&Lane> = self.lanes.values().filter(|l| l.live && l.phase == Phase::Working).collect();
        lanes.sort_by_key(|l| l.row);
        let mut out = Vec::new();
        for l in lanes {
            let key = &self.rows[l.row].key;
            if !kept.iter().any(|r| r.key == *key) {
                continue;
            }
            let (icon, text, size) = match &l.doing {
                Doing::Call { icon, text, size } => (*icon, verb_of(icon, text), *size),
                Doing::Think => ("think", "thinking".to_string(), 0),
                Doing::Say => ("think", "replying".to_string(), 0),
                Doing::Idle => ("think", "working".to_string(), 0),
            };
            let after = if l.doing == Doing::Idle && !l.word.is_empty() { format!("after {}", clip(&l.word, LINE_MAX)) } else { String::new() };
            let tokens = l.out_done + l.out_live;
            let cells = vec![
                Cell::with_icon(icon, text, Tone::Accent),
                cell(after, Tone::Dim),
                cell(if size > 0 { size_of(size) } else { String::new() }, Tone::Dim),
                cell(if tokens > 0 { format!("{} tok", count_of(tokens)) } else { String::new() }, Tone::Dim),
                cell(clock(l.last.saturating_sub(l.since)), Tone::Dim),
            ];
            out.push(Row { data: Some(stamp(l.since)), ..row(&format!("w:{}", self.rows[l.row].key), key, By::Doer, cells) });
        }
        out
    }

    fn call(&mut self, ev: &Ev) {
        let name = s(&ev.data, "name");
        let (icon, text) = open_text(name, &target_of(s(&ev.data, "detail")));
        let id = ev.task.clone().unwrap_or_default();
        let cells = vec![Cell::with_icon(icon, text, Tone::Plain), cell("", Tone::Dim)];
        // a call that was written before it ran is about what was written (the command, the path), not about the words it was described by
        let (_, mut shown) = call_words(name, &target_of(s(&ev.data, "detail")));
        if let Some(Doing::Call { icon: i, text: t, size }) = self.lanes.get(&id).map(|l| &l.doing)
            && *i == icon
            && *size > 0
            && !t.is_empty()
        {
            shown = t.clone();
        }
        if let Some(l) = self.lanes.get_mut(&id) {
            l.word = shown.clone();
        }
        self.doing(ev, Doing::Call { icon, text: shown, size: 0 });
        let at = self.step(ev, By::Doer, cells);
        self.rows[at].merge = icon == "read";
        self.calls.entry(id).or_default().push((name.to_string(), at));
    }

    /// The doer's own list: one quiet step of the lane, `2 of 5 left · writing tests`, said again in place at each change and, when
    /// every item is closed, `5 done`. A lane that has no list has no such row.
    fn list(&mut self, ev: &Ev) {
        let d = &ev.data;
        let (open, total) = (d.get("open").and_then(Json::as_u64).unwrap_or(0), d.get("total").and_then(Json::as_u64).unwrap_or(0));
        let active = s(d, "active");
        let (icon, text) = match (open, active.is_empty()) {
            (0, _) => ("todo-done", format!("{total} done")),
            (_, true) => ("todo", format!("{open} of {total} left")),
            _ => ("todo", format!("{open} of {total} left · {}", clip(active, LINE_MAX))),
        };
        let cells = vec![Cell::with_icon(icon, text, Tone::Dim), cell("", Tone::Dim)];
        let id = ev.task.clone().unwrap_or_default();
        match self.lists.get(&id) {
            Some(at) => self.rows[*at].cells = cells,
            None => {
                let at = self.step(ev, By::Doer, cells);
                self.lists.insert(id, at);
            }
        }
    }

    /// A call that failed says so on its row; one that did not changes nothing.
    fn result(&mut self, ev: &Ev) {
        let id = ev.task.clone().unwrap_or_default();
        let name = s(&ev.data, "name");
        let Some(open) = self.calls.get_mut(&id) else { return };
        let Some(at) = open.iter().position(|(n, _)| n == name).map(|i| open.remove(i).1) else { return };
        if s(&ev.data, "detail").starts_with("failed") {
            let r = &mut self.rows[at];
            r.merge = false;
            r.tone = Tone::Bad;
            r.cells.push(Cell::with_icon("no", "", Tone::Bad));
            r.detail = vec![cell(clip(s(&ev.data, "detail"), LINE_MAX), Tone::Bad)];
        }
    }

    fn shell(&mut self, ev: &Ev) {
        let d = &ev.data;
        let exit = d.get("exit").and_then(Json::as_i64).unwrap_or(0);
        let mut cells = vec![Cell::with_icon("run", clip(s(d, "cmd"), LINE_MAX), Tone::Plain), cell("", Tone::Dim)];
        cells.push(if exit == 0 { Cell::with_icon("done", "", Tone::Good) } else { Cell::with_icon("no", exit.to_string(), Tone::Bad) });
        let at = self.step(ev, By::Doer, cells);
        let cmd = s(d, "cmd").trim();
        if let Some((_, word, q)) = ev.task.as_deref().and_then(|t| self.held.remove(t)).filter(|(t, _, _)| !cmd.is_empty() && (cmd.contains(t.as_str()) || t.contains(cmd))) {
            self.worded(at, word, q);
        }
        let out: Vec<&str> = s(d, "out").lines().collect();
        let from = out.len().saturating_sub(OUT_LINES);
        self.rows[at].detail = out[from..].iter().map(|l| cell(clip(l, LINE_MAX), Tone::Plain)).collect();
    }

    /// The files a task changed, for a doer that tells which ones without the calls that changed them.
    fn changed(&mut self, ev: &Ev) {
        let paths: Vec<String> = ev.data.get("paths").and_then(Json::as_array).map(|a| a.iter().filter_map(Json::as_str).map(String::from).collect()).unwrap_or_default();
        let home = self.home(ev);
        for p in paths {
            let known = self.rows.iter().any(|r| r.parent.as_deref() == Some(home.as_str()) && r.cells.first().is_some_and(|c| matches!(c.icon.as_str(), "edit" | "new") && c.text == p));
            if !known {
                self.step(ev, By::Doer, vec![Cell::with_icon("edit", p, Tone::Plain), cell("", Tone::Dim)]);
            }
        }
    }
}

/// The rows of a thread from its events, in order: what a client that has just come is shown.
pub fn fold<'a>(events: impl IntoIterator<Item = &'a Ev>) -> Rows {
    fold_with(Show::default(), events)
}

/// The same, with the money said as `show` says.
pub fn fold_with<'a>(show: Show, events: impl IntoIterator<Item = &'a Ev>) -> Rows {
    fold_in(show, None, events)
}

/// The same, in orchestrate's group view when `group` names the thread's own lane (`Main`, or the chat's id).
pub fn fold_in<'a>(show: Show, group: Option<String>, events: impl IntoIterator<Item = &'a Ev>) -> Rows {
    let mut rows = Rows::new();
    rows.show = show;
    rows.group = group;
    for ev in events {
        rows.put(ev);
    }
    rows
}
