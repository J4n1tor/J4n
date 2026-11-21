//! The seam of prompt shaping. The front knows what only it knows — whether a line answers a
//! question, is a command, is a short reply to the doer the thread holds, or goes to a doer that never saw the turns it follows —
//! and the desk, which the pack `shape` brings, knows the rest: the switch, the result for a draft, the card, and the notes the
//! operator kept. Without a desk, a draft is ignored and a line is what was typed.

use j4n_proto::Row;


/// The longest line that is shaped; a paste is more than the operator wrote.
pub const LINE_MAX: usize = 4_000;

/// A draft of the composer, as the desk is told of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub thread: String,
    pub text: String,
    /// The doer the line is for (the thread's, or the one named).
    pub target: String,
    /// The same text for the same target gives the same hash: a result belongs to it.
    pub hash: String,
    /// The line goes to a doer that never received the turns it follows (`HANDOFF`).
    pub handoff: bool,
}

/// What the pack `shape` brings to the front.
pub trait Desk: Send + Sync {
    /// The switch, as it stands now.
    fn on(&self) -> bool;
    /// A draft was seen; `skip` is why it will not be shaped, or `None` when it will.
    fn drafted(&self, draft: &Draft, skip: Option<&str>);
    /// `:shape on|off|drop|toggle <key>|pick <n>`: what to say back.
    fn command(&self, thread: &str, args: &[String]) -> Result<String, String>;

    /// Take the settings as an edit left them: the numbers it keeps of them; by default it keeps none.
    fn tune(&self, _settings: &crate::setting::Settings) {}
    /// The text the Brief gets for the rows of the latest result for this line that the operator kept; an error says why they
    /// were not taken (the line then goes as typed).
    fn keep(&self, thread: &str, line: &str, keys: &[String]) -> Result<String, String>;
    /// The keys the card holds now, for a bare Enter that sends what is kept.
    fn card(&self, thread: &str) -> Vec<Row>;
    /// The most a draft may be shaped by `words`: a line that is only a short reply is matched against these.
    fn skips(&self, line: &str) -> bool;
}

/// A short, stable name for a draft: FNV-1a over the target, a separator and the text.
pub fn hash_of(target: &str, text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in target.bytes().chain(std::iter::once(0)).chain(text.bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// What the front knows of a line that decides whether it is shaped.
#[derive(Debug, Clone, Copy, Default)]
pub struct Facts {
    /// It would be taken as the answer to a question that is open.
    pub answers: bool,
    /// It is a command, a setting or a flow (`:fresh`, `/plan …`).
    pub command: bool,
    /// It is a short reply to a doer that holds the thread ("continue", "yes", "do it").
    pub short_reply: bool,
    /// The doer it goes to never received the turns it follows.
    pub handoff: bool,
}

/// Why a line is not shaped, decided without a model (§2.3, steps 2 to 6), or `None`. A short reply is not skipped when it is a
/// handoff: "it" means nothing to a doer that was not there.
pub fn skip_reason(text: &str, f: Facts) -> Option<&'static str> {
    if f.answers {
        return Some("an answer to a question");
    }
    if f.command {
        return Some("a command");
    }
    if text.chars().count() > LINE_MAX {
        return Some("a paste");
    }
    if text.starts_with('!') || text.starts_with('$') {
        return Some("a shell line");
    }
    if f.short_reply && !f.handoff {
        return Some("a short reply to the doer that has the thread");
    }
    None
}

impl super::Front {
    /// Shape a line now and wait for it, for a run with nobody at a composer (`j4n run --shape`):
    /// the keys of the rows that held (a question is dropped: nobody is there to answer it) and the report for stderr.
    pub fn shape_run(&self, thread: &str, text: &str) -> (Vec<String>, String) {
        let Some(desk) = self.desk().filter(|d| d.on()) else { return (Vec::new(), "shape: not run (the pack `shape` is not on)".into()) };
        self.draft(thread, text, "");
        let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let rows = loop {
            let rows = desk.card(thread);
            if rows.first().is_none_or(|r| r.cells.get(1).is_some_and(|c| c.text != "working")) || std::time::Instant::now() >= end {
                break rows;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        let cell = |r: &Row, n: usize| r.cells.get(n).map(|c| c.text.clone()).unwrap_or_default();
        let Some(head) = rows.first() else { return (Vec::new(), "shape: not run (nothing came back)".into()) };
        let (state, detail) = (cell(head, 1), cell(head, 2));
        match state.as_str() {
            "pass" => return (Vec::new(), "shape: pass".into()),
            "ready" => {}
            "working" => return (Vec::new(), "shape: not run (the shaper did not answer in time)".into()),
            _ => return (Vec::new(), format!("shape: not run ({detail})")),
        }
        let items: Vec<&Row> = rows.iter().skip(1).filter(|r| !r.key.starts_with('~')).collect();
        let kept: Vec<&&Row> = items.iter().filter(|r| r.tone == j4n_proto::Tone::Good && r.key != "question").collect();
        let dropped = items.iter().filter(|r| r.key.starts_with("x:") || r.key == "question").count();
        let mut report = vec![format!("shape: add · kept {} · dropped {dropped} · {detail}", kept.len())];
        for r in &items {
            let (kind, words, why) = (cell(r, 0), cell(r, 1), cell(r, 2));
            report.push(match r.key.as_str() {
                "question" => format!("  - question  {words}: nobody is there to answer it"),
                k if k.starts_with("x:") => format!("  - {words}: {why}"),
                _ => format!("  + {kind:<5} {words} ({why})"),
            });
        }
        (kept.iter().map(|r| r.key.clone()).collect(), report.join("\n"))
    }
}
