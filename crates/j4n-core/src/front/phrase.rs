//! The phrase table: the words a line can open with to be a command (the kind `phrase`, `packs/base/phrases/`), and the
//! rule `rule:intake` that answers the question "what is this line?" from it. A line the table does not know is not
//! answered here; the chain goes on (a decider, then the point's fallback, `task`).
//!
//! A phrase may also list `likes`: wordings in plain language (`show me the usage`) that stand for its command, tried before any model is asked.
//! A line is taken for one when its words, with the filler words (`show me the …`, a row of the table with `filler`) left out, are the
//! words of a wording, in any order, each as typed or one typo away (a letter missing, added, changed or two swapped). A line with a word more
//! or fewer is not taken, and one that two commands share at the same distance is left to Jev: the table never guesses.

use crate::answer::{Answerer, fn_rule, picked};
use crate::table::{Kind, Problem, Tables};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::collections::btree_map::Entry;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Phrase {
    pub id: String,
    #[serde(default)]
    pub doc: String,
    #[serde(default)]
    pub words: Vec<String>,
    /// Wordings in plain language that stand for the command: `show me the usage`.
    #[serde(default)]
    pub likes: Vec<String>,
    /// What follows the command's own name, as arguments: `args = ["chart:spend"]` for `chart`.
    #[serde(default)]
    pub args: Vec<String>,
    /// Words a line may have besides a wording's (`show`, `me`, `please`), for every wording; a row with only `filler` adds no command.
    #[serde(default)]
    pub filler: Vec<String>,
    /// What each word after the phrase's own is completed from (`front/complete.rs`): `["doer", "model"]` for `switch`.
    #[serde(default)]
    pub complete: Vec<String>,
    /// The core's command the words run; or
    #[serde(default)]
    pub command: Option<String>,
    /// the flow they start, with what follows them as its goal (`/plan add a flag`): a flow is not a command, and a line that opens
    /// with them is not rebound or elected for.
    #[serde(default)]
    pub flow: Option<String>,
    /// Instead of both: where a line goes when it is one of the `likes` and `weigh.on` is on: `answer` or `light`.
    #[serde(default)]
    pub weight: Option<String>,
}

impl Kind for Phrase {
    const NAME: &'static str = "phrase";
}

/// A command and what came after its words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cmd {
    pub name: String,
    pub args: Vec<String>,
}

/// The command a line is when it opens with the words of a phrase that starts a flow.
pub const FLOW: &str = "flow";

/// The words of every phrase, each to the command it runs: a command of the core's, or [`FLOW`] and the flow's id.
#[derive(Debug, Clone, Default)]
pub struct Phrases {
    by_word: BTreeMap<String, (String, Option<String>)>,
    /// The forms of the arguments a command takes, by its words: a line whose words fit none is answered with them.
    takes: BTreeMap<String, Vec<String>>,
    /// The slots of what follows the words, by its words: what the completion offers for each.
    slots: BTreeMap<String, Vec<String>>,
    docs: Vec<(String, String)>,
    likes: Vec<Like>,
    filler: BTreeSet<String>,
}

/// A wording of plain language, as its content words (filler left out), and the command it stands for.
#[derive(Debug, Clone)]
struct Like {
    wording: String,
    words: Vec<String>,
    name: String,
    args: Vec<String>,
    /// A wording of a weight (`name` is `answer` or `light`), not of a command.
    weight: bool,
}

/// A line the table took for a command: the command, and the wording it was taken for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Taken {
    pub cmd: Cmd,
    pub wording: String,
}

/// The most words a line has to be tried against the wordings: a longer one is a request for work.
const SHORT: usize = 8;

/// Whether `words` are what `form` takes: a word that is written out as it is, `<name>` for one that must be there, `[name]` for one that may
/// be left out (after the ones that must be), `...` for any more.
fn fits(form: &str, words: &[&str]) -> bool {
    let mut at = 0;
    for part in form.split_whitespace() {
        match part {
            "..." => return true,
            p if p.starts_with('<') => {
                if at >= words.len() {
                    return false;
                }
                at += 1;
            }
            p if p.starts_with('[') => at = (at + 1).min(words.len()),
            p => {
                if words.get(at).is_none_or(|w| !w.eq_ignore_ascii_case(p)) {
                    return false;
                }
                at += 1;
            }
        }
    }
    at == words.len()
}

/// The words of `text`: lower case, letters and digits only.
fn words_of(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
}

/// The distance of two words, a swap of two letters being one step; none for a word of three letters or fewer, one letter for four to seven,
/// two for longer words is the most a typo may be.
pub(super) fn near(typed: &str, word: &str) -> Option<usize> {
    let (a, b): (Vec<char>, Vec<char>) = (typed.chars().collect(), word.chars().collect());
    let most = match b.len() {
        0..=3 => 0,
        4..=7 => 1,
        _ => 2,
    };
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in 0..=a.len() {
        for j in 0..=b.len() {
            d[i][j] = match (i, j) {
                (0, j) => j,
                (i, 0) => i,
                (i, j) => {
                    let mut best = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + usize::from(a[i - 1] != b[j - 1]));
                    if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                        best = best.min(d[i - 2][j - 2] + 1);
                    }
                    best
                }
            };
        }
    }
    Some(d[a.len()][b.len()]).filter(|n| *n <= most)
}

impl Phrases {
    /// Read the table. A word two phrases claim goes to the first and is reported.
    pub fn load(tables: &Tables) -> (Phrases, Vec<Problem>) {
        let (mut out, mut found) = (Phrases::default(), Vec::new());
        match tables.list::<Phrase>() {
            Ok(list) => {
                for p in list {
                    out.filler.extend(p.filler.iter().flat_map(|w| words_of(w)));
                    if p.command.is_none() && p.flow.is_none() && !p.filler.is_empty() && p.words.is_empty() && p.likes.is_empty() {
                        continue;
                    }
                    if let Some(w) = &p.weight {
                        if p.command.is_some() || p.flow.is_some() || !p.words.is_empty() || p.likes.is_empty() || !matches!(w.as_str(), "answer" | "light") {
                            found.push(Problem::refused(format!("[[phrase]] {}", p.id), "a phrase with a `weight` (answer or light) has `likes` and no `words`, `command` or `flow`"));
                            continue;
                        }
                        out.likes.extend(p.likes.iter().map(|l| Like { wording: l.clone(), words: Vec::new(), name: w.clone(), args: Vec::new(), weight: true }));
                        continue;
                    }
                    if p.words.is_empty() && p.likes.is_empty() {
                        found.push(Problem::refused(format!("[[phrase]] {}", p.id), "a phrase has `words` that open with a colon, or `likes`, or both"));
                        continue;
                    }
                    let (name, flow) = match (&p.command, &p.flow) {
                        (Some(c), None) => (c.clone(), None),
                        (None, Some(f)) => (FLOW.to_string(), Some(f.clone())),
                        _ => {
                            found.push(Problem::refused(format!("[[phrase]] {}", p.id), "a phrase runs a `command` or starts a `flow`, one of the two"));
                            continue;
                        }
                    };
                    if !p.likes.is_empty() && flow.is_some() {
                        found.push(Problem::refused(format!("[[phrase]] {}", p.id), "`likes` stand for a command of the core's, not for a flow"));
                        continue;
                    }
                    found.extend(out.claim(&format!("[[phrase]] {}", p.id), &p.words, &name, flow.as_deref().or(p.args.first().map(String::as_str)), flow.is_some()));
                    found.extend(out.take_slots(&format!("[[phrase]] {}", p.id), &p.words, &p.complete));
                    // a phrase with no doc is accepted and never listed (`:main`)
                    if let Some(first) = p.words.first().filter(|_| !p.doc.is_empty()) {
                        out.docs.push((first.clone(), p.doc.clone()));
                    }
                    for wording in &p.likes {
                        out.likes.push(Like { wording: wording.clone(), words: Vec::new(), name: name.clone(), args: p.args.clone(), weight: false });
                    }
                }
                let filler = out.filler.clone();
                for l in &mut out.likes {
                    // a wording of a weight is the whole line, filler and all: `are you there` is not `there`
                    l.words = words_of(&l.wording).into_iter().filter(|w| l.weight || !filler.contains(w)).collect();
                }
                out.likes.retain(|l| {
                    let empty = l.words.is_empty();
                    if empty {
                        found.push(Problem::refused(format!("[[phrase]] like `{}`", l.wording), "it is made of filler words only"));
                    }
                    !empty
                });
            }
            Err(p) => found.push(p),
        }
        (out, found)
    }

    /// A command as the operator would type it: `:usage`, `:chart spend`.
    pub fn said(cmd: &Cmd) -> String {
        std::iter::once(format!(":{}", cmd.name)).chain(cmd.args.iter().cloned()).collect::<Vec<_>>().join(" ")
    }

    /// The command a line in plain language is, when it is one of the wordings of a phrase: `show me the usage` is `usage`. Its words, with
    /// the filler left out, are the words of the wording, in any order, each as typed or a typo away (the sum of the distances is the
    /// cost). The wording nearest wins; two commands at the same cost, or a line of more than a few words, is none: the line goes on to Jev.
    pub fn take(&self, line: &str) -> Option<Taken> {
        let first = self.nearest(line, false)?;
        Some(Taken { cmd: Cmd { name: first.name.clone(), args: first.args.clone() }, wording: first.wording.clone() })
    }

    /// The weight a line has by its wording alone: `ping` is `answer`. Matched as [`Phrases::take`] matches a command's wording; the
    /// answer is the weight's word and the wording. None: no wording of a weight is the line's, or two weights are at the same cost.
    pub fn weigh(&self, line: &str) -> Option<(String, String)> {
        self.nearest(line, true).map(|l| (l.name.clone(), l.wording.clone()))
    }

    /// The wording of `weight` kind (or of a command) the line is nearest to, when one is, and every wording at that cost says the same.
    fn nearest(&self, line: &str, weight: bool) -> Option<&Like> {
        let typed = words_of(line);
        if typed.is_empty() || typed.len() > SHORT {
            return None;
        }
        // a weight is read from the whole line: no filler is left out, and a typo is allowed only in a word of five letters or more
        let typed: Vec<String> = if weight { typed } else { typed.into_iter().filter(|w| !self.is_filler(w)).collect() };
        let mut best: Vec<(usize, &Like)> = Vec::new();
        for like in self.likes.iter().filter(|l| l.weight == weight) {
            if let Some(cost) = if weight { Self::strict(&typed, &like.words) } else { Self::cost(&typed, &like.words) } {
                match best.first() {
                    Some((c, _)) if *c < cost => {}
                    Some((c, _)) if *c > cost => best = vec![(cost, like)],
                    _ => best.push((cost, like)),
                }
            }
        }
        let (_, first) = *best.first()?;
        best.iter().all(|(_, l)| (&l.name, &l.args) == (&first.name, &first.args)).then_some(first)
    }

    /// The distance of a whole line from a wording of a weight: the same words in the same order, each as written, or, for a word of five
    /// letters or more, a typo away (`helo`); `ring` and `pin` are not `ping`.
    fn strict(typed: &[String], wording: &[String]) -> Option<usize> {
        if typed.len() != wording.len() {
            return None;
        }
        typed.iter().zip(wording).try_fold(0, |sum, (t, w)| match (t == w, w.chars().count() >= 5) {
            (true, _) => Some(sum),
            (false, true) => near(t, w).map(|d| sum + d),
            (false, false) => None,
        })
    }

    /// Whether `word` is a filler word, or one typo away from one of four letters or more (`shwo me`).
    fn is_filler(&self, word: &str) -> bool {
        self.filler.contains(word) || (word.chars().count() >= 4 && self.filler.iter().any(|f| f.chars().count() >= 4 && near(word, f).is_some()))
    }

    /// The distance of a line's content words from a wording's: each word of the line takes the nearest word of the wording that is not taken.
    fn cost(typed: &[String], wording: &[String]) -> Option<usize> {
        if typed.is_empty() || typed.len() != wording.len() {
            return None;
        }
        let (mut free, mut cost): (Vec<&String>, usize) = (wording.iter().collect(), 0);
        for t in typed {
            let (at, d) = free.iter().enumerate().filter_map(|(i, w)| near(t, w).map(|d| (i, d))).min_by_key(|(_, d)| *d)?;
            free.remove(at);
            cost += d;
        }
        Some(cost)
    }

    /// Take the `words` for the command `name` (and for what follows them as its first argument). A word must open with a colon, or
    /// with a slash when `slash` (a flow's, or a core command's such as `/usage`); one that another phrase has is refused and said.
    pub fn claim(&mut self, at: &str, words: &[String], name: &str, first: Option<&str>, slash: bool) -> Vec<Problem> {
        let mut found = Vec::new();
        for w in words {
            let word = w.trim().to_ascii_lowercase();
            let lead = word.starts_with(':') || (slash && word.starts_with('/'));
            if !lead || word.len() < 2 || word.chars().any(char::is_whitespace) {
                found.push(Problem::refused(at, format!("`{w}` is not a word that opens with a colon{}", if slash { " or a slash" } else { "" })));
            } else if let Entry::Vacant(slot) = self.by_word.entry(word) {
                slot.insert((name.to_string(), first.map(String::from)));
            } else {
                found.push(Problem::refused(at, format!("`{w}` is already a word of another phrase")));
            }
        }
        found
    }

    /// Say what the command of `words` takes: `<name>` is one word, `[name]` one more that may be left out, `...` any words after.
    pub fn take_forms(&mut self, words: &[String], forms: &[String]) {
        if !forms.is_empty() {
            self.takes.extend(words.iter().map(|w| (w.trim().to_ascii_lowercase(), forms.to_vec())));
        }
    }

    /// Say what each word after `words` is completed from; a slot that is none of [`super::complete::KINDS`] is refused and said.
    pub fn take_slots(&mut self, at: &str, words: &[String], slots: &[String]) -> Vec<Problem> {
        let bad: Vec<Problem> = slots.iter().filter(|s| !super::complete::known(s)).map(|s| Problem::refused(at, format!("`{s}` is not a slot to complete: one of {}, `word:look` or words joined by `|`", super::complete::KINDS.join(", ")))).collect();
        if bad.is_empty() && !slots.is_empty() {
            self.slots.extend(words.iter().map(|w| (w.trim().to_ascii_lowercase(), slots.to_vec())));
        }
        bad
    }

    /// The slots of what follows `word`, the first word of a line; none: nothing after it is completed.
    pub fn slots(&self, word: &str) -> &[String] {
        self.slots.get(&word.to_ascii_lowercase()).map_or(&[], Vec::as_slice)
    }

    /// What to answer a line that opens with the words of a command that says what it takes and does not give it: its usage. None: the line
    /// fits one of the forms, or the command says none. (A command is answered here, before a flow or a task is made for it.)
    pub fn misuse(&self, line: &str) -> Option<String> {
        let mut words = line.split_whitespace();
        let lead = words.next()?.to_ascii_lowercase();
        let forms = self.takes.get(&lead)?;
        let rest: Vec<&str> = words.collect();
        (!forms.iter().any(|f| fits(f, &rest))).then(|| format!("usage: {}", forms.iter().map(|f| format!("{lead} {f}")).collect::<Vec<_>>().join("  ·  ")))
    }

    /// The word a person typed as the line would open with it: the word itself when it has a lead, else with a colon or a slash.
    pub fn lead(&self, word: &str) -> Option<String> {
        let word = word.to_ascii_lowercase();
        [word.clone(), format!(":{word}"), format!("/{word}")].into_iter().find(|w| self.by_word.contains_key(w))
    }

    /// The command a line is, when it opens with the words of one.
    pub fn parse(&self, line: &str) -> Option<Cmd> {
        let mut words = line.split_whitespace();
        let (name, flow) = self.by_word.get(&words.next()?.to_ascii_lowercase())?;
        // a flow's id comes first among the arguments, then the line as it was typed
        let rest: Vec<String> = words.map(String::from).collect();
        let args = match flow {
            Some(f) => std::iter::once(f.clone()).chain(rest).collect(),
            None => rest,
        };
        Some(Cmd { name: name.clone(), args })
    }

    /// What to say to a line that opens with a colon and with no command's word: the word, and the commands nearest to it by name (the
    /// start or the longer form of one, or a typo: one letter in a word of three to five, two in a longer one), at most four. The line is not sent to a doer.
    pub fn unknown(&self, line: &str) -> String {
        let word = line.split_whitespace().next().unwrap_or(":").to_ascii_lowercase();
        let mut near: Vec<(usize, &String)> = self
            .by_word
            .keys()
            .filter(|w| w.starts_with(':'))
            .filter_map(|w| {
                let kin = word.len() >= 3 && (w.starts_with(&word) || word.starts_with(w.as_str()));
                let dist = if kin { 0 } else { super::models::edit_distance(&word, w) };
                (dist <= (word.len() / 3).min(2) && word.len() >= 3).then_some((dist, w))
            })
            .collect();
        near.sort();
        let list: Vec<&str> = near.iter().take(4).map(|(_, w)| w.as_str()).collect();
        let hint = if list.is_empty() { "`:help` lists them".to_string() } else { format!("nearest: {}; `:help` lists them", list.join(", ")) };
        format!("no command `{word}` ({hint}); the line was not sent to a doer")
    }

    /// The ids of the flows the phrases start.
    pub fn flows(&self) -> Vec<String> {
        let mut v: Vec<String> = self.by_word.values().filter(|(n, _)| n == FLOW).filter_map(|(_, f)| f.clone()).collect();
        v.sort();
        v.dedup();
        v
    }

    /// `(first word, what it does)` for each phrase.
    pub fn docs(&self) -> &[(String, String)] {
        &self.docs
    }

    /// The wordings that stand for a command, each once: what the completion offers as whole lines (a greeting's weight stands for none).
    pub fn wordings(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for l in self.likes.iter().filter(|l| !l.weight) {
            if !out.contains(&l.wording.as_str()) {
                out.push(&l.wording);
            }
        }
        out
    }
}

/// `rule:intake`: a line that is a command says so; any other line is not this rule's to decide.
pub fn intake_rule(p: Phrases) -> impl Answerer {
    fn_rule("rule:intake", move |q| match p.parse(&q.text) {
        Some(c) => picked("command", format!("it opens with the words of `{}`", c.name)),
        None => Err("it opens with no command's words".into()),
    })
}

/// `rule:weigh`: a line that is a wording of a weight in the table says it; any other line is not this rule's to decide, and the chain goes on
/// to Jev.
pub fn weigh_rule(p: Phrases) -> impl Answerer {
    fn_rule("rule:weigh", move |q| match p.weigh(&q.text) {
        Some((weight, wording)) => picked(weight, format!("the line is a wording of `{wording}`")),
        None => Err("the table has no wording of it".into()),
    })
}
