//! The tools of a model that reach out: the web ([`web`]) and MCP servers ([`mcp`]). Both are `Tools` of a
//! model doer, offered by the word `web` and the id of an `mcp` connector in its `tools`, and both go to the Gate before they act.
//! [`Outside`] is what the binary gives the connectors to make them.

pub mod mcp;
pub mod web;

use j4n_connect::manifest::{Connector, Reach, Survey};
use j4n_connect::model::doer::Tools;
use j4n_connect::net::Net;
use j4n_core::gate::Gate;
use j4n_core::place::Launch;
use mcp::tools::{McpTools, Pinner};
use std::path::Path;
use std::sync::Arc;
use web::Web;

type Secret = Arc<dyn Fn(&str, &str) -> Result<String, String> + Send + Sync>;

/// What makes the tools that reach out.
pub struct Outside {
    /// Reads a secret from where it is kept (and writes it down as one never to be shown).
    pub secret: Secret,
    pub gate: Option<Gate>,
    /// The operator's own machine and network may be fetched from (`web.local`).
    pub local: bool,
    /// The connector (`http-json`) that answers a search (`web.search`).
    pub search: Option<Connector>,
    /// Told when a server offers tools nobody has pinned.
    pub pinner: Option<Pinner>,
    /// Told what a server offers, whole, when its tools are first looked at (what J4n follows of it).
    pub listing: Option<mcp::tools::Listing>,
    /// The pack `guard` is on: where the guard tells what it stopped, with the data of the alert. `None` is today's behaviour.
    pub alert: Option<mcp::tools::Alerter>,
}

impl Reach for Outside {
    fn tools(&self, word: &str, row: Option<&Connector>) -> Option<Arc<dyn Tools>> {
        match row {
            None if word == "web" => {
                let mut web = Web::new(Net::new()).local(self.local);
                if let Some(via) = &self.search {
                    web = web.searching(via.clone(), via.api_key(&*self.secret));
                }
                if let Some(g) = &self.gate {
                    web = web.with_gate(g.clone());
                }
                Some(Arc::new(web))
            }
            Some(row) => {
                let env = row.environment(&*self.secret).ok()?;
                let tools = McpTools::new(row, env, row.api_key(&*self.secret), self.gate.clone(), self.pinner.clone()).with_listing(self.listing.clone());
                Some(Arc::new(match &self.alert {
                    Some(a) => tools.guarded(a.clone()),
                    None => tools,
                }))
            }
            None => None,
        }
    }

    fn survey(&self, row: &Connector, wrap: &dyn Fn(Launch) -> Launch, cwd: &Path) -> Result<Survey, String> {
        let env = row.environment(&*self.secret)?;
        mcp::tools::survey(row, &env, &row.api_key(&*self.secret), wrap, cwd)
    }
}
