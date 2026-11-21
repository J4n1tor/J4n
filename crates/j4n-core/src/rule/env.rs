//! The world a rule reads: the event, the folds as of now, the settings. Nothing is copied but the event; a fold is read where
//! the rule names it, so a rule that looks at `fold.tasks[task]` does not pay for the other five thousand tasks.

use crate::eval::Env;
use crate::fold::Folds;
use crate::log::Ev;
use crate::value::{Key, Value};

/// The fields of an event a rule may name without `ev.`.
const FIELDS: [&str; 9] = ["seq", "at_ms", "kind", "task", "thread", "by", "cause", "class", "data"];

pub struct RuleEnv<'a> {
    ev: Value,
    folds: &'a Folds,
    set: &'a Value,
    now: u64,
    /// The numbers of `/context`, which the front fills for a rule that asks.
    ctx: Value,
}

impl<'a> RuleEnv<'a> {
    pub fn new(ev: &Ev, folds: &'a Folds, set: &'a Value, now: u64) -> RuleEnv<'a> {
        RuleEnv { ev: ev.row(), folds, set, now, ctx: Value::Missing }
    }

    pub fn with_ctx(self, ctx: Value) -> RuleEnv<'a> {
        RuleEnv { ctx, ..self }
    }
}

impl Env for RuleEnv<'_> {
    fn get(&self, root: &str, path: &[Key]) -> Value {
        match root {
            "ev" | "event" => self.ev.at(path),
            "fold" => match path.split_first() {
                Some((Key::Name(id), rest)) => self.folds.read(id, rest, self.now),
                _ => Value::Missing,
            },
            "set" => self.set.at(path),
            "ctx" => self.ctx.at(path),
            field if FIELDS.contains(&field) => {
                let mut full = vec![Key::Name(field.to_string())];
                full.extend_from_slice(path);
                self.ev.at(&full)
            }
            _ => Value::Missing,
        }
    }

    fn now_ms(&self) -> Option<u64> {
        Some(self.now)
    }
}
