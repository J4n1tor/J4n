//! `suggest`: what Tab completes, for the composer and for a Path or Ref field. The token under the cursor is completed by its slot,
//! what the line expects there: a command (`:sw`), a chat (`@c3`), a word of a command (the core's `complete:` answers it), a file, or
//! plain words, which a whole line completes (the core's: the project's lines, the newest answer's steps, the phrases' wordings; the
//! screen's: the open question's options). On an empty line `⇥` asks the core for the next line and puts the first in the line when the
//! answer comes. Code makes every candidate and one score orders them. The rest of the best one is the ghost, drawn dim after the cursor and never part of
//! the line until it is taken: `⇥` takes it whole and steps on through the list, `⇧⇥` steps back, `→` at the end takes it, `⌃→` one
//! word of it, `esc` puts back what was typed. Typing never waits: the core's answer comes when it comes, and one for another draft is
//! dropped. With `complete.on` off the core says so in its answer, and Tab takes only a question's recommended option.
//!
//! The list is drawn in the left of the composer's strip ([`row`]): `⇥`, the candidates two cells apart, the one in the line in the
//! accent, `+k` for the ones that do not fit. It comes when a token has two candidates or more and stays while the line is not empty, so
//! the composer does not bob; it goes when the line is sent or emptied, on `esc`, when the keys leave the line, and while a question
//! stands over the composer.

use crate::app::App;
use crate::fuzzy;
use crate::parts::line::Typed;
use crate::theme::Theme;
use crate::theme::glyph::{Slot as Glyph, Tier};
use crate::theme::palette::Role as Hue;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use j4n_proto::{CHATS, COMPLETE, FieldKind, Json, PALETTE, Row, Shape, Tone, Up};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// The weights of the score, in one table: the token starts the candidate, starts one of its words, or has its letters in order (and
/// then fuzzy's own score, under 100, on top); the candidate is of the slot the line expects; a file; an open question's option. A whole
/// line's match and its source (the project's lines, the answer's steps, a phrase's wording) are the core's, in its row's `score`.
pub const WEIGHTS: Weights = Weights { prefix: 300, word: 200, fuzzy: 100, slot: 50, file: 10, question: 40 };
/// The most candidates the core is asked for.
pub const MOST: u64 = 50;
/// The most cells an item of the list takes, and a sentence's.
pub const ITEM: usize = 24;
pub const SENTENCE: usize = 32;
/// What the core is asked on an empty line: its next line.
pub const NEXT: &str = "line ";
/// The ends of a file's name that make a token a file, besides a `/` in it.
const EXTS: [&str; 12] = ["rs", "py", "md", "toml", "json", "ts", "js", "go", "txt", "yaml", "yml", "sh"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Weights {
    pub prefix: i32,
    pub word: i32,
    pub fuzzy: i32,
    pub slot: i32,
    pub file: i32,
    pub question: i32,
}

/// What the line expects where the cursor is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// Nothing typed.
    Empty,
    /// The first word, `:` or `/` and a name.
    Command,
    /// `@` and a chat.
    Chat,
    /// A word after a command: the core reads the command's `complete` row and says what it is.
    Word,
    File,
    /// Plain words: a whole line completes them, and the token is the line.
    Sentence,
}

/// The slot, the token (the letters typed since the last blank) and where it starts, in characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct At {
    pub slot: Slot,
    pub token: String,
    pub start: usize,
}

/// A candidate: what it puts in the line, what stands beside it, its score, and whether a Decider put it first (drawn `~`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cand {
    pub text: String,
    pub side: String,
    pub score: i32,
    pub made: bool,
}

/// Where the end of `before` (the line up to the cursor) is.
pub fn slot_of(before: &str) -> At {
    let cut = before.char_indices().rev().find(|(_, c)| c.is_whitespace()).map_or(0, |(i, c)| i + c.len_utf8());
    let (head, token) = (before[..cut].trim(), &before[cut..]);
    let file = token.contains('/') || token.rsplit_once('.').is_some_and(|(name, ext)| !name.is_empty() && EXTS.contains(&ext));
    let slot = match () {
        _ if head.is_empty() && token.is_empty() => Slot::Empty,
        _ if head.is_empty() && token.starts_with([':', '/']) => Slot::Command,
        _ if token.starts_with('@') => Slot::Chat,
        _ if head.starts_with([':', '/']) => Slot::Word,
        _ if file => Slot::File,
        _ => return At { slot: Slot::Sentence, token: before.trim_start().into(), start: before.chars().count() - before.trim_start().chars().count() },
    };
    At { slot, token: token.into(), start: before[..cut].chars().count() }
}

/// Whether plain words are asked of the core: three letters or a whole word typed; while the line is one word, only while it reads as a
/// word (letters, a capital first at most), so the start of a key typed alone never goes.
fn ripe(line: &str) -> bool {
    let t = line.trim_start();
    let word = t.chars().all(char::is_alphabetic) && !t.chars().skip(1).any(char::is_uppercase);
    t.contains(char::is_whitespace) || (word && t.chars().count() >= 3)
}

/// What the core is asked for the slot: the line up to the cursor for a command or a command's word, the token for a file; none for a
/// slot the screen holds.
pub fn find(at: &At, before: &str) -> Option<String> {
    // what may be a key is never asked of the core: the words of a `:key …` line, a line that is one key-like word
    if crate::chat::held::key_line(before) || crate::chat::held::key_like(before) {
        return None;
    }
    // a line of one word is a file only while no `/` part of it holds a run a key or a hash has: the start of a key is not yet 20 long
    let lone = !before.trim().contains(char::is_whitespace);
    match at.slot {
        Slot::Command | Slot::Word => Some(format!("line {before}")),
        Slot::File if lone && at.token.split('/').any(crate::chat::held::keyish) => None,
        Slot::File => Some(format!("file {}", at.token)),
        Slot::Sentence if ripe(before) => Some(format!("line {before}")),
        _ => None,
    }
}

/// How well the words typed start a whole line, in whatever case: each word as it is there and the last the start of its word, or that
/// start one letter away in a word of four letters or more; none when not.
pub fn starts(typed: &str, text: &str) -> Option<i32> {
    let words = |s: &str| s.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>();
    let (t, x) = (words(typed), words(text));
    let (last, head) = t.split_last()?;
    if t.len() > x.len() || head.iter().zip(&x).any(|(a, b)| a != b) {
        return None;
    }
    let word: Vec<char> = x[head.len()].chars().collect();
    let last: Vec<char> = last.chars().collect();
    if typed.ends_with(char::is_whitespace) {
        return (word == last && x.len() > t.len()).then_some(WEIGHTS.prefix);
    }
    if word.starts_with(&last) {
        return Some(WEIGHTS.prefix);
    }
    let n = last.len();
    (n >= 4 && (n - 1..=n + 1).any(|k| one_off(&last, &word[..k.min(word.len())]))).then_some(WEIGHTS.word)
}

/// Whether two words are one letter apart: one missing, added, changed, or two side by side swapped.
fn one_off(a: &[char], b: &[char]) -> bool {
    let (a, b) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let lead = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    match b.len() - a.len() {
        0 => a[lead..].len() <= 1 || a[lead + 1..] == b[lead + 1..] || (a[lead..].len() >= 2 && a[lead] == b[lead + 1] && a[lead + 1] == b[lead] && a[lead + 2..] == b[lead + 2..]),
        1 => a[lead..] == b[lead + 1..],
        _ => false,
    }
}

/// How well `token` matches `text`, case aside: at its start, at the start of one of its words, its letters in order; none when not.
pub fn matched(token: &str, text: &str) -> Option<i32> {
    let (t, x) = (token.to_lowercase(), text.to_lowercase());
    if x.starts_with(&t) {
        return Some(WEIGHTS.prefix);
    }
    let mut prev = None;
    for (i, c) in x.char_indices() {
        if prev.is_some_and(|p: char| !p.is_alphanumeric()) && x[i..].starts_with(&t) {
            return Some(WEIGHTS.word);
        }
        prev = Some(c);
    }
    let q: Vec<char> = t.chars().filter(|c| !c.is_whitespace()).collect();
    fuzzy::score(&q, text).map(|s| WEIGHTS.fuzzy + s.clamp(0, WEIGHTS.fuzzy - 1))
}

/// The candidates for `token`, best first: `items` are the text, what stands beside it and what its source adds. One that is the token
/// itself or matches nothing is left out; ties go to the shorter, then to the one listed first.
pub fn rank(token: &str, items: Vec<(String, String, i32)>) -> Vec<Cand> {
    let mut out: Vec<Cand> = Vec::new();
    for (text, side, plus) in items {
        if text != token && !out.iter().any(|c| c.text == text)
            && let Some(m) = matched(token, &text)
        {
            out.push(Cand { score: m + WEIGHTS.slot + plus, text, side, made: false });
        }
    }
    out.sort_by(|a, b| b.score.cmp(&a.score).then(a.text.chars().count().cmp(&b.text.chars().count())));
    out
}

/// The rest of `top` after `token`, when it extends it.
pub fn ghost_of(token: &str, top: &Cand) -> Option<String> {
    let n = token.chars().count();
    (top.text.to_lowercase().starts_with(&token.to_lowercase()) && top.text.chars().count() > n).then(|| top.text.chars().skip(n).collect())
}

/// One word of a ghost: the blanks before it and the word.
pub fn word_of(ghost: &str) -> &str {
    let lead = ghost.len() - ghost.trim_start().len();
    &ghost[..ghost[lead..].find(char::is_whitespace).map_or(ghost.len(), |i| lead + i)]
}

/// Tab stepping through a list that holds still: the line as it was before the first Tab, the characters the candidate stands in for
/// (`start..end`), the list, the one in the line, and the candidates the list was made of, as the strip shows them (none for a field).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub before: Typed,
    pub start: usize,
    pub end: usize,
    pub list: Vec<String>,
    pub at: usize,
    pub cands: Vec<Cand>,
}

impl Step {
    /// The first Tab: the first of `list`, or with `back` the last; none when there is nothing to take.
    pub fn begin(line: &Typed, start: usize, end: usize, list: Vec<String>, back: bool) -> Option<Step> {
        let at = if back { list.len().checked_sub(1)? } else { (!list.is_empty()).then_some(0)? };
        Some(Step { before: line.clone(), start, end, list, at, cands: Vec::new() })
    }

    /// The next (or the one before), round the end.
    pub fn next(&mut self, back: bool) {
        let n = self.list.len().max(1);
        self.at = if back { (self.at + n - 1) % n } else { (self.at + 1) % n };
    }

    /// The line with the candidate in place, the cursor after it.
    pub fn line(&self) -> Typed {
        let chars: Vec<char> = self.before.buf.chars().collect();
        let (start, end) = (self.start.min(chars.len()), self.end.clamp(self.start, chars.len()));
        let pick = self.list.get(self.at).map_or("", String::as_str);
        let buf: String = chars[..start].iter().copied().chain(pick.chars()).chain(chars[end..].iter().copied()).collect();
        Typed { buf, at: start + pick.chars().count() }
    }
}

/// What the screen keeps of the completion: the find last asked of the core, the line whose ghost `esc` hid, Tab's steps, whether the
/// list is up (it stays while the line is not empty), the line whose list `esc` put away, and a `⇥` (or `⇧⇥`, `true`) on an empty line
/// that waits for the core's next line.
#[derive(Debug, Clone, Default)]
pub struct State {
    pub asked: Option<String>,
    pub hidden: Option<String>,
    pub step: Option<Step>,
    pub open: bool,
    pub shut: Option<String>,
    pub next: Option<bool>,
}

/// What the list shows: the candidates, the one in the line (none when the best is not in it), and the slot they fill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct List {
    pub items: Vec<Cand>,
    pub at: Option<usize>,
    pub slot: Slot,
}

/// `text` in at most `w` cells, the tier's mark where it is cut; a path keeps its first folder and its name and loses the middle.
fn cut(th: &Theme, text: &str, w: usize, path: bool) -> String {
    if text.width() <= w {
        return text.to_string();
    }
    let mark = th.glyph(Glyph::Cut);
    if path && let (Some((head, _)), Some((_, name))) = (text.split_once('/'), text.rsplit_once('/')) {
        let short = format!("{head}/{mark}/{name}");
        if short.width() <= w {
            return short;
        }
    }
    let room = w.saturating_sub(mark.width());
    let mut used = 0;
    let kept: String = text.chars().take_while(|c| {
        used += c.width().unwrap_or(0);
        used <= room
    }).collect();
    format!("{}{mark}", kept.trim_end())
}

/// The one in the line: accent and bold; without colour bold and underlined.
fn picked(th: &Theme) -> Style {
    if th.no_color { Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED) } else { th.fg(Hue::Accent).add_modifier(Modifier::BOLD) }
}

/// One item: `~` when a Decider put it first, its text (a chat without its `@`), and dim what stands beside it, in [`ITEM`] cells (a
/// sentence in [`SENTENCE`]); a file is cut in the middle.
fn item(th: &Theme, c: &Cand, slot: Slot, on: bool) -> Vec<Span<'static>> {
    let most = if matches!(slot, Slot::Sentence | Slot::Empty) { SENTENCE } else { ITEM };
    let mut out = Vec::new();
    if c.made {
        out.push(Span::styled(th.glyph(Glyph::Made), th.fg(Hue::Made)));
    }
    let lead = usize::from(c.made);
    let text = if slot == Slot::Chat { c.text.strip_prefix('@').unwrap_or(&c.text) } else { &c.text };
    let text = cut(th, text, most - lead, slot == Slot::File);
    let room = most.saturating_sub(lead + text.width() + 1);
    out.push(Span::styled(text, if on { picked(th) } else { th.tone(Tone::Plain) }));
    if !c.side.is_empty() && room >= 2 {
        out.push(Span::styled(format!(" {}", cut(th, &c.side, room, false)), th.tone(Tone::Dim)));
    }
    out
}

fn width(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.content.width()).sum()
}

/// The row of the list in `room` cells: `⇥` (`tab:` in words), the items two cells apart, `+k` for the ones that do not fit (the list
/// starts at the one in the line when it would not be in sight); `short`, the one in the line and its place, `⇥ :switch 1/5`. With no
/// candidates, `⇥` alone and dim.
pub fn row(th: &Theme, list: &List, room: usize, short: bool) -> Vec<Span<'static>> {
    let words = th.tier == Tier::Words;
    let n = list.items.len();
    let dim = th.tone(Tone::Dim);
    let lead = if words { "tab:".to_string() } else { th.glyph(Glyph::Hint).to_string() };
    let mut out = vec![Span::styled(lead, if n == 0 { dim } else { th.fg(Hue::Accent) })];
    if n == 0 {
        return out;
    }
    let shown: Vec<Vec<Span<'static>>> = list.items.iter().enumerate().map(|(i, c)| item(th, c, list.slot, list.at == Some(i))).collect();
    if short {
        let i = list.at.unwrap_or(0);
        out.push(Span::raw(" "));
        out.extend(shown[i].clone());
        out.push(Span::styled(format!(" {}/{n}", i + 1), dim));
        return out;
    }
    let more = |k: usize| if words { format!("+{k} more") } else { format!("+{k}") };
    let fit = |from: usize| {
        let mut used = width(&out);
        let mut took = 0;
        for (i, s) in shown.iter().enumerate().skip(from) {
            let gap = if took == 0 { 1 } else { 2 };
            let tail = if i + 1 < n { 2 + more(n - took - 1).width() } else { 0 };
            if took > 0 && used + gap + width(s) + tail > room {
                break;
            }
            used += gap + width(s);
            took += 1;
        }
        took
    };
    let mut from = 0;
    let mut took = fit(0);
    if let Some(a) = list.at.filter(|a| *a >= took) {
        (from, took) = (a, fit(a));
    }
    for (k, s) in shown.into_iter().skip(from).take(took).enumerate() {
        out.push(Span::raw(if k == 0 { " " } else { "  " }));
        out.extend(s);
    }
    if took < n {
        out.push(Span::styled(format!("  {}", more(n - took)), dim));
    }
    out
}

fn cell(r: &Row, i: usize) -> &str {
    r.cells.get(i).map_or("", |c| c.text.as_str())
}

/// The open question's recommended option, taken: what `⇥` on an empty line, `→` at the end and `⌃→` do. None when it has none.
pub(crate) fn hint(app: &App) -> Option<Vec<Up>> {
    let a = app.ask_focus()?.clone();
    let o = crate::ask::options(&a, "").into_iter().find(|o| o.hint)?;
    Some(vec![Up::Intent { intent: o.intent(&a) }])
}

impl App {
    /// Whether completion is on: the core's answer says `on: false` while `complete.on` is off; until it has said, it is on.
    pub fn complete_on(&self) -> bool {
        self.rows.get(COMPLETE).and_then(|r| r.first()).and_then(|r| r.data.as_ref()?.get("on")).and_then(Json::as_bool) != Some(false)
    }

    /// The composer has the keys: a chat's line, not its transcript, the list of the chats or the shaper's card.
    pub(crate) fn composing(&self) -> bool {
        self.pin.is_none() && !self.composer.focus && self.field().is_some_and(|f| f.kind == FieldKind::Chat && !self.wins.get(&f.bind).is_some_and(|w| w.at.is_some()))
    }

    fn before(&self) -> String {
        self.line.buf.chars().take(self.line.at).collect()
    }

    /// What the core is asked for the line as it is now: plain words only with the cursor at the end; none for an empty line, whose next
    /// line is asked on `⇥`.
    fn find_now(&self) -> (At, Option<String>) {
        let before = self.before();
        let at = slot_of(&before);
        let end = self.line.at >= self.line.chars();
        let find = if at.slot == Slot::Sentence && !end { None } else { find(&at, &before) };
        (at, find)
    }

    /// Whole lines for the line as typed (on an empty line, its next line): the core's, as it scored them, and the open question's
    /// options; never one that may be a key.
    fn sentences(&self, at: &At, core: &[Row]) -> Vec<Cand> {
        let end = self.line.at >= self.line.chars();
        let own = self.ask_target().filter(|a| a.shape == Shape::Choice && at.slot == Slot::Sentence && end).into_iter().flat_map(|a| &a.options);
        let own = own.filter_map(|c| starts(&at.token, &c.label).map(|m| (c.label.clone(), String::new(), m + WEIGHTS.question)));
        let score = |r: &Row| r.data.as_ref().and_then(|d| d.get("score")).and_then(Json::as_i64).and_then(|s| i32::try_from(s).ok()).unwrap_or(0);
        let core = core.iter().map(|r| (cell(r, 0).to_string(), cell(r, 1).to_string(), score(r)));
        let mut out: Vec<Cand> = Vec::new();
        for (text, side, score) in own.chain(core) {
            let key = crate::chat::held::key_line(&text) || crate::chat::held::key_like(&text);
            if !key && !text.trim().eq_ignore_ascii_case(at.token.trim()) && !out.iter().any(|c| c.text.eq_ignore_ascii_case(&text)) {
                out.push(Cand { text, side, score: score + WEIGHTS.slot, made: false });
            }
        }
        out.sort_by(|a, b| b.score.cmp(&a.score).then(a.text.chars().count().cmp(&b.text.chars().count())));
        out
    }

    /// The candidates for the token at the composer's cursor, best first, and where it is.
    pub fn candidates(&self) -> (At, Vec<Cand>) {
        let (at, find) = self.find_now();
        let rows = |b: &str| self.rows.get(b).map_or(&[][..], Vec::as_slice);
        let chats = |pre: &str| rows(CHATS).iter().map(|r| (format!("{pre}{}", cell(r, 1)), cell(r, 2).to_string(), 0)).collect::<Vec<_>>();
        // the core's answer, only for the line as it is now: an answer for an older draft is dropped
        let find = if at.slot == Slot::Empty { Some(NEXT.to_string()) } else { find };
        let answer = rows(COMPLETE).split_first().filter(|(h, _)| h.key == "~for" && Some(cell(h, 0)) == find.as_deref());
        let core = |rest: &[Row]| rest.iter().map(|r| (cell(r, 0).to_string(), cell(r, 1).to_string(), if r.key.starts_with("file:") { WEIGHTS.file } else { 0 })).collect::<Vec<_>>();
        // what the Decider's pick adds, as the core said it: the core's score is not read for a word, and a command the palette holds too
        // is the palette's row, so it goes by the text
        let lift = |t: &str| answer.and_then(|(_, rest)| rest.iter().find(|r| cell(r, 0) == t)?.data.as_ref()?.get("decider")?.as_i64()).and_then(|n| i32::try_from(n).ok()).unwrap_or(0);
        let items = match at.slot {
            _ if !self.complete_on() => Vec::new(),
            Slot::Empty | Slot::Sentence => Vec::new(),
            // the palette's at once, and the core's words (the phrases' commands) when its answer comes
            Slot::Command => rows(PALETTE)
                .iter()
                .flat_map(|r| cell(r, 0).split_whitespace().filter(|w| w.starts_with([':', '/'])).map(|w| (w.to_string(), String::new(), 0)))
                .chain(answer.map(|(_, rest)| core(rest)).unwrap_or_default())
                .collect(),
            Slot::Chat => chats("@"),
            Slot::Word | Slot::File => match answer {
                Some((h, _)) if cell(h, 1) == "chat" => chats(""),
                Some((_, rest)) => core(rest),
                None => Vec::new(),
            },
        };
        // a candidate the Decider put first is marked so in the core's row
        let made: Vec<&str> = answer.map(|(_, rest)| rest.iter().filter(|r| r.data.as_ref().and_then(|d| d.get("made")).and_then(Json::as_bool) == Some(true)).map(|r| cell(r, 0)).collect()).unwrap_or_default();
        let mut found = match at.slot {
            Slot::Empty | Slot::Sentence if self.complete_on() => self.sentences(&at, answer.map_or(&[][..], |(_, rest)| rest)),
            _ => rank(&at.token, items.into_iter().map(|(t, side, plus)| {
                let up = lift(&t);
                (t, side, plus + up)
            }).collect()),
        };
        // the mark is the Decider's first: a pick that did not come first carries none
        found.iter_mut().enumerate().for_each(|(i, c)| c.made = i == 0 && made.contains(&c.text.as_str()));
        (at, found)
    }

    /// Whether a ghost may stand after the cursor: at the end of a line that is not empty, Tab not stepping, and `esc` did not hide it.
    fn ghostly(&self) -> bool {
        self.suggest.step.is_none() && self.line.at >= self.line.chars() && self.composing() && self.suggest.hidden.as_ref() != Some(&self.line.buf) && !self.line.buf.trim().is_empty()
    }

    /// The ghost: the rest of the best candidate, while the cursor is at the end, Tab is not stepping and `esc` did not hide it.
    pub fn ghost(&self) -> Option<String> {
        if !self.ghostly() {
            return None;
        }
        let (at, found) = self.candidates();
        ghost_of(&at.token, found.first()?)
    }

    /// What the composer shows of the completion now, the candidates made once: the ghost, and the list while it is up.
    pub(crate) fn suggest_now(&mut self) -> (Option<String>, Option<List>) {
        let gone = !self.composing() || !self.complete_on() || self.line.buf.trim().is_empty() || !self.asks_shown().is_empty() || self.suggest.shut.as_ref() == Some(&self.line.buf);
        if gone {
            self.suggest.open = false;
        }
        if let Some(s) = &self.suggest.step {
            let before: String = s.before.buf.chars().take(s.end).collect();
            let items = if s.cands.len() == s.list.len() { s.cands.clone() } else { s.list.iter().map(|t| Cand { text: t.clone(), ..Cand::default() }).collect() };
            self.suggest.open |= !gone;
            return (None, self.suggest.open.then(|| List { items, at: Some(s.at), slot: slot_of(&before).slot }));
        }
        if !self.composing() {
            return (None, None);
        }
        let (at, found) = self.candidates();
        let ghost = if self.ghostly() { found.first().and_then(|c| ghost_of(&at.token, c)) } else { None };
        self.suggest.open |= !gone && found.len() >= 2;
        let on = ghost.is_some().then_some(0);
        (ghost, self.suggest.open.then_some(List { items: found, at: on, slot: at.slot }))
    }

    /// The window of `complete:` for the line as it is now, when its find changed: none while Tab steps (the list holds still) or where
    /// the screen holds the slot. Sent with the key, never waited for.
    pub(crate) fn complete_window(&mut self) -> Option<Up> {
        if self.suggest.step.is_some() || !self.complete_on() || !self.composing() {
            return None;
        }
        let (_, find) = self.find_now();
        if find == self.suggest.asked {
            return None;
        }
        self.suggest.asked.clone_from(&find);
        Some(Up::Window { bind: COMPLETE.into(), from: 0, count: MOST, sort: String::new(), find: find? })
    }

    /// `⇥` (or `⇧⇥`, `back`) in the composer: on an empty line the open question's recommended option, else the core is asked for the
    /// next line and the first (or the last) is put in the line when it answers; on a typed line the best candidate whole, then the next
    /// (or the one before). Nothing to complete, nothing done; Tab never types.
    pub(crate) fn complete_tab(&mut self, back: bool) -> Vec<Up> {
        if !self.composing() {
            return Vec::new();
        }
        if self.line.buf.trim().is_empty() && self.suggest.step.is_none() {
            if let Some(ups) = hint(self).filter(|_| !back) {
                return ups;
            }
            if !self.complete_on() {
                return Vec::new();
            }
            self.suggest.next = Some(back);
            return vec![Up::Window { bind: COMPLETE.into(), from: 0, count: MOST, sort: String::new(), find: NEXT.into() }];
        }
        if let Some(s) = self.suggest.step.as_mut() {
            s.next(back);
            self.line = s.line();
            return Vec::new();
        }
        let (at, found) = self.candidates();
        self.suggest.step = Step::begin(&self.line, at.start, self.line.at, found.iter().map(|c| c.text.clone()).collect(), back).map(|s| Step { cands: found, ..s });
        if let Some(s) = &self.suggest.step {
            self.line = s.line();
        }
        Vec::new()
    }

    /// The core's answer came: a `⇥` on an empty line that waits for the next line puts the first (or the last) in the line and steps on
    /// from there, as on a typed line. An answer for another find leaves it waiting; any key ends the wait.
    pub(crate) fn next_came(&mut self) {
        let head = self.rows.get(COMPLETE).and_then(|r| r.first());
        if self.suggest.next.is_none() || head.is_none_or(|h| h.key != "~for" || cell(h, 0) != NEXT) {
            return;
        }
        let back = self.suggest.next.take() == Some(true);
        if !self.composing() || !self.line.buf.trim().is_empty() || self.suggest.step.is_some() {
            return;
        }
        let (_, found) = self.candidates();
        self.suggest.step = Step::begin(&self.line, 0, self.line.at, found.iter().map(|c| c.text.clone()).collect(), back).map(|s| Step { cands: found, ..s });
        if let Some(s) = &self.suggest.step {
            self.line = s.line();
        }
    }

    /// A key in the composer for the completion, before the question and the line read it: `Some` when it was taken. While Tab steps,
    /// `⏎` and `→` keep the one in the line, `esc` puts back what was typed, and any other key keeps the line and goes on from it.
    pub(crate) fn complete_key(&mut self, k: KeyEvent) -> Option<Vec<Up>> {
        if !self.composing() {
            return None;
        }
        let (ctrl, plain) = (k.modifiers.contains(KeyModifiers::CONTROL), k.modifiers.is_empty());
        let end = self.line.at >= self.line.chars();
        if let Some(s) = self.suggest.step.take() {
            return match k.code {
                KeyCode::Enter if plain => Some(Vec::new()),
                KeyCode::Right if ctrl || end => Some(Vec::new()),
                KeyCode::Esc => {
                    self.line = s.before;
                    self.suggest.shut = Some(self.line.buf.clone());
                    Some(Vec::new())
                }
                _ => None,
            };
        }
        // the list up with no ghost: `esc` puts it away and keeps the line
        if k.code == KeyCode::Esc && plain && self.suggest.open && !self.line.buf.trim().is_empty() && self.ghost().is_none() {
            self.suggest.shut = Some(self.line.buf.clone());
            return Some(Vec::new());
        }
        let ghost = self.ghost()?;
        match k.code {
            KeyCode::Right if plain => {
                let (at, found) = self.candidates();
                self.line = Step::begin(&self.line, at.start, self.line.at, found.into_iter().take(1).map(|c| c.text).collect(), false)?.line();
            }
            KeyCode::Right if ctrl => word_of(&ghost).chars().for_each(|c| self.line.insert(c)),
            KeyCode::Esc if plain => {
                self.suggest.hidden = Some(self.line.buf.clone());
                self.suggest.shut = Some(self.line.buf.clone());
            }
            _ => return None,
        }
        Some(Vec::new())
    }
}
