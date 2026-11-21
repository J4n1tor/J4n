//! The tool `tell`: how a chat's doer sends a note to another chat. A model calls it as it calls any tool; a doer that runs
//! a program of its own calls `j4n.call('tell', …)` through the kit's bridge, so the bridge gains no line. It is in no offer while `chats.on` is off.
//! The Gate answers it as any tool (law 5): a call is a `connect` to `chat:<id>`, `inside` when both chats are on one account, and the pack's rule
//! `tell-across-quarantine` blocks the rest. What the note becomes is the front's (`j4n_core::front::messages`): redacted, cut, held over a floor, carried
//! at the receiver's next turn; the words come back as what happened to it.

use crate::model::doer::{Gated, ToolOut, Tools, action};
use crate::model::{Call, Tool};
use j4n_core::front::messages::Mailbox;
use j4n_core::gate::{Gate, ReadOnly, Verb, Verdict};
use j4n_core::place::{Launch, Site};
use serde_json::json;
use std::sync::Arc;

pub const NAME: &str = "tell";

pub struct TellTool {
    pub post: Arc<Mailbox>,
    pub gate: Option<Gate>,
}

impl Tools for TellTool {
    fn specs(&self) -> Vec<Tool> {
        vec![Tool {
            name: NAME.into(),
            doc: "Tell another chat something: J4n carries the note and the chat reads it as data with its next turn. chat: its id (`c4`); text: what it needs to know.".into(),
            schema: json!({ "type": "object", "properties": { "chat": { "type": "string" }, "text": { "type": "string" } }, "required": ["chat", "text"] }),
        }]
    }

    fn gate(&self, call: &Call, task: &str, site: &Site, read_only: ReadOnly) -> Option<Gated> {
        let chat = call.args.get("chat").and_then(|v| v.as_str()).unwrap_or_default().trim();
        let inside = self.post.get().is_some_and(|p| p.reach(task, chat));
        let mut a = action("builtin", Verb::Connect, &format!("chat:{chat}"), task, site, read_only);
        a.inside = Some(inside);
        let question = format!("tell {chat}");
        match self.gate.as_ref().map_or_else(|| Verdict::Allow { by: "no gate".into() }, |g| g.check(&a)) {
            Verdict::Allow { .. } => None,
            Verdict::Ask { risk, .. } => Some(Gated { action: a, question, risk }),
            Verdict::Block { .. } => Some(Gated { action: a, question, risk: 100 }),
        }
    }

    fn run(&self, call: &Call, site: &Site, _: &(dyn Fn(Launch) -> Launch + Send + Sync)) -> ToolOut {
        let text = |k: &str| call.args.get(k).and_then(|v| v.as_str()).unwrap_or_default();
        let said = match self.post.get() {
            Some(post) => post.tell(&site.task, text("chat"), text("text")),
            None => Err("the post is not there yet".into()),
        };
        match said {
            Ok(text) => ToolOut { text, ..ToolOut::default() },
            Err(why) => ToolOut { text: why, error: true, ..ToolOut::default() },
        }
    }
}
