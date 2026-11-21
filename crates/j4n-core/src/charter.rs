//! A charter: the operator's order to a chat, made concrete, and the rule that answers the approvals of that chat's tasks inside it. The operator signs it on a card
//! that shows its scope in full; from then until its end it says yes to an approval that falls inside its `paths`, its `may`, its budget and its time, and the
//! question goes to the operator for everything else.
//!
//! What makes it safe is where it answers and what it never answers:
//!
//! - **Signed is a fact of the Log, not of a file.** The charter that counts is the one in a `charter.signed` event written by the operator, with the content the
//!   card showed; a row of the kind `charter` in a file means nothing, so what can write a file cannot sign. A change to a signed charter is a new signing, and an
//!   event written by anyone but the operator is not read.
//! - **It answers after the chain.** The question has been walked first: a block of the Gate, a floor and a grant are answered as ever, and a charter is asked
//!   only for the approval that would otherwise wait for the operator, while the operator is there to wait (a run with no one to ask denies).
//! - **It says yes only where the Gate has no say.** A rule that asks, a path outside the project, a task that read untrusted content, a line that is not one plain
//!   command, a destructive step and any verb a charter may not name are left to the operator.
//! - **Its tree is what the core wrote**: the thread of the asking task, and the threads it was started from.

use crate::answer::{Kind, Question, chain::Outcome};
use crate::ask::Answer;
use crate::change::Actor;
use crate::fold::Fold;
use crate::gate::{Action, Gate, Verb, Verdict, check::DEFAULT};
use crate::log::Ev;
use crate::table::Kind as TableKind;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// The event of a signing and of an end.
pub const SIGNED: &str = "charter.signed";
pub const ENDED: &str = "charter.ended";

/// What a charter may name in `may`: the verbs it says yes to, and `checks`, the project's own checks.
pub const MAY: [&str; 5] = ["read", "write", "shell", "spawn", "checks"];

/// An entry of kind `charter`, and the content a signing holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Charter {
    pub id: String,
    /// What the work is, in the operator's words first.
    pub goal: String,
    #[serde(default)]
    pub watch: String,
    /// How often the check `watch` names is run, in minutes, while J4n is open.
    #[serde(default = "every_default")]
    pub watch_every_min: u64,
    /// Where work may read and write: folders or files inside the project.
    pub paths: Vec<String>,
    pub may: Vec<String>,
    /// The doers a `spawn` may name.
    #[serde(default)]
    pub doers: Vec<String>,
    /// What the whole tree may spend, in dollars.
    pub usd: f64,
    /// The end, in milliseconds since the epoch; none: the operator's word ends it.
    #[serde(default)]
    pub until_ms: Option<u64>,
    /// The thread whose tasks, and those started from it, the charter answers for.
    pub thread: String,
    /// The operator's own lines, word for word.
    #[serde(default)]
    pub verbatim: Vec<String>,
}

impl TableKind for Charter {
    const NAME: &'static str = "charter";
}

fn every_default() -> u64 {
    10
}

fn word(w: &str) -> bool {
    !w.is_empty() && w.len() <= 40 && w.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'))
}

/// A path inside the project, as written in a charter or found in an action: relative, no `..`, no `~`, no trailing slash; the project's own folder is `.`.
pub fn clean(p: &str, site: Option<&str>) -> Option<String> {
    let p = p.trim();
    let rel = match site {
        Some(s) if p.starts_with('/') => p.strip_prefix(s.trim_end_matches('/'))?.trim_start_matches('/'),
        _ if p.starts_with(['/', '~']) => return None,
        _ => p,
    };
    if p.contains('\0') || rel.split('/').any(|c| c == "..") {
        return None;
    }
    let rel = rel.trim_start_matches("./").trim_end_matches('/');
    Some(if rel.is_empty() { ".".into() } else { rel.into() })
}

/// `2 h 5 min`, `40 min`: how long a charter has left, as the card says it.
fn span(ms: u64) -> String {
    let min = ms.div_ceil(60_000);
    if min >= 60 { format!("{} h {} min", min / 60, min % 60) } else { format!("{min} min") }
}

fn under(path: &str, root: &str) -> bool {
    root == "." || path == root || path.strip_prefix(root).is_some_and(|r| r.starts_with('/'))
}

/// Whether two paths meet: the same one, or one inside the other.
fn meets(a: &str, b: &str) -> bool {
    under(a, b) || under(b, a)
}

impl Charter {
    /// Whether it reads as a charter: a word for an id, a goal, paths inside the project, a `may` the charter may say, a budget, and an end that has not passed.
    pub fn check(&self, now_ms: u64) -> Result<(), String> {
        let bad = |what: String| Err(format!("charter `{}`: {what}", self.id));
        if !word(&self.id) || !word(&self.thread) {
            return bad("the id and the thread are lowercase words".into());
        }
        if self.goal.trim().is_empty() {
            return bad("it has no goal".into());
        }
        if self.paths.is_empty() {
            return bad("it names no path, and a charter with no path covers nothing".into());
        }
        if let Some(p) = self.paths.iter().find(|p| clean(p, None).is_none()) {
            return bad(format!("`{p}` is not a path inside the project"));
        }
        if self.may.is_empty() {
            return bad("it says yes to nothing".into());
        }
        if let Some(m) = self.may.iter().find(|m| !MAY.contains(&m.as_str())) {
            return bad(format!("`{m}` is not something a charter says yes to ({})", MAY.join(", ")));
        }
        if !(self.usd.is_finite() && self.usd > 0.0) {
            return bad("the budget is a number of dollars above zero".into());
        }
        if self.until_ms.is_some_and(|t| t <= now_ms) {
            return bad("its end has passed".into());
        }
        if !self.watch.trim().is_empty() && (!word(self.watch.trim()) || !(1..=1440).contains(&self.watch_every_min)) {
            return bad("what it watches is the name of one check, run every 1 to 1440 minutes".into());
        }
        if self.may.iter().any(|m| m == "spawn") && self.doers.is_empty() {
            return bad("it may start a doer and names none".into());
        }
        Ok(())
    }

    fn paths(&self) -> Vec<String> {
        self.paths.iter().filter_map(|p| clean(p, None)).collect()
    }

    /// The first path of `other` that meets one of this charter's: the paths two charters may not share.
    pub fn meets(&self, other: &Charter) -> Option<(String, String)> {
        let mine = self.paths();
        other.paths().into_iter().find_map(|o| mine.iter().find(|m| meets(m, &o)).map(|m| (m.clone(), o.clone())))
    }

    /// The scope as the operator reads it, one line each, made by code from the fields: what the card leads with, before any word of the draft's own.
    pub fn scope(&self, now_ms: u64) -> Vec<String> {
        let end = match self.until_ms {
            Some(t) => format!("for {}, then it ends", span(t.saturating_sub(now_ms))),
            None => "until you end it".to_string(),
        };
        let mut lines = vec![
            format!("answers yes, without asking you, to what {} does inside: {}", self.thread, self.paths.join(", ")),
            format!("says yes to: {}", self.may.join(", ")),
            format!("spends at most ${:.2}, {end}", self.usd),
        ];
        if !self.doers.is_empty() {
            lines.push(format!("may start: {}", self.doers.join(", ")));
        }
        if !self.watch.trim().is_empty() {
            lines.push(format!("every {} min, while J4n is open, runs the check `{}` and sends Main a request when it fails", self.watch_every_min, self.watch.trim()));
        }
        lines.push("never answers a block, a path outside the project, a destructive step, a line that is not one plain command, or a task that read from outside; those ask you".into());
        lines
    }

    /// The row the draft writes: the record of what the card showed. It signs nothing.
    pub fn row(&self) -> toml::Table {
        let mut row = toml::Table::try_from(self).unwrap_or_default();
        row.remove("id");
        row
    }

    /// The charter a row holds: the entry's id and its fields.
    pub fn of_row(id: &str, row: &toml::Table) -> Result<Charter, String> {
        let mut row = row.clone();
        row.insert("id".into(), toml::Value::String(id.into()));
        row.try_into().map_err(|e: toml::de::Error| format!("the charter reads wrong: {e}"))
    }

    /// The draft the operator signs: one step writing the row, which widens what J4n may do, so the card is pressed twice. The title and the lines are made here from
    /// the fields, never taken from the drafter's words.
    pub fn draft(&self, now_ms: u64) -> Result<crate::proposal::Draft, String> {
        self.check(now_ms)?;
        let step = crate::proposal::Step { to: crate::change::Where::User, file: Some("charters.toml".into()), kind: Charter::NAME.into(), id: self.id.clone(), op: crate::proposal::StepOp::Set(self.row()) };
        let scope = self.scope(now_ms);
        Ok(crate::proposal::Draft {
            // a charter that was changed is another proposal: the same id with other words is not the one that was accepted
            key: format!("charter:{}:{}", self.id, crate::text::short_hash(serde_json::to_vec(self).unwrap_or_default())),
            title: format!("Sign charter {}: yes inside {} · ${:.2}", self.id, self.paths.join(", "), self.usd),
            why: format!("{} · the goal: {}", scope.join(" · "), crate::text::line(&self.goal, 160)),
            evidence: 1,
            steps: vec![step],
            // every line of the scope, and the goal in the operator's words, are on the card before the second press
            widens: scope.into_iter().chain([format!("goal: {}", crate::text::line(&self.goal, 160))]).collect(),
        })
    }
}

/// A charter that was signed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signed {
    pub charter: Charter,
    /// The proposal the operator accepted.
    pub proposal: String,
    pub at_ms: u64,
    /// When and why it ended: the operator's word, an undone signing, or a signing in its place.
    #[serde(default)]
    pub ended: Option<(u64, String)>,
}

impl Signed {
    /// It answers now: not ended, and its end has not passed.
    fn live(&self, now_ms: u64) -> bool {
        self.ended.is_none() && self.charter.until_ms.is_none_or(|t| now_ms < t)
    }
}

/// The threads a task is for, nearest first.
pub type Tree = dyn Fn(&str) -> Vec<String> + Send + Sync;

/// What the rule asks of the program it lives in: the Gate, the clock, the threads a task came from, what a thread's tree has spent, and the project's checks.
pub struct Hooks {
    pub gate: Gate,
    pub now: Box<dyn Fn() -> u64 + Send + Sync>,
    /// The thread of the task and the threads it was started from, nearest first: what the core wrote.
    pub tree: Box<Tree>,
    /// What the tasks in a thread's tree have spent, in dollars.
    pub spent: Box<dyn Fn(&str) -> f64 + Send + Sync>,
    pub checks: Box<dyn Fn() -> Vec<String> + Send + Sync>,
}

/// The fold `charters` and the rule. A clone is a second handle on the same state: one goes to the fold host, one to the engine.
#[derive(Clone, Default)]
pub struct Charters {
    signed: Arc<Mutex<Vec<Signed>>>,
    on: Arc<AtomicBool>,
    hooks: Arc<OnceLock<Hooks>>,
}

impl Charters {
    pub fn new() -> Charters {
        Charters::default()
    }

    /// The program the rule lives in, given once.
    pub fn attach(&self, hooks: Hooks) {
        let _ = self.hooks.set(hooks);
    }

    /// `crew.on`, read live: off, nothing is answered.
    pub fn set_on(&self, on: bool) {
        self.on.store(on, Ordering::SeqCst);
    }

    fn all(&self) -> std::sync::MutexGuard<'_, Vec<Signed>> {
        self.signed.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Every charter that was signed, ended ones too.
    pub fn list(&self) -> Vec<Signed> {
        self.all().clone()
    }

    /// The charters that answer at `now_ms`.
    pub fn live(&self, now_ms: u64) -> Vec<Signed> {
        self.all().iter().filter(|s| s.live(now_ms)).cloned().collect()
    }

    /// Whether a charter with this id was signed and has not ended.
    pub fn holds(&self, id: &str, now_ms: u64) -> bool {
        self.all().iter().any(|s| s.charter.id == id && s.live(now_ms))
    }

    /// What the tasks of the tree of `thread` have spent, in dollars; unknown (no program to ask) is as good as everything.
    pub fn spent(&self, thread: &str) -> f64 {
        self.hooks.get().map_or(f64::INFINITY, |h| (h.spent)(thread))
    }

    /// The first signed charter that shares a path with `c` (a charter of the same id is the one being changed, and does not count).
    pub fn clash(&self, c: &Charter, now_ms: u64) -> Option<(String, String, String)> {
        self.live(now_ms).iter().filter(|s| s.charter.id != c.id).find_map(|s| c.meets(&s.charter).map(|(a, b)| (s.charter.id.clone(), a, b)))
    }

    /// The approval `q`, answered yes, when a signed charter covers it; none when the operator is to be asked.
    pub fn offer(&self, q: &Question) -> Option<Outcome> {
        if !self.on.load(Ordering::SeqCst) {
            return None;
        }
        let (hooks, act) = (self.hooks.get()?, q.act.as_ref()?);
        let now = (hooks.now)();
        // what the Gate has no say in: a block was answered before, a rule that asks is the operator's, and a path outside, or a task that read from outside, is too
        if !matches!(hooks.gate.check(act), Verdict::Allow { by } if by == DEFAULT) || hooks.gate.outside(act) || hooks.gate.tainted(&q.asker).is_some() {
            return None;
        }
        let tree = (hooks.tree)(&q.asker);
        let checks = (hooks.checks)();
        // the first charter of the tree that covers it: a thread may hold more than one, each with paths of its own
        self.live(now).iter().filter(|s| tree.contains(&s.charter.thread) && (hooks.spent)(&s.charter.thread) < s.charter.usd).find_map(|s| {
            let said = covers(&s.charter, act, &checks)?;
            let why = format!("charter {} (signed by the operator, {}): {said}", s.charter.id, s.proposal);
            Some(Outcome { answer: Answer::Yes, by: format!("rule:charter-{}", s.charter.id), kind: Kind::Rule, conf: Some(1.0), why, pair: Vec::new() })
        })
    }
}

/// What a charter says yes to, in words, or none: the verb is in `may`, the path is inside `paths`, the line is one plain command, and nothing destructive.
fn covers(c: &Charter, act: &Action, checks: &[String]) -> Option<String> {
    let may = |m: &str| c.may.iter().any(|x| x == m);
    let site = act.site.as_deref();
    match act.verb {
        Verb::Read | Verb::Write if may(act.verb.word()) => {
            if act.verb == Verb::Write && act.inside != Some(true) {
                return None;
            }
            let path = clean(&act.target, site)?;
            let root = c.paths().into_iter().find(|r| under(&path, r))?;
            Some(format!("{} {path} is inside {root}", act.verb.word()))
        }
        Verb::Shell if may("shell") || may("checks") => {
            let line = act.target.trim();
            let d = crate::gate::danger::danger(line);
            if crate::gate::grant::grantable(act).is_err() || d.outside || d.class == "unknown" || destructive(line, &c.verbatim) {
                return None;
            }
            let check = checks.iter().any(|k| k == line);
            let plain = may("shell") && matches!(d.class.as_str(), "safe" | "read");
            (check && may("checks") || plain).then(|| format!("`{}` is {}", crate::text::line(line, 60), if check { "one of the project's checks" } else { "a plain command that only reads" }))
        }
        Verb::Spawn if may("spawn") && c.doers.contains(&act.target) => Some(format!("start {}", act.target)),
        _ => None,
    }
}

/// A destructive step the operator's own lines do not hold: such a step came from text that was quoted, and is never answered here.
fn destructive(line: &str, verbatim: &[String]) -> bool {
    crate::front::clamp::quoted_destruction(line, &verbatim.join(" ")).is_some()
}

impl Fold for Charters {
    fn id(&self) -> &str {
        "charters"
    }

    fn version(&self) -> u64 {
        1
    }

    fn wants(&self, kind: &str) -> bool {
        matches!(kind, SIGNED | ENDED | crate::proposal::UNDONE)
    }

    fn apply(&mut self, ev: &Ev) {
        // only the operator signs, ends and undoes: what any other wrote is not read, as a grant another wrote is not
        if ev.by != Actor::Operator {
            return;
        }
        let mut all = self.all();
        let text = |k: &str| ev.data.get(k).and_then(Json::as_str).unwrap_or_default().to_string();
        match ev.kind.as_str() {
            SIGNED => {
                let Some(charter) = ev.data.get("charter").and_then(|c| serde_json::from_value::<Charter>(c.clone()).ok()) else { return };
                if charter.check(ev.at_ms).is_err() {
                    return;
                }
                // a second charter may not take a path a live one holds; the same id is the operator changing it, and the old signing ends
                if all.iter().any(|s| s.charter.id != charter.id && s.live(ev.at_ms) && charter.meets(&s.charter).is_some()) {
                    return;
                }
                for s in all.iter_mut().filter(|s| s.charter.id == charter.id && s.ended.is_none()) {
                    s.ended = Some((ev.at_ms, "signed again".into()));
                }
                all.push(Signed { charter, proposal: text("proposal"), at_ms: ev.at_ms, ended: None });
            }
            ENDED => {
                let id = text("id");
                for s in all.iter_mut().filter(|s| s.charter.id == id && s.ended.is_none()) {
                    s.ended = Some((ev.at_ms, "the operator ended it".into()));
                }
            }
            // taking back the accept that made the signing ends it
            _ => {
                let p = text("id");
                for s in all.iter_mut().filter(|s| s.proposal == p && s.ended.is_none()) {
                    s.ended = Some((ev.at_ms, "the signing was undone".into()));
                }
            }
        }
    }

    fn read(&self, _path: &[crate::value::Key], _now_ms: u64) -> crate::value::Value {
        crate::value::Value::from_json(&serde_json::to_value(&*self.all()).unwrap_or_default())
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
