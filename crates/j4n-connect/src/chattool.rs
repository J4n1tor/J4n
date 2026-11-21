//! The tool `chat`: how Main's doer starts a scout, a chat that only reads. A model calls it as it calls any tool; a doer that runs a program of its own calls
//! `j4n.call('chat', …)` through the kit's bridge, so the bridge gains no line. It is in no offer unless the start wired the post and the word was `orchestrate`.
//! The Gate answers it as any tool (law 5): a call is a `spawn` of `scout:<doer>`. What the call becomes is the front's (`j4n_core::front::scout`): who may call
//! it, the cap, the money, the clamps of a split, and that it never writes; the words come back as what happened to it.

use crate::model::doer::{Gated, ToolOut, Tools, action};
use crate::model::{Call, Tool};
use j4n_core::front::messages::Mailbox;
use j4n_core::front::scout::Want;
use j4n_core::gate::{Gate, ReadOnly, Verb, Verdict};
use j4n_core::place::{Launch, Site};
use serde_json::json;
use std::sync::Arc;

pub const NAME: &str = "chat";

pub struct ChatTool {
    pub post: Arc<Mailbox>,
    pub gate: Option<Gate>,
}

fn word<'a>(call: &'a Call, key: &str) -> &'a str {
    call.args.get(key).and_then(|v| v.as_str()).unwrap_or_default().trim()
}

impl Tools for ChatTool {
    fn specs(&self) -> Vec<Tool> {
        vec![Tool {
            name: NAME.into(),
            doc: "Start a scout: a chat of J4n that only reads, works beside you and reports on its own. The call returns at once with its id; the digest of its report comes with your next turn. goal: what it is to find out; paths: the files or folders it reads; doer, model: optional, the cheap tier by default.".into(),
            schema: json!({
                "type": "object",
                "properties": { "goal": { "type": "string" }, "paths": { "type": "array", "items": { "type": "string" } }, "doer": { "type": "string" }, "model": { "type": "string" } },
                "required": ["goal", "paths"]
            }),
        }]
    }

    fn gate(&self, call: &Call, task: &str, site: &Site, read_only: ReadOnly) -> Option<Gated> {
        let doer = Some(word(call, "doer")).filter(|d| !d.is_empty()).unwrap_or("default");
        let a = action("builtin", Verb::Spawn, &format!("scout:{doer}"), task, site, read_only);
        let question = format!("start a scout on {doer}");
        match self.gate.as_ref().map_or_else(|| Verdict::Allow { by: "no gate".into() }, |g| g.check(&a)) {
            Verdict::Allow { .. } => None,
            Verdict::Ask { risk, .. } => Some(Gated { action: a, question, risk }),
            Verdict::Block { .. } => Some(Gated { action: a, question, risk: 100 }),
        }
    }

    fn run(&self, call: &Call, site: &Site, _: &(dyn Fn(Launch) -> Launch + Send + Sync)) -> ToolOut {
        // a call that asks to write is passed on as one: the front refuses it and says why
        let asks_write = call.args.get("writes").is_some_and(|w| w.as_array().is_none_or(|a| !a.is_empty())) || matches!(word(call, "mode"), "write") || call.args.get("write").and_then(|w| w.as_bool()) == Some(true);
        let want = Want {
            goal: word(call, "goal").to_string(),
            paths: call.args.get("paths").and_then(|p| p.as_array()).map(|a| a.iter().filter_map(|v| v.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default(),
            doer: word(call, "doer").to_string(),
            model: word(call, "model").to_string(),
            write: asks_write,
        };
        let said = match self.post.get() {
            Some(post) => post.scout(&site.task, &want),
            None => Err("the post is not there yet".into()),
        };
        match said {
            Ok(text) => ToolOut { text, ..ToolOut::default() },
            Err(why) => ToolOut { text: why, error: true, ..ToolOut::default() },
        }
    }
}
