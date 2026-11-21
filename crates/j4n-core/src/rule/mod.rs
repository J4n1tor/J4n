//! Rules: reacting to what happened (the kind `rule`, `packs/base/kinds/rule.toml`). A rule says
//! *when this event was written and that holds, do this*; the loop is what does it, so a rule can only ask for what the loop
//! allows: start a task, steer one, say something, park one, cool one. There is no kill, no change of a Gate, no spend.
//!
//! - The words a rule says are made of counts, the operator's own words and the rule's own text. A hole that reads what a doer
//!   or a model wrote (`ev.data.text`, `fold.tasks[task].answer`) is refused when the rule is loaded unless `quote(…)` makes it
//!   inert; what comes out is cut to [`TEXT_MAX`] characters and has no control characters (the taint rule).
//! - Every action carries the event it answers as its `cause`. A reaction to a reaction goes on to a depth of [`DEPTH_MAX`], and
//!   no more than [`RATE_MAX`] actions are taken in [`RATE_WINDOW_MS`]; past either the action is dropped, with a notice.
//! - `limit` and `cooldown` keep a rule from saying the same thing again and again (quietly: that is not an overflow);
//!   `after` waits for quiet — a newer match for the same task starts the wait again — and a task that ends takes its waiting
//!   actions with it.
//! - [`Rules`] is what was loaded and never changes; [`State`] is what a running loop remembers, in memory: a restart forgets
//!   the counts and the waits, as it forgets the tasks they were about.

mod env;
mod run;

pub use env::RuleEnv;
pub use run::{Act, Do, Due, State};

use crate::eval::glob_match;
use crate::expr::Expr;
use crate::fmt::Fmt;
use crate::log::FLASH_KINDS;
use crate::table::{Kind, Problem, Tables};
use serde::Deserialize;

/// The deepest chain of rules answering rules.
pub const DEPTH_MAX: u8 = 8;
/// The most actions in a window; the rest are dropped.
pub const RATE_MAX: usize = 120;
pub const RATE_WINDOW_MS: u64 = 10_000;
/// The longest words a rule says.
pub const TEXT_MAX: usize = 4_000;
/// What `keep` may add to a compaction.
pub const NOTES_MAX: usize = 6;
pub const NOTE_CHARS: usize = 200;
/// The most waits and counts kept at once.
pub const KEYS_MAX: usize = 10_000;

/// What a rule's expressions may name: the event, its fields, the folds and the settings.
pub const ROOTS: [&str; 14] = ["ev", "event", "seq", "at_ms", "kind", "task", "thread", "by", "cause", "class", "data", "fold", "set", "ctx"];

/// What a rule that asks (a reflex) may run: none can widen what J4n may do; [`THROWS`] ask the operator until the judge's calls have held.
pub const ALLOWED: [&str; 4] = ["compact", "fresh", "new", "context"];
pub const THROWS: [&str; 3] = ["compact", "fresh", "new"];
/// The boundaries where the front reads a rule that asks.
pub const ASKS_AT: [&str; 2] = ["task.settled", "chat.say"];
pub const SENTENCE: usize = 400;
pub const NO_CHECK: &str = "a reflex needs a condition (`when` in a rule, `check` in `:reflex add`), in the rule language, such as `ctx.grew >= 0.01`: a judge asked at every turn costs more than it saves";

/// Fields of events whose words come from a doer or a model, or from someone who typed them into a tool.
const TAINTED: [&str; 17] = [
    "text", "answer", "report", "reason", "cmd", "out", "detail", "name", "message", "why", "question", "paths", "note", "summary", "body", "output", "error",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Then {
    Start,
    Steer,
    #[default]
    Notice,
    Control,
    Cool,
    Ask,
    Propose,
    /// Write a `label` event: what happened to a machine's answer, or to an item or mark of a feature that is measured.
    Label,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Control {
    Park,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Per {
    #[default]
    Task,
    Thread,
    All,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitRow {
    #[serde(default)]
    pub per: Per,
    pub max: u32,
    pub within: Option<String>,
}

/// For `label`: what is written. The words are templates over the event, read like `text`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelRow {
    /// The label: `overridden`, `undone`, `failed_after`, `agreed`, `disagreed`, `held`, or another a pack counts.
    pub is: String,
    /// What is labelled: `answer` (default), `shape_item`, `qc_mark`.
    pub of: Option<String>,
    /// Which one: for an answer, its ask's id.
    pub subject: String,
    /// Who made it: the answerer's id, the item kind or the pattern.
    pub producer: String,
    pub model: Option<String>,
    /// The ask point, for an answer.
    pub point: Option<String>,
    /// How much of a wrong answer it counts for, from 0 to 1.
    pub weight: Option<f64>,
}

/// For `propose`: the proposal made. `key`, `title`, `why` and `id` are templates over the event. A
/// field of `set` is a value as it stands (a number, a list), a template when it is text, or an expression when the text begins
/// with `=` (so a number or a list read from a fold keeps its kind). Nothing is written until the operator says yes.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposeRow {
    /// The same thing proposed again has the same key.
    pub key: String,
    pub title: String,
    pub why: String,
    /// How much evidence there is, a count: a declined proposal returns when it has doubled.
    pub evidence: Option<String>,
    /// `home` (default) or `project`.
    pub to: Option<String>,
    pub file: Option<String>,
    pub kind: String,
    pub id: String,
    #[serde(default)]
    pub set: toml::Table,
    #[serde(default)]
    pub unset: Vec<String>,
    #[serde(default)]
    pub remove: bool,
}

/// An entry of kind `rule`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleRow {
    pub id: String,
    pub doc: Option<String>,
    pub on: String,
    pub when: Option<String>,
    pub then: Then,
    pub text: Option<String>,
    /// For `ask`: the command that runs when the judge says the sentence in `text` holds.
    #[serde(rename = "do")]
    pub run: Option<String>,
    /// A rule that is off is read and checked and does nothing.
    #[serde(default)]
    pub off: bool,
    #[serde(default)]
    pub follow: bool,
    pub doer: Option<String>,
    pub place: Option<String>,
    /// For `start` of a task that is not carried on: it belongs to no thread (a spot check is not the thread's last task, and a
    /// line the operator types does not steer it).
    #[serde(default)]
    pub detached: bool,
    #[serde(default)]
    pub compact: bool,
    #[serde(default)]
    pub keep: Vec<String>,
    pub control: Option<Control>,
    pub label: Option<LabelRow>,
    pub propose: Option<ProposeRow>,
    pub cool_for: Option<String>,
    pub after: Option<String>,
    pub limit: Option<LimitRow>,
    pub cooldown: Option<String>,
}

impl Kind for RuleRow {
    const NAME: &'static str = "rule";
}

/// A `label` as it runs: the templates read and checked.
pub(crate) struct LabelSpec {
    pub is: String,
    pub of: String,
    pub subject: Fmt,
    pub producer: Fmt,
    pub model: Option<Fmt>,
    pub point: Option<Fmt>,
    pub weight: f64,
}

/// A `propose` as it runs.
pub(crate) struct ProposeSpec {
    pub key: Fmt,
    pub title: Fmt,
    pub why: Fmt,
    pub evidence: Option<Expr>,
    pub to: crate::change::Where,
    pub file: Option<String>,
    pub kind: String,
    pub id: Fmt,
    pub set: Vec<(String, SetVal)>,
    pub unset: Vec<String>,
    pub remove: bool,
}

/// What a field of a proposal's `set` is made from.
pub(crate) enum SetVal {
    Lit(toml::Value),
    Tpl(Fmt),
    Ex(Expr),
}

pub(crate) struct Limit {
    pub per: Per,
    pub max: u32,
    pub within_ms: Option<u64>,
}

/// A rule as it runs: every expression and template read and checked.
#[derive(Default)]
pub struct Rule {
    pub id: String,
    pub then: Then,
    pub(crate) on: String,
    pub(crate) when: Option<Expr>,
    pub(crate) text: Option<Fmt>,
    pub(crate) run: String,
    pub(crate) follow: bool,
    pub(crate) doer: Option<String>,
    pub(crate) place: Option<String>,
    pub(crate) detached: bool,
    pub(crate) compact: bool,
    pub(crate) keep: Vec<Fmt>,
    pub(crate) label: Option<LabelSpec>,
    pub(crate) propose: Option<ProposeSpec>,
    pub(crate) cool_for: Option<Expr>,
    pub(crate) after_ms: Option<u64>,
    pub(crate) limit: Option<Limit>,
    pub(crate) cooldown_ms: Option<u64>,
    /// An incident (kind `incident`) says its words as an alert; a rule has none.
    pub(crate) alert: Option<Alert>,
}

/// What an incident adds to the words it says.
pub(crate) struct Alert {
    pub incident: String,
    pub like: String,
    pub class: String,
    pub done: String,
    pub source: String,
}

/// The rules the Table holds, indexed by the kinds they answer.
#[derive(Default)]
pub struct Rules {
    pub(crate) list: Vec<Rule>,
    flash: bool,
}

/// The paths a template reads that a doer's or a model's words could be behind, and are not quoted.
fn tainted(reads: Vec<Vec<String>>) -> Vec<String> {
    let mut found = Vec::new();
    for path in reads {
        let whole = matches!(path.iter().map(String::as_str).collect::<Vec<_>>().as_slice(), ["ev"] | ["event"] | ["data"] | ["ev" | "event", "data"] | ["fold"] | ["fold", "tasks"] | ["set"]);
        if whole || path.iter().skip(1).any(|k| TAINTED.contains(&k.as_str())) {
            found.push(path.join("."));
        }
    }
    found
}

fn template(text: &str, what: &str, errs: &mut Vec<String>) -> Option<Fmt> {
    match Fmt::parse(text) {
        Ok(f) => {
            errs.extend(f.check(Some(&ROOTS)).into_iter().map(|m| format!("`{what}`: {m}")));
            let bare = tainted(f.unquoted_reads());
            if !bare.is_empty() {
                errs.push(format!("`{what}` says words a doer or a model wrote, bare: {} — wrap them in quote(…)", bare.join(", ")));
            }
            Some(f)
        }
        Err(e) => {
            errs.push(format!("`{what}`: {e}"));
            None
        }
    }
}

fn expr(text: &str, what: &str, errs: &mut Vec<String>) -> Option<Expr> {
    match Expr::parse(text) {
        Ok(e) => {
            errs.extend(e.check(Some(&ROOTS)).into_iter().map(|m| format!("`{what}`: {m}")));
            Some(e)
        }
        Err(e) => {
            errs.push(format!("`{what}`: {e}"));
            None
        }
    }
}

/// An expression whose value is written down: it may not read what a doer or a model wrote, bare.
fn value_expr(text: &str, what: &str, errs: &mut Vec<String>) -> Option<Expr> {
    let e = expr(text, what, errs)?;
    let bare = tainted(e.unquoted_reads());
    if !bare.is_empty() {
        errs.push(format!("`{what}` writes words a doer or a model wrote, bare: {} — wrap them in quote(…)", bare.join(", ")));
    }
    Some(e)
}

fn dur(text: &Option<String>, what: &str, errs: &mut Vec<String>) -> Option<u64> {
    let text = text.as_deref()?;
    let ms = Expr::dur(text).filter(|ms| *ms > 0);
    if ms.is_none() {
        errs.push(format!("`{what}` is a duration more than zero, such as `5m`, not `{text}`"));
    }
    ms
}

impl RuleRow {
    /// What a rule that asks (a reflex) must say to be one.
    fn refuse_ask(&self, errs: &mut Vec<String>) {
        let said = self.text.as_deref().unwrap_or_default();
        if said.trim().is_empty() || said.chars().count() > SENTENCE || said.chars().any(char::is_control) {
            errs.push(format!("`text` is your sentence, a line of at most {SENTENCE} characters"));
        }
        if self.when.as_deref().is_none_or(|w| w.trim().is_empty()) {
            errs.push(NO_CHECK.into());
        }
        if !ASKS_AT.contains(&self.on.as_str()) {
            errs.push(format!("`on` is one of {}", ASKS_AT.join(", ")));
        }
        if self.limit.as_ref().is_none_or(|l| l.per != Per::Thread || l.max != 1 || l.within.is_none()) {
            errs.push("a reflex needs `limit = { per = \"thread\", max = 1, within = \"10m\" }`: the least time between two times the judge is asked".into());
        }
        if self.id.is_empty() || self.id.len() > 40 || !self.id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')) {
            errs.push("a reflex's id is letters, digits, `-` and `_`, at most 40".into());
        }
        match self.command() {
            Some(c) if ALLOWED.contains(&c.name.as_str()) && (c.name == "compact" || c.args.is_empty()) => {}
            Some(c) if ALLOWED.contains(&c.name.as_str()) => errs.push(format!("`:{}` takes no words in a reflex: only `:compact` takes its mode (`:compact junk`, `:compact keep <words>`)", c.name)),
            _ => errs.push(format!("`do` is one of {}: a reflex never runs anything that widens what J4n may do", ALLOWED.iter().map(|c| format!(":{c}")).collect::<Vec<_>>().join(", "))),
        }
    }

    /// The command of `do`, and nothing else.
    pub fn command(&self) -> Option<crate::front::phrase::Cmd> {
        let mut words = self.run.as_deref()?.split_whitespace();
        let name = words.next()?.trim_start_matches([':', '/']).to_lowercase();
        Some(crate::front::phrase::Cmd { name, args: words.map(String::from).collect() })
    }

    /// The row as the one rule it is, for the front to read at its boundary.
    pub fn rules(&self) -> Result<Rules, Vec<String>> {
        self.compile().map(|r| Rules { list: vec![r], flash: false })
    }

    /// Read and check everything the row says. Every fault is reported, not only the first.
    pub fn compile(&self) -> Result<Rule, Vec<String>> {
        let mut errs = Vec::new();
        let when = self.when.as_deref().and_then(|w| expr(w, "when", &mut errs));
        let text = self.text.as_deref().and_then(|t| template(t, "text", &mut errs));
        let cool_for = self.cool_for.as_deref().and_then(|c| expr(c, "cool_for", &mut errs));
        let keep: Vec<Fmt> = self.keep.iter().filter_map(|k| template(k, "keep", &mut errs)).collect();
        let label = self.label.as_ref().and_then(|l| {
            let of = l.of.clone().unwrap_or_else(|| "answer".into());
            if !["answer", "shape_item", "qc_mark"].contains(&of.as_str()) || l.is.is_empty() || l.is.len() > 32 || l.is.chars().any(|c| c.is_control() || c.is_whitespace()) {
                errs.push("`label` names what it labels (`answer`, `shape_item`, `qc_mark`) and says a word of at most 32 characters".into());
            }
            let weight = l.weight.unwrap_or(0.0);
            if !(0.0..=1.0).contains(&weight) {
                errs.push("a label's `weight` is from 0 to 1".into());
            }
            let subject = template(&l.subject, "label.subject", &mut errs);
            let producer = template(&l.producer, "label.producer", &mut errs);
            let model = l.model.as_deref().map(|t| template(t, "label.model", &mut errs));
            let point = l.point.as_deref().map(|t| template(t, "label.point", &mut errs));
            Some(LabelSpec { is: l.is.clone(), of, subject: subject?, producer: producer?, model: model.flatten(), point: point.flatten(), weight })
        });
        let propose = self.propose.as_ref().and_then(|p| {
            let to = match p.to.as_deref().unwrap_or("home") {
                "home" => Some(crate::change::Where::User),
                "project" => Some(crate::change::Where::Project),
                other => {
                    errs.push(format!("`propose.to` is `home` or `project`, not `{other}`"));
                    None
                }
            };
            if p.set.is_empty() && p.unset.is_empty() && !p.remove {
                errs.push("`propose` says what to `set`, `unset` or `remove`".into());
            }
            if p.remove && (!p.set.is_empty() || !p.unset.is_empty()) {
                errs.push("`propose.remove` stands alone".into());
            }
            let set: Vec<(String, SetVal)> = p
                .set
                .iter()
                .filter_map(|(k, v)| {
                    let at = format!("propose.set.{k}");
                    let val = match v.as_str() {
                        Some(t) => match t.strip_prefix('=') {
                            Some(e) => SetVal::Ex(value_expr(e, &at, &mut errs)?),
                            None => SetVal::Tpl(template(t, &at, &mut errs)?),
                        },
                        None => SetVal::Lit(v.clone()),
                    };
                    Some((k.clone(), val))
                })
                .collect();
            let evidence = p.evidence.as_deref().and_then(|e| expr(e, "propose.evidence", &mut errs));
            let (key, title, why, id) = (
                template(&p.key, "propose.key", &mut errs),
                template(&p.title, "propose.title", &mut errs),
                template(&p.why, "propose.why", &mut errs),
                template(&p.id, "propose.id", &mut errs),
            );
            Some(ProposeSpec { key: key?, title: title?, why: why?, evidence, to: to?, file: p.file.clone(), kind: p.kind.clone(), id: id?, set, unset: p.unset.clone(), remove: p.remove })
        });
        let after_ms = dur(&self.after, "after", &mut errs);
        let cooldown_ms = dur(&self.cooldown, "cooldown", &mut errs);
        let limit = self.limit.as_ref().map(|l| Limit { per: l.per, max: l.max, within_ms: dur(&l.within, "limit.within", &mut errs) });
        if self.then != Then::Ask {
            let reads = when.iter().flat_map(Expr::unquoted_reads).chain(text.iter().flat_map(Fmt::unquoted_reads));
            if reads.into_iter().any(|p| p.first().is_some_and(|r| r == "ctx")) {
                errs.push("`ctx` is read by a rule that asks only: the loop has none".into());
            }
        }
        let mut forbid = |present: bool, field: &str, only: &str| {
            if present {
                errs.push(format!("`{field}` is for {only}"));
            }
        };
        let (start, says) = (self.then == Then::Start, matches!(self.then, Then::Steer | Then::Notice));
        forbid(self.follow && !start, "follow", "`start`");
        forbid(self.doer.is_some() && !start, "doer", "`start`");
        forbid(self.doer.is_some() && self.follow, "doer", "a task that is not carried on");
        forbid(self.place.is_some() && !(start && !self.follow), "place", "`start` of a task that is not carried on");
        forbid(self.detached && !(start && !self.follow), "detached", "`start` of a task that is not carried on");
        forbid(self.compact && !(start && self.follow), "compact", "`start` with `follow`");
        forbid(!self.keep.is_empty() && !self.compact, "keep", "`compact`");
        forbid(self.control.is_some() && self.then != Then::Control, "control", "`control`");
        forbid(self.label.is_some() && self.then != Then::Label, "label", "`label`");
        forbid(self.cool_for.is_some() && self.then != Then::Cool, "cool_for", "`cool`");
        forbid(self.propose.is_some() && self.then != Then::Propose, "propose", "`propose`");
        forbid(self.text.is_some() && !(start || says || self.then == Then::Ask), "text", "`start`, `steer`, `notice` and `ask`");
        forbid(self.run.is_some() && self.then != Then::Ask, "do", "`ask`");
        match self.then {
            Then::Start | Then::Steer | Then::Notice | Then::Ask if self.text.is_none() => errs.push("this needs `text`".into()),
            Then::Control if self.control.is_none() => errs.push("`control` needs `control = \"park\"`".into()),
            Then::Cool if self.cool_for.is_none() => errs.push("`cool` needs `cool_for`".into()),
            Then::Label if self.label.is_none() => errs.push("`label` needs `label = { is = …, subject = …, producer = … }`".into()),
            Then::Ask => self.refuse_ask(&mut errs),
            Then::Propose if self.propose.is_none() => errs.push("`propose` needs `propose = { key, title, why, kind, id, set }`".into()),
            _ => {}
        }
        if self.place.as_deref().is_some_and(|p| p.is_empty() || p.len() > 64 || p.chars().any(|c| c.is_control() || c.is_whitespace())) {
            errs.push("`place` is a place's name, or `@same` for the place of the task of the event".into());
        }
        if self.keep.len() > NOTES_MAX {
            errs.push(format!("`keep` has at most {NOTES_MAX} notes"));
        }
        if !errs.is_empty() {
            return Err(errs);
        }
        Ok(Rule {
            id: self.id.clone(),
            then: self.then,
            on: self.on.clone(),
            when,
            text,
            run: self.run.clone().unwrap_or_default(),
            follow: self.follow,
            doer: self.doer.clone(),
            place: self.place.clone(),
            detached: self.detached,
            compact: self.compact,
            keep,
            label,
            propose,
            cool_for,
            after_ms,
            limit,
            cooldown_ms,
            alert: None,
        })
    }
}

/// An alert is told at most this often for one task and one incident: a run that tries the same thing again is not a storm of rows.
const ALERT_MAX: u32 = 5;
const ALERT_WITHIN_MS: u64 = 3_600_000;
/// The longest name an alert gives its attack.
pub const LIKE_MAX: usize = 40;

/// An entry of kind `incident`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IncidentRow {
    pub id: String,
    pub doc: Option<String>,
    pub like: String,
    pub class: String,
    pub on: String,
    pub when: Option<String>,
    pub done: Option<String>,
    pub text: String,
    pub source: String,
}

impl Kind for IncidentRow {
    const NAME: &'static str = "incident";
}

impl IncidentRow {
    /// An incident is a rule that can only say: read and checked as one, its words an alert.
    pub fn compile(&self) -> Result<Rule, Vec<String>> {
        let mut errs = Vec::new();
        let when = self.when.as_deref().and_then(|w| expr(w, "when", &mut errs));
        let text = template(&self.text, "text", &mut errs);
        let bad = |t: &str| t.chars().any(char::is_control);
        if self.like.trim().is_empty() || self.like.chars().count() > LIKE_MAX || bad(&self.like) {
            errs.push(format!("`like` is a short name of at most {LIKE_MAX} characters"));
        }
        if !self.source.starts_with("https://") || self.source.chars().any(|c| c.is_control() || c.is_whitespace()) {
            errs.push("`source` is an https:// address".into());
        }
        if !errs.is_empty() {
            return Err(errs);
        }
        let alert = Alert { incident: self.id.clone(), like: self.like.trim().into(), class: self.class.clone(), done: self.done.clone().unwrap_or_else(|| "flagged".into()), source: self.source.clone() };
        let limit = Some(Limit { per: Per::Task, max: ALERT_MAX, within_ms: Some(ALERT_WITHIN_MS) });
        Ok(Rule { id: format!("incident:{}", self.id), then: Then::Notice, on: self.on.clone(), when, text, limit, alert: Some(alert), ..Rule::default() })
    }
}

impl Rules {
    /// The Table's rules. A rule that does not compile is a problem and no rule.
    pub fn load(tables: &Tables) -> (Rules, Vec<Problem>) {
        let flashed = Rules::flash_kinds(tables);
        let (mut list, mut problems) = (Vec::new(), Vec::new());
        // one row at a time: a row that cannot be read leaves the others as they are
        for id in tables.ids(RuleRow::NAME) {
            match tables.get::<RuleRow>(id) {
                // a rule that is off does nothing, and one that asks is a reflex: the front reads it at its own boundary, not the loop
                Ok(row) => match row.compile() {
                    Ok(rule) => list.extend((!row.off && row.then != Then::Ask).then_some(rule)),
                    Err(e) => problems.push(Problem::refused(format!("rule {id}"), e.join("; "))),
                },
                Err(p) => problems.push(p),
            }
        }
        for id in tables.ids(IncidentRow::NAME) {
            match tables.get::<IncidentRow>(id) {
                Ok(row) => match row.compile() {
                    Ok(rule) => list.push(rule),
                    Err(e) => problems.push(Problem::refused(format!("incident {id}"), e.join("; "))),
                },
                Err(p) => problems.push(p),
            }
        }
        let flash = list.iter().any(|r| flashed.iter().any(|k| glob_match(&r.on, k)));
        (Rules { list, flash }, problems)
    }

    /// The kinds of event that are only ever flashed: the core's and the ones a pack declares so.
    fn flash_kinds(tables: &Tables) -> Vec<String> {
        let declared = tables.list::<crate::log::EventDecl>().unwrap_or_default();
        FLASH_KINDS.iter().map(ToString::to_string).chain(declared.into_iter().filter(|d| d.ephemeral).map(|d| d.id)).collect()
    }

    /// Whether any rule answers an event of this kind. A task beginning or ending a wait for an answer is wanted by every rule that
    /// waits (`after`): its waits stop while the task waits and count from the resume (`State::hold_waits`, `State::resume_waits`).
    pub fn wants(&self, kind: &str) -> bool {
        self.list.iter().any(|r| glob_match(&r.on, kind) || r.after_ms.is_some() && matches!(kind, "task.waiting" | "task.resumed"))
    }

    /// Whether any rule answers a kind that is only flashed, so the loop has to listen for flashes too.
    pub fn wants_flash(&self) -> bool {
        self.flash
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn ids(&self) -> Vec<&str> {
        self.list.iter().map(|r| r.id.as_str()).collect()
    }
}
