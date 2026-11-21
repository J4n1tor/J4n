//! Main's tool `chat`: while Main works a turn it starts a scout, a chat that only reads, and goes on. The call is held to what a split is held to, and to a few
//! limits of its own, because nothing checks a call the way a plan is checked before it starts:
//!
//! - **Only Main, in a turn of its own.** A chat's doer is offered the same tools as Main's (the offer is made for a connector, not a thread), so the front refuses
//!   a call from anyone else: depth is one.
//! - **Reads only.** A call that asks to write is refused, not turned into a read. A chat that writes goes through a plan, the clamps and the card.
//! - **The cap counts the chats at work**, those a split started and those a call started: past `orchestrate.chats` the call is refused until one is over.
//! - **The money is what is left of the line's**, less what the chats at work were given, so a run of calls cannot spend the line more than once.
//! - **The clamps of a split** (`clamp`, and the stay-inline tests of `branch::decide`) run over the one brief: a read too small to be worth a chat stays with Main.
//!
//! The call returns at once. The chat's report is read as a digest (`digest`) and reaches Main's next Brief; Main is never made to wait.

use super::split::{Env, Go, take};
use super::Front;
use crate::change::Actor;
use crate::task::{Row, TaskSpec};
use serde_json::{Value as Json, json};

/// What a call asks for: the goal, the paths the chat reads, its doer and model (empty: the cheap tier's), and whether the call asked to write.
#[derive(Debug, Clone, Default)]
pub struct Want {
    pub goal: String,
    pub paths: Vec<String>,
    pub doer: String,
    pub model: String,
    pub write: bool,
}

/// What a model is told on the first line of its session when it may start a scout.
pub const TOOL: &str = "You may start a scout while you work: the tool `chat` with `goal`, `paths` (what it reads) and, if you choose, `doer` and `model`. It only reads, works beside you and reports on its own; the digest of its report comes with your next turn, so do not wait for it. Start one for a read that is big or apart from your work, not for a file you can open yourself. A part that writes is not started this way.";
/// What a doer that runs a program of its own is told after its goal: the kit's bridge is its way to the same tool.
pub const TOOL_CLI: &str = "You may start a scout while you work: `j4n.call('chat', goal='…', paths=['…'])` in `j4n py` (and, if you choose, `doer` and `model`). It only reads, works beside you and reports on its own; the digest of its report comes with your next turn, so do not wait for it. Start one for a read that is big or apart from your work, not for a file you can open yourself. A part that writes is not started this way.";

/// What a call to a chat's work is told to hand back, so the claims in the report have something to check.
const DONE: &str = "an answer that names the files and lines it rests on";

impl Front {
    /// A scout may be started now: the word is `orchestrate`, the chats act and the quick look's switch is on.
    pub fn scouting(&self) -> bool {
        self.main_works() && self.0.cfg().orchestrate.look
    }

    /// The line a doer of Main is told about the tool, on the first line of its session: none for a doer the tool is not offered to, one that is not Main's turn,
    /// or while a scout may not be started. A doer that takes a Brief is told as a model is; one that runs a program of its own is told the way through the bridge.
    pub(super) fn scout_line(&self, thread: &str, spec: &TaskSpec, in_session: bool, as_model: bool) -> Option<&'static str> {
        let told = !in_session && thread == self.top() && spec.role.as_deref() == Some("work") && spec.input.get("branch").is_none() && spec.input.get("bench").is_none();
        (told && self.scouting() && self.0.cfg().orchestrate.scouts.contains(&spec.doer)).then_some(if as_model { TOOL } else { TOOL_CLI })
    }

    /// The tool `chat`, from the turn `task`: start a scout of Main's, or say why not. The Log has the call as `orchestrate.scout`, started or refused.
    pub fn scout(&self, task: &str, want: &Want) -> Result<String, String> {
        let row = self.0.tasks.book().get(task).ok_or("there is no such turn")?;
        let thread = row.spec.thread.clone().filter(|t| !t.is_empty()).unwrap_or_else(|| self.top());
        let mut event = json!({ "task": task, "goal": crate::text::clip(&want.goal, 200), "paths": want.paths, "doer": want.doer, "model": want.model });
        match self.scout_start(&row, &thread, want, &mut event) {
            Ok(text) => {
                self.write(&thread, "orchestrate.scout", Actor::Core, event);
                Ok(text)
            }
            Err(why) => {
                event["refused"] = json!(why);
                self.write(&thread, "orchestrate.scout", Actor::Core, event);
                Err(why)
            }
        }
    }

    fn scout_start(&self, row: &Row, thread: &str, want: &Want, event: &mut Json) -> Result<String, String> {
        let o = self.0.cfg().orchestrate.clone();
        if !self.scouting() {
            return Err("a chat is started only while the word is `orchestrate` and `orchestrate.look` is on".into());
        }
        if want.write {
            return Err("a chat started this way only reads: a part that writes goes through a plan, where code limits it and you are asked first".into());
        }
        if thread != self.top() || row.spec.role.as_deref() != Some("work") || row.spec.input.get("branch").is_some() {
            return Err("only Main starts a chat, in a turn of its own: a chat does not start another".into());
        }
        if o.ask == "always" {
            return Err("every chat waits for the operator's yes (`orchestrate.ask`): a chat is started through a split, which asks".into());
        }
        let book = self.0.tasks.book();
        let at_work: Vec<Row> = book
            .rows()
            .into_iter()
            .filter(|r| !r.state.is_terminal() && r.spec.role.as_deref() == Some("work") && r.spec.input.get("parent").and_then(Json::as_str) == Some(thread) && r.spec.input.get("chat").is_some())
            .collect();
        let cap = self.chat_cap_now();
        if at_work.len() >= cap {
            return Err(format!("{} chat{} at work already (`orchestrate.chats`): go on with your own part, and ask again when a digest says one is over", at_work.len(), if at_work.len() == 1 { " is" } else { "s are" }));
        }
        // the cheap tier is the scout's by default, as it is the quick look's
        let (doer, model) = if want.doer.trim().is_empty() {
            let pick = |a: &str, b: &Option<String>| Some(a.to_string()).filter(|w| !w.is_empty()).or_else(|| b.clone()).unwrap_or_default();
            let cheap = self.cheap_pin(thread);
            (pick(&o.doer, &cheap.doer), pick(&o.model, &cheap.model))
        } else {
            (want.doer.trim().to_string(), want.model.trim().to_string())
        };
        if doer.is_empty() {
            return Err("name a `doer`: no cheap tier is named (`orchestrate.look_doer`, `plan.cheap_doer`)".into());
        }
        event["doer"] = json!(doer);
        event["model"] = json!(model);
        let brief = json!({ "id": "scout", "goal": want.goal, "paths": want.paths, "writes": [], "done": DONE, "doer": doer, "model": model, "mode": "read", "context": "brief" });
        let doers: Vec<String> = self.0.who.doers().iter().map(|d| d.id().to_string()).collect();
        let (briefs, _) = take(&json!({ "chats": [brief], "main": "" }).to_string(), &Env { doers: &doers, packs: &[], checks: &[], seats: None })?;
        // what the chats at work were given is not the line's again
        let given: f64 = at_work.iter().filter_map(|r| r.spec.limits.usd_soft).sum();
        let left = (self.0.cfg().usd_line - given).max(0.0);
        let (go, _, refused, stays): (Vec<Go>, _, _, _) = self.clamps(thread, &row.spec.goal, &briefs, &o, left, event);
        let Some(go) = go.first() else {
            return Err(match (refused.first(), stays.first()) {
                (Some((_, why)), _) => format!("not started: {why}"),
                (None, Some((_, why))) => format!("not started, it is a read for you to do yourself: {why}"),
                (None, None) => "not started: no reason was given".into(),
            });
        };
        let chat = self.chat_start(thread, go)?;
        let (doer, model) = (&go.brief.doer, &go.brief.model);
        event["chat"] = json!(chat);
        event["usd"] = json!(go.usd);
        self.note(thread, &format!("scout · {chat} · {doer}{} · {}", if model.is_empty() { String::new() } else { format!(" · {model}") }, crate::text::line(&go.brief.goal, 120)), "accent");
        Ok(format!("{chat} is started on {doer}, reading {}; it reports on its own, with up to ${:.2}. Its digest comes with your next turn: go on with your own part and do not wait for it.", if go.brief.reads.is_empty() { "what its goal names".to_string() } else { go.brief.reads.join(", ") }, go.usd))
    }
}
