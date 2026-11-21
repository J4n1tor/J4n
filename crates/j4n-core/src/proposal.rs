//! Proposals: what J4n makes for itself, and applies only when the operator says yes (law 12).
//!
//! A [`Draft`] is plain data: a title, the evidence it rests on and the [`Step`]s — each one a write to one entry of the Table
//! in the operator's or the project's layer. A rule (`then = "propose"`), the command `:pack on`, a flow or the grow pack
//! make one; nothing is written to a file until [`Proposer::accept`]. Accepting plans every step against the files as they
//! are, applies them one by one through the journal (each is a [`Change`] whose cause is the proposal), and takes the ones it
//! did back if a later one is refused. [`Proposer::undo`] applies the inverse of those changes, newest first, across restarts.
//!
//! - The Log holds `proposal.made`, `proposal.accepted`, `proposal.declined` and `proposal.undone`; the fold [`Proposals`]
//!   is the list. A proposal with the same `key` as an open one only raises that one's evidence. One the operator declined
//!   returns only when its evidence has doubled; one that was accepted does not return (and one that was undone returns like a
//!   declined one: the operator has said what they think of it).
//! - A step that widens what J4n may do (a gate that allows, a pack that adds a connector) takes two presses: the first
//!   `accept` says what it widens and arms the proposal, the second applies it.
//! - What a model or a doer drafts goes through the author gate ([`author_vet`]): files only (never the session), and only kinds that
//!   draw, name or count — never a connector, a gate, a model, an enable, or a rule that starts work.

use crate::change::{Actor, Cause, Change, Where};
use crate::changes::{Changes, LogJournal};
use crate::fold::Fold;
use crate::log::{Ev, Log, New, actor_word};
use crate::table::{Op, Planned, Problem, Tables, Write};
use crate::value::{Key, Value};
use crate::write::{Applied, Journal, Writer};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};
use toml::Table;

pub const MADE: &str = "proposal.made";
pub const ACCEPTED: &str = "proposal.accepted";
pub const DECLINED: &str = "proposal.declined";
pub const UNDONE: &str = "proposal.undone";
/// The operator's own edit, applied at once: its id, its title and the ids of its changes. Never a proposal.
pub const EDITED: &str = "edit.applied";
/// The most proposals the fold keeps; the oldest settled ones go first.
pub const KEEP: usize = 300;
/// The most steps a proposal has.
pub const STEPS_MAX: usize = 8;
/// The kinds a model or a doer may draft: they draw, name or count, and none starts work, reaches out or lets anything through.
pub const AUTHOR_KINDS: [&str; 5] = ["view", "page", "command", "phrase", "fold"];

/// What one step does to one entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StepOp {
    Set(Table),
    Unset(Vec<String>),
    Remove,
}

/// One write: the entry `(kind, id)` of a layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub to: Where,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    pub kind: String,
    pub id: String,
    pub op: StepOp,
}

impl Step {
    fn write(&self, by: Actor, cause: Option<Cause>, at_ms: u64) -> Write {
        let op = match &self.op {
            StepOp::Set(t) => Op::Set(t.clone()),
            StepOp::Unset(f) => Op::Unset(f.clone()),
            StepOp::Remove => Op::Remove,
        };
        Write { to: self.to, file: self.file.clone(), kind: self.kind.clone(), id: self.id.clone(), op, by, cause, at_ms }
    }
}

/// What is proposed, before it has an id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Draft {
    /// The same thing proposed again has the same key.
    pub key: String,
    pub title: String,
    /// The evidence, in words (counts, never a doer's own text).
    pub why: String,
    /// How much evidence there is, as a count: a declined proposal returns when this has doubled.
    #[serde(default)]
    pub evidence: u64,
    pub steps: Vec<Step>,
    /// What the steps widen that the Table cannot tell (a pack that adds a rule or a gate): the operator is asked twice.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub widens: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fate {
    Open,
    /// Applied; the ids of the changes it made.
    Accepted(Vec<String>),
    /// Declined, with the evidence it had.
    Declined,
    Undone,
}

impl Fate {
    pub fn word(&self) -> &'static str {
        match self {
            Fate::Open => "open",
            Fate::Accepted(_) => "accepted",
            Fate::Declined => "declined",
            Fate::Undone => "undone",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub draft: Draft,
    pub by: Actor,
    pub at_ms: u64,
    /// What the steps widen, as planned when it was made.
    #[serde(default)]
    pub widens: Vec<String>,
    pub fate: Fate,
}

/// The fold `proposals`. A clone is a second handle on the same state.
#[derive(Debug, Clone, Default)]
pub struct Proposals(Arc<Mutex<Vec<Proposal>>>);

impl Proposals {
    pub fn new() -> Proposals {
        Proposals::default()
    }

    fn all(&self) -> MutexGuard<'_, Vec<Proposal>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// All of them as rows for a view, newest first: what J4n drafted, and what the operator was asked twice for. An operator's own
    /// line that an older Log holds as a proposal accepted at once is an edit, not a row of Grow.
    pub fn rows(&self) -> Vec<Value> {
        self.list().iter().filter(|p| shown(p)).map(row).collect()
    }

    pub fn get(&self, id: &str) -> Option<Proposal> {
        self.all().iter().find(|p| p.id == id).cloned()
    }

    /// The open proposals, newest first.
    pub fn open(&self) -> Vec<Proposal> {
        self.all().iter().rev().filter(|p| p.fate == Fate::Open).cloned().collect()
    }

    /// The newest first, whatever became of them.
    pub fn list(&self) -> Vec<Proposal> {
        self.all().iter().rev().cloned().collect()
    }

    /// The proposal with this id, or the only open one when `id` is empty.
    pub fn pick(&self, id: &str) -> Result<Proposal, String> {
        if id.is_empty() {
            let open = self.open();
            return match open.len() {
                1 => Ok(open[0].clone()),
                0 => Err("there is no open proposal".into()),
                n => Err(format!("{n} proposals are open; name one ({})", open.iter().map(|p| p.id.as_str()).collect::<Vec<_>>().join(", "))),
            };
        }
        self.get(id).ok_or_else(|| format!("there is no proposal {id}"))
    }

    /// Whether a draft would be taken: not when the same one is open with as much evidence, nor accepted, nor declined (or
    /// undone) with more than half as much as this has.
    pub fn admits(&self, d: &Draft) -> bool {
        admit(&self.all(), d)
    }
}

/// Whether a proposal is one of Grow's: not an operator's own edit (made by the operator, widening nothing, and settled).
pub fn shown(p: &Proposal) -> bool {
    !(p.by == Actor::Operator && p.widens.is_empty() && p.draft.widens.is_empty() && p.fate != Fate::Open)
}

fn admit(all: &[Proposal], d: &Draft) -> bool {
    match all.iter().rev().find(|p| p.draft.key == d.key) {
        None => true,
        Some(p) => match p.fate {
            Fate::Open => d.evidence > p.draft.evidence,
            Fate::Accepted(_) => false,
            Fate::Declined | Fate::Undone => d.evidence > p.draft.evidence && d.evidence >= p.draft.evidence.saturating_mul(2),
        },
    }
}

fn row(p: &Proposal) -> Value {
    let text = |s: &str| Value::text(s);
    let tone = match p.fate {
        Fate::Open => "warn",
        Fate::Accepted(_) => "good",
        _ => "dim",
    };
    Value::Map(BTreeMap::from([
        ("id".to_string(), text(&p.id)),
        ("title".into(), text(&p.draft.title)),
        ("why".into(), text(&p.draft.why)),
        ("evidence".into(), Value::num(p.draft.evidence as f64)),
        ("state".into(), text(p.fate.word())),
        ("tone".into(), text(tone)),
        ("by".into(), text(&actor_word(&p.by))),
        ("steps".into(), Value::num(p.draft.steps.len() as f64)),
        ("widens".into(), text(&p.widens.join(", "))),
        ("at_ms".into(), Value::num(p.at_ms as f64)),
    ]))
}

impl Fold for Proposals {
    fn id(&self) -> &str {
        "proposals"
    }

    fn version(&self) -> u64 {
        1
    }

    fn wants(&self, kind: &str) -> bool {
        matches!(kind, MADE | ACCEPTED | DECLINED | UNDONE)
    }

    fn apply(&mut self, ev: &Ev) {
        let mut all = self.all();
        let id = ev.data.get("id").and_then(Json::as_str).unwrap_or_default().to_string();
        match ev.kind.as_str() {
            MADE => {
                let Some(draft) = ev.data.get("draft").and_then(|d| serde_json::from_value::<Draft>(d.clone()).ok()) else { return };
                let widens = ev.data.get("widens").and_then(|w| serde_json::from_value::<Vec<String>>(w.clone()).ok()).unwrap_or_default();
                if !admit(&all, &draft) {
                    return;
                }
                let id = format!("p{}", ev.seq);
                if let Some(open) = all.iter_mut().find(|p| p.fate == Fate::Open && p.draft.key == draft.key) {
                    open.draft.evidence = open.draft.evidence.max(draft.evidence);
                    open.draft.why = draft.why;
                    return;
                }
                all.push(Proposal { id, draft, by: ev.by.clone(), at_ms: ev.at_ms, widens, fate: Fate::Open });
                while all.len() > KEEP {
                    let at = all.iter().position(|p| p.fate != Fate::Open).unwrap_or(0);
                    all.remove(at);
                }
            }
            kind => {
                let Some(p) = all.iter_mut().find(|p| p.id == id) else { return };
                p.fate = match kind {
                    ACCEPTED if p.fate == Fate::Open => {
                        let ids = ev.data.get("changes").and_then(|c| serde_json::from_value::<Vec<String>>(c.clone()).ok()).unwrap_or_default();
                        Fate::Accepted(ids)
                    }
                    DECLINED if p.fate == Fate::Open => Fate::Declined,
                    UNDONE if matches!(p.fate, Fate::Accepted(_)) => Fate::Undone,
                    _ => return,
                };
            }
        }
    }

    fn read(&self, path: &[Key], _now_ms: u64) -> Value {
        Value::List(self.rows()).at(path)
    }

    fn save(&self) -> Json {
        serde_json::to_value(&*self.all()).unwrap_or_default()
    }

    fn load(&mut self, state: Json) -> Result<(), String> {
        *self.all() = serde_json::from_value(state).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn reset(&mut self) {
        self.all().clear();
    }
}

/// What the author gate lets a model or a doer draft.
pub fn author_vet(d: &Draft) -> Result<(), String> {
    for s in &d.steps {
        if s.to == Where::Session {
            return Err("it may write a file, not the session".into());
        }
        if !AUTHOR_KINDS.contains(&s.kind.as_str()) {
            return Err(format!("`{}` is not a kind it may draft ({})", s.kind, AUTHOR_KINDS.join(", ")));
        }
    }
    Ok(())
}

/// How the proposals reach the files: the tables as they are, and the writer. A test can have its own.
pub trait Hands: Send + Sync {
    /// What the write would change, checked against the files as they are now.
    fn plan(&self, w: &Write) -> Result<Planned, Problem>;
    /// Make it, through the journal.
    fn apply(&self, p: &Planned, journal: &mut dyn Journal) -> Result<Applied, Problem>;
    /// Apply a change as it stands (an undo is one).
    fn redo(&self, c: &Change, journal: &mut dyn Journal) -> Result<Applied, Problem>;
}

/// The files: `load` reads the tables again each time, so a step sees what the one before it wrote.
pub struct Plant {
    pub load: Box<dyn Fn() -> Tables + Send + Sync>,
    pub writer: Writer,
}

impl Hands for Plant {
    fn plan(&self, w: &Write) -> Result<Planned, Problem> {
        (self.load)().plan(w)
    }

    fn apply(&self, p: &Planned, journal: &mut dyn Journal) -> Result<Applied, Problem> {
        self.writer.apply(&(self.load)(), &p.change, p.file_hash.as_deref(), journal)
    }

    fn redo(&self, c: &Change, journal: &mut dyn Journal) -> Result<Applied, Problem> {
        self.writer.apply(&(self.load)(), c, None, journal)
    }
}

/// What `accept` came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Accepted {
    Done(Vec<Change>),
    /// It widens what J4n may do: it is armed, and the next `accept` applies it.
    Confirm(Vec<String>),
}

/// What an operator's edit came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Edited {
    /// Applied at once: the edit's id (`e3`, what `:undo e3` takes back) and its changes.
    Done { id: String, changes: Vec<Change> },
    /// It widens what J4n may do: it is a proposal, and `:accept` twice applies it.
    Proposed { id: String, widens: Vec<String> },
}

/// Makes, applies and takes back proposals.
pub struct Proposer {
    log: Log,
    proposals: Proposals,
    changes: Changes,
    hands: Arc<dyn Hands>,
    clock: Arc<dyn Fn() -> u64 + Send + Sync>,
    armed: Mutex<Option<String>>,
    /// One edit is numbered and applied at a time.
    editing: Mutex<()>,
    /// Told after every edit, accept and undo that changed the files: the front reads the settings again.
    changed: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

fn problem(at: &str, msg: impl Into<String>) -> Problem {
    Problem::refused(at.to_string(), msg)
}

impl Proposer {
    pub fn new(log: Log, proposals: Proposals, changes: Changes, hands: Arc<dyn Hands>, clock: Arc<dyn Fn() -> u64 + Send + Sync>) -> Proposer {
        Proposer { log, proposals, changes, hands, clock, armed: Mutex::new(None), editing: Mutex::new(()), changed: Mutex::new(None) }
    }

    /// Who is told after every edit, accept and undo that changed the files.
    pub fn on_change(&self, f: Arc<dyn Fn() + Send + Sync>) {
        *self.changed.lock().unwrap_or_else(|e| e.into_inner()) = Some(f);
    }

    fn told(&self) {
        let f = self.changed.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(f) = f {
            f();
        }
    }

    pub fn proposals(&self) -> &Proposals {
        &self.proposals
    }

    /// Wait until what was written is in the folds, so a read sees it.
    pub fn log_sync(&self) {
        self.log.sync();
    }

    /// The changes in effect, newest first, as rows for a view.
    pub fn change_rows(&self) -> Vec<Value> {
        match self.changes.read(&[], 0) {
            Value::List(rows) => rows,
            _ => Vec::new(),
        }
    }

    /// Whether every step of `d` would be taken by the Table as it is now (the kind exists, the schema allows the fields), and
    /// what the steps widen. Nothing is written.
    pub fn vet(&self, d: &Draft) -> Result<Vec<String>, Problem> {
        let now = (self.clock)();
        let mut widens = d.widens.clone();
        for s in &d.steps {
            let planned = self.hands.plan(&s.write(Actor::Operator, None, now))?;
            widens.extend(planned.widens);
        }
        widens.sort();
        widens.dedup();
        Ok(widens)
    }

    /// Make a proposal: its id, or `Ok(None)` when it would not be taken (see [`Proposals::admits`]). A draft that cannot be planned — the
    /// kind does not exist, the schema refuses it — is refused here, not at the Tab; and what a model or a doer drafts must pass
    /// the author gate.
    pub fn make(&self, d: Draft, by: Actor, cause: Option<u64>) -> Result<Option<String>, Problem> {
        let at = format!("proposal {}", d.key);
        if d.steps.is_empty() || d.steps.len() > STEPS_MAX {
            return Err(problem(&at, format!("a proposal has one to {STEPS_MAX} steps")));
        }
        if d.title.trim().is_empty() || d.key.trim().is_empty() {
            return Err(problem(&at, "a proposal has a key and a title"));
        }
        if matches!(by, Actor::Doer(_) | Actor::Model(_)) {
            author_vet(&d).map_err(|m| problem(&at, m))?;
        }
        self.log.sync();
        if !self.proposals.admits(&d) {
            return Ok(None);
        }
        let open = self.proposals.open().into_iter().find(|p| p.draft.key == d.key).map(|p| p.id);
        let widens = self.vet(&d)?;
        let mut ev = New::of(MADE, by, json!({ "draft": d, "widens": widens }));
        ev.cause = cause;
        let ev = self.log.append(ev)?;
        Ok(Some(open.unwrap_or(format!("p{}", ev.seq))))
    }

    fn armed(&self) -> MutexGuard<'_, Option<String>> {
        self.armed.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The operator says yes. The steps are applied in order, each against the file as the one before left it; if one is refused
    /// the ones already made are taken back and the proposal stays open.
    pub fn accept(&self, id: &str) -> Result<Accepted, Problem> {
        let at = format!("proposal {id}");
        self.log.sync();
        let p = self.proposals.pick(id).map_err(|m| problem(&at, m))?;
        if p.fate != Fate::Open {
            return Err(problem(&at, format!("it is {} already", p.fate.word())));
        }
        let now = (self.clock)();
        let cause = Some(Cause::Proposal(p.id.clone()));
        let mut widens = p.draft.widens.clone();
        for s in &p.draft.steps {
            widens.extend(self.hands.plan(&s.write(Actor::Operator, cause.clone(), now))?.widens);
        }
        widens.sort();
        widens.dedup();
        if !widens.is_empty() && self.armed().as_deref() != Some(p.id.as_str()) {
            *self.armed() = Some(p.id.clone());
            return Ok(Accepted::Confirm(widens));
        }
        *self.armed() = None;
        let mut made: Vec<Change> = Vec::new();
        let mut journal = LogJournal { log: &self.log };
        for s in &p.draft.steps {
            let step = self.hands.plan(&s.write(Actor::Operator, cause.clone(), (self.clock)())).and_then(|planned| {
                self.hands.apply(&planned, &mut journal)?;
                Ok(planned.change)
            });
            match step {
                Ok(c) => made.push(c),
                Err(e) => {
                    for c in made.iter().rev() {
                        let _ = self.hands.redo(&c.inverse(Actor::Operator, (self.clock)()), &mut journal);
                    }
                    return Err(problem(&at, format!("{} — nothing was changed", e.msg)));
                }
            }
        }
        let ids: Vec<&str> = made.iter().map(|c| c.id.as_str()).collect();
        self.log.append(New::of(ACCEPTED, Actor::Operator, json!({ "id": p.id, "changes": ids })))?;
        self.log.sync();
        self.told();
        Ok(Accepted::Done(made))
    }

    /// The operator's own edit: the steps are applied at once through the journal, each a Change whose cause is the edit,
    /// and `edit.applied` says so in the Log; no proposal is made and Grow shows nothing of it. An edit that widens what J4n may do is
    /// the one exception: it is made a proposal, as before, and `accept` twice applies it.
    pub fn edit(&self, d: Draft) -> Result<Edited, Problem> {
        self.edit_as(d, false)
    }

    /// The operator's edit after the card that named what it widens said yes: applied as any edit, no proposal, and
    /// `edit.applied` keeps what it widened.
    pub fn edit_seen(&self, d: Draft) -> Result<Edited, Problem> {
        self.edit_as(d, true)
    }

    fn edit_as(&self, d: Draft, seen: bool) -> Result<Edited, Problem> {
        let at = format!("edit {}", d.key);
        if d.steps.is_empty() || d.steps.len() > STEPS_MAX {
            return Err(problem(&at, format!("an edit has one to {STEPS_MAX} steps")));
        }
        let _one = self.editing.lock().unwrap_or_else(|e| e.into_inner());
        self.log.sync();
        let widens = self.vet(&d)?;
        if !widens.is_empty() && !seen {
            // the key carries the Log's head: the operator's line is always a new one, whatever became of an earlier one
            let draft = Draft { key: format!("{}:{}", d.key, self.log.head()), ..d };
            let id = self.make(draft, Actor::Operator, None)?.ok_or_else(|| problem(&at, "it was proposed already"))?;
            return Ok(Edited::Proposed { id, widens });
        }
        let id = format!("e{}", self.next_edit());
        let cause = Some(Cause::Edit(id.clone()));
        let mut made: Vec<Change> = Vec::new();
        let mut journal = LogJournal { log: &self.log };
        for s in &d.steps {
            let step = self.hands.plan(&s.write(Actor::Operator, cause.clone(), (self.clock)())).and_then(|planned| {
                self.hands.apply(&planned, &mut journal)?;
                Ok(planned.change)
            });
            match step {
                Ok(c) => made.push(c),
                Err(e) => {
                    for c in made.iter().rev() {
                        let _ = self.hands.redo(&c.inverse(Actor::Operator, (self.clock)()), &mut journal);
                    }
                    return Err(problem(&at, format!("{} — nothing was changed", e.msg)));
                }
            }
        }
        let ids: Vec<&str> = made.iter().map(|c| c.id.as_str()).collect();
        let what: Vec<String> = d.steps.iter().map(|s| format!("{} {}", s.kind, s.id)).collect();
        let mut data = json!({ "id": id, "title": d.title, "what": what, "changes": ids });
        if !widens.is_empty() {
            data["widens"] = json!(widens);
        }
        self.log.append(New::of(EDITED, Actor::Operator, data))?;
        self.log.sync();
        drop(_one);
        self.told();
        Ok(Edited::Done { id, changes: made })
    }

    /// The number of the next edit: one more than the highest the undo stack knows.
    fn next_edit(&self) -> u64 {
        self.changes.edits().into_iter().max().unwrap_or(0) + 1
    }

    pub fn decline(&self, id: &str) -> Result<Proposal, Problem> {
        let at = format!("proposal {id}");
        self.log.sync();
        let p = self.proposals.pick(id).map_err(|m| problem(&at, m))?;
        if p.fate != Fate::Open {
            return Err(problem(&at, format!("it is {} already", p.fate.word())));
        }
        self.log.append(New::of(DECLINED, Actor::Operator, json!({ "id": p.id })))?;
        Ok(p)
    }

    /// Take back a proposal that was accepted (its changes, newest first), or, with no id, the newest change in effect.
    pub fn undo(&self, id: &str) -> Result<Vec<Change>, Problem> {
        let at = if id.is_empty() { "undo".to_string() } else { format!("proposal {id}") };
        self.log.sync();
        let mut journal = LogJournal { log: &self.log };
        let (p, wanted): (Option<Proposal>, Vec<Change>) = if id.is_empty() {
            let active = self.changes.active(0);
            let Some(last) = active.into_iter().next() else { return Err(problem(&at, "there is nothing to undo")) };
            let p = match &last.cause {
                Some(Cause::Proposal(pid)) => self.proposals.get(pid),
                _ => None,
            };
            match (&p, &last.cause) {
                (Some(p), _) => (Some(p.clone()), self.of(p)),
                (None, Some(Cause::Edit(eid))) => (None, self.edit_of(eid)),
                (None, _) => (None, vec![last]),
            }
        } else if is_edit_id(id) {
            (None, self.edit_of(id))
        } else if !id.starts_with('p') && let Some(c) = self.changes.active(0).into_iter().find(|c| c.id == id) {
            // a change named by its id (a row of Log › Changes): the edit it is of goes back whole
            match &c.cause {
                Some(Cause::Edit(eid)) => (None, self.edit_of(eid)),
                Some(Cause::Proposal(pid)) => match self.proposals.get(pid) {
                    Some(p) => (Some(p.clone()), self.of(&p)),
                    None => (None, vec![c]),
                },
                _ => (None, vec![c]),
            }
        } else {
            let p = self.proposals.pick(id).map_err(|m| problem(&at, m))?;
            if !matches!(p.fate, Fate::Accepted(_)) {
                return Err(problem(&at, format!("it is {}, not accepted", p.fate.word())));
            }
            (Some(p.clone()), self.of(&p))
        };
        if wanted.is_empty() {
            return Err(problem(&at, "none of its changes is in effect"));
        }
        let mut undone = Vec::new();
        for c in &wanted {
            let inverse = c.inverse(Actor::Operator, (self.clock)());
            self.hands.redo(&inverse, &mut journal).map_err(|e| problem(&at, format!("{} — {} change(s) were taken back", e.msg, undone.len())))?;
            undone.push(inverse);
        }
        if let Some(p) = p {
            self.log.append(New::of(UNDONE, Actor::Operator, json!({ "id": p.id })))?;
        }
        self.log.sync();
        self.told();
        Ok(undone)
    }

    /// The newest of the operator's edits still in effect on the entry `(kind, id)`, by its id (`e3`): what `u` on the entry's row takes back.
    pub fn last_edit(&self, kind: &str, id: &str) -> Option<String> {
        self.changes.active(0).into_iter().find(|c| c.target.kind == kind && c.target.id == id).and_then(|c| match c.cause {
            Some(Cause::Edit(e)) => Some(e),
            _ => None,
        })
    }

    /// The changes of an edit that are still in effect, newest first.
    fn edit_of(&self, id: &str) -> Vec<Change> {
        self.changes.active(0).into_iter().filter(|c| matches!(&c.cause, Some(Cause::Edit(e)) if e == id)).collect()
    }

    /// The changes of an accepted proposal that are still in effect, newest first.
    fn of(&self, p: &Proposal) -> Vec<Change> {
        let Fate::Accepted(ids) = &p.fate else { return Vec::new() };
        self.changes.active(0).into_iter().filter(|c| ids.contains(&c.id)).collect()
    }
}

/// Whether `id` names an edit (`e3`), not a proposal (`p3`) or a change (sixteen hex digits).
pub fn is_edit_id(id: &str) -> bool {
    id.strip_prefix('e').is_some_and(|n| !n.is_empty() && n.len() < 12 && n.bytes().all(|b| b.is_ascii_digit()))
}
