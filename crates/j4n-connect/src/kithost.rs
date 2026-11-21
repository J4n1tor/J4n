//! The host calls of the kit: what J4n does when code in the `py` kernel asks. The kernel has no
//! key and no network; it writes `{"call", "payload"}` and waits for `{"ok", "value" | "error"}`. The set is closed: `jev_many`, `model`,
//! `need` (a package: [`Host::need`], served with the cell's task), and `tool` with `tools`, the bridge. Every call is looked at here, in this order, and the first
//! refusal ends it:
//!
//! 1. the name is one of the set; 2. `jev_many` only while the setting `kit.judge` is `on` for the project; 3. the shape (at most 24
//!    questions in a request, 400 requests); 4. the leak check (a string the redactor would change refuses the whole call, naming where,
//!    never the value: the request is built from what the agent gave, not from a redacted copy); 5. the egress Gate for the zone
//!    the call goes to; then the call.
//!
//! **The bridge**: `tools` lists the doer's offer and `tool` runs calls of one of its tools, each as a model's call is
//! (its own `gate()`, then its own `run()`): Allow runs, Ask and Block are refused with the question, untrusted words taint the task,
//! 200 calls a cell and 8 at once. What the offer holds is decided where it is made (`manifest.rs`); no tool is named here.
//!
//! Each call leaves a row for the `py.cell` event (what, how long, how much, what it cost), and [`spend`] sums them for the spend fold.

use j4n_core::text::clip;
use crate::decide::{Decider, paid_of};
use crate::model::doer::Tools;
use crate::model::{Call, Client, Msg, Req};
use j4n_core::doer::Metrics;
use j4n_core::gate::{Gate, ReadOnly};
use j4n_core::place::{Launch, Site};
use j4n_core::redact::{MAX_DEPTH, Redactor};
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// The closed set of host calls.
pub const CALLS: [&str; 5] = ["jev_many", "model", "need", "tool", "tools"];
/// The most tool calls in one cell.
pub const MAX_TOOL_CALLS: usize = 200;
/// The most questions in one request, the most requests in one call, and how many run at once.
pub const PER_REQUEST: usize = 24;
pub const MAX_REQUESTS: usize = 400;
pub const AT_ONCE: usize = 8;
const MAX_BYTES: usize = 2_000_000;
const SHOWN: usize = 8;
const ANSWER_MAX: u32 = 4096;
const SYSTEM: &str = "A program is calling you for one piece of work. Use only the prompt and the context it gives you. Be brief.";

/// Where a call may go: `Ok` or why not (the egress Gate, by zone).
pub type Egress = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

/// What the settings and the shared parts say: `kit.judge`, `kit.keep`, `kit.model`, `kit.grants`, the redactor, and the egress check.
#[derive(Clone)]
pub struct KitSet {
    /// J4n's own home, where the packages go (`<home>/py`); none, and `need` has no place to put one.
    pub home: std::path::PathBuf,
    /// The program that installs (`uv`), as a name on the `PATH` or a path.
    pub uv: String,
    /// The packages the operator granted ahead (`kit.grants`): an Ask for one of them is answered, a Block is not.
    pub grants: Vec<String>,
    /// The parts of the kit the operator turned off (`kit.off`): library functions by name, recipes as `recipe:<name>`.
    pub off: Vec<String>,
    pub judge: bool,
    pub keep: f64,
    pub model: String,
    pub red: Redactor,
    pub egress: Egress,
}

impl Default for KitSet {
    fn default() -> KitSet {
        KitSet { home: std::path::PathBuf::new(), uv: "uv".into(), grants: Vec::new(), off: Vec::new(), judge: false, keep: 0.15, model: "light".into(), red: Redactor::new(), egress: Arc::new(|_| Err("nothing checks where this call goes".into())) }
    }
}

/// The chat model of `model`: its client, the model asked for, and the zone its provider is in.
pub struct Chat {
    pub client: Client,
    pub model: String,
    pub zone: String,
}

pub struct Host {
    set: KitSet,
    jev: Option<Arc<Decider>>,
    chat: Option<Chat>,
    /// What code may call: the doer's offer less what is kept from code. The Gate told what a cell read from outside, if the pack `guard` is on.
    offer: Option<Arc<dyn Tools>>,
    guard: Option<Gate>,
}

/// What one cell is held to while it asks the bridge: how the task may write, its time, its calls so far, and the walls a program runs behind.
pub struct Cell<'a> {
    pub read_only: ReadOnly,
    pub until: Instant,
    pub used: &'a AtomicUsize,
    pub wrap: &'a (dyn Fn(Launch) -> Launch + Send + Sync),
}

/// Where in `v` there is a string (or a key) the redactor would change, as paths with no value in them.
fn leaks(red: &Redactor, v: &Json, path: &str, depth: usize, out: &mut Vec<String>) {
    if out.len() >= SHOWN || depth > MAX_DEPTH {
        return;
    }
    match v {
        Json::String(s) if red.scrub(s) != *s => out.push(red.scrub(path)),
        Json::Array(a) => a.iter().enumerate().for_each(|(i, x)| leaks(red, x, &format!("{path}[{i}]"), depth + 1, out)),
        Json::Object(m) => {
            for (k, x) in m {
                if red.scrub(k) != *k {
                    out.push(red.scrub(path));
                }
                leaks(red, x, &format!("{path}.{k}"), depth + 1, out);
            }
        }
        _ => {}
    }
}

/// The first JSON object in a model's reply, which may be fenced or have words around it.
fn object_in(text: &str) -> Option<Json> {
    let (start, end) = (text.find('{')?, text.rfind('}')?);
    serde_json::from_str::<Json>(text.get(start..=end)?).ok().filter(Json::is_object)
}

impl Host {
    pub fn new(set: KitSet) -> Host {
        Host { set, jev: None, chat: None, offer: None, guard: None }
    }

    pub fn with_jev(mut self, jev: Option<Arc<Decider>>) -> Host {
        self.jev = jev;
        self
    }

    pub fn with_chat(mut self, chat: Option<Chat>) -> Host {
        self.chat = chat;
        self
    }

    /// What code may call: the tools of the doer's offer that are fit for it.
    pub fn with_offer(mut self, offer: Option<Arc<dyn Tools>>) -> Host {
        self.offer = offer;
        self
    }

    /// The specs of what code may call, for the offer line.
    pub fn specs(&self) -> Vec<crate::model::Tool> {
        self.offer.as_ref().map(|o| o.specs()).unwrap_or_default()
    }

    /// The Gate that holds a task to the taint rules once a call brought back words that are not the operator's.
    pub fn with_guard(mut self, guard: Option<Gate>) -> Host {
        self.guard = guard;
        self
    }

    /// `<home>/py`, made, for the box to bind read-only; none when there is no home for packages.
    pub fn pydir(&self) -> Option<std::path::PathBuf> {
        crate::kitpkg::ready(&self.set.home)
    }

    /// The folder of `task`'s temp packages, if it has a home.
    pub fn temp_of(&self, task: &str) -> Option<std::path::PathBuf> {
        (!self.set.home.as_os_str().is_empty()).then(|| crate::kitpkg::dir_for(&self.set.home, task, true))
    }

    /// Whether the operator granted `name` ahead (`kit.grants`).
    pub fn granted(&self, name: &str) -> bool {
        self.set.grants.iter().any(|g| crate::kitpkg::canon(g) == crate::kitpkg::canon(name))
    }

    /// Remove the temp folders a J4n that ended badly left behind.
    pub fn sweep(&self) {
        if !self.set.home.as_os_str().is_empty() {
            crate::kitpkg::sweep(&self.set.home);
        }
    }

    /// Whether `name` is there already for `task`: nothing to ask about, nothing to install.
    pub fn has(&self, task: &str, name: &str, temp: bool) -> bool {
        !self.set.home.as_os_str().is_empty() && crate::kitpkg::present(&crate::kitpkg::dir_for(&self.set.home, task, temp), name)
    }

    /// `need`: a package for the cell's task. `asked` is what the Gate let through for this cell (`PyTools::gate`: names it allowed or the
    /// operator said yes to); a package that is already there needs no yes. The reply holds the folder to put on `sys.path`.
    pub fn need(&self, p: &Json, site: &Site, asked: &[(String, bool)], until: Instant) -> (Json, Json) {
        let began = Instant::now();
        let temp = p["temp"].as_bool().unwrap_or(false);
        let mut row = json!({ "call": "need", "name": clip(p["name"].as_str().unwrap_or_default(), 100), "temp": temp });
        let done = self.install(p, site, asked, temp, until, &mut row);
        row["ms"] = json!(began.elapsed().as_millis() as u64);
        row["ok"] = json!(done.is_ok());
        match done {
            Ok(v) => (json!({ "ok": true, "value": v }), row),
            Err(why) => {
                row["error"] = json!(clip(&why, 300));
                (json!({ "ok": false, "error": why }), row)
            }
        }
    }

    fn install(&self, p: &Json, site: &Site, asked: &[(String, bool)], temp: bool, until: Instant, row: &mut Json) -> Result<Json, String> {
        use crate::kitpkg as pkg;
        let name = p["name"].as_str().filter(|n| pkg::valid(n)).ok_or("need takes a package name and nothing else: letters, digits, `.`, `-` and `_`, no version, no extra, no address")?;
        if self.set.home.as_os_str().is_empty() {
            return Err("there is no folder for packages here (J4n's home is not set)".into());
        }
        let dir = pkg::dir_for(&self.set.home, &site.task, temp);
        let note = |size: u64| format!("{name} is ready{} ({}), on sys.path", if temp { " for this task only" } else { "" }, if size == 0 { "already there".to_string() } else { format!("installed, {}", pkg::bytes(size)) });
        if pkg::present(&dir, name) {
            return Ok(json!({ "path": dir, "note": note(0), "size": 0 }));
        }
        if !asked.contains(&(pkg::canon(name), temp)) {
            return Err(format!("`{name}` was not asked about, so it was not installed. Name it as a string in the cell, `need('{name}')`, one package a cell is asked about; the operator's yes comes before the cell runs"));
        }
        let uv = pkg::find(&self.set.uv).ok_or_else(|| format!("uv is not on this machine, so no package can be added; the standard library is here. Install uv (https://docs.astral.sh/uv/), or put `{name}` in {} yourself with `python3 -m pip install --only-binary :all: --target {} {name}` and call need again", dir.display(), dir.display()))?;
        let done = pkg::install(&uv, &dir, name, until)?;
        row["installed"] = json!(true);
        row["size"] = json!(done.size);
        Ok(json!({ "path": dir, "note": note(done.size), "size": done.size }))
    }

    /// The `kit.keep` line, which the library reads as its default.
    pub fn keep(&self) -> f64 {
        self.set.keep
    }

    /// Whether `kit.judge` is on for the project (`j4n.judge`, `choose`, `score` and `keep` work only then).
    pub fn judge(&self) -> bool {
        self.set.judge
    }

    /// The parts of the kit that are turned off (`kit.off`).
    pub fn off(&self) -> &[String] {
        &self.set.off
    }

    /// One call from the kernel: the reply to write back, and the row for the cell's event.
    pub fn serve(&self, name: &str, payload: &Json) -> (Json, Json) {
        let began = Instant::now();
        let mut row = json!({ "call": clip(name, 40) });
        let done = match name {
            "jev_many" => self.jev_many(payload, &mut row),
            "model" => self.chat(payload, &mut row),
            "need" => Err("need is served with the cell's task, by the tool py".into()),
            other => Err(format!("`{}` is not a host call; the calls are {}", clip(other, 40), CALLS.join(", "))),
        };
        row["ms"] = json!(began.elapsed().as_millis() as u64);
        match done {
            Ok(value) => {
                row["ok"] = json!(true);
                (json!({ "ok": true, "value": value }), row)
            }
            Err(why) => {
                row["ok"] = json!(false);
                row["error"] = json!(clip(&why, 300));
                (json!({ "ok": false, "error": why }), row)
            }
        }
    }

    fn refuse_leaks(&self, payload: &Json, root: &str) -> Result<(), String> {
        let mut at = Vec::new();
        leaks(&self.set.red, payload, root, 0, &mut at);
        if at.is_empty() {
            return Ok(());
        }
        Err(format!("refused, nothing was sent: {} hold what looks like a secret (at {}). Drop or cut those and call again", if at.len() == 1 { "a part" } else { "parts" }, at.join(", ")))
    }

    fn jev_many(&self, p: &Json, row: &mut Json) -> Result<Json, String> {
        if !self.set.judge {
            return Err("the decider is off for this project: the setting kit.judge is off, so nothing was sent. Set kit.judge to on if this project's text may go to Jev".into());
        }
        let calls = p["calls"].as_array().ok_or("jev_many needs `calls`: a list of {state, questions}")?;
        if calls.len() > MAX_REQUESTS || p.to_string().len() > MAX_BYTES {
            return Err(format!("too much for one call: at most {MAX_REQUESTS} requests and {MAX_BYTES} bytes"));
        }
        for (k, c) in calls.iter().enumerate() {
            let n = c["questions"].as_object().map_or(0, serde_json::Map::len);
            if n == 0 || n > PER_REQUEST {
                return Err(format!("request {k} holds {n} questions; between 1 and {PER_REQUEST} go in one"));
            }
        }
        let jev = self.jev.as_ref().ok_or("Jev is not connected (no key, or the pack jev is off)")?;
        self.refuse_leaks(&p["calls"], "calls")?;
        (self.set.egress)(&jev.zone_word())?;
        let next = AtomicUsize::new(0);
        let done: Mutex<Vec<Option<Result<Json, String>>>> = Mutex::new(vec![None; calls.len()]);
        std::thread::scope(|s| {
            for _ in 0..AT_ONCE.min(calls.len()) {
                s.spawn(|| {
                    loop {
                        let n = next.fetch_add(1, Ordering::SeqCst);
                        let Some(c) = calls.get(n) else { break };
                        let r = jev.batch(&c["state"], &c["questions"]);
                        done.lock().unwrap_or_else(|e| e.into_inner())[n] = Some(r);
                    }
                });
            }
        });
        let (mut usd, mut known, mut bad) = (0.0, true, 0);
        let (mut tin, mut tout) = (0, 0);
        let replies: Vec<Json> = done
            .into_inner()
            .unwrap_or_default()
            .into_iter()
            .map(|r| match r {
                Some(Ok(reply)) => {
                    let paid = paid_of(&reply);
                    (tin, tout) = (tin + paid.tokens_in, tout + paid.tokens_out);
                    match paid.usd {
                        Some(u) => usd += u,
                        None => known = false,
                    }
                    json!({ "answers": reply["answers"], "tokens": paid.tokens_in + paid.tokens_out })
                }
                other => {
                    bad += 1;
                    json!({ "error": other.and_then(Result::err).unwrap_or_else(|| "it did not run".into()) })
                }
            })
            .collect();
        let items: usize = calls.iter().map(|c| c["questions"].as_object().map_or(0, serde_json::Map::len)).sum();
        *row = json!({ "call": "jev_many", "requests": calls.len(), "items": items, "failed": bad, "tokens_in": tin, "tokens_out": tout, "usd": known.then_some(usd) });
        Ok(Json::Array(replies))
    }

    fn chat(&self, p: &Json, row: &mut Json) -> Result<Json, String> {
        let prompt = p["prompt"].as_str().filter(|s| !s.trim().is_empty()).ok_or("model needs a `prompt`")?;
        if p.to_string().len() > MAX_BYTES {
            return Err(format!("too much for one call: at most {MAX_BYTES} bytes"));
        }
        let chat = self.chat.as_ref().ok_or("no chat model is connected for the kit: the connector named by kit.model (light, in the pack jev) needs its key")?;
        self.refuse_leaks(p, "payload")?;
        (self.set.egress)(&chat.zone)?;
        let shape = p["shape"].as_object().filter(|m| !m.is_empty());
        let mut text = prompt.to_string();
        for (i, c) in p["context"].as_array().into_iter().flatten().enumerate() {
            text.push_str(&format!("\n\n[{}] {}", i + 1, c.as_str().map_or_else(|| c.to_string(), String::from)));
        }
        let system = match shape {
            Some(m) => format!("{SYSTEM} Reply with only a JSON object with exactly these keys: {}.", Json::Object(m.clone())),
            None => SYSTEM.to_string(),
        };
        let msgs = [Msg::User(text)];
        let req = Req { system: &system, max: ANSWER_MAX, ..Req::new(&chat.model, &msgs) };
        let reply = chat.client.send(&req, &mut |_| {}, &AtomicBool::new(false)).map_err(|f| f.words(0))?;
        let u = reply.usage;
        *row = json!({ "call": "model", "model": chat.model, "tokens_in": u.input, "tokens_out": u.output, "usd": u.usd });
        match shape {
            None => Ok(Json::String(reply.text)),
            Some(m) => {
                let obj = object_in(&reply.text).ok_or("its reply is not the JSON object asked for")?;
                match m.keys().find(|k| obj.get(*k).is_none()) {
                    Some(k) => Err(format!("its reply has no `{k}`")),
                    None => Ok(obj),
                }
            }
        }
    }

    /// Whether `name` is answered by [`Host::bridge`] (which runs on the cell's own thread, behind its walls) and not by [`Host::serve`].
    pub fn bridged(name: &str) -> bool {
        matches!(name, "tool" | "tools")
    }

    /// `tools` and `tool`: the reply to write back, and one row for each call that was made.
    pub fn bridge(&self, name: &str, p: &Json, site: &Site, cx: &Cell) -> (Json, Vec<Json>) {
        let began = Instant::now();
        let done = match (&self.offer, name) {
            (None, _) => Err("no tools are offered here, so there is nothing to call".to_string()),
            (Some(o), "tools") => Ok((Json::Array(o.specs().into_iter().map(|t| json!({ "name": t.name, "doc": t.doc, "schema": t.schema })).collect()), Vec::new())),
            (Some(o), _) => self.run_all(o, p, site, cx),
        };
        let row = |ok: bool| json!({ "call": name, "ms": began.elapsed().as_millis() as u64, "ok": ok });
        match done {
            Ok((value, rows)) => (json!({ "ok": true, "value": value }), if name == "tool" { rows } else { vec![row(true)] }),
            Err(why) => (json!({ "ok": false, "error": why }), vec![{ let mut r = row(false); r["error"] = json!(clip(&why, 300)); r }]),
        }
    }

    /// The calls of one tool, at most [`AT_ONCE`] together and [`MAX_TOOL_CALLS`] in the cell: a result for each, in order, and a row.
    fn run_all(&self, offer: &Arc<dyn Tools>, p: &Json, site: &Site, cx: &Cell) -> Result<(Json, Vec<Json>), String> {
        let tool = p["tool"].as_str().filter(|s| !s.trim().is_empty()).ok_or("tool needs the `tool` to call")?;
        let calls = p["calls"].as_array().ok_or("tool needs `calls`: a list of arguments, one for each call")?;
        let known = offer.specs().iter().any(|t| t.name == tool);
        let room = MAX_TOOL_CALLS.saturating_sub(cx.used.fetch_add(calls.len(), Ordering::SeqCst)).min(calls.len());
        let next = AtomicUsize::new(0);
        let done: Mutex<Vec<Option<(Json, Json)>>> = Mutex::new(vec![None; calls.len()]);
        std::thread::scope(|s| {
            for _ in 0..AT_ONCE.min(calls.len()) {
                s.spawn(|| {
                    loop {
                        let n = next.fetch_add(1, Ordering::SeqCst);
                        let Some(args) = calls.get(n) else { break };
                        let r = if !known {
                            refused(tool, args, format!("`{}` is not a tool that code may call", clip(tool, 40)))
                        } else if n >= room {
                            refused(tool, args, format!("at most {MAX_TOOL_CALLS} tool calls in one cell; this one was not made"))
                        } else {
                            self.one(offer, tool, args, n, site, cx)
                        };
                        done.lock().unwrap_or_else(|e| e.into_inner())[n] = Some(r);
                    }
                });
            }
        });
        let (replies, rows) = done.into_inner().unwrap_or_default().into_iter().flatten().unzip();
        Ok((Json::Array(replies), rows))
    }

    /// One call, held as a model's call is: leak walk, the tool's own Gate question (Ask and Block are refused), its run, and then what
    /// it brought back from outside taints the task.
    fn one(&self, offer: &Arc<dyn Tools>, tool: &str, args: &Json, n: usize, site: &Site, cx: &Cell) -> (Json, Json) {
        let began = Instant::now();
        let args = if args.is_null() { json!({}) } else { args.clone() };
        if began >= cx.until {
            return refused(tool, &args, "the cell's time was over before this call began".into());
        }
        if !args.is_object() {
            return refused(tool, &args, "the arguments of a call are a dict".into());
        }
        if let Err(why) = self.refuse_leaks(&args, "args") {
            return refused(tool, &args, why);
        }
        let call = Call { id: format!("kit{n}"), name: tool.into(), args, bad: None };
        if let Some(g) = offer.gate(&call, &site.task, site, cx.read_only) {
            let (word, how) = if g.risk >= 100 { ("blocks", "it is not allowed here") } else { ("asks before", "ask for it with the tool yourself first, or have it granted") };
            return refused(tool, &call.args, format!("the Gate {word} \"{}\": {how}", clip(&g.question, 200)));
        }
        let out = offer.run(&call, site, cx.wrap);
        let mut row = row_of(tool, &call.args, began);
        row["ok"] = json!(!out.error);
        if out.error {
            row["error"] = json!(clip(&out.text, 300));
        }
        if !out.changed.is_empty() {
            row["changed"] = json!(out.changed.iter().take(50).collect::<Vec<_>>());
        }
        if let Some(from) = offer.untrusted(&call) {
            row["taint"] = json!(self.guard.as_ref().is_some_and(|g| g.taint(&site.task, &from)));
            row["untrusted"] = json!(from);
        }
        (json!({ "text": out.text, "error": out.error }), row)
    }
}

/// The row of a call of the bridge: which tool, a digest of its arguments (never the arguments), and when it ended.
fn row_of(tool: &str, args: &Json, began: Instant) -> Json {
    let digest: String = Sha256::digest(args.to_string().as_bytes()).iter().take(6).map(|b| format!("{b:02x}")).collect();
    json!({ "call": "tool", "tool": clip(tool, 40), "args": digest, "ms": began.elapsed().as_millis() as u64 })
}

/// A call that was not made, and why: the cell gets `refused` and the Log gets the row.
fn refused(tool: &str, args: &Json, why: String) -> (Json, Json) {
    let mut row = row_of(tool, args, Instant::now());
    (row["ok"], row["refused"], row["error"]) = (json!(false), json!(true), json!(clip(&why, 300)));
    (json!({ "text": why, "refused": true }), row)
}

/// What the host calls of one cell cost, for the spend fold: the tokens, and the dollars when every call said them.
pub fn spend(cell: &Json) -> Option<Metrics> {
    let rows = cell["host"].as_array()?;
    let sum = |k: &str| rows.iter().filter_map(|r| r[k].as_u64()).sum::<u64>();
    let usd = rows.iter().filter_map(|r| r["usd"].as_f64()).filter(|u| u.is_finite() && *u > 0.0).sum();
    Some(Metrics { usd, tokens_in: sum("tokens_in"), tokens_out: sum("tokens_out"), ..Metrics::default() })
}
