//! The `recall` function of a model that works on its own: it asks the memory in words and is told what was found. It reads and
//! changes nothing, so the Gate has no question for it; what is `private` is not given to it.

use crate::{Index, doers::line};
use j4n_connect::model::{Call, Tool};
use j4n_connect::model::doer::{Gated, ToolOut, Tools};
use j4n_core::gate::ReadOnly;
use j4n_core::log::Log;
use j4n_core::place::{Launch, Site};
use j4n_core::schema::DataClass;
use serde_json::{Value as Json, json};
use std::sync::Arc;

pub struct MemoryTools {
    index: Arc<Index>,
    log: Log,
}

impl MemoryTools {
    pub fn new(index: Arc<Index>, log: Log) -> MemoryTools {
        MemoryTools { index, log }
    }
}

impl Tools for MemoryTools {
    fn specs(&self) -> Vec<Tool> {
        vec![Tool {
            name: "recall".into(),
            doc: "Search the project's memory (notes, what the operator said, what tasks reported) for words; returns the best matches.".into(),
            schema: json!({ "type": "object", "properties": { "query": { "type": "string" }, "limit": { "type": "integer" } }, "required": ["query"] }),
        }]
    }

    fn gate(&self, _: &Call, _: &str, _: &Site, _: ReadOnly) -> Option<Gated> {
        None
    }

    fn run(&self, call: &Call, _: &Site, _: &(dyn Fn(Launch) -> Launch + Send + Sync)) -> ToolOut {
        let fail = |text: String| ToolOut { text, error: true, ..ToolOut::default() };
        if call.name != "recall" {
            return fail(format!("there is no tool `{}`", call.name));
        }
        let Some(q) = call.args.get("query").and_then(Json::as_str).filter(|q| !q.trim().is_empty()) else { return fail("`recall` needs a `query`".into()) };
        let limit = call.args.get("limit").and_then(Json::as_u64).unwrap_or(8) as usize;
        let found = self.index.catch_up(&self.log).and_then(|_| self.index.search(q, None, DataClass::Project, limit));
        match found {
            Ok(hits) if hits.is_empty() => ToolOut { text: format!("nothing in the memory matches `{q}`"), ..ToolOut::default() },
            Ok(hits) => ToolOut { text: hits.iter().map(line).collect::<Vec<_>>().join("\n"), ..ToolOut::default() },
            Err(why) => fail(why),
        }
    }
}
