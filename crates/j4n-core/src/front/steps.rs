//! J4n's own steps on Main's thread under `orchestrate`: the project's check of a change Main made (the doer `check`), the review
//! of that change (a `review` branch of Main's turn), and any branch there whose doer cannot carry on in a session. None of them takes a steer, so a line
//! typed while only they work is no steer: it is an order to Main. The steps are ended first (a new turn of Main's changes the tree they read, so what
//! they would say is of a tree that is gone) and the line goes on as any line of Main's. While Main's own model turn works, a line steers it, as before.
//!
//! Two words act on the steps alone, typed in full (a word, not a key, so nothing ends a check by accident): `skip` ends the step that runs, and the next
//! one still comes (a skipped check is followed by its review); `stop` ends it and every step still to come for that change. Ended work is marked
//! unchecked in the Log (`check.skipped`, `review.skipped`, `unchecked: true`) and in the chat's row, never passed. The composer shows the words while
//! a step can be ended (`composer:strip`'s `skip`).

use super::{Front, Said};
use crate::change::Actor;
use crate::task::{Ctl, HUMAN, Row};
use serde_json::{Value as Json, json};

/// The word that ends the step that runs; the next step still comes.
pub const SKIP: &str = "skip";
/// The word that ends the step that runs and the steps still to come for that change.
pub const STOP: &str = "stop";
/// The words the composer shows while a step can be ended, in the order it shows them.
pub const WORDS: [&str; 2] = [SKIP, STOP];

/// How a step was ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum How {
    /// `skip`: this step only.
    Skip,
    /// `stop` or `:stop`: this step and the ones to come for the change.
    Stop,
    /// A line typed for Main: the steps end and the line goes on.
    Line,
}

impl How {
    fn word(self) -> &'static str {
        match self {
            How::Skip => SKIP,
            How::Stop => STOP,
            How::Line => "line",
        }
    }
}

/// The word of a step: what the row and the hint call it.
fn what(r: &Row) -> &'static str {
    if r.spec.doer == "check" { "check" } else if r.spec.role.as_deref() == Some("review") { "review" } else { "step" }
}

/// The turn of Main a step is about: a review's parent, a check's `check what t28 changed`.
fn of(r: &Row) -> String {
    r.spec.parent.clone().or_else(|| r.spec.goal.strip_prefix("check what ").and_then(|g| g.split_whitespace().next()).map(str::to_string)).unwrap_or_default()
}

impl Front {
    /// Whether a task is a step of J4n's own: the check of a change (an envoy's watch has the role `check` and is no step), a review, or a branch of a
    /// turn whose doer cannot carry on in a session (a steer of it is refused). A chore and Main's own turn are not.
    fn own_step(&self, r: &Row) -> bool {
        let role = r.spec.role.as_deref();
        (r.spec.doer == "check" && role.is_none())
            || role == Some("review")
            || (r.spec.parent.is_some() && role != Some("chore") && !self.0.doers.iter().any(|d| d.id() == r.spec.doer && d.caps().resume))
    }

    /// The steps of J4n's own that work on Main's thread now, oldest first; none when the switch is not `orchestrate`, the thread is not Main's, or a turn
    /// of Main's own works (a line steers that turn). A question to the operator is no step and hides none.
    pub fn own_steps(&self, thread: &str) -> Vec<Row> {
        if !self.main_works() || thread != self.0.cfg().thread {
            return Vec::new();
        }
        let live: Vec<Row> = self.0.tasks.book().rows().into_iter().filter(|r| r.spec.thread.as_deref() == Some(thread) && r.spec.doer != HUMAN && !r.state.is_terminal()).collect();
        if live.iter().any(|r| !self.own_step(r)) {
            return Vec::new();
        }
        live
    }

    /// What the composer is told while a step can be ended: the words, and the step they end (`check t50`). Null when none can.
    pub(super) fn steps_hint(&self, thread: &str) -> Json {
        match self.own_steps(thread).last() {
            Some(r) => json!({ "words": WORDS, "what": format!("{} {}", what(r), r.id) }),
            None => Json::Null,
        }
    }

    /// A line typed in Main while only J4n's own steps work: `skip` and `stop` end them and are done; `:stop` is `stop`; a plain line ends them and goes
    /// on (`None`), to be read and worked as Main's. Any other command, and every line while Main's turn works, is not this step's (`None`).
    pub(super) fn step_line(&self, thread: &str, text: &str) -> Option<Said> {
        let steps = self.own_steps(thread);
        if steps.is_empty() {
            return None;
        }
        let word = text.trim().to_ascii_lowercase();
        let how = match word.as_str() {
            SKIP => How::Skip,
            STOP | ":stop" | ":abort" => How::Stop,
            _ if word.starts_with([':', '/', '@']) => return None,
            _ => How::Line,
        };
        // `skip` ends the newest step only; a line or `stop` ends them all
        let ended: Vec<Row> = if how == How::Skip { steps.into_iter().rev().take(1).collect() } else { steps };
        let line = (how == How::Line).then_some(text);
        let mut said = Vec::new();
        for r in &ended {
            if let Err(p) = self.end_step(thread, r, how, line) {
                return Some(self.refuse(thread, p));
            }
            said.push(format!("{} {}", what(r), r.id));
        }
        let ofs: Vec<String> = ended.iter().map(of).filter(|o| !o.is_empty()).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
        let change = if ofs.is_empty() { String::new() } else { format!(": {}'s change is unchecked", ofs.join(", ")) };
        let next = match how {
            How::Skip => "",
            How::Stop => " · no step follows",
            How::Line => " · the line goes to Main",
        };
        self.note(thread, &format!("ended {}{change}{next}", said.join(", ")), "warn");
        match how {
            How::Line => None,
            _ => Some(Said::Command(how.word().into())),
        }
    }

    /// End one step: the Log says so first (the step that runs on its end reads it), then it is stopped.
    fn end_step(&self, thread: &str, r: &Row, how: How, line: Option<&str>) -> Result<(), String> {
        let kind = format!("{}.skipped", if what(r) == "step" { "task" } else { what(r) });
        let mut data = json!({ "task": r.id, "of": of(r), "doer": r.spec.doer, "by": how.word(), "unchecked": true, "follows": how == How::Skip });
        if let Some(l) = line {
            data["line"] = json!(crate::text::clip(l, super::thread::CHAT_LINE));
        }
        self.write(thread, &kind, Actor::Operator, data);
        self.0.tasks.ctl(&r.id, Ctl::Abort, Actor::Operator).map_err(|p| p.msg)
    }

    /// The step that ended `task` (a check or a review), as the Log says it: `(by, follows)`; `None` when no one ended it.
    pub(super) fn step_ended(&self, task: &str) -> Option<(String, bool)> {
        self.0.log.read(0).filter(|e| e.kind.ends_with(".skipped") && e.data.get("task").and_then(Json::as_str) == Some(task)).last().map(|e| (e.data.get("by").and_then(Json::as_str).unwrap_or_default().to_string(), e.data.get("follows").and_then(Json::as_bool).unwrap_or(false)))
    }

    /// Whether the steps still to come for Main's turn `turn` were called off: a step of it was ended by `stop` or by a line.
    pub(super) fn steps_off(&self, turn: &str) -> bool {
        self.0.log.read(0).any(|e| e.kind.ends_with(".skipped") && e.data.get("of").and_then(Json::as_str) == Some(turn) && e.data.get("follows").and_then(Json::as_bool) == Some(false))
    }
}
