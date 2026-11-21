//! The rules (the kind `gate`) and the check itself.
//!
//! A rule applies to an [`Action`] when its verb is in the rule's `on` and its `when` holds in a world made of the action:
//! `action` (a map: `verb actor actor_kind task target site usd read_only inside class taint worktree foreign`), `target`, and, by verb, `cmd`
//! (Shell), `path` (Read, Write, Apply) and `host` (Net); `danger(cmd)` is the matcher's word on a line.
//!
//! **Block > Ask > Allow.** The first applicable Block (rules are in id order) is the answer. Else the riskiest applicable
//! Ask is, unless a grant covers it. Else an applicable Allow names itself in `by`; else the answer is
//! `Allow{by: "default"}`, which means no rule had a say: the caller reads that as "nobody objected", not as "approved".
//! Nothing loosens a Block, and an Allow row never overrides an Ask: a grant is asked for only when an Ask would stand, and
//! never for a Block.
//!
//! A rule that cannot be read (a `when` that does not parse, an `on` or a `verdict` that is no word) fails closed: it asks,
//! for the verbs it names (all of them when `on` is what is unreadable), at risk 90, and no grant covers that ask. An
//! unreadable Allow is dropped. A gate entry the Table refused, or a file it refused whole (what it held cannot be known), is not in its rows at
//! all; [`Gates::distrust`] puts an ask in its place. The fields of a rule are floors: one that a layer above the layer that set `verdict` supplies (because the
//! lower one left it out) is ignored, so a missing `when` cannot be filled in by a higher layer to switch a Block off.

use super::{Action, Covers, Verb, Verdict};
use crate::change::Actor;
use crate::eval::{Env, Scope};
use crate::expr::Expr;
use crate::table::{Level, Problem, Tables};
use crate::value::{Key, Value};
use std::cell::RefCell;

/// The `by` of a verdict no rule had a say in.
pub const DEFAULT: &str = "default";
/// The risk of an ask that stands in for a rule that could not be read.
const UNREADABLE_RISK: u8 = 90;
const ROOTS: [&str; 5] = ["action", "cmd", "path", "host", "target"];
const EVERY_VERB: u16 = (1 << Verb::ALL.len()) - 1;

/// What a rule says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Says {
    Allow,
    Ask,
    Block,
}

/// One rule that applied, and what it said (see [`explain`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub rule: String,
    pub says: Says,
    pub why: String,
    pub risk: u8,
    /// For an ask a grant would cover: the grant's id.
    pub grant: Option<String>,
}

struct Rule {
    id: String,
    on: u16,
    when: Option<Expr>,
    says: Says,
    why: String,
    risk: u8,
    /// It stands in for a rule that could not be read.
    unreadable: bool,
}

/// The matcher for a task that has a temporary folder of its own: the line and the folder.
type DangerIn = fn(&str, &super::danger::Scope) -> Value;

/// The compiled rules, in id order.
pub struct Gates {
    rules: Vec<Rule>,
    danger: fn(&str) -> Value,
    /// The matcher for a task that has a temporary folder of its own (`action.tmp`): it is asked the line and the folder. None for a matcher a test put in.
    danger_tmp: Option<DangerIn>,
}

fn bit(v: Verb) -> u16 {
    1 << v as u16
}

fn field(row: &Value, name: &str) -> Value {
    row.at(&[Key::Name(name.to_string())])
}

fn word(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        _ => String::new(),
    }
}

fn stand_in(id: &str, on: u16, why: String) -> Rule {
    Rule { id: id.to_string(), on, when: None, says: Says::Ask, why, risk: UNREADABLE_RISK, unreadable: true }
}

/// Read one row. `own` says whether a field came from the layer that set `verdict` (or is a default).
fn compile(row: &Value, own: &dyn Fn(&str) -> bool) -> (Option<Rule>, Vec<String>) {
    let (mut bad, mut foreign) = (Vec::new(), Vec::new());
    let id = word(&field(row, "id"));
    let says = match word(&field(row, "verdict")).as_str() {
        "allow" => Says::Allow,
        "ask" => Says::Ask,
        "block" => Says::Block,
        _ => {
            bad.push("`verdict` is not allow, ask or block".to_string());
            Says::Ask
        }
    };
    let mut on = EVERY_VERB;
    match field(row, "on") {
        Value::List(_) if !own("on") => foreign.push("`on`"),
        Value::List(items) => match items.iter().map(|v| Verb::from_word(&word(v))).collect::<Option<Vec<_>>>() {
            Some(v) if !v.is_empty() => on = v.into_iter().fold(0, |m, v| m | bit(v)),
            _ => bad.push("`on` names a verb that is not one".to_string()),
        },
        _ => bad.push("`on` is not a list of verbs".to_string()),
    }
    let mut when = None;
    match field(row, "when") {
        Value::Missing => {}
        Value::Str(_) if !own("when") => foreign.push("`when`"),
        Value::Str(text) => match Expr::parse(&text) {
            Ok(e) => match e.check(Some(&ROOTS)).as_slice() {
                [] => when = Some(e),
                wrong => bad.push(format!("`when` {}", wrong.join("; "))),
            },
            Err(e) => bad.push(format!("`when` does not parse: {e}")),
        },
        _ => bad.push("`when` is not text".to_string()),
    }
    let risk = match field(row, "risk") {
        Value::Num(n) if own("risk") => n.clamp(0.0, 100.0) as u8,
        _ => 50,
    };
    let why = match word(&field(row, "why")) {
        s if own("why") && !s.is_empty() => s,
        _ => format!("rule `{id}`"),
    };
    let notes = bad.iter().cloned().chain(foreign.iter().map(|f| format!("{f} comes from a layer above the one that set `verdict` and is ignored")));
    let problems: Vec<String> = notes.map(|m| format!("[[gate]] {id}: {m}")).collect();
    let rule = if !bad.is_empty() {
        let msg = format!("rule `{id}` cannot be read ({}): asking", bad.join("; "));
        (says != Says::Allow).then(|| stand_in(&id, on, msg))
    } else if says == Says::Allow && !foreign.is_empty() {
        None
    } else {
        Some(Rule { id, on, when, says, why, risk, unreadable: false })
    };
    (rule, problems)
}

impl Gates {
    fn build(mut rules: Vec<Rule>) -> Gates {
        rules.sort_by(|a, b| a.id.cmp(&b.id));
        Gates { rules, danger: super::danger::danger_value, danger_tmp: Some(super::danger::danger_value_scoped) }
    }

    /// The rules of the kind `gate`, merged across layers. What cannot be read is in the problems (and asks, see the module).
    pub fn load(tables: &Tables) -> (Gates, Vec<Problem>) {
        let (mut rules, mut problems) = (Vec::new(), Vec::new());
        for row in tables.rows("gate") {
            let from = tables.raw("gate", &word(&field(&row, "id"))).map(|m| &m.from);
            let layer = from.and_then(|f| f.get("verdict"));
            let own = |name: &str| from.and_then(|f| f.get(name)).is_none_or(|l| l == "default" || Some(l) == layer);
            let (rule, notes) = compile(&row, &own);
            rules.extend(rule);
            problems.extend(notes.into_iter().map(|m| Problem::warn("gate", m)));
        }
        (Gates::build(rules), problems)
    }

    /// Rules from rows as the Table gives them, without layers: for a caller that holds rows of its own.
    pub fn from_rows(rows: &[Value]) -> (Gates, Vec<Problem>) {
        let (mut rules, mut problems) = (Vec::new(), Vec::new());
        for row in rows {
            let (rule, notes) = compile(row, &|_| true);
            rules.extend(rule);
            problems.extend(notes.into_iter().map(|m| Problem::warn("gate", m)));
        }
        (Gates::build(rules), problems)
    }

    /// Fail closed on what the Table refused: every refused problem that may have been a gate rule becomes a rule that asks for every verb. An
    /// entry the Table named by its kind (`… [[kind]] id`) is a gate's only when the kind is `gate`; a problem that names no entry (a file that is not
    /// TOML, one with no `api`, a layer with too many files) is a file whose rows the Table never read, so what kind they were cannot be known and
    /// they are taken for gates. Pass the problems of `Tables::load` along with the tables.
    pub fn distrust(mut self, problems: &[Problem]) -> Gates {
        for p in problems.iter().filter(|p| p.level == Level::Refused && (p.at.contains("[[gate]]") || !p.at.contains("[["))) {
            let why = format!("a gate rule was refused ({}: {}): asking", p.at, p.msg);
            self.rules.push(stand_in(&format!("refused:{}", p.at), EVERY_VERB, why));
        }
        let (danger, danger_tmp) = (self.danger, self.danger_tmp);
        let mut gates = Gates::build(self.rules);
        (gates.danger, gates.danger_tmp) = (danger, danger_tmp);
        gates
    }

    /// Use another matcher for `danger(cmd)` (a test's); the default is [`super::danger::danger_value`].
    pub fn with_danger(mut self, f: fn(&str) -> Value) -> Gates {
        (self.danger, self.danger_tmp) = (f, None);
        self
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The rules that apply to `action`, in id order.
    fn applying(&self, action: &Action) -> Vec<&Rule> {
        let world = World::of(action, self.danger, self.danger_tmp);
        let applies = |r: &&Rule| r.on & bit(action.verb) != 0 && r.when.as_ref().is_none_or(|w| w.holds(&world));
        self.rules.iter().filter(applies).collect()
    }
}

/// The world a `when` is read in. `danger(cmd)` is asked of the matcher once per line, however many rules ask.
struct World {
    scope: Scope,
    danger: fn(&str) -> Value,
    /// The task's own temporary folder and the matcher that reads it.
    tmp: Option<(super::danger::Scope, DangerIn)>,
    last: RefCell<Option<(String, Value)>>,
}

impl World {
    fn of(a: &Action, danger: fn(&str) -> Value, danger_tmp: Option<DangerIn>) -> World {
        let text = |s: &str| Value::Str(s.to_string());
        let opt = |s: &Option<String>| s.as_deref().map_or(Value::Missing, text);
        // a path is matched cleaned (`a/../b` is `b`), so no row is passed by a `..` and every row sees the same path
        let target = if matches!(a.verb, Verb::Read | Verb::Write | Verb::Apply) { super::clean_path(&a.target) } else { a.target.clone() };
        let actor = match &a.actor {
            Actor::Operator => "operator",
            Actor::Core => "core",
            Actor::Rule(n) | Actor::Doer(n) | Actor::Model(n) | Actor::Pack(n) => n,
        };
        let map = [
            ("verb", text(a.verb.word())),
            ("actor", text(actor)),
            ("actor_kind", text(a.actor.kind().word())),
            ("task", opt(&a.task)),
            ("target", text(&target)),
            ("site", opt(&a.site)),
            ("usd", Value::usd(a.usd)),
            ("read_only", text(a.read_only.word())),
            ("inside", a.inside.map_or(Value::Missing, Value::Bool)),
            ("class", a.class.map_or(Value::Missing, |c| text(c.word()))),
            ("taint", opt(&a.taint)),
            // A read of another project's state (its transcripts, its `.j4n`); only judged for a path that is read, and relative to the folder the caller gave
            // a write in the worktree the task works in, below J4n's home: the task's own files, not J4n's
            ("worktree", if matches!(a.verb, Verb::Write | Verb::Apply) { Value::Bool(super::foreign::own_worktree(&a.target, a.site.as_deref(), &a.state)) } else { Value::Missing }),
            ("foreign", if a.verb == Verb::Read { Value::Bool(super::foreign::foreign_in(&a.target, a.site.as_deref(), &a.state, true)) } else { Value::Missing }),
        ];
        let by_verb = match a.verb {
            Verb::Shell => "cmd",
            Verb::Net => "host",
            Verb::Read | Verb::Write | Verb::Apply => "path",
            _ => "",
        };
        let scope = Scope::new()
            .with("action", Value::Map(map.into_iter().map(|(k, v)| (k.to_string(), v)).collect()))
            .with("target", text(&target))
            .with(by_verb, text(&target));
        let told = super::danger::Scope { site: a.site.clone(), tmp: a.tmp.clone(), access: a.access.clone(), own: a.own.clone(), state: a.state.clone() };
        let told = (told != super::danger::Scope::default()).then_some(told);
        World { scope, danger, tmp: told.zip(danger_tmp), last: RefCell::new(None) }
    }
}

impl Env for World {
    fn get(&self, root: &str, path: &[Key]) -> Value {
        self.scope.get(root, path)
    }

    fn danger(&self, cmd: &str) -> Value {
        let mut last = self.last.borrow_mut();
        match &*last {
            Some((seen, value)) if seen == cmd => value.clone(),
            _ => {
                let value = match &self.tmp {
                    Some((told, f)) => f(cmd, told),
                    None => (self.danger)(cmd),
                };
                *last = Some((cmd.to_string(), value.clone()));
                value
            }
        }
    }
}

/// What the Gate says of `action`. Pure: the same rules, grants and action give the same verdict.
pub fn check(gates: &Gates, grants: &dyn Covers, action: &Action) -> Verdict {
    let said = gates.applying(action);
    if let Some(r) = said.iter().find(|r| r.says == Says::Block) {
        return Verdict::Block { why: r.why.clone(), by: r.id.clone() };
    }
    let asks = said.iter().filter(|r| r.says == Says::Ask);
    if let Some(r) = asks.clone().reduce(|best, r| if r.risk > best.risk { r } else { best }) {
        if !asks.clone().any(|r| r.unreadable)
            && let Some(grant) = grants.covers(action)
        {
            return Verdict::Allow { by: format!("grant:{grant}") };
        }
        return Verdict::Ask { risk: r.risk, why: r.why.clone(), by: r.id.clone() };
    }
    Verdict::Allow { by: said.first().map_or(DEFAULT, |r| r.id.as_str()).to_string() }
}

/// Every rule that applied to `action` and what it said, in id order (for `:gate <line>` help).
pub fn explain(gates: &Gates, grants: &dyn Covers, action: &Action) -> Vec<Hit> {
    let grant = grants.covers(action);
    let hit = |r: &&Rule| Hit {
        rule: r.id.clone(),
        says: r.says,
        why: r.why.clone(),
        risk: r.risk,
        grant: grant.clone().filter(|_| r.says == Says::Ask && !r.unreadable),
    };
    gates.applying(action).iter().map(hit).collect()
}
