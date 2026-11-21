//! The doer `qc`: the rules `qc-live` and `qc-settle` start it for a task's work, `:qc` for a path or a git
//! range, and it does what the flow of the spec lists — the change, the first tier with no model, the second with Jev, the marks and
//! the score — and writes `qc.mark` and `qc.run`. It never blocks a lane: it is a task of its own, off the line's path.

use j4n_core::text::line as clip;
use crate::change::{Change, git};
use crate::facts::{self, Facts, Work};
use crate::judge::{self, Jev, Rubric};
use crate::part::{self, Goal};
use crate::pattern::{Ready, Way};
use crate::scan::{self, Mark};
use crate::score::{Weights, score};
use j4n_connect::connect::{Job, StepDoer};
use j4n_core::change::Actor;
use j4n_core::doer::Doer;
use j4n_core::log::{Log, New};
use j4n_core::place::checkpoint;
use j4n_core::redact::Redactor;
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

pub const DOER: &str = "qc";
/// Failures of Jev in a row after which the second tier stays off until `:qc on`.
const BREAKER: u32 = 3;
/// Why a mark is dismissed (`:qc dismiss m1a2b3c4 because intended`); only `wrong` says the pattern was wrong.
const REASONS: [&str; 4] = ["intended", "pre-existing", "wrong", "later"];

/// The settings and tables a run reads.
#[derive(Clone)]
pub struct Cfg {
    /// What stands before the first `:qc on` or `:qc off`.
    pub on: bool,
    pub jev: bool,
    pub precision: f64,
    pub steer_n: usize,
    pub clock_ms: u64,
    pub patterns: Vec<Ready>,
    pub rubrics: Vec<Rubric>,
    pub weights: Weights,
    /// Tag the edges of the state Jev is sent, and the marks that rest on them alone, `~lexical` (`qc.provenance`).
    pub provenance: bool,
    /// The surface the step `keep` guards.
    pub keeps: Vec<crate::keep::Rule>,
    pub root: PathBuf,
    pub home: PathBuf,
}

pub type Egress = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

#[derive(Clone)]
pub struct Quality {
    pub(crate) cfg: Arc<Cfg>,
    pub(crate) log: Log,
    red: Redactor,
    jev: Arc<OnceLock<(Arc<dyn Jev>, Egress)>>,
    fails: Arc<AtomicU32>,
}

/// What the words of a request ask for.
#[derive(Debug, Default, PartialEq)]
struct Ask {
    /// `live` or `settle` and the task, from a rule.
    task: Option<(String, String)>,
    switch: Option<bool>,
    /// `dismiss` or `keep`, the mark, and the reason (`dismiss … because <word>`, else empty).
    label: Option<(String, String, String)>,
    scope: String,
    goal: String,
    report: String,
    tiers: Option<u8>,
    json: bool,
}

/// Words, with a double-quoted run kept as one.
fn words(text: &str) -> Vec<String> {
    let (mut out, mut cur, mut quoted, mut any) = (Vec::new(), String::new(), false, false);
    for c in text.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn parse(goal: &str) -> Result<Ask, String> {
    let w = words(goal);
    let mut ask = Ask::default();
    match w.first().map(String::as_str) {
        Some(m @ ("@live" | "@settle")) => {
            let task = w.get(1).filter(|t| !t.is_empty()).ok_or("a rule's request names the task")?;
            ask.task = Some((m[1..].to_string(), task.trim_start_matches('@').to_string()));
            return Ok(ask);
        }
        Some("on") if w.len() == 1 => ask.switch = Some(true),
        Some("off") if w.len() == 1 => ask.switch = Some(false),
        Some(l @ ("dismiss" | "keep")) => {
            let id = w.get(1).ok_or_else(|| format!("`{l}` names a mark: `:qc {l} m1a2b3c4`"))?;
            let reason = match (l, w.get(2..).unwrap_or_default()) {
                ("dismiss", [b, r]) if b == "because" && REASONS.contains(&r.as_str()) => r.clone(),
                ("dismiss", [_, ..]) => return Err(format!("`because` takes one of {}: `:qc dismiss {id} because intended`", REASONS.join(" · "))),
                _ => String::new(),
            };
            ask.label = Some((l.to_string(), id.clone(), reason));
        }
        _ => {}
    }
    if ask.switch.is_some() || ask.label.is_some() {
        return Ok(ask);
    }
    let mut it = w.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| it.next().cloned().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "--goal" => ask.goal = value("--goal")?,
            "--report" => ask.report = value("--report")?,
            "--tiers" => {
                ask.tiers = Some(match value("--tiers")?.as_str() {
                    "0" => 0,
                    "0,1" | "1" => 1,
                    other => return Err(format!("--tiers is `0` or `0,1`, not `{other}`")),
                })
            }
            "--json" => ask.json = true,
            "--review" => {}
            s if s.starts_with("--") => return Err(format!("`{s}` is not an option of qc")),
            s if ask.scope.is_empty() => ask.scope = s.to_string(),
            s => return Err(format!("qc checks one thing at a time; `{s}` is a second")),
        }
    }
    Ok(ask)
}

/// A short, stable name: FNV-1a over the words (a nul between them), eight hex digits after `prefix`.
pub(crate) fn short(prefix: char, words: &[&str]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in words.iter().enumerate().flat_map(|(i, w)| (i > 0).then_some(0u8).into_iter().chain(w.bytes())) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{prefix}{:08x}", h & 0xffff_ffff)
}

/// A mark's name: over the pattern, the file and the line.
fn mark_id(m: &Mark) -> String {
    short('m', &[&m.pattern, &m.file, &m.line.to_string()])
}

impl Quality {
    pub fn new(cfg: Cfg, log: Log, red: Redactor) -> Quality {
        Quality { cfg: Arc::new(cfg), log, red, jev: Arc::default(), fails: Arc::default() }
    }

    /// Jev, and the check that a zone may be sent project code (the egress Gate).
    pub fn set_jev(&self, jev: Arc<dyn Jev>, egress: Egress) {
        let _ = self.jev.set((jev, egress));
    }

    pub fn doer(&self) -> Arc<dyn Doer> {
        let me = self.clone();
        Arc::new(StepDoer { id: DOER, doc: "checks the quality of code: marks with a place, a chance and a severity, and a score for display", work: Arc::new(move |job| me.run(job)) })
    }

    pub fn enabled(&self) -> bool {
        facts::read(&self.log, None).switch.unwrap_or(self.cfg.on)
    }

    pub(crate) fn put(&self, kind: &str, data: Json, task: Option<&str>, thread: Option<&str>) {
        let mut n = New::of(kind, Actor::Core, data);
        if let Some(t) = task {
            n = n.task(t);
        }
        if let Some(t) = thread {
            n = n.thread(t);
        }
        let _ = self.log.append(n);
    }

    fn run(&self, job: &Job) -> Result<String, String> {
        let ask = parse(&job.goal)?;
        if let Some(on) = ask.switch {
            self.put("qc.switch", json!({ "on": on }), None, job.cx.spec.thread.as_deref());
            if on {
                self.fails.store(0, Ordering::SeqCst);
            }
            return Ok(format!("qc is {}", if on { "on" } else { "off" }));
        }
        if let Some((label, id, reason)) = &ask.label {
            return self.label(label, id, reason, job.cx.spec.thread.as_deref());
        }
        let started = Instant::now();
        let work_id = ask.task.as_ref().map(|(_, t)| t.as_str());
        let facts = facts::read(&self.log, work_id);
        if work_id.is_some() && !facts.switch.unwrap_or(self.cfg.on) {
            return Ok("qc is off".into());
        }
        let dir = job.cx.site.dir.clone();
        let (change, goal, scope, work, trigger) = match &ask.task {
            Some((trigger, t)) => {
                let w = facts.work.clone().unwrap_or_default();
                if w.paths.is_empty() && w.deleted.is_empty() {
                    return Ok(format!("nothing changed in {t}"));
                }
                let base = checkpoint::base_ref(&self.cfg.home, &self.cfg.root, t);
                let mut paths = w.paths.clone();
                paths.extend(w.deleted.iter().cloned());
                let goal = Goal { line: w.goal.clone(), answer_head: w.answer.clone(), check: w.check.clone(), ..Goal::default() };
                (Ok(Change::of_task(&dir, base, &paths)), goal, format!("task {t}"), Some(w), trigger.clone())
            }
            None => {
                let report = if ask.report.is_empty() { String::new() } else { crate::change::inside(&ask.report).then(|| std::fs::read_to_string(dir.join(&ask.report)).ok()).flatten().unwrap_or_default() };
                let goal = Goal { line: ask.goal.clone(), answer_head: report, ..Goal::default() };
                let scope = if ask.scope.is_empty() { ".".to_string() } else { ask.scope.clone() };
                (self.change_of(&dir, &scope), goal, scope, None, "ask".into())
            }
        };
        let change = match change {
            Ok(c) => c,
            Err(why) => {
                self.put("qc.run", json!({ "run": job.cx.task, "scope": scope, "trigger": trigger, "score": null, "status": "not run", "errors": [{ "tier": 0, "reason": why }], "ms": started.elapsed().as_millis() as u64 }), Some(&job.cx.task), job.cx.spec.thread.as_deref());
                return Err(format!("qc not run: {why}"));
            }
        };
        let report = self.check(job, &ask, &facts, &change, &goal, &scope, &trigger, work.as_ref(), started);
        Ok(report)
    }

    fn change_of(&self, dir: &std::path::Path, scope: &str) -> Result<Change, String> {
        let plain = crate::change::inside(scope.trim_end_matches('/')) || scope == ".";
        let exists = plain && dir.join(scope).exists();
        if exists || !plain {
            return if exists { Change::of_path(dir, scope) } else { Change::of_range(dir, scope) };
        }
        if git(dir, &["rev-parse", "--verify", "--quiet", &format!("{scope}^{{commit}}")]).is_some() || scope.contains("..") {
            return Change::of_range(dir, scope);
        }
        Err(format!("`{scope}` is neither a path here nor a git range"))
    }

    #[allow(clippy::too_many_arguments)]
    fn check(&self, job: &Job, ask: &Ask, facts: &Facts, change: &Change, goal: &Goal, scope: &str, trigger: &str, work: Option<&Work>, started: Instant) -> String {
        let mut errors: Vec<Json> = Vec::new();
        let mut skipped: Vec<Json> = Vec::new();
        // the first tier
        let mut marks = scan::regex(&self.cfg.patterns, change);
        for r in self.cfg.patterns.iter().filter(|r| r.p.kind == Way::Cmd) {
            match crate::cmd::run(r, change) {
                Ok(m) => marks.extend(m),
                Err(why) => skipped.push(json!({ "pattern": r.p.id, "reason": why })),
            }
        }
        let parts = part::cut(change);
        let wants_jev = self.cfg.jev && ask.tiers != Some(0);
        let (mut rubrics, mut tokens, mut judged_parts) = (BTreeMap::<String, (f64, f64)>::new(), 0u64, 0usize);
        if wants_jev {
            match self.tier_one(&parts, change, goal, &marks) {
                Ok((judged, bad)) => {
                    let mut sums: BTreeMap<String, (f64, usize)> = BTreeMap::new();
                    let failed = bad.len();
                    for (i, j) in judged.into_iter().enumerate() {
                        if bad.iter().any(|(b, _)| *b == i) {
                            continue;
                        }
                        judged_parts += 1;
                        tokens += j.tokens;
                        marks.extend(j.marks);
                        for (k, v) in j.rubrics {
                            let e = sums.entry(k).or_default();
                            *e = (e.0 + v, e.1 + 1);
                        }
                    }
                    for (k, (sum, n)) in sums {
                        let top = self.cfg.rubrics.iter().find(|r| r.id == k).map_or(1.0, |r| (r.levels.len().max(2) - 1) as f64);
                        rubrics.insert(k, (sum / n as f64, top));
                    }
                    for (_, why) in &bad {
                        errors.push(json!({ "tier": 1, "reason": why }));
                    }
                    if failed == 0 {
                        self.fails.store(0, Ordering::SeqCst);
                    } else if failed == parts.len() {
                        self.fails.fetch_add(1, Ordering::SeqCst);
                    }
                }
                Err(why) => errors.push(json!({ "tier": 1, "reason": why })),
            }
        }
        marks.sort_by(|a, b| (&a.file, a.line, &a.pattern).cmp(&(&b.file, b.line, &b.pattern)));
        marks.dedup_by(|a, b| a.file == b.file && a.line == b.line && a.pattern == b.pattern);
        // what the operator has said, and what the numbers allow
        let live = trigger == "live";
        let mut shown: Vec<Json> = Vec::new();
        let mut open: Vec<&Mark> = Vec::new();
        for m in &marks {
            let id = mark_id(m);
            let (prec, n) = facts.precision(&m.pattern);
            let dismissed = facts.dismissed.contains(&id);
            let r = self.cfg.patterns.iter().find(|r| r.p.id == m.pattern);
            let steerable = live && !dismissed && m.severity == crate::pattern::Severity::High && r.is_some_and(|r| r.p.kind != Way::Jev) && n >= self.cfg.steer_n && prec >= self.cfg.precision;
            let steer = r.map(|r| r.steer_at(&m.file, m.line, &m.answer)).unwrap_or_default();
            // a verdict that picks among the look-alikes rests on the names a word search found, and says so
            let basis = if self.cfg.provenance && m.tier == 1 && r.is_some_and(|r| r.p.ty.as_deref() == Some("choice")) { part::LEXICAL } else { "" };
            let mut row = json!({
                "mark": id, "run": job.cx.task, "work": work.map(|w| w.task.as_str()), "live": live, "pattern": m.pattern,
                "title": r.map(|r| r.p.title.as_str()).unwrap_or_default(), "file": m.file, "line": m.line, "p": m.p,
                "severity": m.severity.word(), "tier": m.tier, "answer": m.answer, "steer": steer, "steerable": steerable,
                "prec": prec, "n": n, "state": if dismissed { "dismissed" } else { "open" },
            });
            if !basis.is_empty() {
                row["basis"] = json!(basis);
            }
            shown.push(row);
            if !dismissed {
                open.push(m);
            }
        }
        let tier_one_ran = wants_jev && judged_parts > 0;
        let status = if tier_one_ran && judged_parts == parts.len() { "full" } else if tier_one_ran { "partial" } else { "0-only" };
        let coverage = if !wants_jev || parts.is_empty() { 100 } else { judged_parts * 100 / parts.len() };
        let failed_check = goal.check == "fail";
        let value = (!change.files.is_empty()).then(|| score(&self.cfg.weights, &open.iter().map(|m| (*m).clone()).collect::<Vec<_>>(), &rubrics, failed_check));
        let ms = started.elapsed().as_millis() as u64;
        let (task, thread) = (work.map(|w| w.task.as_str()), work.and_then(|w| w.thread.as_deref()).or(job.cx.spec.thread.as_deref()));
        for d in &shown {
            self.put("qc.mark", d.clone(), task.or(Some(&job.cx.task)), thread);
        }
        let status = if value.is_none() { "not run" } else { status };
        self.put(
            "qc.run",
            json!({
                "run": job.cx.task, "work": task, "scope": scope, "trigger": trigger, "parts": parts.len(), "checked": change.files.len(), "skipped": skipped,
                "errors": errors, "rubrics": rubrics.iter().map(|(k, (at, top))| (k.clone(), json!({ "at": at, "top": top }))).collect::<BTreeMap<_, _>>(),
                "score": value, "status": status, "coverage": coverage, "ms": ms, "tokens": tokens, "marks": open.len(), "check_failed": failed_check,
            }),
            task.or(Some(&job.cx.task)),
            thread,
        );
        self.report(ask, &shown, value, status, coverage, parts.len(), ms, tokens, &errors, &skipped)
    }

    /// The second tier for every part, or why it did not run.
    fn tier_one(&self, parts: &[part::Part], change: &Change, goal: &Goal, marks: &[Mark]) -> Result<(Vec<judge::Judged>, judge::Failed), String> {
        let (jev, egress) = self.jev.get().ok_or("Jev is not connected (no key, or the pack jev is off)")?;
        if self.fails.load(Ordering::SeqCst) >= BREAKER {
            return Err(format!("Jev failed {BREAKER} times in a row; `:qc on` tries it again"));
        }
        egress(&jev.zone())?;
        if parts.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }
        let states: Vec<(part::Part, Json)> = parts
            .iter()
            .map(|p| {
                let mut s = part::state(change, p, goal, marks);
                if self.cfg.provenance {
                    part::tag(&mut s);
                }
                self.red.scrub_json(&mut s);
                (p.clone(), s)
            })
            .collect();
        Ok(judge::judge(jev.as_ref(), &self.cfg.patterns, &self.cfg.rubrics, &states, Duration::from_millis(self.cfg.clock_ms)))
    }

    #[allow(clippy::too_many_arguments)]
    fn report(&self, ask: &Ask, marks: &[Json], value: Option<f64>, status: &str, coverage: usize, parts: usize, ms: u64, tokens: u64, errors: &[Json], skipped: &[Json]) -> String {
        let open: Vec<&Json> = marks.iter().filter(|m| m["state"] == "open").collect();
        if ask.json {
            let mut lines: Vec<String> = open
                .iter()
                .map(|m| {
                    let mut row = json!({ "pattern": m["pattern"], "file": m["file"], "line": m["line"], "p": m["p"], "severity": m["severity"], "tier": m["tier"] });
                    if let Some(b) = m.get("basis") {
                        row["basis"] = b.clone();
                    }
                    row.to_string()
                })
                .collect();
            lines.push(json!({ "run": { "score": value, "coverage": coverage, "parts": parts, "checked": marks.len(), "skipped": skipped.len(), "errors": errors.len(), "ms": ms, "tokens": tokens } }).to_string());
            return lines.join("\n");
        }
        let mut lines: Vec<String> = open.iter().map(|m| format!("{}  {}:{} {}  {:.2}{}", m["pattern"].as_str().unwrap_or_default(), m["file"].as_str().unwrap_or_default(), m["line"], m["severity"].as_str().unwrap_or_default(), m["p"].as_f64().unwrap_or(1.0), m["basis"].as_str().map(|b| format!(" {b}")).unwrap_or_default())).collect();
        let tag = if status == "full" { String::new() } else { format!(" {status}") };
        lines.push(match value {
            Some(v) => format!("qc {v:.1}{tag} · coverage {coverage}% · parts {parts} · {:.1}s", ms as f64 / 1000.0),
            None => "qc not run: nothing here to check".to_string(),
        });
        lines.extend(errors.iter().map(|e| format!("  tier {} did not run: {}", e["tier"], clip(e["reason"].as_str().unwrap_or_default(), 160))));
        lines.extend(skipped.iter().map(|s| format!("  {} skipped: {}", s["pattern"].as_str().unwrap_or_default(), clip(s["reason"].as_str().unwrap_or_default(), 160))));
        lines.join("\n")
    }

    /// `d` and `k` on a mark: a label, and the mark's new state.
    fn label(&self, label: &str, id: &str, reason: &str, thread: Option<&str>) -> Result<String, String> {
        let f = facts::read(&self.log, None);
        let Some(mark) = f.marks.get(id) else { return Err(format!("there is no mark `{id}`")) };
        let word = if label == "dismiss" { "dismissed" } else { "kept" };
        let pattern = mark["pattern"].as_str().unwrap_or_default();
        let model = if mark["tier"] == 1 { self.jev.get().map(|(j, _)| j.model()).unwrap_or_default() } else { String::new() };
        let mut n = New::of("label", Actor::Operator, json!({ "subject_kind": "qc_mark", "subject": id, "producer": pattern, "model": model, "point": reason, "label": word, "weight": 1.0 }));
        if let Some(t) = thread {
            n = n.thread(t);
        }
        let _ = self.log.append(n);
        let mut state = mark.clone();
        state["state"] = json!(if label == "dismiss" { "dismissed" } else { "open" });
        state["reason"] = json!(reason);
        let work = mark["work"].as_str().map(String::from);
        self.put("qc.mark.state", state, work.as_deref(), thread);
        Ok(format!("{word} {id} ({pattern}){}", if reason.is_empty() { String::new() } else { format!(" because {reason}") }))
    }
}
