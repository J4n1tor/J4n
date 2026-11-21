//! What the rule `stuck` counts: the fold `failing` is the commands that failed in a row, and two lines that failed are
//! not attempts of the doer's own: a line the Gate refused (the doer was told no, and nothing ran) and the first run of the project's
//! tests before the task changed anything (it is red because the task is about making it green). The engine marks such a `run.shell`
//! with `uncounted`, and the fold skips it: it neither counts nor starts the count again.

use crate::gate::danger::SAFE_RUNNERS;

/// The shell lines of one task, as far as they tell a failure from an attempt.
#[derive(Default)]
pub(super) struct Ran {
    /// The lines the Gate refused whose result has not come yet.
    blocked: Vec<String>,
    /// The task said it changed something.
    changed: bool,
    /// A run of the tests was seen.
    tested: bool,
}

impl Ran {
    /// The Gate said no to this line.
    pub(crate) fn refuse(&mut self, cmd: &str) {
        self.blocked.push(cmd.trim().to_string());
    }

    pub(crate) fn change(&mut self) {
        self.changed = true;
    }

    /// Why a result is not a failed attempt: `refused`, or `first test`; `None` for every other line, and for one that worked.
    pub(crate) fn uncounted(&mut self, cmd: &str, exit: i32) -> Option<&'static str> {
        let cmd = cmd.trim();
        if let Some(at) = self.blocked.iter().position(|b| b == cmd) {
            self.blocked.remove(at);
            return (exit != 0).then_some("refused");
        }
        let first = !self.tested && test_run(cmd);
        self.tested |= first;
        (first && !self.changed && exit != 0).then_some("first test")
    }
}

/// A line that runs the project's tests: a test runner, alone or after `cd x &&`, an `env` word or `uv run`.
pub(super) fn test_run(cmd: &str) -> bool {
    cmd.split(['&', ';', '|']).any(|part| {
        let words: Vec<&str> = part.split_whitespace().skip_while(|w| *w == "cd" || w.contains('=') || *w == "uv" || *w == "run").collect();
        let line = words.join(" ");
        SAFE_RUNNERS.iter().filter(|r| r.contains("test") || r.contains("unittest")).any(|r| line == *r || line.starts_with(&format!("{r} ")))
    })
}
