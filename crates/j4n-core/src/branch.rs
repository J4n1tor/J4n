//! Branches of a plan: each open item of the plan runs inline, as a
//! turn of the main lane, or as a branch of one of three classes. The planner marks an item (`where`); J4n decides, and the three
//! stay-inline tests win over the mark. The decision is a pure function of the plan, so the Log can say which test
//! sent an item inline.
//!
//! A branch is `native` (on the doer `own`), `delegate` (on an external harness) or `passthrough` (its brief is `verbatim`):
//! [`via`] says which one it is.

use crate::keeper::{Item, Plan};
use crate::task::Mode;
use std::collections::BTreeSet;

/// What a branch is given beside its goal. `full` (a fork of the main lane's session) is not built yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    /// The contract, the plan line and the files touched in this line so far.
    Brief,
    /// The goal and what the work changed, nothing of the transcript: a reviewer that has not seen the work done.
    Clean,
    /// The operator's words as typed: no Brief, no card, no plan line.
    Verbatim,
}

impl Context {
    pub fn word(self) -> &'static str {
        match self {
            Context::Brief => "brief",
            Context::Clean => "clean",
            Context::Verbatim => "verbatim",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Scout,
    Review,
    Piece,
}

/// How many times an item is sent back before it is `blocked`: the fourth is the block.
pub const SENDS: u32 = 3;
/// How often a plan's approach is revised, and how many items a replan may add, before the operator is asked.
pub const REVISIONS: u32 = 3;
pub const GROW: u32 = 3;
/// What a line's branches may take together, in dollars.
pub const LINE_USD: f64 = 0.50;

pub const CLASSES: [Class; 3] = [Class::Scout, Class::Review, Class::Piece];

impl Class {
    pub fn word(self) -> &'static str {
        match self {
            Class::Scout => "scout",
            Class::Review => "review",
            Class::Piece => "piece",
        }
    }

    pub fn from_word(w: &str) -> Option<Class> {
        CLASSES.into_iter().find(|c| c.word() == w)
    }
}

/// The one switch: what acts on its own. The setting `lanes` says it; `single` is the default because `auto` measured below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Switch {
    /// Nothing branches, and the plan does not drive itself: a line is a task.
    #[default]
    Single,
    /// The plan drives inline items in order and starts `scout` and `review` branches.
    Auto,
    /// `Auto`, and `piece` branches for items the plan marks disjoint.
    Wide,
    /// Main is a chat with a doer session of its own (`who:main`); the keeper, the chats and the messenger are on; every line is Main's own
    /// turn, never a plan begun from the words of a line.
    Orchestrate,
}

impl Switch {
    pub fn word(self) -> &'static str {
        match self {
            Switch::Single => "single",
            Switch::Auto => "auto",
            Switch::Wide => "wide",
            Switch::Orchestrate => "orchestrate",
        }
    }

    /// Every word, in the order `:lanes` says them.
    pub const ALL: [Switch; 4] = [Switch::Single, Switch::Auto, Switch::Wide, Switch::Orchestrate];

    pub fn from_word(w: &str) -> Option<Switch> {
        Switch::ALL.into_iter().find(|s| s.word() == w)
    }

    /// Whether a line may begin a plan from its words: under `orchestrate` a line is Main's own turn.
    pub fn begins_plans(self) -> bool {
        matches!(self, Switch::Auto | Switch::Wide)
    }
}

/// The stay-inline tests, in the order they are tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stay {
    /// The item needs a decision only the main lane holds: it is marked `main`, or an Ask is open on the main lane.
    Main,
    /// The item reads what this line wrote (its paths meet the paths written so far).
    Coupled,
    /// The item is estimated to cost less than starting a branch.
    Cheap,
}

impl Stay {
    pub fn word(self) -> &'static str {
        match self {
            Stay::Main => "main",
            Stay::Coupled => "coupled",
            Stay::Cheap => "cheap",
        }
    }
}

/// What starting a branch costs, in tokens: the fixed context of a turn, the Brief, and the main turn that reads the report
/// The host's price is not in it: the doer of a branch is elected after the decision.
pub const START_TOKENS: usize = 1_800;
/// What an item is estimated to cost, by what it says it reads: a file, and the words of its goal and its done-when.
pub const READ_TOKENS: usize = 700;

/// What an item is estimated to cost, in tokens, if the main lane did it itself.
pub fn estimate(item: &Item) -> usize {
    crate::brief::tokens(&item.goal) + crate::brief::tokens(&item.done) + READ_TOKENS * (item.reads.len() + item.writes.len())
}

/// What the decision reads besides the plan.
#[derive(Debug, Clone, Default)]
pub struct Ctx {
    pub switch: Switch,
    /// The branch rows: which classes the switch allows, and what each is given.
    pub rules: Rules,
    /// The paths this line has written so far: what the plan's started items wrote, and what the main lane's last turn changed.
    pub written: BTreeSet<String>,
    /// An Ask is open on the main lane.
    pub asked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Runs as a turn of the main lane. `test`: the stay-inline test that sent it there, if one did.
    Inline { test: Option<Stay>, why: String },
    Branch { class: Class, why: String },
}

impl Verdict {
    pub fn place(&self) -> &'static str {
        match self {
            Verdict::Inline { .. } => "inline",
            Verdict::Branch { class, .. } => class.word(),
        }
    }

    pub fn why(&self) -> &str {
        let (Verdict::Inline { why, .. } | Verdict::Branch { why, .. }) = self;
        why
    }

    pub fn test(&self) -> Option<Stay> {
        match self {
            Verdict::Inline { test, .. } => *test,
            Verdict::Branch { .. } => None,
        }
    }
}

fn inline(test: Option<Stay>, why: impl Into<String>) -> Verdict {
    Verdict::Inline { test, why: why.into() }
}

/// The ids of the items that wait for `id`, directly or through others.
fn dependents(plan: &Plan, id: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    loop {
        let more: Vec<String> = plan.items.iter().filter(|o| !found.contains(&o.id) && o.depends.iter().any(|d| d == id || found.contains(d))).map(|o| o.id.clone()).collect();
        if more.is_empty() {
            return found;
        }
        found.extend(more);
    }
}

fn meets(a: &[String], b: &[String]) -> Option<String> {
    a.iter().find(|p| b.contains(p)).cloned()
}

/// Where `item` of `plan` runs. The planner's mark proposes; the tests dispose.
pub fn decide(item: &Item, plan: &Plan, ctx: &Ctx) -> Verdict {
    if ctx.switch == Switch::Single {
        return inline(None, "lanes = single: nothing branches");
    }
    let Some(class) = Class::from_word(&item.mark) else { return inline(None, "the plan marks it inline") };
    if !ctx.rules.allows(class, ctx.switch) {
        return inline(None, ctx.rules.refusal(class));
    }
    if item.main || ctx.asked {
        return inline(Some(Stay::Main), if item.main { "the plan marks it as needing the main lane's decision" } else { "an Ask is open on the main lane" });
    }
    let touched: Vec<String> = item.reads.iter().chain(&item.writes).cloned().collect();
    let written: Vec<String> = ctx.written.iter().cloned().collect();
    // a review reads the work of this line by what it is for: the coupling test is for the classes that would read it by chance
    if class != Class::Review
        && let Some(p) = meets(&touched, &written)
    {
        return inline(Some(Stay::Coupled), format!("it reads `{p}`, which this line wrote"));
    }
    let (cost, start) = (estimate(item), START_TOKENS);
    if cost < start {
        return inline(Some(Stay::Cheap), format!("about {cost} tokens, less than the {start} a branch costs to start"));
    }
    if class != Class::Piece && !item.writes.is_empty() {
        return inline(None, format!("it writes `{}` and a {} only reads", item.writes[0], class.word()));
    }
    if class == Class::Piece {
        if item.writes.is_empty() {
            return inline(None, "a piece with no paths to write is not disjoint from anything");
        }
        // disjoint on paths from every other item that is not done
        // (an item that waits for this one, directly or through others, runs after it: it is no clash)
        let after = dependents(plan, &item.id);
        let clash = plan.items.iter().filter(|o| o.id != item.id && !o.done() && !after.contains(&o.id)).find_map(|o| meets(&item.writes, &o.reads.iter().chain(&o.writes).cloned().collect::<Vec<_>>()).map(|p| (o.id.clone(), p)));
        if let Some((other, p)) = clash {
            return inline(None, format!("`{p}` is also in {other}: the pieces are not disjoint"));
        }
    }
    Verdict::Branch { class, why: format!("{} of about {cost} tokens, apart from this line's work", class.word()) }
}

/// Which of the three words a branch is: `verbatim` is `passthrough`; else the doer `own` is `native`, and any other is
/// `delegate`.
pub fn via(doer: &str, context: Context) -> &'static str {
    match (context, doer) {
        (Context::Verbatim, _) => "passthrough",
        (_, "own") => "native",
        _ => "delegate",
    }
}

/// A row of `packs/lanes/rules/branch.toml`: one class of branch, the switch words under which a rule may start it, and what it is given.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub id: String,
    pub doc: Option<String>,
    pub under: Vec<String>,
    pub mode: String,
    pub context: String,
    pub usd: Option<f64>,
    pub wall: Option<String>,
    /// More time for each file the branch is given past the first, up to `wall_top`: a review of a whole program reads more than one of a file.
    pub wall_file: Option<String>,
    pub wall_top: Option<String>,
}

impl crate::table::Kind for Row {
    const NAME: &'static str = "branch";
}

impl Row {
    fn check(&self) -> Result<(), String> {
        let bad = |what: &str| Err(format!("branch `{}`: {what}", self.id));
        if Class::from_word(&self.id).is_none() {
            return bad("is not a class (scout, review, piece)");
        }
        if self.under.iter().any(|u| !matches!(Switch::from_word(u), Some(Switch::Auto | Switch::Wide))) {
            return bad("`under` lists `auto` or `wide`");
        }
        if !["read", "write"].contains(&self.mode.as_str()) || !["brief", "clean", "verbatim"].contains(&self.context.as_str()) {
            return bad("`mode` is read or write, `context` brief, clean or verbatim");
        }
        if [&self.wall, &self.wall_file, &self.wall_top].iter().any(|w| w.as_deref().is_some_and(|w| crate::expr::Expr::dur(w).is_none())) {
            return bad("`wall`, `wall_file` and `wall_top` are durations such as \"2m\"");
        }
        Ok(())
    }
}

/// The branch rows: the class table as data.
#[derive(Debug, Clone)]
pub struct Rules(std::sync::Arc<Vec<Row>>);

impl Default for Rules {
    fn default() -> Rules {
        Rules::builtin()
    }
}

#[derive(serde::Deserialize)]
struct File {
    #[serde(default)]
    branch: Vec<Row>,
}

impl Rules {
    /// The rows of the pack `lanes` as it ships.
    pub fn builtin() -> Rules {
        Rules::parse(include_str!("../../../packs/lanes/rules/branch.toml")).unwrap_or_else(|e| panic!("packs/lanes/rules/branch.toml: {e}"))
    }

    /// No rows: the pack is off, and nothing branches.
    pub fn none() -> Rules {
        Rules(Default::default())
    }

    pub fn parse(text: &str) -> Result<Rules, String> {
        let file: File = toml::from_str(text).map_err(|e| e.to_string())?;
        file.branch.iter().try_for_each(Row::check)?;
        Ok(Rules(std::sync::Arc::new(file.branch)))
    }

    /// The rows the Table holds (kind `branch`, pack `lanes`); a row that does not read is left out, and its fault returned.
    pub fn load(tables: &crate::table::Tables) -> (Rules, Vec<crate::table::Problem>) {
        let (mut rows, mut problems) = (Vec::new(), Vec::new());
        for id in tables.ids("branch") {
            match tables.get::<Row>(id) {
                Ok(r) => match r.check() {
                    Ok(()) => rows.push(r),
                    Err(e) => problems.push(crate::table::Problem::refused(format!("branch {id}"), e)),
                },
                Err(p) => problems.push(p),
            }
        }
        (Rules(std::sync::Arc::new(rows)), problems)
    }

    fn row(&self, class: Class) -> Option<&Row> {
        self.0.iter().find(|r| r.id == class.word())
    }

    /// May a rule start a branch of `class` while the switch is `switch`?
    pub fn allows(&self, class: Class, switch: Switch) -> bool {
        // `orchestrate` keeps what `auto` starts by rule, until Main chooses the chats itself
        let word = if switch == Switch::Orchestrate { Switch::Auto.word() } else { switch.word() };
        self.row(class).is_some_and(|r| r.under.iter().any(|u| u == word))
    }

    /// Why a class is not started: the switch words that would, or that no row says it.
    pub fn refusal(&self, class: Class) -> String {
        match self.row(class) {
            Some(r) => format!("a {} runs only under lanes = {}", class.word(), r.under.join(" or ")),
            None => format!("no branch row for a {}: the pack `lanes` is off", class.word()),
        }
    }

    /// A scout and a reviewer only read: the Gate refuses a write whatever the doer does.
    pub fn mode(&self, class: Class) -> Mode {
        if self.row(class).is_some_and(|r| r.mode == "write") { Mode::Write } else { Mode::Read }
    }

    pub fn context(&self, class: Class) -> Context {
        match self.row(class).map(|r| r.context.as_str()) {
            Some("clean") => Context::Clean,
            Some("verbatim") => Context::Verbatim,
            _ => Context::Brief,
        }
    }

    /// Dollars and milliseconds a branch of the class may take; none: the flow's own.
    pub fn limits(&self, class: Class) -> (Option<f64>, Option<u64>) {
        let row = self.row(class);
        (row.and_then(|r| r.usd), row.and_then(|r| r.wall.as_deref()).and_then(crate::expr::Expr::dur))
    }

    /// The time a branch of the class may take when it is given `files` files: its `wall`, and `wall_file` more for each file past the first, up to
    /// `wall_top` (never under `wall`); none: the flow's own.
    pub fn wall(&self, class: Class, files: usize) -> Option<u64> {
        let (row, dur) = (self.row(class)?, |w: &Option<String>| w.as_deref().and_then(crate::expr::Expr::dur));
        let base = dur(&row.wall)?;
        let more = dur(&row.wall_file).unwrap_or(0).saturating_mul(files.saturating_sub(1) as u64);
        Some(base.saturating_add(more).min(dur(&row.wall_top).unwrap_or(u64::MAX).max(base)))
    }
}
