//! A line sent and not yet seen taken. The screen never waits for the core to show what the operator typed: the
//! line is a turn of the chat the moment ⏎ is pressed (a *pending* row, drawn as the operator's line is), and the core's own row of it replaces
//! that row when it comes. Under the turn the track (`parts::track`) says how far the line has come while nothing has started: sent, routed,
//! a chat opened, a doer started. A key that answers a question puts the word `sending` on its option, until the question is gone.

use crate::app::App;
use j4n_proto::{Cell, Intent, Reply, Row, Tone};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

/// The words of the four marks of the track, as the line stands at each.
pub(crate) const WORDS: [&str; 4] = ["sending", "routing", "opening", "starting"];

/// What a line that waits shows for how long at the most: a core that never answers is not the screen's to wait on for ever.
const SPAN: Duration = Duration::from_secs(60);

/// How long the mark of a key that answered a question stands when the question does not go: the core refused it, and its note says why.
const MARK: Duration = Duration::from_secs(10);

/// The key of the pending row of the line `n` of its chat.
pub(super) const PENDING: &str = "~sent";

/// A line the operator sent from a chat: where, what, when, the turns the chat had before (key and words), and the turn the core made of it.
pub(crate) struct Sent {
    pub bind: String,
    pub text: String,
    pub at: Instant,
    known: BTreeMap<String, String>,
    pub root: Option<String>,
}

/// The turns of a chat: its rows that hang from none.
fn roots(rows: &[Row]) -> impl Iterator<Item = &Row> {
    rows.iter().filter(|r| r.parent.is_none())
}

/// The core's note of where a line went (`→ c3 · new chat · none matched`): the orchestrator's route row.
pub(crate) fn is_route(r: &Row) -> bool {
    r.cells.first().is_some_and(|c| c.icon == "route" || c.icon.is_empty() && c.text.starts_with('→'))
}

/// `text` in quotes, cut to `n` cells.
pub(crate) fn clip(text: &str, n: usize) -> String {
    format!("“{}”", crate::wrap::fit(&text.split_whitespace().collect::<Vec<_>>().join(" "), n))
}

/// A command is answered in a note and is not a line that goes anywhere: it has the pending row and nothing after it.
fn command(text: &str) -> bool {
    text.starts_with([':', '/'])
}

/// How far the turn at `root` has come while nothing has started, as the mark of the track it is at; `None` once something has (a doer's lane
/// with steps under it, a note that answers, a lane that ended or asks). A pending row is at the first mark. `pinned` are the rows of the list of
/// the chats: the orchestrator's turn is routed to a chat, and that chat's own state says whether it has started.
pub(crate) fn stage(rows: &[Row], pinned: &[Row], root: &str) -> Option<usize> {
    if root.starts_with(PENDING) {
        return Some(0);
    }
    fn icon(r: &Row) -> &str {
        r.cells.first().map_or("", |c| c.icon.as_str())
    }
    let kids: Vec<&Row> = rows.iter().filter(|r| r.parent.as_deref() == Some(root)).collect();
    let (mut route, mut lane) = (None, None);
    for k in &kids {
        if k.data.as_ref().is_some_and(|d| d.get("lane").is_some()) {
            lane = Some(*k);
        } else if is_route(k) {
            route = k.cells.first().map(|c| c.text.trim_start_matches('→').trim().split(" · ").next().unwrap_or_default().to_string());
        } else if !matches!(icon(k), "control" | "session" | "grant" | "compact") {
            return None;
        }
    }
    match (lane, route) {
        // a lane that is queued, or works and has said nothing yet
        (Some(l), _) if icon(l) == "queued" || icon(l) == "working" && !rows.iter().any(|r| r.parent.as_deref() == Some(l.key.as_str())) => Some(3),
        (Some(_), _) => None,
        // sent on to a chat: it is opened, then queued, and when it runs the line has been taken
        (None, Some(chat)) => match pinned.iter().find(|p| p.key == chat).and_then(|p| p.data.as_ref()?.get("state")?.as_str()) {
            Some("queued") => Some(3),
            Some("new") | None => Some(2),
            Some(_) => None,
        },
        (None, None) => Some(1),
    }
}

impl App {
    /// The chat the keys are in, if it is drawn from rows.
    fn chat_bind(&self) -> Option<String> {
        self.field().filter(|f| f.kind == j4n_proto::FieldKind::Chat).map(|f| f.bind.clone())
    }

    /// A line was sent: it is a turn of the chat now.
    pub(crate) fn echo(&mut self, text: &str) {
        let Some(bind) = self.chat_bind() else { return };
        let known = self.rows.get(&bind).map(|r| roots(r).map(|r| (r.key.clone(), r.cells.first().map_or_else(String::new, |c| c.text.clone()))).collect()).unwrap_or_default();
        let at = self.clock.unwrap_or_else(Instant::now);
        self.sent.push(Sent { bind, text: text.to_string(), at, known, root: None });
        self.redraw = true;
    }

    /// The rows of `bind` came: the first turn the chat did not have before is the core's row of the oldest line that waited, and takes its place. A
    /// chat whose first thing was not a line of yours has a turn with no words, and your first line is said in that turn (`rows::new_turn`).
    pub(crate) fn sent_sync(&mut self, bind: &str) {
        let rows = self.rows.get(bind).map_or(&[][..], Vec::as_slice);
        let mut taken: BTreeSet<&str> = self.sent.iter().filter(|s| s.bind == bind).filter_map(|s| s.root.as_deref()).collect();
        let mut claims = Vec::new();
        for (i, s) in self.sent.iter().enumerate().filter(|(_, s)| s.bind == bind && s.root.is_none()) {
            let said = |r: &Row| r.cells.first().map_or("", |c| c.text.as_str()).to_string();
            let mine = roots(rows).find(|r| !taken.contains(r.key.as_str()) && s.known.get(&r.key).is_none_or(|before| before.is_empty() && !said(r).is_empty()));
            if let Some(r) = mine {
                taken.insert(r.key.as_str());
                claims.push((i, r.key.clone()));
            }
        }
        for (i, key) in claims {
            self.sent[i].root = Some(key);
        }
        self.sent_gc();
    }

    /// What is done with is dropped: a line whose turn has started, a command that was answered, a line the core never took.
    pub(crate) fn sent_gc(&mut self) {
        let now = self.clock.unwrap_or_else(Instant::now);
        let held = (self.sent.len(), self.ui.sent.len());
        self.ui.sent.retain(|_, (_, at)| now.saturating_duration_since(*at) < MARK);
        let (rows, pinned) = (&self.rows, self.rows.get(j4n_proto::CHATS).map_or(&[][..], Vec::as_slice));
        self.sent.retain(|s| {
            let live = now.saturating_duration_since(s.at) < SPAN;
            let ended = s.root.as_deref().is_some_and(|k| command(&s.text) || stage(rows.get(&s.bind).map_or(&[][..], Vec::as_slice), pinned, k).is_none());
            live && !ended
        });
        self.redraw |= held != (self.sent.len(), self.ui.sent.len());
    }

    /// The pending rows of `bind`, for the transcript to draw after the rows the core sent, and the keys of the turns that show a track.
    pub(crate) fn pending(&self, bind: &str) -> (Vec<Row>, Vec<String>) {
        let mut rows = Vec::new();
        let mut live: Vec<String> = Vec::new();
        for s in self.sent.iter().filter(|s| s.bind == bind) {
            match &s.root {
                None => {
                    let key = format!("{PENDING}{}", rows.len());
                    rows.push(Row { key: key.clone(), cells: vec![Cell::new(s.text.clone(), Tone::Plain), Cell::new("", Tone::Dim)], ..Row::default() });
                    live.push(key);
                }
                Some(k) if !command(&s.text) => live.push(k.clone()),
                Some(_) => {}
            }
        }
        (rows, live)
    }

    /// The keys that went out: an answer to a question is marked on the option it chose until the question is gone.
    pub(crate) fn sent_answers(&mut self, ups: &[j4n_proto::Up]) {
        for up in ups {
            if let j4n_proto::Up::Intent { intent: Intent::Answer { ask, reply } } = up {
                let id = match reply {
                    Reply::Yes => "yes".to_string(),
                    Reply::No => "no".to_string(),
                    Reply::Pick { id } => id.clone(),
                    Reply::Grant { scope } => scope.clone(),
                    Reply::Text { .. } => continue,
                };
                self.ui.sent.insert(ask.clone(), (id, self.clock.unwrap_or_else(Instant::now)));
                self.redraw = true;
            }
        }
    }
}
