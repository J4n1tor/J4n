//! The librarian: what a task is given to read is made for that task and that doer, when the setting
//! `librarian.on` is on. The order is: code picks, Jev only where code cannot choose, a chat model last (this asks none). It **starts small and grows on
//! need**: what is held back is named to the doer, which asks for it through the tool `context` (a model calls it, a CLI doer calls
//! `j4n.call('context', …)` through the kit's bridge), and the Log says what was added and why (`brief.added`).
//!
//! - **By code** (a fact of the task, never a guess): the project card is held back from a line that is short, or reads and names no file; the
//!   sections that are facts (the contract, the carry-over, the inbox, the parts, the plan, the changed files, the reports) stay as they were; the
//!   skills are those `skill::Library::pick` takes, and the doer may ask for any other by its name. Nothing is held back from a doer that cannot ask
//!   (`Librarian::asks`): it has what it had.
//! - **By Jev**, at the ask point `librarian`, only where code cannot choose: two or more skills fit equally and there is room for fewer. Jev is
//!   asked once for a task, and only when the switch is on. Where Jev is not there, or has not passed its cases, or is not sure, the pick of code
//!   stands and `brief.picked` says so (`jev`).
//! - **Off** (the default): the Brief is what it was, byte for byte, nothing is probed, nothing is asked, nothing is written and the tool is not offered.
//!
//! The doer's way to ask is [`More`], a trait the front answers for through [`Shelf`]: the wiring gives the connectors the shelf before the front exists.

use super::{Core, Front};
use crate::ask::Opt;
use crate::brief::{self, is_short, tokens};
use crate::change::Actor;
use crate::task::Mode;
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};

/// How often a task may ask for more, when the settings say nothing (`librarian.more`).
pub const MORE: usize = 3;

/// What the front is set to do.
#[derive(Clone)]
pub struct Librarian {
    /// The switch, `librarian.on`: off until it is measured against today's fixed context.
    pub on: bool,
    /// The most a task may ask for (`librarian.more`).
    pub more: usize,
    /// The doers that can ask for more: those whose connector offers them the tool `context`. A doer that is not one has nothing held back.
    pub asks: Vec<String>,
}

impl Default for Librarian {
    fn default() -> Librarian {
        Librarian { on: false, more: MORE, asks: Vec::new() }
    }
}

/// What a model is told on the first line of its session, in its contract.
pub const TELL: &str = "More of this project is there if you need it: the tool `context` with `want` `list` says what, and `why` says in a few words what for.";
/// What a doer that runs a program of its own is told after its goal: the kit's bridge is its way to the same tool.
pub const TELL_CLI: &str = "J4n holds more of this project than it put here: `j4n.call('context', want='list')` in `j4n py` says what, and `j4n.call('context', want='<name>', why='<a few words>')` adds it.";

/// What a doer asks of the librarian, through the tool `context`.
pub trait More: Send + Sync {
    /// What this task may ask for, one line each: the name, what it is and its size.
    fn menu(&self, task: &str) -> String;
    /// `want` for this task, `why` in the doer's words: the text, or why not. Written to the Log as `brief.added`.
    fn add(&self, task: &str, want: &str, why: &str) -> Result<String, String>;
}

/// The place the front puts its answer to [`More`] once it exists; the connectors hold it before.
#[derive(Default)]
pub struct Shelf(OnceLock<Arc<dyn More>>);

impl Shelf {
    /// Put the librarian on the shelf, once; a second is ignored.
    pub fn put(&self, more: Arc<dyn More>) {
        let _ = self.0.set(more);
    }

    pub fn get(&self) -> Option<&Arc<dyn More>> {
        self.0.get()
    }
}

/// The front as [`More`]: a weak hold, so the tasks that call it do not keep the front alive.
struct Desk {
    front: Weak<Core>,
    /// What each session has asked for and been given (the tokens of each), in order, by the session's first task: a follow-up in a session is the same
    /// session's, so it neither gets the card twice nor starts its count again.
    asked: Mutex<BTreeMap<String, Vec<(String, usize)>>>,
}

impl More for Desk {
    fn menu(&self, task: &str) -> String {
        let Some(c) = self.front.upgrade() else { return "the front is gone".into() };
        let front = Front(c);
        let got: Vec<String> = self.asked.lock().unwrap_or_else(|e| e.into_inner()).get(&front.library_root(task)).map(|g| g.iter().map(|(w, _)| w.clone()).collect()).unwrap_or_default();
        front.library_menu(task, &got)
    }

    fn add(&self, task: &str, want: &str, why: &str) -> Result<String, String> {
        let front = Front(self.front.upgrade().ok_or("the front is gone")?);
        let more = front.0.cfg().librarian.more;
        let mut asked = self.asked.lock().unwrap_or_else(|e| e.into_inner());
        let got = asked.entry(front.library_root(task)).or_default();
        let want = want.trim();
        if got.iter().any(|(g, _)| g == want) {
            return Ok(format!("`{want}` was added to your context already"));
        }
        // the list, and a want that says nothing, add nothing: they are not counted against the cap
        if want.is_empty() || want == "list" {
            return Ok(front.library_menu(task, &got.iter().map(|(w, _)| w.clone()).collect::<Vec<_>>()));
        }
        if got.len() >= more {
            return Err(format!("this session has asked for more {} times, which is as many as it may (`librarian.more`): work with what it has", got.len()));
        }
        // what the additions of a session may come to together is what the skills of a task may (`skills.tokens`): the rest of it is what this one may take
        let left = front.library_cap().saturating_sub(got.iter().map(|(_, n)| n).sum());
        if left == 0 {
            return Err("what this session has added comes to as many tokens as it may (`skills.tokens`): work with what it has".into());
        }
        let (text, added) = front.library_add(task, want, why, left)?;
        if let Some(n) = added {
            got.push((want.to_string(), n));
        }
        Ok(text)
    }
}

/// The most lines `list` shows, and the most characters of each.
const MENU_LINES: usize = 20;
const MENU_LINE: usize = 160;

/// `text` cut to `max` tokens at a line (a first line that is longer is cut where it stands), and whether it was cut.
fn cut_to(text: &str, max: usize) -> (String, bool) {
    if tokens(text) <= max {
        return (text.to_string(), false);
    }
    let mut out = String::new();
    for l in text.lines() {
        if tokens(&format!("{out}{l}\n")) > max {
            break;
        }
        out.push_str(l);
        out.push('\n');
    }
    if out.is_empty() {
        out = text.chars().take(max * 4).collect();
    }
    (out.trim_end().to_string(), true)
}

/// What a skill is for, in one line: its name, when it applies, its size.
fn skill_line(s: &crate::skill::Skill) -> String {
    format!("{} · {} · ~{} tokens", s.id, s.when, s.cost)
}

/// The one line of the Log and the chat for a task's start: what was held back and why, and how the skills were chosen.
pub(super) struct Picked {
    pub held: Vec<(&'static str, String)>,
    /// How Jev took part: `not asked`, `asked, picked …`, `absent: …` (the pick of code stands).
    pub jev: String,
}

impl Front {
    /// Make the front the answer to [`More`] for this shelf (the wiring gives the same shelf to the connectors).
    pub fn with_librarian(self, shelf: &Shelf) -> Front {
        shelf.put(Arc::new(Desk { front: Arc::downgrade(&self.0), asked: Mutex::default() }));
        self
    }

    /// The librarian is on for this doer: the switch, and a doer that can ask for what it is not given.
    pub(super) fn librarian_for(&self, doer: &str) -> bool {
        let l = &self.0.cfg().librarian;
        l.on && l.asks.iter().any(|d| d == doer)
    }

    /// Whether the project card is for this task, and why or why not: a fact of the line and the task. It is wanted by a task that may write, one
    /// that names a file or a place of the project, and one whose line says it is about the project; not by a short line or a plain question.
    pub(super) fn card_wanted(line: &str, mode: Mode) -> (bool, &'static str) {
        const ABOUT: [&str; 9] = ["project", "repo", "repository", "codebase", "code", "file", "files", "module", "crate"];
        if is_short(line) {
            return (false, "the line is short");
        }
        if mode != Mode::Read {
            return (true, "the task may write");
        }
        let said = brief::said(line);
        if line.split_whitespace().any(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && !matches!(c, '/' | '.' | '_' | '-')).contains(['/', '.']) && w.len() > 2) {
            return (true, "the line names a file");
        }
        if said.iter().any(|w| ABOUT.contains(&w.as_str())) {
            return (true, "the line is about the project");
        }
        (false, "a plain question that names no file")
    }

    /// A doer that runs a program of its own is given what it was (the carry-over, the picked skills, its profile: the connector's), and, when it can ask,
    /// one line after its goal that says there is more and how to ask; what the card and the claims could add is named in `input.held`.
    pub(super) fn library_cli(&self, thread: &str, spec: &mut crate::task::TaskSpec, in_session: bool, jev: &std::cell::RefCell<String>) {
        let asks = self.librarian_for(&spec.doer);
        let mut held: Vec<(&'static str, String)> = Vec::new();
        if asks && !in_session {
            if self.0.cfg().card.as_ref().and_then(|c| c.text()).is_some() {
                held.push(("project", "a program of its own reads the project itself".into()));
            }
            if self.0.cfg().recall.as_ref().and_then(|r| r.text(&spec.goal)).is_some() {
                held.push(("remembered", "claims of an earlier report, for the asking".into()));
            }
            let have: Vec<&str> = spec.input.get("skills").and_then(Json::as_array).into_iter().flatten().filter_map(|s| s.get("id").and_then(Json::as_str)).collect();
            let more = self.0.cfg().skills.as_ref().is_some_and(|l| l.all().iter().any(|s| l.is_on(&s.id) && !have.contains(&s.id.as_str())));
            if !held.is_empty() || more {
                spec.input["context"] = json!(TELL_CLI);
            }
        }
        spec.input["held"] = json!(held.iter().map(|(w, _)| w).collect::<Vec<_>>());
        if !in_session || *jev.borrow() != "not asked" {
            let skills = spec.input.get("skills").and_then(Json::as_array).map_or(0, Vec::len);
            self.library_said(thread, &spec.doer.clone(), &spec.goal.clone(), "cli", &Picked { held, jev: jev.borrow().clone() }, skills);
        }
    }

    /// The Log row and the chat row of a task's start, `brief.picked`.
    pub(super) fn library_said(&self, thread: &str, doer: &str, spec_goal: &str, kind: &str, p: &Picked, picked: usize) {
        let held = if p.held.is_empty() { "nothing held back".to_string() } else { p.held.iter().map(|(w, why)| format!("{w} held back ({why})")).collect::<Vec<_>>().join(" · ") };
        let line = format!("context · {held} · {picked} skills · jev: {} · more on ask (`context`)", p.jev);
        self.write(thread, "brief.picked", Actor::Core, json!({ "doer": doer, "goal": crate::text::clip(spec_goal, 80), "for": kind, "held": p.held.iter().map(|(w, why)| json!({ "what": w, "why": why })).collect::<Vec<_>>(), "skills": picked, "jev": p.jev, "line": line }));
        self.note(thread, &line, "dim");
    }

    /// Ask Jev which of the skills that fit equally to take (the ask point `librarian`); `None` and how it went when Jev did not choose, and the pick
    /// of code stands.
    pub(super) fn library_choose(&self, thread: &str, line: &str, group: &[(String, String)], how: &std::cell::RefCell<String>) -> Option<String> {
        let mut options: Vec<Opt> = group.iter().map(|(id, about)| Opt { id: id.clone(), label: format!("{id}: {about}"), recommended: false }).collect();
        options.push(Opt { id: "none".into(), label: "none of these is better than the others".into(), recommended: true });
        let q = self.0.who.question("librarian", Some(thread), line, options);
        let Some((pick, by, why, _)) = self.0.who.pick("librarian", Some(thread), &q) else {
            *how.borrow_mut() = "absent: no ask point `librarian`, the pick of code stands".into();
            return None;
        };
        if by.starts_with("fallback") {
            *how.borrow_mut() = format!("absent: {why}; the pick of code stands");
            return None;
        }
        if pick == "none" {
            *how.borrow_mut() = format!("asked ({by}): none better; the pick of code stands");
            return None;
        }
        *how.borrow_mut() = format!("asked ({by}): {pick}");
        Some(pick)
    }

    /// The session a task is in: the task and the tasks it follows, newest first.
    fn library_session(&self, task: &str) -> Vec<crate::task::Row> {
        let book = self.0.tasks.book();
        let mut rows: Vec<crate::task::Row> = Vec::new();
        let mut next = Some(task.to_string());
        while let Some(row) = next.take().and_then(|id| book.get(&id)).filter(|_| rows.len() < 64) {
            next = row.spec.follows.clone();
            rows.push(row);
        }
        rows
    }

    /// The first task of the session `task` is in: what a session has asked for is kept by it.
    fn library_root(&self, task: &str) -> String {
        self.library_session(task).last().map_or_else(|| task.to_string(), |r| r.id.clone())
    }

    /// What a session's additions may come to together: what the skills of a task may (`skills.tokens`).
    fn library_cap(&self) -> usize {
        match self.skill_set("tokens") {
            crate::value::Value::Num(n) => n as usize,
            _ => super::skills::TOKENS as usize,
        }
    }

    /// The names that were held back from a session's tasks, and the skills they were given.
    fn library_has(&self, task: &str) -> (Vec<String>, Vec<String>) {
        let rows = self.library_session(task);
        let names = |key: &str, pick: &dyn Fn(&Json) -> Option<String>| -> Vec<String> { rows.iter().flat_map(|r| r.spec.input.get(key).and_then(Json::as_array).into_iter().flatten().filter_map(pick)).collect() };
        (names("held", &|v| v.as_str().map(String::from)), names("skills", &|v| v.get("id").and_then(Json::as_str).map(String::from)))
    }

    /// What the task `task` may ask for: the card and the claims when they were held back from its session, and the skills that were not picked; not
    /// what was added already (`got`). At most [`MENU_LINES`] lines, and it says how many more there are.
    fn library_menu(&self, task: &str, got: &[String]) -> String {
        if self.0.tasks.book().get(task).is_none() {
            return format!("there is no task `{task}`");
        }
        let (held, have) = self.library_has(task);
        let mut lines = Vec::new();
        if held.iter().any(|h| h == "project") && !got.iter().any(|g| g == "project") {
            lines.push("project · what the project is, derived from its files".to_string());
        }
        if held.iter().any(|h| h == "remembered") && !got.iter().any(|g| g == "remembered") {
            lines.push("remembered · what an earlier report of this same task claimed".to_string());
        }
        if let Some(lib) = &self.0.cfg().skills {
            lines.extend(lib.all().iter().filter(|s| lib.is_on(&s.id) && !have.contains(&s.id) && !got.contains(&s.id)).map(skill_line));
        }
        if lines.is_empty() {
            return "nothing more is held for this task".into();
        }
        let more = lines.len().saturating_sub(MENU_LINES);
        let shown: Vec<String> = lines.iter().take(MENU_LINES).map(|l| crate::text::clip(l, MENU_LINE)).collect();
        format!("`context` can add, with `want` as the first word:\n{}{}", shown.join("\n"), if more > 0 { format!("\n… and {more} more: ask by a name you know, or a word of one") } else { String::new() })
    }

    /// `want` for the task, at most `left` tokens of it: the text, and the tokens of it when it was new to the doer (else `None`). Written as `brief.added`; when
    /// the text was cut the answer says so.
    fn library_add(&self, task: &str, want: &str, why: &str, left: usize) -> Result<(String, Option<usize>), String> {
        let row = self.0.tasks.book().get(task).ok_or_else(|| format!("there is no task `{task}`"))?;
        let want = want.trim();
        let (held, have) = self.library_has(task);
        let text = match want {
            "project" if !held.iter().any(|h| h == "project") => return Ok(("the project card is already in your context, or there is none".into(), None)),
            "project" => self.0.cfg().card.as_ref().and_then(|c| c.text()).ok_or("there is no project card: it is off, or nothing is known yet")?,
            "remembered" if !held.iter().any(|h| h == "remembered") => return Ok(("no claims of an earlier report are held for this task".into(), None)),
            "remembered" => self.0.cfg().recall.as_ref().and_then(|r| r.text(&row.spec.goal)).ok_or("no earlier report of this task holds a claim that is still true")?,
            id if have.iter().any(|h| h == id) => return Ok((format!("`{id}` is already in your context"), None)),
            id => {
                let cfg_ = self.0.cfg();
                let skill = cfg_.skills.as_ref().and_then(|l| l.get(id).filter(|s| l.is_on(&s.id))).ok_or_else(|| format!("there is nothing called `{id}` to add: `context` with `want` `list` says what there is"))?;
                skill.body.clone()
            }
        };
        let (whole, (text, cut)) = (tokens(&text), cut_to(&text, left));
        let size = tokens(&text);
        let why = crate::text::clip(why.trim(), 160);
        let said = if cut { format!(" (cut to ~{size} of ~{whole} tokens)") } else { String::new() };
        let line = format!("context · added {want} (~{size} tokens{}) at the doer's ask{}", if cut { format!(" of ~{whole}") } else { String::new() }, if why.is_empty() { String::new() } else { format!(": {why}") });
        let thread = row.spec.thread.clone().unwrap_or_else(|| "main".into());
        self.write(&thread, "brief.added", Actor::Core, json!({ "task": task, "doer": row.spec.doer, "what": want, "why": why, "tokens": size, "of": whole, "cut": cut, "by": "doer", "line": line }));
        self.note(&thread, &line, "dim");
        let mut out = self.0.log.redactor().scrub(&text);
        if cut {
            out.push_str(&format!("\n[cut{said}: a session may add {} tokens together (`skills.tokens`)]", self.library_cap()));
        }
        Ok((out, Some(size)))
    }
}
