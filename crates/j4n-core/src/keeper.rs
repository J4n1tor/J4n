//! The plan keeper: the main lane's plan is a fold of the Log, one plan per
//! thread, and nothing of it is kept anywhere else, so a restart changes nothing.
//!
//! Events: `plan.item` (`op` = `add`, `set`, `drop`, `state`), `plan.approach` (a revision, with its reason; `new` starts the plan
//! again), `plan.asked` (a planning turn was started: its task, `how` and the line), `plan.settled` (it was taken or refused, and
//! what it cost), `plan.holds` (a line that would replace the plan, waiting for the operator's yes). An event the operator wrote
//! (`by` = operator) wins over the planner: the planner's next answer does not change that item and does not bring back one the
//! operator dropped.
//!
//! A planning turn answers with one JSON object, a closed shape ([`take`] checks it and every reference before anything is
//! written; a bad answer leaves the old plan standing):
//! `{"approach": "one paragraph", "items": [{"id", "goal", "reads": [], "writes": [], "depends": [], "done"}]}`.

use crate::answer::{Answerer, fn_rule, picked};
use crate::fold::Fold;
use crate::log::Ev;
use crate::change::Actor;
use crate::value::{Key, Value};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

mod law;
pub use law::{bound as plan_bound, parts as request_parts};

pub const MAX_ITEMS: usize = 24;
/// The tokens the Brief's `plan` line may take.
pub const LINE_TOKENS: usize = 60;
pub const STATES: [&str; 8] = ["todo", "running", "reported", "checked", "reviewed", "merged", "sent-back", "blocked"];
const FIELD: usize = 160;
const PATHS: usize = 12;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Item {
    pub id: String,
    pub goal: String,
    pub state: String,
    /// Where it runs: `inline` (the main lane's own session) or the class of its branch, as J4n decided (`plan.routed`).
    pub place: String,
    /// What the planner marked: `inline`, `scout`, `review` or `piece`.
    pub mark: String,
    /// The planner says it needs a decision only the main lane holds.
    pub main: bool,
    /// Why it runs where it does, and (for a branch) as which of the three words (`via`).
    pub why: String,
    pub via: String,
    /// A chat of a split was briefed for it: it stays a chat's part when a send-back routes it again (`via` is then that route's word).
    pub chat: bool,
    /// The operator says its goal goes to its branch as typed (a `verbatim` brief). Only the operator sets it.
    pub verbatim: bool,
    /// How many times it was started.
    pub tries: u32,
    pub reads: Vec<String>,
    pub writes: Vec<String>,
    pub depends: Vec<String>,
    /// One line: how to tell it is done.
    pub done: String,
    /// The name of the project's check that must pass when it ends (a `check` row's name), or `none`; empty: the planner named no
    /// check, and the item has no contract (an item of the operator's, or one an older Log holds).
    pub check: String,
    pub result: String,
    pub rev: u32,
    /// The task that works on it, while one does.
    pub task: String,
    /// The operator touched it last: the planner does not change it.
    pub mine: bool,
    /// How many times it was sent back; the fourth would be `blocked`.
    pub sends: u32,
    /// The task that validates it now: its check or its review, or the check of its merge.
    pub side: String,
    /// How many claims of its report were `unchecked` when it was reported.
    pub unchecked: u32,
    /// Why it is `blocked`: `cap`, `merge`, `no-doer`, `parts`, `unproven`, `sends` or `failed` (the report to the operator asks for all but the last two).
    pub block: String,
    /// Its end was not shown by a check (it names none, or none could run): it went on (`merged`, or `checked` for a piece, which is
    /// still reviewed and laid in) on the doer's word, and the operator is told. Kept through the states that follow; the operator's
    /// `:plan state` takes it off, and so does a new start.
    pub unproven: bool,
    /// What went wrong with it, by kind: `check`, `weak`, `unchecked`, `repeat`, `stuck`, `blocked` (the triggers of `replan.rs`).
    pub went: BTreeMap<String, u32>,
    /// The Log events that show it, as `e<seq> <kind>: <what>`, newest last (the last six).
    pub proof: Vec<String>,
    /// The reviewer's concerns heard so far, lower-cased: one heard again after a send-back is a `repeat`.
    pub heard: Vec<String>,
}

impl Item {
    /// An item that was started is kept as it is by a new answer of the planner.
    pub fn locked(&self) -> bool {
        !matches!(self.state.as_str(), "todo" | "sent-back" | "blocked")
    }

    /// The planner named a check for it and it writes (inline or a piece), so its end is judged from the Log, not from the doer's word.
    pub fn contract(&self) -> bool {
        !self.check.is_empty() && matches!(self.place.as_str(), "inline" | "piece")
    }

    /// A chat's part of a split that writes: its work is held in a worktree, checked, reviewed and laid in.
    pub fn chat_part(&self) -> bool {
        self.chat && self.place == "piece"
    }

    /// Nothing is left to do for it: merged, or reviewed when it has nothing to merge (a `scout` or a `review`).
    pub fn done(&self) -> bool {
        self.state == "merged" || (self.state == "reviewed" && matches!(self.place.as_str(), "scout" | "review"))
    }

    fn value(&self) -> Value {
        let list = |v: &[String]| Value::List(v.iter().map(Value::text).collect());
        let paths = BTreeMap::from([("read".to_string(), list(&self.reads)), ("write".to_string(), list(&self.writes))]);
        Value::Map(BTreeMap::from([
            ("id".into(), Value::text(&self.id)),
            ("goal".into(), Value::text(&self.goal)),
            ("state".into(), Value::text(&self.state)),
            ("where".into(), Value::text(&self.place)),
            ("mark".into(), Value::text(&self.mark)),
            ("why".into(), Value::text(&self.why)),
            ("paths".into(), Value::Map(paths)),
            ("depends".into(), list(&self.depends)),
            ("done".into(), Value::text(&self.done)),
            ("result".into(), Value::text(&self.result)),
            ("rev".into(), Value::num(f64::from(self.rev))),
        ]))
    }
}

/// A planning turn that is out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asked {
    pub task: String,
    pub how: String,
    pub line: String,
    /// A replan: the Log ids it was told.
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub items: Vec<Item>,
    pub approach: String,
    /// Why the approach was revised, newest last (the last eight).
    pub reasons: Vec<String>,
    pub asked: Option<Asked>,
    /// A line that would replace the plan, waiting for a yes.
    pub held: Option<String>,
    /// Ids the operator dropped: the planner does not bring them back.
    pub gone: BTreeSet<String>,
    /// Millionths of a dollar the planning turns took, and how many there were.
    pub micros: i64,
    pub turns: u32,
    /// What the report to the operator last said (`plan.reported`): it is said once for a plan that ends or blocks in the same way.
    #[serde(default)]
    pub said: String,
    /// How often the approach was revised (`plan.approach` with a `rev`), why last, and the Log ids that showed it.
    #[serde(default)]
    pub revs: u32,
    #[serde(default)]
    pub why: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    /// The triggers a replan was already started for.
    #[serde(default)]
    pub handled: BTreeSet<String>,
    /// A replan that is the operator's to decide: it drops merged work, adds more than `plan.grow` items, or is past
    /// `plan.revisions`.
    #[serde(default)]
    pub pending: Option<Pending>,
}

/// What waits for the operator's yes (`:plan accept`) or no (`:plan reject`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pending {
    /// `drops-merged`, `grow` or `revisions`.
    pub kind: String,
    pub why: String,
    /// The planner's answer, held as it came (empty for `revisions`).
    pub text: String,
    pub evidence: Vec<String>,
}

impl Plan {
    /// Whether there is work left in it: a line is folded into it only then.
    pub fn open(&self) -> bool {
        self.items.iter().any(|i| !i.done())
    }

    /// The item to work on now: the one that runs, else the first that waits and whose dependencies are done.
    pub fn current(&self) -> Option<&Item> {
        let done = |id: &String| self.items.iter().any(|i| i.id == *id && i.done());
        self.items.iter().find(|i| i.state == "running").or_else(|| self.items.iter().find(|i| matches!(i.state.as_str(), "todo" | "sent-back") && i.depends.iter().all(done)))
    }

    /// The one line of the Brief while the plan is open: the current item and the approach, at most [`LINE_TOKENS`] tokens.
    pub fn line(&self) -> Option<String> {
        if !self.open() {
            return None;
        }
        let now = self.current().map(|i| format!("now {}: {}", i.id, i.goal)).unwrap_or_else(|| "now: nothing can start".into());
        let text = format!("{now} · approach: {}", self.approach);
        let cut: String = text.chars().take(4 * (LINE_TOKENS - 4)).collect();
        Some(if cut.len() < text.len() { format!("{cut}…") } else { cut })
    }

    fn item_mut(&mut self, id: &str) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.id == id)
    }
}

/// The fold `plan`. A clone is a second handle on the same state.
#[derive(Clone, Default)]
pub struct Keeper(Arc<Mutex<BTreeMap<String, Plan>>>);

impl Keeper {
    pub fn new() -> Keeper {
        Keeper::default()
    }

    fn lock(&self) -> MutexGuard<'_, BTreeMap<String, Plan>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn plan(&self, thread: &str) -> Option<Plan> {
        self.lock().get(thread).cloned()
    }

    pub fn open(&self, thread: &str) -> bool {
        self.lock().get(thread).is_some_and(Plan::open)
    }

    pub fn threads(&self) -> Vec<String> {
        self.lock().keys().cloned().collect()
    }
}

fn words(d: &Json, key: &str) -> Vec<String> {
    d.get(key).and_then(Json::as_array).map(|a| a.iter().filter_map(Json::as_str).map(String::from).collect()).unwrap_or_default()
}

/// Count what went wrong with `i`, with the Log event (`seq`) that shows it.
fn went(i: &mut Item, seq: u64, kinds: &[(String, u32)], what: &str) {
    for (kind, n) in kinds.iter().filter(|(_, n)| *n > 0) {
        *i.went.entry(kind.clone()).or_default() += n;
        i.proof.push(format!("e{seq} {kind}: {}", what.chars().take(120).collect::<String>()));
    }
    i.proof.drain(..i.proof.len().saturating_sub(6));
}

fn word(d: &Json, key: &str) -> Option<String> {
    d.get(key).and_then(Json::as_str).map(String::from)
}

impl Fold for Keeper {
    fn id(&self) -> &str {
        "plan"
    }

    fn version(&self) -> u64 {
        4
    }

    fn wants(&self, kind: &str) -> bool {
        matches!(kind, "plan.item" | "plan.routed" | "plan.approach" | "plan.asked" | "plan.settled" | "plan.holds" | "plan.reported" | "plan.pending" | "rule.fired" | "thread.fresh")
    }

    fn apply(&mut self, ev: &Ev) {
        let Some(thread) = ev.thread.clone() else { return };
        let mut all = self.lock();
        if ev.kind == "thread.fresh" {
            all.remove(&thread);
            return;
        }
        if ev.kind == "rule.fired" {
            // the main lane is told to step back: the rule `stuck` fired on the task of a running item
            let item = all.get_mut(&thread).zip(ev.task.as_ref()).and_then(|(p, t)| p.items.iter_mut().find(|i| i.state == "running" && i.task == *t));
            if let (Some(i), Some("stuck")) = (item, ev.data.get("rule").and_then(Json::as_str)) {
                went(i, ev.seq, &[("stuck".into(), 1)], "the rule `stuck` told the task to step back");
            }
            return;
        }
        let (d, mine) = (&ev.data, ev.by == Actor::Operator);
        let plan = all.entry(thread).or_default();
        let mut reopen = false;
        match ev.kind.as_str() {
            "plan.approach" => {
                if d.get("new").and_then(Json::as_bool) == Some(true) {
                    *plan = Plan { micros: plan.micros, turns: plan.turns, ..Plan::default() };
                }
                plan.approach = word(d, "text").unwrap_or_default();
                plan.reasons.push(word(d, "reason").unwrap_or_default());
                plan.reasons.drain(..plan.reasons.len().saturating_sub(8));
                plan.held = None;
                plan.pending = None;
                if let Some(rev) = d.get("rev").and_then(Json::as_u64).filter(|r| *r > 0) {
                    // what went wrong has been told to the planner: the counts start again
                    (plan.revs, plan.why, plan.evidence) = (rev as u32, plan.reasons.last().cloned().unwrap_or_default(), words(d, "evidence"));
                    plan.items.iter_mut().for_each(|i| i.went.clear());
                }
            }
            "plan.asked" => {
                plan.asked = Some(Asked { task: word(d, "task").unwrap_or_default(), how: word(d, "how").unwrap_or_default(), line: word(d, "line").unwrap_or_default(), evidence: words(d, "evidence") });
                plan.handled.extend(words(d, "keys"));
                plan.held = None;
            }
            "plan.pending" => {
                plan.handled.extend(words(d, "keys"));
                plan.pending = (d.get("clear").and_then(Json::as_bool) != Some(true)).then(|| Pending { kind: word(d, "kind").unwrap_or_default(), why: word(d, "why").unwrap_or_default(), text: word(d, "text").unwrap_or_default(), evidence: words(d, "evidence") });
            }
            "plan.settled" => {
                plan.asked = None;
                plan.micros += d.get("micros").and_then(Json::as_i64).unwrap_or(0);
                plan.turns += 1;
            }
            "plan.holds" => plan.held = word(d, "line"),
            "plan.reported" => plan.said = word(d, "sig").unwrap_or_default(),
            "plan.item" => {
                let Some(id) = word(d, "id") else { return };
                match word(d, "op").as_deref() {
                    Some("add") if plan.item_mut(&id).is_none() => plan.items.push(Item {
                        id,
                        goal: word(d, "goal").unwrap_or_default(),
                        state: "todo".into(),
                        place: "inline".into(),
                        mark: word(d, "where").unwrap_or_else(|| "inline".into()),
                        main: d.get("main").and_then(Json::as_bool).unwrap_or(false),
                        reads: words(d, "reads"),
                        writes: words(d, "writes"),
                        depends: words(d, "depends"),
                        done: word(d, "done").unwrap_or_default(),
                        check: word(d, "check").unwrap_or_default(),
                        mine,
                        ..Item::default()
                    }),
                    Some("set") => {
                        if let Some(i) = plan.item_mut(&id) {
                            for (key, field) in [("goal", &mut i.goal), ("done", &mut i.done), ("where", &mut i.mark), ("result", &mut i.result), ("check", &mut i.check)] {
                                if let Some(v) = word(d, key) {
                                    *field = v;
                                }
                            }
                            for (key, field) in [("reads", &mut i.reads), ("writes", &mut i.writes), ("depends", &mut i.depends)] {
                                if d.get(key).is_some() {
                                    *field = words(d, key);
                                }
                            }
                            if let Some(on) = d.get("main").and_then(Json::as_bool) {
                                i.main = on;
                            }
                            // only the operator's words go to a branch as typed
                            if let (true, Some(on)) = (mine, d.get("verbatim").and_then(Json::as_bool)) {
                                i.verbatim = on;
                            }
                            i.rev += 1;
                            i.mine |= mine;
                        }
                    }
                    Some("state") => {
                        if let Some(i) = plan.item_mut(&id) {
                            let to = word(d, "state").filter(|s| STATES.contains(&s.as_str())).unwrap_or_else(|| i.state.clone());
                            match to.as_str() {
                                "running" => i.tries += 1,
                                "sent-back" => i.sends += 1,
                                _ => {}
                            }
                            i.state = to;
                            i.result = word(d, "result").unwrap_or_default();
                            // the task that worked it stays through the states that follow; a new start names the new one
                            if let Some(t) = word(d, "task") {
                                i.task = t;
                            }
                            i.side = word(d, "side").unwrap_or_default();
                            i.unchecked = d.get("unchecked").and_then(Json::as_u64).unwrap_or(0) as u32;
                            i.block = word(d, "block").unwrap_or_default();
                            // the mark stays through the states that follow; the operator's word, or a new try, takes it off
                            i.unproven = d.get("unproven").and_then(Json::as_bool).unwrap_or(i.unproven && !mine && !matches!(i.state.as_str(), "running" | "todo" | "sent-back" | "blocked"));
                            let mut kinds: Vec<(String, u32)> = d.get("went").and_then(Json::as_object).into_iter().flatten().map(|(k, n)| (k.clone(), n.as_u64().unwrap_or(0) as u32)).collect();
                            kinds.extend((i.state == "blocked").then(|| ("blocked".to_string(), 1)));
                            for c in words(d, "concerns") {
                                let c = c.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase().chars().take(100).collect::<String>();
                                if i.heard.contains(&c) { kinds.push(("repeat".into(), 1)) } else { i.heard.push(c) }
                            }
                            let result = i.result.clone();
                            went(i, ev.seq, &kinds, &result);
                            i.rev += 1;
                            i.mine |= mine;
                            reopen = matches!(i.state.as_str(), "running" | "todo");
                        }
                    }
                    Some("redo") => {
                        // a replan gives a blocked item another go: its counts start again
                        if let Some(i) = plan.item_mut(&id).filter(|i| i.state == "blocked") {
                            (i.state, i.tries, i.sends, i.unchecked, i.rev) = ("todo".into(), 0, 0, 0, i.rev + 1);
                            (i.block, i.side) = (String::new(), String::new());
                            reopen = true;
                        }
                    }
                    Some("drop") => {
                        plan.items.retain(|i| i.id != id);
                        for i in &mut plan.items {
                            i.depends.retain(|x| *x != id);
                        }
                        if mine {
                            plan.gone.insert(id);
                        }
                    }
                    _ => {}
                }
            }
            "plan.routed" => {
                if let Some(i) = word(d, "id").and_then(|id| plan.item_mut(&id)) {
                    i.place = word(d, "place").unwrap_or_else(|| "inline".into());
                    i.why = word(d, "why").unwrap_or_default();
                    i.via = word(d, "via").unwrap_or_default();
                    i.chat |= i.via == "chat";
                }
            }
            _ => {}
        }
        if reopen || (ev.kind == "plan.item" && word(d, "op").as_deref() == Some("add")) {
            plan.said.clear();
        }
    }

    fn read(&self, path: &[Key], _now_ms: u64) -> Value {
        let plans = self.lock();
        let all = plans.iter().map(|(t, p)| {
            let micros = Value::Usd(p.micros);
            let fields = [
                ("approach", Value::text(&p.approach)),
                ("open", Value::Bool(p.open())),
                ("items", Value::List(p.items.iter().map(Item::value).collect())),
                // the tasks that were an inline item's turns: the plan owns what comes after them (a rule asks `task in …`)
                ("inline", Value::List(p.items.iter().filter(|i| i.place == "inline" && !i.task.is_empty()).map(|i| Value::text(&i.task)).collect())),
                // chats' parts on their way to be laid in: Main's own part is checked with them, at the join, not before (a rule asks `parts_out`)
                ("parts_out", Value::Bool(p.items.iter().any(|i| i.chat_part() && !matches!(i.state.as_str(), "merged" | "blocked")))),
                ("cost", micros),
                ("turns", Value::num(f64::from(p.turns))),
            ];
            (t.clone(), Value::Map(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect()))
        });
        Value::Map(all.collect()).at(path)
    }

    fn save(&self) -> Json {
        serde_json::to_value(&*self.lock()).unwrap_or(Json::Null)
    }

    fn load(&mut self, state: Json) -> Result<(), String> {
        *self.lock() = serde_json::from_value(state).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn reset(&mut self) {
        self.lock().clear();
    }
}

// ---- the planner's answer ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Said {
    approach: String,
    /// A replan says in one line why the approach changes.
    #[serde(default)]
    why: String,
    items: Vec<Given>,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct Given {
    id: String,
    goal: String,
    #[serde(default)]
    reads: Vec<String>,
    #[serde(default)]
    writes: Vec<String>,
    #[serde(default)]
    depends: Vec<String>,
    done: String,
    #[serde(default)]
    check: Option<String>,
    #[serde(default, rename = "where")]
    place: Option<String>,
    #[serde(default)]
    main: bool,
}

/// What a planning turn is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    Begin,
    Add,
    Change,
    Replace,
    /// The approach is changed, with the reason and the evidence.
    Replan,
}

impl How {
    pub fn word(self) -> &'static str {
        match self {
            How::Begin => "begin",
            How::Add => "add",
            How::Change => "change",
            How::Replace => "replace",
            How::Replan => "replan",
        }
    }

    pub fn from_word(w: &str) -> Option<How> {
        [How::Begin, How::Add, How::Change, How::Replace, How::Replan].into_iter().find(|h| h.word() == w)
    }
}

/// An item's `check` is a name from the project's own checks (`names`) or `none`. A name that is not one becomes `none`, and each
/// is said (`id: wrote …`); an item with no `check` keeps none, it has no contract. Free text never reaches a command.
pub fn bind_checks(ops: &mut [Op], names: &[String]) -> Vec<String> {
    let mut said = Vec::new();
    for op in ops {
        if let Op::Add(i) | Op::Set(i) = op
            && !i.check.is_empty()
            && i.check != "none"
            && !names.contains(&i.check)
        {
            said.push(format!("{}: `{}` is no check of this project, so `none`", i.id, i.check.chars().take(24).collect::<String>()));
            i.check = "none".into();
        }
    }
    said
}

/// One change to the plan, as an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// `rev` is 0 but for a replan, which says why (`reason`) and on what (`evidence`).
    Approach { text: String, reason: String, new: bool, rev: u32, evidence: Vec<String> },
    Add(Item),
    Set(Item),
    Drop(String),
    /// A blocked item that is kept by a replan starts again.
    Redo(String),
}

impl Op {
    /// The event this change is: its kind and data.
    pub fn event(&self, task: &str) -> (&'static str, Json) {
        match self {
            Op::Approach { text, reason, new, rev, evidence } => ("plan.approach", json!({ "text": text, "reason": reason, "new": new, "task": task, "rev": rev, "why": reason, "evidence": evidence })),
            Op::Add(i) => ("plan.item", json!({ "op": "add", "id": i.id, "goal": i.goal, "done": i.done, "check": i.check, "reads": i.reads, "writes": i.writes, "depends": i.depends, "where": i.mark, "main": i.main, "task": task })),
            Op::Set(i) => ("plan.item", json!({ "op": "set", "id": i.id, "goal": i.goal, "done": i.done, "check": i.check, "reads": i.reads, "writes": i.writes, "depends": i.depends, "where": i.mark, "main": i.main, "task": task })),
            Op::Drop(id) => ("plan.item", json!({ "op": "drop", "id": id, "task": task })),
            Op::Redo(id) => ("plan.item", json!({ "op": "redo", "id": id, "task": task })),
        }
    }
}

fn item_of(g: &Given) -> Item {
    let check = g.check.as_deref().map(|c| c.trim().to_lowercase()).unwrap_or_default();
    Item { id: g.id.clone(), goal: g.goal.clone(), done: g.done.clone(), check, reads: g.reads.clone(), writes: g.writes.clone(), depends: g.depends.clone(), mark: g.place.clone().unwrap_or_else(|| "inline".into()), main: g.main, ..Item::default() }
}

fn line_of(s: &str, what: &str) -> Result<(), String> {
    if s.trim().is_empty() || s.chars().count() > FIELD || s.chars().any(char::is_control) {
        return Err(format!("{what} is one line of up to {FIELD} characters"));
    }
    Ok(())
}

fn path_ok(p: &str) -> bool {
    !p.is_empty() && p.chars().count() <= 200 && !p.starts_with('/') && !p.starts_with('~') && !p.chars().any(char::is_control) && !p.split('/').any(|c| c == "..") && !p.contains(':')
}

fn id_ok(id: &str) -> bool {
    id.len() <= 24 && id.chars().next().is_some_and(|c| c.is_ascii_alphanumeric()) && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The object an answer holds: from its first `{` to its last `}` (a doer may wrap it in words or a fence).
fn object(text: &str) -> Option<&str> {
    let (a, b) = (text.find('{')?, text.rfind('}')?);
    (a < b).then(|| &text[a..=b])
}

fn cycle(deps: &BTreeMap<&str, Vec<&str>>) -> Option<String> {
    fn walk<'a>(id: &'a str, deps: &BTreeMap<&'a str, Vec<&'a str>>, path: &mut Vec<&'a str>, done: &mut BTreeSet<&'a str>) -> Option<String> {
        if path.contains(&id) {
            return Some(id.to_string());
        }
        if !done.insert(id) {
            return None;
        }
        path.push(id);
        for d in deps.get(id).into_iter().flatten() {
            if let Some(c) = walk(d, deps, path, done) {
                return Some(c);
            }
        }
        path.pop();
        None
    }
    let mut done = BTreeSet::new();
    deps.keys().find_map(|k| walk(k, deps, &mut Vec::new(), &mut done))
}

/// The approach as one paragraph of at most 600 characters: breaks become spaces, a longer one is cut at its last full sentence.
fn one_paragraph(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = flat.chars().take(600).collect();
    if cut.len() == flat.len() { flat } else { cut.rfind(". ").map_or(cut.clone(), |at| cut[..=at].to_string()) }
}

/// What a replan does to the plan beyond its changes: the items it adds, the merged ones it would drop (a real decision) and the
/// started ones it drops (their branches are stopped).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shift {
    pub added: usize,
    pub dropped_done: Vec<String>,
    pub dropped_live: Vec<String>,
    /// The items of a new plan that were folded into others (`x into y`), and the bound they were folded to.
    pub folded: Vec<String>,
    pub bound: usize,
    /// The waits the law took off (`x after y`) because the two share no file.
    pub freed: Vec<String>,
}

/// Check the planner's answer against the shape and against the plan it is for, and say what changes. `Err`: why it is not taken
/// (the old plan stands). Nothing is written here.
pub fn take(text: &str, old: Option<&Plan>, how: How, line: &str) -> Result<Vec<Op>, String> {
    take_with(text, old, how, line, &[]).map(|(ops, _)| ops)
}

/// [`take`], and what a replan (`evidence`: the Log ids it was told) does to the plan. A replan changes the items not yet started, gives a
/// blocked one another go, keeps every started one as it is unless it drops it, and says why in one line.
pub fn take_with(text: &str, old: Option<&Plan>, how: How, line: &str, evidence: &[String]) -> Result<(Vec<Op>, Shift), String> {
    take_within(text, old, how, line, evidence, MAX_ITEMS)
}

/// [`take_with`], for a plan that begins with at most `bound` items: what a planner gave past it is folded into the items
/// before it, and a read that only feeds one item is that item's first step. The paths of an item past the field's length are cut, not
/// refused: a plan that is refused is a run with nothing done.
pub fn take_within(text: &str, old: Option<&Plan>, how: How, line: &str, evidence: &[String], bound: usize) -> Result<(Vec<Op>, Shift), String> {
    let mut said: Said = serde_json::from_str(object(text).ok_or("the answer holds no object")?).map_err(|e| format!("the answer is not the shape of a plan: {e}"))?;
    // a line a planner runs on with is cut to the field's length and made one line, not refused
    let clip = |t: &mut String| *t = t.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(FIELD).collect();
    clip(&mut said.why);
    for g in &mut said.items {
        clip(&mut g.goal);
        clip(&mut g.done);
    }
    if how == How::Replan {
        if !old.is_some_and(Plan::open) {
            return Err("no plan is open to change the approach of".into());
        }
        line_of(&said.why, "the why of a replan")?;
    }
    let fresh = matches!(how, How::Begin | How::Replace) || old.is_none_or(|p| !p.open());
    let approach = one_paragraph(&said.approach);
    if approach.is_empty() {
        return Err("the approach is one paragraph of up to 600 characters".into());
    }
    for g in &mut said.items {
        g.reads.truncate(PATHS);
        g.writes.truncate(PATHS);
    }
    let mut shift = Shift { bound, ..Shift::default() };
    if fresh {
        shift.freed = law::apart(&mut said.items);
        shift.folded = law::fold(&mut said.items, bound.min(MAX_ITEMS));
    }
    if said.items.is_empty() || said.items.len() > MAX_ITEMS {
        return Err(format!("a plan has 1 to {MAX_ITEMS} items, this has {}", said.items.len()));
    }
    let mut seen = BTreeSet::new();
    for g in &said.items {
        if !id_ok(&g.id) || !seen.insert(g.id.as_str()) {
            return Err(format!("`{}` is not an id of its own (lowercase letters, digits and `-`, up to 24)", g.id));
        }
        line_of(&g.goal, &format!("the goal of {}", g.id))?;
        line_of(&g.done, &format!("the done-when of {}", g.id))?;
        for (what, list) in [("reads", &g.reads), ("writes", &g.writes), ("depends", &g.depends)] {
            if list.len() > PATHS {
                return Err(format!("{} has more than {PATHS} {what}", g.id));
            }
        }
        if let Some(w) = g.place.as_deref().filter(|w| *w != "inline" && crate::branch::Class::from_word(w).is_none()) {
            return Err(format!("`{w}` (in {}) is not a place: inline, scout, review or piece", g.id));
        }
        if let Some(p) = g.reads.iter().chain(&g.writes).find(|p| !path_ok(p)) {
            return Err(format!("`{p}` (in {}) is not a path inside the project", g.id));
        }
    }
    let kept = old.filter(|_| !fresh);
    let empty = Plan::default();
    let old = kept.unwrap_or(&empty);
    let known = |id: &str| old.items.iter().any(|i| i.id == id);
    let reason = if how == How::Replan { said.why.trim().to_string() } else { format!("{}: {}", how.word(), line.chars().take(80).collect::<String>()) };
    let rev = if how == How::Replan { old.revs + 1 } else { 0 };
    let mut ops = vec![Op::Approach { text: approach, reason, new: fresh, rev, evidence: evidence.to_vec() }];
    // what each item is to be after this answer; the operator's items and the started ones stay as they are
    let mut after: BTreeMap<&str, (Vec<&str>, bool)> = BTreeMap::new();
    for o in &old.items {
        let given = said.items.iter().find(|g| g.id == o.id);
        let same = |g: &Given| g.goal == o.goal && g.reads == o.reads && g.writes == o.writes && g.depends == o.depends && g.place.as_deref().unwrap_or("inline") == o.mark && g.main == o.main;
        match (given, how) {
            (None, How::Change) if o.locked() => return Err(format!("{} is {}: it is kept, and the answer leaves it out", o.id, o.state)),
            (None, How::Change) if !o.mine => {
                ops.push(Op::Drop(o.id.clone()));
                continue;
            }
            (Some(g), How::Change) if o.locked() && !same(g) => return Err(format!("{} is {}: it is kept as it is", o.id, o.state)),
            (None, How::Replan) if o.done() || !o.mine => {
                if o.done() { shift.dropped_done.push(o.id.clone()) } else if o.locked() { shift.dropped_live.push(o.id.clone()) }
                ops.push(Op::Drop(o.id.clone()));
                continue;
            }
            (Some(g), How::Change | How::Replan) if !o.mine && !o.locked() && !same(g) => {
                after.insert(&o.id, (g.depends.iter().map(String::as_str).collect(), false));
                ops.push(Op::Set(item_of(g)));
                ops.extend((how == How::Replan && o.state == "blocked").then(|| Op::Redo(o.id.clone())));
                continue;
            }
            _ => {}
        }
        ops.extend((how == How::Replan && o.state == "blocked").then(|| Op::Redo(o.id.clone())));
        after.insert(&o.id, (o.depends.iter().map(String::as_str).collect(), true));
    }
    for g in said.items.iter().filter(|g| !known(&g.id)) {
        if old.gone.contains(&g.id) {
            continue;
        }
        after.insert(&g.id, (g.depends.iter().map(String::as_str).collect(), false));
        ops.push(Op::Add(item_of(g)));
        shift.added += 1;
    }
    let mut graph: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (id, (deps, _)) in &after {
        let mut live = Vec::new();
        for d in deps {
            if *d == *id {
                return Err(format!("{id} depends on itself"));
            }
            if after.contains_key(d) {
                live.push(*d);
            } else if !old.gone.contains(*d) {
                return Err(format!("{id} depends on `{d}`, which is not an item"));
            }
        }
        graph.insert(id, live);
    }
    if let Some(c) = cycle(&graph) {
        return Err(format!("{c} depends on itself through the others"));
    }
    // a dependency on an item the operator dropped is not carried
    for op in &mut ops {
        if let Op::Add(i) | Op::Set(i) = op {
            i.depends.retain(|d| !old.gone.contains(d));
        }
    }
    Ok((ops, shift))
}

// ---- rule:fold ----

fn tokens(line: &str) -> Vec<String> {
    line.split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-').to_lowercase()).filter(|w| !w.is_empty()).collect()
}

/// What the rule says of `line` over a plan whose items are `ids`: the choice and why.
pub fn fold_pick(line: &str, ids: &[String]) -> Option<(&'static str, String)> {
    let t = tokens(line);
    let has = |w: &[&str]| t.iter().any(|x| w.contains(&x.as_str()));
    let phrase = |p: &str| line.to_lowercase().contains(p);
    let named = ids.iter().find(|id| t.contains(id) || phrase(&format!("#{id}")) || phrase(&format!("item {id}")) || phrase(&format!("step {id}")));
    let ask = line.trim_end().ends_with('?') && has(&["what", "why", "how", "which", "when", "where", "who", "is", "does", "do", "can", "are", "did"]) && !phrase("also") && !has(&["instead", "stop"]);
    if ask {
        return Some(("answer", "the line is a question".into()));
    }
    if phrase("start over") || phrase("from scratch") || phrase("scrap the plan") || phrase("new plan") || phrase("forget the plan") || phrase("replace the plan") {
        return Some(("replace", "the line says to start the plan again".into()));
    }
    let go = ["go", "continue", "next", "proceed", "yes", "ok", "okay", "resume"];
    if t.len() <= 3 && (has(&go) || phrase("do it") || phrase("carry on") || phrase("go on")) {
        return Some(("answer", "the line says to go on with the plan as it is".into()));
    }
    if has(&["also", "too"]) || phrase("as well") || phrase("one more") || phrase("in addition") || phrase("another") {
        return Some(("add", "the line adds to what is planned".into()));
    }
    if has(&["instead", "rather", "actually", "stop", "skip", "drop", "cancel", "scratch"]) || phrase("no longer") || phrase("never mind") || phrase("don't") {
        return Some(("change", "the line takes something back or puts something else in its place".into()));
    }
    named.map(|id| ("change", format!("the line names the item {id}")))
}

/// `rule:fold`: the cheap cases of the ask point `fold`: a question is an answer; "start over" replaces; "also" adds; "instead",
/// "stop", or a line that names an item changes. Any other line is not this rule's to decide.
pub fn fold_rule(keeper: Keeper) -> impl Answerer {
    fn_rule("rule:fold", move |q| {
        let ids: Vec<String> = q.thread.as_deref().and_then(|t| keeper.plan(t)).map(|p| p.items.into_iter().map(|i| i.id).collect()).unwrap_or_default();
        fold_pick(&q.text, &ids).map_or_else(|| Err("the line says nothing of the plan in so many words".into()), |(id, why)| picked(id, why))
    })
}
