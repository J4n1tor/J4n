//! Reflexes: a command J4n runs by itself when a sentence of the operator's holds, behind one
//! switch, `reflexes.on`, off by default. A reflex is a rule (kind `rule`, `then = "ask"`; data: no code per reflex) with `on` (`task.settled` or `chat.say`), `when` (a
//! mechanical condition that decides when the judge is asked at all; a reflex with none is refused), `text` (the operator's sentence), `do` (the command), a `limit` (the
//! least time between two asks) and `off`. The rule engine is the one engine: `when`, `limit`, the row's validity and its words are [`crate::rule`]'s; what is the front's
//! is when it reads them (a settled boundary), the judge, the floors and the commands.
//!
//! - **The check** is `when`, read at a settled boundary only (law 9): at a task's end (`task.settled`, from [`Front::plan_step`]) or when a line is typed (`chat.say`, before
//!   the line is sent), by `Rules::react` with a `rule::State` of its own for each reflex. It sees `ev`, `ctx` (the numbers of `/context`: `held`, `window`, `floor`, `history`,
//!   `grew`, `cold`, `turns`), `fold` and `set`. The loop never reads a rule that asks (`Rules::load` leaves it out), so a boundary is read once and by the front.
//! - **The judge** is the ask point `reflex`: `rule:reflex` (holds when the chat holds no session), then Jev, then the fallback, which is not to fire. It reads a
//!   digest made by code (the numbers, the heads of the last turns, the sentence), redacted, never the transcript, and only with `reflexes.judge` on for the project.
//! - **The Log** answers "why did it compact": a firing is `rule.fired` (`rule` = `reflex:<id>`, `then` = `reflex`, with the judge's reason), a check that held and did not fire is
//!   `reflex.checked` with the reason (the judge's, or the floor that stopped it).
//! - **The floors**: a reflex runs only commands that cannot widen what J4n may do ([`ALLOWED`]) and never answers an approval; a command that throws context
//!   away ([`THROWS`]) asks the operator until the judge's calls have held (the trust record: `reflexes.trust` calls labelled `held`, none wrong); a daily cap
//!   (`reflexes.cap_usd`) stops the judge; a reflex with probe cases (`reflex_case` rows, `:reflex case`) fires only when its judge got every one right (`:probe`).

use super::usage::UNKNOWN_USD;
use super::wait::{Kind as Wait, Pending};
use super::{Front, Said};
use crate::answer::chain::Walk;
use crate::binder::yes_no;
use crate::answer::{Answerer, Kind as AKind, field, fn_rule, picked};
use crate::ask::{Answer, Opt};
use crate::change::Actor;
use crate::log::Ev;
use crate::rule::{Do, RuleEnv, RuleRow};
pub use crate::rule::{ALLOWED, NO_CHECK, SENTENCE, THROWS};
use crate::table::Kind;
use crate::task::context::k;
use crate::text::{clip, line};
use crate::value::Value;
use serde::Deserialize;
use serde_json::json;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

pub const POINT: &str = "reflex";
/// What the digest holds at most.
const TURNS: usize = 3;
const GOAL: usize = 100;
const ANSWER: usize = 140;
/// How long a question to the operator waits for their yes before it lapses.
pub const PENDING_MS: u64 = 120_000;
/// How long a typed line waits for the judge of a reflex before it goes on as typed.
pub const HOLD_MS: u64 = 15_000;
/// What the front is set to do.
#[derive(Clone, Debug)]
pub struct Reflexes {
    /// The switch, `reflexes.on`: off, nothing is checked, nothing is asked and nothing is written.
    pub on: bool,
    /// The project's consent that the digest may go to the judge (`reflexes.judge`).
    pub judge: bool,
    /// The most the judge may cost in a day (`reflexes.cap_usd`).
    pub cap_usd: f64,
    /// How many calls must have held before a command that throws context away stops asking (`reflexes.trust`).
    pub trust: usize,
}

impl Default for Reflexes {
    fn default() -> Reflexes {
        Reflexes { on: false, judge: false, cap_usd: 0.25, trust: 5 }
    }
}

/// An entry of kind `reflex_case`: at this state the reflex should fire, or should not.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseRow {
    pub id: String,
    pub reflex: String,
    pub fire: bool,
    pub state: String,
}

impl Kind for CaseRow {
    const NAME: &'static str = "reflex_case";
}

/// What the digest and the command read of a rule that asks.
fn sentence(t: &RuleRow) -> &str {
    t.text.as_deref().unwrap_or_default()
}

fn does(t: &RuleRow) -> &str {
    t.run.as_deref().unwrap_or_default()
}

fn throws(t: &RuleRow) -> bool {
    t.command().is_some_and(|c| THROWS.contains(&c.name.as_str()))
}

/// A reflex row of the old form (kind `reflex`: `do`, `when` the sentence, `check`, `event`, `on`, `cool`) as the rule it is now: read as one when it is loaded.
fn migrated(old: &toml::Table) -> toml::Table {
    let mut t = toml::Table::new();
    for (from, to) in [("do", "do"), ("doc", "doc"), ("when", "text"), ("check", "when"), ("event", "on")] {
        t.extend(old.get(from).map(|v| (to.to_string(), v.clone())));
    }
    t.entry("on").or_insert("task.settled".into());
    t.insert("then".into(), "ask".into());
    t.insert("off".into(), (!old.get("on").and_then(toml::Value::as_bool).unwrap_or(false)).into());
    t.insert("limit".into(), toml::Value::Table(toml::Table::from_iter([("per".to_string(), "thread".into()), ("max".to_string(), 1.into()), ("within".to_string(), old.get("cool").cloned().unwrap_or("10m".into()))])));
    t
}

/// A row of the table as the rule it is, read and checked.
fn decode(id: &str, mut t: toml::Table) -> Result<RuleRow, String> {
    t.insert("id".into(), id.into());
    let row: RuleRow = toml::Value::Table(t).try_into().map_err(|e: toml::de::Error| e.message().to_string())?;
    row.compile().map(|_| row).map_err(|e| e.join("; "))
}

/// The numbers of `/context` for one chat, as a check and the digest read them.
struct Facts {
    held: u64,
    window: Option<f64>,
    floor: Option<u64>,
    history: Option<u64>,
    cold: Option<bool>,
    turns: u32,
    who: String,
    grew: Option<f64>,
    grew_tokens: u64,
}

impl Facts {
    fn value(&self) -> Value {
        let mut m: BTreeMap<String, Value> = BTreeMap::new();
        m.insert("held".into(), Value::num(self.held as f64));
        m.insert("turns".into(), Value::num(f64::from(self.turns)));
        m.insert("grew_tokens".into(), Value::num(self.grew_tokens as f64));
        for (key, v) in [("window", self.window), ("grew", self.grew), ("floor", self.floor.map(|f| f as f64)), ("history", self.history.map(|h| h as f64))] {
            if let Some(v) = v {
                m.insert(key.into(), Value::num(v));
            }
        }
        if let Some(c) = self.cold {
            m.insert("cold".into(), Value::Bool(c));
        }
        Value::Map(m)
    }
}

/// A typed line that waits for a reflex's judge: sent once, by whoever takes it out of the map first.
struct Held {
    id: u64,
    line: String,
    keys: Vec<String>,
    /// The operator has been asked what to do with it: their answer (or the question's lapse) sends it, not the bound of the judge's wait.
    asked: bool,
}

/// What the judge said.
struct Judged {
    fire: bool,
    by: String,
    model: String,
    why: String,
    usd: Option<f64>,
    ask: String,
}

/// What the front keeps of the reflexes while it runs.
#[derive(Default)]
pub struct State {
    /// What `:reflex` said since, by id: the row as it stands now (a rule row as a table), or `None` for one removed.
    edits: BTreeMap<String, Option<toml::Table>>,
    cases: Vec<CaseRow>,
    /// What each reflex's rule remembers: its `limit`, the least time between two asks.
    rules: BTreeMap<String, crate::rule::State>,
    /// The size a chat held at the last check that held, by chat and reflex: what `ctx.grew` counts from.
    seen: BTreeMap<(String, String), u64>,
    /// What a reflex's cases passed for: the sentence and cases as they were, and whether the judge got them all right.
    probed: BTreeMap<String, (String, bool)>,
    /// What the calls of the judge in flight are expected to cost (the Log has the rest).
    reserved: f64,
    /// The chats whose settled boundary is being judged now, and the retries being decided (`retry <chat>`).
    busy: BTreeSet<String>,
    /// The keys of `busy` that were asked for again while at work: the thread that finishes runs its work once more (a retry is never lost).
    again: BTreeSet<String>,
    /// What each reflex last said, for the list.
    said: BTreeMap<String, String>,
    /// The line a chat typed that waits for a judge, and what became of the last one released (for a headless run, which waits for it).
    held: BTreeMap<String, Held>,
    released: BTreeMap<String, Said>,
    /// The chats whose held line has left `held` and is being sent: a headless run still waits for it.
    sending: BTreeSet<String>,
    hold_seq: u64,
}

/// `rule:reflex`: the row's own rule, which is the floor of every reflex: a chat that holds no session has nothing to throw away and nothing to carry on.
/// It reads the digest's field `held`.
pub fn reflex_rule() -> impl Answerer {
    fn_rule("rule:reflex", |q| {
        if field(&q.text, "held") == Some("none") { picked("hold", "the chat holds no session: there is nothing to throw away") } else { Err("the digest holds a session: the judge decides".into()) }
    })
}

thread_local! {
    /// Who a command run by a reflex is written as: the reflex. None: the operator, who typed it.
    static DOING: RefCell<Option<Actor>> = const { RefCell::new(None) };
}

/// Who the command being run is by: the reflex that runs it, else the operator.
pub(super) fn doing() -> Actor {
    DOING.with(|d| d.borrow().clone()).unwrap_or(Actor::Operator)
}

/// While it lives the commands this thread runs are the reflex's.
struct Doing;

impl Doing {
    fn as_(by: Actor) -> Doing {
        DOING.with(|d| *d.borrow_mut() = Some(by));
        Doing
    }
}

impl Drop for Doing {
    fn drop(&mut self) {
        DOING.with(|d| *d.borrow_mut() = None);
    }
}

/// Work off the drawing thread, marked busy until it ends, a panic included: a chat's reflexes (key = the chat) and a failure's retry.
struct Busy {
    front: Front,
    key: String,
}

impl Drop for Busy {
    fn drop(&mut self) {
        // a normal end let the key go itself, under the look that found nothing asked for again (a new thread may hold it by now); only a panic is let go here
        if std::thread::panicking() {
            let mut st = self.front.tg();
            st.busy.remove(&self.key);
            st.again.remove(&self.key);
        }
    }
}

/// The cent reserved for one call of the judge ([`Front::reflex_reserve`]), let go when the call ends, a panic included.
struct Reserved(Front);

impl Drop for Reserved {
    fn drop(&mut self) {
        self.0.tg().reserved -= UNKNOWN_USD;
    }
}

/// The end of the judge's work on a held line: the line goes on, unless the operator has been asked what to do with it (their answer sends it).
struct Released {
    front: Front,
    thread: String,
    id: u64,
}

impl Drop for Released {
    fn drop(&mut self) {
        if !self.front.waiting(&self.thread, Wait::Reflex) {
            self.front.reflex_release(&self.thread, Some(self.id), "");
        }
    }
}

impl Front {
    fn tg(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.reflexes.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The reflexes as rows of the table: the rules that ask, the rows of the old form (kind `reflex`) read as the rules they are, and what `:reflex` said since.
    fn reflex_tables(&self) -> BTreeMap<String, toml::Table> {
        let tables = &self.screen().tables;
        let mut rows: BTreeMap<String, toml::Table> = BTreeMap::new();
        for (kind, old) in [("reflex", true), ("rule", false)] {
            for id in tables.ids(kind) {
                let Some(m) = tables.raw(kind, id).filter(|m| old || m.fields.get("then").and_then(toml::Value::as_str) == Some("ask")) else { continue };
                rows.insert(id.to_string(), if old { migrated(&m.fields) } else { m.fields.clone() });
            }
        }
        for (id, edit) in &self.tg().edits {
            match edit {
                Some(t) => rows.insert(id.clone(), t.clone()),
                None => rows.remove(id),
            };
        }
        rows
    }

    /// The reflexes there are now, and what each that is no reflex was refused for (by the rule engine: one validator).
    pub fn reflex_rows(&self) -> (Vec<RuleRow>, Vec<String>) {
        let (mut ok, mut problems) = (Vec::new(), Vec::new());
        for (id, t) in self.reflex_tables() {
            match decode(&id, t) {
                Ok(r) => ok.push(r),
                Err(why) => problems.push(format!("reflex {id} is refused: {why}")),
            }
        }
        (ok, problems)
    }

    fn reflex_cases(&self, id: &str) -> Vec<CaseRow> {
        let tables = &self.screen().tables;
        let mut cases: Vec<CaseRow> = tables.ids(CaseRow::NAME).into_iter().filter_map(|c| tables.get::<CaseRow>(c).ok()).filter(|c| c.reflex == id).collect();
        cases.extend(self.tg().cases.iter().filter(|c| c.reflex == id).cloned());
        cases.sort_by(|a, b| a.id.cmp(&b.id));
        cases
    }

    /// The chat is at a settled boundary: nothing of it runs, and nothing waits for the operator's answer.
    fn reflex_settled(&self, thread: &str) -> bool {
        self.0.tasks.book().open().iter().all(|r| r.spec.thread.as_deref() != Some(thread)) && self.0.tasks.book().open_asks().iter().all(|a| a.thread != thread)
    }

    /// The numbers of `/context` for the chat's live session, with how far it has grown since the check of this reflex last held.
    fn reflex_facts(&self, thread: &str, id: &str) -> Option<Facts> {
        let now = self.log().clock().now_ms();
        let doer = self.binding(thread).map(|b| b.doer);
        let s = self.live_sessions().into_iter().filter(|s| s.thread == thread && doer.as_ref().is_none_or(|d| *d == s.doer)).max_by_key(|s| s.last_ms)?;
        let mut st = self.tg();
        let key = (thread.to_string(), id.to_string());
        let before = *st.seen.entry(key.clone()).or_insert(s.held);
        if s.held < before {
            st.seen.insert(key, s.held);
        }
        let grew_tokens = s.held.saturating_sub(before);
        drop(st);
        Some(Facts {
            held: s.held,
            window: s.window,
            floor: s.floor,
            history: s.history(),
            cold: s.cold(now, self.0.tasks.plans().cache_ms(&s.doer)),
            turns: s.turns,
            who: s.who.clone(),
            grew: s.window.filter(|w| *w > 0.0).and_then(|w| if s.held > 0 { Some(grew_tokens as f64 / (s.held as f64 / w)) } else { None }),
            grew_tokens,
        })
    }

    /// The digest the judge reads: made by code from the numbers, the heads of the last turns and the sentence, and redacted. Never the transcript.
    fn reflex_digest(&self, thread: &str, t: &RuleRow, f: &Facts, said: Option<&str>) -> String {
        let scrub = |s: &str| self.0.log.redactor().scrub(s);
        let pct = |x: f64| format!("{:.1}%", x * 100.0);
        let mut out = vec![format!("reflex: {}", t.id), format!("when: {}", line(&scrub(sentence(t)), SENTENCE))];
        out.push(if f.held == 0 {
            "held: none".to_string()
        } else {
            let of = f.window.map(|w| format!(" · {} of the window", pct(w))).unwrap_or_default();
            format!("held: {}{of} ({})", k(f.held), line(&scrub(&f.who), 60))
        });
        if let (Some(floor), Some(history)) = (f.floor, f.history) {
            out.push(format!("floor {} + history {}", k(floor), k(history)));
        }
        if let Some(g) = f.grew {
            out.push(format!("grew {} of the window since the last check", pct(g)));
        }
        out.push(format!("next line: {}", match f.cold { Some(true) => "cold", Some(false) => "warm", None => "not known" }));
        let turns = self.heads(thread, TURNS);
        if !turns.is_empty() {
            out.push(format!("last turns:\n{}", turns.join("\n")));
        }
        if let Some(l) = said {
            out.push(format!("the line about to be sent: {}", line(&scrub(l), GOAL)));
        }
        out.join("\n")
    }

    /// The heads of the last `n` turns of `thread` since its cut, oldest first: `- you: <goal> → <answer>`, each cut short and redacted.
    pub(super) fn heads(&self, thread: &str, n: usize) -> Vec<String> {
        let scrub = |s: &str| self.0.log.redactor().scrub(s);
        let cut = self.0.binds.cut(thread);
        let rows = self.0.tasks.book().rows();
        let mut turns: Vec<String> = rows
            .iter()
            .rev()
            .take_while(|r| Some(&r.id) != cut.as_ref())
            .filter(|r| r.spec.thread.as_deref() == Some(thread) && r.spec.parent.is_none() && r.spec.doer != crate::task::HUMAN && !r.spec.shrinks_only())
            .filter_map(|r| Some(format!("- you: {} → {}", line(&scrub(&r.spec.goal), GOAL), line(&scrub(r.answer.as_deref().filter(|a| !a.trim().is_empty())?), ANSWER))))
            .take(n)
            .collect();
        turns.reverse();
        turns
    }

    /// What the judge may still cost today, in dollars: the cap less what the Log says its calls cost and what the calls in flight will.
    fn reflex_left(&self) -> f64 {
        self.0.cfg().reflexes.cap_usd - self.spent_today(POINT) - self.tg().reserved
    }

    /// Reserve what a judge call is expected to cost, under the lock, so that calls of several chats and `:probe` cannot all pass the cap together.
    fn reflex_reserve(&self) -> bool {
        let mut st = self.tg();
        let room = self.0.cfg().reflexes.cap_usd - self.spent_today(POINT) - st.reserved > 0.0;
        st.reserved += if room { UNKNOWN_USD } else { 0.0 };
        room
    }

    fn reflex_cap_said(&self) -> String {
        format!("the day's cap (reflexes.cap_usd {:.2}) is spent: the judge is not asked until tomorrow", self.0.cfg().reflexes.cap_usd)
    }

    /// [`reflex_judge`](Front::reflex_judge) with the cap held: `Err` is why it was not asked.
    fn reflex_ask(&self, thread: &str, digest: &str) -> Result<Judged, String> {
        if !self.reflex_reserve() {
            return Err(self.reflex_cap_said());
        }
        let _cent = Reserved(self.clone());
        Ok(self.reflex_judge(thread, digest))
    }

    /// Put the digest to the ask point `reflex`. Only a machine's `fire` is one: whatever else stands is `hold`, and says why.
    fn reflex_judge(&self, thread: &str, digest: &str) -> Judged {
        let options = vec![Opt { id: "fire".into(), label: "fire".into(), recommended: false }, Opt { id: "hold".into(), label: "hold".into(), recommended: true }];
        let q = self.0.who.question(POINT, Some(thread), digest, options);
        let held = |by: &str, why: String| Judged { fire: false, by: by.into(), model: String::new(), why, usd: Some(0.0), ask: q.ask.clone() };
        let Some(asked) = self.0.who.ask(POINT, Some(thread), &q, None) else { return held("front", format!("there is no ask point `{POINT}`")) };
        let (Walk::Answered(o) | Walk::Fallback(o)) = asked.walk else { return held("front", "nobody answered: the fallback is not to fire".into()) };
        let machine = matches!(o.kind, AKind::Decider | AKind::Model) && !o.by.starts_with("fallback");
        let fire = machine && matches!(&o.answer, Answer::Pick { id } if id == "fire");
        let model = self.0.asks.registry.get(&o.by).map(|a| a.model()).unwrap_or_default();
        let why = if machine || o.by.starts_with("rule:") { o.why.clone() } else { format!("no decider answered ({}): the fallback is not to fire", o.why) };
        Judged { fire, by: o.by, model, why, usd: asked.spent.usd, ask: q.ask.clone() }
    }

    /// Put every case of the reflex to its judge. `Ok` when it got them all right; `Err(Ok(why))` when it missed (recorded: it does not fire); `Err(Err(why))` when the
    /// day's cap stopped the cases before any was missed: not proven, and nothing is recorded, so the reflex is not blocked as if its judge had failed.
    fn reflex_prove(&self, thread: &str, t: &RuleRow, cases: &[CaseRow]) -> Result<(), Result<String, String>> {
        let sig = format!("{}\n{}", sentence(t), cases.iter().map(|c| format!("{}:{}:{}", c.id, c.fire, c.state)).collect::<Vec<_>>().join("\n"));
        let mut missed = Vec::new();
        let (mut usd, mut capped) = (0.0, false);
        for c in cases {
            let digest = format!("reflex: {}\nwhen: {}\nstate: {}", t.id, line(&self.0.log.redactor().scrub(sentence(t)), SENTENCE), line(&self.0.log.redactor().scrub(&c.state), SENTENCE));
            let Ok(j) = self.reflex_ask(thread, &digest) else {
                capped = true;
                break;
            };
            usd += j.usd.unwrap_or(UNKNOWN_USD);
            if j.by.starts_with("fallback") || j.by == "front" {
                missed.push(format!("{}: the judge did not answer ({})", c.id, j.why));
            } else if j.fire != c.fire {
                missed.push(format!("{}: it should {} and the judge said {}", c.id, if c.fire { "fire" } else { "hold" }, if j.fire { "fire" } else { "hold" }));
            }
        }
        if capped && missed.is_empty() {
            return Err(Err(format!("not proven: {}", self.reflex_cap_said())));
        }
        let passed = missed.is_empty();
        self.tg().probed.insert(t.id.clone(), (sig, passed));
        self.write(thread, "reflex.probed", Actor::Core, json!({ "reflex": t.id, "passed": passed, "cases": cases.len(), "missed": missed, "usd": usd }));
        if passed { Ok(()) } else { Err(Ok(format!("its judge missed {}: {}", missed.len(), missed.join("; ")))) }
    }

    /// The check of every reflex that is on for `event`, at a settled boundary of the chat; the reflexes that ran or were put to the operator. Nothing is
    /// written, and nothing is asked, with `reflexes.on` off. `said` is the line about to be sent, for `chat.say`.
    pub fn reflexes_at(&self, thread: &str, event: &str, said: Option<&str>) -> Vec<String> {
        if !self.0.cfg().reflexes.on || !self.reflex_settled(thread) || self.waiting(thread, Wait::Reflex) {
            return Vec::new();
        }
        if event == "task.settled" && self.last_task(thread).is_some_and(|r| r.spec.shrinks_only()) {
            return Vec::new();
        }
        let (rows, _) = self.reflex_rows();
        for t in rows.iter().filter(|t| !t.off && t.on == event) {
            let Some(facts) = self.reflex_gate(thread, t, event, said) else { continue };
            if self.reflex_may(thread, t) && self.reflex_decide(thread, t, said, &facts, None) {
                return vec![t.id.clone()];
            }
        }
        Vec::new()
    }

    /// `task.settled`, called at every task's end: the check is made off the caller's thread (the judge may take its time), one at a time for a chat.
    pub fn reflex_step(&self, thread: &str) {
        if !self.0.cfg().reflexes.on {
            return;
        }
        let thread = thread.to_string();
        self.off_thread(thread.clone(), false, move |front| drop(front.reflexes_at(&thread, "task.settled", None)));
    }

    /// `work` on a thread of its own, marked in `busy` under `key` until it ends (a panic included), one at a time for a key: `false` when `key` is already at work.
    /// With `rerun` that is not lost: the thread at work runs `work` once more when it is done.
    pub(super) fn off_thread(&self, key: String, rerun: bool, work: impl Fn(Front) + Send + 'static) -> bool {
        {
            let mut st = self.tg();
            if !st.busy.insert(key.clone()) {
                if rerun {
                    st.again.insert(key);
                }
                return false;
            }
        }
        let front = self.clone();
        std::thread::spawn(move || {
            let _busy = Busy { front: front.clone(), key: key.clone() };
            loop {
                work(front.clone());
                // asked for again while at work: once more; else the key is let go in the same look, so a request cannot come between
                let mut st = front.tg();
                if !st.again.remove(&key) {
                    st.busy.remove(&key);
                    return;
                }
            }
        });
        true
    }

    /// A run does not end while a judge or a decider asked off its thread still thinks: wait for none, at most [`HOLD_MS`]. `true`: none is at work.
    pub fn judges_wait(&self) -> bool {
        let from = self.log().clock().now_ms();
        loop {
            if self.tg().busy.is_empty() {
                return true;
            }
            if self.log().clock().now_ms().saturating_sub(from) > HOLD_MS {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }

    /// A line is about to be sent: when a reflex on `chat.say` has its check hold for it (a line that would start cold), the line waits for the judge,
    /// at most [`HOLD_MS`], and then goes where the judge said (the command runs first) or on as typed. A check that does not hold, the switch off, no consent for the judge,
    /// the day's cap spent, or a chat that is not at a boundary hold nothing: the line goes on at once. The screen says the hold while it lasts.
    pub(super) fn reflex_hold(&self, thread: &str, text: &str, keys: &[String]) -> Option<Said> {
        if !self.0.cfg().reflexes.on || !self.reflex_settled(thread) {
            return None;
        }
        // a line typed while another waits does not pass it: the one that waits goes first, as typed
        if self.tg().held.contains_key(thread) {
            self.reflex_release(thread, None, "your next line came before the judge answered");
            return None;
        }
        if self.waiting(thread, Wait::Reflex) {
            return None;
        }
        let (rows, _) = self.reflex_rows();
        for t in rows.iter().filter(|t| !t.off && t.on == "chat.say") {
            let Some(facts) = self.reflex_gate(thread, t, "chat.say", Some(text)) else { continue };
            if !self.reflex_may(thread, t) {
                return None;
            }
            let (id, at_ms) = {
                let mut st = self.tg();
                st.hold_seq += 1;
                let (id, at_ms) = (st.hold_seq, self.log().clock().now_ms());
                st.held.insert(thread.to_string(), Held { id, line: text.to_string(), keys: keys.to_vec(), asked: false });
                st.released.remove(thread);
                (id, at_ms)
            };
            self.note(thread, &format!("reflex {}: holding your line while the judge looks at it (at most {} s) — it goes to a new session, or on as typed", t.id, HOLD_MS / 1000), "accent");
            let (front, row, line) = (self.clone(), t.clone(), text.to_string());
            let own = thread.to_string();
            std::thread::spawn(move || {
                let _gone = Released { front: front.clone(), thread: own.clone(), id };
                front.reflex_decide(&own, &row, Some(&line), &facts, Some(id));
            });
            let (front, own) = (self.clone(), thread.to_string());
            std::thread::spawn(move || front.reflex_watch(&own, id, at_ms));
            return Some(Said::Held);
        }
        None
    }

    /// The bound of a hold: the judge's wait ([`HOLD_MS`]), and once the operator is asked their answer's ([`PENDING_MS`]). Past it the line goes on as typed. A question
    /// that is gone with the line still held is being answered: the answer sends the line.
    fn reflex_watch(&self, thread: &str, id: u64, at_ms: u64) {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(25));
            let now = self.log().clock().now_ms();
            // one look and one removal under one lock, so an answer cannot come between them
            let lapsed = {
                let Some(asked) = self.tg().held.get(thread).filter(|h| h.id == id).map(|h| h.asked) else { return };
                match (asked, self.wait_lapse(thread, Wait::Reflex, now)) {
                    (true, None) => return,
                    (true, Some(true)) => Some("the question lapsed"),
                    (false, _) if now.saturating_sub(at_ms) > HOLD_MS => Some("the judge did not answer in time"),
                    _ => None,
                }
            };
            if let Some(why) = lapsed {
                self.reflex_release(thread, Some(id), why);
                return;
            }
        }
    }

    /// The held line of the chat goes on as typed (by `id`, if it is still that hold's): whoever takes it out of the map sends it, so it is sent once. It is marked as
    /// being sent from the moment it leaves the map, so a headless run waiting for it never sees the gap; what came of it is written to the Log.
    fn reflex_release(&self, thread: &str, id: Option<u64>, why: &str) {
        let taken = {
            let mut st = self.tg();
            if id.is_some_and(|id| st.held.get(thread).is_none_or(|h| h.id != id)) {
                return;
            }
            let taken = st.held.remove(thread);
            if taken.is_some() {
                st.sending.insert(thread.to_string());
            }
            taken
        };
        let Some(h) = taken else { return };
        if !why.is_empty() {
            self.note(thread, &format!("{why} · your line goes on as typed"), "dim");
        }
        // a panic in sending is no reason to lose the line's place, or to abort a thread that is already unwinding
        let said = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.send_line(thread, &h.line, &h.keys))).unwrap_or_else(|_| Said::Refused("sending the line failed".into()));
        if let Said::Refused(w) = &said {
            self.note(thread, &format!("your held line was not taken: {w}"), "warn");
        }
        self.write(thread, "reflex.released", Actor::Core, json!({ "said": format!("{said:?}"), "why": why }));
        let mut st = self.tg();
        st.released.insert(thread.to_string(), said);
        st.sending.remove(thread);
    }

    /// `:stop` and `:kill` end what the chat's reflexes hold: the line that waits for a judge or for the operator's yes is dropped, with its question. Whether there was one.
    pub(super) fn reflex_drop(&self, thread: &str) -> bool {
        let asked = self.wait_take(thread, Wait::Reflex);
        let line = {
            let mut st = self.tg();
            let line = st.held.remove(thread);
            if line.is_some() {
                st.released.insert(thread.to_string(), Said::Command("stop".into()));
            }
            line
        };
        if line.is_none() && asked.is_none() {
            return false;
        }
        let what = match (&line, &asked) {
            (Some(_), Some(_)) => "the line that waited and the question about it are dropped",
            (Some(_), None) => "the line that waited for the judge is dropped",
            _ => "the question of a reflex is dropped",
        };
        self.write(thread, "reflex.dropped", Actor::Operator, json!({ "line": line.is_some(), "question": asked.is_some() }));
        self.note(thread, &format!("stopped: {what}, and nothing was sent"), "dim");
        true
    }

    /// A headless run: the line of the chat waits here until it has gone on, and says what came of it.
    pub fn held_wait(&self, thread: &str) -> Said {
        loop {
            {
                let mut st = self.tg();
                if !st.held.contains_key(thread) && !st.sending.contains(thread) {
                    return st.released.remove(thread).unwrap_or(Said::Nothing);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }

    fn checked(&self, thread: &str, t: &RuleRow, by: &str, why: &str, usd: Option<f64>, ask: &str) {
        self.tg().said.insert(t.id.clone(), why.to_string());
        self.write(thread, "reflex.checked", Actor::Core, json!({ "reflex": t.id, "fire": false, "by": by, "why": why, "event": t.on, "check": t.when, "usd": usd, "ask": ask }));
    }

    /// The check, which is the reflex's rule read by the rule engine (its `when` and its `limit`): the numbers of the chat when it holds (and the moment is kept for `ctx.grew`).
    fn reflex_gate(&self, thread: &str, t: &RuleRow, event: &str, said: Option<&str>) -> Option<Facts> {
        let now = self.log().clock().now_ms();
        let facts = self.reflex_facts(thread, &t.id)?;
        let rules = t.rules().ok()?;
        let ev = Ev { seq: 0, at_ms: now, kind: event.into(), task: None, thread: Some(thread.into()), by: Actor::Core, cause: None, class: Default::default(), data: json!({ "text": said }) };
        let screen_ = self.screen();
        let env = RuleEnv::new(&ev, self.0.tasks.folds(), &screen_.set, now).with_ctx(facts.value());
        let acts = rules.react(self.tg().rules.entry(t.id.clone()).or_default(), &ev, &env, now);
        if let Some(Do::Dropped(why)) = acts.first().map(|a| &a.what) {
            self.checked(thread, t, "front", why, Some(0.0), "");
        }
        if !acts.iter().any(|a| matches!(a.what, Do::Ask { .. })) {
            return None;
        }
        self.tg().seen.insert((thread.to_string(), t.id.clone()), facts.held);
        Some(facts)
    }

    /// The consent and the day's cap, which stop the judge before it is asked (and say so).
    fn reflex_may(&self, thread: &str, t: &RuleRow) -> bool {
        if !self.0.cfg().reflexes.judge {
            self.checked(thread, t, "front", "the judge may not see this project's text: the setting `reflexes.judge` is off, so nothing was sent", Some(0.0), "");
            return false;
        }
        if self.reflex_left() <= 0.0 {
            self.checked(thread, t, "front", &self.reflex_cap_said(), Some(0.0), "");
            return false;
        }
        true
    }

    /// The cases, the judge and what follows. `hold`: the line of this hold waits for it, and nothing is done once the line has gone on without it.
    fn reflex_decide(&self, thread: &str, t: &RuleRow, said: Option<&str>, facts: &Facts, hold: Option<u64>) -> bool {
        let now = self.log().clock().now_ms();
        let cases = self.reflex_cases(&t.id);
        if !cases.is_empty() {
            let sig = format!("{}\n{}", sentence(t), cases.iter().map(|c| format!("{}:{}:{}", c.id, c.fire, c.state)).collect::<Vec<_>>().join("\n"));
            let known = self.tg().probed.get(&t.id).filter(|(s, _)| *s == sig).map(|(_, ok)| *ok);
            let proof = match known {
                Some(true) => Ok(()),
                Some(false) => Err(Ok("its judge failed its cases and they have not changed: `:probe` runs them again".to_string())),
                None => self.reflex_prove(thread, t, &cases),
            };
            if let Err(why) = proof.map_err(|e| e.unwrap_or_else(|capped| capped)) {
                self.checked(thread, t, "front", &why, None, "");
                return false;
            }
            if self.reflex_left() <= 0.0 {
                self.checked(thread, t, "front", "the day's cap (reflexes.cap_usd) was spent on its cases: the judge is not asked until tomorrow", None, "");
                return false;
            }
        }
        let digest = self.reflex_digest(thread, t, facts, said);
        let j = match self.reflex_ask(thread, &digest) {
            Ok(j) => j,
            Err(why) => {
                self.checked(thread, t, "front", &why, Some(0.0), "");
                return false;
            }
        };
        if !j.fire {
            self.checked(thread, t, &j.by, &j.why, j.usd, &j.ask);
            return false;
        }
        if self.reflex_left() < 0.0 {
            self.checked(thread, t, "front", "the day's cap (reflexes.cap_usd) was spent by this call: it does not fire", j.usd, &j.ask);
            return false;
        }
        // the judge took its time: its line went on as typed (the wait ran out), so there is nothing left for it to decide
        if hold.is_some_and(|id| self.tg().held.get(thread).is_none_or(|h| h.id != id)) {
            self.checked(thread, t, &j.by, "the line no longer waited (it went on as typed, or was stopped) while the judge was thinking, so nothing ran", j.usd, &j.ask);
            return false;
        }
        if throws(t) && !self.reflex_trusted(&j.by) && self.0.who.headless() {
            // a run with nobody at the keyboard has no one to say yes: it asks no one, and nothing is thrown away (the line goes on as typed)
            self.checked(thread, t, &j.by, "the judge is not trusted yet, so the operator would be asked, and a run with nobody at the keyboard asks no one: nothing ran", j.usd, &j.ask);
            return false;
        }
        if throws(t) && !self.reflex_trusted(&j.by) {
            let (n, _) = self.0.asks.state.labelled(&j.by, POINT);
            // the judge took its time: a task may have begun, or another question been put, meanwhile (law 9)
            let p = Pending { id: t.id.clone(), kind: Wait::Reflex, at_ms: now, with: vec![j.ask.clone(), j.by.clone(), j.model.clone()] };
            if !self.reflex_settled(thread) || !self.wait_put(thread, p) {
                self.checked(thread, t, &j.by, "a task began while the judge was thinking, so nothing was asked and nothing ran", j.usd, &j.ask);
                return false;
            }
            if let Some(h) = self.tg().held.get_mut(thread) {
                h.asked = true;
            }
            self.write(thread, "reflex.asked", Actor::Core, json!({ "reflex": t.id, "do": does(t), "by": j.by, "why": j.why, "ask": j.ask, "held": n, "trust": self.0.cfg().reflexes.trust }));
            self.tg().said.insert(t.id.clone(), format!("asked you: {}", j.why));
            let held = if hold.is_some() { " · your line waits: `yes` runs it after, anything else sends it as typed" } else { ", anything else keeps things as they are" };
            self.note(thread, &format!("reflex {}: {} — `yes` runs `{}`{held} (asked until {} of the judge's calls have held; {n} so far)", t.id, j.why, does(t), self.0.cfg().reflexes.trust), "accent");
            return true;
        }
        self.reflex_fire(thread, t, &j.by, &j.why, &j.ask)
    }

    /// The judge's calls at this point that held, none overridden: enough for a command that throws context away to stop asking.
    fn reflex_trusted(&self, by: &str) -> bool {
        let (n, wrong) = self.0.asks.state.labelled(by, POINT);
        n >= self.0.cfg().reflexes.trust && wrong <= 0.0
    }

    /// The firing: `rule.fired` says why, and the command runs.
    fn reflex_fire(&self, thread: &str, t: &RuleRow, by: &str, why: &str, ask: &str) -> bool {
        let Some(cmd) = t.command() else { return false };
        // law 9: the judge took its time, and a command that throws context away never runs in the middle of a turn
        if !self.reflex_settled(thread) {
            self.checked(thread, t, by, "a task began while the judge was thinking, so the command did not run", None, ask);
            return false;
        }
        let rule = format!("reflex:{}", t.id);
        let _by = Doing::as_(Actor::Rule(rule.clone()));
        self.write(thread, "rule.fired", Actor::Rule(rule.clone()), json!({ "rule": rule, "then": "reflex", "do": does(t), "why": why, "by": by, "ask": ask, "event": t.on }));
        self.tg().said.insert(t.id.clone(), format!("ran `{}`: {why}", does(t)));
        self.note(thread, &format!("reflex {}: ran `{}` — {why}", t.id, does(t)), "dim");
        !matches!(self.command(thread, cmd), Said::Refused(_))
    }

    fn label(&self, thread: &str, p: &Pending, what: &str, weight: f64) {
        let [ask, by, model] = &p.with[..] else { return };
        self.write(thread, "label", Actor::Core, json!({ "subject_kind": "answer", "subject": ask, "producer": by, "model": model, "point": POINT, "label": what, "weight": weight }));
    }

    /// The operator's line while a reflex waits for their yes: `yes` runs the command (the call held), `no` keeps things (it was overridden), any other line
    /// is its own and the question goes. `None`: the line is not an answer.
    pub(super) fn reflex_answer(&self, thread: &str, text: &str) -> Option<Said> {
        // `:stop` and `:kill` end the question with the line that waits (`reflex_drop`), and are not an answer to it
        if self.0.phrases.parse(text).is_some_and(|c| matches!(c.name.as_str(), "stop" | "kill")) {
            return None;
        }
        let p = self.wait_take(thread, Wait::Reflex)?;
        let said = Said::Command("reflex".into());
        // the question lapses with time, and with any task or question of the chat that began since: a later `yes` is the doer's
        // a line of theirs that waited for the question goes on first, as typed, unless the answer is a yes
        let gone = |why: &str| self.reflex_release(thread, None, why);
        if p.lapsed(self.log().clock().now_ms()) || !self.reflex_settled(thread) {
            self.note(thread, &format!("reflex {}: the question lapsed, so nothing ran", p.id), "dim");
            gone("");
            return None;
        }
        let yes = yes_no(text);
        if yes == Some(false) {
            self.label(thread, &p, "overridden", 1.0);
            self.note(thread, &format!("reflex {}: kept as it is", p.id), "dim");
            gone("");
            return Some(said);
        }
        if yes.is_none() {
            self.note(thread, &format!("reflex {}: not answered, so nothing ran", p.id), "dim");
            gone("");
            return None;
        }
        let (rows, _) = self.reflex_rows();
        let Some(row) = rows.into_iter().find(|r| r.id == p.id && !r.off).filter(|_| self.0.cfg().reflexes.on) else {
            self.note(thread, &format!("reflex {} is off now, so nothing ran", p.id), "dim");
            gone("");
            return Some(said);
        };
        self.label(thread, &p, "held", 0.0);
        self.reflex_fire(thread, &row, &p.with[1], "you said yes", &p.with[0]);
        gone("");
        Some(said)
    }

    /// `:probe` for reflexes: every reflex's cases put to the judge now (one reflex by its id, or all with `reflexes`), and what came of it said. `true`: the argument was a reflex's, and
    /// the rest of `:probe` has nothing to do.
    pub(super) fn reflex_probe(&self, thread: &str, only: Option<&str>) -> bool {
        let (rows, _) = self.reflex_rows();
        let named = only.is_some_and(|o| o == "reflexes" || rows.iter().any(|r| r.id == o));
        if !self.0.cfg().reflexes.on || (only.is_some() && !named) {
            return false;
        }
        let targets: Vec<RuleRow> = rows.into_iter().filter(|r| only.is_none() || only == Some("reflexes") || only == Some(r.id.as_str())).collect();
        let (front, thread) = (self.clone(), thread.to_string());
        let run = move || {
            for t in &targets {
                let cases = front.reflex_cases(&t.id);
                let said = if cases.is_empty() {
                    format!("{} has no cases: `:reflex case {} fire|hold <state>` adds one", t.id, t.id)
                } else if !front.0.cfg().reflexes.judge {
                    format!("{}: the judge may not see this project's text (reflexes.judge is off): its cases were not put", t.id)
                } else {
                    match front.reflex_prove(&thread, t, &cases) {
                        Ok(()) => format!("{}: its judge got all {} cases right", t.id, cases.len()),
                        Err(Ok(why)) => format!("{}: {why} — it does not fire until it passes", t.id),
                        Err(Err(why)) => format!("{}: {why}", t.id),
                    }
                };
                front.note(&thread, &said, "dim");
            }
        };
        std::thread::spawn(run);
        only.is_some()
    }

    /// `:reflex …` (the commands of the pack `reflexes`).
    pub fn reflex_said(&self, thread: &str, args: &[String]) -> Said {
        let said = Said::Command("reflex".into());
        let word = args.first().map(|w| w.to_lowercase()).unwrap_or_default();
        let id = args.get(1).cloned().unwrap_or_default();
        match word.as_str() {
            "" => {
                let (rows, problems) = self.reflex_rows();
                let mut lines = Vec::new();
                if !self.0.cfg().reflexes.on {
                    lines.push("the reflexes are off: the setting `reflexes.on` (and `reflexes.judge`, for this project) switches them".to_string());
                }
                let st = self.tg();
                for r in &rows {
                    lines.push(format!("{} {} · `{}` when {} · {}", r.id, if r.off { "off" } else { "on" }, does(r), sentence(r), st.said.get(&r.id).map_or("not asked yet".to_string(), |s| clip(s, 120))));
                }
                lines.extend(problems);
                if rows.is_empty() {
                    lines.push("no reflex yet: `:reflex add <id> do <command> when <your sentence> check <condition>`".into());
                }
                drop(st);
                self.note(thread, &lines.join("\n"), "dim");
            }
            "add" => match self.reflex_add(&args[1..]) {
                Ok((row, kept)) => {
                let off = if self.0.cfg().reflexes.on { "" } else { " · the setting `reflexes.on` is off: it is kept, and runs when that is on" };
                let judge = if self.0.cfg().reflexes.judge { "" } else { " · `reflexes.judge` is off: the judge is sent nothing for this project" };
                self.note(thread, &format!("reflex {} added, on{kept}{off}{judge} · no probe cases yet: `:reflex case {} fire|hold <state>` adds one, and until it has some nothing checks its judge", row.id, row.id), "dim")
            }
                Err(why) => return self.refuse(thread, why),
            },
            "on" | "off" => {
                let Some(mut t) = self.reflex_tables().remove(&id) else { return self.refuse(thread, format!("there is no reflex `{id}` (`:reflex` lists them)")) };
                t.insert("off".into(), (word == "off").into());
                self.tg().edits.insert(id.clone(), Some(t.clone()));
                let kept = self.reflex_keep(&id, Some(t));
                self.note(thread, &format!("reflex {id} is {word}{kept}"), "dim");
            }
            "remove" => {
                if !self.reflex_tables().contains_key(&id) {
                    return self.refuse(thread, format!("there is no reflex `{id}`"));
                }
                self.tg().edits.insert(id.clone(), None);
                let kept = self.reflex_keep(&id, None);
                self.note(thread, &format!("reflex {id} removed{kept}"), "dim");
            }
            "case" => {
                let (which, words) = (args.get(2).map(|w| w.to_lowercase()).unwrap_or_default(), args.iter().skip(3).cloned().collect::<Vec<_>>().join(" "));
                if !matches!(which.as_str(), "fire" | "hold") || words.trim().is_empty() {
                    return self.refuse(thread, "usage: `:reflex case <id> fire|hold <the state, in words>`".into());
                }
                if !self.reflex_rows().0.iter().any(|r| r.id == id) {
                    return self.refuse(thread, format!("there is no reflex `{id}`"));
                }
                let n = self.reflex_cases(&id).len() + 1;
                let case = CaseRow { id: format!("{id}-case-{n}"), reflex: id.clone(), fire: which == "fire", state: line(&self.0.log.redactor().scrub(&words), SENTENCE) };
                self.tg().cases.push(case.clone());
                let kept = self.reflex_keep_case(&case);
                self.note(thread, &format!("case {n} of {id}: at this state it should {which}{kept}; `:probe {id}` puts it to the judge"), "dim");
            }
            other => return self.refuse(thread, format!("`:reflex {other}`: say `add`, `on`, `off`, `remove`, `case`, or nothing for the list")),
        }
        said
    }

    /// `:reflex add <id> do <command> when <sentence> check <condition> [at <event>]`: a rule that asks (the sentence is its `text`, the condition its `when`), refused when
    /// it does not hold together (and with no condition).
    fn reflex_add(&self, args: &[String]) -> Result<(RuleRow, String), String> {
        const USAGE: &str = "usage: `:reflex add <id> do <command> when <your sentence> check <condition> [at task.settled|chat.say]`";
        let id = args.first().cloned().ok_or(USAGE)?;
        let mut parts: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        let mut at: Option<&str> = None;
        for w in args.iter().skip(1).map(String::as_str) {
            match w {
                "do" | "when" | "check" | "at" if !parts.contains_key(w) => {
                    parts.insert(w, Vec::new());
                    at = Some(w);
                }
                _ => parts.entry(at.ok_or(USAGE)?).or_default().push(w),
            }
        }
        let part = |p: &str, or: &str| parts.get(p).map_or(or.to_string(), |w| w.join(" "));
        let limit = toml::Table::from_iter([("per".to_string(), "thread".into()), ("max".to_string(), 1.into()), ("within".to_string(), "10m".into())]);
        let t = toml::Table::from_iter([
            ("then".to_string(), "ask".into()),
            ("do".to_string(), part("do", "").into()),
            ("text".to_string(), self.0.log.redactor().scrub(&part("when", "")).into()),
            ("when".to_string(), part("check", "").into()),
            ("on".to_string(), part("at", "task.settled").into()),
            ("limit".to_string(), toml::Value::Table(limit)),
        ]);
        let row = decode(&id, t.clone()).map_err(|why| format!("not added: {why}"))?;
        if self.reflex_rows().0.iter().any(|r| r.id == id) {
            return Err(format!("there is a reflex `{id}` already: `:reflex remove {id}` first"));
        }
        self.tg().edits.insert(id.clone(), Some(t.clone()));
        let kept = self.reflex_keep(&id, Some(t));
        Ok((row, kept))
    }

    /// The row written to the operator's own folder for this project as their own edit, applied at once (`:undo` takes it back); `None`
    /// takes it out. The words that say how it went; empty when the front makes no proposals.
    fn reflex_keep(&self, id: &str, row: Option<toml::Table>) -> String {
        self.propose_row("rule", id, row.map_or(crate::proposal::StepOp::Remove, crate::proposal::StepOp::Set))
    }

    fn reflex_keep_case(&self, c: &CaseRow) -> String {
        let mut t = toml::Table::new();
        t.insert("reflex".into(), toml::Value::String(c.reflex.clone()));
        t.insert("fire".into(), toml::Value::Boolean(c.fire));
        t.insert("state".into(), toml::Value::String(c.state.clone()));
        self.propose_row("reflex_case", &c.id, crate::proposal::StepOp::Set(t))
    }

    fn propose_row(&self, kind: &str, id: &str, op: crate::proposal::StepOp) -> String {
        if self.proposer().is_none() {
            return " (kept for this run: nothing here writes files)".into();
        }
        let step = crate::proposal::Step { to: crate::change::Where::User, file: Some("reflexes.toml".into()), kind: kind.into(), id: id.into(), op };
        let draft = crate::proposal::Draft { key: format!("reflex:{kind}:{id}"), title: format!("Reflex: {kind} {id}"), why: "the operator's own command".into(), evidence: 1, steps: vec![step], widens: Vec::new() };
        match self.edit(draft) {
            Ok(crate::proposal::Edited::Done { id, .. }) => format!(" (`:undo {id}` takes it back)"),
            Ok(crate::proposal::Edited::Proposed { id, .. }) => format!(" (it widens what J4n may do: `:accept {id}`)"),
            Err(why) => format!(" (kept for this run only: {why})"),
        }
    }
}
