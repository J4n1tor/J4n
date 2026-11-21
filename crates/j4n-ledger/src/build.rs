//! The report of one window of a thread, read from its events and from nothing else: the facts, the
//! operator's lines with a receipt each, the answers merged under the line they answer, what needs the operator and what is next.

use j4n_core::text::clip;
use crate::merge::{self, Mark};
use j4n_core::change::Actor;
use j4n_core::log::Ev;
use serde_json::Value as Json;
use std::collections::BTreeSet;

/// The prose setting: no doer's words, their answers quoted, or those and a checked paragraph.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Prose {
    Off,
    Facts,
    Mixed,
}

impl Prose {
    pub fn of(word: &str) -> Prose {
        match word.trim() {
            "off" => Prose::Off,
            "mixed" => Prose::Mixed,
            _ => Prose::Facts,
        }
    }

    pub fn word(self) -> &'static str {
        ["off", "facts", "mixed"][self as usize]
    }
}

/// The answers `(task, text)` that stand under one line (`None`: under no line), in arrival order.
type Group = (Option<usize>, Vec<(String, String)>);

#[derive(Default)]
struct Task {
    id: String,
    by_operator: bool,
    follows: Option<String>,
    /// The task it is a step of, and whether it is a flow's own: a flow says what its step said, and says it once.
    parent: Option<String>,
    flow: bool,
    /// `Some(ok)` once it ended; the word for how.
    ended: Option<(bool, String)>,
    answer: String,
    todo_left: u64,
}

struct Line {
    text: String,
    mark: String,
    /// An item of the inbox, by its id; a line the operator said, by none.
    item: Option<String>,
    item_state: Json,
    /// The task it started or was delivered to.
    task: Option<String>,
    answered: Option<String>,
}

#[derive(Default)]
pub struct Report {
    pub thread: String,
    tasks: Vec<Task>,
    lines: Vec<Line>,
    asks: BTreeSet<String>,
    files: BTreeSet<String>,
    cmds: Vec<(String, i64)>,
    commits: Vec<String>,
    usd: f64,
    plan: Vec<String>,
    decisions: Vec<String>,
    left: Vec<String>,
    /// A task of the thread ended in the window.
    pub ended: bool,
}

fn s<'a>(d: &'a Json, k: &str) -> &'a str {
    d.get(k).and_then(Json::as_str).unwrap_or_default()
}

fn words(list: &Json) -> Vec<String> {
    list.as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default()
}

/// The sha git prints for a commit: `[main abc1234] …`.
fn sha_of(out: &str) -> Option<String> {
    let at = out.find('[')?;
    let inside = &out[at + 1..at + out[at..].find(']')?];
    inside.split_whitespace().last().filter(|w| w.len() >= 7 && w.chars().all(|c| c.is_ascii_hexdigit())).map(String::from)
}

impl Report {
    /// The report of `thread` over `events` (one window, in order).
    pub fn of(thread: &str, events: impl Iterator<Item = Ev>) -> Report {
        let mut r = Report { thread: thread.into(), ..Report::default() };
        let mine = |r: &Report, task: &Option<String>| task.as_ref().is_some_and(|t| r.tasks.iter().any(|x| x.id == *t));
        for ev in events {
            let d = &ev.data;
            let here = ev.thread.as_deref() == Some(thread);
            match ev.kind.as_str() {
                "chat.say" if here => {
                    let text = s(d, "text").trim();
                    if !text.is_empty() && !text.starts_with(':') && !text.starts_with('/') {
                        r.lines.push(Line { text: text.into(), mark: "❯".into(), item: None, item_state: Json::Null, task: None, answered: None });
                    }
                }
                "task.queued" if here => {
                    let spec = d.get("spec").unwrap_or(&Json::Null);
                    let id = ev.task.clone().unwrap_or_default();
                    if s(spec, "doer") == "human" {
                        r.asks.insert(id);
                        continue;
                    }
                    let by_operator = matches!(ev.by, Actor::Operator);
                    if by_operator && let Some(l) = r.lines.last_mut().filter(|l| l.item.is_none() && l.task.is_none() && l.answered.is_none()) {
                        l.task = Some(id.clone());
                    }
                    r.tasks.push(Task { id, by_operator, follows: spec.get("follows").and_then(Json::as_str).map(String::from), parent: spec.get("parent").and_then(Json::as_str).map(String::from), flow: s(spec, "doer").starts_with("flow:"), ..Task::default() });
                }
                "inbox.item" if s(d, "thread") == thread => {
                    let id = s(d, "item").to_string();
                    match r.lines.iter_mut().find(|l| l.item.as_deref() == Some(id.as_str())) {
                        Some(l) => l.item_state = d.clone(),
                        None => {
                            // the line the inbox took is this item, not a line of its own
                            if r.lines.last().is_some_and(|l| l.item.is_none() && l.task.is_none() && l.answered.is_none() && l.text == s(d, "text").trim()) {
                                r.lines.pop();
                            }
                            r.lines.push(Line { text: s(d, "text").into(), mark: s(d, "mark").into(), item: Some(id), item_state: d.clone(), task: None, answered: None });
                        }
                    }
                }
                "task.settled" | "task.stopped" if ev.task.as_ref().is_some_and(|t| r.asks.contains(t)) => {
                    let ask = ev.task.clone().unwrap_or_default();
                    r.asks.remove(&ask);
                    if matches!(ev.by, Actor::Operator)
                        && let Some(l) = r.lines.last_mut().filter(|l| l.item.is_none() && l.task.is_none())
                    {
                        l.answered = Some(ask);
                    }
                }
                "task.settled" | "task.stopped" if mine(&r, &ev.task) => {
                    let t = r.tasks.iter_mut().find(|t| Some(&t.id) == ev.task.as_ref()).expect("`mine` found this task in the report");
                    if ev.kind == "task.settled" {
                        let ok = d.get("ok").and_then(Json::as_bool).unwrap_or(false);
                        t.ended = Some((ok, if ok { "✓".into() } else { format!("✗ {}", clip(s(d, "reason"), 80)) }));
                        t.answer = s(d, "answer").to_string();
                        t.todo_left = d.get("todo_left").and_then(Json::as_u64).unwrap_or(0);
                        if let Some(c) = d.get("changed") {
                            for k in ["created", "modified", "deleted"] {
                                r.files.extend(words(&c[k]));
                            }
                        }
                    } else {
                        let why = d.pointer("/why/kind").and_then(Json::as_str).unwrap_or("stopped");
                        t.ended = Some((false, format!("■ {why}")));
                    }
                    r.ended = true;
                }
                "run.shell" if mine(&r, &ev.task) => {
                    let (cmd, exit) = (s(d, "cmd").to_string(), d.get("exit").and_then(Json::as_i64).unwrap_or(-1));
                    if exit == 0 && cmd.contains("git commit") {
                        r.commits.extend(sha_of(s(d, "out")));
                    }
                    r.cmds.push((cmd, exit));
                }
                "run.metrics" if mine(&r, &ev.task) => r.usd += d.get("plan_usd").or_else(|| d.get("usd")).and_then(Json::as_f64).unwrap_or(0.0),
                "plan.reported" if here => {
                    let (merged, blocked, left) = (words(&d["merged"]), words(&d["blocked"]), words(&d["left"]));
                    let total = merged.len() + blocked.len() + left.len();
                    let unproven = words(&d["unproven"]).len();
                    r.plan.push(format!("plan {}: {}/{total} done ({}){}{}", s(d, "outcome"), merged.len(), merged.join(", "), if blocked.is_empty() { String::new() } else { format!(" · blocked {}", blocked.join(", ")) }, if unproven == 0 { String::new() } else { format!(" · {unproven} unproven") }));
                    r.decisions.extend(words(&d["decisions"]));
                    r.left = left;
                }
                _ => {}
            }
        }
        r
    }

    fn task(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    fn state(&self, id: &str) -> &str {
        self.task(id).and_then(|t| t.ended.as_ref()).map_or("…", |e| e.1.as_str())
    }

    /// The receipt of a line, and whether it waits for the operator.
    fn receipt(&self, l: &Line) -> (String, bool) {
        if l.item.is_none() {
            return match (&l.task, &l.answered) {
                (Some(t), _) => (format!("→ {t} {}", self.state(t)), false),
                (None, Some(a)) => (format!("answered {a}"), false),
                _ => ("no task started".into(), false),
            };
        }
        let (state, turn) = (s(&l.item_state, "state"), s(&l.item_state, "turn"));
        match state {
            "open" => ("waits".into(), true),
            "kept" if l.item_state["pinned"] == true => ("kept · pinned".into(), false),
            "kept" => ("kept".into(), false),
            "delivered" => (format!("delivered to {turn} {}", self.state(turn)), false),
            "started" => (format!("own task {turn} {}", self.state(turn)), false),
            "stopped-task" => (format!("stopped {}", s(&l.item_state, "task")), false),
            other => (other.to_string(), false),
        }
    }

    /// The task each line is answered by: its own, the one it was delivered to, or the one it started.
    fn line_task(&self, l: &Line) -> Option<String> {
        if l.item.is_some() {
            return Some(s(&l.item_state, "turn").to_string()).filter(|t| !t.is_empty() && matches!(s(&l.item_state, "state"), "delivered" | "started"));
        }
        l.task.clone()
    }

    /// Which line answers each task stands under: the line that started it or was delivered to it, the line of the task it follows, or
    /// the line before it. `None`: no line (a task J4n started first).
    fn groups(&self) -> Vec<Group> {
        let mut of: Vec<(String, Option<usize>)> = Vec::new();
        for t in &self.tasks {
            let own = self.lines.iter().position(|l| self.line_task(l).as_deref() == Some(t.id.as_str()));
            let follows = t.follows.as_ref().and_then(|f| of.iter().find(|(id, _)| id == f)).and_then(|x| x.1);
            let before = of.last().and_then(|x| x.1);
            of.push((t.id.clone(), own.or(follows).or(if t.by_operator { None } else { before })));
        }
        let mut groups: Vec<Group> = Vec::new();
        for (id, line) in of {
            let answer = self.task(&id).map(|t| t.answer.trim().to_string()).unwrap_or_default();
            if answer.is_empty() {
                continue;
            }
            // a flow that answers what its step answered says it once: the step's row stands, not `= t2 same as t1`
            if self.task(&id).is_some_and(|t| t.flow) && self.tasks.iter().any(|k| k.parent.as_deref() == Some(id.as_str()) && k.answer.trim() == answer) {
                continue;
            }
            match groups.iter_mut().find(|g| g.0 == line) {
                Some(g) => g.1.push((id, answer)),
                None => groups.push((line, vec![(id, answer)])),
            }
        }
        groups
    }

    fn answer_rows(&self, answers: &[(String, String)], all: bool) -> Vec<String> {
        merge::merge(answers)
            .into_iter()
            .map(|a| match a.mark {
                Mark::Same(by) => format!("= {} same as {by}", a.task),
                mark => {
                    let text = if all { a.full } else { format!("{}{}", a.text, if a.more { " … more" } else { "" }) };
                    format!("{}{}  {text}", if mark == Mark::Differs { "≠ " } else { "" }, a.task)
                }
            })
            .collect()
    }

    pub fn head(&self) -> String {
        let ok = self.tasks.iter().filter(|t| t.ended.as_ref().is_some_and(|e| e.0)).count();
        let bad = self.tasks.iter().filter(|t| t.ended.as_ref().is_some_and(|e| !e.0)).count();
        format!("◆ ledger · {} · {} tasks · ✓ {ok}{}", self.thread, self.tasks.len(), if bad > 0 { format!(" · ✗ {bad}") } else { String::new() })
    }

    /// The facts row: files, commands, commits, cost. `all`: every file and command named.
    fn facts(&self, all: bool) -> String {
        let files: Vec<&str> = self.files.iter().map(String::as_str).collect();
        let shown = if all { files.len() } else { 4 };
        let more = files.len().saturating_sub(shown);
        let mut row = format!("± {} files{}{}", files.len(), if files.is_empty() { "" } else { ": " }, files.iter().take(shown).copied().collect::<Vec<_>>().join(" "));
        if more > 0 {
            row.push_str(&format!(" +{more}"));
        }
        let failed = self.cmds.iter().filter(|c| c.1 != 0).count();
        row.push_str(&format!(" · ❱ {} ran{}", self.cmds.len(), if failed > 0 { format!(", {failed} failed") } else { String::new() }));
        if all {
            for (cmd, exit) in &self.cmds {
                row.push_str(&format!(" · `{cmd}` → {exit}"));
            }
        }
        for c in &self.commits {
            row.push_str(&format!(" · ● commit {c}"));
        }
        row.push_str(&format!(" · ${:.4}", self.usd));
        row
    }

    fn needs(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        out.extend(self.asks.iter().map(|a| format!("{a} asks")));
        out.extend(self.lines.iter().enumerate().filter(|(_, l)| self.receipt(l).1).map(|(n, _)| format!("#{} waits", n + 1)));
        out.extend(self.tasks.iter().filter_map(|t| t.ended.as_ref().filter(|e| !e.0).map(|e| format!("{} {}", t.id, e.1))));
        out.extend(self.decisions.iter().map(|d| format!("decide — {d}")));
        out
    }

    fn next(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if let Some(t) = self.tasks.iter().rev().find(|t| t.ended.is_some()).filter(|t| t.todo_left > 0) {
            out.push(format!("{} to do in {}", t.todo_left, t.id));
        }
        if !self.left.is_empty() {
            out.push(format!("plan left {}", self.left.join(", ")));
        }
        out
    }

    /// The card as rows: `(depth, text)`, the head at 0, its rows at 1 and a line's answers at 2. The chat and the Report page draw the pipes.
    pub fn tree(&self, prose: Prose, all: bool) -> Vec<(u8, String)> {
        let groups = if prose == Prose::Off { Vec::new() } else { self.groups() };
        let under = |line: Option<usize>| groups.iter().find(|g| g.0 == line).map(|g| self.answer_rows(&g.1, all)).unwrap_or_default();
        let mut top: Vec<(String, Vec<String>)> = vec![(self.facts(all), Vec::new())];
        top.extend(self.plan.iter().map(|p| (format!("◆ {p}"), Vec::new())));
        let first = under(None);
        if !first.is_empty() {
            top.push(("◆ started by J4n".into(), first));
        }
        for (n, l) in self.lines.iter().enumerate() {
            let (receipt, _) = self.receipt(l);
            top.push((format!("{} #{} “{}” {receipt}", l.mark, n + 1, clip(&l.text, 60)), under(Some(n))));
        }
        let (needs, next) = (self.needs(), self.next());
        if !needs.is_empty() {
            top.push((format!("◇ needs you: {}", needs.join(" · ")), Vec::new()));
        }
        if !next.is_empty() {
            top.push((format!("◌ next: {}", next.join(" · ")), Vec::new()));
        }
        let mut out = vec![(0, self.head())];
        for (row, kids) in top {
            out.push((1, row));
            out.extend(kids.into_iter().map(|k| (2, k)));
        }
        out
    }

    /// The card as text: the rows on pipes of their own, for a paragraph's request and for a terminal with no screen of ours.
    pub fn render(&self, prose: Prose, all: bool) -> String {
        let tree = self.tree(prose, all);
        let mut out = Vec::new();
        let mut parent_last = false;
        for (i, (d, t)) in tree.iter().enumerate() {
            let last = tree[i + 1..].iter().find(|n| n.0 <= *d).is_none_or(|n| n.0 < *d);
            match d {
                0 => out.push(t.clone()),
                1 => {
                    parent_last = last;
                    out.push(format!("{} {t}", if last { "╰─" } else { "├─" }));
                }
                _ => out.push(format!("{}  {} {t}", if parent_last { " " } else { "│" }, if last { "╰─" } else { "├─" })),
            }
        }
        out.join("\n")
    }

    /// What a paragraph may claim: the facts in full, the receipts and the operator's own lines; never a doer's answer.
    pub fn facts_all(&self) -> String {
        self.render(Prose::Off, true) + "\n" + &self.tasks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>().join(" ") + "\n" + &self.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n")
    }

    /// The doers' answers, as a paragraph's request quotes them.
    pub fn quoted(&self) -> String {
        self.groups().iter().flat_map(|(_, a)| a.iter().map(|(t, text)| format!("{t}: {}", text.lines().map(|l| format!("> {l}")).collect::<Vec<_>>().join("\n")))).collect::<Vec<_>>().join("\n")
    }

    pub fn counts(&self) -> (usize, usize, usize, f64) {
        (self.tasks.len(), self.lines.len(), self.files.len(), self.usd)
    }
}
