//! What a chat of a split may see of the other parts and of the folder: read-only copies in its worktree, at the paths they have in the project
//! (shares, [`crate::place::Place::share`]). Behind `orchestrate.access`, on while `lanes` is `orchestrate`. Each request is the ask point `access`, a yes
//! or a no, walked as a ladder:
//!
//! 1. **The plan**, in code (`rule:access`): a file another part writes is given when this part's `reads` names it or its item `depends` on that one; a
//!    file Main's line wrote in the folder is given when `reads` names it; the folder's build files go to every chat that writes; the chat's own files and
//!    a file nothing holds are refused.
//! 2. **The classifier Deciders** (`decider:jev`, `decider:span`), after their probe cases pass, for what the plan does not name.
//! 3. **Main**, for what is still open: a line `access <id>: yes` or `access <id>: no` in the answer of the turn it was put to, read once; any other form is
//!    a no.
//! 4. **The guard**, above every rung: a secret, a key, a doer's own state, a path outside the project or in `.git` is refused whatever was said. It is
//!    judged before any rung is asked, so such a path reaches no Decider and not Main; the place judges the copy again (a plain file, inside the worktree).
//!
//! The Log has `access.asked`, `access.given` and `access.refused`, each with the rung that answered and why, and `access.shared` for each copy made or
//! refreshed. A share is refreshed at each step of Main's plan and when the chat asks; the chat is told of a change on its next turn.

use super::Front;
use crate::answer::{Answerer, field, fn_rule, picked};
use crate::ask::Opt;
use crate::change::Actor;
use crate::keeper::{Item, Plan};
use crate::place::Shared;
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

pub const POINT: &str = "access";
/// Where a share comes from when no part writes it: the project's folder.
pub const FOLDER: &str = "folder";

/// The folder's build files (a manifest, a lock file, test settings), at the top of the project: what every chat that writes is given.
pub const BUILD: [&str; 20] = [
    "package.json", "package-lock.json", "pnpm-lock.yaml", "yarn.lock", "tsconfig.json", "jest.config.js", "vitest.config.ts", "pyproject.toml", "requirements.txt",
    "setup.cfg", "setup.py", "pytest.ini", "tox.ini", "conftest.py", "Cargo.toml", "Cargo.lock", "go.mod", "go.sum", "Makefile", "Gemfile",
];

/// The most files a share of a folder is.
pub const FILES: usize = 200;

/// What a share of a folder leaves out: git's, J4n's, and build output.
const SKIP: [&str; 8] = [".git", ".j4n", "node_modules", "target", "__pycache__", ".venv", "dist", "build"];

/// What Main is told to answer for the requests put to it.
pub const WORDS: &str = "For each request, end your answer with one line `access <id>: yes` to give the chat a read-only copy of that file, or `access <id>: no`. A line in another form is a no; each request is decided once.";

/// What a chat that writes is told of the tool, after its shares.
pub const TOOL_CLI: &str = "To read another file of another part or of the folder, `j4n.call('access', want='c1:src/x.py', why='…')` in `j4n py` (`folder:` for the folder's; `want='list'` says what you have and what changed). The answer comes at once, or with your next turn when Main decides.";

/// A request no rung settled: Main's to answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Open {
    pub id: String,
    pub chat: String,
    pub path: String,
    pub from: String,
    pub why: String,
    /// When it was put in Main's Brief: only a turn that began after reads Main's word on it.
    pub put_ms: Option<u64>,
    /// Main was woken for it.
    pub woke: bool,
}

/// What the front holds of the access.
#[derive(Debug, Default)]
pub struct State {
    /// What each chat was given: path → where it comes from (`folder`, or the part that writes it).
    given: BTreeMap<String, BTreeMap<String, String>>,
    /// The `(chat, path)` already asked about.
    seen: BTreeSet<(String, String)>,
    open: Vec<Open>,
    n: u64,
    /// What each chat is told on its next turn.
    news: BTreeMap<String, Vec<String>>,
}

/// What came of a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decided {
    Given { by: String, why: String },
    Refused { by: String, why: String },
    /// Put to Main, as this id.
    Open(String),
}

/// `path` is `p` or below it.
pub(super) fn covers(paths: &[String], p: &str) -> bool {
    paths.iter().any(|w| p == w || p.starts_with(&format!("{}/", w.trim_end_matches('/'))))
}

/// Why the guard refuses `path`, whatever any rung said: none when it may be read.
pub fn guard(path: &str) -> Option<&'static str> {
    if !super::split::inside(path) {
        return Some("the path is not inside the project");
    }
    if path.split('/').any(|c| c == ".git") {
        return Some("it is inside `.git`");
    }
    crate::gate::door::secret_file(path).then_some("it is a secret, a key or a doer's own state (the guard's deny list): it is never read")
}

/// `rule:access`, the plan's rung: it reads the facts the front puts as lines (`own`, `theirs`, `named`, `made`, `build`, `writes`, `held`).
pub fn access_rule() -> impl Answerer {
    fn_rule("rule:access", |q| {
        let yes = |k: &str| field(&q.text, k) == Some("yes");
        if yes("own") {
            return picked("no", "the chat writes this file itself: its own file is never a share");
        }
        if !yes("held") {
            return picked("no", "no other part writes it and the folder has no such file");
        }
        if yes("theirs") && yes("named") {
            return picked("yes", "another part writes it, and this part reads it or depends on that part");
        }
        if yes("made") && yes("named") {
            return picked("yes", "Main's line wrote it in the folder, and this part reads it");
        }
        if yes("build") && yes("writes") {
            return picked("yes", "a build file of the folder, given to every chat that writes");
        }
        Err("the plan does not name it for this part".into())
    })
}

/// The lines `access <id>: <word>` of an answer, the id and the word as typed, without marks.
pub fn access_words(answer: &str) -> Vec<(String, String)> {
    let bare = |w: &str| w.trim().trim_matches(['*', '`', '.', ' ']).to_lowercase();
    answer
        .lines()
        .filter_map(|l| {
            let l = l.trim().trim_start_matches(['*', '`', '-', ' ']);
            let head = l.get(..7).filter(|h| h.eq_ignore_ascii_case("access "))?;
            let (id, word) = l[head.len()..].split_once(':')?;
            Some((bare(id), bare(word)))
        })
        .collect()
}

impl Front {
    /// The access works: Main works with chats and `orchestrate.access` is on.
    pub fn access_on(&self) -> bool {
        self.main_works() && self.0.cfg().orchestrate.access
    }

    fn access_state(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.access.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn main_plan(&self) -> Plan {
        self.0.keeper.as_ref().and_then(|k| k.plan(&self.0.cfg().thread)).unwrap_or_default()
    }

    /// The paths a part is given by the plan, before it asks: the files another part writes that it reads or depends on, what Main's line wrote that it
    /// reads, and the folder's build files when it writes.
    fn access_wants(&self, me: &Item, plan: &Plan) -> Vec<String> {
        let root = self.0.cfg().root.clone();
        let mut out: BTreeSet<String> = BTreeSet::new();
        for o in plan.items.iter().filter(|o| o.id != me.id) {
            if me.depends.contains(&o.id) {
                out.extend(o.writes.iter().cloned());
                continue;
            }
            // what the part reads of what the other writes: the file it names inside a folder the other writes, or what the other writes inside a folder it names
            for r in &me.reads {
                for w in &o.writes {
                    if covers(std::slice::from_ref(w), r) {
                        out.insert(r.clone());
                    } else if covers(std::slice::from_ref(r), w) {
                        out.insert(w.clone());
                    }
                }
            }
        }
        out.extend(me.reads.iter().filter(|r| self.main_made(r)).cloned());
        if !me.writes.is_empty() {
            out.extend(BUILD.iter().filter(|b| root.join(b).is_file() || plan.items.iter().any(|o| o.id != me.id && covers(&o.writes, b))).map(|b| b.to_string()));
        }
        out.into_iter().filter(|p| !covers(&me.writes, p)).collect()
    }

    /// Main's own line wrote `path` in the folder.
    fn main_made(&self, path: &str) -> bool {
        let main = self.0.cfg().thread.clone();
        self.0.tasks.book().rows().into_iter().filter(|r| r.spec.thread.as_deref() == Some(main.as_str())).any(|r| r.changed.created.iter().chain(&r.changed.modified).any(|c| c == path))
    }

    /// One request of chat `me` for `path`, up the ladder; `asked_by` is `plan` or `chat`. With `may_open` off a request no rung settles is let go
    /// unasked (the plan's own wants), else it is put to Main.
    pub(super) fn access_ask(&self, me: &Item, plan: &Plan, path: &str, why: &str, asked_by: &str, may_open: bool) -> Decided {
        let chat = me.id.clone();
        let owner = plan.items.iter().find(|o| o.id != me.id && covers(&o.writes, path));
        let from = owner.map_or(FOLDER.to_string(), |o| o.id.clone());
        self.access_state().seen.insert((chat.clone(), path.to_string()));
        self.write(&chat, "access.asked", Actor::Core, json!({ "chat": chat, "path": path, "from": from, "why": crate::text::clip(why, 200), "by": asked_by }));
        let refuse = |by: &str, why: &str| {
            self.write(&chat, "access.refused", Actor::Core, json!({ "chat": chat, "path": path, "from": from, "rung": by, "why": why }));
            Decided::Refused { by: by.into(), why: why.into() }
        };
        if let Some(why) = guard(path) {
            return refuse("guard", why);
        }
        let yn = |b: bool| if b { "yes" } else { "no" };
        let theirs = owner.is_some();
        let named = covers(&me.reads, path) || owner.is_some_and(|o| me.depends.contains(&o.id));
        let held = theirs || self.0.cfg().root.join(path).exists();
        // the facts in the order a probe case puts its slices, so what is proved is what is asked
        let text = format!(
            "chat {chat} asks to read {path}, from {from}, as a read-only copy: {}\nbuild: {}\nheld: {}\nmade: {}\nnamed: {}\nown: {}\ntheirs: {}\nwrites: {}",
            crate::text::line(why, 200),
            yn(BUILD.contains(&path)),
            yn(held),
            yn(self.main_made(path)),
            yn(named),
            yn(covers(&me.writes, path)),
            yn(theirs),
            yn(!me.writes.is_empty())
        );
        let options = ["yes", "no"].iter().map(|id| Opt { id: (*id).into(), label: (*id).into(), recommended: false }).collect();
        let mut q = self.0.who.question(POINT, Some(&chat), &text, options);
        q.goal = me.goal.clone();
        let picked = self.0.who.pick(POINT, Some(&chat), &q).filter(|(_, by, _, _)| !by.starts_with("fallback"));
        match picked {
            Some((pick, by, why, _)) if pick == "yes" => self.access_give(&chat, path, &from, &by, &why),
            Some((_, by, why, _)) => refuse(&by, &why),
            None if !may_open => refuse("plan", "the plan does not name it, and nothing was asked"),
            None => {
                let id = {
                    let mut s = self.access_state();
                    s.n += 1;
                    let id = format!("a{}", s.n);
                    s.open.push(Open { id: id.clone(), chat: chat.clone(), path: path.into(), from: from.clone(), why: crate::text::line(why, 200), put_ms: None, woke: false });
                    id
                };
                self.write(&chat, "access.asked", Actor::Core, json!({ "chat": chat, "path": path, "from": from, "id": id, "rung": "main", "why": "no rung settled it: Main is asked" }));
                Decided::Open(id)
            }
        }
    }

    fn access_give(&self, chat: &str, path: &str, from: &str, by: &str, why: &str) -> Decided {
        self.access_state().given.entry(chat.to_string()).or_default().insert(path.to_string(), from.to_string());
        self.write(chat, "access.given", Actor::Core, json!({ "chat": chat, "path": path, "from": from, "rung": by, "why": why }));
        Decided::Given { by: by.into(), why: why.into() }
    }

    /// The plain files a share of `path` for `chat` is copied from now, each at its project path, and whose they are: from the worktree of the part that
    /// writes it while there is one, else from the folder. A folder is its plain files below it (no link, no `.git`, `.j4n` or build output, none the guard
    /// refuses, none the chat writes itself), at most [`FILES`]; the source must stay inside the worktree or the folder it is read from.
    fn access_files(&self, plan: &Plan, chat: &str, path: &str) -> (Vec<(String, PathBuf)>, String) {
        let owner = plan.items.iter().find(|o| o.id != chat && covers(&o.writes, path));
        let mine: Vec<String> = plan.items.iter().find(|o| o.id == chat).map(|o| o.writes.clone()).unwrap_or_default();
        let site = owner.filter(|o| !o.task.is_empty()).and_then(|o| self.0.tasks.site_dir(&o.task)).filter(|d| d.join(path).exists());
        let whose = match (owner, &site) {
            (Some(o), Some(_)) => format!("{}'s", o.id),
            (Some(o), None) => format!("{}'s, laid in", o.id),
            (None, _) => "the folder's".to_string(),
        };
        let base = site.unwrap_or_else(|| self.0.cfg().root.clone());
        let Ok(base) = base.canonicalize() else { return (Vec::new(), whose) };
        let mut out = Vec::new();
        let mut todo = vec![path.trim_end_matches('/').to_string()];
        while let Some(rel) = todo.pop() {
            let f = base.join(&rel);
            let Ok(meta) = std::fs::symlink_metadata(&f) else { continue };
            if meta.file_type().is_symlink() || !f.canonicalize().is_ok_and(|c| c.starts_with(&base)) || guard(&rel).is_some() || covers(&mine, &rel) {
                continue;
            }
            if meta.is_file() {
                out.push((rel, f));
                if out.len() >= FILES {
                    break;
                }
            } else if meta.is_dir()
                && let Ok(rd) = std::fs::read_dir(&f)
            {
                let mut names: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| !SKIP.contains(&n.as_str())).collect();
                names.sort();
                todo.extend(names.into_iter().rev().map(|n| format!("{rel}/{n}")));
            }
        }
        (out, whose)
    }

    /// The shares of `chat` whose source is there now, as the place takes them before a task starts: `[{path, from}]`.
    pub(super) fn access_input(&self, chat: &str) -> Json {
        let plan = self.main_plan();
        let given = self.access_state().given.get(chat).cloned().unwrap_or_default();
        Json::Array(given.keys().flat_map(|p| self.access_files(&plan, chat, p).0).map(|(p, f)| json!({ "path": p, "from": f })).collect())
    }

    /// What a chat that writes is told of its shares and of the tool, after its brief; empty when the access is off.
    pub(super) fn access_told(&self, chat: &str) -> String {
        if !self.access_on() {
            return String::new();
        }
        let plan = self.main_plan();
        let given = self.access_state().given.get(chat).cloned().unwrap_or_default();
        let list: Vec<String> = given.keys().map(|p| match self.access_files(&plan, chat, p) {
            (files, whose) if files.len() == 1 && files[0].0 == p.trim_end_matches('/') => format!("{p} ({whose})"),
            (files, _) if files.is_empty() => format!("{p} (not written yet)"),
            (files, whose) => format!("{p} ({} files, {whose})", files.len()),
        }).collect();
        let news = self.access_state().news.remove(chat).unwrap_or_default();
        let mut out = String::new();
        if !list.is_empty() {
            out.push_str(&format!("\nShared with you, read-only, as their source has them now (J4n keeps them so and tells you when one changes): {}. Import or read them as they are; do not write them: a change to a share is sent back.", list.join(", ")));
        }
        if !news.is_empty() {
            out.push_str(&format!("\nSince your last turn: {}.", news.join("; ")));
        }
        out.push('\n');
        out.push_str(TOOL_CLI);
        out
    }

    /// Before a chat that writes starts: the plan's rung over what it is given, as the place takes it (`[{path, from}]`). `others` are the paths the other
    /// chats of the split write, some of which are in no item yet.
    pub(super) fn access_start(&self, me: &Item, others: &[String]) -> Json {
        let mut plan = self.main_plan();
        let known: Vec<String> = others.iter().filter(|p| !plan.items.iter().any(|o| o.id != me.id && covers(&o.writes, p))).cloned().collect();
        plan.items.push(Item { id: "another chat".into(), writes: known, ..Item::default() });
        for p in self.access_wants(me, &plan) {
            self.access_ask(me, &plan, &p, "the plan gives it", "plan", false);
        }
        self.access_input(&me.id)
    }

    /// Each step of Main's plan: what the plan gives a part not done yet that it was not asked yet (a part started after it), and every share made as its source is
    /// now. A share that changed is said to its chat on its next turn.
    pub(super) fn access_step(&self, thread: &str) {
        if thread != self.0.cfg().thread || !self.access_on() {
            return;
        }
        let plan = self.main_plan();
        // a part's worktree is checked and reviewed after its turn ends, so its shares are kept as their sources are until it is done
        for me in plan.items.iter().filter(|i| i.chat_part() && self.is_chat_item(i) && !i.done() && !i.task.is_empty() && self.0.tasks.site_dir(&i.task).is_some()) {
            for p in self.access_wants(me, &plan) {
                if !self.access_state().seen.contains(&(me.id.clone(), p.clone())) {
                    self.access_ask(me, &plan, &p, "the plan gives it", "plan", false);
                }
            }
            self.access_sync(&plan, me);
        }
    }

    /// Make every share of `me` as its source is now, in the site of its task.
    fn access_sync(&self, plan: &Plan, me: &Item) {
        let given = self.access_state().given.get(&me.id).cloned().unwrap_or_default();
        let files = given.keys().flat_map(|p| {
            let (files, whose) = self.access_files(plan, &me.id, p);
            files.into_iter().map(move |f| (f, whose.clone()))
        });
        for ((path, from), whose) in files {
            let path = &path;
            match self.0.tasks.share(&me.task, path, &from) {
                Ok(Shared::Same) => {}
                Ok(done) => {
                    let change = if done == Shared::Changed { "changed" } else { "placed" };
                    self.write(&me.id, "access.shared", Actor::Core, json!({ "chat": me.id, "path": path, "from": whose, "task": me.task, "change": change }));
                    if done == Shared::Changed {
                        self.access_state().news.entry(me.id.clone()).or_default().push(format!("{whose} {path} changed, and your read-only copy is as it is now"));
                    }
                }
                Err(why) => {
                    self.write(&me.id, "access.shared", Actor::Core, json!({ "chat": me.id, "path": path, "from": whose, "task": me.task, "error": why }));
                }
            }
        }
    }

    /// The tool `access`, from the turn `task`: `want` is `<part>:<path>`, `folder:<path>` or `list`. What came of it, in words.
    pub fn access_call(&self, task: &str, want: &str, why: &str) -> Result<String, String> {
        if !self.access_on() {
            return Err("the access is off (`orchestrate.access`), or Main works no chats".into());
        }
        let row = self.0.tasks.book().get(task).ok_or("there is no such turn")?;
        let plan = self.main_plan();
        let me = plan.items.iter().find(|i| i.task == row.id && i.chat_part()).cloned().ok_or("only a chat of a split that writes asks for a share")?;
        if want.trim() == "list" {
            let told = self.access_told(&me.id);
            return Ok(if told.trim() == TOOL_CLI { "you have no share yet".into() } else { told.trim().to_string() });
        }
        let (_, path) = want.split_once(':').ok_or("`want` is `<part>:<path>`, `folder:<path>` or `list`")?;
        let path = path.trim();
        let said = match self.access_ask(&me, &plan, path, why, "chat", true) {
            Decided::Given { by, why } => {
                self.access_sync(&plan, &me);
                format!("given by {by} ({why}): {path} is in your worktree, read-only, as its source has it now")
            }
            Decided::Refused { by, why } => format!("refused by {by}: {why}"),
            Decided::Open(id) => {
                self.wake_main(&self.0.cfg().thread);
                format!("no rung settled it, so Main is asked ({id}); its answer comes with your next turn: go on with your part")
            }
        };
        Ok(said)
    }

    /// The requests that wait for Main's word, as its Brief shows them; each is marked as put.
    pub(super) fn access_section(&self) -> Option<String> {
        if !self.access_on() {
            return None;
        }
        let now = self.0.log.clock().now_ms();
        let mut s = self.access_state();
        if s.open.is_empty() {
            return None;
        }
        let lines: Vec<String> = s.open.iter_mut().map(|o| {
            o.put_ms.get_or_insert(now);
            format!("- {}: {} asks to read {} ({}): {}", o.id, o.chat, o.path, o.from, o.why)
        }).collect();
        Some(format!("Requests of chats to read a file, which no rung settled:\n{}\n{WORDS}", lines.join("\n")))
    }

    /// A request waits that Main was not woken for.
    pub(super) fn access_waits(&self) -> bool {
        self.access_on() && self.access_state().open.iter().any(|o| !o.woke)
    }

    /// The first line of each request Main was not woken for yet; each is marked.
    pub(super) fn access_wake(&self) -> Vec<String> {
        if !self.access_on() {
            return Vec::new();
        }
        self.access_state().open.iter_mut().filter(|o| !o.woke).map(|o| {
            o.woke = true;
            format!("{}: {} asks to read {}", o.id, o.chat, o.path)
        }).collect()
    }

    /// Main's turn ended well: each request put to it before that turn began is decided by its word, once. `yes` gives it (the guard judged it before it was
    /// put); `no`, any other word, and no line at all refuse it.
    pub(super) fn access_main_step(&self, thread: &str) {
        if thread != self.0.cfg().thread || !self.access_on() {
            return;
        }
        let Some(row) = self.last_task(thread).filter(|r| r.state == crate::task::State::Done { ok: true } && r.ended_ms.is_some_and(|t| t >= self.0.since_ms)) else { return };
        let began = row.started_ms.unwrap_or(row.queued_ms);
        let put: Vec<Open> = {
            let mut s = self.access_state();
            let (put, wait): (Vec<Open>, Vec<Open>) = std::mem::take(&mut s.open).into_iter().partition(|o| o.put_ms.is_some_and(|p| p <= began));
            s.open = wait;
            put
        };
        if put.is_empty() || !self.once(&format!("access:{}", row.id)) {
            return;
        }
        let words = access_words(row.answer.as_deref().unwrap_or_default());
        for o in put {
            let word = words.iter().find(|(id, _)| *id == o.id).map(|(_, w)| w.as_str());
            let tell = match word {
                Some("yes") => {
                    self.access_give(&o.chat, &o.path, &o.from, "main", &format!("Main said yes ({})", row.id));
                    if let Some(me) = self.main_plan().items.iter().find(|i| i.id == o.chat) {
                        self.access_sync(&self.main_plan(), me);
                    }
                    format!("your request for {} was given by Main", o.path)
                }
                other => {
                    let why = match other {
                        Some("no") => "Main said no".to_string(),
                        Some(w) => format!("Main said `{w}`, which is not yes or no"),
                        None => "Main gave no word on it".to_string(),
                    };
                    self.write(&o.chat, "access.refused", Actor::Core, json!({ "chat": o.chat, "path": o.path, "from": o.from, "rung": "main", "why": why, "by": row.id }));
                    format!("your request for {} was refused: {why}", o.path)
                }
            };
            self.access_state().news.entry(o.chat.clone()).or_default().push(tell);
        }
    }
}
