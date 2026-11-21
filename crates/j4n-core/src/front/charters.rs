//! The front's part of a charter: the draft that waits for the operator's key, the check at the key, the signing the key writes, and the end. The rule that answers
//! is [`crate::charter`]; this is where it meets the Log, the proposals and the tasks.
//!
//! Nothing here lets a charter be made or changed by anyone but the operator: a draft is a proposal (a model or a doer may not draft one at all), the accept is the
//! signing and the only thing that writes `charter.signed`, a change to a signed charter is drafted by the operator alone, and an edit of the row in a file signs nothing.

use super::{Core, Front};
use crate::change::Actor;
use crate::charter::{Charter, ENDED, Hooks, SIGNED};
use crate::log::New;
use crate::proposal::Fate;
use serde_json::json;
use std::sync::Weak;

/// The threads a task is for, nearest first: its own, and those it was started from, as the core wrote them.
pub(super) fn tree(core: &Core, task: &str) -> Vec<String> {
    let book = core.tasks.book();
    let (mut out, mut at) = (Vec::<String>::new(), Some(task.to_string()));
    for _ in 0..8 {
        let Some(row) = at.take().and_then(|id| book.get(&id)) else { break };
        // the envoy a request was made for is marked on Main's task and on the chats it starts for it
        let envoy = row.spec.input.pointer("/envoy/chat").and_then(|p| p.as_str()).map(String::from);
        for t in row.spec.thread.iter().cloned().chain(row.spec.input.get("parent").and_then(|p| p.as_str()).map(String::from)).chain(envoy) {
            if !out.contains(&t) {
                out.push(t);
            }
        }
        at = row.spec.parent;
    }
    out
}

impl Front {
    /// What the rule asks of this program, or none while there is no Gate to ask.
    pub fn charter_hooks(&self) -> Option<Hooks> {
        let gate = self.0.who.gate()?;
        let (core, c1, c2, c3): (Weak<Core>, Weak<Core>, Weak<Core>, Weak<Core>) = (std::sync::Arc::downgrade(&self.0), std::sync::Arc::downgrade(&self.0), std::sync::Arc::downgrade(&self.0), std::sync::Arc::downgrade(&self.0));
        Some(Hooks {
            gate,
            now: Box::new(move || core.upgrade().map_or(0, |c| c.log.clock().now_ms())),
            tree: Box::new(move |task| c1.upgrade().map(|c| tree(&c, task)).unwrap_or_default()),
            spent: Box::new(move |thread| {
                let Some(c) = c2.upgrade() else { return f64::INFINITY };
                let rows = c.tasks.book().rows();
                rows.iter().filter(|r| tree(&c, &r.id).iter().any(|t| t == thread)).map(|r| r.spent.max(r.listed).max(0) as f64 / 1e6).sum()
            }),
            checks: Box::new(move || c3.upgrade().map(|c| c.cfg().check_lines.clone()).unwrap_or_default()),
        })
    }

    /// The rows of the page Tasks › Charters: each charter that was signed, newest first, with where it stands (`live`, `ended` or `over`), what its tree has spent of what it may,
    /// and what it says yes to. Read only: a charter ends by the operator's word or its time, and a change of it is signed again.
    pub fn charter_values(&self) -> Vec<crate::value::Value> {
        use crate::value::Value;
        let now = self.charter_now();
        let rows = self.0.tasks.book().rows();
        let mut signed = self.0.cfg().charters.list();
        signed.sort_by_key(|s| std::cmp::Reverse(s.at_ms));
        signed
            .into_iter()
            .map(|s| {
                let c = &s.charter;
                let spent: f64 = rows.iter().filter(|r| tree(&self.0, &r.id).contains(&c.thread)).map(|r| r.spent.max(r.listed).max(0) as f64 / 1e6).sum();
                let (state, tone, why) = match (&s.ended, c.until_ms) {
                    (Some((_, why)), _) => ("ended", "dim", why.clone()),
                    (None, Some(t)) if now >= t => ("over", "dim", "its time is up".to_string()),
                    _ => ("live", "good", String::new()),
                };
                let until = c.until_ms.map(|t| crate::front::rows::time_of_day(t, None)).unwrap_or_else(|| "until you end it".to_string());
                let text = |s: &str| Value::text(s);
                Value::Map(std::collections::BTreeMap::from([
                    ("id".to_string(), text(&c.id)),
                    ("state".into(), text(state)),
                    ("tone".into(), text(tone)),
                    ("goal".into(), text(&crate::text::line(&c.goal, 120))),
                    ("chat".into(), text(&c.thread)),
                    ("paths".into(), text(&c.paths.join(", "))),
                    ("may".into(), text(&c.may.join(", "))),
                    ("spent".into(), text(&format!("${:.2} of ${:.2}", spent.abs(), c.usd))),
                    ("until".into(), text(&until)),
                    ("why".into(), text(&why)),
                    ("watch".into(), text(&crate::text::line(&c.watch, 120))),
                ]))
            })
            .collect()
    }

    fn charter_now(&self) -> u64 {
        self.0.log.clock().now_ms()
    }

    /// Draft a charter for the operator to sign: the proposal's id. A charter that was signed is changed only by the operator's own draft; any change is a new
    /// signing, so what a charter may do is never wider than what the card last showed. A second charter that meets the path of a live one is refused.
    pub fn charter_draft(&self, by: Actor, c: &Charter) -> Result<String, String> {
        self.charter_draft_from(by, c, None)
    }

    /// [`charter_draft`](Front::charter_draft) for a draft made in the envoy chat `envoy`: the envoy may propose a change to the charter of its own chat, which only
    /// the operator's key in that chat signs.
    pub fn charter_draft_from(&self, by: Actor, c: &Charter, envoy: Option<&str>) -> Result<String, String> {
        let proposer = self.proposer().ok_or("nothing here makes proposals")?;
        let now = self.charter_now();
        if self.0.cfg().charters.holds(&c.id, now) && by != Actor::Operator && envoy != Some(c.thread.as_str()) {
            return Err(format!("charter {} is signed: only the operator changes it, and a change is signed again", c.id));
        }
        if let Some((other, mine, theirs)) = self.0.cfg().charters.clash(c, now) {
            return Err(format!("`{mine}` meets `{theirs}`, which charter {other} holds: two charters may not share a path"));
        }
        let draft = c.draft(now)?;
        proposer.make(draft, by, None).map_err(|p| p.msg)?.ok_or_else(|| "that charter was proposed already".to_string())
    }

    /// At the operator's key on a proposal: the charter it holds, if it holds one, checked again now (a path may have been taken since it was drafted).
    /// `thread` is where the key was pressed: while envoys work, a charter is signed in its own envoy chat and nowhere else.
    pub(super) fn charter_before_accept(&self, id: &str, thread: &str) -> Result<Option<(Charter, String)>, String> {
        let Some(proposer) = self.proposer() else { return Ok(None) };
        proposer.log_sync();
        let Ok(p) = proposer.proposals().pick(id) else { return Ok(None) };
        let Some(step) = p.draft.steps.iter().find(|s| s.kind == <Charter as crate::table::Kind>::NAME) else { return Ok(None) };
        if p.fate != Fate::Open {
            return Ok(None);
        }
        let crate::proposal::StepOp::Set(row) = &step.op else { return Err("a charter proposal that does not write the charter".into()) };
        let c = Charter::of_row(&step.id, row)?;
        let now = self.charter_now();
        c.check(now)?;
        if self.envoys_on() && c.thread != thread {
            return Err(format!("charter {} is signed in {}, where the operator and its envoy talk: go in and press the key there", c.id, c.thread));
        }
        if let Some((other, mine, theirs)) = self.0.cfg().charters.clash(&c, now) {
            return Err(format!("`{mine}` meets `{theirs}`, which charter {other} holds: two charters may not share a path"));
        }
        Ok(Some((c, p.id)))
    }

    /// The operator's key was pressed on a charter's proposal and it was applied: the signing, written by the operator with the content the card showed. The words
    /// it adds to the note.
    pub(super) fn charter_signed(&self, c: &Charter, proposal: &str) -> String {
        let data = json!({ "charter": c, "proposal": proposal, "signer": "operator" });
        match self.0.log.append(New::of(SIGNED, Actor::Operator, data)) {
            Ok(_) => {
                self.0.log.sync();
                format!(" · charter {} is signed: it answers for {} inside {}, and you when it is outside", c.id, c.thread, c.paths.join(", "))
            }
            Err(e) => format!(" · the signing was not written ({e}): the charter answers nothing"),
        }
    }

    /// The operator ends a charter: it answers nothing from then on. Anyone else is refused.
    pub fn charter_end(&self, by: &Actor, id: &str) -> Result<(), String> {
        if *by != Actor::Operator {
            return Err("only the operator ends a charter".into());
        }
        if !self.0.cfg().charters.holds(id, self.charter_now()) {
            return Err(format!("there is no charter {id} that answers"));
        }
        self.0.log.append(New::of(ENDED, Actor::Operator, json!({ "id": id }))).map_err(|e| e.to_string())?;
        self.0.log.sync();
        Ok(())
    }

    /// The act that mends a refusal to sign somewhere else: go into the charter's own chat.
    pub(super) fn charter_lead(&self, id: &str) -> Option<super::Lead> {
        let proposer = self.proposer()?;
        let p = proposer.proposals().pick(id).ok()?;
        let step = p.draft.steps.iter().find(|s| s.kind == <Charter as crate::table::Kind>::NAME)?;
        let crate::proposal::StepOp::Set(row) = &step.op else { return None };
        let c = Charter::of_row(&step.id, row).ok()?;
        Some(super::Lead { label: format!("go into {}", c.thread), intent: j4n_proto::Intent::Enter { chat: c.thread }, risk: 0 })
    }
}
