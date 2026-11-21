//! The chat and the log as the TUI is shown them: events of the Log turned into the lines of a stream (`j4n-proto`'s
//! `Item`). One function does both the live path and the replay after a restart, so the chat of a thread that is read
//! back from the Log is the chat that was on the screen (the streamed words that were never written are the one
//! difference: the settled message stands where they were).

pub(super) use crate::text::line as clip;
use crate::change::Actor;
use crate::log::{Ev, actor_word};
use crate::task::Why;
use j4n_proto::{Item, Line, Role, Tone};
use serde_json::Value as Json;
use std::collections::HashMap;

/// The lines of a shell command's output that are shown.
const OUT_LINES: usize = 8;
/// The longest line of a tool or a shell output that is shown.
const LINE_MAX: usize = 200;

/// What a place held, in words: the counts, and the first few names, the ones git ignores among them.
fn work_words(work: Option<&Json>) -> String {
    let Some(w) = work.and_then(|w| serde_json::from_value::<crate::place::Work>(w.clone()).ok()) else { return "what it changed".into() };
    let names: Vec<&str> = w.files.iter().chain(&w.ignored).map(String::as_str).collect();
    let shown = names.iter().take(5).copied().collect::<Vec<_>>().join(", ");
    let more = if names.len() > 5 { format!(", +{} more", names.len() - 5) } else { String::new() };
    format!("{} ({shown}{more})", w.counts())
}

fn text(ev: &Ev, key: &str) -> String {
    ev.data.get(key).and_then(Json::as_str).unwrap_or_default().to_string()
}

fn line(ev: &Ev, role: Role, tone: Tone, body: String) -> Line {
    Line { key: format!("s{}", ev.seq), role, text: body, task: ev.task.clone(), partial: false, tone }
}

/// The last lines of an output, each cut short.
fn tail(out: &str) -> String {
    let lines: Vec<&str> = out.lines().collect();
    let from = lines.len().saturating_sub(OUT_LINES);
    let shown: Vec<String> = lines[from..].iter().map(|l| clip(l, LINE_MAX)).collect();
    if from > 0 { format!("… {from} lines above\n{}", shown.join("\n")) } else { shown.join("\n") }
}

/// The line an event is, when it is one that stands as words (a notice, an error, a stop): what the rows of the chat say of it.
pub(super) fn settled_line(ev: &Ev) -> Option<Line> {
    Chat::settled(ev)
}

/// The chat of one thread, as events come. It remembers the line each task is still streaming.
#[derive(Default)]
pub struct Chat {
    open: HashMap<String, String>,
    made: u64,
}

impl Chat {
    pub fn new() -> Chat {
        Chat::default()
    }

    /// What an event changes in the chat, in order; nothing for an event the chat does not show.
    pub fn items(&mut self, ev: &Ev) -> Vec<Item> {
        let task = ev.task.clone().unwrap_or_default();
        match ev.kind.as_str() {
            "run.delta" => {
                let t = text(ev, "text");
                if t.is_empty() {
                    return Vec::new();
                }
                if let Some(key) = self.open.get(&task) {
                    return vec![Item::Append { key: key.clone(), text: t }];
                }
                self.made += 1;
                let key = format!("p{}.{}", if task.is_empty() { "-" } else { &task }, self.made);
                self.open.insert(task, key.clone());
                vec![Item::Put { line: Line { key, role: Role::Doer, text: t, task: ev.task.clone(), partial: true, tone: Tone::Plain } }]
            }
            "run.message" => {
                let mut l = line(ev, Role::Doer, Tone::Plain, text(ev, "text"));
                if let Some(key) = self.open.remove(&task) {
                    l.key = key;
                }
                vec![Item::Put { line: l }]
            }
            _ => Self::settled(ev).into_iter().map(|line| Item::Put { line }).collect(),
        }
    }

    fn settled(ev: &Ev) -> Option<Line> {
        let d = &ev.data;
        let (role, tone, body) = match ev.kind.as_str() {
            "chat.say" => (Role::You, Tone::Plain, text(ev, "text")),
            "chat.note" => {
                let tone = match d.get("tone").and_then(Json::as_str) {
                    Some("bad") => Tone::Bad,
                    Some("warn") => Tone::Warn,
                    Some("good") => Tone::Good,
                    _ => Tone::Dim,
                };
                (Role::Notice, tone, text(ev, "text"))
            }
            // a view as plain words, for a stream that cannot draw it; the screen draws the row of `rows.rs` instead
            "chat.view" => (Role::Notice, Tone::Dim, text(ev, "text")),
            "run.reasoning" => (Role::Reasoning, Tone::Dim, text(ev, "text")),
            "run.tool_call" => (Role::Tool, Tone::Plain, clip(&format!("{} {}", text(ev, "name"), text(ev, "detail")), LINE_MAX)),
            "run.tool_result" => (Role::Tool, Tone::Dim, clip(&format!("{} → {}", text(ev, "name"), text(ev, "detail")), LINE_MAX)),
            "run.shell" => {
                let exit = d.get("exit").and_then(Json::as_i64).unwrap_or(0);
                let mut body = format!("$ {}", clip(&text(ev, "cmd"), LINE_MAX));
                let out = tail(&text(ev, "out"));
                if !out.is_empty() {
                    body = format!("{body}\n{out}");
                }
                if exit != 0 {
                    body = format!("{body}\nexit {exit}");
                }
                (Role::Shell, if exit == 0 { Tone::Plain } else { Tone::Bad }, body)
            }
            "run.notice" => (Role::Notice, Tone::Dim, text(ev, "text")),
            "run.error" => (Role::Error, Tone::Bad, text(ev, "text")),
            "run.changed" => {
                let paths: Vec<&str> = d.get("paths").and_then(Json::as_array).map(|a| a.iter().filter_map(Json::as_str).collect()).unwrap_or_default();
                (Role::Notice, Tone::Dim, format!("changed {}", paths.join(", ")))
            }
            "task.stopped" => {
                let why = d.get("why").and_then(|w| serde_json::from_value::<Why>(w.clone()).ok());
                let (tone, body) = match &why {
                    Some(Why::Aborted) => (Tone::Dim, "stopped".to_string()),
                    Some(Why::Wall) => (Tone::Warn, "stopped: the time it was given ran out".to_string()),
                    Some(Why::Capped) => (Tone::Warn, "stopped: it spent past its soft cap — carry on to let it go further".to_string()),
                    Some(Why::HardCap) => (Tone::Bad, "stopped: it spent past the hard cap".to_string()),
                    Some(Why::Failed { message }) => (Tone::Bad, format!("failed: {message}")),
                    Some(other) => (Tone::Warn, format!("stopped: {}", other.word())),
                    None => (Tone::Warn, "stopped".to_string()),
                };
                (Role::Notice, tone, body)
            }
            "task.settled" if d.get("ok").and_then(Json::as_bool) == Some(false) => {
                let reason = text(ev, "reason");
                if reason.trim().is_empty() {
                    return None;
                }
                (Role::Error, Tone::Bad, reason)
            }
            "task.cap_reached" => (Role::Notice, Tone::Warn, "spend passed the soft cap".to_string()),
            "task.cooling" => {
                let wait = d.get("ms").and_then(Json::as_u64).map(crate::fmt::duration).unwrap_or_default();
                (Role::Notice, Tone::Warn, format!("a limit was hit: it goes on by itself in {wait}"))
            }
            "task.compacting" if d.get("only").and_then(Json::as_bool) == Some(true) => (Role::Notice, Tone::Dim, "shrinking the session".to_string()),
            "task.compacting" => (Role::Notice, Tone::Dim, "shrinking the session before it goes on".to_string()),
            "task.compacted" => {
                let note = d.get("note").and_then(Json::as_str).map(|n| format!(" ({n})")).unwrap_or_default();
                // a `:compact` says the sizes: what it held, what it holds, what was cut
                let held = d.get("held").and_then(Json::as_u64);
                let sizes = match (held, d.get("held_after").and_then(Json::as_u64)) {
                    (Some(h), Some(a)) => format!(": {} → {} (cut {})", crate::task::context::k(h), crate::task::context::k(a), crate::task::context::k(h.saturating_sub(a))),
                    (Some(h), None) => format!(": {} held before; what it holds now shows with the next line", crate::task::context::k(h)),
                    _ => String::new(),
                };
                match d.get("ok").and_then(Json::as_bool) {
                    Some(true) => (Role::Notice, Tone::Dim, format!("the session was shrunk{sizes}{note}")),
                    _ => (Role::Notice, Tone::Warn, "the session was not shrunk".to_string()),
                }
            }
            "task.kept" => {
                let (dir, why) = (text(ev, "dir"), text(ev, "why"));
                (Role::Notice, Tone::Warn, format!("kept in {dir}, not in your folder: {} — {why}", work_words(d.get("work"))))
            }
            "task.applied" => {
                let note = d.get("notes").and_then(Json::as_array).map(|n| n.iter().filter_map(Json::as_str).map(|n| format!(" ({n})")).collect::<String>()).unwrap_or_default();
                (Role::Notice, Tone::Accent, format!("laid into your folder, uncommitted: {}{note}", work_words(d.get("work"))))
            }
            "task.queued" if matches!(ev.by, Actor::Rule(_)) => {
                let rule = actor_word(&ev.by);
                let goal = d.pointer("/spec/goal").and_then(Json::as_str).unwrap_or_default();
                (Role::Notice, Tone::Accent, format!("carried on by {rule}: {}", clip(goal, LINE_MAX)))
            }
            "rule.dropped" => (Role::Notice, Tone::Warn, format!("rule {} did not act: {}", text(ev, "rule"), text(ev, "why"))),
            "ask.answered" => {
                let by = if d.get("via").and_then(Json::as_str) == Some("human") { "you".to_string() } else { text(ev, "by") };
                let why = text(ev, "why");
                let tail = if why.is_empty() { String::new() } else { format!(" — {why}") };
                (Role::Notice, Tone::Dim, format!("{}: {}{}", by, text(ev, "answer"), tail))
            }
            "gate.verdict" => {
                let block = text(ev, "verdict") == "block";
                (Role::Notice, if block { Tone::Bad } else { Tone::Dim }, format!("{} {} {}", text(ev, "verdict"), text(ev, "verb"), clip(&text(ev, "target"), LINE_MAX)))
            }
            "thread.bound" => {
                let model = d.get("model").and_then(Json::as_str).map(|m| format!(" · {m}")).unwrap_or_default();
                let host = d.get("endpoint").and_then(Json::as_str).map(|h| format!(" on {h}")).unwrap_or_default();
                (Role::Notice, Tone::Accent, format!("bound to {}{model}{host} ({})", text(ev, "doer"), text(ev, "how")))
            }
            "answerer.off" => (Role::Notice, Tone::Warn, format!("{} failed {} times in a row: it is not asked again for a day", text(ev, "answerer"), d.get("failures").and_then(Json::as_u64).unwrap_or(0))),
            "probe.result" if d.get("passed").and_then(Json::as_bool) == Some(false) => {
                let (a, p) = (text(ev, "answerer"), text(ev, "point"));
                let why = d.get("why").and_then(Json::as_str).map(str::to_string).unwrap_or_else(|| format!("wrong on {}", d.get("failed").and_then(Json::as_array).map_or(0, Vec::len)));
                (Role::Notice, Tone::Warn, format!("{a} did not pass its cases for {p} ({why}): it does not answer there until it does"))
            }
            "thread.fresh" => (Role::Notice, Tone::Accent, "fresh: the next line chooses again".to_string()),
            _ => return None,
        };
        Some(line(ev, role, tone, body))
    }
}
