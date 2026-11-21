//! Offload, in the front: a line of the operator's that is a light chore goes by rule to a cheaper doer, as a
//! task under the turn the thread last had, and what it answers is told to the main lane in its next Brief like a branch's report. The rule and the
//! choice of the doer are [`crate::offload`]'s. Nothing here writes: a model chore runs `read` under the Gate, a `check` chore is the project's
//! own command. The main lane stays the only planner and the only writer.
//!
//! Also the main doer's pick (`:setup doer claude`, the setting `lane.doer`): the election falls to it when it is ready.

use super::thread::Binding;
use super::wait::{Kind as Wait, Pending};
use super::{Front, Pin, Said};
use crate::binder::yes_no;
use crate::change::Actor;
use crate::offload::{Chore, Facts, Route, route};
use crate::task::Mode;
use serde_json::json;
use std::collections::BTreeSet;

/// What a chore may take when its class says nothing.
const USD: f64 = 0.15;
/// What a failed check's words are cut to when they are told to the main lane: the first line and the end of the output.
const TAIL: usize = 280;

/// What the front keeps of offload while it runs: the switch as `:offload` last said it, the pick as `:setup doer` last said it, and the chores whose
/// answer was told to the main lane already (the reviews that wait for the operator's word wait in [`super::wait`]).
#[derive(Default)]
pub struct State {
    on: Option<bool>,
    pick: Option<Option<String>>,
    told: BTreeSet<String>,
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("…{}", s.chars().skip(s.chars().count() - n).collect::<String>()) }
}

impl Front {
    fn chores(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.chores.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// What `:offload` and `:setup doer` said while J4n ran is what the settings say now (after an edit or an `:undo` the settings
    /// are read again, so the overlay goes).
    pub(super) fn chores_reset(&self) {
        let mut c = self.chores();
        (c.on, c.pick) = (None, None);
    }

    /// Changes taken back that were of `offload.on` or `lane.doer`, where nothing reads the settings again: the overlay is what the
    /// layers below say (trap 1).
    pub(super) fn chores_undone(&self, undone: &[crate::change::Change]) {
        let screen = self.screen();
        for c in undone.iter().filter(|c| c.target.kind == "setting") {
            let below = |f: &str| screen.tables.field_below(c.target.layer, "setting", &c.target.id, f).cloned();
            let now = c.after.as_ref().and_then(|t| t.get("value").cloned()).or_else(|| below("value")).or_else(|| below("default"));
            match (c.target.id.as_str(), now) {
                ("offload.on", Some(toml::Value::Boolean(b))) => self.chores().on = Some(b),
                ("lane.doer", Some(toml::Value::String(w))) => self.chores().pick = Some(Some(w).filter(|w| !w.is_empty())),
                ("lane.doer", None) => self.chores().pick = Some(None),
                _ => {}
            }
        }
    }

    /// Whether lines go to chores now: the switch as `:offload` last said it, else the setting at the start.
    pub fn offload_on(&self) -> bool {
        let o = &self.0.cfg().offload;
        !o.chores.is_empty() && self.chores().on.unwrap_or(o.on)
    }

    /// The doer the operator picked for the main lane (`:setup doer`, else the setting `lane.doer`), if any.
    pub fn main_pick(&self) -> Option<String> {
        self.chores().pick.clone().unwrap_or_else(|| self.0.cfg().main.doer.clone())
    }

    fn stay(&self, thread: &str, chore: &Chore, why: &str) {
        self.write(thread, "offload.stayed", Actor::Core, json!({ "chore": chore.id, "why": why }));
        self.note(thread, &format!("offload: {} stays with the main lane — {why}", chore.id), "dim");
    }

    /// A line that is a light chore, started as a task under the thread's last turn: `Some(said)` when it was taken, `None` when it is the main
    /// lane's (the rows say why when it stays for a reason, such as the day's cap). A thread that has had no turn has no turn to be under, and
    /// a line typed while a task works steers it: neither is a chore.
    pub(super) fn chore_take(&self, thread: &str, text: &str) -> Option<Said> {
        self.chore_collect(thread);
        if !self.offload_on() {
            return None;
        }
        if let Some(p) = self.wait_take(thread, Wait::Review) {
            if yes_no(text) == Some(true) {
                return Some(self.chore_run(thread, &p.id, &p.with[0]));
            }
            self.note(thread, &format!("the {} stays with the main lane", p.id), "dim");
        }
        let (b, last) = (self.binding(thread)?, self.last_task(thread).filter(|r| r.state.is_terminal())?);
        let cfg_ = self.0.cfg();
        let taken = cfg_.offload.take(text)?;
        if taken.chore.asks() {
            self.wait_put(thread, Pending { id: taken.chore.id.clone(), kind: Wait::Review, at_ms: self.log().clock().now_ms(), with: vec![taken.line.clone()] });
            self.write(thread, "offload.asked", Actor::Core, json!({ "chore": taken.chore.id, "line": taken.line }));
            self.note(thread, &format!("a {} reads what {} changed, on a cheaper doer; `yes` sends it, anything else keeps it with the main lane", taken.chore.id, last.id), "accent");
            return Some(Said::Command("offload".into()));
        }
        self.chore_go(thread, taken.chore, &taken.line, &b)
    }

    /// The review the operator said yes to.
    fn chore_run(&self, thread: &str, id: &str, line: &str) -> Said {
        let cfg_ = self.0.cfg();
        let (Some(chore), Some(b)) = (cfg_.offload.chores.iter().find(|c| c.id == id), self.binding(thread)) else { return self.refuse(thread, format!("no {id} to send: the thread has no turn")) };
        self.chore_go(thread, chore, line, &b).unwrap_or_else(|| Said::Command("offload".into()))
    }

    /// Where the chore goes and its task. `None`: it stays, and a row said why.
    fn chore_go(&self, thread: &str, chore: &Chore, line: &str, b: &Binding) -> Option<Said> {
        let parent = self.last_task(thread)?;
        let (doer, model, account, why) = if chore.how == "check" {
            if !self.knows_doer("check") {
                self.stay(thread, chore, "no doer `check` is declared");
                return None;
            }
            ("check".to_string(), String::new(), None, "the project's own command: no model, and only a failure comes back".to_string())
        } else {
            let ready: Vec<String> = self.0.who.ready(if chore.how == "review" { "review" } else { "scout" }, &[]).into_iter().map(|o| o.id).collect();
            let (pro, now) = (self.0.tasks.plans().accounts_on("pro"), self.log().clock().now_ms());
            let window = |d: &str, a: &str| self.window_now(d, a);
            let credits = || self.0.cfg().credits.as_ref().and_then(|c| c.left(now).ok()).and_then(|l| l.remaining);
            let facts = Facts { main: (&b.doer, b.account.as_deref().unwrap_or("")), ready: &ready, pro: &pro, window: &window, spent: self.spent_today("chore"), main_model: b.model.as_deref().unwrap_or(""), credits: &credits };
            match route(&self.0.cfg().offload, &facts, chore.usd.unwrap_or(USD)) {
                Route::Stay { why } => {
                    self.stay(thread, chore, &why);
                    return None;
                }
                Route::Go { doer, model, account, why, .. } => (doer, model, account, why),
            }
        };
        let goal = match chore.how.as_str() {
            "check" => format!("run the {} check", if chore.check.is_empty() { "project's tests" } else { &chore.check }),
            "review" => format!("{line}\nRead the files the last turn changed and say what is wrong with the change, with a path:line for each point. {SHAPE}"),
            _ => format!("{line}\n{SHAPE}"),
        };
        let mut spec = match self.spec(thread, &goal, &doer) {
            Ok(s) => s,
            Err(e) => return Some(self.refuse(thread, e)),
        };
        (spec.parent, spec.role, spec.account) = (Some(parent.id.clone()), Some("chore".into()), account.clone());
        // a chore never writes: the Gate refuses a write of a task in `read` whatever the doer does
        spec.tune.mode = Mode::Read;
        if !model.is_empty() {
            match self.pinned(&Pin { doer: Some(doer.clone()), model: Some(model.clone()) }, b, "chore") {
                Ok(p) => (spec.tune.model, spec.tune.endpoint) = (p.model, p.endpoint),
                Err(e) => {
                    self.stay(thread, chore, &e);
                    return None;
                }
            }
        }
        self.login_for(thread, &mut spec);
        spec.limits.usd_soft = [spec.limits.usd_soft, (doer != "check").then_some(chore.usd.unwrap_or(USD))].into_iter().flatten().reduce(f64::min);
        spec.limits.wall_ms = [spec.limits.wall_ms, chore.wall.as_deref().and_then(crate::expr::Expr::dur)].into_iter().flatten().reduce(u64::min);
        spec.input = json!({ "chore": chore.id, "check": chore.check, "strict": !chore.check.is_empty(), "branch": { "class": "chore", "context": "brief", "via": crate::branch::via(&doer, crate::branch::Context::Brief), "paths": [] } });
        if doer != "check" {
            self.briefed(thread, &mut spec, &goal, false, (None, None, None));
        }
        let who = crate::task::who(&doer, &spec.tune.model.clone().unwrap_or_default(), "");
        match self.0.tasks.spawn(spec, Actor::Core) {
            Ok(task) => {
                self.write(thread, "offload.routed", Actor::Core, json!({ "chore": chore.id, "how": chore.how, "doer": doer, "model": model, "account": account, "why": why, "task": task, "under": parent.id }));
                self.note(thread, &format!("offload: {} → {who} ({why}) as {task} under {}; `:ask {line}` sends it to the main lane", chore.id, parent.id), "dim");
                Some(Said::Started { task })
            }
            Err(p) => Some(self.refuse(thread, p.msg)),
        }
    }

    /// What the chores that ended came to, told to the main lane once: its next Brief carries it as the section `branches`. A check that passed
    /// says nothing (only failures come back), a failed one says the first line and the end of its output, and a model's answer is its report.
    pub fn chore_collect(&self, thread: &str) {
        let Some(lanes) = self.lanes() else { return };
        let rows = self.0.tasks.book().rows();
        for r in rows.iter().filter(|r| r.spec.role.as_deref() == Some("chore") && r.spec.thread.as_deref() == Some(thread) && r.state.is_terminal()) {
            if !self.chores().told.insert(r.id.clone()) {
                continue;
            }
            let ok = matches!(r.state, crate::task::State::Done { ok: true });
            let failed = match &r.state {
                crate::task::State::Stopped { why: crate::task::Why::Failed { message } } => Some(message.clone()),
                _ => None,
            };
            let said = r.answer.clone().or(failed).or_else(|| r.reason.clone()).unwrap_or_else(|| r.state.word().to_string());
            if r.spec.doer == "check" {
                let text = if ok { said.clone() } else { format!("{}\n{}", said.lines().next().unwrap_or_default(), clip(&said, TAIL)) };
                let body = json!({ "answer": text, "sure": "high", "found": [], "ran": [], "open": [] }).to_string();
                lanes.settle(&self.0.log, r, &self.0.cfg().root, &body, !ok);
            } else {
                lanes.settle(&self.0.log, r, &self.0.cfg().root, &if ok { said } else { format!("did not finish: {said}") }, true);
            }
        }
    }

    /// `:offload` says whether lines go to chores, and which; `:offload on` or `:offload off` turns it (a proposal in the operator's folder, taken
    /// back by `:undo`; the next start reads it from the file); `:offload yes` sends the review that waits.
    pub(super) fn offload_said(&self, thread: &str, args: &[String]) -> Said {
        let said = Said::Command("offload".into());
        let word = args.first().map(String::as_str);
        if word == Some("yes") {
            return match self.wait_take(thread, Wait::Review) {
                Some(p) => self.chore_run(thread, &p.id, &p.with[0]),
                None => self.refuse(thread, "no review waits for a yes".into()),
            };
        }
        if let Some(w) = word {
            let Some(to) = ["on", "off"].iter().position(|x| x == &w).map(|i| i == 0) else { return self.refuse(thread, format!("`{w}` is not a word of offload: on, off or yes")) };
            if self.0.cfg().offload.chores.is_empty() {
                return self.pack_off(thread, "offload");
            }
            if let Err(why) = self.set_setting(thread, "offload.on", "offload.toml", toml::Value::Boolean(to), &format!("offload: {w}")) {
                return self.refuse(thread, why);
            }
            self.chores().on = Some(to);
            return said;
        }
        let o = &self.0.cfg().offload;
        let classes: Vec<String> = o.chores.iter().map(|c| format!("{}{}", c.id, if c.asks() { " (asks)" } else { "" })).collect();
        let state = if self.offload_on() { "on" } else { "off" };
        self.note(thread, &format!("offload {state} — chores {}; ${:.2} of ${:.2} spent today; order {}; offload.on in Settings › All keeps every line with the main lane", classes.join(", "), self.spent_today("chore"), o.cap_usd, o.order.join(" ")), "dim");
        said
    }

    /// `:setup doer <id>` (or `:setup doer` alone to say it): the doer the main lane's first election falls to. A proposal in the operator's folder
    /// like `:lanes`; it holds for the threads that are not bound yet, and `:switch` moves one that is.
    pub(super) fn setup_pick(&self, thread: &str, args: &[String]) -> Said {
        // there is no Setup page: the pick is said in the chat, which stays open
        let said = Said::Command("setup".into());
        let Some(id) = args.first() else {
            let now = self.main_pick().unwrap_or_else(|| "none: the election chooses".into());
            self.note(thread, &format!("the main lane's doer: {now}; Settings › All › lane.doer picks one (`:setup doer <id>` too)"), "dim");
            return said;
        };
        if !self.knows_doer(id) || !self.0.who.ready("work", &[]).iter().any(|o| o.id == *id) {
            return self.refuse_to(thread, format!("`{id}` is not ready for work: it is not installed, not signed in or not for work · ⏎ opens Agents, which says which"), super::Lead::open("open Agents", "connectors"));
        }
        if let Err(why) = self.set_setting(thread, "lane.doer", "lane.toml", toml::Value::String(id.clone()), &format!("The main lane's doer: {id}")) {
            return self.refuse(thread, why);
        }
        self.chores().pick = Some(Some(id.clone()));
        self.note(thread, &format!("the main lane's doer is {id} for a thread that is not bound yet; `:switch {id}` moves this one"), "accent");
        said
    }

    /// A setting written in the operator's folder as their edit: applied at once, `:undo` takes it back.
    fn set_setting(&self, thread: &str, id: &str, file: &str, value: toml::Value, title: &str) -> Result<(), String> {
        let back = self.edit_setting(id, file, value, title)?;
        self.said_edit(thread, &format!("{title}{back}"), "dim");
        Ok(())
    }
}

/// How a model chore answers: the report of a branch, which J4n checks against the files before the main lane reads it.
const SHAPE: &str = "Write nothing. Reply with only this JSON object: {\"answer\": \"one short paragraph\", \"found\": [{\"claim\": \"...\", \"at\": \"path:line\"}], \"sure\": \"low|mid|high\"}.";
