//! The shaper, as a doer (`shape`): the rule `shape-draft` starts it for a draft, and it does what the
//! flow of the spec lists — the state, the shaper model, the checks without a model, the judge of another family, at most two rounds —
//! and writes `shape.result`. The models are doers it asks (as the bench asks the doer under test), with the persona in the Brief of
//! the request, so any connector of the dialects `openai` and `anthropic` can be the shaper, and so can a script.

use crate::check::{self, Dropped, Facts, Item, Limits};
use crate::desk::{Desk, Outcome};
use crate::state;
use j4n_connect::connect::{Job, StepDoer};
use j4n_core::change::Actor;
use j4n_core::doer::{Doer, Emit, RunCtl, RunEv};
use j4n_core::log::{Log, New};
use j4n_core::task::Mode;
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

pub const DOER: &str = "shape";
/// Failures in a row after which the shaper stays off until `:shape on`.
const BREAKER: u32 = 3;
/// The judge sends a draft back once.
const ROUNDS: u32 = 2;

/// The settings the shaper reads.
#[derive(Clone, Default)]
pub struct Cfg {
    /// `<doer>[:<model>]` of the shaper, and of the judge (`off`: none).
    pub model: String,
    pub judge: String,
    pub style: String,
    pub tone: String,
    pub format: String,
    pub words: String,
    pub max_tokens: usize,
    pub timeout_ms: u64,
    /// The commands of the project's `[[check]]` rows.
    pub checks: Vec<String>,
    /// The texts of the pack's `persona` rows, by id.
    pub personas: BTreeMap<String, String>,
}

#[derive(Clone)]
pub struct Shaper {
    cfg: Arc<Cfg>,
    desk: Arc<Desk>,
    log: Log,
    doers: Arc<OnceLock<Vec<Arc<dyn Doer>>>>,
}

/// `claude-cli:haiku` → the doer and the model it is asked for.
fn split(spec: &str) -> (&str, Option<&str>) {
    match spec.split_once(':') {
        Some((d, m)) => (d, Some(m).filter(|m| !m.is_empty())),
        None => (spec, None),
    }
}

/// What one request to a model came to.
struct Asked {
    answer: String,
    usd: f64,
    tokens: u64,
}

impl Shaper {
    pub fn new(cfg: Cfg, desk: Arc<Desk>, log: Log) -> Shaper {
        Shaper { cfg: Arc::new(cfg), desk, log, doers: Arc::default() }
    }

    /// The doers a model may be asked through, once the front is made.
    pub fn set_doers(&self, doers: Vec<Arc<dyn Doer>>) {
        let _ = self.doers.set(doers);
    }

    pub fn doer(&self) -> Arc<dyn Doer> {
        let me = self.clone();
        Arc::new(StepDoer { id: DOER, doc: "shapes the line in the composer: notes with sources, checked without a model", work: Arc::new(move |job| me.run(job)) })
    }

    fn arm(&self, spec: &str, what: &str) -> Result<(Arc<dyn Doer>, Option<String>), String> {
        let (id, model) = split(spec);
        if id.is_empty() {
            return Err(format!("{what} is not set"));
        }
        let arm = self.doers.get().and_then(|d| d.iter().find(|d| d.id() == id)).ok_or_else(|| j4n_core::doctor::no_doer(what, id))?;
        Ok((arm.clone(), model.map(String::from)))
    }

    /// One request to a model, with the persona in its Brief and a time to answer in.
    fn ask(&self, job: &Job, arm: &Arc<dyn Doer>, model: Option<&str>, persona: &str, prompt: &str) -> Result<Asked, String> {
        let (emit, rx) = Emit::channel();
        let mut cx = job.cx.nested(prompt, emit);
        cx.spec.doer = arm.id().to_string();
        cx.spec.thread = None;
        cx.spec.account = None;
        cx.spec.tune.model = model.map(String::from);
        cx.spec.tune.mode = Mode::Read;
        cx.spec.input = json!({ "brief": persona });
        let mut run = arm.start(cx).map_err(|p| p.msg)?;
        let until = Instant::now() + Duration::from_millis(self.cfg.timeout_ms);
        let (mut streamed, mut usd, mut tokens) = (String::new(), 0.0, 0);
        loop {
            if Instant::now() >= until {
                let _ = run.ctl(RunCtl::Kill);
                return Err(format!("{} did not answer in {} ms", arm.id(), self.cfg.timeout_ms));
            }
            if matches!(job.rx.try_recv(), Ok(RunCtl::Abort | RunCtl::Kill)) {
                let _ = run.ctl(RunCtl::Kill);
                return Err("stopped".into());
            }
            match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(RunEv::Text(t)) => streamed.push_str(&t),
                Ok(RunEv::Message { text, .. }) => streamed = text,
                Ok(RunEv::Metrics(m)) => {
                    usd += m.usd;
                    tokens += m.tokens_in + m.tokens_out;
                }
                Ok(RunEv::End { answer, .. }) => return Ok(Asked { answer: answer.unwrap_or(streamed), usd, tokens }),
                Ok(RunEv::Error(why)) => return Err(why),
                // a shaper has no tools; a question it asks is not answered
                Ok(RunEv::Ask(spec)) => {
                    let _ = run.ctl(RunCtl::Answer { local: spec.local, answer: j4n_core::ask::Answer::No });
                }
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Err("the run went away".into()),
            }
        }
    }

    fn persona(&self, id: &str) -> String {
        self.cfg.personas.get(id).cloned().unwrap_or_default()
    }

    fn run(&self, job: &Job) -> Result<String, String> {
        let hash = job.goal.trim().to_string();
        let Some(draft) = self.desk.claim(&hash) else { return Ok("nothing to shape".into()) };
        let started = Instant::now();
        let result = if self.desk.fails.load(Ordering::SeqCst) >= BREAKER { Err(format!("the shaper failed {BREAKER} times in a row and is off until `:shape on`")) } else { self.shape(job, &draft, started) };
        match &result {
            Ok(_) => self.desk.fails.store(0, Ordering::SeqCst),
            Err(_) => {
                self.desk.fails.fetch_add(1, Ordering::SeqCst);
            }
        }
        let data = match &result {
            Ok(o) => json!({
                "draft_hash": hash, "verdict": o.verdict, "rounds": o.rounds, "ms": o.ms, "tokens": o.tokens, "usd": o.usd, "by": o.by, "judge": o.judge,
                "items": o.items.iter().map(|i| json!({ "key": i.key, "kind": i.kind, "text": i.text, "source": i.source })).collect::<Vec<_>>(),
                "dropped": o.dropped.iter().map(|d| json!({ "text": d.text, "reason": d.reason })).collect::<Vec<_>>(),
            }),
            Err(why) => json!({ "draft_hash": hash, "verdict": "not_run", "reason": why, "ms": started.elapsed().as_millis() as u64 }),
        };
        let line = match &result {
            Ok(o) => format!("shape: {} · kept {} · dropped {}", o.verdict, o.items.len(), o.dropped.len()),
            Err(why) => format!("shape: not run ({why})"),
        };
        self.desk.finish(&hash, result);
        let mut ev = New::of("shape.result", Actor::Doer(DOER.into()), data).task(&job.cx.task);
        if !draft.thread.is_empty() {
            ev = ev.thread(&draft.thread);
        }
        self.log.append(ev).map_err(|p| p.msg)?;
        Ok(line)
    }

    fn shape(&self, job: &Job, draft: &j4n_core::front::shape::Draft, started: Instant) -> Result<Outcome, String> {
        let cfg = &self.cfg;
        let (shaper, shaper_model) = self.arm(&cfg.model, "shape.model")?;
        let judge = if cfg.judge.is_empty() || cfg.judge == "off" { None } else { Some(self.arm(&cfg.judge, "shape.judge")?) };
        let rewrite = cfg.style == "rewrite";
        let facts: Facts = state::facts(&self.log, &draft.thread, &draft.text, &job.cx.site.dir, &cfg.checks);
        let mut system = self.persona("shaper");
        if rewrite {
            system.push_str("\n\n");
            system.push_str(&self.persona(&format!("tone-{}", cfg.tone)));
        }
        if !cfg.words.trim().is_empty() {
            system.push_str(&format!("\n\nThe operator's own instruction: {}", cfg.words.trim()));
        }
        let (mut usd, mut tokens, mut back) = (0.0, 0, String::new());
        let limits = Limits { max_tokens: cfg.max_tokens, rewrite };
        for round in 1..=ROUNDS {
            let (mut prompt, _) = state::render(&facts, &draft.target, !draft.handoff, &cfg.words, &cfg.style, &cfg.tone, &cfg.format);
            prompt.push_str(&back);
            let asked = self.ask(job, &shaper, shaper_model.as_deref(), &system, &prompt)?;
            (usd, tokens) = (usd + asked.usd, tokens + asked.tokens);
            let reply = check::parse(&asked.answer)?;
            let mut checked = check::check(&reply, &facts, limits);
            let mut note = String::new();
            if let (Some((judge, judge_model)), false) = (&judge, checked.items.is_empty()) {
                let rows: Vec<Json> = checked.items.iter().map(|i| json!({ "id": i.key, "kind": i.kind, "text": i.text, "source": i.source })).collect();
                let ask = json!({ "line": draft.text, "turns": facts.turns.iter().map(|t| json!({ "id": t.id, "you": t.you, "answer_head": t.answer_head })).collect::<Vec<_>>(), "items": rows }).to_string();
                let said = self.ask(job, judge, judge_model.as_deref(), &self.persona("shape-judge"), &ask)?;
                (usd, tokens) = (usd + said.usd, tokens + said.tokens);
                let verdict = check::parse_any(&said.answer)?;
                let mut why_back = Vec::new();
                for v in verdict.get("items").and_then(Json::as_array).into_iter().flatten() {
                    if v.get("faithful").and_then(Json::as_bool) == Some(false) {
                        let id = v.get("id").and_then(Json::as_str).unwrap_or_default();
                        let why: String = v.get("why").and_then(Json::as_str).unwrap_or("not faithful").chars().take(80).collect();
                        if let Some(at) = checked.items.iter().position(|i| i.key == id) {
                            let gone = checked.items.remove(at);
                            why_back.push(format!("{}: {why}", gone.text));
                            checked.dropped.push(Dropped { text: gone.text, reason: format!("the judge: {why}") });
                        }
                    }
                }
                let changes = verdict.get("changes_ask").and_then(Json::as_bool) == Some(true);
                if changes && round < ROUNDS {
                    back = format!("\n\nThe judge sent the draft back: it changes what was asked. {}", why_back.join("; "));
                    continue;
                }
                if changes {
                    for gone in std::mem::take(&mut checked.items) {
                        checked.dropped.push(Dropped { text: gone.text, reason: "the judge: it changes the ask".into() });
                    }
                }
                note = if changes { "changes the ask".into() } else if checked.dropped.iter().any(|d| d.reason.starts_with("the judge")) { "some not faithful".into() } else { "faithful".into() };
            }
            let verdict = if checked.items.is_empty() { "pass" } else { reply.get("verdict").and_then(Json::as_str).filter(|v| *v != "pass").unwrap_or("add") };
            return Ok(Outcome {
                hash: draft.hash.clone(),
                verdict: verdict.into(),
                items: checked.items,
                dropped: checked.dropped,
                rounds: round,
                ms: started.elapsed().as_millis() as u64,
                tokens,
                usd,
                by: format!("{}{}", cfg.model, if judge.is_some() { format!(" / {}", cfg.judge) } else { String::new() }),
                judge: if note.is_empty() { String::new() } else { format!("{note} · {round} rounds") },
            });
        }
        Err("the shaper did not settle".into())
    }
}

impl Item {
    /// For a test: an item with just a kind and a text.
    pub fn plain(kind: &'static str, text: &str) -> Item {
        Item { key: kind.into(), kind, text: text.into(), source: String::new(), proof: "turn", options: Vec::new(), parts: Vec::new() }
    }
}
