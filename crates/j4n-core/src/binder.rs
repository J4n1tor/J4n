//! The open-ask binder: which open question a typed line answers (the open-ask law).
//!
//! A pure function of the line and the asks that are open: no model, no clock, no I/O. While any ask is open a typed line goes
//! here first. The result is one of
//!
//! - [`Bind::Answer`]: the line is an [`Answer`] to one ask, and it passes that ask's [`AskShape::check`];
//! - [`Bind::Which`]: two or more asks fit the line equally, so the operator is asked which one they are answering. Nothing is
//!   sent until they say; the binder never guesses and a `Which` is never re-routed;
//! - [`Bind::NotAnswer`]: the line answers nothing (a `:command`, a new request, an empty or unreadable line); the caller
//!   routes it as usual.
//!
//! What answers what (a line is trimmed; ASCII case and `’` do not matter; a line with a control character, longer than
//! [`TEXT_MAX`] characters, or starting with `:` is `NotAnswer`):
//!
//! | shape      | the line is an answer when                                                                           |
//! |------------|------------------------------------------------------------------------------------------------------|
//! | `YesNo`    | it is an assent (`yes`, `ok`, `go ahead`, `yes please do it` …) or a refusal (`no`, `not now` …)       |
//! | `Approval` | the same, and nothing else: no free text, no `always`, no `don't ask again` (grants are another order)  |
//! | `Choice`   | it is a position (`2`, `2.`, `(2)`, `2)`, `#3`, `option 2`, `the second one`, `last`), an option's id or label, or a word or two that name one option; else a plain reply |
//! | `Text`     | it is a plain reply                                                                                  |
//!
//! An assent or a refusal is a whole-word phrase from the lists below, optionally followed by polite filler (`please`,
//! `thanks`, `do it`); anything else after it (`yes, but only the tests`, `no, never ask again`) makes the line no answer. It
//! answers yes/no questions and approvals only, never a choice or a text. A position outside the options is no answer at all
//! (it is not turned into text). If a position and a name point at different options, or a name fits several, no answer.
//!
//! A plain reply is one line of at most twelve words with no `?`, not opening with a question word, `also`/`now`/`then`,
//! thanks or a greeting, not a one-word command, not marked `btw`/`new task`, not a clause (`the build is broken`), and
//! either one word, or opening like an answer (`the`, `my`, `about`, `use X`, `call it X`), or made of names, numbers, times
//! or paths. It must carry a letter or a digit. A line aimed at an ask by `focus` only has to be a plain line; for a choice it is any line
//! that is no option (a position outside them, `decide yourself`, `sub chat 1`): the card is up, so the line is the operator's own answer.
//!
//! Which ask: with a `focus` (a task id), the ask whose id or `asker` is the focus, the newest if several; the line either
//! answers that ask or is `NotAnswer`. A focus that matches no open ask is ignored. Without one, every open ask that takes the
//! line is a candidate. A position, a name or a yes/no word counts for more than a plain reply to a text question, which
//! counts for more than a plain reply to a choice; a plain reply that is plainly the kind a question wants (a number for
//! `how many`, a time for `when`, a path for `which file`) counts one more. A plain reply of the wrong kind for a question
//! is dropped when another ask also takes the line. One top candidate answers; several make a `Which`, oldest first.

use crate::ask::{Answer, AskShape, Opt, TEXT_MAX};
use std::collections::BTreeSet;

/// One open question as the binder sees it. `thread` is the chat thread the question was shown in; the binder does not use it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAsk {
    pub ask: String,
    pub asker: String,
    pub thread: String,
    pub question: String,
    pub shape: AskShape,
    pub opened_ms: u64,
}

/// One entry of the "which question are you answering?" menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Which {
    pub ask: String,
    /// The question on one line, at most 80 characters.
    pub question: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bind {
    Answer { ask: String, answer: Answer },
    /// Oldest first.
    Which(Vec<Which>),
    NotAnswer,
}

const ASSENT: &[&str] = &[
    "yes", "y", "yep", "yeah", "yup", "sure", "ok", "okay", "do it", "go ahead", "go for it", "please do", "sounds good",
    "that's fine", "thats fine", "fine", "proceed", "confirmed", "correct", "agreed", "yes please", "yes do it", "yes go ahead",
    "ok do it", "okay do it", "please", "of course", "go on then", "do so", "make it so", "that works", "works for me", "lgtm",
];
const REFUSAL: &[&str] = &[
    "no", "n", "nope", "nah", "don't", "dont", "do not", "not now", "not today", "not right now", "not yet", "stop", "cancel",
    "never mind", "nevermind", "no thanks", "no thank you", "skip it", "leave it", "don't do it", "dont do it", "no don't",
    "no dont", "hold off", "wait",
];
/// Words that may follow an assent or a refusal without changing it.
const FILLER: &[&str] = &[
    "please", "pls", "plz", "thanks", "thank", "you", "thx", "do", "it", "so", "go", "ahead", "now", "that", "this", "is", "fine",
    "works", "proceed", "of", "course", "for", "me", "i", "think", "very", "much", "right",
];
/// Words around the name of an option that are not part of the name.
const WRAP: &[&str] = &[
    "use", "using", "the", "one", "a", "an", "option", "please", "pls", "i", "think", "go", "with", "take", "pick", "choose", "that",
    "this", "it", "is", "fine", "just", "then", "let's", "lets", "ok", "okay", "sure", "prefer", "like", "want", "will", "id", "i'd",
    "i'll",
];
const PREFIXES: &[&str] = &[
    "option ", "number ", "no. ", "choice ", "go with ", "i'd go with ", "id go with ", "i would go with ", "i'll go with ",
    "let's go with ", "lets go with ", "go for ", "let's do ", "lets do ", "i pick ", "i choose ", "i'll take ", "ill take ",
    "pick ", "choose ", "take ", "do ", "the ",
];
const ORDINALS: &[&str] = &["first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth", "tenth"];
const CARDINALS: &[&str] = &["one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten"];
/// Words that may follow a number after a bare space and still leave it a pick (`2 please`); `3 weeks of logs` is a quantity.
const SPACE_OK: &[&str] = &[
    "please", "pls", "plz", "thanks", "thank", "thx", "yes", "yep", "yeah", "ok", "okay", "sure", "is", "it's", "its", "and", "then",
    "but", "for", "sounds", "looks", "fine", "works", "that", "this", "one", "option", "now", "go", "let's", "lets", "first", "i",
    "we", "do", "just", "only", "will", "would", "should", "can", "the", "a", "no", "nope",
];
const QUANTITY: &[&str] = &[
    "plus", "minus", "times", "x", "divided", "over", "percent", "%", "+", "-", "*", "/", "=", "files", "file", "lines", "line",
    "tests", "test", "minutes", "hours", "days", "seconds", "things", "of", "in", "items", "bugs", "errors", "more", "less", "kb",
    "mb", "gb", "ms", "words", "chars", "steps", "pages",
];
/// Openings of a line that is a question, a continuation, thanks or a greeting, not an answer.
const NOT_REPLY: &[&str] = &[
    "what", "what's", "whats", "which", "where", "where's", "who", "whom", "whose", "when", "how", "why", "also", "now", "then",
    "and", "so", "next", "thanks", "thank", "thx", "hi", "hello", "hey",
];
const COMMANDS: &[&str] = &[
    "commit", "push", "pull", "undo", "redo", "retry", "revert", "help", "deploy", "fix", "run", "lint", "rebase", "restart",
    "rollback", "abort", "quit", "exit", "continue", "resume", "status",
];
const UNRELATED: &[&str] = &["btw", "by the way", "new task", "unrelated", "off topic", "off-topic", "something else"];
/// How a reply of two words or more may open and still read as an answer.
const ANSWER_START: &[&str] = &[
    "the", "a", "an", "my", "our", "your", "his", "her", "their", "its", "it", "it's", "that", "that's", "this", "those", "these", "i",
    "i'd", "i'll", "maybe", "probably", "perhaps", "about", "around", "roughly", "in", "at", "on", "under", "near", "from", "to",
    "for", "with", "without", "no", "not", "none", "neither", "both", "either", "all", "any", "some", "every", "each", "same",
    "other", "another",
];
const PICKS: &[&str] = &["use", "using", "pick", "choose", "call it", "name it", "go with", "go for", "i'd go", "i think", "i guess"];
const ANCHOR: &[&str] = &[
    "one", "ones", "first", "second", "third", "fourth", "last", "other", "another", "same", "newer", "older", "shorter", "longer",
    "bigger", "smaller", "latest", "oldest", "newest",
];
const REQUESTS: &[&str] = &["i want", "i need", "i'd like", "id like", "you should", "you need", "you can", "do not", "dont do", "don't do"];
const SPELLED: &[&str] = &["one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "twenty", "hundred", "once", "twice"];
const TIME_WORDS: &[&str] = &[
    "today", "tomorrow", "tonight", "yesterday", "noon", "midnight", "monday", "tuesday", "wednesday", "thursday", "friday",
    "saturday", "sunday", "morning", "evening", "afternoon", "asap",
];
const HEDGE: &[&str] = &["about", "around", "roughly", "maybe", "approximately", "like", "just", "at", "by", "before", "until", "till"];

/// What kind of thing a reply is, and what a question asks for; used only to tell two waiting questions apart.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Number,
    Time,
    Path,
    Other,
}
#[derive(Clone, Copy, PartialEq)]
enum Wants {
    Quantity,
    Time,
    Place,
    Anything,
}

/// A way a line can answer one ask: `rank` 2 for a position, a name or a yes/no word, 1 for a plain reply to a text question,
/// 0 for a plain reply to a choice (its way out, not its shape).
struct Hit {
    answer: Answer,
    rank: u8,
    text: bool,
}

fn numeric(w: &str) -> bool {
    let d = w.chars().take_while(|c| c.is_ascii_digit() || ".,:".contains(*c)).count();
    let rest = &w[d..];
    (d > 0 && w.starts_with(|c: char| c.is_ascii_digit()) && rest.len() <= 3 && rest.chars().all(|c| c.is_ascii_alphabetic()))
        || SPELLED.contains(&w)
}

fn path_like(w: &str) -> bool {
    w.contains('/')
        || w.rsplit_once('.').is_some_and(|(s, e)| !s.is_empty() && (1..=5).contains(&e.len()) && e.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// The kind of a reply, read from its first content word: `cat notes.txt` is a command that names a path, not a path.
fn reply_kind(l: &str) -> Kind {
    let trim = |w: &str| w.trim_matches(|c: char| !c.is_alphanumeric() && !"/._-:".contains(c)).to_string();
    let ws: Vec<String> = l.split_whitespace().map(trim).filter(|w| !w.is_empty()).collect();
    let first = ws.get(ws.iter().take_while(|w| HEDGE.contains(&w.as_str())).count()).map_or("", String::as_str);
    if ws.len() <= 4 && numeric(first) {
        Kind::Number
    } else if ws.len() <= 3 && TIME_WORDS.contains(&first) {
        Kind::Time
    } else if path_like(first) {
        Kind::Path
    } else {
        Kind::Other
    }
}

fn wants(question: &str) -> Wants {
    let l = question.to_ascii_lowercase();
    let has = |ps: &[&str]| ps.iter().any(|p| l.contains(p));
    if has(&["how many", "how much", "how long", "how often", "how big", "what number", "what port", "how far", "how old"]) {
        Wants::Quantity
    } else if has(&["when ", "when's", "what time", "what date", "which day", "how soon"]) {
        Wants::Time
    } else if has(&["which file", "what file", "which path", "what path", "which folder", "which dir", "where "]) {
        Wants::Place
    } else {
        Wants::Anything
    }
}

/// Could a reply of this kind answer a question that wants that? An unclassifiable reply fits everything: a menu is cheaper than a wrong bind.
fn fits(r: Kind, w: Wants) -> bool {
    match r {
        Kind::Number => w != Wants::Place,
        Kind::Time => matches!(w, Wants::Time | Wants::Anything),
        Kind::Path => matches!(w, Wants::Place | Wants::Anything),
        Kind::Other => true,
    }
}

/// Whether the reply is plainly what the question wants (a number for `how many`).
fn is_wanted(r: Kind, w: Wants) -> bool {
    matches!((r, w), (Kind::Number, Wants::Quantity | Wants::Time) | (Kind::Time, Wants::Time) | (Kind::Path, Wants::Place))
}

/// The line when it is fit to be an answer at all: trimmed, one plain line of at most [`TEXT_MAX`] characters.
fn clean(line: &str) -> Option<&str> {
    let t = line.trim();
    let plain = |c: char| {
        !c.is_control() && !matches!(c, '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
    };
    (!t.is_empty() && !t.starts_with(':') && t.chars().nth(TEXT_MAX).is_none() && t.chars().all(plain)).then_some(t)
}

/// Whether a line is a plain yes (`Some(true)`) or a plain no (`Some(false)`), by the words the binder takes for them; any other line is `None`. The one list
/// of a yes for every question that waits for one.
pub fn yes_no(line: &str) -> Option<bool> {
    let l = line.trim().to_ascii_lowercase().replace('\u{2019}', "'");
    if phrased(&l, ASSENT) { Some(true) } else { phrased(&l, REFUSAL).then_some(false) }
}

/// Whether `l` is made of phrases of `list` (the first one opening the line) and filler.
fn phrased(l: &str, list: &[&str]) -> bool {
    let ws: Vec<&str> = l.split_whitespace().map(|w| w.trim_matches(|c: char| ".,!;:\"()".contains(c))).filter(|w| !w.is_empty()).collect();
    let phrases: Vec<Vec<&str>> = list.iter().map(|p| p.split(' ').collect()).collect();
    let own = |w: &str| phrases.iter().any(|p| p.len() == 1 && p[0] == w);
    let mut i = 0;
    while i < ws.len() {
        match phrases.iter().filter(|p| ws[i..].starts_with(p)).map(|p| p.len()).max() {
            Some(n) => i += n,
            None if i > 0 && (FILLER.contains(&ws[i]) || own(ws[i])) => i += 1,
            None => return false,
        }
    }
    !ws.is_empty()
}

fn words(s: &str) -> Vec<&str> {
    s.split(|c: char| !(c.is_alphanumeric() || "_-./'".contains(c))).map(|w| w.trim_matches(['.', '-', '\''])).filter(|w| !w.is_empty()).collect()
}

/// A position: `None` when the line is no position, `Some(None)` when it is one that no option has.
fn position(l: &str, len: usize) -> Option<Option<usize>> {
    let mut s = l.trim_start_matches(['(', '[', '#', ' ']);
    for p in PREFIXES {
        if let Some(r) = s.strip_prefix(p) {
            s = r.trim_start_matches(['#', ' ']);
        }
    }
    let digits = s.bytes().take_while(u8::is_ascii_digit).count();
    let (n, rest, word) = if digits > 0 {
        let r = &s[digits..];
        let suffix = ["st", "nd", "rd", "th"].iter().find_map(|x| r.strip_prefix(x).filter(|t| !t.starts_with(char::is_alphanumeric)));
        (s[..digits].parse::<usize>().ok().filter(|_| digits <= 2)?, suffix.unwrap_or(r), false)
    } else {
        let w = s.split(|c: char| !c.is_ascii_alphabetic()).next()?;
        let at = |t: &[&str]| t.iter().position(|x| *x == w).map(|i| i + 1);
        (if w == "last" { len } else { at(ORDINALS).or_else(|| at(CARDINALS))? }, &s[w.len()..], true)
    };
    let seps: &[char] = if word { &['.', ')', '!'] } else { &['.', ')', ']', ',', ':', '-', '!'] };
    let tail = rest.trim_start_matches(|c: char| c == ' ' || seps.contains(&c));
    let first = tail.split_whitespace().next().unwrap_or("");
    if !rest.is_empty() && !rest.starts_with(seps) && !(rest.starts_with(' ') && SPACE_OK.contains(&first)) {
        return None;
    }
    let quantity = tail.contains('?') || tail.split_whitespace().count() > 8 || tail.starts_with(|c: char| c.is_ascii_digit()) || QUANTITY.contains(&first);
    (!quantity).then(|| (1..=len).contains(&n).then(|| n - 1))
}

/// The option a line names: `Ok(None)` names none, `Err` names several. The id or the label whole, else one to three words all
/// of which are words of one option.
fn named(l: &str, opts: &[Opt]) -> Result<Option<usize>, ()> {
    let t = l.trim_end_matches(['.', '!', ',', ';']);
    let content: Vec<&str> = words(t).into_iter().filter(|w| !WRAP.contains(w)).collect();
    type Tier<'a> = &'a dyn Fn(&str, &str) -> bool;
    let tiers: [Tier; 3] = [
        &|id, _| id == t,
        &|_, label| label == t,
        &|id, label| (1..=3).contains(&content.len()) && content.iter().all(|c| words(label).contains(c) || words(id).contains(c)),
    ];
    let lower: Vec<(String, String)> = opts.iter().map(|o| (o.id.to_ascii_lowercase(), o.label.to_ascii_lowercase())).collect();
    for tier in tiers {
        let hits: Vec<usize> = (0..opts.len()).filter(|&i| tier(&lower[i].0, &lower[i].1)).collect();
        match hits[..] {
            [] => {}
            [i] => return Ok(Some(i)),
            _ => return Err(()),
        }
    }
    Ok(None)
}

/// A line that reads like an answer to a question rather than a new request; a `focused` line only has to be a plain line.
fn plain_reply(l: &str, focused: bool) -> bool {
    if !l.chars().any(char::is_alphanumeric) || phrased(l, ASSENT) || phrased(l, REFUSAL) {
        return false;
    }
    let ws: Vec<&str> = l.split_whitespace().collect();
    let word = |w: &str| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_string();
    let first = word(ws[0]);
    let two = ws.get(1).map(|s| format!("{first} {s}")).unwrap_or_default();
    if focused {
        return true;
    }
    if l.contains('?') || ws.len() > 12 || NOT_REPLY.contains(&first.as_str()) || REQUESTS.contains(&two.as_str()) {
        return false;
    }
    if UNRELATED.iter().any(|u| l.contains(u)) || (l.starts_with('/') && !l[1..].contains(['/', '.'])) {
        return false;
    }
    if ws.len() == 1 {
        return !COMMANDS.contains(&first.as_str());
    }
    
    let names = ws.iter().all(|w| word(w).chars().any(|c| "0123456789_-./".contains(c)));
    let picks = PICKS.iter().any(|p| l.strip_prefix(p).is_some_and(|r| r.starts_with(' ')));
    let kinded = ws.len() <= 3 && reply_kind(l) != Kind::Other;
    let anchored = ws.iter().any(|w| ANCHOR.contains(&word(w).as_str()));
    names || picks || kinded || (ANSWER_START.contains(&first.as_str()) && (anchored || ws.len() <= 3))
}

/// How `raw` (and `l`, its lowercase) answers `ask`, if it does.
fn hit(ask: &OpenAsk, raw: &str, l: &str, focused: bool) -> Option<Hit> {
    let structural = |answer| Some(Hit { answer, rank: 2, text: false });
    let text = |rank| plain_reply(l, focused).then(|| Hit { answer: Answer::Text { text: raw.to_string() }, rank, text: true });
    // the card is up: any line that is not a yes or a no is the operator's own answer to a choice
    let own = || (!phrased(l, ASSENT) && !phrased(l, REFUSAL)).then(|| Hit { answer: Answer::Text { text: raw.to_string() }, rank: 0, text: true });
    let found = match &ask.shape {
        AskShape::YesNo | AskShape::Approval { .. } => yes_no(l).and_then(|y| structural(if y { Answer::Yes } else { Answer::No })),
        AskShape::Text => text(1),
        AskShape::Choice { options } => {
            let pick = |i: usize| structural(Answer::Pick { id: options[i].id.clone() });
            match (position(l, options.len()), named(l, options)) {
                (Some(Some(i)), Ok(None)) => pick(i),
                (Some(Some(i)), Ok(Some(j))) if i == j => pick(i),
                (Some(None) | None, Ok(Some(j))) => pick(j),
                // the card of the question is up and the line is for it: a line that is no option is the operator's own answer
                (Some(_), _) | (_, Err(())) if focused => own(),
                (Some(_), _) | (_, Err(())) => None,
                (None, Ok(None)) if focused => own(),
                (None, Ok(None)) => text(0),
            }
        }
    };
    found.filter(|h| ask.shape.check(&h.answer).is_ok())
}

fn menu(ask: &OpenAsk) -> Which {
    let words: Vec<&str> = ask.question.split(|c: char| c.is_control() || c.is_whitespace()).filter(|w| !w.is_empty()).collect();
    Which { ask: ask.ask.clone(), question: words.join(" ").chars().take(80).collect() }
}

/// Which open ask `line` answers, and how. `focus` is the task the operator is looking at (see the module doc).
pub fn bind(line: &str, open: &[OpenAsk], focus: Option<&str>) -> Bind {
    let Some(raw) = clean(line) else { return Bind::NotAnswer };
    let l = raw.to_ascii_lowercase().replace('\u{2019}', "'");
    let mut seen = BTreeSet::new();
    let open: Vec<&OpenAsk> = open.iter().filter(|a| seen.insert(a.ask.as_str())).collect();
    let answer = |a: &OpenAsk, h: Hit| Bind::Answer { ask: a.ask.clone(), answer: h.answer };
    let focused = focus.filter(|f| !f.is_empty()).and_then(|f| open.iter().copied().filter(|a| a.ask == f || a.asker == f).max_by_key(|a| a.opened_ms));
    if let Some(a) = focused {
        return hit(a, raw, &l, true).map_or(Bind::NotAnswer, |h| answer(a, h));
    }
    let mut hits: Vec<(&OpenAsk, Hit)> = open.iter().filter_map(|a| hit(a, raw, &l, false).map(|h| (*a, h))).collect();
    if hits.len() > 1 {
        let kind = reply_kind(&l);
        hits.retain(|(a, h)| !h.text || fits(kind, wants(&a.question)));
        for (a, h) in &mut hits {
            h.rank += u8::from(h.text && is_wanted(kind, wants(&a.question)));
        }
    }
    let top = hits.iter().map(|(_, h)| h.rank).max().unwrap_or(0);
    hits.retain(|(_, h)| h.rank == top);
    hits.sort_by_key(|(a, _)| a.opened_ms);
    match hits.len() {
        0 => Bind::NotAnswer,
        1 => hits.pop().map_or(Bind::NotAnswer, |(a, h)| answer(a, h)),
        _ => Bind::Which(hits.iter().map(|(a, _)| menu(a)).collect()),
    }
}
