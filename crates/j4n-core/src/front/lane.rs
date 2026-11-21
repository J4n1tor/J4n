//! The one switch: the setting `lanes`, which the start reads and `:lanes` says while J4n runs.
//! `:lanes auto` is a view of the setting as `:look` is of the theme: the operator's edit in their folder for this project, applied at
//! once and taken back by `:undo`. The switch changes at once.

use super::{Front, Said};
use crate::branch::{Class, Switch};
use crate::change::{Actor, Change};
use serde_json::json;

impl Front {
    /// What acts on its own now: the word `:lanes` last said, or the setting at the start.
    pub fn lanes_now(&self) -> Switch {
        self.0.lane.lock().unwrap_or_else(|e| e.into_inner()).unwrap_or(self.0.cfg().lanes)
    }

    fn set_switch(&self, to: Switch) {
        *self.0.lane.lock().unwrap_or_else(|e| e.into_inner()) = Some(to);
    }

    /// `:lanes` says what each word does and which is on; `:lanes auto` turns it to that word.
    pub(super) fn lanes_said(&self, thread: &str, args: &[String]) -> Said {
        let (said, now) = (Said::Command("lanes".into()), self.lanes_now());
        let Some(word) = args.first() else {
            let all: Vec<String> = Switch::ALL.iter().map(|s| if *s == now { format!("[{}]", s.word()) } else { s.word().to_string() }).collect();
            let what = match now {
                Switch::Single => "a line is a task: no plan keeper, no branches",
                Switch::Auto => "the plan keeper is on; scouts and reviewers branch; the main lane is the only writer",
                Switch::Wide => "also pieces, written in held worktrees and merged one at a time",
                Switch::Orchestrate => "Main has a doer session of its own and works each line itself; a cheap look at each plain line can open chats (`orchestrate.look`), and the crew's seats, the envoys, the plan keeper and the messenger are on; each of those is `auto` and a value you set for it wins",
            };
            self.note(thread, &format!("lanes {} — {what}", all.join(" · ")), "dim");
            return said;
        };
        let Some(to) = Switch::from_word(word) else {
            let words: Vec<&str> = Switch::ALL.iter().map(|s| s.word()).collect();
            return self.refuse(thread, format!("`{word}` is not a word of lanes: it is one of {}", words.join(", ")));
        };
        if to == now {
            return self.refuse(thread, format!("lanes is {word} already"));
        }
        match self.edit_setting("lanes", "lanes.toml", toml::Value::String(word.clone()), &format!("Lanes: {word}")) {
            Ok(back) => {
                self.set_switch(to);
                self.write(thread, "lanes.set", Actor::Operator, json!({ "from": now.word(), "to": word }));
                // the Decider's chat point and the doers' tool `tell` are wired when J4n starts
                let wired = if to == Switch::Orchestrate && self.0.cfg().chats.screen && !self.0.cfg().chats.wired { " · ⟳ the tools `tell`, `chat` and the envoy's, and the chat point of the Decider, wait for the next start" } else { "" };
                self.said_edit(thread, &format!("lanes is {word}{back}{wired}"), "dim");
                if to != Switch::Single && !self.0.cfg().branches.allows(Class::Scout, to) {
                    self.note(thread, "the pack `lanes` is off: the plan is kept, but no scout, reviewer or piece starts · Settings › Packs › lanes: ⏎ turns it on, from the next start", "warn");
                }
            }
            Err(why) => return self.refuse(thread, why),
        }
        said
    }

    /// Changes taken back that were of the switch: it is what the layers below say again.
    pub(super) fn lanes_undone(&self, undone: &[Change]) {
        let screen = self.screen();
        let tables = &screen.tables;
        for c in undone.iter().filter(|c| c.target.kind == "setting" && c.target.id == "lanes") {
            let below = |f: &str| tables.field_below(c.target.layer, "setting", "lanes", f);
            if let Some(to) = c.after.as_ref().and_then(|t| t.get("value")).or_else(|| below("value")).or_else(|| below("default")).and_then(toml::Value::as_str).and_then(Switch::from_word) {
                self.set_switch(to);
            }
        }
    }
}
