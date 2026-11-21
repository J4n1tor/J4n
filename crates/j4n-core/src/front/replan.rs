//! The approach, changed: a trigger of [`replan::RULES`](crate::replan::RULES) starts a planning turn in
//! the main lane's session that is told the reason and the evidence by Log id, and its answer changes the items not yet started. Done work
//! is kept: a replan that would drop a merged item, add more than `plan.grow` items, or is past `plan.revisions` is the operator's to
//! decide, held as `plan.pending` until `:plan accept` or `:plan reject`. Running branches whose item is dropped are stopped with
//! their reports kept.

use super::{Front, Said};
use crate::branch::Switch;
use crate::change::Actor;
use crate::keeper::{How, Op, Plan, Shift, take_with};
use crate::replan::triggers;
use crate::task::Ctl;
use serde_json::json;

impl Front {
    /// Start a replan if a trigger holds, the main lane is free and the plan is not waiting for the operator. Past `plan.revisions` it asks
    /// instead and stops. One replan answers every trigger that holds when it starts.
    pub(super) fn plan_replan(&self, thread: &str) {
        let Some(plan) = self.0.keeper.as_ref().and_then(|k| k.plan(thread)).filter(|p| p.asked.is_none() && p.pending.is_none()) else { return };
        let found = triggers(&plan);
        // with `single` the plan is driven by the operator's lines; a busy main lane cannot take a planning turn, and the trigger stands
        // a plan that is only the chats Main started has no approach to change: their blocks are Main's to settle (digest)
        let chats_only = plan.items.iter().all(|i| self.is_chat_item(i));
        if found.is_empty() || self.lanes_now() == Switch::Single || chats_only || self.settled(thread).is_none() {
            return;
        }
        let Some(b) = self.binding(thread) else { return };
        let keys: Vec<String> = found.iter().map(|t| t.key.clone()).collect();
        let reason = found.iter().map(|t| format!("{}: {} ({})", t.item, t.says, t.rule)).collect::<Vec<_>>().join("; ");
        let mut evidence: Vec<String> = found.iter().flat_map(|t| t.proof.clone()).collect();
        evidence.extend(found.iter().filter_map(|t| plan.items.iter().find(|i| i.id == t.item)).filter(|i| !i.task.is_empty()).map(|i| format!("{} was worked by task {}", i.id, i.task)));
        evidence.dedup();
        if plan.revs >= self.0.cfg().revisions {
            let why = format!("the approach was changed {} times (plan.revisions {}); another is yours to decide — {reason}", plan.revs, self.0.cfg().revisions);
            self.write(thread, "plan.pending", Actor::Core, json!({ "kind": "revisions", "why": why, "text": reason, "evidence": evidence, "keys": keys }));
            self.note(thread, &format!("{why}. `:plan accept` changes the approach once more, `:plan reject` leaves the plan as it is"), "warn");
            return;
        }
        let said = self.plan_turn_with(thread, &reason, How::Replan, &b, &evidence, &keys);
        if matches!(said, Said::Started { .. }) {
            self.note(thread, &format!("changing the approach: {reason}"), "accent");
        }
    }

    /// A replan that would drop merged work or add more than `plan.grow` items is held for the operator. `true`: it is held.
    pub(super) fn replan_held(&self, thread: &str, text: &str, asked: &crate::keeper::Asked, shift: &Shift, micros: i64) -> bool {
        let (kind, why) = if !shift.dropped_done.is_empty() {
            ("drops-merged", format!("the new approach drops merged work ({}); done work is kept unless you say so", shift.dropped_done.join(", ")))
        } else if shift.added > self.0.cfg().grow as usize {
            ("grow", format!("the new approach adds {} items, more than plan.grow ({})", shift.added, self.0.cfg().grow))
        } else {
            return false;
        };
        self.write(thread, "plan.pending", Actor::Core, json!({ "kind": kind, "why": why, "text": text, "evidence": asked.evidence }));
        self.write(thread, "plan.settled", Actor::Core, json!({ "task": asked.task, "ok": false, "why": "held for the operator", "micros": micros }));
        self.note(thread, &format!("changed approach, not taken yet: {why}. `:plan accept` takes it, `:plan reject` keeps the plan as it is"), "warn");
        true
    }

    /// Write what a plan answer changes, stop the branches of the items it drops (their reports are kept), and say so.
    pub(super) fn plan_apply(&self, thread: &str, plan: &Plan, task: &str, ops: &[Op], shift: &Shift, micros: i64) {
        for op in ops {
            let (kind, data) = op.event(task);
            self.write(thread, kind, Actor::Core, data);
        }
        if !task.is_empty() {
            self.write(thread, "plan.settled", Actor::Core, json!({ "task": task, "ok": true, "changes": ops.len(), "micros": micros, "folded": shift.folded, "freed": shift.freed }));
        }
        if !shift.freed.is_empty() {
            self.note(thread, &format!("plan: no wait between items that share no file ({})", shift.freed.join(", ")), "dim");
        }
        if !shift.folded.is_empty() {
            self.note(thread, &format!("plan: folded {} to keep it within {} items ({})", shift.folded.len(), shift.bound, shift.folded.join(", ")), "dim");
        }
        let book = self.0.tasks.book();
        let mut stopped = Vec::new();
        for i in plan.items.iter().filter(|i| shift.dropped_live.contains(&i.id)) {
            for t in [&i.task, &i.side].into_iter().filter(|t| book.get(t).is_some_and(|r| !r.state.is_terminal())) {
                let _ = self.0.tasks.ctl(t, Ctl::Abort, Actor::Core);
            }
            // a running branch ends with a report that is kept for the main lane's next Brief
            if i.state == "running" && i.place != "inline" && !i.task.is_empty() {
                let _ = self.0.tasks.sync();
                self.report(&i.task);
            }
            stopped.push(i.id.clone());
        }
        let why = ops.iter().find_map(|o| if let Op::Approach { reason, rev, .. } = o { (*rev > 0).then(|| reason.clone()) } else { None });
        let text = match why {
            Some(why) if stopped.is_empty() => format!("changed approach: {why}"),
            Some(why) => format!("changed approach: {why} (stopped {}, their reports are kept)", stopped.join(", ")),
            None => {
                let now = self.0.keeper.as_ref().and_then(|k| k.plan(thread)).map(|p| p.items.len()).unwrap_or_default();
                format!("the plan has {now} item(s); `:plan` shows it")
            }
        };
        self.note(thread, &text, "accent");
    }

    /// `:plan accept` / `:plan reject`: the operator's word on the replan that waits.
    pub(super) fn replan_decide(&self, thread: &str, plan: &Plan, accept: bool) -> Said {
        let said = Said::Command("plan".into());
        let Some(p) = plan.pending.clone() else { return said };
        self.write(thread, "plan.pending", Actor::Operator, json!({ "clear": true }));
        if !accept {
            self.note(thread, "the plan stays as it is", "dim");
            return said;
        }
        if p.kind == "revisions" {
            return match self.binding(thread) {
                Some(b) if self.settled(thread).is_some() => self.plan_turn_with(thread, &p.text, How::Replan, &b, &p.evidence, &[]),
                _ => self.refuse(thread, "the main lane is busy: say `:plan accept` again when it is free".into()),
            };
        }
        match take_with(&p.text, Some(plan), How::Replan, "", &p.evidence) {
            Ok((mut ops, shift)) => {
                self.bound_checks(thread, &mut ops);
                self.plan_apply(thread, plan, "", &ops, &shift, 0);
            }
            Err(why) => self.note(thread, &format!("the held approach no longer fits the plan: {why}"), "warn"),
        }
        said
    }
}
