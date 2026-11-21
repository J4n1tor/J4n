//! `form`: value fields (`fields/`) kept as a draft, and a submit row that says them as one command. What is typed stays in the screen's
//! `values`; `App::set_value` sends no `Intent::Set` for a bind a form holds. The submit is `Intent::Run { command, args }`, which the core
//! already says as `:command args…` (no change in `j4n-proto`).

use crate::fields::text_of;
use j4n_proto::{Intent, Json, Up};
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct Form {
    /// The command the submit row runs (`account`), the words before the values (`add`), and the bindings of the fields in order.
    pub command: String,
    pub head: Vec<String>,
    pub binds: Vec<String>,
}

impl Form {
    pub fn new(command: &str, head: &[&str], binds: &[&str]) -> Form {
        let own = |w: &[&str]| w.iter().map(|s| (*s).into()).collect();
        Form { command: command.into(), head: own(head), binds: own(binds) }
    }

    /// Whether this form keeps the value of `bind` as a draft.
    pub fn holds(&self, bind: &str) -> bool {
        self.binds.iter().any(|b| b == bind)
    }

    /// The submit row: the command with the head and each field's value as a word, a field with none left out.
    pub fn submit(&self, values: &HashMap<String, Json>) -> Vec<Up> {
        let words = self.binds.iter().filter_map(|b| values.get(b)).map(text_of).filter(|w| !w.is_empty());
        vec![Up::Intent { intent: Intent::Run { command: self.command.clone(), args: self.head.iter().cloned().chain(words).collect() } }]
    }

    /// The draft goes: the values of the fields are dropped, so the form opens empty again.
    pub fn clear(&self, values: &mut HashMap<String, Json>) {
        self.binds.iter().for_each(|b| drop(values.remove(b)));
    }
}
