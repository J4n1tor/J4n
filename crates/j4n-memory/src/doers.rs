//! `:remember` and `:recall` as doers: the flows and commands that start them are data (the base pack).

use crate::{Hit, Index};
use j4n_connect::connect::{Job, StepDoer, Work, clip};
use j4n_core::change::Actor;
use j4n_core::doer::Doer;
use j4n_core::log::{Log, New};
use j4n_core::schema::DataClass;
use serde_json::json;
use std::sync::Arc;

pub const DOERS: [&str; 2] = ["remember", "recall"];
/// The most hits `:recall` shows.
const SHOWN: usize = 8;

/// One hit as a line of the chat: when it was said, where, and the words that matched.
pub(crate) fn line(h: &Hit) -> String {
    let kind = match h.kind.as_str() {
        "note" => "note",
        "chat.say" => "you",
        _ => "report",
    };
    let thread = if h.thread.is_empty() { String::new() } else { format!(" in {}", h.thread) };
    format!("#{} {kind}{thread}: {}", h.seq, clip(&h.text.replace('\n', " "), 240))
}

pub fn doers(index: Arc<Index>, log: Log) -> Vec<Arc<dyn Doer>> {
    let (i, l) = (index, log.clone());
    let remember: Work = Arc::new(move |job| remember(job, &log));
    let recall: Work = Arc::new(move |job| recall(job, &i, &l));
    vec![
        Arc::new(StepDoer { id: DOERS[0], doc: "keeps a note in the memory", work: remember }),
        Arc::new(StepDoer { id: DOERS[1], doc: "finds notes, lines and reports by their words", work: recall }),
    ]
}

fn remember(job: &Job, log: &Log) -> Result<String, String> {
    let text = job.goal.trim();
    if text.is_empty() {
        return Err("say what to remember: `:remember the build needs --release`".into());
    }
    let mut ev = New::of("note", Actor::Doer(DOERS[0].into()), json!({ "text": text }));
    if let Some(t) = &job.cx.spec.thread {
        ev = ev.thread(t);
    }
    let ev = log.append(ev).map_err(|p| p.msg)?;
    Ok(format!("remembered as #{}", ev.seq))
}

fn recall(job: &Job, index: &Index, log: &Log) -> Result<String, String> {
    let words = job.goal.trim();
    if words.is_empty() {
        return Err("say what to look for: `:recall release build`".into());
    }
    index.catch_up(log)?;
    let hits = index.search(words, None, DataClass::Private, SHOWN)?;
    if hits.is_empty() {
        return Ok(format!("nothing in the memory matches `{}`", clip(words, 80)));
    }
    Ok(hits.iter().map(line).collect::<Vec<_>>().join("\n"))
}
