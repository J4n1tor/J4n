//! The operator's edits, in place and live.
//!
//! - An edit the operator makes (a row of a tab, `Intent::Set`, or a typed `:look`, `:lanes`, `:setup doer`, `:skill`, `:reflex`, `:pack`,
//!   `:offload`) applies at once through [`Proposer::edit`](crate::proposal::Proposer::edit): a Change in the Log with an undo, and no
//!   Proposal, so Grow shows nothing of it. Only an edit that widens what J4n may do is still a proposal, asked twice.
//! - After an edit, an `:accept` or an `:undo`, [`Front::settings_changed`] reads the files again and puts the settings where every reader
//!   looks (`Live`, the screen, `Cfg::apply`, each pack's `tune`, the plans), so a setting takes effect at once. A setting that
//!   decides what is wired at the start says `live = false`: its row says it waits for the next start, and `:restart` is the way.

use super::{Cfg, Front, Reload, Said};
use crate::change::{Actor, Where};
use crate::proposal::{Draft, Edited, Step, StepOp};
use crate::setting::{Live, SettingRow, Settings};
use crate::table::Tables;
use serde_json::Value as Json;
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// The kinds a screen may set a field of with `Intent::Set`, and the field of each that it may set.
pub const SET_KINDS: [&str; 6] = ["thread", "setting", "account", "plan", "connector", "chat"];

/// The fields of an `account` row the operator edits in place; `connector` is what the row is, and is set when it is added.
const ACCOUNT_FIELDS: [&str; 3] = ["remark", "proxy", "plan"];
/// The fields of a `plan` row the operator edits in place: the price and what it is for.
const PLAN_FIELDS: [&str; 3] = ["price", "period", "assume"];
/// The one field of a `connector` row the operator edits in place: `off`, which leaves it out of the doers and the choosers at the next start.
const CONNECTOR_FIELDS: [&str; 1] = ["off"];

impl Front {
    /// The same front, with the live handle on the settings, the plans, and how the files are read again after an edit.
    pub fn with_live(self, live: Live, plans: crate::cost::LivePlans, reload: Reload) -> Front {
        match Arc::try_unwrap(self.0) {
            Ok(mut core) => {
                core.live = live;
                core.plans = Some(plans);
                core.reload = Some(reload);
                Front(Arc::new(core))
            }
            Err(shared) => Front(shared),
        }
    }

    /// Read the settings again after every edit, accept and undo the Proposer makes, whoever asked for it (a typed line, a row of a tab,
    /// the doer `account`): the files are what they are, so the front holds what they say.
    pub fn follow_edits(&self) {
        let Some(proposer) = self.proposer() else { return };
        let me = Arc::downgrade(&self.0);
        proposer.on_change(Arc::new(move || {
            if let Some(core) = me.upgrade() {
                Front(core).settings_changed();
            }
        }));
    }

    /// How many times the settings changed while J4n runs: a screen sent fewer is sent the look and the tree again.
    pub fn settings_rev(&self) -> u64 {
        self.0.settings_rev.load(Ordering::SeqCst)
    }

    /// The settings after an edit, an `:accept` or an `:undo`: the files are read again, and what every
    /// reader holds is swapped. Nothing is read again where the front was given no way to (`with_live`): the overlays of the commands stand.
    pub fn settings_changed(&self) {
        let Some(reload) = self.0.reload.clone() else { return };
        let tables = reload();
        let (settings, _) = Settings::load(&tables);
        let was = self.0.live.now();
        self.0.live.set(settings.clone());
        // the word `orchestrate` on or off is a new start for Main: the session it held before the switch is not the session of the mode
        let orchestrating = |s: &Settings| s.word("lanes", "single") == "orchestrate";
        let switched = orchestrating(&was.settings) != orchestrating(&settings);
        let screen = self.screen().with_settings(&settings, tables.clone());
        *self.0.screen.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(screen);
        let mut cfg: Cfg = (*self.cfg()).clone();
        for why in cfg.apply(&settings, &tables) {
            self.note(&cfg.thread, &why, "warn");
        }
        let cfg = Arc::new(cfg);
        *self.0.cfg.write().unwrap_or_else(|e| e.into_inner()) = cfg.clone();
        // each pack's own numbers
        cfg.tunes.iter().for_each(|t| t.tune(&settings));
        cfg.hosts.tune(&settings);
        cfg.inbox.iter().for_each(|i| i.tune(&settings));
        cfg.ledger.iter().for_each(|l| l.tune(&settings));
        cfg.parts.iter().for_each(|p| p.tune(&settings));
        if let Some(d) = self.desk() {
            d.tune(&settings);
        }
        if let Some(lib) = &cfg.skills {
            lib.set_off(settings.word("skills.off", ""));
        }
        // the plans, and the plan each account names
        if let Some(plans) = &self.0.plans {
            plans.set(crate::cost::Plans::with_accounts(&tables).0);
        }
        // what the commands said while J4n ran is what the settings say now
        *self.0.look.lock().unwrap_or_else(|e| e.into_inner()) = None;
        *self.0.lane.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.chores_reset();
        if switched {
            let main = cfg.thread.clone();
            self.write(&main, "thread.fresh", super::reflexes::doing(), serde_json::json!({ "why": "lanes" }));
            self.forget_moves(&main);
            self.note(&main, "the word orchestrate changed · Main starts a new session: the next line starts cold", "dim");
        }
        self.0.who.hands().set(&cfg.hand);
        // a feature that was just switched on and lacks what it needs says so, once
        self.need_check(&cfg.thread);
        // the two switches the Log also says: an edit of the setting is the operator's word, so the Log says it too
        self.switch_said(&was.settings, &settings, "shape.on");
        self.switch_said(&was.settings, &settings, "qc.on");
        self.0.settings_rev.fetch_add(1, Ordering::SeqCst);
    }

    /// `qc.on` and `shape.on` are also said by `:qc` and `:shape`, whose last word in the Log wins over the setting: an edit of the setting
    /// writes the same word, so the two never disagree.
    fn switch_said(&self, was: &Settings, now: &Settings, id: &str) {
        let (a, b) = (was.get(id), now.get(id));
        let Some(crate::value::Value::Bool(on)) = b.filter(|_| a != b) else { return };
        let word = if *on { "on" } else { "off" };
        match id {
            "shape.on" => {
                if let Some(d) = self.desk() {
                    let _ = d.command(&self.cfg().thread, &[word.to_string()]);
                }
            }
            _ if self.screen().packs.iter().any(|p| p.id == "qc" && p.on) => {
                self.write(&self.cfg().thread, "qc.switch", Actor::Operator, serde_json::json!({ "on": on }));
            }
            _ => {}
        }
    }

    /// The grants the operator gave that still count: `fold:grants` rows not used up, and none of a session that is over.
    pub fn grant_rows(&self) -> Vec<crate::value::Value> {
        use crate::value::Value;
        let now = self.log().clock().now_ms();
        self.screen().fold_rows("grants", now).into_iter().filter(|r| matches!(r, Value::Map(m) if m.get("used") != Some(&Value::Bool(true)) && m.get("counts") != Some(&Value::Bool(false)))).collect()
    }

    /// `:grant` lists the grants; `:grant back <id>` takes one back (Settings › Gates `x`): a `grant.revoked` by the operator, and for
    /// one that is always here also its session grant and the row of the operator's `gates.toml`, which goes as the operator's edit (an undo
    /// puts the row back).
    pub(super) fn grant_said(&self, thread: &str, args: &[String]) -> Said {
        use crate::value::Value;
        let text = |m: &std::collections::BTreeMap<String, Value>, k: &str| match m.get(k) {
            Some(Value::Str(s)) => s.clone(),
            _ => String::new(),
        };
        let rows: Vec<_> = self.grant_rows().into_iter().filter_map(|r| if let Value::Map(m) = r { Some(m) } else { None }).collect();
        let Some(id) = args.first().filter(|w| *w == "back").and(args.get(1)) else {
            if args.first().is_some_and(|w| w == "back") {
                return self.refuse(thread, "`:grant back <id>`: the id is a row of Settings › Gates".into());
            }
            let lines: Vec<String> = rows.iter().map(|m| format!("{}  {}  {} {}", text(m, "id"), text(m, "says"), text(m, "verb"), text(m, "target"))).collect();
            self.note(thread, &if lines.is_empty() { "no grant counts now".to_string() } else { lines.join("\n") }, "dim");
            return Said::Command("grant".into());
        };
        let Some(g) = rows.iter().find(|m| text(m, "id") == *id) else { return self.refuse(thread, format!("no grant `{id}` counts now")) };
        let (verb, target) = (text(g, "verb"), text(g, "target"));
        let mut ids = vec![id.clone()];
        let mut back = String::new();
        if text(g, "scope") == "here" {
            ids.extend(rows.iter().filter(|m| text(m, "scope") == "session" && text(m, "verb") == verb && text(m, "target") == target).map(|m| text(m, "id")));
            let row = crate::gate::grant::here_id_of(&verb, &target);
            // the row was written by the Gate's own writer, not by an edit: the tables are read again to hold it
            if self.screen().tables.raw("gate", &row).is_none() {
                self.settings_changed();
            }
            if self.screen().tables.raw("gate", &row).is_some() {
                let step = Step { to: Where::User, file: Some("gates.toml".into()), kind: "gate".into(), id: row.clone(), op: StepOp::Remove };
                let draft = Draft { key: format!("gate:{row}"), title: format!("{verb} {target}: not always here"), why: "the operator's own edit".into(), evidence: 1, steps: vec![step], widens: Vec::new() };
                match self.edit(draft) {
                    Ok(Edited::Done { id: e, .. }) => back = format!(" (`:undo {e}` puts the row of always here back)"),
                    Ok(Edited::Proposed { .. }) => {}
                    Err(why) => return self.refuse(thread, why),
                }
            }
        }
        for g in &ids {
            let taken = crate::gate::grant::Grants::revoke(&Actor::Operator, g).is_ok_and(|ev| self.log().append(ev.thread(thread)).is_ok());
            if !taken {
                return self.refuse(thread, format!("`{g}` was not taken back"));
            }
        }
        self.note(thread, &format!("{verb} {target}: asked again from now{back}"), "dim");
        Said::Command("grant".into())
    }

    /// The operator's own edit: applied at once, a Change in the Log with an undo, never a Grow row; the settings are read again.
    /// An edit that widens what J4n may do is a proposal, asked twice, as before.
    pub(super) fn edit(&self, draft: Draft) -> Result<Edited, String> {
        let proposer = self.proposer().ok_or("nothing here makes proposals or edits")?;
        proposer.edit(draft).map_err(|p| p.msg)
    }

    /// The edit the card said yes to, having named what it widens: applied at once, no Grow row.
    pub(super) fn edit_seen(&self, draft: Draft) -> Result<Edited, String> {
        let proposer = self.proposer().ok_or("nothing here makes proposals or edits")?;
        proposer.edit_seen(draft).map_err(|p| p.msg)
    }

    /// A setting written in the operator's folder for this project, as their edit: the words that say how to take it back, or why not.
    pub(super) fn edit_setting(&self, id: &str, file: &str, value: toml::Value, title: &str) -> Result<String, String> {
        let mut row = toml::Table::new();
        row.insert("value".into(), value);
        let step = Step { to: Where::User, file: Some(file.into()), kind: "setting".into(), id: id.into(), op: StepOp::Set(row) };
        let draft = Draft { key: format!("setting:{id}"), title: title.into(), why: "the operator's own edit".into(), evidence: 1, steps: vec![step], widens: Vec::new() };
        match self.edit(draft)? {
            Edited::Done { id: e, .. } => Ok(self.undo_words(&e, id)),
            Edited::Proposed { id: p, .. } => Ok(format!(" (it widens what J4n may do: `:accept {p}`)")),
        }
    }

    /// `(`:undo e3` takes it back)`, and for a setting that waits for the next start, that it does.
    pub(super) fn undo_words(&self, edit: &str, setting: &str) -> String {
        let waits = self.setting_row(setting).is_some_and(|r| !r.is_live());
        format!(" (`:undo {edit}` takes it back{})", if waits { "; ⟳ it acts from the next start: ⏎ restarts now" } else { "" })
    }

    /// The row of a setting as the Table holds it now.
    fn setting_row(&self, id: &str) -> Option<SettingRow> {
        self.screen().tables.get::<SettingRow>(id).ok()
    }

    /// `Intent::Set` of a kind but `thread`: a setting, a field of an account, or of a plan, checked by the kind's schema and
    /// written as the operator's edit.
    pub(super) fn set_said(&self, thread: &str, kind: &str, id: &str, field: &str, value: &Json) -> Said {
        let said = Said::Command("set".into());
        let done = match kind {
            "setting" => self.set_setting_value(id, field, value),
            "account" => self.set_row("account", id, field, value, &ACCOUNT_FIELDS),
            "plan" => self.set_row("plan", id, field, value, &PLAN_FIELDS),
            "connector" => self.set_row("connector", id, field, value, &CONNECTOR_FIELDS),
            "chat" => self.set_chat(id, field, value),
            _ => Err(format!("there is nothing of kind `{kind}` to change here (there are {})", SET_KINDS.join(", "))),
        };
        match done {
            Ok(words) => {
                self.said_edit(thread, &words, "dim");
                said
            }
            Err(why) => self.refuse(thread, why),
        }
    }

    fn set_setting_value(&self, id: &str, field: &str, value: &Json) -> Result<String, String> {
        if !field.is_empty() && field != "value" {
            return Err(format!("a setting has one field to change, `value`, not `{field}`"));
        }
        let row = self.setting_row(id).ok_or_else(|| format!("there is no setting `{id}`"))?;
        // the word `default` of a pick (Agents › Setup's model) is the setting left as its default, for one that is empty by default and has no such word
        let empty = row.default.as_str() == Some("") && row.words.as_ref().is_none_or(|w| !w.iter().any(|w| w == "default"));
        let value = &match value {
            Json::String(s) if empty && s.trim() == "default" => Json::String(String::new()),
            v => v.clone(),
        };
        let v = row.value_of(value)?;
        let now = self.screen().set.pointer(&format!("/{}", id.replace('.', "/")));
        // a row with `auto` holds `auto`, `on` or `off` while the settings hold the switch it comes to: `on` is not "already" because `auto` comes to on
        let now = if row.auto.is_some() {
            let screen = self.screen();
            screen.tables.raw("setting", id).and_then(|m| m.fields.get("value").or_else(|| m.fields.get("default")).and_then(|v| row.check(v).ok())).unwrap_or(now)
        } else {
            now
        };
        if row.check(&v).ok().as_ref() == Some(&now) {
            return Err(format!("{id} is {} already", word_of(&v)));
        }
        // the file it is written to: the one a `:look` or a `:lanes` writes, else one named after its first word
        let file = match id.split('.').next().unwrap_or(id) {
            "theme" => "look.toml".to_string(),
            first => format!("{first}.toml"),
        };
        let back = self.edit_setting(id, &file, v.clone(), &format!("{id}: {}", word_of(&v)))?;
        Ok(format!("{id} is {}{back}", word_of(&v)))
    }

    fn set_row(&self, kind: &str, id: &str, field: &str, value: &Json, fields: &[&str]) -> Result<String, String> {
        if !fields.contains(&field) {
            return Err(format!("a {kind} row's field to change is one of {}, not `{field}`", fields.join(", ")));
        }
        let tables = self.screen().tables.clone();
        if tables.raw(kind, id).is_none() {
            return Err(format!("there is no {kind} `{id}`"));
        }
        // the file that holds it in the operator's folder, else the one an add writes
        let file = match kind {
            "account" => format!("account/{id}.toml"),
            "connector" => format!("connectors/{id}.toml"),
            _ => "plan.toml".into(),
        };
        // a key of a view sends a value as text: `true` and `false` are the yes and no of a switch
        let value = &match (kind, value) {
            ("connector", Json::String(s)) if matches!(s.trim(), "true" | "false") => Json::Bool(s.trim() == "true"),
            _ => value.clone(),
        };
        let op = match value {
            Json::Null => StepOp::Unset(vec![field.to_string()]),
            Json::String(s) if s.trim().is_empty() => StepOp::Unset(vec![field.to_string()]),
            Json::String(s) => StepOp::Set(toml::Table::from_iter([(field.to_string(), toml::Value::String(s.trim().to_string()))])),
            // a switch of a connection: `off` is a yes or a no, and a no leaves the field out
            Json::Bool(false) => StepOp::Unset(vec![field.to_string()]),
            Json::Bool(b) => StepOp::Set(toml::Table::from_iter([(field.to_string(), toml::Value::Boolean(*b))])),
            // a price typed in the editor of Agents › Plans is a number of dollars: `$20`, not `$20.0`
            Json::Number(n) if kind == "plan" => {
                let x = n.as_f64().unwrap_or_default();
                let words = if x.fract() == 0.0 { format!("${x:.0}") } else { format!("${x:.2}") };
                StepOp::Set(toml::Table::from_iter([(field.to_string(), toml::Value::String(words))]))
            }
            other => return Err(format!("{other} is not words for `{field}`")),
        };
        let shown = match &op {
            StepOp::Set(t) => t.get(field).map(word_of).unwrap_or_default(),
            _ => "left out".into(),
        };
        let step = Step { to: Where::User, file: Some(file), kind: kind.into(), id: id.into(), op };
        let draft = Draft { key: format!("{kind}:{id}:{field}"), title: format!("{kind} {id}: {field} {shown}"), why: "the operator's own edit".into(), evidence: 1, steps: vec![step], widens: Vec::new() };
        match self.edit(draft)? {
            // a connection is read at the start: its switch waits for the next one, and the restart card follows
            Edited::Done { id: e, .. } if kind == "connector" => Ok(format!("connector {id} is {} · ⟳ next start (`:undo {e}` takes it back)", if shown == "true" { "off" } else { "on" })),
            Edited::Done { id: e, .. } => Ok(format!("{kind} {id}: {field} is {shown} (`:undo {e}` takes it back)")),
            Edited::Proposed { id: p, widens } => Ok(format!("{kind} {id}: this widens what J4n may do ({}): `:accept {p}`", widens.join(", "))),
        }
    }

    /// The settings that were changed since the start and wait for the next one (the restart card names them).
    pub fn settings_waiting(&self) -> Vec<String> {
        self.screen().waiting()
    }
}

/// A value as the operator reads it: words without quotes.
fn word_of(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

impl Cfg {
    /// The fields the settings fill, from the settings as they are now: the wiring calls it
    /// at the start and the front after every edit. What is wired at the start (`weigh.on`, `librarian.on`, `chats.on`, `reflexes.on`
    /// and what the connectors say) is not touched. The words for a value that is not one of its words.
    pub fn apply(&mut self, settings: &Settings, tables: &Tables) -> Vec<String> {
        let mut notices = Vec::new();
        let word = |id: &str| Some(settings.word(id, "")).filter(|w| !w.is_empty()).map(String::from);
        let pin = |doer: &str, model: &str| super::Pin { doer: word(doer), model: word(model) };
        self.failover = settings.word("route.failover", "wait").to_string();
        let lanes = settings.word("lanes", "single");
        self.lanes = crate::branch::Switch::from_word(lanes).unwrap_or_else(|| {
            notices.push(format!("the setting `lanes` is `{lanes}`, which is not single, auto, wide or orchestrate: single stands"));
            Default::default()
        });
        self.sends = settings.num("plan.sends", 3.0) as u32;
        self.lay_retry = settings.word("plan.lay_refused", "block") == "retry";
        self.usd_line = settings.num("plan.usd_line", 0.5);
        self.revisions = settings.num("plan.revisions", 3.0) as u32;
        self.grow = settings.num("plan.grow", 3.0) as u32;
        (self.planner, self.reviewer, self.cheap) = (pin("plan.doer", "plan.model"), pin("plan.review_doer", "plan.review_model"), pin("plan.cheap_doer", "plan.cheap_model"));
        self.prices = super::tiers::parse(settings.word("offload.prices", super::tiers::PRICES));
        self.hand = settings.word("pick.hand", "").split_whitespace().map(String::from).collect();
        self.waived = settings.word("need.waived", "").split_whitespace().map(String::from).collect();
        (self.compact_at, self.compact_max) = (settings.num("compact.at", 0.7), settings.num("compact.max", 3.0) as u32);
        let mode = settings.word("session.mode", "auto");
        self.session = super::session::Life::from_word(mode).unwrap_or_else(|| {
            notices.push(format!("the setting `session.mode` is `{mode}`, which is not resume, auto or each: auto stands"));
            Default::default()
        });
        self.session_big = settings.num("session.big", 40_000.0) as u64;
        // the doers the tool `chat` is offered to were set by the start and are not a setting
        self.orchestrate = super::look::Orchestrate { scouts: std::mem::take(&mut self.orchestrate.scouts), ..super::look::Orchestrate::from(settings) };
        self.weigh.answer = settings.word("weigh.answer", "light").to_string();
        self.librarian.more = settings.num("librarian.more", super::librarian::MORE as f64) as usize;
        (self.chats.chars, self.chats.hops, self.chats.rate) = (settings.num("chats.message.chars", 2000.0) as usize, settings.num("chats.message.hops", 3.0) as u32, settings.num("chats.message.rate", 6.0) as u32);
        const DAY: u64 = 86_400_000;
        self.chats.archive = super::archive::Archive { auto: settings.flag("chats.archive.auto", true), idle_ms: settings.dur_ms("chats.archive.idle", DAY), keep_ms: settings.dur_ms("chats.archive.keep", 14 * DAY) };
        self.reflexes.judge = settings.flag("reflexes.judge", false);
        self.reflexes.cap_usd = settings.num("reflexes.cap_usd", 0.25).max(0.0);
        self.reflexes.trust = settings.num("reflexes.trust", 5.0).max(1.0) as usize;
        self.main = pin("lane.doer", "lane.model");
        let (offload, found) = crate::offload::Offload::load(tables, settings.flag("offload.on", true), |k, d| settings.num(k, d), |k, d| settings.word(k, d).to_string());
        notices.extend(found.iter().map(ToString::to_string));
        self.offload = offload;
        let (crew, found) = crate::crew::Crew::load(tables, settings);
        notices.extend(found.iter().map(ToString::to_string));
        self.crew = crew;
        // `crew.on` is read live by the rule that answers for a signed charter
        self.charters.set_on(settings.flag("crew.on", false));
        notices
    }
}
