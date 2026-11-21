//! What a rule does when an event comes: which rules answer it, what each says, and whether it is allowed to say it now.

use super::{DEPTH_MAX, KEYS_MAX, NOTE_CHARS, Per, RATE_MAX, RATE_WINDOW_MS, Rule, Rules, SetVal, TEXT_MAX, Then};
use crate::eval::{Env, glob_match};
use crate::fmt::Fmt;
use crate::log::Ev;
use crate::task::id_number;
use crate::value::Value;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

/// The most events whose depth is remembered.
const DEPTHS_MAX: usize = 16_384;

/// What a rule asks the loop to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Do {
    /// Begin a task, or carry on the task of the event (`follow`), optionally having its session shrunk first.
    Start { text: String, follow: bool, doer: Option<String>, place: Option<String>, detached: bool, compact: bool, keep: Vec<String> },
    Steer(String),
    Notice(String),
    /// An incident matched (kind `incident`): the alert the screen shows and the Log keeps (`guard.alert`).
    Alert { incident: String, like: String, class: String, done: String, source: String, text: String },
    Park,
    Cool { ms: u64 },
    /// Put `text` to the judge of the point `reflex`, and run `run` when it says yes. The front reads it: the loop never carries it out.
    Ask { text: String, run: String },
    /// Write a `label` event.
    Label { is: String, of: String, subject: String, producer: String, model: String, point: String, weight: f64 },
    /// Make a proposal (`proposal.made`): nothing is written to a file until the operator accepts it.
    Propose(crate::proposal::Draft),
    /// Wait `ms` and then ask again whether to act (`due`). The loop sets the timer and tells the state its number.
    Arm { rule: usize, key: String, ms: u64, ev: Box<Ev> },
    /// The rule wanted to act and was not let.
    Dropped(String),
}

/// One thing a rule asked for, with what the loop needs to carry it out and record it.
#[derive(Debug, Clone, PartialEq)]
pub struct Act {
    pub rule: String,
    /// The event it answers.
    pub cause: u64,
    /// How many rules deep the chain it belongs to already is.
    pub depth: u8,
    pub task: Option<String>,
    /// The thread of the event: where what the rule starts or says belongs when it has no task of its own.
    pub thread: Option<String>,
    pub what: Do,
}

/// The timer of a wait whose task waits for an answer: none runs.
const HELD: u64 = u64::MAX;

struct Pending {
    /// The newest event that matched.
    ev: Ev,
    timer: u64,
    /// How long the quiet must last.
    ms: u64,
}

/// What a timer that ended a wait finds.
#[derive(Debug, PartialEq)]
pub enum Due {
    /// The wait was replaced, or the task it was for has ended.
    Stale,
    /// Something matched since: the quiet has this long yet to last.
    Again(u64),
    /// The quiet lasted: the event it was waiting on.
    Fire(Box<Ev>),
}

/// What a running loop remembers of its rules.
#[derive(Default)]
pub struct State {
    fired: HashMap<(usize, String), VecDeque<u64>>,
    last: HashMap<(usize, String), u64>,
    pending: HashMap<(usize, String), Pending>,
    depths: BTreeMap<u64, u8>,
    /// By task number. Kept after the task is over: the events that say it ended are answered after it has.
    task_depths: BTreeMap<u64, u8>,
    rate: VecDeque<u64>,
    /// When the overflow of the rate was last said.
    noted: Option<u64>,
    /// The (rule, task) pairs whose end of a chain has been said: once is enough, or the notice is the storm.
    too_deep: HashSet<(usize, String)>,
}

impl State {
    /// How deep a chain of reactions the event is in.
    pub fn depth(&self, ev: &Ev) -> u8 {
        let by_task = ev.task.as_deref().and_then(id_number).and_then(|n| self.task_depths.get(&n)).copied().unwrap_or(0);
        self.depths.get(&ev.seq).copied().unwrap_or(0).max(by_task)
    }

    /// An event a rule's action wrote.
    pub fn mark(&mut self, seq: u64, depth: u8) {
        self.depths.insert(seq, depth);
        while self.depths.len() > DEPTHS_MAX {
            self.depths.pop_first();
        }
    }

    /// A task a rule began: what it does is a part of the chain.
    pub fn mark_task(&mut self, task: &str, depth: u8) {
        let Some(n) = id_number(task) else { return };
        self.task_depths.insert(n, depth);
        while self.task_depths.len() > DEPTHS_MAX {
            self.task_depths.pop_first();
        }
    }

    /// A match for a wait that is already waiting renews it — its event, and so the time the quiet counts from — and the timer
    /// stays: a stream of events does not set and take back a timer for each. False when nothing waits for it.
    pub fn renew(&mut self, rule: usize, key: &str, ev: &Ev) -> bool {
        self.pending.get_mut(&(rule, key.to_string())).map(|p| p.ev = ev.clone()).is_some()
    }

    /// The task waits for an answer (`task.waiting`): its waits stop, however long the answer takes, and the timers they held are
    /// returned to be taken back. Time spent waiting for the operator is not quiet.
    pub fn hold_waits(&mut self, task: &str) -> Vec<u64> {
        self.pending.iter_mut().filter(|((_, k), p)| k == task && p.timer != HELD).map(|(_, p)| std::mem::replace(&mut p.timer, HELD)).collect()
    }

    /// The task has its answer and goes on (`task.resumed`, at `at_ms`): the quiet of each of its waits counts from then, and the waits that
    /// were held need a timer again, `(rule, key, ms)`. The event a wait answers stays the one that matched.
    pub fn resume_waits(&mut self, task: &str, at_ms: u64) -> Vec<(usize, String, u64)> {
        let mut again = Vec::new();
        for ((rule, k), p) in self.pending.iter_mut().filter(|((_, k), _)| k == task) {
            p.ev.at_ms = at_ms;
            if p.timer == HELD {
                again.push((*rule, k.clone(), p.ms));
            }
        }
        again
    }

    /// The loop has set a timer for a new wait of `ms`.
    pub fn arm(&mut self, rule: usize, key: &str, ev: Ev, timer: u64, ms: u64) {
        self.pending.insert((rule, key.to_string()), Pending { ev, timer, ms });
    }

    /// A task is over: what was waiting for it goes, and the timers of the waits are returned. What it counted stays — the events
    /// that say it ended, or flashed while it ran, are answered after it has ended, and a limit must hold for them too.
    pub fn forget(&mut self, task: &str) -> Vec<u64> {
        let mut timers = Vec::new();
        self.pending.retain(|(_, k), p| {
            let keep = k != task;
            if !keep && p.timer != HELD {
                timers.push(p.timer);
            }
            keep
        });
        timers
    }

    /// A timer that ended a wait has fired. Matches that came meanwhile have moved the end of the quiet, so it is asked how much
    /// of it is left: the loop sets the timer again for that long (`rearm`), or the quiet is over.
    pub fn take_due(&mut self, rule: usize, key: &str, timer: u64, now: u64) -> Due {
        let slot = (rule, key.to_string());
        let Some(p) = self.pending.get(&slot).filter(|p| p.timer == timer) else { return Due::Stale };
        match (p.ev.at_ms.saturating_add(p.ms)).checked_sub(now).filter(|left| *left > 0) {
            Some(left) => Due::Again(left),
            None => self.pending.remove(&slot).map_or(Due::Stale, |p| Due::Fire(Box::new(p.ev))),
        }
    }

    /// A wait that cannot go on (no timer could be set) is given up.
    pub fn drop_wait(&mut self, rule: usize, key: &str) {
        self.pending.remove(&(rule, key.to_string()));
    }

    /// The wait goes on under a new timer.
    pub fn rearm(&mut self, rule: usize, key: &str, timer: u64) {
        if let Some(p) = self.pending.get_mut(&(rule, key.to_string())) {
            p.timer = timer;
        }
    }

    pub fn waiting(&self) -> usize {
        self.pending.len()
    }
}

/// What a file can hold of a value; a time or a sum of money is written as the words that read back.
fn to_toml(v: &Value) -> Option<toml::Value> {
    Some(match v {
        Value::Bool(b) => toml::Value::Boolean(*b),
        Value::Num(n) if n.fract() == 0.0 && n.abs() < 9.0e15 => toml::Value::Integer(*n as i64),
        Value::Num(n) => toml::Value::Float(*n),
        Value::Str(s) => toml::Value::String(clean(s, TEXT_MAX)),
        Value::Dur(ms) => toml::Value::String(crate::fmt::duration(*ms)),
        Value::List(items) => toml::Value::Array(items.iter().map(to_toml).collect::<Option<_>>()?),
        Value::Map(m) => toml::Value::Table(m.iter().map(|(k, v)| to_toml(v).map(|v| (k.clone(), v))).collect::<Option<_>>()?),
        Value::Usd(_) | Value::Missing => return None,
    })
}

fn key_of(per: Per, ev: &Ev) -> String {
    match per {
        Per::Task => ev.task.clone().unwrap_or_default(),
        Per::Thread => ev.thread.clone().or_else(|| ev.task.clone()).unwrap_or_default(),
        Per::All => String::new(),
    }
}

/// Words made safe to be said: no control characters but line ends and tabs, no more than [`TEXT_MAX`] characters.
fn clean(text: &str, max: usize) -> String {
    let t: String = text.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t')).collect();
    let t = t.trim();
    if t.chars().count() <= max { t.to_string() } else { format!("{}…", t.chars().take(max).collect::<String>()) }
}

impl Rules {
    fn act(&self, rule: &Rule, ev: &Ev, depth: u8, what: Do) -> Act {
        Act { rule: rule.id.clone(), cause: ev.seq, depth, task: ev.task.clone(), thread: ev.thread.clone(), what }
    }

    /// The actions the rules ask for in answer to `ev`.
    pub fn react(&self, st: &mut State, ev: &Ev, env: &dyn Env, now: u64) -> Vec<Act> {
        let mut out = Vec::new();
        for (i, rule) in self.list.iter().enumerate().filter(|(_, r)| glob_match(&r.on, &ev.kind)) {
            if rule.when.as_ref().is_some_and(|w| !w.holds(env)) {
                continue;
            }
            let depth = st.depth(ev);
            if depth >= DEPTH_MAX {
                if st.too_deep.len() >= KEYS_MAX {
                    st.too_deep.clear();
                }
                if st.too_deep.insert((i, key_of(Per::Task, ev))) {
                    out.push(self.act(rule, ev, depth, Do::Dropped(format!("it answers a chain of {depth} reactions"))));
                }
                continue;
            }
            if let Some(ms) = rule.after_ms {
                if st.pending.len() >= KEYS_MAX && !st.pending.contains_key(&(i, key_of(Per::Task, ev))) {
                    out.push(self.act(rule, ev, depth, Do::Dropped("too many reactions are waiting".into())));
                } else {
                    let key = key_of(Per::Task, ev);
                    out.push(self.act(rule, ev, depth, Do::Arm { rule: i, key, ms, ev: Box::new(ev.clone()) }));
                }
                continue;
            }
            out.extend(self.fire(st, i, ev, env, now, depth));
        }
        out
    }

    /// A wait is over (see [`State::take_due`]): the action, if the rule's condition still holds as the world is now and it may
    /// still say it. A task that stopped being what the rule was about in the meantime takes the action with it.
    pub fn due(&self, st: &mut State, rule: usize, ev: &Ev, env: &dyn Env, now: u64) -> Option<Act> {
        if self.list.get(rule)?.when.as_ref().is_some_and(|w| !w.holds(env)) {
            return None;
        }
        let depth = st.depth(ev);
        self.fire(st, rule, ev, env, now, depth)
    }

    fn fire(&self, st: &mut State, i: usize, ev: &Ev, env: &dyn Env, now: u64, depth: u8) -> Option<Act> {
        let rule = self.list.get(i)?;
        let key = (i, key_of(rule.limit.as_ref().map_or(Per::Task, |l| l.per), ev));
        if rule.cooldown_ms.is_some_and(|cd| st.last.get(&key).is_some_and(|last| now < last.saturating_add(cd))) {
            return None;
        }
        if let Some(l) = &rule.limit {
            let times = st.fired.entry(key.clone()).or_default();
            if let Some(within) = l.within_ms {
                while times.front().is_some_and(|t| now.saturating_sub(*t) > within) {
                    times.pop_front();
                }
            }
            if times.len() >= l.max as usize {
                return None;
            }
        }
        while st.rate.front().is_some_and(|t| now.saturating_sub(*t) > RATE_WINDOW_MS) {
            st.rate.pop_front();
        }
        if st.rate.len() >= RATE_MAX {
            // said once a window, or the notice would be the storm
            let say = st.noted.is_none_or(|at| now.saturating_sub(at) > RATE_WINDOW_MS);
            st.noted = Some(now);
            return say.then(|| self.act(rule, ev, depth, Do::Dropped(format!("more than {RATE_MAX} reactions in {} seconds", RATE_WINDOW_MS / 1000))));
        }
        let what = match self.say(rule, env) {
            Ok(w) => w,
            Err(why) => return Some(self.act(rule, ev, depth, Do::Dropped(why))),
        };
        if st.fired.len() >= KEYS_MAX || st.last.len() >= KEYS_MAX {
            st.fired.clear();
            st.last.clear();
        }
        st.fired.entry(key.clone()).or_default().push_back(now);
        st.last.insert(key, now);
        st.rate.push_back(now);
        Some(self.act(rule, ev, depth, what))
    }

    /// What the rule does, its words filled in.
    fn say(&self, rule: &Rule, env: &dyn Env) -> Result<Do, String> {
        let words = |f: &Option<Fmt>| f.as_ref().map(|f| clean(&f.render(env), TEXT_MAX)).unwrap_or_default();
        let nonempty = |t: String| if t.is_empty() { Err("its words came out empty".to_string()) } else { Ok(t) };
        match rule.then {
            Then::Start => {
                let keep = rule.keep.iter().map(|k| clean(&k.render(env), NOTE_CHARS)).filter(|k| !k.is_empty()).collect();
                Ok(Do::Start { text: nonempty(words(&rule.text))?, follow: rule.follow, doer: rule.doer.clone(), place: rule.place.clone(), detached: rule.detached, compact: rule.compact, keep })
            }
            Then::Steer => nonempty(words(&rule.text)).map(Do::Steer),
            Then::Notice => nonempty(words(&rule.text)).map(|text| match &rule.alert {
                Some(a) => Do::Alert { incident: a.incident.clone(), like: a.like.clone(), class: a.class.clone(), done: a.done.clone(), source: a.source.clone(), text },
                None => Do::Notice(text),
            }),
            Then::Control => Ok(Do::Park),
            Then::Cool => match rule.cool_for.as_ref().map(|e| e.eval(env)) {
                Some(Value::Dur(ms)) if ms > 0 => Ok(Do::Cool { ms }),
                Some(Value::Num(n)) if n >= 1.0 && n.is_finite() => Ok(Do::Cool { ms: n as u64 }),
                _ => Err("it gave no time to wait".into()),
            },
            Then::Label => {
                let l = rule.label.as_ref().ok_or("it has no label to write")?;
                let (subject, producer) = (words(&Some(l.subject.clone())), words(&Some(l.producer.clone())));
                if [&subject, &producer].iter().any(|w| w.is_empty() || *w == crate::value::DASH) {
                    return Err("what it labels, or who made it, came out empty".into());
                }
                let opt = |f: &Option<Fmt>| words(f);
                Ok(Do::Label { is: l.is.clone(), of: l.of.clone(), subject, producer, model: opt(&l.model), point: opt(&l.point), weight: l.weight })
            }
            Then::Propose => {
                let p = rule.propose.as_ref().ok_or("it has nothing to propose")?;
                let say = |f: &Fmt| clean(&f.render(env), TEXT_MAX);
                let (key, title, why, id) = (say(&p.key), say(&p.title), say(&p.why), say(&p.id));
                if [&key, &title, &why, &id].iter().any(|w| w.is_empty() || **w == crate::value::DASH) {
                    return Err("its key, title, reason or id came out empty".into());
                }
                let evidence = match p.evidence.as_ref().map(|e| e.eval(env)) {
                    Some(Value::Num(n)) if n >= 1.0 && n.is_finite() => n as u64,
                    Some(Value::Num(_)) | Some(Value::Missing) => return Err("it has no evidence yet".into()),
                    _ => 1,
                };
                let op = if p.remove {
                    crate::proposal::StepOp::Remove
                } else if p.set.is_empty() {
                    crate::proposal::StepOp::Unset(p.unset.clone())
                } else {
                    let mut t = toml::Table::new();
                    for (k, val) in &p.set {
                        let v = match val {
                            SetVal::Lit(v) => v.clone(),
                            SetVal::Tpl(f) => toml::Value::String(clean(&f.render(env), TEXT_MAX)),
                            SetVal::Ex(e) => to_toml(&e.eval(env)).ok_or_else(|| format!("`{k}` came out as nothing a file can hold"))?,
                        };
                        t.insert(k.clone(), v);
                    }
                    crate::proposal::StepOp::Set(t)
                };
                let step = crate::proposal::Step { to: p.to, file: p.file.clone(), kind: p.kind.clone(), id, op };
                Ok(Do::Propose(crate::proposal::Draft { key, title, why, evidence, steps: vec![step], widens: Vec::new() }))
            }
            Then::Ask => nonempty(words(&rule.text)).map(|text| Do::Ask { text, run: rule.run.clone() }),
        }
    }
}
