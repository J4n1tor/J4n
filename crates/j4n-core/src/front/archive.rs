//! The archive of the chats, when `chats.on` is on. It tidies the list and nothing else: the Log is a chain and keeps every row.
//!
//! - **The fold is the only state**: `chat.archived`, `chat.unarchived` and `chat.removed` (`thread.rs`). A line typed in an archived chat, or a turn begun
//!   in it, brings it back; the chooser that routes a line never offers it (`chats.rs`).
//! - **By rules, no model**: `:archive` / `/archive` in the orchestrator archives every chat whose turns are over and ok, with nothing queued, cooling or
//!   waiting, no open plan item and no word for `chats.archive.idle`; one line says which, then it is done. `:archive <chat>` names one, `:unarchive <chat>`
//!   takes it out. A chat that runs, cools, waits or is queued is not archived, and the orchestrator never is.
//! - **By itself** (`chats.archive.auto`): a chat whose plan is done with every item shown, not merely said, to hold (no `unproven`) is archived once it
//!   has been done for as long as the list waits before it folds it.
//! - **Removed after a while** (`chats.archive.keep`, `0s` is never): the chat is gone from every list, `chat.removed` says what was deleted, and the
//!   doer's session files of the chat go, found by the session ids the Log holds for its turns. The Log's own rows stay.

use super::chats::FOLD_AFTER_MS;
use super::thread::ChatRow;
use super::{Front, Said};
use crate::change::Actor;
use crate::task::{HUMAN, State};
use serde_json::{Value as Json, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// What the front is set to do (`chats.archive.*`); the default does nothing by itself.
#[derive(Clone, Default)]
pub struct Archive {
    pub auto: bool,
    /// How long a chat is untouched before `/archive` takes it.
    pub idle_ms: u64,
    /// How long an archived chat is kept before it is removed; 0 keeps it for ever.
    pub keep_ms: u64,
}

/// Where a login keeps its doers' sessions, `(the claude config folder, the home the other doers use)`, for an account (empty: the operator's own).
pub type Homes = Arc<dyn Fn(&str) -> (PathBuf, PathBuf) + Send + Sync>;

/// The most deleted paths the Log row of a removal names.
const NAMED: usize = 40;

impl Front {
    /// Why a chat is not archived now, in a line; none when it may be.
    fn archive_blocker(&self, c: &ChatRow) -> Option<String> {
        if c.parent.is_some() {
            return Some(format!("{} is a branch: archive the chat it is under", c.id));
        }
        if c.archived_ms.is_some() {
            return Some(format!("{} is archived already", c.id));
        }
        let branches: Vec<String> = self.0.binds.chats().into_iter().filter(|b| b.parent.as_deref() == Some(c.id.as_str())).filter_map(|b| b.task).collect();
        let open = self.0.tasks.book().rows().into_iter().find(|r| !r.state.is_terminal() && (r.spec.thread.as_deref() == Some(c.id.as_str()) || branches.contains(&r.id)));
        let how = open.map(|r| match r.state {
            State::Waiting { .. } => "waits for you",
            State::Cooling { .. } => "cools: its host is at a limit",
            State::Queued => "has a turn queued",
            _ => "is running",
        });
        let mail = self.0.binds.mail().iter().any(|m| m.to == c.id && m.state != "delivered");
        how.or(mail.then_some("has a message that waits for it")).map(|w| format!("{} {w}: nothing is archived while it does", c.id))
    }

    /// Whether a chat's plan is done and each of its items was shown to hold; a chat with no plan has no such proof (J4n records none for a plain turn).
    fn verified(&self, id: &str) -> bool {
        self.0.keeper.as_ref().and_then(|k| k.plan(id)).is_some_and(|p| !p.items.is_empty() && p.items.iter().all(|i| i.done() && !i.unproven))
    }

    /// The newest turn of the chat, if it has one, ended ok.
    fn last_ok(&self, c: &ChatRow) -> bool {
        self.0.tasks.book().rows().iter().rfind(|r| r.spec.thread.as_deref() == Some(c.id.as_str()) && r.spec.doer != HUMAN && r.spec.parent.is_none()).is_none_or(|r| matches!(r.state, State::Done { ok: true }))
    }

    /// The top chats that are not archived and may be, by the rules alone.
    fn archive_picks(&self, idle_ms: u64) -> Vec<String> {
        let now = self.0.log.clock().now_ms();
        let open = |id: &str| self.0.keeper.as_ref().and_then(|k| k.plan(id)).is_some_and(|p| p.items.iter().any(|i| !i.done()));
        let tops = self.0.binds.chats().into_iter().filter(|c| c.parent.is_none());
        tops.filter(|c| self.archive_blocker(c).is_none() && self.last_ok(c) && !open(&c.id) && now.saturating_sub(c.touched_ms) >= idle_ms).map(|c| c.id).collect()
    }

    fn archive_chat(&self, id: &str, by: &str, why: &str) -> Result<(), String> {
        if id == self.0.cfg().thread {
            return Err("the main chat is never archived".into());
        }
        let c = self.chat(id).ok_or_else(|| format!("there is no chat `{id}`"))?;
        if let Some(why) = self.archive_blocker(&c) {
            return Err(why);
        }
        self.write(id, "chat.archived", if by == "you" { Actor::Operator } else { Actor::Core }, json!({ "by": by, "why": why }));
        Ok(())
    }

    /// `:archive [<chat>]` and `/archive`: a named chat; in a chat, that chat; in the orchestrator with no name, the chats the rules pick.
    pub(super) fn archive_said(&self, thread: &str, args: &[String]) -> Said {
        let top = self.0.cfg().thread.clone();
        let said = Said::Command("archive".into());
        let Some(id) = args.first().cloned().or_else(|| (thread != top).then(|| thread.to_string())) else {
            let idle = self.0.cfg().chats.archive.idle_ms;
            let picks = self.archive_picks(idle);
            if picks.is_empty() {
                self.note(&top, &format!("nothing to archive: a chat needs its turns over and ok, nothing open in its plan, and {} untouched", crate::fmt::duration(idle)), "dim");
                return said;
            }
            self.note(&top, &format!("archiving {} · over, ok and untouched for {}", picks.join(" · "), crate::fmt::duration(idle)), "dim");
            picks.iter().for_each(|p| drop(self.archive_chat(p, "rules", "its turns are over and ok, nothing is open, and it is idle")));
            return said;
        };
        match self.archive_chat(&id, "you", "you asked for it") {
            Ok(()) => {
                self.note(&top, &format!("{id} archived · `:unarchive {id}` brings it back"), "dim");
                if id == thread { Said::Enter(String::new()) } else { said }
            }
            Err(why) => self.refuse(thread, why),
        }
    }

    /// `:unarchive <chat>`: in an archived chat, itself.
    pub(super) fn unarchive_said(&self, thread: &str, args: &[String]) -> Said {
        let id = args.first().map_or(thread, String::as_str);
        match self.chat(id) {
            Some(c) if c.archived_ms.is_some() => {
                self.write(id, "chat.unarchived", Actor::Operator, json!({}));
                self.note(&self.0.cfg().thread, &format!("{id} is back in the list"), "dim");
                Said::Command("unarchive".into())
            }
            Some(_) => self.refuse(thread, format!("{id} is not archived")),
            None => self.refuse(thread, format!("there is no chat `{id}`")),
        }
    }

    /// On J4n's own clock (the screen's tick): a chat that was shown done is archived, an archived chat kept long enough is removed.
    pub fn archive_sweep(&self) {
        let cfg = self.0.cfg();
        if !self.chats_on() {
            return;
        }
        let now = self.0.log.clock().now_ms();
        let tops: Vec<ChatRow> = self.0.binds.chats().into_iter().filter(|c| c.parent.is_none()).collect();
        for c in tops.iter().filter(|c| cfg.chats.archive.auto && c.archived_ms.is_none() && now.saturating_sub(c.touched_ms) >= FOLD_AFTER_MS && self.last_ok(c) && self.verified(&c.id)) {
            drop(self.archive_chat(&c.id, "auto", "its plan is done and every item was shown to hold"));
        }
        let keep = cfg.chats.archive.keep_ms;
        for c in tops.iter().filter(|c| keep > 0 && c.archived_ms.is_some_and(|t| now.saturating_sub(t) >= keep)) {
            self.remove_chat(c, &cfg);
        }
    }

    /// The chat and its branches are gone, and the session files of its turns with them.
    fn remove_chat(&self, c: &ChatRow, cfg: &super::Cfg) {
        let mut seen: Vec<(String, String, String)> = Vec::new();
        for r in self.0.tasks.book().rows().iter().filter(|r| r.spec.thread.as_deref() == Some(c.id.as_str())) {
            let one = r.session.as_ref().map(|s| (s.doer.clone(), s.id.clone(), r.spec.account.clone().unwrap_or_default()));
            seen.extend(one.filter(|o| !seen.contains(o)));
        }
        let mut gone: Vec<String> = Vec::new();
        for (doer, id, account) in &seen {
            let safe = |s: &str| s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) && !s.starts_with('.');
            let Some(homes) = cfg.homes.as_ref().filter(|_| !id.is_empty() && safe(id) && safe(account)) else { continue };
            let (claude, home) = homes(account);
            for path in session_files(&claude, &home, doer, id) {
                if (if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) }).is_ok() {
                    gone.push(path.display().to_string());
                }
            }
        }
        let sessions: Vec<Json> = seen.iter().map(|(d, i, _)| json!({ "doer": d, "id": i })).collect();
        let deleted = gone.len();
        gone.truncate(NAMED);
        let branches = self.0.binds.chats().into_iter().filter(|b| b.parent.as_deref() == Some(c.id.as_str()));
        branches.for_each(|b| _ = self.write(&b.id, "chat.removed", Actor::Core, json!({ "with": c.id })));
        self.write(&c.id, "chat.removed", Actor::Core, json!({ "title": c.title, "sessions": sessions, "deleted": deleted, "paths": gone }));
        self.note(&cfg.thread, &format!("{} removed after {} in the archive · {deleted} session file(s) deleted", c.id, crate::fmt::duration(cfg.chats.archive.keep_ms)), "dim");
    }
}

/// The files and folders a doer keeps for one session, by the id only: a name that is the id (or ends `_<id>`), at the depth the doer files them.
fn session_files(claude: &Path, home: &Path, doer: &str, id: &str) -> Vec<PathBuf> {
    let kids = |d: &Path| std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path()).collect::<Vec<_>>();
    // `<base>/<project>/<name>` where the name is one of these (pi puts a time before the id, `<time>_<id>.jsonl`)
    let under = |base: PathBuf, names: &[String], tail: bool| -> Vec<PathBuf> {
        kids(&base).iter().flat_map(|p| kids(p)).filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| names.iter().any(|w| n == w || tail && n.ends_with(&format!("_{w}"))))).collect()
    };
    match doer {
        "claude" => under(claude.join("projects"), &[format!("{id}.jsonl"), id.to_string()], false),
        "pi" | "omp" => [".pi", ".omp"].iter().flat_map(|d| under(home.join(d).join("agent/sessions"), &[format!("{id}.jsonl")], true)).collect(),
        "opencode" => {
            let store = home.join(".local/share/opencode/storage");
            under(store.join("session"), &[format!("{id}.json")], false).into_iter().chain(Some(store.join("message").join(id)).filter(|p| p.is_dir())).collect()
        }
        _ => Vec::new(),
    }
}
