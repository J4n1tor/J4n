//! What the core completes (`complete:`): the screen asks with a window whose `find` is `<slot> <token>`, and is sent the row `~for`
//! (the find as asked, so an answer for an older draft is told apart) and the candidates of the slot, best first, whole: the window is
//! not cut by its find. The slot `line` is the line up to the cursor; its first word is a command, whose `complete` row says what each
//! word after it is, and while that first word is being typed it is a command: the phrases' words and the palette's. Plain words are a
//! `sentence`: whole lines whose first words are the ones typed, from the project's own lines (by how often and how lately, read from the
//! Log), the newest answer's next steps and the phrases' wordings. An empty line is the `next` line: the newest answer's steps, then the
//! lines typed most often. What the screen holds itself (the chats) is answered with `~for` alone.
//!
//! The Decider (the ask point `complete`) only picks among code's candidates: for a whole line, the next line, or a slot whose best two
//! are close. The window's find must stand still for a pause first; one ask is in flight at a time, a newer window drops the one that
//! waits, and the answer lands in a cache whose order every later window of the same candidates takes with no ask. It is sent the typed
//! line, the heads of the last turns and the candidates' texts, redacted, never a file's text, and at most `complete.decider_day` times a
//! day; each pause is a `complete.ranked` in the Log. Its failures are its own (the point is outside the breaker, which every point
//! shares): [`FAILS`] in a row rest its asks for [`REST_MS`] and leave the answerer as it was.

use super::Front;
use crate::answer::chain::Walk;
use crate::answer::{State, StateView};
use crate::ask::{Answer, Opt};
use crate::change::Actor;
use crate::gate::{Action, Gate, Verb, Verdict};
use crate::log::{Log, New};
use crate::setting::{SettingRow, Ty};
use crate::table::Kind;
use crate::value::Value;
use j4n_proto::{Cell, Json, Look, Row, Tone, json};
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The slots the core has words for, besides `word:look` and words written out joined by `|`.
pub const KINDS: [&str; 8] = ["doer", "model", "setting", "value", "page", "chat", "file", "pack"];
/// The most candidates sent.
pub const MOST: usize = 50;
/// The most entries of the project read for its file names, how deep, and how long the names are kept before they are read again.
const FILES_MOST: usize = 5_000;
const DEPTH: usize = 10;
const FRESH: Duration = Duration::from_secs(5);
/// Folders never read for names: the repository's own, J4n's, and what a build makes.
const SKIP: [&str; 4] = [".git", ".j4n", "target", "node_modules"];
/// What a file adds to the score of a match (the plan's source weight), the newest answer's step, a phrase's wording, and the most the
/// project's lines add by how often and how lately they were typed.
const FILE: i64 = 10;
const STEP: i64 = 25;
const WORDING: i64 = 20;
const LINES: f64 = 30.0;
/// A start of the typed words (the plan's match weights): as typed, or with a typo in the last word.
const START: i64 = 300;
const TYPO: i64 = 200;
/// How long a typed line takes to count half as much, and the most events read from the Log on one key.
const HALF_MS: f64 = 7.0 * 86_400_000.0;
const READ: usize = 50_000;
/// The most steps taken from an answer, and the words a list item that is a step to take may start with.
const STEPS: usize = 5;
const VERBS: [&str; 40] = [
    "add", "ask", "build", "change", "check", "commit", "confirm", "create", "delete", "deploy", "document", "fix", "install", "keep", "make",
    "merge", "move", "open", "push", "read", "release", "remove", "rename", "rerun", "restart", "review", "revert", "run", "send", "set", "split",
    "start", "stop", "tag", "test", "try", "update", "use", "verify", "write",
];

/// The Decider's ask point, how long the find stands still before it is asked, how close the best two of a slot must be for it, the most
/// candidates it is sent, what its confidence adds (× [`SURE`]), how many orders the cache keeps, and a day of the Log's clock.
pub const POINT: &str = "complete";
pub const PAUSE_MS: u64 = 600;
const CLOSE: i64 = 20;
const SENT: usize = 8;
const SURE: f64 = 60.0;
const CACHED: usize = 512;
const DAY_MS: u64 = 86_400_000;
/// How many failed asks in a row stop the asks of the completion, and for how long: its own count, apart from the answerer's breaker.
pub const FAILS: u32 = 3;
pub const REST_MS: u64 = 600_000;
/// What a Decider is told the asker is doing, and how the notice of the day's cap starts.
const GOAL: &str = "which of the candidates the operator most likely means to type: the line typed so far is first, then the candidates, the heads of the last turns of the chat and the slot";
const CAPPED: &str = "suggestions by code only today";

/// Whether a command's `complete` may name `slot`.
pub fn known(slot: &str) -> bool {
    KINDS.contains(&slot) || slot == "word:look" || (slot.contains('|') && slot.split('|').all(|w| !w.trim().is_empty() && !w.contains(char::is_whitespace)))
}

/// What the completion keeps between keys: the project's file names as last read, kept a moment so that typing does not read the folders
/// on every key, and the Log as read so far.
#[derive(Default)]
pub struct Kept {
    read: Option<(Instant, Vec<String>)>,
    said: Said,
    rank: Rank,
}

/// The Decider's side of a screen: the pause that waits, the one ask in flight, and the orders it gave.
#[derive(Default)]
struct Rank {
    wait: Option<Wait>,
    /// The candidates of the last pause that was asked or logged, and of the last window: a window sent again for the same is no new
    /// pause, and one that comes back to them after another is.
    last: Option<u64>,
    seen: Option<u64>,
    /// A line was sent and no window has come since: the find that stands is the sent line's, and no pause is taken for it.
    hush: bool,
    busy: Arc<AtomicBool>,
    flying: bool,
    cache: Arc<Mutex<Cache>>,
}

/// A pause that waits: the candidates' key, when the find was last asked (the Log's clock), and what the Decider would be sent.
struct Wait {
    key: u64,
    at: u64,
    slot: String,
    draft: String,
    cands: Vec<String>,
}

/// Clears the flag of the ask in flight when its thread ends, however it ends.
struct Done(Arc<AtomicBool>);

impl Drop for Done {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// The orders the Decider gave, by their candidates' key, the oldest given up first past [`CACHED`].
#[derive(Default)]
struct Cache {
    map: HashMap<u64, Pick>,
    order: VecDeque<u64>,
}

/// What the Decider said of a set of candidates: the one it put first and how sure it was (none when it was not sure), and who answered.
#[derive(Clone)]
struct Pick {
    top: Option<String>,
    conf: f64,
    by: String,
}

impl Cache {
    fn put(&mut self, key: u64, pick: Pick) {
        if self.map.insert(key, pick).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > CACHED {
            self.order.pop_front().map(|k| self.map.remove(&k));
        }
    }
}

impl Kept {
    /// A line was sent: the pause that waited for its find is dropped, as the screen sends no window for an empty line.
    pub fn sent(&mut self) {
        (self.rank.wait, self.rank.hush) = (None, true);
    }

    /// The screen asked for a window: its find is the line as typed now.
    pub fn asked(&mut self) {
        self.rank.hush = false;
    }
}

/// The Log, read on from where it was left: each line the operator typed (how often, each time counting half as much a week later, as of
/// when it was last typed), the newest whole answer of each thread, and the tasks whose words are no answer to the operator (the quick
/// look, a split), as the chat leaves them out.
#[derive(Default)]
struct Said {
    next: u64,
    lines: HashMap<String, (f64, u64)>,
    answers: HashMap<String, String>,
    unsaid: std::collections::HashSet<String>,
    /// The Decider's asks of a day (its day, how many), and the day the notice of its cap was said.
    asked: (u64, u64),
    told: Option<u64>,
    /// Asks that failed in a row, and until when none is made after [`FAILS`] of them.
    fails: u32,
    rest: u64,
}

fn decay(f: f64, ms: u64) -> f64 {
    f * 0.5f64.powf(ms as f64 / HALF_MS)
}

impl Said {
    fn catch(&mut self, log: &Log) {
        if self.next > log.head() {
            return;
        }
        for ev in log.read(self.next).take(READ) {
            self.next = ev.seq + 1;
            let text = ev.data.get("text").and_then(Json::as_str).unwrap_or_default().trim();
            match ev.kind.as_str() {
                "chat.say" if ev.by == Actor::Operator && kept_line(text) => {
                    let (f, at) = self.lines.get(text).copied().unwrap_or((0.0, ev.at_ms));
                    self.lines.insert(text.to_string(), (decay(f, ev.at_ms.saturating_sub(at)) + 1.0, ev.at_ms));
                }
                "task.queued" if matches!(ev.data.pointer("/spec/role").and_then(Json::as_str), Some("look" | "split")) => {
                    self.unsaid.extend(ev.task.clone());
                }
                "run.message" if ev.data.get("partial").and_then(Json::as_bool) != Some(true) && !text.is_empty() && !ev.task.as_ref().is_some_and(|t| self.unsaid.contains(t)) => {
                    if let Some(t) = &ev.thread {
                        self.answers.insert(t.clone(), text.to_string());
                    }
                }
                "complete.ranked" if ev.data.get("cached").and_then(Json::as_bool) == Some(false) => {
                    let day = ev.at_ms / DAY_MS;
                    self.asked = if self.asked.0 == day { (day, self.asked.1 + 1) } else { (day, 1) };
                    self.fails = if ev.data.get("failed").and_then(Json::as_bool) == Some(true) { self.fails + 1 } else { 0 };
                    if self.fails >= FAILS {
                        (self.fails, self.rest) = (0, ev.at_ms + REST_MS);
                    }
                }
                "chat.note" if ev.by == Actor::Core && text.starts_with(CAPPED) => self.told = Some(ev.at_ms / DAY_MS),
                _ => {}
            }
        }
    }

    /// What a line adds by how often and how lately it was typed: up to [`LINES`].
    fn weight(&self, line: &str, now: u64) -> i64 {
        let (f, at) = self.lines.get(line).copied().unwrap_or_default();
        let f = decay(f, now.saturating_sub(at));
        (LINES * f / (f + 2.0)).round() as i64
    }
}

/// Whether a typed line may come back as a sentence: plain words (no command, so no `:key` line, and no chat's name), nothing the redactor
/// took out of it, not one long word (a key may have been sent as text), three characters at the least.
fn kept_line(t: &str) -> bool {
    t.chars().count() >= 3 && !t.starts_with([':', '/', '@']) && !t.contains('«') && (t.contains(char::is_whitespace) || t.chars().count() < 20)
}

/// The steps the newest answer names: the items of a list under a line that says `next`, `then` or `steps` (or on that line, after its
/// `:`, as `1. … 2. …`), and the items of its other lists that start with a verb; at most [`STEPS`], each cut at its first sentence.
pub fn steps(answer: &str) -> Vec<String> {
    let lead = |t: &str| t.split(|c: char| !c.is_alphanumeric()).any(|w| matches!(w.to_lowercase().as_str(), "next" | "then" | "steps"));
    let (mut out, mut under) = (Vec::new(), false);
    for line in answer.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(item) = item_of(line) {
            if under || item.split_whitespace().next().is_some_and(|w| VERBS.contains(&w.to_lowercase().as_str())) {
                out.push(item.to_string());
            }
            continue;
        }
        under = lead(line);
        // a lead and its list on one line: `Proposed next steps: 1. Deploy the fix. 2. Add a test.`
        if let Some((head, tail)) = line.split_once(':').filter(|(h, _)| lead(h)) {
            under = lead(head);
            let mut rest = tail.trim();
            while let Some(item) = item_of(rest) {
                let numbered = |r: &str| r.starts_with(|c: char| c.is_ascii_digit()) && item_of(r).is_some();
                let end = (1..item.len()).filter(|i| item.is_char_boundary(*i)).find(|i| item[..*i].ends_with(char::is_whitespace) && numbered(&item[*i..]));
                out.push(item[..end.unwrap_or(item.len())].to_string());
                rest = end.map_or("", |i| &item[i..]);
            }
        }
    }
    let mut kept: Vec<String> = Vec::new();
    for s in out.iter().filter_map(|i| first_sentence(i)) {
        if !kept.contains(&s) && kept.len() < STEPS {
            kept.push(s);
        }
    }
    kept
}

/// The text of a list item: after `-`, `*`, `•`, or a number and `.` or `)`, and a blank.
fn item_of(line: &str) -> Option<&str> {
    let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let rest = if digits > 0 { line[digits..].strip_prefix(['.', ')'])? } else { line.strip_prefix(['-', '*', '•'])? };
    rest.starts_with(char::is_whitespace).then(|| rest.trim_start()).filter(|r| !r.is_empty())
}

/// An item as a line to type: its first sentence, without the marks of emphasis or its last stop, its first letter small unless the word is
/// all capitals.
fn first_sentence(item: &str) -> Option<String> {
    let plain = item.replace("**", "").replace('`', "");
    let end = plain.char_indices().find(|(i, c)| matches!(c, '.' | '!' | '?') && plain[i + 1..].starts_with(char::is_whitespace)).map_or(plain.len(), |(i, _)| i);
    let s = plain[..end].trim().trim_end_matches(['.', '!', '?', ':', ';', ',']).trim();
    let mut chars = s.chars();
    let first = chars.next()?;
    let caps = s.split_whitespace().next().is_some_and(|w| w.chars().filter(|c| c.is_alphabetic()).all(char::is_uppercase) && w.chars().count() > 1);
    Some(if caps { s.to_string() } else { first.to_lowercase().chain(chars).collect() })
}

/// How well the words typed start `text`, in whatever case: each word as it is there and the last the start of its word (300), or that
/// start a typo away, in a word of four letters or more, as the phrase table allows (200). Nothing typed starts every line.
fn starts(typed: &str, text: &str) -> Option<i64> {
    let words = |s: &str| s.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>();
    let (t, x) = (words(typed), words(text));
    let Some((last, head)) = t.split_last() else { return Some(START) };
    if t.len() > x.len() || head.iter().zip(&x).any(|(a, b)| a != b) {
        return None;
    }
    let word = &x[head.len()];
    if typed.ends_with(char::is_whitespace) {
        return (word == last && x.len() > t.len()).then_some(START);
    }
    if word.starts_with(last.as_str()) {
        return Some(START);
    }
    let n = last.chars().count();
    (n >= 4 && (n - 1..=n + 1).any(|k| super::phrase::near(last, &word.chars().take(k).collect::<String>()).is_some())).then_some(TYPO)
}

impl Kept {
    fn names(&mut self, root: &Path, gate: Option<&Gate>) -> Vec<String> {
        if self.read.as_ref().is_some_and(|(at, _)| at.elapsed() > FRESH) {
            self.read = None;
        }
        // with no guard to ask, no file name is offered
        self.read.get_or_insert_with(|| (Instant::now(), gate.map(|g| walk(root, g)).unwrap_or_default())).1.clone()
    }
}

impl Front {
    /// The rows of `complete:` for `find`: `~for`, then at most `most` candidates, best first.
    pub fn complete(&self, thread: &str, find: &str, most: usize, kept: &mut Kept) -> Vec<Row> {
        let (slot, rest) = find.split_once(' ').unwrap_or((find, ""));
        let (slot, token, before) = if slot == "line" { self.slot_of(rest) } else { (slot.to_string(), rest.to_string(), Vec::new()) };
        // `complete.on` off: the screen is told so in `~for`, and is sent no word
        let on = !self.screen().setting_is("complete.on", &toml::Value::Boolean(false));
        let mut found: Vec<(i64, String, String)> = match slot.as_str() {
            _ if !on => Vec::new(),
            "sentence" | "next" => self.sentences(thread, &token, kept),
            _ => self.words_for(thread, &slot, &token, &before, kept).into_iter().filter_map(|(text, detail, plus)| fit(&token, &text).map(|s| (s + plus, text, detail))).collect(),
        };
        // a stable sort: ties go to the shorter, then to the one listed first
        let order = |found: &mut Vec<(i64, String, String)>| found.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.chars().count().cmp(&b.1.chars().count())));
        order(&mut found);
        // the Decider's pick for these candidates, when it gave one: what it adds, and whether it stands first
        let lift = self.ranked(&slot, &token, &found, kept).and_then(|p| Some((p.top?, (p.conf * SURE).round() as i64)));
        if let Some((top, plus)) = &lift {
            found.iter_mut().filter(|f| f.1 == *top).for_each(|f| f.0 += plus);
            order(&mut found);
        }
        let head = Row { key: "~for".into(), cells: vec![Cell::new(find, Tone::Plain), Cell::new(&slot, Tone::Dim), Cell::new(&token, Tone::Dim)], data: Some(json!({"find": find, "slot": slot, "token": token, "on": on})), ..Row::default() };
        let rows = found.into_iter().take(most).enumerate().map(|(i, (score, text, detail))| {
            let mut data = json!({"slot": slot, "text": text, "score": score});
            if let Some((_, plus)) = lift.as_ref().filter(|(t, _)| *t == text) {
                data["decider"] = json!(plus);
                data["made"] = json!(i == 0);
            }
            Row { key: format!("{slot}:{text}"), cells: vec![Cell::new(&text, Tone::Plain), Cell::new(detail, Tone::Dim)], data: Some(data), ..Row::default() }
        });
        std::iter::once(head).chain(rows).collect()
    }

    /// Whether the Decider may be asked at all: `complete.on` and `complete.decider` on, and a Decider in the point's chain that is set up
    /// and has passed the point's cases. None of it is said: code's order stands.
    fn decider_ready(&self) -> bool {
        let screen = self.screen();
        let off = |id: &str| screen.setting_is(id, &toml::Value::Boolean(false));
        let (asks, now) = (self.asks(), self.0.log.clock().now_ms());
        !off("complete.on") && !off("complete.decider") && asks.points.get(POINT).is_some_and(|p| {
            p.chain.iter().filter(|s| s.kind.is_model()).any(|s| asks.registry.get(&s.id()).is_some_and(|a| a.kind() == s.kind && asks.state.state_for(&s.id(), POINT, &a.model(), now) == State::Active))
        })
    }

    /// The Decider's pick for the candidates of `found`, from the cache; and when the candidates may be asked of it, the pause that waits
    /// for them (a newer find takes the place of the one that waits). Only a whole line, the next line, or a slot whose best two are close.
    fn ranked(&self, slot: &str, token: &str, found: &[(i64, String, String)], kept: &mut Kept) -> Option<Pick> {
        let close = matches!(slot, "sentence" | "next") || found.get(1).is_some_and(|b| found[0].0 - b.0 <= CLOSE);
        let scrub = |s: &str| self.0.log.redactor().scrub(s);
        // a candidate that held what the redactor takes out is never sent; a line that did is not asked about at all
        let cands: Vec<String> = found.iter().map(|f| f.1.clone()).filter(|t| scrub(t) == *t).take(SENT).collect();
        let r = &mut kept.rank;
        if r.hush || cands.len() < 2 || !close || scrub(token) != token || !self.decider_ready() {
            // the window is the newest: what waited for an older one is dropped
            (r.wait, r.seen) = (None, None);
            return None;
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (slot, token, &cands).hash(&mut h);
        let key = h.finish();
        if r.seen.replace(key) != Some(key) && r.last == Some(key) {
            r.last = None;
        }
        if r.last == Some(key) {
            r.wait = None;
        } else if r.wait.as_ref().is_none_or(|w| w.key != key) {
            r.wait = Some(Wait { key, at: self.0.log.clock().now_ms(), slot: slot.into(), draft: token.into(), cands });
        }
        r.cache.lock().unwrap_or_else(|e| e.into_inner()).map.get(&key).cloned()
    }

    /// The clock of the completion, on the screen's thread: the pause that waited long enough is asked of the Decider on a thread of its
    /// own (or, when its order is kept, logged), unless one is in flight or the day's cap is reached, which is said once a day in `thread`
    /// with the key to the setting. True when an ask came back since the last call: its window is to be sent again.
    pub fn complete_tick(&self, thread: &str, kept: &mut Kept) -> bool {
        let r = &mut kept.rank;
        let busy = r.busy.load(Ordering::SeqCst);
        let landed = r.flying && !busy;
        r.flying &= busy;
        let now = self.0.log.clock().now_ms();
        if busy || r.wait.as_ref().is_none_or(|w| now < w.at + PAUSE_MS) {
            return landed;
        }
        let Some(w) = r.wait.take() else { return landed };
        r.last = Some(w.key);
        let pick = r.cache.lock().unwrap_or_else(|e| e.into_inner()).map.get(&w.key).cloned();
        if let Some(p) = pick {
            self.ranked_said(&w, &p, (0, Some(0.0)), None);
            return landed;
        }
        kept.said.catch(&self.0.log);
        let day = now / DAY_MS;
        let cap = self.screen().set.pointer("/complete/decider_day").to_json().as_f64().unwrap_or(300.0).max(0.0) as u64;
        let used = if kept.said.asked.0 == day { kept.said.asked.1 } else { 0 };
        // after FAILS failed asks in a row the completion rests, by code's order alone; the answerer is not touched
        if now < kept.said.rest {
            return landed;
        }
        if used >= cap {
            if kept.said.told != Some(day) {
                kept.said.told = Some(day);
                let text = format!("{CAPPED}: the Decider was asked {used} times today, as complete.decider_day allows · ⏎ opens Settings › All");
                self.note_to(thread, &text, "dim", super::Lead::open("open All", "config"));
            }
            return landed;
        }
        let r = &mut kept.rank;
        r.busy.store(true, Ordering::SeqCst);
        r.flying = true;
        let (front, busy, cache, heads) = (self.clone(), r.busy.clone(), r.cache.clone(), self.heads(thread, 3));
        std::thread::spawn(move || {
            let _done = Done(busy);
            // the order is kept before it is said, so the window sent again for the event finds it
            let (pick, ms, usd, failed) = front.complete_ask(&w, &heads);
            // a failure is not kept: the same candidates are asked again after the rest
            if !failed {
                cache.lock().unwrap_or_else(|e| e.into_inner()).put(w.key, pick.clone());
            }
            front.ranked_said(&w, &pick, (ms, usd), Some(failed));
        });
        landed
    }

    /// One question to the point `complete` for the candidates that waited: the one put first, how long the walk took, what it cost and
    /// whether a step of it failed.
    fn complete_ask(&self, w: &Wait, heads: &[String]) -> (Pick, u64, Option<f64>, bool) {
        let scrub = |s: &str| self.0.log.redactor().scrub(s);
        let list: Vec<String> = w.cands.iter().enumerate().map(|(i, c)| format!("{}: {c}", i + 1)).collect();
        let text = format!("{}\ncandidates: {}\nchat: {}\nslot: {}", scrub(&w.draft), scrub(&list.join("; ")), heads.join("; "), w.slot);
        let options = w.cands.iter().enumerate().map(|(i, c)| Opt { id: (i + 1).to_string(), label: scrub(c), recommended: false }).collect();
        let mut q = self.0.who.question(POINT, None, &text, options);
        q.goal = GOAL.into();
        let Some(asked) = self.0.who.ask(POINT, None, &q, None) else { return (Pick { top: None, conf: 0.0, by: "front".into() }, 0, Some(0.0), false) };
        let pick = match &asked.walk {
            Walk::Answered(o) => match &o.answer {
                Answer::Pick { id } => Pick { top: id.parse::<usize>().ok().and_then(|n| w.cands.get(n.wrapping_sub(1))).cloned(), conf: f64::from(o.conf.unwrap_or(0.0)), by: o.by.clone() },
                _ => Pick { top: None, conf: 0.0, by: o.by.clone() },
            },
            Walk::Fallback(o) => Pick { top: None, conf: 0.0, by: o.by.clone() },
            _ => Pick { top: None, conf: 0.0, by: "none".into() },
        };
        let failed = asked.notes.iter().any(|n| n.kind == "ask.step" && n.data.get("outcome").and_then(Json::as_str) == Some("failed"));
        (pick, asked.ms, asked.spent.usd, failed)
    }

    /// The Log's `complete.ranked` of a pause: the slot, how many candidates, the one put first, who and how sure, what it took and cost,
    /// and whether a step of the ask failed (`None`: the order was the cache's, nothing was asked).
    fn ranked_said(&self, w: &Wait, p: &Pick, (ms, usd): (u64, Option<f64>), failed: Option<bool>) {
        let conf = p.top.as_ref().map(|_| (p.conf * 100.0).round() / 100.0);
        let data = json!({ "slot": w.slot, "n": w.cands.len(), "top": p.top, "by": p.by, "conf": conf, "ms": ms, "usd": usd, "cached": failed.is_none(), "failed": failed.unwrap_or(false) });
        let _ = self.0.log.append(New::of("complete.ranked", Actor::Core, data));
    }

    /// The slot the end of `line` is in, its token, and the words between the command's own and it. A model is of the doer named
    /// before it (`model:claude`), a value of the setting named before it. Plain words are a sentence, whose token is the whole line, and
    /// an empty line is the next line.
    fn slot_of(&self, line: &str) -> (String, String, Vec<String>) {
        let mut words: Vec<&str> = line.split_whitespace().collect();
        let token = if line.is_empty() || line.ends_with(char::is_whitespace) { "" } else { words.pop().unwrap_or_default() };
        let Some((first, args)) = words.split_first().filter(|(w, _)| w.starts_with([':', '/'])) else {
            return match () {
                _ if words.is_empty() && token.starts_with([':', '/']) => ("command".into(), token.into(), Vec::new()),
                _ if line.trim().is_empty() => ("next".into(), String::new(), Vec::new()),
                _ => ("sentence".into(), line.trim_start().into(), Vec::new()),
            };
        };
        let slots = self.0.phrases.slots(first);
        let named = |kind: &str, of: &str| slots.iter().position(|s| s == of).and_then(|i| args.get(i)).map_or_else(|| kind.to_string(), |a| format!("{kind}:{a}"));
        let slot = match slots.get(args.len()).map(String::as_str) {
            Some("model") => named("model", "doer"),
            Some("value") => named("value", "setting"),
            Some(s) => s.to_string(),
            None => String::new(),
        };
        (slot, token.into(), args.iter().map(|a| a.to_string()).collect())
    }

    /// Whole lines for the words typed, each with its score: the newest answer's steps, the project's lines and the phrases' wordings
    /// whose first words they are. With nothing typed, the next line: the steps in the answer's order, then the lines by how often and how
    /// lately they were typed. The line as typed, and one that may be a key, never are.
    fn sentences(&self, thread: &str, typed: &str, kept: &mut Kept) -> Vec<(i64, String, String)> {
        let next = typed.trim().is_empty();
        if !next && !kept_line(typed.trim()) {
            return Vec::new();
        }
        let said = &mut kept.said;
        said.catch(&self.0.log);
        let now = self.0.log.clock().now_ms();
        let same = |s: &str| s.trim().eq_ignore_ascii_case(typed.trim());
        let steps = said.answers.get(thread).map(|a| steps(a)).unwrap_or_default();
        let mut out: Vec<(i64, String, String)> = Vec::new();
        for (i, s) in steps.into_iter().enumerate() {
            if let Some(m) = starts(typed, &s) {
                out.push((m + STEP - if next { i as i64 } else { 0 }, s, String::new()));
            }
        }
        for line in said.lines.keys() {
            if let Some(m) = if next { Some(0) } else { starts(typed, line) } {
                out.push((m + said.weight(line, now), line.clone(), String::new()));
            }
        }
        if !next {
            out.extend(self.0.phrases.wordings().into_iter().filter_map(|w| starts(typed, w).map(|m| (m + WORDING, w.to_string(), String::new()))));
        }
        let mut seen: Vec<String> = Vec::new();
        out.retain(|(_, s, _)| {
            let new = !same(s) && !seen.iter().any(|x| x.eq_ignore_ascii_case(s));
            seen.push(s.clone());
            new
        });
        // the lines were read from a map: in the order of their words, so the same Log gives the same list once sorted by score
        out.sort_by(|a, b| a.1.cmp(&b.1));
        out
    }

    /// The words of `slot`, each with what is said beside it and what it adds to its score.
    fn words_for(&self, thread: &str, slot: &str, token: &str, before: &[String], files: &mut Kept) -> Vec<(String, String, i64)> {
        let plain = |t: &str, d: &str| (t.to_string(), d.to_string(), 0);
        let (kind, arg) = slot.split_once(':').unwrap_or((slot, ""));
        let screen = self.screen();
        let setting = |id: &str| screen.tables.get::<SettingRow>(id).ok();
        let now = |id: &str| match screen.set.pointer(&format!("/{}", id.replace('.', "/"))).to_json() {
            serde_json::Value::String(s) => s,
            serde_json::Value::Null => String::new(),
            v => v.to_string(),
        };
        match kind {
            _ if slot.contains('|') => slot.split('|').map(|w| plain(w.trim(), "")).collect(),
            "doer" => self.0.doers.iter().map(|d| plain(d.id(), "")).collect(),
            // the models its connector names, then the lists read for it, as the library shows them: none is read from the provider here
            "model" => {
                let Some(doer) = (if arg.is_empty() { self.binding(thread).map(|b| b.doer) } else { Some(arg.to_string()) }) else { return Vec::new() };
                let cfg = self.cfg();
                let named = screen.tables.rows("connector").into_iter().find_map(|r| match r {
                    Value::Map(m) if m.get("id") == Some(&Value::text(doer.as_str())) => m.get("models").cloned(),
                    _ => None,
                });
                let mut list: Vec<String> = match named {
                    Some(Value::List(l)) => l.into_iter().filter_map(|v| if let Value::Str(s) = v { Some(s) } else { None }).collect(),
                    _ => Vec::new(),
                };
                let listed = cfg.listing.as_ref().map(|c| c.read()).unwrap_or_default().remove(&doer).map(|l| l.models).unwrap_or_default();
                for m in cfg.models.kept(&doer).into_iter().chain(listed) {
                    if !list.contains(&m) {
                        list.push(m);
                    }
                }
                list.iter().map(|m| plain(m, "")).collect()
            }
            "setting" => screen.tables.ids(SettingRow::NAME).into_iter().filter(|id| setting(id).is_some()).map(|id| plain(id, &now(id))).collect(),
            "value" => {
                let Some(row) = setting(arg) else { return Vec::new() };
                let words = row.words.clone().unwrap_or_else(|| if row.ty == Ty::Bool { vec!["true".into(), "false".into()] } else { Vec::new() });
                let held = now(arg);
                words.iter().map(|w| plain(w, if *w == held { "now" } else { "" })).collect()
            }
            "page" => screen.pages.ids().map(|(id, title)| plain(id, title)).collect(),
            "pack" => screen.packs.iter().map(|p| plain(&p.id, if p.on { "on" } else { "off" })).collect(),
            "word" if arg == "look" => {
                let look = self.look_spoken();
                match before.first().and_then(|part| Look::PARTS.iter().find(|(p, _)| p == part)) {
                    Some((part, words)) => super::fonts::said_words(part, words).iter().map(|w| plain(w, if look.word(part).as_deref() == Some(*w) { "now" } else { "" })).collect(),
                    None if before.is_empty() => Look::PARTS.iter().map(|(p, _)| plain(p, &look.word(p).unwrap_or_default())).collect(),
                    None => Vec::new(),
                }
            }
            // the phrases' commands (`:switch`, `:stop`), which the palette does not list, and the palette's commands and flows, each once
            "command" => {
                let palette = screen.palette();
                let said = palette.iter().filter_map(|r| r.cells.first()).flat_map(|c| c.text.split_whitespace().map(str::to_string).collect::<Vec<_>>());
                let mut out: Vec<String> = Vec::new();
                for w in self.0.phrases.docs().iter().map(|(w, _)| w.clone()).chain(said).filter(|w| w.starts_with([':', '/'])) {
                    if !out.contains(&w) {
                        out.push(w);
                    }
                }
                out.iter().map(|w| plain(w, "")).collect()
            }
            // only the project's own files, by their path from its folder
            "file" if token.starts_with(['/', '~']) || token.split('/').any(|s| s == "..") => Vec::new(),
            "file" => {
                let root = Some(self.cfg().root.clone()).filter(|r| !r.as_os_str().is_empty()).or_else(|| std::env::current_dir().ok()).unwrap_or_default();
                files.names(&root, self.who().gate().as_ref()).into_iter().map(|f| (f, String::new(), FILE)).collect()
            }
            // the chats and the sentences are the screen's own
            _ => Vec::new(),
        }
    }
}

/// How well `token` fits `text`, in whatever case: at its start (300), at the start of one of its words (200), its letters in order (100).
fn fit(token: &str, text: &str) -> Option<i64> {
    let (t, x) = (token.to_lowercase(), text.to_lowercase());
    if x.starts_with(&t) {
        return Some(300);
    }
    if x.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(&t)) {
        return Some(200);
    }
    let mut left = x.chars();
    t.chars().all(|c| left.any(|y| y == c)).then_some(100)
}

/// The files under `root`, by their path from it, nearest first and then in order, that the guard lets a model read and whose name is not a
/// secret's. No link is followed, and the folders of [`SKIP`] are not read.
fn walk(root: &Path, gate: &Gate) -> Vec<String> {
    let (mut out, mut left, mut dirs) = (Vec::new(), FILES_MOST, VecDeque::from([(root.to_path_buf(), 0)]));
    while let Some((dir, depth)) = dirs.pop_front() {
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<_> = read.flatten().collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for e in entries {
            if left == 0 {
                return out;
            }
            left -= 1;
            let (Ok(ty), name) = (e.file_type(), e.file_name().to_string_lossy().into_owned()) else { continue };
            if ty.is_dir() && depth < DEPTH && !SKIP.contains(&name.as_str()) {
                dirs.push_back((e.path(), depth + 1));
            } else if ty.is_file() && !crate::redact::secret_file(&name) && readable(gate, root, &e.path())
                && let Ok(rel) = e.path().strip_prefix(root)
            {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out
}

/// Whether the guard lets a model read `path`: a block is never offered.
fn readable(gate: &Gate, root: &Path, path: &Path) -> bool {
    let mut a = Action::new(Verb::Read, Actor::Model("complete".into()), &path.to_string_lossy());
    a.site = Some(root.to_string_lossy().into_owned());
    !matches!(gate.check(&a), Verdict::Block { .. })
}
