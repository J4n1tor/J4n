//! The merge of the answers to one line: in arrival order, a boilerplate opener is cut, a sentence already
//! shown is not shown again, each answer is clipped to two sentences, and answers that each say something of their own are kept side by
//! side and marked: J4n does not read meaning, so it says they differ and decides nothing.

use j4n_core::text::clip;

/// Words that open an answer and say nothing, when a `,` `!` `.` `:` or a dash follows them.
const OPENERS: [&str; 15] = ["sure", "certainly", "of course", "okay", "ok", "great", "absolutely", "alright", "all right", "got it", "perfect", "done", "no problem", "happy to help", "yes"];
/// Whole first sentences that say nothing.
const WHOLE: [&str; 8] = ["i've done that", "i have done that", "i've done it", "here you go", "here is what i did", "here's what i did", "i've finished", "all done"];
/// The most of a sentence that is shown before it is clipped.
const SENTENCE_MAX: usize = 240;
pub const SHOWN: usize = 2;

/// How an answer stands among the others of its line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mark {
    /// The only answer with words of its own.
    One,
    /// One of two or more that each say something of their own: shown side by side.
    Differs,
    /// Everything it said was said by an earlier answer (named).
    Same(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub task: String,
    /// Up to two of its own sentences.
    pub text: String,
    /// All of its own sentences.
    pub full: String,
    pub more: bool,
    pub mark: Mark,
}

/// The answer with a boilerplate opener cut, as often as one opens it.
pub fn trim_opener(text: &str) -> &str {
    let mut s = text.trim_start();
    'again: loop {
        for (o, whole) in OPENERS.iter().map(|o| (*o, false)).chain(WHOLE.iter().map(|w| (*w, true))) {
            let Some(head) = s.get(..o.len()) else { continue };
            let ends = s[o.len()..].chars().next().map_or(whole, |c| matches!(c, ',' | '!' | '.' | ':' | '—' | '-'));
            if head.eq_ignore_ascii_case(o) && ends {
                s = s[o.len()..].trim_start_matches([',', '!', '.', ':', '—', '-']).trim_start();
                continue 'again;
            }
        }
        return s;
    }
}

/// The sentences of a text: a `.` `!` or `?` followed by a space ends one, and so does a new line.
pub fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\n' {
            out.push(std::mem::take(&mut cur));
            continue;
        }
        cur.push(c);
        if matches!(c, '.' | '!' | '?') && chars.peek().is_none_or(|n| n.is_whitespace()) {
            out.push(std::mem::take(&mut cur));
        }
    }
    out.push(cur);
    out.into_iter().map(|s| s.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|s| !s.is_empty()).collect()
}

/// What makes two sentences the same: the words, in lower case, without the mark that ends them.
fn key(s: &str) -> String {
    s.to_lowercase().trim_end_matches(['.', '!', '?']).trim().to_string()
}

/// The answers to one line, `(task, text)` in arrival order, merged.
pub fn merge(answers: &[(String, String)]) -> Vec<Shown> {
    let mut seen: Vec<(String, String)> = Vec::new();
    let mut out: Vec<Shown> = Vec::new();
    for (task, text) in answers {
        let mut own: Vec<String> = Vec::new();
        let mut first = None;
        // an answer that is only an opener is shown as it is: it is all there is
        let said = Some(sentences(trim_opener(text))).filter(|s| !s.is_empty()).unwrap_or_else(|| sentences(text));
        for s in said {
            let k = key(&s);
            if let Some((_, by)) = seen.iter().find(|(seen, _)| *seen == k) {
                first.get_or_insert_with(|| by.clone());
            } else if !k.is_empty() {
                seen.push((k, task.clone()));
                own.push(s);
            }
        }
        let mark = match (own.is_empty(), first) {
            (true, Some(by)) => Mark::Same(by),
            _ => Mark::One,
        };
        if own.is_empty() && mark == Mark::One {
            continue;
        }
        let text = own.iter().take(SHOWN).map(|s| clip(s, SENTENCE_MAX)).collect::<Vec<_>>().join(" ");
        out.push(Shown { task: task.clone(), text, full: own.join(" "), more: own.len() > SHOWN, mark });
    }
    if out.iter().filter(|s| s.mark == Mark::One).count() > 1 {
        for s in out.iter_mut().filter(|s| s.mark == Mark::One) {
            s.mark = Mark::Differs;
        }
    }
    out
}
