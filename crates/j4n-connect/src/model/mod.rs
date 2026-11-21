//! Chat models over HTTP: the dialects `openai` (OpenAI-compatible gateways: OpenRouter, a direct API, a local server) and
//! `anthropic` (the Messages API). A [`Client`] sends one request and reads the stream
//! it gets back; what the two dialects share is here, what differs is a [`Wire`]:
//!
//! - **tools are native**: the functions on offer ride in the request's own `tools`, the fragments of a streamed call are
//!   put together by their index, and **every call of a reply** comes back in order as a [`Call`], never as text;
//! - **reasoning goes back**: what a model thought is collected, kept with the assistant message that made a call, and sent
//!   back on the latest [`THINK_BACK`] call only (the cached prefix holds); without it every step starts again from the bare transcript;
//! - **room to write**: a turn may write [`LADDER`]`[0]` tokens, and a gateway that refuses the number is asked again with
//!   the next lower one; a call whose arguments are not JSON is refused as such and never as "missing a path", and arguments
//!   sent as a JSON *string* are decoded to the object they hold;
//! - **an empty reply is not an answer**: [`step`] asks again once, with a line saying so, and a second empty reply is a
//!   failure that names the tokens it cost.
//!
//! Streamed text and reasoning are handed to a callback as they come and are never part of what is kept until the turn ends.

pub mod anthropic;
pub mod doer;
pub mod offer;
pub mod openai;

use crate::net::{Net, NetError, Reply as Http};
use serde_json::Value as Json;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The sizes a turn may write, tried from the first down: a gateway that refuses the number is asked again with the next.
pub const LADDER: [u32; 4] = [32768, 16384, 8192, 4096];
/// The most a retry after an empty reply that hit its limit may ask for.
pub const RETRY_MAX: u32 = 16384;
/// How many of the latest calls go back with the reasoning that led to them (one; with more the cached prefix changes every request).
pub const THINK_BACK: usize = 1;
/// What a model is told when it answered with nothing.
pub const NUDGE: &str = "Your last reply was empty. Answer, or call a tool.";
/// Why a call is refused when its arguments are not JSON; a long call can be cut off before it is complete.
pub const CUT_OFF: &str = "not valid JSON — a long call can be cut off before it is complete; send a shorter one, write a long file in pieces";

/// What a model thought before it answered. `sig` is the proof some providers attach to it (`anthropic`), which must go back
/// with it unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Think {
    pub text: String,
    pub sig: String,
}

/// One call of a tool the model asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub id: String,
    pub name: String,
    /// The arguments as an object; `{}` when they could not be read (then `bad` says why).
    pub args: Json,
    /// Why the arguments are not usable, in words that go back to the model as the call's result.
    pub bad: Option<String>,
}

impl Call {
    /// A call from what the model streamed: `raw` is the text of its arguments.
    pub fn from_stream(id: &str, name: &str, raw: &str, cut: bool) -> Call {
        let (args, bad) = parse_args(raw, cut);
        Call { id: id.into(), name: name.into(), args, bad }
    }
}

/// The arguments of a call. Empty is no arguments; a JSON string holding an object is that object; anything else that is not an
/// object is refused (a stream that was cut off in the middle is the usual reason, and it is told as that).
pub fn parse_args(raw: &str, cut: bool) -> (Json, Option<String>) {
    let none = Json::Object(Default::default());
    if raw.trim().is_empty() {
        return (none, None);
    }
    match serde_json::from_str::<Json>(raw) {
        Ok(Json::Object(o)) => (Json::Object(o), None),
        Ok(Json::String(s)) => match serde_json::from_str::<Json>(&s) {
            Ok(o @ Json::Object(_)) => (o, None),
            _ => (none, Some(format!("the arguments are {CUT_OFF_SHORT}"))),
        },
        Ok(_) => (none, Some("the arguments are not a JSON object".into())),
        Err(_) => (none, Some(if cut { format!("the reply ended before the call was complete: {CUT_OFF}") } else { CUT_OFF.to_string() })),
    }
}

const CUT_OFF_SHORT: &str = "a string that does not hold a JSON object";

/// A message of the conversation.
#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    User(String),
    Assistant { text: String, think: Vec<Think>, calls: Vec<Call> },
    /// What a call came to. `error` when it was refused or failed.
    Result { call: String, name: String, text: String, error: bool },
}

/// A function the model may call.
#[derive(Debug, Clone, PartialEq)]
pub struct Tool {
    pub name: String,
    pub doc: String,
    /// A JSON Schema for its arguments.
    pub schema: Json,
}

/// One request.
#[derive(Debug, Clone)]
pub struct Req<'a> {
    pub model: &'a str,
    pub system: &'a str,
    pub msgs: &'a [Msg],
    pub tools: &'a [Tool],
    /// The most the reply may write.
    pub max: u32,
    /// `low` · `medium` · `high`, for a model that reasons.
    pub effort: Option<&'a str>,
    /// The host that is to serve it (a gateway's provider preference); only that one, never another.
    pub host: Option<&'a str>,
    /// Sent so the gateway keeps one session on one host, and its cache with it.
    pub session: Option<&'a str>,
}

impl<'a> Req<'a> {
    pub fn new(model: &'a str, msgs: &'a [Msg]) -> Req<'a> {
        Req { model, system: "", msgs, tools: &[], max: LADDER[0], effort: None, host: None, session: None }
    }

    /// The same request with another limit.
    pub fn max(&self, max: u32) -> Req<'a> {
        Req { max, ..self.clone() }
    }
}

/// How a reply ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finish {
    Stop,
    /// It wrote as much as it was allowed.
    Length,
    /// It stopped to have its calls run.
    Calls,
    Other(String),
}

/// What a request cost, in tokens (and dollars, when the gateway says).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Usage {
    /// The whole prompt, the cached part included.
    pub input: u64,
    pub output: u64,
    /// The part of the prompt that was read from the cache.
    pub cached: u64,
    /// The part written to the cache.
    pub wrote: u64,
    pub usd: Option<f64>,
}

/// What came back.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reply {
    pub text: String,
    pub think: Vec<Think>,
    pub calls: Vec<Call>,
    pub finish: Option<Finish>,
    pub usage: Usage,
    /// The host that served it, when the gateway says.
    pub served: Option<String>,
    /// The model that answered, when the gateway says (it may be another than was asked for).
    pub model: Option<String>,
    /// Milliseconds until the first word, thought or call came.
    pub ttft_ms: Option<u64>,
    pub ms: u64,
}

impl Reply {
    /// It said nothing and asked for nothing.
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty() && self.calls.is_empty()
    }
}

/// What the stream says as it comes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delta {
    Text(String),
    Reasoning(String),
    /// A call has begun, or has grown: its place among the reply's, its name, what the arguments so far say it is about, and how many bytes
    /// of arguments there are. Live only; the reply's `calls` are what settles.
    Call { id: String, name: String, target: String, bytes: u64 },
    /// The tokens the reply has written so far, as the stream's usage says.
    Tokens(u64),
    /// The stream broke and the request is asked again: what the reply has streamed so far is dropped, the next words are the reply's own.
    Restart,
}

/// Why a request did not give a reply.
#[derive(Debug, Clone, PartialEq)]
pub enum Fail {
    /// No answer at all: the connection, the proxy, a timeout.
    Net(String),
    /// The gateway answered with a status of 400 or more.
    Status { code: u16, body: String, retry_after: Option<u64> },
    /// The stream broke, or said it failed.
    Stream(String),
    /// Two replies in a row said nothing.
    Empty { tokens: u64 },
    Cancelled,
}

impl Fail {
    /// The failure as words the failure table reads (`packs/base/failures`): a limit says `429 rate limit`, and `(resets at …)`
    /// when the gateway said how long to wait.
    pub fn words(&self, now_ms: u64) -> String {
        match self {
            Fail::Net(e) => format!("could not reach the model: {e}"),
            Fail::Status { code, body, retry_after } => {
                let label = match code {
                    429 => "429 rate limit".to_string(),
                    529 => "529 overloaded".to_string(),
                    c => format!("HTTP {c}"),
                };
                let reset = retry_after.map(|s| format!(" (resets at {})", (now_ms / 1000).saturating_add(s))).unwrap_or_default();
                format!("{label}{reset}: {}", excerpt(body))
            }
            Fail::Stream(e) => format!("the model's stream failed: {e}"),
            Fail::Empty { tokens } => format!("the model returned no content twice ({tokens} tokens written)"),
            Fail::Cancelled => "stopped".into(),
        }
    }

    /// Whether trying the same request again may work: a limit, a busy host, a broken stream, no answer.
    pub fn transient(&self) -> bool {
        match self {
            Fail::Net(_) | Fail::Stream(_) => true,
            Fail::Status { code, .. } => matches!(code, 408 | 425 | 429 | 500 | 502 | 503 | 504 | 529),
            Fail::Empty { .. } | Fail::Cancelled => false,
        }
    }
}

/// The text of an error body, as one short line: gateways put the reason in `error.message` or `message`.
pub fn excerpt(body: &str) -> String {
    let said = serde_json::from_str::<Json>(body).ok().and_then(|v| {
        v.pointer("/error/message").or_else(|| v.get("message")).or_else(|| v.get("error")).and_then(Json::as_str).map(str::to_string)
    });
    let text = said.unwrap_or_else(|| body.to_string());
    text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(300).collect()
}

/// A dialect: the request it writes and the stream it reads.
pub trait Wire: Send + Sync {
    fn url(&self, base: &str) -> String;
    fn headers(&self, key: &str) -> Vec<(String, String)>;
    fn body(&self, req: &Req) -> Json;
    /// Read the stream to its end. `lines` are the lines of the body; the callback hears the text and reasoning as they come.
    fn read(&self, lines: &mut dyn Iterator<Item = Result<String, NetError>>, on: &mut dyn FnMut(Delta), cancel: &AtomicBool) -> Result<Reply, Fail>;
}

/// Whether a gateway's answer to a request says the number it was asked to write is too large.
pub fn refuses_number(code: u16, body: &str) -> bool {
    if !matches!(code, 400 | 413 | 422) {
        return false;
    }
    let b = body.to_lowercase();
    ["max_tokens", "max_completion_tokens", "max_output_tokens", "maximum output", "output tokens", "output limit", "too many tokens"].iter().any(|w| b.contains(w))
}

/// The lower rung of the ladder under `max`, if there is one.
pub fn below(max: u32) -> Option<u32> {
    LADDER.into_iter().find(|n| *n < max)
}

/// A model on the other end of a URL.
/// A key read when it is used, from where it is kept now: a key typed in Agents › Providers acts with no restart.
pub type KeyFn = Arc<dyn Fn() -> String + Send + Sync>;

pub struct Client {
    wire: Box<dyn Wire>,
    net: Net,
    base: String,
    key: String,
    /// Where the key is read each time a request is sent; none: `key`, as it was made.
    fresh: Option<KeyFn>,
    /// The waits before asking again a request that got no answer at all; none for a client that does not ride over drops.
    backoff: Vec<Duration>,
    /// Told each time such a request is about to be asked again.
    on_retry: Option<Arc<dyn Fn(String) + Send + Sync>>,
}

impl Client {
    pub fn new(wire: Box<dyn Wire>, net: Net, base: &str, key: &str) -> Client {
        Client { wire, net, base: base.trim_end_matches('/').to_string(), key: key.to_string(), fresh: None, backoff: Vec::new(), on_retry: None }
    }

    /// Read the key from `fresh` each time a request is sent.
    pub fn keyed(mut self, fresh: Option<KeyFn>) -> Client {
        self.fresh = fresh;
        self
    }

    /// The key as it is now.
    pub fn key(&self) -> String {
        self.fresh.as_ref().map_or_else(|| self.key.clone(), |f| f())
    }

    /// Ask again, after these waits, a request that failed before any byte of an answer came (no status line: [`Fail::Net`]), and
    /// tell `said` each time. So is one whose stream broke before it finished, while no call of the reply had begun: a tool
    /// runs only after the reply is whole, so nothing has been done; what was streamed is dropped (`Delta::Restart`) and the broken try's tokens are
    /// paid and not counted. One whose stream broke with a call begun is not: the words before the call are already kept as it began.
    pub fn riding(mut self, backoff: &[Duration], said: impl Fn(String) + Send + Sync + 'static) -> Client {
        self.backoff = backoff.to_vec();
        self.on_retry = Some(Arc::new(said));
        self
    }

    /// Wait `d` in slices, giving up the moment a stop comes. False when it did.
    fn wait(d: Duration, cancel: &AtomicBool) -> bool {
        let end = Instant::now() + d;
        while Instant::now() < end {
            if cancel.load(Ordering::SeqCst) {
                return false;
            }
            std::thread::sleep(Duration::from_millis(25).min(end.saturating_duration_since(Instant::now())));
        }
        !cancel.load(Ordering::SeqCst)
    }

    /// One exchange: send, and read the stream to its end. A status of 400 or more is a [`Fail::Status`].
    pub fn once(&self, req: &Req, on: &mut dyn FnMut(Delta), cancel: &AtomicBool) -> Result<Reply, Fail> {
        let started = Instant::now();
        let headers = self.wire.headers(&self.key());
        let pairs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let http: Http = self.net.post_json(&self.wire.url(&self.base), &pairs, &self.wire.body(req)).map_err(|e| Fail::Net(e.0))?;
        if !http.ok() {
            let (code, retry_after) = (http.status, http.headers.get("retry-after").and_then(|v| v.trim().parse::<u64>().ok()));
            let body = http.text().unwrap_or_default();
            return Err(Fail::Status { code, body, retry_after });
        }
        // the wire times from where it begins to read, which is after the status line: the time to the first token is counted from the request
        let waited = started.elapsed().as_millis() as u64;
        let mut lines = http.lines();
        let mut reply = self.wire.read(&mut lines, on, cancel)?;
        reply.ttft_ms = reply.ttft_ms.map(|t| t.saturating_add(waited));
        reply.ms = started.elapsed().as_millis() as u64;
        Ok(reply)
    }

    /// One request, asked again with a smaller number when the gateway refuses the one it was given.
    pub fn send(&self, req: &Req, on: &mut dyn FnMut(Delta), cancel: &AtomicBool) -> Result<Reply, Fail> {
        let mut max = req.max;
        let mut tries = 0;
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(Fail::Cancelled);
            }
            let mut opened = false;
            let mut watch = |d: Delta| {
                opened |= matches!(d, Delta::Call { .. });
                on(d);
            };
            let said = self.once(&req.max(max), &mut watch, cancel);
            match said {
                Err(Fail::Stream(e)) if !opened && tries < self.backoff.len() => {
                    let wait = self.backoff[tries];
                    tries += 1;
                    if let Some(say) = &self.on_retry {
                        say(format!("the model's stream broke ({e}): asking again in {} s, try {tries} of {}", wait.as_secs(), self.backoff.len()));
                    }
                    on(Delta::Restart);
                    if !Self::wait(wait, cancel) {
                        return Err(Fail::Cancelled);
                    }
                }
                Err(Fail::Net(e)) if tries < self.backoff.len() => {
                    let wait = self.backoff[tries];
                    tries += 1;
                    if let Some(say) = &self.on_retry {
                        say(format!("the model could not be reached ({e}): asking again in {} s, try {tries} of {}", wait.as_secs(), self.backoff.len()));
                    }
                    if !Self::wait(wait, cancel) {
                        return Err(Fail::Cancelled);
                    }
                }
                Err(Fail::Status { code, body, .. }) if refuses_number(code, &body).then(|| below(max)).flatten().is_some() => {
                    max = below(max).unwrap_or(max);
                }
                other => return other,
            }
        }
    }
}

/// One step of a conversation: a request, and the empty reply asked again once. `convo` is what has been said; the reply
/// is not added to it. The nudge is, when it was needed and worked, so what the model saw is what is kept.
pub fn step(client: &Client, req: &Req, convo: &mut Vec<Msg>, on: &mut dyn FnMut(Delta), cancel: &AtomicBool) -> Result<Reply, Fail> {
    let first = client.send(req, on, cancel)?;
    if !first.is_empty() {
        return Ok(first);
    }
    // a reasoning model can spend the whole limit thinking; a larger limit gives it room to answer
    let max = if first.finish == Some(Finish::Length) { req.max.max((req.max.saturating_mul(4)).min(RETRY_MAX)) } else { req.max };
    let mut again = convo.clone();
    again.push(Msg::User(NUDGE.into()));
    let second = client.send(&Req { msgs: &again, ..req.max(max) }, on, cancel)?;
    if second.is_empty() {
        return Err(Fail::Empty { tokens: first.usage.output + second.usage.output });
    }
    convo.push(Msg::User(NUDGE.into()));
    Ok(second)
}

/// The conversation as a request sends it: reasoning stays only on the latest [`THINK_BACK`] assistant message that made a
/// call (and the latest tool-use turn, by Anthropic's rule); on the others it is left out, so that what was sent before is sent again unchanged.
pub fn thought_back(msgs: &[Msg]) -> Vec<Msg> {
    let with_calls: Vec<usize> = msgs.iter().enumerate().filter(|(_, m)| matches!(m, Msg::Assistant { calls, .. } if !calls.is_empty())).map(|(i, _)| i).collect();
    let keep: Vec<usize> = with_calls.iter().rev().take(THINK_BACK).copied().collect();
    msgs.iter()
        .enumerate()
        .map(|(i, m)| match m {
            Msg::Assistant { text, calls, .. } if !keep.contains(&i) => Msg::Assistant { text: text.clone(), think: Vec::new(), calls: calls.clone() },
            other => other.clone(),
        })
        .collect()
}

/// Fold a gateway's stream into data lines: `data: …` frames, with `[DONE]` and the comments left out.
pub fn frames<'a>(lines: &'a mut dyn Iterator<Item = Result<String, NetError>>) -> impl Iterator<Item = Result<String, NetError>> + 'a {
    lines.filter_map(|l| match l {
        Ok(l) => {
            let data = l.strip_prefix("data:")?.trim();
            (!data.is_empty() && data != "[DONE]").then(|| Ok(data.to_string()))
        }
        Err(e) => Some(Err(e)),
    })
}
