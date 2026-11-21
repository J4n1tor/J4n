//! The tools of one MCP server, as the tools of a model. The server is started the first time its tools are looked at, with a
//! cleared environment (`PATH`, `HOME`, the connector's `env` and its `secrets` and nothing else of the operator's), and its tool
//! list is held to the hashes the operator pinned (`pins` of the connector): a tool that is new, or whose words or schema have
//! changed since, is **not offered**, and the pin that would offer it is proposed to the operator (the `pinner`). Every call is
//! checked against the tool's schema, put to the Gate by what the tool can do, and its result is words for the model to read.

use j4n_core::text::clip;
use super::classify::{Class, classify};
use super::http::Http;
use super::stdio::Stdio;
use super::{Client, McpTool, Transport, validate};
use j4n_connect::manifest::{Connector, Survey};
use j4n_connect::model::doer::{Gated, ToolOut, Tools, action};
use j4n_connect::model::{Call, Tool};
use j4n_connect::net::Net;
use j4n_core::gate::{Action, Gate, ReadOnly, Verb, Verdict};
use j4n_core::place::{Launch, Site};
use std::collections::BTreeMap;
use std::path::Path;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

/// What is told when tools are found that the operator has not pinned: the server's id, and each tool's name and hash.
pub type Pinner = Arc<dyn Fn(&str, &BTreeMap<String, String>) + Send + Sync>;

/// What is told when a server's tools are first looked at: the server's id, and each tool's name and hash (the whole list).
pub type Listing = Arc<dyn Fn(&str, &BTreeMap<String, String>) + Send + Sync>;

/// What is told when the guard stops or catches something: the data of the alert (`kind`, `of`, `tool`, `done`, …), for the Log.
pub type Alerter = Arc<dyn Fn(Value) + Send + Sync>;

struct Listed {
    tool: McpTool,
    hash: String,
    class: Class,
    name: String,
    pinned: bool,
}

enum State {
    Fresh,
    Up(Arc<Client>, Arc<Vec<Listed>>),
    Down(String),
}

pub struct McpTools {
    row: Connector,
    env: BTreeMap<String, String>,
    key: String,
    net: Net,
    gate: Option<Gate>,
    pinner: Option<Pinner>,
    listing: Option<Listing>,
    /// The pack `guard` is on: a tool is looked at again before each call, and a change is told here.
    guard: Option<Alerter>,
    state: Mutex<State>,
}

/// The transport of `row`: a program that serves on stdio, started as `launch` says, or an address.
fn transport(row: &Connector, env: &BTreeMap<String, String>, key: &str, net: &Net, wrap: &dyn Fn(Launch) -> Launch, cwd: &Path) -> Result<Box<dyn Transport>, String> {
    if let Some(url) = &row.url {
        let mut headers = Vec::new();
        for (k, v) in &row.headers {
            headers.push((k.clone(), v.replace("{key}", key)));
        }
        return Ok(Box::new(Http::new(net.clone(), url, headers)));
    }
    let mut argv: Vec<String> = vec!["env".into(), "-i".into()];
    for (k, fallback) in [("PATH", "/usr/local/bin:/usr/bin:/bin"), ("HOME", "/"), ("LANG", "C.UTF-8")] {
        argv.push(format!("{k}={}", std::env::var(k).unwrap_or_else(|_| fallback.into())));
    }
    argv.extend(env.iter().map(|(k, v)| format!("{k}={v}")));
    argv.push(row.program().to_string());
    argv.extend(row.start.iter().cloned());
    let launch = Launch { login: row.login.clone(), ..Launch::new(argv, cwd.to_path_buf(), BTreeMap::new()) };
    Ok(Box::new(Stdio::start(&wrap(launch))?))
}

fn listed(client: &Client, row: &Connector) -> Result<Vec<Listed>, String> {
    let mut seen = std::collections::BTreeSet::new();
    Ok(client
        .tools()?
        .into_iter()
        .map(|tool| {
            let hash = tool.hash();
            let class = classify(&tool.name, &tool.annotations);
            let name = j4n_connect::model::offer::exposed(&row.id, &tool.name);
            let pinned = row.pins.get(&tool.name) == Some(&hash);
            Listed { tool, hash, class, name, pinned }
        })
        // two tools that come out under one name are neither
        .filter(|l| seen.insert(l.name.clone()))
        .collect())
}

impl McpTools {
    pub fn new(row: &Connector, env: BTreeMap<String, String>, key: String, gate: Option<Gate>, pinner: Option<Pinner>) -> McpTools {
        McpTools { net: row.net(), row: row.clone(), env, key, gate, pinner, listing: None, guard: None, state: Mutex::new(State::Fresh) }
    }

    /// Tell `listing` what the server offers when its tools are looked at.
    pub fn with_listing(mut self, listing: Option<Listing>) -> McpTools {
        self.listing = listing;
        self
    }

    /// Hold the tools to their pins at call time, not only when they are first listed, and tell `alerter` of a change.
    pub fn guarded(mut self, alerter: Alerter) -> McpTools {
        self.guard = Some(alerter);
        self
    }

    fn alert(&self, data: Value) {
        if let Some(a) = &self.guard {
            a(data);
        }
    }

    fn up(&self) -> Option<(Arc<Client>, Arc<Vec<Listed>>)> {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(*st, State::Fresh) {
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir());
            let made = transport(&self.row, &self.env, &self.key, &self.net, &|l| l, &cwd).and_then(Client::start).and_then(|c| listed(&c, &self.row).map(|l| (c, l)));
            *st = match made {
                Ok((client, list)) => {
                    let unpinned: BTreeMap<String, String> = list.iter().filter(|l| !l.pinned).map(|l| (l.tool.name.clone(), l.hash.clone())).collect();
                    if let (false, Some(p)) = (unpinned.is_empty(), &self.pinner) {
                        p(&self.row.id, &unpinned);
                    }
                    let changed: Vec<&str> = list.iter().filter(|l| !l.pinned && self.row.pins.contains_key(&l.tool.name)).map(|l| l.tool.name.as_str()).collect();
                    if !changed.is_empty() {
                        self.alert(json!({ "kind": "pin", "of": self.row.id, "tools": changed, "when": "listed", "done": "not offered" }));
                    }
                    if let Some(l) = &self.listing {
                        l(&self.row.id, &list.iter().map(|t| (t.tool.name.clone(), t.hash.clone())).collect());
                    }
                    State::Up(Arc::new(client), Arc::new(list))
                }
                Err(why) => State::Down(why),
            };
        }
        match &*st {
            State::Up(c, l) => Some((c.clone(), l.clone())),
            _ => None,
        }
    }

    /// The tool as the server offers it now, against the pin: the list is asked for again, so a server that changed a tool after it was
    /// first listed (the rug pull) is caught at the call, not at the next start. `Err` is what the model is told; the tool is dropped from
    /// what is offered, the change is proposed and told (`guard.alert`).
    fn recheck(&self, client: &Client, name: &str) -> Result<(), String> {
        let cannot = |why: String| format!("`{name}` was not sent: {} could not be checked against its pin first ({why})", self.row.id);
        let fresh = listed(client, &self.row).map_err(cannot)?;
        let ok = fresh.iter().any(|l| l.pinned && l.tool.name == name);
        let now = fresh.iter().find(|l| l.tool.name == name).map(|l| l.hash.clone());
        let unpinned: BTreeMap<String, String> = fresh.iter().filter(|l| !l.pinned).map(|l| (l.tool.name.clone(), l.hash.clone())).collect();
        let was = self.row.pins.get(name).cloned().unwrap_or_default();
        if !ok {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if let State::Up(c, _) = &*st {
                *st = State::Up(c.clone(), Arc::new(fresh));
            }
            drop(st);
            if let (false, Some(p)) = (unpinned.is_empty(), &self.pinner) {
                p(&self.row.id, &unpinned);
            }
            self.alert(json!({ "kind": "pin", "of": self.row.id, "tools": [name], "was": was, "now": now, "when": "called", "done": "blocked" }));
            return Err(match now {
                Some(_) => format!("`{name}` was not sent: {} changed its words or its schema since the operator pinned it; it is blocked until it is pinned again", self.row.id),
                None => format!("`{name}` was not sent: {} no longer offers it", self.row.id),
            });
        }
        Ok(())
    }

    /// Why the server is not there, when it is not.
    pub fn down(&self) -> Option<String> {
        match &*self.state.lock().unwrap_or_else(|e| e.into_inner()) {
            State::Down(why) => Some(why.clone()),
            _ => None,
        }
    }
}

/// Start the server behind `wrap`, list its tools, and let it go: what the Connect flow's canary shows.
pub fn survey(row: &Connector, env: &BTreeMap<String, String>, key: &str, wrap: &dyn Fn(Launch) -> Launch, cwd: &Path) -> Result<Survey, String> {
    let client = Client::start(transport(row, env, key, &row.net(), wrap, cwd)?)?;
    let list = listed(&client, row)?;
    client.close();
    if list.is_empty() {
        return Err("the server started and has no tools".into());
    }
    let tools: Vec<(String, String, String)> = list.iter().map(|l| (l.tool.name.clone(), l.hash.clone(), l.class.word().to_string())).collect();
    let text = format!("{} tool(s): {}", list.len(), list.iter().take(12).map(|l| format!("{} ({})", l.tool.name, l.class.word())).collect::<Vec<_>>().join(", "));
    Ok(Survey { tools, text })
}

impl Tools for McpTools {
    fn untrusted(&self, _: &Call) -> Option<String> {
        Some(format!("mcp:{}", self.row.id))
    }

    fn specs(&self) -> Vec<Tool> {
        let Some((_, list)) = self.up() else { return Vec::new() };
        list.iter().filter(|l| l.pinned).map(|l| Tool { name: l.name.clone(), doc: clip(&format!("[{}] {}", self.row.id, l.tool.description), 1000), schema: l.tool.schema.clone() }).collect()
    }

    fn gate(&self, call: &Call, task: &str, site: &Site, read_only: ReadOnly) -> Option<Gated> {
        let (_, list) = self.up()?;
        let l = list.iter().find(|l| l.pinned && l.name == call.name)?;
        let verb = if l.class == Class::Read { Verb::Read } else { Verb::Write };
        let a: Action = action(&self.row.id, verb, &format!("mcp:{}/{}", self.row.id, l.tool.name), task, site, read_only);
        let question = format!("{} on {} ({}): {}", l.tool.name, self.row.id, l.class.word(), clip(&call.args.to_string(), 150));
        let verdict = self.gate.as_ref().map_or_else(|| Verdict::Ask { risk: l.class.risk(), why: "no rules are loaded".into(), by: "no gate".into() }, |g| g.check(&a));
        match verdict {
            Verdict::Allow { .. } => None,
            Verdict::Ask { risk, .. } => Some(Gated { action: a, question, risk: risk.max(l.class.risk()) }),
            Verdict::Block { .. } => Some(Gated { action: a, question, risk: 100 }),
        }
    }

    fn run(&self, call: &Call, _: &Site, _: &(dyn Fn(Launch) -> Launch + Send + Sync)) -> ToolOut {
        let bad = |t: String| ToolOut { text: t, error: true, ..ToolOut::default() };
        let Some((client, list)) = self.up() else { return bad(format!("{} is not there: {}", self.row.id, self.down().unwrap_or_default())) };
        let Some(l) = list.iter().find(|l| l.pinned && l.name == call.name) else { return bad(format!("there is no tool `{}`", call.name)) };
        if self.guard.is_some()
            && let Err(why) = self.recheck(&client, &l.tool.name)
        {
            return bad(why);
        }
        let problems = validate::problems(&call.args, &l.tool.schema);
        if !problems.is_empty() {
            return bad(format!("`{}` was not sent; the server's schema for it says:\n{}", l.tool.name, problems.join("\n")));
        }
        match client.call(&l.tool.name, &call.args) {
            Ok(out) => ToolOut { text: if out.text.is_empty() { "(the tool answered with nothing)".into() } else { out.text }, error: out.error, ..ToolOut::default() },
            Err(e) => bad(format!("{}: {e}", self.row.id)),
        }
    }
}

/// The proposal that pins tools: the server's pins as they are now, with these added or replaced.
pub fn pin_draft(server: &str, now: &BTreeMap<String, String>, found: &BTreeMap<String, String>) -> j4n_core::proposal::Draft {
    use j4n_core::change::Where;
    use j4n_core::proposal::{Draft, Step, StepOp};
    let mut all = now.clone();
    all.extend(found.iter().map(|(k, v)| (k.clone(), v.clone())));
    let changed: Vec<&str> = found.keys().filter(|k| now.contains_key(*k)).map(String::as_str).collect();
    let names: Vec<&str> = found.keys().map(String::as_str).take(12).collect();
    let mut fields = toml::Table::new();
    fields.insert("pins".into(), toml::Value::Table(all.iter().map(|(k, v)| (k.clone(), toml::Value::String(v.clone()))).collect()));
    let why = if changed.is_empty() {
        format!("The server `{server}` offers {} tool(s) nobody has pinned: {}. A pinned tool is offered to a model; its words and schema are held to the hash.", found.len(), names.join(", "))
    } else {
        format!("The server `{server}` changed the words or the schema of {}: {}. They are not offered until you pin them again; read what changed first.", changed.len(), changed.join(", "))
    };
    Draft {
        key: format!("pin:{server}:{}", found.values().cloned().collect::<Vec<_>>().join(",")),
        title: format!("Pin {} tool(s) of {server}", found.len()),
        why,
        evidence: found.len() as u64,
        steps: vec![Step { to: Where::User, file: Some(format!("connectors/{server}.toml")), kind: "connector".into(), id: server.to_string(), op: StepOp::Set(fields) }],
        widens: vec![format!("mcp {server}: offers {} tool(s) to a model", found.len())],
    }
}
