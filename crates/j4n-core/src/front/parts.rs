//! The seam of the part runner. A part of the operator's document is an item of the plan; when
//! its turn is over the front asks the runner before it marks the item merged, and the runner takes it through its gate and its commit with
//! tasks of its own. With no runner, or the switch off, the front is exactly what it was.

use crate::keeper::{Item, Plan};
use crate::task::Row;
use serde_json::Value as Json;

/// What the runner may do through the front: read a task, start one in the thread.
pub trait Hands {
    fn row(&self, task: &str) -> Option<Row>;
    fn start(&self, doer: &str, goal: &str, input: Json) -> Result<String, String>;
}

/// What came of an item whose turn is over.
pub enum Taken {
    /// Not a part: the front does what it always did.
    Not,
    /// A part on its way (its gate, its commit, or the operator's word): the item stays as it is.
    Wait,
    /// The part is over: the item's new state, its result and why it is blocked.
    Settle { state: &'static str, result: String, block: &'static str },
}

/// How a part's turn starts: in a new session or not, shrunk first or not, and the Brief's section `parts`.
pub struct Boundary {
    pub fresh: bool,
    pub compact: bool,
    pub brief: String,
}

pub trait Parts: Send + Sync {
    /// The switch, as it stands now.
    fn on(&self) -> bool;
    /// `item` is a part of a run the operator started with `:parts go`.
    fn part(&self, thread: &str, item: &Item) -> bool;
    /// The turn of `item` is over (`work` is its row).
    fn ended(&self, thread: &str, item: &Item, work: &Row, hands: &dyn Hands) -> Taken;
    /// The item is about to start: how, when it is a part.
    fn boundary(&self, thread: &str, item: &Item) -> Option<Boundary>;
    /// `:parts …`: the words to say, or why not; `plan` is the thread's plan as it stands.
    fn command(&self, thread: &str, args: &[String], plan: Option<&Plan>) -> Result<String, String>;

    /// Take the settings as an edit left them: the numbers it keeps of them; by default it keeps none.
    fn tune(&self, _settings: &crate::setting::Settings) {}
}

impl super::Front {
    /// The plan of the thread is a run the operator started with `:parts go`: its items are the operator's lines, so it steps and
    /// reports under every word of `lanes`, `single` too. A plan from a typed line or `/plan` does not: there `single` leaves it to the operator.
    pub(super) fn part_run(&self, thread: &str) -> bool {
        let cfg_ = self.0.cfg();
        let Some(parts) = cfg_.parts.as_ref().filter(|p| p.on()) else { return false };
        self.0.keeper.as_ref().and_then(|k| k.plan(thread)).is_some_and(|plan| plan.items.iter().any(|i| parts.part(thread, i)))
    }
}
