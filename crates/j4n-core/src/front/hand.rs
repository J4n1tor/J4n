//! Pick by hand: the operator, not a Decider or a model, chooses at an ask point. The points they name in `pick.hand` get the `human` step at the end of their chain, and
//! when a walk comes to it with a person at the keyboard, one pick row waits: the options numbered, the suggested one first. A digit picks, Enter takes the suggested
//! one, Esc takes the point's fallback, and so does a minute without an answer. It is the ask card of the parts library, drawn like any other; this file only holds
//! the questions and the pick. Nothing waits in a run with no one to ask. J4n's own questions put a row the same way, worded by the asker ([`Asking`]),
//! and one may take the operator's own words as its answer (the row `custom`).

use super::Front;
use crate::ask::Opt;
use j4n_proto::{AskView, By, Cell, Choice, Reply, Shape, Tone};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// How long a pick row waits for a key before the fallback stands in.
pub const WAIT: Duration = Duration::from_secs(60);
/// The longest part of the line a pick row shows.
const SHOWN: usize = 80;

struct Held {
    id: String,
    asker: String,
    question: String,
    why: String,
    line: String,
    options: Vec<Opt>,
    /// The operator's own words are an answer (the row `custom` is drawn).
    words: bool,
    /// `None` while it waits; `Some(None)` for the fallback; `Some(Some(_))` for an answer.
    done: Option<Option<Picked>>,
}

/// What the operator answered on a pick row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Picked {
    /// One of the options, by its id.
    Id(String),
    /// Their own words, on a row that takes them.
    Words(String),
}

/// A pick row as its asker words it: who asks, the question, why it is asked, the line it is about, the options (the suggested one first once
/// put), whether the operator's own words answer it, and how long it waits.
pub struct Asking<'a> {
    pub asker: &'a str,
    pub question: &'a str,
    pub why: &'a str,
    pub line: &'a str,
    pub options: &'a [Opt],
    pub words: bool,
    pub wait: Duration,
}

/// The ready answers of the row `custom` that leave the choice to J4n: they take the point's fallback.
const LEAVE: [&str; 2] = ["decide yourself", "skip this question"];

/// The points picked by hand and the pick rows that wait.
#[derive(Default)]
pub struct Hands {
    points: Mutex<BTreeSet<String>>,
    open: Mutex<Vec<Held>>,
    woke: Condvar,
    n: AtomicU64,
}

impl Hands {
    /// The points the operator picks at (`pick.hand`).
    pub fn set(&self, points: &[String]) {
        *self.points.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = points.iter().cloned().collect();
    }

    pub fn has(&self, point: &str) -> bool {
        self.points.lock().unwrap_or_else(std::sync::PoisonError::into_inner).contains(point)
    }

    /// Put a pick row and wait for a key: the id picked, or `None` for the fallback (Esc, or no key within `wait`).
    pub fn ask(&self, point: &str, line: &str, options: &[Opt], wait: Duration) -> Option<String> {
        let question = format!("{point}: you pick");
        let a = Asking { asker: "j4n", question: &question, why: "you answer this point yourself: ⏎ takes the first, a digit picks, Esc takes the fallback", line, options, words: false, wait };
        match self.ask_as(&a)? {
            Picked::Id(id) => Some(id),
            Picked::Words(_) => None,
        }
    }

    /// Put a pick row worded by its asker and wait: the answer, or `None` for the fallback (Esc, a ready answer that leaves it to J4n, or no key within the wait).
    pub fn ask_as(&self, a: &Asking) -> Option<Picked> {
        let mut options = a.options.to_vec();
        // the suggested one first
        options.sort_by_key(|o| !o.recommended);
        let id = format!("hand:{}", self.n.fetch_add(1, Ordering::SeqCst) + 1);
        let held = Held { id: id.clone(), asker: a.asker.into(), question: a.question.into(), why: a.why.into(), line: crate::text::line(a.line, SHOWN), options, words: a.words, done: None };
        self.open.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(held);
        let end = Instant::now() + a.wait;
        let mut open = self.open.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            let at = open.iter().position(|h| h.id == id)?;
            if let Some(done) = open[at].done.clone() {
                open.remove(at);
                return done;
            }
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                open.remove(at);
                return None;
            }
            open = self.woke.wait_timeout(open, left).unwrap_or_else(std::sync::PoisonError::into_inner).0;
        }
    }

    /// The pick rows that wait, as the screen is sent them.
    pub fn views(&self) -> Vec<AskView> {
        let open = self.open.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        open.iter()
            .filter(|h| h.done.is_none())
            .map(|h| AskView {
                ask: h.id.clone(),
                asker: h.asker.clone(),
                question: h.question.clone(),
                shape: Shape::Choice,
                options: h.options.iter().map(|o| Choice { id: o.id.clone(), label: o.label.clone(), recommended: o.recommended, by: if o.recommended { By::Core } else { By::Operator }, ..Choice::default() }).collect(),
                facts: vec![Cell::new(h.line.clone(), Tone::Dim)],
                why: h.why.clone(),
                ..AskView::default()
            })
            .collect()
    }

    /// The operator's key for the row `ask`: `None` when it is not a pick row of this run. A pick not among the options is refused and the row waits on.
    pub fn answer(&self, ask: &str, reply: &Reply) -> Option<Result<(), String>> {
        if !ask.starts_with("hand:") {
            return None;
        }
        let mut open = self.open.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(h) = open.iter_mut().find(|h| h.id == ask && h.done.is_none()) else { return Some(Err("that question is not open any more".into())) };
        h.done = match reply {
            Reply::Pick { id } if h.options.iter().any(|o| o.id == *id) => Some(Some(Picked::Id(id.clone()))),
            Reply::Yes => Some(h.options.first().map(|o| Picked::Id(o.id.clone()))),
            Reply::Text { text } if h.words => Some(worded(&h.options, text)),
            Reply::No => Some(None),
            _ => return Some(Err("answer with one of the options".into())),
        };
        self.woke.notify_all();
        Some(Ok(()))
    }
}

impl Front {
    /// The pick rows that wait (`Down::Asks`).
    pub fn hand_asks(&self) -> Vec<AskView> {
        self.0.who.hands().views()
    }

    /// The operator's key for a pick row: `None` when `ask` is not one.
    pub(super) fn hand_reply(&self, thread: &str, ask: &str, reply: &Reply) -> Option<super::Said> {
        Some(match self.0.who.hands().answer(ask, reply)? {
            Ok(()) => super::Said::Nothing,
            Err(why) => self.refuse(thread, why),
        })
    }

    /// Whether `ask` is a pick row, whose key must not wait for the line that is being taken: that line is waiting for it.
    pub fn is_hand_row(ask: &str) -> bool {
        ask.starts_with("hand:")
    }
}

/// The operator's own words on a row that takes them: an option's number, id or label picks it, a ready answer that leaves it to J4n is the fallback,
/// and any other line is their own answer.
fn worded(options: &[Opt], text: &str) -> Option<Picked> {
    let t = text.trim();
    if let Some(o) = t.parse::<usize>().ok().and_then(|n| options.get(n.wrapping_sub(1))).or_else(|| options.iter().find(|o| o.id.eq_ignore_ascii_case(t) || o.label.eq_ignore_ascii_case(t))) {
        return Some(Picked::Id(o.id.clone()));
    }
    let low = t.to_lowercase();
    (!LEAVE.iter().any(|l| low.starts_with(l))).then(|| Picked::Words(t.to_string()))
}
