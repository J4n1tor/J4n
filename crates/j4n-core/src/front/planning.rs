//! The plan, in the front: a planning turn, a new line folded into an open plan, `:plan`, the Brief's
//! `plan` line and the rows of the Tasks page. The plan itself is the fold [`Keeper`]; everything here is events written to the Log.

pub(super) use crate::text::line as clip;
use super::thread::Binding;
use super::route::TRIES;
use super::parts::{Hands, Taken};
use super::{Front, Pin, Said};
use crate::answer::{Answerer, fn_rule};
use crate::ask::{Answer, Opt};
use crate::change::Actor;
use crate::branch::Switch;
use crate::keeper::{Asked, How, Keeper, Plan, STATES, fold_pick, plan_bound, request_parts, take_within, bind_checks};
use crate::task::{Mode, State};
use j4n_proto::{Act, Cell, Intent, Row as PRow, Tone};
use serde_json::{Value as Json, json};

/// The answers the ask point `fold` takes.
pub const FOLD: [&str; 4] = ["add", "change", "answer", "replace"];

/// Whether a line is plan-shaped by its words alone, with no model: it says `plan` as a request (it opens with `plan`, or says
/// `plan this|it|out|the …` or `make a plan`) and is no question, or it lists at least three steps (`(1) … (2) … (3) …`, `1.` / `1)`, or
/// three bullet lines).
pub fn plan_shaped(line: &str) -> bool {
    let low = line.trim().to_lowercase();
    let words: Vec<&str> = low.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let asks = words.first().is_some_and(|w| *w == "plan" || (*w == "please" && words.get(1) == Some(&"plan")))
        || low.contains("make a plan")
        || words.windows(2).any(|w| w[0] == "plan" && matches!(w[1], "this" | "it" | "out" | "the"));
    if asks && !low.ends_with('?') {
        return true;
    }
    let steps = (1..=9).take_while(|n| low.contains(&format!("({n})")) || low.contains(&format!("\n{n}.")) || low.contains(&format!("\n{n})")) || low.contains(&format!(" {n}) ")) || low.contains(&format!(": {n}. ")) || low.starts_with(&format!("{n}. ")) || low.starts_with(&format!("{n}) "))).count();
    steps >= 3 || low.lines().filter(|l| l.trim_start().starts_with("- ") || l.trim_start().starts_with("* ")).count() >= 3
}

/// `rule:plan`: the ask point `plan` (yes/no: is this line plan-shaped?), answered by [`plan_shaped`] alone, so an outside benchmark can ask it
/// through `j4n answer`. It always answers, sure of itself; no model is in its chain.
pub fn plan_rule() -> impl Answerer {
    fn_rule("rule:plan", |q| {
        Ok(if plan_shaped(&q.text) { (Answer::Yes, "the line asks for a plan or lists steps".into()) } else { (Answer::No, "the line neither asks for a plan nor lists three steps".into()) })
    })
}

/// What a planning turn is told. The model has no tool for the plan: it answers in one shape, and J4n checks it.
fn prompt(plan: Option<&Plan>, how: How, line: &str, checks: &[String]) -> String {
    let mut text = String::from("Plan this for the project; do not do it. Read what you need and change nothing. ");
    text.push_str("Answer with one JSON object and nothing else: {\"approach\": \"one paragraph on how the work will go\", \"items\": [{\"id\": \"short-id\", \"goal\": \"one line\", \"reads\": [\"path\"], \"writes\": [\"path\"], \"depends\": [\"id\"], \"done\": \"one line: how to tell it is done\", \"check\": \"name|none\", \"where\": \"inline|scout|review|piece\", \"main\": false}]}. ");
    let names: Vec<&str> = checks.iter().map(String::as_str).chain(["none"]).collect();
    text.push_str(&"The items are the steps in order. `id` is lowercase letters, digits and `-`. `reads` and `writes` are the paths inside the project each step reads and changes. `depends` names the items that must be done first; parts that share no file have no `depends`, so one that fails does not stop the rest. `check` is one word: {checks}, J4n runs it when the step ends and the step counts only when it passes; `none` when nothing can tell. `where` is your proposal for who does it: `inline` (the main session), `scout` (a read-only look that writes nothing), `review` (a read-only check of finished work), `piece` (a change to paths no other item touches); J4n decides. `main` is true when only the main session can settle it. ".replace("{checks}", &names.join(", ")));
    match (plan.filter(|p| p.open()), how) {
        (Some(p), How::Add | How::Change | How::Replan) => {
            text.push_str("There is a plan already; answer with the whole plan again. Keep the items that are not named as they are, with their ids. An item that is running or merged is kept exactly. ");
            text.push_str(match how {
                How::Add => "Add the new request as items; change nothing else.\n\nThe plan now:\n",
                How::Change => "Change the plan for the new request.\n\nThe plan now:\n",
                // the old plan, the evidence and the reason are the planner's whole input
                _ => "The approach is not working. Change it: add `\"why\": \"one line on why the approach changes\"` to the object. Items that are merged are done and stay in the plan; leave an item out only if it is not needed any more, and add at most a few items. Items that are not started may be changed or dropped; an item that is blocked may be changed, or kept to start again.\n\nThe plan now:\n",
            });
            text.push_str(&format!("approach: {}\n", p.approach));
            for i in &p.items {
                text.push_str(&format!("{} [{}] {} | reads {} | writes {} | depends {} | done when {} | check {}\n", i.id, i.state, i.goal, i.reads.join(","), i.writes.join(","), i.depends.join(","), i.done, if i.check.is_empty() { "none" } else { &i.check }));
            }
        }
        // a plan as big as the request, not as big as the planner can make it
        _ => text.push_str(&format!("The request has {} part(s): give at most {} items, one for each part. A read that only feeds a change is the first step of that change's item, not an item of its own; no item to check, review or sum up, J4n does that. Give one or two when the work is that small, and it is kept as the plan.\n", request_parts(line), plan_bound(line, None, 0.0))),
    }
    text.push_str(&format!("\nRequest: {line}"));
    text
}

impl Front {
    /// The same front, keeping a plan per thread in `keeper` (the fold the Folds host feeds). Without one a line is always a task.
    pub fn with_keeper(self, keeper: Keeper) -> Front {
        match std::sync::Arc::try_unwrap(self.0) {
            Ok(mut core) => {
                core.keeper = Some(keeper);
                Front(std::sync::Arc::new(core))
            }
            Err(shared) => Front(shared),
        }
    }

    pub fn keeper(&self) -> Option<&Keeper> {
        self.0.keeper.as_ref()
    }

    /// The thread's open plan as the strip counts it: the items that are done and all of them.
    pub fn plan_counts(&self, thread: &str) -> Option<(usize, usize)> {
        let plan = self.0.keeper.as_ref()?.plan(thread).filter(|p| p.open() && !p.items.is_empty())?;
        Some((plan.items.iter().filter(|i| i.done()).count(), plan.items.len()))
    }

    /// The Brief's one `plan` line while the thread's plan is open (at most 60 tokens); none otherwise.
    pub fn plan_line(&self, thread: &str) -> Option<String> {
        self.0.keeper.as_ref()?.plan(thread)?.line()
    }

    /// A line typed while the thread's plan is open is not run as typed: the ask point `fold` says what it does to the plan. `None`: the
    /// line goes the way it always went (no plan open, a task that works is steered, or nothing is bound).
    pub(super) fn fold_line(&self, thread: &str, text: &str, keys: &[String]) -> Option<Said> {
        // with `single` the keeper is off: a line is a task, and an open plan (`/plan`) is only changed by `/plan`
        let plan = self.0.keeper.as_ref().filter(|_| self.lanes_now() != Switch::Single)?.plan(thread).filter(Plan::open)?;
        if plan.asked.is_some() || self.last_task(thread).is_some_and(|r| !r.state.is_terminal()) {
            return None;
        }
        // the open rows are the chats Main started: a line is Main's own turn, not a change to their plan
        if plan.items.iter().filter(|i| !i.done()).all(|i| self.is_chat_item(i)) {
            return None;
        }
        let b = self.binding(thread)?;
        // the rule that opens the chain answers first and writes no decision row: the cheap cases cost the turn nothing
        let ids: Vec<String> = plan.items.iter().map(|i| i.id.clone()).collect();
        let (pick, by, why) = match fold_pick(text, &ids) {
            Some((pick, why)) => (pick.to_string(), "rule:fold".to_string(), why),
            None => {
                let options = FOLD.iter().map(|id| Opt { id: (*id).into(), label: (*id).into(), recommended: *id == "add" }).collect();
                let mut q = self.0.who.question("fold", Some(thread), text, options);
                // what is being done is the plan, as the probe cases of `fold` carry it, not the line itself
                q.goal = clip(&format!("the plan: {} | {}", plan.approach, plan.items.iter().map(|i| format!("{} [{}] {}", i.id, i.state, i.goal)).collect::<Vec<_>>().join("; ")), 800);
                let (pick, by, why, _) = self.0.who.pick("fold", Some(thread), &q).unwrap_or_else(|| ("add".into(), "front".into(), "no ask point `fold`".into(), None));
                (pick, by, why)
            }
        };
        self.write(thread, "plan.folded", Actor::Core, json!({ "pick": pick, "by": by, "why": why, "line": text }));
        self.note(thread, &format!("plan: {pick} — {why}"), "dim");
        Some(match pick.as_str() {
            // a question touches no item; a line that says to go on works the current one
            "answer" => self.turn_as(thread, text, &b, keys, (!text.trim_end().ends_with('?'), false, true)),
            "replace" => {
                self.write(thread, "plan.holds", Actor::Core, json!({ "line": text }));
                self.note(thread, "this looks like a new plan, not a change to the one open: `:plan replace` drops the open plan and plans this line again", "warn");
                Said::Command("plan".into())
            }
            "change" => self.plan_turn(thread, text, How::Change, &b),
            _ => self.plan_turn(thread, text, How::Add, &b),
        })
    }

    /// A planning turn: the thread's own doer, in its own session, read-only, asked for the plan in one shape. What it answers is taken
    /// (or refused) when it ends: [`plan_collect`](Front::plan_collect).
    pub(super) fn plan_turn(&self, thread: &str, line: &str, how: How, b: &Binding) -> Said {
        self.plan_turn_with(thread, line, how, b, &[], &[])
    }

    /// [`plan_turn`](Front::plan_turn), for a replan: `evidence` is what the planner is shown by Log id, `keys` the triggers it answers.
    pub(super) fn plan_turn_with(&self, thread: &str, line: &str, how: How, b: &Binding, evidence: &[String], keys: &[String]) -> Said {
        let Some(keeper) = &self.0.keeper else { return self.refuse(thread, "nothing here keeps a plan".into()) };
        // the planner may be another doer or model than the work's; the items keep the thread's binding
        let b = &match self.pinned(&self.0.cfg().planner, b, "plan") {
            Ok(b) => b,
            Err(why) => return self.refuse(thread, why),
        };
        if matches!(how, How::Begin | How::Replace) {
            self.0.out.lock().unwrap_or_else(|e| e.into_inner()).remove(thread);
        }
        let mut goal = prompt(keeper.plan(thread).as_ref(), how, line, &self.0.cfg().checks);
        if !evidence.is_empty() {
            goal.push_str(&format!("\nEvidence, by Log id:\n{}", evidence.join("\n")));
        }
        let mut spec = match self.spec(thread, &goal, &b.doer) {
            Ok(spec) => spec,
            Err(why) => return self.refuse(thread, why),
        };
        spec.role = Some("plan".into());
        spec.tune.mode = Mode::Read;
        spec.tune.model = b.model.clone();
        spec.tune.endpoint = b.endpoint.clone();
        spec.account = b.account.clone();
        self.login_for(thread, &mut spec);
        let in_session = self.follow(thread, &b.doer, &mut spec, true);
        self.briefed(thread, &mut spec, line, in_session, (None, None, None));
        match self.0.tasks.spawn(spec, Actor::Operator) {
            Ok(task) => {
                self.write(thread, "plan.asked", Actor::Core, json!({ "task": task, "how": how.word(), "line": clip(line, 400), "evidence": evidence, "keys": keys }));
                Said::Started { task }
            }
            Err(p) => self.refuse(thread, p.msg),
        }
    }

    /// `b` with the doer and model `pin` names in place of its own (`plan.doer`, `plan.model`; the reviewer's are `plan.review_*`); `b` itself
    /// when `pin` is empty. A model word is matched against the models of the doer it runs on, as `--model` is: the newest of several, and a
    /// word that matches none refuses the turn. Another doer brings none of the thread's model, host or account; another model, no host.
    pub(super) fn pinned(&self, pin: &Pin, b: &Binding, role: &str) -> Result<Binding, String> {
        let mut p = b.clone();
        if let Some(d) = pin.doer.as_ref().filter(|d| **d != b.doer) {
            if !self.knows_doer(d) {
                return Err(format!("the {role} doer `{d}` is not one J4n has"));
            }
            (p.doer, p.model, p.endpoint, p.account) = (d.clone(), None, None, None);
        }
        if let Some(word) = &pin.model {
            p.model = Some(match super::models::settle(self.0.cfg().models.as_ref(), &p.doer, word).map_err(|why| format!("the {role} model: {why}"))? {
                Some(super::models::Resolved::One(m)) => m,
                Some(super::models::Resolved::Many(all)) => all.first().cloned().unwrap_or_else(|| word.clone()),
                _ => word.clone(),
            });
            if p.model != b.model {
                p.endpoint = None;
            }
        }
        Ok(p)
    }

    /// Take what the planning turns and the items that run have come to. Safe to call at any time and again: what was taken is in the
    /// Log, so a restart finds a turn that ended while J4n was away and takes it now.
    pub fn plan_collect(&self, thread: &str) {
        let Some(keeper) = &self.0.keeper else { return };
        let _one = self.0.plan_lock.lock().unwrap_or_else(|e| e.into_inner());
        let Some(plan) = keeper.plan(thread) else { return };
        if let Some(asked) = plan.asked.clone() {
            self.plan_taken(thread, &plan, &asked);
        }
        for i in plan.items.iter().filter(|i| i.state == "running" && !i.task.is_empty()) {
            let Some(row) = self.0.tasks.book().get(&i.task).filter(|r| r.state.is_terminal()) else { continue };
            if i.place != "inline" {
                self.branch_ended(thread, i, &row);
                continue;
            }
            // an inline turn is the main lane writing the folder itself: when it ends well there is nothing to validate or merge
            let ok = matches!(row.state, State::Done { ok: true });
            // a turn that ended with the doer's own list open did not finish the item; the plan sends it again, the rule `followups` is not for it
            if ok && row.todo_left > 0 {
                let active = if row.active.is_empty() { String::new() } else { format!(" (in hand: {})", clip(&row.active, 120)) };
                self.send_back(thread, i, &format!("the doer's own list still had {} item(s) open when the turn ended{active}", row.todo_left), "sends");
                continue;
            }
            // a part is merged only once its gate is green and its commit made; until then it stays as it is
            match self.0.cfg().parts.as_ref().filter(|p| p.on()).map_or(Taken::Not, |p| p.ended(thread, i, &row, &Hand { front: self, thread })) {
                Taken::Not => {}
                Taken::Wait => continue,
                Taken::Settle { state, result, block } => {
                    self.write(thread, "plan.item", Actor::Core, json!({ "op": "state", "id": i.id, "state": state, "result": result, "block": block }));
                    continue;
                }
            }
            // an item with a contract is not merged when its turn ends well: its scope and its check say so (`validate.rs`)
            let state = if ok && i.contract() { "reported" } else if ok { "merged" } else if i.tries >= TRIES + i.sends { "blocked" } else { "todo" };
            let result = if ok { clip(row.answer.as_deref().unwrap_or_default(), 160) } else { format!("did not finish: {}", row.reason.as_deref().unwrap_or(row.state.word())) };
            self.write(thread, "plan.item", Actor::Core, json!({ "op": "state", "id": i.id, "state": state, "result": result, "block": if state == "blocked" { "failed" } else { "" } }));
        }
        self.plan_validate(thread);
    }

    fn plan_taken(&self, thread: &str, plan: &Plan, asked: &Asked) {
        let Some(row) = self.0.tasks.book().get(&asked.task).filter(|r| r.state.is_terminal()) else { return };
        let how = How::from_word(&asked.how).unwrap_or(How::Begin);
        let taken = match (&row.state, &row.answer) {
            (State::Done { ok: true }, Some(text)) => take_within(text, Some(plan), how, &asked.line, &asked.evidence, self.plan_bound_now(thread, &asked.line, row.spent)),
            _ => Err(row.reason.clone().unwrap_or_else(|| "the planning turn did not end with an answer".into())),
        };
        let micros = row.spent;
        match taken {
            Ok((mut ops, shift)) => {
                self.bound_checks(thread, &mut ops);
                if how == How::Replan && self.replan_held(thread, row.answer.as_deref().unwrap_or_default(), asked, &shift, micros) {
                    return;
                }
                self.plan_apply(thread, plan, &asked.task, &ops, &shift, micros);
            }
            Err(why) => {
                self.write(thread, "plan.settled", Actor::Core, json!({ "task": asked.task, "ok": false, "why": why, "micros": micros }));
                let stands = if plan.open() { "; the plan stands as it was" } else { "" };
                self.note(thread, &format!("no plan taken: {why}{stands}"), "warn");
            }
        }
    }

    /// The checks an answer names are the project's or `none`; each other word is said.
    pub(super) fn bound_checks(&self, thread: &str, ops: &mut [crate::keeper::Op]) {
        for said in bind_checks(ops, &self.0.cfg().checks) {
            self.note(thread, &format!("plan: {said}"), "warn");
        }
    }

    /// How many items a plan for `line` may begin with: the request's parts and the money left. An item costs about what the planning
    /// turn (`planning`: micros) did, which read the project; what is left is the run's limit (`--usd`) less what the thread has spent.
    fn plan_bound_now(&self, thread: &str, line: &str, planning: i64) -> usize {
        let spent: i64 = self.0.tasks.book().rows().iter().filter(|r| r.spec.thread.as_deref() == Some(thread)).map(|r| r.spent).sum();
        plan_bound(line, self.0.cfg().usd_soft.map(|cap| cap - spent as f64 / 1e6), planning as f64 / 1e6)
    }

    /// The first turn of a line that starts an item of the open plan marks it running; it is marked done when the turn ends.
    pub(super) fn plan_started(&self, thread: &str, task: &str) {
        let Some(item) = self.0.keeper.as_ref().and_then(|k| k.plan(thread)).and_then(|p| p.current().cloned()) else { return };
        if item.state != "running" {
            self.write(thread, "plan.item", Actor::Core, json!({ "op": "state", "id": item.id, "state": "running", "task": task }));
        }
    }

    /// `:plan`: show the plan, edit it, or begin one. An edit is the operator's event and wins over the planner's next answer.
    pub(super) fn plan_command(&self, thread: &str, args: &[String]) -> Said {
        let said = Said::Command("plan".into());
        let Some(keeper) = &self.0.keeper else { return self.refuse(thread, "nothing here keeps a plan".into()) };
        self.plan_collect(thread);
        let plan = keeper.plan(thread).unwrap_or_default();
        let edit = |data: Json| {
            self.write(thread, "plan.item", Actor::Operator, data);
        };
        let known = |id: &str| plan.items.iter().any(|i| i.id == id);
        let list = |words: &[String]| -> Vec<String> { words.iter().flat_map(|w| w.split(',')).filter(|w| !w.is_empty()).map(String::from).collect() };
        match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            [] => {
                let text = if plan.items.is_empty() { "no plan: `:plan <what to plan>` makes one".to_string() } else { self.plan_text(&plan) };
                self.note(thread, &text, "dim");
            }
            ["replace" | "yes"] => {
                let Some(line) = plan.held.clone() else { return self.refuse(thread, "no line waits for a yes to replace the plan".into()) };
                return self.plan_begin(thread, &line, How::Replace);
            }
            ["accept" | "reject"] if plan.pending.is_none() => return self.refuse(thread, "no replan waits for a yes".into()),
            ["accept"] => return self.replan_decide(thread, &plan, true),
            ["reject"] => return self.replan_decide(thread, &plan, false),
            ["drop", id] if known(id) => edit(json!({ "op": "drop", "id": id })),
            ["state", id, state] if known(id) && STATES.contains(state) => edit(json!({ "op": "state", "id": id, "state": state, "result": "set by the operator" })),
            ["add", ..] if args.len() > 1 => {
                let id = (1..).map(|n| format!("n{n}")).find(|id| !known(id) && !plan.gone.contains(id)).unwrap_or_default();
                edit(json!({ "op": "add", "id": id, "goal": clip(&args[1..].join(" "), 160), "done": "the operator says so" }));
            }
            ["set", id, field, ..] if known(id) && args.len() > 3 => {
                let value = &args[3..];
                let mut data = json!({ "op": "set", "id": id });
                match *field {
                    "where" if value.len() != 1 || (value[0] != "inline" && crate::branch::Class::from_word(&value[0]).is_none()) => return self.refuse(thread, "where is inline, scout, review or piece".into()),
                    "main" | "verbatim" if value.len() != 1 || !["yes", "no"].contains(&value[0].as_str()) => return self.refuse(thread, format!("{field} is yes or no")),
                    "main" | "verbatim" => data[*field] = json!(value[0] == "yes"),
                    "goal" | "done" | "where" | "result" => data[*field] = json!(clip(&value.join(" "), 160)),
                    "depends" | "reads" | "writes" => data[*field] = json!(list(value)),
                    _ => return self.refuse(thread, "a field is goal, done, where, main, verbatim, result, depends, reads or writes".into()),
                }
                edit(data);
            }
            ["drop" | "state" | "set", ..] => return self.refuse(thread, "`:plan drop <id>`, `:plan state <id> <state>`, `:plan set <id> <field> <words>`, `:plan add <goal>`: the id is one of the plan's, the state one of ".to_string() + &STATES.join(", ")),
            words => {
                if plan.open() {
                    return self.refuse(thread, "a plan is open: say the line and it is folded into it, or `:plan replace` after a line that asks for a new one".into());
                }
                return self.plan_begin(thread, &words.join(" "), How::Begin);
            }
        }
        said
    }

    /// Begin (or replace) a plan from `line`: the thread is bound first if it is not.
    fn plan_begin(&self, thread: &str, line: &str, how: How) -> Said {
        if let Some(row) = self.last_task(thread).filter(|r| !r.state.is_terminal()) {
            return self.busy(thread, &row.id, "");
        }
        match self.binding(thread).map(Ok).unwrap_or_else(|| self.bind_first(thread, line, None)) {
            Ok(b) => self.plan_turn(thread, line, how, &b),
            Err(why) => self.refuse(thread, why),
        }
    }

    fn plan_text(&self, plan: &Plan) -> String {
        let mut lines = vec![format!("plan · {}", plan.approach)];
        for i in &plan.items {
            let after = if i.depends.is_empty() { String::new() } else { format!(" ← {}", i.depends.join(",")) };
            lines.push(format!("{} {} {}{after}", i.state, i.id, i.goal));
        }
        lines.join("\n")
    }

    /// The rows of the plan for the Tasks page: a root with the approach and what the planning turns cost, the items under it.
    pub fn plan_rows(&self, thread: &str) -> Vec<PRow> {
        let Some(plan) = self.0.keeper.as_ref().and_then(|k| k.plan(thread)).filter(|p| !p.items.is_empty()) else { return Vec::new() };
        let root = format!("plan:{thread}");
        let done = plan.items.iter().filter(|i| i.done()).count();
        // what the plan took: its planning turns and the rows of its items (fold `lanes`)
        let ledger = self.plan_ledger(thread);
        let micros = ledger.as_ref().map_or(plan.micros, |l| l.micros());
        let cost = if micros > 0 { format!("${:.4}", micros as f64 / 1e6) } else { String::new() };
        let head = vec![
            Cell::new("plan", Tone::Dim),
            Cell::new(format!("{done}/{}", plan.items.len()), if plan.open() { Tone::Accent } else { Tone::Good }),
            Cell::new("", Tone::Plain),
            Cell::new(clip(&plan.approach, 60), Tone::Plain),
            Cell::new(cost, Tone::Dim),
        ];
        let mut rows = vec![PRow { key: root.clone(), cells: head, tone: Tone::Accent, detail: plan.reasons.iter().rev().take(3).map(|r| Cell::new(clip(r, 200), Tone::Dim)).collect(), ..PRow::default() }];
        let last = plan.items.len().saturating_sub(1);
        for (n, i) in plan.items.iter().enumerate() {
            let tone = match i.state.as_str() {
                "merged" => Tone::Good,
                "running" => Tone::Accent,
                "blocked" | "sent-back" => Tone::Warn,
                "reviewed" if i.done() => Tone::Good,
                "reported" | "checked" | "reviewed" => Tone::Accent,
                _ => Tone::Plain,
            };
            let pipe = if n == last { "└" } else { "├" };
            let spent = ledger.as_ref().and_then(|l| l.items.iter().find(|(id, _)| *id == i.id)).filter(|(_, t)| t.micros > 0).map(|(_, t)| format!("${:.4} {:.1}s", t.micros as f64 / 1e6, t.wall_ms as f64 / 1000.0)).unwrap_or_default();
            let cells = vec![Cell::new(format!("{pipe} {}", i.id), Tone::Dim), Cell::new(&i.state, tone), Cell::new(&i.place, Tone::Dim), Cell::new(clip(&i.goal, 60), Tone::Plain), Cell::new(spent, Tone::Dim)];
            let run = |args: &[&str]| Intent::Run { command: "plan".into(), args: args.iter().map(|a| (*a).into()).collect() };
            let actions = vec![
                Act { key: "d".into(), label: "drop".into(), intent: run(&["drop", &i.id]), risk: 40 },
                Act { key: "m".into(), label: "done".into(), intent: run(&["state", &i.id, "merged"]), risk: 20 },
            ];
            let detail = [i.done.as_str(), i.result.as_str()].into_iter().filter(|t| !t.is_empty()).map(|t| Cell::new(clip(t, 200), Tone::Dim)).collect();
            rows.push(PRow { key: format!("plan:{thread}:{}", i.id), cells, tone, detail, actions, parent: Some(root.clone()), ..PRow::default() });
        }
        rows
    }
}

/// What the part runner may do through the front: read a task, start one in the thread, in the folder (a turn that is over takes no
/// child, so it is the thread's own task, and its end is the thread's boundary as any task's is).
struct Hand<'a> {
    front: &'a Front,
    thread: &'a str,
}

impl Hands for Hand<'_> {
    fn row(&self, task: &str) -> Option<crate::task::Row> {
        self.front.0.tasks.book().get(task)
    }

    fn start(&self, doer: &str, goal: &str, input: Json) -> Result<String, String> {
        let mut spec = self.front.spec(self.thread, goal, doer)?;
        (spec.role, spec.place, spec.input) = (Some("parts".into()), "here".into(), input);
        self.front.0.tasks.spawn(spec, Actor::Core).map_err(|p| p.msg)
    }
}
