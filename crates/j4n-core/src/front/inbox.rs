//! The seam of the prompt inbox. A line typed while a task is not over is asked of the inbox before the
//! front steers or refuses it, and at the settled boundary it is asked what to say to the next turn; the pack `inbox` (the crate `j4n-inbox`) tags it, writes its receipt and says what it did. With no inbox, or the
//! switch off, the front is exactly what it was.

/// A line typed over a task that is not over.
pub struct Over<'a> {
    pub thread: &'a str,
    pub text: &'a str,
    pub task: &'a str,
    /// The state word of the task (`running`, `cooling`, `queued`, `waiting`).
    pub state: &'static str,
    /// What the line is compared with: the task's goal and origin, the paths it changed, the item of its own list in hand.
    pub context: String,
}

/// What the inbox did with a line: its item, the tag, and the words to say.
pub struct Kept {
    pub id: String,
    pub tag: &'static str,
    pub note: String,
    /// A `steer` item of a rule that has earned it, in `auto`: the front sends it into the task at once if the task can take it.
    pub now: bool,
}

/// What waits to be said to a model in the next turn (the items are claimed until `sent` or `back`).
pub struct Due {
    pub ids: Vec<String>,
    /// The Brief section `inbox`: the lines as typed, numbered.
    pub text: String,
    /// The line of the turn for a doer that takes a Brief, and for one that does not (the numbered lines are in it).
    pub line: String,
    pub plain: String,
}

pub trait Inbox: Send + Sync {
    /// The switch, as it stands now.
    fn on(&self) -> bool;
    /// Tag the line and write its item; `None`: the line is not the inbox's (a task that waits on an ask refuses it as before).
    fn take(&self, over: &Over) -> Option<Kept>;
    /// What became of an item the front acted on: `stopped-task`, or `dropped`.
    fn settled(&self, id: &str, state: &str);
    /// The `steer` and `hold` items and the pinned notes of the thread. At the boundary (`work`) only when a line is among them.
    fn due(&self, thread: &str, work: bool) -> Option<Due>;
    /// The oldest `side` item, claimed.
    fn side(&self, thread: &str) -> Option<(String, String)>;
    /// The receipt of items said to a turn (`delivered`) or started as one (`started`).
    fn sent(&self, ids: &[String], state: &str, task: &str);
    /// The turn did not start: the items are free again.
    fn back(&self, ids: &[String]);
    /// `:inbox …`: the words to say, or why not.
    fn command(&self, thread: &str, args: &[String]) -> Result<String, String>;

    /// Take the settings as an edit left them: the numbers it keeps of them; by default it keeps none.
    fn tune(&self, _settings: &crate::setting::Settings) {}
}
