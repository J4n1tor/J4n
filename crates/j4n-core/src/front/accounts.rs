//! Agents › Add account: the form of a new account and the logins found on this machine, as the rows
//! of one view (`fold:account-add`, pack `accounts`).
//!
//! - **The draft** is the core's: each field row carries its editor (`data.edit`) bound as `draft:account.<field>`, so the
//!   screen sends what is typed as `Intent::Set { kind: "draft" }` and draws nothing of its own; the rows say the next free name, a name
//!   that is taken and the way out. A draft is not an edit: it is in no file and no Log, and it goes once `:account add` is said.
//! - **Found on this machine**: a connector that keeps accounts and whose own probe says it is signed in on the operator's own
//!   login (`claude auth status` on `~/.claude`, a connector's `login` paths) is a row "claude · yours": the connectors' own probes, run on
//!   a thread of their own and never waited for; `u` says `:account use <connector>`, which names it as an account and copies nothing.

use super::{Front, Said};
use crate::value::Value;
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// The form the draft is of (`draft:account.<field>`).
pub const FORM: &str = "account";
/// The page of the form, which `:account add` with no name opens.
pub const PAGE: &str = "account-add";
/// The fields of the form, in the order they are drawn and said.
pub const FIELDS: [&str; 4] = ["connector", "name", "plan", "proxy"];

/// The next free name of an account of `connector`: `claude-1`, then `claude-2`, …, the first that no account has.
pub fn next_free<'a>(taken: impl IntoIterator<Item = &'a str> + Clone, connector: &str) -> String {
    (1..).map(|n| format!("{connector}-{n}")).find(|name| !taken.clone().into_iter().any(|t| t == name)).unwrap_or_default()
}

/// What is wrong with a name for an account, said as the row says it; none: it is a name.
pub fn name_trouble<'a>(name: &str, taken: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let ok = !name.is_empty() && name.len() <= 32 && name.starts_with(|c: char| c.is_ascii_lowercase()) && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if !ok {
        return Some(format!("`{name}` is not a name: lower case letters, digits, - and _, starting with a letter"));
    }
    taken.into_iter().any(|t| t == name).then(|| format!("{name} is an account already"))
}

fn text(s: impl AsRef<str>) -> Value {
    Value::text(s.as_ref())
}

fn field(m: &toml::Table, k: &str) -> String {
    m.get(k).and_then(toml::Value::as_str).unwrap_or_default().to_string()
}

impl Front {
    /// A field of a form's draft, as the screen sent it (`Intent::Set { kind: "draft" }`): kept, never written.
    pub fn draft_set(&self, form: &str, field: &str, value: &serde_json::Value) -> Result<(), String> {
        // the add line of Agents › Overview: a program or a path, kept under its own key
        if form == super::connect::FORM && field == super::connect::FIELD {
            let word = value.as_str().unwrap_or_default().trim().to_string();
            let mut d = self.0.drafts.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let key = format!("{form}.{field}");
            if word.is_empty() { d.remove(&key) } else { d.insert(key, word) };
            return Ok(());
        }
        if form != FORM || !FIELDS.contains(&field) {
            return Err(format!("there is no field `{field}` of a form `{form}` to fill"));
        }
        let word = match value {
            serde_json::Value::String(s) => s.trim().to_string(),
            serde_json::Value::Null => String::new(),
            other => other.to_string(),
        };
        let mut d = self.0.drafts.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // another connector: the name and the plan that were filled for the one before go with it
        if field == "connector" && d.get("connector") != Some(&word) {
            d.remove("name");
            d.remove("plan");
        }
        if word.is_empty() { d.remove(field) } else { d.insert(field.into(), word) };
        Ok(())
    }

    /// The draft of the form is gone: it was said (`:account add`), so the next opens on the next free name.
    pub fn draft_clear(&self) {
        self.0.drafts.lock().unwrap_or_else(std::sync::PoisonError::into_inner).retain(|k, _| k.starts_with("connect."));
    }

    /// `:account add` with no name, or with a connector only (`:setup` opens it): the form, with that connector in it.
    pub(super) fn account_form_open(&self, thread: &str, connector: Option<&str>) -> Said {
        if let Some(c) = connector {
            if !self.account_connectors().iter().any(|(id, _)| id == c) {
                return self.refuse(thread, format!("`{c}` keeps no accounts: Agents › Add account lists the connectors that do"));
            }
            let _ = self.draft_set(FORM, "connector", &serde_json::Value::String(c.into()));
        }
        Said::Open(PAGE.into())
    }

    /// The connectors that keep accounts (a `[connector.signin]`, or an agent with `login` paths), with what they are.
    fn account_connectors(&self) -> Vec<(String, String)> {
        let tables = &self.screen().tables;
        tables
            .ids("connector")
            .into_iter()
            .filter_map(|id| tables.raw("connector", id).map(|m| (id, &m.fields)))
            .filter(|(_, f)| f.get("signin").is_some_and(toml::Value::is_table))
            .map(|(id, f)| (id.to_string(), field(f, "doc")))
            .collect()
    }

    /// The rows of `fold:account-add`: the fields of the form, the add row, and the logins found on this machine.
    pub fn account_add_rows(&self) -> Vec<Value> {
        let screen = self.screen();
        let tables = &screen.tables;
        let accounts: Vec<(String, toml::Table)> = tables.ids("account").into_iter().filter_map(|id| tables.raw("account", id).map(|m| (id.to_string(), m.fields.clone()))).collect();
        let taken: Vec<&str> = accounts.iter().map(|(id, _)| id.as_str()).collect();
        let connectors = self.account_connectors();
        let d = self.0.drafts.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        let mut rows = Vec::new();
        if !connectors.is_empty() {
            let pick = d.get("connector").cloned().filter(|c| connectors.iter().any(|(id, _)| id == c)).unwrap_or_else(|| {
                connectors.iter().find(|(id, _)| id == "claude").unwrap_or(&connectors[0]).0.clone()
            });
            let free = next_free(taken.iter().copied(), &pick);
            let name = d.get("name").cloned().unwrap_or_else(|| free.clone());
            let trouble = name_trouble(&name, taken.iter().copied());
            let plans: Vec<String> = tables.ids("plan").into_iter().filter(|p| tables.raw("plan", p).is_some_and(|m| field(&m.fields, "connector") == pick)).map(String::from).collect();
            let plan = d.get("plan").cloned().filter(|p| plans.contains(p)).unwrap_or_else(|| "none".into());
            let proxy = d.get("proxy").cloned().unwrap_or_else(|| "system".into());
            let row = |id: &str, now: &str, note: String, kind: &str, options: Vec<String>, tone: &str| {
                let mut edit = BTreeMap::from([("kind".to_string(), text(kind)), ("bind".into(), text(format!("draft:{FORM}.{id}"))), ("value".into(), text(now))]);
                if !options.is_empty() {
                    edit.insert("options".into(), Value::List(options.into_iter().map(Value::Str).collect()));
                }
                Value::Map(BTreeMap::from([
                    ("id".to_string(), text(id)),
                    ("kind".into(), text("field")),
                    ("label".into(), text(id)),
                    ("now".into(), text(now)),
                    ("note".into(), text(note)),
                    ("tone".into(), text(tone)),
                    ("free".into(), text(&free)),
                    ("taken".into(), Value::Bool(id == "name" && trouble.is_some())),
                    ("edit".into(), Value::Map(edit)),
                ]))
            };
            let doc = connectors.iter().find(|(id, _)| *id == pick).map(|(_, doc)| doc.clone()).unwrap_or_default();
            rows.push(row("connector", &pick, doc, "enum", connectors.iter().map(|(id, _)| id.clone()).collect(), "plain"));
            let name_note = match &trouble {
                Some(why) if name != free => format!("{why}: t takes {free}"),
                Some(why) => why.clone(),
                None if name == free => "the next free name".into(),
                None => "a short name for you".into(),
            };
            rows.push(row("name", &name, name_note, "str", Vec::new(), if trouble.is_some() { "bad" } else { "plain" }));
            let plan_note = if plans.is_empty() { format!("{pick} has no plan rows: the connector's own") } else { "the plan its requests are paid by".to_string() };
            rows.push(row("plan", &plan, plan_note, "enum", std::iter::once("none".to_string()).chain(plans).collect(), "plain"));
            rows.push(row("proxy", &proxy, "optional: system, or http://host:port, socks5://host:port".into(), "str", Vec::new(), "plain"));
            // the add row: the line it says is the one the operator would type (`:account add …`), which is the approval
            let mut said = format!("{pick} {name}");
            if plan != "none" {
                said.push_str(&format!(" plan={plan}"));
            }
            if proxy != "system" {
                said.push_str(&format!(" proxy={proxy}"));
            }
            let shown = format!("{name} · {}", if plan == "none" { pick.as_str() } else { plan.as_str() });
            rows.push(Value::Map(BTreeMap::from([
                ("id".to_string(), text("add")),
                ("kind".into(), text("add")),
                ("label".into(), text("add account")),
                ("now".into(), text(if trouble.is_some() { "not yet".to_string() } else { shown })),
                ("note".into(), text(if trouble.is_some() { "fix the name first" } else { "⏎ adds it and opens the sign-in" })),
                ("tone".into(), text(if trouble.is_some() { "dim" } else { "accent" })),
                ("ok".into(), Value::Bool(trouble.is_none())),
                ("said".into(), text(said)),
            ])));
        }
        rows.extend(self.found_rows(&accounts));
        rows
    }

    /// The logins of the operator's own that a connector's probe found signed in, each with the account that names it, if one does
    /// (`keeps = "yours"`). None yet while the first probe runs.
    fn found_rows(&self, accounts: &[(String, toml::Table)]) -> Vec<Value> {
        // only a connector that can tell a login from none: `login` paths, or a probe that asks (`probe.auth`, `probe.signed`)
        let tables = &self.screen().tables;
        let tells = |id: &str| {
            let Some(m) = tables.raw("connector", id) else { return false };
            let some = |v: Option<&toml::Value>| v.and_then(toml::Value::as_array).is_some_and(|a| !a.is_empty());
            let probe = m.fields.get("probe");
            some(m.fields.get("login")) || some(probe.and_then(|p| p.get("auth"))) || some(probe.and_then(|p| p.get("signed")))
        };
        let ids: Vec<String> = self.account_connectors().into_iter().map(|(id, _)| id).filter(|id| tells(id)).collect();
        let found = self.found_now(&ids);
        if found.is_empty() {
            return Vec::new();
        }
        let head = Value::Map(BTreeMap::from([("id".to_string(), text("found")), ("kind".into(), text("head")), ("label".into(), text("found on this machine")), ("now".into(), text("J4n copies nothing")), ("note".into(), text("it names the login, and the login stays what it is"))]));
        std::iter::once(head)
            .chain(found.into_iter().map(|c| {
                let named = accounts.iter().find(|(_, f)| field(f, "connector") == c && field(f, "keeps") == "yours").map(|(id, _)| id.clone()).unwrap_or_default();
                let note = if named.is_empty() { "the default login of a task with no account · u uses it as an account".to_string() } else { format!("named {named}: J4n copied nothing, it stays the default login") };
                Value::Map(BTreeMap::from([
                    ("id".to_string(), text(format!("found:{c}"))),
                    ("kind".into(), text("found")),
                    ("connector".into(), text(&c)),
                    ("label".into(), text(format!("{c} · yours"))),
                    ("now".into(), text("signed in")),
                    ("note".into(), text(note)),
                    ("named".into(), text(named)),
                    ("tone".into(), text("good")),
                ]))
            }))
            .collect()
    }

    /// The connectors of `ids` whose own probe says they are signed in on the operator's own login, as the last probe found them; a probe
    /// that is missing or stale is started on a thread of its own, never waited for, and [`Front::found_landed`] counts it in.
    fn found_now(&self, ids: &[String]) -> Vec<String> {
        let f = self.0.found.clone();
        let last = f.last.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let stale = last.as_ref().is_none_or(|(at, _)| at.elapsed() >= STALE);
        let now = last.as_ref().map(|(_, l)| l.clone()).unwrap_or_default();
        drop(last);
        if stale && !f.busy.swap(true, Ordering::SeqCst) {
            let doers: Vec<_> = self.0.doers.iter().filter(|d| ids.iter().any(|i| i == d.id())).cloned().collect();
            std::thread::spawn(move || {
                let signed: Vec<String> = std::thread::scope(|s| {
                    let jobs: Vec<_> = doers.iter().map(|d| s.spawn(move || (d.id().to_string(), d.probe()))).collect();
                    jobs.into_iter().filter_map(|j| j.join().ok()).filter(|(_, p)| p.installed && p.authed).map(|(id, _)| id).collect()
                });
                *f.last.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some((Instant::now(), signed));
                f.busy.store(false, Ordering::SeqCst);
                f.landed.fetch_add(1, Ordering::SeqCst);
            });
        }
        now
    }

    /// How many probes of the logins on this machine have landed: a screen sends the views again when one did.
    pub fn found_landed(&self) -> u64 {
        self.0.found.landed.load(Ordering::SeqCst)
    }

    /// How many answers of the pre-flight's probes have landed (the doctor's look, apart from the screen's work): a screen sends the views
    /// again when one did.
    pub fn setup_landed(&self) -> u64 {
        self.cfg().setup.as_ref().map_or(0, crate::doctor::Check::landed)
    }
}

/// How long what the probes found stands before they are asked again.
const STALE: Duration = Duration::from_secs(10);

/// What the probes of the logins on this machine found last, and whether one runs.
#[derive(Default)]
pub struct Found {
    last: Mutex<Option<(Instant, Vec<String>)>>,
    busy: AtomicBool,
    landed: AtomicU64,
}
