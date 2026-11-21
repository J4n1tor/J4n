//! The `[[connector]]` rows (`packs/base/kinds/connector.toml`): anything outside, said in data. A row
//! names a program and a dialect; [`Connectors::load`] reads the rows of a [`Tables`] and checks each against what its dialect
//! needs, and [`Connectors::doers`] makes the doers, so a program J4n has never seen is a file and no Rust.

use crate::acp::Acp;
use crate::decide::{Decider, DeciderConfig, DeciderWire, ModelAnswerer, ModelConfig};
use crate::hosts::Models;
use crate::model::anthropic::Anthropic;
use crate::model::doer::{Config as ModelConfig_, Dialect as ModelDialect, ModelDoer};
use crate::model::offer::{AskTool, Offer};
use crate::model::openai::Openai;
use crate::model::{Client, Wire};
use j4n_core::answer::Answerer;
use crate::claude::{self, Claude};
use crate::oneshot::OneShot;
use crate::pi::Pi;
use crate::command::{CommandTools, Union};
use crate::tools::Builtin;
use crate::{proc, template};
use j4n_core::doer::{Doer, Probe};
use j4n_core::expr::Expr;
use j4n_core::gate::Gate;
use j4n_core::place::Launch;
use j4n_core::table::{Kind, Problem, Tables};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// How long a probe is given.
const PROBE_LIMIT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Is {
    Agent,
    Tool,
    Service,
    Model,
    Decider,
    Mcp,
}

impl Is {
    pub fn as_str(self) -> &'static str {
        match self {
            Is::Agent => "agent",
            Is::Tool => "tool",
            Is::Service => "service",
            Is::Model => "model",
            Is::Decider => "decider",
            Is::Mcp => "mcp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Dialect {
    Text,
    JsonLast,
    JsonlMap,
    Acp,
    ClaudeStream,
    PiRpc,
    Test,
    Mcp,
    HttpJson,
    Openai,
    Anthropic,
    Systemone,
    Decisions,
    Builtin,
}

impl Dialect {
    pub fn word(self) -> &'static str {
        match self {
            Dialect::Text => "text",
            Dialect::JsonLast => "json-last",
            Dialect::JsonlMap => "jsonl-map",
            Dialect::Acp => "acp",
            Dialect::ClaudeStream => "claude-stream",
            Dialect::PiRpc => "pi-rpc",
            Dialect::Test => "test",
            Dialect::Mcp => "mcp",
            Dialect::HttpJson => "http-json",
            Dialect::Openai => "openai",
            Dialect::Anthropic => "anthropic",
            Dialect::Systemone => "systemone",
            Dialect::Decisions => "decisions",
            Dialect::Builtin => "builtin",
        }
    }

    /// A dialect that speaks to an HTTP endpoint: it names a `url` and no program.
    pub fn is_http(self) -> bool {
        matches!(self, Dialect::Openai | Dialect::Anthropic | Dialect::Systemone | Dialect::Decisions)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    Pi,
    Omp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    Read,
    Write,
    Net,
    #[default]
    Exec,
}

/// How a program says the models it has: `argv` is run, and each line of what it prints names one (`pi --list-models` prints a table
/// with a `model` column; any other output has the model first on its line).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListSpec {
    #[serde(default)]
    pub argv: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeSpec {
    #[serde(default)]
    pub argv: Vec<String>,
    #[serde(default)]
    pub auth: Vec<String>,
    /// Signed in when any of these is there: `~/path` is a file or folder under the home, `$NAME` a variable that is set.
    #[serde(default)]
    pub signed: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArgSpec {
    pub name: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventSpec {
    pub when: String,
    pub emit: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub cmd: Option<String>,
    #[serde(default)]
    pub out: Option<String>,
    #[serde(default)]
    pub exit: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapSpec {
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub end: Option<String>,
    #[serde(default)]
    pub answer: Option<String>,
    #[serde(default)]
    pub cost: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    /// What a line says the run used: input tokens without the cache parts, output tokens, read from the cache, written to it.
    #[serde(default)]
    pub tokens_in: Option<String>,
    #[serde(default)]
    pub tokens_out: Option<String>,
    #[serde(default)]
    pub cached: Option<String>,
    #[serde(default)]
    pub wrote: Option<String>,
    /// Words (any case) of the program's stderr that say it refused a tool: the line that holds one is named when a run ends on a tool.
    #[serde(default)]
    pub rejects: Vec<String>,
    /// Words (any case) of a tool's error that say the program's own rules refused it, and whose text is those rules: the run says one sentence
    /// (`permit::refusal`) in its place, so the rules are not carried in the Log and the context of the turns after it.
    #[serde(default)]
    pub refused: Vec<String>,
    #[serde(default)]
    pub events: Vec<EventSpec>,
}

/// `[connector.guard]`: J4n makes the permission config of a program that has rules of its own, from the gate's tables (what only reads, the
/// test runners, the refused forms of a reader, the credential paths) and from these rows, and gives it to each task in the variable `env`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuardSpec {
    /// Whose config it is (`opencode`): the one shape that is built.
    pub kind: String,
    /// The variable that carries the config: it outranks the operator's and the project's own, and nothing is written.
    pub env: String,
    /// Added in front of the prompt, and only with the config (`--auto` answers what the config leaves to ask).
    #[serde(default)]
    pub args: Vec<String>,
    /// Allowed at both levels, beyond what the gate's tables give (version checks).
    #[serde(default)]
    pub allow: Vec<String>,
    /// Denied at both levels, after every allow (the folders a `cd` may not leave for).
    #[serde(default)]
    pub deny: Vec<String>,
    /// Allowed only under `--approval full`: the repository's own writes, files an interpreter runs, `mkdir`.
    #[serde(default)]
    pub full_allow: Vec<String>,
    /// Denied under `--approval full`, after everything: the inline forms of an interpreter, `rm`, a push.
    #[serde(default)]
    pub full_deny: Vec<String>,
    /// Allowed under `--approval full` **as written**, with nothing after them: the command with no argument (`python3`, which is what opencode sees of a
    /// script on stdin that follows a `cd … &&`) and a `git config <key>` that reads that one key. The other lists stand for the command with any arguments.
    #[serde(default)]
    pub full_exact: Vec<String>,
    /// Allowed under `--approval full` **only when the task is behind the box**: the deletes, which then can only reach what the wall leaves
    /// writable (the project, the program's own state folders). Last, so they stand over `full_deny` and the refused forms of a reader.
    #[serde(default)]
    pub boxed_allow: Vec<String>,
}

/// For `jsonl-map`: a program that is a local server. `argv` starts it; a line it prints that holds `ready` names its address
/// (`listening on http://127.0.0.1:2800`); each task posts `body` (a JSON template, `{prompt}` escaped for it) to `path` on that address
/// and reads the reply's lines as the program's output; the server is taken down after the answer.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServeSpec {
    pub ready: String,
    #[serde(default)]
    pub path: Option<String>,
    pub body: String,
    /// The body of a run that carries on in a session (`{prompt}` and `{session}`).
    #[serde(default)]
    pub resume: Option<String>,
}

/// What to run once to see that a connector works: the goal to give it, and a word its answer must hold.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanarySpec {
    #[serde(default)]
    pub goal: String,
    /// For a tool: the arguments of a run that has no effect (`--help`), made in place of the goal.
    #[serde(default)]
    pub argv: Vec<String>,
    #[serde(default)]
    pub expect: Option<String>,
}

/// How a harness is signed in to as an account: the login to run with the account's folder as its
/// config, the page the account's window opens at, and how to see afterwards who it is signed in as.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signin {
    #[serde(default)]
    pub argv: Vec<String>,
    /// Added to `argv` on a second sign-in, so the page starts on the right account (`--email {account.email}`).
    #[serde(default)]
    pub again: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub page: Option<String>,
    /// What the account links in from the operator's own config folder, so settings and skills stay one. A file that holds a login
    /// is never linked, whatever is written here.
    #[serde(default)]
    pub share: Vec<String>,
    #[serde(default)]
    pub check: Option<SigninCheck>,
    /// Minutes the person is given to sign in (20 by default).
    #[serde(default)]
    pub wait: Option<u64>,
    /// The same in seconds, for a sign-in that is known to be quick or for a test; it wins over `wait`.
    #[serde(default)]
    pub wait_secs: Option<u64>,
}

/// Run as the account after the login ends: `ok`, `email` and `plan` are JSON pointers into what it prints; without `ok`, an exit
/// of 0 is a yes.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigninCheck {
    #[serde(default)]
    pub argv: Vec<String>,
    #[serde(default)]
    pub ok: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub plan: Option<String>,
}

/// What a child is started with for one job, beyond the row's `modes`: flags, and environment added to the child's.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// The two profiles of a CLI doer, `full` (work in the project: what the tool loads by itself) and `lean` (a chore, a light line, a
/// decider: the least that still answers), and the roles that take `lean` when the setting `profile` is `auto`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profiles {
    #[serde(default)]
    pub full: Profile,
    #[serde(default)]
    pub lean: Profile,
    #[serde(default)]
    pub lean_for: Vec<String>,
    /// Added to every child while the setting `doer.scope` is `project`: what keeps the operator's own folder (user settings, plugins, MCP
    /// servers, other sessions) out of a doer that works in a project.
    #[serde(default)]
    pub project: Profile,
}

/// One `[[connector]]` row.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connector {
    pub id: String,
    #[serde(default)]
    pub doc: String,
    pub is: Is,
    pub dialect: Dialect,
    #[serde(default)]
    pub program: Option<String>,
    /// Programs to look for, in order: the first one on this machine stands where `{found}` is in `probe.argv` and
    /// `start` (the browser of account windows: Chromium, Chrome, Brave or Edge, on Linux or macOS).
    #[serde(default)]
    pub candidates: Vec<String>,
    /// The candidate that was found when the row was read; `None` with candidates: none of them is here.
    #[serde(skip)]
    pub found: Option<String>,
    #[serde(default)]
    pub family: Option<Family>,
    #[serde(default)]
    pub probe: ProbeSpec,
    #[serde(default)]
    pub start: Vec<String>,
    #[serde(default)]
    pub resume: Vec<String>,
    #[serde(default)]
    pub model: Vec<String>,
    #[serde(default)]
    pub modes: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub oneshot: Vec<String>,
    /// For `claude-stream`: the profiles a task is started with.
    #[serde(default)]
    pub profile: Profiles,
    /// For a program with permission rules of its own: the config J4n makes for each task, and the arguments that go with it.
    #[serde(default)]
    pub guard: Option<GuardSpec>,
    /// `--approval full`, told by the wire: the program's config is the wider one. Not a field of a row.
    #[serde(skip)]
    pub permit_full: bool,
    /// The setting `profile`, told by the wire: which of them a task gets. Not a field of a row.
    #[serde(skip)]
    pub pick: crate::claude::Pick,
    /// The setting `doer.scope` is `project`, told by the wire: the child gets the row's `profile.project`. Not a field of a row.
    #[serde(skip)]
    pub scoped: bool,
    /// J4n's own key file for this connector (`<home>/keys/<id>`), told by the wire for a row that has a `key`. Not a field of a row.
    #[serde(skip)]
    pub kept: Option<std::path::PathBuf>,
    /// J4n's key file for the provider that names the same key reference (`<home>/keys/<provider>`): what a key typed for the provider while no
    /// connector of it was on gives this one, read after its own. Told by the wire. Not a field of a row.
    #[serde(skip)]
    pub kept_for: Option<std::path::PathBuf>,
    /// J4n's key store, told by the wire for a program the operator gave its provider's key (the setting `doer.keys`): only then is a `kept:`
    /// secret given to its child, and a task that runs as an account still keeps the account's sign-in. Not a field of a row.
    #[serde(skip)]
    pub keyed: Option<j4n_core::keys::Keys>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub secrets: BTreeMap<String, String>,
    /// The operator's own key variables the program reads by itself (`CLAUDE_CODE_OAUTH_TOKEN`): its child is given them as the operator set them.
    #[serde(default)]
    pub pass: Vec<String>,
    /// The names of the setting `keys.pass`, told by the wire for a program that is an agent. Not a field of a row.
    #[serde(skip)]
    pub pass_all: Vec<String>,
    #[serde(default)]
    pub login: Vec<String>,
    /// `false`: a tool of this connector runs with the network cut where a box can cut it (place `box`), whatever `box.net` says. Left out, `box.net` decides.
    #[serde(default)]
    pub net: Option<bool>,
    /// How the program is signed in to as an account.
    #[serde(default)]
    pub signin: Option<Signin>,
    #[serde(default)]
    pub models: Vec<String>,
    /// More models than `models` names, asked of the program: read once a day and when a model word matches none.
    #[serde(default)]
    pub list: Option<ListSpec>,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub args: Vec<ArgSpec>,
    #[serde(default)]
    pub argv: Vec<String>,
    /// For a program that reads its prompt from its standard input: the text written there, a template (`{prompt}`).
    #[serde(default)]
    pub stdin: Option<String>,
    /// For `jsonl-map`: what a line is before it is JSON, in order (`ansi`, `sse`, `frames`, `logfmt`, `colon`, `plain`; [`crate::decode`]).
    #[serde(default)]
    pub decode: Vec<String>,
    /// Each run of the program is put to the Gate first, in write mode (`run \`x\`?`), as a tool's is: for a program that writes without asking and
    /// whose permissions J4n has no way to set (one learned by trying). In read mode the walls hold it and nothing is asked.
    #[serde(default)]
    pub gated: bool,
    /// Where the program says how much of its own usage limit is used and when it ends (see [`crate::limits`]).
    #[serde(default)]
    pub limits: Vec<crate::limits::Row>,
    /// For `jsonl-map`: the program is a local server that each task posts to (see [`ServeSpec`]).
    #[serde(default)]
    pub serve: Option<ServeSpec>,
    #[serde(default)]
    pub risk: Risk,
    /// What the program's own words say a run of it does (a tool learned by trying): the Gate warns by the greater of this and `risk`, so a
    /// connector that `connect.auto` made `read` still shows a delete as one.
    #[serde(default)]
    pub says: Option<Risk>,
    #[serde(default)]
    pub map: Option<MapSpec>,
    /// For the dialects that speak HTTP: the base URL (`https://openrouter.ai/api/v1`), and where the key is (`env:NAME`).
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
    /// An account's proxy (`system` or a URL), which replaces the system's for this connector's calls.
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default)]
    pub system: Option<String>,
    #[serde(default)]
    pub context: Option<u64>,
    #[serde(default)]
    pub effort: Option<String>,
    /// Where the question of an answerer goes, for egress: `any` (default), `trusted`, or `local`.
    #[serde(default)]
    pub zone: Option<String>,
    /// A local model needs no key.
    #[serde(default)]
    pub keyless: bool,
    /// For `test`: the scripted doer takes a Brief with each line, as a model does, so a test can read what it was given.
    #[serde(default)]
    pub brief: bool,
    /// For a model that works on its own: the ids of the connectors (dialect `builtin`) whose tools it is given.
    #[serde(default)]
    pub tools: Vec<String>,
    /// What the Connect flow runs once before it proposes the connector.
    #[serde(default)]
    pub canary: Option<CanarySpec>,
    /// For `http-json`: the method (`get` by default), the headers and the body as templates over the `args`, and where in the
    /// JSON answer the words are (a pointer such as `/data/0/text`; left out, the whole body).
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub result: Option<String>,
    /// For `mcp`: the hash of each tool of the server that the operator accepted, by the tool's name. A tool that is not here, or
    /// whose description or schema has since changed, is not offered to a model.
    #[serde(default)]
    pub pins: BTreeMap<String, String>,
    /// For `mcp`: the tools of the server may be called from the cells of the kit (`j4n.call`), under the Gate as a model's call is.
    /// Off by default: a server's answers are untrusted and its effects are outside J4n.
    #[serde(default)]
    pub code: bool,
    /// The operator turned it off (Agents › Overview, `o`): the row stays, and it is not read into the doers at the start.
    #[serde(default)]
    pub off: bool,
}

impl Kind for Connector {
    const NAME: &'static str = "connector";
}

impl Connector {
    /// The program, which the dialects that start one must have.
    pub fn program(&self) -> &str {
        self.program.as_deref().unwrap_or_default()
    }

    /// The argument names a template of this connector may use.
    fn known(&self) -> Vec<&str> {
        let mut known = vec!["prompt", "promptfile", "model", "session", "cwd", "effort"];
        if self.is == Is::Service {
            known.push("url");
        }
        known.extend(self.args.iter().map(|a| a.name.as_str()));
        known
    }

    /// What is wrong with the row for its dialect, if anything.
    pub fn check(&self) -> Result<(), String> {
        if self.list.as_ref().is_some_and(|l| l.argv.is_empty()) {
            return Err("`list` needs an `argv`: the command that prints the models".into());
        }
        crate::limits::check(&self.limits)?;
        let dialect = self.dialect;
        if let Some(g) = &self.guard {
            if g.kind != "opencode" || g.env.is_empty() {
                return Err(format!("`guard` makes the config of `opencode` and names the variable that carries it (`kind = \"{}\"`)", g.kind));
            }
            if !matches!(dialect, Dialect::JsonlMap | Dialect::Acp) {
                return Err("`guard` is for a program run once with `jsonl-map`, or kept with `acp`".into());
            }
        }
        if self.code && dialect != Dialect::Mcp {
            return Err("`code` is for an `mcp` server: its tools may be called from the kit's cells".into());
        }
        if dialect == Dialect::Builtin {
            return if self.is == Is::Tool { Ok(()) } else { Err("the dialect `builtin` is the set of tools J4n carries: its connector is a `tool`".into()) };
        }
        if dialect == Dialect::HttpJson {
            return crate::httpjson::check(self);
        }
        if dialect == Dialect::Mcp {
            return self.check_mcp();
        }
        if !self.tools.is_empty() && !matches!(dialect, Dialect::Openai | Dialect::Anthropic) {
            return Err("`tools` is for a model that works on its own (the dialects `openai` and `anthropic`)".into());
        }
        if dialect.is_http() {
            return self.check_http();
        }
        if !matches!(self.is, Is::Agent | Is::Tool | Is::Service) {
            return Err(format!("only an `agent`, a `tool` or a `service` can be started so far, not a {:?}", self.is).to_lowercase());
        }
        if self.is == Is::Service && dialect != Dialect::Text {
            return Err("a `service` is started by what uses it (an account's window), and says how in `start`: its dialect is `text`".into());
        }
        if dialect != Dialect::ClaudeStream && (self.profile.full.args.len() + self.profile.lean.args.len() + self.profile.full.env.len() + self.profile.lean.env.len() + self.profile.lean_for.len() + self.profile.project.args.len() + self.profile.project.env.len()) > 0 {
            return Err("`profile` is for the dialect `claude-stream`: no other doer has a measured profile".into());
        }
        if dialect != Dialect::Test && self.program().is_empty() {
            return Err(format!("the dialect `{}` runs a program: `program` is needed", dialect.word()));
        }
        match dialect {
            Dialect::JsonlMap => {
                if self.start.is_empty() && self.argv.is_empty() && self.stdin.is_none() && !self.env.values().any(|v| v.contains("{prompt}")) {
                    return Err("`jsonl-map` needs `start` (or `argv`, `stdin`, or a variable of `env` with `{prompt}`): where the goal goes".into());
                }
                crate::decode::steps(&self.decode)?;
                let map = self.map.as_ref().ok_or("`jsonl-map` needs a `map`, how its lines become events")?;
                if map.events.is_empty() && map.end.is_none() {
                    return Err("`map` says nothing: it needs `events` or an `end`".into());
                }
                crate::jsonmap::Mapper::new(map)?;
            }
            Dialect::PiRpc if self.family.is_none() => return Err("`pi-rpc` needs `family`: `pi` or `omp`".into()),
            _ => {}
        }
        if (!self.decode.is_empty() || self.serve.is_some()) && !matches!(dialect, Dialect::JsonlMap) {
            return Err("`decode` and `serve` are for the dialect `jsonl-map`".into());
        }
        if self.stdin.is_some() && !matches!(dialect, Dialect::JsonlMap | Dialect::Text) {
            return Err("`stdin` is for the dialects `jsonl-map` and `text`".into());
        }
        if let Some(sv) = &self.serve
            && (sv.ready.trim().is_empty() || !sv.body.contains("{prompt}") || self.argv.is_empty())
        {
            return Err("`serve` needs `ready` (what the server prints before its address), a `body` with `{prompt}`, and the `argv` that starts the server".into());
        }
        let known = self.known();
        for arg in self.start.iter().chain(self.stdin.iter()).chain(self.serve.iter().flat_map(|v| std::iter::once(&v.body).chain(v.resume.iter()))).chain(self.env.values().filter(|v| v.contains('{'))).chain(&self.argv).chain(&self.resume).chain(&self.model).chain(&self.oneshot).chain(self.modes.values().flatten()) {
            if let Some(stray) = template::names(arg).into_iter().find(|n| !known.contains(n)) {
                return Err(format!("`{{{stray}}}` in `{arg}` is not something a run brings (the prompt, model, session, cwd, effort or an argument of the tool)"));
            }
        }
        for e in self.map.iter().flat_map(|m| &m.events) {
            if Expr::parse(&e.when).is_err() {
                return Err(format!("`when = {}` is not an expression", e.when));
            }
        }
        Ok(())
    }

    /// What an `mcp` connector needs: it is an `mcp`, and it is a program that serves on stdio or a URL of a server that streams.
    fn check_mcp(&self) -> Result<(), String> {
        if self.is != Is::Mcp {
            return Err("the dialect `mcp` is for a connector that is `mcp`".into());
        }
        match (self.program.as_deref().filter(|p| !p.is_empty()), self.url.as_deref()) {
            (Some(_), None) => {}
            (None, Some(u)) if u.starts_with("http://") || u.starts_with("https://") => {}
            (None, Some(_)) => return Err("an `mcp` server's `url` starts with http:// or https://".into()),
            (Some(_), Some(_)) => return Err("an `mcp` server is a `program` that serves on stdio or a `url`, not both".into()),
            (None, None) => return Err("an `mcp` server needs a `program` that serves on stdio, or a `url`".into()),
        }
        if self.key.as_deref().is_some_and(|k| !(k.starts_with("env:") || k.starts_with("file:"))) {
            return Err("`key` is where the key is kept: `env:NAME` or `file:/path`".into());
        }
        if let Some(p) = &self.proxy {
            crate::net::Proxy::parse(p)?;
        }
        if self.headers.values().any(|v| template::names(v).iter().any(|n| *n != "key")) {
            return Err("a header of an `mcp` server may hold `{key}` and no other name".into());
        }
        if self.headers.values().any(|v| v.contains("{key}")) && self.key.is_none() {
            return Err("a header uses `{key}`, and the connector names no `key`".into());
        }
        Ok(())
    }

    /// What an HTTP connector needs: a base URL, a proxy that parses, and a kind that fits the dialect.
    fn check_http(&self) -> Result<(), String> {
        let word = self.dialect.word();
        let fits = match self.dialect {
            Dialect::Openai | Dialect::Anthropic => matches!(self.is, Is::Model | Is::Agent),
            _ => self.is == Is::Decider,
        };
        if !fits {
            let want = if matches!(self.dialect, Dialect::Openai | Dialect::Anthropic) { "`model` (or `agent`)" } else { "`decider`" };
            return Err(format!("the dialect `{word}` is for a connector that is {want}, not {:?}", self.is).to_lowercase());
        }
        let url = self.url.as_deref().unwrap_or_default();
        if !(url.starts_with("http://") || url.starts_with("https://")) || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(format!("the dialect `{word}` talks to a `url` that starts with http:// or https://"));
        }
        if let Some(p) = &self.proxy {
            crate::net::Proxy::parse(p)?;
        }
        if self.key.as_deref().is_some_and(|k| !(k.starts_with("env:") || k.starts_with("file:"))) {
            return Err("`key` is where the key is kept: `env:NAME` or `file:/path`".into());
        }
        if self.zone.as_deref().is_some_and(|z| !["any", "trusted", "local"].contains(&z)) {
            return Err("`zone` is `any`, `trusted` or `local`".into());
        }
        Ok(())
    }

    /// The model it speaks to by default.
    pub fn default_model(&self) -> &str {
        self.models.first().map_or("", String::as_str)
    }

    /// The client of an HTTP connector: behind its account's proxy, if it has one.
    pub fn net(&self) -> crate::net::Net {
        let net = crate::net::Net::new();
        match self.proxy.as_deref().map(crate::net::Proxy::parse) {
            Some(Ok(p)) => net.proxied(p),
            _ => net,
        }
    }

    /// The key, read now from where it is kept; empty when there is none (`secret` says why not, and that is no failure here: a
    /// model without a key is a doer that is not logged in).
    /// The order: an `env:` name that is set wins; then the key J4n keeps for the connector (typed in Agents › Providers);
    /// then the row's own `file:`. Each is read now, so what was typed or removed a moment ago is what the next child gets.
    pub fn api_key(&self, secret: &dyn Fn(&str, &str) -> Result<String, String>) -> String {
        self.read_key(secret).unwrap_or_default()
    }

    /// [`Connector::api_key`], and when there is no key, why: `Err(None)` when no place holds one, `Err(Some(why))` when one does and the
    /// reader refused it (a file others can read, a value too short to keep out of the Log), in the reader's words, which never hold the value.
    pub fn read_key(&self, secret: &dyn Fn(&str, &str) -> Result<String, String>) -> Result<String, Option<String>> {
        let Some(reference) = self.key.as_deref() else { return Err(None) };
        let name = j4n_core::keys::marker(&self.id);
        // each place in order, with whether it holds anything there: only a place that does makes the reader's no a refusal
        let home = std::env::var("HOME").unwrap_or_default();
        let env = reference.strip_prefix("env:").map(|var| (reference.to_string(), std::env::var(var).is_ok_and(|v| !v.trim().is_empty())));
        let kept = [self.kept.as_ref(), self.kept_for.as_ref()].into_iter().flatten().filter(|p| p.is_file()).map(|p| (format!("file:{}", p.display()), true));
        let file = reference.strip_prefix("file:").map(|p| (reference.to_string(), p.strip_prefix("~/").map_or_else(|| std::path::PathBuf::from(p), |rest| std::path::Path::new(&home).join(rest)).exists()));
        let places: Vec<(String, bool)> = env.into_iter().chain(kept).chain(file).collect();
        let mut why = None;
        for (reference, there) in &places {
            match secret(&name, reference) {
                Ok(v) => return Ok(v),
                Err(e) if *there => why = why.or(Some(e)),
                Err(_) => {}
            }
        }
        Err(why)
    }

    /// [`Connector::api_key`] as a reader the doer keeps and asks at each start.
    fn key_fn(&self, secret: &Arc<crate::modellist::Secret>) -> crate::model::KeyFn {
        let (c, secret) = (self.clone(), secret.clone());
        Arc::new(move || c.api_key(&*secret))
    }

    /// The environment of a child: `env`, and each secret read from where it points (`env:NAME`, `file:/path`); `secret` is told
    /// the variable's name and the reference, and answers the value (and is where it is registered as one never to be written).
    /// A `kept:<provider>` is the key J4n keeps for that provider, given only where the wire said the program works on it ([`Connector::keyed`]);
    /// elsewhere it is left out, so a program on its own sign-in is never handed a key the operator did not mean for it. A key variable of
    /// J4n's own environment that the row names ([`Connector::named_keys`]) is given as the operator set it, and no other is.
    pub fn environment(&self, secret: &dyn Fn(&str, &str) -> Result<String, String>) -> Result<BTreeMap<String, String>, String> {
        let mut env = self.env.clone();
        for (name, reference) in &self.secrets {
            let reference = match (reference.strip_prefix("kept:"), &self.keyed) {
                (None, _) => reference.clone(),
                (Some(_), None) => continue,
                (Some(p), Some(keys)) => keys.path(p).filter(|f| f.is_file()).map(|f| format!("file:{}", f.display())).ok_or_else(|| format!("secret {name}: no key is kept for {p} · Agents › Providers › {p}: ⏎ types it"))?,
            };
            env.insert(name.clone(), secret(name, &reference).map_err(|e| format!("secret {name}: {e}"))?);
        }
        // a key variable of the operator's own is taken out of what every child inherits; the one this row names is given back to its program
        for name in self.named_keys() {
            if let (false, Ok(v)) = (env.contains_key(name), std::env::var(name)) {
                env.insert(name.to_string(), v);
            }
        }
        Ok(env)
    }

    /// The key variables this row names as its program's own: a name of `secrets` (a `kept:` one too, which the operator may have set
    /// themselves), of `pass` and a `$NAME` of `probe.signed`; and, for an agent's program, those of the setting `keys.pass`.
    fn named_keys(&self) -> impl Iterator<Item = &str> {
        let signed = self.probe.signed.iter().filter_map(|s| s.strip_prefix('$'));
        let names = self.pass.iter().chain(&self.pass_all).map(String::as_str);
        self.secrets.keys().map(String::as_str).chain(names).chain(signed).filter(|n| j4n_core::host::key_like(n))
    }

    /// The variables of `secrets` that are a key J4n keeps (`kept:`): what a task run as an account is not given, as it signs in itself.
    pub fn kept_names(&self) -> impl Iterator<Item = &str> {
        self.secrets.iter().filter(|(_, r)| r.starts_with("kept:")).map(|(n, _)| n.as_str())
    }

    /// Signed in: one of `probe.signed` is there (a login file under the child's home, a key in its environment), else, for a
    /// connector with none listed, one of its `login` paths is; a `probe.auth`, a key of its own (`secrets`) or no `login` asks nothing.
    fn signed(&self, env: &BTreeMap<String, String>) -> bool {
        let home = env.get("HOME").cloned().or_else(|| std::env::var("HOME").ok()).unwrap_or_default();
        let set = |n: &str| env.get(n).cloned().or_else(|| std::env::var(n).ok()).is_some_and(|v| !v.is_empty());
        let there = |s: &str| s.strip_prefix("~/").is_some_and(|r| std::path::Path::new(&home).join(r).exists());
        if self.probe.signed.is_empty() {
            // a key J4n keeps signs it in only where it was given (it is then in `env`)
            let keyed = self.secrets.iter().any(|(n, r)| !r.starts_with("kept:") || set(n));
            return !self.probe.auth.is_empty() || keyed || self.login.iter().all(|l| !l.starts_with("~/")) || self.login.iter().any(|l| there(l));
        }
        self.probe.signed.iter().any(|s| s.strip_prefix('$').map_or_else(|| there(s), set))
    }

    /// Whether the program is there and usable: `probe.argv` (else `--version`) ends well, and `probe.auth` if there is one.
    pub fn probe(&self, env: &BTreeMap<String, String>) -> Probe {
        let ask = |argv: Vec<String>| -> Result<String, String> {
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir());
            let (text, exit) = proc::capture(&Launch::new(argv, cwd, env.clone()), PROBE_LIMIT).map_err(|e| e.to_string())?;
            if exit.code == Some(0) { Ok(text) } else { Err(exit.how()) }
        };
        let argv = if self.probe.argv.is_empty() { vec![self.program().to_string(), "--version".into()] } else { self.probe.argv.clone() };
        let out = ask(argv);
        let version = out.as_ref().ok().and_then(|t| t.lines().next()).and_then(|l| l.split_whitespace().next()).map(String::from);
        let authed = out.is_ok() && (self.probe.auth.is_empty() || ask(self.probe.auth.clone()).is_ok()) && self.signed(env);
        let detail = match (&out, authed) {
            (Err(why), _) => format!("`{}` did not answer: {why}", self.program()),
            // the state word, never the login folders it looked in
            (Ok(_), false) => "not logged in".to_string(),
            (Ok(_), true) => String::new(),
        };
        Probe { installed: out.is_ok(), version, authed, detail }
    }
}

/// The words of `tools` that mean tools which reach out (`web`), besides the id of an `mcp` connector: they are made by whoever
/// carries them (`j4n-reach`), which this crate does not depend on.
pub const REACH_WORDS: [&str; 2] = ["web", "memory"];

/// The word of `tools` for the function `ask` (the loop puts the question; there is no connector behind it).
pub const ASK_WORD: &str = "ask";

/// What a look at an `mcp` server found: each tool as `(name, hash, class)` — the hash is what is pinned, the class (`read`,
/// `write`, `exec`) what the Gate is told — and the words that say it.
#[derive(Debug, Clone, Default)]
pub struct Survey {
    pub tools: Vec<(String, String, String)>,
    pub text: String,
}

/// Makes the tools of a model that reach out.
pub trait Reach: Send + Sync {
    /// The tools for a word of `tools`: `web`, or an `mcp` connector (its row is given).
    fn tools(&self, word: &str, row: Option<&Connector>) -> Option<Arc<dyn crate::model::doer::Tools>>;
    /// Start the server of `row` behind `wrap` (the place's walls), list its tools, and let it go.
    fn survey(&self, row: &Connector, wrap: &dyn Fn(Launch) -> Launch, cwd: &std::path::Path) -> Result<Survey, String>;
}

/// The connectors of a set of tables.
#[derive(Clone, Default)]
pub struct Connectors {
    list: Vec<Connector>,
    /// What the builtin tools of a model ask before they act.
    gate: Option<Gate>,
    reach: Option<Arc<dyn Reach>>,
    /// The pack `guard` is on: a model run tells the Gate what it read from outside.
    guarded: bool,
    /// The setting `kit.mode` is `on`: a model that names `py` or `*` among its tools is given the tool `py`.
    kit: bool,
    /// What the kit's host calls are held to: `kit.judge`, `kit.keep`, `kit.model`, the redactor and the egress check.
    kit_set: crate::kithost::KitSet,
    /// The librarian's shelf, while `librarian.on` is on: the tool `context` is offered where there is a shelf, and nowhere else.
    librarian: Option<Arc<j4n_core::front::librarian::Shelf>>,
    /// The post's mailbox, while `chats.on` is on and a screen uses the chats: the tool `tell` is offered where there is one, and nowhere else.
    post: Option<Arc<j4n_core::front::messages::Mailbox>>,
    /// The tool `chat` is offered beside `tell` where this is set (the word `orchestrate` at the start) and there is a post.
    scouts: bool,
    /// The envoy's tools `charter` and `request` are offered where there is a post and this is set (`crew.envoys` under the word `orchestrate`).
    envoys: bool,
    /// The tool `access` is offered where there is a post and this is set (`orchestrate.access` under the word `orchestrate`).
    access: bool,
    /// The reader of secrets that a doer keeps, to read its key at each start; none: the key is read once, when it is made.
    live: Option<Arc<crate::modellist::Secret>>,
}

impl std::fmt::Debug for Connectors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(&self.list).finish()
    }
}

/// Whether `candidate` is a program on this machine: a path as it stands (spaces allowed, as in a macOS app bundle), or a
/// name on the PATH.
fn candidate_here(candidate: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let runnable = |p: &Path| p.is_file() && p.metadata().is_ok_and(|m| m.permissions().mode() & 0o111 != 0);
    if candidate.contains('/') {
        return runnable(Path::new(candidate));
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path).any(|d| runnable(&d.join(candidate)))
}

impl Connector {
    /// The row with `{found}` filled in from its `candidates`: the first one that is here, else the first one, so that a probe
    /// of it fails and says which program it looked for.
    fn resolved(mut self) -> Connector {
        if self.candidates.is_empty() {
            return self;
        }
        self.found = self.candidates.iter().find(|c| candidate_here(c)).cloned();
        let with = self.found.clone().unwrap_or_else(|| self.candidates[0].clone());
        for w in self.probe.argv.iter_mut().chain(self.start.iter_mut()) {
            *w = w.replace("{found}", &with);
        }
        self
    }
}

impl Connectors {
    /// Read the rows. A row that does not fit its dialect is left out and reported.
    pub fn load(tables: &Tables) -> (Connectors, Vec<Problem>) {
        let mut found = Vec::new();
        let mut list = Vec::new();
        match tables.list::<Connector>() {
            Ok(rows) => {
                for c in rows.into_iter().filter(|c| !c.off).map(Connector::resolved) {
                    match c.check() {
                        Ok(()) => list.push(c),
                        Err(why) => found.push(Problem::refused(format!("[[connector]] {}", c.id), why)),
                    }
                }
            }
            Err(p) => found.push(p),
        }
        // a model is given the tools of a connector that is there: a set J4n carries (`builtin`), a command that was connected as a
        // tool, or `*`, every command that was
        let sets: Vec<String> = list.iter().filter(|c| c.dialect == Dialect::Builtin).map(|c| c.id.clone()).collect();
        let commands: Vec<String> = list.iter().filter(|c| crate::command::offered(c)).map(|c| c.id.clone()).chain(list.iter().filter(|c| c.dialect == Dialect::Mcp).map(|c| c.id.clone())).chain(REACH_WORDS.map(String::from)).chain([ASK_WORD.to_string(), "py".to_string()]).collect();
        list.retain(|c| {
            match c.tools.iter().find(|t| *t != "*" && !sets.contains(t) && !commands.contains(t)) {
                Some(t) => {
                    found.push(Problem::refused(format!("[[connector]] {}", c.id), format!("`tools` names `{t}`, and no connector with the dialect `builtin`, and no command connected as a tool, no `mcp` server and not `web`, is called that")));
                    false
                }
                None => true,
            }
        });
        (Connectors { list, gate: None, reach: None, guarded: false, kit: false, kit_set: Default::default(), librarian: None, post: None, scouts: false, envoys: false, access: false, live: None }, found)
    }

    pub fn all(&self) -> &[Connector] {
        &self.list
    }

    pub fn get(&self, id: &str) -> Option<&Connector> {
        self.list.iter().find(|c| c.id == id)
    }

    /// Who makes the tools that reach out (`web`, `mcp`).
    pub fn set_reach(&mut self, reach: Arc<dyn Reach>) {
        self.reach = Some(reach);
    }

    /// The Gate the builtin tools of a model are checked by before they act.
    pub fn set_gate(&mut self, gate: Gate) {
        self.gate = Some(gate);
    }

    /// The pack `guard` is on: a model that reads from outside is held to the taint rules for the rest of that run.
    pub fn set_guard(&mut self, on: bool) {
        self.guarded = on;
    }

    /// The setting `kit.mode` is `on`: the tool `py` is offered to a model that names it (or `*`).
    pub fn set_kit(&mut self, on: bool) {
        self.kit = on;
    }

    /// The setting `librarian.on` is `on`: the tool `context` is offered to a model that has tools, and to the cells of a doer through the bridge.
    pub fn set_librarian(&mut self, shelf: Option<Arc<j4n_core::front::librarian::Shelf>>) {
        self.librarian = shelf;
    }

    /// The setting `chats.on` is on (and a screen uses the chats): the tool `tell` is offered to a model that has tools, and to the cells of a doer through the bridge.
    pub fn set_post(&mut self, post: Option<Arc<j4n_core::front::messages::Mailbox>>) {
        self.post = post;
    }

    /// The tool `chat` is offered where there is a post and this is set.
    pub fn set_scouts(&mut self, on: bool) {
        self.scouts = on;
    }

    /// The envoy's tools are offered where there is a post and this is set.
    pub fn set_envoys(&mut self, on: bool) {
        self.envoys = on;
    }

    /// The tool `access` is offered where there is a post and this is set.
    pub fn set_access(&mut self, on: bool) {
        self.access = on;
    }

    /// The doers that can ask the librarian for more: models that have tools, and doers that run a program of their own while the kit gives them the bridge.
    pub fn context_doers(&self) -> Vec<String> {
        self.with_tools(self.librarian.is_some())
    }

    /// The doers the tool `chat` is offered to: the same ones, while the start wired the post and the word `orchestrate`.
    pub fn scout_doers(&self) -> Vec<String> {
        self.with_tools(self.post.is_some() && (self.scouts || self.envoys || self.access))
    }

    /// The doers the tool `tell` is offered to: those that have tools, while the start wired the post.
    pub fn tell_doers(&self) -> Vec<String> {
        self.with_tools(self.post.is_some())
    }

    fn with_tools(&self, on: bool) -> Vec<String> {
        let bridge = self.kit && crate::py::python_here();
        let asks = |c: &Connector| match c.dialect {
            Dialect::Openai | Dialect::Anthropic => !c.tools.is_empty(),
            d => bridge && runs_a_program(d),
        };
        self.list.iter().filter(|c| on && c.is != Is::Service && asks(c)).map(|c| c.id.clone()).collect()
    }

    /// What the host calls of the tool `py` are held to.
    pub fn set_kit_set(&mut self, set: crate::kithost::KitSet) {
        self.kit_set = set;
    }

    /// `--approval full`: the permission config of a program that has one is the wider of the two.
    pub fn set_permit_full(&mut self, full: bool) {
        self.list.iter_mut().for_each(|c| c.permit_full = full);
    }

    /// The setting `profile`: which profile a task of a CLI doer gets, `full`, `auto` or `lean`.
    pub fn set_profile(&mut self, pick: crate::claude::Pick) {
        self.list.iter_mut().for_each(|c| c.pick = pick);
    }

    /// The setting `doer.scope`: `project` (the default) gives a CLI doer the row's `profile.project`, `user` leaves it as the tool loads itself.
    pub fn set_scope(&mut self, project: bool) {
        self.list.iter_mut().for_each(|c| c.scoped = project);
    }

    /// The doers a task of `role` is started lean for, and whose `lean` profile says something: the places a `light` line may go to. With the
    /// setting `profile` at `full`, or a `lean` that is empty, none.
    pub fn lean_doers(&self, role: &str) -> Vec<String> {
        use crate::claude::Pick;
        let used = |c: &Connector| match c.pick {
            Pick::Full => false,
            Pick::Lean => true,
            Pick::Auto => c.profile.lean_for.iter().any(|r| r == role),
        };
        self.list.iter().filter(|c| c.dialect == Dialect::ClaudeStream && !(c.profile.lean.args.is_empty() && c.profile.lean.env.is_empty()) && used(c)).map(|c| c.id.clone()).collect()
    }

    /// Say that the program of `id` is `program` (the operator's own, from the environment).
    /// Where J4n keeps the keys typed in Agents › Providers (`<home>/keys`): each row that has a `key` reads its own file there.
    /// `providers` is `(provider id, its key reference)`: a connector with the same reference reads the provider's file after its own.
    pub fn keep_keys(&mut self, dir: &std::path::Path, providers: &[(String, String)]) {
        for c in self.list.iter_mut().filter(|c| c.key.is_some()) {
            c.kept = Some(dir.join(&c.id));
            c.kept_for = providers.iter().find(|(id, r)| Some(r) == c.key.as_ref() && *id != c.id).map(|(id, _)| dir.join(id));
        }
    }

    /// The programs the operator gave their provider's key (the setting `doer.keys`, connector ids): each reads its `kept:` secrets from
    /// `keys`. What it says: the ids that name no connector with a `kept:` secret, which are given nothing.
    pub fn bind_keys(&mut self, keys: &j4n_core::keys::Keys, ids: &[&str]) -> Vec<String> {
        for c in self.list.iter_mut() {
            c.keyed = ids.contains(&c.id.as_str()).then(|| keys.clone());
        }
        ids.iter().filter(|id| !self.list.iter().any(|c| c.id == **id && c.kept_names().next().is_some())).map(|id| format!("doer.keys names `{id}`, which is no program that can be given a key J4n keeps")).collect()
    }

    /// The names of the setting `keys.pass`: every agent's program is given them as the operator set them, a tool and a shell line never.
    pub fn pass_to_agents(&mut self, names: &[&str]) {
        for c in self.list.iter_mut().filter(|c| c.is == Is::Agent && c.program.is_some()) {
            c.pass_all = names.iter().map(|n| n.to_string()).collect();
        }
    }

    /// The reader of secrets the doers and answerers keep, so that each reads its key when it is used and not only when it is made.
    pub fn set_secret(&mut self, secret: Arc<crate::modellist::Secret>) {
        self.live = Some(secret);
    }

    fn key_fn(&self, c: &Connector) -> Option<crate::model::KeyFn> {
        self.live.as_ref().map(|s| c.key_fn(s))
    }

    /// Why the connector's key is not used, read where keys are read live; none: [`model_doer`] reads it once, when the doer is made.
    fn why_fn(&self, c: &Connector) -> Option<crate::model::doer::WhyFn> {
        let (c, s) = (c.clone(), self.live.clone()?);
        Some(Arc::new(move || c.read_key(&*s).err().flatten()))
    }

    pub fn set_program(&mut self, id: &str, program: &str) {
        if let Some(c) = self.list.iter_mut().find(|c| c.id == id) {
            c.program = Some(program.to_string());
        }
    }

    /// A doer for each connector that can be one (an agent, a model or a tool). A connector that cannot be made is reported; a
    /// decider is an answerer and not a doer ([`Connectors::answerers`]).
    pub fn doers(&self, secret: &dyn Fn(&str, &str) -> Result<String, String>) -> (Vec<Arc<dyn Doer>>, Vec<Problem>) {
        self.doers_with(secret, &Models::default())
    }

    /// [`Connectors::doers`], with the model rows that say which model a connector serves by default.
    pub fn doers_with(&self, secret: &dyn Fn(&str, &str) -> Result<String, String>, models: &Models) -> (Vec<Arc<dyn Doer>>, Vec<Problem>) {
        let (mut doers, mut found) = (Vec::new(), Vec::new());
        // one kernel pool for every doer that is given the tool `py`: the own agent's cells and a shell's `j4n py` meet in one kernel for a
        // task. Each doer has its own view of it, with the host that holds what its cells may call.
        let kit = (self.kit && crate::py::python_here()).then(|| self.gate.clone().map_or_else(crate::py::PyTools::new, |g| crate::py::PyTools::new().with_gate(g)));
        let view = |c: &Connector| kit.as_ref().map(|k| Arc::new(k.view(Arc::new(self.kit_host(secret).with_offer(Some(self.bridge_offer(c, secret))).with_guard(self.gate.clone().filter(|_| self.guarded))))));
        for c in self.list.iter().filter(|c| c.is != Is::Service && !matches!(c.dialect, Dialect::Systemone | Dialect::Decisions | Dialect::Builtin | Dialect::Mcp)) {
            if matches!(c.dialect, Dialect::Openai | Dialect::Anthropic) {
                let kit = if c.tools.iter().any(|t| t == "*" || t == "py") { view(c) } else { None };
                let doer = model_doer(c, secret, models, self.gate.as_ref(), (self.guarded, kit.clone(), self.librarian.clone(), self.post.clone(), self.scouts, self.envoys, self.access), self.reach.as_ref(), &self.list, (self.key_fn(c), self.why_fn(c)));
                // a model with the tool `py` is offered the loop in its first text when the goal's shape fits
                doers.push(match kit.filter(|_| crate::py::python_here()) {
                    Some(py) => Arc::new(crate::kitsock::Kitted::offering(doer, py)),
                    None => doer,
                });
                continue;
            }
            match build(c, secret) {
                // a doer that runs a program of its own is told of the kit and given the task's socket
                Ok(d) if runs_a_program(c.dialect) => doers.push(match view(c) {
                    Some(py) => Arc::new(crate::kitsock::Kitted::new(d, py)),
                    None => Arc::new(crate::kitsock::Kitted::plain(d)),
                }),
                Ok(d) => doers.push(d),
                Err(why) => found.push(Problem::refused(format!("[[connector]] {}", c.id), why)),
            }
        }
        (doers, found)
    }
}

/// The model a connector speaks to by default: its `model` row, else the first of its `models`.
fn default_model(c: &Connector, models: &Models) -> String {
    models.default_of(&c.id).unwrap_or_else(|| c.default_model()).to_string()
}

/// Whether a doer of this dialect runs a program of its own with a shell (`j4n py` reaches the kit from there).
fn runs_a_program(d: Dialect) -> bool {
    matches!(d, Dialect::ClaudeStream | Dialect::PiRpc | Dialect::Acp | Dialect::Text | Dialect::JsonLast | Dialect::JsonlMap)
}

/// The sets of tools a row's `tools` words name, apart from `py`, `ask` and the MCP servers: the tools J4n carries, the commands that were
/// connected, and the ones that reach out (`web`, `memory`). A model is offered these; so is code in its cells.
struct Carried {
    builtin: Option<Arc<dyn crate::model::doer::Tools>>,
    commands: Option<Arc<dyn crate::model::doer::Tools>>,
    reached: Vec<Arc<dyn crate::model::doer::Tools>>,
}

fn carried(words: &[String], gate: Option<&Gate>, reach: Option<&Arc<dyn Reach>>, all: &[Connector], secret: &dyn Fn(&str, &str) -> Result<String, String>) -> Carried {
    let named = |id: &str| words.iter().any(|t| t == id);
    let builtin = all.iter().any(|x| x.dialect == Dialect::Builtin && named(&x.id)).then(|| Arc::new(gate.map_or_else(Builtin::new, |g| Builtin::new().with_gate(g.clone()))) as Arc<dyn crate::model::doer::Tools>);
    let rows: Vec<Connector> = all.iter().filter(|x| crate::command::offered(x) && (named("*") || named(&x.id))).cloned().collect();
    let commands = (!rows.is_empty()).then(|| {
        let keys = rows.iter().filter(|x| x.dialect == Dialect::HttpJson && x.key.is_some()).map(|x| (x.id.clone(), x.api_key(secret))).collect();
        let tools = CommandTools::new(rows).with_keys(keys);
        Arc::new(if let Some(g) = gate { tools.with_gate(g.clone()) } else { tools }) as Arc<dyn crate::model::doer::Tools>
    });
    let reached = reach.map(|r| REACH_WORDS.iter().filter(|w| named(w)).filter_map(|w| r.tools(w, None)).collect()).unwrap_or_default();
    Carried { builtin, commands, reached }
}

/// What a model's offer is given beside its connector's own tools: whether the guard is on, the kit's tools, the librarian's shelf and the post's mailbox.
type Offered = (bool, Option<Arc<crate::py::PyTools>>, Option<Arc<j4n_core::front::librarian::Shelf>>, Option<Arc<j4n_core::front::messages::Mailbox>>, bool, bool, bool);

#[allow(clippy::too_many_arguments)]
fn model_doer(c: &Connector, secret: &dyn Fn(&str, &str) -> Result<String, String>, models: &Models, gate: Option<&Gate>, (guarded, kit, more, post, scouts, envoys, access): Offered, reach: Option<&Arc<dyn Reach>>, all: &[Connector], (fresh, why): (Option<crate::model::KeyFn>, Option<crate::model::doer::WhyFn>)) -> Arc<dyn Doer> {
    let dialect = if c.dialect == Dialect::Anthropic { ModelDialect::Anthropic } else { ModelDialect::Openai };
    // a model that is given the tool `py` is told how to use the kit: only while `kit.mode` is `on`, and not where there is no python3
    let py = kit.is_some() && (c.tools.iter().any(|t| t == "*" || t == "py")) && crate::py::python_here();
    let card = kit.as_ref().filter(|_| py).map(|k| k.card(crate::py::CARD)).unwrap_or_default();
    let system = [c.system.as_deref().unwrap_or_default(), card.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n\n");
    let doer = ModelDoer::new(ModelConfig_ {
        id: c.id.clone(),
        dialect,
        base: c.url.clone().unwrap_or_default(),
        key: c.api_key(secret),
        keyless: c.keyless,
        net: c.net(),
        model: default_model(c, models),
        system: system.clone(),
        context: c.context,
        effort: c.effort.clone(),
    })
    .keyed(fresh)
    .refusing(why.unwrap_or_else(|| Arc::new({ let why = c.read_key(secret).err().flatten(); move || why.clone() })));
    if c.tools.is_empty() {
        return Arc::new(doer);
    }
    let named = |id: &str| c.tools.iter().any(|t| t == id);
    let mut sets: Vec<Arc<dyn crate::model::doer::Tools>> = Vec::new();
    // the tools J4n carries and the commands that were connected, with `py` between and `ask` after them; then what reaches out (`web`, `memory`)
    let have = carried(&c.tools, gate, reach, all, secret);
    sets.extend(have.builtin);
    if let Some(tools) = kit.filter(|_| py) {
        sets.push(tools);
    }
    sets.extend(have.commands);
    // the librarian's tool, while its switch is on
    if let Some(shelf) = more {
        sets.push(Arc::new(crate::contexttool::ContextTool(shelf)));
    }
    // the post's tool, while chats are on
    if let Some(post) = post {
        if scouts {
            sets.push(Arc::new(crate::chattool::ChatTool { post: post.clone(), gate: gate.cloned() }));
        }
        if envoys {
            sets.push(Arc::new(crate::envoytool::EnvoyTool { post: post.clone(), gate: gate.cloned() }));
        }
        if access {
            sets.push(Arc::new(crate::accesstool::AccessTool { post: post.clone(), gate: gate.cloned() }));
        }
        sets.push(Arc::new(crate::telltool::TellTool { post, gate: gate.cloned() }));
    }
    // `ask` is the loop's own; each `mcp` server that is named (or all of them, by `*`) is a set of its own
    let asks = named(ASK_WORD);
    if asks {
        sets.push(Arc::new(AskTool));
    }
    sets.extend(have.reached);
    let mut servers: Vec<(String, Arc<dyn crate::model::doer::Tools>)> = Vec::new();
    if let Some(reach) = reach {
        for row in all.iter().filter(|x| x.dialect == Dialect::Mcp && (named("*") || named(&x.id))) {
            servers.extend(reach.tools(&row.id, Some(row)).map(|t| (row.id.clone(), t)));
        }
    }
    let offer = Offer::new(Arc::new(Union(sets)), servers, &system);
    let mut doer = doer.with_tools(Arc::new(offer));
    if let Some(g) = gate.filter(|_| guarded) {
        doer = doer.guarded(g.clone());
    }
    Arc::new(if asks { doer.asking() } else { doer })
}

impl Connectors {
    /// What code in the cells of a doer may call: the tools its row names, built as a model is given them, less `py`,
    /// `ask` and the MCP servers that say nothing of `code`. A doer that runs a program of its own names none (its row may not), so it has what J4n's models have: the
    /// tools their rows name, and an MCP server only where its row says `code = true`. No tool is named here, in `kithost.rs` or in the library: what is in the offer is callable and nothing else.
    pub fn bridge_offer(&self, c: &Connector, secret: &dyn Fn(&str, &str) -> Result<String, String>) -> Arc<dyn crate::model::doer::Tools> {
        let words: Vec<String> = if c.tools.is_empty() { self.list.iter().filter(|m| matches!(m.dialect, Dialect::Openai | Dialect::Anthropic)).flat_map(|m| m.tools.clone()).collect() } else { c.tools.clone() };
        let have = carried(&words, self.gate.as_ref(), self.reach.as_ref(), &self.list, secret);
        // a server's tools only where its row says `code = true`, and the doer names it (or `*`) as it would for a model
        let servers = self.reach.iter().flat_map(|r| self.list.iter().filter(|x| x.dialect == Dialect::Mcp && x.code && words.iter().any(|w| w == "*" || *w == x.id)).filter_map(|x| r.tools(&x.id, Some(x))));
        let context = self.librarian.iter().map(|s| Arc::new(crate::contexttool::ContextTool(s.clone())) as Arc<dyn crate::model::doer::Tools>);
        let tell = self.post.iter().map(|p| Arc::new(crate::telltool::TellTool { post: p.clone(), gate: self.gate.clone() }) as Arc<dyn crate::model::doer::Tools>);
        let chat = self.post.iter().filter(|_| self.scouts).map(|p| Arc::new(crate::chattool::ChatTool { post: p.clone(), gate: self.gate.clone() }) as Arc<dyn crate::model::doer::Tools>);
        let envoy = self.post.iter().filter(|_| self.envoys).map(|p| Arc::new(crate::envoytool::EnvoyTool { post: p.clone(), gate: self.gate.clone() }) as Arc<dyn crate::model::doer::Tools>);
        let access = self.post.iter().filter(|_| self.access).map(|p| Arc::new(crate::accesstool::AccessTool { post: p.clone(), gate: self.gate.clone() }) as Arc<dyn crate::model::doer::Tools>);
        Arc::new(Union(have.builtin.into_iter().chain(have.commands).chain(context).chain(tell).chain(chat).chain(envoy).chain(access).chain(have.reached).chain(servers).collect()))
    }

    /// The host the tool `py` is given: Jev when it has its key and the chat model `kit.model` names (what its cells may call is the doer's: [`Connectors::bridge_offer`]).
    fn kit_host(&self, secret: &dyn Fn(&str, &str) -> Result<String, String>) -> crate::kithost::Host {
        let chat = self.get(&self.kit_set.model).filter(|c| matches!(c.dialect, Dialect::Openai | Dialect::Anthropic)).and_then(|c| {
            let key = c.api_key(secret);
            let wire: Box<dyn Wire> = if c.dialect == Dialect::Anthropic { Box::new(Anthropic) } else { Box::new(Openai) };
            let base = c.url.clone().unwrap_or_default();
            (!key.is_empty() || c.keyless).then(|| crate::kithost::Chat { client: Client::new(wire, c.net().idle(Duration::from_secs(60)), &base, &key), model: c.default_model().to_string(), zone: crate::decide::zone_of(&base, c.zone.as_deref()) })
        });
        crate::kithost::Host::new(self.kit_set.clone()).with_jev(self.decider("jev", secret)).with_chat(chat)
    }

    /// The decider of the `systemone` connector `id`, for a caller that puts several questions to it in one call (quality control's
    /// second tier); none when it has no key or is not that dialect.
    pub fn decider(&self, id: &str, secret: &dyn Fn(&str, &str) -> Result<String, String>) -> Option<Arc<Decider>> {
        let c = self.list.iter().find(|c| c.id == id && c.dialect == Dialect::Systemone && matches!(c.is, Is::Decider))?;
        let key = c.api_key(secret);
        (!key.is_empty() || c.keyless).then(|| Arc::new(decider_of(c, key).keyed(self.key_fn(c))))
    }

    /// The deciders and models that are not made for want of a key (`answerers` leaves them out): their ids.
    pub fn keyless(&self, secret: &dyn Fn(&str, &str) -> Result<String, String>) -> Vec<&str> {
        let wants = |c: &&Connector| c.dialect.is_http() && matches!(c.is, Is::Decider | Is::Model) && !c.keyless && c.api_key(secret).is_empty();
        self.list.iter().filter(wants).map(|c| c.id.as_str()).collect()
    }

    /// The answerers the connectors make: a `decider` is a `decider:<id>`, a `model` is a `model:<id>` that answers closed questions.
    /// One with no key (and not `keyless`) is made only where keys are read live ([`Connectors::set_secret`]), and is not ready until a key
    /// is typed; elsewhere it is not made. With none ready, every point answers by its rule, its rank or its fallback.
    pub fn answerers(&self, secret: &dyn Fn(&str, &str) -> Result<String, String>) -> Vec<Arc<dyn Answerer>> {
        let mut out: Vec<Arc<dyn Answerer>> = Vec::new();
        for c in self.list.iter().filter(|c| c.dialect.is_http() && matches!(c.is, Is::Decider | Is::Model)) {
            let key = c.api_key(secret);
            // with a reader of live keys one is made with no key too, and is ready once a key is typed; without one it is not made
            let live = self.key_fn(c).filter(|_| !c.keyless);
            if key.is_empty() && !c.keyless && live.is_none() {
                continue;
            }
            let base = c.url.clone().unwrap_or_default();
            let a: Arc<dyn Answerer> = match c.dialect {
                Dialect::Systemone | Dialect::Decisions => Arc::new(decider_of(c, key).keyed(self.key_fn(c))),
                _ => {
                    let wire: Box<dyn Wire> = if c.dialect == Dialect::Anthropic { Box::new(Anthropic) } else { Box::new(Openai) };
                    let client = Client::new(wire, c.net().idle(Duration::from_secs(60)), &base, &key).keyed(self.key_fn(c));
                    Arc::new(ModelAnswerer::new(ModelConfig { id: c.id.clone(), model: c.default_model().to_string(), zone: c.zone.clone(), base }, client))
                }
            };
            out.push(match live {
                Some(key) => Arc::new(Keyed(a, key)),
                None => a,
            });
        }
        out
    }
}

/// An answerer that is ready only while its key reads as something: made at the start with no key, it is asked from the
/// question after one is typed, and not after it is removed. Probes pass it by until then, so a missing key is never a failed probe.
struct Keyed(Arc<dyn Answerer>, crate::model::KeyFn);

impl Answerer for Keyed {
    fn id(&self) -> &str {
        self.0.id()
    }
    fn kind(&self) -> j4n_core::answer::Kind {
        self.0.kind()
    }
    fn offer(&self, q: &j4n_core::answer::Question) -> j4n_core::answer::Offer {
        self.0.offer(q)
    }
    fn offer_paid(&self, q: &j4n_core::answer::Question) -> (j4n_core::answer::Offer, Option<j4n_core::answer::Paid>) {
        self.0.offer_paid(q)
    }
    fn model(&self) -> String {
        self.0.model()
    }
    fn zone(&self) -> Option<String> {
        self.0.zone()
    }
    fn ready(&self) -> bool {
        !(self.1)().trim().is_empty()
    }
}

fn decider_of(c: &Connector, key: String) -> Decider {
    Decider::new(DeciderConfig {
        id: c.id.clone(),
        wire: if c.dialect == Dialect::Decisions { DeciderWire::Decisions } else { DeciderWire::Systemone },
        base: c.url.clone().unwrap_or_default(),
        key,
        model: c.default_model().to_string(),
        zone: c.zone.clone(),
        net: c.net().idle(Duration::from_secs(30)),
        backoff: vec![Duration::from_secs(2), Duration::from_secs(4)],
    })
}

/// The doer of one connector.
pub fn build(c: &Connector, secret: &dyn Fn(&str, &str) -> Result<String, String>) -> Result<Arc<dyn Doer>, String> {
    c.check()?;
    let env = c.environment(secret)?;
    Ok(match c.dialect {
        Dialect::Text | Dialect::JsonLast | Dialect::JsonlMap => Arc::new(OneShot::new(c.clone(), env)?),
        Dialect::ClaudeStream => Arc::new(Claude::new(claude::Config::from_connector(c, env))),
        Dialect::PiRpc => Arc::new(Pi::new(c.clone(), env)?),
        Dialect::Acp => Arc::new(Acp::new(c.clone(), env)),
        Dialect::Test => Arc::new(crate::test::TestDoer::new().named(&c.id).briefed(c.brief)),
        Dialect::HttpJson => crate::httpjson::doer(c, secret),
        other => return Err(format!("the dialect `{}` is not built", other.word())),
    })
}
