//! What a model is offered beyond the builtin tools: the function `ask`, and the tools of the MCP servers
//! under the turn-path law's ceiling for a turn's fixed context.
//!
//! - **`ask`** puts one closed question to whoever is at the screen. The loop answers it ([`Turns`](super::doer)): the tool here is
//!   only its words. It is always a choice or a free line, never an approval; a permission is the Gate's, asked of a call.
//! - **[`Offer`]** holds the sets of tools and the MCP servers. A server's functions are offered one by one while the system prompt
//!   and every schema stay within [`CEILING`]; a server that would take the turn past it is offered as the one function
//!   `mcp(server, tool, args)` with a line that lists what it has.

use super::Tool;
use super::doer::{Gated, ToolOut, Tools};
use crate::model::Call;
use j4n_core::gate::ReadOnly;
use j4n_core::place::{Launch, Site};
use serde_json::{Value as Json, json};
use std::sync::{Arc, OnceLock};

/// The turn-path law's ceiling for the fixed context of a turn, in tokens (`fixtures/turn-path/budget.toml`; a law checks the two agree).
pub const CEILING: usize = 659;

/// The most options a question has.
pub const OPTIONS_MAX: usize = 6;

/// The name of a server's tool as a model calls it: letters, digits, `_` and `-`, at most 64.
pub fn exposed(server: &str, tool: &str) -> String {
    let clean = |s: &str| s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect::<String>();
    let name = format!("{}__{}", clean(server), clean(tool));
    name.chars().take(64).collect()
}

/// The function `ask`.
pub struct AskTool;

impl AskTool {
    pub const NAME: &'static str = "ask";
}

impl Tools for AskTool {
    fn specs(&self) -> Vec<Tool> {
        vec![Tool {
            name: Self::NAME.into(),
            doc: "Ask the operator a question with 2-6 options.".into(),
            schema: json!({ "type": "object", "properties": { "q": { "type": "string" }, "options": { "type": "array", "items": { "type": "string" } } }, "required": ["q", "options"] }),
        }]
    }

    fn gate(&self, _: &Call, _: &str, _: &Site, _: ReadOnly) -> Option<Gated> {
        None
    }

    fn run(&self, _: &Call, _: &Site, _: &(dyn Fn(Launch) -> Launch + Send + Sync)) -> ToolOut {
        ToolOut { text: "`ask` is put to the operator by the loop, not run here".into(), error: true, ..ToolOut::default() }
    }
}

/// The question and its options in a call of `ask`, or why they are no question.
pub fn question(call: &Call) -> Result<(String, Vec<String>), String> {
    let q = call.args.get("q").and_then(Json::as_str).map(str::trim).filter(|q| !q.is_empty()).ok_or("`ask` needs a question `q`")?;
    let options: Vec<String> = call.args.get("options").and_then(Json::as_array).map(|a| a.iter().filter_map(Json::as_str).map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).collect()).unwrap_or_default();
    if !(2..=OPTIONS_MAX).contains(&options.len()) {
        return Err(format!("`ask` needs 2 to {OPTIONS_MAX} `options` to pick from, and has {}", options.len()));
    }
    Ok((q.to_string(), options))
}

/// A tool as the request carries it, to count what a turn holds before every call.
pub fn tokens(system: &str, tools: &[Tool]) -> usize {
    let sent: Vec<Json> = tools.iter().map(|t| json!({ "type": "function", "function": { "name": t.name, "description": t.doc, "parameters": t.schema } })).collect();
    j4n_core::brief::tokens(system) + j4n_core::brief::tokens(&Json::Array(sent).to_string())
}

/// Which server a function belongs to, or that it is the hub.
enum Owner {
    Base,
    Server(usize),
    Hub,
}

struct Plan {
    specs: Vec<Tool>,
    /// For each server: the names it has on offer one by one, and the tools it holds back for the hub.
    own: Vec<(Vec<String>, Vec<Tool>)>,
}

/// The sets of tools of a model and its MCP servers, offered within the ceiling.
pub struct Offer {
    base: Arc<dyn Tools>,
    servers: Vec<(String, Arc<dyn Tools>)>,
    system: String,
    ceiling: usize,
    plan: OnceLock<Plan>,
}

impl Offer {
    pub fn new(base: Arc<dyn Tools>, servers: Vec<(String, Arc<dyn Tools>)>, system: &str) -> Offer {
        Offer { base, servers, system: system.to_string(), ceiling: CEILING, plan: OnceLock::new() }
    }

    /// A ceiling other than the law's, for a test.
    pub fn within(mut self, ceiling: usize) -> Offer {
        self.ceiling = ceiling;
        self
    }

    /// The servers are looked at once, when the first turn asks what is on offer.
    fn plan(&self) -> &Plan {
        self.plan.get_or_init(|| {
            let mut specs = self.base.specs();
            let mut own = Vec::new();
            for (_, server) in &self.servers {
                let found = server.specs();
                let mut both = specs.clone();
                both.extend(found.iter().cloned());
                if found.is_empty() || tokens(&self.system, &both) <= self.ceiling {
                    own.push((found.iter().map(|t| t.name.clone()).collect(), Vec::new()));
                    specs = both;
                } else {
                    own.push((Vec::new(), found));
                }
            }
            let held: Vec<String> = own.iter().zip(&self.servers).filter(|((_, held), _)| !held.is_empty()).map(|((_, held), (id, _))| format!("{id}: {}", held.iter().map(|t| t.name.split_once("__").map_or(t.name.as_str(), |(_, n)| n)).collect::<Vec<_>>().join(" "))).collect();
            if !held.is_empty() {
                let list: String = held.join("; ").chars().take(300).collect();
                specs.push(Tool {
                    name: "mcp".into(),
                    doc: format!("Call a tool of an MCP server. {list}"),
                    schema: json!({ "type": "object", "properties": { "server": { "type": "string" }, "tool": { "type": "string" }, "args": { "type": "object" } }, "required": ["server", "tool"] }),
                });
            }
            Plan { specs, own }
        })
    }

    fn owner(&self, name: &str) -> Owner {
        let plan = self.plan();
        if name == "mcp" && plan.own.iter().any(|(_, held)| !held.is_empty()) {
            return Owner::Hub;
        }
        plan.own.iter().position(|(names, _)| names.iter().any(|n| n == name)).map_or(Owner::Base, Owner::Server)
    }

    /// The call of a hub call: the server's own function, with the arguments the model gave it.
    fn inner(&self, call: &Call) -> Result<(usize, Call), String> {
        let (Some(server), Some(tool)) = (call.args.get("server").and_then(Json::as_str), call.args.get("tool").and_then(Json::as_str)) else {
            return Err("`mcp` needs a `server` and a `tool`".into());
        };
        let plan = self.plan();
        let Some(at) = self.servers.iter().position(|(id, _)| id == server) else { return Err(format!("there is no server `{server}`; the servers are {}", self.servers.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>().join(", "))) };
        let name = exposed(server, tool);
        if !plan.own[at].1.iter().any(|t| t.name == name) {
            return Err(format!("`{server}` has no tool `{tool}` to call this way"));
        }
        let args = call.args.get("args").cloned().filter(Json::is_object).unwrap_or_else(|| json!({}));
        Ok((at, Call { id: call.id.clone(), name, args, bad: None }))
    }
}

impl Tools for Offer {
    fn untrusted(&self, call: &Call) -> Option<String> {
        match self.owner(&call.name) {
            Owner::Base => self.base.untrusted(call),
            Owner::Server(i) => self.servers[i].1.untrusted(call),
            Owner::Hub => {
                let (at, inner) = self.inner(call).ok()?;
                self.servers[at].1.untrusted(&inner)
            }
        }
    }

    fn specs(&self) -> Vec<Tool> {
        self.plan().specs.clone()
    }

    fn gate(&self, call: &Call, task: &str, site: &Site, read_only: ReadOnly) -> Option<Gated> {
        match self.owner(&call.name) {
            Owner::Base => self.base.gate(call, task, site, read_only),
            Owner::Server(i) => self.servers[i].1.gate(call, task, site, read_only),
            Owner::Hub => {
                let (at, inner) = self.inner(call).ok()?;
                self.servers[at].1.gate(&inner, task, site, read_only)
            }
        }
    }

    fn run(&self, call: &Call, site: &Site, wrap: &(dyn Fn(Launch) -> Launch + Send + Sync)) -> ToolOut {
        match self.owner(&call.name) {
            Owner::Base => self.base.run(call, site, wrap),
            Owner::Server(i) => self.servers[i].1.run(call, site, wrap),
            Owner::Hub => match self.inner(call) {
                Ok((at, inner)) => self.servers[at].1.run(&inner, site, wrap),
                Err(why) => ToolOut { text: why, error: true, ..ToolOut::default() },
            },
        }
    }
}
