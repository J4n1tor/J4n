//! A line is weighed before it is sent when the setting `weigh.on` is on: the weight chooses **where the
//! line goes**, never the model of the session the chat is bound to (changing it throws the cache away). The ask point `weigh` is walked in
//! this order: the local table (`rule:weigh`, the `phrase` rows with a `weight`), then Jev (`decider:jev`), then the point's fallback `task`:
//!
//! - `answer`: a line that needs no tool and no project (`ping`) is answered by a cheap model with no tools, the doer `weigh.answer`, given nothing
//!   but the line (scrubbed by the Log's redactor) and in no session of the chat;
//! - `light`: a short question about the project goes to a lean session on the cheap tier (`plan.cheap_doer`, `plan.cheap_model`) in mode `read`, in a
//!   session of its own; `lean` of a doer's connector is empty until it is measured, and then the line is a `task` and the Log says so;
//! - `task`: as it always was: the chat's doer, in its session.
//!
//! Every place that cannot take a line says why in `intake.decided` (`weighed`, `was`) and in a row, and the line goes on as a task. Nothing is
//! written and nothing is asked with the switch off (the point is not walked and its cases are not put to Jev). A line typed while a task works,
//! while a plan is open, or with rows kept beside it is not weighed, nor is one that names a doer in exact syntax (`@codex …`, that is its place), nor, unless the
//! table knows it, one that goes on from what the doer said: any line after an answer that ended in a question, and any line that no Decider is sure stands alone
//! (`read:goes_on`), which only the doer's own session can read (Jev is given the bare line). No word of a line decides this. `:ask <line>` sends a line to the chat's doer as it is,
//! whatever it would weigh (it skips this, as it skips the phrase table).

use super::{Front, Said, named};
use crate::ask::Opt;
use crate::change::Actor;
use crate::task::Mode;
use serde_json::json;

/// What `weigh` offers.
pub const WEIGHS: [&str; 3] = ["answer", "light", "task"];

/// The roles of the tasks a weighed line starts, and of the quick look's. None is a session the chat carries on in (`own_tasks`).
pub const ROLES: [&str; 3] = ["answer", "light", "look"];

/// What an answer may take, and what a light line may.
const ANSWER_USD: f64 = 0.02;
const LIGHT_USD: f64 = 0.15;

/// What the front is set to weigh with.
#[derive(Clone)]
pub struct Weigh {
    /// The switch, `weigh.on`: off until a drill of mixed lines has been measured.
    pub on: bool,
    /// The doer that answers (`weigh.answer`).
    pub answer: String,
    /// The doers whose connector starts a `light` task with a lean profile that is not empty (`Connectors::lean_doers`).
    pub lean: Vec<String>,
}

impl Default for Weigh {
    fn default() -> Weigh {
        Weigh { on: false, answer: "light".into(), lean: Vec::new() }
    }
}

/// Where a weighed line goes.
struct Place {
    doer: String,
    model: Option<String>,
    endpoint: Option<String>,
    account: Option<String>,
    role: &'static str,
    usd: f64,
}

impl Front {
    /// Weigh a line and, when it is `answer` or `light` and there is a place that can take it, start its task. `None`: the line goes on as it
    /// always went (the switch is off, the line is not one to weigh, it is a `task`, or the place it was weighed for cannot take it).
    pub(super) fn weigh_take(&self, thread: &str, text: &str, keys: &[String]) -> Option<Said> {
        if !self.0.cfg().weigh.on || !keys.is_empty() || self.0.keeper.as_ref().is_some_and(|k| k.open(thread)) || self.settled(thread).is_none() {
            return None;
        }
        // a line that names a doer or a model is where that says (`ask codex what this project does`), not a weight's
        if self.names_something(thread, text) {
            self.kept_task(thread, "front:named", "the line names a doer or a model");
            return None;
        }
        if self.0.phrases.weigh(text).is_none()
            && let Some(why) = self.follow_up(thread, text)
        {
            self.kept_task(thread, "front:follow-up", &why);
            return None;
        }
        let options = WEIGHS.iter().map(|id| Opt { id: (*id).into(), label: (*id).into(), recommended: *id == "task" }).collect();
        let q = self.0.who.question("weigh", Some(thread), text, options);
        let (pick, by, why, _) = self.0.who.pick("weigh", Some(thread), &q).unwrap_or_else(|| ("task".into(), "front".into(), "no ask point `weigh`".into(), None));
        if pick == "task" {
            self.write(thread, "intake.decided", Actor::Core, json!({ "pick": "task", "by": by, "why": why, "weighed": true }));
            return None;
        }
        let place = match self.weigh_place(thread, &pick) {
            Ok(p) => p,
            Err(no) => {
                self.write(thread, "intake.decided", Actor::Core, json!({ "pick": "task", "by": by, "why": no, "weighed": true, "was": pick }));
                self.note(thread, &format!("weighed `{pick}` ({by}), but {no}: it goes to the doer as a task"), "dim");
                return None;
            }
        };
        let goal = if pick == "answer" { self.0.log.redactor().scrub(text) } else { text.to_string() };
        let mut spec = match self.spec(thread, &goal, &place.doer) {
            Ok(spec) => spec,
            Err(e) => return Some(self.refuse(thread, e)),
        };
        spec.role = Some(place.role.into());
        // a question is read, never worked: a task in `read` is not given a way to write
        spec.tune.mode = Mode::Read;
        (spec.tune.model, spec.tune.endpoint, spec.account) = (place.model.clone(), place.endpoint, place.account);
        self.login_for(thread, &mut spec);
        spec.limits.usd_soft = [spec.limits.usd_soft, Some(place.usd)].into_iter().flatten().reduce(f64::min);
        let who = crate::task::who(&place.doer, place.model.as_deref().unwrap_or_default(), "");
        match self.0.tasks.spawn(spec, Actor::Operator) {
            Ok(task) => {
                self.write(thread, "intake.decided", Actor::Core, json!({ "pick": pick, "by": by, "why": why, "weighed": true, "doer": place.doer, "model": place.model, "task": task }));
                self.note(thread, &format!("took this for `{pick}` ({why}): {who} as {task}; `:ask {text}` sends it to the doer of this chat as it is"), "dim");
                Some(Said::Started { task })
            }
            Err(p) => Some(self.refuse(thread, p.msg)),
        }
    }

    fn kept_task(&self, thread: &str, by: &str, why: &str) {
        self.write(thread, "intake.decided", Actor::Core, json!({ "pick": "task", "by": by, "why": why, "weighed": true }));
    }

    /// Whether the line names a doer in exact syntax (`@codex …`, `pi: …`), or the read found that it asks for one.
    pub(super) fn names_something(&self, thread: &str, text: &str) -> bool {
        let ids: Vec<&str> = self.0.who.doers().iter().map(|d| d.id()).collect();
        named::parse(text, &ids).is_some() || self.read_named(thread).is_some()
    }

    /// Why a line in a thread that has a doer goes on from what the doer said, so only its session can read it: the doer's last answer ended in a question.
    /// Or no Decider is sure it stands alone. None: it stands alone.
    fn follow_up(&self, thread: &str, text: &str) -> Option<String> {
        self.binding(thread)?;
        let last = self.last_task(thread).filter(|r| r.state.is_terminal() && !r.spec.role.as_deref().is_some_and(|x| ROLES.contains(&x)))?;
        let ends = last.answer.as_deref().is_some_and(|a| a.trim_end_matches(|c: char| c.is_whitespace() || "\"'`)*_".contains(c)).ends_with('?'));
        if ends {
            return Some(format!("the last answer of {} ended in a question: this line may answer it", last.id));
        }
        // a line stands alone only when a Decider is sure of it (`read:goes_on`); no word of it is looked at, and where none is sure the doer's own session reads it
        (self.goes_on(thread, text) != Some(false)).then(|| "no Decider is sure the line stands alone: only the doer's session can read it".to_string())
    }

    /// The doer, model and account a weighed line goes to; `Err`: why nothing there can take it.
    fn weigh_place(&self, thread: &str, pick: &str) -> Result<Place, String> {
        let (cfg, ready) = (&self.0.cfg(), |role: &str, doer: &str| self.0.who.ready(role, &[]).iter().any(|o| o.id == doer));
        if pick == "answer" {
            let id = &cfg.weigh.answer;
            if !self.knows_doer(id) {
                return Err(format!("no doer `{id}` is there to answer it (`weigh.answer`; the pack `jev` has `light`, which needs a key)"));
            }
            if !ready("answer", id) {
                return Err(format!("{id} is not ready: it has no key, is not installed or is not for answering"));
            }
            return Ok(Place { doer: id.clone(), model: None, endpoint: None, account: None, role: "answer", usd: ANSWER_USD });
        }
        let cheap = &cfg.cheap;
        if cheap.doer.is_none() && cheap.model.is_none() {
            return Err("no cheap tier is named (`plan.cheap_doer`, `plan.cheap_model`)".into());
        }
        let base = match (self.binding(thread), &cheap.doer) {
            (Some(b), _) => b,
            (None, Some(d)) if self.knows_doer(d) => self.bind_to(d.clone(), None, "light").0,
            (None, Some(d)) => return Err(format!("the cheap doer `{d}` is not one J4n has")),
            (None, None) => return Err("the chat has no doer yet, and the cheap tier names none".into()),
        };
        let b = self.pinned(cheap, &base, "cheap")?;
        if !cfg.weigh.lean.contains(&b.doer) {
            return Err(format!("{} has no lean profile to start it with (`lean` is empty until it is measured, or the setting `profile` is `full`)", b.doer));
        }
        if !ready("scout", &b.doer) {
            return Err(format!("{} is not ready for a read: it is not installed or not signed in", b.doer));
        }
        Ok(Place { doer: b.doer, model: b.model, endpoint: b.endpoint, account: b.account, role: "light", usd: LIGHT_USD })
    }
}
