//! The look of a plain line, after the read (`read`): under the word `orchestrate`, before Main works a plain line, the read says what the line is (`answer`: Main
//! replies, no work; `one`: one lane, Main works it; `split`: parts that could run as chats beside Main), whether it goes on from the last answer, which chat it is about,
//! and which doer it asks for. The Deciders answer what they are sure of; the cheap tier's model, with no tool that writes, is shown the line, the catalogue of what J4n
//! offers and only the questions still open. Code decides what the answer is worth: a split whose chats only read stays one lane unless the files the line names are big,
//! a read of big files that the read called one or an answer is split by code into read chats (scouts), a standing rule is never taken as a reply, and a feature that is off is
//! proposed, never switched on. An answer or a one is Main's own turn; a split goes on to Main's planning turn and the briefs (`split.rs`). The read is written to the Log
//! (`orchestrate.looked`, one for every plain line, with why when it was not made) and said in Main's chat.
//!
//! Code goes first only for what is exact: a command, `@c3`, `@claude`, and a line typed while a task works never reach the read. Where the read cannot run (no Decider is
//! sure, no cheap tier, the doer not ready, a wrong answer, too slow) the line goes to Main as typed and the row says why.

use super::{Front, Lead, chats};
use crate::change::Actor;
use crate::setting::Settings;
use crate::task::Mode;
use serde_json::{Value as Json, json};
use std::time::Duration;

/// What the look may spend, and how long a line waits for it: for its first reply (a slow service holds the request before any word comes), then for
/// its answer from that reply on.
const LOOK_USD: f64 = 0.05;
pub const LOOK_FIRST: Duration = Duration::from_secs(120);
pub const LOOK_WAIT: Duration = Duration::from_secs(45);
/// The events that say the model has begun to answer (not `run.served`, said before the model is asked, nor `run.tokens`, said as 0 by a request asked again).
const REPLY: [&str; 7] = ["run.delta", "run.reasoning_delta", "run.open", "run.message", "run.reasoning", "run.tool_call", "run.metrics"];
/// The most chats a look may name before the rest are ignored (the same as the most that run at once).
const MAX_CHATS: usize = 4;

/// The questions of the read the quick model answers when `orchestrate.read_model` is not set.
pub const READ_MODEL: [&str; 1] = ["kind"];

/// What the quick look is set to (`orchestrate.*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Orchestrate {
    pub look: bool,
    pub read_big: u64,
    /// The doer and model word of the look; empty: the cheap tier's.
    pub doer: String,
    pub model: String,
    /// The most tokens the catalogue may take.
    pub catalogue: usize,
    /// The most chats a split starts, the most that write at once, and which wait for a yes (`writes`, `always`, `never`).
    pub chats: usize,
    pub writers: usize,
    pub ask: String,
    /// The most tokens a digest of a chat may take, and how many of the newest ones Main's Brief holds.
    pub digest: usize,
    pub digests: usize,
    /// The doers the tool `chat` is offered to (the start sets them, as the librarian's are).
    pub scouts: Vec<String>,
    /// The questions of the read the quick model may answer (`kind`, `goes_on`, `chat`, `asked`): the ones cleared for it. A question left out is settled by a Decider
    /// or not at all, and the line goes as typed.
    pub read_model: Vec<String>,
    /// A lane Main worked itself is reviewed on another doer, and concerns go back to Main (`orchestrate.review`, on).
    pub review: bool,
    /// The dearest model a chat is briefed on, as a model word priced by `offload.prices` (`orchestrate.chat_top`; empty: no ceiling).
    pub chat_top: String,
    /// Main's word acts on a chat's stopped part under `--approval full` (`orchestrate.held` `main`); `operator`: it is always the operator's.
    pub held_main: bool,
    /// The chats of a split are given read-only copies of the other parts' files and the folder's (`orchestrate.access`).
    pub access: bool,
    /// When the read is unsure of the way of working, the operator is asked (`orchestrate.way` `ask`), or the read's way is taken (`decide`).
    pub way_ask: bool,
}

impl Default for Orchestrate {
    fn default() -> Orchestrate {
        Orchestrate { look: false, read_big: 200_000, doer: String::new(), model: String::new(), catalogue: 1500, chats: 3, writers: 2, ask: "writes".into(), digest: 400, digests: 3, scouts: Vec::new(), read_model: READ_MODEL.iter().map(|w| w.to_string()).collect(), review: true, chat_top: String::new(), held_main: true, access: false, way_ask: true }
    }
}

impl Orchestrate {
    pub fn from(s: &Settings) -> Orchestrate {
        let d = Orchestrate::default();
        Orchestrate {
            look: s.flag("orchestrate.look", d.look),
            read_big: s.num("orchestrate.read_big", d.read_big as f64).max(0.0) as u64,
            doer: s.word("orchestrate.look_doer", "").trim().to_string(),
            model: s.word("orchestrate.look_model", "").trim().to_string(),
            catalogue: s.num("orchestrate.catalogue", d.catalogue as f64).max(1.0) as usize,
            chats: s.num("orchestrate.chats", d.chats as f64).max(1.0) as usize,
            writers: s.num("orchestrate.writers", d.writers as f64).max(0.0) as usize,
            digest: s.num("orchestrate.digest", d.digest as f64).max(50.0) as usize,
            digests: s.num("orchestrate.digests", d.digests as f64).max(0.0) as usize,
            ask: Some(s.word("orchestrate.ask", "writes")).filter(|w| ["writes", "always", "never"].contains(w)).unwrap_or("writes").to_string(),
            scouts: Vec::new(),
            read_model: s.word("orchestrate.read_model", &READ_MODEL.join(" ")).split_whitespace().map(String::from).collect(),
            review: s.flag("orchestrate.review", d.review),
            chat_top: s.word("orchestrate.chat_top", "").trim().to_string(),
            held_main: s.word("orchestrate.held", "main") != "operator",
            access: s.flag("orchestrate.access", d.access),
            way_ask: s.word("orchestrate.way", "ask") != "decide",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Answer,
    One,
    Split,
}

impl Decision {
    pub fn word(self) -> &'static str {
        match self {
            Decision::Answer => "answer",
            Decision::One => "one",
            Decision::Split => "split",
        }
    }

    pub fn from_word(w: &str) -> Option<Decision> {
        [Decision::Answer, Decision::One, Decision::Split].into_iter().find(|d| d.word() == w)
    }
}

/// A chat the look would start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookChat {
    pub goal: String,
    pub writes: bool,
    pub doer: String,
    pub model: String,
}

/// What a look answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Looked {
    pub decision: Decision,
    pub chats: Vec<LookChat>,
    pub features: Vec<String>,
    pub why: String,
}

/// The answer of the look, from the text the model sent: the first JSON object in it (a fence around it is no harm). A decision that is none of the three
/// is an error, not a guess; a chat or a feature that is not well formed is left out.
pub fn parse(text: &str) -> Result<Looked, String> {
    let (from, to) = (text.find('{'), text.rfind('}'));
    let (Some(from), Some(to)) = (from, to) else { return Err("the answer holds no JSON object".into()) };
    if from >= to {
        return Err("the answer holds no JSON object".into());
    }
    let v: Json = serde_json::from_str(&text[from..=to]).map_err(|e| format!("the answer is not JSON: {e}"))?;
    let word = v.get("decision").and_then(Json::as_str).unwrap_or_default();
    let decision = Decision::from_word(word).ok_or_else(|| format!("`{word}` is not answer, one or split"))?;
    let chats = chats_of(&v);
    let features = v.get("features").and_then(Json::as_array).map(|a| a.iter().filter_map(Json::as_str).map(String::from).collect()).unwrap_or_default();
    Ok(Looked { decision, chats, features, why: crate::text::line(v.get("why").and_then(Json::as_str).unwrap_or_default(), 300) })
}

/// The chats an answer names, at most [`MAX_CHATS`]; one that is not an object is left out.
pub(super) fn chats_of(v: &Json) -> Vec<LookChat> {
    let text_of = |c: &Json, k: &str| c.get(k).and_then(Json::as_str).unwrap_or_default().to_string();
    v.get("chats").and_then(Json::as_array).map(|a| a.iter().filter(|c| c.is_object()).take(MAX_CHATS).map(|c| LookChat { goal: text_of(c, "goal"), writes: c.get("writes").and_then(Json::as_bool).unwrap_or(true), doer: text_of(c, "doer"), model: text_of(c, "model") }).collect()).unwrap_or_default()
}

/// A line that states a rule for later (`from now on …`, `whenever …`), which is never a question to answer.
pub fn standing_rule(text: &str) -> bool {
    let low = text.trim().to_lowercase();
    ["from now on", "whenever ", "every time ", "each time ", "always ", "never again", "from here on"].iter().any(|p| low.starts_with(p) || low.contains(&format!(", {p}")) || low.contains(&format!(". {p}")))
}

/// Why a split the read named no parts for is one lane: Main's planning turn would start from nothing the read saw.
pub const NO_PARTS: &str = "the read said split and named no parts";

/// What the line is taken as, with the look's answer and what code knows: `big` is whether the files the line names hold more than `orchestrate.read_big`.
pub fn taken(looked: &Looked, line: &str, big: bool) -> (Decision, &'static str) {
    match looked.decision {
        Decision::Split if looked.chats.is_empty() => (Decision::One, NO_PARTS),
        Decision::Answer if standing_rule(line) => (Decision::One, "a standing rule is never answered as read: Main takes it and a feature is proposed"),
        Decision::Split if !looked.chats.is_empty() && looked.chats.iter().all(|c| !c.writes) && !big => (Decision::One, "the chats would only read, and Main reads files this small itself"),
        d => (d, ""),
    }
}

/// Whether the line asks for a change to files (`fix`, `rename`, `write the summary to …`): work that writes stays as the look said, whatever the files weigh. The words are few
/// and plain; a line that carries one of them and only reads is left to the look, which costs wall time and never correctness.
pub fn changes_files(line: &str) -> bool {
    const CHANGES: [&str; 26] = [
        "fix", "change", "edit", "rename", "refactor", "delete", "remove", "rewrite", "replace", "migrate", "implement", "update", "patch", "add", "create", "commit", "merge", "move", "format", "apply", "install", "deploy", "revert", "overwrite", "append", "save",
    ];
    let low = line.to_lowercase();
    let words: Vec<&str> = low.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    words.iter().any(|w| CHANGES.contains(w)) || words.iter().enumerate().any(|(i, w)| *w == "write" && words[i + 1..].iter().take(4).any(|n| ["to", "into", "over", "back"].contains(n)))
}

/// The cut of the files a line names into the reads of at most `cap` chats: in the order named, a chat takes paths until the next would take it over `read_big`;
/// when that is more chats than `cap`, the paths (largest first) go to the chat holding the least.
fn cut(named: &[(String, u64)], read_big: u64, cap: usize) -> Vec<Vec<(String, u64)>> {
    let mut groups: Vec<Vec<(String, u64)>> = Vec::new();
    for n in named.iter().filter(|n| n.1 > 0) {
        match groups.last_mut() {
            Some(g) if g.iter().map(|x| x.1).sum::<u64>() + n.1 <= read_big => g.push(n.clone()),
            _ => groups.push(vec![n.clone()]),
        }
    }
    if groups.len() <= cap.max(1) {
        return groups;
    }
    let mut all: Vec<(String, u64)> = groups.into_iter().flatten().collect();
    all.sort_by_key(|n| std::cmp::Reverse(n.1));
    let mut out: Vec<Vec<(String, u64)>> = vec![Vec::new(); cap.max(1)];
    for n in all {
        if let Some(g) = out.iter_mut().min_by_key(|g| g.iter().map(|x| x.1).sum::<u64>()) {
            g.push(n);
        }
    }
    out
}

/// A read of big files that the look called `one` or an answer: the work only reads (no word of a change, no standing rule) and the files the line names hold more
/// than `read_big`, which ORCHESTRATE-MODE's reads rule sends to scouts. Code proposes the split (read chats, at most `cap`, the paths cut by size), and the reason; the
/// clamps and the approval after it are those of any split. `None` leaves the look's answer as it was.
pub fn scouted(looked: &Looked, line: &str, named: &[(String, u64)], read_big: u64, cap: usize) -> Option<(Looked, String)> {
    let total: u64 = named.iter().map(|n| n.1).sum();
    if !matches!(looked.decision, Decision::Answer | Decision::One) || total <= read_big || standing_rule(line) || changes_files(line) {
        return None;
    }
    let chats = cut(named, read_big, cap)
        .into_iter()
        .map(|g| {
            let (paths, bytes): (Vec<&str>, u64) = (g.iter().map(|n| n.0.as_str()).collect(), g.iter().map(|n| n.1).sum());
            LookChat { goal: format!("read {} ({} kB) and report what the line asks of them: {}", paths.join(", "), bytes / 1000, crate::text::line(line, 120)), writes: false, doer: String::new(), model: String::new() }
        })
        .collect();
    Some((Looked { decision: Decision::Split, chats, features: looked.features.clone(), why: looked.why.clone() }, format!("named {} kB > read_big: code split the read", total / 1000)))
}

/// The bytes under `root/path`: a file's size, or a folder's files in all (`left` counts the entries still to be walked); a path that is not there holds nothing.
fn weigh(root: &std::path::Path, path: &str, left: &mut usize) -> u64 {
    fn walk(p: &std::path::Path, left: &mut usize) -> u64 {
        let Ok(meta) = std::fs::symlink_metadata(p) else { return 0 };
        if meta.is_file() {
            return meta.len();
        }
        if !meta.is_dir() || *left == 0 {
            return 0;
        }
        std::fs::read_dir(p).map(|d| d.flatten().map(|e| { *left = left.saturating_sub(1); walk(&e.path(), left) }).sum()).unwrap_or(0)
    }
    if path.starts_with('/') || path.contains("..") { 0 } else { walk(&root.join(path), left) }
}

/// The paths a line names inside `root`, each with the bytes it holds (a folder is counted to a few thousand files in all); a name that is not there holds nothing.
pub fn named_sizes(root: &std::path::Path, line: &str) -> Vec<(String, u64)> {
    let mut left = 5000;
    chats::paths(line).into_iter().map(|p| { let n = weigh(root, &p, &mut left); (p, n) }).collect()
}

/// The bytes the paths of a brief hold, counted as `named_sizes` counts.
pub fn bytes_of(root: &std::path::Path, paths: &[String]) -> u64 {
    let mut left = 5000;
    paths.iter().map(|p| weigh(root, p, &mut left)).sum()
}

/// The bytes the paths a line names hold, files and folders under `root`.
pub fn named_bytes(root: &std::path::Path, line: &str) -> u64 {
    named_sizes(root, line).iter().map(|n| n.1).sum()
}

impl Front {
    /// The look's answer with what code makes of it: the answer as it is to be worked (the look's chats, or the read chats code cut), the word the look said, what the line
    /// is taken as and why code changed it, if it did.
    fn judged(&self, looked: Looked, line: &str) -> (Looked, &'static str, Decision, String) {
        let (root, o) = (self.0.cfg().root.clone(), self.0.cfg().orchestrate.clone());
        let named = named_sizes(&root, line);
        let said = looked.decision.word();
        if let Some((cut, why)) = scouted(&looked, line, &named, o.read_big, o.chats.min(MAX_CHATS)) {
            return (cut, said, Decision::Split, why);
        }
        let (decision, why) = taken(&looked, line, named.iter().map(|n| n.1).sum::<u64>() > o.read_big);
        (looked, said, decision, why.to_string())
    }

    /// The read of a plain line typed in Main under the switch (see `read`): `None` for every other line, which is never read. A line typed while a task works steers
    /// it: it is not read, and the Log and the screen say so. A line typed while only J4n's own steps work has ended them before it gets here (`steps`). What the read says of a doer the line asks for is kept for the election of the line's doer.
    pub(super) fn read_plain(&self, thread: &str, text: &str, keys: &[String]) -> Option<super::read::Read> {
        self.0.asked.lock().unwrap_or_else(|e| e.into_inner()).remove(thread);
        if !self.0.cfg().orchestrate.look || !self.main_works() || thread != self.0.cfg().thread || !keys.is_empty() || text.trim().is_empty() || text.starts_with([':', '/', '@']) {
            return None;
        }
        if self.settled(thread).is_none() {
            let why = "a task works: the line steers it";
            self.write(thread, "orchestrate.looked", Actor::Core, json!({ "line": crate::text::clip(text, super::thread::CHAT_LINE), "skipped": why }));
            self.note(thread, &format!("the line was not read: {why}"), "dim");
            return None;
        }
        // marked as the read's: the group view puts the read's own head in its place
        self.write(thread, "chat.note", Actor::Core, json!({ "text": "reading the line …", "tone": "dim", "read": "reading" }));
        let read = self.read_line(thread, text);
        if let Some(a) = read.asked.as_ref().and_then(|a| a.value.clone()) {
            self.0.asked.lock().unwrap_or_else(|e| e.into_inner()).insert(thread.to_string(), a);
        }
        Some(read)
    }

    /// What the read comes to, before Main works the line: the row and the Log say what the line was taken as and by whom, or why it was not read. Returns when
    /// the line is to be sent by the caller.
    pub(super) fn quick_look(&self, thread: &str, text: &str, read: Option<super::read::Read>) -> super::split::After {
        use super::split::After;
        let Some(read) = read else { return After::Send(None) };
        let mut event = json!({ "line": crate::text::clip(text, super::thread::CHAT_LINE), "read": read.json(), "ms": read.ms });
        if let (Some(m), Some(o)) = (read.model.as_object(), event.as_object_mut()) {
            o.extend(m.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        let mut split: Option<Looked> = None;
        // the read is unsure of the way: it said split and named no parts (the way is asked once the row is said)
        let mut unsure: Option<(Looked, String)> = None;
        let note;
        let mut level = "dim";
        if read.kind.is_none() {
            // nothing settled the kind: Main works the line as typed, and the row says which rung was missing
            let why = read.model_error.clone().unwrap_or_else(|| "no Decider was sure and the model did not say".into());
            event["error"] = json!(why);
            note = format!("the line was not read: {why} · the line goes to Main as typed");
            level = "warn";
        } else {
            let (looked, said, decision, why) = self.judged(read.looked(), text);
            let features: Vec<String> = looked.features.iter().filter(|f| self.catalogue().packs.contains(f)).cloned().collect();
            event["decision"] = json!(said);
            event["taken"] = json!(decision.word());
            event["chats"] = json!(looked.chats.len());
            event["features"] = json!(features);
            event["why"] = json!(looked.why);
            if !why.is_empty() {
                event[if decision == Decision::Split && said != "split" { "proposed" } else { "clamped" }] = json!(why);
            }
            self.quick_look_features(thread, &features, standing_rule(text));
            // a line that goes on from what Main said, or asks for a doer, is that session's or that doer's: one turn, never a split
            let goes_on = read.goes_on.as_ref().is_some_and(|g| g.value);
            let asked = read.asked.as_ref().is_some_and(|a| a.value.is_some());
            let seen = if decision == Decision::Split && !goes_on && !asked { format!(" ({} chat{})", looked.chats.len(), if looked.chats.len() == 1 { "" } else { "s" }) } else { String::new() };
            let by = if why.is_empty() { String::new() } else { format!(" ({why})") };
            let cost = event["usd"].as_f64().map(|u| format!(" · ${u:.4}")).unwrap_or_default();
            let (head, who) = self.read_words(&read, decision);
            note = format!("read as {head}{by}{seen} · {who}{cost} · {:.1}s{}", read.ms as f64 / 1000.0, if looked.why.is_empty() { String::new() } else { format!(" · {}", looked.why) });
            if goes_on || asked {
                event["taken"] = json!(if goes_on { "goes_on" } else { "asked" });
            } else if decision == Decision::Split && !looked.chats.is_empty() {
                split = Some(looked);
            } else if why == NO_PARTS {
                unsure = Some((looked, why));
            }
        }
        let read_ok = event.get("taken").is_some();
        self.write(thread, "orchestrate.looked", Actor::Core, event);
        // the group view draws a read that came to something as a head of its own, from the event above: this note says it to every other reader
        self.write(thread, "chat.note", Actor::Core, json!({ "text": note, "tone": level, "read": if read_ok { "read" } else { "" } }));
        if let Some((looked, why)) = unsure {
            return self.way(thread, text, looked, &why);
        }
        match split {
            Some(looked) => self.split(thread, text, &looked, &self.catalogue()),
            None => After::Send(None),
        }
    }

    /// The model step of the read and of the look: a task of the role `look` on the cheap tier (or the pin `over`) in a mode that only reads, waited for, with `goal`
    /// as its whole request. Its answer text comes back; what it cost and served goes into `event`.
    pub(super) fn look_task(&self, thread: &str, goal: String, over: &super::Pin, event: &mut Json) -> Result<String, String> {
        let cfg = self.0.cfg();
        let (o, cheap) = (&cfg.orchestrate, &cfg.cheap);
        let pick = |first: &Option<String>, own: &str, tier: &Option<String>| first.clone().or_else(|| Some(own.to_string()).filter(|w| !w.is_empty())).or_else(|| tier.clone());
        let set = super::Pin { doer: pick(&over.doer, &o.doer, &cheap.doer), model: pick(&over.model, &o.model, &cheap.model) };
        // nothing named: the cheap tier of a ready doer, unless the operator went on without it
        let waived = self.waived(super::needs::ORCHESTRATE);
        self.need_check(thread);
        let pin = if set.doer.is_none() && set.model.is_none() && !waived { self.tier_pin(super::tiers::Tier::Cheap, thread) } else { set };
        if pin.doer.is_none() && pin.model.is_none() {
            return Err(if waived { "no cheap tier is named, and you went on without one (`orchestrate.look_doer`, `plan.cheap_doer`)" } else { "no cheap tier is named (`orchestrate.look_doer`, `plan.cheap_doer`), and no ready doer has a model with a price (`offload.prices`)" }.into());
        }
        // a model named and no doer, on a line that binds nothing yet: the doer is the one that has the cheap tier
        let pin = if pin.doer.is_none() && self.binding(thread).is_none() { super::Pin { doer: self.tier_pin(super::tiers::Tier::Cheap, thread).doer, ..pin } } else { pin };
        let base = match (self.binding(thread), &pin.doer) {
            (Some(b), _) => b,
            (None, Some(d)) if self.knows_doer(d) => self.bind_to(d.clone(), None, "look").0,
            (None, Some(d)) => return Err(format!("the doer `{d}` is not one J4n has")),
            (None, None) => return Err("Main has no doer yet, and the cheap tier names none".into()),
        };
        let b = self.pinned(&pin, &base, "look")?;
        if !self.0.who.ready("scout", &[]).iter().any(|r| r.id == b.doer) {
            return Err(format!("{} is not ready for a read: it is not installed or not signed in", b.doer));
        }
        let mut spec = self.spec(thread, &goal, &b.doer).map_err(|e| e.to_string())?;
        spec.role = Some("look".into());
        spec.tune.mode = Mode::Read;
        (spec.tune.model, spec.tune.endpoint, spec.account) = (b.model.clone(), b.endpoint.clone(), b.account.clone());
        self.login_for(thread, &mut spec);
        spec.limits.usd_soft = Some(LOOK_USD);
        let [first, answer] = cfg.look_wait;
        spec.limits.wall_ms = Some((first + answer).as_millis() as u64);
        // the first reply is waited for apart from the answer: a look that ran out before any word came lost the split of the whole line
        let replies = self.0.log.subscribe("run.*", 64);
        let task = self.0.tasks.spawn(spec, Actor::Core).map_err(|p| p.msg)?;
        event["task"] = json!(task);
        let (book, end, over) = (self.0.tasks.book(), std::time::Instant::now() + first, |r: &crate::task::Row| r.state.is_terminal());
        let replied = loop {
            if book.get(&task).is_some_and(|r| over(&r)) {
                break true;
            }
            let Some(left) = end.checked_duration_since(std::time::Instant::now()) else { break false };
            if replies.recv_timeout(left.min(Duration::from_millis(100))).is_some_and(|e| e.task.as_deref() == Some(task.as_str()) && REPLY.contains(&e.kind.as_str())) {
                break true;
            }
        };
        drop(replies);
        let Some(row) = book.wait(&task, over, if replied { answer } else { Duration::ZERO }) else {
            let _ = self.0.tasks.ctl(&task, crate::task::Ctl::Abort, Actor::Core);
            return Err(if replied { "it did not answer in time".into() } else { format!("no reply came in {}", crate::fmt::duration(first.as_millis() as u64)) });
        };
        event["doer"] = json!(b.doer);
        event["model"] = json!(if row.model.is_empty() { b.model.clone().unwrap_or_default() } else { row.model.clone() });
        event["usd"] = json!(row.spent as f64 / 1e6);
        if !matches!(row.state, crate::task::State::Done { ok: true }) {
            return Err("its task did not end well".into());
        }
        Ok(row.answer.unwrap_or_default())
    }

    /// A feature the look named that is off is proposed, never switched on: the row's `⏎` turns the pack on, which is the operator's act.
    pub(super) fn quick_look_features(&self, thread: &str, features: &[String], rule: bool) {
        let packs = self.screen().packs.clone();
        let wanted: Vec<&str> = features.iter().map(String::as_str).chain(rule.then_some("reflexes").filter(|r| !features.iter().any(|f| f == r))).collect();
        for id in wanted {
            if let Some(p) = packs.iter().find(|p| p.id == id && !p.on && p.note.is_empty()) {
                let lead = Lead::run(&format!("turn on {id}"), "pack", &["on", id]);
                self.note_to(thread, &format!("this line is for the feature `{id}` ({}), which is off · ⏎ turns it on", crate::text::line(&p.use_when, 70)), "accent", lead);
            }
        }
    }

    /// The read alone, for a caller outside J4n (`j4n look`): the line is read with the project's catalogue (the Deciders first, then the model `model` of `doer`, else the
    /// setting's, else the cheap tier's, for what they left open) in a thread of its own, and the answer comes back as JSON with what code takes it as. `forced` puts the
    /// questions of a last turn, of chats and of doers that the thread has no material for. Nothing is sent to Main and no session is carried; the model step
    /// is one read-only task of that thread, which the Log keeps as it keeps every task.
    pub fn look_once(&self, thread: &str, text: &str, doer: Option<&str>, model: Option<&str>, forced: &super::read::Forced) -> Result<Json, Json> {
        let catalogue = self.catalogue();
        let over = super::Pin { doer: doer.map(String::from), model: model.map(String::from) };
        let read = self.read_over(thread, text, &over, forced);
        let mut event = json!({ "tokens": catalogue.tokens, "read": read.json(), "ms": read.ms });
        if let (Some(m), Some(o)) = (read.model.as_object(), event.as_object_mut()) {
            o.extend(m.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        if read.kind.is_none() {
            return Err(json!({ "error": read.model_error.clone().unwrap_or_else(|| "nothing settled what the line is".into()) }));
        }
        let (looked, said, decision, why) = self.judged(read.looked(), text);
        let chats: Vec<Json> = looked.chats.iter().map(|c| json!({ "goal": c.goal, "writes": c.writes, "doer": c.doer, "model": c.model })).collect();
        let features: Vec<&String> = looked.features.iter().filter(|f| catalogue.packs.contains(f)).collect();
        event["decision"] = json!(said);
        event["taken"] = json!(decision.word());
        event["chats"] = json!(chats);
        event["features"] = json!(features);
        event["why"] = json!(looked.why);
        if !why.is_empty() {
            event[if decision == Decision::Split && said != "split" { "proposed" } else { "clamped" }] = json!(why);
        }
        Ok(event)
    }
}
