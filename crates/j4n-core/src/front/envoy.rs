//! The envoy: a chat the operator opens that talks the work through with them, drafts the charter they sign in its chat, sends Main their request while the charter holds,
//! shows every question of the work, and says whether the outcome matches what they asked. Behind `crew.envoys`, with `crew.on` and the word `orchestrate`, and only where
//! a screen is there to be asked.
//!
//! What keeps the envoy from being the operator:
//!
//! - **The operator's words come from the Log.** The lines that go to Main as the spec are the operator's own lines typed in the envoy chat, never an argument of a tool.
//! - **The request is not an operator line.** It is written under its own kind (`crew.request`, by the core, with where it came from); it starts Main's turn below the
//!   typed-line path, so no command, answer or steer is read from it, and it is refused while Main works, never steered into the task.
//! - **It carries the envoy's taint.** What any turn of the envoy chat read from outside is held for the chat (the Gate keeps it past the turn), and Main's task starts
//!   tainted by it, and so do the chats Main starts for the request.
//! - **A destructive step that the operator's lines do not hold** is refused before Main is asked.
//! - **Questions of the work are the envoy chat's.** A task whose tree holds an envoy chat asks there; Main's screen says it waits and where.
//! - **The tools are the chat's own.** A call from a thread that is not an envoy chat is refused, and the charter's id and thread are made here.

use super::{Front, Lead, Said, Turn};
use crate::change::Actor;
use crate::charter::Charter;
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;

/// The most requests one charter sends Main, the first included: a differing end check sends the next, and then it stops and asks the operator.
pub const ROUNDS: usize = crate::links::REQUEST_HOPS as usize;
/// How many of the operator's lines, and how many characters of them, travel with a request.
const LINES: usize = 20;
const CHARS: usize = 2000;

/// What the envoy's call to draft a charter says; the id, the thread and the operator's words are made here.
#[derive(Debug, Clone, Default)]
pub struct CharterArgs {
    pub goal: String,
    /// The name of one check of the project to run while J4n is open, and how often (minutes; none: ten).
    pub watch: String,
    pub every: Option<u64>,
    pub paths: Vec<String>,
    pub may: Vec<String>,
    pub doers: Vec<String>,
    pub usd: f64,
    /// How long it holds, in minutes; none: until the operator ends it.
    pub minutes: Option<u64>,
}

/// What an envoy's model is told on the first line of its session about its tools.
pub const TOOLS: &str = "You are the operator's envoy, with two tools. `charter` (goal, paths, may, usd, minutes, watch) drafts what the operator may sign here; you cannot sign it or widen it, and what you leave out of it is not allowed. `request` (charter, text) sends Main the request once a charter is signed in this chat; the operator's own lines go with it. Tell the operator what you drafted and what Main answers; never answer an approval.";
/// The same for a doer that runs a program of its own: the kit's bridge is its way to the tools.
pub const TOOLS_CLI: &str = "You are the operator's envoy, with two tools through `j4n py`: `j4n.call('charter', goal='…', paths=['…'], may=['…'], usd=1.0)` drafts what the operator may sign here (you cannot sign or widen it), and `j4n.call('request', charter='k4', text='…')` sends Main the request once a charter is signed in this chat. Tell the operator what you drafted and what Main answers; never answer an approval.";

/// What the envoys remember in memory: the chats that are envoys (read once from the Log) and what was said once.
#[derive(Default)]
pub(super) struct Envoys {
    chats: Option<BTreeSet<String>>,
    said: BTreeSet<String>,
    /// The request each charter has waiting for Main's turn to end: `(chat, text, source)`, one for each charter.
    pub(super) queued: std::collections::BTreeMap<String, (String, String, &'static str)>,
    /// The watch of each charter.
    pub(super) watch: std::collections::BTreeMap<String, super::watch::Watch>,
}

/// What sending a request came to.
pub enum Sent {
    /// Main's turn started.
    Started,
    /// Main works: the request waits for its turn to end and is then sent, checked again.
    Queued,
}

impl Front {
    /// Envoys work: the switches, the word `orchestrate`, and a screen to be asked on.
    pub fn envoys_on(&self) -> bool {
        let c = self.0.cfg();
        c.crew.on && c.crew.envoys && self.main_works() && self.0.who.charters_answer()
    }

    pub(super) fn envoy_state(&self) -> std::sync::MutexGuard<'_, Envoys> {
        let mut g = self.0.envoys.lock().unwrap_or_else(|e| e.into_inner());
        if g.chats.is_none() {
            g.chats = Some(self.0.log.read(0).filter(|e| e.kind == "chat.seated" && e.data.get("seat").and_then(Json::as_str) == Some(crate::crew::ENVOY)).filter_map(|e| e.thread).collect());
        }
        g
    }

    /// Whether `thread` is a chat that was opened as an envoy: a record the core wrote, never a word a doer says.
    pub fn is_envoy(&self, thread: &str) -> bool {
        self.envoy_state().chats.as_ref().is_some_and(|c| c.contains(thread))
    }

    /// Said once: true the first time `key` is given.
    pub(super) fn once(&self, key: &str) -> bool {
        self.envoy_state().said.insert(key.to_string())
    }

    /// [`once`](Front::once) was called with `key`.
    pub(super) fn was_once(&self, key: &str) -> bool {
        self.envoy_state().said.contains(key)
    }

    /// The envoy chat whose tree `task` is in, when envoys work.
    pub fn envoy_of_task(&self, task: &str) -> Option<String> {
        if !self.envoys_on() {
            return None;
        }
        let chat = super::charters::tree(&self.0, task).into_iter().find(|t| self.is_envoy(t))?;
        // a question of the work goes to its envoy only where a link carries it; a refused one is shown where questions of that task were shown before
        if self.links_hold() {
            let asker = self.0.tasks.book().get(task).and_then(|r| r.spec.thread).unwrap_or_default();
            let (from, to) = (self.party(&asker), crate::links::Party::Envoy(chat.clone()));
            if let Err(why) = self.link_admit(&from, &to, "question", &chat) {
                if self.once(&format!("refused:{task}")) {
                    self.link_refused(&chat, &from, &to, "question", &why);
                }
                return None;
            }
        }
        Some(chat)
    }

    /// `:envoy`: open the chat and go into it. The refusal leads to the switch.
    pub fn envoy_said(&self, thread: &str) -> Said {
        if !self.envoys_on() {
            let c = self.0.cfg();
            let why = if !c.crew.envoys {
                "envoys are off · ⏎ turns them on"
            } else if !c.crew.on {
                "the crew is off · ⏎ turns it on"
            } else if !self.main_works() {
                "an envoy works under the word `orchestrate`: `:lanes orchestrate`"
            } else {
                "an envoy needs a screen: with no one to ask there is no envoy"
            };
            let lead = if !c.crew.envoys { Lead::set("turn envoys on", "crew.envoys", json!(true)) } else if !c.crew.on { Lead::set("turn the crew on", "crew.on", json!(true)) } else { Lead::run("say the word", "lanes", &["orchestrate"]) };
            return self.refuse_to(thread, why.into(), lead);
        }
        let chat = self.open_chat(None, "envoy", "", "");
        self.write(&chat, "chat.seated", Actor::Core, json!({ "seat": crate::crew::ENVOY, "role": "envoy", "reason": "the operator opened it", "narrowed": [] }));
        self.envoy_state().chats.get_or_insert_with(Default::default).insert(chat.clone());
        // the seat's words and tool lists are the first writes of the chat's own settings, as for any seated chat
        if let Some(seat) = self.0.cfg().crew.seats.iter().find(|s| s.id == crate::crew::ENVOY) {
            for (key, value) in [("personality", json!(seat.personality)), ("tools.allow", json!(seat.tools_allow)), ("tools.deny", json!(seat.tools_deny))] {
                // a list the seat leaves empty is nothing to write
                if value.as_array().is_some_and(Vec::is_empty) {
                    continue;
                }
                if let Err(why) = self.chat_set(&chat, key, &value, &crate::crew::Writer::Seat) {
                    self.note(&chat, &format!("setting `{key}` was not set: {why}"), "warn");
                }
            }
        }
        self.note(&chat, "envoy · it talks the work through with you, drafts what you sign here, and tells Main", "dim");
        Said::Enter(chat)
    }

    /// The operator's own lines in `chat`, oldest first, the latest few: what the spec is checked against.
    pub(super) fn operator_lines(&self, chat: &str) -> Vec<String> {
        let mut lines: Vec<String> = self
            .0
            .log
            .read(0)
            .filter(|e| e.kind == "chat.say" && e.thread.as_deref() == Some(chat) && e.by == Actor::Operator)
            .filter_map(|e| e.data.get("text").and_then(Json::as_str).map(|t| t.trim().to_string()))
            .filter(|t| !t.is_empty() && !t.starts_with([':', '/', '@']))
            .collect();
        let from = lines.len().saturating_sub(LINES);
        lines.drain(..from);
        let mut chars = 0;
        lines.retain(|l| {
            chars += l.chars().count();
            chars <= CHARS
        });
        lines
    }

    /// The thread a tool call comes from, when it is an envoy chat of this front's and envoys work.
    fn envoy_caller(&self, task: &str) -> Result<String, String> {
        if !self.envoys_on() {
            return Err("envoys are off".into());
        }
        let thread = self.0.tasks.book().get(task).and_then(|r| r.spec.thread).unwrap_or_default();
        if self.is_envoy(&thread) { Ok(thread) } else { Err("only an envoy chat does this".into()) }
    }

    /// The tool `charter`: draft the charter for the operator to sign in this chat. The id, the thread and the operator's words are made here; a charter that was
    /// signed is changed by a new draft, which is signed again by the operator in this chat and by no one else.
    pub fn envoy_charter(&self, task: &str, a: &CharterArgs) -> Result<String, String> {
        let chat = self.envoy_caller(task)?;
        let now = self.0.log.clock().now_ms();
        let id = format!("k{}", chat.trim_start_matches('c'));
        let checks = self.0.cfg().checks.clone();
        if !a.watch.trim().is_empty() && !checks.iter().any(|c| *c == a.watch.trim()) {
            return Err(format!("`{}` is not a check of this project ({}): name one of them to watch", a.watch.trim(), if checks.is_empty() { "it has none".to_string() } else { checks.join(", ") }));
        }
        let c = Charter {
            id: id.clone(),
            goal: a.goal.trim().to_string(),
            watch: a.watch.trim().to_string(),
            watch_every_min: a.every.unwrap_or(10),
            paths: a.paths.iter().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect(),
            may: a.may.iter().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect(),
            doers: a.doers.iter().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect(),
            usd: a.usd,
            until_ms: a.minutes.map(|m| now + m * 60_000),
            thread: chat.clone(),
            verbatim: self.operator_lines(&chat),
        };
        let proposal = self.charter_draft_from(Actor::Core, &c, Some(&chat))?;
        self.note_to(&chat, &format!("charter {id} drafted for you to sign · ⏎ shows what it would answer"), "accent", Lead::run("show the charter", "accept", &[&proposal]));
        Ok(format!("{proposal} is drafted: the operator reads it and signs it in this chat. Say so, and wait for the signing; you cannot sign or widen it."))
    }

    /// The tool `request`: send Main the operator's request under the charter that holds. Past the round cap, the budget or the end, and with a destructive step the
    /// operator's own lines do not hold, it is refused; while Main works it waits for the end of Main's turn and is then sent (never steered into the turn).
    pub fn envoy_request(&self, task: &str, charter: &str, text: &str) -> Result<String, String> {
        let chat = self.envoy_caller(task)?;
        match self.envoy_send(&chat, charter, text, "envoy")? {
            Sent::Started => Ok("sent to Main: its report comes back to you, and you say whether it matches".into()),
            Sent::Queued => Ok("Main is working: the request is waiting for its turn to end and is then sent; its report comes back to you".into()),
        }
    }

    /// Everything a request is held to, checked now: the charter holds in this chat, the rounds since it was signed, its budget, and the operator's words. Then it is
    /// sent, or queued while Main works (one waiting request for each charter, checked again when it is sent).
    pub(super) fn envoy_send(&self, chat: &str, charter: &str, text: &str, source: &'static str) -> Result<Sent, String> {
        let now = self.0.log.clock().now_ms();
        let held = self.0.cfg().charters.live(now).into_iter().find(|s| s.charter.id == charter && s.charter.thread == chat).ok_or_else(|| format!("charter {charter} is not signed in this chat, or it has ended: the operator signs it here first"))?;
        let c = &held.charter;
        if text.trim().is_empty() {
            return Err("a request needs words".into());
        }
        let top = self.0.cfg().thread.clone();
        // the link says how many requests one charter sends Main (the star's, three) and whether this envoy may send one; the requests are counted since this
        // signing, so a charter signed again starts its rounds again
        let sent = self.0.log.read(0).filter(|e| e.kind == "crew.request" && e.at_ms >= held.at_ms && e.data.get("charter").and_then(Json::as_str) == Some(charter)).count();
        let (from, to) = (crate::links::Party::Envoy(chat.to_string()), crate::links::Party::Main);
        if let Err(refused) = self.0.cfg().crew.links.admit(&from, &to, "request", &|_| sent) {
            let why = match refused {
                crate::links::Refused::Hops { cap, .. } => format!("charter {charter} has sent Main {cap} requests: ask the operator before another"),
                other => other.to_string(),
            };
            self.write(chat, "crew.refused", Actor::Core, json!({ "carries": "request", "from": from.name(), "to": to.name(), "why": why }));
            return Err(why);
        }
        if self.0.cfg().charters.spent(&c.thread) >= c.usd {
            return Err(format!("charter {charter} has spent its ${:.2}: ask the operator before another request", c.usd));
        }
        let lines = self.operator_lines(chat);
        if let Some(step) = super::clamp::quoted_destruction(text, &lines.join(" ")) {
            return Err(format!("the request carries `{step}`, which the operator's own lines do not say: ask the operator"));
        }
        if self.settled(&top).is_none() {
            self.envoy_state().queued.insert(charter.to_string(), (chat.to_string(), text.to_string(), source));
            return Ok(Sent::Queued);
        }
        let b = match self.binding(&top) {
            Some(b) => b,
            None => self.bind_first(&top, text, None)?,
        };
        let taint = self.0.who.gate().and_then(|g| g.held(chat));
        let spoken: Vec<String> = lines.iter().map(|l| format!("- {l}")).collect();
        let by = if source == "watch" { "watch" } else { "request" };
        let line = format!(
            "you via {chat} · charter {charter} · {by}: {}\n\nThe operator's own words, which are what this is checked against; a request that does not match them is a question to the operator, not a guess:\n{}",
            text.trim(),
            spoken.join("\n")
        );
        self.write(&top, "crew.request", Actor::Core, json!({ "via": chat, "charter": charter, "source": source, "text": crate::text::clip(text.trim(), 400), "lines": lines.len(), "taint": taint, "round": sent + 1, "usd": c.usd }));
        self.note(&top, &format!("you via {chat} · charter {charter} · {by} · {}", crate::text::line(text.trim(), 100)), "accent");
        let mut extra = json!({ "envoy": { "chat": chat, "charter": charter } });
        if let Some(from) = &taint {
            extra["taint_from"] = json!(from);
        }
        let said = self.begin(&top, &line, &b, &[], Turn { item: false, compact: false, fresh: false, carry: None, operator: false, brief: None, input: Some(extra) });
        match said {
            Said::Started { .. } => Ok(Sent::Started),
            Said::Refused(why) => Err(why),
            other => Err(format!("Main did not start: {other:?}")),
        }
    }

    /// Main's turn is over: the request that waited is sent, checked again; one that can no longer be sent is said in the envoy chat.
    fn queued_send(&self, top: &str) {
        if self.settled(top).is_none() {
            return;
        }
        let waiting: Vec<(String, (String, String, &'static str))> = std::mem::take(&mut self.envoy_state().queued).into_iter().collect();
        for (charter, (chat, text, source)) in waiting {
            match self.envoy_send(&chat, &charter, &text, source) {
                Ok(_) => {}
                Err(why) => self.note(&chat, &format!("the request that waited for Main was not sent: {why}"), "warn"),
            }
        }
    }

    /// A turn of Main for the envoy, or of the envoy for Main's report: what a settled task starts next. Called with the thread of the step.
    pub(super) fn envoy_step(&self, thread: &str) {
        if !self.envoys_on() {
            return;
        }
        let top = self.0.cfg().thread.clone();
        if thread == top {
            self.end_check(&top);
            self.queued_send(&top);
        } else if self.is_envoy(thread) {
            self.end_checked(thread);
        }
    }

    /// Main's turn for a request ended well: the envoy is asked whether the report matches the spec, once.
    fn end_check(&self, top: &str) {
        let Some(row) = self.last_task(top).filter(|r| r.state == crate::task::State::Done { ok: true } && r.ended_ms.is_some_and(|t| t >= self.0.since_ms)) else { return };
        let Some(chat) = row.spec.input.pointer("/envoy/chat").and_then(Json::as_str).map(String::from) else { return };
        let charter = row.spec.input.pointer("/envoy/charter").and_then(Json::as_str).unwrap_or_default().to_string();
        if self.last_task(&chat).is_some_and(|r| !r.state.is_terminal()) || !self.0.cfg().charters.holds(&charter, self.0.log.clock().now_ms()) {
            return;
        }
        let Some(b) = self.binding(&chat) else { return };
        if !self.once(&format!("check:{}", row.id)) {
            return;
        }
        if self.link_pass(&crate::links::Party::Main, &crate::links::Party::Envoy(chat.clone()), "report", &chat).is_err() {
            return;
        }
        let report = crate::text::clip(row.answer.as_deref().unwrap_or_default().trim(), 1500);
        let told = format!("J4n: Main reports its work done:\n{report}\n\nCompare it with your spec and the operator's own words in this chat. Answer with one JSON object and nothing else: {{\"matches\": true, \"differs\": [\"what differs, if anything\"]}}.");
        let input = Some(json!({ "endcheck": { "charter": charter, "of": row.id } }));
        let _ = self.begin(&chat, &told, &b, &[], Turn { item: false, compact: false, fresh: false, carry: None, operator: false, brief: None, input });
    }

    /// The envoy's answer to the end check: it says what it found; a difference goes back to Main under the charter, until the rounds are used, then to the operator.
    fn end_checked(&self, chat: &str) {
        let Some(row) = self.last_task(chat).filter(|r| r.state.is_terminal() && r.ended_ms.is_some_and(|t| t >= self.0.since_ms)) else { return };
        let Some(of) = row.spec.input.pointer("/endcheck/of").and_then(Json::as_str).map(String::from) else { return };
        let charter = row.spec.input.pointer("/endcheck/charter").and_then(Json::as_str).unwrap_or_default().to_string();
        if !self.once(&format!("checked:{}", row.id)) {
            return;
        }
        let answer = row.answer.as_deref().unwrap_or_default();
        let v = answer.find('{').zip(answer.rfind('}')).and_then(|(a, b)| serde_json::from_str::<Json>(&answer[a..=b.max(a)]).ok());
        let matches = v.as_ref().and_then(|v| v.get("matches")).and_then(Json::as_bool);
        let differs: Vec<String> = v.as_ref().and_then(|v| v.get("differs")).and_then(Json::as_array).map(|a| a.iter().filter_map(Json::as_str).map(String::from).collect()).unwrap_or_default();
        let top = self.0.cfg().thread.clone();
        self.write(&top, "crew.checked", Actor::Core, json!({ "via": chat, "charter": charter, "of": of, "matches": matches, "differs": differs }));
        match matches {
            Some(true) => self.note(chat, &format!("end check · matches what you asked · charter {charter} stays until it ends"), "good"),
            Some(false) if !differs.is_empty() => {
                let what = differs.join("; ");
                match self.envoy_send(chat, &charter, &format!("the report differs from the spec: {what}. Put it right inside the charter."), "envoy") {
                    Ok(_) => self.note(chat, &format!("end check · differs: {what} · sent back to Main"), "warn"),
                    Err(why) => self.note(chat, &format!("end check · differs: {what} · not sent back ({why}) · it is yours to say"), "warn"),
                }
            }
            _ => self.note(chat, "end check · the envoy did not say whether it matches · it is yours to say", "warn"),
        }
    }

    /// What an envoy chat's turn is given: a mode that only reads. Its seat's words are in its own settings layer, written when it was opened.
    pub(super) fn envoy_seat(&self, thread: &str, spec: &mut crate::task::TaskSpec) {
        if self.is_envoy(thread) {
            spec.tune.mode = crate::task::Mode::Read;
        }
    }

    /// The line an envoy's doer is told about its tools, on the first line of its session: a model in its contract, a doer with a program of its own after its goal.
    pub(super) fn envoy_line(&self, thread: &str, in_session: bool, as_model: bool) -> Option<&'static str> {
        (!in_session && self.is_envoy(thread) && self.envoys_on()).then_some(if as_model { TOOLS } else { TOOLS_CLI })
    }

    /// What a chat Main starts for a request carries of it: the envoy it is for (so its questions go there) and what the envoy read from outside.
    pub(super) fn envoy_inherit(&self, parent: &str, spec: &mut crate::task::TaskSpec) {
        let Some(main) = self.last_task(parent) else { return };
        for key in ["envoy", "taint_from"] {
            if let Some(v) = main.spec.input.get(key) {
                spec.input[key] = v.clone();
            }
        }
        if spec.input.get("taint_from").is_none()
            && let Some(from) = self.0.who.gate().and_then(|g| g.tainted(&main.id))
        {
            spec.input["taint_from"] = json!(from);
        }
    }

    /// Said once in Main: a question of the work waits in the envoy chat, and the row leads there.
    pub(super) fn envoy_waits(&self, ask: &str, chat: &str, question: &str) {
        if !self.once(&format!("waits:{ask}")) {
            return;
        }
        let top = self.0.cfg().thread.clone();
        self.note_to(&top, &format!("{chat} · a question of its work waits there: {}", crate::text::line(question, 80)), "warn", Lead { label: format!("go into {chat}"), intent: j4n_proto::Intent::Enter { chat: chat.into() }, risk: 0 });
    }
}

impl Front {
    /// What the Gate asks to tell which thread holds what a task read: an envoy chat does.
    pub fn envoy_hold_hook(&self) -> crate::gate::Holder {
        let core = std::sync::Arc::downgrade(&self.0);
        Box::new(move |task| {
            let front = Front(core.upgrade()?);
            let thread = front.0.tasks.book().get(task)?.spec.thread?;
            front.is_envoy(&thread).then_some(thread)
        })
    }
}
