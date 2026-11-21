//! The part runner. The operator names a document; its parts are shown once as a Draft
//! and become items of the plan only on `:parts go`, at most `parts.max` a run. Each part's turn is the plan's inline item; when it is
//! over its gate runs (the command of its row, else the project's check), and only a green gate is committed: the paths the turn changed and
//! no others, by the doer `parts` through the Gate, on the branch checked out, never pushed. A red gate, a turn that did not end well or a
//! commit that failed stops the run. `show` waits for `:parts next` after each green gate; `auto` goes on. With the switch off the front
//! never calls this crate.

pub mod doc;
pub mod git;

use j4n_core::text::line as clip;
use doc::Part;
use j4n_connect::connect::StepDoer;
use j4n_core::change::Actor;
use j4n_core::compact::MustKeep;
use j4n_core::doer::Doer;
use j4n_core::fmt::Fmt;
use j4n_core::eval::Scope;
use j4n_core::front::parts::{Boundary, Hands, Parts, Taken};
use j4n_core::keeper::{Item, Plan};
use j4n_core::log::{Log, New};
use j4n_core::setting::Settings;
use j4n_core::task::{Row, State, Why};
use j4n_core::value::Value;
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

/// The commit message, as `Fmt` makes it: a subject and one line of the gate (each a paragraph of its own, a `-m` each).
const SUBJECT: &str = "J4n part {id}: {title}";
const GATE: &str = "gate: {gate} — green ({run}, part {n} of {of}, {doc})";
/// The most of a document read.
const DOC_MAX: u64 = 512 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Off,
    Show,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Compact,
    Continue,
    Reset,
}

impl Edge {
    fn word(self) -> &'static str {
        match self {
            Edge::Compact => "compact",
            Edge::Continue => "continue",
            Edge::Reset => "reset",
        }
    }
}

/// A run: the parts `go` took, as `(item, part)`, in order.
#[derive(Debug, Clone, Default)]
struct Run {
    id: String,
    doc: String,
    items: Vec<(String, Part)>,
}

/// Where a part is: its last `parts.step`.
#[derive(Debug, Clone, Default)]
struct Step {
    phase: String,
    task: String,
    sha: String,
    /// What its turn changed, the questions it had answered and the operator's steers (the must-keep of a `reset`).
    paths: Vec<String>,
    decisions: Vec<String>,
    prompts: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct Thread {
    /// The document and its parts as the operator keeps them, in order.
    doc: String,
    parts: Vec<Part>,
    /// The parts of this document a run has taken.
    taken: Vec<String>,
    runs: Vec<Run>,
    steps: BTreeMap<String, Step>,
}

pub struct Runner {
    log: Log,
    root: PathBuf,
    /// `parts.mode`, `parts.max` and `parts.boundary` as the settings say now (an edit tunes them; a run going logged its own).
    knobs: Mutex<(Mode, usize, Edge)>,
    /// Each thread, read from the Log the first time it is asked about.
    threads: Mutex<Option<BTreeMap<String, Thread>>>,
}

fn s<'a>(d: &'a Json, k: &str) -> &'a str {
    d.get(k).and_then(Json::as_str).unwrap_or_default()
}

fn ok(row: &Row) -> bool {
    matches!(row.state, State::Done { ok: true })
}

/// Why a task did not end well: its reason, the message it failed with, its answer, or its state.
fn why(row: &Row) -> String {
    let failed = match &row.state {
        State::Stopped { why: Why::Failed { message } } => Some(message.clone()),
        _ => None,
    };
    row.reason.clone().filter(|r| !r.trim().is_empty()).or(failed).or(row.answer.clone()).unwrap_or_else(|| row.state.word().into())
}

/// The numbers of the settings the runner reads.
fn knobs(settings: &Settings) -> (Mode, usize, Edge) {
    let mode = match settings.word("parts.mode", "off").trim() {
        "show" => Mode::Show,
        "auto" => Mode::Auto,
        _ => Mode::Off,
    };
    let edge = match settings.word("parts.boundary", "compact").trim() {
        "continue" => Edge::Continue,
        "reset" => Edge::Reset,
        _ => Edge::Compact,
    };
    let max = settings.num("parts.max", 3.0).clamp(1.0, 24.0) as usize;
    (mode, max, edge)
}

impl Runner {
    fn mode(&self) -> Mode {
        self.knobs.lock().unwrap_or_else(|e| e.into_inner()).0
    }

    fn max(&self) -> usize {
        self.knobs.lock().unwrap_or_else(|e| e.into_inner()).1
    }

    fn edge(&self) -> Edge {
        self.knobs.lock().unwrap_or_else(|e| e.into_inner()).2
    }

    pub fn new(log: &Log, settings: &Settings, root: &Path) -> Runner {
        Runner { log: log.clone(), root: root.to_path_buf(), knobs: Mutex::new(knobs(settings)), threads: Mutex::new(None) }
    }

    /// The doer that runs a part's gate command and its commit.
    pub fn doer() -> Arc<dyn Doer> {
        Arc::new(StepDoer { id: git::DOER, doc: "runs a part's gate and commits the paths its turn changed, each line asked of the Gate first; never pushes", work: Arc::new(git::work) })
    }

    fn write(&self, thread: &str, kind: &str, by: Actor, data: Json) -> bool {
        self.log.append(New::of(kind, by, data).thread(thread)).is_ok()
    }

    fn note(&self, thread: &str, text: &str, tone: &str) {
        self.write(thread, "chat.note", Actor::Core, json!({ "text": text, "tone": tone }));
    }

    /// Every thread as its `parts.*` events leave it (a restart reads where each part was).
    fn threads(&self) -> MutexGuard<'_, Option<BTreeMap<String, Thread>>> {
        let mut all = self.threads.lock().unwrap_or_else(|e| e.into_inner());
        if all.is_none() {
            self.log.sync();
            let mut map: BTreeMap<String, Thread> = BTreeMap::new();
            for ev in self.log.read(0).filter(|e| e.kind.starts_with("parts.")) {
                let Some(t) = ev.thread.clone() else { continue };
                apply(map.entry(t).or_default(), &ev.kind, &ev.data);
            }
            *all = Some(map);
        }
        all
    }

    /// Write a `parts.*` event and take it into the thread's state.
    fn put(&self, thread: &str, kind: &str, by: Actor, data: Json) {
        if self.write(thread, kind, by, data.clone()) {
            apply(self.threads().get_or_insert_default().entry(thread.into()).or_default(), kind, &data);
        }
    }

    fn thread(&self, thread: &str) -> Thread {
        self.threads().as_ref().and_then(|m| m.get(thread).cloned()).unwrap_or_default()
    }

    /// The run and the part an item is, when it is one (an item of the same id that another made is not).
    fn part_of(&self, thread: &str, item: &Item) -> Option<(Run, usize)> {
        let t = self.thread(thread);
        t.runs.into_iter().rev().find_map(|r| {
            let n = r.items.iter().position(|(id, p)| *id == item.id && item.goal == p.words)?;
            Some((r, n))
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn step(&self, thread: &str, run: &Run, id: &str, phase: &str, task: &str, sha: &str, why: &str) {
        self.put(thread, "parts.step", Actor::Core, json!({ "run": run.id, "part": id, "phase": phase, "task": task, "sha": sha, "why": clip(why, 400) }));
    }

    /// The run stops at `id`: the item is blocked, nothing more of the run starts, and the operator is told.
    fn stop(&self, thread: &str, run: &Run, id: &str, why: &str) -> Taken {
        self.step(thread, run, id, "stopped", "", "", why);
        self.note(thread, &format!("◆ parts · {} · {id} stopped the run: {} · nothing of it was committed", run.id, clip(why, 300)), "warn");
        Taken::Settle { state: "blocked", result: clip(why, 400), block: "parts" }
    }

    /// The Draft card: the document, the parts in order with their gates, what the next run takes, and the words that change it.
    fn card(&self, t: &Thread) -> String {
        let next: Vec<&Part> = t.parts.iter().filter(|p| !t.taken.contains(&p.id)).take(self.max()).collect();
        let mut text = format!("◆ parts · {} · {} parts · the next run takes {} (parts.max {})", t.doc, t.parts.len(), next.len(), self.max());
        for (n, p) in t.parts.iter().enumerate() {
            let mark = if t.taken.contains(&p.id) { "✓" } else if next.iter().any(|x| x.id == p.id) { "▸" } else { "·" };
            text.push_str(&format!("\n│ {mark} {} {} · {} ⟂ gate {}", n + 1, p.id, p.title(), p.gate_words()));
        }
        text.push_str("\n╰ :parts go · :parts keep 3 1 2 · :parts drop 2");
        text
    }

    fn read(&self, thread: &str, path: &str) -> Result<String, String> {
        let p = Path::new(path);
        let full = if p.is_absolute() { p.to_path_buf() } else { self.root.join(p) };
        let (root, full) = (self.root.canonicalize().map_err(|e| e.to_string())?, full.canonicalize().map_err(|e| format!("`{path}` cannot be read: {e}"))?);
        let rel = full.strip_prefix(&root).map_err(|_| format!("`{path}` is outside the project folder"))?;
        if std::fs::metadata(&full).map(|m| m.len()).unwrap_or(0) > DOC_MAX {
            return Err(format!("`{path}` is more than {} KiB", DOC_MAX / 1024));
        }
        let text = std::fs::read_to_string(&full).map_err(|e| format!("`{path}` cannot be read: {e}"))?;
        let parts = doc::read(&text);
        if parts.is_empty() {
            return Err(format!("`{path}` has no parts: no table with a `Gate` column, no `###` step and no table"));
        }
        let doc = rel.display().to_string();
        let order: Vec<&str> = parts.iter().map(|p| p.id.as_str()).collect();
        self.put(thread, "parts.draft", Actor::Operator, json!({ "doc": doc, "parts": parts, "order": order }));
        Ok(self.card(&self.thread(thread)))
    }

    /// `keep 3 1 2` / `drop 2`: the parts by their numbers on the card.
    fn edit(&self, thread: &str, word: &str, nums: &[String]) -> Result<String, String> {
        let t = self.thread(thread);
        if t.parts.is_empty() {
            return Err("no Draft: `:parts <document>` reads one".into());
        }
        let picked: Vec<usize> = nums.iter().flat_map(|w| w.split(',')).filter(|w| !w.is_empty()).map(|w| w.parse::<usize>().ok().filter(|n| (1..=t.parts.len()).contains(n)).ok_or(format!("`{w}` is not a part's number (1 to {})", t.parts.len()))).collect::<Result<_, _>>()?;
        if picked.is_empty() {
            return Err(format!("`:parts {word}` takes the parts' numbers"));
        }
        let order: Vec<&str> = match word {
            "keep" => picked.iter().map(|n| t.parts[n - 1].id.as_str()).collect(),
            _ => t.parts.iter().enumerate().filter(|(i, _)| !picked.contains(&(i + 1))).map(|(_, p)| p.id.as_str()).collect(),
        };
        self.put(thread, "parts.draft", Actor::Operator, json!({ "doc": t.doc, "order": order }));
        Ok(self.card(&self.thread(thread)))
    }

    /// `go`: the next parts of the Draft become items of the plan, the operator's, each after the one before.
    fn go(&self, thread: &str, plan: Option<&Plan>) -> Result<String, String> {
        let t = self.thread(thread);
        if let Some(plan) = plan.filter(|p| p.open()) {
            let left: Vec<String> = plan.items.iter().filter(|i| !i.done()).map(|i| format!("{} ({})", i.id, i.state)).collect();
            return Err(format!("the plan is open ({}): one plan at a time — let it end, or `:plan` drop what is left", left.join(", ")));
        }
        let next: Vec<Part> = t.parts.iter().filter(|p| !t.taken.contains(&p.id)).take(self.max()).cloned().collect();
        if next.is_empty() {
            return Err(if t.parts.is_empty() { "no Draft: `:parts <document>` reads one".into() } else { format!("every part of {} has run", t.doc) });
        }
        let used = |id: &str| plan.is_some_and(|p| p.items.iter().any(|i| i.id == id));
        let run = format!("r{}", t.runs.len() + 1);
        let mut items: Vec<(String, Part)> = Vec::new();
        for p in next {
            let id = (1..).map(|n| if n == 1 { p.id.clone() } else { format!("{}-{n}", p.id.chars().take(20).collect::<String>()) }).find(|id| !used(id)).unwrap_or_default();
            let depends: Vec<&str> = items.last().map(|(prev, _)| prev.as_str()).into_iter().collect();
            let done = format!("its gate is green: {}", p.gate_words());
            self.write(thread, "plan.item", Actor::Operator, json!({ "op": "add", "id": id, "goal": p.words, "where": "inline", "main": true, "depends": depends, "done": done }));
            items.push((id, p));
        }
        let ids: Vec<&str> = items.iter().map(|(i, _)| i.as_str()).collect();
        let parts: Vec<&str> = items.iter().map(|(_, p)| p.id.as_str()).collect();
        self.put(thread, "parts.run", Actor::Operator, json!({ "run": run, "doc": t.doc, "items": ids, "parts": parts, "max": self.max(), "boundary": self.edge().word() }));
        let how = if self.mode() == Mode::Show { "each green gate waits for `:parts next`" } else { "each green gate is committed and the next part starts" };
        Ok(format!("◆ parts · {run} · {} · {} · {how}; a red gate stops the run", t.doc, ids.join(" → ")))
    }

    /// `next` (show): the part whose gate is green is committed and the run goes on.
    fn next(&self, thread: &str) -> Result<String, String> {
        let t = self.thread(thread);
        let ready = t.runs.iter().rev().find_map(|r| r.items.iter().find(|(id, _)| t.steps.get(id).is_some_and(|s| s.phase == "ready")).map(|(id, _)| (r.clone(), id.clone())));
        let Some((run, id)) = ready else { return Err("no part waits for its commit".into()) };
        let task = t.steps.get(&id).map(|s| s.task.clone()).unwrap_or_default();
        self.step(thread, &run, &id, "go", &task, "", "the operator said next");
        Ok(format!("{id}: committing, then the next part"))
    }

    /// The gate of a part, started as a child of its turn: its command on the doer `parts`, else the project's check.
    fn gate(&self, thread: &str, run: &Run, id: &str, part: &Part, work: &Row, hands: &dyn Hands) -> Taken {
        let started = if part.cmd.is_empty() {
            hands.start("check", &format!("check part {id}"), json!({ "item": id }))
        } else {
            hands.start(git::DOER, &format!("gate of part {id}: {}", part.gate_words()), json!({ "job": "gate", "argv": part.cmd }))
        };
        match started {
            Ok(task) => {
                let c = &work.changed;
                let paths: Vec<&String> = c.created.iter().chain(&c.modified).chain(&c.deleted).collect();
                self.put(thread, "parts.step", Actor::Core, json!({ "run": run.id, "part": id, "phase": "gate", "task": task, "sha": "", "why": "", "paths": paths, "decisions": work.decisions, "prompts": work.prompts }));
                Taken::Wait
            }
            Err(why) => self.stop(thread, run, id, &format!("its gate could not start: {why}")),
        }
    }

    /// The commit of a green part: the paths its turn changed, a message made by `Fmt`.
    fn commit(&self, thread: &str, run: &Run, n: usize, work: &Row, hands: &dyn Hands) -> Taken {
        let (id, part) = &run.items[n];
        let c = &work.changed;
        let paths: Vec<String> = c.created.iter().chain(&c.modified).chain(&c.deleted).cloned().collect::<std::collections::BTreeSet<_>>().into_iter().collect();
        if c.more > 0 {
            return self.stop(thread, run, id, &format!("its turn changed more files than J4n keeps the names of ({} more): commit it by hand", c.more));
        }
        if paths.is_empty() {
            self.step(thread, run, id, "done", "", "", "it changed nothing");
            return Taken::Settle { state: "merged", result: "gate green; it changed nothing, so nothing was committed".into(), block: "" };
        }
        let scope = Scope::new()
            .with("id", Value::text(id.as_str()))
            .with("title", Value::text(part.title()))
            .with("gate", Value::text(part.gate_words()))
            .with("run", Value::text(run.id.as_str()))
            .with("doc", Value::text(run.doc.as_str()))
            .with("n", Value::num((n + 1) as f64))
            .with("of", Value::num(run.items.len() as f64));
        let render = |t: &str| Fmt::parse(t).map(|f| f.render(&scope)).unwrap_or_else(|_| t.to_string());
        let mut message = vec![render(SUBJECT), render(GATE)];
        // the doer's words are only quoted
        message.extend(work.answer.iter().flat_map(|a| a.lines().map(str::trim).filter(|l| !l.is_empty()).take(2).map(|l| format!("> {}", clip(l, 200))).collect::<Vec<_>>()));
        let message: Vec<String> = message.into_iter().map(|m| m.chars().map(|c| if c.is_control() || (c.is_whitespace() && !c.is_ascii()) { ' ' } else { c }).collect()).collect();
        match hands.start(git::DOER, &format!("commit part {id}"), json!({ "job": "commit", "paths": paths, "message": message })) {
            Ok(task) => {
                self.step(thread, run, id, "commit", &task, "", "");
                Taken::Wait
            }
            Err(why) => self.stop(thread, run, id, &format!("its commit could not start: {why}")),
        }
    }
}

/// One `parts.*` event into a thread's state.
fn apply(t: &mut Thread, kind: &str, d: &Json) {
    let words = |k: &str| d.get(k).and_then(Json::as_array).map(|a| a.iter().filter_map(Json::as_str).map(String::from).collect::<Vec<_>>()).unwrap_or_default();
    match kind {
        "parts.draft" => {
            if let Some(parts) = d.get("parts").and_then(|p| serde_json::from_value::<Vec<Part>>(p.clone()).ok()) {
                // a new document: what earlier runs took of another is not this one's
                if s(d, "doc") != t.doc {
                    t.taken.clear();
                }
                (t.doc, t.parts) = (s(d, "doc").to_string(), parts);
            }
            let order = words("order");
            let all = std::mem::take(&mut t.parts);
            t.parts = order.iter().filter_map(|id| all.iter().find(|p| p.id == *id).cloned()).collect();
        }
        "parts.run" => {
            let parts = words("parts");
            let items: Vec<(String, Part)> = words("items").into_iter().zip(&parts).filter_map(|(i, pid)| t.parts.iter().find(|p| p.id == *pid).map(|p| (i, p.clone()))).collect();
            t.taken.extend(parts);
            t.runs.push(Run { id: s(d, "run").into(), doc: s(d, "doc").into(), items });
        }
        "parts.step" => {
            let old = t.steps.remove(s(d, "part")).unwrap_or_default();
            let keep = |k: &str, was: Vec<String>| if d.get(k).is_some() { words(k) } else { was };
            let step = Step { phase: s(d, "phase").into(), task: s(d, "task").into(), sha: s(d, "sha").into(), paths: keep("paths", old.paths), decisions: keep("decisions", old.decisions), prompts: keep("prompts", old.prompts) };
            t.steps.insert(s(d, "part").into(), step);
        }
        _ => {}
    }
}

impl Parts for Runner {
    fn tune(&self, settings: &Settings) {
        *self.knobs.lock().unwrap_or_else(|e| e.into_inner()) = knobs(settings);
    }

    fn on(&self) -> bool {
        self.mode() != Mode::Off
    }

    fn part(&self, thread: &str, item: &Item) -> bool {
        self.part_of(thread, item).is_some()
    }

    fn ended(&self, thread: &str, item: &Item, work: &Row, hands: &dyn Hands) -> Taken {
        let Some((run, n)) = self.part_of(thread, item) else { return Taken::Not };
        let (id, part) = &run.items[n];
        let step = self.thread(thread).steps.get(id).cloned().unwrap_or_default();
        let row = (!step.task.is_empty()).then(|| hands.row(&step.task)).flatten();
        let over = row.as_ref().is_some_and(|r| r.state.is_terminal());
        match step.phase.as_str() {
            "" | "work" => {
                if !ok(work) {
                    return self.stop(thread, &run, id, &format!("its turn did not finish: {}", why(work)));
                }
                self.gate(thread, &run, id, part, work, hands)
            }
            "gate" if !over => Taken::Wait,
            "gate" => {
                let row = row.unwrap_or_else(|| work.clone());
                let answer = row.answer.clone().unwrap_or_default();
                // green is a pass and nothing else: a check that could not run is no proof (law 16)
                if !(ok(&row) && answer.trim_end().ends_with(": passed")) {
                    let said = if ok(&row) { answer } else { why(&row) };
                    let n = said.chars().count();
                    return self.stop(thread, &run, id, &format!("its gate {} is red: {}", part.gate_words(), said.chars().skip(n.saturating_sub(300)).collect::<String>()));
                }
                if self.mode() == Mode::Show {
                    self.step(thread, &run, id, "ready", &row.id, "", "gate green");
                    self.note(thread, &format!("◆ part {id} · gate {} green · `:parts next` commits it and goes on", part.gate_words()), "good");
                    return Taken::Wait;
                }
                self.commit(thread, &run, n, work, hands)
            }
            "ready" => Taken::Wait,
            "go" => self.commit(thread, &run, n, work, hands),
            "commit" if !over => Taken::Wait,
            "commit" => {
                let row = row.unwrap_or_else(|| work.clone());
                let answer = row.answer.clone().unwrap_or_default();
                if !ok(&row) {
                    return self.stop(thread, &run, id, &format!("its commit failed: {}", why(&row)));
                }
                let sha = answer.strip_prefix("committed ").and_then(|a| a.split_whitespace().next()).unwrap_or_default().to_string();
                self.step(thread, &run, id, "done", &row.id, &sha, &answer);
                let short: String = sha.chars().take(7).collect();
                let result = if sha.is_empty() { format!("gate green; {}", clip(&answer, 200)) } else { format!("committed {short}") };
                self.note(thread, &format!("◆ part {id} · gate green · {result}"), "good");
                Taken::Settle { state: "merged", result, block: "" }
            }
            "done" => Taken::Settle { state: "merged", result: format!("committed {}", step.sha.chars().take(7).collect::<String>()), block: "" },
            _ => Taken::Settle { state: "blocked", result: "the run stopped here".into(), block: "parts" },
        }
    }

    fn boundary(&self, thread: &str, item: &Item) -> Option<Boundary> {
        let (run, n) = self.part_of(thread, item)?;
        let t = self.thread(thread);
        let (id, part) = &run.items[n];
        let mut text = format!("document: {} (run {}, part {} of {})\nnow: {id} — {}\ngate: {} (J4n runs it when you end, and commits what you changed only if it is green)", run.doc, run.id, n + 1, run.items.len(), part.title(), part.gate_words());
        for (prev, p) in run.items.iter().take(n) {
            let done = t.steps.get(prev).map(|s| if s.sha.is_empty() { "gate green, nothing to commit".to_string() } else { format!("gate green, committed {}", s.sha.chars().take(7).collect::<String>()) }).unwrap_or_default();
            text.push_str(&format!("\ndone: {prev} — {} · {done}", p.title()));
        }
        let fresh = self.edge() == Edge::Reset;
        if fresh && n > 0 {
            let before: Vec<&Step> = run.items.iter().take(n).filter_map(|(prev, _)| t.steps.get(prev)).collect();
            let all = |f: fn(&Step) -> &Vec<String>| before.iter().flat_map(|s| f(s).iter().cloned()).collect::<Vec<_>>();
            let keep = MustKeep { goal: format!("the parts of {}, one at a time; this one is {id}", run.doc), changed: all(|s| &s.paths), decisions: all(|s| &s.decisions), prompts: all(|s| &s.prompts), ..MustKeep::default() };
            text.push_str(&format!("\n\n{}", keep.render()));
        }
        Some(Boundary { fresh, compact: self.edge() == Edge::Compact, brief: text })
    }

    fn command(&self, thread: &str, args: &[String], plan: Option<&Plan>) -> Result<String, String> {
        match args.first().map(String::as_str) {
            None => {
                let t = self.thread(thread);
                if t.parts.is_empty() { Err("no Draft: `:parts <document>` reads one".into()) } else { Ok(self.card(&t)) }
            }
            Some("go") => self.go(thread, plan),
            Some("next") => self.next(thread),
            Some(w @ ("keep" | "drop")) => self.edit(thread, w, &args[1..]),
            Some(_) => self.read(thread, &args.join(" ")),
        }
    }
}
