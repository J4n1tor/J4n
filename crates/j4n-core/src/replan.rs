//! When the approach is wrong: the triggers of a replan are data — a table of what
//! went wrong with an item (the fold `plan` counts it, `Item::went`) and how often — and a plan is read against it. Nothing here asks the
//! main model whether to change the approach; a trigger starts a planning turn (`front/replan.rs`) that is told why.
//!
//! `escalate` (a stronger model for a check that fails twice) shares the first trigger and stays unbuilt.

use crate::keeper::Plan;

/// One trigger: the kind of `went` it reads, how many make it, and what it says.
pub struct Rule {
    pub id: &'static str,
    pub went: &'static str,
    pub at: u32,
    pub says: &'static str,
}

pub const RULES: [Rule; 6] = [
    Rule { id: "check-twice", went: "check", at: 2, says: "the same check failed twice" },
    Rule { id: "blocked", went: "blocked", at: 1, says: "it is blocked" },
    Rule { id: "unsure", went: "weak", at: 1, says: "its report is unsure or leaves a point open" },
    Rule { id: "unchecked-twice", went: "unchecked", at: 2, says: "two claims of its reports could not be checked" },
    Rule { id: "concern-repeats", went: "repeat", at: 1, says: "a reviewer's concern came back after it was sent back" },
    Rule { id: "stuck", went: "stuck", at: 1, says: "the main lane was told it is stuck" },
];

/// A rule that holds for an item, and what shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trigger {
    pub rule: &'static str,
    pub item: String,
    pub says: &'static str,
    /// The Log events that show it (`e<seq> <kind>: <what>`).
    pub proof: Vec<String>,
    /// Names this occurrence: a replan is started once for it.
    pub key: String,
}

/// The triggers that hold for `plan` and have not been answered by a replan. A plan with nothing left to do has none.
pub fn triggers(plan: &Plan) -> Vec<Trigger> {
    if !plan.open() {
        return Vec::new();
    }
    // a part the part runner stopped is the operator's to decide, not a planner's
    let held = |i: &crate::keeper::Item, r: &Rule| i.went.get(r.went).copied().filter(|n| *n >= r.at && (r.went != "blocked" || (i.state == "blocked" && !matches!(i.block.as_str(), "parts" | "unproven"))));
    let all = plan.items.iter().flat_map(|i| RULES.iter().filter_map(move |r| held(i, r).map(|n| Trigger { rule: r.id, item: i.id.clone(), says: r.says, proof: i.proof.clone(), key: format!("{}:{}:{n}:r{}", r.id, i.id, plan.revs) })));
    all.filter(|t| !plan.handled.contains(&t.key)).collect()
}
