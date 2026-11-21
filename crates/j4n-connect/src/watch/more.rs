//! The further kinds of source of the watcher: a list of models, a docs page, one tracked issue. They ask the
//! way the feeds do (a conditional request, a 304 writes nothing) and say what changed through the sets and events of the watch.

use super::*;
use crate::net::Reply;

type Stamp = (Option<String>, Option<String>, Option<String>);

/// What each kind of answer is asked to be.
pub(super) fn accept(a: Adapter) -> &'static str {
    match a {
        Adapter::Issue => "application/vnd.github+json",
        Adapter::Models | Adapter::Page => "application/json, text/plain;q=0.9, */*;q=0.1",
        _ => "application/atom+xml, application/xml;q=0.9, application/json;q=0.8, */*;q=0.1",
    }
}

/// `https://github.com/<owner>/<repo>` as the owner and the repository.
pub(super) fn repo(url: &str) -> Option<(String, String)> {
    let mut parts = url.strip_prefix("https://github.com/")?.trim_end_matches('/').split('/');
    let (owner, name) = (parts.next()?, parts.next()?);
    (parts.next().is_none() && !owner.is_empty()).then(|| (owner.into(), name.into()))
}

impl Watcher {
    /// Where a source is asked: its `url`, but an issue is asked of GitHub's REST (the budget's), not of its page.
    pub(super) fn target(&self, s: &Source) -> Option<String> {
        match s.adapter {
            Adapter::Issue => {
                let (owner, name) = repo(s.url.as_deref()?)?;
                Some(format!("{}/repos/{owner}/{name}/issues/{}", self.rest, sets::issue_number(s.watch.as_deref()?)?))
            }
            _ => s.url.clone(),
        }
    }

    /// A REST request within the floor is taken (and counted); false when the floor says wait.
    pub(super) fn rest_slot(&self, host: &str, now: u64) -> bool {
        let mut b = self.budget.lock().unwrap_or_else(|e| e.into_inner());
        let ok = b.rest_ok(host, now);
        if ok {
            b.used(now);
        }
        ok
    }

    /// What GitHub said is left of the hour.
    pub(super) fn rest_left(&self, reply: &Reply) {
        if let Some(r) = reply.headers.get("x-ratelimit-remaining").and_then(|v| v.parse().ok()) {
            self.budget.lock().unwrap_or_else(|e| e.into_inner()).left(r);
        }
    }

    /// Write what a source now remembers: only a first look, or a change of it, reaches the Log. True for a first look.
    pub(super) fn keep(&self, id: &str, old: &Option<Seen>, now_seen: Seen) -> bool {
        let first = old.is_none();
        if old.as_ref() != Some(&now_seen) {
            self.put("watch.seen", now_seen.json(id));
            self.tally(|c| c.changed += 1);
        } else {
            self.tally(|c| c.quiet += 1);
        }
        first
    }

    /// A list of models or a docs page: its set of names as it is now, against the last look's. Models are those of the providers named that
    /// J4n holds, and a change in one is a break; a page's changes are news, never more.
    pub(super) fn looked(&self, s: &Source, old: Option<Seen>, text: &str, (etag, modified, moved): Stamp) -> Option<String> {
        let models = s.adapter == Adapter::Models;
        let set = if models { sets::models(text, &sets::providers(s.watch.as_deref().unwrap_or_default()), self.uses.get(&s.of).unwrap_or(&Uses::default())) } else { sets::lines(text) };
        let Some(set) = set else {
            self.tally(|c| c.failed += 1);
            return Some(format!("{}: the answer is not {}", s.id, if models { "a list of models" } else { "a page" }));
        };
        let changes = self.observe_in(&s.id, &set, if models { Class::Breaks } else { Class::Touches });
        self.keep(&s.id, &old, Seen { etag, modified, moved, ..Seen::default() });
        let n = |w: &str| changes.iter().filter(|c| c.word() == w).count();
        (!changes.is_empty()).then(|| format!("{}: {} gone, {} changed, {} added", s.id, n("gone"), n("changed"), n("added")))
    }

    /// One tracked issue: said when its state changes (closed, or reopened); the first look only remembers it.
    pub(super) fn issue(&self, s: &Source, old: Option<Seen>, text: &str, (etag, modified, moved): Stamp, now: u64) -> Option<String> {
        let Some(i) = sets::issue(text) else {
            self.tally(|c| c.failed += 1);
            return Some(format!("{}: the answer is not an issue", s.id));
        };
        let link = format!("{}/issues/{}", s.url.as_deref().unwrap_or_default().trim_end_matches('/'), i.number);
        let said = old.as_ref().and_then(|o| o.ids.first()).filter(|was| **was != i.state).map(|was| {
            self.put("watch.issue", json!({ "source": s.id, "of": s.of, "number": i.number, "title": i.title, "state": i.state, "was": was, "comments": i.comments, "link": link, "lead": i.title, "at_ms": now }));
            format!("{}: issue #{} is now {} (it was {was})", s.id, i.number, i.state)
        });
        self.keep(&s.id, &old, Seen { etag, modified, ids: vec![i.state], moved, latest: None });
        said
    }
}
