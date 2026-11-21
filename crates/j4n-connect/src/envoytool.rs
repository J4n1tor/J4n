//! The envoy's two tools: `charter` drafts the charter the operator signs in the envoy's chat, and `request` sends Main the operator's request under a charter that
//! holds. A model calls them as it calls any tool; a doer that runs a program of its own calls `j4n.call('charter', …)` through the kit's bridge. They are in no offer
//! unless the start wired the post and the envoys. The Gate answers them as any tool (a `spawn` of `envoy:<tool>`); what a call becomes is the front's
//! (`j4n_core::front::envoy`): who may call it, the charter's id, thread and the operator's words, and every limit.

use crate::model::doer::{Gated, ToolOut, Tools, action};
use crate::model::{Call, Tool};
use j4n_core::front::envoy::CharterArgs;
use j4n_core::front::messages::Mailbox;
use j4n_core::gate::{Gate, ReadOnly, Verb, Verdict};
use j4n_core::place::{Launch, Site};
use serde_json::json;
use std::sync::Arc;

pub const CHARTER: &str = "charter";
pub const REQUEST: &str = "request";

pub struct EnvoyTool {
    pub post: Arc<Mailbox>,
    pub gate: Option<Gate>,
}

fn text<'a>(call: &'a Call, key: &str) -> &'a str {
    call.args.get(key).and_then(|v| v.as_str()).unwrap_or_default().trim()
}

fn words(call: &Call, key: &str) -> Vec<String> {
    call.args.get(key).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default()
}

impl Tools for EnvoyTool {
    fn specs(&self) -> Vec<Tool> {
        vec![
            Tool {
                name: CHARTER.into(),
                doc: "Draft the charter the operator signs in this chat: what may be done without asking them. goal: the work in the operator's words; paths: where it may read and write (inside the project); may: read, write, shell, checks, spawn; doers: for spawn; usd: the most it may spend; minutes: how long it holds (leave out: until the operator ends it); watch: the name of one check of the project to run while J4n is open, every_minutes: how often (ten by default); when it fails Main gets a request. You cannot sign it.".into(),
                schema: json!({
                    "type": "object",
                    "properties": { "goal": { "type": "string" }, "paths": { "type": "array", "items": { "type": "string" } }, "may": { "type": "array", "items": { "type": "string" } }, "doers": { "type": "array", "items": { "type": "string" } }, "usd": { "type": "number" }, "minutes": { "type": "integer" }, "watch": { "type": "string" }, "every_minutes": { "type": "integer" } },
                    "required": ["goal", "paths", "may", "usd"]
                }),
            },
            Tool {
                name: REQUEST.into(),
                doc: "Send Main the operator's request, under a charter the operator signed in this chat. charter: its id; text: the request in your fuller words. Main gets the operator's own lines with it. It is refused while Main works, past the budget or the end, and for a destructive step the operator did not say.".into(),
                schema: json!({ "type": "object", "properties": { "charter": { "type": "string" }, "text": { "type": "string" } }, "required": ["charter", "text"] }),
            },
        ]
    }

    fn gate(&self, call: &Call, task: &str, site: &Site, read_only: ReadOnly) -> Option<Gated> {
        let a = action("builtin", Verb::Spawn, &format!("envoy:{}", call.name), task, site, read_only);
        let question = format!("the envoy's {}", call.name);
        match self.gate.as_ref().map_or_else(|| Verdict::Allow { by: "no gate".into() }, |g| g.check(&a)) {
            Verdict::Allow { .. } => None,
            Verdict::Ask { risk, .. } => Some(Gated { action: a, question, risk }),
            Verdict::Block { .. } => Some(Gated { action: a, question, risk: 100 }),
        }
    }

    fn run(&self, call: &Call, site: &Site, _: &(dyn Fn(Launch) -> Launch + Send + Sync)) -> ToolOut {
        let Some(post) = self.post.get() else { return ToolOut { text: "the post is not there yet".into(), error: true, ..ToolOut::default() } };
        let said = match call.name.as_str() {
            CHARTER => {
                let args = CharterArgs {
                    goal: text(call, "goal").into(),
                    watch: text(call, "watch").into(),
                    every: call.args.get("every_minutes").and_then(|v| v.as_u64()),
                    paths: words(call, "paths"),
                    may: words(call, "may"),
                    doers: words(call, "doers"),
                    usd: call.args.get("usd").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    minutes: call.args.get("minutes").and_then(|v| v.as_u64()),
                };
                post.envoy_charter(&site.task, &args)
            }
            _ => post.envoy_request(&site.task, text(call, "charter"), text(call, "text")),
        };
        match said {
            Ok(text) => ToolOut { text, ..ToolOut::default() },
            Err(why) => ToolOut { text: why, error: true, ..ToolOut::default() },
        }
    }
}
