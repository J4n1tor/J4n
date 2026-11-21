//! The skills of a task: which the library gives a line (`skill::Library::pick`, local, no model), how that is
//! said (a dim row of the chat and `brief.skills` in the Log, one for each task), and `:skill` (list, search, show, on, off).

use super::{Front, Said};
use crate::change::{Actor, Change};
use crate::skill::Ask;
use crate::task::{Mode, TaskSpec};
use crate::value::Value;
use serde_json::{Value as Json, json};

/// What a pick is held to when the settings say nothing: a few skills, and not many tokens.
const MAX: f64 = 3.0;
pub(super) const TOKENS: f64 = 1200.0;

/// How the picked skills reach the doer: in its Brief, as a folder of its own, or (a doer with neither) as the best one's words after its goal.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Way { Brief, Folder, Words }

impl Front {
    pub(super) fn skill_set(&self, id: &str) -> Value {
        self.screen().set.pointer(&format!("/skills/{id}"))
    }

    /// The skills picked for `line`, as the section of a Brief (`None` when none fit, or the library or its switch is off). The task is told which
    /// (`input.skills`: for a doer that reads a folder of its own, with their words too), the Log says so (`brief.skills`) and so does the chat.
    pub(super) fn skills_pick(&self, thread: &str, spec: &mut TaskSpec, line: &str, in_session: bool, way: Way, jev: Option<&std::cell::RefCell<String>>) -> Option<String> {
        let (folder, one) = (way == Way::Folder, way == Way::Words);
        let cfg_ = self.0.cfg();
        let lib = cfg_.skills.as_ref()?;
        let num = |id: &str, or: f64| match self.skill_set(id) {
            Value::Num(n) => n,
            _ => or,
        };
        if matches!(self.skill_set("pick"), Value::Bool(false)) {
            return None;
        }
        fn word(w: &str) -> &str {
            w.trim_matches(|c: char| !c.is_alphanumeric() && !matches!(c, '/' | '.' | '*' | '_' | '-'))
        }
        let mut files: Vec<String> = line.split_whitespace().map(word).filter(|w| w.contains(['.', '/']) && w.len() > 2).map(String::from).collect();
        files.extend(spec.input.pointer("/branch/paths").and_then(Json::as_array).into_iter().flatten().filter_map(Json::as_str).map(String::from));
        let kinds = [if spec.tune.mode == Mode::Read { "read" } else { "write" }.to_string()].into_iter().chain(spec.role.clone()).collect();
        let (max, cap) = ((num("max", MAX) as usize).min(if one { 1 } else { usize::MAX }), num("tokens", TOKENS) as usize);
        let ask = Ask { line, files, kinds, in_session, max, tokens: cap };
        // where skills fit equally and there is room for fewer, Jev may choose which; with the librarian off, code's order is all there is
        let fresh = match jev {
            Some(how) => lib.pick_by(&ask, &mut |group| self.library_choose(thread, line, group, how)),
            None => lib.pick(&ask),
        };
        // what the session has already: a doer that reads a folder keeps the skills its child was started with, and a child is shared only by tasks of the same set, so
        // the set only grows. A doer with a Brief is given what fits this line and nothing else ("continue" picks none, and the turn path stays lean)
        let prev: Vec<String> = spec.follows.as_deref().and_then(|f| self.0.tasks.book().get(f)).map(|p| p.spec.input.get("skills").and_then(Json::as_array).into_iter().flatten().filter_map(|s| s.get("id").and_then(Json::as_str).map(String::from)).collect()).unwrap_or_default();
        let mut picked = fresh.clone();
        if in_session && folder {
            for s in prev.iter().filter(|id| fresh.iter().all(|p| &p.id != *id) && lib.is_on(id)).filter_map(|id| lib.get(id)) {
                if picked.len() < max && picked.iter().map(|p| p.tokens).sum::<usize>() + s.cost <= cap {
                    picked.push(crate::skill::Picked { id: s.id.clone(), why: "kept from the session".into(), tokens: s.cost });
                }
            }
        }
        if picked.is_empty() {
            return None;
        }
        let skills: Vec<_> = picked.iter().filter_map(|p| lib.get(&p.id).map(|s| (p, s))).collect();
        spec.input["skills"] = Json::Array(skills.iter().map(|(p, s)| if folder { json!({ "id": p.id, "why": p.why, "tokens": p.tokens, "when": s.when, "description": s.description, "body": s.body }) } else { json!({ "id": p.id, "why": p.why, "tokens": p.tokens }) }).collect());
        // nothing new for a doer that is given a folder: the session keeps what it had, and nothing is said
        if fresh.is_empty() && folder {
            return None;
        }
        let tokens: usize = picked.iter().map(|p| p.tokens).sum();
        let said = picked.iter().map(|p| format!("{} ({})", p.id, p.why)).collect::<Vec<_>>().join(" · ");
        let on = lib.all().iter().filter(|s| lib.is_on(&s.id)).count();
        let text = format!("skills · {said} · {} of {on} · ~{tokens} tokens{}", picked.len(), if folder { " · as its own folder" } else { "" });
        self.write(thread, "brief.skills", Actor::Core, json!({ "doer": spec.doer, "skills": picked.iter().map(|p| json!({ "id": p.id, "why": p.why, "tokens": p.tokens })).collect::<Vec<_>>(), "tokens": tokens, "line": text }));
        self.note(thread, &text, "dim");
        Some(skills.iter().map(|(p, s)| format!("### {}\n{}", p.id, s.body)).collect::<Vec<_>>().join("\n\n"))
    }

    /// `:skill` lists the skills; `:skill tests` searches; `:skill show <id>` reads one; `:skill on|off <id>` switches one (kept in the operator's folder like `:lanes`).
    pub(super) fn skill_said(&self, thread: &str, args: &[String]) -> Said {
        let said = Said::Command("skill".into());
        let Some(lib) = &self.0.cfg().skills else { return self.pack_off(thread, "skills") };
        let line = |s: &crate::skill::Skill| format!("{} {}  {}  {} · ~{} tokens · {}", s.id, if lib.is_on(&s.id) { "on" } else { "off" }, s.from, s.about(), s.cost, s.when);
        match args.first().map(String::as_str) {
            Some(w @ ("on" | "off")) => {
                let Some(id) = args.get(1) else { return self.refuse(thread, format!("`:skill {w} <name>`: the names are in `:skill`")) };
                let off = match lib.switch(id, w == "on") {
                    Ok(off) => off,
                    Err(why) => return self.refuse(thread, why),
                };
                let kept = self.skill_keep(&off);
                self.write(thread, "skill.set", Actor::Operator, json!({ "id": id, "on": w == "on" }));
                self.note(thread, &format!("skill {id} is {w}{}", kept.map_or_else(|| " until J4n ends".into(), |c| format!(" (`:undo {c}` takes it back)"))), "dim");
            }
            Some("show") => match args.get(1).and_then(|id| lib.get(id)) {
                Some(s) => self.note(thread, &format!("{}\n{}\n\n{}", line(s), s.from, s.body), "dim"),
                None => return self.refuse(thread, "`:skill show <name>`: the names are in `:skill`".into()),
            },
            _ => {
                let found = if args.is_empty() { lib.all().iter().collect() } else { lib.search(&args.join(" ")) };
                let text = if found.is_empty() { format!("no skill matches `{}`", args.join(" ")) } else { found.iter().map(|s| line(s)).collect::<Vec<_>>().join("\n") };
                self.note(thread, &text, "dim");
            }
        }
        said
    }

    /// What was switched off is the setting `skills.off`, as the operator's edit; the id of the edit, when one was made.
    fn skill_keep(&self, off: &str) -> Option<String> {
        let mut row = toml::Table::new();
        row.insert("value".into(), toml::Value::String(off.into()));
        let step = crate::proposal::Step { to: crate::change::Where::User, file: Some("skills.toml".into()), kind: "setting".into(), id: "skills.off".into(), op: crate::proposal::StepOp::Set(row) };
        let draft = crate::proposal::Draft { key: "skills.off".into(), title: "Skills switched off".into(), why: "the operator switched a skill".into(), evidence: 1, steps: vec![step], widens: Vec::new() };
        match self.edit(draft).ok()? {
            crate::proposal::Edited::Done { id, .. } => Some(id),
            crate::proposal::Edited::Proposed { .. } => None,
        }
    }

    /// Changes taken back that were of `skills.off`: it is what the layers below say again.
    pub(super) fn skills_undone(&self, undone: &[Change]) {
        let (cfg, screen) = (self.0.cfg(), self.screen());
        let (Some(lib), tables) = (&cfg.skills, &screen.tables) else { return };
        for c in undone.iter().filter(|c| c.target.kind == "setting" && c.target.id == "skills.off") {
            let below = |f: &str| tables.field_below(c.target.layer, "setting", "skills.off", f);
            let now = c.after.as_ref().and_then(|t| t.get("value")).or_else(|| below("value")).or_else(|| below("default")).and_then(toml::Value::as_str).unwrap_or_default();
            lib.set_off(now);
        }
    }
}
