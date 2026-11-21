//! Terminal commands and apps as the tools of a model: a connector with `is = "tool"` and the dialect
//! `text` offers itself as a function — its `id` is the name, its `args` the parameters — and a call runs its `argv` with the
//! arguments filled in, behind the place's walls, with a clock and the end of its output kept. Nothing is a shell line: the
//! words of a call become separate arguments, so a value is never read again as syntax, and one that would stand alone and begin
//! with a dash is refused (`template`). Every call is put to the Gate as a command line first, like `run`.

use crate::httpjson::HttpJson;
use crate::manifest::{Connector, Dialect, Is, Risk};
use crate::model::doer::{Gated, ToolOut, Tools, action};
use crate::model::{Call, Tool};
use crate::template::{self, Vars};
use crate::tools::plain;
use j4n_core::gate::{Action, Gate, ReadOnly, Verb, Verdict};
use j4n_core::place::{Launch, Site};
use serde_json::{Value as Json, json};

/// A command tool runs at most this long.
const SECS: u64 = 60;

/// The names of the tools J4n carries: a command called so is not offered to a model, which would take it for the other.
const CARRIED: [&str; 8] = ["list", "read", "grep", "edit", "write", "run", "git", "py"];

/// The connectors that can be offered to a model as a function: a command with a command line, or an endpoint said in data.
pub fn offered(c: &Connector) -> bool {
    c.is == Is::Tool && !CARRIED.contains(&c.id.as_str()) && (c.dialect == Dialect::HttpJson || (c.dialect == Dialect::Text && !c.argv.is_empty()))
}

pub struct CommandTools {
    rows: Vec<Connector>,
    /// The key of each endpoint that has one, read when the set was made.
    keys: std::collections::BTreeMap<String, String>,
    gate: Option<Gate>,
}

impl CommandTools {
    pub fn new(rows: Vec<Connector>) -> CommandTools {
        CommandTools { rows, keys: Default::default(), gate: None }
    }

    pub fn with_keys(mut self, keys: std::collections::BTreeMap<String, String>) -> CommandTools {
        self.keys = keys;
        self
    }

    pub fn with_gate(mut self, gate: Gate) -> CommandTools {
        self.gate = Some(gate);
        self
    }

    fn row(&self, name: &str) -> Option<&Connector> {
        self.rows.iter().find(|c| c.id == name)
    }

    /// The values of a call's arguments, or why it cannot be made.
    fn vars(c: &Connector, call: &Call) -> Result<Vars, String> {
        let mut vars = Vars::new();
        for a in &c.args {
            let given = call.args.get(&a.name).map(|v| v.as_str().map_or_else(|| v.to_string(), String::from));
            match given.or_else(|| a.default.clone()) {
                Some(v) if v.contains('\0') => return Err(format!("`{}` holds a character a command line cannot", a.name)),
                Some(v) => {
                    vars.insert(a.name.clone(), v);
                }
                None if a.required => return Err(format!("`{}` needs `{}`", c.id, a.name)),
                None => {}
            }
        }
        Ok(vars)
    }

    /// The command line a call comes to, or why it cannot.
    fn argv(c: &Connector, call: &Call) -> Result<Vec<String>, String> {
        template::render(&c.argv, &Self::vars(c, call)?)
    }

    fn endpoint(&self, c: &Connector) -> HttpJson {
        HttpJson::new(c, self.keys.get(&c.id).cloned().unwrap_or_default())
    }
}

impl Tools for CommandTools {
    fn untrusted(&self, call: &Call) -> Option<String> {
        let c = self.row(&call.name)?;
        (c.dialect == Dialect::HttpJson).then(|| format!("net:{}", c.id))
    }

    fn specs(&self) -> Vec<Tool> {
        self.rows
            .iter()
            .map(|c| {
                let props: serde_json::Map<String, Json> = c.args.iter().map(|a| (a.name.clone(), json!({ "type": "string" }))).collect();
                let required: Vec<&str> = c.args.iter().filter(|a| a.required && a.default.is_none()).map(|a| a.name.as_str()).collect();
                Tool { name: c.id.clone(), doc: if c.doc.is_empty() { format!("Run `{}`.", c.program()) } else { c.doc.clone() }, schema: json!({ "type": "object", "properties": props, "required": required }) }
            })
            .collect()
    }

    fn gate(&self, call: &Call, task: &str, site: &Site, read_only: ReadOnly) -> Option<Gated> {
        let c = self.row(&call.name)?;
        let (verb, line) = if c.dialect == Dialect::HttpJson {
            (Verb::Net, self.endpoint(c).address(&Self::vars(c, call).ok()?).ok()?)
        } else {
            (Verb::Shell, Self::argv(c, call).ok()?.join(" "))
        };
        let a: Action = action(&c.id, verb, &line, task, site, read_only);
        let verdict = self.gate.as_ref().map_or_else(|| Verdict::Ask { risk: 40, why: "no rules are loaded".into(), by: "no gate".into() }, |g| g.check(&a));
        let risk = match c.risk {
            Risk::Read => 10,
            Risk::Write | Risk::Net => 40,
            Risk::Exec => 60,
        };
        let question = if verb == Verb::Net { format!("call `{line}`") } else { format!("run `{line}`") };
        match verdict {
            Verdict::Allow { .. } => None,
            Verdict::Ask { .. } => Some(Gated { action: a, question, risk }),
            Verdict::Block { .. } => Some(Gated { action: a, question, risk: 100 }),
        }
    }

    fn run(&self, call: &Call, site: &Site, wrap: &(dyn Fn(Launch) -> Launch + Send + Sync)) -> ToolOut {
        let Some(c) = self.row(&call.name) else {
            return ToolOut { text: format!("there is no tool `{}`", call.name), error: true, ..ToolOut::default() };
        };
        if c.dialect == Dialect::HttpJson {
            let done = Self::vars(c, call).and_then(|vars| self.endpoint(c).call(&vars));
            return match done {
                Ok(text) => ToolOut { text, ..ToolOut::default() },
                Err(e) => ToolOut { text: e, error: true, ..ToolOut::default() },
            };
        }
        let argv = match Self::argv(c, call) {
            Ok(a) => a,
            Err(e) => return ToolOut { text: e, error: true, ..ToolOut::default() },
        };
        let shown = argv.join(" ");
        let mut launch = Launch::new(argv, site.dir.clone(), c.env.clone());
        launch.login = c.login.clone();
        launch.no_net = c.net == Some(false);
        plain(wrap(launch), &shown, SECS)
    }
}

/// Several sets of tools offered as one. A name belongs to the first set that has it; the sets of a model are made so that no two
/// share a name (`Connectors::load` refuses a command tool called like a builtin one).
pub struct Union(pub Vec<std::sync::Arc<dyn Tools>>);

impl Union {
    fn owner(&self, name: &str) -> Option<&std::sync::Arc<dyn Tools>> {
        self.0.iter().find(|t| t.specs().iter().any(|s| s.name == name))
    }
}

impl Tools for Union {
    fn untrusted(&self, call: &Call) -> Option<String> {
        self.owner(&call.name)?.untrusted(call)
    }

    fn specs(&self) -> Vec<Tool> {
        self.0.iter().flat_map(|t| t.specs()).collect()
    }

    fn gate(&self, call: &Call, task: &str, site: &Site, read_only: ReadOnly) -> Option<Gated> {
        self.owner(&call.name)?.gate(call, task, site, read_only)
    }

    fn run(&self, call: &Call, site: &Site, wrap: &(dyn Fn(Launch) -> Launch + Send + Sync)) -> ToolOut {
        match self.owner(&call.name) {
            Some(t) => t.run(call, site, wrap),
            None => ToolOut { text: format!("there is no tool `{}`", call.name), error: true, ..ToolOut::default() },
        }
    }
}
