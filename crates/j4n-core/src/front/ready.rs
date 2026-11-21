//! Packs that are ready to go: a pack's `[ready]` names the settings it turns on, and a ready-to-go switch on its Packs row (and `:pack ready on|off <id>`)
//! decides whether enabling the pack writes them. The settings are written into the operator's layer in the same edit that enables the pack, so one
//! `:undo` takes the whole of it back, and the needs check runs at once, so a card for what the feature lacks is shown before it first acts. Turning ready
//! off, or the pack off, takes back what ready wrote (the enable row keeps the ids in `wrote`) unless the operator has changed a value since. A pack that is
//! on without the operator's say (J4n basic) does not fire it by itself: its switch is there, and off, until the operator turns it on.

use super::{Front, Said};
use crate::change::Where;
use crate::pack::{Info, Ready};
use crate::proposal::{Draft, Edited, Step, StepOp};

impl Front {
    /// The value the operator's layer holds for a setting, if it says one.
    fn ready_said_value(&self, id: &str) -> Option<toml::Value> {
        self.screen().tables.layer_entry(Where::User, "setting", id)?.get("value").cloned()
    }

    /// What the enable row of a pack holds now (the files as they are, which a switch said while J4n runs has changed).
    fn ready_row(&self, id: &str) -> toml::Table {
        self.screen().tables.layer_entry(Where::User, "enable", id).cloned().unwrap_or_default()
    }

    /// The steps that write the settings of `ready` that do not hold already, with their ids put into `row.wrote`.
    fn ready_apply(&self, ready: &Ready, row: &mut toml::Table) -> Vec<Step> {
        let screen = self.screen();
        let mut wrote: Vec<String> = row.get("wrote").and_then(toml::Value::as_array).into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect();
        let mut steps = Vec::new();
        for (id, v) in &ready.set {
            let v = v.value();
            if screen.setting_is(id, &v) {
                continue;
            }
            let file = format!("{}.toml", if id.starts_with("theme.") { "look" } else { id.split('.').next().unwrap_or(id) });
            steps.push(Step { to: Where::User, file: Some(file), kind: "setting".into(), id: id.clone(), op: StepOp::Set(toml::Table::from_iter([("value".to_string(), v)])) });
            if !wrote.contains(id) {
                wrote.push(id.clone());
            }
        }
        row.insert("wrote".into(), toml::Value::Array(wrote.into_iter().map(toml::Value::String).collect()));
        steps
    }

    /// The steps that take back what ready wrote, those the operator's layer still holds as ready wrote them; `row.wrote` is emptied.
    fn ready_take_back(&self, ready: &Ready, pack: &str, row: &mut toml::Table) -> Vec<Step> {
        let wrote = self.ready_row(pack).get("wrote").and_then(toml::Value::as_array).cloned().unwrap_or_default();
        let steps = wrote
            .iter()
            .filter_map(toml::Value::as_str)
            .filter(|id| ready.set.get(*id).is_some_and(|v| self.ready_said_value(id) == Some(v.value())))
            .map(|id| Step { to: Where::User, file: None, kind: "setting".into(), id: id.to_string(), op: StepOp::Unset(vec!["value".into()]) })
            .collect();
        row.insert("wrote".into(), toml::Value::Array(Vec::new()));
        steps
    }

    /// The steps that go with enabling (`on`) or disabling a pack: ready's settings written, or taken back. `row` is the enable row being written.
    pub(super) fn ready_enable_steps(&self, info: &Info, on: bool, row: &mut toml::Table) -> Vec<Step> {
        let Some(ready) = &info.ready else { return Vec::new() };
        if !on {
            return self.ready_take_back(ready, &info.id, row);
        }
        match self.ready_row(&info.id).get("ready").and_then(toml::Value::as_bool).unwrap_or(ready.default) {
            true => self.ready_apply(ready, row),
            false => Vec::new(),
        }
    }

    /// `:pack ready on|off <id>`: the switch of a pack's row. On a pack that is on it writes or takes back the settings now; on one that is off it says
    /// whether enabling will.
    pub(super) fn ready_said(&self, thread: &str, args: &[String]) -> Said {
        let said = Said::Command("pack".into());
        let (Some(word), Some(id)) = (args.first().map(String::as_str).filter(|w| matches!(*w, "on" | "off")), args.get(1)) else {
            return self.refuse(thread, "`:pack ready on <pack>` or `:pack ready off <pack>`: Grow › Packs `g` does the same".into());
        };
        let on = word == "on";
        let screen = self.screen();
        let Some(info) = screen.packs.iter().find(|p| p.id == *id) else { return self.refuse(thread, format!("there is no pack `{id}`")) };
        let Some(ready) = info.ready.clone() else { return self.refuse(thread, format!("the pack `{id}` has nothing to switch on for itself")) };
        let row_said = self.ready_row(id);
        let pack_on = row_said.get("on").and_then(toml::Value::as_bool).unwrap_or(info.on);
        let mut row = toml::Table::from_iter([("on".to_string(), toml::Value::Boolean(pack_on)), ("ready".to_string(), toml::Value::Boolean(on))]);
        let steps = match (pack_on, on) {
            (true, true) => self.ready_apply(&ready, &mut row),
            (true, false) => self.ready_take_back(&ready, id, &mut row),
            (false, _) => Vec::new(),
        };
        if row_said.get("ready").and_then(toml::Value::as_bool) == Some(on) && steps.is_empty() {
            return self.refuse(thread, format!("ready to go is {word} for {id} already"));
        }
        let mut all = vec![Step { to: Where::User, file: Some("harness.toml".into()), kind: "enable".into(), id: id.clone(), op: StepOp::Set(row) }];
        all.extend(steps);
        let sets: Vec<String> = ready.set.iter().map(|(k, v)| format!("{k} = {}", word_of(&v.value()))).collect();
        let title = format!("Ready to go {word} for the pack {id}");
        let draft = Draft { key: format!("ready:{word}:{id}"), title, why: format!("{} — {}", info.summary, sets.join(", ")), evidence: 1, steps: all, widens: Vec::new() };
        match self.edit(draft) {
            Ok(Edited::Done { id: made, .. }) => {
                let what = if pack_on { if on { format!("{} written", sets.join(", ")) } else { "what it wrote is taken back".into() } } else { format!("{id} will {}write {} when it is turned on", if on { "" } else { "not " }, sets.join(", ")) };
                self.said_edit(thread, &format!("ready to go is {word} for {id}: {what} (`:undo {made}` takes it back)"), "dim");
                self.need_check(thread);
            }
            Ok(Edited::Proposed { id: made, .. }) => self.note(thread, &format!("proposed {made}: `:accept {made}`"), "warn"),
            Err(why) => return self.refuse(thread, why),
        }
        said
    }
}

/// A value as the operator reads it: words without quotes.
fn word_of(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
