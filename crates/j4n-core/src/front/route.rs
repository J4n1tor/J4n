//! The plan, started: each ready item of the open plan runs inline, as a turn of the main lane in plan
//! order, or as a branch of a class, and a branch is linked to its item. Which one is [`branch::decide`](crate::branch::decide)'s: the
//! planner's mark proposes, the three stay-inline tests (cheaper than a branch, reads what this line wrote, needs a decision only the main lane holds) dispose, and the Log says which test (`plan.routed`).
//!
//! Only item state `todo`/`sent-back → running` is moved here; what follows a branch (`reported … merged`, the send-backs, the caps and
//! the report to the operator) is `validate.rs`'s. A `piece` works in a held worktree. A branch is given no way to start a
//! branch: the loop refuses it (`task/engine.rs`).

use super::thread::Binding;
use super::{Front, Said};
use crate::branch::{Class, Context, Ctx, Switch, Verdict, decide, via};
use crate::change::Actor;
use crate::flow::run::{Elect, MAP_AT_ONCE};
use crate::keeper::{Item, Plan};
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;

/// An item is started this many times before it is `blocked`; what follows a block is the validation and the replan.
pub const TRIES: u32 = 3;

/// Why a chat of a split that writes goes on as a piece.
const CHAT_PIECE: &str = "a chat that writes goes on as a piece: its work is held in a worktree";

/// A chat of a split that writes: a piece by Main's brief, whatever the rules of the switch say.
fn chat_piece(item: &Item) -> bool {
    item.chat_part()
}

/// The words an item is given to work on.
fn text_of(item: &Item) -> String {
    let back = if item.state == "sent-back" { format!("\nSent back ({} of {}): {}", item.sends, crate::branch::SENDS, item.result) } else { String::new() };
    format!("{}\nDone when: {}{back}", item.goal, item.done)
}

impl Front {
    /// Take what has come to the plan, then start what it can start: [`plan_collect`](Front::plan_collect) and [`plan_drive`](Front::plan_drive).
    pub fn plan_step(&self, thread: &str) {
        self.compact_step(thread);
        // a chat that waits out a limit goes on at once as a login that is open
        self.limits_step(thread);
        self.retry_step(thread);
        // the reflexes that check a task's end, off this thread (the judge may take its time)
        self.reflex_step(thread);
        self.plan_collect(thread);
        self.inbox_step(thread);
        self.plan_drive(thread);
        // a lane Main worked itself under `orchestrate` is reviewed on another doer, as a chat is
        self.second_step(thread);
        // what the orchestrator is told of the chat (its turn that ended, its plan), and what Main is told of the chats it started
        self.orch_step(thread);
        self.digest_step(thread);
        self.held_step(thread);
        self.access_main_step(thread);
        self.access_step(thread);
        self.envoy_step(thread);
        self.decided_step(thread);
        // a settled boundary of a chat takes what waits for it, and a message that wakes it starts its turn
        self.mail_step(thread);
        // nothing of the thread is open, so what came of it is reported (the ledger makes nothing when no task of it ended since)
        if let Some(ledger) = self.0.cfg().ledger.as_ref().filter(|l| l.on() && !self.0.tasks.book().open().iter().any(|r| r.spec.thread.as_deref() == Some(thread))) {
            ledger.settled(thread);
        }
    }

    /// The paths this line has written so far: what the started items said they write, and what their turns changed.
    fn written(&self, plan: &Plan) -> BTreeSet<String> {
        let mut paths = BTreeSet::new();
        let book = self.0.tasks.book();
        for i in plan.items.iter().filter(|i| i.locked()) {
            paths.extend(i.writes.iter().cloned());
            if let Some(c) = book.get(&i.task).map(|r| r.changed) {
                paths.extend(c.created.into_iter().chain(c.modified).chain(c.deleted));
            }
        }
        paths
    }

    /// Start every item of the open plan that is ready and has a place to run: branches up to `MAP_AT_ONCE`, and one inline turn when the
    /// main lane is free. With `lanes = single` nothing is started: the plan is driven by the operator's lines, except a
    /// run of parts the operator started with `:parts go`, which steps its own items.
    pub fn plan_drive(&self, thread: &str) {
        let switch = self.lanes_now();
        let Some(keeper) = self.0.keeper.as_ref().filter(|_| switch != Switch::Single || self.part_run(thread)) else { return };
        let _one = self.0.plan_lock.lock().unwrap_or_else(|e| e.into_inner());
        let Some(plan) = keeper.plan(thread).filter(|p| p.open() && p.asked.is_none() && p.pending.is_none()) else { return };
        let Some(b) = self.binding(thread) else { return };
        let busy = self.settled(thread).is_none();
        let ctx = Ctx { switch, rules: self.0.cfg().branches.clone(), written: self.written(&plan), asked: self.0.tasks.book().open_asks().iter().any(|a| a.thread == thread) };
        let started = |i: &&Item| i.state == "running";
        let mut free = !busy && !plan.items.iter().filter(started).any(|i| i.place == "inline");
        let mut reading = plan.items.iter().filter(started).filter(|i| Class::from_word(&i.place).is_some()).count();
        let done = |id: &String| plan.items.iter().any(|i| i.id == *id && i.done());
        let spent = self.line_usd(thread, &plan);
        for item in plan.items.iter().filter(|i| matches!(i.state.as_str(), "todo" | "sent-back") && i.tries < TRIES + i.sends && i.depends.iter().all(done)) {
            // a chat of a split that writes is a piece by Main's brief, not by a rule: sent back, it goes on as one, in a worktree of its own, since its
            // work is held there and this checkout has none of it (no rule, not even the cheap test, sends it inline)
            let mut verdict = if chat_piece(item) { Verdict::Branch { class: Class::Piece, why: CHAT_PIECE.into() } } else { decide(item, &plan, &ctx) };
            if let Verdict::Branch { class, .. } = &verdict {
                let class = *class;
                if spent >= self.0.cfg().usd_line {
                    verdict = Verdict::Inline { test: None, why: format!("the line's branches took ${spent:.2} of the ${:.2} they may take together", self.0.cfg().usd_line) };
                } else if class == Class::Piece && !self.0.tasks.has_place("worktree") {
                    verdict = Verdict::Inline { test: None, why: "a piece needs a held worktree and there is no place `worktree`: it runs inline".into() };
                } else if class == Class::Piece && !crate::place::checkpoint::has_commit(&self.0.cfg().root) {
                    verdict = Verdict::Inline { test: None, why: "a piece needs a held worktree and this folder has no git commit to cut one from: it runs inline".into() };
                } else if reading >= MAP_AT_ONCE {
                    continue;
                } else {
                    match self.branch_start(thread, item, class, &plan, &ctx, &b) {
                        Ok(()) => {
                            reading += 1;
                            continue;
                        }
                        Err(why) => verdict = Verdict::Inline { test: None, why: format!("no {} started: {why}", class.word()) },
                    }
                }
            }
            // inline items run one at a time, in plan order: while the first cannot start, none after it does
            if !std::mem::take(&mut free) {
                continue;
            }
            self.inline_start(thread, item, &verdict, &b);
        }
    }

    fn routed(&self, thread: &str, item: &Item, place: &str, verdict: &Verdict, task: &str, extra: Json) {
        let mut data = json!({ "id": item.id, "place": place, "why": verdict.why(), "test": verdict.test().map(|t| t.word()), "task": task, "mark": item.mark });
        if let (Some(d), Some(e)) = (data.as_object_mut(), extra.as_object()) {
            d.extend(e.clone());
        }
        self.write(thread, "plan.routed", Actor::Core, data);
        let by = if task.is_empty() { String::new() } else { format!(" ({task})") };
        self.note(thread, &format!("{} → {place}{by}: {}", item.id, verdict.why()), "dim");
    }

    fn inline_start(&self, thread: &str, item: &Item, verdict: &Verdict, b: &Binding) {
        // refused: the note says why, and the item stays what it was
        // a part starts as its runner says (in session, shrunk past `compact.at`, or a new session) with the section `parts`
        let said = match self.0.cfg().parts.as_ref().filter(|p| p.on()).and_then(|p| p.boundary(thread, item)) {
            Some(how) => self.begin(thread, &text_of(item), b, &[], super::Turn { item: false, compact: how.compact, fresh: how.fresh, carry: None, operator: false, brief: Some(how.brief), input: None }),
            None => self.turn_as(thread, &text_of(item), b, &[], (false, true, false)),
        };
        if let Said::Started { task } = said {
            self.routed(thread, item, "inline", verdict, &task, json!({ "via": "" }));
            self.write(thread, "plan.item", Actor::Core, json!({ "op": "state", "id": item.id, "state": "running", "task": task }));
        }
    }

    /// Elect a doer for the class, start the branch with its brief, and link it to the item.
    fn branch_start(&self, thread: &str, item: &Item, class: Class, plan: &Plan, ctx: &Ctx, b: &Binding) -> Result<(), String> {
        let context = if item.verbatim { Context::Verbatim } else { self.0.cfg().branches.context(class) };
        let book = self.0.tasks.book();
        // a reviewer is not the doer of the work it reviews
        let by_item = |ids: &[String]| plan.items.iter().filter(|i| ids.contains(&i.id)).filter_map(|i| book.get(&i.task)).map(|r| r.spec.doer).collect::<Vec<_>>();
        let exclude = if class == Class::Review { by_item(&item.depends) } else { Vec::new() };
        let goal = if item.verbatim { item.goal.clone() } else { text_of(item) };
        // a chat's part sent back is told its shares again, and what changed of them since
        let goal = if chat_piece(item) { format!("{goal}{}", self.access_told(&item.id)) } else { goal };
        // what it is given: this line's files for a brief, what the work it reviews changed for a clean one
        let paths: Vec<String> = if context == Context::Clean {
            let wrote: BTreeSet<String> = plan.items.iter().filter(|i| item.depends.contains(&i.id)).flat_map(|i| i.writes.iter().cloned()).collect();
            wrote.into_iter().collect()
        } else {
            ctx.written.iter().cloned().collect()
        };
        let (task, elected, via) = self.branch_task(thread, &item.id, class, &goal, context, exclude, paths, None, b)?;
        let ruled = if chat_piece(item) { CHAT_PIECE.to_string() } else { decide(item, plan, ctx).why().to_string() };
        let verdict = Verdict::Branch { class, why: format!("{ruled}; {}", elected.why) };
        self.routed(thread, item, class.word(), &verdict, &task, json!({ "via": via, "doer": elected.doer, "context": context.word() }));
        self.write(thread, "plan.item", Actor::Core, json!({ "op": "state", "id": item.id, "state": "running", "task": task }));
        Ok(())
    }

    /// A run with no operator that named its doer keeps it for the items of its plan, unless an item's own words name another: the doer and why. An interactive session's pin is the main lane's, and its branches are still elected for the class.
    fn run_pin(&self, thread: &str, goal: &str) -> Option<(String, String)> {
        let pin = self.0.cfg().doer.clone().filter(|_| self.0.who.headless())?;
        Some(match self.named(thread, goal).map(|(d, _)| d).filter(|d| *d != pin) {
            Some(named) => (named, "the item names it".into()),
            None => (pin, "the run's --doer".into()),
        })
    }

    /// One branch task of the main lane's line for `item`: the doer elected for the class (not the main lane's, for a review, and not
    /// those of `exclude`), the class's mode and limits, the brief, the link back. A `piece` works in a held worktree. Returns the
    /// task, who was elected and how it was handed over (`native`, `delegate` or `passthrough`). `place` puts it where another task worked (`@t5`): a review looks at
    /// the held work.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn branch_task(&self, thread: &str, item: &str, class: Class, goal: &str, context: Context, exclude: Vec<String>, paths: Vec<String>, place: Option<String>, b: &Binding) -> Result<(String, crate::flow::run::Elected, &'static str), String> {
        let parent = self.last_task(thread).ok_or("the main lane has no turn to branch from")?;
        self.branch_from(parent, thread, item, class, goal, context, exclude, paths, place, b)
    }

    /// [`branch_task`](Front::branch_task) from a turn named by its caller, not the thread's newest task (which may be the check of that turn).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn branch_from(&self, parent: crate::task::Row, thread: &str, item: &str, class: Class, goal: &str, context: Context, mut exclude: Vec<String>, paths: Vec<String>, place: Option<String>, b: &Binding) -> Result<(String, crate::flow::run::Elected, &'static str), String> {
        exclude.extend(self.0.out.lock().unwrap_or_else(|e| e.into_inner()).get(thread).into_iter().flatten().cloned());
        if class == Class::Review {
            exclude.push(b.doer.clone());
            exclude.sort();
            exclude.dedup();
        }
        let role = if class == Class::Piece { "work" } else { class.word() };
        // a review may be pinned to a doer and model of its own (`plan.review_doer`, `plan.review_model`), over the election; the cheap
        // branches (scout, piece) likewise (`plan.cheap_doer`, `plan.cheap_model`)
        let (pin, key, what) = if class == Class::Review { (self.0.cfg().reviewer.clone(), "plan.review_", "review") } else { (self.cheap_pin(thread), "plan.cheap_", "cheap") };
        let elected = match self.run_pin(thread, goal) {
            _ if pin.doer.is_some() || pin.model.is_some() => {
                let r = self.pinned(&pin, b, what)?;
                crate::flow::run::Elected { doer: r.doer, model: r.model, endpoint: r.endpoint, why: if class != Class::Review && self.0.cfg().cheap.doer.is_none() && self.0.cfg().cheap.model.is_none() { "the cheap tier of a ready doer: nothing is set (`plan.cheap_doer`, `plan.cheap_model`)".to_string() } else { format!("the setting `{key}doer` / `{key}model` names it") } }
            }
            Some((doer, why)) => crate::flow::run::Elected { doer, model: None, endpoint: None, why },
            None => self.0.who.elect(role, goal, Some(thread), &exclude, self.0.cfg().branches.mode(class))?,
        };
        // a branch that reads is read by its report: it is told the shape, or its `open` is never read (the operator's verbatim words go as typed)
        let shaped = if matches!(class, Class::Review | Class::Scout) && context != Context::Verbatim { format!("{goal}\n{}", crate::report::SHAPE) } else { goal.to_string() };
        let mut spec = self.spec(thread, &shaped, &elected.doer)?;
        (spec.parent, spec.role, spec.account) = (Some(parent.id), Some(class.word().into()), b.account.clone());
        // a branch on the main lane's own doer with no model elected keeps the thread's
        let same = elected.doer == b.doer && elected.model.is_none();
        (spec.tune.mode, spec.tune.model, spec.tune.endpoint) = (self.0.cfg().branches.mode(class), if same { b.model.clone() } else { elected.model.clone() }, if same { b.endpoint.clone() } else { elected.endpoint.clone() });
        self.login_for(thread, &mut spec);
        let (usd, wall) = (self.0.cfg().branches.limits(class).0, self.0.cfg().branches.wall(class, paths.len()));
        // a piece tried again keeps the cap its last try had (a chat's part, its brief's): the class has none of its own
        let last = (class == Class::Piece).then(|| self.0.keeper.as_ref()?.plan(thread)?.items.into_iter().find(|i| i.id == item)).flatten();
        let before = last.and_then(|i| self.0.tasks.book().get(&i.task)).and_then(|r| r.spec.limits.usd_soft);
        spec.limits.usd_soft = [spec.limits.usd_soft, usd, before].into_iter().flatten().reduce(f64::min);
        spec.limits.wall_ms = [spec.limits.wall_ms, wall].into_iter().flatten().reduce(u64::min);
        let via = via(&elected.doer, context);
        spec.input = json!({ "item": item, "branch": { "class": class.word(), "context": context.word(), "via": via, "paths": paths } });
        if class == Class::Piece {
            // whatever `work.apply` says, its work stays in the worktree until the `merge` step takes it
            spec.place = "worktree".into();
            spec.input["hold"] = json!(true);
            if self.access_on() {
                spec.input["shares"] = self.access_input(item);
            }
        }
        if let Some(place) = place {
            spec.place = place;
        }
        if context != Context::Verbatim {
            self.briefed(thread, &mut spec, goal, true, (None, None, None));
        }
        let task = self.0.tasks.spawn(spec, Actor::Core).map_err(|p| p.msg)?;
        // a plan's branch is a chat under the chat that started it
        self.chat_for_branch(thread, &task, item, goal);
        Ok((task, elected, via))
    }
}
