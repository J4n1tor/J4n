//! The tool `context`: how a doer asks the librarian for more than its task was started with. A model calls it as it calls
//! any tool; a doer that runs a program of its own calls `j4n.call('context', …)` through the kit's bridge, which offers whatever the doer's offer
//! holds. It is in no offer while `librarian.on` is off. What it gives back is the librarian's (`j4n_core::front::librarian::More`): the text asked for,
//! or why not, and the Log says what was added (`brief.added`). It reads the project's own facts and reaches nowhere, so the Gate is not asked.

use crate::model::Tool;
use crate::model::Call;
use crate::model::doer::{Gated, ToolOut, Tools};
use j4n_core::front::librarian::Shelf;
use j4n_core::gate::ReadOnly;
use j4n_core::place::{Launch, Site};
use serde_json::json;
use std::sync::Arc;

pub const NAME: &str = "context";

pub struct ContextTool(pub Arc<Shelf>);

impl Tools for ContextTool {
    fn specs(&self) -> Vec<Tool> {
        vec![Tool {
            name: NAME.into(),
            doc: "More context for this task. want: `list`, `project`, `remembered` or a skill's name; why: a few words.".into(),
            schema: json!({ "type": "object", "properties": { "want": { "type": "string" }, "why": { "type": "string" } }, "required": ["want"] }),
        }]
    }

    fn gate(&self, _: &Call, _: &str, _: &Site, _: ReadOnly) -> Option<Gated> {
        None
    }

    fn run(&self, call: &Call, site: &Site, _: &(dyn Fn(Launch) -> Launch + Send + Sync)) -> ToolOut {
        let text = |k: &str| call.args.get(k).and_then(|v| v.as_str()).unwrap_or_default();
        let said = match self.0.get() {
            Some(more) => more.add(&site.task, text("want"), text("why")),
            None => Err("the librarian is not there yet".into()),
        };
        match said {
            Ok(text) => ToolOut { text, ..ToolOut::default() },
            Err(why) => ToolOut { text: why, error: true, ..ToolOut::default() },
        }
    }
}
