//! A second look at a lane that Main worked itself, under the word `orchestrate` (`orchestrate.review`, on by default): what a split's chats get, a line
//! worked in one lane gets too. When Main's own turn on the main thread ends well and changed files, and no plan of the thread holds it (a plan's items are
//! checked and reviewed by `validate`), a `review` branch reads what it changed, with a clean context, on another doer than the one that wrote it (the
//! reviewer's pin, `plan.review_doer` and `plan.review_model`, or the election). Concerns it names in `open` go back to Main as a turn of their own, which is
//! looked at again in turn; after `Cfg.sends` (3) rounds the concerns left are the operator's to decide. A review that did not finish sends nothing back.
//!
//! A split whose chats wrote is joined once no chat's part is out (the plan done, or blocked with nothing left to start): Main is given one turn that
//! says what each chat laid in, which parts were stopped and where their work is held (not to be written again: laying it in is the operator's), and
//! what Main's planning turn kept for itself (`main`), to wire the parts together and run the project's checks; that turn is reviewed like any lane of
//! Main's. The look at Main's lane takes its newest turn of work on the thread, whatever approval or check came after it; a review of an older turn
//! that ended while a newer one was started (the join of a split begins as the last review ends) is acted on once the newest turn's review is done.
//!
//! Nothing is kept but the tasks: the review is the branch of the turn it reads (`input.item` is `second`), and the round is in the input of the turn that
//! answered it. The reviews already acted on are remembered while J4n runs, so a step that runs again does not act twice.

use super::{Front, Said, Turn};
use crate::branch::{Class, Context, Switch};
use crate::change::Actor;
use crate::task::{Row, State};
use serde_json::{Value as Json, json};

/// The item word of a review of Main's own turn, in the review task's input.
pub const ITEM: &str = "second";

/// What a concern made of a review's own words begins with: such a review is a concern each time it is had.
pub const WORDS: &str = "(the review's own words, not in the report's shape)";

/// The paths a turn changed, as the review is given them.
fn changed(row: &Row) -> Vec<String> {
    let c = &row.changed;
    c.created.iter().chain(&c.modified).chain(&c.deleted).cloned().collect()
}

/// The paths a review of the turn reads: what it changed, and for the turn that joined a split's parts, the files the chats laid in.
fn read(row: &Row) -> Vec<String> {
    let joined = row.spec.input.pointer("/second/joined").and_then(Json::as_array).into_iter().flatten().filter_map(Json::as_str).map(String::from);
    let mut all = changed(row);
    for f in joined {
        if !all.contains(&f) {
            all.push(f);
        }
    }
    all
}

/// The round a turn of Main is: 0 for a line of the operator's, n for the n-th answer to a review.
fn round(row: &Row) -> u64 {
    row.spec.input.pointer("/second/round").and_then(Json::as_u64).unwrap_or(0)
}

/// The line a round answers: the operator's words, carried from turn to turn.
fn line_of(row: &Row) -> String {
    row.spec.input.pointer("/second/line").and_then(Json::as_str).map_or_else(|| row.spec.goal.clone(), str::to_string)
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n).collect::<String>()) }
}

impl Front {
    /// Whether a turn of Main is one this step looks at again: under `orchestrate` with the setting on, on the main thread, with no plan open there, the turn
    /// a work turn that ended well and changed files.
    fn second_wanted(&self, thread: &str, row: &Row) -> bool {
        let cfg = self.0.cfg();
        self.lanes_now() == Switch::Orchestrate
            && cfg.orchestrate.review
            && thread == cfg.thread
            // a plan's item is checked and reviewed by its plan; Main's own part of a split is not an item, and is looked at here
            && !self.0.keeper.as_ref().and_then(|k| k.plan(thread)).is_some_and(|p| p.items.iter().any(|i| i.task == row.id))
            && row.spec.role.as_deref() == Some("work")
            && row.state == State::Done { ok: true }
            && !changed(row).is_empty()
    }

    /// The split's plan was reported (done, or blocked with nothing left to start) and no chat's part is still out: Main joins the parts that were laid
    /// in, and is told which were stopped and where their work is held, once for each report of the plan (`sig`).
    pub(super) fn second_wire(&self, thread: &str, plan: &crate::keeper::Plan, sig: &str) {
        let cfg = self.0.cfg();
        if self.lanes_now() != Switch::Orchestrate || thread != cfg.thread {
            return;
        }
        let parts = || plan.items.iter().filter(|i| i.chat_part());
        let laid: Vec<&crate::keeper::Item> = parts().filter(|i| i.state == "merged").collect();
        let held: Vec<&crate::keeper::Item> = parts().filter(|i| i.state == "blocked").collect();
        if (laid.is_empty() && held.is_empty()) || parts().any(|i| !matches!(i.state.as_str(), "merged" | "blocked")) {
            return;
        }
        if !self.0.second.lock().unwrap_or_else(|e| e.into_inner()).insert(format!("wire:{sig}")) {
            return;
        }
        let (mut line, mut main, mut still) = (String::new(), String::new(), Vec::new());
        let mut points: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
        for e in self.0.log.read(0).filter(|e| e.thread.as_deref() == Some(thread)) {
            if e.kind == "orchestrate.split" {
                line = e.data.get("line").and_then(Json::as_str).unwrap_or_default().to_string();
                main = e.data.get("main").and_then(Json::as_str).unwrap_or_default().to_string();
            }
            // what the folder's check said after the last part was laid in: the failures it had before the parts are Main's to fix
            if e.kind == "plan.item" && e.data.get("state").and_then(Json::as_str) == Some("merged") && laid.iter().any(|i| e.data.get("id").and_then(Json::as_str) == Some(i.id.as_str())) {
                still = e.data.get("still").and_then(Json::as_array).into_iter().flatten().filter_map(Json::as_str).map(String::from).collect();
            }
            // the points a part's review left for Main (raised again after the part answered them, or notes), from its newest review
            if e.kind == "plan.item"
                && let Some(id) = e.data.get("id").and_then(Json::as_str).filter(|id| laid.iter().any(|i| i.id == *id))
            {
                match e.data.get("state").and_then(Json::as_str) {
                    Some("reviewed") if e.data.get("main").is_some() => {
                        points.insert(id.to_string(), e.data["main"].as_array().into_iter().flatten().filter_map(Json::as_str).map(String::from).collect());
                    }
                    Some("sent-back") => {
                        points.remove(id);
                    }
                    _ => {}
                }
            }
        }
        let Some(b) = self.binding(thread) else { return };
        let book = self.0.tasks.book();
        let mut joined: Vec<String> = Vec::new();
        let parts: Vec<String> = laid
            .iter()
            .map(|i| {
                let files = book.get(&i.task).and_then(|r| r.applied).map(|w| w.files).unwrap_or_default();
                joined.extend(files.iter().cloned());
                format!("- {}: {} ({})", i.id, clip(&i.goal, 160), if files.is_empty() { "no file named".to_string() } else { files.join(", ") })
            })
            .collect();
        let stopped: Vec<String> = held
            .iter()
            .map(|i| {
                let at = book.get(&i.task).and_then(|r| r.kept).map(|k| format!("its work is held at {}", k.dir)).unwrap_or_else(|| "no work of it is held".to_string());
                format!("- {}: {} ({at}; {})", i.id, clip(&i.goal, 160), clip(&i.result, 160))
            })
            .collect();
        let mut text = if laid.is_empty() { String::new() } else { format!("The chats' parts are laid in this folder, each checked and reviewed:\n{}\n", parts.join("\n")) };
        if !stopped.is_empty() {
            if self.held_main() {
                text.push_str(&format!("These parts were stopped and are not in this folder; do not write them again:\n{}\n{}\n", stopped.join("\n"), super::digest::HELD));
            } else {
                text.push_str(&format!("These parts were stopped and are not in this folder; laying their work in is the operator's to decide, so do not write them again:\n{}\n", stopped.join("\n")));
            }
        }
        if !points.is_empty() {
            let said: Vec<String> = points.iter().map(|(id, p)| format!("- {id}: {}", clip(&p.join("; "), 400))).collect();
            text.push_str(&format!("The reviews left these points for you to weigh; they did not send the parts back (none names a case that fails, or the part answered it once):\n{}\n", said.join("\n")));
        }
        if !still.is_empty() {
            text.push_str(&format!("The folder's check still fails after the last part was laid in, as it failed before it: {}. The parts did not cause it, so they were laid in; fix it as you join them.\n", clip(&still.join(", "), 300)));
        }
        text.push_str(&format!(
            "Now join what is here{}. Wire the parts together, run the project's checks and the program itself where it can run, and say what runs and what does not.{}",
            if main.trim().is_empty() { String::new() } else { format!(" with your own part, as you planned it: {}", main.trim()) },
            if line.is_empty() { String::new() } else { format!("\n\nThe line was: {line}") }
        ));
        let input = json!({ "second": { "round": 0, "line": if line.is_empty() { text.clone() } else { line.clone() }, "wire": sig, "joined": joined } });
        let said = self.begin(thread, &text, &b, &[], Turn { item: false, compact: false, fresh: false, carry: None, operator: false, brief: None, input: Some(input) });
        let task = if let Said::Started { task } = &said { Some(task.clone()) } else { None };
        let ids = |v: &[&crate::keeper::Item]| v.iter().map(|i| i.id.clone()).collect::<Vec<_>>();
        self.write(thread, "second.wire", Actor::Core, json!({ "sig": sig, "laid": ids(&laid), "held": ids(&held), "task": task }));
        let said = if held.is_empty() { format!("the chats' parts are laid in ({})", ids(&laid).join(", ")) } else { format!("laid in: {} · stopped, held: {}", if laid.is_empty() { "none".to_string() } else { ids(&laid).join(", ") }, ids(&held).join(", ")) };
        self.note(thread, &format!("{said}: Main joins what is here"), "accent");
    }

    /// The goal of the review of `row`: the line, the files, and the shape of the answer.
    fn second_goal(&self, row: &Row) -> String {
        let (files, joined) = (changed(row), read(row).split_off(changed(row).len()));
        // a review that ran a compile or a script (refused: they write) reached its limit with nothing said, so it is told both; its time grows with what it reads
        let time = self.0.cfg().branches.wall(Class::Review, files.len() + joined.len()).map(|ms| format!(" You have {}: say what you found before it is out, even with files left unread.", crate::fmt::duration(ms))).unwrap_or_default();
        let base = format!(
            "Review the change Main just made for this line, which you did not see made: {}\nThe files it changed: {}.{}\nRead only: a command that writes anything (a compile that leaves a `.pyc`, a script put in `/tmp`) is refused, so read the code instead of running it.{time} Say what is wrong with it in `open`, each as `file:line what goes wrong`, the most serious first; leave `open` empty when nothing is.\nDone when every changed file is read.",
            clip(&line_of(row), 1200),
            files.join(", "),
            if joined.is_empty() { String::new() } else { format!(" The parts it joined, which the chats wrote: {}.", joined.join(", ")) }
        );
        let crew = &self.0.cfg().crew;
        match crew.quality().then(|| crew.seats.iter().find(|s| s.id == "reviewer")).flatten() {
            Some(seat) => format!("{}\n\n{base}", seat.personality),
            None => base,
        }
    }

    /// What a review names: its report's `open`, or, for an answer that is no report, its own words as the one concern, so the writer reads them and
    /// fixes what holds (a review is often prose). A review that said nothing names nothing.
    pub(super) fn concerns(&self, review: &Row) -> Vec<String> {
        let words = || review.answer.as_deref().map(str::trim).filter(|a| !a.is_empty()).map(|a| vec![format!("{WORDS} {}", clip(a, 2400))]).unwrap_or_default();
        match self.report(&review.id) {
            Some(r) if r.open.iter().any(|o| o.starts_with("not a report")) => words(),
            Some(r) => r.open,
            None => Vec::new(),
        }
    }

    /// The step: start the review of Main's turn that ended, or act on the review that ended. A review of an older turn whose end no step acted on (a newer
    /// turn of Main that changed files was started as it ended: the join of a split) is acted on once the newest turn's own review has nothing left to do.
    pub(super) fn second_step(&self, thread: &str) {
        // nothing runs on the thread; Main's newest turn of work is the one looked at, not an approval or a check that came after it
        let Some(Some(_)) = self.settled(thread) else { return };
        let main = |r: &Row| r.spec.thread.as_deref() == Some(thread) && r.spec.parent.is_none() && r.spec.doer != crate::task::HUMAN && r.spec.role.as_deref() == Some("work");
        let reviews = |of: &str| self.0.tasks.book().rows().into_iter().rev().filter(|r| r.spec.parent.as_deref() == Some(of) && r.spec.role.as_deref() == Some("review") && r.spec.input.get("item").and_then(Json::as_str) == Some(ITEM)).collect::<Vec<_>>();
        // the newest that changed something: a turn that only answered (a decision asked of Main while the review ran) does not hide that review's end
        let Some(row) = self.0.tasks.book().rows().into_iter().rev().filter(main).find(|r| !r.state.is_terminal() || !changed(r).is_empty()) else { return };
        if !row.state.is_terminal() {
            return;
        }
        if self.second_wanted(thread, &row) {
            match reviews(&row.id).into_iter().next() {
                None if !self.second_acted(&row.id) => return self.second_start(thread, &row),
                // the newest turn's review reads the folder while it runs: an older review's concerns wait, so no turn of Main writes under it
                Some(r) if !r.state.is_terminal() => return,
                Some(r) if !self.second_acted(&r.id) => return self.second_ended(thread, &row, &r, false),
                _ => {}
            }
        }
        // an older review that ended while J4n runs and was not acted on: one a step, oldest first, so its turn on the concerns is reviewed in turn
        let book = self.0.tasks.book();
        let owed = book.rows().into_iter().filter(main).filter(|p| p.id != row.id && p.state.is_terminal() && self.second_wanted(thread, p)).find_map(|p| {
            let r = reviews(&p.id).into_iter().find(|r| r.state.is_terminal() && r.ended_ms.is_some_and(|t| t >= self.0.since_ms) && !self.second_acted(&r.id))?;
            Some((p, r))
        });
        if let Some((p, r)) = owed {
            self.second_ended(thread, &p, &r, true);
        }
    }

    /// Whether the end of the review `id` was acted on while J4n runs.
    fn second_acted(&self, id: &str) -> bool {
        self.0.second.lock().unwrap_or_else(|e| e.into_inner()).contains(id)
    }

    fn second_start(&self, thread: &str, row: &Row) {
        if !self.0.second.lock().unwrap_or_else(|e| e.into_inner()).insert(row.id.clone()) {
            return;
        }
        // the operator called off the steps of this change (`stop`, or a line typed while its check ran): no review comes
        if self.steps_off(&row.id) {
            self.write(thread, "second.ended", Actor::Core, json!({ "of": row.id, "outcome": "skipped", "why": "the operator ended the steps of this change" }));
            return;
        }
        let Some(b) = self.binding(thread) else { return };
        let goal = self.second_goal(row);
        // branched from Main's turn itself: the thread's newest task may be that turn's check, and the review is found by its parent
        match self.branch_from(row.clone(), thread, ITEM, Class::Review, &goal, Context::Clean, vec![row.spec.doer.clone()], read(row), None, &b) {
            Ok((task, elected, _)) => {
                self.write(thread, "second.started", Actor::Core, json!({ "of": row.id, "task": task, "round": round(row), "doer": elected.doer, "model": elected.model, "why": elected.why }));
                let on = format!("{}{}", elected.doer, elected.model.as_ref().map(|m| format!(" · {m}")).unwrap_or_default());
                self.note(thread, &format!("reviewing what Main changed ({} file{}) on {on}, another doer than the one that wrote it", changed(row).len(), if changed(row).len() == 1 { "" } else { "s" }), "dim");
            }
            Err(why) => {
                self.write(thread, "second.ended", Actor::Core, json!({ "of": row.id, "outcome": "none", "why": why }));
                self.note(thread, &format!("no one could review what Main changed: {why}"), "warn");
            }
        }
    }

    /// What came of the review `review` of Main's turn `row`; `older`: a newer turn of Main changed files since `row`, so Main is told the concerns are of an
    /// earlier change.
    fn second_ended(&self, thread: &str, row: &Row, review: &Row, older: bool) {
        if !self.0.second.lock().unwrap_or_else(|e| e.into_inner()).insert(review.id.clone()) {
            return;
        }
        // a review the operator ended is unchecked work, not a review that failed: its row and the Log said so when it was ended
        if self.step_ended(&review.id).is_some() {
            self.write(thread, "second.ended", Actor::Core, json!({ "of": row.id, "review": review.id, "outcome": "skipped" }));
            return;
        }
        let concerns = self.concerns(review);
        if review.state != (State::Done { ok: true }) {
            let why = review.reason.clone().unwrap_or_else(|| review.state.word().to_string());
            // a review stopped at its limit after it named concerns: they stand as a finished review's would
            if concerns.is_empty() {
                self.write(thread, "second.ended", Actor::Core, json!({ "of": row.id, "review": review.id, "outcome": "failed", "why": clip(&why, 300) }));
                self.note(thread, &format!("the review of Main's change did not finish ({}); nothing is sent back", clip(&why, 120)), "warn");
                return;
            }
            self.note(thread, &format!("the review of Main's change stopped ({}) after it named {} concern{}; they stand", clip(&why, 80), concerns.len(), if concerns.len() == 1 { "" } else { "s" }), "dim");
        }
        let (n, sends) = (round(row), u64::from(self.0.cfg().sends));
        if concerns.is_empty() {
            self.write(thread, "second.ended", Actor::Core, json!({ "of": row.id, "review": review.id, "outcome": "clean", "round": n }));
            self.note(thread, &format!("reviewed on {}: no concern", review.spec.doer), "accent");
            return;
        }
        if n >= sends {
            self.write(thread, "second.ended", Actor::Core, json!({ "of": row.id, "review": review.id, "outcome": "left", "round": n, "concerns": concerns }));
            self.note(thread, &format!("after {sends} rounds the review on {} still names {} concern{}, yours to decide: {}", review.spec.doer, concerns.len(), if concerns.len() == 1 { "" } else { "s" }, clip(&concerns.join("; "), 300)), "warn");
            return;
        }
        let Some(b) = self.binding(thread) else { return };
        let list = concerns.iter().map(|c| format!("- {c}")).collect::<Vec<_>>().join("\n");
        let text = if older {
            format!("A review of an earlier change of yours, on another doer ({}), names these concerns; you have changed more since:\n{list}\nFix each one that still holds, and say in a line why you leave any. Change nothing else.", review.spec.doer)
        } else {
            format!("A review of your last change, on another doer ({}), names these concerns:\n{list}\nFix each one that holds, and say in a line why you leave any. Change nothing else.", review.spec.doer)
        };
        let input = json!({ "second": { "round": n + 1, "line": line_of(row), "review": review.id } });
        let said = self.begin(thread, &text, &b, &[], Turn { item: false, compact: false, fresh: false, carry: None, operator: false, brief: None, input: Some(input) });
        let task = if let Said::Started { task } = &said { Some(task.clone()) } else { None };
        self.write(thread, "second.ended", Actor::Core, json!({ "of": row.id, "review": review.id, "outcome": "sent-back", "round": n + 1, "concerns": concerns, "task": task }));
        self.note(thread, &format!("the review on {} names {} concern{}: back to Main ({} of {sends})", review.spec.doer, concerns.len(), if concerns.len() == 1 { "" } else { "s" }, n + 1), "warn");
    }
}
