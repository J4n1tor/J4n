//! The `tasks` fold: every task, its state and what came of it, rebuilt from the Log. It is the one place a task's state is
//! kept — the loop reads it, the pages read it, and a restart finds it where it was.

use super::{Changed, SessionRef, State, TaskSpec, Why, id_number, micros};
use crate::cost::Shares;
use crate::fold::Fold;
use crate::log::Ev;
use crate::value::{Key, Value};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// The most tasks the fold keeps; past it the oldest finished ones are left out (and said).
pub const KEEP: usize = 5000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub id: String,
    pub spec: TaskSpec,
    pub state: State,
    /// The answer of a task that finished, or the report a stopped one had when it stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    /// Why a task that finished is not `ok`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Why a task that ended well did not do its job (`task.settled` says `not_run`): a check that could not run is no evidence, and no pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_run: Option<String>,
    #[serde(default)]
    pub todo_left: u32,
    /// The words of the item of the doer's list that was in hand when it ended with some left.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub active: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionRef>,
    pub queued_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_ms: Option<u64>,
    /// Millionths of a dollar.
    #[serde(default)]
    pub spent: i64,
    /// Millionths of a dollar at list price; what `spent` is when the task ran on no plan.
    #[serde(default)]
    pub listed: i64,
    /// A request of the task was paid by a plan: `spent` is its plan cost and `listed` is beside it.
    #[serde(default)]
    pub planned: bool,
    /// What the requests of the task took of the plan's windows (`0.8% of 5h`); empty when none was on a plan with a window to say.
    #[serde(default, skip_serializing_if = "Shares::is_empty")]
    pub shares: Shares,
    /// What the last request on a plan said of its price: where to put one when the plan has none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub plan_note: String,
    #[serde(default)]
    pub tokens_in: u64,
    #[serde(default)]
    pub tokens_out: u64,
    /// What the last call of the task's newest turn read, when its doer said it: the size of the session when the turn ended.
    #[serde(default)]
    pub last: u64,
    /// The parts of `tokens_in` read from the cache and written to it, over the task's turns (with the list cost they price a token).
    #[serde(default)]
    pub cached: u64,
    #[serde(default)]
    pub wrote: u64,
    /// What the first call of that turn read: on the first turn of a session, its floor.
    #[serde(default)]
    pub first: u64,
    /// The model that served the task, as the last `run.served` named it; `spec.tune.model` is only what was asked for.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    #[serde(default)]
    pub capped: bool,
    #[serde(default)]
    pub changed: Changed,
    /// How many times in a row rules (and nobody else) have carried this task on from the one before it.
    #[serde(default)]
    pub auto: u32,
    /// How many times the session this task belongs to has been shrunk, in this chain of tasks.
    #[serde(default)]
    pub compactions: u32,
    /// The goal of the first task of the chain: the operator's own words.
    #[serde(default)]
    pub origin: String,
    /// What kind of failure it stopped with (`failure` rows), when it failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    /// How full the doer's context was when it finished, from 0 to 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<f64>,
    /// The operator's last steers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompts: Vec<String>,
    /// Questions answered while it ran, with the answers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<String>,
    /// Work the task left in a place of its own that is not in the project's folder yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kept: Option<crate::place::Kept>,
    /// Work of the task that was brought to the project's folder from the place it was done in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied: Option<crate::place::Work>,
    /// The held work a lay-in took, and where it was held: held again when the lay-in is rewound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub laid: Option<crate::place::Kept>,
}

/// The model of a task's work: the one that served, else the one that was asked for, else the words that say it is not known
/// yet, never nothing. The scripted `test` doer serves no model (empty).
pub fn model_word(doer: &str, served: &str, asked: &str) -> String {
    match [served, asked].into_iter().find(|m| !m.is_empty()) {
        Some(m) => model_name(m),
        None if doer == "test" => String::new(),
        None => "model not known yet".to_string(),
    }
}

/// The short name of a model id, for the operator's eyes: `claude-sonnet-5-5` is `sonnet 5.5`, `anthropic/claude-haiku-5.5`
/// is `haiku 5.5`, `claude-opus-4-1-20250805` is `opus 4.1`, `claude-3-5-sonnet-20241022` is `sonnet 3.5`. The rule: the maker's prefix
/// (`vendor/`, a leading `~`, `claude-`) and a trailing date go; a number after a word is set off by a blank, a number after a number
/// is a dot (`5-5` is `5.5`); any other dash stays (`gpt-oss-20b`), and a suffix (`[1m]`, `:batch`) is kept. A word that is already
/// short (`haiku`) and one with blanks in it (`model not known yet`) are returned as they are. The Log keeps the id.
pub fn model_name(id: &str) -> String {
    let id = id.trim();
    if id.contains(char::is_whitespace) {
        return id.to_string();
    }
    let (name, suffix) = id.find(['[', ':']).map_or((id, ""), |at| id.split_at(at));
    let name = name.trim_start_matches('~');
    let name = name.rsplit('/').next().unwrap_or(name);
    let name = name.strip_prefix("claude-").unwrap_or(name);
    let mut parts: Vec<&str> = name.split('-').filter(|p| !p.is_empty()).collect();
    if parts.len() > 1 && parts.last().is_some_and(|p| p.len() == 8 && p.starts_with("20") && p.bytes().all(|b| b.is_ascii_digit())) {
        parts.pop();
    }
    // a version is digits and dots; three digits or more without a dot are a size or a date, not a version
    let ver = |p: &str| p.bytes().next().is_some_and(|b| b.is_ascii_digit()) && p.bytes().all(|b| b.is_ascii_digit() || b == b'.') && (p.len() < 3 || p.contains('.'));
    // `3-5-sonnet`: the version leads, the name follows
    let lead = parts.iter().take_while(|p| ver(p)).count();
    if lead > 0 && lead < parts.len() {
        parts.rotate_left(lead);
    }
    let mut out = String::new();
    for (n, p) in parts.iter().enumerate() {
        if n > 0 {
            out.push(match (ver(parts[n - 1]), ver(p)) {
                (true, true) => '.',
                (false, true) => ' ',
                _ => '-',
            });
        }
        out.push_str(p);
    }
    format!("{out}{suffix}")
}

/// Who did the work, `doer · model`.
pub fn who(doer: &str, served: &str, asked: &str) -> String {
    match model_word(doer, served, asked).as_str() {
        "" => doer.to_string(),
        m => format!("{doer} · {m}"),
    }
}

impl Row {
    /// [`who`] for this task.
    pub fn who(&self) -> String {
        let who = who(&self.spec.doer, &self.model, self.spec.tune.model.as_deref().unwrap_or_default());
        // the login it ran as is said last
        match self.spec.account.as_deref().filter(|a| !a.is_empty()) {
            Some(a) => format!("{who} · {a}"),
            None => who,
        }
    }
}

/// The most steers and decisions a row keeps.
const REMEMBERED: usize = 8;

fn remember(list: &mut Vec<String>, line: String) {
    list.push(line);
    if list.len() > REMEMBERED {
        list.remove(0);
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Inner {
    rows: BTreeMap<u64, Row>,
    #[serde(default)]
    dropped: u64,
    /// The operator's last line in each thread, as typed (a compaction keeps this as the goal, not a planning turn's prompt).
    #[serde(default)]
    said: BTreeMap<String, String>,
}

/// The fold `tasks`. A clone is a second handle on the same state: add one to the fold host and keep the other to ask.
#[derive(Clone, Default)]
pub struct Book(Arc<(Mutex<Inner>, Condvar)>);

impl Book {
    pub fn new() -> Book {
        Book::default()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get(&self, id: &str) -> Option<Row> {
        self.lock().rows.get(&id_number(id)?).cloned()
    }

    /// Every task, oldest first.
    pub fn rows(&self) -> Vec<Row> {
        self.lock().rows.values().cloned().collect()
    }

    /// The tasks of one doer's session in one thread, oldest first (what the session holds).
    pub fn of_session(&self, thread: Option<&str>, session: &super::SessionRef) -> Vec<Row> {
        self.lock().rows.values().filter(|r| r.session.as_ref() == Some(session) && r.spec.thread.as_deref() == thread).cloned().collect()
    }

    /// The price of one fresh input token, in dollars, as the newest task of `doer` on `model` that was priced says it: its list cost over the
    /// tokens it read and wrote at the provider's ratios. None: no task of that model has a cost and tokens yet.
    pub fn unit(&self, doer: &str, model: &str, ratios: &crate::cost::Ratios) -> Option<f64> {
        self.lock().rows.values().rev().filter(|r| r.spec.doer == doer && r.model == model).find_map(|r| super::context::unit_of(r, ratios))
    }

    /// The price of a token of `doer` on `model` that no turn has priced: the newest priced turn of another of its models, times the ratio of their price
    /// words (`offload.prices`, `haiku=1 … opus=5`); none when no turn of the doer is priced or a model has no word.
    pub fn unit_scaled(&self, doer: &str, model: &str, ratios: &crate::cost::Ratios, prices: &[(String, f64)]) -> Option<f64> {
        let (want, of) = (crate::front::tiers::price_of(prices, model)?, |m: &str| crate::front::tiers::price_of(prices, m));
        self.lock().rows.values().rev().filter(|r| r.spec.doer == doer).find_map(|r| Some(super::context::unit_of(r, ratios)? * want / of(&r.model)?))
    }

    /// The tasks that are not finished.
    pub fn open(&self) -> Vec<Row> {
        self.lock().rows.values().filter(|r| !r.state.is_terminal()).cloned().collect()
    }

    pub fn children(&self, id: &str) -> Vec<Row> {
        self.lock().rows.values().filter(|r| r.spec.parent.as_deref() == Some(id)).cloned().collect()
    }

    /// The questions waiting for an answer, oldest first, as the binder reads them (`crate::binder`).
    pub fn open_asks(&self) -> Vec<crate::binder::OpenAsk> {
        let inner = self.lock();
        inner
            .rows
            .values()
            .filter(|r| r.spec.doer == super::HUMAN && r.state == State::Running)
            .filter_map(|r| {
                let ask = r.spec.ask()?;
                let asker = r.spec.parent.clone()?;
                let thread = r.spec.thread.clone().unwrap_or_default();
                Some(crate::binder::OpenAsk { ask: r.id.clone(), asker, thread, question: ask.question, shape: ask.shape, opened_ms: r.queued_ms })
            })
            .collect()
    }

    /// The operator's last line in `thread`.
    pub(super) fn said(&self, thread: &str) -> Option<String> {
        self.lock().said.get(thread).cloned()
    }

    pub(super) fn thread(&self, id: &str) -> Option<String> {
        self.lock().rows.get(&id_number(id)?)?.spec.thread.clone()
    }

    /// The number the next task gets.
    pub(super) fn next_number(&self) -> u64 {
        self.lock().rows.keys().next_back().map_or(1, |n| n + 1)
    }

    /// The row of `id` once `ready` holds for it, waiting up to `limit`; the row as it stands when time is up is not
    /// returned — `None` says the wait ran out.
    pub fn wait(&self, id: &str, ready: impl Fn(&Row) -> bool, limit: Duration) -> Option<Row> {
        let n = id_number(id)?;
        let end = Instant::now() + limit;
        let mut inner = self.lock();
        loop {
            if let Some(row) = inner.rows.get(&n).filter(|r| ready(r)) {
                return Some(row.clone());
            }
            let left = end.checked_duration_since(Instant::now())?;
            inner = self.0.1.wait_timeout(inner, left).unwrap_or_else(|e| e.into_inner()).0;
        }
    }
}

fn text(data: &Json, key: &str) -> Option<String> {
    data.get(key).and_then(Json::as_str).map(str::to_string)
}

fn row_value(r: &Row, children: &[String]) -> Value {
    let t = |s: &str| Value::text(s);
    let mut m = BTreeMap::from([
        ("id".to_string(), t(&r.id)),
        ("goal".to_string(), t(&r.spec.goal)),
        ("doer".to_string(), t(&r.spec.doer)),
        ("model".to_string(), t(&r.model)),
        ("place".to_string(), t(&r.spec.place)),
        ("mode".to_string(), t(if r.spec.tune.mode == super::Mode::Read { "read" } else { "write" })),
        ("state".to_string(), t(r.state.word())),
        ("spent".to_string(), Value::Usd(r.spent)),
        ("listed".to_string(), Value::Usd(r.listed)),
        ("tokens_in".to_string(), Value::num(r.tokens_in as f64)),
        ("tokens_out".to_string(), Value::num(r.tokens_out as f64)),
        ("capped".to_string(), Value::Bool(r.capped)),
        ("todo_left".to_string(), Value::num(f64::from(r.todo_left))),
        ("queued_ms".to_string(), Value::num(r.queued_ms as f64)),
        ("children".to_string(), Value::List(children.iter().map(|c| t(c)).collect())),
        ("auto".to_string(), Value::num(f64::from(r.auto))),
        ("compactions".to_string(), Value::num(f64::from(r.compactions))),
        ("origin".to_string(), t(&r.origin)),
        ("kept".to_string(), Value::Bool(r.kept.is_some())),
        ("applied".to_string(), Value::Bool(r.applied.is_some())),
    ]);
    let mut put = |k: &str, v: Option<Value>| {
        if let Some(v) = v {
            m.insert(k.to_string(), v);
        }
    };
    put("parent", r.spec.parent.as_deref().map(t));
    put("follows", r.spec.follows.as_deref().map(t));
    put("thread", r.spec.thread.as_deref().map(t));
    put("answer", r.answer.as_deref().map(t));
    put("reason", r.reason.as_deref().map(t));
    put("session", r.session.as_ref().map(|s| t(&s.id)));
    put("class", r.class.as_deref().map(t));
    put("window", r.window.map(Value::num));
    put("started_ms", r.started_ms.map(|n| Value::num(n as f64)));
    put("ended_ms", r.ended_ms.map(|n| Value::num(n as f64)));
    match &r.state {
        State::Done { ok } => put("ok", Some(Value::Bool(*ok))),
        State::Stopped { why } => put("why", Some(t(why.word()))),
        State::Waiting { ask } => put("ask", Some(t(ask))),
        State::Cooling { until_ms } => put("until_ms", Some(Value::num(*until_ms as f64))),
        _ => {}
    }
    Value::Map(m)
}

impl Fold for Book {
    fn id(&self) -> &str {
        "tasks"
    }

    fn version(&self) -> u64 {
        9
    }

    fn wants(&self, kind: &str) -> bool {
        matches!(
            kind,
            "chat.say"
                | "task.queued"
                | "task.started"
                | "task.waiting"
                | "task.resumed"
                | "task.settled"
                | "task.stopped"
                | "task.retuned"
                | "task.moved"
                | "task.cooling"
                | "task.steered"
                | "task.kept"
                | "task.applied"
                | "task.rewound"
                | "ask.answered"
                | "task.cap_reached"
                | "run.metrics"
                | "run.served"
                | "run.session"
        )
    }

    fn apply(&mut self, ev: &Ev) {
        if ev.kind == "chat.say" {
            if let (Some(thread), true, Some(text)) = (&ev.thread, matches!(ev.by, crate::change::Actor::Operator), ev.data.get("text").and_then(Json::as_str)) {
                self.lock().said.insert(thread.clone(), text.to_string());
            }
            return;
        }
        let Some(n) = ev.task.as_deref().and_then(id_number) else { return };
        let mut inner = self.lock();
        // the question an answer was for: the goal of the task that was the question
        let asked = (ev.kind == "ask.answered")
            .then(|| ev.data.get("ask").and_then(Json::as_str).and_then(id_number).and_then(|a| inner.rows.get(&a)).map(|r| r.spec.goal.clone()))
            .flatten();
        if ev.kind == "task.queued" {
            if let Some(spec) = ev.data.get("spec").and_then(|s| serde_json::from_value::<TaskSpec>(s.clone()).ok())
                && !inner.rows.contains_key(&n)
            {
                let before = spec.follows.as_deref().and_then(id_number).and_then(|f| inner.rows.get(&f));
                let by_rule = matches!(ev.by, crate::change::Actor::Rule(_));
                let (auto, compactions) = before.map_or((0, 0), |b| (if by_rule { b.auto + 1 } else { 0 }, b.compactions));
                let origin = before.map(|b| b.origin.clone()).filter(|o| !o.is_empty()).unwrap_or_else(|| spec.goal.clone());
                // the operator's own `:compact` is not one of the chain's: `compact.max` is for what the loop does by itself
                let compactions = compactions + u32::from(spec.compact.is_some() && !spec.shrinks_only());
                let row = Row {
                    id: format!("t{n}"),
                    spec,
                    state: State::Queued,
                    answer: None,
                    reason: None,
                    not_run: None,
                    todo_left: 0,
                    active: String::new(),
                    session: None,
                    queued_ms: ev.at_ms,
                    started_ms: None,
                    ended_ms: None,
                    spent: 0,
                    listed: 0,
                    planned: false,
                    shares: Shares::default(),
                    plan_note: String::new(),
                    tokens_in: 0,
                    tokens_out: 0,
                    last: 0,
                    first: 0,
                    cached: 0,
                    wrote: 0,
                    model: String::new(),
                    capped: false,
                    changed: Changed::default(),
                    auto,
                    compactions,
                    origin,
                    class: None,
                    window: None,
                    prompts: Vec::new(),
                    decisions: Vec::new(),
                    kept: None,
                    applied: None,
                    laid: None,
                };
                inner.rows.insert(n, row);
                while inner.rows.len() > KEEP {
                    let Some(oldest) = inner.rows.iter().find(|(_, r)| r.state.is_terminal()).map(|(k, _)| *k) else { break };
                    inner.rows.remove(&oldest);
                    inner.dropped += 1;
                }
            }
        } else if let Some(row) = inner.rows.get_mut(&n) {
            let open = !row.state.is_terminal();
            let d = &ev.data;
            match ev.kind.as_str() {
                "task.started" if open => {
                    row.state = State::Running;
                    row.started_ms.get_or_insert(ev.at_ms);
                    if let Some(s) = d.get("session").and_then(|s| serde_json::from_value(s.clone()).ok()) {
                        row.session = Some(s);
                    }
                }
                "task.waiting" if open => {
                    if let Some(ask) = text(d, "ask") {
                        row.state = State::Waiting { ask };
                    }
                }
                "task.resumed" if open => row.state = State::Running,
                "task.settled" if open => {
                    row.state = State::Done { ok: d.get("ok").and_then(Json::as_bool).unwrap_or(false) };
                    row.answer = text(d, "answer");
                    row.reason = text(d, "reason");
                    row.not_run = text(d, "not_run");
                    row.active = text(d, "active").unwrap_or_default();
                    row.todo_left = d.get("todo_left").and_then(Json::as_u64).unwrap_or(0).min(u32::MAX.into()) as u32;
                    row.changed = d.get("changed").and_then(|c| serde_json::from_value(c.clone()).ok()).unwrap_or_default();
                    row.window = d.get("window").and_then(Json::as_f64);
                    row.ended_ms = Some(ev.at_ms);
                }
                "task.kept" => row.kept = serde_json::from_value(d.clone()).ok(),
                "task.applied" => {
                    row.laid = row.kept.take();
                    row.applied = d.get("work").and_then(|w| serde_json::from_value(w.clone()).ok());
                }
                // the lay-in was taken out of the folder again: nothing of it is laid in, and what was held is held again, to be laid in once more
                "task.rewound" => {
                    if row.applied.take().is_some() {
                        row.kept = row.laid.take().map(|k| crate::place::Kept { why: "its lay-in was rewound; it is held again".into(), ..k });
                    }
                }
                "task.cooling" if open => {
                    row.state = State::Cooling { until_ms: d.get("until_ms").and_then(Json::as_u64).unwrap_or(ev.at_ms) };
                }
                "task.steered" if matches!(ev.by, crate::change::Actor::Operator) => {
                    if let Some(t) = text(d, "text") {
                        remember(&mut row.prompts, t);
                    }
                }
                "task.stopped" if open => {
                    row.class = text(d, "class");
                    let why = d.get("why").and_then(|w| serde_json::from_value(w.clone()).ok());
                    row.state = State::Stopped {
                        why: why.unwrap_or(Why::Failed { message: "stopped for a reason this version does not know".into() }),
                    };
                    row.answer = text(d, "report");
                    row.changed = d.get("changed").and_then(|c| serde_json::from_value(c.clone()).ok()).unwrap_or_default();
                    row.ended_ms = Some(ev.at_ms);
                }
                // it went on as another login, and in a session of its own
                "task.moved" => {
                    row.spec.account = text(d, "to");
                    row.spec.resume = None;
                    row.session = None;
                }
                "task.retuned" => {
                    if let Some(t) = d.get("tune").and_then(|t| serde_json::from_value(t.clone()).ok()) {
                        row.spec.tune = t;
                    }
                }
                "task.cap_reached" => row.capped = true,
                "ask.answered" => {
                    if let (Some(q), Some(a)) = (asked, text(d, "answer")) {
                        remember(&mut row.decisions, format!("{q} → {a}"));
                    }
                }
                "run.metrics" => {
                    // what a task spent against its cap is what it took of the plan, when it ran on one
                    let spent = d.get("plan_usd").or_else(|| d.get("usd")).and_then(Json::as_f64).unwrap_or(0.0);
                    row.spent = row.spent.saturating_add(micros(spent));
                    row.listed = row.listed.saturating_add(micros(d.get("usd").and_then(Json::as_f64).unwrap_or(0.0)));
                    row.planned |= d.get("plan_usd").is_some();
                    if let (Some(of), Some(frac)) = (d.get("share_of").and_then(Json::as_str), d.get("share").and_then(Json::as_f64)) {
                        row.shares.add(of, frac);
                    }
                    row.plan_note = d.get("plan").and_then(Json::as_str).filter(|w| w.starts_with("no price")).unwrap_or_default().to_string();
                    row.tokens_in = row.tokens_in.saturating_add(d.get("tokens_in").and_then(Json::as_u64).unwrap_or(0));
                    row.tokens_out = row.tokens_out.saturating_add(d.get("tokens_out").and_then(Json::as_u64).unwrap_or(0));
                    // the newest turn's size, and 0 for a turn that did not say it (a stale one would be taken for it); the floor stays the first said
                    row.last = d.get("last").and_then(Json::as_u64).unwrap_or(0);
                    if row.first == 0 {
                        row.first = d.get("first").and_then(Json::as_u64).unwrap_or(0);
                    }
                    row.cached = row.cached.saturating_add(d.get("cached").and_then(Json::as_u64).unwrap_or(0));
                    row.wrote = row.wrote.saturating_add(d.get("wrote").and_then(Json::as_u64).unwrap_or(0));
                }
                "run.served" => {
                    if let Some(model) = text(d, "model").filter(|m| !m.is_empty()) {
                        row.model = model;
                    }
                }
                "run.session" => {
                    if let (Some(doer), Some(id)) = (text(d, "doer"), text(d, "id")) {
                        row.session = Some(SessionRef { doer, id });
                    }
                }
                _ => {}
            }
        }
        drop(inner);
        self.0.1.notify_all();
    }

    fn read(&self, path: &[Key], _now_ms: u64) -> Value {
        let inner = self.lock();
        let kids = |id: &str| -> Vec<String> {
            inner.rows.values().filter(|r| r.spec.parent.as_deref() == Some(id)).map(|r| r.id.clone()).collect()
        };
        if let Some(Key::Name(id)) = path.first() {
            return match id_number(id).and_then(|n| inner.rows.get(&n)) {
                // the children are found by a look at every task: only when they are asked for
                Some(r) => {
                    let wants = path.get(1).is_none_or(|k| matches!(k, Key::Name(n) if n == "children"));
                    row_value(r, &if wants { kids(&r.id) } else { Vec::new() }).at(&path[1..])
                }
                None => Value::Missing,
            };
        }
        let all = inner.rows.values().map(|r| (r.id.clone(), row_value(r, &kids(&r.id)))).collect();
        Value::Map(all).at(path)
    }

    fn save(&self) -> Json {
        serde_json::to_value(&*self.lock()).unwrap_or_default()
    }

    fn load(&mut self, state: Json) -> Result<(), String> {
        *self.lock() = serde_json::from_value(state).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn reset(&mut self) {
        *self.lock() = Inner::default();
    }

    fn notes(&self) -> Vec<String> {
        match self.lock().dropped {
            0 => Vec::new(),
            n => vec![format!("{n} older finished tasks are left out; the most it keeps is {KEEP}")],
        }
    }
}

/// An event's data for the row it names, for the loop and the tests: `{"spec": …}`.
pub(super) fn queued(spec: &TaskSpec) -> Json {
    json!({ "spec": spec })
}
