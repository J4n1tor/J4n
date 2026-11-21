//! What a question can be answered with, numbered as the operator sees it: the fixed answers of a yes or no and of an approval, or the
//! options the core listed for a choice. The same list is drawn in the popup, in the tray and inline, and it is what a digit, `⇥` and
//! `⏎` pick from, so what is shown and what is sent cannot differ.

use crate::parts::pick::MOST;
use j4n_proto::{AskView, By, Choice, Control, Intent, Reply, Shape};

/// What an option sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Send {
    Reply(Reply),
    /// An order to the task that asked: stop it.
    Stop,
    /// The row of the operator's own line (`custom`): its first press is the input, nothing is sent.
    Type,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opt {
    pub id: String,
    /// The glyph slot drawn before the label, if any.
    pub icon: &'static str,
    pub label: String,
    pub note: String,
    /// The one `⇥` takes.
    pub hint: bool,
    /// The note and the hint were made by a model: drawn with `~`.
    pub model: bool,
    pub widens: bool,
    pub send: Send,
}

fn lay<'a>(a: &'a AskView, id: &str) -> Option<&'a Choice> {
    a.options.iter().find(|c| c.id == id)
}

/// Whether a recommendation may be shown as the hint: on an approval only the core's own word or a rule's, never a model's.
fn trusted(a: &AskView, by: By) -> bool {
    a.shape != Shape::Approval || matches!(by, By::Core | By::Rule)
}

fn fixed(a: &AskView, id: &str, icon: &'static str, label: String, widens: bool, send: Send) -> Opt {
    let over = lay(a, id);
    let by = over.map_or(By::Operator, |c| c.by);
    Opt {
        id: id.to_string(),
        icon,
        label: over.filter(|c| !c.label.is_empty()).map_or(label, |c| c.label.clone()),
        note: over.map_or(String::new(), |c| c.note.clone()),
        hint: over.is_some_and(|c| c.recommended) && trusted(a, by),
        model: by == By::Model,
        widens: widens || over.is_some_and(|c| c.widens),
        send,
    }
}

/// An approval the core itself asks (the Setup card): `esc` is its no, and no task stands behind it.
pub fn front_owned(a: &AskView) -> bool {
    a.shape == Shape::Approval && a.asker == "j4n"
}

/// The answers that are ready to send under `custom`, the first the one that leaves the choice to the model: the label, and what the model is told.
const READY: [(&str, &str); 2] = [("decide yourself", "Decide yourself: pick what you judge best and go on."), ("skip it", "Skip this question and go on without it.")];

/// Whether a question has the row `custom` and the answers under it: a choice a model asked, which is not a score. The routing question of the front
/// (`asker` `j4n`) has its own rows and no custom.
pub fn custom(a: &AskView) -> bool {
    a.shape == Shape::Choice && a.asker != "j4n" && !is_score(a)
}

/// The options of a question, in the order they are numbered. `who` is the name of the task that asked, for the `stop` of an approval.
pub fn options(a: &AskView, who: &str) -> Vec<Opt> {
    match a.shape {
        Shape::Text => Vec::new(),
        Shape::Choice => {
            let mut out: Vec<Opt> = a
                .options
                .iter()
                .take(MOST)
                .map(|c| Opt {
                    id: c.id.clone(),
                    icon: "",
                    label: c.label.clone(),
                    note: c.note.clone(),
                    hint: c.recommended,
                    model: c.by == By::Model,
                    widens: c.widens,
                    send: Send::Reply(Reply::Pick { id: c.id.clone() }),
                })
                .collect();
            if custom(a) {
                let own = |id: &str, label: &str, send: Send| Opt { id: id.into(), icon: "", label: label.into(), note: String::new(), hint: false, model: false, widens: false, send };
                out.push(own("custom", "custom", Send::Type));
                out.extend(READY.iter().map(|(label, said)| own(label, label, Send::Reply(Reply::Text { text: (*said).into() }))));
            }
            out
        }
        Shape::YesNo | Shape::Approval => {
            let mut out = vec![fixed(a, "yes", "yes", "yes".into(), false, Send::Reply(Reply::Yes)), fixed(a, "no", "no", "no".into(), false, Send::Reply(Reply::No))];
            if a.shape == Shape::Approval {
                for g in &a.grants {
                    let label = match g.as_str() {
                        "task" => "yes · this task".to_string(),
                        "session" => "yes · this session".to_string(),
                        "here" => "yes · always here".to_string(),
                        other => format!("yes · {other}"),
                    };
                    out.push(fixed(a, g, "yes", label, true, Send::Reply(Reply::Grant { scope: g.clone() })));
                }
                // J4n's own approval (the Setup card) has no task to stop
                if !front_owned(a) {
                    out.push(fixed(a, "stop", "stopped", format!("stop {who}"), false, Send::Stop));
                }
            }
            out
        }
    }
}

impl Opt {
    /// What is asked of the core when this option is taken for `a`.
    pub fn intent(&self, a: &AskView) -> Intent {
        match &self.send {
            Send::Reply(reply) => Intent::Answer { ask: a.ask.clone(), reply: reply.clone() },
            Send::Stop => Intent::Control { task: a.asker.clone(), control: Control::Abort },
            // nothing is sent by it: the line is typed in the composer (see `ask::press`); a caller that asks gets a blank line, which the core refuses
            Send::Type => Intent::Say { text: String::new(), with: Vec::new() },
        }
    }
}

/// A choice whose options are all numbers is a score (`1 2 3 4 5`).
pub fn is_score(a: &AskView) -> bool {
    a.shape == Shape::Choice && !a.options.is_empty() && a.options.iter().all(|c| c.label.trim().parse::<u32>().is_ok())
}

/// The glyph slot of the kind of question.
pub fn shape_icon(a: &AskView) -> &'static str {
    match a.shape {
        Shape::Approval => "risky",
        Shape::Text => "ask-text",
        Shape::Choice if is_score(a) => "ask-score",
        Shape::Choice => "ask-choice",
        Shape::YesNo => "ask",
    }
}
