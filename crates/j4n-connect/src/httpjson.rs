//! The dialect `http-json`: a REST endpoint said in data. The row gives the `url`, the `method`, the
//! `headers` and the `body` as templates over the tool's `args`, and `result`, where in the JSON answer the words are; a call is
//! one request, and what comes back is the answer. Nothing but the arguments is filled in: a value in the URL is percent-escaped,
//! one in the body is a JSON string's inside, so a value is never read again as syntax; the key (`key = "env:NAME"`) may stand in
//! a header as `{key}` and nowhere else, so it is never in an address or a body that a log or a server may keep. A call leaves
//! through the one network builder ([`Net`]) behind the connector's own proxy.

use j4n_core::text::clip;
use crate::manifest::Connector;
use crate::net::Net;
use crate::template::{self, Vars};
use j4n_core::ask::{Answer, AskShape, AskSpec};
use j4n_core::change::Actor;
use j4n_core::doer::{Caps, Doer, Probe, Run, RunCtl, RunEv, StartCx};
use j4n_core::gate::{Action, ReadOnly, Verb};
use j4n_core::table::Problem;
use j4n_core::task::Mode;
use serde_json::Value as Json;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};

/// The most of an answer that is kept.
const ANSWER_CHARS: usize = 12_000;

pub const METHODS: [&str; 5] = ["get", "post", "put", "patch", "delete"];

pub struct HttpJson {
    c: Connector,
    net: Net,
    key: String,
}

/// What is wrong with the row of an `http-json` connector, if anything.
pub fn check(c: &Connector) -> Result<(), String> {
    use crate::manifest::Is;
    if !matches!(c.is, Is::Tool | Is::Service) {
        return Err("`http-json` is for a connector that is a `tool` or a `service`".into());
    }
    let url = c.url.as_deref().unwrap_or_default();
    if !(url.starts_with("http://") || url.starts_with("https://")) || url.chars().any(|ch| ch.is_whitespace() || ch.is_control()) {
        return Err("`http-json` calls a `url` that starts with http:// or https://".into());
    }
    if let Some(m) = c.method.as_deref().filter(|m| !METHODS.contains(m)) {
        return Err(format!("`method` is one of {}, not `{m}`", METHODS.join(", ")));
    }
    if let Some(p) = &c.proxy {
        crate::net::Proxy::parse(p)?;
    }
    if c.key.as_deref().is_some_and(|k| !(k.starts_with("env:") || k.starts_with("file:"))) {
        return Err("`key` is where the key is kept: `env:NAME` or `file:/path`".into());
    }
    if let Some(r) = c.result.as_deref().filter(|r| !r.is_empty() && !r.starts_with('/')) {
        return Err(format!("`result` is a JSON pointer such as `/data/0/text`, not `{r}`"));
    }
    let known: Vec<&str> = c.args.iter().map(|a| a.name.as_str()).collect();
    let in_headers: Vec<&str> = c.headers.values().flat_map(|v| template::names(v)).collect();
    for (where_, text) in [("url", c.url.as_deref().unwrap_or_default()), ("body", c.body.as_deref().unwrap_or_default())] {
        for n in template::names(text) {
            if n == "key" {
                return Err(format!("`{{key}}` stands in a header, not in the {where_}: the key is kept out of addresses and bodies"));
            }
            if !known.contains(&n) {
                return Err(format!("`{{{n}}}` in the {where_} is not one of the `args`"));
            }
        }
    }
    if let Some(n) = in_headers.iter().find(|n| **n != "key" && !known.contains(n)) {
        return Err(format!("`{{{n}}}` in a header is not one of the `args`, or `key`"));
    }
    if in_headers.contains(&"key") && c.key.is_none() {
        return Err("a header uses `{key}`, and the connector names no `key`".into());
    }
    if let Some(bad) = c.headers.keys().find(|k| k.is_empty() || !k.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-')) {
        return Err(format!("`{bad}` is not a header name"));
    }
    if let Some(b) = c.body.as_deref().filter(|b| !b.trim().is_empty()) {
        let dummy: Vars = known.iter().map(|n| (n.to_string(), "x".to_string())).collect();
        let shown = template::fill_with(b, &dummy, &template::json_part)?;
        if serde_json::from_str::<Json>(&shown).is_err() {
            return Err("`body` is a JSON document, with `{name}` where an argument goes".into());
        }
    }
    Ok(())
}

impl HttpJson {
    pub fn new(c: &Connector, key: String) -> HttpJson {
        HttpJson { c: c.clone(), net: c.net(), key }
    }

    /// The values of the arguments from the words of a call: a JSON object by name, or any other words as the first.
    pub fn vars(c: &Connector, given: &serde_json::Map<String, Json>, plain: Option<&str>) -> Result<Vars, String> {
        let mut vars = Vars::new();
        for (i, a) in c.args.iter().enumerate() {
            let value = match plain {
                Some(text) => (i == 0).then(|| text.to_string()),
                None => given.get(&a.name).map(|v| v.as_str().map_or_else(|| v.to_string(), String::from)),
            };
            match value.or_else(|| a.default.clone()) {
                Some(v) => {
                    vars.insert(a.name.clone(), v);
                }
                None if a.required => return Err(format!("the argument `{}` is needed", a.name)),
                None => {}
            }
        }
        Ok(vars)
    }

    /// The address a call goes to, which is what the Gate is told.
    pub fn address(&self, vars: &Vars) -> Result<String, String> {
        template::fill_with(self.c.url.as_deref().unwrap_or_default(), vars, &template::url_part)
    }

    /// One request, and the words of its answer.
    pub fn call(&self, vars: &Vars) -> Result<String, String> {
        let url = self.address(vars)?;
        let mut headers: Vec<(String, String)> = Vec::new();
        let mut with_key = vars.clone();
        with_key.insert("key".into(), self.key.clone());
        for (k, v) in &self.c.headers {
            if v.contains("{key}") && self.key.is_empty() {
                return Err(format!("`{}` needs the key, and none could be read from `{}`", self.c.id, self.c.key.as_deref().unwrap_or_default()));
            }
            headers.push((k.clone(), template::fill_with(v, &with_key, &|s| s.to_string())?));
        }
        let body = match self.c.body.as_deref().filter(|b| !b.trim().is_empty()) {
            Some(b) => Some(template::fill_with(b, vars, &template::json_part)?.into_bytes()),
            None => None,
        };
        let pairs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let method = self.c.method.as_deref().unwrap_or("get").to_ascii_uppercase();
        let reply = self.net.request(&method, &url, &pairs, body).map_err(|e| e.to_string())?;
        let (status, ok) = (reply.status, reply.ok());
        let text = reply.text().map_err(|e| e.to_string())?;
        if !ok {
            return Err(format!("{} answered {status}: {}", self.c.id, clip(text.trim(), 300)));
        }
        match self.c.result.as_deref().filter(|r| !r.is_empty()) {
            None => Ok(clip(text.trim(), ANSWER_CHARS)),
            Some(pointer) => {
                let doc: Json = serde_json::from_str(&text).map_err(|_| format!("{} did not answer with JSON: {}", self.c.id, clip(text.trim(), 200)))?;
                match doc.pointer(pointer) {
                    Some(Json::String(s)) => Ok(clip(s, ANSWER_CHARS)),
                    Some(Json::Null) | None => Err(format!("the answer of {} has nothing at `{pointer}`", self.c.id)),
                    Some(v) => Ok(clip(&v.to_string(), ANSWER_CHARS)),
                }
            }
        }
    }

    /// What the Gate is asked before a call: reaching an address.
    pub fn action(&self, address: &str, task: &str, read_only: ReadOnly) -> Action {
        let mut a = Action::new(Verb::Net, Actor::Doer(self.c.id.clone()), address).task(task);
        a.read_only = read_only;
        a
    }
}

enum Msg {
    Ctl(RunCtl),
}

struct CallRun {
    tx: Sender<Msg>,
}

impl Run for CallRun {
    fn ctl(&mut self, ctl: RunCtl) -> Result<(), Problem> {
        self.tx.send(Msg::Ctl(ctl)).map_err(|_| Problem::refused("http-json", "the run is over"))
    }
}

impl Doer for HttpJson {
    fn id(&self) -> &str {
        &self.c.id
    }

    fn probe(&self) -> Probe {
        Probe { installed: true, version: None, authed: self.c.key.is_none() || !self.key.is_empty(), detail: if self.c.key.is_some() && self.key.is_empty() { "no key".into() } else { String::new() } }
    }

    fn caps(&self) -> Caps {
        Caps::default()
    }

    fn start(&self, cx: StartCx) -> Result<Box<dyn Run>, Problem> {
        let id = self.c.id.clone();
        let text = cx.text.clone().unwrap_or_else(|| cx.spec.goal.clone());
        let given = serde_json::from_str::<Json>(&text).ok().and_then(|j| if let Json::Object(m) = j { Some(m) } else { None });
        let vars = Self::vars(&self.c, &given.clone().unwrap_or_default(), given.is_none().then_some(text.as_str())).map_err(|e| Problem::refused(&id, e))?;
        let address = self.address(&vars).map_err(|e| Problem::refused(&id, e))?;
        let read_only = if cx.spec.tune.mode == Mode::Read { ReadOnly::Explicit } else { ReadOnly::No };
        let act = self.action(&address, &cx.task, read_only);
        let me = HttpJson { c: self.c.clone(), net: self.net.clone(), key: self.key.clone() };
        let (tx, rx) = mpsc::channel::<Msg>();
        let out = cx.out.clone();
        std::thread::spawn(move || {
            let spec = AskSpec { local: "call".into(), question: format!("call `{}`?", clip(&address, 200)), shape: AskShape::Approval { action: format!("{}: {}", me.c.id, clip(&address, 200)), risk: 40 }, inband: true, point: None, act: Some(Box::new(act)) };
            out.send(RunEv::Ask(spec));
            loop {
                match rx.recv() {
                    Ok(Msg::Ctl(RunCtl::Answer { answer: Answer::Yes, .. })) => break,
                    Ok(Msg::Ctl(RunCtl::Answer { .. })) => {
                        out.send(RunEv::Error("this was not allowed".into()));
                        return;
                    }
                    Ok(Msg::Ctl(RunCtl::Abort | RunCtl::Kill)) | Err(_) => return,
                    Ok(_) => {}
                }
            }
            match me.call(&vars) {
                Ok(answer) => out.send(RunEv::End { answer: Some(answer), todo_left: 0 }),
                Err(why) => out.send(RunEv::Error(why)),
            };
        });
        Ok(Box::new(CallRun { tx }))
    }
}

/// The doer of an `http-json` connector, the key read from where it is kept.
pub fn doer(c: &Connector, secret: &dyn Fn(&str, &str) -> Result<String, String>) -> Arc<dyn Doer> {
    Arc::new(HttpJson::new(c, c.api_key(secret)))
}
