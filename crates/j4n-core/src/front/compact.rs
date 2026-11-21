//! `:compact [junk · keep <words>]`: the operator shrinks a chat's session by hand. At a settled boundary
//! it shrinks now; while a task runs it is queued and made at the next boundary (law 9: never mid-turn). The shrinking is the loop's own
//! (`task.compacting`), a task that does only that (`task::COMPACT_ROLE`), so a doer that can shrink a session does it the way it does
//! for a rule, and one that cannot says so and nothing is faked. The modes are rows of data (the kind `compact_mode`, `packs/base`): the words
//! each hands to the doer's compaction, beside what must stay.

use super::thread::Binding;
use super::{Front, Said};
use crate::compact::HOW_CHARS;
use crate::table::Kind;
use crate::task::{COMPACT_ROLE, Row, State};
use crate::text::clip;
use serde::Deserialize;
use serde_json::json;

/// A way to shrink a session, as one row of the pack says it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeRow {
    pub id: String,
    #[serde(default)]
    pub doc: String,
    /// What is handed to the doer's compaction; `{words}` is where the operator's own words go.
    #[serde(default)]
    pub words: String,
    /// The mode needs words of the operator's (`keep <words>`).
    #[serde(default)]
    pub takes: bool,
}

impl Kind for ModeRow {
    const NAME: &'static str = "compact_mode";
}

/// What the operator asked for: the mode and the words handed over.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Ask {
    pub mode: String,
    pub how: String,
}

impl Front {
    fn modes(&self) -> Vec<ModeRow> {
        let tables = &self.screen().tables;
        let mut rows: Vec<ModeRow> = tables.ids(ModeRow::NAME).into_iter().filter_map(|id| tables.get::<ModeRow>(id).ok()).collect();
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        rows
    }

    /// The words after `:compact` as a mode and what it hands over; the usage line when they are none of the modes.
    pub fn compact_ask(&self, args: &[String]) -> Result<Ask, String> {
        let modes = self.modes();
        let usage = || {
            let forms: Vec<String> = modes.iter().filter(|m| !m.words.is_empty()).map(|m| if m.takes { format!("{} <words>", m.id) } else { m.id.clone() }).collect();
            format!("`:compact [{}]`: no such mode", forms.join(" · "))
        };
        let word = args.first().map(|w| w.to_lowercase()).unwrap_or_else(|| "plain".into());
        let Some(row) = modes.iter().find(|m| m.id == word) else { return Err(usage()) };
        let said = args.iter().skip(1).map(String::as_str).collect::<Vec<_>>().join(" ");
        match (row.takes, said.trim().is_empty()) {
            (true, true) => return Err(format!("`:compact {} <words>`: say what it is about", row.id)),
            (false, false) => return Err(usage()),
            _ => {}
        }
        // one line: a line break in the operator's words would let them write a line of the block of their own (a `- goal:`)
        let said = said.replace(['\n', '\r'], " ");
        let how = clip(&row.words.replace("{words}", &self.0.log.redactor().scrub(said.trim())), HOW_CHARS);
        Ok(Ask { mode: row.id.clone(), how })
    }

    /// The newest task of the chat that holds a live session of `doer` (what `:fresh` ended is not looked at).
    fn own_session(&self, thread: &str, doer: &str) -> Option<Row> {
        self.own_tasks(thread, doer).into_iter().find(|r| r.session.is_some())
    }

    fn can_compact(&self, doer: &str) -> bool {
        self.0.doers.iter().any(|d| d.id() == doer && d.caps().compact)
    }

    /// `:compact`: now when the chat is at a settled boundary, at the next one when a task runs. With no session there is nothing to shrink, and
    /// a doer with no way to shrink one is told so and nothing is changed.
    pub(super) fn compact_said(&self, thread: &str, args: &[String]) -> Said {
        let ask = match self.compact_ask(args) {
            Ok(a) => a,
            Err(why) => return self.refuse(thread, why),
        };
        let Some(b) = self.binding(thread) else {
            self.note(thread, "nothing to compact: no line has run in this chat yet", "dim");
            return Said::Command("compact".into());
        };
        if self.own_session(thread, &b.doer).is_none() {
            self.note(thread, &format!("nothing to compact: this chat has no session of {} yet", b.doer), "dim");
            return Said::Command("compact".into());
        }
        if !self.can_compact(&b.doer) {
            return self.refuse(thread, format!("cannot compact: {} has no way to shrink a session, so nothing was changed", b.doer));
        }
        if self.settled(thread).is_none() {
            self.0.moves.lock().unwrap_or_else(|e| e.into_inner()).queued.insert(thread.to_string(), ask);
            self.note(thread, "compact queued: after the running task", "dim");
            return Said::Command("compact".into());
        }
        match self.compact_now(thread, &b, &ask) {
            Ok(task) => Said::Started { task },
            Err(why) => self.refuse(thread, why),
        }
    }

    /// The settled boundary of a chat (the plan's step, at every task's end): a `:compact` that was queued is made now when the task ended well,
    /// before the plan starts the next item; after a stop or a failure it is dropped and the chat says so, the session being left as it is.
    pub(super) fn compact_step(&self, thread: &str) {
        let Some(ask) = self.0.moves.lock().unwrap_or_else(|e| e.into_inner()).queued.get(thread).cloned() else { return };
        let Some(Some(last)) = self.settled(thread) else { return };
        self.0.moves.lock().unwrap_or_else(|e| e.into_inner()).queued.remove(thread);
        if last.state != (State::Done { ok: true }) {
            return self.note(thread, &format!("compact dropped: {} did not end well, and a session is not shrunk after that", last.id), "warn");
        }
        let Some(b) = self.binding(thread) else { return };
        if let Err(why) = self.compact_now(thread, &b, &ask) {
            self.note(thread, &format!("compact dropped: {why}"), "warn");
        }
    }

    /// Start the task that shrinks the chat's session: it follows the newest turn, resumes its session, and asks the doer's compaction with what
    /// the loop knows must stay (the plan line among it), the mode's words, and the operator's.
    fn compact_now(&self, thread: &str, b: &Binding, ask: &Ask) -> Result<String, String> {
        let prev = self.own_session(thread, &b.doer).ok_or_else(|| format!("nothing to compact: this chat has no session of {} yet", b.doer))?;
        if !self.can_compact(&b.doer) {
            return Err(format!("cannot compact: {} has no way to shrink a session, so nothing was changed", b.doer));
        }
        let mut spec = self.spec(thread, &format!("compact the session ({})", ask.mode), &b.doer)?;
        spec.role = Some(COMPACT_ROLE.into());
        spec.tune.model = b.model.clone();
        spec.tune.endpoint = b.endpoint.clone();
        // a session is its login's: it is shrunk as the login that holds it
        spec.account = prev.spec.account.clone();
        spec.follows = Some(prev.id.clone());
        spec.resume = prev.session.clone();
        spec.compact = Some(self.plan_line(thread).into_iter().collect());
        spec.input = json!({ "compact_mode": ask.mode, "compact_how": ask.how });
        self.0.tasks.spawn(spec, super::reflexes::doing()).map_err(|p| p.msg)
    }
}
