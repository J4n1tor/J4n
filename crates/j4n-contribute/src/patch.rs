//! Carried patches: a pack fix is a Proposal whose steps write the operator's own layer over a row of a pack, and
//! the layer above sits over every update, so it is carried without anything being applied again. What a patch needs is to be *seen* and to
//! *retire*: when an update brings the pack row to say what the fix said, the fix is part of J4n and says so, once.

use j4n_core::change::Where;
use j4n_core::proposal::{Proposal, StepOp};
use j4n_core::table::Tables;

/// A proposal whose key starts so is a fix of the person's own to a pack row.
pub const KEY: &str = "fix:";

/// Where a patch stands against the rows that are loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// The operator's layer still says it, and the pack's row does not.
    Carried,
    /// Every row it set now says, below the operator's layer, what the fix said: the pack has it.
    Retired,
    /// The operator's layer no longer holds the rows it set.
    Withdrawn,
}

impl State {
    pub fn word(self) -> &'static str {
        match self {
            State::Carried => "carried",
            State::Retired => "retired",
            State::Withdrawn => "withdrawn",
        }
    }
}

/// Whether a proposal is a patch: a fix by its key, with at least one `Set` step to the operator's layer.
pub fn is_patch(p: &Proposal) -> bool {
    p.draft.key.starts_with(KEY) && p.draft.steps.iter().any(|s| s.to == Where::User && matches!(s.op, StepOp::Set(_)))
}

/// Where an accepted patch stands now. Only the rows loaded say: a patch accepted in this run is not in them until the next start.
pub fn state(p: &Proposal, tables: &Tables) -> State {
    let (mut same, mut gone, mut mine) = (0, 0, 0);
    for s in p.draft.steps.iter().filter(|s| s.to == Where::User) {
        let StepOp::Set(fields) = &s.op else { continue };
        mine += 1;
        match tables.layer_entry(Where::User, &s.kind, &s.id) {
            None => gone += 1,
            Some(_) if fields.iter().all(|(f, v)| tables.field_below(Where::User, &s.kind, &s.id, f) == Some(v)) => same += 1,
            Some(_) => {}
        }
    }
    if mine > 0 && gone == mine {
        State::Withdrawn
    } else if same > 0 && same + gone == mine {
        State::Retired
    } else {
        State::Carried
    }
}
