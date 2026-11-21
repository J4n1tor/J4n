//! The ask point `retry` in the front: a chat turn that ended `Failed` is asked once whether to try the same
//! doer again, to hand the line to another doer, or to stop. Behind the switch `retry.auto` (off): with it off nothing here runs and a failure is what
//! it was, a stop whose menu offers `try again` to the operator. Asked of failures that may mend (class `transient`, or none), never of a limit (that
//! is waited out, `cool-on-limit`) nor of a failure that will not mend (`permanent`), nor of a branch, a chore, a question or an item of a plan, which
//! have their own paths. The chain is `rule:retry`, then the decider; the fallback is `stop`.

use super::Front;
use crate::answer::{Answerer, field, fn_rule, picked};
use crate::ask::Opt;
use crate::change::Actor;
use crate::task::{Row, State, Why};
use serde_json::json;

/// The most failures in a row that are tried again, the setting `retry.max` (the failed task not counted).
const MAX: u32 = 1;

/// What a failure is put to the point as: the sentence (how often, with what), and the facts as lines the rule reads, `same: yes` when it is what
/// happened the time before and `class: X` (a change in the sentence changes no pick).
pub fn failure_text(times: usize, message: &str, same: bool, class: Option<&str>) -> String {
    let how = match times {
        1 => "once".to_string(),
        2 => "twice".to_string(),
        n => format!("{n} times"),
    };
    let one: String = message.split_whitespace().collect::<Vec<_>>().join(" ");
    let one = if one.chars().count() > 160 { format!("{}…", one.chars().take(160).collect::<String>()) } else { one };
    format!("the task failed {how} with `{one}`{}{}", if same { "\nsame: yes" } else { "" }, class.map(|c| format!("\nclass: {c}")).unwrap_or_default())
}

/// What `rule:retry` says of a question `failure_text` made: a failure that came again, or that is permanent, is a stop. Anything else is not the
/// rule's to decide.
pub fn retry_pick(text: &str) -> Option<(&'static str, String)> {
    if field(text, "class") == Some("permanent") {
        return Some(("stop", "the failure is permanent: no doer can mend it".into()));
    }
    if field(text, "class") == Some("refused") {
        return Some(("stop", "the run was refused a tool: it would be refused again".into()));
    }
    (field(text, "same") == Some("yes")).then(|| ("stop", "the same failure came again".into()))
}

/// `rule:retry`.
pub fn retry_rule() -> impl Answerer {
    fn_rule("rule:retry", |q| retry_pick(&q.text).map_or_else(|| Err("nothing in the failure says it is one that will not mend".into()), |(id, why)| picked(id, why)))
}

/// A failure's message and class, if the row is one.
fn failed(r: &Row) -> Option<(&str, Option<&str>)> {
    match &r.state {
        State::Stopped { why: Why::Failed { message } } => Some((message.as_str(), r.class.as_deref())),
        _ => None,
    }
}

/// The words of a failure with what changes from one try to the next (numbers, case) taken out, to tell if it is the same one.
fn same_words(m: &str) -> String {
    m.chars().filter(|c| !c.is_ascii_digit()).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

impl Front {
    /// The switch `retry.auto`, and the bound `retry.max`.
    fn retry_on(&self) -> (bool, u32) {
        let set = &self.screen().set;
        (matches!(set.pointer("/retry/auto"), crate::value::Value::Bool(true)), match set.pointer("/retry/max") {
            crate::value::Value::Num(n) if n >= 0.0 => n as u32,
            _ => MAX,
        })
    }

    /// Called when a task ends. With a screen attached the decider is asked on a thread of its own, so the screen goes on drawing; a run with nobody at
    /// the keyboard asks in place, because its loop ends when no task is open.
    pub(super) fn retry_step(&self, thread: &str) {
        if !self.retry_on().0 {
            return;
        }
        if self.0.who.headless() {
            return self.retry_now(thread);
        }
        let thread = thread.to_string();
        self.off_thread(format!("retry {thread}"), true, move |front| front.retry_now(&thread));
    }

    /// The failure at the end of the thread, if it is one to ask about, and the failures in a row it ends (newest first).
    fn retry_now(&self, thread: &str) {
        let max = self.retry_on().1;
        let book = self.0.tasks.book();
        let Some(last) = self.settled(thread).flatten().filter(|r| r.spec.role.as_deref() == Some("work")) else { return };
        let (Some((message, class)), true) = (failed(&last), last.spec.input.get("branch").is_none()) else { return };
        if !matches!(class, None | Some("transient")) || self.0.keeper.as_ref().and_then(|k| k.plan(thread)).is_some_and(|p| p.items.iter().any(|i| i.task == last.id)) {
            return;
        }
        // asked once for each failure
        if !self.0.retried.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(last.id.clone()) {
            return;
        }
        let mut chain = vec![last.clone()];
        while let Some(prev) = chain.last().and_then(|r| r.spec.follows.as_deref()).and_then(|f| book.get(f)).filter(|p| failed(p).is_some() && chain.len() < 16) {
            chain.push(prev);
        }
        if chain.len() as u32 > max {
            return self.note(thread, &format!("retry: stopped after {} failures in a row (`retry.max` is {max})", chain.len()), "dim");
        }
        let same = chain.get(1).and_then(|p| failed(p)).is_some_and(|(m, _)| same_words(m) == same_words(message));
        let options = ["retry", "escalate", "stop"].iter().map(|id| Opt { id: (*id).into(), label: (*id).into(), recommended: *id == "stop" }).collect();
        let mut q = self.0.who.question("retry", Some(thread), &failure_text(chain.len(), message, same, class), options);
        q.goal = last.spec.goal.clone();
        let (pick, by, why) = self.0.who.pick("retry", Some(thread), &q).map_or(("stop".into(), "front".into(), "no ask point `retry`".into()), |(id, by, why, _)| (id, by, why));
        self.write(thread, "retry.decided", Actor::Core, json!({ "task": last.id, "pick": pick, "by": by, "why": why, "failures": chain.len() }));
        // never mid-turn: the decider took its time, and the line may have gone on since: a turn that began is not put aside for the one that failed
        if pick != "stop" && self.settled(thread).flatten().is_none_or(|r| r.id != last.id) {
            return self.note(thread, &format!("retry: not acted on ({pick}): a turn of the chat began while the decider was thinking"), "dim");
        }
        match pick.as_str() {
            "retry" => match self.0.tasks.resume(&last.id, None, Actor::Core) {
                Ok(t) => self.note(thread, &format!("retry: {} again as {t} — {why}", last.spec.doer), "accent"),
                Err(p) => self.note(thread, &format!("retry: not tried again: {}", p.msg), "warn"),
            },
            "escalate" => self.escalate(thread, &last, &chain, &why),
            _ => self.note(thread, &format!("retry: stop — {why}"), "dim"),
        }
    }

    /// The line of a failed turn handed to a doer that is not one of the doers that failed it, as a task that follows it.
    fn escalate(&self, thread: &str, last: &Row, chain: &[Row], why: &str) {
        let tried: Vec<String> = chain.iter().map(|r| r.spec.doer.clone()).collect();
        let text = format!("{} failed: {}", last.spec.doer, last.spec.goal);
        let (doer, _, how) = match self.0.who.doer("work", Some(thread), &text, &tried) {
            Ok(d) => d,
            Err(e) => return self.note(thread, &format!("retry: stop — {e}"), "dim"),
        };
        let (b, _) = self.bind_to(doer.clone(), None, "escalate");
        let mut spec = match self.spec(thread, &last.spec.goal, &doer) {
            Ok(s) => s,
            Err(e) => return self.note(thread, &format!("retry: not handed on: {e}"), "warn"),
        };
        spec.role = Some("work".into());
        (spec.tune.model, spec.tune.endpoint, spec.account, spec.follows) = (b.model, b.endpoint, b.account, Some(last.id.clone()));
        self.login_for(thread, &mut spec);
        self.briefed(thread, &mut spec, &last.spec.goal, false, (None, None, None));
        match self.0.tasks.spawn(spec, Actor::Core) {
            Ok(t) => self.note(thread, &format!("retry: handed to {doer} as {t} ({how}) — {why}"), "accent"),
            Err(p) => self.note(thread, &format!("retry: not handed on: {}", p.msg), "warn"),
        }
    }
}
