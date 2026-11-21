//! A session's life: the setting `session.mode` says when a line carries on in the doer's session
//! and when it starts a new one. `resume` always carries on, `auto` starts a new one when carrying on would be cold and the
//! session holds more than `session.big` tokens, `each` starts one for every line the operator types. A new session is handed a short
//! carry-over made by code from the Log, never the old transcript; a row in the chat says why, and `:resume` takes the old one back.

use super::Front;
use crate::change::Actor;
use crate::task::context::k;
use crate::task::{Row, SessionRef, State, TaskSpec};
use crate::text::{clip, line};
use serde_json::json;
use std::collections::BTreeMap;

/// What a session's life is, as `session.mode` says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Life {
    Resume,
    #[default]
    Auto,
    Each,
}

impl Life {
    pub fn from_word(word: &str) -> Option<Life> {
        Some(match word {
            "resume" => Life::Resume,
            "auto" => Life::Auto,
            "each" => Life::Each,
            _ => return None,
        })
    }

    pub fn word(self) -> &'static str {
        match self {
            Life::Resume => "resume",
            Life::Auto => "auto",
            Life::Each => "each",
        }
    }
}

/// What the carry-over holds at most: the last answers (and the line each answered), the plan line, the paths changed, and the whole.
pub const CARRY_ANSWERS: usize = 3;
pub const CARRY_ANSWER: usize = 400;
pub const CARRY_GOAL: usize = 160;
pub const CARRY_PLAN: usize = 300;
pub const CARRY_FILES: usize = 20;
pub const CARRY_PATH: usize = 120;
pub const CARRY_ALL: usize = 2400;

/// The sessions a thread has left and the one the operator asked for back, per chat.
#[derive(Default)]
pub(super) struct Moves {
    /// The session the last change left, until the next line is started.
    left: BTreeMap<String, SessionRef>,
    /// `:resume` was said: the next line carries on in this session, once.
    back: BTreeMap<String, SessionRef>,
    /// A `:compact` said while a task ran: it is made at the next settled boundary.
    pub(super) queued: BTreeMap<String, super::compact::Ask>,
}

impl Front {
    /// The session of the last turn that was with this doer carries on, whatever model it now runs on, and whatever else (a flow the operator
    /// started with `/plan`) was done in the thread since; `:fresh` ends it. Unless `session.mode` says the line starts a new one, and `:resume`
    /// was not said. `operator`: the line is the operator's own, not a plan item or a boundary's. Whether the turn is in a session.
    pub(super) fn follow(&self, thread: &str, doer: &str, spec: &mut TaskSpec, operator: bool) -> bool {
        // a session is a login's: a turn that runs as another login does not carry on in it
        let own: Vec<_> = self.own_tasks(thread, doer).into_iter().filter(|r| r.spec.account == spec.account).collect();
        let Some(newest) = own.iter().find(|r| r.session.is_some()) else { return false };
        let mut moves = self.0.moves.lock().unwrap_or_else(|e| e.into_inner());
        moves.left.remove(thread);
        let back = moves.back.remove(thread).filter(|s| s.doer == doer);
        let asked = back.as_ref().and_then(|s| own.iter().find(|r| r.session.as_ref() == Some(s)));
        let prev = asked.unwrap_or(newest);
        if asked.is_none()
            && let Some(why) = self.cut(thread, doer, prev, operator)
        {
            moves.left.extend(prev.session.clone().map(|s| (thread.to_string(), s)));
            drop(moves);
            self.changed(thread, doer, spec, prev, &why);
            return false;
        }
        spec.follows = Some(prev.id.clone());
        spec.resume = prev.session.clone();
        true
    }

    /// Why the line starts a new session instead of carrying on in `prev`'s, by `session.mode`; none: it carries on. The cold and the size are
    /// what `/context` says (`task::context`), and a provider whose cache life is not said is never cold.
    fn cut(&self, thread: &str, doer: &str, prev: &Row, operator: bool) -> Option<String> {
        let mode = self.0.cfg().session;
        if mode == Life::Resume {
            return None;
        }
        if mode == Life::Each && operator {
            return Some("every line its own (session.mode each)".into());
        }
        let id = &prev.session.as_ref()?.id;
        let s = self.live_sessions().into_iter().find(|s| s.thread == thread && s.doer == doer && s.id == *id)?;
        let cold = s.cold(self.log().clock().now_ms(), self.0.tasks.plans().cache_ms(doer)) == Some(true);
        (cold && !s.unsure && s.held > self.0.cfg().session_big).then(|| format!("cold and {} held (session.mode {})", k(s.held), mode.word()))
    }

    /// A new session begins: the row in the chat says why, the Log has it, and the line is handed the carry-over in its text (a doer that
    /// takes a Brief, in the section `carry`).
    fn changed(&self, thread: &str, doer: &str, spec: &mut TaskSpec, prev: &Row, why: &str) {
        let text = self.carry(thread, prev);
        self.note(thread, &format!("new session: {why} · :resume to keep the old one"), "dim");
        let from = prev.session.as_ref().map(|s| s.id.as_str());
        self.write(thread, "session.changed", Actor::Core, json!({ "doer": doer, "from": from, "why": why, "mode": self.0.cfg().session.word(), "carry": text.chars().count() }));
        spec.input["carry"] = json!(text);
    }

    /// What the new session is told of the old one, from the Log's tasks and the plan, never the transcript: the last answers as the ledger keeps
    /// them (`Row.answer`, with the line each answered), the open plan line, and the files the session changed. Capped.
    fn carry(&self, thread: &str, prev: &Row) -> String {
        let cut = self.0.binds.cut(thread);
        let rows = self.0.tasks.book().rows();
        let mine = rows.iter().rev().take_while(|r| Some(&r.id) != cut.as_ref()).filter(|r| r.spec.thread.as_deref() == Some(thread) && r.spec.parent.is_none() && r.spec.doer != crate::task::HUMAN);
        // a planning turn's answer is the plan, which the plan line says
        let said = mine.clone().filter(|r| !matches!(r.spec.role.as_deref(), Some("plan") | Some(crate::task::COMPACT_ROLE)));
        // every piece is scrubbed before it is cut: a secret cut in half is not one the redactor knows (as the reports of branches are scrubbed whole)
        let scrub = |t: &str| self.0.log.redactor().scrub(t);
        let mut out = vec!["Carried over from the session before this one, from the Log (not its transcript):".to_string()];
        let mut answers: Vec<String> = said.filter(|r| r.state == (State::Done { ok: true })).filter_map(|r| Some(format!("- you: {} → {}", line(&scrub(&r.spec.goal), CARRY_GOAL), line(&scrub(r.answer.as_deref().filter(|a| !a.trim().is_empty())?), CARRY_ANSWER)))).take(CARRY_ANSWERS).collect();
        answers.reverse();
        if !answers.is_empty() {
            out.push(format!("Last answers:\n{}", answers.join("\n")));
        }
        if let Some(plan) = self.plan_line(thread) {
            out.push(format!("Plan: {}", line(&scrub(&plan), CARRY_PLAN)));
        }
        let id = prev.session.as_ref().map(|s| &s.id);
        let mut files: Vec<String> = Vec::new();
        for r in mine.filter(|r| r.session.as_ref().map(|s| &s.id) == id) {
            for p in r.changed.created.iter().chain(&r.changed.modified) {
                if !files.contains(p) {
                    files.push(p.clone());
                }
            }
        }
        if !files.is_empty() {
            out.push(format!("Files changed: {}", files.iter().take(CARRY_FILES).map(|p| clip(&scrub(p), CARRY_PATH)).collect::<Vec<_>>().join(", ")));
        }
        clip(&out.join("\n"), CARRY_ALL)
    }

    /// `:resume`: the next line carries on in the session the last change left, or, when none was left, in the one there is even if `auto` would
    /// have cut it. Once; `:fresh` forgets it.
    pub(super) fn resume_said(&self, thread: &str) -> super::Said {
        let said = super::Said::Command("resume".into());
        let Some(b) = self.binding(thread) else { return self.refuse(thread, "not bound yet: no session to take back".into()) };
        let mut moves = self.0.moves.lock().unwrap_or_else(|e| e.into_inner());
        let old = moves.left.get(thread).filter(|s| s.doer == b.doer).cloned().or_else(|| self.own_tasks(thread, &b.doer).into_iter().find_map(|r| r.session));
        let Some(old) = old else {
            drop(moves);
            return self.refuse(thread, "no session of this doer to take back".into());
        };
        moves.back.insert(thread.to_string(), old.clone());
        drop(moves);
        self.note(thread, &format!("the next line carries on in session {} (once)", clip(&old.id, 8)), "dim");
        said
    }

    /// `:fresh`: what `:resume` could take back is gone with the session.
    pub(super) fn forget_moves(&self, thread: &str) {
        let mut moves = self.0.moves.lock().unwrap_or_else(|e| e.into_inner());
        moves.left.remove(thread);
        moves.back.remove(thread);
        moves.queued.remove(thread);
    }
}
