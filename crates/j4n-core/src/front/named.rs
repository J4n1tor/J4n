//! A line that names its doer in exact syntax: `@pi deepseek 4.1 build a game`, `@claude opus fix the bug`, `pi: build a game`. Nothing in the middle of a
//! sentence names a doer: `use pi with deepseek` or `ask codex` is a line for the read (`read:asked`), which settles it with a Decider or the quick model.
//! `rule:named` answers `who:work`, `who:main`, `who:plan` and `who:review` from the exact syntax, and the front reads the same words for the model.
//!
//! The words that follow the doer, up to `to`, `for` or the end of the clause, are what the model may be called; the front matches them against the
//! doer's models and keeps the longest start of them that is one ([`Named::words`]).

use crate::answer::{Answerer, fn_rule, picked};
use crate::ask::AskShape;

/// The words that may stand between a doer and its model.
const BETWEEN: [&str; 7] = ["with", "and", "a", "an", "the", "model", "using"];
/// The words that end what the model is called: the task begins.
const STOPS: [&str; 9] = ["to", "for", "so", "then", "that", "which", "in", "please", "but"];
/// How many words after the doer a model may be called by.
const MOST: usize = 8;

/// A doer the line names, and the words after it that may name its model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    pub doer: String,
    pub words: Vec<String>,
}

pub(super) fn clean(token: &str) -> String {
    token.trim_matches(|c: char| ",.;:!?\"'()".contains(c)).to_lowercase()
}

/// The doer among `ids` that `line` names in exact syntax, if it names one.
pub fn parse(line: &str, ids: &[&str]) -> Option<Named> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let is_doer = |t: &str| ids.iter().find(|id| id.eq_ignore_ascii_case(&clean(t))).map(|id| (*id).to_string());
    let after = |at: usize, doer: String| {
        let mut words: Vec<String> = Vec::new();
        // a clause that ends at the doer names no model
        for (i, t) in tokens.iter().enumerate().skip(at + 1).take_while(|_| !tokens[at].ends_with([',', '.', ';', '!', '?'])) {
            let w = clean(t);
            if w.is_empty() || words.is_empty() && BETWEEN.contains(&w.as_str()) {
                continue;
            }
            if STOPS.contains(&w.as_str()) || words.len() >= MOST {
                break;
            }
            words.push(w);
            if t.ends_with([',', '.', ';', '!', '?']) || i + 1 == tokens.len() {
                break;
            }
        }
        Named { doer, words }
    };
    let first = tokens.first()?;
    if first.ends_with(':')
        && let Some(doer) = is_doer(first)
    {
        return Some(after(0, doer));
    }
    // `@claude opus …`: the at sign, then a doer's id and nothing else in the word (`@c3` is a chat's and is no doer)
    first.strip_prefix('@').filter(|w| !w.is_empty()).and_then(is_doer).map(|doer| after(0, doer))
}

/// `rule:named`: a line that names a doer that is offered picks it; any other line is not this rule's to decide.
pub fn named_rule(doers: Vec<String>) -> impl Answerer {
    fn_rule("rule:named", move |q| {
        let AskShape::Choice { options } = &q.shape else { return Err("it is not a choice of doers".into()) };
        let ids: Vec<&str> = doers.iter().map(String::as_str).collect();
        match parse(&q.text, &ids) {
            Some(n) if options.iter().any(|o| o.id == n.doer) => picked(n.doer.clone(), format!("the line names {}", n.doer)),
            Some(n) => Err(format!("the line names {}, which is not ready for this", n.doer)),
            None => Err("the line names no doer as `@doer` or `doer:`".into()),
        }
    })
}
