//! Commands from data (the kind `command`, `packs/base/kinds/command.toml`): what the operator can ask for by name. The palette lists
//! them, a line that opens with their words runs them, and `j4n <word> …` does the same from a shell. A command is one of the
//! core's own (`core`), starts a flow with what follows its words as the goal (`flow`), or opens a page (`open`); a pack that adds
//! one adds a row of the palette and a word, and the core is not changed for it.

use super::phrase::{FLOW, Phrases};
use crate::table::{Kind, Problem, Tables};
use j4n_proto::{Cell, Intent, Row, Tone};
use serde::Deserialize;
use std::collections::BTreeMap;

/// The command a line is when it opens with the words of a command that opens a page; its first argument is the page.
pub const OPEN: &str = "open";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandRow {
    pub id: String,
    #[serde(default)]
    pub doc: String,
    #[serde(default)]
    pub words: Option<Vec<String>>,
    #[serde(default)]
    pub args: Vec<String>,
    /// Old words that still run the command and are listed nowhere (a rename keeps them).
    #[serde(default)]
    pub was: Vec<String>,
    /// The forms of what follows the words that the command takes (`signin <id>`): a line that fits none is answered with them, and no flow
    /// or task is made for it.
    #[serde(default)]
    pub takes: Vec<String>,
    /// What each word after the command's own is completed from (`front/complete.rs`): `["on|off", "pack"]`.
    #[serde(default)]
    pub complete: Vec<String>,
    #[serde(default)]
    pub core: Option<String>,
    #[serde(default)]
    pub flow: Option<String>,
    #[serde(default)]
    pub open: Option<String>,
}

impl Kind for CommandRow {
    const NAME: &'static str = "command";
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Core(String),
    Flow(String),
    Page(String),
}

#[derive(Debug, Clone)]
pub struct Command {
    pub id: String,
    pub doc: String,
    pub words: Vec<String>,
    pub was: Vec<String>,
    pub args: Vec<String>,
    pub takes: Vec<String>,
    pub complete: Vec<String>,
    pub target: Target,
    /// What stands beside it when its pack is not settled.
    pub tag: String,
}

#[derive(Debug, Clone, Default)]
pub struct Commands {
    list: Vec<Command>,
}

impl Commands {
    /// The commands the Table holds. `tags` says, by the name of a pack's layer, what to put beside its commands. One that does not
    /// do exactly one thing is left out and said.
    pub fn load(tables: &Tables, tags: &BTreeMap<String, String>) -> (Commands, Vec<Problem>) {
        let (mut list, mut found) = (Vec::new(), Vec::new());
        for id in tables.ids(CommandRow::NAME) {
            let r = match tables.get::<CommandRow>(id) {
                Ok(r) => r,
                Err(p) => {
                    found.push(p);
                    continue;
                }
            };
            let target = match (&r.core, &r.flow, &r.open) {
                (Some(c), None, None) => Target::Core(c.clone()),
                (None, Some(f), None) => Target::Flow(f.clone()),
                (None, None, Some(p)) => Target::Page(p.clone()),
                _ => {
                    found.push(Problem::refused(format!("[[command]] {id}"), "a command is one of the core's (`core`), starts a `flow` or `open`s a page: one of the three"));
                    continue;
                }
            };
            let layer = tables.raw(CommandRow::NAME, id).and_then(|m| ["core", "flow", "open"].iter().find_map(|k| m.from.get(*k))).cloned().unwrap_or_default();
            let words = r.words.clone().unwrap_or_else(|| vec![format!(":{}", r.id)]);
            list.push(Command { id: r.id, doc: r.doc, words, was: r.was, args: r.args, takes: r.takes, complete: r.complete, target, tag: tags.get(&layer).cloned().unwrap_or_default() });
        }
        (Commands { list }, found)
    }

    /// Leave out the commands that open a page `has` does not know, and say so.
    pub fn retain_pages(&mut self, has: &dyn Fn(&str) -> bool) -> Vec<Problem> {
        let mut found = Vec::new();
        self.list.retain(|c| match &c.target {
            Target::Page(p) if !has(p) => {
                found.push(Problem::refused(format!("[[command]] {}", c.id), format!("it opens the page `{p}`, and there is no such page")));
                false
            }
            _ => true,
        });
        found
    }

    pub fn all(&self) -> &[Command] {
        &self.list
    }

    /// Give the words of the commands to the phrase table, which is what a line is read against.
    pub fn claim(&self, phrases: &mut Phrases) -> Vec<Problem> {
        let mut found = Vec::new();
        for c in &self.list {
            let at = format!("[[command]] {}", c.id);
            let words: Vec<String> = c.words.iter().chain(&c.was).cloned().collect();
            phrases.take_forms(&words, &c.takes);
            found.extend(phrases.take_slots(&at, &words, &c.complete));
            found.extend(match &c.target {
                Target::Core(name) => phrases.claim(&at, &words, name, None, true),
                Target::Flow(flow) => phrases.claim(&at, &words, FLOW, Some(flow), true),
                Target::Page(page) => phrases.claim(&at, &words, OPEN, Some(page), false),
            });
        }
        found
    }

    /// The palette: a row for each command, its words and what follows them, and the key that runs it.
    pub fn rows(&self) -> Vec<Row> {
        self.list
            .iter()
            .map(|c| {
                let words = c.words.join(" ");
                let args = c.args.iter().map(|a| format!("<{a}>")).collect::<Vec<_>>().join(" ");
                let mut cells = vec![Cell::new(words.clone(), Tone::Accent), Cell::new(args, Tone::Dim), Cell::new(c.doc.clone(), Tone::Plain)];
                if !c.tag.is_empty() {
                    cells.push(Cell::new(c.tag.clone(), Tone::Warn));
                }
                let run = c.words.first().cloned().unwrap_or_default();
                let act = (c.args.is_empty()).then(|| j4n_proto::Act { key: "enter".into(), label: "run".into(), intent: Intent::Say { text: run, with: Vec::new() }, risk: 0 });
                Row { key: c.id.clone(), cells, actions: act.into_iter().collect(), ..Row::default() }
            })
            .collect()
    }
}
