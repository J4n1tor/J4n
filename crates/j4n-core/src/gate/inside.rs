//! The approvals J4n takes itself (`approval.inside`): under the switch, an approval whose effect stays inside the project
//! (its folder, a chat's worktree, the task's own temporary folder) is said yes to by code, as the Gate's answer (`rule:gate`); a line code
//! cannot read for itself is put to a Decider on the point `inside`, which may say *inside, harmless* or *ask*.
//!
//! [`read`] says which of the three a line is:
//!
//! - [`Inside::No`]: the operator is asked. A rule had a say of its own (a block; an ask other than `unreadable-line-asks` for text inside a here-document),
//!   the task only reads or read from outside, a remote shell, a write or a delete outside the project, a class other than `safe`, `read` and `write`
//!   (a push, a secret, J4n's own files, a discard, an install …), a delete that is not of build folders, a command that is not read as staying inside
//!   (`danger::Reach::loose`: a deploy script, the network, a program the matcher does not know), Python that [`super::pyedit`] asks about.
//! - [`Inside::Unsure`]: everything is read as inside but the paths of some Python, built while it runs. Only this reaches a Decider.
//! - [`Inside::Yes`]: an edit, a test run, a linter, a check script of the project, readers.
//!
//! A Decider's yes stands only while [`read`] still says `Unsure` for the line: the engine reads it again on the answer.

use super::check::{Hit, Says};
use super::danger::{Remote, Scope, danger_scoped};
use super::pyedit::{self, Edit};
use super::{Action, ReadOnly, Verb};

/// The id of the rule whose ask the switch may answer: only for a line that is unreadable for text inside a here-document of Python.
pub const UNREADABLE: &str = "unreadable-line-asks";
/// The id of the rule that asks for every delete: the switch takes a remove of build folders over it.
pub const DELETES: &str = "delete-asks";
/// The ask point a Decider answers for a line code cannot read.
pub const POINT: &str = "inside";
/// The longest line put to a Decider; a longer one is the operator's.
pub const LINE_MAX: usize = 12_000;

/// What `approval.inside` makes of an action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inside {
    /// Taken by code: what it is, in a few words.
    Yes(String),
    /// Put to a Decider: what code could not read.
    Unsure(String),
    /// Asked of the operator: why.
    No(String),
}

impl Inside {
    pub fn word(&self) -> &'static str {
        match self {
            Inside::Yes(_) => "inside",
            Inside::Unsure(_) => "unsure",
            Inside::No(_) => "ask",
        }
    }

    pub fn why(&self) -> &str {
        match self {
            Inside::Yes(w) | Inside::Unsure(w) | Inside::No(w) => w,
        }
    }
}

/// What `approval.inside` makes of `told` (the action as the Gate tells it), given the rules that applied to it (`check::explain`).
pub fn read(told: &Action, hits: &[Hit]) -> Inside {
    let no = |why: String| Inside::No(why);
    if let Some(h) = hits.iter().find(|h| h.says == Says::Block) {
        return no(format!("the rule `{}` blocks it", h.rule));
    }
    if told.read_only != ReadOnly::No {
        return no("the task only reads".into());
    }
    if let Some(from) = &told.taint {
        return no(format!("the task read from outside ({from})"));
    }
    match told.verb {
        Verb::Write | Verb::Apply => {
            if let Some(h) = hits.iter().find(|h| h.says == Says::Ask) {
                return no(format!("the rule `{}` asks", h.rule));
            }
            let t = &told.target;
            let named = t.split('/').any(|c| c.eq_ignore_ascii_case(".git") || c.eq_ignore_ascii_case(".j4n") || c == "..");
            if told.inside != Some(true) || named || super::danger::danger(&format!("cat {}", quoted(t))).class == "secret" {
                return no("the file is not inside the project".into());
            }
            Inside::Yes(format!("an edit of {t}"))
        }
        Verb::Shell => shell(told, hits),
        v => no(format!("a `{}` is asked about", v.word())),
    }
}

fn quoted(t: &str) -> String {
    format!("'{}'", t.replace('\'', ""))
}

fn shell(told: &Action, hits: &[Hit]) -> Inside {
    let no = |why: String| Inside::No(why);
    let Some(site) = told.site.as_deref() else { return no("the line has no project folder".into()) };
    let scope = Scope { site: Some(site.to_string()), tmp: told.tmp.clone(), access: told.access.clone(), own: told.own.clone(), state: told.state.clone() };
    let d = danger_scoped(&told.target, &scope);
    // the rule that asks for every delete has its say only over what is not a build folder, which is read below
    if let Some(h) = hits.iter().find(|h| h.says == Says::Ask && !(h.rule == UNREADABLE && d.reach.odd_in_docs) && !(h.rule == DELETES && !d.reach.deletes)) {
        return no(format!("the rule `{}` asks", h.rule));
    }
    if d.remote != Remote::No {
        return no("it runs a remote shell".into());
    }
    if d.outside {
        return no("it writes or deletes outside the project".into());
    }
    let allowed = |c: &str| matches!(c, "safe" | "read" | "write") || (c == "unknown" && d.reach.odd_in_docs) || (c == "delete" && !d.reach.deletes);
    if let Some(c) = d.classes.iter().find(|c| !allowed(c)) {
        return no(format!("a `{c}` line"));
    }
    if d.reach.deletes {
        return no("it deletes what is not a build folder".into());
    }
    if !d.reach.loose.is_empty() {
        return no(format!("J4n does not read `{}` as staying inside", d.reach.loose.join("`, `")));
    }
    let mut unsure = None;
    for code in &d.reach.code {
        match pyedit::read(code, Some(site)) {
            Edit::Asks(why) => return no(why),
            Edit::Unsure(why) => {
                unsure.get_or_insert(why);
            }
            Edit::Inside => {}
        }
    }
    match unsure {
        Some(why) => Inside::Unsure(why),
        None if d.reach.code.is_empty() => Inside::Yes("readers, test runs and edits inside the project".into()),
        None => Inside::Yes("Python that edits files inside the project by their names, and readers".into()),
    }
}

/// The question a Decider is put for a line [`read`] is unsure of: `None` when the line is too long to put.
pub fn question(told: &Action, why: &str) -> Option<String> {
    let site = told.site.as_deref().unwrap_or("the project folder");
    (told.target.chars().count() <= LINE_MAX).then(|| {
        format!(
            "A coding agent asks to run the shell line below in the project folder {site}. J4n read all of it but this: {why}. Answer `inside` only if \
             every file the line reads or writes is inside that folder and it does nothing else: no network, no other program, no credentials, nothing \
             deleted. If you are not sure, answer `ask`, and the operator is asked.\n\n{}",
            told.target
        )
    })
}
