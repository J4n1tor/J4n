//! Chats, when the setting `chats.on` is on: every prompt can be a chat of its own.
//! A chat is a thread with its own doer session, named `c3`; a plan's branch is a chat under its chat, `c3.1`, which is the view of one branch task
//! of its parent's thread. The orchestrator's thread (`Cfg.thread`, the one the screen opens on) has no doer: a line typed there is **routed**. Under `lanes = orchestrate` it is Main, a chat with a doer of its own
//! (`who:main`): a line is Main's own turn unless code finds a chat for it.
//!
//! - **Code first**: a line goes to the chat it matches (it names a file the chat has touched, or it shares words with what the chat was told), and
//!   opens a new chat when it matches none. Jev (the ask point `chats`) is asked only when code has no match and there are chats, or, with more than one
//!   match, for the hint on the question the operator is asked. Where no Decider answers (off, no key, or not sure) and there are chats to join, the
//!   operator is asked, with the chat the line names (`c1`) recommended: a new chat is opened without a question only when Jev is sure, or there is
//!   none to join. The Log says why: `Decider off`.
//! - **Asks**: more than one chat matches, or the one that does is cooling or failed: one question, `which chat takes this?`, answered from the
//!   orchestrator (`route:<n>`, a pick, or the number typed). Nothing is guessed, nothing is sent until it is answered.
//! - **The route row** comes after the send: `→ c3 · same files · parser.rs`, in the orchestrator's turn, and the Log has `chat.routed`. A line is
//!   never moved afterwards.
//! - **The words**: `:new`, `:new <words>`, `:fresh`, `:clear`, `:history`, `:chats`, `:top`. With the switch off `:new` is `:fresh` and the rest say the
//!   chats are off; nothing here is probed, asked or written.

use super::thread::{CHAT_LINE, ChatRow};
use super::{Front, Said, askview, rows};
use crate::ask::Opt;
use crate::change::Actor;
use crate::task::{HUMAN, Row, State};
use j4n_proto::{AskView, By, Cell, Choice, Row as PRow, Shape, Tone};
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;

/// What the front is set to do.
#[derive(Clone, Default)]
pub struct Chats {
    /// The switch, `chats.on`: off, a line is the thread's and nothing here runs.
    pub on: bool,
    /// `chats.on` is true in the settings, though this start has no screen to show the chats on (a run or a pipe): `:chats` says so.
    pub set: bool,
    /// This start can use the chats: it has a screen, or the word at the start was `orchestrate` (a run or a pipe then works them without one). Under any
    /// other word a start with no screen has one thread and no chats.
    pub screen: bool,
    /// What the start wired for the chats (the Decider's point, the tool `tell`): `chats.on` or the word `orchestrate` at the start.
    pub wired: bool,
    /// The floors of a message (the settings `chats.message.*`): 0 is the core's own floor, and a number above it is the floor.
    pub chars: usize,
    pub hops: u32,
    pub rate: u32,
    /// The archive (`chats.archive.*`).
    pub archive: super::archive::Archive,
    /// The doers the tool `tell` is offered to: set by the start, which wired the post, and not a setting.
    pub tells: Vec<String>,
}

/// The id of the option that opens a new chat.
pub const NEW: &str = "new";
/// What the Log and the screen say when no Decider can be asked: the word of what it is, not the name of the pack.
pub const DECIDER_OFF: &str = "Decider off";
/// The longest title.
const TITLE: usize = 80;
/// The doer that runs a project's own check: its task is no turn of a chat.
const CHECK: &str = "check";
/// How much of an answer a chat's end carries into the orchestrator's thread: the screen cuts it to a preview.
const END_TEXT: usize = 2000;
/// How long a chat that is done stays in the list before the screen folds it: half an hour.
pub(super) const FOLD_AFTER_MS: u64 = 30 * 60_000;
/// A chat a line matches, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub chat: String,
    /// `same files` or `same subject`.
    pub why: &'static str,
    /// What matched: the file, or the words.
    pub detail: String,
}

/// Why a line went where it went: what the route row says, who decided, and what matched.
struct Reason<'a> {
    why: &'a str,
    by: &'a str,
    detail: &'a str,
}

/// The line that is being routed: the thread it was typed in, its words and the rows kept beside it.
struct Typed<'a> {
    thread: &'a str,
    text: &'a str,
    keys: &'a [String],
}

/// What Jev said of a line, when it was asked.
struct Jev {
    pick: Option<String>,
    by: String,
    why: String,
    /// No Decider answered at all: the pack is off, or its key is not there.
    off: bool,
}

/// The question the operator is asked which chat takes a line, while it waits (`route:<n>`).
#[derive(Clone)]
pub(super) struct Pending {
    pub id: String,
    pub line: String,
    pub keys: Vec<String>,
    pub question: String,
    /// `(id, label, who)`: a chat and its `doer · model`, and `new`.
    pub options: Vec<(String, String, String)>,
    /// The chat the screen marks as recommended: Jev's pick, or the chat the line names.
    pub hint: Option<String>,
    /// Who made the hint: the Decider (`~`), or a rule (the line names the chat).
    pub hint_by: By,
    pub why: String,
}

/// The paths a text names: a word with a `/`, or with a dot and an extension that starts with a letter.
pub fn paths(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in text.split_whitespace() {
        let w = w.trim_matches(|c: char| !(c.is_alphanumeric() || matches!(c, '/' | '.' | '_' | '-')));
        let w = w.trim_end_matches('.');
        let ext = w.rsplit_once('.').map(|(_, e)| e).filter(|e| (1..=5).contains(&e.len()) && e.starts_with(|c: char| c.is_ascii_alphabetic()) && e.chars().all(|c| c.is_ascii_alphanumeric()));
        if w.len() > 2 && !w.contains("://") && (w.contains('/') || ext.is_some()) && !out.iter().any(|o| o == w) {
            out.push(w.to_string());
        }
    }
    out
}

/// Whether two paths are the one: equal, or one is the other's tail after a `/`.
fn same_file(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim_start_matches("./"), b.trim_start_matches("./"));
    a == b || a.ends_with(&format!("/{b}")) || b.ends_with(&format!("/{a}"))
}

/// The most other chats a doer is told of.
const CHATS_TOLD: usize = 8;

impl Front {
    /// The chats act: `chats.on` is set, or the word of `lanes` is `orchestrate` and a screen uses them. The word is read each time, so
    /// `:lanes single` and `:undo` end it at once and nothing of it is kept.
    pub fn chats_on(&self) -> bool {
        let c = self.0.cfg();
        c.chats.on || (c.chats.screen && self.lanes_now() == crate::branch::Switch::Orchestrate)
    }

    /// Main is a chat with a session of its own: the word is `orchestrate` and the chats act.
    pub fn main_works(&self) -> bool {
        self.chats_on() && self.lanes_now() == crate::branch::Switch::Orchestrate
    }

    /// What a doer is told about the chats: that it is chat `thread` of J4n in this project and which the others are, on the first line of its
    /// session and on a line that asks about chats. Only this project's chats, by id and title, at most [`CHATS_TOLD`]; none when the thread is not a chat
    /// (the switch is off). Without it a model has no idea what `c3` is and goes looking in the program that runs it.
    pub fn chats_line(&self, thread: &str, line: &str, in_session: bool) -> Option<String> {
        if in_session && !crate::brief::about_chats(line) {
            return None;
        }
        let me = self.chat(thread)?;
        let others: Vec<String> = self
            .chats()
            .into_iter()
            .filter(|c| c.parent.is_none() && c.archived_ms.is_none() && c.id != me.id)
            .take(CHATS_TOLD)
            .map(|c| format!("{} \"{}\"", c.id, crate::text::line(&c.title, 40)))
            .collect();
        let rest = if others.is_empty() { "There is no other chat yet.".to_string() } else { format!("The other chats here: {}.", others.join("; ")) };
        Some(format!("You are chat {} of J4n in this project: J4n keeps one conversation per chat, each with its own doer and session. {rest} A chat means one of these; the sessions of the program that runs you are not J4n's chats.", me.id))
    }

    /// The thread the screen opens on and the orchestrator's: it has no doer when the switch is on.
    pub fn top(&self) -> String {
        self.0.cfg().thread.clone()
    }

    /// The chats, a top-level chat and then its branches.
    pub fn chats(&self) -> Vec<ChatRow> {
        self.0.binds.chats()
    }

    pub fn chat(&self, id: &str) -> Option<ChatRow> {
        self.0.binds.chat(id)
    }

    /// The thread whose events and tasks a chat shows: its own, and for a branch its parent's.
    pub fn chat_thread(&self, id: &str) -> Option<String> {
        let c = self.0.binds.chat(id)?;
        Some(c.parent.unwrap_or(c.id))
    }

    /// Open a chat: `parent` for a branch (`task`, the plan `item`), none for a top-level chat. The id is the next one; the title is cut from `line`.
    pub(super) fn open_chat(&self, parent: Option<&str>, line: &str, task: &str, item: &str) -> String {
        let _one = self.0.chat_lock.lock().unwrap_or_else(|e| e.into_inner());
        let id = self.0.binds.next_chat(parent);
        self.write_open(&id, parent, line, task, item);
        id
    }

    fn write_open(&self, id: &str, parent: Option<&str>, line: &str, task: &str, item: &str) {
        let title = crate::text::line(line.trim(), TITLE);
        self.write(id, "chat.opened", Actor::Core, json!({ "parent": parent.unwrap_or_default(), "title": title, "task": task, "item": item }));
    }

    /// A plan's branch is a chat under the chat that started it: the same chat again for an item that is sent back.
    pub(super) fn chat_for_branch(&self, thread: &str, task: &str, item: &str, goal: &str) {
        if !self.chats_on() || self.0.binds.chat(thread).is_none_or(|c| c.parent.is_some()) {
            return;
        }
        let first = goal.lines().next().unwrap_or_default();
        let _one = self.0.chat_lock.lock().unwrap_or_else(|e| e.into_inner());
        let known = self.0.binds.chats().into_iter().find(|c| c.parent.as_deref() == Some(thread) && !item.is_empty() && c.item == item);
        let id = known.map_or_else(|| self.0.binds.next_chat(Some(thread)), |c| c.id);
        self.write_open(&id, Some(thread), first, task, item);
    }

    // ---- what a line matches ----

    /// The files a chat has to do with: the ones its lines name, and the ones its tasks changed.
    fn chat_files(&self, chat: &ChatRow) -> BTreeSet<String> {
        let mut files: BTreeSet<String> = chat.lines.iter().flat_map(|l| paths(l)).collect();
        for r in self.0.tasks.book().rows().into_iter().filter(|r| r.spec.thread.as_deref() == Some(chat.id.as_str())) {
            files.extend(r.changed.created.into_iter().chain(r.changed.modified).chain(r.changed.deleted));
        }
        files
    }

    /// The chats a line matches by code: a file in common (`same files`). Newest first. No word of the line is matched.
    pub fn hits(&self, text: &str, chats: &[ChatRow]) -> Vec<Hit> {
        let named = paths(text);
        let mut files: Vec<Hit> = Vec::new();
        for c in chats {
            let have = self.chat_files(c);
            if let Some(f) = named.iter().find(|n| have.iter().any(|h| same_file(n, h))) {
                files.push(Hit { chat: c.id.clone(), why: "same files", detail: f.rsplit('/').next().unwrap_or(f).to_string() });
            }
        }
        files.sort_by_key(|h| std::cmp::Reverse(self.chat(&h.chat).map_or(0, |c| c.touched_ms)));
        files
    }

    /// The doer and model of a chat as it is shown, `doer · model`: the model that served its last task under its binding, else the pin.
    pub(super) fn chat_who(&self, thread: &str) -> String {
        let Some(b) = self.binding(thread) else { return String::new() };
        let last = self.last_task(thread).filter(|r| r.spec.doer == b.doer);
        let served = last.as_ref().filter(|r| r.spec.tune.model == b.model).map(|r| r.model.clone()).unwrap_or_default();
        let who = crate::task::who(&b.doer, &served, b.model.as_deref().unwrap_or_default());
        // the login the chat runs as: the pin, else where its last turn ran
        match b.account.clone().or_else(|| last.and_then(|r| r.spec.account)).filter(|a| !a.is_empty()) {
            Some(a) => format!("{who} · {a}"),
            None => who,
        }
    }

    /// A chat whose last turn cooled or failed: a line sent there is a problem, said before it is sent.
    pub(super) fn chat_problem(&self, id: &str) -> Option<String> {
        let last = self.last_task(id)?;
        match last.state {
            State::Cooling { .. } => Some(format!("{id} is cooling: its host is at a limit")),
            State::Done { ok: false } | State::Stopped { why: crate::task::Why::Failed { .. } } => Some(format!("{id}'s last turn failed")),
            _ => None,
        }
    }

    /// Ask Jev which chat a line belongs to, at the ask point `chats`, among `chats` and a new one. `pick` is none when Jev did not choose: it is not
    /// there, has not passed its cases, or passed.
    fn route_jev(&self, thread: &str, text: &str, chats: &[&ChatRow]) -> Jev {
        let mut options: Vec<Opt> = chats.iter().map(|c| Opt { id: c.id.clone(), label: format!("{}: {}", c.id, c.title), recommended: false }).collect();
        options.push(Opt { id: NEW.into(), label: "a new chat: none of these".into(), recommended: true });
        let q = self.0.who.question("chats", Some(thread), text, options);
        let off = !self.decider_on();
        match self.0.who.pick("chats", Some(thread), &q) {
            None => Jev { pick: None, by: "front".into(), why: DECIDER_OFF.into(), off: true },
            Some((_, by, why, _)) if by.starts_with("fallback") => Jev { pick: None, by, why: if off { DECIDER_OFF.into() } else { format!("Decider unsure: {why}") }, off },
            Some((pick, by, why, _)) => Jev { pick: Some(pick), by, why, off: false },
        }
    }

    /// Whether a Decider can be asked now: the ask point `chats` has a decider or a model in its chain that is installed and has its key.
    /// The status bar draws the icon from it, and the Log says `Decider off` when it is not.
    pub fn decider_on(&self) -> bool {
        let asks = self.asks();
        asks.points.get("chats").is_some_and(|p| p.chain.iter().any(|s| s.kind.is_model() && asks.registry.get(&s.id()).is_some()))
    }

    /// The chat a line names by its id (`c1`, `c2`): the first of `chats` that has the word. Only a hint: the line is never sent by it.
    fn named_chat(text: &str, chats: &[ChatRow]) -> Option<String> {
        text.split(|c: char| !c.is_alphanumeric()).find_map(|w| {
            let w = w.to_ascii_lowercase();
            chats.iter().find(|c| c.id == w).map(|c| c.id.clone())
        })
    }

    // ---- routing ----

    /// A line typed in the orchestrator goes to the chat it matches, or opens one. `None`: the line is not routed (the switch is off, or this
    /// is a chat's own thread).
    pub(super) fn route_line(&self, thread: &str, text: &str, keys: &[String], read: Option<&super::read::Read>) -> Option<Said> {
        if !self.chats_on() || thread != self.0.cfg().thread {
            return None;
        }
        // a line while a question waits is the next line (an answer was taken in `say_with`): the question is dropped, and said
        let held = self.0.routes.lock().unwrap_or_else(|e| e.into_inner()).remove(thread);
        if let Some(p) = held {
            self.route_dropped(thread, (&p.id, &p.line), "a new line came", "the question of which chat takes the last line is dropped: it was not answered");
        }
        // an archived chat is not offered a line
        let all: Vec<ChatRow> = self.0.binds.chats().into_iter().filter(|c| c.parent.is_none() && c.archived_ms.is_none()).collect();
        // Main works its own line: only what the read found for it takes it to a chat, and nothing here matches a word
        if self.main_works() {
            return self.route_read(thread, text, &all, read?);
        }
        let hits = self.hits(text, &all);
        let (to, why, by, detail) = match hits.as_slice() {
            [] if all.is_empty() => (None, "new chat · none matched".into(), "code".into(), String::new()),
            [] => {
                let jev = self.route_jev(thread, text, &all.iter().collect::<Vec<_>>());
                match jev.pick.as_deref() {
                    Some(NEW) => (None, "new chat · none matched".into(), jev.by, jev.why),
                    Some(p) if all.iter().any(|c| c.id == p) => (Some(p.to_string()), "same subject".into(), jev.by, jev.why),
                    // Jev is off or not sure: the operator is asked, a new chat is not opened behind their back
                    _ => {
                        let every: Vec<Hit> = all.iter().map(|c| Hit { chat: c.id.clone(), why: "none matched", detail: String::new() }).collect();
                        let named = Self::named_chat(text, &all);
                        let why = if jev.off { DECIDER_OFF.to_string() } else { "Decider not sure".to_string() };
                        return Some(self.route_ask(&Typed { thread, text, keys }, "which chat takes this?", &every, named.map(|n| (n, By::Rule)), &why));
                    }
                }
            }
            [one] => match self.chat_problem(&one.chat) {
                Some(problem) => {
                    return Some(self.route_ask(&Typed { thread, text, keys }, &format!("{problem}: send the line there anyway, or open a new chat?"), std::slice::from_ref(one), None, "one chat matches and it has a problem"));
                }
                None => (Some(one.chat.clone()), one.why.to_string(), "code".into(), one.detail.clone()),
            },
            many => {
                let chats: Vec<&ChatRow> = many.iter().filter_map(|h| all.iter().find(|c| c.id == h.chat)).collect();
                let hint = self.route_jev(thread, text, &chats).pick.map(|p| (p, By::Model));
                return Some(self.route_ask(&Typed { thread, text, keys }, "which chat takes this?", many, hint, &format!("{} chats match", many.len())));
            }
        };
        let (line, reason) = (Typed { thread, text, keys }, Reason { why: &why, by: &by, detail: &detail });
        Some(match to {
            Some(chat) => self.send_to(&line, &chat, &reason),
            None => self.send_new(&line, &reason),
        })
    }

    /// A line Main would work, read: about a chat (the read's `read:chat`) it goes there; going on from what a chat said it goes to the chat whose doer spoke last.
    fn route_read(&self, thread: &str, text: &str, all: &[ChatRow], read: &super::read::Read) -> Option<Said> {
        let (to, why, by) = match (&read.chat, &read.goes_on) {
            (Some(c), _) if c.value.as_ref().is_some_and(|id| all.iter().any(|x| &x.id == id)) => {
                let id = c.value.clone().unwrap_or_default();
                (id, "the read: about this chat".to_string(), c.by.clone())
            }
            (_, Some(g)) if g.value => {
                let id = self.spoke_last(all)?;
                (id.clone(), format!("goes on from {id}'s last answer"), g.by.clone())
            }
            _ => return None,
        };
        let line = Typed { thread, text, keys: &[] };
        if let Some(problem) = self.chat_problem(&to) {
            let hit = Hit { chat: to.clone(), why: "the read", detail: by };
            return Some(self.route_ask(&line, &format!("{problem}: send the line there anyway, or open a new chat?"), std::slice::from_ref(&hit), None, "the chat the line is about has a problem"));
        }
        Some(self.send_to(&line, &to, &Reason { why: &why, by: &by, detail: "" }))
    }

    /// The chat whose doer spoke last (a turn of it ended or began), not the one you typed in last: an empty chat, or one you only typed to, has said
    /// nothing a reply could go on from. The newer chat breaks a tie.
    fn spoke_last(&self, chats: &[ChatRow]) -> Option<String> {
        let rows = self.0.tasks.book().rows();
        chats
            .iter()
            .filter_map(|c| {
                let at = rows.iter().filter(|r| r.spec.thread.as_deref() == Some(c.id.as_str()) && r.spec.doer != HUMAN).map(|r| r.ended_ms.or(r.started_ms).unwrap_or(r.queued_ms)).max()?;
                Some((at, c.id.trim_start_matches('c').parse::<u64>().unwrap_or(0), c.id.clone()))
            })
            .max()
            .map(|(_, _, id)| id)
    }

    /// The line goes into the chat `chat`; the route row follows the send.
    fn send_to(&self, line: &Typed, chat: &str, r: &Reason) -> Said {
        let said = self.say_with(chat, line.text, line.keys);
        self.route_said(line, chat, r, false, &said);
        said
    }

    /// The line opens a chat, and goes into it.
    fn send_new(&self, line: &Typed, r: &Reason) -> Said {
        let chat = self.open_chat(None, line.text, "", "");
        let said = self.say_with(&chat, line.text, line.keys);
        self.route_said(line, &chat, r, true, &said);
        said
    }

    /// The Log's `chat.routed` and the route row, in the thread the line was typed in. A chat that refused the line (its task waits for an
    /// answer, it has no doer) did not take it: the row and the Log say so, so the line is not taken for sent.
    fn route_said(&self, line: &Typed, chat: &str, r: &Reason, new: bool, said: &Said) {
        let Reason { why, by, detail } = r;
        let refused = if let Said::Refused(why) = said { Some(why.as_str()) } else { None };
        self.write(line.thread, "chat.routed", Actor::Core, json!({ "line": crate::text::clip(line.text, CHAT_LINE), "to": chat, "why": why, "by": by, "detail": detail, "new": new, "refused": refused }));
        let shown = if new || detail.is_empty() || *why == "same subject" && *by != "code" { String::new() } else { format!(" · {detail}") };
        let row = format!("→ {chat} · {why}{shown}");
        // the Decider is off: the row leads to the page that turns it on
        if *detail == DECIDER_OFF {
            let lead = super::Lead::open("open the deciders", "deciders");
            self.note_to(line.thread, &if row.ends_with(DECIDER_OFF) { format!("{row} · ⏎ opens Connect › Deciders") } else { format!("{row} · {DECIDER_OFF} · ⏎ opens Connect › Deciders") }, "dim", lead);
        } else {
            self.note(line.thread, &row, "dim");
        }
        if let Some(why) = refused {
            self.note(line.thread, &format!("not sent to {chat}: {why}"), "warn");
        }
    }

    // ---- the question ----

    /// Ask which chat takes a line: one question with an option for each matching chat and a new one, held until it is answered.
    fn route_ask(&self, line: &Typed, question: &str, hits: &[Hit], hint: Option<(String, By)>, why: &str) -> Said {
        let Typed { thread, text, keys } = *line;
        let n = self.0.route_n.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        let mut options: Vec<(String, String, String)> = hits
            .iter()
            .map(|h| {
                let c = self.chat(&h.chat);
                (h.chat.clone(), format!("{} · {}", h.chat, c.as_ref().map_or("", |c| c.title.as_str())), self.chat_who(&h.chat))
            })
            .collect();
        options.push((NEW.into(), "new chat".into(), String::new()));
        let pending = Pending { id: format!("route:{n}"), line: text.to_string(), keys: keys.to_vec(), question: question.to_string(), options, hint_by: hint.as_ref().map_or(By::Operator, |h| h.1), hint: hint.map(|h| h.0), why: why.to_string() };
        let list: Vec<&str> = pending.options.iter().map(|o| o.0.as_str()).collect();
        // held before it is written: a screen that starts between the two does not find an open question that nothing holds
        self.0.routes.lock().unwrap_or_else(|e| e.into_inner()).insert(thread.to_string(), pending.clone());
        self.write(thread, "chat.route.asked", Actor::Core, json!({ "ask": pending.id, "line": crate::text::clip(text, CHAT_LINE), "options": list, "hint": pending.hint, "why": why }));
        let said = format!("{question} {}", pending.options.iter().map(|o| o.1.as_str()).collect::<Vec<_>>().join(" · "));
        // the Decider is off: the note leads to the page that turns it on
        if why == DECIDER_OFF {
            self.note_to(thread, &format!("{said} · {DECIDER_OFF} · ⏎ opens Connect › Deciders"), "warn", super::Lead::open("open the deciders", "deciders"));
        } else {
            self.note(thread, &said, "warn");
        }
        Said::Asking { ask: pending.id.clone() }
    }

    /// The questions of which chat takes a line, as the operator is shown them (`Down::Asks`): an option for each chat, its `doer · model` at the right,
    /// and Jev's pick marked as the model's.
    pub fn route_asks(&self) -> Vec<AskView> {
        let route: Vec<AskView> = self
            .0
            .routes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .map(|p| AskView {
                ask: p.id.clone(),
                asker: "j4n".into(),
                question: p.question.clone(),
                shape: Shape::Choice,
                options: p.options.iter().map(|(id, label, who)| Choice { id: id.clone(), label: label.clone(), recommended: p.hint.as_deref() == Some(id), by: if p.hint.as_deref() == Some(id) { p.hint_by } else { By::Operator }, note: who.clone(), ..Choice::default() }).collect(),
                facts: vec![Cell::new(crate::text::line(&p.line, 60), Tone::Dim)],
                why: p.why.clone(),
                ..AskView::default()
            })
            .collect();
        route.into_iter().chain(self.mail_asks()).collect()
    }

    /// The line answers the question that waits in `thread`: a number, the id of a chat, or `new`.
    pub(super) fn route_picked(&self, thread: &str, text: &str) -> Option<Said> {
        let id = {
            let routes = self.0.routes.lock().unwrap_or_else(|e| e.into_inner());
            let p = routes.get(thread)?;
            let text = text.trim();
            match text.parse::<usize>() {
                Ok(n) => p.options.get(n.wrapping_sub(1)).map(|o| o.0.clone()),
                Err(_) => p.options.iter().find(|o| o.0.eq_ignore_ascii_case(text)).map(|o| o.0.clone()),
            }?
        };
        Some(self.route_answer(thread, &id))
    }

    /// Answer the question of `thread` with the option `id`: the line goes there, or opens a chat.
    pub(super) fn route_answer(&self, thread: &str, id: &str) -> Said {
        let Some(p) = self.0.routes.lock().unwrap_or_else(|e| e.into_inner()).remove(thread) else { return self.refuse(thread, "no question waits for an answer".into()) };
        let line = Typed { thread, text: &p.line, keys: &p.keys };
        // what Jev could not say stays in the Log with the answer: `Decider off`
        let detail = if p.why.starts_with("Decider") { p.why.as_str() } else { "" };
        if id == NEW {
            let why = if p.why == DECIDER_OFF { format!("new chat · {DECIDER_OFF}") } else { "new chat · you chose it".to_string() };
            return self.send_new(&line, &Reason { why: &why, by: "you", detail });
        }
        if self.chat(id).is_none() {
            self.0.routes.lock().unwrap_or_else(|e| e.into_inner()).insert(thread.to_string(), p);
            return self.refuse(thread, format!("there is no chat `{id}`"));
        }
        self.send_to(&line, id, &Reason { why: "you chose it", by: "you", detail })
    }

    /// A question of which chat takes a line ended with no answer: the Log says so (`chat.route.dropped`) and the row says why.
    fn route_dropped(&self, thread: &str, (ask, line): (&str, &str), why: &str, said: &str) {
        self.write(thread, "chat.route.dropped", Actor::Core, json!({ "ask": ask, "line": crate::text::clip(line, CHAT_LINE), "why": why }));
        self.note(thread, said, "dim");
    }

    /// A question the Log opened and nothing closed was held in memory and is lost with the run: it is dropped and said with its line, so it can be
    /// typed again. Called when a screen starts; one held here is not lost.
    pub fn route_resume(&self) {
        for (thread, ask, line) in self.0.binds.route_open().into_iter().filter(|_| self.chats_on()) {
            if !self.0.routes.lock().unwrap_or_else(|e| e.into_inner()).contains_key(&thread) {
                self.route_dropped(&thread, (&ask, &line), "lost with a restart", &format!("the question of which chat takes \"{}\" was lost with the restart: type the line again", crate::text::line(&line, 60)));
            }
        }
    }

    /// The answer of the operator from a button or a key, to a question of this module (`route:<n>`): `None` when `ask` is not one.
    pub(super) fn route_reply(&self, ask: &str, reply: &j4n_proto::Reply) -> Option<Said> {
        if !ask.starts_with("route:") {
            return None;
        }
        let thread = self.0.routes.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(_, p)| p.id == ask).map(|(t, _)| t.clone());
        let Some(thread) = thread else { return Some(Said::Refused("that question is not open any more".into())) };
        Some(match reply {
            j4n_proto::Reply::Pick { id } => self.route_answer(&thread, id),
            j4n_proto::Reply::No => {
                let held = self.0.routes.lock().unwrap_or_else(|e| e.into_inner()).remove(&thread);
                if let Some(p) = held {
                    self.route_dropped(&thread, (&p.id, &p.line), "you said no", "the line was not sent: you said no to every chat");
                }
                Said::Nothing
            }
            _ => self.refuse(&thread, "answer with one of the options".into()),
        })
    }

    // ---- the words ----

    /// `:new`: a new chat, and `:new <words>` a new chat that is sent the words, the operator staying where they are.
    pub(super) fn new_said(&self, thread: &str, args: &[String]) -> Said {
        let words = args.join(" ");
        if words.trim().is_empty() {
            let id = self.open_chat(None, "", "", "");
            return Said::Enter(id);
        }
        let id = self.open_chat(None, &words, "", "");
        let said = self.say_with(&id, words.trim(), &[]);
        self.write(thread, "chat.routed", Actor::Core, json!({ "line": crate::text::clip(&words, CHAT_LINE), "to": id, "why": "you asked for it", "by": "you", "detail": "", "new": true }));
        self.note(thread, &format!("→ {id} new chat · you asked for it"), "dim");
        said
    }

    /// `:fresh` in a chat starts its session again; in the orchestrator, which has none (and Main has one under `orchestrate`), it says so.
    pub(super) fn fresh_chat(&self, thread: &str) -> bool {
        if !self.chats_on() {
            return false;
        }
        if thread == self.0.cfg().thread && !self.main_works() {
            self.note(thread, "the orchestrator has no session of its own · :fresh is for a chat", "dim");
            return true;
        }
        self.write(thread, "thread.fresh", super::reflexes::doing(), json!({}));
        self.forget_moves(thread);
        self.note(thread, "new session by :fresh · the next line starts cold", "dim");
        true
    }

    /// `:clear`: what the chat shows is emptied and the Log keeps it (it is a chain); `:history` brings the rows back.
    pub(super) fn clear_said(&self, thread: &str) -> Said {
        let said = Said::Command("clear".into());
        let events: Vec<crate::log::Ev> = self.0.log.read(0).filter(|e| e.thread.as_deref() == Some(thread)).collect();
        let cut = cleared_before(&events);
        // the lines that said `:clear` and what an earlier one said are not rows to clear: twice in a row, the second finds nothing
        let is_clear = |e: &crate::log::Ev| match e.kind.as_str() {
            "chat.say" => e.data.get("text").and_then(Json::as_str).and_then(|t| self.0.phrases.parse(t)).is_some_and(|c| c.name == "clear"),
            _ => e.data.get("clear").is_some_and(Json::is_boolean),
        };
        let tagged: Vec<crate::log::Ev> = events.iter().filter(|e| e.seq > cut && !is_clear(e)).map(|e| tagged(e, thread)).collect();
        let shown = rows::fold_with(self.screen().show(), tagged.iter()).rows().len();
        let said_it = |text: String| self.write(thread, "chat.note", Actor::Core, json!({ "text": text, "tone": "dim", "clear": true }));
        if shown == 0 {
            said_it("nothing to clear".into());
            return said;
        }
        self.write(thread, "chat.cleared", Actor::Operator, json!({ "rows": shown }));
        // `⏎` brings them back: the note's act is `:history`
        let back = j4n_proto::Act { key: "enter".into(), label: "bring them back".into(), intent: j4n_proto::Intent::Run { command: "history".into(), args: Vec::new() }, risk: 0 };
        let text = format!("cleared {shown} row{} · the Log keeps them · ⏎ brings them back (`:history`)", if shown == 1 { "" } else { "s" });
        self.write(thread, "chat.note", Actor::Core, json!({ "text": text, "tone": "dim", "clear": true, "act": back }));
        said
    }

    pub(super) fn history_said(&self, thread: &str) -> Said {
        let events: Vec<crate::log::Ev> = self.0.log.read(0).filter(|e| e.thread.as_deref() == Some(thread)).collect();
        if cleared_before(&events) == 0 {
            self.note(thread, "nothing is cleared here", "dim");
        } else {
            self.write(thread, "chat.history", Actor::Operator, json!({}));
        }
        Said::Command("history".into())
    }

    /// `:chats`: the list, in words (the screen opens it as a popup).
    pub(super) fn chats_said(&self) -> Said {
        self.mail_resume(None);
        Said::Chats
    }

    // ---- what the screen is sent ----

    /// What a chat is doing, from its newest turn: the mark of the lane it would be, the word and the tone.
    fn chat_state(&self, rows: &[Row]) -> (&'static str, &'static str, Tone) {
        let newest = rows.iter().rev().find(|r| !r.state.is_terminal()).or(rows.last());
        match newest.map(|r| &r.state) {
            None => ("", "new", Tone::Dim),
            Some(State::Waiting { .. }) => ("ask", "waits", Tone::Warn),
            Some(State::Running) => ("working", "running", Tone::Accent),
            Some(State::Cooling { .. }) => ("cooling", "cools", Tone::Dim),
            Some(State::Queued) => ("queued", "queued", Tone::Dim),
            Some(State::Done { ok: true }) => ("done", "done", Tone::Good),
            Some(State::Done { ok: false }) => ("failed", "failed", Tone::Bad),
            Some(State::Stopped { .. }) => ("stopped", "stopped", Tone::Dim),
        }
    }

    /// The tasks a chat shows, of `all` the tasks there are: its thread's turns, or for a branch the branch task.
    pub(super) fn chat_rows_of(&self, c: &ChatRow, all: &[Row]) -> Vec<Row> {
        match &c.task {
            Some(t) => all.iter().filter(|r| r.id == *t).cloned().collect(),
            None => all.iter().filter(|r| r.spec.thread.as_deref() == Some(c.id.as_str()) && r.spec.doer != HUMAN && r.spec.parent.is_none()).cloned().collect(),
        }
    }

    /// How full the context of the chat's session is, from its newest turn of the session.
    fn chat_window(&self, id: &str) -> Option<f64> {
        let b = self.binding(id)?;
        let own = self.own_tasks(id, &b.doer);
        let session = own.iter().find_map(|r| r.session.clone())?;
        own.iter().filter(|r| r.session.as_ref() == Some(&session)).find_map(|r| r.window)
    }

    /// The pinned list: a row for each chat. The ones that wait come first, then the ones that run, then the newest; a branch is
    /// nested under its chat.
    pub fn chat_list(&self) -> Vec<PRow> {
        let now = self.0.log.clock().now_ms();
        let asks = self.0.tasks.book().open_asks();
        let (chats, all) = (self.0.binds.chats(), self.0.tasks.book().rows());
        let mut tops: Vec<(u8, &ChatRow)> = chats.iter().filter(|c| c.parent.is_none()).map(|c| (if c.archived_ms.is_some() { 3 } else { self.rank(c, &chats, &all) }, c)).collect();
        tops.sort_by_key(|(rank, c)| (*rank, std::cmp::Reverse(c.touched_ms), std::cmp::Reverse(c.id.trim_start_matches('c').parse::<u64>().unwrap_or(0))));
        let mut out = Vec::new();
        for (_, c) in tops {
            let at = out.len();
            out.push(self.chat_row(c, now, (&asks, &all)));
            out.extend(chats.iter().filter(|b| b.parent.as_deref() == Some(c.id.as_str())).map(|b| self.chat_row(b, now, (&asks, &all))));
            // an archived chat and its branches are the screen's `archived (N)` row
            if c.archived_ms.is_some() {
                out[at..].iter_mut().filter_map(|r| r.data.as_mut()).for_each(|d| d["archived"] = Json::Bool(true));
                continue;
            }
            // a chat that is done, and all of its branches, for a while: the screen folds them into `N done before`
            let over = out[at..].iter().all(|r| r.data.as_ref().is_some_and(|d| d["state"] == "done" && now.saturating_sub(d["ended_ms"].as_u64().unwrap_or(now)) >= FOLD_AFTER_MS));
            if over {
                out[at..].iter_mut().filter_map(|r| r.data.as_mut()).for_each(|d| d["old"] = Json::Bool(true));
            }
        }
        out
    }

    /// `0` a chat that waits for you, `1` one that works, `2` the others.
    fn rank(&self, c: &ChatRow, chats: &[ChatRow], all: &[Row]) -> u8 {
        let rows = self.chat_rows_of(c, all);
        let branches = chats.iter().filter(|b| b.parent.as_deref() == Some(c.id.as_str())).flat_map(|b| self.chat_rows_of(b, all)).collect::<Vec<_>>();
        let all = rows.iter().chain(branches.iter());
        if all.clone().any(|r| matches!(r.state, State::Waiting { .. })) {
            0
        } else if all.clone().any(|r| matches!(r.state, State::Running | State::Cooling { .. } | State::Queued)) {
            1
        } else {
            2
        }
    }

    fn chat_row(&self, c: &ChatRow, now: u64, (asks, all): (&[crate::binder::OpenAsk], &[Row])) -> PRow {
        let rows = self.chat_rows_of(c, all);
        let (icon, word, tone) = self.chat_state(&rows);
        let thread = c.parent.clone().unwrap_or_else(|| c.id.clone());
        let who = match &c.task {
            Some(t) => rows.first().filter(|r| r.id == *t).map(Row::who).unwrap_or_default(),
            None => self.chat_who(&thread),
        };
        let window = if c.task.is_some() { rows.first().and_then(|r| r.window) } else { self.chat_window(&c.id) };
        let micros: i64 = rows.iter().map(|r| r.spent).sum();
        let last = rows.iter().rev().find(|r| !r.state.is_terminal()).or(rows.last());
        // a chat that is over says when: the clock text here, and `data.at_ms` for the screen to say how long ago
        let mut at_ms = None;
        let right = match last.map(|r| (&r.state, r)) {
            None => "new".to_string(),
            Some((State::Running, r)) => rows::clock(now.saturating_sub(r.started_ms.unwrap_or(r.queued_ms))),
            Some((State::Waiting { ask }, _)) => format!("waits {}", asks.iter().find(|a| a.ask == *ask).map_or_else(String::new, |a| askview::wait(now.saturating_sub(a.opened_ms)))).trim_end().to_string(),
            Some((State::Cooling { until_ms }, _)) => format!("cools {}", askview::wait(until_ms.saturating_sub(now))),
            Some((State::Queued, _)) => "queued".to_string(),
            Some((_, r)) => {
                let at = r.ended_ms.unwrap_or(r.queued_ms);
                at_ms = Some(at);
                rows::time_of_day(at, None)
            }
        };
        let title = if c.title.is_empty() { "no line yet".to_string() } else { c.title.clone() };
        let cost = if micros > 0 { rows::money(micros as f64 / 1e6) } else { String::new() };
        // the seat comes before the doer, while the crew is on
        let seat = c.seat.clone().filter(|_| self.0.cfg().crew.on);
        let seated = match (&seat, who.is_empty()) {
            (Some(s), false) => format!("{s} · {who}"),
            (Some(s), true) => s.clone(),
            (None, _) => who.clone(),
        };
        let cells = vec![
            Cell::with_icon(icon, "", tone),
            Cell::new(&c.id, Tone::Dim),
            Cell::new(title, if c.title.is_empty() { Tone::Dim } else { Tone::Plain }),
            Cell::new(seated, Tone::Dim),
            Cell::new(window.map(|w| format!("{:.0}%", w * 100.0)).unwrap_or_default(), if window.is_some_and(|w| w >= 0.8) { Tone::Warn } else { Tone::Dim }),
            Cell::new(cost, Tone::Dim),
            Cell::new(right, if word == "waits" { Tone::Warn } else { Tone::Dim }),
        ];
        let ended = rows.iter().filter_map(|r| r.ended_ms).max();
        let data = json!({ "chat": c.id, "state": word, "branch": c.task.is_some(), "doer": who, "seat": seat, "window": window, "micros": micros, "ended_ms": ended, "at_ms": at_ms });
        PRow { key: c.id.clone(), cells, tone, parent: c.parent.clone(), data: Some(data), ..PRow::default() }
    }

    /// Where the screen is, for `chat:here`: the crumb from the first level down, the state of the chat and the counts of the orchestrator.
    pub fn chat_here(&self, here: &str) -> Json {
        let chats = self.0.binds.chats();
        let rows = self.chat_list();
        let count = |w: &str| rows.iter().filter(|r| r.data.as_ref().is_some_and(|d| d["state"] == w)).count();
        let mut crumb: Vec<Json> = Vec::new();
        let mut at = self.chat(here);
        while let Some(c) = at {
            crumb.push(json!({ "id": c.id, "title": c.title }));
            at = c.parent.as_deref().and_then(|p| self.chat(p));
        }
        crumb.reverse();
        let state = rows.iter().find(|r| r.key == here).and_then(|r| r.data.as_ref().map(|d| (d["state"].clone(), r.cells[0].icon.clone())));
        json!({
            "top": here.is_empty() || here == self.0.cfg().thread,
            "id": here,
            "title": chats.iter().find(|c| c.id == here).map(|c| c.title.clone()).unwrap_or_default(),
            "crumb": crumb,
            "state": state.as_ref().map(|s| s.0.clone()).unwrap_or(Json::Null),
            "icon": state.map(|s| s.1).unwrap_or_default(),
            "chats": chats.iter().filter(|c| c.parent.is_none() && c.archived_ms.is_none()).count(),
            "running": count("running") + count("cools") + count("queued"),
            "waits": count("waits"),
            "windows": self.windows_row(),
            "today": self.today_words(),
            "form": self.gauge_form(),
        })
    }

    /// The windows of the providers in use, for the orchestrator's row: the fullest reading of each doer's window, with when it resets.
    fn windows_row(&self) -> Vec<Json> {
        let mut all: Vec<crate::cost::WindowNow> = Vec::new();
        for w in self.0.tasks.windows() {
            match all.iter_mut().find(|l| l.doer == w.doer && l.window == w.window) {
                Some(l) if l.used < w.used => *l = w,
                Some(_) => {}
                None => all.push(w),
            }
        }
        all.sort_by(|a, b| (&a.doer, &a.window).cmp(&(&b.doer, &b.window)));
        all.iter().map(|w| json!({ "doer": w.doer, "window": w.short(), "used": w.used, "resets": u64::try_from(w.resets).ok().map(|r| rows::time_of_day(r.saturating_mul(1000), None)) })).collect()
    }

    /// What today has cost (the machine's day, the tasks queued since its midnight), as the bar words it; the main chat's windows row says it too.
    pub fn today_words(&self) -> Option<String> {
        let now = self.0.log.clock().now_ms();
        let (now_s, off) = ((now / 1000) as i64, crate::clock::offset_at((now / 1000) as i64));
        let midnight = ((now_s + off).div_euclid(86_400) * 86_400 - off).max(0) as u64 * 1000;
        let all = self.0.tasks.book().rows();
        let today: Vec<&Row> = all.iter().filter(|r| r.spec.doer != HUMAN && r.queued_ms >= midnight).collect();
        if !today.iter().any(|r| r.spent + r.listed > 0) {
            return None;
        }
        let (spent, listed): (i64, i64) = (today.iter().map(|r| r.spent).sum(), today.iter().map(|r| r.listed).sum());
        let mut shares = crate::cost::Shares::default();
        today.iter().for_each(|r| shares.merge(&r.shares));
        Some(self.screen().show().money_of(shares.words().as_deref(), today.iter().any(|r| r.planned), &super::serve::usd(spent), None, &super::serve::usd(listed)))
    }

    /// How the gauge is drawn (`look.gauge`): `auto` (the screen picks the shape from the width it has), the thin line, or the one circle slice.
    pub fn gauge_form(&self) -> &'static str {
        match self.screen().set.pointer("/look/gauge") {
            crate::value::Value::Str(s) if s == "slice" => "slice",
            crate::value::Value::Str(s) if s == "line" => "line",
            _ => "auto",
        }
    }

    // ---- what the orchestrator is told of its chats ----

    /// A chat's newest turn that is over, and its plan as it stands, each said once in the orchestrator's thread: `chat.ended` (the chat and the first
    /// line of its answer) and `chat.plan` (the plan's count and the lanes of its items). The Log is what says "once": a restart finds it in `threads`.
    pub(super) fn orch_step(&self, thread: &str) {
        let top = self.0.cfg().thread.clone();
        if !self.chats_on() || thread == top || self.chat(thread).is_none_or(|c| c.parent.is_some()) {
            return;
        }
        let _one = self.0.chat_lock.lock().unwrap_or_else(|e| e.into_inner());
        let book = self.0.tasks.book();
        let mine = |r: &&Row| r.spec.thread.as_deref() == Some(thread) && r.spec.doer != HUMAN && r.spec.parent.is_none();
        let fresh = |r: &&Row| r.state.is_terminal() && r.ended_ms.is_some_and(|t| t >= self.0.since_ms);
        // every turn of the chat that ended since this front began and was not said (a turn that ended while a newer one runs is said too); what an older
        // Log holds is not said again or for the first time. A task of the doer `check` is no turn of the chat: it says its verdict below.
        for r in book.rows().iter().filter(mine).filter(|r| r.spec.doer != CHECK).filter(fresh) {
            let key = format!("end:{}", r.id);
            if !self.0.binds.said(&key) {
                let ok = matches!(r.state, State::Done { ok: true });
                let answer = r.answer.as_deref().or(r.reason.as_deref()).unwrap_or_default().trim();
                // a chat Main briefed answers with a report: its own words are the preview, the digest says the rest
                let said = serde_json::from_str::<serde_json::Value>(answer).ok().and_then(|j| j.get("answer").and_then(|a| a.as_str()).map(|a| a.trim().to_string())).filter(|a| !a.is_empty());
                let answer = said.as_deref().unwrap_or(answer);
                let (line, text) = (crate::text::line(answer, 120), crate::text::clip(answer, END_TEXT));
                let title = self.chat(thread).map(|c| c.title).unwrap_or_default();
                self.write(&top, "chat.ended", Actor::Core, json!({ "chat": thread, "task": r.id, "ok": ok, "line": line, "text": text, "title": title, "key": key, "ended_ms": r.ended_ms }));
            }
        }
        // a check that ran where the chat's turn worked: its word goes on the row of the turn it checked
        for r in book.rows().iter().filter(mine).filter(|r| r.spec.doer == CHECK).filter(fresh) {
            let key = format!("check:{}", r.id);
            if !self.0.binds.said(&key) {
                let said = r.answer.as_deref().or(r.reason.as_deref()).unwrap_or_default().trim();
                let verdict = match r.state {
                    State::Done { ok: true } if !said.contains("not run") => "passed",
                    State::Done { ok: false } => "failed",
                    _ => "not run",
                };
                let of = r.spec.goal.strip_prefix("check what ").and_then(|g| g.split_whitespace().next()).unwrap_or_default();
                self.write(&top, "chat.checked", Actor::Core, json!({ "chat": thread, "task": r.id, "of": of, "verdict": verdict, "line": crate::text::line(said, 120), "key": key, "ended_ms": r.ended_ms }));
            }
        }
        // a plan of a chat that nothing touched since this front began is an old one
        if self.chat(thread).is_none_or(|c| c.touched_ms < self.0.since_ms) {
            return;
        }
        let Some(plan) = self.0.keeper.as_ref().and_then(|k| k.plan(thread)).filter(|p| !p.items.is_empty()) else { return };
        let (done, total) = (plan.items.iter().filter(|i| i.done()).count(), plan.items.len());
        let key = format!("plan:{thread}:{done}/{total}");
        if !self.0.binds.said(&key) {
            let chats = self.0.binds.chats();
            let lanes: Vec<Json> = plan
                .items
                .iter()
                .map(|i| {
                    let label = chats.iter().find(|c| c.parent.as_deref() == Some(thread) && c.item == i.id).map_or_else(|| i.id.clone(), |c| c.id.clone());
                    json!({ "label": label, "doer": book.get(&i.task).map(|r| r.spec.doer).unwrap_or_default() })
                })
                .collect();
            self.write(&top, "chat.plan", Actor::Core, json!({ "chat": thread, "done": done, "total": total, "lanes": lanes, "key": key }));
        }
    }

    /// A line typed in a branch chat steers its lane while it works; a lane that is over says so.
    pub fn branch_say(&self, thread: &str, task: &str, text: &str) -> Said {
        match self.0.tasks.book().get(task).map(|r| r.state) {
            Some(State::Running) => self.control(thread, task, j4n_proto::Control::Steer { text: text.to_string() }),
            Some(_) => self.refuse(thread, format!("{task} is over: this lane takes no line")),
            None => self.refuse(thread, format!("there is no task {task}")),
        }
    }

    /// The next chat that waits for you, then the next that runs, after `from` (`⇥1`): its id, or none when nothing waits or runs.
    pub fn next_chat_after(&self, from: &str) -> Option<String> {
        let rows = self.chat_list();
        let cands = |state: &str| rows.iter().filter(|r| r.data.as_ref().is_some_and(|d| d["state"] == state)).map(|r| r.key.clone()).collect::<Vec<_>>();
        for state in ["waits", "running"] {
            let list = cands(state);
            if let Some(at) = list.iter().position(|k| k == from) {
                if let Some(next) = list.get(at + 1).or_else(|| list.first()).filter(|k| *k != from) {
                    return Some(next.clone());
                }
            } else if let Some(first) = list.first() {
                return Some(first.clone());
            }
        }
        None
    }
}

/// The event as the chat `here` folds it (empty: the orchestrator): a message between chats is drawn by who is looking, so the fold is told.
pub fn tagged(ev: &crate::log::Ev, here: &str) -> crate::log::Ev {
    let mut ev = ev.clone();
    if matches!(ev.kind.as_str(), "chat.message" | "chat.message.delivered") {
        ev.data["here"] = Json::String(here.to_string());
    }
    ev
}

/// The `seq` of the last event that emptied a chat's rows (`chat.cleared`) that no `chat.history` has brought back since; 0 when none did.
pub fn cleared_before(events: &[crate::log::Ev]) -> u64 {
    events.iter().fold(0, |cut, e| match e.kind.as_str() {
        "chat.cleared" => e.seq,
        "chat.history" => 0,
        _ => cut,
    })
}
