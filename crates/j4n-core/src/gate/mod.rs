//! The Gate: every side effect is checked once, by a pure function. This file holds the words the
//! rest of the core and the packs share — what is asked ([`Action`]), what is answered ([`Verdict`]) and what may widen an
//! answer ([`Covers`]); the parts are in the modules below.
//!
//! - [`check`]: the rules (the kind `gate`) and `check(gates, grants, action)`; **Block > Ask > Allow**, and a grant, a
//!   mode or a layer never turns a Block into anything else.
//! - [`danger`]: what a shell line is — the matcher the rules read as `danger(cmd)`.
//! - [`grant`]: the operator's yes that lasts (`once · task · session · here`), and the file that keeps *here*.
//! - [`inside`]: the approvals J4n takes itself under `approval.inside`, their effect inside the project; [`pyedit`] reads a doer's Python edit for it.

pub mod check;
pub mod danger;
pub mod door;
pub mod foreign;
pub mod grant;
pub mod inside;
pub mod pyedit;
pub mod pyprobe;
pub mod pyread;
pub mod relay;

use crate::change::Actor;
use crate::schema::DataClass;
use serde::{Deserialize, Serialize};

/// What kind of effect is asked about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verb {
    /// A command line.
    Shell,
    Read,
    Write,
    Net,
    Spend,
    /// Starting a doer or a process of its own.
    Spawn,
    Setting,
    /// Applying a change or a proposal to the world.
    Apply,
    /// Data leaving for a provider: `class` against the zone in `target`.
    Egress,
    /// Connecting a connector, a server or an account.
    Connect,
}

impl Verb {
    pub const ALL: [Verb; 10] =
        [Verb::Shell, Verb::Read, Verb::Write, Verb::Net, Verb::Spend, Verb::Spawn, Verb::Setting, Verb::Apply, Verb::Egress, Verb::Connect];

    pub fn word(self) -> &'static str {
        match self {
            Verb::Shell => "shell",
            Verb::Read => "read",
            Verb::Write => "write",
            Verb::Net => "net",
            Verb::Spend => "spend",
            Verb::Spawn => "spawn",
            Verb::Setting => "setting",
            Verb::Apply => "apply",
            Verb::Egress => "egress",
            Verb::Connect => "connect",
        }
    }

    pub fn from_word(word: &str) -> Option<Verb> {
        Verb::ALL.into_iter().find(|v| v.word() == word)
    }
}

/// Whether the task that asks only reads, and whether it was told to (`/plan`, a review or validate step) or it was only
/// guessed from its words: the first blocks a write, the second asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReadOnly {
    #[default]
    No,
    Explicit,
    Inferred,
}

impl ReadOnly {
    pub fn word(self) -> &'static str {
        match self {
            ReadOnly::No => "no",
            ReadOnly::Explicit => "explicit",
            ReadOnly::Inferred => "inferred",
        }
    }
}

/// One effect, asked about before it is done.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    pub verb: Verb,
    /// Who wants it done.
    pub actor: Actor,
    /// The task it is done for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// What it is done to: the command line (`Shell`), a path (`Read`, `Write`, `Apply`), a host (`Net`), a setting's id,
    /// the provider's zone (`Egress`), a connector's id (`Connect`, `Spawn`).
    pub target: String,
    /// The folder the task works in, when that matters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<String>,
    /// What it costs, in dollars (`Spend`).
    #[serde(default)]
    pub usd: f64,
    #[serde(default)]
    pub read_only: ReadOnly,
    /// For a path: whether it lies inside the project's folder, as the caller found by looking at the real path (the
    /// rules cannot do arithmetic on paths). `None` for what is not a path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inside: Option<bool>,
    /// For `Egress`: the most sensitive class of the data that would leave.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<DataClass>,
    /// Where untrusted content came from, when the task this is done for has read some (the Gate fills it in): `web`,
    /// `mcp:<server>`. A rule reads it as `action.taint`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taint: Option<String>,
    /// The temporary folder the task has of its own, when it has one (the Gate fills it in): a delete inside it is a write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmp: Option<String>,
    /// The folders the operator gave J4n access to (the Gate fills it in from the grants): what a write or a delete may reach besides the
    /// project and the task's tmp.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub access: Vec<String>,
    /// The scratch folders below a system temporary folder that the task's own lines made (the Gate fills it in): inside, like its tmp.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub own: Vec<String>,
    /// Where the doer is, so that another project's state can be told from its own (the Gate fills it in): see [`foreign`].
    #[serde(default, skip_serializing_if = "foreign::Own::is_empty")]
    pub state: foreign::Own,
}

impl Action {
    pub fn new(verb: Verb, actor: Actor, target: &str) -> Action {
        Action { verb, actor, task: None, target: target.to_string(), site: None, usd: 0.0, read_only: ReadOnly::No, inside: None, class: None, taint: None, tmp: None, access: Vec::new(), own: Vec::new(), state: foreign::Own::default() }
    }

    pub fn shell(actor: Actor, cmd: &str) -> Action {
        Action::new(Verb::Shell, actor, cmd)
    }

    pub fn task(mut self, task: &str) -> Action {
        self.task = Some(task.to_string());
        self
    }
}

/// What the Gate says. `by` is the id of the rule (or `grant:<id>`, or `default`) that said it, so a verdict can be traced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "lowercase")]
pub enum Verdict {
    Allow { by: String },
    /// Needs a yes from the operator (or a grant); `risk` is 0–100.
    Ask { risk: u8, why: String, by: String },
    /// Never, whoever asks and whatever was granted.
    Block { why: String, by: String },
}

impl Verdict {
    pub fn word(&self) -> &'static str {
        match self {
            Verdict::Allow { .. } => "allow",
            Verdict::Ask { .. } => "ask",
            Verdict::Block { .. } => "block",
        }
    }

    pub fn by(&self) -> &str {
        match self {
            Verdict::Allow { by } | Verdict::Ask { by, .. } | Verdict::Block { by, .. } => by,
        }
    }
}

/// What can turn an Ask into an Allow: the operator's standing yes. It never touches a Block.
pub trait Covers: Send + Sync {
    /// The id of a grant that covers `action`, if one does.
    fn covers(&self, action: &Action) -> Option<String>;
}

/// No grants at all.
pub struct NoGrants;

impl Covers for NoGrants {
    fn covers(&self, _action: &Action) -> Option<String> {
        None
    }
}

/// Which thread holds what a task read from outside: given a task, its thread when one does.
pub type Holder = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// The Gate as the loop holds it: the rules, and the grants that can turn an ask into a yes. Clones share both.
#[derive(Clone)]
pub struct Gate {
    gates: std::sync::Arc<std::sync::RwLock<check::Gates>>,
    grants: grant::Grants,
    /// The tasks that have read untrusted content, and from where: their actions carry it.
    taints: std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, String>>>,
    /// What a held thread read from outside, by thread: a task of that thread that read untrusted content leaves it here, where the end of the task does not clear it.
    held: std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, String>>>,
    /// Which thread holds what a task read, if any (set once, by the front).
    holds: std::sync::Arc<std::sync::OnceLock<Holder>>,
    /// The temporary folder each task has of its own: their actions carry it.
    tmps: std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, String>>>,
    /// The scratch folders each task's lines made below a system temporary folder.
    owns: std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, Vec<String>>>>,
    /// Where the doers work: the operator's home, J4n's home, the project's root.
    state: foreign::Own,
    /// The programs that a yes let run in this process, by folder: what a `Program` grant is offered for (the second line of a program that was allowed).
    said: std::sync::Arc<std::sync::Mutex<std::collections::BTreeSet<(String, String)>>>,
    /// `approval.inside`: an approval whose effect stays inside the project is taken by code, and a line code cannot read is put to a Decider.
    inside: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Gate {
    pub fn new(gates: check::Gates, grants: grant::Grants) -> Gate {
        Gate { gates: std::sync::Arc::new(std::sync::RwLock::new(gates)), grants, taints: Default::default(), held: Default::default(), holds: Default::default(), tmps: Default::default(), owns: Default::default(), state: foreign::Own::default(), said: Default::default(), inside: Default::default() }
    }

    /// Tell the Gate where the doers work, so that a read of another project's state is told from a read of their own.
    pub fn with_state(mut self, state: foreign::Own) -> Gate {
        self.state = state;
        self
    }

    pub fn check(&self, action: &Action) -> Verdict {
        let gates = self.gates.read().unwrap_or_else(|e| e.into_inner());
        let told = self.told(action);
        let verdict = check::check(&gates, &self.grants, &told);
        // a rule that allows the line lets it run: what it makes is the task's. An ask is registered when it is answered yes (`keep`), a refusal never
        if matches!(&verdict, Verdict::Allow { by } if by != check::DEFAULT) {
            self.keep_made(&told);
        }
        verdict
    }

    /// The line `action` runs (a rule allowed it, or a yes answered it): a scratch folder it makes below a system temporary folder (`mkdir /tmp/x` of a first
    /// folder that is not there) becomes the task's own, so a later line that writes or deletes in it is inside.
    pub fn keep(&self, action: &Action) {
        let told = self.told(action);
        self.keep_made(&told);
        if let (Some(site), Ok(program)) = (told.site.clone(), grant::program_of(&told)) {
            self.said.lock().unwrap_or_else(|e| e.into_inner()).insert((site, program));
        }
    }

    /// The id of a grant that lets `action` go on, when no rule has a say about it (nothing allows it, nothing asks) or a grant turned a rule's ask
    /// into a yes: what the Gate's answerer says of a line to the operator's yes that lasts. `None` for what a rule blocks or asks.
    pub fn granted(&self, action: &Action) -> Option<String> {
        match self.check(action) {
            Verdict::Allow { by } if by == check::DEFAULT => self.grants.reaches(&self.told(action)),
            Verdict::Allow { by } => by.strip_prefix("grant:").map(str::to_string),
            _ => None,
        }
    }

    /// The scopes the operator may be offered for an approval whose verdict is `verdict` ([`grant::offered`]), and two more for this session when
    /// the action has them: `folder` for a write inside the project, `program` for a shell line whose program a yes already let run here.
    pub fn offers(&self, action: &Action, verdict: &Verdict) -> Vec<grant::Scope> {
        let mut scopes = grant::offered(action, verdict);
        if scopes.is_empty() {
            return scopes;
        }
        let told = self.told(action);
        match told.verb {
            Verb::Write if told.inside == Some(true) && grant::grantable(&told).is_ok() && grant::folder_of(&told.target).is_some() => scopes.push(grant::Scope::Folder),
            Verb::Shell => {
                let said = self.said.lock().unwrap_or_else(|e| e.into_inner());
                if let (Some(site), Ok(program)) = (told.site.clone(), grant::program_of(&told))
                    && said.contains(&(site, program))
                {
                    scopes.push(grant::Scope::Program);
                }
            }
            _ => {}
        }
        scopes
    }

    fn keep_made(&self, told: &Action) {
        let (Some(task), Some(site), Verb::Shell) = (told.task.as_deref(), told.site.as_ref(), told.verb) else { return };
        let scope = danger::Scope { site: Some(site.clone()), tmp: told.tmp.clone(), access: told.access.clone(), own: told.own.clone(), state: told.state.clone() };
        for root in danger::danger_scoped(&told.target, &scope).made {
            self.add_own(task, &root);
        }
    }

    /// `root`, a folder (or a prefix ending in `*`) below a system temporary folder, is the task's own.
    pub fn add_own(&self, task: &str, root: &str) {
        let mut all = self.owns.lock().unwrap_or_else(|e| e.into_inner());
        let mine = all.entry(task.to_string()).or_default();
        if !mine.iter().any(|r| r == root) {
            mine.push(root.to_string());
        }
    }

    /// The scratch folders `task`'s lines made.
    pub fn owns_of(&self, task: &str) -> Vec<String> {
        self.owns.lock().unwrap_or_else(|e| e.into_inner()).get(task).cloned().unwrap_or_default()
    }

    /// `action` as the Gate reads it: with what the task read (`taint`), its own temporary folder, the folders the operator gave access to, and, for a
    /// write to a path inside the project's reach by those, `inside`.
    pub fn told(&self, action: &Action) -> Action {
        let task = action.task.as_deref();
        let mut told = action.clone();
        if told.taint.is_none() {
            told.taint = task.and_then(|t| self.tainted(t));
        }
        if told.tmp.is_none() {
            told.tmp = task.and_then(|t| self.own_tmp(t));
        }
        if told.access.is_empty() {
            told.access = self.grants.access(task);
        }
        if told.own.is_empty() {
            told.own = task.map(|t| self.owns_of(t)).unwrap_or_default();
        }
        if told.state.is_empty() {
            told.state = self.state.clone();
        }
        // a path under the task's tmp, a granted folder or an own folder is inside when its REAL path (links followed, at the longest part that exists) is under
        // the folder too. The caller looked at the project only, so its "not inside" is for the project: the words alone never turn it into
        // a yes, a link out of the folder keeps it outside
        if matches!(told.verb, Verb::Write | Verb::Apply) && told.inside != Some(true) {
            let dots = told.target.split('/').any(|c| c == "..");
            let text = |root: &str| root.starts_with('/') && (told.target == root || told.target.starts_with(&format!("{}/", root.trim_end_matches('/'))));
            let roots: Vec<&String> = told.tmp.iter().chain(&told.access).chain(&told.own).filter(|r| text(r)).collect();
            // by its words it is below one of them: by its real path it is or it is not (a link out of the folder is outside, never unknown)
            if !roots.is_empty() {
                told.inside = Some(!dots && roots.iter().any(|r| real_under(r, &told.target)));
            }
        }
        told
    }

    /// Does this action write or delete outside the project, the task's tmp and what was granted? `--approval full` does not answer an
    /// approval for it: the operator, or a grant, does. A write to a path the caller found outside, and a shell line the matcher finds outside.
    pub fn outside(&self, action: &Action) -> bool {
        let told = self.told(action);
        match told.verb {
            Verb::Write | Verb::Apply => told.inside == Some(false),
            Verb::Shell => danger::danger_scoped(&told.target, &danger::Scope { site: told.site.clone(), tmp: told.tmp.clone(), access: told.access.clone(), own: told.own.clone(), state: told.state.clone() }).outside,
            _ => false,
        }
    }

    /// `task` has a temporary folder of its own, `path` (absolute): a `rm` of what is inside it, and not of it, is a write and not a delete.
    pub fn set_tmp(&self, task: &str, path: &str) {
        self.tmps.lock().unwrap_or_else(|e| e.into_inner()).insert(task.to_string(), path.to_string());
    }

    /// The temporary folder `task` has of its own, if it has one.
    pub fn own_tmp(&self, task: &str) -> Option<String> {
        self.tmps.lock().unwrap_or_else(|e| e.into_inner()).get(task).cloned()
    }

    /// The task is over: forget its temporary folder.
    pub fn forget_tmp(&self, task: &str) {
        self.owns.lock().unwrap_or_else(|e| e.into_inner()).remove(task);
        self.tmps.lock().unwrap_or_else(|e| e.into_inner()).remove(task);
    }

    /// `task` has read untrusted content from `from`; true the first time, false when it had already (the first source stays).
    pub fn taint(&self, task: &str, from: &str) -> bool {
        let mut all = self.taints.lock().unwrap_or_else(|e| e.into_inner());
        let new = !all.contains_key(task);
        all.entry(task.to_string()).or_insert_with(|| from.to_string());
        drop(all);
        // a thread that is held keeps it past the end of the task: what an envoy read in one turn is in what it sends in another
        if let Some(thread) = self.holds.get().and_then(|f| f(task)) {
            self.held.lock().unwrap_or_else(|e| e.into_inner()).entry(thread).or_insert_with(|| from.to_string());
        }
        new
    }

    /// Say which thread holds what its tasks read, given a task: set once.
    pub fn hold_by(&self, f: Holder) {
        let _ = self.holds.set(f);
    }

    /// What `thread` has read from outside in any of its turns, if it has.
    pub fn held(&self, thread: &str) -> Option<String> {
        self.held.lock().unwrap_or_else(|e| e.into_inner()).get(thread).cloned()
    }

    /// Where `task` read untrusted content, if it has.
    pub fn tainted(&self, task: &str) -> Option<String> {
        self.taints.lock().unwrap_or_else(|e| e.into_inner()).get(task).cloned()
    }

    /// The task is over: forget what it read.
    pub fn untaint(&self, task: &str) {
        self.taints.lock().unwrap_or_else(|e| e.into_inner()).remove(task);
    }

    pub fn grants(&self) -> &grant::Grants {
        &self.grants
    }

    /// Turn `approval.inside` on or off (the settings say; a run that denies every ask keeps it off).
    pub fn set_inside(&self, on: bool) {
        self.inside.store(on, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn inside_on(&self) -> bool {
        self.inside.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// What `approval.inside` makes of `action` ([`inside::read`], with every rule that applied to it): `None` while the switch is off.
    pub fn inside(&self, action: &Action) -> Option<inside::Inside> {
        if !self.inside_on() {
            return None;
        }
        let told = self.told(action);
        let hits = check::explain(&self.gates.read().unwrap_or_else(|e| e.into_inner()), &self.grants, &told);
        Some(inside::read(&told, &hits))
    }

    /// Put new rules in place (after the operator's file or a layer changed).
    pub fn replace(&self, gates: check::Gates) {
        *self.gates.write().unwrap_or_else(|e| e.into_inner()) = gates;
    }
}

/// Is `target` at or below `root`, once links are followed? Each path is resolved at the longest part that exists and the rest is appended as the words give it;
/// a path whose existing part cannot be resolved is not under anything.
pub(super) fn real_under(root: &str, target: &str) -> bool {
    fn real(p: &str) -> Option<std::path::PathBuf> {
        let mut rest = Vec::new();
        let mut at = std::path::Path::new(p);
        loop {
            if let Ok(real) = at.canonicalize() {
                return Some(rest.iter().rev().fold(real, |acc, c| acc.join(c)));
            }
            rest.push(at.file_name()?.to_os_string());
            at = at.parent()?;
        }
    }
    match (real(root), real(target)) {
        (Some(r), Some(t)) => t.starts_with(&r),
        _ => false,
    }
}

/// `path` as the rows read it: `.` and empty parts dropped, a `..` taken with the part before it, by the text alone (no file is looked
/// at, a link is not followed). A `..` with nothing before it is kept, except at the root; `~` is never taken by one.
pub fn clean_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|p| *p != ".." && !(parts.len() == 1 && *p == "~")) => {
                parts.pop();
            }
            ".." if path.starts_with('/') => {}
            _ => parts.push(part),
        }
    }
    let joined = parts.join("/");
    if path.starts_with('/') { format!("/{joined}") } else if joined.is_empty() { ".".into() } else { joined }
}
