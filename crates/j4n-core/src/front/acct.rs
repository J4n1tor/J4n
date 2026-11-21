//! Which login a turn runs as: a doer may have several, the operator's own and the accounts signed in
//! here. A turn goes to one that is not limited; a chat that waits out a limit goes on at once when another is open, and an account signed in
//! meanwhile is one of them. What a login has said of its limit is the Log's (`Usage::limited_until`); the pick is pure ([`choose`]).

use super::Front;
use super::thread::Binding;
use super::usage::hm;
use crate::change::Actor;
use crate::task::{Ctl, Row, State, TaskSpec};
use serde_json::json;

/// A login a turn may run as: `account` empty is the doer's own.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub account: String,
    /// When it is let in again (ms), if it is shut now.
    pub shut: Option<u64>,
    /// How full its tightest window stands (0 to 1), or 0 when none is known.
    pub load: f64,
}

/// What a pick came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Chosen {
    /// An open login.
    Open(String),
    /// Every login is shut: the one that is let in first, and when (ms).
    Shut(String, u64),
}

/// An open login: the one the chat ran as last while it is open (its session is there), else the doer's own, else the one with the most room.
/// With none open, the one that is let in first, so the chat waits the shortest. None: no login at all.
pub fn choose(cands: &[Candidate], stay: Option<&str>) -> Option<Chosen> {
    let mut open: Vec<&Candidate> = cands.iter().filter(|c| c.shut.is_none()).collect();
    open.sort_by(|a, b| (!a.account.is_empty()).cmp(&!b.account.is_empty()).then(a.load.total_cmp(&b.load)).then(a.account.cmp(&b.account)));
    if let Some(c) = stay.and_then(|s| open.iter().find(|c| c.account == s)).or(open.first()) {
        return Some(Chosen::Open(c.account.clone()));
    }
    cands.iter().filter_map(|c| c.shut.map(|at| (at, &c.account))).min().map(|(at, a)| Chosen::Shut(a.clone(), at))
}

/// How a login is called to the operator.
fn name(account: &str) -> &str {
    if account.is_empty() { "default" } else { account }
}

impl Front {
    /// The logins of `doer` that are signed in here, besides its own: the accounts of the project (a row accepted a moment ago is in it) that
    /// the Log says signed in and have not been removed.
    fn logins_of(&self, doer: &str) -> Vec<String> {
        let rows: Vec<String> = self.accounts_now().into_iter().filter(|(d, _)| d == doer).map(|(_, a)| a).collect();
        let mut u = self.usage();
        u.catch_up(self.log());
        rows.into_iter().filter(|a| u.signed(a).is_some_and(|s| s.ok)).collect()
    }

    /// The doer's own login and each signed-in one, with what is known of each.
    fn candidates(&self, doer: &str, model: &str, logins: &[String], skip: Option<&str>) -> Vec<Candidate> {
        let now = self.log().clock().now_ms();
        let mut u = self.usage();
        u.catch_up(self.log());
        std::iter::once(String::new()).chain(logins.iter().cloned()).filter(|a| Some(a.as_str()) != skip).map(|a| Candidate { shut: u.limited_until(doer, &a, model, now), load: u.tightest(doer, &a, now).unwrap_or(0.0), account: a }).collect()
    }

    /// A chat is pinned to a login by `:switch <doer> account=<name>`; a login a turn was sent to for its limit is not a pin.
    fn login_pin(b: &Binding) -> Option<&str> {
        b.account.as_deref().filter(|a| !a.is_empty() && b.how == "switch")
    }

    /// The binding a turn runs under: the login that is open. Nothing changes for a doer that has no account signed in, nor for a
    /// pinned chat, which says why it waits when its login is shut. A pick that is not where the chat ran last is said, in the chat and in the Log.
    pub(super) fn account_route(&self, thread: &str, b: &Binding) -> Binding {
        self.route_as(thread, b, "work")
    }

    /// [`account_route`](Front::account_route) for a task of another role of the chat (the planner, the reviewer, a branch, a chore, a light
    /// answer): the main chat's own work runs as a login the same way. The task's own account is what it would have run as.
    pub fn login_for(&self, thread: &str, spec: &mut TaskSpec) {
        let Some(b) = self.binding(thread) else { return };
        let how = if Self::login_pin(&b).is_some() && b.account == spec.account { "switch" } else { "routed" };
        let was = Binding { doer: spec.doer.clone(), model: spec.tune.model.clone(), endpoint: None, account: spec.account.clone(), how: how.into(), answer: None, lines: 0, tasks: 0 };
        spec.account = self.route_as(thread, &was, spec.role.as_deref().unwrap_or("work")).account;
    }

    fn route_as(&self, thread: &str, b: &Binding, role: &str) -> Binding {
        let logins = self.logins_of(&b.doer);
        if logins.is_empty() {
            return b.clone();
        }
        let now = self.log().clock().now_ms();
        if let Some(pin) = Self::login_pin(b) {
            let shut = self.usage().limited_until(&b.doer, pin, b.model.as_deref().unwrap_or_default(), now);
            if let Some(at) = shut {
                self.note(thread, &format!("{} is pinned to {pin}, which is limited, back in {}: it waits (`:switch {}` lets any login take it)", thread, hm(at - now), b.doer), "warn");
            }
            return b.clone();
        }
        let last = self.last_task(thread).filter(|r| r.spec.doer == b.doer);
        let from = last.as_ref().map(|r| r.spec.account.clone().unwrap_or_default());
        let cands = self.candidates(&b.doer, b.model.as_deref().unwrap_or_default(), &logins, None);
        let Some(chosen) = choose(&cands, from.as_deref()) else { return b.clone() };
        let (to, waits) = match &chosen {
            Chosen::Open(a) => (a.clone(), None),
            Chosen::Shut(a, at) => (a.clone(), Some(*at)),
        };
        let shut: Vec<String> = cands.iter().filter(|c| c.shut.is_some() && c.account != to).map(|c| name(&c.account).to_string()).collect();
        let was = name(from.as_deref().unwrap_or_default());
        let why = match (waits, shut.is_empty()) {
            (Some(at), _) => format!("every {} login is limited: the first is back in {}", b.doer, hm(at - now)),
            (None, false) => format!("{} limited", shut.join(" and ")),
            (None, true) => format!("{was} is not open"),
        };
        if from.as_deref().is_some_and(|f| f != to) || (from.is_none() && !to.is_empty()) || waits.is_some() {
            let carry = if last.as_ref().is_some_and(|r| r.session.is_some()) && from.as_deref().is_some_and(|f| f != to) { " · a new session" } else { "" };
            self.write(thread, "account.routed", Actor::Core, json!({ "doer": b.doer, "role": role, "from": from.clone().filter(|f| !f.is_empty()), "to": Some(to.clone()).filter(|t| !t.is_empty()), "why": why }));
            self.note(thread, &format!("{} · {}: {why}{carry}", b.doer, name(&to)), if role == "work" { "warn" } else { "dim" });
        }
        Binding { account: Some(to).filter(|a| !a.is_empty()), ..b.clone() }
    }

    /// What waits out a limit in the chat (its turn, its planner, a branch, a chore) goes on at once as a login that is open, when it is not
    /// pinned.
    pub fn limits_step(&self, thread: &str) {
        let cooling: Vec<Row> = self.0.tasks.book().open().into_iter().filter(|r| r.spec.thread.as_deref() == Some(thread) && matches!(r.state, State::Cooling { .. })).collect();
        cooling.iter().for_each(|r| self.move_cooling(thread, r));
    }

    /// Every chat that waits out a limit, as [`limits_step`](Front::limits_step) does one: a limit was said, or an account was signed in.
    pub fn limits_sweep(&self) {
        let mut threads: Vec<String> = self.0.tasks.book().open().into_iter().filter(|r| matches!(r.state, State::Cooling { .. })).filter_map(|r| r.spec.thread).collect();
        threads.sort();
        threads.dedup();
        threads.iter().for_each(|t| self.limits_step(t));
    }

    fn move_cooling(&self, thread: &str, row: &Row) {
        let Some(b) = self.binding(thread).filter(|b| !(b.doer == row.spec.doer && Self::login_pin(b).is_some() && b.account == row.spec.account)) else { return };
        let b = Binding { doer: row.spec.doer.clone(), model: row.spec.tune.model.clone(), ..b };
        let logins = self.logins_of(&b.doer);
        let from = row.spec.account.clone().unwrap_or_default();
        let now = self.log().clock().now_ms();
        let Some(back) = self.usage().limited_until(&b.doer, &from, b.model.as_deref().unwrap_or_default(), now) else { return };
        let cands: Vec<Candidate> = self.candidates(&b.doer, b.model.as_deref().unwrap_or_default(), &logins, Some(&from));
        let Some(Chosen::Open(to)) = choose(&cands, None) else { return };
        let why = format!("{} is limited, back in {}", name(&from), hm(back - now));
        let order = Ctl::Move { account: Some(to.clone()).filter(|t| !t.is_empty()), why: why.clone() };
        match self.0.tasks.ctl(&row.id, order, Actor::Core) {
            Ok(()) => {
                self.write(thread, "account.routed", Actor::Core, json!({ "doer": b.doer, "task": row.id, "from": Some(from).filter(|f| !f.is_empty()), "to": Some(to.clone()).filter(|t| !t.is_empty()), "why": why, "moved": true }));
                self.note(thread, &format!("{} · {}: {why} · a new session", b.doer, name(&to)), "warn");
            }
            Err(p) => self.note(thread, &format!("{} could not go on as {}: {}", row.id, name(&to), p.msg), "dim"),
        }
    }
}
