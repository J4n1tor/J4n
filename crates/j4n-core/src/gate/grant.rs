//! Grants: the operator's yes that lasts. An approval offers *once*, *this task*,
//! *this session* and *always here*; the last three only for a line that is plain enough that the operator saw all of it.
//!
//! - A grant is the operator's word for one action: the same verb on exactly the same target (a shell line, a path, a host).
//!   There is no pattern: what the operator saw is what is allowed, never more. A shell line is covered only while it is
//!   still one plain command, so a grant for `make build` never covers `make build; rm -rf ~`.
//! - `Once` is used up by the first thing it covers; `Task` ends with its task; `Session` is the process's (a grant of another
//!   session does not cover); `Here` is a `gate` row in the operator's own file for this project,
//!   `<home>/projects/<project key>/gates.toml` ([`write_here`]), which counts in every project (a project's own files are never trusted) and, like every
//!   allow, never overrides an ask or a block.
//! - Not offered beyond *once*: a compound line, a program that can run anything, a line the matcher thinks dangerous, an
//!   apply, a setting, spending or sending data out. A write to a path is offered (the path and what is below it).
//!   Only the operator makes a grant; a `grant` event written by anyone else is not read, so a rule a machine wrote is not a gate
//!   a machine loosened.
//! - Two grants widen a yes by *where* and not by *which line*, both for this session only: `Folder` (a write of a
//!   file inside the project allows the writes below its folder) and `Program` (a shell line of a program that was allowed in this
//!   session, in this folder, allows the lines of that program here: [`program_of`] says which lines). Neither is ever read by
//!   [`Covers`], so a rule that asks or blocks is not overridden; [`Grants::reaches`] answers only where no rule had a say.
//! - [`Grants`] is a fold (`grants`) of the events `grant`, `grant.used`, `grant.revoked` and the end of a task.

use super::danger::danger;
use super::{Action, Covers, Verb, Verdict};
use crate::change::{Actor, Where, digest};
use crate::fold::Fold;
use crate::log::{Ev, New};
use crate::state::project_key;
use crate::table::{Layer, LayerKind, Problem, Tables};
use crate::value::{Key, Value};
use crate::write::{Applied, Journal, Writer};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

/// How long a target may be and still be granted.
const TARGET_MAX: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Once,
    Task,
    Session,
    Here,
    /// The folder of a write inside the project, for this session.
    Folder,
    /// The program of a shell line, in this folder, for this session.
    Program,
}

impl Scope {
    pub fn word(self) -> &'static str {
        match self {
            Scope::Once => "once",
            Scope::Task => "task",
            Scope::Session => "session",
            Scope::Here => "here",
            Scope::Folder => "folder",
            Scope::Program => "program",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Grant {
    pub id: String,
    pub scope: Scope,
    pub verb: Verb,
    /// Exactly what was approved.
    pub target: String,
    #[serde(default)]
    pub task: Option<String>,
    pub session: String,
    pub at_ms: u64,
    #[serde(default)]
    pub used: bool,
    /// For a `Program` grant: the folder the lines run in.
    #[serde(default)]
    pub site: Option<String>,
}

fn plain(t: &str) -> Result<(), String> {
    if t.trim().is_empty() || t.chars().count() > TARGET_MAX || t.chars().any(char::is_control) {
        return Err("only a short line of plain text is remembered".into());
    }
    Ok(())
}

/// The program a `Program` grant would be for, when `action` (as the Gate tells it, with the folders the task may reach) may have one:
/// one plain command that starts with the program itself (no `VAR=` prefix, no `sudo`), of the class `safe`, `read` or `write`, that
/// writes nothing outside the project, in a task with a folder. A program that can run anything is refused (`sh`, `sudo`, `env`,
/// `curl`, `xargs` …) except an interpreter (`python3`, `node` …), which the operator's own scripts need; for an interpreter, no
/// code on the line (`-c`, `-e`, `--eval`, `-` or a here-document), so what runs is a file that a write of the task made.
pub fn program_of(action: &Action) -> Result<String, String> {
    plain(&action.target)?;
    if action.verb != Verb::Shell {
        return Err("only a shell line has a program".into());
    }
    let Some(site) = action.site.clone() else { return Err("the line has no folder to be allowed in".into()) };
    let d = super::danger::danger_scoped(&action.target, &super::danger::Scope { site: Some(site), tmp: action.tmp.clone(), access: action.access.clone(), own: action.own.clone(), state: action.state.clone() });
    let first = action.target.split_whitespace().next().unwrap_or_default();
    if d.compound || d.program.is_empty() || first.rsplit('/').next() != Some(d.program.as_str()) {
        return Err("a line of several commands, or one that does not start with its program, is asked about every time".into());
    }
    if !matches!(d.class.as_str(), "safe" | "read" | "write") || d.outside {
        return Err(format!("a `{}` line, or one that writes outside the project, is asked about every time", d.class));
    }
    let interp = super::danger::interpreter(&d.program);
    if d.broad && !interp {
        return Err(format!("`{}` can run anything, so it is asked about every time", d.program));
    }
    let code = |w: &str| matches!(w, "-" | "-c" | "-e" | "-E" | "-p" | "--eval" | "--print" | "--exec") || w.starts_with("<<");
    if interp && (action.target.contains("<<") || action.target.split_whitespace().skip(1).any(code)) {
        return Err(format!("code written on the line of `{}` is asked about every time", d.program));
    }
    Ok(d.program)
}

/// `path` is `root` or below it, by its folders.
fn below(root: &str, path: &str) -> bool {
    path == root || path.strip_prefix(root.trim_end_matches('/')).is_some_and(|r| r.starts_with('/'))
}

/// The folder a `Folder` grant for the write of `path` is for: its folder, when that is a folder of the project below the root of the file system.
pub fn folder_of(path: &str) -> Option<&str> {
    path.rsplit_once('/').map(|(dir, _)| dir).filter(|d| d.starts_with('/') && d.len() > 1)
}

/// Why a grant was not made, in words the operator can read.
pub fn grantable(action: &Action) -> Result<(), String> {
    let t = &action.target;
    plain(t)?;
    match action.verb {
        Verb::Shell => {
            let d = danger(t);
            if d.compound {
                return Err("a line of several commands is asked about every time".into());
            }
            if d.broad {
                return Err(format!("`{}` can run anything, so it is asked about every time", d.program));
            }
            if !matches!(d.class.as_str(), "safe" | "read" | "write") {
                return Err(format!("a `{}` command is asked about every time", d.class));
            }
            Ok(())
        }
        Verb::Net | Verb::Read | Verb::Spawn | Verb::Connect => Ok(()),
        // access to a place outside the project: the operator's word for this path and what is below it, an absolute one or `~/…`
        // never J4n's own folder: what the operator keeps there is asked about every time (an account's removal)
        Verb::Write if t.starts_with(['/', '~']) && !t.split('/').any(|c| c == ".." || c == ".j4n") => Ok(()),
        v => Err(format!("a `{}` is asked about every time", v.word())),
    }
}

/// The scopes the operator may be offered for an approval whose verdict is `verdict`: none unless it asks.
pub fn offered(action: &Action, verdict: &Verdict) -> Vec<Scope> {
    match verdict {
        Verdict::Ask { .. } => {}
        Verdict::Allow { by } if by == super::check::DEFAULT => {}
        _ => return Vec::new(),
    }
    let mut scopes = vec![Scope::Once];
    if grantable(action).is_ok() {
        scopes.extend([Scope::Task, Scope::Session]);
        // a row in the operator's file never overrides an ask, so it keeps nothing a rule asks about
        if matches!(verdict, Verdict::Allow { .. }) || action.verb == Verb::Write {
            scopes.push(Scope::Here);
        }
    }
    scopes
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Inner {
    grants: BTreeMap<String, Grant>,
}

/// The fold `grants`. A clone is a second handle on the same state: add one to the fold host and give the other to the Gate.
#[derive(Clone)]
pub struct Grants {
    session: String,
    inner: Arc<Mutex<Inner>>,
}

impl Grants {
    /// `session` names this process's run: a grant of another session no longer covers.
    pub fn new(session: &str) -> Grants {
        Grants { session: session.to_string(), inner: Arc::new(Mutex::new(Inner::default())) }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn session(&self) -> &str {
        &self.session
    }

    /// The grants that are still worth showing: not used up, not revoked.
    pub fn list(&self) -> Vec<Grant> {
        self.lock().grants.values().filter(|g| !g.used).cloned().collect()
    }

    /// The `grant` event for approving `action` at `scope`, to be appended by the caller. Only the operator makes one, only for
    /// a scope [`offered`] for the action.
    pub fn event(&self, by: &Actor, scope: Scope, action: &Action) -> Result<New, String> {
        if *by != Actor::Operator {
            return Err("only the operator gives a grant".into());
        }
        // a program is judged by [`program_of`], which lets an interpreter through that [`grantable`] refuses
        if !matches!(scope, Scope::Once | Scope::Program) {
            grantable(action)?;
        }
        let task = action.task.clone();
        // a `Folder` or a `Program` grant is for a place, so what it names is not the line the operator saw but the folder or the program
        let (verb, target, site) = match scope {
            Scope::Folder if action.verb == Verb::Write => (Verb::Write, folder_of(&action.target).ok_or("this file has no folder to be allowed")?.to_string(), None),
            Scope::Folder => return Err("only a write has a folder".into()),
            Scope::Program => (Verb::Shell, program_of(action)?, action.site.clone()),
            _ => (action.verb, action.target.clone(), None),
        };
        let id = format!("g{}", &digest(&format!("{}|{}|{}|{:?}|{}{}", scope.word(), verb.word(), target, task, self.session, site.as_ref().map(|s| format!("|{s}")).unwrap_or_default()))[..10]);
        let mut data = json!({ "id": id, "scope": scope, "verb": verb, "target": target, "task": task, "session": self.session });
        if let (Some(site), Some(into)) = (site, data.as_object_mut()) {
            into.insert("site".into(), json!(site));
        }
        let mut n = New::of("grant", Actor::Operator, data);
        n.task = task;
        Ok(n)
    }

    /// The event that says `id` was used (a `Once` grant is then spent).
    pub fn used(id: &str) -> New {
        New::of("grant.used", Actor::Core, json!({ "id": id }))
    }

    pub fn revoke(by: &Actor, id: &str) -> Result<New, String> {
        if *by != Actor::Operator {
            return Err("only the operator takes a grant back".into());
        }
        Ok(New::of("grant.revoked", Actor::Operator, json!({ "id": id })))
    }
}

impl Grants {
    /// The folders the operator gave J4n access to, for `task`: a grant of a write to a path that is not used up, for this task, this session or
    /// this project (`Here`: the grant event stays in the project's Log). What a write or a delete may reach besides the project and the task's tmp.
    pub fn access(&self, task: Option<&str>) -> Vec<String> {
        self.lock()
            .grants
            .values()
            .filter(|g| !g.used && g.verb == Verb::Write && g.target.starts_with(['/', '~']))
            .filter(|g| match g.scope {
                Scope::Once => false,
                Scope::Task => g.task.is_some() && g.task.as_deref() == task,
                Scope::Session => g.session == self.session,
                Scope::Here => true,
                // inside the project already: no more reach than the project has
                Scope::Folder | Scope::Program => false,
            })
            .map(|g| g.target.clone())
            .collect()
    }
}

impl Covers for Grants {
    fn covers(&self, action: &Action) -> Option<String> {
        if !matches!(action.verb, Verb::Shell | Verb::Net | Verb::Read | Verb::Spawn | Verb::Connect) || grantable(action).is_err() {
            return None;
        }
        self.lock()
            .grants
            .values()
            .find(|g| {
                !g.used
                    && g.verb == action.verb
                    && g.target == action.target
                    && match g.scope {
                        Scope::Once => true,
                        Scope::Task => g.task.is_some() && g.task == action.task,
                        Scope::Session => g.session == self.session,
                        Scope::Here | Scope::Folder | Scope::Program => false,
                    }
            })
            .map(|g| g.id.clone())
    }
}

impl Grants {
    /// The id of a grant that lets `action` (as the Gate tells it) go on, for an action that no rule had a say about: what [`Covers`] says,
    /// and what it does not say, because no rule asked about it: a write of a file the operator allowed for this task or session (or of a
    /// file below a folder allowed for this session), inside the project, and a line of a program allowed for this session in this folder.
    pub fn reaches(&self, action: &Action) -> Option<String> {
        if let Some(id) = self.covers(action) {
            return Some(id);
        }
        let inner = self.lock();
        match action.verb {
            Verb::Write if grantable(action).is_ok() && action.inside == Some(true) => inner
                .grants
                .values()
                .filter(|g| g.verb == Verb::Write && !g.used)
                .find(|g| match g.scope {
                    Scope::Once => g.target == action.target,
                    Scope::Task => g.task.is_some() && g.task == action.task && below(&g.target, &action.target),
                    Scope::Session | Scope::Folder => g.session == self.session && below(&g.target, &action.target),
                    Scope::Here | Scope::Program => false,
                })
                .map(|g| g.id.clone()),
            Verb::Shell => {
                let program = program_of(action).ok()?;
                inner.grants.values().find(|g| g.scope == Scope::Program && g.session == self.session && g.target == program && g.site == action.site).map(|g| g.id.clone())
            }
            _ => None,
        }
    }
}

fn text(ev: &Ev, field: &str) -> Option<String> {
    ev.data.get(field).and_then(Json::as_str).map(str::to_string)
}

impl Fold for Grants {
    fn id(&self) -> &str {
        "grants"
    }

    fn version(&self) -> u64 {
        1
    }

    fn wants(&self, kind: &str) -> bool {
        matches!(kind, "grant" | "grant.used" | "grant.revoked" | "task.settled" | "task.stopped")
    }

    fn apply(&mut self, ev: &Ev) {
        let mut inner = self.lock();
        match ev.kind.as_str() {
            "grant" if ev.by == Actor::Operator => {
                let Ok(scope) = serde_json::from_value::<Scope>(ev.data.get("scope").cloned().unwrap_or_default()) else { return };
                let Ok(verb) = serde_json::from_value::<Verb>(ev.data.get("verb").cloned().unwrap_or_default()) else { return };
                let (Some(id), Some(target)) = (text(ev, "id"), text(ev, "target")) else { return };
                let g = Grant { id: id.clone(), scope, verb, target, task: text(ev, "task"), session: text(ev, "session").unwrap_or_default(), at_ms: ev.at_ms, used: false, site: text(ev, "site") };
                inner.grants.insert(id, g);
            }
            "grant.used" => {
                if let Some(g) = text(ev, "id").and_then(|id| inner.grants.get_mut(&id)).filter(|g| g.scope == Scope::Once) {
                    g.used = true;
                }
            }
            "grant.revoked" if ev.by == Actor::Operator => {
                if let Some(id) = text(ev, "id") {
                    inner.grants.remove(&id);
                }
            }
            "task.settled" | "task.stopped" => {
                if let Some(t) = &ev.task {
                    inner.grants.retain(|_, g| !(g.scope == Scope::Task && g.task.as_ref() == Some(t)));
                }
            }
            _ => {}
        }
    }

    fn read(&self, path: &[Key], _now_ms: u64) -> Value {
        let inner = self.lock();
        let map = inner.grants.iter().map(|(id, g)| {
            let m = BTreeMap::from([
                ("scope".to_string(), Value::text(g.scope.word())),
                ("verb".to_string(), Value::text(g.verb.word())),
                ("target".to_string(), Value::text(&g.target)),
                ("used".to_string(), Value::Bool(g.used)),
                ("says".to_string(), Value::text(says(g.scope))),
                ("counts".to_string(), Value::Bool(!matches!(g.scope, Scope::Session | Scope::Folder | Scope::Program) || g.session == self.session)),
            ]);
            (id.clone(), Value::Map(m))
        });
        Value::Map(map.collect()).at(path)
    }

    fn save(&self) -> Json {
        serde_json::to_value(&*self.lock()).unwrap_or_default()
    }

    fn load(&mut self, state: Json) -> Result<(), String> {
        *self.lock() = serde_json::from_value(state).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn reset(&mut self) {
        self.lock().grants.clear();
    }
}

// ---- always here ----

/// The folder of the operator's own rules for this project, outside the project: `<home>/projects/<key>`.
pub fn here_dir(home: &Path, root: &Path) -> PathBuf {
    home.join("projects").join(project_key(root))
}

/// The layer those rules make (kind `User`), to load above the packs and the home layer.
pub fn here_layer(home: &Path, root: &Path) -> (Layer, Vec<Problem>) {
    Layer::from_dir(LayerKind::User, "here", &here_dir(home, root))
}

/// The id of the row that allows `action` here.
pub fn here_id(action: &Action) -> String {
    here_id_of(action.verb.word(), &action.target)
}

/// The id of the row that allows `verb` on exactly `target` here.
pub fn here_id_of(verb: &str, target: &str) -> String {
    format!("here-{}", &digest(&format!("{verb}|{target}"))[..8])
}

/// The words a grant's scope is said in on a row: `once`, `this task`, `this session`, `always here`.
pub fn says(scope: Scope) -> &'static str {
    match scope {
        Scope::Once => "once",
        Scope::Task => "this task",
        Scope::Session => "this session",
        Scope::Here => "always here",
        Scope::Folder => "this folder, this session",
        Scope::Program => "this program here, this session",
    }
}

/// Write the row that allows exactly `action` in this project, through the Table writer, recording the change in `journal`
/// first. The same action twice is the same row. Refused for anything but the operator, and for an action
/// [`grantable`] refuses.
pub fn write_here(home: &Path, root: &Path, action: &Action, by: &Actor, at_ms: u64, journal: &mut dyn Journal) -> Result<Applied, Problem> {
    let at = format!("grant {}", action.target);
    if *by != Actor::Operator {
        return Err(Problem::refused(at, "only the operator gives a grant"));
    }
    grantable(action).map_err(|m| Problem::refused(at.clone(), m))?;
    let (layer, mut problems) = here_layer(home, root);
    problems.retain(|p| p.level == crate::table::Level::Refused);
    if let Some(p) = problems.pop() {
        return Err(p);
    }
    let (tables, problems) = Tables::load(vec![layer]);
    if let Some(p) = problems.into_iter().find(|p| p.level == crate::table::Level::Refused) {
        return Err(p);
    }
    let quoted = action.target.replace('\\', "\\\\").replace('\'', "\\'");
    let row = toml::toml! {
        on = [(action.verb.word())]
        when = (format!("target == '{quoted}'"))
        verdict = "allow"
        why = "always here"
    };
    let w = crate::table::Write {
        to: Where::User,
        file: Some("gates.toml".into()),
        kind: "gate".into(),
        id: here_id(action),
        op: crate::table::Op::Set(row),
        by: by.clone(),
        cause: None,
        at_ms,
    };
    let planned = tables.plan(&w)?;
    Writer::new(Some(here_dir(home, root)), None).apply(&tables, &planned.change, planned.file_hash.as_deref(), journal)
}
