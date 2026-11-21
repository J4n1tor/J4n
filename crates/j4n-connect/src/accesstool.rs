//! The tool `access`: how a chat of a split that writes asks to read a file of another part or of the folder, as a read-only copy in its worktree. A model
//! calls it as it calls any tool; a doer that runs a program of its own calls `j4n.call('access', …)` through the kit's bridge. It is in no offer unless the
//! start wired the post and `orchestrate.access` is on. The Gate answers it as any tool (law 5): a call is a `read` of `share:<want>`. Who may have what is
//! the front's (`j4n_core::front::access`): the plan, the Deciders, Main, and the guard above them; the words come back as what came of it.

use crate::model::doer::{Gated, ToolOut, Tools, action};
use crate::model::{Call, Tool};
use j4n_core::front::messages::Mailbox;
use j4n_core::gate::{Gate, ReadOnly, Verb, Verdict};
use j4n_core::place::{Launch, Site};
use serde_json::json;
use std::sync::Arc;

pub const NAME: &str = "access";

pub struct AccessTool {
    pub post: Arc<Mailbox>,
    pub gate: Option<Gate>,
}

fn word<'a>(call: &'a Call, key: &str) -> &'a str {
    call.args.get(key).and_then(|v| v.as_str()).unwrap_or_default().trim()
}

impl Tools for AccessTool {
    fn specs(&self) -> Vec<Tool> {
        vec![Tool {
            name: NAME.into(),
            doc: "Ask to read a file of another chat's part or of the folder: a read-only copy is put in your worktree at its path, kept as its source is. want: `<chat>:<path>` (`c1:src/engine.py`), `folder:<path>`, or `list` for what you have and what changed; why: a few words.".into(),
            schema: json!({ "type": "object", "properties": { "want": { "type": "string" }, "why": { "type": "string" } }, "required": ["want"] }),
        }]
    }

    fn gate(&self, call: &Call, task: &str, site: &Site, read_only: ReadOnly) -> Option<Gated> {
        let a = action("builtin", Verb::Read, &format!("share:{}", word(call, "want")), task, site, read_only);
        let question = format!("read a copy of {}", word(call, "want"));
        match self.gate.as_ref().map_or_else(|| Verdict::Allow { by: "no gate".into() }, |g| g.check(&a)) {
            Verdict::Allow { .. } => None,
            Verdict::Ask { risk, .. } => Some(Gated { action: a, question, risk }),
            Verdict::Block { .. } => Some(Gated { action: a, question, risk: 100 }),
        }
    }

    fn run(&self, call: &Call, site: &Site, _: &(dyn Fn(Launch) -> Launch + Send + Sync)) -> ToolOut {
        let said = match self.post.get() {
            Some(post) => post.access(&site.task, word(call, "want"), word(call, "why")),
            None => Err("the post is not there yet".into()),
        };
        match said {
            Ok(text) => ToolOut { text, ..ToolOut::default() },
            Err(why) => ToolOut { text: why, error: true, ..ToolOut::default() },
        }
    }
}
