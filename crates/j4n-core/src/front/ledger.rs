//! The seam of the result ledger. When nothing of a thread is open the front asks the ledger to say what came
//! of it; the pack `ledger` (the crate `j4n-ledger`) builds the report from the Log, never from a model, and `:report` reads it again. With no
//! ledger, or the switch off, the front is exactly what it was.

pub trait Ledger: Send + Sync {
    /// The switch, as it stands now.
    fn on(&self) -> bool;
    /// Nothing of the thread is open: report what ended since the last report (nothing, when no task of the thread ended).
    fn settled(&self, thread: &str);
    /// `:report [thread] [all]`: the words to say and the same as rows `(depth, text)` for the chat to hang on pipes, or why not.
    fn command(&self, thread: &str, args: &[String]) -> Result<(String, Vec<(u8, String)>), String>;

    /// Take the settings as an edit left them: the numbers it keeps of them; by default it keeps none.
    fn tune(&self, _settings: &crate::setting::Settings) {}
}
