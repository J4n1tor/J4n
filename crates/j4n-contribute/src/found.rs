//! What is a finding: an event of the Log that says J4n itself failed, and nothing else. It is deterministic and
//! it errs toward silence. A provider's own error, a login, a missing program, a quota and a cancelled task are the person's setup and are never
//! findings; an error that J4n did not word itself (a provider's 5xx, a network blip: class `transient`, no row) is not either. What counts is
//! what J4n says in its own fixed words when it meets a floor it set, a hang it watched for, a panic it caught, or a stream it could not read.

use j4n_core::log::{Ev, actor_word};

/// A thing J4n says in its own words when it hits one of its own floors: the words, what kind of finding it is, and its id.
/// and the one sentence that stands for it: a finding is made of that, never of the rest of the message, which can hold a program's output.
const OWN: [(&str, &str, &str, &str); 5] = [
    ("the model sent the same reply twice in a row", "floor", "same-reply", "the model sent the same reply twice in a row"),
    ("turns without finishing", "floor", "turn-cap", "the model went on for turns without finishing"),
    ("times and it is still not right", "floor", "flow-back", "a flow step went back as often as it may"),
    ("seconds after it was asked", "hang", "silent-start", "the program said nothing after it was asked"),
    ("did not end with a line of JSON", "dialect", "no-json", "the program did not end with a line of JSON"),
];

/// The three places the engine catches a panic and stops the task, in the words it stops it with.
const PANIC: [&str; 3] = ["the loop failed while handling an order", "the doer stopped without a report", "the doer failed while starting"];

/// The failure rows that are never J4n's: the agent's own budget stop, and a login, a missing program or a model that is gone.
const NOT_OURS: [&str; 2] = ["gone", "agent-stop"];

/// One finding, before it has a fingerprint.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// `floor`, `hang`, `dialect`, `panic` or `word`.
    pub source: &'static str,
    pub id: String,
    pub row: String,
    /// What the fingerprint is made of: the message (or the person's words), stripped by the fingerprint.
    pub text: String,
    /// What the tab shows, as one clipped line.
    pub title: String,
    /// The event it came from.
    pub cause: u64,
}

/// A text as one clipped line.
pub fn clip(text: &str, most: usize) -> String {
    let clean: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.chars().count() > most { format!("{}…", clean.chars().take(most).collect::<String>()) } else { clean }
}

fn text<'a>(ev: &'a Ev, path: &str) -> &'a str {
    ev.data.pointer(path).and_then(|v| v.as_str()).unwrap_or_default()
}

/// The finding an event is, if it is one.
pub fn judge(ev: &Ev) -> Option<Finding> {
    let who = actor_word(&ev.by);
    let row = who.split_once(':').map_or(who.as_str(), |(_, id)| id).to_string();
    match ev.kind.as_str() {
        "run.error" => {
            let (message, failure) = (text(ev, "/text"), text(ev, "/row"));
            if text(ev, "/class") == "rate-limit" || NOT_OURS.contains(&failure) {
                return None;
            }
            let (_, source, id, says) = OWN.iter().find(|(words, ..)| message.contains(words))?;
            Some(Finding { source, id: (*id).into(), row, text: (*says).into(), title: (*says).into(), cause: ev.seq })
        }
        "task.stopped" => {
            let message = text(ev, "/why/message");
            (text(ev, "/why/kind") == "failed" && who == "core" && PANIC.contains(&message))
                .then(|| Finding { source: "panic", id: "caught".into(), row: String::new(), text: message.into(), title: clip(message, 100), cause: ev.seq })
        }
        "run.unread" => {
            let (dialect, kind) = (text(ev, "/dialect"), text(ev, "/kind"));
            (!dialect.is_empty() && !kind.is_empty()).then(|| Finding {
                source: "dialect",
                id: format!("unread:{kind}"),
                row: dialect.into(),
                text: String::new(),
                title: clip(&format!("{dialect}: a frame of kind \"{kind}\" cannot be read"), 100),
                cause: ev.seq,
            })
        }
        _ => None,
    }
}

/// The mark a finding has in the tab.
pub fn mark(source: &str) -> &'static str {
    match source {
        "floor" => "▲",
        "hang" => "◌",
        "word" => "¶",
        _ => "✗",
    }
}
