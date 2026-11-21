//! What the frames of `claude -p --output-format stream-json` mean in J4n's words (dialect
//! `claude-stream`): one line in, the [`RunEv`]s it makes out. Tested
//! against recorded frames (`fixtures/claude-frames/`). Pure: no process, no clock, so a recorded turn replays the same way.
//!
//! - Text and thinking arrive as `stream_event` deltas and are said as [`RunEv::Text`] and [`RunEv::Reasoning`]; the settled
//!   `assistant` text block adds nothing when it was streamed, and says itself when it was not. What was said before a tool
//!   call is the loop's narration (it keeps the tail), and the `result` frame's text is the report.
//! - A `tool_result` names only its `tool_use_id`, so the name and the command come from the `tool_use` block of the same id.
//!   A Bash result is a [`RunEv::Shell`] with the real exit code.
//! - A `can_use_tool` request is a question the child waits on: an approval that carries the effect it asks about (the Gate
//!   answers it before the operator is asked), or an `AskUserQuestion`'s own questions, one at a time.

use crate::effects;
use j4n_core::ask::{AskShape, AskSpec, Opt};
use j4n_core::change::Actor;
use j4n_core::doer::{Limit, Metrics, Role, RunEv, Served};
use j4n_core::gate::{Action, ReadOnly, Verb, relay};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

/// The variable an account hands the child its `claude setup-token` token in.
pub const OAUTH_TOKEN_ENV: &str = "CLAUDE_CODE_OAUTH_TOKEN";

/// What the turn that is going on is for, so a permission question can say which effect it asks about.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub task: String,
    /// The folder the task works in.
    pub dir: PathBuf,
    pub read_only: ReadOnly,
    /// The turn shrinks the session: what it says of the window is not the window after (claude says that with its next call).
    pub compacting: bool,
}

/// A `can_use_tool` request the child is blocked on: the answer must carry its id (and the tool's input back).
#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    pub request_id: String,
    pub input: Value,
    /// `AskUserQuestion`'s own `questions`; empty for a plain permission question. The child is answered once, after the last.
    pub questions: Vec<Value>,
    /// `(question, answer)` so far, in order.
    pub answers: Vec<(String, String)>,
    /// What a pick of option number `n` of the question now open means (the label the tool defined).
    pub labels: Vec<String>,
    first: AskSpec,
    /// The line J4n runs in place of the one asked about, when it is a remote shell routed through the relay.
    pub routed: Option<String>,
}

impl Pending {
    /// The question to put to whoever answers now: the permission question, or the next unanswered one of the tool.
    pub fn spec(&mut self) -> Option<AskSpec> {
        let Some(q) = self.questions.get(self.answers.len()) else {
            self.labels.clear();
            return (self.answers.is_empty()).then(|| self.first.clone());
        };
        let text = q.get("question").and_then(Value::as_str).unwrap_or("question").to_string();
        let mut recommended = false;
        self.labels.clear();
        let mut options = Vec::new();
        for o in q.get("options").and_then(Value::as_array).into_iter().flatten().take(j4n_core::ask::OPTIONS_MAX) {
            let Some(label) = o.get("label").and_then(Value::as_str) else { continue };
            let hint = o.get("description").and_then(Value::as_str).filter(|d| !d.is_empty());
            // the tool's own label is what the child is answered with; the description only helps the person choose
            let first = !recommended && label.to_lowercase().contains("(recommended)");
            recommended |= first;
            self.labels.push(label.to_string());
            options.push(Opt {
                id: self.labels.len().to_string(),
                label: one_line(&hint.map_or_else(|| label.to_string(), |h| format!("{label} — {h}"))),
                recommended: first,
            });
        }
        let shape = if options.is_empty() { AskShape::Text } else { AskShape::Choice { options } };
        let local = format!("{}#{}", clip(&self.request_id, 40), self.answers.len() + 1);
        Some(AskSpec { local, question: clip(&text, 3000), shape, inband: true, point: None, act: None })
    }

    /// The tool's input for the response that lets it run: as asked, or with the routed line for a remote shell.
    pub fn allowed_input(&self) -> Value {
        let mut input = self.input.clone();
        if let (Some(line), Some(obj)) = (&self.routed, input.as_object_mut()) {
            obj.insert("command".into(), Value::String(line.clone()));
        }
        input
    }

    /// The `local` of the question open now.
    pub fn local(&self) -> String {
        if self.questions.is_empty() {
            self.first.local.clone()
        } else {
            format!("{}#{}", clip(&self.request_id, 40), self.answers.len() + 1)
        }
    }

    /// The tool's input with every answer collected, for the response that lets the tool run.
    pub fn answered_input(&self) -> Value {
        let mut input = self.input.clone();
        let answers: serde_json::Map<String, Value> = self.answers.iter().map(|(q, a)| (q.clone(), Value::String(a.clone()))).collect();
        if let Some(obj) = input.as_object_mut() {
            obj.insert("answers".into(), Value::Object(answers));
        }
        input
    }

    /// The text of the question now open (what the answer is filed under).
    pub fn asked(&self) -> String {
        self.questions.get(self.answers.len()).and_then(|q| q.get("question")).and_then(Value::as_str).unwrap_or("question").to_string()
    }
}

/// What one line of stdout made.
#[derive(Debug, Default)]
pub struct Line {
    pub evs: Vec<RunEv>,
    /// The session id the frame named (every frame of a real stream does).
    pub session: Option<String>,
    /// The `result` frame: the turn is over, the child stays.
    pub turn_end: bool,
    /// A `can_use_tool` request; its question is in `Pending::spec`.
    pub ask: Option<Pending>,
    /// The child gave up a question it asked (`control_cancel_request`).
    pub cancelled: Option<String>,
}

pub struct Translator {
    /// J4n's own program, which has `relay`, when the child was told of it.
    relay: Option<String>,
    /// `tool_use` id → (name, what the result is about: the command, or the path a write changes).
    tools: HashMap<String, (String, String)>,
    /// Streamed text not yet settled by an `assistant` block.
    streamed: String,
    /// Said since the last tool: the loop's tail, which the report is compared with.
    tail: String,
    /// `total_cost_usd` is the child's running total since it launched, not the turn's: the last total seen (less what a resumed session
    /// carried in), so a turn reports only what it added (a reused child would otherwise book every earlier turn again).
    spent: f64,
    /// On a resumed session `total_cost_usd` and `modelUsage` start from the session's saved cost state, which holds earlier turns, on other
    /// models too. What the child's own calls were by model, in units (see [`units`]), and the part of the first total that was not theirs.
    own: BTreeMap<String, f64>,
    carried: Option<f64>,
    /// The calls of the turn so far by message id (a message is said once for each block): the model and the units.
    calls: HashMap<String, (String, f64)>,
    /// The child runs on an explicit token: a 401 cannot be cured by the CLI's own refresh, so its first retry says so.
    explicit_token: bool,
    auth_reported: bool,
    junk: u32,
    /// The kinds of frame this run has said it cannot read.
    unread: BTreeSet<String>,
    /// What the last call of the model put in the window, and how large the window is until a `result` frame says.
    ctx_tokens: u64,
    /// What the turn's first call put in the window.
    ctx_first: u64,
    ctx_window: u64,
    /// What the session holds after a compaction, as the CLI's `compact_boundary` frame says it (`compact_metadata.post_tokens`). None: it did not.
    compact_after: Option<u64>,
    /// The doer's own list, folded from its list tools, in the order the items were made.
    list: Vec<Item>,
    /// What the last `RunEv::List` said, so that only a change is said again.
    listed: (u32, u32, String),
    /// The model that serves the child: the full id the `init` frame names, then the one `modelUsage` shows did the turn.
    model: String,
    /// The model was said to the turn that is going on.
    said: bool,
    /// The calls being written, by the stream they are of (a sub-agent's has its own) and their index in the message (live only).
    writing: HashMap<(String, u64), Writing>,
    /// What the model has written in this turn so far, by message (live only), what the messages that ended counted, and what was last said of it.
    outs: BTreeMap<String, Spoken>,
    out_done: u64,
    out_told: u64,
}

/// A call that is still being written: its name (empty until the stream or a key says), the arguments so far and what was last said of it.
struct Writing {
    name: String,
    raw: String,
    target: String,
    told: Option<usize>,
}

/// What one message has written so far: the bytes of its words, reasoning and arguments, and the tokens the stream counted when it says (the
/// closing `message_delta`; before that the bytes are all there is, a token being about four of them).
#[derive(Default)]
struct Spoken {
    bytes: u64,
    real: u64,
}

impl Spoken {
    fn tokens(&self) -> u64 {
        if self.real > 0 { self.real } else { self.bytes / 4 }
    }
}

/// How many bytes of a call's arguments, and how many tokens, are written between two of the live words that say them.
const OPEN_STEP: usize = 512;
const TOKENS_STEP: u64 = 40;

/// An item of the doer's list: claude's number for it (from 1), whether it is open, whether it is the one in progress, and its words.
struct Item {
    id: String,
    open: bool,
    doing: bool,
    words: String,
}

/// The window of a model until the CLI says otherwise.
const DEFAULT_WINDOW: u64 = 200_000;
/// The kinds of frame that are read, or that are heard and deliberately left alone: any other kind is a stream this J4n cannot read.
const HEARD: [&str; 16] = [
    "assistant", "user", "result", "system", "stream_event", "control_request", "control_response", "control_cancel_request", "rate_limit_event", "keep_alive", "tool_progress", "auth_status", "tool_use_summary", "prompt_suggestion",
    "streamlined_text", "streamlined_tool_use_summary",
];

/// A usage as input tokens at the list prices' ratios: a read of the cache is a tenth, a write of it a quarter more, an output five.
const USAGE: [&str; 4] = ["input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens", "output_tokens"];
/// The same four, as a `modelUsage` entry names them.
const ENTRY: [&str; 4] = ["inputTokens", "cacheReadInputTokens", "cacheCreationInputTokens", "outputTokens"];

fn units(usage: Option<&Value>, keys: [&str; 4]) -> f64 {
    let n = |i: usize| usage.and_then(|u| u.get(keys[i])).and_then(Value::as_f64).unwrap_or(0.0);
    n(0) + 0.1 * n(1) + 1.25 * n(2) + 5.0 * n(3)
}

/// A model without the suffix of its window (`claude-opus-5-5[1m]`).
fn model_key(m: &str) -> &str {
    m.split('[').next().unwrap_or(m)
}

/// What one call of the model put in the window: fresh input and both cache buckets.
fn in_window(usage: Option<&Value>) -> u64 {
    let n = |k: &str| usage.and_then(|u| u.get(k)).and_then(Value::as_u64).unwrap_or(0);
    n("input_tokens") + n("cache_read_input_tokens") + n("cache_creation_input_tokens")
}

/// The first `max` characters of `s`, with an ellipsis when there were more.
pub fn clip(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

fn one_line(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

fn text_of(block: &Value) -> String {
    match block.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// The exit code of a Bash result: claude says `Exit code N` first in the text of a failed command, and flags the result.
/// A failure that names no code is 1.
pub fn bash_exit(output: &str, is_error: bool) -> (i32, &str) {
    if !is_error {
        return (0, output);
    }
    let (first, rest) = output.split_once('\n').unwrap_or((output, ""));
    match first.strip_prefix("Exit code ").and_then(|n| n.trim().parse::<i32>().ok()) {
        Some(code) => (code, rest),
        None => (1, output),
    }
}

/// The variable of the child's environment that names J4n's own program (the one that has `relay`).
pub const RELAY_ENV: &str = "J4N_RELAY";

/// The effect a tool asks about, as the Gate knows effects; `None` for what is not one (the operator is asked).
pub fn effect(tool: &str, input: &Value, turn: &Turn) -> Option<Action> {
    let text = |k: &str| input.get(k).and_then(Value::as_str);
    let actor = Actor::Doer("claude".into());
    let mut path_verb = None;
    let mut act = match tool {
        "Bash" => Action::shell(actor, text("command")?),
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
            path_verb = Some(Verb::Write);
            Action::new(Verb::Write, actor, text("file_path").or(text("notebook_path"))?)
        }
        "Read" | "Glob" | "Grep" | "LS" => {
            path_verb = Some(Verb::Read);
            Action::new(Verb::Read, actor, text("file_path").or(text("path")).unwrap_or("."))
        }
        "WebFetch" => Action::new(Verb::Net, actor, &effects::host_of(text("url")?)),
        "WebSearch" => Action::new(Verb::Net, actor, "web.search"),
        "Task" | "Agent" => Action::new(Verb::Spawn, actor, text("subagent_type").unwrap_or("agent")),
        _ => return None,
    };
    act.task = Some(turn.task.clone());
    act.site = Some(turn.dir.display().to_string());
    act.read_only = turn.read_only;
    if path_verb.is_some() {
        act.inside = Some(effects::inside(&turn.dir, &act.target));
    }
    Some(act)
}

fn risk_of(act: Option<&Action>) -> u8 {
    match act.map(|a| a.verb) {
        Some(Verb::Read) => 10,
        Some(Verb::Net | Verb::Spawn) => 30,
        Some(Verb::Shell) => 50,
        _ => 40,
    }
}

impl Translator {
    pub fn new(env: &std::collections::BTreeMap<String, String>) -> Translator {
        Translator {
            relay: env.get(RELAY_ENV).filter(|v| !v.is_empty()).cloned(),
            tools: HashMap::new(),
            streamed: String::new(),
            tail: String::new(),
            spent: 0.0,
            own: BTreeMap::new(),
            carried: None,
            calls: HashMap::new(),
            explicit_token: env.get(OAUTH_TOKEN_ENV).is_some_and(|v| !v.is_empty()),
            auth_reported: false,
            junk: 0,
            unread: BTreeSet::new(),
            ctx_tokens: 0,
            ctx_first: 0,
            ctx_window: DEFAULT_WINDOW,
            compact_after: None,
            list: Vec::new(),
            listed: (0, 0, String::new()),
            model: String::new(),
            said: false,
            writing: HashMap::new(),
            outs: BTreeMap::new(),
            out_done: 0,
            out_told: 0,
        }
    }

    /// The model that serves, said to the turn once and again whenever it changes: `run.served` with the full id and the host
    /// (claude is Anthropic's own). A child is kept between turns, so each turn hears it once, at its first frame that knows it.
    fn serve(&mut self, model: Option<&str>, out: &mut Line) {
        let model = model.filter(|m| !m.is_empty() && !m.starts_with('<'));
        if model.is_none_or(|m| m == self.model) && (self.said || self.model.is_empty()) {
            return;
        }
        if let Some(m) = model {
            self.model = m.to_string();
        }
        self.said = true;
        out.evs.push(RunEv::Served(Served { model: self.model.clone(), host: Some("anthropic".into()), ..Served::default() }));
    }

    fn boundary(&mut self) {
        self.streamed.clear();
        self.tail.clear();
    }

    /// Translate one line of stdout. A line that is not JSON is said (the first few), never dropped silently. `turn` is
    /// what is going on now; between turns it is `None` and a question cannot be asked.
    pub fn line(&mut self, line: &str, turn: Option<&Turn>) -> Line {
        let mut out = Line::default();
        if line.trim().is_empty() {
            return out;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            self.junk += 1;
            if self.junk <= 3 {
                out.evs.push(RunEv::Notice(format!("claude sent a line that is not JSON: {}", one_line(&clip(line, 80)))));
            }
            return out;
        };
        out.session = v.get("session_id").and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from);
        let kind = v.get("type").and_then(Value::as_str);
        match kind {
            Some("assistant") => self.assistant(&v, &mut out),
            Some("stream_event") => self.delta(&v, &mut out),
            Some("user") => self.results(&v, &mut out),
            Some("control_request") if v.pointer("/request/subtype").and_then(Value::as_str) == Some("can_use_tool") => {
                self.permission(&v, turn, &mut out);
            }
            Some("control_cancel_request") => out.cancelled = v.get("request_id").and_then(Value::as_str).map(String::from),
            Some("result") => self.result(&v, &mut out, turn.is_some_and(|t| t.compacting)),
            Some("rate_limit_event") => self.rate_limit(&v, &mut out),
            Some("system") if v.get("subtype").and_then(Value::as_str) == Some("api_retry") => self.retry(&v, &mut out),
            Some("system") if v.get("subtype").and_then(Value::as_str) == Some("compact_boundary") => self.compact_after = v.pointer("/compact_metadata/post_tokens").and_then(Value::as_u64).filter(|n| *n > 0),
            Some("system") if v.get("subtype").and_then(Value::as_str) == Some("init") => self.serve(v.get("model").and_then(Value::as_str), &mut out),
            Some(other) if !HEARD.contains(&other) && self.unread.insert(other.to_string()) => out.evs.push(RunEv::Unread { dialect: "claude-stream".into(), kind: other.chars().take(60).collect() }),
            _ => {}
        }
        out
    }

    fn assistant(&mut self, v: &Value, out: &mut Line) {
        self.serve(None, out);
        let model = v.pointer("/message/model").and_then(Value::as_str).filter(|m| !m.starts_with('<'));
        if let (Some(id), Some(model)) = (v.pointer("/message/id").and_then(Value::as_str), model) {
            self.calls.insert(id.to_string(), (model.to_string(), units(v.pointer("/message/usage"), USAGE)));
            // the turn is priced when it ends, and a long one is many calls: what they have used so far is said as it goes
            let so_far: f64 = self.calls.values().map(|(_, u)| u).sum();
            if so_far > 0.0 {
                out.evs.push(RunEv::Spend(so_far));
            }
        }
        // what one call put in the window: fresh input and both cache buckets (a `result` frame sums every call of the turn,
        // so only an `assistant` frame's usage is the window)
        let tokens = in_window(v.pointer("/message/usage"));
        if tokens > 0 {
            self.ctx_tokens = tokens;
            if self.ctx_first == 0 {
                self.ctx_first = tokens;
            }
        }
        for block in v.pointer("/message/content").and_then(Value::as_array).into_iter().flatten() {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    let text = block.get("text").and_then(Value::as_str).unwrap_or("");
                    if self.streamed.is_empty() {
                        // never streamed (a command's own output, a CLI that sent no partials): it still has its place
                        if !text.trim().is_empty() {
                            self.tail.push_str(text);
                            out.evs.push(RunEv::Text(text.to_string()));
                        }
                    } else if self.streamed.starts_with(text) {
                        self.streamed.drain(..text.len());
                    } else {
                        self.streamed.clear();
                    }
                }
                Some("thinking") => {
                    let t = block.get("thinking").and_then(Value::as_str).unwrap_or("");
                    if !t.trim().is_empty() {
                        out.evs.push(RunEv::Message { role: Role::Reasoning, text: t.to_string() });
                    }
                }
                Some("tool_use") => {
                    let name = block.get("name").and_then(Value::as_str).unwrap_or("tool");
                    let input = block.get("input");
                    let get = |k: &str| input.and_then(|i| i.get(k)).and_then(Value::as_str);
                    let command = get("command").unwrap_or("");
                    let detail = match get("description") {
                        Some(d) => d.to_string(),
                        None if !command.is_empty() => clip(command, 120),
                        None => clip(&input.map(Value::to_string).unwrap_or_default(), 120),
                    };
                    let about = if name == "Bash" { command } else { get("file_path").or(get("notebook_path")).unwrap_or("") };
                    if let Some(id) = block.get("id").and_then(Value::as_str) {
                        self.tools.insert(id.to_string(), (name.to_string(), about.to_string()));
                    }
                    let own = v.get("parent_tool_use_id").is_none_or(Value::is_null);
                    if own {
                        self.fold_list(name, input.unwrap_or(&Value::Null));
                    }
                    self.boundary();
                    out.evs.push(RunEv::Tool { name: name.to_string(), detail, end: false });
                    if own {
                        out.evs.extend(self.said_list());
                    }
                }
                _ => {}
            }
        }
    }

    /// The doer's list tools change the list: `TaskCreate` adds an item (claude numbers them from 1), `TaskUpdate` sets one's status
    /// (`completed` and `deleted` close it, `in_progress` makes it the one at work) and `TodoWrite` (older claude) sends the whole list.
    /// Any other tool counts as nothing.
    fn fold_list(&mut self, name: &str, input: &Value) {
        let text = |k: &str| input.get(k).and_then(Value::as_str);
        let words = |i: &Value| ["activeForm", "subject", "content"].iter().find_map(|k| i.get(*k).and_then(Value::as_str)).map(|w| clip(w.trim(), 120));
        match name {
            "TaskCreate" => self.list.push(Item { id: (self.list.len() + 1).to_string(), open: true, doing: false, words: words(input).unwrap_or_default() }),
            "TaskUpdate" => {
                let id = text("taskId").map(String::from).or_else(|| input.get("taskId").and_then(Value::as_u64).map(|n| n.to_string()));
                let status = text("status");
                if let Some(at) = id.and_then(|id| self.list.iter().position(|i| i.id == id)) {
                    if status == Some("in_progress") {
                        self.list.iter_mut().for_each(|i| i.doing = false);
                    }
                    let item = &mut self.list[at];
                    if let Some(w) = words(input) {
                        item.words = w;
                    }
                    if let Some(s) = status {
                        (item.open, item.doing) = (!matches!(s, "completed" | "deleted"), s == "in_progress");
                    }
                }
            }
            "TodoWrite" => {
                let todos = input.get("todos").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
                let status = |t: &Value| t.get("status").and_then(Value::as_str).map(String::from);
                self.list = todos.iter().enumerate().map(|(n, t)| Item { id: (n + 1).to_string(), open: status(t).as_deref() != Some("completed"), doing: status(t).as_deref() == Some("in_progress"), words: words(t).unwrap_or_default() }).collect();
            }
            _ => {}
        }
    }

    /// The list as it stands, when it is not what was last said: how many are open, how many there are and the words of the one at work.
    fn said_list(&mut self) -> Option<RunEv> {
        let open = self.list.iter().filter(|i| i.open).count().min(u32::MAX as usize) as u32;
        let active = self.list.iter().find(|i| i.open && i.doing).map(|i| i.words.clone()).unwrap_or_default();
        let now = (open, self.list.len().min(u32::MAX as usize) as u32, active);
        if now.1 == 0 || now == self.listed {
            return None;
        }
        self.listed = now.clone();
        Some(RunEv::List { open: now.0, total: now.1, active: now.2 })
    }

    /// The streaming half of the same content. Text and thinking are said as they come. A tool call's arguments arrive as JSON fragments with no
    /// name (the settled call has it): they are said as a call being written, as `Open`, by their size in steps and when the file or the
    /// command they are about has parsed, and the words the model has written so far as `Tokens`, a count for the whole turn. Neither is
    /// written to the Log; the `Tool` of the settled call and the turn's `Metrics` are.
    fn delta(&mut self, v: &Value, out: &mut Line) {
        let event = v.get("event").unwrap_or(&Value::Null);
        let parent = v.get("parent_tool_use_id").and_then(Value::as_str).unwrap_or("").to_string();
        let at = event.get("index").and_then(Value::as_u64).unwrap_or(0);
        let kind = event.get("type").and_then(Value::as_str);
        let message = v.get("api_message_id").and_then(Value::as_str).or_else(|| event.pointer("/message/id").and_then(Value::as_str)).unwrap_or(&parent).to_string();
        match kind {
            Some("message_start") => {
                self.writing.retain(|(p, _), _| *p != parent);
                self.out_done += self.outs.remove(&message).map_or(0, |s| s.tokens());
            }
            Some("message_delta") => {
                let real = event.pointer("/usage/output_tokens").and_then(Value::as_u64).unwrap_or(0);
                self.outs.entry(message).or_default().real = real;
                self.said_tokens(out);
            }
            Some("content_block_start") if event.pointer("/content_block/type").and_then(Value::as_str) == Some("tool_use") => {
                let name = event.pointer("/content_block/name").and_then(Value::as_str).unwrap_or_default().to_string();
                self.writing.insert((parent.clone(), at), Writing { name, raw: String::new(), target: String::new(), told: None });
                self.said_call(&parent, at, out);
            }
            Some("content_block_stop") => {
                self.writing.remove(&(parent, at));
            }
            Some("content_block_delta") => {
                let d = event.get("delta").unwrap_or(&Value::Null);
                let (said, thought, json) = (d.get("text").and_then(Value::as_str), d.get("thinking").and_then(Value::as_str), d.get("partial_json").and_then(Value::as_str));
                match (d.get("type").and_then(Value::as_str), said, thought, json) {
                    (Some("text_delta"), Some(t), ..) if !t.is_empty() => {
                        self.streamed.push_str(t);
                        self.tail.push_str(t);
                        out.evs.push(RunEv::Text(t.to_string()));
                        self.outs.entry(message).or_default().bytes += t.len() as u64;
                    }
                    (Some("thinking_delta"), _, Some(t), _) => {
                        // a thinking the CLI keeps to itself says only how long it is
                        let hidden = d.get("estimated_tokens_delta").and_then(Value::as_u64).map_or(0, |n| n * 4);
                        self.outs.entry(message).or_default().bytes += if t.is_empty() { hidden } else { t.len() as u64 };
                        // said empty, it is only the sign that the model is thinking
                        if !t.is_empty() || hidden > 0 {
                            out.evs.push(RunEv::Reasoning(t.to_string()));
                        }
                    }
                    (Some("input_json_delta"), .., Some(j)) => {
                        self.outs.entry(message).or_default().bytes += j.len() as u64;
                        let w = self.writing.entry((parent.clone(), at)).or_insert(Writing { name: String::new(), raw: String::new(), target: String::new(), told: None });
                        w.raw.push_str(j);
                        self.said_call(&parent, at, out);
                    }
                    _ => {}
                }
                self.said_tokens(out);
            }
            _ => {}
        }
    }

    /// The call at `at` of `parent`'s stream, as `Open`: when it is first written, when its tool or its target is known, and at each [`OPEN_STEP`] bytes.
    fn said_call(&mut self, parent: &str, at: u64, out: &mut Line) {
        let Some(w) = self.writing.get_mut(&(parent.to_string(), at)) else { return };
        if w.raw.is_empty() && w.told.is_some() {
            return;
        }
        let name = if w.name.is_empty() { crate::opening::name_in(&w.raw).to_string() } else { w.name.clone() };
        let target = crate::opening::target_in(&w.raw);
        if w.told.is_some_and(|n| w.raw.len() < n + OPEN_STEP) && name == w.name && target == w.target {
            return;
        }
        (w.name, w.target, w.told) = (name.clone(), target.clone(), Some(w.raw.len()));
        let id = if parent.is_empty() { at.to_string() } else { format!("{parent}:{at}") };
        out.evs.push(RunEv::Open { id, name, target, bytes: w.raw.len() as u64 });
    }

    /// What the turn's model has written so far, when it has gone on by [`TOKENS_STEP`] since it was last said.
    fn said_tokens(&mut self, out: &mut Line) {
        let now = self.out_done + self.outs.values().map(Spoken::tokens).sum::<u64>();
        if now.abs_diff(self.out_told) >= TOKENS_STEP {
            self.out_told = now;
            out.evs.push(RunEv::Tokens(now));
        }
    }

    fn results(&mut self, v: &Value, out: &mut Line) {
        let blocks = v.pointer("/message/content").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
        for block in blocks.iter().filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_result")) {
            let is_error = block.get("is_error").and_then(Value::as_bool).unwrap_or(false);
            let output = text_of(block);
            let (name, about) = block
                .get("tool_use_id")
                .and_then(Value::as_str)
                .and_then(|id| self.tools.remove(id))
                .unwrap_or_else(|| ("tool".into(), String::new()));
            self.boundary();
            if name == "Bash" {
                let (exit, shown) = bash_exit(&output, is_error);
                out.evs.push(RunEv::Shell { cmd: about, out: shown.to_string(), exit });
                continue;
            }
            let detail = clip(&output, 200);
            out.evs.push(RunEv::Tool { name: name.clone(), detail: if is_error { format!("failed: {detail}") } else { detail }, end: true });
            if !is_error && !about.is_empty() && matches!(name.as_str(), "Edit" | "Write" | "MultiEdit" | "NotebookEdit") {
                out.evs.push(RunEv::Changed(vec![about]));
            }
        }
    }

    fn permission(&mut self, v: &Value, turn: Option<&Turn>, out: &mut Line) {
        let (Some(id), Some(turn)) = (v.get("request_id").and_then(Value::as_str), turn) else { return };
        let request = v.get("request").unwrap_or(&Value::Null);
        let tool = request.get("tool_name").and_then(Value::as_str).unwrap_or("tool");
        let input = request.get("input").cloned().unwrap_or(Value::Null);
        let questions: Vec<Value> = if tool == "AskUserQuestion" {
            input.get("questions").and_then(Value::as_array).cloned().unwrap_or_default()
        } else {
            Vec::new()
        };
        let act = effect(tool, &input, turn);
        // a remote shell runs inside the relay, which replaces every secret in what it prints
        let routed = act.as_ref().filter(|a| a.verb == Verb::Shell).and_then(|a| match (relay::route(&a.target, self.relay.as_deref().unwrap_or("")), &self.relay) {
            (relay::Route::Routed(line), Some(_)) => Some(line),
            _ => None,
        });
        let what = act.as_ref().map_or_else(|| clip(&input.to_string(), 200), |a| a.target.clone());
        let shown = one_line(&clip(&format!("{tool}: {what}"), 300));
        let first = AskSpec {
            local: clip(id, 60),
            question: if routed.is_some() { format!("allow {shown}? (remote: what it prints goes through J4n's redaction)") } else { format!("allow {shown}?") },
            shape: AskShape::Approval { action: shown, risk: risk_of(act.as_ref()) },
            inband: true,
            point: None,
            act: act.map(Box::new),
        };
        out.ask = Some(Pending { request_id: id.to_string(), input, questions, answers: Vec::new(), labels: Vec::new(), first, routed });
    }

    /// The model that did the turn and what the turn cost. The keys of `modelUsage` are the full ids, and its entries and `total_cost_usd` count
    /// from the start of the session's saved cost state, not of this child: a session resumed after a turn on sonnet holds that turn, and a
    /// turn on haiku after it was priced as sonnet, and the whole of the old state again. The turn's model is the one its calls named, its cost the
    /// total less what was there before the child's first call (each entry's cost by the share of its tokens that were the child's own).
    fn priced(&mut self, v: &Value) -> (Option<String>, f64) {
        let total = v.get("total_cost_usd").and_then(Value::as_f64).unwrap_or(0.0);
        let listed = v.get("modelUsage").and_then(Value::as_object);
        let cost = |u: &Value| u.get("costUSD").and_then(Value::as_f64).unwrap_or(0.0);
        let mut turn: BTreeMap<String, f64> = BTreeMap::new();
        for (model, u) in std::mem::take(&mut self.calls).into_values() {
            *turn.entry(model).or_default() += u;
        }
        // the frame's own usage is the turn's exact size; the calls say how it divides between models
        let (sum, whole) = (turn.values().sum::<f64>(), units(v.get("usage"), USAGE));
        if sum > 0.0 && whole > 0.0 {
            turn.values_mut().for_each(|u| *u *= whole / sum);
        }
        for (model, u) in &turn {
            *self.own.entry(model_key(model).to_string()).or_default() += u;
        }
        let main = turn.iter().max_by(|a, b| a.1.total_cmp(b.1)).map(|(m, _)| listed.and_then(|l| l.keys().find(|k| model_key(k) == model_key(m))).unwrap_or(m).clone());
        let main = main.or_else(|| listed.and_then(|m| m.iter().max_by(|a, b| cost(a.1).total_cmp(&cost(b.1)))).map(|(k, _)| k.clone()));
        if self.carried.is_none() && !self.own.is_empty() {
            let mine: f64 = listed.map_or(0.0, |l| {
                l.iter()
                    .map(|(m, e)| {
                        let (all, own) = (units(Some(e), ENTRY), self.own.get(model_key(m)).copied().unwrap_or(0.0));
                        cost(e) * if all <= 0.0 { 0.0 } else if own >= all * 0.99 { 1.0 } else { own / all }
                    })
                    .sum()
            });
            self.carried = Some(if mine > 0.0 && total > mine + 1e-6 { total - mine } else { 0.0 });
        }
        let counted = (total - self.carried.unwrap_or(0.0)).max(0.0);
        let usd = (counted - self.spent).max(0.0);
        self.spent = self.spent.max(counted);
        (main, usd)
    }

    fn result(&mut self, v: &Value, out: &mut Line, compacting: bool) {
        out.turn_end = true;
        (self.writing, self.outs, self.out_done, self.out_told) = (HashMap::new(), BTreeMap::new(), 0, 0);
        let (main, usd) = self.priced(v);
        self.serve(main.as_deref(), out);
        self.said = false;
        if let Some(window) = v.get("modelUsage").and_then(Value::as_object).into_iter().flat_map(|m| m.values()).filter_map(|u| u.get("contextWindow").and_then(Value::as_u64)).max().filter(|w| *w > 0) {
            self.ctx_window = window;
        }
        // the usage of a `result` frame adds every call of the turn up; its last iteration, when it lists them, is the last call
        let last_call = in_window(v.pointer("/usage/iterations").and_then(Value::as_array).and_then(|calls| calls.last()));
        let tokens = if self.ctx_tokens > 0 { self.ctx_tokens } else { last_call };
        let first = std::mem::take(&mut self.ctx_first);
        self.ctx_tokens = 0;
        // a turn that shrinks the session says what it left when the CLI does (`post_tokens`): that is the size and the window, and the turn's own
        // calls read the whole session to write the summary, so they are neither
        let after = self.compact_after.take().filter(|_| compacting);
        if tokens > 0 && !compacting {
            out.evs.push(RunEv::Window((tokens as f64 / self.ctx_window as f64).clamp(0.0, 1.0)));
        }
        if let Some(n) = after {
            out.evs.push(RunEv::Window((n as f64 / self.ctx_window as f64).clamp(0.0, 1.0)));
        }
        let n = |k: &str| v.pointer(&format!("/usage/{k}")).and_then(Value::as_u64).unwrap_or(0);
        // what the model read is the fresh input and both cache buckets (`input_tokens` alone under-counts: it leaves out the cache);
        // the two cache parts are in `tokens_in` and are said apart as well
        let (cached, wrote) = (n("cache_read_input_tokens"), n("cache_creation_input_tokens"));
        out.evs.push(RunEv::Metrics(Metrics { usd, tokens_in: n("input_tokens").saturating_add(cached).saturating_add(wrote), tokens_out: n("output_tokens"), cached, wrote, last: if compacting { after.unwrap_or(0) } else { tokens }, first: if compacting { 0 } else { first.min(tokens) } }));
        let subtype = v.get("subtype").and_then(Value::as_str).unwrap_or("");
        let text = v.get("result").and_then(Value::as_str).unwrap_or("");
        let failed = v.get("is_error").and_then(Value::as_bool).unwrap_or(false) || subtype.starts_with("error");
        let tail = std::mem::take(&mut self.tail);
        self.boundary();
        self.tools.clear();
        if failed {
            // a usage limit arrives as an ordinary result that says "You've hit your limit …"
            let message = match (subtype.starts_with("error"), text.is_empty()) {
                (true, true) => subtype.to_string(),
                (true, false) => format!("{subtype}: {text}"),
                (false, false) => text.to_string(),
                (false, true) => "claude reported an error".to_string(),
            };
            out.evs.push(RunEv::Error(message));
            return;
        }
        // The report is the text that settled last. When it is the very text the loop has in its tail, it is that text (the
        // loop then files it once, as the answer, and not again as narration).
        let answer = if text.trim().is_empty() {
            None
        } else if text.trim() == tail.trim() {
            Some(tail)
        } else {
            Some(text.to_string())
        };
        let todo_left = self.list.iter().filter(|i| i.open).count().min(u32::MAX as usize) as u32;
        out.evs.push(RunEv::End { answer, todo_left });
    }

    /// A rejected window announces itself before the turn dies; say so in words the failure table knows.
    fn rate_limit(&mut self, v: &Value, out: &mut Line) {
        let info = v.get("rate_limit_info");
        let field = |k: &str| info.and_then(|i| i.get(k));
        let mut limits: Vec<Limit> = field("unifiedWindows")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(window, w)| {
                let status = w.get("status").and_then(Value::as_str).unwrap_or("allowed").to_string();
                Some(Limit { window: window.clone(), utilization: w.get("utilization")?.as_f64()?, resets_at: w.get("resetsAt")?.as_i64()?, status })
            })
            .collect();
        limits.sort_by(|a, b| a.window.cmp(&b.window));
        if !limits.is_empty() {
            out.evs.push(RunEv::Limits(limits));
        }
        let status = field("status").and_then(Value::as_str).unwrap_or("allowed");
        if status == "allowed" {
            return;
        }
        let resets = field("resetsAt").and_then(Value::as_i64);
        if status.starts_with("rejected") || status == "blocked" {
            let until = resets.map(|t| format!(" (resets at {t})")).unwrap_or_default();
            out.evs.push(RunEv::Error(format!("rate limit: claude window {status}{until}")));
        } else {
            let which = field("rateLimitType").and_then(Value::as_str).unwrap_or("");
            out.evs.push(RunEv::Notice(limit_words(which, field("utilization").and_then(Value::as_f64), resets, j4n_core::clock::when_local)));
        }
    }

    /// The CLI retries a refused token ten times before it says so; with an explicit token the first retry already says it.
    fn retry(&mut self, v: &Value, out: &mut Line) {
        if self.explicit_token && !self.auth_reported && v.get("error_status").and_then(Value::as_i64) == Some(401) {
            self.auth_reported = true;
            out.evs.push(RunEv::Error("authentication failed: the claude token was refused (401)".into()));
        }
    }
}

/// What a window that is running low is told to the operator as: `claude · 92% of the 5-hour limit used · resets 14:20`. The status
/// word and the epoch the stream carries are not said; `when` turns the reset time (seconds since 1970) into the operator's clock.
pub fn limit_words(window: &str, used: Option<f64>, resets: Option<i64>, when: impl Fn(i64) -> String) -> String {
    let name = window.replace("five_hour", "5-hour").replace("seven_day", "7-day").replace('_', " ");
    let name = if name.is_empty() { "usage".to_string() } else { name };
    let mut words = match used {
        Some(u) => format!("claude · {:.0}% of the {name} limit used", u * 100.0),
        None => format!("claude · close to the {name} limit"),
    };
    if let Some(t) = resets {
        words.push_str(&format!(" · resets {}", when(t)));
    }
    words
}
