//! Brief: the View that is read by a model. A Brief is a fixed order of sections, each with the class of
//! the data it carries (so the egress Gate can check a prompt as it checks a question), cut to a token budget with a reserve for the
//! answer — and the same bytes for the same inputs, whichever machine it is made on.
//!
//! What the Brief is **not**: the thread. A doer that keeps its own session keeps its own history, and a model J4n runs keeps the
//! transcript; the Brief rides with each request, is never stored as history, and carries what changes from
//! turn to turn — the contract for this line, and what a flow wants said.
//!
//! The contract is where the answer is sized to the line. "This is a short conversational line, answer in one sentence" is for the first line of a
//! thread. "continue", "yes", "2", "go on", "do it" in a going thread are lines to the doer that holds the work, they get the
//! contract of work, and they select no card.

use crate::schema::DataClass;

/// The share of the window kept free for the answer (`router-policy/context.rs`).
pub const OUTPUT_RESERVE: f64 = 0.15;

/// The contract of a line that is work.
pub const WORK: &str = "Do what the task says in this folder, with the tools you have. Say what you changed and what you could not check. A reply of a line or two is right when that is all there is to say.";
/// The contract of the first line of a thread when it is small talk.
pub const LEAN: &str = "This is a short conversational line. Answer in one sentence, and do not look at the project.";
/// Added when the task only reads (a plan, a review): the tools stay, and the Gate refuses a change.
pub const READ_ONLY: &str = "This task only reads: say what you would change. A change is refused.";

/// Added to the answer turn's contract when `brief.plain_answer` is on: the one sentence that asks for words. Nothing about
/// how the answer is formatted is ever sent: the reader draws it.
pub const PLAIN: &str = "Say what a thing is in words; when you cite an id from the project, add a few words saying what it is.";

/// Words that make a short line a task and not small talk.
const TASK_WORDS: [&str; 23] = [
    "fix", "add", "write", "make", "build", "create", "implement", "run", "test", "refactor", "remove", "delete", "update", "change", "rename", "install", "deploy", "review", "explain", "show", "list", "find", "check",
];

/// An estimate of the tokens in a text: a token is about four characters.
pub fn tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

/// A goal too short to be a task (`hi`, `thanks`, `ping`): three words or fewer, no question, none of them a task word.
pub fn is_short(line: &str) -> bool {
    let words: Vec<String> = line.split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()).filter(|w| !w.is_empty()).collect();
    !words.is_empty() && words.len() <= 3 && !line.contains('?') && !words.iter().any(|w| TASK_WORDS.contains(&w.as_str()))
}

/// The contract for a line. Small talk is only the first line of a thread: in a going one every line is work.
pub fn contract(line: &str, in_session: bool, read_only: bool) -> String {
    let lean = !in_session && is_short(line);
    let mut text = if lean { LEAN.to_string() } else { WORK.to_string() };
    if read_only {
        text.push(' ');
        text.push_str(READ_ONLY);
    }
    text
}

/// Whether a line asks about J4n's chats: it says `chat` or `chats` (`sub chats`, `other chats`) or the id of one (`c3`).
pub fn about_chats(line: &str) -> bool {
    said(line).iter().any(|w| w.starts_with("chat") || w == "subchat" || (w.len() > 1 && w.starts_with('c') && w[1..].chars().all(|c| c.is_ascii_digit())))
}

/// A card offered to a turn (a skill, [`crate::skill::Skill::card`]; or a help card): picked when the line says one of its words. A short line in a going thread
/// picks none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub id: String,
    pub words: Vec<String>,
    pub text: String,
}

/// The words of a line, lowercase, with what is not a letter or a digit trimmed off each.
pub fn said(line: &str) -> Vec<String> {
    line.split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()).filter(|w| !w.is_empty()).collect()
}

/// The cards the line selects.
pub fn cards<'a>(line: &str, in_session: bool, offered: &'a [Card]) -> Vec<&'a Card> {
    if in_session && is_short(line) {
        return Vec::new();
    }
    let said = said(line);
    offered.iter().filter(|c| c.words.iter().any(|w| said.contains(&w.to_lowercase()))).collect()
}

/// The sections, in the order they are written, and which of them may be cut when the budget is short (last first).
const ORDER: [(&str, bool); 17] = [("contract", false), ("seat", false), ("chats", false), ("carry", false), ("inbox", false), ("messages", false), ("parts", false), ("project", true), ("remembered", true), ("concerns", true), ("plan", true), ("branches", true), ("digests", true), ("skills", true), ("changed", true), ("notes", true), ("shape", true)];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub name: &'static str,
    pub class: DataClass,
    pub text: String,
}

/// What a Brief came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub text: String,
    pub tokens: usize,
    /// The sections that are in it, in order.
    pub sections: Vec<&'static str>,
    /// The sections that did not fit.
    pub cut: Vec<&'static str>,
    /// The most sensitive class of what it carries.
    pub class: DataClass,
}

fn rank(c: DataClass) -> u8 {
    match c {
        DataClass::Public => 0,
        DataClass::Project => 1,
        DataClass::Private => 2,
        DataClass::Secret => 3,
    }
}

#[derive(Debug, Clone, Default)]
pub struct Brief(Vec<Section>);

impl Brief {
    pub fn new() -> Brief {
        Brief::default()
    }

    /// Add a section; a name outside the fixed order, or no text, adds nothing.
    pub fn with(mut self, name: &'static str, class: DataClass, text: &str) -> Brief {
        if ORDER.iter().any(|(n, _)| *n == name) && !text.trim().is_empty() {
            self.0.retain(|s| s.name != name);
            self.0.push(Section { name, class, text: text.trim().to_string() });
        }
        self
    }

    /// The Brief as text for a model whose window is `window` tokens: the sections in their order, without those that would take
    /// more than the window less its reserve for the answer. The contract is never cut; a section is cut whole, the last first.
    pub fn render(&self, window: usize) -> Rendered {
        let budget = ((window as f64) * (1.0 - OUTPUT_RESERVE)) as usize;
        let mut kept: Vec<&Section> = ORDER.iter().filter_map(|(n, _)| self.0.iter().find(|s| s.name == *n)).collect();
        let size = |v: &[&Section]| v.iter().map(|s| tokens(&s.text) + tokens(s.name) + 2).sum::<usize>();
        let mut cut = Vec::new();
        while size(&kept) > budget {
            // the last section that may be cut
            let Some(at) = kept.iter().rposition(|s| ORDER.iter().any(|(n, may)| *n == s.name && *may)) else { break };
            cut.push(kept.remove(at).name);
        }
        let text = kept.iter().map(|s| format!("## {}\n{}", s.name, s.text)).collect::<Vec<_>>().join("\n\n");
        let class = kept.iter().map(|s| s.class).max_by_key(|c| rank(*c)).unwrap_or(DataClass::Public);
        Rendered { tokens: tokens(&text), sections: kept.iter().map(|s| s.name).collect(), text, cut, class }
    }
}
