//! The fold `lanes`: for each main lane (a thread), the tree of branches its lines gave rise to, with
//! what each took and what came of it, rolled up to the line and the thread. A branch is a task with a parent; the line is the task
//! at the top of its chain. A flow's own row has the total of its children.
//!
//! It also keeps what the main lane has not been told yet: the reports of branches (`report.made`) wait here
//! until `report.sent` says they went into a Brief, which is how a report reaches the lane once.
//!
//! Outcome of a branch that is over: `killed` (stopped before it finished), `blocked` (it failed, or waited for an answer nobody gave),
//! `merged` (its work was laid into the folder), `used` (its report was read, or a flow went on with its answer), else `ignored`.

use crate::change::Actor;
use crate::fold::Fold;
use crate::log::{Ev, Log, New};
use crate::report::Report;
use crate::task::{HUMAN, id_number};
use crate::value::{Key, Value};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

/// The most tasks the fold keeps (the oldest finished ones go first), and the most commands it keeps of one.
const KEEP: usize = 4000;
const RAN: usize = 64;
/// The most reports that wait for one thread.
const WAITING: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum End {
    Ok,
    Blocked,
    Killed,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Node {
    parent: Option<u64>,
    thread: String,
    goal: String,
    doer: String,
    class: String,
    model: String,
    host: String,
    queued_ms: u64,
    started_ms: Option<u64>,
    ended_ms: Option<u64>,
    end: Option<End>,
    micros: i64,
    tokens_in: u64,
    tokens_out: u64,
    cached: u64,
    applied: bool,
    used: bool,
    /// The plan item the task was started for (`input.item`), if any.
    #[serde(default)]
    item: String,
    /// A chat Main started (`input.chat`): it reports as a branch does, though it has no parent task.
    #[serde(default)]
    chat: bool,
    /// The `(cmd, exit)` of its `run.shell` events, without the text of a here-document: what a report's `ran` is checked against (branches and chats only).
    ran: Vec<(String, i64)>,
    /// The `seq` of each of them (what the Log was found broken under is dropped).
    #[serde(default)]
    ran_at: Vec<u64>,
}

#[derive(Default, Serialize, Deserialize)]
struct Inner {
    nodes: BTreeMap<u64, Node>,
    waiting: BTreeMap<String, Vec<(String, Report)>>,
    #[serde(default)]
    lost: u64,
}

/// What a set of branches took.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Totals {
    pub branches: u32,
    pub micros: i64,
    pub wall_ms: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cached: u64,
}

impl Totals {
    fn add(&mut self, o: &Totals) {
        self.branches += o.branches;
        self.micros = self.micros.saturating_add(o.micros);
        self.wall_ms = self.wall_ms.saturating_add(o.wall_ms);
        self.tokens_in = self.tokens_in.saturating_add(o.tokens_in);
        self.tokens_out = self.tokens_out.saturating_add(o.tokens_out);
        self.cached = self.cached.saturating_add(o.cached);
    }
}

/// One task of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub depth: usize,
    pub goal: String,
    pub doer: String,
    pub class: String,
    pub model: String,
    pub host: String,
    pub micros: i64,
    pub wall_ms: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cached: u64,
    /// `used · ignored · merged · blocked · killed`; empty while it is not over.
    pub outcome: &'static str,
    /// This task and everything under it: a flow's row has the total of its children.
    pub sub: Totals,
}

/// A line of the main lane and its branches, in the order they were queued; each is under its parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub root: Row,
    pub rows: Vec<Row>,
    /// The sum of the rows of its branches (not the line's own).
    pub total: Totals,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Lane {
    pub thread: String,
    pub lines: Vec<Line>,
    /// The sum of the totals of the lines.
    pub total: Totals,
}

/// The fold `lanes`. A clone is a second handle on the same state.
#[derive(Clone, Default)]
pub struct Lanes(Arc<Mutex<Inner>>);

fn text(d: &Json, key: &str) -> String {
    d.get(key).and_then(Json::as_str).unwrap_or_default().to_string()
}

fn clip(s: &str, n: usize) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= n { one } else { format!("{}…", one.chars().take(n).collect::<String>()) }
}

impl Node {
    fn wall(&self) -> u64 {
        match (self.ended_ms, self.started_ms.unwrap_or(self.queued_ms)) {
            (Some(end), from) => end.saturating_sub(from),
            _ => 0,
        }
    }

    fn outcome(&self) -> &'static str {
        match self.end {
            None => "",
            Some(End::Killed) => "killed",
            Some(End::Blocked) => "blocked",
            Some(End::Ok) if self.applied => "merged",
            Some(End::Ok) if self.used => "used",
            Some(End::Ok) => "ignored",
        }
    }

    fn own(&self) -> Totals {
        Totals { branches: 1, micros: self.micros, wall_ms: self.wall(), tokens_in: self.tokens_in, tokens_out: self.tokens_out, cached: self.cached }
    }
}

impl Inner {
    fn lock_thread(&self, n: u64) -> String {
        self.nodes.get(&n).map(|x| x.thread.clone()).unwrap_or_default()
    }

    /// The tree under `id`, as rows, depth first.
    fn tree(&self, id: u64, depth: usize, out: &mut Vec<Row>) -> Totals {
        let Some(n) = self.nodes.get(&id) else { return Totals::default() };
        let at = out.len();
        out.push(Row {
            id: format!("t{id}"),
            depth,
            goal: n.goal.clone(),
            doer: n.doer.clone(),
            class: n.class.clone(),
            model: n.model.clone(),
            host: n.host.clone(),
            micros: n.micros,
            wall_ms: n.wall(),
            tokens_in: n.tokens_in,
            tokens_out: n.tokens_out,
            cached: n.cached,
            outcome: n.outcome(),
            sub: Totals::default(),
        });
        let mut sub = n.own();
        let kids: Vec<u64> = self.nodes.iter().filter(|(_, k)| k.parent == Some(id)).map(|(k, _)| *k).collect();
        for k in kids {
            sub.add(&self.tree(k, depth + 1, out));
        }
        out[at].sub = sub;
        sub
    }
}

impl Lanes {
    pub fn new() -> Lanes {
        Lanes::default()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The `(cmd, exit)` the task ran itself: what a report of it is checked against.
    pub fn ran(&self, task: &str) -> Vec<(String, i64)> {
        id_number(task).and_then(|n| self.lock().nodes.get(&n).map(|x| x.ran.clone())).unwrap_or_default()
    }

    /// A branch's answer as a checked report, written to the Log as `report.made`: its claims are looked up in the branch's place (the
    /// work it kept, else `root`) and its `ran` among its own `run.shell` events. `deliver` is for the main lane's Brief (`branches`); a
    /// flow that reads the report itself passes false. An answer that is not in the shape of a report is one that says so.
    pub fn settle(&self, log: &Log, row: &crate::task::Row, root: &Path, answer: &str, deliver: bool) -> Report {
        let mut report = Report::take(answer).unwrap_or_else(|why| Report::plain(answer, &format!("not a report: {why}")));
        let dir = row.kept.as_ref().map(|k| PathBuf::from(&k.dir)).filter(|d| d.is_dir()).unwrap_or_else(|| root.to_path_buf());
        report.check(&dir, &self.ran(&row.id));
        // the goal and the files its checked claims name, as hashes: what a later task of the same goal is shown the claims against
        let (goal, files) = (crate::report::goal_key(&row.spec.goal), report.files(&dir));
        let mut n = New::of("report.made", Actor::Core, json!({ "report": report.json(), "deliver": deliver, "goal": goal, "files": files })).task(&row.id);
        if let Some(t) = &row.spec.thread {
            n = n.thread(t);
        }
        let _ = log.append(n);
        report
    }

    /// The tasks started for plan item `item` of `thread`, oldest first (`input.item`): its branches, their checks and their reviews.
    pub fn item_tasks(&self, thread: &str, item: &str) -> Vec<String> {
        self.lock().nodes.iter().filter(|(_, n)| n.thread == thread && n.item == item).map(|(k, _)| format!("t{k}")).collect()
    }

    /// What `tasks` took, each on its own (not what is under them).
    pub fn spent(&self, tasks: &[String]) -> Totals {
        let inner = self.lock();
        let mut sum = Totals::default();
        for n in tasks.iter().filter_map(|t| id_number(t)).filter_map(|n| inner.nodes.get(&n)) {
            sum.add(&n.own());
        }
        sum
    }

    /// The reports that wait to be told to the main lane of `thread`, oldest first.
    pub fn waiting(&self, thread: &str) -> Vec<(String, Report)> {
        self.lock().waiting.get(thread).cloned().unwrap_or_default()
    }

    /// The threads that have a line with a branch.
    pub fn threads(&self) -> Vec<String> {
        let inner = self.lock();
        let mut all: Vec<String> = inner.nodes.values().filter(|n| n.parent.is_some()).map(|n| n.thread.clone()).collect();
        all.sort();
        all.dedup();
        all
    }

    /// The lane of `thread`: the lines that have at least one branch, each with its tree, and the totals.
    pub fn lane(&self, thread: &str) -> Lane {
        let inner = self.lock();
        let top = |mut n: u64| {
            for _ in 0..16 {
                match inner.nodes.get(&n).and_then(|x| x.parent) {
                    Some(p) if inner.nodes.contains_key(&p) => n = p,
                    _ => break,
                }
            }
            n
        };
        let mut roots: Vec<u64> = inner.nodes.iter().filter(|(_, n)| n.parent.is_some() && n.thread == thread).map(|(k, _)| top(*k)).collect();
        roots.sort_unstable();
        roots.dedup();
        let mut lane = Lane { thread: thread.to_string(), ..Lane::default() };
        for r in roots {
            let mut rows = Vec::new();
            let all = inner.tree(r, 0, &mut rows);
            let root = rows.remove(0);
            let mut total = all;
            // the line's own is not a branch
            total.branches -= 1;
            total.micros -= root.micros;
            total.wall_ms = total.wall_ms.saturating_sub(root.wall_ms);
            total.tokens_in -= root.tokens_in;
            total.tokens_out -= root.tokens_out;
            total.cached -= root.cached;
            lane.total.add(&total);
            lane.lines.push(Line { root, rows, total });
        }
        lane
    }
}

impl Fold for Lanes {
    fn id(&self) -> &str {
        "lanes"
    }

    fn version(&self) -> u64 {
        4
    }

    fn wants(&self, kind: &str) -> bool {
        matches!(
            kind,
            "task.queued" | "task.started" | "task.settled" | "task.stopped" | "task.applied" | "task.rewound" | "run.metrics" | "run.served" | "run.shell" | "report.made" | "report.sent" | "flow.step.ended" | "log.broken"
        )
    }

    fn apply(&mut self, ev: &Ev) {
        let mut inner = self.lock();
        let d = &ev.data;
        if ev.kind == "log.broken" {
            // the lines between the break and this note may not be J4n's, so what they say a task ran is not evidence
            let after = d.get("after").and_then(Json::as_u64).unwrap_or(0);
            for x in inner.nodes.values_mut() {
                let kept: Vec<_> = x.ran.drain(..).zip(x.ran_at.drain(..)).filter(|(_, at)| *at <= after || *at >= ev.seq).collect();
                (x.ran, x.ran_at) = kept.into_iter().unzip();
            }
            return;
        }
        if ev.kind == "report.sent" {
            let thread = ev.thread.clone().unwrap_or_default();
            let sent: Vec<String> = d.get("tasks").and_then(Json::as_array).map(|a| a.iter().filter_map(Json::as_str).map(String::from).collect()).unwrap_or_default();
            if let Some(w) = inner.waiting.get_mut(&thread) {
                w.retain(|(t, _)| !sent.contains(t));
            }
            for t in sent.iter().filter_map(|t| id_number(t)) {
                if let Some(n) = inner.nodes.get_mut(&t) {
                    n.used = true;
                }
            }
            return;
        }
        let Some(n) = ev.task.as_deref().and_then(id_number) else { return };
        if ev.kind == "task.queued" {
            let spec = d.get("spec").cloned().unwrap_or(Json::Null);
            if spec.get("doer").and_then(Json::as_str) == Some(HUMAN) || inner.nodes.contains_key(&n) {
                return;
            }
            let at = |p: &str| spec.pointer(p).and_then(Json::as_str).unwrap_or_default().to_string();
            let node = Node {
                parent: spec.get("parent").and_then(Json::as_str).and_then(id_number),
                thread: at("/thread"),
                goal: clip(&at("/goal"), 80),
                doer: at("/doer"),
                class: at("/role"),
                item: at("/input/item"),
                chat: !at("/input/chat").is_empty(),
                model: at("/tune/model"),
                host: at("/tune/endpoint"),
                queued_ms: ev.at_ms,
                ..Node::default()
            };
            inner.nodes.insert(n, node);
            while inner.nodes.len() > KEEP {
                let Some(old) = inner.nodes.iter().find(|(_, x)| x.end.is_some()).map(|(k, _)| *k) else { break };
                inner.nodes.remove(&old);
                inner.lost += 1;
            }
            return;
        }
        if ev.kind == "report.made" {
            let thread = ev.thread.clone().unwrap_or_else(|| inner.lock_thread(n));
            let Some(report) = d.get("report").and_then(|r| serde_json::from_value::<Report>(r.clone()).ok()) else { return };
            if d.get("deliver").and_then(Json::as_bool) == Some(false) {
                if let Some(x) = inner.nodes.get_mut(&n) {
                    x.used = true;
                }
                return;
            }
            let w = inner.waiting.entry(thread).or_default();
            w.push((format!("t{n}"), report));
            let over = w.len().saturating_sub(WAITING);
            w.drain(..over);
            inner.lost += over as u64;
            return;
        }
        // a flow went on with the answer of the child it names
        if ev.kind == "flow.step.ended" {
            if d.get("ok").and_then(Json::as_bool) == Some(true)
                && let Some(c) = d.get("task").and_then(Json::as_str).and_then(id_number).and_then(|c| inner.nodes.get_mut(&c))
            {
                c.used = true;
            }
            return;
        }
        let Some(x) = inner.nodes.get_mut(&n) else { return };
        match ev.kind.as_str() {
            "task.started" => {
                x.started_ms.get_or_insert(ev.at_ms);
            }
            "task.settled" if x.end.is_none() => {
                x.end = Some(if d.get("ok").and_then(Json::as_bool) == Some(true) { End::Ok } else { End::Blocked });
                x.ended_ms = Some(ev.at_ms);
            }
            "task.stopped" if x.end.is_none() => {
                x.end = Some(match d.pointer("/why/kind").and_then(Json::as_str) {
                    Some("failed" | "parked" | "deadline") => End::Blocked,
                    _ => End::Killed,
                });
                x.ended_ms = Some(ev.at_ms);
            }
            "task.applied" => x.applied = true,
            "task.rewound" => x.applied = false,
            "run.metrics" => {
                let spent = d.get("plan_usd").or_else(|| d.get("usd")).and_then(Json::as_f64).unwrap_or(0.0);
                x.micros = x.micros.saturating_add(crate::task::micros(spent));
                x.tokens_in = x.tokens_in.saturating_add(d.get("tokens_in").and_then(Json::as_u64).unwrap_or(0));
                x.tokens_out = x.tokens_out.saturating_add(d.get("tokens_out").and_then(Json::as_u64).unwrap_or(0));
                // the doers that say their cache parts with the cost (claude) rather than in `run.served` (the model doers): one row, never both
                x.cached = x.cached.saturating_add(d.get("cached").and_then(Json::as_u64).unwrap_or(0));
            }
            "run.served" => {
                x.cached = x.cached.saturating_add(d.get("cached").and_then(Json::as_u64).unwrap_or(0));
                // what served is what ran; the pin it was asked under stays in the Log
                if let Some(model) = Some(text(d, "model")).filter(|m| !m.is_empty()) {
                    x.model = model;
                }
                if let Some(h) = d.get("host").and_then(Json::as_str).filter(|h| !h.is_empty()) {
                    x.host = h.to_string();
                }
            }
            "run.shell" if x.parent.is_some() || x.chat => {
                x.ran.push((clip(&no_heredoc(&text(d, "cmd")), 300), d.get("exit").and_then(Json::as_i64).unwrap_or(0)));
                x.ran_at.push(ev.seq);
                x.ran.drain(..x.ran.len().saturating_sub(RAN));
                x.ran_at.drain(..x.ran_at.len().saturating_sub(RAN));
            }
            _ => {}
        }
    }

    fn read(&self, path: &[Key], _now_ms: u64) -> Value {
        let threads = self.threads();
        let totals = |t: &Totals| {
            Value::Map(BTreeMap::from([
                ("branches".to_string(), Value::num(f64::from(t.branches))),
                ("usd".to_string(), Value::Usd(t.micros)),
                ("wall_ms".to_string(), Value::num(t.wall_ms as f64)),
                ("tokens_in".to_string(), Value::num(t.tokens_in as f64)),
                ("tokens_out".to_string(), Value::num(t.tokens_out as f64)),
                ("cached".to_string(), Value::num(t.cached as f64)),
            ]))
        };
        let row = |r: &Row| {
            let mut m = BTreeMap::from([
                ("id".to_string(), Value::text(&r.id)),
                ("depth".to_string(), Value::num(r.depth as f64)),
                ("class".to_string(), Value::text(&r.class)),
                ("doer".to_string(), Value::text(&r.doer)),
                ("model".to_string(), Value::text(&r.model)),
                ("host".to_string(), Value::text(&r.host)),
                ("outcome".to_string(), Value::text(r.outcome)),
            ]);
            if let Value::Map(t) = totals(&Totals { branches: 1, micros: r.micros, wall_ms: r.wall_ms, tokens_in: r.tokens_in, tokens_out: r.tokens_out, cached: r.cached }) {
                m.extend(t);
            }
            m.insert("sub".into(), totals(&r.sub));
            Value::Map(m)
        };
        let all = threads.iter().map(|t| {
            let lane = self.lane(t);
            let lines = lane.lines.iter().map(|l| (l.root.id.clone(), Value::Map(BTreeMap::from([("total".to_string(), totals(&l.total)), ("rows".to_string(), Value::List(l.rows.iter().map(row).collect()))])))).collect();
            (t.clone(), Value::Map(BTreeMap::from([("total".to_string(), totals(&lane.total)), ("lines".to_string(), Value::Map(lines))])))
        });
        Value::Map(all.collect()).at(path)
    }

    fn save(&self) -> Json {
        serde_json::to_value(&*self.lock()).unwrap_or(Json::Null)
    }

    fn load(&mut self, state: Json) -> Result<(), String> {
        *self.lock() = serde_json::from_value(state).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn reset(&mut self) {
        *self.lock() = Inner::default();
    }

    fn notes(&self) -> Vec<String> {
        match self.lock().lost {
            0 => Vec::new(),
            n => vec![format!("{n} older tasks or waiting reports are left out of the lanes; the most it keeps is {KEEP} tasks and {WAITING} reports a thread")],
        }
    }
}

/// A command line without the text of its here-documents (`cat > app.py <<'EOF'` … `EOF`): a file written that way is not a command, and the commands after it
/// would be lost to the clip; its lines are joined with ` ; `, as the clip makes one line.
fn no_heredoc(cmd: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut until: Option<String> = None;
    for line in cmd.lines() {
        if let Some(end) = &until {
            if line.trim() == end {
                until = None;
            }
            continue;
        }
        out.push(line);
        until = line.split("<<").nth(1).map(|w| w.trim_start_matches('-').trim()).and_then(|w| w.split_whitespace().next()).map(|w| w.trim_matches(['\'', '"']).to_string()).filter(|w| !w.is_empty());
    }
    // the lines are commands of their own: the clip makes one line of them
    out.join(" ; ")
}
